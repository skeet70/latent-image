{-# LANGUAGE OverloadedStrings #-}
-- Behavioural tests for the typed witness receipt boundary.
module Main where

import Control.Monad (unless)
import Data.Aeson (Value (..), encode, object, (.=))
import qualified Data.Aeson.KeyMap as KeyMap
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as BL
import qualified Data.Vector as V
import Data.Word (Word32, Word64)
import Emulation.FirstStageWitness

expected :: Expected
expected =
  Expected
    { expectedStops =
        [ (OwnedEntry, GuestAddress 0x6000)
        , (StorageInit, GuestAddress 0x2034)
        , (TrailerGate, GuestAddress 0x1cc0)
        , (StageImages, GuestAddress 0x1d08)
        , (HandoffVector, GuestAddress 0x6100)
        ]
    , expectedReads =
        [ ExpectedRead OwnedImage (GuestAddress 0x6000) (B.pack [0xaa])
        , ExpectedRead HandoffWords (GuestAddress 0x4300) (B.pack [0x20, 0x00, 0x04, 0x00, 0xf8, 0xcd, 0x69, 0x00])
        , ExpectedRead SkipList (GuestAddress 0x4310) (B.replicate 4 0)
        , ExpectedRead LoaderA (GuestAddress 0x40000) (B.pack [0xbb])
        , ExpectedRead LoaderB (GuestAddress 0xd0000) (B.pack [0xcc])
        , ExpectedRead SegmentTable (GuestAddress 0x61c000) (B.pack [0xdd])
        , ExpectedRead Image4 (GuestAddress 0x61c400) (B.pack [0xee])
        , ExpectedRead Image4Bss (GuestAddress 0x89ec00) (B.replicate 4 0)
        , ExpectedRead Image4Arg (GuestAddress 0x61c40c) (B.pack [0xf8, 0xcd, 0x69, 0x00])
        , ExpectedRead Image4Publication (GuestAddress 0x61c188) (B.pack [0x10, 0x43, 0x00, 0x00])
        , ExpectedRead PeerCells (GuestAddress 0x40020000) (B.replicate 8 0)
        , ExpectedRead RetainedEventWord (GuestAddress 0x400a00c4) (B.replicate 4 0)
        ]
    , expectedHandoffTarget = GuestAddress 0x40020
    , expectedHandoffArgument = GuestAddress 0x69cdf8
    }

stopValue :: Int -> Word64 -> Word32 -> Word32 -> Word32 -> Word32 -> Word32 -> Value
stopValue core instructions r0 r1 r2 r3 cpsr =
  object
    [ "label" .= label
    , "address" .= address
    , "core" .= core
    , "instructions" .= instructions
    , "registers" .= object ["r0" .= r0, "r1" .= r1, "r2" .= r2, "r3" .= r3]
    , "sp" .= sp
    , "lr" .= (0 :: Word32)
    , "cpsr" .= cpsr
    ]
  where
    (label, address, sp) = case instructions of
      10 -> ("owned-first-stage-entry", 0x6000 :: Word32, 0xfff00f00)
      20 -> ("retained-storage-init", 0x2034, 0)
      30 -> ("retained-trailer-gate", 0x1cc0, 0)
      40 -> ("retained-stage-images", 0x1d08, 0)
      50 -> ("owned-handoff-vector", 0x6100, 0)
      _ -> ("", 0, 0)

readValue :: String -> Word32 -> B.ByteString -> Value
readValue label address content =
  object
    [ "label" .= label
    , "address" .= address
    , "length" .= B.length content
    , "sha256" .= sha256Hex content
    , "hex" .= Null
    ]

validReceipt :: Value
validReceipt =
  object
    [ "schema" .= (1 :: Int)
    , "result" .= ("passed" :: String)
    , "error" .= Null
    , "instructions" .= (60 :: Word64)
    , "machine_failure" .= Null
    , "stops"
        .= [ stopValue 0 10 0 2 0 0 0xd3
           , stopValue 0 20 0 0 0 0 0x13
           , stopValue 0 30 0 0 0 0 0x13
           , stopValue 0 40 0 0 0 0 0x13
           , stopValue 0 50 0x69cdf8 0 0x40020 0x4300 0xd3
           ]
    , "reads"
        .= [ readValue (readName label) address content
           | ExpectedRead label address content <- expectedReads expected
           ]
    ]

parseValid :: Value -> Either String Receipt
parseValid value = parseReceipt (BL.toStrict (encode value))

replaceStops :: ([Value] -> [Value]) -> Value -> Value
replaceStops change (Object root) =
  Object (KeyMap.insert "stops" (Array (V.fromList (change (stops root)))) root)
  where
    stops object = case KeyMap.lookup "stops" object of
      Just (Array values) -> V.toList values
      _ -> mempty
replaceStops _ value = value

firstStopField :: String -> Value -> Value -> Value
firstStopField field replacement (Object root) =
  Object (KeyMap.insert "stops" (Array (V.fromList (replace (stops root)))) root)
  where
    stops object = case KeyMap.lookup "stops" object of
      Just (Array values) -> V.toList values
      _ -> mempty
    replace [] = []
    replace (Object stop : rest) = Object (setField field replacement stop) : rest
    replace (value : rest) = value : replace rest
firstStopField _ _ value = value

firstRegister :: String -> Value -> Value -> Value
firstRegister field replacement (Object root) =
  Object (KeyMap.insert "stops" (Array (V.fromList (replace (stops root)))) root)
  where
    stops object = case KeyMap.lookup "stops" object of
      Just (Array values) -> V.toList values
      _ -> mempty
    replace [] = []
    replace (Object stop : rest) =
      case KeyMap.lookup "registers" stop of
        Just (Object registers) ->
          Object (KeyMap.insert "registers" (Object (KeyMap.insert field replacement registers)) stop) : rest
        _ -> Object stop : rest
    replace (value : rest) = value : replace rest
firstRegister _ _ value = value

firstStopDelete :: String -> Value -> Value
firstStopDelete field (Object root) =
  Object (KeyMap.insert "stops" (Array (V.fromList (replace (stops root)))) root)
  where
    stops object = case KeyMap.lookup "stops" object of
      Just (Array values) -> V.toList values
      _ -> mempty
    replace [] = []
    replace (Object stop : rest) = Object (KeyMap.delete field stop) : rest
    replace (value : rest) = value : replace rest
firstStopDelete _ value = value

setField :: String -> Value -> KeyMap.KeyMap Value -> KeyMap.KeyMap Value
setField field replacement object =
  case field of
    "sp" -> KeyMap.insert "sp" replacement object
    "core" -> KeyMap.insert "core" replacement object
    _ -> object

expectLeft :: String -> Either String a -> IO ()
expectLeft label result = case result of
  Left _ -> pure ()
  Right _ -> fail (label ++ ": expected refusal")

main :: IO ()
main = do
  unless (decodeLeWord "20000400" == Right 0x40020) (fail "little-endian target decode failed")
  unless (leWordAt (B.pack [0x20, 0x00, 0x04, 0x00]) 0 == Right 0x40020) (fail "byte little-endian target decode failed")
  unless (encodeLeWord 0x40020 == B.pack [0x20, 0x00, 0x04, 0x00]) (fail "little-endian target encode failed")
  unless (decodeLeWord "f8cd6900" == Right 0x69cdf8) (fail "little-endian argument decode failed")
  let value = validReceipt
  receipt <- either fail pure (parseValid value)
  either fail pure (validateReceipt expected receipt)
  let wrongCore = replaceStops (map setCore) value
  expectLeft "wrong core" (parseValid wrongCore >>= validateReceipt expected)
  let reordered = replaceStops reorder value
  expectLeft "reordered stops" (parseValid reordered >>= validateReceipt expected)
  let duplicated = replaceStops (\values -> take 1 values ++ values) value
  expectLeft "duplicate stops" (parseValid duplicated >>= validateReceipt expected)
  let nonStringFrame = firstStopField "sp" (String "0xfff00f00") value
  expectLeft "nonstring frame field" (parseValid nonStringFrame)
  let nonStringRegister = firstRegister "r0" (String "0x0") value
  expectLeft "nonstring register" (parseValid nonStringRegister)
  let missingField = firstStopDelete "sp" value
  expectLeft "missing stop field" (parseValid missingField)
  let wrongContent =
        expected
          { expectedReads =
              [ if expectedLabel entry == OwnedImage
                  then entry {expectedContent = B.pack [0xab]}
                  else entry
              | entry <- expectedReads expected
              ]
          }
  expectLeft "mismatched staged image" (validateReceipt wrongContent receipt)
  putStrLn "first-stage witness receipt tests: pass"
  where
    setCore (Object stop) = Object (KeyMap.insert "core" (Number 1) stop)
    setCore other = other
    reorder values
      | length values >= 2 = [values !! 1, values !! 0] ++ drop 2 values
      | otherwise = values
