{-# LANGUAGE OverloadedStrings #-}
-- | Synthetic container, after host/firmware/UpdateContainerTests.hs
-- fixtureBytesAt, with loaders A and B at the X-M1 load addresses and extents.
-- Pattern bytes only; no firmware content.
module Fixtures (fixtureDat, fixtureImage4, fixtureImage5, patternBytes) where

import Data.Bits (complement)
import qualified Data.ByteString as B
import Data.Word (Word32)
import qualified FujiAF.UpdateContainer as U

record4Media, record5Media, plainMedia, image2Media, image2Size :: Int
record4Media = 0x140400
record5Media = 0x140440
plainMedia = 0x160000
image2Media = 0x4d0000
image2Size = 0x200

patternBytes :: Int -> B.ByteString
patternBytes n = B.pack [fromIntegral ((i * 7 + 3) `mod` 251) | i <- [0 .. n - 1]]

patchMediaWord :: Int -> Word32 -> B.ByteString -> B.ByteString
patchMediaWord off w bytes = B.take off bytes <> U.putMediaWord w <> B.drop (off + 4) bytes

segments :: [U.SegmentEntry]
segments =
  [ U.SegmentEntry 0 0x40000 0xdae0 0 0 0x60000 0x10000 0 0
  , U.SegmentEntry 1 0xd0000 0xdaa0 0 0 0x70000 0x10000 0 0
  , U.SegmentEntry 2 0x3000 (fromIntegral image2Size) 0 0 (fromIntegral image2Media) 0x10000 0 0
  , U.SegmentEntry 3 0x4000 0x200 0 0 0x33000 0x10000 0 0
  , U.SegmentEntry 4 0x61c400 0x282828 0x89ec00 0x352e00 (fromIntegral record4Media) (fromIntegral (record5Media - record4Media)) 0x20 1
  , U.SegmentEntry 5 0xbf1a00 0x4f1e60 0x1460000 0x2bbe40 (fromIntegral record5Media) (fromIntegral (plainMedia - record5Media)) 0x20 1
  , U.SegmentEntry 6 0xbf1a00 0x4f1e60 0x1460000 0x2bbe40 (fromIntegral plainMedia) 0x360000 0x20 0
  , U.SegmentEntry 7 0 0 0 0 0xb40000 0x20000 0 0
  ]

record4, record5 :: U.CompressedRecord
record4 = U.CompressedRecord (U.RecordHeader 0x282800 0x34 1 0x4000 4) (patternBytes 0x20) Nothing
record5 = U.CompressedRecord (U.RecordHeader 0x4f1e60 0x4015 2 0x4000 4) (patternBytes 0x4001) Nothing

-- | Decoded images matching the records' declared sizes.
fixtureImage4, fixtureImage5 :: B.ByteString
fixtureImage4 = B.replicate 0x282800 0x5a
fixtureImage5 = B.replicate 0x4f1e60 0xa5

tableEnd :: Int
tableEnd = U.tableEndOffset U.Stock

tableTail :: B.ByteString
tableTail =
  foldl
    (\t (off, w) -> patchMediaWord (off - tableEnd) w t)
    (patternBytes (record4Media - tableEnd))
    ( [(0x140184,0x1234),(0x1401a4,0x4061c1b0), U.stockPlainLoad, (0x1401ac,0x360000)]
        ++ zip [0x1401b0,0x1401b4 .. 0x1401f8]
          [0,0x01100000,0xac5a0,0,0,0x44,2,0x011c0000,0x1cca0,0,0,0x46,3,0x011e0000,0x26a5e0,0,0,0x47,0xffffffff]
    )

serializeEntry :: U.SegmentEntry -> B.ByteString
serializeEntry e = B.concat (map U.putMediaWord
  [U.segId e, U.segLoad e, U.segSize e, U.segBss e, U.segBssLen e, U.segFlash e, U.segFlashLen e, U.segWord7 e, U.segFlag e])

serializeRecordBytes :: U.CompressedRecord -> B.ByteString
serializeRecordBytes r = B.concat
  [ U.putMediaWord (U.recDecodedSize (U.recHeader r)), U.putMediaWord (U.recTotalLen (U.recHeader r))
  , U.putMediaWord (U.recChunkCount (U.recHeader r)), U.putMediaWord (U.recChunkSize (U.recHeader r))
  , U.putMediaWord (U.recMode (U.recHeader r)), U.recStream r ]

bodyPrefix :: B.ByteString
bodyPrefix = B.concat
  [ foldl (\bytes (off, value) -> patchMediaWord off value bytes)
      (patternBytes (U.segmentTableOffset U.Stock))
      [(0x140000,0x4061c040),(0x140004,0x4061c1a4),(0x140008,0),(0x14000c,0x4061c184)]
  , B.concat (map serializeEntry segments)
  , U.putMediaWord 0xffffffff
  , tableTail
  , serializeRecordBytes record4
  , patternBytes (record5Media - record4Media - B.length (serializeRecordBytes record4))
  , serializeRecordBytes record5
  , patternBytes (image2Media - record5Media - B.length (serializeRecordBytes record5))
  , B.map complement (B.pack [fromIntegral ((i * 5 + 7) `mod` 251) | i <- [0 .. image2Size - 1]])
  , patternBytes 5
  ]

body :: B.ByteString
body = bodyPrefix <> B.concat
  [ U.putMediaWord (fromIntegral (B.length bodyPrefix)), U.putMediaWord (fromIntegral (B.length bodyPrefix))
  , U.putMediaWord 0x20, U.putMediaWord 0, U.putMediaWord 0
  , U.putMediaWord (U.segLoad (segments !! 4)), U.putMediaWord (U.segSize (segments !! 4))
  , U.putMediaHalf 0xffff, U.putMediaHalf 0xaaaa ]

fixtureDat :: B.ByteString
fixtureDat = B.concat
  [ U.putWord32 4, identity, U.putWord32 1, U.putWord32 0x50, U.putWord32 (mainSum body), U.putWord32 1
  , B.replicate 0x28 0xff, U.putWord32 0, U.putWord32 mainLength, U.putWord32 mainLength
  , U.putWord32 aux1Length, U.putWord32 (auxSum aux1), U.putWord32 (mainLength + aux1Length)
  , U.putWord32 (fromIntegral (B.length aux2)), U.putWord32 (auxSum aux2)
  , body, aux1, aux2 ]
  where
    identity = "IDENTITYRECORD01" <> patternBytes (U.identityBytes - 16)
    aux1 = B.concat [U.putWord16 (U.pair16Offset p) <> U.putWord16 (U.pair16Length p) | p <- U.stockAux1]
    aux2 = B.concat [U.putWord32 (U.pair32Offset p) <> U.putWord32 (U.pair32Length p) | p <- U.stockAux2]
    mainLength = fromIntegral (B.length body)
    aux1Length = fromIntegral (B.length aux1)
    mainSum bytes = complement (B.foldl' (\acc b -> acc + fromIntegral (complement b)) 0 bytes)
    auxSum bytes = complement (B.foldl' (\acc b -> acc + fromIntegral b) 0 bytes)
