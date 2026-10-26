{-# LANGUAGE OverloadedStrings #-}
-- | @xm1-bulk-read-e2e@: runs the camera's @read-bulk@ and @read-ram@
-- operations (FF80-BULK-READ-01) against the emulated X-M1 1.50 through the
-- RAM-push bridge's endpoint transport, compares every captured byte with
-- the emulated memory and, optionally, with a reference file, and records the
-- two readers' rates from the bridge's transfer log.
module Main (main) where

import Control.Exception (IOException, bracket, catch, displayException)
import Control.Monad (forM, forM_, unless)
import Data.Aeson (Value (..), eitherDecodeStrict', encode, object, (.=))
import qualified Data.Aeson.Key as K
import qualified Data.Aeson.KeyMap as KM
import Data.Bits (xor)
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import qualified Data.ByteString.Lazy as BL
import Data.Char (isHexDigit)
import Data.List (isPrefixOf)
import qualified Data.Text as T
import qualified Data.Vector as V
import Data.Word (Word32)
import Emulation.CheckpointKey (CheckpointRoute (..), Interpreter (..), RouteKey (..), UnicornLibrary (..), guestState, routeKey, routeKeyHex)
import Emulation.Sha256 (sha256File, sha256Hex)
import Emulation.EmulatorSlot (reexecInSlot, slotPolicyFromEnvironment)
import Emulation.ViewerRuntime (DefaultRuntime (..), Firmware (..), Python (..), StagedRuntime (..), StockOriginal (..), Workspace (..), require, resolve, stageViewerRuntime, stagedFirmwarePath, validateFirmware)
import qualified Emulation.ViewerRuntime as ViewerRuntime
import FujiModPaths (fujimodRamPushBridge, fujimodRun)
import EmulatorUsb (EmulatorStart (..), emulatorReadMemory, emulatorUsbApi, startEmulator, stopEmulator)
import FujiAF.ReadBulk (ReadBulkReport (..), bulkCaptureName, bulkChunkCount, bulkWindowAddress, bulkWindowBytes, mkBulkWindow, readBulkResultLine, runReadBulk)
import FujiAF.ReadRam (mkRamWatch, ramCaptureName, readRamRequestLength, runReadRam, singlePass)
import FujiAF.Routes (RouteWindow (..))
import FujiAF.Transport (Clock (..), ensurePrivateDirectory, writePrivateBytes)
import Data.IORef (atomicModifyIORef', modifyIORef', newIORef)
import Data.Word (Word64)
import GHC.Clock (getMonotonicTimeNSec)
import Numeric (readHex, showHex)
import System.Directory (canonicalizePath, doesDirectoryExist, doesFileExist)
import System.Environment (getArgs)
import System.Exit (exitFailure)
import System.FilePath (splitDirectories, takeDirectory, takeFileName, (</>))
import System.IO (BufferMode (LineBuffering), hPutStrLn, hSetBuffering, stderr, stdout)
import System.Process (callProcess)

data Engine = Retained | FujiMod deriving (Eq)

data Options = Options
  { optNand :: Maybe FilePath
  , optCard :: Maybe FilePath
  , optReceipt :: Maybe FilePath
  , optFirmware :: Maybe FilePath
  , optAddress :: Int
  , optLength :: Int
  , optD1Windows :: Int
  , optReference :: Maybe FilePath
  , optFresh :: Bool
  , optEngine :: Engine
  , optRetainedReceipt :: Maybe FilePath
  }

data Command = Help | Validate Firmware | Run Options

usage :: String
usage = unlines
  [ "xm1-bulk-read-e2e --installed-nand NAND --sd-card-image CARD --receipt-output DIR [options]"
  , ""
  , "Boots the emulated X-M1 1.50 to its ordinary-boot ff80 attach (restoring the keyed attach"
  , "checkpoint when one exists), runs read-bulk ADDRESS LENGTH and a read-ram pass of D1-WINDOWS"
  , "248-byte windows from ADDRESS through the endpoint transport, and compares every captured"
  , "byte with the emulated memory. NAND must carry a stored 0xf7 != 0. All paths are beneath"
  , "workspace/private-data/; DIR must be new. Holds one emulator slot. No camera or USB I/O."
  , ""
  , "  --firmware DAT          installed DAT (default: the stock 1.50 DAT)"
  , "  --address 0xADDR        read-bulk window start (default: 0x00bf1a00, decoded image 5)"
  , "  --length 0xLEN          read-bulk window length (default: 0x400000)"
  , "  --d1-windows N          read-ram windows, 1..2032 (default: 128)"
  , "  --reference FILE        also compare the read-bulk capture with FILE's first LENGTH bytes"
  , "  --fresh-boot            cold-boot and replace the attach checkpoint"
  , "  --fujimod               use the pinned FujiMod endpoint bridge"
  , "  --compare-retained DIR  require identical captures from an earlier retained invocation"
  ]

parseCommand :: [String] -> IO Command
parseCommand ["--help"] = pure Help
parseCommand ["--validate-firmware", path] = pure (Validate (Firmware path))
parseCommand arguments = Run <$> go (Options Nothing Nothing Nothing Nothing 0x00bf1a00 0x400000 128 Nothing False Retained Nothing) arguments
  where
    go options [] = pure options
    go options ("--installed-nand" : value : rest) = go options { optNand = Just value } rest
    go options ("--sd-card-image" : value : rest) = go options { optCard = Just value } rest
    go options ("--receipt-output" : value : rest) = go options { optReceipt = Just value } rest
    go options ("--firmware" : value : rest) = go options { optFirmware = Just value } rest
    go options ("--address" : value : rest) = hex value >>= \v -> go options { optAddress = v } rest
    go options ("--length" : value : rest) = hex value >>= \v -> go options { optLength = v } rest
    go options ("--d1-windows" : value : rest) = case reads value of
      [(count, "")] | count >= 1 && count <= 2032 -> go options { optD1Windows = count } rest
      _ -> ioError (userError "--d1-windows must be 1..2032")
    go options ("--reference" : value : rest) = go options { optReference = Just value } rest
    go options ("--fresh-boot" : rest) = go options { optFresh = True } rest
    go options ("--fujimod" : rest) = go options { optEngine = FujiMod } rest
    go options ("--compare-retained" : value : rest) = go options { optRetainedReceipt = Just value } rest
    go _ (argument : _) = ioError (userError ("unknown option or missing value: " ++ argument ++ "\n" ++ usage))
    hex ('0' : 'x' : digits) | not (null digits) && length digits <= 8 && all isHexDigit digits, [(v, "")] <- readHex digits = pure v
    hex value = ioError (userError ("expected 0x-hex, not " ++ value))

privatePath :: Workspace -> String -> FilePath -> IO FilePath
privatePath root label supplied = do
  private <- canonicalizePath (resolve root "private-data")
  path <- canonicalizePath (resolve root supplied)
  require (splitDirectories private `isPrefixOf` splitDirectories path) (label ++ " must be beneath workspace/private-data/")
  exists <- doesFileExist path
  require exists (label ++ " must be a regular file")
  pure path

freshReceipt :: Workspace -> FilePath -> IO FilePath
freshReceipt root supplied = do
  private <- canonicalizePath (resolve root "private-data")
  let path = resolve root supplied
  parent <- canonicalizePath (takeDirectory path)
  require (splitDirectories private `isPrefixOf` splitDirectories parent) "--receipt-output must be beneath workspace/private-data/"
  let destination = parent </> takeFileName path
  taken <- (||) <$> doesDirectoryExist destination <*> doesFileExist destination
  require (not taken && takeFileName path `notElem` ["", ".", ".."]) "--receipt-output must name a new directory"
  ensurePrivateDirectory destination
  pure destination

-- | A session clock that advances 1 us per reading. An emulated 1 MiB bulk IN
-- takes tens of host seconds, past the operations' 5000 ms transfer budget.
steppingClock :: IO Clock
steppingClock = do
  now <- newIORef (1000000000 :: Word64)
  pure (TimedClock (atomicModifyIORef' now (\tick -> (tick + 1000, tick)))
                   (\ms -> modifyIORef' now (+ fromIntegral ms * 1000000)))

-- | The emulated bytes of [address, address + count) in 4 KiB reads.
emulatedBytes :: (Word32 -> Int -> IO B.ByteString) -> Int -> Int -> IO B.ByteString
emulatedBytes readMemory address count =
  B.concat <$> forM [0, 0x1000 .. count - 1] (\offset -> readMemory (fromIntegral (address + offset)) (min 0x1000 (count - offset)))

-- | Offsets where the two strings differ.
differences :: B.ByteString -> B.ByteString -> [Int]
differences a b = [ i | (i, x, y) <- zip3 [0 ..] (B.unpack a) (B.unpack b), x /= y ]
  ++ [B.length a `min` B.length b | B.length a /= B.length b]

comparison :: String -> B.ByteString -> B.ByteString -> Value
comparison label expected actual = object
  [ "against" .= label, "bytes" .= B.length expected, "equal" .= (expected == actual)
  , "differing_bytes" .= length differing, "first_differences" .= take 64 differing ]
  where differing = if expected == actual then [] else differences expected actual

-- | Sums of the transfer log's instructions and seconds over [from, from + count).
transferSpan :: Engine -> V.Vector Value -> Int -> Int -> (Integer, Double)
transferSpan engine entries from count = foldr add (0, 0) (V.toList (V.slice from count entries))
  where
    add (Object fields) (instructions, seconds) =
      (instructions + number "instructions" fields, seconds + realNumber elapsedField fields)
    add _ total = total
    elapsedField = case engine of Retained -> "seconds"; FujiMod -> "host_seconds"
    number name fields = case KM.lookup (K.fromString name) fields of
      Just (Number n) -> round n
      _ -> 0
    realNumber name fields = case KM.lookup (K.fromString name) fields of
      Just (Number n) -> realToFrac n
      _ -> 0

rate :: Int -> Integer -> Double -> Value
rate bytes instructions seconds = object
  [ "bytes" .= bytes, "guest_instructions" .= instructions, "bridge_seconds" .= seconds
  , "bytes_per_million_instructions" .= (fromIntegral bytes * 1e6 / max 1 (fromIntegral instructions) :: Double)
  , "bytes_per_bridge_second" .= (fromIntegral bytes / max 1e-9 seconds :: Double) ]

run :: Options -> IO ()
run options = do
  hSetBuffering stdout LineBuffering
  arguments <- getArgs
  policy <- slotPolicyFromEnvironment
  reexecInSlot policy arguments
  root <- ViewerRuntime.workspace Nothing
  nand <- maybe (ioError (userError "--installed-nand is required")) (privatePath root "--installed-nand") (optNand options)
  card <- maybe (ioError (userError "--sd-card-image is required")) (privatePath root "--sd-card-image") (optCard options)
  reference <- traverse (privatePath root "--reference") (optReference options)
  receipt <- maybe (ioError (userError "--receipt-output is required")) (freshReceipt root) (optReceipt options)
  window <- either (ioError . userError) pure (mkBulkWindow (optAddress options) (optLength options))
  let d1Count = optD1Windows options
      d1Windows = [ RouteWindow i (optAddress options + i * readRamRequestLength) readRamRequestLength "read-bulk comparison window" | i <- [0 .. d1Count - 1] ]
  watch <- either (ioError . userError) pure (mkRamWatch d1Windows singlePass)
  let Workspace workspacePath = root
      checkpoints = workspacePath </> "private-data" </> "emulator-checkpoints" </>
        (if optEngine options == FujiMod then "xm1-bulk-read-fujimod" else "xm1-bulk-read")
      fallback = ReplacedRuntime ("emulator-runtimes" </> "xm1-bulk-read-e2e")
  (destination, executable, checkpointArguments) <- case optEngine options of
    FujiMod -> do
      (ViewerRuntime.Runtime path, _) <- ViewerRuntime.stageFujiModRuntime root Nothing fallback (Firmware <$> optFirmware options)
      pure (path, fujimodRamPushBridge, [])
    Retained -> do
      staged <- stageViewerRuntime root Nothing fallback (Firmware <$> optFirmware options) StageStockOriginal
      let Python python = stagedPython staged
      callProcess python ["tools/firmware_registry.py", "unpack", "xm1-150"]
      let destination = stagedDirectory staged
      guest <- guestState destination (InterpreterExecutable python) (UnicornLibrary (stagedUnicorn staged))
      installed <- sha256File (destination </> stagedFirmwarePath)
      adjustment <- sha256File nand
      cardDigest <- sha256File card
      let key = routeKey guest CardAttach
            [ "profile" .= String "firmware/x-m1/1.50/emulator-rec.json", "installed_dat" .= sha256Hex installed
            , "adjustment_nand" .= sha256Hex adjustment, "menu_sd_image" .= sha256Hex cardDigest
            , "ui_conversion_profile" .= String "declared-ui-cpu-fallback" ]
      pure (destination, python,
        ["--checkpoint-key", routeKeyHex key, "--checkpoint-inputs", C.unpack (BL.toStrict (encode (routeKeyDocument key)))])
  putStrLn ("X-M1 bulk-read runtime: " ++ destination)
  let bridgeArguments = (case optEngine options of
        FujiMod ->
          [ "--run", fujimodRun
          , "--boot-rom", destination </> ViewerRuntime.stagedRomPath
          , "--installed-dat", stagedFirmwarePath
          , "--adjustment-nand", nand, "--sd-card-image", card
          , "--output", receipt </> "bridge", "--ram-push-checkpoints", checkpoints ]
        Retained ->
          [ "tools/continue_rec_scanout.py"
          , "--profile", "firmware/x-m1/1.50/emulator-rec.json"
          , "--installed-dat", stagedFirmwarePath
          , "--adjustment-nand", nand, "--menu-sd-image", card
          , "--menus", "--ram-push-server"
          , "--output", receipt </> "bridge", "--ram-push-checkpoints", checkpoints
          , "--ui-conversion-profile", "declared-ui-cpu-fallback" ])
        ++ checkpointArguments ++ ["--fresh-boot" | optFresh options]
      bulkDirectory = receipt </> "read-bulk"
      d1Directory = receipt </> "read-ram"
      aliasDirectory = receipt </> "read-bulk-alias"
  -- The same physical bytes through the other address alias, at most one chunk.
  aliasWindow <- either (ioError . userError) pure
    (mkBulkWindow (bulkWindowAddress window `xor` 0x40000000) (min 0x100000 (bulkWindowBytes window)))
  (bulkReport, bulkWall, d1Wall, comparisons, referenceComparison) <-
    bracket (startEmulator executable destination bridgeArguments (\_ -> pure (Nothing, ()))) (\(emulator, _, _) -> stopEmulator emulator) $ \(emulator, start, ()) -> do
      putStrLn ("emulator start: " ++ show (bootStart start))
      let api = emulatorUsbApi emulator
      ensurePrivateDirectory bulkDirectory
      bulkStarted <- getMonotonicTimeNSec
      report <- steppingClock >>= \clock -> runReadBulk window clock api bulkDirectory
      bulkFinished <- getMonotonicTimeNSec
      putStrLn (readBulkResultLine report)
      ensurePrivateDirectory d1Directory
      d1Started <- getMonotonicTimeNSec
      requests <- steppingClock >>= \clock -> runReadRam watch clock api d1Directory
      d1Finished <- getMonotonicTimeNSec
      putStrLn ("read-ram: " ++ show requests ++ " read requests")
      ensurePrivateDirectory aliasDirectory
      aliasReport <- steppingClock >>= \clock -> runReadBulk aliasWindow clock api aliasDirectory
      putStrLn ("alias " ++ readBulkResultLine aliasReport)
      captured <- B.readFile (bulkDirectory </> bulkCaptureName window)
      aliasCaptured <- B.readFile (aliasDirectory </> bulkCaptureName aliasWindow)
      memory <- emulatedBytes (emulatorReadMemory emulator) (bulkWindowAddress window) (bulkWindowBytes window)
      d1Captured <- B.concat <$> forM d1Windows (\w -> B.readFile (d1Directory </> "ram" </> (ramCaptureName 0 (routeWindowAddress w) ++ ".bin")))
      expectedReference <- traverse (\path -> B.take (bulkWindowBytes window) <$> B.readFile path) reference
      let results =
            [ ("read-bulk", comparison "emulated memory after the reads" memory captured)
            , ("read-ram", comparison "emulated memory after the reads" (B.take (B.length d1Captured) memory) d1Captured)
            , ("read-bulk-alias", comparison "emulated memory after the reads" (B.take (B.length aliasCaptured) memory) aliasCaptured) ]
          -- Writable image words differ from the load-time decode at run time.
          referenceResult = [ comparison path expected captured | (Just path, Just expected) <- [(reference, expectedReference)] ]
      pure (report, bulkFinished - bulkStarted, d1Finished - d1Started, results, referenceResult)
  session <- B.readFile (receipt </> "bridge" </> "emulator-session.json")
  transfers <- case eitherDecodeStrict' session of
    Right (Object fields) | Just (Array entries) <- KM.lookup "transfer_log" fields -> pure entries
    _ -> ioError (userError "the bridge session receipt lacks its transfer log")
  -- read-bulk: C0 2, identity 4, config 4, query 4, source state 4, two per
  -- chunk, C4 2. read-ram: C0 2, identity 4, config 4, four per window, C4 2.
  let chunks = bulkChunkCount window
      bulkTransfers w = 20 + 2 * bulkChunkCount w
      d1Bytes = d1Count * readRamRequestLength
      expected = bulkTransfers window + 12 + 4 * d1Count + bulkTransfers aliasWindow
  require (V.length transfers == expected)
    ("the bridge logged " ++ show (V.length transfers) ++ " transfers, not the sessions' " ++ show expected)
  let (bulkInstructions, bulkSeconds) = transferSpan (optEngine options) transfers 18 (2 * chunks)
      (d1Instructions, d1Seconds) = transferSpan (optEngine options) transfers (bulkTransfers window + 10) (4 * d1Count)
      equal = and [ e | (_, Object fields) <- comparisons, Just (Bool e) <- [KM.lookup "equal" fields] ]
      referenceValue = case referenceComparison of
        [value] -> value
        _ -> Null
      summary = object
        [ "schema" .= (1 :: Int)
        , "scope" .= ("emulated X-M1 1.50 ordinary-boot ff80 read-bulk and read-ram through the endpoint transport" :: T.Text)
        , "session_clock" .= ("stepping, 1 us per reading; the result line's bulk-ms is that clock, not a rate" :: T.Text)
        , "window" .= object [ "address" .= ("0x" ++ showHex (bulkWindowAddress window) ""), "bytes" .= bulkWindowBytes window, "chunks" .= chunks ]
        , "alias_window" .= object [ "address" .= ("0x" ++ showHex (bulkWindowAddress aliasWindow) ""), "bytes" .= bulkWindowBytes aliasWindow ]
        , "read_bulk" .= object
            [ "sha256" .= reportSha256 bulkReport, "capacity" .= ("0x" ++ showHex (reportCapacity bulkReport) "")
            , "result" .= readBulkResultLine bulkReport
            , "data_phase" .= rate (bulkWindowBytes window) bulkInstructions bulkSeconds
            , "session_host_seconds" .= (fromIntegral bulkWall / 1e9 :: Double) ]
        , "read_ram" .= object
            [ "windows" .= d1Count
            , "data_phase" .= rate d1Bytes d1Instructions d1Seconds
            , "session_host_seconds" .= (fromIntegral d1Wall / 1e9 :: Double) ]
        , "comparisons" .= object [ K.fromString name .= value | (name, value) <- comparisons ]
        , "reference" .= referenceValue
        , "equal" .= equal ]
  writePrivateBytes (receipt </> "summary.json") (BL.toStrict (encode summary))
  BL.putStr (encode summary) >> putStrLn ""
  unless equal (ioError (userError "a captured byte differs from the emulated memory"))
  forM_ (optRetainedReceipt options) $ \supplied -> do
    retained <- canonicalizePath (resolve root supplied)
    private <- canonicalizePath (resolve root "private-data")
    require (optEngine options == FujiMod && splitDirectories private `isPrefixOf` splitDirectories retained)
      "--compare-retained requires --fujimod and a retained receipt beneath private-data/"
    forM_ (["read-bulk" </> bulkCaptureName window, "read-bulk-alias" </> bulkCaptureName aliasWindow] ++
      ["read-ram" </> "ram" </> (ramCaptureName 0 (routeWindowAddress w) ++ ".bin") | w <- d1Windows]) $ \relative -> do
      expectedBytes <- B.readFile (retained </> relative)
      actualBytes <- B.readFile (receipt </> relative)
      require (expectedBytes == actualBytes) ("FujiMod capture differs from retained receipt: " ++ relative)
    writePrivateBytes (receipt </> "counterpart.json")
      (BL.toStrict (encode (object ["equal" .= True, "captured_bytes_and_alias_reads" .= True, "sessions_closed" .= True])))

main :: IO ()
main = (getArgs >>= parseCommand >>= dispatch) `catch` report
  where
    dispatch Help = putStr usage
    dispatch (Validate firmware) = validateFirmware firmware
    dispatch (Run options) = run options
    report :: IOException -> IO ()
    report errorValue = hPutStrLn stderr (displayException errorValue) >> exitFailure
