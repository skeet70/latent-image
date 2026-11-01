module Emulation.LoaderPrefixArtifactChecks
  ( ArtifactInput (..)
  , SymbolDocument (..)
  , checkNaturalArtifact
  , checkRefusalArtifact
  , checkNegativeArtifact
  , checkInterleavedTailArtifact
  ) where

import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as BC
import Data.Bits (xor)
import Data.Word (Word32)
import Emulation.FirstStageWitness (encodeLeWord, leWordAt)
import Emulation.LoaderPrefixReceiptFixtures
  ( RecordByteClass (..), accepted, checkProjectionAdmission, expect )
import Emulation.LoaderPrefixWitness
  ( Guest (..), Polling (..), ProgramFacts (..), Scenario (..), StagedImages (..), deriveOwnedFacts, deriveScenarioOwnedFacts )
import qualified FujiAF.FirstStage as F
import qualified FujiAF.UpdateContainer as U
import qualified StartupStageContract as Stage

newtype SymbolDocument = SymbolDocument B.ByteString

data ArtifactInput = ArtifactInput U.MediaImage SymbolDocument

symbolAt :: SymbolDocument -> String -> IO Word32
symbolAt (SymbolDocument symbols) name = either fail pure (F.parseSymbols symbols >>= fmap F.symbolAddress . flip F.lookupSymbol name)

-- | Record 2 holding the image at 0x10400 and loader B publishing @entry@.
stagedFor :: B.ByteString -> Word32 -> StagedImages
stagedFor bin entry =
  StagedImages
    (B.replicate 0x400 0 <> bin <> B.replicate (0x4c80 - 0x400 - B.length bin) 0)
    (B.replicate 0xc 0 <> encodeLeWord entry <> B.replicate (0xdaa0 - 0x10) 0)

checkRecordProjection :: B.ByteString -> B.ByteString -> Word32 -> ProgramFacts -> IO ()
checkRecordProjection bin symbols prefix facts = do
  let images = stagedFor bin prefix
      original = stagedRecord images
      locations = [(offset, MutableLogByte) | offset <- [0x4c60 .. 0x4c6b]]
        ++ [(offset, ImmutableRecordByte) | offset <- 0x4c5f : [0x4c6c .. 0x4c7f]]
  mapM_ (\(offset, classification) -> do
    let changedBytes = B.take offset original <> B.singleton (B.index original offset `xor` 1) <> B.drop (offset + 1) original
    changed <- either fail pure (deriveOwnedFacts bin symbols (images {stagedRecord = changedBytes}))
    expect ("initial full identity binds record byte " ++ show offset) (factRecord changed /= factRecord facts)
    expect ("immutable projection class at record byte " ++ show offset)
      ((factRecordImmutable changed == factRecordImmutable facts) == (classification == MutableLogByte))
    checkProjectionAdmission facts changed classification
    ) locations

checkNaturalArtifact :: ArtifactInput -> IO ProgramFacts
checkNaturalArtifact (ArtifactInput image@(U.MediaImage bin) document@(SymbolDocument symbols)) = do
  prefixEntry <- symbolAt document "xm1_loader_prefix_entry"
  facts <- either fail pure (deriveOwnedFacts bin symbols (stagedFor bin prefixEntry))
  expect "combined prefix artifact is admitted at its typed boundary"
    (accepted (Stage.parseSecondaryPrefixArtifact image symbols))
  expect "secondary artifact cannot be admitted as a firststage artifact"
    (not (accepted (Stage.parseFirstStageArtifact image symbols)))
  expect "truncated secondary artifact is refused at its typed boundary"
    (not (accepted (Stage.parseSecondaryPrefixArtifact (U.MediaImage (B.take (B.length bin - 4) bin)) symbols)))
  let Guest entry = factEntry facts
  expect "owned entry is the published prefix entry" (entry == prefixEntry)
  expect "correct image declares no refusal" (factRefusal facts == Nothing)
  expect "ordinary image cannot establish the invalid-core negative"
    (not (accepted (deriveScenarioOwnedFacts NegativeInvalidCore bin symbols (stagedFor bin prefixEntry))))
  expect "ordinary image cannot establish the predicate-zero negative"
    (not (accepted (deriveScenarioOwnedFacts NegativePredicateZero bin symbols (stagedFor bin prefixEntry))))
  expect "loader B naming another entry is refused" (not (accepted (deriveOwnedFacts bin symbols (stagedFor bin 0xd0020))))
  checkRecordProjection bin symbols prefixEntry facts
  pure facts

checkRefusalArtifact :: ArtifactInput -> IO ()
checkRefusalArtifact (ArtifactInput (U.MediaImage bin) document@(SymbolDocument symbols)) = do
  refusalEntry <- symbolAt document "xm1_loader_prefix_entry"
  refusalFacts <- either fail pure (deriveOwnedFacts bin symbols (stagedFor bin refusalEntry))
  expect "refusal control declares its refusal site" (factRefusal refusalFacts /= Nothing)

checkNegativeArtifact :: Scenario -> ArtifactInput -> IO ()
checkNegativeArtifact scenario (ArtifactInput (U.MediaImage bin) document@(SymbolDocument symbols)) = do
  entry <- symbolAt document "xm1_loader_prefix_entry"
  let images = stagedFor bin entry
      derive = deriveScenarioOwnedFacts scenario
      marker = case scenario of
        NegativeInvalidCore -> "xm1_loader_prefix_invalid_core_fault"
        NegativePredicateZero -> "xm1_loader_prefix_predicate_zero_fault"
        _ -> error "negative prefix artifact requires a standalone negative scenario"
      withoutMarker = BC.unlines (filter (\line -> case BC.words line of
        [_, _, _, name] -> BC.unpack name /= marker
        _ -> True) (BC.lines symbols))
  facts <- either fail pure (derive bin symbols images)
  expect "negative source image is refused by ordinary preparation"
    (not (accepted (deriveOwnedFacts bin symbols images)))
  expect "negative preparation requires its exported marker"
    (not (accepted (derive bin withoutMarker images)))
  case scenario of
    NegativePredicateZero -> do
      expect "zero-accepting image has no polling cycle" (factPeriod facts == ZeroAcceptingExit)
      address <- symbolAt document marker
      let offset = fromIntegral address - 0x10400
          changed = B.take offset bin <> encodeLeWord 0 <> B.drop (offset + 4) bin
      expect "predicate preparation binds the marker bytes"
        (not (accepted (derive changed symbols (stagedFor changed entry))))
      let wrongContinuation = B.concat
            [if leWordAt bin location == Right 0xd0aac then encodeLeWord 0xd0ab0 else B.take 4 (B.drop location bin)
            | location <- [0, 4 .. B.length bin - 4]]
      expect "predicate preparation proves its actual continuation"
        (not (accepted (derive wrongContinuation symbols (stagedFor wrongContinuation entry))))
    _ -> expect "invalid-core image retains its actual polling cycle" (case factPeriod facts of
      WaitingPoll _ -> True
      ZeroAcceptingExit -> False)

checkInterleavedTailArtifact :: ArtifactInput -> IO ()
checkInterleavedTailArtifact (ArtifactInput image@(U.MediaImage bin) (SymbolDocument symbols)) = do
  artifact <- either fail pure (Stage.parseFirstStageArtifact image symbols)
  hook <- either fail pure (Stage.interleavedTailHook artifact)
  expect "interleaved hook displaces the retained installer word" (Stage.interleavedTailSite hook == 0x40040)
  let offset = fromIntegral (Stage.interleavedTailTarget hook) - 0x6000
      changed = B.take offset bin <> encodeLeWord 0xe3a03028 <> B.drop (offset + 4) bin
  expect "interleaved hook binds the helper instructions"
    (not (accepted (Stage.parseFirstStageArtifact (U.MediaImage changed) symbols >>= Stage.interleavedTailHook)))
