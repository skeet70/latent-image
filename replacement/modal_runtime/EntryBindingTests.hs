{-# LANGUAGE GHC2021 #-}
module Main (main) where

import Control.Monad (forM_, unless)
import Data.Bits (shiftR, (.&.), (.|.))
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import Data.Word (Word32)
import Emulation.Sha256 (sha256, sha256Hex)
import Replacement.ModalRuntime.EntryBinding

main :: IO ()
main = do
  forM_ [0x00a88000, 0x00980000] $ \base -> do
    let bytes = image base Intact
    binding <- either error pure (bindEntry (sha256 bytes) bytes)
    unless (entryTextAddress binding == EntryAddress base
        && entrySeamAddress binding == EntryAddress (base + 32)
        && entryOwnedAddress binding == EntryAddress (base + 160)
        && entryOwnedSize binding == EntryExtent 16
        && entryText binding == B.take 256 (B.drop 256 bytes)
        && entrySeamBytes binding == B.take 56 (B.drop 288 bytes)) $
      error "binding must expose the supplied ELF's text and actual function locations"
  forM_ damages $ \damage -> do
    let bytes = image 0x00a88000 damage
    unless (refused (bindEntry (sha256 bytes) bytes)) (error ("accepted invalid source binding: " ++ show damage))
  let bytes = image 0x00a88000 Intact
      digest = sha256 bytes
      arguments = ["--runner", "runner", "--id5", "id5", "--kernel-elf", "kernel.elf", "--kernel-elf-sha256", sha256Hex digest, "--out", "out"]
  unless (refused (bindEntry (sha256 B.empty) bytes)) (error "wrong ELF digest accepted")
  options <- either error pure (parseEntryOptions arguments)
  unless (entryRunnerPath options == "runner" && entryId5Path options == "id5"
      && entryKernelPath options == "kernel.elf" && entryKernelDigest options == digest
      && entryOutputPath options == "out") (error "required CLI arguments were not preserved")
  unless (accepted (reversePairs arguments)) (error "CLI flag order must not affect acceptance")
  forM_ [[], take 6 arguments ++ drop 8 arguments, arguments ++ ["--out", "again"],
      arguments ++ ["--entry-elf", "scratch.elf"], take 7 arguments ++ ["invalid"] ++ drop 8 arguments,
      ["runner", "id5", "kernel.elf", "scratch.elf", "out"], arguments ++ ["--out"]] $ \invalid ->
    unless (refused (parseEntryOptions invalid)) (error "invalid or incomplete CLI accepted")
 where
  accepted = not . refused . parseEntryOptions
  reversePairs [] = []
  reversePairs (flag : value : rest) = reversePairs rest ++ [flag, value]
  reversePairs _ = error "paired flags required"

refused :: Either a b -> Bool
refused (Left _) = True
refused (Right _) = False

data Damage = Intact | MissingEntry | DuplicateEntry | WrongEntryKind | WrongEntrySection
  | WrongEntrySize | MisalignedEntry | MisalignedOwned | EmptyOwned | OwnedOutsideText
  | OverlappingFunctions | WritableText | UnallocatedText | WritableSegment | NonExecutableSegment
  | TruncatedSegment | MisboundSegment | WrongResume | UndefinedResume | WrongRetainedTarget
  | RetainedCall | ConditionalRetained | WrongOwnedTarget | DuplicateText
  deriving (Eq, Show)

damages :: [Damage]
damages = [MissingEntry, DuplicateEntry, WrongEntryKind, WrongEntrySection, WrongEntrySize,
  MisalignedEntry, MisalignedOwned, EmptyOwned, OwnedOutsideText, OverlappingFunctions,
  WritableText, UnallocatedText, WritableSegment, NonExecutableSegment, TruncatedSegment,
  MisboundSegment, WrongResume, UndefinedResume, WrongRetainedTarget, RetainedCall,
  ConditionalRetained, WrongOwnedTarget, DuplicateText]

-- Synthetic ELF32 with an A32 seam and dispatcher in one file-backed text segment.
image :: Word32 -> Damage -> B.ByteString
image base damage = B.concat
  [ header, segment, B.replicate (256 - 84) 0, text, names, strings, symbols, sections ]
 where
  entry = base + if damage == MisalignedEntry then 34 else 32
  owned = base + case damage of
    MisalignedOwned -> 162
    OwnedOutsideText -> 252
    OverlappingFunctions -> 64
    _ -> 160
  text = patch 84 ownedBranch (patch 68 retainedBranch (B.replicate 256 0))
  retainedBranch = case damage of
    WrongRetainedTarget -> branch (entry + 36) 0x00bfb030
    RetainedCall -> branch (entry + 36) 0x00bfb02c .|. 0x01000000
    ConditionalRetained -> branch (entry + 36) 0x00bfb02c .&. 0x0fffffff
    _ -> branch (entry + 36) 0x00bfb02c
  ownedBranch = branch (entry + 52) (owned + if damage == WrongOwnedTarget then 4 else 0)
  names = C.pack "\0.text\0.shstrtab\0.strtab\0.symtab\0"
  strings = C.pack "\0rb_modal_entry\0rb_modal_dispatch\0__rb_modal_stock_resume\0"
  entrySymbol = symbol 1 entry (if damage == WrongEntrySize then 52 else 56)
    (if damage == WrongEntryKind then 1 else 2) (if damage == WrongEntrySection then 0xfff1 else 1)
  symbols = B.concat $ [B.replicate 16 0]
    ++ [entrySymbol | damage /= MissingEntry]
    ++ [entrySymbol | damage == DuplicateEntry]
    ++ [ symbol 16 owned (if damage == EmptyOwned then 0 else 16) 2 1
       , symbol 34 (if damage == WrongResume then 0x00bfb030 else 0x00bfb02c) 0 0
           (if damage == UndefinedResume then 0 else 0xfff1)
       ]
  namesOffset = 512
  stringsOffset = namesOffset + B.length names
  symbolsOffset = stringsOffset + B.length strings
  sectionsOffset = symbolsOffset + B.length symbols
  header = B.concat
    [ B.pack [0x7f, 0x45, 0x4c, 0x46, 1, 1, 1, 0], B.replicate 8 0
    , half 2, half 40, word 1, word entry, word 52, word (fromIntegral sectionsOffset)
    , word 0, half 52, half 32, half 1, half 40, half (if damage == DuplicateText then 6 else 5), half 2
    ]
  segment = B.concat (map word [1, if damage == MisboundSegment then 252 else 256, base, base,
    if damage == TruncatedSegment then 252 else 256, 256,
    if damage == WritableSegment then 7 else if damage == NonExecutableSegment then 4 else 5, 4])
  textSection = section 1 1 (if damage == WritableText then 7 else if damage == UnallocatedText then 4 else 6) base 256 256 0 4 0
  sections = B.concat $
    [ B.replicate 40 0, textSection
    , section 7 3 0 0 (fromIntegral namesOffset) (fromIntegral (B.length names)) 0 1 0
    , section 17 3 0 0 (fromIntegral stringsOffset) (fromIntegral (B.length strings)) 0 1 0
    , section 25 2 0 0 (fromIntegral symbolsOffset) (fromIntegral (B.length symbols)) 3 4 16
    ] ++ [textSection | damage == DuplicateText]
  symbol name address size kind place = B.concat [word name, word address, word size, B.pack [kind, 0], half place]
  section name kind flags address offset size link alignment entrySize =
    B.concat (map word [name, kind, flags, address, offset, size, link, 0, alignment, entrySize])

word :: Word32 -> B.ByteString
word value = B.pack [fromIntegral (value `shiftR` (8 * index)) | index <- [0 .. 3]]

half :: Word32 -> B.ByteString
half = B.take 2 . word

patch :: Int -> Word32 -> B.ByteString -> B.ByteString
patch offset value bytes = B.take offset bytes <> word value <> B.drop (offset + 4) bytes

branch :: Word32 -> Word32 -> Word32
branch site target = 0xea000000 .|. (((target - site - 8) `shiftR` 2) .&. 0x00ffffff)
