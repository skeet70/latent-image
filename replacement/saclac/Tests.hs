{-# LANGUAGE OverloadedStrings #-}
module Main (main) where

import qualified A32 as A
import qualified ArmElf as ELF
import Control.Monad (forM_, unless)
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import Data.Bits ((.&.), (.|.), shiftL, shiftR)
import Data.List (sortOn)
import Data.Word (Word32)
import qualified FujiAF.FirstStage as F
import qualified FujiAF.Host as H
import qualified FujiAF.UpdateContainer as U
import Replacement.Saclac.Artifact hiding (admit)
import qualified Replacement.Saclac.Artifact as Artifact
import System.Environment (getArgs)
import System.Directory (canonicalizePath)

check :: String -> Bool -> IO ()
check name = flip H.ensure ("saclac-placement: " ++ name)

refused :: String -> PlacementError -> Either PlacementError a -> IO ()
refused name expected result = case result of
  Left actual -> check (name ++ ": " ++ show actual ++ ", expected " ++ show expected) (actual == expected)
  Right _ -> H.failH ("saclac-placement: refused artifact admitted: " ++ name)

symbol :: B.ByteString -> String -> IO F.Symbol
symbol bytes name = either H.failH pure (F.parseSymbols bytes >>= flip F.lookupSymbol name)

replaceSymbol :: B.ByteString -> String -> C.ByteString -> B.ByteString
replaceSymbol rows name replacement = C.unlines
  ([row | row <- C.lines rows, C.pack name `notElem` C.words row] ++ [replacement])

branch :: Word32 -> Word32 -> Word32
branch address target = 0xea000000 .|. (((target - address - 8) `shiftR` 2) .&. 0xffffff)

patch :: Word32 -> B.ByteString -> Word32 -> Word32 -> B.ByteString
patch base bytes address word =
  let offset = fromIntegral (address - base)
   in B.take offset bytes <> U.putWord32 word <> B.drop (offset + 4) bytes

main :: IO ()
main = do
  arguments <- getArgs
  case arguments of
    [elfPath, binPath, symbolsPath, disasmPath, sectionsPath] -> do
      elf <- canonicalizePath elfPath >>= H.readRegular (8 * 1024 * 1024)
      let admit = Artifact.admit elf
      bin <- canonicalizePath binPath >>= H.readRegular (8 * 1024 * 1024)
      symbols <- canonicalizePath symbolsPath >>= H.readRegular (4 * 1024 * 1024)
      disasm <- canonicalizePath disasmPath >>= H.readRegular (64 * 1024 * 1024)
      sections <- canonicalizePath sectionsPath >>= H.readRegular (1024 * 1024)
      admitted <- either (H.failH . show) pure (admit bin symbols disasm sections)
      body <- either (H.failH . show) pure (fixedBody admitted)
      let base = extentLow fixedExtent
          imageBase = 0xbf1a00
          imageBytes = 0x4f1e60
          blank = patch imageBase (B.replicate imageBytes 0) taskCreateSite taskCreateWord
          publications = sortOn extentLow (Extent taskCreateSite (taskCreateSite + 4) : extents)
          gaps = zip (imageBase : map extentHigh publications) (map extentLow publications ++ [imageBase + fromIntegral imageBytes])
          outsideUntouched image (low, high) = B.all (== 0) (B.take (fromIntegral (high - low)) (B.drop (fromIntegral (low - imageBase)) image))
      installed <- either (H.failH . show) pure (installedImage blank admitted)
      either (H.failH . show) pure (admitInstalled installed admitted)
      check "bytes outside declared publications are untouched" (all (outsideUntouched installed) gaps)
      refused "unknown task creation preimage" (TaskCreatePreimage 0)
        (installedImage (patch imageBase blank taskCreateSite 0) admitted)
      refused "installed task creation target differs" (ExtentImageMismatch taskCreateSite 4)
        (admitInstalled (patch imageBase installed taskCreateSite taskCreateWord) admitted)
      forM_ entries $ \entry -> do
        target <- F.symbolAddress <$> symbol symbols (entryBody entry)
        let offset = fromIntegral (entryAddress entry - imageBase)
        check (entryVeneer entry ++ " branches to its fixed body")
          (A.decode (entryAddress entry) (U.word32At installed offset) == A.Branch (A.Cond 14) False target)
        unless (any (`extentContains` target) [fixedExtent]) (H.failH "entry body leaves fixed range")
      forM_ extents $ \(Extent low high) -> forM_ [low, low + 4 .. high - 4] $ \address ->
        unless (address `elem` map entryAddress entries) $
          check "retired non-entry words are trap fill" (U.word32At installed (fromIntegral (address - imageBase)) == trapWord)
      refused "symbol outside" (SymbolOutside "stray" 0x100000 4)
        (admit bin (symbols <> "00100000 00000004 T stray\n") disasm sections)
      refused "mutable symbol in retired code" (DataSymbol "d")
        (admit bin (symbols <> "00efcc00 00000004 d stray\n") disasm sections)
      refused "section outside" (SectionOutside ".stray" 0x100000 4)
        (admit bin symbols disasm (sections <> "  [ 7] .stray PROGBITS 00100000 00cc00 000004 00 AX 0 0 4\n"))
      refused "writable retired code" (WritableSection ".data")
        (admit bin symbols disasm (sections <> "  [ 7] .data PROGBITS 00efcc00 00cc00 000004 00 WA 0 0 4\n"))
      refused "BSS in retired code" (WritableSection ".bss")
        (admit bin symbols disasm (sections <> "  [ 8] .bss NOBITS 00efcc00 00cc00 000004 00 WA 0 0 4\n"))
      refused "moved veneer" (EntryMoved "xm1_saclac_dispatch_entry" 0xefca6c 0xefca70)
        (admit bin (replaceSymbol symbols "xm1_saclac_dispatch_entry" "00efca70 00000004 T xm1_saclac_dispatch_entry") disasm sections)
      refused "veneer size" (EntryVeneerSize "xm1_saclac_dispatch_entry" 8)
        (admit bin (replaceSymbol symbols "xm1_saclac_dispatch_entry" "00efca6c 00000008 T xm1_saclac_dispatch_entry") disasm sections)
      refused "body outside fixed RAM" (EntryNotOwned "xm1_saclac_dispatch" 0x100000)
        (admit bin (replaceSymbol symbols "xm1_saclac_dispatch" "00100000 00000000 T xm1_saclac_dispatch") disasm sections)
      dispatch <- F.symbolAddress <$> symbol symbols "xm1_saclac_dispatch"
      refused "veneer target mismatch" (EntryBranchMismatch "xm1_saclac_dispatch_entry" 0xefca6c dispatch)
        (admit (patch base bin 0xefca6c (branch 0xefca6c (dispatch + 4))) symbols disasm sections)
      counts <- F.symbolAddress <$> symbol symbols "SACLAC_CELL_COUNTS"
      refused "retained table extent mismatch" (TableLengthMismatch "SACLAC_CELL_COUNTS" 0 528 529)
        (admit (patch base bin counts 529) symbols disasm sections)
      marker <- F.symbolAddress <$> symbol symbols "XM1_REQUIRES_LENS_SAFETY_CONTRACT"
      refused "missing lens safety flag" SafetyMarkerInvalid
        (admit (patch base bin marker 0) symbols disasm sections)
      refused "retired fill differs" (FillDiffers 0xefca70)
        (admitInstalled (patch imageBase installed 0xefca70 0) admitted)
      parsed <- either H.failH pure (ELF.parseElf elf)
      references <- either H.failH pure (ELF.addressRelocations parsed)
      let dataReferences =
            [(reference, section) | reference <- references, ELF.addressKind reference == ELF.DataReference,
              section <- ELF.elfSections parsed, not (ELF.sectionExecutable section),
              ELF.sectionAddress section <= ELF.addressSite reference,
              ELF.addressSite reference + 4 <= ELF.sectionAddress section + ELF.sectionSize section]
      case dataReferences of
        (reference, section) : _ -> do
          let site = ELF.addressSite reference
              fileOffset = ELF.sectionOffset section + site - ELF.sectionAddress section
              target = 0x123454
              badElf = patch 0 elf fileOffset target
          refused "undeclared definite reference" (ReferenceOutside site target)
            (Artifact.admit badElf (patch base bin site target) symbols disasm sections)
        [] -> H.failH "linked ELF has no data relocation for the refusal control"
      boundaryReferenceTests elf bin symbols disasm sections admitted parsed dataReferences
      stackTests
      putStrLn ("saclac-placement: fixed body " ++ show (B.length body) ++ " bytes; veneers, layout, publications and stack controls passed")
    _ -> H.failH "usage: saclac-placement-tests LINKED_ELF LINKED_BIN SYMBOLS DISASM SECTIONS"

boundaryReferenceTests :: B.ByteString -> B.ByteString -> B.ByteString -> B.ByteString -> B.ByteString
  -> Admitted -> ELF.Elf -> [(ELF.AddressRelocation, ELF.Section)] -> IO ()
boundaryReferenceTests elf bin symbols disasm sections admitted parsed references = do
  let bss = [section | section <- admittedSections admitted, sectionAllocated section,
        sectionNoBits section, sectionName section == ".bss"]
      boundarySymbols = [(index, value) | (index, value) <- zip [0 :: Word32 ..] (ELF.elfSymbols parsed),
        ELF.symbolName value == "__power_cpu_end"]
      symbolTables = [section | section <- ELF.elfSections parsed, ELF.sectionType section == 2]
  case (bss, boundarySymbols, symbolTables, references) of
    ([section], [(identity, boundary)], [table], (reference, source) : _) -> do
      let site = ELF.addressSite reference
          end = sectionLow section + fromIntegral (sectionBytes section)
          contentOffset = ELF.sectionOffset source + site - ELF.sectionAddress source
          symbolOffset = ELF.sectionOffset table + identity * 16
          relocationOffsets =
            [offset + 4 | relocation <- ELF.elfSections parsed, ELF.sectionType relocation == 9,
              offset <- [ELF.sectionOffset relocation, ELF.sectionOffset relocation + 8
                .. ELF.sectionOffset relocation + ELF.sectionSize relocation - 8],
              U.word32At elf (fromIntegral offset) == site]
          run image target = Artifact.admit (patch 0 image contentOffset target)
            (patch (extentLow fixedExtent) bin site target) symbols disasm sections
          refusedEndpoint name result = case result of
            Left (ReferenceOutside _ target) -> check name (target == end)
            other -> H.failH (name ++ ": expected endpoint refusal, got " ++ show other)
      check "linked power boundary equals allocated BSS endpoint" (ELF.symbolValue boundary == end)
      check "endpoint is not allocated storage" (not (any (\(low, high) -> low <= end && end < high) (ownedSpans admitted)))
      case relocationOffsets of
        [encodingOffset] -> do
          let metadata = patch 0 elf encodingOffset ((identity `shiftL` 8) .|. 2)
              direct = patch 0 metadata encodingOffset ((identity `shiftL` 8) .|. 29)
              wrongName = patch 0 metadata symbolOffset 0
              wrongValue = patch 0 metadata (symbolOffset + 4) (end + 4)
              sized = patch 0 metadata (symbolOffset + 8) 4
              undefinedSymbol = patch 0 metadata (symbolOffset + 12)
                (U.word32At metadata (fromIntegral (symbolOffset + 12)) .&. 0xffff)
          either (H.failH . show) (const (pure ())) (run metadata end)
          refusedEndpoint "unnamed endpoint is not metadata" (run wrongName end)
          refusedEndpoint "boundary symbol has wrong value" (run wrongValue end)
          refused "boundary addend leaves endpoint" (ReferenceOutside site (end + 4)) (run metadata (end + 4))
          refusedEndpoint "sized symbol is not endpoint metadata" (run sized end)
          refused "endpoint metadata is not a branch destination" (ReferenceOutside site end)
            (Artifact.admit (patch 0 direct contentOffset (branch site end))
              (patch (extentLow fixedExtent) bin site (branch site end)) symbols disasm sections)
          case run undefinedSymbol end of
            Left (ElfRejected _) -> pure ()
            other -> H.failH ("undefined boundary symbol admitted: " ++ show other)
          refused "raw ELF/bin reference disagreement" (ExtentImageMismatch site 4)
            (Artifact.admit (patch 0 metadata contentOffset end) (patch (extentLow fixedExtent) bin site (end + 4))
              symbols disasm sections)
        _ -> H.failH "fixture requires one relocation at its data-reference site"
    _ -> H.failH "fixture requires allocated BSS, __power_cpu_end and a data relocation"

stackTests :: IO ()
stackTests = do
  small <- maybe (H.failH "invalid fixture stack budget") pure (stackBudget 4)
  fitting <- maybe (H.failH "invalid fixture stack budget") pure (stackBudget 8)
  let base = 0xba0020
      symbols = "00ba0020 0000000c T fixture\n"
      sections = "[ 1] .text PROGBITS 00ba0020 000000 00000c 00 AX 0 0 4\n"
      disasm = "  ba0020:\tpush {r11, lr}\n  ba0024:\tmov r0, r0\n  ba0028:\tpop {r11, pc}\n"
      body = B.concat (map U.putWord32 [0xe92d4800, 0xe1a00000, 0xe8bd8800])
      run budget bytes = checkStackArtifact budget bytes symbols disasm sections
  either (H.failH . show) pure (run fitting body)
  refused "frame exceeds caller budget" (WalkRefused base (StackBudgetExceeded 8)) (run small body)
  refused "direct recursion" (WalkRefused (base + 4) (RecursiveCall base))
    (run fitting (patch base body (base + 4) (branch (base + 4) base .|. (1 `shiftL` 24))))
  refused "unrecognized frame" (WalkRefused base FrameNotDerivable)
    (run fitting (patch base body base 0x192d4800))
