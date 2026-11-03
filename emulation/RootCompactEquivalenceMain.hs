{-# LANGUAGE LambdaCase, OverloadedStrings #-}
-- | Checks that a compact root-task receipt decodes to exactly the values of its original.
-- Both JSON documents are streamed; only one sample of each is decoded at a time.
module Main (main) where

import Control.Exception (catch)
import Control.Monad (foldM, unless, when)
import Data.Aeson (Value(..), eitherDecodeStrict')
import qualified Data.Aeson.Key as K
import qualified Data.Aeson.KeyMap as KM
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import qualified Data.ByteString.Lazy as L
import qualified Data.ByteString.Unsafe as BU
import Data.Int (Int64)
import Data.List (find, sort)
import qualified Data.Map.Strict as Map
import Data.Maybe (fromMaybe, isJust)
import Data.Scientific (toBoundedInteger)
import qualified Data.Set as Set
import qualified Data.Text as T
import qualified Data.Vector as V
import Data.Word (Word8)
import Emulation.Sha256 (parseSha256Hex, sha256, sha256File, sha256Hex)
import FujiAF.JSON (decodeStrictIntegers, encodeCanonical)
import System.Directory (doesPathExist, getFileSize)
import System.Environment (getArgs)
import System.Exit (exitFailure)
import System.FilePath ((</>))
import System.IO (hPutStrLn, stderr)
import System.IO.Error (ioeGetErrorString)

-- | Remaining bytes of the current chunk, the unread chunks, and the absolute offset.
-- After 'normal', the current chunk is empty only at end of input.
data Cur = Cur !B.ByteString [B.ByteString] !Int64

-- | Depth, inside a string, after a backslash inside a string.
data Scan = Scan !Int !Bool !Bool

data Step = Element B.ByteString Cur | End Cur

-- | Samples compared, references resolved, end of the last reference.
data Tally = Tally !Int !Int !Int

data Meta = Meta { metaBlobBytes :: Int, metaBlobSha :: String, metaSourceBytes :: Integer, metaSourceSha :: String }

main :: IO ()
main = do
  args <- getArgs
  case args of
    [original, compactDir, output] -> run original compactDir output `catch` \err -> do
      hPutStrLn stderr ("root-compact-equivalence: " ++ ioeGetErrorString err)
      exitFailure
    _ -> do
      hPutStrLn stderr "usage: root-compact-equivalence ORIGINAL_ROOT_TASK_JSON COMPACT_DIR OUTPUT_JSON"
      exitFailure

run :: FilePath -> FilePath -> FilePath -> IO ()
run original compactDir output = do
  refuseExisting output
  let compactJson = compactDir </> "root-task.json"
  blob <- B.readFile (compactDir </> "root-task.bytes")
  (compactMembers, compactSamples) <- open compactJson >>= \cur -> walkTop "compact" cur skipSamples
  samplesAt <- maybe (failWith "compact: top-level samples missing") pure compactSamples
  meta <- maybe (failWith "compact: top-level compact missing") (orFail . parseMeta) (Map.lookup "compact" compactMembers)
  unless (metaBlobBytes meta == B.length blob) $
    failWith ("compact.blob_bytes " ++ show (metaBlobBytes meta) ++ " but blob has " ++ show (B.length blob) ++ " bytes")
  unless (metaBlobSha meta == sha256Hex (sha256 blob)) $ failWith "compact.blob_sha256 does not match the blob"
  sourceBytes <- getFileSize original
  unless (metaSourceBytes meta == sourceBytes) $
    failWith ("compact.source_bytes " ++ show (metaSourceBytes meta) ++ " but original has " ++ show sourceBytes ++ " bytes")
  sourceSha <- sha256Hex <$> sha256File original
  unless (metaSourceSha meta == sourceSha) $ failWith ("compact.source_sha256 does not match original sha256 " ++ sourceSha)
  (originalMembers, tally) <- open original >>= \cur -> walkTop "original" cur (compareSamples blob compactJson samplesAt)
  Tally samples references end <- maybe (failWith "original: top-level samples missing") pure tally
  let compactRest = Map.delete "compact" compactMembers
  case find (`Map.notMember` compactRest) (Map.keys originalMembers) of
    Just key -> failWith ("top-level member " ++ show key ++ " only in original")
    Nothing -> pure ()
  case find (`Map.notMember` originalMembers) (Map.keys compactRest) of
    Just key -> failWith ("top-level member " ++ show key ++ " only in compact")
    Nothing -> pure ()
  case find (\key -> Map.lookup key originalMembers /= Map.lookup key compactRest) (Map.keys originalMembers) of
    Just key -> failWith ("top-level member " ++ show key ++ difference (originalMembers Map.! key) (compactRest Map.! key))
    Nothing -> pure ()
  unless (end == B.length blob) $
    failWith ("references end at " ++ show end ++ " but blob_bytes is " ++ show (B.length blob))
  refuseExisting output
  B.writeFile output $ encodeCanonical $ Object $ KM.fromList
    [ ("schema", Number 1)
    , ("equivalent", Bool True)
    , ("samples", Number (fromIntegral samples))
    , ("references", Number (fromIntegral references))
    , ("blob_bytes", Number (fromIntegral (B.length blob)))
    , ("source_bytes", Number (fromIntegral sourceBytes))
    , ("source_sha256_checked", Bool True)
    ]

refuseExisting :: FilePath -> IO ()
refuseExisting path = do
  exists <- doesPathExist path
  when exists (failWith (path ++ " already exists"))

failWith :: String -> IO a
failWith = ioError . userError

orFail :: Either String a -> IO a
orFail = either failWith pure

parseMeta :: Value -> Either String Meta
parseMeta (Object m) = do
  unless (Set.fromList (KM.keys m) == Set.fromList ["schema", "blob", "blob_bytes", "blob_sha256", "source_bytes", "source_sha256"]) $
    Left "compact: keys must be schema, blob, blob_bytes, blob_sha256, source_bytes, source_sha256"
  schema <- integer "schema"
  unless (schema == (1 :: Int)) (Left "compact.schema must be 1")
  blobName <- text "blob"
  unless (blobName == "root-task.bytes") (Left "compact.blob must be root-task.bytes")
  Meta <$> integer "blob_bytes" <*> digest "blob_sha256" <*> integer "source_bytes" <*> digest "source_sha256"
 where
  integer :: Integral a => K.Key -> Either String a
  integer key = case KM.lookup key m of
    Just (Number n) | Just i <- toBoundedInteger n, i >= (0 :: Int) -> Right (fromIntegral i)
    _ -> Left ("compact." ++ K.toString key ++ " must be a non-negative integer")
  text key = case KM.lookup key m of
    Just (String s) -> Right s
    _ -> Left ("compact." ++ K.toString key ++ " must be a string")
  digest key = do
    s <- T.unpack <$> text key
    _ <- either (\e -> Left ("compact." ++ K.toString key ++ ": " ++ e)) Right (parseSha256Hex s)
    Right s
parseMeta _ = Left "compact must be an object"

-- Cursor

open :: FilePath -> IO Cur
open path = openAt path 0

openAt :: FilePath -> Int64 -> IO Cur
openAt path offset = do
  bytes <- L.readFile path
  pure (normal (Cur B.empty (L.toChunks (L.drop offset bytes)) offset))

normal :: Cur -> Cur
normal (Cur c (x : xs) p) | B.null c = normal (Cur x xs p)
normal cur = cur

position :: Cur -> Int64
position (Cur _ _ p) = p

peek :: Cur -> Maybe Word8
peek (Cur c _ _) = if B.null c then Nothing else Just (BU.unsafeHead c)

advance :: Int -> Cur -> Cur
advance n (Cur c rest p) = normal (Cur (BU.unsafeDrop n c) rest (p + fromIntegral n))

isSpace :: Word8 -> Bool
isSpace b = b == 32 || b == 10 || b == 13 || b == 9

skipSpace :: Cur -> Cur
skipSpace cur@(Cur c _ _)
  | B.null c = cur
  | n == B.length c = skipSpace (advance n cur)
  | otherwise = advance n cur
 where
  n = B.length (B.takeWhile isSpace c)

expect :: Word8 -> String -> Cur -> Either String Cur
expect byte what cur
  | peek cur == Just byte = Right (advance 1 cur)
  | otherwise = Left (what ++ " expected at byte " ++ show (position cur))

-- | The bytes of the JSON value starting at the cursor, which the caller decodes.
takeValue :: Cur -> Either String (B.ByteString, Cur)
takeValue cur = case peek cur of
  Nothing -> Left "value expected at end of input"
  Just b
    | b == 123 || b == 91 || b == 34 -> nested (Scan 0 False False) [] cur
    | otherwise -> scalar [] cur
 where
  nested s acc c@(Cur chunk _ _)
    | B.null chunk = Left ("unterminated value starting at byte " ++ show (position cur))
    | otherwise = case scanChunk s chunk of
        Right n -> Right (B.concat (reverse (B.take n chunk : acc)), advance n c)
        Left s' -> nested s' (chunk : acc) (advance (B.length chunk) c)
  scalar acc c@(Cur chunk _ _)
    | not (B.null chunk) && n == B.length chunk = scalar (chunk : acc) (advance n c)
    | otherwise = Right (B.concat (reverse (B.take n chunk : acc)), advance n c)
   where
    n = B.length (B.takeWhile (\x -> not (isSpace x || x == 44 || x == 93 || x == 125)) chunk)

-- | Index just past the value's end within the chunk, or the state at the chunk's end.
scanChunk :: Scan -> B.ByteString -> Either Scan Int
scanChunk start chunk = go start 0
 where
  size = B.length chunk
  go s@(Scan depth inString escaped) i
    | i >= size = Left s
    | inString = if escaped
        then go (Scan depth True False) (i + 1)
        else case x of
          92 -> go (Scan depth True True) (i + 1)
          34 | depth == 0 -> Right (i + 1)
             | otherwise -> go (Scan depth False False) (i + 1)
          _ -> go s (i + 1)
    | x == 34 = go (Scan depth True False) (i + 1)
    | x == 123 || x == 91 = go (Scan (depth + 1) False False) (i + 1)
    | x == 125 || x == 93 = if depth <= 1 then Right (i + 1) else go (Scan (depth - 1) False False) (i + 1)
    | otherwise = go s (i + 1)
   where
    x = BU.unsafeIndex chunk i

-- | Cursor at the element or at the closing bracket; 'arrayOpen' consumes the opening one.
arrayOpen :: String -> Cur -> Either String Cur
arrayOpen what cur = skipSpace <$> expect 91 (what ++ " '['") cur

arrayStep :: String -> Cur -> Either String Step
arrayStep what cur = case peek cur of
  Just 93 -> Right (End (advance 1 cur))
  _ -> do
    (bytes, c1) <- takeValue cur
    let c2 = skipSpace c1
    case peek c2 of
      Just 44 -> do
        let c3 = skipSpace (advance 1 c2)
        when (peek c3 == Just 93) (Left (what ++ ": trailing comma at byte " ++ show (position c3)))
        Right (Element bytes c3)
      Just 93 -> Right (Element bytes c2)
      _ -> Left (what ++ ": ',' or ']' expected at byte " ++ show (position c2))

-- | Decodes every top-level member except samples, which the handler consumes from its first byte.
walkTop :: String -> Cur -> (Cur -> IO (r, Cur)) -> IO (Map.Map T.Text Value, Maybe r)
walkTop label start onSamples = do
  c1 <- orFail (expect 123 (label ++ ": top-level '{'") (skipSpace start))
  let c2 = skipSpace c1
  if peek c2 == Just 125 then finish Map.empty Nothing (advance 1 c2) else member Map.empty Nothing c2
 where
  member acc samples cur = do
    unless (peek cur == Just 34) (failWith (label ++ ": member name expected at byte " ++ show (position cur)))
    (nameBytes, c1) <- orFail (takeValue cur)
    name <- case decodeStrictIntegers nameBytes of
      Right (String s) -> pure s
      _ -> failWith (label ++ ": invalid member name at byte " ++ show (position cur))
    when (Map.member name acc || (name == "samples" && isJust samples)) $
      failWith (label ++ ": duplicate top-level member " ++ show name)
    c2 <- skipSpace <$> orFail (expect 58 (label ++ ": ':'") (skipSpace c1))
    if name == "samples"
      then do
        (r, c3) <- onSamples c2
        next acc (Just r) (skipSpace c3)
      else do
        (bytes, c3) <- orFail (takeValue c2)
        v <- orFail (decodeIn (label ++ " member " ++ show name) bytes)
        next (Map.insert name v acc) samples (skipSpace c3)
  next acc samples cur = case peek cur of
    Just 44 -> member acc samples (skipSpace (advance 1 cur))
    Just 125 -> finish acc samples (advance 1 cur)
    _ -> failWith (label ++ ": ',' or '}' expected at byte " ++ show (position cur))
  finish acc samples cur = case peek (skipSpace cur) of
    Nothing -> pure (acc, samples)
    Just _ -> failWith (label ++ ": trailing bytes after the top-level object")

-- | Aeson accepting what the strict decoder refused means a duplicate key, a non-integer number or a size cap.
decodeIn :: String -> B.ByteString -> Either String Value
decodeIn what bytes = case decodeStrictIntegers bytes of
  Right v -> Right v
  Left e -> case eitherDecodeStrict' bytes :: Either String Value of
    Right _ -> Left (what ++ ": duplicate key, non-integer number or decoder cap (" ++ e ++ ")")
    Left _ -> Left (what ++ ": invalid JSON (" ++ e ++ ")")

-- | Validates each compact sample's syntax and returns the samples array offset.
skipSamples :: Cur -> IO (Int64, Cur)
skipSamples cur = do
  c1 <- orFail (arrayOpen "compact samples" cur)
  let go :: Int -> Cur -> IO Cur
      go i c = orFail (arrayStep "compact samples" c) >>= \case
        End c' -> pure c'
        Element bytes c' -> orFail (decodeIn ("compact sample " ++ show i) bytes) >> go (i + 1) c'
  c2 <- go 0 c1
  pure (position cur, c2)

compareSamples :: B.ByteString -> FilePath -> Int64 -> Cur -> IO (Tally, Cur)
compareSamples blob compactJson samplesAt originalCur = do
  o1 <- orFail (arrayOpen "original samples" originalCur)
  c1 <- openAt compactJson samplesAt >>= orFail . arrayOpen "compact samples"
  loop (Tally 0 0 0) o1 c1
 where
  loop tally@(Tally i refs off) oc cc = do
    os <- orFail (arrayStep "original samples" oc)
    cs <- orFail (arrayStep "compact samples" cc)
    case (os, cs) of
      (End oc', End _) -> pure (tally, oc')
      (End _, Element _ _) -> failWith ("sample " ++ show i ++ ": compact has more samples than original")
      (Element _ _, End _) -> failWith ("sample " ++ show i ++ ": original has more samples than compact")
      (Element ob oc', Element cb cc') -> do
        ov <- orFail (decodeIn ("original sample " ++ show i) ob)
        cv <- orFail (decodeIn ("compact sample " ++ show i) cb)
        (expanded, refs', off') <- orFail (expandSample blob i cv refs off)
        unless (ov == expanded) $ failWith ("sample " ++ show i ++ " member " ++ difference ov expanded)
        loop (Tally (i + 1) refs' off') oc' cc'

-- | Replaces each reference with its blob bytes; references must be contiguous from the running offset.
expandSample :: B.ByteString -> Int -> Value -> Int -> Int -> Either String (Value, Int, Int)
expandSample blob i (Object sample) refs0 off0 = do
  (withViews, refs1, off1) <- case KM.lookup "views" sample of
    Just (Array views) -> do
      (expanded, refs, off) <- foldM view ([], refs0, off0) (zip [0 :: Int ..] (V.toList views))
      Right (KM.insert "views" (Array (V.fromList (reverse expanded))) sample, refs, off)
    _ -> Right (sample, refs0, off0)
  case KM.lookup "interrupt_stack" withViews of
    Just (Object stack) -> do
      (bytes, off2) <- resolve "interrupt_stack.bytes" (KM.lookup "bytes" stack) off1
      Right (Object (KM.insert "interrupt_stack" (Object (KM.insert "bytes" bytes stack)) withViews), refs1 + 1, off2)
    _ -> Right (Object withViews, refs1, off1)
 where
  view (acc, refs, off) (j, Object v) = do
    (bytes, off') <- resolve ("views[" ++ show j ++ "].bytes") (KM.lookup "bytes" v) off
    Right (Object (KM.insert "bytes" bytes v) : acc, refs + 1, off')
  view _ (j, _) = Left (here ("views[" ++ show j ++ "]") "object expected")
  resolve what (Just (Object ref)) off
    | KM.size ref == 2
    , Just (Number o) <- KM.lookup "offset" ref
    , Just (Number l) <- KM.lookup "length" ref
    , Just offset <- toBoundedInteger o
    , Just len <- toBoundedInteger l = do
        unless (offset == off) (Left (here what ("offset " ++ show offset ++ " but the previous reference ends at " ++ show off)))
        unless (len >= 0 && len <= B.length blob - off) (Left (here what ("length " ++ show len ++ " exceeds the blob")))
        Right (Array (V.generate len (\k -> Number (fromIntegral (BU.unsafeIndex blob (off + k))))), off + len)
  resolve what _ _ = Left (here what "reference {offset,length} expected")
  here what msg = "sample " ++ show i ++ " member " ++ what ++ ": " ++ msg
expandSample _ i _ _ _ = Left ("sample " ++ show i ++ ": object expected")

-- | Path to the first difference between an original value and an expanded compact value.
difference :: Value -> Value -> String
difference (Object a) (Object b)
  | Just k <- find (\k -> not (KM.member k b)) keys = "." ++ K.toString k ++ " only in original"
  | Just k <- find (\k -> not (KM.member k a)) (sort (KM.keys b)) = "." ++ K.toString k ++ " only in compact"
  | Just k <- find (\k -> KM.lookup k a /= KM.lookup k b) keys = "." ++ K.toString k ++ difference (at k a) (at k b)
 where
  keys = sort (KM.keys a)
  at k = fromMaybe Null . KM.lookup k
difference (Array a) (Array b)
  | V.length a /= V.length b = " length " ++ show (V.length a) ++ " in original, " ++ show (V.length b) ++ " in compact"
  | Just j <- V.findIndex id (V.zipWith (/=) a b) = "[" ++ show j ++ "]" ++ difference (a V.! j) (b V.! j)
difference a b = " differs: " ++ shown a ++ " in original, " ++ shown b ++ " in compact"
 where
  shown v = let s = C.unpack (encodeCanonical v) in if length s > 80 then take 77 s ++ "..." else s
