{-# LANGUAGE OverloadedStrings #-}

-- | save-file-machine-suite RUNNER LOADER_A IMAGE4 IMAGE5 PLAIN OWNED_ELF EXIF_ELF OUTPUT CONTROL=ELF ...
--
-- Runs each scenario of "Replacement.SaveFileMachine.Cases" natively against overlay 46 as the
-- plain image holds it, and against one overlay-46 carrier holding both owned units: the Exif
-- writer ELF over its extents and the save file machine ELF (owned, or a deliberately wrong
-- control) over its extent. The stock image-4 registry registers, posts and dispatches; its
-- kernel send appends each message to a queue the driver drains after every call. File-service,
-- report, naming, property and kernel calls are stubs that log their arguments, stack addresses
-- replaced by a marker; the string and memory services run from their stock bytes behind a
-- recorder, and the completion is stock image-5 0x00e4e5dc. The stock role runs once to fix the
-- expectation; equal requires the stock and owned runs to pass with equal logs, every control to
-- fail a case after reaching the exit, and every stock branch direction reached or declared
-- unreachable.
module Main (main) where

import qualified ArmElf as E
import Control.Monad (forM, forM_, unless, when)
import Data.Aeson (Value (..), encode, object, (.=))
import qualified Data.Aeson.Key as Key
import qualified Data.Aeson.KeyMap as KM
import Data.Bits (shiftL, shiftR, (.&.), (.|.))
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as BL
import Data.Char (digitToInt)
import Data.IORef (modifyIORef', newIORef, readIORef)
import Data.List (nub)
import Data.Maybe (fromMaybe)
import qualified Data.Text as T
import qualified Data.Vector as V
import Data.Word (Word32)
import qualified Emulation.Coverage as Coverage
import qualified Emulation.NativeCase as Native
import Emulation.Sha256 (sha256, sha256Hex)
import qualified Replacement.ExifWriter.Cases as Exif
import Replacement.FileService.Native (Extent (..), extentContains, hexWord, word32le, words32le)
import Replacement.SaveFileMachine.Cases
import System.Directory (createDirectoryIfMissing, doesPathExist)
import System.Environment (getArgs, lookupEnv)
import System.Exit (exitFailure)
import System.FilePath ((</>))

driverBase, resultsBase, stackBase, stackTop, exitAddress, stubBase, callLog, queueBase, trapWord :: Word32
driverBase = 0x300000
resultsBase = 0x301000
stackBase = 0x302000
stackTop = 0x304000
exitAddress = 0x305000
stubBase = 0x200000
callLog = 0x210000
queueBase = 0x220000
trapWord = 0xe7f000f0

callLogBytes, queueBytes :: Int
callLogBytes = 0x4000
queueBytes = 8 + 16 * 64

overlay :: Extent
overlay = Extent 0x011c0000 0x011dcca0

-- | Registry headers and slot arrays of workers 1 and 2.
saveRegistry, cancelRegistry :: (Word32, Word32)
saveRegistry = (0x0096e058, 0x0095769c)
cancelRegistry = (0x0096e064, 0x0095771c)

workerHeaders :: [(Word32, Word32)]
workerHeaders = [saveRegistry, cancelRegistry]

dispatchEntry, registryInit :: Word32
dispatchEntry = 0x006e2aa0
registryInit = 0x006e2390

data Role = Stock | Owned String E.Elf

roleLabel :: Role -> String
roleLabel Stock = "stock"
roleLabel (Owned label _) = label

-- Encoders ---------------------------------------------------------------------

movw, movt :: Word32 -> Word32 -> Word32
movw register value = 0xe3000000 .|. ((value .&. 0xf000) `shiftL` 4) .|. (register `shiftL` 12) .|. (value .&. 0xfff)
movt register value =
  0xe3400000 .|. (((value `shiftR` 16) .&. 0xf000) `shiftL` 4) .|. (register `shiftL` 12) .|. ((value `shiftR` 16) .&. 0xfff)

-- | MOVW and MOVT of @value@ under condition @condition@.
load :: Word32 -> Word32 -> [Word32]
load register value = [movw register value, movt register value]

conditional :: Word32 -> Word32 -> Word32
conditional condition instruction = (instruction .&. 0x0fffffff) .|. (condition `shiftL` 28)

branch, branchLink :: Word32 -> Word32 -> Word32
branch pc target = 0xea000000 .|. (((target - pc - 8) `shiftR` 2) .&. 0xffffff)
branchLink pc target = 0xeb000000 .|. (((target - pc - 8) `shiftR` 2) .&. 0xffffff)

ldrImm, strImm, ldrbImm :: Word32 -> Word32 -> Word32 -> Word32
ldrImm rd rn offset = 0xe5900000 .|. (rn `shiftL` 16) .|. (rd `shiftL` 12) .|. offset
strImm rd rn offset = 0xe5800000 .|. (rn `shiftL` 16) .|. (rd `shiftL` 12) .|. offset
ldrbImm rd rn offset = 0xe5d00000 .|. (rn `shiftL` 16) .|. (rd `shiftL` 12) .|. offset

-- | @str rd, [r5], #4@: append to the log cursor.
logRegister :: Word32 -> Word32
logRegister rd = 0xe4856004 .|. (rd `shiftL` 12)

-- | Appends @rn@ to the log, a value inside the stack replaced by @0xffffffff@.
logMasked :: Word32 -> [Word32]
logMasked rn =
  load 6 stackBase
    ++ [ 0xe0406006 .|. (rn `shiftL` 16)
       , 0xe3560a02
       , 0x33e06000
       , 0x21a06000 .|. rn
       , logRegister 6
       ]

-- Stubs ------------------------------------------------------------------------------------

data Extra = NoExtra | Payload | PrefixBytes

data Return = ReturnZero | ReturnWord Word32 | ReturnArgumentPlus Word32

-- | A logged stand-in: name, address it replaces (none for the pending call), logged argument
-- registers, whether the fifth argument is logged, extra logging and its return.
data Stub = Stub {stubName :: String, stubAddress :: Word32, stubArguments :: Word32, stubStackArgument :: Bool, stubExtra :: Extra, stubReturn :: Return}

stubs :: [Stub]
stubs =
  [ Stub "current-task" 0x723cb8 0 False NoExtra (ReturnWord 0x0a)
  , Stub "send" 0x72503c 1 False Payload ReturnZero
  , Stub "set-flag" 0x724558 2 False NoExtra ReturnZero
  , Stub "open" 0x71c648 3 False NoExtra (ReturnWord openHandle)
  , Stub "write" 0x71c75c 4 False NoExtra ReturnZero
  , Stub "close" 0x71c7bc 4 False NoExtra ReturnZero
  , Stub "seek" 0x71c8d8 4 True NoExtra ReturnZero
  , Stub "release" 0x71c938 2 False NoExtra ReturnZero
  , Stub "delete" 0x70f430 2 False NoExtra ReturnZero
  , Stub "report" 0x726c74 2 False NoExtra ReturnZero
  , Stub "prepare-name" 0x70fbfc 3 False PrefixBytes ReturnZero
  , Stub "prefix-mode" 0x70f790 1 False NoExtra ReturnZero
  , Stub "property" 0x69f8fc 1 False NoExtra (ReturnArgumentPlus 0x11)
  , Stub "pending" pendingCall 0 False NoExtra ReturnZero
  ]

stubAt :: Int -> Word32
stubAt index = stubBase + 0x100 * fromIntegral index

-- | Logs the stub's address and arguments, runs its extra logging and returns its value.
stubCode :: Stub -> [Word32]
stubCode stub =
  [0xe92d41f0] ++ load 4 callLog ++ [ldrImm 5 4 0] ++ load 6 (stubAddress stub) ++ [logRegister 6]
    ++ concat [logMasked register | register <- take (fromIntegral (stubArguments stub)) [0 .. 3]]
    ++ (if stubStackArgument stub then ldrImm 7 13 24 : logMasked 7 else [])
    ++ extra (stubExtra stub)
    ++ [strImm 5 4 0]
    ++ returnValue (stubReturn stub)
    ++ [0xe8bd81f0]
  where
    extra NoExtra = []
    extra Payload =
      [ldrImm 8 1 8]
        ++ concat [[ldrImm 6 8 (4 * i), logRegister 6] | i <- [0 .. 3]]
        ++ load 7 queueBase
        ++ [ ldrImm 6 7 4
           , 0xe2861001
           , strImm 1 7 4
           , 0xe0876206
           , 0xe2866008
           ]
        ++ concat [[ldrImm 0 8 (4 * i), strImm 0 6 (4 * i)] | i <- [0 .. 3]]
    extra PrefixBytes =
      [ 0xe3520000
      , conditional 1 (ldrImm 6 2 0)
      , 0x03a06000
      , logRegister 6
      , conditional 1 (ldrbImm 6 2 4)
      , 0x03a06000
      , logRegister 6
      ]
    returnValue ReturnZero = [0xe3a00000]
    returnValue (ReturnWord value) = load 0 value
    returnValue (ReturnArgumentPlus value) = [0xe2800000 .|. value]

-- | An instruction word, or a BL whose offset the assembler resolves.
data Item = W Word32 | BL Word32

assemble :: Word32 -> [Item] -> [Word32]
assemble base items = zipWith place [base, base + 4 ..] items
  where
    place _ (W value) = value
    place pc (BL target) = branchLink pc target

-- | Dispatches each queued message to the registry of the worker its queue byte names (0x1b:
-- worker 2, otherwise worker 1) until the read index meets the write index.
drainItems :: Word32 -> [Item]
drainItems at = prefix ++ [W (branch (at + 4 * fromIntegral (length prefix)) (at + 12))]
  where
    prefix =
      map W
        ( [0xe92d4070] ++ load 4 queueBase
            ++ [ ldrImm 0 4 0
               , ldrImm 1 4 4
               , 0xe1500001
               , 0x08bd8070
               , 0xe2801001
               , strImm 1 4 0
               , 0xe0841200
               , 0xe2811008
               , ldrbImm 2 1 3
               ]
            ++ load 0 (fst saveRegistry)
            ++ [0xe352001b]
            ++ map (conditional 0) (load 0 (fst cancelRegistry))
        )
        ++ [BL dispatchEntry]

-- | Initialises both workers' registries, makes each step's call (stack arguments spilled, r0
-- stored to the next result slot when the step returns) and drains the queue after each.
driverWords :: [Step] -> [Word32]
driverWords steps = assemble driverBase (body ++ drainItems drainAt)
  where
    body = [W 0xe92d4ff8] ++ initialise ++ concat (zipWith step [0 ..] steps) ++ [W 0xe8bd8ff8]
    drainAt = driverBase + 4 * fromIntegral (length body)
    initialise = concat [map W (load 0 header ++ [0xe3a01010] ++ load 2 slots) ++ [BL registryInit] | (header, slots) <- workerHeaders]
    slotOf index = fromIntegral (length (filter stepReturns (take index steps))) :: Word32
    step index (Step entry arguments stack returns) =
      let frame = 8 * ((fromIntegral (length stack) + 1) `div` 2) :: Word32
          reserve = [0xe24dd000 .|. frame | not (null stack)]
          spill = concat [load 12 value ++ [0xe58dc000 .|. (4 * i)] | (i, value) <- zip [0 ..] stack]
          loads = concat [load register value | (register, value) <- zip [0 ..] arguments]
          release = [0xe28dd000 .|. frame | not (null stack)]
          store = if returns then load 12 resultsBase ++ [strImm 0 12 (4 * slotOf index)] else []
       in map W (reserve ++ spill ++ loads) ++ [BL entry] ++ map W (release ++ store) ++ [BL drainAt]

-- | Appends the service address and its first @arguments@ registers to the log, each register
-- that names the stack replaced by @0xffffffff@, then runs the displaced first instruction and
-- continues at the service's second instruction.
recorder :: Word32 -> Word32 -> Word32 -> Word32 -> [Word32]
recorder at service displaced arguments =
  body ++ [displaced, branch (at + 4 * fromIntegral (length body) + 4) (service + 4)]
  where
    body =
      [0xe92d0070] ++ load 4 callLog ++ [ldrImm 5 4 0] ++ load 6 service ++ [logRegister 6]
        ++ concat [logMasked register | register <- take (fromIntegral arguments) [0 .. 3]]
        ++ [strImm 5 4 0, 0xe8bd0070]

-- Images --------------------------------------------------------------------------------------

data Loaded = Loaded Word32 B.ByteString

slice :: [(String, Loaded)] -> String -> Word32 -> Word32 -> B.ByteString
slice loaded label low high = case lookup label loaded of
  Just (Loaded base bytes) | low >= base && fromIntegral (high - base) <= B.length bytes -> B.take (fromIntegral (high - low)) (B.drop (fromIntegral (low - base)) bytes)
  _ -> error ("no " ++ label ++ " bytes for " ++ hexWord low)

wordOf :: B.ByteString -> Word32
wordOf bytes = foldr (\index acc -> acc `shiftL` 8 .|. fromIntegral (B.index bytes index)) 0 [0 .. 3]

ownedBytes :: E.Elf -> Extent -> B.ByteString
ownedBytes elf extent@(Extent low high) = B.pack [byte address | address <- [low .. high - 1]]
  where
    sections = [s | s <- E.elfSections elf, E.sectionAllocated s, E.sectionSize s > 0, extentContains extent (E.sectionAddress s)]
    byte address = case [value | s <- sections, E.sectionAddress s <= address, address < E.sectionAddress s + E.sectionSize s, Just value <- [E.byteAt elf address]] of
      (value : _) -> value
      [] -> B.index (word32le trapWord) (fromIntegral (address .&. 3))

-- | Overlay 46: stock, or the Exif writer ELF over its extents and @machine@ over the machine's.
overlayBytes :: [(String, Loaded)] -> E.Elf -> Role -> B.ByteString
overlayBytes loaded exif role = case role of
  Stock -> stock
  Owned _ machine -> foldl patch stock ([(exif, extent) | (_, extent) <- Exif.ownedExtents] ++ [(machine, ownedExtent)])
  where
    Extent low high = overlay
    stock = slice loaded "plain" low high
    patch bytes (elf, extent@(Extent from to)) =
      let at = fromIntegral (from - low)
       in B.take at bytes <> ownedBytes elf extent <> B.drop (at + fromIntegral (to - from)) bytes

ownedFunctions :: E.Elf -> [(Word32, Word32)]
ownedFunctions elf =
  nub
    [ (E.symbolValue symbol, E.symbolSize symbol)
    | symbol <- E.elfSymbols elf
    , E.symbolKind symbol == E.FunctionSymbol
    , E.symbolSize symbol > 0
    , extentContains ownedExtent (E.symbolValue symbol)
    ]

-- Case documents ----------------------------------------------------------------------------

data Region = Region String Word32 String B.ByteString

-- | A stock code range that runs, the recorded services inside it (entry, logged arguments) and
-- its function extents.
data Code = Code {codeName :: String, codeImage :: String, codeLow :: Word32, codeHigh :: Word32, codeRecorded :: [(Word32, Word32)], codeFunctions :: [(Word32, Word32)]}

stockCode :: [Code]
stockCode =
  [ Code "modulo" "loader-a" 0x4d978 0x4da8c [] [(0x4d978, 0xf4), (0x4da6c, 0x20)]
  , Code "registry" "image4" 0x6e2390 0x6e2bf0 [] [(0x6e2390, 0x860)]
  , Code "strings" "image4" 0x721b04 0x721bdc [(0x721b04, 2), (0x721bac, 2)] [(0x721b04, 0x3c), (0x721bac, 0x30)]
  , Code "memory" "image4" 0x722054 0x722218 [(0x722054, 3), (0x7221c8, 3)] [(0x722054, 0x14c), (0x7221a0, 0x28), (0x7221c8, 0x50)]
  , Code "queue-ids" "image4" 0x7d088c 0x7d0890 [] []
  , Code "completion" "image5" 0xe4e5dc 0xe4e644 [] [(0xe4e5dc, 0x68)]
  ]

recorderAt :: Int -> Word32
recorderAt index = stubAt (length stubs + index)

recorded :: [(Word32, Word32)]
recorded = concatMap codeRecorded stockCode

serviceRegions :: [(String, Loaded)] -> ([Region], [(Word32, Word32)])
serviceRegions loaded = (code ++ stubRegions ++ [Region "stubs" stubBase "rx" stubBytes], functions)
  where
    placed = zip [recorderAt i | i <- [0 ..]] recorded
    redirect low bytes = foldl (redirectOne low) bytes placed
    redirectOne low bytes (at, (entry, _))
      | entry >= low && fromIntegral (entry - low) < B.length bytes =
          let o = fromIntegral (entry - low) in B.take o bytes <> word32le (branch entry at) <> B.drop (o + 4) bytes
      | otherwise = bytes
    code = [Region (codeName c) (codeLow c) "rx" (redirect (codeLow c) (slice loaded (codeImage c) (codeLow c) (codeHigh c))) | c <- stockCode]
    stubRegions = [Region ("stub-" ++ stubName s) (stubAddress s) "rx" (word32le (branch (stubAddress s) (stubAt i))) | (i, s) <- zip [0 ..] stubs]
    padded ws = take 64 (ws ++ repeat trapWord)
    displaced entry = wordOf (slice loaded (imageOf entry) entry (entry + 4))
    imageOf entry = case [codeImage c | c <- stockCode, codeLow c <= entry, entry < codeHigh c] of
      (label : _) -> label
      [] -> error ("no code range holds " ++ hexWord entry)
    stubBytes =
      words32le
        ( concat [padded (stubCode s) | s <- stubs]
            ++ concat [padded (recorder at entry (displaced entry) arguments) | (at, (entry, arguments)) <- placed]
        )
    split (low, size)
      | any (\(_, (entry, _)) -> entry == low) placed = [(low, 4), (low + 4, size - 4)]
      | otherwise = [(low, size)]
    functions =
      concatMap (concatMap split . codeFunctions) stockCode
        ++ [(stubAddress s, 4) | s <- stubs]
        ++ [(stubAt i, 0x100) | i <- [0 .. length stubs + length recorded - 1]]

data Expectation = Expectation {expectedRegions :: [(String, B.ByteString)], expectedResults :: [Word32]}

initialBlocks :: Scenario -> [Block]
initialBlocks scenario = [Block name address (foldl (apply address) bytes (scenarioPatches scenario)) | Block name address bytes <- blocks]
  where
    apply base bytes (at, patch)
      | at >= base && fromIntegral (at - base) + B.length patch <= B.length bytes =
          let o = fromIntegral (at - base) in B.take o bytes <> patch <> B.drop (o + B.length patch) bytes
      | otherwise = bytes

writableRegions :: Scenario -> [Region]
writableRegions scenario =
  [Region name address "rw" bytes | Block name address bytes <- initialBlocks scenario]
    ++ [ Region "log" callLog "rw" (word32le (callLog + 4) <> B.replicate (callLogBytes - 4) 0)
       , Region "queue" queueBase "rw" (B.replicate queueBytes 0)
       , Region "results" resultsBase "rw" (B.replicate 0x100 0)
       ]

preserved :: [(String, Word32)]
preserved = [("r" ++ show index, 0x10101010 * fromIntegral index) | index <- [4 :: Int .. 11]]

registers :: [(String, Word32)] -> Value
registers pairs = object [Key.fromString name .= hexWord value | (name, value) <- pairs]

caseDocument :: [(String, Loaded)] -> E.Elf -> Role -> Scenario -> Expectation -> (String, Value, B.ByteString)
caseDocument loaded exif role scenario expectation = (ident, document, blob)
  where
    ident = scenarioName scenario ++ "-" ++ (case role of Stock -> "stock"; Owned "owned" _ -> "owned"; Owned _ _ -> "control")
    (serviceCode, serviceFunctions) = serviceRegions loaded
    driver = words32le (driverWords (scenarioSteps scenario))
    Extent overlayLow _ = overlay
    writable = writableRegions scenario
    regions =
      [Region "overlay46" overlayLow "rx" (overlayBytes loaded exif role)]
        ++ serviceCode
        ++ writable
        ++ [ Region "driver" driverBase "rx" driver
           , Region "stack" stackBase "rw" (B.replicate (fromIntegral (stackTop - stackBase)) 0xa5)
           , Region "exit" exitAddress "rx" (word32le 0xe12fff1e)
           ]
    blob = B.concat [bytes | Region _ _ _ bytes <- regions]
    blobName = sha256Hex (sha256 blob)
    blobId = "a-" ++ take 40 blobName
    offsets = scanl (+) 0 [B.length bytes | Region _ _ _ bytes <- regions]
    machineFunctions = case role of
      Stock -> stockFunctions
      Owned _ elf -> ownedFunctions elf
    initial = [("r" ++ show index, 0x0e0e0e00 + fromIntegral index) | index <- [0 :: Int .. 3]] ++ preserved ++ [("r12", 0x0c0c0c0c), ("sp", stackTop), ("lr", exitAddress), ("cpsr", 0x10)]
    expectedMemory =
      [ object ["region" .= name, "offset" .= (0 :: Int), "hex" .= Native.hexBytes bytes]
      | Region name _ _ _ <- writable
      , name /= "results"
      , Just bytes <- [lookup name (expectedRegions expectation)]
      ]
        ++ [ object ["region" .= ("results" :: String), "offset" .= (0 :: Int), "hex" .= Native.hexBytes (words32le (expectedResults expectation))]
           | not (null (expectedResults expectation))
           ]
    document =
      object
        [ "schema_version" .= (1 :: Int)
        , "id" .= ident
        , "origin" .= (case role of Stock -> "stock"; Owned _ _ -> "synthetic" :: String)
        , "provenance" .= (scenarioProvenance scenario ++ " Role " ++ roleLabel role ++ "; X-M1 1.50 plain image overlay 46, image 4 registry and string services, image 5 completion, loader A division.")
        , "cpu" .= object ["architecture" .= ("arm" :: String), "model" .= ("cortex-r5f" :: String), "mode" .= ("arm" :: String), "endian" .= ("little" :: String)]
        , "artifacts" .= [object ["id" .= blobId, "path" .= (blobName ++ ".bin"), "size" .= B.length blob, "sha256" .= blobName]]
        , "entry" .= hexWord driverBase
        , "exit" .= hexWord exitAddress
        , "registers" .= registers initial
        , "regions" .=
            [ object
                [ "id" .= name
                , "address" .= hexWord address
                , "size" .= B.length bytes
                , "permissions" .= permissions
                , "initial" .= object ["artifact" .= blobId, "offset" .= offset, "length" .= B.length bytes]
                ]
            | (Region name address permissions bytes, offset) <- zip regions offsets
            ]
        , "functions" .= [object ["address" .= hexWord address, "size" .= size] | (address, size) <- machineFunctions ++ serviceFunctions ++ [(driverBase, fromIntegral (B.length driver))]]
        , "stubs" .= ([] :: [Value])
        , "limits" .= object ["instructions" .= (4000000 :: Int), "wall_ms" .= (10000 :: Int)]
        , "ignored_memory" .= [object ["region" .= ("stack" :: String), "offset" .= (0 :: Int), "length" .= (fromIntegral (stackTop - stackBase) :: Int), "reason" .= ("private-frame" :: String)]]
        , "expected" .=
            object
              [ "status" .= ("returned" :: String)
              , "registers" .= registers (preserved ++ [("sp", stackTop), ("pc", exitAddress)])
              , "memory" .= expectedMemory
              , "events" .= ([] :: [Value])
              ]
        , "exclusions" .= ["Host Cortex-R5F native profile. The registry's kernel send, the file service, the event report, naming, property and flag services are logging stubs; no kernel, scheduling, interrupt, media task or device qualification." :: String]
        ]

-- Execution ---------------------------------------------------------------------------------

data Observation = Observation
  { observationCase :: String
  , observationPassed :: Bool
  , observationStatus :: String
  , observationMismatches :: [String]
  , observationInstructions :: Integer
  , observationMemory :: [(Word32, B.ByteString)]
  }

field :: String -> Value -> Value
field name (Object fields) = fromMaybe Null (KM.lookup (Key.fromString name) fields)
field _ _ = Null

text :: Value -> String
text (String value) = T.unpack value
text _ = ""

fromHex :: String -> B.ByteString
fromHex = B.pack . go
  where
    go (a : b : rest) = fromIntegral (digitToInt a * 16 + digitToInt b) : go rest
    go _ = []

readAddress :: String -> Word32
readAddress textual = foldl (\acc c -> acc * 16 + fromIntegral (digitToInt c)) 0 (drop 2 textual)

observe :: Native.CaseReport -> Observation
observe report =
  Observation
    { observationCase = Native.caseIdText (Native.reportCase report)
    , observationPassed = text (field "status" value) == "passed"
    , observationStatus = text (field "status" runner)
    , observationMismatches = case field "mismatches" value of
        Array items -> map text (V.toList items)
        _ -> []
    , observationInstructions = case field "instructions" runner of
        Number count -> truncate count
        String count | [(parsed, "")] <- reads (T.unpack count) -> parsed
        _ -> 0
    , observationMemory = case field "memory" runner of
        Array items -> [(readAddress (text (field "address" item)), fromHex (text (field "bytes" item))) | item <- V.toList items]
        _ -> []
    }
  where
    value = Native.reportValue report
    runner = field "runner" value

runRole :: FilePath -> FilePath -> [(String, Loaded)] -> E.Elf -> Role -> String -> [(Scenario, Expectation)] -> IO [Observation]
runRole runner directory loaded exif role phase cases = do
  let batchDirectory = directory </> (phase ++ "-" ++ roleLabel role)
  createDirectoryIfMissing True batchDirectory
  listed <- forM cases $ \(scenario, expectation) -> do
    let (ident, document, blob) = caseDocument loaded exif role scenario expectation
    B.writeFile (batchDirectory </> (sha256Hex (sha256 blob) ++ ".bin")) blob
    BL.writeFile (batchDirectory </> (ident ++ ".case.json")) (encode document)
    pure (object ["id" .= ident, "path" .= (ident ++ ".case.json")])
  let batchPath = batchDirectory </> "batch.json"
  BL.writeFile batchPath (encode (object ["schema_version" .= (1 :: Int), "cases" .= listed]))
  collected <- newIORef []
  summary <- Native.runBatch runner batchPath $ \report -> do
    BL.writeFile (batchDirectory </> (Native.caseIdText (Native.reportCase report) ++ ".report.json")) (encode (Native.reportValue report))
    modifyIORef' collected (observe report :)
  observations <- reverse <$> readIORef collected
  unless (Native.summaryStatus summary == Native.BatchComplete && length observations == length cases) $
    fail ("native batch did not complete for " ++ phase ++ " " ++ roleLabel role ++ ": " ++ show (Native.summaryReason summary))
  pure observations

-- | The expectation a stock observation fixes: every writable region's final bytes and r0 of
-- each returning call.
expectationOf :: Scenario -> Observation -> Expectation
expectationOf scenario observation =
  Expectation
    { expectedRegions = [(name, bytesAt address) | Region name address _ _ <- writableRegions scenario, name /= "results"]
    , expectedResults = take returning (wordsOf (bytesAt resultsBase))
    }
  where
    bytesAt address = fromMaybe B.empty (lookup address (observationMemory observation))
    returning = length (filter stepReturns (scenarioSteps scenario))

initialExpectation :: Scenario -> Expectation
initialExpectation scenario =
  Expectation
    { expectedRegions = [(name, bytes) | Region name _ _ bytes <- writableRegions scenario, name /= "results"]
    , expectedResults = [0 | step <- scenarioSteps scenario, stepReturns step]
    }

wordsOf :: B.ByteString -> [Word32]
wordsOf bytes
  | B.length bytes < 4 = []
  | otherwise = wordOf bytes : wordsOf (B.drop 4 bytes)

logWords :: Observation -> [Word32]
logWords observation = drop 1 (wordsOf (fromMaybe B.empty (lookup callLog (observationMemory observation))))

stockCoverage :: [(String, Loaded)] -> FilePath -> [Scenario] -> IO Coverage.Coverage
stockCoverage loaded directory cases = do
  observations <- forM cases $ \scenario -> do
    bytes <- B.readFile (directory </> (scenarioName scenario ++ "-stock.report.json"))
    either (\problem -> fail (scenarioName scenario ++ " coverage: " ++ problem)) pure (Coverage.parseObservation bytes)
  pure (Coverage.coverage extents observations)
  where
    extents = [Coverage.Extent entry size (Just (sha256Hex (sha256 (slice loaded "plain" entry (entry + size))))) | (entry, size) <- stockFunctions]

missingDirections :: Coverage.Coverage -> [(Word32, String)]
missingDirections result =
  [ (Coverage.branchAddress decision, missing)
  | function <- Coverage.coverageFunctions result
  , decision <- Coverage.functionBranches function
  , missing <- case Coverage.branchDirection decision of
      Coverage.Both -> []
      Coverage.PassedOnly -> ["not-taken"]
      Coverage.FailedOnly -> ["taken"]
      _ -> ["taken", "not-taken"]
  ]

atExit :: Observation -> Bool
atExit observation = observationStatus observation == "returned"

-- Main ----------------------------------------------------------------------------------------

loadElf :: [(String, Word32)] -> [Extent] -> FilePath -> IO (E.Elf, String)
loadElf required extents path = do
  bytes <- B.readFile path
  elf <- either fail pure (E.parseElf bytes)
  forM_ required $ \(name, address) ->
    case [symbol | symbol <- E.elfSymbols elf, E.symbolName symbol == name, E.symbolKind symbol == E.FunctionSymbol] of
      [symbol] | E.symbolValue symbol == address -> pure ()
      _ -> fail (path ++ ": " ++ name ++ " is missing or not at its stock address")
  let outside =
        [ E.sectionName section
        | section <- E.elfSections elf
        , E.sectionAllocated section
        , E.sectionSize section > 0
        , not (any (\extent -> extentContains extent (E.sectionAddress section) && extentContains extent (E.sectionAddress section + E.sectionSize section - 1)) extents)
        ]
  unless (null outside) (fail (path ++ ": sections outside the retired extents: " ++ unwords outside))
  pure (elf, sha256Hex (sha256 bytes))

loadMachine :: FilePath -> IO (E.Elf, String)
loadMachine = loadElf entries [ownedExtent]

splitControl :: String -> (String, FilePath)
splitControl argument = case break (== '=') argument of
  (name, '=' : path) -> (name, path)
  _ -> error ("control argument is not NAME=ELF: " ++ argument)

main :: IO ()
main = do
  arguments <- getArgs
  case arguments of
    runner : loaderPath : image4Path : image5Path : plainPath : ownedPath : exifPath : output : controlArguments@(_ : _) -> do
      exists <- doesPathExist output
      when exists (fail "output exists")
      createDirectoryIfMissing True output
      loaded <- forM (zip images [loaderPath, image4Path, image5Path, plainPath]) $ \(image, path) -> do
        bytes <- B.readFile path
        unless (sha256Hex (sha256 bytes) == imageDigest image) (fail (imageLabel image ++ " identity differs"))
        pure (imageLabel image, Loaded (imageBase image) bytes)
      let Extent machineLow machineHigh = ownedExtent
          overlapping = [() | (_, Extent low high) <- Exif.ownedExtents, low < machineHigh, machineLow < high]
      unless (null overlapping) (fail "the Exif writer and machine extents overlap")
      (exif, exifDigest) <- loadElf Exif.entries (map snd Exif.ownedExtents) exifPath
      (owned, ownedDigest) <- loadMachine ownedPath
      observed <- runRole runner output loaded exif Stock "observe" [(scenario, initialExpectation scenario) | scenario <- scenarios]
      forM_ observed $ \observation ->
        unless (atExit observation) (fail ("stock observation " ++ observationCase observation ++ " ended " ++ observationStatus observation ++ ": " ++ unwords (observationMismatches observation)))
      let judged = zip scenarios (zipWith expectationOf scenarios observed)
      stocks <- runRole runner output loaded exif Stock "judge" judged
      covered <- stockCoverage loaded (output </> "judge-stock") scenarios
      owneds <- runRole runner output loaded exif (Owned "owned" owned) "judge" judged
      controls <- forM (map splitControl controlArguments) $ \(name, path) -> do
        (elf, digest) <- loadMachine path
        results <- runRole runner output loaded exif (Owned name elf) "judge" judged
        let detected = [observationCase o | o <- results, atExit o, not (observationPassed o)]
            other = [(observationCase o, observationStatus o) | o <- results, not (atExit o)]
        pure (name, digest, detected, other)
      let compared =
            [ (scenario, stock, mine, problems)
            | (scenario, stock, mine) <- zip3 scenarios stocks owneds
            , let problems =
                    ["stock " ++ observationStatus stock ++ ": " ++ unwords (observationMismatches stock) | not (observationPassed stock)]
                      ++ ["owned " ++ observationStatus mine ++ ": " ++ unwords (observationMismatches mine) | not (observationPassed mine)]
            ]
          different = [() | (_, _, _, problems) <- compared, not (null problems)]
          undetected = [name | (name, _, detected, _) <- controls, null detected]
          branches = concatMap Coverage.functionBranches (Coverage.coverageFunctions covered)
          missing = missingDirections covered
          declared = [(address, direction) | (address, direction, _) <- unreachableDirections]
          uncovered = [gap | gap <- missing, gap `notElem` declared]
          staleUnreachable = [gap | gap <- declared, gap `notElem` missing]
          unauthenticated = [entry | function <- Coverage.coverageFunctions covered, let entry = Coverage.functionEntry function, not (Coverage.functionAuthenticated function)]
          complete = null uncovered && null staleUnreachable && null unauthenticated
          equal = null different && null undetected && complete
          report =
            object
              [ "suite" .= ("save-file-machine" :: String)
              , "images" .= [object ["label" .= imageLabel image, "base" .= hexWord (imageBase image), "sha256" .= imageDigest image] | image <- images]
              , "plain_sha256" .= concat [imageDigest image | image <- images, imageLabel image == "plain"]
              , "owned_sha256" .= ownedDigest
              , "exif_writer_sha256" .= exifDigest
              , "decomp_commit" .= ("55436fc85c0560dd252b0410af6f2427fe198cd3" :: String)
              , "verdict" .= (if equal then "equal" else "different" :: String)
              , "cases" .=
                  [ object
                      [ "case" .= scenarioName scenario
                      , "stock_status" .= observationStatus stock
                      , "owned_status" .= observationStatus mine
                      , "stock_instructions" .= observationInstructions stock
                      , "owned_instructions" .= observationInstructions mine
                      , "logged_words" .= length (logWords stock)
                      , "differences" .= problems
                      ]
                  | (scenario, stock, mine, problems) <- compared
                  ]
              , "controls" .=
                  [ object
                      [ "name" .= name
                      , "sha256" .= digest
                      , "exit_detections" .= detected
                      , "other_statuses" .= [object ["case" .= ident, "status" .= status] | (ident, status) <- other]
                      , "detected" .= not (null detected)
                      ]
                  | (name, digest, detected, other) <- controls
                  ]
              , "stock_coverage" .=
                  object
                    [ "functions" .= length (Coverage.coverageFunctions covered)
                    , "unauthenticated" .= map hexWord unauthenticated
                    , "branches" .= length branches
                    , "directions" .= (2 * length branches)
                    , "directions_covered" .= (2 * length branches - length missing)
                    , "unreachable" .= [object ["address" .= hexWord address, "direction" .= direction, "reason" .= reason] | (address, direction, reason) <- unreachableDirections]
                    , "uncovered" .= [object ["address" .= hexWord address, "direction" .= direction] | (address, direction) <- uncovered]
                    , "stale_unreachable" .= [object ["address" .= hexWord address, "direction" .= direction] | (address, direction) <- staleUnreachable]
                    , "complete" .= complete
                    ]
              ]
      BL.writeFile (output </> "report.json") (encode report)
      result <- lookupEnv "XM1_RUN_RESULT"
      forM_ result $ \directory -> do
        BL.writeFile (directory </> "save-file-machine.json") (encode report)
        BL.writeFile (directory </> "save-file-machine-coverage.json") (Coverage.coverageJson covered)
      putStrLn
        ( show (length compared) ++ " cases, " ++ show (length different) ++ " different; "
            ++ show (length controls - length undetected) ++ "/" ++ show (length controls) ++ " controls detected; "
            ++ show (2 * length branches - length missing) ++ "/" ++ show (2 * length branches) ++ " stock branch directions covered, "
            ++ show (length declared) ++ " declared unreachable, " ++ show (length uncovered) ++ " uncovered; "
            ++ (if equal then "equal" else "different")
        )
      unless equal exitFailure
    _ -> fail "usage: save-file-machine-suite RUNNER LOADER_A IMAGE4 IMAGE5 PLAIN OWNED_ELF EXIF_ELF OUTPUT CONTROL=ELF ..."
