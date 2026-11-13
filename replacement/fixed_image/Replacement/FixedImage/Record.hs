{-# LANGUAGE OverloadedStrings #-}
module Replacement.FixedImage.Record
  ( BuildId(..), BodyBounds(..), Bss(..), Publication(..), Image(..), publication, applyPublications
  , makeRecord, crc32, stageRecord, restoreRecord, ceilingPublications, reservationPublications, corruptRecord
  ) where

import Control.Monad (foldM, unless)
import qualified Data.ByteString as B
import Data.Bits (complement, shiftL, shiftR, xor, (.&.), (.|.))
import Data.List (nub)
import Data.Word (Word32)
import qualified FujiAF.UpdateContainer as U
import Replacement.FixedImage.Layout

newtype BuildId = BuildId Word32 deriving (Eq, Show)
data BodyBounds = BodyBounds Word32 Word32 deriving (Eq, Show)
data Bss = Bss Word32 Word32 deriving (Eq, Show)
data Image = Image4 | Image5 | Plain deriving (Eq, Show)
data Publication = Publication Image Word32 Word32 Word32 deriving (Eq, Show)

publication :: Word32 -> Word32 -> Word32 -> Either String Publication
publication address old new = do
  unless (address .&. 3 == 0 && old /= new) (Left "publication is unaligned or unchanged")
  image <- if address >= 0x61c400 && address + 4 <= 0x89ec00 then Right Image4
    else if address >= 0xbf1a00 && address + 4 <= 0x10c1a20 then Right Image5
    else if address >= 0x1100000 && address + 4 <= 0x1180000 then Right Plain
    else Left "publication leaves the admitted image windows"
  pure (Publication image address old new)

ceilingPublications :: [Publication]
ceilingPublications =
  [ Publication Image4 0x72b12c 0xe3013a00 (instruction 0xe3003000 (imageBase .&. 0xffff))
  , Publication Image4 0x72b130 0xe34030bf (instruction 0xe3403000 (imageBase `shiftR` 16))
  ]

reservationPublications :: [Publication]
reservationPublications = ceilingPublications ++
  [ Publication Image4 0x69ce18 0xe3022e00 (instruction 0xe3002000 (clearBytes .&. 0xffff))
  , Publication Image4 0x69ce1c 0xe3402035 (instruction 0xe3402000 (clearBytes `shiftR` 16))
  ]
  where
    clearBytes = imageBase - 0x89ec00

instruction :: Word32 -> Word32 -> Word32
instruction opcode immediate = opcode .|. ((immediate .&. 0xf000) `shiftL` 4) .|. (immediate .&. 0xfff)

applyPublications :: Image -> [Publication] -> B.ByteString -> Either String B.ByteString
applyPublications image rows bytes = do
  unless (length addresses == length (nub addresses)) (Left "duplicate publication address")
  whenImage4
  foldM apply bytes selected
  where
    selected = [(address, old, new) | Publication target address old new <- rows, target == image]
    addresses = [address | (address, _, _) <- selected]
    base = case image of Image4 -> 0x61c400; Image5 -> 0xbf1a00; Plain -> 0x1100000
    apply current (address, old, new) = do
      let offset = fromIntegral (address - base)
      unless (offset + 4 <= B.length current) (Left "publication leaves decoded image")
      unless (U.word32At current offset == old) (Left ("publication preimage differs at " ++ U.showHexWord address))
      pure (B.take offset current <> U.putWord32 new <> B.drop (offset + 4) current)
    whenImage4 = unless (image /= Image4) $ mapM_ checkHeap
      [offset | (low, high) <- [(0x89cda4,0x89d4f8),(0x89d4fc,0x89d908),(0x89da3c,0x89dfc0),(0x89e464,0x89e7c4)]
      , address <- [low,low+4..high-56], let offset = address - 0x61c400
      , offset + 56 <= B.length bytes
      , map (U.word32At bytes . (offset +)) [0,4,8] == [0x48454150,1,0xa71a00]]
    checkHeap offset = unless (U.word32At bytes (offset+12) <= imageBase)
      (Left "owned heap arena overlaps the fixed image reservation; link general-heap-fixed-image")

crc32 :: B.ByteString -> Word32
crc32 = complement . B.foldl' byte 0xffffffff
  where
    byte crc value = iterate bit (crc `xor` fromIntegral value) !! 8
    bit crc = (crc `shiftR` 1) `xor` (if crc .&. 1 == 0 then 0 else 0xedb88320)

makeRecord :: BuildId -> BodyBounds -> Bss -> B.ByteString -> Either String B.ByteString
makeRecord (BuildId build) (BodyBounds first lastByte) (Bss begin end) body = do
  let size = headerBytes + fromIntegral (B.length body)
  unless (first == imageBase + headerBytes && toInteger lastByte == toInteger first + toInteger (B.length body))
    (Left "body symbols do not describe the supplied fixed-region binary")
  unless (not (B.null body) && toInteger (B.length body) + toInteger headerBytes <= toInteger (imageLimit - imageBase)) (Left "fixed image body exceeds its reserved range or is empty")
  unless (begin >= imageBase + size && begin <= end && end <= imageLimit && begin .&. 3 == 0 && end .&. 3 == 0) (Left "BSS bounds overlap file bytes or leave the fixed range")
  pure (B.concat (map U.putWord32 [recordMagic, imageBase, size, crc32 body, build, begin, end, 0]) <> body)

recordOffset :: U.Parts -> Either String Int
recordOffset parts = do
  let segment = [entry | entry <- U.partSegments parts, U.segId entry == 5]
  start <- case segment of
    [entry] -> pure (U.segFlash entry + fromIntegral U.recordHeaderBytes + fromIntegral (B.length (U.recStream (U.partRecord5 parts))))
    _ -> Left "container lacks unique image 5"
  unless (recordFlash >= start && recordFlash + recordBlocks * blockBytes <= start + fromIntegral (B.length (U.partPostHead parts))) (Left "fixed record is outside container post-head fill")
  pure (fromIntegral (recordFlash - start))

stageRecord :: B.ByteString -> U.Parts -> Either String U.Parts
stageRecord record parts = do
  offset <- recordOffset parts
  let spanBytes = fromIntegral (recordBlocks * blockBytes)
      source = U.partPostHead parts
  unless (B.length record >= fromIntegral headerBytes && B.length record <= spanBytes) (Left "record exceeds two blocks or lacks header")
  unless (B.all (== 0) (B.take spanBytes (B.drop offset source))) (Left "fixed record carrier is not shipped fill")
  let padded = record <> B.replicate (spanBytes - B.length record) 0xff
  pure parts { U.partPostHead = B.take offset source <> B.map complement padded <> B.drop (offset + spanBytes) source }

restoreRecord :: U.Parts -> U.Parts -> Either String U.Parts
restoreRecord original staged = do
  offset <- recordOffset staged
  originalOffset <- recordOffset original
  let spanBytes = fromIntegral (recordBlocks * blockBytes)
      source = U.partPostHead staged
  pure staged { U.partPostHead = B.take offset source <> B.take spanBytes (B.drop originalOffset (U.partPostHead original)) <> B.drop (offset + spanBytes) source }

corruptRecord :: U.Parts -> Either String U.Parts
corruptRecord parts = do
  offset <- recordOffset parts
  let source = U.partPostHead parts
      location = offset + fromIntegral headerBytes
  unless (U.mediaWordAt source offset == recordMagic) (Left "corruption control requires an owned record")
  let changed = B.index source location `xor` 1
  pure parts { U.partPostHead = B.take location source <> B.singleton changed <> B.drop (location + 1) source }
