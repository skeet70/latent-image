{-# LANGUAGE OverloadedStrings #-}
module Replacement.TaskSubmission.Artifact
  ( CanonicalSource, LinkedArtifact, StagedImage4, StagedCandidate, ArtifactError (..)
  , canonicalSource, deriveArtifact, linkedBytes, frameProof
  , stageImage4, stagedBytes, contractValue, stageCandidate, candidateBytes, candidateStagedImage4, candidateContractValue
  ) where

import Control.Monad (forM, unless)
import qualified A32 as A
import Data.Aeson (Value, object, (.=))
import Data.Bits ((.&.))
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import Data.List (nub)
import Data.Word (Word32)
import qualified Emulation.Image4Staging as Image4
import qualified Emulation.Sha256 as SHA
import qualified FujiAF.FirstStage as F
import Replacement.TaskSubmission.Admission

newtype SourceIdentity = SourceIdentity SHA.Sha256
data SourceIdentities = SourceIdentities SourceIdentity SourceIdentity SourceIdentity SourceIdentity
data CanonicalSource = CanonicalSource B.ByteString B.ByteString SourceIdentities
data LinkedArtifact = LinkedArtifact B.ByteString SourceIdentity SourceIdentity FrameProof
data StagedImage4 = StagedImage4 CanonicalSource LinkedArtifact B.ByteString
data StagedCandidate = StagedCandidate StagedImage4 Image4.StagedCandidate
data ArtifactError = SourceMismatch | ExtentRejected | SymbolRejected | DependencyRejected | CodeRejected AdmissionError deriving (Eq, Show)

require :: Bool -> ArtifactError -> Either ArtifactError ()
require premise failure = unless premise (Left failure)
identity :: B.ByteString -> SourceIdentity
identity = SourceIdentity . SHA.sha256
identityText :: SourceIdentity -> String
identityText (SourceIdentity digest) = SHA.sha256Hex digest

canonicalSource :: B.ByteString -> B.ByteString -> B.ByteString -> B.ByteString -> Either ArtifactError CanonicalSource
canonicalSource dat image loaderA loaderB = do
  let sources@(SourceIdentities datIdentity imageIdentity loaderAIdentity loaderBIdentity) =
        SourceIdentities (identity dat) (identity image) (identity loaderA) (identity loaderB)
      identities = [datIdentity,imageIdentity,loaderAIdentity,loaderBIdentity]
  require (map identityText identities ==
    [ "43d67afa1a2bc9ae8481bc0acf02cb085ed0f515122dcbd26addcae102cd1490"
    , "f2e2acab892dafeed85171f6870a411f0a2d5eae10921b86a24630d401dcff16"
    , "21366ffff7cebc6176b8a8125bf3a9974835a51070a58ea325f7eed8aabd8882"
    , "8fb0c322a22443a3783cdab9071db17212a5629f6375646218ac1e043dc672ec"
    ]) SourceMismatch
  pure (CanonicalSource dat image sources)

deriveArtifact :: B.ByteString -> B.ByteString -> Either ArtifactError LinkedArtifact
deriveArtifact bytes symbolBytes = do
  require (B.length bytes == 852) ExtentRejected
  symbols <- either (const (Left SymbolRejected)) Right (F.parseSymbols symbolBytes)
  require (length (nub (map F.symbolName symbols)) == length symbols) SymbolRejected
  let symbol name = either (const (Left SymbolRejected)) Right (F.lookupSymbol symbols name)
      at name = F.symbolAddress <$> symbol name
      exact name value = at name >>= \address -> require (address == value) SymbolRejected
      rows = filter (not . C.null) (C.lines symbolBytes)
      textSymbols = [entry | (entry,row) <- zip symbols rows, C.words row !! 2 `elem` ["T","t"], F.symbolSize entry > 0]
  exact "__task_submission_start" 0x725dd8
  exact "__task_submission_end" 0x72612c
  exact "xm1_task_submit_entry" 0x725dd8
  exact "xm1_task_release_entry" 0x726098
  exact "xm1_task_release_lock_literal" 0x726128
  mapM_ (\(name,size) -> symbol name >>= \entry -> require (F.symbolSize entry == size) SymbolRejected)
    [("xm1_task_submit_entry",4),("xm1_task_release_entry",12),("xm1_task_release_lock_literal",4)]
  require (all (`elem` map F.symbolName textSymbols)
    ["xm1_task_submit_entry","xm1_task_release_entry","xm1_task_submit","xm1_task_release"]) SymbolRejected
  bodiesEnd <- at "__task_submission_bodies_end"
  require (bodiesEnd > 0x725dd8 && bodiesEnd <= 0x726098 && bodiesEnd .&. 3 == 0) ExtentRejected
  mapM_ (\name -> do
    address <- at name
    require (address > 0x725dd8 && address < bodiesEnd && address .&. 3 == 0) SymbolRejected) ["xm1_task_submit","xm1_task_release"]
  mapM_ (\(name,expected) -> at ("xm1_task_" ++ name) >>= \address -> require (address == expected) DependencyRejected)
    [("critical_enter",0x7235d8),("critical_exit",0x7236b0),("reservation",0x725c6c)
    ,("fill",0x7221c8),("cache",0x42634),("log",0x41a34),("error_callback_dispatch",0x723398)
    ,("try_lock",0x427d4),("unlock",0x427fc),("class_table",0x7cad9c),("interface_roots",0x96bd20)
    ,("wrapper_base",0x40a5b204),("state_base",0x40a0fc44),("error_word",0x40a0f82c),("release_lock",0x409c280c)]
  extents <- forM textSymbols $ \entry -> do
    let start = F.symbolAddress entry
        stop = toInteger start+toInteger (F.symbolSize entry)
    require (start .&. 3 == 0 && F.symbolSize entry .&. 3 == 0 &&
      ((start >= 0x725dd8 && stop <= toInteger bodiesEnd) || (start == 0x726098 && stop == 0x7260a4))) SymbolRejected
    pure (CodeExtent (CodeAddress start) (CodeAddress (fromInteger stop)))
  proof <- either (Left . CodeRejected) Right (deriveFrameProof (NativeLayout extents) bytes)
  pure (LinkedArtifact bytes (identity bytes) (identity symbolBytes) proof)

linkedBytes :: LinkedArtifact -> B.ByteString
linkedBytes (LinkedArtifact bytes _ _ _) = bytes
frameProof :: LinkedArtifact -> FrameProof
frameProof (LinkedArtifact _ _ _ proof) = proof

stageImage4 :: CanonicalSource -> LinkedArtifact -> StagedImage4
stageImage4 source@(CanonicalSource _ original _) artifact =
  let offset = 0x725dd8-0x61c400
      staged = B.take offset original <> linkedBytes artifact <> B.drop (offset+852) original
  in StagedImage4 source artifact staged
stagedBytes :: StagedImage4 -> B.ByteString
stagedBytes (StagedImage4 _ _ bytes) = bytes

stageCandidate :: Image4.Tools -> Image4.ScratchDirectory -> StagedImage4 -> IO StagedCandidate
stageCandidate tools scratch staged@(StagedImage4 (CanonicalSource dat _ _) _ image) = do
  candidate <- Image4.runImage4Staging tools scratch dat dat image
  pure (StagedCandidate staged candidate)
candidateBytes :: StagedCandidate -> B.ByteString
candidateBytes (StagedCandidate _ candidate) = Image4.candidateDat candidate
candidateStagedImage4 :: StagedCandidate -> StagedImage4
candidateStagedImage4 (StagedCandidate staged _) = staged
candidateContractValue :: StagedCandidate -> Value
candidateContractValue (StagedCandidate staged candidate) = object
  [ "schema" .= (1 :: Int), "qualification" .= ("host-admitted-owned-task-submission" :: String)
  , "task_submission" .= contractValue staged
  , "candidate_dat_sha256" .= Image4.candidateSha256 candidate
  , "candidate_image4_sha256" .= Image4.candidateImage4Sha256 candidate
  ]

contractValue :: StagedImage4 -> Value
contractValue (StagedImage4 (CanonicalSource _ _ (SourceIdentities dat image loaderA loaderB)) (LinkedArtifact linked linkedIdentity symbolIdentity proof) staged) = object
  [ "schema" .= (1 :: Int), "qualification" .= ("conditional-owned-task-submission-admission" :: String)
  , "source_dat_image4_loader_a_loader_b_sha256" .= map identityText [dat,image,loaderA,loaderB]
  , "linked_sha256" .= identityText linkedIdentity, "symbols_sha256" .= identityText symbolIdentity
  , "linked_bytes" .= B.length linked, "patch" .= ([0x725dd8,0x72612c] :: [Word32])
  , "staged_image4_sha256" .= identityText (identity staged)
  , "frames" .= [object ["entry" .= show entry,"low_from_entry_sp" .= offset (scratchLow proof entry)] | entry <- [Submission,Release]]
  , "calls" .= [object ["entry" .= show (callEntry boundary),"instruction" .= address (callInstruction boundary)
      , "continuation" .= address (callContinuation boundary),"target" .= show (callTarget boundary)
      , "target_register" .= fmap (\(A.Reg register) -> register) (callTargetRegister boundary)
      , "r0_provenance" .= show (callR0 boundary),"r1_provenance" .= show (callR1 boundary)
      , "parameter_words" .= [object ["offset" .= offset (parameterOffset word),"provenance" .= show (parameterProvenance word)] | word <- callParameters boundary]
      , "actual_sp_from_entry" .= offset (callStack boundary),"caller_live_words" .= map offset (callLiveWords boundary)] | boundary <- callBoundaries proof]
  , "execution_pcs" .= map address (executionPcs proof)
  , "global_writes" .= map address (globalWrites proof)
  , "open_premises" .=
      ([ "qualification joins nested retained or owned #142/#143 demands at each actual call SP, using actual writable backing and disjoint live objects"
       , "native own-frame bounds are not a composed-stack proof; use joinStack with all reached callee demands"
       , "guest descriptor, tables, roots, state, callback and code lifetimes and aliases remain runtime obligations"
       , "external calls preserve SP and r4-r11; release adapter also preserves incoming r3"
       , "bounded natural and controlled qualification does not retire all unconditional callers"
       ] :: [String])
  ]
  where offset (StackOffset value) = value
        address (CodeAddress value) = value
