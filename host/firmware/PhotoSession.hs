{-# LANGUAGE OverloadedStrings #-}
-- Host analysis of the owner photo session for spec/10 section 19.5.6 (X-M1
-- model check): pixel identity of two JPEGs, ColorChecker patch rectangles,
-- and e_model, the model's prediction of the X-M1's Velvia, ASTIA and PRO Neg
-- output from its PROVIA output of the same scene.
module PhotoSession
  ( Decoder (..)
  , photoIdentical
  , chartPatches
  , eModel
  ) where

import Control.Monad (forM, unless, when)
import Data.Aeson (Value (..), object, (.=))
import qualified Data.Aeson as A
import qualified Data.Aeson.Key as K
import qualified Data.Aeson.KeyMap as KM
import qualified Data.Aeson.Types as AT
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as BC
import qualified Data.ByteString.Unsafe as BU
import Data.Maybe (catMaybes)
import qualified Data.Text as T
import System.Exit (ExitCode (..))
import System.FilePath (takeDirectory, (</>))

import FujiAF.Colour
import FujiAF.FilmFit (Stats (..), stats)
import FujiAF.FilmInvert (invert)
import FujiAF.FilmStage
import qualified FujiAF.Host as H
import FujiAF.JSON (decodeStrict)

data Decoder = Decoder {decoderFfmpeg :: FilePath, decoderFfprobe :: FilePath}

data ChromaLayout = Chroma444 | Chroma422 | Chroma420 deriving (Eq, Show)

-- | A decoded JPEG's native planes.
data Planes = Planes
  { planesWidth :: !Int
  , planesHeight :: !Int
  , planesLayout :: !ChromaLayout
  , planesY :: !B.ByteString
  , planesCb :: !B.ByteString
  , planesCr :: !B.ByteString
  }

chromaSize :: ChromaLayout -> Int -> Int -> (Int, Int)
chromaSize layout w h = case layout of
  Chroma444 -> (w, h)
  Chroma422 -> ((w + 1) `div` 2, h)
  Chroma420 -> ((w + 1) `div` 2, (h + 1) `div` 2)

decode :: Decoder -> FilePath -> IO Planes
decode dec file = do
  (t1, c1, probe, e1) <- H.runBounded 120000000 4096 (decoderFfprobe dec) ["-v", "error", "-select_streams", "v:0", "-show_entries", "stream=width,height,pix_fmt", "-of", "csv=p=0", file] B.empty
  H.ensure (not t1 && c1 == ExitSuccess) ("ffprobe failed on " ++ file ++ ": " ++ BC.unpack (B.take 300 e1))
  (w, h, layout) <- case BC.split ',' (BC.strip probe) of
    [ws, hs, fmt] | Just (w, _) <- BC.readInt ws, Just (h, _) <- BC.readInt hs -> do
      layout <- case fmt of
        "yuvj444p" -> pure Chroma444
        "yuvj422p" -> pure Chroma422
        "yuvj420p" -> pure Chroma420
        _ -> H.failH (file ++ ": unsupported pixel format " ++ BC.unpack fmt)
      pure (w, h, layout)
    _ -> H.failH (file ++ ": unexpected ffprobe output " ++ BC.unpack probe)
  let (cw, ch) = chromaSize layout w h
      total = w * h + 2 * cw * ch
  (t2, c2, raw, e2) <- H.runBounded 300000000 (total + 1) (decoderFfmpeg dec) ["-v", "error", "-nostdin", "-i", file, "-f", "rawvideo", "-"] B.empty
  H.ensure (not t2 && c2 == ExitSuccess) ("ffmpeg failed on " ++ file ++ ": " ++ BC.unpack (B.take 300 e2))
  H.ensure (B.length raw == total) (file ++ ": decoded size does not match its planes")
  let (yp, rest) = B.splitAt (w * h) raw
      (cbp, crp) = B.splitAt (cw * ch) rest
  pure (Planes w h layout yp cbp crp)

-- | Exit status 0 when the two JPEGs decode to identical planes.
photoIdentical :: Decoder -> FilePath -> FilePath -> IO Bool
photoIdentical dec a b = do
  pa <- decode dec a
  pb <- decode dec b
  let sameShape = (planesWidth pa, planesHeight pa, planesLayout pa) == (planesWidth pb, planesHeight pb, planesLayout pb)
      differing p q = length (filter id (B.zipWith (/=) p q))
  if not sameShape
    then putStrLn "different dimensions or chroma layout" >> pure False
    else do
      let counts = [differing (planesY pa) (planesY pb), differing (planesCb pa) (planesCb pb), differing (planesCr pa) (planesCr pb)]
      putStrLn ("differing samples Y, Cb, Cr: " ++ show counts ++ " of " ++ show [B.length (planesY pa), B.length (planesCb pa), B.length (planesCr pa)])
      pure (all (== 0) counts)

data Rect = Rect {rectName :: T.Text, rectX :: Int, rectY :: Int, rectW :: Int, rectH :: Int}

rectJson :: Rect -> Value
rectJson r = object ["name" .= rectName r, "x" .= rectX r, "y" .= rectY r, "w" .= rectW r, "h" .= rectH r]

colorCheckerNames :: [T.Text]
colorCheckerNames =
  [ "dark skin", "light skin", "blue sky", "foliage", "blue flower", "bluish green"
  , "orange", "purplish blue", "moderate red", "purple", "yellow green", "orange yellow"
  , "blue", "green", "red", "yellow", "magenta", "cyan"
  , "white 9.5", "neutral 8", "neutral 6.5", "neutral 5", "neutral 3.5", "black 2"
  ]

-- | The 24 ColorChecker Classic patch squares of side `size` from the centres
-- of dark skin, bluish green, white 9.5 and black 2, interpolated bilinearly.
chartPatches :: (Double, Double) -> (Double, Double) -> (Double, Double) -> (Double, Double) -> Int -> Value
chartPatches tl tr bl br size =
  A.toJSON
    [ rectJson (Rect name (round cx - size `div` 2) (round cy - size `div` 2) size size)
    | (i, name) <- zip [0 :: Int ..] colorCheckerNames
    , let (r, c) = i `divMod` 6
          u = fromIntegral c / 5
          v = fromIntegral r / 3
          lerp (x0, y0) (x1, y1) t = (x0 + (x1 - x0) * t, y0 + (y1 - y0) * t)
          (cx, cy) = lerp (lerp tl tr u) (lerp bl br u) v
    ]

data Mean = Mean {meanY :: Double, meanCb :: Double, meanCr :: Double, lumaSd :: Double}

-- | Means over the central half of a rectangle in each dimension.
rectMean :: Planes -> Rect -> Maybe Mean
rectMean p r
  | x0 < 0 || y0 < 0 || x1 > planesWidth p || y1 > planesHeight p || x1 <= x0 || y1 <= y0 = Nothing
  | otherwise = Just (Mean my (avg cbs) (avg crs) (sqrt (max 0 (avg (map (\v -> v * v) ys) - my * my))))
 where
  x0 = rectX r + rectW r `div` 4
  y0 = rectY r + rectH r `div` 4
  x1 = x0 + max 1 (rectW r `div` 2)
  y1 = y0 + max 1 (rectH r `div` 2)
  (cw, _) = chromaSize (planesLayout p) (planesWidth p) (planesHeight p)
  (sx, sy) = case planesLayout p of
    Chroma444 -> (1, 1)
    Chroma422 -> (2, 1)
    Chroma420 -> (2, 2)
  sample plane stride x y = fromIntegral (BU.unsafeIndex plane (y * stride + x)) :: Double
  ys = [sample (planesY p) (planesWidth p) x y | y <- [y0 .. y1 - 1], x <- [x0 .. x1 - 1]]
  cpos = [(x `div` sx, y `div` sy) | y <- [y0, y0 + sy .. y1 - 1], x <- [x0, x0 + sx .. x1 - 1]]
  cbs = [sample (planesCb p) cw x y | (x, y) <- cpos]
  crs = [sample (planesCr p) cw x y | (x, y) <- cpos]
  my = avg ys
  avg xs = sum xs / fromIntegral (max 1 (length xs))

data Scene = Scene
  { sceneName :: T.Text
  , sceneFrames :: [(T.Text, FilePath)]
  , scenePatches :: [Rect]
  , sceneBlocks :: Maybe (Int, Double)
  }

parseManifest :: FilePath -> Value -> Either String [Scene]
parseManifest dir = AT.parseEither $ A.withObject "manifest" $ \o -> do
  scenes <- o A..: "scenes"
  forM scenes $ A.withObject "scene" $ \s -> do
    name <- s A..: "name"
    frames <- s A..: "frames"
    rects <- s A..:? "patches" A..!= []
    patches <- forM rects $ A.withObject "patch" $ \r -> Rect <$> r A..: "name" <*> r A..: "x" <*> r A..: "y" <*> r A..: "w" <*> r A..: "h"
    blocks <- s A..:? "blocks"
    blockSpec <- traverse (A.withObject "blocks" (\b -> (,) <$> b A..: "size" <*> b A..: "maxLumaSd")) blocks
    framesList <- A.withObject "frames" (\f -> traverse (\(k, v) -> (K.toText k,) . (dir </>) <$> A.parseJSON v) (KM.toList f)) frames
    pure (Scene name framesList patches blockSpec)

-- | Prediction of each film's frame from the scene's PROVIA frame through
-- the X-M1 stages of `films` (film key to parameters, "provia" included).
eModel :: Decoder -> [(T.Text, FilmParams)] -> FilePath -> IO Value
eModel dec films manifestPath = do
  manifest <- H.readRegular (16 * 1024 * 1024) manifestPath >>= either H.failH pure . decodeStrict
  scenes <- either H.failH pure (parseManifest (takeDirectory manifestPath) manifest)
  proviaParams <- maybe (H.failH "parameters for provia missing") pure (lookup "provia" films)
  let provia = compileStage proviaParams
  perScene <- forM scenes $ \sc -> do
    proviaPath <- maybe (H.failH (T.unpack (sceneName sc) ++ ": no provia frame")) pure (lookup "provia" (sceneFrames sc))
    pp <- decode dec proviaPath
    let blockRects = case sceneBlocks sc of
          Nothing -> []
          Just (size, maxSd) ->
            [ r
            | by <- [0, size .. planesHeight pp - size]
            , bx <- [0, size .. planesWidth pp - size]
            , let r = Rect (T.pack ("block " ++ show bx ++ "," ++ show by)) bx by size size
            , Just m <- [rectMean pp r]
            , lumaSd m <= maxSd
            ]
        regions = [("patch", r) | r <- scenePatches sc] ++ [("block", r) | r <- blockRects]
        sources = [(kind, r, m) | (kind, r) <- regions, Just m <- [rectMean pp r]]
        inverted = [(kind, r, m, invert provia (meanY m) (meanCb m) (meanCr m)) | (kind, r, m) <- sources]
        selfCheck = [ciede2000 (ycbcrToLabF (meanY m) (meanCb m) (meanCr m)) (labOf (develop provia w)) | (_, _, m, Just w) <- inverted]
    when (null sources) (H.failH (T.unpack (sceneName sc) ++ ": no patches or blocks"))
    filmResults <- forM [(k, f) | (k, f) <- sceneFrames sc, k /= "provia"] $ \(key, file) -> do
      params <- maybe (H.failH (T.unpack key ++ ": no X-M1 parameters for this film")) pure (lookup key films)
      fp <- decode dec file
      unless ((planesWidth fp, planesHeight fp) == (planesWidth pp, planesHeight pp)) (H.failH (file ++ ": dimensions differ from the PROVIA frame"))
      let stage = compileStage params
          rows =
            catMaybes
              [ case (mw, rectMean fp r) of
                  (Just w, Just obs) ->
                    let observed = ycbcrToLabF (meanY obs) (meanCb obs) (meanCr obs)
                        predicted = labOf (develop stage w)
                        unchanged = ycbcrToLabF (meanY m) (meanCb m) (meanCr m)
                     in Just (kind, rectName r, ciede2000 observed predicted, ciede2000 observed unchanged)
                  _ -> Nothing
              | (kind, r, m, mw) <- inverted
              ]
          summary k = let ds = [d | (kd, _, d, _) <- rows, k == "all" || kd == k] in statsValue ds
          baseline = statsValue [u | (_, _, _, u) <- rows]
      pure
        ( key
        , [d | (_, _, d, _) <- rows]
        , object
            [ "frame" .= file
            , "eModel" .= object ["all" .= summary "all", "patches" .= summary "patch", "blocks" .= summary "block"]
            , "proviaAsPrediction" .= baseline
            , "patches" .= [object ["name" .= n, "deltaE00" .= r3 d] | (k, n, d, _) <- rows, k == ("patch" :: T.Text)]
            ]
        )
    pure
      ( sceneName sc
      , [(k, ds) | (k, ds, _) <- filmResults]
      , object
          [ "regions" .= length sources
          , "notInvertible" .= length [() | (_, _, _, Nothing) <- inverted]
          , "proviaRedevelopResidual" .= statsValue selfCheck
          , "films" .= object [K.fromText k .= v | (k, _, v) <- filmResults]
          ]
      )
  let keys = foldr (\k acc -> if k `elem` acc then acc else k : acc) [] [k | (_, rs, _) <- perScene, (k, _) <- rs]
  pure $
    object
      [ "scenes" .= object [K.fromText n .= v | (n, _, v) <- perScene]
      , "eModelByFilm" .= object [K.fromText k .= statsValue (concat [ds | (_, rs, _) <- perScene, (k', ds) <- rs, k' == k]) | k <- keys]
      ]
 where
  labOf (YCbCr y cb cr) = ycbcrToLab y cb cr
  r3 x = fromIntegral (round (x * 1000) :: Integer) / 1000 :: Double
  statsValue ds = let Stats m p x = stats ds in object ["mean" .= r3 m, "p95" .= r3 p, "max" .= r3 x, "n" .= length ds]
