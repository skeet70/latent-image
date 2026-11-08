{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}

-- | Production proof for bounded render trace-hit receipts. The producer writes the same declared
-- rows through Emulation.ObservationStream; the consumer reads them through the menu-engine pair
-- observation reader. The source is a JSONL derivation of the retained owned oracle-path
-- observation, amplified with an explicit instruction offset.
module Main (main) where

import Control.Monad (unless)
import Data.Aeson (Value (..), eitherDecodeStrict', encode, object, (.=))
import qualified Data.Aeson as J
import qualified Data.Aeson.Key as K
import qualified Data.Aeson.KeyMap as KM
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy.Char8 as BLC
import qualified Data.Text as T
import Data.Word (Word32)
import Emulation.ObservationStream
import EmulatorUsb (TraceHit (..), TraceHitCursor (..), TraceHitLimit (..), TraceHitPage (..))
import Replacement.ModalRuntime.RenderPair (DrawCoverage (..), RenderConfig (..), checkRenderCoverageReader)
import Numeric (readHex, showHex)
import Replacement.ModalRuntime.MenuEnginePair
import Replacement.ModalRuntime.RenderObjectProvenance (ProvenanceSequence (..))
import Replacement.ModalRuntime.RenderStackEvidence (InstructionCounter (..))
import System.Directory (createDirectoryIfMissing)
import System.Environment (getArgs)
import System.Exit (exitFailure)

main :: IO ()
main = do
  arguments <- getArgs
  case arguments of
    ["produce", source, countText, directory] -> runProduce source (readCount countText) directory
    ["consume", source, observation, countText] -> runConsume source observation (readCount countText)
    _ -> refuse "usage: render-observation-bounded produce SOURCE_JSONL COUNT DIR | consume SOURCE_JSONL OBSERVATION COUNT"

readCount :: String -> Int
readCount text = case reads text of
  [(value, "")] | value > 0 -> value
  _ -> error "count must be a positive integer"

refuse :: String -> IO a
refuse message = putStrLn ("render-observation-bounded: " ++ message) >> exitFailure

runProduce :: FilePath -> Int -> FilePath -> IO ()
runProduce source count directory = do
  base <- readBase source
  unless (not (null base)) (refuse "source has no rows")
  createDirectoryIfMissing True directory
  let sites = nubStrings (map traceSite base)
      lrSites = nubStrings [traceSite hit | hit <- base, traceLr hit /= Nothing]
      spSites = nubStrings [traceSite hit | hit <- base, traceSp hit /= Nothing]
      sourcePage_ cursor limit = pure (pageAt base count cursor limit)
      readRows = [object ["label" .= label, "bytes" .= hexText bytes]
              | (label, bytes) <- ("task1-stack", B.replicate 3072 0) : [(label, B.replicate 8 0) | label <- engineReads]]
      spec = ObservationSpec directory "e2e-observation-replay" "replay"
        (object ["phase" .= ("replay" :: T.Text), "reads" .= readRows])
        [TraceHitSource (TraceHitCursor 0) (TraceHitCursor count) (TraceHitSnapshot count) sourcePage_]
        (TraceHitFormat lrSites spSites sites)
        (object ["menu_messages" .= ([] :: [Word32]), "d3_hook_r0" .= ([] :: [Word32]), "classic_chrome_site_entries" .= ([] :: [String])])
  ref <- writeObservationStream spec
  print (encodeRef ref)

runConsume :: FilePath -> FilePath -> Int -> IO ()
runConsume source observation count = do
  base <- readBase source
  unless (not (null base)) (refuse "source has no rows")
  result <- readObservationReader EngineResident observation
  reader <- either (refuse . ("observation: " ++)) pure result
  unless (readerPhase reader == "replay") (refuse "observation phase differs")
  let step index hit
        | hit == toHit (replicatedHit base index) = Right (index + 1)
        | otherwise = Left ("row " ++ show index ++ " differs from its source row")
  rows <- readerFoldHits reader step 0
  rowsCount <- either (refuse . ("rows: " ++)) pure rows
  unless (rowsCount == count) (refuse ("observation has " ++ show rowsCount ++ " hits, expected " ++ show count))
  stack <- checkStackReader 0 [reader]
  _ <- either (refuse . ("stack: " ++)) pure stack
  pair <- comparePairReader [reader] [reader]
  _ <- either (refuse . ("pair: " ++)) pure pair
  coverage <- checkRenderCoverageReader (RenderConfig [0..18] Nothing 0xa4ce00 DrawsRequired) [reader] [reader]
  _ <- either (refuse . ("render coverage: " ++)) pure coverage
  putStrLn (jsonLine [("rows", J.toJSON rowsCount), ("phase", J.String (readerPhase reader)), ("checks", J.String "row/stack/pair/render-coverage")])


pageAt :: [TraceHit] -> Int -> TraceHitCursor -> TraceHitLimit -> TraceHitPage
pageAt base count (TraceHitCursor from) (TraceHitLimit limit) =
  TraceHitPage count (TraceHitCursor from) [replicatedHit base index | index <- [from .. min (from + limit) count - 1]]

replicatedHit :: [TraceHit] -> Int -> TraceHit
replicatedHit base index =
  let hit = base !! (index `mod` length base)
  in hit { traceInstructions = traceInstructions hit + fromIntegral index }

toHit :: TraceHit -> Hit
toHit hit = Hit
  { hitSite = parseSite (T.pack (traceSite hit))
  , hitCore = traceCore hit
  , hitRegisters = (traceR0 hit, traceR1 hit, traceR2 hit, traceR3 hit)
  , hitLr = traceLr hit
  , hitSp = traceSp hit
  , hitMemory = traceMemory hit
  , hitProvenanceSequence = traceProvenanceSequence hit
  , hitInstructions = Just (InstructionCounter (fromIntegral (traceInstructions hit)))
  }

readBase :: FilePath -> IO [TraceHit]
readBase path = do
  bytes <- B.readFile path
  let lines_ = filter (not . B.null) (B.split 10 bytes)
  mapM (parseHit . either (error "source row") id . eitherDecodeStrict') lines_

parseHit :: Value -> IO TraceHit
parseHit raw = case raw of
  Object fields -> do
    site <- text "site" fields
    core <- number "core" fields
    instructions <- number "instructions" fields
    r0 <- word "r0" fields
    r1 <- word "r1" fields
    r2 <- word "r2" fields
    r3 <- word "r3" fields
    lr <- optionalWord "lr" fields
    sp <- optionalWord "sp" fields
    memory <- case KM.lookup "memory" fields of
      Nothing -> pure []
      Just (Array entries) -> mapM (bytesOf . textValue) (foldr (:) [] entries)
      Just _ -> error "memory is not an array"
    provenance <- case KM.lookup "render_object_provenance_sequence" fields of
      Nothing -> pure Nothing
      Just (Number n) -> pure (Just (ProvenanceSequence (truncate n)))
      _ -> error "provenance sequence is not a number"
    pure (TraceHit site core instructions r0 r1 r2 r3 lr sp memory provenance)
  _ -> error "trace hit is not an object"
 where
  text name fields = case KM.lookup (K.fromString name) fields of
    Just (String value) -> pure (T.unpack value)
    _ -> error (name ++ " is missing")
  number name fields = case KM.lookup (K.fromString name) fields of
    Just (Number value) -> pure (truncate value)
    _ -> error (name ++ " is missing")
  word name fields = optionalWord name fields >>= maybe (error (name ++ " is missing")) pure
  optionalWord name fields = case KM.lookup (K.fromString name) fields of
    Just (String value) -> either error (pure . Just) (hexWord (T.unpack value))
    Nothing -> pure Nothing
    _ -> error (name ++ " is not a hex string")
  textValue (String encoded) = T.unpack encoded
  textValue _ = ""
  bytesOf encoded = either error pure (hexBytes encoded)

hexText :: B.ByteString -> T.Text
hexText = T.pack . concatMap (\byte -> let digits = showHex byte "" in replicate (2 - length digits) '0' ++ digits) . B.unpack

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

encodeRef :: ObservationRef -> String
encodeRef = BLC.unpack . encode . J.toJSON

jsonLine :: [(String, J.Value)] -> String
jsonLine fields = BLC.unpack (encode (object [K.fromString key .= value | (key, value) <- fields]))

nubStrings :: [String] -> [String]
nubStrings = foldr (\value rest -> if value `elem` rest then rest else value : rest) []
