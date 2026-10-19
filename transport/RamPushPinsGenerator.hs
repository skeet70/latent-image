-- Writes the RamPushReviewedPins module from the RAM-push review record.
module Main where

import qualified Data.ByteString as B
import System.Environment (getArgs)
import System.Exit (die)
import FujiAF.RamPushReview (parseReviewRecord, pinsModuleSource)

main :: IO ()
main = do
  arguments <- getArgs
  case arguments of
    [recordPath, out] -> do
      document <- B.readFile recordPath
      either (die . ((recordPath ++ ": ") ++)) (writeFile out . pinsModuleSource) (parseReviewRecord document)
    _ -> die "usage: ram-push-pins-generator RECORD OUT"
