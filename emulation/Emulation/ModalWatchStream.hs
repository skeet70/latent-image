{-# LANGUAGE BangPatterns #-}
{-# LANGUAGE OverloadedStrings #-}
module Emulation.ModalWatchStream
  ( readFinalModalWatch, readFinalModalWatchFrom, compactFinalWatch ) where

import Control.Monad (unless)
import qualified Crypto.Hash.SHA256 as SHA256
import Data.Aeson (Value (..), eitherDecodeStrict', encode, object, (.=))
import qualified Data.Aeson.Key as K
import qualified Data.Aeson.KeyMap as KM
import qualified Data.Aeson.Types as J
import qualified Data.Attoparsec.ByteString as A
import qualified Data.Attoparsec.ByteString.Char8 as C
import qualified Data.ByteString as B
import qualified Data.ByteString.Builder as BB
import qualified Data.ByteString.Lazy as BL
import qualified Data.ByteString.Lazy.Char8 as BLC
import qualified Data.Text as T
import qualified Data.Text.Encoding as TE
import Emulation.ModalObservation (FinalModalWatch, finalModalWatchFromRecords, modalLifetimeWriterLimit)
import Emulation.Sha256 (sha256, sha256Hex)
import Emulation.WatchObservation (WatchRecord, parseWatchRecord, watchLimitValue)
import FujiAF.JSON (valueParser)
import System.Directory (doesFileExist, getFileSize, removeFile, renameFile)
import System.Exit (ExitCode (..))
import System.FilePath (takeDirectory, takeFileName, (</>))
import System.IO (IOMode (ReadMode, WriteMode), withBinaryFile)
import System.Posix.Files (setFileMode)
import System.Process (CreateProcess (..), StdStream (..), proc, readProcessWithExitCode, terminateProcess, waitForProcess, withCreateProcess)

newtype RowCount = RowCount Int

data RecordsReference = RecordsReference
  { referencePath :: T.Text
  , referenceSha256 :: T.Text
  , referenceCount :: Int
  , referenceBytes :: Int
  }

jsonSpace :: A.Parser ()
jsonSpace = C.skipWhile (`elem` [' ', '\t', '\n', '\r'])

readFinalModalWatch :: FilePath -> IO (Either String FinalModalWatch)
readFinalModalWatch path = withBinaryFile path ReadMode $ \handle -> do
  parsed <- readWatchEncodingFrom (B.hGetSome handle 65536)
  case parsed of
    Left problem -> pure (Left problem)
    Right (summary, Left rows) -> pure (finalModalWatchFromRecords summary rows)
    Right (summary, Right reference) -> readCompacted path summary reference

-- The source returns an empty chunk only at EOF.
readFinalModalWatchFrom :: IO B.ByteString -> IO (Either String FinalModalWatch)
readFinalModalWatchFrom next = do
  parsed <- readWatchEncodingFrom next
  pure $ do
    (summary, encoding) <- parsed
    case encoding of
      Left rows -> finalModalWatchFromRecords summary rows
      Right _ -> Left "streamed terminal watch has no sidecar path"

readWatchEncodingFrom :: IO B.ByteString -> IO (Either String (Value, Either [WatchRecord] RecordsReference))
readWatchEncodingFrom next = advance (A.parse (watchEncodingParser <* jsonSpace <* A.endOfInput) B.empty)
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

parseReference :: Value -> Maybe RecordsReference
parseReference value = J.parseMaybe (J.withObject "records reference" $ \fields -> do
  schema <- fields J..: "schema" :: J.Parser Int
  unless (schema == 1) (fail "unknown records reference schema")
  RecordsReference <$> fields J..: "path" <*> fields J..: "sha256"
    <*> fields J..: "count" <*> fields J..: "bytes") value

readCompacted :: FilePath -> Value -> RecordsReference -> IO (Either String FinalModalWatch)
readCompacted finalPath summary reference = do
  let name = T.unpack (referencePath reference)
      sidecar = takeDirectory finalPath </> name
  if takeFileName name /= name || name `elem` ["", ".", ".."]
    then pure (Left "compacted watch sidecar path is not a basename")
    else do
      present <- doesFileExist sidecar
      if not present then pure (Left "compacted watch sidecar is missing")
      else do
        size <- getFileSize sidecar
        if size /= toInteger (referenceBytes reference) then pure (Left "compacted watch sidecar size differs")
        else if size > compressedLimit then pure (Left "compacted watch sidecar exceeds its compressed bound")
        else do
          compressedDigest <- withBinaryFile sidecar ReadMode $ \handle -> hashChunks (B.hGetSome handle 65536) SHA256.init
          if compressedDigest /= T.unpack (referenceSha256 reference)
            then pure (Left "compacted watch sidecar digest differs")
            else do
              streamed <- streamSidecar sidecar
              pure $ do
                (rows, digest) <- streamed
                expected <- case summary of
                  Object fields -> case KM.lookup "records_sha256" fields of
                    Just (String text) | T.length text == 64 -> Right (T.unpack text)
                    _ -> Left "compacted watch summary lacks its row digest"
                  _ -> Left "compacted watch summary is not an object"
                unless (digest == expected) (Left "compacted watch row digest differs")
                unless (length rows == referenceCount reference) (Left "compacted watch row count differs")
                finalModalWatchFromRecords summary rows
 where
  compressedLimit = 512 * 1024 * 1024 :: Integer

digestHex :: SHA256.Ctx -> String
digestHex = BLC.unpack . BB.toLazyByteString . BB.byteStringHex . SHA256.finalize

hashChunks :: IO B.ByteString -> SHA256.Ctx -> IO String
hashChunks next !context = do
  chunk <- next
  if B.null chunk then pure (digestHex context)
  else hashChunks next (SHA256.update context chunk)

streamSidecar :: FilePath -> IO (Either String ([WatchRecord], String))
streamSidecar sidecar =
  withCreateProcess (proc "gzip" ["-dc", sidecar]) { std_out = CreatePipe, std_err = CreatePipe } $
    \_ output errors process -> case (output, errors) of
      (Just outputHandle, Just errorHandle) -> do
        result <- readLines outputHandle
        case result of
          Left problem -> do
            terminateProcess process
            _ <- waitForProcess process
            pure (Left problem)
          Right (records, digest) -> do
            code <- waitForProcess process
            case code of
              ExitSuccess -> pure (Right (records, digest))
              _ -> do
                message <- B.hGetContents errorHandle
                pure (Left ("compacted watch decompression failed: " ++ T.unpack (T.strip (TE.decodeUtf8 message))))
      _ -> pure (Left "compacted watch decompression has no pipes")
 where
  lineLimit = 1024 * 1024
  decodedLimit = 1024 * 1024 * 1024
  readLines handle = go handle B.empty 0 0 [] SHA256.init
  go handle partial !total !count records !digest = do
    chunk <- B.hGetSome handle 65536
    if B.null chunk
      then if B.null partial
        then pure (Right (reverse records, digestHex digest))
        else pure (Left "compacted watch row is not LF terminated")
      else do
        let pieces = B.split 10 (partial <> chunk)
            complete = init pieces
            rest = last pieces
        if B.length rest > lineLimit then pure (Left "compacted watch row exceeds its line bound")
        else case consume complete total count records digest of
          Left problem -> pure (Left problem)
          Right (nextTotal, nextCount, nextRecords, nextDigest) -> go handle rest nextTotal nextCount nextRecords nextDigest
  consume [] !total !count records !digest = Right (total, count, records, digest)
  consume (piece : remaining) !total !count records !digest
    | B.null piece = Left "compacted watch contains a blank row"
    | B.length piece > lineLimit = Left "compacted watch row exceeds its line bound"
    | total + B.length piece + 1 > decodedLimit = Left "compacted watch rows exceed their decoded bound"
    | count >= watchLimitValue modalLifetimeWriterLimit = Left "compacted watch records exceed the declared limit"
    | otherwise = case eitherDecodeStrict' piece of
        Left problem -> Left ("compacted watch row: " ++ problem)
        Right value -> case parseWatchRecord value of
          Left problem -> Left ("compacted watch row: " ++ problem)
          Right record -> consume remaining (total + B.length piece + 1) (count + 1) (record : records)
            (SHA256.update (SHA256.update digest piece) "\n")

compactFinalWatch :: FilePath -> FilePath -> IO (Either String ())
compactFinalWatch rawPath finalPath = do
  raw <- B.readFile rawPath
  case A.parseOnly (watchRowsParser <* jsonSpace <* A.endOfInput) raw of
    Left problem -> pure (Left problem)
    Right (summary, rows) -> do
      let !rowCount = length rows
          jsonl = finalPath ++ ".records.jsonl"
          compressed = jsonl ++ ".gz"
      withBinaryFile jsonl WriteMode $ \handle ->
        mapM_ (\row -> B.hPut handle row >> B.hPut handle "\n") rows
      (code, _, errors) <- readProcessWithExitCode "gzip" ["-n", "-f", jsonl] ""
      case code of
        ExitSuccess -> do
          compressedBytes <- B.readFile compressed
          let reference = object
                [ "schema" .= (1 :: Int)
                , "path" .= takeFileName compressed
                , "sha256" .= sha256Hex (sha256 compressedBytes)
                , "count" .= rowCount
                , "bytes" .= B.length compressedBytes ]
              final = case summary of
                Object fields -> Object (KM.insert "records" reference fields)
                _ -> summary
              temporary = finalPath ++ ".tmp"
          BL.writeFile temporary (encode final)
          setFileMode temporary 0o600
          renameFile temporary finalPath
          removeFile rawPath
          pure (Right ())
        _ -> pure (Left ("compacted watch compression failed: " ++ errors))

watchEncodingParser :: A.Parser (Value, Either [WatchRecord] RecordsReference)
watchEncodingParser = do
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
          parsedRecords <- recordsEncoding
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
          pure (Object (KM.delete "ok" updated), parsedRecords)
        _ -> fail "terminal watch field lacks a comma or closing brace"
    recordsEncoding :: A.Parser (Either [WatchRecord] RecordsReference)
    recordsEncoding = do
      first <- C.peekChar'
      if first == '['
        then Left <$> recordArray
        else Right <$> referenceObject
    referenceObject :: A.Parser RecordsReference
    referenceObject = do
      entry <- value
      maybe (fail "terminal watch records reference is invalid") pure (parseReference entry)
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

watchRowsParser :: A.Parser (Value, [B.ByteString])
watchRowsParser = do
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
          parsedRecords <- recordArrayRaw
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
          pure (Object (KM.delete "ok" updated), parsedRecords)
        _ -> fail "terminal watch field lacks a comma or closing brace"
    recordArrayRaw :: A.Parser [B.ByteString]
    recordArrayRaw = do
      _ <- C.char '['
      jsonSpace
      first <- C.peekChar'
      if first == ']' then C.anyChar *> jsonSpace *> pure [] else recordRowsRaw (RowCount 0) []
    recordRowsRaw (RowCount !count) !reversed = do
      unless (count < watchLimitValue modalLifetimeWriterLimit) (fail "watch records exceed the declared limit")
      (raw, _) <- A.match value
      separator <- C.anyChar
      jsonSpace
      case separator of
        ',' -> recordRowsRaw (RowCount (count + 1)) (raw : reversed)
        ']' -> pure (reverse (raw : reversed))
        _ -> fail "terminal watch row lacks a comma or closing bracket"
