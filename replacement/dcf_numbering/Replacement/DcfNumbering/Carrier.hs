{-# LANGUAGE OverloadedStrings #-}

-- | The in-place image-4 carrier of the owned DCF numbering: the five retired extents, the ten
-- entries, the splice of a linked image into image 4, and the census of references into the
-- extents from every scanned image.
module Replacement.DcfNumbering.Carrier
  ( Extent (..)
  , extents
  , inExtents
  , Entry (..)
  , entries
  , linkedBase
  , stageImage
  , Finding (..)
  , Verdict (..)
  , ReviewedData (..)
  , reviewedData
  , census
  , findingValue
  ) where

import Control.Monad (forM_, unless, when)
import Data.Aeson (Value, object, (.=))
import Data.Bits (shiftL, shiftR, (.&.))
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import Data.Word (Word32)
import Numeric (readHex, showHex)
import ReferenceScan (Address (..), Image (..), Reference (..), ReferenceKind (..), renderReference, scanReferences)

-- | A half-open guest address range of image 4.
data Extent = Extent {extentStart :: Word32, extentEnd :: Word32} deriving (Eq, Show)

extents :: [Extent]
extents =
  [ Extent 0x70f790 0x70f8b8, Extent 0x70faa0 0x7101e4, Extent 0x71141c 0x7115a4
  , Extent 0x7159f8 0x715a58, Extent 0x716184 0x71657c ]

inExtents :: Word32 -> Bool
inExtents address = any (\(Extent s e) -> s <= address && address < e) extents

data Entry = Entry {entryAddress :: Word32, entryName :: String} deriving (Eq, Show)

-- | The stock entries; the owned veneer at each is `dcf_entry_NAME`, a branch to `xm1_dcf_NAME`.
entries :: [Entry]
entries =
  [ Entry 0x70f790 "set_prefix_mode", Entry 0x70f840 "check_name"
  , Entry 0x70faa0 "apply_continuous_numbering", Entry 0x70fb68 "read_numbers"
  , Entry 0x70fbfc "prepare_name", Entry 0x71141c "note_written", Entry 0x7159f8 "read_profile"
  , Entry 0x715a2c "write_profile", Entry 0x716184 "create_folder", Entry 0x716324 "last_used" ]

-- | The address of the linked image's first byte.
linkedBase :: Word32
linkedBase = 0x70f790

image4Base :: Word32
image4Base = 0x61c400

-- | Image 4 with each extent replaced by the linked image's bytes at the same addresses, zero past
-- the linked image's end. Every defined symbol must lie inside one extent and every entry must be
-- an ARM branch to its owned function.
stageImage :: B.ByteString -> B.ByteString -> B.ByteString -> Either String B.ByteString
stageImage image linked symbols = do
  let lastEnd = extentEnd (last extents)
  unless (B.length linked <= fromIntegral (lastEnd - linkedBase)) (Left "linked image extends past the last extent")
  unless (fromIntegral (B.length image) >= lastEnd - image4Base) (Left "image 4 does not hold the extents")
  rows <- mapM parse (filter (not . null) (map C.words (C.lines symbols)))
  let defined = [(name, address, size) | (name, address, size, kind) <- rows, kind /= "A"]
  forM_ defined $ \(name, address, size) ->
    unless (any (\(Extent s e) -> s <= address && address + size <= e) extents)
      (Left ("symbol leaves the extents: " ++ C.unpack name))
  forM_ entries $ \(Entry address name) -> do
    let symbol text = [a | (n, a, _) <- defined, n == C.pack text]
    when (symbol ("dcf_entry_" ++ name) /= [address]) (Left ("entry symbol moved or missing: " ++ name))
    target <- case symbol ("xm1_dcf_" ++ name) of
      [a] -> Right a
      _ -> Left ("owned function missing: " ++ name)
    let offset = fromIntegral (address - linkedBase)
    unless (offset + 4 <= B.length linked) (Left ("entry outside the linked image: " ++ name))
    let expected = 0xea000000 + (((target - address - 8) `shiftR` 2) .&. 0xffffff)
    unless (wordAt linked offset == expected) (Left ("entry is not a branch to its owned function: " ++ name))
  let padded = linked <> B.replicate (fromIntegral (lastEnd - linkedBase) - B.length linked) 0
      splice bytes (Extent s e) =
        let at = fromIntegral (s - image4Base)
            count = fromIntegral (e - s)
         in B.take at bytes <> B.take count (B.drop (fromIntegral (s - linkedBase)) padded) <> B.drop (at + count) bytes
  pure (foldl splice image extents)
  where
    parse [addressText, sizeText, kind, name]
      | [(address, "")] <- readHex (C.unpack addressText), [(size, "")] <- readHex (C.unpack sizeText) =
          Right (name, address, size, kind)
    parse row = Left ("invalid symbol row: " ++ C.unpack (C.unwords row))

wordAt :: B.ByteString -> Int -> Word32
wordAt bytes offset = sum [fromIntegral (B.index bytes (offset + i)) `shiftL` (8 * i) | i <- [0 .. 3]]

-- | A scanned word reviewed as data: its image, site and the word the site holds.
data ReviewedData = ReviewedData {reviewedImage :: String, reviewedSite :: Word32, reviewedWord :: Word32, reviewedReason :: String}

-- | The aligned words whose value lies in an extent. None is loaded PC-relative by any A32 or
-- Thumb instruction of the scanned images; each sits in a table of like values.
reviewedData :: [ReviewedData]
reviewedData =
  [ ReviewedData "image4" 0x7bdfd0 0x0070f87c "image4 data: alternating 0xf0104cXX register table"
  , ReviewedData "image4" 0x7c5574 0x0070fff1 "image4 data: odd value"
  , ReviewedData "image4" 0x7c5598 0x0070ffe5 "image4 data: odd value"
  , ReviewedData "image4" 0x7cfa00 0x0071000b "image4 data: odd value"
  , ReviewedData "image4" 0x7d1878 0x00710005 "image4 data: odd value"
  , ReviewedData "image4" 0x7d5a50 0x0070ffe6 "image4 data: value not word-aligned"
  , ReviewedData "image4" 0x7dfd50 0x0070fd7c "image4 data: signed halfword coefficient table, repeated at 0x7ea76c"
  , ReviewedData "image4" 0x7e4d90 0x0070ffee "image4 data: value not word-aligned"
  , ReviewedData "image4" 0x7ea76c 0x0070fd7c "image4 data: signed halfword coefficient table, repeated at 0x7dfd50"
  , ReviewedData "image5" 0xd691c8 0x00710071 "image5 data: odd value"
  , ReviewedData "image5" 0xd691cc 0x00710071 "image5 data: odd value"
  , ReviewedData "image5" 0xf562dc 0x0071006e "image5 data: value not word-aligned"
  , ReviewedData "image5" 0xf56520 0x0071006f "image5 data: odd value"
  , ReviewedData "image5" 0xf580bc 0x00710070 "image5 data: ascending halfword ramp 0x5c..0x88"
  , ReviewedData "image5" 0x1070158 0x00710071 "image5 data: odd value"
  , ReviewedData "image5" 0x10871b8 0x00710065 "image5 data: odd value"
  , ReviewedData "image5" 0x1099f48 0x0070fc82 "image5 data: value not word-aligned"
  , ReviewedData "plain" 0x13954cc 0x00716552 "overlay 47 data: value not word-aligned"
  , ReviewedData "plain" 0x13954f0 0x00716552 "overlay 47 data: value not word-aligned"
  , ReviewedData "plain" 0x1395514 0x00716552 "overlay 47 data: value not word-aligned"
  , ReviewedData "plain" 0x13a2590 0x00716572 "overlay 47 data: value not word-aligned"
  , ReviewedData "plain" 0x1449760 0x00716573 "overlay 47 data: odd value"
  ]

data Verdict
  = EntryBranch
  | EntryPointer
  | Reviewed String
  | Unaligned
  | Refused
  deriving (Eq, Show)

-- | A reference into the extents from outside them, with the word at its site.
data Finding = Finding {findingReference :: Reference, findingWord :: Word32, findingVerdict :: Verdict}

-- | Every reference into the extents from a site outside them. An A32 branch or aligned pointer
-- word to an entry is admitted; a halfword-aligned word is a decode of two unrelated halfwords,
-- which no A32 load reads; a reviewed data word is admitted; anything else is refused.
census :: [Image] -> [Finding]
census images = concatMap scan images
  where
    targets = map Address (concat [[s, s + 1 .. e - 1] | Extent s e <- extents])
    scan image =
      [ Finding reference (siteWord image site) (verdict reference (siteWord image site))
      | reference@(Reference _ _ _ (Address site)) <- scanReferences targets image
      , not (imageName image == "image4" && inExtents site)
      ]
    entrySet = map entryAddress entries
    verdict (Reference (Address target) kind name (Address site)) word
      | kind `elem` [ArmB, ArmBL] && target `elem` entrySet = EntryBranch
      | kind == PointerWord && word `elem` entrySet = EntryPointer
      | kind == PointerHalfword = Unaligned
      | kind == PointerWord, [reason] <- [reviewedReason r | r <- reviewedData, reviewedImage r == name, reviewedSite r == site, reviewedWord r == word] = Reviewed reason
      | otherwise = Refused
    siteWord (Image _ (Address base) bytes) site =
      let offset = fromIntegral (site - base)
       in if offset + 4 <= B.length bytes then wordAt bytes offset else 0

findingValue :: Finding -> Value
findingValue (Finding reference@(Reference (Address target) _ name (Address site)) word verdict) =
  object
    [ "image" .= name, "site" .= hex site, "target" .= hex target, "word" .= hex word
    , "reference" .= renderReference reference, "verdict" .= verdictText ]
  where
    hex value = "0x" ++ showHex value ""
    verdictText :: String
    verdictText = case verdict of
      EntryBranch -> "entry-branch"
      EntryPointer -> "entry-pointer"
      Reviewed reason -> "reviewed-data: " ++ reason
      Unaligned -> "halfword-aligned-word"
      Refused -> "refused"
