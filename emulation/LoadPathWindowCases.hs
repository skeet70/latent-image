{-# LANGUAGE OverloadedStrings #-}
-- LOAD-WINDOW provenance cases (1.50, host-only).
--
-- These cases observe software stores during storage initialization, the
-- table-block transfer request and the segment-record walk. They do not execute
-- media movement or image decode. The free window is [0x0061c1fc,0x0061c400).
-- reports/filmsim-prefix-widening.md section 5.4 records the historical runs.
--
-- Six cases, all executing pinned 1.50 bytes in the bounded runner:
--
--   load-init-storage    the stock storage initialization FUN_0000325c, which
--                        produces the RAM state the copy path reads (geometry
--                        rows, the ready cell and the buffer pointers).
--   load-window-copy     the stock table-block copy chain FUN_00001dc4 ->
--                        FUN_0000218c -> FUN_0000213c -> FUN_000030d8 ->
--                        FUN_000035c4 -> FUN_00002ab8, started from the state
--                        load-init-storage produced. It returns to the sentinel
--                        with error-valued r0=0xffffff01.
--   load-segment-walk-*  the stock segment-record walk FUN_00004174 over the
--                        installed container's table block, ids 0, 1, 4 and 9.
--
-- Declared inputs, named in each case's notes: the device register windows are
-- ordinary writable memory; the media transfer and the hardware decompressor
-- are not executed.
module Main where

import FujiAF.Host
import FujiAF.JSON (encodeCanonical)
import Control.Monad
import Data.Aeson (Value)
import qualified Data.Aeson as A
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as BC
import Data.Bits
import Data.List (find, foldl')
import Data.Maybe (fromMaybe)
import qualified Data.Text as T
import Data.Word
import Numeric
import System.Directory
import System.Environment
import System.FilePath
import System.IO
import System.Exit (ExitCode(..))

-- ---------------------------------------------------------------------------
-- Pinned inputs (exact SHA-256 required before any case is written).
-- ---------------------------------------------------------------------------

recordPath, containerPath :: FilePath
recordPath = "private-data/analysis/x-m1-1.50/decode/boot-40048.bin"
containerPath = "private-data/analysis/x-m1-1.50/update/prefix/FWUP0002-prefix.DAT"

recordHash, containerHash, tableSliceHash :: String
recordHash = "c2da900dcd3f193b05b5ae968dfcb87723571462eafd84c0fed8467f66f7dff3"
containerHash = "acb0fbbb9507421f48a6254920cafb5ce20e1fd6bf821bc80c912c1f99c29ca8"
tableSliceHash = "bb96a64ef0a0021c3bdd5b03ccd1ddbd17748c94e25dd24876413d3b5517da8e"

-- The container stores media as the complement of the payload export; the boot
-- table-block copy maps media 0x140000 to RAM 0x0061c000, and a DAT offset is
-- media + 0x25c (payload = media + 0x48, DAT = payload + 0x214).
containerToMedia :: Word32 -> Int -> B.ByteString -> B.ByteString
containerToMedia media size dat =
  B.map complement (B.take size (B.drop (fromIntegral (media + 0x25c)) dat))

-- Hashing goes through a temp file, not a stdin pipe: the shared runBounded
-- stdin path returned a different digest for the complemented 1 KiB table slice
-- than sha256sum does on the same bytes as a file argument.
sha256FileBytes :: B.ByteString -> IO String
sha256FileBytes bytes = withTemporary "loadwin-hash" $ \dir -> do
  let path = dir </> "blob"
  B.writeFile path bytes
  (timed, code, out, _) <- runBounded 15000000 1024 "sha256sum" [path] B.empty
  ensure (not timed && code == ExitSuccess && length (BC.unpack out) >= 64) "SHA256 helper failed"
  pure (take 64 (BC.unpack out))

readPinned :: FilePath -> String -> IO (B.ByteString, Value)
readPinned path expected = do
  bytes <- B.readFile path
  digest <- sha256FileBytes bytes
  ensure (digest == expected) (path ++ ": pinned input hash differs: " ++ digest)
  pure (bytes, obj [("path", s path), ("sha256", s digest), ("size", n (fromIntegral (B.length bytes)))])

-- The 38 MiB container is read once, but only the 0x400-byte media table slice
-- is pinned by hash: hashing the whole container through the shared helper is
-- not affordable. The container's own recorded SHA-256 names the install
-- artifact (E-FILMSIM-INSTALL-150).
readTable :: FilePath -> IO (B.ByteString, Value)
readTable path = do
  dat <- B.readFile path
  let slice = containerToMedia 0x140000 0x400 dat
  digest <- sha256FileBytes slice
  ensure (digest == tableSliceHash) (path ++ ": media table slice hash differs: " ++ digest)
  pure (slice, obj
    [ ("path", s path)
    , ("sha256", s containerHash)
    , ("media", s "0x000140000")
    , ("size", n 0x400)
    , ("slice_sha256", s digest)
    ])

tableBase, tableSize, blockSize :: Word32
tableBase = 0x0061c000
tableSize = 0x400
blockSize = 0x20000

windowLo, windowHi :: Word32
windowLo = 0x0061c1fc
windowHi = 0x0061c400

-- The table header carries the 0x40000000 address alias, so the walk reads the
-- block through 0x4061c000 as well.
aliasBase :: Word32
aliasBase = 0x4061c000

exitAddress, stackBase, stackSize :: Word32
exitAddress = 0x3c000000
stackBase = 0x3b000000
stackSize = 0x400

-- ---------------------------------------------------------------------------
-- Function ranges, from the Ghidra decompilation's per-function markers
-- (private-data/analysis/x-m1-1.50/decode/ghidra/boot-40048.c). Consecutive
-- entries tile the record, so the declared ranges never overlap.
-- ---------------------------------------------------------------------------

functionMap :: [(Word32, Word32)]
functionMap =
  [ (0x1800,0x1854),(0x1854,0x18b8),(0x18b8,0x19a8),(0x19a8,0x19e4),(0x19e4,0x1a84)
  , (0x1a84,0x1aa0),(0x1aa0,0x1cc0),(0x1cc0,0x1d08),(0x1d08,0x1dc4),(0x1dc4,0x1de8)
  , (0x1de8,0x1e4c),(0x1e4c,0x2034),(0x2034,0x2044),(0x2044,0x2058),(0x2058,0x209c)
  , (0x209c,0x2118),(0x2118,0x213c),(0x213c,0x218c),(0x218c,0x21a0),(0x21a0,0x21b8)
  , (0x21b8,0x2250),(0x2250,0x231c),(0x231c,0x23bc),(0x23bc,0x25d4),(0x25d4,0x2708)
  , (0x2708,0x2744),(0x2744,0x2784),(0x2784,0x27dc),(0x27dc,0x2874),(0x2874,0x28ac)
  , (0x28ac,0x2970),(0x2970,0x2998),(0x2998,0x2a18),(0x2a18,0x2a44),(0x2a44,0x2a84)
  , (0x2a84,0x2ab8),(0x2ab8,0x2d64),(0x2d64,0x2db8),(0x2db8,0x2e3c),(0x2e3c,0x2fc8)
  , (0x2fc8,0x3038),(0x3038,0x30a0),(0x30a0,0x30bc),(0x30bc,0x30d8),(0x30d8,0x3180)
  , (0x3180,0x31f8),(0x31f8,0x3218),(0x3218,0x325c),(0x325c,0x33e0),(0x33e0,0x3408)
  , (0x3408,0x3464),(0x3464,0x3508),(0x3508,0x3528),(0x3528,0x35ac),(0x35ac,0x35c4)
  , (0x35c4,0x37a0),(0x37a0,0x3844),(0x3844,0x39b0),(0x39b0,0x3b6c),(0x3b6c,0x3b9c)
  , (0x3b9c,0x3bcc),(0x3bcc,0x3c1c),(0x3c1c,0x3c54),(0x3c54,0x3cbc),(0x3cbc,0x3ce4)
  , (0x3ce4,0x3d08),(0x3d08,0x3d3c),(0x3d3c,0x3dc0),(0x3dc0,0x3ea0),(0x3ea0,0x4044)
  , (0x4044,0x4090),(0x4090,0x40b0),(0x40b0,0x4174),(0x4174,0x4300) ]

functionRange :: Word32 -> (Word32, Word32)
functionRange a = fromMaybe (error ("function not mapped: " ++ hex32 a)) (find ((== a) . fst) functionMap)

-- Call closures, read from the record listing. Every BL target a case can
-- reach is declared; the runner refuses a call to anything else.
initClosure, copyClosure, walkClosure :: [Word32]
initClosure = [0x325c,0x2708,0x2a44,0x2a18,0x2744,0x2998,0x3038,0x2970,0x2db8,0x2e3c,0x28ac]
copyClosure = [0x1dc4,0x218c,0x213c,0x209c,0x2118,0x30d8,0x31f8,0x3218,0x35c4,0x2fc8,0x3408,0x3528,0x2ab8,0x28ac,0x2a84,0x2db8,0x2970,0x27dc,0x3508]
walkClosure = [0x4174]

-- ---------------------------------------------------------------------------
-- Case model and wire emission.
-- ---------------------------------------------------------------------------

type Region = (Word32, Int, Int, B.ByteString) -- address, size, permissions, bytes

data Stage = Stage
  { stId :: String
  , stEntry :: Word32
  , stClosure :: [Word32]
  , stRegions :: [Region]
  , stRegisters :: [Word32]
  , stBudget :: Int
  , stNotes :: [String]
  }

hexBytes :: B.ByteString -> String
hexBytes = concatMap (\x -> let t = showHex x "" in replicate (2 - length t) '0' ++ t) . B.unpack

hex32 :: Word32 -> String
hex32 v = let t = showHex v "" in replicate (8 - length t) '0' ++ t

word32le :: Word32 -> B.ByteString
word32le v = B.pack [fromIntegral (v `shiftR` k) | k <- [0,8,16,24]]

wordAt :: B.ByteString -> Int -> Word32
wordAt bytes off = foldr (.|.) 0 [ fromIntegral (B.index bytes (off + k)) `shiftL` (8 * k) | k <- [0 .. 3] ]

patchWord :: Region -> Word32 -> Word32 -> Region
patchWord (base, size, perms, bytes) address value
  | address >= base && fromIntegral (address - base) + 4 <= B.length bytes =
      (base, size, perms, B.take off bytes <> word32le value <> B.drop (off + 4) bytes)
  | otherwise = (base, size, perms, bytes)
  where off = fromIntegral (address - base)

-- Device register windows the boot record touches, declared as ordinary
-- writable memory: the media controller, the clock/companion block, the dma
-- fill engine, the decompressor and the timer.
deviceRegions :: [Region]
deviceRegions =
  [ (0xfff52000, 0x1000, 3, B.replicate 0x1000 0)
  , (0xfff88000, 0x1000, 3, B.replicate 0x1000 0)
  , (0xfff02000, 0x1000, 3, B.replicate 0x1000 0)
  , (0xfff43000, 0x1000, 3, B.replicate 0x1000 0)
  , (0xfff91000, 0x1000, 3, B.replicate 0x1000 0)
  ]

registerVector :: Word32 -> [Word32]
registerVector entry = replicate 13 0 ++ [stackBase + stackSize, exitAddress, 0x10]

wireOf :: Stage -> B.ByteString
wireOf st = BC.pack (unlines header) <> BC.pack (unlines regionLines) <> BC.pack (unlines functionLines)
  where
    header =
      [ "FUJI_NATIVE_V1"
      , "cortex-r5f-arm-le"
      , unwords [hex32 (stEntry st), hex32 exitAddress, show (stBudget st), "10000"]
      , unwords (map hex32 (stRegisters st))
      , unwords [show (length (stRegions st)), show (length (stClosure st)), "0"]
      ]
    regionLines =
      [ unwords [hex32 a, showHex sz "", show p, hexBytes b] | (a, sz, p, b) <- stRegions st ]
    functionLines =
      [ unwords [hex32 a, showHex (end - a) ""] | a <- stClosure st, let (_, end) = functionRange a ]

-- ---------------------------------------------------------------------------
-- Result queries over the runner's report.
-- ---------------------------------------------------------------------------

field :: Value -> String -> Value
field = get

ru :: Value -> String -> String
ru result k = str (field result k)

-- The runner reports registers under "registers".
rr :: Value -> String -> String
rr result k = str (field (field result "registers") k)

provenance :: Value -> [Value]
provenance result = list (field result "memory_provenance")

provenanceTotal :: Value -> Integer
provenanceTotal result = num (field result "memory_provenance_total")

addressOf, pcOf :: Value -> Maybe Word32
addressOf p = hexText (field p "address")
pcOf p = hexText (field p "pc")

hexText :: Value -> Maybe Word32
hexText v = case str v of
  ('0':'x':rest) -> case readHex rest of [(x, "")] -> Just x; _ -> Nothing
  _ -> Nothing

writeValueAt :: Value -> Word32 -> Word32 -> Maybe Word32
writeValueAt result pc address =
  case [ num (field p "value") | p <- provenance result
       , str (field p "kind") == "write"
       , addressOf p == Just address
       , pcOf p == Just pc ] of
    (x:_) -> Just (fromIntegral x)
    [] -> Nothing

writesInRange :: Value -> Word32 -> Word32 -> [Value]
writesInRange result lo hi =
  [ p | p <- provenance result
      , str (field p "kind") == "write"
      , Just a <- [addressOf p], a >= lo, a < hi ]

allWrites :: Value -> [Value]
allWrites result = [ p | p <- provenance result, str (field p "kind") == "write" ]

finalRegion :: Value -> Word32 -> Maybe B.ByteString
finalRegion result address =
  case find (\r -> str (field r "address") == "0x" ++ hex32 address) (list (field result "memory")) of
    Nothing -> Nothing
    Just rec -> case field rec "bytes" of
      A.String t -> Just (B.pack (unpackHex (T.unpack t)))
      _ -> Nothing
  where
    unpackHex = go
    go (a:b:rest) = fromIntegral (digit a * 16 + digit b) : go rest
    go _ = []
    digit c | c >= '0' && c <= '9' = fromEnum c - fromEnum '0'
            | c >= 'a' && c <= 'f' = fromEnum c - fromEnum 'a' + 10
            | otherwise = 0

lastLine :: B.ByteString -> B.ByteString
lastLine = last . filter (not . B.null) . BC.lines

expect :: Bool -> String -> IO ()
expect yes message = ensure yes (message ++ "; case files retained")

-- ---------------------------------------------------------------------------
-- Case files.
-- ---------------------------------------------------------------------------

emitCase :: FilePath -> Stage -> Value -> Value -> String -> IO ()
emitCase out st artifacts result wireHash = do
  let caseDir = out
      input = obj
        [ ("classification", s "emulation")
        , ("id", s (stId st))
        , ("entry", s ("0x" ++ hex32 (stEntry st)))
        , ("registers", arr (map (s . ("0x" ++) . hex32) (stRegisters st)))
        , ("regions", arr
            [ obj [ ("address", s ("0x" ++ hex32 a))
                  , ("size", n (fromIntegral sz))
                  , ("permissions", s (if p == 5 then "rx" else "rw"))
                  , ("hex", s (hexBytes b)) ]
            | (a, sz, p, b) <- stRegions st ])
        , ("functions", arr
            [ obj [ ("address", s ("0x" ++ hex32 a)), ("size", n (fromIntegral (end - a))) ]
            | a <- stClosure st, let (_, end) = functionRange a ])
        , ("artifacts", artifacts)
        , ("notes", arr (map s (stNotes st)))
        ]
      report = obj
        [ ("classification", s "emulation")
        , ("id", s (stId st))
        , ("wire_sha256", s wireHash)
        , ("input", input)
        , ("runner", result)
        ]
  createDirectoryIfMissing True caseDir
  B.writeFile (caseDir </> stId st <.> "wire") (wireOf st)
  B.writeFile (caseDir </> stId st <.> "input.json") (encodeCanonical input)
  B.writeFile (caseDir </> stId st <.> "result.json") (encodeCanonical report)

-- ---------------------------------------------------------------------------
-- Case 1: stock storage initialization.
-- ---------------------------------------------------------------------------

dataCase :: B.ByteString -> Stage
dataCase record =
  Stage
    { stId = "load-init-storage"
    , stEntry = 0x325c
    , stClosure = initClosure
    , stRegions =
        [ (0x0, 0x4000, 5, B.take 0x4000 record)
        , (0x4000, 0x300, 3, B.take 0x300 (B.drop 0x4000 record))
        , (0x4300, 0x1000, 3, B.replicate 0x1000 0)
        , (stackBase, fromIntegral stackSize, 3, B.replicate (fromIntegral stackSize) 0)
        ]
        ++ deviceRegions
        ++ [ (exitAddress, 4, 5, B.replicate 4 0) ]
    , stRegisters = registerVector 0x325c
    , stBudget = 200000
    , stNotes =
        [ "stock storage initialization FUN_0000325c; its final memory is the declared input of load-window-copy"
        , "device register windows are writable case memory, not device semantics"
        ]
    }

runInit :: FilePath -> FilePath -> B.ByteString -> Value -> IO (Value, [Region])
runInit out runner record artifacts = do
  let st = dataCase record
      wire = wireOf st
  (timed, _, raw, _) <- runBounded 30000000 (8*1024*1024) runner [] wire
  expect (not timed) (stId st ++ ": host timeout")
  result <- jsonRead (lastLine raw)
  wireHash <- sha256 wire
  emitCase out st artifacts result wireHash
  expect (ru result "status" == "returned") (stId st ++ ": expected returned, got " ++ ru result "status" ++ " at " ++ rr result "pc")
  expect (provenanceTotal result <= 4096) (stId st ++ ": provenance log truncated")
  expect (null (writesInRange result tableBase (tableBase + blockSize))) (stId st ++ ": unexpected write inside the table block")
  expect (writeValueAt result 0x33c0 0x4a08 == Just 3) (stId st ++ ": geometry selector write absent")
  expect (writeValueAt result 0x33d4 0x4800 == Just 1) (stId st ++ ": storage-ready publication absent")
  expect (writeValueAt result 0x3358 0x4288 == Just 0x400) (stId st ++ ": geometry row-3 limit absent")
  expect (writeValueAt result 0x3284 0x47fc == Just 0x4a20) (stId st ++ ": media buffer pointer absent")
  putStrLn (stId st ++ ": " ++ ru result "status" ++ ", " ++ ru result "instructions" ++ " instructions, "
            ++ show (provenanceTotal result) ++ " provenance records, none in the table block")
  let carried = [ (a, sz, p, fromMaybe (B.replicate sz 0) (finalRegion result a))
                | (a, sz, p, _) <- stRegions st ]
  pure (result, carried)

-- ---------------------------------------------------------------------------
-- Case 2: the table-block copy chain from that state.
-- ---------------------------------------------------------------------------

copyCase :: [Region] -> Stage
copyCase carriage =
  Stage
    { stId = "load-window-copy"
    , stEntry = 0x1dc4
    , stClosure = copyClosure
    , stRegions = foldl' (\rs (a, v) -> map (\r -> patchWord r a v) rs) carriage
        [ (0x4804, 0x4310)               -- empty media skip list
        , (0x4310, 0)
        , (0xfff52404, 0x0000ffff) ]     -- declared ready read status
        ++ [ (tableBase, fromIntegral blockSize, 3, B.replicate (fromIntegral blockSize) 0) ]
    , stRegisters = registerVector 0x1dc4
    , stBudget = 200000
    , stNotes =
        [ "stage 2: the stock table-block copy chain from the load-init-storage state"
        , "declared device inputs beyond the carried state: an empty media skip list at 0x4310 and the ready read-status word 0xfff52404 = 0x0000ffff"
        , "the destination range [0x61c000,0x63c000) is declared writable and starts cleared"
        , "the transfer core 0x2ab8 executes descriptor assembly and controller request stores; the chain returns to the sentinel with error-valued r0=0xffffff01 and no media payload movement"
        ]
    }

runCopy :: FilePath -> FilePath -> [Region] -> Value -> IO Value
runCopy out runner carriage artifacts = do
  let st = copyCase carriage
      wire = wireOf st
  (timed, _, raw, _) <- runBounded 30000000 (8*1024*1024) runner [] wire
  expect (not timed) (stId st ++ ": host timeout")
  result <- jsonRead (lastLine raw)
  wireHash <- sha256 wire
  emitCase out st artifacts result wireHash
  expect (ru result "status" == "returned") (stId st ++ ": expected returned, got " ++ ru result "status" ++ " at " ++ rr result "pc")
  expect (rr result "pc" == "0x" ++ hex32 exitAddress) (stId st ++ ": stop pc " ++ rr result "pc")
  expect (rr result "r0" == "0xffffff01") (stId st ++ ": expected error-valued r0=0xffffff01, got " ++ rr result "r0")
  expect (provenanceTotal result <= 4096) (stId st ++ ": provenance log truncated")
  let windowWrites = writesInRange result windowLo windowHi
      blockWrites = writesInRange result tableBase (tableBase + blockSize)
  expect (null blockWrites) (stId st ++ ": unexpected write inside the table block: " ++ show (length blockWrites))
  expect (null windowWrites) (stId st ++ ": unexpected write inside [0x61c1fc,0x61c400): " ++ show (length windowWrites))
  expect (writeValueAt result 0x30d8 0x3b0003b4 == Just tableBase) (stId st ++ ": descriptor destination absent")
  expect (writeValueAt result 0x311c 0x3b000398 == Just 1) (stId st ++ ": descriptor unit count absent")
  expect (writeValueAt result 0x2b88 0xfff528a4 == Just tableBase) (stId st ++ ": transfer-core destination store absent")
  expect (writeValueAt result 0x2ba4 0xfff5201c == Just 0x300100) (stId st ++ ": transfer-core count store absent")
  expect (writeValueAt result 0x2b9c 0xfff52884 == Just 0x10001) (stId st ++ ": transfer-core start store absent")
  expect (writeValueAt result 0x2c50 0xfff52884 == Just 0x10000) (stId st ++ ": transfer-core start clear absent")
  putStrLn (stId st ++ ": " ++ ru result "status" ++ " at " ++ rr result "pc" ++ ", error-valued r0=" ++ rr result "r0" ++ ", " ++ ru result "instructions"
            ++ " instructions, " ++ show (provenanceTotal result) ++ " provenance records, "
            ++ show (length windowWrites) ++ " in [0x61c1fc,0x61c400)")
  pure result

-- ---------------------------------------------------------------------------
-- Case 3: the segment-record walk over the installed table block.
-- ---------------------------------------------------------------------------

walkStage :: B.ByteString -> B.ByteString -> Word32 -> Stage
walkStage record table id0 =
  Stage
    { stId = "load-segment-walk-" ++ show id0
    , stEntry = 0x4174
    , stClosure = walkClosure
    , stRegions =
        [ (0x4174, 0x18c, 5, B.take 0x18c (B.drop 0x4174 record))
        , (tableBase, fromIntegral tableSize, 3, table)
        , (aliasBase, fromIntegral tableSize, 3, table)
        , (stackBase, fromIntegral stackSize, 3, B.replicate (fromIntegral stackSize) 0)
        , (exitAddress, 4, 5, B.replicate 4 0)
        ]
    , stRegisters = case registerVector 0x4174 of (x:rest) -> id0 : rest; [] -> []
    , stBudget = 20000
    , stNotes =
        [ "stock segment-record walk FUN_00004174 over the installed container's table block"
        , "the block region is declared at both 0x0061c000 and its 0x4061c000 alias, which the header pointer uses"
        ]
    }

-- Expected return: the entry whose first word equals the id, walked in 9-word
-- steps from the header pointer; 0 after the 0xffffffff terminator.
expectedWalk :: B.ByteString -> Word32 -> Word32
expectedWalk table id0
  | entryId header == id0 = header
  | otherwise = search (header + 0x24)
  where
    header = wordAt table 0
    entryId a = wordAt table (fromIntegral (a - aliasBase))
    search a
      | entryId a == id0 = a
      | entryId a == 0xffffffff = 0
      | otherwise = search (a + 0x24)

runWalk :: FilePath -> FilePath -> B.ByteString -> B.ByteString -> Word32 -> Value -> IO ()
runWalk out runner record table id0 artifacts = do
  let st = walkStage record table id0
      wire = wireOf st
  (timed, _, raw, _) <- runBounded 30000000 (8*1024*1024) runner [] wire
  expect (not timed) (stId st ++ ": host timeout")
  result <- jsonRead (lastLine raw)
  wireHash <- sha256 wire
  emitCase out st artifacts result wireHash
  let expected = expectedWalk table id0
  expect (ru result "status" == "returned") (stId st ++ ": expected returned, got " ++ ru result "status" ++ " at " ++ rr result "pc")
  expect (provenanceTotal result <= 4096) (stId st ++ ": provenance log truncated")
  expect (null (writesInRange result tableBase (tableBase + tableSize))) (stId st ++ ": unexpected write in the table block")
  expect (all (\p -> maybe False (\a -> a >= stackBase && a < stackBase + stackSize) (addressOf p)) (allWrites result)) (stId st ++ ": writes outside the stack frame")
  expect (rr result "r0" == "0x" ++ hex32 expected) (stId st ++ ": walk returned " ++ rr result "r0" ++ ", expected 0x" ++ hex32 expected)
  putStrLn (stId st ++ ": returned 0x" ++ hex32 expected ++ " for id " ++ show id0 ++ ", "
            ++ ru result "instructions" ++ " instructions, no writes")

main :: IO ()
main = do
  hSetBuffering stdout LineBuffering
  [runner, out] <- getArgs
  (record, recordIdentity) <- readPinned recordPath recordHash
  putStrLn ("pinned record " ++ take 16 recordHash)
  (table, tableIdentity) <- readTable containerPath
  putStrLn ("pinned table slice " ++ take 16 tableSliceHash)
  runnerDigest <- B.readFile runner >>= sha256FileBytes
  putStrLn ("hashed runner " ++ take 16 runnerDigest)
  let artifacts = arr [recordIdentity, tableIdentity]
  putStrLn ("record " ++ take 16 recordHash ++ ", container " ++ take 16 containerHash ++ ", table slice " ++ take 16 tableSliceHash ++ ", runner " ++ take 16 runnerDigest)
  (_initResult, carriage) <- runInit out runner record artifacts
  _copyResult <- runCopy out runner carriage artifacts
  forM_ [0, 1, 4, 9 :: Word32] $ \i -> runWalk out runner record table i artifacts
  putStrLn "all load-window cases passed their expectations"
