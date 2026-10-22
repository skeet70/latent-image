{-# LANGUAGE OverloadedStrings #-}
-- Relative look statistics for the secondary check of spec/10 section 19.5.6:
-- Fujifilm's F-Log2 3D LUTs applied to a scene set, and any film-against-PROVIA
-- rendering pair binned by the PROVIA rendering's hue, chroma and lightness.
module LookReference
  ( fLog2Encode
  , M3
  , applyM3
  , primariesToXyz
  , fGamutToXyz
  , bt709ToXyz
  , invertM3
  , Cube
  , cubeSize
  , parseCube
  , applyCube
  , lutLab
  , sector
  , LabPair (..)
  , GroupStats (..)
  , groupStats
  , relativeStats
  ) where

import Control.Monad (unless)
import Data.Aeson (Value, object, (.=))
import qualified Data.Aeson.Key as K
import qualified Data.ByteString.Char8 as BC
import Data.List (sortOn)
import qualified Data.Map.Strict as M
import qualified Data.Text as T
import qualified Data.Vector.Unboxed as U

import FujiAF.Colour

-- | F-Log2 encoding of scene linear reflection (F-Log2 Data Sheet Ver.1.1,
-- section 2-3), clipped to [0, 1].
fLog2Encode :: Double -> Double
fLog2Encode x = max 0 (min 1 v)
 where
  v
    | x >= 0.000889 = 0.245281 * logBase 10 (5.555556 * x + 0.064829) + 0.384316
    | otherwise = 8.799461 * x + 0.092864

type M3 = ((Double, Double, Double), (Double, Double, Double), (Double, Double, Double))

applyM3 :: M3 -> (Double, Double, Double) -> (Double, Double, Double)
applyM3 ((a, b, c), (d, e, f), (g, h, i)) (x, y, z) = (a * x + b * y + c * z, d * x + e * y + f * z, g * x + h * y + i * z)

invertM3 :: M3 -> M3
invertM3 ((a, b, c), (d, e, f), (g, h, i)) =
  ( ((e * i - f * h) / det, (c * h - b * i) / det, (b * f - c * e) / det)
  , ((f * g - d * i) / det, (a * i - c * g) / det, (c * d - a * f) / det)
  , ((d * h - e * g) / det, (b * g - a * h) / det, (a * e - b * d) / det)
  )
 where
  det = a * (e * i - f * h) - b * (d * i - f * g) + c * (d * h - e * g)

-- | Linear RGB to XYZ (white Y = 1) from the xy chromaticities of R, G, B and white.
primariesToXyz :: (Double, Double) -> (Double, Double) -> (Double, Double) -> (Double, Double) -> M3
primariesToXyz r g b (wx, wy) = ((sr * xr, sg * xg, sb * xb), (sr, sg, sb), (sr * zr, sg * zg, sb * zb))
 where
  col (x, y) = (x / y, (1 - x - y) / y)
  (xr, zr) = col r
  (xg, zg) = col g
  (xb, zb) = col b
  (sr, sg, sb) = applyM3 (invertM3 ((xr, xg, xb), (1, 1, 1), (zr, zg, zb))) (wx / wy, 1, (1 - wx - wy) / wy)

-- | F-Gamut (F-Log2 Data Sheet Ver.1.1, section 3) and ITU-R BT.709, both D65.
fGamutToXyz, bt709ToXyz :: M3
fGamutToXyz = primariesToXyz (0.708, 0.292) (0.170, 0.797) (0.131, 0.046) (0.3127, 0.3290)
bt709ToXyz = primariesToXyz (0.64, 0.33) (0.30, 0.60) (0.15, 0.06) (0.3127, 0.3290)

-- | A 3D LUT in the .cube layout: size n, output triples with red varying fastest.
data Cube = Cube !Int !(U.Vector Double)

cubeSize :: Cube -> Int
cubeSize (Cube n _) = n

-- | A .cube file with LUT_3D_SIZE, the default [0, 1] domain and no 1D part.
parseCube :: BC.ByteString -> Either String Cube
parseCube bytes = do
  let ls = filter (\l -> not (BC.null l) && BC.head l /= '#') (map BC.strip (BC.lines bytes))
      (header, rows) = span (\l -> not (BC.null l) && not (isNumberStart (BC.head l))) ls
  n <- case [w | l <- header, ["LUT_3D_SIZE", w] <- [BC.words l]] of
    [w] | Just (v, rest) <- BC.readInt w, BC.null rest, v >= 2 -> Right v
    _ -> Left "cube needs one LUT_3D_SIZE of at least 2"
  flip mapM_ header $ \l -> case BC.words l of
    ("TITLE" : _) -> Right ()
    ["LUT_3D_SIZE", _] -> Right ()
    ["DOMAIN_MIN", "0", "0", "0"] -> Right ()
    ["DOMAIN_MAX", "1", "1", "1"] -> Right ()
    _ -> Left ("cube header line not supported: " ++ BC.unpack l)
  values <- traverse row rows
  unless (length values == n * n * n) (Left ("cube has " ++ show (length values) ++ " rows, expected " ++ show (n * n * n)))
  pure (Cube n (U.fromList (concat values)))
 where
  isNumberStart c = c == '-' || c == '.' || (c >= '0' && c <= '9')
  row l = case traverse decimal (BC.words l) of
    Just vs@[_, _, _] -> Right vs
    _ -> Left ("cube row is not three numbers: " ++ BC.unpack l)

-- | A plain decimal: optional sign, digits, optional fraction.
decimal :: BC.ByteString -> Maybe Double
decimal s0 = do
  let (sign, s) = case BC.uncons s0 of
        Just ('-', rest) -> (-1, rest)
        _ -> (1, s0)
      (ip, fp) = BC.break (== '.') s
  i <- if BC.null ip then Just 0 else whole ip
  f <- case BC.uncons fp of
    Nothing -> Just 0
    Just (_, digits)
      | BC.null digits -> Just 0
      | otherwise -> (\v -> fromIntegral v / 10 ^^ BC.length digits) <$> whole digits
  pure (sign * (fromIntegral i + f))
 where
  whole t = case BC.readInt t of
    Just (v, rest) | BC.null rest, BC.all (\c -> c >= '0' && c <= '9') t -> Just v
    _ -> Nothing

-- | Trilinear interpolation, inputs clipped to [0, 1].
applyCube :: Cube -> (Double, Double, Double) -> (Double, Double, Double)
applyCube (Cube n t) (r, g, b) = (channel 0, channel 1, channel 2)
 where
  pos v = let s = max 0 (min 1 v) * fromIntegral (n - 1); i = min (n - 2) (floor s) in (i, s - fromIntegral i)
  (ri, rf) = pos r
  (gi, gf) = pos g
  (bi, bf) = pos b
  at i j k c = t U.! (3 * (i + n * (j + n * k)) + c)
  lerp u x y = x + u * (y - x)
  channel c =
    lerp
      bf
      (lerp gf (lerp rf (at ri gi bi c) (at (ri + 1) gi bi c)) (lerp rf (at ri (gi + 1) bi c) (at (ri + 1) (gi + 1) bi c)))
      (lerp gf (lerp rf (at ri gi (bi + 1) c) (at (ri + 1) gi (bi + 1) c)) (lerp rf (at ri (gi + 1) (bi + 1) c) (at (ri + 1) (gi + 1) (bi + 1) c)))

-- | A D65 scene colour (reflection, white Y = 1) through F-Gamut, F-Log2 and
-- the LUT, its BT.709 output read with the sRGB transfer function, in
-- L*a*b* relative to D65.
lutLab :: Cube -> Xyz -> Lab
lutLab cube (Xyz x y z) = xyzToLab d65 (Xyz ox oy oz)
 where
  (fr, fg, fb) = applyM3 (invertM3 fGamutToXyz) (x, y, z)
  (cr, cg, cb) = applyCube cube (fLog2Encode fr, fLog2Encode fg, fLog2Encode fb)
  lin v = let c = max 0 (min 1 v) in if c <= 0.04045 then c / 12.92 else ((c + 0.055) / 1.055) ** 2.4
  (ox, oy, oz) = applyM3 bt709ToXyz (lin cr, lin cg, lin cb)

-- | 30-degree CIELAB hue sector of a colour with C* >= 5, "neutral" below.
sector :: Lab -> T.Text
sector lab
  | chromaOf lab < 5 = "neutral"
  | otherwise = T.pack ("h" ++ show (30 * (floor (hueOf lab) `div` 30 :: Int)))

chromaOf :: Lab -> Double
chromaOf (Lab _ a b) = sqrt (a * a + b * b)

-- | Hue angle in degrees, [0, 360).
hueOf :: Lab -> Double
hueOf (Lab _ a b) = let h = atan2 b a * 180 / pi in if h < 0 then h + 360 else h

-- | The same scene colour rendered by PROVIA (base) and by the other film.
data LabPair = LabPair {pairBase :: Lab, pairFilm :: Lab}

-- | Means over a group: C*_film / C*_base, dL*, dC* and the hue difference
-- in degrees (film minus base, wrapped to [-180, 180)).
data GroupStats = GroupStats {gsN :: Int, gsChromaRatio :: Double, gsDL :: Double, gsDC :: Double, gsDH :: Double} deriving (Show)

groupStats :: [LabPair] -> GroupStats
groupStats ps = GroupStats n (avg ratio) (avg dl) (avg dc) (avg dh)
 where
  n = length ps
  avg f = if n == 0 then 0 else sum (map f ps) / fromIntegral n
  ratio (LabPair b f) = chromaOf f / chromaOf b
  dl (LabPair (Lab lb _ _) (Lab lf _ _)) = lf - lb
  dc (LabPair b f) = chromaOf f - chromaOf b
  dh (LabPair b f) = let d = hueOf f - hueOf b in d - 360 * fromIntegral (floor ((d + 180) / 360) :: Int)

-- | Group statistics of the chromatic pairs (base C* >= 5) by base hue sector,
-- tone band (base L* < 35, 35-70, > 70), chroma band (5-15, 15-30, >= 30) and
-- the sector crossings, and of the neutral pairs by base L* in steps of 10.
relativeStats :: [LabPair] -> Value
relativeStats ps =
  object
    [ "chromatic" .= statsJson (groupStats chromatic)
    , "bySector" .= grouped statsJson sectorOf chromatic
    , "byTone" .= grouped statsJson tone chromatic
    , "byChroma" .= grouped statsJson chromaBand chromatic
    , "bySectorTone" .= grouped statsJson (\p -> sectorOf p <> " " <> tone p) chromatic
    , "bySectorChroma" .= grouped statsJson (\p -> sectorOf p <> " " <> chromaBand p) chromatic
    , "neutral" .= neutralJson (groupStats neutral)
    , "neutralByL" .= grouped neutralJson lStep neutral
    ]
 where
  (chromatic, neutral) = (filter ((>= 5) . chromaOf . pairBase) ps, filter ((< 5) . chromaOf . pairBase) ps)
  sectorOf = sector . pairBase
  lOf (LabPair (Lab l _ _) _) = l
  tone p
    | lOf p < 35 = "L<35"
    | lOf p <= 70 = "L35-70"
    | otherwise = "L>70"
  chromaBand p
    | chromaOf (pairBase p) < 15 = "C5-15"
    | chromaOf (pairBase p) < 30 = "C15-30"
    | otherwise = "C>=30"
  lStep p = let k = 10 * (floor (lOf p / 10) :: Int) in T.pack ("L" ++ show k ++ "-" ++ show (k + 10))
  grouped json key xs = object [K.fromText k .= json (groupStats g) | (k, g) <- sortOn fst (M.toList (M.fromListWith (flip (++)) [(key p, [p]) | p <- xs]))]

statsJson, neutralJson :: GroupStats -> Value
statsJson s = object ["n" .= gsN s, "chromaRatio" .= r3 (gsChromaRatio s), "dL" .= r3 (gsDL s), "dC" .= r3 (gsDC s), "dh" .= r3 (gsDH s)]
neutralJson s = object ["n" .= gsN s, "dL" .= r3 (gsDL s), "dC" .= r3 (gsDC s)]

r3 :: Double -> Double
r3 x = fromIntegral (round (x * 1000) :: Integer) / 1000
