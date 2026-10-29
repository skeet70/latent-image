-- | The independent save-facade expectation model. Values come from the
-- image-5 decompilation, not from the Rust candidate.
module Replacement.Save.Domain
  ( Kind (..)
  , kindWord
  , kindBank
  , kindSaveMode
  , kindVariant
  , kindIsThree
  , SelectorCase (..)
  , Gates (..)
  , Completion (..)
  , Wait (..)
  , Scenario (..)
  , Event (..)
  , Expectation (..)
  , scenarioDomain
  , expectation
  , frameBase
  , resultBase
  , statusBase
  , slotsBase
  , mailboxBase
  , stackBase
  , stackTop
  , exitBase
  , ownedBase
  , stockFacadeAddress
  , taskId
  , descriptorAddress
  , extentNameAddress
  ) where

import Data.Bits ((.&.))
import Data.Int (Int32)
import Data.List (intercalate)
import Data.Word (Word32)
import Numeric (showHex)

data Kind = K0 | K1 | K2 | K3 | K4
  deriving (Eq, Show)

kindWord :: Kind -> Word32
kindWord K0 = 0
kindWord K1 = 1
kindWord K2 = 2
kindWord K3 = 3
kindWord K4 = 4

kindBank :: Kind -> Word32
kindBank K1 = 1
kindBank K2 = 2
kindBank K3 = 2
kindBank K4 = 2
kindBank _ = 0

kindSaveMode :: Kind -> Word32
kindSaveMode K2 = 3
kindSaveMode K3 = 3
kindSaveMode K4 = 1
kindSaveMode _ = 0

kindVariant :: Kind -> Word32
kindVariant K3 = 1
kindVariant _ = 0

kindIsThree :: Kind -> Bool
kindIsThree K3 = True
kindIsThree _ = False

data SelectorCase = SelectorCase
  { selectorPresent :: Bool
  , selectorDispatch :: Word32
  , selectorProfile :: Word32
  }
  deriving (Eq, Show)

data Gates = Gates
  { gateFrameState :: Word32
  , gateCommand :: Word32
  , gatePreset :: Word32
  , gateLayout :: Word32
  }
  deriving (Eq, Show)

data Completion = Completion
  { completionCodes :: [Int32]
  , phaseTwoCallbacks :: Word32
  }
  deriving (Eq, Show)

data Wait = Wait
  { waitFlags :: Word32
  , phaseTwoFlags :: Word32
  }
  deriving (Eq, Show)

data Scenario = Scenario
  { scenarioId :: String
  , scenarioKind :: Kind
  , scenarioSelectors :: SelectorCase
  , scenarioGates :: Gates
  , scenarioInitialStatus :: Word32
  , scenarioCompletion :: Completion
  , scenarioWait :: Wait
  , scenarioReentry :: Bool
  , scenarioLatePhaseOne :: Maybe Int32
  }
  deriving (Eq, Show)

data Event = Event Word32 [(String, Word32)] [(String, Word32)]
  deriving (Eq, Show)

data Expectation = Expectation
  { expectedReturn :: Word32
  , expectedStatus :: Word32
  , expectedHandback :: Word32
  , expectedShortcut :: Bool
  , expectedMetadata :: Maybe Word32
  , expectedMode :: Word32
  , expectedVariant :: Maybe Word32
  , expectedEvents :: [Event]
  , expectedPhaseTwo :: Bool
  }
  deriving (Eq, Show)

frameBase, resultBase, statusBase, slotsBase, mailboxBase, stackBase, stackTop, exitBase, ownedBase :: Word32
frameBase = 0x00210000
resultBase = 0x00214000
statusBase = 0x0154d970
slotsBase = 0x0155ccb8
mailboxBase = 0x00215000
stackBase = 0x00216000
stackTop = 0x00217000
exitBase = 0x00218000
ownedBase = 0x00100000

stockFacadeAddress :: Word32
stockFacadeAddress = 0x00e518c0

taskId :: Word32
taskId = 0x45

descriptorAddress :: Word32 -> Word32
descriptorAddress bank = frameBase + 0x27d0 + bank * 0x150

extentNameAddress :: Word32
extentNameAddress = frameBase + 0x2f64

event :: Word32 -> [(String, Word32)] -> [(String, Word32)] -> Event
event = Event

presentEvent :: Bool -> Event
presentEvent present = event 0x0073abec [] [("r0", if present then 1 else 0)]

profileEvent :: Word32 -> Event
profileEvent profile = event 0x0073ac60 [] [("r0", profile)]

byteEvent :: Word32 -> Event
byteEvent value = event 0x0069f8fc [("r0", 0xc3)] [("r0", value)]

admission :: SelectorCase -> ([Event], Bool)
admission selectors
  | not (selectorPresent selectors) = ([presentEvent False], False)
  | dispatchShortcut = (base, True)
  | overrideShortcut = (base ++ overrideEvents, True)
  | otherwise = (base ++ overrideEvents ++ [profileEvent profile], profile == 0xa8)
  where
    profile = selectorProfile selectors
    base = presentEvent True : byteEvent (selectorDispatch selectors) : dispatchEvents
    (dispatchEvents, dispatchShortcut) = dispatch selectors
    (overrideEvents, overrideShortcut) = override selectors

dispatch :: SelectorCase -> ([Event], Bool)
dispatch selectors
  | selectorDispatch selectors /= 1 = ([], False)
  | profile == 0xa2 = ([first], True)
  | profile > 0x82 =
      if profile <= 0x87
        then ([first, second, third], True)
        else (let fourth = profileEvent profile in (if profile == 0xa4 then ([first, second, third, fourth], True) else ([first, second, third, fourth], False)))
  | otherwise =
      if profile == 0xa4
        then ([first, second, third], True)
        else ([first, second, third], False)
  where
    profile = selectorProfile selectors
    first = profileEvent profile
    second = profileEvent profile
    third = profileEvent profile

override :: SelectorCase -> ([Event], Bool)
override selectors
  | profile == 0xa5 = ([first, second], True)
  | profile == 0xa7 = ([first, second, third, fourth], True)
  | profile == 0xa9 = ([first, second, third, fourth, fifth, sixth], True)
  | otherwise = ([first, second, third, fourth, fifth, sixth], False)
  where
    profile = selectorProfile selectors
    first = presentEvent True
    second = profileEvent profile
    third = presentEvent True
    fourth = profileEvent profile
    fifth = presentEvent True
    sixth = profileEvent profile

expectation :: Scenario -> Expectation
expectation scenario
  | shortcut =
      Expectation
        { expectedReturn = 1
        , expectedStatus = initial
        , expectedHandback = initial
        , expectedShortcut = True
        , expectedMetadata = Nothing
        , expectedMode = kindSaveMode kind
        , expectedVariant = Nothing
        , expectedEvents = if scenarioReentry scenario then selectorEvents ++ selectorEvents else selectorEvents
        , expectedPhaseTwo = False
        }
  | otherwise =
      Expectation
        { expectedReturn = result
        , expectedStatus = finalStatus
        , expectedHandback = handbackStatus
        , expectedShortcut = False
        , expectedMetadata = Just metadataWord
        , expectedMode = kindSaveMode kind
        , expectedVariant = Just (kindVariant kind)
        , expectedEvents = if scenarioReentry scenario then singleEvents ++ singleEvents else singleEvents
        , expectedPhaseTwo = phaseTwo
        }
  where
    kind = scenarioKind scenario
    gates = scenarioGates scenario
    selectors = scenarioSelectors scenario
    initial = scenarioInitialStatus scenario
    completion = scenarioCompletion scenario
    wait = scenarioWait scenario
    bank = kindBank kind
    descriptor = descriptorAddress bank
    (selectorEvents, shortcut) = admission selectors
    metadataWord = if kind == K0 && gateFrameState gates == 1 then 1 else 0
    nameExtent =
      not (kindIsThree kind)
        && (kind == K0 || (kind == K1 && gateFrameState gates == 0))
        && gateLayout gates == 2
    copyExtent = not (kindIsThree kind) && bank == 2 && gateLayout gates == 2
    interrupted = waitFlags wait .&. 0x20 /= 0
    result = if interrupted then 0 else 1
    completionStatus = case reverse (completionCodes completion) of
      [] -> initial
      (code : _) -> statusOf code
    phaseTwo = interrupted && completionStatus == 0
    handbackStatus = if phaseTwo then 0 else completionStatus
    finalStatus = case scenarioLatePhaseOne scenario of
      Just code -> statusOf code
      Nothing -> handbackStatus
    callbacks = length (completionCodes completion)
    releaseCallbacks = fromIntegral (phaseTwoCallbacks completion)
    singleEvents =
      selectorEvents
        ++ [event 0x00e4e0b4 [("r0", frameBase), ("r1", metadataWord)] [("r0", 0)]]
        ++ [event 0x011d5850 [("r0", gatePreset gates)] [("r0", 1)] | gateCommand gates == 0]
        ++ [event 0x011d5934 [("r0", extentNameAddress), ("r1", descriptor)] [("r0", 0)] | nameExtent]
        ++ [event 0x007101e4 [("r0", 2)] [("r0", 0)]]
        ++ [event 0x00723cb8 [] [("r0", taskId)]]
        ++ replicate callbacks (event 0x00724558 [("r0", taskId), ("r1", 0x10)] [("r0", 0)])
        ++ [event 0x011d591c [("r0", extentNameAddress), ("r1", descriptor)] [("r0", 0)] | copyExtent]
        ++ if phaseTwo
          then
            event 0x00723cb8 [] [("r0", taskId)]
              : replicate
                releaseCallbacks
                (event 0x00724558 [("r0", taskId), ("r1", 0x10)] [("r0", 0)])
          else []

statusOf :: Int32 -> Word32
statusOf 0 = 0
statusOf (-4) = 1
statusOf _ = 2

scenarioDomain :: [Scenario]
scenarioDomain =
  kindScenarios
    ++ admissionScenarios
    ++ gateScenarios
    ++ extentScenarios
    ++ reentryScenarios
    ++ lifecycleScenarios
  where
    defaultGates = Gates 0 0 0 2
    ordinary = SelectorCase False 0 0
    kindScenarios =
      [ Scenario
          (ident ["kind", kindTag kind, completionLabel, statusTag initial])
          kind
          ordinary
          defaultGates
          initial
          completion
          wait
          False
          Nothing
      | kind <- [K0, K1, K2, K3, K4]
      , (completionLabel, completion, wait, initial) <- completionClasses
      ]
    admissionScenarios =
      [ Scenario
          (ident ["admit", selectorTag selectors, statusTag initial])
          K0
          selectors
          defaultGates
          initial
          (Completion [0] 0)
          (Wait 0x10 0x10)
          False
          Nothing
      | selectors <- admissionCases
      , initial <- [0, 2]
      ]
    gateScenarios =
      [ Scenario
          (ident ["gate", kindTag kind, stateTag state, commandTag command, presetTag preset, layoutTag layout])
          kind
          ordinary
          (Gates state command preset layout)
          0
          (Completion [0] 0)
          (Wait 0x10 0x10)
          False
          Nothing
      | kind <- [K0, K1]
      , state <- [0, 1]
      , command <- [0, 1]
      , preset <- [0, 1]
      , layout <- [2, 0]
      ]
    extentScenarios =
      [ Scenario
          (ident ["extent", kindTag kind, layoutTag layout])
          kind
          ordinary
          (Gates 0 0 0 layout)
          0
          (Completion [0] 0)
          (Wait 0x10 0x10)
          False
          Nothing
      | (kind, layout) <- [(K2, 0), (K4, 0), (K3, 2), (K1, 2), (K1, 0), (K2, 2), (K4, 2), (K3, 0)]
      ]
    reentryScenarios =
      [ Scenario
          "reentry-shortcut-stale"
          K0
          (SelectorCase True 1 0xa2)
          defaultGates
          2
          (Completion [] 0)
          (Wait 0x20 0x10)
          True
          Nothing
      , Scenario
          "reentry-interrupt-phase-two"
          K2
          ordinary
          defaultGates
          0
          (Completion [] 1)
          (Wait 0x20 0x10)
          True
          Nothing
      ]
    lifecycleScenarios =
      [ Scenario
          "lifecycle-late-callback"
          K2
          ordinary
          defaultGates
          0
          (Completion [0] 1)
          (Wait 0x20 0x10)
          False
          (Just (-4))
      ]

completionClasses :: [(String, Completion, Wait, Word32)]
completionClasses =
  [ ("lost-fresh", Completion [] 1, Wait 0x20 0x10, 0)
  , ("lost-stale", Completion [] 0, Wait 0x20 0x10, 2)
  , ("code0", Completion [0] 0, Wait 0x10 0x10, 0)
  , ("code-minus4", Completion [-4] 0, Wait 0x10 0x10, 0)
  , ("code-minus1", Completion [-1] 0, Wait 0x10 0x10, 0)
  , ("code0-minus4", Completion [0, -4] 0, Wait 0x10 0x10, 0)
  , ("code-minus4-0", Completion [-4, 0] 0, Wait 0x10 0x10, 0)
  , ("code0-interrupt", Completion [0] 1, Wait 0x30 0x10, 0)
  , ("code-minus4-interrupt", Completion [-4] 0, Wait 0x30 0x10, 0)
  , ("code0-twice", Completion [0, 0] 0, Wait 0x10 0x10, 0)
  ]

admissionCases :: [SelectorCase]
admissionCases =
  [ SelectorCase True 1 0xa2
  , SelectorCase True 1 0xa4
  , SelectorCase True 1 0x83
  , SelectorCase True 1 0x87
  , SelectorCase True 1 0x82
  , SelectorCase True 1 0x88
  , SelectorCase True 1 0x80
  , SelectorCase True 0 0xa2
  , SelectorCase True 0 0xa5
  , SelectorCase True 0 0xa7
  , SelectorCase True 0 0xa9
  , SelectorCase True 0 0xa8
  ]

ident :: [String] -> String
ident parts = "s-" ++ intercalate "-" parts

kindTag :: Kind -> String
kindTag K0 = "k0"
kindTag K1 = "k1"
kindTag K2 = "k2"
kindTag K3 = "k3"
kindTag K4 = "k4"

statusTag :: Word32 -> String
statusTag value = "st" ++ showHex value ""

selectorTag :: SelectorCase -> String
selectorTag selectors =
  (if selectorPresent selectors then "p" else "n")
    ++ "-d"
    ++ showHex (selectorDispatch selectors) ""
    ++ "-q"
    ++ showHex (selectorProfile selectors) ""

stateTag, commandTag, presetTag, layoutTag :: Word32 -> String
stateTag value = "s" ++ showHex value ""
commandTag value = "c" ++ showHex value ""
presetTag value = "p" ++ showHex value ""
layoutTag value = "l" ++ showHex value ""
