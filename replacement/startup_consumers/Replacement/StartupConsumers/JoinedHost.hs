{-# LANGUAGE OverloadedStrings #-}
module Replacement.StartupConsumers.JoinedHost
  ( HostAdmission, HostPolicyBytes, HostError
  , HostRequest(..), HostEnvironment(..), runHostAdmission
  , hostPolicyFor, policyBytes, policyCaptureDigest, policyObserverDigest, hostSummary
  ) where

import Control.Exception (Exception)
import Control.Monad (forM, forM_)
import qualified Data.Aeson as J
import Data.Bits ((.&.))
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C8
import qualified Data.ByteString.Lazy as L
import Data.List (sort)
import qualified Data.Text as T
import qualified Emulation.Sha256 as SHA
import qualified FujiAF.Host as H
import qualified Replacement.KernelInstall.Wire as K
import qualified Replacement.StartupConsumers.JoinedWire as Wire
import System.Directory (createDirectory, getPermissions, setOwnerExecutable, setPermissions)
import System.Exit (ExitCode(..))
import System.FilePath ((</>))

-- Fields: host checker executable, capture producer executable, analysis root, output root. The output root must not exist when admission starts.
data HostEnvironment = HostEnvironment FilePath FilePath FilePath FilePath
data HostRequest = Qualify FilePath | Compare FilePath FilePath | RefusalControls FilePath FilePath

data HostError = PolicyUnavailable Wire.CaptureRole deriving (Show)
instance Exception HostError

data HostPolicyBytes = HostPolicyBytes B.ByteString K.Digest K.Digest K.Digest K.Digest
data HostAdmission = HostAdmission [(Wire.CaptureRole,HostPolicyBytes)] J.Value

policyBytes :: HostPolicyBytes -> B.ByteString
policyBytes (HostPolicyBytes bytes _ _ _ _) = bytes
policyCaptureDigest :: HostPolicyBytes -> K.Digest
policyCaptureDigest (HostPolicyBytes _ capture _ _ _) = capture
policyObserverDigest :: HostPolicyBytes -> K.Digest
policyObserverDigest (HostPolicyBytes _ _ observer _ _) = observer
hostPolicyFor :: HostAdmission -> Wire.CaptureRole -> Either HostError HostPolicyBytes
hostPolicyFor (HostAdmission policies _) role = maybe (Left (PolicyUnavailable role)) Right (lookup role policies)
hostSummary :: HostAdmission -> J.Value
hostSummary (HostAdmission _ summary) = summary

data HostScope = SharedRouteAndDispatcher deriving (Eq)
instance J.FromJSON HostScope where
  parseJSON = J.withText "host admission scope" $ \value ->
    if value=="shared-route-and-dispatcher" then pure SharedRouteAndDispatcher else fail "unknown host admission scope"
data HostVerification = HostVerification Int HostScope Bool (Maybe String) K.Digest Wire.CaptureRole Wire.FileBinding Wire.FileBinding Wire.FileBinding
instance J.FromJSON HostVerification where
  parseJSON = J.withObject "host route admission" $ \o -> HostVerification <$> o J..: "schema" <*> o J..: "scope" <*> o J..: "accepted"
    <*> o J..: "error" <*> o J..: "receipt" <*> o J..: "role" <*> o J..: "route_effects" <*> o J..: "dispatcher_effects" <*> o J..: "observation_policy"
data HostComparison = HostComparison Int HostScope Bool (Maybe String) K.Digest K.Digest Wire.FileBinding Wire.FileBinding Wire.FileBinding Wire.FileBinding Wire.FileBinding Wire.FileBinding
instance J.FromJSON HostComparison where
  parseJSON = J.withObject "host route comparison" $ \o -> HostComparison <$> o J..: "schema" <*> o J..: "scope" <*> o J..: "accepted"
    <*> o J..: "error" <*> o J..: "retained" <*> o J..: "joined" <*> o J..: "retained_route_effects" <*> o J..: "joined_route_effects"
    <*> o J..: "retained_dispatcher_effects" <*> o J..: "joined_dispatcher_effects"
    <*> o J..: "retained_observation_policy" <*> o J..: "joined_observation_policy"

newtype HostBaseline = HostBaseline K.Digest
instance J.FromJSON HostBaseline where
  parseJSON = J.withObject "host control baseline" $ \o -> do
    name <- o J..: "path"
    if (name::String)=="baseline-pair/shared-comparison.json" then HostBaseline <$> o J..: "sha256" else fail "unexpected control baseline"
data NestedPush = NestedPush K.Core K.Address K.Count K.Count K.Address K.Address K.Address K.Count
instance J.FromJSON NestedPush where
  parseJSON = J.withObject "retained callee push" $ \o -> NestedPush <$> o J..: "core" <*> o J..: "call_sp" <*> o J..: "call_start" <*> o J..: "call_end"
    <*> o J..: "address" <*> o J..: "physical" <*> o J..: "pc" <*> o J..: "instructions"
data HostControlName = PhaseArgument | LoaderLink | CallbackResult | DescriptorStore | ForeignLiveWord | SourceAlias
  | SourceTcm | MissingCarrierFetch | MenuPulse | ResetBytes | TimerRead | MissingGuard | ModeCell | ModeBoundary
  | ModeRead | ModeAttribution | RepeatedZeroSegment | StopOwnCount | MissingAddedGuard | DispatcherOneFetch | DispatcherAttemptOnly
  | PolicyMissingAcquisition | PolicyWrongAcquisition | PolicyPrematureSharedRelease | PolicyOmittedNoop | PolicyTraceOccurrence | PolicySegmentOwners
  deriving (Eq, Ord, Enum, Bounded)
controlNames :: [(T.Text,HostControlName)]
controlNames =
  [("phase-argument",PhaseArgument),("loader-link",LoaderLink),("callback-result",CallbackResult),("descriptor-store",DescriptorStore)
  ,("foreign-live-word",ForeignLiveWord),("source-alias",SourceAlias),("source-tcm",SourceTcm),("missing-carrier-fetch",MissingCarrierFetch)
  ,("menu-pulse",MenuPulse),("reset-bytes",ResetBytes),("timer-read",TimerRead),("missing-guard",MissingGuard)
  ,("mode-cell",ModeCell),("mode-boundary",ModeBoundary),("mode-read",ModeRead),("mode-attribution",ModeAttribution)
  ,("repeated-zero-segment",RepeatedZeroSegment),("stop-own-count",StopOwnCount),("missing-added-guard",MissingAddedGuard)
  ,("dispatcher-one-fetch",DispatcherOneFetch),("dispatcher-attempt-only",DispatcherAttemptOnly)
  ,("policy-missing-acquisition",PolicyMissingAcquisition),("policy-wrong-acquisition",PolicyWrongAcquisition)
  ,("policy-premature-shared-release",PolicyPrematureSharedRelease),("policy-omitted-noop",PolicyOmittedNoop)
  ,("policy-trace-occurrence",PolicyTraceOccurrence),("policy-segment-owners",PolicySegmentOwners)]
instance J.FromJSON HostControlName where
  parseJSON = J.withText "host control name" $ \value -> maybe (fail "unknown host control") pure (lookup value controlNames)
data HostCaseOutcome = CaseRefused | CaseAdmitted | CaseSetupFailure
instance J.FromJSON HostCaseOutcome where
  parseJSON = J.withText "host control outcome" $ \value -> case value of
    "refused" -> pure CaseRefused
    "admitted" -> pure CaseAdmitted
    "setup-failure" -> pure CaseSetupFailure
    _ -> fail "unknown control outcome"
data HostCaseObservation = HostCaseObservation HostCaseOutcome (Maybe String)
instance J.FromJSON HostCaseObservation where
  parseJSON = J.withObject "host control observation" $ \o -> HostCaseObservation <$> o J..: "outcome" <*> o J..:? "error"
data HostControlCase = HostControlCase HostControlName Bool Wire.RelativeFilename K.Digest HostCaseObservation
instance J.FromJSON HostControlCase where
  parseJSON = J.withObject "host control case" $ \o -> HostControlCase <$> o J..: "name" <*> o J..: "passed" <*> o J..: "directory"
    <*> o J..: "receipt" <*> o J..: "observed"
data HostControls = HostControls Int HostScope Bool (Maybe String) HostBaseline NestedPush [HostControlCase]
instance J.FromJSON HostControls where
  parseJSON = J.withObject "host refusal controls" $ \o -> HostControls <$> o J..: "schema" <*> o J..: "scope" <*> o J..: "accepted"
    <*> o J..: "error" <*> o J..: "baseline" <*> o J..: "nested_push" <*> o J..: "cases"

data PolicyScope = SharedExternalObservationPolicy deriving (Eq)
instance J.FromJSON PolicyScope where
  parseJSON = J.withText "host policy scope" $ \value ->
    if value=="shared-external-observation-policy" then pure SharedExternalObservationPolicy else fail "unknown host policy scope"
data PolicyBinding = PolicyBinding Int PolicyScope K.Digest K.Digest K.Digest
instance J.FromJSON PolicyBinding where
  parseJSON = J.withObject "host policy binding" $ \o -> PolicyBinding <$> o J..: "schema" <*> o J..: "scope"
    <*> o J..: "capture_sha256" <*> o J..: "trace_sha256" <*> o J..: "contract_sha256"

data CaptureInput = CaptureInput FilePath Wire.SharedCapture K.Digest
captureEnvelope :: CaptureInput -> Wire.SharedCapture
captureEnvelope (CaptureInput _ capture _) = capture
captureDigest :: CaptureInput -> K.Digest
captureDigest (CaptureInput _ _ digest) = digest

hash :: B.ByteString -> K.Digest
hash = K.Digest . SHA.sha256Hex . SHA.sha256
readJson :: J.FromJSON a => B.ByteString -> IO a
readJson = either H.failH pure . J.eitherDecodeStrict'
writeJson :: FilePath -> J.Value -> IO ()
writeJson path = B.writeFile path . L.toStrict . J.encode
readBound :: Int -> FilePath -> Wire.FileBinding -> IO B.ByteString
readBound cap root binding = do
  bytes <- H.readRegular cap (root </> Wire.relativeName (Wire.bindingPath binding))
  H.ensure (hash bytes==Wire.bindingDigest binding) "host input/output file binding differs"
  pure bytes
readNamedBound :: FilePath -> FilePath -> Wire.FileBinding -> IO B.ByteString
readNamedBound root name binding = do
  H.ensure (Wire.relativeName (Wire.bindingPath binding)==name) "host output filename differs"
  readBound 0x40000000 root binding

readCapture :: K.Digest -> FilePath -> IO CaptureInput
readCapture observer root = do
  bytes <- H.readRegular 0x40000000 (root </> "startup-joined.json")
  capture <- readJson bytes
  H.ensure (Wire.identityExecutable (Wire.captureIdentity capture)==observer) "capture producer differs from the admitted observer"
  _ <- readNamedBound root "trace.json" (Wire.captureTrace capture)
  _ <- readNamedBound root "timer-transactions.json" (Wire.captureTimers capture)
  _ <- readNamedBound root "input-update.DAT" (Wire.identityUpdateFile (Wire.captureIdentity capture))
  checkpoint <- H.readRegular 0x10000000 (root </> "reset.fujimod")
  H.ensure (hash checkpoint==Wire.captureCheckpoint capture) "host checkpoint binding differs"
  let artifacts = Wire.captureArtifacts capture
      expected = sort [Wire.relativeName (Wire.artifactFilename role) | role <- [minBound..maxBound]]
  H.ensure (sort (map (Wire.relativeName . Wire.bindingPath) artifacts)==expected) "host source artifact set differs"
  forM_ artifacts $ \binding -> do
    _ <- readBound 0x4000000 root binding
    pure ()
  forM_ [binding | epoch <- Wire.captureEpochs capture,Wire.WatchFile binding _ <- [Wire.epochEvidence epoch]] $ \binding -> do
    _ <- readBound 0x40000000 root binding
    pure ()
  pure (CaptureInput root capture (hash bytes))

checkCaptureUnchanged :: CaptureInput -> IO ()
checkCaptureUnchanged (CaptureInput root _ digest) = do
  bytes <- H.readRegular 0x40000000 (root </> "startup-joined.json")
  H.ensure (hash bytes==digest) "host input capture changed during admission"

checkPolicy :: K.Digest -> K.Digest -> FilePath -> FilePath -> CaptureInput -> Wire.FileBinding -> IO HostPolicyBytes
checkPolicy observer resultDigest root name input binding = do
  bytes <- readNamedBound root name binding
  PolicyBinding schema scope capture trace contract <- readJson bytes
  let envelope = captureEnvelope input
      contracts = [Wire.bindingDigest file | file <- Wire.captureArtifacts envelope,Wire.bindingPath file==Wire.artifactFilename Wire.JoinedContract]
  H.ensure (schema==1 && scope==SharedExternalObservationPolicy && capture==captureDigest input
    && trace==Wire.bindingDigest (Wire.captureTrace envelope) && contracts==[contract]) "host observation policy does not bind these inputs"
  pure (HostPolicyBytes bytes capture observer resultDigest (Wire.bindingDigest binding))

checkSingle :: K.Digest -> FilePath -> CaptureInput -> IO HostAdmission
checkSingle observer root input = do
  bytes <- H.readRegular 0x100000 (root </> "shared-verification.json")
  HostVerification schema scope accepted failure receipt role route dispatcher policy <- readJson bytes
  H.ensure (schema==1 && scope==SharedRouteAndDispatcher && accepted && failure==Nothing
    && receipt==captureDigest input && role==Wire.captureRole (captureEnvelope input)) "host admission does not bind this capture"
  _ <- readNamedBound root "route-effects.json" route
  _ <- readNamedBound root "dispatcher-effects.json" dispatcher
  capability <- checkPolicy observer (hash bytes) root "observation-policy.json" input policy
  summary <- readJson bytes
  pure (HostAdmission [(role,capability)] summary)

checkComparison :: K.Digest -> FilePath -> B.ByteString -> CaptureInput -> CaptureInput -> IO HostAdmission
checkComparison observer root bytes retained joined = do
  HostComparison schema scope accepted failure retainedDigest joinedDigest retainedRoute joinedRoute retainedDispatcher joinedDispatcher retainedPolicy joinedPolicy <- readJson bytes
  H.ensure (schema==1 && scope==SharedRouteAndDispatcher && accepted && failure==Nothing
    && retainedDigest==captureDigest retained && joinedDigest==captureDigest joined) "host comparison does not bind these captures"
  leftRoute <- readNamedBound root "retained-route-effects.json" retainedRoute
  rightRoute <- readNamedBound root "joined-route-effects.json" joinedRoute
  leftDispatcher <- readNamedBound root "retained-dispatcher-effects.json" retainedDispatcher
  rightDispatcher <- readNamedBound root "joined-dispatcher-effects.json" joinedDispatcher
  H.ensure (leftRoute==rightRoute && leftDispatcher==rightDispatcher) "host route or dispatcher public effects differ"
  left <- checkPolicy observer (hash bytes) root "retained-observation-policy.json" retained retainedPolicy
  right <- checkPolicy observer (hash bytes) root "joined-observation-policy.json" joined joinedPolicy
  summary <- readJson bytes
  pure (HostAdmission [(Wire.RetainedCapture,left),(Wire.JoinedCapture,right)] summary)

checkControls :: K.Digest -> FilePath -> CaptureInput -> CaptureInput -> IO HostAdmission
checkControls observer root retained joined = do
  bytes <- H.readRegular 0x100000 (root </> "shared-refusal-controls.json")
  HostControls schema scope accepted failure (HostBaseline baselineDigest)
    (NestedPush _ (K.Address callSp) callStart callEnd (K.Address guest) (K.Address physical) pc instructions) cases <- readJson bytes
  let refused (HostControlCase _ True _ _ (HostCaseObservation CaseRefused (Just message))) = not (null message)
      refused _ = False
  H.ensure (schema==1 && scope==SharedRouteAndDispatcher && accepted && failure==Nothing
    && sort [name | HostControlCase name _ _ _ _ <- cases]==[minBound..maxBound] && all refused cases
    && callStart < instructions && instructions <= callEnd && pc/=K.Address 0
    && physical==guest .&. 0xbfffffff && physical < callSp .&. 0xbfffffff) "host refusal cohort is incomplete"
  forM_ cases $ \(HostControlCase name _ directory receipt _) -> do
    H.ensure ([(T.unpack text) | (text,control) <- controlNames,control==name]==[Wire.relativeName directory]) "host control directory differs"
    capture <- H.readRegular 0x40000000 (root </> Wire.relativeName directory </> "startup-joined.json")
    H.ensure (hash capture==receipt) "host control receipt binding differs"
  let baselineRoot = root </> "baseline-pair"
  baseline <- H.readRegular 0x100000 (baselineRoot </> "shared-comparison.json")
  H.ensure (hash baseline==baselineDigest) "control baseline binding differs"
  HostAdmission policies _ <- checkComparison observer baselineRoot baseline retained joined
  summary <- readJson bytes
  pure (HostAdmission policies summary)

runHost :: FilePath -> FilePath -> K.Digest -> K.Digest -> [String] -> IO ()
runHost checker root (K.Digest checkerHex) (K.Digest producerHex) arguments = do
  (limited,status,out,err) <- H.runBounded 1800000000 (16*1024*1024) checker arguments B.empty
  B.writeFile (root </> "host-stdout.log") out
  B.writeFile (root </> "host-stderr.log") err
  writeJson (root </> "host-invocation.json") (J.object ["arguments" J..= arguments,"limited" J..= limited
    ,"exit" J..= (case status of ExitSuccess -> (0::Int); ExitFailure code -> code)
    ,"checker_sha256" J..= checkerHex,"producer_sha256" J..= producerHex])
  H.ensure (not limited && status==ExitSuccess) ("route/dispatcher host refusal: "++C8.unpack (B.take 2048 (out<>err)))

runHostAdmission :: HostEnvironment -> HostRequest -> IO HostAdmission
runHostAdmission (HostEnvironment checker producer analysis output) request = do
  createDirectory output
  producerDigest <- hash <$> H.readRegular 0x10000000 producer
  executable <- H.readRegular 0x10000000 checker
  let checkerDigest = hash executable
      frozenChecker = output </> "host-checker"
      hostOutput = output </> "route-dispatcher"
      paths = case request of Qualify capture -> [capture]; Compare retained joined -> [retained,joined]; RefusalControls retained joined -> [retained,joined]
  inputs <- forM paths (readCapture producerDigest)
  case inputs of
    [retained,joined] -> H.ensure (Wire.captureRole (captureEnvelope retained)==Wire.RetainedCapture
      && Wire.captureRole (captureEnvelope joined)==Wire.JoinedCapture) "host comparison capture roles differ"
    _ -> pure ()
  B.writeFile frozenChecker executable
  permissions <- getPermissions frozenChecker
  setPermissions frozenChecker (setOwnerExecutable True permissions)
  let arguments = (case request of
        Qualify capture -> ["--shared-check",capture]
        Compare retained joined -> ["--shared-compare-retained",retained,"--shared-compare-joined",joined]
        RefusalControls retained joined -> ["--shared-refusal-controls",retained,"--shared-refusal-joined",joined])
        ++["--analysis",analysis,"--output",hostOutput]
  runHost frozenChecker output checkerDigest producerDigest arguments
  executed <- H.readRegular 0x10000000 frozenChecker
  H.ensure (hash executed==checkerDigest) "executed host checker changed during admission"
  forM_ inputs checkCaptureUnchanged
  case (request,inputs) of
    (Qualify _,[input]) -> checkSingle producerDigest hostOutput input
    (Compare _ _,[retained,joined]) -> do
      bytes <- H.readRegular 0x100000 (hostOutput </> "shared-comparison.json")
      checkComparison producerDigest hostOutput bytes retained joined
    (RefusalControls _ _,[retained,joined]) -> checkControls producerDigest hostOutput retained joined
    _ -> H.failH "host capture request differs"
