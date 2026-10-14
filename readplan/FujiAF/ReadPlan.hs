{-# LANGUAGE OverloadedStrings #-}
-- Window lists for the bounded adjustment-area read channel, the scopes that select them, and the
-- plan document the generator writes. Pure: the caller supplies image bytes.
module FujiAF.ReadPlan
  ( Scope(..)
  , scopeName
  , scopeNames
  , parseScope
  , AddressRange(..)
  , ScopeRule(..)
  , scopeRule
  , PlanId
  , planId
  , planIdString
  , Window(..)
  , windowLength
  , maximumWindows
  , oracleOffset
  , oracleByteOffset
  , tableWindowOffsets
  , validateWindows
  , oracleIndex
  , readBytes
  , transferCount
  , Plan(..)
  , buildPlan
  , firmwareBasis
  , planValue
  , showOffset
  ) where

import Data.Aeson (Value, object, (.=))
import Data.Char (isAsciiLower, isAsciiUpper, isDigit)
import Data.List (intercalate, nub)
import qualified Data.Map.Strict as Map
import Data.Word (Word32)
import Numeric (showHex)
import FujiAF.AdjustmentTables

-- The firmware applies no length check on opcode 0x1a0002 function 2, so every request is held to
-- 256 bytes by the host.
windowLength :: Word32
windowLength = 256

-- The read command's journal ceiling admits 2032 windows; the plan schema admits 2048.
maximumWindows :: Int
maximumWindows = 2032

-- The window holding the product string the tag-4 identity reply copies.
oracleOffset :: Word32
oracleOffset = 0x000100

-- Position of that string inside the oracle window: adjustment offset 0x15c.
oracleByteOffset :: Word32
oracleByteOffset = 0x5c

data Scope = Rc0Bound | Rc0Extent | Adjustment42000 | GateByte deriving (Eq, Show, Enum, Bounded)

scopeName :: Scope -> String
scopeName Rc0Bound = "rc0-bound"
scopeName Rc0Extent = "rc0-extent"
scopeName Adjustment42000 = "adjustment-42000"
scopeName GateByte = "gate-byte"

scopeNames :: [String]
scopeNames = map scopeName [minBound .. maxBound]

parseScope :: String -> Either String Scope
parseScope text = case lookup text [(scopeName scope, scope) | scope <- [minBound .. maxBound]] of
  Just scope -> Right scope
  Nothing -> Left ("unknown scope " ++ show text ++ ", expected one of " ++ unwords scopeNames)

-- Inclusive on both ends.
data AddressRange = AddressRange
  { rangeFirst :: Word32
  , rangeLast :: Word32
  , rangePurpose :: String
  } deriving (Eq, Show)

data ScopeRule
  = FromTables Extent
  | FixedRanges [AddressRange]
  deriving (Eq, Show)

-- 0x42000 is the extent 0x0069f204 zeroes on signature failure and the update completion reloads;
-- 0x0073d3ec writes the 0xfd00 block at 0x400000.
scopeRule :: Scope -> ScopeRule
scopeRule Rc0Bound = FromTables RowZeroOnly
scopeRule Rc0Extent = FromTables WholeExtent
scopeRule Adjustment42000 = FixedRanges
  [ AddressRange 0x000000 0x041fff "adjustment area 0x000000-0x041fff that 0x0069f204 zeroes and the update reloads"
  , AddressRange 0x400000 0x40fcff "0xfd00 block at 0x400000 that 0x0073d3ec writes"
  ]
scopeRule GateByte = FixedRanges
  [ AddressRange 0x0000f7 0x0000f7 "gate byte 0xf7 that 0x00647bd4 reads through 0x0069f8fc before the debug text callback"
  ]

newtype PlanId = PlanId String deriving (Eq, Show)

planIdString :: PlanId -> String
planIdString (PlanId text) = text

-- read-plan.schema.json#/properties/plan_id.
planId :: String -> Either String PlanId
planId text
  | null text || length text > 80 = Left "plan id must be 1 to 80 characters"
  | all accepted text = Right (PlanId text)
  | otherwise = Left "plan id accepts only A-Z, a-z, 0-9, hyphen and underscore"
  where accepted c = isAsciiUpper c || isAsciiLower c || isDigit c || c == '-' || c == '_'

data Window = Window
  { windowIndex :: Int
  , windowOffset :: AdjOffset
  , windowPurpose :: String
  } deriving (Eq, Show)

showOffset :: Word32 -> String
showOffset value = "0x" ++ replicate (6 - length digits) '0' ++ digits
  where digits = showHex value ""

-- Windows that carry a known byte rather than a table entry. Every scope includes them.
fixedWindows :: [(Word32, String)]
fixedWindows =
  [ (oracleOffset, "identity oracle: product string at +0x15c, the bytes the tag-4 identity reply copies")
  , (0x000200, "signature at +0x202 that 0x0069f204 compares against 0xaa55")
  , (0x00c000, "flag at +0xc043 read by 0x00d5eed0, an immediate in code rather than a table entry")
  ]

alignDown :: Word32 -> Word32
alignDown offset = offset - offset `mod` windowLength

tableWindowMap :: [TableResolution] -> Map.Map Word32 String
tableWindowMap resolutions = Map.map describe grouped
  where
    grouped = Map.fromListWith (flip (++))
      [ (alignDown offset, [name])
      | resolution <- resolutions
      , let TableName name = tableName (resolvedTable resolution)
      , ResolvedEntry _ (AdjOffset offset) <- resolvedEntries resolution ]
    describe names = "develop tables: " ++ intercalate ", " (nub names)

-- The windows the resolved table entries alone reach, which is what the prototype in
-- private-data/analysis/x-m1-1.50/adj-windows/windows.c counts.
tableWindowOffsets :: [TableResolution] -> [Word32]
tableWindowOffsets = Map.keys . tableWindowMap

rangeWindowMap :: [AddressRange] -> Map.Map Word32 String
rangeWindowMap ranges = Map.fromListWith (\later earlier -> earlier ++ "; " ++ later)
  [ (offset, rangePurpose range)
  | range <- ranges
  , offset <- [alignDown (rangeFirst range), alignDown (rangeFirst range) + windowLength .. rangeLast range] ]

-- Offsets ascend and the index is the array position, which is the order the read command
-- executes. The oracle is named by index.
windowsFrom :: Map.Map Word32 String -> Either String [Window]
windowsFrom selected = do
  let merged = Map.unionWith (\fixed derived -> fixed ++ "; " ++ derived) (Map.fromList fixedWindows) selected
      windows =
        [ Window index (AdjOffset offset) (bounded 256 purpose)
        | (index, (offset, purpose)) <- zip [0 ..] (Map.toAscList merged) ]
  validateWindows windows
  pure windows

bounded :: Int -> String -> String
bounded limit text
  | length text <= limit = text
  | otherwise = take (limit - 3) text ++ "..."

validateWindows :: [Window] -> Either String ()
validateWindows [] = Left "a read plan needs at least one window"
validateWindows windows
  | length windows > maximumWindows =
      Left ("a read plan carries at most " ++ show maximumWindows ++ " windows")
  | otherwise = do
      sequence_ (zipWith checkWindow [0 ..] windows)
      sequence_ (zipWith ascending windows (drop 1 windows))
      _ <- oracleIndex windows
      Right ()
  where
    checkWindow position (Window index (AdjOffset offset) purpose)
      | index /= position = Left ("window index " ++ show index ++ " is not its array position")
      | offset `mod` windowLength /= 0 = Left ("window " ++ showOffset offset ++ " is not 256-aligned")
      | offset > adjustmentAreaLimit - windowLength =
          Left ("window " ++ showOffset offset ++ " reaches past the adjustment area")
      | null purpose || length purpose > 256 =
          Left ("window " ++ showOffset offset ++ " needs a purpose of 1 to 256 characters")
      | otherwise = Right ()
    ascending (Window _ (AdjOffset earlier) _) (Window _ (AdjOffset later) _)
      | earlier < later = Right ()
      | otherwise = Left ("window offsets must ascend: " ++ showOffset earlier ++ " then " ++ showOffset later)

oracleIndex :: [Window] -> Either String Int
oracleIndex windows = case [index | Window index (AdjOffset offset) _ <- windows, offset == oracleOffset] of
  (index : _) -> Right index
  [] -> Left ("every plan carries the oracle window " ++ showOffset oracleOffset)

readBytes :: [Window] -> Int
readBytes windows = length windows * fromIntegral windowLength

-- C0 open is 2 transfers, the tag-4 identity is 4, each window is 4 and C4 close is 2.
transferCount :: [Window] -> Int
transferCount windows = 4 * length windows + 8

data Plan = Plan
  { planScope :: Scope
  , planName :: PlanId
  , planImages :: [DecodedImage]
  , planTables :: [TableResolution]
  , planWindowList :: [Window]
  , planGenerator :: Sha256
  }

-- Table scopes resolve all of developTables from the images; the fixed-range scope reads none.
buildPlan :: Scope -> PlanId -> Sha256 -> [DecodedImage] -> Either String Plan
buildPlan scope name generator images = case scopeRule scope of
  FromTables extent -> do
    set <- imageSet images
    tables <- resolveTables set extent developTables
    windows <- windowsFrom (tableWindowMap tables)
    pure (Plan scope name (imageSetImages set) tables windows generator)
  FixedRanges ranges
    | null images -> do
        windows <- windowsFrom (rangeWindowMap ranges)
        pure (Plan scope name [] [] windows generator)
    | otherwise -> Left ("scope " ++ scopeName scope ++ " is a fixed address range and reads no image")

-- Scope, table extent and every image's load address and SHA-256. A basis that would not fit the
-- schema's 256 characters is refused rather than shortened.
firmwareBasis :: Plan -> Either String String
firmwareBasis plan
  | length basis <= 256 = Right basis
  | otherwise = Left ("firmware basis for " ++ show (length (planImages plan)) ++ " images exceeds 256 characters")
  where
    basis = "1.50 " ++ scopeName (planScope plan) ++ "; " ++ source (scopeRule (planScope plan))
    source (FromTables extent) =
      show (length (planTables plan)) ++ " develop tables " ++ extentName extent ++ "; "
        ++ intercalate "; " (map image (planImages plan))
    source (FixedRanges ranges) =
      "ranges " ++ intercalate ", " [showOffset (rangeFirst range) ++ "-" ++ showOffset (rangeLast range) | range <- ranges]
        ++ "; no image read"
    image decoded =
      let CodeAddress at = imageLoadAddress decoded
      in "image@" ++ showAddress at ++ " sha256 " ++ sha256Text (imageDigest decoded)
    extentName RowZeroOnly = "row-zero"
    extentName WholeExtent = "whole-extent"

planValue :: Plan -> Either String Value
planValue plan = do
  index <- oracleIndex (planWindowList plan)
  basis <- firmwareBasis plan
  pure (object
    [ "schema_version" .= (1 :: Int)
    , "plan_id" .= planIdString (planName plan)
    , "firmware_basis" .= basis
    , "channel" .= object
        [ "opcode" .= ("0x1a0002" :: String)
        , "function" .= (2 :: Int)
        , "mb" .= (1 :: Int)
        , "length" .= (fromIntegral windowLength :: Int)
        ]
    , "windows" .= map windowValue (planWindowList plan)
    , "oracle" .= object
        [ "window_index" .= index
        , "byte_offset" .= ("0x" ++ showHex oracleByteOffset "")
        , "expect" .= ("product-string" :: String)
        ]
    , "generated_by" .= object
        [ "tool" .= ("readplan/read-plan " ++ scopeName (planScope plan))
        , "sha256" .= sha256Text (planGenerator plan)
        ]
    ])
  where
    windowValue (Window index (AdjOffset offset) purpose) = object
      [ "index" .= index
      , "offset" .= showOffset offset
      , "length" .= (fromIntegral windowLength :: Int)
      , "purpose" .= purpose
      ]
