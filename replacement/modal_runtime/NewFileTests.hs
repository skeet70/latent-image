module Main (main) where

import Control.Exception (IOException, try)
import Control.Monad (unless)
import Replacement.ModalRuntime.NewFile (writeNewFile)
import System.Directory (getTemporaryDirectory, removeFile)
import System.Exit (exitFailure)
import System.FilePath ((</>))

main :: IO ()
main = do
  temporary <- getTemporaryDirectory
  let path = temporary </> "new-file-tests.rs"
  writeNewFile path "first\n"
  created <- readFile path
  second <- try (writeNewFile path "second\n") :: IO (Either IOException ())
  kept <- readFile path
  length kept `seq` removeFile path
  let results =
        [ ("a new path is written", created == "first\n")
        , ("an existing path refuses", either (const True) (const False) second)
        , ("an existing path keeps its bytes", kept == "first\n")
        ]
      failures = [label | (label, passed) <- results, not passed]
  mapM_ (putStrLn . ("FAIL: " ++)) failures
  unless (null failures) exitFailure
  putStrLn ("new file tests passed: " ++ show (length results))
