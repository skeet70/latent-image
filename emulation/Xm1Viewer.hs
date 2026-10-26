module Main where

import Control.Exception (IOException, catch, displayException)
import Control.Monad (when)
import Emulation.EmulatorSlot (reexecInSlot, slotPolicyFromEnvironment)
import Emulation.Ff80Rehearsal (runFujiModRehearsal)
import Emulation.ViewerRuntime (DefaultRuntime (..), Firmware (..), FixedSceneFrame, Python (..), Runtime (..), StagedRuntime (..), StockOriginal (..), Workspace (..), fixedSceneFrameArguments, parseFixedSceneFrame, require, resolve, stageViewerRuntime, stagedFirmwarePath, validateFirmware)
import qualified Emulation.ViewerRuntime as ViewerRuntime
import FujiModPaths (fujimodRamPushBridge, fujimodRun)
import Xm1RamPushE2E (AllowlistPolicy (..), AttachHold (..), BootMode (..), CardMode (..), E2ESettings (..), EmulationRuntime (..), FujiModBridge (..), OverrideReason (..), PushMode (..), RelinkMode (..), UiConversionProfile (..), bankD3Payload, classicChromeIconPayload, classicChromePayload, runRamPushE2E, warmAttachCheckpoint)
import Data.List (isPrefixOf, nub)
import qualified Data.Text as T
import Data.Word (Word16)
import System.Directory (canonicalizePath, doesDirectoryExist, doesFileExist, listDirectory, pathIsSymbolicLink)
import System.Environment (getArgs)
import System.Exit (exitFailure)
import System.FilePath ((</>), splitDirectories, takeDirectory, takeFileName)
import System.IO (hFlush, hPutStrLn, stderr, stdout)
import System.Posix.Files (getFileStatus, isRegularFile)
import System.Posix.Process (executeFile)
import System.Process (callProcess)
import Text.Read (readMaybe)

newtype VirtualSDImage = VirtualSDImage FilePath deriving (Eq)
newtype InstalledNand = InstalledNand FilePath deriving (Eq)
newtype ReceiptDirectory = ReceiptDirectory FilePath deriving (Eq)
newtype TrajectoryFile = TrajectoryFile FilePath deriving (Eq)
newtype Port = Port Word16

data Bind = IPv4Loopback

data CardSelection = WithCard | WithoutCard deriving (Eq)

data RuntimeChoice = RetainedChoice | FujiModChoice deriving (Eq)

-- | The RAM payload `--ram-push-e2e` pushes.
data RamPayloadChoice = BankD3Choice | ClassicChromeChoice | ClassicChromeIconChoice deriving (Eq)

data Options = Options
  { workspaceOption :: Maybe Workspace
  , outputOption :: Maybe Runtime
  , firmwareOption :: Maybe Firmware
  , virtualSDImageOption :: Maybe VirtualSDImage
  , installedNandOption :: Maybe InstalledNand
  , receiptDirectoryOption :: Maybe ReceiptDirectory
  , ff80RehearsalOption :: Bool
  , ramPushE2EOption :: Bool
  , relinkModeOption :: RelinkMode
  , allowlistOverrideOption :: Maybe OverrideReason
  , bootModeOption :: BootMode
  , segmentsOption :: Maybe [T.Text]
  , pushModeOption :: PushMode
  , trajectoriesOption :: Maybe TrajectoryFile
  , ramPayloadOption :: RamPayloadChoice
  , attachHoldOption :: AttachHold
  , uiConversionProfileOption :: UiConversionProfile
  , fixedSceneFrameOption :: Maybe FixedSceneFrame
  , cardModeOption :: CardSelection
  , runtimeOption :: RuntimeChoice
  , portOption :: Port
  , bindOption :: Bind
  , warmCheckpointOption :: Bool
  }

data Command = Help | Validate Firmware | Launch Options

usage :: String
usage = unlines
  [ "Usage: bazel run //emulation:xm1-viewer -- [options]"
  , ""
  , "Boot the preserved X-M1 1.50 emulator and serve its real framebuffer."
  , "Host-only: no camera or card-reader I/O. Runtime files stay under private-data/."
  , ""
  , "  --help                 Show this help without staging or opening inputs"
  , "  --port PORT            Loopback port, 0 chooses an available port (default: 0)"
  , "  --bind ADDRESS         127.0.0.1 or localhost (default: 127.0.0.1)"
  , "  --workspace PATH       Repository root (default: BUILD_WORKSPACE_DIRECTORY)"
  , "  --output PATH          Fresh runtime directory under workspace/private-data/"
  , "  --firmware PATH        Stock or modified X-M1 1.50-compatible DAT input"
  , "  --sd-card-image PATH   Regular raw virtual-card image beneath private-data/"
  , "  --ff80-rehearsal       Run our Haskell service client against pinned FujiMod"
  , "  --ram-push-e2e        Run allocation, relink, install, bank UI and unload in emulation"
  , "  --cardless             --ram-push-e2e: boot the installed DAT and adjustment NAND with no SD"
  , "                         card through the cardless RAM-push bridge"
  , "  --fujimod              Run RAM-push E2E or the closed ff80 rehearsal on pinned FujiMod"
  , "  --unreviewed-relink    --ram-push-e2e: relink and push without the compiled source-identity"
  , "                         and publication-contract pins; the receipt records the mode"
  , "  --allowlist-override REASON"
  , "                         --unreviewed-relink: push a bank whose ELF fails the allowlist check;"
  , "                         the receipt records the reason and every violation"
  , "  --fresh-boot           --ram-push-e2e: cold boot and replace the keyed attach checkpoint"
  , "  --warm-checkpoint      --ram-push-e2e: restore or cold-boot and save the keyed attach checkpoint,"
  , "                         then stop; needs no --receipt-output or trajectories"
  , "  --segments A,B         --ram-push-e2e: run each named trajectory segment, with the segments it"
  , "                         requires, in its own process from the post-install checkpoint"
  , "  --installed-nand PATH  Committed f7=1 NAND input for --ff80-rehearsal"
  , "  --receipt-output PATH  Fresh receipt directory beneath private-data/"
  , "  --flash-candidate      --ram-push-e2e: --firmware is a flash candidate whose loader publishes the"
  , "                         bank; boot it and run every segment (or --segments) without the RAM push"
  , "  --classic-chrome       --ram-push-e2e: push the Classic Chrome payload in place of the recipe bank and D3"
  , "  --classic-chrome-icon  --ram-push-e2e: push the Classic Chrome payload with its icon and live-view words"
  , "  --attach-hold-q        --ram-push-e2e: hold Q across the attach edge of every boot (USB-toggle candidates)"
  , "  --trajectories PATH    --ram-push-e2e trajectory file (default: emulation/ram-push-e2e-trajectories.json,"
  , "                         emulation/ram-push-cc-e2e-trajectories.json with --classic-chrome,"
  , "                         emulation/ram-push-cc-icon-e2e-trajectories.json with --classic-chrome-icon,"
  , "                         or emulation/ui-trajectories.json with --flash-candidate)"
  , "  --ui-conversion-profile PROFILE"
  , "                         declared-ui-cpu-fallback (default) or strict; also the profile a"
  , "                         --ram-push-e2e session boots and keys its attach checkpoint with."
  , "                         The fallback is an unmeasured UI-only model."
  , "  --fixed-scene-frame N  Pin every scene render to the positive frame index N for the run"
  , "                         instead of the advancing synthetic source; unselected by default"
  , ""
  , "Default DAT: private-data/firmware/x-m1-1.50/FWUP0002.DAT"
  , "Source: emulation/xm1-upstream; native engine and Python come from devenv."
  , "ROM: the owner-captured X-M1 16 KiB block; its profile fixes the expected identity."
  ]

parseCommand :: [String] -> IO Command
parseCommand ["--validate-firmware", path]
  | path /= "--help" = pure (Validate (Firmware path))
parseCommand args
  | "--help" `elem` args = pure Help
  | otherwise = Launch <$> go defaults args
  where
    defaults = Options Nothing Nothing Nothing Nothing Nothing Nothing False False ReviewedRelink Nothing CheckpointedBoot Nothing RamPush Nothing BankD3Choice NoAttachHold DeclaredUiCpuFallback Nothing WithCard RetainedChoice (Port 0) IPv4Loopback False
    go options [] = pure options
    go options ("--port" : value : rest) = do
      port <- case readMaybe value :: Maybe Integer of
        Just number | number >= 0 && number <= 65535 -> pure (Port (fromInteger number))
        _ -> ioError (userError "--port must be an integer from 0 to 65535")
      go options { portOption = port } rest
    go options ("--bind" : value : rest) = do
      require (value `elem` ["127.0.0.1", "localhost"]) "--bind must be 127.0.0.1 or localhost"
      go options { bindOption = IPv4Loopback } rest
    go options ("--workspace" : value : rest) = do
      require (not (null value)) "--workspace must be nonempty"
      go options { workspaceOption = Just (Workspace value) } rest
    go options ("--output" : value : rest) = do
      require (not (null value)) "--output must be nonempty"
      go options { outputOption = Just (Runtime value) } rest
    go options ("--firmware" : value : rest) = do
      require (not (null value)) "--firmware must be nonempty"
      go options { firmwareOption = Just (Firmware value) } rest
    go options ("--sd-card-image" : value : rest) = do
      require (not (null value)) "--sd-card-image must be nonempty"
      go options { virtualSDImageOption = Just (VirtualSDImage value) } rest
    go options ("--cardless" : rest) =
      go options { cardModeOption = WithoutCard } rest
    go options ("--fujimod" : rest) =
      go options { runtimeOption = FujiModChoice } rest
    go options ("--ff80-rehearsal" : rest) =
      go options { ff80RehearsalOption = True } rest
    go options ("--ram-push-e2e" : rest) =
      go options { ramPushE2EOption = True } rest
    go options ("--unreviewed-relink" : rest) =
      go options { relinkModeOption = UnreviewedRelink EnforceAllowlist } rest
    go options ("--allowlist-override" : value : rest) = do
      require (not (null value)) "--allowlist-override needs a reason"
      go options { allowlistOverrideOption = Just (OverrideReason value) } rest
    go options ("--fresh-boot" : rest) =
      go options { bootModeOption = FreshBoot } rest
    go options ("--warm-checkpoint" : rest) =
      go options { warmCheckpointOption = True } rest
    go options ("--segments" : value : rest) = do
      let names = filter (not . T.null) (T.splitOn (T.singleton ',') (T.pack value))
      require (not (null names) && nub names == names) "--segments must name distinct segments"
      go options { segmentsOption = Just names } rest
    go options ("--flash-candidate" : rest) =
      go options { pushModeOption = FlashCandidate } rest
    go options ("--classic-chrome" : rest) = do
      require (ramPayloadOption options == BankD3Choice) "--classic-chrome and --classic-chrome-icon are exclusive"
      go options { ramPayloadOption = ClassicChromeChoice } rest
    go options ("--classic-chrome-icon" : rest) = do
      require (ramPayloadOption options == BankD3Choice) "--classic-chrome and --classic-chrome-icon are exclusive"
      go options { ramPayloadOption = ClassicChromeIconChoice } rest
    go options ("--attach-hold-q" : rest) =
      go options { attachHoldOption = AttachHoldQ } rest
    go options ("--installed-nand" : value : rest) = do
      require (not (null value)) "--installed-nand must be nonempty"
      go options { installedNandOption = Just (InstalledNand value) } rest
    go options ("--receipt-output" : value : rest) = do
      require (not (null value)) "--receipt-output must be nonempty"
      go options { receiptDirectoryOption = Just (ReceiptDirectory value) } rest
    go options ("--trajectories" : value : rest) = do
      require (not (null value)) "--trajectories must be nonempty"
      go options { trajectoriesOption = Just (TrajectoryFile value) } rest
    go options ("--ui-conversion-profile" : value : rest) = do
      profile <- case value of
        "strict" -> pure StrictUiConversion
        "declared-ui-cpu-fallback" -> pure DeclaredUiCpuFallback
        _ -> ioError (userError "--ui-conversion-profile must be strict or declared-ui-cpu-fallback")
      go options { uiConversionProfileOption = profile } rest
    go options ("--fixed-scene-frame" : value : rest) = do
      frame <- either (ioError . userError) pure (parseFixedSceneFrame value)
      go options { fixedSceneFrameOption = Just frame } rest
    go _ (argument : _) = ioError (userError ("Unknown option or missing value: " ++ argument ++ "\n" ++ usage))

validateVirtualSDImage :: Workspace -> VirtualSDImage -> IO VirtualSDImage
validateVirtualSDImage root (VirtualSDImage supplied) = do
  private <- canonicalizePath (resolve root "private-data")
  path <- canonicalizePath (resolve root supplied)
  require (splitDirectories private `isPrefixOf` splitDirectories path) "--sd-card-image must be beneath workspace/private-data/"
  status <- getFileStatus path
  require (isRegularFile status) "--sd-card-image must be a regular file"
  pure (VirtualSDImage path)
validateInstalledNand :: Workspace -> InstalledNand -> IO InstalledNand
validateInstalledNand root (InstalledNand supplied) = do
  private <- canonicalizePath (resolve root "private-data")
  path <- canonicalizePath (resolve root supplied)
  require (splitDirectories private `isPrefixOf` splitDirectories path) "--installed-nand must be beneath workspace/private-data/"
  status <- getFileStatus path
  require (isRegularFile status) "--installed-nand must be a regular file"
  pure (InstalledNand path)

freshReceiptDirectory :: Workspace -> ReceiptDirectory -> IO ReceiptDirectory
freshReceiptDirectory root (ReceiptDirectory supplied) = do
  private <- canonicalizePath (resolve root "private-data")
  let path = resolve root supplied
  require (takeFileName path `notElem` ["", ".", ".."]) "--receipt-output must name a fresh directory"
  parent <- canonicalizePath (takeDirectory path)
  require (splitDirectories private `isPrefixOf` splitDirectories parent) "--receipt-output must be beneath workspace/private-data/"
  let destination = parent </> takeFileName path
  directoryExists <- doesDirectoryExist destination
  fileExists <- doesFileExist destination
  if directoryExists
    then do
      linked <- pathIsSymbolicLink destination
      entries <- listDirectory destination
      require (not linked && null entries) "--receipt-output existing directory must be empty and nonsymlink"
    else require (not fileExists) "--receipt-output must not already exist as a file"
  pure (ReceiptDirectory destination)

bindAddress :: Bind -> String
bindAddress IPv4Loopback = "127.0.0.1"

launch :: Options -> IO ()
launch options = do
  arguments <- getArgs
  policy <- slotPolicyFromEnvironment
  reexecInSlot policy arguments
  root <- ViewerRuntime.workspace (workspaceOption options)
  media <- traverse (validateVirtualSDImage root) (virtualSDImageOption options)
  nand <- traverse (validateInstalledNand root) (installedNandOption options)
  receipt <- traverse (freshReceiptDirectory root) (receiptDirectoryOption options)
  let ff80 = ff80RehearsalOption options
      e2e = ramPushE2EOption options
      cardless = cardModeOption options == WithoutCard
  require (not (ff80 && e2e)) "--ff80-rehearsal and --ram-push-e2e are exclusive"
  require (e2e || not cardless) "--cardless belongs to --ram-push-e2e"
  require (runtimeOption options == RetainedChoice || ff80 || (e2e && pushModeOption options == RamPush))
    "--fujimod belongs to --ram-push-e2e or --ff80-rehearsal"
  when ff80 $ do
    require (media == Nothing) "--ff80-rehearsal does not accept an SD-card image"
    require (nand /= Nothing) "--ff80-rehearsal requires --installed-nand"
    require (receipt /= Nothing) "--ff80-rehearsal requires --receipt-output"
  when e2e $ do
    case cardModeOption options of
      WithCard -> require (media /= Nothing) "--ram-push-e2e requires --sd-card-image"
      WithoutCard -> do
        require (media == Nothing) "--cardless boots without --sd-card-image"
        require (pushModeOption options == RamPush) "--cardless belongs to the RAM push, not --flash-candidate"
    require (nand /= Nothing) "--ram-push-e2e requires --installed-nand"
    if warmCheckpointOption options
      then do
        require (receipt == Nothing) "--warm-checkpoint writes no receipt"
        require (trajectoriesOption options == Nothing && segmentsOption options == Nothing)
          "--warm-checkpoint runs no trajectory"
      else require (receipt /= Nothing) "--ram-push-e2e requires --receipt-output"
    require (case firmwareOption options of Just _ -> True; Nothing -> False)
      "--ram-push-e2e requires the RC0 --firmware input"
  when (not ff80 && not e2e) $ do
    require (nand == Nothing) "--installed-nand belongs to a closed emulator rehearsal"
    require (receipt == Nothing) "--receipt-output belongs to a closed emulator rehearsal"
  when (not e2e) $ do
    require (not (warmCheckpointOption options)) "--warm-checkpoint belongs to --ram-push-e2e"
    require (trajectoriesOption options == Nothing) "--trajectories belongs to --ram-push-e2e"
    require (relinkModeOption options == ReviewedRelink) "--unreviewed-relink belongs to --ram-push-e2e"
    require (bootModeOption options == CheckpointedBoot) "--fresh-boot belongs to --ram-push-e2e"
    require (segmentsOption options == Nothing) "--segments belongs to --ram-push-e2e"
    require (pushModeOption options == RamPush) "--flash-candidate belongs to --ram-push-e2e"
    require (ramPayloadOption options == BankD3Choice) "--classic-chrome and --classic-chrome-icon belong to --ram-push-e2e"
    require (attachHoldOption options == NoAttachHold) "--attach-hold-q belongs to --ram-push-e2e"
  require (ramPayloadOption options == BankD3Choice || pushModeOption options /= FlashCandidate)
    "--classic-chrome and --classic-chrome-icon push a RAM payload; --flash-candidate pushes none"
  require (allowlistOverrideOption options == Nothing || relinkModeOption options /= ReviewedRelink)
    "--allowlist-override belongs to --unreviewed-relink"
  -- Without `--output`, `--ram-push-e2e` restages into one fixed directory.
  let fallback = if e2e
        then ReplacedRuntime ("emulator-runtimes" </> "xm1-ram-push-e2e")
        else UniqueRuntime "xm1-viewer"
      stageFujiMod = do
        (Runtime path, source) <- ViewerRuntime.stageFujiModRuntime root (outputOption options) fallback (firmwareOption options)
        pure (path, source, fujimodRamPushBridge)
  (destination, Firmware dat, executable) <- case (ff80, runtimeOption options) of
    (True, _) -> stageFujiMod
    (False, FujiModChoice) -> stageFujiMod
    (False, RetainedChoice) -> do
      staged <- stageViewerRuntime root (outputOption options) fallback (firmwareOption options)
        (if e2e then StageStockOriginal else OmitStockOriginal)
      let Python python = stagedPython staged
      pure (stagedDirectory staged, stagedSource staged, python)
  putStrLn ("X-M1 viewer runtime: " ++ destination)
  hFlush stdout
  if ff80
    then do
      let nandPath = case nand of Just (InstalledNand path) -> path; Nothing -> error "validated NAND missing"
          receiptPath = case receipt of Just (ReceiptDirectory path) -> path; Nothing -> error "validated receipt directory missing"
      runFujiModRehearsal destination nandPath
        (resolve root "private-data/emulator-checkpoints/xm1-ff80-fujimod") receiptPath
    else if e2e
      then do
        when (runtimeOption options == RetainedChoice) $
          callProcess executable ["tools/firmware_registry.py", "unpack", "xm1-150"]
        cardInput <- case media of
          Just (VirtualSDImage image) -> pure (CardBacked image)
          Nothing | cardless -> pure Cardless
                  | otherwise -> ioError (userError "--ram-push-e2e requires --sd-card-image")
        let nandPath = case nand of Just (InstalledNand path) -> path; Nothing -> error "validated NAND missing"
            receiptPath = case receipt of Just (ReceiptDirectory path) -> path; Nothing -> error "validated receipt directory missing"
            Workspace workspacePath = root
            defaultTrajectories = case (pushModeOption options, ramPayloadOption options) of
              (RamPush, BankD3Choice) -> "emulation/ram-push-e2e-trajectories.json"
              (RamPush, ClassicChromeChoice) -> "emulation/ram-push-cc-e2e-trajectories.json"
              (RamPush, ClassicChromeIconChoice) -> "emulation/ram-push-cc-icon-e2e-trajectories.json"
              (FlashCandidate, _) -> "emulation/ui-trajectories.json"
            TrajectoryFile trajectories = maybe (TrajectoryFile defaultTrajectories) id
              (trajectoriesOption options)
            checkpointRootPath = resolve root $ case (runtimeOption options, cardModeOption options) of
              (FujiModChoice, WithCard) -> "private-data/emulator-checkpoints/xm1-ram-push-fujimod"
              (FujiModChoice, WithoutCard) -> "private-data/emulator-checkpoints/xm1-ram-push-fujimod-cardless"
              (RetainedChoice, WithCard) -> "private-data/emulator-checkpoints/xm1-ram-push"
              (RetainedChoice, WithoutCard) -> "private-data/emulator-checkpoints/xm1-ram-push-cardless"
            runtime = case runtimeOption options of
              RetainedChoice -> RetainedRuntime
              FujiModChoice -> FujiModRuntime (FujiModBridge fujimodRamPushBridge fujimodRun
                (resolve root "private-data/analysis/x-m1-1.50/bootrom/bootrom-0x80000000-4000.bin"))
            relinkMode = case (relinkModeOption options, allowlistOverrideOption options) of
              (UnreviewedRelink _, Just reason) -> UnreviewedRelink (OverrideAllowlist reason)
              (mode, _) -> mode
            settings = E2ESettings relinkMode (bootModeOption options)
              checkpointRootPath (segmentsOption options)
              (pushModeOption options)
              (case ramPayloadOption options of
                BankD3Choice -> bankD3Payload
                ClassicChromeChoice -> classicChromePayload
                ClassicChromeIconChoice -> classicChromeIconPayload)
              (attachHoldOption options)
              (uiConversionProfileOption options) cardInput (fixedSceneFrameOption options) Nothing runtime dat
        if warmCheckpointOption options
          then warmAttachCheckpoint settings executable stagedFirmwarePath nandPath (destination </> "warm-attach")
          else runRamPushE2E settings executable workspacePath (resolve root trajectories) stagedFirmwarePath nandPath receiptPath
      else do
        let Port port = portOption options
            mediaArguments = case media of
              Nothing -> []
              Just (VirtualSDImage path) -> ["--sd-card-image", path]
            conversionArguments = case uiConversionProfileOption options of
              StrictUiConversion -> []
              DeclaredUiCpuFallback -> ["--ui-conversion-profile", "declared-ui-cpu-fallback"]
        executeFile executable False
          ([ "tools/start_setup_viewer.py"
           , "--firmware", stagedFirmwarePath
           , "--firmware-name", takeFileName dat
           , "--bind", bindAddress (bindOption options)
           , "--port", show port
           , "--output", destination </> "viewer"
           ] ++ mediaArguments ++ conversionArguments
              ++ fixedSceneFrameArguments (fixedSceneFrameOption options)) Nothing

main :: IO ()
main = (getArgs >>= parseCommand >>= run) `catch` report
  where
    run Help = putStr usage
    run (Validate firmware) = validateFirmware firmware
    run (Launch options) = launch options
    report :: IOException -> IO ()
    report errorValue = hPutStrLn stderr (displayException errorValue) >> exitFailure
