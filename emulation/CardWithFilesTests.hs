{-# LANGUAGE OverloadedStrings #-}
-- | Behavioural checks for card-with-files: the input image is unchanged, the
-- written files read back from the output with mdir/mtype, and an output equal
-- to the input is refused.
module Main (main) where

import Control.Monad (filterM, unless)
import qualified Crypto.Hash.SHA256 as SHA256
import qualified Data.ByteString as B
import Data.List (isInfixOf)
import System.Directory (createDirectory, doesFileExist, getTemporaryDirectory, makeAbsolute, removeDirectoryRecursive, removeFile)
import System.Environment (getArgs, getExecutablePath)
import System.Exit (ExitCode (..), exitFailure)
import System.FilePath ((</>))
import System.IO (IOMode (WriteMode), hClose, hSetFileSize, openBinaryFile, openTempFile)
import System.Process (proc, readCreateProcessWithExitCode)

check :: String -> Bool -> IO ()
check label passed = unless passed (putStrLn ("FAIL: " ++ label) >> exitFailure)

digest :: FilePath -> IO B.ByteString
digest path = SHA256.hash <$> B.readFile path

-- | A Nix tool from this test's runfiles, or the name on PATH.
toolFor :: String -> IO FilePath
toolFor name = do
  executable <- getExecutablePath
  let runfiles = executable ++ ".runfiles"
      repository = if name == "mkfs.vfat" then "lab_dosfstools" else "lab_mtools"
      candidates =
        [ runfiles </> repository </> "bin" </> name
        , runfiles </> "_main" </> repository </> "bin" </> name
        ]
  present <- filterM doesFileExist candidates
  pure (case present of (path : _) -> path; [] -> name)

main :: IO ()
main = do
  [toolArg] <- getArgs
  tool <- makeAbsolute toolArg
  mkfs <- toolFor "mkfs.vfat"
  mdir <- toolFor "mdir"
  mtype <- toolFor "mtype"
  temporary <- getTemporaryDirectory
  (root, handle) <- openTempFile temporary "card-with-files-test"
  hClose handle
  removeFile root
  createDirectory root
  let image = root </> "card.img"
      copy = root </> "copy.img"
      one = root </> "one.txt"
      two = root </> "two.txt"
  handle' <- openBinaryFile image WriteMode
  hSetFileSize handle' (64 * 1024 * 1024)
  hClose handle'
  (mkfsCode, _, mkfsErr) <- readCreateProcessWithExitCode (proc mkfs ["-F", "32", image]) ""
  check ("mkfs.vfat makes the fixture card: " ++ mkfsErr) (mkfsCode == ExitSuccess)
  B.writeFile one "first file\n"
  B.writeFile two "second file\n"
  before <- digest image
  (code, out, err) <- readCreateProcessWithExitCode (proc tool [image, copy, one, two]) ""
  check ("card-with-files copies and writes: " ++ err ++ out) (code == ExitSuccess)
  after <- digest image
  check "the input image is unchanged" (before == after)
  (listCode, listing, listErr) <- readCreateProcessWithExitCode (proc mdir ["-i", copy, "::/"]) ""
  check ("mdir lists the copied card: " ++ listErr) (listCode == ExitSuccess)
  check ("one.txt is in the FAT root: " ++ listing) ("one" `isInfixOf` listing && "two" `isInfixOf` listing)
  (readCode, body, readErr) <- readCreateProcessWithExitCode (proc mtype ["-i", copy, "::/one.txt"]) ""
  check ("mtype reads the written file: " ++ readErr) (readCode == ExitSuccess)
  check "the written file reads back byte for byte" (body == "first file\n")
  (refused, _, refuseErr) <- readCreateProcessWithExitCode (proc tool [image, image, one]) ""
  check ("an output equal to the input is refused: " ++ refuseErr) (refused /= ExitSuccess && "differ" `isInfixOf` refuseErr)
  removeDirectoryRecursive root
  putStrLn "card-with-files: checks passed"
