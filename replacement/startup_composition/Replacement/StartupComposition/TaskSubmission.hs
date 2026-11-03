{-# LANGUAGE OverloadedStrings #-}
module Replacement.StartupComposition.TaskSubmission
  ( SourceFiles (..), NativeFiles (..), CompositionSource, JoinedArtifact, JoinedProof
  , JoinError (..), JoinedCall, joinedProof, wrapperArtifact, criticalArtifact, storageArtifact
  , entryLow, criticalEnterLow, criticalExitLow, reservationLow, joinedCalls
  , joinedBoundary, calleeLow, nestedLow, joinStack
  , ImageWindow (..), mergeImageWindows
  , StagedImage4, StagedCandidate, CandidateSources (..), canonicalSource, deriveArtifact
  , stageImage4, stagedBytes, stagedSources, stageCandidate, candidateBytes, candidateSources
  , candidateStagedImage4, contractValue, candidateContractValue
  ) where

import Control.Monad (forM, unless)
import Data.Aeson (Value, object, (.=))
import Data.Bits (complement)
import qualified Data.ByteString as B
import Data.List (sortOn)
import Data.Word (Word32)
import qualified Emulation.CriticalPair as C
import qualified Emulation.Image4Staging as Image4
import qualified Emulation.Sha256 as SHA
import qualified FujiAF.FirstStage as F
import qualified FujiAF.UpdateContainer as U
import Replacement.StartupComposition.Artifact (SourceFiles (..), NativeFiles (..))
import qualified Replacement.TaskStorage.Artifact as S
import qualified Replacement.TaskSubmission.Admission as A
import qualified Replacement.TaskSubmission.Artifact as W

data CompositionSource = CompositionSource SourceFiles W.CanonicalSource C.Original S.CanonicalSource
data JoinedCall = JoinedCall
  { joinedBoundary :: A.CallBoundary, calleeLow :: A.StackOffset, nestedLow :: A.StackOffset }
  deriving (Eq, Show)
data JoinedProof = JoinedProof
  { proofLows :: [(A.EntryPoint,A.StackOffset)]
  , criticalEnterLow :: A.StackOffset, criticalExitLow :: A.StackOffset, reservationLow :: A.StackOffset
  , joinedCalls :: [JoinedCall]
  }
data JoinedArtifact = JoinedArtifact W.LinkedArtifact C.LinkedArtifact S.LinkedArtifact JoinedProof
data StagedImage4 = StagedImage4 CompositionSource JoinedArtifact C.Staging W.StagedImage4 S.StagedImage4 CandidateSources
data StagedCandidate = StagedCandidate StagedImage4 Image4.StagedCandidate CandidateSources
data CandidateSources = CandidateSources
  { candidateLoaderA :: B.ByteString, candidateLoaderB :: B.ByteString
  , candidateRecord2 :: B.ByteString, candidateImage4 :: B.ByteString }
data ImageWindow = ImageWindow A.CodeAddress A.CodeAddress B.ByteString deriving (Eq, Show)
data JoinError
  = WrapperRejected W.ArtifactError | CriticalRejected String | StorageRejected String
  | SourceRejected String | MissingOwnedCall A.FixedService | CallRejected A.AdmissionError
  | ContinuationRejected | WindowRejected | CandidateRejected String
  deriving (Eq, Show)

require :: Bool -> JoinError -> Either JoinError ()
require condition failure = unless condition (Left failure)

canonicalSource :: SourceFiles -> Either JoinError CompositionSource
canonicalSource files = do
  wrapper <- either (Left . WrapperRejected) Right
    (W.canonicalSource (sourceDat files) (sourceImage4 files) (sourceLoaderA files) (sourceLoaderB files))
  critical <- either (Left . SourceRejected) Right (C.deriveOriginal (sourceDat files) (sourceImage4 files))
  storage <- either (Left . SourceRejected) Right
    (S.canonicalSource (sourceDat files) (sourceImage4 files) (sourceLoaderA files) (sourceLoaderB files))
  pure (CompositionSource files wrapper critical storage)

deriveArtifact :: NativeFiles -> NativeFiles -> NativeFiles -> Either JoinError JoinedArtifact
deriveArtifact wrapperFiles criticalFiles storageFiles = do
  wrapper <- either (Left . WrapperRejected) Right (W.deriveArtifact (nativeBytes wrapperFiles) (nativeSymbols wrapperFiles))
  critical <- either (Left . CriticalRejected) Right (C.deriveLinkedArtifact (nativeBytes criticalFiles) (nativeSymbols criticalFiles))
  storage <- either (Left . StorageRejected) Right (S.deriveArtifact (nativeBytes storageFiles) (nativeSymbols storageFiles))
  let frame = W.frameProof wrapper
      layout = C.witnessLayout critical
      C.FrameBytes enter = C.witnessEntryFrame layout
      C.FrameBytes exit = C.witnessExitFrame layout
      S.FrameOffset rust = S.scratchLowFromRustSp (S.admissionScratch (S.artifactAdmission storage))
      demands = [(A.CriticalEnter,A.StackOffset (negate (toInteger enter)))
                ,(A.CriticalExit,A.StackOffset (negate (toInteger exit)))
                ,(A.Reservation,A.StackOffset (min rust (-8)-32))]
      owned = [(boundary,low) | boundary <- A.callBoundaries frame
        , A.FixedService target <- [A.callTarget boundary], Just low <- [lookup target demands]]
  mapM_ (\target -> require (any ((== A.FixedService target) . A.callTarget . fst) owned) (MissingOwnedCall target))
    [A.CriticalEnter,A.CriticalExit,A.Reservation]
  calls <- forM owned $ \(boundary,low) -> do
    let A.CodeAddress instruction = A.callInstruction boundary
        A.CodeAddress continuation = A.callContinuation boundary
        A.StackOffset actual = A.callStack boundary
        A.StackOffset demand = low
        live = [offset | A.StackOffset offset <- A.callLiveWords boundary]
    require (actual `mod` 8 == 0) (CallRejected A.CallStackUnaligned)
    require (all (\offset -> offset >= actual && offset+4 <= 0 && offset `mod` 4 == 0) live)
      (CallRejected A.NestedStackOverlap)
    require (continuation == instruction+4 && A.CodeAddress continuation `elem` A.executionPcs frame) ContinuationRejected
    pure (JoinedCall boundary low (A.StackOffset (actual+demand)))
  let lowFor entry = minimum (A.scratchLow frame entry:
        [nestedLow call | call <- calls,A.callEntry (joinedBoundary call) == entry])
      proof = JoinedProof [(entry,lowFor entry) | entry <- [A.Submission,A.Release]]
        (A.StackOffset (negate (toInteger enter))) (A.StackOffset (negate (toInteger exit)))
        (A.StackOffset (min rust (-8)-32)) calls
  pure (JoinedArtifact wrapper critical storage proof)

joinedProof :: JoinedArtifact -> JoinedProof
joinedProof (JoinedArtifact _ _ _ proof) = proof
wrapperArtifact :: JoinedArtifact -> W.LinkedArtifact
wrapperArtifact (JoinedArtifact wrapper _ _ _) = wrapper
criticalArtifact :: JoinedArtifact -> C.LinkedArtifact
criticalArtifact (JoinedArtifact _ critical _ _) = critical
storageArtifact :: JoinedArtifact -> S.LinkedArtifact
storageArtifact (JoinedArtifact _ _ storage _) = storage
entryLow :: JoinedProof -> A.EntryPoint -> A.StackOffset
entryLow proof entry = minimum [low | (kind,low) <- proofLows proof,kind == entry]

joinStack :: JoinedArtifact -> A.EntryPoint -> A.StackContext -> [A.CalleeDemand] -> Either JoinError A.JoinedStack
joinStack artifact entry context retained = do
  let proof = joinedProof artifact
      owned = [A.CalleeDemand (A.FixedService target) low | (target,low) <-
        [(A.CriticalEnter,criticalEnterLow proof),(A.CriticalExit,criticalExitLow proof),(A.Reservation,reservationLow proof)]]
  either (Left . CallRejected) Right (A.joinStack (W.frameProof (wrapperArtifact artifact)) entry context (owned++retained))

mergeImageWindows :: B.ByteString -> [ImageWindow] -> Either JoinError B.ByteString
mergeImageWindows original supplied = do
  let windows = sortOn (\(ImageWindow (A.CodeAddress start) _ _) -> start) supplied
      endOf (ImageWindow _ (A.CodeAddress end) _) = end
      startOf (ImageWindow (A.CodeAddress start) _ _) = start
  require (and (zipWith (\left right -> endOf left <= startOf right) windows (drop 1 windows))) WindowRejected
  mapM_ (\(ImageWindow (A.CodeAddress start) (A.CodeAddress end) bytes) ->
    require (start >= 0x61c400 && start < end && start `mod` 4 == 0 && end `mod` 4 == 0
      && toInteger end-0x61c400 <= toInteger (B.length original)
      && toInteger (B.length bytes) == toInteger end-toInteger start) WindowRejected) windows
  let build offset [] = B.drop offset original
      build offset (ImageWindow (A.CodeAddress start) (A.CodeAddress end) bytes:rest) =
        let begin = fromIntegral (start-0x61c400); finish = fromIntegral (end-0x61c400)
        in B.take (begin-offset) (B.drop offset original) <> bytes <> build finish rest
  pure (build 0 windows)

extractSources :: B.ByteString -> B.ByteString -> Either JoinError CandidateSources
extractSources dat image = do
  parsed <- either (Left . CandidateRejected) Right (U.parseContainer dat)
  let parts = U.parsedParts parsed
      loader ident base extent = case filter ((== ident) . U.segId) (U.partSegments parts) of
        [segment] -> do
          let offset = fromIntegral (U.segFlash segment)
              count = fromIntegral (U.segSize segment)
              pre = U.partPreTable parts
          require (U.segLoad segment == base && count == extent && offset+count <= B.length pre)
            (CandidateRejected "loader geometry differs")
          pure (B.map complement (B.take count (B.drop offset pre)))
        _ -> Left (CandidateRejected "loader segment is not unique")
  primary <- loader 0 0x40000 0xdae0
  secondary <- loader 1 0xd0000 0xdaa0
  record <- either (Left . CandidateRejected) Right (F.recordSlotBytes F.SecondRecord parts)
  pure (CandidateSources primary secondary (B.map complement record) image)

stageImage4 :: CompositionSource -> JoinedArtifact -> Either JoinError StagedImage4
stageImage4 source@(CompositionSource files wrapperSource criticalSource storageSource) artifact = do
  critical <- either (Left . CriticalRejected) Right (C.stageStartup criticalSource (criticalArtifact artifact))
  let wrapper = W.stageImage4 wrapperSource (wrapperArtifact artifact)
      storage = S.stageImage4 storageSource (storageArtifact artifact)
      window start end image = ImageWindow (A.CodeAddress start) (A.CodeAddress end)
        (B.take (fromIntegral (end-start)) (B.drop (fromIntegral (start-0x61c400)) image))
  image <- mergeImageWindows (sourceImage4 files)
    [window 0x7235d8 0x7235dc (C.patchedImage4 critical),window 0x7236b0 0x7236b4 (C.patchedImage4 critical)
    ,window 0x725cc4 0x725da8 (S.stagedBytes storage),window 0x725dd8 0x72612c (W.stagedBytes wrapper)]
  sources <- extractSources (C.startupDat critical) image
  pure (StagedImage4 source artifact critical wrapper storage sources)

stagedSources :: StagedImage4 -> CandidateSources
stagedSources (StagedImage4 _ _ _ _ _ sources) = sources
stagedBytes :: StagedImage4 -> B.ByteString
stagedBytes = candidateImage4 . stagedSources
stageCandidate :: Image4.Tools -> Image4.ScratchDirectory -> StagedImage4 -> IO StagedCandidate
stageCandidate tools scratch staged@(StagedImage4 (CompositionSource files _ _ _) _ critical _ _ _) = do
  candidate <- Image4.runImage4Staging tools scratch (sourceDat files) (C.startupDat critical) (stagedBytes staged)
  sources <- either (fail . show) pure (extractSources (Image4.candidateDat candidate) (stagedBytes staged))
  pure (StagedCandidate staged candidate sources)
candidateBytes :: StagedCandidate -> B.ByteString
candidateBytes (StagedCandidate _ candidate _) = Image4.candidateDat candidate
candidateSources :: StagedCandidate -> CandidateSources
candidateSources (StagedCandidate _ _ sources) = sources
candidateStagedImage4 :: StagedCandidate -> StagedImage4
candidateStagedImage4 (StagedCandidate staged _ _) = staged

digest :: B.ByteString -> String
digest = SHA.sha256Hex . SHA.sha256
stackOffsetValue :: A.StackOffset -> Integer
stackOffsetValue (A.StackOffset value) = value
codeAddressValue :: A.CodeAddress -> Word32
codeAddressValue (A.CodeAddress value) = value
criticalAddress :: C.Guest -> Word32
criticalAddress (C.Guest value) = value
storageAddress :: S.InstructionAddress -> Word32
storageAddress (S.InstructionAddress value) = value

contractValue :: StagedImage4 -> Value
contractValue staged@(StagedImage4 _ _ critical _ _ _) = compositionContract staged (C.startupDat critical)

compositionContract :: StagedImage4 -> B.ByteString -> Value
compositionContract staged@(StagedImage4 _ artifact critical wrapper storage sources) finalDat =
  let proof = joinedProof artifact
      wrapperProof = W.frameProof (wrapperArtifact artifact)
      criticalMetadata = C.criticalProof (criticalArtifact artifact)
      storageMetadata = storageArtifact artifact
  in object
    [ "schema" .= (1 :: Int),"qualification" .= ("conditional-owned-task-composition" :: String)
    , "task_submission" .= W.contractValue wrapper
    , "critical_pair" .= C.stagingValue critical finalDat
    , "task_storage" .= S.contractValue storage
    , "component_linked_bytes" .= object ["wrapper" .= B.length (W.linkedBytes (wrapperArtifact artifact))
        ,"critical" .= B.length (C.linkedBytes (criticalArtifact artifact)),"storage" .= B.length (S.linkedBytes (storageArtifact artifact))]
    , "sources" .= object ["loader_a_sha256" .= digest (candidateLoaderA sources),"loader_b_sha256" .= digest (candidateLoaderB sources)
        ,"record2_sha256" .= digest (candidateRecord2 sources),"image4_sha256" .= digest (stagedBytes staged)]
    , "execution" .= object
        [ "wrapper_pcs" .= map codeAddressValue (A.executionPcs wrapperProof),"wrapper_global_store_pcs" .= map codeAddressValue (A.globalWrites wrapperProof)
        , "critical_pcs" .= map criticalAddress (C.criticalExecutionPcs criticalMetadata)
        , "critical_counter_store_pcs" .= map criticalAddress (C.criticalCounterStorePcs criticalMetadata)
        , "critical_private_store_pcs" .= map criticalAddress (C.criticalPrivateStorePcs criticalMetadata)
        , "storage_pcs" .= map storageAddress (S.executionPcs storageMetadata),"storage_guest_store_pcs" .= map storageAddress (S.guestStorePcs storageMetadata)
        , "storage_size_store_pcs" .= map storageAddress (S.sizeStorePcs storageMetadata)]
    , "owned_stack" .= object
        [ "submission_low" .= stackOffsetValue (entryLow proof A.Submission),"release_low" .= stackOffsetValue (entryLow proof A.Release)
        , "critical_enter_low" .= stackOffsetValue (criticalEnterLow proof),"critical_exit_low" .= stackOffsetValue (criticalExitLow proof)
        , "reservation_low" .= stackOffsetValue (reservationLow proof)
        , "calls" .= [object ["entry" .= show (A.callEntry boundary),"instruction" .= codeAddressValue (A.callInstruction boundary)
            ,"continuation" .= codeAddressValue (A.callContinuation boundary),"target" .= show (A.callTarget boundary)
            ,"stack_from_entry" .= stackOffsetValue (A.callStack boundary),"caller_live_words" .= map stackOffsetValue (A.callLiveWords boundary)
            ,"callee_low" .= stackOffsetValue (calleeLow call),"nested_low" .= stackOffsetValue (nestedLow call)]
          | call <- joinedCalls proof,let boundary = joinedBoundary call]]
    , "open_premises" .= (["owned bounds exclude remaining retained callees; joinStack requires their demands"
        ,"actual natural incoming stack, writable backing, aliases, TCM and live-frame/resource separation"
        ,"retained selector, IRQ and lock bindings remain stable; acquisition success is not assumed"
        ,"component lifetime, concurrent writers and later service/task behavior remain conditional"] :: [String])
    ]

candidateContractValue :: StagedCandidate -> Value
candidateContractValue (StagedCandidate staged candidate _) = object
  [ "schema" .= (1 :: Int),"qualification" .= ("host-admitted-owned-task-composition" :: String)
  , "composition" .= compositionContract staged (Image4.candidateDat candidate),"candidate_dat_sha256" .= Image4.candidateSha256 candidate
  , "candidate_image4_sha256" .= Image4.candidateImage4Sha256 candidate ]
