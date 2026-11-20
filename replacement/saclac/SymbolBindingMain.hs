{-# LANGUAGE OverloadedStrings #-}
module Main (main) where

import qualified Data.ByteString.Lazy as BL
import Data.Aeson (encode)
import Replacement.Saclac.SymbolBinding
import System.Environment (getArgs)
import System.Exit (exitFailure)
import System.IO (hPutStrLn, stderr)

main :: IO ()
main = do
  args <- getArgs
  case args of
    ["--production", production, "--debug", debug] -> run production debug
    _ -> do
      hPutStrLn stderr "usage: qualification-symbol-binding --production SYMBOLS --debug SYMBOLS"
      exitFailure

run :: FilePath -> FilePath -> IO ()
run productionPath debugPath = do
  production <- parseSymbols <$> readFile productionPath
  debug <- parseSymbols <$> readFile debugPath
  let report = checkSymbols production debug
  BL.putStr (encode (reportJson report))
  putStrLn ""
  mapM_ (\(name, productionSize, debugSize) ->
           putStrLn ("function size change: " ++ name ++ " " ++ show productionSize ++ " -> " ++ show debugSize))
        (reportFunctionChanges report)
  if reportVerdict report == "match" then pure () else exitFailure
