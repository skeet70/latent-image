{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}
module Replacement.RecordStore.CaseJson
  ( encodeCase
  , decodeCase
  ) where

import Data.Aeson (Result (..), Value (..), fromJSON, object, (.=))
import qualified Data.Aeson.Key as K
import qualified Data.Aeson.KeyMap as KM
import qualified Data.ByteString as B
import qualified Data.ByteString.Builder as BB
import qualified Data.ByteString.Lazy as BL
import qualified Data.Map.Strict as M
import qualified Data.Text as T
import qualified Data.Text.Encoding as TE
import Data.Word (Word16, Word32)
import FujiAF.JSON (decodeStrictIntegers, encodeCanonical)
import Replacement.RecordStore.Cases (CaseName (..), Operation (..), OracleCase (..), Registration (..), Source (..))
import Replacement.RecordStore.Generate (DomainError (..), ValidCase, oracleCase, validCase)
import Replacement.RecordStore.Model (Bank (..), BytePosition (..), Callback (..), Fault (..), Kind (..), Medium (..), Seed (..), bankBytes, geometry, poolBlock)

encodeCase :: ValidCase -> B.ByteString
encodeCase = encodeCanonical . caseValue . oracleCase

caseValue :: OracleCase -> Value
caseValue candidate = object
  [ "version" .= (1 :: Int)
  , "name" .= caseName (name candidate)
  , "kind" .= fromEnum (kind candidate)
  , "banks" .= object
      [ "bank0" .= hexText (bankBytes g Bank0 (initialMedium candidate))
      , "bank1" .= hexText (bankBytes g Bank1 (initialMedium candidate))
      ]
  , "operations" .= map operationValue (operations candidate)
  , "callbacks" .= callbackText (callbacks candidate)
  , "fault" .= ("none" :: T.Text)
  ]
  where
    g = geometry (kind candidate)

operationValue :: Operation -> Value
operationValue op = case op of
  Open -> object ["op" .= ("open" :: T.Text)]
  Register Normal -> object ["op" .= ("register" :: T.Text), "mode" .= ("normal" :: T.Text)]
  Register _ -> object ["op" .= ("register" :: T.Text), "mode" .= ("unsupported" :: T.Text)]
  Close -> object ["op" .= ("close" :: T.Text)]
  Latest -> object ["op" .= ("latest" :: T.Text)]
  Append (Seed seed) source -> object ["op" .= ("append" :: T.Text), "seed" .= seed, "source" .= sourceText source]
  EraseAll -> object ["op" .= ("erase-all" :: T.Text)]
  Collect -> object ["op" .= ("collect" :: T.Text)]
  ReadAt (BytePosition position) lengthBytes -> object ["op" .= ("read-at" :: T.Text), "position" .= position, "length" .= lengthBytes]
  Zero -> object ["op" .= ("zero" :: T.Text)]
  On selected nested -> object ["op" .= ("on" :: T.Text), "kind" .= fromEnum selected, "body" .= operationValue nested]
  CacheBoundary timestamp age0 age1 ->
    object ["op" .= ("cache-boundary" :: T.Text), "timestamp" .= timestamp, "age0" .= age0, "age1" .= age1]

sourceText :: Source -> T.Text
sourceText CallerSource = "caller"
sourceText StagingSource = "staging"

callbackText :: Callback -> T.Text
callbackText NoCallback = "none"
callbackText MarkerCallback = "marker"
callbackText CorrectingCallback = "correcting"
callbackText Hardware2 = "hardware2"
callbackText Hardware3 = "hardware3"

hexText :: B.ByteString -> T.Text
hexText = TE.decodeUtf8 . BL.toStrict . BB.toLazyByteString . BB.byteStringHex

decodeCase :: B.ByteString -> Either DomainError ValidCase
decodeCase bytes = do
  value <- either (Left . MalformedCaseJson) Right (decodeStrictIntegers bytes)
  candidate <- parseCase value
  validCase candidate

parseCase :: Value -> Either DomainError OracleCase
parseCase value = do
  values <- objectFields ["version", "name", "kind", "banks", "operations", "callbacks", "fault"] value
  case values of
    [versionValue, nameValue, kindValue, banksValue, operationsValue, callbacksValue, faultValue] ->
      parseFields versionValue nameValue kindValue banksValue operationsValue callbacksValue faultValue
    _ -> Left (MalformedCaseJson "case fields")

parseFields :: Value -> Value -> Value -> Value -> Value -> Value -> Value -> Either DomainError OracleCase
parseFields versionValue nameValue kindValue banksValue operationsValue callbacksValue faultValue = do
  version <- intValue "version" versionValue
  if version /= 1
    then Left (UnsupportedCaseVersion version)
    else do
      nameText <- textValue "name" nameValue
      kindNumber <- intValue "kind" kindValue
      kind' <- case kindNumber of
        0 -> Right Kind0
        1 -> Right Kind1
        2 -> Right Kind2
        3 -> Right Kind3
        4 -> Right Kind4
        _ -> Left (MalformedCaseJson "kind")
      bankValues <- objectFields ["bank0", "bank1"] banksValue
      case bankValues of
        [bank0Value, bank1Value] -> do
          bank0 <- hexBytes "bank0" bank0Value
          bank1 <- hexBytes "bank1" bank1Value
          operationValues <- listValue "operations" operationsValue
          operations' <- mapM parseOperation operationValues
          callback' <- parseCallback callbacksValue
          fault <- parseFault faultValue
          let g = geometry kind'
          Right OracleCase
            { name = CaseName (T.unpack nameText)
            , kind = kind'
            , initialMedium = Medium (M.fromList [(poolBlock g Bank0, bank0), (poolBlock g Bank1, bank1)])
            , operations = operations'
            , callbacks = callback'
            , injection = fault
            }
        _ -> Left (MalformedCaseJson "banks fields")

parseOperation :: Value -> Either DomainError Operation
parseOperation value = do
  tag <- field "op" value >>= textValue "op"
  case tag of
    "open" -> exact ["op"] value >> Right Open
    "register" -> do
      mode <- field "mode" value >>= textValue "mode"
      case mode of
        "normal" -> exact ["op", "mode"] value >> Right (Register Normal)
        _ -> Left (MalformedCaseJson "register mode")
    "close" -> exact ["op"] value >> Right Close
    "latest" -> exact ["op"] value >> Right Latest
    "append" -> do
      seed <- field "seed" value >>= word16Value "seed"
      sourceText' <- field "source" value >>= textValue "source"
      source <- case sourceText' of
        "caller" -> Right CallerSource
        "staging" -> Right StagingSource
        _ -> Left (MalformedCaseJson "append source")
      exact ["op", "seed", "source"] value
      Right (Append (Seed seed) source)
    "erase-all" -> exact ["op"] value >> Right EraseAll
    "collect" -> exact ["op"] value >> Right Collect
    "read-at" -> do
      position <- field "position" value >>= word32Value "position"
      lengthBytes <- field "length" value >>= intValue "length"
      exact ["op", "position", "length"] value
      Right (ReadAt (BytePosition position) lengthBytes)
    "zero" -> exact ["op"] value >> Right Zero
    _ -> Left (MalformedCaseJson ("unsupported operation " ++ T.unpack tag))

parseCallback :: Value -> Either DomainError Callback
parseCallback value = do
  tag <- textValue "callbacks" value
  case tag of
    "none" -> Right NoCallback
    "marker" -> Right MarkerCallback
    "correcting" -> Right CorrectingCallback
    _ -> Left (MalformedCaseJson "unsupported callbacks")

parseFault :: Value -> Either DomainError Fault
parseFault value = do
  tag <- textValue "fault" value
  case tag of
    "none" -> Right NoFault
    _ -> Left (MalformedCaseJson "unsupported fault")

exact :: [T.Text] -> Value -> Either DomainError ()
exact names value = () <$ objectFields names value

field :: T.Text -> Value -> Either DomainError Value
field name value = case value of
  Object fields -> maybe (Left (MalformedCaseJson ("missing field " ++ T.unpack name))) Right (KM.lookup (K.fromText name) fields)
  _ -> Left (MalformedCaseJson "object required")

objectFields :: [T.Text] -> Value -> Either DomainError [Value]
objectFields names value = case value of
  Object fields
    | KM.size fields == length names -> mapM (lookupField fields) names
    | otherwise -> Left (MalformedCaseJson ("unexpected fields " ++ show (map K.toText (KM.keys fields))))
  _ -> Left (MalformedCaseJson "object required")
  where
    lookupField fields name =
      maybe (Left (MalformedCaseJson ("missing field " ++ T.unpack name))) Right (KM.lookup (K.fromText name) fields)

listValue :: T.Text -> Value -> Either DomainError [Value]
listValue label value = case fromJSON value of
  Success items -> Right items
  Error _ -> Left (MalformedCaseJson (T.unpack label ++ " array required"))

textValue :: T.Text -> Value -> Either DomainError T.Text
textValue label value = case value of
  String text -> Right text
  _ -> Left (MalformedCaseJson (T.unpack label ++ " string required"))

intValue :: T.Text -> Value -> Either DomainError Int
intValue label value = case fromJSON value of
  Success number -> Right number
  Error _ -> Left (MalformedCaseJson (T.unpack label ++ " integer required"))

word16Value :: T.Text -> Value -> Either DomainError Word16
word16Value label value = case fromJSON value of
  Success number -> Right number
  Error _ -> Left (MalformedCaseJson (T.unpack label ++ " integer required"))

word32Value :: T.Text -> Value -> Either DomainError Word32
word32Value label value = case fromJSON value of
  Success number -> Right number
  Error _ -> Left (MalformedCaseJson (T.unpack label ++ " integer required"))

hexBytes :: T.Text -> Value -> Either DomainError B.ByteString
hexBytes label value = case value of
  String text -> either (Left . MalformedCaseJson) Right (decodeHex text)
  _ -> Left (MalformedCaseJson (T.unpack label ++ " string required"))

decodeHex :: T.Text -> Either String B.ByteString
decodeHex text
  | odd byteCount = Left "odd hex length"
  | not (B.all hexDigit bytes) = Left "invalid hex digit"
  | otherwise = Right (fst (B.unfoldrN (byteCount `div` 2) pair 0))
  where
    bytes = TE.encodeUtf8 text
    byteCount = B.length bytes
    pair index
      | index >= byteCount = Nothing
      | otherwise = Just (nibble (B.index bytes index) * 16 + nibble (B.index bytes (index + 1)), index + 2)
    hexDigit word = (word >= 0x30 && word <= 0x39) || (word >= 0x61 && word <= 0x66)
    nibble word
      | word <= 0x39 = word - 0x30
      | otherwise = word - 0x61 + 10
