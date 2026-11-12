{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}

-- | The tracked case table the static audit reads must be the table this suite
-- emits. A divergence fails here instead of leaving the audit on a declaration.
module Main (main) where

import Data.Aeson (Value, eitherDecodeStrict')
import qualified Data.ByteString as B
import Replacement.RecordStore.Harness (caseTableValue)
import System.Environment (getArgs)
import System.Exit (exitFailure)

main :: IO ()
main = do
  [path] <- getArgs
  tracked <- either fail pure . eitherDecodeStrict' =<< B.readFile path :: IO Value
  if tracked == caseTableValue
    then putStrLn "record-store case table equals the suite emission"
    else putStrLn "record-store tracked case table differs from the suite emission" >> exitFailure
