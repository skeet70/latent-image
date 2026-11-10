{-# LANGUAGE OverloadedStrings #-}
module Main (main) where

import Control.Monad (forM_, unless)
import Data.Aeson (Value (..), eitherDecodeStrict', encode, object, toJSON, (.=))
import Data.Bits (xor)
import qualified Data.Aeson.Key as K
import qualified Data.Aeson.KeyMap as KM
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as BL
import Data.Either (isLeft)
import Data.IORef (atomicModifyIORef', newIORef)
import qualified Data.Text as T
import Emulation.ModalObservation (finalModalWatchSummary, parseFinalModalWatch)
import Emulation.ModalWatchStream (compactFinalWatch, readFinalModalWatch, readFinalModalWatchFrom)
import Emulation.Sha256 (sha256, sha256Hex)
import EmulatorUsb (decodeBridgeWatchLine)
import System.Directory (createDirectoryIfMissing, getTemporaryDirectory, removeFile, removePathForcibly)
import System.Environment (getArgs)
import System.FilePath ((</>))
import System.Exit (ExitCode (..))
import System.Process (readProcessWithExitCode)

row :: Int -> Value
row ordinal = object
  [ "source" .= ("cpu-write" :: String), "core" .= (0 :: Int), "pc" .= (0x3000 :: Int)
  , "address" .= (0x1000 :: Int), "physical" .= (0x1000 :: Int), "size" .= (4 :: Int)
  , "before" .= ([0, 0, 0, 0] :: [Int]), "value" .= (1 :: Int), "byte_order" .= ("little" :: String)
  , "instructions" .= ordinal, "tick" .= ordinal ]

watch :: [Value] -> Value
watch rows = object
  [ "ok" .= True, "enabled" .= True, "ranges" .= ([[0x1000, 0x1004]] :: [[Int]])
  , "records" .= rows, "record_count" .= length rows, "records_sha256" .= rowDigest rows
  , "violation" .= Null, "classes" .= [object ["range" .= ([0x1000, 0x1004] :: [Int]), "kinds" .= (["cpu-write", "host-write", "model-bus-write"] :: [String])]]
  , "views" .= ("fixed" :: String), "fetch_attribution" .= object ["sources" .= ([] :: [Value]), "ranges" .= ([[0x2000, 0x2004]] :: [[Int]])] ]

rowDigest :: [Value] -> String
rowDigest rows = sha256Hex (sha256 (B.concat [BL.toStrict (encode value) <> "\n" | value <- rows]))

chunks :: Int -> B.ByteString -> [B.ByteString]
chunks size bytes | B.null bytes = []
                  | otherwise = let (part, remaining) = B.splitAt size bytes in part : chunks size remaining

readChunks :: [B.ByteString] -> IO (Either String Value)
readChunks pieces = do
  source <- newIORef pieces
  let next = atomicModifyIORef' source $ \remaining -> case remaining of
        [] -> ([], B.empty)
        part : rest -> (rest, part)
  fmap finalModalWatchSummary <$> readFinalModalWatchFrom next

check :: String -> Bool -> IO ()
check label passed = unless passed (ioError (userError label))

compactionTests :: IO ()
compactionTests = do
  base <- getTemporaryDirectory
  let root = base </> "modal-watch-compaction"
  removePathForcibly root
  createDirectoryIfMissing True root
  let fixtureRows = map row [1 .. 8192]
      fixture = watch fixtureRows
      raw = root </> "raw.json"
      compact = root </> "e2e-modal-watch-final.json"
  B.writeFile raw (BL.toStrict (encode fixture))
  inline <- readFinalModalWatch raw
  check "frozen inline watch reads" (either (const False) (const True) inline)
  result <- compactFinalWatch raw compact
  check "compaction succeeds" (result == Right ())
  compacted <- readFinalModalWatch compact
  check "long compacted watch preserves the summary and row digest"
    (fmap finalModalWatchSummary compacted == fmap finalModalWatchSummary inline)
  compactBytes <- B.readFile compact
  finalValue <- either (ioError . userError) pure (eitherDecodeStrict' compactBytes) :: IO Value
  let writeFinal value = BL.writeFile compact (encode value)
      modifyFinal change = do
        bytes <- B.readFile compact
        value <- either (ioError . userError) pure (eitherDecodeStrict' bytes)
        changed <- change value
        writeFinal changed
      setSummary key value = modifyFinal $ \current -> case current of
        Object fields -> pure (Object (KM.insert key value fields))
        _ -> ioError (userError "compacted final is not an object")
      setReference pairs = modifyFinal $ \current -> case current of
        Object fields -> case KM.lookup "records" fields of
          Just (Object reference) -> pure (Object (KM.insert "records" (Object (foldr (uncurry KM.insert) reference pairs)) fields))
          _ -> ioError (userError "compacted reference is not an object")
        _ -> ioError (userError "compacted final is not an object")
      reject label = do
        bad <- readFinalModalWatch compact
        check label (isLeft bad)
      sidecar = compact ++ ".records.jsonl.gz"
      jsonl = compact ++ ".records.jsonl"
      rowBytes = [BL.toStrict (encode value) <> "\n" | value <- fixtureRows]
      jsonlBytes = B.concat rowBytes
      replaceRows bytes = do
        B.writeFile jsonl bytes
        (code, _, errors) <- readProcessWithExitCode "gzip" ["-n", "-f", jsonl] ""
        check ("fixture compression failed: " ++ errors) (code == ExitSuccess)
        compressed <- B.readFile sidecar
        setReference
          [ ("sha256", String (T.pack (sha256Hex (sha256 compressed))))
          , ("bytes", toJSON (B.length compressed)) ]
  writeFinal finalValue
  setReference [("count", Number 3)]
  reject "compacted watch rejects a wrong row count"
  writeFinal finalValue
  setSummary "records_sha256" (String (T.pack (replicate 64 '0')))
  reject "compacted watch rejects a wrong row digest"
  writeFinal finalValue
  setReference [("sha256", String (T.pack (replicate 64 '0')))]
  reject "compacted watch rejects a wrong sidecar digest"
  sidecarBytes <- B.readFile sidecar
  writeFinal finalValue
  let truncated = B.take (B.length sidecarBytes - 8) sidecarBytes
  B.writeFile sidecar truncated
  setReference [("sha256", String (T.pack (sha256Hex (sha256 truncated)))), ("bytes", toJSON (B.length truncated))]
  reject "compacted watch rejects a truncated gzip"
  let corrupt = B.concat
        [ B.take (B.length sidecarBytes - 5) sidecarBytes
        , B.singleton (B.index sidecarBytes (B.length sidecarBytes - 5) `xor` 1)
        , B.drop (B.length sidecarBytes - 4) sidecarBytes ]
  writeFinal finalValue
  B.writeFile sidecar corrupt
  setReference [("sha256", String (T.pack (sha256Hex (sha256 corrupt)))), ("bytes", toJSON (B.length corrupt))]
  reject "compacted watch rejects a corrupt gzip CRC"
  writeFinal finalValue
  B.writeFile sidecar sidecarBytes
  setSummary "views" (String "observe")
  reject "long compacted watch rejects an invalid summary"
  writeFinal finalValue
  replaceRows (B.concat (reverse rowBytes))
  reject "long compacted watch rejects reordered rows with the original digest"
  writeFinal finalValue
  replaceRows (B.init jsonlBytes)
  reject "long compacted watch rejects a final row without LF"
  writeFinal finalValue
  replaceRows (jsonlBytes <> "\n")
  reject "long compacted watch rejects a trailing blank row"
  writeFinal finalValue
  replaceRows (jsonlBytes <> "{\n")
  reject "long compacted watch rejects malformed JSON after valid rows"
  writeFinal finalValue
  let invalidRow = case row 8192 of
        Object fields -> Object (KM.insert "size" (Number 8) fields)
        value -> value
      invalidBytes = B.concat (init rowBytes) <> BL.toStrict (encode invalidRow) <> "\n"
  replaceRows invalidBytes
  setSummary "records_sha256" (String (T.pack (sha256Hex (sha256 invalidBytes))))
  reject "long compacted watch rejects an invalid late witness with matching digests"
  writeFinal finalValue
  replaceRows (jsonlBytes <> B.replicate (1024 * 1024 + 1) 32 <> "\n")
  reject "long compacted watch rejects an oversized late row"
  writeFinal finalValue
  let spacedBytes = B.concat [" \t" <> B.init bytes <> " \r\n" | bytes <- rowBytes]
  replaceRows spacedBytes
  setSummary "records_sha256" (String (T.pack (sha256Hex (sha256 spacedBytes))))
  spaced <- readFinalModalWatch compact
  check "long compacted watch hashes exact row bytes including JSON whitespace"
    (either (const False) (const True) spaced)
  writeFinal finalValue
  B.writeFile sidecar sidecarBytes
  removeFile sidecar
  reject "compacted watch rejects a missing sidecar"
  removePathForcibly root

main :: IO ()
main = do
  arguments <- getArgs
  case arguments of
    [] -> runTests
    ["--read-final", path] -> do
      result <- readFinalModalWatch path
      case result of
        Left problem -> ioError (userError problem)
        Right final -> BL.putStr (encode (finalModalWatchSummary final) <> "\n")
    _ -> ioError (userError "usage: modal-watch-stream-tests [--read-final PATH]")

runTests :: IO ()
runTests = do
  let fixture = watch [row 1, row 2]
      encoded value = BL.toStrict (encode value)
      old bytes = finalModalWatchSummary <$> parseFinalModalWatch (decodeBridgeWatchLine bytes)
      equivalent label bytes = forM_ [1, 2, 7, 4096, 65536] $ \size -> do
        streamed <- readChunks (chunks size bytes)
        check label (streamed == old bytes)
  equivalent "streaming watch preserves complete header and row admission" (encoded fixture)
  equivalent "empty terminal watch preserves its source declaration" (encoded (watch []))
  case fixture of
    Object fields -> do
      let ordered = [(key, value) | (key, value) <- KM.toList fields, key /= "records"]
          member (key, value) = encode (String (K.toText key)) <> ":" <> encode value
          entries = map member ordered
          rows = "\"records\":" <> encode [row 1, row 2]
          body values = BL.toStrict ("{" <> BL.intercalate "," values <> "}")
      equivalent "records may precede every summary field" (body (rows : entries))
      equivalent "records may follow every summary field" (body (entries ++ [rows]))
      equivalent "nonempty records may have whitespace before the next field" (body ((rows <> " \n\t\r") : entries))
      equivalent "nonempty records may have whitespace before the object closes" (body (entries ++ [rows <> " \n\t\r"]))
      let emptyEntries = map member [(key, entry) | (key, entry) <- ordered, key /= "record_count"] ++ ["\"record_count\":0"]
      equivalent "empty records may have whitespace before the next field" (body ("\"records\":[] \n\t\r" : emptyEntries))
      equivalent "empty records may have whitespace before the object closes" (body (emptyEntries ++ ["\"records\":[] \n\t\r"]))
      forM_ ["\v", "\f"] $ \invalidSpace ->
        forM_
          [ invalidSpace <> encoded fixture
          , "{" <> invalidSpace <> B.drop 1 (encoded fixture)
          , encoded fixture <> invalidSpace
          , body (("\"records\":" <> BL.fromStrict invalidSpace <> "[]") : emptyEntries)
          , body (("\"records\":[" <> BL.fromStrict invalidSpace <> "]") : emptyEntries)
          , body (("\"records\":[]" <> BL.fromStrict invalidSpace) : emptyEntries)
          ] $ \invalid -> do
            streamed <- readChunks (chunks 1 invalid)
            check "non-JSON delimiter whitespace refuses" (isLeft streamed)
      equivalent "JSON string escapes and UTF-8 cross arbitrary chunk boundaries"
        (encoded (Object (KM.insert "violation" (String "quoted \" brace } bracket ] backslash \\ newline\n λ") fields)))
      forM_
        [ Object (KM.delete "records" fields)
        , Object (KM.delete "ranges" fields)
        , Object (KM.insert "record_count" (Number 3) fields)
        , Object (KM.insert "records_sha256" (String "short") fields)
        , Object (KM.insert "views" (String "observe") fields)
        , Object (KM.insert "fetch_attribution" (object ["sources" .= [row 1], "ranges" .= ([[0x2000, 0x2004]] :: [[Int]])]) fields)
        , Object (KM.insert "ok" (Bool False) fields)
        , Object (KM.insert "classes" (toJSON [object ["range" .= ([0x1000, 0x1004] :: [Int]), "kinds" .= (["cpu-read"] :: [String])]]) fields)
        , Object (KM.insert "ranges" (toJSON ([[0x1004, 0x1000]] :: [[Int]])) fields)
        , Object (KM.insert "records" (case row 2 of Object record -> toJSON [row 1, Object (KM.insert "size" (Number 8) record)]; value -> toJSON [value]) fields)
        ] $ \invalid -> do
          let bytes = encoded invalid
          check "legacy admission refuses invalid fixture" (isLeft (old bytes))
          streamed <- readChunks (chunks 3 bytes)
          check "streaming admission refuses invalid header or late row" (isLeft streamed)
    _ -> ioError (userError "watch fixture is not an object")
  forM_ [B.init (encoded fixture), encoded fixture <> "{}", B.take 100 (encoded fixture)] $ \invalid -> do
    streamed <- readChunks (chunks 1 invalid)
    check "truncated or trailing watch data refuses" (isLeft streamed)
  let large = encoded (watch (map row [1 .. 4096]))
  equivalent "multi-chunk terminal watch preserves every typed row" large
  compactionTests
  putStrLn "streaming modal watch admission passed"
