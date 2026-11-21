{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}

-- | Stage-0 submitted work over the render pair's e2e observations (`emulation/menu-engine-pair-trajectories/engine.jq`,
-- `submission_sites`): the retained OpenVG and EGL calls of VGLIB's render `0x6d724c` with the arrays their pointer
-- arguments name, the 104-byte payloads task 73 sends to task 71, the commands task 72 dispatches to the vector
-- engine and the interrupt status each completion leaves (reports/display-render-contracts/vglib-frame.md,
-- "Vector-engine submission"). Calls and payloads on a frame step's core belong to that step; dispatches and
-- completions run on task 72 and belong to the step open on any core.
module Replacement.ModalRuntime.FrameSubmission
  ( SubmissionSite (..)
  , Operation (..)
  , Stream (..)
  , SubmittedFrame (..)
  , PhaseSubmission (..)
  , FramePath (..)
  , Mismatch (..)
  , submissionSite
  , operation
  , phaseSubmission
  , compareSubmission
  , frameInstructions
  ) where

import Control.Monad (foldM, unless, when)
import qualified Data.ByteString as B
import qualified Data.Map.Strict as M
import qualified Data.Text as T
import Data.Word (Word32, Word64)
import Replacement.ModalRuntime.FramePresentation (FrameRecord (..), parseFrameRecord)
import Replacement.ModalRuntime.MenuEnginePair (Hit (..), Site (..))
import Replacement.ModalRuntime.RenderStackEvidence (InstructionCounter (..))

data SubmissionSite
  = SetInteger | SetFloat | SetVector | SetIntegers | Clear | LoadIdentity | LoadMatrix | MultiplyMatrix
  | Translate | Rotate | Scale | SetPaint | DrawPath | DrawImage | Finish | GetInteger
  | MakeCurrent | CopyBuffers | GetError
  | JobSubmit | JobSubmitReturn | JobDispatch | JobCompletion
  deriving (Eq, Ord, Show, Enum, Bounded)

siteLabel :: SubmissionSite -> T.Text
siteLabel site = case site of
  SetInteger -> "vg-set-integer"
  SetFloat -> "vg-set-float"
  SetVector -> "vg-set-vector"
  SetIntegers -> "vg-set-integers"
  Clear -> "vg-clear"
  LoadIdentity -> "vg-load-identity"
  LoadMatrix -> "vg-load-matrix"
  MultiplyMatrix -> "vg-multiply-matrix"
  Translate -> "vg-translate"
  Rotate -> "vg-rotate"
  Scale -> "vg-scale"
  SetPaint -> "vg-set-paint"
  DrawPath -> "vg-draw-path"
  DrawImage -> "vg-draw-image"
  Finish -> "vg-finish"
  GetInteger -> "vg-get-integer"
  MakeCurrent -> "egl-make-current"
  CopyBuffers -> "egl-copy-buffers"
  GetError -> "egl-get-error"
  JobSubmit -> "vg-job-submit"
  JobSubmitReturn -> "vg-job-submit-return"
  JobDispatch -> "vg-job-dispatch"
  JobCompletion -> "vg-job-completion"

submissionSite :: T.Text -> Maybe SubmissionSite
submissionSite text = M.lookup text sites
  where sites = M.fromList [(siteLabel site, site) | site <- [minBound .. maxBound]]

-- | One submitted operation: its site, the argument words that are values, and the bytes its pointer argument
-- names. Pointer words themselves are not compared: stock passes arrays in its draw methods' frames.
data Operation = Operation SubmissionSite [Word32] B.ByteString deriving (Eq, Show)

-- | Task 73's calls and payloads, or task 72's dispatches and completions.
data Stream = CallStream | EngineStream deriving (Eq, Show)

stream :: SubmissionSite -> Stream
stream site = if site `elem` [JobDispatch, JobCompletion] then EngineStream else CallStream

operation :: SubmissionSite -> Hit -> Either String Operation
operation site hit = case site of
  SetInteger -> values [r0, r1]
  SetFloat -> values [r0, r1]
  SetVector -> counted 8
  SetIntegers -> counted 4
  Clear -> values [r0, r1, r2, r3]
  LoadIdentity -> values []
  LoadMatrix -> pointed 36
  MultiplyMatrix -> pointed 36
  Translate -> values [r0, r1]
  Rotate -> values [r0]
  Scale -> values [r0, r1]
  SetPaint -> values [r0, r1]
  DrawPath -> values [r0, r1]
  DrawImage -> values [r0]
  Finish -> values []
  GetInteger -> values [r0]
  MakeCurrent -> Operation site [r0, r1, r2, r3] <$> memory 44
  CopyBuffers -> values [r0, r1, r2]
  GetError -> values []
  JobSubmit -> Operation site [r1] <$> memory 104
  JobSubmitReturn -> values [r0]
  JobDispatch -> Operation site [] <$> memory 112
  JobCompletion -> Operation site [] <$> memory 4
 where
  (r0, r1, r2, r3) = hitRegisters hit
  values operands = Right (Operation site operands B.empty)
  memory size = case hitMemory hit of
    [bytes] | B.length bytes == size -> Right bytes
    _ -> Left (T.unpack (siteLabel site) ++ " lacks its " ++ show size ++ "-byte read")
  pointed size = Operation site [] <$> memory size
  counted limit = do
    when (r1 > limit) (Left (T.unpack (siteLabel site) ++ " count " ++ show r1 ++ " exceeds its read"))
    Operation site [r0, r1] . B.take (4 * fromIntegral r1) <$> memory (4 * fromIntegral limit)

-- | Whether the owned frame step rendered the frame, from `rb_render_frame_record`'s owned-frame count.
data FramePath = StockStep | OwnedStep deriving (Eq, Show)

data SubmittedFrame = SubmittedFrame
  { framePath :: FramePath
  , frameCalls :: [Operation]
  , frameEngine :: [Operation]
  , frameEntry :: Maybe InstructionCounter
  , frameReturn :: Maybe InstructionCounter
  } deriving (Eq, Show)

data PhaseSubmission = PhaseSubmission
  { phaseLabel :: T.Text
  , phaseFrames :: [SubmittedFrame]
  , phaseOutsideCalls :: [Operation]
  , phaseOutsideEngine :: [Operation]
  } deriving (Eq, Show)

-- | Guest instructions from a frame step's entry to its dispatcher return, summed over both cores.
frameInstructions :: SubmittedFrame -> Maybe Word64
frameInstructions frame = case (frameEntry frame, frameReturn frame) of
  (Just (InstructionCounter entry), Just (InstructionCounter after)) | after >= entry -> Just (after - entry)
  _ -> Nothing

data Open = Open { openRecord :: Maybe FrameRecord, openEntry :: Maybe InstructionCounter, openCalls :: [Operation], openEngine :: [Operation] }

-- | The one declared normalization. `vgc_geometry_allocate_buffers` allocates a zeroed 32-byte request from the
-- graphics heap, fills it and passes its address to `vgc_submit_thread_request` (`I4:82adf8-82ae84`), which sends
-- a payload with flags 1 at +0, opcode 0xc at +4 and that address at +0x2c (`I4:8555f4-85560c`); the engine
-- writes its results into the request, which the caller reads back (`I4:82ae8c-82aea0`). The heap address
-- differs between roles, so an opcode-0xc payload or command is compared with the word at +0x2c replaced by the
-- 32 bytes `vg-geometry-request` read through it at the send.
geometryRequestHeader :: B.ByteString
geometryRequestHeader = B.pack [1, 0, 0, 0, 0xc, 0, 0, 0]

geometryRequestOffset :: Int
geometryRequestOffset = 0x2c

-- | The address word and the request bytes of a `vg-geometry-request` hit.
geometryRequest :: Hit -> Either String (Word32, B.ByteString)
geometryRequest hit = case hitMemory hit of
  [payload, request] | B.length payload == 104, B.take 8 payload == geometryRequestHeader, B.length request == 32 ->
    Right (payloadWord payload, request)
  _ -> Left "vg-geometry-request lacks its opcode-0xc payload and 32-byte request reads"

payloadWord :: B.ByteString -> Word32
payloadWord payload = foldr (\byte word -> word * 256 + fromIntegral byte) 0 (B.unpack (B.take 4 (B.drop geometryRequestOffset payload)))

-- | An opcode-0xc job submission or dispatch with its request address replaced by the request's bytes.
resolveGeometry :: M.Map Word32 B.ByteString -> Operation -> Either String Operation
resolveGeometry requests submitted@(Operation site values bytes)
  | site `elem` [JobSubmit, JobDispatch] && B.take 8 bytes == geometryRequestHeader =
      case M.lookup (payloadWord bytes) requests of
        Just request -> Right (Operation site values (B.take geometryRequestOffset bytes <> request <> B.drop (geometryRequestOffset + 4) bytes))
        Nothing -> Left (T.unpack (siteLabel site) ++ ": opcode-0xc job without its vg-geometry-request")
  | otherwise = Right submitted

label :: Hit -> T.Text
label hit = case hitSite hit of
  OtherSite text -> text
  _ -> ""

-- | The record read the frame sites take in the owned frame package: the sixth read after the front byte, the
-- object words, the two image words and the engine-1 shadow.
frameRecordRead :: Hit -> Either String (Maybe FrameRecord)
frameRecordRead hit = case hitMemory hit of
  [_, _, _, _, _, bytes] -> Just <$> parseFrameRecord bytes
  [_, _, _, _, _] -> Right Nothing
  _ -> Left "frame step lacks its reads"

-- | One observation's frames in recording order, and its submitted work outside every frame step.
phaseSubmission :: T.Text -> [Hit] -> Either String PhaseSubmission
phaseSubmission phase hits = do
  (open, frames, outsideCalls, outsideEngine, _) <- foldM step (M.empty, [], [], [], M.empty) hits
  unless (M.null open) (Left (T.unpack phase ++ ": frame step without its dispatcher return"))
  pure (PhaseSubmission phase (reverse frames) (reverse outsideCalls) (reverse outsideEngine))
 where
  step (open, frames, calls, engine, requests) hit = let core = hitCore hit in case label hit of
    "frame-step-entry" -> do
      when (M.member core open) (Left (T.unpack phase ++ ": nested frame step"))
      record <- frameRecordRead hit
      pure (M.insert core (Open record (hitInstructions hit) [] []) open, frames, calls, engine, requests)
    "frame-dispatch-return" -> case M.lookup core open of
      Nothing -> pure (open, frames, calls, engine, requests)
      Just current -> do
        after <- frameRecordRead hit
        path <- case (openRecord current, after) of
          (Just before, Just later)
            | recordOwnedFrames later == recordOwnedFrames before + 1 -> Right OwnedStep
            | recordOwnedFrames later == recordOwnedFrames before -> Right StockStep
            | otherwise -> Left "owned frame count changed by more than one in a step"
          _ -> Right StockStep
        let frame = SubmittedFrame path (reverse (openCalls current)) (reverse (openEngine current)) (openEntry current) (hitInstructions hit)
        pure (M.delete core open, frame : frames, calls, engine, requests)
    "vg-geometry-request" -> do
      (address, request) <- either (Left . ((T.unpack phase ++ ": ") ++)) Right (geometryRequest hit)
      pure (open, frames, calls, engine, M.insert address request requests)
    text -> case submissionSite text of
      Nothing -> pure (open, frames, calls, engine, requests)
      Just site -> do
        submitted <- either (Left . ((T.unpack phase ++ ": ") ++)) Right (operation site hit >>= resolveGeometry requests)
        case stream site of
          CallStream -> case M.lookup core open of
            Just current -> pure (M.insert core current { openCalls = submitted : openCalls current } open, frames, calls, engine, requests)
            Nothing -> pure (open, frames, submitted : calls, engine, requests)
          EngineStream -> case M.toList open of
            [(owner, current)] -> pure (M.insert owner current { openEngine = submitted : openEngine current } open, frames, calls, engine, requests)
            [] -> pure (open, frames, calls, submitted : engine, requests)
            _ -> Left (T.unpack phase ++ ": engine work while frame steps are open on both cores")

-- | The first difference between the roles: phase, frame index (absent outside frame steps), stream, operation
-- index and both operations.
data Mismatch = Mismatch
  { mismatchPhase :: T.Text
  , mismatchFrame :: Maybe Int
  , mismatchStream :: Stream
  , mismatchIndex :: Int
  , mismatchStock :: Maybe Operation
  , mismatchOwned :: Maybe Operation
  } deriving (Eq, Show)

-- | Equal phases, equal frame counts per phase, and per frame (and outside frames) equal operations in order in
-- both streams. A phase or frame-count difference is reported as a `Left`.
compareSubmission :: [PhaseSubmission] -> [PhaseSubmission] -> Either String (Maybe Mismatch)
compareSubmission stock owned = do
  unless (map phaseLabel stock == map phaseLabel owned) (Left "the roles' phases differ")
  firstJust <$> mapM comparePhase (zip stock owned)
 where
  comparePhase (left, right) = do
    let phase = phaseLabel left
    unless (length (phaseFrames left) == length (phaseFrames right))
      (Left (T.unpack phase ++ ": stock has " ++ show (length (phaseFrames left)) ++ " frame steps, owned "
        ++ show (length (phaseFrames right))))
    let framed = [ difference phase (Just index) kind (pick a) (pick b)
                 | (index, a, b) <- zip3 [0 ..] (phaseFrames left) (phaseFrames right)
                 , (kind, pick) <- [(CallStream, frameCalls), (EngineStream, frameEngine)] ]
        outside = [ difference phase Nothing CallStream (phaseOutsideCalls left) (phaseOutsideCalls right)
                  , difference phase Nothing EngineStream (phaseOutsideEngine left) (phaseOutsideEngine right) ]
    pure (firstJust (framed ++ outside))
  difference phase frame kind left right =
    case [index | (index, a, b) <- zip3 [0 ..] (pad left) (pad right), a /= b] of
      index : _ -> Just (Mismatch phase frame kind index (at left index) (at right index))
      [] -> Nothing
   where
    width = max (length left) (length right)
    pad operations = map Just operations ++ replicate (width - length operations) Nothing
  at operations index = if index < length operations then Just (operations !! index) else Nothing
  firstJust values = case [value | Just value <- values] of
    value : _ -> Just value
    [] -> Nothing
