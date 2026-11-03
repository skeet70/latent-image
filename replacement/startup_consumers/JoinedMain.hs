{-# LANGUAGE OverloadedStrings #-}
module Main (main) where

import Control.Exception (SomeException, displayException, try)
import Control.Monad (forM)
import qualified Data.Aeson as J
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as L
import qualified Emulation.Image4Staging as Image4
import qualified FujiAF.Host as H
import qualified Replacement.StartupComposition.Artifact as C
import qualified Replacement.StartupComposition.LoaderStartup as LS
import qualified Replacement.StartupConsumers.JoinedControls as Controls
import qualified Replacement.StartupConsumers.JoinedHost as Host
import qualified Replacement.StartupConsumers.JoinedWire as Wire
import qualified Replacement.StartupConsumers.JoinedWitness as Witness
import qualified Replacement.StartupConsumers.Source as Source
import qualified Replacement.StartupConsumers.Witness as Consumer
import System.Directory (createDirectory)
import System.Environment (getArgs)
import System.FilePath ((</>))

newtype StageDirectory = StageDirectory FilePath
newtype ObservationDirectory = ObservationDirectory FilePath
newtype OutputDirectory = OutputDirectory FilePath
data PairMode = CompareEffects | RefusalControls
data Mode = Qualify ObservationDirectory | Pair PairMode ObservationDirectory ObservationDirectory

admitted :: Show failure => Either failure value -> IO value
admitted = either (H.failH . show) pure
writeJson :: FilePath -> J.Value -> IO ()
writeJson path = B.writeFile path . L.toStrict . J.encode

readObserved :: ObservationDirectory -> IO Witness.ObservedFiles
readObserved (ObservationDirectory root) = do
  capture <- H.readRegular 0x40000000 (root </> "startup-joined.json")
  envelope <- either H.failH pure (J.eitherDecodeStrict' capture)
  let readNamed cap name = H.readRegular cap (root </> Wire.relativeName name)
  artifacts <- forM [minBound..maxBound] $ \role -> Witness.CapturedArtifact role <$> readNamed 0x4000000 (Wire.artifactFilename role)
  watches <- forM [binding | epoch <- Wire.captureEpochs envelope,Wire.WatchFile binding _ <- [Wire.epochEvidence epoch]] $ \binding ->
    Witness.CapturedWatch (Wire.bindingPath binding) <$> readNamed 0x40000000 (Wire.bindingPath binding)
  Witness.ObservedFiles capture <$> readNamed 0x10000000 Wire.resetFilename <*> readNamed 0x40000000 Wire.traceFilename
    <*> readNamed 0x40000000 Wire.timerFilename <*> readNamed 0x4000000 Wire.updateFilename <*> pure artifacts <*> pure watches

-- Each role's policy is issued by the host check of the same capture bytes read here.
rolePolicy :: Host.HostAdmission -> Witness.ObservedFiles -> IO Host.HostPolicyBytes
rolePolicy host files = do
  envelope <- either H.failH pure (J.eitherDecodeStrict' (Witness.observedCapture files))
  admitted (Host.hostPolicyFor host (Wire.captureRole envelope))

main :: IO ()
main = do
  arguments <- getArgs
  case arguments of
    encoder:rebuild:decoder:modeText:stagePath:runPath:romPath:observerPath:checkerPath:analysis:scratchPath:outputPath:observations -> do
      mode <- case (modeText,observations) of
        ("qualify",[directory]) -> pure (Qualify (ObservationDirectory directory))
        ("compare",[retained,joined]) -> pure (Pair CompareEffects (ObservationDirectory retained) (ObservationDirectory joined))
        ("refusal-controls",[retained,joined]) -> pure (Pair RefusalControls (ObservationDirectory retained) (ObservationDirectory joined))
        _ -> H.failH usage
      let output = OutputDirectory outputPath
      createDirectory outputPath
      result <- try (execute (Image4.Tools encoder rebuild decoder) (StageDirectory stagePath) runPath romPath observerPath checkerPath analysis scratchPath output mode)
      case result of
        Right value -> writeJson (outputPath </> "qualification.json") value
        Left exception -> do
          writeJson (outputPath </> "qualification.json") (J.object ["schema" J..= (1::Int),"accepted" J..= False,"error" J..= displayException (exception::SomeException)])
          H.failH (displayException exception)
    _ -> H.failH usage

execute :: Image4.Tools -> StageDirectory -> FilePath -> FilePath -> FilePath -> FilePath -> FilePath -> FilePath -> OutputDirectory -> Mode -> IO J.Value
execute tools (StageDirectory stage) runPath romPath observerPath checkerPath analysis scratchPath (OutputDirectory output) mode = do
  let staged name = H.readRegular 0x4000000 (stage </> name)
      native image symbols = C.NativeFiles <$> staged image <*> staged symbols
  sources <- Source.SourceFiles <$> staged "canonical.dat" <*> staged "canonical-image4.bin" <*> staged "canonical-loader-a.bin" <*> staged "canonical-loader-b.bin"
  natives <- Witness.NativeInputs <$> native "loader-a.bin" "loader-a.symbols" <*> native "loader-b.bin" "loader-b.symbols"
    <*> native "dispatcher.bin" "dispatcher.symbols" <*> native "installer.bin" "installer.symbols"
  producer <- Consumer.LoaderProducerFiles <$> H.readRegular 0x10000000 observerPath <*> H.readRegular 0x100000 runPath
    <*> H.readRegular 0x1000000 romPath <*> staged "candidate.dat" <*> staged "contract.json"
  source <- admitted (LS.canonicalSource (C.SourceFiles (Source.sourceDat sources) (Source.sourceImage4 sources) (Source.sourceLoaderA sources) (Source.sourceLoaderB sources)))
  artifact <- admitted (LS.deriveArtifact (Witness.nativeLoaderA natives) (Witness.nativeLoaderB natives) (Witness.nativeDispatcher natives) (Witness.nativeInstaller natives))
  scratch <- Image4.validateScratchDirectory scratchPath
  candidate <- LS.stageCandidate tools scratch source artifact
  inputs <- admitted (Witness.admitInputs sources natives producer candidate)
  let host = Host.HostEnvironment (Host.CheckerExecutable checkerPath) (Host.ProducerExecutable observerPath) analysis (output </> "host")
  case mode of
    Qualify directory@(ObservationDirectory path) -> do
      files <- readObserved directory
      admission <- Host.runHostAdmission host (Host.Qualify path)
      policy <- rolePolicy admission files
      qualified <- admitted (Witness.admitCapture inputs policy files)
      pure (J.object ["schema" J..= (1::Int),"accepted" J..= True,"scope" J..= ("bounded-three-component-startup-composition"::String)
        ,"consumer" J..= Witness.qualificationValue qualified,"route_dispatcher" J..= Host.hostSummary admission])
    Pair pairMode retained@(ObservationDirectory retainedPath) joined@(ObservationDirectory joinedPath) -> do
      retainedFiles <- readObserved retained
      joinedFiles <- readObserved joined
      admission <- Host.runHostAdmission host (case pairMode of
        CompareEffects -> Host.Compare retainedPath joinedPath
        RefusalControls -> Host.RefusalControls retainedPath joinedPath)
      retainedPolicy <- rolePolicy admission retainedFiles
      joinedPolicy <- rolePolicy admission joinedFiles
      qualifiedRetained <- admitted (Witness.admitCapture inputs retainedPolicy retainedFiles)
      qualifiedJoined <- admitted (Witness.admitCapture inputs joinedPolicy joinedFiles)
      admitted (Witness.compareCaptures qualifiedRetained qualifiedJoined)
      checks <- case pairMode of
        CompareEffects -> pure J.Null
        RefusalControls -> either H.failH pure (Controls.checkRefusals inputs retainedPolicy joinedPolicy retainedFiles joinedFiles)
      pure (J.object ["schema" J..= (1::Int),"accepted" J..= True,"comparison" J..= ("retained-three-component-joined-agree"::String)
        ,"roles" J..= map Witness.qualificationValue [qualifiedRetained,qualifiedJoined],"post_return_scheduler_equality" J..= False
        ,"route_dispatcher" J..= Host.hostSummary admission,"controls" J..= checks])

usage :: String
usage = "usage: startup-joined-check ENCODER REBUILD DECODER qualify|compare|refusal-controls STAGED_DIR RUN_JSON BOOT_ROM OBSERVER HOST_CHECKER ANALYSIS_ROOT SCRATCH_DIR NEW_OUTPUT_DIR [CAPTURE_DIR|RETAINED_DIR JOINED_DIR]"
