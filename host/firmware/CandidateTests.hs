{-# LANGUAGE OverloadedStrings #-}
-- The latent-image.classic-chrome-parameters.v1 file on a synthetic
-- candidate: field encoding and chroma storage order against values worked
-- by hand from reports/classic-chrome-patch.md section 7 and
-- reports/film-simulation.md, read-back, and the refusals the build relies
-- on; the chroma-gain scaling (>> 34, floor) and the adjustment range.
module Main (main) where

import Data.Aeson (Value (..))
import qualified Data.Aeson.Key as K
import qualified Data.Aeson.KeyMap as KM
import qualified Data.Vector as V

import FujiAF.ClassicChromeCandidate
import FujiAF.FilmStage
import FujiAF.Host (ensure, failH)
import FujiAF.XM1Develop (Adjustments (..), DynamicRange (..), adjustmentIndices, decodeField, scaleChroma)

main :: IO ()
main = do
  m <- either failH pure (colourMatrix [[1100, -60, -16], [-30, 1080, -26], [20, -120, 1124]])
  c <- either failH pure (chroma 1000 (-30) 60 1100)
  let linear = [4 * s | s <- [0, 21, 43, 64, 85, 107, 128, 171, 213, 256, 341, 427, 512, 683, 853, 1024, 1195, 1365, 1536, 1877, 2219, 2560, 2901, 3243, 3584, 3755, 3925]] ++ [0x3fff]
  t <- either failH pure (toneRecord [2, 2, 3, 4, 5, 5, 5, 6, 6] linear)
  let cand = Candidate "synthetic" [Input "synthetic/bundle.json" (mconcat (replicate 64 "0"))] 115 Nothing m c t
  v <- either failH pure (candidateJson cand)

  -- raw fields: 13-bit two's complement, chroma in storage order +0, +2, +4, +6
  ensure (top v "chroma" == ints [1000, 1100, 0x1fe2, 60]) ("chroma storage order: " ++ show (top v "chroma"))
  ensure (top v "colour_matrix" == Array (V.fromList [ints [1100, 0x1fc4, 0x1ff0], ints [0x1fe2, 1080, 0x1fe6], ints [20, 0x1f88, 1124]])) "matrix field encoding"
  -- the chroma stage reads +0, +4, +6, +2 as (Cr from R-Y, Cr from B-Y, Cb from R-Y, Cb from B-Y)
  let stored = [1000, 1100, 0x1fe2, 60]
      reading = map (decodeField . (stored !!)) [0, 2, 3, 1]
  ensure (reading == [crFromRY c, crFromBY c, cbFromRY c, cbFromBY c]) ("stage reading order gives " ++ show reading)
  ensure (decodeField 0x1fb0 == -80 && decodeField 0x0fb0 == 4016) "field sign from bits 12-15"
  ensure (top v "reuse" == Object (KM.fromList [(k, String "PROVIA") | k <- ["luma_band", "chroma_gain", "hs_tone", "tone_scalar"]])) "reuse is PROVIA"

  -- the file reads back to the candidate
  ensure (parseCandidate v == Right cand) "read-back differs"

  -- refusals
  let refused label w = ensure (either (const True) (const False) (parseCandidate w)) ("accepted " ++ label)
  refused "an unknown field" (set [] "extra" (Number 1) v)
  refused "a fractional e_method" (set ["provenance"] "e_method" (Number 1.5) v)
  refused "a decreasing curve" (set ["tone_record"] "curve" (ints (reverse linear)) v)
  refused "a curve not ending at 0x3fff" (set ["tone_record"] "curve" (ints (init linear ++ [0x3ffe])) v)
  refused "non-PROVIA reuse" (set ["reuse"] "luma_band" (String "ASTIA") v)
  refused "another firmware" (set ["target"] "firmware_sha256" (String (mconcat (replicate 64 "a"))) v)
  refused "a chroma field above u16" (set [] "chroma" (ints [1000, 1100, 0x1fe2, 0x10000]) v)

  -- chroma gains: the five-gain product over 2^34, floored
  ensure (scaleChroma (2 ^ (34 :: Int)) c == Right c) "unity gain product changed the chroma"
  ensure (scaleChroma (9 * 2 ^ (31 :: Int)) c == chroma 1125 (-34) 67 1237) "a 9/8 gain product scaled the chroma wrongly"
  ensure (either (const True) (const False) (adjustmentIndices (Adjustments 3 0 0 DR100))) "Color +3 was admitted"
  ensure (adjustmentIndices (Adjustments 2 (-2) 0 DR400) == Right (0, 4, 2, 3, 2)) "adjustment indices"
  putStrLn "candidate tests passed"
 where
  ints = Array . V.fromList . map (Number . fromIntegral) :: [Int] -> Value
  top (Object o) k = maybe Null id (KM.lookup k o)
  top _ _ = Null
  set :: [K.Key] -> K.Key -> Value -> Value -> Value
  set [] k x (Object o) = Object (KM.insert k x o)
  set (p : ps) k x (Object o) = Object (maybe o (\inner -> KM.insert p (set ps k x inner) o) (KM.lookup p o))
  set _ _ _ other = other
