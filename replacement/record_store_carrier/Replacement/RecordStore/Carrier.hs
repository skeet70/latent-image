{-# LANGUAGE OverloadedStrings #-}
module Replacement.RecordStore.Carrier (stageImage, scanRetired, scanObsoleteGap, entryAddresses, retiredRegions, cacheContract, cacheSymbols) where

import Control.Monad (unless)
import Data.Aeson (Value, object, (.=))
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

scanObsoleteGap :: [(ScanImage, Image)] -> [ScanHit]
scanObsoleteGap images = filter retainedSite (scanReferences (Carrier (GuestAddress 0x89f8f0) (GuestAddress 0x89f8f8)) images S.empty)
  where retainedSite hit = not (hitImage hit == ScanImage4 && inCarrier recordSpan (hitSite hit))

cacheSymbols :: [(C.ByteString, Word32)]
cacheSymbols =
  [("record_store_initialized",0x89f8b0),("record_store_wrappers",0x89f8b4),("record_store_state",0x89f8f8)
  ,("record_store_block",0x8a0080),("record_store_second_block",0x8c0080)
  ,("record_store_cache_tags",0x8e0080),("record_store_cache_timestamp",0x8e0088),("record_store_cache_ages",0x89fa60)
  ,("record_store_staging_0",0x8e16a0),("record_store_staging_1",0x8e00a0),("record_store_staging_2",0x8e1700)
  ,("record_store_staging_3",0x89fa80),("record_store_staging_4",0x8e10a0)]

cacheContract :: Value
cacheContract = object
  ["buffers" .= ([[0x8a0080,0x8c0080],[0x8c0080,0x8e0080]]::[[Word32]])
  ,"tags" .= object ["address" .= (0x8e0080::Word32),"bytes" .= (8::Int),"initial_values" .= ([0xffffffff,0xffffffff]::[Word32])]
  ,"timestamp" .= object ["address" .= (0x8e0088::Word32),"bytes" .= (4::Int),"initial_value" .= (0::Word32)]
  ,"ages" .= object ["address" .= (0x89fa60::Word32),"bytes" .= (8::Int),"initial_values" .= ([0,0]::[Word32])]
  ,"initialized_by" .= (0x6ac2b4::Word32)]

stageImage :: B.ByteString -> B.ByteString -> B.ByteString -> Either String B.ByteString
stageImage image linked symbols = do
    unless (B.length linked == 0x1e60) (Left "linked record span must be exactly 7776 bytes")
    forMEntries
    unless (length parsed == length (C.lines symbols)) (Left "invalid carrier symbol row")
    mapM_ (\(name,address) -> unless (lookup name parsed == Just address && length (filter ((== name) . fst) parsed) == 1) (Left ("cache/service symbol moved, duplicated or missing: " ++ C.unpack name))) cacheSymbols
    unless (lookup "record_store_cache_tag" parsed == Nothing) (Left "obsolete cache tag symbol remains")
    unless (all (\(name,address) -> address < 0x89f8f0 || address >= 0x89f8f8 || name `elem` map fst cacheSymbols) parsed) (Left "symbol aliases obsolete cache gap")
    unless (all (\(name,address) -> not (address >= 0x89fa60 && address < 0x89fa68 || address >= 0x8a0080 && address < 0x8e008c) || lookup name cacheSymbols == Just address) parsed) (Left "undeclared cache ownership alias")
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
