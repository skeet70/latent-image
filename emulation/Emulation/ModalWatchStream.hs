{-# LANGUAGE BangPatterns #-}
{-# LANGUAGE OverloadedStrings #-}
module Emulation.ModalWatchStream (readFinalModalWatch, readFinalModalWatchFrom) where

import Control.Monad (unless)
import Data.Aeson (Value (..))
import qualified Data.Aeson.Key as K
import qualified Data.Aeson.KeyMap as KM
import qualified Data.Attoparsec.ByteString as A
import qualified Data.Attoparsec.ByteString.Char8 as C
import qualified Data.ByteString as B
import Emulation.ModalObservation (FinalModalWatch, finalModalWatchFromRecords, modalLifetimeWriterLimit)
import Emulation.WatchObservation (WatchRecord, parseWatchRecord, watchLimitValue)
import FujiAF.JSON (valueParser)
import System.IO (IOMode (ReadMode), withBinaryFile)

newtype RowCount = RowCount Int

jsonSpace :: A.Parser ()
jsonSpace = C.skipWhile (`elem` [' ', '\t', '\n', '\r'])

readFinalModalWatch :: FilePath -> IO (Either String FinalModalWatch)
readFinalModalWatch path = withBinaryFile path ReadMode $ \handle ->
  readFinalModalWatchFrom (B.hGetSome handle 65536)

-- The source returns an empty chunk only at EOF.
readFinalModalWatchFrom :: IO B.ByteString -> IO (Either String FinalModalWatch)
readFinalModalWatchFrom next = advance (A.parse (watchParser <* jsonSpace <* A.endOfInput) B.empty)
  where
    advance result = case result of
      A.Partial continuation -> do
        chunk <- next
        if B.null chunk then pure (finished (continuation B.empty)) else advance (continuation chunk)
      _ -> pure (finished result)
    finished result = case result of
      A.Done remaining value | B.null remaining -> Right value
      A.Done _ _ -> Left "terminal watch has trailing bytes"
      A.Fail _ context problem -> Left (unwords context ++ ": " ++ problem)
      A.Partial _ -> Left "terminal watch ended before its JSON value completed"

watchParser :: A.Parser FinalModalWatch
watchParser = do
  jsonSpace
  _ <- C.char '{'
  jsonSpace
  first <- C.peekChar'
  if first == '}' then fail "terminal watch has no fields" else fields KM.empty Nothing
  where
    value = valueParser
    fields !summary records = do
      keyValue <- value
      key <- case keyValue of
        String name -> pure (K.fromText name)
        _ -> fail "terminal watch key is not text"
      unless (key `elem` ["ok", "enabled", "ranges", "records", "record_count", "records_sha256", "violation", "classes", "views", "fetch_attribution"])
        (fail "terminal watch has an unknown field")
      unless (not (KM.member key summary) && (key /= "records" || maybe True (const False) records))
        (fail "terminal watch repeats a field")
      _ <- C.char ':'
      jsonSpace
      (updated, updatedRecords) <- if key == "records"
        then do
          parsedRecords <- recordArray
          pure (summary, Just parsedRecords)
        else do
          entry <- value
          unless (key /= "ok" || entry == Bool True) (fail "terminal watch RPC did not succeed")
          pure (KM.insert key entry summary, records)
      separator <- C.anyChar
      jsonSpace
      case separator of
        ',' -> fields updated updatedRecords
        '}' -> do
          parsedRecords <- maybe (fail "terminal watch has no records field") pure updatedRecords
          either fail pure (finalModalWatchFromRecords (Object (KM.delete "ok" updated)) parsedRecords)
        _ -> fail "terminal watch field lacks a comma or closing brace"
    recordArray :: A.Parser [WatchRecord]
    recordArray = do
      _ <- C.char '['
      jsonSpace
      first <- C.peekChar'
      if first == ']' then C.anyChar *> jsonSpace *> pure [] else recordRows (RowCount 0) []
    recordRows (RowCount !count) !reversed = do
      unless (count < watchLimitValue modalLifetimeWriterLimit) (fail "watch records exceed the declared limit")
      raw <- value
      !record <- either fail pure (parseWatchRecord raw)
      separator <- C.anyChar
      jsonSpace
      case separator of
        ',' -> recordRows (RowCount (count + 1)) (record : reversed)
        ']' -> pure (reverse (record : reversed))
        _ -> fail "terminal watch row lacks a comma or closing bracket"
