{-# LANGUAGE OverloadedStrings #-}
module Replacement.LoaderOrchestration.Artifact
  ( CanonicalSource, LinkedArtifact, StagedCandidate, ArtifactError(..)
  , canonicalSource, deriveArtifact, stageCandidate, candidateBytes, contractValue
  , linkedBytes, linkedProof, sourceLoader, candidateLoader
  ) where

import Control.Monad (forM, unless)
import Data.Aeson (Value, object, (.=))
import Data.Bits ((.&.))
import qualified Data.ByteString as B
import Data.Word (Word32)
import qualified Emulation.Sha256 as SHA
import qualified FujiAF.FirstStage as F
import qualified FujiAF.UpdateContainer as U
import qualified Replacement.StartupDispatch.Admission as D
import Replacement.LoaderOrchestration.Admission
import Replacement.LoaderOrchestration.Interface

newtype ContentDigest = ContentDigest SHA.Sha256
newtype LinkedSymbols = LinkedSymbols B.ByteString
data CanonicalSource = CanonicalSource B.ByteString U.Parsed B.ByteString B.ByteString B.ByteString D.FrameProof
data LinkedArtifact = LinkedArtifact LoaderSide B.ByteString LinkedSymbols CodeAddress FrameProof
data StagedCandidate = StagedCandidate CanonicalSource LinkedArtifact LinkedArtifact B.ByteString B.ByteString B.ByteString
data ArtifactError = SourceMismatch | ContainerRejected | SymbolRejected | SideRejected | CodeRejected AdmissionError deriving (Eq, Show)

require :: Bool -> ArtifactError -> Either ArtifactError ()
require premise failure = unless premise (Left failure)
identity :: B.ByteString -> ContentDigest
identity = ContentDigest . SHA.sha256
identityText :: ContentDigest -> String
identityText (ContentDigest digest) = SHA.sha256Hex digest

canonicalSource :: B.ByteString -> B.ByteString -> B.ByteString -> B.ByteString -> Either ArtifactError CanonicalSource
canonicalSource dat image4 loaderA loaderB = do
  require (map (identityText . identity) [dat,image4,loaderA,loaderB] ==
    [ "43d67afa1a2bc9ae8481bc0acf02cb085ed0f515122dcbd26addcae102cd1490"
    , "f2e2acab892dafeed85171f6870a411f0a2d5eae10921b86a24630d401dcff16"
    , "21366ffff7cebc6176b8a8125bf3a9974835a51070a58ea325f7eed8aabd8882"
    , "8fb0c322a22443a3783cdab9071db17212a5629f6375646218ac1e043dc672ec"
    ]) SourceMismatch
  require (U.word32At image4 12==0x69cdf8) SourceMismatch
  parsed <- either (const (Left ContainerRejected)) Right (U.parseContainer dat)
  provider <- either (const (Left SourceMismatch)) Right (D.deriveFrameProof (B.take 396 (B.drop (0x69cdf8-0x61c400) image4)))
  require (D.scratchLow provider==(-48)) SourceMismatch
  pure (CanonicalSource dat parsed image4 loaderA loaderB provider)

deriveArtifact :: LoaderSide -> B.ByteString -> B.ByteString -> Either ArtifactError LinkedArtifact
deriveArtifact side bytes symbols = do
  parsed <- either (const (Left SymbolRejected)) Right (F.parseSymbols symbols)
  let at name = either (const (Left SymbolRejected)) (Right . F.symbolAddress) (F.lookupSymbol parsed name)
      CodeAddress first = carrierStart side
  start <- at "__loader_orchestration_start"
  end <- at "__loader_orchestration_end"
  entry <- at "xm1_loader_orchestration_entry"
  compiler <- at "xm1_loader_orchestration"
  require (start==first&&entry==first&&end>first&&end<=first+340&&end .&. 3==0&&toInteger (B.length bytes)==toInteger end-toInteger first) SymbolRejected
  proof <- either (Left . CodeRejected) Right (deriveFrameProof side bytes (CodeAddress compiler))
  pure (LinkedArtifact side bytes (LinkedSymbols symbols) (CodeAddress compiler) proof)

linkedBytes :: LinkedArtifact -> B.ByteString
linkedBytes (LinkedArtifact _ bytes _ _ _) = bytes
linkedProof :: LinkedArtifact -> FrameProof
linkedProof (LinkedArtifact _ _ _ _ proof) = proof
sourceLoader :: CanonicalSource -> LoaderSide -> B.ByteString
sourceLoader (CanonicalSource _ _ _ loaderA loaderB _) side = case side of LoaderA -> loaderA; LoaderB -> loaderB
candidateLoader :: StagedCandidate -> LoaderSide -> B.ByteString
candidateLoader (StagedCandidate _ _ _ _ loaderA loaderB) side = case side of LoaderA -> loaderA; LoaderB -> loaderB
candidateBytes :: StagedCandidate -> B.ByteString
candidateBytes (StagedCandidate _ _ _ bytes _ _) = bytes

padded :: LinkedArtifact -> B.ByteString
padded artifact = let bytes=linkedBytes artifact in bytes<>B.concat (replicate ((340-B.length bytes) `div` 4) (U.putWord32 0xe320f000))

patches :: CanonicalSource -> LinkedArtifact -> Either ArtifactError [F.LoaderWordPatch]
patches source artifact@(LinkedArtifact side _ _ _ _) = do
  let original = sourceLoader source side
      replacement = padded artifact
      CodeAddress start = carrierStart side
      loaderId = F.LoaderId (case side of LoaderA -> 0; LoaderB -> 1)
      values = [(offset,U.word32At original (0x9ac+offset),U.word32At replacement offset) | offset <- [0,4..336]]
  forM [(offset,before,after) | (offset,before,after) <- values,before/=after] $ \(offset,before,after) ->
    either (const (Left ContainerRejected)) Right (F.loaderWordPatch loaderId (start+fromIntegral offset) before after)

stageCandidate :: CanonicalSource -> LinkedArtifact -> LinkedArtifact -> Either ArtifactError StagedCandidate
stageCandidate source@(CanonicalSource _ parsed _ _ _ _) a@(LinkedArtifact sideA _ _ _ _) b@(LinkedArtifact sideB _ _ _ _) = do
  require (sideA==LoaderA&&sideB==LoaderB) SideRejected
  aPatches <- patches source a
  bPatches <- patches source b
  let changes = aPatches++bPatches
      original = U.parsedParts parsed
  stagedParts <- either (const (Left ContainerRejected)) Right (F.applyLoaderWordPatches changes original)
  staged <- either (const (Left ContainerRejected)) Right (U.rebuildContainer parsed stagedParts)
  consumed <- either (const (Left ContainerRejected)) Right (U.parseContainer staged)
  either (const (Left ContainerRejected)) Right (F.verifyPreTablePatches [] changes parsed consumed)
  let loader side artifact = let bytes=sourceLoader source side in B.take 0x9ac bytes<>padded artifact<>B.drop 0xb00 bytes
  pure (StagedCandidate source a b staged (loader LoaderA a) (loader LoaderB b))

addressWord :: CodeAddress -> Word32
addressWord (CodeAddress value) = value

contractValue :: StagedCandidate -> Value
contractValue candidate@(StagedCandidate source@(CanonicalSource dat _ image4 loaderA loaderB provider) a b _ _ _) = object
  [ "schema" .= (1::Int), "qualification" .= ("conditional-source-only"::String)
  , "source_dat_sha256" .= identityText (identity dat)
  , "source_image4_sha256" .= identityText (identity image4)
  , "source_loader_a_sha256" .= identityText (identity loaderA)
  , "source_loader_b_sha256" .= identityText (identity loaderB)
  , "candidate_dat_sha256" .= identityText (identity (candidateBytes candidate))
  , "dispatcher_provider" .= object
      [ "role" .= ("CanonicalRetained"::String), "entry" .= (0x69cdf8::Word32)
      , "source_image4_sha256" .= identityText (identity image4)
      , "local_stack_low_from_service_sp" .= D.scratchLow provider
      , "local_stack_low_from_carrier_entry" .= (D.scratchLow provider-16)
      , "caller_live_stack" .= ([-16,0]::[Int]), "caller_live_scratch_below_service_sp" .= ([]::[Int]) ]
  , "loaders" .= map loaderValue [a,b]
  , "open_premises" .=
      ([ "The actual retained service providers and their mapped stack/resource footprints remain admitted at the original SP/FP."
       , "The carrier has no live Rust stack storage below its sixteen-byte frame. Transitive retained depth is not the dispatcher's local bound."
       , "An owned dispatcher or installer requires its actual joined provider proof; standalone qualification is not inherited."
       , "Initial selected task records, returned callback backing, IRQ activity and shared-resource lifetime remain separate runtime premises."
       , "Diagnostic absence is bounded to the observed natural route, not global restored or computed callers." ]::[String])
  ]
  where
    loaderValue (LinkedArtifact side bytes (LinkedSymbols symbols) compiler proof) =
      let CodeAddress start = carrierStart side
          boundary (site,target) = case target of
            Retained service -> object ["pc" .= addressWord site,"kind" .= ("Retained"::String),"service" .= show service,"target" .= addressWord (serviceAddress side service)]
            Dispatcher -> object ["pc" .= addressWord site,"kind" .= ("Dispatcher"::String),"target" .= (0x69cdf8::Word32)]
          path (PathProof (Scenario role completion setup) effects stopped) = object
            [ "role" .= show role,"completion" .= show completion,"setup" .= show setup
            , "terminal" .= show stopped,"phases" .= [show selected | DispatchCall selected <- effects] ]
      in object
        [ "side" .= show side,"patch" .= [start,start+340],"linked_sha256" .= identityText (identity bytes)
        , "symbols_sha256" .= identityText (identity symbols),"linked_bytes" .= B.length bytes
        , "staged_loader_sha256" .= identityText (identity (candidateLoader candidate side))
        , "compiler_entry" .= addressWord compiler,"body_entry" .= (addressWord compiler+8)
        , "frame_bytes" .= (16::Int),"service_sp_from_entry" .= (-16::Int),"service_fp_from_entry" .= (-4::Int)
        , "execution_pcs" .= map addressWord (executionPcs proof),"calls" .= map boundary (callBoundaries proof)
        , "paths" .= map path (proofPaths proof),"source_loader_sha256" .= identityText (identity (sourceLoader source side)) ]
