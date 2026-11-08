{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}

-- | Bounded render trace-hit receipts. A segment's declared hits are inline when the whole range
-- fits one page, otherwise a JSONL sidecar referenced by the observation. The producer freezes the
-- end count and validates every page; the consumer reads either schema without decoding the whole
-- observation.
module Emulation.ObservationStream
  ( TraceHitSource (..)
  , TraceHitSnapshot (..)
  , emulatorTraceHitSource
  , foldTraceHitSources
  , TraceHitFormat (..)
  , TraceHitRef (..)
  , ObservationRef (..)
  , ObservationSpec (..)
  , writeObservationStream
  , foldTraceHitRows
  , foldTraceHitLines
  , RowReader
  , rowNext
  , withRowReader
  ) where

import Control.Exception (bracket)
import Control.Monad (foldM, forM_, unless)
import Data.Aeson (Value (..), eitherDecodeStrict', encode, object, toJSON, withObject, (.:), (.=))
import qualified Data.Aeson as J
import qualified Data.Aeson.KeyMap as KM
import Data.Aeson.Types (Parser, parseEither)
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import Data.IORef (modifyIORef', newIORef, readIORef, writeIORef)
import qualified Data.ByteString.Lazy as BL
import qualified Data.Text as T
import Emulation.Sha256 (Sha256, parseSha256Hex, sha256File, sha256Hex)
import EmulatorUsb
  ( Emulator, TraceHit (..), TraceHitCursor (..), TraceHitLimit (..), TraceHitPage (..)
  , emulatorTraceHitPage, traceHitValue )
import System.Directory (doesFileExist, getFileSize)
import System.FilePath (isAbsolute, takeDirectory, takeFileName, (</>))
import System.IO (IOMode (ReadMode, WriteMode), hClose, hIsEOF, openBinaryFile)

newtype TraceHitSnapshot = TraceHitSnapshot Int deriving (Eq, Show)

data TraceHitSource = TraceHitSource
  { sourceStart :: TraceHitCursor
  , sourceEnd :: TraceHitCursor
  , sourceSnapshot :: TraceHitSnapshot
  , sourcePage :: TraceHitCursor -> TraceHitLimit -> IO TraceHitPage
  }

emulatorTraceHitSource :: Emulator -> TraceHitCursor -> TraceHitCursor -> TraceHitSnapshot -> TraceHitSource
emulatorTraceHitSource emulator start end snapshot =
  TraceHitSource start end snapshot (emulatorTraceHitPage emulator)

data TraceHitFormat = TraceHitFormat
  { formatLrLabels :: [String]
  , formatSpLabels :: [String]
  , formatDeclaredLabels :: [String]
  }

data TraceHitRef = TraceHitRef
  { traceHitRefPath :: FilePath
  , traceHitRefSha256 :: Sha256
  , traceHitRefCount :: Int
  } deriving (Eq)

data ObservationRef = ObservationRef
  { observationRefPhase :: T.Text
  , observationRefPath :: FilePath
  , observationRefSha256 :: Sha256
  , observationRefBytes :: Integer
  , observationRefTraceHits :: TraceHitRef
  } deriving (Eq)

data ObservationSpec = ObservationSpec
  { specDirectory :: FilePath
  , specStem :: FilePath
  , specPhase :: T.Text
  , specPrefix :: Value
  , specSources :: [TraceHitSource]
  , specFormat :: TraceHitFormat
  , specSuffix :: Value
  }

instance J.ToJSON TraceHitRef where
  toJSON (TraceHitRef path digest count) = object
    [ "schema" .= (1 :: Int), "path" .= path, "sha256" .= sha256Hex digest, "count" .= count ]

instance J.FromJSON TraceHitRef where
  parseJSON = withObject "trace-hit reference" $ \fields -> do
    schema <- fields .: "schema" :: Parser Int
    unless (schema == 1) (fail "unsupported trace-hit reference schema")
    path <- fields .: "path"
    text <- fields .: "sha256"
    digest <- either fail pure (parseSha256Hex text)
    count <- fields .: "count"
    unless (count >= 0) (fail "trace-hit reference count is negative")
    pure (TraceHitRef path digest count)

instance J.ToJSON ObservationRef where
  toJSON (ObservationRef phase path digest bytes hits) = object
    [ "phase" .= phase, "path" .= path, "sha256" .= sha256Hex digest
    , "bytes" .= bytes, "trace_hits" .= hits ]

foldTraceHitSources :: [TraceHitSource] -> (a -> TraceHit -> IO a) -> a -> IO a
foldTraceHitSources sources step initial = do
  state <- newIORef initial
  failure <- newIORef Nothing
  forM_ sources $ \source -> do
    cursor <- newIORef (sourceStart source)
    let TraceHitSnapshot snapshot = sourceSnapshot source
        TraceHitCursor end = sourceEnd source
    if end > snapshot then writeIORef failure (Just "trace-hit interval end exceeds its frozen snapshot")
    else if cursorValue (sourceStart source) > end then writeIORef failure (Just "trace-hit source start exceeds its frozen end")
    else do
      let loop = do
            failureState <- readIORef failure
            case failureState of
              Just _ -> pure ()
              Nothing -> do
                TraceHitCursor from <- readIORef cursor
                if from == end then pure ()
                else if from > end then writeIORef failure (Just "trace-hit source start exceeds its frozen end")
                else do
                  let remaining = end - from
                      limit = TraceHitLimit (min 16 remaining)
                  page <- sourcePage source (TraceHitCursor from) limit
                  if tracePageCount page /= snapshot
                    then writeIORef failure (Just "trace-hit page count changed while collecting a segment")
                    else if tracePageCursor page /= TraceHitCursor from
                      then writeIORef failure (Just "trace-hit page cursor differs from the requested cursor")
                      else if length (tracePageHits page) /= min 16 remaining
                        then writeIORef failure (Just "trace-hit page is short, empty or over the frozen count")
                        else do
                          current <- readIORef state
                          next <- foldM step current (tracePageHits page)
                          writeIORef state next
                          writeIORef cursor (TraceHitCursor (from + length (tracePageHits page)))
                          loop
      loop
  problem <- readIORef failure
  final <- readIORef state
  case problem of
    Just message -> ioError (userError message)
    Nothing -> pure final

foldTraceHitLines :: FilePath -> Value -> (a -> B.ByteString -> IO a) -> a -> IO (Either String a)
foldTraceHitLines directory value step initial = case value of
  Object fields -> case KM.lookup "trace_hits" fields of
    Just (Array rows) -> Right <$> foldM (\state row -> step state (BL.toStrict (encode row))) initial (foldr (:) [] rows)
    Just raw@(Object _) -> case parseEither (J.parseJSON :: Value -> Parser TraceHitRef) raw of
      Left problem -> pure (Left ("trace_hits reference: " ++ problem))
      Right ref -> foldSidecarLines directory ref step initial
    Just _ -> pure (Left "trace_hits is neither an array nor a reference")
    Nothing -> pure (Left "trace_hits is missing")
  _ -> pure (Left "observation is not an object")

foldSidecarLines :: FilePath -> TraceHitRef -> (a -> B.ByteString -> IO a) -> a -> IO (Either String a)
foldSidecarLines directory ref step initial
  | isAbsolute relative || takeFileName relative /= relative || takeDirectory relative /= "." =
      pure (Left "trace-hit reference path is not a plain file name")
  | otherwise = do
      let path = directory </> relative
      present <- doesFileExist path
      if not present then pure (Left ("trace-hit sidecar is missing: " ++ path)) else do
        parsed <- bracket (openBinaryFile path ReadMode) hClose readLines
        case parsed of
          Left problem -> pure (Left problem)
          Right (state, count) -> do
            digest <- sha256File path
            if count /= traceHitRefCount ref
              then pure (Left ("trace-hit sidecar has " ++ show count ++ " rows, the reference names " ++ show (traceHitRefCount ref)))
              else pure (if digest == traceHitRefSha256 ref then Right state else Left "trace-hit sidecar digest differs from its reference")
 where
  relative = traceHitRefPath ref
  readLines handle = do
    state <- newIORef initial
    count <- newIORef (0 :: Int)
    failure <- newIORef Nothing
    let loop = do
          problem <- readIORef failure
          case problem of
            Just _ -> pure ()
            Nothing -> do
              eof <- hIsEOF handle
              if eof then pure () else do
                line <- C.hGetLine handle
                if B.null line then writeIORef failure (Just "trace-hit sidecar has an empty line")
                else do
                  currentState <- readIORef state
                  next <- step currentState line
                  writeIORef state next
                  current <- readIORef count
                  writeIORef count (current + 1)
                loop
    loop
    problem <- readIORef failure
    final <- readIORef state
    total <- readIORef count
    pure $ case problem of
      Just message -> Left message
      Nothing -> Right (final, total)

writeObservationStream :: ObservationSpec -> IO ObservationRef
writeObservationStream spec = do
  let directory = specDirectory spec
      stem = specStem spec
      observationPath = directory </> stem ++ ".json"
      sidecarPath = directory </> stem ++ ".trace-hits.jsonl"
      totalRows = sum [cursorValue (sourceEnd source) - cursorValue (sourceStart source) | source <- specSources spec]
      inline = totalRows <= 16
  (inlineRows, sidecar, declaredRows) <-
    if inline
      then do
        rows <- collectDeclared spec
        pure (rows, Nothing, length rows)
      else do
        (count, digest) <- writeSidecar spec sidecarPath
        pure ([], Just digest, count)
  writeObservationFile spec observationPath inlineRows sidecar declaredRows
  observationDigest <- sha256File observationPath
  observationBytes <- getFileSize observationPath
  let traceRef = case sidecar of
        Just digest -> TraceHitRef (takeFileName sidecarPath) digest declaredRows
        Nothing -> TraceHitRef (takeFileName observationPath) observationDigest declaredRows
  pure (ObservationRef (specPhase spec) (takeFileName observationPath) observationDigest observationBytes traceRef)

writeSidecar :: ObservationSpec -> FilePath -> IO (Int, Sha256)
writeSidecar spec path = do
  count <- bracket (openBinaryFile path WriteMode) hClose $ \handle ->
    foldTraceHitSources (specSources spec) (step handle) 0
  digest <- sha256File path
  pure (count, digest)
 where
  step handle count hit
    | isDeclared spec hit = do
        B.hPut handle (BL.toStrict (encode (traceHitValue (formatLrLabels (specFormat spec)) (formatSpLabels (specFormat spec)) hit)) <> "\n")
        pure (count + 1)
    | otherwise = pure count

collectDeclared :: ObservationSpec -> IO [Value]
collectDeclared spec = foldTraceHitSources (specSources spec) step []
 where
  step rows hit
    | isDeclared spec hit = pure (rows ++ [traceHitValue (formatLrLabels (specFormat spec)) (formatSpLabels (specFormat spec)) hit])
    | otherwise = pure rows

isDeclared :: ObservationSpec -> TraceHit -> Bool
isDeclared spec hit = traceSite hit `elem` formatDeclaredLabels (specFormat spec)

writeObservationFile :: ObservationSpec -> FilePath -> [Value] -> Maybe Sha256 -> Int -> IO ()
writeObservationFile spec path rows sidecar count = do
  let traceFieldValue = case sidecar of
        Just digest -> object
          [ "schema" .= (1 :: Int)
          , "path" .= (specStem spec ++ ".trace-hits.jsonl")
          , "sha256" .= sha256Hex digest, "count" .= count ]
        Nothing -> toJSON rows
      prefix = encodeObjectPrefix (specPrefix spec)
      suffix = encodeObjectSuffix (specSuffix spec)
      bytes = prefix <> ",\"trace_hits\":" <> BL.toStrict (encode traceFieldValue)
        <> (if B.null suffix then "" else "," <> suffix) <> "\n"
  BL.writeFile path (BL.fromStrict bytes)

encodeObjectPrefix :: Value -> B.ByteString
encodeObjectPrefix value = case BL.toStrict (encode value) of
  bytes | B.length bytes >= 2 && B.head bytes == 123 && B.last bytes == 125 -> B.init bytes
  _ -> "{"

encodeObjectSuffix :: Value -> B.ByteString
encodeObjectSuffix value = case BL.toStrict (encode value) of
  bytes | B.length bytes >= 2 && B.head bytes == 123 && B.last bytes == 125 ->
    if bytes == "{}" then "" else B.tail bytes
  _ -> ""

data RowReader = RowReader
  { readerNext :: IO (Either String (Maybe Value))
  , readerFinish :: IO (Either String ())
  , readerClose :: IO ()
  }

rowNext :: RowReader -> IO (Either String (Maybe Value))
rowNext = readerNext

withRowReader :: FilePath -> Value -> (RowReader -> IO (Either String a)) -> IO (Either String a)
withRowReader directory value action = case traceField value of
  Left problem -> pure (Left problem)
  Right (Array rows) -> do
    remaining <- newIORef (foldr (:) [] rows)
    let reader = RowReader
          { readerNext = do
              rest <- readIORef remaining
              case rest of
                [] -> pure (Right Nothing)
                row : more -> writeIORef remaining more >> pure (Right (Just row))
          , readerFinish = do
              rest <- readIORef remaining
              pure (if null rest then Right () else Left "trace-hit row counts differ")
          , readerClose = pure () }
    action reader
  Right (Object _) -> case parseEither (J.parseJSON :: Value -> Parser TraceHitRef) (traceValue value) of
    Left problem -> pure (Left ("trace_hits reference: " ++ problem))
    Right ref -> withSidecar directory ref action
  Right _ -> pure (Left "trace_hits is neither an array nor a reference")

withSidecar :: FilePath -> TraceHitRef -> (RowReader -> IO (Either String a)) -> IO (Either String a)
withSidecar directory ref action
  | isAbsolute relative || takeFileName relative /= relative || takeDirectory relative /= "." =
      pure (Left "trace-hit reference path is not a plain file name")
  | otherwise = do
      let path = directory </> relative
      present <- doesFileExist path
      if not present then pure (Left ("trace-hit sidecar is missing: " ++ path)) else do
        handle <- openBinaryFile path ReadMode
        count <- newIORef (0 :: Int)
        verified <- newIORef Nothing
        let verify = do
              cached <- readIORef verified
              case cached of
                Just result -> pure result
                Nothing -> do
                  eof <- hIsEOF handle
                  if not eof
                    then do
                      _ <- C.hGetLine handle
                      modifyIORef' count (+ 1)
                      verify
                    else do
                      total <- readIORef count
                      digest <- sha256File path
                      let result = if total /= traceHitRefCount ref
                            then Left ("trace-hit sidecar has " ++ show total ++ " rows, the reference names " ++ show (traceHitRefCount ref))
                            else if digest /= traceHitRefSha256 ref then Left "trace-hit sidecar digest differs from its reference" else Right ()
                      writeIORef verified (Just result)
                      pure result
            reader = RowReader
              { readerNext = do
                  cached <- readIORef verified
                  case cached of
                    Just (Left problem) -> pure (Left problem)
                    Just (Right ()) -> pure (Right Nothing)
                    Nothing -> do
                      eof <- hIsEOF handle
                      if eof then do
                        checked <- verify
                        pure (case checked of Left problem -> Left problem; Right () -> Right Nothing)
                      else do
                        line <- C.hGetLine handle
                        modifyIORef' count (+ 1)
                        case eitherDecodeStrict' line of
                          Left problem -> pure (Left ("trace-hit sidecar line: " ++ problem))
                          Right row -> pure (Right (Just row))
              , readerFinish = verify
              , readerClose = hClose handle }
        result <- action reader
        finish <- readerFinish reader
        readerClose reader
        pure (finish >> result)
 where
  relative = traceHitRefPath ref

traceField :: Value -> Either String Value
traceField (Object fields) = maybe (Left "trace_hits is missing") Right (KM.lookup "trace_hits" fields)
traceField _ = Left "observation is not an object"

traceValue :: Value -> Value
traceValue value = case traceField value of
  Right field -> field
  Left _ -> Null

foldTraceHitRows :: FilePath -> Value -> (a -> Value -> Either String a) -> a -> IO (Either String a)
foldTraceHitRows directory value step initial = case value of
  Object fields -> case KM.lookup "trace_hits" fields of
    Just (Array rows) -> pure (foldM step initial (foldr (:) [] rows))
    Just raw@(Object _) -> case parseEither (J.parseJSON :: Value -> Parser TraceHitRef) raw of
      Left problem -> pure (Left ("trace_hits reference: " ++ problem))
      Right ref -> foldSidecar directory ref step initial
    Just _ -> pure (Left "trace_hits is neither an array nor a reference")
    Nothing -> pure (Left "trace_hits is missing")
  _ -> pure (Left "observation is not an object")

foldSidecar :: FilePath -> TraceHitRef -> (a -> Value -> Either String a) -> a -> IO (Either String a)
foldSidecar directory ref step initial
  | isAbsolute relative || takeFileName relative /= relative || takeDirectory relative /= "." =
      pure (Left "trace-hit reference path is not a plain file name")
  | otherwise = do
      let path = directory </> relative
      present <- doesFileExist path
      if not present then pure (Left ("trace-hit sidecar is missing: " ++ path)) else do
        parsed <- bracket (openBinaryFile path ReadMode) hClose readLines
        case parsed of
          Left problem -> pure (Left problem)
          Right (state, count) -> do
            digest <- sha256File path
            if count /= traceHitRefCount ref
              then pure (Left ("trace-hit sidecar has " ++ show count ++ " rows, the reference names " ++ show (traceHitRefCount ref)))
              else pure (if digest == traceHitRefSha256 ref then Right state else Left "trace-hit sidecar digest differs from its reference")
 where
  relative = traceHitRefPath ref
  readLines handle = do
    state <- newIORef initial
    count <- newIORef (0 :: Int)
    failure <- newIORef Nothing
    let loop = do
          failureState <- readIORef failure
          case failureState of
            Just _ -> pure ()
            Nothing -> do
              eof <- hIsEOF handle
              if eof then pure () else do
                line <- C.hGetLine handle
                if B.null line then writeIORef failure (Just "trace-hit sidecar has an empty line")
                else case eitherDecodeStrict' line of
                  Left lineProblem -> writeIORef failure (Just ("trace-hit sidecar line: " ++ lineProblem))
                  Right row -> do
                    currentState <- readIORef state
                    case step currentState row of
                      Left stepProblem -> writeIORef failure (Just stepProblem)
                      Right next -> do
                        writeIORef state next
                        current <- readIORef count
                        writeIORef count (current + 1)
                loop
    loop
    problem <- readIORef failure
    final <- readIORef state
    total <- readIORef count
    pure $ case problem of
      Just message -> Left message
      Nothing -> Right (final, total)

cursorValue :: TraceHitCursor -> Int
cursorValue (TraceHitCursor value) = value

