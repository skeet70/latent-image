{-# LANGUAGE OverloadedStrings #-}
-- The film-stage model against x70view film_stage.py outputs on synthetic
-- parameter sets (fixtures/film-stage/PROVENANCE.md), CIEDE2000 against the
-- published Sharma, Wu and Dalal (2005) test data, the parameter boundary, the
-- colour fit on a target it can reach, and the inverse stage.
module Main (main) where

import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as BC
import System.Environment (getArgs)
import System.FilePath ((</>))

import FujiAF.Colour
import FujiAF.FilmFit
import FujiAF.FilmGrid (oracleGrid)
import FujiAF.FilmInvert (invert)
import FujiAF.FilmStage
import qualified FujiAF.Host as H
import FujiAF.JSON (decodeStrict)

main :: IO ()
main = do
  args <- getArgs
  dir <- case args of
    [d] -> pure d
    _ -> H.failH "usage: film-stage-tests FIXTURE-DIR"

  -- every grid point equals the oracle's Y, Cb and Cr
  params <- mapM (loadParams dir) ["identity", "synthetic-a", "synthetic-b"]
  mapM_ (checkOracle dir) params

  -- CIEDE2000 on all 34 published pairs, to the table's four decimals
  let wrong = [(p, q, e, ciede2000 p q) | (p, q, e) <- sharma, abs (ciede2000 p q - e) > 0.00005]
  H.ensure (null wrong) ("CIEDE2000 differs from the published value: " ++ show (take 3 wrong))

  -- a field outside its 13-bit range is refused at the boundary
  let (name0, base) = head' params
      overflow = B.concat ["{\"gainTableQ9\":[", list (gainTableQ9 (filmGainTable base)), "],\"matrixQ10\":[[4096,-3072,0],[0,1024,0],[0,0,1024]],", rest base, "}"]
  H.ensure (either (const True) (const False) (decodeStrict overflow >>= parseFilmParams)) (name0 ++ ": a 4096 matrix field was admitted")

  -- the fit reaches a target rendered by the same stage with another setting
  let (_, a) = params !! 1
      stage = compileStage a
      exposure = Exposure 9000
      patches = map patchXyz (setPatches colorChecker24 ++ take 120 (setPatches gamutSet))
      inputs = map (sceneInput xm1Camera exposure) patches
  goal <- maybe (H.failH "goal setting outside the fields") pure (settingOf [-250, -40, -90, -60, 30, -380, 1400, -40, 120, 1700])
  start <- maybe (H.failH "start setting outside the fields") pure (settingOf [0, 0, 0, 0, 0, 0, 1024, 0, 0, 1024])
  let target = map (renderLab (applySetting goal stage)) inputs
      fit = fitColour stage inputs target [start]
      Stats m _ x = stats (deltas (applySetting (fitSetting fit) stage) inputs target)
  H.ensure (m < 0.05 && x < 0.5) ("fit did not reach a reachable target: mean " ++ show m ++ ", max " ++ show x)

  -- an output the inverse accepts develops again to within one level; the
  -- stage is synthetic-a with the identity set's linear tone record
  let (_, ident) = head' params
      linear = compileStage a {filmTone = filmTone ident}
      redeveloped =
        [ (o, r)
        | w <- oracleGrid
        , let o@(YCbCr y cb cr) = develop linear w
        , Just w' <- [invert linear (fromIntegral y) (fromIntegral cb) (fromIntegral cr)]
        , let r = develop linear w'
        ]
      far = [(o, r) | (o@(YCbCr a0 b0 c0), r@(YCbCr a1 b1 c1)) <- redeveloped, maximum (map abs [a0 - a1, b0 - b1, c0 - c1]) > 1]
  H.ensure (length redeveloped >= 2000) ("the inverse accepted only " ++ show (length redeveloped) ++ " grid outputs")
  H.ensure (length far * 200 <= length redeveloped) ("inverse round trip off by more than one level at " ++ show (length far) ++ " points, first " ++ show (take 2 far))
  putStrLn "film-stage tests passed"
 where
  head' (p : _) = p
  head' [] = error "no parameter sets"
  list = B.intercalate "," . map (BC.pack . show)
  rest p =
    B.concat
      [ "\"tone\":{\"exponents\":[", list (toneExponents (filmTone p)), "],\"points\":[", list (tonePoints (filmTone p)), "]},"
      , "\"chromaQ10\":{\"crFromRY\":1024,\"crFromBY\":0,\"cbFromRY\":0,\"cbFromBY\":1024},"
      , "\"lumaBands\":{\"bounds\":[64,128,192,255],\"bands\":[", B.intercalate "," (replicate 5 "{\"shift\":0,\"gain\":256}"), "]}"
      ]

loadParams :: FilePath -> String -> IO (String, FilmParams)
loadParams dir name = do
  bytes <- B.readFile (dir </> (name ++ ".params.json"))
  p <- either (H.failH . ((name ++ ": ") ++)) pure (decodeStrict bytes >>= parseFilmParams)
  pure (name, p)

checkOracle :: FilePath -> (String, FilmParams) -> IO ()
checkOracle dir (name, p) = do
  bytes <- B.readFile (dir </> (name ++ ".ycc"))
  H.ensure (B.length bytes == 3 * length oracleGrid) (name ++ ": oracle output has the wrong size")
  let st = compileStage p
      at i = fromIntegral (B.index bytes i)
      misses = [(w, develop st w) | (i, w) <- zip [0 ..] oracleGrid, develop st w /= YCbCr (at (3 * i)) (at (3 * i + 1)) (at (3 * i + 2))]
  H.ensure (null misses) (name ++ ": " ++ show (length misses) ++ " grid points differ from film_stage.py, first " ++ show (take 1 misses))

sharma :: [(Lab, Lab, Double)]
sharma =
  [ (Lab l1 a1 b1, Lab l2 a2 b2, e)
  | [l1, a1, b1, l2, a2, b2, e] <-
      [ [50.0000, 2.6772, -79.7751, 50.0000, 0.0000, -82.7485, 2.0425]
      , [50.0000, 3.1571, -77.2803, 50.0000, 0.0000, -82.7485, 2.8615]
      , [50.0000, 2.8361, -74.0200, 50.0000, 0.0000, -82.7485, 3.4412]
      , [50.0000, -1.3802, -84.2814, 50.0000, 0.0000, -82.7485, 1.0000]
      , [50.0000, -1.1848, -84.8006, 50.0000, 0.0000, -82.7485, 1.0000]
      , [50.0000, -0.9009, -85.5211, 50.0000, 0.0000, -82.7485, 1.0000]
      , [50.0000, 0.0000, 0.0000, 50.0000, -1.0000, 2.0000, 2.3669]
      , [50.0000, -1.0000, 2.0000, 50.0000, 0.0000, 0.0000, 2.3669]
      , [50.0000, 2.4900, -0.0010, 50.0000, -2.4900, 0.0009, 7.1792]
      , [50.0000, 2.4900, -0.0010, 50.0000, -2.4900, 0.0010, 7.1792]
      , [50.0000, 2.4900, -0.0010, 50.0000, -2.4900, 0.0011, 7.2195]
      , [50.0000, 2.4900, -0.0010, 50.0000, -2.4900, 0.0012, 7.2195]
      , [50.0000, -0.0010, 2.4900, 50.0000, 0.0009, -2.4900, 4.8045]
      , [50.0000, -0.0010, 2.4900, 50.0000, 0.0010, -2.4900, 4.8045]
      , [50.0000, -0.0010, 2.4900, 50.0000, 0.0011, -2.4900, 4.7461]
      , [50.0000, 2.5000, 0.0000, 50.0000, 0.0000, -2.5000, 4.3065]
      , [50.0000, 2.5000, 0.0000, 73.0000, 25.0000, -18.0000, 27.1492]
      , [50.0000, 2.5000, 0.0000, 61.0000, -5.0000, 29.0000, 22.8977]
      , [50.0000, 2.5000, 0.0000, 56.0000, -27.0000, -3.0000, 31.9030]
      , [50.0000, 2.5000, 0.0000, 58.0000, 24.0000, 15.0000, 19.4535]
      , [50.0000, 2.5000, 0.0000, 50.0000, 3.1736, 0.5854, 1.0000]
      , [50.0000, 2.5000, 0.0000, 50.0000, 3.2972, 0.0000, 1.0000]
      , [50.0000, 2.5000, 0.0000, 50.0000, 1.8634, 0.5757, 1.0000]
      , [50.0000, 2.5000, 0.0000, 50.0000, 3.2592, 0.3350, 1.0000]
      , [60.2574, -34.0099, 36.2677, 60.4626, -34.1751, 39.4387, 1.2644]
      , [63.0109, -31.0961, -5.8663, 62.8187, -29.7946, -4.0864, 1.2630]
      , [61.2901, 3.7196, -5.3901, 61.4292, 2.2480, -4.9620, 1.8731]
      , [35.0831, -44.1164, 3.7933, 35.0232, -40.0716, 1.5901, 1.8645]
      , [22.7233, 20.0904, -46.6940, 23.0331, 14.9730, -42.5619, 2.0373]
      , [36.4612, 47.8580, 18.3852, 36.2715, 50.5065, 21.2231, 1.4146]
      , [90.8027, -2.0831, 1.4410, 91.1528, -1.6435, 0.0447, 1.4441]
      , [90.9257, -0.5406, -0.9208, 88.6381, -0.8985, -0.7239, 1.5381]
      , [6.7747, -0.2908, -2.4247, 5.8714, -0.0985, -2.2286, 0.6377]
      , [2.0776, 0.0795, -1.1350, 0.9033, -0.0636, -0.5514, 0.9082]
      ]
  ]
