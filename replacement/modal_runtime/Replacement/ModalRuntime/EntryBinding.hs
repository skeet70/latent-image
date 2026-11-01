{-# LANGUAGE GHC2021 #-}
module Replacement.ModalRuntime.EntryBinding
  ( EntryOptions (..)
  , EntryBinding (..)
  , EntryAddress (..)
  , EntryExtent (..)
  , parseEntryOptions
  , bindEntry
  , entryUsage
  ) where

import qualified A32 as A
import qualified ArmElf as E
import Control.Monad (foldM, unless)
import Data.Bits ((.&.))
import qualified Data.ByteString as B
import qualified Data.Map.Strict as M
import Data.Word (Word32)
import Emulation.Sha256 (Sha256, parseSha256Hex)
import Replacement.ModalRuntime.KernelBinding (bindElfText)

newtype EntryAddress = EntryAddress { entryAddressWord :: Word32 } deriving (Eq, Show)
newtype EntryExtent = EntryExtent { entryExtentBytes :: Word32 } deriving (Eq, Show)

data EntryBinding = EntryBinding
  { entryTextAddress :: EntryAddress
  , entryText :: B.ByteString
  , entrySeamAddress :: EntryAddress
  , entrySeamBytes :: B.ByteString
  , entryOwnedAddress :: EntryAddress
  , entryOwnedSize :: EntryExtent
  }

data EntryOptions = EntryOptions
  { entryRunnerPath :: FilePath
  , entryId5Path :: FilePath
  , entryKernelPath :: FilePath
  , entryKernelDigest :: Sha256
  , entryOutputPath :: FilePath
  }

data Flag = Runner | Id5 | Kernel | KernelDigest | Output deriving (Eq, Ord)

entryUsage :: String
entryUsage = "usage: qualify-entry --runner QUALIFIED_RUNNER --id5 ID5 --kernel-elf KERNEL_ELF --kernel-elf-sha256 SHA256 --out NEW_OUTPUT_DIRECTORY"

parseEntryOptions :: [String] -> Either String EntryOptions
parseEntryOptions arguments = do
  options <- foldM add M.empty =<< pairs arguments
  digest <- required KernelDigest options >>= parseSha256Hex
  EntryOptions <$> required Runner options <*> required Id5 options <*> required Kernel options <*> pure digest <*> required Output options
 where
  pairs [] = Right []
  pairs (name : value : rest) = do
    flag <- case name of
      "--runner" -> Right Runner
      "--id5" -> Right Id5
      "--kernel-elf" -> Right Kernel
      "--kernel-elf-sha256" -> Right KernelDigest
      "--out" -> Right Output
      _ -> Left entryUsage
    unless (not (null value) && take 2 value /= "--") (Left entryUsage)
    ((flag, value) :) <$> pairs rest
  pairs _ = Left entryUsage
  add options (flag, value) = do
    unless (M.notMember flag options) (Left "duplicate entry option")
    pure (M.insert flag value options)
  required flag = maybe (Left entryUsage) Right . M.lookup flag

bindEntry :: Sha256 -> B.ByteString -> Either String EntryBinding
bindEntry expected bytes = do
  (elf, textSection, text) <- bindElfText expected bytes
  let textStart = E.sectionAddress textSection
      textSize = E.sectionSize textSection
      textEnd = toInteger textStart + toInteger textSize
  unless (E.sectionAllocated textSection && not (E.sectionWritable textSection)
      && textStart .&. 3 == 0 && textSize .&. 3 == 0 && textEnd <= 0x100000000) $
    Left "modal .text must be allocated, read-only and A32-aligned"
  segment <- case [part | part <- E.elfSegments elf, E.loadSegment part,
      toInteger (E.segmentAddress part) < textEnd,
      toInteger textStart < toInteger (E.segmentAddress part) + toInteger (E.segmentMemorySize part)] of
    [part] -> Right part
    _ -> Left "modal .text requires one load segment"
  unless (E.segmentExecutable segment && not (E.segmentWritable segment)
      && E.segmentAddress segment <= textStart
      && textEnd <= toInteger (E.segmentAddress segment) + toInteger (E.segmentFileSize segment)
      && textEnd <= toInteger (E.segmentAddress segment) + toInteger (E.segmentMemorySize segment)
      && toInteger (E.sectionOffset textSection) - toInteger (E.segmentOffset segment)
        == toInteger textStart - toInteger (E.segmentAddress segment)) $
    Left "modal .text must be file-backed by a read-only executable segment"
  seam <- function elf textSection "rb_modal_entry"
  owned <- function elf textSection "rb_modal_dispatch"
  unless (E.symbolSize seam == 56) (Left "modal entry must be exactly 56 bytes")
  unless (toInteger (E.symbolValue seam) + 56 <= toInteger (E.symbolValue owned)
      || toInteger (E.symbolValue owned) + toInteger (E.symbolSize owned) <= toInteger (E.symbolValue seam)) $
    Left "modal entry and dispatcher extents overlap"
  resume <- uniqueSymbol elf "__rb_modal_stock_resume"
  unless (E.symbolValue resume == 0x00bfb02c && E.symbolSection resume == E.Absolute) $
    Left "modal stock resume must be absolute stock+4"
  let seamStart = E.symbolValue seam
      mechanism = B.take 56 (B.drop (fromIntegral (seamStart - textStart)) text)
  branch elf (seamStart + 36) 0x00bfb02c
  branch elf (seamStart + 52) (E.symbolValue owned)
  pure (EntryBinding (EntryAddress textStart) text (EntryAddress seamStart) mechanism
    (EntryAddress (E.symbolValue owned)) (EntryExtent (E.symbolSize owned)))
 where
  uniqueSymbol elf name = case [item | item <- E.elfSymbols elf, E.symbolName item == name] of
    [item] -> Right item
    _ -> Left ("unique ELF symbol required: " ++ name)
  function elf textSection name = do
    item <- uniqueSymbol elf name
    index <- case [index | (index, part) <- zip [0 ..] (E.elfSections elf), part == textSection] of
      [index] -> Right index
      _ -> Left "unique modal text section required"
    unless (E.symbolKind item == E.FunctionSymbol && E.symbolSection item == E.InSection index
        && E.symbolValue item .&. 3 == 0 && E.symbolSize item > 0 && E.symbolSize item .&. 3 == 0
        && E.symbolValue item >= E.sectionAddress textSection
        && toInteger (E.symbolValue item) + toInteger (E.symbolSize item)
          <= toInteger (E.sectionAddress textSection) + toInteger (E.sectionSize textSection)) $
      Left ("modal function must have an aligned extent inside .text: " ++ name)
    pure item
  branch elf site target = case A.decode site <$> E.wordAt elf site of
    Just (A.Branch (A.Cond 14) False actual) | actual == target -> Right ()
    _ -> Left "modal exit must be unconditional A32 B to its exact bound target"
