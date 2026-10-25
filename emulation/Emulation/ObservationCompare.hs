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
import Data.Maybe (fromMaybe)
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

converts :: [T.Text] -> Bool
converts = elem "cc-entry-settings-converter"

-- | A segment's Classic Chrome site entries as live-view develop passes, each starting at
-- `cc-entry-marker`. Consecutive identical passes collapse to one, and the passes before the
-- first and after the last pass that enters `cc-entry-settings-converter` are dropped.
passes :: [T.Text] -> [[T.Text]]
passes entries = trim (map NE.head (NE.group (split entries)))
  where
    split [] = []
    split (first : rest) = let (pass, more) = break (== "cc-entry-marker") rest in (first : pass) : split more
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
