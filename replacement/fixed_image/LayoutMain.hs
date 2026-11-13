module Main where

import Replacement.FixedImage.Layout
import System.Environment (getArgs)
import System.Exit (die)

main :: IO ()
main = do
  arguments <- getArgs
  case arguments of
    ["rust"] -> putStr rustConstants
    ["memory"] -> putStr linkerMemory
    _ -> die "usage: fixed-image-layout rust|memory"
