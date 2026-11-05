module Main (main) where

import qualified Crypto.Hash.SHA256 as SHA
import qualified Data.ByteString as B
import Numeric (showHex)
import ReferenceScan
import System.Environment (getArgs)
import System.Exit (die)
import System.FilePath ((</>))

main :: IO ()
main = do
  args <- getArgs
  dir <- case args of
    [path] -> pure path
    _ -> die "usage: reference-scan LZ_DIRECTORY"
  image4 <- pinned dir "id4-0061c400.bin" WholeImage 2631680 "f2e2acab892dafeed85171f6870a411f0a2d5eae10921b86a24630d401dcff16"
  image5 <- pinned dir "id5-00bf1a00.bin" WholeImage 5185120 "3cd501c07533938ae4d9347616986fd1649005a588c36b5abf05e36ef4fd892a"
  overlay44 <- pinned dir "plain-01100000.bin" (OverlaySlice 0) 0xac5a0 "2dfaa5d4bd025d68e991a329cc1377d5da16de656148dc4027d9a6bc0c349f5c"
  overlay46 <- pinned dir "plain-01100000.bin" (OverlaySlice 0xc0000) 0x1cca0 "a9dee59faaec1765d9f3a2dd7f75fd7d423a8b8061599b5d7f6752b6bdd4cf84"
  overlay47 <- pinned dir "plain-01100000.bin" (OverlaySlice 0xe0000) 0x26a5e0 "0cf67b7221c3b0755092da0ae4c6d6fb817f2b32771ee53dff691fc59b28770c"
  let refs = concatMap (scanReferences engineTargets)
        [Image "image4" (Address 0x61c400) image4, Image "image5" (Address 0xbf1a00) image5
        ,Image "overlay44" (Address 0x1100000) overlay44, Image "overlay46" (Address 0x11c0000) overlay46
        ,Image "overlay47" (Address 0x11e0000) overlay47]
  mapM_ (emit refs) engineTargets
  where
    emit refs target@(Address address) = do
      let matches = map renderReference (filter ((== target) . referenceTarget) refs)
          hex = showHex address ""
      putStrLn ("0x" ++ replicate (8 - length hex) '0' ++ hex ++ "\t" ++ if null matches then "-" else unwords matches)

data InputExtent = WholeImage | OverlaySlice Int deriving (Show)

pinned :: FilePath -> FilePath -> InputExtent -> Int -> String -> IO B.ByteString
pinned dir file extent size expected = do
  bytes <- B.readFile (dir </> file)
  let slice = case extent of
        WholeImage -> bytes
        OverlaySlice offset -> B.take size (B.drop offset bytes)
      actual = concatMap (\b -> let h = showHex b "" in replicate (2 - length h) '0' ++ h) (B.unpack (SHA.hash slice))
  if B.length slice == size && actual == expected
    then pure slice
    else die (file ++ " " ++ show extent ++ ": SHA-256/size mismatch, got " ++ actual)
