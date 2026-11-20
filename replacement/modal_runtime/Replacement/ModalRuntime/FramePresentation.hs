{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}

-- | VGLIB stage-0 frame publication over the render pair's e2e observations
-- (`emulation/menu-engine-pair-trajectories/engine.jq`, `frame_sites`): each role's frame steps at
-- `0x6d7920`, each paired with the dispatcher return `0x6d6f08` that follows it on the same core,
-- with the hand-off's item configuration, the display registration `0x6d80f0`, the `0xfefe` post
-- `0x6d7f30` and the event wait's result inside the step (reports/display-render-contracts/vglib-frame.md).
module Replacement.ModalRuntime.FramePresentation
  ( BufferIndex (..)
  , ImageAddress (..)
  , StageObjects (..)
  , BufferState (..)
  , FrameRecord (..)
  , FrameState (..)
  , WaitOutcome (..)
  , StepPath (..)
  , FrameStep (..)
  , PresentedFrame (..)
  , PhaseFrames (..)
  , DisplayRecord (..)
  , FrameRole (..)
  , frameSteps
  , checkRoleFrames
  , presentedFrames
  , comparePresentation
  , parseFrameRecord
  , displayRecordOf
  , frameRecordOf
  , declaredObjects
  ) where

import Control.Monad (foldM, forM, forM_, unless, when, zipWithM_)
import qualified Data.Aeson as J
import qualified Data.Aeson.Types as J
import Data.Bits ((.&.), shiftL)
import qualified Data.ByteString as B
import qualified Data.Map.Strict as M
import qualified Data.Text as T
import Data.Word (Word8, Word32, Word64)
import Numeric (readHex)
import Replacement.ModalRuntime.MenuEnginePair (Hit (..), Observation (..), Site (..))

-- | The front byte `0x8e21f8` in its valid range: buffer 0 is `O(0,0)+0x1c`, buffer 1 `O(0,1)+0x1c`.
data BufferIndex = EvenBuffer | OddBuffer deriving (Eq, Ord, Show)

newtype ImageAddress = ImageAddress Word32 deriving (Eq, Ord, Show)

-- | `O(0,0)` and `O(0,1)`, the words at `0x8e2158`/`0x8e215c`.
data StageObjects = StageObjects Word32 Word32 deriving (Eq, Show)

-- | `rb_render_frame_record` buffer states (replacement/render_adapter/README.md).
data BufferState = BufferFree | BufferDrawing | BufferQueued | BufferScanning deriving (Eq, Show)

data FrameRecord = FrameRecord
  { recordLastOwned :: Word64
  , recordLastAcknowledged :: Word64
  , recordBufferSequences :: (Word64, Word64)
  , recordBufferStates :: (BufferState, BufferState)
  , recordDrainPending :: Bool
  , recordOwnedFrames :: Word32
  , recordFailedWaits :: Word32
  } deriving (Eq, Show)

-- | The reads at a frame-step entry or return.
data FrameState = FrameState
  { stateFront :: BufferIndex
  , stateObjects :: StageObjects
  , stateImages :: (ImageAddress, ImageAddress)
  , stateShadow :: Word32
  , stateRecord :: Maybe FrameRecord
  } deriving (Eq, Show)

data WaitOutcome = WaitSucceeded | WaitFailed deriving (Eq, Show)

-- | Whether the owned step rendered the frame or resumed stock at `0x6d7930`.
data StepPath = StockPath | OwnedPath deriving (Eq, Show)

-- | An item configuration call: engine, group, item and address (`r0`-`r3`).
data ItemConfiguration = ItemConfiguration Word32 Word32 Word32 Word32 deriving (Eq, Show)

data FrameStep = FrameStep
  { stepBefore :: FrameState
  , stepAfter :: FrameState
  , stepPath :: StepPath
  , stepPublished :: Maybe (Word32, ImageAddress)
  , stepRegistrations :: Int
  , stepPosts :: Int
  , stepWait :: Maybe WaitOutcome
  } deriving (Eq, Show)

-- | One published frame as both roles must present it.
data PresentedFrame = PresentedFrame
  { presentedBuffer :: BufferIndex
  , presentedImage :: ImageAddress
  , presentedWait :: WaitOutcome
  } deriving (Eq, Show)

data PhaseFrames = PhaseFrames
  { phaseName :: T.Text
  , phaseSteps :: [FrameStep]
  , phaseDisplayRecord :: DisplayRecord
  , phaseFrameRecord :: Maybe FrameRecord
  } deriving (Eq, Show)

-- | The display-record cells `0x95a69c`, `0x95a6a0`, `0x95a6a8` and `0x95a6ac` at a capture.
data DisplayRecord = DisplayRecord Word32 Word32 Word32 Word32 deriving (Eq, Show)

data FrameRole = StockFrames | OwnedFrames deriving (Eq, Show)

word32 :: B.ByteString -> Int -> Either String Word32
word32 bytes offset
  | offset < 0 || offset + 4 > B.length bytes = Left "short frame word"
  | otherwise = Right (sum [fromIntegral (B.index bytes (offset + i)) `shiftL` (8 * i) | i <- [0 .. 3]])

word64 :: B.ByteString -> Int -> Either String Word64
word64 bytes offset = do
  low <- word32 bytes offset
  high <- word32 bytes (offset + 4)
  pure (fromIntegral low + fromIntegral high `shiftL` 32)

bufferIndex :: Word8 -> Either String BufferIndex
bufferIndex 0 = Right EvenBuffer
bufferIndex 1 = Right OddBuffer
bufferIndex value = Left ("front byte outside the two-buffer range: " ++ show value)

other :: BufferIndex -> BufferIndex
other EvenBuffer = OddBuffer
other OddBuffer = EvenBuffer

bufferState :: Word32 -> Either String BufferState
bufferState value = case value of
  0 -> Right BufferFree
  1 -> Right BufferDrawing
  2 -> Right BufferQueued
  3 -> Right BufferScanning
  _ -> Left ("frame record buffer state outside its enumeration: " ++ show value)

-- | Layout 1 of `rb_render_frame_record`, 0x78 bytes.
parseFrameRecord :: B.ByteString -> Either String FrameRecord
parseFrameRecord bytes = do
  unless (B.length bytes == 0x78) (Left "frame record is not 0x78 bytes")
  magic <- word32 bytes 0
  unless (magic == 0x52464252) (Left "frame record magic is not RBFR")
  layout <- word32 bytes 4
  unless (layout == 1) (Left "frame record layout is not 1")
  lastOwned <- word64 bytes 0x10
  acknowledged <- word64 bytes 0x18
  sequences <- (,) <$> word64 bytes 0x20 <*> word64 bytes 0x28
  states <- (,) <$> (word32 bytes 0x30 >>= bufferState) <*> (word32 bytes 0x34 >>= bufferState)
  drain <- word32 bytes 0x38 >>= \value -> case value of
    0 -> Right False
    1 -> Right True
    _ -> Left "frame record drain flag is not 0 or 1"
  FrameRecord lastOwned acknowledged sequences states drain <$> word32 bytes 0x3c <*> word32 bytes 0x40

-- | The step reads: front byte, `0x8e2158` (8 bytes), the two `+0x1c` words, the engine-1 shadow
-- `0x96bc10`, and in the owned frame package the record. `declared` is `(O(0,0), O(0,1))` whose
-- `+0x1c` the trajectory reads.
frameState :: FrameRole -> StageObjects -> [B.ByteString] -> Either String FrameState
frameState role declared memory = case memory of
  front : objects : first : second : shadow : rest -> do
    unless (B.length front == 1) (Left "frame front read is not one byte")
    index <- bufferIndex (B.head front)
    objectsRead <- StageObjects <$> word32 objects 0 <*> word32 objects 4
    unless (objectsRead == declared) (Left "stage-0 buffer objects differ from the declared objects whose images were read")
    images <- (,) <$> (ImageAddress <$> word32 first 0) <*> (ImageAddress <$> word32 second 0)
    shadowWord <- word32 shadow 0
    record <- case (role, rest) of
      (StockFrames, []) -> Right Nothing
      (OwnedFrames, []) -> Right Nothing
      (OwnedFrames, [bytes]) -> Just <$> parseFrameRecord bytes
      _ -> Left "frame step reads differ from the declared set"
    pure (FrameState index objectsRead images shadowWord record)
  _ -> Left "frame step lacks its reads"

label :: Hit -> T.Text
label hit = case hitSite hit of
  OtherSite text -> text
  _ -> ""

data Open = Open
  { openEntry :: FrameState
  , openConfigurations :: [ItemConfiguration]
  , openRegistrations :: Int
  , openPosts :: Int
  , openWaits :: [Word32]
  }

-- | One observation's frame steps in recording order. A frame-step entry opens a step on its core;
-- the next dispatcher return on that core closes it.
frameSteps :: FrameRole -> StageObjects -> Observation -> Either String [FrameStep]
frameSteps role declared observation = do
  (open, steps) <- foldM step (M.empty, []) (observationHits observation)
  unless (M.null open) (Left (T.unpack (observationPhase observation) ++ ": frame step without its dispatcher return"))
  pure (reverse steps)
 where
  inside core open f = case M.lookup core open of
    Nothing -> Left ("frame event outside a frame step: " ++ T.unpack (observationPhase observation))
    Just current -> Right (M.insert core (f current) open)
  step (open, steps) hit = let core = hitCore hit in case label hit of
    "frame-step-entry" -> do
      when (M.member core open) (Left "nested frame step")
      entry <- frameState role declared (hitMemory hit)
      pure (M.insert core (Open entry [] 0 0 []) open, steps)
    "frame-dispatch-return" -> case M.lookup core open of
      Nothing -> pure (open, steps)
      Just current -> do
        after <- frameState role declared (hitMemory hit)
        closed <- close current after
        pure (M.delete core open, closed : steps)
    "frame-item-configuration" -> case M.lookup core open of
      Nothing -> pure (open, steps)
      Just _ -> let (r0, r1, r2, r3) = hitRegisters hit
        in (, steps) <$> inside core open (\current -> current { openConfigurations = openConfigurations current ++ [ItemConfiguration r0 r1 r2 r3] })
    "frame-display-registration" -> (, steps) <$> anyCore open (\current -> current { openRegistrations = openRegistrations current + 1 })
    "frame-owned-display-registration"
      | role == OwnedFrames -> (, steps) <$> anyCore open (\current -> current { openRegistrations = openRegistrations current + 1 })
      | otherwise -> Left "stock role reached rb_render_frame_scanout_bridge"
    "frame-display-post" -> (, steps) <$> anyCore open (\current -> current { openPosts = openPosts current + 1 })
    "frame-wait-return" -> let (r0, _, _, _) = hitRegisters hit
      in (, steps) <$> inside core open (\current -> current { openWaits = openWaits current ++ [r0] })
    _ -> pure (open, steps)
  -- The registration and the post run in the display completion and the scheduler's delivery, not
  -- on the stepping core; the one open step takes them.
  anyCore open f = case M.toList open of
    [(core, _)] -> inside core open f
    [] -> Left ("display completion outside a frame step: " ++ T.unpack (observationPhase observation))
    _ -> Left "display completion while steps are open on both cores"
  close current after = do
    let published = [(engine, ImageAddress address) | ItemConfiguration engine 1 0 address <- openConfigurations current]
    publication <- case published of
      [] -> Right Nothing
      [one] -> Right (Just one)
      _ -> Left "frame step published group 1 item 0 more than once"
    path <- case (stateRecord (openEntry current), stateRecord after) of
      (Just before, Just later)
        | recordOwnedFrames later == recordOwnedFrames before + 1 -> Right OwnedPath
        | recordOwnedFrames later == recordOwnedFrames before -> Right StockPath
        | otherwise -> Left "owned frame count changed by more than one in a step"
      _ -> Right StockPath
    wait <- case (path, openWaits current) of
      (StockPath, []) -> Right Nothing
      (StockPath, [result]) -> Right (Just (if result == 0 then WaitSucceeded else WaitFailed))
      (OwnedPath, []) -> case (stateRecord (openEntry current), stateRecord after) of
        (Just before, Just later)
          | recordFailedWaits later == recordFailedWaits before -> Right (Just WaitSucceeded)
          | recordFailedWaits later == recordFailedWaits before + 1 -> Right (Just WaitFailed)
        _ -> Left "owned frame step's failed-wait count moved by more than one"
      (OwnedPath, _) -> Left "owned frame step reached the stock event wait"
      _ -> Left "frame step waited more than once"
    pure (FrameStep (openEntry current) after path publication (openRegistrations current) (openPosts current) wait)

-- | The read `frame-display-record` (20 bytes from `0x95a69c`).
displayRecordOf :: Observation -> Either String DisplayRecord
displayRecordOf observation = case [bytes | (name, bytes) <- observationReads observation, name == "frame-display-record"] of
  [bytes] | B.length bytes == 20 -> DisplayRecord <$> word32 bytes 0 <*> word32 bytes 4 <*> word32 bytes 0xc <*> word32 bytes 0x10
  _ -> Left (T.unpack (observationPhase observation) ++ ": capture lacks the frame display record")

frameRecordOf :: FrameRole -> Observation -> Either String (Maybe FrameRecord)
frameRecordOf role observation = case (role, [bytes | (name, bytes) <- observationReads observation, name == "frame-record"]) of
  (_, []) -> Right Nothing
  (OwnedFrames, [bytes]) -> Just <$> parseFrameRecord bytes
  _ -> Left (T.unpack (observationPhase observation) ++ ": frame record read differs from the role")

-- | Each role's own laws over its phases:
--
-- * a step that publishes advances the front byte to the other buffer, publishes the image that
--   buffer names (and, on engine 1, leaves it in the shadow's low 30 bits), and takes one display
--   completion registration (`0x6d80f0`, or on owned frames `rb_render_frame_scanout_bridge`) and one
--   `0xfefe` post; a step that publishes nothing takes neither;
-- * consecutive steps continue from the front byte the previous one left;
-- * owned: an owned step advances the record's last owned sequence by one and leaves the published
--   buffer Queued or Scanning under that sequence; every capture with a record is drained.
checkRoleFrames :: FrameRole -> [PhaseFrames] -> Either String ()
checkRoleFrames role phases = do
  let steps = concatMap phaseSteps phases
  zipWithM_ (\previous next -> unless (stateFront (stepAfter previous) == stateFront (stepBefore next))
    (Left "a frame step starts from a front byte its predecessor did not leave")) steps (drop 1 steps)
  forM_ phases $ \phase -> do
    let where_ = T.unpack (phaseName phase) ++ ": "
    forM_ (phaseSteps phase) $ \step -> either (Left . (where_ ++)) Right (stepLaw role step)
    case (role, phaseFrameRecord phase) of
      (OwnedFrames, Just record) -> do
        let (first, second) = recordBufferStates record
        when (any (`elem` [BufferDrawing, BufferQueued]) [first, second] || recordDrainPending record)
          (Left (where_ ++ "owned publisher is not drained at the capture"))
      _ -> Right ()

stepLaw :: FrameRole -> FrameStep -> Either String ()
stepLaw role step = case stepPublished step of
  Nothing -> do
    unless (stepRegistrations step == 0 && stepPosts step == 0)
      (Left "a frame step without publication registered or posted a display completion")
    unless (stateFront (stepAfter step) == stateFront (stepBefore step))
      (Left "a frame step without publication moved the front byte")
  Just (engine, image) -> do
    let front = stateFront (stepAfter step)
        named = case front of
          EvenBuffer -> fst (stateImages (stepAfter step))
          OddBuffer -> snd (stateImages (stepAfter step))
        ImageAddress address = image
    unless (front == other (stateFront (stepBefore step))) (Left "a published frame did not alternate the front byte")
    unless (image == named) (Left "the published address is not the image the post-step front byte names")
    when (engine == 1 && stateShadow (stepAfter step) .&. 0x3fffffff /= address .&. 0x3fffffff)
      (Left "the engine-1 shadow does not hold the published address")
    unless (stepRegistrations step == 1) (Left "a published frame did not register exactly one display completion")
    unless (stepPosts step == 1) (Left "a published frame did not post exactly one 0xfefe")
    case (role, stepPath step, stateRecord (stepBefore step), stateRecord (stepAfter step)) of
      (OwnedFrames, OwnedPath, Just before, Just after) -> do
        unless (recordLastOwned after == recordLastOwned before + 1)
          (Left "an owned frame did not advance the record's sequence by one")
        let (state, ownedSequence) = case front of
              EvenBuffer -> (fst (recordBufferStates after), fst (recordBufferSequences after))
              OddBuffer -> (snd (recordBufferStates after), snd (recordBufferSequences after))
            otherState = case front of
              EvenBuffer -> snd (recordBufferStates after)
              OddBuffer -> fst (recordBufferStates after)
        unless (state `elem` [BufferQueued, BufferScanning] && ownedSequence == recordLastOwned after)
          (Left "the published buffer's record state does not hold the owned frame")
        when (otherState == BufferDrawing) (Left "the unpublished buffer is still drawing after the step")
      (OwnedFrames, StockPath, Just before, Just after) ->
        unless (recordLastOwned after == recordLastOwned before)
          (Left "a stock-path frame step advanced the owned sequence")
      _ -> Right ()

-- | The published frames, in order, that both roles must present.
presentedFrames :: PhaseFrames -> Either String [PresentedFrame]
presentedFrames phase = sequence
  [ case stepWait step of
      Just wait -> Right (PresentedFrame (stateFront (stepAfter step)) image wait)
      Nothing -> Left (T.unpack (phaseName phase) ++ ": a published frame has no wait result")
  | step <- phaseSteps phase, Just (_, image) <- [stepPublished step] ]

-- | The roles present the same frames per phase, take the same number of frame steps, and leave
-- equal display-record cells at each capture.
comparePresentation :: [PhaseFrames] -> [PhaseFrames] -> Either String ()
comparePresentation stock owned = do
  unless (map phaseName stock == map phaseName owned) (Left "frame phases differ between roles")
  forM_ (zip stock owned) $ \(left, right) -> do
    let where_ = T.unpack (phaseName left) ++ ": "
    unless (length (phaseSteps left) == length (phaseSteps right))
      (Left (where_ ++ show (length (phaseSteps left)) ++ " stock frame steps for " ++ show (length (phaseSteps right)) ++ " owned"))
    stockFrames <- presentedFrames left
    ownedFrames <- presentedFrames right
    unless (stockFrames == ownedFrames) (Left (where_ ++ "presented frame sequences differ"))
    unless (phaseDisplayRecord left == phaseDisplayRecord right) (Left (where_ ++ "display-record cells differ at the capture"))

-- | The stage-0 objects whose `+0x1c` words the receipt's `frame-step-entry` and
-- `frame-dispatch-return` sites read (`declared_trace_sites` of `e2e-result.json`), after checking
-- both sites declare the step reads in order; the owned frame package adds its 0x78-byte record.
declaredObjects :: FrameRole -> J.Value -> Either String StageObjects
declaredObjects role = J.parseEither $ J.withObject "e2e result" $ \fields -> do
  sites <- fields J..: "declared_trace_sites" :: J.Parser [J.Value]
  declared <- forM sites $ J.withObject "declared site" $ \site -> do
    name <- site J..: "label" :: J.Parser T.Text
    reads_ <- site J..:? "reads" J..!= [] :: J.Parser [J.Value]
    pure (name, reads_)
  stepReads <- forM ["frame-step-entry", "frame-dispatch-return"] $ \name -> case [reads_ | (label_, reads_) <- declared, label_ == name] of
    [reads_] -> mapM (J.withObject "declared read" $ \row -> (,) <$> (row J..: "address" >>= hex) <*> (row J..: "length" :: J.Parser Int)) reads_
    _ -> fail ("receipt declares no single " ++ T.unpack name ++ " site")
  case stepReads of
    [entry, returned] | entry == returned -> case (role, entry) of
      (_, [(0x8e21f8, 1), (0x8e2158, 8), (first, 4), (second, 4), (0x96bc10, 4)]) -> objects first second
      (OwnedFrames, [(0x8e21f8, 1), (0x8e2158, 8), (first, 4), (second, 4), (0x96bc10, 4), (_, 0x78)]) -> objects first second
      _ -> fail "frame step reads differ from the declared set"
    _ -> fail "frame step entry and dispatcher return read differently"
 where
  hex text = case readHex (T.unpack text) of
    [(value, "")] -> pure (value :: Word32)
    _ -> fail "declared read address is not hex"
  objects first second
    | first >= 0x1c && second >= 0x1c = pure (StageObjects (first - 0x1c) (second - 0x1c))
    | otherwise = fail "declared image reads precede their objects"
