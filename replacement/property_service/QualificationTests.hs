module Main (main) where

import Control.Exception (IOException, bracket, try)
import Control.Monad (unless)
import qualified Data.ByteString as B
import Data.Either (isLeft)
import Replacement.PropertyService.Qualification
import System.Directory (createDirectory, createDirectoryIfMissing, createDirectoryLink, getTemporaryDirectory, removeDirectoryRecursive, removeFile, withCurrentDirectory)
import System.Environment (lookupEnv, setEnv, unsetEnv)
import System.FilePath ((</>))
import System.IO (hClose, openTempFile)

check :: String -> Bool -> IO ()
check label passed = unless passed (fail label)

main :: IO ()
main = do
  check "positive decimal storage limit" (parseStorageLimit "75161927680" == Right (StorageLimitBytes 75161927680))
  check "invalid storage limits are refused" (all (isLeft . parseStorageLimit) ["", "0", "-1", "+1", "1G", " 1", "1 "])
  check "positive batch checkpoint" (parseBatchCheckpoint "1" == Right (BatchCheckpoint 1))
  check "overflowing batch checkpoint is refused" (isLeft (parseBatchCheckpoint (show (toInteger (maxBound :: Int) + 1))))
  bracket temporary removeDirectoryRecursive $ \base -> do
    let root = base </> "root"
        outside = base </> "outside"
        admission = StorageAdmission root (StorageLimitBytes 75161927680)
        rejected path = do
          result <- try (checkStoragePaths admission [path]) :: IO (Either IOException ())
          check ("outside path admitted: " ++ path) (isLeft result)
    createDirectory root
    createDirectory outside
    checkStoragePaths admission [root </> "fresh-run", root </> "cache"]
    rejected root
    rejected (base </> "root-other" </> "run")
    rejected (root </> ".." </> "outside" </> "run")
    createDirectoryLink outside (root </> "escape")
    rejected (root </> "escape" </> "run")
    let runRoot = base </> "run-root"
        unrelated = runRoot </> "unrelated"
        runOut = runRoot </> "out"
        runCache = runRoot </> "cache"
        runAdmission = StorageAdmission runRoot (StorageLimitBytes (2 * 1024 ^ (3 :: Int) + 1024 * 1024))
    createDirectoryIfMissing True unrelated
    createDirectoryIfMissing True runOut
    createDirectoryIfMissing True runCache
    B.writeFile (unrelated </> "foreign.bin") (B.replicate (8 * 1024 * 1024) 0)
    StorageSnapshot (StorageBytes measured) _ _ <- measureRunStorage runAdmission [runOut, runCache]
    check "the storage checkpoint measures only the run's own directories" (measured == 0)
    storageCheckpoint BeforeBatch runAdmission [runOut, runCache]
    B.writeFile (runOut </> "batch.bin") (B.replicate (2 * 1024 * 1024) 0)
    overLimit <- try (storageCheckpoint BeforeBatch runAdmission [runOut, runCache]) :: IO (Either IOException ())
    check "the run's own allocation over the limit is refused" (isLeft overLimit)
    overlapping <- try (measureRunStorage runAdmission [runOut, runOut </> "cache"]) :: IO (Either IOException StorageSnapshot)
    check "overlapping measured paths are refused" (isLeft overlapping)
    let runfiles = base </> "runfiles"
        workspace = base </> "workspace"
        input = "property-runtime-fixture.bin"
        bundled = runfiles </> "_main" </> input
    createDirectoryIfMissing True (runfiles </> "_main")
    createDirectory workspace
    writeFile bundled "bundled"
    bracket (lookupEnv "RUNFILES_DIR") (maybe (unsetEnv "RUNFILES_DIR") (setEnv "RUNFILES_DIR")) $ \_ -> do
      setEnv "RUNFILES_DIR" runfiles
      withCurrentDirectory workspace $ do
        resolved <- resolveRuntimePath input
        contents <- readFile resolved
        check "bundled runtime input remains available from a workspace invocation" (contents == "bundled")
        writeFile input "explicit"
        direct <- resolveRuntimePath input >>= readFile
        check "an explicit local input precedes bundled defaults" (direct == "explicit")
        absolute <- resolveRuntimePath bundled >>= readFile
        check "absolute runtime inputs retain their supplied bytes" (absolute == "bundled")
        missing <- try (resolveRuntimePath "absent-runtime-input.bin") :: IO (Either IOException FilePath)
        check "missing runtime inputs are refused" (isLeft missing)
  where
    temporary = do
      parent <- getTemporaryDirectory
      (path, handle) <- openTempFile parent "property-storage-boundary"
      hClose handle
      removeFile path
      createDirectory path
      pure path
