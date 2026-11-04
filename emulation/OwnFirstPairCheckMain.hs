{-# LANGUAGE OverloadedStrings #-}
-- | own-first-pair-check: qualifies one observation, compares a stock and an owned observation, or
-- requires a guest control's refusal of the expected kind.
module Main (main) where

import Control.Monad (unless, when)
import Data.Aeson (Value, object, (.=))
import qualified Data.Aeson as A
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as BL
import Emulation.OwnFirstPairCheck
import Emulation.OwnFirstPairPlan
import System.Directory (doesPathExist)
import System.Environment (getArgs)
import System.Exit (die, exitFailure)
import System.FilePath ((</>))

usage :: String
usage = unlines
  [ "usage: own-first-pair-check qualify PLAN OBSERVATION_DIR OUTPUT"
  , "       own-first-pair-check compare STOCK_PLAN STOCK_OBSERVATION_DIR OWNED_PLAN OWNED_OBSERVATION_DIR OUTPUT"
  , "       own-first-pair-check control CONTROL_PLAN OBSERVATION_DIR OUTPUT" ]

load :: FilePath -> FilePath -> IO (B.ByteString, Plan, Observation)
load planPath directory = do
  planBytes <- B.readFile planPath
  plan <- either (die . ("own-first-pair-check: plan: " ++)) pure (decodePlan planBytes)
  observed <- B.readFile (directory </> "observation.json")
    >>= either (die . ("own-first-pair-check: observation: " ++)) pure . decodeObservation
  pure (planBytes, plan, observed)

write :: FilePath -> Value -> IO ()
write output value = do
  exists <- doesPathExist output
  when exists (die "own-first-pair-check: OUTPUT exists")
  BL.writeFile output (A.encode value <> "\n")

refusalValue :: Refusal -> Value
refusalValue refusal = object ["kind" .= show (refusalKind refusal), "refusal" .= show refusal]

qualified :: String -> FilePath -> FilePath -> IO Qualified
qualified what planPath directory = do
  (planBytes, plan, observed) <- load planPath directory
  either (\refusal -> die ("own-first-pair-check: " ++ what ++ " refused: " ++ show refusal)) pure (qualify planBytes plan observed)

main :: IO ()
main = getArgs >>= \arguments -> case arguments of
  ["qualify", planPath, directory, output] -> do
    (planBytes, plan, observed) <- load planPath directory
    case qualify planBytes plan observed of
      Right result -> do
        write output (object ["qualified" .= True, "result" .= qualifiedValue result])
        putStrLn ("own-first-pair-check: " ++ roleWire (qualifiedRole result) ++ " qualified over " ++ show (qualifiedCalls result) ++ " calls")
      Left refusal -> do
        write output (object ["qualified" .= False, "refusal" .= refusalValue refusal])
        putStrLn ("own-first-pair-check: refused: " ++ show refusal)
        exitFailure
  ["compare", stockPlan, stockDirectory, ownedPlan, ownedDirectory, output] -> do
    stock <- qualified "stock" stockPlan stockDirectory
    owned <- qualified "owned" ownedPlan ownedDirectory
    unless (qualifiedRole stock == StockRole && qualifiedRole owned == OwnedRole) (die "own-first-pair-check: compare takes a stock and an owned observation")
    differences <- either (\indices -> die ("own-first-pair-check: observation inputs " ++ show indices ++ " differ between the runs")) pure
      (compareRoutes stock owned)
    let step = maybe A.Null (\milestone -> object ["phase" .= milestonePhase milestone, "key" .= milestoneKey milestone
          , "expected" .= show (milestoneExpected milestone), "observed" .= show (milestoneObserved milestone)])
    write output (object
      [ "equal" .= null differences
      , "differences" .= [object ["step" .= differenceIndex d, "stock" .= step (differenceStock d), "owned" .= step (differenceOwned d)] | d <- differences]
      , "stock" .= qualifiedValue stock, "owned" .= qualifiedValue owned ])
    if null differences
      then putStrLn ("own-first-pair-check: route steps and menu states equal over " ++ show (length (qualifiedMilestones stock)) ++ " steps")
      else putStrLn ("own-first-pair-check: " ++ show (length differences) ++ " route steps differ") >> exitFailure
  ["control", planPath, directory, output] -> do
    (planBytes, plan, observed) <- load planPath directory
    unless (planRole plan `elem` [ControlRestoredRole, ControlWrongIRole]) (die "own-first-pair-check: control takes a control plan")
    let verdict = controlVerdict planBytes plan observed
        passed = controlHolds verdict
    write output (object
      [ "role" .= roleWire (planRole plan), "passed" .= passed
      , "refusal" .= fmap refusalValue (controlRefusal verdict), "observer_stopped" .= controlStopped verdict ])
    putStrLn ("own-first-pair-check: control " ++ (if passed then "holds: " else "does not hold: ")
      ++ maybe "qualified" show (controlRefusal verdict) ++ maybe "" ("; observer stopped: " ++) (fmap show (controlStopped verdict)))
    unless passed exitFailure
  _ -> die usage
