{-# LANGUAGE OverloadedStrings #-}

-- | Stock-against-owned cases for the owned playback machines. Each case enters the retained
-- dispatcher 0x011e8bf8 for one machine with the event and state that select one action, over
-- scripted retained services, and expects the result, the ordered service calls and the final
-- guest state of an independent model of the stock action bytes. Machine 0's model is below;
-- the others are path models.
module Replacement.PlaybackActor.Cases
  ( code
  , scenarios
  , OwnedMachine (..)
  , ownedMachines
  ) where

import Data.Bits (shiftR)
import qualified Data.ByteString as B
import Data.Word (Word32)
import Replacement.FileService.Native
import Replacement.PlaybackActor.Machine30 (machine30)
import Replacement.PlaybackActor.Model (machineScenarios)

plainBase :: Word32
plainBase = 0x01100000

-- | An owned machine: its export prefix, its stock action starts in table order with the end of
-- its extent, and its first republished table row.
data OwnedMachine = OwnedMachine
  { ownedName :: String
  , ownedExport :: String
  , ownedActions :: [Word32]
  , ownedEnd :: Word32
  , ownedRows :: Word32
  }

ownedMachines :: [OwnedMachine]
ownedMachines =
  [ OwnedMachine "machine0" "xm1_playback_m0_action" actionAddresses 0x011ea434 0x01218ec8
  , OwnedMachine "machine30" "xm1_playback_m30_action"
      [ 0x011f31bc, 0x011f31d4, 0x011f320c, 0x011f3244, 0x011f325c, 0x011f3278, 0x011f3294, 0x011f3338
      , 0x011f3370, 0x011f33a8, 0x011f33c0, 0x011f33dc, 0x011f351c, 0x011f3530, 0x011f3620, 0x011f3644
      , 0x011f37f4, 0x011f3868, 0x011f3a60, 0x011f3c3c, 0x011f3cac, 0x011f3d8c, 0x011f3f5c, 0x011f40cc
      , 0x011f41a0, 0x011f41d8, 0x011f425c, 0x011f42f8, 0x011f4410, 0x011f44b0, 0x011f4524, 0x011f45e4
      , 0x011f4698, 0x011f4758
      ]
      0x011f4810
      0x01219890
  ]

rowsEnd :: OwnedMachine -> Word32
rowsEnd m = ownedRows m + 4 * fromIntegral (length (ownedActions m))

ownedLow :: OwnedMachine -> Word32
ownedLow m = case ownedActions m of
  low : _ -> low
  [] -> ownedEnd m

-- | The dispatcher, the machine tables it reads and each owned machine's action extent and
-- table rows.
code :: Code
code = Code
  { codeImageBase = plainBase
  , codeExtents =
      concat
        [ [(ownedName m ++ "-actions", Extent (ownedLow m) (ownedEnd m)), (ownedName m ++ "-table", Extent (ownedRows m) (rowsEnd m))]
        | m <- ownedMachines
        ]
  , codeStockFunctions =
      concat [zip starts (zipWith (-) (drop 1 starts ++ [ownedEnd m]) starts) | m <- ownedMachines, let starts = ownedActions m]
  , codeRetained =
      ("dispatcher", Extent dispatcher 0x011e8cd4)
        : [ ("machine-tables-" ++ show n, Extent low high)
          | (n, (low, high)) <- zip [0 :: Int ..] (gaps tablesLow [(ownedRows m, rowsEnd m) | m <- ownedMachines] tablesHigh)
          , low < high
          ]
  , codeRetainedFunctions = [(dispatcher, 220)]
  }
  where
    tablesLow = 0x01215510
    tablesHigh = 0x0121a6dc
    gaps low holes high = case [h | h <- holes, fst h >= low] of
      [] -> [(low, high)]
      hs -> let (s, e) = minimum hs in (low, s) : gaps e [h | h <- holes, fst h >= e] high

dispatcher :: Word32
dispatcher = 0x011e8bf8

-- | Machine 0's table indices 2..32.
actionAddresses :: [Word32]
actionAddresses =
  [ 0x011e8ce4, 0x011e8da4, 0x011e8db8, 0x011e8e48, 0x011e8e9c, 0x011e8f3c, 0x011e8fd4, 0x011e906c
  , 0x011e9104, 0x011e9170, 0x011e919c, 0x011e91b4, 0x011e9370, 0x011e94c0, 0x011e95ac, 0x011e9768
  , 0x011e98d8, 0x011e99b4, 0x011e9b08, 0x011e9b88, 0x011e9d8c, 0x011e9fa0, 0x011ea01c, 0x011ea030
  , 0x011ea044, 0x011ea05c, 0x011ea070, 0x011ea088, 0x011ea09c, 0x011ea154, 0x011ea1dc
  ]

-- Retained services --------------------------------------------------------------------

parameter, navigation, transition, machine20, bufferMode, effect, machine34, message :: Word32
parameter = 0x0120bd54
navigation = 0x0120c268
transition = 0x0120c7b4
machine20 = 0x0120da08
bufferMode = 0x0120dad8
effect = 0x0120e830
machine34 = 0x0120f120
message = 0x01212e48

publish, workState, storageBuffer, recordFlags, errorTrace, sourceError, notifyPrimary, consumeSecondary :: Word32
publish = 0x0120c650
workState = 0x0121ca80
storageBuffer = 0x0121cca4
recordFlags = 0x0121cd84
errorTrace = 0x0121cde4
sourceError = 0x0121ce80
notifyPrimary = 0x0121d578
consumeSecondary = 0x0121d5ac

notify081f, notify0819, notify084e, bindRecord, notify0850, notify084f, cancelStep, callbackReady :: Word32
notify081f = 0x0121d5e8
notify0819 = 0x0121d624
notify084e = 0x0121d660
bindRecord = 0x0121d694
notify0850 = 0x0121d6f8
notify084f = 0x0121d72c
cancelStep = 0x0121d7b0
callbackReady = 0x0121d7f8

post, modeOptions, resetWorkspaces, notify0821, trace, notify0825, notify0826, deviceNotification :: Word32
post = 0x0121e624
modeOptions = 0x0121e794
resetWorkspaces = 0x0121e884
notify0821 = 0x0121ea08
trace = 0x0121eb68
notify0825 = 0x0121eb94
notify0826 = 0x0121ebc8
deviceNotification = 0x0072b680

-- Guest state ----------------------------------------------------------------------------

-- | Machine event halfwords at +0, state halfwords at +0xc0, the mode block at +0x18c.
machinesBase :: Word32
machinesBase = 0x014ea4ec

machinesBytes :: Int
machinesBytes = 0x1c0

stateOffset, bufferStepOffset, modeValueOffset :: Int
stateOffset = 0xc0
bufferStepOffset = 0x18c + 0x18
modeValueOffset = 0x18c + 0x1c

modeValue :: Word32
modeValue = 0x5a3c0081

-- Model ------------------------------------------------------------------------------------

data Effect
  = SetState Word32
  | Void Word32
  | Work Word32
  | Packet Word32 Word32
  | ClearStep

-- | An action: its prelude (calls without a read result, statuses read and discarded), the
-- source its status comes from, and its arms; Nothing selects the default arm.
data Action = Action
  { actionPrelude :: [Prelude]
  , actionSource :: Maybe Word32
  , actionArms :: Maybe Int -> ([Effect], Word32)
  }

data Prelude = Invoke Word32 | Discard Word32

primary :: ([Effect], Word32)
primary = ([SetState 1, Void notifyPrimary, Work 1, Void publish], 0)

posted :: Maybe Word32 -> Word32 -> Word32 -> ([Effect], Word32)
posted state call result = (maybe [] (\s -> [SetState s]) state ++ [Void call, Void post], result)

abort :: ([Effect], Word32)
abort = ([SetState 2, Void post], 255)

packet081e, packet082d :: Effect
packet081e = Packet 0x81e modeValue
packet082d = Packet 0x82d 0

startWork :: Word32 -> ([Effect], Word32)
startWork n = ([SetState 6, Work n, Void publish], 0)

-- | Statuses 8, 12 and 255 of the actions at 0x011e8e9c..0x011e906c.
restart :: Maybe Int -> ([Effect], Word32)
restart status = case status of
  Just 8 -> primary
  Just 12 -> ([SetState 1, Void sourceError, Work 1, Void publish], 0)
  Just 255 -> abort
  _ -> ([], 0)

-- | Statuses 4, 8, 12 and 255 of the routing actions with their state writes.
common :: Maybe Int -> Maybe ([Effect], Word32)
common status = case status of
  Just 4 -> Just (posted (Just 2) notifyPrimary 1)
  Just 8 -> Just primary
  Just 12 -> Just (posted (Just 2) sourceError 1)
  Just 255 -> Just abort
  _ -> Nothing

values :: Maybe Int -> Maybe ([Effect], Word32)
values status = case status of
  Just 5 -> Just (posted (Just 2) notify0825 1)
  Just 6 -> Just (posted (Just 2) notify0826 1)
  _ -> Nothing

orElse :: Maybe a -> a -> a
orElse value fallback = maybe fallback id value

-- | Statuses 9, 13, 17, 15, 16 and the default of the machine-34 actions.
machine34Tail :: Bool -> Maybe Int -> ([Effect], Word32)
machine34Tail stateOn17 status = case status of
  Just 9 -> ([SetState 6, Void notifyPrimary], 0)
  Just 13 -> ([SetState 6, packet081e], 0)
  Just 17 -> ([SetState 6 | stateOn17] ++ [packet082d], 17)
  Just 15 -> posted (Just 2) notify081f 15
  Just 16 -> posted (Just 2) notify0819 16
  _ -> ([SetState 6], 0)

bufferTail :: Maybe Int -> ([Effect], Word32)
bufferTail status = case status of
  Just 13 -> ([packet081e], 0)
  Just 9 -> ([Void notifyPrimary], 0)
  Just 17 -> ([packet082d], 0)
  Just 15 -> posted (Just 2) notify081f 15
  _ -> ([], 0)

-- | Table indices 2..32 in order.
actions :: [Action]
actions =
  [ Action [Invoke resetWorkspaces] (Just parameter) $ \s -> case s of
      Just 8 -> primary
      Just 4 -> posted (Just 2) notifyPrimary 2
      Just 12 -> posted (Just 2) sourceError 1
      Just 255 -> abort
      _ -> ([Void callbackReady], 0)
  , Action [Invoke trace] Nothing (const ([], 0))
  , Action [] (Just parameter) $ \s -> case s of
      Just 8 -> primary
      Just 12 -> posted (Just 1) sourceError 1
      Just 255 -> abort
      _ -> ([], 0)
  , Action [] Nothing (const ([], 0)) -- 0x011e8e48 has its own scenarios
  , Action [] (Just navigation) restart -- 0x011e8e9c reads 0x0120e830 after the status
  , Action [] (Just bufferMode) restart
  , Action [] (Just message) restart
  , Action [] (Just machine34) restart
  , Action [] (Just machine20) $ \s -> case s of
      Just 8 -> primary
      Just 4 -> posted (Just 2) notifyPrimary 1
      _ -> ([], 0)
  , Action [Discard parameter, Invoke notifyPrimary] Nothing (const ([ClearStep, Void post], 2))
  , Action [] Nothing (const ([], 1))
  , Action [] (Just navigation) $ \s -> values s `orElse` case s of
      Just 10 -> startWork 2
      Just 11 -> startWork 3
      Just 9 -> ([Void notifyPrimary], 0)
      Just 20 -> ([SetState 3, Void notify084f], 0)
      Just 21 -> ([SetState 3, Void notify0850], 0)
      _ -> common s `orElse` ([], 0)
  , Action [] (Just bufferMode) $ \s -> common s `orElse` bufferTail s
  , Action [] (Just message) $ \s -> common s `orElse` case s of
      Just 9 -> ([SetState 5, Void notifyPrimary], 0)
      _ -> ([SetState 5], 0)
  , Action [] (Just machine34) $ \s -> common s `orElse` machine34Tail True s
  , Action [Invoke bindRecord, Invoke modeOptions] (Just navigation) $ \s -> values s `orElse` case s of
      Just 4 -> posted Nothing notifyPrimary 1
      Just 10 -> startWork 2
      Just 11 -> startWork 3
      Just 12 -> posted Nothing sourceError 1
      Just 255 -> abort
      Just 20 -> ([SetState 3, Void notify084f], 0)
      Just 21 -> ([SetState 3, Void notify0850], 0)
      _ -> ([SetState 3], 0)
  , Action [Invoke bindRecord] (Just bufferMode) $ \s -> case s of
      Just 4 -> posted Nothing notifyPrimary 1
      Just 15 -> posted Nothing notify081f 15
      Just 12 -> posted Nothing sourceError 1
      Just 255 -> abort
      Just 13 -> ([SetState 4, packet081e], 0)
      _ -> ([SetState 4], 0)
  , Action [Invoke bindRecord] (Just bufferMode) $ \s -> common s `orElse` bufferTail s
  , Action [Invoke bindRecord, Invoke modeOptions] (Just message) $ \s -> case s of
      Just 4 -> posted Nothing notifyPrimary 1
      Just 12 -> posted Nothing sourceError 1
      Just 255 -> abort
      _ -> ([SetState 5], 0)
  , Action [Invoke bindRecord, Invoke cancelStep] (Just machine34) $ \s -> common s `orElse` (values s `orElse` machine34Tail False s)
  , Action [Invoke bindRecord, Invoke cancelStep] (Just machine34) $ \s -> common s `orElse` (values s `orElse` machine34Tail True s)
  , Action [] (Just machine20) $ \s -> case s of
      Just 4 -> posted (Just 2) notifyPrimary 1
      Just 8 -> primary
      _ -> ([SetState 7], 0)
  , Action [Invoke storageBuffer] Nothing (const ([], 0))
  , Action [Invoke recordFlags] Nothing (const ([], 0))
  , Action [Invoke recordFlags, Invoke post] Nothing (const ([], 0))
  , Action [Invoke errorTrace] Nothing (const ([], 0))
  , Action [Invoke errorTrace, Invoke post] Nothing (const ([], 0))
  , Action [Invoke notify0821] Nothing (const ([], 0))
  , Action [] (Just parameter) $ \s -> case s of
      Just 4 -> posted (Just 2) notifyPrimary 2
      _ -> common s `orElse` ([], 0)
  , Action [] (Just transition) $ \s -> case s of
      Just 4 -> posted (Just 0) consumeSecondary 3
      Just 12 -> posted (Just 2) sourceError 1
      Just 255 -> abort
      _ -> ([], 0)
  , Action [] (Just machine34) $ \s -> common s `orElse` (values s `orElse` case s of
      Just 14 -> ([SetState 6, Void notify084e], 0)
      Just 18 -> posted (Just 2) notifyPrimary 1
      _ -> machine34Tail True s)
  ]

-- | Every status some action distinguishes, and 7, which none does.
statuses :: [Int]
statuses = [4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 20, 21, 255]

-- Cases ---------------------------------------------------------------------------------------

returning :: Word32 -> Int -> Service
returning address value = Service address [] [("r0", fromIntegral value)] [] []

void :: Word32 -> Service
void address = Service address [] [] [] []

effectServices :: Effect -> [Service]
effectServices e = case e of
  SetState _ -> []
  Void address -> [void address]
  Work n -> [Service workState [("r0", n)] [] [] []]
  Packet c v -> [Service deviceNotification [] [("r0", 0)] [AtRegister "r0" 0 (words32le [c, v])] []]
  ClearStep -> []

finalState :: Word32 -> [Effect] -> Word32
finalState initial = foldl step initial
  where
    step _ (SetState s) = s
    step s _ = s

-- | The event of machine 0 and the state it is in when the dispatcher selects table index i,
-- from the first cell of its transition table at 0x01219f00 naming i.
selector :: B.ByteString -> Int -> (Word32, Word32)
selector image index =
  case [ (fromIntegral (cell `div` 8), fromIntegral (cell `mod` 8))
       | cell <- [0 .. 8 * 32 - 1]
       , fromIntegral (B.index image (offset + 2 * cell)) == index
       ] of
    first : _ -> first
    [] -> error ("no machine-0 transition selects action " ++ show index)
  where
    offset = fromIntegral (0x01219f00 - plainBase)

halfword :: Word32 -> B.ByteString
halfword v = B.pack [fromIntegral v, fromIntegral (v `shiftR` 8)]

-- | The buffer-step word holds 0x0badf00d until an action clears it.
machines :: Word32 -> Word32 -> Bool -> B.ByteString
machines event state cleared =
  B.concat
    [ halfword event
    , B.replicate (stateOffset - 2) 0
    , halfword state
    , B.replicate (bufferStepOffset - stateOffset - 2) 0
    , words32le [if cleared then 0 else 0x0badf00d, modeValue]
    , B.replicate (machinesBytes - modeValueOffset - 4) 0
    ]

scenario :: B.ByteString -> String -> Int -> [Service] -> [Effect] -> Word32 -> Scenario
scenario image name index services effects result =
  Scenario
    { scenarioName = name
    , scenarioEntry = dispatcher
    , scenarioData =
        [DataRegion "machines" machinesBase (machines event state False) (machines event (finalState state effects) cleared)]
    , scenarioCalls = [Call dispatcher [0] [] True]
    , scenarioServices = services
    , scenarioResults = [result]
    , scenarioStop = Nothing
    }
  where
    (event, state) = selector image index
    cleared = any isClear effects
    isClear ClearStep = True
    isClear _ = False

preludeServices :: Prelude -> Service
preludeServices p = case p of
  Invoke address -> void address
  Discard address -> returning address 0x31

scenarios :: B.ByteString -> [Scenario]
scenarios image = machine0Scenarios image ++ machineScenarios image machine30

-- | Every machine-0 action with every status; the action at 0x011e8e48 also with every second
-- status after navigation status 8.
machine0Scenarios :: B.ByteString -> [Scenario]
machine0Scenarios image = concat (zipWith cases [2 ..] actions)
  where
    named index suffix = "machine0-action" ++ show index ++ suffix
    run index suffix leading (effects, result) =
      scenario image (named index suffix) index (leading ++ concatMap effectServices effects) effects result
    cases :: Int -> Action -> [Scenario]
    cases 5 _ =
      [ run 5 ("-nav" ++ show s) [returning effect 0x77, returning navigation s] ([], 0) | s <- statuses, s /= 8 ]
        ++ [ run 5 ("-nav8-transition" ++ show t) [returning effect 0x77, returning navigation 8, returning transition t]
               (if t == 4 then ([Void consumeSecondary, Void post], 3) else ([SetState 1], 0))
           | t <- statuses ]
    cases index action = case actionSource action of
      Nothing -> [run index "" (map preludeServices (actionPrelude action)) (actionArms action Nothing)]
      Just source ->
        [ run index ("-status" ++ show s) (map preludeServices (actionPrelude action) ++ [returning source s] ++ [returning effect 0x77 | index == 6]) (actionArms action (if s == 7 then Nothing else Just s))
        | s <- statuses
        ]
