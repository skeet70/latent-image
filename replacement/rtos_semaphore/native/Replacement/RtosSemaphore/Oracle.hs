{-# LANGUAGE OverloadedStrings #-}

-- | The semaphore family native cases: the stock layout each case builds, the seven stock entries
-- and their owned replacements, the batch runs and the per-case comparison.
module Replacement.RtosSemaphore.Oracle
  ( Facts (..)
  , DomainFacts (..)
  , ContextFacts (..)
  , LockFacts (..)
  , TaskFacts (..)
  , DefinitionFacts (..)
  , HeadFacts (..)
  , ConfigFacts (..)
  , ControlFacts (..)
  , WaitBlockFacts (..)
  , WaitQueueFacts (..)
  , StateFacts (..)
  , parseFacts
  , Domain (..)
  , domainLabel
  , Expectation (..)
  , Check (..)
  , Case (..)
  , Service (..)
  , Image (..)
  , Run (..)
  , extents
  , entries
  , runFamily
  ) where

import Control.Monad (forM, unless)
import Data.Aeson (FromJSON (..), eitherDecodeStrict', withObject, (.:))
import Data.Bits ((.&.), (.|.), complement, shiftL, shiftR)
import qualified Data.ByteString as B
import qualified Data.Map.Strict as M
import Data.List (isPrefixOf, sortOn)
import Data.Maybe (isJust)
import Data.Word (Word32, Word8)
import Emulation.CachedNativeBatch (BatchReuse, cachedObservedBatch)
import Emulation.NativeCase (caseIdText)
import qualified Emulation.NativeObservation as N
import Emulation.NativeObservation
  ( KernelObservation (..)
  , NativeObservation (..)
  , Ordered (..)
  , Register (..)
  , RegionId (..)
  , RunnerStatus (..)
  , Write (..)
  , WriteValue (..)
  , WriteBytes (..)
  )
import qualified Replacement.RtosPlatform.Aggregate as Agg
import Emulation.OwnedCase
  ( CaseOutcome (..)
  , CaseSpec (..)
  , InstructionLimit (..)
  , KernelCase (..)
  , Permission (..)
  , caseOutcome
  , mkRegion
  , mkStackContext
  , plainLimit
  , readWordAt
  , word32le
  , writeBatch
  )
import System.FilePath ((</>))

data Domain = DomainA | DomainB deriving (Eq, Ord, Show)

newtype GuestAddress = GuestAddress Word32 deriving (Eq, Ord, Show)

domainLabel :: Domain -> String
domainLabel DomainA = "a"
domainLabel DomainB = "b"

peer :: Domain -> Domain
peer DomainA = DomainB
peer DomainB = DomainA

data DomainFacts = DomainFacts
  { dfId :: Word32
  , dfSchedulerLock :: Word32
  , dfObjectLock :: Word32
  , dfContexts :: Word32
  , dfReadyHeads :: Word32
  , dfReadyBitmap :: Word32
  , dfRunning :: Word32
  , dfDispatchEnabled :: Word32
  , dfSelected :: Word32
  , dfTaskTable :: Word32
  , dfContextA :: Word32
  , dfContextB :: Word32
  , dfHandlerContext :: Word32
  , dfCurrentContext :: Word32
  , dfReschedule :: Word32
  , dfTimerBase :: Word32
  , dfTimerCount :: Word32
  , dfTimerHeap :: Word32
  , dfTickNow :: Word32
  , dfSemaphoreConfigs :: Word32
  , dfSemaphoreControls :: Word32
  , dfSemaphoreWaits :: Word32
  , dfSemaphoreTimers :: Word32
  }
  deriving (Eq, Show)

instance FromJSON DomainFacts where
  parseJSON = withObject "DomainFacts" $ \o ->
    DomainFacts
      <$> o .: "id" <*> o .: "scheduler_lock" <*> o .: "object_lock" <*> o .: "contexts"
      <*> o .: "ready_heads" <*> o .: "ready_bitmap" <*> o .: "running"
      <*> o .: "dispatch_enabled" <*> o .: "selected" <*> o .: "task_table"
      <*> o .: "context_a" <*> o .: "context_b" <*> o .: "handler_context"
      <*> o .: "current_context" <*> o .: "reschedule" <*> o .: "timer_base"
      <*> o .: "timer_count" <*> o .: "timer_heap" <*> o .: "tick_now"
      <*> o .: "semaphore_configs" <*> o .: "semaphore_controls"
      <*> o .: "semaphore_waits" <*> o .: "semaphore_timers"

data ContextFacts = ContextFacts
  { cfSize :: Word32
  , cfId :: Word32
  , cfOwner :: Word32
  , cfSchedulerLock :: Word32
  , cfObjectLock :: Word32
  , cfReadyHeads :: Word32
  , cfReadyBitmap :: Word32
  , cfRunning :: Word32
  , cfSelected :: Word32
  , cfDispatchEnabled :: Word32
  , cfFirstEntry :: Word32
  , cfTasks :: Word32
  , cfTaskCount :: Word32
  , cfSemaphores :: Word32
  , cfSemaphoreCount :: Word32
  , cfTimerHeap :: Word32
  , cfTimerCount :: Word32
  , cfTimerBase :: Word32
  , cfTickNow :: Word32
  }
  deriving (Eq, Show)

instance FromJSON ContextFacts where
  parseJSON = withObject "ContextFacts" $ \o ->
    ContextFacts
      <$> o .: "size" <*> o .: "id" <*> o .: "owner" <*> o .: "scheduler_lock"
      <*> o .: "object_lock" <*> o .: "ready_heads" <*> o .: "ready_bitmap"
      <*> o .: "running" <*> o .: "selected" <*> o .: "dispatch_enabled"
      <*> o .: "first_entry" <*> o .: "tasks" <*> o .: "task_count"
      <*> o .: "semaphores" <*> o .: "semaphore_count"
      <*> o .: "timer_heap" <*> o .: "timer_count" <*> o .: "timer_base" <*> o .: "tick_now"

data LockFacts = LockFacts {lfWord :: Word32, lfAcquire :: Word32, lfRelease :: Word32}
  deriving (Eq, Show)

instance FromJSON LockFacts where
  parseJSON = withObject "LockFacts" $ \o ->
    LockFacts <$> o .: "word" <*> o .: "acquire" <*> o .: "release"

data TaskFacts = TaskFacts
  { tfSize :: Word32
  , tfNext :: Word32
  , tfPrev :: Word32
  , tfDefinition :: Word32
  , tfState :: Word32
  , tfPriority :: Word32
  , tfFlags :: Word32
  , tfWait :: Word32
  , tfSavedSp :: Word32
  , tfResumePc :: Word32
  , tfDomain :: Word32
  , tfTerminatePending :: Word32
  , tfPriorityChangePending :: Word32
  , tfCancelPending :: Word32
  , tfContextDiscarded :: Word32
  , tfPendingPriority :: Word32
  }
  deriving (Eq, Show)

instance FromJSON TaskFacts where
  parseJSON = withObject "TaskFacts" $ \o ->
    TaskFacts
      <$> o .: "size" <*> o .: "next" <*> o .: "prev" <*> o .: "definition"
      <*> o .: "state" <*> o .: "priority" <*> o .: "flags" <*> o .: "wait"
      <*> o .: "saved_sp" <*> o .: "resume_pc" <*> o .: "domain"
      <*> o .: "terminate_pending" <*> o .: "priority_change_pending"
      <*> o .: "cancel_pending" <*> o .: "context_discarded"
      <*> o .: "pending_priority"

data DefinitionFacts = DefinitionFacts {defPriority :: Word32, defSlot :: Word32}
  deriving (Eq, Show)

instance FromJSON DefinitionFacts where
  parseJSON = withObject "DefinitionFacts" $ \o ->
    DefinitionFacts <$> o .: "priority" <*> o .: "slot"

data HeadFacts = HeadFacts {hfSize :: Word32, hfNext :: Word32, hfPrev :: Word32}
  deriving (Eq, Show)

instance FromJSON HeadFacts where
  parseJSON = withObject "HeadFacts" $ \o ->
    HeadFacts <$> o .: "size" <*> o .: "next" <*> o .: "prev"

data ConfigFacts = ConfigFacts
  { cfgSize :: Word32
  , cfgAttributes :: Word32
  , cfgInitial :: Word32
  , cfgMaximum :: Word32
  , cfgExists :: Word32
  , cfgPriorityOrder :: Word32
  }
  deriving (Eq, Show)

instance FromJSON ConfigFacts where
  parseJSON = withObject "ConfigFacts" $ \o ->
    ConfigFacts
      <$> o .: "size" <*> o .: "attributes" <*> o .: "initial" <*> o .: "maximum"
      <*> o .: "exists" <*> o .: "priority_order"

data ControlFacts = ControlFacts {ctlSize :: Word32, ctlQueue :: Word32, ctlCount :: Word32}
  deriving (Eq, Show)

instance FromJSON ControlFacts where
  parseJSON = withObject "ControlFacts" $ \o ->
    ControlFacts <$> o .: "size" <*> o .: "queue" <*> o .: "count"

data WaitBlockFacts = WaitBlockFacts
  { wbSize :: Word32
  , wbResult :: Word32
  , wbQueue :: Word32
  , wbOwner :: Word32
  }
  deriving (Eq, Show)

instance FromJSON WaitBlockFacts where
  parseJSON = withObject "WaitBlockFacts" $ \o ->
    WaitBlockFacts <$> o .: "size" <*> o .: "result" <*> o .: "queue" <*> o .: "owner"

data WaitQueueFacts = WaitQueueFacts
  { wqSize :: Word32
  , wqNext :: Word32
  , wqPrev :: Word32
  , wqConfig :: Word32
  }
  deriving (Eq, Show)

instance FromJSON WaitQueueFacts where
  parseJSON = withObject "WaitQueueFacts" $ \o ->
    WaitQueueFacts <$> o .: "size" <*> o .: "next" <*> o .: "prev" <*> o .: "config"

data StateFacts = StateFacts
  { stReady :: Word8
  , stWait :: Word8
  , stSuspended :: Word8
  , stSleeping :: Word8
  , stObjectQueued :: Word8
  , stOwnerLockQueued :: Word8
  }
  deriving (Eq, Show)

instance FromJSON StateFacts where
  parseJSON = withObject "StateFacts" $ \o ->
    StateFacts
      <$> o .: "ready" <*> o .: "wait" <*> o .: "suspended" <*> o .: "sleeping"
      <*> o .: "object_queued" <*> o .: "owner_lock_queued"

data Facts = Facts
  { factsA :: DomainFacts
  , factsB :: DomainFacts
  , factsOwnId :: Word32
  , factsContext :: ContextFacts
  , factsLock :: LockFacts
  , factsTask :: TaskFacts
  , factsDefinition :: DefinitionFacts
  , factsHead :: HeadFacts
  , factsPriorities :: Word32
  , factsTaskSlots :: Word32
  , factsSemaphoreSlots :: Word32
  , factsConfig :: ConfigFacts
  , factsControl :: ControlFacts
  , factsWaitBlock :: WaitBlockFacts
  , factsWaitQueue :: WaitQueueFacts
  , factsTimerEntrySize :: Word32
  , factsStates :: StateFacts
  }
  deriving (Eq, Show)

instance FromJSON Facts where
  parseJSON = withObject "Facts" $ \o ->
    Facts
      <$> o .: "a" <*> o .: "b" <*> o .: "own_id" <*> o .: "context" <*> o .: "lock"
      <*> o .: "task" <*> o .: "definition" <*> o .: "ready_head" <*> o .: "priorities"
      <*> o .: "task_slots" <*> o .: "semaphore_slots" <*> o .: "semaphore_config"
      <*> o .: "semaphore_control" <*> o .: "wait_block" <*> o .: "wait_queue"
      <*> o .: "timer_entry_size"
      <*> (StateFacts <$> o .: "state_ready" <*> o .: "state_wait" <*> o .: "state_suspended"
            <*> o .: "state_sleeping" <*> o .: "state_object_queued"
            <*> o .: "state_owner_lock_queued")

parseFacts :: B.ByteString -> Either String Facts
parseFacts = eitherDecodeStrict'

domainFacts :: Facts -> Domain -> DomainFacts
domainFacts facts DomainA = factsA facts
domainFacts facts DomainB = factsB facts

data Service = Create | Delete | Signal | InterruptSignal | Wait | Poll | TimedWait
  deriving (Eq, Show)

data Check = Equals GuestAddress Word32 | ByteEquals GuestAddress Word8 | NoWrite GuestAddress
  deriving (Eq, Show)

data Expectation = Expectation
  { expectationResult :: Word32
  , expectationArrival :: Bool
  , expectationSlot :: Word32
  , expectationChecks :: [Check]
  , expectationRefusal :: Maybe RunnerStatus
  , expectationBudget :: Maybe InstructionLimit
  , expectationDriverMask :: Maybe Word32
  }
  deriving (Eq, Show)

data Boundary = TaskArrival | CallerReturn deriving (Eq, Show)

data WakerTarget = WakerSignal | WakerDelete | WakerTick Int deriving (Eq, Show)

data Waker = Waker
  { wakerTarget :: WakerTarget
  , wakerArgs :: [(Register, Word32)]
  }

data Case = Case
  { caseName :: String
  , caseService :: Service
  , caseDomain :: Domain
  , caseSlot :: Word32
  , caseClass :: Maybe Word32
  , caseWords :: [(GuestAddress, Word32)]
  , caseTimeout :: Word32
  , caseStock :: Expectation
  , caseOwned :: Maybe Expectation
  , caseDifference :: Maybe String
  , caseHandler :: Bool
  , caseMasked :: Bool
  , caseDispatchEnabled :: Bool
  , caseBank :: Maybe (Domain, Word32)
  , caseReceipt :: Maybe (Word32, Word32)
  , caseBoundary :: Boundary
  , caseWaker :: Maybe Waker
  , caseMask :: Maybe Word32
  , caseEntryControl :: Maybe Word32
  , caseRestoresMask :: Bool
  , caseTimerIndex :: Maybe TimerIndexCell
  }

data Image = Image {imageBase :: Word32, imageBytes :: B.ByteString, imageSymbols :: [Agg.AggregateSymbol], imagePlacements :: [Agg.SectionPlacement]}

newtype TimerIndexCell = TimerIndexCell GuestAddress

data TimerContract = RecordingTimer | OwnedTimer deriving (Eq)

timerContract :: Image -> TimerContract
timerContract image
  | all (`elem` names) ["xm1_rtos_timer_remove_body", "xm1_rtos_timer_tick_body"] = OwnedTimer
  | otherwise = RecordingTimer
  where
    names = map Agg.symbolName (imageSymbols image)

data Run = Run
  { runDomain :: Domain
  , runCases :: [(String, Either String [String])]
  , runMatches :: Int
  , runReuse :: (BatchReuse, BatchReuse)
  , runControls :: [(String, Bool)]
  }

data Page = Page RegionId GuestAddress Int Permission B.ByteString

entries :: Domain -> Service -> Word32
entries DomainA service = case service of
  Create -> 0x4d2a8
  Delete -> 0x4d338
  Signal -> 0x47604
  InterruptSignal -> 0x47844
  Wait -> 0x47a54
  Poll -> 0x47c94
  TimedWait -> 0x47dd0
entries DomainB service = case service of
  Create -> 0xdd26c
  Delete -> 0xdd2fc
  Signal -> 0xd75c4
  InterruptSignal -> 0xd7804
  Wait -> 0xd7a14
  Poll -> 0xd7c54
  TimedWait -> 0xd7d90

extents :: Domain -> [(Word32, Word32)]
extents DomainA =
  [ (0x4d2a8, 0x90), (0x4d338, 0x64)
  , (0x47604, 0x240), (0x47844, 0x210), (0x47a54, 0x240), (0x47c94, 0x13c), (0x47dd0, 0x274)
  ]
extents DomainB =
  [ (0xdd26c, 0x90), (0xdd2fc, 0x64)
  , (0xd75c4, 0x240), (0xd7804, 0x210), (0xd7a14, 0x240), (0xd7c54, 0x13c), (0xd7d90, 0x274)
  ]

retained :: Domain -> [(Word32, Word32)]
retained DomainA =
  [ (0x4252c, 40), (0x42560, 8), (0x42568, 12), (0x4259c, 20), (0x425b0, 24)
  , (0x433e4, 96), (0x43444, 96), (0x444e4, 132)
  , (0x43640, 40), (0x43668, 32), (0x43688, 28), (0x436a4, 8), (0x436ac, 124), (0x43728, 52), (0x4375c, 72)
  , (0x437a4, 56), (0x437fc, 20), (0x439d8, 60), (0x43a14, 40), (0x44018, 44)
  , (0x44044, 240), (0x44134, 128)
  , (0x44568, 204), (0x446fc, 72), (0x44744, 188), (0x44800, 228), (0x448e4, 220)
  , (0x44ac0, 196), (0x44b84, 220)
  , (0x43c48, 92), (0x43ca4, 124), (0x43d20, 176), (0x43dd0, 72), (0x43f70, 44)
  , (0x44c60, 72), (0x44ca8, 24)
  ]
retained DomainB =
  [ (0xd252c, 40), (0xd2560, 8), (0xd2568, 12), (0xd259c, 20), (0xd25b0, 24)
  , (0xd33e4, 96), (0xd3444, 96), (0xd44a4, 132)
  , (0xd3640, 40), (0xd3668, 32), (0xd3688, 28), (0xd36a4, 8), (0xd36ac, 124), (0xd3728, 52), (0xd375c, 72)
  , (0xd37a4, 56), (0xd37fc, 20), (0xd398c, 60), (0xd39c8, 40), (0xd3fd4, 48)
  , (0xd4004, 240), (0xd40f4, 128)
  , (0xd4528, 204), (0xd46bc, 72), (0xd4704, 188), (0xd47c0, 228), (0xd48a4, 220)
  , (0xd4a80, 196), (0xd4b44, 220)
  , (0xd3c04, 92), (0xd3c60, 124), (0xd3cdc, 176), (0xd3d8c, 72), (0xd3f2c, 44)
  , (0xd4c20, 72), (0xd4c68, 24)
  ]

timerRemoval :: Domain -> Word32
timerRemoval DomainA = 0x44ca8
timerRemoval DomainB = 0xd4c68

timeoutCallback :: Domain -> Word32
timeoutCallback DomainA = 0x44318
timeoutCallback DomainB = 0xd42d8

callerStack, handlerStack, targetStack, exitAddress, stopAddress, receiptAddress, requestAddress, globalLock :: Word32
callerStack = 0x00959800
handlerStack = 0x00959a00
targetStack = 0x00959c00
exitAddress = 0x00958f00
stopAddress = 0x00959200
receiptAddress = 0x00960000
requestAddress = 0x0095a000
globalLock = 0x400c0804

wakerSlot, wakerPriority, wakerStack, wakerStubBase, maskAddress, dispatchMaskAddress :: Word32
wakerSlot = 2
wakerPriority = 14
wakerStack = 0x00970000
wakerStubBase = 0x00959080
maskAddress = receiptAddress + 0x10
dispatchMaskAddress = maskAddress + 4

definitionBase, definitionStride :: Word32
definitionBase = 0x0096c000
definitionStride = 0x40

timerToken :: Word32
timerToken = receiptAddress + 0x40

timerStub :: B.ByteString
timerStub = B.concat (map word32le [0xe3002000, 0xe3402096, 0xe5820000, 0xe5821004, 0xe3a00000, 0xe12fff1e])

bank :: Domain -> GuestAddress
bank DomainA = GuestAddress 0xfffb30cc
bank DomainB = GuestAddress 0xfffb40cc

callerBytes :: B.ByteString
callerBytes = B.concat [word32le (0xca11e500 + value) | value <- [0 .. 15]]

runningSlot, idleSlot :: Word32
runningSlot = 1
idleSlot = 3

taskAddress :: Facts -> Domain -> Word32 -> Word32
taskAddress facts domain slot = dfTaskTable (domainFacts facts domain) + (slot - 1) * tfSize (factsTask facts)

definitionAddress :: Facts -> Domain -> Word32 -> Word32
definitionAddress facts domain slot =
  definitionBase
    + (if domain == DomainA then 0 else fromIntegral (factsTaskSlots facts)) * definitionStride
    + (slot - 1) * definitionStride

configAddress, controlAddress, waitAddress, timerAddress :: Facts -> Domain -> Word32 -> Word32
configAddress facts domain slot = dfSemaphoreConfigs (domainFacts facts domain) + (slot - 1) * cfgSize (factsConfig facts)
controlAddress facts domain slot = dfSemaphoreControls (domainFacts facts domain) + (slot - 1) * ctlSize (factsControl facts)
waitAddress facts domain slot = dfSemaphoreWaits (domainFacts facts domain) + (slot - 1) * wbSize (factsWaitBlock facts)
timerAddress facts domain slot = dfSemaphoreTimers (domainFacts facts domain) + (slot - 1) * factsTimerEntrySize facts

countCell, existsCell, attributeCell, maximumCell, initialCell, queueCell :: Facts -> Domain -> Word32 -> Word32
countCell facts domain slot = controlAddress facts domain slot + ctlCount (factsControl facts)
existsCell facts domain slot = configAddress facts domain slot + cfgExists (factsConfig facts)
attributeCell facts domain slot = configAddress facts domain slot + cfgAttributes (factsConfig facts)
maximumCell facts domain slot = configAddress facts domain slot + cfgMaximum (factsConfig facts)
initialCell facts domain slot = configAddress facts domain slot + cfgInitial (factsConfig facts)
queueCell facts domain slot = controlAddress facts domain slot + ctlQueue (factsControl facts)

waitResult, waitQueueCell, waitOwner :: Facts -> Domain -> Word32 -> Word32
waitResult facts domain slot = waitAddress facts domain slot + wbResult (factsWaitBlock facts)
waitQueueCell facts domain slot = waitAddress facts domain slot + wbQueue (factsWaitBlock facts)
waitOwner facts domain slot = waitAddress facts domain slot + wbOwner (factsWaitBlock facts)

recordAddress :: Facts -> Domain -> Domain -> Word32
recordAddress facts executing described = case described of
  DomainA -> dfContextA (domainFacts facts executing)
  DomainB -> dfContextB (domainFacts facts executing)

poke :: Word32 -> Word32 -> (GuestAddress, Word32)
poke address value = (GuestAddress address, value)

eq :: Word32 -> Word32 -> Check
eq address value = Equals (GuestAddress address) value

byte :: Word32 -> Word8 -> Check
byte address value = ByteEquals (GuestAddress address) value

noWrite :: Word32 -> Check
noWrite address = NoWrite (GuestAddress address)

packedState :: Facts -> Word8 -> Word32 -> Word32
packedState _facts state priority = fromIntegral state .|. (priority `shiftL` 8)

queuedState :: Facts -> Word8
queuedState facts = stWait states .|. stObjectQueued states .|. stOwnerLockQueued states
  where
    states = factsStates facts

created :: Facts -> Domain -> Word32 -> Word32 -> Word32 -> Word32 -> [(GuestAddress, Word32)]
created facts domain slot attributes initial maxCount =
  [ poke (attributeCell facts domain slot) attributes
  , poke (initialCell facts domain slot) initial
  , poke (maximumCell facts domain slot) maxCount
  , poke (existsCell facts domain slot) 1
  , poke (countCell facts domain slot) initial
  , poke (queueCell facts domain slot) (controlAddress facts domain slot)
  , poke (queueCell facts domain slot + wqPrev (factsWaitQueue facts)) (controlAddress facts domain slot)
  , poke (queueCell facts domain slot + wqConfig (factsWaitQueue facts)) (configAddress facts domain slot)
  ]

waiterAt :: Facts -> Domain -> Word32 -> Domain -> Word32 -> Word32 -> [(GuestAddress, Word32)]
waiterAt facts owner objectSlot objectDomain slot priority =
  [ poke (taskAddress facts owner slot + tfState (factsTask facts)) (packedState facts (queuedState facts) priority)
  , poke (taskAddress facts owner slot + tfWait (factsTask facts)) (waitAddress facts owner slot)
  , poke (waitResult facts owner slot) 0
  , poke (waitQueueCell facts owner slot) (queueCell facts objectDomain objectSlot)
  , poke (waitOwner facts owner slot) (dfId (domainFacts facts objectDomain))
  , poke (queueCell facts objectDomain objectSlot) (taskAddress facts owner slot)
  , poke (queueCell facts objectDomain objectSlot + wqPrev (factsWaitQueue facts)) (taskAddress facts owner slot)
  , poke (taskAddress facts owner slot + tfNext (factsTask facts)) (queueCell facts objectDomain objectSlot)
  , poke (taskAddress facts owner slot + tfPrev (factsTask facts)) (queueCell facts objectDomain objectSlot)
  ]

plain :: String -> Service -> Domain -> Word32 -> [(GuestAddress, Word32)] -> Word32 -> Word32 -> Bool -> Word32 -> [Check] -> Case
plain name service domain slot wordList timeout result arrival selected checks =
  Case name service domain slot Nothing wordList timeout
    (Expectation result arrival selected checks Nothing Nothing Nothing) Nothing Nothing
    (service == InterruptSignal) False True Nothing Nothing
    TaskArrival Nothing Nothing Nothing (service == Create || service == Delete) Nothing

cases :: Facts -> Domain -> [Case]
cases facts executing =
  [ plain "create-fresh" Create executing 1
      [ poke (existsCell facts executing 1) 0
      , poke requestAddress 1, poke (requestAddress + 4) 3, poke (requestAddress + 8) 5
      ]
      0 0 False 0
      [ eq (attributeCell facts executing 1) 1, eq (initialCell facts executing 1) 3
      , eq (maximumCell facts executing 1) 5, eq (existsCell facts executing 1) 1
      , eq (countCell facts executing 1) 3
      ]
  , plain "create-exists" Create executing 1
      (created facts executing 1 0 1 1
        ++ [poke requestAddress 7, poke (requestAddress + 4) 9, poke (requestAddress + 8) 9])
      0 (fromIntegral (-41 :: Int)) False 0
      [ eq (attributeCell facts executing 1) 0, eq (countCell facts executing 1) 1 ]
  , (plain "create-range-refused" Create executing 251
      [ poke (existsCell facts executing 1) 0
      , poke requestAddress 1, poke (requestAddress + 4) 1, poke (requestAddress + 8) 1
      ]
      0 0 False 0 [])
      { caseStock = Expectation 0 False 0 [] (Just MemoryGuard) Nothing Nothing }
      { caseOwned = Just (Expectation (fromIntegral (-18 :: Int)) False 0 [] Nothing Nothing Nothing)
      , caseDifference = Just "semaphore-create-range-refused"
      }
  , plain "delete-plain" Delete executing 1 (created facts executing 1 1 4 6) 0 0 False 0
      [ eq (attributeCell facts executing 1) 0, eq (initialCell facts executing 1) 1
      , eq (maximumCell facts executing 1) 1, eq (existsCell facts executing 1) 0
      , eq (countCell facts executing 1) 1
      ]
  , (plain "delete-waiter" Delete executing 1
      (created facts executing 1 0 0 1 ++ waiterAt facts executing 1 executing 2 3)
      0 0 False 0
      [ eq (existsCell facts executing 1) 0, eq (countCell facts executing 1) 1
      , byte (taskAddress facts executing 2 + tfState (factsTask facts)) (queuedState facts)
      ])
      { caseOwned = Just (Expectation 0 True 2
          [ eq (existsCell facts executing 1) 0, eq (countCell facts executing 1) 1
          , byte (taskAddress facts executing 2 + tfState (factsTask facts)) (stReady (factsStates facts))
          , eq (waitResult facts executing 2) (fromIntegral (-49 :: Int))
          , eq (queueCell facts executing 1) (controlAddress facts executing 1)
           ] Nothing Nothing Nothing)
      , caseDifference = Just "semaphore-delete-releases-waiters"
      }
  , plain "signal-below-max" Signal executing 1 (created facts executing 1 0 0 2) 0 0 False 0
      [ eq (countCell facts executing 1) 1 ]
  , plain "signal-at-max" Signal executing 1 (created facts executing 1 0 2 2) 0
      (fromIntegral (-43 :: Int)) False 0
      [ eq (countCell facts executing 1) 2, noWrite (countCell facts executing 1) ]
  , plain "signal-waiter" Signal executing 1
      (created facts executing 1 0 0 2 ++ waiterAt facts executing 1 executing 2 3)
      0 0 True 2
      [ eq (countCell facts executing 1) 0
      , byte (taskAddress facts executing 2 + tfState (factsTask facts)) (stReady (factsStates facts))
      , eq (waitResult facts executing 2) 0
      ]
  , (plain "signal-waiter-handler" InterruptSignal executing 1
      (created facts executing 1 0 0 2 ++ waiterAt facts executing 1 executing 2 3)
      0 0 False 0
      [ eq (countCell facts executing 1) 0
      , byte (taskAddress facts executing 2 + tfState (factsTask facts)) (stReady (factsStates facts))
      , eq (dfReschedule (domainFacts facts executing)) 1
      ])
      { caseHandler = True }
  , (plain "signal-waiter-peer" Signal executing 1
      (created facts executing 1 0 0 2 ++ waiterAt facts (peer executing) 1 executing 2 3)
      0 0 False 0 [ eq (countCell facts executing 1) 0 ])
      { caseBank = Just (peer executing, 0x01000000) }
  , (plain "signal-armed-waiter" Signal executing 1
      (created facts executing 1 0 0 2 ++ waiterAt facts executing 1 executing 2 3
        ++ [poke (waitResult facts executing 2) timerToken])
      0 0 True 2 [ eq (countCell facts executing 1) 0, eq (waitResult facts executing 2) 0 ])
      { caseReceipt = Just (recordAddress facts executing executing, timerToken) }
  , plain "wait-count" Wait executing 1 (created facts executing 1 0 2 2) 0 0 False 0
      [ eq (countCell facts executing 1) 1 ]
  , plain "wait-blocks" Wait executing 1 (created facts executing 1 0 0 1) 0 0 True 3
      [ eq (countCell facts executing 1) 0
      , byte (taskAddress facts executing runningSlot + tfState (factsTask facts)) (queuedState facts)
      , eq (waitResult facts executing runningSlot) 0
      , eq (waitOwner facts executing runningSlot) (dfId (domainFacts facts executing))
      , eq (queueCell facts executing 1) (taskAddress facts executing runningSlot)
      ]
  , (plain "wait-dispatch-disabled" Wait executing 1 (created facts executing 1 0 0 1) 0
      (fromIntegral (-25 :: Int)) False 0 [ eq (countCell facts executing 1) 0 ])
      { caseDispatchEnabled = False }
  , plain "poll-count" Poll executing 1 (created facts executing 1 0 1 1) 0 0 False 0
      [ eq (countCell facts executing 1) 0 ]
  , plain "poll-empty" Poll executing 1 (created facts executing 1 0 0 1) 0
      (fromIntegral (-50 :: Int)) False 0
      [ eq (countCell facts executing 1) 0, noWrite (countCell facts executing 1) ]
  , (plain "poll-dispatch-disabled" Poll executing 1 (created facts executing 1 0 1 1) 0 0 False 0
      [ eq (countCell facts executing 1) 0 ])
      { caseDispatchEnabled = False }
  , plain "timed-poll-empty" TimedWait executing 1 (created facts executing 1 0 0 1) 0
      (fromIntegral (-50 :: Int)) False 0
      [ eq (countCell facts executing 1) 0, noWrite (countCell facts executing 1) ]
  , plain "timed-forever-blocks" TimedWait executing 1 (created facts executing 1 0 0 1)
      (fromIntegral (-5 :: Int)) 0 True 3
      [ eq (countCell facts executing 1) 0, eq (waitResult facts executing runningSlot) 0 ]
  , plain "timed-arms" TimedWait executing 1
      (created facts executing 1 0 0 1 ++ [poke (dfTickNow (domainFacts facts executing)) 100])
      10 0 True 3
      [ eq (waitResult facts executing runningSlot) (timerAddress facts executing runningSlot)
      , eq (timerAddress facts executing runningSlot + 4) (timeoutCallback executing)
      , eq (timerAddress facts executing runningSlot + 8) (taskAddress facts executing runningSlot)
      ]
  , (plain "signal-handler-in-task" InterruptSignal executing 1 (created facts executing 1 0 0 2) 0
      (fromIntegral (-25 :: Int)) False 0 [ eq (countCell facts executing 1) 0 ])
      { caseHandler = False }
  , (plain "signal-task-in-handler" Signal executing 1 (created facts executing 1 0 0 2) 0
      (fromIntegral (-25 :: Int)) False 0 [ eq (countCell facts executing 1) 0 ])
      { caseHandler = True }
  , (plain "signal-masked" Signal executing 1 (created facts executing 1 0 0 2) 0
      (fromIntegral (-25 :: Int)) False 0 [ eq (countCell facts executing 1) 0 ])
      { caseMasked = True }
  , (plain "signal-bad-class" Signal executing 1 (created facts executing 1 0 0 2) 0
      (fromIntegral (-18 :: Int)) False 0 [ eq (countCell facts executing 1) 0 ])
      { caseClass = Just 3 }
  , (plain "signal-bad-index" Signal executing 1 (created facts executing 1 0 0 2) 0
      (fromIntegral (-18 :: Int)) False 0 [ eq (countCell facts executing 1) 0 ])
      { caseSlot = 251 }
  , (plain "signal-not-created" Signal executing 1
      (created facts executing 1 0 0 2 ++ [poke (existsCell facts executing 1) 0])
      0 (fromIntegral (-42 :: Int)) False 0 [ eq (countCell facts executing 1) 0 ])
  , (plain "delete-handler-defers" Delete executing 1
      (created facts executing 1 0 0 1 ++ waiterAt facts executing 1 executing 2 3)
      0 0 False 0
      [ eq (existsCell facts executing 1) 0, eq (countCell facts executing 1) 1
      , byte (taskAddress facts executing 2 + tfState (factsTask facts)) (queuedState facts)
      , eq (dfReschedule (domainFacts facts executing)) 0
      ])
      { caseHandler = True
      , caseRestoresMask = True
      , caseOwned = Just (Expectation 0 False 0
          [ eq (existsCell facts executing 1) 0, eq (countCell facts executing 1) 1
          , byte (taskAddress facts executing 2 + tfState (factsTask facts)) (stReady (factsStates facts))
          , eq (waitResult facts executing 2) (fromIntegral (-49 :: Int))
          , eq (queueCell facts executing 1) (controlAddress facts executing 1)
          , eq (dfReschedule (domainFacts facts executing)) 1
           ] Nothing Nothing Nothing)
      , caseDifference = Just "semaphore-delete-releases-waiters"
      }
  , resumed "wait-signal-return" Wait 0 (Waker WakerSignal [(R0, 1)]) 0 0x93 Nothing
      (Expectation 0 False 0 (returnChecks 0 False) Nothing Nothing Nothing) Nothing Nothing
  , resumed "wait-signal-return-f" Wait 0 (Waker WakerSignal [(R0, 1)]) 0 0xD3 (Just 0x53)
      (Expectation 0 False 0 (returnChecks 0 False) Nothing Nothing Nothing) Nothing Nothing
  , resumed "timed-timeout-return" TimedWait 5 (Waker (WakerTick 2) []) timedOut 0x93 Nothing
      (Expectation timedOut False 0 (returnChecks timedOut False ++ timeoutChecks) Nothing Nothing Nothing) Nothing Nothing
  , resumed "wait-delete-return" Wait 0 (Waker WakerDelete [(R0, 1)]) released 0x93 Nothing
      (Expectation 0 False 0 [] Nothing (Just (InstructionLimit 20000)) Nothing)
      (Just (Expectation released False 0 (returnChecks released True) Nothing Nothing Nothing))
      (Just "semaphore-delete-releases-waiters")
  ]
  where
    timedOut = fromIntegral (-50 :: Int)
    released = fromIntegral (-49 :: Int)
    resumed name service timeout waker result mask entryControl stock owned difference =
      (plain name service executing 1 (created facts executing 1 0 0 1) timeout result False 0
        (expectationChecks stock))
        { caseBoundary = CallerReturn
        , caseWaker = Just waker
        , caseMask = Just mask
        , caseEntryControl = entryControl
        , caseRestoresMask = False
        , caseStock = stock { expectationDriverMask = stockDriver }
        , caseOwned = fmap (\expectation -> expectation { expectationDriverMask = ownedDriver }) owned
        , caseDifference = difference
        }
      where
        stockDriver = case wakerTarget waker of
          WakerSignal -> Nothing
          _ -> Just mask
        ownedDriver = case wakerTarget waker of
          WakerTick _ -> Just mask
          _ -> Nothing
    returnChecks result deleted =
      [ eq (waitResult facts executing runningSlot) result
      , eq (waitOwner facts executing runningSlot) (dfId (domainFacts facts executing))
      , eq (waitQueueCell facts executing runningSlot) (queueCell facts executing 1)
      , eq (queueCell facts executing 1) (controlAddress facts executing 1)
      , eq (queueCell facts executing 1 + wqPrev (factsWaitQueue facts)) (controlAddress facts executing 1)
      , byte (taskAddress facts executing runningSlot + tfState (factsTask facts)) (stReady (factsStates facts))
      , eq (readyHeadAt 10) (taskAddress facts executing runningSlot)
      , eq (dfRunning (domainFacts facts executing)) (taskAddress facts executing runningSlot)
      , eq (dfSelected (domainFacts facts executing)) (taskAddress facts executing runningSlot)
      ] ++ (if deleted
              then [eq (existsCell facts executing 1) 0, eq (countCell facts executing 1) 1]
              else [eq (countCell facts executing 1) 0])
    timeoutChecks =
      [ eq (dfTimerCount (domainFacts facts executing)) 0
      , eq (dfReschedule (domainFacts facts executing)) 1
      ]
    readyHeadAt priority =
      dfReadyHeads (domainFacts facts executing) + priority * hfSize (factsHead facts)

bindTimer :: TimerContract -> Facts -> Domain -> Case -> Case
bindTimer RecordingTimer _ _ one = one
bindTimer OwnedTimer facts executing one
  | Just (_, entry) <- caseReceipt one =
      one
        { caseReceipt = Nothing
        , caseTimerIndex = Just (TimerIndexCell (GuestAddress entry))
        , caseWords = caseWords one ++
            [ poke entry 1
            , poke (dfTimerCount words') 1
            , poke (dfTimerHeap words') 0
            , poke (dfTimerHeap words' + 4) entry
            ]
        }
  | Just (Waker (WakerTick _) _) <- caseWaker one =
      one {caseTimerIndex = Just (TimerIndexCell (GuestAddress (timerAddress facts executing runningSlot)))}
  | otherwise = one
  where
    words' = domainFacts facts executing

data Role = StockRole | OwnedRole deriving (Eq, Show)

layoutWords :: Facts -> Domain -> Case -> [(GuestAddress, Word32)]
layoutWords facts _executing one =
  concat [recordPokes block described | block <- domains, described <- domains]
    ++ concatMap taskPokes domains
    ++ concatMap objectPokes domains
    ++ concatMap readyPokes domains
  where
    domains = [DomainA, DomainB]
    waker = isJust (caseWaker one)
    recordPokes block described =
      let record = recordAddress facts block described
          describedWords = domainFacts facts described
          (acquire, release) = lockEntries block
          lock offset address =
            [ poke (record + offset + lfWord (factsLock facts)) address
            , poke (record + offset + lfAcquire (factsLock facts)) acquire
            , poke (record + offset + lfRelease (factsLock facts)) release
            ]
       in [ poke (record + cfId (factsContext facts)) (if block == described then factsOwnId facts else dfId describedWords)
          , poke (record + cfOwner (factsContext facts)) (dfId describedWords)
          , poke (record + cfReadyHeads (factsContext facts)) (dfReadyHeads describedWords)
          , poke (record + cfReadyBitmap (factsContext facts)) (dfReadyBitmap describedWords)
          , poke (record + cfRunning (factsContext facts)) (dfRunning describedWords)
          , poke (record + cfSelected (factsContext facts)) (dfSelected describedWords)
          , poke (record + cfDispatchEnabled (factsContext facts)) (dfDispatchEnabled describedWords)
          , poke (record + cfFirstEntry (factsContext facts)) (trampoline described)
          , poke (record + cfTasks (factsContext facts)) (dfTaskTable describedWords)
          , poke (record + cfTaskCount (factsContext facts)) (factsTaskSlots facts)
          , poke (record + cfSemaphores (factsContext facts)) (dfSemaphoreControls describedWords)
          , poke (record + cfSemaphoreCount (factsContext facts)) (factsSemaphoreSlots facts)
          , poke (record + cfTimerHeap (factsContext facts)) (dfTimerHeap describedWords)
          , poke (record + cfTimerCount (factsContext facts)) (dfTimerCount describedWords)
          , poke (record + cfTimerBase (factsContext facts)) (dfTimerBase describedWords)
          , poke (record + cfTickNow (factsContext facts)) (dfTickNow describedWords)
          ]
          ++ lock (cfSchedulerLock (factsContext facts)) (dfSchedulerLock describedWords)
          ++ lock (cfObjectLock (factsContext facts)) (dfObjectLock describedWords)
    taskPokes domain =
      concat
      [ let address = taskAddress facts domain slot
            definition = definitionAddress facts domain slot
            priority = case slot of
              1 -> 10
              2 | waker -> wakerPriority
              2 -> 3
              3 -> 15
              _ -> slot `mod` factsPriorities facts
            isWaker = waker && slot == wakerSlot && domain == _executing
         in [ poke address address
            , poke (address + tfPrev (factsTask facts)) address
            , poke (address + tfDefinition (factsTask facts)) definition
            , poke (address + tfDomain (factsTask facts)) (dfId (domainFacts facts domain))
            , poke (address + tfState (factsTask facts)) (packedState facts 0 priority)
            , poke (address + tfSavedSp (factsTask facts)) (if isWaker then wakerStack else targetStack)
            , poke (address + tfResumePc (factsTask facts)) (if isWaker then wakerStubBase else stopAddress)
            , poke (definition + defPriority (factsDefinition facts)) priority
            , poke (definition + 4) (0xa5510000 + slot)
            , poke (definition + 8) stopAddress
            , poke (definition + 0x10) (targetStack - 0x100)
            , poke (definition + 0x14) 0x100
            , poke (definition + defSlot (factsDefinition facts)) slot
            ]
      | slot <- [1 .. factsTaskSlots facts]
      ]
    objectPokes domain =
      concat
        [ let config = configAddress facts domain slot
              control = controlAddress facts domain slot
           in [ poke (config + cfgExists (factsConfig facts)) 0
              , poke (queueCell facts domain slot) control
              , poke (queueCell facts domain slot + wqPrev (factsWaitQueue facts)) control
              , poke (queueCell facts domain slot + wqConfig (factsWaitQueue facts)) config
              , poke (countCell facts domain slot) 0
              ]
        | slot <- [1 .. factsSemaphoreSlots facts]
        ]
    readyPokes domain =
      let dw = domainFacts facts domain
          headAt priority = dfReadyHeads dw + priority * hfSize (factsHead facts)
          member slot priority =
            let address = taskAddress facts domain slot
                headAddress = headAt priority
             in [ poke headAddress address
                , poke (headAddress + hfPrev (factsHead facts)) address
                , poke address headAddress
                , poke (address + tfPrev (factsTask facts)) headAddress
                , poke (address + tfState (factsTask facts)) (packedState facts (stReady (factsStates facts)) priority)
                ]
       in concat
            [ [ poke (headAt priority) (headAt priority)
              , poke (headAt priority + hfPrev (factsHead facts)) (headAt priority)
              ]
            | priority <- [0 .. factsPriorities facts - 1]
            ]
            ++ member runningSlot 10
            ++ member idleSlot 15
            ++ concat [ member wakerSlot wakerPriority | waker && domain == _executing ]
            ++ [ poke (dfReadyBitmap dw) ((1 `shiftL` 10) .|. (1 `shiftL` 15)
                  .|. (if waker && domain == _executing then 1 `shiftL` fromIntegral wakerPriority else 0))
               , poke (dfRunning dw) (taskAddress facts domain runningSlot)
               , poke (dfSelected dw) (taskAddress facts domain runningSlot)
               , poke (dfDispatchEnabled dw) 1
               , poke (dfSchedulerLock dw) 0
               , poke (dfObjectLock dw) 0
               , poke (dfCurrentContext dw) (recordAddress facts domain domain)
               , poke (dfTimerCount dw) 0
               , poke (dfTickNow dw) 0
               ]

trampoline :: Domain -> Word32
trampoline DomainA = 0x43728
trampoline DomainB = 0xd3728

lockEntries :: Domain -> (Word32, Word32)
lockEntries DomainA = (0x439d8, 0x43a14)
lockEntries DomainB = (0xd398c, 0xd39c8)

kernelBounds :: Facts -> Domain -> (GuestAddress, Int)
kernelBounds facts domain = (GuestAddress low, fromIntegral (high - low))
  where
    dw = domainFacts facts domain
    low = minimum
      [ dfHandlerContext dw, dfCurrentContext dw, dfContexts dw
      , dfSchedulerLock dw, dfObjectLock dw, dfTimerHeap dw
      , dfSemaphoreConfigs dw, dfSemaphoreControls dw
      , dfSemaphoreWaits dw, dfSemaphoreTimers dw, dfTaskTable dw
      ]
    high = maximum
      [ dfContextA dw + cfSize (factsContext facts)
      , dfContextB dw + cfSize (factsContext facts)
      , dfReadyHeads dw + factsPriorities facts * hfSize (factsHead facts)
      , dfSelected dw + 4
      , dfTaskTable dw + factsTaskSlots facts * tfSize (factsTask facts)
      , dfSemaphoreConfigs dw + factsSemaphoreSlots facts * cfgSize (factsConfig facts)
      , dfSemaphoreControls dw + factsSemaphoreSlots facts * ctlSize (factsControl facts)
      , dfSemaphoreWaits dw + factsTaskSlots facts * wbSize (factsWaitBlock facts)
      , dfSemaphoreTimers dw + factsTaskSlots facts * factsTimerEntrySize facts
      , dfTimerHeap dw + 0x1000
      , dfSemaphoreConfigs dw + (factsSemaphoreSlots facts + 1) * cfgSize (factsConfig facts) + 0x200
      , dfTickNow dw + 4
      ]

loaderDataSize :: Word32
loaderDataSize = 0x0000_0060

-- | The loader window holding `kernel_find_ready`'s nibble table. The literal is A `0x4daa0`
-- and B `0xdda64`; both windows lie inside their pinned loader prefixes.
loaderDataBase :: Domain -> Word32
loaderDataBase DomainA = 0x0004_da80
loaderDataBase DomainB = 0x000d_da40

loaderDataBytes :: Image -> Word32 -> B.ByteString
loaderDataBytes image base =
  B.take (fromIntegral loaderDataSize) (B.drop (fromIntegral (base - imageBase image)) (imageBytes image))

callerSp :: Case -> Word32
callerSp one = if caseHandler one then handlerStack else callerStack

pages :: Facts -> Image -> Case -> [Page]
pages facts loaderImage one =
  [ let (address, size) = kernelBounds facts domain
     in Page (if domain == DomainA then RegionId "kernel-a" else RegionId "kernel-b") address size ReadWrite (B.replicate size 0)
  | domain <- [DomainA, DomainB]
  ]
    ++ [ Page (RegionId "definitions") (GuestAddress definitionBase) (fromIntegral (2 * factsTaskSlots facts * definitionStride)) ReadWrite (B.replicate (fromIntegral (2 * factsTaskSlots facts * definitionStride)) 0)
       , Page (RegionId "request") (GuestAddress requestAddress) 16 ReadWrite (B.replicate 16 0)
       , Page (RegionId "timer-seam") (GuestAddress receiptAddress) 0x100 ReadWrite (B.replicate 0x100 0)
       , Page (RegionId "global-lock") (GuestAddress globalLock) 4 ReadWrite (B.replicate 4 0)
       , Page (RegionId "stack") (GuestAddress (callerSp one - 0x400)) 0x400 ReadWrite (B.replicate 0x400 0)
       , Page (RegionId "caller-frame") (GuestAddress (callerSp one)) (B.length callerBytes) ReadWrite callerBytes
       , Page (RegionId "target-stack") (GuestAddress (targetStack - 0x100)) 0x100 ReadWrite (B.replicate 0x100 0)
       , Page (RegionId "bank-a") (bank DomainA) 4 Device (B.replicate 4 0)
       , Page (RegionId "bank-b") (bank DomainB) 4 Device (B.replicate 4 0)
       , let base = if imageBase loaderImage == 0x40000 then loaderDataBase DomainA else loaderDataBase DomainB
          in Page (RegionId "loader-data") (GuestAddress base) (fromIntegral loaderDataSize) ReadOnly
               (loaderDataBytes loaderImage base)
       ]
    ++ [ Page (RegionId "waker-stack") (GuestAddress (wakerStack - 0x400)) 0x400 ReadWrite (B.replicate 0x400 0)
       | isJust (caseWaker one)
       ]

contains :: GuestAddress -> Int -> Page -> Bool
contains (GuestAddress address) width (Page _ (GuestAddress base) size _ _) =
  toInteger base <= toInteger address && toInteger address + toInteger width <= toInteger base + toInteger size

overlay :: GuestAddress -> B.ByteString -> [(GuestAddress, Word32)] -> B.ByteString
overlay (GuestAddress start) initial words' = B.concat (pieces 0 ordered)
  where
    ordered = M.toAscList (M.fromList
      [ (address, value)
      | (address@(GuestAddress raw), value) <- words'
      , raw >= start
      , toInteger raw + 4 <= toInteger start + toInteger (B.length initial)
      ])
    pieces cursor [] = [B.drop cursor initial]
    pieces cursor ((GuestAddress address, value) : rest) =
      let offset = fromIntegral (address - start)
       in B.take (offset - cursor) (B.drop cursor initial)
            : word32le value
            : pieces (offset + 4) rest

aggregate :: Image -> Bool
aggregate image = not (null (imageSymbols image))

symbolSpans :: Image -> [(Word32, Word32)]
symbolSpans image = [(Agg.symbolAddress symbol, Agg.symbolSize symbol) | symbol <- imageSymbols image]

ownedSpans :: Domain -> Role -> Image -> [(Word32, Word32)]
ownedSpans domain role owned
  | role == OwnedRole, aggregate owned = symbolSpans owned
  | otherwise = extents domain

codeBounds :: Domain -> Role -> Image -> [(Word32, Word32)] -> (Word32, Word32)
codeBounds domain role owned extra =
  let placementSpans = [(Agg.placementAddress placement, Agg.placementBytes placement) | placement <- imagePlacements owned]
      spans = extents domain ++ retained domain ++ extra ++ (if role == OwnedRole then symbolSpans owned ++ placementSpans ++ [(imageBase owned, fromIntegral (B.length (imageBytes owned))) | aggregate owned] else [])
   in (minimum (map fst spans), maximum [address + size | (address, size) <- spans])

mergeSpans :: [(Word32, Word32)] -> [(Word32, Word32)]
mergeSpans spans = go (sortOn fst spans)
  where
    go [] = []
    go ((start, size) : rest) = extend start (start + size) rest
    extend start end [] = [(start, end - start)]
    extend start end ((lo, size) : rest)
      | lo < end = extend start (max end (lo + size)) rest
      | otherwise = (start, end - start) : extend lo (lo + size) rest

codeBytes :: TimerContract -> Domain -> Role -> Image -> Image -> [(Word32, Word32)] -> B.ByteString
codeBytes contract domain role loader owned extra
  | contract == OwnedTimer = withPlacements
  | otherwise = install (timerRemoval domain) timerStub withPlacements
  where
    (base, top) = codeBounds domain role owned extra
    slice image address size =
      if address < imageBase image
        || toInteger address + toInteger size > toInteger (imageBase image) + toInteger (B.length (imageBytes image))
        then error "semaphore code slice is outside its image"
        else B.take (fromIntegral size) (B.drop (fromIntegral (address - imageBase image)) (imageBytes image))
    original = slice loader base (top - base)
    install address bytes current =
      let offset = fromIntegral (address - base)
       in B.take offset current <> bytes <> B.drop (offset + B.length bytes) current
    patches = ownedSpans domain role owned
    withSymbols = foldl patch original patches
    withPlacements
      | role == OwnedRole, aggregate owned = install (imageBase owned) (imageBytes owned) original
      | otherwise = withSymbols
    patch current (address, size) = install address (slice owned address size) current

tickEntry :: Domain -> Word32
tickEntry DomainA = 0x44d8c
tickEntry DomainB = 0xd4d4c

dispatcher :: Domain -> Word32
dispatcher DomainA = 0x43640
dispatcher DomainB = 0xd3640

timeoutExtents :: Domain -> [(Word32, Word32)]
timeoutExtents DomainA = [(0x44318, 0x1cc), (0x44d8c, 0x1a0)]
timeoutExtents DomainB = [(0xd42d8, 0x1cc), (0xd4d4c, 0x1a0)]

wakerCodeExtents :: Domain -> Case -> [(Word32, Word32)]
wakerCodeExtents domain one = case caseWaker one of
  Just (Waker (WakerTick _) _) -> timeoutExtents domain
  _ -> []

data StubInstruction = StubWord Word32 | StubBranchLink Word32

stubWords :: Facts -> Domain -> Waker -> [Word32]
stubWords facts domain waker = assembleStub wakerStubBase (stubProgram facts domain waker)
  where
    stubProgram facts' domain' waker' =
      [ StubWord 0xe10fc000
      , StubWord (movw 3 (maskAddress .&. 0xffff))
      , StubWord (movt 3 (maskAddress `shiftR` 16))
      , StubWord 0xe583c000
      , StubWord 0xf1080080
      ]
        ++ handlerPrelude facts' domain' waker'
        ++ concatMap argWords (wakerArgs waker')
        ++ wakeCalls facts' domain' waker'
    handlerPrelude facts' domain' waker' = case wakerTarget waker' of
      WakerTick _ ->
        let handler = dfHandlerContext (domainFacts facts' domain')
         in [ StubWord (movw 3 (handler .&. 0xffff))
            , StubWord (movt 3 (handler `shiftR` 16))
            , StubWord 0xe3a00001
            , StubWord 0xe5830000
            ]
      _ -> []
    wakeCalls facts' domain' waker' = case wakerTarget waker' of
      WakerTick count -> tickCalls facts' domain' count
      WakerSignal -> tailCalls domain' (entries domain' Signal)
      WakerDelete -> tailCalls domain' (entries domain' Delete)
    tickCalls facts' domain' count =
      replicate count (StubBranchLink (tickEntry domain'))
        ++ [ StubWord (movw 3 (handler .&. 0xffff))
           , StubWord (movt 3 (handler `shiftR` 16))
           , StubWord 0xe3a00000
           , StubWord 0xe5830000
           ]
        ++ driverMaskCode
        ++ [StubBranchLink (dispatcher domain'), StubWord 0xeafffffe]
      where
        handler = dfHandlerContext (domainFacts facts' domain')
    tailCalls domain' target =
      [StubBranchLink target] ++ driverMaskCode
        ++ [StubBranchLink (dispatcher domain'), StubWord 0xeafffffe]
    driverMaskCode =
      [ StubWord 0xf10c0080
      , StubWord 0xe10fc000
      , StubWord (movw 3 (dispatchMaskAddress .&. 0xffff))
      , StubWord (movt 3 (dispatchMaskAddress `shiftR` 16))
      , StubWord 0xe583c000
      ]
    argWords (register, value) =
      [ StubWord (movw (registerNumber register) (value .&. 0xffff))
      , StubWord (movt (registerNumber register) (value `shiftR` 16))
      ]
    registerNumber register = case register of
      R0 -> 0
      R1 -> 1
      R2 -> 2
      R3 -> 3
      _ -> error "a waker argument is not in r0-r3"

assembleStub :: Word32 -> [StubInstruction] -> [Word32]
assembleStub base = go base
  where
    go _ [] = []
    go address (StubWord word : rest) = word : go (address + 4) rest
    go address (StubBranchLink target : rest) =
      (0xeb000000 .|. (((target - address - 8) `shiftR` 2) .&. 0xffffff)) : go (address + 4) rest

movw :: Word32 -> Word32 -> Word32
movw register immediate =
  0xe3000000 .|. ((immediate .&. 0xf000) `shiftL` 4) .|. (register `shiftL` 12) .|. (immediate .&. 0xfff)

movt :: Word32 -> Word32 -> Word32
movt register immediate =
  0xe3400000 .|. ((immediate .&. 0xf000) `shiftL` 4) .|. (register `shiftL` 12) .|. (immediate .&. 0xfff)

continuationBytes :: Facts -> Domain -> Case -> B.ByteString
continuationBytes facts executing one = installWaker zero
  where
    size = fromIntegral (stopAddress - exitAddress + 4)
    zero = B.replicate size 0
    installWaker image = case caseWaker one of
      Nothing -> image
      Just waker -> installAt wakerStubBase (B.concat (map word32le (stubWords facts executing waker))) image
    installAt address bytes image =
      B.take offset image <> bytes <> B.drop (offset + B.length bytes) image
      where offset = fromIntegral (address - exitAddress)

expectationOf :: Role -> Case -> Expectation
expectationOf StockRole one = caseStock one
expectationOf OwnedRole one = maybe (caseStock one) id (caseOwned one)

caseSpec :: TimerContract -> Facts -> Domain -> Role -> Image -> Image -> Case -> CaseSpec
caseSpec contract facts executing role loader owned one
  | not (null outside) = error (caseName one ++ ": unmapped fixture words " ++ show outside)
  | otherwise = CaseSpec
      { caseId = emittedId executing one
      , caseProvenance = "X-M1 1.50 semaphore family, pinned x70view 55436fc85c0560dd252b0410af6f2427fe198cd3; synthetic task/object state and timer argument seam"
      , caseExclusion = "Private compiler frames are not byte-compared. Dispatch checks the selected-task arrival and the live stock continuation."
          ++ (if contract == RecordingTimer then " The timer removal entry is a recording stub; it proves arguments, not heap removal." else " Timer removal and tick execute unmodified linked bodies; the approved Detached-index correction is asserted per role.")
          ++ (if isJust (caseWaker one) then " The resumed caller owns the caller stack context; a synthetic waker task on its own declared stack calls the stock signal, delete or timer tick, and the suspended caller resumes through its saved dispatcher frame into the wait epilogue. The waker stub records the caller's dispatch CPSR, and the tick or fallback driver masks I and records its own CPSR before the dispatcher call; flags are excluded and the I/F/mode bits are compared." else "")
      , caseEntry = wire (entries executing (caseService one))
      , caseExit = wire exitAddress
      , caseRegions =
          [mkRegion (RegionId "code") (wire codeBase) (fromIntegral (codeTop - codeBase)) ReadExecute (codeBytes contract executing role loader owned extra)]
            ++ [mkRegion ident (wire (guestWord address)) size permission (overlay address background words') | Page ident address size permission background <- dataPages]
            ++ [mkRegion (RegionId "continuations") (wire exitAddress) (fromIntegral (stopAddress - exitAddress + 4)) ReadExecute (continuationBytes facts executing one)]
      , caseFunctions =
          [(wire address, size) | (address, size) <- mergeSpans functionSpans]
            ++ [(wire wakerStubBase, 0x80) | isJust (caseWaker one)]
      , caseInitial = initialRegisters executing one
      , caseExpectedStatus = if isJust budget then InstructionBudget else if arrival then StopAddress else Returned
      , caseExpectedRegisters = expectedRegisters
      , caseExpectedMemory = [(RegionId "caller-frame", callerBytes)]
      , caseCp14Identity = Just (if executing == DomainA then 0x4003 else 0x4004)
      , caseKernel = Just (KernelCase (maybe plainLimit id budget) (if arrival then Just (wire stopAddress) else Nothing) (Just 0x80001234) (Just 0) (Just 0) ignored stacks)
      }
  where
    expectation = expectationOf role one
    arrival = expectationArrival expectation
    budget = expectationBudget expectation
    expectedRegisters
      | isJust budget = [(R0, 0)]
      | arrival = [(Pc, stopAddress), (Sp, targetStack)]
      | otherwise = [(R0, expectationResult expectation)]
    extra = wakerCodeExtents executing one ++ [(timerRemoval executing, 0x6c) | contract == OwnedTimer]
    placementFunctions = [(Agg.placementAddress placement, Agg.placementBytes placement) | placement <- imagePlacements owned, ".text." `isPrefixOf` Agg.placementSection placement]
    allocated = symbolSpans owned ++ [(Agg.placementAddress placement, Agg.placementBytes placement) | placement <- imagePlacements owned]
    overlaps (left, size) (right, bytes) = left < right + bytes && right < left + size
    preserved = [extent | extent <- retained executing ++ extra, role /= OwnedRole || not (any (overlaps extent) allocated)]
    functionSpans = ownedSpans executing role owned ++ preserved ++ placementFunctions
    (codeBase, codeTop) = codeBounds executing role owned extra
    dataPages = pages facts loader one
    words' = layoutWords facts executing one
      ++ [ poke (dfHandlerContext (domainFacts facts executing)) (if caseHandler one then 1 else 0)
         , poke (dfDispatchEnabled (domainFacts facts executing)) (if caseDispatchEnabled one then 1 else 0)
         ]
      ++ caseWords one
    outside = [address | (address, _) <- words', not (any (contains address 4) dataPages)]
    ignored
      | isJust budget = [R0 .. R12] ++ [Lr]
      | caseBoundary one == CallerReturn = [R1, R2, R3, R12, Lr]
      | arrival = [R0 .. R12] ++ [Lr]
      | otherwise = [R1 .. R12] ++ [Lr]
    stacks
      | isJust (caseWaker one) =
          map stack [(callerSp one - 0x400, callerSp one), (wakerStack - 0x400, wakerStack)]
      | otherwise = []
    stack (low, high) = either error id (mkStackContext (N.GuestAddress (fromIntegral low)) (N.GuestAddress (fromIntegral high)))

initialRegisters :: Domain -> Case -> [(Register, Word32)]
initialRegisters executing one =
  [(register, value register) | register <- [R0 .. R12]]
    ++ [(Sp, callerSp one), (Lr, exitAddress), (Cpsr, cpsr)]
  where
    cpsr
      | Just control <- caseEntryControl one = control
      | caseMasked one = 0x93
      | otherwise = 0x13
    value R0 = handle
    value R1 = case caseService one of
      Create -> requestAddress
      TimedWait -> caseTimeout one
      _ -> 0xa5a50001
    value register = 0xa5a50000 + fromIntegral (fromEnum register)
    handle = case caseClass one of
      Just cls -> (cls `shiftL` 16) .|. caseSlot one
      Nothing
        | caseDomain one == executing -> caseSlot one
        | otherwise -> (if caseDomain one == DomainA then 1 else 2) `shiftL` 16 .|. caseSlot one

emittedId :: Domain -> Case -> String
emittedId executing one = caseName one ++ "-" ++ domainLabel executing

wire :: Word32 -> N.GuestAddress
wire = N.GuestAddress . fromIntegral

readWord :: Facts -> Image -> Case -> NativeObservation -> GuestAddress -> Maybe Word32
readWord facts loader one observation address@(GuestAddress raw) = case
  [ (base, bytes)
  | page@(Page ident (GuestAddress base) _ _ _) <- pages facts loader one
  , contains address 4 page
  , Just bytes <- [lookup ident (nativeMemory observation)]
  ]
  of
    (base, bytes) : _ -> Just (readWordAt bytes (fromIntegral (raw - base)))
    [] -> Nothing

readByte :: Facts -> Image -> Case -> NativeObservation -> GuestAddress -> Maybe Word8
readByte facts loader one observation address@(GuestAddress raw) = case
  [ (base, bytes)
  | page@(Page ident (GuestAddress base) _ _ _) <- pages facts loader one
  , contains address 1 page
  , Just bytes <- [lookup ident (nativeMemory observation)]
  ]
  of
    (base, bytes) : _ -> Just (B.index bytes (fromIntegral (raw - base)))
    [] -> Nothing

writesAt :: NativeObservation -> GuestAddress -> [Word32]
writesAt observation address = case nativeKernel observation >>= observedOrder of
  Nothing -> []
  Just events ->
    [ fromIntegral value
    | OrderedWrite (Write actual _) (WriteValue value) <- events
    , actual == wire (guestWord address)
    ]

guestWord :: GuestAddress -> Word32
guestWord (GuestAddress raw) = raw

holds :: Facts -> Image -> Case -> NativeObservation -> Check -> Bool
holds facts loader one observation check = case check of
  Equals address value -> readWord facts loader one observation address == Just value
  ByteEquals address value -> readByte facts loader one observation address == Just value
  NoWrite address -> null (writesAt observation address)

compareRole :: Facts -> Image -> Domain -> CaseSpec -> Case -> Expectation -> NativeObservation -> [String]
compareRole facts loader _ spec one expectation observation =
  [ "wrong exit " ++ show (caseOutcome spec observation)
  | caseOutcome spec observation /= wanted
  ]
    ++ [ "refused " ++ show status ++ " is not " ++ show expectedRefusal
       | Just expectedRefusal <- [expectationRefusal expectation]
       , nativeStatus observation /= expectedRefusal
       , let status = nativeStatus observation
       ]
    ++ [ "result " ++ show resultActual ++ " is not " ++ show (expectationResult expectation)
       | expectationRefusal expectation == Nothing
       , expectationBudget expectation == Nothing
       , not (expectationArrival expectation)
       , resultActual /= Just (expectationResult expectation)
       ]
    ++ [ "register " ++ show register ++ " is " ++ show actual ++ " not " ++ show expected
       | expectationRefusal expectation == Nothing
       , expectationBudget expectation == Nothing
       , (register, expected) <- caseExpectedRegisters spec
       , let actual = N.lookupRegister register observation
       , actual /= Just expected
       ]
    ++ [ "caller register " ++ show register ++ " is " ++ show actual ++ " not " ++ show expected
       | expectationRefusal expectation == Nothing
       , expectationBudget expectation == Nothing
       , not (expectationArrival expectation)
       , (register, expected) <- callerAbi
       , let actual = N.lookupRegister register observation
       , actual /= Just expected
       ]
    ++ [ "CPSR control " ++ show controlActual ++ " is not " ++ show expectedControl
       | expectationRefusal expectation == Nothing
       , expectationBudget expectation == Nothing
       , controlActual /= Just expectedControl
       ]
    ++ [ "saved caller mask " ++ show savedMask ++ " is not " ++ show mask
       | Just mask <- [caseMask one]
       , let savedMask = fmap (.&. 0x1ff) (readWord facts loader one observation (GuestAddress maskAddress))
       , savedMask /= Just mask
       ]
    ++ [ "driver mask " ++ show driverMask ++ " is not " ++ show mask
       | Just mask <- [expectationDriverMask expectation]
       , let driverMask = fmap (.&. 0x1ff) (readWord facts loader one observation (GuestAddress dispatchMaskAddress))
       , driverMask /= Just mask
       ]
    ++ [ "caller frame changed"
       | lookup (RegionId "caller-frame") (nativeMemory observation) /= Just callerBytes
       ]
    ++ [ "missing effect " ++ show check
       | check <- expectationChecks expectation
       , not (holds facts loader one observation check)
       ]
    ++ [ "bank " ++ show domain ++ " holds " ++ show actual
       | domain <- [DomainA, DomainB]
       , let actual = readWord facts loader one observation (bank domain)
       , actual /= Just (expectedBank domain)
       ]
    ++ [ "timer receipt differs"
       | (expectedOwner, expectedEntry) <- maybe [] pure (caseReceipt one)
       , ( readWord facts loader one observation (GuestAddress receiptAddress)
         , readWord facts loader one observation (GuestAddress (receiptAddress + 4))
         ) /= (Just expectedOwner, Just expectedEntry)
       ]
    ++ [ "unexpected timer receipt"
       | caseReceipt one == Nothing
       , readWord facts loader one observation (GuestAddress receiptAddress) /= Just 0
         || readWord facts loader one observation (GuestAddress (receiptAddress + 4)) /= Just 0
       ]
  where
    wanted = case expectationRefusal expectation of
      Just _ -> OutcomeRefused (nativeStatus observation)
      Nothing -> case expectationBudget expectation of
        Just limit -> OutcomeNoReturnWithin limit (nativeKernel observation >>= observedReserved)
        Nothing -> if expectationArrival expectation then OutcomeStopped (wire stopAddress) else OutcomeReturned
    resultActual = N.lookupRegister R0 observation
    callerAbi =
      [(Sp, callerSp one)]
        ++ [ (register, 0xa5a50000 + fromIntegral (fromEnum register))
           | isJust (caseWaker one)
           , register <- [R4 .. R11]
           ]
    controlActual = fmap (.&. 0x1ff) (N.lookupRegister Cpsr observation)
    expectedControl
      | expectationArrival expectation = 0x93
      | admissionRefusal = entryControl
      | caseRestoresMask one = entryControl
      | otherwise = entryControl .&. complement 0x80
    entryControl
      | Just control <- caseEntryControl one = control
      | caseMasked one = 0x93
      | otherwise = 0x13
    admissionRefusal = expectationResult expectation `elem` map (fromIntegral :: Int -> Word32) [-25, -18, -42]
    expectedBank domain = case caseBank one of
      Just (target, value) | target == domain -> value
      _ -> 0

compareFamily :: Facts -> Domain -> Image -> Case -> CaseSpec -> CaseSpec -> NativeObservation -> NativeObservation -> [String]
compareFamily facts executing loader one stockSpec ownedSpec stock owned =
  compareRole facts loader executing stockSpec one (caseStock one) stock
    ++ compareRole facts loader executing ownedSpec one ownedExpectation owned
    ++ timerIndexErrors facts loader one StockRole stock
    ++ timerIndexErrors facts loader one OwnedRole owned
    ++ ( if caseOwned one == Nothing
          then memoryDifferences
          else
            [ "the owned role did not differ from the defective stock expectation"
            | null (compareRole facts loader executing ownedSpec one (caseStock one) owned)
            ]
       )
  where
    ownedExpectation = maybe (caseStock one) id (caseOwned one)
    compared = [ident | Page ident _ _ _ _ <- pages facts loader one, ident `notElem` map RegionId ["stack", "target-stack", "waker-stack"]]
    memory observation = M.fromList [(ident, normalize ident bytes) | (ident, bytes) <- nativeMemory observation, ident `elem` compared]
    normalize ident = normalizeTimer ident . normalizeFrame ident
    normalizeFrame ident bytes
      | expectationArrival (caseStock one) || maybe False expectationArrival (caseOwned one) =
          if ident == region then clear (clear bytes savedSpOffset) resumeOffset else bytes
      | isJust (caseWaker one), ident == region =
          foldl clear bytes [callerSavedSp, callerResume, wakerSavedSp, wakerResume]
      | otherwise = bytes
    clear bytes offset
      | offset + 4 > B.length bytes = bytes
      | otherwise = B.take offset bytes <> word32le 0 <> B.drop (offset + 4) bytes
    region = if executing == DomainA then RegionId "kernel-a" else RegionId "kernel-b"
    kernelLow = fst (kernelBounds facts executing)
    offsetOf address = fromIntegral (address - guestWord kernelLow)
    savedSpOffset = offsetOf (taskAddress facts executing runningSlot + tfSavedSp (factsTask facts))
    resumeOffset = offsetOf (taskAddress facts executing runningSlot + tfResumePc (factsTask facts))
    callerSavedSp = savedSpOffset
    callerResume = resumeOffset
    wakerSavedSp = offsetOf (taskAddress facts executing wakerSlot + tfSavedSp (factsTask facts))
    wakerResume = offsetOf (taskAddress facts executing wakerSlot + tfResumePc (factsTask facts))
    normalizeTimer ident bytes = case caseTimerIndex one of
      Nothing -> bytes
      Just (TimerIndexCell address) -> case
        [fromIntegral (guestWord address - guestWord base) | page@(Page actual base _ _ _) <- pages facts loader one, actual == ident, contains address 4 page] of
          [offset] -> clear bytes offset
          _ -> bytes
    memoryDifferences =
      ["missing state region " ++ show ident | ident <- compared, M.notMember ident (memory stock) || M.notMember ident (memory owned)]
        ++ ["state region " ++ show ident ++ " differs" | ident <- compared, M.lookup ident (memory stock) /= M.lookup ident (memory owned)]

timerIndexErrors :: Facts -> Image -> Case -> Role -> NativeObservation -> [String]
timerIndexErrors facts loader one role observation = case caseTimerIndex one of
  Nothing -> []
  Just (TimerIndexCell address) ->
    ["timer index does not satisfy the named Detached-index contract"
    | readWord facts loader one observation address /= Just expected
        || writes address /= expectedWrites
    ]
  where
    expected = if role == OwnedRole then 0 else 1
    expectedWrites = inserted ++ [(4, 0) | role == OwnedRole]
    inserted = [(4, 1) | Just (Waker (WakerTick _) _) <- [caseWaker one]]
    writes address =
      [ (width, fromIntegral value :: Word32)
      | Just kernel <- [nativeKernel observation]
      , Just events <- [observedOrder kernel]
      , OrderedWrite (Write actual (WriteBytes width)) (WriteValue value) <- events
      , actual == wire (guestWord address)
      ]

timerControls :: Facts -> Domain -> Image -> Case -> CaseSpec -> CaseSpec -> NativeObservation -> NativeObservation -> [(String, Bool)]
timerControls facts executing loader one stockSpec ownedSpec stock owned = case caseTimerIndex one of
  Nothing -> []
  Just (TimerIndexCell address) ->
    [ ("owned-timer-relink-refused", rejected stock (mutate address 1 owned))
    , ("stock-timer-detach-refused", rejected (mutate address 0 stock) owned)
    , ("missing-detach-write-refused", rejected stock (owned {nativeKernel = fmap withoutDetach (nativeKernel owned)}))
    , ("live-caller-frame-mutation-refused", rejected stock (mutate (GuestAddress (callerSp one)) 0 owned))
    ]
    where
      rejected reference subject = not (null (compareFamily facts executing loader one stockSpec ownedSpec reference subject))
      withoutDetach kernel = kernel {observedOrder = fmap (filter keep) (observedOrder kernel)}
      keep (OrderedWrite (Write actual _) (WriteValue 0)) = actual /= wire (guestWord address)
      keep _ = True
      mutate target value observation = observation {nativeMemory = map replace (nativeMemory observation)}
        where
          replace (ident, bytes) = case
            [fromIntegral (guestWord target - guestWord base) | page@(Page actual base _ _ _) <- pages facts loader one, actual == ident, contains target 4 page] of
              [offset] -> (ident, B.take offset bytes <> word32le value <> B.drop (offset + 4) bytes)
              _ -> (ident, bytes)

runFamily :: FilePath -> FilePath -> FilePath -> Facts -> Domain -> Image -> Image -> IO Run
runFamily runner cacheRoot directory facts executing stock owned = do
  let contract = timerContract owned
      selected = map (bindTimer contract facts executing) (cases facts executing)
      stockCases = [(one, caseSpec contract facts executing StockRole stock stock one) | one <- selected]
      ownedCases = [(one, caseSpec contract facts executing OwnedRole stock owned one) | one <- selected]
  stockBatch <- writeBatch (directory </> "stock") (map snd stockCases)
  ownedBatch <- writeBatch (directory </> "owned") (map snd ownedCases)
  (stockReuse, stockRaw) <- cachedObservedBatch cacheRoot runner stockBatch
  (ownedReuse, ownedRaw) <- cachedObservedBatch cacheRoot runner ownedBatch
  let reference = M.fromList [(caseIdText ident, observation) | (ident, observation) <- stockRaw]
      subject = M.fromList [(caseIdText ident, observation) | (ident, observation) <- ownedRaw]
  results <- forM stockCases $ \(one, stockSpec) -> do
    let ident = emittedId executing one
        ownedSpec = case lookup (caseName one) [(caseName candidate, spec) | (candidate, spec) <- ownedCases] of
          Just spec -> spec
          Nothing -> error "the owned spec is missing for a case" 
    pure (caseName one, case (M.lookup ident reference, M.lookup ident subject) of
      (Just original, Just replacement) -> Right (compareFamily facts executing stock one stockSpec ownedSpec original replacement)
      (Nothing, _) -> Left "stock case did not run"
      (_, Nothing) -> Left "owned case did not run")
  unless (length stockRaw == length selected && length ownedRaw == length selected) (fail "native observations do not cover the current semaphore case set")
  let controls = concat
        [ [(caseName one ++ "/" ++ name, passed) | (name, passed) <- timerControls facts executing stock one stockSpec ownedSpec original replacement]
        | (one, stockSpec) <- stockCases
        , Just ownedSpec <- [lookup (caseName one) [(caseName candidate, spec) | (candidate, spec) <- ownedCases]]
        , Just original <- [M.lookup (emittedId executing one) reference]
        , Just replacement <- [M.lookup (emittedId executing one) subject]
        ]
  pure (Run executing results (length [() | (_, Right []) <- results]) (stockReuse, ownedReuse) controls)
