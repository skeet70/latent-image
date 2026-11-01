module Main where

import Data.Word (Word32)
import Numeric (readHex)
import Replacement.ModalRuntime.Qualify (Options (..), parseCaseSelection, runQualification)
import System.Environment (getArgs)
import System.Exit (exitFailure, exitSuccess)

main :: IO ()
main = do
  arguments <- getArgs
  runner <- required arguments "--runner"
  build <- required arguments "--build"
  output <- required arguments "--out"
  id5 <- required arguments "--id5"
  id5Base <- parseAddress (required arguments "--id5-base")
  textDigest <- required arguments "--kernel-text-sha256"
  symsDigest <- required arguments "--kernel-syms-sha256"
  selected <- either error pure (parseCaseSelection (values arguments "--case"))
  qualified <-
    runQualification
      Options
        { optionRunner = runner
        , optionBuild = build
        , optionOutput = output
        , optionId5 = id5
        , optionId5Base = id5Base
        , optionKernelTextSha256 = textDigest
        , optionKernelSymsSha256 = symsDigest
        , optionCases = selected
        }
  if qualified then exitSuccess else exitFailure

required :: [String] -> String -> IO String
required arguments name =
  case dropWhile (/= name) arguments of
    (_ : value : _) -> pure value
    _ -> error ("missing " ++ name)


values :: [String] -> String -> [String]
values [] _ = []
values (name : rest) wanted
  | name == wanted = case rest of
      value : remaining -> value : values remaining wanted
      [] -> error ("missing value for " ++ wanted)
  | otherwise = values rest wanted

parseAddress :: IO String -> IO Word32
parseAddress value = do
  text <- value
  case text of
    '0' : 'x' : digits
      | not (null digits)
      , [(word, "")] <- readHex digits -> pure (word :: Word32)
    _ -> error ("invalid 32-bit hexadecimal address: " ++ text)
