{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}
module Main where

import Control.Monad (forM_, unless)
import qualified Data.ByteString as B
import Data.Word (Word32)
import Emulation.NativeObservation (GuestAddress (..), NativeObservation (..), Register (..), RegionId (..), RunnerStatus (..), Write (..), provenanceWrites)
import Emulation.OwnedCase
import System.Environment (getArgs)
import System.FilePath ((</>))

newtype Cursor = Cursor Word32
newtype Kind = Kind Word32

block :: Cursor -> B.ByteString
block (Cursor cursor) = assemble [word offset | offset <- [0,4..0x42c]]
 where
  word :: Int -> Word32
  word offset = case offset of
    0 -> 0x77c0f0
    4 -> 0x15b
    8 -> cursor
    0x18 -> 4
    0x424 -> 1
    0x42c -> 1
    _ -> 0

main :: IO ()
main = do
  [runner, image4Path, image5Path, out] <- getArgs
  image4 <- B.readFile image4Path
  image5 <- B.readFile image5Path
  let nodes = B.take 0x20000 (B.drop (0x776000 - 0x61c400) image4)
      code = B.take (0xbfad04 - 0xbfa234) (B.drop (0xbfa234 - 0xbf1a00) image5)
  unless (readWordAt nodes (0x77c0f0 - 0x776000 + 0x1c) == 0) (fail "probe node has a parent")
  let values = readWordAt nodes (0x77c0f0 - 0x776000 + 0x20)
  forM_ [0,4] $ \cursor -> unless (readWordAt nodes (fromIntegral values - 0x776000 + 8 * cursor + 4) == 0) (fail "probe row has a child")
  forM_ [("no-child", Kind 0xe, Cursor 0), ("no-child-terminator", Kind 0xe, Cursor 4), ("no-parent", Kind 0xf, Cursor 0)] $ \(name, Kind kind, cursor) ->
    forM_ [("dispatch",0xbfa234), ("switch",0xbfab98)] $ \(seam, entry) -> do
      let ident = name ++ "-" ++ seam
          before = block cursor
          spec = CaseSpec
            { caseId = ident
            , caseProvenance = "I6 injected-state probe; X-M1 1.50 image-5 dispatch/change-level bytes; image-4 node 0x77c0f0, null parent and null row child; expected zero return without calls or block writes from id5.dis 0xbfabd0..0xbfabdc and 0xbfac30..0xbfac50"
            , caseExclusion = "Prepared state, not key-reachable acceptance; no peripheral or scheduler execution"
            , caseEntry = GuestAddress entry
            , caseExit = GuestAddress 0x958000
            , caseRegions =
                [ mkRegion (RegionId "code") (GuestAddress 0xbfa234) (B.length code) ReadExecute code
                , mkRegion (RegionId "nodes") (GuestAddress 0x776000) (B.length nodes) ReadOnly nodes
                , mkRegion (RegionId "block") (GuestAddress 0x9571d0) 0x430 ReadWrite before
                , mkRegion (RegionId "exit") (GuestAddress 0x958000) 4 ReadExecute (assemble [0xe12fff1e])
                , mkRegion (RegionId "stack") (GuestAddress 0x959000) 0x1000 ReadWrite (B.replicate 0x1000 0x5a)
                ]
            , caseFunctions = [(GuestAddress 0xbfa234, 0x5cc), (GuestAddress 0xbfab98, 0x16c)]
            , caseInitial = [(register, value register) | register <- [R0,R1,R2,R3,R4,R5,R6,R7,R8,R9,R10,R11,R12,Sp,Lr,Cpsr]]
            , caseExpectedStatus = Returned
            , caseExpectedRegisters = [(R0,0),(Sp,0x959ff0)]
            , caseExpectedMemory = []
            , caseIgnoredMemory = []
            , caseCp14Identity = Nothing
            , caseKernel = Nothing
            }
          value register = case register of
            R0 -> kind
            R1 -> 0x10
            R2 -> if seam == "dispatch" then 0x1d else 0x15
            R3 -> if seam == "dispatch" then 0 else 0x1d
            Sp -> 0x959ff0
            Lr -> 0x958000
            Cpsr -> 0x10
            _ -> 0
      path <- writeCase (out </> ident) spec
      observed <- loadObserved runner path
      unless (nativeStatus observed == Returned && registerValue observed R0 == 0) (fail (ident ++ ": expected returned zero"))
      unless (maybe False (all (\write -> writeAddress write >= GuestAddress 0x959000 && writeAddress write < GuestAddress 0x95a000)) (provenanceWrites (nativeProvenance observed))) (fail (ident ++ ": non-stack write or truncated log"))
      unless (nativeBlock observed == before) (fail (ident ++ ": block changed"))
      putStrLn (ident ++ ": returned 0, block unchanged, no external call")
 where
  nativeBlock observed = regionBytesOf observed (RegionId "block")
