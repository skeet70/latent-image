-- | Runtime staging shared by the X-M1 full-system emulator launchers. A
-- runtime is a fresh directory beneath @private-data/@ holding the vendored
-- emulator, the devenv native engine, the captured ROM and the selected DAT;
-- the preserved Python programs run with it as their root.
module Emulation.ViewerRuntime
  ( Workspace (..)
  , Runtime (..)
  , Firmware (..)
  , Python (..)
  , FixedSceneFrame (..)
  , DefaultRuntime (..)
  , StockOriginal (..)
  , StagedRuntime (..)
  , require
  , resolve
  , workspace
  , freshRuntime
  , parseFixedSceneFrame
  , fixedSceneFrameArguments
  , validateFirmware
  , stageViewerRuntime
  , stagedFirmwarePath
  , stagedRomPath
  ) where

import Control.Monad (forM_, unless, when)
import qualified Data.ByteString as B
import Data.List (isPrefixOf)
import Data.Time.Clock.POSIX (getPOSIXTime)
import Data.Word (Word64)
import qualified FujiAF.UpdateContainer as U
import System.Directory (canonicalizePath, copyFile, createDirectory, createDirectoryIfMissing, doesDirectoryExist, doesFileExist, getCurrentDirectory, getFileSize, listDirectory, pathIsSymbolicLink, removeDirectoryRecursive, setCurrentDirectory)
import System.Environment (getExecutablePath, lookupEnv, setEnv)
import System.FilePath ((</>), isAbsolute, splitDirectories, takeDirectory, takeFileName)
import System.IO (IOMode (ReadMode), withBinaryFile)
import System.Posix.Files (setFileMode)
import System.Posix.Process (getProcessID)
import Text.Read (readMaybe)

newtype Workspace = Workspace FilePath
newtype Runtime = Runtime FilePath
newtype Firmware = Firmware FilePath
newtype Python = Python FilePath

-- | A frame index every scene render of the run uses in place of the source's
-- advancing frame.
newtype FixedSceneFrame = FixedSceneFrame Word64

parseFixedSceneFrame :: String -> Either String FixedSceneFrame
parseFixedSceneFrame value = case readMaybe value of
  Just frame | frame > 0 -> Right (FixedSceneFrame frame)
  _ -> Left "--fixed-scene-frame must be a positive integer"

fixedSceneFrameArguments :: Maybe FixedSceneFrame -> [String]
fixedSceneFrameArguments Nothing = []
fixedSceneFrameArguments (Just (FixedSceneFrame frame)) = ["--fixed-scene-frame", show frame]

-- | Where a launcher stages when no @--output@ names the runtime.
data DefaultRuntime
  = -- | This path beneath @private-data/@, removed first if it exists.
    ReplacedRuntime FilePath
  | -- | A new @private-data/PREFIX-PID-MICROS@.
    UniqueRuntime String

-- | Whether the pinned stock DAT is also staged at the registry's original path.
data StockOriginal = StageStockOriginal | OmitStockOriginal deriving (Eq)

data StagedRuntime = StagedRuntime
  { stagedDirectory :: FilePath
  , stagedPython :: Python
  , stagedUnicorn :: FilePath
  , stagedSource :: Firmware
  }

-- | The selected DAT's path inside a staged runtime.
stagedFirmwarePath :: FilePath
stagedFirmwarePath = "inputs/selected.DAT"

stagedRomPath :: FilePath
stagedRomPath = "inputs/x-m1-1.50/bootrom.bin"

require :: Bool -> String -> IO ()
require condition message = unless condition (ioError (userError message))

resolve :: Workspace -> FilePath -> FilePath
resolve (Workspace root) path = if isAbsolute path then path else root </> path

-- | The supplied root, else @BUILD_WORKSPACE_DIRECTORY@, else the current directory.
workspace :: Maybe Workspace -> IO Workspace
workspace supplied = do
  root <- case supplied of
    Just (Workspace path) -> pure path
    Nothing -> lookupEnv "BUILD_WORKSPACE_DIRECTORY" >>= maybe getCurrentDirectory pure
  Workspace <$> canonicalizePath root

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

freshRuntime :: Workspace -> Maybe Runtime -> DefaultRuntime -> IO Runtime
freshRuntime root supplied fallback = do
  private <- canonicalizePath (resolve root "private-data")
  path <- case (supplied, fallback) of
    (Just (Runtime value), _) -> pure (resolve root value)
    (Nothing, ReplacedRuntime relative) -> do
      let path = private </> relative
      createDirectoryIfMissing True (takeDirectory path)
      exists <- doesDirectoryExist path
      when exists $ do
        linked <- pathIsSymbolicLink path
        require (not linked) ("the runtime directory must not be a symlink: " ++ path)
        removeDirectoryRecursive path
      pure path
    (Nothing, UniqueRuntime prefix) -> do
      pid <- getProcessID
      micros <- (round . (* 1000000) <$> getPOSIXTime) :: IO Integer
      pure (private </> (prefix ++ "-" ++ show pid ++ "-" ++ show micros))
  require (takeFileName path `notElem` ["", ".", ".."]) "--output must name a fresh directory"
  parent <- canonicalizePath (takeDirectory path)
  require (splitDirectories private `isPrefixOf` splitDirectories parent) "--output must be beneath workspace/private-data/"
  let destination = parent </> takeFileName path
  createDirectory destination
  setFileMode destination 0o700
  pure (Runtime destination)

-- | Container compatibility with the retained 1.50 layout and a bounded image-4
-- prefix. A passing container is not evidence that its application boots.
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

-- | Stages a fresh runtime and makes it the current directory. Without a
-- selected DAT the pinned stock 1.50 DAT is staged. The calling executable is
-- staged as @XM1_VIEWER_VALIDATOR@, so it must answer @--validate-firmware PATH@
-- with 'validateFirmware'.
stageViewerRuntime :: Workspace -> Maybe Runtime -> DefaultRuntime -> Maybe Firmware -> StockOriginal -> IO StagedRuntime
stageViewerRuntime root output fallback selected stock = do
  python <- requiredEnvironment "XM1_VIEWER_PYTHON"
  native <- requiredEnvironment "XM1_VIEWER_NATIVE_PACKAGE"
  unicorn <- requiredEnvironment "X70_UNICORN_LIBRARY"
  let source = resolve root "emulation/xm1-upstream"
      stockDat = resolve root "private-data/firmware/x-m1-1.50/FWUP0002.DAT"
      dat = maybe stockDat (\(Firmware path) -> resolve root path) selected
      rom = resolve root "private-data/analysis/x-m1-1.50/bootrom/bootrom-0x80000000-4000.bin"
      stockInputs = [stockDat | stock == StageStockOriginal]
  forM_ ([python, native </> "x70_native/_native.abi3.so", unicorn, dat, rom, source </> "tools/start_setup_viewer.py"] ++ stockInputs) $ \path -> do
    exists <- doesFileExist path
    require exists ("Required viewer input is missing: " ++ path)
  byteCount <- getFileSize dat
  require (byteCount > 0 && byteCount <= 64 * 1024 * 1024) "DAT must be 1..64 MiB"
  Runtime destination <- freshRuntime root output fallback
  forM_ ["tools", "emulator", "firmware", "native"] $ \directory ->
    copyTree (source </> directory) (destination </> directory)
  stageFile (native </> "x70_native/_native.abi3.so") (destination </> "native/python/x70_native/_native.abi3.so")
  stageFile dat (destination </> stagedFirmwarePath)
  stageFile rom (destination </> stagedRomPath)
  forM_ stockInputs $ \path ->
    stageFile path (destination </> "firmware/x-m1/1.50/original/FWUP0002.DAT")
  setEnv "PYTHONPATH" ((destination </> "native/python") ++ ":" ++ destination)
  setEnv "PYTHONNOUSERSITE" "1"
  setEnv "PYTHONDONTWRITEBYTECODE" "1"
  setCurrentDirectory destination
  validator <- getExecutablePath
  stageFile validator (destination </> "viewer-validator")
  setFileMode (destination </> "viewer-validator") 0o700
  setEnv "XM1_VIEWER_VALIDATOR" (destination </> "viewer-validator")
  pure (StagedRuntime destination (Python python) unicorn (Firmware dat))
