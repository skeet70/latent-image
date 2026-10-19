-- Colorimetry for the film-stage fit: CIE XYZ and L*a*b*, CIEDE2000, the
-- camera output's JFIF YCbCr as sRGB, public camera colour matrices and the
-- scene patch sets.
module FujiAF.Colour
  ( Xyz (..)
  , Lab (..)
  , Rgb (..)
  , d50
  , d65
  , labToXyz
  , xyzToLab
  , bradfordD50toD65
  , ciede2000
  , ycbcrToLab
  , ycbcrToLabF
  , CameraModel (..)
  , xm1Camera
  , x70Camera
  , xt1Camera
  , whiteBalanced
  , Patch (..)
  , PatchSet (..)
  , colorChecker24
  , neutralRamp
  , gamutSet
  , inRec2020
  ) where

data Xyz = Xyz !Double !Double !Double deriving (Eq, Show)

data Lab = Lab !Double !Double !Double deriving (Eq, Show)

data Rgb = Rgb !Double !Double !Double deriving (Eq, Show)

type M3 = ((Double, Double, Double), (Double, Double, Double), (Double, Double, Double))

apply :: M3 -> (Double, Double, Double) -> (Double, Double, Double)
apply ((a, b, c), (d, e, f), (g, h, i)) (x, y, z) = (a * x + b * y + c * z, d * x + e * y + f * z, g * x + h * y + i * z)

-- | ICC D50 and CIE D65 (2 degree) white points, Y = 1.
d50, d65 :: Xyz
d50 = Xyz 0.96422 1 0.82521
d65 = Xyz 0.95047 1 1.08883

labToXyz :: Xyz -> Lab -> Xyz
labToXyz (Xyz wx wy wz) (Lab l a b) = Xyz (wx * finv fx) (wy * finv fy) (wz * finv fz)
 where
  fy = (l + 16) / 116
  fx = fy + a / 500
  fz = fy - b / 200
  finv t = if t > 6 / 29 then t * t * t else 3 * (6 / 29) ^ (2 :: Int) * (t - 4 / 29)

xyzToLab :: Xyz -> Xyz -> Lab
xyzToLab (Xyz wx wy wz) (Xyz x y z) = Lab (116 * fy - 16) (500 * (fx - fy)) (200 * (fy - fz))
 where
  f t = if t > (6 / 29) ^ (3 :: Int) then t ** (1 / 3) else t / (3 * (6 / 29) ^ (2 :: Int)) + 4 / 29
  (fx, fy, fz) = (f (x / wx), f (y / wy), f (z / wz))

-- | Bradford chromatic adaptation, ICC D50 to D65 (Lindbloom).
bradfordD50toD65 :: Xyz -> Xyz
bradfordD50toD65 (Xyz x y z) = let (a, b, c) = apply m (x, y, z) in Xyz a b c
 where
  m =
    ( (0.9555766, -0.0230393, 0.0631636)
    , (-0.0282895, 1.0099416, 0.0210077)
    , (0.0122982, -0.0204830, 1.3299098)
    )

-- | CIEDE2000 with kL = kC = kH = 1 (Sharma, Wu and Dalal 2005).
ciede2000 :: Lab -> Lab -> Double
ciede2000 (Lab l1 a1 b1) (Lab l2 a2 b2) =
  sqrt (sq (dL / sl) + sq (dC / sc) + sq (dH / sh) + rt * (dC / sc) * (dH / sh))
 where
  sq v = v * v
  deg r = r * 180 / pi
  rad d = d * pi / 180
  c1 = sqrt (sq a1 + sq b1)
  c2 = sqrt (sq a2 + sq b2)
  cbar = (c1 + c2) / 2
  g = 0.5 * (1 - sqrt (cbar ** 7 / (cbar ** 7 + 25 ** 7)))
  a1' = (1 + g) * a1
  a2' = (1 + g) * a2
  c1' = sqrt (sq a1' + sq b1)
  c2' = sqrt (sq a2' + sq b2)
  hue a b = if a == 0 && b == 0 then 0 else let h = deg (atan2 b a) in if h < 0 then h + 360 else h
  h1' = hue a1' b1
  h2' = hue a2' b2
  dL = l2 - l1
  dC = c2' - c1'
  dh
    | c1' * c2' == 0 = 0
    | abs (h2' - h1') <= 180 = h2' - h1'
    | h2' - h1' > 180 = h2' - h1' - 360
    | otherwise = h2' - h1' + 360
  dH = 2 * sqrt (c1' * c2') * sin (rad (dh / 2))
  lbar = (l1 + l2) / 2
  cbar' = (c1' + c2') / 2
  hbar
    | c1' * c2' == 0 = h1' + h2'
    | abs (h1' - h2') <= 180 = (h1' + h2') / 2
    | h1' + h2' < 360 = (h1' + h2' + 360) / 2
    | otherwise = (h1' + h2' - 360) / 2
  t = 1 - 0.17 * cos (rad (hbar - 30)) + 0.24 * cos (rad (2 * hbar)) + 0.32 * cos (rad (3 * hbar + 6)) - 0.20 * cos (rad (4 * hbar - 63))
  dTheta = 30 * exp (negate (sq ((hbar - 275) / 25)))
  rc = 2 * sqrt (cbar' ** 7 / (cbar' ** 7 + 25 ** 7))
  sl = 1 + 0.015 * sq (lbar - 50) / sqrt (20 + sq (lbar - 50))
  sc = 1 + 0.045 * cbar'
  sh = 1 + 0.015 * cbar' * t
  rt = negate (sin (rad (2 * dTheta))) * rc

-- | 8-bit full-range BT.601 (JFIF) YCbCr read as sRGB, in L*a*b* relative to D65.
ycbcrToLab :: Int -> Int -> Int -> Lab
ycbcrToLab y cb cr = ycbcrToLabF (fromIntegral y) (fromIntegral cb) (fromIntegral cr)

-- | ycbcrToLab on mean (non-integer) levels.
ycbcrToLabF :: Double -> Double -> Double -> Lab
ycbcrToLabF fy cb cr = xyzToLab d65 (Xyz x yy z)
 where
  (fcb, fcr) = (cb - 128, cr - 128)
  r = fy + 1.402 * fcr
  g = fy - 0.344136 * fcb - 0.714136 * fcr
  b = fy + 1.772 * fcb
  lin v = let c = max 0 (min 255 v) / 255 in if c <= 0.04045 then c / 12.92 else ((c + 0.055) / 1.055) ** 2.4
  (x, yy, z) =
    apply
      ((0.4124564, 0.3575761, 0.1804375), (0.2126729, 0.7151522, 0.0721750), (0.0193339, 0.1191920, 0.9503041))
      (lin r, lin g, lin b)

-- | A camera's XYZ (D65) to raw-RGB matrix, LibRaw adobe_coeff values x 10000.
data CameraModel = CameraModel {cameraName :: String, cameraXyzToRaw :: [Int]}

-- LibRaw 0.22.1 src/tables/colordata.cpp, entries "X-M1" (shared with X-Pro1 and
-- X-E1), "X70" and "X-T1" (shared with X-E2).
xm1Camera, x70Camera, xt1Camera :: CameraModel
xm1Camera = CameraModel "X-M1" [10413, -3996, -993, -3721, 11640, 2361, -733, 1540, 6011]
x70Camera = CameraModel "X70" [10450, -4329, -878, -3217, 11105, 2421, -752, 1758, 6519]
xt1Camera = CameraModel "X-T1" [8458, -2451, -855, -4597, 12447, 2407, -1475, 2482, 6526]

-- | Raw RGB of a D65-referred colour, each channel divided by the D65 white's response.
whiteBalanced :: CameraModel -> Xyz -> Rgb
whiteBalanced cam (Xyz x y z) = Rgb (r / wr) (g / wg) (b / wb)
 where
  m = case map ((/ 10000) . fromIntegral) (cameraXyzToRaw cam) of
    [a, b', c, d, e, f, g', h, i] -> ((a, b', c), (d, e, f), (g', h, i))
    _ -> error "camera matrix needs nine entries"
  (r, g, b) = apply m (x, y, z)
  Xyz x0 y0 z0 = d65
  (wr, wg, wb) = apply m (x0, y0, z0)

data Patch = Patch {patchName :: String, patchXyz :: Xyz} deriving (Show)

data PatchSet = PatchSet {setName :: String, setPatches :: [Patch]}

-- | ColorChecker Classic, X-Rite L*a*b* (D50, 2 degree) for charts made after
-- November 2014, adapted to D65 with Bradford.
colorChecker24 :: PatchSet
colorChecker24 = PatchSet "colorchecker24" [Patch n (bradfordD50toD65 (labToXyz d50 (Lab l a b))) | (n, l, a, b) <- values]
 where
  values =
    [ ("dark skin", 37.54, 14.37, 14.92)
    , ("light skin", 64.66, 19.27, 17.5)
    , ("blue sky", 49.32, -3.82, -22.54)
    , ("foliage", 43.46, -12.74, 22.72)
    , ("blue flower", 54.94, 9.61, -24.79)
    , ("bluish green", 70.48, -32.26, -0.37)
    , ("orange", 62.73, 35.83, 56.5)
    , ("purplish blue", 39.43, 10.75, -45.17)
    , ("moderate red", 50.57, 48.64, 16.67)
    , ("purple", 30.1, 22.54, -20.87)
    , ("yellow green", 71.77, -24.13, 58.19)
    , ("orange yellow", 71.51, 18.24, 67.37)
    , ("blue", 28.37, 15.42, -49.8)
    , ("green", 54.38, -39.72, 32.27)
    , ("red", 42.43, 51.05, 28.62)
    , ("yellow", 81.8, 2.67, 80.41)
    , ("magenta", 50.63, 51.28, -14.12)
    , ("cyan", 49.57, -29.71, -28.32)
    , ("white 9.5", 95.19, -1.03, 2.93)
    , ("neutral 8", 81.29, -0.57, 0.44)
    , ("neutral 6.5", 66.89, -0.75, -0.06)
    , ("neutral 5", 50.76, -0.13, 0.14)
    , ("neutral 3.5", 35.63, -0.46, -0.48)
    , ("black 2", 20.64, 0.07, -0.46)
    ]

-- | 25 D65 greys from Y = 0.005 to 1.0, equal steps in log Y.
neutralRamp :: PatchSet
neutralRamp = PatchSet "neutral-ramp" [Patch ("grey Y=" ++ show y) (scale y d65) | i <- [0 .. 24 :: Int], let y = 0.005 * 200 ** (fromIntegral i / 24)]
 where
  scale k (Xyz x y z) = Xyz (k * x) (k * y) (k * z)

-- | L*a*b* (D65) grid, L* 20..90 by 10, hue by 20 degrees, C* 15..105 by 15,
-- kept where linear Rec.2020 RGB lies in [0, 1].
gamutSet :: PatchSet
gamutSet =
  PatchSet
    "gamut"
    [ Patch ("L" ++ show l ++ " h" ++ show h ++ " C" ++ show c) xyz
    | l <- [20, 30 .. 90 :: Int]
    , h <- [0, 20 .. 340 :: Int]
    , c <- [15, 30 .. 105 :: Int]
    , let hr = fromIntegral h * pi / 180
          xyz = labToXyz d65 (Lab (fromIntegral l) (fromIntegral c * cos hr) (fromIntegral c * sin hr))
    , inRec2020 xyz
    ]

inRec2020 :: Xyz -> Bool
inRec2020 (Xyz x y z) = all (\v -> v >= 0 && v <= 1) [r, g, b]
 where
  (r, g, b) =
    apply
      ((1.7166512, -0.3556708, -0.2533663), (-0.6666844, 1.6164812, 0.0157685), (0.0176399, -0.0427706, 0.9421031))
      (x, y, z)
