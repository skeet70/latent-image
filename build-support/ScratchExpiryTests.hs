{-# LANGUAGE OverloadedStrings #-}
{-# LANGUAGE ScopedTypeVariables #-}
module Main (main) where

import Control.Exception (bracket, finally)
import Control.Monad (unless)
import Data.Aeson (FromJSON (..), eitherDecodeStrict', encode, object, withObject, (.:), (.:?), (.=))
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as BC
import qualified Data.ByteString.Lazy as BL
import Data.List (isPrefixOf)
import Data.Time.Calendar (Day, addDays)
import Data.Time.Clock (utctDay)
import Data.Time.Clock.POSIX (POSIXTime, getPOSIXTime, posixSecondsToUTCTime)
import Data.Time.Format (defaultTimeLocale, formatTime)
import Foreign.C.Types (CInt (..))
import System.Directory (canonicalizePath, createDirectoryIfMissing, doesPathExist, getTemporaryDirectory, listDirectory, removePathForcibly)
import System.Environment (getArgs, getEnvironment)
import System.Exit (ExitCode (..))
import System.FilePath ((</>), takeDirectory)
import System.Posix.Files (createSymbolicLink, setFileMode, setFileSize, setSymbolicLinkTimesHiRes)
import System.Posix.IO (OpenMode (ReadWrite), closeFd, defaultFileFlags, openFd)
import System.Posix.Temp (mkdtemp)
import System.Posix.Types (Fd (..))
import System.Process (CreateProcess (..), proc, readCreateProcessWithExitCode, readProcessWithExitCode)

foreign import ccall unsafe "flock" c_flock :: CInt -> CInt -> IO CInt

lockExclusiveNonBlocking :: CInt
lockExclusiveNonBlocking = 2 + 4

data Summary = Summary
  { summaryApplied :: Bool
  , summaryStopped :: Bool
  , summaryMaxAgeDays :: Integer
  , summaryWorkAgeHours :: Integer
  , summaryExpiredFiles :: Integer
  , summaryExpiredBytes :: Integer
  , summaryExpiredWorkRuns :: Integer
  , summaryClosedAbandonedRuns :: Integer
  , summaryPinnedFiles :: Integer
  , summaryPinnedBytes :: Integer
  , summaryRetainedFiles :: Integer
  , summaryRetainedBytes :: Integer
  , summaryOwnedDirectories :: Integer
  , summaryRemovedDirectories :: Integer
  , summaryFailures :: Integer
  }

instance FromJSON Summary where
  parseJSON = withObject "summary" $ \summaryObject ->
    Summary
      <$> summaryObject .: "applied"
      <*> summaryObject .: "stopped"
      <*> summaryObject .: "max_age_days"
      <*> summaryObject .: "work_age_hours"
      <*> summaryObject .: "expired_files"
      <*> summaryObject .: "expired_bytes"
      <*> summaryObject .: "expired_work_runs"
      <*> summaryObject .: "closed_abandoned_runs"
      <*> summaryObject .: "pinned_files"
      <*> summaryObject .: "pinned_bytes"
      <*> summaryObject .: "retained_files"
      <*> summaryObject .: "retained_bytes"
      <*> summaryObject .: "owned_directories"
      <*> summaryObject .: "removed_directories"
      <*> summaryObject .: "failures"

check :: Bool -> String -> IO ()
check ok message = unless ok (ioError (userError message))

main :: IO ()
main = do
  arguments <- getArgs
  case arguments of
    [input] -> runTests input Nothing
    [input, runOutput] -> runTests input (Just runOutput)
    _ -> ioError (userError "expected the scratch-expiry executable and optionally the run-output executable")

runTests :: FilePath -> Maybe FilePath -> IO ()
runTests input relativeRunOutput = do
  executable <- canonicalizePath input
  maybeRunOutput <- traverse canonicalizePath relativeRunOutput
  temporary <- getTemporaryDirectory
  bracket (mkdtemp (temporary </> "scratch-expiry-")) removePathForcibly $ \parent -> do
    now <- getPOSIXTime
    let current = floor now :: Integer
        future = current + 8 * 86400
        today = utctDay (posixSecondsToUTCTime (fromIntegral future))

    fixture parent "stop-file" $ \workspace -> do
      let old = workspace </> "private-data/scratch/old.bin"
      B.writeFile old "old"
      age old
      B.writeFile (workspace </> "private-data/scratch/NO-EXPIRY") ""
      summary <- runTool executable workspace 7 True future
      check (summaryStopped summary) "stop file did not stop the sweep"
      check (summaryExpiredFiles summary == 0) "stopped sweep reported expired files"
      check (summaryExpiredBytes summary == 0) "stopped sweep reported expired bytes"
      check (summaryExpiredWorkRuns summary == 0) "stopped sweep reported expired runs"
      check (summaryOwnedDirectories summary == 0) "stopped sweep reported owned directories"
      check (summaryPinnedFiles summary == 0) "stopped sweep reported pinned files"
      check (summaryRetainedFiles summary == 0) "stopped sweep reported retained files"
      present <- doesPathExist old
      check present "stopped sweep removed a file"

    fixture parent "baselines-prefix" $ \workspace -> do
      let kept = workspace </> "private-data/verify-candidate/baselines-select/keep.bin"
      createDirectoryIfMissing True (takeDirectory kept)
      B.writeFile kept "baseline"
      age kept
      summary <- runTool executable workspace 7 True future
      check (summaryExpiredFiles summary == 0) "baselines-prefixed file expired"
      check (summaryPinnedFiles summary == 0) "baselines-prefixed file was counted as pinned"
      check (summaryRetainedFiles summary == 0) "baselines-prefixed file was counted as retained"
      present <- doesPathExist kept
      check present "baselines-prefixed file was removed"

    fixture parent "noncanonical-workspace" $ \workspace -> do
      let old = workspace </> "private-data/scratch/old.bin"
          alias = parent </> "noncanonical-workspace-alias"
      B.writeFile old "old"
      age old
      createSymbolicLink workspace alias
      summary <- runTool executable alias 7 True future
      check (summaryExpiredFiles summary == 1) "a non-canonical workspace was not swept"
      gone <- doesPathExist old
      check (not gone) "a non-canonical workspace left the file"

    fixture parent "branch-owned" $ \workspace -> do
      git workspace ["checkout", "-q", "-b", "agent/Foo"]
      commitAt workspace (future - 86400) "marker.txt" "owned\n"
      git workspace ["checkout", "-q", "main"]
      let owned = workspace </> "private-data/scratch/Foo/old.bin"
      createDirectoryIfMissing True (takeDirectory owned)
      B.writeFile owned "owned"
      age owned
      kept <- runTool executable workspace 7 True future
      check (summaryOwnedDirectories kept == 1) "recent branch commit did not own its directory"
      check (summaryExpiredFiles kept == 0) "file inside a branch-owned directory expired"
      present <- doesPathExist owned
      check present "file inside a branch-owned directory was removed"
      git workspace ["branch", "-D", "agent/Foo"]
      released <- runTool executable workspace 7 True future
      check (summaryOwnedDirectories released == 0) "deleted branch still owns its directory"
      check (summaryExpiredFiles released == 1) "released file did not expire"
      gone <- doesPathExist owned
      check (not gone) "released file survived"

    fixture parent "recent-file-owned" $ \workspace -> do
      let owned = workspace </> "private-data/scratch/Bar/old.bin"
          fresh = workspace </> "private-data/scratch/Bar/deep/fresh.bin"
          other = workspace </> "private-data/scratch/Baz/old.bin"
      createDirectoryIfMissing True (takeDirectory fresh)
      createDirectoryIfMissing True (takeDirectory other)
      B.writeFile owned "old"
      age owned
      B.writeFile fresh "fresh"
      setAgeSeconds fresh future
      B.writeFile other "other"
      age other
      summary <- runTool executable workspace 7 True future
      check (summaryOwnedDirectories summary == 1) "recent file did not own its directory"
      check (summaryExpiredFiles summary == 1) "old sibling expired or unrelated file was kept"
      ownedPresent <- doesPathExist owned
      freshPresent <- doesPathExist fresh
      otherGone <- doesPathExist other
      check (ownedPresent && freshPresent) "owned directory lost a file"
      check (not otherGone) "unowned directory was not swept"

    fixture parent "probe-errors" $ \workspace -> do
      let taskList = workspace </> "private-data/scratch/CannotList"
          hidden = taskList </> "hidden"
          oldList = taskList </> "old.bin"
          taskStat = workspace </> "private-data/scratch/CannotStat"
          noexec = taskStat </> "noexec"
          oldStat = taskStat </> "old.bin"
      createDirectoryIfMissing True hidden
      B.writeFile oldList "old"
      age oldList
      createDirectoryIfMissing True noexec
      B.writeFile (noexec </> "entry.bin") "entry"
      B.writeFile oldStat "old"
      age oldStat
      setFileMode hidden 0o000
      setFileMode noexec 0o600
      summary <- runTool executable workspace 7 True future
        `finally` do
          setFileMode hidden 0o700
          setFileMode noexec 0o700
      check (summaryOwnedDirectories summary == 2) "unreadable probe entries did not own their directories"
      check (summaryExpiredFiles summary == 0) "a file under an unreadable probe entry expired"
      check (summaryFailures summary == 0) "the ownership probe reported a failure"
      listPresent <- doesPathExist oldList
      statPresent <- doesPathExist oldStat
      check (listPresent && statPresent) "an unreadable probe directory lost a file"

    fixture parent "hold-valid" $ \workspace -> do
      let task = workspace </> "private-data/scratch/Held"
          old = task </> "old.bin"
      createDirectoryIfMissing True task
      B.writeFile old "held"
      age old
      B.writeFile (task </> "HOLD") (holdContent 117 (addDays 1 today))
      summary <- runTool executable workspace 7 True future
      check (summaryOwnedDirectories summary == 1) "valid HOLD did not own its directory"
      check (summaryExpiredFiles summary == 0) "file under a valid HOLD expired"
      present <- doesPathExist old
      check present "file under a valid HOLD was removed"

    fixture parent "hold-expired" $ \workspace -> do
      let task = workspace </> "private-data/scratch/Expired"
          old = task </> "old.bin"
      createDirectoryIfMissing True task
      B.writeFile old "expired hold"
      age old
      B.writeFile (task </> "HOLD") (holdContent 117 (addDays (-1) today))
      summary <- runTool executable workspace 7 True future
      check (summaryOwnedDirectories summary == 0) "expired HOLD owned its directory"
      check (summaryExpiredFiles summary == 2) "an expired HOLD and the file under it did not both expire"
      gone <- doesPathExist old
      check (not gone) "file under an expired HOLD survived"

    fixture parent "hold-too-long" $ \workspace -> do
      let task = workspace </> "private-data/scratch/Long"
          old = task </> "old.bin"
      createDirectoryIfMissing True task
      B.writeFile old "long hold"
      age old
      B.writeFile (task </> "HOLD") (holdContent 117 (addDays 25 today))
      before <- runTool executable workspace 7 True future
      check (summaryOwnedDirectories before == 1) "a clamped HOLD did not keep its directory before the clamp"
      check (summaryExpiredFiles before == 0) "a clamped HOLD expired before the clamp"
      present <- doesPathExist old
      check present "a clamped HOLD lost its file before the clamp"
      after <- runTool executable workspace 7 True (future + 23 * 86400)
      check (summaryOwnedDirectories after == 0) "a clamped HOLD still owned its directory after the clamp"
      check (summaryExpiredFiles after == 2) "a clamped HOLD did not expire after the clamp"
      gone <- doesPathExist old
      check (not gone) "a clamped HOLD kept its file after the clamp"

    fixture parent "work-unjudged" $ \workspace -> do
      before <- writeRun workspace "Task" "Before" "unjudged" (Just (future - 49 * 3600))
      after <- writeRun workspace "Task" "After" "unjudged" (Just (future - 47 * 3600))
      summary <- runTool executable workspace 7 True future
      check (summaryExpiredWorkRuns summary == 1) "wrong expired-run count for unjudged work"
      check (summaryExpiredBytes summary == 4) "unjudged work bytes were not reported"
      beforeGone <- doesPathExist (before </> "work")
      afterPresent <- doesPathExist (after </> "work")
      check (not beforeGone) "old unjudged work survived"
      check afterPresent "recent unjudged work was removed"
      checkRunFilesKept before
      checkRunFilesKept after

    fixture parent "work-passed" $ \workspace -> do
      run <- writeRun workspace "Task" "Passed" "passed" (Just future)
      summary <- runTool executable workspace 7 True future
      check (summaryExpiredWorkRuns summary == 1) "passed run work was not reported"
      gone <- doesPathExist (run </> "work")
      check (not gone) "passed run work survived"
      checkRunFilesKept run

    fixture parent "work-failed" $ \workspace -> do
      run <- writeRun workspace "Task" "Failed" "failed" (Just (future - 100 * 86400))
      summary <- runTool executable workspace 7 True future
      check (summaryExpiredWorkRuns summary == 0) "failed run work was reported"
      present <- doesPathExist (run </> "work")
      check present "failed run work was removed"
      checkRunFilesKept run

    fixture parent "work-open-held" $ \workspace -> do
      run <- writeRun workspace "Task" "Held" "open" Nothing
      setAgeSeconds (run </> "lock") (future - 49 * 3600)
      descriptor <- holdLock (run </> "lock")
      summary <- runTool executable workspace 7 True future
      closeFd descriptor
      check (summaryExpiredWorkRuns summary == 0) "held open run work was reported"
      present <- doesPathExist (run </> "work")
      check present "held open run work was removed"
      checkRunFilesKept run

    fixture parent "work-open-free" $ \workspace -> do
      run <- writeRun workspace "Task" "Free" "open" Nothing
      dry <- runTool executable workspace 7 False future
      check (summaryClosedAbandonedRuns dry == 1) "a free open run was not reported in the dry run"
      check (summaryExpiredWorkRuns dry == 0) "a free open run lost work in the dry run"
      recordBefore <- readRunFile (run </> "run.json")
      check (runFileOutcome recordBefore == "open") "the dry run rewrote an open run"
      workBefore <- doesPathExist (run </> "work")
      check workBefore "the dry run removed open work"
      appliedRun <- runTool executable workspace 7 True future
      check (summaryClosedAbandonedRuns appliedRun == 1) "a free open run was not closed"
      check (summaryExpiredWorkRuns appliedRun == 0) "closing a free open run removed its work"
      recordAfter <- readRunFile (run </> "run.json")
      check (runFileOutcome recordAfter == "unjudged") "the closed run was not marked unjudged"
      check (runFileClosedAt recordAfter == Just (isoTime future)) "the closed run has the wrong closed_at"
      workAfter <- doesPathExist (run </> "work")
      check workAfter "closing a free open run removed its work"
      checkRunFilesKept run
      later <- runTool executable workspace 7 True (future + 49 * 3600)
      check (summaryExpiredWorkRuns later == 1) "unjudged work from a closed run did not expire"
      gone <- doesPathExist (run </> "work")
      check (not gone) "unjudged work from a closed run survived"
      checkRunFilesKept run

    fixture parent "untrusted-run" $ \workspace -> do
      schemaRun <- writeRunWithJson workspace "Task" "Schema" (runJsonWith 2 "exec" "passed" (Just future))
      openerRun <- writeRunWithJson workspace "Task" "Opener" (runJsonWith 1 "other" "passed" (Just future))
      summary <- runTool executable workspace 7 True future
      check (summaryExpiredWorkRuns summary == 0) "an untrusted run was acted on"
      schemaWork <- doesPathExist (schemaRun </> "work")
      openerWork <- doesPathExist (openerRun </> "work")
      check (schemaWork && openerWork) "an untrusted run lost its work"

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

    fixture parent "cited-large" $ \workspace -> do
      let large = workspace </> "private-data/scratch/large.bin"
      writeLarge large
      commit workspace "notes.md" "private-data/scratch/large.bin\n"
      summary <- runTool executable workspace 7 True future
      check (summaryExpiredFiles summary == 1) "large cited file did not expire"
      check (summaryPinnedFiles summary == 0) "large cited file was reported pinned"
      gone <- doesPathExist large
      check (not gone) "large cited file survived"

    fixture parent "cited-large-open" $ \workspace -> do
      let large = workspace </> "private-data/scratch/large.bin"
      writeLarge large
      commit workspace "docs/evidence.jsonl" "{\"status\":\"open\",\"artifacts\":[{\"path\":\"private-data/scratch/large.bin\"}]}\n"
      summary <- runTool executable workspace 7 True future
      check (summaryPinnedFiles summary == 1) "open record did not pin the large file"
      check (summaryExpiredFiles summary == 0) "file cited by an open record expired"
      present <- doesPathExist large
      check present "file cited by an open record was removed"

    fixture parent "citation-removed" $ \workspace -> do
      let old = workspace </> "private-data/scratch/grace.bin"
      B.writeFile old "grace"
      age old
      commitAt workspace (future - 10 * 86400) "notes.md" "private-data/scratch/grace.bin\n"
      commitAt workspace (future - 86400) "notes.md" "no citations here\n"
      summary <- runTool executable workspace 7 True future
      check (summaryPinnedFiles summary == 1) "citation at the grace commit did not pin"
      check (summaryExpiredFiles summary == 0) "file cited at the grace commit expired"
      present <- doesPathExist old
      check present "file cited at the grace commit was removed"

    fixture parent "citation-stale-branch" $ \workspace -> do
      git workspace ["checkout", "-q", "-b", "stale"]
      commitAt workspace (future - 10 * 86400) "notes.md" "private-data/scratch/stale.bin\n"
      git workspace ["checkout", "-q", "main"]
      let old = workspace </> "private-data/scratch/stale.bin"
      B.writeFile old "stale"
      age old
      summary <- runTool executable workspace 7 True future
      check (summaryPinnedFiles summary == 0) "stale branch citation pinned a file"
      check (summaryExpiredFiles summary == 1) "file cited only by a stale branch did not expire"
      gone <- doesPathExist old
      check (not gone) "file cited only by a stale branch survived"

    fixture parent "citation-live-branch" $ \workspace -> do
      git workspace ["checkout", "-q", "-b", "live"]
      commitAt workspace (future - 86400) "notes.md" "private-data/scratch/live.bin\n"
      git workspace ["checkout", "-q", "main"]
      let old = workspace </> "private-data/scratch/live.bin"
      B.writeFile old "live"
      age old
      summary <- runTool executable workspace 7 True future
      check (summaryPinnedFiles summary == 1) "live branch citation did not pin a file"
      check (summaryExpiredFiles summary == 0) "file cited by a live branch expired"
      present <- doesPathExist old
      check present "file cited by a live branch was removed"

    fixture parent "origin-main-pin" $ \workspace -> do
      git workspace ["checkout", "-q", "-b", "cited"]
      commitAt workspace (future - 10 * 86400) "notes.md" "private-data/scratch/origin.bin\n"
      git workspace ["update-ref", "refs/remotes/origin/main", "HEAD"]
      git workspace ["checkout", "-q", "main"]
      let old = workspace </> "private-data/scratch/origin.bin"
      B.writeFile old "origin"
      age old
      summary <- runTool executable workspace 7 True future
      check (summaryPinnedFiles summary == 1) "origin/main citation did not pin"
      present <- doesPathExist old
      check present "origin/main-pinned file was removed"

    fixture parent "full-ref-branch" $ \workspace -> do
      git workspace ["checkout", "-q", "-b", "live"]
      commitAt workspace (future - 86400) "notes.md" "private-data/scratch/live.bin\n"
      git workspace ["tag", "live", "main"]
      git workspace ["checkout", "-q", "main"]
      let old = workspace </> "private-data/scratch/live.bin"
      B.writeFile old "live"
      age old
      summary <- runTool executable workspace 7 True future
      check (summaryPinnedFiles summary == 1) "a live branch citation did not pin with an ambiguous tag"
      present <- doesPathExist old
      check present "a live-branch-pinned file was removed"

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

    fixture parent "branch-keep" $ \workspace -> do
      git workspace ["checkout", "-q", "-b", "keep-branch"]
      commitAt workspace (future - 86400) "build-support/scratch-keep.txt" "# fixture\nprivate-data/scratch/branch-kept\n"
      git workspace ["checkout", "-q", "main"]
      let old = workspace </> "private-data/scratch/branch-kept/deep/old.bin"
      createDirectoryIfMissing True (takeDirectory old)
      B.writeFile old "branch kept"
      age old
      summary <- runTool executable workspace 7 True future
      check (summaryPinnedFiles summary == 1) "live branch keep-list entry did not pin its subtree"
      present <- doesPathExist old
      check present "live branch keep-list subtree file was removed"

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
      let raw = parent </> "no-head-workspace"
      createDirectoryIfMissing True raw
      workspace <- canonicalizePath raw
      git workspace ["init", "-q", "-b", "main"]
      createDirectoryIfMissing True (workspace </> "private-data/scratch")
      createDirectoryIfMissing True (workspace </> "private-data/verify-candidate")
      let old = workspace </> "private-data/scratch/old.bin"
      B.writeFile old "old"
      age old
      (status, output, errors) <- runRaw executable ["--workspace", workspace, "--max-age-days", "7", "--now", show future, "--apply"]
      check (status /= ExitSuccess) "a workspace without HEAD was accepted"
      check (null output) "HEAD failure wrote to stdout"
      check (length (lines errors) == 1) "HEAD failure reason is not one line"
      present <- doesPathExist old
      check present "a workspace without HEAD removed a file"

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
      setAgeSeconds recent (future + 3600)
      commit workspace "notes.md" "private-data/scratch/pinned.bin\n"
      dry <- runTool executable workspace 7 False future
      check (summaryApplied dry == False) "count fixture dry run reported applied"
      check (summaryStopped dry == False) "count fixture reported stopped"
      check (summaryMaxAgeDays dry == 7) "count fixture reported the wrong max age"
      check (summaryWorkAgeHours dry == 48) "count fixture reported the wrong work age"
      check (summaryExpiredFiles dry == 1) "count fixture expired count"
      check (summaryExpiredBytes dry == 4) "count fixture expired bytes"
      check (summaryExpiredWorkRuns dry == 0) "count fixture expired run count"
      check (summaryClosedAbandonedRuns dry == 0) "count fixture closed run count"
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

    case maybeRunOutput of
      Nothing -> pure ()
      Just runOutput -> runOutputTest executable runOutput parent future

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
  now <- getPOSIXTime
  commitAt workspace (floor now) relative content

commitAt :: FilePath -> Integer -> FilePath -> B.ByteString -> IO ()
commitAt workspace epoch relative content = do
  let path = workspace </> relative
  createDirectoryIfMissing True (takeDirectory path)
  B.writeFile path content
  git workspace ["add", relative]
  environment <- getEnvironment
  let stamp = show epoch ++ " +0000"
      environment' = ("GIT_AUTHOR_DATE", stamp) : ("GIT_COMMITTER_DATE", stamp) : filter (\(name, _) -> name /= "GIT_AUTHOR_DATE" && name /= "GIT_COMMITTER_DATE") environment
      process = (proc "git" ["-C", workspace, "commit", "-q", "-m", relative]) {env = Just environment'}
  (status, output, errors) <- readCreateProcessWithExitCode process ""
  unless (status == ExitSuccess) (ioError (userError ("git commit: " ++ output ++ errors)))

writeLarge :: FilePath -> IO ()
writeLarge path = do
  B.writeFile path "x"
  setFileSize path (16 * 1024 * 1024 + 1)
  age path

holdContent :: Integer -> Day -> B.ByteString
holdContent issue untilDay = BC.pack ("issue: " ++ show issue ++ "\nuntil: " ++ formatTime defaultTimeLocale "%Y-%m-%d" untilDay ++ "\n")

isoTime :: Integer -> String
isoTime seconds = formatTime defaultTimeLocale "%Y-%m-%dT%H:%M:%SZ" (posixSecondsToUTCTime (fromIntegral seconds))

data RunFile = RunFile
  { runFileOutcome :: String
  , runFileClosedAt :: Maybe String
  }

instance FromJSON RunFile where
  parseJSON = withObject "run file" $ \runObject ->
    RunFile
      <$> runObject .: "outcome"
      <*> runObject .:? "closed_at"

readRunFile :: FilePath -> IO RunFile
readRunFile path = do
  content <- B.readFile path
  case eitherDecodeStrict' content of
    Left problem -> ioError (userError ("invalid run file: " ++ problem))
    Right record -> pure record

runJson :: String -> Maybe Integer -> B.ByteString
runJson = runJsonWith 1 "exec"

runJsonWith :: Int -> String -> String -> Maybe Integer -> B.ByteString
runJsonWith schema opener outcome closed = BL.toStrict (encode (object fields))
 where
  fields =
    [ "schema_version" .= schema
    , "task" .= ("Task" :: String)
    , "label" .= ("Label" :: String)
    , "opener" .= opener
    , "command" .= (["true"] :: [String])
    , "commit" .= ("0000000000000000000000000000000000000000" :: String)
    , "opened_at" .= ("2026-01-01T00:00:00Z" :: String)
    , "closed_at" .= fmap isoTime closed
    , "exit_status" .= fmap (const (0 :: Int)) closed
    , "outcome" .= outcome
    , "reason" .= (Nothing :: Maybe String)
    , "work_file_count" .= (1 :: Int)
    , "work_bytes" .= (4 :: Int)
    , "work_sha256" .= ("" :: String)
    , "work_files" .= (["work/dump.bin"] :: [String])
    ]

writeRun :: FilePath -> String -> String -> String -> Maybe Integer -> IO FilePath
writeRun workspace task name outcome closed = writeRunWithJson workspace task name (runJson outcome closed)

writeRunWithJson :: FilePath -> String -> String -> B.ByteString -> IO FilePath
writeRunWithJson workspace task name record = do
  let directory = workspace </> "private-data/scratch" </> task </> "runs" </> name
  createDirectoryIfMissing True (directory </> "work")
  B.writeFile (directory </> "work/dump.bin") "work"
  createDirectoryIfMissing True (directory </> "result")
  B.writeFile (directory </> "result/verdict.json") "{}"
  B.writeFile (directory </> "run.json") record
  B.writeFile (directory </> "lock") ""
  now <- getPOSIXTime
  let activity = workspace </> "private-data/scratch" </> task </> "activity"
  B.writeFile activity "recent"
  setSymbolicLinkTimesHiRes activity (now + 8 * 86400) (now + 8 * 86400)
  pure directory

checkRunFilesKept :: FilePath -> IO ()
checkRunFilesKept directory = do
  runJsonPresent <- doesPathExist (directory </> "run.json")
  lockPresent <- doesPathExist (directory </> "lock")
  resultPresent <- doesPathExist (directory </> "result/verdict.json")
  check (runJsonPresent && lockPresent && resultPresent) "run metadata was removed"

holdLock :: FilePath -> IO Fd
holdLock path = do
  descriptor <- openFd path ReadWrite defaultFileFlags
  let Fd rawDescriptor = descriptor
  result <- c_flock rawDescriptor lockExclusiveNonBlocking
  unless (result == 0) (ioError (userError "could not hold the run lock"))
  pure descriptor

ancient :: POSIXTime
ancient = 1000000000

age :: FilePath -> IO ()
age path = setSymbolicLinkTimesHiRes path ancient ancient

setAgeSeconds :: FilePath -> Integer -> IO ()
setAgeSeconds path seconds = setSymbolicLinkTimesHiRes path (fromIntegral seconds) (fromIntegral seconds)

runRaw :: FilePath -> [String] -> IO (ExitCode, String, String)
runRaw executable arguments = readProcessWithExitCode executable arguments ""

runTool :: FilePath -> FilePath -> Integer -> Bool -> Integer -> IO Summary
runTool executable workspace days applied nowSeconds = runToolWith executable workspace days 48 applied nowSeconds

runToolWith :: FilePath -> FilePath -> Integer -> Integer -> Bool -> Integer -> IO Summary
runToolWith executable workspace days hours applied nowSeconds = do
  let arguments =
        [ "--workspace", workspace
        , "--max-age-days", show days
        , "--work-age-hours", show hours
        , "--now", show nowSeconds
        ]
          ++ (if applied then ["--apply"] else [])
  (status, output, errors) <- runRaw executable arguments
  unless (status == ExitSuccess) (ioError (userError (unwords arguments ++ ": " ++ errors)))
  case eitherDecodeStrict' (BC.pack output) of
    Left problem -> ioError (userError ("invalid summary: " ++ problem ++ ": " ++ output))
    Right summary -> pure summary

runOutputTest :: FilePath -> FilePath -> FilePath -> Integer -> IO ()
runOutputTest executable runOutput parent future = fixture parent "run-output" $ \workspace -> do
  git workspace ["checkout", "-q", "-b", "agent/Task"]
  (status, output, errors) <- runIn workspace runOutput ["exec", "--", "false"]
  check (status == ExitFailure 1) ("run-output exec false exited " ++ show status ++ ": " ++ output ++ errors)
  runs <- filter (not . isPrefixOf ".") <$> listDirectory (workspace </> "private-data/scratch/Task/runs")
  run <- case runs of
    [name] -> pure (workspace </> "private-data/scratch/Task/runs" </> name)
    _ -> ioError (userError ("expected one run directory, found " ++ show runs))
  recordBefore <- B.readFile (run </> "run.json")
  summary <- runTool executable workspace 7 True (future - 8 * 86400)
  check (summaryExpiredWorkRuns summary == 0) "the sweep removed failed run work"
  check (summaryFailures summary == 0) "the sweep failed on the real run record"
  work <- doesPathExist (run </> "work")
  check work "the sweep removed failed work"
  recordAfter <- B.readFile (run </> "run.json")
  check (recordBefore == recordAfter) "the sweep rewrote a failed run record"
  runJsonPresent <- doesPathExist (run </> "run.json")
  lockPresent <- doesPathExist (run </> "lock")
  resultPresent <- doesPathExist (run </> "result")
  check (runJsonPresent && lockPresent && resultPresent) "run metadata was removed"

runIn :: FilePath -> FilePath -> [String] -> IO (ExitCode, String, String)
runIn directory program arguments = do
  environment <- getEnvironment
  let removed = ["XM1_TASK_OUTPUT_MAX", "XM1_RUN_DIR", "XM1_RUN_WORK", "XM1_RUN_RESULT", "XM1_RUN_LABEL", "XM1_RUN_WORK_MAX", "XM1_RUN_POLL_MICROS", "XM1_EMULATOR_SLOT"]
      environment' = filter (\(name, _) -> name `notElem` removed) environment
  readCreateProcessWithExitCode (proc program arguments) {cwd = Just directory, env = Just environment'} ""
