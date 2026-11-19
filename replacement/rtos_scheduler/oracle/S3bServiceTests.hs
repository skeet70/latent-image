module Main (main) where

import Control.Monad (unless)
import Data.Either (isLeft)
import Replacement.RtosScheduler.S3bService
import System.Exit (exitFailure)

everyService :: [(Bool, Service)]
everyService = [(b, s) | b <- [False, True], s <- [minBound .. maxBound]]

main :: IO ()
main = do
  let chosen selection = [key | key@(b, s) <- everyService, selects selection b s]
      checks =
        [ ("named services only", fmap chosen (parseServiceSelection "B:Wake,B:CancelWake,B:InterruptWake") == Right [(True, Wake), (True, InterruptWake), (True, CancelWake)])
        , ("default selects all twelve", chosen allServices == everyService)
        , ("unknown service refused", isLeft (parseServiceSelection "B:Sleep"))
        , ("unknown domain refused", isLeft (parseServiceSelection "C:Wake"))
        ]
      failed = [name | (name, False) <- checks]
  unless (null failed) (mapM_ putStrLn failed >> exitFailure)
