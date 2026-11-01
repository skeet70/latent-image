{-# LANGUAGE OverloadedStrings #-}
module Replacement.KernelInstall.Artifact
  ( CanonicalSource, LinkedArtifact, StagedImage4, StagedCandidate, ArtifactError (..)
  , canonicalSource, deriveArtifact, stageImage4, stagedBytes, contractValue
  , stageCandidate, candidateBytes, candidateContractValue
  , linkedBytes, installationProof, artifactValue
  ) where

import Control.Monad (unless)
import Data.Aeson (Value, object, (.=))
import Data.Aeson.Types (Pair)
import Data.Bits ((.&.))
import qualified Data.ByteString as B
import Data.Word (Word32)
import qualified Emulation.Sha256 as SHA
import qualified Emulation.Image4Staging as Image4
import qualified FujiAF.FirstStage as F
import qualified FujiAF.UpdateContainer as U
import Replacement.KernelInstall.Admission

newtype SourceIdentity = SourceIdentity SHA.Sha256
newtype CodeAddress = CodeAddress Word32
data ArtifactError = SourceMismatch | ExtentRejected | SymbolRejected | CodeRejected AdmissionError deriving (Eq, Show)
data CanonicalSource = CanonicalSource B.ByteString B.ByteString [SourceIdentity]
data LinkedArtifact = LinkedArtifact B.ByteString SourceIdentity SourceIdentity CodeAddress InstallationProof
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
  require (U.word32At loaderA 0x10 == 0x4004dae0 && U.word32At loaderB 0x10 == 0x400ddaa0
    && U.word32At image (0x7caf3c-0x61c400) == 3) SourceMismatch
  pure (CanonicalSource dat image identities)

deriveArtifact :: B.ByteString -> B.ByteString -> Either ArtifactError LinkedArtifact
deriveArtifact bytes symbolBytes = do
  symbols <- either (const (Left SymbolRejected)) Right (F.parseSymbols symbolBytes)
  let at name = either (const (Left SymbolRejected)) (Right . F.symbolAddress) (F.lookupSymbol symbols name)
  start <- at "__kernel_install_start"
  end <- at "__kernel_install_end"
  entry <- at "xm1_kernel_install_entry"
  rust <- at "xm1_kernel_install"
  require (start == 0x725a04 && end > start && end <= 0x725b54 && end .&. 3 == 0
    && toInteger (B.length bytes) == toInteger end-toInteger start) ExtentRejected
  require (entry == start && rust > start && rust < end && rust .&. 3 == 0) SymbolRejected
  proof <- either (Left . CodeRejected) Right (deriveInstallationProof bytes)
  pure (LinkedArtifact bytes (identity bytes) (identity symbolBytes) (CodeAddress rust) proof)

stageImage4 :: CanonicalSource -> LinkedArtifact -> StagedImage4
stageImage4 source@(CanonicalSource _ original _) artifact@(LinkedArtifact linked _ _ _ _) =
  let offset = 0x725a04-0x61c400
      padding = B.concat (replicate ((336-B.length linked) `div` 4) (U.putWord32 0xe320f000))
      staged = B.take offset original <> linked <> padding <> B.drop (offset+336) original
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
  [ "schema" .= (1 :: Int), "qualification" .= ("host-admitted-installation" :: String)
  , "kernel_install" .= contractValue staged
  , "candidate_dat_sha256" .= Image4.candidateSha256 candidate
  , "candidate_image4_sha256" .= Image4.candidateImage4Sha256 candidate
  ]

linkedBytes :: LinkedArtifact -> B.ByteString
linkedBytes (LinkedArtifact bytes _ _ _ _) = bytes

installationProof :: LinkedArtifact -> InstallationProof
installationProof (LinkedArtifact _ _ _ _ proof) = proof

artifactValue :: LinkedArtifact -> Value
artifactValue = object . artifactFields

contractValue :: StagedImage4 -> Value
contractValue (StagedImage4 (CanonicalSource _ _ sources) artifact staged) = object
  (artifactFields artifact ++
    [ "source_dat_image4_loader_a_loader_b_sha256" .= map identityText sources
    , "staged_image4_sha256" .= identityText (identity staged)
    ])

artifactFields :: LinkedArtifact -> [Pair]
artifactFields (LinkedArtifact linked linkedIdentity symbols (CodeAddress rust) proof) =
  [ "schema" .= (1 :: Int), "qualification" .= ("conditional-host-admission" :: String)
  , "linked_sha256" .= identityText linkedIdentity, "symbols_sha256" .= identityText symbols
  , "linked_bytes" .= B.length linked
  , "patch" .= ([0x725a04,0x725b54] :: [Word32]), "rust_entry" .= rust
  , "scratch_low_from_installer_sp" .= scratchLow proof, "invalid_marker_loop" .= refusalPc proof
  , "execution_pcs" .= executionPcs proof, "retained_calls" .= ([0x42634] :: [Word32])
  , "nonstack_word_stores" .= (1457 :: Int), "destination_bytes" .= (5828 :: Int)
  , "caller_abi" .=
      ([ "r4-r12, r11 and incoming SP preserved on success; r0=0, r1=r3=0x40a0fc2c, r2=0, LR=0x725a40, NZCV=0110"
       , "invalid marker is every word other than3; roots/cache precede rejection and local stores do not execute"
       , "only NZCV flag updates are admitted; CPSR control, Q and GE remain unchanged"
       ] :: [String])
  , "open_premises" .=
      ([ "actual natural caller, aligned live scratch, immutable installed code and literals"
       , "both-core mapping, TCM, alias and non-CPU writer exclusions"
       , "retained cache service0x42634 returns under its pinned source contract"
       , "root publication is not secondary initializer completion or kernel-service ownership"
       , "optional BSS clear, initialized cursors/callbacks, wrapper records and pool lifetimes remain separate"
       ] :: [String])
  ]
