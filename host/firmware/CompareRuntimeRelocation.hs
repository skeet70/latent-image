module Main (main) where

import Control.Exception (IOException, try)
import Data.Aeson (encode, object, (.=))
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy.Char8 as BL
import RuntimeRelocationCompare (Comparison (comparisonAccepted), PackageBytes (..), comparePackages, comparisonValue)
import System.Environment (getArgs)
import System.Exit (ExitCode (..), exitWith)
import System.FilePath ((</>))

main :: IO ()
main = do
  arguments <- getArgs
  case arguments of
    [left, right] -> do
      inputs <- try ((,) <$> package left <*> package right)
      case inputs of
        Left problem -> refuse (show (problem :: IOException))
        Right (a, b) -> case comparePackages a b of
          Left problem -> refuse problem
          Right report -> do
            BL.putStrLn (encode (comparisonValue report))
            exitWith (if comparisonAccepted report then ExitSuccess else ExitFailure 1)
    _ -> refuse "usage: compare-runtime-relocation LEFT_PACKAGE_DIR RIGHT_PACKAGE_DIR"
 where
  package directory = PackageBytes <$> load "kernel.elf" <*> load "runtime.bin" <*> load "ram-push-manifest.json" <*> load "bank-plan.json"
   where load name = B.readFile (directory </> name)
  refuse :: String -> IO ()
  refuse problem = do
    BL.putStrLn (encode (object ["schema" .= ("runtime-relocation-comparison-1" :: String), "accepted" .= False, "input_error" .= problem]))
    exitWith (ExitFailure 2)
