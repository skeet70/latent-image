{-# LANGUAGE OverloadedStrings #-}

-- | file-service-suite INCREMENT RUNNER IMAGE4 OWNED_ELF OUTPUT [CONTROL=ELF ...]
--
-- Runs one increment's cases on the stock image-4 bytes and on the owned ELF, then on each
-- deliberately wrong owned ELF. Equal requires every stock and owned case to meet its
-- independent expectation with a returned status, no stock/owned difference, and every
-- control failing at least one case after reaching the case exit.
module Main (main) where

import qualified ArmElf as E
import Control.Monad (forM, forM_, unless)
import Data.Aeson (encode, object, (.=))
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as BL
import Emulation.Sha256 (sha256, sha256Hex)
import qualified Replacement.FileService.Handles as Handles
import Replacement.FileService.Native
import System.Directory (createDirectoryIfMissing, doesPathExist)
import System.Environment (getArgs, lookupEnv)
import System.Exit (exitFailure)
import System.FilePath ((</>))

image4Digest :: String
image4Digest = "f2e2acab892dafeed85171f6870a411f0a2d5eae10921b86a24630d401dcff16"

data Increment = Increment
  { incrementCode :: Code
  , incrementScenarios :: [Scenario]
  , incrementEntries :: [(String, Word)]
  }

increments :: [(String, Increment)]
increments =
  [ ( "handles"
    , Increment Handles.code Handles.scenarios
        [ ("xm1_file_service_map_bind", 0x71ea34)
        , ("xm1_file_service_map_unbind", 0x71eaac)
        , ("xm1_file_service_map_lookup", 0x71eb28)
        , ("xm1_file_service_tag_allocate", 0x71ef18)
        , ("xm1_file_service_tag_release", 0x71eff4)
        ]
    )
  ]

loadElf :: [(String, Word)] -> FilePath -> IO (E.Elf, String)
loadElf entries path = do
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
    incrementName : runner : imagePath : ownedPath : output : controlArguments
      | Just increment <- lookup incrementName increments -> do
          exists <- doesPathExist output
          unless (not exists) (fail "output exists")
          createDirectoryIfMissing True output
          image <- B.readFile imagePath
          unless (sha256Hex (sha256 image) == image4Digest) (fail "image 4 identity differs")
          let entries = incrementEntries increment
              scenarios = incrementScenarios increment
              code = incrementCode increment
          (owned, ownedDigest) <- loadElf entries ownedPath
          stocks <- runRole runner output code image Stock scenarios
          owneds <- runRole runner output code image (Owned "owned" owned) scenarios
          let cases =
                [ (scenario, stock, ownedObservation, compareRoles scenario stock ownedObservation)
                | (scenario, stock, ownedObservation) <- zip3 scenarios stocks owneds
                ]
          controls <- forM (map splitControl controlArguments) $ \(name, path) -> do
            (elf, digest) <- loadElf entries path
            observed <- runRole runner output code image (Owned name elf) scenarios
            let (detected, unreturned) = controlDetections observed
            pure (name, digest, detected, unreturned)
          let different = [() | (_, _, _, differences) <- cases, not (null differences)]
              undetected = [name | (name, _, detected, _) <- controls, null detected]
              equal = null different && null undetected && not (null controls)
              report =
                object
                  [ "suite" .= ("file-service-" ++ incrementName)
                  , "image4_sha256" .= image4Digest
                  , "owned_sha256" .= ownedDigest
                  , "decomp_commit" .= ("55436fc85c0560dd252b0410af6f2427fe198cd3" :: String)
                  , "verdict" .= (if equal then "equal" else "different" :: String)
                  , "cases" .=
                      [ object
                          [ "case" .= scenarioName scenario
                          , "entry" .= hexWord (scenarioEntry scenario)
                          , "stock_status" .= observationRunnerStatus stock
                          , "owned_status" .= observationRunnerStatus ownedObservation
                          , "stock_instructions" .= observationInstructions stock
                          , "owned_instructions" .= observationInstructions ownedObservation
                          , "service_calls" .= length (scenarioServices scenario)
                          , "differences" .= differences
                          ]
                      | (scenario, stock, ownedObservation, differences) <- cases
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
          forM_ result $ \directory -> BL.writeFile (directory </> ("file-service-" ++ incrementName ++ ".json")) (encode report)
          putStrLn
            ( incrementName ++ ": " ++ show (length cases) ++ " cases, " ++ show (length different) ++ " different; "
                ++ show (length controls - length undetected) ++ "/" ++ show (length controls) ++ " controls detected; "
                ++ (if equal then "equal" else "different")
            )
          unless equal exitFailure
    _ -> fail "usage: file-service-suite INCREMENT RUNNER IMAGE4 OWNED_ELF OUTPUT [CONTROL=ELF ...]"
