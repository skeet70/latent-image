{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}

-- | Host-only replay of the render trace-hit receipt collector. It reads the declared hits from
-- one retained observation, replicates them to a representative count with an explicit
-- instruction offset, and runs either the in-memory observation encode or a paged streamed
-- collector. The source is the owned oracle-path observation; no guest runs.
module Main (main) where

import Control.Monad (forM_, unless)
import Data.Aeson (Value (..), eitherDecodeStrict', encode, object, withObject, (.:), (.=))
import qualified Data.Aeson as J
import Data.Aeson.Types (parseEither)
import qualified Data.Aeson.Key as K
import qualified Data.Aeson.KeyMap as KM
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as BL
import qualified Data.ByteString.Lazy.Char8 as BLC
import qualified Data.Text as T
import Data.Word (Word32)
import EmulatorUsb (TraceHit (..), traceHitValue)
import Emulation.Sha256 (sha256File, sha256Hex)
import Numeric (readHex)
import System.Environment (getArgs)
import System.Exit (exitFailure)
import System.IO (IOMode (WriteMode), hClose, hFlush, openBinaryFile)
import System.Mem (performGC)

main :: IO ()
main = do
  arguments <- getArgs
  case arguments of
    [mode, source, countText, output] -> do
      count <- case reads countText of
        [(value, "")] | value > 0 -> pure (value :: Int)
        _ -> refuse "count must be a positive integer"
      sourceValue <- either refuse pure . eitherDecodeStrict' =<< B.readFile source
      base <- either refuse pure (parseEither (withObject "observation" (\fields -> fields .: "trace_hits")) sourceValue)
      hits <- parseHits base
      performGC
      unless (not (null hits)) (refuse "source has no trace hits")
      case mode of
        "old" -> oldCollector [ replicatedHit hits index | index <- [0 .. count - 1] ] output
        "new" -> newCollector hits count output
        _ -> refuse "mode must be old or new"
      digest <- sha256File output
      putStrLn (jsonLine
        [ ("mode", J.String (T.pack mode)), ("source", J.String (T.pack source))
        , ("base_hits", J.toJSON (length hits)), ("replicated_hits", J.toJSON count)
        , ("amplification", J.String "base hit i mod base count, instructions plus index")
        , ("output_sha256", J.String (T.pack (sha256Hex digest)))
        ])
    _ -> refuse "usage: render-trace-hit-memory old|new SOURCE_OBSERVATION COUNT OUTPUT"

refuse :: String -> IO a
refuse message = putStrLn ("render-trace-hit-memory: " ++ message) >> exitFailure

oldCollector :: [TraceHit] -> FilePath -> IO ()
oldCollector hits output = do
  let rows = map (traceHitValue [] []) hits
      observation = object ["phase" .= ("replay" :: T.Text), "trace_hits" .= rows]
  BL.writeFile output (encode observation)
  putStrLn ("old rows: " ++ show (length rows))

newCollector :: [TraceHit] -> Int -> FilePath -> IO ()
newCollector hits count output = do
  handle <- openBinaryFile output WriteMode
  B.hPut handle "{\"phase\":\"replay\",\"trace_hits\":["
  let pages = (count + 255) `div` 256
  forM_ [0 .. pages - 1] $ \pageIndex -> do
    let first = pageIndex * 256
        last_ = min count (first + 256)
        page = [ replicatedHit hits index | index <- [first .. last_ - 1] ]
    forM_ (zip [0 :: Int ..] page) $ \(rowIndex, hit) -> do
      unless (pageIndex == 0 && rowIndex == 0) (B.hPut handle ",")
      B.hPut handle (BL.toStrict (encode (traceHitValue [] [] hit)))
    hFlush handle
  B.hPut handle "]}\n"
  hClose handle
  putStrLn ("new rows: " ++ show count ++ " in " ++ show pages ++ " pages")

replicatedHit :: [TraceHit] -> Int -> TraceHit
replicatedHit hits index =
  let hit = hits !! (index `mod` length hits)
  in hit { traceInstructions = traceInstructions hit + fromIntegral index }

parseHits :: Value -> IO [TraceHit]
parseHits (Array values) = mapM parseHit (foldr (:) [] values)
parseHits _ = refuse "trace_hits is not an array"

parseHit :: Value -> IO TraceHit
parseHit raw = case raw of
  Object fields -> do
    site <- text "site" fields
    core <- number "core" fields
    instructions <- integer "instructions" fields
    r0 <- word "r0" fields
    r1 <- word "r1" fields
    r2 <- word "r2" fields
    r3 <- word "r3" fields
    lr <- optionalWord "lr" fields
    sp <- optionalWord "sp" fields
    memory <- case KM.lookup "memory" fields of
      Nothing -> pure []
      Just (Array entries) -> mapM (bytesOf . textValue) (foldr (:) [] entries)
      Just _ -> refuse "memory is not an array"
    pure (TraceHit site core instructions r0 r1 r2 r3 lr sp memory Nothing)
  _ -> refuse "trace hit is not an object"
 where
  text name fields = case KM.lookup (K.fromString name) fields of
    Just (String value) -> pure (T.unpack value)
    _ -> refuse (name ++ " is missing")
  number name fields = case KM.lookup (K.fromString name) fields of
    Just (Number value) -> pure (truncate value)
    _ -> refuse (name ++ " is missing")
  integer name fields = case KM.lookup (K.fromString name) fields of
    Just (Number value) -> pure (truncate value)
    _ -> refuse (name ++ " is missing")
  word name fields = optionalWord name fields >>= maybe (refuse (name ++ " is missing")) pure
  optionalWord name fields = case KM.lookup (K.fromString name) fields of
    Just (String value) -> either refuse (pure . Just) (hexWord (T.unpack value))
    Nothing -> pure Nothing
    _ -> refuse (name ++ " is not a hex string")
  textValue (String value) = T.unpack value
  textValue _ = ""
  bytesOf encoded = either refuse pure (hexBytes encoded)

hexWord :: String -> Either String Word32
hexWord text = case text of
  '0' : 'x' : digits | [(value, "")] <- readHex digits, value <= 0xffffffff -> Right value
  _ -> Left ("not a hexadecimal word: " ++ text)

hexBytes :: String -> Either String B.ByteString
hexBytes text
  | odd (length text) = Left "odd hexadecimal length"
  | otherwise = B.pack <$> mapM pair (pairs text)
 where
  pairs [] = []
  pairs (high : low : rest) = [high, low] : pairs rest
  pairs _ = []
  pair [high, low] = case readHex [high, low] of
    [(value, "")] -> Right value
    _ -> Left "invalid hexadecimal byte"
  pair _ = Left "invalid hexadecimal byte"

jsonLine :: [(String, J.Value)] -> String
jsonLine fields = BLC.unpack (encode (object [K.fromString key .= value | (key, value) <- fields]))

-- The replay uses the encoded row form directly, so the old and new collectors share the exact
-- traceHitValue output. The old path retains every row in one Value; the new path writes each
-- 256-row page and flushes it.
