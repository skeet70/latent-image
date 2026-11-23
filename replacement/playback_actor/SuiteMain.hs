{-# LANGUAGE OverloadedStrings #-}

-- | playback-actor-suite RUNNER PLAIN OWNED_ELF OUTPUT CONTROL=ELF ...
--
-- Runs the owned machines' cases on the stock plain-image bytes, on the owned ELF and on each
-- deliberately wrong owned ELF. Equal requires every stock and owned case to meet its
-- independent expectation with no stock/owned difference, and every control to fail at
-- least one case after reaching the case exit.
module Main (main) where

import qualified ArmElf as E
import Control.DeepSeq (force)
import Control.Exception (evaluate)
import Control.Monad (forM, forM_, unless)
import Data.Aeson (encode, object, (.=))
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as BL
import Emulation.Sha256 (sha256, sha256Hex)
import Replacement.FileService.Native
import Replacement.PlaybackActor.Cases (OwnedMachine (..), code, ownedMachines, scenarios)
import System.Directory (createDirectoryIfMissing, doesPathExist)
import System.Environment (getArgs, lookupEnv)
import System.Exit (exitFailure)
import System.FilePath ((</>))

plainDigest :: String
plainDigest = "ee211bdaad39005b4f72e3443a76ca194905381a967cab787d6a4ef14e2d889d"

-- | Each owned machine's table rows must name its own action exports in index order.
loadElf :: FilePath -> IO (E.Elf, String)
loadElf path = do
  bytes <- B.readFile path
  elf <- either fail pure (E.parseElf bytes)
  let addressOf export = case [E.symbolValue symbol | symbol <- E.elfSymbols elf, E.symbolName symbol == export, E.symbolKind symbol == E.FunctionSymbol] of
        [value] -> pure value
        _ -> fail (path ++ ": " ++ export ++ " is missing")
      name machine index = ownedExport machine ++ (if index < 10 then "0" else "") ++ show (index :: Int)
      row machine index = B.pack [maybe 0 id (E.byteAt elf (ownedRows machine + 4 * index + k)) | k <- [0 .. 3]]
  forM_ ownedMachines $ \machine ->
    forM_ (zip [0 ..] [name machine index | index <- [2 .. length (ownedActions machine) + 1]]) $ \(index, export) -> do
      value <- addressOf export
      unless (row machine index == words32le [fromIntegral value]) (fail (path ++ ": table row " ++ show index ++ " does not name " ++ export))
  pure (elf, sha256Hex (sha256 bytes))

splitControl :: String -> (String, FilePath)
splitControl argument = case break (== '=') argument of
  (name, '=' : path) -> (name, path)
  _ -> error ("control argument is not NAME=ELF: " ++ argument)

main :: IO ()
main = do
  arguments <- getArgs
  case arguments of
    runner : plainPath : ownedPath : output : controlArguments@(_ : _) -> do
      exists <- doesPathExist output
      unless (not exists) (fail "output exists")
      createDirectoryIfMissing True output
      image <- B.readFile plainPath
      unless (sha256Hex (sha256 image) == plainDigest) (fail "plain image identity differs")
      let cases = scenarios image
      (owned, ownedDigest) <- loadElf ownedPath
      stocks <- runRole runner output code image Stock cases
      owneds <- runRole runner output code image (Owned "owned" owned) cases
      let compared = [(scenario, stock, mine, compareRoles scenario stock mine) | (scenario, stock, mine) <- zip3 cases stocks owneds]
      controls <- forM (map splitControl controlArguments) $ \(name, path) -> do
        (elf, digest) <- loadElf path
        observed <- runRole runner output code image (Owned name elf) cases
        (detected, unreturned) <- evaluate (force (controlDetections observed))
        pure (name, digest, detected, unreturned)
      let different = [() | (_, _, _, differences) <- compared, not (null differences)]
          undetected = [name | (name, _, detected, _) <- controls, null detected]
          equal = null different && null undetected
          report =
            object
              [ "suite" .= ("playback-actor" :: String)
              , "plain_sha256" .= plainDigest
              , "owned_sha256" .= ownedDigest
              , "decomp_commit" .= ("55436fc85c0560dd252b0410af6f2427fe198cd3" :: String)
              , "verdict" .= (if equal then "equal" else "different" :: String)
              , "cases" .=
                  [ object
                      [ "case" .= scenarioName scenario
                      , "stock_status" .= observationRunnerStatus stock
                      , "owned_status" .= observationRunnerStatus mine
                      , "stock_mismatches" .= observationMismatches stock
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
      forM_ result $ \directory -> BL.writeFile (directory </> "playback-actor.json") (encode report)
      putStrLn
        ( show (length compared) ++ " cases, " ++ show (length different) ++ " different; "
            ++ show (length controls - length undetected) ++ "/" ++ show (length controls) ++ " controls detected; "
            ++ (if equal then "equal" else "different")
        )
      unless equal exitFailure
    _ -> fail "usage: playback-actor-suite RUNNER PLAIN OWNED_ELF OUTPUT CONTROL=ELF ..."
