{-# LANGUAGE OverloadedStrings #-}
{-# LANGUAGE ScopedTypeVariables #-}
module Main (main) where

import Control.Exception (bracket)
import Control.Monad (unless)
import Data.Aeson (FromJSON (parseJSON), eitherDecodeStrict', withObject, (.:))
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as BC
import Data.Time.Clock.POSIX (POSIXTime, getPOSIXTime)
import System.Directory (canonicalizePath, createDirectoryIfMissing, doesPathExist, getTemporaryDirectory, removePathForcibly)
import System.Environment (getArgs)
import System.Exit (ExitCode (..))
import System.FilePath ((</>), takeDirectory)
import System.Posix.Files (createSymbolicLink, setSymbolicLinkTimesHiRes)
import System.Posix.Temp (mkdtemp)
import System.Process (readProcessWithExitCode)

data Summary = Summary
  { summaryApplied :: Bool
  , summaryMaxAgeDays :: Integer
  , summaryExpiredFiles :: Integer
  , summaryExpiredBytes :: Integer
  , summaryPinnedFiles :: Integer
  , summaryPinnedBytes :: Integer
  , summaryRetainedFiles :: Integer
  , summaryRetainedBytes :: Integer
  , summaryOwnedDirectories :: Integer
  , summaryRemovedDirectories :: Integer
  , summaryFailures :: Integer
  }

instance FromJSON Summary where
  parseJSON = withObject "summary" $ \object' ->
    Summary
      <$> object' .: "applied"
      <*> object' .: "max_age_days"
      <*> object' .: "expired_files"
      <*> object' .: "expired_bytes"
      <*> object' .: "pinned_files"
      <*> object' .: "pinned_bytes"
      <*> object' .: "retained_files"
      <*> object' .: "retained_bytes"
      <*> object' .: "owned_directories"
      <*> object' .: "removed_directories"
      <*> object' .: "failures"

check :: Bool -> String -> IO ()
check ok message = unless ok (ioError (userError message))

main :: IO ()
main = do
  arguments <- getArgs
  case arguments of
    [input] -> runTests input
    _ -> ioError (userError "expected the scratch-expiry executable")

runTests :: FilePath -> IO ()
runTests input = do
  executable <- canonicalizePath input
  temporary <- getTemporaryDirectory
  bracket (mkdtemp (temporary </> "scratch-expiry-")) removePathForcibly $ \parent -> do
    now <- getPOSIXTime
    let future = floor now + 8 * 86400 :: Integer
        current = floor now :: Integer
        afterReference = fromIntegral (future + 86400) :: POSIXTime

    fixture parent "dry-run" $ \workspace -> do
      let old = workspace </> "private-data/scratch/old.bin"
      B.writeFile old "old"
      age old
      dry <- runTool executable workspace 7 False future
      check (summaryApplied dry == False) "dry run reported applied"
      check (summaryExpiredFiles dry == 1) "dry run did not report the expired file"
      check (summaryExpiredBytes dry == 3) "dry run reported the wrong expired size"
      present <- doesPathExist old
      check present "dry run removed the expired file"
      appliedRun <- runTool executable workspace 7 True future
      check (summaryApplied appliedRun == True) "apply did not report applied"
      check (summaryExpiredFiles appliedRun == 1) "apply reported the wrong expired count"
      gone <- doesPathExist old
      check (not gone) "expired file survived apply"

    fixture parent "recent" $ \workspace -> do
      let recent = workspace </> "private-data/scratch/recent.bin"
      B.writeFile recent "recent"
      summary <- runTool executable workspace 7 True current
      check (summaryExpiredFiles summary == 0) "recent file expired"
      present <- doesPathExist recent
      check present "recent file was removed"

    fixture parent "cited" $ \workspace -> do
      let old = workspace </> "private-data/scratch/cited.bin"
      B.writeFile old "cited"
      age old
      commit workspace "notes.md" "see (private-data/scratch/cited.bin)\n"
      summary <- runTool executable workspace 7 True future
      check (summaryExpiredFiles summary == 0) "cited file expired"
      check (summaryPinnedFiles summary == 1) "cited file was not reported pinned"
      present <- doesPathExist old
      check present "cited file was removed"

    fixture parent "branch-citation" $ \workspace -> do
      git workspace ["checkout", "-q", "-b", "cites"]
      commit workspace "notes.md" "private-data/scratch/branch.bin\n"
      git workspace ["checkout", "-q", "main"]
      let old = workspace </> "private-data/scratch/branch.bin"
      B.writeFile old "branch cited"
      age old
      summary <- runTool executable workspace 7 True future
      check (summaryExpiredFiles summary == 0) "branch-cited file expired"
      check (summaryPinnedFiles summary == 1) "branch-cited file was not reported pinned"
      present <- doesPathExist old
      check present "branch-cited file was removed"

    fixture parent "baselines" $ \workspace -> do
      let kept = workspace </> "private-data/verify-candidate/baselines/keep.bin"
      createDirectoryIfMissing True (takeDirectory kept)
      B.writeFile kept "baseline"
      age kept
      summary <- runTool executable workspace 7 True future
      check (summaryExpiredFiles summary == 0) "baseline file expired"
      check (summaryPinnedFiles summary == 0) "baseline file was counted as pinned"
      check (summaryRetainedFiles summary == 0) "baseline file was counted as retained"
      present <- doesPathExist kept
      check present "baseline file was removed"

    fixture parent "analysis" $ \workspace -> do
      let kept = workspace </> "private-data/analysis/keep.bin"
      createDirectoryIfMissing True (takeDirectory kept)
      B.writeFile kept "analysis"
      age kept
      summary <- runTool executable workspace 7 True future
      check (summaryExpiredFiles summary == 0) "analysis file expired"
      present <- doesPathExist kept
      check present "analysis file was removed"

    fixture parent "symlink" $ \workspace -> do
      let outside = workspace </> "outside"
          target = outside </> "keep.bin"
          link = workspace </> "private-data/scratch/link"
      createDirectoryIfMissing True outside
      B.writeFile target "target"
      createSymbolicLink outside link
      age link
      summary <- runTool executable workspace 7 True future
      check (summaryExpiredFiles summary == 1) "symlink did not expire"
      linked <- doesPathExist link
      check (not linked) "symlink survived apply"
      present <- doesPathExist target
      check present "symlink target was removed"
      contents <- B.readFile target
      check (contents == "target") "symlink target changed"

    fixture parent "empty-directories" $ \workspace -> do
      let nested = workspace </> "private-data/scratch/nested/deep"
          old = nested </> "old.bin"
      createDirectoryIfMissing True nested
      B.writeFile old "old"
      age old
      summary <- runTool executable workspace 7 True future
      check (summaryRemovedDirectories summary == 2) "wrong removed-directory count"
      nestedExists <- doesPathExist nested
      check (not nestedExists) "emptied nested directory survived"
      scratchExists <- doesPathExist (workspace </> "private-data/scratch")
      verifyExists <- doesPathExist (workspace </> "private-data/verify-candidate")
      check (scratchExists && verifyExists) "a sweep root was removed"

    do
      let raw = parent </> "linked-workspace"
          outside = parent </> "linked-data"
      createDirectoryIfMissing True raw
      createDirectoryIfMissing True outside
      workspace <- canonicalizePath raw
      git workspace ["init", "-q", "-b", "main"]
      createSymbolicLink outside (workspace </> "private-data")
      (status, output, errors) <- runRaw executable ["--workspace", workspace, "--max-age-days", "7", "--now", show future, "--apply"]
      check (status /= ExitSuccess) "symlinked private-data was accepted"
      check (null output) "refusal wrote to stdout"
      check (length (lines errors) == 1) "refusal reason is not one line"

    fixture parent "keep-list" $ \workspace -> do
      commit workspace "build-support/scratch-keep.txt" "# fixture\nprivate-data/scratch/kept\n"
      let old = workspace </> "private-data/scratch/kept/sub/old.bin"
      createDirectoryIfMissing True (takeDirectory old)
      B.writeFile old "kept"
      age old
      summary <- runTool executable workspace 7 True future
      check (summaryPinnedFiles summary == 1) "keep-list subtree file was not pinned"
      check (summaryPinnedBytes summary == 4) "keep-list subtree file reported the wrong size"
      present <- doesPathExist old
      check present "keep-list subtree file was removed"

    fixture parent "cited-directory" $ \workspace -> do
      commit workspace "notes.md" "private-data/scratch/cited-dir/\n"
      let old = workspace </> "private-data/scratch/cited-dir/old.bin"
      createDirectoryIfMissing True (takeDirectory old)
      B.writeFile old "not pinned"
      age old
      summary <- runTool executable workspace 7 True future
      check (summaryExpiredFiles summary == 1) "file under a cited directory did not expire"
      check (summaryPinnedFiles summary == 0) "cited directory pinned a file"
      gone <- doesPathExist old
      check (not gone) "file under a cited directory was kept"

    fixture parent "branch-keep" $ \workspace -> do
      git workspace ["checkout", "-q", "-b", "keep-branch"]
      commit workspace "build-support/scratch-keep.txt" "# fixture\nprivate-data/scratch/branch-kept\n"
      git workspace ["checkout", "-q", "main"]
      let old = workspace </> "private-data/scratch/branch-kept/deep/old.bin"
      createDirectoryIfMissing True (takeDirectory old)
      B.writeFile old "branch kept"
      age old
      summary <- runTool executable workspace 7 True future
      check (summaryPinnedFiles summary == 1) "branch keep-list entry did not pin its subtree"
      present <- doesPathExist old
      check present "branch keep-list subtree file was removed"

    fixture parent "branch-owned" $ \workspace -> do
      git workspace ["checkout", "-q", "-b", "agent/Foo"]
      git workspace ["checkout", "-q", "main"]
      let owned = workspace </> "private-data/scratch/Foo/old.bin"
      createDirectoryIfMissing True (takeDirectory owned)
      B.writeFile owned "owned"
      age owned
      kept <- runTool executable workspace 7 True future
      check (summaryOwnedDirectories kept == 1) "branch-owned directory was not reported"
      check (summaryExpiredFiles kept == 0) "file inside a branch-owned directory expired"
      present <- doesPathExist owned
      check present "file inside a branch-owned directory was removed"
      git workspace ["branch", "-D", "agent/Foo"]
      released <- runTool executable workspace 7 True future
      check (summaryOwnedDirectories released == 0) "deleted branch still owns its directory"
      check (summaryExpiredFiles released == 1) "released file did not expire"
      gone <- doesPathExist owned
      check (not gone) "released file survived"

    fixture parent "suffix-owned" $ \workspace -> do
      git workspace ["checkout", "-q", "-b", "agent/Foo"]
      git workspace ["checkout", "-q", "main"]
      let owned = workspace </> "private-data/scratch/Foo-20261108/old.bin"
      createDirectoryIfMissing True (takeDirectory owned)
      B.writeFile owned "suffix owned"
      age owned
      summary <- runTool executable workspace 7 True future
      check (summaryOwnedDirectories summary == 1) "suffix-owned directory was not reported"
      check (summaryExpiredFiles summary == 0) "file inside a suffix-owned directory expired"
      present <- doesPathExist owned
      check present "file inside a suffix-owned directory was removed"

    fixture parent "prefix-not-owned" $ \workspace -> do
      git workspace ["checkout", "-q", "-b", "agent/Foo"]
      git workspace ["checkout", "-q", "main"]
      let old = workspace </> "private-data/scratch/Foobar/old.bin"
      createDirectoryIfMissing True (takeDirectory old)
      B.writeFile old "not owned"
      age old
      summary <- runTool executable workspace 7 True future
      check (summaryOwnedDirectories summary == 0) "a shared prefix was treated as ownership"
      check (summaryExpiredFiles summary == 1) "file under an unowned prefix did not expire"
      gone <- doesPathExist old
      check (not gone) "file under an unowned prefix survived"

    fixture parent "worktree-owned" $ \workspace -> do
      git workspace ["worktree", "add", "-q", "-b", "other", parent </> "Bar", "main"]
      let owned = workspace </> "private-data/scratch/Bar/old.bin"
      createDirectoryIfMissing True (takeDirectory owned)
      B.writeFile owned "worktree owned"
      age owned
      summary <- runTool executable workspace 7 True future
      check (summaryOwnedDirectories summary == 1) "worktree basename did not own its directory"
      check (summaryExpiredFiles summary == 0) "file inside a worktree-owned directory expired"
      present <- doesPathExist owned
      check present "file inside a worktree-owned directory was removed"

    fixture parent "counts" $ \workspace -> do
      let scratch = workspace </> "private-data/scratch"
          old = scratch </> "old.bin"
          pinned = scratch </> "pinned.bin"
          recent = scratch </> "recent.bin"
          baseline = workspace </> "private-data/verify-candidate/baselines/keep.bin"
      B.writeFile old "old\n"
      B.writeFile pinned "keep\n"
      B.writeFile recent "fresh\n"
      createDirectoryIfMissing True (takeDirectory baseline)
      B.writeFile baseline "base\n"
      age old
      age pinned
      age baseline
      setSymbolicLinkTimesHiRes recent afterReference afterReference
      commit workspace "notes.md" "private-data/scratch/pinned.bin\n"
      dry <- runTool executable workspace 7 False future
      check (summaryApplied dry == False) "count fixture dry run reported applied"
      check (summaryMaxAgeDays dry == 7) "count fixture reported the wrong max age"
      check (summaryExpiredFiles dry == 1) "count fixture expired count"
      check (summaryExpiredBytes dry == 4) "count fixture expired bytes"
      check (summaryPinnedFiles dry == 1) "count fixture pinned count"
      check (summaryPinnedBytes dry == 5) "count fixture pinned bytes"
      check (summaryRetainedFiles dry == 1) "count fixture retained count"
      check (summaryRetainedBytes dry == 6) "count fixture retained bytes"
      check (summaryRemovedDirectories dry == 0) "count fixture removed directories in dry run"
      check (summaryFailures dry == 0) "count fixture reported failures"
      appliedRun <- runTool executable workspace 7 True future
      check (summaryExpiredFiles appliedRun == 1) "count fixture apply expired count"
      gone <- doesPathExist old
      check (not gone) "count fixture expired file survived apply"
      pinnedPresent <- doesPathExist pinned
      recentPresent <- doesPathExist recent
      check (pinnedPresent && recentPresent) "count fixture removed a retained file"

fixture :: FilePath -> String -> (FilePath -> IO ()) -> IO ()
fixture parent name action = do
  let raw = parent </> name
  createDirectoryIfMissing True raw
  workspace <- canonicalizePath raw
  git workspace ["init", "-q", "-b", "main"]
  git workspace ["config", "user.email", "fixture@example.invalid"]
  git workspace ["config", "user.name", "Fixture"]
  createDirectoryIfMissing True (workspace </> "build-support")
  B.writeFile (workspace </> "README.md") "fixture\n"
  B.writeFile (workspace </> "build-support/scratch-keep.txt") "# fixture keep list\n"
  git workspace ["add", "README.md", "build-support/scratch-keep.txt"]
  git workspace ["commit", "-q", "-m", "fixture"]
  createDirectoryIfMissing True (workspace </> "private-data/scratch")
  createDirectoryIfMissing True (workspace </> "private-data/verify-candidate")
  action workspace

git :: FilePath -> [String] -> IO ()
git workspace arguments = do
  (status, output, errors) <- readProcessWithExitCode "git" (["-C", workspace] ++ arguments) ""
  unless (status == ExitSuccess) (ioError (userError ("git " ++ unwords arguments ++ ": " ++ output ++ errors)))

commit :: FilePath -> FilePath -> B.ByteString -> IO ()
commit workspace relative content = do
  let path = workspace </> relative
  createDirectoryIfMissing True (takeDirectory path)
  B.writeFile path content
  git workspace ["add", relative]
  git workspace ["commit", "-q", "-m", relative]

ancient :: POSIXTime
ancient = 1000000000

age :: FilePath -> IO ()
age path = setSymbolicLinkTimesHiRes path ancient ancient

runRaw :: FilePath -> [String] -> IO (ExitCode, String, String)
runRaw executable arguments = readProcessWithExitCode executable arguments ""

runTool :: FilePath -> FilePath -> Integer -> Bool -> Integer -> IO Summary
runTool executable workspace days applied nowSeconds = do
  let arguments =
        ["--workspace", workspace, "--max-age-days", show days, "--now", show nowSeconds]
          ++ (if applied then ["--apply"] else [])
  (status, output, errors) <- runRaw executable arguments
  unless (status == ExitSuccess) (ioError (userError (unwords arguments ++ ": " ++ errors)))
  case eitherDecodeStrict' (BC.pack output) of
    Left problem -> ioError (userError ("invalid summary: " ++ problem ++ ": " ++ output))
    Right summary -> pure summary
