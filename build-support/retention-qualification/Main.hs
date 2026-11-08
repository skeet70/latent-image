{-# LANGUAGE DeriveAnyClass #-}
{-# LANGUAGE DeriveGeneric #-}
{-# LANGUAGE DerivingStrategies #-}
{-# LANGUAGE OverloadedStrings #-}
module Main (main) where

import Control.Monad (filterM, foldM, unless)
import Data.Aeson (FromJSON (parseJSON), ToJSON, eitherDecodeStrict', encode, object, withObject, withText, (.:), (.=))
import qualified Crypto.Hash.SHA256 as SHA
import Data.Bits ((.&.))
import qualified Data.ByteString as Bytes
import qualified Data.ByteString.Char8 as B
import qualified Data.ByteString.Lazy as BL
import Data.Char (isHexDigit)
import Data.List (sort)
import qualified Data.Set as Set
import Data.Text (Text)
import qualified Data.Text as T
import qualified Data.Text.Encoding as TE
import GHC.Clock (getMonotonicTimeNSec)
import GHC.Generics (Generic)
import qualified GHC.Foreign as GHC
import GHC.IO.Encoding (getFileSystemEncoding)
import Numeric (showHex, showOct)
import System.Directory (canonicalizePath, doesPathExist)
import System.Environment (getArgs, getEnv)
import System.Exit (ExitCode (..), exitFailure)
import System.FilePath ((</>), isAbsolute, normalise, splitDirectories)
import System.IO (IOMode (ReadMode), hPutStrLn, stderr, withBinaryFile)
import System.Posix.Files (fileMode, getSymbolicLinkStatus, isRegularFile, isSymbolicLink, readSymbolicLink)
import System.Process (readProcessWithExitCode)

newtype SourceGeneration = SourceGeneration Text deriving stock (Eq) deriving newtype (ToJSON, FromJSON)
newtype SourceInput = SourceInput FilePath deriving stock (Eq, Ord)
instance FromJSON SourceInput where
  parseJSON = withText "source input" $ \text -> do
    let input = T.unpack text
    unless (not (null input) && not (isAbsolute input) && '\0' `notElem` input && normalise input == input && all (`notElem` [".", ".."]) (splitDirectories input)) (fail "invalid captured source input")
    pure (SourceInput input)
data SourceManifest = SourceManifest SourceGeneration [SourceInput]
instance FromJSON SourceManifest where
  parseJSON = withObject "source manifest" $ \o -> do
    schema <- o .: "schema"
    unless (schema == (2 :: Int)) (fail "unsupported source manifest schema")
    SourceManifest <$> o .: "digest" <*> o .: "inputs"
newtype ArtifactPath = ArtifactPath FilePath deriving stock (Eq, Ord, Show) deriving newtype (ToJSON)
newtype WallSeconds = WallSeconds Double deriving newtype (ToJSON)
newtype EventCount = EventCount Int deriving stock (Eq) deriving newtype (ToJSON)
newtype UnitCount = UnitCount Int deriving newtype (ToJSON, FromJSON)
newtype UnitFingerprint = UnitFingerprint FilePath deriving stock (Eq, Ord) deriving newtype (FromJSON)
newtype ReportPath = ReportPath FilePath
newtype PolicyPath = PolicyPath FilePath
newtype ToolExecutable = ToolExecutable FilePath
newtype ApprovedPolicy = ApprovedPolicy String deriving newtype (ToJSON)
newtype AvailableBytes = AvailableBytes Integer deriving newtype (ToJSON)
newtype QualificationRoot = QualificationRoot FilePath deriving stock (Eq) deriving newtype (FromJSON, ToJSON)
newtype QualificationPolicy = QualificationPolicy [QualificationRoot]
instance FromJSON QualificationPolicy where
  parseJSON = withObject "qualification policy" $ \o -> do
    roots <- o .: "roots"
    QualificationPolicy <$> mapM (withObject "qualification root" (\r -> r .: "cargoTarget")) roots

data CargoMessage = Artifact Bool [ArtifactPath] | Other
instance FromJSON CargoMessage where
  parseJSON = withObject "Cargo message" $ \o -> do
    reason <- o .: "reason"
    if (reason :: Text) == "compiler-artifact" then Artifact <$> o .: "fresh" <*> (map ArtifactPath <$> o .: "filenames") else pure Other

data BuildResult = BuildResult { successful :: Bool, wallSeconds :: WallSeconds, nativeFreshEvents :: EventCount, nativeCompiledEvents :: EventCount, nativeCompiledPaths :: [ArtifactPath] } deriving (Generic, ToJSON)
newtype SelectedMember = SelectedMember FilePath

data RetentionUnit = RetentionUnit UnitFingerprint [SelectedMember]
instance FromJSON RetentionUnit where
  parseJSON = withObject "retention unit" $ \o -> RetentionUnit <$> o .: "fingerprint" <*> (map SelectedMember <$> o .: "members")
data RetentionStatus = BelowBudget | Complete | TargetUnmet | DeferredBusy deriving (Eq, Show, Generic, ToJSON)
instance FromJSON RetentionStatus where
  parseJSON = withText "retention status" $ \value -> case value of
    "BelowBudget" -> pure BelowBudget
    "Complete" -> pure Complete
    "TargetUnmet" -> pure TargetUnmet
    "DeferredBusy" -> pure DeferredBusy
    _ -> fail "unexpected apply retention status"
data RetentionRootResult = RetentionRootResult QualificationRoot RetentionStatus UnitCount [UnitFingerprint] [RetentionUnit]
instance FromJSON RetentionRootResult where
  parseJSON = withObject "retention root" $ \o -> RetentionRootResult <$> o .: "cargoTarget" <*> o .: "status" <*> o .: "removedUnitCount" <*> o .: "removedUnitFingerprints" <*> o .: "units"
newtype RetentionResult = RetentionResult [RetentionRootResult]
instance FromJSON RetentionResult where
  parseJSON = withObject "retention result" $ \o -> RetentionResult <$> o .: "rootReports"

verifySource :: SourceGeneration -> IO ()
verifySource generation = do
  SourceManifest expected inputs <- either (ioError . userError) pure . eitherDecodeStrict' =<< B.readFile ".looklab-generation.json"
  unless (expected == generation && not (null inputs) && length inputs == Set.size (Set.fromList inputs)) (ioError (userError "invalid captured source manifest"))
  context <- foldM fingerprint SHA.init (sort inputs)
  let actual = SourceGeneration (TE.decodeUtf8 (renderDigest (SHA.finalize context)))
  unless (actual == generation) (ioError (userError "captured source bytes changed; leave automation disabled"))
 where
  fingerprint !context (SourceInput input) = do
    status <- getSymbolicLinkStatus input
    record <- if isSymbolicLink status then do
      target <- filesystemBytes =<< readSymbolicLink input
      pure (B.intercalate "\0" ["link", "0", renderDigest (SHA.hash ("link\0" <> target))])
    else if isRegularFile status then do
      content <- withBinaryFile input ReadMode (hashContents SHA.init)
      pure (B.intercalate "\0" ["file", B.pack (showOct (fileMode status .&. 0o7777) ""), renderDigest content])
    else ioError (userError "unsupported captured source input")
    name <- filesystemBytes input
    pure (SHA.update (SHA.update (SHA.update (SHA.update context name) "\0") record) "\n")
  hashContents !context handle = do
    chunk <- B.hGetSome handle (1024 * 1024)
    if B.null chunk then pure (SHA.finalize context) else hashContents (SHA.update context chunk) handle
  filesystemBytes input = do
    encoding <- getFileSystemEncoding
    GHC.withCStringLen encoding input B.packCStringLen
  renderDigest = B.pack . concatMap (\byte -> let value = showHex byte "" in replicate (2 - length value) '0' ++ value) . Bytes.unpack

build :: IO BuildResult
build = do
  begin <- getMonotonicTimeNSec
  (prepared, preparationOut, preparationErr) <- readProcessWithExitCode "sh" ["site/scripts/wasm-assets.sh", "--prep", "--scene", "--match"] ""
  hPutStrLn stderr (preparationOut ++ preparationErr)
  if prepared /= ExitSuccess then do
    end <- getMonotonicTimeNSec
    pure (BuildResult False (elapsed begin end) (EventCount 0) (EventCount 0) [])
  else do
    (code, output, diagnostics) <- readProcessWithExitCode "cargo" ["test", "--locked", "--workspace", "--features", "looklab-fit/synth", "--no-run", "--jobs", "2", "--message-format=json"] ""
    end <- getMonotonicTimeNSec
    hPutStrLn stderr diagnostics
    messages <- mapM (either (ioError . userError) pure . eitherDecodeStrict') (filter (not . B.null) (B.lines (TE.encodeUtf8 (T.pack output))))
    pure (BuildResult (code == ExitSuccess) (elapsed begin end) (EventCount (length [() | Artifact True _ <- messages])) (EventCount (length [() | Artifact False _ <- messages])) (Set.toList (Set.fromList (concat [paths | Artifact False paths <- messages]))))
 where
  elapsed begin end = WallSeconds (fromIntegral (end - begin) / 1_000_000_000)

retain :: ToolExecutable -> PolicyPath -> ApprovedPolicy -> IO RetentionResult
retain (ToolExecutable executable) (PolicyPath policy) (ApprovedPolicy approved) = do
  (code, output, diagnostics) <- readProcessWithExitCode "sudo" ["-n", "--", executable, "retain", "--policy", policy, "--apply", "--approved-policy-sha256", approved, "+RTS", "-M2G", "-RTS"] ""
  hPutStrLn stderr diagnostics
  unless (code == ExitSuccess) (ioError (userError "retention invocation failed; inspect its diagnostics and audit"))
  either (ioError . userError) pure (eitherDecodeStrict' (TE.encodeUtf8 (T.pack output)))

available :: QualificationRoot -> IO AvailableBytes
available (QualificationRoot root) = do
  (code, output, diagnostics) <- readProcessWithExitCode "df" ["-B1", "--output=avail", "--", root] ""
  case (code, words output) of
    (ExitSuccess, [_, value]) | [(bytes, "")] <- reads value -> pure (AvailableBytes bytes)
    _ -> ioError (userError ("cannot measure available space: " ++ diagnostics))

writeResult :: ReportPath -> BL.ByteString -> IO ()
writeResult (ReportPath destination) bytes = BL.writeFile destination bytes >> BL.putStr bytes

main :: IO ()
main = do
  arguments <- getArgs
  source <- T.pack <$> getEnv "LOOKLAB_SOURCE_GENERATION"
  unless (T.length source == 64 && T.all isHexDigit source) (ioError (userError "expected a captured source generation"))
  let generation = SourceGeneration source
  verifySource generation
  rootPath <- getEnv "CARGO_TARGET_DIR"
  canonicalRoot <- canonicalizePath rootPath
  unless (canonicalRoot == rootPath) (ioError (userError "expected a canonical Cargo target root"))
  let currentRoot = QualificationRoot canonicalRoot
  case arguments of
    ["baseline", destination] -> do
      prime <- build
      verifySource generation
      if not (successful prime) then do
        writeResult (ReportPath destination) (encode (object ["sourceGeneration" .= generation, "cargoTarget" .= currentRoot, "prime" .= prime, "baselineReady" .= False]))
        exitFailure
      else do
        warm <- build
        verifySource generation
        writeResult (ReportPath destination) (encode (object ["sourceGeneration" .= generation, "cargoTarget" .= currentRoot, "prime" .= prime, "warmBaseline" .= warm, "baselineReady" .= successful warm]))
        unless (successful warm) exitFailure
    ["qualify", executable, policy, approved, destination] -> do
      unless (length approved == 64 && all isHexDigit approved) (ioError (userError "invalid approved policy digest"))
      policyBytes <- B.readFile policy
      QualificationPolicy roots <- either (ioError . userError) pure (eitherDecodeStrict' policyBytes)
      unless (roots == [currentRoot]) (ioError (userError "qualification policy must contain exactly the current Cargo target root"))
      before <- build
      verifySource generation
      unless (successful before) (ioError (userError "normal build failed before retention; no deletion attempted"))
      beforeSpace <- available currentRoot
      RetentionResult first <- retain (ToolExecutable executable) (PolicyPath policy) (ApprovedPolicy approved)
      verifySource generation
      immediateSpace <- available currentRoot
      after <- build
      verifySource generation
      unless (successful after) (ioError (userError "normal build failed after retention; leave automation disabled"))
      afterBuildSpace <- available currentRoot
      let firstRemoved = sum [count | RetentionRootResult _ _ (UnitCount count) _ _ <- first]
          removedMembers = Set.fromList [ArtifactPath (root </> member) | RetentionRootResult (QualificationRoot root) _ _ removedMarkers units <- first,
            RetentionUnit marker members <- units, marker `elem` removedMarkers, SelectedMember member <- members]
      recreated <- filterM (\(ArtifactPath path) -> doesPathExist path) (Set.toList removedMembers)
      RetentionResult second <- if firstRemoved == 0 then pure (RetentionResult []) else retain (ToolExecutable executable) (PolicyPath policy) (ApprovedPolicy approved)
      verifySource generation
      let repeatedRemoved = sum [count | RetentionRootResult _ _ (UnitCount count) _ _ <- second]
          repeatVerified = case second of
            [RetentionRootResult actual status _ _ _] -> actual == currentRoot && status /= DeferredBusy
            _ -> False
          accepted = firstRemoved > 0 && null recreated && nativeCompiledEvents after == EventCount 0 && repeatVerified && repeatedRemoved == 0
      writeResult (ReportPath destination) (encode (object ["sourceGeneration" .= generation, "cargoTarget" .= currentRoot, "policySha256" .= ApprovedPolicy approved, "before" .= before, "after" .= after, "availableBeforeRetention" .= beforeSpace, "availableImmediatelyAfterRetention" .= immediateSpace, "availableAfterNormalBuild" .= afterBuildSpace, "spaceMeasurementsIncludeConcurrentActivity" .= True, "removedUnitCount" .= UnitCount firstRemoved, "recreatedSelectedArtifacts" .= recreated, "repeatInspectionVerified" .= repeatVerified, "repeatRemovedUnitCount" .= UnitCount repeatedRemoved, "qualificationAccepted" .= accepted]))
      unless accepted exitFailure
    _ -> ioError (userError "normal-build-measure baseline REPORT | qualify TOOL POLICY APPROVED_SHA256 REPORT")
