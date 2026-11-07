{-# LANGUAGE OverloadedStrings #-}

-- | The F2 family-native cases over both entry domains, each run against the stock loader
-- bytes and the owned image and compared observation by observation.
--
-- Regions are consolidated to the admission limits: one code envelope, the consolidated
-- object tables, the block, tasks, definitions, messages, stacks, banks and lock words.
-- The retained stock helpers come from the stock loader bytes in both roles.
module Replacement.RtosObjects.Suite
  ( Family (..)
  , ServiceId (..)
  , Slot (..)
  , Call (..)
  , Invocation (..)
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
import Data.Word (Word32, Word8)
import Emulation.CachedNativeBatch (BatchReuse (..), cachedObservedBatch)
import Emulation.NativeCase (CaseId (..), caseIdText)
import Emulation.NativeCase (Region (..))
import Emulation.NativeObservation (GuestAddress (..), NativeObservation (..), RegionId (..), Register (..), RunnerStatus (..), regionIdOf)
import Emulation.OwnedCase (CaseOutcome (..), CaseRegion (..), CaseSpec (..), KernelCase (..), Permission (..), caseOutcome, mkRegion, plainLimit, readWordAt, writeBatch)
import Replacement.RtosObjects.Layout
import Replacement.RtosOwnFirst.Regions (Domain (..))
import Replacement.RtosScheduler.Oracle (stripScratch)
import System.FilePath ((</>))

data Family = SetFlag | Send
  deriving (Eq, Show)

newtype ServiceId = ServiceId { serviceWord :: Word32 }
  deriving (Eq, Show)

newtype Slot = Slot { slotWord :: Word32 }
  deriving (Eq, Show)

-- | An object service with its id, or the owner release with its record.
data Call
  = ObjectService Family ServiceId
  | OwnerRelease GuestAddress
  deriving (Eq, Show)

-- | The caller's boundary: a task entry or the recovered handler callback.
data Invocation = TaskInvocation | HandlerInvocation
  deriving (Eq, Show)

data Exit
  = Returns
  | DispatchesTo GuestAddress
  deriving (Eq, Show)

data Expect
  = Equals GuestAddress Word32
  | ByteEquals GuestAddress Word8
  | Nonzero GuestAddress
  | Result Word32
  deriving (Eq, Show)

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

data Image = Image
  { imageBase :: Word32
  , imageBytes :: B.ByteString
  }

-- Synthetic addresses shared with the scheduler oracle and the fixture.
definitionBase, definitionStride, stackBase, stackPointer, handlerStackBase, handlerStackPointer, callerTop, exitAddress, receiptAddress, stopAddress, lockWordBase :: Word32
definitionBase = 0x0096c000
definitionStride = 0x20
stackBase = 0x00959700
stackPointer = 0x00959800
handlerStackBase = 0x00959900
handlerStackPointer = 0x00959a00
callerTop = 0x00959810
exitAddress = 0x00958f00
receiptAddress = 0x00960000
stopAddress = 0x00959200
lockWordBase = 0x00962000

waiterSlot, runningSlot :: Slot
waiterSlot = Slot 2
runningSlot = Slot 1

-- Retained stock helpers, from the stock loader in both roles.
flagCheck, fpDisable, timerEntry, raiseIrq :: Domain -> Word32
flagCheck DomainA = 0x480cc
flagCheck DomainB = 0xd808c
fpDisable DomainA = 0x4252c
fpDisable DomainB = 0xd252c
timerEntry DomainA = 0x44ca8
timerEntry DomainB = 0xd4c68
raiseIrq DomainA = 0x433e4
raiseIrq DomainB = 0xd33e4

releaseEntry :: Domain -> Word32
releaseEntry DomainA = 0x44568
releaseEntry DomainB = 0xd4528

entryAddress :: Domain -> Family -> Word32
entryAddress DomainA SetFlag = 0x4813c
entryAddress DomainA Send = 0x4aa44
entryAddress DomainB SetFlag = 0xd80fc
entryAddress DomainB Send = 0xdaa04

-- | The owned extents of S1, S2, the wait release and the two F2 bodies.
extents :: Domain -> [(Word32, Word32)]
extents DomainA =
  [ (0x40b00, 0x24), (0x43640, 0x28), (0x43668, 0x20), (0x43688, 0xa0), (0x43728, 0x34), (0x4375c, 0x48), (0x437a4, 0x38), (0x437fc, 0x14)
  , (0x439d8, 0x3c), (0x43a14, 0x28), (0x43c48, 0x5c), (0x43ca4, 0x7c), (0x43d20, 0xb0), (0x43dd0, 0x48), (0x43f70, 0x2c), (0x44018, 0x2c), (0x44044, 0xf0), (0x44134, 0x80)
  , (0x44568, 0xcc)
  , (0x4813c, 0x260), (0x4aa44, 0x304)
  ]
extents DomainB =
  [ (0xd0b00, 0x24), (0xd3640, 0x28), (0xd3668, 0x20), (0xd3688, 0xa0), (0xd3728, 0x34), (0xd375c, 0x48), (0xd37a4, 0x38), (0xd37fc, 0x14)
  , (0xd398c, 0x3c), (0xd39c8, 0x28), (0xd3c04, 0x5c), (0xd3c60, 0x7c), (0xd3cdc, 0xb0), (0xd3d8c, 0x48), (0xd3f2c, 0x2c), (0xd3fd4, 0x30), (0xd4004, 0xf0), (0xd40f4, 0x80)
  , (0xd4528, 0xcc)
  , (0xd80fc, 0x260), (0xdaa04, 0x304)
  ]

-- | The stock extents of the retained helpers, sliced from the stock loader in both roles.
retained :: Domain -> [(Word32, Word32)]
retained DomainA = [(0x480cc, 0x40), (0x4252c, 0x20), (0x44ca8, 0x80), (0x433e4, 0x60)]
retained DomainB = [(0xd808c, 0x40), (0xd252c, 0x20), (0xd4c68, 0x80), (0xd33e4, 0x60)]

loaderBase :: Domain -> Word32
loaderBase DomainA = 0x40000
loaderBase DomainB = 0xd0000

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

serviceOf :: Facts -> Domain -> Slot -> ServiceId
serviceOf facts domain slot = ServiceId ((dfId (domainFacts facts domain) `shiftL` 16) .|. slotWord slot)

-- | The 20 cases per domain from the harness review contract table.
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
      running = task facts domain (slotWord runningSlot)
      dispatch = dfDispatchEnabled w
      peerFlag = flagConfig facts peerDomain 1
      peerControl = flagControl facts peerDomain 1
      peerWait = flagWait facts peerDomain (slotWord waiterSlot)
      peerTask = task facts peerDomain (slotWord waiterSlot)
      flagBase =
        [ (GuestAddress (flag + factsFlagConfigExists facts), 1)
        , (GuestAddress (control + factsWaitQueueNext facts), control)
        , (GuestAddress (control + factsWaitQueuePrev facts), control)
        , (GuestAddress (control + factsFlagControlPattern facts), 0)
        ]
      runningWords =
        [ (GuestAddress (running + factsTaskState facts), fromIntegral (factsStateReady facts))
        , (GuestAddress (running + factsTaskPriority facts), 15)
        , (GuestAddress (running + factsTaskDomain facts), own)
        , (GuestAddress (dfReadyBitmap w), 0x8000)
        , (GuestAddress (dfSelected w), running)
        ]
      waiter state taskAddr waitAddr =
        [ (GuestAddress (taskAddr + factsTaskNext facts), control)
        , (GuestAddress (taskAddr + factsTaskPrev facts), control)
        , (GuestAddress (taskAddr + factsTaskState facts), state .|. (3 `shiftL` 8))
        , (GuestAddress (taskAddr + factsTaskDomain facts), own)
        , (GuestAddress (taskAddr + factsTaskWait facts), waitAddr)
        ]
      queued = fromIntegral (factsStateWait facts) + fromIntegral (factsStateObjectQueued facts)
      heldLocks record =
        let idx = if record == recordAddress facts domain DomainA then 0 else 1
         in [ (GuestAddress (lockWordBase + 8 * idx), 1), (GuestAddress (lockWordBase + 8 * idx + 4), 1) ]
      message = 0x40000000
   in [ Case "set_no_waiter" (ObjectService SetFlag (serviceOf facts domain (Slot 1))) TaskInvocation [(R1, 1)] (flagBase ++ runningWords) 0 Returns [Result 0, Equals (GuestAddress (control + factsFlagControlPattern facts)) 1]
      , Case "set_satisfied" (ObjectService SetFlag (serviceOf facts domain (Slot 1))) TaskInvocation [(R1, 1)] (flagBase ++ [(GuestAddress (control + factsWaitQueueNext facts), t)] ++ waiter queued t wait ++ runningWords) 0 (DispatchesTo (GuestAddress stopAddress)) [Equals (GuestAddress (wait + factsFlagWaitResult facts)) 1, ByteEquals (GuestAddress (t + factsTaskState facts)) (factsStateReady facts)]
      , Case "set_unsatisfied" (ObjectService SetFlag (serviceOf facts domain (Slot 1))) TaskInvocation [(R1, 1)] (flagBase ++ [(GuestAddress (control + factsWaitQueueNext facts), t)] ++ waiter queued t wait ++ runningWords) 0 Returns [Result 0, Equals (GuestAddress (control + factsFlagControlPattern facts)) 1, ByteEquals (GuestAddress (t + factsTaskState facts)) (fromIntegral (factsStateWait facts) .|. fromIntegral (factsStateObjectQueued facts))]
      , Case "set_clear_on_match" (ObjectService SetFlag (serviceOf facts domain (Slot 1))) TaskInvocation [(R1, 1)] (flagBase ++ [(GuestAddress (flag + factsFlagConfigAttributes facts), fromIntegral (factsFlagClearOnMatch facts)), (GuestAddress (control + factsWaitQueueNext facts), t)] ++ waiter queued t wait ++ runningWords) 0 (DispatchesTo (GuestAddress stopAddress)) [Equals (GuestAddress (control + factsFlagControlPattern facts)) 0, Equals (GuestAddress (wait + factsFlagWaitResult facts)) 1]
      , Case "set_waiter_peer" (ObjectService SetFlag (serviceOf facts domain (Slot 1))) TaskInvocation [(R1, 1)] (flagBase ++ [(GuestAddress (control + factsWaitQueueNext facts), peerTask), (GuestAddress (peerWait + factsWaitBlockQueue facts), control)] ++ [(GuestAddress (peerTask + factsTaskNext facts), control), (GuestAddress (peerTask + factsTaskPrev facts), control), (GuestAddress (peerTask + factsTaskState facts), queued .|. (3 `shiftL` 8)), (GuestAddress (peerTask + factsTaskDomain facts), peer), (GuestAddress (peerTask + factsTaskWait facts), peerWait)] ++ runningWords) 0 Returns [Result 0, Nonzero (GuestAddress 0xfffb40cc)]
      , Case "set_object_peer" (ObjectService SetFlag (serviceOf facts peerDomain (Slot 1))) TaskInvocation [(R1, 1)] ([(GuestAddress (peerFlag + factsFlagConfigExists facts), 1), (GuestAddress (peerControl + factsWaitQueueNext facts), t), (GuestAddress (peerControl + factsWaitQueuePrev facts), peerControl), (GuestAddress (peerControl + factsFlagControlPattern facts), 0)] ++ waiter queued t wait ++ runningWords) 0 (DispatchesTo (GuestAddress stopAddress)) [Equals (GuestAddress (peerControl + factsFlagControlPattern facts)) 1, Equals (GuestAddress (dfSelected w)) t]
      , Case "set_timer_cancel" (ObjectService SetFlag (serviceOf facts domain (Slot 1))) TaskInvocation [(R1, 1)] (flagBase ++ [(GuestAddress (control + factsWaitQueueNext facts), t), (GuestAddress (wait + factsWaitBlockResult facts), timerEntry domain)] ++ waiter queued t wait ++ runningWords) 0 (DispatchesTo (GuestAddress stopAddress)) [Equals (GuestAddress (wait + factsWaitBlockResult facts)) 0, Equals (GuestAddress (receiptAddress + 4)) (timerEntry domain)]
      , Case "send_blocked" (ObjectService Send (serviceOf facts domain (Slot 1))) TaskInvocation [(R1, message)] ([(GuestAddress (mailbox + factsMailboxConfigExists facts), 1), (GuestAddress (mailControl + factsWaitQueueNext facts), t), (GuestAddress (mailWait + factsWaitBlockQueue facts), mailControl), (GuestAddress message, 0x5e4d1e00)] ++ [(GuestAddress (t + factsTaskNext facts), mailControl), (GuestAddress (t + factsTaskPrev facts), mailControl), (GuestAddress (t + factsTaskState facts), queued .|. (3 `shiftL` 8)), (GuestAddress (t + factsTaskDomain facts), own), (GuestAddress (t + factsTaskWait facts), mailWait)] ++ runningWords) 0 (DispatchesTo (GuestAddress stopAddress)) [Equals (GuestAddress (mailWait + factsMailboxWaitMessage facts)) message, Equals (GuestAddress message) 0x5e4d1e00]
      , Case "send_empty_fifo" (ObjectService Send (serviceOf facts domain (Slot 1))) TaskInvocation [(R1, message)] [(GuestAddress (mailbox + factsMailboxConfigExists facts), 1), (GuestAddress (mailControl + factsMailboxControlHead facts), 0), (GuestAddress (mailControl + factsMailboxControlTail facts), 0), (GuestAddress (mailControl + factsWaitQueueNext facts), mailControl), (GuestAddress (mailControl + factsWaitQueuePrev facts), mailControl), (GuestAddress message, 0)] 0 Returns [Result 0, Equals (GuestAddress (mailControl + factsMailboxControlHead facts)) message, Equals (GuestAddress (mailControl + factsMailboxControlTail facts)) message, Equals (GuestAddress message) 0]
      , Case "send_priority_equal" (ObjectService Send (serviceOf facts domain (Slot 1))) TaskInvocation [(R1, message)] [(GuestAddress (mailbox + factsMailboxConfigExists facts), 1), (GuestAddress (mailbox + factsMailboxConfigAttributes facts), fromIntegral (factsMailboxMessagePriority facts)), (GuestAddress (mailbox + factsMailboxConfigPriorityLimit facts), 32), (GuestAddress (mailControl + factsMailboxControlHead facts), 0x40000008), (GuestAddress (mailControl + factsMailboxControlTail facts), 0x40000008), (GuestAddress (mailControl + factsWaitQueueNext facts), mailControl), (GuestAddress 0x40000008, 0), (GuestAddress 0x4000000c, 5), (GuestAddress message, 0), (GuestAddress (message + 4), 5)] 0 Returns [Result 0, Equals (GuestAddress 0x40000008) message, Equals (GuestAddress message) 0]
      , Case "send_waiter_peer" (ObjectService Send (serviceOf facts domain (Slot 1))) TaskInvocation [(R1, message)] ([(GuestAddress (mailbox + factsMailboxConfigExists facts), 1), (GuestAddress (mailControl + factsWaitQueueNext facts), peerTask), (GuestAddress (peerWait + factsWaitBlockQueue facts), mailControl), (GuestAddress message, 0)] ++ [(GuestAddress (peerTask + factsTaskNext facts), mailControl), (GuestAddress (peerTask + factsTaskPrev facts), mailControl), (GuestAddress (peerTask + factsTaskState facts), queued .|. (3 `shiftL` 8)), (GuestAddress (peerTask + factsTaskDomain facts), peer), (GuestAddress (peerTask + factsTaskWait facts), peerWait)] ++ runningWords) 0 Returns [Result 0, Nonzero (GuestAddress 0xfffb40cc), Equals (GuestAddress (peerWait + factsMailboxWaitMessage facts)) message]
      , Case "release_plain_insert" (OwnerRelease (GuestAddress (recordAddress facts domain (dfId w)))) TaskInvocation [(R1, t)] (heldLocks (recordAddress facts domain (dfId w)) ++ [(GuestAddress (wait + factsWaitBlockResult facts), 0)] ++ waiter queued t wait ++ runningWords) 0 Returns [Result 1, ByteEquals (GuestAddress (t + factsTaskState facts)) (factsStateReady facts)]
      , Case "release_terminate_reset" (OwnerRelease (GuestAddress (recordAddress facts domain (dfId w)))) TaskInvocation [(R1, t)] (heldLocks (recordAddress facts domain (dfId w)) ++ [(GuestAddress (t + factsTaskTerminatePending facts), 1)] ++ waiter queued t wait ++ runningWords) 0 Returns [Result 0, Equals (GuestAddress (t + factsTaskContextDiscarded facts)) 1]
      , Case "release_terminate_prepare" (OwnerRelease (GuestAddress (recordAddress facts domain (dfId w)))) TaskInvocation [(R1, t)] (heldLocks (recordAddress facts domain (dfId w)) ++ [(GuestAddress (t + factsTaskTerminatePending facts), 1), (GuestAddress (t + factsTaskState facts), queued .|. (2 `shiftL` 8) .|. (1 `shiftL` 16))] ++ waiter queued t wait ++ runningWords) 0 Returns [Result 1, ByteEquals (GuestAddress (t + factsTaskState facts)) (factsStateReady facts), ByteEquals (GuestAddress (t + factsTaskFlags facts)) 0]
      , Case "release_priority" (OwnerRelease (GuestAddress (recordAddress facts domain (dfId w)))) TaskInvocation [(R1, t)] (heldLocks (recordAddress facts domain (dfId w)) ++ [(GuestAddress (t + factsTaskPriorityChangePending facts), 1), (GuestAddress (t + factsTaskPendingPriority facts), 7)] ++ waiter queued t wait ++ runningWords) 0 Returns [Result 1, ByteEquals (GuestAddress (t + factsTaskPriority facts)) 7]
      , Case "release_suspended" (OwnerRelease (GuestAddress (recordAddress facts domain (dfId w)))) TaskInvocation [(R1, t)] (heldLocks (recordAddress facts domain (dfId w)) ++ [(GuestAddress (t + factsTaskState facts), fromIntegral (factsStateSuspended facts) .|. (3 `shiftL` 8))] ++ waiter queued t wait ++ runningWords) 0 Returns [Result 0, ByteEquals (GuestAddress (t + factsTaskState facts)) (factsStateSuspended facts)]
      , Case "release_timer" (OwnerRelease (GuestAddress (recordAddress facts domain (dfId w)))) TaskInvocation [(R1, t)] (heldLocks (recordAddress facts domain (dfId w)) ++ [(GuestAddress (wait + factsWaitBlockResult facts), timerEntry domain)] ++ waiter queued t wait ++ runningWords) 0 Returns [Result 1, Equals (GuestAddress (wait + factsWaitBlockResult facts)) 0, Equals (GuestAddress (receiptAddress + 4)) (timerEntry domain)]
      , Case "release_owner_peer" (OwnerRelease (GuestAddress (recordAddress facts domain peer))) TaskInvocation [(R1, peerTask)] (heldLocks (recordAddress facts domain peer) ++ [(GuestAddress (peerWait + factsWaitBlockResult facts), 0)] ++ [(GuestAddress (peerTask + factsTaskNext facts), peerControl), (GuestAddress (peerTask + factsTaskPrev facts), peerControl), (GuestAddress (peerTask + factsTaskState facts), queued .|. (3 `shiftL` 8)), (GuestAddress (peerTask + factsTaskDomain facts), peer), (GuestAddress (peerTask + factsTaskWait facts), peerWait)]) 0 Returns [Result 1, Equals (GuestAddress (dfSelected (domainFacts facts peer))) peerTask]
      , Case "release_plain_insert_handler" (OwnerRelease (GuestAddress (recordAddress facts domain (dfId w)))) HandlerInvocation [(R1, t)] (heldLocks (recordAddress facts domain (dfId w)) ++ [(GuestAddress (wait + factsWaitBlockResult facts), 0)] ++ waiter queued t wait ++ runningWords) 0 Returns [Result 1, ByteEquals (GuestAddress (t + factsTaskState facts)) (factsStateReady facts)]
      , Case "release_timer_handler" (OwnerRelease (GuestAddress (recordAddress facts domain (dfId w)))) HandlerInvocation [(R1, t)] (heldLocks (recordAddress facts domain (dfId w)) ++ [(GuestAddress (wait + factsWaitBlockResult facts), timerEntry domain)] ++ waiter queued t wait ++ runningWords) 0 Returns [Result 1, Equals (GuestAddress (wait + factsWaitBlockResult facts)) 0, Equals (GuestAddress (receiptAddress + 4)) (timerEntry domain)]
      , Case "set_satisfied_no_dispatch" (ObjectService SetFlag (serviceOf facts domain (Slot 1))) TaskInvocation [(R1, 1)] (flagBase ++ [(GuestAddress (control + factsWaitQueueNext facts), t)] ++ waiter queued t wait ++ runningWords ++ [(GuestAddress (dfDispatchEnabled w), 0)]) 0 Returns [Result 0, Equals (GuestAddress (wait + factsFlagWaitResult facts)) 1, ByteEquals (GuestAddress (t + factsTaskState facts)) (factsStateReady facts)]
      , Case "send_blocked_no_dispatch" (ObjectService Send (serviceOf facts domain (Slot 1))) TaskInvocation [(R1, message)] ([(GuestAddress (mailbox + factsMailboxConfigExists facts), 1), (GuestAddress (mailControl + factsWaitQueueNext facts), t), (GuestAddress (mailWait + factsWaitBlockQueue facts), mailControl), (GuestAddress message, 0x5e4d1e00)] ++ [(GuestAddress (t + factsTaskNext facts), mailControl), (GuestAddress (t + factsTaskPrev facts), mailControl), (GuestAddress (t + factsTaskState facts), queued .|. (3 `shiftL` 8)), (GuestAddress (t + factsTaskDomain facts), own), (GuestAddress (t + factsTaskWait facts), mailWait)] ++ runningWords ++ [(GuestAddress (dfDispatchEnabled w), 0)]) 0 Returns [Result 0, Equals (GuestAddress (mailWait + factsMailboxWaitMessage facts)) message, Equals (GuestAddress message) 0x5e4d1e00]
      ]

-- | Region bytes with word pokes contained by the region.
overlay :: Word32 -> B.ByteString -> [(GuestAddress, Word32)] -> B.ByteString
overlay base bytes pokes = foldl apply bytes [(GuestAddress a, v) | (GuestAddress a, v) <- pokes, a >= base, a + 4 <= base + fromIntegral (B.length bytes)]
  where
    apply current (GuestAddress address, value) =
      let at = fromIntegral (address - base)
          encoded = B.pack [fromIntegral (value `shiftR` shift) | shift <- [0, 8, 16, 24]]
       in B.take at current <> encoded <> B.drop (at + B.length encoded) current

-- | The record, lock, ready and task pokes the fixture plants in every case.
layoutWords :: Facts -> Domain -> [(GuestAddress, Word32)]
layoutWords facts domain =
  concat [recordPokes of' | of' <- [DomainA, DomainB]]
    ++ cellPokes
    ++ taskPokes
  where
    w = domainFacts facts domain
    recordPokes of' =
      let record = recordAddress facts domain of'
          w' = domainFacts facts of'
          idx = if of' == DomainA then 0 else 1
          lockWord offset = lockWordBase + 8 * idx + (if offset == factsContextObjectLock facts then 4 else 0)
          lockRecord offset =
            [ (GuestAddress (record + offset + factsLockWord facts), lockWord offset)
            , (GuestAddress (lockWord offset), 0)
            ]
       in [ (GuestAddress (record + factsContextId facts), if of' == domain then factsOwnId facts else dfId w')
          , (GuestAddress (record + factsContextOwner facts), dfId w')
          , (GuestAddress (record + factsContextReadyHeads facts), dfReadyHeads w')
          , (GuestAddress (record + factsContextReadyBitmap facts), dfReadyBitmap w')
          , (GuestAddress (record + factsContextRunning facts), dfRunning w')
          , (GuestAddress (record + factsContextSelected facts), dfSelected w')
          , (GuestAddress (record + factsContextDispatchEnabled facts), dfDispatchEnabled w')
          , (GuestAddress (record + factsContextFirstEntry facts), trampoline of')
          , (GuestAddress (record + 0x58), 8)
          , (GuestAddress (record + 0x68), 1)
          ]
            ++ lockRecord (factsContextSchedulerLock facts)
            ++ lockRecord (factsContextObjectLock facts)
    cellPokes =
      let w = domainFacts facts domain
          headAt q = dfReadyHeads w + q * factsHeadSize facts
       in concat [[(GuestAddress (headAt q), headAt q), (GuestAddress (headAt q + factsHeadPrev facts), headAt q)] | q <- [0 .. factsPriorities facts - 1]]
            ++ [ (GuestAddress (dfReadyBitmap w), 0)
               , (GuestAddress (dfSelected w), 0)
               , (GuestAddress (dfRunning w), task facts domain (slotWord runningSlot))
               , (GuestAddress (dfDispatchEnabled w), 1)
               , (GuestAddress (dfSchedulerLock w), 0)
               , (GuestAddress (dfObjectLock w), 0)
               ]
    taskPokes =
      concat
        [ [ (GuestAddress (taskAt slot + factsTaskDefinition facts), definitionAddress facts domain slot)
          , (GuestAddress (taskAt slot + factsTaskDomain facts), dfId (domainFacts facts domain))
          , (GuestAddress (taskAt slot + factsTaskState facts), fromIntegral (factsStateDormant facts) .|. ((slotWord slot `mod` factsPriorities facts) `shiftL` 8))
          , (GuestAddress (definitionAddress facts domain slot + factsDefinitionPriority facts), slotWord slot `mod` factsPriorities facts)
          , (GuestAddress (definitionAddress facts domain slot + 0x10), 0x00950000 + slotWord slot * 0x100)
          , (GuestAddress (definitionAddress facts domain slot + 0x14), 0x100)
          ]
        | slot <- map Slot [1 .. 120]
        ]
    taskAt slot = task facts domain (slotWord slot)

-- | The 4 KiB page containing an address.
pageOf :: Word32 -> Word32
pageOf address = address .&. 0xfffff000

-- | The case's spec over one image.
caseFor :: Facts -> Domain -> Image -> Case -> CaseSpec
caseFor facts domain image one =
  CaseSpec
    { caseId = caseName one ++ "-" ++ show domain
    , caseProvenance = "X-M1 1.50 loader " ++ show domain ++ " " ++ show (caseCall one) ++ " over layout.json and the retained stock helpers"
    , caseExclusion = "the F2 subset stops at the waiter dispatch or the service return; receiver return values and F3 handler flag-set are excluded"
    , caseEntry = GuestAddress (fromIntegral (entryFor (caseCall one)))
    , caseExit = GuestAddress (fromIntegral exitAddress)
    , caseRegions = regions
    , caseFunctions = functions
    , caseInitial = seededRegisters
    , caseExpectedStatus = Returned
    , caseExpectedRegisters = expectedRegisters
    , caseExpectedMemory = []
    , caseCp14Identity = Nothing
    , caseKernel = Just (KernelCase plainLimit stop (Just 0) (Just 0) (Just 0) [R4, R5, R6, R7, R8, R9, R10, R11, R12])
    }
  where
    entryFor (ObjectService family _) = entryAddress domain family
    entryFor (OwnerRelease _) = releaseEntry domain
    entryWord (ObjectService _ id) = serviceWord id
    entryWord (OwnerRelease (GuestAddress record)) = record
    stop = case caseExitKind one of
      Returns -> Nothing
      DispatchesTo address -> Just address
    handler = caseInvocation one == HandlerInvocation
    cpsr = if handler then 0x93 else 0x13
    sp = if handler then handlerStackPointer else stackPointer
    seededRegisters =
      [(R0, entryWord (caseCall one))]
        ++ [(r, 0xa5a50000 + fromIntegral (fromEnum r)) | r <- [R1 .. R12], r `notElem` map fst (caseRegisters one)]
        ++ caseRegisters one
        ++ [(Sp, sp), (Lr, exitAddress), (Cpsr, cpsr)]
    expectedRegisters = case caseExitKind one of
      Returns -> [(R0, 0)]
      DispatchesTo _ -> [(R0, 0), (Pc, fromIntegral stopAddress)]
    words' = caseWords one
    regions =
      [ codeRegion
      , dataRegion "block" (pageOf (dfSchedulerLock (domainFacts facts domain))) 0x1000 words'
      , dataRegion "tasks" (dfTaskTable (domainFacts facts domain)) 0x2000 words'
      , dataRegion "definitions" definitionBase 0x2000 words'
      , dataRegion "objects" 0x40055000 0xe000 words'
      , dataRegion "messages" 0x40000000 0x1000 words'
      , dataRegion "locks" lockWordBase 0x100 words'
      , dataRegion "stack" stackBase (fromIntegral (stackPointer - stackBase)) words'
      , dataRegion "caller-frame" stackPointer (fromIntegral (callerTop - stackPointer)) words'
      , dataRegion "handler-stack" handlerStackBase (fromIntegral (handlerStackPointer - handlerStackBase)) words'
      , dataRegion "stop" stopAddress 0x10 []
      ]
    codeRegion =
      mkRegion (RegionId "code") (GuestAddress (fromIntegral (minimum (map fst (extents domain))))) (fromIntegral (maximum [start + size | (start, size) <- extents domain] - minimum (map fst (extents domain)))) ReadExecute (overlay (minimum (map fst (extents domain))) (codeBytes (minimum (map fst (extents domain))) (maximum [start + size | (start, size) <- extents domain] - minimum (map fst (extents domain)))) [])
    codeBytes base size = B.take (fromIntegral size) (B.drop (fromIntegral (base - imageBase image)) (imageBytes image))
    dataRegion ident base size pokes = mkRegion (RegionId ident) (GuestAddress (fromIntegral base)) size ReadWrite (overlay base (B.replicate size 0) (layoutWords facts domain ++ pokes))
    functions =
      [(GuestAddress (fromIntegral start), size) | (start, size) <- extents domain]
        ++ [(GuestAddress (fromIntegral (start)), size) | (start, size) <- retained domain]
        ++ [(GuestAddress (fromIntegral stopAddress), 0x10)]

compareFamily :: Facts -> Domain -> Case -> NativeObservation -> NativeObservation -> [String]
compareFamily facts domain one stock owned =
  requireExit ++ requireResult ++ requireExpectations ++ compareCpsr ++ compareMemory ++ compareOrder
  where
    bases = [ (regionName (regionIdOf (caseRegion region)), fromIntegral (baseAddress (caseRegion region))) | region <- caseRegions (caseFor facts domain (Image 0 B.empty) one) ]
    requireExit =
      case caseExitKind one of
        Returns -> ["stock exit " ++ show (caseOutcome (caseFor facts domain (Image 0 B.empty) one) stock) | caseOutcome (caseFor facts domain (Image 0 B.empty) one) stock /= OutcomeReturned]
        DispatchesTo address -> ["stock exit " ++ show (caseOutcome (caseFor facts domain (Image 0 B.empty) one) stock) ++ " expected the stop" | caseOutcome (caseFor facts domain (Image 0 B.empty) one) stock /= OutcomeStopped address]
    requireResult = ["owned r0 is " ++ show (M.findWithDefault 0 R0 (nativeRegisters owned)) ++ " expected " ++ show expected | Result expected <- caseExpect one, M.findWithDefault 0 R0 (nativeRegisters owned) /= expected]
    requireExpectations = ["owned " ++ show expectation ++ " not observed" | expectation <- caseExpect one, not (holdsOne expectation owned)]
    holdsOne (Equals address expected) observation = observedWord address observation == Just expected
    holdsOne (ByteEquals address expected) observation = (observedWord address observation >>= \word -> Just (fromIntegral (word .&. 0xff))) == Just expected
    holdsOne (Nonzero address) observation = maybe False (/= 0) (observedWord address observation)
    holdsOne (Result _) _ = True
    observedWord (GuestAddress address) observation =
      case [(base, bytes) | (ident, base) <- bases, base <= address, Just bytes <- [lookup (RegionId ident) (nativeMemory observation)], address < base + fromIntegral (B.length bytes)] of
        (base, bytes) : _ -> Just (readWordAt bytes (fromIntegral (address - base)))
        [] -> Nothing
    compareCpsr = ["cpsr " ++ show (control owned) ++ " expected " ++ show (control stock) | control owned /= control stock]
    control observation = M.findWithDefault 0 Cpsr (nativeRegisters observation) .&. 0x1ff
    compareMemory = ["region " ++ regionName ident ++ " differs" | (ident, expected) <- nativeMemory stock, Just actual <- [lookup ident (nativeMemory owned)], actual /= expected]
    compareOrder = ["ordered effects differ" | orderOf stock /= orderOf owned]
    orderOf observation = nativeKernel observation >>= observedOrder

data FamilyRun = FamilyRun
  { familyDomain :: Domain
  , familyCases :: [(String, Either String [String])]
  , familyMatches :: Int
  , familyReuse :: (BatchReuse, BatchReuse)
  }

runFamily :: FilePath -> FilePath -> FilePath -> Facts -> Domain -> Image -> Image -> IO FamilyRun
runFamily runner cacheRoot directory facts domain stock owned = do
  let stockCases = [(caseId (caseFor facts domain stock one), (caseName one, caseFor facts domain stock one)) | one <- cases facts domain]
      ownedCases = [(caseId (caseFor facts domain owned one), (caseName one, caseFor facts domain owned one)) | one <- cases facts domain]
  stockBatch <- writeBatch (directory </> "stock") (map (snd . snd) stockCases)
  ownedBatch <- writeBatch (directory </> "owned") (map (snd . snd) ownedCases)
  (stockReuse, stockRaw) <- cachedObservedBatch cacheRoot runner stockBatch
  (ownedReuse, ownedRaw) <- cachedObservedBatch cacheRoot runner ownedBatch
  let stockObserved = M.fromList [(caseIdText ident, observation) | (ident, observation) <- stockRaw]
      ownedObserved = M.fromList [(caseIdText ident, observation) | (ident, observation) <- ownedRaw]
  results <- forM stockCases $ \(ident, (label, _)) -> do
    let one = head [c | c <- cases facts domain, caseName c == label]
    pure (label, case (M.lookup ident stockObserved, M.lookup ident ownedObserved) of
      (Just s, Just o) -> Right (compareFamily facts domain one (stripScratch s) (stripScratch o))
      (Nothing, _) -> Left "stock case did not run"
      (_, Nothing) -> Left "owned case did not run")
  pure FamilyRun
    { familyDomain = domain
    , familyCases = results
    , familyMatches = length [() | (_, Right []) <- results]
    , familyReuse = (stockReuse, ownedReuse)
    }
