{-# LANGUAGE OverloadedStrings #-}
module Replacement.RecordStore.References (FunctionRanges, parseRanges, resolvedReferences, referenceSummary) where

import qualified A32 as A
import Data.Aeson (Value, object, (.=))
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import qualified Data.Map.Strict as M
import Data.List (sortOn)
import Data.Maybe (mapMaybe)
import Data.Word (Word32)
import qualified FujiAF.UpdateContainer as U
import Replacement.RecordStore.Carrier
import Replacement.PropertyService.Placement
import ServiceInventory (Image (..))
import Text.Read (readMaybe)

newtype FunctionRanges = FunctionRanges (M.Map Word32 Word32)

parseRanges :: B.ByteString -> Either String FunctionRanges
parseRanges bytes = do
  ranges <- mapM parse [fields | line <- C.lines bytes, let fields = C.split '\t' line, take 1 fields == ["range"]]
  if null ranges then Left "function-ranges index is empty" else Right (FunctionRanges (M.fromList (merge (sortOn fst ranges))))
  where
    parse [_,_,_,_,address,size,_,_] = do
      start <- maybe (Left "invalid function start") Right (readMaybe (C.unpack address) :: Maybe Word32)
      count <- maybe (Left "invalid function size") Right (readMaybe (C.unpack size) :: Maybe Word32)
      if count == 0 || start + count < start then Left "invalid function extent" else Right (start,start+count)
    parse _ = Left "invalid function-ranges row"
    merge = reverse . foldl' add []
    add [] range = [range]
    add ((start,end):rest) (next,lastByte)
      | next <= end = (start,max end lastByte):rest
      | otherwise = (next,lastByte):(start,end):rest

inCode :: FunctionRanges -> GuestAddress -> Bool
inCode (FunctionRanges ranges) (GuestAddress address) = maybe False ((address <) . snd) (M.lookupLE address ranges)

resolvedReferences :: FunctionRanges -> [(ScanImage, Image)] -> [ScanHit]
resolvedReferences ranges images = filter resolved (scanRetired images) ++ concatMap literals images
  where
    resolved hit = inCode ranges (hitSite hit) && case hitReference hit of WordInto -> False; _ -> True
    literals (label,image@(Image _ _ base bytes)) = mapMaybe (literal label image . (base +) . fromIntegral . (4 *)) [0..B.length bytes `div` 4 - 1]
    literal label image site
      | not (inCode ranges (GuestAddress site)) || (label == ScanImage4 && site >= 0x6ac2b4 && site < 0x6ae114) = Nothing
      | otherwise = case A.decode site (wordAt image site) of
          A.Load _ A.Word _ _ register (A.OffsetImmediate displacement) A.OffsetOnly | register == A.pc -> do
            let address = fromIntegral (toInteger site + 8 + toInteger displacement)
            value <- checkedWord image address
            if any (\region -> inCarrier region (GuestAddress value)) retiredRegions && GuestAddress value `notElem` entryAddresses
              then Just (ScanHit label (GuestAddress site) value WordInto) else Nothing
          _ -> Nothing
    checkedWord (Image _ _ base bytes) address
      | address < base || toInteger address + 4 > toInteger base + toInteger (B.length bytes) = Nothing
      | otherwise = Just (U.word32At bytes (fromIntegral (address-base)))
    wordAt (Image _ _ base bytes) address = U.word32At bytes (fromIntegral (address-base))

referenceSummary :: FunctionRanges -> [(ScanImage, Image)] -> Value
referenceSummary ranges images = object
  [ "raw_aligned_word_candidates" .= [object ["image" .= show label, "range_start" .= start, "range_end" .= end,
      "code" .= length [() | hit <- matching, inCode ranges (hitSite hit)],
      "data_or_unmapped" .= length [() | hit <- matching, not (inCode ranges (hitSite hit))]]
      | (label,_) <- images, region@(Carrier (GuestAddress start) (GuestAddress end)) <- retiredRegions,
        let matching = [hit | hit <- raw, hitImage hit == label, inCarrier region (GuestAddress (hitWord hit))]]
  , "raw_candidates" .= [object ["image" .= show (hitImage hit), "site" .= address, "word" .= hitWord hit,
      "in_function_range" .= inCode ranges (hitSite hit)] | hit <- raw, let GuestAddress address = hitSite hit]
  , "computed_reference_obligation" .= ("Raw candidates and unresolved computed references require cutover fetch/access traps; function ranges are not a complete code/data proof." :: String)
  ]
  where raw = [hit | hit <- scanRetired images, hitReference hit == WordInto]
