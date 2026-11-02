{-# LANGUAGE OverloadedStrings #-}
module Replacement.StartupConsumers.JoinedControls (checkRefusals) where

import Control.Monad (forM)
import qualified Data.Aeson as J
import qualified Data.Aeson.Key as Key
import qualified Data.Aeson.KeyMap as KM
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as L
import qualified Data.Vector as V
import qualified Replacement.StartupConsumers.JoinedWire as S
import qualified Replacement.StartupConsumers.JoinedWitness as Q
import qualified Replacement.StartupConsumers.Witness as W
import qualified Replacement.StartupConsumers.WitnessControls as Consumer

data ControlName = MissingArtifacts | MissingCopiedArtifact | ChangedCopiedArtifact | StandaloneOwnedProvider
  | MissingCatalogSource | ChangedUpdate | ChangedCheckpoint | ReusedCheckpoint | ChangedTrace | ChangedTimers
  | WrongWindowRole | MissingWindowEnd | FalseWindowTermination | FalseBeginCounts | FalseEndCounts | FalseTraceIndex
  | PrematureWindowEnd | MissingEpochs | EpochGap | MissingWatchBytes | ChangedWatchBytes
  | WindowSchema | WindowIdentity | MissingMode | MissingMarker deriving (Eq, Show)
data Step = Field Key.Key | First
data Control = Control ControlName Q.JoinedError (J.Value -> Q.ObservedFiles -> Either String Q.ObservedFiles)

replace :: [Step] -> J.Value -> J.Value -> Either String J.Value
replace [] value _ = Right value
replace (Field key:rest) value (J.Object fields) = do
  child <- maybe (Left "control field absent") Right (KM.lookup key fields)
  changed <- replace rest value child
  pure (J.Object (KM.insert key changed fields))
replace (First:rest) value (J.Array values) = case values V.!? 0 of
  Nothing -> Left "control array empty"
  Just child -> do changed <- replace rest value child; pure (J.Array (values V.// [(0,changed)]))
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
  ,Control EpochGap Q.SharedGuardRejected (captureChange [Field "epochs",First,Field "start"] (J.Number 1))
  ,Control MissingWatchBytes Q.SharedGuardRejected $ \_ files -> pure files {Q.observedWatches=drop 1 (Q.observedWatches files)}
  ,Control ChangedWatchBytes Q.SharedGuardRejected $ \_ files -> case Q.observedWatches files of
    Q.CapturedWatch name bytes:rest -> pure files {Q.observedWatches=Q.CapturedWatch name (bytes<>"\n"):rest}
    [] -> Left "control watch files absent"
  ,Control WindowSchema (Q.ConsumerRejected W.DecodeRejected) (captureChange [Field "consumer",Field "window",Field "schema"] (J.Number 0))
  ,Control WindowIdentity (Q.ConsumerRejected W.IdentityRejected) (captureChange [Field "consumer",Field "window",Field "identity",Field "observer"] (J.String "0000000000000000000000000000000000000000000000000000000000000000"))
  ,Control MissingMode (Q.ConsumerRejected W.EffectRejected) (removeHeaderChange (J.Number 0x61c008))
  ,Control MissingMarker (Q.ConsumerRejected W.EffectRejected) (removeHeaderChange (J.Number 0x7caf3c))]
  where
    removeHeaderChange address root files = do
      changed <- removeHeader address root
      pure files {Q.observedCapture=L.toStrict (J.encode changed)}

checkRefusals :: Q.Inputs -> Q.ObservedFiles -> Q.ObservedFiles -> Either String J.Value
checkRefusals inputs retained joined = do
  before <- either (Left . show) Right (Q.admitCapture inputs retained)
  after <- either (Left . show) Right (Q.admitCapture inputs joined)
  either (Left . show) Right (Q.compareCaptures before after)
  rows <- forM [retained,joined] $ \files -> do
    root <- J.eitherDecodeStrict' (Q.observedCapture files)
    forM controls $ \(Control name expected mutate) -> do
      changed <- mutate root files
      case Q.admitCapture inputs changed of
        Left actual | actual==expected -> pure (J.object ["control" J..= show name,"rejection" J..= show expected])
        Left actual -> Left (show name++": expected "++show expected++", got "++show actual)
        Right _ -> Left (show name++": mutation admitted")
  retainedCapture <- J.eitherDecodeStrict' (Q.observedCapture retained)
  joinedCapture <- J.eitherDecodeStrict' (Q.observedCapture joined)
  consumer <- Consumer.checkLoaderPairRefusals (Q.consumerInputs inputs)
    (S.consumerEvidence (S.captureConsumer retainedCapture)) (S.consumerEvidence (S.captureConsumer joinedCapture))
  pure (J.object ["schema" J..= (1::Int),"qualification" J..= ("exact-shared-startup-refusals"::String)
    ,"wrapper_controls" J..= rows,"consumer_controls" J..= consumer])
