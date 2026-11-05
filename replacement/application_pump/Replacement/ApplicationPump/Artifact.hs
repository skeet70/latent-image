{-# LANGUAGE OverloadedStrings #-}
module Replacement.ApplicationPump.Artifact
  ( CanonicalSource, LinkedArtifact, StagedImage5, StagedCandidate, ArtifactError (..)
  , canonicalSource, deriveArtifact, artifactProof, stageImage5, stagedBytes, contractValue
  , stageCandidate, candidateBytes, candidateContractValue
  ) where

import Control.Monad (unless)
import Data.Aeson (Value, object, (.=))
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import Data.List (isPrefixOf, nub)
import Data.Word (Word32)
import qualified Emulation.Image4Staging as Shared
import qualified Emulation.Sha256 as SHA
import qualified FujiAF.FirstStage as F
import qualified FujiAF.UpdateContainer as U
import Replacement.ApplicationPump.Admission
import qualified Replacement.ApplicationPump.Staging as Image5

newtype SourceIdentity = SourceIdentity SHA.Sha256
newtype CodeBytes = CodeBytes B.ByteString
data ArtifactError = SourceMismatch | ExtentRejected | SymbolRejected | StockContentsRejected | CodeRejected AdmissionError deriving (Eq, Show)
data CanonicalSource = CanonicalSource B.ByteString B.ByteString SourceIdentity SourceIdentity
data LinkedArtifact = LinkedArtifact CodeBytes SourceIdentity SourceIdentity FrameProof
data StagedImage5 = StagedImage5 CanonicalSource LinkedArtifact B.ByteString
data StagedCandidate = StagedCandidate StagedImage5 Image5.StagedCandidate

require :: Bool -> ArtifactError -> Either ArtifactError ()
require condition refusal = unless condition (Left refusal)

identity :: B.ByteString -> SourceIdentity
identity = SourceIdentity . SHA.sha256

identityText :: SourceIdentity -> String
identityText (SourceIdentity digest) = SHA.sha256Hex digest

canonicalSource :: B.ByteString -> B.ByteString -> Either ArtifactError CanonicalSource
canonicalSource dat image = do
  require (identityText (identity dat) == "43d67afa1a2bc9ae8481bc0acf02cb085ed0f515122dcbd26addcae102cd1490"
    && identityText (identity image) == "3cd501c07533938ae4d9347616986fd1649005a588c36b5abf05e36ef4fd892a") SourceMismatch
  _ <- either (Left . CodeRejected) Right (deriveFrameProof (B.take 32 (B.drop patchOffset image)))
  pure (CanonicalSource dat image (identity dat) (identity image))

patchOffset :: Int
patchOffset = 0xbfbd20 - 0xbf1a00

deriveArtifact :: B.ByteString -> B.ByteString -> Either ArtifactError LinkedArtifact
deriveArtifact bytes symbolBytes = do
  require (not (B.null bytes) && B.length bytes <= 32 && B.length bytes `mod` 4 == 0) ExtentRejected
  symbols <- either (const (Left SymbolRejected)) Right (F.parseSymbols symbolBytes)
  let names = map F.symbolName symbols
      end = 0xbfbd20 + fromIntegral (B.length bytes)
      required = [ ("__application_pump_start",0xbfbd20,0)
        , ("__application_pump_end",end,0)
        , ("xm1_application_pump",0xbfbd20,fromIntegral (B.length bytes))
        , ("xm1_application_prehook",0xbfb1b8,0)
        , ("xm1_application_normalizer",0xbfb20c,0)
        , ("xm1_application_posthook",0xbfb1e0,0) ]
      isMapping name = name == "$a" || "$a." `isPrefixOf` name
      valid symbol = case lookup (F.symbolName symbol) [(name,(address,size)) | (name,address,size) <- required] of
        Just (address,size) -> F.symbolAddress symbol == address && F.symbolSize symbol == size
        Nothing -> isMapping (F.symbolName symbol) && F.symbolSize symbol == 0
          && F.symbolAddress symbol >= 0xbfbd20 && F.symbolAddress symbol < end
          && F.symbolAddress symbol `mod` 4 == 0
      validKind row = case C.words row of
        [_,_,kind,name] -> if C.unpack name `elem` ["xm1_application_prehook","xm1_application_normalizer","xm1_application_posthook"]
          then kind == "A" else if C.unpack name == "xm1_application_pump" then kind == "T"
          else kind `elem` ["T","t","A"]
        _ -> False
  require (length names == length (nub names) && all valid symbols
    && all (\(name,_,_) -> name `elem` names) required
    && all validKind (filter (not . C.null) (C.lines symbolBytes))) SymbolRejected
  proof <- either (Left . CodeRejected) Right (deriveFrameProof bytes)
  pure (LinkedArtifact (CodeBytes bytes) (identity bytes) (identity symbolBytes) proof)

artifactProof :: LinkedArtifact -> FrameProof
artifactProof (LinkedArtifact _ _ _ proof) = proof

stageImage5 :: CanonicalSource -> LinkedArtifact -> Either ArtifactError StagedImage5
stageImage5 source@(CanonicalSource _ original _ _) artifact@(LinkedArtifact (CodeBytes linked) _ _ _) = do
  let padding = B.concat (replicate ((32 - B.length linked) `div` 4) (U.putWord32 0xe320f000))
      image = B.take patchOffset original <> linked <> padding <> B.drop (patchOffset+32) original
  require (image /= original) StockContentsRejected
  pure (StagedImage5 source artifact image)

stagedBytes :: StagedImage5 -> B.ByteString
stagedBytes (StagedImage5 _ _ bytes) = bytes

stageCandidate :: Shared.Tools -> Image5.ScratchDirectory -> StagedImage5 -> IO StagedCandidate
stageCandidate tools scratch staged@(StagedImage5 (CanonicalSource dat original _ _) _ image) = do
  candidate <- Image5.runImage5Staging tools scratch dat original image
  pure (StagedCandidate staged candidate)

candidateBytes :: StagedCandidate -> B.ByteString
candidateBytes (StagedCandidate _ candidate) = Image5.candidateDat candidate

candidateContractValue :: StagedCandidate -> Value
candidateContractValue (StagedCandidate staged candidate) = object
  [ "schema" .= (1 :: Int), "qualification" .= ("conditional-source-only" :: String)
  , "application_pump" .= contractValue staged
  , "candidate_dat_sha256" .= Image5.candidateSha256 candidate
  , "candidate_image5_sha256" .= Image5.candidateImage5Sha256 candidate
  ]

contractValue :: StagedImage5 -> Value
contractValue (StagedImage5 (CanonicalSource _ _ dat source) (LinkedArtifact (CodeBytes code) linked symbols proof) image) = object
  [ "schema" .= (1 :: Int), "qualification" .= ("conditional-source-only" :: String)
  , "source_dat_sha256" .= identityText dat, "source_image5_sha256" .= identityText source
  , "linked_sha256" .= identityText linked, "symbols_sha256" .= identityText symbols
  , "staged_image5_sha256" .= identityText (identity image)
  , "carrier" .= ([0xbfbd20,0xbfbd40] :: [Word32]), "emitted_bytes" .= B.length code
  , "carrier_sha256" .= identityText (identity (B.take 32 (B.drop patchOffset image)))
  , "image5_base" .= (0xbf1a00 :: Word32)
  , "entry" .= (0xbfbd20 :: Word32), "caller" .= (0xc23274 :: Word32)
  , "return" .= (0xc23278 :: Word32), "backedge" .= (0xc22f84 :: Word32)
  , "retained_calls" .= ([0xbfb1b8,0xbfb20c,0xbfb1e0] :: [Word32])
  , "call_sites" .= map callSiteValue (proofCallSites proof)
  , "return_pc" .= addressWord (returnInstruction proof)
  , "restored_registers" .= ([4..11] :: [Int])
  , "result" .= ("symbolic-normalizer-r0" :: String)
  , "scratch_low" .= scratchLow proof
  , "caller_abi" .= (["r0 is the symbolic normalizer result after a volatile-clobbering posthook"
      ,"incoming SP and r4-r11 restored; r1-r3,r12,LR,NZCV are caller-clobbered"
      ,"straight-line A32 only; no admitted cycles or implicit PC writes"] :: [String])
  , "open_premises" .= (["retained services return in A32 preserving r4-r11 and SP"
      ,"compiler frame and retained-call stack backing are mapped, aligned and live"
      ,"retained services and interrupts do not alias live saved words or require the original frame layout"
      ,"task1 context, initialization, readiness, executable publication and alias lifetime need runtime observation"
      ,"normalizer/controllers/settings/input/rendering remain retained; no persistence or hardware acceptance"] :: [String])
  ]

addressWord :: CodeAddress -> Word32
addressWord (CodeAddress address) = address

callSiteValue :: CallSite -> Value
callSiteValue site = object
  [ "phase" .= (case callPhase site of Prehook -> "prehook"; Normalizer -> "normalizer"; Posthook -> "posthook" :: String)
  , "pc" .= addressWord (callInstruction site), "target" .= addressWord (callTarget site)
  , "return" .= addressWord (callReturn site)
  , "sp_relative" .= (case callStack site of StackOffset offset -> offset)
  ]
