{-# LANGUAGE OverloadedStrings #-}
module FujiAF.AfRing
  ( AfEventKind(..), EventSequence, unEventSequence, EventCount, unEventCount
  , ObservedWord(..), AfEventCore(..), AfEvent(..), AfRingCapture, ringCommittedTotal, ringOverwritten, ringEvents
  , ContentionStatus(..), ringContentionStatus, ringContentionLoss
  , HeaderField(..), RingSlot, unRingSlot, AfRingError(..), AfRingReadError(..)
  , AfRingSource(..), afRingOffset, afRingBytes, afRingCapacity
  , decodeAfRing, afRingValue, afRingWindow, readAfRing
  ) where

import Control.Exception (Exception, throwIO)
import Control.Monad (forM, unless)
import Data.Aeson (Value, object, (.=))
import Data.Bits ((.&.), shiftL)
import qualified Data.ByteString as B
import Data.Word (Word32)
import System.FilePath ((</>))
import FujiAF.RamPush
import FujiAF.ReadBulk
import FujiAF.Transport (Clock, UsbApi)

newtype EventSequence = EventSequence { unEventSequence :: Word32 } deriving (Eq, Ord, Show)
newtype EventCount = EventCount { unEventCount :: Word32 } deriving (Eq, Ord, Show)
newtype ObservedWord = ObservedWord { unObservedWord :: Word32 } deriving (Eq, Show)
newtype RingSlot = RingSlot { unRingSlot :: Int } deriving (Eq, Show)
data AfEventKind = DirectionDecision | PeakDecision | AcceptedStatistic | Synthetic deriving (Eq, Show)
data AfEventCore = AfCpu0 deriving (Eq, Show)
data ContentionStatus = NoContentionLoss | ContentionLossDetected deriving (Eq, Show)
data AfEvent = AfEvent
  { eventSequence :: EventSequence
  , eventKind :: AfEventKind
  , eventR0 :: ObservedWord
  , eventR1 :: ObservedWord
  , eventR2 :: ObservedWord
  , eventR3 :: ObservedWord
  , eventSearchState :: ObservedWord
  , eventFocusStatistic :: ObservedWord
  , eventCore :: AfEventCore
  } deriving (Eq, Show)
data AfRingCapture = AfRingCapture
  { ringCommittedTotal :: EventCount
  , ringOverwritten :: EventCount
  , ringContentionStatus :: ContentionStatus
  , ringEvents :: [AfEvent]
  } deriving (Eq, Show)
data HeaderField = Magic | Version | HeaderBytes | EventBytes | Capacity | WriterActive | CoreScope | ContentionLoss | Reserved deriving (Eq, Show)
data AfRingError
  = RingExtentMismatch Int
  | HeaderMismatch HeaderField ObservedWord
  | ActiveWriter
  | OverwrittenMismatch EventCount EventCount
  | SlotSequenceMismatch RingSlot EventSequence ObservedWord
  | NonzeroUnusedSlot RingSlot
  | UnknownEventKind RingSlot ObservedWord
  | UnexpectedEventCore RingSlot ObservedWord
  deriving (Eq, Show)
instance Exception AfRingError

data AfRingSource
  = AfRingManifest RamPushManifest
  | AfRingAllocation RamAllocState
  | AfRingState RamPushState
  deriving (Eq, Show)
data AfRingReadError
  = NotAfRingAllocation Int
  | AfRingEntriesMismatch
  | AfRingCoreMismatch ObservedWord
  | AfRingBaseInvalid Address
  | AfRingWindowRejected String
  deriving (Eq, Show)
instance Exception AfRingReadError

afRingOffset, afRingBytes, afRingCapacity :: Int
afRingOffset = 0x4000
afRingBytes = 0x2440
afRingCapacity = 256

-- Committed event slots stop changing at sequence 0xffffffff.
decodeAfRing :: B.ByteString -> Either AfRingError AfRingCapture
decodeAfRing bytes = do
  unless (B.length bytes == afRingBytes) (Left (RingExtentMismatch (B.length bytes)))
  mapM_ (\(field,at,wanted) -> unless (word at == wanted) (Left (HeaderMismatch field (ObservedWord (word at)))))
    [(Magic,0,0x31524641), (Version,4,1), (HeaderBytes,8,64), (EventBytes,12,36), (Capacity,16,256), (CoreScope,32,0x4003)]
  case word 28 of
    0 -> pure ()
    1 -> Left ActiveWriter
    other -> Left (HeaderMismatch WriterActive (ObservedWord other))
  contention <- case word 36 of
    0 -> Right NoContentionLoss
    1 -> Right ContentionLossDetected
    other -> Left (HeaderMismatch ContentionLoss (ObservedWord other))
  mapM_ (\at -> unless (word at == 0) (Left (HeaderMismatch Reserved (ObservedWord (word at))))) [40,44..60]
  let total = word 20
      overwritten = word 24
      retained = min total (fromIntegral afRingCapacity)
      expectedOverwritten = total - retained
  unless (overwritten == expectedOverwritten)
    (Left (OverwrittenMismatch (EventCount expectedOverwritten) (EventCount overwritten)))
  events <- forM [toInteger total - toInteger retained + 1 .. toInteger total] $ \number -> do
    let sequenceNumber = fromInteger number
        slot = fromIntegral ((sequenceNumber - 1) .&. 255)
        at = 64 + 36*slot
        expected = EventSequence sequenceNumber
    unless (word at == sequenceNumber)
      (Left (SlotSequenceMismatch (RingSlot slot) expected (ObservedWord (word at))))
    kind <- case word (at+4) of
      1 -> Right DirectionDecision
      2 -> Right PeakDecision
      3 -> Right AcceptedStatistic
      0x80000001 -> Right Synthetic
      other -> Left (UnknownEventKind (RingSlot slot) (ObservedWord other))
    unless (word (at+32) == 0x4003)
      (Left (UnexpectedEventCore (RingSlot slot) (observed (at+32))))
    pure (AfEvent expected kind (observed (at+8)) (observed (at+12)) (observed (at+16))
      (observed (at+20)) (observed (at+24)) (observed (at+28)) AfCpu0)
  mapM_ (\slot -> unless (B.all (== 0) (B.take 36 (B.drop (64+36*slot) bytes)))
    (Left (NonzeroUnusedSlot (RingSlot slot)))) [fromIntegral retained .. afRingCapacity-1]
  pure (AfRingCapture (EventCount total) (EventCount overwritten) contention events)
  where
    word :: Int -> Word32
    word at = foldr (\index acc -> acc + (fromIntegral (B.index bytes (at+index)) `shiftL` (8*index))) 0 [0..3]
    observed :: Int -> ObservedWord
    observed = ObservedWord . word

ringContentionLoss :: AfRingCapture -> Bool
ringContentionLoss capture = ringContentionStatus capture == ContentionLossDetected

afRingWindow :: AfRingSource -> Either AfRingReadError BulkWindow
afRingWindow source = do
  let (base,size) = case source of
        AfRingManifest manifest -> (allocationBase manifest, allocationSize manifest)
        AfRingAllocation allocation -> (allocBase allocation, allocSize allocation)
        AfRingState state -> (stateBase state, stateSize state)
  case payloadOfSize size of
    Just payload | payloadKind payload == AfRing -> pure ()
    _ -> Left (NotAfRingAllocation size)
  let expectedEntries = payloadEntries afRingPayload base
  case source of
    AfRingManifest manifest -> unless (manifestEntries manifest == expectedEntries) (Left AfRingEntriesMismatch)
    AfRingState state -> unless (stateEntries state == expectedEntries) (Left AfRingEntriesMismatch)
    AfRingAllocation allocation -> unless (allocCpu0 allocation == 0x4003) (Left (AfRingCoreMismatch (ObservedWord (allocCpu0 allocation))))
  let address = unAddress base
  unless (address /= 0 && address .&. 7 == 0 && toInteger address + toInteger size <= 0x100000000)
    (Left (AfRingBaseInvalid base))
  either (Left . AfRingWindowRejected) Right (mkBulkWindow (fromIntegral address + afRingOffset) afRingBytes)

-- One bulk capture is non-atomic; matching slot sequences do not establish coherence.
readAfRing :: AfRingSource -> Clock -> UsbApi -> FilePath -> IO AfRingCapture
readAfRing source clock api directory = do
  window <- either throwIO pure (afRingWindow source)
  _ <- runReadBulk window clock api directory
  bytes <- B.readFile (directory </> bulkCaptureName window)
  either throwIO pure (decodeAfRing bytes)

afRingValue :: AfRingCapture -> Value
afRingValue capture = object
  [ "schema" .= ("xm1-af-ring-1" :: String)
  , "capture_semantics" .= ("best-effort capture; not a coherent snapshot" :: String)
  , "timestamp_available" .= False
  , "version" .= (1 :: Int), "header_bytes" .= (64 :: Int), "event_bytes" .= (36 :: Int)
  , "capacity" .= afRingCapacity, "core_scope" .= (0x4003 :: Word32)
  , "search_state_address" .= (0x016c0fc0 :: Word32)
  , "focus_statistic_address" .= (0x016c1160 :: Word32)
  , "committed_total" .= unEventCount (ringCommittedTotal capture)
  , "overwritten" .= unEventCount (ringOverwritten capture)
  , "contention_loss" .= ringContentionLoss capture
  , "overflowed" .= (unEventCount (ringOverwritten capture) /= 0)
  , "saturated" .= (unEventCount (ringCommittedTotal capture) == maxBound)
  , "events" .= map eventValue (ringEvents capture)
  ]
  where
    eventValue event =
      let (code,name,hook) = case eventKind event of
            DirectionDecision -> (1, "direction-decision", Just 0x00f41c48)
            PeakDecision -> (2, "peak-decision", Just 0x00f42abc)
            AcceptedStatistic -> (3, "accepted-statistic-consumer", Just 0x00f44fec)
            Synthetic -> (0x80000001, "synthetic", Nothing)
      in object
        [ "sequence" .= unEventSequence (eventSequence event)
        , "kind" .= (name :: String), "kind_code" .= (code :: Word32), "hook_address" .= (hook :: Maybe Word32)
        , "core" .= (case eventCore event of AfCpu0 -> "cpu0" :: String)
        , "core_identity" .= (case eventCore event of AfCpu0 -> 0x4003 :: Word32)
        , "r0" .= unObservedWord (eventR0 event), "r1" .= unObservedWord (eventR1 event)
        , "r2" .= unObservedWord (eventR2 event), "r3" .= unObservedWord (eventR3 event)
        , "search_state" .= unObservedWord (eventSearchState event)
        , "focus_statistic" .= unObservedWord (eventFocusStatistic event)
        ]
