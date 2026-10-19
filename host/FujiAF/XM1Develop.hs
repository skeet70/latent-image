-- The X-M1 1.50 develop-stage data a still selects for a film and its image
-- adjustments, read from the decoded images 4 and 5 and the fallback
-- adjustment image (reports/classic-chrome-model.md sections 2.1 and 6.6),
-- and the adjustment-image encoding of colour matrix and chroma fields.
module FujiAF.XM1Develop
  ( Images
  , loadImages
  , DevelopCode (..)
  , Adjustments (..)
  , DynamicRange (..)
  , defaultAdjustments
  , adjustmentIndices
  , gainTableKey
  , gainTableKeyAt
  , gainRecords
  , toneKeyAt
  , toneRecords
  , lumaPreset
  , chromaGainProduct
  , chromaGainProductAt
  , scaleChroma
  , matrixFields
  , chromaFields
  , encodeField
  , decodeField
  , chromaStorage
  , chromaFromStorage
  , matrixStorage
  , x70GainRecords
  ) where

import Control.Monad (unless, when)
import Data.Bits (shiftL, shiftR, (.&.), (.|.))
import qualified Data.ByteString as B
import Data.Word (Word16)

import FujiAF.FilmStage

-- | Decoded image 4 (base 0x0061c400), image 5 (base 0x00bf1a00) and the
-- fallback adjustment image (complemented payload 0xe0048, 0x10000 bytes).
data Images = Images {image4 :: B.ByteString, image5 :: B.ByteString, adjustment :: B.ByteString}

-- | Develop code of FUN_00d78418: PROVIA 0, Velvia 4, ASTIA 5, PRO Neg Std 6, PRO Neg Hi 7.
newtype DevelopCode = DevelopCode Int deriving (Eq, Show)

loadImages :: FilePath -> FilePath -> FilePath -> IO (Either String Images)
loadImages id4 id5 payload = do
  i4 <- B.readFile id4
  i5 <- B.readFile id5
  p <- B.readFile payload
  pure $ do
    when (B.length i4 /= 0x282800) (Left "image 4 is not the 0x282800-byte 1.50 decode")
    when (B.length i5 /= 0x4f1e60) (Left "image 5 is not the 0x4f1e60-byte 1.50 decode")
    let adj = B.take 0x10000 (B.drop 0xe0048 p)
    unless (B.length adj == 0x10000 && B.take 4 (B.drop 0x15c adj) == B.pack [0x58, 0x2d, 0x4d, 0x31] && u16 adj 0x202 == Just 0xaa55)
      (Left "payload has no X-M1 fallback adjustment image at 0xe0048")
    pure (Images i4 i5 adj)

u16 :: B.ByteString -> Int -> Maybe Int
u16 b o
  | o >= 0 && o + 1 < B.length b = Just (fromIntegral (B.index b o) .|. (fromIntegral (B.index b (o + 1)) `shiftL` 8))
  | otherwise = Nothing

u32 :: B.ByteString -> Int -> Maybe Int
u32 b o = (\lo hi -> lo .|. (hi `shiftL` 16)) <$> u16 b o <*> u16 b (o + 2)

-- | u16 of image 5 at a decode address.
at5 :: Images -> Int -> Either String Int
at5 im a = maybe (Left ("image 5 read outside the image at " ++ show a)) Right (u16 (image5 im) (a - 0xbf1a00))

-- | Adjustment byte (FUN_0069f8fc) and little-endian halfword (FUN_0073ed6c).
adjByte :: Images -> Int -> Either String Int
adjByte im o
  | o >= 0 && o < B.length (adjustment im) = Right (fromIntegral (B.index (adjustment im) o))
  | otherwise = Left ("adjustment read outside the image at " ++ show o)

adjHalf :: Images -> Int -> Either String Int
adjHalf im o = maybe (Left ("adjustment read outside the image at " ++ show o)) Right (u16 (adjustment im) o)

-- | Adjustment byte named by the u16 entry `i` of the image-5 offset table at `table`.
viaTable :: Images -> Int -> Int -> Either String Int
viaTable im table i = at5 im (table + 2 * i) >>= adjByte im

-- | Image adjustments as menu settings: Color, Highlight and Shadow -2..+2.
data Adjustments = Adjustments {adjColor :: Int, adjHighlight :: Int, adjShadow :: Int, adjDr :: DynamicRange} deriving (Eq, Show)

data DynamicRange = DR100 | DR200 | DR400 deriving (Eq, Show, Enum, Bounded)

defaultAdjustments :: Adjustments
defaultAdjustments = Adjustments 0 0 0 DR100

-- | Record indices (Color +0x10c, Highlight +0xfc, Shadow +0x100, level2,
-- DR index +0x154). Index = 2 - setting and DR100/200/400 = level2 1/2/3,
-- DR index 0/1/2 are inference: the initialiser stores index 2, the Color
-- gains fall from index 0 to 4, Highlight index 0 raises and Shadow index 0
-- lowers the gain records, and FUN_0073fc64 maps +0x154 0..4 to level2 1..5.
adjustmentIndices :: Adjustments -> Either String (Int, Int, Int, Int, Int)
adjustmentIndices (Adjustments c h s dr) = do
  unless (all (\v -> v >= -2 && v <= 2) [c, h, s]) (Left "adjustment outside -2..+2")
  let drIndex = fromEnum dr
  pure (2 - c, 2 - h, 2 - s, drIndex + 1, drIndex)

-- | Gain-table record key of FUN_00d69df0 for develop mode 0 at the default adjustments.
gainTableKey :: Images -> DevelopCode -> Either String Int
gainTableKey im code = gainTableKeyAt im code defaultAdjustments

-- | Gain-table record key of FUN_00d69df0 for develop mode 0: family by code
-- from 0xd6a708, H/S family from 0xd6acec[5 H + 25 (family - 1) + S], key
-- from 0xd6a920[6 (hsfamily - 1) + level2].
gainTableKeyAt :: Images -> DevelopCode -> Adjustments -> Either String Int
gainTableKeyAt im (DevelopCode code) adj = do
  (_, highlight, shadow, level2, _) <- adjustmentIndices adj
  family <- viaTable im 0xd6a708 code
  hs <- viaTable im 0xd6acec (5 * highlight + 25 * (family - 1) + shadow)
  unless (hs >= 1 && hs <= 80) (Left ("H/S family " ++ show hs ++ " outside 1..80"))
  viaTable im 0xd6a920 (6 * (hs - 1) + level2)

-- | Tone-record key of FUN_00d675d8 for develop mode 0 and level1 2: family
-- from 0xd684a0 and from 0xd683b0 (index 5 code + level1), curve id from
-- 0xd685f4[6 (family - 1) + level2]; the two tables must agree.
toneKeyAt :: Images -> DevelopCode -> Adjustments -> Either String Int
toneKeyAt im (DevelopCode code) adj = do
  (_, _, _, level2, _) <- adjustmentIndices adj
  keys <- mapM (\table -> do
    family <- viaTable im table (5 * code + 2)
    unless (family >= 1 && family <= 60) (Left ("tone family " ++ show family ++ " outside 1..60"))
    viaTable im 0xd685f4 (6 * (family - 1) + level2)) [0xd684a0, 0xd683b0]
  case keys of
    [a, b] | a == b -> Right a
    _ -> Left ("tone tables A and B disagree: " ++ show keys)

-- | The keyed tone records of image 4 (count word 0x824bc4, records of 0x48
-- bytes from 0x824bc8) that FUN_00690924 searches.
toneRecords :: Images -> Either String [(Int, ToneRecord)]
toneRecords im = do
  let b = image4 im
      base = 0x61c400
  count <- maybe (Left "tone-record count unreadable") Right (u32 b (0x824bc4 - base))
  unless (count > 0 && count < 256) (Left "tone-record count out of range")
  mapM (\i -> do
    let o = 0x824bc8 - base + 0x48 * i
    key <- maybe (Left "tone record unreadable") Right (u32 b o)
    let es = [fromIntegral (B.index b (o + 4 + k)) | k <- [0 .. 8]]
    ps <- maybe (Left "tone record unreadable") Right (mapM (\k -> u16 b (o + 14 + 2 * k)) [0 .. 27])
    t <- toneRecord es ps
    pure (key, t)) [0 .. count - 1]

-- | The keyed 22-point records of image 4 (count word 0x825630, records of
-- 0x30 bytes from 0x825634) that FUN_00690b74 reads.
gainRecords :: Images -> Either String [(Int, GainTable)]
gainRecords im = do
  count <- maybe (Left "gain-record count unreadable") Right (u32 (image4 im) (0x825630 - 0x61c400))
  unless (count > 0 && count < 1024) (Left "gain-record count out of range")
  recordsAt (image4 im) (0x825634 - 0x61c400) count

recordsAt :: B.ByteString -> Int -> Int -> Either String [(Int, GainTable)]
recordsAt b start count = mapM one [0 .. count - 1]
 where
  one i = do
    let o = start + 0x30 * i
    key <- maybe (Left "gain record unreadable") Right (u32 b o)
    pts <- maybe (Left "gain record unreadable") Right (mapM (\k -> u16 b (o + 4 + 2 * k)) [0 .. 21])
    table <- gainTable pts
    pure (key, table)

-- | The X70 1.10 copy of the same record table in section 4 (base 0x8a0400),
-- record 0 at 0xb5f89c, read while keys run 0, 1, 2, ...
x70GainRecords :: B.ByteString -> Either String [(Int, GainTable)]
x70GainRecords s4 = do
  let start = 0xb5f89c - 0x8a0400
      keys = takeWhile id [u32 s4 (start + 0x30 * i) == Just i | i <- [0 ..]]
  when (null keys) (Left "no X70 gain records at 0xb5f89c")
  recordsAt s4 start (length keys)

-- | Luma-band preset of FUN_00d618a4 for row 0: id from 0xd62040, four bound
-- bytes through 0xd61d74, ten halfwords through 0xd61cd4 (five shifts with
-- the sign filled from bits 10-15, five gains).
lumaPreset :: Images -> DevelopCode -> Either String (Int, LumaBands)
lumaPreset im (DevelopCode code) = do
  i <- viaTable im 0xd62040 code
  unless (i >= 1 && i <= 8) (Left ("luma-band preset id " ++ show i ++ " outside 1..8"))
  bounds <- mapM (\k -> at5 im (0xd61d74 + 8 * (i - 1) + 2 * k) >>= adjByte im) [0 .. 3]
  vals <- mapM (\k -> at5 im (0xd61cd4 + 20 * (i - 1) + 2 * k) >>= adjHalf im) [0 .. 9]
  let (shifts, gains) = splitAt 5 vals
      signed v = if v .&. 0xfc00 /= 0 then (v .|. 0xfc00) - 0x10000 else v
  bands <- lumaBands bounds (zipWith LumaBand (map signed shifts) gains)
  pure (i, bands)

-- | Product of the five chroma gains at the default adjustments.
chromaGainProduct :: Images -> DevelopCode -> Either String Integer
chromaGainProduct im code = chromaGainProductAt im code defaultAdjustments

-- | Product of the five chroma gains FUN_00d60ea8 multiplies into the 2x2
-- before its >> 34: Color (0xd62194[+0x10c]), 0xd61f34 (mode 0, index 2),
-- film (0xd61e04 selector, 0xd620b0[5 (sel - 1) + DR index]), 0xd61e64
-- (index 0) and 0xd62134 (scene index 0). 2^34 is unity.
chromaGainProductAt :: Images -> DevelopCode -> Adjustments -> Either String Integer
chromaGainProductAt im (DevelopCode code) adj = do
  (colorIdx, _, _, _, drIndex) <- adjustmentIndices adj
  color <- viaTable im 0xd62194 colorIdx
  second <- viaTable im 0xd61f34 2
  sel <- viaTable im 0xd61e04 code
  film <- if sel >= 1 && sel <= 5 then viaTable im 0xd620b0 (5 * (sel - 1) + drIndex) else Right 128
  fourth <- viaTable im 0xd61e64 0
  fifth <- viaTable im 0xd62134 0
  pure (product (map fromIntegral [color, second, film, fourth, fifth]))

-- | Each 2x2 field times the gain product, arithmetic shift right by 34.
scaleChroma :: Integer -> Chroma -> Either String Chroma
scaleChroma g (Chroma a b c d) = chroma (s a) (s b) (s c) (s d)
 where
  s v = fromIntegral ((fromIntegral v * g) `shiftR` 34)

-- | Raw u16 fields of colour matrix `id` (offsets from 0xd67f88), row-major.
matrixFields :: Images -> Int -> Either String [Word16]
matrixFields im i = mapM (\k -> fromIntegral <$> (at5 im (0xd67f88 + 18 * (i - 1) + 2 * k) >>= adjHalf im)) [0 .. 8]

-- | Raw u16 fields of chroma entry `id` (offsets from 0xd61e14), storage order +0, +2, +4, +6.
chromaFields :: Images -> Int -> Either String [Word16]
chromaFields im i = mapM (\k -> fromIntegral <$> (at5 im (0xd61e14 + 8 * (i - 1) + 2 * k) >>= adjHalf im)) [0 .. 3]

-- | A 13-bit signed field value in the adjustment encoding: two's complement
-- in bits 0-12.
encodeField :: Int -> Either String Word16
encodeField v
  | v >= -4096 && v <= 4095 = Right (fromIntegral (v .&. 0x1fff))
  | otherwise = Left ("field value " ++ show v ++ " outside 13-bit signed")

-- | The stages' reading: twelve magnitude bits, negative when any of bits 12-15 is set.
decodeField :: Word16 -> Int
decodeField w = let v = fromIntegral w :: Int in if v .&. 0xf000 /= 0 then (v .&. 0xfff) - 0x1000 else v

-- | Storage order +0, +2, +4, +6 of a chroma entry. FUN_00d60cc4 reads
-- +0, +4, +6, +2 into (Cr from R-Y, Cr from B-Y, Cb from R-Y, Cb from B-Y).
chromaStorage :: Chroma -> Either String [Word16]
chromaStorage (Chroma crR crB cbR cbB) = mapM encodeField [crR, cbB, crB, cbR]

chromaFromStorage :: [Word16] -> Either String Chroma
chromaFromStorage [s0, s2, s4, s6] = chroma (decodeField s0) (decodeField s4) (decodeField s6) (decodeField s2)
chromaFromStorage _ = Left "chroma entry needs four fields"

matrixStorage :: ColourMatrix -> Either String [[Word16]]
matrixStorage = mapM (mapM encodeField) . matrixRows
