module Main where

import Control.Exception (IOException, catch, displayException)
import Control.Monad (forM_, unless, when)
import qualified FujiAF.UpdateContainer as U
import qualified Data.ByteString as B
import Data.List (isPrefixOf)
import Data.Word (Word16)
import System.Directory (canonicalizePath, copyFile, createDirectory, createDirectoryIfMissing, doesDirectoryExist, doesFileExist, getCurrentDirectory, getFileSize, listDirectory, pathIsSymbolicLink, setCurrentDirectory)
import System.Environment (getArgs, getExecutablePath, lookupEnv, setEnv)
import System.Exit (exitFailure)
import System.FilePath ((</>), isAbsolute, splitDirectories, takeDirectory, takeFileName)
import System.IO (IOMode (ReadMode), hFlush, hPutStrLn, stderr, stdout, withBinaryFile)
import System.Posix.Files (getFileStatus, isRegularFile, setFileMode)
import System.Posix.Process (executeFile, getProcessID)
import Data.Time.Clock.POSIX (getPOSIXTime)
import Text.Read (readMaybe)

newtype Workspace = Workspace FilePath
newtype Runtime = Runtime FilePath
newtype Firmware = Firmware FilePath
newtype VirtualSDImage = VirtualSDImage FilePath
newtype Python = Python FilePath
newtype NativePackage = NativePackage FilePath
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

require :: Bool -> String -> IO ()
require condition message = unless condition (ioError (userError message))

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

resolve :: Workspace -> FilePath -> FilePath
resolve (Workspace root) path = if isAbsolute path then path else root </> path

workspace :: Options -> IO Workspace
workspace options = do
  root <- case workspaceOption options of
    Just (Workspace path) -> pure path
    Nothing -> lookupEnv "BUILD_WORKSPACE_DIRECTORY" >>= maybe getCurrentDirectory pure
  Workspace <$> canonicalizePath root

validateVirtualSDImage :: Workspace -> VirtualSDImage -> IO VirtualSDImage
validateVirtualSDImage root (VirtualSDImage supplied) = do
  private <- canonicalizePath (resolve root "private-data")
  path <- canonicalizePath (resolve root supplied)
  require (splitDirectories private `isPrefixOf` splitDirectories path) "--sd-card-image must be beneath workspace/private-data/"
  status <- getFileStatus path
  require (isRegularFile status) "--sd-card-image must be a regular file"
  pure (VirtualSDImage path)

requiredEnvironment :: String -> IO FilePath
requiredEnvironment name = do
  value <- lookupEnv name
  case value of
    Just path | not (null path) -> pure path
    _ -> ioError (userError (name ++ " is missing; run this command inside the checked-in devenv shell"))

copyTree :: FilePath -> FilePath -> IO ()
copyTree source destination = do
  linked <- pathIsSymbolicLink source
  require (not linked) ("Runtime source must not contain symlinks: " ++ source)
  directory <- doesDirectoryExist source
  if directory
    then do
      createDirectoryIfMissing True destination
      names <- listDirectory source
      forM_ names $ \name -> when (name `notElem` ["__pycache__", ".git", ".tools", "target"]) $
        copyTree (source </> name) (destination </> name)
    else copyFile source destination

stageFile :: FilePath -> FilePath -> IO ()
stageFile source destination = do
  createDirectoryIfMissing True (takeDirectory destination)
  copyFile source destination

freshRuntime :: Workspace -> Maybe Runtime -> IO Runtime
freshRuntime root supplied = do
  private <- canonicalizePath (resolve root "private-data")
  path <- case supplied of
    Just (Runtime value) -> pure (resolve root value)
    Nothing -> do
      pid <- getProcessID
      micros <- (round . (* 1000000) <$> getPOSIXTime) :: IO Integer
      pure (private </> ("xm1-viewer-" ++ show pid ++ "-" ++ show micros))
  require (takeFileName path `notElem` ["", ".", ".."]) "--output must name a fresh directory"
  parent <- canonicalizePath (takeDirectory path)
  require (splitDirectories private `isPrefixOf` splitDirectories parent) "--output must be beneath workspace/private-data/"
  let destination = parent </> takeFileName path
  createDirectory destination
  setFileMode destination 0o700
  pure (Runtime destination)

validateFirmware :: Firmware -> IO ()
validateFirmware (Firmware path) = do
  bytes <- withBinaryFile path ReadMode $ \handle -> B.hGet handle (64 * 1024 * 1024 + 1)
  require (B.length bytes <= 64 * 1024 * 1024) "DAT exceeds 64 MiB"
  parsed <- either (ioError . userError) pure (U.parseContainer bytes)
  let parts = U.parsedParts parsed
      segments = U.partSegments parts
  require (U.partVersionMajor parts == 1 && U.partVersionMinor parts == 0x50) "Only X-M1 1.50-compatible DATs are supported"
  require (length segments == 8) "Expected the X-M1 1.50 segment table"
  let image4 = segments !! 4
      base = U.segLoad image4
  require (base >= 0x0061c000 && base <= 0x0061c400 && base `mod` 4 == 0) "Unsupported image-4 load address"
  require (U.segSize image4 == 0x00282828 + (0x0061c400 - base)) "Image-4 prefix does not preserve stock addresses"
  let trailer = U.partTrailer parts
      normalized = parsed {U.parsedParts = parts
        { U.partSegments = take 4 segments ++ [image4 {U.segLoad = 0x0061c400, U.segSize = 0x00282828}] ++ drop 5 segments
        , U.partTrailer = trailer {U.trImageLoad = 0x0061c400, U.trImageSize = 0x00282828}
        }}
      variableFields = ["stock main checksum", "stock image 4 header", "stock image 5 header", "stock table layout"]
      results = U.checks parsed ++ U.carriedChecks parsed ++
        filter ((`notElem` variableFields) . U.checkField) (U.stockChecks normalized)
  forM_ results $ \result -> require (U.checkPassed result)
    ("Incompatible DAT: " ++ U.checkField result ++ "; expected " ++ U.checkExpected result)

bindAddress :: Bind -> String
bindAddress IPv4Loopback = "127.0.0.1"

launch :: Options -> IO ()
launch options = do
  root <- workspace options
  media <- traverse (validateVirtualSDImage root) (virtualSDImageOption options)
  Python python <- Python <$> requiredEnvironment "XM1_VIEWER_PYTHON"
  NativePackage native <- NativePackage <$> requiredEnvironment "XM1_VIEWER_NATIVE_PACKAGE"
  unicorn <- requiredEnvironment "X70_UNICORN_LIBRARY"
  let source = resolve root "emulation/xm1-upstream"
      firmware = case firmwareOption options of
        Just (Firmware path) -> Firmware (resolve root path)
        Nothing -> Firmware (resolve root "private-data/firmware/x-m1-1.50/FWUP0002.DAT")
      Firmware dat = firmware
      romRelative = "inputs/x-m1-1.50/bootrom.bin"
      rom = resolve root "private-data/analysis/x-m1-1.50/bootrom/bootrom-0x80000000-4000.bin"
  forM_ [python, native </> "x70_native/_native.abi3.so", unicorn, dat, rom, source </> "tools/start_setup_viewer.py"] $ \path -> do
    exists <- doesFileExist path
    require exists ("Required viewer input is missing: " ++ path)
  byteCount <- getFileSize dat
  require (byteCount > 0 && byteCount <= 64 * 1024 * 1024) "DAT must be 1..64 MiB"
  Runtime destination <- freshRuntime root (outputOption options)
  forM_ ["tools", "emulator", "firmware", "native"] $ \directory ->
    copyTree (source </> directory) (destination </> directory)
  stageFile (native </> "x70_native/_native.abi3.so") (destination </> "native/python/x70_native/_native.abi3.so")
  stageFile dat (destination </> "inputs/selected.DAT")
  stageFile rom (destination </> romRelative)
  setEnv "PYTHONPATH" ((destination </> "native/python") ++ ":" ++ destination)
  setEnv "PYTHONNOUSERSITE" "1"
  setEnv "PYTHONDONTWRITEBYTECODE" "1"
  setCurrentDirectory destination
  validator <- getExecutablePath
  stageFile validator (destination </> "viewer-validator")
  setFileMode (destination </> "viewer-validator") 0o700
  setEnv "XM1_VIEWER_VALIDATOR" (destination </> "viewer-validator")
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
     , "--firmware", "inputs/selected.DAT"
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
