{-# LANGUAGE OverloadedStrings #-}
module Replacement.IdleTask.Artifact
  ( CanonicalSource, LinkedArtifact, StagedImage4, StagedCandidate, ArtifactError (..)
  , canonicalSource, deriveArtifact, artifactProof, artifactBytes
  , stageImage4, stagedBytes, contractValue
  , stageCandidate, candidateBytes, candidateContractValue
  ) where

import Control.Monad (unless)
import Data.Aeson (Value, object, (.=))
import Data.Bits ((.&.))
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import Data.Word (Word32)
import qualified Emulation.Image4Staging as Image4
import qualified Emulation.Sha256 as SHA
import qualified FujiAF.FirstStage as F
import qualified FujiAF.UpdateContainer as U
import Replacement.IdleTask.Admission

newtype SourceIdentity = SourceIdentity SHA.Sha256
newtype OriginalDat = OriginalDat B.ByteString
newtype DecodedImage4 = DecodedImage4 B.ByteString
data SourceIdentities = SourceIdentities SourceIdentity SourceIdentity
data ArtifactError = SourceMismatch | ExtentRejected | SymbolRejected | CodeRejected AdmissionError
  deriving (Eq, Show)
data CanonicalSource = CanonicalSource OriginalDat DecodedImage4 SourceIdentities
data LinkedArtifact = LinkedArtifact B.ByteString SourceIdentity SourceIdentity IdleProof
data StagedImage4 = StagedImage4 CanonicalSource LinkedArtifact DecodedImage4
data StagedCandidate = StagedCandidate StagedImage4 Image4.StagedCandidate

origin, limit :: Word32
origin = 0x69cbf0
limit = 0x69cc00

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
  original <- either (Left . CodeRejected) Right
    (deriveIdleProof (B.take 16 (B.drop (fromIntegral (origin-0x61c400)) image)))
  require (scratchLow original == -4 && wfiPc original == origin+8 && resumePc original == origin+12) SourceMismatch
  pure (CanonicalSource (OriginalDat dat) (DecodedImage4 image) (SourceIdentities datIdentity imageIdentity))

deriveArtifact :: B.ByteString -> B.ByteString -> Either ArtifactError LinkedArtifact
deriveArtifact bytes symbolBytes = do
  require (B.length bytes >= 8 && B.length bytes <= 16 && B.length bytes `mod` 4 == 0) ExtentRejected
  symbols <- either (const (Left SymbolRejected)) Right (F.parseSymbols symbolBytes)
  let lookupSymbol name = either (const (Left SymbolRejected)) Right (F.lookupSymbol symbols name)
  start <- lookupSymbol "__idle_task_start"
  end <- lookupSymbol "__idle_task_end"
  rust <- lookupSymbol "xm1_idle_task"
  require (F.symbolAddress start == origin && F.symbolAddress end == origin+fromIntegral (B.length bytes)
    && F.symbolAddress end <= limit && F.symbolSize start == 0 && F.symbolSize end == 0) ExtentRejected
  require (F.symbolAddress rust == origin && F.symbolSize rust == fromIntegral (B.length bytes)) SymbolRejected
  require (all (\symbol -> let address = F.symbolAddress symbol; size = F.symbolSize symbol
    in address .&. 3 == 0 && address >= origin && address <= F.symbolAddress end
      && toInteger address+toInteger size <= toInteger (F.symbolAddress end)) symbols) SymbolRejected
  require (all validKind (filter (not . C.null) (C.lines symbolBytes))) SymbolRejected
  proof <- either (Left . CodeRejected) Right (deriveIdleProof bytes)
  require (all (\symbol -> F.symbolSize symbol == 0 || F.symbolName symbol == "xm1_idle_task") symbols) SymbolRejected
  pure (LinkedArtifact bytes (identity bytes) (identity symbolBytes) proof)
  where
    validKind line = case C.words line of
      [_,_,kind,name] -> kind `elem` ["T","t","A","a","B","b","D","d","R","r"]
        && (name /= "xm1_idle_task" || kind == "T")
      _ -> False

artifactProof :: LinkedArtifact -> IdleProof
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
  [ "schema" .= (1 :: Int), "qualification" .= ("host-admitted-idle-task" :: String)
  , "idle_task" .= contractValue staged
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
  , "wfi_pc" .= wfiPc proof, "resume_pc" .= resumePc proof
  , "source_dat_image4_sha256" .= [identityText datIdentity,identityText imageIdentity]
  , "observable_contract" .=
      ([ "divergent WFI followed by an unconditional direct backedge to the sole WFI"
       , "incoming r0-r10,r12,LR and CPSR preserved; entry-only private SP/r11 frame permitted"
       , "no external calls, data accesses, MMIO, return, shared state or repeated stack effects"
       , "all registers, flags, live SP and private stack remain invariant across repeated waits"
       ] :: [String])
  , "open_premises" .=
      ([ "no runtime acceptance: host admission is not natural entry, wake, IRQ restoration or repeated-wait evidence"
       , "actual task/core identity, A32 SVC context and an aligned allocated live stack remain runtime obligations"
       , "the entry frame leaves at least32 bytes of the1024-byte task stack for the source IRQ frame; retained IRQ stack use is separate"
       , "immutable installed code and executable aliases, TCM, task resources and continuous attributed writes remain runtime obligations"
       , "physical interrupt behavior and root104/standby93 ownership are not established"
       ] :: [String])
  ]
