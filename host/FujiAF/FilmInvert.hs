-- Stage input recovered from a developed (Y, Cb, Cr): the inverse of
-- FujiAF.FilmStage.develop in floating point, for predicting one film's
-- output from another film's output of the same scene.
module FujiAF.FilmInvert (invert) where

import qualified Data.Vector.Unboxed as U

import FujiAF.FilmStage

-- | Stage input that develops to the mean levels (Y, Cb, Cr): luma band,
-- chroma 2x2 and BT.601 undone, each 8-bit level read as the middle of its
-- 64-wide curve bin, curve A and the matrix inverted, the gain table undone by
-- fixed-point iteration. Nothing when Cb, Cr or an 8-bit channel is within
-- half a level of 0 or 255, the band factor or a determinant is zero, or the
-- input leaves 14 bits.
invert :: Stage -> Double -> Double -> Double -> Maybe Rgb14
invert st y cb cr = do
  mapM_ inside [cb, cr]
  let bounds = bandBounds (stageBands st)
      LumaBand shift gain = bandList (stageBands st) !! length (filter (<= round y) bounds)
      f = fromIntegral gain + fromIntegral shift * y / 256
  nonZero f
  let cbSum = (cb - 128) * 1024 / f * 1024 / 578 * 256
      crSum = (cr - 128) * 1024 / f * 1024 / 730 * 256
      Chroma c0 c1 c3 c2 = stageChroma st
      det = fromIntegral (c0 * c2 - c1 * c3)
  nonZero det
  let dr = (crSum * fromIntegral c2 - cbSum * fromIntegral c1) / det
      db = (cbSum * fromIntegral c0 - crSum * fromIntegral c3) / det
      r8 = y + dr
      b8 = y + db
      g8 = (1024 * y - 306 * r8 - 117 * b8) / 601
  mapM_ inside [r8, g8, b8]
  let post = map (curveInverse (stageTone st) . (\v -> 64 * v + 31.5)) [r8, g8, b8]
  pre <- solve3 (map (map ((/ 1024) . fromIntegral)) (matrixRows (stageMatrix st))) post
  let w = iterate (step pre) (map (* 4) pre) !! 40
  [wr, wg, wb] <- traverse within w
  pure (Rgb14 wr wg wb)
 where
  nonZero v = if v == 0 then Nothing else Just ()
  inside v = if v > 0.5 && v < 254.5 then Just () else Nothing
  within v = let q = round v in if q >= 0 && q <= 16383 then Just q else Nothing
  gainAt w =
    let x12 v = min 4095 (max 0 (floor (v / 4))) :: Int
        b = case map x12 w of
          [r, g, bb] -> (3 * r + 7 * g + 6 * bb) `div` 16
          _ -> 0
     in fromIntegral (stageGain st U.! b)
  step pre w = let g = gainAt w in map (\x -> x * 2048 / g) pre

-- | Input at which a non-decreasing curve LUT reaches c, linear between entries.
curveInverse :: U.Vector Int -> Double -> Double
curveInverse lut c = go 0 (U.length lut - 1)
 where
  at i = fromIntegral (lut U.! i) :: Double
  go lo hi
    | hi - lo <= 1 = if at hi == at lo then fromIntegral lo else fromIntegral lo + (c - at lo) / (at hi - at lo)
    | at mid <= c = go mid hi
    | otherwise = go lo mid
   where
    mid = (lo + hi) `div` 2

solve3 :: [[Double]] -> [Double] -> Maybe [Double]
solve3 [[a, b, c], [d, e, f], [g, h, i]] [p, q, r]
  | det == 0 = Nothing
  | otherwise =
      Just
        [ (p * (e * i - f * h) - b * (q * i - f * r) + c * (q * h - e * r)) / det
        , (a * (q * i - f * r) - p * (d * i - f * g) + c * (d * r - q * g)) / det
        , (a * (e * r - q * h) - b * (d * r - q * g) + p * (d * h - e * g)) / det
        ]
 where
  det = a * (e * i - f * h) - b * (d * i - f * g) + c * (d * h - e * g)
solve3 _ _ = Nothing
