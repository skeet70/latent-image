{-# LANGUAGE OverloadedStrings #-}
module Emulation.RootTaskWire where

import Control.Monad (unless)
import Data.Aeson
import Data.Aeson.Types (Parser)
import qualified Data.Map.Strict as M
import Data.Word (Word32)
import Emulation.TaskMemoryWire
import qualified Emulation.StartupTaskWire as S
import qualified Replacement.KernelInstall.Wire as W

data Boundary = Start | RootEntry | RootInstruction | ServiceEntry | ServiceReturn | TaskSubmit | PeriodicCallback
  | IrqEntry | IrqFrame | IrqReturn | PostCleanup deriving (Eq, Ord, Show)
data Sample = Sample
  { sampleHit :: Int, sampleBoundary :: Boundary, sampleContext :: Context, sampleCodeWord :: W.WordValue
  , sampleCursor :: Int, sampleViews :: [View] } deriving (Eq, Show)
data Hit = Hit {hitBoundary :: Boundary, hitCore :: S.KernelCore, hitState :: W.Context, hitCount :: W.Count} deriving (Eq, Show)
data Receipt = Receipt
  { receiptSchema :: Int, receiptRole :: Role, receiptOutcome :: S.Outcome, receiptError :: Maybe String, receiptMachineFailure :: Maybe String
  , receiptIdentity :: Identity, receiptCheckpoint :: W.Digest, receiptCheckpointReused :: Bool, receiptInstructions :: W.Count
  , receiptWatchDigest :: W.Digest, receiptWatch :: S.WatchResult, receiptWatchEndError :: Maybe String
  , receiptSites :: M.Map W.Address Boundary, receiptHits :: [Hit], receiptSamples :: [Sample], receiptMenu :: S.MenuResult
  , receiptEndCounts :: Maybe [W.Count] } deriving (Eq, Show)

instance FromJSON Boundary where
  parseJSON = withText "root boundary" $ \value -> case value of
    "start" -> pure Start; "root-entry" -> pure RootEntry; "root-instruction" -> pure RootInstruction
    "service-entry" -> pure ServiceEntry; "service-return" -> pure ServiceReturn; "task-submit" -> pure TaskSubmit
    "periodic-callback" -> pure PeriodicCallback; "irq-entry" -> pure IrqEntry; "irq-frame" -> pure IrqFrame
    "irq-return" -> pure IrqReturn; "post-cleanup" -> pure PostCleanup; _ -> fail "invalid root boundary"
instance FromJSON Sample where
  parseJSON = withObject "root sample" $ \o -> do
    hit <- o .: "hit"; cursor <- o .: "watch_cursor"
    unless (hit >= 0 && hit < 16384 && cursor >= 0 && cursor <= 1048576) (fail "root sample exceeds bound")
    Sample hit <$> o .: "boundary" <*> o .: "context" <*> o .: "code_word" <*> pure cursor <*> o .: "views"
instance FromJSON Hit where
  parseJSON = withObject "root hit" $ \o -> do
    boundary <- o .: "site"; pc <- o .: "address" >>= S.hexWord; core <- o .: "core" :: Parser Int
    selected <- case core of 0 -> pure S.PrimaryKernel; 1 -> pure S.SecondaryKernel; _ -> fail "invalid root hit core"
    registers <- o .: "registers" :: Parser (M.Map String String)
    let names = ["r" ++ show index | index <- [0 :: Int ..12]]
    unless (M.keys registers == M.keys (M.fromList [(name,()) | name <- names])) (fail "incomplete root hit registers")
    values <- mapM (\name -> maybe (fail "missing root register") (fmap W.WordValue . S.hexWord) (M.lookup name registers)) names
    sp <- o .: "sp" >>= S.hexWord; lr <- o .: "lr" >>= S.hexWord; cpsr <- o .: "cpsr"; clock <- o .: "instructions"
    pure (Hit boundary selected (W.Context (W.Address pc) values (W.Address sp) (W.Address lr) cpsr [] [] []) clock)
instance FromJSON Receipt where
  parseJSON = withObject "root receipt" $ \o -> do
    scope <- o .: "scope" :: Parser String
    unless (scope == "natural-reset-root-task-lifecycle") (fail "invalid root scope")
    rawSites <- o .: "sites" :: Parser (M.Map String Boundary)
    sites <- mapM (\(key,boundary) -> do
      location <- case reads key :: [(Integer,String)] of
        [(number,"")] | number >= 0 && number <= toInteger (maxBound :: Word32) -> pure (fromInteger number)
        _ -> fail "invalid root site"
      pure (W.Address location,boundary)) (M.toList rawSites)
    Receipt <$> o .: "schema" <*> o .: "role" <*> o .: "outcome" <*> o .:? "error" <*> o .:? "machine_failure"
      <*> o .: "identity" <*> o .: "checkpoint_sha256" <*> o .: "checkpoint_reused" <*> o .: "instructions"
      <*> o .: "memory_watch_sha256" <*> o .: "watch" <*> o .:? "watch_end_error" <*> pure (M.fromList sites)
      <*> o .: "hits" <*> o .: "samples" <*> o .: "final_menu" <*> o .:? "lifecycle_end_counts"
