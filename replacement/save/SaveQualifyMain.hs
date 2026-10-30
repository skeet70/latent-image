{-# LANGUAGE ScopedTypeVariables #-}

module Main (main) where

import Control.Exception (IOException, try)
import qualified Data.ByteString as B
import FujiAF.Host (ensure, failH, get, jsonPrint, str)
import FujiAF.JSON (encodeCanonical)
import Replacement.Save.Qualify (Paths (..), runQualification)
import System.Environment (getArgs)
import System.Exit (ExitCode (..), exitWith)

main :: IO ()
main = do
  arguments <- getArgs
  outcome <- try $ do
    (paths, reportPath) <- parseArguments arguments
    report <- runQualification paths
    pure (report, reportPath)
  case outcome of
    Left (exception :: IOException) -> do
      putStrLn ("refused: " ++ show exception)
      exitWith (ExitFailure 2)
    Right (report, reportPath) -> do
      jsonPrint report
      case reportPath of
        Nothing -> pure ()
        Just path -> B.writeFile path (encodeCanonical report)
      if str (get report "status") == "qualified"
        then exitWith ExitSuccess
        else exitWith (ExitFailure 1)

parseArguments :: [String] -> IO (Paths, Maybe FilePath)
parseArguments arguments =
  case positional of
    [contract, owned, faulty, storage, storageFaulty, waitModel, stageModel, runner] -> do
      ensure (not (null stock)) "--stock-image is required"
      ensure (not (null output)) "--output is required"
      pure
        ( Paths
            { pathContract = contract
            , pathOwned = owned
            , pathFaulty = faulty
            , pathStorage = storage
            , pathStorageFaulty = storageFaulty
            , pathWait = waitModel
            , pathStage = stageModel
            , pathRunner = runner
            , pathStockImage = stock
            , pathOutput = output
            }
        , report
        )
    _ -> failH "usage: save-qualify CONTRACT OWNED FAULTY STORAGE STORAGE_FAULTY WAIT STAGE RUNNER --stock-image PATH --output DIR [--report FILE]"
  where
    (positional, flags) = collect arguments
    stock = valueOf "--stock-image"
    output = valueOf "--output"
    report = case valueOf "--report" of
      "" -> Nothing
      path -> Just path
    valueOf name =
      case [next | (flag, next) <- flags, flag == name] of
        (value : _) -> value
        [] -> ""
    collect [] = ([], [])
    collect (flag : next : rest)
      | flag `elem` ["--stock-image", "--output", "--report"] =
          let (positionals, pending) = collect rest in (positionals, (flag, next) : pending)
    collect (value : rest) = let (positionals, pending) = collect rest in (value : positionals, pending)
