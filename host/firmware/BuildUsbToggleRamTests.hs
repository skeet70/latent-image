module Main where

import Control.Monad (forM_, unless)
import Data.Bits (xor)
import qualified Data.ByteString as B
import Data.Either (isLeft)
import Data.Word (Word32)
import FujiAF.ClassicChrome (Address (..))
import System.Environment (getArgs)
import System.Exit (exitFailure)
import System.Process (readProcess)
import UsbToggleRam

check :: String -> Bool -> IO ()
check label passed = unless passed $ putStrLn ("FAIL: " ++ label) >> exitFailure

main :: IO ()
main = do
  args <- getArgs
  (stubPath, builder) <- case args of
    [stub, binary] -> pure (stub, binary)
    _ -> fail "expected STUB BUILDER"
  abi <- readProcess builder ["--abi"] ""
  check "builder reports bounded publication ABI" (abi == "23 22 1 6\n")
  bytes <- B.readFile stubPath
  stub <- either fail pure (reviewedStub bytes)
  -- Synthetic original extent contains byte values 0 through 143.
  let oldBytes = B.pack [0 .. 143]
  extent <- either fail pure (extentWords oldBytes)
  forM_ [0x00a7e8c8, 0x00c20000, 0x00c21f88] $ \base -> do
    plan <- either fail pure (publications (Address base) stub extent)
    let table = publicationTable plan
        field index offset = B.take 4 (B.drop (16 * index + offset) table)
        setupOld = B.concat [field index 4 | index <- [0 .. 21]]
        setupNew = B.concat [field index 8 | index <- [0 .. 21]]
    check "exactly 23 publication records" (B.length table == 23 * 16)
    check "setup writes preserve the original 22 words" (setupOld == B.take 88 oldBytes)
    check "setup writes reproduce the reviewed stub verbatim" (setupNew == bytes)
    check "setup addresses ascend through the fixed stub" (map publicationAddress (take 22 plan) == map Address [0x00bf4454, 0x00bf4458 .. 0x00bf44a8])
    check "setup records precede activation" (all (== B.pack [0, 0, 0, 0]) [field index 12 | index <- [0 .. 21]])
    check "activation publishes only the reviewed hook word"
      (B.drop (22 * 16) table == B.pack [0x00,0xf9,0xec,0x00,0xfd,0x3f,0xdf,0xeb,0xd3,0x92,0xf4,0xeb,1,0,0,0])
    check "runtime allocation contains aligned installer" (let Address start = installerStart (Address base) in start `mod` 32 == 0 && start >= base && toInteger start + 0xd00 <= toInteger base + toInteger allocationSize)
  forM_ [0, 8 .. 24] $ \offset -> do
    let base = 0x00c20000 + offset
        Address start = installerStart (Address base)
    check "allocation contains every eight-byte alignment" (start + 0xd00 <= base + allocationSize)
  forM_ [0x00bf4450, 0x00ecf900, 0x00c20001, 0xfffffff8] $ \base ->
    check "unsafe allocation is refused" (isLeft (publications (Address base) stub extent))
  forM_ [0 .. B.length bytes - 1] $ \offset -> do
    let changed = B.take offset bytes <> B.singleton (B.index bytes offset `xor` 1) <> B.drop (offset + 1) bytes
    check "every single-byte stub mutation is refused" (isLeft (reviewedStub changed))
  check "truncated stub is refused" (isLeft (reviewedStub (B.take 84 bytes)))
  check "extended stub is refused" (isLeft (reviewedStub (bytes <> B.replicate 4 0)))
  check "unreviewed stock image is refused" (isLeft (stockExtent (B.replicate 0x300000 0)))
  check "modified unreviewed stock image is refused" (isLeft (stockExtent (B.singleton 1 <> B.replicate 0x2fffff 0)))
  check "truncated original extent is refused" (isLeft (extentWords (B.take 140 oldBytes)))
  check "oversized original extent is refused" (isLeft (extentWords (oldBytes <> B.replicate 4 0)))
  check "publication old/new words retain little-endian meaning"
    (publicationTable [Publication (Address 0x11223344) (0x55667788 :: Word32) 0x99aabbcc Setup]
      == B.pack [0x44,0x33,0x22,0x11,0x88,0x77,0x66,0x55,0xcc,0xbb,0xaa,0x99,0,0,0,0])
  putStrLn "USB toggle RAM builder: checks passed"
