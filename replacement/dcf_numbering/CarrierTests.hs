module Main (main) where

import Control.Monad (unless)
import Data.Bits (shiftR, (.&.))
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import Data.Word (Word32)
import Numeric (showHex)
import ReferenceScan (Address (..), Image (..), Reference (..))
import Replacement.DcfNumbering.Carrier
import System.Environment (getArgs)

word :: Word32 -> B.ByteString
word value = B.pack [fromIntegral (value `shiftR` bit) | bit <- [0, 8, 16, 24]]

check :: String -> Bool -> IO ()
check name ok = unless ok (fail name)

refuses :: Either String a -> Bool
refuses = either (const True) (const False)

image4Base :: Word32
image4Base = 0x61c400

branch :: Word32 -> Word32 -> Word32
branch site target = 0xea000000 + (((target - site - 8) `shiftR` 2) .&. 0xffffff)

bl :: Word32 -> Word32 -> Word32
bl site target = 0xeb000000 + (((target - site - 8) `shiftR` 2) .&. 0xffffff)

-- | Bytes at BASE with WORDS placed at their addresses over a 0x55 fill.
placed :: Word32 -> Int -> [(Word32, Word32)] -> B.ByteString
placed base size wordsAt = foldl put (B.replicate size 0x55) wordsAt
  where
    put bytes (address, value) = let at = fromIntegral (address - base) in B.take at bytes <> word value <> B.drop (at + 4) bytes

verdicts :: String -> Word32 -> B.ByteString -> [(Word32, Verdict)]
verdicts name base bytes = [(site, findingVerdict f) | f <- census [Image name (Address base) bytes], let Reference _ _ _ (Address site) = findingReference f]

main :: IO ()
main = do
  let entryAt name = maybe (error name) id (lookup name [(n, a) | Entry a n <- entries])
      symbolText table = C.pack (concat [showHex a "" ++ " " ++ showHex s "" ++ " T " ++ n ++ "\n" | (n, a, s) <- table])
      rows = concat [[("dcf_entry_" ++ n, a, 4 :: Word32), ("xm1_dcf_" ++ n, a + 4, 4)] | Entry a n <- entries]
      lastEnd = extentEnd (last extents)
      linked = placed linkedBase (fromIntegral (lastEnd - linkedBase) - 12) [(a, branch a (a + 4)) | Entry a _ <- entries]
      source = B.pack (take (0x89ec00 - 0x61c400) (cycle [1 .. 251]))
      at address bytes = B.index bytes (fromIntegral (address - image4Base))
  case stageImage source linked (symbolText rows) of
    Left problem -> fail problem
    Right staged -> do
      check "staged image keeps its length" (B.length staged == B.length source)
      check "bytes outside the extents are unchanged"
        (and [at a staged == at a source | a <- [0x70f700 .. 0x716600], not (inExtents a)])
      check "each extent holds the linked bytes"
        (and [at a staged == B.index linked (fromIntegral (a - linkedBase)) | Extent s e <- extents, a <- [s .. min e (lastEnd - 12) - 1]])
      check "the extent tail past the linked image is zero" (and [at a staged == 0 | a <- [lastEnd - 12 .. lastEnd - 1]])
  check "a moved entry symbol refuses"
    (refuses (stageImage source linked (symbolText [(n, if n == "dcf_entry_check_name" then a + 4 else a, s) | (n, a, s) <- rows])))
  check "a missing owned function refuses"
    (refuses (stageImage source linked (symbolText [row | row@(n, _, _) <- rows, n /= "xm1_dcf_last_used"])))
  check "a symbol outside the extents refuses"
    (refuses (stageImage source linked (symbolText (("stray", 0x7101e0, 8) : rows))))
  check "an entry that does not branch to its owned function refuses"
    (refuses (stageImage source (placed linkedBase (B.length linked) [(a, if n == "note_written" then branch a (a + 8) else branch a (a + 4)) | Entry a n <- entries]) (symbolText rows)))
  check "a linked image past the last extent refuses"
    (refuses (stageImage source (linked <> B.replicate 16 0) (symbolText rows)))
  let base5 = 0xbf1a00
      five = placed base5 0x480000
        [ (0xbf2000, bl 0xbf2000 (entryAt "prepare_name"))
        , (0xbf2004, bl 0xbf2004 (entryAt "prepare_name" + 8))
        , (0xbf2010, entryAt "last_used")
        , (0xbf2014, 0x716330)
        , (0xbf2020, 0xe3060330), (0xbf2024, 0xe3400071)
        , (0xbf2030, 0x716332)
        , (0xf580bc, 0x00710070)
        , (0xf580c0, 0x00710074)
        ]
      found = verdicts "image5" base5 five
  check "a branch to an entry is admitted" (lookup 0xbf2000 found == Just EntryBranch)
  check "a branch into an entry's body refuses" (lookup 0xbf2004 found == Just Refused)
  check "a pointer word to an entry is admitted" (lookup 0xbf2010 found == Just EntryPointer)
  check "a pointer word into an extent refuses" (lookup 0xbf2014 found == Just Refused)
  check "a MOVW/MOVT pair into an extent refuses" (lookup 0xbf2024 found == Just Refused)
  check "a word whose value is not word-aligned is admitted" (lookup 0xbf2030 found == Just NotWordAddress)
  check "a reviewed data word is admitted" (case lookup 0xf580bc found of Just (Reviewed _) -> True; _ -> False)
  check "an unreviewed word beside a reviewed one refuses" (lookup 0xf580c0 found == Just Refused)
  let four = placed image4Base 0x282800 [(0x70fc00, bl 0x70fc00 0x716330), (0x720000, bl 0x720000 0x716330)]
      found4 = verdicts "image4" image4Base four
  check "a reference from inside the extents is not reported" (lookup 0x70fc00 found4 == Nothing)
  check "a reference from image 4 outside the extents refuses" (lookup 0x720000 found4 == Just Refused)
  args <- getArgs
  let pairs (binPath : symbolsPath : rest) = do
        bin <- B.readFile binPath
        symbols <- B.readFile symbolsPath
        either (fail . ((binPath ++ ": ") ++)) (const (pure ())) (stageImage source bin symbols)
        pairs rest
      pairs [] = pure ()
      pairs _ = fail "expected linked bin/symbol pairs"
  pairs args
