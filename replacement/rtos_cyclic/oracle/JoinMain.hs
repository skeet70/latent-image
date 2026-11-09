{-# LANGUAGE OverloadedStrings #-}

-- | Runs the core-0 app-callback join: the emitted app caller at 0x725424 over the loader-published
-- root and interface table, the emitted cyclic create/start entries, the emitted timer tick, and
-- the stock registry dispatcher and stack-guard callback, against the stock caller and stock loader
-- entries. Writes a terminal JSON.
module Main (main) where

import Control.Monad (unless)
import Data.Aeson (object, (.=), encode)
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as BL
import Emulation.Sha256 (sha256File, sha256Hex)
import FujiAF.Host (failH)
import FujiAF.StorageBudget (checkStorageBudget)
import qualified Replacement.RtosCyclic.AppSuite as App
import Replacement.RtosCyclic.JoinSuite
import qualified Replacement.RtosCyclic.Suite as Cyclic
import qualified Replacement.RtosTimer.Suite as Timer
import Replacement.RtosOwnFirst.Regions (Domain (..))
import System.Directory (createDirectoryIfMissing)
import System.Environment (getArgs)
import System.Exit (exitFailure)
import System.FilePath (takeDirectory)

main :: IO ()
main = do
  runner <- required "--runner"
  image4Path <- required "--image4"
  loaderAPath <- required "--loader-a"
  loaderBPath <- required "--loader-b"
  appPath <- required "--app-bin"
  appSymbolsPath <- required "--app-symbols"
  cyclicPath <- required "--cyclic-bin"
  cyclicSymbolsPath <- required "--cyclic-symbols"
  timerPath <- required "--timer-bin"
  timerSymbolsPath <- required "--timer-symbols"
  cacheRoot <- required "--cache-root"
  resultsPath <- required "--results"
  image4 <- B.readFile image4Path
  loaderA <- B.readFile loaderAPath
  loaderB <- B.readFile loaderBPath
  app <- B.readFile appPath
  cyclic <- B.readFile cyclicPath
  timer <- B.readFile timerPath
  appSymbols <- App.readOwnedFunctions appSymbolsPath
  cyclicSymbols <- Cyclic.parseOwnedSymbols DomainA cyclicSymbolsPath
  timerSymbols <- Timer.parseOwnedSymbols timerSymbolsPath
  image4Digest <- sha256File image4Path
  loaderADigest <- sha256File loaderAPath
  loaderBDigest <- sha256File loaderBPath
  appDigest <- sha256File appPath
  cyclicDigest <- sha256File cyclicPath
  timerDigest <- sha256File timerPath
  unless (sha256Hex image4Digest == "f2e2acab892dafeed85171f6870a411f0a2d5eae10921b86a24630d401dcff16") (failH (image4Path ++ " is not the pinned X-M1 1.50 image4"))
  unless (sha256Hex loaderADigest == "21366ffff7cebc6176b8a8125bf3a9974835a51070a58ea325f7eed8aabd8882") (failH (loaderAPath ++ " is not the pinned X-M1 1.50 loader A"))
  unless (sha256Hex loaderBDigest == "8fb0c322a22443a3783cdab9071db17212a5629f6375646218ac1e043dc672ec") (failH (loaderBPath ++ " is not the pinned X-M1 1.50 loader B"))
  unless (B.length app == 0x384) (failH (appPath ++ " is not the 0x384-byte app-callback image"))
  unless (B.length cyclic == 0x1ac0) (failH (cyclicPath ++ " is not the 0x1ac0-byte cyclic A image"))
  unless (B.length timer == 0x4dc) (failH (timerPath ++ " is not the 0x4dc-byte timer A image"))
  checkStorageBudget (takeDirectory resultsPath)
  createDirectoryIfMissing True cacheRoot
  createDirectoryIfMissing True (takeDirectory resultsPath)
  let inputs =
        Inputs
          { inImage4 = image4
          , inLoaderA = loaderA
          , inLoaderB = loaderB
          , inApp = app
          , inCyclic = cyclic
          , inTimer = timer
          , inAppSymbols = appSymbols
          , inCyclicSymbols = cyclicSymbols
          , inTimerSymbols = timerSymbols
          }
  (runs, controls) <- runJoin runner (takeDirectory resultsPath) cacheRoot inputs
  let failures = [(runStage run, runRole run, message) | run <- runs, message <- runDifferences run]
      failedChecks = [(runStage run, runRole run, description) | run <- runs, (description, False) <- runChecks run]
      failedControls = [controlName control | control <- controls, not (controlHolds control)]
      caseValue run =
        object
          [ "stage" .= runStage run
          , "role" .= runRole run
          , "checks" .= [object ["check" .= description, "holds" .= holds] | (description, holds) <- runChecks run]
          , "differences" .= runDifferences run
          , "reuse" .= show (runReuse run)
          ]
      controlValue control =
        object
          [ "control" .= controlName control
          , "case" .= controlCase control
          , "holds" .= controlHolds control
          ]
      document =
        object
          [ "schema" .= (1 :: Int)
          , "inputs" .=
              object
                [ "image4_sha256" .= sha256Hex image4Digest
                , "loader_a_sha256" .= sha256Hex loaderADigest
                , "loader_b_sha256" .= sha256Hex loaderBDigest
                , "app_bin_sha256" .= sha256Hex appDigest
                , "cyclic_bin_sha256" .= sha256Hex cyclicDigest
                , "timer_bin_sha256" .= sha256Hex timerDigest
                , "app_functions" .= length appSymbols
                , "cyclic_functions" .= length cyclicSymbols
                , "timer_functions" .= length timerSymbols
                ]
          , "cases" .= map caseValue runs
          , "controls" .= map controlValue controls
          , "failures" .= (length failures + length failedChecks + length failedControls)
          ]
  BL.writeFile resultsPath (encode document)
  mapM_ (\(stage, role, message) -> putStrLn (stage ++ " " ++ role ++ ": " ++ message)) failures
  mapM_ (\(stage, role, description) -> putStrLn (stage ++ " " ++ role ++ ": check failed: " ++ description)) failedChecks
  mapM_ (\name -> putStrLn ("control failed: " ++ name)) failedControls
  putStrLn ("join cases: " ++ show (length runs) ++ ", checks: " ++ show (sum (map (length . runChecks) runs)) ++ ", controls: " ++ show (length controls) ++ ", failures: " ++ show (length failures + length failedChecks + length failedControls))
  unless (null failures && null failedChecks && null failedControls) exitFailure

required :: String -> IO FilePath
required flag = do
  arguments <- getArgs
  case dropWhile (/= flag) arguments of
    [] -> failH (flag ++ " is required")
    [_] -> failH (flag ++ " needs a value")
    _ : value : _ -> pure value
