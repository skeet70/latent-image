-- | The oracle's command line: the fixed grid, a generated corpus, or a replay of retained cases.
-- Generation-only flags are refused without @--generate@, and a flag without a value is an error
-- rather than an absent flag.
module Replacement.RtosOwnFirst.Command
  ( Command (..)
  , GenerateOptions (..)
  , ReplayOptions (..)
  , parseCommand
  ) where

import Control.Monad (unless, void, when)
import Data.List (find)
import Replacement.RtosOwnFirst.Controls
import Replacement.RtosOwnFirst.Generate
import Replacement.RtosOwnFirst.Oracle (slotTag)
import Replacement.RtosOwnFirst.Shrink

data Command
  = GridCommand
  | GenerateCommand GenerateOptions
  | ReplayCommand ReplayOptions

data GenerateOptions = GenerateOptions
  { optionSeed :: GenerationSeed
  , optionCount :: GenerationCount
  , optionBudget :: ShrinkBudget
  , optionControl :: Control
  }

data ReplayOptions = ReplayOptions
  { optionSource :: FilePath
  }
  deriving (Eq, Show)

parseCommand :: [String] -> Either String Command
parseCommand arguments = do
  let unknown = [argument | argument <- arguments, isFlag argument, argument `notElem` allowedFlags]
  unless (null unknown) (Left ("unknown option " ++ unwords unknown))
  mapM_ (\name -> void (flagValue name arguments)) commonValueFlags
  aggregateValues <- mapM (`flagValue` arguments) aggregateValueFlags
  when (any (/= Nothing) aggregateValues && any (== Nothing) aggregateValues)
    (Left "aggregate mode requires --aggregate-a-bin, --aggregate-a-symbols, --placements-a, --aggregate-b-bin, --aggregate-b-symbols and --placements-b")
  generate <- flagValue "--generate" arguments
  replay <- flagValue "--replay" arguments
  let timingOnly = "--timing-only" `elem` arguments
      generationOnly = ["--seed", "--shrink-budget", "--control"]
  case (generate, replay) of
    (Just _, Just _) -> Left "--generate and --replay are mutually exclusive"
    (Just countText, Nothing) -> do
      when timingOnly (Left "--generate cannot be combined with --timing-only")
      count <- parseGenerationCount countText
      seedArgument <- requiredFlag "--seed"
      seed <- parseGenerationSeed seedArgument
      budget <- maybe (Right defaultShrinkBudget) parseShrinkBudget =<< flagValue "--shrink-budget" arguments
      control <- controlFlag
      Right (GenerateCommand (GenerateOptions seed count budget control))
    (Nothing, Just source) -> do
      when timingOnly (Left "--replay cannot be combined with --timing-only")
      rejectGenerationOnly generationOnly
      Right (ReplayCommand (ReplayOptions source))
    (Nothing, Nothing) -> do
      rejectGenerationOnly generationOnly
      Right GridCommand
  where
    requiredFlag name = flagValue name arguments >>= maybe (Left ("missing " ++ name)) Right
    rejectGenerationOnly names = mapM_ (\name -> when (name `elem` arguments) (Left (name ++ " needs --generate"))) names
    controlFlag = do
      given <- flagValue "--control" arguments
      control <- case given of
        Nothing -> case [candidate | candidate <- mutants, controlSlot candidate == generationSlot] of
          candidate : _ -> Right candidate
          [] -> Left ("no control for " ++ slotTag generationSlot)
        Just name -> maybe (Left ("unknown control " ++ name)) Right (find ((== name) . controlName) mutants)
      when (controlSlot control /= generationSlot) (Left (controlName control ++ " does not test " ++ slotTag generationSlot))
      Right control

commonValueFlags, generationValueFlags :: [String]
commonValueFlags = ["--runner", "--regions", "--carriers", "--owned-a-bin", "--owned-a-symbols", "--owned-b-bin", "--owned-b-symbols", "--loader-a", "--loader-b", "--work", "--summary"] ++ aggregateValueFlags
generationValueFlags = ["--seed", "--shrink-budget", "--control"]

aggregateValueFlags :: [String]
aggregateValueFlags = ["--aggregate-a-bin", "--aggregate-a-symbols", "--placements-a", "--aggregate-b-bin", "--aggregate-b-symbols", "--placements-b"]

allowedFlags :: [String]
allowedFlags = "--generate" : "--replay" : "--timing-only" : commonValueFlags ++ generationValueFlags

isFlag :: String -> Bool
isFlag ('-' : '-' : _) = True
isFlag _ = False

-- | A flag is present only with a following value that is not itself a flag, and at most once.
flagValue :: String -> [String] -> Either String (Maybe String)
flagValue name = go Nothing
  where
    go found [] = Right found
    go found (argument : rest)
      | argument == name = case rest of
          [] -> Left (name ++ " requires a value")
          value : _
            | isFlag value -> Left (name ++ " requires a value")
          value : _ -> case found of
            Just _ -> Left (name ++ " is given more than once")
            Nothing -> go (Just value) rest
      | otherwise = go found rest
