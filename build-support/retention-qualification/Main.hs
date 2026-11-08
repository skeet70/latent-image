{-# LANGUAGE DeriveAnyClass #-}
{-# LANGUAGE DeriveGeneric #-}
{-# LANGUAGE DerivingStrategies #-}
{-# LANGUAGE OverloadedStrings #-}
module Main (main) where

import Control.Monad (filterM, unless)
import Data.Aeson (FromJSON (parseJSON), ToJSON, eitherDecodeStrict', encode, object, withObject, (.:), (.=))
import qualified Data.ByteString.Char8 as B
import qualified Data.ByteString.Lazy as BL
import Data.Char (isHexDigit)
import qualified Data.Set as Set
import Data.Text (Text)
import qualified Data.Text as T
import qualified Data.Text.Encoding as TE
import GHC.Clock (getMonotonicTimeNSec)
import GHC.Generics (Generic)
import System.Directory (doesPathExist)
import System.Environment (getArgs, getEnv)
import System.Exit (ExitCode (..), exitFailure)
import System.FilePath ((</>))
import System.IO (hPutStrLn, stderr)
import System.Process (readProcessWithExitCode)

newtype SourceGeneration = SourceGeneration Text deriving newtype (ToJSON)
newtype ArtifactPath = ArtifactPath FilePath deriving stock (Eq, Ord, Show) deriving newtype (ToJSON)
newtype WallSeconds = WallSeconds Double deriving newtype (ToJSON)
newtype EventCount = EventCount Int deriving newtype (ToJSON)
newtype UnitCount = UnitCount Int deriving newtype (ToJSON, FromJSON)
newtype UnitFingerprint = UnitFingerprint FilePath deriving stock (Eq, Ord) deriving newtype (FromJSON)
newtype ReportPath = ReportPath FilePath
newtype PolicyPath = PolicyPath FilePath
newtype ToolExecutable = ToolExecutable FilePath
newtype ApprovedPolicy = ApprovedPolicy String
newtype AvailableBytes = AvailableBytes Integer deriving newtype (ToJSON)
newtype QualificationRoot = QualificationRoot FilePath deriving stock (Eq) deriving newtype (FromJSON)
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

data BuildResult = BuildResult { successful :: Bool, wallSeconds :: WallSeconds, freshEvents :: EventCount, compiledEvents :: EventCount, compiledPaths :: [ArtifactPath] } deriving (Generic, ToJSON)
newtype SelectedMember = SelectedMember FilePath

data RetentionUnit = RetentionUnit UnitFingerprint [SelectedMember]
instance FromJSON RetentionUnit where
  parseJSON = withObject "retention unit" $ \o -> RetentionUnit <$> o .: "fingerprint" <*> (map SelectedMember <$> o .: "members")
data RetentionRootResult = RetentionRootResult FilePath UnitCount [UnitFingerprint] [RetentionUnit]
instance FromJSON RetentionRootResult where
  parseJSON = withObject "retention root" $ \o -> RetentionRootResult <$> o .: "cargoTarget" <*> o .: "removedUnitCount" <*> o .: "removedUnitFingerprints" <*> o .: "units"
newtype RetentionResult = RetentionResult [RetentionRootResult]
instance FromJSON RetentionResult where
  parseJSON = withObject "retention result" $ \o -> RetentionResult <$> o .: "rootReports"

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
  (code, output, diagnostics) <- readProcessWithExitCode "sudo" ["-n", "--", executable, "retain", "--policy", policy, "--apply", "--approved-policy-sha256", approved] ""
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
  case arguments of
    ["baseline", destination] -> do
      prime <- build
      if not (successful prime) then do
        writeResult (ReportPath destination) (encode (object ["sourceGeneration" .= generation, "prime" .= prime, "baselineReady" .= False]))
        exitFailure
      else do
        warm <- build
        writeResult (ReportPath destination) (encode (object ["sourceGeneration" .= generation, "prime" .= prime, "warmBaseline" .= warm, "baselineReady" .= successful warm]))
        unless (successful warm) exitFailure
    ["qualify", executable, policy, approved, destination] -> do
      unless (length approved == 64 && all isHexDigit approved) (ioError (userError "invalid approved policy digest"))
      currentRoot <- QualificationRoot <$> getEnv "CARGO_TARGET_DIR"
      policyBytes <- B.readFile policy
      QualificationPolicy roots <- either (ioError . userError) pure (eitherDecodeStrict' policyBytes)
      unless (roots == [currentRoot]) (ioError (userError "qualification policy must contain exactly the current Cargo target root"))
      before <- build
      unless (successful before) (ioError (userError "normal build failed before retention; no deletion attempted"))
      beforeSpace <- available currentRoot
      RetentionResult first <- retain (ToolExecutable executable) (PolicyPath policy) (ApprovedPolicy approved)
      immediateSpace <- available currentRoot
      after <- build
      unless (successful after) (ioError (userError "normal build failed after retention; leave automation disabled"))
      afterBuildSpace <- available currentRoot
      let firstRemoved = sum [count | RetentionRootResult _ (UnitCount count) _ _ <- first]
          removedMembers = Set.fromList [ArtifactPath (root </> member) | RetentionRootResult root _ removedMarkers units <- first,
            RetentionUnit marker members <- units, marker `elem` removedMarkers, SelectedMember member <- members]
      recreated <- filterM (\(ArtifactPath path) -> doesPathExist path) (Set.toList removedMembers)
      RetentionResult second <- if firstRemoved == 0 then pure (RetentionResult []) else retain (ToolExecutable executable) (PolicyPath policy) (ApprovedPolicy approved)
      let repeatedRemoved = sum [count | RetentionRootResult _ (UnitCount count) _ _ <- second]
          accepted = firstRemoved > 0 && null recreated && repeatedRemoved == 0
      writeResult (ReportPath destination) (encode (object ["sourceGeneration" .= generation, "before" .= before, "after" .= after, "availableBeforeRetention" .= beforeSpace, "availableImmediatelyAfterRetention" .= immediateSpace, "availableAfterNormalBuild" .= afterBuildSpace, "spaceMeasurementsIncludeConcurrentActivity" .= True, "removedUnitCount" .= UnitCount firstRemoved, "recreatedSelectedArtifacts" .= recreated, "repeatRemovedUnitCount" .= UnitCount repeatedRemoved, "qualificationAccepted" .= accepted]))
      unless accepted exitFailure
    _ -> ioError (userError "normal-build-measure baseline REPORT | qualify TOOL POLICY APPROVED_SHA256 REPORT")
