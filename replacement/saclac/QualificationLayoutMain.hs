{-# LANGUAGE OverloadedStrings #-}
module Main (main) where

import qualified Data.ByteString.Lazy as BL
import Data.Aeson (encode, object, (.=))
import Data.List (isPrefixOf)
import Replacement.Saclac.QualificationLayout
import System.Environment (getArgs)
import System.Exit (exitFailure)
import System.IO (hPutStrLn, stderr)

data Options = Options
  { optDwarfdump :: FilePath
  , optProductions :: [FilePath]
  , optQualification :: FilePath
  , optRootSuffix :: String
  , optJsonOut :: Maybe FilePath
  }

defaultOptions :: Options
defaultOptions = Options
  { optDwarfdump = ""
  , optProductions = []
  , optQualification = ""
  , optRootSuffix = "saclac/root/src/stock/composition.rs"
  , optJsonOut = Nothing
  }

main :: IO ()
main = do
  args <- getArgs
  options <- either (failUsage) pure (parseOptions defaultOptions args)
  let roots = [RootSpec "OWNER" (optRootSuffix options), RootSpec "RECOGNITION" (optRootSuffix options)]
  reports <- mapM (\production -> checkLayouts (optDwarfdump options) production (optQualification options) roots) (optProductions options)
  let verdict :: String
      verdict
        | any ((== "missing") . reportVerdict) reports = "missing"
        | any ((== "mismatch") . reportVerdict) reports = "mismatch"
        | otherwise = "match"
      combined = object
        [ "verdict" .= verdict
        , "types_compared" .= sum (map reportTypesCompared reports)
        , "mismatches" .=
            [ object
                [ "production" .= mismatchProduction mismatch
                , "qualification" .= mismatchQualification mismatch
                , "path" .= mismatchPath mismatch
                , "detail" .= mismatchDetail mismatch
                ]
            | report <- reports
            , mismatch <- reportMismatches report
            ]
        ]
      bytes = encode combined
  BL.putStr bytes
  putStrLn ""
  case optJsonOut options of
    Nothing -> pure ()
    Just path -> BL.writeFile path bytes
  if verdict == "match" then pure () else exitFailure

failUsage :: String -> IO a
failUsage message = do
  hPutStrLn stderr message
  hPutStrLn stderr usage
  exitFailure

usage :: String
usage = unlines
  [ "usage: qualification-layout-check --dwarfdump PATH --qualification ELF"
  , "         --production ELF [--production ELF ...]"
  , "         [--root-suffix SUFFIX] [--json-out PATH]"
  ]

parseOptions :: Options -> [String] -> Either String Options
parseOptions options [] = validate options
parseOptions options ("--dwarfdump" : value : rest) = parseOptions (options { optDwarfdump = value }) rest
parseOptions options ("--production" : value : rest) = parseOptions (options { optProductions = optProductions options ++ [value] }) rest
parseOptions options ("--qualification" : value : rest) = parseOptions (options { optQualification = value }) rest
parseOptions options ("--root-suffix" : value : rest) = parseOptions (options { optRootSuffix = value }) rest
parseOptions options ("--json-out" : value : rest) = parseOptions (options { optJsonOut = Just value }) rest
parseOptions _ (flag : _)
  | "--" `isPrefixOf` flag = Left ("unknown option " ++ flag)
parseOptions _ (value : _) = Left ("unexpected argument " ++ value)

validate :: Options -> Either String Options
validate options
  | null (optDwarfdump options) = Left "--dwarfdump is required"
  | null (optQualification options) = Left "--qualification is required"
  | null (optProductions options) = Left "at least one --production is required"
  | otherwise = Right options
