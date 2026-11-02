{-# LANGUAGE OverloadedStrings #-}
module Replacement.StartupConsumers.JoinedWire
  ( ArtifactRole(..), RelativeFilename, relativeName, artifactFilename, traceFilename, timerFilename, resetFilename, updateFilename
  , FileBinding(..), CaptureRole(..), ProgramBinding(..), CoreCounts(..), Observed(..), MenuCursor(..)
  , SourceKind(..), SourceBinding(..), InputIdentity(..), TraceIndex(..), SegmentIndex(..), Endpoint(..), Termination(..), ConsumerWindow(..)
  , EpochStage(..), EpochEvidence(..), Epoch(..), TraceHit(..), SharedCapture(..)
  ) where

import Control.Monad (unless)
import Data.Aeson
import Data.Aeson.Types (Parser)
import qualified Data.Aeson.KeyMap as KM
import qualified Data.Text as T
import Data.Word (Word32, Word64)
import Numeric (readHex)
import Replacement.KernelInstall.Wire (Address(..), Count(..), Core, Digest)
import qualified Replacement.StartupConsumers.Wire as W

newtype RelativeFilename = RelativeFilename FilePath deriving (Eq, Ord, Show)
relativeName :: RelativeFilename -> FilePath
relativeName (RelativeFilename value) = value
instance FromJSON RelativeFilename where
  parseJSON = withText "relative filename" $ \value -> do
    let name = T.unpack value
    unless (not (null name) && name/="." && name/=".." && all (`notElem` ['/', '\\', '\NUL']) name) (fail "expected one relative filename")
    pure (RelativeFilename name)

data ArtifactRole = CanonicalDat | CanonicalImage4 | CanonicalLoaderA | CanonicalLoaderB
  | CandidateDat | CandidateImage4 | CandidateLoaderA | CandidateLoaderB
  | NativeLoaderA | SymbolsLoaderA | NativeLoaderB | SymbolsLoaderB
  | NativeDispatcher | SymbolsDispatcher | NativeInstaller | SymbolsInstaller | JoinedContract
  deriving (Eq, Ord, Show, Enum, Bounded)
artifactFilename :: ArtifactRole -> RelativeFilename
artifactFilename role = RelativeFilename ("staged-"++case role of
  CanonicalDat -> "canonical.dat"; CanonicalImage4 -> "canonical-image4.bin"
  CanonicalLoaderA -> "canonical-loader-a.bin"; CanonicalLoaderB -> "canonical-loader-b.bin"
  CandidateDat -> "candidate.dat"; CandidateImage4 -> "candidate-image4.bin"
  CandidateLoaderA -> "candidate-loader-a.bin"; CandidateLoaderB -> "candidate-loader-b.bin"
  NativeLoaderA -> "loader-a.bin"; SymbolsLoaderA -> "loader-a.symbols"
  NativeLoaderB -> "loader-b.bin"; SymbolsLoaderB -> "loader-b.symbols"
  NativeDispatcher -> "dispatcher.bin"; SymbolsDispatcher -> "dispatcher.symbols"
  NativeInstaller -> "installer.bin"; SymbolsInstaller -> "installer.symbols"; JoinedContract -> "contract.json")
traceFilename, timerFilename, resetFilename, updateFilename :: RelativeFilename
traceFilename = RelativeFilename "trace.json"
timerFilename = RelativeFilename "timer-transactions.json"
resetFilename = RelativeFilename "reset.fujimod"
updateFilename = RelativeFilename "input-update.DAT"

data FileBinding = FileBinding {bindingPath :: RelativeFilename,bindingDigest :: Digest} deriving (Eq, Show)
instance FromJSON FileBinding where parseJSON = withObject "file binding" $ \o -> FileBinding <$> o .: "path" <*> o .: "sha256"
newtype Artifacts = Artifacts [FileBinding]
instance FromJSON Artifacts where parseJSON = withObject "artifacts" $ \o -> Artifacts <$> o .: "files"

data CaptureRole = RetainedCapture | JoinedCapture deriving (Eq, Show)
instance FromJSON CaptureRole where parseJSON = W.tagged "shared role" [("retained",RetainedCapture),("joined",JoinedCapture)]
data ProgramBinding = RetainedProgram | JoinedProgram [FileBinding] deriving (Eq, Show)
instance FromJSON ProgramBinding where
  parseJSON = withObject "program binding" $ \o -> do
    role <- o .: "role"
    case role of
      RetainedCapture -> pure RetainedProgram
      JoinedCapture -> do Artifacts files <- o .: "artifacts"; pure (JoinedProgram files)

data CoreCounts = CoreCounts Count Count deriving (Eq, Show)
instance FromJSON CoreCounts where
  parseJSON value = do
    values <- parseJSON value
    case values of [primary,secondary] -> pure (CoreCounts primary secondary); _ -> fail "expected two actual core counts"
data Observed a = Observed a | ObservationFailed String deriving (Eq, Show)
instance FromJSON a => FromJSON (Observed a) where
  parseJSON = withObject "fallible observation" $ \o -> do
    case (KM.lookup "Ok" o,KM.lookup "Err" o) of
      (Just value,Nothing) -> Observed <$> parseJSON value
      (Nothing,Just message) -> ObservationFailed <$> parseJSON message
      _ -> fail "expected one observation result"
data MenuCursor = MenuCursor Word32 Word32 Word32 deriving (Eq, Show)
instance FromJSON MenuCursor where
  parseJSON value = do
    values <- parseJSON value
    case values of [group,page,item] -> pure (MenuCursor group page item); _ -> fail "expected a menu cursor"

data SourceKind = BootA | BootB | LoaderA | LoaderB | Image4 | Image5 | Plain deriving (Eq, Ord, Show, Enum, Bounded)
instance FromJSON SourceKind where
  parseJSON = W.tagged "source kind" [("boot-a",BootA),("boot-b",BootB),("loader-a",LoaderA),("loader-b",LoaderB),("image4",Image4),("image5",Image5),("plain",Plain)]
data SourceBinding = SourceBinding {sourceKind :: SourceKind,sourceStart :: Address,sourceBytes :: Count,sourceDigest :: Digest} deriving (Eq, Show)
instance FromJSON SourceBinding where parseJSON = withObject "source binding" $ \o -> SourceBinding <$> o .: "kind" <*> o .: "start" <*> o .: "bytes" <*> o .: "sha256"
data InputIdentity = InputIdentity
  {identityRun :: Digest,identityUpdate :: Digest,identityUpdateFile :: FileBinding,identityRom :: Digest
  ,identityExecutable :: Digest,identitySources :: [SourceBinding]} deriving (Eq, Show)
instance FromJSON InputIdentity where
  parseJSON = withObject "shared identity" $ \o -> InputIdentity <$> o .: "run" <*> o .: "update" <*> o .: "update_file"
    <*> o .: "rom" <*> o .: "executable" <*> o .: "sources"

newtype TraceIndex = TraceIndex Word64 deriving (Eq, Ord, Show)
instance FromJSON TraceIndex where parseJSON value = TraceIndex <$> parseJSON value
newtype SegmentIndex = SegmentIndex Word64 deriving (Eq, Ord, Show)
instance FromJSON SegmentIndex where parseJSON value = SegmentIndex <$> parseJSON value
data Endpoint = Endpoint {endpointInstructions :: Count,endpointCounts :: CoreCounts,endpointTraceIndex :: TraceIndex} deriving (Eq, Show)
instance FromJSON Endpoint where parseJSON = withObject "subobservation endpoint" $ \o -> Endpoint <$> o .: "instructions" <*> o .: "counts" <*> o .: "trace_index"
data Termination = FirstFamiliesAndSecondaryRoot1 deriving (Eq, Show)
instance FromJSON Termination where parseJSON = W.tagged "subobservation termination" [("first-families-and-secondary-root1",FirstFamiliesAndSecondaryRoot1)]
data ConsumerWindow = ConsumerWindow {consumerBegin :: Maybe Endpoint,consumerEnd :: Maybe Endpoint,consumerTermination :: Maybe Termination,consumerEvidence :: W.Receipt} deriving (Eq, Show)
instance FromJSON ConsumerWindow where parseJSON = withObject "consumer window" $ \o -> ConsumerWindow <$> o .: "begin" <*> o .: "end" <*> o .: "termination" <*> o .: "window"

data EpochStage = Bootstrap | Startup | Tail deriving (Eq, Show)
instance FromJSON EpochStage where parseJSON = W.tagged "epoch stage" [("bootstrap",Bootstrap),("startup",Startup),("tail",Tail)]
data EpochEvidence = WatchFile FileBinding (Maybe String) | ConsumerSegment SegmentIndex deriving (Eq, Show)
instance FromJSON EpochEvidence where
  parseJSON = withObject "epoch evidence" $ \o -> do
    kind <- o .: "kind" :: Parser T.Text
    case kind of
      "watch" -> WatchFile <$> o .: "file" <*> o .: "teardown_error"
      "consumer-segment" -> ConsumerSegment <$> o .: "segment"
      _ -> fail "unknown epoch evidence"
data Epoch = Epoch {epochStage :: EpochStage,epochStart :: Count,epochEnd :: Count,epochStartCounts :: CoreCounts,epochEndCounts :: CoreCounts,epochEvidence :: EpochEvidence} deriving (Eq, Show)
instance FromJSON Epoch where parseJSON = withObject "shared epoch" $ \o -> Epoch <$> o .: "stage" <*> o .: "start" <*> o .: "end" <*> o .: "start_counts" <*> o .: "end_counts" <*> o .: "evidence"

newtype HexAddress = HexAddress Address
instance FromJSON HexAddress where
  parseJSON = withText "hexadecimal address" $ \value -> case T.stripPrefix "0x" value of
    Nothing -> fail "expected hexadecimal address"
    Just digits -> case readHex (T.unpack digits) of [(number,"")] | number <= (0xffffffff :: Integer) -> pure (HexAddress (Address (fromInteger number))); _ -> fail "invalid hexadecimal address"
data TraceHit = TraceHit {hitPc :: Address,hitCore :: Core,hitInstructions :: Count} deriving (Eq, Show)
instance FromJSON TraceHit where
  parseJSON = withObject "global trace hit" $ \o -> do
    HexAddress pc <- o .: "address"
    TraceHit pc <$> o .: "core" <*> o .: "instructions"

data SharedCapture = SharedCapture
  {captureSchema :: Int,captureRole :: CaptureRole,captureProgram :: ProgramBinding,captureArtifacts :: [FileBinding]
  ,captureIdentity :: InputIdentity,captureCheckpoint :: Digest,captureInstructions :: Count,captureCounts :: Observed CoreCounts
  ,captureMenu :: Observed (Maybe MenuCursor),captureError :: Maybe String,captureFailure :: Maybe String
  ,captureEpochs :: [Epoch],captureConsumer :: ConsumerWindow,captureTrace :: FileBinding,captureTimers :: FileBinding} deriving (Eq, Show)
instance FromJSON SharedCapture where
  parseJSON = withObject "shared startup capture" $ \o -> do
    Artifacts artifacts <- o .: "artifacts"
    SharedCapture <$> o .: "schema" <*> o .: "role" <*> o .: "program" <*> pure artifacts
      <*> o .: "identity" <*> o .: "checkpoint" <*> o .: "instructions" <*> o .: "counts" <*> o .: "menu"
      <*> o .: "error" <*> o .: "machine_failure" <*> o .: "epochs" <*> o .: "consumer" <*> o .: "trace" <*> o .: "timer_transactions"
