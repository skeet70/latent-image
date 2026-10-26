{-# LANGUAGE OverloadedStrings #-}
-- The photo-session commands of //host/firmware:classic-chrome-model through
-- its command line, on flat-patch JPEGs written here by the pinned ffmpeg from
-- outputs of the synthetic fixture stages.
module Main (main) where

import Control.Monad (forM_, unless)
import Data.Aeson (Value (..), object, (.=))
import qualified Data.Aeson as A
import qualified Data.Aeson.Key as K
import qualified Data.Aeson.KeyMap as KM
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as BC
import System.Environment (getArgs)
import System.Exit (ExitCode (..))
import System.FilePath ((</>))

import FujiAF.ClassicChromeCandidate (Candidate (..), candidateJson)
import FujiAF.FilmStage
import FujiAF.Host (ensure, failH, runBounded, withTemporary)
import FujiAF.JSON (decodeStrict, encodeCanonical)

-- | Four 64 x 64 patches side by side, each one stage output.
patchInputs :: [Rgb14]
patchInputs = [Rgb14 4000 3000 2000, Rgb14 2000 5000 3000, Rgb14 6000 6000 6000, Rgb14 3000 2500 6000]

main :: IO ()
main = do
  [tool, ffmpeg, ffprobe, fixtures] <- getArgs
  baseJson <- B.readFile (fixtures </> "synthetic-a.params.json") >>= either failH pure . decodeStrict
  provia <- either failH pure (parseFilmParams baseJson)
  let otherJson = case baseJson of
        Object o -> Object (KM.insert "chromaQ10" (object ["crFromRY" .= (2600 :: Int), "crFromBY" .= (-200 :: Int), "cbFromRY" .= (400 :: Int), "cbFromBY" .= (3000 :: Int)]) o)
        v -> v
  other <- either failH pure (parseFilmParams otherJson)
  withTemporary "classic-chrome-photo" $ \root -> do
    let filmKeys = ["provia", "velvia", "astia", "proNegStd", "proNegHi", "classicChrome"] :: [String]
        entry k = object (("xtrans2" .= baseJson) : ["xm1" .= (if k == "velvia" then otherJson else baseJson) | k /= "classicChrome"])
        bundle = object ["films" .= object [(fromStr k, entry k) | k <- filmKeys]]
        bundlePath = root </> "bundle.json"
    B.writeFile bundlePath (encodeCanonical bundle)
    writeJpeg ffmpeg (root </> "provia.jpg") (map (develop (compileStage provia)) patchInputs)
    writeJpeg ffmpeg (root </> "velvia.jpg") (map (develop (compileStage other)) patchInputs)

    -- identical decodes pass, different ones fail
    (c1, _) <- run tool ["photo-identical", "--ffmpeg", ffmpeg, "--ffprobe", ffprobe, root </> "provia.jpg", root </> "provia.jpg"]
    ensure (c1 == ExitSuccess) "a JPEG did not decode identical to itself"
    (c2, _) <- run tool ["photo-identical", "--ffmpeg", ffmpeg, "--ffprobe", ffprobe, root </> "provia.jpg", root </> "velvia.jpg"]
    ensure (c2 /= ExitSuccess) "two different JPEGs were reported identical"

    let difference name a b = do
          let out = root </> name
          (code, _) <- run tool ["photo-difference", "--ffmpeg", ffmpeg, "--ffprobe", ffprobe, root </> a, root </> b, out]
          ensure (code == ExitSuccess) "photo-difference failed"
          B.readFile out >>= either failH pure . decodeStrict
        plane result key = field (field result "planes") key
    identical <- difference "identical.json" "provia.jpg" "provia.jpg"
    forward <- difference "forward.json" "provia.jpg" "velvia.jpg"
    backward <- difference "backward.json" "velvia.jpg" "provia.jpg"
    forM_ ["Y", "Cb", "Cr"] $ \key -> do
      let self = plane identical key
          f = plane forward key
          b = plane backward key
      ensure (num self "samples" == 256 * 64) "photo-difference omitted native samples"
      ensure (num self "differing" == 0 && numD (field self "maxAbsolute") == 0) "an identical plane has nonzero differences"
      ensure (field self "histogram" == A.toJSON [object ["difference" .= (0 :: Int), "samples" .= (256 * 64 :: Int)]]) "an identical plane has the wrong histogram"
      ensure (num f "differing" == num b "differing") "reversing images changed the number of differences"
      ensure (abs (numD (field f "meanSigned") + numD (field b "meanSigned")) < 1e-12) "reversing images did not reverse signed differences"
      ensure (field f "rootMeanSquare" == field b "rootMeanSquare") "reversing images changed RMS error"
    ensure (any (\key -> num (plane forward key) "differing" > 0) ["Y", "Cb", "Cr"]) "photo-difference lost a changed image"

    let wb = object ["green" .= (302 :: Int), "red" .= (342 :: Int), "blue" .= (876 :: Int)]
        captureManifest = object ["shot" .= ("provia.jpg" :: String), "conversion" .= ("provia.jpg" :: String), "selectedWb" .= wb, "autoWb" .= wb, "blockSize" .= (32 :: Int), "maxLumaSd" .= (1 :: Double)]
    B.writeFile (root </> "capture.json") (encodeCanonical captureManifest)
    (captureCode, _) <- run tool ["capture-difference", "--ffmpeg", ffmpeg, "--ffprobe", ffprobe, bundlePath, root </> "capture.json", root </> "capture-result.json"]
    ensure (captureCode == ExitSuccess) "capture-difference failed"
    capture <- B.readFile (root </> "capture-result.json") >>= either failH pure . decodeStrict
    ensure (num capture "invertibleBlocks" > 0) "capture-difference omitted all flat patches"
    ensure (numD (field (field capture "unchangedShotResidual") "max") == 0) "an identical capture pair has a nonzero observed residual"
    case field capture "hypotheses" of
      Array hypotheses -> do
        let named name = [h | h <- foldr (:) [] hypotheses, field h "name" == A.toJSON (name :: String)]
        case (named "identity", named "auto-instead-of-selected-wb", named "fitted-diagonal-input-gain") of
          ([identity], [auto], [fitted]) -> do
            ensure (field identity "conversionResidual" == field auto "conversionResidual") "equal WB triplets changed the prediction"
            ensure (field fitted "rgbInputGain" == A.toJSON ([1, 1, 1] :: [Double])) "an identical capture pair has non-unity fitted gain"
          _ -> failH "capture-difference omitted a gain counterfactual"
      _ -> failH "capture-difference did not report gain counterfactuals"

    -- the chart grid puts the corner patches on the given centres
    (c3, out3) <- run tool ["chart-patches", "100,50", "600,50", "100,350", "600,350", "40"]
    ensure (c3 == ExitSuccess) "chart-patches failed"
    rects <- either failH pure (decodeStrict out3)
    case rects of
      Array xs | length xs == 24 -> do
        let centre v = (num v "x" + 20, num v "y" + 20)
            items = foldr (:) [] xs
        ensure (centre (head' items) == (100, 50)) "dark skin is not at its corner"
        ensure (centre (last items) == (600, 350)) "black 2 is not at its corner"
      _ -> failH "chart-patches did not give 24 rectangles"

    -- the model's own film output is predicted from its PROVIA output; the
    -- PROVIA frame offered as the film frame is not
    let patchList = [object ["name" .= ("p" ++ show i), "x" .= (64 * i), "y" .= (0 :: Int), "w" .= (64 :: Int), "h" .= (64 :: Int)] | i <- [0 .. 3 :: Int]]
        manifest film = object ["scenes" .= [object ["name" .= ("flat" :: String), "frames" .= object ["provia" .= ("provia.jpg" :: String), "velvia" .= film], "patches" .= patchList]]]
        ccManifest film = object ["scenes" .= [object ["name" .= ("flat" :: String), "frames" .= object ["provia" .= ("provia.jpg" :: String), "classicChrome" .= film], "patches" .= patchList]]]
    forM_ [("velvia.jpg", True), ("provia.jpg", False)] $ \(film, matches) -> do
      B.writeFile (root </> "manifest.json") (encodeCanonical (manifest (film :: String)))
      (c, _) <- run tool ["e-model", "--ffmpeg", ffmpeg, "--ffprobe", ffprobe, bundlePath, root </> "manifest.json", root </> "e-model.json"]
      ensure (c == ExitSuccess) "e-model failed"
      result <- B.readFile (root </> "e-model.json") >>= either failH pure . decodeStrict
      let mean = numD (field (field (field result "eModelByFilm") "velvia") "mean")
          n = numD (field (field (field result "eModelByFilm") "velvia") "n")
      ensure (n == 4) ("e-model used " ++ show n ++ " of 4 patches")
      if matches
        then ensure (mean < 0.8) ("e-model of the model's own output is " ++ show mean)
        else ensure (mean > 2) ("e-model did not separate a wrong film frame: " ++ show mean)

    -- with a candidate file, the classicChrome frame is predicted through the
    -- PROVIA stage carrying the candidate's matrix, chroma and tone record
    candidate <- either failH pure (candidateJson (Candidate "synthetic" [] 0 Nothing (filmMatrix provia) (filmChroma other) (filmTone provia)))
    B.writeFile (root </> "candidate.json") (encodeCanonical candidate)
    writeJpeg ffmpeg (root </> "cc.jpg") (map (develop (compileStage provia {filmChroma = filmChroma other})) patchInputs)
    forM_ [("cc.jpg", True), ("provia.jpg", False)] $ \(film, matches) -> do
      B.writeFile (root </> "manifest.json") (encodeCanonical (ccManifest (film :: String)))
      (c, _) <- run tool ["e-model", "--ffmpeg", ffmpeg, "--ffprobe", ffprobe, bundlePath, root </> "manifest.json", root </> "e-model.json", root </> "candidate.json"]
      ensure (c == ExitSuccess) "e-model with a candidate failed"
      result <- B.readFile (root </> "e-model.json") >>= either failH pure . decodeStrict
      let mean = numD (field (field (field result "eModelByFilm") "classicChrome") "mean")
          regions = field (field (field (field (field result "scenes") "flat") "films") "classicChrome") "regions"
      ensure (case regions of Array xs -> length xs == 4; _ -> False) "e-model did not list the four classicChrome regions"
      if matches
        then ensure (mean < 0.8) ("e-model of the candidate's own output is " ++ show mean)
        else ensure (mean > 2) ("e-model did not separate a PROVIA frame offered as Classic Chrome: " ++ show mean)

    -- relative: a frame with halved chroma reads as a chroma ratio near one
    -- half; a frame shifted by one patch matches only with its offset
    let outputs = map (develop (compileStage provia)) patchInputs
        halved = [YCbCr y (128 + (cb - 128) `div` 2) (128 + (cr - 128) `div` 2) | YCbCr y cb cr <- outputs]
        shifted = last outputs : init outputs
    writePacked (root </> "provia.yuv") outputs
    writePacked (root </> "halved.yuv") halved
    writePacked (root </> "shifted.yuv") shifted
    let pair group cc offset = object (["group" .= (group :: String), "provia" .= ("provia.yuv" :: String), "classicChrome" .= (cc :: String), "packed" .= object ["width" .= (256 :: Int), "height" .= (64 :: Int)], "blocks" .= object ["size" .= (16 :: Int), "maxLumaSd" .= (1 :: Double)]] ++ ["offset" .= [o, 0 :: Int] | Just o <- [offset]])
    B.writeFile (root </> "pairs.json") (encodeCanonical (object ["pairs" .= [pair "halved" "halved.yuv" Nothing, pair "shifted" "shifted.yuv" (Just 64), pair "unaligned" "shifted.yuv" Nothing]]))
    (c4, _) <- run tool ["relative", root </> "pairs.json", root </> "relative.json"]
    ensure (c4 == ExitSuccess) "relative failed"
    rel <- B.readFile (root </> "relative.json") >>= either failH pure . decodeStrict
    let source g = field (field rel "sources") g
        ratio g = numD (field (field (source g) "chromatic") "chromaRatioMean")
        blocks g = numD (field (source g) "blocks")
        sectorRatios g = case field (source g) "byHueSector" of
          Array xs -> [numD (field (field s "stats") "chromaRatioMean") | s <- foldr (:) [] xs]
          _ -> []
    ensure (blocks "halved" == 64) ("relative kept " ++ show (blocks "halved") ++ " of 64 blocks")
    ensure (ratio "halved" > 0.35 && ratio "halved" < 0.65) ("halved chroma read as ratio " ++ show (ratio "halved"))
    ensure (blocks "shifted" == 48) ("the offset pair kept " ++ show (blocks "shifted") ++ " of 48 blocks")
    ensure (all (\r -> abs (r - 1) < 0.001) (sectorRatios "shifted")) "the offset pair changed chroma in a hue sector"
    ensure (any (\r -> abs (r - 1) > 0.05) (sectorRatios "unaligned")) "an unaligned pair read as identical"
  putStrLn "photo-session tests passed"
 where
  fromStr = K.fromString
  head' (x : _) = x
  head' [] = Null
  field (Object o) k = maybe Null id (KM.lookup k o)
  field _ _ = Null
  num v k = case field v k of
    Number x -> round x :: Int
    _ -> -1
  numD (Number x) = realToFrac x :: Double
  numD _ = -1

run :: FilePath -> [String] -> IO (ExitCode, B.ByteString)
run tool args = do
  (timedOut, code, out, err) <- runBounded 300000000 (16 * 1024 * 1024) tool args B.empty
  unless (not timedOut) (failH ("timed out: " ++ unwords args))
  unless (code == ExitSuccess || take 1 args == ["photo-identical"]) (failH (unwords args ++ ": " ++ BC.unpack (B.take 400 err)))
  pure (code, out)

-- | A 256 x 64 JPEG of four flat patches, written from 4:4:4 planes at the
-- highest quality.
writeJpeg :: FilePath -> FilePath -> [YCbCr] -> IO ()
writeJpeg ffmpeg path patches = do
  let row f = B.concat [B.replicate 64 (fromIntegral (f p)) | p <- patches]
      plane f = B.concat (replicate 64 (row f))
      raw = B.concat [plane ycY, plane ycCb, plane ycCr]
  (timedOut, code, _, err) <- runBounded 120000000 65536 ffmpeg ["-v", "error", "-y", "-f", "rawvideo", "-pix_fmt", "yuvj444p", "-s", "256x64", "-i", "-", "-frames:v", "1", "-q:v", "1", "-pix_fmt", "yuvj444p", path] raw
  ensure (not timedOut && code == ExitSuccess) ("ffmpeg could not write " ++ path ++ ": " ++ BC.unpack (B.take 400 err))

-- | A 256 x 64 packed Y0 Y1 Cb Cr plane of four flat 64 x 64 patches.
writePacked :: FilePath -> [YCbCr] -> IO ()
writePacked path patches = B.writeFile path (B.concat (replicate 64 row))
 where
  row = B.concat [B.concat (replicate 32 (B.pack (map fromIntegral [y, y, cb, cr]))) | YCbCr y cb cr <- patches]
