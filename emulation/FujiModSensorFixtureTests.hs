{-# LANGUAGE OverloadedStrings #-}
-- | Behavioural checks for fujimod-xm1-sensor-fixture through its CLI.
module Main (main) where

import Control.Monad (unless)
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C8
import System.Directory (createDirectory, doesPathExist, getTemporaryDirectory, makeAbsolute, removeDirectoryRecursive, removeFile)
import System.Environment (getArgs)
import System.Exit (ExitCode (..), exitFailure)
import System.FilePath ((</>))
import System.IO (hClose, openTempFile)
import System.Process (proc, readCreateProcessWithExitCode)

check :: String -> Bool -> IO ()
check label passed = unless passed (putStrLn ("FAIL: " ++ label) >> exitFailure)

base :: B.ByteString
base =
  "{\"devices\":{\"nodes\":[{\"name\":\"image_input\",\"model\":{\"kind\":\"image-input\",\"sensor\":null,\
  \\"frame_period\":null,\"raw_irq\":{\"core\":0,\"line\":36},\"options\":{\"sensor_period_ticks\":null,\
  \\"clock\":{\"source\":{\"kind\":\"periodic\",\"period_ticks\":400000}},\
  \\"cfa\":[0,1,2,0,1,2,0,1,2,0,1,2,0,1,2,0,1,2,0,1,2,0,1,2,0,1,2,0,1,2,0,1,2,0,1,2]}}}]},\
  \\"scene\":{},\"generated_assets\":[]}"

fixture :: Int -> Int -> B.ByteString
fixture width height =
  "{\"schema\":1,\"id\":\"test\",\"image_input\":\"image_input\",\"frames\":\"frames\",\"asset\":\"asset\",\
  \\"raster\":{\"width\":" <> C8.pack (show width) <> ",\"height\":" <> C8.pack (show height) <> "},\
  \\"scene\":\"raw16-index-ramp12\",\"sensor_period_ticks\":400000,\
  \\"response\":{\"black\":0,\"full_scale\":4095,\"reference_white\":[4095,4095,4095]}}"

main :: IO ()
main = do
  [toolArg] <- getArgs
  tool <- makeAbsolute toolArg
  temporary <- getTemporaryDirectory
  (root, handle) <- openTempFile temporary "sensor-fixture-test"
  hClose handle
  removeFile root
  createDirectory root
  let run = root </> "base.json"
      valid = root </> "valid.json"
      oversized = root </> "oversized.json"
      derived = root </> "derived.json"
      altered = root </> "altered.json"
      refused = root </> "refused.json"
  B.writeFile run base
  B.writeFile valid (fixture 4096 2)
  B.writeFile oversized (fixture 4611686018427387904 2)
  (code, out, err) <- readCreateProcessWithExitCode (proc tool ["write", "--run", run, "--fixture", valid, "--output", derived]) ""
  check ("a 4096 x 2 raster derives: " ++ err) (code == ExitSuccess)
  (verified, verifiedOut, verifyErr) <- readCreateProcessWithExitCode (proc tool ["verify", "--run", run, "--fixture", valid, "--document", derived]) ""
  check ("the derived document verifies: " ++ verifyErr) (verified == ExitSuccess && verifiedOut == out)
  bytes <- B.readFile derived
  B.writeFile altered (B.snoc bytes 10)
  (mismatch, mismatchOut, mismatchErr) <- readCreateProcessWithExitCode (proc tool ["verify", "--run", run, "--fixture", valid, "--document", altered]) ""
  check ("a document differing by one byte is refused: " ++ mismatchErr) (mismatch /= ExitSuccess && null mismatchOut)
  (overflow, overflowOut, overflowErr) <- readCreateProcessWithExitCode (proc tool ["write", "--run", run, "--fixture", oversized, "--output", refused]) ""
  written <- doesPathExist refused
  check ("a raster whose byte count overflows Int is refused: " ++ overflowErr) (overflow /= ExitSuccess && null overflowOut && not written)
  removeDirectoryRecursive root
  putStrLn "fujimod-xm1-sensor-fixture: checks passed"
