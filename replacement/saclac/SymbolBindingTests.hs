{-# LANGUAGE OverloadedStrings #-}
module Main (main) where

import qualified FujiAF.Host as H
import Replacement.Saclac.SymbolBinding
import System.Environment (getArgs)

main :: IO ()
main = do
  args <- getArgs
  case args of
    [production, debug, wrongProduction, wrongDebug] -> do
      check "production" production debug
      check "wrong-driver-store" wrongProduction wrongDebug
    _ -> H.failH "expected: PRODUCTION_SYMBOLS DEBUG_SYMBOLS WRONG_PRODUCTION_SYMBOLS WRONG_DEBUG_SYMBOLS"

check :: String -> FilePath -> FilePath -> IO ()
check label productionPath debugPath = do
  production <- parseSymbols <$> readFile productionPath
  debug <- parseSymbols <$> readFile debugPath
  let report = checkSymbols production debug
  H.ensure (reportVerdict report == "match")
    (label ++ " data symbols differ: " ++ show (reportMismatches report))
  H.ensure (reportDataSymbols report > 0) (label ++ " has no data symbols")
  H.ensure (maybe False (const True) (reportUniformShift report))
    (label ++ " data addresses do not move by one uniform shift")
  putStrLn (label ++ " data symbols=" ++ show (reportDataSymbols report)
    ++ " shift=" ++ show (reportUniformShift report)
    ++ " function size changes=" ++ show (length (reportFunctionChanges report)))
  mapM_ (\(name, productionSize, debugSize) ->
           putStrLn ("  " ++ name ++ " " ++ show productionSize ++ " -> " ++ show debugSize))
        (reportFunctionChanges report)
