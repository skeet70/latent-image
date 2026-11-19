module Replacement.RtosScheduler.S3bService
  ( Service (..)
  , ServiceSelection
  , allServices
  , parseServiceSelection
  , selects
  ) where

import qualified Data.Set as Set

data Service = Activate | InterruptActivate | CancelActivation | Wake | InterruptWake | CancelWake deriving (Eq, Ord, Show, Enum, Bounded)

-- | Domain-B flag and service of each selected S3b service.
newtype ServiceSelection = ServiceSelection (Set.Set (Bool, Service)) deriving (Eq, Show)

allServices :: ServiceSelection
allServices = ServiceSelection (Set.fromList [(b, s) | b <- [False, True], s <- [minBound .. maxBound]])

-- | Parses a comma-separated list of @A:Service@ or @B:Service@.
parseServiceSelection :: String -> Either String ServiceSelection
parseServiceSelection text = ServiceSelection . Set.fromList <$> traverse item (splitOn ',' text)
  where
    item (domain : ':' : name) = (,) <$> parseDomain domain <*> parseService name
    item other = Left ("service selection item is not DOMAIN:SERVICE: " ++ show other)
    parseDomain 'A' = Right False
    parseDomain 'B' = Right True
    parseDomain other = Left ("service selection domain is not A or B: " ++ show other)
    parseService name = case [s | s <- [minBound .. maxBound], show s == name] of
      [s] -> Right s
      _ -> Left ("unknown S3b service: " ++ show name)

selects :: ServiceSelection -> Bool -> Service -> Bool
selects (ServiceSelection chosen) b s = Set.member (b, s) chosen

splitOn :: Char -> String -> [String]
splitOn separator text = case break (== separator) text of
  (part, []) -> [part]
  (part, _ : rest) -> part : splitOn separator rest
