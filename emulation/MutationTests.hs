{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}

-- | Synthetic tests for the wrong-variant generator. The ELF here is built in
-- the test: one text section whose function extents hold a conditional
-- branch, every mutable store form, two literal pools marked by @$d@, an
-- unreachable store, a store reached through fallthrough that a PC-relative
-- load targets, and a @$d@ word reached by fallthrough. No suite, image or
-- native run is involved.
module Main (main) where

import qualified ArmElf as E
import Control.Monad (forM_, unless)
import qualified Data.ByteString as B
import Data.Bits (shiftR, xor, (.&.))
import Data.List (nub, sort)
import Data.Word (Word16, Word32)
import Emulation.Judge (ReportControl (..))
import Emulation.Mutation
import Emulation.ScenarioGeneration (ScenarioSeed (..))
import System.Exit (exitFailure)

main :: IO ()
main = do
  let bytes = syntheticElf 0x1000 codeWords symbols
  elf <- either fail pure (E.parseElf bytes)
  let extents =
        [ Extent "handlers" 0x1000 0x30
        , Extent "literal" 0x1030 0x10
        , Extent "marker" 0x1040 0x0c
        ]
  sites <- either fail pure (eligibleSites elf extents)
  check "the eligible sites are exactly the classified forms"
    (map siteAddress sites == [0x1004, 0x1008, 0x100c, 0x1010, 0x1020, 0x1024, 0x1034])
  check "each site keeps the kind its encoding has"
    (map siteKind sites == [BranchInverted, StoreDropped, StoreDropped, StoreDropped, StoreDropped, StoreDropped, StoreDropped])
  check "each site records its own word"
    (map siteWord sites == [0x1a000001, 0xe5810000, 0x15810000, 0xe1c100b4, 0xe92d4010, 0xe8a00002, 0xe5801000])

  -- Data is never a site, whatever it decodes to, under every seed.
  let located = [(extentOf extents (siteAddress site), site) | site <- sites]
      dataWords = [0x1018, 0x101c, 0x102c, 0x1038, 0x1044]
  forM_ [0 .. 15] $ \seed -> do
    variants <- either (fail . renderMutationError) pure (planMutations (ScenarioSeed seed) (length sites) located)
    check ("no data word is a variant site under seed " ++ show seed)
      (null [() | variant <- variants, variantAddress variant `elem` dataWords])
    check ("seed " ++ show seed ++ " draws every site once")
      (sort (map variantAddress variants) == map siteAddress sites)

  -- The same seed regenerates the same variants.
  first <- either (fail . renderMutationError) pure (planMutations (ScenarioSeed 20261111) 3 located)
  second <- either (fail . renderMutationError) pure (planMutations (ScenarioSeed 20261111) 3 located)
  check "the same seed gives the same variants" (first == second)
  check "the same seed gives distinct sites" (length (nub (map variantAddress first)) == 3)
  let firstAddresses =
        [ case planMutations (ScenarioSeed seed) 1 located of
            Right (variant : _) -> variantAddress variant
            _ -> 0
        | seed <- [0 .. 7]
        ]
  check "different seeds draw different first sites" (length (nub firstAddresses) > 1)

  -- Each variant changes exactly one word, and the word is the intended one.
  forM_ first $ \variant -> do
    patched <- either fail pure (patchElf elf variant)
    check "the variant keeps the ELF's length" (B.length patched == B.length bytes)
    let differing = [index | index <- [0 .. B.length bytes - 1], B.index patched index /= B.index bytes index]
        offset = 52 + fromIntegral (variantAddress variant - 0x1000)
    check ("variant " ++ show (variantIndex variant) ++ " changes one word")
      (not (null differing) && all (\index -> index >= offset && index < offset + 4) differing)
    patchedElf <- either fail pure (E.parseElf patched)
    check ("variant " ++ show (variantIndex variant) ++ " carries its mutated word")
      (E.wordAt patchedElf (variantAddress variant) == Just (variantMutated variant))

  branch <- require "an inverted branch" (\variant -> variantKind variant == BranchInverted) first
  check "an inverted branch flips only the condition bit"
    (variantMutated branch == variantOriginal branch `xor` 0x10000000)
  check "an inverted branch keeps the branch form"
    ((variantMutated branch .&. 0x0ffffff0) == (variantOriginal branch .&. 0x0ffffff0))
  store <- require "a dropped store" (\variant -> variantKind variant == StoreDropped) first
  check "a dropped store is a no-op under the store's condition"
    (variantMutated store .&. 0xf0000000 == variantOriginal store .&. 0xf0000000)
  check "a dropped store moves a register to itself"
    ((variantMutated store .&. 0x0fffffff) == 0x01a00000)
  conditional <- require "the conditional store" (\variant -> variantOriginal variant == 0x15810000) first
  check "a conditional store keeps its condition in the no-op"
    (variantMutated conditional == 0x11a00000)

  -- The refusal paths.
  thumbElf <- either fail pure (E.parseElf (syntheticElf 0x1000 codeWords (symbols ++ [SynthSymbol "$t" 0x1030 0])))
  case eligibleSites thumbElf extents of
    Left message -> check "a Thumb mapping symbol refuses the selection" (contains "Thumb" message)
    Right found -> fail ("a Thumb extent was mutated: " ++ show (map siteAddress found))
  oddEntry <- either fail pure (E.parseElf (syntheticElf 0x1001 codeWords symbols))
  case eligibleSites oddEntry extents of
    Left _ -> pure ()
    Right _ -> fail "a Thumb entry did not refuse the selection"

  -- The count bounds.
  case planMutations (ScenarioSeed 1) (length sites + 1) located of
    Left (TooFewSites requested available) ->
      check "more variants than sites is refused" (requested == length sites + 1 && available == length sites)
    Left other -> fail ("the count bound gave the wrong refusal: " ++ show other)
    Right _ -> fail "more variants than sites was accepted"
  case planMutations (ScenarioSeed 1) 0 located of
    Left (InvalidVariantCount 0 limit) -> check "a zero count is refused" (limit == mutationCountLimit)
    Left other -> fail ("the zero count gave the wrong refusal: " ++ show other)
    Right _ -> fail "a zero count was accepted"

  -- The judge's control shape carries the address and the kind.
  let sample = Variant (ScenarioSeed 7) 3 "handlers" 0x1004 0x1a000001 0x0a000001 BranchInverted
      row = variantControl (VariantOutcome sample True ["handlers"] ["effect ring"])
      survivor = variantControl (VariantOutcome (sample {variantKind = StoreDropped}) False [] [])
  check "the control row is named by index, kind and address"
    (reportControlName row == "mutant-3-branch-inverted-0x00001004")
  check "the control row carries the detection" (reportControlDetected row)
  check "the control row carries the differences" (reportControlDifferences row == ["effect ring"])
  check "an undetected variant is a control row with detected false" (not (reportControlDetected survivor))
  check "the survivor name carries its kind" (contains "store-dropped" (reportControlName survivor))

  putStrLn "mutation synthetic tests passed"

-- The synthetic ELF -------------------------------------------------------

data SynthSymbol = SynthSymbol
  { symbolText :: String
  , symbolAddress :: Word32
  , symbolExtent :: Word32
  }

symbols :: [SynthSymbol]
symbols =
  [ SynthSymbol "$a" 0x1000 0
  , SynthSymbol "handlers" 0x1000 0x30
  , SynthSymbol "$d" 0x1018 0
  , SynthSymbol "$a" 0x1020 0
  , SynthSymbol "literal" 0x1030 0x10
  , SynthSymbol "marker" 0x1040 0x0c
  , SynthSymbol "$d" 0x1044 0
  , SynthSymbol "$a" 0x1048 0
  ]

-- 0x1000: cmp r0, #0; bne 0x1010; str r0, [r1]; strne r0, [r1];
-- 0x1010: strh r0, [r1, #4]; b 0x1020;
-- 0x1018: two literal-pool words that decode as a store and a branch;
-- 0x1020: push {r4, lr}; stm r0!, {r1}; bx lr; strb r2, [r3, #1] (unreached);
-- 0x1030: ldr r1, [pc, #0]; str r1, [r0]; push {r4, lr} (the load's target); bx lr;
-- 0x1040: cmp r0, #0; a $d word that decodes as a store; bx lr.
codeWords :: [Word32]
codeWords =
  [ 0xe3500000, 0x1a000001, 0xe5810000, 0x15810000
  , 0xe1c100b4, 0xea000001
  , 0xe5810000, 0x1a000002
  , 0xe92d4010, 0xe8a00002, 0xe12fff1e, 0xe5c32001
  , 0xe59f1000, 0xe5801000, 0xe92d4010, 0xe12fff1e
  , 0xe3500000, 0xe5810000, 0xe12fff1e
  ]

-- | A minimal little-endian ELF32 executable: a .text section at 0x1000, one
-- symbol table and its two string tables.
syntheticElf :: Word32 -> [Word32] -> [SynthSymbol] -> B.ByteString
syntheticElf entry instructionWords rows = B.concat
  [ header, text, symbolBytes, stringBytes, shstr, shstrPadding, sectionHeaders ]
  where
    textOffset = 52 :: Word32
    text = B.concat (map word32 instructionWords)
    textSize = fromIntegral (B.length text)
    symbolOffset = textOffset + textSize
    symbolCount = 1 + length rows
    symbolSize = fromIntegral (16 * symbolCount)
    nameOffsets = scanl (\offset row -> offset + fromIntegral (B.length (ascii (symbolText row))) + 1) (1 :: Word32) rows
    stringBytes = B.singleton 0 <> B.concat [ascii (symbolText row) <> B.singleton 0 | row <- rows]
    symbolBytes = B.replicate 16 0 <> B.concat
      [ symbolEntry offset row | (offset, row) <- zip nameOffsets rows ]
    stringOffset = symbolOffset + symbolSize
    stringSize = fromIntegral (B.length stringBytes)
    shstr = B.singleton 0 <> ascii ".text\0.symtab\0.strtab\0.shstrtab\0"
    shstrOffset = stringOffset + stringSize
    shstrSize = fromIntegral (B.length shstr)
    shstrAligned = shstrOffset + (4 - shstrOffset `mod` 4) `mod` 4
    shstrPadding = B.replicate (fromIntegral (shstrAligned - shstrOffset)) 0
    sectionOffset = shstrAligned + shstrSize
    header = B.concat
      [ B.pack [0x7f, 0x45, 0x4c, 0x46, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0]
      , half 2, half 40, word32 1, word32 entry, word32 0, word32 sectionOffset
      , word32 0, half 52, half 32, half 0, half 40, half 5, half 4
      ]
    sectionHeaders = B.concat
      [ B.replicate 40 0
      , section 1 1 6 0x1000 textOffset textSize 0 0 4 0
      , section 7 2 0 0 symbolOffset symbolSize 3 1 4 16
      , section 15 3 0 0 stringOffset stringSize 0 0 1 0
      , section 23 3 0 0 shstrOffset shstrSize 0 0 1 0
      ]
    section name kind flags address offset size link info align entsize = B.concat
      [ word32 name, word32 kind, word32 flags, word32 address, word32 offset, word32 size
      , word32 link, word32 info, word32 align, word32 entsize
      ]
    symbolEntry nameOffset row = B.concat
      [ word32 nameOffset
      , word32 (symbolAddress row)
      , word32 (symbolExtent row)
      , B.pack [if take 1 (symbolText row) == "$" then 0 else 0x12, 0]
      , half 1
      ]

word32 :: Word32 -> B.ByteString
word32 value = B.pack [fromIntegral (value `shiftR` (8 * index)) | index <- [0 :: Int .. 3]]

half :: Word16 -> B.ByteString
half value = B.pack [fromIntegral value, fromIntegral (value `shiftR` 8)]

ascii :: String -> B.ByteString
ascii = B.pack . map (fromIntegral . fromEnum)

extentOf :: [Extent] -> Word32 -> Extent
extentOf extents address =
  case [extent | extent <- extents, extentStart extent <= address, address < extentStart extent + extentSize extent] of
    (extent : _) -> extent
    [] -> error ("no extent holds " ++ show address)

contains :: String -> String -> Bool
contains needle haystack = any (prefix needle) (suffixes haystack)
  where
    prefix candidate text = take (length candidate) text == candidate
    suffixes [] = [[]]
    suffixes text@(_ : rest) = text : suffixes rest

check :: String -> Bool -> IO ()
check label yes = unless yes (putStrLn ("FAIL: " ++ label) >> exitFailure)

require :: String -> (Variant -> Bool) -> [Variant] -> IO Variant
require label predicate variants = case filter predicate variants of
  (variant : _) -> pure variant
  [] -> fail ("the drawn variants include no " ++ label)
