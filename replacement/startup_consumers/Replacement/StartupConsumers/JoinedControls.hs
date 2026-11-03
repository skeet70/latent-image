{-# LANGUAGE OverloadedStrings #-}
module Replacement.StartupConsumers.JoinedControls (checkRefusals) where

import Control.Monad (forM)
import qualified Data.Aeson as J
import Data.Bits (xor)
import qualified Data.Aeson.Key as Key
import qualified Data.Aeson.KeyMap as KM
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as L
import qualified Data.Vector as V
import Numeric (showHex)
import qualified Emulation.Sha256 as SHA
import qualified Replacement.KernelInstall.Wire as K
import qualified Replacement.StartupConsumers.Evidence as Evidence
import qualified Replacement.StartupConsumers.Wire as Raw
import qualified Replacement.StartupConsumers.JoinedHost as Host
import qualified Replacement.StartupConsumers.JoinedWire as S
import qualified Replacement.StartupConsumers.JoinedWitness as Q
import qualified Replacement.StartupConsumers.ObservationPolicy as Policy
import qualified Replacement.StartupConsumers.Witness as W
import qualified Replacement.StartupConsumers.WitnessControls as Consumer

data ControlName = MissingArtifacts | MissingCopiedArtifact | ChangedCopiedArtifact | StandaloneOwnedProvider
  | MissingCatalogSource | ChangedUpdate | ChangedCheckpoint | ReusedCheckpoint | ChangedTrace | ChangedTimers
  | WrongWindowRole | MissingWindowEnd | FalseWindowTermination | FalseBeginCounts | FalseEndCounts | FalseTraceIndex
  | PrematureWindowEnd | MissingEpochs | EpochGap | MissingWatchBytes | ChangedWatchBytes
  | WindowSchema | WindowIdentity | MissingMode | MissingMarker | InteriorTraceContext deriving (Eq, Show)
newtype VectorOffset = VectorOffset Int
data Step = Field Key.Key | At VectorOffset
data Control = Control ControlName Q.JoinedError (J.Value -> Q.ObservedFiles -> Either String Q.ObservedFiles)

replace :: [Step] -> J.Value -> J.Value -> Either String J.Value
replace [] value _ = Right value
replace (Field key:rest) value (J.Object fields) = do
  child <- maybe (Left "control field absent") Right (KM.lookup key fields)
  changed <- replace rest value child
  pure (J.Object (KM.insert key changed fields))
replace (At (VectorOffset index):rest) value (J.Array values) = case values V.!? index of
  Nothing -> Left "control array offset absent"
  Just child -> do changed <- replace rest value child; pure (J.Array (values V.// [(index,changed)]))
replace _ _ _ = Left "control path shape differs"

captureChange :: [Step] -> J.Value -> J.Value -> Q.ObservedFiles -> Either String Q.ObservedFiles
captureChange path value original files = do
  changed <- replace path value original
  pure files {Q.observedCapture=L.toStrict (J.encode changed)}

removeHeader :: J.Value -> J.Value -> Either String J.Value
removeHeader address root = do
  consumer <- objectField "consumer" root
  window <- objectField "window" consumer
  resources <- arrayField "resources" window
  victim <- case [value | value <- V.toList resources,field "kind" value==Just (J.String "header"),field "physical" value==Just address] of
    [value] -> Right value
    _ -> Left "control header resource absent or duplicated"
  identifier <- maybe (Left "control resource identifier absent") Right (field "id" victim)
  probes <- arrayField "probes" window
  changedProbes <- mapM (dropSnapshot identifier) (V.toList probes)
  let retained = V.filter (/=victim) resources
  withResources <- replace [Field "consumer",Field "window",Field "resources"] (J.Array retained) root
  replace [Field "consumer",Field "window",Field "probes"] (J.Array (V.fromList changedProbes)) withResources
  where
    dropSnapshot identifier probe = do
      snapshots <- arrayField "snapshots" probe
      replace [Field "snapshots"] (J.Array (V.filter ((/=Just identifier) . field "resource") snapshots)) probe

interiorTraceContext :: J.Value -> Q.ObservedFiles -> Either String Q.ObservedFiles
interiorTraceContext root files = do
  envelope <- J.eitherDecodeStrict' (Q.observedCapture files)
  let window = S.consumerEvidence (S.captureConsumer envelope)
  selected <- case [probe | probe <- Raw.receiptProbes window,Raw.probeKind probe == Raw.FamilyEntry
    ,Raw.probeFamily probe == Just Raw.MessageConstruct,Raw.probeInvocation probe /= Nothing] of
    [probe] -> Right probe
    _ -> Left "control selected constructor entry absent or duplicated"
  originalTrace <- J.eitherDecodeStrict' (Q.observedTrace files)
  hits <- case J.fromJSON originalTrace of J.Success values -> Right values; J.Error message -> Left message
  (index,hit) <- case [(VectorOffset index,hit) | (index,hit) <- zip [0..] hits
    ,S.hitPc hit == K.contextPc (Evidence.machine selected),S.hitCore hit == Evidence.core selected
    ,S.hitInstructions hit == K.Count (Evidence.probeClock selected)] of
    value:_ -> Right value
    [] -> Left "control selected global trace occurrence absent"
  changedWord <- case drop 12 (S.registerWords (S.hitRegisters hit)) of
    [K.WordValue value] -> Right (value `xor` 1)
    _ -> Left "control r12 absent"
  changedTrace <- replace [At index,Field "registers",Field "r12"] (J.toJSON ("0x"++showHex changedWord "")) originalTrace
  changedRoute <- replace [Field "route",Field "events",At index,Field "registers",Field "r",At (VectorOffset 12)] (J.toJSON changedWord) root
  let bytes = L.toStrict (J.encode changedTrace)
  changed <- replace [Field "trace",Field "sha256"] (J.toJSON (SHA.sha256Hex (SHA.sha256 bytes))) changedRoute
  pure files {Q.observedCapture=L.toStrict (J.encode changed),Q.observedTrace=bytes}

field :: Key.Key -> J.Value -> Maybe J.Value
field key (J.Object fields) = KM.lookup key fields
field _ _ = Nothing
objectField :: Key.Key -> J.Value -> Either String J.Value
objectField key value = case field key value of Just result@(J.Object _) -> Right result; _ -> Left "control object absent"
arrayField :: Key.Key -> J.Value -> Either String (V.Vector J.Value)
arrayField key value = case field key value of Just (J.Array values) -> Right values; _ -> Left "control array absent"

controls :: [Control]
controls =
  [Control MissingArtifacts Q.SharedArtifactRejected (captureChange [Field "artifacts",Field "files"] (J.Array V.empty))
  ,Control MissingCopiedArtifact Q.SharedArtifactRejected $ \_ files -> pure files {Q.observedArtifacts=drop 1 (Q.observedArtifacts files)}
  ,Control ChangedCopiedArtifact Q.SharedArtifactRejected $ \_ files -> case Q.observedArtifacts files of
    Q.CapturedArtifact role bytes:rest -> pure files {Q.observedArtifacts=Q.CapturedArtifact role (B.snoc bytes 0):rest}
    [] -> Left "control copied artifacts absent"
  ,Control StandaloneOwnedProvider Q.SharedDecodeRejected (captureChange [Field "program",Field "role"] (J.String "owned"))
  ,Control MissingCatalogSource Q.SharedIdentityRejected (captureChange [Field "identity",Field "sources"] (J.Array V.empty))
  ,Control ChangedUpdate Q.SharedIdentityRejected $ \_ files -> pure files {Q.observedUpdate=B.snoc (Q.observedUpdate files) 0}
  ,Control ChangedCheckpoint Q.SharedCheckpointRejected $ \_ files -> pure files {Q.observedCheckpoint=B.snoc (Q.observedCheckpoint files) 0}
  ,Control ReusedCheckpoint Q.SharedCheckpointRejected (captureChange [Field "consumer",Field "window",Field "checkpoint_reused"] (J.Bool True))
  ,Control ChangedTrace Q.SharedIdentityRejected $ \_ files -> pure files {Q.observedTrace=Q.observedTrace files<>"\n"}
  ,Control ChangedTimers Q.SharedIdentityRejected $ \_ files -> pure files {Q.observedTimers=Q.observedTimers files<>"\n"}
  ,Control WrongWindowRole Q.SharedIdentityRejected (captureChange [Field "consumer",Field "window",Field "role"] (J.String "installer"))
  ,Control MissingWindowEnd Q.SharedSubobservationRejected (captureChange [Field "consumer",Field "end"] J.Null)
  ,Control FalseWindowTermination Q.SharedSubobservationRejected (captureChange [Field "consumer",Field "termination"] J.Null)
  ,Control FalseBeginCounts Q.SharedSubobservationRejected (captureChange [Field "consumer",Field "begin",Field "counts"] (J.toJSON ([0,0]::[Int])))
  ,Control FalseEndCounts Q.SharedSubobservationRejected (captureChange [Field "consumer",Field "end",Field "counts"] (J.toJSON ([0,0]::[Int])))
  ,Control FalseTraceIndex Q.SharedSubobservationRejected (captureChange [Field "consumer",Field "begin",Field "trace_index"] (J.Number 0))
  ,Control PrematureWindowEnd Q.SharedSubobservationRejected (captureChange [Field "consumer",Field "end",Field "instructions"] (J.Number 0))
  ,Control MissingEpochs Q.SharedGuardRejected (captureChange [Field "epochs"] (J.Array V.empty))
  ,Control EpochGap Q.SharedGuardRejected (captureChange [Field "epochs",At (VectorOffset 0),Field "start"] (J.Number 1))
  ,Control MissingWatchBytes Q.SharedGuardRejected $ \_ files -> pure files {Q.observedWatches=drop 1 (Q.observedWatches files)}
  ,Control ChangedWatchBytes Q.SharedGuardRejected $ \_ files -> case Q.observedWatches files of
    Q.CapturedWatch name bytes:rest -> pure files {Q.observedWatches=Q.CapturedWatch name (bytes<>"\n"):rest}
    [] -> Left "control watch files absent"
  ,Control WindowSchema (Q.ConsumerRejected W.DecodeRejected) (captureChange [Field "consumer",Field "window",Field "schema"] (J.Number 0))
  ,Control WindowIdentity (Q.ConsumerRejected W.IdentityRejected) (captureChange [Field "consumer",Field "window",Field "identity",Field "observer"] (J.String "0000000000000000000000000000000000000000000000000000000000000000"))
  ,Control MissingMode (Q.ConsumerRejected W.EffectRejected) (removeHeaderChange (J.Number 0x61c008))
  ,Control MissingMarker (Q.ConsumerRejected W.EffectRejected) (removeHeaderChange (J.Number 0x7caf3c))
  ,Control InteriorTraceContext Q.SharedSubobservationRejected interiorTraceContext]
  where
    removeHeaderChange address root files = do
      changed <- removeHeader address root
      pure files {Q.observedCapture=L.toStrict (J.encode changed)}

-- The caller admits and compares both captures first; each role's unmutated capture is admitted again here once under its own issued policy.
checkRefusals :: Q.Inputs -> Host.HostPolicyBytes -> Host.HostPolicyBytes -> Q.ObservedFiles -> Q.ObservedFiles -> Either String J.Value
checkRefusals inputs retainedPolicy joinedPolicy retained joined = do
  let crossBinding = Q.SharedPolicyRejected Policy.PolicyBindingRejected
  crossed <- forM [("retained capture with joined policy",joinedPolicy,retained),("joined capture with retained policy",retainedPolicy,joined)] $ \(label,policy,files) ->
    case Q.admitCapture inputs policy files of
      Left actual | actual==crossBinding -> pure (J.object ["control" J..= (label::String),"rejection" J..= show crossBinding])
      Left actual -> Left (label++": expected "++show crossBinding++", got "++show actual)
      Right _ -> Left (label++": admitted")
  checked <- forM [(retainedPolicy,retained),(joinedPolicy,joined)] $ \(policy,files) -> do
    (observation,check) <- either (Left . show) Right (Q.mutationChecker inputs policy files)
    root <- J.eitherDecodeStrict' (Q.observedCapture files)
    row <- forM controls $ \(Control name expected mutate) -> do
      changed <- mutate root files
      case check changed of
        Left actual | actual==expected -> pure (J.object ["control" J..= show name,"rejection" J..= show expected])
        Left actual -> Left (show name++": expected "++show expected++", got "++show actual)
        Right () -> Left (show name++": mutation admitted")
    pure (observation,row)
  (retainedObservation,joinedObservation,rows) <- case checked of
    [(left,leftRows),(right,rightRows)] -> Right (left,right,[leftRows,rightRows])
    _ -> Left "control role census differs"
  retainedCapture <- J.eitherDecodeStrict' (Q.observedCapture retained)
  joinedCapture <- J.eitherDecodeStrict' (Q.observedCapture joined)
  consumer <- Consumer.checkLoaderPairRefusals retainedObservation joinedObservation (Q.consumerInputs inputs)
    (S.consumerEvidence (S.captureConsumer retainedCapture)) (S.consumerEvidence (S.captureConsumer joinedCapture))
  pure (J.object ["schema" J..= (1::Int),"qualification" J..= ("exact-shared-startup-refusals"::String)
    ,"wrapper_controls" J..= rows,"policy_binding_controls" J..= crossed,"consumer_controls" J..= consumer])
