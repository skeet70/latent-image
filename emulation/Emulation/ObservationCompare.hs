{-# LANGUAGE OverloadedStrings #-}
-- | The comparison behind //emulation:e2e-observation-compare: receipt values with their
-- timing and location fields left out, and the paths where two of them differ.
module Emulation.ObservationCompare
  ( e2eIgnored
  , toggleIgnored
  , normalize
  , differences
  , compareObservationFiles
  ) where

import Control.Exception (IOException, try)
import Control.Monad (forM_)
import Data.Aeson (Value (..), eitherDecodeFileStrict', encode, object, (.=))
import qualified Data.Aeson.Key as K
import qualified Data.Aeson.KeyMap as KM
import qualified Data.ByteString.Lazy.Char8 as BL
import Data.IORef (modifyIORef', newIORef, readIORef, writeIORef)
import Data.List (sort)
import qualified Data.List.NonEmpty as NE
import Data.Maybe (fromMaybe)
import qualified Data.Text as T
import qualified Data.Vector as V
import Emulation.ObservationStream (RowReader, rowNext, withRowReader)
import System.FilePath (takeDirectory)

-- | Fields left out at any depth. Only FujiMod menu receipts carry @scanout_evidence@.
e2eIgnored :: [K.Key]
e2eIgnored = ["seconds", "instructions", "virtual_ticks", "sp", "path", "frame_number", "sha256", "scanout_evidence"]

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

-- | A converting segment's site entries as two sequences compared apart: every converter entry
-- in order, and the develop passes with their converter entries removed, consecutive identical
-- passes counted once. How the two interleave is not kept.
data SiteSequences = SiteSequences [T.Text] [[T.Text]]

-- | 'Nothing' when no entry converts.
siteSequences :: [T.Text] -> Maybe SiteSequences
siteSequences names
  | not (any isConverterEntry entries) = Nothing
  | otherwise = Just (SiteSequences (map entryText (filter isConverterEntry entries)) (map (map entryText) developed))
  where
    entries = map parseEntry names
    developed = map NE.head (NE.group (filter (not . null) (map (filter (not . isConverterEntry)) (split entries))))
    split [] = []
    split (first : rest) = let (pass, more) = break (== EntryMarker) rest in (first : pass) : split more

-- | VALUE without the fields named in KEYS at any depth. A segment's
-- @classic_chrome_site_entries@ become its 'siteSequences' when one of them converts, and stay
-- the exact list otherwise.
normalize :: [K.Key] -> Value -> Value
normalize keys (Object fields) = Object (KM.mapWithKey site (KM.filterWithKey (\key _ -> key `notElem` keys) fields))
  where
    site "classic_chrome_site_entries" (Array items)
      | Just names <- mapM text (V.toList items), Just (SiteSequences converters developed) <- siteSequences names =
          object ["converters" .= converters, "passes" .= developed]
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

-- | Compares two observation files, including the trace hits each records inline or in a
-- sidecar its reference names. Row fields keep 'e2eIgnored' semantics, so registers and read
-- bytes are compared; a reference's path is not compared, but its sidecar is read to EOF and
-- checked against its count and digest. Both streams are read to EOF even after a row differs.
compareObservationFiles :: FilePath -> FilePath -> String -> IO [String]
compareObservationFiles left right prefix = do
  loadedLeft <- loadObservation left
  loadedRight <- loadObservation right
  case (loadedLeft, loadedRight) of
    (Right leftValue, Right rightValue) -> do
      let base = differences prefix
            (normalize e2eIgnored (withoutTraceHits leftValue))
            (normalize e2eIgnored (withoutTraceHits rightValue))
      traced <- compareTraceHits (takeDirectory left) leftValue (takeDirectory right) rightValue prefix
      pure (base ++ traced)
    (Left problem, Left other) -> pure [prefix ++ ": " ++ problem, prefix ++ ": " ++ other]
    (Left problem, _) -> pure [prefix ++ ": " ++ problem]
    (_, Left problem) -> pure [prefix ++ ": " ++ problem]

loadObservation :: FilePath -> IO (Either String Value)
loadObservation path = do
  loaded <- try (eitherDecodeFileStrict' path) :: IO (Either IOException (Either String Value))
  pure $ case loaded of
    Left problem -> Left (path ++ ": " ++ show problem)
    Right (Left problem) -> Left (path ++ ": " ++ problem)
    Right (Right value) -> Right value

withoutTraceHits :: Value -> Value
withoutTraceHits (Object fields) = Object (KM.delete "trace_hits" fields)
withoutTraceHits value = value

-- | A stream that reports a problem stops being read here; 'withRowReader' still finishes it,
-- which drains a sidecar and checks its count and digest. A problem observed while walking is
-- not reported a second time by the finish.
compareTraceHits :: FilePath -> Value -> FilePath -> Value -> String -> IO [String]
compareTraceHits leftDirectory leftValue rightDirectory rightValue prefix = do
  found <- newIORef []
  entered <- newIORef False
  let record message = modifyIORef' found (message :)
      problem stream message = record (prefix ++ ".trace_hits: " ++ stream ++ ": " ++ message)
      rowPath index = prefix ++ ".trace_hits[" ++ show index ++ "]"
      rows index leftRow rightRow =
        forM_ (differences (rowPath index) (normalize e2eIgnored leftRow) (normalize e2eIgnored rightRow)) record
  compared <- withRowReader leftDirectory leftValue $ \leftReader ->
    withRowReader rightDirectory rightValue $ \rightReader -> do
      writeIORef entered True
      let walk leftDone rightDone index = do
            leftNext <- if leftDone then pure (Right Nothing) else rowNext leftReader
            rightNext <- if rightDone then pure (Right Nothing) else rowNext rightReader
            case (leftNext, rightNext) of
              (Right Nothing, Right Nothing) -> pure ()
              (Right (Just leftRow), Right (Just rightRow)) -> rows index leftRow rightRow >> walk False False (index + 1)
              (Right Nothing, Right (Just _)) ->
                record (prefix ++ ".trace_hits: row counts differ") >> drainRows rightReader (problem "candidate")
              (Right (Just _), Right Nothing) ->
                record (prefix ++ ".trace_hits: row counts differ") >> drainRows leftReader (problem "retained")
              (Left message, Left other) -> problem "retained" message >> problem "candidate" other
              (Left message, Right Nothing) -> problem "retained" message
              (Left message, Right (Just _)) -> problem "retained" message >> drainRows rightReader (problem "candidate")
              (Right Nothing, Left message) -> problem "candidate" message
              (Right (Just _), Left message) -> problem "candidate" message >> drainRows leftReader (problem "retained")
      walk False False (0 :: Int)
      pure (Right ())
  walked <- readIORef entered
  case compared of
    Right () -> pure ()
    Left message
      | walked -> pure ()
      | otherwise -> do
          before <- length <$> readIORef found
          rightOnly <- withRowReader rightDirectory rightValue $ \rightReader ->
            drainRows rightReader (problem "candidate") >> pure (Right ())
          after <- length <$> readIORef found
          case rightOnly of
            Left other
              | after == before -> record (prefix ++ ".trace_hits: " ++ other)
            _ | after == before -> record (prefix ++ ".trace_hits: " ++ message)
            _ -> pure ()
  reverse <$> readIORef found

drainRows :: RowReader -> (String -> IO ()) -> IO ()
drainRows reader report = do
  next <- rowNext reader
  case next of
    Right Nothing -> pure ()
    Right (Just _) -> drainRows reader report
    Left message -> report message
