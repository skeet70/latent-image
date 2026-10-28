-- | build-recipe-package refuses before it writes anything: without an output
-- directory, with one outside the RC completion directory, and when an input
-- is missing. No tool runs and no firmware is read.
module Main where

import Control.Monad (unless)
import Data.List (isInfixOf)
import System.Directory (createDirectory, doesPathExist, getTemporaryDirectory, makeAbsolute, removeDirectoryRecursive, removeFile)
import System.Environment (getArgs, getEnvironment)
import System.Exit (ExitCode (..), exitFailure)
import System.FilePath ((</>))
import System.IO (hClose, openTempFile)
import System.Process (CreateProcess (..), proc, readCreateProcessWithExitCode)

check :: String -> Bool -> IO ()
check label passed = unless passed (putStrLn ("FAIL: " ++ label) >> exitFailure)

outDir :: FilePath
outDir = "private-data/analysis/x-m1-1.50/recipes/rc-complete-20261013/refusal-test"

main :: IO ()
main = do
  [binaryArg] <- getArgs
  binary <- makeAbsolute binaryArg
  base <- getTemporaryDirectory
  (root, h) <- openTempFile base "build-recipe-package-test"
  hClose h
  removeFile root
  createDirectory root
  inherited <- getEnvironment
  let tools = replicate 13 (root </> "absent-tool")
      cleanEnv = filter ((`notElem` ["BUILD_WORKSPACE_DIRECTORY", "STOCK", "SPEC", "D3_SPEC"]) . fst) inherited
      runIn extra arguments =
        readCreateProcessWithExitCode (proc binary (tools ++ arguments)) {cwd = Just root, env = Just (extra ++ cleanEnv)} ""
      refused label needle (code, _, err) = do
        check (label ++ ": exit 1") (code == ExitFailure 1)
        check (label ++ ": stderr names " ++ show needle ++ ", got " ++ show err) (needle `isInfixOf` err)

  runIn [] [] >>= refused "no output directory" "usage: build-recipe-package OUT_DIR"
  runIn [] ["private-data/elsewhere"]
    >>= refused "output outside the RC directory" "output must stay under the host-only RC completion directory"
  runIn [("STOCK", "absent.DAT")] [outDir] >>= refused "missing stock DAT" "missing input: absent.DAT"
  writeFile (root </> "stock.DAT") "not the stock DAT"
  runIn [("STOCK", "stock.DAT")] [outDir]
    >>= refused "missing bank spec" "missing input: private-data/analysis/x-m1-1.50/recipes/bank-spec.json"
  created <- doesPathExist (root </> outDir)
  check "no output directory is created on refusal" (not created)

  removeDirectoryRecursive root
  putStrLn "build-recipe-package: refusal checks passed"
