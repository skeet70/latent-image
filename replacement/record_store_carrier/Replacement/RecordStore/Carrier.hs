{-# LANGUAGE OverloadedStrings #-}
module Replacement.RecordStore.Carrier (stageImage, scanRetired, entryAddresses, retiredRegions) where

import Control.Monad (unless)
import Data.Bits ((.&.), shiftL, shiftR)
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import qualified Data.Set as S
import Data.Word (Word32)
import Numeric (readHex)
import Replacement.PropertyService.Placement
import ServiceInventory (Image (..))

entryAddresses :: [GuestAddress]
entryAddresses = map GuestAddress [0x6ac2b4,0x6ac328,0x6ac478,0x6ac4c8,0x6ac7d4,0x6ac984,0x6acab8,0x6acda0,0x6ae0fc]

recordSpan :: Carrier
recordSpan = Carrier (GuestAddress 0x6ac2b4) (GuestAddress 0x6ae114)

retiredRegions :: [Carrier]
retiredRegions = [recordSpan, region 0x89f8b0 0x89fa60, region 0x89fa60 0x89fa68,
    region 0x8a0080 0x8e0080, region 0x8e0080 0x8e008c, region 0x917fa8 0x917fe4]
  where region a b = Carrier (GuestAddress a) (GuestAddress b)

scanRetired :: [(ScanImage, Image)] -> [ScanHit]
scanRetired images = filter refused (concatMap (\region -> scanReferences region images S.empty) retiredRegions)
  where
    refused hit
      | hitImage hit == ScanImage4 && inCarrier recordSpan (hitSite hit) = False
      | otherwise = case hitReference hit of
          BranchInto target -> target `notElem` entryAddresses
          WordInto -> GuestAddress (hitWord hit) `notElem` entryAddresses
          MovPairInto target -> target `notElem` entryAddresses
          PcRelativeInto target -> target `notElem` entryAddresses

stageImage :: B.ByteString -> B.ByteString -> B.ByteString -> Either String B.ByteString
stageImage image linked symbols = do
    unless (B.length linked == 0x1e60) (Left "linked record span must be exactly 7776 bytes")
    forMEntries
    pure (B.take offset image <> linked <> B.drop (offset + B.length linked) image)
  where
    offset = 0x6ac2b4 - 0x61c400
    word bytes i = sum [fromIntegral (B.index bytes (i+j)) `shiftL` (8*j) | j <- [0..3]] :: Word32
    names = ["open","register","close","read_latest","append","erase_all","collect","read_at","zero"]
    parsed = [(name, address) | line <- C.lines symbols, [addressText,_,_,name] <- [C.words line], [(address,"")] <- [readHex (C.unpack addressText)]]
    forMEntries = mapM_ check (zip entryAddresses names)
    check (GuestAddress address, name) = do
      unless (lookup (C.pack ("record_store_entry_" ++ name)) parsed == Just address) (Left ("entry symbol moved: " ++ name))
      unless (word linked (fromIntegral (address - 0x6ac2b4)) .&. 0xff000000 == 0xea000000) (Left ("entry is not an ARM branch: " ++ name))
      target <- maybe (Left ("missing owned function: " ++ name)) Right (lookup (C.pack ("record_store_" ++ name)) parsed)
      unless (target >= 0x6ac2b4 && target < 0x6ae114 && target .&. 3 == 0) (Left ("owned function leaves span: " ++ name))
      let expected = 0xea000000 + (((target - address - 8) `shiftR` 2) .&. 0xffffff)
      unless (word linked (fromIntegral (address - 0x6ac2b4)) == expected) (Left ("entry branch target mismatch: " ++ name))
      unless (offset + B.length linked <= B.length image) (Left "image4 does not contain record span")
