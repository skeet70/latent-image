{-# LANGUAGE OverloadedStrings #-}

-- Behavioural tests for the Classic Chrome build library: parameter-file
-- refusals, the data block of the public synthetic set, A32 BL encoding and
-- the image-4 prefix layout. The only file read is the synthetic set.
module Main (main) where

import Control.Monad (forM_, unless)
import Data.Aeson (Value (..))
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as BC
import Data.IORef (IORef, modifyIORef', newIORef, readIORef)
import Data.List (sortOn)
import Data.Scientific (fromFloatDigits)
import Data.Word (Word16, Word32)
import FujiAF.ClassicChrome
import qualified FujiAF.Host as H
import FujiAF.JSON (encodeCanonical)
import System.Environment (getArgs)
import System.Exit (exitFailure)
import System.IO (hPutStrLn, stderr)

expect :: IORef [String] -> String -> Bool -> IO ()
expect failures label ok = unless ok (modifyIORef' failures (label :))

halves :: [Word16] -> B.ByteString
halves = B.concat . map (\h -> B.pack [fromIntegral (h `mod` 256), fromIntegral (h `div` 256)])

ramp :: [Integer]
ramp = [i * 0x3fff `div` 27 | i <- [0 .. 27]]

-- A valid document; each field can be replaced or removed by the cases.
document :: [(String, Maybe Value)] -> B.ByteString
document overrides =
  encodeCanonical
    ( H.obj
        ( [(k, v) | (k, d) <- base, Just v <- [maybe (Just d) id (lookup k overrides)]]
            ++ [(k, v) | (k, Just v) <- overrides, k `notElem` map fst base]
        )
    )
  where
    base =
      [ ("schema", H.s "latent-image.classic-chrome-parameters.v1")
      , ("target", target stockFirmwareSha256)
      , ("approximation_of", H.s "test")
      , ("provenance", provenance [])
      , ("colour_matrix", matrix [[0x400, 0, 0], [0, 0x400, 0], [0, 0, 0x400]])
      , ("chroma", ints [0x400, 0, 0, 0x400])
      , ("tone_record", tone (replicate 9 0) ramp)
      , ("reuse", reuse "PROVIA")
      ]

target :: String -> Value
target digest = H.obj [("model", H.s "X-M1"), ("firmware_version", H.s "1.50"), ("firmware_sha256", H.s digest)]

provenance :: [(String, Value)] -> Value
provenance extra = H.merge (H.obj [("method", H.s "test"), ("inputs", H.arr []), ("e_method", H.n 0), ("e_model", Null)]) extra

ints :: [Integer] -> Value
ints = H.arr . map H.n

matrix :: [[Integer]] -> Value
matrix = H.arr . map ints

tone :: [Integer] -> [Integer] -> Value
tone h c = H.obj [("head", ints h), ("curve", ints c)]

reuse :: String -> Value
reuse lumaBand = H.obj [("luma_band", H.s lumaBand), ("chroma_gain", H.s "PROVIA"), ("hs_tone", H.s "PROVIA"), ("tone_scalar", H.s "PROVIA")]

refused :: B.ByteString -> Bool
refused bytes = either (const True) (const False) (parseParameters bytes)

main :: IO ()
main = do
  arguments <- getArgs
  syntheticPath <- case arguments of
    [p] -> pure p
    _ -> hPutStrLn stderr "usage: classic-chrome-tests SYNTHETIC_JSON" >> exitFailure
  failures <- newIORef []
  let check = expect failures

  -- Refusals.
  let replaced k v = document [(k, Just v)]
      refusals =
        [ ("wrong target sha", replaced "target" (target (replicate 64 'a')))
        , ("wrong schema", replaced "schema" (H.s "latent-image.classic-chrome-parameters.v2"))
        , ("unknown top-level field", replaced "extra" (H.n 1))
        , ("missing field", document [("reuse", Nothing)])
        , ("unknown provenance field", replaced "provenance" (provenance [("note", H.s "x")]))
        , ("non-integer matrix value", replaced "colour_matrix" (H.arr [H.arr [Number (fromFloatDigits (1.5 :: Double)), H.n 0, H.n 0], ints [0, 0x400, 0], ints [0, 0, 0x400]]))
        , ("non-integer e_method", replaced "provenance" (provenance [("e_method", Number (fromFloatDigits (0.5 :: Double)))]))
        , ("string e_model", replaced "provenance" (provenance [("e_model", H.s "1")]))
        , ("matrix row of two", replaced "colour_matrix" (matrix [[0x400, 0], [0, 0x400, 0], [0, 0, 0x400]]))
        , ("matrix of two rows", replaced "colour_matrix" (matrix [[0x400, 0, 0], [0, 0x400, 0]]))
        , ("chroma of three", replaced "chroma" (ints [0x400, 0, 0]))
        , ("head of eight", replaced "tone_record" (tone (replicate 8 0) ramp))
        , ("head byte 256", replaced "tone_record" (tone (256 : replicate 8 0) ramp))
        , ("curve of 27", replaced "tone_record" (tone (replicate 9 0) (drop 1 ramp)))
        , ("curve not monotone", replaced "tone_record" (tone (replicate 9 0) (take 5 ramp ++ [0] ++ drop 6 ramp)))
        , ("curve not ending 0x3fff", replaced "tone_record" (tone (replicate 9 0) (take 27 ramp ++ [0x3ffe])))
        , ("curve above 0x3fff", replaced "tone_record" (tone (replicate 9 0) (take 27 ramp ++ [0x4000])))
        , ("matrix top nibble 2", replaced "colour_matrix" (matrix [[0x2000, 0, 0], [0, 0x400, 0], [0, 0, 0x400]]))
        , ("chroma top nibble 2", replaced "chroma" (ints [0x400, 0, 0, 0x2400]))
        , ("negative matrix value", replaced "colour_matrix" (matrix [[-1, 0, 0], [0, 0x400, 0], [0, 0, 0x400]]))
        , ("reuse not PROVIA", replaced "reuse" (reuse "VELVIA"))
        , ("bad input sha", replaced "provenance" (provenance [("inputs", H.arr [H.obj [("path", H.s "p"), ("sha256", H.s "ABC")]])]))
        ]
  forM_ refusals $ \(label, bytes) -> check ("refuses " ++ label) (refused bytes)
  check "refuses 1.0 spelled as a float" (refused (BC.pack (BC.unpack (document []) `replaceFirst` ("\"e_method\":0", "\"e_method\":0.0"))))
  check "accepts the base document" (not (refused (document [])))
  check "accepts an integer e_model" (not (refused (replaced "provenance" (provenance [("e_model", H.n 250)]))))
  check "accepts a top-nibble-1 field" (not (refused (replaced "colour_matrix" (matrix [[0x1fff, 0, 0], [0, 0x400, 0], [0, 0, 0x400]]))))

  -- Data block of the synthetic set.
  synthetic <- B.readFile syntheticPath
  case parseParameters synthetic of
    Left message -> check ("synthetic set parses: " ++ message) False
    Right p -> do
      let expected =
            B.concat
              [ "CCD1"
              , halves [0x400, 0, 0, 0, 0x400, 0, 0, 0, 0x400]
              , halves [0x400, 0, 0x400, 0]
              , B.replicate 2 0
              , B.replicate 9 0
              , B.singleton 0
              , halves (map fromInteger ramp)
              , B.replicate 2 0
              ]
      check "synthetic data block is 100 bytes" (B.length (encodeDataBlock p) == 100)
      check "synthetic data block bytes" (encodeDataBlock p == expected)
  case parseParameters (replaced "chroma" (ints [0x101, 0x202, 0x303, 0x404])) of
    Left message -> check ("distinct chroma parses: " ++ message) False
    Right p -> check "chroma stored +0,+2,+4,+6 is written +0,+4,+6,+2" (B.take 8 (B.drop 0x16 (encodeDataBlock p)) == halves [0x101, 0x303, 0x404, 0x202])
  case parseParameters (replaced "tone_record" (tone [1 .. 9] ramp)) of
    Left message -> check ("tone head parses: " ++ message) False
    Right p -> check "tone head at +0x20 then a zero byte" (B.take 10 (B.drop 0x20 (encodeDataBlock p)) == B.pack ([1 .. 9] ++ [0]))

  -- BL encoding.
  check "stock colour hook word decodes to 0x00d670a0" (blTarget (Address 0x00d67284) 0xebffff85 == Just (Address 0x00d670a0))
  check "stock tone hook word decodes to 0x00690924" (blTarget (Address 0x00d677ac) 0xebe4a45c == Just (Address 0x00690924))
  check "encodes the stock colour hook word" (encodeBl (Address 0x00d67284) (Address 0x00d670a0) == Right 0xebffff85)
  check "encodes the stock chroma hook word" (encodeBl (Address 0x00d64ef4) (Address 0x00d60cc4) == Right 0xebffef72)
  forM_ [(0x00d67284, 0x00738ff4), (0x00d677bc, 0x00736a80), (0x00001000, 0x00001008), (0x00001000, 0x00001000), (0x02000000, 0x00000008), (0x00000000, 0x01fffffc + 8)] $ \(site, to) ->
    check ("BL round trip " ++ show (site :: Word32, to :: Word32)) ((encodeBl (Address site) (Address to) >>= maybe (Left "no decode") Right . blTarget (Address site)) == Right (Address to))
  check "refuses a BL beyond +32 MiB" (either (const True) (const False) (encodeBl (Address 0) (Address 0x02000008)))
  check "refuses a misaligned BL target" (either (const True) (const False) (encodeBl (Address 0x1000) (Address 0x2002)))
  check "non-BL word has no BL target" (blTarget (Address 0x1000) 0xea000000 == Nothing)

  -- Prefix layout.
  let spans = sortOn fst [(a, n) | r <- prefixLayout, let (Address a, n) = extent r]
      extent (Region _ a n) = (a, n)
      extent (ZeroFill a n) = (a, n)
      Address low = prefixBase
      Address high = prefixEnd
  check "prefix regions tile [0x61c200, 0x61c400) without gaps or overlap" (and (zipWith (\(a, n) (b, _) -> a + fromIntegral n == b) spans (drop 1 spans)) && fmap fst (safeHead spans) == Just low && (let (a, n) = last spans in a + fromIntegral n) == high)
  check "prefix window is 0x61c200..0x61c400" (low == 0x0061c200 && high == 0x0061c400)
  forM_ [(DataBlock, 0x0061c200, 100), (RawValues, 0x0061c264, 56), (ShootingLabels, 0x0061c2b0, 144), (ShootingValues, 0x0061c340, 72), (RawLabels, 0x0061c388, 112)] $ \(content, address, size) ->
    check ("prefix region " ++ show content) (regionAddress content == Address address && regionSize content == size)
  check "data-bearing regions use 484 bytes" (sum [n | Region _ _ n <- prefixLayout] == 484)

  failed <- readIORef failures
  unless (null failed) $ do
    mapM_ (hPutStrLn stderr . ("FAIL: " ++)) (reverse failed)
    exitFailure
  putStrLn "classic-chrome-tests: all checks passed"
  where
    safeHead (x : _) = Just x
    safeHead [] = Nothing

replaceFirst :: String -> (String, String) -> String
replaceFirst haystack (needle, replacement) = go haystack
  where
    go s@(c : rest)
      | take (length needle) s == needle = replacement ++ drop (length needle) s
      | otherwise = c : go rest
    go [] = []
