{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}
module Main (main) where

import Control.Exception (IOException, displayException, try)
import qualified Data.ByteString.Lazy as BL
import FujiAF.JSON (encodeCanonical)
import Replacement.Service.Packaged
import System.Environment (getArgs)
import System.Exit (exitFailure)
import System.IO (hPutStrLn, stderr)

data Options = Options
  { optionReceipt :: FilePath
  , optionPackage :: FilePath
  , optionReport :: Maybe FilePath
  }

usage :: String
usage = unlines
  [ "usage: packaged-qualification --receipt E2E_DIR --package PACKAGE_DIR [--report FILE]"
  , ""
  , "Consumes an existing RAM-push E2E receipt, its relinked manifest and kernel.syms,"
  , "and the host-only recipe package. It exits zero only when the five selected 0x110"
  , "rows are candidate-qualified with owned entry/trampoline/stock/return traces,"
  , "page-visit cancellation/reopen, and unload/rollback evidence."
  ]

parseArgs :: [String] -> Either String Options
parseArgs = go Nothing Nothing Nothing
  where
    go receipt package report [] =
      case (receipt, package) of
        (Just r, Just p) -> Right (Options r p report)
        _ -> Left "both --receipt and --package are required"
    go receipt package report ("--receipt" : value : rest) =
      go (Just value) package report rest
    go receipt package report ("--package" : value : rest) =
      go receipt (Just value) report rest
    go receipt package _ ("--report" : value : rest) =
      go receipt package (Just value) rest
    go _ _ _ (argument : _) = Left ("unknown option or missing value: " ++ argument)

main :: IO ()
main = do
  arguments <- getArgs
  case parseArgs arguments of
    Left problem -> hPutStrLn stderr (problem ++ "\n" ++ usage) >> exitFailure
    Right options -> run options

run :: Options -> IO ()
run options = do
  outcome <- try (qualifyPackaged (PackagedInputs (optionReceipt options) (optionPackage options)))
    :: IO (Either IOException (Either String PackagedReport))
  case outcome of
    Left problem ->
      hPutStrLn stderr ("packaged qualification read failed: " ++ displayException problem) >> exitFailure
    Right (Left problem) ->
      hPutStrLn stderr ("packaged qualification invalid receipt: " ++ problem) >> exitFailure
    Right (Right report) -> do
      case optionReport options of
        Nothing -> pure ()
        Just path -> BL.writeFile path (encodeCanonical (packagedReportValue report) <> "\n")
      putStrLn
        ( "service packaged qualification "
            ++ packagedStatusLabel (packagedStatus report)
            ++ ": "
            ++ show (qualifiedRowCount (packagedRows report))
            ++ "/5 selected rows"
        )
      case packagedProblems report of
        [] -> pure ()
        problems -> mapM_ (hPutStrLn stderr . ("refused: " ++)) problems
      if packagedStatus report == PackagedQualified then pure () else exitFailure
