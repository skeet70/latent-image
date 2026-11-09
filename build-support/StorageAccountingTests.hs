{-# LANGUAGE ForeignFunctionInterface #-}
{-# LANGUAGE OverloadedStrings #-}
{-# LANGUAGE ScopedTypeVariables #-}
module Main (main) where

import Control.Concurrent (threadDelay)
import Control.Exception (bracket, finally)
import Control.Monad (unless, when)
import Data.Aeson (FromJSON (parseJSON), eitherDecodeStrict', encode, withObject, (.:))
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as BL
import Data.Maybe (listToMaybe)
import qualified Data.Text as T
import qualified Data.Text.Encoding as TE
import Data.Time.Clock.POSIX (getPOSIXTime)
import Foreign.C.Types (CInt (..))
import GHC.IO.Encoding (setFileSystemEncoding, utf8)
import StorageAccountingTypes
import System.Directory (canonicalizePath, createDirectory, doesPathExist, getTemporaryDirectory, removeDirectoryRecursive, removeFile)
import System.Environment (getArgs, getEnvironment)
import System.Exit (ExitCode (..))
import System.FilePath ((</>))
import System.IO (hClose, openTempFile)
import System.Posix.Files (createLink, createSymbolicLink, setFileMode)
import System.Posix.IO (OpenMode (ReadWrite), closeFd, defaultFileFlags, openFd)
import System.Posix.Types (Fd (..))
import System.Posix.User (getEffectiveUserID)
import System.Process (CreateProcess (..), proc, readCreateProcessWithExitCode)
import Text.Read (readMaybe)

foreign import ccall unsafe "flock" c_flock :: CInt -> CInt -> IO CInt

newtype SampleSummary = SampleSummary
  { sampleSummaryStatus :: SampleStatus
  }
instance FromJSON SampleSummary where
  parseJSON = withObject "sample report" $ \o ->
    SampleSummary <$> o .: "sampleStatus"

data CheckSummary = CheckSummary
  { checkSummaryStatus :: CheckStatus
  , checkSummaryProblems :: [String]
  }
instance FromJSON CheckSummary where
  parseJSON = withObject "check report" $ \o ->
    CheckSummary <$> o .: "checkStatus" <*> o .: "checkProblems"

check :: Bool -> String -> IO ()
check ok message = unless ok (ioError (userError message))

incompressible :: Int -> B.ByteString
incompressible count = B.pack [fromIntegral ((index * 1103515245 + 12345) `mod` 256) | index <- [0 .. count - 1]]

runTool :: FilePath -> [String] -> IO (ExitCode, B.ByteString, B.ByteString)
runTool executable arguments = do
  environment <- getEnvironment
  let utf8Environment = ("LC_ALL", "C.UTF-8") : filter ((/= "LC_ALL") . fst) environment
  runToolWithEnvironment executable utf8Environment arguments

runToolWithEnvironment :: FilePath -> [(String, String)] -> [String] -> IO (ExitCode, B.ByteString, B.ByteString)
runToolWithEnvironment executable environment arguments = do
  (code, out, err) <- readCreateProcessWithExitCode (proc executable arguments) { env = Just environment } ""
  pure (code, TE.encodeUtf8 (T.pack out), TE.encodeUtf8 (T.pack err))

validConfig :: FilePath -> AccountingConfig
validConfig root = AccountingConfig
  { accountingSchemaVersion = 1
  , accountingScanRoots = [ScanRoot (RootId "root") (StoragePath root) 2]
  , accountingFilesystemPaths = [StoragePath root]
  , accountingReserveBytes = ByteCount 0
  , accountingCommandTimeout = Seconds 30
  , accountingMaximumAge = Seconds 3600
  , accountingBudgets = [ProducerBudget (ProducerId "root") [StoragePath root] (Just (ByteCount 1000000000000)) [] Nothing]
  }

writeConfig :: FilePath -> AccountingConfig -> IO ()
writeConfig path config = BL.writeFile path (encode config)

readState :: FilePath -> IO AccountingState
readState stateDirectory = do
  bytes <- B.readFile (stateDirectory </> "accounting.json")
  either (ioError . userError) pure (eitherDecodeStrict' bytes)

decodeSample :: B.ByteString -> IO SampleSummary
decodeSample bytes = either (ioError . userError) pure (eitherDecodeStrict' bytes)

decodeCheck :: B.ByteString -> IO CheckSummary
decodeCheck bytes = either (ioError . userError) pure (eitherDecodeStrict' bytes)

sampleArguments :: FilePath -> FilePath -> [String] -> [String]
sampleArguments config state extra = ["sample", "--config", config, "--state", state] ++ extra

checkArguments :: FilePath -> FilePath -> [String]
checkArguments config state = ["check", "--config", config, "--state", state]

withCase :: FilePath -> String -> (FilePath -> IO ()) -> IO ()
withCase workspace name action = do
  let directory = workspace </> name
  createDirectory directory
  canonical <- canonicalizePath directory
  action canonical

withWorkspace :: (FilePath -> IO ()) -> IO ()
withWorkspace action = do
  base <- getTemporaryDirectory
  bracket (do
    (name, handle) <- openTempFile base "storage-accounting-tests"
    hClose handle
    removeFile name
    createDirectory name
    canonicalizePath name) removeDirectoryRecursive action

main :: IO ()
main = do
  setFileSystemEncoding utf8
  arguments <- getArgs
  case arguments of
    [tool] -> withWorkspace $ \workspace -> do
      testInvalidConfig tool workspace
      testMalformedStatePreserved tool workspace
      testGrowth tool workspace
      testConfigChange tool workspace
      testFilesystemOnlyCheck tool workspace
      testStaleCheck tool workspace
      testWeirdFilename tool workspace
      testMissingRoot tool workspace
      testPermissionCoverage tool workspace
      testLockBusy tool workspace
      testTimeout tool workspace
      testCheckAndGuard tool workspace
      testStateSymlinks tool workspace
      testProducerHardlinkAttribution tool workspace
      testCreatedAndRemovedDirectoryGrowth tool workspace
      testFilesystemProbeBindings tool workspace
    _ -> ioError (userError "storage-accounting-tests TOOL")

testInvalidConfig :: FilePath -> FilePath -> IO ()
testInvalidConfig tool workspace = withCase workspace "invalid-config" $ \directory -> do
  let root = directory </> "root"
      state = directory </> "state"
      config = directory </> "config.json"
  createDirectory root
  writeConfig config ((validConfig root) { accountingSchemaVersion = 2 })
  (code, _, _) <- runTool tool (sampleArguments config state [])
  check (code /= ExitSuccess) "sample must reject an unsupported config schema"
  exists <- doesPathExist (state </> "accounting.json")
  check (not exists) "invalid config must not create state"

testMalformedStatePreserved :: FilePath -> FilePath -> IO ()
testMalformedStatePreserved tool workspace = withCase workspace "malformed-state" $ \directory -> do
  let root = directory </> "root"
      state = directory </> "state"
      config = directory </> "config.json"
      stateFile = state </> "accounting.json"
      original = "not accounting json"
  createDirectory root
  createDirectory state
  setFileMode state 0o700
  B.writeFile stateFile original
  setFileMode stateFile 0o600
  writeConfig config (validConfig root)
  (code, _, _) <- runTool tool (sampleArguments config state [])
  check (code /= ExitSuccess) "sample must reject malformed prior state"
  bytes <- B.readFile stateFile
  check (bytes == original) "malformed prior state must be preserved"

testGrowth :: FilePath -> FilePath -> IO ()
testGrowth tool workspace = withCase workspace "growth" $ \directory -> do
  let root = directory </> "root"
      state = directory </> "state"
      config = directory </> "config.json"
  createDirectory root
  B.writeFile (root </> "seed") (incompressible 4096)
  writeConfig config (validConfig root)
  (firstCode, firstOut, firstErr) <- runTool tool (sampleArguments config state [])
  check (firstCode == ExitSuccess) ("first sample must succeed: " ++ show firstErr)
  firstSummary <- decodeSample firstOut
  check (sampleSummaryStatus firstSummary == SampleComplete) "first sample must be complete"
  (allocated, _, _) <- readCreateProcessWithExitCode (proc "fallocate" ["-l", "1048576", root </> "grow"]) ""
  check (allocated == ExitSuccess) "growth fixture must allocate its declared extent"
  threadDelay 1200000
  (secondCode, _, _) <- runTool tool (sampleArguments config state [])
  check (secondCode == ExitSuccess) "second sample must succeed"
  secondState <- readState state
  check (growthCompatibleBaseline (stateGrowth secondState)) "same config must keep a compatible baseline"
  case listToMaybe [growth | growth <- growthDirectories (stateGrowth secondState), growthDirectory growth == StoragePath root] of
    Just growth -> case growthAllocatedBytes growth of
      ByteDelta delta -> check (delta == 1048576) "allocated growth must be reported in bytes"
    Nothing -> check False "second sample must report root directory growth"
  removeFile (root </> "grow")
  threadDelay 1200000
  (thirdCode, _, _) <- runTool tool (sampleArguments config state [])
  check (thirdCode == ExitSuccess) "third sample must succeed"
  thirdState <- readState state
  case listToMaybe [growth | growth <- growthDirectories (stateGrowth thirdState), growthDirectory growth == StoragePath root] of
    Just growth -> case growthAllocatedBytes growth of
      ByteDelta delta -> check (delta == -1048576) "released allocation must be reported as negative bytes"
    Nothing -> check False "third sample must report root directory growth"

testConfigChange :: FilePath -> FilePath -> IO ()
testConfigChange tool workspace = withCase workspace "config-change" $ \directory -> do
  let root = directory </> "root"
      state = directory </> "state"
      config = directory </> "config.json"
      changed = (validConfig root) { accountingReserveBytes = ByteCount 1 }
  createDirectory root
  writeConfig config (validConfig root)
  (firstCode, _, _) <- runTool tool (sampleArguments config state [])
  check (firstCode == ExitSuccess) "baseline sample must succeed"
  writeConfig config changed
  (secondCode, _, _) <- runTool tool (sampleArguments config state [])
  check (secondCode == ExitSuccess) "changed-config sample must succeed"
  secondState <- readState state
  check (not (growthCompatibleBaseline (stateGrowth secondState))) "a changed config digest must drop the growth baseline"
  writeConfig config changed
  (modeCode, _, _) <- runTool tool (sampleArguments config state ["--filesystem-only"])
  check (modeCode == ExitSuccess) "filesystem-only sample must succeed"
  modeState <- readState state
  check (snapshotFilesystemOnly (stateCurrent modeState)) "filesystem-only sample must record its mode"
  check (not (growthCompatibleBaseline (stateGrowth modeState))) "a changed sampling mode must drop the growth baseline"

testFilesystemOnlyCheck :: FilePath -> FilePath -> IO ()
testFilesystemOnlyCheck tool workspace = withCase workspace "filesystem-only-check" $ \directory -> do
  let root = directory </> "root"
      state = directory </> "state"
      config = directory </> "config.json"
  createDirectory root
  writeConfig config (validConfig root)
  (sampleCode, _, _) <- runTool tool (sampleArguments config state [])
  check (sampleCode == ExitSuccess) "detailed sample must succeed"
  (checkCode, _, _) <- runTool tool (checkArguments config state)
  check (checkCode == ExitSuccess) "fresh detailed state must check healthy"
  (filesystemCode, _, _) <- runTool tool (sampleArguments config state ["--filesystem-only"])
  check (filesystemCode == ExitSuccess) "filesystem-only sample must succeed"
  (recheckCode, recheckOut, _) <- runTool tool (checkArguments config state)
  check (recheckCode /= ExitSuccess) "filesystem-only state must not check healthy"
  summary <- decodeCheck recheckOut
  check (checkSummaryStatus summary == CheckUnhealthy) "filesystem-only check must report unhealthy"

testStaleCheck :: FilePath -> FilePath -> IO ()
testStaleCheck tool workspace = withCase workspace "stale-check" $ \directory -> do
  let root = directory </> "root"
      state = directory </> "state"
      config = directory </> "config.json"
      stale = (validConfig root) { accountingMaximumAge = Seconds 1 }
  createDirectory root
  writeConfig config stale
  (sampleCode, _, _) <- runTool tool (sampleArguments config state [])
  check (sampleCode == ExitSuccess) "stale-check sample must succeed"
  threadDelay 2100000
  (checkCode, checkOut, _) <- runTool tool (checkArguments config state)
  check (checkCode /= ExitSuccess) "stale state must not check healthy"
  summary <- decodeCheck checkOut
  check (checkSummaryStatus summary == CheckUnhealthy) "stale check must report unhealthy"

testWeirdFilename :: FilePath -> FilePath -> IO ()
testWeirdFilename tool workspace = withCase workspace "weird-filename" $ \directory -> do
  let root = directory </> "root"
      state = directory </> "state"
      config = directory </> "config.json"
      weird = "line\nbreak\ttab space-\955"
  createDirectory root
  createDirectory (root </> weird)
  B.writeFile (root </> weird </> "data") "x"
  writeConfig config (validConfig root)
  (code, _, _) <- runTool tool (sampleArguments config state [])
  check (code == ExitSuccess) "sample with unusual filenames must succeed"
  stateValue <- readState state
  check (any ((== StoragePath (root </> weird)) . directoryPath) (snapshotDirectories (stateCurrent stateValue))) "unusual filename must round-trip through du and JSON"

testMissingRoot :: FilePath -> FilePath -> IO ()
testMissingRoot tool workspace = withCase workspace "missing-root" $ \directory -> do
  let rootPath = directory </> "root-file"
      state = directory </> "state"
      config = directory </> "config.json"
      missing = (validConfig directory)
        { accountingScanRoots = [ScanRoot (RootId "root") (StoragePath rootPath) 2]
        }
  B.writeFile rootPath "not a directory"
  writeConfig config missing
  (sampleCode, sampleOut, _) <- runTool tool (sampleArguments config state [])
  check (sampleCode == ExitSuccess) "missing scan root must still publish explicit incomplete state"
  summary <- decodeSample sampleOut
  check (sampleSummaryStatus summary == SampleIncomplete) "missing scan root must mark the sample incomplete"
  stateValue <- readState state
  check (any ((== MissingPath) . issueKind) (snapshotCoverageIssues (stateCurrent stateValue))) "missing scan root must be an explicit issue"
  (checkCode, _, _) <- runTool tool (checkArguments config state)
  check (checkCode /= ExitSuccess) "incomplete coverage must not check healthy"

testPermissionCoverage :: FilePath -> FilePath -> IO ()
testPermissionCoverage tool workspace = withCase workspace "permission-coverage" $ \directory -> do
  caller <- getEffectiveUserID
  when (caller /= 0) $ do
    let root = directory </> "root"
        blocked = root </> "blocked"
        state = directory </> "state"
        config = directory </> "config.json"
    createDirectory root
    createDirectory blocked
    B.writeFile (blocked </> "data") "x"
    writeConfig config (validConfig root)
    setFileMode blocked 0o000
    (code, _, _) <- runTool tool (sampleArguments config state []) `finally` setFileMode blocked 0o755
    check (code == ExitSuccess) "permission denial must still publish explicit incomplete state"
    stateValue <- readState state
    check (any ((== PermissionDenied) . issueKind) (snapshotCoverageIssues (stateCurrent stateValue))) "permission denial must be an explicit issue"

testLockBusy :: FilePath -> FilePath -> IO ()
testLockBusy tool workspace = withCase workspace "lock-busy" $ \directory -> do
  let root = directory </> "root"
      state = directory </> "state"
      config = directory </> "config.json"
      lock = state </> "accounting.lock"
  createDirectory root
  createDirectory state
  setFileMode state 0o700
  B.writeFile lock ""
  setFileMode lock 0o600
  writeConfig config (validConfig root)
  bracket (openFd lock ReadWrite defaultFileFlags) closeFd $ \fd -> case fd of
    Fd descriptor -> do
      result <- c_flock descriptor 2
      check (result == 0) "test must acquire the state lock"
      (code, _, _) <- runTool tool (sampleArguments config state [])
      check (code /= ExitSuccess) "sample must fail while another process holds the state lock"
      exists <- doesPathExist (state </> "accounting.json")
      check (not exists) "busy sample must not write state"
  (code, _, _) <- runTool tool (sampleArguments config state [])
  check (code == ExitSuccess) "sample must succeed after the state lock is released"

testTimeout :: FilePath -> FilePath -> IO ()
testTimeout tool workspace = withCase workspace "timeout" $ \directory -> do
  let root = directory </> "root"
      state = directory </> "state"
      config = directory </> "config.json"
      fakeBin = directory </> "bin"
      fakeDu = fakeBin </> "du"
      quick = (validConfig root) { accountingCommandTimeout = Seconds 1 }
  createDirectory root
  createDirectory fakeBin
  B.writeFile fakeDu "#!/bin/sh\nexec sleep 30\n"
  setFileMode fakeDu 0o755
  writeConfig config quick
  environment <- getEnvironment
  let originalPath = maybe "" id (lookup "PATH" environment)
      fakeEnvironment =
        ("PATH", fakeBin ++ ":" ++ originalPath)
        : ("LC_ALL", "C.UTF-8")
        : filter (\(name, _) -> name /= "PATH" && name /= "LC_ALL") environment
  start <- getPOSIXTime
  (code, _, _) <- runToolWithEnvironment tool fakeEnvironment (sampleArguments config state [])
  end <- getPOSIXTime
  check (code == ExitSuccess) "timed-out du must still publish explicit incomplete state"
  check (end - start < 15) "sample must honor the command timeout"
  stateValue <- readState state
  check (any ((== TimedOut) . issueKind) (snapshotCoverageIssues (stateCurrent stateValue))) "du timeout must be an explicit issue"

testCheckAndGuard :: FilePath -> FilePath -> IO ()
testCheckAndGuard tool workspace = withCase workspace "check-guard" $ \directory -> do
  let root = directory </> "root"
      state = directory </> "state"
      config = directory </> "config.json"
      badState = directory </> "bad-state"
      marker = directory </> "marker"
  createDirectory root
  writeConfig config (validConfig root)
  (sampleCode, _, _) <- runTool tool (sampleArguments config state [])
  check (sampleCode == ExitSuccess) "guard fixture sample must succeed"
  (checkCode, checkOut, _) <- runTool tool (checkArguments config state)
  check (checkCode == ExitSuccess) "fresh detailed state must check healthy"
  summary <- decodeCheck checkOut
  check (checkSummaryStatus summary == CheckHealthy) "healthy check must report healthy"
  check (null (checkSummaryProblems summary)) "healthy check must report no problems"
  (guardCode, _, _) <- runTool tool ["guard", "--config", config, "--state", state, "--", "sh", "-c", "exit 7"]
  check (guardCode == ExitFailure 7) "guard must preserve the child exit status"
  createDirectory badState
  setFileMode badState 0o700
  B.writeFile (badState </> "accounting.json") "garbage"
  setFileMode (badState </> "accounting.json") 0o600
  (refusalCode, _, _) <- runTool tool ["guard", "--config", config, "--state", badState, "--", "sh", "-c", "touch \"$1\"", "sh", marker]
  check (refusalCode /= ExitSuccess) "guard must refuse on malformed state"
  markerExists <- doesPathExist marker
  check (not markerExists) "refused guard must not execute the command"

testStateSymlinks :: FilePath -> FilePath -> IO ()
testStateSymlinks tool workspace = withCase workspace "state-symlinks" $ \directory -> do
  let root = directory </> "root"
      config = directory </> "config.json"
      realState = directory </> "real-state"
      linkState = directory </> "link-state"
      realParent = directory </> "real-parent"
      linkParent = directory </> "link-parent"
  createDirectory root
  writeConfig config (validConfig root)
  createDirectory realState
  createSymbolicLink realState linkState
  (linkCode, _, _) <- runTool tool (sampleArguments config linkState [])
  check (linkCode /= ExitSuccess) "sample must reject a symlinked state directory"
  linkStateFile <- doesPathExist (realState </> "accounting.json")
  check (not linkStateFile) "symlinked state directory must not receive state"
  createDirectory realParent
  createSymbolicLink realParent linkParent
  (parentCode, _, _) <- runTool tool (sampleArguments config (linkParent </> "state") [])
  check (parentCode /= ExitSuccess) "sample must reject a state directory under an untrusted symlinked parent"
  parentState <- doesPathExist (realParent </> "state")
  check (not parentState) "untrusted symlinked parent must not receive a state directory"

testProducerHardlinkAttribution :: FilePath -> FilePath -> IO ()
testProducerHardlinkAttribution tool workspace = withCase workspace "producer-hardlinks" $ \directory -> do
  let first = directory </> "a"
      selected = directory </> "z"
      config = directory </> "config.json"
      state = directory </> "state"
      base = validConfig selected
  createDirectory first
  createDirectory selected
  (allocated, _, _) <- readCreateProcessWithExitCode (proc "fallocate" ["-l", "2097152", first </> "payload"]) ""
  check (allocated == ExitSuccess) "allocate hardlinked compiler-output fixture"
  createLink (first </> "payload") (selected </> "payload")
  writeConfig config base
    { accountingScanRoots = [ScanRoot (RootId "first") (StoragePath first) 1, ScanRoot (RootId "selected") (StoragePath selected) 1]
    , accountingBudgets = map (\budget -> budget
        { producerPaths = [StoragePath selected], producerByteBudget = Just (ByteCount 1048576) }) (accountingBudgets base)
    }
  (code, _, _) <- runTool tool (sampleArguments config state [])
  check (code == ExitSuccess) "hardlinked fixture must be measurable"
  value <- readState state
  check (map budgetVerdict (stateBudgets value) == [OverBudget]) "a hardlink charged outside the producer must not hide its allocation"

testCreatedAndRemovedDirectoryGrowth :: FilePath -> FilePath -> IO ()
testCreatedAndRemovedDirectoryGrowth tool workspace = withCase workspace "copy-growth" $ \directory -> do
  let root = directory </> "root"
      child = root </> "copy"
      config = directory </> "config.json"
      state = directory </> "state"
  createDirectory root
  writeConfig config (validConfig root)
  (baselineCode, _, _) <- runTool tool (sampleArguments config state [])
  check (baselineCode == ExitSuccess) "baseline inventory must succeed"
  createDirectory child
  (allocated, _, _) <- readCreateProcessWithExitCode (proc "fallocate" ["-l", "1048576", child </> "payload"]) ""
  check (allocated == ExitSuccess) "allocate new-copy fixture"
  (statCode, statOut, _) <- readCreateProcessWithExitCode (proc "stat" ["-c", "%b %B", "--", child]) ""
  directoryBytes <- case (statCode, traverse readMaybe (words statOut) :: Maybe [Integer]) of
    (ExitSuccess, Just [blocks, blockBytes]) -> pure (blocks * blockBytes)
    _ -> ioError (userError "cannot measure fixture directory allocation")
  threadDelay 1200000
  (createdCode, _, _) <- runTool tool (sampleArguments config state [])
  check (createdCode == ExitSuccess) "new-copy inventory must succeed"
  created <- readState state
  let deltas value = [(growthDirectory row, growthAllocatedBytes row) | row <- growthDirectories (stateGrowth value)]
      expected = 1048576 + directoryBytes
  check (lookup (StoragePath child) (deltas created) == Just (ByteDelta expected)) "a new directory must report its full allocation from a known zero baseline"
  removeDirectoryRecursive child
  threadDelay 1200000
  (removedCode, _, _) <- runTool tool (sampleArguments config state [])
  check (removedCode == ExitSuccess) "removed-copy inventory must succeed"
  removed <- readState state
  check (lookup (StoragePath child) (deltas removed) == Just (ByteDelta (negate expected))) "a removed directory must report its full allocation returning to zero"

testFilesystemProbeBindings :: FilePath -> FilePath -> IO ()
testFilesystemProbeBindings tool workspace = withCase workspace "filesystem-bindings" $ \directory -> do
  let root = directory </> "root"
      fakeBin = directory </> "bin"
      config = directory </> "config.json"
      state = directory </> "state"
      marker = directory </> "marker"
      stateFile = state </> "accounting.json"
  createDirectory root
  createDirectory fakeBin
  environment <- getEnvironment
  let probeEnvironment = ("PATH", fakeBin ++ ":" ++ maybe "" id (lookup "PATH" environment)) : filter ((/= "PATH") . fst) environment
      run arguments = runToolWithEnvironment tool probeEnvironment arguments
  writeConfig config (validConfig root)
    { accountingFilesystemPaths = [StoragePath "/proc", StoragePath root], accountingReserveBytes = ByteCount 100 }
  B.writeFile (fakeBin </> "df") "#!/bin/sh\nfor last in \"$@\"; do :; done\nprintf 'Filesystem 1B-blocks Used Available\\n'\nif [ \"$last\" = /proc ]; then printf 'tmpfs 10000 3000 7000\\n'; else printf 'tmpfs 10000 9999 1\\n'; fi\n"
  setFileMode (fakeBin </> "df") 0o755
  (lowCode, _, _) <- run (sampleArguments config state [])
  check (lowCode == ExitSuccess) "same-source filesystem probes must be measurable"
  low <- readState state
  check (stateLowSpace low) "a second filesystem sharing a source label must not hide its low reserve"
  B.writeFile (fakeBin </> "df") "#!/bin/sh\nprintf 'Filesystem 1B-blocks Used Available\\ntmpfs 10000 3000 7000\\n'\n"
  (healthyCode, _, _) <- run (sampleArguments config state [])
  check (healthyCode == ExitSuccess) "healthy filesystem baseline must succeed"
  healthy <- readState state
  swapped <- case snapshotFilesystems (stateCurrent healthy) of
    [first, second] -> do
      check (filesystemId first /= filesystemId second) "distinct mounted filesystems need distinct identities"
      pure healthy { stateCurrent = (stateCurrent healthy)
        { snapshotFilesystems = [first { filesystemId = filesystemId second }, second { filesystemId = filesystemId first }] } }
    _ -> ioError (userError "both configured filesystem bindings must be retained")
  BL.writeFile stateFile (encode swapped)
  (guardCode, _, _) <- run ["guard", "--config", config, "--state", state, "--", "sh", "-c", "touch \"$1\"", "sh", marker]
  check (guardCode /= ExitSuccess) "swapping probe identities must refuse admission even when the identity set is unchanged"
  markerExists <- doesPathExist marker
  check (not markerExists) "a changed filesystem binding must not execute the guarded command"
  before <- B.readFile stateFile
  B.writeFile (fakeBin </> "stat") "#!/bin/sh\nexit 1\n"
  setFileMode (fakeBin </> "stat") 0o755
  (failedCode, _, _) <- run (sampleArguments config state [])
  check (failedCode /= ExitSuccess) "an inventory without any filesystem measurement must fail"
  after <- B.readFile stateFile
  check (after == before) "a fatal filesystem probe must preserve the previous accounting state"
