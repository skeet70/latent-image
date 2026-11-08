-- | Deterministic valid +0x16c cases. The current-task word is a task-record start of the domain's
-- 120-record table, as @rtos_layout@'s @DomainWords::task_slot@ admits; arbitrary words are not part
-- of the leaf's input contract.
module Replacement.RtosOwnFirst.Generate
  ( GenerationSeed (..)
  , GenerationCount
  , mkGenerationCount
  , generationCountValue
  , generationSlot
  , parseGenerationSeed
  , parseGenerationCount
  , generatedLabel
  , reducedLabel
  , seedText
  , generateRows
  , validRow
  , taskSlotOf
  ) where

import Data.Bits (complement, shiftR, testBit, xor, (.&.))
import Data.Char (isHexDigit)
import Data.Word (Word32, Word64)
import Numeric (readDec, readHex, showHex)
import Replacement.RtosOwnFirst.Oracle
import Replacement.RtosOwnFirst.Regions (Domain, domainLabel)

-- | The settled own-first leaf this corpus admits.
generationSlot :: Slot
generationSlot = S16c

taskSlots :: Int
taskSlots = 120

maxGeneratedRows :: Int
maxGeneratedRows = 256

newtype GenerationSeed = GenerationSeed Word64
  deriving (Eq, Show)

newtype GenerationCount = GenerationCount Int
  deriving (Eq, Show)

mkGenerationCount :: Int -> Either String GenerationCount
mkGenerationCount count
  | count >= 1 && count <= maxGeneratedRows = Right (GenerationCount count)
  | otherwise = Left ("count must be 1.." ++ show maxGeneratedRows ++ ": " ++ show count)

generationCountValue :: GenerationCount -> Int
generationCountValue (GenerationCount count) = count

parseGenerationSeed :: String -> Either String GenerationSeed
parseGenerationSeed text = case text of
  '0' : 'x' : digits@(_ : _)
    | length digits <= 16
    , all isHexDigit digits
    , [(value, "")] <- readHex digits ->
        Right (GenerationSeed value)
  _ ->
    case readDec text of
      [(value, "")] | value <= toInteger (maxBound :: Word64) -> Right (GenerationSeed (fromInteger value))
      _ -> Left ("seed must be a decimal or 0x-prefixed 64-bit word: " ++ text)

parseGenerationCount :: String -> Either String GenerationCount
parseGenerationCount text = case readDec text of
  [(value, "")] | value <= toInteger (maxBound :: Int) -> mkGenerationCount (fromInteger value)
  _ -> Left ("count must be 1.." ++ show maxGeneratedRows ++ ": " ++ text)

-- | A case id of the generated corpus, unique within a batch directory.
generatedLabel :: GenerationSeed -> Domain -> CaseKey -> String
generatedLabel seed domain (CaseKey index) =
  "gen-" ++ seedText seed ++ "-" ++ domainLabel domain ++ "-" ++ slotTag generationSlot ++ "-" ++ show index

-- | A case id of a reduced case, distinct from the generated id it came from.
reducedLabel :: String -> GenerationSeed -> Domain -> CaseKey -> String
reducedLabel role seed domain (CaseKey index) =
  "reduced-" ++ role ++ "-" ++ seedText seed ++ "-" ++ domainLabel domain ++ "-" ++ slotTag generationSlot ++ "-" ++ show index

seedText :: GenerationSeed -> String
seedText (GenerationSeed seed) = showHex seed ""

-- | The corpus: one extremal valid row (last task record, both interrupt masks, all flag patterns),
-- then seeded rows. Every row is a valid +0x16c input.
generateRows :: GenerationSeed -> GenerationCount -> [Row]
generateRows seed (GenerationCount count) = take count (extremal : drawn)
  where
    extremal =
      Row
        (EntryState System False True flagMask)
        0
        0
        (TaskAt (recordBytes * (taskSlots - 1)))
        0
        0
        0
        bufferPointer
        Nothing
        OrdinaryStack
        SingleCall
    drawn = map draw (chunksOf 8 (stream (seedWord seed)))
    draw [w0, w1, w2, w3, w4, w5, w6, w7] =
      Row
        (EntryState (pick [Supervisor, System] w0) (testBit w6 3) (testBit w1 0) (pick [0, flagMask, 0x60050000, w7 .&. flagMask] w2))
        (pick [0, 0, 0, 0, 1, 2, 0x80000000, 0xffffffff] w3)
        0
        (TaskAt (recordBytes * fromIntegral (pick [0, taskSlots - 1, 59, fromIntegral (w5 `mod` fromIntegral taskSlots)] w4)))
        0
        0
        0
        bufferPointer
        Nothing
        OrdinaryStack
        SingleCall
    draw _ = error "unreachable chunk"

seedWord :: GenerationSeed -> Word32
seedWord (GenerationSeed seed) = fromIntegral (seed `xor` (seed `shiftR` 32))

chunksOf :: Int -> [a] -> [[a]]
chunksOf size items = case splitAt size items of
  (chunk, rest) | length chunk == size -> chunk : chunksOf size rest
  _ -> []

pick :: [a] -> Word32 -> a
pick values word = values !! fromIntegral (word `mod` fromIntegral (length values))

-- | The +0x16c input contract: a task-record start of the 120-record table, an admitted entry mode,
-- the fixed buffer pointer and a single call. The leaf never dereferences the task word.
validRow :: Row -> Bool
validRow row =
  entryMode entry `elem` [Supervisor, System]
    && entryFlags entry .&. complement flagMask == 0
    && rowR0 row == bufferPointer
    && rowIdentity row == Nothing
    && rowCalls row == SingleCall
    && case rowTask row of
      TaskAt offset -> offset >= 0 && offset `mod` recordBytes == 0 && offset `div` recordBytes < taskSlots
      _ -> False
  where
    entry = rowEntry row

-- | The task record index of a valid row, 0-based.
taskSlotOf :: Row -> Int
taskSlotOf row = case rowTask row of
  TaskAt offset -> offset `div` recordBytes
  _ -> error "row does not name a task record"
