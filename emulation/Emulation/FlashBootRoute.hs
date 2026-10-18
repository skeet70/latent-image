-- | The flash-boot check's fresh-settings route and its judgement of the
-- upstream @probe_setup_menus.py --interactive@ transcript.
module Emulation.FlashBootRoute
  ( Sha256
  , parseSha256
  , sha256Text
  , MenuState (..)
  , shootingMenu
  , Phase (..)
  , phaseName
  , RouteStep
  , freshBootRoute
  , probeCommand
  , Observation (..)
  , CheckFailure (..)
  , Milestone (..)
  , DriveResult (..)
  , judgeProbeOutput
  ) where

import Data.Aeson (Value)
import Data.Char (isDigit)
import Data.List (isPrefixOf, stripPrefix)
import Data.Maybe (listToMaybe, mapMaybe)
import Text.Read (readMaybe)

-- | 64 lowercase hex digits.
newtype Sha256 = Sha256 String deriving (Eq, Show)

parseSha256 :: String -> Maybe Sha256
parseSha256 text
  | length text == 64 && all (`elem` ("0123456789abcdef" :: String)) text = Just (Sha256 text)
  | otherwise = Nothing

sha256Text :: Sha256 -> String
sha256Text (Sha256 text) = text

newtype Instructions = Instructions Integer

-- | The firmware's retained current menu node: menu id, setting id, cursor.
data MenuState = MenuState Integer Integer Integer deriving (Eq, Show)

shootingMenu :: MenuState
shootingMenu = MenuState 1 5 2

data RouteKey = KeyUp | KeyOk | KeyMenu

data RouteStep = Press RouteKey | Continue Instructions

data Phase = LanguagePhase | DatePhase | LiveViewPhase | ShootingMenuPhase deriving (Eq, Show)

phaseName :: Phase -> String
phaseName LanguagePhase = "language"
phaseName DatePhase = "date"
phaseName LiveViewPhase = "live-view"
phaseName ShootingMenuPhase = "shooting-menu"

-- | The exercised erased-settings route of the interactive viewer. A probe
-- continuation is at most 15,000,000 instructions. The menu node is current
-- right after MENU; stock 1.50 showed the rendered menu 90,000,000 later.
freshBootRoute :: [(Phase, [RouteStep])]
freshBootRoute =
  [ (LanguagePhase, replicate 12 (Press KeyUp) ++ [Press KeyOk])
  , (DatePhase, [Press KeyOk])
  , (LiveViewPhase, map (Continue . Instructions) [15000000, 15000000, 10000000])
  , (ShootingMenuPhase, Press KeyMenu : replicate 6 (Continue (Instructions 15000000)))
  ]

probeCommand :: RouteStep -> String
probeCommand (Press KeyUp) = "key up"
probeCommand (Press KeyOk) = "key ok"
probeCommand (Press KeyMenu) = "key menu"
probeCommand (Continue (Instructions count)) = "run " ++ show count

data Observation = Observation
  { observedInstructions :: Integer
  , observedStop :: Maybe String
  , observedMenu :: Maybe MenuState
  , observedFrame :: Maybe Sha256
  }
  deriving (Show)

-- | One response of @probe_setup_menus.py --interactive@: a printed state, or
-- the one-line exception text of a refused command.
data ProbeResponse = ProbeState Observation | ProbeRefusal String

-- | The output before the interactive prompt line, which ends with the restored
-- snapshot's state, and one response per command after it.
data ProbeTranscript = ProbeTranscript [ProbeResponse] (Maybe [ProbeResponse])

parseProbeOutput :: String -> ProbeTranscript
parseProbeOutput output = case break ("Commands:" `isPrefixOf`) (lines output) of
  (before, _ : after) -> ProbeTranscript (responses before) (Just (responses after))
  (before, []) -> ProbeTranscript (responses before) Nothing
  where
    responses [] = []
    responses (line : rest)
      | Just status <- stripPrefix "instructions=" line =
          let (block, remaining) = span ("  " `isPrefixOf`) rest
           in maybe (ProbeRefusal ("unparsed probe state: " ++ line)) ProbeState (observation status block) : responses remaining
      | null line = responses rest
      | otherwise = ProbeRefusal line : responses rest
    observation status block = do
      let (count, afterCount) = span isDigit status
      instructions <- readMaybe count
      (_, afterFailure) <- breakOn " failure=" afterCount
      (failure, failureError) <- breakOn " error=" afterFailure
      let stops = [text | text <- [failure, failureError], text /= "None"]
          fields = concatMap words block
          field name = listToMaybe (mapMaybe (stripPrefix name) fields)
          menu = MenuState <$> (field "id=" >>= readMaybe) <*> (field "setting=" >>= readMaybe) <*> (field "cursor=" >>= readMaybe)
      pure (Observation instructions (if null stops then Nothing else Just (unwords stops)) menu (field "frame=" >>= parseSha256))

breakOn :: String -> String -> Maybe (String, String)
breakOn needle = go []
  where
    go before rest
      | Just after <- stripPrefix needle rest = Just (reverse before, after)
    go _ [] = Nothing
    go before (character : rest) = go (character : before) rest

data CheckFailure
  = BootStopped String (Maybe Value)
  | ModelStopped Phase String
  | InputRefused Phase String
  | DriveEnded Phase String
  | MenuMissed (Maybe MenuState)
  deriving (Show)

data Milestone = Milestone String Observation

data DriveResult = DriveResult [Milestone] (Maybe Observation) (Maybe CheckFailure)

-- | The drive's outcome from the probe's standard output; the first argument
-- describes the probe's exit for a transcript that ends early.
judgeProbeOutput :: String -> String -> DriveResult
judgeProbeOutput probeError = evaluateDrive probeError . parseProbeOutput

-- | Pairs each route step with the probe's response to it.
evaluateDrive :: String -> ProbeTranscript -> DriveResult
evaluateDrive probeError (ProbeTranscript before after) = case (reverse before, after) of
  (ProbeState start : _, Just responses) -> case observedStop start of
    Just stop -> DriveResult [Milestone "snapshot" start] (Just start) (Just (ModelStopped LanguagePhase stop))
    Nothing -> walk steps responses start [Milestone "snapshot" start]
  (ProbeRefusal message : _, _) -> DriveResult [] Nothing (Just (InputRefused LanguagePhase message))
  _ -> DriveResult [] Nothing (Just (DriveEnded LanguagePhase ("snapshot restore: " ++ probeError)))
  where
    steps =
      [ (phase, step, index == length phaseSteps)
      | (phase, phaseSteps) <- freshBootRoute
      , (index, step) <- zip [1 :: Int ..] phaseSteps
      ]
    walk [] _ final reached
      | observedMenu final == Just shootingMenu = DriveResult (reverse reached) (Just final) Nothing
      | otherwise = DriveResult (reverse reached) (Just final) (Just (MenuMissed (observedMenu final)))
    walk ((phase, step, closes) : more) remaining previous reached = case remaining of
      [] -> DriveResult (reverse reached) (Just previous) (Just (DriveEnded phase (probeCommand step ++ ": " ++ probeError)))
      ProbeRefusal message : _ -> DriveResult (reverse reached) (Just previous) (Just (InputRefused phase (probeCommand step ++ ": " ++ message)))
      ProbeState observed : rest -> case observedStop observed of
        Just stop -> DriveResult (reverse (Milestone (phaseName phase) observed : reached)) (Just observed) (Just (ModelStopped phase stop))
        Nothing -> walk more rest observed (if closes then Milestone (phaseName phase) observed : reached else reached)

