{-# LANGUAGE OverloadedStrings #-}
module Main (main) where

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
  tailSymbols <- either H.failH pure (F.parseSymbols "00001000 00000004 T first\n00001004 00000004 T second\n")
  let tails = halt { imageBytes = B.concat (map U.putWord32 [0xeaffffff, 0xeafffffd]), imageSymbols = tailSymbols }
  either (H.failH . show) pure (check zero tails)
  let tailThenCall = tails { imageBytes = B.concat (map U.putWord32 [0xeaffffff, 0xebfffffd]) }
  H.ensure (check zero tailThenCall == Left (RecursiveCall (Guest (base + 4)) (Guest base))) "call growth through a tail edge was admitted"
  let earlyWords = [0xe3500000, 0x112fff1e, 0xe92d4800, 0xe24dd010, 0xeb000001, 0xe28dd010, 0xe8bd8800, 0xe92d4800, 0xe8bd8800]
      earlyBody = B.concat (map U.putWord32 earlyWords)
  earlySymbols <- either H.failH pure (F.parseSymbols "00001000 0000001c T early\n0000101c 00000008 T callee\n")
  let early = Image (Guest base) earlyBody earlySymbols (S.fromList (map Guest [base, base + 4 .. base + 32]))
  exact <- maybe (H.failH "invalid frame budget") pure (stackBytes 32)
  under <- maybe (H.failH "invalid frame budget") pure (stackBytes 31)
  either (H.failH . show) pure (check exact early)
  H.ensure (check under early == Left (BudgetExceeded (Guest base) exact)) "early return hid the allocated frame or callee depth"
  let skipsFrame = early { imageBytes = B.take 4 earlyBody <> U.putWord32 0x1a000004 <> B.drop 8 earlyBody }
  H.ensure (check exact skipsFrame == Left (FrameNotDerivable (Guest base))) "a branch past frame allocation was treated as a leaf return"
  putStrLn "stack-proof: consecutive pushes, direct depth, recursion, frame and image-bound controls passed"
