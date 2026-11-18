{-# LANGUAGE OverloadedStrings #-}
module Main (main) where

import qualified A32 as A
import Control.Monad (forM_)
import qualified Data.ByteString as B
import qualified Data.Set as S
import qualified FujiAF.FirstStage as F
import qualified FujiAF.Host as H
import qualified FujiAF.UpdateContainer as U
import Replacement.Toolchain.Stack

main :: IO ()
main = do
  let base = 0x1000
      words' = [0xe92d4830, 0xe92d500f, 0xeb000001, 0xe8bd500f, 0xe8bd8830, 0xe92d4800, 0xe8bd8800]
      body = B.concat (map U.putWord32 words')
      points = S.fromList (map Guest [base, base + 4 .. base + 24])
  symbols <- either H.failH pure (F.parseSymbols "00001000 00000014 T caller\n00001014 00000008 T callee\n")
  fitting <- maybe (H.failH "invalid budget") pure (stackBytes 48)
  short <- maybe (H.failH "invalid budget") pure (stackBytes 47)
  let image = Image (Guest base) body symbols points
  either (H.failH . show) pure (check fitting image)
  H.ensure (check short image == Left (BudgetExceeded (Guest base) fitting)) "consecutive pushes and callee stack were not summed"
  let recursive = image { imageBytes = B.take 8 body <> U.putWord32 0xebfffffc <> B.drop 12 body }
  H.ensure (check fitting recursive == Left (RecursiveCall (Guest (base + 8)) (Guest base))) "recursive call was admitted"
  let unknownFrame = image { imageBytes = U.putWord32 0x192d4830 <> B.drop 4 body }
  H.ensure (check fitting unknownFrame == Left (FrameNotDerivable (Guest base))) "conditional frame was treated as known"
  let outside = image { imageInstructions = S.insert (Guest (base + 28)) points }
  H.ensure (check fitting outside == Left (OutsideImage (Guest (base + 28)))) "missing instruction bytes were admitted"
  H.ensure (stackBytes (-1) == Nothing) "negative stack allowance was admitted"
  zero <- maybe (H.failH "invalid zero budget") pure (stackBytes 0)
  haltSymbols <- either H.failH pure (F.parseSymbols "00001000 00000008 T halt\n")
  let halt = Image (Guest base) (B.concat (map U.putWord32 [0xe320f003, 0xeafffffd])) haltSymbols (S.fromList [Guest base, Guest (base + 4)])
  either (H.failH . show) pure (check zero halt)
  nonreturnSymbols <- either H.failH pure (F.parseSymbols "00001000 0000000c T caller\n00001010 0000000c T halt\n")
  let nonreturnBody = B.concat (map U.putWord32 [0xe92d4800, 0xeb000001, 0, 0, 0xe92d4800, 0xe320f003, 0xeafffffd])
      nonreturnPoints = S.fromList (map Guest [base, base + 4, base + 16, base + 20, base + 24])
      nonreturn = Image (Guest base) nonreturnBody nonreturnSymbols nonreturnPoints
  sixteenHalt <- maybe (H.failH "invalid halt budget") pure (stackBytes 16)
  fifteenHalt <- maybe (H.failH "invalid short halt budget") pure (stackBytes 15)
  either (H.failH . show) pure (check sixteenHalt nonreturn)
  H.ensure (check fifteenHalt nonreturn == Left (BudgetExceeded (Guest base) sixteenHalt))
    "a nonreturning callee lost its caller's live frame"
  let conditionalHalt = nonreturn { imageBytes = B.take 4 nonreturnBody <> U.putWord32 0x0b000001 <> B.drop 8 nonreturnBody }
  H.ensure (check sixteenHalt conditionalHalt == Left (FrameNotDerivable (Guest base)))
    "a conditional nonreturning call hid its uncovered fall-through"
  let returningHalt = nonreturn { imageBytes = B.take 20 nonreturnBody <> U.putWord32 0xe8bd8800 <> B.drop 24 nonreturnBody }
  H.ensure (check sixteenHalt returningHalt == Left (FrameNotDerivable (Guest base)))
    "a returning callee was assumed not to return into data"
  tailSymbols <- either H.failH pure (F.parseSymbols "00001000 00000004 T first\n00001004 00000004 T second\n")
  let tails = halt { imageBytes = B.concat (map U.putWord32 [0xeaffffff, 0xeafffffd]), imageSymbols = tailSymbols }
  either (H.failH . show) pure (check zero tails)
  tailCallSymbols <- either H.failH pure (F.parseSymbols "00001000 00000004 T first\n00001004 00000008 T second\n")
  let tailThenCall = tails { imageBytes = B.concat (map U.putWord32 [0xeaffffff, 0xebfffffd, 0xe12fff1e]),
        imageSymbols = tailCallSymbols, imageInstructions = S.fromList (map Guest [base, base + 4, base + 8]) }
  H.ensure (check zero tailThenCall == Left (RecursiveCall (Guest (base + 4)) (Guest base))) "call growth through a tail edge was admitted"
  let earlyWords = [0xe3500000, 0x112fff1e, 0xe92d4800, 0xe24dd010, 0xeb000001, 0xe28dd010, 0xe8bd8800, 0xe92d4800, 0xe8bd8800]
      earlyBody = B.concat (map U.putWord32 earlyWords)
  earlySymbols <- either H.failH pure (F.parseSymbols "00001000 0000001c T early\n0000101c 00000008 T callee\n")
  let early = Image (Guest base) earlyBody earlySymbols (S.fromList (map Guest [base, base + 4 .. base + 32]))
  exact <- maybe (H.failH "invalid frame budget") pure (stackBytes 32)
  under <- maybe (H.failH "invalid frame budget") pure (stackBytes 31)
  either (H.failH . show) pure (check exact early)
  H.ensure (check under early == Left (BudgetExceeded (Guest base) exact)) "early return hid the allocated frame or callee depth"
  let splitWords = [0xe3500000, 0x0a000006, 0xe92d4800, 0xe1a0b00d, 0xe1a00001,
        0xeb000004, 0xe1a01000, 0xe3a00001, 0xe8bd8800, 0xe3a00000, 0xe12fff1e,
        0xe92d4800, 0xe8bd8800]
      splitBody = B.concat (map U.putWord32 splitWords)
  splitSymbols <- either H.failH pure (F.parseSymbols "00001000 0000002c T split\n0000102c 00000008 T callee\n")
  let split = Image (Guest base) splitBody splitSymbols (S.fromList (map Guest [base, base + 4 .. base + 48]))
  sixteen <- maybe (H.failH "invalid split-frame budget") pure (stackBytes 16)
  fifteen <- maybe (H.failH "invalid short split-frame budget") pure (stackBytes 15)
  either (H.failH . show) pure (check sixteen split)
  H.ensure (check fifteen split == Left (BudgetExceeded (Guest base) sixteen))
    "a branch to a frameless suffix hid caller or callee stack"
  let changingLeaf = split { imageBytes = B.take 36 splitBody <> U.putWord32 0xe24dd004 <> B.drop 40 splitBody }
  H.ensure (check sixteen changingLeaf == Left (FrameNotDerivable (Guest base)))
    "a skipped suffix that changes SP was treated as a leaf return"
  let dispatchWords = [0xe3500000, 0x0a000001, 0xea00000a, 0xe320f000, 0xe3510000,
        0x0a000003, 0xe92d4800, 0xe1a0b00d, 0xeb000002, 0xe8bd8800, 0xe3a00000,
        0xe12fff1e, 0xe92d4800, 0xe8bd8800, 0xe92d4800, 0xe24dd008, 0xe28dd008, 0xe8bd8800]
  dispatchSymbols <- either H.failH pure (F.parseSymbols "00001000 00000030 T dispatch\n00001030 00000008 T callee\n00001038 00000010 T tail\n")
  let dispatch = Image (Guest base) (B.concat (map U.putWord32 dispatchWords)) dispatchSymbols
        (S.fromList (map Guest [base, base + 4 .. base + 68]))
  either (H.failH . show) pure (check sixteen dispatch)
  H.ensure (check fifteen dispatch == Left (BudgetExceeded (Guest base) sixteen))
    "pre-frame dispatch lost a framed call or tail callee budget"
  let dispatchBody = imageBytes dispatch
      returningDispatch = dispatch { imageBytes = B.take 8 dispatchBody <> U.putWord32 0xe12fff1e
        <> B.take 8 (B.drop 12 dispatchBody) <> U.putWord32 0x0affffff <> B.drop 24 dispatchBody }
      jumpingDispatch = dispatch { imageBytes = B.take 8 dispatchBody <> U.putWord32 0xea000002 <> B.drop 12 dispatchBody }
  forM_ [returningDispatch, jumpingDispatch] $ \variant -> do
    either (H.failH . show) pure (check sixteen variant)
    H.ensure (check fifteen variant == Left (BudgetExceeded (Guest base) sixteen))
      "a return or jump to the frame entry hid the called frame"
  singleSymbols <- either H.failH pure (F.parseSymbols "00001000 00000010 T single\n")
  let single instructions = Image (Guest base) (B.concat (map U.putWord32 instructions))
        singleSymbols (S.fromList (map Guest [base, base + 4 .. base + 12]))
      hiddenFrame = single [0xe52de008, 0xe92d4800, 0xe8bd4800, 0xe49df008]
  eight <- maybe (H.failH "invalid push budget") pure (stackBytes 8)
  seven <- maybe (H.failH "invalid short push budget") pure (stackBytes 7)
  H.ensure (check eight hiddenFrame == Left (BudgetExceeded (Guest base) sixteen))
    "a single-transfer allocation before a push hid the 16-byte peak"
  either (H.failH . show) pure (check sixteen hiddenFrame)
  afterSymbols <- either H.failH pure (F.parseSymbols "00001000 00000014 T after\n")
  let after = Image (Guest base)
        (B.concat (map U.putWord32 [0xeaffffff, 0xe92d4800, 0xe52de008, 0xe49de008, 0xe8bd8800]))
        afterSymbols (S.fromList (map Guest [base, base + 4 .. base + 16]))
  H.ensure (check eight after == Left (BudgetExceeded (Guest base) sixteen))
    "an allocation after the initial push hid the 16-byte peak"
  either (H.failH . show) pure (check sixteen after)
  dualSymbols <- either H.failH pure (F.parseSymbols "00001000 0000001c T dual\n")
  let dual = Image (Guest base)
        (B.concat (map U.putWord32 [0xe3500000, 0x0a000001, 0xe92d4800, 0xe8bd8800, 0xe1c1c0d0, 0xe1c2c0d0, 0xe12fff1e]))
        dualSymbols (S.fromList (map Guest [base, base + 4 .. base + 24]))
  H.ensure (check sixteen dual == Left (FrameNotDerivable (Guest base)))
    "a dual load into SP was treated as stack-neutral"
  let exclusiveSp = dual { imageBytes = B.concat
        (map U.putWord32 [0xe3500000, 0x0a000001, 0xe92d4800, 0xe8bd8800, 0xe1b1cf9f, 0xe1b2cf9f, 0xe12fff1e]) }
  H.ensure (check sixteen exclusiveSp == Left (FrameNotDerivable (Guest base)))
    "an exclusive double load's second SP destination was treated as stack-neutral"
  let exclusiveNeutral = single [0xe92d4800, 0xe1b40f9f, 0xe320f000, 0xe8bd8800]
  either (H.failH . show) pure (check eight exclusiveNeutral)
  H.ensure (check seven exclusiveNeutral == Left (BudgetExceeded (Guest base) eight))
    "a non-stack exclusive double load changed the exact frame budget"
  gapSymbols <- either H.failH pure (F.parseSymbols "00001000 00000018 T gap\n")
  let gap = Image (Guest base)
        (B.concat (map U.putWord32 [0x0a000000, 0xe24dd008, 0xe92d4800, 0xe8bd4800, 0x128dd008, 0xe12fff1e]))
        gapSymbols (S.fromList (map Guest [base, base + 8, base + 12, base + 16, base + 20]))
  H.ensure (check sixteen gap == Left (FrameNotDerivable (Guest base)))
    "a conditional branch hid an uncovered fall-through allocation"
  frameSymbols <- either H.failH pure (F.parseSymbols "00001000 00000028 T frame\n")
  forM_ [(0xe53b0008, 0xe49b0008), (0xe52b0008, 0xe49b0008),
      (0xe41b0008, 0xe49b0008), (0xe40b0008, 0xe49b0008),
      (0xe92b0003, 0xe8bb0003), (0xe93b0003, 0xe8bb0003)] $ \(change, restore) -> do
    let changedFrame = Image (Guest base)
          (B.concat (map U.putWord32 [0x0a000001, 0xe92d4800, 0xe8bd8800, 0xe92d4800,
            0xe1a0b00d, change, 0xe1a0d00b, restore, 0xe1a0d00b, 0xe8bd8800]))
          frameSymbols (S.fromList (map Guest [base, base + 4 .. base + 36]))
    H.ensure (check sixteen changedFrame == Left (FrameNotDerivable (Guest base)))
      "transfer-base writeback left a stale frame position available to restore SP"
  let exclusiveFrame = Image (Guest base)
        (B.concat (map U.putWord32 [0x0a000001, 0xe92d4800, 0xe8bd8800, 0xe92d4800, 0xe1a0b00d,
          0xe1b1af9f, 0xe1a0d00b, 0xe1b2af9f, 0xe1a0d00b, 0xe8bd8800]))
        frameSymbols (S.fromList (map Guest [base, base + 4 .. base + 36]))
  H.ensure (check sixteen exclusiveFrame == Left (FrameNotDerivable (Guest base)))
    "an exclusive double load left a stale frame position available to restore SP"
  loopSymbols <- either H.failH pure (F.parseSymbols "00001000 00000008 T growing\n")
  let growing = Image (Guest base) (B.concat (map U.putWord32 [0xe24dd008, 0xeafffffd]))
        loopSymbols (S.fromList (map Guest [base, base + 4]))
  H.ensure (check fitting growing == Left (FrameNotDerivable (Guest base)))
    "a stack-growing loop was assigned a finite frame"
  let balanced = single [0xe92d4800, 0xe24dd008, 0xe28dd008, 0xeafffffc]
  either (H.failH . show) pure (check sixteen balanced)
  H.ensure (check fifteen balanced == Left (BudgetExceeded (Guest base) sixteen))
    "a balanced loop hid its deepest stack offset"
  let variable = single [0xe72d0001, 0xe92d4800, 0xe8bd4800, 0xe12fff1e]
  H.ensure (check fitting variable == Left (FrameNotDerivable (Guest base)))
    "a register-sized SP adjustment was assigned a fixed frame"
  let flagMultiply = single [0xe92d4800, 0xe0120092, 0xe8bd4800, 0xe12fff1e]
  either (H.failH . show) pure (check eight flagMultiply)
  forM_ [0xe01d0092, 0xe012009f, 0xe0120f92, 0xe01f0092] $ \word -> do
    let changed = flagMultiply
          { imageBytes = B.take 4 (imageBytes flagMultiply) <> U.putWord32 word
              <> B.drop 8 (imageBytes flagMultiply) }
    H.ensure (check eight changed == Left (FrameNotDerivable (Guest base)))
      "MULS admitted an SP/PC destination or PC operand"
  forM_ [0x16655f90, 0xe6627f71] $ \parallel -> do
    let parallelArithmetic = single [0xe92d4800, parallel, 0xe8bd4800, 0xe12fff1e]
    either (H.failH . show) pure (check eight parallelArithmetic)
    H.ensure (check seven parallelArithmetic == Left (BudgetExceeded (Guest base) eight))
      "a parallel arithmetic operation hid the live frame"
  let parallelArithmetic = single [0xe92d4800, 0xe6627f71, 0xe8bd4800, 0xe12fff1e]
  forM_ [0xe662df71, 0xe66f7f71, 0xe6627f7f, 0xe6027f71, 0xe6427f71, 0xe6627fb1, 0xe6627fd1] $ \word -> do
    let changed = parallelArithmetic { imageBytes = B.take 4 (imageBytes parallelArithmetic) <> U.putWord32 word <> B.drop 8 (imageBytes parallelArithmetic) }
    H.ensure (check eight changed == Left (FrameNotDerivable (Guest base)))
      "a parallel arithmetic operation admitted an SP destination, PC operand or unallocated encoding"
  parallelFrameSymbols <- either H.failH pure (F.parseSymbols "00001000 00000014 T parallel\n")
  let parallelFrame = Image (Guest base)
        (B.concat (map U.putWord32 [0xe92d4800, 0xe1a0b00d, 0xe662bf71, 0xe1a0d00b, 0xe8bd8800]))
        parallelFrameSymbols (S.fromList (map Guest [base, base + 4 .. base + 16]))
  H.ensure (check eight parallelFrame == Left (FrameNotDerivable (Guest base)))
    "a parallel arithmetic operation left a stale frame pointer usable"
  let packHalf = single [0xe92d4800, 0xe6800810, 0xe8bd4800, 0xe12fff1e]
  either (H.failH . show) pure (check eight packHalf)
  H.ensure (check seven packHalf == Left (BudgetExceeded (Guest base) eight))
    "a packed-halfword operation hid the live frame"
  forM_ [0xe680f810, 0xe68f0810, 0xe680081f, 0xe680d810] $ \word -> do
    let changed = packHalf { imageBytes = B.take 4 (imageBytes packHalf) <> U.putWord32 word <> B.drop 8 (imageBytes packHalf) }
    H.ensure (check eight changed == Left (FrameNotDerivable (Guest base)))
      "a packed-halfword operation admitted an SP destination or PC operand"
  let packedFrame = parallelFrame
        { imageBytes = B.concat (map U.putWord32 [0xe92d4800, 0xe1a0b00d, 0xe680b810, 0xe1a0d00b, 0xe8bd8800]) }
  H.ensure (check eight packedFrame == Left (FrameNotDerivable (Guest base)))
    "a packed-halfword operation left a stale frame pointer usable"
  let bytePairs = single [0xe92d4800, 0xe6cf0070, 0xe8bd4800, 0xe12fff1e]
  either (H.failH . show) pure (check eight bytePairs)
  H.ensure (check seven bytePairs == Left (BudgetExceeded (Guest base) eight))
    "UXTB16 changed the exact live-frame budget"
  forM_ [0xe6cfd070, 0xe6cff070, 0xe6cf007f] $ \word -> do
    let changed = bytePairs
          { imageBytes = B.take 4 (imageBytes bytePairs) <> U.putWord32 word
              <> B.drop 8 (imageBytes bytePairs) }
    H.ensure (check eight changed == Left (FrameNotDerivable (Guest base)))
      "UXTB16 admitted an SP/PC destination or PC source"
  H.ensure
    (case A.decode base 0xe6cf0070 of
      A.ZeroExtend (A.Cond 14) (A.Reg 0) (A.Reg 0) 0 extension ->
        A.zeroExtendValue extension 0x00120034 == 0x00120034
      _ -> False)
    "UXTB16 collapsed its two zero-extended bytes into one scalar byte"
  forM_ [0xe59d0008, 0xe1cd00d8, 0xe5240008, 0xe4940008] $ \transfer -> do
    let framed = single [transfer, 0xe92d4800, 0xe8bd4800, 0xe12fff1e]
        leaf = single [transfer, 0xe12fff1e, 0xe320f000, 0xe320f000]
    either (H.failH . show) pure (check eight framed)
    H.ensure (check seven framed == Left (BudgetExceeded (Guest base) eight))
      ("a transfer preserving SP changed the frame budget: " ++ show transfer)
    either (H.failH . show) pure (check zero leaf)
  epilogueSymbols <- either H.failH pure (F.parseSymbols "00001000 0000001c T epilogue\n")
  let epilogueWords = [0xe92d4800, 0xe28db004, 0xe24dd010, 0xe3500000, 0x024bd004, 0x08bd8800, 0xeafffffe]
      epilogue = Image (Guest base) (B.concat (map U.putWord32 epilogueWords))
        epilogueSymbols (S.fromList (map Guest [base, base + 4 .. base + 24]))
  twentyFour <- maybe (H.failH "invalid epilogue budget") pure (stackBytes 24)
  twentyThree <- maybe (H.failH "invalid short epilogue budget") pure (stackBytes 23)
  either (H.failH . show) pure (check twentyFour epilogue)
  H.ensure (check twentyThree epilogue == Left (BudgetExceeded (Guest base) twentyFour))
    "a conditional return epilogue hid its live frame"
  forM_ [0x18bd8800, 0xe320f000] $ \changedWord -> do
    let changed = epilogue
          { imageBytes = B.take 20 (imageBytes epilogue) <> U.putWord32 changedWord <> B.drop 24 (imageBytes epilogue) }
    H.ensure (check twentyFour changed == Left (FrameNotDerivable (Guest base)))
      "a conditional stack restore without its matching return was admitted"
  flagRestoreSymbols <- either H.failH pure
    (F.parseSymbols "00001000 00000020 T flag_restore\n")
  let flagRestore = Image (Guest base) (B.concat (map U.putWord32
        [0xe92d4800, 0xe1a0b00d, 0xe24dd010, 0xe3500000,
         0x025bd000, 0x08bd8800, 0xe28dd018, 0xe12fff1e]))
        flagRestoreSymbols (S.fromList (map Guest [base, base + 4 .. base + 28]))
  H.ensure (check twentyFour flagRestore == Left (FrameNotDerivable (Guest base)))
    "a flag-setting conditional restore controlled its paired conditional return"
  peakSymbols <- either H.failH pure
    (F.parseSymbols "00001000 00000018 T taken_peak\n")
  let takenPeak = Image (Guest base) (B.concat (map U.putWord32
        [0xe92d4800, 0xe1a0b00d, 0xe28dd008, 0x024bd010, 0x049df018, 0xe12fff1e]))
        peakSymbols (S.fromList (map Guest [base, base + 4 .. base + 20]))
  either (H.failH . show) pure (check twentyFour takenPeak)
  H.ensure (check twentyThree takenPeak == Left (BudgetExceeded (Guest base) twentyFour))
    "a taken conditional restore hid its deepest stack offset"
  epilogueCallSymbols <- either H.failH pure
    (F.parseSymbols "00001000 00000020 T epilogue_call\n00001024 00000008 T halt\n")
  let epilogueCallWords =
        [0xe92d4800, 0xe28db004, 0xe24dd010, 0xe3500000, 0x024bd004, 0x08bd8800,
         0xe58d0000, 0xeb000000, 0, 0xe320f003, 0xeafffffd]
      epilogueCall = Image (Guest base) (B.concat (map U.putWord32 epilogueCallWords))
        epilogueCallSymbols (S.fromList (map Guest
          [base, base + 4, base + 8, base + 12, base + 16, base + 20, base + 24,
           base + 28, base + 36, base + 40]))
  either (H.failH . show) pure (check twentyFour epilogueCall)
  continuingSymbols <- either H.failH pure
    (F.parseSymbols "00001000 00000020 T continuing_epilogue\n")
  let continuingEpilogue = Image (Guest base) (B.concat (map U.putWord32
        [0xe92d4800, 0xe1a0b00d, 0xe3500000, 0x11a0d00b, 0x18bd8800,
         0xe59d0000, 0xe1a0d00b, 0xe8bd8800]))
        continuingSymbols (S.fromList (map Guest [base, base + 4 .. base + 28]))
  either (H.failH . show) pure (check eight continuingEpilogue)
  H.ensure (check seven continuingEpilogue == Left (BudgetExceeded (Guest base) eight))
    "a continuing conditional epilogue changed the exact frame budget"
  moveEpilogueSymbols <- either H.failH pure
    (F.parseSymbols "00001000 0000001c T move_epilogue\n00001020 00000008 T halt\n")
  let moveEpilogueWords =
        [0xe92d4ff8, 0xe28db020, 0xe3500000, 0x01a0d00b, 0x08bd8800,
         0xe3a00001, 0xeb000000, 0, 0xe320f001, 0xeafffffd]
      moveEpilogue = Image (Guest base) (B.concat (map U.putWord32 moveEpilogueWords))
        moveEpilogueSymbols (S.fromList (map Guest
          [base, base + 4, base + 8, base + 12, base + 16, base + 20, base + 24,
           base + 32, base + 36]))
  forty <- maybe (H.failH "invalid move epilogue budget") pure (stackBytes 40)
  thirtyNine <- maybe (H.failH "invalid short move epilogue budget") pure (stackBytes 39)
  either (H.failH . show) pure (check forty moveEpilogue)
  H.ensure (check thirtyNine moveEpilogue == Left (BudgetExceeded (Guest base) forty))
    "a conditional frame-pointer move epilogue hid its live frame"
  vfpSymbols <- either H.failH pure (F.parseSymbols "00001000 00000024 T vfp\n")
  let vfpWords =
        [0xe92d4800, 0xe1a0b00d, 0xed2d8b04, 0xed9f8a14, 0xee880a00,
         0xee101a10, 0xecbd8b04, 0xe1a0d00b, 0xe8bd8800]
      vfpFrame = Image (Guest base) (B.concat (map U.putWord32 vfpWords))
        vfpSymbols (S.fromList (map Guest [base, base + 4 .. base + 32]))
  either (H.failH . show) pure (check twentyFour vfpFrame)
  H.ensure (check twentyThree vfpFrame == Left (BudgetExceeded (Guest base) twentyFour))
    "VFP register saves were not included in the exact frame budget"
  forM_ [0xee10ba10, 0xee10da10] $ \transfer -> do
    let changed = vfpFrame
          { imageBytes = B.take 20 (imageBytes vfpFrame) <> U.putWord32 transfer
              <> B.drop 24 (imageBytes vfpFrame) }
    H.ensure (check twentyFour changed == Left (FrameNotDerivable (Guest base)))
      "a VFP transfer admitted an SP destination or a stale frame pointer"
  let vfpRegisterLoad = single [0xed940a00, 0xe12fff1e, 0xe320f000, 0xe320f000]
  either (H.failH . show) pure (check zero vfpRegisterLoad)
  selectedSymbols <- either H.failH pure
    (F.parseSymbols "00001000 00000008 T selected\n00001008 00000008 T unrelated\n")
  let selectedBody = B.concat (map U.putWord32 [0xe92d4800, 0xe8bd8800, 0xe92d4803, 0xe8bd8803])
      selected = Image (Guest base) selectedBody selectedSymbols
        (S.fromList (map Guest [base, base + 4 .. base + 12]))
  either (H.failH . show) pure (checkEntry eight (Guest base) selected)
  H.ensure (check eight selected == Left (BudgetExceeded (Guest (base + 8)) sixteen))
    "the whole-image check ignored a deep function outside the selected root"
  reachableSymbols <- either H.failH pure
    (F.parseSymbols "00001000 0000000c T selected\n0000100c 00000008 T reachable\n")
  let reachableBody = B.concat (map U.putWord32 [0xe92d4800, 0xeb000000, 0xe8bd8800, 0xe92d4803, 0xe8bd8803])
      reachable = Image (Guest base) reachableBody reachableSymbols
        (S.fromList (map Guest [base, base + 4 .. base + 16]))
  H.ensure (checkEntry sixteen (Guest base) reachable
      == Left (BudgetExceeded (Guest base) twentyFour))
    "the selected-root check ignored a deep reachable callee"
  zeroSizedSymbols <- either H.failH pure
    (F.parseSymbols "00001000 0000000c T caller\n0000100c 00000000 T assembly_leaf\n")
  let zeroSizedBody = B.concat (map U.putWord32
        [0xe92d4800, 0xeb000000, 0xe8bd8800, 0xe12fff1e])
      zeroSizedLeaf = Image (Guest base) zeroSizedBody zeroSizedSymbols
        (S.fromList (map Guest [base, base + 4 .. base + 12]))
  either (H.failH . show) pure (check eight zeroSizedLeaf)
  undersizedSymbols <- either H.failH pure
    (F.parseSymbols "00001000 00000008 T assembly_wrapper\n")
  let undersized = Image (Guest base)
        (B.concat (map U.putWord32 [0xe3a00001, 0xeaffffff, 0xe12fff1e]))
        undersizedSymbols (S.fromList (map Guest [base, base + 4, base + 8]))
  either (H.failH . show) pure (check zero undersized)
  sharedBodySymbols <- either H.failH pure
    (F.parseSymbols "00001000 00000008 T helper\n00001010 00000004 T caller\n")
  let sharedBody = Image (Guest base) (B.concat (map U.putWord32
        [0xe3a00001, 0xeaffffff, 0xe92d4800, 0xe8bd8800, 0xeafffffc]))
        sharedBodySymbols (S.fromList (map Guest [base, base + 4 .. base + 16]))
  either (H.failH . show) pure (check eight sharedBody)
  H.ensure (check seven sharedBody == Left (BudgetExceeded (Guest base) eight))
    "an unlabeled shared assembly body changed the exact frame budget"
  modeSymbols <- either H.failH pure (F.parseSymbols "00001000 00000000 T mode\n")
  let modeFunction instructions = Image (Guest base) (B.concat (map U.putWord32 instructions))
        modeSymbols (S.fromList
          (map Guest [base, base + 4 .. base + 4 * fromIntegral (length instructions - 1)]))
      saveIrq = modeFunction [0xe10f1000, 0xe3812080, 0xe121f002, 0xe2010080, 0xe12fff1e]
      restoreIrq = modeFunction [0xe2001080, 0xe10f2000, 0xe3c22080, 0xe1811002, 0xe121f001, 0xe12fff1e]
      slot164 = modeFunction
        [0xe92d4800, 0xe1a0b00d, 0xe10f0000, 0xe3801080, 0xe121f001, 0xe2000080, 0xe8bd8800]
      slot168 = modeFunction
        [0xe92d4800, 0xe1a0b00d, 0xe1a013a0, 0xe10f0000, 0xe7c70391, 0xe121f000, 0xe8bd8800]
  either (H.failH . show) pure (check zero saveIrq)
  either (H.failH . show) pure (check zero restoreIrq)
  either (H.failH . show) pure (check eight slot164)
  either (H.failH . show) pure (check eight slot168)
  let modeChanging = saveIrq
        { imageBytes = B.take 4 (imageBytes saveIrq) <> U.putWord32 0xe3812001
            <> B.drop 8 (imageBytes saveIrq) }
      lowInsert = slot168
        { imageBytes = B.take 16 (imageBytes slot168) <> U.putWord32 0xe7c70011
            <> B.drop 20 (imageBytes slot168) }
  H.ensure (check zero modeChanging == Left (FrameNotDerivable (Guest base)))
    "a control write that can change the processor mode was admitted"
  H.ensure (check eight lowInsert == Left (FrameNotDerivable (Guest base)))
    "a control write replacing processor-mode bits was admitted"
  let skippedStatus = modeFunction
        [0xea000001, 0xe10f1000, 0xe3812080, 0xe121f002, 0xe12fff1e]
  H.ensure (check zero skippedStatus == Left (FrameNotDerivable (Guest base)))
    "a branch directly into a status write borrowed skipped provenance"
  putStrLn "stack-proof: consecutive pushes, direct depth, recursion, frame and image-bound controls passed"
