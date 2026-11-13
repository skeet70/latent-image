{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}
module Main (main) where

import Control.Monad (unless)
import Data.Aeson (Value, encode, object, (.=))
import qualified Data.ByteString.Lazy as L
import qualified Data.ByteString as B
import System.Environment (getArgs)
import Replacement.Saclac.PayloadProjection

main :: IO ()
main = do
  declaration <- parse (policy [field 0 2, field 4 1] [padding 2 2])
  check (projectPayload declaration Outgoing (QueueId 47) (Operation 16166) [1,2,3,4,5] == Right [1,2,5])
  check (projectPayload declaration Outgoing (QueueId 47) (Operation 16166) [1,2,9,8,5] == Right [1,2,5])
  check (projectPayload declaration Outgoing (QueueId 47) (Operation 16166) [1,7,3,4,5] == Right [1,7,5])
  check (isLeft (projectPayload declaration Incoming (QueueId 47) (Operation 16166) [1,2,3,4,5]))
  check (isLeft (projectPayload declaration Outgoing (QueueId 50) (Operation 16166) [1,2,3,4,5]))
  check (isLeft (projectPayload declaration Outgoing (QueueId 47) (Operation 16167) [1,2,3,4,5]))
  check (isLeft (projectPayload declaration Outgoing (QueueId 47) (Operation 16166) [1,2,3,4]))
  check (isLeft (parseDeclaration (L.toStrict (encode (policy [field 0 2] [padding 3 2])))))
  check (isLeft (parseDeclaration (L.toStrict (encode (policy [field 0 3, field 2 3] [])))))
  check (isLeft (parseDeclaration (L.toStrict (encode (policy [field 0 6] [])))))
  roundTrip <- either fail pure (parseDeclaration (L.toStrict (encode (declarationJson declaration))))
  check (projectPayload roundTrip Outgoing (QueueId 47) (Operation 16166) [1,2,3,4,5] == Right [1,2,5])
  arguments <- getArgs
  source <- case arguments of
    [path] -> B.readFile path
    _ -> fail "expected payload field declaration path"
  recovered <- either fail pure (parseDeclaration source)
  let image bytes = projectPayload recovered Outgoing (QueueId 47) (Operation 0x3f26) bytes
      zeros = replicate 232 0
      changed offset = take offset zeros ++ 1 : drop (offset + 1) zeros
  check (image zeros == image (changed 94))
  check (image zeros /= image (changed 96))
  check (isLeft (image (replicate 231 0)))
  putStrLn "payload field projection behavioral tests passed"
  where parse = either fail pure . parseDeclaration . L.toStrict . encode

policy :: [Value] -> [Value] -> Value
policy fields paddingRows = object
  [ "schema" .= (1 :: Int), "source_commit" .= ("synthetic-fixture" :: String)
  , "messages" .= [object
      [ "direction" .= ("outgoing" :: String), "queue" .= (47 :: Int), "operation" .= (16166 :: Int)
      , "bytes" .= (5 :: Int), "fields" .= fields, "padding" .= paddingRows
      ]]
  ]

field :: Int -> Int -> Value
field offset width = object ["name" .= ("synthetic-field" :: String), "offset" .= offset, "bytes" .= width, "sources" .= (["synthetic fixture"] :: [String])]

padding :: Int -> Int -> Value
padding offset width = object ["offset" .= offset, "bytes" .= width, "sources" .= (["synthetic fixture"] :: [String])]

isLeft :: Either a b -> Bool
isLeft (Left _) = True
isLeft (Right _) = False

check :: Bool -> IO ()
check yes = unless yes (fail "payload projection behavior differs")
