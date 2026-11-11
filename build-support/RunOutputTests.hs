{-# LANGUAGE OverloadedStrings #-}
{-# LANGUAGE ScopedTypeVariables #-}
module Main (main) where

import Control.Concurrent (threadDelay)
import Control.Exception (IOException, bracket, try)
import Control.Monad (filterM, unless, void, when)
import Data.Aeson (FromJSON (parseJSON), eitherDecodeStrict', withObject, (.:), (.:?))
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as BC
import Data.List (isInfixOf, sort)
import Data.Maybe (listToMaybe)
import System.Directory (canonicalizePath, createDirectoryIfMissing, doesDirectoryExist, doesPathExist, getTemporaryDirectory, listDirectory, removeFile, removePathForcibly)
import System.Environment (getArgs, getEnvironment)
import System.Exit (ExitCode (..))
import System.FilePath ((</>))
import System.Posix.Signals (sigKILL, signalProcess, signalProcessGroup)
import System.Posix.Temp (mkdtemp)
import System.Posix.Types (ProcessID)
import System.Process (CreateProcess (..), StdStream (NoStream), createProcess, getPid, proc, readCreateProcessWithExitCode, readProcessWithExitCode, waitForProcess)
import System.Timeout (timeout)
import Text.Read (readMaybe)

data Record = Record
  { recordLabel :: String
  , recordOutcome :: String
  , recordReason :: Maybe String
  , recordExitStatus :: Maybe Int
  , recordWorkFileCount :: Integer
  , recordWorkBytes :: Integer
  , recordWorkSha256 :: String
  , recordClosedAt :: Maybe String
  }

instance FromJSON Record where
  parseJSON = withObject "run" $ \object' ->
    Record
      <$> object' .: "label"
      <*> object' .: "outcome"
      <*> object' .:? "reason"
      <*> object' .:? "exit_status"
      <*> object' .: "work_file_count"
      <*> object' .: "work_bytes"
      <*> object' .: "work_sha256"
      <*> object' .:? "closed_at"

check :: Bool -> String -> IO ()
check ok message = unless ok (ioError (userError message))

main :: IO ()
main = do
  arguments <- getArgs
  case arguments of
    [input] -> runTests input
    _ -> ioError (userError "expected the run-output executable")

runTests :: FilePath -> IO ()
runTests input = do
  executable <- canonicalizePath input
  temporary <- getTemporaryDirectory
  bracket (mkdtemp (temporary </> "run-output-")) removePathForcibly $ \parent -> do
    fixture parent "pass" $ \workspace -> do
      (status, _, errors) <- execTool executable workspace [] "pass"
        (shell "printf 'work\\n' > \"$XM1_RUN_WORK/out.bin\"; printf 'verdict\\n' > \"$XM1_RUN_RESULT/verdict.json\"")
      check (status == ExitSuccess) ("pass run exited " ++ show status ++ ": " ++ errors)
      directory <- onlyRun workspace
      work <- doesPathExist (directory </> "work")
      check (not work) "a passed run kept work"
      verdict <- doesPathExist (directory </> "result" </> "verdict.json")
      check verdict "a passed run lost its result"
      record <- readRecord directory
      check (recordOutcome record == "passed") "a passed run was not recorded passed"
      check (recordExitStatus record == Just 0) "a passed run recorded the wrong exit status"
      check (recordWorkFileCount record == 1) "a passed run recorded the wrong work file count"
      check (recordWorkBytes record == 5) "a passed run recorded the wrong work bytes"
      check (length (recordWorkSha256 record) == 64) "a passed run recorded no work digest"

    fixture parent "failure" $ \workspace -> do
      (status, _, _) <- execTool executable workspace [] "failure"
        (shell "printf 'work\\n' > \"$XM1_RUN_WORK/out.bin\"; exit 7")
      check (status == ExitFailure 7) "a failed run did not propagate its exit status"
      directory <- onlyRun workspace
      work <- doesPathExist (directory </> "work" </> "out.bin")
      check work "a failed run lost its work"
      record <- readRecord directory
      check (recordOutcome record == "failed") "a failed run was not recorded failed"
      check (recordExitStatus record == Just 7) "a failed run recorded the wrong exit status"
      check (recordReason record == Nothing) "a plain failure recorded a reason"

    fixture parent "supersession" $ \workspace -> do
      (first, _, _) <- execTool executable workspace [] "same" (shell "printf 'first\\n' > \"$XM1_RUN_WORK/out.bin\"; exit 1")
      check (first == ExitFailure 1) "the first failed run exited unexpectedly"
      firstRun <- findRun workspace "same"
      (second, _, _) <- execTool executable workspace [] "same" (shell "printf 'second\\n' > \"$XM1_RUN_WORK/out.bin\"; exit 1")
      check (second == ExitFailure 1) "the second failed run exited unexpectedly"
      secondRun <- findRun workspace "same"
      check (firstRun /= secondRun) "the second run reused the first directory"
      firstWork <- doesPathExist (firstRun </> "work")
      check (not firstWork) "the older failed run kept its work"
      secondWork <- doesPathExist (secondRun </> "work" </> "out.bin")
      check secondWork "the newer failed run lost its work"
      (other, _, _) <- execTool executable workspace [] "other" (shell "printf 'other\\n' > \"$XM1_RUN_WORK/out.bin\"; exit 1")
      check (other == ExitFailure 1) "the other failed run exited unexpectedly"
      secondStill <- doesPathExist (secondRun </> "work" </> "out.bin")
      check secondStill "a different label removed the previous label's work"

    fixture parent "environment" $ \workspace -> do
      (status, _, errors) <- execTool executable workspace [("TEST_RUN_OUTPUT", executable)] "environment"
        (shell
          (unlines
            [ "printf '%s\\n' \"$XM1_RUN_DIR\" \"$XM1_RUN_WORK\" \"$XM1_RUN_RESULT\" > \"$XM1_RUN_RESULT/env.txt\""
            , "\"$TEST_RUN_OUTPUT\" exec -- sh -c 'printf nested > \"$XM1_RUN_WORK/nested.txt\"'"
            , "printf '%s' \"$?\" > \"$XM1_RUN_RESULT/nested.txt\""
            ]))
      check (status == ExitSuccess) ("environment run exited " ++ show status ++ ": " ++ errors)
      directory <- onlyRun workspace
      content <- B.readFile (directory </> "result" </> "env.txt")
      let seen = lines (BC.unpack content)
      check (seen == [directory, directory </> "work", directory </> "result"]) ("child environment: " ++ show seen)
      nested <- B.readFile (directory </> "result" </> "nested.txt")
      check (nested == "0") "the nested exec did not exit zero"
      directories <- runDirectories workspace
      check (length directories == 1) "the nested exec opened a second run"

    fixture parent "result-limit" $ \workspace -> do
      (status, _, errors) <- execTool executable workspace [] "large"
        (shell "dd if=/dev/zero of=\"$XM1_RUN_RESULT/big.bin\" bs=1048576 count=17 status=none")
      check (status == ExitFailure 75) ("a large result exited " ++ show status ++ ": " ++ errors)
      directory <- onlyRun workspace
      record <- readRecord directory
      check (recordOutcome record == "failed") "a large result was not recorded failed"
      check (recordReason record == Just "result-too-large") "a large result recorded the wrong reason"
      check (recordExitStatus record == Just 0) "a large result recorded the wrong exit status"

    fixture parent "task-budget" $ \workspace -> do
      (failed, _, _) <- execTool executable workspace [] "old"
        (shell "dd if=/dev/urandom of=\"$XM1_RUN_WORK/big.bin\" bs=1048576 count=2 status=none; exit 1")
      check (failed == ExitFailure 1) "the budget fixture failed run exited unexpectedly"
      oldRun <- findRun workspace "old"
      let inputs = workspace </> "private-data" </> "scratch" </> "main" </> "inputs"
      createDirectoryIfMissing True inputs
      randomFile (inputs </> "a.bin") 2
      removeBudgetCache workspace
      (first, _, errors) <- runTool executable workspace [("XM1_TASK_OUTPUT_MAX", "3M")] ["exec", "--label", "first", "--", "true"]
      check (first == ExitSuccess) ("the budget removal run exited " ++ show first ++ ": " ++ errors)
      oldWork <- doesPathExist (oldRun </> "work")
      check (not oldWork) "the budget did not remove the old failed work"
      firstRun <- findRun workspace "first"
      firstRecord <- readRecord firstRun
      check (recordOutcome firstRecord == "passed") "the run after budget removal did not pass"
      randomFile (inputs </> "b.bin") 2
      removeBudgetCache workspace
      before <- runDirectories workspace
      (refused, _, refusalErrors) <- runTool executable workspace [("XM1_TASK_OUTPUT_MAX", "3M")] ["exec", "--label", "second", "--", "true"]
      check (refused == ExitFailure 75) ("the budget refusal exited " ++ show refused)
      check ("Keep verdicts and digests in result/" `isInfixOf` refusalErrors) "the refusal did not print the clearing sentence"
      after <- runDirectories workspace
      check (after == before) "a refused open created a run directory"

    fixture parent "abandoned" $ \workspace -> do
      environment <- cleanEnvironment []
      (_, _, _, process) <- createProcess
        (proc executable ["exec", "--label", "abandoned", "--", "sh", "-c", "printf '%s' $$ > \"$XM1_RUN_RESULT/child.pid\"; sleep 30"])
        {cwd = Just workspace, env = Just environment, std_out = NoStream, std_err = NoStream}
      childFile <- waitForChild workspace 100
      check (childFile /= Nothing) "the abandoned fixture did not start"
      runPid <- getPid process
      pid <- case runPid of
        Nothing -> ioError (userError "cannot read the fixture process id")
        Just value -> pure value
      signalProcess sigKILL pid
      _ <- waitForProcess process
      childPid <- case childFile of
        Just path -> do
          text <- readFile path
          pure (readMaybe text :: Maybe ProcessID)
        Nothing -> pure Nothing
      case childPid of
        Just value -> void (try (signalProcessGroup sigKILL value) :: IO (Either IOException ()))
        Nothing -> pure ()
      (status, _, errors) <- execTool executable workspace [] "next" (shell "printf 'next\\n' > \"$XM1_RUN_RESULT/next.txt\"")
      check (status == ExitSuccess) ("the run after abandonment exited " ++ show status ++ ": " ++ errors)
      abandoned <- findRun workspace "abandoned"
      record <- readRecord abandoned
      check (recordOutcome record == "unjudged") "an abandoned open run was not recorded unjudged"
      check (recordClosedAt record /= Nothing) "an abandoned run recorded no close time"

    fixture parent "keep" $ \workspace -> do
      let inputs = workspace </> "private-data" </> "scratch" </> "main" </> "inputs"
      createDirectoryIfMissing True inputs
      B.writeFile (inputs </> "blocked.txt") "original\n"
      (status, _, errors) <- execTool executable workspace [("TEST_RUN_OUTPUT", executable)] "keep"
        (shell
          (unlines
            [ "mkdir -p \"$XM1_RUN_WORK/sub\""
            , "printf 'data\\n' > \"$XM1_RUN_WORK/sub/keep.bin\""
            , "\"$TEST_RUN_OUTPUT\" keep \"$XM1_RUN_WORK/sub/keep.bin\""
            , "printf 'moved=%s\\n' \"$?\" >> \"$XM1_RUN_RESULT/keep.txt\""
            , "printf 'new\\n' > \"$XM1_RUN_WORK/blocked.txt\""
            , "\"$TEST_RUN_OUTPUT\" keep \"$XM1_RUN_WORK/blocked.txt\""
            , "printf 'blocked=%s\\n' \"$?\" >> \"$XM1_RUN_RESULT/keep.txt\""
            , "\"$TEST_RUN_OUTPUT\" keep /etc/hostname"
            , "printf 'outside=%s\\n' \"$?\" >> \"$XM1_RUN_RESULT/keep.txt\""
            , "exit 3"
            ]))
      check (status == ExitFailure 3) ("the keep fixture exited " ++ show status ++ ": " ++ errors)
      directory <- onlyRun workspace
      kept <- B.readFile (inputs </> "sub" </> "keep.bin")
      check (kept == "data\n") "keep did not move the file into inputs"
      movedAway <- doesPathExist (directory </> "work" </> "sub" </> "keep.bin")
      check (not movedAway) "keep left the file in work"
      blocked <- B.readFile (inputs </> "blocked.txt")
      check (blocked == "original\n") "keep replaced an existing input"
      outside <- doesPathExist (inputs </> "etc" </> "hostname")
      check (not outside) "keep moved a path outside the work directory"
      outcomes <- B.readFile (directory </> "result" </> "keep.txt")
      check (BC.unpack outcomes == "moved=0\nblocked=1\noutside=1\n") ("keep outcomes: " ++ show (BC.unpack outcomes))

    fixture parent "digest" $ \workspace -> do
      let script = shell "printf alpha > \"$XM1_RUN_WORK/a.txt\"; mkdir -p \"$XM1_RUN_WORK/sub\"; printf beta > \"$XM1_RUN_WORK/sub/b.txt\""
      (first, _, _) <- execTool executable workspace [] "one" script
      check (first == ExitSuccess) "the first digest run exited unexpectedly"
      (second, _, _) <- execTool executable workspace [] "two" script
      check (second == ExitSuccess) "the second digest run exited unexpectedly"
      firstRun <- findRun workspace "one"
      secondRun <- findRun workspace "two"
      firstRecord <- readRecord firstRun
      secondRecord <- readRecord secondRun
      check (recordWorkSha256 firstRecord == recordWorkSha256 secondRecord) "identical work produced different digests"
      check (length (recordWorkSha256 firstRecord) == 64) "the work digest is not SHA-256 hex"
      check (recordWorkFileCount firstRecord == 2) "the work file count is wrong"
      check (recordWorkBytes firstRecord == 9) "the work bytes are wrong"
      (third, _, _) <- execTool executable workspace [] "three" (shell "printf gamma > \"$XM1_RUN_WORK/a.txt\"; mkdir -p \"$XM1_RUN_WORK/sub\"; printf beta > \"$XM1_RUN_WORK/sub/b.txt\"")
      check (third == ExitSuccess) "the third digest run exited unexpectedly"
      thirdRun <- findRun workspace "three"
      thirdRecord <- readRecord thirdRun
      check (recordWorkSha256 firstRecord /= recordWorkSha256 thirdRecord) "different work produced the same digest"

    fixture parent "work-budget" $ \workspace -> do
      outcome <- timeout 20_000_000 $
        runTool executable workspace [("XM1_RUN_POLL_MICROS", "100000")]
          ["exec", "--label", "budget", "--work-budget", "65536", "--", "sh", "-c",
            "dd if=/dev/urandom of=\"$XM1_RUN_WORK/big.bin\" bs=1048576 count=4 status=none; sleep 30"]
      (status, _, errors) <- case outcome of
        Nothing -> ioError (userError "the work budget did not terminate the command")
        Just value -> pure value
      check (status == ExitFailure 75) ("the work budget exited " ++ show status ++ ": " ++ errors)
      directory <- onlyRun workspace
      record <- readRecord directory
      check (recordOutcome record == "failed") "the work budget did not fail the run"
      check (recordReason record == Just "work-budget") "the work budget recorded the wrong reason"
      work <- doesPathExist (directory </> "work" </> "big.bin")
      check work "the work budget lost the run's work"

fixture :: FilePath -> String -> (FilePath -> IO ()) -> IO ()
fixture parent name action = do
  let raw = parent </> name
  createDirectoryIfMissing True raw
  workspace <- canonicalizePath raw
  git workspace ["init", "-q", "-b", "main"]
  git workspace ["config", "user.email", "fixture@example.invalid"]
  git workspace ["config", "user.name", "Fixture"]
  B.writeFile (workspace </> "README.md") "fixture\n"
  git workspace ["add", "README.md"]
  git workspace ["commit", "-q", "-m", "fixture"]
  createDirectoryIfMissing True (workspace </> "private-data")
  action workspace

git :: FilePath -> [String] -> IO ()
git workspace arguments = do
  (status, output, errors) <- readCreateProcessWithExitCode (proc "git" (["-C", workspace] ++ arguments)) ""
  unless (status == ExitSuccess) (ioError (userError ("git " ++ unwords arguments ++ ": " ++ output ++ errors)))

shell :: String -> [String]
shell text = ["sh", "-c", text]

execTool :: FilePath -> FilePath -> [(String, String)] -> String -> [String] -> IO (ExitCode, String, String)
execTool executable workspace overrides label script =
  runTool executable workspace overrides (["exec", "--label", label, "--"] ++ script)

runTool :: FilePath -> FilePath -> [(String, String)] -> [String] -> IO (ExitCode, String, String)
runTool executable workspace overrides arguments = do
  environment <- cleanEnvironment overrides
  readCreateProcessWithExitCode (proc executable arguments) {cwd = Just workspace, env = Just environment} ""

cleanEnvironment :: [(String, String)] -> IO [(String, String)]
cleanEnvironment overrides = do
  inherited <- getEnvironment
  let dropped =
        [ "XM1_RUN_DIR"
        , "XM1_RUN_WORK"
        , "XM1_RUN_RESULT"
        , "XM1_RUN_LABEL"
        , "XM1_TASK_OUTPUT_MAX"
        , "XM1_RUN_WORK_MAX"
        , "XM1_RUN_POLL_MICROS"
        , "XM1_EMULATOR_SLOT"
        ]
      base = filter ((`notElem` dropped) . fst) inherited
  pure (foldl (\environment (key, value) -> (key, value) : filter ((/= key) . fst) environment) base overrides)

runDirectories :: FilePath -> IO [FilePath]
runDirectories workspace = do
  let runs = workspace </> "private-data" </> "scratch" </> "main" </> "runs"
  present <- doesDirectoryExist runs
  if not present
    then pure []
    else do
      names <- listDirectory runs
      directories <- filterM (doesDirectoryExist . (runs </>)) (sort names)
      pure (map (runs </>) directories)

onlyRun :: FilePath -> IO FilePath
onlyRun workspace = do
  directories <- runDirectories workspace
  case directories of
    [directory] -> pure directory
    _ -> ioError (userError ("expected one run directory, found " ++ show (length directories)))

findRun :: FilePath -> String -> IO FilePath
findRun workspace label = do
  directories <- runDirectories workspace
  matches <- filterM (\directory -> (== label) . recordLabel <$> readRecord directory) directories
  case reverse matches of
    directory : _ -> pure directory
    [] -> ioError (userError ("no run labelled " ++ label))

readRecord :: FilePath -> IO Record
readRecord directory = do
  content <- B.readFile (directory </> "run.json")
  case eitherDecodeStrict' content of
    Left problem -> ioError (userError ("invalid run.json in " ++ directory ++ ": " ++ problem))
    Right record -> pure record

removeBudgetCache :: FilePath -> IO ()
removeBudgetCache workspace = do
  let path = workspace </> "private-data" </> "scratch" </> "main" </> "runs" </> ".budget"
  present <- doesPathExist path
  when present (removeFile path)

randomFile :: FilePath -> Int -> IO ()
randomFile path megabytes = do
  (status, _, errors) <- readProcessWithExitCode "dd"
    ["if=/dev/urandom", "of=" ++ path, "bs=1048576", "count=" ++ show megabytes, "status=none"] ""
  unless (status == ExitSuccess) (ioError (userError ("dd: " ++ errors)))

waitForChild :: FilePath -> Int -> IO (Maybe FilePath)
waitForChild workspace attempts = do
  directories <- runDirectories workspace
  let candidates = [directory </> "result" </> "child.pid" | directory <- directories]
  present <- filterM doesPathExist candidates
  case listToMaybe present of
    Just path -> pure (Just path)
    Nothing
      | attempts <= 0 -> pure Nothing
      | otherwise -> threadDelay 100_000 >> waitForChild workspace (attempts - 1)
