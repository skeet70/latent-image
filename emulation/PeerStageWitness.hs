{-# LANGUAGE OverloadedStrings #-}
module Main where

import Control.Monad (forM_, unless, when)
import Data.Aeson (Value, encode, object, (.=))
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as BL
import Data.Bits (complement)
import Data.Word (Word32)
import Emulation.EmulatorSlot (attachGuard, reexecInSlot, slotPolicyFromEnvironment)
import Emulation.FirstStageWitness (leWordAt, sha256Hex)
import Emulation.PeerStageWitness
import qualified Emulation.PeerWaitWitness as W
import qualified Emulation.PeerStartupWitness as S
import Emulation.ViewerRuntime (DefaultRuntime (..), Firmware (..), Runtime (..), Workspace, freshRuntime, resolve, validateFirmware, workspace)
import qualified FujiAF.FirstStage as F
import qualified FujiAF.Host as H
import qualified FujiAF.PeerWaitControl as C
import qualified FujiAF.UpdateContainer as U
import System.Directory (canonicalizePath, doesFileExist)
import System.Environment (getArgs)
import System.Exit (ExitCode (..))
import System.FilePath ((</>))
import System.IO (stderr)
import System.Process (showCommandForUser)

newtype Driver = Driver FilePath
newtype RunDescription = RunDescription FilePath
newtype BootRom = BootRom FilePath

data Launch = PrepareOnly | Launch Driver RunDescription BootRom

data Observation = NaturalBaseline | WaitControl C.PeerControl

data PeerInput = NaturalInput Firmware | ControlledInput Firmware C.ControlManifest

data SourceInput = StockSource U.Parsed | ControlledSource C.ControlArtifact

-- | Candidate image, candidate symbols and source DAT for an owned program.
data StartupProgram = RetainedStartup | OwnedStartup FilePath FilePath FilePath (Maybe S.StartupFault)

data Options
  = Consume FilePath Argument
  | ConsumeControl FilePath FilePath
  | Prepare FilePath FilePath FilePath Observation Launch
  | ConsumeStartup FilePath FilePath
  | CompareStartup FilePath FilePath FilePath FilePath
  | PrepareStartup FilePath FilePath FilePath S.Scenario StartupProgram Launch

usage :: String
usage = unlines
  [ "usage: peer-stage-witness --receipt FILE --argument WORD"
  , "       peer-stage-witness --control-receipt FILE --control-manifest FILE"
  , "       peer-stage-witness --firmware DAT --fflzd FFLZD --output DIR"
  , "       peer-stage-witness --firmware DAT --fflzd FFLZD --output DIR --launch --driver DRIVER --run RUN_JSON --boot-rom ROM"
  , "       peer-stage-witness --firmware DAT --fflzd FFLZD --output DIR --control held-sentinel|delayed-release [--launch --driver DRIVER --run RUN_JSON --boot-rom ROM]"
  , "       peer-stage-witness --startup retained|owned --scenario natural|held-sentinel|delayed-release [--endpoint handoff|loader-entry] --firmware DAT --fflzd FFLZD --output DIR [--candidate-image BIN --candidate-symbols SYMS --source-firmware ORIGINAL_DAT] [--launch --driver DRIVER --run RUN_JSON --boot-rom ROM]"
  , "       peer-stage-witness --startup-receipt FILE --startup-contract FILE"
  , "       peer-stage-witness --compare RETAINED_RECEIPT RETAINED_CONTRACT OWNED_RECEIPT OWNED_CONTRACT"
  , "Natural preparation writes peer-witness-contract.json and peer-witness-expected.json without starting an emulator."
  , "Launch requires an explicit driver, run description and captured 16 KiB ROM."
  , "Control preparation preserves the input and writes peer-witness-contract.json, peer-control.DAT and peer-control-manifest.json instead of the natural expected-policy file."
  , "Startup preparation writes a schema-3 peer-witness-contract.json and peer-startup-expected.json; --endpoint is required for natural and delayed-release and refused for held-sentinel."
  , "Owned startup requires --firmware to be the owned-staged DAT (staged on the control DAT for controls) and the candidate image, symbols and original DAT it was staged from."
  , "Owned --startup-fault retired-stock-fetch requires --scenario natural --endpoint handoff and a candidate calling retired stock stub_a; the schema-3 producer must refuse it on core 1 at 0x12960."
  ]

data Flag
  = ReceiptFile | ArgumentWord | ControlReceiptFile | ControlManifestFile | ControlMode | FirmwareFile | FflzdFile | OutputDirectory | DriverFile | RunFile | RomFile
  | StartupMode | ScenarioKind | EndpointName | CandidateImageFile | CandidateSymbolsFile | SourceFirmwareFile | StartupFaultName
  | StartupReceiptFile | StartupContractFile | CompareRetainedFile | CompareRetainedContract | CompareOwnedFile | CompareOwnedContract
  deriving (Eq)

parseOptions :: [String] -> Either String Options
parseOptions arguments = do
  (values, launch) <- collect [] False arguments
  let value flag = lookup flag values
      need name flag = maybe (Left (name ++ " is required\n" ++ usage)) Right (value flag)
      only allowed = unless (all ((`elem` allowed) . fst) values) (Left ("incompatible options\n" ++ usage))
      launchMode = if launch
        then Launch <$> (Driver <$> need "--driver" DriverFile) <*> (RunDescription <$> need "--run" RunFile) <*> (BootRom <$> need "--boot-rom" RomFile)
        else do
          unless (all ((`notElem` [DriverFile,RunFile,RomFile]) . fst) values) (Left "driver/run/ROM require --launch")
          pure PrepareOnly
      candidateFlags = [CandidateImageFile,CandidateSymbolsFile,SourceFirmwareFile,StartupFaultName]
  case (value StartupReceiptFile, value CompareRetainedFile, value StartupMode) of
    (Nothing, Nothing, Nothing) -> case (value ReceiptFile, value ControlReceiptFile) of
      (Just path, Nothing) -> do
        only [ReceiptFile,ArgumentWord]
        when launch (Left "receipt mode cannot launch")
        Consume path <$> (need "--argument" ArgumentWord >>= parseArgument)
      (Nothing, Just path) -> do
        only [ControlReceiptFile,ControlManifestFile]
        when launch (Left "receipt mode cannot launch")
        ConsumeControl path <$> need "--control-manifest" ControlManifestFile
      (Just _, Just _) -> Left "natural and control receipt modes are mutually exclusive"
      (Nothing, Nothing) -> do
        only [FirmwareFile,FflzdFile,OutputDirectory,DriverFile,RunFile,RomFile,ControlMode]
        firmware <- need "--firmware" FirmwareFile
        fflzd <- need "--fflzd" FflzdFile
        output <- need "--output" OutputDirectory
        observation <- maybe (Right NaturalBaseline) (fmap WaitControl . C.parseControlName) (value ControlMode)
        execution <- launchMode
        pure (Prepare firmware fflzd output observation execution)
    (Just path, Nothing, Nothing) -> do
      only [StartupReceiptFile,StartupContractFile]
      when launch (Left "receipt mode cannot launch")
      ConsumeStartup path <$> need "--startup-contract" StartupContractFile
    (Nothing, Just retained, Nothing) -> do
      only [CompareRetainedFile,CompareRetainedContract,CompareOwnedFile,CompareOwnedContract]
      when launch (Left "comparison mode cannot launch")
      CompareStartup retained <$> need "--compare" CompareRetainedContract <*> need "--compare" CompareOwnedFile <*> need "--compare" CompareOwnedContract
    (Nothing, Nothing, Just mode) -> do
      only ([FirmwareFile,FflzdFile,OutputDirectory,DriverFile,RunFile,RomFile,StartupMode,ScenarioKind,EndpointName] ++ candidateFlags)
      scenario <- need "--scenario" ScenarioKind >>= \kind -> startupScenario kind (value EndpointName)
      program <- case mode of
        "retained" -> do
          unless (all ((`notElem` candidateFlags) . fst) values) (Left "retained startup refuses candidate and startup-fault options")
          pure RetainedStartup
        "owned" -> OwnedStartup <$> need "--candidate-image" CandidateImageFile <*> need "--candidate-symbols" CandidateSymbolsFile
          <*> need "--source-firmware" SourceFirmwareFile <*> traverse S.parseStartupFault (value StartupFaultName)
        _ -> Left "--startup must be retained or owned"
      firmware <- need "--firmware" FirmwareFile
      fflzd <- need "--fflzd" FflzdFile
      output <- need "--output" OutputDirectory
      PrepareStartup firmware fflzd output scenario program <$> launchMode
    _ -> Left "startup receipt, comparison and startup preparation modes are mutually exclusive"
  where
    collect values launch [] = Right (values,launch)
    collect values False ("--launch":rest) = collect values True rest
    collect _ True ("--launch":_) = Left "duplicate --launch"
    collect values launch ("--compare":retained:retainedContract:owned:ownedContract:rest) = do
      when (any (\entry -> null entry || take 2 entry == "--") [retained,retainedContract,owned,ownedContract]) (Left ("--compare requires " ++ compareOperands))
      when (CompareRetainedFile `elem` map fst values) (Left "duplicate option: --compare")
      collect ((CompareOwnedContract,ownedContract):(CompareOwnedFile,owned):(CompareRetainedContract,retainedContract):(CompareRetainedFile,retained):values) launch rest
    collect _ _ ("--compare":_) = Left ("--compare requires " ++ compareOperands ++ "\n" ++ usage)
    collect values launch (name:entry:rest) = do
      flag <- case name of
        "--receipt" -> Right ReceiptFile
        "--argument" -> Right ArgumentWord
        "--control-receipt" -> Right ControlReceiptFile
        "--control-manifest" -> Right ControlManifestFile
        "--control" -> Right ControlMode
        "--firmware" -> Right FirmwareFile
        "--fflzd" -> Right FflzdFile
        "--output" -> Right OutputDirectory
        "--driver" -> Right DriverFile
        "--run" -> Right RunFile
        "--boot-rom" -> Right RomFile
        "--startup" -> Right StartupMode
        "--scenario" -> Right ScenarioKind
        "--endpoint" -> Right EndpointName
        "--candidate-image" -> Right CandidateImageFile
        "--candidate-symbols" -> Right CandidateSymbolsFile
        "--source-firmware" -> Right SourceFirmwareFile
        "--startup-fault" -> Right StartupFaultName
        "--startup-receipt" -> Right StartupReceiptFile
        "--startup-contract" -> Right StartupContractFile
        _ -> Left ("unknown option: " ++ name ++ "\n" ++ usage)
      when (null entry || take 2 entry == "--") (Left ("missing value for " ++ name))
      when (flag `elem` map fst values) (Left ("duplicate option: " ++ name))
      collect ((flag,entry):values) launch rest
    collect _ _ [name] = Left ("missing value for " ++ name ++ "\n" ++ usage)
    compareOperands = "RETAINED_RECEIPT RETAINED_CONTRACT OWNED_RECEIPT OWNED_CONTRACT"

startupScenario :: String -> Maybe String -> Either String S.Scenario
startupScenario kind endpoint = case (kind, endpoint) of
  ("held-sentinel", Nothing) -> Right S.HeldSentinel
  ("held-sentinel", Just _) -> Left "--endpoint is refused for held-sentinel"
  ("natural", Just name) -> S.Natural <$> endpointOf name
  ("delayed-release", Just name) -> S.DelayedRelease <$> endpointOf name
  ("natural", Nothing) -> Left "--endpoint is required for natural"
  ("delayed-release", Nothing) -> Left "--endpoint is required for delayed-release"
  _ -> Left "--scenario must be natural, held-sentinel or delayed-release"
  where
    endpointOf name = case name of
      "handoff" -> Right S.Handoff
      "loader-entry" -> Right S.LoaderEntry
      _ -> Left "--endpoint must be handoff or loader-entry"

scenarioControl :: S.Scenario -> Maybe C.PeerControl
scenarioControl (S.Natural _) = Nothing
scenarioControl S.HeldSentinel = Just C.HeldSentinel
scenarioControl (S.DelayedRelease _) = Just C.DelayedRelease

maxContainer, maxImage :: Int
maxContainer = 64 * 1024 * 1024
maxImage = 16 * 1024 * 1024

segmentById :: U.Parts -> Word32 -> IO U.SegmentEntry
segmentById parts wanted = case filter ((== wanted) . U.segId) (U.partSegments parts) of
  [entry] -> pure entry
  _ -> H.failH "container segment id is absent or repeated"

mediaSlice :: B.ByteString -> Int -> Int -> Int -> IO B.ByteString
mediaSlice bytes start offset size = do
  H.ensure (start >= 0 && offset >= 0 && size >= 0 && start + offset + size <= B.length bytes) "media extent exceeds DAT"
  pure (B.map complement (B.take size (B.drop (start + offset) bytes)))

decodeImage4 :: FilePath -> FilePath -> U.Parts -> IO B.ByteString
decodeImage4 fflzd logs parts = H.withTemporary "peer-stage-fflzd" $ \directory -> do
  let record = U.partRecord4 parts
      size = fromIntegral (U.recDecodedSize (U.recHeader record))
      input = directory </> "record.bin"
      output = directory </> "image-4.bin"
  H.ensure (size > 0 && size <= maxImage) "image4 decoded size exceeds bound"
  B.writeFile input (B.map complement (U.serializeRecord record))
  (bounded,code,out,err) <- H.runBounded 120000000 (1024 * 1024) fflzd [input,"0",output,show size] B.empty
  B.writeFile (logs </> "fflzd.stdout.log") out
  B.writeFile (logs </> "fflzd.stderr.log") err
  H.ensure (not bounded && code == ExitSuccess) "fflzd failed or exceeded wall/output bounds; see fflzd logs"
  decoded <- H.readRegular maxImage output
  H.ensure (B.length decoded == size) "fflzd output length differs from record header"
  pure decoded

pinnedRecord2 :: String
pinnedRecord2 = "2f1bf3f0e483b892d2890599679c1b515cfc1be8b8c27cbb06d4d913e428abb4"

expectedValue :: FilePath -> B.ByteString -> B.ByteString -> B.ByteString -> B.ByteString -> Argument -> IO Value
expectedValue firmware dat record2 loader image4 (Argument arg) = do
  datHash <- checkedDigest dat
  recordHash <- checkedDigest record2
  loaderHash <- checkedDigest loader
  imageHash <- checkedDigest image4
  pure (object
    [ "schema" .= (1 :: Int), "firmware" .= firmware, "firmware_sha256" .= F.sha256Hex datHash
    , "record2_sha256" .= F.sha256Hex recordHash, "loader_b_sha256" .= F.sha256Hex loaderHash
    , "image4_sha256" .= F.sha256Hex imageHash, "argument" .= arg
    , "loader_b_load" .= (0xd0000 :: Word32), "loader_b_entry" .= (0xd0020 :: Word32)
    , "argument_source" .= (0x61c40c :: Word32), "budget" .= (20000000 :: Int)
    , "cp14_identities" .= ([0x4003,0x5003] :: [Word32])
    , "stops" .= map stopName [Record2Entry,RendezvousTail,Record2Handoff,LoaderBEntry]
    , "watch_record_limit" .= (16384 :: Int)
    ])
  where
    checkedDigest = either H.failH pure . F.parseSha256 . sha256Hex

consume :: FilePath -> Argument -> IO ()
consume path expected = do
  bytes <- H.readRegular maxReceiptBytes path
  receipt <- either H.failH pure (parseReceipt bytes)
  either H.failH pure (validateReceipt expected receipt)
  putStrLn ("peer-stage receipt accepted: " ++ path)

consumeControl :: FilePath -> C.ControlManifest -> IO ()
consumeControl path manifest = do
  bytes <- H.readRegular maxReceiptBytes path
  receipt <- either H.failH pure (W.parseControlReceipt bytes)
  let expected = Argument (C.argumentWord (C.manifestArgument manifest))
  either H.failH pure (W.validateControlReceipt (C.manifestControl manifest) expected receipt)
  putStrLn ("peer-control receipt accepted: " ++ path)

qualifySource :: Observation -> Firmware -> B.ByteString -> IO SourceInput
qualifySource NaturalBaseline firmware dat = do
  validateFirmware firmware
  parsed <- either H.failH pure (U.parseContainer dat)
  forM_ (U.checks parsed ++ U.carriedChecks parsed ++ U.stockChecks parsed) $ \check ->
    H.ensure (U.checkPassed check) ("container contract: " ++ U.checkField check)
  pure (StockSource parsed)
qualifySource (WaitControl control) _ dat =
  ControlledSource <$> either H.failH pure (C.buildControl control dat)

sourceParsed :: SourceInput -> U.Parsed
sourceParsed (StockSource parsed) = parsed
sourceParsed (ControlledSource artifact) = C.controlSource artifact

prepareInput :: SourceInput -> Firmware -> FilePath -> Argument -> IO PeerInput
prepareInput (StockSource _) firmware _ _ = pure (NaturalInput firmware)
prepareInput (ControlledSource artifact) _ output (Argument expected) = do
  let manifest = C.controlManifest artifact
      path = output </> "peer-control.DAT"
  H.ensure (C.argumentWord (C.manifestArgument manifest) == expected) "decoded image4 argument differs from the control source binding"
  B.writeFile path (C.controlDat artifact)
  BL.writeFile (output </> "peer-control-manifest.json") (encode (C.manifestValue manifest))
  pure (ControlledInput (Firmware path) manifest)

inputFirmware :: PeerInput -> Firmware
inputFirmware (NaturalInput firmware) = firmware
inputFirmware (ControlledInput firmware _) = firmware

inputContract :: PeerInput -> Value
inputContract (NaturalInput _) = contractValue
inputContract (ControlledInput _ manifest) = W.controlContractValue (C.manifestControl manifest)

consumeInput :: PeerInput -> FilePath -> Argument -> IO ()
consumeInput (NaturalInput _) path expected = consume path expected
consumeInput (ControlledInput _ manifest) path _ = consumeControl path manifest

consumeStartup :: FilePath -> S.Scenario -> S.Program -> IO S.ValidatedStartupReceipt
consumeStartup path scenario program = do
  bytes <- H.readRegular S.maxStartupReceiptBytes path
  receipt <- either H.failH pure (S.parseStartupReceipt bytes)
  accepted <- either H.failH pure (S.validateStartupReceipt scenario program receipt)
  putStrLn ("peer-startup receipt accepted: " ++ path)
  pure accepted

readStartupContract :: FilePath -> IO (S.Scenario, S.Program)
readStartupContract path = H.readRegular (1024 * 1024) path >>= either H.failH pure . S.parseStartupContract

compareStartup :: FilePath -> FilePath -> FilePath -> FilePath -> IO ()
compareStartup retainedPath retainedContractPath ownedPath ownedContractPath = do
  (retainedScenario, retainedProgram) <- readStartupContract retainedContractPath
  (ownedScenario, ownedProgram) <- readStartupContract ownedContractPath
  H.ensure (retainedProgram == S.Retained) "retained contract does not name the retained program"
  H.ensure (ownedProgram /= S.Retained) "owned contract does not name an owned program"
  H.ensure (retainedScenario == ownedScenario) "retained and owned contracts differ in scenario"
  retained <- consumeStartup retainedPath retainedScenario retainedProgram
  owned <- consumeStartup ownedPath ownedScenario ownedProgram
  either H.failH pure (S.compareStartupReceipts retained owned)
  putStrLn ("peer-startup comparison accepted: " ++ retainedPath ++ " " ++ ownedPath)

enterSlot :: [String] -> Launch -> IO ()
enterSlot _ PrepareOnly = pure ()
enterSlot args Launch {} = do
  policy <- slotPolicyFromEnvironment
  reexecInSlot policy args
  attachGuard args

-- | Checks the X-M1 1.50 loader-B and image-4 bindings; returns loader B.
qualifyBindings :: B.ByteString -> U.Parsed -> IO B.ByteString
qualifyBindings dat parsed = do
  let parts = U.parsedParts parsed
      mainStart = U.stagingLength + fromIntegral (U.hMainOffset (U.parsedHeader parsed))
  H.ensure (U.partVersionMajor parts == 1 && U.partVersionMinor parts == 0x50) "requires X-M1 1.50"
  segment1 <- segmentById parts 1
  segment4 <- segmentById parts 4
  H.ensure (U.segLoad segment1 == 0xd0000 && U.segSize segment1 >= 0x24 && U.segSize segment1 <= 0x20000) "segment1 loader extent differs"
  H.ensure (U.segLoad segment4 == 0x61c400) "image4 load address differs from original binding"
  loader <- mediaSlice dat mainStart (fromIntegral (U.segFlash segment1)) (fromIntegral (U.segSize segment1))
  entry <- either H.failH pure (leWordAt loader 0xc)
  load <- either H.failH pure (leWordAt loader 4)
  H.ensure (entry == 0xd0020 && load == U.segLoad segment1) "loaderB header entry or load differs"
  pure loader

launchDriver :: Workspace -> FilePath -> FilePath -> FilePath -> Firmware -> Driver -> RunDescription -> BootRom -> IO () -> IO ()
launchDriver root output contract receipt (Firmware update) (Driver driverPath) (RunDescription runPath) (BootRom romPath) consumeReceipt = do
  driver <- canonicalizePath (resolve root driverPath)
  run <- canonicalizePath (resolve root runPath)
  rom <- canonicalizePath (resolve root romPath)
  romBytes <- H.readRegular 0x4000 rom
  H.ensure (B.length romBytes == 0x4000) "captured boot ROM must be exactly 16 KiB"
  let launchArgs = ["--run",run,"--update",update,"--boot-rom",rom,"--output",output,"--peer-witness",contract]
  putStrLn (showCommandForUser driver launchArgs)
  (bounded,code,out,err) <- H.runBounded 120000000 (1024 * 1024) driver launchArgs B.empty
  B.writeFile (output </> "driver.stdout.log") out
  B.writeFile (output </> "driver.stderr.log") err
  B.putStr out
  B.hPut stderr err
  hasReceipt <- doesFileExist receipt
  when hasReceipt consumeReceipt
  H.ensure (not bounded) "peer driver exceeded wall/output bounds"
  H.ensure (code == ExitSuccess) "peer driver exited nonzero"
  H.ensure hasReceipt "peer driver did not write peer-witness.json"

maxCandidateImage, maxCandidateSymbols :: Int
maxCandidateImage = 64 * 1024
maxCandidateSymbols = 1024 * 1024

prepareStartup :: Workspace -> FilePath -> FilePath -> FilePath -> S.Scenario -> StartupProgram -> Launch -> IO ()
prepareStartup root firmwarePath fflzdPath outputPath scenario choice execution = do
  datPath <- canonicalizePath (resolve root firmwarePath)
  let fflzd = resolve root fflzdPath
      control = scenarioControl scenario
  dat <- H.readRegular maxContainer datPath
  (parsed, program, retainedSource, faultExpectation) <- case choice of
    RetainedStartup -> do
      source <- qualifySource (maybe NaturalBaseline WaitControl control) (Firmware datPath) dat
      let parsed = sourceParsed source
          mainStart = U.stagingLength + fromIntegral (U.hMainOffset (U.parsedHeader parsed))
      record2 <- mediaSlice dat mainStart 0x50000 0x4c80
      H.ensure (sha256Hex record2 == pinnedRecord2) "record2 is not the original peer boot record"
      pure (parsed, S.Retained, Just source, Nothing)
    OwnedStartup imagePath symbolsPath originalPath fault -> do
      originalCanonical <- canonicalizePath (resolve root originalPath)
      original <- H.readRegular maxContainer originalCanonical
      _ <- qualifySource NaturalBaseline (Firmware originalCanonical) original
      bin <- H.readRegular maxCandidateImage (resolve root imagePath)
      symbols <- H.readRegular maxCandidateSymbols (resolve root symbolsPath)
      entry <- either H.failH pure (S.candidateEntry symbols)
      staged <- either H.failH pure (S.verifyStagedOwned original dat bin entry control)
      facts <- either H.failH pure (S.deriveOwnedFacts bin symbols staged)
      faultExpectation <- traverse (\selected -> either H.failH pure (S.startupFaultExpectation selected scenario bin facts)) fault
      parsed <- either H.failH pure (U.parseContainer dat)
      pure (parsed, S.Owned facts, Nothing, faultExpectation)
  _ <- qualifyBindings dat parsed
  Runtime output <- freshRuntime root (Just (Runtime outputPath)) (UniqueRuntime "peer-stage-witness")
  image4 <- decodeImage4 fflzd output (U.parsedParts parsed)
  argumentWord <- either H.failH pure (leWordAt image4 0xc)
  let expected = Argument argumentWord
  launched <- case retainedSource of
    Just source -> inputFirmware <$> prepareInput source (Firmware datPath) output expected
    Nothing -> pure (Firmware datPath)
  let Firmware launchedPath = launched
      contract = output </> "peer-witness-contract.json"
      receipt = output </> "peer-witness.json"
  launchedBytes <- if launchedPath == datPath then pure dat else H.readRegular maxContainer launchedPath
  BL.writeFile contract (encode (S.contractValue scenario program))
  BL.writeFile (output </> "peer-startup-expected.json") (encode (object (
    [ "schema" .= (3 :: Int), "scenario" .= S.scenarioValue scenario, "program" .= S.programName program
    , "firmware" .= launchedPath, "firmware_sha256" .= sha256Hex launchedBytes
    , "record_sha256" .= S.programRecordHex program, "argument" .= argumentWord
    , "loader_b_entry" .= (0xd0020 :: Word32)
    ] ++ maybe [] (\expectedFault -> ["expected_refusal" .= startupFaultValue expectedFault]) faultExpectation)))
  case execution of
    PrepareOnly -> do
      putStrLn ("peer-startup preparation: " ++ output)
      putStrLn ("receipt consumer: " ++ showCommandForUser "peer-stage-witness" ["--startup-receipt",receipt,"--startup-contract",contract])
      putStrLn "launch requires a separate explicit --launch invocation with --driver, --run and --boot-rom"
    Launch driver run rom -> launchDriver root output contract receipt launched driver run rom $ do
      accepted <- consumeStartup receipt scenario program
      case scenario of
        S.HeldSentinel -> pure ()
        _ -> do
          observed <- either H.failH pure (S.startupArgument accepted)
          H.ensure (observed == expected) "handoff argument differs from the decoded image-4 argument"
  where
    startupFaultValue :: S.StartupFault -> Value
    startupFaultValue S.RetiredStockFetch = object
      [ "kind" .= ("retired-stock-fetch" :: String), "label" .= ("forbidden-fetch" :: String)
      , "core" .= (1 :: Int), "address" .= (0x12960 :: Word32)
      , "error" .= ("startup forbidden fetch of retired stock code at 0x12960 on core 1" :: String)
      ]

main :: IO ()
main = do
  args <- getArgs
  case args of
    ["--help"] -> putStr usage
    _ -> do
      options <- either H.failH pure (parseOptions args)
      root <- workspace Nothing
      case options of
        Consume path expected -> consume (resolve root path) expected
        ConsumeControl path manifestPath -> do
          manifestBytes <- H.readRegular (256 * 1024) (resolve root manifestPath)
          manifest <- either H.failH pure (C.parseManifest manifestBytes)
          consumeControl (resolve root path) manifest
        Prepare firmwarePath fflzdPath outputPath observation execution -> do
          enterSlot args execution
          datPath <- canonicalizePath (resolve root firmwarePath)
          let fflzd = resolve root fflzdPath
          dat <- H.readRegular maxContainer datPath
          source <- qualifySource observation (Firmware datPath) dat
          let parsed = sourceParsed source
          let parts = U.parsedParts parsed
              mainStart = U.stagingLength + fromIntegral (U.hMainOffset (U.parsedHeader parsed))
          H.ensure (U.partVersionMajor parts == 1 && U.partVersionMinor parts == 0x50) "requires X-M1 1.50"
          record2 <- mediaSlice dat mainStart 0x50000 0x4c80
          H.ensure (sha256Hex record2 == pinnedRecord2) "record2 is not the original peer boot record"
          loader <- qualifyBindings dat parsed
          Runtime output <- freshRuntime root (Just (Runtime outputPath)) (UniqueRuntime "peer-stage-witness")
          image4 <- decodeImage4 fflzd output parts
          argumentWord <- either H.failH pure (leWordAt image4 0xc)
          let expected = Argument argumentWord
          input <- prepareInput source (Firmware datPath) output expected
          let contract = output </> "peer-witness-contract.json"
              receipt = output </> "peer-witness.json"
          BL.writeFile contract (encode (inputContract input))
          case input of
            NaturalInput _ -> do
              binding <- expectedValue datPath dat record2 loader image4 expected
              BL.writeFile (output </> "peer-witness-expected.json") (encode binding)
            ControlledInput _ _ -> pure ()
          case execution of
            PrepareOnly -> do
              putStrLn ("peer-stage preparation: " ++ output)
              let consumerArgs = case input of
                    NaturalInput _ -> ["--receipt",receipt,"--argument",show argumentWord]
                    ControlledInput _ _ -> ["--control-receipt",receipt,"--control-manifest",output </> "peer-control-manifest.json"]
              putStrLn ("receipt consumer: " ++ showCommandForUser "peer-stage-witness" consumerArgs)
              putStrLn "launch requires a separate explicit --launch invocation with --driver, --run and --boot-rom"
            Launch driver run rom ->
              launchDriver root output contract receipt (inputFirmware input) driver run rom (consumeInput input receipt expected)
        ConsumeStartup path contractPath -> do
          (scenario, program) <- readStartupContract (resolve root contractPath)
          _ <- consumeStartup (resolve root path) scenario program
          pure ()
        CompareStartup retainedPath retainedContractPath ownedPath ownedContractPath ->
          compareStartup (resolve root retainedPath) (resolve root retainedContractPath) (resolve root ownedPath) (resolve root ownedContractPath)
        PrepareStartup firmwarePath fflzdPath outputPath scenario program execution -> do
          enterSlot args execution
          prepareStartup root firmwarePath fflzdPath outputPath scenario program execution
