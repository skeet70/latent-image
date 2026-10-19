-- The deterministic 14-bit input grid of the film-stage oracle check.
module FujiAF.FilmGrid (oracleGrid) where

import Data.Bits (shiftL, shiftR, (.&.))
import Data.Word (Word32)

import FujiAF.FilmStage (Rgb14 (..))

-- | 1728 triples of 12 levels, 4096 uniform points and 1024 points uniform in
-- log2 of the value, both from the 32-bit LCG s' = 1664525 s + 1013904223
-- seeded with 0x5eed.
oracleGrid :: [Rgb14]
oracleGrid = lattice ++ uniform ++ logarithmic
 where
  levels = [0, 5, 37, 130, 400, 900, 1700, 3000, 5000, 8000, 12000, 16383]
  lattice = [Rgb14 r g b | r <- levels, g <- levels, b <- levels]
  stream = drop 1 (iterate (\s -> 1664525 * s + 1013904223) (0x5eed :: Word32))
  (uniformDraws, rest) = splitAt (3 * 4096) stream
  logDraws = take (3 * 1024) rest
  uniform = triples [fromIntegral (s `shiftR` 18) | s <- uniformDraws]
  logarithmic = triples [logValue s | s <- logDraws]
  logValue s =
    let e = fromIntegral (s `shiftR` 28) :: Int
        m = fromIntegral ((s `shiftR` 14) .&. 0x3fff) :: Int
     in min 16383 (((1 `shiftL` e) * (16384 + m)) `shiftR` 14)
  triples (r : g : b : xs) = Rgb14 r g b : triples xs
  triples _ = []
