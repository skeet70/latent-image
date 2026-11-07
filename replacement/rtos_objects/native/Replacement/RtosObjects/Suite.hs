{-# LANGUAGE OverloadedStrings #-}

-- | The F2 family-native cases over both entry domains, each run against the stock loader
-- bytes and the owned image and compared observation by observation.
--
-- The data layout is the scheduler oracle's: every context record, lock record, ready cell
-- and task record is planted from layout.json. The +0xd4 contract stays refused in the
-- manifest; these cases are the oracle hypothesis.
module Replacement.RtosObjects.Suite
  ( Family (..)
  , ServiceId (..)
  , Slot (..)
  , Exit (..)
  , Case (..)
  , Image (..)
  , Expect (..)
  , cases
  , caseFor
  , compareFamily
  , runFamily
  , FamilyRun (..)
  , extents
  ) where

import Control.Monad (forM)
import qualified Data.ByteString as B
import Data.Bits ((.&.), (.|.), shiftL, shiftR)
import qualified Data.Map.Strict as M
import Data.Word (Word32)
import Emulation.NativeObservation (GuestAddress (..), NativeObservation (..), RegionId (..), Register (..), RunnerStatus (..), registerValue, regionIdOf)
import Emulation.CachedNativeBatch (BatchReuse (..), cachedObservedBatch)
import Emulation.NativeCase (CaseId (..), caseIdText)
import Emulation.OwnedCase (CaseOutcome (..), CaseRegion (..), CaseSpec (..), KernelCase (..), Permission (..), assemble, caseOutcome, mkRegion, plainLimit, word32le, writeBatch)
import Replacement.RtosObjects.Layout
import Replacement.RtosOwnFirst.Regions (Domain (..))
import Replacement.RtosScheduler.Oracle (stripScratch)
import System.FilePath ((</>))

data Family = SetFlag | Send
  deriving (Eq, Show)

-- | The owner record a release call receives in r0.
newtype Owner = Owner { ownerRecord :: GuestAddress }
  deriving (Eq, Show)

-- | One entry call: an object service with its id, or the owner release with its record.
data Call
  = ObjectService Family ServiceId
  | OwnerRelease Owner
  deriving (Eq, Show)

-- | The caller's mode at the entry.
data Invocation = TaskInvocation | HandlerInvocation
  deriving (Eq, Show)

-- | The service id in r0: the object's class in bits 16 and up, the slot below.
newtype ServiceId = ServiceId { serviceWord :: Word32 }
  deriving (Eq, Show)

-- | A one-based slot in the object's table.
newtype Slot = Slot { slotWord :: Word32 }
  deriving (Eq, Show)

-- | Where a case must end: at the caller's return with the service's r0, or at the selected
-- task's resume point when the wake dispatches.
data Exit
  = Returns
  | DispatchesTo GuestAddress
  deriving (Eq, Show)

-- | One observable the owned run must show.
data Expect
  = Equals GuestAddress Word32
  | Nonzero GuestAddress
  | Result Word32
  deriving (Eq, Show)

-- | One case: the words it plants over the layout, the registers it enters with, the timer
-- stub's return, the exit kind and the effects it must show.
data Case = Case
  { caseName :: String
  , caseCall :: Call
  , caseInvocation :: Invocation
  , caseRegisters :: [(Register, Word32)]
  , caseWords :: [(GuestAddress, Word32)]
  , caseTimer :: Word32
  , caseExitKind :: Exit
  , caseExpect :: [Expect]
  }

-- | The image a body is taken from.
data Image = Image
  { imageBase :: Word32
  , imageBytes :: B.ByteString
  }

definitionBase, definitionStride, stackBase, stackPointer, handlerStackPointer, callerTop, exitAddress, receiptAddress :: Word32
definitionBase = 0x0096c000
definitionStride = 0x20
stackBase = 0x00959700
stackPointer = 0x00959800
handlerStackPointer = 0x00959900
callerTop = 0x00959810
exitAddress = 0x00958f00
receiptAddress = 0x00960000

waiterSlot, runningSlot :: Slot
waiterSlot = Slot 2
runningSlot = Slot 1

recordAddress :: Facts -> Domain -> Domain -> Word32
recordAddress facts executing of' = case of' of
  DomainA -> dfContextA (domainFacts facts executing)
  DomainB -> dfContextB (domainFacts facts executing)

definitionAddress :: Facts -> Domain -> Slot -> Word32
definitionAddress facts domain slot =
  definitionBase + fromIntegral ((index domain * 120 + slotWord slot - 1) * fromIntegral definitionStride)
  where
    index DomainA = 0
    index DomainB = 1

trampoline :: Domain -> Word32
trampoline DomainA = 0x43728
trampoline DomainB = 0xd3728

lockEntries :: Domain -> (Word32, Word32)
lockEntries DomainA = (0x439d8, 0x43a14)
lockEntries DomainB = (0xd398c, 0xd39c8)

timerEntry :: Domain -> Word32
timerEntry DomainA = 0x44ca8
timerEntry DomainB = 0xd4c68

raiseIrqEntry :: Domain -> Word32
raiseIrqEntry DomainA = 0x433e4
raiseIrqEntry DomainB = 0xd33e4

releaseEntry :: Domain -> Word32
releaseEntry DomainA = 0x44568
releaseEntry DomainB = 0xd4528

releaseSize :: Word32
releaseSize = 0xcc

entryAddress :: Domain -> Family -> Word32
entryAddress DomainA SetFlag = 0x4813c
entryAddress DomainA Send = 0x4aa44
entryAddress DomainA Release = 0x44568
entryAddress DomainB SetFlag = 0xd80fc
entryAddress DomainB Send = 0xdaa04
entryAddress DomainB Release = 0xd4528

-- | The stock extents of S1, S2, the wait release and the two F2 bodies.
extents :: Domain -> [(Word32, Word32)]
extents DomainA =
  [ (0x40b00, 0x24), (0x43640, 0x28), (0x43668, 0x20), (0x43688, 0xa0), (0x43728, 0x34), (0x4375c, 0x48), (0x437a4, 0x38), (0x437fc, 0x14)
  , (0x439d8, 0x3c), (0x43a14, 0x28), (0x43c48, 0x5c), (0x43ca4, 0x7c), (0x43d20, 0xb0), (0x43dd0, 0x48), (0x43f70, 0x2c), (0x44018, 0x2c), (0x44044, 0xf0), (0x44134, 0x80)
  , (0x44568, releaseSize)
  , (0x4813c, 0x260), (0x4aa44, 0x304)
  ]
extents DomainB =
  [ (0xd0b00, 0x24), (0xd3640, 0x28), (0xd3668, 0x20), (0xd3688, 0xa0), (0xd3728, 0x34), (0xd375c, 0x48), (0xd37a4, 0x38), (0xd37fc, 0x14)
  , (0xd398c, 0x3c), (0xd39c8, 0x28), (0xd3c04, 0x5c), (0xd3c60, 0x7c), (0xd3cdc, 0xb0), (0xd3d8c, 0x48), (0xd3f2c, 0x2c), (0xd3fd4, 0x30), (0xd4004, 0xf0), (0xd40f4, 0x80)
  , (0xd4528, releaseSize)
  , (0xd80fc, 0x260), (0xdaa04, 0x304)
  ]

-- | The record, lock, cell and task pokes, as the scheduler oracle plants them.
layoutPokes :: Facts -> Domain -> [(GuestAddress, Word32)]
layoutPokes facts executing =
  concat [recordPokes block of' | block <- [DomainA, DomainB], of' <- [DomainA, DomainB]]
    ++ cellPokes
    ++ taskPokes
  where
    recordPokes block of' =
      let record = recordAddress facts executing of'
          w = domainFacts facts of'
          (acquire, release) = lockEntries block
          lockRecord offset =
            [ (GuestAddress (record + offset + factsLockWord facts), 0)
            , (GuestAddress (record + offset + factsLockAcquire facts), acquire)
            , (GuestAddress (record + offset + factsLockRelease facts), release)
            ]
       in [ (GuestAddress (record + factsContextId facts), if block == of' then factsOwnId facts else dfId w)
          , (GuestAddress (record + factsContextOwner facts), dfId w)
          , (GuestAddress (record + factsContextReadyHeads facts), dfReadyHeads w)
          , (GuestAddress (record + factsContextReadyBitmap facts), dfReadyBitmap w)
          , (GuestAddress (record + factsContextRunning facts), dfRunning w)
          , (GuestAddress (record + factsContextSelected facts), dfSelected w)
          , (GuestAddress (record + factsContextDispatchEnabled facts), dfDispatchEnabled w)
          , (GuestAddress (record + factsContextFirstEntry facts), trampoline of')
          ]
            ++ lockRecord (factsContextSchedulerLock facts)
            ++ lockRecord (factsContextObjectLock facts)
    cellPokes =
      let w = domainFacts facts executing
          headAt q = dfReadyHeads w + q * factsHeadSize facts
       in concat [[(GuestAddress (headAt q), headAt q), (GuestAddress (headAt q + factsHeadPrev facts), headAt q)] | q <- [0 .. factsPriorities facts - 1]]
            ++ [ (GuestAddress (dfReadyBitmap w), 0)
               , (GuestAddress (dfSelected w), 0)
               , (GuestAddress (dfRunning w), taskAt runningSlot)
               , (GuestAddress (dfDispatchEnabled w), 1)
               , (GuestAddress (dfSchedulerLock w), 0)
               , (GuestAddress (dfObjectLock w), 0)
               ]
    taskPokes =
      concat
        [ [ (GuestAddress (taskAt slot + factsTaskDefinition facts), definitionAddress facts executing slot)
          , (GuestAddress (taskAt slot + factsTaskDomain facts), dfId (domainFacts facts executing))
          , (GuestAddress (taskAt slot + factsTaskState facts), fromIntegral (factsStateDormant facts) .|. ((slotWord slot `mod` factsPriorities facts) `shiftL` 8))
          , (GuestAddress (taskAt slot + factsTaskSavedSp facts), 0x00958000 + slotWord slot * 0x100)
          , (GuestAddress (taskAt slot + factsTaskResumePc facts), 0x00959000 + slotWord slot * 0x100)
          , (GuestAddress (definitionAddress facts executing slot + factsDefinitionPriority facts), slotWord slot `mod` factsPriorities facts)
          , (GuestAddress (definitionAddress facts executing slot + 0x10), 0x00950000 + slotWord slot * 0x100)
          , (GuestAddress (definitionAddress facts executing slot + 0x14), 0x100)
          ]
        | slot <- map Slot [1 .. 120]
        ]
    taskAt slot = task facts executing (slotWord slot)

-- | The service id for an object of @domain@'s class at slot 1.
serviceOf :: Facts -> Domain -> Slot -> ServiceId
serviceOf facts domain slot = ServiceId ((dfId (domainFacts facts domain) `shiftL` 16) .|. slotWord slot)

-- | The ten cases of reports/rtos-platform-options.md 4.14.6.
cases :: Facts -> Domain -> [Case]
cases facts domain =
  let w = domainFacts facts domain
      own = dfId w
      peerDomain = case domain of DomainA -> DomainB; DomainB -> DomainA
      peer = dfId (domainFacts facts peerDomain)
      flag = flagConfig facts domain 1
      control = flagControl facts domain 1
      wait = flagWait facts domain (slotWord waiterSlot)
      mailbox = mailboxConfig facts domain 1
      mailControl = mailboxControl facts domain 1
      mailWait = mailboxWait facts domain (slotWord waiterSlot)
      t = task facts domain (slotWord waiterSlot)
      dispatch = dfDispatchEnabled w
      doorbell = GuestAddress 0xfffb40cc
      resume = GuestAddress (0x00959000 + slotWord waiterSlot * 0x100)
      flagBase =
        [ (GuestAddress (flag + factsFlagConfigExists facts), 1)
        , (GuestAddress (control + factsWaitQueueNext facts), control)
        , (GuestAddress (control + factsWaitQueuePrev facts), control)
        , (GuestAddress (control + factsFlagControlPattern facts), 0)
        ]
      waiter state =
        [ (GuestAddress (t + factsTaskNext facts), t)
        , (GuestAddress (t + factsTaskPrev facts), t)
        , (GuestAddress (t + factsTaskState facts), state .|. (3 `shiftL` 8))
        , (GuestAddress (t + factsTaskDomain facts), own)
        , (GuestAddress (t + factsTaskWait facts), wait)
        ]
      queued = fromIntegral (factsStateWait facts) + fromIntegral (factsStateObjectQueued facts)
      peerFlag = flagConfig facts peerDomain 1
      peerControl = flagControl facts peerDomain 1
      peerWait = flagWait facts peerDomain (slotWord waiterSlot)
      peerFlagBase =
        [ (GuestAddress (peerFlag + factsFlagConfigExists facts), 1)
        , (GuestAddress (peerControl + factsWaitQueueNext facts), peerControl)
        , (GuestAddress (peerControl + factsWaitQueuePrev facts), peerControl)
        , (GuestAddress (peerControl + factsFlagControlPattern facts), 0)
        ]
      waitWords pattern mode =
        [ (GuestAddress (control + factsWaitQueueNext facts), t)
        , (GuestAddress (wait + factsWaitBlockQueue facts), control)
        , (GuestAddress (wait + factsFlagWaitPattern facts), pattern)
        , (GuestAddress (wait + factsFlagWaitMode facts), mode)
        ]
   in [ Case "set_no_waiter" (ObjectService SetFlag (serviceOf facts domain (Slot 1))) TaskInvocation [(R1, 1)] flagBase 0 Returns [Result 0, Equals (GuestAddress (control + factsFlagControlPattern facts)) 1]
      , Case "set_satisfied" (ObjectService SetFlag (serviceOf facts domain (Slot 1))) TaskInvocation [(R1, 1)] (flagBase ++ waitWords 1 1 ++ waiter queued) 0 (DispatchesTo resume) [Result 0, Equals (GuestAddress (wait + factsFlagWaitResult facts)) 1, Equals (GuestAddress (t + factsTaskState facts)) (fromIntegral (factsStateReady facts))]
      , Case "set_unsatisfied" (ObjectService SetFlag (serviceOf facts domain (Slot 1))) TaskInvocation [(R1, 1)] (flagBase ++ waitWords 2 1 ++ waiter queued) 0 Returns [Result 0, Equals (GuestAddress (control + factsFlagControlPattern facts)) 1, Equals (GuestAddress (t + factsTaskState facts)) queued]
      , Case "set_clear_on_match" (ObjectService SetFlag (serviceOf facts domain (Slot 1))) TaskInvocation [(R1, 1)] (flagBase ++ [(GuestAddress (flag + factsFlagConfigAttributes facts), fromIntegral (factsFlagClearOnMatch facts))] ++ waitWords 1 1 ++ waiter queued) 0 (DispatchesTo resume) [Result 0, Equals (GuestAddress (control + factsFlagControlPattern facts)) 0, Equals (GuestAddress (wait + factsFlagWaitResult facts)) 1]
      , Case "set_waiter_peer" (ObjectService SetFlag (serviceOf facts domain (Slot 1))) TaskInvocation [(R1, 1)] (flagBase ++ waitWords 1 1 ++ waiter queued ++ [(GuestAddress (t + factsTaskDomain facts), peer)]) 0 Returns [Result 0, Nonzero doorbell]
      , Case "set_object_peer" (ObjectService SetFlag (serviceOf facts peerDomain (Slot 1))) TaskInvocation [(R1, 1)] (peerFlagBase ++ [(GuestAddress (peerControl + factsWaitQueueNext facts), t), (GuestAddress (peerWait + factsWaitBlockQueue facts), peerControl), (GuestAddress (peerWait + factsFlagWaitPattern facts), 1), (GuestAddress (peerWait + factsFlagWaitMode facts), 1)] ++ [(GuestAddress (t + factsTaskNext facts), t), (GuestAddress (t + factsTaskPrev facts), t), (GuestAddress (t + factsTaskState facts), queued .|. (3 `shiftL` 8)), (GuestAddress (t + factsTaskDomain facts), own), (GuestAddress (t + factsTaskWait facts), peerWait)]) 0 Returns [Result 0, Nonzero doorbell]
      , Case "set_timer_cancel" (ObjectService SetFlag (serviceOf facts domain (Slot 1))) TaskInvocation [(R1, 1)] (flagBase ++ [(GuestAddress (wait + factsWaitBlockResult facts), timerEntry domain)] ++ waitWords 1 1 ++ waiter queued) 0 (DispatchesTo resume) [Result 0, Equals (GuestAddress (wait + factsWaitBlockResult facts)) 0, Equals (GuestAddress (receiptAddress + 4)) (timerEntry domain)]
      , Case "send_blocked" (ObjectService Send (serviceOf facts domain (Slot 1))) TaskInvocation [(R1, messageAddress)] ([(GuestAddress (mailbox + factsMailboxConfigExists facts), 1), (GuestAddress (mailControl + factsWaitQueueNext facts), t), (GuestAddress (mailWait + factsWaitBlockQueue facts), mailControl), (GuestAddress messageAddress, sentinel), (GuestAddress (messageAddress + 4), 0)] ++ [(GuestAddress (t + factsTaskNext facts), t), (GuestAddress (t + factsTaskPrev facts), t), (GuestAddress (t + factsTaskState facts), queued .|. (3 `shiftL` 8)), (GuestAddress (t + factsTaskDomain facts), own), (GuestAddress (t + factsTaskWait facts), mailWait)]) 0 (DispatchesTo resume) [Result 0, Equals (GuestAddress (mailWait + factsMailboxWaitMessage facts)) messageAddress, Equals (GuestAddress messageAddress) sentinel]
      , Case "send_empty_fifo" (ObjectService Send (serviceOf facts domain (Slot 1))) TaskInvocation [(R1, messageAddress)] [(GuestAddress (mailbox + factsMailboxConfigExists facts), 1), (GuestAddress (mailControl + factsMailboxControlHead facts), 0), (GuestAddress (mailControl + factsMailboxControlTail facts), 0), (GuestAddress (mailControl + factsWaitQueueNext facts), mailControl), (GuestAddress (mailControl + factsWaitQueuePrev facts), mailControl), (GuestAddress messageAddress, 0), (GuestAddress (messageAddress + 4), 0)] 0 Returns [Result 0, Equals (GuestAddress (mailControl + factsMailboxControlHead facts)) messageAddress, Equals (GuestAddress (mailControl + factsMailboxControlTail facts)) messageAddress, Equals (GuestAddress messageAddress) 0]
      , Case "send_priority_equal" (ObjectService Send (serviceOf facts domain (Slot 1))) TaskInvocation [(R1, messageAddress)] [(GuestAddress (mailbox + factsMailboxConfigExists facts), 1), (GuestAddress (mailbox + factsMailboxConfigAttributes facts), fromIntegral (factsMailboxMessagePriority facts)), (GuestAddress (mailControl + factsMailboxControlHead facts), queuedMessage), (GuestAddress (mailControl + factsMailboxControlTail facts), queuedMessage), (GuestAddress (mailControl + factsWaitQueueNext facts), mailControl), (GuestAddress queuedMessage, 0), (GuestAddress (queuedMessage + 4), 5), (GuestAddress messageAddress, 0), (GuestAddress (messageAddress + 4), 5)] 0 Returns [Result 0, Equals (GuestAddress queuedMessage) messageAddress, Equals (GuestAddress messageAddress) 0]
      , Case "send_waiter_peer" (ObjectService Send (serviceOf facts domain (Slot 1))) TaskInvocation [(R1, messageAddress)] ([(GuestAddress (mailbox + factsMailboxConfigExists facts), 1), (GuestAddress (mailControl + factsWaitQueueNext facts), t), (GuestAddress (mailWait + factsWaitBlockQueue facts), mailControl), (GuestAddress messageAddress, 0)] ++ [(GuestAddress (t + factsTaskNext facts), t), (GuestAddress (t + factsTaskPrev facts), t), (GuestAddress (t + factsTaskState facts), queued .|. (3 `shiftL` 8)), (GuestAddress (t + factsTaskDomain facts), peer), (GuestAddress (t + factsTaskWait facts), mailWait)]) 0 Returns [Result 0, Nonzero doorbell]
      , Case "release_plain_insert" (OwnerRelease (Owner (GuestAddress (record facts domain (dfId w)))) TaskInvocation) [(R1, t)] ((waiter queued) ++ [(GuestAddress (wait + factsWaitBlockResult facts), 0)]) 0 Returns [Result 1, Equals (GuestAddress (t + factsTaskState facts)) (fromIntegral (factsStateReady facts) .|. (3 `shiftL` 8))]
      , Case "release_terminate_reset" (OwnerRelease (Owner (GuestAddress (record facts domain (dfId w)))) TaskInvocation) [(R1, t)] ((waiter queued) ++ [(GuestAddress (t + factsTaskTerminatePending facts), 1)]) 0 Returns [Result 0, Equals (GuestAddress (t + factsTaskContextDiscarded facts)) 1]
      , Case "release_terminate_prepare" (OwnerRelease (Owner (GuestAddress (record facts domain (dfId w)))) TaskInvocation) [(R1, t)] ((waiter queued) ++ [(GuestAddress (t + factsTaskTerminatePending facts), 1), (GuestAddress (t + factsTaskState facts), queued .|. (3 `shiftL` 8) .|. (1 `shiftL` 16))]) 0 Returns [Result 1, Equals (GuestAddress (t + factsTaskState facts)) (fromIntegral (factsStateReady facts) .|. (3 `shiftL` 8))]
      , Case "release_priority" (OwnerRelease (Owner (GuestAddress (record facts domain (dfId w)))) TaskInvocation) [(R1, t)] ((waiter queued) ++ [(GuestAddress (t + factsTaskPriorityChangePending facts), 1), (GuestAddress (t + factsTaskPendingPriority facts), 7)]) 0 Returns [Result 1, Equals (GuestAddress (t + factsTaskState facts)) (fromIntegral (factsStateReady facts) .|. (7 `shiftL` 8))]
      , Case "release_suspended" (OwnerRelease (Owner (GuestAddress (record facts domain (dfId w)))) TaskInvocation) [(R1, t)] ((waiter queued) ++ [(GuestAddress (t + factsTaskState facts), fromIntegral (factsStateSuspended facts) .|. (3 `shiftL` 8))]) 0 Returns [Result 0, Equals (GuestAddress (t + factsTaskState facts)) (fromIntegral (factsStateSuspended facts) .|. (3 `shiftL` 8))]
      , Case "release_timer" (OwnerRelease (Owner (GuestAddress (record facts domain (dfId w)))) TaskInvocation) [(R1, t)] ((waiter queued) ++ [(GuestAddress (wait + factsWaitBlockResult facts), timerEntry domain)]) 0 Returns [Result 1, Equals (GuestAddress (wait + factsWaitBlockResult facts)) 0, Equals (GuestAddress (receiptAddress + 4)) (timerEntry domain)]
      , Case "release_owner_peer" (OwnerRelease (Owner (GuestAddress (record facts domain peer)))) TaskInvocation [(R1, t)] (waiter queued) 0 Returns [Result 1, Equals (GuestAddress (dfSelected (domainFacts facts peer))) t]
      , Case "release_plain_insert_handler" (OwnerRelease (Owner (GuestAddress (record facts domain (dfId w))))) HandlerInvocation [(R1, t)] ((waiter queued) ++ [(GuestAddress (wait + factsWaitBlockResult facts), 0)]) 0 Returns [Result 1, Equals (GuestAddress (t + factsTaskState facts)) (fromIntegral (factsStateReady facts) .|. (3 `shiftL` 8))]
      , Case "release_timer_handler" (OwnerRelease (Owner (GuestAddress (record facts domain (dfId w))))) HandlerInvocation [(R1, t)] ((waiter queued) ++ [(GuestAddress (wait + factsWaitBlockResult facts), timerEntry domain)]) 0 Returns [Result 1, Equals (GuestAddress (wait + factsWaitBlockResult facts)) 0, Equals (GuestAddress (receiptAddress + 4)) (timerEntry domain)]
      ]
  where
    queuedMessage = messageAddress + 8

messageAddress :: Word32
messageAddress = 0x40000000

sentinel :: Word32
sentinel = 0x5e4d1e00

overlay :: Word32 -> B.ByteString -> [(GuestAddress, Word32)] -> B.ByteString
overlay base = foldl apply
  where
    apply bytes (GuestAddress address, value) =
      let at = fromIntegral (address - base)
          encoded = word32le value
       in B.take at bytes <> encoded <> B.drop (at + B.length encoded) bytes

background :: Int -> B.ByteString
background size = B.replicate size 0

data Page = Page
  { pageId :: RegionId
  , pageBase :: Word32
  , pageSize :: Int
  , pagePermission :: Permission
  , pagePokes :: [(GuestAddress, Word32)]
  }

casePages :: Facts -> Domain -> Case -> [Page]
casePages facts domain one =
  regions
  where
    w = domainFacts facts domain
    headAt q = dfReadyHeads w + q * factsHeadSize facts
    blockLow = minimum [dfSchedulerLock w, dfObjectLock w, dfContextA w, dfContextB w, dfReadyHeads w, dfReadyBitmap w, dfRunning w, dfSelected w, dfDispatchEnabled w]
    blockHigh = maximum [dfSchedulerLock w + 4, dfObjectLock w + 4, dfContextA w + factsContextSize facts, dfContextB w + factsContextSize facts, headAt (factsPriorities facts - 1) + factsHeadSize facts, dfReadyBitmap w + 4, dfRunning w + 4, dfSelected w + 4, dfDispatchEnabled w + 4]
    words' = layoutPokes facts domain ++ caseWords one
    regions =
      [ Page (RegionId "block") blockLow (fromIntegral (blockHigh - blockLow)) ReadWrite words'
      , Page (RegionId "tasks") (dfTaskTable w) (120 * fromIntegral (factsTaskSize facts)) ReadWrite words'
      , Page (RegionId "definitions") definitionBase (240 * fromIntegral definitionStride) ReadWrite words'
      , Page (RegionId "flags") (dfFlagConfigs w) (120 * fromIntegral (factsFlagConfigSize facts)) ReadWrite words'
      , Page (RegionId "flag-controls") (dfFlagControls w) (120 * fromIntegral (factsFlagControlSize facts)) ReadWrite words'
      , Page (RegionId "flag-waits") (dfFlagWaits w) (120 * fromIntegral (factsFlagWaitSize facts)) ReadWrite words'
      , Page (RegionId "flag-timers") (dfFlagTimers w) (120 * fromIntegral (factsTimerEntrySize facts)) ReadWrite words'
      , Page (RegionId "mailbox-configs") (dfMailboxConfigs w) (fromIntegral (factsMailboxConfigSize facts)) ReadWrite words'
      , Page (RegionId "mailbox-controls") (dfMailboxControls w) (fromIntegral (factsMailboxControlSize facts)) ReadWrite words'
      , Page (RegionId "mailbox-waits") (dfMailboxWaits w) (120 * fromIntegral (factsMailboxWaitSize facts)) ReadWrite words'
      , Page (RegionId "mailbox-timers") (dfMailboxTimers w) (120 * fromIntegral (factsTimerEntrySize facts)) ReadWrite words'
      , Page (RegionId "stack") stackBase (fromIntegral (stackPointer - stackBase)) ReadWrite []
      , Page (RegionId "caller-frame") stackPointer (fromIntegral (callerTop - stackPointer)) ReadWrite []
      , Page (RegionId "exit") exitAddress 4 ReadExecute []
      , Page (RegionId "receipt") receiptAddress 8 ReadWrite []
      , Page (RegionId "doorbell") 0xfffb4000 0x1000 Device []
      , Page (RegionId "doorbell-1") 0xfffb3000 0x1000 Device []
      ]

dataRegions :: Facts -> Domain -> Case -> [CaseRegion]
dataRegions facts domain one = map build (casePages facts domain one)

build :: Page -> CaseRegion
build region =
  mkRegion (pageId region) (GuestAddress (fromIntegral (pageBase region))) (pageSize region) (pagePermission region) (overlay (pageBase region) (background (pageSize region)) [(address, value) | (address, value) <- pagePokes region, inside address])

inside :: GuestAddress -> Bool
inside (GuestAddress address) = True

timerStub :: B.ByteString
timerStub = assemble [movw 2 (receiptAddress .&. 0xffff), movt 2 (receiptAddress `shiftR` 16), 0xe5820000, 0xe5821004, 0xe3a00000, 0xe12fff1e]

movw :: Word32 -> Word32 -> Word32
movw rd imm = 0xe3000000 .|. (rd `shiftL` 12) .|. ((imm .&. 0xf000) `shiftL` 4) .|. (imm .&. 0xfff)

movt :: Word32 -> Word32 -> Word32
movt rd imm = 0xe3400000 .|. (rd `shiftL` 12) .|. ((imm .&. 0xf000) `shiftL` 4) .|. (imm .&. 0xfff)

caseFor :: Facts -> Domain -> Image -> Case -> CaseSpec
caseFor facts domain image one =
  CaseSpec
    { caseId = caseName one ++ "-" ++ show domain
    , caseProvenance = "X-M1 1.50 loader " ++ show domain ++ " " ++ show (caseCall one) ++ " over layout.json and the rtos_layout stock data layout"
    , caseExclusion = ""
    , caseEntry = GuestAddress (fromIntegral (entryFor (caseCall one)))
    , caseExit = GuestAddress (fromIntegral exitAddress)
    , caseRegions =
        [mkRegion (RegionId ("code-" ++ show base)) (GuestAddress (fromIntegral base)) (fromIntegral size) ReadExecute (slice base size) | (base, size) <- extents domain]
          ++ [ mkRegion (RegionId "code-timer") (GuestAddress (fromIntegral (timerEntry domain))) (B.length timerStub) ReadExecute timerStub
             , mkRegion (RegionId "code-raise") (GuestAddress (fromIntegral (raiseIrqEntry domain))) 8 ReadExecute (assemble [0xe3a00000, 0xe12fff1e])
             ]
          ++ dataRegions facts domain one
    , caseFunctions = [(GuestAddress (fromIntegral base), size) | (base, size) <- extents domain]
    , caseInitial = [(R0, entryWord (caseCall one))] ++ caseRegisters one ++ [(Sp, sp), (Lr, exitAddress), (Cpsr, cpsr)]
    , caseExpectedStatus = Returned
    , caseExpectedRegisters = []
    , caseExpectedMemory = []
    , caseCp14Identity = Nothing
    , caseKernel = Just (KernelCase plainLimit stop Nothing Nothing Nothing [])
    }
  where
    slice base size = B.take (fromIntegral size) (B.drop (fromIntegral (base - imageBase image)) (imageBytes image))
    stop = case caseExitKind one of
      Returns -> Nothing
      DispatchesTo address -> Just address
    entryFor (ObjectService family _) = entryAddress domain family
    entryFor (OwnerRelease _) = releaseEntry domain
    entryWord (ObjectService _ id) = serviceWord id
    entryWord (OwnerRelease (Owner record)) = guestWord record
    sp = case (caseCall one, caseInvocation one) of
      (OwnerRelease _, HandlerInvocation) -> handlerStackPointer
      _ -> stackPointer
    cpsr = case (caseCall one, caseInvocation one) of
      (OwnerRelease _, HandlerInvocation) -> 0x92
      (OwnerRelease _, TaskInvocation) -> 0x93
      _ -> 0x13

compareFamily :: Facts -> Domain -> Case -> NativeObservation -> NativeObservation -> [String]
compareFamily facts domain one stock owned =
  requireExit ++ requireResult ++ requireExpectations ++ requireReceipt ++ compareRegisters ++ compareCpsr ++ compareMemory
  where
    bases = regionBases facts domain one
    spec = caseFor facts domain (Image 0 B.empty) one
    requireExit =
      case caseExitKind one of
        Returns ->
          [ "stock did not return: " ++ show (caseOutcome spec stock) | caseOutcome spec stock /= OutcomeReturned]
            ++ [ "owned did not return: " ++ show (caseOutcome spec owned) | caseOutcome spec owned /= OutcomeReturned]
        DispatchesTo address ->
          [ "stock exit " ++ show (caseOutcome spec stock) ++ " expected the stop at " ++ show address | caseOutcome spec stock /= OutcomeStopped address]
            ++ [ "owned exit " ++ show (caseOutcome spec owned) ++ " expected the stop at " ++ show address | caseOutcome spec owned /= OutcomeStopped address]
    requireResult =
      [ "owned r0 is " ++ show (registerValue owned R0) ++ " expected " ++ show expected | Result expected <- caseExpect one, registerValue owned R0 /= expected]
    requireExpectations =
      [ "owned " ++ show expectation ++ " not observed" | expectation <- caseExpect one, not (holdsOne expectation owned)]
    requireReceipt =
      [ "timer receipt differs: " ++ show (receipt stock) ++ " against " ++ show (receipt owned) | receipt stock /= receipt owned]
    holdsOne (Equals address expected) observation = observedWord bases address observation == Just expected
    holdsOne (Nonzero address) observation = maybe False (/= 0) (observedWord bases address observation)
    holdsOne (Result _) _ = True
    receipt observation = (observedWord bases (GuestAddress receiptAddress) observation, observedWord bases (GuestAddress (receiptAddress + 4)) observation)
    compareRegisters =
      [ "r" ++ show r ++ " is " ++ show (M.lookup r (nativeRegisters owned)) ++ " expected " ++ show (M.lookup r (nativeRegisters stock))
      | r <- [R4, R5, R6, R7, R8, R9, R10, R11, Sp]
      , M.lookup r (nativeRegisters owned) /= M.lookup r (nativeRegisters stock)
      ]
    compareCpsr =
      [ "cpsr " ++ show (control owned) ++ " expected " ++ show (control stock) | control owned /= control stock ]
    control observation = M.findWithDefault 0 Cpsr (nativeRegisters observation) .&. 0xdf
    compareMemory =
      [ "region " ++ regionName ident ++ " is missing from the owned observation" | (ident, _) <- nativeMemory stock, ident `notElem` map fst (nativeMemory owned)]
        ++ [ "region " ++ regionName ident ++ " is extra in the owned observation" | (ident, _) <- nativeMemory owned, ident `notElem` map fst (nativeMemory stock)]
        ++ [ "region " ++ regionName ident ++ " differs" | (ident, expected) <- nativeMemory stock, Just actual <- [lookup ident (nativeMemory owned)], actual /= expected]

regionBases :: Facts -> Domain -> Case -> [(String, Word32)]
regionBases facts domain one =
  [ (regionName (pageId region), pageBase region) | region <- casePages facts domain one ]

observedWord :: [(String, Word32)] -> GuestAddress -> NativeObservation -> Maybe Word32
observedWord bases (GuestAddress address) observation =
  case [wordAt bytes (address - base) | (ident, base) <- bases, base <= address, (ident', bytes) <- nativeMemory observation, regionName ident' == ident, address < base + fromIntegral (B.length bytes)] of
    word : _ -> Just word
    [] -> Nothing

wordAt :: B.ByteString -> Word32 -> Word32
wordAt bytes offset =
  let at = fromIntegral offset
   in B.foldl' (\value byte -> value `shiftL` 8 .|. fromIntegral byte) 0 (B.take 4 (B.drop at bytes))

data FamilyRun = FamilyRun
  { familyDomain :: Domain
  , familyCases :: [(String, Either String [String])]
  , familyMatches :: Int
  , familyReuse :: (BatchReuse, BatchReuse)
  }

runFamily :: FilePath -> FilePath -> FilePath -> Facts -> Domain -> Image -> Image -> IO FamilyRun
runFamily runner cacheRoot directory facts domain stock owned = do
  let stockCases = [(caseName one, caseFor facts domain stock one) | one <- cases facts domain]
      ownedCases = [(caseName one, caseFor facts domain owned one) | one <- cases facts domain]
      byName = M.fromList [(caseName one, one) | one <- cases facts domain]
  stockBatch <- writeBatch (directory </> "stock") (map snd stockCases)
  ownedBatch <- writeBatch (directory </> "owned") (map snd ownedCases)
  (stockReuse, stockRaw) <- cachedObservedBatch cacheRoot runner stockBatch
  (ownedReuse, ownedRaw) <- cachedObservedBatch cacheRoot runner ownedBatch
  let stockObserved = M.fromList [(caseIdText ident, observation) | (ident, observation) <- stockRaw]
      ownedObserved = M.fromList [(caseIdText ident, observation) | (ident, observation) <- ownedRaw]
  results <- forM stockCases $ \(label, _) ->
    pure
      ( label
      , case (M.lookup label stockObserved, M.lookup label ownedObserved) of
          (Just s, Just o) -> Right (compareFamily facts domain (byName M.! label) (stripScratch s) (stripScratch o))
          (Nothing, _) -> Left "stock case did not run"
          (_, Nothing) -> Left "owned case did not run"
      )
  pure
    FamilyRun
      { familyDomain = domain
      , familyCases = results
      , familyMatches = length [() | (_, Right []) <- results]
      , familyReuse = (stockReuse, ownedReuse)
      }
