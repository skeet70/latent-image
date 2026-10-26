{-# LANGUAGE OverloadedStrings #-}
-- | The comparison behind //emulation:e2e-observation-compare: receipt values with their
-- timing and location fields left out, and the paths where two of them differ.
module Emulation.ObservationCompare
  ( e2eIgnored
  , toggleIgnored
  , normalize
  , passes
  , differences
  ) where

import Data.Aeson (Value (..), encode)
import qualified Data.Aeson.Key as K
import qualified Data.Aeson.KeyMap as KM
import qualified Data.ByteString.Lazy.Char8 as BL
import Data.List (dropWhileEnd, sort)
import qualified Data.List.NonEmpty as NE
import Data.Maybe (fromMaybe, isJust, isNothing)
import qualified Data.Text as T
import qualified Data.Vector as V

e2eIgnored :: [K.Key]
e2eIgnored = ["seconds", "instructions", "virtual_ticks", "sp", "path", "frame_number", "sha256"]

-- | Also the access probe's device-model transfers, which come from each engine's own models;
-- the C0 session id, which differs between hardware sessions too; and an attach's cells at the
-- first settle round its personality predicate accepts, a moment set by guest timing.
toggleIgnored :: [K.Key]
toggleIgnored = e2eIgnored ++
  [ "engine", "emulator", "directory", "image_wide_transfers_covering_extent", "sid", "cells_after_start" ]

-- | The two converter sites' entries.
converterEntry :: T.Text -> Bool
converterEntry name = name `elem` ["cc-entry-settings-converter", "cc-entry-capture-converter"]

-- | A pass split at its first converter: the entries before it, and the rest.
converterSplit :: [T.Text] -> Maybe ([T.Text], [T.Text])
converterSplit pass = case break converterEntry pass of
  (_, []) -> Nothing
  (prefix, body) -> Just (prefix, body)

converts :: [T.Text] -> Bool
converts = isJust . converterSplit

-- | A segment's Classic Chrome site entries as live-view develop passes, each starting at
-- `cc-entry-marker`. A converter pass split by idle passes counts as one pass when every pass
-- between the parts is idle and a continuation's entries before its first converter equal the
-- first part's. The merge drops that duplicated prefix and keeps every remaining entry of the
-- continuation in order. Consecutive identical passes collapse to one, and the passes before
-- the first and after the last converter pass are dropped.
passes :: [T.Text] -> [[T.Text]]
passes entries = trim (map NE.head (NE.group (merge (split entries))))
  where
    split [] = []
    split (first : rest) = let (pass, more) = break (== "cc-entry-marker") rest in (first : pass) : split more
    merge [] = []
    merge (pass : rest)
      | Just (prefix, _) <- converterSplit pass = absorb prefix [pass] rest
      | otherwise = pass : merge rest
    -- Parts are collected in reverse and concatenated once per merged pass.
    absorb prefix parts rest = case span (isNothing . converterSplit) rest of
      (idle, next : after)
        | Just (prefix', body) <- converterSplit next, prefix' == prefix, not (null idle) ->
            absorb prefix (body : parts) after
      _ -> concat (reverse parts) : merge rest
    trim = dropWhileEnd (not . converts) . dropWhile (not . converts)

-- | VALUE without the fields named in KEYS at any depth. A segment's
-- @classic_chrome_site_entries@ become its 'passes' when one of them converts, and stay
-- the exact list otherwise.
normalize :: [K.Key] -> Value -> Value
normalize keys (Object fields) = Object (KM.mapWithKey site (KM.filterWithKey (\key _ -> key `notElem` keys) fields))
  where
    site "classic_chrome_site_entries" (Array items)
      | Just names <- mapM text (V.toList items), let developed = passes names, not (null developed) =
          Array (V.fromList [Array (V.fromList (map String pass)) | pass <- developed])
    site _ value = normalize keys value
    text (String name) = Just name
    text _ = Nothing
normalize keys (Array items) = Array (V.map (normalize keys) items)
normalize _ value = value

-- | Paths below AT where the two values differ.
differences :: String -> Value -> Value -> [String]
differences at (Object a) (Object b) =
  concat [ differences (at ++ "." ++ K.toString key) (field a) (field b)
         | key <- sort (KM.keys (KM.union a b)), let field = fromMaybe Null . KM.lookup key ]
differences at (Array a) (Array b)
  | V.length a == V.length b = concat (zipWith3 (\index x y -> differences (at ++ "[" ++ show index ++ "]") x y) [0 :: Int ..] (V.toList a) (V.toList b))
differences at a b
  | a == b = []
  | otherwise = [at ++ ": " ++ short a ++ " != " ++ short b]
  where short = take 160 . BL.unpack . encode
