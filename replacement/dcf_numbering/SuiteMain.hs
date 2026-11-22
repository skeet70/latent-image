{-# LANGUAGE OverloadedStrings #-}

-- | dcf-numbering-suite RUNNER IMAGE4 OWNED_ELF OUTPUT CONTROL=ELF ...
--
-- Runs the DCF numbering cases on the stock image-4 bytes, on the owned ELF and on each
-- deliberately wrong owned ELF. Equal requires every stock and owned case to meet its
-- independent expectation with no stock/owned difference, and every control to fail at
-- least one case after reaching the case exit.
module Main (main) where

import qualified ArmElf as E
import Control.Monad (forM, forM_, unless)
import Data.Aeson (encode, object, (.=))
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as BL
import Emulation.Sha256 (sha256, sha256Hex)
import Replacement.DcfNumbering.Cases (code, entries, scenarios)
import Replacement.FileService.Native
import System.Directory (createDirectoryIfMissing, doesPathExist)
import System.Environment (getArgs, lookupEnv)
import System.Exit (exitFailure)
import System.FilePath ((</>))

image4Digest :: String
image4Digest = "f2e2acab892dafeed85171f6870a411f0a2d5eae10921b86a24630d401dcff16"

loadElf :: FilePath -> IO (E.Elf, String)
loadElf path = do
  bytes <- B.readFile path
  elf <- either fail pure (E.parseElf bytes)
  forM_ entries $ \(name, address) ->
    case [symbol | symbol <- E.elfSymbols elf, E.symbolName symbol == name, E.symbolKind symbol == E.FunctionSymbol] of
      [symbol] | fromIntegral (E.symbolValue symbol) == address -> pure ()
      _ -> fail (path ++ ": " ++ name ++ " is missing or not at its stock address")
  pure (elf, sha256Hex (sha256 bytes))

splitControl :: String -> (String, FilePath)
splitControl argument = case break (== '=') argument of
  (name, '=' : path) -> (name, path)
  _ -> error ("control argument is not NAME=ELF: " ++ argument)

main :: IO ()
main = do
  arguments <- getArgs
  case arguments of
    runner : imagePath : ownedPath : output : controlArguments@(_ : _) -> do
      exists <- doesPathExist output
      unless (not exists) (fail "output exists")
      createDirectoryIfMissing True output
      image <- B.readFile imagePath
      unless (sha256Hex (sha256 image) == image4Digest) (fail "image 4 identity differs")
      let cases = scenarios image
      (owned, ownedDigest) <- loadElf ownedPath
      stocks <- runRole runner output code image Stock cases
      owneds <- runRole runner output code image (Owned "owned" owned) cases
      let compared = [(scenario, stock, mine, compareRoles scenario stock mine) | (scenario, stock, mine) <- zip3 cases stocks owneds]
      controls <- forM (map splitControl controlArguments) $ \(name, path) -> do
        (elf, digest) <- loadElf path
        observed <- runRole runner output code image (Owned name elf) cases
        let (detected, unreturned) = controlDetections observed
        pure (name, digest, detected, unreturned)
      let different = [() | (_, _, _, differences) <- compared, not (null differences)]
          undetected = [name | (name, _, detected, _) <- controls, null detected]
          equal = null different && null undetected
          report =
            object
              [ "suite" .= ("dcf-numbering" :: String)
              , "image4_sha256" .= image4Digest
              , "owned_sha256" .= ownedDigest
              , "decomp_commit" .= ("55436fc85c0560dd252b0410af6f2427fe198cd3" :: String)
              , "verdict" .= (if equal then "equal" else "different" :: String)
              , "cases" .=
                  [ object
                      [ "case" .= scenarioName scenario
                      , "entry" .= hexWord (scenarioEntry scenario)
                      , "stock_status" .= observationRunnerStatus stock
                      , "owned_status" .= observationRunnerStatus mine
                      , "stock_instructions" .= observationInstructions stock
                      , "owned_instructions" .= observationInstructions mine
                      , "service_calls" .= length (scenarioServices scenario)
                      , "differences" .= differences
                      ]
                  | (scenario, stock, mine, differences) <- compared
                  ]
              , "controls" .=
                  [ object
                      [ "name" .= name
                      , "sha256" .= digest
                      , "exit_detections" .= detected
                      , "other_statuses" .= [object ["case" .= ident, "status" .= status] | (ident, status) <- unreturned]
                      , "detected" .= not (null detected)
                      ]
                  | (name, digest, detected, unreturned) <- controls
                  ]
              ]
      BL.writeFile (output </> "report.json") (encode report)
      result <- lookupEnv "XM1_RUN_RESULT"
      forM_ result $ \directory -> BL.writeFile (directory </> "dcf-numbering.json") (encode report)
      putStrLn
        ( show (length compared) ++ " cases, " ++ show (length different) ++ " different; "
            ++ show (length controls - length undetected) ++ "/" ++ show (length controls) ++ " controls detected; "
            ++ (if equal then "equal" else "different")
        )
      unless equal exitFailure
    _ -> fail "usage: dcf-numbering-suite RUNNER IMAGE4 OWNED_ELF OUTPUT CONTROL=ELF ..."
