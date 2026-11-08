{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}
module Main (main) where

import Control.Monad (unless)
import qualified Data.Attoparsec.ByteString as A
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import Data.Either (isLeft)
import FujiAF.JSON (decodeStrict, valueParser, valueParserAtomic)

check :: String -> Bool -> IO ()
check label passed = unless passed (ioError (userError label))

main :: IO ()
main = do
  let atomic bytes = A.parseOnly (valueParserAtomic <* A.endOfInput) bytes
      full bytes = A.parseOnly (valueParser <* A.endOfInput) bytes
  check "atomic parses a bare number" (atomic "1" == decodeStrict "1")
  check "atomic rejects trailing whitespace" (isLeft (atomic "1 "))
  check "atomic rejects leading whitespace" (isLeft (atomic " 1"))
  check "value parser accepts surrounding whitespace" (full " \n\t1\r " == decodeStrict "1")
  check "atomic parses an object" (atomic "{\"a\":1}" == decodeStrict "{\"a\":1}")
  check "duplicate keys are refused" (isLeft (decodeStrict "{\"a\":1,\"a\":2}"))
  let nesting = C.pack (concat (replicate 66 "[") ++ "0" ++ concat (replicate 66 "]"))
  check "nesting cap is enforced" (isLeft (decodeStrict nesting))
  check "number token cap is enforced" (isLeft (decodeStrict (B.replicate 200 49)))
