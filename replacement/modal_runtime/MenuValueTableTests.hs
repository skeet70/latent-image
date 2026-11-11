{-# LANGUAGE GHC2021 #-}
module Main where

import Control.Monad (unless)
import qualified Data.ByteString as B
import Data.Bits (shiftR)
import Data.Word (Word32)
import Replacement.ModalRuntime.MenuTables
import Replacement.ModalRuntime.MenuValueTable

main :: IO ()
main = do
  let image = image4 imageFixture
      document = ValueDocument image4Pin [PropertyTable 0xf6 sizeRows, PropertyTable 0xfe feRows] [0x200]
  case generateTables image document of
    Left reason -> fail reason
    Right generated -> do
      let tables = generatedConversions generated
          size = maybe [] id (lookup 0xf6 tables)
          aspect = maybe [] id (lookup 0xf9 tables)
      check "size table" (lookup 0xf6 tables == Just sizeRows)
      check "aspect table" (lookup 0xf9 tables == Just aspectRows)
      check "change marker table" (lookup 0x1e5 tables == Just [ConversionRow 0 0 0])
      check "node table" (lookup 0xfe tables == Just feRows)
      check "pairs" (generatedPairs generated == expectedPairs)
      check "token one" (converted size 0 == Just 0 && converted aspect 1 == Just 1)
      check "token two" (converted size 0 == Just 0 && converted aspect 2 == Just 2)
  check "another image 4 refused" (isLeft (generateTables image document {documentImage4 = "other"}))
  check "rows that differ from the image refused"
    (isLeft (generateTables image document {documentProperties = [PropertyTable 0xf6 (ConversionRow 9 9 9 : sizeRows)]}))
  check "a changed token-1 pair refused" (isLeft (generateTables (image4 (placePairs [(1, 0, 2)] imageFixture)) document))
  check "a null pointer that decodes refused"
    (isLeft (generateTables image document {documentNullProperties = [0xf6]}))

converted :: [ConversionRow] -> Word32 -> Maybe Word32
converted rows key = case [conversionStored row | row <- rows, conversionEffective row == key] of
  stored : _ -> Just stored
  [] -> Nothing

check :: String -> Bool -> IO ()
check label passed = unless passed (fail label)

isLeft :: Either a b -> Bool
isLeft (Left _) = True
isLeft (Right _) = False

sizeRows, feRows :: [ConversionRow]
sizeRows = [ConversionRow 0 0 0, ConversionRow 1 1 1, ConversionRow 2 2 2]
feRows = [ConversionRow 5 6 7]

expectedPairs :: [(Word32, Word32, Word32)]
expectedPairs =
  [ (0, 0, 0), (1, 0, 1), (2, 0, 2), (3, 0, 3), (4, 1, 0), (5, 1, 1)
  , (6, 1, 2), (7, 1, 3), (8, 2, 0), (9, 2, 1), (10, 2, 2), (11, 2, 3)
  ]

imageFixture :: B.ByteString
imageFixture = placePairs [] base
 where
  base = foldl place (B.replicate (fromIntegral (imageEnd - 0x0061c400)) 0)
    [ (0x0081b09c + 4 * 0xf6, words32 [0x00770000])
    , (0x0081b09c + 4 * 0xf9, words32 [0x00770100])
    , (0x0081b09c + 4 * 0x1e5, words32 [0x00770200])
    , (0x0081b09c + 4 * 0xfe, words32 [0x00770300])
    , (0x00770000, words32 (concatMap rowWords sizeRows ++ [0xffff, 0xffff, 0xffff]))
    , (0x00770100, words32 (concatMap rowWords aspectRows ++ [0xffff, 0xffff, 0xffff]))
    , (0x00770200, words32 [0, 0, 0, 0xffff, 0xffff, 0xffff])
    , (0x00770300, words32 (concatMap rowWords feRows ++ [0xffff, 0xffff, 0xffff]))
    ]
  rowWords (ConversionRow stored column1 effective) = [stored, column1, effective]

aspectRows :: [ConversionRow]
aspectRows = [ConversionRow 0 0 0, ConversionRow 1 1 1, ConversionRow 2 2 2, ConversionRow 3 3 3]

placePairs :: [(Word32, Word32, Word32)] -> B.ByteString -> B.ByteString
placePairs overrides bytes = foldl placePairsOne bytes pairs
 where
  defaults = expectedPairs
  pairs = foldl (\current (token, size, aspect) -> replace token size aspect current) defaults overrides
  replace token size aspect = map (\(t, s, a) -> if t == token then (t, size, aspect) else (t, s, a))
  placePairsOne current (token, size, aspect) =
    place current (0x007a00cc + 16 * token, words32 [size, aspect])

place :: B.ByteString -> (Word32, B.ByteString) -> B.ByteString
place bytes (address, value) =
  let offset = fromIntegral (address - 0x0061c400)
  in B.take offset bytes <> value <> B.drop (offset + B.length value) bytes

words32 :: [Word32] -> B.ByteString
words32 = B.concat . map word32

word32 :: Word32 -> B.ByteString
word32 value = B.pack [fromIntegral (value `shiftR` shift) | shift <- [0, 8, 16, 24]]

imageEnd :: Word32
imageEnd = 0x0081b900
