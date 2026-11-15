{-# LANGUAGE OverloadedStrings #-}
module Main (main) where

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
  forM_ [0xe59d0008, 0xe1cd00d8, 0xe5240008, 0xe4940008] $ \transfer -> do
    let framed = single [transfer, 0xe92d4800, 0xe8bd4800, 0xe12fff1e]
        leaf = single [transfer, 0xe12fff1e, 0xe320f000, 0xe320f000]
    either (H.failH . show) pure (check eight framed)
    H.ensure (check seven framed == Left (BudgetExceeded (Guest base) eight))
      ("a transfer preserving SP changed the frame budget: " ++ show transfer)
    either (H.failH . show) pure (check zero leaf)
  putStrLn "stack-proof: consecutive pushes, direct depth, recursion, frame and image-bound controls passed"
