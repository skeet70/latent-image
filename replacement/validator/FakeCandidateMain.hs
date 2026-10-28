module Main where

import qualified Data.ByteString.Char8 as BC
import FujiAF.Host (runBounded)
import System.Environment (getArgs)
import System.Exit (ExitCode (..), exitWith)
import System.IO (hGetContents, stdin)

isRefusalCheck :: String -> Bool
isRefusalCheck line = case words line of
  ("check" : _) -> True
  _ -> False

main :: IO ()
main = do
  arguments <- getArgs
  case arguments of
    [delegate, mode] -> do
      input <- hGetContents stdin
      let forwarded = case mode of
            "omit-refusal-request" -> filter (not . isRefusalCheck) (lines input)
            _ -> lines input
      (timedOut, exit, out, _) <- runBounded 60000000 (16 * 1024 * 1024) delegate [] (BC.pack (unlines forwarded))
      BC.putStr out
      if timedOut || exit /= ExitSuccess then exitWith (ExitFailure 1) else pure ()
    _ -> exitWith (ExitFailure 2)
