{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}
module Main (main) where

import Control.Monad (unless)
import Data.IORef (IORef, atomicModifyIORef', newIORef)
import System.IO.Unsafe (unsafePerformIO)
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
  let startRecord = projectPayload recovered Outgoing (QueueId 47) (Operation 0x3f56)
      startBytes = replicate 68 0
      mutate offset bytes = take offset bytes ++ 1 : drop (offset + 1) bytes
  mapM_ (\offset -> check (startRecord startBytes == startRecord (mutate offset startBytes))) ([20..23] ++ [48..63])
  mapM_ (\offset -> check (startRecord startBytes /= startRecord (mutate offset startBytes))) ([0..19] ++ [24..47] ++ [64..67])
  check (isLeft (startRecord (replicate 67 0)))
  let updateRecord = projectPayload recovered Outgoing (QueueId 47) (Operation 0x3f57)
      updateBytes = replicate 20 0
  mapM_ (\offset -> check (updateRecord updateBytes == updateRecord (mutate offset updateBytes))) [10,11]
  mapM_ (\offset -> check (updateRecord updateBytes /= updateRecord (mutate offset updateBytes))) ([0..9] ++ [12..19])
  let captureRecord = projectPayload recovered Outgoing (QueueId 47) (Operation 0x3f49)
      captureBytes = replicate 176 0
  mapM_ (\offset -> check (captureRecord captureBytes /= captureRecord (mutate offset captureBytes))) [0..175]
  check (isLeft (captureRecord (replicate 175 0)))
  let windowRequest = projectPayload recovered Outgoing (QueueId 44) (Operation 0x3c0c)
      windowBytes = replicate 80 0
      windowUnwritten = [10,11] ++ [17..19] ++ [28..31] ++ [34,35]
  mapM_ (\offset -> check (windowRequest windowBytes == windowRequest (mutate offset windowBytes))) windowUnwritten
  mapM_ (\offset -> check (windowRequest windowBytes /= windowRequest (mutate offset windowBytes))) (filter (`notElem` windowUnwritten) [0..79])
  let sourceRequest = projectPayload recovered Outgoing (QueueId 44) (Operation 0x3c11)
      sourceBytes = replicate 56 0
      sourceUnwritten = [18,19,26,27]
  mapM_ (\offset -> check (sourceRequest sourceBytes == sourceRequest (mutate offset sourceBytes))) sourceUnwritten
  mapM_ (\offset -> check (sourceRequest sourceBytes /= sourceRequest (mutate offset sourceBytes))) (filter (`notElem` sourceUnwritten) [0..55])
  let through = projectPayload recovered Outgoing (QueueId 48) (Operation 0x4002)
      throughBytes = replicate 208 0
      nonzero = take 32 throughBytes ++ [8] ++ drop 33 throughBytes
      zeroAlternate = take 32 throughBytes ++ [1] ++ drop 33 throughBytes
      nonzeroAlternate = take 32 throughBytes ++ [9] ++ drop 33 throughBytes
  mapM_ (\offset -> check (through throughBytes == through (mutate offset throughBytes))) ([36..39] ++ [148..156] ++ [162,163,182,183])
  mapM_ (\offset -> check (through nonzero /= through (mutate offset nonzero))) [36..39]
  check (through throughBytes /= through nonzero)
  check (through throughBytes /= through (mutate 180 throughBytes))
  check (through zeroAlternate == through (mutate 36 zeroAlternate))
  check (through nonzeroAlternate /= through (mutate 36 nonzeroAlternate))
  recoveredRoundTrip <- either fail pure (parseDeclaration (L.toStrict (encode (declarationJson recovered))))
  check (projectPayload recoveredRoundTrip Outgoing (QueueId 48) (Operation 0x4002) (mutate 36 nonzero) == through (mutate 36 nonzero))
  let half = projectPayload recovered Outgoing (QueueId 44) (Operation 0x3c0c)
      halfBytes = replicate 80 0
      holes = [10,11,17,18,19,28,29,30,31,34,35]
  mapM_ (\offset -> check (half halfBytes == half (mutate offset halfBytes))) holes
  mapM_ (\offset -> check (half halfBytes /= half (mutate offset halfBytes))) (filter (`notElem` holes) [0..79])
  let record = projectPayload recovered Outgoing (QueueId 45) (Operation 0x3d00)
      liveRecord = take 168 zeros ++ [0x68,0x35,0xf3,0x00]
      s1Record = take 168 zeros ++ [0xa8,0x41,0xf2,0x00]
      s1Padding = [4,5,6,7] ++ [30,31] ++ [38,39] ++ [54,55] ++ [109] ++ [114,115]
  check (fmap length (record liveRecord) == Right 172)
  check (fmap length (record s1Record) == Right 159)
  mapM_ (\offset -> check (record liveRecord /= record (mutate offset liveRecord))) [0..171]
  mapM_ (\offset -> check (record s1Record == record (mutate offset s1Record))) s1Padding
  mapM_ (\offset -> check (record s1Record /= record (mutate offset s1Record)))
    (filter (`notElem` s1Padding) [0..171])
  check (isLeft (record (take 168 zeros ++ [0,0,0,0])))
  check (isLeft (record (take 171 zeros)))
  let command402c = projectPayload recovered Outgoing (QueueId 48) (Operation 0x402c)
      commandBytes = take 56 (replicate 60 0) ++ [0xe4,0xa7,0xf3,0x00]
      commandUnwritten = [20..28] ++ [34,35] ++ [54,55]
  mapM_ (\offset -> check (command402c commandBytes == command402c (mutate offset commandBytes))) commandUnwritten
  mapM_ (\offset -> check (command402c commandBytes /= command402c (mutate offset commandBytes)))
    (filter (`notElem` commandUnwritten) [0..59])
  check (isLeft (command402c (take 56 (replicate 60 0))))
  let alternateBytes = take 56 (replicate 60 0) ++ [0x08,0x27,0xf2,0x00]
      alternateUnwritten = [20..28] ++ [30] ++ [33,34,35] ++ [54,55]
  mapM_ (\offset -> check (command402c alternateBytes == command402c (mutate offset alternateBytes))) alternateUnwritten
  mapM_ (\offset -> check (command402c alternateBytes /= command402c (mutate offset alternateBytes)))
    (filter (`notElem` alternateUnwritten) [0..59])
  discriminating
  putStrLn "payload field projection behavioral tests passed"
  where parse = either fail pure . parseDeclaration . L.toStrict . encode

-- Two senders of one operation are separated by the word each copies at the
-- same offset, and a payload matching neither is refused.
discriminating :: IO ()
discriminating = do
  declaration <- parse (twoSenders
    [ sender 0 0x01020304 [field 0 4, field 4 1] [padding 5 3]
    , sender 0 0x01020305 [field 0 4] [padding 4 4] ])
  let project = projectPayload declaration Outgoing (QueueId 47) (Operation 16166)
  check (project [4,3,2,1,9,0,0,0] == Right [4,3,2,1,9])
  check (project [5,3,2,1,9,0,0,0] == Right [5,3,2,1])
  check (isLeft (project [6,3,2,1,9,0,0,0]))
  check (isLeft (project [4,3,2,1,9,0,0]))
  check (isLeft (project [4,3,2,1,9,0,0,0,0]))
  roundTrip <- either fail pure (parseDeclaration (L.toStrict (encode (declarationJson declaration))))
  check (projectPayload roundTrip Outgoing (QueueId 47) (Operation 16166) [5,3,2,1,9,0,0,0] == Right [5,3,2,1])
  check (isLeft (parseDeclaration (L.toStrict (encode (twoSenders
    [sender 0 0x01020304 [field 0 4] [padding 4 4], sender 0 0x01020304 [field 0 4] [padding 4 4]])))))
  check (isLeft (parseDeclaration (L.toStrict (encode (twoSenders
    [sender 0 0x01020304 [field 0 4] [padding 4 4], plain [field 0 4] [padding 4 4]])))))
  check (isLeft (parseDeclaration (L.toStrict (encode (twoSenders
    [sender 5 0x01020304 [field 0 4] [padding 4 4]])))))
  check (isLeft (parseDeclaration (L.toStrict (encode (twoSenders
    [sender 7 0x01020304 [field 0 4] [padding 4 4]])))))
  where parse = either fail pure . parseDeclaration . L.toStrict . encode

twoSenders :: [Value] -> Value
twoSenders entries = object
  [ "schema" .= (1 :: Int), "source_commit" .= ("synthetic-fixture" :: String)
  , "messages" .= entries
  ]

sender :: Int -> Int -> [Value] -> [Value] -> Value
sender discriminatorOffset value fields paddingRows = object
  [ "direction" .= ("outgoing" :: String), "queue" .= (47 :: Int), "operation" .= (16166 :: Int)
  , "bytes" .= (8 :: Int), "fields" .= fields, "padding" .= paddingRows
  , "discriminator" .= object ["offset" .= discriminatorOffset, "value" .= value]
  ]

plain :: [Value] -> [Value] -> Value
plain fields paddingRows = object
  [ "direction" .= ("outgoing" :: String), "queue" .= (47 :: Int), "operation" .= (16166 :: Int)
  , "bytes" .= (8 :: Int), "fields" .= fields, "padding" .= paddingRows
  ]

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
check yes = do
  index <- atomicModifyIORef' checkIndex (\value -> (value + 1, value))
  unless yes (fail ("payload projection behavior differs at check " ++ show index))

{-# NOINLINE checkIndex #-}
checkIndex :: IORef Int
checkIndex = unsafePerformIO (newIORef 0)
