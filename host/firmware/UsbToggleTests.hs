-- Merge, stub and step-assembly checks of FujiAF.UsbToggle over synthetic
-- write sets, synthetic base steps and the linked stub blob.
module Main (main) where

import Control.Monad (unless)
import qualified Crypto.Hash.SHA256 as SHA256
import Data.Aeson (Value)
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as BC
import Data.List (sort)
import qualified FujiAF.Host as H
import FujiAF.UsbToggle
import Numeric (showHex)
import System.Environment (getArgs)
import System.Exit (exitFailure)

check :: String -> Bool -> IO Bool
check name ok = do
  putStrLn ((if ok then "PASS " else "FAIL ") ++ name)
  pure ok

word :: String -> Int -> Write
word name a = Write WordWrite name "CC" "synthetic" (fromIntegral a) (B.pack [1, 2, 3, 4]) (B.pack [5, 6, 7, 8])

hex :: B.ByteString -> String
hex = concatMap (\x -> let d = showHex x "" in replicate (2 - length d) '0' ++ d) . B.unpack . SHA256.hash

-- | A file-bearing step and its bytes.
fileStep :: String -> FilePath -> B.ByteString -> (Value, Maybe B.ByteString)
fileStep kind path bytes = (H.obj [("kind", H.s kind), ("path", H.s path), ("sha256", H.s (hex bytes))], Just bytes)

imageSet :: Integer -> B.ByteString
imageSet segment = BC.pack ("{\"image\":{\"segment_id\":" ++ show segment ++ "}}")

bind :: [(Value, Maybe B.ByteString)] -> Either String [BaseStep]
bind = mapM (uncurry baseStep)

main :: IO ()
main = do
  [stubPath] <- getArgs
  stub <- B.readFile stubPath
  let extent = B.replicate extentLength 0xaa
      toggle = either error id (toggleWrites stub extent)
      base = [word "late" 0x00eab32c, word "early" 0x00d461ec]
      merged = mergeWrites base toggle
      addresses = either (const []) (map wAddress) merged
      image4 = fileStep "image-patch-set" "parts/a-image4.json" (imageSet 4)
      image5 = fileStep "image-patch-set" "parts/a-image5.json" (imageSet 5)
      record = (H.obj [("kind", H.s "image2-record"), ("id", H.s "0x83d")], Nothing)
      image2 = fileStep "image2-patch-set" "parts/a-image2.json" (BC.pack "image2")
      font = fileStep "font-patch-set" "../other/a-font.json" (BC.pack "font")
      plain = fileStep "plain-patch-set" "a-plain.json" (BC.pack "plain")
      merged5 = H.obj [("kind", H.s "image-patch-set"), ("path", H.s "usb-toggle-image5.json"), ("sha256", H.s "merged")]
      assembled = bind [record, image4, image5, image2, font, plain] >>= assembleSteps merged5
      outSteps = either (const []) fst assembled
      outFiles = either (const []) snd assembled
      field k v = H.str (H.get v k)
  results <-
    sequence
      [ check "two toggle writes: stub at the extent, hook word at 0x00ecf900" (map wAddress toggle == [extentStart, hookSite])
      , check "hook word branches to the extent" (map wNew (filter ((== WordWrite) . wKind) toggle) == [B.pack [0xd3, 0x92, 0xf4, 0xeb]])
      , check "stub current bytes are the extent prefix" (all (\w -> wKind w /= BlobWrite || wCurrent w == B.take (B.length stub) extent) toggle)
      , check "disjoint merge keeps every write in address order" (addresses == sort addresses && length addresses == 4)
      , check "a base write at the hook word is refused" (isLeft (mergeWrites [word "hook" (fromIntegral hookSite)] toggle))
      , check "a base write inside the extent is refused" (isLeft (mergeWrites [word "inside" (fromIntegral extentStart + 0x10)] toggle))
      , check "a base write at 0x00bf44ac, past the stub in the extent, is refused" (isLeft (mergeWrites [word "tail" 0x00bf44ac] toggle))
      , check "a base write at the extent's last word is refused" (isLeft (mergeWrites [word "last" 0x00bf44e0] toggle))
      , check "a base write ending at the extent start is kept" (isRight (mergeWrites [word "below" (fromIntegral extentStart - 4)] toggle))
      , check "a base write at the extent end is kept" (isRight (mergeWrites [word "above" 0x00bf44e4] toggle))
      , check "an oversized stub is refused" (isLeft (toggleWrites (B.replicate (extentLength + 4) 0) extent))
      , check "linked stub calls exactly the four stock callees" (sort (uniq (stubBlTargets stub)) == sort stubCallees)
      , check "linked stub fits the extent" (B.length stub <= extentLength && B.length stub `mod` 4 == 0)
      , check "steps: image 4, merged image 5, then the rest in base order" (map (field "kind") outSteps == ["image-patch-set", "image-patch-set", "image2-record", "image2-patch-set", "font-patch-set", "plain-patch-set"] && field "sha256" (outSteps !! 1) == "merged")
      , check "steps: the base image-5 set is dropped" (all ((/= "parts/a-image5.json") . field "path") outSteps && notElem "a-image5.json" (map fst outFiles))
      , check "steps: every file step is copied under its file name, byte-equal" (outFiles == [("a-image4.json", imageSet 4), ("a-image2.json", BC.pack "image2"), ("a-font.json", BC.pack "font"), ("a-plain.json", BC.pack "plain")])
      , check "steps: file steps name their copy and keep their SHA-256" (map (field "path") outSteps == ["a-image4.json", "usb-toggle-image5.json", "", "a-image2.json", "a-font.json", "a-plain.json"] && field "sha256" (outSteps !! 3) == hex (BC.pack "image2"))
      , check "steps: a path-less record passes through unchanged" (outSteps !! 2 == fst record)
      , check "steps: a file that differs from its step SHA-256 is refused" (isLeft (baseStep (fst image2) (Just (BC.pack "other"))))
      , check "steps: a file step without its bytes is refused" (isLeft (baseStep (fst plain) Nothing))
      , check "steps: two base files with one file name are refused" (isLeft (bind [image2, fileStep "font-patch-set" "elsewhere/a-image2.json" (BC.pack "x")] >>= assembleSteps merged5))
      , check "steps: a base file named like a builder output is refused" (isLeft (bind [fileStep "plain-patch-set" "x/usb-toggle.json" (BC.pack "x")] >>= assembleSteps merged5))
      ]
  unless (and results) exitFailure
  where
    isLeft = either (const True) (const False)
    isRight = not . isLeft
    uniq = foldr (\x acc -> if x `elem` acc then acc else x : acc) []
