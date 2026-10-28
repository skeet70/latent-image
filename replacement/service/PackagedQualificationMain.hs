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
  , optionReference :: FilePath
  , optionReport :: Maybe FilePath
  }

usage :: String
usage = unlines
  [ "usage: packaged-qualification --receipt E2E_DIR --package PACKAGE_DIR --reference-dat DAT [--report FILE]"
  , ""
  , "Consumes an existing RAM-push E2E receipt, its relinked manifest and kernel.syms,"
  , "the host-only recipe package, and the typed boot/install reference DAT. It exits"
  , "zero only when the five selected 0x110 rows are candidate-qualified with owned"
  , "entry/trampoline/stock/return traces, page-visit cancellation/reopen, and"
  , "unload/rollback evidence."
  ]

parseArgs :: [String] -> Either String Options
parseArgs = go Nothing Nothing Nothing Nothing
  where
    go receipt package reference report [] =
      case (receipt, package, reference) of
        (Just r, Just p, Just d) -> Right (Options r p d report)
        _ -> Left "--receipt, --package and --reference-dat are required"
    go _ package reference report ("--receipt" : value : rest) =
      go (Just value) package reference report rest
    go receipt _ reference report ("--package" : value : rest) =
      go receipt (Just value) reference report rest
    go receipt package _ report ("--reference-dat" : value : rest) =
      go receipt package (Just value) report rest
    go receipt package reference _ ("--report" : value : rest) =
      go receipt package reference (Just value) rest
    go _ _ _ _ (argument : _) = Left ("unknown option or missing value: " ++ argument)

main :: IO ()
main = do
  arguments <- getArgs
  case parseArgs arguments of
    Left problem -> hPutStrLn stderr (problem ++ "\n" ++ usage) >> exitFailure
    Right options -> run options

run :: Options -> IO ()
run options = do
  outcome <- try (qualifyPackaged (PackagedInputs (optionReceipt options) (optionPackage options) (optionReference options)))
    :: IO (Either IOException (Either String PackagedReport))
  case outcome of
    Left problem ->
      hPutStrLn stderr ("packaged qualification read failed: " ++ displayException problem) >> exitFailure
    Right (Left problem) ->
      hPutStrLn stderr ("packaged qualification invalid receipt: " ++ problem) >> exitFailure
    Right (Right report) -> do
      case optionReport options of
        Nothing -> pure ()
        Just path -> BL.writeFile path (BL.fromStrict (encodeCanonical (packagedReportValue report)) <> "\n")
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
