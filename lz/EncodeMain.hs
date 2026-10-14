-- Encode a decoded image into a compressed record (FujiAF.Lz.Encode) and
-- report the record against the shipped-record invariants
-- (FujiAF.Lz.Format.verifyRecord) plus the encoder-defect scans the record
-- review asks for: F1 (the zero-length literal-run geometry and its symptom
-- classes, including where the pre-fix encoder would have emitted it), F2
-- (total_len landing on a feed-page multiple) and F3 (record size against the
-- flash window). The encoder refuses the F1/F2/F3 classes itself, so a refusal
-- is the finding; a clean record is shipped only through rebuild-update
-- replace-record4.
module Main where

import Control.Monad (forM_, unless)
import Data.Bits ((.&.), (.|.), shiftL, shiftR)
import qualified Data.ByteString as B
import qualified FujiAF.Host as H
import qualified FujiAF.Lz.Encode as Encode
import qualified FujiAF.Lz.Format as Format
import Data.List (tails)
import Data.Word (Word32)
import System.Environment (getArgs)
import System.Exit (exitFailure)

main :: IO ()
main = do
  arguments <- getArgs
  (window, imagePath, recordPath) <- case arguments of
    [windowText, image, record] -> do
      value <- hexValue windowText
      pure (value, image, record)
    _ -> H.failH "usage: lz-encode WINDOW_HEX IMAGE RECORD"
  image <- H.readRegular (16 * 1024 * 1024) imagePath
  record <- case Encode.encodeRecord window image of
    Left Encode.EmptyImage -> H.failH (imagePath ++ ": empty image")
    Left Encode.ImageTooLarge -> H.failH (imagePath ++ ": image too large")
    Left Encode.InvalidTokenization -> H.failH (imagePath ++ ": encoder REFUSES: invalid tokenization (F1 class)")
    Left Encode.FeedAlignedTotalLen -> H.failH (imagePath ++ ": encoder REFUSES: total_len lands on a feed-page multiple (F2)")
    Left Encode.WindowExceeded -> H.failH (imagePath ++ ": encoder REFUSES: record exceeds the flash window (F3)")
    Right bytes -> pure bytes
  imageDigest <- H.sha256 image
  recordDigest <- H.sha256 record
  putStrLn (imagePath ++ ": " ++ show (B.length image) ++ " bytes, sha256 " ++ imageDigest)
  putStrLn ("record: " ++ show (B.length record) ++ " bytes, sha256 " ++ recordDigest)
  header <- maybe (H.failH "record: unparseable header") pure (Format.parseHeader record)
  let chunk = fromIntegral (Format.headerChunk header) :: Int
      totalLen = fromIntegral (Format.headerTotalLen header) :: Int
      stream = B.drop Format.headerSize record
      tokens = walkTokens stream
      recordOffset s = Format.headerSize + s
      literalRuns = [(recordOffset o, fromIntegral len) | (o, Format.LiteralRun len _) <- tokens] :: [(Int, Int)]
      matches =
        [ (recordOffset o, fromIntegral (Format.distanceValue d), fromIntegral (Format.matchLenValue l))
        | (o, Format.Match d l) <- tokens
        ] ::
          [(Int, Int, Int)]
      bounds = Format.pageBoundaries header
      endsAt b = any (\(o, t) -> recordOffset o + Format.tokenSize t == b) tokens
      zeroRuns = [o | (o, len) <- literalRuns, len == 0]
      capAtRoom129 =
        [ o
        | (o, len) <- literalRuns
        , len == 127
        , let r = chunk - (o `mod` chunk)
        , r >= 129
        , (r - 129) `mod` 128 == 0
        ]
      room1Starts = [recordOffset o | (o, _) <- tokens, chunk - (recordOffset o `mod` chunk) == 1]
      crossing =
        [ recordOffset o
        | (o, t) <- tokens
        , let e = recordOffset o + Format.tokenSize t
        , any (\b -> recordOffset o < b && b < e) bounds
        ]
      -- The fixed encoder splits a cap run at page room 129 into a 126-byte
      -- run plus a one-byte run closing the page. The pre-fix encoder emitted
      -- the full 127-run there and then a zero-length run at room 1, so these
      -- split positions mark where this stream's input carries the
      -- defect geometry.
      preFixGeometry =
        [ recordOffset o
        | (o, Format.LiteralRun 126 _) : next <- tails tokens
        , chunk - (recordOffset o `mod` chunk) == 129
        , (o2, Format.LiteralRun 1 _) <- take 1 next
        , o2 == o + 127
        ]
  putStrLn
    ( "header: out_size 0x"
        ++ showHexInt (fromIntegral (Format.headerOutSize header))
        ++ ", total_len 0x"
        ++ showHexInt totalLen
        ++ ", pages 0x"
        ++ showHexInt (fromIntegral (Format.headerPages header))
        ++ ", chunk 0x"
        ++ showHexInt chunk
        ++ ", mode "
        ++ show (Format.headerMode header)
    )
  putStrLn
    ( "tokens: "
        ++ show (length tokens)
        ++ " total, "
        ++ show (length literalRuns)
        ++ " literal runs ("
        ++ show (sum (map snd literalRuns))
        ++ " bytes, longest "
        ++ show (maximum (0 : map snd literalRuns))
        ++ ", at cap 127: "
        ++ show (length [() | (_, len) <- literalRuns, len == 127])
        ++ "), "
        ++ show (length matches)
        ++ " matches (dist "
        ++ show (minimum (2047 : [d | (_, d, _) <- matches]))
        ++ ".."
        ++ show (maximum (0 : [d | (_, d, _) <- matches]))
        ++ ", len "
        ++ show (minimum (15 : [l | (_, _, l) <- matches]))
        ++ ".."
        ++ show (maximum (0 : [l | (_, _, l) <- matches]))
        ++ ", len<3: "
        ++ show (length [() | (_, _, l) <- matches, l < 3])
        ++ ")"
    )
  putStrLn
    ( "feed boundaries: "
        ++ show (length bounds)
        ++ " interior, token ends at every one: "
        ++ (if all endsAt bounds then "yes" else "NO")
    )
  report "F1 pre-fix geometry: 126-run at page room 129 closed by a 1-run, record offsets" preFixGeometry
  report "F1 zero-length literal runs (defect emissions) at record offsets" zeroRuns
  report "F1 trigger residue: 127-cap run starts at page room 129+128k, record offsets" capAtRoom129
  report "F1 symptom: token starts at page room 1, record offsets" room1Starts
  report "F1 aftermath: tokens crossing a feed boundary, record offsets" crossing
  putStrLn
    ( "F2 total_len 0x"
        ++ showHexInt totalLen
        ++ " mod 0x4000 = 0x"
        ++ showHexInt (totalLen `mod` chunk)
        ++ (if totalLen `mod` chunk == 0 then " -> FLAG: feed-page multiple, unobserved engine geometry" else "")
    )
  putStrLn
    ( "F3 record 0x"
        ++ showHexInt (B.length record)
        ++ " of window 0x"
        ++ showHexInt (fromIntegral window)
        ++ " ("
        ++ show ((B.length record * 100) `div` fromIntegral window)
        ++ " per cent)"
    )
  let violations = Format.verifyRecord record
  if null violations
    then putStrLn "structure: pass (contiguous tokens, exact total_len, feed-boundary rule, stock token domain)"
    else do
      putStrLn ("structure: FAIL (" ++ show (length violations) ++ " violations)")
      forM_ violations (putStrLn . ("  " ++))
  B.writeFile recordPath record
  putStrLn ("wrote " ++ recordPath)
  unless (null violations) exitFailure

report :: String -> [Int] -> IO ()
report label positions =
  putStrLn
    ( label
        ++ ": "
        ++ show (length positions)
        ++ ( if null positions
               then ""
               else " at " ++ unwords (map (("0x" ++) . showHexInt) (take 16 positions)) ++ (if length positions > 16 then " ..." else "")
           )
    )

-- Diagnostic token walk: like Format.tokenize but records a zero-length
-- literal run (the F1 defect emission) as a token instead of stopping there,
-- so a defective stream still gets a full position report. Stops at a
-- malformed match tail, which the encoder never emits.
walkTokens :: B.ByteString -> [(Int, Format.Token)]
walkTokens body = go 0
  where
    n = B.length body
    go ip
      | ip >= n = []
      | t == 0 = (ip, Format.LiteralRun 0 B.empty) : go (ip + 1)
      | t < 0x80 =
          (ip, Format.LiteralRun t (B.take (fromIntegral t) (B.drop (ip + 1) body)))
            : go (ip + 1 + fromIntegral t)
      | ip + 2 > n = []
      | otherwise =
          let v = ((fromIntegral t .&. 0x7f :: Int) `shiftL` 8) .|. fromIntegral (B.index body (ip + 1))
           in case (Format.mkDistance (fromIntegral (v `shiftR` 4)), Format.mkMatchLen (fromIntegral (v .&. 0xf))) of
                (Just d, Just l) -> (ip, Format.Match d l) : go (ip + 2)
                _ -> []
      where
        t = B.index body ip

hexValue :: String -> IO Word32
hexValue ('0' : 'x' : digits@(_ : _))
  | all (`elem` ("0123456789abcdef" :: String)) digits = pure (fromIntegral (foldl step 0 digits))
  where
    step acc c = acc * 16 + toInteger (hexDigit c)
hexValue _ = H.failH "window: expected a 0x-prefixed hex value"

hexDigit :: Char -> Int
hexDigit c
  | '0' <= c && c <= '9' = fromEnum c - fromEnum '0'
  | otherwise = fromEnum c - fromEnum 'a' + 10

showHexInt :: Int -> String
showHexInt n
  | n < 16 = [digit n]
  | otherwise = showHexInt (n `div` 16) ++ [digit (n `mod` 16)]
  where
    digit d = if d < 10 then toEnum (fromEnum '0' + d) else toEnum (fromEnum 'a' + d - 10)
