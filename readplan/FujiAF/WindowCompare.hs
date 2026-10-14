{-# LANGUAGE OverloadedStrings #-}
-- Comparison of captured adjustment-area windows against the firmware fallback adjustment image.
-- Pure: the caller reads the captured bytes and hands them in.
module FujiAF.WindowCompare
  ( parsePlan
  , FallbackImage
  , fallbackBytes
  , fallbackLoadLength
  , fallbackImage
  , Outcome(..)
  , WindowReport(..)
  , Feed(..)
  , captureName
  , compareWindow
  , feedsAt
  , Summary(..)
  , summarize
  , reportLines
  ) where

import Data.Aeson (Value(..))
import qualified Data.Aeson.Key as K
import qualified Data.Aeson.KeyMap as M
import qualified Data.ByteString as B
import Data.List (nub, sort)
import Data.Scientific (toBoundedInteger)
import qualified Data.Text as T
import qualified Data.Vector as V
import Data.Word (Word32)
import Numeric (readHex)
import FujiAF.AdjustmentTables
import FujiAF.ReadPlan

-- Adjustment offset 0 is byte 0 of the fallback image at payload 0xe0048.
newtype FallbackImage = FallbackImage { fallbackBytes :: B.ByteString }

-- 0x0069f204 calls FUN_006a9ac4 from 0x0069f27c with this length, so a window at or past it has
-- no fallback bytes.
fallbackLoadLength :: Int
fallbackLoadLength = 0x42000

-- Takes the payload bytes from the fallback base onward.
fallbackImage :: B.ByteString -> FallbackImage
fallbackImage = FallbackImage . B.take fallbackLoadLength

data Outcome
  = Matched
  | Differed [Word32]
  | NotComparable String
  deriving (Eq, Show)

data WindowReport = WindowReport
  { reportWindow :: Window
  , reportOutcome :: Outcome
  } deriving (Eq, Show)

-- A resolved table entry that reads a differing byte.
data Feed = Feed
  { feedTable :: String
  , feedKind :: RowZeroIndex
  , feedEntry :: Int
  , feedOffset :: Word32
  } deriving (Eq, Show)

captureName :: Window -> FilePath
captureName (Window _ (AdjOffset offset) _) = drop 2 (showOffset offset) ++ ".bin"

compareWindow :: FallbackImage -> Window -> Either String B.ByteString -> WindowReport
compareWindow fallback window captured = WindowReport window outcome
  where
    Window _ (AdjOffset offset) _ = window
    size = fromIntegral windowLength
    outcome = case captured of
      Left reason -> NotComparable reason
      Right bytes
        | B.length bytes /= size ->
            NotComparable ("captured window is " ++ show (B.length bytes) ++ " bytes, expected " ++ show size)
        | fromIntegral offset + size > B.length (fallbackBytes fallback) ->
            NotComparable ("fallback image stops before " ++ showOffset (offset + windowLength))
        | otherwise ->
            let reference = B.take size (B.drop (fromIntegral offset) (fallbackBytes fallback))
                differing = [ offset + fromIntegral position
                            | position <- [0 .. size - 1]
                            , B.index bytes position /= B.index reference position ]
            in if null differing then Matched else Differed differing

-- Attribution is byte-exact: an entry feeds a differing byte when the entry resolves to that
-- offset. Bytes inside a block that no entry starts at are counted as unattributed.
feedsAt :: [TableResolution] -> [Word32] -> [Feed]
feedsAt resolutions differing =
  [ Feed name (tableRowZeroIndex (resolvedTable resolution)) index offset
  | resolution <- resolutions
  , let TableName name = tableName (resolvedTable resolution)
  , ResolvedEntry index (AdjOffset offset) <- resolvedEntries resolution
  , offset `elem` differing ]

data Summary = Summary
  { summaryMatched :: Int
  , summaryDiffered :: Int
  , summaryNotComparable :: Int
  , summaryDifferingBytes :: Int
  , summaryDevelopCodes :: [Int]
  , summaryFilters :: [Int]
  , summaryUnindexedTables :: [String]
  , summaryUnattributed :: Int
  } deriving (Eq, Show)

summarize :: [TableResolution] -> [WindowReport] -> Summary
summarize resolutions reports = Summary
  { summaryMatched = count (== Matched)
  , summaryDiffered = length [() | WindowReport _ (Differed _) <- reports]
  , summaryNotComparable = length [() | WindowReport _ (NotComparable _) <- reports]
  , summaryDifferingBytes = length differing
  , summaryDevelopCodes = sort (nub [feedEntry feed | feed <- feeds, feedKind feed == ByDevelopCode])
  , summaryFilters = sort (nub [feedEntry feed | feed <- feeds, feedKind feed == ByMonochromeFilter])
  , summaryUnindexedTables = nub [feedTable feed | feed <- feeds, feedKind feed == Unindexed]
  , summaryUnattributed = length (filter (`notElem` map feedOffset feeds) differing)
  }
  where
    count predicate = length (filter (predicate . reportOutcome) reports)
    differing = concat [offsets | WindowReport _ (Differed offsets) <- reports]
    feeds = feedsAt resolutions differing

reportLines :: [TableResolution] -> [WindowReport] -> [String]
reportLines resolutions reports = map line reports ++ summaryLines
  where
    line (WindowReport window outcome) = "window " ++ showOffset (offsetOf window) ++ " " ++ describe outcome
    offsetOf (Window _ (AdjOffset offset) _) = offset
    describe Matched = "match"
    describe (NotComparable reason) = "not-comparable " ++ reason
    describe (Differed []) = "differ"
    describe (Differed (first : rest)) =
      "differ " ++ show (1 + length rest) ++ " bytes from " ++ showOffset first
        ++ " through " ++ showOffset (foldl max first rest)
    summary = summarize resolutions reports
    summaryLines =
      [ show (length reports) ++ " windows: " ++ show (summaryMatched summary) ++ " match, "
          ++ show (summaryDiffered summary) ++ " differ, " ++ show (summaryNotComparable summary) ++ " not comparable"
      , "differing bytes " ++ show (summaryDifferingBytes summary)
          ++ ", unattributed " ++ show (summaryUnattributed summary)
      , "develop codes fed by differing bytes: " ++ list (map show (summaryDevelopCodes summary))
      , "monochrome filter indices: " ++ list (map show (summaryFilters summary))
      , "unindexed tables with differing entries: " ++ list (summaryUnindexedTables summary)
      ]
    list [] = "none"
    list items = unwords items

parsePlan :: Value -> Either String [Window]
parsePlan document = do
  expect document "schema_version" (Number 1)
  channel <- member document "channel"
  expect channel "opcode" (String "0x1a0002")
  expect channel "function" (Number 2)
  expect channel "mb" (Number 1)
  expect channel "length" (Number 256)
  items <- member document "windows" >>= array
  windows <- traverse parseWindow items
  validateWindows windows
  pure windows

parseWindow :: Value -> Either String Window
parseWindow value = do
  expect value "length" (Number 256)
  index <- member value "index" >>= integer
  offset <- member value "offset" >>= text >>= parseOffset
  purpose <- member value "purpose" >>= text
  pure (Window index (AdjOffset offset) purpose)

member :: Value -> String -> Either String Value
member (Object fields) key = maybe (Left ("plan field " ++ key ++ " is missing")) Right (M.lookup (K.fromString key) fields)
member _ key = Left ("plan field " ++ key ++ " needs an object")

expect :: Value -> String -> Value -> Either String ()
expect document key wanted = do
  actual <- member document key
  if actual == wanted then Right () else Left ("plan field " ++ key ++ " is not the value this tool reads")

array :: Value -> Either String [Value]
array (Array items) = Right (V.toList items)
array _ = Left "plan windows must be an array"

integer :: Value -> Either String Int
integer (Number value) = maybe (Left "plan integer out of range") Right (toBoundedInteger value)
integer _ = Left "plan integer expected"

text :: Value -> Either String String
text (String value) = Right (T.unpack value)
text _ = Left "plan string expected"

parseOffset :: String -> Either String Word32
parseOffset ('0' : 'x' : digits) = case readHex digits of
  [(value, "")] | value < fromIntegral adjustmentAreaLimit -> Right (fromIntegral (value :: Integer))
  _ -> Left ("window offset 0x" ++ digits ++ " is not an adjustment-area address")
parseOffset other = Left ("window offset " ++ other ++ " is not hexadecimal")
