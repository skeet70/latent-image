{-# LANGUAGE OverloadedStrings #-}
-- | Compare a production symbol table with its debuginfo build.
--
-- Data, bss and rodata symbols must keep their names and sizes and move by one
-- uniform address shift. Function size changes are reported, not refused:
-- debuginfo changes LLVM codegen in a few functions but must not change type
-- layout or the data image.
module Replacement.Saclac.SymbolBinding
  ( Symbol(..)
  , SymbolMismatch(..)
  , SymbolReport(..)
  , parseSymbols
  , checkSymbols
  , reportJson
  ) where

import Data.Aeson (Value, object, (.=))
import Data.Char (isHexDigit)
import Data.List (nub, sortOn)
import qualified Data.Map.Strict as M
import Data.Word (Word64)
import Numeric (readHex)

data Symbol = Symbol
  { symbolAddress :: Word64
  , symbolSize :: Word64
  , symbolType :: Char
  , symbolName :: String
  } deriving (Eq, Show)

data SymbolMismatch = SymbolMismatch
  { mismatchSymbol :: String
  , mismatchDetail :: String
  } deriving (Eq, Show)

data SymbolReport = SymbolReport
  { reportVerdict :: String
  , reportDataSymbols :: Int
  , reportUniformShift :: Maybe Integer
  , reportFunctionChanges :: [(String, Word64, Word64)]
  , reportMismatches :: [SymbolMismatch]
  } deriving (Eq, Show)

parseSymbols :: String -> [Symbol]
parseSymbols = concatMap parseLine . lines
  where
    parseLine line =
      case words line of
        addressToken : sizeToken : typeToken : nameTokens@(_ : _) ->
          case (parseHex addressToken, parseHex sizeToken, typeToken) of
            (Just address, Just size, [kind]) ->
              [Symbol address size kind (unwords nameTokens)]
            _ -> []
        _ -> []

parseHex :: String -> Maybe Word64
parseHex token =
  if not (null token) && all isHexDigit token
    then case readHex token of
      ((value, _) : _) -> Just value
      [] -> Nothing
    else Nothing

dataKinds :: String
dataKinds = "dDbBrR"

textKinds :: String
textKinds = "tT"

isData :: Symbol -> Bool
isData symbol = symbolType symbol `elem` dataKinds

isText :: Symbol -> Bool
isText symbol = symbolType symbol `elem` textKinds

checkSymbols :: [Symbol] -> [Symbol] -> SymbolReport
checkSymbols production debug =
  let productionData = dataMap production
      debugData = dataMap debug
      commonNames = sortOn id (M.keys (M.intersection productionData debugData))
      missingInDebug = [name | name <- M.keys productionData, not (M.member name debugData)]
      missingInProduction = [name | name <- M.keys debugData, not (M.member name productionData)]
      sizeMismatches =
        [ SymbolMismatch name ("size " ++ show (symbolSize (productionData M.! name)) ++ " vs " ++ show (symbolSize (debugData M.! name)))
        | name <- commonNames
        , symbolSize (productionData M.! name) /= symbolSize (debugData M.! name)
        ]
      deltas =
        [ toInteger (symbolAddress (debugData M.! name)) - toInteger (symbolAddress (productionData M.! name))
        | name <- commonNames
        ]
      distinctDeltas = nub deltas
      shift = case distinctDeltas of
        [delta] -> Just delta
        _ -> Nothing
      shiftMismatch = case shift of
        Just _ -> []
        Nothing -> [SymbolMismatch "<addresses>" ("non-uniform data shifts " ++ show distinctDeltas)]
      missingMismatches =
        [SymbolMismatch name "missing from debug symbols" | name <- missingInDebug]
          ++ [SymbolMismatch name "missing from production symbols" | name <- missingInProduction]
      textChanges =
        [ (name, symbolSize (productionText M.! name), symbolSize (debugText M.! name))
        | name <- M.keys (M.intersection productionText debugText)
        , symbolSize (productionText M.! name) /= symbolSize (debugText M.! name)
        ]
      productionText = textMap production
      debugText = textMap debug
      mismatches = sizeMismatches ++ shiftMismatch ++ missingMismatches
      verdict = if null mismatches then "match" else "mismatch"
  in SymbolReport verdict (M.size (M.intersection productionData debugData)) shift (sortOn (\(name, _, _) -> name) textChanges) mismatches

dataMap :: [Symbol] -> M.Map String Symbol
dataMap symbols = M.fromList [(symbolName symbol, symbol) | symbol <- symbols, isData symbol]

textMap :: [Symbol] -> M.Map String Symbol
textMap symbols = M.fromList [(symbolName symbol, symbol) | symbol <- symbols, isText symbol]

reportJson :: SymbolReport -> Value
reportJson report =
  object
    [ "verdict" .= reportVerdict report
    , "data_symbols" .= reportDataSymbols report
    , "uniform_shift" .= reportUniformShift report
    , "function_size_changes" .=
        [ object ["name" .= name, "production" .= production, "debug" .= debug]
        | (name, production, debug) <- reportFunctionChanges report
        ]
    , "mismatches" .=
        [ object ["symbol" .= mismatchSymbol mismatch, "detail" .= mismatchDetail mismatch]
        | mismatch <- reportMismatches report
        ]
    ]
