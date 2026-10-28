{-# LANGUAGE OverloadedStrings #-}
-- | Area-3 record domain: the documented parity contract, its oracle and the
-- mutation domains. The oracle is written from the contract, not from the Rust
-- candidate.
module Replacement.Area3.Domain
  ( Record (..)
  , mkRecord
  , recordBytes
  , recordHex
  , recordSize
  , protectedSize
  , protectedBits
  , rowCount
  , columnCount
  , rowParityBits
  , rowParityOffset
  , columnParityOffset
  , rowParityMask
  , RowIndex (..)
  , ColumnIndex (..)
  , BitSite (..)
  , Syndrome (..)
  , Outcome (..)
  , outcomeClass
  , outcomeValue
  , describeOutcome
  , allBitSites
  , bitIndex
  , siteOfIndex
  , flipBit
  , flipBits
  , flipRecordBit
  , wordAt
  , storedRowWord
  , highRowBits
  , computedParity
  , storedParity
  , Oracle (..)
  , oracle
  , MutationKind (..)
  , Mutation (..)
  , singleBitMutations
  , twoBitMutations
  , mutationCounts
  , GroundKind (..)
  , GroundCase (..)
  , groundCases
  , protectedBody
  , buildRecord
  , constructedBodies
  ) where

import qualified Data.ByteString as B
import Data.Bits (complement, popCount, shiftL, shiftR, testBit, xor, (.&.), (.|.))
import Data.Word (Word16, Word32, Word8)
import Numeric (showHex)

recordSize :: Int
recordSize = 52

protectedSize :: Int
protectedSize = 44

protectedBits :: Int
protectedBits = protectedSize * 8

rowCount :: Int
rowCount = protectedSize `div` 4

columnCount :: Int
columnCount = 32

rowParityBits :: Int
rowParityBits = rowCount

rowParityOffset :: Int
rowParityOffset = protectedSize

columnParityOffset :: Int
columnParityOffset = protectedSize + 4

rowParityMask :: Word32
rowParityMask = (1 `shiftL` rowParityBits) - 1

newtype Record = Record B.ByteString
  deriving (Eq)

instance Show Record where
  show = recordHex

recordBytes :: Record -> B.ByteString
recordBytes (Record bytes) = bytes

recordHex :: Record -> String
recordHex (Record bytes) = concatMap hexByte (B.unpack bytes)
  where
    hexByte byte =
      let text = showHex byte ""
       in replicate (2 - length text) '0' ++ text

mkRecord :: B.ByteString -> Either String Record
mkRecord bytes
  | B.length bytes == recordSize = Right (Record bytes)
  | otherwise = Left ("record must be " ++ show recordSize ++ " bytes")

newtype RowIndex = RowIndex Word8
  deriving (Eq, Ord, Show)

newtype ColumnIndex = ColumnIndex Word8
  deriving (Eq, Ord, Show)

data BitSite = BitSite RowIndex ColumnIndex
  deriving (Eq, Ord, Show)

data Syndrome = Syndrome
  { syndromeRow :: Word16
  , syndromeColumn :: Word32
  }
  deriving (Eq, Show)

data Outcome
  = Accepted
  | Corrected BitSite
  | Rejected Syndrome
  deriving (Eq, Show)

outcomeClass :: Outcome -> String
outcomeClass Accepted = "accepted"
outcomeClass (Corrected _) = "corrected"
outcomeClass (Rejected _) = "rejected"

outcomeValue :: Outcome -> Word32
outcomeValue Accepted = 0
outcomeValue (Corrected _) = 0
outcomeValue (Rejected _) = 1

describeOutcome :: Outcome -> String
describeOutcome Accepted = "accepted"
describeOutcome (Corrected (BitSite (RowIndex row) (ColumnIndex column))) =
  "corrected row " ++ show row ++ " column " ++ show column
describeOutcome (Rejected (Syndrome row column)) =
  "rejected row 0x" ++ pad 4 (showHex row "") ++ " column 0x" ++ pad 8 (showHex column "")
  where
    pad width text = replicate (width - length text) '0' ++ text

allBitSites :: [BitSite]
allBitSites =
  [ BitSite (RowIndex (fromIntegral row)) (ColumnIndex (fromIntegral column))
  | row <- [0 .. rowCount - 1]
  , column <- [0 .. columnCount - 1]
  ]

bitIndex :: BitSite -> Int
bitIndex (BitSite (RowIndex row) (ColumnIndex column)) =
  fromIntegral row * columnCount + fromIntegral column

siteOfIndex :: Int -> Maybe BitSite
siteOfIndex index
  | index < 0 || index >= protectedBits = Nothing
  | otherwise =
      Just
        ( BitSite
            (RowIndex (fromIntegral (index `div` columnCount)))
            (ColumnIndex (fromIntegral (index `mod` columnCount)))
        )

flipRecordBit :: Int -> Record -> Record
flipRecordBit index (Record bytes) =
  Record
    ( B.take offset bytes
        <> B.singleton (B.index bytes offset `xor` (1 `shiftL` bit))
        <> B.drop (offset + 1) bytes
    )
  where
    offset = index `div` 8
    bit = index `mod` 8

flipBit :: BitSite -> Record -> Record
flipBit site = flipRecordBit (bitIndex site)

flipBits :: [BitSite] -> Record -> Record
flipBits sites record = foldl' (flip flipBit) record sites

wordAt :: Int -> Record -> Word32
wordAt index (Record bytes) =
  foldl'
    (\word offset -> word .|. (fromIntegral (B.index bytes (index * 4 + offset)) `shiftL` (8 * offset)))
    0
    [0 .. 3]

storedRowWord :: Record -> Word32
storedRowWord (Record bytes) =
  foldl'
    (\word offset -> word .|. (fromIntegral (B.index bytes (rowParityOffset + offset)) `shiftL` (8 * offset)))
    0
    [0 .. 3]

storedColumnWord :: Record -> Word32
storedColumnWord (Record bytes) =
  foldl'
    (\word offset -> word .|. (fromIntegral (B.index bytes (columnParityOffset + offset)) `shiftL` (8 * offset)))
    0
    [0 .. 3]

highRowBits :: Record -> Word32
highRowBits record = storedRowWord record .&. complement rowParityMask

computedParity :: Record -> Syndrome
computedParity record = Syndrome row column
  where
    words' = [wordAt index record | index <- [0 .. rowCount - 1]]
    column = foldl' xor 0 words'
    row =
      foldl'
        (\acc (index, word) -> acc .|. (fromIntegral (popCount word .&. 1) `shiftL` index))
        0
        (zip [0 :: Int ..] words')

storedParity :: Record -> Syndrome
storedParity record =
  Syndrome
    (fromIntegral (storedRowWord record .&. rowParityMask))
    (storedColumnWord record)

data Oracle = Oracle
  { oracleOutcome :: Outcome
  , oracleFinal :: Record
  }
  deriving (Eq, Show)

oracle :: Record -> Oracle
oracle record =
  case Syndrome (syndromeRow computed `xor` syndromeRow stored) (syndromeColumn computed `xor` syndromeColumn stored) of
    Syndrome 0 0 -> Oracle Accepted record
    syndrome@(Syndrome row column)
      | popCount row == 1 && popCount column == 1 ->
          let site = BitSite (rowOf row) (columnOf column)
           in Oracle (Corrected site) (flipBit site record)
      | otherwise -> Oracle (Rejected syndrome) record
  where
    computed = computedParity record
    stored = storedParity record
    rowOf value = RowIndex (fromIntegral (trailing value))
    columnOf value = ColumnIndex (fromIntegral (trailing value))
    trailing value = length (takeWhile (not . testBit value) [0 :: Int ..])

data MutationKind
  = SingleBit BitSite
  | TwoBit BitSite BitSite
  deriving (Eq, Show)

data Mutation = Mutation
  { mutationId :: String
  , mutationKind :: MutationKind
  , mutationRecord :: Record
  , mutationExpected :: Oracle
  }
  deriving (Show)

singleBitMutations :: Record -> [Mutation]
singleBitMutations base =
  [ Mutation
      ("single-" ++ show index)
      (SingleBit site)
      mutated
      (oracle mutated)
  | (index, site) <- zip [0 :: Int ..] allBitSites
  , let mutated = flipBit site base
  ]

twoBitMutations :: Record -> [Mutation]
twoBitMutations base =
  [ Mutation
      ("two-" ++ show first ++ "-" ++ show second)
      (TwoBit siteA siteB)
      mutated
      (oracle mutated)
  | (first, siteA) <- zip [0 :: Int ..] allBitSites
  , (second, siteB) <- zip [0 :: Int ..] allBitSites
  , second > first
  , let mutated = flipBits [siteA, siteB] base
  ]

mutationCounts :: (Int, Int)
mutationCounts = (length allBitSites, length allBitSites * (length allBitSites - 1) `div` 2)

data GroundKind
  = ValidRecord Int
  | SingleBitRecord Int BitSite
  | RowParityBit Int
  | ColumnParityBit Int
  | ParityPair Int Int
  deriving (Eq, Show)

data GroundCase = GroundCase
  { groundId :: String
  , groundRecord :: Record
  , groundExpected :: Oracle
  , groundKind :: GroundKind
  }
  deriving (Show)

protectedBody :: Record -> B.ByteString
protectedBody (Record bytes) = B.take protectedSize bytes

constructedBodies :: Record -> [B.ByteString]
constructedBodies base =
  [ B.replicate protectedSize 0
  , B.map complement (protectedBody base)
  , B.pack [fromIntegral ((index * 37 + 11) `mod` 256) | index <- [0 .. protectedSize - 1]]
  ]

buildRecord :: B.ByteString -> Either String Record
buildRecord body
  | B.length body /= protectedSize =
      Left ("constructed body must be " ++ show protectedSize ++ " bytes")
  | otherwise =
      let provisional = Record (body <> B.replicate 8 0)
          computed = computedParity provisional
          bytes =
            body
              <> littleEndian (fromIntegral (syndromeRow computed))
              <> littleEndian (syndromeColumn computed)
       in mkRecord bytes

littleEndian :: Word32 -> B.ByteString
littleEndian value =
  B.pack
    [ fromIntegral (value `shiftR` shift)
    | shift <- [0, 8, 16, 24]
    ]

groundCases :: Record -> Either String [GroundCase]
groundCases base = do
  constructed <- traverse buildRecord (constructedBodies base)
  let validCases =
        [ GroundCase ("record-valid-" ++ show index) record (oracle record) (ValidRecord index)
        | (index, record) <- zip [1 ..] constructed
        ]
      singleCases =
        [ GroundCase
            ( "record-valid-" ++ show index
                ++ "-single-" ++ show (bitIndex site)
            )
            mutated
            (oracle mutated)
            (SingleBitRecord index site)
        | (index, record) <- zip [1 ..] constructed
        , site <-
            [ BitSite (RowIndex 0) (ColumnIndex 0)
            , BitSite (RowIndex 10) (ColumnIndex 31)
            ]
        , let mutated = flipBit site record
        ]
      rowCases =
        [ GroundCase ("parity-row-" ++ show index) record (oracle record) (RowParityBit index)
        | index <- [0 .. rowParityBits - 1]
        , let record = flipRecordBit (rowParityOffset * 8 + index) base
        ]
      columnCases =
        [ GroundCase ("parity-column-" ++ show index) record (oracle record) (ColumnParityBit index)
        | index <- [0 .. columnCount - 1]
        , let record = flipRecordBit (columnParityOffset * 8 + index) base
        ]
      pairCases =
        [ GroundCase
            ("parity-pair-" ++ show row ++ "-" ++ show column)
            record
            (oracle record)
            (ParityPair row column)
        | row <- [0, rowParityBits - 1]
        , column <- [0, columnCount - 1]
        , let record =
                flipRecordBit (columnParityOffset * 8 + column)
                  (flipRecordBit (rowParityOffset * 8 + row) base)
        ]
  pure (validCases ++ singleCases ++ rowCases ++ columnCases ++ pairCases)
