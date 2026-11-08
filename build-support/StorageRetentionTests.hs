{-# LANGUAGE ForeignFunctionInterface #-}
{-# LANGUAGE OverloadedStrings #-}
{-# LANGUAGE ScopedTypeVariables #-}
module Main (main) where

import Control.Concurrent (threadDelay)
import Control.Exception (bracket)
import Control.Monad (forM, forM_, unless, when)
import qualified Crypto.Hash.SHA256 as SHA
import Data.Aeson (FromJSON (parseJSON), ToJSON (toJSON), eitherDecodeStrict', encode, object, withObject, withText, (.:), (.=))
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as BL
import Data.List (isPrefixOf, isSuffixOf, sort)
import Data.List.NonEmpty (NonEmpty (..))
import qualified Data.List.NonEmpty as NonEmpty
import qualified Data.Text as T
import qualified Data.Text.Encoding as TE
import Data.Time.Clock.POSIX (POSIXTime, getPOSIXTime)
import Foreign.C.Types (CInt (..), CLong (..), CSize (..))
import Foreign.Ptr (Ptr, intPtrToPtr, nullPtr)
import Numeric (showHex)
import System.Directory (canonicalizePath, createDirectory, createDirectoryIfMissing, doesDirectoryExist, doesPathExist, getTemporaryDirectory, listDirectory, removeDirectoryRecursive, removeFile)
import System.Environment (getArgs, getEnvironment, getExecutablePath)
import System.Exit (ExitCode (..))
import System.FilePath ((</>), splitDirectories, takeDirectory)
import System.IO (hClose, hFlush, openTempFile, stdout)
import System.Posix.Files (createLink, createSymbolicLink, fileMode, fileSize, getSymbolicLinkStatus, isDirectory, readSymbolicLink, setFileMode, setFileTimesHiRes)
import System.Posix.IO (OpenFileFlags (cloexec), OpenMode (ReadOnly, ReadWrite), closeFd, defaultFileFlags, openFd)
import System.Posix.Process (executeFile)
import System.Posix.Types (Fd (..))
import System.Process (CreateProcess (..), StdStream (CreatePipe), createProcess, proc, readProcessWithExitCode, terminateProcess, waitForProcess)

foreign import ccall unsafe "flock" c_flock :: CInt -> CInt -> IO CInt
foreign import ccall unsafe "mmap" c_mmap :: Ptr () -> CSize -> CInt -> CInt -> CInt -> CLong -> IO (Ptr ())

data RetainOutput = RetainOutput
  { outputPolicySha256 :: T.Text
  , outputRunMode :: RunMode
  , outputRootReports :: [RootReport]
  }

data RunMode = PreviewMode | ApplyMode deriving (Eq, Show)

instance FromJSON RunMode where
  parseJSON = withText "run mode" $ \text -> case text of
    "Preview" -> pure PreviewMode
    "Apply" -> pure ApplyMode
    _ -> fail ("unknown run mode: " ++ T.unpack text)

instance FromJSON RetainOutput where
  parseJSON = withObject "retain output" $ \o ->
    RetainOutput <$> o .: "policySha256" <*> o .: "runMode" <*> o .: "rootReports"

data RetentionStatus = BelowBudget | PreviewSelection | Complete | TargetUnmet | DeferredBusy deriving (Eq, Show)

instance FromJSON RetentionStatus where
  parseJSON = withText "retention status" $ \text -> case text of
    "BelowBudget" -> pure BelowBudget
    "Preview" -> pure PreviewSelection
    "Complete" -> pure Complete
    "TargetUnmet" -> pure TargetUnmet
    "DeferredBusy" -> pure DeferredBusy
    _ -> fail ("unknown retention status: " ++ T.unpack text)

newtype ByteCount = ByteCount Integer deriving (Eq, Ord, Show)
newtype UnitCount = UnitCount Integer deriving (Eq, Ord, Show)
newtype Nanoseconds = Nanoseconds Integer deriving (Eq, Ord, Show)
newtype Seconds = Seconds Integer deriving (Eq, Show)

instance FromJSON ByteCount where parseJSON = fmap ByteCount . parseJSON
instance ToJSON ByteCount where toJSON (ByteCount value) = toJSON value
instance FromJSON UnitCount where parseJSON = fmap UnitCount . parseJSON
instance FromJSON Nanoseconds where parseJSON = fmap Nanoseconds . parseJSON
instance ToJSON Seconds where toJSON (Seconds value) = toJSON value

byteValue :: ByteCount -> Integer
byteValue (ByteCount value) = value

countValue :: UnitCount -> Integer
countValue (UnitCount value) = value

nanosecondsValue :: Nanoseconds -> Integer
nanosecondsValue (Nanoseconds value) = value

data RootReport = RootReport
  { reportStatus :: RetentionStatus
  , reportBeforeBytes :: ByteCount
  , reportAfterBytes :: ByteCount
  , reportBudgetBytes :: ByteCount
  , reportTargetBytes :: ByteCount
  , reportSelectedUnitCount :: UnitCount
  , reportRemovedUnitCount :: UnitCount
  , reportRecentUnitCount :: UnitCount
  , reportBlockedUnitCount :: UnitCount
  , reportTargetMet :: Bool
  , reportAuditPath :: Maybe FilePath
  , reportInspectionComplete :: Maybe Bool
  , reportUnits :: [UnitReport]
  }

instance FromJSON RootReport where
  parseJSON = withObject "root report" $ \o ->
    RootReport
      <$> o .: "status"
      <*> o .: "beforeBytes"
      <*> o .: "afterBytes"
      <*> o .: "budgetBytes"
      <*> o .: "targetBytes"
      <*> o .: "selectedUnitCount"
      <*> o .: "removedUnitCount"
      <*> o .: "recentUnitCount"
      <*> o .: "blockedUnitCount"
      <*> o .: "targetMet"
      <*> o .: "auditPath"
      <*> o .: "inspectionComplete"
      <*> o .: "units"

data UnitReport = UnitReport
  { reportedMembers :: [T.Text]
  , reportedNewestMtimeNanoseconds :: Nanoseconds
  }

instance FromJSON UnitReport where
  parseJSON = withObject "unit" $ \o ->
    UnitReport <$> o .: "members" <*> o .: "newestMtimeNanoseconds"

data RootPolicy = RootPolicy
  { rootPolicyTarget :: FilePath
  , rootPolicyBudget :: ByteCount
  , rootPolicyTargetBytes :: ByteCount
  , rootPolicyMinimumAge :: Seconds
  }

data ToolResult = ToolResult
  { resultCode :: ExitCode
  , resultOut :: B.ByteString
  , resultErr :: String
  }

data Unit = Unit
  { unitMarker :: FilePath
  , unitOutputs :: NonEmpty FilePath
  }

hashSuffix :: String
hashSuffix = "0123456789abcdef"

slack :: Integer
slack = 128 * 1024

minimumAgeSeconds :: Seconds
minimumAgeSeconds = Seconds 86400

payloadBytes :: Int
payloadBytes = 512 * 1024

cachedirTag, fingerprintJson, markerBytes, elf :: B.ByteString
cachedirTag = "Signature: 8a477f597d28d172789f06886806bc55\n# synthetic Cargo retention fixture\n"
fingerprintJson = "{\"rustc\":1,\"features\":\"[]\",\"target\":2,\"profile\":3,\"path\":4,\"deps\":[]}"
markerBytes = "0123456789abcdef"
elf = B.pack [127, 69, 76, 70, 1, 2, 3, 4]

libraryUnit :: String -> Unit
libraryUnit crate =
  let output extension = "debug/deps/lib" ++ crate ++ "-" ++ hashSuffix ++ extension
  in Unit
    ("debug/.fingerprint/" ++ crate ++ "-" ++ hashSuffix ++ "/lib-" ++ crate)
    (output ".rlib" :| map output [".rmeta", ".so"])

testUnit :: String -> Unit
testUnit crate =
  Unit
    ("debug/.fingerprint/" ++ crate ++ "-" ++ hashSuffix ++ "/test-lib-" ++ crate)
    (("debug/deps/" ++ crate ++ "-" ++ hashSuffix) :| [])

buildScriptUnit :: Unit
buildScriptUnit =
  Unit
    ("debug/.fingerprint/builder-" ++ hashSuffix ++ "/build-script-build-script-build")
    (("debug/build/builder-" ++ hashSuffix ++ "/build-script-build") :| [])

unitJson :: Unit -> FilePath
unitJson unit = unitMarker unit ++ ".json"

unitFiles :: Unit -> [FilePath]
unitFiles unit = unitMarker unit : unitJson unit : NonEmpty.toList (unitOutputs unit)

unitSelectedMembers :: Unit -> [FilePath]
unitSelectedMembers unit = unitMarker unit : NonEmpty.toList (unitOutputs unit)

unitDirectory :: Unit -> FilePath
unitDirectory = takeDirectory . unitMarker

outputBytes :: FilePath -> B.ByteString
outputBytes path
  | ".rlib" `isSuffixOf` path = "!<arch>\n" <> filler
  | ".rmeta" `isSuffixOf` path = "rust" <> filler
  | otherwise = elf <> filler
 where
  filler = B.replicate payloadBytes 0x41

check :: Bool -> String -> IO ()
check ok message = unless ok (ioError (userError message))

put :: FilePath -> B.ByteString -> IO ()
put path bytes = createDirectoryIfMissing True (takeDirectory path) >> B.writeFile path bytes

setTimes :: POSIXTime -> FilePath -> IO ()
setTimes time path = setFileTimesHiRes path time time

ageTree :: POSIXTime -> FilePath -> IO ()
ageTree time root = do
  entries <- listDirectory root
  forM_ entries $ \name -> do
    let path = root </> name
    status <- getSymbolicLinkStatus path
    if isDirectory status then ageTree time path else setTimes time path
  setTimes time root

refreshUnit :: FilePath -> POSIXTime -> Unit -> IO ()
refreshUnit root time unit = do
  setTimes time (root </> unitDirectory unit)
  forM_ (unitFiles unit) (setTimes time . (root </>))

apparentBytes :: FilePath -> IO Integer
apparentBytes root = do
  entries <- listDirectory root
  fmap sum $ forM entries $ \name -> do
    let path = root </> name
    status <- getSymbolicLinkStatus path
    if isDirectory status then apparentBytes path else pure (fromIntegral (fileSize status))

relativePaths :: FilePath -> IO [FilePath]
relativePaths root = sort <$> go ""
 where
  go relative = do
    entries <- listDirectory (root </> relative)
    fmap concat $ forM entries $ \name -> do
      let child = if null relative then name else relative </> name
      status <- getSymbolicLinkStatus (root </> child)
      descendants <- if isDirectory status then go child else pure []
      pure (child : descendants)

snapshot :: FilePath -> [FilePath] -> IO [(FilePath, B.ByteString)]
snapshot root paths = forM paths $ \path -> do
  bytes <- B.readFile (root </> path)
  pure (path, bytes)

buildRoot :: FilePath -> [Unit] -> IO ()
buildRoot root units = do
  put (root </> "CACHEDIR.TAG") cachedirTag
  put (root </> "debug/.cargo-lock") ""
  forM_ units $ \unit -> do
    put (root </> unitMarker unit) markerBytes
    put (root </> unitJson unit) fingerprintJson
    forM_ (unitOutputs unit) $ \path -> put (root </> path) (outputBytes path)

writePolicy :: FilePath -> [RootPolicy] -> FilePath -> IO ()
writePolicy path roots audit = BL.writeFile path (encode (object [
    "policyVersion" .= (1 :: Int),
    "roots" .= [object [
        "cargoTarget" .= rootPolicyTarget policy,
        "budgetBytes" .= rootPolicyBudget policy,
        "targetBytes" .= rootPolicyTargetBytes policy,
        "minimumAgeSeconds" .= rootPolicyMinimumAge policy] | policy <- roots],
    "artifactClasses" .= (["LibraryUnits", "CargoTestUnits", "BuildScriptUnits"] :: [T.Text]),
    "auditDirectory" .= audit]))

sha256Hex :: FilePath -> IO T.Text
sha256Hex path = do
  bytes <- B.readFile path
  pure (hex (SHA.finalize (SHA.update SHA.init bytes)))
 where
  hex = T.pack . concatMap byte . B.unpack
  byte value = let text = showHex value "" in replicate (2 - length text) '0' ++ text

invoke :: FilePath -> [String] -> IO ToolResult
invoke executable arguments = do
  (code, out, err) <- readProcessWithExitCode executable arguments ""
  pure (ToolResult code (TE.encodeUtf8 (T.pack out)) err)

invokeBounded :: FilePath -> [String] -> IO ToolResult
invokeBounded executable arguments = do
  (code, out, err) <- readProcessWithExitCode "timeout" (["20", executable] ++ arguments) ""
  pure (ToolResult code (TE.encodeUtf8 (T.pack out)) err)

decodeOutput :: ToolResult -> IO RetainOutput
decodeOutput result = do
  check (resultCode result == ExitSuccess) ("retain failed: " ++ show (resultCode result) ++ "\n" ++ resultErr result)
  case eitherDecodeStrict' (resultOut result) of
    Right output -> pure output
    Left err -> ioError (userError ("invalid retain output: " ++ err))

runPreview :: FilePath -> FilePath -> IO RetainOutput
runPreview executable policy = decodeOutput =<< invoke executable ["retain", "--policy", policy]

runApply :: FilePath -> FilePath -> IO RetainOutput
runApply executable policy = do
  digest <- sha256Hex policy
  decodeOutput =<< invoke executable ["retain", "--policy", policy, "--apply", "--approved-policy-sha256", T.unpack digest]

runApplyWithoutDAC :: FilePath -> FilePath -> IO RetainOutput
runApplyWithoutDAC executable policy = do
  digest <- sha256Hex policy
  decodeOutput =<< invoke "setpriv" ["--bounding-set=-dac_override,-dac_read_search", "--", executable, "retain", "--policy", policy, "--apply", "--approved-policy-sha256", T.unpack digest]

runApplyBounded :: FilePath -> FilePath -> IO RetainOutput
runApplyBounded executable policy = do
  digest <- sha256Hex policy
  decodeOutput =<< invokeBounded executable ["retain", "--policy", policy, "--apply", "--approved-policy-sha256", T.unpack digest]

rootReport :: RetainOutput -> IO RootReport
rootReport output = case outputRootReports output of
  [report] -> pure report
  _ -> ioError (userError "expected exactly one root report")

expectPolicy :: RetainOutput -> FilePath -> RunMode -> IO ()
expectPolicy output policy mode = do
  digest <- sha256Hex policy
  check (outputPolicySha256 output == digest) "policy digest differs from the policy bytes"
  check (outputRunMode output == mode) ("run mode " ++ show (outputRunMode output) ++ ", expected " ++ show mode)

expectStatus :: RootReport -> RetentionStatus -> IO ()
expectStatus report expected =
  check (reportStatus report == expected) ("status " ++ show (reportStatus report) ++ ", expected " ++ show expected)

expectSelectedUnits :: RootReport -> [Unit] -> IO ()
expectSelectedUnits report units = do
  let expected = sort (concatMap (map T.pack . unitSelectedMembers) units)
      actual = sort (concatMap reportedMembers (reportUnits report))
  check (actual == expected) ("selected unit members differ: " ++ show actual)

assertRetained :: FilePath -> Unit -> IO ()
assertRetained root unit = forM_ (unitFiles unit) $ \path ->
  doesPathExist (root </> path) >>= \exists -> check exists ("unit member removed: " ++ path)

assertArtifactsRemoved :: FilePath -> Unit -> IO ()
assertArtifactsRemoved root unit = forM_ (unitSelectedMembers unit) $ \path ->
  doesPathExist (root </> path) >>= \exists -> check (not exists) ("unit artifact retained: " ++ path)

assertUnchanged :: FilePath -> [(FilePath, B.ByteString)] -> IO ()
assertUnchanged root expected = forM_ expected $ \(path, bytes) -> do
  actual <- B.readFile (root </> path)
  check (actual == bytes) ("fixture content changed: " ++ path)

assertNoRunFiles :: FilePath -> IO ()
assertNoRunFiles audit = do
  exists <- doesDirectoryExist audit
  when exists $ do
    entries <- listDirectory audit
    check (null entries) ("unexpected run files: " ++ show entries)

assertAudit :: RootReport -> FilePath -> IO ()
assertAudit report audit = case reportAuditPath report of
  Nothing -> ioError (userError "apply reported no audit path")
  Just path -> do
    check (splitDirectories audit `isPrefixOf` splitDirectories path) "audit path is outside the policy audit directory"
    doesPathExist path >>= \exists -> check exists ("audit file missing: " ++ path)

configure :: FilePath -> FilePath -> FilePath -> ByteCount -> IO ()
configure root audit policy (ByteCount removal) = do
  measured <- apparentBytes root
  let budget = measured - slack
      target = measured - removal
  check (budget > 0 && target >= 0 && target < budget) "test target is outside the policy range"
  writePolicy policy [RootPolicy root (ByteCount budget) (ByteCount target) minimumAgeSeconds] audit

sumUnits :: FilePath -> [Unit] -> IO ByteCount
sumUnits root units = do
  sizes <- forM units $ \unit -> fmap sum $ forM (unitSelectedMembers unit) $ \path -> do
    status <- getSymbolicLinkStatus (root </> path)
    pure (fromIntegral (fileSize status))
  pure (ByteCount (sum sizes))

withChild :: CreateProcess -> IO a -> IO a
withChild command action = bracket start stop (const action)
 where
  start = do
    (_, Just output, _, child) <- createProcess command { std_out = CreatePipe }
    ready <- B.hGet output 5
    hClose output
    check (ready == "ready") "child did not reach its active state"
    pure child
  stop child = terminateProcess child >> waitForProcess child >> pure ()

main :: IO ()
main = do
  arguments <- getArgs
  case arguments of
    [tool] -> do
      self <- getExecutablePath
      environment <- getEnvironment
      let fixtureEnvironment = ("LC_ALL", "C.UTF-8") : filter ((/= "LC_ALL") . fst) environment
      executeFile "unshare" True
        ["--user", "--map-root-user", "--pid", "--fork", "--mount-proc", self, "--isolated-fixtures", tool] (Just fixtureEnvironment)
    ["--isolated-fixtures", tool] -> runFixtures tool
    ["--mapped-fixture", file] -> do
      mapped <- bracket (openFd file ReadOnly defaultFileFlags) closeFd $ \(Fd fd) ->
        c_mmap nullPtr 4096 1 2 fd 0
      check (mapped /= intPtrToPtr (-1)) "cannot map the synthetic fixture"
      putStr "ready"
      hFlush stdout
      threadDelay 60_000_000
    _ -> ioError (userError "storage-retention-tests TOOL")

runFixtures :: FilePath -> IO ()
runFixtures argument = do
  executable <- canonicalizePath argument
  self <- getExecutablePath
  base <- getTemporaryDirectory
  now <- getPOSIXTime
  let old = now - 10 * 86400
      oldNanoseconds = floor (toRational old * 1_000_000_000) :: Integer
  bracket (do
      (name, handle) <- openTempFile base "storage-retention-tests"
      hClose handle
      removeFile name
      createDirectory name
      canonicalizePath name) removeDirectoryRecursive $ \workspace -> do
    let fresh name action = do
          let root = workspace </> name
              audit = workspace </> (name ++ ".audit")
              policy = workspace </> (name ++ ".policy.json")
          createDirectory root
          action root audit policy
    fresh "below-budget" $ \root audit policy -> do
      let unit = libraryUnit "alpha"
      buildRoot root [unit]
      ageTree old root
      measured <- apparentBytes root
      writePolicy policy [RootPolicy root (ByteCount (measured + slack)) (ByteCount (measured - 1024)) minimumAgeSeconds] audit
      paths <- relativePaths root
      contents <- snapshot root (unitFiles unit)
      preview <- runPreview executable policy
      expectPolicy preview policy PreviewMode
      previewReport <- rootReport preview
      expectStatus previewReport BelowBudget
      check (countValue (reportSelectedUnitCount previewReport) == 0) "below-budget preview selected a unit"
      check (countValue (reportRemovedUnitCount previewReport) == 0) "below-budget preview removed a unit"
      check (byteValue (reportBeforeBytes previewReport) == measured) "below-budget preview bytes differ from a fresh measurement"
      check (byteValue (reportBeforeBytes previewReport) <= byteValue (reportBudgetBytes previewReport)) "below-budget preview exceeds its budget"
      check (not (reportTargetMet previewReport)) "below-budget preview claimed the lower target was met"
      check (null (reportUnits previewReport)) "below-budget preview reported units"
      check (reportAuditPath previewReport == Nothing) "below-budget preview reported an audit"
      check (reportInspectionComplete previewReport == Nothing) "below-budget preview inspected processes"
      relativePaths root >>= \current -> check (current == paths) "below-budget preview changed the root"
      assertUnchanged root contents
      output <- runApply executable policy
      expectPolicy output policy ApplyMode
      report <- rootReport output
      expectStatus report BelowBudget
      check (countValue (reportSelectedUnitCount report) == 0) "below-budget apply selected a unit"
      check (countValue (reportRemovedUnitCount report) == 0) "below-budget apply removed a unit"
      check (byteValue (reportBeforeBytes report) == measured) "below-budget apply bytes differ from a fresh measurement"
      check (byteValue (reportBeforeBytes report) <= byteValue (reportBudgetBytes report)) "below-budget apply exceeds its budget"
      check (not (reportTargetMet report)) "below-budget apply claimed the lower target was met"
      check (null (reportUnits report)) "below-budget apply reported units"
      check (reportAuditPath report == Nothing) "below-budget apply wrote an audit"
      check (reportInspectionComplete report == Nothing) "below-budget apply inspected processes"
      relativePaths root >>= \current -> check (current == paths) "below-budget apply changed the root"
      assertUnchanged root contents
      assertNoRunFiles audit
    fresh "preview" $ \root audit policy -> do
      let oldUnit = libraryUnit "alpha"
          recentUnit = libraryUnit "beta"
      buildRoot root [oldUnit, recentUnit]
      ageTree old root
      refreshUnit root now recentUnit
      removal <- sumUnits root [oldUnit]
      configure root audit policy removal
      measuredBefore <- apparentBytes root
      paths <- relativePaths root
      contents <- snapshot root (unitFiles oldUnit ++ unitFiles recentUnit)
      output <- runPreview executable policy
      expectPolicy output policy PreviewMode
      report <- rootReport output
      expectStatus report PreviewSelection
      check (countValue (reportSelectedUnitCount report) == 1) "preview selected the wrong unit count"
      check (countValue (reportRemovedUnitCount report) == 0) "preview removed a unit"
      check (byteValue (reportBeforeBytes report) == measuredBefore) "preview bytes differ from a fresh measurement"
      check (reportTargetMet report) "preview did not project a met target"
      check (reportAuditPath report == Nothing) "preview reported an audit"
      check (reportInspectionComplete report == Just True) "preview inspection was incomplete"
      forM_ (reportUnits report) $ \unit ->
        check (nanosecondsValue (reportedNewestMtimeNanoseconds unit) < oldNanoseconds + 86400 * 1_000_000_000) "preview unit is not aged"
      expectSelectedUnits report [oldUnit]
      relativePaths root >>= \current -> check (current == paths) "preview changed the root"
      assertUnchanged root contents
      assertNoRunFiles audit
    fresh "eviction" $ \root audit policy -> do
      let library = libraryUnit "alpha"
          script = buildScriptUnit
          tests = testUnit "gamma"
          recent = libraryUnit "delta"
          evicted = [library, script, tests]
      buildRoot root [library, script, tests, recent]
      ageTree old root
      refreshUnit root now recent
      removal <- sumUnits root evicted
      configure root audit policy removal
      measuredBefore <- apparentBytes root
      retained <- snapshot root (unitFiles recent)
      output <- runApply executable policy
      expectPolicy output policy ApplyMode
      report <- rootReport output
      expectStatus report Complete
      measuredAfter <- apparentBytes root
      check (countValue (reportSelectedUnitCount report) == 3) "apply selected the wrong unit count"
      check (countValue (reportRemovedUnitCount report) == 3) "apply removed the wrong unit count"
      check (countValue (reportRecentUnitCount report) == 1) "apply did not report the recent unit"
      check (countValue (reportBlockedUnitCount report) == 0) "apply blocked an unblocked unit"
      check (byteValue (reportBeforeBytes report) == measuredBefore) "reported before bytes differ from a fresh measurement"
      check (byteValue (reportAfterBytes report) == measuredAfter) "reported after bytes differ from a fresh measurement"
      check (reportTargetMet report) "apply did not meet the target"
      check (byteValue (reportAfterBytes report) <= byteValue (reportTargetBytes report)) "met target reported above its target"
      check (reportInspectionComplete report == Just True) "apply inspection was incomplete"
      expectSelectedUnits report evicted
      mapM_ (assertArtifactsRemoved root) evicted
      assertRetained root recent
      assertUnchanged root retained
      assertAudit report audit
    fresh "oldest-first" $ \root audit policy -> do
      let oldest = libraryUnit "zeta"
          newerOld = libraryUnit "alpha"
          recent = libraryUnit "beta"
      buildRoot root [oldest, newerOld, recent]
      ageTree old root
      refreshUnit root (now - 5 * 86400) newerOld
      refreshUnit root now recent
      removal <- sumUnits root [oldest]
      configure root audit policy removal
      retained <- snapshot root (unitFiles newerOld ++ unitFiles recent)
      output <- runApply executable policy
      report <- rootReport output
      expectStatus report Complete
      expectSelectedUnits report [oldest]
      assertArtifactsRemoved root oldest
      assertUnchanged root retained
    fresh "recent-member" $ \root audit policy -> do
      let protected = libraryUnit "alpha"
          eligible = libraryUnit "beta"
      buildRoot root [protected, eligible]
      ageTree old root
      setTimes now (root </> NonEmpty.head (unitOutputs protected))
      removal <- sumUnits root [eligible]
      configure root audit policy removal
      retained <- snapshot root (unitFiles protected)
      output <- runApply executable policy
      report <- rootReport output
      expectStatus report Complete
      check (countValue (reportSelectedUnitCount report) == 1) "recent-member selection count differs"
      check (countValue (reportRemovedUnitCount report) == 1) "recent-member removal count differs"
      check (countValue (reportRecentUnitCount report) == 1) "recent member did not mark its unit recent"
      check (countValue (reportBlockedUnitCount report) == 0) "recent member marked its unit blocked"
      check (reportTargetMet report) "recent-member run did not meet the target"
      expectSelectedUnits report [eligible]
      assertRetained root protected
      assertUnchanged root retained
      assertArtifactsRemoved root eligible
    fresh "recent-control" $ \root audit policy -> do
      let protected = libraryUnit "alpha"
          eligible = libraryUnit "beta"
      buildRoot root [protected, eligible]
      ageTree old root
      setTimes now (root </> unitJson protected)
      removal <- sumUnits root [eligible]
      configure root audit policy removal
      retained <- snapshot root (unitFiles protected)
      output <- runApply executable policy
      report <- rootReport output
      expectStatus report Complete
      check (countValue (reportSelectedUnitCount report) == 1) "recent-control selection count differs"
      check (countValue (reportRecentUnitCount report) == 1) "recent control did not mark its unit recent"
      check (countValue (reportBlockedUnitCount report) == 0) "recent control marked its unit blocked"
      expectSelectedUnits report [eligible]
      assertRetained root protected
      assertUnchanged root retained
      assertArtifactsRemoved root eligible
    fresh "changed-member" $ \root audit policy -> do
      let protected = libraryUnit "alpha"
          eligible = libraryUnit "beta"
          changed = NonEmpty.head (unitOutputs protected)
      buildRoot root [protected, eligible]
      ageTree old root
      removal <- sumUnits root [eligible]
      configure root audit policy removal
      preview <- runPreview executable policy
      previewReport <- rootReport preview
      check (countValue (reportSelectedUnitCount previewReport) == 1) "preview did not select the old unit"
      expectSelectedUnits previewReport [protected]
      put (root </> changed) ("XXXXXXXX" <> B.replicate (payloadBytes - 8) 0x42)
      output <- runApply executable policy
      report <- rootReport output
      expectStatus report Complete
      check (countValue (reportSelectedUnitCount report) == 1) "changed-member selection count differs"
      check (countValue (reportRemovedUnitCount report) == 1) "changed-member removal count differs"
      expectSelectedUnits report [eligible]
      assertRetained root protected
      B.readFile (root </> changed) >>= \bytes -> check (B.take 8 bytes == "XXXXXXXX") "changed member was deleted"
      assertArtifactsRemoved root eligible
    fresh "mapped-member" $ \root audit policy -> do
      let protected = libraryUnit "alpha"
          eligible = libraryUnit "beta"
      buildRoot root [protected, eligible]
      ageTree old root
      removal <- sumUnits root [eligible]
      configure root audit policy removal
      retained <- snapshot root (unitFiles protected)
      withChild ((proc self ["--mapped-fixture", root </> NonEmpty.head (unitOutputs protected)]) { cwd = Just workspace }) $ do
        output <- runApply executable policy
        report <- rootReport output
        expectStatus report Complete
        check (countValue (reportRemovedUnitCount report) == 1) "mapped-member removal count differs"
        check (countValue (reportBlockedUnitCount report) == 1) "mapped member did not block its unit"
        expectSelectedUnits report [eligible]
        assertRetained root protected
        assertUnchanged root retained
        assertArtifactsRemoved root eligible
    fresh "open-member" $ \root audit policy -> do
      let protected = libraryUnit "alpha"
          eligible = libraryUnit "beta"
      buildRoot root [protected, eligible]
      ageTree old root
      removal <- sumUnits root [eligible]
      configure root audit policy removal
      retained <- snapshot root (unitFiles protected)
      withChild (proc "sh" ["-c", "exec 3<\"$1\"; printf ready; exec sleep 60", "reader", root </> NonEmpty.head (unitOutputs protected)]) $ do
        output <- runApply executable policy
        report <- rootReport output
        expectStatus report Complete
        check (countValue (reportRemovedUnitCount report) == 1) "open-member removal count differs"
        check (countValue (reportBlockedUnitCount report) == 1) "open member did not block its unit"
        expectSelectedUnits report [eligible]
        assertRetained root protected
        assertUnchanged root retained
        assertArtifactsRemoved root eligible
    fresh "open-control" $ \root audit policy -> do
      let protected = libraryUnit "alpha"
          eligible = libraryUnit "beta"
      buildRoot root [protected, eligible]
      ageTree old root
      removal <- sumUnits root [eligible]
      configure root audit policy removal
      retained <- snapshot root (unitFiles protected)
      withChild (proc "sh" ["-c", "exec 3<\"$1\"; printf ready; exec sleep 60", "reader", root </> unitJson protected]) $ do
        output <- runApply executable policy
        report <- rootReport output
        expectStatus report Complete
        check (countValue (reportBlockedUnitCount report) == 1) "open fingerprint control did not block its unit"
        expectSelectedUnits report [eligible]
        assertUnchanged root retained
        assertArtifactsRemoved root eligible
    fresh "unwritable-artifact-parent" $ \root audit policy -> do
      let protected = libraryUnit "alpha"
          eligible = buildScriptUnit
          deps = root </> "debug/deps"
      buildRoot root [protected, eligible]
      ageTree old root
      removal <- sumUnits root [protected, eligible]
      configure root audit policy removal
      retained <- snapshot root (unitFiles protected)
      bracket (getSymbolicLinkStatus deps) (setFileMode deps . fileMode) $ \_ -> do
        setFileMode deps 0o555
        output <- runApplyWithoutDAC executable policy
        report <- rootReport output
        expectStatus report TargetUnmet
        check (countValue (reportRemovedUnitCount report) == 1) "undeletable unit prevented independent eviction"
        check (not (reportTargetMet report)) "unwritable artifact parent claimed the target was met"
        assertUnchanged root retained
        assertArtifactsRemoved root eligible
    fresh "hardlinked-member" $ \root audit policy -> do
      let protected = libraryUnit "alpha"
          eligible = libraryUnit "beta"
          linked = NonEmpty.head (unitOutputs protected)
      buildRoot root [protected, eligible]
      ageTree old root
      createLink (root </> linked) (workspace </> "external-hardlink")
      removal <- sumUnits root [eligible]
      configure root audit policy removal
      retained <- snapshot root (unitFiles protected)
      output <- runApply executable policy
      report <- rootReport output
      expectStatus report Complete
      check (countValue (reportRemovedUnitCount report) == 1) "hardlinked-member removal count differs"
      check (countValue (reportBlockedUnitCount report) == 1) "hardlinked member did not block its unit"
      expectSelectedUnits report [eligible]
      assertRetained root protected
      assertUnchanged root retained
      assertArtifactsRemoved root eligible
    fresh "unrelated-symlink" $ \root audit policy -> do
      let eligible = libraryUnit "alpha"
          recent = libraryUnit "beta"
          outside = workspace </> "outside-payload.bin"
          link = "imports/pointer.bin"
      buildRoot root [eligible, recent]
      ageTree old root
      refreshUnit root now recent
      put outside "external payload"
      createDirectoryIfMissing True (takeDirectory (root </> link))
      createSymbolicLink outside (root </> link)
      removal <- sumUnits root [eligible]
      configure root audit policy removal
      retained <- snapshot root (unitFiles recent)
      output <- runApply executable policy
      report <- rootReport output
      expectStatus report Complete
      check (countValue (reportSelectedUnitCount report) == 1) "symlink run selected the wrong unit count"
      check (countValue (reportRemovedUnitCount report) == 1) "symlink run removed the wrong unit count"
      check (countValue (reportBlockedUnitCount report) == 0) "unrelated symlink blocked a unit"
      check (countValue (reportRecentUnitCount report) == 1) "symlink run lost the recent unit"
      expectSelectedUnits report [eligible]
      assertArtifactsRemoved root eligible
      assertRetained root recent
      assertUnchanged root retained
      readSymbolicLink (root </> link) >>= \target -> check (target == outside) "unrelated symlink was replaced"
      B.readFile outside >>= \bytes -> check (bytes == "external payload") "symlink target content changed"
    fresh "symlinked-members" $ \root audit policy -> do
      let symlinkedOutput = libraryUnit "alpha"
          symlinkedControl = libraryUnit "gamma"
          eligible = libraryUnit "beta"
          outside = workspace </> "outside-payload.bin"
          outputPath = NonEmpty.head (unitOutputs symlinkedOutput)
          controlPath = unitJson symlinkedControl
      buildRoot root [symlinkedOutput, symlinkedControl, eligible]
      ageTree old root
      put outside "external payload"
      removeFile (root </> outputPath)
      createSymbolicLink outside (root </> outputPath)
      removeFile (root </> controlPath)
      createSymbolicLink outside (root </> controlPath)
      removal <- sumUnits root [eligible]
      configure root audit policy removal
      output <- runApply executable policy
      report <- rootReport output
      expectStatus report Complete
      check (countValue (reportSelectedUnitCount report) == 1) "symlinked-member selection count differs"
      check (countValue (reportRemovedUnitCount report) == 1) "symlinked-member removal count differs"
      check (countValue (reportBlockedUnitCount report) == 1) "symlinked output did not block its unit"
      expectSelectedUnits report [eligible]
      assertArtifactsRemoved root eligible
      assertRetained root symlinkedOutput
      assertRetained root symlinkedControl
      readSymbolicLink (root </> outputPath) >>= \target -> check (target == outside) "symlinked output was replaced"
      readSymbolicLink (root </> controlPath) >>= \target -> check (target == outside) "symlinked control was replaced"
      B.readFile outside >>= \bytes -> check (bytes == "external payload") "symlink target content changed"
    fresh "target-unmet" $ \root audit policy -> do
      let eligible = libraryUnit "alpha"
          recent = libraryUnit "beta"
      buildRoot root [eligible, recent]
      ageTree old root
      refreshUnit root now recent
      removal <- sumUnits root [eligible, recent]
      configure root audit policy removal
      retained <- snapshot root (unitFiles recent)
      output <- runApply executable policy
      report <- rootReport output
      expectStatus report TargetUnmet
      measuredAfter <- apparentBytes root
      check (not (reportTargetMet report)) "unmet target was reported as met"
      check (countValue (reportSelectedUnitCount report) == 1) "target-unmet selection count differs"
      check (countValue (reportRemovedUnitCount report) == 1) "target-unmet removal count differs"
      check (countValue (reportRecentUnitCount report) == 1) "target-unmet recent count differs"
      check (byteValue (reportAfterBytes report) == measuredAfter) "reported after bytes differ from a fresh measurement"
      check (byteValue (reportAfterBytes report) > byteValue (reportTargetBytes report)) "target-unmet run reached its target"
      check (reportInspectionComplete report == Just True) "target-unmet inspection was incomplete"
      expectSelectedUnits report [eligible]
      assertArtifactsRemoved root eligible
      assertRetained root recent
      assertUnchanged root retained
      assertAudit report audit
    fresh "busy-lock" $ \root audit policy -> do
      let eligible = libraryUnit "alpha"
          recent = libraryUnit "beta"
      buildRoot root [eligible, recent]
      ageTree old root
      refreshUnit root now recent
      removal <- sumUnits root [eligible]
      configure root audit policy removal
      paths <- relativePaths root
      contents <- snapshot root (unitFiles eligible ++ unitFiles recent)
      bracket (openFd (root </> "debug/.cargo-lock") ReadWrite defaultFileFlags { cloexec = True }) closeFd $ \(Fd fd) -> do
        result <- c_flock fd 6
        check (result == 0) "cannot acquire fixture Cargo lock"
        output <- runApplyBounded executable policy
        report <- rootReport output
        expectStatus report DeferredBusy
        check (countValue (reportRemovedUnitCount report) == 0) "busy lock removed a unit"
        check (reportAuditPath report == Nothing) "busy lock wrote an audit"
        check (reportInspectionComplete report == Nothing) "busy lock inspected processes"
        relativePaths root >>= \current -> check (current == paths) "busy lock changed the root"
        assertUnchanged root contents
        assertNoRunFiles audit
      output <- runApply executable policy
      report <- rootReport output
      expectStatus report Complete
      check (countValue (reportRemovedUnitCount report) == 1) "released lock did not reclaim the eligible unit"
      assertArtifactsRemoved root eligible
      assertRetained root recent
    fresh "missing-approval" $ \root audit policy -> do
      let eligible = libraryUnit "alpha"
          recent = libraryUnit "beta"
      buildRoot root [eligible, recent]
      ageTree old root
      refreshUnit root now recent
      removal <- sumUnits root [eligible]
      configure root audit policy removal
      contents <- snapshot root (unitFiles eligible ++ unitFiles recent)
      result <- invoke executable ["retain", "--policy", policy, "--apply"]
      check (resultCode result /= ExitSuccess) "apply without standing approval succeeded"
      assertUnchanged root contents
      assertNoRunFiles audit
    fresh "wrong-approval" $ \root audit policy -> do
      let eligible = libraryUnit "alpha"
          recent = libraryUnit "beta"
      buildRoot root [eligible, recent]
      ageTree old root
      refreshUnit root now recent
      removal <- sumUnits root [eligible]
      configure root audit policy removal
      contents <- snapshot root (unitFiles eligible ++ unitFiles recent)
      result <- invoke executable ["retain", "--policy", policy, "--apply", "--approved-policy-sha256", replicate 64 '0']
      check (resultCode result /= ExitSuccess) "apply with a wrong standing digest succeeded"
      assertUnchanged root contents
      assertNoRunFiles audit
      malformed <- invoke executable ["retain", "--policy", policy, "--apply", "--approved-policy-sha256", "not-a-digest"]
      check (resultCode malformed /= ExitSuccess) "apply with a malformed standing digest succeeded"
      assertUnchanged root contents
      assertNoRunFiles audit
    fresh "repeated-noop" $ \root audit policy -> do
      let eligible = libraryUnit "alpha"
          recent = libraryUnit "beta"
      buildRoot root [eligible, recent]
      ageTree old root
      refreshUnit root now recent
      removal <- sumUnits root [eligible]
      configure root audit policy removal
      retained <- snapshot root (unitFiles recent)
      first <- runApply executable policy
      firstReport <- rootReport first
      expectStatus firstReport Complete
      measuredAfterFirst <- apparentBytes root
      check (byteValue (reportAfterBytes firstReport) == measuredAfterFirst) "first apply reported after bytes differ from a fresh measurement"
      check (countValue (reportRemovedUnitCount firstReport) == 1) "first apply did not reclaim the eligible unit"
      assertArtifactsRemoved root eligible
      assertUnchanged root retained
      audits <- sort <$> listDirectory audit
      check (not (null audits)) "first apply wrote no audit"
      measuredBeforeSecond <- apparentBytes root
      second <- runApply executable policy
      secondReport <- rootReport second
      expectStatus secondReport BelowBudget
      check (countValue (reportRemovedUnitCount secondReport) == 0) "repeated run removed a unit"
      check (countValue (reportSelectedUnitCount secondReport) == 0) "repeated run selected a unit"
      check (byteValue (reportBeforeBytes secondReport) == measuredBeforeSecond) "repeated run bytes differ from a fresh measurement"
      check (reportAuditPath secondReport == Nothing) "repeated run wrote an audit"
      laterAudits <- sort <$> listDirectory audit
      check (laterAudits == audits) "repeated run changed the audit directory"
      assertArtifactsRemoved root eligible
      assertUnchanged root retained
  putStrLn "storage retention behavioral tests passed"
