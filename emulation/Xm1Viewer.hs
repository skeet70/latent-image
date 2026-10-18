module Main where

import Control.Exception (IOException, catch, displayException)
import Emulation.EmulatorSlot (reexecInSlot, slotPolicyFromEnvironment)
import Emulation.ViewerRuntime (DefaultRuntime (..), Firmware (..), Python (..), Runtime (..), StagedRuntime (..), StockOriginal (..), Workspace (..), require, resolve, stageViewerRuntime, stagedFirmwarePath, validateFirmware)
import qualified Emulation.ViewerRuntime as ViewerRuntime
import Data.List (isPrefixOf)
import Data.Word (Word16)
import System.Directory (canonicalizePath)
import System.Environment (getArgs)
import System.Exit (exitFailure)
import System.FilePath ((</>), splitDirectories, takeFileName)
import System.IO (hFlush, hPutStrLn, stderr, stdout)
import System.Posix.Files (getFileStatus, isRegularFile)
import System.Posix.Process (executeFile)
import Text.Read (readMaybe)

newtype VirtualSDImage = VirtualSDImage FilePath
newtype Port = Port Word16

data Bind = IPv4Loopback

data UiConversionProfile = StrictUiConversion | DeclaredUiCpuFallback

data Options = Options
  { workspaceOption :: Maybe Workspace
  , outputOption :: Maybe Runtime
  , firmwareOption :: Maybe Firmware
  , virtualSDImageOption :: Maybe VirtualSDImage
  , uiConversionProfileOption :: UiConversionProfile
  , portOption :: Port
  , bindOption :: Bind
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
  , "  --ui-conversion-profile PROFILE"
  , "                         strict (default) or declared-ui-cpu-fallback"
  , "                         The fallback is an unmeasured UI-only model."
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
    defaults = Options Nothing Nothing Nothing Nothing StrictUiConversion (Port 0) IPv4Loopback
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
    go options ("--ui-conversion-profile" : value : rest) = do
      profile <- case value of
        "strict" -> pure StrictUiConversion
        "declared-ui-cpu-fallback" -> pure DeclaredUiCpuFallback
        _ -> ioError (userError "--ui-conversion-profile must be strict or declared-ui-cpu-fallback")
      go options { uiConversionProfileOption = profile } rest
    go _ (argument : _) = ioError (userError ("Unknown option or missing value: " ++ argument ++ "\n" ++ usage))

validateVirtualSDImage :: Workspace -> VirtualSDImage -> IO VirtualSDImage
validateVirtualSDImage root (VirtualSDImage supplied) = do
  private <- canonicalizePath (resolve root "private-data")
  path <- canonicalizePath (resolve root supplied)
  require (splitDirectories private `isPrefixOf` splitDirectories path) "--sd-card-image must be beneath workspace/private-data/"
  status <- getFileStatus path
  require (isRegularFile status) "--sd-card-image must be a regular file"
  pure (VirtualSDImage path)

bindAddress :: Bind -> String
bindAddress IPv4Loopback = "127.0.0.1"

launch :: Options -> IO ()
launch options = do
  arguments <- getArgs
  policy <- slotPolicyFromEnvironment
  reexecInSlot policy arguments
  root <- ViewerRuntime.workspace (workspaceOption options)
  media <- traverse (validateVirtualSDImage root) (virtualSDImageOption options)
  staged <- stageViewerRuntime root (outputOption options) (UniqueRuntime "xm1-viewer") (firmwareOption options) OmitStockOriginal
  let destination = stagedDirectory staged
      Python python = stagedPython staged
      Firmware dat = stagedSource staged
  putStrLn ("X-M1 viewer runtime: " ++ destination)
  hFlush stdout
  let Port port = portOption options
      mediaArguments = case media of
        Nothing -> []
        Just (VirtualSDImage path) -> ["--sd-card-image", path]
      conversionArguments = case uiConversionProfileOption options of
        StrictUiConversion -> []
        DeclaredUiCpuFallback -> ["--ui-conversion-profile", "declared-ui-cpu-fallback"]
  executeFile python False
    ([ "tools/start_setup_viewer.py"
     , "--firmware", stagedFirmwarePath
     , "--firmware-name", takeFileName dat
     , "--bind", bindAddress (bindOption options)
     , "--port", show port
     , "--output", destination </> "viewer"
     ] ++ mediaArguments ++ conversionArguments) Nothing

main :: IO ()
main = (getArgs >>= parseCommand >>= run) `catch` report
  where
    run Help = putStr usage
    run (Validate firmware) = validateFirmware firmware
    run (Launch options) = launch options
    report :: IOException -> IO ()
    report errorValue = hPutStrLn stderr (displayException errorValue) >> exitFailure
