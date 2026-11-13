{-# LANGUAGE OverloadedStrings #-}

-- | Admission of the linked saclac image over its two retired extents. Each
-- refused case asserts the rule and the address that refused it, so deleting
-- that rule changes the result.
module Main (main) where

import qualified A32 as A
import Control.Monad (when)
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import qualified FujiAF.FirstStage as F
import qualified FujiAF.Host as H
import Data.Bits ((.&.), (.|.), shiftL, shiftR)
import Data.IORef (IORef, modifyIORef', newIORef, readIORef)
import Data.Word (Word32)
import Replacement.Saclac.Artifact
import System.Environment (getArgs, lookupEnv)

imageBytes :: Int
imageBytes = 0x4f1e60

-- | Run one named case only, for the rule-deletion check; every case runs when
-- the variable is unset.
selected :: String -> IO Bool
selected name = maybe (pure True) (pure . (== name)) =<< lookupEnv "SACLAC_PLACEMENT_ONLY"

check :: IORef Int -> String -> Bool -> IO ()
check counter name passed = do
  run <- selected name
  when run $ do
    H.ensure passed ("saclac-placement: " <> name)
    modifyIORef' counter (+ 1)

refused :: IORef Int -> String -> PlacementError -> Either PlacementError a -> IO ()
refused counter name expected result = do
  run <- selected name
  when run $ do
    case result of
      Left actual
        | actual == expected -> pure ()
        | otherwise ->
            H.failH
              ("saclac-placement: " <> name <> ": refused as " <> show actual
                <> ", expected " <> show expected)
      Right _ -> H.failH ("saclac-placement: a refused artifact was admitted: " <> name)
    modifyIORef' counter (+ 1)

main :: IO ()
main = do
  arguments <- getArgs
  case arguments of
    [binPath, symbolsPath, disasmPath, sectionsPath] -> do
      counter <- newIORef (0 :: Int)
      bin <- B.readFile binPath
      symbols <- B.readFile symbolsPath
      disasm <- B.readFile disasmPath
      sections <- B.readFile sectionsPath
      admitted <- either (H.failH . show) pure (admit bin symbols disasm sections)
      let blank = B.replicate imageBytes 0
      installed <- either (H.failH . show) pure (installedImage blank admitted)
      either (H.failH . show) pure (admitInstalled installed admitted)
      let extentTotal = sum (map extentBytes extents)
          ownedTotal = sum [sectionBytes section | section <- admittedSections admitted, sectionAllocated section]
          secondCounts = symbolAddress symbols "SACLAC_SECOND_COUNTS"
          actionCounts = symbolAddress symbols "SACLAC_ACTION_COUNTS"
          cellCounts = symbolAddress symbols "SACLAC_CELL_COUNTS"
      check counter "the two extents total 2,192 bytes" (extentTotal == 2192)
      check counter "every owned section lies inside an extent"
        (all (\section -> any (\extent -> extentContains extent (sectionLow section)
                  && extentContains extent (sectionLow section + fromIntegral (sectionBytes section) - 1))
                  extents) [section | section <- admittedSections admitted, sectionAllocated section])
      check counter "the dispatcher entry word is a branch" (installedWord installed 0xefca6c .&. 0x0f000000 == 0x0a000000)
      check counter "the router entry word is a branch" (installedWord installed 0xefcb88 .&. 0x0f000000 == 0x0a000000)
      check counter "unused dispatcher bytes carry the trap word" (installedWord installed 0xefca70 == trapWord)
      check counter "unused router bytes carry the trap word" (installedWord installed 0xefd2f8 == trapWord)
      check counter "the installed image leaves every byte outside the extents alone"
        (B.take ((fromIntegral (0xefca6c :: Word32) :: Int) - fromIntegral (0xbf1a00 :: Word32)) installed == B.take ((fromIntegral (0xefca6c :: Word32) :: Int) - fromIntegral (0xbf1a00 :: Word32)) blank
          && B.drop ((fromIntegral (0xefd2fc :: Word32) :: Int) - fromIntegral (0xbf1a00 :: Word32)) installed == B.drop ((fromIntegral (0xefd2fc :: Word32) :: Int) - fromIntegral (0xbf1a00 :: Word32)) blank)
      check counter "the rows and states are two declared arrays"
        (length retainedData == 6
          && any (\word -> retainedAddress word == 0x169d468) retainedData
          && any (\word -> retainedAddress word == 0x169d4b4) retainedData)
      check counter "machine 3's second table is two words" (linkedWord bin (secondCounts + 12) == 2)
      check counter "machine 14's second table is two words" (linkedWord bin (secondCounts + 56) == 2)
      check counter "machine 20's action table is eight words" (linkedWord bin (actionCounts + 80) == 8)
      check counter "machine 21's action table is seven words" (linkedWord bin (actionCounts + 84) == 7)
      refused counter "symbol outside" (SymbolOutside "stray" 0x00100000 4)
        (admit bin (symbols <> "00100000 00000004 T stray\n") disasm sections)
      refused counter "data symbol" (DataSymbol "d")
        (admit bin (symbols <> "00efcc00 00000004 d stray\n") disasm sections)
      refused counter "section outside" (SectionOutside ".stray" 0x00100000 4)
        (admit bin symbols disasm (sections <> "  [ 7] .stray           PROGBITS        00100000 00cc00 000004 00  AX  0   0  4\n"))
      refused counter "writable section" (WritableSection ".data")
        (admit bin symbols disasm (sections <> "  [ 7] .data            PROGBITS        00efcc00 00cc00 000004 00  WA  0   0  4\n"))
      refused counter "NOBITS section" (WritableSection ".bss")
        (admit bin symbols disasm (sections <> "  [ 8] .bss             NOBITS          00efcc00 00cc00 000004 00  WA  0   0  4\n"))
      refused counter "moved entry" (EntryMoved "xm1_saclac_dispatch_entry" 0xefca6c 0xefca70)
        (admit bin (movedEntrySymbols symbols) disasm sections)
      refused counter "entry veneer size" (EntryVeneerSize "xm1_saclac_dispatch_entry" 8)
        (admit bin (sizedVeneerSymbols symbols) disasm sections)
      refused counter "entry body outside the extents" (EntryNotOwned "xm1_saclac_dispatch" 0x00100000)
        (admit bin (movedBodySymbols symbols) disasm sections)
      mismatchEntry <- patchAt bin 0xefca6c (branchAt 0xefca6c 0xefcb90)
      refused counter "entry branch mismatch" (EntryBranchMismatch "xm1_saclac_dispatch_entry" 0xefca6c 0xefcce8)
        (admit mismatchEntry symbols disasm sections)
      wrongLength <- patchAt bin cellCounts 529
      refused counter "a declared count that is not the table's extent"
        (TableLengthMismatch "SACLAC_CELL_COUNTS" 0 528 529)
        (admit wrongLength symbols disasm sections)
      refused counter "wrong fill" (FillDiffers 0xefca70) (admitInstalled (corruptFill installed) admitted)
      rejectedInstruction <- patchAt bin 0xefcba4 0xef000000
      refused counter "a supervisor call is outside the admitted ISA"
        (WalkRefused 0xefcba4 (DecoderRejected A.SupervisorCall))
        (admit rejectedInstruction symbols disasm sections)
      refused counter "a fall-through beyond the declared extents"
        (WalkRefused 0xefd2fc FallThroughOutsideOwned)
        (admit bin symbols (disasm <> "  efd2f8:\tandeq r0, r0, r0\n") sections)
      -- The literal pool, the stores and the direct branches.
      badLiteral <- patchAt bin 0xefcce4 0x00123454
      refused counter "a literal naming no declared address"
        (WalkRefused 0xefcce4 (LiteralOutside 0x00123454))
        (admit badLiteral symbols disasm sections)
      codeStore <- patchAt bin 0xefccbc 0xe1c300b0
      refused counter "a store through a register a code literal loaded"
        (WalkRefused 0xefccbc StoreToCode)
        (admit codeStore symbols disasm sections)
      pcStore <- patchAt bin 0xefcbf0 0xe58f3018
      refused counter "a store through pc" (WalkRefused 0xefcbf0 StoreToCode)
        (admit pcStore symbols disasm sections)
      undeclaredBranch <- patchAt bin 0xefcca0 (branchAt 0xefcca0 0x00123454)
      refused counter "undeclared branch" (WalkRefused 0xefcca0 (BranchTargetNotAllowed 0x00123454))
        (admit undeclaredBranch symbols disasm sections)
      branchToFill <- patchAt bin 0xefcca0 (branchAt 0xefcca0 0xefca70)
      refused counter "a direct branch into retired trap fill"
        (WalkRefused 0xefcca0 (BranchTargetNotAllowed 0xefca70))
        (admit branchToFill symbols disasm sections)
      undeclaredCall <- patchAt bin 0xefcc6c (linkBranchAt 0xefcc6c 0x00100000)
      refused counter "undeclared call" (WalkRefused 0xefcc6c (BranchTargetNotAllowed 0x00100000))
        (admit undeclaredCall symbols disasm sections)
      conditionalBranch <- patchAt bin 0xefcca0 (branchWith 1 0xefcca0 0x00100000)
      refused counter "conditional branch outside" (WalkRefused 0xefcca0 (BranchTargetNotAllowed 0x00100000))
        (admit conditionalBranch symbols disasm sections)
      dataBranch <- patchAt bin 0xefcca0 (branchAt 0xefcca0 0xefccc4)
      refused counter "a branch to a data word" (WalkRefused 0xefcca0 BranchToDataWord)
        (admit dataBranch symbols disasm sections)
      dataFallThrough <- patchAt bin 0xefcd38 (branchWith 1 0xefcd38 0xefcb8c)
      refused counter "a fall-through into a data word" (WalkRefused 0xefcd3c FallThroughIntoDataWord)
        (admit dataFallThrough symbols disasm sections)
      pcWriteback <- patchAt bin 0xefcbf4 0xe5bf30e4
      refused counter "a pc-relative load with writeback" (WalkRefused 0xefcbf4 (DecoderRejected A.Unadmitted))
        (admit pcWriteback symbols disasm sections)
      literalOutside <- patchAt bin 0xefcb98 0xe51f3400
      refused counter "a pc-relative load outside the image" (WalkRefused 0xefcb98 PcRelativeLiteralOutside)
        (admit literalOutside symbols disasm sections)
      -- Indirect control flow, only inside the stock-boundary functions.
      refused counter "an indirect branch outside the boundary functions"
        (WalkRefused 0xefcc2c (IndirectOutsideBoundary "stray"))
        (admit bin (renamedBoundarySymbols symbols) disasm sections)
      conditionalExchange <- patchAt bin 0xefcc6c 0x112fff33
      check counter "a conditional register branch inside the boundary is admitted"
        (either (const False) (const True) (admit conditionalExchange symbols disasm sections))
      backwardBranch <- patchAt bin 0xefcbb0 (branchWith 2 0xefcbb0 0xefcbac)
      check counter "a boundary branch to an owned join is admitted"
        (either (const False) (const True) (admit backwardBranch symbols disasm sections))
      -- The frame, the call graph and task 43's stack.
      noFrame <- patchAt bin 0xefcd04 0x192d4800
      refused counter "a function whose frame cannot be derived" (WalkRefused 0xefcd04 FrameNotDerivable)
        (admit noFrame symbols disasm sections)
      recursion <- patchAt bin 0xefccb0 (branchAt 0xefccb0 0xefcb8c)
      refused counter "a function that calls itself" (WalkRefused 0xefccb0 (RecursiveCall 0xefcb8c))
        (admit recursion symbols disasm sections)
      stackBudget <- patchAt bin 0xefcb94 0xe24ddb01
      refused counter "a frame that exceeds the owned stack budget"
        (WalkRefused 0xefca6c (StackBudgetExceeded 1068))
        (admit stackBudget symbols disasm sections)
      passed <- readIORef counter
      putStrLn ("saclac-placement: " <> show passed <> " checks passed; owned " <> show ownedTotal <> " of " <> show extentTotal <> " extent bytes")
    _ -> H.failH "usage: saclac-placement-tests LINKED_BIN SYMBOLS DISASM SECTIONS"
  where
    patchAt :: B.ByteString -> Word32 -> Word32 -> IO B.ByteString
    patchAt image address replacement = pure (patchWord image address replacement)
    installedWord :: B.ByteString -> Word32 -> Word32
    installedWord image address =
      let offset = (fromIntegral (address :: Word32) :: Int) - fromIntegral (0xbf1a00 :: Word32)
      in fromIntegral (B.index image offset)
         + fromIntegral (B.index image (offset + 1)) * 0x100
         + fromIntegral (B.index image (offset + 2)) * 0x10000
         + fromIntegral (B.index image (offset + 3)) * 0x1000000
    linkedWord :: B.ByteString -> Word32 -> Word32
    linkedWord image address =
      let offset = fromIntegral (address - 0xefca6c)
      in fromIntegral (B.index image offset)
         + fromIntegral (B.index image (offset + 1)) * 0x100
         + fromIntegral (B.index image (offset + 2)) * 0x10000
         + fromIntegral (B.index image (offset + 3)) * 0x1000000
    symbolAddress :: B.ByteString -> String -> Word32
    symbolAddress rows name = case F.parseSymbols rows >>= \parsed -> F.lookupSymbol parsed name of
      Right symbol -> F.symbolAddress symbol
      Left _ -> 0
    corruptFill image =
      let offset = (fromIntegral (0xefca70 :: Word32) :: Int) - fromIntegral (0xbf1a00 :: Word32)
      in B.take offset image <> B.singleton 0 <> B.drop (offset + 1) image
    movedEntrySymbols rows =
      C.unlines
        ([row | row <- C.lines rows, "xm1_saclac_dispatch_entry" `notElem` C.words row]
          ++ ["00efca70 00000004 T xm1_saclac_dispatch_entry"])
    sizedVeneerSymbols rows =
      C.unlines
        ([row | row <- C.lines rows, "xm1_saclac_dispatch_entry" `notElem` C.words row]
          ++ ["00efca6c 00000008 T xm1_saclac_dispatch_entry"])
    movedBodySymbols rows =
      C.unlines
        ([row | row <- C.lines rows, "xm1_saclac_dispatch" `notElem` C.words row]
          ++ ["00100000 00000000 T xm1_saclac_dispatch"])
    renamedBoundarySymbols rows =
      C.unlines
        [ case C.words row of
            (_ : _ : _ : name : _)
              | "_RINvNtCs5dX5h91vudr_10xm1_saclac6engine8dispatch" `C.isPrefixOf` name ->
                  "00efcb8c 0000015c t stray"
            _ -> row
        | row <- C.lines rows
        ]
    patchWord :: B.ByteString -> Word32 -> Word32 -> B.ByteString
    patchWord image address word =
      let offset = fromIntegral (address - 0xefca6c)
          encode value = B.pack [fromIntegral (value `shiftR` (8 * index)) | index <- [0 :: Int .. 3]]
      in B.take offset image <> encode word <> B.drop (offset + 4) image
    branchWith :: Word32 -> Word32 -> Word32 -> Word32
    branchWith condition address target =
      (condition `shiftL` 28) .|. 0x0a000000 .|. (((target - address - 8) `shiftR` 2) .&. 0xffffff)
    branchAt :: Word32 -> Word32 -> Word32
    branchAt = branchWith 14
    linkBranchAt :: Word32 -> Word32 -> Word32
    linkBranchAt address target =
      0xeb000000 .|. (((target - address - 8) `shiftR` 2) .&. 0xffffff)
