{-# LANGUAGE GHC2021 #-}
module Main (main) where

import Control.Monad (unless)
import SettingsStorageMatrixTests (matrixConsumerCases)
import System.Exit (exitFailure)

check :: String -> Bool -> IO Bool
check name passed = do
  unless passed (putStrLn ("FAIL " ++ name))
  pure passed

main :: IO ()
main = do
  results <- mapM (uncurry check) matrixConsumerCases
  unless (and results) exitFailure
  putStrLn ("settings-storage-matrix-tests: " ++ show (length results) ++ " passed")
