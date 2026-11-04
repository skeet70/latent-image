{-# LANGUAGE OverloadedStrings #-}
module Emulation.RootTaskWire where

import Control.Monad (unless)
import Data.Aeson
import Data.Aeson.Types (Parser)
import qualified Data.ByteString as B
import qualified Data.Map.Strict as M
import qualified Data.Text as T
import Data.Word (Word32)
import Emulation.TaskMemoryWire
import qualified Emulation.Sha256 as SHA
import qualified Emulation.StartupTaskWire as S
import qualified Replacement.KernelInstall.Wire as W

data Boundary = Start | RootEntry | RootInstruction | ServiceEntry | ServiceReturn | TaskSubmit | PeriodicCallback
  | IrqEntry | IrqFrame | IrqReturn | PostCleanup deriving (Eq, Ord, Show)
-- | Core0 ATCM bytes from 8 below the sample SP to the IRQ stack top.
data InterruptStack = InterruptStack {interruptLow :: W.Address, interruptBytes :: W.Bytes} deriving (Eq, Show)
data Sample = Sample
  { sampleHit :: Int, sampleBoundary :: Boundary, sampleContext :: Context, sampleCodeWord :: W.WordValue
  , sampleCursor :: Int, sampleViews :: [View], sampleInterruptStack :: Maybe InterruptStack } deriving (Eq, Show)
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
-- | Where view and interrupt-stack bytes live: inline JSON integer arrays, or the compact wire's
-- `{"offset","length"}` references into the `root-task.bytes` sidecar.
data ByteSource = InlineBytes | Sidecar B.ByteString
byteSource :: B.ByteString -> ByteSource
byteSource sidecar = if B.null sidecar then InlineBytes else Sidecar sidecar
bytesWith :: ByteSource -> Value -> Parser W.Bytes
bytesWith InlineBytes value = parseJSON value
bytesWith (Sidecar sidecar) value = flip (withObject "compact byte reference") value $ \o -> do
  offset <- o .: "offset" :: Parser Integer; size <- o .: "length" :: Parser Integer
  unless (offset >= 0 && size >= 0 && offset+size <= toInteger (B.length sidecar)) (fail "compact byte reference leaves its sidecar")
  pure (W.Bytes (B.take (fromInteger size) (B.drop (fromInteger offset) sidecar)))
viewWith :: ByteSource -> Value -> Parser View
viewWith source = withObject "root view" $ \o -> do
  aliases <- o .: "aliases" :: Parser [[T.Text]]
  digests <- case concat aliases of
    first:rest | length aliases == 2 && all ((==2) . length) aliases && all (==first) rest -> do
      digest <- parseJSON (String first)
      pure (replicate 2 (replicate 2 digest))
    _ -> mapM (mapM (parseJSON . String)) aliases
  View <$> o .: "object" <*> o .: "core" <*> o .: "physical" <*> (o .: "bytes" >>= bytesWith source) <*> pure digests
sampleWith :: ByteSource -> Value -> Parser Sample
sampleWith source = withObject "root sample" $ \o -> do
  hit <- o .: "hit"; cursor <- o .: "watch_cursor"
  unless (hit >= 0 && hit < 16384 && cursor >= 0 && cursor <= 1048576) (fail "root sample exceeds bound")
  views <- o .: "views" >>= mapM (viewWith source)
  stack <- o .:? "interrupt_stack" >>= mapM (withObject "root interrupt stack" (\s ->
    InterruptStack <$> s .: "low" <*> (s .: "bytes" >>= bytesWith source)))
  Sample hit <$> o .: "boundary" <*> o .: "context" <*> o .: "code_word" <*> pure cursor <*> pure views <*> pure stack
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
-- | A nonempty sidecar requires the compact wire's `compact` binding to its size and SHA-256; an empty one forbids it.
receiptWith :: B.ByteString -> Value -> Parser Receipt
receiptWith sidecar = withObject "root receipt" $ \o -> do
  scope <- o .: "scope" :: Parser String
  unless (scope == "natural-reset-root-task-lifecycle") (fail "invalid root scope")
  binding <- o .:? "compact"
  case (binding,byteSource sidecar) of
    (Nothing,InlineBytes) -> pure ()
    (Just compact,Sidecar bytes) -> flip (withObject "compact binding") compact $ \c -> do
      schema <- c .: "schema" :: Parser Int; name <- c .: "blob" :: Parser String
      size <- c .: "blob_bytes" :: Parser Integer; digest <- c .: "blob_sha256" :: Parser String
      unless (schema == 1 && name == "root-task.bytes" && size == toInteger (B.length bytes)
        && digest == SHA.sha256Hex (SHA.sha256 bytes)) (fail "compact sidecar binding differs")
    _ -> fail "compact binding and sidecar presence differ"
  rawSites <- o .: "sites" :: Parser (M.Map String Boundary)
  sites <- mapM (\(key,boundary) -> do
    location <- case reads key :: [(Integer,String)] of
      [(number,"")] | number >= 0 && number <= toInteger (maxBound :: Word32) -> pure (fromInteger number)
      _ -> fail "invalid root site"
    pure (W.Address location,boundary)) (M.toList rawSites)
  samples <- o .: "samples" >>= mapM (sampleWith (byteSource sidecar))
  Receipt <$> o .: "schema" <*> o .: "role" <*> o .: "outcome" <*> o .:? "error" <*> o .:? "machine_failure"
    <*> o .: "identity" <*> o .: "checkpoint_sha256" <*> o .: "checkpoint_reused" <*> o .: "instructions"
    <*> o .: "memory_watch_sha256" <*> o .: "watch" <*> o .:? "watch_end_error" <*> pure (M.fromList sites)
    <*> o .: "hits" <*> pure samples <*> o .: "final_menu" <*> o .:? "lifecycle_end_counts"
