{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}

-- | Frame presentation laws over synthetic receipts: the hit and read shapes of
-- `engine.jq`'s frame sites, two stage-0 images, and the owned frame record.
module Main (main) where

import Control.Monad (unless)
import qualified Data.Aeson as J
import Data.Bits (shiftR)
import qualified Data.ByteString as B
import Data.Either (isLeft, isRight)
import qualified Data.Text as T
import Data.Word (Word32, Word64)
import Numeric (showHex)
import Replacement.ModalRuntime.FramePresentation
import Replacement.ModalRuntime.MenuEnginePair
import System.Exit (exitFailure)

objectZero, objectOne, imageZero, imageOne :: Word32
objectZero = 0x00a7de28
objectOne = 0x00a7df58
imageZero = 0x01800000
imageOne = 0x01900000

objects :: StageObjects
objects = StageObjects objectZero objectOne

le32 :: Word32 -> B.ByteString
le32 value = B.pack [fromIntegral (value `shiftR` (8 * i)) | i <- [0 .. 3]]

le64 :: Word64 -> B.ByteString
le64 value = B.pack [fromIntegral (value `shiftR` (8 * i)) | i <- [0 .. 7]]

data Record = Record
  { lastOwned :: Word64, sequences :: (Word64, Word64), states :: (Word32, Word32)
  , drain :: Word32, ownedFrames :: Word32, failedWaits :: Word32, magic :: Word32 }

idle :: Record
idle = Record 0 (0, 0) (0, 0) 0 0 0 0x52464252

recordBytes :: Record -> B.ByteString
recordBytes record = B.concat
  [ le32 (magic record), le32 1, le64 0, le64 (lastOwned record), le64 (lastOwned record)
  , le64 (fst (sequences record)), le64 (snd (sequences record))
  , le32 (fst (states record)), le32 (snd (states record)), le32 (drain record)
  , le32 (ownedFrames record), le32 (failedWaits record), B.replicate 52 0 ]

stepMemory :: Word32 -> Word32 -> Maybe Record -> [B.ByteString]
stepMemory front shadow record =
  [B.singleton (fromIntegral front), le32 objectZero <> le32 objectOne, le32 imageZero, le32 imageOne, le32 shadow]
  ++ maybe [] (pure . recordBytes) record

hit :: T.Text -> Int -> (Word32, Word32, Word32, Word32) -> [B.ByteString] -> Hit
hit name core registers memory = Hit (OtherSite name) core registers Nothing Nothing memory Nothing Nothing

-- | One frame step that publishes the buffer after `front` and completes its wait, as the stock
-- body does.
publishedStep :: Word32 -> Maybe (Record, Record) -> [Hit]
publishedStep front records =
  let next = 1 - front
      image = if next == 0 then imageZero else imageOne
      (before, after) = maybe (Nothing, Nothing) (\(b, a) -> (Just b, Just a)) records
      owned = maybe False (\(b, a) -> ownedFrames a == ownedFrames b + 1) records
  in [ hit "frame-step-entry" 0 (0, 0, 0, 0) (stepMemory front 0 before)
     , hit "frame-item-configuration" 0 (1, 1, 0, image) [] ]
     ++ [hit "frame-display-registration" 1 (0, 0, 0, 0) [], hit "frame-display-post" 1 (0, 0, 0, 0) []]
     ++ [hit "frame-wait-return" 0 (0, 0, 0, 0) [] | not owned]
     ++ [hit "frame-dispatch-return" 0 (0, 0, 0, 0) (stepMemory next image after)]

displayCells :: B.ByteString
displayCells = B.concat (map le32 [1, 2, 3, 4, 5])

observation :: T.Text -> [Hit] -> [(T.Text, B.ByteString)] -> Observation
observation name hits extra = Observation name EngineResident ([("frame-display-record", displayCells)] ++ extra) hits Nothing

phaseOf :: FrameRole -> Observation -> Either String PhaseFrames
phaseOf role segment = PhaseFrames (observationPhase segment) <$> frameSteps role objects segment
  <*> displayRecordOf segment <*> frameRecordOf role segment

check :: String -> Bool -> IO Bool
check name good = putStrLn ((if good then "ok   " else "FAIL ") ++ name) >> pure good

stockPhase :: [Hit] -> Either String PhaseFrames
stockPhase hits = phaseOf StockFrames (observation "picker-route" hits [])

ownedRecords :: Word64 -> Word32 -> Word32 -> (Record, Record)
ownedRecords ownedSequence front frames =
  let next = 1 - front
      before = idle { lastOwned = ownedSequence, ownedFrames = frames }
      published = if next == 0 then ((ownedSequence + 1, 0), (2, 0)) else ((0, ownedSequence + 1), (0, 2))
  in (before, idle { lastOwned = ownedSequence + 1, ownedFrames = frames + 1, sequences = fst published, states = snd published })

ownedPhase :: [Hit] -> Record -> Either String PhaseFrames
ownedPhase hits capture = phaseOf OwnedFrames (observation "picker-route" hits [("frame-record", recordBytes capture)])

declaredResult :: [J.Value] -> J.Value
declaredResult reads_ = J.object ["declared_trace_sites" J..=
  [ J.object ["address" J..= ("006d7920" :: T.Text), "label" J..= ("frame-step-entry" :: T.Text), "reads" J..= reads_]
  , J.object ["address" J..= ("006d6f08" :: T.Text), "label" J..= ("frame-dispatch-return" :: T.Text), "reads" J..= reads_] ]]

declaredRead :: Word32 -> Int -> J.Value
declaredRead address bytes = J.object ["address" J..= (replicate (8 - length digits) '0' ++ digits), "length" J..= bytes]
  where digits = showHex address ""

main :: IO ()
main = do
  let twoFrames = publishedStep 0 Nothing ++ publishedStep 1 Nothing
      valid = stockPhase twoFrames
      stockValid = either (const []) pure valid
  results <- sequence
    [ check "two alternating stock frames satisfy the role laws" (fmap (checkRoleFrames StockFrames . pure) valid == Right (Right ()))
    , check "presented frames name the buffer the front byte advanced to"
        ((valid >>= presentedFrames) == Right [PresentedFrame OddBuffer (ImageAddress imageOne) WaitSucceeded, PresentedFrame EvenBuffer (ImageAddress imageZero) WaitSucceeded])
    , check "a published address other than the front buffer's image refuses"
        (refusesRole StockFrames (stockPhase (replaceRegisters "frame-item-configuration" (1, 1, 0, imageZero) (publishedStep 0 Nothing))))
    , check "a published frame without its 0xfefe post refuses"
        (refusesRole StockFrames (stockPhase (filter ((/= OtherSite "frame-display-post") . hitSite) (publishedStep 0 Nothing))))
    , check "a step that does not continue from the previous front byte refuses"
        (refusesRole StockFrames (stockPhase (publishedStep 0 Nothing ++ publishedStep 0 Nothing)))
    , check "a failed event wait is presented as failed"
        ((stockPhase (replaceRegisters "frame-wait-return" (0xffffff01, 0, 0, 0) (publishedStep 0 Nothing)) >>= presentedFrames)
          == Right [PresentedFrame OddBuffer (ImageAddress imageOne) WaitFailed])
    , check "a display completion outside any frame step refuses"
        (isLeft (stockPhase (hit "frame-display-post" 1 (0, 0, 0, 0) [] : twoFrames)))
    , check "dispatcher returns of other actions are not frame steps"
        (fmap (length . phaseSteps) (stockPhase (hit "frame-dispatch-return" 0 (0, 0, 0, 0) (stepMemory 0 0 Nothing) : twoFrames)) == Right 2)
    , check "stage objects other than the declared ones refuse"
        (isLeft (phaseOf StockFrames (observation "x" (replaceMemory "frame-step-entry" (\memory -> take 1 memory ++ [le32 0 <> le32 objectOne] ++ drop 2 memory) twoFrames) [])))
    , check "owned frames with a one-step sequence and queued buffer satisfy the owned laws"
        (fmap (checkRoleFrames OwnedFrames . pure) (ownedPhase (owned 7 0 3 ++ owned 8 1 4) idle) == Right (Right ()))
    , check "owned and stock roles presenting the same frames compare equal"
        (fmap (\o -> comparePresentation stockValid [o]) (ownedPhase (owned 7 0 3 ++ owned 8 1 4) idle) == Right (Right ()))
    , check "an owned sequence advancing by two refuses"
        (refusesRole OwnedFrames (ownedPhase (publishedStep 0 (Just (let (b, a) = ownedRecords 7 0 3 in (b, a { lastOwned = 9, sequences = (0, 9) })))) idle))
    , check "an owned published buffer left Drawing refuses"
        (refusesRole OwnedFrames (ownedPhase (publishedStep 0 (Just (let (b, a) = ownedRecords 7 0 3 in (b, a { states = (0, 1) })))) idle))
    , check "an owned capture with a queued buffer refuses as undrained"
        (refusesRole OwnedFrames (ownedPhase (owned 7 0 3) idle { states = (0, 2) }))
    , check "differing frame-step counts refuse the pair"
        (fmap (\o -> comparePresentation stockValid [o]) (ownedPhase (owned 7 0 3) idle) /= Right (Right ()))
    , check "differing display-record cells refuse the pair"
        (let changed = phaseOf OwnedFrames (Observation "picker-route" EngineResident [("frame-display-record", B.replicate 20 9), ("frame-record", recordBytes idle)] (owned 7 0 3 ++ owned 8 1 4) Nothing)
         in fmap (\o -> comparePresentation stockValid [o]) changed /= Right (Right ()))
    , check "a frame record without its magic refuses" (isLeft (parseFrameRecord (recordBytes idle { magic = 0 })))
    , check "a frame record with an unknown buffer state refuses" (isLeft (parseFrameRecord (recordBytes idle { states = (4, 0) })))
    , check "declared step reads yield the objects whose images are read"
        (declaredObjects StockFrames (declaredResult stockReads) == Right objects)
    , check "an owned receipt may add the frame record to the step reads"
        (declaredObjects OwnedFrames (declaredResult (stockReads ++ [declaredRead 0x00b00000 0x78])) == Right objects)
    , check "a stock receipt reading a frame record refuses"
        (isLeft (declaredObjects StockFrames (declaredResult (stockReads ++ [declaredRead 0x00b00000 0x78]))))
    , check "step reads out of order refuse" (isLeft (declaredObjects StockFrames (declaredResult (reverse stockReads))))
    , check "synthetic phases are well formed" (isRight valid)
    ]
  unless (and results) exitFailure
  putStrLn ("frame presentation tests passed: " ++ show (length results))
 where
  owned ownedSequence front frames = publishedStep front (Just (ownedRecords ownedSequence front frames))
  stockReads = [declaredRead 0x008e21f8 1, declaredRead 0x008e2158 8, declaredRead (objectZero + 0x1c) 4, declaredRead (objectOne + 0x1c) 4, declaredRead 0x0096bc10 4]
  refusesRole role phase = case phase of
    Right built -> isLeft (checkRoleFrames role [built])
    Left _ -> True
  replaceRegisters name registers = map (\h -> if hitSite h == OtherSite name then h { hitRegisters = registers } else h)
  replaceMemory name f = map (\h -> if hitSite h == OtherSite name then h { hitMemory = f (hitMemory h) } else h)
