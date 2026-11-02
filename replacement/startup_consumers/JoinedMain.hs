{-# LANGUAGE OverloadedStrings #-}
module Main (main) where

import Control.Exception (SomeException, displayException, try)
import Control.Monad (forM, forM_, unless)
import qualified Data.Aeson as J
import Data.Bits ((.&.))
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C8
import qualified Data.ByteString.Lazy as L
import Data.List (sort)
import qualified Emulation.Image4Staging as Image4
import qualified Emulation.Sha256 as SHA
import qualified FujiAF.Host as H
import qualified Replacement.KernelInstall.Wire as K
import qualified Replacement.StartupComposition.Artifact as C
import qualified Replacement.StartupComposition.LoaderStartup as LS
import qualified Replacement.StartupConsumers.JoinedControls as Controls
import qualified Replacement.StartupConsumers.JoinedWire as Wire
import qualified Replacement.StartupConsumers.JoinedWitness as Witness
import qualified Replacement.StartupConsumers.Source as Source
import qualified Replacement.StartupConsumers.Witness as Consumer
import System.Directory (createDirectory)
import System.Environment (getArgs)
import System.Exit (ExitCode(..))
import System.FilePath ((</>))

newtype StageDirectory = StageDirectory FilePath
newtype ObservationDirectory = ObservationDirectory FilePath
newtype OutputDirectory = OutputDirectory FilePath
data PairMode = CompareEffects | RefusalControls
data Mode = Qualify ObservationDirectory | Pair PairMode ObservationDirectory ObservationDirectory

data HostScope = SharedRouteAndDispatcher deriving (Eq, Show)
instance J.FromJSON HostScope where
  parseJSON = J.withText "host admission scope" $ \value -> if value=="shared-route-and-dispatcher" then pure SharedRouteAndDispatcher else fail "unknown host admission scope"
data HostVerification = HostVerification Int HostScope Bool (Maybe String) K.Digest Wire.CaptureRole Wire.FileBinding Wire.FileBinding
instance J.FromJSON HostVerification where
  parseJSON = J.withObject "host route admission" $ \o -> HostVerification <$> o J..: "schema" <*> o J..: "scope" <*> o J..: "accepted"
    <*> o J..: "error" <*> o J..: "receipt" <*> o J..: "role" <*> o J..: "route_effects" <*> o J..: "dispatcher_effects"
data HostComparison = HostComparison Int HostScope Bool (Maybe String) K.Digest K.Digest [Wire.FileBinding]
instance J.FromJSON HostComparison where
  parseJSON = J.withObject "host route comparison" $ \o -> do
    effects <- sequence [o J..: "retained_route_effects",o J..: "joined_route_effects",o J..: "retained_dispatcher_effects",o J..: "joined_dispatcher_effects"]
    HostComparison <$> o J..: "schema" <*> o J..: "scope" <*> o J..: "accepted" <*> o J..: "error" <*> o J..: "retained" <*> o J..: "joined" <*> pure effects

newtype HostBaseline = HostBaseline K.Digest
instance J.FromJSON HostBaseline where
  parseJSON = J.withObject "host control baseline" $ \o -> do
    name <- o J..: "path"
    if (name::String)=="baseline-pair/shared-comparison.json" then HostBaseline <$> o J..: "sha256" else fail "unexpected control baseline"
data NestedPush = NestedPush K.Address K.Count K.Count K.Address K.Address K.Address K.Count
instance J.FromJSON NestedPush where
  parseJSON = J.withObject "retained callee push" $ \o -> NestedPush <$> o J..: "call_sp" <*> o J..: "call_start" <*> o J..: "call_end"
    <*> o J..: "address" <*> o J..: "physical" <*> o J..: "pc" <*> o J..: "instructions"
data HostControlName = PhaseArgument | LoaderLink | CallbackResult | DescriptorStore | ForeignLiveWord | SourceAlias
  | SourceTcm | MissingCarrierFetch | MenuPulse | ResetBytes | TimerRead | MissingGuard deriving (Eq, Ord, Show, Enum, Bounded)
instance J.FromJSON HostControlName where
  parseJSON = J.withText "host control name" $ \value -> maybe (fail "unknown host control") pure (lookup value
    [("phase-argument",PhaseArgument),("loader-link",LoaderLink),("callback-result",CallbackResult),("descriptor-store",DescriptorStore)
    ,("foreign-live-word",ForeignLiveWord),("source-alias",SourceAlias),("source-tcm",SourceTcm),("missing-carrier-fetch",MissingCarrierFetch)
    ,("menu-pulse",MenuPulse),("reset-bytes",ResetBytes),("timer-read",TimerRead),("missing-guard",MissingGuard)])
data HostCaseOutcome = CaseRefused | CaseAdmitted | CaseSetupFailure
instance J.FromJSON HostCaseOutcome where
  parseJSON = J.withText "host control outcome" $ \value -> case value of
    "refused" -> pure CaseRefused; "admitted" -> pure CaseAdmitted; "setup-failure" -> pure CaseSetupFailure; _ -> fail "unknown control outcome"
data HostCaseObservation = HostCaseObservation HostCaseOutcome (Maybe String)
instance J.FromJSON HostCaseObservation where parseJSON = J.withObject "host control observation" $ \o -> HostCaseObservation <$> o J..: "outcome" <*> o J..:? "error"
data HostControlCase = HostControlCase HostControlName Bool HostCaseObservation
instance J.FromJSON HostControlCase where parseJSON = J.withObject "host control case" $ \o -> HostControlCase <$> o J..: "name" <*> o J..: "passed" <*> o J..: "observed"
data HostControls = HostControls Int HostScope Bool (Maybe String) HostBaseline NestedPush [HostControlCase]
instance J.FromJSON HostControls where
  parseJSON = J.withObject "host refusal controls" $ \o -> HostControls <$> o J..: "schema" <*> o J..: "scope" <*> o J..: "accepted"
    <*> o J..: "error" <*> o J..: "baseline" <*> o J..: "nested_push" <*> o J..: "cases"

hash :: B.ByteString -> K.Digest
hash = K.Digest . SHA.sha256Hex . SHA.sha256
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

checkEffectFiles :: FilePath -> [Wire.FileBinding] -> IO ()
checkEffectFiles root bindings = forM_ bindings $ \binding -> do
  bytes <- H.readRegular 0x40000000 (root </> Wire.relativeName (Wire.bindingPath binding))
  H.ensure (hash bytes == Wire.bindingDigest binding) "host effect binding differs"

checkHostComparison :: FilePath -> B.ByteString -> K.Digest -> K.Digest -> IO ()
checkHostComparison root bytes expectedRetained expectedJoined = do
  HostComparison schema scope accepted failure retainedDigest joinedDigest effects <- either H.failH pure (J.eitherDecodeStrict' bytes)
  unless (schema==1 && scope==SharedRouteAndDispatcher && accepted && failure==Nothing
    && retainedDigest==expectedRetained && joinedDigest==expectedJoined) (H.failH "host comparison does not bind these captures")
  checkEffectFiles root effects

checkHostControls :: FilePath -> K.Digest -> K.Digest -> IO ()
checkHostControls root retained joined = do
  bytes <- H.readRegular 0x100000 (root </> "shared-refusal-controls.json")
  HostControls schema scope accepted failure (HostBaseline baselineDigest)
    (NestedPush (K.Address callSp) callStart callEnd (K.Address guest) (K.Address physical) pc instructions) cases <-
      either H.failH pure (J.eitherDecodeStrict' bytes)
  let refused (HostControlCase _ True (HostCaseObservation CaseRefused (Just message))) = not (null message)
      refused _ = False
  H.ensure (schema==1 && scope==SharedRouteAndDispatcher && accepted && failure==Nothing
    && sort [name | HostControlCase name _ _ <- cases] == [minBound..maxBound] && all refused cases
    && callStart < instructions && instructions <= callEnd && pc/=K.Address 0
    && physical == guest .&. 0xbfffffff && physical < callSp .&. 0xbfffffff) "host refusal cohort is incomplete"
  let baselineRoot = root </> "baseline-pair"
  baseline <- H.readRegular 0x100000 (baselineRoot </> "shared-comparison.json")
  H.ensure (hash baseline == baselineDigest) "control baseline binding differs"
  checkHostComparison baselineRoot baseline retained joined

runHost :: FilePath -> FilePath -> [String] -> IO ()
runHost observer root arguments = do
  (limited,status,out,err) <- H.runBounded 1800000000 (16*1024*1024) observer arguments B.empty
  B.writeFile (root </> "host-stdout.log") out
  B.writeFile (root </> "host-stderr.log") err
  writeJson (root </> "host-invocation.json") (J.object ["arguments" J..= arguments,"limited" J..= limited,"exit" J..= case status of ExitSuccess -> (0::Int); ExitFailure code -> code])
  H.ensure (not limited && status==ExitSuccess) ("route/dispatcher host refusal: "++C8.unpack (B.take 2048 (out<>err)))

main :: IO ()
main = do
  arguments <- getArgs
  case arguments of
    encoder:rebuild:decoder:modeText:stagePath:runPath:romPath:observerPath:analysis:scratchPath:outputPath:observations -> do
      mode <- case (modeText,observations) of
        ("qualify",[directory]) -> pure (Qualify (ObservationDirectory directory))
        ("compare",[retained,joined]) -> pure (Pair CompareEffects (ObservationDirectory retained) (ObservationDirectory joined))
        ("refusal-controls",[retained,joined]) -> pure (Pair RefusalControls (ObservationDirectory retained) (ObservationDirectory joined))
        _ -> H.failH usage
      let output = OutputDirectory outputPath
      createDirectory outputPath
      result <- try (execute (Image4.Tools encoder rebuild decoder) (StageDirectory stagePath) runPath romPath observerPath analysis scratchPath output mode)
      case result of
        Right value -> writeJson (outputPath </> "qualification.json") value
        Left exception -> do
          writeJson (outputPath </> "qualification.json") (J.object ["schema" J..= (1::Int),"accepted" J..= False,"error" J..= displayException (exception::SomeException)])
          H.failH (displayException exception)
    _ -> H.failH usage

execute :: Image4.Tools -> StageDirectory -> FilePath -> FilePath -> FilePath -> FilePath -> FilePath -> OutputDirectory -> Mode -> IO J.Value
execute tools (StageDirectory stage) runPath romPath observerPath analysis scratchPath (OutputDirectory output) mode = do
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
  let hostOutput = output </> "route-dispatcher"
  case mode of
    Qualify directory@(ObservationDirectory path) -> do
      files <- readObserved directory
      qualified <- admitted (Witness.admitCapture inputs files)
      runHost observerPath output ["--shared-check",path,"--analysis",analysis,"--output",hostOutput]
      host <- H.readRegular 0x100000 (hostOutput </> "shared-verification.json") >>= either H.failH pure . J.eitherDecodeStrict'
      let HostVerification schema scope accepted failure receipt role routeEffects dispatcherEffects = host
      envelope <- either H.failH pure (J.eitherDecodeStrict' (Witness.observedCapture files))
      H.ensure (schema==1 && scope==SharedRouteAndDispatcher && accepted && failure==Nothing
        && receipt==hash (Witness.observedCapture files) && role==Wire.captureRole envelope) "host admission does not bind this capture"
      checkEffectFiles hostOutput [routeEffects,dispatcherEffects]
      pure (J.object ["schema" J..= (1::Int),"accepted" J..= True,"scope" J..= ("bounded-three-component-startup-composition"::String),"consumer" J..= Witness.qualificationValue qualified])
    Pair pairMode retained@(ObservationDirectory retainedPath) joined@(ObservationDirectory joinedPath) -> do
      retainedFiles <- readObserved retained
      joinedFiles <- readObserved joined
      qualifiedRetained <- admitted (Witness.admitCapture inputs retainedFiles)
      qualifiedJoined <- admitted (Witness.admitCapture inputs joinedFiles)
      admitted (Witness.compareCaptures qualifiedRetained qualifiedJoined)
      checks <- case pairMode of
        CompareEffects -> do
          runHost observerPath output ["--shared-compare-retained",retainedPath,"--shared-compare-joined",joinedPath,"--analysis",analysis,"--output",hostOutput]
          host <- H.readRegular 0x100000 (hostOutput </> "shared-comparison.json")
          checkHostComparison hostOutput host (hash (Witness.observedCapture retainedFiles)) (hash (Witness.observedCapture joinedFiles))
          pure J.Null
        RefusalControls -> do
          consumerControls <- either H.failH pure (Controls.checkRefusals inputs retainedFiles joinedFiles)
          runHost observerPath output ["--shared-refusal-controls",retainedPath,"--shared-refusal-joined",joinedPath,"--analysis",analysis,"--output",hostOutput]
          checkHostControls hostOutput (hash (Witness.observedCapture retainedFiles)) (hash (Witness.observedCapture joinedFiles))
          pure consumerControls
      pure (J.object ["schema" J..= (1::Int),"accepted" J..= True,"comparison" J..= ("retained-three-component-joined-agree"::String)
        ,"roles" J..= map Witness.qualificationValue [qualifiedRetained,qualifiedJoined],"post_return_scheduler_equality" J..= False,"controls" J..= checks])

usage :: String
usage = "usage: startup-joined-check ENCODER REBUILD DECODER qualify|compare|refusal-controls STAGED_DIR RUN_JSON BOOT_ROM OBSERVER ANALYSIS_ROOT SCRATCH_DIR NEW_OUTPUT_DIR [CAPTURE_DIR|RETAINED_DIR JOINED_DIR]"
