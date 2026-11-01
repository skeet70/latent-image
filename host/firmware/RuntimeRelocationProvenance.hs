-- | Retained ELF32 REL records produced by the RAM bank linker's --emit-relocs.
module RuntimeRelocationProvenance
  ( AbsoluteRelocation (..)
  , absoluteRelocations
  ) where

import qualified ArmElf as E
import Control.Monad (forM, unless)
import Data.Bits (shiftL, shiftR, (.&.))
import qualified Data.ByteString as B
import Data.Word (Word32)

data AbsoluteRelocation = AbsoluteRelocation
  { relocationSite :: Word32
  , relocationSymbol :: E.Symbol
  , relocationSymbolSection :: Maybe String
  , relocationAddend :: Word32
  } deriving (Eq, Show)

absoluteRelocations :: E.Elf -> Either String [AbsoluteRelocation]
absoluteRelocations elf = do
  shoff <- fileWord 0x20
  let tables = [(index, section) | (index, section) <- zip [0 :: Int ..] (E.elfSections elf), E.sectionType section == 9]
  unless (not (null tables)) (Left "retained ELF REL records required; relink with --emit-relocs")
  relocations <- concat <$> forM tables (readTable shoff)
  unless (not (null relocations)) (Left "retained R_ARM_ABS32 pointer provenance required")
  pure relocations
 where
  bytes = E.elfBytes elf
  fileWord :: Int -> Either String Word32
  fileWord offset = do
    unless (offset >= 0 && offset + 4 <= B.length bytes) (Left "ELF relocation field outside file")
    pure (sum [fromIntegral (B.index bytes (offset + index)) `shiftL` (8 * index) | index <- [0 .. 3]])
  indexed :: String -> [a] -> Word32 -> Either String a
  indexed label values index =
    if toInteger index < toInteger (length values) then pure (values !! fromIntegral index) else Left (label ++ " index outside ELF table")
  readTable shoff (index, section) = do
    let header = fromIntegral shoff + index * 40
    link <- fileWord (header + 24)
    info <- fileWord (header + 28)
    entrySize <- fileWord (header + 36)
    symbols <- indexed "relocation symbol table" (E.elfSections elf) link
    target <- indexed "relocation target section" (E.elfSections elf) info
    unless (E.sectionType symbols == 2 && entrySize == 8 && E.sectionSize section `mod` 8 == 0) (Left "ELF32 REL/symbol table layout changed")
    records <- forM [0 .. fromIntegral (E.sectionSize section) `div` 8 - 1] $ \record -> do
      let offset = fromIntegral (E.sectionOffset section) + record * 8
      site <- fileWord offset
      encoding <- fileWord (offset + 4)
      pure (site, encoding .&. 0xff, encoding `shiftR` 8)
    if not (E.sectionAllocated target) then pure [] else do
      unless (E.sectionType target == 1) (Left "relocation target must be allocated PROGBITS")
      forM [(site, symbolIndex) | (site, kind, symbolIndex) <- records, kind == 2] $ \(site, symbolIndex) -> do
        unless (site .&. 3 == 0 && E.sectionAddress target <= site && toInteger site + 4 <= toInteger (E.sectionAddress target) + toInteger (E.sectionSize target)) (Left "R_ARM_ABS32 site outside its target section or unaligned")
        symbol <- indexed "R_ARM_ABS32 symbol" (E.elfSymbols elf) symbolIndex
        sectionName <- case E.symbolSection symbol of
          E.Undefined -> Left "R_ARM_ABS32 references undefined symbol"
          E.Absolute -> pure Nothing
          E.InSection sectionIndex -> Just . E.sectionName <$> indexed "R_ARM_ABS32 symbol section" (E.elfSections elf) (fromIntegral sectionIndex)
        targetWord <- maybe (Left "R_ARM_ABS32 word outside allocated bytes") pure (E.wordAt elf site)
        pure (AbsoluteRelocation site symbol sectionName (targetWord - E.symbolValue symbol))
