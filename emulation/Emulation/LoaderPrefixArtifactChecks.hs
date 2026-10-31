module Emulation.LoaderPrefixArtifactChecks
  ( ArtifactInput (..)
  , SymbolDocument (..)
  , checkNaturalArtifact
  , checkRefusalArtifact
  ) where

import qualified Data.ByteString as B
import Data.Bits (xor)
import Data.Word (Word32)
import Emulation.FirstStageWitness (encodeLeWord)
import Emulation.LoaderPrefixReceiptFixtures
  ( RecordByteClass (..), accepted, checkProjectionAdmission, expect )
import Emulation.LoaderPrefixWitness
  ( Guest (..), ProgramFacts (..), StagedImages (..), deriveOwnedFacts )
import Emulation.PollPeriod (PollPeriod (..))
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
  dispatch <- symbolAt document "xm1_loader_prefix_dispatch"
  facts <- either fail pure (deriveOwnedFacts bin symbols (stagedFor bin prefixEntry))
  expect "combined prefix artifact is admitted at its typed boundary"
    (accepted (Stage.parseSecondaryPrefixArtifact image symbols))
  expect "secondary artifact cannot be admitted as a firststage artifact"
    (not (accepted (Stage.parseFirstStageArtifact image symbols)))
  expect "truncated secondary artifact is refused at its typed boundary"
    (not (accepted (Stage.parseSecondaryPrefixArtifact (U.MediaImage (B.take (B.length bin - 4) bin)) symbols)))
  let Guest entry = factEntry facts
      Guest post = factPostLoad facts
      PollPeriod period = factPeriod facts
  expect "owned entry is the published prefix entry" (entry == prefixEntry)
  expect "owned post-load site lies in dispatch" (post > dispatch && post < dispatch + 0x60)
  expect "owned poll period is derived" (period > 0)
  expect "correct image declares no refusal" (factRefusal facts == Nothing)
  expect "loader B naming another entry is refused" (not (accepted (deriveOwnedFacts bin symbols (stagedFor bin 0xd0020))))
  checkRecordProjection bin symbols prefixEntry facts
  pure facts

checkRefusalArtifact :: ArtifactInput -> IO ()
checkRefusalArtifact (ArtifactInput (U.MediaImage bin) document@(SymbolDocument symbols)) = do
  refusalEntry <- symbolAt document "xm1_loader_prefix_entry"
  refusalFacts <- either fail pure (deriveOwnedFacts bin symbols (stagedFor bin refusalEntry))
  expect "refusal control declares its refusal site" (factRefusal refusalFacts /= Nothing)
