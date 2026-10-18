-- | Judgement of synthetic transcripts in the format @probe_setup_menus.py
-- --interactive@ prints: its state before and after the prompt line, then one
-- response per route command.
module Main (main) where

import Control.Monad (unless)
import Emulation.FlashBootRoute
import System.Exit (exitFailure)

-- | One printed state; @stop@ is the Python repr of the failure, or @None@.
state :: Integer -> String -> MenuState -> [String]
state instructions stop (MenuState menu setting cursor) =
  [ "instructions=" ++ show instructions ++ " ticks=1 failure=" ++ stop ++ " error=None"
  , "  ADC={'8': 1023, '9': 1023} pressed=None UI=5/0/0 menu=0x785088 id=" ++ show menu
      ++ " type=0 setting=" ++ show setting ++ " cursor=" ++ show cursor
      ++ " subcursor=0 visible=3 selected={'index': 0, 'value': 1, 'child_pointer': 0, 'status': 0, 'visible': True, 'selectable': True}"
  , "  frame=" ++ replicate 64 'a' ++ " output=.tools/flash-boot-drive"
  , "  Frame identity and retained menu pointer alone do not establish visible menu coverage."
  ]

language, live :: MenuState
language = MenuState 69 52 12
live = MenuState 67 56 0

routeLength :: Int
routeLength = length [() | (_, steps) <- freshBootRoute, _ <- steps]

-- | A restored snapshot followed by the given responses.
transcript :: [[String]] -> String
transcript responses = unlines (concat
  ([state 80000280 "None" language, state 80000280 "None" language, ["Commands: key NAME, run COUNT, state, quit."]] ++ responses))

clean :: Int -> [[String]]
clean count = [state (80000280 + toInteger index) "None" live | index <- [1 .. count]]

finalState :: MenuState -> [[String]]
finalState menu = clean (routeLength - 1) ++ [state 450019207 "None" menu]

failureOf :: String -> Maybe CheckFailure
failureOf output = let DriveResult _ _ failure = judgeProbeOutput "probe exit: ExitFailure 1" output in failure

expect :: String -> Bool -> IO Bool
expect name ok = do
  putStrLn ((if ok then "ok   " else "FAIL ") ++ name)
  pure ok

main :: IO ()
main = do
  results <-
    sequence
      [ expect "a route ending at SHOOTING MENU passes" $
          case judgeProbeOutput "" (transcript (finalState shootingMenu)) of
            DriveResult _ (Just final) Nothing -> observedMenu final == Just shootingMenu
            _ -> False
      , expect "a passing route records the snapshot and each phase" $
          let DriveResult milestones _ _ = judgeProbeOutput "" (transcript (finalState shootingMenu))
           in [name | Milestone name _ <- milestones] == "snapshot" : map (phaseName . fst) freshBootRoute
      , expect "a route ending outside SHOOTING MENU fails with the reached menu" $
          case failureOf (transcript (finalState live)) of
            Just (MenuMissed (Just reached)) -> reached == live
            _ -> False
      , expect "a model stop during the date prompt fails in that phase" $
          let stop = "{'message': 'CPU engine exception', 'cpu': 0, 'pc': '0x69cdf8'}"
              refused = ["ValueError: Restore the checkpoint before another input after a strict stop"]
              responses = clean 13 ++ [state 300000000 stop live] ++ replicate (routeLength - 14) refused
           in case failureOf (transcript responses) of
                Just (ModelStopped DatePhase _) -> True
                _ -> False
      , expect "a model stop in the restored snapshot fails before any input" $
          let output = unlines (concat [state 80000280 "{'message': 'stopped'}" language, ["Commands: key NAME, run COUNT, state, quit."]])
           in case failureOf output of
                Just (ModelStopped LanguagePhase _) -> True
                _ -> False
      , expect "a refused command fails in its phase" $
          case failureOf (transcript (clean 15 ++ [["ValueError: Continuation limit must be 0..15000000"]])) of
            Just (InputRefused LiveViewPhase _) -> True
            _ -> False
      , expect "a transcript that ends early fails in the unanswered phase" $
          case failureOf (transcript (clean 5)) of
            Just (DriveEnded LanguagePhase _) -> True
            _ -> False
      , expect "a probe that never reaches its prompt fails" $
          case failureOf "" of
            Just (DriveEnded _ _) -> True
            _ -> False
      ]
  unless (and results) exitFailure
