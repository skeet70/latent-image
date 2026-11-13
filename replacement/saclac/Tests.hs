{-# LANGUAGE OverloadedStrings #-}
module Main (main) where

import qualified A32 as A
import Control.Monad (forM_, unless)
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import Data.Bits ((.&.), (.|.), shiftL, shiftR)
import Data.Word (Word32)
import qualified FujiAF.FirstStage as F
import qualified FujiAF.Host as H
import qualified FujiAF.UpdateContainer as U
import Replacement.Saclac.Artifact
import System.Environment (getArgs)

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
    [binPath, symbolsPath, disasmPath, sectionsPath] -> do
      bin <- H.readRegular (4 * 1024 * 1024) binPath
      symbols <- H.readRegular (1024 * 1024) symbolsPath
      disasm <- H.readRegular (1024 * 1024) disasmPath
      sections <- H.readRegular (1024 * 1024) sectionsPath
      admitted <- either (H.failH . show) pure (admit bin symbols disasm sections)
      body <- either (H.failH . show) pure (fixedBody admitted)
      let base = extentLow fixedExtent
          imageBase = 0xbf1a00
          imageBytes = 0x4f1e60
          blank = B.replicate imageBytes 0
          allocations = fixedExtent : extents
          gaps = zip (imageBase : map extentHigh extents) (map extentLow extents ++ [imageBase + fromIntegral imageBytes])
          outsideUntouched image (low, high) = B.all (== 0) (B.take (fromIntegral (high - low)) (B.drop (fromIntegral (low - imageBase)) image))
      installed <- either (H.failH . show) pure (installedImage blank admitted)
      either (H.failH . show) pure (admitInstalled installed admitted)
      check "three retired extents total 2320 bytes" (length extents == 3 && sum (map extentBytes extents) == 2320)
      check "fixed body is nonempty and bounded" (not (B.null body) && B.length body <= extentBytes fixedExtent)
      check "every allocated section fits one declared allocation"
        (all (\section -> any (\extent -> extentContains extent (sectionLow section) && extentContains extent (sectionLow section + fromIntegral (sectionBytes section) - 1)) allocations)
          [section | section <- admittedSections admitted, sectionAllocated section, sectionBytes section > 0])
      check "bytes outside retired extents are untouched" (all (outsideUntouched installed) gaps)
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
      stackTests
      putStrLn ("saclac-placement: fixed body " ++ show (B.length body) ++ " bytes; veneers, layout, publications and stack controls passed")
    _ -> H.failH "usage: saclac-placement-tests LINKED_BIN SYMBOLS DISASM SECTIONS"

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
