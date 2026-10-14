{-# LANGUAGE OverloadedStrings, ScopedTypeVariables #-}
-- Behavioural tests for //af-session:af-session. Every assertion goes through
-- the command line against fixtures built by the pinned ffmpeg; no module
-- internals are imported.
module Main (main) where

import Control.Exception (IOException, try)
import Control.Monad (forM_, void)
import Data.Aeson (Value (..))
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as BC
import Data.Ratio ((%))
import FujiAF.Host
import FujiAF.JSON (encodeCanonical)
import GHC.Clock (getMonotonicTimeNSec)
import System.Directory (doesFileExist)
import System.Environment (getArgs)
import System.Exit (ExitCode (..))
import System.FilePath ((</>))

check :: Bool -> String -> IO ()
check = ensure

reject :: IO a -> IO ()
reject action = do
  result <- try (void action)
  case result of
    Left (_ :: IOException) -> pure ()
    Right _ -> failH "unsupported input accepted"

runTool :: FilePath -> [String] -> IO (Int, BC.ByteString, BC.ByteString)
runTool tool args = do
  (timedOut, code, out, err) <- runBounded 120000000 (32 * 1024 * 1024) tool args B.empty
  check (not timedOut) ("tool timed out: " ++ unwords args)
  pure (case code of ExitSuccess -> 0; ExitFailure k -> k, out, err)

timedRun :: FilePath -> [String] -> IO (Int, Integer)
timedRun tool args = do
  start <- getMonotonicTimeNSec
  (rc, _, _) <- runTool tool args
  end <- getMonotonicTimeNSec
  pure (rc, fromIntegral (end - start))

toolOK :: FilePath -> [String] -> IO ()
toolOK tool args = do
  (rc, _, err) <- runTool tool args
  check (rc == 0) ("tool failed: " ++ unwords args ++ " :: " ++ BC.unpack (B.take 400 err))

toolFails :: FilePath -> [String] -> IO ()
toolFails tool args = do
  (rc, _, _) <- runTool tool args
  check (rc /= 0) ("tool accepted: " ++ unwords args)

-- | One frame's centre pixel as rgb24, decoded by the pinned ffmpeg.
centrePixel :: FilePath -> FilePath -> IO (Int, Int, Int)
centrePixel ffmpeg png = do
  (timedOut, code, out, err) <- runBounded 60000000 4096 ffmpeg
    [ "-v", "error", "-i", png
    , "-vf", "crop=1:1:32:24,format=rgb24"
    , "-f", "rawvideo", "-pix_fmt", "rgb24", "-"
    ] B.empty
  check (not timedOut && code == ExitSuccess) ("pixel read failed: " ++ png ++ " " ++ BC.unpack (B.take 200 err))
  case B.unpack (B.take 3 out) of
    [red, green, blue] -> pure (fromIntegral red, fromIntegral green, fromIntegral blue)
    _ -> failH "pixel read returned no rgb triplet"

dominant :: String -> (Int, Int, Int) -> Bool
dominant name (red, green, blue) = case name of
  "blue" -> blue > 200 && red < 64 && green < 64
  "red" -> red > 200 && green < 64 && blue < 64
  "green" -> green > 100 && red < 64 && blue < 64
  _ -> False

-- | One ffmpeg command, refusing a timeout or a nonzero exit.
runFfmpeg :: FilePath -> [String] -> IO ()
runFfmpeg ffmpeg arguments = do
  (timedOut, code, _, err) <- runBounded 120000000 (4 * 1024 * 1024) ffmpeg arguments B.empty
  check (not timedOut && code == ExitSuccess) ("ffmpeg failed: " ++ BC.unpack (B.take 400 err))

-- | The fixture's decoded pixel format, so a chroma-subsampled crop test cannot
-- silently run on a format without subsampling.
imagePixelFormat :: FilePath -> FilePath -> IO String
imagePixelFormat ffprobe image = do
  (timedOut, code, out, err) <- runBounded 60000000 (1024 * 1024) ffprobe
    ["-v", "error", "-select_streams", "v:0", "-show_entries", "stream=pix_fmt", "-of", "json", image] B.empty
  check (not timedOut && code == ExitSuccess) ("ffprobe pixel format failed: " ++ BC.unpack (B.take 200 err))
  value <- jsonRead out
  case list (get value "streams") of
    (stream:_) -> pure (str (get stream "pix_fmt"))
    [] -> failH "ffprobe reported no image stream"

-- | The documented metric recomputed here from raw ffmpeg bytes of the
-- requested crop, so a rounded or shifted crop changes the expectation.
expectedAcutance :: FilePath -> FilePath -> Int -> Int -> Int -> Int -> IO String
expectedAcutance ffmpeg image x y w h = do
  (timedOut, code, out, err) <- runBounded 120000000 (64 * 1024 * 1024) ffmpeg
    [ "-v", "error", "-i", image
    , "-vf", "format=gray,crop=" ++ show w ++ ":" ++ show h ++ ":" ++ show x ++ ":" ++ show y
    , "-f", "rawvideo", "-pix_fmt", "gray", "-"
    ] B.empty
  check (not timedOut && code == ExitSuccess) ("expected-crop extraction failed: " ++ BC.unpack (B.take 200 err))
  check (B.length out == w * h) "expected-crop extraction has the wrong size"
  let byte index = fromIntegral (B.index out index) :: Integer
      interior = [yy * w + xx | yy <- [1 .. h - 2], xx <- [1 .. w - 2]]
      total = foldr (\index acc -> acc + abs (byte (index + 1) - byte (index - 1)) + abs (byte (index + w) - byte (index - w))) 0 interior
      value = fromIntegral total % (2 * fromIntegral (length interior)) :: Rational
      micros = ceiling (value * 1000000) :: Integer
      (whole, rest) = micros `divMod` 1000000
  pure (show whole ++ "." ++ replicate (6 - length (show rest)) '0' ++ show rest)

-- | 25 frames at 25 fps: blue 0-14, red 15-20, green 21-24.
makeVideo :: FilePath -> FilePath -> IO ()
makeVideo ffmpeg video = runFfmpeg ffmpeg
  [ "-v", "error", "-y"
  , "-f", "lavfi", "-i", "color=c=blue:s=64x48:r=25:d=0.60"
  , "-f", "lavfi", "-i", "color=c=red:s=64x48:r=25:d=0.24"
  , "-f", "lavfi", "-i", "color=c=green:s=64x48:r=25:d=0.16"
  , "-filter_complex", "[0:v][1:v][2:v]concat=n=3:v=1[out]"
  , "-map", "[out]"
  , "-c:v", "libx264", "-pix_fmt", "yuv420p"
  , "-fps_mode", "passthrough"
  , video
  ]

-- | One still image from a lavfi source and optional filters.
makeStill :: FilePath -> [String] -> FilePath -> IO ()
makeStill ffmpeg inputs output = runFfmpeg ffmpeg (["-v", "error", "-y"] ++ inputs ++ ["-frames:v", "1", output])

main :: IO ()
main = do
  [tool, checker, ffmpeg, ffprobe] <- getArgs
  withTemporary "af-session-tests" $ \root -> do
    let video = root </> "synthetic.mp4"
        framesPath = root </> "frames.json"
    makeVideo ffmpeg video

    -- frames: timestamps, spacing, digest
    toolOK tool ["frames", "--video", video, "--ffprobe", ffprobe, "--out", framesPath]
    listing <- readRegular (64 * 1024 * 1024) framesPath >>= jsonRead
    check (get listing "record_type" == s "af-video-frames") "frames record type"
    check (get (get listing "clock") "time_base" == s "1/12800") "frames time base"
    check (get (get listing "stream") "codec_name" == s "h264"
             && get (get listing "stream") "width" == n 64
             && get (get listing "stream") "height" == n 48) "frames stream"
    let frameList = list (get listing "frames")
        frameMs i = case drop i frameList of
          (entry:_) -> str (get entry "ms")
          [] -> ""
    check (length frameList == 25) "frame count"
    check (frameMs 0 == "0.000000" && frameMs 14 == "560.000000" && frameMs 15 == "600.000000" && frameMs 24 == "960.000000") "frame timestamps"
    check (get (get listing "spacing") "min_ms" == s "40.000000"
             && get (get listing "spacing") "max_ms" == s "40.000000"
             && get (get listing "spacing") "mean_ms" == s "40.000000") "spacing strings"
    check (get (get listing "spacing") "duplicate_pts" == n 0
             && get (get listing "spacing") "non_monotonic_pts" == n 0
             && get (get listing "spacing") "gaps_above_1_5_median" == n 0) "spacing counters"
    videoBytes <- readRegular (8 * 1024 * 1024) video
    digest <- sha256 videoBytes
    check (get (get listing "video") "sha256" == s digest) "video digest"
    check (get (get listing "video") "path" == s video) "video path"
    toolFails tool ["frames", "--video", framesPath, "--ffprobe", ffprobe]
    toolFails tool ["frames", "--video", video, "--video", video, "--ffprobe", ffprobe]

    -- extract: index-to-content correspondence and output directories
    let exDir = root </> "frames-out" </> "nested"
    toolOK tool ["extract", "--video", video, "--frames", "14-15,20-21", "--out", exDir, "--ffmpeg", ffmpeg]
    forM_ [(14 :: Int, "blue"), (15, "red"), (20, "red"), (21, "green")] $ \(index, name) -> do
      let text = show index
          png = exDir </> ("frame-" ++ replicate (6 - length text) '0' ++ text ++ ".png")
      exists <- doesFileExist png
      check exists ("missing extraction " ++ png)
      pixel <- centrePixel ffmpeg png
      check (dominant name pixel) ("frame " ++ show index ++ " is not " ++ name ++ ": " ++ show pixel)
    toolFails tool ["extract", "--video", video, "--frames", "100-101", "--out", exDir, "--ffmpeg", ffmpeg]
    toolFails tool ["extract", "--video", video, "--frames", "15-14", "--out", exDir, "--ffmpeg", ffmpeg]

    -- annotate: interval, uncertainty, ambiguity, flags, refusals
    let annotateArgs videoPath framesFile extra =
          [ "annotate", "--video", videoPath, "--frames", framesFile
          , "--session-id", "TEST-SESSION", "--trial-id", "T1"
          , "--pair-id", "E01:P01:near:01", "--scene-id", "P01", "--start", "near" ] ++ extra
        standard = ["--start-before", "14", "--start-after", "15"
                   , "--confirmation-before", "20", "--confirmation-after", "21"]
        annotateNamed name extra = do
          let out = root </> (name ++ ".json")
          toolOK tool (annotateArgs video framesPath (extra ++ ["--out", out]))
          readRegular (8 * 1024 * 1024) out >>= jsonRead
        annotate name extra = annotateNamed name (standard ++ extra)
        annotateLast name finalFlag = do
          let out = root </> (name ++ ".json")
          toolOK tool (annotateArgs video framesPath (standard ++ ["--out", out, finalFlag]))
          readRegular (8 * 1024 * 1024) out >>= jsonRead
    annotation <- annotate "ann-basic" []
    check (get (get annotation "latency_ms") "lower_ms" == s "200.000000") "annotation lower"
    check (get (get annotation "latency_ms") "upper_ms" == s "280.000000") "annotation upper"
    check (get annotation "latency_missing_reason" == Null) "annotation missing reason"
    check (get (get (get (get annotation "events") "start") "after") "pts"
             == obj [("clock_id", s "ext-video-pts"), ("ticks", s "7680")]) "start edge ticks"
    check (get (get annotation "recording") "frame_count" == n 25) "annotation frame count"
    check (get (get annotation "recording") "spacing_min_ms" == s "40.000000") "annotation spacing"
    check (get (get annotation "validity") "valid" == Bool True) "annotation validity"
    check (get (get annotation "secondary_flags") "hunting_observed" == Null) "unset flag"
    schemaCheck checker "schemas/af-session-annotation.schema.json" [annotation]

    widened <- annotate "ann-widened" ["--sync-uncertainty-ms", "3"]
    check (get (get widened "latency_ms") "lower_ms" == s "197.000000") "widened lower"
    check (get (get widened "latency_ms") "upper_ms" == s "283.000000") "widened upper"
    check (get (get widened "clock") "uncertainty_ms" == s "3.000000") "declared uncertainty"
    schemaCheck checker "schemas/af-session-annotation.schema.json" [widened]

    -- barrel: bracketed order, signed interval, uncertainty-aware relation
    trailing <- annotate "ann-trailing" ["--barrel-before", "17", "--barrel-after", "18"]
    check (get (get trailing "barrel_relation") "relation" == s "trails") "trailing relation"
    check (get (get (get trailing "barrel_relation") "interval_ms") "lower_ms" == s "80.000000") "trailing lower"
    check (get (get (get trailing "barrel_relation") "interval_ms") "upper_ms" == s "160.000000") "trailing upper"
    schemaCheck checker "schemas/af-session-annotation.schema.json" [trailing]
    unresolved <- annotate "ann-unresolved"
      ["--barrel-before", "17", "--barrel-after", "18", "--sync-uncertainty-ms", "100"]
    check (get (get unresolved "barrel_relation") "relation" == s "unresolved") "uncertainty did not withhold the relation"
    check (get (get (get unresolved "barrel_relation") "interval_ms") "lower_ms" == s "-20.000000") "unresolved lower"
    check (get (get (get unresolved "barrel_relation") "interval_ms") "upper_ms" == s "260.000000") "unresolved upper"
    schemaCheck checker "schemas/af-session-annotation.schema.json" [unresolved]
    overlapping <- annotate "ann-overlapping" ["--barrel-before", "20", "--barrel-after", "21"]
    check (get (get overlapping "barrel_relation") "relation" == s "overlaps") "overlapping relation"
    check (get (get (get overlapping "barrel_relation") "interval_ms") "lower_ms" == s "-40.000000") "overlapping lower"
    check (get (get (get overlapping "barrel_relation") "interval_ms") "upper_ms" == s "40.000000") "overlapping upper"
    leading <- annotate "ann-leading" ["--barrel-before", "22", "--barrel-after", "23"]
    check (get (get leading "barrel_relation") "relation" == s "leads") "leading relation"
    check (get (get (get leading "barrel_relation") "interval_ms") "lower_ms" == s "-120.000000") "leading lower"
    check (get (get (get leading "barrel_relation") "interval_ms") "upper_ms" == s "-40.000000") "leading upper"
    leadingUncertain <- annotate "ann-leading-uncertain"
      ["--barrel-before", "22", "--barrel-after", "23", "--sync-uncertainty-ms", "200"]
    check (get (get leadingUncertain "barrel_relation") "relation" == s "unresolved") "leading uncertainty not withheld"
    schemaCheck checker "schemas/af-session-annotation.schema.json" [leadingUncertain]

    missing <- annotateNamed "ann-missing" (["--start-before", "14", "--start-after", "15"]
                                            ++ ["--confirmation-before", "20"])
    check (get missing "latency_missing_reason" /= Null) "missing timing not reported"
    check (get missing "latency_ms" == Null) "missing timing still timed"
    schemaCheck checker "schemas/af-session-annotation.schema.json" [missing]
    ambiguous <- annotate "ann-ambiguous" ["--ambiguous-confirmation"]
    check (get (get (get ambiguous "events") "confirmation") "ambiguous" == Bool True) "ambiguity flag"
    check (get ambiguous "latency_ms" == Null) "ambiguous timing kept"
    check (get (get ambiguous "validity") "valid" == Bool True) "ambiguity changed validity"
    schemaCheck checker "schemas/af-session-annotation.schema.json" [ambiguous]
    cancelled <- annotate "ann-cancelled"
      [ "--s1release-before", "16", "--s1release-after", "17"
      , "--s2press-before", "18", "--s2press-after", "19"
      , "--capture-before", "19", "--capture-after", "20"
      , "--focus-restart", "--invalid-reason", "operator stopped the attempt" ]
    check (get (get cancelled "validity") "valid" == Bool False) "invalid attempt marked valid"
    check (get (get cancelled "validity") "invalid_reason" == s "operator stopped the attempt") "invalid reason"
    check (get (get cancelled "secondary_flags") "focus_restart_observed" == Bool True) "focus restart flag"
    check (get (get (get (get cancelled "events") "capture") "after") "pts"
             == obj [("clock_id", s "ext-video-pts"), ("ticks", s "10240")]) "capture edge ticks"
    check (get (get (get cancelled "events") "s2_press") "after" == get (get (get cancelled "events") "capture") "before")
      "S2 and capture brackets disagree"
    check (get (get cancelled "capture_window") "verdict" == s "within") "capture window verdict"
    check (get (get (get cancelled "capture_window") "interval_ms") "lower_ms" == s "0.000000") "capture window lower"
    check (get (get (get cancelled "capture_window") "interval_ms") "upper_ms" == s "80.000000") "capture window upper"
    schemaCheck checker "schemas/af-session-annotation.schema.json" [cancelled]

    -- full-press-to-capture window against the 2 s deadline, on a 3.2 s fixture
    let longVideo = root </> "long.mp4"
        longFrames = root </> "long.frames.json"
    runFfmpeg ffmpeg [ "-v", "error", "-y", "-f", "lavfi", "-i", "color=c=blue:s=64x48:r=25:d=3.2"
                     , "-c:v", "libx264", "-pix_fmt", "yuv420p", "-fps_mode", "passthrough", longVideo ]
    toolOK tool ["frames", "--video", longVideo, "--ffprobe", ffprobe, "--out", longFrames]
    listingLong <- readRegular (64 * 1024 * 1024) longFrames >>= jsonRead
    check (length (list (get listingLong "frames")) == 80) "long fixture frame count"
    let longAnnotate name extra = do
          let out = root </> (name ++ ".json")
          toolOK tool (annotateArgs longVideo longFrames (extra ++ ["--out", out]))
          readRegular (8 * 1024 * 1024) out >>= jsonRead
        windowOf record = get record "capture_window"
    windowWithin <- longAnnotate "ann-window-within"
      ["--s2press-before", "10", "--s2press-after", "11", "--capture-before", "12", "--capture-after", "13"]
    check (get (windowOf windowWithin) "verdict" == s "within") "within verdict"
    check (get (get (windowOf windowWithin) "interval_ms") "lower_ms" == s "40.000000") "within lower"
    check (get (get (windowOf windowWithin) "interval_ms") "upper_ms" == s "120.000000") "within upper"
    check (get (windowOf windowWithin) "deadline_ms" == s "2000.000000") "window deadline"
    schemaCheck checker "schemas/af-session-annotation.schema.json" [windowWithin]
    windowAfter <- longAnnotate "ann-window-after"
      ["--s2press-before", "10", "--s2press-after", "11", "--capture-before", "70", "--capture-after", "71"]
    check (get (windowOf windowAfter) "verdict" == s "after") "after verdict"
    windowSpanning <- longAnnotate "ann-window-spanning"
      ["--s2press-before", "10", "--s2press-after", "11", "--capture-before", "60", "--capture-after", "61"]
    check (get (windowOf windowSpanning) "verdict" == s "unresolved") "spanning verdict"
    check (get (get (windowOf windowSpanning) "interval_ms") "lower_ms" == s "1960.000000") "spanning lower"
    check (get (get (windowOf windowSpanning) "interval_ms") "upper_ms" == s "2040.000000") "spanning upper"
    windowWidened <- longAnnotate "ann-window-widened"
      ["--s2press-before", "10", "--s2press-after", "11", "--capture-before", "49", "--capture-after", "50"
      , "--sync-uncertainty-ms", "500"]
    check (get (windowOf windowWidened) "verdict" == s "unresolved") "uncertainty did not widen the window"
    windowAbsent <- longAnnotate "ann-window-absent" ["--s2press-before", "10", "--s2press-after", "11"]
    check (windowOf windowAbsent == Null) "window emitted without a capture bracket"
    toolFails tool (annotateArgs longVideo longFrames
      ["--s2press-before", "10", "--s2press-after", "11", "--capture-before", "0", "--capture-after", "1"])
    toolFails tool (annotateArgs longVideo longFrames
      ["--s2press-before", "10", "--s2press-after", "11", "--capture-before", "0", "--capture-after", "1"
      , "--sync-uncertainty-ms", "500"])
    overlappingWindow <- longAnnotate "ann-window-overlapping"
      ["--s2press-before", "10", "--s2press-after", "11", "--capture-before", "9", "--capture-after", "11"]
    check (get (windowOf overlappingWindow) "verdict" == s "unresolved") "overlapping brackets treated as exact"
    check (get (get (windowOf overlappingWindow) "interval_ms") "lower_ms" == s "0.000000") "overlapping lower"
    check (get (get (windowOf overlappingWindow) "interval_ms") "upper_ms" == s "40.000000") "overlapping upper"
    sameFrameWindow <- longAnnotate "ann-window-sameframe"
      ["--s2press-before", "10", "--s2press-after", "11", "--capture-before", "10", "--capture-after", "11"]
    check (get (windowOf sameFrameWindow) "verdict" == s "unresolved") "same-frame brackets treated as exact"
    check (get (get (windowOf sameFrameWindow) "interval_ms") "lower_ms" == s "0.000000") "same-frame lower"
    schemaCheck checker "schemas/af-session-annotation.schema.json" [overlappingWindow, sameFrameWindow]

    -- boolean flags must survive being the last argument
    forM_ ["--hunting", "--focus-restart", "--wrong-plane-suspected"] $ \flag -> do
      flagged <- annotateLast ("ann-flag-" ++ drop 2 flag) flag
      let key = case flag of
            "--hunting" -> "hunting_observed"
            "--focus-restart" -> "focus_restart_observed"
            _ -> "wrong_plane_suspected"
      check (get (get flagged "secondary_flags") key == Bool True) (flag ++ " lost at the end of argv")

    -- nested output directory creation for JSON writers
    let nestedAnn = root </> "ann-nested" </> "deep" </> "ann.json"
    toolOK tool (annotateArgs video framesPath (standard ++ ["--out", nestedAnn]))
    nestedExists <- doesFileExist nestedAnn
    check nestedExists "annotate did not create the output directory"

    -- refusals
    toolFails tool (annotateArgs video framesPath (["--start-before", "16", "--start-after", "15"
                                                  , "--confirmation-before", "20", "--confirmation-after", "21"]))
    toolFails tool (annotateArgs video framesPath (["--start-before", "0", "--start-after", "25"
                                                  , "--confirmation-before", "20", "--confirmation-after", "21"]))
    toolFails tool (annotateArgs video framesPath (["--start-before", "20", "--start-after", "21"
                                                  , "--confirmation-before", "14", "--confirmation-after", "15"]))
    toolFails tool (annotateArgs video framesPath (["--start-before", "14", "--start-after", "14"
                                                  , "--confirmation-before", "20", "--confirmation-after", "21"]))
    toolFails tool (annotateArgs (root </> "other.mp4") framesPath standard)
    toolFails tool (annotateArgs video framesPath (standard ++ ["--sync-uncertainty-ms", "-1"]))
    let duplicated = set "spacing" (set "duplicate_pts" (n 1) (get listing "spacing")) listing
        badMinimum = set "spacing" (set "min_ms" (s "41.000000") (get listing "spacing")) listing
        badClock = set "clock" (set "time_base" (s "0/1") (get listing "clock")) listing
        badDigest = set "video" (set "sha256" (s (replicate 64 '0')) (get listing "video")) listing
    B.writeFile (root </> "dup.json") (encodeCanonical duplicated <> "\n")
    B.writeFile (root </> "min.json") (encodeCanonical badMinimum <> "\n")
    B.writeFile (root </> "clock.json") (encodeCanonical badClock <> "\n")
    B.writeFile (root </> "digest.json") (encodeCanonical badDigest <> "\n")
    toolFails tool (annotateArgs video (root </> "dup.json") standard)
    toolFails tool (annotateArgs video (root </> "min.json") standard)
    toolFails tool (annotateArgs video (root </> "clock.json") standard)
    toolFails tool (annotateArgs video (root </> "digest.json") standard)
    -- replaced video bytes at the same pathname must be refused by the re-hash
    originalBytes <- readRegular (8 * 1024 * 1024) video
    makeStill ffmpeg ["-f", "lavfi", "-i", "color=c=black:s=64x48"] video
    toolFails tool (annotateArgs video framesPath standard)
    B.writeFile video originalBytes
    annotationAfterRestore <- annotate "ann-restored" []
    check (get (get annotationAfterRestore "latency_ms") "lower_ms" == s "200.000000") "restored video refused"

    -- measure: metric, saturation, exact JPEG crop, bounded runtime
    let flatPng = root </> "flat.png"
        whitePng = root </> "white.png"
        edgePng = root </> "edge.png"
        lineJpeg = root </> "line.jpg"
        bigPng = root </> "big.png"
        measureImage name extra = do
          let out = root </> name </> "measure.json"
          toolOK tool (["measure"] ++ extra ++ ["--ffmpeg", ffmpeg, "--ffprobe", ffprobe, "--out", out])
          readRegular (8 * 1024 * 1024) out >>= jsonRead
    makeStill ffmpeg ["-f", "lavfi", "-i", "color=c=gray:s=64x48"] flatPng
    makeStill ffmpeg ["-f", "lavfi", "-i", "color=c=white:s=64x48"] whitePng
    makeStill ffmpeg ["-f", "lavfi", "-i", "color=c=black:s=64x48"
                     , "-vf", "drawbox=x=32:y=0:w=16:h=48:color=white:t=fill"] edgePng
    makeStill ffmpeg ["-f", "lavfi", "-i", "color=c=black:s=64x48"
                     , "-vf", "drawbox=x=9:y=0:w=1:h=48:color=white:t=fill", "-q:v", "1", "-pix_fmt", "yuvj420p"] lineJpeg
    lineFormat <- imagePixelFormat ffprobe lineJpeg
    check (take 6 lineFormat == "yuvj42") ("cropping fixture is not chroma subsampled: " ++ lineFormat)
    makeStill ffmpeg ["-f", "lavfi", "-i", "color=c=black:s=1000x700"
                     , "-vf", "drawbox=x=500:y=0:w=1:h=700:color=white:t=fill"] bigPng
    flat <- measureImage "meas-flat" ["--image", flatPng, "--roi", "0:0:64:48"]
    check (get flat "acutance" == s "0.000000") "flat acutance"
    check (get flat "saturated_fraction" == s "0.000000") "flat saturation"
    check (get flat "metric" == s "mean-central-gradient-8bit") "metric name"
    check (get flat "pixels" == n 3072) "roi pixels"
    white <- measureImage "meas-white" ["--image", whitePng, "--roi", "0:0:64:48"]
    check (get white "saturated_fraction" == s "1.000000") "white saturation"
    edge <- measureImage "meas-edge" ["--image", edgePng, "--roi", "0:0:64:48"]
    let edgeValue = read (str (get edge "acutance")) :: Double
    check (edgeValue > 5 && edgeValue < 100) "edge acutance out of range"
    cropped <- measureImage "meas-cropped" ["--image", edgePng, "--roi", "0:0:16:48"]
    check (get cropped "acutance" == s "0.000000") "crop outside the edge"
    normalized <- measureImage "meas-normalized" ["--image", edgePng, "--roi", "0:0:64:48"
                                                 , "--sweep-max", str (get edge "acutance")]
    check (get normalized "normalized" == s "1.000000") "normalization"
    -- Odd origin and odd size on a chroma-subsampled JPEG: the requested crop's
    -- value must equal the independently extracted crop, and the line one pixel
    -- later must read differently because it leaves the ROI.
    expectedLine <- expectedAcutance ffmpeg lineJpeg 9 9 11 11
    lineOnEdge <- measureImage "meas-line" ["--image", lineJpeg, "--roi", "9:9:11:11"]
    lineBeside <- measureImage "meas-line-beside" ["--image", lineJpeg, "--roi", "10:10:11:11"]
    check (get lineOnEdge "acutance" == s expectedLine) "odd-origin crop differs from the expected crop"
    let onEdgeValue = read (str (get lineOnEdge "acutance")) :: Double
        besideValue = read (str (get lineBeside "acutance")) :: Double
    check (onEdgeValue >= 10) ("line on the ROI column read too low: " ++ show onEdgeValue)
    check (besideValue <= 5) ("line beside the ROI read too high: " ++ show besideValue)
    -- Documented ROI size with an analytically known acutance, bounded in time.
    let bigOut = root </> "meas-big" </> "measure.json"
        bigArgs = ["measure", "--image", bigPng, "--roi", "100:100:900:600"
                  , "--ffmpeg", ffmpeg, "--ffprobe", ffprobe, "--out", bigOut]
    (bigRc, bigNanos) <- timedRun tool bigArgs
    check (bigRc == 0) "large-ROI measure failed"
    check (bigNanos < 20000000000) ("large-ROI measure took " ++ show (bigNanos `div` 1000000) ++ " ms")
    big <- readRegular (8 * 1024 * 1024) bigOut >>= jsonRead
    -- One white column inside a 900x600 ROI: two interior columns carry a full
    -- 255 step over 598 rows, so the mean is 255 / 898.
    check (get big "acutance" == s "0.283965") "large-ROI acutance"
    check (get big "saturated_fraction" == s "0.001111") "large-ROI saturation"
    check (get big "pixels" == n (900 * 600)) "large-ROI pixels"
    toolFails tool ["measure", "--image", edgePng, "--roi", "60:0:16:48", "--ffmpeg", ffmpeg, "--ffprobe", ffprobe]
    toolFails tool ["measure", "--image", edgePng, "--roi", "0:0:2:2", "--ffmpeg", ffmpeg, "--ffprobe", ffprobe]
    toolFails tool ["measure", "--image", framesPath, "--roi", "0:0:64:48", "--ffmpeg", ffmpeg, "--ffprobe", ffprobe]

    reject (schemaCheck checker "schemas/af-session-annotation.schema.json"
              [set "latency_ms" (obj [("lower_ms", s "not-a-time")]) annotation])
    reject (schemaCheck checker "schemas/af-session-annotation.schema.json"
              [set "reviewer_notes" (n 7) annotation])
    reject (schemaCheck checker "schemas/af-session-annotation.schema.json"
              [set "clock" (obj []) annotation])
    reject (schemaCheck checker "schemas/af-session-annotation.schema.json"
              [set "barrel_relation" (obj [("relation", s "settled"), ("interval_ms", Null)]) annotation])
    toolOK tool ["--help"]
    putStrLn "PASS: extraction, annotation, acutance and provenance contracts verified"
