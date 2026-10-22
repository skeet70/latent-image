{-# LANGUAGE OverloadedStrings #-}
-- The photo-session commands of //host/firmware:classic-chrome-model through
-- its command line, on flat-patch JPEGs written here by the pinned ffmpeg from
-- outputs of the synthetic fixture stages.
module Main (main) where

import Control.Monad (forM_, unless)
import Data.Aeson (Value (..), object, (.=))
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
