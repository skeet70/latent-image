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

data SiteEntry
  = EntryMarker
  | EntrySettingsConverter
  | EntryCaptureConverter
  | EntryOther SiteLabel
  deriving (Eq)

newtype SiteLabel = SiteLabel T.Text
  deriving (Eq)

data PassParts = PassParts
  { passPrefix :: [SiteEntry]
  , passBody :: [SiteEntry]
  }

parseEntry :: T.Text -> SiteEntry
parseEntry name
  | name == "cc-entry-marker" = EntryMarker
  | name == "cc-entry-settings-converter" = EntrySettingsConverter
  | name == "cc-entry-capture-converter" = EntryCaptureConverter
  | otherwise = EntryOther (SiteLabel name)

entryText :: SiteEntry -> T.Text
entryText EntryMarker = "cc-entry-marker"
entryText EntrySettingsConverter = "cc-entry-settings-converter"
entryText EntryCaptureConverter = "cc-entry-capture-converter"
entryText (EntryOther (SiteLabel name)) = name

isConverterEntry :: SiteEntry -> Bool
isConverterEntry EntrySettingsConverter = True
isConverterEntry EntryCaptureConverter = True
isConverterEntry _ = False

passParts :: [SiteEntry] -> Maybe PassParts
passParts pass = case break isConverterEntry pass of
  (_, []) -> Nothing
  (prefix, body) -> Just (PassParts prefix body)

converts :: [SiteEntry] -> Bool
converts = any isConverterEntry

-- | A converter pass split by idle passes merges only when the continuation repeats the first
-- part's entries before its first converter; the duplicate prefix is dropped once.
passes :: [T.Text] -> [[T.Text]]
passes = map (map entryText) . developPasses . map parseEntry

developPasses :: [SiteEntry] -> [[SiteEntry]]
developPasses entries = trim (map NE.head (NE.group (merge (split entries))))
  where
    split [] = []
    split (first : rest) = let (pass, more) = break (== EntryMarker) rest in (first : pass) : split more
    merge [] = []
    merge (pass : rest)
      | Just parts <- passParts pass = absorb (passPrefix parts) [pass] rest
      | otherwise = pass : merge rest
    absorb prefix parts rest = case span (not . converts) rest of
      (idle, next : after)
        | Just nextParts <- passParts next, passPrefix nextParts == prefix, not (null idle) ->
            absorb prefix (passBody nextParts : parts) after
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
