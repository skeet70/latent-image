{-# LANGUAGE OverloadedStrings #-}
-- | M1 flash-boot check. The upstream viewer cold-boots the exact candidate DAT
-- with the captured ROM and saves a snapshot; the upstream menu probe drives the
-- fresh-settings route from that snapshot to SHOOTING MENU.
module Main where

import Control.Exception (IOException, catch, displayException)
import Control.Monad (when, (>=>))
import Data.Aeson (Key, Value (..), encode, object, toJSON, withObject, (.:), (.=))
import Data.Aeson.Types (Parser, parseMaybe)
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import qualified Data.ByteString.Lazy as BL
import Data.List (intercalate, isPrefixOf, sort)
import Data.Maybe (listToMaybe, mapMaybe)
import Emulation.EmulatorSlot (reexecInSlot, slotPolicyFromEnvironment)
import Emulation.FlashBootRoute
import Emulation.ViewerRuntime (DefaultRuntime (..), Firmware (..), FixedSceneFrame, Python (..), Runtime (..), StagedRuntime (..), StockOriginal (..), fixedSceneFrameArguments, parseFixedSceneFrame, resolve, stageViewerRuntime, stagedFirmwarePath, stagedRomPath, validateFirmware, workspace)
import FujiAF.JSON (decodeStrict)
import System.Directory (canonicalizePath, doesDirectoryExist, doesFileExist, getFileSize, listDirectory)
import System.Environment (getArgs, setEnv)
import System.Exit (ExitCode, exitFailure)
import System.FilePath ((</>), takeExtension, takeFileName)
import System.IO (Handle, IOMode (WriteMode), hFlush, hGetLine, hIsEOF, hPutStrLn, stderr, stdout, withFile)
import System.Process (StdStream (..), createProcess, proc, readCreateProcessWithExitCode, readProcess, std_err, std_in, std_out, terminateProcess, waitForProcess)

usage :: String
usage = unlines
  [ "Usage: bazel run //emulation:flash-boot-check -- --firmware DAT [--output DIR]"
  , ""
  , "Cold-boot the exact DAT bytes with the captured X-M1 ROM in the full-system emulator,"
  , "select English, accept the date prompt, wait for live view and press MENU. Exit 0 only"
  , "when the guest reaches SHOOTING MENU (menu 1, setting 5, cursor 2) with no model stop."
  , "Host-only: no camera or card-reader I/O."
  , ""
  , "  --firmware PATH  Candidate X-M1 1.50-compatible DAT"
  , "  --output PATH    Fresh runtime directory beneath workspace/private-data/"
  , "                   (default: private-data/flash-boot-check-PID-MICROS)"
  , "  --fixed-scene-frame N  Pin every scene render to the positive frame index N for the run"
  , ""
  , "The receipt is OUTPUT/flash-boot-receipt.json."
  ]

data BootResult = BootResult
  { bootInstructions :: Integer
  , bootTicks :: Integer
  , bootFrame :: Maybe Sha256
  }

-- | The last nonempty lines of a process's standard error, then its exit status.
exitReport :: Int -> String -> ExitCode -> String
exitReport count errors code =
  intercalate "\n" (reverse (take count (reverse (filter (not . null) (lines errors)))) ++ ["exit: " ++ show code])

-- | Runs the viewer until it serves, which follows the snapshot save, then
-- stops it. A viewer that exits first failed to boot.
bootCandidate :: StagedRuntime -> FilePath -> Maybe FixedSceneFrame -> IO (Either CheckFailure BootResult)
bootCandidate staged snapshot fixedFrame = do
  let Python python = stagedPython staged
      Firmware source = stagedSource staged
      directory = stagedDirectory staged
      viewerOutput = directory </> "viewer"
      errorLog = directory </> "boot.stderr"
  (printed, served, code) <- withFile errorLog WriteMode $ \errors -> do
    (_, Just output, _, process) <- createProcess (proc python
      ([ "tools/start_setup_viewer.py"
       , "--firmware", stagedFirmwarePath
       , "--firmware-name", takeFileName source
       , "--save-snapshot", snapshot
       , "--output", viewerOutput
       , "--port", "0"
       ] ++ fixedSceneFrameArguments fixedFrame)) { std_in = NoStream, std_out = CreatePipe, std_err = UseHandle errors }
    (printed, served) <- readUntilServing output []
    when served (terminateProcess process)
    code <- waitForProcess process
    pure (printed, served, code)
  let documents = mapMaybe (either (const Nothing) Just . decodeStrict . C.pack) printed
      saved = listToMaybe (mapMaybe (parseMaybe snapshotRecord) documents)
      frame = listToMaybe (mapMaybe (parseMaybe summaryFrame >=> parseSha256) documents)
  case (served, saved) of
    (True, Just (instructions, ticks)) -> pure (Right (BootResult instructions ticks frame))
    _ -> do
      diagnostics <- readDiagnostics (viewerOutput </> "boot-failure.json")
      errors <- readFile errorLog
      pure (Left (BootStopped (exitReport 1 errors code) diagnostics))
  where
    snapshotRecord :: Value -> Parser (Integer, Integer)
    snapshotRecord = withObject "snapshot" $ \record -> do
      _ <- record .: "snapshot" :: Parser String
      (,) <$> record .: "instructions" <*> record .: "virtual_ticks"
    summaryFrame :: Value -> Parser String
    summaryFrame = withObject "summary" $ \summary ->
      summary .: "frame" >>= withObject "frame" (.: "png_sha256")

readUntilServing :: Handle -> [String] -> IO ([String], Bool)
readUntilServing output printed = do
  finished <- hIsEOF output
  if finished
    then pure (reverse printed, False)
    else do
      line <- hGetLine output
      putStrLn line
      hFlush stdout
      if "X-M1 firmware viewer:" `isPrefixOf` line
        then pure (reverse (line : printed), True)
        else readUntilServing output (line : printed)

readDiagnostics :: FilePath -> IO (Maybe Value)
readDiagnostics path = do
  exists <- doesFileExist path
  if exists then either (const Nothing) Just . decodeStrict <$> B.readFile path else pure Nothing

-- | The registry id of the firmware profile a portable snapshot was saved with.
newtype FirmwareId = FirmwareId String

snapshotFirmware :: FilePath -> IO FirmwareId
snapshotFirmware snapshot = do
  manifest <- B.readFile (snapshot </> "manifest.json")
  let firmwareId = withObject "manifest" $ \document ->
        document .: "factory_options" >>= withObject "factory_options" (\options ->
          options .: "firmware_profile" >>= withObject "firmware_profile" (.: "firmware_id"))
  maybe (ioError (userError ("snapshot manifest names no firmware profile: " ++ snapshot))) (pure . FirmwareId)
    (either (const Nothing) Just (decodeStrict manifest) >>= parseMaybe firmwareId)

driveDirectory :: FilePath
driveDirectory = ".tools" </> "flash-boot-drive"

-- | The probe must import the emulator with the snapshot's firmware selected,
-- as @start_setup_viewer.py --snapshot@ does.
driveRoute :: StagedRuntime -> FilePath -> Maybe FixedSceneFrame -> IO DriveResult
driveRoute staged snapshot fixedFrame = do
  let Python python = stagedPython staged
      directory = stagedDirectory staged
      commands = [probeCommand step | (_, steps) <- freshBootRoute, step <- steps] ++ ["quit"]
  FirmwareId firmwareId <- snapshotFirmware snapshot
  setEnv "XM1_VIEWER_INITIAL_FIRMWARE" firmwareId
  (code, output, errors) <- readCreateProcessWithExitCode (proc python
    ([ "tools/probe_setup_menus.py"
     , "--snapshot", snapshot
     , "--output", directory </> driveDirectory
     , "--interactive"
     ] ++ fixedSceneFrameArguments fixedFrame)) (unlines commands)
  writeFile (directory </> "drive.stdout") output
  writeFile (directory </> "drive.stderr") errors
  putStr output
  pure (judgeProbeOutput (exitReport 3 errors code) output)

data SourceIdentity = SourceIdentity
  { stagedTreeDigest :: Sha256
  , stagedTreeFiles :: Int
  , romDigest :: Sha256
  , engineDigest :: Sha256
  , unicornPath :: FilePath
  , unicornDigest :: Sha256
  , pythonPath :: FilePath
  }

-- | The staged trees that decide guest execution, excluding the candidate DAT.
identityTrees :: [FilePath]
identityTrees = ["emulator", "firmware", "native", "tools", "inputs/x-m1-1.50"]

sha256Files :: [FilePath] -> IO [Sha256]
sha256Files [] = pure []
sha256Files paths = do
  let (batch, rest) = splitAt 256 paths
  output <- readProcess "sha256sum" ("--" : batch) ""
  digests <- traverse (\line -> maybe (ioError (userError ("unexpected sha256sum line: " ++ line))) pure
    (parseSha256 (takeWhile (/= ' ') line))) (lines output)
  if length digests == length batch then (digests ++) <$> sha256Files rest
    else ioError (userError "sha256sum omitted a file")

sha256String :: String -> IO Sha256
sha256String text = do
  output <- readProcess "sha256sum" [] text
  maybe (ioError (userError "sha256sum returned no digest")) pure (parseSha256 (take 64 output))

sha256File :: FilePath -> IO Sha256
sha256File path = sha256Files [path] >>= \digests -> case digests of
  [digest] -> pure digest
  _ -> ioError (userError ("sha256sum returned no digest for " ++ path))

relativeFiles :: FilePath -> FilePath -> IO [FilePath]
relativeFiles root relative = do
  directory <- doesDirectoryExist (root </> relative)
  if directory
    then do
      names <- sort <$> listDirectory (root </> relative)
      concat <$> traverse (relativeFiles root . (relative </>)) names
    else pure [relative]

sourceIdentity :: StagedRuntime -> IO SourceIdentity
sourceIdentity staged = do
  let directory = stagedDirectory staged
      Python python = stagedPython staged
  files <- sort . concat <$> traverse (relativeFiles directory) identityTrees
  digests <- sha256Files (map (directory </>) files)
  let manifest = zip files digests
      lookupDigest name = maybe (ioError (userError ("staged file missing: " ++ name))) pure (lookup name manifest)
  tree <- sha256String (concat [sha256Text digest ++ "  " ++ file ++ "\n" | (file, digest) <- manifest])
  rom <- lookupDigest stagedRomPath
  engine <- lookupDigest "native/python/x70_native/_native.abi3.so"
  unicorn <- canonicalizePath (stagedUnicorn staged)
  unicornSha <- sha256File unicorn
  interpreter <- canonicalizePath python
  pure (SourceIdentity tree (length files) rom engine unicorn unicornSha interpreter)

menuValue :: MenuState -> Value
menuValue (MenuState menu setting cursor) = object ["menu_id" .= menu, "setting_id" .= setting, "cursor" .= cursor]

sha256Value :: Sha256 -> Value
sha256Value = toJSON . sha256Text

milestoneValue :: Milestone -> Value
milestoneValue (Milestone name observed) = object
  [ "phase" .= name
  , "instructions" .= observedInstructions observed
  , "menu" .= fmap menuValue (observedMenu observed)
  , "frame_sha256" .= fmap sha256Value (observedFrame observed)
  ]

failureValue :: CheckFailure -> Value
failureValue failure = case failure of
  BootStopped message diagnostics -> record "boot-stopped" Nothing message ["diagnostics" .= diagnostics]
  ModelStopped phase message -> record "model-stopped" (Just phase) message []
  InputRefused phase message -> record "input-refused" (Just phase) message []
  DriveEnded phase message -> record "drive-ended" (Just phase) message []
  MenuMissed reached -> record "menu-missed" (Just ShootingMenuPhase)
    "the route ended outside SHOOTING MENU" ["reached_menu" .= fmap menuValue reached]
  where
    record :: String -> Maybe Phase -> String -> [(Key, Value)] -> Value
    record kind phase message extra = object
      (["kind" .= kind, "phase" .= fmap phaseName phase, "message" .= message] ++ extra)

bootStopInstructions :: CheckFailure -> Maybe Integer
bootStopInstructions (BootStopped _ diagnostics) =
  diagnostics >>= parseMaybe (withObject "boot failure" (.: "instructions"))
bootStopInstructions _ = Nothing

failureSummary :: CheckFailure -> String
failureSummary failure = case failure of
  BootStopped message _ -> "boot stopped: " ++ message
  ModelStopped phase message -> phaseName phase ++ ": model stop: " ++ message
  InputRefused phase message -> phaseName phase ++ ": input refused: " ++ message
  DriveEnded phase message -> phaseName phase ++ ": drive ended: " ++ message
  MenuMissed reached -> "SHOOTING MENU not reached; final menu " ++ maybe "none" showMenu reached
  where
    showMenu (MenuState menu setting cursor) = show (menu, setting, cursor)

data Command = Help | Validate Firmware | Check (Maybe Runtime) Firmware (Maybe FixedSceneFrame)

parseCommand :: [String] -> IO Command
parseCommand ["--validate-firmware", path]
  | path /= "--help" = pure (Validate (Firmware path))
parseCommand arguments
  | "--help" `elem` arguments = pure Help
  | otherwise = go Nothing Nothing Nothing arguments
  where
    go output (Just firmware) frame [] = pure (Check output firmware frame)
    go _ Nothing _ [] = ioError (userError ("--firmware is required\n" ++ usage))
    go output _ frame ("--firmware" : value : rest) | not (null value) = go output (Just (Firmware value)) frame rest
    go _ firmware frame ("--output" : value : rest) | not (null value) = go (Just (Runtime value)) firmware frame rest
    go output firmware _ ("--fixed-scene-frame" : value : rest) = do
      selected <- either (ioError . userError) pure (parseFixedSceneFrame value)
      go output firmware (Just selected) rest
    go _ _ _ (argument : _) = ioError (userError ("Unknown option or missing value: " ++ argument ++ "\n" ++ usage))

check :: Maybe Runtime -> Firmware -> Maybe FixedSceneFrame -> IO ()
check output (Firmware supplied) fixedFrame = do
  root <- workspace Nothing
  validateFirmware (Firmware (resolve root supplied))
  arguments <- getArgs
  policy <- slotPolicyFromEnvironment
  reexecInSlot policy arguments
  staged <- stageViewerRuntime root output (UniqueRuntime "flash-boot-check") (Just (Firmware supplied)) OmitStockOriginal
  let directory = stagedDirectory staged
      Firmware source = stagedSource staged
      snapshot = directory </> "boot-snapshot"
      receiptPath = directory </> "flash-boot-receipt.json"
  putStrLn ("flash-boot-check runtime: " ++ directory)
  hFlush stdout
  datDigest <- sha256File (directory </> stagedFirmwarePath)
  datBytes <- getFileSize (directory </> stagedFirmwarePath)
  identity <- sourceIdentity staged
  booted <- bootCandidate staged snapshot fixedFrame
  DriveResult milestones final failure <- case booted of
    Left stopped -> pure (DriveResult [] Nothing (Just stopped))
    Right _ -> driveRoute staged snapshot fixedFrame
  drivePngs <- do
    exists <- doesDirectoryExist (directory </> driveDirectory)
    if exists
      then map ((directory </> driveDirectory) </>) . filter ((== ".png") . takeExtension) . sort
        <$> listDirectory (directory </> driveDirectory)
      else pure []
  let receipt = object
        [ "schema" .= (1 :: Int)
        , "scope" .= ("emulated X-M1 1.50 cold boot of an exact DAT to SHOOTING MENU" :: String)
        , "result" .= (maybe "passed" (const "failed") failure :: String)
        , "failure" .= fmap failureValue failure
        , "firmware" .= object ["path" .= source, "bytes" .= datBytes, "sha256" .= sha256Value datDigest]
        , "emulator_source" .= object
            [ "staged_trees" .= identityTrees
            , "staged_tree_sha256" .= sha256Value (stagedTreeDigest identity)
            , "staged_files" .= stagedTreeFiles identity
            , "rom_sha256" .= sha256Value (romDigest identity)
            , "native_engine_sha256" .= sha256Value (engineDigest identity)
            , "unicorn_library" .= object ["path" .= unicornPath identity, "sha256" .= sha256Value (unicornDigest identity)]
            , "python" .= pythonPath identity
            ]
        , "boot" .= either (const Null) (\boot -> object
            [ "instructions" .= bootInstructions boot
            , "virtual_ticks" .= bootTicks boot
            , "frame_png_sha256" .= fmap sha256Value (bootFrame boot)
            ]) booted
        , "route" .= [object ["phase" .= phaseName phase, "commands" .= map probeCommand steps] | (phase, steps) <- freshBootRoute]
        , "expected_menu" .= menuValue shootingMenu
        , "milestones" .= map milestoneValue milestones
        , "instructions" .= maybe (failure >>= bootStopInstructions) (Just . observedInstructions) final
        , "frame_sha256" .= (final >>= fmap sha256Value . observedFrame)
        , "frame_png" .= drivePngs
        ]
  BL.writeFile receiptPath (encode receipt <> "\n")
  putStrLn ("flash-boot-check receipt: " ++ receiptPath)
  case failure of
    Nothing -> putStrLn ("flash-boot-check: passed: SHA-256 " ++ sha256Text datDigest ++ " reached SHOOTING MENU")
    Just stopped -> do
      hPutStrLn stderr ("flash-boot-check: failed: SHA-256 " ++ sha256Text datDigest ++ ": " ++ failureSummary stopped)
      exitFailure

main :: IO ()
main = (getArgs >>= parseCommand >>= run) `catch` report
  where
    run Help = putStr usage
    run (Validate firmware) = validateFirmware firmware
    run (Check output firmware fixedFrame) = check output firmware fixedFrame
    report :: IOException -> IO ()
    report errorValue = hPutStrLn stderr (displayException errorValue) >> exitFailure
