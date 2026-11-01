{-# LANGUAGE OverloadedStrings #-}
module Replacement.PlainLoad.Artifact
  ( CanonicalSource, LinkedArtifact, StagedImage4, StagedCandidate, ArtifactError (..)
  , canonicalSource, deriveArtifact, stageImage4, stagedBytes, contractValue
  , stageCandidate, candidateBytes, candidateContractValue
  ) where

import Control.Monad (unless)
import Data.Aeson (Value, object, (.=))
import Data.Bits ((.&.))
import qualified Data.ByteString as B
import Data.Word (Word32)
import qualified Emulation.Image4Staging as Image4
import qualified Emulation.Sha256 as SHA
import qualified FujiAF.FirstStage as F
import qualified FujiAF.UpdateContainer as U
import Replacement.PlainLoad.Admission

newtype SourceIdentity = SourceIdentity SHA.Sha256
newtype CodeAddress = CodeAddress Word32 deriving (Eq, Ord)
data Symbol = StartSymbol | EndSymbol | EntrySymbol | RustSymbol
newtype OriginalDat = OriginalDat B.ByteString
newtype DecodedImage4 = DecodedImage4 B.ByteString
data SourceIdentities = SourceIdentities SourceIdentity SourceIdentity
data ArtifactError = SourceMismatch | ExtentRejected | SymbolRejected | CodeRejected AdmissionError
  deriving (Eq, Show)
data CanonicalSource = CanonicalSource OriginalDat DecodedImage4 SourceIdentities
data LinkedArtifact = LinkedArtifact B.ByteString SourceIdentity SourceIdentity CodeAddress ProducerProof
data StagedImage4 = StagedImage4 CanonicalSource LinkedArtifact DecodedImage4
data StagedCandidate = StagedCandidate StagedImage4 Image4.StagedCandidate

origin, limit :: CodeAddress
origin = CodeAddress 0x6a9844
limit = CodeAddress 0x6a9938

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
  pure (CanonicalSource (OriginalDat dat) (DecodedImage4 image) (SourceIdentities datIdentity imageIdentity))

deriveArtifact :: B.ByteString -> B.ByteString -> Either ArtifactError LinkedArtifact
deriveArtifact bytes symbolBytes = do
  symbols <- either (const (Left SymbolRejected)) Right (F.parseSymbols symbolBytes)
  let address symbol = do
        let name = case symbol of
              StartSymbol -> "__plain_load_start"
              EndSymbol -> "__plain_load_end"
              EntrySymbol -> "xm1_plain_load_entry"
              RustSymbol -> "xm1_plain_load"
        word <- either (const (Left SymbolRejected)) (Right . F.symbolAddress) (F.lookupSymbol symbols name)
        require (word .&. 3 == 0) SymbolRejected
        pure (CodeAddress word)
  start@(CodeAddress startWord) <- address StartSymbol
  end@(CodeAddress endWord) <- address EndSymbol
  entry <- address EntrySymbol
  rust <- address RustSymbol
  require (start == origin && end > start && end <= limit
    && toInteger (B.length bytes) == toInteger endWord-toInteger startWord) ExtentRejected
  require (entry == start && rust > start && rust < end) SymbolRejected
  proof <- either (Left . CodeRejected) Right (deriveProducerProof bytes)
  pure (LinkedArtifact bytes (identity bytes) (identity symbolBytes) rust proof)

stageImage4 :: CanonicalSource -> LinkedArtifact -> StagedImage4
stageImage4 source@(CanonicalSource _ (DecodedImage4 original) _) artifact@(LinkedArtifact linked _ _ _ _) =
  let CodeAddress start = origin
      CodeAddress end = limit
      offset = fromIntegral (start-0x61c400)
      extent = fromIntegral (end-start)
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
  [ "schema" .= (1 :: Int), "qualification" .= ("host-admitted-plain-producer" :: String)
  , "plain_load" .= contractValue staged
  , "candidate_dat_sha256" .= Image4.candidateSha256 candidate
  , "candidate_image4_sha256" .= Image4.candidateImage4Sha256 candidate
  ]

contractValue :: StagedImage4 -> Value
contractValue (StagedImage4 (CanonicalSource _ _ (SourceIdentities datIdentity imageIdentity))
    (LinkedArtifact linked linkedIdentity symbolIdentity (CodeAddress rust) proof) (DecodedImage4 staged)) =
  let CodeAddress start = origin
      CodeAddress end = limit
  in object
  [ "schema" .= (1 :: Int), "qualification" .= ("conditional-finite-host-admission" :: String)
  , "source_dat_image4_sha256" .= [identityText datIdentity,identityText imageIdentity]
  , "linked_sha256" .= identityText linkedIdentity, "symbols_sha256" .= identityText symbolIdentity
  , "linked_bytes" .= B.length linked, "staged_image4_sha256" .= identityText (identity staged)
  , "entry" .= start, "replaced_extent" .= [start,end], "rust_entry" .= rust
  , "scratch_low_from_entry_sp" .= scratchLow proof, "null_refusal_pc" .= refusalPc proof
  , "execution_pcs" .= executionPcs proof, "return_pcs" .= returnPcs proof
  , "call_sites" .= [object ["pc" .= pc,"target" .= target] | (pc,target) <- callSites proof]
  , "retained_calls" .= ([0x657114,0x6571a0,0x657184,0x6a8f1c,0x73b924,0x724b68,0x724c00,0x69b9c4] :: [Word32])
  , "caller_abi" .=
      ([ "AAPCS r4-r11 and incoming SP preserved; r0 is the saved read status in read mode and zero in nonzero mode"
       , "successful return restores LR0x6a992c; r1-r3,r12 and NZCV are volatile"
       , "canonical caller0x73b554 consumes r0 at0x73b558 before its next retained query"
       , "segment lookup6 precedes extent lookup even on the missing-segment nonreturning path"
       , "final segment length is loaded after retained calls; full-length progress is unconditional on read success"
       ] :: [String])
  , "open_premises" .=
      ([ "finite emitted-code admission is not natural loading, independent byte-publication acceptance or full universal equivalence"
       , "tagged nonaliasing segment/extent words; dependency provenance is not algebraic equality"
       , "unsampled scalar values and unreachable instructions are not universally quantified by the finite corpus"
       , "actual loader/task caller, aligned allocated live stack, immutable admitted code/literals and input tables"
       , "retained services preserve their admitted AAPCS and borrowed argument/request lifetimes, including complete callee stack usage"
       , "nonzero mode may synthesize progress without a transfer; progress, done and return do not prove committed image bytes"
       , "both-core guest/physical views, MPU/companion synchronization, aliases, TCM, callbacks and every writer remain separately bound"
       , "continuous exclusion of original code/copies, computed caller closure and reset/checkpoint invalidation remain runtime obligations"
       ] :: [String])
  ]
