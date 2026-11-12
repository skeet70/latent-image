{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}

-- | Stock-side branch coverage from the native runner's per-extent bitmaps.
--
-- The runner reports, for every declared function extent, one bitmap of the
-- words it entered and two bitmaps of the condition outcomes those words
-- evaluated. This module folds the reports of a cohort over a declared list of
-- stock function extents and answers, per extent, which conditional branches
-- were reached in both directions, in one direction or not at all, and which
-- instructions never executed.
--
-- An extent is authenticated against the census digest when the census names
-- one: the bytes come from a report's final memory and must hash to that
-- digest. A report that declares the extent but carries different bytes is
-- counted as foreign and contributes no coverage. Without a census digest the
-- declared rows are accepted and the extent is reported unauthenticated.
module Emulation.Coverage
  ( -- * Declared extents
    Extent (..)
  , parseExtents
    -- * Runner observations
  , ExtentObservation (..)
  , Observation (..)
  , parseObservation
    -- * Coverage
  , Direction (..)
  , directionLabel
  , Branch (..)
  , FunctionCoverage (..)
  , Coverage (..)
  , coverage
  , CoverageFold
  , emptyCoverageFold
  , addCoverageObservation
  , finishCoverage
  , coverageJson
  , coverageTable
    -- * Instruction classification
  , conditionalBranch
  , conditionName
  ) where

import Data.Aeson (Value (..), eitherDecodeStrict', encode, object, (.=))
import qualified Data.Aeson.Key as K
import qualified Data.Aeson.KeyMap as KM
import qualified Data.Bits as Bits
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as BL
import Data.List (sortOn)
import Data.Maybe (listToMaybe, mapMaybe)
import Data.Scientific (toBoundedInteger)
import qualified Data.IntSet as IS
import qualified Data.Text as T
import qualified Data.Text.Encoding as TE
import qualified Data.Vector as V
import Data.Word (Word32)
import Emulation.Sha256 (sha256, sha256Hex)
import Emulation.SuiteAudit (parseWord32)
import Numeric (showHex)
import Text.Printf (printf)

-- | One declared stock function extent. @extentStock@ is the census digest of
-- its stock bytes, when the census carries one.
data Extent = Extent
  { extentEntry :: Word32
  , extentBytes :: Word32
  , extentStock :: Maybe String
  } deriving (Eq, Show)

-- | Parse a census file: either @{"entries":[...]}@ as
-- @emulation/suite-audit-cases/*.json@ writes it, or a bare array. A row names
-- @entry@ (or @base@/@address@), @extent@ (or @bytes@/@size@) and optionally
-- @stock.sha256@ (or @sha256@).
parseExtents :: B.ByteString -> Either String [Extent]
parseExtents bytes = do
  value <- eitherDecodeStrict' bytes
  rows <- case value of
    Object root -> case KM.lookup "entries" root of
      Just (Array entries) -> pure (V.toList entries)
      Just _ -> Left "extents: entries is not an array"
      Nothing -> Left "extents: object has no entries array"
    Array entries -> pure (V.toList entries)
    _ -> Left "extents must be a JSON object with entries or an array"
  mapM parseExtent rows

parseExtent :: Value -> Either String Extent
parseExtent (Object row) = do
  entry <- case listToMaybe (mapMaybe (flip KM.lookup row . K.fromString) ["entry", "base", "address"]) of
    Just (String text) -> maybe (Left "extent entry is not an address") Right (parseWord32 (T.unpack text))
    Just (Number _) -> maybe (Left "extent entry is not an address") Right (fromIntegral <$> toBoundedIntegerValue row ["entry", "base", "address"])
    _ -> Left "extent row has none of entry, base, address"
  size <- case listToMaybe (mapMaybe (flip KM.lookup row . K.fromString) ["extent", "bytes", "size"]) of
    Just (String text) -> case reads (T.unpack text) of
      [(number, "")] | number >= 0 && number <= toInteger (maxBound :: Word32) -> Right (fromIntegral (number :: Integer))
      _ -> Left "extent size is not a positive word count"
    Just (Number _) -> maybe (Left "extent size is not a positive word count") (Right . fromIntegral) (toBoundedIntegerValue row ["extent", "bytes", "size"])
    _ -> Left "extent row has none of extent, bytes, size"
  if size == 0 then Left "extent size must be positive" else pure ()
  let stock = case KM.lookup "stock" row of
        Just (Object nested) -> textField nested "sha256"
        _ -> textField row "sha256"
  pure Extent { extentEntry = entry, extentBytes = size, extentStock = stock }
parseExtent _ = Left "extent row must be an object"

toBoundedIntegerValue :: KM.KeyMap Value -> [String] -> Maybe Word32
toBoundedIntegerValue row names = listToMaybe
  [ fromIntegral value
  | name <- names
  , Just (Number number) <- [KM.lookup (K.fromString name) row]
  , Just value <- [toBoundedInteger number :: Maybe Word32]
  ]

textField :: KM.KeyMap Value -> String -> Maybe String
textField row key = case KM.lookup (K.fromString key) row of
  Just (String text) -> Just (T.unpack text)
  _ -> Nothing

-- | One report's row for a declared extent: the three bitmaps it returned.
data ExtentObservation = ExtentObservation
  { extentObservationBase :: Word32
  , extentObservationBytes :: Word32
  , extentObservationExecuted :: B.ByteString
  , extentObservationPassed :: B.ByteString
  , extentObservationFailed :: B.ByteString
  } deriving (Eq, Show)

-- | The parts of one runner observation this tool reads: the extent rows, the
-- extents the runner could not observe, and the final memory the stock bytes
-- are authenticated from.
data Observation = Observation
  { observationExtents :: [ExtentObservation]
  , observationUnobserved :: [(Word32, Word32)]
  , observationMemory :: [(Word32, B.ByteString)]
  } deriving (Eq, Show)

-- | Parse a runner report: a judged @native-result.json@ whose observation is
-- under @runner@, or a bare observation object.
parseObservation :: B.ByteString -> Either String Observation
parseObservation bytes = do
  value <- eitherDecodeStrict' bytes
  let runner = case value of
        Object root | KM.member "runner" root -> getValue value "runner"
        _ -> value
  extents <- mapM parseExtentRow (listOf runner "executed")
  unobserved <- mapM parseUnobservedRow (listOf runner "executed_unobserved")
  memory <- mapM parseMemoryRow (listOf runner "memory")
  pure Observation
    { observationExtents = mapMaybe id extents
    , observationUnobserved = mapMaybe id unobserved
    , observationMemory = mapMaybe id memory
    }

parseExtentRow :: Value -> Either String (Maybe ExtentObservation)
parseExtentRow (Object row) = do
  base <- addressField row "base"
  size <- numberField row "bytes"
  executed <- hexField row "executed"
  passed <- hexField row "condition_passed"
  failed <- hexField row "condition_failed"
  pure (Just ExtentObservation
    { extentObservationBase = base
    , extentObservationBytes = size
    , extentObservationExecuted = executed
    , extentObservationPassed = passed
    , extentObservationFailed = failed
    })
parseExtentRow _ = Left "executed row must be an object"

parseUnobservedRow :: Value -> Either String (Maybe (Word32, Word32))
parseUnobservedRow (Object row) = do
  base <- addressField row "base"
  size <- numberField row "bytes"
  pure (Just (base, size))
parseUnobservedRow _ = Left "executed_unobserved row must be an object"

parseMemoryRow :: Value -> Either String (Maybe (Word32, B.ByteString))
parseMemoryRow (Object row) = do
  base <- addressField row "address"
  case KM.lookup "bytes" row of
    Just (String text) -> do
      bytes <- hexBytes text
      pure (Just (base, bytes))
    _ -> pure Nothing
parseMemoryRow _ = Left "memory row must be an object"

addressField :: KM.KeyMap Value -> String -> Either String Word32
addressField row key = case KM.lookup (K.fromString key) row of
  Just (String text) -> maybe (Left (key ++ " is not an address")) Right (parseWord32 (T.unpack text))
  Just (Number _) -> maybe (Left (key ++ " is not an address")) Right (fromIntegral <$> toBoundedIntegerValue row [key])
  _ -> Left ("missing address " ++ key)

numberField :: KM.KeyMap Value -> String -> Either String Word32
numberField row key = case KM.lookup (K.fromString key) row of
  Just (String text) -> case reads (T.unpack text) of
    [(number, "")] | number >= 0 && number <= toInteger (maxBound :: Word32) -> Right (fromIntegral (number :: Integer))
    _ -> Left (key ++ " is not a word count")
  Just (Number _) -> maybe (Left (key ++ " is not a word count")) Right (toBoundedIntegerValue row [key])
  _ -> Left ("missing count " ++ key)

hexField :: KM.KeyMap Value -> String -> Either String B.ByteString
hexField row key = case KM.lookup (K.fromString key) row of
  Just (String text) -> hexBytes text
  _ -> Left ("missing bitmap " ++ key)

hexBytes :: T.Text -> Either String B.ByteString
hexBytes text
  | odd (B.length raw) = Left "bitmap has an odd number of hexadecimal digits"
  | not (B.all isHex raw) = Left "bitmap is not hexadecimal"
  | otherwise = Right (fst (B.unfoldrN (B.length raw `div` 2) step raw))
  where
    raw = TE.encodeUtf8 text
    isHex byte =
      (byte >= 0x30 && byte <= 0x39) || (byte >= 0x61 && byte <= 0x66) || (byte >= 0x41 && byte <= 0x46)
    step rest
      | B.null rest = Nothing
      | otherwise = Just (hexValue (B.head rest) * 16 + hexValue (B.index rest 1), B.drop 2 rest)
    hexValue byte
      | byte <= 0x39 = byte - 0x30
      | byte <= 0x46 = byte - 0x41 + 10
      | otherwise = byte - 0x61 + 10

getValue :: Value -> String -> Value
getValue (Object objectValue) key = maybe Null id (KM.lookup (K.fromString key) objectValue)
getValue _ _ = Null

listOf :: Value -> String -> [Value]
listOf value key = case getValue value key of
  Array entries -> V.toList entries
  _ -> []

-- | The directions a conditional branch can take across a cohort.
data Direction = Both | PassedOnly | FailedOnly | Never | Unknown
  deriving (Eq, Show)

directionLabel :: Direction -> String
directionLabel Both = "both"
directionLabel PassedOnly = "passed-only"
directionLabel FailedOnly = "failed-only"
directionLabel Never = "never"
directionLabel Unknown = "unknown"

-- | One conditional branch and the directions the cohort reached.
data Branch = Branch
  { branchAddress :: Word32
  , branchCondition :: String
  , branchDirection :: Direction
  } deriving (Eq, Show)

-- | One function extent's aggregate coverage.
data FunctionCoverage = FunctionCoverage
  { functionEntry :: Word32
  , functionBytes :: Word32
  , functionStock :: Maybe String
  , functionAuthenticated :: Bool
  , functionDeclaredCases :: Int
  , functionUnobservedCases :: Int
  , functionForeignCases :: Int
  , functionBranches :: [Branch]
  , functionUnexecuted :: [(Word32, Maybe Word32)]
  } deriving (Eq, Show)

data Coverage = Coverage
  { coverageFunctions :: [FunctionCoverage]
  } deriving (Eq, Show)

-- | Fold every report over the declared extents. @reports@ is ordered as the
-- caller read it; the result is ordered by extent entry.
coverage :: [Extent] -> [Observation] -> Coverage
coverage extents observations =
  finishCoverage (foldl' addCoverageObservation (emptyCoverageFold extents) observations)

-- | The same fold, one report at a time, so a caller that reads reports from
-- disk does not hold them all.
newtype CoverageFold = CoverageFold ([Extent], [Aggregate])

emptyCoverageFold :: [Extent] -> CoverageFold
emptyCoverageFold extents = CoverageFold (extents, map (const emptyAggregate) extents)

addCoverageObservation :: CoverageFold -> Observation -> CoverageFold
addCoverageObservation (CoverageFold (extents, aggregates)) observation =
  let aggregates' = forceAll (zipWith addOne extents aggregates)
   in aggregates' `seq` CoverageFold (extents, aggregates')
  where
    addOne extent aggregate = strictAggregate (addObservationToAggregate extent aggregate observation)
    forceAll [] = []
    forceAll (aggregate : rest) =
      let forced = forceAggregate aggregate
          forcedRest = forceAll rest
       in forced `seq` forcedRest `seq` (forced : forcedRest)

forceAggregate :: Aggregate -> Aggregate
forceAggregate aggregate =
  aggregateDeclared aggregate `seq`
  aggregateUnobserved aggregate `seq`
  aggregateForeign aggregate `seq`
  aggregateExecuted aggregate `seq`
  aggregatePassed aggregate `seq`
  aggregateFailed aggregate `seq`
  aggregateBytes aggregate `seq`
  aggregate

finishCoverage :: CoverageFold -> Coverage
finishCoverage (CoverageFold (extents, aggregates)) =
  Coverage { coverageFunctions = sortOn functionEntry (zipWith finishExtent extents aggregates) }

strictAggregate :: Aggregate -> Aggregate
strictAggregate aggregate =
  aggregateDeclared aggregate `seq`
  aggregateUnobserved aggregate `seq`
  aggregateForeign aggregate `seq`
  aggregate
    { aggregateExecuted = strictSet (aggregateExecuted aggregate)
    , aggregatePassed = strictSet (aggregatePassed aggregate)
    , aggregateFailed = strictSet (aggregateFailed aggregate)
    , aggregateBytes = case aggregateBytes aggregate of
        Just bytes -> B.length bytes `seq` Just bytes
        Nothing -> Nothing
    }
  where
    strictSet set = IS.foldl' (\n _ -> n + 1) (0 :: Int) set `seq` set

data Aggregate = Aggregate
  { aggregateDeclared :: Int
  , aggregateUnobserved :: Int
  , aggregateForeign :: Int
  , aggregateExecuted :: IS.IntSet
  , aggregatePassed :: IS.IntSet
  , aggregateFailed :: IS.IntSet
  , aggregateBytes :: Maybe B.ByteString
  }

emptyAggregate :: Aggregate
emptyAggregate = Aggregate 0 0 0 IS.empty IS.empty IS.empty Nothing

finishExtent :: Extent -> Aggregate -> FunctionCoverage
finishExtent extent aggregate =
  let words_ = wordCount (extentBytes extent)
      stock = aggregateBytes aggregate
      authenticated = case extentStock extent of
        Just digest -> maybe False ((== digest) . sha256Hex . sha256) stock
        Nothing -> False
      branches =
        [ Branch (addressOf extent index) (conditionName condition) (direction aggregate index)
        | index <- [0 .. words_ - 1]
        , Just (condition, _) <- [conditionalBranch =<< stockWord stock index]
        ]
      unexecuted =
        [ (addressOf extent index, stockWord stock index)
        | index <- [0 .. words_ - 1]
        , not (IS.member index (aggregateExecuted aggregate))
        ]
   in FunctionCoverage
        { functionEntry = extentEntry extent
        , functionBytes = extentBytes extent
        , functionStock = extentStock extent
        , functionAuthenticated = authenticated
        , functionDeclaredCases = aggregateDeclared aggregate
        , functionUnobservedCases = aggregateUnobserved aggregate
        , functionForeignCases = aggregateForeign aggregate
        , functionBranches = branches
        , functionUnexecuted = unexecuted
        }

addObservationToAggregate :: Extent -> Aggregate -> Observation -> Aggregate
addObservationToAggregate extent aggregate observation =
  let entry = extentEntry extent
      size = extentBytes extent
      end = toInteger entry + toInteger size
      contains base bytes = toInteger base <= toInteger entry && end <= toInteger base + toInteger bytes
      declared = [ row | row <- observationExtents observation
                  , contains (extentObservationBase row) (extentObservationBytes row) ]
      unobserved = [ () | (base, bytes) <- observationUnobserved observation, contains base bytes ]
      accepted = mapMaybe (accept observation) declared
      foreignCases = length declared - length accepted
      merge accumulator row =
        let base = extentObservationBase row
            offset = fromIntegral ((toInteger entry - toInteger base) `div` 4)
            width = wordCount (extentObservationBytes row)
            window bitmap = subBits offset (wordCount size) (bits width bitmap)
         in accumulator
              { aggregateExecuted = IS.union (aggregateExecuted accumulator) (window (extentObservationExecuted row))
              , aggregatePassed = IS.union (aggregatePassed accumulator) (window (extentObservationPassed row))
              , aggregateFailed = IS.union (aggregateFailed accumulator) (window (extentObservationFailed row))
              , aggregateBytes = case aggregateBytes accumulator of
                  Just bytes -> Just bytes
                  Nothing -> extentBytesAt observation extent
              }
   in case accepted of
        [] -> aggregate
          { aggregateUnobserved = aggregateUnobserved aggregate + length unobserved
          , aggregateForeign = aggregateForeign aggregate + foreignCases
          }
        rows -> foldl' merge aggregate
          { aggregateDeclared = aggregateDeclared aggregate + length rows
          , aggregateForeign = aggregateForeign aggregate + foreignCases
          } rows
  where
    accept reported row = case extentStock extent of
      Nothing -> Just row
      Just digest -> case extentBytesAt reported extent of
        Just bytes | sha256Hex (sha256 bytes) == digest -> Just row
        _ -> Nothing

-- | The stock bytes of an extent from one report's final memory, when a region
-- covers the whole extent.
extentBytesAt :: Observation -> Extent -> Maybe B.ByteString
extentBytesAt observation extent =
  listToMaybe
    [ B.take size (B.drop (fromIntegral offset) bytes)
    | (base, bytes) <- observationMemory observation
    , let offset = toInteger (extentEntry extent) - toInteger base
    , offset >= 0
    , offset + toInteger (extentBytes extent) <= toInteger (B.length bytes)
    , let size = fromIntegral (extentBytes extent)
    ]

direction :: Aggregate -> Int -> Direction
direction aggregate index
  | not (IS.member index (aggregateExecuted aggregate)) = Never
  | IS.member index (aggregatePassed aggregate) && IS.member index (aggregateFailed aggregate) = Both
  | IS.member index (aggregatePassed aggregate) = PassedOnly
  | IS.member index (aggregateFailed aggregate) = FailedOnly
  | otherwise = Unknown

wordCount :: Word32 -> Int
wordCount bytes = fromIntegral ((toInteger bytes + 3) `div` 4)

addressOf :: Extent -> Int -> Word32
addressOf extent index = extentEntry extent + fromIntegral index * 4

bits :: Int -> B.ByteString -> IS.IntSet
bits words_ bitmap = IS.fromList
  [ index
  | index <- [0 .. words_ - 1]
  , let byte = index `div` 8
  , byte < B.length bitmap
  , Bits.testBit (B.index bitmap byte) (index `mod` 8)
  ]

-- | The word indices of one census extent inside a declared row's bitmap.
subBits :: Int -> Int -> IS.IntSet -> IS.IntSet
subBits offset width source = IS.fromList
  [ index - offset
  | index <- IS.toList source
  , index >= offset
  , index < offset + width
  ]

stockWord :: Maybe B.ByteString -> Int -> Maybe Word32
stockWord Nothing _ = Nothing
stockWord (Just bytes) index =
  let offset = index * 4
   in if offset + 4 <= B.length bytes
        then Just (fromIntegral (B.index bytes offset)
          Bits..|. (fromIntegral (B.index bytes (offset + 1)) `Bits.shiftL` 8)
          Bits..|. (fromIntegral (B.index bytes (offset + 2)) `Bits.shiftL` 16)
          Bits..|. (fromIntegral (B.index bytes (offset + 3)) `Bits.shiftL` 24))
        else Nothing

-- | The condition of a conditional branch word, or @Nothing@ for a word that
-- is not a branch or whose condition is AL/NV. Recognised forms are @B@, @BL@,
-- @BX@, @BLX@ (register), @LDR pc@ and @LDM@/@POP@ with PC in the list.
conditionalBranch :: Word32 -> Maybe (Word32, Word32)
conditionalBranch word
  | condition >= 0xe = Nothing
  | isBranch = Just (condition, word)
  | otherwise = Nothing
  where
    condition = word `Bits.shiftR` 28
    isBranch =
      word Bits..&. 0x0e000000 == 0x0a000000
        || word Bits..&. 0x0ffffff0 `elem` [0x012fff10, 0x012fff30]
        || word Bits..&. 0x0c10f000 == 0x0410f000
        || (word Bits..&. 0x0e000000 == 0x08000000 && word Bits..&. 0x00108000 == 0x00108000)

conditionName :: Word32 -> String
conditionName condition = case condition of
  0x0 -> "eq"
  0x1 -> "ne"
  0x2 -> "cs"
  0x3 -> "cc"
  0x4 -> "mi"
  0x5 -> "pl"
  0x6 -> "vs"
  0x7 -> "vc"
  0x8 -> "hi"
  0x9 -> "ls"
  0xa -> "ge"
  0xb -> "lt"
  0xc -> "gt"
  0xd -> "le"
  0xe -> "al"
  _ -> "nv"

-- | The JSON document: the functions with their branches and unexecuted
-- instructions, plus cohort totals.
coverageJson :: Coverage -> BL.ByteString
coverageJson result = encode $ object
  [ "schema" .= (1 :: Int)
  , "functions" .= map functionJson (coverageFunctions result)
  , "totals" .= object
      [ "functions" .= length functions
      , "declared_cases" .= sum (map functionDeclaredCases functions)
      , "unobserved_cases" .= sum (map functionUnobservedCases functions)
      , "foreign_cases" .= sum (map functionForeignCases functions)
      , "branches" .= length branches
      , "both" .= countDirection Both
      , "one_direction" .= (countDirection PassedOnly + countDirection FailedOnly)
      , "never" .= countDirection Never
      , "unexecuted_instructions" .= sum (map (length . functionUnexecuted) functions)
      ]
  ]
  where
    functions = coverageFunctions result
    branches = concatMap functionBranches functions
    countDirection wanted = length [() | branch <- branches, branchDirection branch == wanted]

functionJson :: FunctionCoverage -> Value
functionJson function = object
  [ "entry" .= hex32 (functionEntry function)
  , "bytes" .= functionBytes function
  , "stock_sha256" .= functionStock function
  , "authenticated" .= functionAuthenticated function
  , "declared_cases" .= functionDeclaredCases function
  , "unobserved_cases" .= functionUnobservedCases function
  , "foreign_cases" .= functionForeignCases function
  , "branches" .=
      [ object
          [ "address" .= hex32 (branchAddress branch)
          , "condition" .= branchCondition branch
          , "direction" .= directionLabel (branchDirection branch)
          ]
      | branch <- functionBranches function
      ]
  , "unexecuted" .=
      [ object
          [ "address" .= hex32 address
          , "word" .= maybe Null (String . T.pack . hex32) word
          ]
      | (address, word) <- functionUnexecuted function
      ]
  ]

-- | The human-readable table: one row per function, then one row per
-- conditional branch and one per unexecuted instruction.
coverageTable :: Coverage -> String
coverageTable result = unlines (functionTable ++ [""] ++ branchTable ++ [""] ++ unexecutedTable)
  where
    functions = coverageFunctions result
    functionTable =
      [ printf "%-12s %6s %6s %11s %6s %6s %6s %8s %6s %6s"
          ("function" :: String) ("bytes" :: String) ("cases" :: String) ("unobserved" :: String)
          ("foreign" :: String) ("both" :: String) ("one" :: String) ("never" :: String)
          ("unexec" :: String) ("auth" :: String)
      ] ++
      [ printf "%-12s %6d %6d %11d %6d %6d %6d %8d %6d %6s"
          (hex32 (functionEntry function)) (functionBytes function) (functionDeclaredCases function)
          (functionUnobservedCases function) (functionForeignCases function)
          (count Both function) (count PassedOnly function + count FailedOnly function)
          (count Never function) (length (functionUnexecuted function))
          (if functionAuthenticated function then "yes" else "no" :: String)
      | function <- functions
      ]
    branchTable =
      [ printf "%-12s %-12s %-9s %s" ("function" :: String) ("address" :: String) ("condition" :: String) ("direction" :: String)
      ] ++
      [ printf "%-12s %-12s %-9s %s"
          (hex32 (functionEntry function)) (hex32 (branchAddress branch))
          (branchCondition branch) (directionLabel (branchDirection branch))
      | function <- functions, branch <- functionBranches function
      ]
    unexecutedTable =
      [ printf "%-12s %-12s %s" ("function" :: String) ("address" :: String) ("word" :: String)
      ] ++
      [ printf "%-12s %-12s %s"
          (hex32 (functionEntry function)) (hex32 address)
          (maybe "-" hex32 word)
      | function <- functions, (address, word) <- functionUnexecuted function
      ]

count :: Direction -> FunctionCoverage -> Int
count wanted function = length [() | branch <- functionBranches function, branchDirection branch == wanted]

hex32 :: Word32 -> String
hex32 value = "0x" ++ replicate (8 - length text) '0' ++ text
  where
    text = showHex value ""
