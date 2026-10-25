-- | card-with-files: copies a card image to a new path and writes named files
-- into the copy's FAT partition with mtools. The input image is never opened
-- for writing.
--
--   card-with-files INPUT OUTPUT [--offset BYTES] FILE...
--
-- OUTPUT must not exist and must differ from INPUT. --offset is the byte
-- offset of the FAT partition in the image (0 for a superfloppy). Each FILE
-- lands in the FAT root under its base name; the tool reads it back with
-- mtype, compares the bytes and lists the root with mdir.
module Main (main) where

import Control.Monad (filterM, forM_, unless, when)
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C8
import qualified Data.ByteString.Lazy as BL
import Data.Char (isDigit, isHexDigit, toLower)
import System.Directory (canonicalizePath, doesFileExist)
import System.Environment (getArgs, getExecutablePath)
import System.Exit (ExitCode (..), die)
import System.FilePath ((</>), takeFileName)
import System.Process (proc, readCreateProcessWithExitCode)

usage :: String
usage =
  unlines
    [ "usage: card-with-files INPUT OUTPUT [--offset BYTES] FILE..."
    , "  copies INPUT to a new OUTPUT and writes each FILE into the card's FAT root"
    , "  with mtools; --offset is the FAT partition's byte offset (default 0)"
    ]

main :: IO ()
main = do
  arguments <- getArgs
  (input, output, offset, files) <- case arguments of
    (a : b : rest) -> pure (a, b, 0, rest)
    _ -> die usage
  (inputPath, outputPath, partitionOffset, filePaths) <- parseTail input output offset files
  same <- (==) <$> canonicalizePath inputPath <*> canonicalizePath outputPath
  when same $ die "card-with-files: OUTPUT must differ from INPUT"
  exists <- doesFileExist outputPath
  when exists $ die ("card-with-files: " ++ outputPath ++ " already exists")
  when (null filePaths) $ die "card-with-files: no file to write"
  forM_ filePaths $ \path -> do
    present <- doesFileExist path
    unless present $ die ("card-with-files: missing input file " ++ path)
  BL.readFile inputPath >>= BL.writeFile outputPath
  executable <- getExecutablePath
  mcopy <- toolPath executable "mcopy"
  mtype <- toolPath executable "mtype"
  mdir <- toolPath executable "mdir"
  let image = outputPath ++ "@@" ++ show partitionOffset
  forM_ filePaths $ \path -> do
    let member = takeFileName path
        runtime = ["-i", image, "::/" ++ member]
    runTool "mcopy" mcopy ["-o", "-i", image, path, "::/" ++ member]
    (code, readBack, err) <- readCreateProcessWithExitCode (proc mtype runtime) ""
    when (code /= ExitSuccess) $
      die ("card-with-files: mtype " ++ member ++ " failed: " ++ err)
    source <- B.readFile path
    unless (C8.pack readBack == source) $
      die ("card-with-files: " ++ member ++ " does not read back byte for byte")
    putStrLn ("wrote " ++ member ++ " (" ++ show (B.length source) ++ " bytes)")
  (code, listing, err) <- readCreateProcessWithExitCode (proc mdir ["-i", image, "::/"]) ""
  when (code /= ExitSuccess) $ die ("card-with-files: mdir failed: " ++ err)
  putStrLn listing
  putStrLn ("copied " ++ inputPath ++ " to " ++ outputPath ++ "; partition offset " ++ show partitionOffset)

parseTail :: FilePath -> FilePath -> Int -> [String] -> IO (FilePath, FilePath, Int, [FilePath])
parseTail input output offset rest = case rest of
  ("--offset" : value : files) -> do
    parsed <- parseOffset value
    pure (input, output, parsed, files)
  ("--offset" : _) -> die "card-with-files: --offset needs a value"
  files -> pure (input, output, offset, files)

parseOffset :: String -> IO Int
parseOffset text = case text of
  ('0' : 'x' : digits)
    | not (null digits) && all isHexDigit digits -> pure (readBase 16 digits)
  _ | not (null text) && all isDigit text -> pure (read text)
  _ -> die ("card-with-files: bad offset " ++ show text)
 where
  readBase base = foldl (\acc c -> acc * base + digit c) 0
  digit c
    | isDigit c = fromEnum c - fromEnum '0'
    | toLower c >= 'a' && toLower c <= 'f' = 10 + fromEnum (toLower c) - fromEnum 'a'
    | otherwise = 0

toolPath :: FilePath -> String -> IO FilePath
toolPath executable name = do
  let runfiles = executable ++ ".runfiles"
      candidates =
        [ runfiles </> "lab_mtools" </> "bin" </> name
        , runfiles </> "_main" </> "lab_mtools" </> "bin" </> name
        ]
  present <- filterM doesFileExist candidates
  pure (case present of (path : _) -> path; [] -> name)

runTool :: String -> FilePath -> [String] -> IO ()
runTool label executable arguments = do
  (code, _, err) <- readCreateProcessWithExitCode (proc executable arguments) ""
  case code of
    ExitSuccess -> pure ()
    _ -> die ("card-with-files: " ++ label ++ " failed: " ++ err)
