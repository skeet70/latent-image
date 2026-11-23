{-# LANGUAGE OverloadedStrings #-}

-- | Owned units staged into the plain image, and the census of references into their extents.
module Replacement.PlainOverlay.Stage
  ( Refusal (..)
  , StagedUnit (..)
  , stagePlain
  , trapWord
  , Verdict (..)
  , Finding (..)
  , census
  , findingValue
  , verdictKey
  ) where

import Control.Monad (foldM, forM_, unless, when)
import qualified ArmElf as E
import Data.Aeson (Value, object, (.=))
import Data.Bits (shiftL, shiftR, (.&.), (.|.))
import qualified Data.ByteString as B
import Data.List (find, tails)
import Data.Maybe (isJust)
import Data.Word (Word32, Word8)
import ReferenceScan (Address (..), Image (..), Reference (..), ReferenceKind (..), renderReference, scanReferences)
import Replacement.PlainOverlay.Unit

data Refusal
  = PlainTooShort
  | UnitsOverlap String String
  | OutsideUnit String Word32
  | EntryNotBranch String String
  | RowNotPublished String Word32
  | RowNotStock String Word32 Word32
  deriving (Eq, Show)

-- | A staged unit: the bytes its ELF defines inside the extents and the rows it rewrote, with
-- their owned values.
data StagedUnit = StagedUnit {stagedUnit :: Unit, stagedDefined :: Int, stagedRows :: [(Row, Word32)]}

-- | The A32 undefined instruction that fills extent bytes the ELF leaves undefined.
trapWord :: Word32
trapWord = 0xe7f000f0

-- | The plain image with each unit's extents holding its ELF's bytes (the trap word where the ELF
-- defines none) and each row holding the ELF's word. Refused when an allocated ELF byte lies
-- outside the unit's extents and rows, two units share a byte, an entry is not an A32 branch into
-- the unit's extents, a row's word is not defined by the ELF, or a row's prior word is not its
-- stock value.
stagePlain :: B.ByteString -> [(Unit, E.Elf)] -> Either Refusal (B.ByteString, [StagedUnit])
stagePlain plain units = do
  unless (and [fromIntegral (B.length plain) >= spanEnd (overlaySpan (unitOverlay u)) - plainBase | (u, _) <- units]) (Left PlainTooShort)
  forM_ [(a, b) | ((a, _) : rest) <- tails units, (b, _) <- rest] $ \(a, b) ->
    when (or [overlaps x y | x <- unitSpans a, y <- unitSpans b]) (Left (UnitsOverlap (unitName a) (unitName b)))
  foldM stageOne (plain, []) units >>= \(bytes, staged) -> pure (bytes, reverse staged)
  where
    overlaps (Span s e) (Span s' e') = s < e' && s' < e
    stageOne (bytes, done) (unit, elf) = do
      let name = unitName unit
          sections = [s | s <- E.elfSections elf, E.sectionAllocated s, E.sectionSize s > 0]
          covered a = any (`inside` a) (unitSpans unit)
      forM_ sections $ \s ->
        case find (not . covered) [E.sectionAddress s .. E.sectionAddress s + E.sectionSize s - 1] of
          Just a -> Left (OutsideUnit name a)
          Nothing -> pure ()
      forM_ (unitEntries unit) $ \(Entry entry address) ->
        case E.wordAt elf address of
          Just w | w .&. 0xff000000 == 0xea000000, any (`inside` branchTarget address w) (unitExtents unit) -> pure ()
          _ -> Left (EntryNotBranch name entry)
      rows <- mapM (\row -> maybe (Left (RowNotPublished name (rowAddress row))) (\w -> pure (row, w)) (E.wordAt elf (rowAddress row))) (unitRows unit)
      forM_ (unitRows unit) $ \(Row address stock) -> do
        let prior = wordIn bytes address
        unless (prior == stock) (Left (RowNotStock name address prior))
      let defined a = E.byteAt elf a
          extentBytes (Span s e) = (s, B.pack [maybe (trapByte a) id (defined a) | a <- [s .. e - 1]])
          rowBytes (Row a _, w) = (a, B.pack (le w))
          patched = foldl splice bytes (map extentBytes (unitExtents unit) ++ map rowBytes rows)
          count = length [() | Span s e <- unitExtents unit, a <- [s .. e - 1], isJust (defined a)]
      pure (patched, StagedUnit unit count rows : done)
    splice bytes (address, new) =
      let at = fromIntegral (address - plainBase)
       in B.take at bytes <> new <> B.drop (at + B.length new) bytes
    trapByte a = le trapWord !! fromIntegral (a .&. 3)

branchTarget :: Word32 -> Word32 -> Word32
branchTarget site w =
  let imm = w .&. 0xffffff
      displacement = (if imm .&. 0x800000 /= 0 then imm .|. 0xff000000 else imm) `shiftL` 2
   in site + 8 + displacement

le :: Word32 -> [Word8]
le v = [fromIntegral (v `shiftR` s) | s <- [0, 8, 16, 24]]

wordIn :: B.ByteString -> Word32 -> Word32
wordIn bytes address =
  let at = fromIntegral (address - plainBase)
   in sum [fromIntegral (B.index bytes (at + i)) `shiftL` (8 * i) | i <- [0 .. 3]]

data Verdict
  = EntryBranch
  | EntryPointer
  | NotWordAddress
  | ThumbStateEntry
  | HalfwordSite
  | Reviewed String
  | Refused
  deriving (Eq, Show)

-- | A reference into a unit's extents from a site outside every unit's extents, with the word at
-- its site.
data Finding = Finding {findingUnit :: String, findingReference :: Reference, findingWord :: Word32, findingVerdict :: Verdict}

-- | Every reference from the scanned images into the units' extents. An A32 branch or an aligned
-- pointer word to an entry is admitted. The extents hold A32 code and word literals, so a pointer
-- word whose value is not word-aligned names neither, a word at a halfword-aligned site is read by
-- no A32 or Thumb literal load, and a branch that arrives in Thumb state (A32 BLX, Thumb BL and
-- B.W) would execute A32 code as Thumb. A reviewed site holding its reviewed word is admitted;
-- anything else is refused.
census :: [Unit] -> [(Scanned, Image)] -> [Finding]
census units images = concatMap scan images
  where
    targets = map Address (concat [[s .. e - 1] | u <- units, Span s e <- unitExtents u])
    staged a = any (\u -> any (`inside` a) (unitExtents u)) units
    owner a = find (\u -> any (`inside` a) (unitExtents u)) units
    scan (scanned, image) =
      [ Finding (maybe "" unitName (owner target)) reference word (verdict scanned (owner target) reference word)
      | reference@(Reference (Address target) _ _ (Address site)) <- scanReferences targets image
      , not (scanned == Plain && staged site)
      , let word = siteWord image site
      ]
    verdict scanned unit (Reference (Address target) kind _ (Address site)) word
      | kind `elem` [ArmB, ArmBL] && target `elem` entries = EntryBranch
      | kind == PointerWord && word `elem` entries = EntryPointer
      | kind == PointerWord && word .&. 3 /= 0 = NotWordAddress
      | kind `elem` [ArmBLX, ThumbBL, ThumbBW, ThumbConditionalBW] = ThumbStateEntry
      | kind == PointerHalfword = HalfwordSite
      | Just r <- find (\r -> reviewedImage r == scanned && reviewedSite r == site && reviewedWord r == word) reviewed = Reviewed (reviewedReason r)
      | otherwise = Refused
      where
        entries = maybe [] (map entryAddress . unitEntries) unit
        reviewed = maybe [] unitReviewed unit
    siteWord (Image _ (Address base) bytes) site =
      let offset = fromIntegral (site - base)
       in if offset + 4 <= B.length bytes then sum [fromIntegral (B.index bytes (offset + i)) `shiftL` (8 * i) | i <- [0 .. 3]] else 0

verdictKey :: Verdict -> String
verdictKey verdict = case verdict of
  EntryBranch -> "entry-branch"
  EntryPointer -> "entry-pointer"
  NotWordAddress -> "not-word-aligned"
  ThumbStateEntry -> "thumb-state-entry"
  HalfwordSite -> "halfword-aligned-site"
  Reviewed _ -> "reviewed-data"
  Refused -> "refused"

findingValue :: Finding -> Value
findingValue (Finding unit reference@(Reference (Address target) _ name (Address site)) word verdict) =
  object
    ( [ "unit" .= unit, "image" .= name, "site" .= hexWord site, "target" .= hexWord target, "word" .= hexWord word
      , "reference" .= renderReference reference
      , "verdict" .= (case verdict of Reviewed reason -> "reviewed-data: " ++ reason; _ -> verdictKey verdict)
      ]
        ++ ["overlay" .= overlayNumber o | name == scannedName Plain, Just o <- [overlayOf site]]
    )
