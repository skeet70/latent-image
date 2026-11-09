{-# LANGUAGE OverloadedStrings #-}
{-# LANGUAGE ScopedTypeVariables #-}
module Main (main) where

import Control.Concurrent (threadDelay)
import Control.Exception (IOException, bracket, catch, try)
import Control.Monad (forM, forM_, unless)
import Data.Aeson (eitherDecodeStrict', encode)
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as BL
import Data.List (isPrefixOf, isSuffixOf)
import Data.Maybe (fromMaybe)
import StorageAccountingTypes
import System.Directory (canonicalizePath, createDirectory, doesPathExist, getTemporaryDirectory, listDirectory, removeDirectoryRecursive, removeFile, renameDirectory)
import System.Environment (getArgs, getEnvironment)
import System.Exit (ExitCode (..))
import System.FilePath ((</>))
import System.IO (hClose, openTempFile)
import System.IO.Error (isDoesNotExistError)
import System.Posix.Files (createSymbolicLink, getSymbolicLinkStatus, isSymbolicLink, setFileMode)
import System.Posix.Signals (sigINT, sigKILL, sigTERM, signalProcess, signalProcessGroup)
import System.Posix.Types (ProcessID)
import System.Process (CreateProcess (..), ProcessHandle, StdStream (..), createProcess, getPid, getProcessExitCode, proc, readCreateProcessWithExitCode, terminateProcess)

check :: Bool -> String -> IO ()
check condition message = unless condition (ioError (userError message))

withWorkspace :: (FilePath -> IO ()) -> IO ()
withWorkspace action = do
  base <- getTemporaryDirectory
  bracket (do
    (name, handle) <- openTempFile base "storage-runtime-tests"
    hClose handle
    removeFile name
    createDirectory name
    canonicalizePath name) removeDirectoryRecursive action

withCase :: FilePath -> String -> (FilePath -> IO ()) -> IO ()
withCase workspace name action = do
  let directory = workspace </> name
  createDirectory directory
  canonical <- canonicalizePath directory
  action canonical

validConfig :: FilePath -> Seconds -> AccountingConfig
validConfig root timeoutValue = AccountingConfig
  { accountingSchemaVersion = 1
  , accountingScanRoots = [ScanRoot (RootId "root") (StoragePath root) 2]
  , accountingFilesystemPaths = [StoragePath root]
  , accountingReserveBytes = ByteCount 0
  , accountingCommandTimeout = timeoutValue
  , accountingMaximumAge = Seconds 3600
  , accountingBudgets = [ProducerBudget (ProducerId "root") [StoragePath root] (Just (ByteCount 1000000000000)) [] Nothing]
  }

writeConfig :: FilePath -> AccountingConfig -> IO ()
writeConfig path config = BL.writeFile path (encode config)

sampleArguments :: FilePath -> FilePath -> [String]
sampleArguments config state = ["sample", "--config", config, "--state", state]

readStateFrom :: FilePath -> IO AccountingState
readStateFrom directory = do
  bytes <- B.readFile (directory </> "accounting.json")
  either (ioError . userError) pure (eitherDecodeStrict' bytes)

environmentWith :: [(String, String)] -> IO [(String, String)]
environmentWith overrides = do
  base <- getEnvironment
  let overridden = map fst overrides
  pure (overrides ++ filter (\(name, _) -> name `notElem` overridden) base)

runCollector :: FilePath -> [(String, String)] -> [String] -> IO (ExitCode, String, String)
runCollector tool overrides arguments = do
  environment <- environmentWith overrides
  readCreateProcessWithExitCode (proc tool arguments) { env = Just environment } ""

startCollector :: FilePath -> [(String, String)] -> [String] -> IO ProcessHandle
startCollector tool overrides arguments = do
  environment <- environmentWith overrides
  (_, _, _, processHandle) <- createProcess (proc tool arguments)
    { env = Just environment
    , std_in = NoStream
    , std_out = NoStream
    , std_err = NoStream
    }
  pure processHandle

collectorPid :: ProcessHandle -> IO ProcessID
collectorPid processHandle = getPid processHandle >>= maybe (ioError (userError "collector has no pid")) pure

readPidFiles :: FilePath -> String -> IO [ProcessID]
readPidFiles control prefix = do
  entries <- listDirectory control
  fmap concat $ forM [entry | entry <- entries, prefix `isPrefixOf` entry, ".pid" `isSuffixOf` entry] $ \entry -> do
    result <- try (readPid (control </> entry)) :: IO (Either IOException ProcessID)
    pure (either (const []) (: []) result)

waitForPidFiles :: FilePath -> String -> Int -> Int -> IO ()
waitForPidFiles control prefix expected limit = go 0
 where
  go elapsed = do
    pids <- readPidFiles control prefix
    if length pids >= expected
      then pure ()
      else if elapsed >= limit
        then ioError (userError ("timed out waiting for " ++ prefix ++ " pid files in " ++ control))
        else threadDelay pollMicros >> go (elapsed + pollMicros)

waitForExit :: ProcessHandle -> Int -> IO ExitCode
waitForExit processHandle limit = go 0
 where
  go elapsed = do
    exitCode <- getProcessExitCode processHandle
    case exitCode of
      Just code -> pure code
      Nothing
        | elapsed >= limit -> do
            terminateProcess processHandle `catch` \(_ :: IOException) -> pure ()
            ioError (userError "collector did not exit")
        | otherwise -> threadDelay pollMicros >> go (elapsed + pollMicros)

readPid :: FilePath -> IO ProcessID
readPid path = do
  contents <- readFile path
  case break (== '\n') contents of
    (value, '\n' : _) -> case reads value of
      [(number, "")] -> pure (fromIntegral (number :: Int))
      _ -> ioError (userError ("invalid pid file: " ++ path))
    _ -> ioError (userError ("incomplete pid file: " ++ path))

processRunning :: ProcessID -> IO Bool
processRunning processId = do
  let path = "/proc" </> show (fromIntegral processId :: Int) </> "stat"
  exists <- doesPathExist path
  if not exists
    then pure False
    else do
      contents <- readFile path `catch` \(_ :: IOException) -> pure ""
      let afterComm = reverse (fst (break (== ')') (reverse contents)))
          state = case dropWhile (== ' ') afterComm of
            character : _ -> character
            [] -> '?'
      pure (state /= 'Z' && not (null contents))

waitForDead :: ProcessID -> Int -> IO ()
waitForDead processId limit = go 0
 where
  go elapsed = do
    running <- processRunning processId
    if not running
      then pure ()
      else if elapsed >= limit
        then ioError (userError ("process " ++ show (fromIntegral processId :: Int) ++ " is still running"))
        else threadDelay (5 * pollMicros) >> go (elapsed + 5 * pollMicros)

pollMicros :: Int
pollMicros = 10000

blockingDuScript :: String
blockingDuScript = unlines
  [ "#!/bin/sh"
  , "if [ -e \"$CONTROL/blocked\" ]; then exit 0; fi"
  , ": > \"$CONTROL/blocked\""
  , "trap '' TERM"
  , "echo $$ > \"$CONTROL/du.$$.pid\""
  , "sh -c 'trap \"\" TERM; echo $$ > \"$CONTROL/child.$$.pid\"; exec sleep 300' &"
  , "wait"
  ]

writeExecutable :: FilePath -> String -> IO ()
writeExecutable path contents = do
  writeFile path contents
  setFileMode path 0o755

fakeEnvironment :: FilePath -> FilePath -> IO [(String, String)]
fakeEnvironment fakeBin control = do
  base <- getEnvironment
  let originalPath = fromMaybe "" (lookup "PATH" base)
  pure
    ( [ ("PATH", fakeBin ++ ":" ++ originalPath)
      , ("CONTROL", control)
      , ("LC_ALL", "C.UTF-8")
      ]
    ++ filter (\(name, _) -> name `notElem` ["PATH", "CONTROL", "LC_ALL"]) base
    )

main :: IO ()
main = do
  arguments <- getArgs
  case arguments of
    [tool] -> withWorkspace $ \workspace -> do
      testTimeoutKillsProbeGroup tool workspace
      testCancellationPreservesState tool workspace
      testRenameAnchorsStateWrites tool workspace
    _ -> ioError (userError "storage-accounting-runtime-tests COLLECTOR")

testTimeoutKillsProbeGroup :: FilePath -> FilePath -> IO ()
testTimeoutKillsProbeGroup tool workspace = withCase workspace "timeout-group" $ \directory -> do
  let root = directory </> "root"
      state = directory </> "state"
      config = directory </> "config.json"
      fakeBin = directory </> "bin"
      control = directory </> "control"
      fakeDu = fakeBin </> "du"
  createDirectory root
  createDirectory fakeBin
  createDirectory control
  writeExecutable fakeDu blockingDuScript
  writeConfig config (validConfig root (Seconds 1))
  environment <- fakeEnvironment fakeBin control
  (code, _, _) <- runCollector tool environment (sampleArguments config state)
  check (code == ExitSuccess) "timed-out sample must still publish incomplete state"
  leaders <- readPidFiles control "du."
  descendants <- readPidFiles control "child."
  check (not (null leaders)) "the fake du must record its leader pid"
  check (not (null descendants)) "the fake du must record its descendant pid"
  mapM_ (\processId -> waitForDead processId 5000000) (leaders ++ descendants)
  stateValue <- readStateFrom state
  check (any ((== TimedOut) . issueKind) (snapshotCoverageIssues (stateCurrent stateValue)))
    "du timeout must be an explicit coverage issue"

testCancellationPreservesState :: FilePath -> FilePath -> IO ()
testCancellationPreservesState tool workspace = withCase workspace "cancellation" $ \directory -> do
  let root = directory </> "root"
      state = directory </> "state"
      config = directory </> "config.json"
      fakeBin = directory </> "bin"
      control = directory </> "control"
      fakeDu = fakeBin </> "du"
      stateFile = state </> "accounting.json"
  createDirectory root
  createDirectory fakeBin
  createDirectory control
  writeConfig config (validConfig root (Seconds 60))
  (baselineCode, _, _) <- runCollector tool [] (sampleArguments config state)
  check (baselineCode == ExitSuccess) "baseline sample must succeed"
  original <- B.readFile stateFile
  writeExecutable fakeDu blockingDuScript
  forM_ [("SIGINT", sigINT), ("SIGTERM", sigTERM)] $ \(label, signal) -> do
    clearControl control
    environment <- fakeEnvironment fakeBin control
    processHandle <- startCollector tool environment (sampleArguments config state)
    processId <- collectorPid processHandle
    waitForPidFiles control "du." 1 10000000
    waitForPidFiles control "child." 1 10000000
    leaders <- readPidFiles control "du."
    descendants <- readPidFiles control "child."
    signalProcess signal processId
    code <- waitForExit processHandle 10000000
    check (code /= ExitSuccess) ("collector must fail after " ++ label)
    mapM_ (\probePid -> waitForDead probePid 5000000) (leaders ++ descendants)
    preserved <- B.readFile stateFile
    check (preserved == original) ("previous state must survive " ++ label)

testRenameAnchorsStateWrites :: FilePath -> FilePath -> IO ()
testRenameAnchorsStateWrites tool workspace = withCase workspace "rename-anchored" $ \directory -> do
  let root = directory </> "root"
      state = directory </> "state"
      moved = directory </> "state-moved"
      victim = directory </> "victim"
      config = directory </> "config.json"
      fakeBin = directory </> "bin"
      control = directory </> "control"
      fakeDu = fakeBin </> "du"
      sentinel = "victim temporary sentinel"
      victimState = "victim accounting state"
  createDirectory root
  createDirectory fakeBin
  createDirectory control
  createDirectory victim
  setFileMode victim 0o700
  B.writeFile (victim </> "accounting.json.tmp") sentinel
  setFileMode (victim </> "accounting.json.tmp") 0o600
  B.writeFile (victim </> "accounting.json") victimState
  setFileMode (victim </> "accounting.json") 0o600
  writeExecutable fakeDu blockingDuScript
  writeConfig config (validConfig root (Seconds 60))
  environment <- fakeEnvironment fakeBin control
  processHandle <- startCollector tool environment (sampleArguments config state)
  waitForPidFiles control "du." 1 10000000
  waitForPidFiles control "child." 1 10000000
  leaders <- readPidFiles control "du."
  descendants <- readPidFiles control "child."
  renameDirectory state moved
  createSymbolicLink victim state
  forM_ leaders $ \leader ->
    signalProcessGroup sigKILL (fromIntegral leader) `catch` \(_ :: IOException) -> signalProcess sigKILL leader
  code <- waitForExit processHandle 15000000
  check (code == ExitSuccess) "sample must publish state after the probe is released"
  mapM_ (\probePid -> waitForDead probePid 5000000) (leaders ++ descendants)
  sentinelAfter <- B.readFile (victim </> "accounting.json.tmp")
  check (sentinelAfter == sentinel) "the victim temporary sentinel must remain unchanged"
  victimStateAfter <- B.readFile (victim </> "accounting.json")
  check (victimStateAfter == victimState) "the victim state file must remain unchanged"
  _ <- readStateFrom moved
  temporaryExists <- doesPathExist (moved </> "accounting.json.tmp")
  check (not temporaryExists) "a successful write must not leave a temporary artifact"
  linkStatus <- getSymbolicLinkStatus state
  check (isSymbolicLink linkStatus) "the replaced state path must remain a symlink"

clearControl :: FilePath -> IO ()
clearControl control = do
  entries <- listDirectory control
  forM_ entries $ \entry ->
    removeFile (control </> entry) `catch` \(err :: IOException) ->
      unless (isDoesNotExistError err) (ioError err)
