{-# LANGUAGE OverloadedStrings #-}
-- Strict bounded JSON shared by host-only tools. No filesystem or native I/O.
module FujiAF.JSON (decodeStrict, decodeStrictIntegers, encodeCanonical) where
import Control.Applicative ((<|>))
import Control.Monad (unless)
import Data.Aeson (Value(..), eitherDecodeStrict')
import qualified Data.Aeson.Key as K
import qualified Data.Aeson.KeyMap as M
import qualified Data.Attoparsec.ByteString.Char8 as A
import qualified Data.Attoparsec.ByteString as AB
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import Data.Char (ord)
import Data.List (sortOn)
import Data.Scientific (base10Exponent, coefficient, formatScientific, FPFormat(Generic, Exponent))
import qualified Data.Text as T
import qualified Data.Vector as V
import Numeric (showHex)

decodeStrict :: B.ByteString -> Either String Value
decodeStrict = decode False
-- Runtime contracts contain exact JSON integers: 1.0 and 1e0 are rejected,
-- matching Python's exact type(int) checks before decoding loses token spelling.
decodeStrictIntegers :: B.ByteString -> Either String Value
decodeStrictIntegers = decode True

decode :: Bool -> B.ByteString -> Either String Value
decode integers bytes = A.parseOnly (space *> value 0 <* space <* A.endOfInput) bytes
 where
  space = A.skipWhile (`elem` [' ','\t','\n','\r'])
  token p = p <* space
  value :: Int -> A.Parser Value
  value depth = do
    unless (depth <= 64) (fail "JSON nesting cap exceeded")
    token (object depth <|> array depth <|> (String <$> str) <|> literal "true" (Bool True) <|> literal "false" (Bool False) <|> literal "null" Null <|> number)
  literal s v = A.string s *> pure v
  str = do
    (raw,()) <- A.match (A.char '"' *> chars)
    case eitherDecodeStrict' raw of
      Right (String s) -> pure s
      _ -> fail "invalid UTF-8 JSON string"
  chars = do
    ch <- AB.anyWord8
    case ch of
      34 -> pure ()
      92 -> AB.anyWord8 *> chars
      _ -> chars
  number = do
    raw <- A.takeWhile1 (`elem` ("-+0123456789.eE" :: String))
    -- All reviewed profiles fit within 128 numeric token bytes and exponent
    -- magnitude 128. Check spelling before Aeson can construct an enormous
    -- exponent; this also bounds canonicalization of unvalidated documents.
    unless (B.length raw <= 128) (fail "JSON number token cap exceeded")
    let (_,suffix) = C.break (`elem` ("eE" :: String)) raw
    case C.uncons suffix of
      Nothing -> pure ()
      Just (_,digits) -> case reads (C.unpack (if C.take 1 digits == "+" then C.drop 1 digits else digits)) :: [(Integer,String)] of
        [(exponentValue,"")] -> unless (abs exponentValue <= 128) (fail "JSON exponent cap exceeded")
        _ -> fail "invalid JSON exponent"
    unless (not integers || not (B.any (`elem` [46,101,69]) raw)) (fail "exact JSON integer required")
    case eitherDecodeStrict' raw of
      Right (Number n) -> pure (Number n)
      _ -> fail "invalid finite JSON number"
  object depth = do
    _ <- token (A.char '{')
    (A.char '}' *> pure (Object M.empty)) <|> members depth M.empty
  members depth acc = do
    key <- token str
    unless (not (M.member (K.fromText key) acc)) (fail "duplicate JSON key")
    _ <- token (A.char ':')
    v <- value (depth+1)
    let next = M.insert (K.fromText key) v acc
    (A.char '}' *> pure (Object next)) <|> (token (A.char ',') *> members depth next)
  array depth = do
    _ <- token (A.char '[')
    xs <- A.sepBy (value (depth+1)) (token (A.char ','))
    _ <- A.char ']'
    pure (Array (V.fromList xs))

-- Python json.dumps(sort_keys=True,separators=(",",":"),ensure_ascii=True)
-- compatible for the integer-only reviewed profiles, including UTF-16 escapes.
encodeCanonical :: Value -> B.ByteString
encodeCanonical = C.pack . encode
 where
  encode Null = "null"
  encode (Bool True) = "true"
  encode (Bool False) = "false"
  encode (String s) = quoted s
  encode (Number n)
    | base10Exponent n >= 0 && base10Exponent n <= 128 = show (coefficient n * 10 ^ base10Exponent n)
    | abs (toInteger (base10Exponent n)) > 128 = formatScientific Exponent Nothing n
    | otherwise = formatScientific Generic Nothing n
  encode (Array xs) = "[" ++ join (map encode (V.toList xs)) ++ "]"
  encode (Object xs) = "{" ++ join [quoted (K.toText k) ++ ":" ++ encode v | (k,v) <- sortOn (K.toText . fst) (M.toList xs)] ++ "}"
  join [] = ""
  join (x:xs) = x ++ concatMap (',':) xs
  quoted s = '"' : concatMap escape (T.unpack s) ++ "\""
  escape '"' = "\\\""
  escape '\\' = "\\\\"
  escape '\b' = "\\b"
  escape '\f' = "\\f"
  escape '\n' = "\\n"
  escape '\r' = "\\r"
  escape '\t' = "\\t"
  escape ch | ord ch < 32 || ord ch >= 127 = unicode (ord ch)
            | otherwise = [ch]
  unicode n | n <= 65535 = hex4 n
            | otherwise = hex4 (55296 + (n-65536) `div` 1024) ++ hex4 (56320 + (n-65536) `mod` 1024)
  hex4 n = let s=showHex n "" in "\\u" ++ replicate (4-length s) '0' ++ s
