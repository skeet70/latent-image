{-# LANGUAGE OverloadedStrings #-}
-- Behavioural tests for first-record staging in the opaque pre-table part;
-- fixtures are synthetic Parts values and no private container is read.
module Main where

import Control.Monad (unless)
import qualified Data.ByteString as B
import Data.Bits (complement)
import FujiAF.FirstStage
import qualified FujiAF.Host as H
import qualified FujiAF.UpdateContainer as U

syntheticParts :: B.ByteString -> U.Parts
syntheticParts preTable =
  U.Parts
    { U.partIdentity = B.cons 1 (B.replicate 511 0)
    , U.partVersionMajor = 1
    , U.partVersionMinor = 0
    , U.partSelector = 1
    , U.partRangeFill = B.replicate 40 0xff
    , U.partPreTable = preTable
    , U.partTableLayout = U.Stock
    , U.partSegments = [segment k | k <- [0 .. 7]]
    , U.partTableTail = B.empty
    , U.partRecord4 = record
    , U.partGap45 = B.empty
    , U.partRecord5 = record
    , U.partPostHead = B.empty
    , U.partImage2 = B.singleton 0
    , U.partPostTail = B.empty
    , U.partTrailer = U.Trailer 0 0 0 0 0 0 0 0 0xaaaa
    , U.partAux1 = []
    , U.partAux2 = []
    }
  where
    segment k =
      U.SegmentEntry
        { U.segId = k
        , U.segLoad = 0
        , U.segSize = 0
        , U.segBss = 0
        , U.segBssLen = 0
        , U.segFlash = 0
        , U.segFlashLen = 0
        , U.segWord7 = 0
        , U.segFlag = 0
        }
    record = U.CompressedRecord (U.RecordHeader 0 20 1 0x4000 4) B.empty Nothing

sourcePreTable :: B.ByteString
sourcePreTable =
  payload
  where
    size = 0x140040 :: Int
    base = B.replicate size 0x11
    withSlot = B.take 0x40000 base <> B.replicate 0x10000 0 <> B.drop 0x50000 base
    withRecord2 = replace 0x50000 (B.pack [0xb6, 0xb6, 0xb6, 0xb6]) withSlot
    payload = replace 0x60000 (B.pack [0xc7, 0xc7]) withRecord2
    replace offset bytes = \value -> B.take offset value <> bytes <> B.drop (offset + B.length bytes) value

plan :: B.ByteString -> FirstStagePlan
plan image =
  FirstStagePlan
    { planEntry = 0x6000
    , planLoadOffset = 0x6000
    , planImage = U.MediaImage image
    , planRetainedExtents = [(0, 0x5280)]
    }

assertTrue :: String -> Bool -> IO ()
assertTrue label ok = unless ok (H.failH (label ++ ": failed"))

assertLeft :: Show a => String -> Either String a -> IO ()
assertLeft label result = case result of
  Left _ -> pure ()
  Right value -> H.failH (label ++ ": expected refusal, got " ++ show value)

main :: IO ()
main = do
  let parts = syntheticParts sourcePreTable
      image = B.pack [0xde, 0xad, 0xbe, 0xef, 0x01, 0x02, 0x03, 0x04]
  slot <- either H.failH pure (firstRecordSlotBytes parts)
  assertTrue "slot length" (B.length slot == firstRecordSlotLength)
  assertTrue "source record 2 marker" (B.index sourcePreTable 0x50000 == 0xb6)
  staged <- either H.failH pure (replaceFirstRecordSlot (plan image) parts)
  let stagedPre = U.partPreTable staged
      stagedSlot = B.take firstRecordSlotLength (B.drop firstRecordSlotOffset stagedPre)
  assertTrue "prefix before slot" (B.take firstRecordSlotOffset stagedPre == B.take firstRecordSlotOffset sourcePreTable)
  assertTrue "suffix after slot" (B.drop (firstRecordSlotOffset + firstRecordSlotLength) stagedPre == B.drop (firstRecordSlotOffset + firstRecordSlotLength) sourcePreTable)
  assertTrue "record 2 marker" (B.take 4 (B.drop 0x50000 stagedPre) == B.pack [0xb6, 0xb6, 0xb6, 0xb6])
  assertTrue "carrier marker" (B.take 2 (B.drop 0x60000 stagedPre) == B.pack [0xc7, 0xc7])
  assertTrue "reset target" (U.mediaWordAt stagedSlot resetVectorWordOffset == 0x6000)
  assertTrue "retained body below image" (B.take 0x20 stagedSlot == B.take 0x20 slot)
  assertTrue "retained body between vector and image" (B.drop 0x24 (B.take (planLoadOffset (plan image)) stagedSlot) == B.drop 0x24 (B.take (planLoadOffset (plan image)) slot))
  assertTrue "image bytes" (B.map complement (B.take (B.length image) (B.drop (planLoadOffset (plan image)) stagedSlot)) == image)
  assertTrue "unrelated parts unchanged" (U.partIdentity staged == U.partIdentity parts && U.partSelector staged == U.partSelector parts)
  assertLeft "empty image" (replaceFirstRecordSlot (plan B.empty) parts)
  assertLeft "entry outside image" (replaceFirstRecordSlot ((plan image) {planEntry = 0x6400}) parts)
  assertLeft "unaligned entry" (replaceFirstRecordSlot ((plan image) {planEntry = 0x6002}) parts)
  assertLeft "image past slot" (replaceFirstRecordSlot ((plan image) {planLoadOffset = 0xfffc}) parts)
  assertLeft "image overlaps retained" (replaceFirstRecordSlot ((plan image) {planLoadOffset = 0x5000, planEntry = 0x5000}) parts)
  assertLeft "image overlaps vector" (replaceFirstRecordSlot ((plan image) {planLoadOffset = 0x20, planEntry = 0x20}) parts)
  assertLeft "retained extent outside slot" (replaceFirstRecordSlot ((plan image) {planRetainedExtents = [(0x10000, 0x20000)]}) parts)
  assertLeft "short carrier" (firstRecordSlotBytes (parts {U.partPreTable = B.replicate 0x40000 0}))
  assertTrue "patch order is vector then image" $
    case planPatches (plan image) of
      Right [(offset, bytes), (loadOffset, stagedBytes)] -> offset == resetVectorWordOffset && B.length bytes == 4 && loadOffset == 0x6000 && stagedBytes == B.map complement image
      _ -> False
  putStrLn "first-stage staging tests: 20 assertions pass"
