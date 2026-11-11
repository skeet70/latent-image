-- | Behavioural checks for runtime staging: the staged file carries the source
-- bytes, keeps the source permissions, and is independent of the source.
module Main (main) where

import Control.Exception (IOException, try)
import Control.Monad (unless, when)
import Data.Bits ((.&.))
import qualified Data.ByteString as B
import Emulation.ViewerRuntime (DefaultRuntime (..), Runtime (..), Workspace (..), freshRuntime, stageFile)
import Data.List (isPrefixOf)
import System.Directory (canonicalizePath, createDirectoryIfMissing, doesDirectoryExist, doesFileExist, getTemporaryDirectory, removeDirectoryRecursive)
import System.Environment (lookupEnv, setEnv, unsetEnv)
import System.Exit (exitFailure)
import System.FilePath ((</>), takeDirectory, takeFileName)
import System.Posix.Files (fileMode, getFileStatus, setFileMode)

check :: String -> Bool -> IO ()
check label passed = unless passed (putStrLn ("FAIL: " ++ label) >> exitFailure)

main :: IO ()
main = do
  scratch <- lookupEnv "TEST_TMPDIR" >>= maybe getTemporaryDirectory pure
  let root = scratch </> "viewer-runtime-staging"
      source = root </> "source.DAT"
      staged = root </> "staged" </> "inputs" </> "selected.DAT"
      second = root </> "staged" </> "firmware" </> "FWUP0002.DAT"
      missing = root </> "missing.DAT"
      absent = root </> "staged" </> "inputs" </> "absent.DAT"
      bytes = B.pack [fromIntegral (index `mod` 251) | index <- [0 .. 262143 :: Int]]
  existing <- doesDirectoryExist root
  when existing (removeDirectoryRecursive root)
  createDirectoryIfMissing True root
  B.writeFile source bytes
  setFileMode source 0o640
  stageFile source staged
  stagedBytes <- B.readFile staged
  stagedMode <- (.&. 0o777) . fileMode <$> getFileStatus staged
  check "the staged file holds the source bytes" (stagedBytes == bytes)
  check "the staged file keeps the source permissions" (stagedMode == 0o640)
  B.appendFile staged (B.pack [0 .. 9])
  sourceBytes <- B.readFile source
  check "writing to the staged file leaves the source unchanged" (sourceBytes == bytes)
  stageFile source second
  secondBytes <- B.readFile second
  check "staging a second time succeeds with the same bytes" (secondBytes == bytes)
  missingResult <- try (stageFile missing absent) :: IO (Either IOException ())
  absentExists <- doesFileExist absent
  check "staging a missing source fails without leaving a destination" $
    case missingResult of
      Left _ -> not absentExists
      Right _ -> False
  let private = root </> "private-data"
      work = private </> "scratch" </> "task" </> "runs" </> "run" </> "work"
  createDirectoryIfMissing True work
  setEnv "XM1_RUN_WORK" work
  Runtime underWork <- freshRuntime (Workspace root) Nothing (UniqueRuntime "viewer-runtime-test")
  canonicalWork <- canonicalizePath work
  check "a run work directory holds the fresh runtime" (takeDirectory underWork == canonicalWork)
  check "the fresh runtime keeps its unique name" ("viewer-runtime-test-" `isPrefixOf` takeFileName underWork)
  unsetEnv "XM1_RUN_WORK"
  Runtime defaultRuntime <- freshRuntime (Workspace root) Nothing (UniqueRuntime "viewer-runtime-test")
  canonicalPrivate <- canonicalizePath private
  check "without a run work directory the fresh runtime stays under private-data" (takeDirectory defaultRuntime == canonicalPrivate)
  setEnv "XM1_RUN_WORK" (root </> "absent-work")
  Runtime absentRuntime <- freshRuntime (Workspace root) Nothing (UniqueRuntime "viewer-runtime-test")
  check "an absent run work directory falls back to private-data" (takeDirectory absentRuntime == canonicalPrivate)
  unsetEnv "XM1_RUN_WORK"
  removeDirectoryRecursive root
  putStrLn "viewer-runtime staging: checks passed"
