module Main (main) where

import Control.Monad (forM_, unless)
import qualified Data.ByteString as B
import Data.Bits (shiftR)
import Data.Word (Word16, Word32)
import ReferenceScan

main :: IO ()
main = do
  forM_ cases $ \(name, kind, base, bytes, target) -> do
    let image = Image name (Address base) bytes
        refs = scanReferences [Address target] image
    unless (any ((== kind) . referenceKind) refs) (fail (name ++ ": missing reference"))
    unless (null (scanReferences [Address (target + 4)] image)) (fail (name ++ ": near address accepted"))
  forM_ invalid $ \(name, bytes, target) ->
    unless (null (scanReferences [Address target] (Image name (Address 0x1000) bytes))) (fail (name ++ ": invalid encoding accepted"))
  putStrLn "reference-scan: 20 encoding cases with near-address negatives and 8 invalid/non-pair cases passed"

cases :: [(String, ReferenceKind, Word32, B.ByteString, Word32)]
cases =
  [ ("aligned pointer", PointerWord, 0x1000, words32 [0x12345678], 0x12345678)
  , ("halfword pointer", PointerHalfword, 0x1000, halves [0,0x5678,0x1234], 0x12345678)
  , ("Thumb pointer", PointerWord, 0x1000, words32 [0x12345679], 0x12345678)
  , ("A32 B", ArmB, 0x1000, words32 [0xea00000e], 0x1040)
  , ("A32 BL", ArmBL, 0x1000, words32 [0xeb00000e], 0x1040)
  , ("A32 BLX", ArmBLX, 0x1000, words32 [0xfa00000e], 0x1040)
  , ("A32 BLX H", ArmBLX, 0x1000, words32 [0xfb00000e], 0x1042)
  , ("Thumb BL", ThumbBL, 0x1000, halves [0xf000,0xf81e], 0x1040)
  , ("Thumb BLX aligned PC", ThumbBLX, 0x1002, halves [0xf000,0xe81e], 0x1040)
  , ("Thumb B.W", ThumbBW, 0x1000, halves [0xf000,0xb81e], 0x1040)
  , ("Thumb conditional B.W", ThumbConditionalBW, 0x1000, halves [0xf000,0x801e], 0x1040)
  , ("A32 MOVW MOVT", ArmMovPair, 0x1000, words32 [0xe3050678,0xe3410234], 0x12345678)
  , ("Thumb MOVW MOVT", ThumbMovPair, 0x1000, halves [0xf245,0x6078,0xf2c1,0x2034], 0x12345678)
  , ("A32 ADR add", ArmADR, 0x1000, words32 [0xe28f0038], 0x1040)
  , ("A32 ADR subtract", ArmADR, 0x1000, words32 [0xe24f0038], 0xfd0)
  , ("Thumb ADR narrow", ThumbADR, 0x1002, halves [0xa00f], 0x1040)
  , ("Thumb ADR wide add", ThumbADR, 0x1000, halves [0xf20f,0x003c], 0x1040)
  , ("Thumb ADR wide subtract", ThumbADR, 0x1000, halves [0xf2af,0x0034], 0xfd0)
  , ("A32 negative B", ArmB, 0x1000, words32 [0xeafffff2], 0xfd0)
  , ("Thumb negative B.W", ThumbBW, 0x1000, halves [0xf7ff,0xbfe6], 0xfd0)
  ]

invalid :: [(String, B.ByteString, Word32)]
invalid =
  [ ("Thumb BLX reserved bit", halves [0xf000,0xe81f], 0x1040)
  , ("Thumb reserved condition", halves [0xf380,0x801e], 0x1040)
  , ("A32 ADR S bit", words32 [0xe29f0038], 0x1040)
  , ("A32 ADR non-PC", words32 [0xe28e0038], 0x1040)
  , ("A32 pair wrong register", words32 [0xe3050678,0xe3411234], 0x12345678)
  , ("Thumb pair wrong register", halves [0xf245,0x6078,0xf2c1,0x2134], 0x12345678)
  , ("A32 pair beyond window", words32 ([0xe3050678] ++ replicate 16 0 ++ [0xe3410234]), 0x12345678)
  , ("Thumb pair beyond window", B.concat [halves [0xf245,0x6078], B.replicate 64 0, halves [0xf2c1,0x2034]], 0x12345678)
  ]

words32 :: [Word32] -> B.ByteString
words32 = B.pack . concatMap (\w -> [fromIntegral (shiftR w n) | n <- [0,8,16,24]])

halves :: [Word16] -> B.ByteString
halves = B.pack . concatMap (\w -> [fromIntegral w, fromIntegral (shiftR w 8)])
