{-# LANGUAGE OverloadedStrings #-}
-- Integer model of the EXR Processor II film stage on flat input: gain table,
-- colour matrix, tone curve A, 8-bit RGB, BT.601 YCbCr, chroma 2x2 and luma
-- bands. The arithmetic is the one x70view documents for the X70 RAW
-- conversion (research/film-simulation/raw-clone.md, matrix.md); film_stage.py
-- is its external oracle.
module FujiAF.FilmStage
  ( GainTable
  , gainTable
  , gainTableQ9
  , ColourMatrix
  , colourMatrix
  , matrixRows
  , ToneRecord
  , toneRecord
  , toneExponents
  , tonePoints
  , Chroma (..)
  , chroma
  , LumaBand (..)
  , LumaBands
  , lumaBands
  , bandBounds
  , bandList
  , FilmParams (..)
  , Rgb14 (..)
  , rgb14
  , YCbCr (..)
  , Stage (..)
  , compileStage
  , withColour
  , develop
  , registerWords
  , parseFilmParams
  , filmParamsJson
  ) where

import Control.Monad (unless)
import Data.Aeson (Value, object, (.=))
import qualified Data.Aeson as A
import qualified Data.Aeson.Types as AT
import Data.Bits (shiftL, shiftR, (.&.), (.|.))
import qualified Data.Vector.Unboxed as U
import Data.Word (Word32)

-- | 22 Q9 control points of the brightness gain curve (13-bit fields).
newtype GainTable = GainTable (U.Vector Int) deriving (Eq, Show)

-- | Q10 rows, each field a 13-bit signed value.
data ColourMatrix = ColourMatrix !(Int, Int, Int) !(Int, Int, Int) !(Int, Int, Int) deriving (Eq, Show)

-- | The 0x48-byte tone record body: nine segment exponents and 28 u14 points.
data ToneRecord = ToneRecord !(U.Vector Int) !(U.Vector Int) deriving (Eq, Show)

-- | Q10 2x2 on (R-Y, B-Y), each field 13-bit signed.
data Chroma = Chroma
  { crFromRY :: !Int
  , crFromBY :: !Int
  , cbFromRY :: !Int
  , cbFromBY :: !Int
  }
  deriving (Eq, Show)

-- | Q8 chroma factor gain + shift*Y/256 of one luma band.
data LumaBand = LumaBand {bandShift :: !Int, bandGain :: !Int} deriving (Eq, Show)

-- | Four non-decreasing bounds and five bands; a luma on a bound takes the upper band.
data LumaBands = LumaBands !(U.Vector Int) ![LumaBand] deriving (Eq, Show)

data FilmParams = FilmParams
  { filmGainTable :: !GainTable
  , filmMatrix :: !ColourMatrix
  , filmTone :: !ToneRecord
  , filmChroma :: !Chroma
  , filmBands :: !LumaBands
  }
  deriving (Eq, Show)

-- | White-balanced 14-bit camera values.
data Rgb14 = Rgb14 !Int !Int !Int deriving (Eq, Show)

data YCbCr = YCbCr {ycY :: !Int, ycCb :: !Int, ycCr :: !Int} deriving (Eq, Show)

inRange :: Int -> Int -> Int -> Bool
inRange lo hi v = lo <= v && v <= hi

signed13 :: Int -> Bool
signed13 = inRange (-4096) 4095

gainTable :: [Int] -> Either String GainTable
gainTable xs = do
  unless (length xs == 22) (Left "gain table needs 22 points")
  unless (all (inRange 0 8191) xs) (Left "gain table point outside 13 bits")
  pure (GainTable (U.fromList xs))

gainTableQ9 :: GainTable -> [Int]
gainTableQ9 (GainTable v) = U.toList v

colourMatrix :: [[Int]] -> Either String ColourMatrix
colourMatrix [[a, b, c], [d, e, f], [g, h, i]]
  | all signed13 [a, b, c, d, e, f, g, h, i] = Right (ColourMatrix (a, b, c) (d, e, f) (g, h, i))
  | otherwise = Left "matrix field outside 13-bit signed"
colourMatrix _ = Left "matrix needs three rows of three"

matrixRows :: ColourMatrix -> [[Int]]
matrixRows (ColourMatrix (a, b, c) (d, e, f) (g, h, i)) = [[a, b, c], [d, e, f], [g, h, i]]

toneRecord :: [Int] -> [Int] -> Either String ToneRecord
toneRecord es ps = do
  unless (length es == 9) (Left "tone record needs 9 exponents")
  unless (all (inRange 0 7) es) (Left "tone exponent outside 0..7")
  unless (length ps == 28) (Left "tone record needs 28 points")
  unless (all (inRange 0 16383) ps) (Left "tone point outside 14 bits")
  pure (ToneRecord (U.fromList es) (U.fromList ps))

toneExponents, tonePoints :: ToneRecord -> [Int]
toneExponents (ToneRecord e _) = U.toList e
tonePoints (ToneRecord _ p) = U.toList p

chroma :: Int -> Int -> Int -> Int -> Either String Chroma
chroma a b c d
  | all signed13 [a, b, c, d] = Right (Chroma a b c d)
  | otherwise = Left "chroma field outside 13-bit signed"

lumaBands :: [Int] -> [LumaBand] -> Either String LumaBands
lumaBands bounds bs = do
  unless (length bounds == 4 && all (inRange 0 255) bounds) (Left "luma bands need four byte bounds")
  unless (and (zipWith (<=) bounds (drop 1 bounds))) (Left "luma band bounds decrease")
  unless (length bs == 5) (Left "luma bands need five bands")
  unless (all (\(LumaBand s g) -> inRange (-1024) 1023 s && inRange 0 1023 g) bs) (Left "luma band outside its field")
  pure (LumaBands (U.fromList bounds) bs)

bandBounds :: LumaBands -> [Int]
bandBounds (LumaBands b _) = U.toList b

bandList :: LumaBands -> [LumaBand]
bandList (LumaBands _ bs) = bs

rgb14 :: Int -> Int -> Int -> Either String Rgb14
rgb14 r g b
  | all (inRange 0 16383) [r, g, b] = Right (Rgb14 r g b)
  | otherwise = Left "camera value outside 14 bits"

-- | Piecewise cubic Bezier over 0..4095: segment k spans widths!!k inputs and
-- uses points 3k..3k+3 with t quantised to 9 bits; inputs past the last
-- segment take the last point.
bezierLut :: U.Vector Int -> [Int] -> U.Vector Int
bezierLut points widths = U.generate 4096 at
 where
  starts = scanl (+) 0 widths
  segs = zip3 [0 :: Int ..] starts widths
  final = U.last points
  at x = case [(k, s, n) | (k, s, n) <- segs, x >= s, x < s + n] of
    ((k, s, n) : _) ->
      let t = ((x - s) `shiftL` 9) `div` n
          u = 512 - t
          p i = points U.! (3 * k + i)
       in (u * u * u * p 0 + 3 * u * u * t * p 1 + 3 * u * t * t * p 2 + t * t * t * p 3 + (1 `shiftL` 26)) `shiftR` 27
    [] -> final

-- | Parameters with their curves evaluated once.
data Stage = Stage
  { stageGain :: !(U.Vector Int)
  , stageTone :: !(U.Vector Int)
  , stageMatrix :: !ColourMatrix
  , stageChroma :: !Chroma
  , stageBands :: !LumaBands
  }

compileStage :: FilmParams -> Stage
compileStage p =
  Stage
    { stageGain = bezierLut table [64, 64, 128, 256, 512, 1024, 2048]
    , stageTone = bezierLut pts [16 `shiftL` (e .&. 7) | e <- U.toList es]
    , stageMatrix = filmMatrix p
    , stageChroma = filmChroma p
    , stageBands = filmBands p
    }
 where
  GainTable table = filmGainTable p
  ToneRecord es pts = filmTone p

-- | The same curves with another matrix and chroma.
withColour :: ColourMatrix -> Chroma -> Stage -> Stage
withColour m c s = s {stageMatrix = m, stageChroma = c}

clip :: Int -> Int -> Int -> Int
clip lo hi = max lo . min hi

develop :: Stage -> Rgb14 -> YCbCr
develop st (Rgb14 wr wg wb) = YCbCr y (out cb) (out cr)
 where
  x12 w = min (w `shiftR` 2) 4095
  bright = (3 * x12 wr + 7 * x12 wg + 6 * x12 wb) `shiftR` 4
  g = stageGain st U.! bright
  gained w = min ((w * g) `shiftR` 11) 4095
  (xr, xg, xb) = (gained wr, gained wg, gained wb)
  ColourMatrix r0 r1 r2 = stageMatrix st
  row (a, b, c) = clip 0 4095 ((((a * xr) `shiftR` 7) + ((b * xg) `shiftR` 7) + ((c * xb) `shiftR` 7) + 4) `shiftR` 3)
  tone v = (stageTone st U.! v) `shiftR` 6
  (r8, g8, b8) = (tone (row r0), tone (row r1), tone (row r2))
  y = (((306 * r8) `shiftR` 7) + ((601 * g8) `shiftR` 7) + ((117 * b8) `shiftR` 7) + 4) `shiftR` 3
  dr = r8 - y
  db = b8 - y
  Chroma c0 c1 c3 c2 = stageChroma st
  cb = clip (-512) 511 (((((c2 * db) `shiftR` 8) + ((c3 * dr) `shiftR` 8)) * 578) `shiftR` 10)
  cr = clip (-512) 511 (((((c0 * dr) `shiftR` 8) + ((c1 * db) `shiftR` 8)) * 730) `shiftR` 10)
  LumaBands bounds bands = stageBands st
  LumaBand shift gain = bands !! U.length (U.filter (<= y) bounds)
  f = gain + ((shift * y) `shiftR` 8)
  out c = clip 0 255 (128 + ((c * f + 512) `shiftR` 10))

-- | The live FFF19000 words (relative offsets) film_stage.develop reads.
registerWords :: FilmParams -> [(Int, Word32)]
registerWords p =
  [(0x304 + 4 * i, pair (t !! (2 * i)) (t !! (2 * i + 1))) | i <- [0 .. 10]]
    ++ concat [[(0x400 + 8 * r, pair a b), (0x404 + 8 * r, field13 c)] | (r, [a, b, c]) <- zip [0 ..] (matrixRows (filmMatrix p))]
    ++ [ (0x500, fromIntegral (foldr (.|.) 0 [(e .&. 0xf) `shiftL` (4 * i) | (i, e) <- zip [0 ..] (take 8 es)]))
       , (0x504, fromIntegral (es !! 8 .&. 0xf))
       ]
    ++ [(0x508 + 4 * i, fromIntegral ((ps !! (2 * i)) .|. ((ps !! (2 * i + 1)) `shiftL` 16))) | i <- [0 .. 13]]
    ++ [(0x604, pair c0 c1), (0x608, pair c2 c3)]
    ++ [(0x640, fromIntegral (foldr (.|.) 0 [b `shiftL` (8 * i) | (i, b) <- zip [0 ..] (bandBounds (filmBands p))]))]
    ++ [(0x644 + 4 * i, fromIntegral ((s .&. 0x7ff) .|. ((g .&. 0x3ff) `shiftL` 16))) | (i, LumaBand s g) <- zip [0 ..] (bandList (filmBands p))]
 where
  t = gainTableQ9 (filmGainTable p)
  es = toneExponents (filmTone p)
  ps = tonePoints (filmTone p)
  Chroma c0 c1 c3 c2 = filmChroma p
  field13 v = fromIntegral (v .&. 0x1fff)
  pair a b = field13 a .|. (field13 b `shiftL` 16)

parseFilmParams :: Value -> Either String FilmParams
parseFilmParams = AT.parseEither $ A.withObject "film parameters" $ \o -> do
  table <- o A..: "gainTableQ9"
  rows <- o A..: "matrixQ10"
  toneO <- o A..: "tone"
  es <- toneO A..: "exponents"
  ps <- toneO A..: "points"
  ch <- o A..: "chromaQ10"
  [a, b, c, d] <- mapM (ch A..:) ["crFromRY", "crFromBY", "cbFromRY", "cbFromBY"]
  bandsO <- o A..: "lumaBands"
  bounds <- bandsO A..: "bounds"
  bs <- bandsO A..: "bands" >>= mapM (A.withObject "band" (\x -> LumaBand <$> x A..: "shift" <*> x A..: "gain"))
  let checked = FilmParams <$> gainTable table <*> colourMatrix rows <*> toneRecord es ps <*> chroma a b c d <*> lumaBands bounds bs
  either fail pure checked

filmParamsJson :: FilmParams -> Value
filmParamsJson p =
  object
    [ "gainTableQ9" .= gainTableQ9 (filmGainTable p)
    , "matrixQ10" .= matrixRows (filmMatrix p)
    , "tone" .= object ["exponents" .= toneExponents (filmTone p), "points" .= tonePoints (filmTone p)]
    , "chromaQ10" .= object ["crFromRY" .= c0, "crFromBY" .= c1, "cbFromRY" .= c3, "cbFromBY" .= c2]
    , "lumaBands"
        .= object
          [ "bounds" .= bandBounds (filmBands p)
          , "bands" .= [object ["shift" .= s, "gain" .= g] | LumaBand s g <- bandList (filmBands p)]
          ]
    ]
 where
  Chroma c0 c1 c3 c2 = filmChroma p
