{-# LANGUAGE OverloadedStrings #-}
module Replacement.RootTask.Artifact
  ( CanonicalSource, LinkedArtifact, StagedImage4, StagedCandidate, ArtifactError (..)
  , canonicalSource, deriveArtifact, artifactProof, artifactBytes
  , stageImage4, stagedBytes, contractValue
  , stageCandidate, candidateBytes, candidateContractValue
  ) where

import Control.Monad (forM_, unless)
import Data.Aeson (Value, object, (.=))
import Data.Bits ((.&.))
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import Data.Word (Word32)
import qualified Emulation.Image4Staging as Image4
import qualified Emulation.Sha256 as SHA
import qualified FujiAF.FirstStage as F
import qualified FujiAF.UpdateContainer as U
import Replacement.RootTask.Admission
import Replacement.RootTask.Services (serviceSymbols)
import qualified StockInventory as Stock

newtype SourceIdentity = SourceIdentity SHA.Sha256
newtype OriginalDat = OriginalDat B.ByteString
newtype DecodedImage4 = DecodedImage4 B.ByteString
data SourceIdentities = SourceIdentities SourceIdentity SourceIdentity
data ArtifactError = SourceMismatch | ExtentRejected | SymbolRejected | CodeRejected AdmissionError
  deriving (Eq, Show)
data CanonicalSource = CanonicalSource OriginalDat DecodedImage4 SourceIdentities
data LinkedArtifact = LinkedArtifact B.ByteString SourceIdentity SourceIdentity RootProof
data StagedImage4 = StagedImage4 CanonicalSource LinkedArtifact DecodedImage4
data StagedCandidate = StagedCandidate StagedImage4 Image4.StagedCandidate

origin, limit :: Word32
origin = 0x69d160
limit = 0x69d2b4
require :: Bool -> ArtifactError -> Either ArtifactError ()
require premise refusal = unless premise (Left refusal)
identity :: B.ByteString -> SourceIdentity
identity = SourceIdentity . SHA.sha256
identityText :: SourceIdentity -> String
identityText (SourceIdentity digest) = SHA.sha256Hex digest

canonicalSource :: B.ByteString -> B.ByteString -> Either ArtifactError CanonicalSource
canonicalSource dat image = do
  let datIdentity = identity dat
      imageIdentity = identity image
  require (identityText datIdentity == "43d67afa1a2bc9ae8481bc0acf02cb085ed0f515122dcbd26addcae102cd1490"
    && identityText imageIdentity == "f2e2acab892dafeed85171f6870a411f0a2d5eae10921b86a24630d401dcff16") SourceMismatch
  derived <- either (const (Left SourceMismatch)) Right (Stock.deriveFunction (Stock.GuestAddress 0x61c400) image
    (Stock.FunctionWindow (Stock.GuestAddress origin) (Stock.GuestAddress limit)) [])
  require (derived == (origin,340)) SourceMismatch
  _ <- either (Left . CodeRejected) Right
    (deriveRootProof (B.take 340 (B.drop (fromIntegral (origin-0x61c400)) image)))
  pure (CanonicalSource (OriginalDat dat) (DecodedImage4 image) (SourceIdentities datIdentity imageIdentity))

deriveArtifact :: B.ByteString -> B.ByteString -> Either ArtifactError LinkedArtifact
deriveArtifact bytes symbolBytes = do
  require (not (B.null bytes) && B.length bytes <= 340 && B.length bytes `mod` 4 == 0) ExtentRejected
  symbols <- either (const (Left SymbolRejected)) Right (F.parseSymbols symbolBytes)
  let lookupSymbol name = either (const (Left SymbolRejected)) Right (F.lookupSymbol symbols name)
      endAddress = origin+fromIntegral (B.length bytes)
  start <- lookupSymbol "__root_task_start"
  end <- lookupSymbol "__root_task_end"
  entry <- lookupSymbol "xm1_root_task"
  require (F.symbolAddress start == origin && F.symbolAddress end == endAddress
    && endAddress <= limit && F.symbolSize start == 0 && F.symbolSize end == 0) ExtentRejected
  require (F.symbolAddress entry == origin && F.symbolSize entry == fromIntegral (B.length bytes)) SymbolRejected
  forM_ serviceSymbols $ \(name,address) -> do
    symbol <- lookupSymbol name
    require (F.symbolAddress symbol == address && F.symbolSize symbol == 0) SymbolRejected
  forM_ symbols $ \symbol -> case lookup (F.symbolName symbol) serviceSymbols of
    Just address -> require (F.symbolAddress symbol == address && F.symbolSize symbol == 0) SymbolRejected
    Nothing -> do
      let address = F.symbolAddress symbol
          size = F.symbolSize symbol
      require (address .&. 3 == 0 && address >= origin && address <= endAddress
        && toInteger address+toInteger size <= toInteger endAddress
        && (size == 0 || F.symbolName symbol == "xm1_root_task")) SymbolRejected
  require (all validKind (filter (not . C.null) (C.lines symbolBytes))) SymbolRejected
  proof <- either (Left . CodeRejected) Right (deriveRootProof bytes)
  pure (LinkedArtifact bytes (identity bytes) (identity symbolBytes) proof)
  where
    validKind line = case C.words line of
      [_,_,kind,name] -> if C.unpack name `elem` map fst serviceSymbols then kind `elem` ["A","a"]
        else kind `elem` ["T","t"] && (name /= "xm1_root_task" || kind == "T")
      _ -> False

artifactProof :: LinkedArtifact -> RootProof
artifactProof (LinkedArtifact _ _ _ proof) = proof
artifactBytes :: LinkedArtifact -> B.ByteString
artifactBytes (LinkedArtifact bytes _ _ _) = bytes
stageImage4 :: CanonicalSource -> LinkedArtifact -> StagedImage4
stageImage4 source@(CanonicalSource _ (DecodedImage4 original) _) artifact =
  let linked = artifactBytes artifact
      offset = fromIntegral (origin-0x61c400)
      extent = fromIntegral (limit-origin)
      padding = B.concat (replicate ((extent-B.length linked) `div` 4) (U.putWord32 0xe320f000))
      staged = B.take offset original <> linked <> padding <> B.drop (offset+extent) original
  in StagedImage4 source artifact (DecodedImage4 staged)
stagedBytes :: StagedImage4 -> B.ByteString
stagedBytes (StagedImage4 _ _ (DecodedImage4 bytes)) = bytes
stageCandidate :: Image4.Tools -> Image4.ScratchDirectory -> StagedImage4 -> IO StagedCandidate
stageCandidate tools scratch staged@(StagedImage4 (CanonicalSource (OriginalDat original) _ _) _ (DecodedImage4 image)) = do
  candidate <- Image4.runImage4Staging tools scratch original original image
  pure (StagedCandidate staged candidate)
candidateBytes :: StagedCandidate -> B.ByteString
candidateBytes (StagedCandidate _ candidate) = Image4.candidateDat candidate
candidateContractValue :: StagedCandidate -> Value
candidateContractValue (StagedCandidate staged candidate) = object
  [ "schema" .= (1 :: Int), "qualification" .= ("host-admitted-root-task" :: String)
  , "root_task" .= contractValue staged
  , "candidate_dat_sha256" .= Image4.candidateSha256 candidate
  , "candidate_image4_sha256" .= Image4.candidateImage4Sha256 candidate
  ]
contractValue :: StagedImage4 -> Value
contractValue (StagedImage4 (CanonicalSource _ _ (SourceIdentities datIdentity imageIdentity))
    (LinkedArtifact linked linkedIdentity symbolIdentity proof) (DecodedImage4 staged)) = object
  [ "schema" .= (1 :: Int), "qualification" .= ("finite-fixed-site-host-admission" :: String)
  , "entry" .= origin, "replaced_extent" .= [origin,limit]
  , "linked_bytes" .= B.length linked, "linked_sha256" .= identityText linkedIdentity
  , "symbols_sha256" .= identityText symbolIdentity, "staged_image4_sha256" .= identityText (identity staged)
  , "scratch_low_from_entry_sp" .= scratchLow proof, "execution_pcs" .= executionPcs proof
  , "call_sites" .= [object ["pc" .= callPc site,"target" .= callTarget site
      ,"kind" .= (case callKind site of Call -> "call"; TailCall -> "tail-call" :: String)] | site <- callSites proof]
  , "halt_pcs" .= haltPcs proof, "return_pcs" .= returnPcs proof, "scenario_count" .= scenarioCount proof
  , "source_dat_image4_sha256" .= [identityText datIdentity,identityText imageIdentity]
  , "observable_contract" .=
      ([ "ordered retained calls, resources1..97 exactly once with argument1, ignored resource and registration results"
       , "checked-init nonzero halts, zero-mode optional configuration writes and invariant halt, nonzero later-publication chain"
       , "periodic callback/context remain retained; cleanup argument0 and opaque call-result handback through incoming LR"
       , "AAPCS r4-r11 and incoming SP restored on return; volatile registers and flags are unconstrained"
       , "no external stores, writable sections, unresolved PC writes, unbounded resource loops or hidden reachable code"
       ] :: [String])
  , "open_premises" .=
      ([ "finite checked-result representatives0,1,ffffffff,80000000 are not universal equivalence or runtime acceptance"
       , "natural69cfa4 mode/return/coordination outcome remains open"
       , "five early workers and tasks95/60/59 need actual creation, scheduling and continuation evidence"
       , "periodic registration/delivery and maintained callbacks remain runtime obligations"
       , "natural current-task cleanup nonreturn or mechanically returning continuation remains open; retained cleanup returns-63 if it returns"
       , "later image/publication/consumer ordering under153 remains separate"
       , "aligned allocated live stack, retained AAPCS services, immutable code/literals, aliases, TCM and continuous attributed writes remain runtime obligations"
       ] :: [String])
  ]
