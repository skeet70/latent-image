{-# LANGUAGE OverloadedStrings #-}
module Emulation.IdleTaskWire where

import Control.Monad (unless)
import Data.Aeson
import Data.Aeson.Types (Parser)
import qualified Data.Map.Strict as M
import Data.Word (Word32)
import qualified Emulation.StartupTaskWire as S
import qualified Replacement.KernelInstall.Wire as W

data Role = Retained | Owned deriving (Eq, Show)
data Boundary = Start | IdleEntry | IdleInstruction | IrqEntry | IrqFrame | IrqReturn deriving (Eq, Ord, Show)
data Object = CreationRecord | TaskNode | Current | Selected | Pool | Stack | Code | LoadedCode | InterfaceRoots | InterfaceTable deriving (Eq, Ord, Show)
data View = View {viewObject :: Object, viewCore :: S.KernelCore, viewPhysical :: W.Address, viewBytes :: W.Bytes, viewAliases :: [[W.Digest]]} deriving (Eq, Show)
data Context = Context {contextCore :: S.KernelCore, contextState :: W.Context, contextSpsr :: W.WordValue} deriving (Eq, Show)
data Sample = Sample {sampleHit :: Int, sampleBoundary :: Boundary, sampleContext :: Context, sampleCodeWord :: W.WordValue, sampleCursor :: Int, sampleViews :: [View]} deriving (Eq, Show)
data Hit = Hit {hitBoundary :: Boundary, hitCore :: S.KernelCore, hitState :: W.Context, hitCount :: W.Count} deriving (Eq, Show)
data Identity = Identity {identityRun :: W.Digest, identityUpdate :: W.Digest, identityRom :: W.Digest, identityObserver :: W.Digest, identityLinked :: W.Digest, identitySymbols :: W.Digest, identityContract :: W.Digest, identitySources :: [S.SourceIdentity]} deriving (Eq, Show)
data Receipt = Receipt
  { receiptSchema :: Int, receiptRole :: Role, receiptOutcome :: S.Outcome, receiptError :: Maybe String, receiptMachineFailure :: Maybe String
  , receiptIdentity :: Identity, receiptCheckpoint :: W.Digest, receiptCheckpointReused :: Bool, receiptInstructions :: W.Count
  , receiptWatchDigest :: W.Digest, receiptWatch :: S.WatchResult, receiptWatchEndError :: Maybe String
  , receiptSites :: M.Map W.Address Boundary, receiptHits :: [Hit], receiptSamples :: [Sample], receiptMenu :: S.MenuResult, receiptEndCounts :: Maybe [W.Count]
  } deriving (Eq, Show)

parseRole :: String -> Parser Role
parseRole "retained" = pure Retained
parseRole "owned" = pure Owned
parseRole _ = fail "invalid idle role"
instance FromJSON Role where parseJSON value = parseJSON value >>= parseRole
instance ToJSON Role where toJSON Retained = String "retained"; toJSON Owned = String "owned"
instance FromJSON Boundary where
  parseJSON = withText "idle boundary" $ \value -> case value of
    "start" -> pure Start; "entry" -> pure IdleEntry; "instruction" -> pure IdleInstruction
    "irq-entry" -> pure IrqEntry; "irq-frame" -> pure IrqFrame; "irq-return" -> pure IrqReturn
    _ -> fail "invalid idle boundary"
instance FromJSON Object where
  parseJSON = withText "idle view" $ \value -> case value of
    "creation-record" -> pure CreationRecord; "task-node" -> pure TaskNode; "current" -> pure Current; "selected" -> pure Selected
    "pool" -> pure Pool; "stack" -> pure Stack; "code" -> pure Code; "interface-roots" -> pure InterfaceRoots; "interface-table" -> pure InterfaceTable
    "loaded-code" -> pure LoadedCode
    _ -> fail "invalid idle object"
instance FromJSON View where
  parseJSON = withObject "idle view" $ \o -> View <$> o .: "object" <*> o .: "core" <*> o .: "physical" <*> o .: "bytes" <*> o .: "aliases"
instance FromJSON Context where
  parseJSON value = withObject "idle context" (\o -> Context <$> o .: "core" <*> parseJSON value <*> o .: "spsr") value
instance FromJSON Sample where
  parseJSON = withObject "idle sample" $ \o -> do
    hit <- o .: "hit"; cursor <- o .: "watch_cursor"
    unless (hit >= 0 && hit < 16384 && cursor >= 0 && cursor <= 1048576) (fail "idle sample exceeds bound")
    Sample hit <$> o .: "boundary" <*> o .: "context" <*> o .: "code_word" <*> pure cursor <*> o .: "views"
instance FromJSON Hit where
  parseJSON = withObject "idle hit" $ \o -> do
    boundary <- o .: "site"; pc <- o .: "address" >>= S.hexWord; core <- o .: "core" :: Parser Int
    selected <- case core of 0 -> pure S.PrimaryKernel; 1 -> pure S.SecondaryKernel; _ -> fail "invalid idle hit core"
    registers <- o .: "registers" :: Parser (M.Map String String)
    let names = ["r" ++ show index | index <- [0 :: Int ..12]]
    unless (M.keys registers == M.keys (M.fromList [(name,()) | name <- names])) (fail "incomplete idle hit registers")
    values <- mapM (\name -> maybe (fail "missing idle register") (fmap W.WordValue . S.hexWord) (M.lookup name registers)) names
    sp <- o .: "sp" >>= S.hexWord; lr <- o .: "lr" >>= S.hexWord; cpsr <- o .: "cpsr"; count <- o .: "instructions"
    pure (Hit boundary selected (W.Context (W.Address pc) values (W.Address sp) (W.Address lr) cpsr [] [] []) count)
instance FromJSON Identity where
  parseJSON = withObject "idle identity" $ \o -> Identity <$> o .: "run" <*> o .: "update" <*> o .: "boot_rom" <*> o .: "observer"
    <*> o .: "linked" <*> o .: "symbols" <*> o .: "contract" <*> o .: "sources"
instance FromJSON Receipt where
  parseJSON = withObject "idle receipt" $ \o -> do
    scope <- o .: "scope" :: Parser String
    unless (scope == "natural-reset-idle-task-lifecycle") (fail "invalid idle scope")
    rawSites <- o .: "sites" :: Parser (M.Map String Boundary)
    sites <- mapM (\(key,boundary) -> do
      location <- case reads key :: [(Integer,String)] of
        [(number,"")] | number >= 0 && number <= toInteger (maxBound :: Word32) -> pure (fromInteger number)
        _ -> fail "invalid idle site"
      pure (W.Address location,boundary)) (M.toList rawSites)
    Receipt <$> o .: "schema" <*> o .: "role" <*> o .: "outcome" <*> o .:? "error" <*> o .:? "machine_failure"
      <*> o .: "identity" <*> o .: "checkpoint_sha256" <*> o .: "checkpoint_reused" <*> o .: "instructions"
      <*> o .: "memory_watch_sha256" <*> o .: "watch" <*> o .:? "watch_end_error" <*> pure (M.fromList sites)
      <*> o .: "hits" <*> o .: "samples" <*> o .: "final_menu" <*> o .:? "lifecycle_end_counts"
