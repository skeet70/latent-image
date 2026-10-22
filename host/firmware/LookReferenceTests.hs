{-# LANGUAGE OverloadedStrings #-}
module Main (main) where

import Control.Monad (forM_)
import Data.Aeson (Value (..))
import qualified Data.Aeson.Key as K
import qualified Data.Aeson.KeyMap as KM
import qualified Data.ByteString.Char8 as BC
import Data.Either (isLeft)
import Data.Scientific (toRealFloat)

import FujiAF.Colour (Lab (..))
import FujiAF.Host (ensure)
import LookReference

main :: IO ()
main = do
  -- F-Log2 Data Sheet Ver.1.1, section 2-2: 10-bit code values 95, 400 and
  -- 570 for 0 %, 18 % and 90 % reflection
  forM_ [(0, 95), (0.18, 400), (0.9, 570 :: Int)] $ \(x, code) ->
    ensure (round (1023 * fLog2Encode x) == code) ("F-Log2 of " ++ show x ++ " is not code " ++ show code)

  -- both gamuts map RGB white to the D65 white; BT.709 is the sRGB matrix
  -- of FujiAF.Colour, whose white (Z 1.08883) differs from xy (0.3127, 0.3290)
  -- by 2.3e-4 in Z
  forM_ [fGamutToXyz, bt709ToXyz] $ \m -> do
    let (x, y, z) = applyM3 m (1, 1, 1)
    ensure (all (< 1e-3) (zipWith (\u v -> abs (u - v)) [x, y, z] [0.95046, 1, 1.08906])) "RGB white is not D65"
  let srgb = [0.4124564, 0.3575761, 0.1804375, 0.2126729, 0.7151522, 0.0721750, 0.0193339, 0.1191920, 0.9503041]
      ((m11, m12, m13), (m21, m22, m23), (m31, m32, m33)) = bt709ToXyz
  ensure (all (< 3e-4) (zipWith (\u v -> abs (u - v)) [m11, m12, m13, m21, m22, m23, m31, m32, m33] srgb)) "BT.709 matrix differs from sRGB"
  let (rx, gx, bx) = applyM3 (invertM3 fGamutToXyz) (applyM3 fGamutToXyz (0.2, 0.5, 0.7))
  ensure (all (< 1e-12) (zipWith (\u v -> abs (u - v)) [rx, gx, bx] [0.2, 0.5, 0.7])) "F-Gamut matrix inverse is not an inverse"

  -- a cube of a map linear in each channel interpolates it exactly, red fastest
  let affine :: (Double, Double, Double) -> (Double, Double, Double)
      affine (r, g, bb) = (0.5 * r + 0.1, g, 1 - bb)
      grid = [(fromIntegral ri / 2, fromIntegral gi / 2, fromIntegral bi / 2) | bi <- [0 .. 2 :: Int], gi <- [0 .. 2 :: Int], ri <- [0 .. 2 :: Int]]
      row p = let (u, v, w) = affine p in BC.pack (unwords (map show [u, v, w]))
      cubeText = BC.unlines (["# comment", "TITLE \"affine\"", "LUT_3D_SIZE 3", "DOMAIN_MIN 0 0 0", "DOMAIN_MAX 1 1 1", ""] ++ map row grid)
  cube <- either (fail . ("cube refused: " ++)) pure (parseCube cubeText)
  ensure (cubeSize cube == 3) "cube size not read"
  forM_ [(0.3, 0.6, 0.9), (0.05, 0.95, 0.5), (1, 0, 0.25)] $ \p -> do
    let (u, v, w) = applyCube cube p
        (eu, ev, ew) = affine p
    ensure (all (< 1e-9) [abs (u - eu), abs (v - ev), abs (w - ew)]) ("cube interpolation is off at " ++ show p)
  ensure (isLeft (parseCube (BC.unlines (["LUT_3D_SIZE 3"] ++ take 26 (map row grid))))) "a short cube was accepted"
  ensure (isLeft (parseCube (BC.unlines (map row grid)))) "a cube without LUT_3D_SIZE was accepted"
  ensure (isLeft (parseCube (BC.unlines (["LUT_3D_SIZE 3", "DOMAIN_MAX 2 2 2"] ++ map row grid)))) "a non-default domain was accepted"

  -- pairs with a known change: C* halved, L* + 2, hue + 10 degrees; neutrals L* - 1
  let polar l cc hdeg = Lab l (cc * cos (hdeg * pi / 180)) (cc * sin (hdeg * pi / 180))
      chromatic = [LabPair (polar l cc hdeg) (polar (l + 2) (cc / 2) (hdeg + 10)) | l <- [30, 50, 80], cc <- [10, 20, 40], hdeg <- [5, 35 .. 335]]
      neutral = [LabPair (Lab l 1 1) (Lab (l - 1) 1 1) | l <- [25, 55]]
      result = relativeStats (chromatic ++ neutral)
      num v k = case field v k of
        Number s -> toRealFloat s :: Double
        _ -> -999
      close x y = abs (x - y) < 1e-3
  ensure (num (field result "chromatic") "n" == fromIntegral (length chromatic)) "chromatic pairs miscounted"
  case field result "bySector" of
    Object o -> do
      ensure (KM.size o == 12) "not every sector holds pairs"
      forM_ (KM.elems o) $ \s ->
        ensure (close (num s "chromaRatio") 0.5 && close (num s "dL") 2 && close (num s "dh") 10) ("sector statistics wrong: " ++ show s)
    _ -> fail "bySector missing"
  ensure (close (num (field (field result "byChroma") "C15-30") "dC") (-10)) "chroma band C15-30 does not show dC -10"
  ensure (close (num (field result "neutral") "dL") (-1) && num (field result "neutral") "n" == 2) "neutral statistics wrong"
  ensure (num (field (field result "neutralByL") "L20-30") "n" == 1) "neutral L* bins wrong"
  ensure (sector (polar 50 3 90) == "neutral" && sector (polar 50 20 95) == "h90") "sector naming changed"
  putStrLn "look-reference tests passed"
 where
  field (Object o) k = maybe Null id (KM.lookup (K.fromText k) o)
  field _ _ = Null
