module Main (main) where

import EvidenceRecord qualified
import System.Environment (getArgs)

main :: IO ()
main = getArgs >>= EvidenceRecord.run
