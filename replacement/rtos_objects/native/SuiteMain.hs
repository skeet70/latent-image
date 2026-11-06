{-# LANGUAGE OverloadedStrings #-}

-- | Runs the F2 family cases in both entry domains and reports each case's differences.
module Main (main) where

import Control.Monad (forM_, unless)
import qualified Data.ByteString as B
import System.Environment (getArgs)
import System.Exit (exitFailure)
import System.FilePath ((</>))
import Replacement.RtosObjects.Suite (FamilyEnv (..), FamilyRun (..), runFamily)
import Replacement.RtosOwnFirst.Regions (Domain (..))

main :: IO ()
main = do
  [runner, directory, binA, binB] <- getArgs
  bytesA <- B.readFile binA
  bytesB <- B.readFile binB
  let envA = FamilyEnv DomainA 0x40054648 0x4005ab10 0x400c9c0 0x400d1b0 0x40054700 0x4005ab30 0x4005ab38
      envB = FamilyEnv DomainB 0x400d4648 0x400dab10 0x400dc9c0 0x400dd1b0 0x400d4700 0x400dab30 0x400dab38
  runA <- runFamily runner (directory </> "a") envA bytesA
  runB <- runFamily runner (directory </> "b") envB bytesB
  let runs = [runA, runB]
      failures =
        [(name, message) | run <- runs, (name, Left message) <- familyCases run]
          ++ [(name, unlines differences) | run <- runs, (name, Right differences) <- familyCases run, not (null differences)]
  forM_ runs $ \run -> forM_ (familyCases run) $ \(name, result) ->
    putStrLn (show (familyDomain run) ++ " " ++ name ++ ": " ++ either ("failed: " ++) (const "ok") result)
  forM_ failures $ \(name, message) -> putStrLn (name ++ ": " ++ message)
  unless (null failures) exitFailure
