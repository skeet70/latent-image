-- Merge and stub checks of FujiAF.UsbToggle over synthetic write sets and the
-- linked stub blob.
module Main (main) where

import Control.Monad (unless)
import qualified Data.ByteString as B
import Data.List (sort)
import FujiAF.UsbToggle
import System.Environment (getArgs)
import System.Exit (exitFailure)

check :: String -> Bool -> IO Bool
check name ok = do
  putStrLn ((if ok then "PASS " else "FAIL ") ++ name)
  pure ok

word :: String -> Int -> Write
word name a = Write WordWrite name "CC" "synthetic" (fromIntegral a) (B.pack [1, 2, 3, 4]) (B.pack [5, 6, 7, 8])

main :: IO ()
main = do
  [stubPath] <- getArgs
  stub <- B.readFile stubPath
  let extent = B.replicate extentLength 0xaa
      toggle = either error id (toggleWrites stub extent)
      base = [word "late" 0x00eab32c, word "early" 0x00d461ec]
      merged = mergeWrites base toggle
      addresses = either (const []) (map wAddress) merged
  results <-
    sequence
      [ check "two toggle writes: stub at the extent, hook word at 0x00ecf900" (map wAddress toggle == [extentStart, hookSite])
      , check "hook word branches to the extent" (map wNew (filter ((== WordWrite) . wKind) toggle) == [B.pack [0xd3, 0x92, 0xf4, 0xeb]])
      , check "stub current bytes are the extent prefix" (all (\w -> wKind w /= BlobWrite || wCurrent w == B.take (B.length stub) extent) toggle)
      , check "disjoint merge keeps every write in address order" (addresses == sort addresses && length addresses == 4)
      , check "a base write at the hook word is refused" (isLeft (mergeWrites [word "hook" (fromIntegral hookSite)] toggle))
      , check "a base write inside the extent is refused" (isLeft (mergeWrites [word "inside" (fromIntegral extentStart + 0x10)] toggle))
      , check "a base write ending at the extent start is kept" (isRight (mergeWrites [word "below" (fromIntegral extentStart - 4)] toggle))
      , check "an oversized stub is refused" (isLeft (toggleWrites (B.replicate (extentLength + 4) 0) extent))
      , check "linked stub calls exactly the four stock callees" (sort (uniq (stubBlTargets stub)) == sort stubCallees)
      , check "linked stub fits the extent" (B.length stub <= extentLength && B.length stub `mod` 4 == 0)
      ]
  unless (and results) exitFailure
  where
    isLeft = either (const True) (const False)
    isRight = not . isLeft
    uniq = foldr (\x acc -> if x `elem` acc then acc else x : acc) []
