{-# LANGUAGE OverloadedStrings, ScopedTypeVariables #-}
-- Host annotation of external-video presentation-timestamp brackets for one
-- ordinary-photography AF attempt. Frame indices follow the recording's
-- presentation order, the order ffprobe lists frames and ffmpeg's select
-- filter counts, so a bracket picked by eye maps to one index space. No camera
-- or USB access.
module FujiAF.Session
  ( -- commands
    commandFrames
  , commandExtract
  , commandAnnotate
  , commandMeasure
  ) where

import Control.Monad (forM_, when)
import Data.Aeson (Value (..))
import Data.Char (isDigit)
import Data.List (sort)
import Data.Maybe (fromMaybe, isJust)
import Data.Ratio ((%))
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as BC
import Data.Scientific (floatingOrInteger)
import FujiAF.Host
import FujiAF.JSON (decodeStrict, encodeCanonical)
import System.Directory (createDirectoryIfMissing, listDirectory)
import System.Exit (ExitCode (..))
import System.FilePath (takeDirectory, (</>))
import System.IO (hPutStrLn, stderr)

-- | A stream time base: one tick is @tbNum / tbDen@ seconds.
data TimeBase = TimeBase { tbNum :: Integer, tbDen :: Integer }
  deriving (Eq, Show)

parseTimeBase :: String -> Maybe TimeBase
parseTimeBase spec = case break (== '/') spec of
  (whole, '/':frac)
    | all isDigit whole, all isDigit frac, not (null whole), not (null frac) ->
        let numer = read whole; denom = read frac
        in if numer > 0 && denom > 0 then Just (TimeBase numer denom) else Nothing
  _ -> Nothing

ticksToMs :: TimeBase -> Integer -> Rational
ticksToMs tb ticks = fromIntegral ticks * 1000 * (tbNum tb % tbDen tb)

data Rounding = Down | Up | Nearest
  deriving (Eq, Show)

-- | A millisecond value in whole nanoseconds, rounded in the requested direction.
nanos :: Rational -> Rounding -> Integer
nanos value direction = case direction of
  Down -> floor scaled
  Up -> ceiling scaled
  Nearest -> floor (scaled + 1 % 2)
  where scaled = value * 1000000

-- | Fixed six-decimal millisecond string for a nanosecond count, sign included.
formatMs :: Integer -> String
formatMs count
  | count < 0 = '-' : formatMs (abs count)
  | otherwise = show whole ++ "." ++ replicate (6 - length frac) '0' ++ frac
  where
    (whole, rest) = abs count `divMod` 1000000
    frac = show rest

data Edge = Edge { edgeIndex :: Int, edgeTicks :: Integer }
  deriving (Eq, Show)

-- | @before@ is the last frame without the event, @after@ the first with it.
data Bracket = Bracket
  { bracketBefore :: Maybe Edge
  , bracketAfter :: Maybe Edge
  , bracketAmbiguous :: Bool
  } deriving (Eq, Show)

bracketComplete :: Bracket -> Bool
bracketComplete candidate =
  isJust (bracketBefore candidate) && isJust (bracketAfter candidate) && not (bracketAmbiguous candidate)

-- | Confirmation latency from the protocol: with a start bracket [s0,s1] and a
-- confirmation bracket [c0,c1], the interval is [max(0,c0-s1), c1-s0].
latencyInterval :: TimeBase -> Bracket -> Bracket -> Maybe (Rational, Rational)
latencyInterval tb start confirmation = do
  s0 <- bracketBefore start
  s1 <- bracketAfter start
  c0 <- bracketBefore confirmation
  c1 <- bracketAfter confirmation
  guardM (bracketComplete start && bracketComplete confirmation)
  guardM (edgeIndex s0 < edgeIndex s1 && edgeIndex c0 < edgeIndex c1)
  let lower = max 0 (ticksToMs tb (edgeTicks c0) - ticksToMs tb (edgeTicks s1))
      upper = ticksToMs tb (edgeTicks c1) - ticksToMs tb (edgeTicks s0)
  guardM (lower <= upper)
  pure (lower, upper)

data MotionRelation = RelationTrails | RelationOverlaps | RelationLeads | RelationUnresolved
  deriving (Eq, Show)

-- | Signed confirmation-minus-motion-end interval over the brackets, widened by
-- the declared uncertainty, with the relation its sign supports. A bracketed
-- order whose sign the widened interval does not resolve is reported as
-- unresolved rather than as a weaker version of itself.
motionRelation :: TimeBase -> Rational -> Bracket -> Bracket -> Maybe (MotionRelation, Rational, Rational)
motionRelation tb uncertainty motion confirmation = do
  m0 <- bracketBefore motion
  m1 <- bracketAfter motion
  c0 <- bracketBefore confirmation
  c1 <- bracketAfter confirmation
  guardM (bracketComplete motion && bracketComplete confirmation)
  let lower = ticksToMs tb (edgeTicks c0) - ticksToMs tb (edgeTicks m1) - uncertainty
      upper = ticksToMs tb (edgeTicks c1) - ticksToMs tb (edgeTicks m0) + uncertainty
      bracketed
        | edgeIndex c1 <= edgeIndex m0 = RelationLeads
        | edgeIndex c0 >= edgeIndex m1 = RelationTrails
        | otherwise = RelationOverlaps
      relation = case bracketed of
        RelationLeads | upper < 0 -> RelationLeads
        RelationTrails | lower > 0 -> RelationTrails
        RelationOverlaps -> RelationOverlaps
        _ -> RelationUnresolved
  pure (relation, lower, upper)

guardM :: Bool -> Maybe ()
guardM yes = if yes then Just () else Nothing

-- | Full-press-to-capture window: [max(0, capture.before - press.after),
-- capture.after - press.before] widened by the declared uncertainty, with the
-- 2 s verdict resolved only when the widened interval does not span it. Bounds
-- are ordered and nonnegative; an order the brackets do not resolve (the
-- capture's first-with frame not after the press's first-with frame) is
-- unresolved whatever the bounds say.
captureWindow :: TimeBase -> Rational -> Bracket -> Bracket -> Maybe (Rational, Rational, String)
captureWindow tb uncertainty press capture = do
  p0 <- bracketBefore press
  p1 <- bracketAfter press
  c0 <- bracketBefore capture
  c1 <- bracketAfter capture
  guardM (bracketComplete press && bracketComplete capture)
  let rawLower = max 0 (ticksToMs tb (edgeTicks c0) - ticksToMs tb (edgeTicks p1) - uncertainty)
      rawUpper = ticksToMs tb (edgeTicks c1) - ticksToMs tb (edgeTicks p0) + uncertainty
      upper = max rawLower rawUpper
      verdict
        | edgeIndex c1 <= edgeIndex p1 = "unresolved"
        | upper <= 2000 = "within"
        | rawLower > 2000 = "after"
        | otherwise = "unresolved"
  pure (rawLower, upper, verdict)

data Spacing = Spacing
  { spacingCount :: Int
  , spacingDuplicatePts :: Int
  , spacingNonMonotonicPts :: Int
  , spacingMinMs :: Rational
  , spacingMaxMs :: Rational
  , spacingMeanMs :: Rational
  , spacingGapsAbove :: Int
  } deriving (Eq, Show)

-- | Inter-frame summary of a presentation-timestamp list. Duplicate and
-- nonmonotonic timestamps are counted, never repaired; gaps above 1.5x the
-- lower median spacing are counted against a zero median as well.
summarizeSpacing :: TimeBase -> [Integer] -> Maybe Spacing
summarizeSpacing tb pts = do
  let deltas = zipWith (-) (drop 1 pts) pts
      ms = map (ticksToMs tb) deltas
      sorted = sort deltas
  guardM (length deltas >= 1)
  median <- atMay sorted ((length deltas - 1) `div` 2)
  (lo, hi) <- extremes ms
  pure Spacing
    { spacingCount = length deltas
    , spacingDuplicatePts = length (filter (== 0) deltas)
    , spacingNonMonotonicPts = length (filter (< 0) deltas)
    , spacingMinMs = lo
    , spacingMaxMs = hi
    , spacingMeanMs = sum ms / fromIntegral (length deltas)
    , spacingGapsAbove = length (filter (\d -> 2 * d > 3 * median) deltas)
    }

-- | Minimum and maximum in one pass, Nothing for an empty list.
extremes :: [Rational] -> Maybe (Rational, Rational)
extremes [] = Nothing
extremes (x:xs) = Just (foldl' (\(lo, hi) v -> (min lo v, max hi v)) (x, x) xs)

-- | Total index access; Nothing when the index is outside the list.
atMay :: [a] -> Int -> Maybe a
atMay xs i
  | i < 0 = Nothing
  | otherwise = case drop i xs of
      (x:_) -> Just x
      [] -> Nothing

data Probe = Probe
  { probeTimeBase :: TimeBase
  , probeFramePts :: [Integer]
  , probeStream :: Value
  } deriving (Show)

-- | Requires a video stream with a parsable time base and one integer @pts@ per
-- decoded frame. A frame without an integer presentation timestamp is a
-- recording defect, not a value to interpolate.
parseProbe :: Value -> Either String Probe
parseProbe value = do
  stream <- case list (get value "streams") of
    (x:_) -> Right x
    [] -> Left "ffprobe reported no video stream"
  tb <- maybe (Left "stream time_base missing or unparsable")
              Right (parseTimeBase (str (get stream "time_base")))
  let rawFrames = list (get value "frames")
  if null rawFrames
    then Left "ffprobe reported no frames"
    else do
      pts <- mapM framePts (zip [0 :: Int ..] rawFrames)
      pure Probe
        { probeTimeBase = tb
        , probeFramePts = pts
        , probeStream = obj
            [ ("codec_name", get stream "codec_name")
            , ("width", get stream "width")
            , ("height", get stream "height")
            , ("avg_frame_rate", get stream "avg_frame_rate")
            ]
        }
  where
    framePts (i, frame) = case get frame "pts" of
      Number x -> case floatingOrInteger x :: Either Double Integer of
        Right t -> Right t
        Left _ -> Left ("frame " ++ show i ++ " has a fractional pts")
      other -> Left ("frame " ++ show i ++ " has no integer pts (" ++ take 32 (show other) ++ ")")

-- argv helpers ---------------------------------------------------------------

flagValue :: [String] -> String -> Maybe String
flagValue args flag = go args
  where
    go (x:y:rest) | x == flag = Just y
                  | otherwise = go (y : rest)
    go _ = Nothing

hasFlag :: [String] -> String -> Bool
hasFlag args flag = flag `elem` args

-- | Refuse a repeated flag: first-wins parsing would silently drop a value.
ensureNoRepeats :: [String] -> IO ()
ensureNoRepeats = go []
  where
    go _ [] = pure ()
    go seen (arg:rest) = do
      ensure (take 2 arg /= "--" || arg `notElem` seen) ("repeated flag " ++ arg)
      go (arg : seen) rest

requireArg :: [String] -> String -> String -> IO String
requireArg args flag label =
  maybe (failH (label ++ ": " ++ flag ++ " is required")) pure (flagValue args flag)

intArg :: [String] -> String -> String -> IO (Maybe Int)
intArg args flag label = case flagValue args flag of
  Nothing -> pure Nothing
  Just text -> case reads text of
    [(v, "")] | v >= 0 && v <= 100000000 -> pure (Just v)
    _ -> failH (label ++ ": " ++ flag ++ " needs a nonnegative integer")

msArg :: [String] -> String -> String -> IO (Maybe Rational)
msArg args flag label = case flagValue args flag of
  Nothing -> pure Nothing
  Just text -> case parseFixedDecimal text of
    Just v | v >= 0 -> pure (Just v)
    _ -> failH (label ++ ": " ++ flag ++ " needs a nonnegative decimal number")

parseFixedDecimal :: String -> Maybe Rational
parseFixedDecimal text = case break (== '.') text of
  (whole, "") | digits whole -> Just (read whole % 1)
  (whole, '.':frac) | digits whole, digits frac ->
    Just (read (whole ++ frac) % (10 ^ length frac))
  _ -> Nothing
  where digits part = not (null part) && length part <= 12 && all isDigit part

-- command: frames ------------------------------------------------------------

ffprobeTimeout, ffmpegTimeout, hashTimeout :: Int
ffprobeTimeout = 300000000
ffmpegTimeout = 900000000
hashTimeout = 900000000

commandFrames :: [String] -> IO ()
commandFrames args = do
  ensureNoRepeats args
  video <- requireArg args "--video" "frames"
  let ffprobePath = fromMaybe "ffprobe" (flagValue args "--ffprobe")
      hasher = fromMaybe "sha256sum" (flagValue args "--sha256sum")
  (timed, code, out, err) <- runBounded ffprobeTimeout (16 * 1024 * 1024) ffprobePath
    [ "-v", "error"
    , "-select_streams", "v:0"
    , "-show_entries", "stream=index,codec_name,width,height,avg_frame_rate,time_base:frame=pts"
    , "-of", "json"
    , video
    ] B.empty
  ensure (not timed) "ffprobe exceeded its time budget"
  ensure (code == ExitSuccess) ("ffprobe failed: " ++ BC.unpack (B.take 512 err))
  probeValue <- either failH pure (decodeStrict out)
  probe <- either failH pure (parseProbe probeValue)
  let tb = probeTimeBase probe
      pts = probeFramePts probe
  spacing <- maybe (failH "recording has fewer than two frames") pure (summarizeSpacing tb pts)
  when (spacingDuplicatePts spacing > 0 || spacingNonMonotonicPts spacing > 0) $
    hPutStrLn stderr "af-session: warning: the recording has duplicate or nonmonotonic presentation timestamps; annotate refuses it"
  digest <- fileDigest hasher video
  let document = obj
        [ ("schema_version", n 1)
        , ("record_type", s "af-video-frames")
        , ("video", obj [("path", s video), ("sha256", s digest)])
        , ("clock", obj
            [ ("clock_id", s "ext-video-pts")
            , ("source", s "ffprobe presentation timestamps of the recording's video stream")
            , ("time_base", s (show (tbNum tb) ++ "/" ++ show (tbDen tb)))
            , ("origin", s "container timeline origin; the first listed frame is not assumed to start at zero")
            , ("wrap_width", Null)
            , ("synchronization", s "all brackets are read within this one recording; no cross-recording subtraction")
            , ("uncertainty_ms", Null)
            ])
        , ("stream", probeStream probe)
        , ("frames", arr
            [ obj
                [ ("index", n (fromIntegral i))
                , ("pts", obj [("clock_id", s "ext-video-pts"), ("ticks", s (show p))])
                , ("ms", s (formatMs (nanos (ticksToMs tb p) Nearest)))
                ]
            | (i, p) <- zip [0 :: Int ..] pts
            ])
        , ("spacing", obj
            [ ("count", n (fromIntegral (spacingCount spacing)))
            , ("min_ms", s (formatMs (nanos (spacingMinMs spacing) Down)))
            , ("max_ms", s (formatMs (nanos (spacingMaxMs spacing) Up)))
            , ("mean_ms", s (formatMs (nanos (spacingMeanMs spacing) Nearest)))
            , ("duplicate_pts", n (fromIntegral (spacingDuplicatePts spacing)))
            , ("non_monotonic_pts", n (fromIntegral (spacingNonMonotonicPts spacing)))
            , ("gaps_above_1_5_median", n (fromIntegral (spacingGapsAbove spacing)))
            ])
        ]
  writeOut (flagValue args "--out") document

fileDigest :: FilePath -> FilePath -> IO String
fileDigest hasher path = do
  (timed, code, out, err) <- runBounded hashTimeout 4096 hasher [path] B.empty
  ensure (not timed) "sha256sum exceeded its time budget"
  ensure (code == ExitSuccess) ("sha256sum failed: " ++ BC.unpack (B.take 512 err))
  let digest = take 64 (BC.unpack out)
  ensure (length digest == 64 && all isHex digest) "sha256sum returned no 64-hex digest"
  pure digest

isHex :: Char -> Bool
isHex c = isDigit c || ('a' <= c && c <= 'f')

writeOut :: Maybe FilePath -> Value -> IO ()
writeOut Nothing document = jsonPrint document
writeOut (Just path) document = do
  createDirectoryIfMissing True (takeDirectory path)
  B.writeFile path (encodeCanonical document <> "\n")

-- command: measure -----------------------------------------------------------

-- | @X:Y:W:H@ region in image pixels; the crop must hold at least one interior
-- pixel on each side for the central differences.
parseRoi :: String -> Either String (Int, Int, Int, Int)
parseRoi text = case splitOn ':' text of
  [xs, ys, ws, hs] -> do
    x <- value xs
    y <- value ys
    w <- value ws
    h <- value hs
    if w >= 3 && h >= 3 && w <= 100000 && h <= 100000
      then Right (x, y, w, h)
      else Left "ROI width and height must be at least 3 pixels"
  _ -> Left "ROI must be X:Y:W:H"
  where
    value part
      | not (null part) && length part <= 6 && all isDigit part = Right (read part)
      | otherwise = Left ("malformed ROI number: " ++ part)

-- | Mean central-difference gradient magnitude over the ROI interior in 8-bit
-- units, with the fraction of ROI bytes at 255. One linear pass over the
-- decoded bytes; no intermediate list.
gradientStats :: Int -> Int -> B.ByteString -> Maybe (Rational, Rational)
gradientStats w h bytes
  | w < 3 || h < 3 || B.length bytes /= w * h = Nothing
  | otherwise = Just (fromIntegral total / (2 * fromIntegral interiorCount), fromIntegral clipped / fromIntegral (w * h))
  where
    interiorCount = (w - 2) * (h - 2)
    at index = fromIntegral (B.index bytes index) :: Integer
    total = foldl' step 0 [y * w + x | y <- [1 .. h - 2], x <- [1 .. w - 2]]
    step acc index = acc + abs (at (index + 1) - at (index - 1)) + abs (at (index + w) - at (index - w))
    clipped = B.count 255 bytes

commandMeasure :: [String] -> IO ()
commandMeasure args = do
  ensureNoRepeats args
  image <- requireArg args "--image" "measure"
  roiText <- requireArg args "--roi" "measure"
  (x, y, w, h) <- either failH pure (parseRoi roiText)
  let ffmpegPath = fromMaybe "ffmpeg" (flagValue args "--ffmpeg")
      ffprobePath = fromMaybe "ffprobe" (flagValue args "--ffprobe")
      hasher = fromMaybe "sha256sum" (flagValue args "--sha256sum")
  (probeTimed, probeCode, probeOut, probeErr) <- runBounded ffprobeTimeout (1024 * 1024) ffprobePath
    [ "-v", "error", "-select_streams", "v:0"
    , "-show_entries", "stream=width,height", "-of", "json", image
    ] B.empty
  ensure (not probeTimed) "ffprobe exceeded its time budget"
  ensure (probeCode == ExitSuccess) ("ffprobe failed: " ++ BC.unpack (B.take 512 probeErr))
  probeValue <- either failH pure (decodeStrict probeOut)
  (imageWidth, imageHeight) <- case list (get probeValue "streams") of
    (stream:_) -> do
      let width = num (get stream "width")
          height = num (get stream "height")
      ensure (width > 0 && height > 0) "image has no pixel dimensions"
      pure (width, height)
    [] -> failH "ffprobe reported no image stream"
  ensure (fromIntegral x + fromIntegral w <= imageWidth && fromIntegral y + fromIntegral h <= imageHeight)
    "ROI falls outside the image"
  (timed, code, out, err) <- runBounded ffmpegTimeout (64 * 1024 * 1024) ffmpegPath
    [ "-v", "error", "-i", image
    , "-vf", "format=gray,crop=" ++ show w ++ ":" ++ show h ++ ":" ++ show x ++ ":" ++ show y
    , "-f", "rawvideo", "-pix_fmt", "gray", "-"
    ] B.empty
  ensure (not timed) "ffmpeg exceeded its time budget"
  ensure (code == ExitSuccess) ("ffmpeg failed: " ++ BC.unpack (B.take 512 err))
  ensure (B.length out == w * h) "decoded image does not match the requested ROI"
  (acutance, clipped) <- maybe (failH "ROI needs at least 3 by 3 pixels") pure (gradientStats w h out)
  sweepMax <- msArg args "--sweep-max" "measure"
  digest <- fileDigest hasher image
  let document = obj
        [ ("schema_version", n 1)
        , ("record_type", s "af-image-acutance")
        , ("image", obj [("path", s image), ("sha256", s digest)])
        , ("roi", obj [("x", n (fromIntegral x)), ("y", n (fromIntegral y)), ("width", n (fromIntegral w)), ("height", n (fromIntegral h))])
        , ("metric", s "mean-central-gradient-8bit")
        , ("acutance", s (formatMs (nanos acutance Up)))
        , ("saturated_fraction", s (formatMs (nanos clipped Nearest)))
        , ("pixels", n (fromIntegral (w * h)))
        , ("normalized", case sweepMax of
            Just m | m > 0 -> s (formatMs (nanos (acutance / m) Nearest))
            Just _ -> Null
            Nothing -> Null)
        ]
  writeOut (flagValue args "--out") document

-- command: extract -----------------------------------------------------------

-- | A comma-separated list of single indices and inclusive @A-B@ ranges, capped
-- so a typo cannot expand into an unbounded selection.
parseFrameSpec :: String -> Either String [Int]
parseFrameSpec spec
  | null tokens || any null tokens = Left "empty frame selection"
  | otherwise = do
      selection <- concat <$> mapM token tokens
      if length selection > 4096
        then Left "frame selection exceeds 4096 frames"
        else Right (sortUnique selection)
  where
    tokens = splitOn ',' spec
    token text = case break (== '-') text of
      (a, '-':bPart) -> do
        lo <- index a
        hi <- index bPart
        if lo <= hi then Right [lo .. hi] else Left ("reversed frame range: " ++ text)
      (a, "") -> (\i -> [i]) <$> index a
      _ -> Left ("malformed frame token: " ++ text)
    index text
      | not (null text) && length text <= 8 && all isDigit text = Right (read text)
      | otherwise = Left ("malformed frame index: " ++ text)
    sortUnique = foldl' (\acc x -> if x `elem` acc then acc else acc ++ [x]) []

splitOn :: Char -> String -> [String]
splitOn sep text = case break (== sep) text of
  (chunk, []) -> [chunk]
  (chunk, _ : rest) -> chunk : splitOn sep rest

-- | Merge sorted unique indices into contiguous runs.
coalesceRuns :: [Int] -> [(Int, Int)]
coalesceRuns [] = []
coalesceRuns (x:xs) = go x x xs
  where
    go start end [] = [(start, end)]
    go start end (y:ys)
      | y == end + 1 = go start y ys
      | otherwise = (start, end) : go y y ys

frameFileName :: Int -> String
frameFileName index = "frame-" ++ replicate (6 - length text) '0' ++ text ++ ".png"
  where text = show index

commandExtract :: [String] -> IO ()
commandExtract args = do
  ensureNoRepeats args
  video <- requireArg args "--video" "extract"
  spec <- requireArg args "--frames" "extract"
  outDir <- requireArg args "--out" "extract"
  let ffmpegPath = fromMaybe "ffmpeg" (flagValue args "--ffmpeg")
  indices <- either failH pure (parseFrameSpec spec)
  let runs = coalesceRuns indices
  ensure (not (null runs)) "frame selection is empty"
  ensure (length runs <= 64) "at most 64 contiguous frame runs per extraction"
  ensure (length indices <= 512) "at most 512 frames per extraction"
  createDirectoryIfMissing True outDir
  forM_ runs $ \(lo, hi) -> do
    let pattern = outDir </> "frame-%06d.png"
        filterArg = "select=between(n\\," ++ show lo ++ "\\," ++ show hi ++ ")"
    (timed, code, _, err) <- runBounded ffmpegTimeout (4 * 1024 * 1024) ffmpegPath
      [ "-v", "error", "-nostdin", "-y"
      , "-i", video
      , "-vf", filterArg
      , "-fps_mode", "passthrough"
      , "-start_number", show lo
      , pattern
      ] B.empty
    ensure (not timed) "ffmpeg exceeded its time budget"
    ensure (code == ExitSuccess) ("ffmpeg failed: " ++ BC.unpack (B.take 512 err))
  names <- listDirectory outDir
  let wanted = map frameFileName indices
      missing = filter (`notElem` names) wanted
  ensure (null missing) ("extraction wrote no file for: " ++ unwords (take 8 missing))
  mapM_ putStrLn (sort wanted)

-- command: annotate ----------------------------------------------------------

commandAnnotate :: [String] -> IO ()
commandAnnotate args = do
  ensureNoRepeats args
  video <- requireArg args "--video" "annotate"
  framesPath <- requireArg args "--frames" "annotate"
  sessionId <- requireArg args "--session-id" "annotate"
  trialId <- requireArg args "--trial-id" "annotate"
  sceneId <- requireArg args "--scene-id" "annotate"
  startKind <- requireArg args "--start" "annotate"
  bytes <- readRegular (64 * 1024 * 1024) framesPath
  listing <- either failH pure (decodeStrict bytes)
  ensure (get listing "record_type" == s "af-video-frames") "frames file is not an af-video-frames record"
  ensure (get listing "schema_version" == n 1) "unsupported frames schema version"
  let recordedVideo = str (get (get listing "video") "path")
      recordedDigest = str (get (get listing "video") "sha256")
  ensure (recordedVideo == video) ("frames file names a different video: " ++ recordedVideo)
  ensure (length recordedDigest == 64 && all isHex recordedDigest) "frames file carries no valid video digest"
  let hasher = fromMaybe "sha256sum" (flagValue args "--sha256sum")
  digest <- fileDigest hasher video
  ensure (digest == recordedDigest)
    ("the recording's bytes no longer match the frames listing: " ++ digest)
  tb <- maybe (failH "frames file clock time_base missing or unparsable")
              pure (parseTimeBase (str (get (get listing "clock") "time_base")))
  let frames = list (get listing "frames")
  ensure (length frames >= 2) "frames file has fewer than two frames"
  forM_ (zip [0 :: Int ..] frames) $ \(i, entry) ->
    ensure (num (get entry "index") == fromIntegral i) "frames file index column is not contiguous from zero"
  let ptsText = map (\entry -> str (get (get entry "pts") "ticks")) frames
  ensure (all (\t -> not (null t) && all isDigit t) ptsText) "frames file has a malformed pts column"
  let ticks = map read ptsText :: [Integer]
      frameCount = length frames
  -- The pts column is the evidence; the listing summary is checked against it.
  spacing <- maybe (failH "frames file has fewer than two frames") pure (summarizeSpacing tb ticks)
  let summary = get listing "spacing"
      duplicates = spacingDuplicatePts spacing
      nonMonotonic = spacingNonMonotonicPts spacing
  ensure (num (get summary "duplicate_pts") == fromIntegral duplicates
            && num (get summary "non_monotonic_pts") == fromIntegral nonMonotonic
            && num (get summary "gaps_above_1_5_median") == fromIntegral (spacingGapsAbove spacing)
            && str (get summary "min_ms") == formatMs (nanos (spacingMinMs spacing) Down)
            && str (get summary "max_ms") == formatMs (nanos (spacingMaxMs spacing) Up)
            && str (get summary "mean_ms") == formatMs (nanos (spacingMeanMs spacing) Nearest))
    "frames file spacing summary disagrees with its pts column"
  ensure (duplicates == 0 && nonMonotonic == 0)
    "recording has duplicate or nonmonotonic presentation timestamps; timing is withheld"
  let edgeAt flag = do
        selected <- intArg args flag "annotate"
        forM_ selected $ \k ->
          ensure (k < frameCount)
            (flag ++ " names frame " ++ show k ++ " outside 0.." ++ show (frameCount - 1))
        case selected of
          Nothing -> pure Nothing
          Just k -> case atMay ticks k of
            Nothing -> failH (flag ++ ": frames file has no ticks for index " ++ show k)
            Just t -> pure (Just (Edge k t))
      bracketAt name = do
        before <- edgeAt ("--" ++ name ++ "-before")
        after <- edgeAt ("--" ++ name ++ "-after")
        let ambiguous = hasFlag args ("--ambiguous-" ++ name)
        case (before, after, ambiguous) of
          (Nothing, Nothing, False) -> pure Nothing
          _ -> do
            case (before, after) of
              (Just beforeEdge, Just afterEdge) -> ensure (edgeIndex beforeEdge < edgeIndex afterEdge)
                (name ++ ": the last-without frame must precede the first-with frame")
              _ -> pure ()
            pure (Just (Bracket before after ambiguous))
  startEvent <- bracketAt "start"
  confirmationEvent <- bracketAt "confirmation"
  barrelEvent <- bracketAt "barrel"
  s1Event <- bracketAt "s1release"
  s2Event <- bracketAt "s2press"
  captureEvent <- bracketAt "capture"
  uncertainty <- msArg args "--sync-uncertainty-ms" "annotate"
  ensure (case (startEvent >>= bracketAfter, confirmationEvent >>= bracketAfter) of
            (Just startEdge, Just confirmationEdge) -> edgeIndex confirmationEdge > edgeIndex startEdge
            _ -> True)
    "the confirmation bracket precedes the start bracket"
  ensure (case (s2Event >>= bracketBefore, captureEvent >>= bracketAfter) of
            (Just pressEdge, Just captureEdge) -> edgeIndex captureEdge > edgeIndex pressEdge
            _ -> True)
    "the capture bracket precedes the full press"
  let latencyRaw = do
        startB <- startEvent
        confirmationB <- confirmationEvent
        latencyInterval tb startB confirmationB
      latency = case (latencyRaw, uncertainty) of
        (Just (lo, hi), Just u) -> Just (max 0 (lo - u), hi + u)
        (Just interval, Nothing) -> Just interval
        (Nothing, _) -> Nothing
      missingReason
        | isJust latency = Nothing
        | otherwise = Just (missingReasonFor startEvent confirmationEvent)
      barrelRaw = do
        barrelB <- barrelEvent
        confirmationB <- confirmationEvent
        motionRelation tb (fromMaybe 0 uncertainty) barrelB confirmationB
      captureRaw = do
        s2B <- s2Event
        captureB <- captureEvent
        captureWindow tb (fromMaybe 0 uncertainty) s2B captureB
      invalidReason = flagValue args "--invalid-reason"
      seen flag = if hasFlag args flag then b True else Null
      mkInterval method (lo, hi) = obj
        [ ("lower_ms", s (formatMs (nanos lo Down)))
        , ("upper_ms", s (formatMs (nanos hi Up)))
        , ("method", s method)
        , ("sync_uncertainty_ms", maybe Null (s . formatMs . flip nanos Nearest) uncertainty)
        , ("rounding", s "outward to 1e-6 ms")
        ]
      edgeDoc (Edge i t) = obj
        [ ("frame_index", n (fromIntegral i))
        , ("pts", obj [("clock_id", s "ext-video-pts"), ("ticks", s (show t))])
        ]
      firstPts = case ptsText of (p:_) -> p; [] -> ""
      lastPts = foldl' (\_ p -> p) "" ptsText
      bracketDoc bracket' = obj
        [ ("before", maybe Null edgeDoc (bracketBefore bracket'))
        , ("after", maybe Null edgeDoc (bracketAfter bracket'))
        , ("ambiguous", b (bracketAmbiguous bracket'))
        ]
      document = obj
        [ ("schema_version", n 1)
        , ("record_type", s "af-attempt-annotation")
        , ("session_id", s sessionId)
        , ("trial_id", s trialId)
        , ("pair_id", maybe Null s (flagValue args "--pair-id"))
        , ("scene_id", s sceneId)
        , ("start", s startKind)
        , ("video", obj [("path", s video), ("sha256", s recordedDigest)])
        , ("clock", obj
            [ ("clock_id", s "ext-video-pts")
            , ("source", s "external recording presentation timestamps, one continuous file")
            , ("time_base", s (show (tbNum tb) ++ "/" ++ show (tbDen tb)))
            , ("origin", s "container timeline origin; the first listed frame is not assumed to start at zero")
            , ("wrap_width", Null)
            , ("synchronization", s "start and confirmation brackets read from the same recording; no cross-recording subtraction")
            , ("uncertainty_ms", maybe Null (s . formatMs . flip nanos Nearest) uncertainty)
            ])
        , ("recording", obj
            [ ("frame_count", n (fromIntegral frameCount))
            , ("first_pts_ticks", s firstPts)
            , ("last_pts_ticks", s lastPts)
            , ("duplicate_pts", n (fromIntegral duplicates))
            , ("non_monotonic_pts", n (fromIntegral nonMonotonic))
            , ("spacing_min_ms", s (formatMs (nanos (spacingMinMs spacing) Down)))
            , ("spacing_max_ms", s (formatMs (nanos (spacingMaxMs spacing) Up)))
            , ("spacing_mean_ms", s (formatMs (nanos (spacingMeanMs spacing) Nearest)))
            , ("gaps_above_1_5_median", n (fromIntegral (spacingGapsAbove spacing)))
            , ("stream", get listing "stream")
            ])
        , ("events", obj
            [ ("start", maybe Null bracketDoc startEvent)
            , ("confirmation", maybe Null bracketDoc confirmationEvent)
            , ("barrel_motion_end", maybe Null bracketDoc barrelEvent)
            , ("s1_release", maybe Null bracketDoc s1Event)
            , ("s2_press", maybe Null bracketDoc s2Event)
            , ("capture", maybe Null bracketDoc captureEvent)
            ])
        , ("latency_ms", maybe Null (mkInterval latencyMethod) latency)
        , ("latency_missing_reason", maybe Null s missingReason)
        , ("barrel_relation", case barrelRaw of
            Nothing -> Null
            Just (relation, lo, hi) -> obj
              [ ("relation", s (relationName relation))
              , ("interval_ms", mkInterval barrelMethod (lo, hi))
              ])
        , ("capture_window", case captureRaw of
            Nothing -> Null
            Just (lo, hi, verdict) -> obj
              [ ("verdict", s verdict)
              , ("deadline_ms", s (formatMs (nanos 2000 Nearest)))
              , ("interval_ms", mkInterval captureMethod (lo, hi))
              ])
        , ("validity", obj
            [ ("valid", b (invalidReason == Nothing))
            , ("invalid_reason", maybe Null s invalidReason)
            ])
        , ("secondary_flags", obj
            [ ("hunting_observed", seen "--hunting")
            , ("focus_restart_observed", seen "--focus-restart")
            , ("wrong_plane_suspected", seen "--wrong-plane-suspected")
            ])
        , ("reference", obj
            [ ("roi", maybe Null s (flagValue args "--roi"))
            , ("target_plane", maybe Null s (flagValue args "--target-plane"))
            , ("sweep_id", maybe Null s (flagValue args "--sweep-id"))
            ])
        , ("reviewer_notes", maybe Null s (flagValue args "--notes"))
        ]
  writeOut (flagValue args "--out") document

latencyMethod, barrelMethod, captureMethod :: String
latencyMethod = "physical-trigger-proxy-to-displayed-confirmation latency: [max(0, confirmation.before - start.after), confirmation.after - start.before] milliseconds, widened by the declared synchronization and visibility uncertainty, rounded outward to 1e-6 ms. Frame rate bounds this sampling only; it does not bound internal decision or display latency"
barrelMethod = "signed confirmation-minus-motion-end interval [confirmation.before - motion.after, confirmation.after - motion.before] milliseconds, widened by the declared synchronization and visibility uncertainty, rounded outward to 1e-6 ms. The relation is reported only when the widened interval's sign is resolved"
captureMethod = "full-press-to-capture window [max(0, capture.before - press.after), capture.after - press.before] milliseconds, widened by the declared synchronization and visibility uncertainty, rounded outward to 1e-6 ms. The verdict is within when the widened upper bound is at most the 2 s deadline, after when the widened lower bound exceeds it, and unresolved when the interval spans the deadline"

relationName :: MotionRelation -> String
relationName RelationTrails = "trails"
relationName RelationOverlaps = "overlaps"
relationName RelationLeads = "leads"
relationName RelationUnresolved = "unresolved"

missingReasonFor :: Maybe Bracket -> Maybe Bracket -> String
missingReasonFor startEvent confirmationEvent
  | Nothing <- startEvent, Nothing <- confirmationEvent = "neither the start nor the confirmation bracket is recorded"
  | Nothing <- startEvent = "the start bracket is not recorded"
  | Nothing <- confirmationEvent = "the confirmation bracket is not recorded"
  | otherwise = "a bracketed edge is ambiguous, incomplete or out of order"
