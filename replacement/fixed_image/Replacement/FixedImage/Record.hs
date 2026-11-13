{-# LANGUAGE OverloadedStrings #-}
module Replacement.FixedImage.Record
  ( BuildId(..), Bss(..), Publication(..), Image(..), publication, applyPublications
  , makeRecord, crc32, stageRecord, restoreRecord
  ) where

import Control.Monad (foldM, unless)
import qualified Data.ByteString as B
import Data.Bits (complement, shiftR, xor, (.&.))
import Data.List (nub)
import Data.Word (Word32)
import qualified FujiAF.UpdateContainer as U
import Replacement.FixedImage.Layout

newtype BuildId = BuildId Word32 deriving (Eq, Show)
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

applyPublications :: Image -> [Publication] -> B.ByteString -> Either String B.ByteString
applyPublications image rows bytes = do
  unless (length addresses == length (nub addresses)) (Left "duplicate publication address")
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

crc32 :: B.ByteString -> Word32
crc32 = complement . B.foldl' byte 0xffffffff
  where
    byte crc value = iterate bit (crc `xor` fromIntegral value) !! 8
    bit crc = (crc `shiftR` 1) `xor` (if crc .&. 1 == 0 then 0 else 0xedb88320)

makeRecord :: BuildId -> Bss -> B.ByteString -> Either String B.ByteString
makeRecord (BuildId build) (Bss begin end) body = do
  let size = headerBytes + fromIntegral (B.length body)
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
