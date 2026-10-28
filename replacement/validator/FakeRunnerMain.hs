module Main where

import Control.Exception (evaluate)
import System.Environment (lookupEnv)
import System.Exit (ExitCode (..), exitWith)
import System.IO (hGetContents, stdin)

main :: IO ()
main = do
  wire <- hGetContents stdin
  _ <- evaluate (length wire)
  mode <- maybe "" id <$> lookupEnv "FAKE_RUNNER_MODE"
  case mode of
    "nonzero-exit-returned" -> do
      putStrLn "{\"wire_version\":1,\"status\":\"returned\",\"unicorn_error\":0}"
      exitWith (ExitFailure 1)
    "returned-nonzero-unicorn" ->
      putStrLn "{\"wire_version\":1,\"status\":\"returned\",\"unicorn_error\":1}"
    "fractional-unicorn" ->
      putStrLn "{\"wire_version\":1,\"status\":\"returned\",\"unicorn_error\":0.5}"
    "negative-counter" ->
      putStrLn "{\"wire_version\":1,\"status\":\"returned\",\"unicorn_error\":0,\"instructions\":\"-1\"}"
    _ -> putStrLn "{\"wire_version\":1,\"status\":\"returned\",\"unicorn_error\":0}"
