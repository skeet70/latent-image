{-# LANGUAGE OverloadedStrings #-}

-- | The fixed-pool family native cases: the stock layout each case builds, the seven stock entries
-- and their owned replacements, the batch runs and the per-case comparison.
module Replacement.RtosFixedPool.Oracle
  ( Facts (..)
  , DomainFacts (..)
  , ContextFacts (..)
  , LockFacts (..)
  , TaskFacts (..)
  , DefinitionFacts (..)
  , HeadFacts (..)
  , ConfigFacts (..)
  , ControlFacts (..)
  , PoolWaitFacts (..)
  , WaitBlockFacts (..)
  , WaitQueueFacts (..)
  , StateFacts (..)
  , parseFacts
  , Domain (..)
  , domainLabel
  , Case (..)
  , Check (..)
  , Expectation (..)
  , Service (..)
  , Image (..)
  , Run (..)
  , extents
  , entries
  , bodyRegion
  , bodyLength
  , runFamily
  ) where

import Control.Monad (forM, unless)
import Data.Aeson (FromJSON (..), eitherDecodeStrict', withObject, (.:))
import Data.Bits ((.&.), (.|.), complement, shiftL, shiftR)
import qualified Data.ByteString as B
import qualified Data.Map.Strict as M
import Data.List (isPrefixOf, sort, nub)
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
  , KernelCase (..)
  , Permission (..)
  , assemble
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
  , dfPoolConfigs :: Word32
  , dfPoolControls :: Word32
  , dfPoolWaits :: Word32
  , dfPoolTimers :: Word32
  , dfDefaultPool :: Word32
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
      <*> o .: "fixed_pool_configs" <*> o .: "fixed_pool_controls"
      <*> o .: "fixed_pool_waits" <*> o .: "fixed_pool_timers"
      <*> o .: "default_pool_storage"

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
  , cfTimerHeap :: Word32
  , cfTimerCount :: Word32
  , cfTimerBase :: Word32
  , cfTick :: Word32
  , cfPools :: Word32
  , cfPoolCount :: Word32
  }
  deriving (Eq, Show)

instance FromJSON ContextFacts where
  parseJSON = withObject "ContextFacts" $ \o ->
    ContextFacts
      <$> o .: "size" <*> o .: "id" <*> o .: "owner" <*> o .: "scheduler_lock"
      <*> o .: "object_lock" <*> o .: "ready_heads" <*> o .: "ready_bitmap"
      <*> o .: "running" <*> o .: "selected" <*> o .: "dispatch_enabled"
      <*> o .: "first_entry" <*> o .: "tasks" <*> o .: "task_count"
      <*> o .: "timer_heap" <*> o .: "timer_count" <*> o .: "timer_base" <*> o .: "tick_now"
      <*> o .: "fixed_pools" <*> o .: "fixed_pool_count"

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

data DefinitionFacts = DefinitionFacts {defPriority :: Word32}
  deriving (Eq, Show)

instance FromJSON DefinitionFacts where
  parseJSON = withObject "DefinitionFacts" $ \o ->
    DefinitionFacts <$> o .: "priority"

data HeadFacts = HeadFacts {hfSize :: Word32, hfNext :: Word32, hfPrev :: Word32}
  deriving (Eq, Show)

instance FromJSON HeadFacts where
  parseJSON = withObject "HeadFacts" $ \o ->
    HeadFacts <$> o .: "size" <*> o .: "next" <*> o .: "prev"

data ConfigFacts = ConfigFacts
  { cfgSize :: Word32
  , cfgAttributes :: Word32
  , cfgBlockSize :: Word32
  , cfgBegin :: Word32
  , cfgEnd :: Word32
  , cfgExists :: Word32
  , cfgPriorityOrder :: Word32
  }
  deriving (Eq, Show)

instance FromJSON ConfigFacts where
  parseJSON = withObject "ConfigFacts" $ \o ->
    ConfigFacts
      <$> o .: "size" <*> o .: "attributes" <*> o .: "block_size" <*> o .: "begin"
      <*> o .: "end" <*> o .: "exists" <*> o .: "priority_order"

data ControlFacts = ControlFacts
  { ctlSize :: Word32
  , ctlQueue :: Word32
  , ctlNextFree :: Word32
  , ctlRecycled :: Word32
  }
  deriving (Eq, Show)

instance FromJSON ControlFacts where
  parseJSON = withObject "ControlFacts" $ \o ->
    ControlFacts
      <$> o .: "size" <*> o .: "queue" <*> o .: "next_free" <*> o .: "recycled"

data PoolWaitFacts = PoolWaitFacts {pwSize :: Word32, pwBlock :: Word32}
  deriving (Eq, Show)

instance FromJSON PoolWaitFacts where
  parseJSON = withObject "PoolWaitFacts" $ \o ->
    PoolWaitFacts <$> o .: "size" <*> o .: "block"

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
  , factsPoolSlots :: Word32
  , factsConfig :: ConfigFacts
  , factsControl :: ControlFacts
  , factsPoolWait :: PoolWaitFacts
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
      <*> o .: "task_slots" <*> o .: "fixed_pool_slots" <*> o .: "pool_config"
      <*> o .: "pool_control" <*> o .: "pool_wait" <*> o .: "wait_block"
      <*> o .: "wait_queue" <*> o .: "timer_entry_size"
      <*> (StateFacts <$> o .: "state_ready" <*> o .: "state_wait" <*> o .: "state_suspended"
            <*> o .: "state_sleeping" <*> o .: "state_object_queued"
            <*> o .: "state_owner_lock_queued")

parseFacts :: B.ByteString -> Either String Facts
parseFacts = eitherDecodeStrict'

domainFacts :: Facts -> Domain -> DomainFacts
domainFacts facts DomainA = factsA facts
domainFacts facts DomainB = factsB facts

data Service = Create | Delete | Get | Poll | TimedGet | Release | PollUnchecked
  deriving (Eq, Show)

data Check = Equals GuestAddress Word32 | ByteEquals GuestAddress Word8 | NoWrite GuestAddress
  deriving (Eq, Show)

data Expectation = Expectation
  { expectationResult :: Word32
  , expectationArrival :: Bool
  , expectationChecks :: [Check]
  }
  deriving (Eq, Show)

data Case = Case
  { caseName :: String
  , caseService :: Service
  , caseObject :: Domain
  , caseSlot :: Word32
  , caseClass :: Maybe Word32
  , caseWords :: [(GuestAddress, Word32)]
  , caseTimeout :: Word32
  , caseStock :: Expectation
  , caseOwned :: Maybe Expectation
  , caseDifference :: Maybe String
  , caseHandler :: Bool
  , caseMasked :: Bool
  , caseDispatch :: Bool
  , caseBank :: Maybe (Domain, Word32)
  , caseReceipt :: Maybe (Word32, Word32)
  , caseTimerIndex :: Maybe GuestAddress
  , caseR1 :: Maybe Word32
  , caseResume :: Bool
  }

data Image = Image {imageBase :: Word32, imageBytes :: B.ByteString, imageSymbols :: [Agg.AggregateSymbol], imagePlacements :: [Agg.SectionPlacement]}

data Run = Run
  { runDomain :: Domain
  , runCases :: [(String, Either String [String])]
  , runMatches :: Int
  , runReuse :: (BatchReuse, BatchReuse)
  }

data Page = Page RegionId GuestAddress Int Permission B.ByteString

entries :: Domain -> Service -> Word32
entries DomainA service = case service of
  Create -> 0x4d6c4
  Delete -> 0x4d790
  Get -> 0x4b480
  Poll -> 0x4b6d0
  TimedGet -> 0x4b814
  Release -> 0x4bab8
  PollUnchecked -> 0x411cc
entries DomainB service = case service of
  Create -> 0xdd688
  Delete -> 0xdd754
  Get -> 0xdb440
  Poll -> 0xdb690
  TimedGet -> 0xdb7d4
  Release -> 0xdba78
  PollUnchecked -> 0xd11cc

extents :: Domain -> [(Word32, Word32)]
extents DomainA =
  [ (0x411cc, 0xf0), (0x4b480, 0x250), (0x4b6d0, 0x144), (0x4b814, 0x2a4)
  , (0x4bab8, 0x294), (0x4d6c4, 0xcc), (0x4d790, 0x88)
  ]
extents DomainB =
  [ (0xd11cc, 0xf0), (0xdb440, 0x250), (0xdb690, 0x144), (0xdb7d4, 0x2a4)
  , (0xdba78, 0x294), (0xdd688, 0xcc), (0xdd754, 0x88)
  ]

retained :: Domain -> [(Word32, Word32)]
retained DomainA =
  [ (0x4252c, 40), (0x42560, 8), (0x42568, 12), (0x42574, 8), (0x4257c, 12)
  , (0x4259c, 0x14), (0x425b0, 0x18)
  , (0x433e4, 96), (0x43444, 96), (0x43640, 40), (0x43668, 0x20), (0x43688, 0xa0)
  , (0x43728, 0x34), (0x4375c, 0x48), (0x437a4, 0x38), (0x437fc, 0x14)
  , (0x439d8, 60), (0x43a14, 40)
  , (0x43c48, 92), (0x43ca4, 124), (0x43d20, 176), (0x43dd0, 72), (0x43f70, 44)
  , (0x44018, 44), (0x44044, 240), (0x44134, 128), (0x44318, 460), (0x44d8c, 416)
  , (0x444e4, 132), (0x44568, 204), (0x44634, 200), (0x446fc, 72), (0x44744, 188)
  , (0x44800, 228), (0x448e4, 220), (0x4b41c, 100)
  , (0x44ac0, 196), (0x44b84, 220), (0x44c60, 72), (0x44ca8, 228)
  , (0x4d978, 244), (0x4da6c, 32)
  ]
retained DomainB =
  [ (0xd252c, 40), (0xd2560, 8), (0xd2568, 12), (0xd2574, 8), (0xd257c, 12)
  , (0xd259c, 0x14), (0xd25b0, 0x18)
  , (0xd33e4, 96), (0xd3444, 96), (0xd3640, 40), (0xd3668, 0x20), (0xd3688, 0xa0)
  , (0xd3728, 0x34), (0xd375c, 0x48), (0xd37a4, 0x38), (0xd37fc, 0x14)
  , (0xd398c, 60), (0xd39c8, 40)
  , (0xd3c04, 92), (0xd3c60, 124), (0xd3cdc, 176), (0xd3d8c, 72), (0xd3f2c, 44)
  , (0xd3fd4, 48), (0xd4004, 240), (0xd40f4, 128), (0xd42d8, 460), (0xd4d4c, 416)
  , (0xd44a4, 132), (0xd4528, 204), (0xd45f4, 200), (0xd46bc, 72), (0xd4704, 188)
  , (0xd47c0, 228), (0xd48a4, 220), (0xdb3dc, 100)
  , (0xd4a80, 196), (0xd4b44, 220), (0xd4c20, 72), (0xd4c68, 228)
  , (0xdd93c, 244), (0xdda30, 32)
  ]

bodyRegion, bodyLength :: Word32
bodyRegion = 0x50000
bodyLength = 0x3000

timerRemoval :: Domain -> Word32
timerRemoval DomainA = 0x44ca8
timerRemoval DomainB = 0xd4c68

timeoutCallback :: Domain -> Word32
timeoutCallback DomainA = 0x44318
timeoutCallback DomainB = 0xd42d8

callerStack, handlerStack, targetStack, exitAddress, stopAddress, receiptAddress, requestAddress, outAddress, poolRegion, globalLock :: Word32
callerStack = 0x00959800
handlerStack = 0x00959a00
targetStack = 0x00959c00
exitAddress = 0x00958f00
stopAddress = 0x00959200
receiptAddress = 0x00960000
requestAddress = 0x0095a000
outAddress = 0x0095a010

poolRegion = 0x40000000
globalLock = 0x400c0804

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

lowestBitAddress :: Domain -> Word32
lowestBitAddress DomainA = 0x0004_daa0
lowestBitAddress DomainB = 0x000d_da64

driverEntry, expiryEntry, deleteEntry, driverStack, driverSize, expirySize, deleteSize :: Word32
driverEntry = exitAddress + 0x40
expiryEntry = exitAddress + 0x80
deleteEntry = exitAddress + 0x100
driverStack = 0x0095_9e00
driverSize = 32
expirySize = 84
deleteSize = 12

branchLink :: Word32 -> Word32 -> Word32
branchLink from to = 0xeb00_0000 .|. ((to - (from + 8)) `shiftR` 2 .&. 0x00ff_ffff)

branch :: Word32 -> Word32 -> Word32
branch from to = 0xea00_0000 .|. ((to - (from + 8)) `shiftR` 2 .&. 0x00ff_ffff)

movw :: Word32 -> Word32 -> Word32
movw register immediate =
  0xe300_0000 .|. ((immediate .&. 0xf000) `shiftL` 4) .|. (register `shiftL` 12) .|. (immediate .&. 0x0fff)

movt :: Word32 -> Word32 -> Word32
movt register immediate =
  0xe340_0000 .|. ((immediate .&. 0xf000) `shiftL` 4) .|. (register `shiftL` 12) .|. (immediate .&. 0x0fff)

driverBytes :: Domain -> B.ByteString
driverBytes domain = assemble
  [ 0xe3a0_0001
  , 0xe300_1000
  , 0xe344_1000
  , 0xe10f_2000
  , 0xe3c2_2080
  , 0xe121_f002
  , branchLink (driverEntry + 24) (entries domain Release)
  , 0xeaff_fffe
  ]

expiryBytes :: Domain -> B.ByteString
expiryBytes domain = assemble
  [ movw 0 (interrupt `shiftR` 0 .&. 0xffff)
  , movt 0 (interrupt `shiftR` 16)
  , 0xe3a0_1001
  , 0xe580_1000
  , 0xe10f_2000
  , 0xe3c2_2080
  , 0xe121_f002
  , movw 0 (tick `shiftR` 0 .&. 0xffff)
  , movt 0 (tick `shiftR` 16)
  , 0xe3a0_101e
  , 0xe580_1000
  , branchLink (expiryEntry + 44) tickBody
  , movw 0 (interrupt `shiftR` 0 .&. 0xffff)
  , movt 0 (interrupt `shiftR` 16)
  , 0xe3a0_1000
  , 0xe580_1000
  , 0xe10f_2000
  , 0xe382_2080
  , 0xe121_f002
  , branchLink (expiryEntry + 76) dispatcher
  , 0xeaff_fffe
  ]
  where
    (interrupt, tick, tickBody, dispatcher) = case domain of
      DomainA -> (0x4005_45a4, 0x4005_4770, 0x44d8c, 0x43640)
      DomainB -> (0x400e_4564, 0x400e_4730, 0xd4d4c, 0xd3640)

deleteBytes :: Domain -> B.ByteString
deleteBytes domain = assemble
  [ 0xe3a0_0001
  , branchLink (deleteEntry + 4) (entries domain Delete)
  , branch (deleteEntry + 8) stopAddress
  ]

runningSlot, idleSlot, waiterSlot, secondWaiterSlot :: Word32
runningSlot = 1
idleSlot = 3
waiterSlot = 2
secondWaiterSlot = 4

taskAddress :: Facts -> Domain -> Word32 -> Word32
taskAddress facts domain slot = dfTaskTable (domainFacts facts domain) + (slot - 1) * tfSize (factsTask facts)

definitionAddress :: Facts -> Domain -> Word32 -> Word32
definitionAddress facts domain slot =
  definitionBase
    + (if domain == DomainA then 0 else fromIntegral (factsTaskSlots facts)) * definitionStride
    + (slot - 1) * definitionStride

configAddress, controlAddress, waitAddress, timerAddress :: Facts -> Domain -> Word32 -> Word32
configAddress facts domain slot = dfPoolConfigs (domainFacts facts domain) + (slot - 1) * cfgSize (factsConfig facts)
controlAddress facts domain slot = dfPoolControls (domainFacts facts domain) + (slot - 1) * ctlSize (factsControl facts)
waitAddress facts domain slot = dfPoolWaits (domainFacts facts domain) + (slot - 1) * pwSize (factsPoolWait facts)
timerAddress facts domain slot = dfPoolTimers (domainFacts facts domain) + (slot - 1) * factsTimerEntrySize facts

attributesCell, strideCell, beginCell, endCell, existsCell :: Facts -> Domain -> Word32 -> Word32
attributesCell facts domain slot = configAddress facts domain slot + cfgAttributes (factsConfig facts)
strideCell facts domain slot = configAddress facts domain slot + cfgBlockSize (factsConfig facts)
beginCell facts domain slot = configAddress facts domain slot + cfgBegin (factsConfig facts)
endCell facts domain slot = configAddress facts domain slot + cfgEnd (factsConfig facts)
existsCell facts domain slot = configAddress facts domain slot + cfgExists (factsConfig facts)

queueCell, nextFreeCell, recycledCell :: Facts -> Domain -> Word32 -> Word32
queueCell facts domain slot = controlAddress facts domain slot + ctlQueue (factsControl facts)
nextFreeCell facts domain slot = controlAddress facts domain slot + ctlNextFree (factsControl facts)
recycledCell facts domain slot = controlAddress facts domain slot + ctlRecycled (factsControl facts)

waitResult, waitQueueCell, waitOwner, waitBlockCell :: Facts -> Domain -> Word32 -> Word32
waitResult facts domain slot = waitAddress facts domain slot + wbResult (factsWaitBlock facts)
waitQueueCell facts domain slot = waitAddress facts domain slot + wbQueue (factsWaitBlock facts)
waitOwner facts domain slot = waitAddress facts domain slot + wbOwner (factsWaitBlock facts)
waitBlockCell facts domain slot = waitAddress facts domain slot + pwBlock (factsPoolWait facts)

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

packedState :: Word8 -> Word32 -> Word32
packedState state priority = fromIntegral state .|. (priority `shiftL` 8)

queuedState :: Facts -> Word8
queuedState facts = stWait states .|. stObjectQueued states .|. stOwnerLockQueued states
  where
    states = factsStates facts

created :: Facts -> Domain -> Word32 -> Word32 -> Word32 -> Word32 -> Word32 -> [(GuestAddress, Word32)]
created facts domain slot attributes count size buffer =
  [ poke (attributesCell facts domain slot) attributes
  , poke (strideCell facts domain slot) ((size + 3) .&. complement 3)
  , poke (beginCell facts domain slot) buffer
  , poke (endCell facts domain slot) (buffer + ((size + 3) .&. complement 3) * count)
  , poke (existsCell facts domain slot) 1
  , poke (queueCell facts domain slot) (controlAddress facts domain slot)
  , poke (queueCell facts domain slot + wqPrev (factsWaitQueue facts)) (controlAddress facts domain slot)
  , poke (queueCell facts domain slot + wqConfig (factsWaitQueue facts)) (configAddress facts domain slot)
  , poke (nextFreeCell facts domain slot) buffer
  , poke (recycledCell facts domain slot) 0
  ]

waiterAt :: Facts -> Domain -> Word32 -> Domain -> Word32 -> Word32 -> [(GuestAddress, Word32)]
waiterAt facts owner objectSlot objectDomain slot priority =
  [ poke (taskAddress facts owner slot + tfState (factsTask facts)) (packedState (queuedState facts) priority)
  , poke (taskAddress facts owner slot + tfWait (factsTask facts)) (waitAddress facts owner slot)
  , poke (waitResult facts owner slot) 0
  , poke (waitQueueCell facts owner slot) (queueCell facts objectDomain objectSlot)
  , poke (waitOwner facts owner slot) (dfId (domainFacts facts objectDomain))
  , poke (queueCell facts objectDomain objectSlot) (taskAddress facts owner slot)
  , poke (queueCell facts objectDomain objectSlot + wqPrev (factsWaitQueue facts)) (taskAddress facts owner slot)
  , poke (taskAddress facts owner slot + tfNext (factsTask facts)) (queueCell facts objectDomain objectSlot)
  , poke (taskAddress facts owner slot + tfPrev (factsTask facts)) (queueCell facts objectDomain objectSlot)
  ]

twoWaiters :: Facts -> Domain -> Word32 -> Domain -> (Word32, Word32) -> (Word32, Word32) -> [(GuestAddress, Word32)]
twoWaiters facts owner objectSlot objectDomain (first, firstPriority) (second, secondPriority) =
  [ poke (queueCell facts objectDomain objectSlot) (taskAddress facts owner first)
  , poke (queueCell facts objectDomain objectSlot + wqPrev (factsWaitQueue facts)) (taskAddress facts owner second)
  , poke (taskAddress facts owner first + tfState (factsTask facts)) (packedState (queuedState facts) firstPriority)
  , poke (taskAddress facts owner first + tfWait (factsTask facts)) (waitAddress facts owner first)
  , poke (taskAddress facts owner first + tfNext (factsTask facts)) (taskAddress facts owner second)
  , poke (taskAddress facts owner first + tfPrev (factsTask facts)) (queueCell facts objectDomain objectSlot)
  , poke (waitResult facts owner first) 0
  , poke (waitQueueCell facts owner first) (queueCell facts objectDomain objectSlot)
  , poke (waitOwner facts owner first) (dfId (domainFacts facts objectDomain))
  , poke (taskAddress facts owner second + tfState (factsTask facts)) (packedState (queuedState facts) secondPriority)
  , poke (taskAddress facts owner second + tfWait (factsTask facts)) (waitAddress facts owner second)
  , poke (taskAddress facts owner second + tfNext (factsTask facts)) (queueCell facts objectDomain objectSlot)
  , poke (taskAddress facts owner second + tfPrev (factsTask facts)) (taskAddress facts owner first)
  , poke (waitResult facts owner second) 0
  , poke (waitQueueCell facts owner second) (queueCell facts objectDomain objectSlot)
  , poke (waitOwner facts owner second) (dfId (domainFacts facts objectDomain))
  ]

resumeWordsWith :: Facts -> Domain -> Word32 -> [(GuestAddress, Word32)]
resumeWordsWith facts domain entry =
  created facts domain 1 0 1 8 poolRegion
    ++ [ poke (nextFreeCell facts domain 1) (poolRegion + 8)
       , poke (recycledCell facts domain 1) 0
       , poke (taskAddress facts domain idleSlot + tfSavedSp (factsTask facts)) driverStack
       , poke (taskAddress facts domain idleSlot + tfResumePc (factsTask facts)) entry
       , poke (taskAddress facts domain idleSlot + tfState (factsTask facts)) (packedState (stReady (factsStates facts)) 15)
       ]

resumeWords :: Facts -> Domain -> [(GuestAddress, Word32)]
resumeWords facts domain = resumeWordsWith facts domain driverEntry

createWords :: Word32 -> Word32 -> Word32 -> Word32 -> [(GuestAddress, Word32)]
createWords attributes count size buffer =
  [ poke requestAddress attributes
  , poke (requestAddress + 4) count
  , poke (requestAddress + 8) size
  , poke (requestAddress + 0x0c) buffer
  ]

plain :: String -> Service -> Domain -> Word32 -> [(GuestAddress, Word32)] -> Word32 -> Word32 -> Bool -> [Check] -> Case
plain name service objectDomain slot fixture timeout result arrival checks =
  Case name service objectDomain slot Nothing fixture timeout
    (Expectation result arrival checks) Nothing Nothing
    False False True Nothing Nothing Nothing Nothing False

cases :: Facts -> Domain -> [Case]
cases facts executing =
  [ plain "create-fresh" Create executing 1 (createWords 0 2 8 poolRegion) 0 0 False
      [ eq (attributesCell facts executing 1) 0
      , eq (strideCell facts executing 1) 8
      , eq (beginCell facts executing 1) poolRegion
      , eq (endCell facts executing 1) (poolRegion + 16)
      , eq (existsCell facts executing 1) 1
      , eq (nextFreeCell facts executing 1) poolRegion
      , eq (recycledCell facts executing 1) 0
      , eq (queueCell facts executing 1) (controlAddress facts executing 1)
      ]
  , plain "create-exists" Create executing 1
      (created facts executing 1 7 2 8 poolRegion ++ createWords 1 4 8 (poolRegion + 0x100))
      0 (fromIntegral (-41 :: Int)) False
      [ eq (attributesCell facts executing 1) 7
      , eq (strideCell facts executing 1) 8
      , eq (beginCell facts executing 1) poolRegion
      , eq (existsCell facts executing 1) 1
      ]
  , (plain "create-unrounded-end" Create executing 1 (createWords 0 3 5 poolRegion) 0 0 False
      [ eq (strideCell facts executing 1) 8
      , eq (endCell facts executing 1) (poolRegion + 15)
      , eq (existsCell facts executing 1) 1
      ])
      { caseOwned = Just (Expectation 0 False
          [ eq (strideCell facts executing 1) 8
          , eq (endCell facts executing 1) (poolRegion + 24)
          , eq (existsCell facts executing 1) 1
          ])
      , caseDifference = Just "fixed-pool-create-rounded-end"
      }
  , (plain "create-range-refused" Create executing 123 (createWords 0 1 8 poolRegion) 0 0 False [])
      { caseOwned = Just (Expectation (fromIntegral (-18 :: Int)) False [])
      , caseDifference = Just "fixed-pool-create-range-refused"
      }
  , (plain "create-class-ignored" Create executing 1 (createWords 0 1 8 poolRegion) 0 0 False
      [ eq (existsCell facts executing 1) 1 ])
      { caseClass = Just 3 }
  , plain "delete-plain" Delete executing 1 (created facts executing 1 0 2 8 poolRegion) 0 0 False
      [ eq (attributesCell facts executing 1) 0
      , eq (strideCell facts executing 1) 4
      , eq (beginCell facts executing 1) (dfDefaultPool (domainFacts facts executing))
      , eq (endCell facts executing 1) (dfDefaultPool (domainFacts facts executing) + 4)
      , eq (existsCell facts executing 1) 0
      , eq (nextFreeCell facts executing 1) (dfDefaultPool (domainFacts facts executing))
      , eq (recycledCell facts executing 1) 0
      , eq (queueCell facts executing 1) (controlAddress facts executing 1)
      ]
  , (plain "delete-waiter" Delete executing 1
      (created facts executing 1 0 1 8 poolRegion ++ waiterAt facts executing 1 executing 2 3)
      0 0 False
      [ eq (existsCell facts executing 1) 0
      , byte (taskAddress facts executing 2 + tfState (factsTask facts)) (queuedState facts)
      ])
      { caseOwned = Just (Expectation 0 True
          [ eq (existsCell facts executing 1) 0
          , byte (taskAddress facts executing 2 + tfState (factsTask facts)) (stReady (factsStates facts))
          , eq (waitResult facts executing 2) (fromIntegral (-49 :: Int))
          , eq (queueCell facts executing 1) (controlAddress facts executing 1)
          ])
      , caseDifference = Just "fixed-pool-delete-stranded-waiter"
      }
  , (plain "delete-range-refused" Delete executing 123 [] 0 0 False [])
      { caseOwned = Just (Expectation (fromIntegral (-18 :: Int)) False [])
      , caseDifference = Just "fixed-pool-delete-range-refused"
      }
  , plain "get-bump" Get executing 1 (created facts executing 1 0 2 8 poolRegion) 0 0 False
      [ eq outAddress poolRegion, eq (nextFreeCell facts executing 1) (poolRegion + 8) ]
  , plain "get-free-list" Get executing 1
      (created facts executing 1 0 2 8 poolRegion
        ++ [ poke (recycledCell facts executing 1) (poolRegion + 8)
           , poke (nextFreeCell facts executing 1) (poolRegion + 16)
           , poke (poolRegion + 8) 0
           ])
      0 0 False [eq outAddress (poolRegion + 8), eq (recycledCell facts executing 1) 0]
  , plain "poll-empty" Poll executing 1
      (created facts executing 1 0 1 8 poolRegion
        ++ [poke (nextFreeCell facts executing 1) (poolRegion + 8)])
      0 (fromIntegral (-50 :: Int)) False [noWrite outAddress]
  , plain "timed-poll-empty" TimedGet executing 1
      (created facts executing 1 0 1 8 poolRegion
        ++ [poke (nextFreeCell facts executing 1) (poolRegion + 8)])
      0 (fromIntegral (-50 :: Int)) False [noWrite outAddress]
  , plain "get-blocks" Get executing 1
      (created facts executing 1 0 1 8 poolRegion
        ++ [poke (nextFreeCell facts executing 1) (poolRegion + 8)])
      0 0 True
      [ byte (taskAddress facts executing runningSlot + tfState (factsTask facts)) (queuedState facts)
      , eq (waitOwner facts executing runningSlot) (dfId (domainFacts facts executing))
      , eq (waitQueueCell facts executing runningSlot) (queueCell facts executing 1)
      , eq (waitResult facts executing runningSlot) 0
      , eq (queueCell facts executing 1) (taskAddress facts executing runningSlot)
      ]
  , plain "timed-forever-blocks" TimedGet executing 1
      (created facts executing 1 0 1 8 poolRegion
        ++ [poke (nextFreeCell facts executing 1) (poolRegion + 8)])
      (fromIntegral (-1 :: Int)) 0 True
      [ eq (waitResult facts executing runningSlot) 0
      , eq (queueCell facts executing 1) (taskAddress facts executing runningSlot)
      ]
  , plain "timed-arms" TimedGet executing 1
      (created facts executing 1 0 1 8 poolRegion
        ++ [ poke (nextFreeCell facts executing 1) (poolRegion + 8)
           , poke (dfTickNow (domainFacts facts executing)) 100
           ])
      30 0 True
      [ eq (waitResult facts executing runningSlot) (timerAddress facts executing runningSlot)
      , eq (timerAddress facts executing runningSlot + 4) (timeoutCallback executing)
      , eq (timerAddress facts executing runningSlot + 8) (taskAddress facts executing runningSlot)
      ]
  , plain "release-free" Release executing 1
      (created facts executing 1 0 2 8 poolRegion
        ++ [poke (nextFreeCell facts executing 1) (poolRegion + 8)])
      0 0 False
      [ eq (recycledCell facts executing 1) poolRegion, eq poolRegion 0 ]
  , plain "release-waiter" Release executing 1
      (created facts executing 1 0 1 8 poolRegion
        ++ [poke (nextFreeCell facts executing 1) (poolRegion + 8)]
        ++ waiterAt facts executing 1 executing 2 3)
      0 0 True
      [ eq (waitBlockCell facts executing 2) poolRegion
      , eq (waitResult facts executing 2) 0
      , byte (taskAddress facts executing 2 + tfState (factsTask facts)) (stReady (factsStates facts))
      , eq (queueCell facts executing 1) (controlAddress facts executing 1)
      ]
  , (plain "release-waiter-peer" Release executing 1
      (created facts executing 1 0 1 8 poolRegion
        ++ [poke (nextFreeCell facts executing 1) (poolRegion + 8)] ++ waiterAt facts (peer executing) 1 executing 2 3)
      0 0 False
      [ eq (waitBlockCell facts (peer executing) 2) poolRegion ])
      { caseBank = Just (peer executing, 0x01000000) }
  , (plain "release-armed-timer" Release executing 1
      (created facts executing 1 0 1 8 poolRegion
        ++ [poke (nextFreeCell facts executing 1) (poolRegion + 8)]
        ++ waiterAt facts executing 1 executing 2 3
        ++ [poke (waitResult facts executing 2) timerToken])
      0 0 True
      [ eq (waitResult facts executing 2) 0 ])
      { caseReceipt = Just (recordAddress facts executing executing, timerToken) }
  , (plain "release-misaligned" Release executing 1
      (created facts executing 1 0 2 8 poolRegion
        ++ [poke (nextFreeCell facts executing 1) (poolRegion + 8)])
      0 (fromIntegral (-17 :: Int)) False [eq (recycledCell facts executing 1) 0])
      { caseR1 = Just (poolRegion + 4) }
  , (plain "release-out-of-range" Release executing 1
      (created facts executing 1 0 2 8 poolRegion
        ++ [poke (nextFreeCell facts executing 1) (poolRegion + 8)])
      0 (fromIntegral (-17 :: Int)) False [eq (recycledCell facts executing 1) 0])
      { caseR1 = Just (poolRegion - 4) }
  , (plain "release-end" Release executing 1
      (created facts executing 1 0 2 8 poolRegion
        ++ [poke (nextFreeCell facts executing 1) (poolRegion + 8)])
      0 (fromIntegral (-17 :: Int)) False [eq (recycledCell facts executing 1) 0])
      { caseR1 = Just (poolRegion + 16) }
  , (plain "release-double" Release executing 1
      (created facts executing 1 0 2 8 poolRegion
        ++ [ poke (nextFreeCell facts executing 1) (poolRegion + 8)
           , poke (recycledCell facts executing 1) poolRegion
           , poke poolRegion 0
           ])
      0 0 False
      [eq (recycledCell facts executing 1) poolRegion, eq poolRegion poolRegion])
      { caseOwned = Just (Expectation (fromIntegral (-17 :: Int)) False
          [eq (recycledCell facts executing 1) poolRegion, eq poolRegion 0])
      , caseDifference = Just "fixed-pool-double-release"
      }
  , (plain "release-unallocated" Release executing 1
      (created facts executing 1 0 2 8 poolRegion
        ++ [poke (nextFreeCell facts executing 1) poolRegion])
      0 0 False
      [eq (recycledCell facts executing 1) (poolRegion + 8)])
      { caseOwned = Just (Expectation (fromIntegral (-17 :: Int)) False
          [eq (recycledCell facts executing 1) 0])
      , caseDifference = Just "fixed-pool-unallocated-release"
      , caseR1 = Just (poolRegion + 8)
      }
  , plain "unchecked-created" PollUnchecked executing 1
      (created facts executing 1 0 1 8 poolRegion)
      0 0 False [eq outAddress poolRegion]
  , (plain "unchecked-deleted" PollUnchecked executing 1
      (created facts executing 1 0 1 8 poolRegion
        ++ [ poke (attributesCell facts executing 1) 0
           , poke (strideCell facts executing 1) 4
           , poke (beginCell facts executing 1) (dfDefaultPool (domainFacts facts executing))
           , poke (endCell facts executing 1) (dfDefaultPool (domainFacts facts executing) + 4)
           , poke (existsCell facts executing 1) 0
           , poke (nextFreeCell facts executing 1) (dfDefaultPool (domainFacts facts executing))
           ])
      0 0 False [eq outAddress (dfDefaultPool (domainFacts facts executing))])
      { caseOwned = Just (Expectation (fromIntegral (-42 :: Int)) False [noWrite outAddress])
      , caseDifference = Just "fixed-pool-unchecked-deleted-alias"
      }
  , (plain "unchecked-never-created" PollUnchecked executing 1 [] 0 0 False
      [eq outAddress (dfDefaultPool (domainFacts facts executing))])
      { caseOwned = Just (Expectation (fromIntegral (-42 :: Int)) False [noWrite outAddress])
      , caseDifference = Just "fixed-pool-unchecked-default-alias"
      }
  , (plain "unchecked-masked-handler" PollUnchecked executing 1
      (created facts executing 1 0 1 8 poolRegion)
      0 0 False [eq outAddress poolRegion])
      { caseHandler = True, caseMasked = True }
  , (plain "get-masked" Get executing 1 (created facts executing 1 0 1 8 poolRegion)
      0 (fromIntegral (-25 :: Int)) False [noWrite outAddress])
      { caseMasked = True }
  , (plain "get-handler" Get executing 1 (created facts executing 1 0 1 8 poolRegion)
      0 (fromIntegral (-25 :: Int)) False [noWrite outAddress])
      { caseHandler = True }
  , (plain "get-dispatch-disabled" Get executing 1 (created facts executing 1 0 1 8 poolRegion)
      0 (fromIntegral (-25 :: Int)) False [noWrite outAddress])
      { caseDispatch = False }
  , (plain "get-bad-class" Get executing 1 (created facts executing 1 0 1 8 poolRegion)
      0 (fromIntegral (-18 :: Int)) False [noWrite outAddress])
      { caseClass = Just 3 }
  , plain "get-bad-index" Get executing 123 (created facts executing 1 0 1 8 poolRegion)
      0 (fromIntegral (-18 :: Int)) False [noWrite outAddress]
  , plain "get-not-created" Get executing 1 [] 0 (fromIntegral (-42 :: Int)) False [noWrite outAddress]
  , plain "timed-bad-timeout" TimedGet executing 1 (created facts executing 1 0 1 8 poolRegion)
      (fromIntegral (-2 :: Int)) (fromIntegral (-17 :: Int)) False [noWrite outAddress]
  , (plain "poll-dispatch-disabled" Poll executing 1 (created facts executing 1 0 1 8 poolRegion)
      0 0 False [eq outAddress poolRegion])
      { caseDispatch = False }
  , (plain "release-dispatch-disabled" Release executing 1
      (created facts executing 1 0 2 8 poolRegion
        ++ [poke (nextFreeCell facts executing 1) (poolRegion + 8)])
      0 0 False [eq (recycledCell facts executing 1) poolRegion])
      { caseDispatch = False }
  , plain "priority-order" Release executing 1
      (created facts executing 1 0 1 8 poolRegion
        ++ [poke (nextFreeCell facts executing 1) (poolRegion + 8)]
        ++ twoWaiters facts executing 1 executing (secondWaiterSlot, 3) (waiterSlot, 15))
      0 0 True
      [ eq (waitBlockCell facts executing secondWaiterSlot) poolRegion
      , eq (queueCell facts executing 1) (taskAddress facts executing waiterSlot)
      ]
  , plain "fifo-order" Release executing 1
      (created facts executing 1 0 1 8 poolRegion
        ++ [poke (nextFreeCell facts executing 1) (poolRegion + 8)]
        ++ twoWaiters facts executing 1 executing (waiterSlot, 5) (secondWaiterSlot, 3))
      0 0 True
      [ eq (waitBlockCell facts executing waiterSlot) poolRegion
      , eq (queueCell facts executing 1) (taskAddress facts executing secondWaiterSlot)
      ]
  , (plain "release-handler" Release executing 1
      (created facts executing 1 0 2 8 poolRegion
        ++ [poke (nextFreeCell facts executing 1) (poolRegion + 8)])
      0 (fromIntegral (-25 :: Int)) False [eq (recycledCell facts executing 1) 0])
      { caseHandler = True }
  , (plain "get-resume-wake" Get executing 1 (resumeWords facts executing) 0 0 False
      [eq outAddress poolRegion])
      { caseResume = True }
  , (plain "timed-resume-wake" TimedGet executing 1 (resumeWords facts executing) 30 0 False
      [eq outAddress poolRegion])
      { caseResume = True
      , caseReceipt = Just (recordAddress facts executing executing, timerAddress facts executing runningSlot)
      }
  , (plain "timed-resume-expiry" TimedGet executing 1
      (resumeWordsWith facts executing expiryEntry) 30 (fromIntegral (-50 :: Int)) False
      [noWrite outAddress, eq (dfTimerCount (domainFacts facts executing)) 0])
      { caseResume = True }
  , (plain "get-resume-delete" Get executing 1
      (resumeWordsWith facts executing deleteEntry) 0 0 False [])
      { caseResume = True
      , caseStock = Expectation 0 True []
      , caseOwned = Just (Expectation (fromIntegral (-49 :: Int)) False [noWrite outAddress])
      , caseDifference = Just "fixed-pool-delete-releases-suspended-get"
      }
  ]

bindTimer :: Facts -> Domain -> Image -> Case -> Case
bindTimer facts executing image one
  | not (aggregate image) = one
  | Just (_, token) <- caseReceipt one =
      let entry = if token == timerToken then timerAddress facts executing waiterSlot else token
          seeded = if caseResume one then [] else
            [ poke (waitResult facts executing waiterSlot) entry
            , poke entry 1
            , poke (dfTimerCount dw) 1
            , poke (dfTimerHeap dw) 0
            , poke (dfTimerHeap dw + 4) entry
            ]
       in one {caseReceipt = Nothing, caseTimerIndex = Just (GuestAddress entry), caseWords = caseWords one ++ seeded}
  | caseName one == "timed-resume-expiry" =
      one {caseTimerIndex = Just (GuestAddress (timerAddress facts executing runningSlot))}
  | otherwise = one
  where
    dw = domainFacts facts executing

data Role = StockRole | OwnedRole deriving (Eq, Show)

layoutWords :: Facts -> [(GuestAddress, Word32)]
layoutWords facts =
  concat [recordPokes block described | block <- domains, described <- domains]
    ++ concatMap taskPokes domains
    ++ concatMap objectPokes domains
    ++ concatMap readyPokes domains
  where
    domains = [DomainA, DomainB]
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
          , poke (record + cfTimerHeap (factsContext facts)) (dfTimerHeap describedWords)
          , poke (record + cfTimerCount (factsContext facts)) (dfTimerCount describedWords)
          , poke (record + cfTimerBase (factsContext facts)) (dfTimerBase describedWords)
          , poke (record + cfTick (factsContext facts)) (dfTickNow describedWords)
          , poke (record + cfPools (factsContext facts)) (dfPoolControls describedWords)
          , poke (record + cfPoolCount (factsContext facts)) (factsPoolSlots facts)
          ]
          ++ lock (cfSchedulerLock (factsContext facts)) (dfSchedulerLock describedWords)
          ++ lock (cfObjectLock (factsContext facts)) (dfObjectLock describedWords)
    taskPokes domain =
      concat
      [ let address = taskAddress facts domain slot
            definition = definitionAddress facts domain slot
            priority = case slot of
              1 -> 10
              2 -> 3
              3 -> 15
              _ -> slot `mod` factsPriorities facts
         in [ poke address address
            , poke (address + tfPrev (factsTask facts)) address
            , poke (address + tfDefinition (factsTask facts)) definition
            , poke (address + tfDomain (factsTask facts)) (dfId (domainFacts facts domain))
            , poke (address + tfState (factsTask facts)) (packedState 0 priority)
            , poke (address + tfSavedSp (factsTask facts)) targetStack
            , poke (address + tfResumePc (factsTask facts)) stopAddress
            , poke (definition + defPriority (factsDefinition facts)) priority
            , poke (definition + 4) (0xa5510000 + slot)
            , poke (definition + 8) stopAddress
            , poke (definition + 0x10) (targetStack - 0x100)
            , poke (definition + 0x14) 0x100
            , poke (definition + 0x20) slot
            ]
      | slot <- [1 .. factsTaskSlots facts]
      ]
    objectPokes domain =
      concat
        [ let config = configAddress facts domain slot
              control = controlAddress facts domain slot
           in [ poke (config + cfgAttributes (factsConfig facts)) 0
              , poke (config + cfgBlockSize (factsConfig facts)) 4
              , poke (config + cfgBegin (factsConfig facts)) (dfDefaultPool (domainFacts facts domain))
              , poke (config + cfgEnd (factsConfig facts)) (dfDefaultPool (domainFacts facts domain) + 4)
              , poke (config + cfgExists (factsConfig facts)) 0
              , poke (control + ctlQueue (factsControl facts)) control
              , poke (control + ctlQueue (factsControl facts) + wqPrev (factsWaitQueue facts)) control
              , poke (control + ctlQueue (factsControl facts) + wqConfig (factsWaitQueue facts)) config
              , poke (control + ctlNextFree (factsControl facts)) (dfDefaultPool (domainFacts facts domain))
              , poke (control + ctlRecycled (factsControl facts)) 0
              ]
        | slot <- [1 .. factsPoolSlots facts]
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
                , poke (address + tfState (factsTask facts)) (packedState (stReady (factsStates facts)) priority)
                ]
       in concat
            [ [ poke (headAt priority) (headAt priority)
              , poke (headAt priority + hfPrev (factsHead facts)) (headAt priority)
              ]
            | priority <- [0 .. factsPriorities facts - 1]
            ]
            ++ member runningSlot 10
            ++ member idleSlot 15
            ++ [ poke (dfReadyBitmap dw) ((1 `shiftL` 10) .|. (1 `shiftL` 15))
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
      , dfPoolConfigs dw, dfPoolControls dw, dfPoolWaits dw, dfPoolTimers dw
      , dfTaskTable dw, dfDefaultPool dw
      ]
    high = maximum
      [ dfContextA dw + cfSize (factsContext facts)
      , dfContextB dw + cfSize (factsContext facts)
      , dfReadyHeads dw + factsPriorities facts * hfSize (factsHead facts)
      , dfSelected dw + 4
      , dfTaskTable dw + factsTaskSlots facts * tfSize (factsTask facts)
      , dfPoolConfigs dw + factsPoolSlots facts * cfgSize (factsConfig facts)
      , dfPoolControls dw + factsPoolSlots facts * ctlSize (factsControl facts)
      , dfPoolWaits dw + factsTaskSlots facts * pwSize (factsPoolWait facts)
      , dfPoolTimers dw + factsTaskSlots facts * factsTimerEntrySize facts
      , dfTimerHeap dw + 0x1000
      , dfTickNow dw + 4
      , dfDefaultPool dw + 4
      ]

callerSp :: Case -> Word32
callerSp one = if caseHandler one then handlerStack else callerStack

pages :: Facts -> Case -> [Page]
pages facts one =
  [ let (address, size) = kernelBounds facts DomainA
     in Page (RegionId "kernel-a") address size ReadWrite (B.replicate size 0)
  ]
    ++ [ let (address, size) = kernelBounds facts DomainB
         in Page (RegionId "kernel-b") address size ReadWrite (B.replicate size 0)
       ]
    ++ [ Page (RegionId "definitions") (GuestAddress definitionBase) (fromIntegral (2 * factsTaskSlots facts * definitionStride)) ReadWrite (B.replicate (fromIntegral (2 * factsTaskSlots facts * definitionStride)) 0)
       , Page (RegionId "request") (GuestAddress requestAddress) 0x20 ReadWrite (B.replicate 0x20 0)
       , Page (RegionId "pool-region") (GuestAddress poolRegion) 0x100 ReadWrite (B.replicate 0x100 0)
       , Page (RegionId "timer-seam") (GuestAddress receiptAddress) 0x100 ReadWrite (B.replicate 0x100 0)
       , Page (RegionId "global-lock") (GuestAddress globalLock) 4 ReadWrite (B.replicate 4 0)
       , Page (RegionId "stack") (GuestAddress (callerSp one - 0x400)) 0x400 ReadWrite (B.replicate 0x400 0)
       , Page (RegionId "caller-frame") (GuestAddress (callerSp one)) (B.length callerBytes) ReadWrite callerBytes
       , Page (RegionId "target-stack") (GuestAddress (targetStack - 0x100)) 0x100 ReadWrite (B.replicate 0x100 0)
       , Page (RegionId "driver-stack") (GuestAddress (driverStack - 0x100)) 0x100 ReadWrite (B.replicate 0x100 0)
       , Page (RegionId "bank-a") (bank DomainA) 4 Device (B.replicate 4 0)
       , Page (RegionId "bank-b") (bank DomainB) 4 Device (B.replicate 4 0)
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
symbolSpans image = [(Agg.symbolAddress symbol, Agg.symbolSize symbol) | symbol <- imageSymbols image, Agg.symbolSize symbol > 0]

ownedSpans :: Domain -> Role -> Image -> [(Word32, Word32)]
ownedSpans domain role owned
  | role == OwnedRole, aggregate owned = symbolSpans owned
  | role == StockRole = extents domain
  | otherwise = extents domain ++ [(bodyRegion, bodyLength)]

storageSpan :: Image -> (Word32, Word32)
storageSpan image = (imageBase image, fromIntegral (B.length (imageBytes image)))

codeBounds :: Domain -> Role -> Image -> (Word32, Word32)
codeBounds domain role owned =
  let spans = extents domain ++ retained domain ++ [(lowestBitAddress domain, 16)]
        ++ if role == StockRole then [] else if aggregate owned then [storageSpan owned] else [(bodyRegion, bodyLength)]
   in (minimum (map fst spans), maximum [address + size | (address, size) <- spans])

codeBytes :: Domain -> Role -> Image -> Image -> B.ByteString
codeBytes domain role loader owned
  | role == OwnedRole, aggregate owned = install (imageBase owned) (imageBytes owned) original
  | otherwise = withTimer (if role == OwnedRole then foldl patch original (ownedSpans domain role owned) else original)
  where
    (base, top) = codeBounds domain role owned
    slice image address size
      | address < imageBase image
          || toInteger address + toInteger size > toInteger (imageBase image) + toInteger (B.length (imageBytes image)) =
          error "fixed-pool code slice is outside its image"
      | otherwise = B.take (fromIntegral size) (B.drop (fromIntegral (address - imageBase image)) (imageBytes image))
    original = install low (slice loader low (high - low)) (B.replicate (fromIntegral (top - base)) 0)
    low = max base (imageBase loader)
    high = min top (imageBase loader + fromIntegral (B.length (imageBytes loader)))
    install address bytes current =
      let offset = fromIntegral (address - base)
       in B.take offset current <> bytes <> B.drop (offset + B.length bytes) current
    withTimer current
      | aggregate owned = current
      | otherwise = install (timerRemoval domain) timerStub current
    patch current (address, size) = install address (slice owned address size) current

continuationBytes :: Domain -> B.ByteString
continuationBytes domain = foldl install zero
  [(driverEntry, driverBytes domain), (expiryEntry, expiryBytes domain), (deleteEntry, deleteBytes domain)]
  where
    zero = B.replicate (fromIntegral (stopAddress - exitAddress + 4)) 0
    install current (address, bytes) =
      let offset = fromIntegral (address - exitAddress)
       in B.take offset current <> bytes <> B.drop (offset + B.length bytes) current

expectationOf :: Role -> Case -> Expectation
expectationOf StockRole one = caseStock one
expectationOf OwnedRole one = maybe (caseStock one) id (caseOwned one)

caseSpec :: Facts -> Domain -> Role -> Image -> Image -> Case -> CaseSpec
caseSpec facts executing role loader owned one
  | not (null outside) = error (caseName one ++ ": unmapped fixture words " ++ show outside)
  | otherwise = CaseSpec
      { caseId = emittedId executing one
      , caseProvenance = "X-M1 1.50 fixed-pool family, pinned x70view 55436fc85c0560dd252b0410af6f2427fe198cd3; synthetic task/object state"
      , caseExclusion = "Private compiler frames are not byte-compared. Dispatch rows check selected-task arrival; resumed rows check the live caller frame, service return, output, SP, callee-saved registers and CPSR control. Aggregate cases execute linked timer removal and tick; standalone cases retain the timer argument seam."
      , caseEntry = wire (entries executing (caseService one))
      , caseExit = wire exitAddress
      , caseRegions =
          codeRegions
            ++ [mkRegion ident (wire (guestWord address)) size permission (overlay address background words') | Page ident address size permission background <- dataPages]
            ++ [mkRegion (RegionId "continuations") (wire exitAddress) (fromIntegral (stopAddress - exitAddress + 4)) ReadExecute (continuationBytes executing)]
      , caseFunctions = [(wire address, size) | (address, size) <- functionSpans ++ [(driverEntry, driverSize), (expiryEntry, expirySize), (deleteEntry, deleteSize)]]
      , caseInitial = initialRegisters executing one
      , caseExpectedStatus = if arrival then StopAddress else Returned
      , caseExpectedRegisters = if arrival
          then [(Pc, stopAddress), (Sp, arrivalSp role)]
          else (R0, expectationResult expectation) : [(register, seed register) | register <- [R4 .. R11]] ++ [(Sp, callerSp one)]
      , caseExpectedMemory = [(RegionId "caller-frame", callerBytes)]
      , caseCp14Identity = Just (if executing == DomainA then 0x4003 else 0x4004)
      , caseKernel = Just (KernelCase plainLimit (if arrival then Just (wire stopAddress) else Nothing) (Just 0x80001234) (Just 0) (Just 0) ignored contexts)
      }
  where
    expectation = expectationOf role one
    arrival = expectationArrival expectation
    placementFunctions = [(Agg.placementAddress placement, Agg.placementBytes placement) | role == OwnedRole, placement <- imagePlacements owned, Agg.placementBytes placement > 0, Agg.placementSection placement == ".text" || ".text." `isPrefixOf` Agg.placementSection placement]
    allocated = symbolSpans owned ++ [(Agg.placementAddress placement, Agg.placementBytes placement) | placement <- imagePlacements owned]
    allocatedSections = [(Agg.placementAddress p, Agg.placementBytes p) | p <- imagePlacements owned, Agg.placementBytes p > 0]
    overlaps (left, size) (right, width) = left < right + width && right < left + size
    preserved = [extent | extent <- retained executing, role /= OwnedRole || not (any (overlaps extent) allocated)]
    functionSpans
      | role == OwnedRole && aggregate owned = preserved ++ concatMap splitFunctions placementFunctions
          ++ [extent | extent <- symbolSpans owned, not (any (overlaps extent) allocatedSections)]
      | otherwise = ownedSpans executing role owned ++ preserved
    splitFunctions (address, size) =
      let starts = sort (nub (address : address + size :
            [at | (at, _) <- symbolSpans owned, at > address, at < address + size]))
       in [(lo, hi - lo) | (lo, hi) <- zip starts (drop 1 starts)]
    (codeBase, codeTop) = codeBounds executing role owned
    readonly = [(Agg.placementAddress p, Agg.placementAddress p + Agg.placementBytes p)
      | role == OwnedRole, p <- imagePlacements owned, Agg.placementBytes p > 0
      , not (Agg.placementSection p == ".text" || ".text." `isPrefixOf` Agg.placementSection p)]
    cuts = sort (nub (codeBase : codeTop : concat [[lo, hi] | (lo, hi) <- readonly]))
    installed = codeBytes executing role loader owned
    codeRegions =
      [mkRegion (RegionId ("code-" ++ show index)) (wire lo) (fromIntegral (hi - lo))
        (if any (\(start, end) -> start <= lo && hi <= end) readonly then ReadOnly else ReadExecute)
        (B.take (fromIntegral (hi - lo)) (B.drop (fromIntegral (lo - codeBase)) installed))
      | (index, (lo, hi)) <- zip [0 :: Int ..] (zip cuts (drop 1 cuts)), hi > lo]
    dataPages = pages facts one
    words' = layoutWords facts
      ++ [ poke (dfHandlerContext (domainFacts facts executing)) (if caseHandler one then 1 else 0)
         , poke (dfDispatchEnabled (domainFacts facts executing)) (if caseDispatch one then 1 else 0)
         ]
      ++ caseWords one
    outside = [address | (address, _) <- words', not (any (contains address 4) dataPages)]
    ignored = if arrival then [R0 .. R12] ++ [Lr] else [R1 .. R12] ++ [Lr]
    contexts
      | caseResume one =
          [ stackContext (callerSp one - 0x400) (callerSp one)
          , stackContext (driverStack - 0x100) driverStack
          ]
      | arrival =
          [ stackContext (callerSp one - 0x400) (callerSp one)
          , stackContext (targetStack - 0x100) targetStack
          ]
      | otherwise = []
    stackContext low high = case mkStackContext (wire low) (wire high) of
      Left problem -> error problem
      Right context -> context
    arrivalSp StockRole | caseResume one = driverStack
    arrivalSp _ = targetStack

seed :: Register -> Word32
seed register = 0xa5a50000 + fromIntegral (fromEnum register)

initialRegisters :: Domain -> Case -> [(Register, Word32)]
initialRegisters executing one =
  [(register, value register) | register <- [R0 .. R12]]
    ++ [(Sp, callerSp one), (Lr, exitAddress), (Cpsr, cpsr)]
  where
    cpsr
      | caseMasked one = 0x93
      | caseHandler one = 0x93
      | otherwise = 0x13
    value R0 = handle
    value R1 = case caseR1 one of
      Just raw -> raw
      Nothing -> case caseService one of
        Create -> requestAddress
        Get -> outAddress
        Poll -> outAddress
        TimedGet -> outAddress
        Release -> poolRegion
        PollUnchecked -> outAddress
        Delete -> 0xa5a50001
    value R2 = if caseService one == TimedGet then caseTimeout one else 0xa5a50002
    value register = seed register
    handle = case caseClass one of
      Just cls -> (cls `shiftL` 16) .|. caseSlot one
      Nothing
        | caseObject one == executing -> caseSlot one
        | otherwise -> (if caseObject one == DomainA then 1 else 2) `shiftL` 16 .|. caseSlot one

emittedId :: Domain -> Case -> String
emittedId executing one = caseName one ++ "-" ++ domainLabel executing

wire :: Word32 -> N.GuestAddress
wire = N.GuestAddress . fromIntegral

guestWord :: GuestAddress -> Word32
guestWord (GuestAddress raw) = raw

readWord :: Facts -> Case -> NativeObservation -> GuestAddress -> Maybe Word32
readWord facts one observation address@(GuestAddress raw) = case
  [ (base, bytes)
  | page@(Page ident (GuestAddress base) _ _ _) <- pages facts one
  , contains address 4 page
  , Just bytes <- [lookup ident (nativeMemory observation)]
  ]
  of
    (base, bytes) : _ -> Just (readWordAt bytes (fromIntegral (raw - base)))
    [] -> Nothing

readByte :: Facts -> Case -> NativeObservation -> GuestAddress -> Maybe Word8
readByte facts one observation address@(GuestAddress raw) = case
  [ (base, bytes)
  | page@(Page ident (GuestAddress base) _ _ _) <- pages facts one
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

holds :: Facts -> Case -> NativeObservation -> Check -> Bool
holds facts one observation check = case check of
  Equals address value -> readWord facts one observation address == Just value
  ByteEquals address value -> readByte facts one observation address == Just value
  NoWrite address -> null (writesAt observation address)

compareRole :: Facts -> Role -> CaseSpec -> Case -> Expectation -> NativeObservation -> [String]
compareRole facts role spec one expectation observation =
  [ "wrong exit " ++ show (caseOutcome spec observation)
  | caseOutcome spec observation /= wanted
  ]
    ++ [ "result " ++ show resultActual ++ " is not " ++ show (expectationResult expectation)
       | not (expectationArrival expectation), resultActual /= Just (expectationResult expectation)
       ]
    ++ [ "CPSR control " ++ show controlActual ++ " is not " ++ show expectedControl
       | controlActual /= Just expectedControl
       ]
    ++ [ "caller frame changed"
       | lookup (RegionId "caller-frame") (nativeMemory observation) /= Just callerBytes
       ]
    ++ [ "arrival pc " ++ show actual ++ " is not " ++ show stopAddress
       | expectationArrival expectation, let actual = N.lookupRegister Pc observation, actual /= Just stopAddress
       ]
    ++ [ "arrival sp " ++ show actual ++ " is not " ++ show expectedArrivalSp
       | expectationArrival expectation, let actual = N.lookupRegister Sp observation, actual /= Just expectedArrivalSp
       ]
    ++ [ "return sp " ++ show actual ++ " is not " ++ show (callerSp one)
       | not (expectationArrival expectation), let actual = N.lookupRegister Sp observation, actual /= Just (callerSp one)
       ]
    ++ [ "caller register " ++ show register ++ " changed"
       | not (expectationArrival expectation)
       , register <- [R4 .. R11]
       , N.lookupRegister register observation /= Just (seed register)
       ]
    ++ [ "missing effect " ++ show check
       | check <- expectationChecks expectation
       , not (holds facts one observation check)
       ]
    ++ [ "bank " ++ show domain ++ " holds " ++ show actual
       | domain <- [DomainA, DomainB]
       , let actual = readWord facts one observation (bank domain)
       , actual /= Just (expectedBank domain)
       ]
    ++ [ "timer receipt differs"
       | (expectedOwner, expectedEntry) <- maybe [] pure (caseReceipt one)
       , ( readWord facts one observation (GuestAddress receiptAddress)
         , readWord facts one observation (GuestAddress (receiptAddress + 4))
         ) /= (Just expectedOwner, Just expectedEntry)
       ]
    ++ [ "unexpected timer receipt"
       | caseReceipt one == Nothing
       , readWord facts one observation (GuestAddress receiptAddress) /= Just 0
         || readWord facts one observation (GuestAddress (receiptAddress + 4)) /= Just 0
       ]
  where
    wanted = if expectationArrival expectation then OutcomeStopped (wire stopAddress) else OutcomeReturned
    resultActual = N.lookupRegister R0 observation
    controlActual = fmap (.&. 0x1ff) (N.lookupRegister Cpsr observation)
    expectedArrivalSp
      | role == StockRole && caseResume one = driverStack
      | otherwise = targetStack
    expectedControl
      | expectationArrival expectation = 0x93
      | caseMasked one = 0x93
      | caseHandler one = 0x93
      | otherwise = 0x13
    expectedBank domain = case caseBank one of
      Just (target, value) | target == domain -> value
      _ -> 0

compareFamily :: Facts -> Domain -> Case -> CaseSpec -> CaseSpec -> NativeObservation -> NativeObservation -> [String]
compareFamily facts executing one stockSpec ownedSpec stock owned =
  compareRole facts StockRole stockSpec one (caseStock one) stock
    ++ compareRole facts OwnedRole ownedSpec one ownedExpectation owned
    ++ timerIndexErrors facts one StockRole stock
    ++ timerIndexErrors facts one OwnedRole owned
    ++ ( if caseOwned one == Nothing
          then memoryDifferences
          else
            [ "the owned role did not differ from the defective stock expectation"
            | null (compareRole facts OwnedRole ownedSpec one (caseStock one) owned)
            ]
       )
  where
    ownedExpectation = maybe (caseStock one) id (caseOwned one)
    compared = [ident | Page ident _ _ _ _ <- pages facts one, ident `notElem` map RegionId ["stack", "target-stack", "driver-stack"]]
    memory observation = M.fromList [(ident, normalizeTimer ident (normalize ident bytes)) | (ident, bytes) <- nativeMemory observation, ident `elem` compared]
    kernelIdents = if executing == DomainA then [RegionId "kernel-a"] else [RegionId "kernel-b"]
    normalize ident bytes
      | ident `elem` kernelIdents && (expectationArrival (caseStock one) || caseResume one) =
          case [base | Page region (GuestAddress base) _ _ _ <- pages facts one, region == ident] of
            [base] ->
              let slots = if caseResume one then [runningSlot, idleSlot] else [runningSlot]
                  zero current saved =
                    let offset = fromIntegral (toInteger (taskAddress facts executing saved) + toInteger (tfSavedSp (factsTask facts)) - toInteger base)
                     in if offset < 0 || offset + 4 > B.length current
                          then current
                          else B.take offset current <> word32le 0 <> B.drop (offset + 4) current
               in foldl zero bytes slots
            _ -> bytes
      | otherwise = bytes
    normalizeTimer ident bytes = case caseTimerIndex one of
      Nothing -> bytes
      Just address -> case
        [fromIntegral (guestWord address - guestWord base) | page@(Page actual base _ _ _) <- pages facts one, actual == ident, contains address 4 page] of
          [offset] -> B.take offset bytes <> word32le 0 <> B.drop (offset + 4) bytes
          _ -> bytes
    memoryDifferences =
      ["missing state region " ++ show ident | ident <- compared, M.notMember ident (memory stock) || M.notMember ident (memory owned)]
        ++ ["state region " ++ show ident ++ " differs" | ident <- compared, M.lookup ident (memory stock) /= M.lookup ident (memory owned)]

timerIndexErrors :: Facts -> Case -> Role -> NativeObservation -> [String]
timerIndexErrors facts one role observation = case caseTimerIndex one of
  Nothing -> []
  Just address ->
    ["timer index violates the named Detached-index contract"
    | readWord facts one observation address /= Just expected || writes address /= expectedWrites]
  where
    expected = if role == OwnedRole then 0 else 1
    expectedWrites = [(4, 1) | caseResume one] ++ [(4, 0) | role == OwnedRole]
    writes address =
      [(width, fromIntegral value :: Word32)
      | Just kernel <- [nativeKernel observation]
      , Just events <- [observedOrder kernel]
      , OrderedWrite (Write actual (WriteBytes width)) (WriteValue value) <- events
      , actual == wire (guestWord address)]

timerControls :: Facts -> Domain -> Case -> CaseSpec -> CaseSpec -> NativeObservation -> NativeObservation -> [(String, Either String [String])]
timerControls facts executing one stockSpec ownedSpec stock owned = case caseTimerIndex one of
  Nothing -> []
  Just address ->
    [check "owned-relink-refused" stock (mutate address 1 owned)
    , check "stock-detach-refused" (mutate address 0 stock) owned
    , check "missing-detach-write-refused" stock (owned {nativeKernel = fmap withoutDetach (nativeKernel owned)})
    , check "live-caller-frame-mutation-refused" stock (mutate (GuestAddress (callerSp one)) 0 owned)]
    where
      check name reference subject = (caseName one ++ "/" ++ name, Right
        ["corruption escaped the oracle" | null (compareFamily facts executing one stockSpec ownedSpec reference subject)])
      withoutDetach kernel = kernel {observedOrder = fmap (filter keep) (observedOrder kernel)}
      keep (OrderedWrite (Write actual _) (WriteValue 0)) = actual /= wire (guestWord address)
      keep _ = True
      mutate target value observation = observation {nativeMemory = map replace (nativeMemory observation)}
        where
          replace (ident, bytes) = case
            [fromIntegral (guestWord target - guestWord base) | page@(Page actual base _ _ _) <- pages facts one, actual == ident, contains target 4 page] of
              [offset] -> (ident, B.take offset bytes <> word32le value <> B.drop (offset + 4) bytes)
              _ -> (ident, bytes)

readonlyCases :: Image -> CaseSpec -> [(RunnerStatus, CaseSpec)]
readonlyCases image template =
  [ (status, template
      { caseId = label ++ "-" ++ show address
      , caseEntry = wire probe
      , caseRegions = mkRegion (RegionId "readonly-probe") (wire probe) (B.length program) ReadExecute program : caseRegions template
      , caseFunctions = (wire probe, fromIntegral (B.length program)) : caseFunctions template
      , caseExpectedStatus = status
      , caseExpectedRegisters = []
      , caseExpectedMemory = []
      })
  | placement <- imagePlacements image
  , Agg.placementBytes placement >= 4
  , let section = Agg.placementSection placement
  , not (section == ".text" || ".text." `isPrefixOf` section)
  , let address = Agg.placementAddress placement
  , (label, status, instruction) <-
      [("readonly-write", MemoryGuard, 0xe5801000), ("readonly-fetch", FetchGuard, 0xe12fff10)]
  , let probe = receiptAddress + 0x200
        program = assemble [movw 0 (address .&. 0xffff), movt 0 (address `shiftR` 16), instruction, 0xe12fff1e]
  ]

runFamily :: FilePath -> FilePath -> FilePath -> Facts -> Domain -> Image -> Image -> IO Run
runFamily runner cacheRoot directory facts executing stock owned = do
  let selected = map (bindTimer facts executing owned) (cases facts executing)
      stockCases = [(one, caseSpec facts executing StockRole stock owned one) | one <- selected]
      ownedCases = [(one, caseSpec facts executing OwnedRole stock owned one) | one <- selected]
  stockBatch <- writeBatch (directory </> "stock") (map snd stockCases)
  ownedBatch <- writeBatch (directory </> "owned") (map snd ownedCases)
  (stockReuse, stockRaw) <- cachedObservedBatch cacheRoot runner stockBatch
  (ownedReuse, ownedRaw) <- cachedObservedBatch cacheRoot runner ownedBatch
  refusalResults <- case ownedCases of
    [] -> pure []
    (_, template) : _ -> do
      let refusals = readonlyCases owned template
      if null refusals then pure [] else do
        batch <- writeBatch (directory </> "readonly-controls") (map snd refusals)
        (_, observed) <- cachedObservedBatch cacheRoot runner batch
        let byId = M.fromList [(caseIdText ident, observation) | (ident, observation) <- observed]
        pure [(caseId spec, case M.lookup (caseId spec) byId of
          Nothing -> Left "readonly refusal did not run"
          Just observation -> Right ["wrong readonly refusal " ++ show (nativeStatus observation) | nativeStatus observation /= status])
          | (status, spec) <- refusals]
  let reference = M.fromList [(caseIdText ident, observation) | (ident, observation) <- stockRaw]
      subject = M.fromList [(caseIdText ident, observation) | (ident, observation) <- ownedRaw]
  results <- forM stockCases $ \(one, stockSpec) -> do
    let ident = emittedId executing one
        ownedSpec = case lookup (caseName one) [(caseName candidate, spec) | (candidate, spec) <- ownedCases] of
          Just spec -> spec
          Nothing -> error "the owned spec is missing for a case"
    pure (caseName one, case (M.lookup ident reference, M.lookup ident subject) of
      (Just original, Just replacement) -> Right (compareFamily facts executing one stockSpec ownedSpec original replacement)
      (Nothing, _) -> Left "stock case did not run"
      (_, Nothing) -> Left "owned case did not run")
  unless (length stockRaw == length selected && length ownedRaw == length selected) (fail "native observations do not cover the current fixed-pool case set")
  let controls = concat
        [timerControls facts executing one stockSpec ownedSpec original replacement
        | (one, stockSpec) <- stockCases
        , Just ownedSpec <- [lookup (caseName one) [(caseName candidate, spec) | (candidate, spec) <- ownedCases]]
        , Just original <- [M.lookup (emittedId executing one) reference]
        , Just replacement <- [M.lookup (emittedId executing one) subject]]
      payloadResults
        | not (aggregate owned) = []
        | otherwise =
            let installed = codeBytes executing OwnedRole stock owned
                (base, top) = codeBounds executing OwnedRole owned
                offset = fromIntegral (imageBase owned - base)
                size = B.length (imageBytes owned)
                canonicalBytes = codeBytes executing StockRole stock owned
                (stockBase, stockTop) = codeBounds executing StockRole owned
                canonical address =
                  B.index installed (fromIntegral (address - base)) == B.index canonicalBytes (fromIntegral (address - stockBase))
             in [("whole-effective-carrier-bytes", Right
                    ["effective carrier bytes changed" | B.take size (B.drop offset installed) /= imageBytes owned])
                , ("canonical-unowned-gap-bytes", Right
                    ["canonical unowned bytes changed" | not (all canonical
                      [address | address <- [max base stockBase .. min top stockTop - 1]
                      , address < imageBase owned || address >= imageBase owned + fromIntegral size])])]
      allResults = results ++ controls ++ refusalResults ++ payloadResults
  pure (Run executing allResults (length [() | (_, Right []) <- allResults]) (stockReuse, ownedReuse))
