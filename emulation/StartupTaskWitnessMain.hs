{-# LANGUAGE OverloadedStrings #-}
module Main (main) where

import Control.Exception (IOException, try)
import Control.Monad (forM, unless)
import qualified Data.Aeson as J
import qualified Data.ByteString.Lazy as L
import Data.Either (isRight)
import Data.List.NonEmpty (NonEmpty(..))
import qualified Data.List.NonEmpty as NE
import qualified Emulation.Image4Staging as Image4
import Emulation.StartupTaskControls (refusalControls)
import Emulation.StartupTaskWitness
import Emulation.TaskSubmissionComparison (compareReceipts, comparisonValue)
import Emulation.TaskSubmissionControlWire (ProbeCase(..), probeCase)
import Emulation.TaskSubmissionProgram (NativeInputs(..))
import qualified FujiAF.Host as H
import qualified Replacement.TaskSubmission.Artifact as Artifact
import System.Directory (doesPathExist)
import System.Environment (getArgs)
import System.Exit (exitFailure)
import System.FilePath ((</>))

newtype InputFile = InputFile FilePath
newtype AnalysisRoot = AnalysisRoot FilePath
newtype ObservationDirectory = ObservationDirectory FilePath
newtype ScratchPath = ScratchPath FilePath
newtype OutputFile = OutputFile FilePath

data SourcePaths = SourcePaths InputFile InputFile InputFile AnalysisRoot InputFile
data NativePaths = NativePaths InputFile InputFile ScratchPath InputFile InputFile
data StockCommand = Qualify | RefusalControls
data OwnedMode = QualifyOwned | CompareOwned | CohortOwned
data OwnedCommand = OwnedQualify (NonEmpty ObservationDirectory)
  | Compare ObservationDirectory ObservationDirectory
  | Cohort ObservationDirectory ObservationDirectory
data Arguments = Historical StockCommand SourcePaths ObservationDirectory OutputFile
  | Owned Image4.Tools SourcePaths NativePaths OwnedCommand OutputFile

data ExpectedCase = AnyCase | NaturalCase | ControlledCase ProbeCase
data QualificationFailure = ObservationReadRejected String | ReceiptRejected WitnessError | CohortCaseRejected
  deriving (Show)
type Qualification = Either QualificationFailure QualifiedReceipt

usage :: String
usage = unlines
  [ "usage: startup-task-check [ENCODER REBUILD DECODER] qualify|refusal-controls DAT RUN ROM ANALYSIS_ROOT OBSERVER OBSERVATION_DIR OUT_JSON"
  , "       startup-task-check ENCODER REBUILD DECODER owned-qualify DAT RUN ROM ANALYSIS_ROOT OBSERVER LINKED_BIN SYMBOLS SCRATCH_DIR CANDIDATE_DAT CONTRACT OBSERVATION_DIR... OUT_JSON"
  , "       startup-task-check ENCODER REBUILD DECODER compare|cohort DAT RUN ROM ANALYSIS_ROOT OBSERVER LINKED_BIN SYMBOLS SCRATCH_DIR CANDIDATE_DAT CONTRACT RETAINED_DIR OWNED_DIR OUT_JSON"
  ]

arguments :: [String] -> IO Arguments
arguments raw = case raw of
  "qualify":paths -> historical Qualify paths
  "refusal-controls":paths -> historical RefusalControls paths
  encoder:rebuild:decoder:mode:paths -> case mode of
    "qualify" -> historical Qualify paths
    "refusal-controls" -> historical RefusalControls paths
    "owned-qualify" -> owned (Image4.Tools encoder rebuild decoder) QualifyOwned paths
    "compare" -> owned (Image4.Tools encoder rebuild decoder) CompareOwned paths
    "cohort" -> owned (Image4.Tools encoder rebuild decoder) CohortOwned paths
    _ -> H.failH usage
  _ -> H.failH usage
  where
    sourcePaths dat run rom analysis observer = SourcePaths (InputFile dat) (InputFile run) (InputFile rom)
      (AnalysisRoot analysis) (InputFile observer)
    historical mode [dat,run,rom,analysis,observer,observations,output] = pure
      (Historical mode (sourcePaths dat run rom analysis observer) (ObservationDirectory observations) (OutputFile output))
    historical _ _ = H.failH usage
    owned tools mode (dat:run:rom:analysis:observer:linked:symbols:scratch:candidate:contract:paths) = do
      (command,output) <- case (mode,reverse paths) of
        (QualifyOwned,output:lastDirectory:rest) -> pure
          (OwnedQualify (NE.reverse (ObservationDirectory lastDirectory :| map ObservationDirectory rest)),OutputFile output)
        (CompareOwned,[output,ownedDirectory,retainedDirectory]) -> pure
          (Compare (ObservationDirectory retainedDirectory) (ObservationDirectory ownedDirectory),OutputFile output)
        (CohortOwned,[output,ownedDirectory,retainedDirectory]) -> pure
          (Cohort (ObservationDirectory retainedDirectory) (ObservationDirectory ownedDirectory),OutputFile output)
        _ -> H.failH usage
      pure (Owned tools (sourcePaths dat run rom analysis observer)
        (NativePaths (InputFile linked) (InputFile symbols) (ScratchPath scratch) (InputFile candidate) (InputFile contract)) command output)
    owned _ _ _ = H.failH usage

sourceFiles :: SourcePaths -> IO SourceFiles
sourceFiles (SourcePaths (InputFile dat) (InputFile run) (InputFile rom) (AnalysisRoot analysis) (InputFile observer)) =
  SourceFiles <$> H.readRegular 0x4000000 dat <*> H.readRegular 0x100000 run <*> H.readRegular 0x10000 rom
    <*> H.readRegular 0x20000 (analysis </> "decode/loader-40000.bin") <*> H.readRegular 0x20000 (analysis </> "decode/loader-d0000.bin")
    <*> H.readRegular 0x400000 (analysis </> "lz/id4-0061c400.bin") <*> H.readRegular 0x10000000 observer

observedFiles :: ObservationDirectory -> IO ObservedFiles
observedFiles (ObservationDirectory directory) = ObservedFiles
  <$> H.readRegular 0x20000000 (directory </> "startup-tasks.json")
  <*> H.readRegular 0x2000000 (directory </> "memory-watch.json")
  <*> H.readRegular 0x10000000 (directory </> "reset.fujimod")

ownedInputs :: Image4.Tools -> SourceFiles -> NativePaths -> IO Inputs
ownedInputs tools sources (NativePaths (InputFile linked) (InputFile symbols) (ScratchPath scratchPath) (InputFile candidate) (InputFile contract)) = do
  native <- NativeInputs <$> H.readRegular 852 linked <*> H.readRegular 0x10000 symbols
    <*> H.readRegular 0x4000000 candidate <*> H.readRegular 0x100000 contract
  canonical <- either (H.failH . show) pure
    (Artifact.canonicalSource (sourceDat sources) (sourceImage sources) (sourceA sources) (sourceB sources))
  artifact <- either (H.failH . show) pure (Artifact.deriveArtifact (nativeFlat native) (nativeSymbols native))
  scratch <- Image4.validateScratchDirectory scratchPath
  staged <- Artifact.stageCandidate tools scratch (Artifact.stageImage4 canonical artifact)
  either (H.failH . show) pure (admitOwnedInputs sources native staged)

qualify :: Inputs -> ExpectedCase -> ObservationDirectory -> IO Qualification
qualify inputs expected directory = do
  loaded <- try (observedFiles directory) :: IO (Either IOException ObservedFiles)
  pure $ case loaded of
    Left problem -> Left (ObservationReadRejected (show problem))
    Right files -> case admitReceipt inputs files of
      Left problem -> Left (ReceiptRejected problem)
      Right checked -> if matches checked then Right checked else Left CohortCaseRejected
  where
    matches checked = case expected of
      AnyCase -> True
      NaturalCase -> case qualifiedProbeData checked of Nothing -> True; Just _ -> False
      ControlledCase selected -> fmap probeCase (qualifiedProbeData checked) == Just selected

qualificationReport :: ObservationDirectory -> Qualification -> J.Value
qualificationReport (ObservationDirectory directory) result = J.object
  (["observation_directory" J..= directory,"accepted" J..= isRight result] ++ case result of
    Left problem -> ["refusal" J..= show problem]
    Right checked -> ["qualification" J..= qualificationValue checked])

pairReport :: Inputs -> Inputs -> ExpectedCase -> ObservationDirectory -> ObservationDirectory -> IO (Bool,J.Value)
pairReport retainedInputs candidateInputs expected retainedDirectory ownedDirectory = do
  retained <- qualify retainedInputs expected retainedDirectory
  owned <- qualify candidateInputs expected ownedDirectory
  let compared = case (retained,owned) of
        (Right left,Right right) -> Just (compareReceipts left right)
        _ -> Nothing
      accepted = case compared of Just (Right _) -> True; _ -> False
      comparison = case compared of
        Nothing -> J.object ["accepted" J..= False,"refusal" J..= ("receipt-qualification-refused" :: String)]
        Just (Left problem) -> J.object ["accepted" J..= False,"refusal" J..= show problem]
        Just (Right proof) -> J.object ["accepted" J..= True,"comparison" J..= comparisonValue proof]
  pure (accepted,J.object ["accepted" J..= accepted,"retained" J..= qualificationReport retainedDirectory retained
    ,"owned" J..= qualificationReport ownedDirectory owned,"comparison" J..= comparison])

cohortCases :: [(FilePath,ExpectedCase)]
cohortCases = ("natural",NaturalCase) :
  [("fresh-enabled",ControlledCase FreshEnabled),("fresh-masked",ControlledCase FreshMasked)
  ,("reused-record",ControlledCase ReusedRecord),("reserved-refusal",ControlledCase ReservedRefusal)
  ,("partial-pair",ControlledCase PartialPair),("sentinel-fresh",ControlledCase SentinelFresh)
  ,("release-unlocked",ControlledCase ReleaseUnlocked),("release-peer-lock",ControlledCase ReleasePeerLock)
  ,("release-unlock-refusal",ControlledCase ReleaseUnlockRefusal),("callback-unreturned",ControlledCase CallbackUnreturned)]

runOwned :: Inputs -> Inputs -> OwnedCommand -> IO (Bool,J.Value)
runOwned retainedInputs candidateInputs command = case command of
  OwnedQualify directories -> do
    results <- forM (NE.toList directories) $ \directory -> do
      checked <- qualify candidateInputs AnyCase directory
      pure (isRight checked,qualificationReport directory checked)
    let accepted = all fst results
    pure (accepted,J.object ["schema" J..= (1 :: Int),"accepted" J..= accepted,"qualifications" J..= map snd results])
  Compare retained owned -> do
    (accepted,result) <- pairReport retainedInputs candidateInputs AnyCase retained owned
    pure (accepted,J.object ["schema" J..= (1 :: Int),"accepted" J..= accepted,"pair" J..= result])
  Cohort (ObservationDirectory retainedRoot) (ObservationDirectory ownedRoot) -> do
    results <- forM cohortCases $ \(name,expected) -> do
      (accepted,result) <- pairReport retainedInputs candidateInputs expected
        (ObservationDirectory (retainedRoot </> name)) (ObservationDirectory (ownedRoot </> name))
      pure (accepted,J.object ["case" J..= name,"result" J..= result])
    let accepted = all fst results
    pure (accepted,J.object ["schema" J..= (1 :: Int),"accepted" J..= accepted,"pairs" J..= map snd results])

main :: IO ()
main = do
  selected <- getArgs >>= arguments
  let OutputFile output = case selected of Historical _ _ _ path -> path; Owned _ _ _ _ path -> path
  exists <- doesPathExist output
  H.ensure (not exists) "checker output already exists"
  (accepted,result) <- case selected of
    Historical mode paths directory _ -> do
      sources <- sourceFiles paths
      observed <- observedFiles directory
      value <- case mode of
        Qualify -> do
          inputs <- either (H.failH . show) pure (admitInputs sources)
          checked <- either (H.failH . show) pure (admitReceipt inputs observed)
          pure (qualificationValue checked)
        RefusalControls -> either (H.failH . show) pure (refusalControls sources observed)
      pure (True,value)
    Owned tools paths native command _ -> do
      sources <- sourceFiles paths
      retainedInputs <- either (H.failH . show) pure (admitInputs sources)
      candidateInputs <- ownedInputs tools sources native
      runOwned retainedInputs candidateInputs command
  L.writeFile output (J.encode result)
  unless accepted exitFailure
