{-# LANGUAGE OverloadedStrings #-}
module Main (main) where

import Control.Monad (forM_, unless)
import Data.Aeson (Value (..), encode, object, toJSON, (.=))
import qualified Data.Aeson.Key as K
import qualified Data.Aeson.KeyMap as KM
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as BL
import Data.Either (isLeft)
import Data.IORef (atomicModifyIORef', newIORef)
import Emulation.ModalObservation (finalModalWatchSummary, parseFinalModalWatch)
import Emulation.ModalWatchStream (readFinalModalWatchFrom)
import EmulatorUsb (decodeBridgeWatchLine)

row :: Int -> Value
row ordinal = object
  [ "source" .= ("cpu-write" :: String), "core" .= (0 :: Int), "pc" .= (0x3000 :: Int)
  , "address" .= (0x1000 :: Int), "physical" .= (0x1000 :: Int), "size" .= (4 :: Int)
  , "before" .= ([0, 0, 0, 0] :: [Int]), "value" .= (1 :: Int), "byte_order" .= ("little" :: String)
  , "instructions" .= ordinal, "tick" .= ordinal ]

watch :: [Value] -> Value
watch rows = object
  [ "ok" .= True, "enabled" .= True, "ranges" .= ([[0x1000, 0x1004]] :: [[Int]])
  , "records" .= rows, "record_count" .= length rows, "records_sha256" .= replicate 64 '0'
  , "violation" .= Null, "classes" .= [object ["range" .= ([0x1000, 0x1004] :: [Int]), "kinds" .= (["cpu-write", "host-write", "model-bus-write"] :: [String])]]
  , "views" .= ("fixed" :: String), "fetch_attribution" .= object ["sources" .= ([] :: [Value]), "ranges" .= ([[0x2000, 0x2004]] :: [[Int]])] ]

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

main :: IO ()
main = do
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
  putStrLn "streaming modal watch admission passed"
