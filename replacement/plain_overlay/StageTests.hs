-- | plain-overlay staging and census over a synthetic plain image and the owned overlay-46 ELFs.
module Main (main) where

import qualified ArmElf as E
import Control.Monad (unless)
import qualified Data.Aeson as A
import Data.Bits (shiftL, shiftR, (.&.))
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as BL
import Data.Word (Word32)
import ReferenceScan (Address (..), Image (..), Reference (..))
import Replacement.PlainOverlay.Stage
import Replacement.PlainOverlay.Unit
import System.Environment (getArgs)

check :: String -> Bool -> IO ()
check name ok = unless ok (fail name)

word :: Word32 -> B.ByteString
word value = B.pack [fromIntegral (value `shiftR` bit) | bit <- [0, 8, 16, 24]]

wordAt :: B.ByteString -> Word32 -> Word32
wordAt bytes address = sum [fromIntegral (B.index bytes (fromIntegral (address - plainBase) + i)) `shiftL` (8 * i) | i <- [0 .. 3]]

-- | Bytes at BASE with WORDS placed at their addresses over a 0x55 fill.
placed :: Word32 -> Int -> [(Word32, Word32)] -> B.ByteString
placed base size wordsAt = foldl put (B.replicate size 0x55) wordsAt
  where
    put bytes (address, value) = let at = fromIntegral (address - base) in B.take at bytes <> word value <> B.drop (at + 4) bytes

bl :: Word32 -> Word32 -> Word32
bl site target = 0xeb000000 + (((target - site - 8) `shiftR` 2) .&. 0xffffff)

loadUnit :: FilePath -> IO Unit
loadUnit path = B.readFile path >>= either fail pure . parseUnit

loadElf :: FilePath -> IO E.Elf
loadElf path = B.readFile path >>= either fail pure . E.parseElf

reparse :: Unit -> Either String Unit
reparse = parseUnit . BL.toStrict . A.encode . unitValue

main :: IO ()
main = do
  args <- getArgs
  (saveUnit, saveElf, exifUnit, exifElf) <- case args of
    [saveJson, saveElfPath, exifJson, exifElfPath] -> (,,,) <$> loadUnit saveJson <*> loadElf saveElfPath <*> loadUnit exifJson <*> loadElf exifElfPath
    _ -> fail "usage: stage-tests SAVE_UNIT SAVE_ELF EXIF_UNIT EXIF_ELF"
  let plain = B.pack (take 0x34a5e0 (cycle [1 .. 251]))
      at bytes a = B.index bytes (fromIntegral (a - plainBase))
      declared a = any (\u -> any (`inside` a) (unitSpans u)) [saveUnit, exifUnit]
      trap a = word trapWord `B.index` fromIntegral (a .&. 3)
  case stagePlain plain [(saveUnit, saveElf), (exifUnit, exifElf)] of
    Left refusal -> fail (show refusal)
    Right (staged, _) -> do
      check "the staged image keeps its length" (B.length staged == B.length plain)
      check "bytes outside the declared extents are stock"
        (and [at staged a == at plain a | a <- [plainBase .. plainBase + fromIntegral (B.length plain) - 1], not (declared a)])
      check "each extent holds its ELF's bytes and the trap word where the ELF defines none"
        (and [at staged a == maybe (trap a) id (E.byteAt elf a) | (u, elf) <- [(saveUnit, saveElf), (exifUnit, exifElf)], Span s e <- unitExtents u, a <- [s .. e - 1]])
  let tables = 0x011d5d44
      narrowed = saveUnit {unitExtents = [Span 0x011d5090 tables]}
      split = saveUnit {unitExtents = [Span 0x011d5090 tables, Span (tables + 4) 0x011d5df8]}
  check "an ELF byte outside the declared extents refuses"
    (refusalOf (stagePlain plain [(narrowed, saveElf)]) == Just (OutsideUnit "save_file_machine" tables))
  case stagePlain plain [(split {unitRows = [Row tables (wordAt plain tables)]}, saveElf)] of
    Left refusal -> fail ("a stock row refused: " ++ show refusal)
    Right (staged, _) -> check "a row whose prior word is stock holds the ELF's word" (Just (wordAt staged tables) == E.wordAt saveElf tables)
  check "a row whose prior word is not its stock value refuses"
    (refusalOf (stagePlain plain [(split {unitRows = [Row tables (wordAt plain tables + 1)]}, saveElf)]) == Just (RowNotStock "save_file_machine" tables (wordAt plain tables)))
  check "a row the ELF does not define refuses"
    (refusalOf (stagePlain plain [(saveUnit {unitRows = [Row 0x011dc000 (wordAt plain 0x011dc000)]}, saveElf)]) == Just (RowNotPublished "save_file_machine" 0x011dc000))
  check "two units over one byte refuse"
    (refusalOf (stagePlain plain [(saveUnit, saveElf), (saveUnit {unitName = "again"}, saveElf)]) == Just (UnitsOverlap "save_file_machine" "again"))
  check "an entry that is not a branch into the unit refuses"
    (refusalOf (stagePlain plain [(saveUnit {unitEntries = Entry "tables" tables 0 False [] : unitEntries saveUnit}, saveElf)]) == Just (EntryNotBranch "save_file_machine" "tables"))
  check "a plain image too short for the overlay refuses"
    (refusalOf (stagePlain (B.take 0xc0000 plain) [(saveUnit, saveElf)]) == Just PlainTooShort)
  check "the manifests round-trip" (reparse saveUnit == Right saveUnit && reparse exifUnit == Right exifUnit)
  check "an extent outside its overlay refuses" (isLeft (reparse saveUnit {unitExtents = [Span 0x011dc000 0x011dd000]}))
  check "overlapping extents refuse" (isLeft (reparse saveUnit {unitExtents = [Span 0x011d5090 0x011d5df8, Span 0x011d5d00 0x011d5e00]}))
  check "an entry outside the extents refuses" (isLeft (reparse saveUnit {unitEntries = [Entry "outside" 0x011d5df8 0 False []]}))
  check "an output through a register the entry does not read refuses"
    (isLeft (reparse saveUnit {unitEntries = [Entry "begin" 0x011d5600 1 False [Output (ArgumentRegister 1) 0 (Bytes 4)]]}))
  check "a length the entry's arguments do not carry refuses"
    (isLeft (reparse saveUnit {unitEntries = [Entry "begin" 0x011d5600 2 False [Output (ArgumentRegister 0) 0 (CountRegister 2)]]}))
  check "a returned length on an entry that returns nothing refuses"
    (isLeft (reparse saveUnit {unitEntries = [Entry "begin" 0x011d5600 3 False [Output (ArgumentRegister 1) 0 (UpToResult 16)]]}))
  check "a stack output on an entry with fewer than four register arguments refuses"
    (isLeft (reparse saveUnit {unitEntries = [Entry "begin" 0x011d5600 3 False [Output (StackArgument 0) 0 (Bytes 12)]]}))
  check "a row inside an extent refuses" (isLeft (reparse saveUnit {unitRows = [Row tables 0]}))
  let base5 = 0xbf1a00
      begin = 0x011d5600
      five = placed base5 0x4b0000
        [ (0xbf2000, bl 0xbf2000 begin)
        , (0xbf2004, bl 0xbf2004 (begin + 8))
        , (0xbf2010, begin)
        , (0xbf2014, begin + 4)
        , (0xbf2018, begin + 1)
        , (0xbf2020, 0xe3050604), (0xbf2024, 0xe340011d)
        , (0xbf2030, 0xfa000000 + (((begin + 8 - 0xbf2030 - 8) `shiftR` 2) .&. 0xffffff))
        , (0x01098488, 0x1a04f515)
        , (0x0109848c, 0x1a04f515)
        ]
      plainImage = placed plainBase 0x34a5e0 [(0x011d5000, bl 0x011d5000 (begin + 8)), (0x011d5200, bl 0x011d5200 (begin + 8))]
      findings = census [saveUnit, exifUnit] [(Image5, Image "image5" (Address base5) five), (Plain, Image "plain" (Address plainBase) plainImage)]
      verdictAt name site = [findingVerdict f | f <- findings, let Reference _ _ image (Address s) = findingReference f, image == name, s == site]
  check "a branch to an entry is admitted" (verdictAt "image5" 0xbf2000 == [EntryBranch])
  check "a branch into an entry's body refuses" (verdictAt "image5" 0xbf2004 == [Refused])
  check "a pointer word to an entry is admitted" (verdictAt "image5" 0xbf2010 == [EntryPointer])
  check "a pointer word into the extent refuses" (verdictAt "image5" 0xbf2014 == [Refused])
  check "a word whose value is not word-aligned is admitted" (verdictAt "image5" 0xbf2018 == [NotWordAddress])
  check "a MOVW/MOVT pair into the extent refuses" (Refused `elem` verdictAt "image5" 0xbf2024)
  check "an A32 BLX into the extent arrives in Thumb state and is admitted" (ThumbStateEntry `elem` verdictAt "image5" 0xbf2030)
  check "a reviewed data word is admitted" (case verdictAt "image5" 0x01098488 of [Reviewed _] -> True; _ -> False)
  check "the reviewed word at another site refuses" (verdictAt "image5" 0x0109848c == [Refused])
  check "a plain-image reference from outside the extents refuses" (verdictAt "plain" 0x011d5000 == [Refused])
  check "a plain-image reference from inside a staged extent is not reported" (null (verdictAt "plain" 0x011d5200))
  where
    refusalOf = either Just (const Nothing)
    isLeft = either (const True) (const False)
