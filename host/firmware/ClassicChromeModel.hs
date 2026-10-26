{-# LANGUAGE OverloadedStrings #-}
-- Host colour model for Classic Chrome on the X-M1 (spec/10 section 19.5.4):
-- oracle inputs and comparison, the private parameter bundle, the
-- leave-one-out method error and the Classic Chrome fit.
module Main (main) where

import Control.Monad (forM, forM_, unless, when)
import Data.Aeson (Value (..), object, (.=))
import qualified Data.Aeson as A
import qualified Data.Aeson.Key as K
import qualified Data.Aeson.KeyMap as KM
import qualified Data.Aeson.Types as AT
import Data.Bits (shiftR, (.&.))
import qualified Data.ByteString as B
import qualified Data.ByteString.Builder as BB
import qualified Data.ByteString.Lazy as BL
import Data.List (sortOn)
import qualified Data.Text as T
import qualified Data.Text.Encoding as TE
import Numeric (readHex)
import System.Environment (getArgs)
import System.Exit (exitFailure)
import System.FilePath ((</>))
import System.IO (hPutStrLn, stderr)

import FujiAF.ClassicChromeCandidate (Candidate (..), Input (..), candidateJson, parseCandidate)
import FujiAF.Colour
import FujiAF.FilmFit
import FujiAF.FilmGrid (oracleGrid)
import FujiAF.FilmInvert (invert)
import FujiAF.FilmStage
import qualified FujiAF.Host as H
import FujiAF.JSON (decodeStrict, encodeCanonical)
import FujiAF.XM1Develop
import LookReference (LabPair (..), lutLab, parseCube, relativeStats, sector)
import PhotoSession (Decoder (..), PairGroup (..), Ycc (..), captureDifference, chartPatches, eModel, pairDeltaStats, pairDeltaTables, photoDifference, photoIdentical, relativePairs, yccLab)
import SiblingTables (runTables)

usage :: String
usage =
  unlines
    [ "classic-chrome-model tables FFLZD XM1-150.DAT XT1-552.DAT X70-SECTION4.bin X70-RECIPES.json OUT.json"
    , "  sibling tables from the pinned DATs (decoded with //lz:fflzd), X70 section 4 and x70view's"
    , "  recipes-srgb.json, with the spec/10 section 19.5.1 comparisons printed"
    , "classic-chrome-model oracle-input OUTDIR NAME=PARAMS.json ..."
    , "  writes OUTDIR/grid.u16 (u16 LE R,G,B per point) and OUTDIR/NAME.words.json"
    , "classic-chrome-model oracle-compare PARAMS.json ORACLE.ycc"
    , "  compares the model with film_stage.develop output (u8 Y,Cb,Cr per grid point)"
    , "classic-chrome-model bundle TABLES.json RECIPES.json SESSION.json STOCK-REGISTERS.json X70-SECTION4.bin XM1-ID4.bin XM1-ID5.bin XM1-PAYLOAD.bin OUT.json"
    , "  film parameter sets for X-M1 1.50 and X-Trans II from the sibling tables, x70view's"
    , "  recipes-srgb.json, a PROVIA DR100 session log and stock-film-registers.json, X70 section 4,"
    , "  and the X-M1 decoded images 4 and 5 and complemented payload"
    , "classic-chrome-model method BUNDLE.json OUT.json"
    , "  leave-one-out e_method over Velvia, ASTIA, PRO Neg Std and PRO Neg Hi for each fit variant,"
    , "  and the selected relative variant"
    , "classic-chrome-model fit-cc BUNDLE.json METHOD.json CANDIDATE.json REPORT.json"
    , "  the X-M1 Classic Chrome matrix and chroma by the variant METHOD.json selected, as the"
    , "  latent-image.classic-chrome-parameters.v1 file and a residual report"
    , "classic-chrome-model photo-identical [--ffmpeg PATH] [--ffprobe PATH] A.JPG B.JPG"
    , "  exit 0 when the two JPEGs decode to identical Y, Cb and Cr planes"
    , "classic-chrome-model photo-difference [--ffmpeg PATH] [--ffprobe PATH] A.JPG B.JPG OUT.json"
    , "  native-plane signed B-minus-A histograms, changed counts, mean absolute and RMS error"
    , "classic-chrome-model capture-difference [--ffmpeg PATH] [--ffprobe PATH] BUNDLE.json MANIFEST.json OUT.json"
    , "  flat-block input-gain counterfactuals for a PROVIA shot and its RAW conversion"
    , "classic-chrome-model chart-patches X,Y X,Y X,Y X,Y SIZE"
    , "  ColorChecker Classic patch squares from the centres of dark skin, bluish green,"
    , "  white 9.5 and black 2 (pixels), for an e-model manifest"
    , "classic-chrome-model e-model [--ffmpeg PATH] [--ffprobe PATH] BUNDLE.json MANIFEST.json OUT.json [CANDIDATE.json]"
    , "  e_model: each scene's Velvia, ASTIA and PRO Neg frames predicted from its PROVIA frame"
    , "  through the X-M1 stages; with CANDIDATE.json also its classicChrome frame through the"
    , "  X-M1 PROVIA stage with the candidate's matrix, chroma and tone record"
    , "classic-chrome-model settings BUNDLE.json CANDIDATE.json XM1-ID4.bin XM1-ID5.bin XM1-PAYLOAD.bin X70VIEW-FRONTEND.md OUT.json"
    , "  expected Color, Highlight, Shadow and DR responses of the X-M1 PROVIA stage, the Classic"
    , "  Chrome candidate and the X-Trans II Classic Chrome reference (spec/10 19.5.6 item 6)"
    , "classic-chrome-model look-reference FILM PROVIA.cube FILM.cube BUNDLE.json CANDIDATE.json E-MODEL.json OUT.json"
    , "  FILM (velvia, astia, proNegStd, proNegHi or classicChrome) against PROVIA by PROVIA hue,"
    , "  chroma and lightness: Fujifilm's F-Log2 LUTs on the neutral ramp, a low-chroma grid and the"
    , "  gamut set, the X-M1 stage (the candidate for classicChrome) on the same set, and FILM's"
    , "  regions of an e-model output"
    , "classic-chrome-model relative [--ffmpeg PATH] [--ffprobe PATH] MANIFEST.json OUT.json [BUNDLE.json CANDIDATE.json]"
    , "  Classic Chrome against PROVIA on same-scene frame pairs (flat blocks, binned by the PROVIA"
    , "  colour), and with the bundle and candidate the same statistics over the patch set for the"
    , "  X-Trans II stages and the X-M1 PROVIA stage against the candidate"
    ]

main :: IO ()
main = do
  args <- getArgs
  let (dec, positional) = decoderFlags (Decoder "ffmpeg" "ffprobe") args
  case positional of
    ["tables", fflzd, xm1Dat, xt1Dat, s4, recipes, out] -> runTables fflzd xm1Dat xt1Dat s4 recipes out
    ("oracle-input" : out : specs) | not (null specs) -> oracleInput out specs
    ["oracle-compare", params, ycc] -> oracleCompare params ycc
    ["bundle", tables, recipes, session, stockRegs, s4, id4, id5, payload, out] -> buildBundle (BundleInputs tables recipes session stockRegs s4 id4 id5 payload) out
    ["method", bundle, out] -> runMethod bundle out
    ["fit-cc", bundle, method, candidate, out] -> runFitCc bundle method candidate out
    ["photo-identical", a, b] -> photoIdentical dec a b >>= \same -> unless same exitFailure
    ["photo-difference", a, b, out] -> photoDifference dec a b >>= writeJson out
    ["capture-difference", bundle, manifest, out] -> do
      b <- readBundle bundle
      captureDifference dec (xm1 b Provia) manifest >>= writeJson out
    ["chart-patches", tl, tr, bl, br, size] | Just corners <- mapM point [tl, tr, bl, br], [(n, "")] <- reads size -> case corners of
      [p0, p1, p2, p3] -> B.putStr (encodeCanonical (chartPatches p0 p1 p2 p3 n) <> "\n")
      _ -> hPutStrLn stderr usage >> exitFailure
    ["settings", bundle, candidate, id4, id5, payload, frontend, out] -> runSettings bundle candidate id4 id5 payload frontend out
    ("e-model" : bundle : manifest : out : rest) | length rest <= 1 -> do
      b <- readBundle bundle
      cc <- forM rest $ \p -> (\c -> ("classicChrome", xm1Candidate b c)) <$> readCandidate p
      eModel dec ([(filmKey f, xm1 b f) | f <- [Provia .. ProNegHi]] ++ cc) manifest >>= writeJson out
    ["look-reference", film, proviaCube, filmCube, bundle, candidate, eModelOut, out] -> runLookReference (T.pack film) proviaCube filmCube bundle candidate eModelOut out
    ("relative" : manifest : out : model) | length model `elem` [0, 2] -> runRelative dec manifest out model
    _ -> hPutStrLn stderr usage >> exitFailure
 where
  point s = case break (== ',') s of
    (x, ',' : y) | [(px, "")] <- reads x, [(py, "")] <- reads y -> Just (px, py)
    _ -> Nothing

decoderFlags :: Decoder -> [String] -> (Decoder, [String])
decoderFlags d ("--ffmpeg" : p : rest) = decoderFlags d {decoderFfmpeg = p} rest
decoderFlags d ("--ffprobe" : p : rest) = decoderFlags d {decoderFfprobe = p} rest
decoderFlags d (x : rest) = let (d', xs) = decoderFlags d rest in (d', x : xs)
decoderFlags d [] = (d, [])

readJson :: FilePath -> IO Value
readJson file = H.readRegular (256 * 1024 * 1024) file >>= either (H.failH . ((file ++ ": ") ++)) pure . decodeStrict

writeJson :: FilePath -> Value -> IO ()
writeJson file v = B.writeFile file (encodeCanonical v <> "\n")

readParams :: FilePath -> IO FilmParams
readParams file = readJson file >>= either (H.failH . ((file ++ ": ") ++)) pure . parseFilmParams

-- Oracle -------------------------------------------------------------------

oracleInput :: FilePath -> [String] -> IO ()
oracleInput out specs = do
  BL.writeFile (out </> "grid.u16") (BB.toLazyByteString (mconcat [BB.word16LE (fromIntegral c) | Rgb14 r g b <- oracleGrid, c <- [r, g, b]]))
  forM_ specs $ \spec -> case break (== '=') spec of
    (name, '=' : file) -> do
      p <- readParams file
      writeJson (out </> (name ++ ".words.json")) (object [K.fromString (show o) .= w | (o, w) <- registerWords p])
    _ -> H.failH ("expected NAME=PARAMS.json, got " ++ spec)

oracleCompare :: FilePath -> FilePath -> IO ()
oracleCompare paramsPath yccPath = do
  st <- compileStage <$> readParams paramsPath
  bytes <- B.readFile yccPath
  let n = length oracleGrid
  H.ensure (B.length bytes == 3 * n) ("oracle output must hold " ++ show n ++ " points")
  let oracle i = YCbCr (at (3 * i)) (at (3 * i + 1)) (at (3 * i + 2))
      at = fromIntegral . B.index bytes
      misses = [(i, w, develop st w, oracle i) | (i, w) <- zip [0 ..] oracleGrid, develop st w /= oracle i]
  putStrLn (show (n - length misses) ++ " / " ++ show n ++ " grid points equal")
  unless (null misses) $ do
    mapM_ (\(i, w, m, o) -> putStrLn ("point " ++ show (i :: Int) ++ " " ++ show w ++ ": model " ++ show m ++ ", oracle " ++ show o)) (take 10 misses)
    exitFailure

-- Bundle -------------------------------------------------------------------

data Film = Provia | Velvia | Astia | ProNegStd | ProNegHi | ClassicChrome deriving (Eq, Show, Enum, Bounded)

filmKey :: Film -> T.Text
filmKey f = case f of
  Provia -> "provia"
  Velvia -> "velvia"
  Astia -> "astia"
  ProNegStd -> "proNegStd"
  ProNegHi -> "proNegHi"
  ClassicChrome -> "classicChrome"

-- | DR100 selection shared by both bodies: tone key, matrix id, chroma id and
-- X70 luma-band preset. Classic Chrome's chroma is the X-T1 entry at 0xce9a.
data Selection = Selection {selTone :: Int, selMatrix :: Int, selChroma :: Int, selPreset :: Int}

selection :: Film -> Selection
selection f = case f of
  Provia -> Selection 1 1 1 1
  Velvia -> Selection 7 2 2 2
  Astia -> Selection 12 3 3 1
  ProNegStd -> Selection 17 4 4 3
  ProNegHi -> Selection 22 5 5 3
  ClassicChrome -> Selection 38 9 0 1

-- | x70view recipes-srgb.json fsim values for the preset lookup.
fsimOf :: Film -> Integer
fsimOf f = case f of
  Provia -> 0
  Velvia -> 3
  Astia -> 1
  ProNegStd -> 11
  ProNegHi -> 12
  ClassicChrome -> 13

path :: Value -> [T.Text] -> Either String Value
path v [] = Right v
path (Object o) (k : ks) = maybe (Left ("missing " ++ T.unpack k)) (`path` ks) (KM.lookup (K.fromText k) o)
path _ (k : _) = Left ("not an object at " ++ T.unpack k)

ints :: Value -> Either String [Int]
ints = AT.parseEither A.parseJSON

toneOf :: Value -> T.Text -> Int -> Either String ToneRecord
toneOf tables body key = do
  records <- path tables ["tone", body, "records"] >>= AT.parseEither A.parseJSON :: Either String [Value]
  case [r | r <- records, path r ["key"] == Right (Number (fromIntegral key))] of
    [r] -> do
      es <- path r ["exponents"] >>= ints
      ps <- path r ["points"] >>= ints
      toneRecord es ps
    _ -> Left ("tone key " ++ show key ++ " not unique in " ++ T.unpack body)

matrixOf :: Value -> T.Text -> Int -> Either String ColourMatrix
matrixOf tables body i = do
  xs <- path tables ["matrices", body, "ids", T.pack (show i)] >>= ints
  case xs of
    [a, b, c, d, e, f, g, h, k] -> colourMatrix [[a, b, c], [d, e, f], [g, h, k]]
    _ -> Left "matrix entry needs nine values"

-- | tables.json chroma values are [m00, m01, m10, m11] of
-- [Cr <- R-Y, Cr <- B-Y; Cb <- R-Y, Cb <- B-Y].
chromaFrom :: [Int] -> Either String Chroma
chromaFrom [crR, crB, cbR, cbB] = chroma crR crB cbR cbB
chromaFrom _ = Left "chroma entry needs four values"

chromaOf :: Value -> T.Text -> Film -> Either String Chroma
chromaOf tables body film
  | film == ClassicChrome = path tables ["xt1_classic_chrome_chroma", "values"] >>= ints >>= chromaFrom
  | otherwise = path tables ["chroma", body, "ids", T.pack (show (selChroma (selection film)))] >>= ints >>= chromaFrom

presetOf :: Value -> Film -> Either String LumaBands
presetOf recipes film = do
  sims <- path recipes ["film_sims"] >>= AT.parseEither A.parseJSON :: Either String [Value]
  sim <- case [s | s <- sims, path s ["fsim"] == Right (Number (fromIntegral (fsimOf film)))] of
    [s] -> Right s
    _ -> Left "fsim not unique in recipes"
  sel <- path sim ["luma_band_preset", "selector"] >>= AT.parseEither A.parseJSON
  when (sel /= selPreset (selection film)) (Left ("unexpected luma-band preset for " ++ show film))
  bounds <- path sim ["luma_band_preset", "bytes"] >>= ints
  u16 <- path sim ["luma_band_preset", "u16"] >>= ints
  let sext v = let x = v .&. 0x3ff in if x >= 0x200 then x - 0x400 else x
  case splitAt 5 u16 of
    (shifts, gains) | length gains == 5 -> lumaBands bounds (zipWith LumaBand (map sext shifts) gains)
    _ -> Left "luma-band preset needs ten values"

-- | Gain table of the logged PROVIA DR100 RAW conversion: still words
-- FFF44108..FFF44130, two 13-bit fields each, low half first.
gainTableOf :: Value -> Either String GainTable
gainTableOf session = do
  engine <- path session ["raw_convert", "preview_engine"]
  ws <- forM [0xfff44108, 0xfff4410c .. 0xfff44130 :: Int] $ \a -> do
    v <- path engine [T.pack ("0x" ++ showHexLower a)]
    s <- AT.parseEither A.parseJSON v
    case readHex (drop 2 s) of
      [(w, "")] -> Right (w :: Int)
      _ -> Left "register word not hex"
  gainTable (concat [[w .&. 0x1fff, (w `shiftR` 16) .&. 0x1fff] | w <- ws])
 where
  showHexLower n = let go 0 acc = acc; go k acc = go (k `div` 16) ("0123456789abcdef" !! (k `mod` 16) : acc) in go n ""

-- | Record key of the gain table in x70view's stock-film-registers.json live
-- words 0x304..0x32c for an fsim value.
liveGainKey :: [(Int, GainTable)] -> Value -> Film -> Either String Int
liveGainKey records stock film = do
  ws <- forM [0x304, 0x308 .. 0x32c :: Int] $ \o -> do
    s <- path stock ["films", T.pack (show (fsimOf film)), T.pack ("0x" ++ hexLower o)] >>= AT.parseEither A.parseJSON
    case readHex (drop 2 s) of
      [(w, "")] -> Right (w :: Int)
      _ -> Left "register word not hex"
  table <- gainTable (concat [[w .&. 0x1fff, (w `shiftR` 16) .&. 0x1fff] | w <- ws])
  case [k | (k, t) <- records, t == table] of
    [k] -> Right k
    ks -> Left ("live gain table of " ++ show film ++ " matches records " ++ show ks)
 where
  hexLower n = let go 0 acc = acc; go k acc = go (k `div` 16) ("0123456789abcdef" !! (k `mod` 16) : acc) in go n ""

-- | X-M1 develop code of FUN_00d78418 for each film the X-M1 has.
developCode :: Film -> Maybe DevelopCode
developCode f = case f of
  Provia -> Just (DevelopCode 0)
  Velvia -> Just (DevelopCode 4)
  Astia -> Just (DevelopCode 5)
  ProNegStd -> Just (DevelopCode 6)
  ProNegHi -> Just (DevelopCode 7)
  ClassicChrome -> Nothing

data BundleInputs = BundleInputs
  { inTables, inRecipes, inSession, inStockRegisters, inX70Section4, inXm1Id4, inXm1Id5, inXm1Payload :: FilePath
  }

-- | X-Trans II sets: X-T1 matrices and chroma, X70 tone records, presets and
-- gain records (PROVIA and Classic Chrome key 1, the logged PROVIA DR100
-- table; the other films key 101, from the +100 offset their live tables
-- show against PROVIA's). X-M1 sets: X-M1 matrices and chroma (checked
-- against the fallback image's raw fields), tone records, and the gain
-- record, luma-band preset and chroma gains its develop stages select.
buildBundle :: BundleInputs -> FilePath -> IO ()
buildBundle inp out = do
  tables <- readJson (inTables inp)
  recipes <- readJson (inRecipes inp)
  session <- readJson (inSession inp)
  stock <- readJson (inStockRegisters inp)
  x70Records <- B.readFile (inX70Section4 inp) >>= either H.failH pure . x70GainRecords
  im <- loadImages (inXm1Id4 inp) (inXm1Id5 inp) (inXm1Payload inp) >>= either H.failH pure
  xm1Records <- either H.failH pure (gainRecords im)
  logged <- either H.failH pure (gainTableOf session)
  let record rs k = maybe (Left ("no gain record " ++ show k)) Right (lookup k rs)
  x70Provia <- either H.failH pure (record x70Records 1)
  H.ensure (x70Provia == logged) "X70 gain record 1 differs from the logged PROVIA DR100 table"
  liveKeys <- either H.failH pure (mapM (\f -> (f,) <$> liveGainKey x70Records stock f) [minBound .. maxBound])
  let liveProvia = maybe 0 id (lookup Provia liveKeys)
  forM_ liveKeys $ \(f, k) ->
    H.ensure (k - liveProvia == (if f `elem` [Provia, ClassicChrome] then 0 else 100)) ("live gain key of " ++ show f ++ " is not PROVIA's family offset")
  let xt2Gain f = record x70Records (if f `elem` [Provia, ClassicChrome] then 1 else 101)
      xt2 f = do
        g <- xt2Gain f
        m <- matrixOf tables "xt1_552" (selMatrix (selection f))
        t <- toneOf tables "x70_110" (selTone (selection f))
        c <- chromaOf tables "xt1_552" f
        bands <- presetOf recipes f
        pure (FilmParams g m t c bands)
      xm1Set f code = do
        key <- gainTableKey im code
        g <- record xm1Records key
        (presetId, bands) <- lumaPreset im code
        gains <- chromaGainProduct im code
        when (gains /= 2 ^ (34 :: Int)) (Left ("X-M1 chroma gains for " ++ show f ++ " are not unity: " ++ show gains))
        let sel = selection f
        m <- matrixOf tables "xm1_150" (selMatrix sel)
        c <- chromaOf tables "xm1_150" f
        rawM <- matrixFields im (selMatrix sel)
        rawC <- chromaFields im (selChroma sel)
        encM <- matrixStorage m
        encC <- chromaStorage c
        when (concat encM /= rawM) (Left ("X-M1 matrix " ++ show (selMatrix sel) ++ " does not re-encode to its adjustment fields"))
        when (encC /= rawC) (Left ("X-M1 chroma " ++ show (selChroma sel) ++ " does not re-encode to its adjustment fields in storage order"))
        when (chromaFromStorage rawC /= Right c) (Left "chroma storage order disagrees with the stage's reading order")
        t <- toneOf tables "xm1_150" (selTone sel)
        pure (FilmParams g m t c bands, object ["gainRecordKey" .= key, "lumaBandPreset" .= presetId, "chromaGainProduct" .= gains])
  xt2Sets <- either H.failH pure (mapM (\f -> (f,) <$> xt2 f) [minBound .. maxBound])
  xm1Sets <- either H.failH pure (sequence [(f,) <$> xm1Set f code | f <- [minBound .. maxBound], Just code <- [developCode f]])
  inputs <- forM [("tables", inTables inp), ("recipes", inRecipes inp), ("session", inSession inp), ("stockRegisters", inStockRegisters inp), ("x70Section4", inX70Section4 inp), ("xm1Id4", inXm1Id4 inp), ("xm1Id5", inXm1Id5 inp), ("xm1Payload", inXm1Payload inp)] $ \(k, p) -> (k,) <$> (B.readFile p >>= H.sha256)
  writeJson out $
    object
      [ "inputsSha256" .= object [K.fromText k .= T.pack h | (k, h) <- inputs]
      , "x70LiveGainKeys" .= object [K.fromText (filmKey f) .= k | (f, k) <- liveKeys]
      , "xm1Develop" .= object [K.fromText (filmKey f) .= info | (f, (_, info)) <- xm1Sets]
      , "films"
          .= object
            [ K.fromText (filmKey f) .= object (("xtrans2" .= filmParamsJson xt) : ["xm1" .= filmParamsJson p | Just (p, _) <- [lookup f xm1Sets]])
            | (f, xt) <- xt2Sets
            ]
      ]

-- Method -------------------------------------------------------------------

data Bundle = Bundle {xtrans2 :: Film -> FilmParams, xm1 :: Film -> FilmParams}

readBundle :: FilePath -> IO Bundle
readBundle p = do
  v <- readJson p
  let get body film = either error id (path v ["films", filmKey film, body] >>= parseFilmParams)
  either H.failH pure (mapM_ (\f -> path v ["films", filmKey f, "xtrans2"] >>= parseFilmParams) [minBound .. maxBound :: Film])
  either H.failH pure (mapM_ (\f -> path v ["films", filmKey f, "xm1"] >>= parseFilmParams) [Provia .. ProNegHi])
  pure (Bundle (get "xtrans2") (get "xm1"))

-- | Gain table per film: the bundle's selected records, or those scaled per
-- film so an 18 % grey renders at PROVIA's luma.
data GainMode = SelectedTables | GreyMatched deriving (Eq)

data Scenario = Scenario {scenarioName :: T.Text, scenarioXm1 :: CameraModel, scenarioXtrans2 :: CameraModel, scenarioGain :: GainMode}

scenarios :: [Scenario]
scenarios =
  [ Scenario "xm1-vs-x70" xm1Camera x70Camera SelectedTables
  , Scenario "xm1-vs-xt1" xm1Camera xt1Camera SelectedTables
  , Scenario "xm1-vs-x70-grey-matched" xm1Camera x70Camera GreyMatched
  ]

scaleGain :: Double -> FilmParams -> FilmParams
scaleGain s p = p {filmGainTable = either error id (gainTable [min 8191 (round (s * fromIntegral g)) | g <- gainTableQ9 (filmGainTable p)])}

-- | The bundle as a scenario sees it, and the scenario's exposure.
scenarioBundle :: Scenario -> Bundle -> (Bundle, Exposure)
scenarioBundle sc b = case scenarioGain sc of
  SelectedTables -> (b, k)
  GreyMatched -> (Bundle (\f -> scaleGain (factor f) (xtrans2 b f)) (\f -> scaleGain (factor f) (xm1 b f)), k)
 where
  k@(Exposure e) = calibrateExposure (compileStage (xtrans2 b Provia))
  grey = let v = max 0 (min 16383 (round (0.18 * e))) in Rgb14 v v v
  lumaOf p = ycY (develop (compileStage p) grey)
  target = lumaOf (xtrans2 b Provia)
  factor f = bisect (\s -> lumaOf (scaleGain s (xtrans2 b f)) >= target) 0.125 2 (30 :: Int)
  bisect ok lo hi n
    | n == 0 = hi
    | ok mid = bisect ok lo mid (n - 1)
    | otherwise = bisect ok mid hi (n - 1)
   where
    mid = (lo + hi) / 2

patchSets :: [PatchSet]
patchSets = [colorChecker24, neutralRamp, gamutSet]

allPatches :: [Patch]
allPatches = concatMap setPatches patchSets

settingFor :: FilmParams -> ColourSetting
settingFor p = ColourSetting (filmMatrix p) (filmChroma p)

r3 :: Double -> Double
r3 x = fromIntegral (round (x * 1000) :: Integer) / 1000

statsJson :: [Double] -> Value
statsJson ds = let Stats m p x = stats ds in object ["mean" .= r3 m, "p95" .= r3 p, "max" .= r3 x, "n" .= length ds]

-- | Stats over all patches and per patch set.
breakdown :: [Double] -> Value
breakdown ds = object (("all" .= statsJson ds) : [K.fromString (setName s) .= statsJson (take (length (setPatches s)) (drop off ds)) | (s, off) <- zip patchSets offsets])
 where
  offsets = scanl (+) 0 (map (length . setPatches) patchSets)

settingJson :: ColourSetting -> Value
settingJson (ColourSetting m (Chroma a b c d)) =
  object ["matrixQ10" .= matrixRows m, "chromaQ10" .= object ["crFromRY" .= a, "crFromBY" .= b, "cbFromRY" .= c, "cbFromBY" .= d]]

data Context = Context {ctxBundle :: Bundle, ctxInputsXm1 :: [Rgb14], ctxInputsXtrans2 :: [Rgb14], ctxExposure :: Exposure}

context :: Bundle -> Scenario -> Context
context b0 sc = Context b (map (sceneInput (scenarioXm1 sc) k . patchXyz) allPatches) (map (sceneInput (scenarioXtrans2 sc) k . patchXyz) allPatches) k
 where
  (b, k) = scenarioBundle sc b0

renders :: Stage -> [Rgb14] -> [Lab]
renders st = map (renderLab st)

-- | The X-M1 stage for a film with the X-M1's tone record and PROVIA's colour
-- where the film's own X-M1 colour is not used (Classic Chrome).
xm1Template :: Bundle -> Film -> FilmParams
xm1Template b film = case film of
  ClassicChrome -> (xm1 b Provia) {filmTone = filmTone (xtrans2 b ClassicChrome), filmBands = filmBands (xtrans2 b ClassicChrome)}
  _ -> xm1 b film

-- | The absolute reference (the X-Trans II stage's rendering of the film) and
-- the relative target (the X-M1's PROVIA plus the X-Trans II film's L*a*b*
-- difference from X-Trans II PROVIA).
data Targets = Targets {reference :: [Lab], relativeTarget :: [Lab]}

targets :: Context -> Film -> Targets
targets ctx film = Targets ref (zipWith3 shift xmProvia ref xtProvia)
 where
  b = ctxBundle ctx
  ref = renders (compileStage (xtrans2 b film)) (ctxInputsXtrans2 ctx)
  xtProvia = renders (compileStage (xtrans2 b Provia)) (ctxInputsXtrans2 ctx)
  xmProvia = renders (compileStage (xm1 b Provia)) (ctxInputsXm1 ctx)
  shift (Lab l0 a0 b0) (Lab l1 a1 b1) (Lab l2 a2 b2) = Lab (l0 + l1 - l2) (a0 + a1 - a2) (b0 + b1 - b2)

-- | Where the X-M1's PROVIA difference enters the 2x2 (Cr, Cb) = C (R-Y, B-Y).
data Side = OutputSide | InputSide deriving (Eq)

-- | Relative chroma construction rounded to Q10: OutputSide gives
-- C_xm1Provia C_xt2Provia^-1 C_xt2Film, InputSide C_xt2Film C_xt2Provia^-1 C_xm1Provia.
constructChroma :: Side -> Bundle -> Film -> Either String Chroma
constructChroma side b film = chroma (q10 a') (q10 b') (q10 c') (q10 d')
 where
  unit (Chroma a bb c d) = ((fromIntegral a / 1024, fromIntegral bb / 1024), (fromIntegral c / 1024, fromIntegral d / 1024)) :: ((Double, Double), (Double, Double))
  mul ((a, bb), (c, d)) ((e, f), (g, h)) = ((a * e + bb * g, a * f + bb * h), (c * e + d * g, c * f + d * h))
  inv ((a, bb), (c, d)) = let det = a * d - bb * c in ((d / det, negate bb / det), (negate c / det, a / det))
  q10 x = round (x * 1024) :: Int
  xmP = unit (filmChroma (xm1 b Provia))
  xtP = unit (filmChroma (xtrans2 b Provia))
  xtF = unit (filmChroma (xtrans2 b film))
  ((a', b'), (c', d')) = case side of
    OutputSide -> xmP `mul` inv xtP `mul` xtF
    InputSide -> xtF `mul` inv xtP `mul` xmP

-- | A way of choosing the X-M1 matrix and chroma for a film.
data Variant
  = Absolute
  | RelativeFree
  | RelativeFixedChroma Side
  | RelativeRidge Side Double

variantName :: Variant -> T.Text
variantName v = case v of
  Absolute -> "absolute"
  RelativeFree -> "relative-free"
  RelativeFixedChroma s -> "relative-fixed-chroma-" <> sideName s
  RelativeRidge s w -> "relative-ridge-" <> sideName s <> "-w" <> T.pack (show w)
 where
  sideName OutputSide = "output"
  sideName InputSide = "input"

variants :: [Variant]
variants =
  [Absolute, RelativeFree]
    ++ [RelativeFixedChroma s | s <- [OutputSide, InputSide]]
    ++ [RelativeRidge s w | s <- [OutputSide, InputSide], w <- [0.01, 0.1, 1, 10]]

-- | The variants the selection chooses from: relative, with chroma tied to its construction.
selectable :: Variant -> Bool
selectable v = case v of
  RelativeFixedChroma _ -> True
  RelativeRidge _ _ -> True
  _ -> False

variantSide :: Variant -> Maybe Side
variantSide v = case v of
  RelativeFixedChroma s -> Just s
  RelativeRidge s _ -> Just s
  _ -> Nothing

-- | Starts: the X-Trans II film's setting, the X-M1's PROVIA setting, and the
-- identity matrix with the constructed chroma (or X-Trans II film chroma).
variantStarts :: Bundle -> Film -> Variant -> [ColourSetting]
variantStarts b film v =
  [ settingFor (xtrans2 b film)
  , settingFor (xm1 b Provia)
  , ColourSetting (either error id (colourMatrix [[1024, 0, 0], [0, 1024, 0], [0, 0, 1024]])) startChroma
  ]
 where
  startChroma = maybe (filmChroma (xtrans2 b film)) (\s -> either error id (constructChroma s b film)) (variantSide v)

fitVariantFrom :: Context -> Film -> Variant -> [ColourSetting] -> FitResult
fitVariantFrom ctx film v = fitIn space stage (ctxInputsXm1 ctx) goal
 where
  b = ctxBundle ctx
  stage = compileStage (xm1Template b film)
  t = targets ctx film
  constructed s = either error id (constructChroma s b film)
  (space, goal) = case v of
    Absolute -> (fullSpace, reference t)
    RelativeFree -> (fullSpace, relativeTarget t)
    RelativeFixedChroma s -> (matrixSpace (constructed s), relativeTarget t)
    RelativeRidge s w -> (ridgeSpace w (constructed s), relativeTarget t)

fitVariant :: Context -> Film -> Variant -> FitResult
fitVariant ctx film v = fitVariantFrom ctx film v (variantStarts (ctxBundle ctx) film v)

settingDistance :: ColourSetting -> ColourSetting -> Int
settingDistance (ColourSetting m1 (Chroma a b c d)) (ColourSetting m2 (Chroma e f g h)) =
  maximum (zipWith (\x y -> abs (x - y)) (concat (matrixRows m1) ++ [a, b, c, d]) (concat (matrixRows m2) ++ [e, f, g, h]))

-- | One run of a variant per start: settings, objectives, the largest Q10
-- difference between any two, CIEDE2000 between any two runs' renderings, and
-- for films the X-M1 has, each run's mean e_method.
startSpread :: Context -> Film -> Variant -> Value
startSpread ctx film v =
  object
    ( [ "runs" .= [object (["setting" .= settingJson (fitSetting r), "objective" .= r3 (fitObjective r)] ++ eMethodOf r) | r <- runs]
      , "maxQ10Difference" .= maximum (0 : [settingDistance (fitSetting x) (fitSetting y) | x <- runs, y <- runs])
      , "objectiveSpread" .= r3 (maximum (map fitObjective runs) - minimum (map fitObjective runs))
      , "renderingSpread" .= renderingSpread ctx film (map fitSetting runs)
      ]
    )
 where
  b = ctxBundle ctx
  runs = [fitVariantFrom ctx film v [s] | s <- variantStarts b film v]
  eMethodOf r
    | film == ClassicChrome = []
    | otherwise =
        let real = renders (compileStage (xm1 b film)) (ctxInputsXm1 ctx)
            fitted = renders (applySetting (fitSetting r) (compileStage (xm1 b film))) (ctxInputsXm1 ctx)
         in ["eMethodMean" .= r3 (statMean (stats (zipWith ciede2000 real fitted)))]

-- | CIEDE2000 between the X-M1 renderings of every pair of settings: the
-- largest pairwise mean and the largest single patch difference.
renderingSpread :: Context -> Film -> [ColourSetting] -> Value
renderingSpread ctx film settings = object ["maxPairMean" .= r3 (maximum (0 : map fst pairs)), "maxPatch" .= r3 (maximum (0 : map snd pairs))]
 where
  stage = compileStage (xm1Template (ctxBundle ctx) film)
  rendered = [renders (applySetting s stage) (ctxInputsXm1 ctx) | s <- settings]
  pairs = [let ds = zipWith ciede2000 x y in (statMean (stats ds), statMax (stats ds)) | (i, x) <- zip [0 :: Int ..] rendered, (j, y) <- zip [0 ..] rendered, i < j]

looFilms :: [Film]
looFilms = [Velvia, Astia, ProNegStd, ProNegHi]

primaryScenario :: T.Text
primaryScenario = "xm1-vs-x70"

runMethod :: FilePath -> FilePath -> IO ()
runMethod bundlePath out = do
  b0 <- readBundle bundlePath
  results <- forM scenarios $ \sc -> do
    let ctx = context b0 sc
        b = ctxBundle ctx
    films <- forM looFilms $ \film -> do
      let t = targets ctx film
          xm1Stage = compileStage (xm1 b film)
          real = renders xm1Stage (ctxInputsXm1 ctx)
          against setting = zipWith ciede2000 real (renders (applySetting setting xm1Stage) (ctxInputsXm1 ctx))
          vs refs st = zipWith ciede2000 refs (renders st (ctxInputsXm1 ctx))
          perVariant = [(v, fitVariant ctx film v) | v <- variants]
      forM_ perVariant $ \(v, fit) -> hPutStrLn stderr (T.unpack (scenarioName sc) ++ " " ++ show film ++ " " ++ T.unpack (variantName v) ++ " e_method " ++ show (stats (against (fitSetting fit))))
      pure
        ( film
        , [(variantName v, statMean (stats e), statP95 (stats e), statMax (stats e)) | (v, fit) <- perVariant, let e = against (fitSetting fit)]
        , object
            [ "xm1Real" .= settingJson (settingFor (xm1 b film))
            , "constructedChroma" .= object [K.fromText n .= either (const Null) chromaJson (constructChroma s b film) | (n, s) <- [("output", OutputSide), ("input", InputSide)]]
            , "baselineReuseXm1Provia" .= breakdown (against (settingFor (xm1 b Provia)))
            , "baselineCopyXtrans2" .= breakdown (against (settingFor (xtrans2 b film)))
            , "xm1RealVsXtrans2" .= breakdown (vs (reference t) xm1Stage)
            , "xm1RealVsRelativeTarget" .= breakdown (vs (relativeTarget t) xm1Stage)
            , "variants"
                .= object
                  [ K.fromText (variantName v)
                    .= object
                      [ "setting" .= settingJson (fitSetting fit)
                      , "eMethod" .= breakdown (against (fitSetting fit))
                      , "residualVsReference" .= breakdown (vs (reference t) (applySetting (fitSetting fit) xm1Stage))
                      , "residualVsRelativeTarget" .= breakdown (vs (relativeTarget t) (applySetting (fitSetting fit) xm1Stage))
                      ]
                  | (v, fit) <- perVariant
                  ]
            ]
        )
    let Exposure k = ctxExposure ctx
        summary =
          [ (name, avg [m | (_, rows, _) <- films, (n, m, _, _) <- rows, n == name], avg [p | (_, rows, _) <- films, (n, _, p, _) <- rows, n == name], maximum (0 : [x | (_, rows, _) <- films, (n, _, _, x) <- rows, n == name]))
          | name <- map variantName variants
          ]
    pure
      ( scenarioName sc
      , ctx
      , summary
      , object
          [ "exposure" .= r3 k
          , "films" .= object [K.fromText (filmKey f) .= v | (f, _, v) <- films]
          , "summary" .= object [K.fromText n .= object ["meanOfMeans" .= r3 m, "meanOfP95" .= r3 p, "maxOfMax" .= r3 x] | (n, m, p, x) <- summary]
          ]
      )
  (primaryCtx, primarySummary) <- case [(c, s) | (n, c, s, _) <- results, n == primaryScenario] of
    [r] -> pure r
    _ -> H.failH "primary scenario missing"
  let ranking = sortOn (\(_, m, p, _) -> (m, p)) [row | row@(n, _, _, _) <- primarySummary, n `elem` map variantName (filter selectable variants)]
  chosen <- case ranking of
    ((n, _, _, _) : _) -> maybe (H.failH "selected variant unknown") pure (lookup n [(variantName v, v) | v <- variants])
    [] -> H.failH "no selectable variant"
  let stability = object [K.fromText (filmKey f) .= startSpread primaryCtx f chosen | f <- looFilms]
      stabilityAll = object [K.fromText (variantName v) .= object [K.fromText (filmKey f) .= startSpread primaryCtx f v | f <- looFilms] | v <- variants, selectable v]
  writeJson out $
    object
      ( methodHeader
          ++ [ "scenarios" .= object [K.fromText n .= v | (n, _, _, v) <- results]
             , "selection"
                .= object
                  [ "scenario" .= primaryScenario
                  , "criterion" .= ("lowest mean over Velvia, ASTIA, PRO Neg Std and PRO Neg Hi of the per-film mean e_method, then of the per-film p95; relative variants with chroma fixed to or ridged toward the construction" :: T.Text)
                  , "variant" .= variantName chosen
                  , "ranking" .= [object ["variant" .= n, "meanOfMeans" .= r3 m, "meanOfP95" .= r3 p, "maxOfMax" .= r3 x] | (n, m, p, x) <- ranking]
                  , "startStability" .= stability
                  , "startStabilityAllSelectable" .= stabilityAll
                  ]
             ]
      )
 where
  avg xs = sum xs / fromIntegral (max 1 (length xs))

chromaJson :: Chroma -> Value
chromaJson (Chroma a b c d) = object ["crFromRY" .= a, "crFromBY" .= b, "cbFromRY" .= c, "cbFromBY" .= d]

methodHeader :: [AT.Pair]
methodHeader =
  [ "patchSets" .= object [K.fromString (setName s) .= length (setPatches s) | s <- patchSets]
  , "metric" .= ("CIEDE2000 between sRGB renderings, L*a*b* relative to D65" :: T.Text)
  , "objective" .= ("mean squared CIEDE2000 over all patches, plus ridge residuals for ridge variants" :: T.Text)
  ]

-- Classic Chrome -------------------------------------------------------------

-- | Writes the v1 parameter file the patch build reads (CANDIDATE) and the
-- fit's residuals and stability (REPORT).
runFitCc :: FilePath -> FilePath -> FilePath -> FilePath -> IO ()
runFitCc bundlePath methodPath candidatePath out = do
  b0 <- readBundle bundlePath
  method <- readJson methodPath
  chosenName <- either H.failH pure (path method ["selection", "variant"] >>= AT.parseEither A.parseJSON)
  chosen <- maybe (H.failH ("unknown variant " ++ T.unpack chosenName)) pure (lookup chosenName [(variantName v, v) | v <- variants])
  results <- forM scenarios $ \sc -> do
    let ctx = context b0 sc
        b = ctxBundle ctx
        t = targets ctx ClassicChrome
        template = compileStage (xm1Template b ClassicChrome)
        rendered fit = renders (applySetting (fitSetting fit) template) (ctxInputsXm1 ctx)
        bySector goal ds = object [K.fromText s .= statsJson xs | (s, xs) <- sortOn fst [(s, [d | (d, l) <- zip ds goal, sector l == s]) | s <- uniq (map sector goal)]]
        report fit =
          let fitted = rendered fit
              vsRef = zipWith ciede2000 (reference t) fitted
              vsRel = zipWith ciede2000 (relativeTarget t) fitted
              ramp = take (length (setPatches neutralRamp)) (drop (length (setPatches colorChecker24)) (zip3 (reference t) (relativeTarget t) fitted))
           in object
                [ "setting" .= settingJson (fitSetting fit)
                , "residualVsRelativeTarget" .= breakdown vsRel
                , "residualVsRelativeTargetByHueSector" .= bySector (relativeTarget t) vsRel
                , "residualVsReference" .= breakdown vsRef
                , "residualVsReferenceByHueSector" .= bySector (reference t) vsRef
                , "ramp" .= [object ["referenceL" .= r3 l0, "relativeTargetL" .= r3 l1, "fittedL" .= r3 l2, "fittedChroma" .= r3 (sqrt (a * a + bb * bb))] | (Lab l0 _ _, Lab l1 _ _, Lab l2 a bb) <- ramp]
                ]
        modes goal = object [K.fromText (filmKey f) .= breakdown (zipWith ciede2000 goal (renders (compileStage (xm1 b f)) (ctxInputsXm1 ctx))) | f <- [Provia .. ProNegHi]]
        chosenFit = fitVariant ctx ClassicChrome chosen
        absoluteFit = fitVariant ctx ClassicChrome Absolute
    hPutStrLn stderr (T.unpack (scenarioName sc) ++ " Classic Chrome " ++ T.unpack chosenName ++ " vs relative target " ++ show (stats (zipWith ciede2000 (relativeTarget t) (rendered chosenFit))))
    pure
      ( scenarioName sc
      , fitSetting chosenFit
      , object
          [ "selected" .= report chosenFit
          , "absoluteComparison" .= report absoluteFit
          , "constructedChroma" .= either (const Null) chromaJson (maybe (Left "no side") (\s -> constructChroma s b ClassicChrome) (variantSide chosen))
          , "startStability" .= startSpread ctx ClassicChrome chosen
          , "existingModesVsRelativeTarget" .= modes (relativeTarget t)
          , "existingModesVsReference" .= modes (reference t)
          ]
      )
  primary <- maybe (H.failH "primary scenario missing") pure (lookup primaryScenario [(n, v) | (n, _, v) <- results])
  primarySetting <- maybe (H.failH "primary scenario missing") pure (lookup primaryScenario [(n, s) | (n, s, _) <- results])
  let ccTone = filmTone (xtrans2 b0 ClassicChrome)
      settings = [(n, s) | (n, s, _) <- results]
  primaryCtx <- maybe (H.failH "primary scenario missing") pure (lookup primaryScenario [(scenarioName sc, context b0 sc) | sc <- scenarios])
  eMethod <- either H.failH pure (path method ["selection", "ranking"] >>= AT.parseEither A.parseJSON >>= firstMean)
  inputs <- forM [bundlePath, methodPath] $ \p -> Input (T.pack p) . T.pack <$> (B.readFile p >>= H.sha256)
  let cand = Candidate ("PROVIA-relative fit, " <> chosenName <> ", leave-one-out selected; reports/classic-chrome-model.md") inputs (ceiling (eMethod * 100)) Nothing (settingMatrix primarySetting) (settingChroma primarySetting) ccTone
  candValue <- either H.failH pure (candidateJson cand)
  H.ensure (parseCandidate candValue == Right cand) "the candidate file does not read back to the fitted setting"
  writeJson candidatePath candValue
  writeJson out $
    object
      [ "label" .= ("approximation of Classic Chrome as shipped on EXR Processor II with X-Trans CMOS II; sibling-derived reference, not X-M1 ground truth" :: T.Text)
      , "method" .= chosenName
      , "tone" .= object ["source" .= ("X70 1.10 tone record key 38, unchanged" :: T.Text), "record" .= object ["exponents" .= toneExponents ccTone, "points" .= tonePoints ccTone]]
      , "lumaBands" .= ("PROVIA's (X70 preset 1, X-M1 hue-range id 1)" :: T.Text)
      , "filmChromaGain" .= ("PROVIA's" :: T.Text)
      , "candidateScenario" .= primaryScenario
      , "candidate" .= primary
      , "sensorStability"
          .= object
            [ "maxQ10Difference" .= object [K.fromText (n <> "-vs-" <> m) .= settingDistance s1 s2 | (n, s1) <- settings, (m, s2) <- settings, n < m]
            , "renderingSpreadOnPrimaryScenario" .= renderingSpread primaryCtx ClassicChrome (map snd settings)
            ]
      , "scenarios" .= object [K.fromText n .= v | (n, _, v) <- results]
      ]
 where
  uniq = foldr (\x acc -> if x `elem` acc then acc else x : acc) []
  firstMean :: [Value] -> Either String Double
  firstMean (r : _) = path r ["meanOfMeans"] >>= AT.parseEither A.parseJSON
  firstMean [] = Left "selection ranking is empty"

-- Image adjustments ---------------------------------------------------------------

-- | The adjustments spec 19.5.6 item 6 names, each moved alone from the default.
adjustmentSettings :: [(T.Text, Adjustments)]
adjustmentSettings =
  [("default", d)]
    ++ [("Color " <> signed v, d {adjColor = v}) | v <- steps]
    ++ [("Highlight " <> signed v, d {adjHighlight = v}) | v <- steps]
    ++ [("Shadow " <> signed v, d {adjShadow = v}) | v <- steps]
    ++ [("DR200", d {adjDr = DR200}), ("DR400", d {adjDr = DR400})]
 where
  d = defaultAdjustments
  steps = [-2, -1, 1, 2]
  signed v = T.pack (if v > 0 then '+' : show v else show v)

-- | x70view research/film-simulation/frontend.md at revision a47591d9bd.
x70FrontendSha256 :: String
x70FrontendSha256 = "f695de631e29c7b23496df412bed46e89671fbcd248cb9443637a9e3d2d11b7a"

-- | X70 1.10 Color chroma factors in Q7 for -2..+2, as the sentence
-- "the Color factor is A, B, C, D, E / 128" of the pinned frontend.md states them.
readX70ColorFactors :: FilePath -> IO [Integer]
readX70ColorFactors file = do
  bytes <- B.readFile file
  digest <- H.sha256 bytes
  H.ensure (digest == x70FrontendSha256) (file ++ ": SHA-256 is not x70view frontend.md at a47591d9bd")
  let ws = words (map (\c -> if c == ',' then ' ' else c) (T.unpack (TE.decodeUtf8Lenient bytes)))
      after ("Color" : "factor" : "is" : rest) = Just rest
      after (_ : rest) = after rest
      after [] = Nothing
      number s = case reads s of
        [(n, "")] -> Just n
        _ -> Nothing
  case after ws of
    Just (f0 : f1 : f2 : f3 : f4 : "/" : "128" : _) | Just factors <- traverse number [f0, f1, f2, f3, f4] -> pure factors
    _ -> H.failH (file ++ ": no Color factor sentence")

data Response = Response {respShadowL :: Double, respMidL :: Double, respHighL :: Double, respChroma :: Double, respMean :: Double}

-- | Change of a rendering against its default rendering: mean dL* over the
-- neutral-ramp patches whose default L* is below 35, 35-70 and above 70,
-- mean dC*ab over the 18 chromatic ColorChecker patches, mean CIEDE2000
-- over all patches.
response :: [Lab] -> [Lab] -> Response
response base moved = Response (band (< 35)) (band (\l -> l >= 35 && l <= 70)) (band (> 70)) dc (avg (zipWith ciede2000 base moved))
 where
  nCc = length (setPatches colorChecker24)
  ramp = take (length (setPatches neutralRamp)) (drop nCc (zip base moved))
  band p = avg [l1 - l0 | (Lab l0 _ _, Lab l1 _ _) <- ramp, p l0]
  cab (Lab _ a b) = sqrt (a * a + b * b)
  dc = avg [cab m - cab o | (o, m) <- take 18 (zip base moved)]
  avg xs = if null xs then 0 else sum xs / fromIntegral (length xs)

responseJson :: Response -> Value
responseJson r = object ["shadowDL" .= r3 (respShadowL r), "midDL" .= r3 (respMidL r), "highlightDL" .= r3 (respHighL r), "chromaticDC" .= r3 (respChroma r), "meanDE00" .= r3 (respMean r)]

-- | Two changes move the same way: same sign, or both under 0.5.
sameDirection :: Double -> Double -> Bool
sameDirection x y = x * y > 0 || (abs x < 0.5 && abs y < 0.5)

-- | Expected per-setting behaviour (spec 19.5.6 item 6): the X-M1 PROVIA
-- stage and the X-M1 Classic Chrome candidate (PROVIA's selectors, the
-- candidate's matrix, chroma and tone record) under each adjustment, with
-- every X-M1 selector read from the 1.50 images, against the X-Trans II
-- Classic Chrome reference under the same adjustment.
runSettings :: FilePath -> FilePath -> FilePath -> FilePath -> FilePath -> FilePath -> FilePath -> IO ()
runSettings bundlePath candidatePath id4 id5 payload frontend out = do
  colorFactors <- readX70ColorFactors frontend
  b <- readBundle bundlePath
  cand <- readCandidate candidatePath
  im <- loadImages id4 id5 payload >>= either H.failH pure
  gains <- either H.failH pure (gainRecords im)
  tones <- either H.failH pure (toneRecords im)
  let provia = DevelopCode 0
      Exposure k = calibrateExposure (compileStage (xtrans2 b Provia))
      inputs cam adj = map (sceneInput cam (Exposure (k / 2 ^ fromEnum (adjDr adj))) . patchXyz) allPatches
      record rs key = maybe (Left ("no record " ++ show key)) Right (lookup key rs)
      setting adj = do
        gainKey <- gainTableKeyAt im provia adj
        g <- record gains gainKey
        toneKey <- toneKeyAt im provia adj
        t <- record tones toneKey
        product' <- chromaGainProductAt im provia adj
        let pv = xm1 b Provia
        pvChroma <- scaleChroma product' (filmChroma pv)
        ccChroma <- scaleChroma product' (candidateChroma cand)
        let xmProvia = pv {filmGainTable = g, filmTone = t, filmChroma = pvChroma}
            xmCc = pv {filmGainTable = g, filmMatrix = candidateMatrix cand, filmTone = candidateTone cand, filmChroma = ccChroma}
            xt = xtrans2 b ClassicChrome
            refProduct = (colorFactors !! (adjColor adj + 2)) * 2 ^ (27 :: Int)
        refChroma <- scaleChroma refProduct (filmChroma xt)
        let ref = xt {filmGainTable = g, filmChroma = refChroma}
        pure (gainKey, toneKey, product', xmProvia, xmCc, ref)
  rows <- forM adjustmentSettings $ \(name, adj) -> do
    (gainKey, toneKey, product', xmProvia, xmCc, ref) <- either H.failH pure (setting adj)
    let xmIn = inputs xm1Camera adj
        xtIn = inputs x70Camera adj
    pure (name, gainKey, toneKey, product', renders (compileStage xmProvia) xmIn, renders (compileStage xmCc) xmIn, renders (compileStage ref) xtIn)
  (_, _, _, _, baseP, baseC, baseR) <- case rows of
    (r : _) -> pure r
    [] -> H.failH "no settings"
  let table =
        [ (name, gainKey, toneKey, product', response baseP p, response baseC c, response baseR r, avg (zipWith ciede2000 p c))
        | (name, gainKey, toneKey, product', p, c, r) <- drop 1 rows
        ]
      agrees rc rr = and (zipWith sameDirection (comps rc) (comps rr))
      comps r = [respShadowL r, respMidL r, respHighL r, respChroma r]
      ratio p = fromIntegral p / 2 ^ (34 :: Int) :: Double
      f2 x = let s = show (fromIntegral (round (x * 100) :: Integer) / 100 :: Double) in if x >= 0 then '+' : s else s
      triple r = T.pack (f2 (respShadowL r) ++ " / " ++ f2 (respMidL r) ++ " / " ++ f2 (respHighL r) ++ ", C " ++ f2 (respChroma r))
  writeJson out $
    object
      [ "label" .= ("expected response of the X-M1 Classic Chrome candidate to image adjustments; X-Trans II reference sibling-derived" :: T.Text)
      , "metrics" .= ("dL* means over neutral-ramp patches with default L* < 35, 35-70, > 70; dC*ab mean over the 18 chromatic ColorChecker patches; CIEDE2000 mean over all patches; each against the same pipeline's default rendering" :: T.Text)
      , "settings"
          .= [ object
                 [ "setting" .= name
                 , "xm1GainRecordKey" .= gainKey
                 , "xm1ProviaToneKey" .= toneKey
                 , "xm1ChromaGain" .= r3 (ratio product')
                 , "xm1Provia" .= responseJson rp
                 , "xm1ClassicChrome" .= responseJson rc
                 , "xtrans2ClassicChromeReference" .= responseJson rr
                 , "classicChromeVsProviaMeanDE00" .= r3 dcp
                 , "directionAgreesWithReference" .= agrees rc rr
                 ]
             | (name, gainKey, toneKey, product', rp, rc, rr, dcp) <- table
             ]
      ]
  putStrLn "| Setting | X-M1 gain record | Chroma gain | X-M1 Classic Chrome dL* shadow / mid / highlight, dC* | X-M1 PROVIA | X-Trans II Classic Chrome reference | Same direction |"
  putStrLn "| --- | ---: | ---: | --- | --- | --- | --- |"
  forM_ table $ \(name, gainKey, _, product', rp, rc, rr, _) ->
    putStrLn (T.unpack (T.intercalate " | " ["| " <> name, T.pack (show gainKey), T.pack (show (r3 (ratio product'))), triple rc, triple rp, triple rr, if agrees rc rr then "yes" else "no"]) ++ " |")
 where
  avg xs = if null xs then 0 else sum xs / fromIntegral (length xs)

readCandidate :: FilePath -> IO Candidate
readCandidate p = readJson p >>= either (H.failH . ((p ++ ": ") ++)) pure . parseCandidate

-- | The X-M1 Classic Chrome stage at DR100 with adjustments 0: PROVIA's
-- selectors with the candidate's matrix, chroma and tone record.
xm1Candidate :: Bundle -> Candidate -> FilmParams
xm1Candidate b cand = (xm1 b Provia) {filmMatrix = candidateMatrix cand, filmChroma = candidateChroma cand, filmTone = candidateTone cand}

-- | A film against PROVIA for Fujifilm's F-Log2 LUTs and the X-M1 stage
-- (the candidate for classicChrome) on the neutral ramp, a low-chroma grid
-- and the gamut set, and for that film's regions of an e-model output
-- (observed and predicted).
runLookReference :: T.Text -> FilePath -> FilePath -> FilePath -> FilePath -> FilePath -> FilePath -> IO ()
runLookReference film proviaCube filmCube bundlePath candidatePath eModelPath out = do
  let readCube p = H.readRegular (64 * 1024 * 1024) p >>= either (H.failH . ((p ++ ": ") ++)) pure . parseCube
  pvLut <- readCube proviaCube
  filmLut <- readCube filmCube
  b <- readBundle bundlePath
  cand <- readCandidate candidatePath
  filmParams <- maybe (H.failH ("no X-M1 stage for film " ++ T.unpack film)) pure (lookup film (("classicChrome", xm1Candidate b cand) : [(filmKey f, xm1 b f) | f <- [Velvia .. ProNegHi]]))
  eModelOut <- readJson eModelPath
  regions <- either H.failH pure (filmRegions film eModelOut)
  digests <- forM [proviaCube, filmCube, bundlePath, candidatePath, eModelPath] $ \p -> (K.fromString p .=) <$> (B.readFile p >>= H.sha256)
  let lowChroma = labGrid "low-chroma" [20, 30 .. 90] [0, 20 .. 340] [4, 8, 12]
      sets = [neutralRamp, lowChroma, gamutSet]
      scene = concatMap setPatches sets
      Exposure k = calibrateExposure (compileStage (xtrans2 b Provia))
      inputs = map (sceneInput xm1Camera (Exposure k) . patchXyz) scene
      xmProvia = compileStage (xm1 b Provia)
      xmFilm = compileStage filmParams
      lutPairs = [LabPair (lutLab pvLut (patchXyz p)) (lutLab filmLut (patchXyz p)) | p <- scene]
      modelPairs = [LabPair (renderLab xmProvia w) (renderLab xmFilm w) | w <- inputs]
      ramp cube = [r3 l | p <- setPatches neutralRamp, let Lab l _ _ = lutLab cube (patchXyz p)]
      modelRamp st = [r3 l | p <- setPatches neutralRamp, let Lab l _ _ = renderLab st (sceneInput xm1Camera (Exposure k) (patchXyz p))]
  writeJson out $
    object
      [ "inputs" .= object digests
      , "film" .= film
      , "scene" .= object ["reflection" .= ("D65, white Y = 1" :: T.Text), "sets" .= object [K.fromString (setName s) .= length (setPatches s) | s <- sets]]
      , "lut"
          .= object
            [ "method" .= ("XYZ to F-Gamut, F-Log2 (Data Sheet Ver.1.1), trilinear 3D LUT, BT.709 output read with the sRGB transfer function, CIELAB D65" :: T.Text)
            , "rampY" .= [r3 y | p <- setPatches neutralRamp, let Xyz _ y _ = patchXyz p]
            , "rampL" .= object ["provia" .= ramp pvLut, "film" .= ramp filmLut]
            , "relative" .= relativeStats lutPairs
            ]
      , "model"
          .= object
            [ "method" .= ("X-M1 sensor model, exposure of section 4, X-M1 PROVIA stage against the film's X-M1 stage (the candidate for classicChrome)" :: T.Text)
            , "rampL" .= object ["provia" .= modelRamp xmProvia, "film" .= modelRamp xmFilm]
            , "relative" .= relativeStats modelPairs
            ]
      , "pair" .= object ["observed" .= relativeStats [LabPair p o | (p, o, _) <- regions], "predicted" .= relativeStats [LabPair p q | (p, _, q) <- regions], "regions" .= length regions]
      ]

-- | (PROVIA, observed, predicted) L*a*b* of every region of one film in an
-- e-model output.
filmRegions :: T.Text -> Value -> Either String [(Lab, Lab, Lab)]
filmRegions film v = do
  scenes <- path v ["scenes"]
  sceneList <- case scenes of
    Object o -> Right (KM.elems o)
    _ -> Left "scenes is not an object"
  fmap concat $ forM sceneList $ \sc -> case path sc ["films", film, "regions"] of
    Left _ -> Right []
    Right rs -> do
      items <- AT.parseEither A.parseJSON rs :: Either String [Value]
      forM items $ \r -> (,,) <$> lab r "provia" <*> lab r "observed" <*> lab r "predicted"
 where
  lab r k =
    path r [k] >>= AT.parseEither A.parseJSON >>= \xs -> case xs of
      [l, a, bb] -> Right (Lab l a bb)
      _ -> Left "L*a*b* needs three values"

-- Classic Chrome against PROVIA ---------------------------------------------------

-- | The relative statistics of every manifest group and, given a bundle and
-- candidate, of the patch set through the X-Trans II PROVIA and Classic
-- Chrome stages (X70 sensor) and through the X-M1 PROVIA stage and the
-- candidate (X-M1 sensor), at the fit's exposure. For a group marked for
-- X-M1 prediction, each PROVIA block is inverted through the X-M1 PROVIA
-- stage and developed through the candidate, which gives a predicted source
-- on the measured colours and the CIEDE2000 of that prediction against the
-- measured Classic Chrome block.
runRelative :: Decoder -> FilePath -> FilePath -> [FilePath] -> IO ()
runRelative dec manifest out model = do
  (groups, pairsJson) <- relativePairs dec manifest
  let measured = [(groupName g, [(yccLab p, yccLab c) | (p, c) <- groupBlocks g]) | g <- groups]
  (modelled, predictions) <- case model of
    [bundlePath, candidatePath] -> do
      b <- readBundle bundlePath
      cand <- readCandidate candidatePath
      let k = calibrateExposure (compileStage (xtrans2 b Provia))
          inputs cam = map (sceneInput cam k . patchXyz) allPatches
          pv = xm1 b Provia
          cc = xm1Candidate b cand
          pvStage = compileStage pv
          ccStage = compileStage cc
          pair p c xs = zip (renders (compileStage p) xs) (renders (compileStage c) xs)
          predict g =
            let rows = [(yccLab p, yccLab c, renderLab pvStage w, renderLab ccStage w) | (p@(Ycc y cb cr), c) <- groupBlocks g, Just w <- [invert pvStage y cb cr]]
                st ds = let Stats m p x = stats ds in object ["mean" .= r3 m, "p95" .= r3 p, "max" .= r3 x, "n" .= length ds]
             in ( (groupName g <> " predicted", [(p, q) | (p, _, _, q) <- rows])
                , ( groupName g
                  , object
                      [ "notInvertible" .= (length (groupBlocks g) - length rows)
                      , "proviaRedevelopResidual" .= st [ciede2000 p r | (p, _, r, _) <- rows]
                      , "predictedVsMeasured" .= st [ciede2000 c q | (_, c, _, q) <- rows]
                      , "proviaAsPrediction" .= st [ciede2000 c p | (p, c, _, _) <- rows]
                      ]
                  )
                )
          predicted = [predict g | g <- groups, groupXm1Prediction g]
      pure
        ( [ ("model X-Trans II", pair (xtrans2 b Provia) (xtrans2 b ClassicChrome) (inputs x70Camera))
          , ("model X-M1 candidate", pair pv cc (inputs xm1Camera))
          ]
            ++ map fst predicted
        , map snd predicted
        )
    _ -> pure ([], [])
  let sources = modelled ++ measured
  writeJson out $
    object
      [ "metric" .= ("per block or patch, Classic Chrome against PROVIA: C*ab ratio, dL*, hue shift in degrees; binned by the PROVIA colour's 30-degree hue sector (C* >= 5) and chroma band, and dL* by PROVIA L* for neutral (C* < 5) and chromatic blocks; sRGB L*a*b* relative to D65" :: T.Text)
      , "pairs" .= pairsJson
      , "sources" .= object [K.fromText n .= pairDeltaStats bs | (n, bs) <- sources]
      , "xm1Predictions" .= object [K.fromText n .= v | (n, v) <- predictions]
      ]
  putStr (pairDeltaTables sources)
