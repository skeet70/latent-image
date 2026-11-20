{-# LANGUAGE OverloadedStrings #-}
module Main (main) where

import qualified FujiAF.Host as H
import Replacement.Saclac.SymbolBinding
import System.Environment (getArgs)

main :: IO ()
main = do
  args <- getArgs
  case args of
    [production, debug, bad] -> do
      accepted <- checkFiles production debug
      H.ensure (reportVerdict accepted == "match")
        ("identical data symbols were refused: " ++ show (reportMismatches accepted))
      H.ensure (reportUniformShift accepted == Just 8)
        ("uniform shift was " ++ show (reportUniformShift accepted))
      H.ensure (any (\(name, _, _) -> name == "func_a") (reportFunctionChanges accepted))
        "function size change was not reported"
      refused <- checkFiles production bad
      H.ensure (reportVerdict refused == "mismatch")
        "changed data size was accepted"
      H.ensure (any ((== "data_a") . mismatchSymbol) (reportMismatches refused))
        ("refusal did not name data_a: " ++ show (reportMismatches refused))
    _ -> H.failH "expected: PRODUCTION_SYMBOLS DEBUG_SYMBOLS BAD_SYMBOLS"

checkFiles :: FilePath -> FilePath -> IO SymbolReport
checkFiles productionPath debugPath = do
  production <- parseSymbols <$> readFile productionPath
  debug <- parseSymbols <$> readFile debugPath
  pure (checkSymbols production debug)
