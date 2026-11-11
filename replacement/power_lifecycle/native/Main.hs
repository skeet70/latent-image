{-# LANGUAGE OverloadedStrings #-}
module Main (main) where

import Control.Monad (unless)
import Data.Aeson (encode, object, (.=))
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as L
import Emulation.Sha256 (sha256, sha256Hex)
import FujiModNativePaths (fujimodNativeCase)
import Replacement.PowerLifecycle.RecordHelperSuite (ControlResult (..), controlIdentities, runSuite, sourceIdentity)
import System.Directory (createDirectoryIfMissing)
import System.Environment (getArgs)
import System.Exit (exitFailure)
import System.FilePath ((</>))

main :: IO ()
main = do
  arguments <- getArgs
  runner <- case dropWhile (/= "--runner") arguments of
    [] -> pure fujimodNativeCase
    _ : value : _ -> pure value
    _ -> fail "--runner requires a value"
  cache <- case dropWhile (/= "--cache-root") arguments of
    [] -> pure "private-data/cache/native"
    _ : value : _ -> pure value
    _ -> fail "--cache-root requires a value"
  image4 <- required arguments "--image4"
  image5 <- required arguments "--image5"
  elf <- required arguments "--owned-elf"
  output <- required arguments "--output"
  let only = case dropWhile (/= "--rows") arguments of
        [] -> []
        _ : value : _ -> [address | address <- splitOnComma value, not (null address)]
        _ -> error "--rows requires a value"
  createDirectoryIfMissing True output
  image4Hash <- sha256Hex . sha256 <$> B.readFile image4
  ownedHash <- sha256Hex . sha256 <$> B.readFile elf
  (differences, controls) <- runSuite runner cache output image4 image5 elf only
  let failedControls = [controlHelper control ++ " " ++ controlVariant control | control <- controls, not (controlDetected control)]
      passed = null differences && null failedControls && not (null controls)
  L.writeFile (output </> "record-helper-report.json") (encode (object
    [ "source" .= sourceIdentity
    , "image4_path" .= image4
    , "image4_sha256" .= image4Hash
    , "owned_path" .= elf
    , "owned_sha256" .= ownedHash
    , "runner" .= runner
    , "rows" .= only
    , "passed" .= passed
    , "differences" .= differences
    , "controls" .= [object ["helper" .= controlHelper control, "module" .= controlModule control, "variant" .= controlVariant control, "detected" .= controlDetected control] | control <- controls]
    , "control_identities" .= controlIdentities
    , "scope" .= ("Class (a) stock bodies that are pure functions of their registers and declared ordinary memory. Every case maps only the compared extents and staged pages; no MMIO, coprocessor, interrupt, timing or device claim." :: String)
    ]))
  mapM_ putStrLn (take 200 differences)
  unless (null failedControls) (putStrLn ("undetected controls: " ++ show failedControls))
  print (length differences, length controls, passed)
  unless passed exitFailure

required :: [String] -> String -> IO FilePath
required arguments flag = case dropWhile (/= flag) arguments of
  _ : value : _ -> pure value
  _ -> fail (flag ++ " requires a value")

splitOnComma :: String -> [String]
splitOnComma value = case break (== ',') value of
  (one, []) -> [one]
  (one, _ : rest) -> one : splitOnComma rest
