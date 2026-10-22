{-# LANGUAGE OverloadedStrings #-}
-- Host analysis of the owner photo session for spec/10 section 19.5.6 (X-M1
-- model check): pixel identity of two JPEGs, ColorChecker patch rectangles,
-- e_model, the model's prediction of the X-M1's Velvia, ASTIA and PRO Neg
-- output from its PROVIA output of the same scene, and the Classic Chrome
-- against PROVIA differences of same-scene frame pairs.
module PhotoSession
  ( Decoder (..)
  , photoIdentical
  , chartPatches
  , eModel
  , Ycc (..)
  , yccLab
  , PairGroup (..)
  , relativePairs
  , pairDeltaStats
  , pairDeltaTables
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
import Data.List (sort)
import Data.Maybe (catMaybes)
import qualified Data.Text as T
import System.Exit (ExitCode (..))
import System.FilePath (takeDirectory, (</>))
import Text.Printf (printf)

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

-- | How a frame file is stored: a JPEG, or a packed full-range 4:2:2 plane
-- of the given size with bytes Y0 Y1 Cb Cr per pixel pair (the x70view
-- live-view captures).
data FrameFormat = JpegFrame | PackedY0Y1CbCr !Int !Int

readFrame :: Decoder -> FrameFormat -> FilePath -> IO Planes
readFrame dec JpegFrame file = decode dec file
readFrame _ (PackedY0Y1CbCr w h) file = do
  bytes <- H.readRegular (2 * w * h + 1) file
  H.ensure (even w && w > 0 && h > 0 && B.length bytes == 2 * w * h) (file ++ ": not a packed " ++ show w ++ "x" ++ show h ++ " Y0 Y1 Cb Cr plane")
  let byteAt i = BU.unsafeIndex bytes i
      plane n at = fst (B.unfoldrN n (\i -> Just (byteAt (at i), i + 1)) 0)
      pairs = w * h `div` 2
  pure (Planes w h Chroma422 (plane (w * h) (\p -> 4 * (p `div` 2) + p `mod` 2)) (plane pairs (\c -> 4 * c + 2)) (plane pairs (\c -> 4 * c + 3)))

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

-- | One region of one film frame: the PROVIA frame's colour, the film frame's,
-- the model's prediction and their CIEDE2000.
data Row = Row
  { rowKind :: T.Text
  , rowName :: T.Text
  , rowProvia :: Lab
  , rowObserved :: Lab
  , rowPredicted :: Lab
  , rowDelta :: Double
  , rowUnchangedDelta :: Double
  , rowClipped :: Bool
  }

-- | A region mean whose sRGB channel lies within half a level of 0 or 255.
clipped :: Mean -> Bool
clipped m = any (\v -> v <= 0.5 || v >= 254.5) [y + 1.402 * cr, y - 0.344136 * cb - 0.714136 * cr, y + 1.772 * cb]
 where
  y = meanY m
  cb = meanCb m - 128
  cr = meanCr m - 128

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
                     in Just (Row kind (rectName r) unchanged observed predicted (ciede2000 observed predicted) (ciede2000 observed unchanged) (clipped obs))
                  _ -> Nothing
              | (kind, r, m, mw) <- inverted
              ]
          summary k = statsValue [rowDelta row | row <- rows, k == "all" || rowKind row == k]
          baseline = statsValue (map rowUnchangedDelta rows)
      pure
        ( key
        , map rowDelta rows
        , object
            [ "frame" .= file
            , "eModel" .= object ["all" .= summary "all", "patches" .= summary "patch", "blocks" .= summary "block"]
            , "proviaAsPrediction" .= baseline
            , "targetClipped" .= length (filter rowClipped rows)
            , "patches" .= [object ["name" .= rowName row, "deltaE00" .= r3 (rowDelta row)] | row <- rows, rowKind row == ("patch" :: T.Text)]
            , "regions"
                .= [ object
                      [ "name" .= rowName row
                      , "kind" .= rowKind row
                      , "provia" .= labJson (rowProvia row)
                      , "observed" .= labJson (rowObserved row)
                      , "predicted" .= labJson (rowPredicted row)
                      , "deltaE00" .= r3 (rowDelta row)
                      , "targetClipped" .= rowClipped row
                      ]
                   | row <- rows
                   ]
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
  labJson (Lab l a b) = map r3 [l, a, b]
  statsValue ds = let Stats m p x = stats ds in object ["mean" .= r3 m, "p95" .= r3 p, "max" .= r3 x, "n" .= length ds]

-- Classic Chrome against PROVIA ----------------------------------------------

data Pair = Pair
  { pairGroup :: T.Text
  , pairProvia :: FilePath
  , pairClassicChrome :: FilePath
  , pairFormat :: FrameFormat
  , pairOffset :: (Int, Int)
  , pairBlockSize :: Int
  , pairMaxLumaSd :: Double
  , pairXm1Prediction :: Bool
  }

-- | Mean 8-bit full-range Y, Cb, Cr of a block.
data Ycc = Ycc {yccY :: !Double, yccCb :: !Double, yccCr :: !Double}

yccLab :: Ycc -> Lab
yccLab (Ycc y cb cr) = ycbcrToLabF y cb cr

-- | A manifest group: its pairs' blocks pooled, as (PROVIA, Classic Chrome)
-- means, and whether the X-M1 stages predict its Classic Chrome frames.
data PairGroup = PairGroup {groupName :: T.Text, groupXm1Prediction :: Bool, groupBlocks :: [(Ycc, Ycc)]}

parsePairs :: FilePath -> Value -> Either String [Pair]
parsePairs dir = AT.parseEither $ A.withObject "manifest" $ \o -> do
  pairs <- o A..: "pairs"
  forM pairs $ A.withObject "pair" $ \p -> do
    packed <- p A..:? "packed"
    format <- maybe (pure JpegFrame) (A.withObject "packed" (\s -> PackedY0Y1CbCr <$> s A..: "width" <*> s A..: "height")) packed
    (dx, dy) <- maybe (pure (0, 0)) offsetPair =<< (p A..:? "offset")
    blocks <- p A..: "blocks"
    (size, maxSd) <- A.withObject "blocks" (\b -> (,) <$> b A..: "size" <*> b A..: "maxLumaSd") blocks
    Pair <$> p A..: "group" <*> ((dir </>) <$> p A..: "provia") <*> ((dir </>) <$> p A..: "classicChrome") <*> pure format <*> pure (dx, dy) <*> pure size <*> pure maxSd <*> p A..:? "xm1Prediction" A..!= False
 where
  offsetPair v = case v of
    [dx, dy] -> pure (dx, dy)
    _ -> fail "offset needs [dx, dy]"

-- | Per pair, the means of every block that is flat in both frames: the
-- PROVIA frame's block at (x, y) and the Classic Chrome frame's at
-- (x + dx, y + dy). Pairs are pooled by group in manifest order; a group
-- is predicted when any of its pairs asks for it.
relativePairs :: Decoder -> FilePath -> IO ([PairGroup], Value)
relativePairs dec manifestPath = do
  manifest <- H.readRegular (16 * 1024 * 1024) manifestPath >>= either H.failH pure . decodeStrict
  pairs <- either H.failH pure (parsePairs (takeDirectory manifestPath) manifest)
  measured <- forM pairs $ \pr -> do
    pp <- readFrame dec (pairFormat pr) (pairProvia pr)
    cp <- readFrame dec (pairFormat pr) (pairClassicChrome pr)
    unless ((planesWidth pp, planesHeight pp) == (planesWidth cp, planesHeight cp)) (H.failH (pairClassicChrome pr ++ ": dimensions differ from the PROVIA frame"))
    let size = pairBlockSize pr
        (dx, dy) = pairOffset pr
        flat m = lumaSd m <= pairMaxLumaSd pr
        blocks =
          [ (yccOf m, yccOf c)
          | by <- [0, size .. planesHeight pp - size]
          , bx <- [0, size .. planesWidth pp - size]
          , Just m <- [rectMean pp (Rect "" bx by size size)]
          , flat m
          , Just c <- [rectMean cp (Rect "" (bx + dx) (by + dy) size size)]
          , flat c
          ]
    pure (pr, blocks)
  let names = foldl (\acc g -> if g `elem` acc then acc else acc ++ [g]) [] (map (pairGroup . fst) measured)
      members g = [m | m@(pr, _) <- measured, pairGroup pr == g]
      groups = [PairGroup g (any (pairXm1Prediction . fst) (members g)) (concatMap snd (members g)) | g <- names]
      pairJson (pr, bs) = object ["group" .= pairGroup pr, "provia" .= pairProvia pr, "classicChrome" .= pairClassicChrome pr, "offset" .= [fst (pairOffset pr), snd (pairOffset pr)], "blockSize" .= pairBlockSize pr, "maxLumaSd" .= pairMaxLumaSd pr, "blocks" .= length bs]
  pure (groups, A.toJSON (map pairJson measured))
 where
  yccOf m = Ycc (meanY m) (meanCb m) (meanCr m)

cab :: Lab -> Double
cab (Lab _ a b) = sqrt (a * a + b * b)

hueDeg :: Lab -> Double
hueDeg (Lab _ a b) = let h = atan2 b a * 180 / pi in if h < 0 then h + 360 else h

-- | 30-degree CIELAB hue sector of the PROVIA colour, Nothing below C* 5.
hueSector :: Lab -> Maybe Int
hueSector l
  | cab l < 5 = Nothing
  | otherwise = Just (30 * (floor (hueDeg l) `div` 30))

chromaBands :: [(T.Text, Double, Double)]
chromaBands = [("C5-15", 5, 15), ("C15-30", 15, 30), ("C30-50", 30, 50), ("C50+", 50, 1 / 0)]

bandOf :: Lab -> Maybe T.Text
bandOf l = case [n | (n, lo, hi) <- chromaBands, cab l >= lo, cab l < hi] of
  (n : _) -> Just n
  [] -> Nothing

lightnessBand :: Lab -> Int
lightnessBand (Lab l _ _) = max 0 (min 90 (10 * floor (l / 10)))

-- | Per-block differences of Classic Chrome from PROVIA: chroma ratio,
-- dL* and hue shift in degrees, wrapped to (-180, 180].
data Delta = Delta {dRatio :: Double, dLight :: Double, dHue :: Double, dChroma :: Double, dProviaC :: Double}

delta :: (Lab, Lab) -> Delta
delta (p@(Lab l0 _ _), c@(Lab l1 _ _)) = Delta (cab c / cab p) (l1 - l0) wrapped (cab c - cab p) (cab p)
 where
  d = hueDeg c - hueDeg p
  wrapped
    | d > 180 = d - 360
    | d <= -180 = d + 360
    | otherwise = d

data Summary = Summary {sN :: Int, sRatio :: Double, sRatioMedian :: Double, sDL :: Double, sDh :: Double, sProviaC :: Double}

summarise :: [(Lab, Lab)] -> Summary
summarise blocks = Summary (length ds) (avg (map dRatio ds)) (median (map dRatio ds)) (avg (map dLight ds)) (avg (map dHue ds)) (avg (map dProviaC ds))
 where
  ds = map delta blocks
  avg xs = if null xs then 0 else sum xs / fromIntegral (length xs)
  median xs = case sort xs of
    [] -> 0
    s -> let n = length s in if odd n then s !! (n `div` 2) else (s !! (n `div` 2 - 1) + s !! (n `div` 2)) / 2

summaryJson :: Summary -> Value
summaryJson s = object ["n" .= sN s, "chromaRatioMean" .= round3 (sRatio s), "chromaRatioMedian" .= round3 (sRatioMedian s), "dL" .= round3 (sDL s), "dhDeg" .= round3 (sDh s), "proviaChroma" .= round3 (sProviaC s)]

round3 :: Double -> Double
round3 x = fromIntegral (round (x * 1000) :: Integer) / 1000

sectorName :: Int -> T.Text
sectorName h = T.pack ("h" ++ show h ++ "-" ++ show (h + 30))

-- | The relative statistics of (PROVIA, Classic Chrome) colour pairs, binned
-- by the PROVIA colour: hue sector, hue sector and chroma band, chroma band,
-- and for the tone response L* band (neutral blocks, C* below 5, and
-- chromatic blocks separately).
pairDeltaStats :: [(Lab, Lab)] -> Value
pairDeltaStats blocks =
  object
    [ "blocks" .= length blocks
    , "chromatic" .= summaryJson (summarise chromatic)
    , "byHueSector" .= [object ["sector" .= sectorName h, "stats" .= summaryJson (summarise bs)] | (h, bs) <- bySector chromatic]
    , "byHueSectorAndChromaBand" .= [object ["sector" .= sectorName h, "band" .= n, "stats" .= summaryJson (summarise xs)] | (h, bs) <- bySector chromatic, (n, _, _) <- chromaBands, let xs = [b | b <- bs, bandOf (fst b) == Just n], not (null xs)]
    , "byChromaBand" .= [object ["band" .= n, "stats" .= summaryJson (summarise xs)] | (n, _, _) <- chromaBands, let xs = [b | b <- chromatic, bandOf (fst b) == Just n], not (null xs)]
    , "tone"
        .= [ object ["proviaL" .= T.pack ("L" ++ show l ++ "-" ++ show (l + 10)), "neutral" .= neutralJson ns, "chromatic" .= summaryJson (summarise cs)]
           | l <- [0, 10 .. 90]
           , let ns = [b | b <- neutral, lightnessBand (fst b) == l]
                 cs = [b | b <- chromatic, lightnessBand (fst b) == l]
           , not (null ns && null cs)
           ]
    ]
 where
  chromatic = [b | b <- blocks, hueSector (fst b) /= Nothing]
  neutral = [b | b <- blocks, hueSector (fst b) == Nothing]
  neutralJson ns = let ds = map delta ns in object ["n" .= length ds, "dL" .= round3 (avg (map dLight ds)), "dC" .= round3 (avg (map dChroma ds))]
  avg xs = if null xs then 0 else sum xs / fromIntegral (length xs)

bySector :: [(Lab, Lab)] -> [(Int, [(Lab, Lab)])]
bySector bs = [(h, xs) | h <- [0, 30 .. 330], let xs = [b | b <- bs, hueSector (fst b) == Just h], not (null xs)]

-- | Markdown tables of the relative statistics, one column per source: per
-- hue sector, per chroma band and per hue sector and chroma band "n, mean
-- chroma ratio, dL*, dh", and the tone response "n, dL*" by PROVIA L* for
-- neutral and for chromatic blocks.
pairDeltaTables :: [(T.Text, [(Lab, Lab)])] -> String
pairDeltaTables sources =
  unlines $
    table "Hue sector (C* >= 5)" [(sectorName h, \bs -> cell [b | b <- bs, hueSector (fst b) == Just h]) | h <- [0, 30 .. 330]]
      ++ [""]
      ++ table "Chroma band" [(n, \bs -> cell [b | b <- bs, bandOf (fst b) == Just n]) | (n, _, _) <- chromaBands]
      ++ [""]
      ++ table "Hue sector, chroma band" [(sectorName h <> " " <> n, \bs -> cell [b | b <- bs, hueSector (fst b) == Just h, bandOf (fst b) == Just n]) | h <- [0, 30 .. 330], (n, _, _) <- chromaBands]
      ++ [""]
      ++ table "PROVIA L* (neutral, C* < 5)" [(lName l, \bs -> toneCell [b | b <- bs, hueSector (fst b) == Nothing, lightnessBand (fst b) == l]) | l <- [0, 10 .. 90]]
      ++ [""]
      ++ table "PROVIA L* (chromatic, C* >= 5)" [(lName l, \bs -> toneCell [b | b <- bs, hueSector (fst b) /= Nothing, lightnessBand (fst b) == l]) | l <- [0, 10 .. 90]]
 where
  table title rows =
    ("| " ++ title ++ concatMap ((" | " ++) . T.unpack . fst) sources ++ " |")
      : ("| ---" ++ concat (replicate (length sources) " | ---") ++ " |")
      : [ "| " ++ T.unpack name ++ concatMap (\(_, bs) -> " | " ++ f bs) sources ++ " |"
        | (name, f) <- rows
        , any (\(_, bs) -> f bs /= "-") sources
        ]
  cell [] = "-"
  cell xs = let s = summarise xs in show (sN s) ++ ", " ++ f2 (sRatio s) ++ ", " ++ s2 (sDL s) ++ ", " ++ s2 (sDh s)
  toneCell [] = "-"
  toneCell xs = show (length xs) ++ ", " ++ s2 (avg (map (dLight . delta) xs))
  lName l = T.pack ("L" ++ show l ++ "-" ++ show (l + 10))
  avg xs = sum xs / fromIntegral (length xs)
  f2 = printf "%.2f" :: Double -> String
  s2 = printf "%+.2f" :: Double -> String
