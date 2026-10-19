{-# LANGUAGE OverloadedStrings #-}
-- The private Classic Chrome parameter file the patch build consumes,
-- schema latent-image.classic-chrome-parameters.v1
-- (reports/classic-chrome-patch.md section 7).
module FujiAF.ClassicChromeCandidate
  ( Candidate (..)
  , Input (..)
  , candidateJson
  , parseCandidate
  , candidateSchema
  ) where

import Control.Monad (forM, unless, when)
import Data.Aeson (Value (..), object, (.=))
import qualified Data.Aeson.Key as K
import qualified Data.Aeson.KeyMap as KM
import Data.List (sort)
import Data.Scientific (floatingOrInteger)
import qualified Data.Text as T
import qualified Data.Vector as V
import Data.Word (Word16)

import FujiAF.FilmStage
import FujiAF.XM1Develop (chromaFromStorage, chromaStorage, decodeField, matrixStorage)

candidateSchema :: T.Text
candidateSchema = "latent-image.classic-chrome-parameters.v1"

data Input = Input {inputPath :: T.Text, inputSha256 :: T.Text} deriving (Eq, Show)

-- | e_method and e_model are mean CIEDE2000 in hundredths.
data Candidate = Candidate
  { candidateMethod :: T.Text
  , candidateInputs :: [Input]
  , candidateEMethod :: Int
  , candidateEModel :: Maybe Int
  , candidateMatrix :: ColourMatrix
  , candidateChroma :: Chroma
  , candidateTone :: ToneRecord
  }
  deriving (Eq, Show)

firmwareSha256 :: T.Text
firmwareSha256 = "43d67afa1a2bc9ae8481bc0acf02cb085ed0f515122dcbd26addcae102cd1490"

approximationOf :: T.Text
approximationOf = "Classic Chrome, EXR Processor II with X-Trans CMOS II (X-T1 5.52, X70 1.10)"

checkCurve :: [Int] -> Either String ()
checkCurve ps = do
  unless (length ps == 28) (Left "tone curve needs 28 points")
  unless (and (zipWith (<=) ps (drop 1 ps))) (Left "tone curve decreases")
  unless (all (\p -> p >= 0 && p <= 0x3fff) ps && last ps == 0x3fff) (Left "tone curve must stay within 0x3fff and end at 0x3fff")

candidateJson :: Candidate -> Either String Value
candidateJson c = do
  m <- matrixStorage (candidateMatrix c)
  ch <- chromaStorage (candidateChroma c)
  let tone = candidateTone c
  checkCurve (tonePoints tone)
  pure $
    object
      [ "schema" .= candidateSchema
      , "target" .= object ["model" .= ("X-M1" :: T.Text), "firmware_version" .= ("1.50" :: T.Text), "firmware_sha256" .= firmwareSha256]
      , "approximation_of" .= approximationOf
      , "provenance"
          .= object
            [ "method" .= candidateMethod c
            , "inputs" .= [object ["path" .= inputPath i, "sha256" .= inputSha256 i] | i <- candidateInputs c]
            , "e_method" .= candidateEMethod c
            , "e_model" .= candidateEModel c
            ]
      , "colour_matrix" .= map (map toInteger) m
      , "chroma" .= map toInteger ch
      , "tone_record" .= object ["head" .= toneExponents tone, "curve" .= tonePoints tone]
      , "reuse" .= object [k .= ("PROVIA" :: T.Text) | k <- ["luma_band", "chroma_gain", "hs_tone", "tone_scalar"]]
      ]

-- | The build's reading of the file: exact field sets, integers only, target
-- the pinned 1.50 DAT, every reuse PROVIA.
parseCandidate :: Value -> Either String Candidate
parseCandidate v = do
  top <- fields ["schema", "target", "approximation_of", "provenance", "colour_matrix", "chroma", "tone_record", "reuse"] v
  schema <- text =<< field top "schema"
  when (schema /= candidateSchema) (Left "unknown schema")
  target <- fields ["model", "firmware_version", "firmware_sha256"] =<< field top "target"
  tm <- text =<< field target "model"
  tv <- text =<< field target "firmware_version"
  ts <- text =<< field target "firmware_sha256"
  when ((tm, tv, ts) /= ("X-M1", "1.50", firmwareSha256)) (Left "target is not the pinned X-M1 1.50 DAT")
  approx <- text =<< field top "approximation_of"
  when (approx /= approximationOf) (Left "approximation_of differs")
  prov <- fields ["method", "inputs", "e_method", "e_model"] =<< field top "provenance"
  method <- text =<< field prov "method"
  inputs <- field prov "inputs" >>= array >>= mapM (\x -> do o <- fields ["path", "sha256"] x; Input <$> (text =<< field o "path") <*> (text =<< field o "sha256"))
  eMethod <- int =<< field prov "e_method"
  eModel <- field prov "e_model" >>= \x -> if x == Null then Right Nothing else Just <$> int x
  rows <- field top "colour_matrix" >>= array >>= mapM (\r -> array r >>= mapM int)
  unless (length rows == 3 && all ((== 3) . length) rows) (Left "colour_matrix needs three rows of three")
  matrixRaw <- mapM (mapM word16) rows
  m <- colourMatrix (map (map decodeField) matrixRaw)
  chRaw <- field top "chroma" >>= array >>= mapM (\x -> int x >>= word16)
  ch <- chromaFromStorage chRaw
  toneO <- fields ["head", "curve"] =<< field top "tone_record"
  hd <- field toneO "head" >>= array >>= mapM int
  unless (length hd == 9 && all (\b -> b >= 0 && b <= 255) hd) (Left "tone head needs nine bytes")
  curve <- field toneO "curve" >>= array >>= mapM int
  checkCurve curve
  tone <- toneRecord hd curve
  reuse <- fields ["luma_band", "chroma_gain", "hs_tone", "tone_scalar"] =<< field top "reuse"
  forM_' ["luma_band", "chroma_gain", "hs_tone", "tone_scalar"] $ \k -> do
    r <- text =<< field reuse k
    when (r /= "PROVIA") (Left ("reuse " ++ T.unpack k ++ " is not PROVIA"))
  pure (Candidate method inputs eMethod eModel m ch tone)
 where
  forM_' xs f = () <$ forM xs f
  fields names (Object o) = do
    let keys = sort (map K.toText (KM.keys o))
    when (keys /= sort names) (Left ("fields " ++ show keys ++ ", expected " ++ show (sort names)))
    pure o
  fields _ _ = Left "object expected"
  field o k = maybe (Left ("missing " ++ T.unpack k)) Right (KM.lookup (K.fromText k) o)
  text (String s) = Right s
  text _ = Left "string expected"
  array (Array a) = Right (V.toList a)
  array _ = Left "array expected"
  int (Number n) = either (const (Left "number is not an integer")) Right (floatingOrInteger n :: Either Double Int)
  int _ = Left "integer expected"
  word16 :: Int -> Either String Word16
  word16 x = if x >= 0 && x <= 0xffff then Right (fromIntegral x) else Left "field outside u16"
