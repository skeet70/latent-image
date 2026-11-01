{-# LANGUAGE OverloadedStrings #-}
module Replacement.StartupDispatch.Artifact
  ( CanonicalSource, LinkedArtifact, StagedImage4, StagedCandidate, ArtifactError (..)
  , canonicalSource, deriveArtifact, stageImage4, stagedBytes, contractValue
  , stageCandidate, candidateBytes, candidateContractValue
  ) where

import Control.Monad (forM_, unless)
import Data.Aeson (Value, object, (.=))
import Data.Bits ((.&.))
import qualified Data.ByteString as B
import Data.Word (Word32)
import qualified Emulation.Sha256 as SHA
import qualified Emulation.Image4Staging as Image4
import qualified FujiAF.FirstStage as F
import qualified FujiAF.UpdateContainer as U
import Replacement.StartupDispatch.Admission

newtype SourceIdentity = SourceIdentity SHA.Sha256
newtype CodeAddress = CodeAddress Word32
data ArtifactError = SourceMismatch | ExtentRejected | SymbolRejected | CodeRejected AdmissionError deriving (Eq, Show)
data CanonicalSource = CanonicalSource B.ByteString B.ByteString [SourceIdentity]
data LinkedArtifact = LinkedArtifact B.ByteString SourceIdentity SourceIdentity CodeAddress FrameProof
data StagedImage4 = StagedImage4 CanonicalSource LinkedArtifact B.ByteString
data StagedCandidate = StagedCandidate StagedImage4 Image4.StagedCandidate

require :: Bool -> ArtifactError -> Either ArtifactError ()
require condition failure = unless condition (Left failure)

identity :: B.ByteString -> SourceIdentity
identity = SourceIdentity . SHA.sha256

identityText :: SourceIdentity -> String
identityText (SourceIdentity digest) = SHA.sha256Hex digest

canonicalSource :: B.ByteString -> B.ByteString -> B.ByteString -> B.ByteString -> Either ArtifactError CanonicalSource
canonicalSource dat image loaderA loaderB = do
  let identities = map identity [dat,image,loaderA,loaderB]
  require (map identityText identities ==
    [ "43d67afa1a2bc9ae8481bc0acf02cb085ed0f515122dcbd26addcae102cd1490"
    , "f2e2acab892dafeed85171f6870a411f0a2d5eae10921b86a24630d401dcff16"
    , "21366ffff7cebc6176b8a8125bf3a9974835a51070a58ea325f7eed8aabd8882"
    , "8fb0c322a22443a3783cdab9071db17212a5629f6375646218ac1e043dc672ec"
    ]) SourceMismatch
  forM_ [loaderA,loaderB] $ \loader -> do
    require (map (U.word32At loader) [0x9f8,0x9fc,0xa30,0xa34,0xa38,0xa3c,0xa64,0xa68,0xae8,0xaec] ==
      [0xe3a00000,0xe12fff34,0xe1a00005,0xe12fff34,0xe3a00003,0xe12fff34,0xe3a00002,0xe12fff34,0xe3a00004,0xe12fff34]) SourceMismatch
  sourceProof <- either (Left . CodeRejected) Right (deriveFrameProof (B.take 396 (B.drop (0x69cdf8-0x61c400) image)))
  require (scratchLow sourceProof == -48) SourceMismatch
  pure (CanonicalSource dat image identities)

deriveArtifact :: B.ByteString -> B.ByteString -> Either ArtifactError LinkedArtifact
deriveArtifact bytes symbolBytes = do
  symbols <- either (const (Left SymbolRejected)) Right (F.parseSymbols symbolBytes)
  let at name = either (const (Left SymbolRejected)) (Right . F.symbolAddress) (F.lookupSymbol symbols name)
  start <- at "__startup_dispatch_start"
  end <- at "__startup_dispatch_end"
  entry <- at "xm1_startup_dispatch_entry"
  rust <- at "xm1_startup_dispatch"
  require (start == 0x69cdf8 && end > start && end <= 0x69cf84 && end .&. 3 == 0
    && toInteger (B.length bytes) == toInteger end - toInteger start) ExtentRejected
  require (entry == start && rust >= start + 28 && rust < end && rust .&. 3 == 0) SymbolRejected
  proof <- either (Left . CodeRejected) Right (deriveFrameProof bytes)
  pure (LinkedArtifact bytes (identity bytes) (identity symbolBytes) (CodeAddress rust) proof)

stageImage4 :: CanonicalSource -> LinkedArtifact -> StagedImage4
stageImage4 source@(CanonicalSource _ original _) artifact@(LinkedArtifact linked _ _ _ _) =
  let offset = 0x69cdf8 - 0x61c400
      padding = B.concat (replicate ((396 - B.length linked) `div` 4) (U.putWord32 0xe320f000))
      staged = B.take offset original <> linked <> padding <> B.drop (offset + 396) original
  in StagedImage4 source artifact staged

stagedBytes :: StagedImage4 -> B.ByteString
stagedBytes (StagedImage4 _ _ bytes) = bytes

stageCandidate :: Image4.Tools -> Image4.ScratchDirectory -> StagedImage4 -> IO StagedCandidate
stageCandidate tools scratch staged@(StagedImage4 (CanonicalSource original _ _) _ image) = do
  candidate <- Image4.runImage4Staging tools scratch original original image
  pure (StagedCandidate staged candidate)

candidateBytes :: StagedCandidate -> B.ByteString
candidateBytes (StagedCandidate _ candidate) = Image4.candidateDat candidate

candidateContractValue :: StagedCandidate -> Value
candidateContractValue (StagedCandidate staged candidate) = object
  [ "schema" .= (1 :: Int), "qualification" .= ("conditional-source-only" :: String)
  , "startup_dispatch" .= contractValue staged
  , "candidate_dat_sha256" .= Image4.candidateSha256 candidate
  , "candidate_image4_sha256" .= Image4.candidateImage4Sha256 candidate
  ]

contractValue :: StagedImage4 -> Value
contractValue (StagedImage4 (CanonicalSource _ _ sources) (LinkedArtifact _ linked symbols (CodeAddress rust) proof) staged) = object
  [ "schema" .= (1 :: Int), "qualification" .= ("conditional-source-only" :: String)
  , "source_dat_image4_loader_a_loader_b_sha256" .= map identityText sources
  , "linked_sha256" .= identityText linked, "symbols_sha256" .= identityText symbols
  , "staged_image4_sha256" .= identityText (identity staged)
  , "patch" .= ([0x69cdf8,0x69cf84] :: [Word32]), "rust_entry" .= rust
  , "incoming_sp_relative" .= object
      [ "guest_frame_bottom" .= (-48 :: Int), "descriptor" .= (-44 :: Int)
      , "unknown_word" .= (-20 :: Int), "compiler_and_adapter_low" .= scratchLow proof
      , "assigned_offsets" .= ([0,4,8,12,16,20] :: [Int]) ]
  , "retained_calls" .= ([0x657184,0x7221a0,0x725a04,0x725dd8] :: [Word32])
  , "conditional_paths" .= length (proofPaths proof)
  , "caller_abi" .=
      ([ "A/B startup caller offsets 9fc,a34,a3c,a68,aec; image entry stays in r4"
       , "r4-r11 and incoming SP preserved; phase2 r0 consumed as callback, other r0 discarded"
       , "r1-r3,r12,LR,NZCV are dead at these caller boundaries, not exact private-ABI equivalence"
       , "owned instructions do not change CPSR control/Q/GE; retained-call CPSR effects remain retained"
       ] :: [String])
  , "open_premises" .=
      ([ "retained calls return in A32 preserving r4-r11 and SP, with the declared r0 outcome classes"
       , "actual compiler scratch and retained-call stack backing are mapped, aligned and live"
       , "retained writes, interrupt activity and aliases exclude saved registers, scratch and executing code"
       , "descriptor D+24 has guest word-access semantics; no Rust reference covers it"
       , "phase4 retains the same D; retained alias writes to D+24 are neither read nor constrained by owned code"
       , "conditional clear mapping/effects, setup installation, descriptor consumption and descriptor lifetime"
       , "task submission status does not establish creation, activation, scheduling or execution"
       , "callback publication does not qualify diagnostics, image5 callbacks or restored-PC capabilities"
       , "natural A/B routes, physical backing, aliases, concurrency and consumer-visible runtime rejection"
       ] :: [String])
  ]
