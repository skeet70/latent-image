{-# LANGUAGE OverloadedStrings #-}
module Replacement.RtosObjects.Suite
  ( Image (..), AggregatePayload (..), Payload (..), FamilyRun (..), extents, retainedStock, runFamily ) where

import Control.Monad (forM, unless)
import Data.Bits ((.&.), (.|.), shiftL, shiftR)
import qualified Data.ByteString as B
import Data.List (isPrefixOf, nub, sort)
import qualified Data.Map.Strict as M
import Data.Word (Word32, Word8, Word64)
import Emulation.CachedNativeBatch (BatchReuse, ResultPolicy (..), cachedObservedBatchWith)
import Emulation.Judge (StockEvidence (..), evidenceAt)
import Emulation.NativeCase (CaseId (..), caseIdText)
import qualified Emulation.NativeObservation as N
import Emulation.NativeObservation (Barrier (..), InstructionCount (..), KernelObservation (..), NativeObservation (..), Ordered (..), RegionId (..), Register (..), RunnerStatus (..), Write (..), WriteValue (..))
import Emulation.OwnedCase (CaseOutcome (..), CaseSpec (..), InstructionLimit (..), KernelCase (..), Permission (..), assemble, caseOutcome, mkRegion, mkStackContext, plainLimit, readWordAt, word32le, writeBatch)
import Replacement.RtosObjects.Layout hiding (record)
import Replacement.RtosOwnFirst.Regions (Domain (..), GuestAddress (..), domainLabel)
import qualified Replacement.RtosOwnFirst.Regions as Regions
import qualified Replacement.RtosPlatform.Aggregate as Agg
import System.FilePath ((</>), takeDirectory)

newtype Slot = Slot Word32 deriving (Eq, Show)
data Family = SetFlag | SetHandler | Clear | Wait | Poll | TimedWait | Create | Delete | Send deriving (Eq, Show)
data Helper = WaitRelease Domain | WaitBlock Domain | WaitBlockTimed Domain | Check Domain deriving (Eq, Show)
data Direct = Receive Domain | TimedReceive Domain deriving (Eq, Show)
data Call
  = ObjectService Family Domain
  | OwnerRelease Domain
  | HelperEntry Helper
  | DirectEntry Direct
  deriving (Eq, Show)
data Invocation = TaskInvocation | HandlerCallback | HandlerContext deriving (Eq, Show)
data Boundary = CallerReturn | TaskArrival deriving (Eq, Show)
data WakeOutcome = NoWaiter | Unsatisfied Domain | Released Domain deriving (Eq, Show)
data ObservationRole = StockObservation | OwnedObservation deriving Eq
data LockFence = AwaitAcquireFence | AwaitReleaseFence | PublishedLock | InvalidLockOrder deriving Eq
data Expect = Equals GuestAddress Word32 | ByteEquals GuestAddress Word8 | Result Word32 deriving Show

data Case = Case
  { caseName :: CaseId
  , caseCall :: Call
  , caseInvocation :: Invocation
  , caseWake :: WakeOutcome
  , caseArguments :: [(Register, Word32)]
  , caseWords :: [(GuestAddress, Word32)]
  , caseBoundary :: Boundary
  , caseExpect :: [Expect]
  , caseTimer :: Maybe GuestAddress
  , caseTimerIndex :: Maybe GuestAddress
  , caseStackArguments :: [Word32]
  , caseWaker :: Maybe Waker
  , caseOwnedExpect :: [Expect]
  , caseStockBudget :: Maybe Word32
  }

-- | The task the blocking wait hands control to: it calls one stock or owned service on the
-- blocked waiter and then re-enters the dispatcher, which resumes the caller.
data WakerTarget = WakerSet | WakerTick Int | WakerDelete deriving (Eq, Show)
data Waker = Waker { wakerTarget :: WakerTarget, wakerArguments :: [(Register, Word32)] }

data Image = Image { imageBase :: Word32, imageBytes :: B.ByteString }

data AggregatePayload = AggregatePayload
  { aggregateImage :: Agg.AggregateImage
  , aggregateSymbols :: [Agg.AggregateSymbol]
  , aggregatePlacements :: [Agg.SectionPlacement]
  , aggregateCarriers :: [Regions.Extent]
  , aggregateFill :: Word32
  }

data Payload = Standalone Image | Aggregate AggregatePayload

isAggregate :: Payload -> Bool
isAggregate (Aggregate _) = True
isAggregate _ = False
data Page = Page RegionId GuestAddress Int Permission B.ByteString

data FamilyRun = FamilyRun
  { familyDomain :: Domain
  , familyCases :: [(String, Either String [String])]
  , familyGuards :: [(String, Either String [String])]
  , familyMatches :: Int
  , familyReuse :: (BatchReuse, BatchReuse)
  , familyControls :: [(String, Bool)]
  , familyStockInstructions :: [(String, Word64)]
  , familyEvidence :: [(String, (Word32, Word32, Int))]
  }

wireAddress :: GuestAddress -> N.GuestAddress
wireAddress (GuestAddress address) = N.GuestAddress (fromIntegral address)

pointerWord :: GuestAddress -> Word32
pointerWord (GuestAddress address) = address

peer :: Domain -> Domain
peer DomainA = DomainB
peer DomainB = DomainA

runningSlot, waiterSlot, idleSlot :: Slot
runningSlot = Slot 1
waiterSlot = Slot 2
idleSlot = Slot 3

slotWord :: Slot -> Word32
slotWord (Slot slot) = slot

definitionBase, definitionStride, stackPointer, handlerStackPointer, targetStackPointer, exitAddress, stopAddress, receiptAddress, globalLock :: Word32
definitionBase = 0x0096c000
definitionStride = 0x40
stackPointer = 0x00959800
handlerStackPointer = 0x00959a00
targetStackPointer = 0x00959c00
exitAddress = 0x00958f00
stopAddress = 0x00959200
receiptAddress = 0x00960000
globalLock = 0x400c0804

timerToken :: GuestAddress
timerToken = GuestAddress (receiptAddress + 0x40)

-- | The synthetic waker task's own stack, stub window and the two mask words its stub records.
wakerSlot :: Slot
wakerSlot = Slot 2

wakerPriority, wakerStack, wakerStubBase, maskAddress, dispatchMaskAddress :: Word32
wakerPriority = 14
wakerStack = 0x00970000
wakerStubBase = 0x00959080
maskAddress = receiptAddress + 0x10
dispatchMaskAddress = receiptAddress + 0x14

dispatcherEntry, tickEntry :: Domain -> Word32
dispatcherEntry DomainA = 0x43640
dispatcherEntry DomainB = 0xd3640
tickEntry DomainA = 0x44d8c
tickEntry DomainB = 0xd4d4c

timerStub :: B.ByteString
timerStub = assemble [0xe3002000, 0xe3402096, 0xe5820000, 0xe5821004, 0xe3a00000, 0xe12fff1e]

timerEntry, releaseEntry, trampoline, resumeEntry :: Domain -> Word32
timerEntry DomainA = 0x44ca8
timerEntry DomainB = 0xd4c68
releaseEntry DomainA = 0x44568
releaseEntry DomainB = 0xd4528
trampoline DomainA = 0x43728
trampoline DomainB = 0xd3728
resumeEntry DomainA = 0x43668
resumeEntry DomainB = 0xd3668

-- | The stock extent an entered address belongs to, so a stock-side count is inside its function.
entryExtent :: Domain -> Word32 -> Word32
entryExtent domain entry = case [size | (address, size) <- extents domain, address <= entry, entry < address + size] of
  size : _ -> size
  [] -> 4

entryAddress :: Domain -> Family -> Word32
entryAddress DomainA family = case family of
  SetFlag -> 0x4813c
  SetHandler -> 0x4839c
  Clear -> 0x485c0
  Wait -> 0x486f8
  Poll -> 0x489ac
  TimedWait -> 0x48b30
  Create -> 0x4d39c
  Delete -> 0x4d434
  Send -> 0x4aa44
entryAddress DomainB family = case family of
  SetFlag -> 0xd80fc
  SetHandler -> 0xd835c
  Clear -> 0xd8580
  Wait -> 0xd86b8
  Poll -> 0xd896c
  TimedWait -> 0xd8af0
  Create -> 0xdd360
  Delete -> 0xdd3f8
  Send -> 0xdaa04

helperEntry :: Domain -> Helper -> Word32
helperEntry DomainA helper = case helper of
  WaitRelease _ -> 0x44744
  WaitBlock _ -> 0x44800
  WaitBlockTimed _ -> 0x448e4
  Check _ -> 0x480cc
helperEntry DomainB helper = case helper of
  WaitRelease _ -> 0xd4704
  WaitBlock _ -> 0xd47c0
  WaitBlockTimed _ -> 0xd48a4
  Check _ -> 0xd808c

directEntry :: Domain -> Direct -> Word32
directEntry DomainA direct = case direct of
  Receive _ -> 0x4ad48
  TimedReceive _ -> 0x4b0e4
directEntry DomainB direct = case direct of
  Receive _ -> 0xdad08
  TimedReceive _ -> 0xdb0a4

directDomain :: Direct -> Domain
directDomain (Receive domain) = domain
directDomain (TimedReceive domain) = domain

callEntry :: Domain -> Call -> Word32
callEntry executing call = case call of
  ObjectService family _ -> entryAddress executing family
  OwnerRelease _ -> releaseEntry executing
  HelperEntry helper -> helperEntry executing helper
  DirectEntry direct -> directEntry executing direct

lockEntries :: Domain -> (Word32, Word32)
lockEntries DomainA = (0x439d8, 0x43a14)
lockEntries DomainB = (0xd398c, 0xd39c8)

bank :: Domain -> GuestAddress
bank DomainA = GuestAddress 0xfffb30cc
bank DomainB = GuestAddress 0xfffb40cc

extents :: Domain -> [(Word32, Word32)]
extents DomainA =
  [ (0x40b00, 0x24), (0x43640, 0x28), (0x43668, 0x20), (0x43688, 0xa0)
  , (0x43728, 0x34), (0x4375c, 0x48), (0x437a4, 0x38), (0x437fc, 0x14)
  , (0x439d8, 0x3c), (0x43a14, 0x28), (0x43c48, 0x5c), (0x43ca4, 0x7c)
  , (0x43d20, 0xb0), (0x43dd0, 0x48), (0x43f70, 0x2c), (0x44018, 0x2c)
  , (0x44044, 0xf0), (0x44134, 0x80), (0x44568, 0xcc), (0x44744, 0xbc)
  , (0x44800, 0xe4), (0x448e4, 0xdc), (0x480cc, 0x70), (0x4813c, 0x260)
  , (0x4839c, 0x224), (0x485c0, 0x138), (0x486f8, 0x2b4), (0x489ac, 0x184)
  , (0x48b30, 0x308), (0x4aa44, 0x304), (0x4ad48, 0x254), (0x4af9c, 0x148)
  , (0x4b0e4, 0x2a8), (0x4d39c, 0x98), (0x4d434, 0x68), (0x4d5bc, 0x98)
  , (0x4d654, 0x70)
  ]
extents DomainB =
  [ (0xd0b00, 0x24), (0xd3640, 0x28), (0xd3668, 0x20), (0xd3688, 0xa0)
  , (0xd3728, 0x34), (0xd375c, 0x48), (0xd37a4, 0x38), (0xd37fc, 0x14)
  , (0xd398c, 0x3c), (0xd39c8, 0x28), (0xd3c04, 0x5c), (0xd3c60, 0x7c)
  , (0xd3cdc, 0xb0), (0xd3d8c, 0x48), (0xd3f2c, 0x2c), (0xd3fd4, 0x30)
  , (0xd4004, 0xf0), (0xd40f4, 0x80), (0xd4528, 0xcc), (0xd4704, 0xbc)
  , (0xd47c0, 0xe4), (0xd48a4, 0xdc), (0xd808c, 0x70), (0xd80fc, 0x260)
  , (0xd835c, 0x224), (0xd8580, 0x138), (0xd86b8, 0x2b4), (0xd896c, 0x184)
  , (0xd8af0, 0x308), (0xdaa04, 0x304), (0xdad08, 0x254), (0xdaf5c, 0x148)
  , (0xdb0a4, 0x2a8), (0xdd360, 0x98), (0xdd3f8, 0x68), (0xdd580, 0x98)
  , (0xdd618, 0x70)
  ]

retained :: Domain -> [(Word32, Word32)]
retained DomainA = [(0x4252c, 0x28), (0x433e4, 0x60), (0x43444, 0x60), (0x42560, 0x68), (0x446fc, 0x48), (0x44c60, 0x48), (0x444e4, 0x84), (0x4da9c, 0x44), (0x44318, 0x1cc), (0x44d8c, 0x1a0)]
retained DomainB = [(0xd252c, 0x28), (0xd33e4, 0x60), (0xd3444, 0x60), (0xd2560, 0x68), (0xd46bc, 0x48), (0xd4c20, 0x48), (0xd44a4, 0x84), (0xdda5c, 0x44), (0xd42d8, 0x1cc), (0xd4d4c, 0x1a0)]

recordAddress :: Facts -> Domain -> Domain -> Word32
recordAddress facts executing described = case described of
  DomainA -> dfContextA (domainFacts facts executing)
  DomainB -> dfContextB (domainFacts facts executing)

definitionAddress :: Facts -> Domain -> Slot -> Word32
definitionAddress facts domain slot = definitionBase + (index * factsTaskSlots facts + slotWord slot - 1) * definitionStride
  where index = if domain == DomainA then 0 else 1

taskAt :: Facts -> Domain -> Slot -> Word32
taskAt facts domain = task facts domain . slotWord

packedState :: Word8 -> Word32 -> Word32
packedState state priority = fromIntegral state .|. (priority `shiftL` 8)

poke :: Word32 -> Word32 -> (GuestAddress, Word32)
poke address value = (GuestAddress address, value)

cases :: Facts -> Domain -> [Case]
cases facts executing =
  [ service "set-no-waiter" SetFlag executing NoWaiter CallerReturn [(R1, 1)] [] [Result 0, eq patternCell 1]
  , service "set-satisfied" SetFlag executing (Released executing) TaskArrival [(R1, 1)] (flagWaiter executing executing 1) [eq reportCell 1, ready executing]
  , service "set-unsatisfied" SetFlag executing (Unsatisfied executing) CallerReturn [(R1, 1)] (flagWaiter executing executing 2) [Result 0, eq patternCell 1, state executing queued]
  , service "set-clear-on-match" SetFlag executing (Released executing) TaskArrival [(R1, 1)] (flagWaiter executing executing 1 ++ [poke (flag + factsFlagConfigAttributes facts) (factsFlagClearOnMatch facts)]) [eq patternCell 0, eq reportCell 1]
  , service "set-waiter-peer" SetFlag executing (Released remote) CallerReturn [(R1, 1)] (flagWaiter remote executing 1)
      [Result 0, ready remote, eq (dfSelected remoteWords) remoteTask, Equals (bank remote) 0x01000000, Equals (bank executing) 0]
  , service "set-object-peer" SetFlag remote (Released executing) TaskArrival [(R1, 1)] (flagWaiter executing remote 1) [ready executing, eq (dfSelected ownWords) localTask, Equals (bank remote) 0, Equals (bank executing) 0]
  , withTimer (service "set-timer-cancel" SetFlag executing (Released executing) TaskArrival [(R1, 1)] (flagWaiter executing executing 1) [ready executing])
  , serviceWith HandlerContext "set-handler-no-waiter" SetHandler executing NoWaiter CallerReturn [(R1, 4)] [] [Result 0, eq patternCell 4, eq (dfReschedule ownWords) 0]
  , serviceWith HandlerContext "set-handler-satisfied" SetHandler executing (Released executing) CallerReturn [(R1, 1)] (flagWaiter executing executing 1)
      [Result 0, eq patternCell 1, eq reportCell 1, ready executing, eq (dfReschedule ownWords) 1]
  , service "clear-keep" Clear executing NoWaiter CallerReturn [(R1, 0x0f)] [poke patternCell 0xff] [Result 0, eq patternCell 0x0f]
  , service "clear-none" Clear executing NoWaiter CallerReturn [(R1, 0)] [poke patternCell 0xff] [Result 0, eq patternCell 0]
  , service "wait-all-match" Wait executing NoWaiter CallerReturn [(R1, 0x11), (R2, 0), (R3, outputCell)]
      [poke patternCell 0x33] [Result 0, eq outputCell 0x33, eq patternCell 0x33]
  , service "wait-any-match" Wait executing NoWaiter CallerReturn [(R1, 0x10), (R2, 1), (R3, outputCell)]
      [poke patternCell 0x33] [Result 0, eq outputCell 0x33]
  , service "poll-match" Poll executing NoWaiter CallerReturn [(R1, 0x11), (R2, 0), (R3, outputCell)]
      [poke patternCell 0x33] [Result 0, eq outputCell 0x33]
  , service "poll-timeout" Poll executing NoWaiter CallerReturn [(R1, 0x44), (R2, 0), (R3, outputCell)]
      [poke patternCell 0x33] [Result 0xffffffce, eq outputCell 0, eq patternCell 0x33]
  , service "poll-second-waiter" Poll executing NoWaiter CallerReturn [(R1, 0x11), (R2, 0), (R3, outputCell)]
      (flagWaiter executing executing 1) [Result 0xffffffe4, eq outputCell 0]
  , withStack [100] (service "timed-wait-match" TimedWait executing NoWaiter CallerReturn [(R1, 0x11), (R2, 0), (R3, outputCell)]
      [poke patternCell 0x33] [Result 0, eq outputCell 0x33])
  , withStack [0] (service "timed-wait-poll" TimedWait executing NoWaiter CallerReturn [(R1, 0x44), (R2, 0), (R3, outputCell)]
      [poke patternCell 0x33] [Result 0xffffffce, eq outputCell 0, eq patternCell 0x33])
  , service "wait-blocks" Wait executing NoWaiter TaskArrival [(R1, 0x44), (R2, 0), (R3, outputCell)]
      [poke patternCell 0x33] blockChecks
  , withStack [50] (service "timed-wait-arms" TimedWait executing NoWaiter TaskArrival [(R1, 0x44), (R2, 0), (R3, outputCell)]
      [poke patternCell 0x33] (filter (not . isResultZero) blockChecks ++
        [ eq (blockWait + factsWaitBlockResult facts) runningTimer
        , eq runningTimer 1, eq (runningTimer + 4) timeoutCallback, eq (runningTimer + 8) runningTask
        , eq (dfTimerCount ownWords) 1, eq (dfTimerHeap ownWords + 4) runningTimer
        ]))
  , resumed "wait-resume-set" Wait 0 (Waker WakerSet [(R0, flagHandle), (R1, 0x11)])
      [ eq (blockWait + factsWaitBlockResult facts) 0, eq outputCell 0x11, Result 0 ]
  , (resumed "timed-wait-resume-tick" TimedWait 5 (Waker (WakerTick 2) [])
      [ eq (blockWait + factsWaitBlockResult facts) 0xffffffce, eq outputCell 0, Result 0xffffffce
      , eq (dfTimerCount ownWords) 0 ])
      { caseOwnedExpect = [eq (dfFlagTimers ownWords) 0] }
  , (resumed "wait-resume-delete" Wait 0 (Waker WakerDelete [(R0, flagHandle)])
      [])
      { caseOwnedExpect =
          [ eq (blockWait + factsWaitBlockResult facts) 0xffffffcf, eq outputCell 0, Result 0xffffffcf
          , byte (runningTask + factsTaskState facts) (factsStateReady facts)
          , eq controlQueue controlQueue, eq (controlQueue + factsWaitQueuePrev facts) controlQueue ]
      , caseStockBudget = Just 20000
      }
  , service "create-fresh" Create executing NoWaiter CallerReturn [(R1, requestPointer)]
      [poke (flag + factsFlagConfigExists facts) 0, poke requestPointer 0, poke (requestPointer + 4) 0x5a]
      [Result 0, eq (flag + factsFlagConfigExists facts) 1, eq (flag + factsFlagConfigAttributes facts) 0
      , eq (flag + factsFlagConfigInitial facts) 0x5a, eq patternCell 0x5a]
  , service "create-existing" Create executing NoWaiter CallerReturn [(R1, requestPointer)]
      [poke requestPointer 0, poke (requestPointer + 4) 0x5a]
      [Result 0xffffffd7, eq (flag + factsFlagConfigExists facts) 1, eq (flag + factsFlagConfigInitial facts) 0]
  , service "delete-idle" Delete executing NoWaiter CallerReturn [] []
      [Result 0, eq (flag + factsFlagConfigExists facts) 0, eq (flag + factsFlagConfigAttributes facts) 0
      , eq (flag + factsFlagConfigInitial facts) 0, eq patternCell 0
      , eq controlQueue controlQueue, eq (controlQueue + factsWaitQueuePrev facts) controlQueue]
  , service "send-blocked" Send executing (Released executing) TaskArrival [(R1, message)] (mailWaiter executing ++ [poke message sentinel]) [eq mailResult message, eq message sentinel]
  , service "send-empty-fifo" Send executing NoWaiter CallerReturn [(R1, message)] [poke message sentinel] [Result 0, eq (mailControl + factsMailboxControlHead facts) message, eq (mailControl + factsMailboxControlTail facts) message, eq message 0]
  , service "send-priority-equal" Send executing NoWaiter CallerReturn [(R1, message)]
      [ poke (mailbox + factsMailboxConfigAttributes facts) (factsMailboxMessagePriority facts)
      , poke (mailControl + factsMailboxControlHead facts) oldMessage
      , poke (mailControl + factsMailboxControlTail facts) oldMessage
      , poke oldMessage 0, poke (oldMessage + 4) 5, poke message sentinel, poke (message + 4) 5
      ] [Result 0, eq oldMessage message, eq message 0, eq (mailControl + factsMailboxControlTail facts) oldMessage]
  , service "send-waiter-peer" Send executing (Released remote) CallerReturn [(R1, message)] (mailWaiter remote) [Result 0, ready remote, eq (mailboxWait facts remote 2 + factsMailboxWaitMessage facts) message, Equals (bank remote) 0x01000000, Equals (bank executing) 0]
  , release "release-plain-insert" executing TaskInvocation [] [Result 1, ready executing]
  , release "release-terminate-reset" executing TaskInvocation [poke (localTask + factsTaskTerminatePending facts) 1] [Result 0, state executing (factsStateDormant facts), eq (localTask + factsTaskContextDiscarded facts) 1]
  , release "release-terminate-prepare" executing TaskInvocation
      [poke (localTask + factsTaskTerminatePending facts) 1, poke (localTask + factsTaskState facts) (packedState queued 3 .|. 0x10000)]
      [Result 1, ready executing, byte (localTask + factsTaskFlags facts) 0, eq (localTask + factsTaskSavedSp facts) targetStackPointer, eq (localTask + factsTaskResumePc facts) (trampoline executing)]
  , release "release-priority" executing TaskInvocation [poke (localTask + factsTaskPriorityChangePending facts) 1, poke (localTask + factsTaskPendingPriority facts) 7]
      [Result 1, byte (localTask + factsTaskPriority facts) 7, eq (localTask + factsTaskPriorityChangePending facts) 0]
  , release "release-suspended" executing TaskInvocation [poke (localTask + factsTaskState facts) (packedState (queued .|. factsStateSuspended facts) 3)] [Result 0, state executing (factsStateSuspended facts)]
  , withTimer (release "release-timer" executing TaskInvocation [] [Result 1, ready executing])
  , release "release-owner-peer" remote TaskInvocation [] [Result 1, ready remote, eq (dfSelected remoteWords) remoteTask]
  , release "release-plain-insert-handler" executing HandlerCallback [] [Result 1, ready executing]
  , withTimer (release "release-timer-handler" executing HandlerCallback [] [Result 1, ready executing])
  , service "set-satisfied-no-dispatch" SetFlag executing (Released executing) CallerReturn [(R1, 1)] (flagWaiter executing executing 1 ++ noDispatch) [Result 0, ready executing, eq reportCell 1]
  , service "send-blocked-no-dispatch" Send executing (Released executing) CallerReturn [(R1, message)] (mailWaiter executing ++ [poke message sentinel] ++ noDispatch) [Result 0, ready executing, eq mailResult message, eq message sentinel]
  , helper "helper-release-queued" (HelperEntry (WaitRelease executing)) CallerReturn [(R1, localTask)] (flagWaiter executing executing 1)
      [Result 1, eq waitCell 0xffffffcf, ready executing, eq controlQueue controlQueue, eq (controlQueue + factsWaitQueuePrev facts) controlQueue]
  , helper "helper-release-unqueued" (HelperEntry (WaitRelease executing)) CallerReturn [(R1, localTask)]
      (flagWaiter executing executing 1 ++ [poke (localTask + factsTaskState facts) (packedState (factsStateWait facts) 3)])
      [Result 1, eq waitCell 0xffffffcf, ready executing, eq controlQueue localTask, eq (controlQueue + factsWaitQueuePrev facts) localTask]
  , helper "helper-block" (HelperEntry (WaitBlock executing)) CallerReturn
      [(R1, controlQueue), (R2, blockWait)] []
      [ eq (blockWait + factsWaitBlockResult facts) 0, eq (blockWait + factsWaitBlockQueue facts) controlQueue
      , eq (blockWait + factsWaitBlockOwner facts) ownerWord, eq (runningTask + factsTaskWait facts) blockWait
      , byte (runningTask + factsTaskState facts) (factsStateWait facts .|. factsStateObjectQueued facts .|. factsStateOwnerLockQueued facts)
      , eq controlQueue runningTask, eq (controlQueue + factsWaitQueuePrev facts) runningTask
      , eq (runningTask + factsTaskNext facts) controlQueue, eq (runningTask + factsTaskPrev facts) controlQueue]
  , withStack [0] (helper "helper-block-timed" (HelperEntry (WaitBlockTimed executing)) CallerReturn
      [(R1, controlQueue), (R2, blockWait), (R3, mailTimer1)] []
      [ eq (blockWait + factsWaitBlockResult facts) 0, eq (blockWait + factsWaitBlockQueue facts) controlQueue
      , eq (blockWait + factsWaitBlockOwner facts) ownerWord, eq (runningTask + factsTaskWait facts) blockWait
      , eq controlQueue runningTask, eq (dfTimerCount ownWords) 0])
  , helper "helper-check-match" (HelperEntry (Check executing)) CallerReturn [(R1, 0x11), (R2, 0), (R3, outputCell)]
      [poke patternCell 0x33] [Result 1, eq outputCell 0x33, eq patternCell 0x33]
  , helper "helper-check-miss" (HelperEntry (Check executing)) CallerReturn [(R1, 0x44), (R2, 0), (R3, outputCell)]
      [poke patternCell 0x33] [Result 0, eq outputCell 0, eq patternCell 0x33]
  , helper "helper-check-clear-on-match" (HelperEntry (Check executing)) CallerReturn [(R1, 0x11), (R2, 1), (R3, outputCell)]
      [poke patternCell 0x33, poke (flag + factsFlagConfigAttributes facts) (factsFlagClearOnMatch facts)]
      [Result 1, eq outputCell 0x33, eq patternCell 0]
  , helper "mailbox-receive-stock" (DirectEntry (Receive executing)) TaskArrival [(R1, outputCell)]
      [poke (runningDefinition + 0x20) 1]
      [ eq (mailWait1 + factsWaitBlockResult facts) 0, eq (mailWait1 + factsWaitBlockQueue facts) mailControl
      , eq (mailWait1 + factsWaitBlockOwner facts) ownerWord, eq (runningTask + factsTaskWait facts) mailWait1
      , byte (runningTask + factsTaskState facts) (factsStateWait facts .|. factsStateObjectQueued facts .|. factsStateOwnerLockQueued facts)
      , eq mailControl runningTask, eq (mailControl + factsWaitQueuePrev facts) runningTask
      , eq outputCell 0, eq (dfSelected ownWords) (taskAt facts executing idleSlot) ]
  , helper "mailbox-timed-receive-stock" (DirectEntry (TimedReceive executing)) TaskArrival [(R1, outputCell), (R2, 50)]
      [poke (runningDefinition + 0x20) 1]
      [ eq (mailWait1 + factsWaitBlockResult facts) mailTimer1, eq (mailWait1 + factsWaitBlockQueue facts) mailControl
      , eq (mailWait1 + factsWaitBlockOwner facts) ownerWord
      , byte (runningTask + factsTaskState facts) (factsStateWait facts .|. factsStateObjectQueued facts .|. factsStateOwnerLockQueued facts)
      , eq mailControl runningTask, eq (dfTimerCount ownWords) 1, eq (mailTimer1 + 4) timeoutCallback
      , eq (mailTimer1 + 8) runningTask, eq outputCell 0 ]
  ]
  where
    ownWords = domainFacts facts executing
    remote = peer executing
    remoteWords = domainFacts facts remote
    localTask = taskAt facts executing waiterSlot
    remoteTask = taskAt facts remote waiterSlot
    flag = flagConfig facts executing 1
    patternCell = flagControl facts executing 1 + factsFlagControlPattern facts
    controlQueue = flagControl facts executing 1
    reportCell = flagWait facts executing 2 + factsFlagWaitResult facts
    mailbox = mailboxConfig facts executing 1
    mailControl = mailboxControl facts executing 1
    mailResult = mailboxWait facts executing 2 + factsMailboxWaitMessage facts
    mailWait1 = mailboxWait facts executing 1
    mailTimer1 = dfMailboxTimers ownWords
    timeoutCallback = case executing of DomainA -> 0x44318; DomainB -> 0xd42d8
    runningTask = taskAt facts executing runningSlot
    runningDefinition = definitionAddress facts executing runningSlot
    blockWait = flagWait facts executing 1
    ownerWord = dfId ownWords
    requestPointer = 0x00957400
    outputCell = 0x40000008
    message = 0x40000000
    oldMessage = message + 8
    sentinel = 0x5e4d1e00
    queued = factsStateWait facts .|. factsStateObjectQueued facts
    noDispatch = [poke (dfDispatchEnabled ownWords) 0]
    eq address value = Equals (GuestAddress address) value
    byte address value = ByteEquals (GuestAddress address) value
    state owner value = byte (taskAt facts owner waiterSlot + factsTaskState facts) value
    ready owner = state owner (factsStateReady facts)
    runningTimer = dfFlagTimers ownWords
    flagHandle = (ownerWord `shiftL` 16) .|. 1
    isResultZero (Equals (GuestAddress address) 0) = address == blockWait + factsWaitBlockResult facts
    isResultZero _ = False
    blockChecks =
      [ eq (blockWait + factsWaitBlockResult facts) 0, eq (blockWait + factsWaitBlockQueue facts) controlQueue
      , eq (blockWait + factsWaitBlockOwner facts) ownerWord, eq (runningTask + factsTaskWait facts) blockWait
      , byte (runningTask + factsTaskState facts) (factsStateWait facts .|. factsStateObjectQueued facts .|. factsStateOwnerLockQueued facts)
      , eq controlQueue runningTask, eq (controlQueue + factsWaitQueuePrev facts) runningTask
      , eq (runningTask + factsTaskNext facts) controlQueue, eq (runningTask + factsTaskPrev facts) controlQueue
      ]
    resumed name family timeout waker checks =
      (serviceWith TaskInvocation name family executing NoWaiter CallerReturn
        [(R1, 0x11), (R2, 0), (R3, outputCell)] [poke patternCell 0] checks)
        { caseBoundary = CallerReturn
        , caseWaker = Just waker
        , caseStackArguments = [timeout]
        }
    service = serviceWith TaskInvocation
    serviceWith invocation name family objectDomain outcome boundary args words' checks =
      Case (CaseId name) (ObjectService family objectDomain) invocation outcome args words' boundary checks Nothing Nothing [] Nothing [] Nothing
    helper name call boundary args words' checks =
      Case (CaseId name) call TaskInvocation (Released executing) args (helperWords call ++ words') boundary checks Nothing Nothing [] Nothing [] Nothing
    helperWords (HelperEntry _) = [poke (dfSchedulerLock ownWords) 1, poke (dfObjectLock ownWords) 1]
    helperWords _ = []
    release name owner invocation words' checks =
      Case (CaseId name) (OwnerRelease owner) invocation (Released owner) [(R1, taskAt facts owner waiterSlot)] (releaseWords owner ++ words') CallerReturn checks Nothing Nothing [] Nothing [] Nothing
    flagWaiter owner objectDomain wanted =
      let queue = flagControl facts objectDomain 1
          wait = flagWait facts owner 2
       in queuedTask owner queue wait
            ++ [poke (wait + factsFlagWaitPattern facts) wanted, poke (wait + factsFlagWaitMode facts) 1]
    mailWaiter owner = queuedTask owner mailControl (mailboxWait facts owner 2)
    queuedTask owner queue wait =
      let taskAddress = taskAt facts owner waiterSlot
          objectDomain = if queue == flagControl facts remote 1 then remote else executing
       in [ poke queue taskAddress, poke (queue + factsWaitQueuePrev facts) taskAddress
          , poke (taskAddress + factsTaskNext facts) queue, poke (taskAddress + factsTaskPrev facts) queue
          , poke (taskAddress + factsTaskState facts) (packedState queued 3), poke (taskAddress + factsTaskWait facts) wait
          , poke (wait + factsWaitBlockQueue facts) queue, poke (wait + factsWaitBlockOwner facts) (recordAddress facts owner objectDomain)
          ]
    releaseWords owner =
      let taskAddress = taskAt facts owner waiterSlot
          wait = flagWait facts owner 2
          words' = domainFacts facts owner
       in [ poke (taskAddress + factsTaskState facts) (packedState queued 3), poke (taskAddress + factsTaskWait facts) wait
          , poke (wait + factsWaitBlockResult facts) 0, poke (wait + factsWaitBlockOwner facts) (recordAddress facts owner owner)
          , poke (dfSchedulerLock words') 1, poke (dfObjectLock words') 1
          ]
    withStack values one = one { caseStackArguments = values ++ caseStackArguments one }
    waitCell = flagWait facts executing 2 + factsWaitBlockResult facts
    withTimer one =
      let owner = case caseWake one of Released domain -> domain; _ -> error "timer case has no waiter"
          wait = flagWait facts owner 2
       in one {caseTimer = Just timerToken, caseWords = caseWords one ++ [poke (wait + factsWaitBlockResult facts) (pointerWord timerToken)], caseExpect = caseExpect one ++ [eq (wait + factsWaitBlockResult facts) 0, eq receiptAddress (recordAddress facts executing owner), eq (receiptAddress + 4) (pointerWord timerToken)]}

bindTimer :: Facts -> Domain -> Payload -> Case -> Case
bindTimer facts _executing payload one
  | isAggregate payload, Just _ <- caseTimer one =
      let owner = case caseWake one of Released domain -> domain; _ -> error "timer case has no owner"
          dw = domainFacts facts owner
          entry = dfFlagTimers dw + factsTimerEntrySize facts
          wait = flagWait facts owner 2
          receipt (Equals (GuestAddress address) _) = address == receiptAddress || address == receiptAddress + 4
          receipt _ = False
       in one
          { caseTimer = Nothing
          , caseTimerIndex = Just (GuestAddress entry)
          , caseWords = caseWords one ++
              [poke (wait + factsWaitBlockResult facts) entry, poke entry 1,
               poke (dfTimerCount dw) 1, poke (dfTimerHeap dw) 0, poke (dfTimerHeap dw + 4) entry]
          , caseExpect = filter (not . receipt) (caseExpect one) ++ [Equals (GuestAddress (dfTimerCount dw)) 0]
          }
  | otherwise = one

layoutWords :: Facts -> Case -> [(GuestAddress, Word32)]
layoutWords facts one =
  concat [recordPokes block described | block <- domains, described <- domains]
    ++ concatMap taskPokes domains ++ concatMap objectPokes domains ++ concatMap readyPokes domains
  where
    domains = [DomainA, DomainB]
    waker = case caseWaker one of Just _ -> True; Nothing -> False
    recordPokes block described =
      let record = recordAddress facts block described
          words' = domainFacts facts described
          (acquire, release) = lockEntries block
          lock offset address = [poke (record + offset + factsLockWord facts) address, poke (record + offset + factsLockAcquire facts) acquire, poke (record + offset + factsLockRelease facts) release]
       in [ poke (record + factsContextId facts) (if block == described then factsOwnId facts else dfId words')
          , poke (record + factsContextOwner facts) (dfId words')
          , poke (record + factsContextReadyHeads facts) (dfReadyHeads words')
          , poke (record + factsContextReadyBitmap facts) (dfReadyBitmap words')
          , poke (record + factsContextRunning facts) (dfRunning words')
          , poke (record + factsContextSelected facts) (dfSelected words')
          , poke (record + factsContextDispatchEnabled facts) (dfDispatchEnabled words')
          , poke (record + factsContextFirstEntry facts) (trampoline described)
          , poke (record + factsContextTasks facts) (dfTaskTable words')
          , poke (record + factsContextTaskCount facts) (factsTaskSlots facts)
          , poke (record + factsContextTimerHeap facts) (dfTimerHeap words')
          , poke (record + factsContextTimerCount facts) (dfTimerCount words')
          , poke (record + factsContextTimerBase facts) (dfTimerBase words')
          , poke (record + factsContextTickNow facts) (dfTickNow words')
          , poke (record + factsContextFlags facts) (dfFlagControls words')
          , poke (record + factsContextFlagCount facts) 120
          , poke (record + factsContextMailboxes facts) (dfMailboxControls words')
          , poke (record + factsContextMailboxCount facts) 1
          ] ++ lock (factsContextSchedulerLock facts) (dfSchedulerLock words') ++ lock (factsContextObjectLock facts) (dfObjectLock words')
    taskPokes domain = concat
      [ let address = taskAt facts domain slot
            definition = definitionAddress facts domain slot
            isWaker = waker && slot == wakerSlot
            priority = if slot == runningSlot then 10 else if isWaker then wakerPriority else if slot == idleSlot then 15 else if slot == waiterSlot then 3 else slotWord slot `mod` factsPriorities facts
         in [ poke address address, poke (address + factsTaskPrev facts) address
            , poke (address + factsTaskDefinition facts) definition, poke (address + factsTaskDomain facts) (dfId (domainFacts facts domain))
            , poke (address + factsTaskState facts) (packedState (factsStateDormant facts) priority)
            , poke (address + factsTaskSavedSp facts) (if isWaker then wakerStack else targetStackPointer)
            , poke (address + factsTaskResumePc facts) (if isWaker then wakerStubBase else stopAddress)
            , poke (definition + factsDefinitionPriority facts) priority
            , poke (definition + 4) (0xa5510000 + slotWord slot), poke (definition + 8) stopAddress
            , poke (definition + 0x10) (targetStackPointer - 0x100), poke (definition + 0x14) 0x100
            , poke (definition + 0x20) (slotWord slot)
            ]
      | slot <- map Slot [1 .. factsTaskSlots facts]
      ]
    objectPokes domain =
      concat
        [ let config = flagConfig facts domain slot
              control = flagControl facts domain slot
           in [poke (config + factsFlagConfigExists facts) 1, poke control control, poke (control + factsWaitQueuePrev facts) control, poke (control + factsWaitQueueConfig facts) config]
        | slot <- [1 .. 120]
        ]
        ++ let config = mailboxConfig facts domain 1
               control = mailboxControl facts domain 1
            in [poke (config + factsMailboxConfigExists facts) 1, poke (config + factsMailboxConfigPriorityLimit facts) 32, poke control control, poke (control + factsWaitQueuePrev facts) control, poke (control + factsWaitQueueConfig facts) config]
    readyPokes domain =
      let words' = domainFacts facts domain
          headAt priority = dfReadyHeads words' + priority * factsHeadSize facts
          member slot priority =
            let address = taskAt facts domain slot
                headAddress = headAt priority
             in [poke headAddress address, poke (headAddress + factsHeadPrev facts) address, poke address headAddress, poke (address + factsTaskPrev facts) headAddress, poke (address + factsTaskState facts) (packedState (factsStateReady facts) priority)]
       in concat [[poke (headAt priority) (headAt priority), poke (headAt priority + factsHeadPrev facts) (headAt priority)] | priority <- [0 .. factsPriorities facts - 1]]
            ++ member runningSlot 10 ++ member idleSlot 15
            ++ concat [member wakerSlot wakerPriority | waker]
            ++ [ poke (dfReadyBitmap words') ((1 `shiftL` 10) .|. (1 `shiftL` 15) .|. (if waker then 1 `shiftL` fromIntegral wakerPriority else 0))
               , poke (dfRunning words') (taskAt facts domain runningSlot), poke (dfSelected words') (taskAt facts domain runningSlot)
               , poke (dfDispatchEnabled words') 1, poke (dfSchedulerLock words') 0, poke (dfObjectLock words') 0
               , poke (dfCurrentContext words') (recordAddress facts domain domain)
               ]

kernelBounds :: Facts -> Domain -> (GuestAddress, Int)
kernelBounds facts domain = (GuestAddress low, fromIntegral (high - low))
  where
    words' = domainFacts facts domain
    low = minimum [dfHandlerContext words', dfCurrentContext words', dfContexts words', dfSchedulerLock words', dfObjectLock words', dfTimerHeap words']
    high = maximum
      [ dfContextA words' + factsContextSize facts, dfContextB words' + factsContextSize facts
      , dfReadyHeads words' + factsPriorities facts * factsHeadSize facts, dfSelected words' + 4
      , dfTaskTable words' + factsTaskSlots facts * factsTaskSize facts
      , dfFlagConfigs words' + 120 * factsFlagConfigSize facts, dfFlagControls words' + 120 * factsFlagControlSize facts
      , dfFlagWaits words' + factsTaskSlots facts * factsFlagWaitSize facts, dfFlagTimers words' + factsTaskSlots facts * factsTimerEntrySize facts
      , dfMailboxConfigs words' + factsMailboxConfigSize facts, dfMailboxControls words' + factsMailboxControlSize facts
      , dfMailboxWaits words' + factsTaskSlots facts * factsMailboxWaitSize facts, dfMailboxTimers words' + factsTaskSlots facts * factsTimerEntrySize facts
      , dfTimerHeap words' + 0x1000, dfTimerCount words' + 4, dfTimerBase words' + 4, dfTickNow words' + 4
      ]

callerSp :: Case -> Word32
callerSp one = if caseInvocation one == HandlerCallback then handlerStackPointer else stackPointer

-- | The caller's own frame, with a fifth argument at its top for the helpers that take one.
callerBytes :: Case -> B.ByteString
callerBytes one = B.concat [word32le (word index) | index <- [0 .. 15]]
  where
    stack = caseStackArguments one
    word index
      | index < length stack = stack !! index
      | otherwise = 0xca11e500 + fromIntegral index

pages :: Facts -> Case -> [Page]
pages facts one =
  [ let (address, size) = kernelBounds facts domain in page (if domain == DomainA then "kernel-a" else "kernel-b") address size ReadWrite
  | domain <- [DomainA, DomainB]
  ]
  ++ [ page "definitions" (GuestAddress definitionBase) (fromIntegral (2 * factsTaskSlots facts * definitionStride)) ReadWrite
     , page "messages" (GuestAddress 0x40000000) 16 ReadWrite
     , page "request" (GuestAddress 0x00957400) 16 ReadWrite
     , page "timer-seam" (GuestAddress receiptAddress) 0x100 ReadWrite
     , page "global-lock" (GuestAddress globalLock) 4 ReadWrite
     , page "stack" (GuestAddress (callerSp one - 0x400)) 0x400 ReadWrite
     , Page (RegionId "caller-frame") (GuestAddress (callerSp one)) (B.length (callerBytes one)) ReadWrite (callerBytes one)
     , page "target-stack" (GuestAddress (targetStackPointer - 0x100)) 0x100 ReadWrite
     , page "bank-a" (bank DomainA) 4 Device
     , page "bank-b" (bank DomainB) 4 Device
     ]
     ++ [page "waker-stack" (GuestAddress (wakerStack - 0x400)) 0x400 ReadWrite | case caseWaker one of Just _ -> True; Nothing -> False]
  where page name address size permission = Page (RegionId name) address size permission (B.replicate size 0)

contains :: GuestAddress -> Int -> Page -> Bool
contains (GuestAddress address) width (Page _ (GuestAddress base) size _ _) =
  toInteger base <= toInteger address && toInteger address + toInteger width <= toInteger base + toInteger size

overlay :: GuestAddress -> B.ByteString -> [(GuestAddress, Word32)] -> B.ByteString
overlay (GuestAddress start) initial words' = B.concat (pieces 0 ordered)
  where
    ordered = M.toAscList (M.fromList [(address, value) | (address@(GuestAddress raw), value) <- words', raw >= start, toInteger raw + 4 <= toInteger start + toInteger (B.length initial)])
    pieces cursor [] = [B.drop cursor initial]
    pieces cursor ((GuestAddress address, value) : rest) =
      let offset = fromIntegral (address - start)
       in B.take (offset - cursor) (B.drop cursor initial) : word32le value : pieces (offset + 4) rest

codeBounds :: Domain -> Payload -> (Word32, Word32)
codeBounds domain payload =
  let spans = extents domain ++ retained domain ++ case payload of
        Standalone _ -> []
        Aggregate aggregate -> [(Agg.imageBase (aggregateImage aggregate), fromIntegral (B.length (Agg.imageBytes (aggregateImage aggregate))))]
   in (minimum (map fst spans), maximum [address + size | (address, size) <- spans])

codeBytes :: Domain -> ObservationRole -> Image -> Payload -> B.ByteString
codeBytes domain role loader payload = case payload of
  Aggregate aggregate | role == OwnedObservation ->
    either error id
      (Agg.installCarriers base original (aggregateFill aggregate) (aggregateSymbols aggregate)
        (aggregatePlacements aggregate) (aggregateImage aggregate)
        [(lo, hi - lo) | Regions.Extent (GuestAddress lo) (GuestAddress hi) <- aggregateCarriers aggregate])
  Aggregate _ -> original
  Standalone owned -> install (timerEntry domain) timerStub
    (foldl (\current (address, size) -> install address (slice owned address size) current) original (extents domain))
  where
    (base, top) = codeBounds domain payload
    slice image address size =
      if address < imageBase image || toInteger address + toInteger size > toInteger (imageBase image) + toInteger (B.length (imageBytes image))
        then error "F2 code slice is outside its image"
        else B.take (fromIntegral size) (B.drop (fromIntegral (address - imageBase image)) (imageBytes image))
    low = max base (imageBase loader)
    high = min top (imageBase loader + fromIntegral (B.length (imageBytes loader)))
    original = install low (slice loader low (high - low)) (B.replicate (fromIntegral (top - base)) 0)
    install address bytes current = let offset = fromIntegral (address - base) in B.take offset current <> bytes <> B.drop (offset + B.length bytes) current

-- | Non-carrier extents whose bytes come from the stock loader in the owned role.
retainedStock :: Domain -> Payload -> [(Word32, Word32)]
retainedStock domain payload = case payload of
  Aggregate aggregate ->
    let allocated = [(lo, hi - lo) | Regions.Extent (GuestAddress lo) (GuestAddress hi) <- aggregateCarriers aggregate]
        overlaps (lo,size) (at,width) = lo < at + width && at < lo + size
     in filter (\extent -> not (any (overlaps extent) allocated)) (extents domain ++ retained domain)
  Standalone _ -> []

functionSpans :: Domain -> ObservationRole -> Payload -> [(Word32, Word32)]
functionSpans domain role payload = case payload of
  Aggregate aggregate | role == OwnedObservation ->
    let symbols = aggregateSymbols aggregate
        placements = aggregatePlacements aggregate
        allocated = [(lo, hi - lo) | Regions.Extent (GuestAddress lo) (GuestAddress hi) <- aggregateCarriers aggregate]
        overlaps (lo,size) (at,width) = lo < at + width && at < lo + size
        kept = filter (\extent -> not (any (overlaps extent) allocated)) (extents domain ++ retained domain ++ [(releaseEntry domain, 0xcc)])
        sections = [(Agg.placementAddress p, Agg.placementBytes p) | p <- placements, Agg.placementBytes p > 0, Agg.placementSection p == ".text" || ".text." `isPrefixOf` Agg.placementSection p]
        spans = sections ++ [(Agg.symbolAddress s, Agg.symbolSize s) | s <- symbols, Agg.symbolSize s > 0] ++ kept
        cuts = sort (nub (concat [[at, at + size] | (at, size) <- spans]))
     in [(lo, hi - lo) | (lo,hi) <- zip cuts (drop 1 cuts), any (\(at,size) -> at <= lo && hi <= at + size) spans]
  Aggregate _ ->
    let base = timerEntry domain - 0x1e8
     in extents domain ++ retained domain ++ [(base, 0xc4), (base + 0xc4, 0xdc), (timerEntry domain, 0xe4)]
  Standalone _ ->
    let base = timerEntry domain - 0x1e8
     in extents domain ++ retained domain ++ [(base, 0xc4), (base + 0xc4, 0xdc), (timerEntry domain, fromIntegral (B.length timerStub))]

caseFor :: Facts -> Domain -> ObservationRole -> Image -> Payload -> Case -> CaseSpec
caseFor facts executing role loader payload one
  | not (null outside) = error (caseIdText (caseName one) ++ ": unmapped fixture words " ++ show outside)
  | otherwise = CaseSpec
      { caseId = emittedId executing one
      , caseProvenance = "X-M1 1.50 F2 stock ABI; synthetic task/object state"
      , caseExclusion =
          "Private compiler frames are not byte-compared. Dispatch checks selected-task arrival and the live S1 continuation, not the original service or receiver return. Handler callbacks enter SVC, not IRQ mode. Standalone timer receipts prove arguments; aggregate rows execute timer removal."
            ++ case payload of
              Aggregate _ -> " Extents outside the carrier union execute retained stock loader bytes."
              Standalone _ -> ""
      , caseEntry = wireAddress (GuestAddress entry)
      , caseExit = wireAddress (GuestAddress exitAddress)
      , caseRegions =
          codeRegions
            ++ [mkRegion ident (wireAddress address) size permission (overlay address background words') | Page ident address size permission background <- dataPages]
            ++ [mkRegion (RegionId "continuations") (wireAddress (GuestAddress exitAddress)) (fromIntegral (stopAddress - exitAddress + 4)) ReadExecute (continuationBytes facts executing one)]
      , caseFunctions =
          [(wireAddress (GuestAddress address), size) | (address, size) <- functionSpans executing role payload]
            ++ [(wireAddress (GuestAddress wakerStubBase), stubSize) | Just _ <- [caseWaker one]]
      , caseInitial = initialRegisters facts executing one
      , caseExpectedStatus = if caseBudget == Nothing then (if caseBoundary one == CallerReturn then Returned else StopAddress) else InstructionBudget
      , caseExpectedRegisters = expectedRegisters
      , caseExpectedMemory = [(RegionId "caller-frame", callerBytes one)]
      , caseIgnoredMemory = []
      , caseCp14Identity = Just (if executing == DomainA then 0x4003 else 0x4004)
      , caseKernel = Just (KernelCase (maybe plainLimit (InstructionLimit . fromIntegral) caseBudget) (if caseBoundary one == TaskArrival then Just (wireAddress (GuestAddress stopAddress)) else Nothing) (Just 0x80001234) (Just 0) (Just 0) ignored stacks)
      }
  where
    entry = callEntry executing (caseCall one)
    (codeBase, codeTop) = codeBounds executing payload
    installed = codeBytes executing role loader payload
    readonly = case (role, payload) of
      (OwnedObservation, Aggregate aggregate) ->
        [(Agg.placementAddress p, Agg.placementAddress p + Agg.placementBytes p) | p <- aggregatePlacements aggregate, Agg.placementBytes p > 0, not (Agg.placementSection p == ".text" || ".text." `isPrefixOf` Agg.placementSection p)]
      _ -> []
    cuts = sort (nub (codeBase : codeTop : concat [[lo,hi] | (lo,hi) <- readonly]))
    codeRegions =
      [mkRegion (RegionId ("code-" ++ show index)) (wireAddress (GuestAddress lo)) (fromIntegral (hi - lo))
        (if any (\(at,end) -> at <= lo && hi <= end) readonly then ReadOnly else ReadExecute)
        (B.take (fromIntegral (hi-lo)) (B.drop (fromIntegral (lo-codeBase)) installed))
      | (index,(lo,hi)) <- zip [0 :: Int ..] (zip cuts (drop 1 cuts)), hi > lo]
    dataPages = pages facts one
    words' = layoutWords facts one ++ [poke (dfHandlerContext (domainFacts facts executing)) (if caseInvocation one `elem` [HandlerCallback, HandlerContext] then 1 else 0)] ++ caseWords one
    outside = [address | (address, _) <- words', not (any (contains address 4) dataPages)]
    caseBudget = case (role, caseStockBudget one) of
      (StockObservation, Just limit) -> Just limit
      _ -> Nothing
    expectedRegisters
      | caseBudget /= Nothing = [(R0, 0)]
      | caseBoundary one == CallerReturn =
          [(R0, value) | Just value <- [expectedResult one]] ++ [(register, seed register) | register <- [R4 .. R11]] ++ [(Sp, callerSp one)]
      | otherwise = [(Pc, stopAddress), (Sp, targetStackPointer)]
    ignored
      | caseBudget /= Nothing = [R0 .. R12] ++ [Lr]
      | caseBoundary one == CallerReturn = [R1,R2,R3,R12,Lr]
      | otherwise = [R0 .. R12] ++ [Lr]
    stacks = case caseWaker one of
      Nothing -> []
      Just _ -> map stack [(callerSp one - 0x400, callerSp one), (wakerStack - 0x400, wakerStack)]
    stack (low, high) = either error id (mkStackContext (N.GuestAddress (fromIntegral low)) (N.GuestAddress (fromIntegral high)))

stubSize :: Word32
stubSize = 0x80

-- | The caller's continuation window with the waker task's stub, when the case has one.
continuationBytes :: Facts -> Domain -> Case -> B.ByteString
continuationBytes facts executing one = case caseWaker one of
  Nothing -> image
  Just waker -> installAt wakerStubBase (B.concat (map word32le (stubWords facts executing waker))) image
  where
    size = fromIntegral (stopAddress - exitAddress + 4)
    image = B.replicate size 0
    installAt address bytes current =
      let offset = fromIntegral (address - exitAddress)
       in B.take offset current <> bytes <> B.drop (offset + B.length bytes) current

data StubInstruction = StubWord Word32 | StubBranchLink Word32

-- | The waker runs on its own stack: it records the caller's dispatch CPSR, masks I, calls the
-- service, restores the driver mask and re-enters the dispatcher, which resumes the caller.
stubWords :: Facts -> Domain -> Waker -> [Word32]
stubWords facts executing waker = assembleStub wakerStubBase (prologue ++ arguments ++ calls)
  where
    prologue =
      [ StubWord 0xe10fc000
      , StubWord (movw 3 (maskAddress .&. 0xffff))
      , StubWord (movt 3 (maskAddress `shiftR` 16))
      , StubWord 0xe583c000
      , StubWord 0xf1080080
      ]
        ++ case wakerTarget waker of
          WakerTick _ ->
            [ StubWord (movw 3 (handler .&. 0xffff))
            , StubWord (movt 3 (handler `shiftR` 16))
            , StubWord 0xe3a00001
            , StubWord 0xe5830000
            ]
          _ -> []
    handler = dfHandlerContext (domainFacts facts executing)
    arguments = concatMap argumentWord (wakerArguments waker)
    argumentWord (register, value) =
      [ StubWord (movw (registerNumber register) (value .&. 0xffff))
      , StubWord (movt (registerNumber register) (value `shiftR` 16))
      ]
    calls = case wakerTarget waker of
      WakerTick count -> replicate count (StubBranchLink (tickEntry executing)) ++ handlerTail
      WakerSet -> [StubBranchLink (entryAddress executing SetFlag)] ++ driverTail
      WakerDelete -> [StubBranchLink (entryAddress executing Delete)] ++ driverTail
    handlerTail =
      [ StubWord (movw 3 (handler .&. 0xffff))
      , StubWord (movt 3 (handler `shiftR` 16))
      , StubWord 0xe3a00000
      , StubWord 0xe5830000
      ] ++ driverTail
    driverTail =
      [ StubWord 0xf10c0080
      , StubWord 0xe10fc000
      , StubWord (movw 3 (dispatchMaskAddress .&. 0xffff))
      , StubWord (movt 3 (dispatchMaskAddress `shiftR` 16))
      , StubWord 0xe583c000
      , StubBranchLink (dispatcherEntry executing)
      , StubWord 0xeafffffe
      ]

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

registerNumber :: Register -> Word32
registerNumber register = case register of
  R0 -> 0
  R1 -> 1
  R2 -> 2
  R3 -> 3
  _ -> error "a waker argument is not in r0-r3"

seed :: Register -> Word32
seed register = 0xa5a50000 + fromIntegral (fromEnum register)

expectedResult :: Case -> Maybe Word32
expectedResult one = case [value | Result value <- caseExpect one] of
  [] -> Nothing
  [value] -> Just value
  _ -> error "return case states more than one ABI result"

initialRegisters :: Facts -> Domain -> Case -> [(Register, Word32)]
initialRegisters facts executing one =
  [(register, value register) | register <- [R0 .. R12]] ++ [(Sp, callerSp one), (Lr, exitAddress), (Cpsr, if caseInvocation one == HandlerCallback then 0x93 else 0x13)]
  where
    value R0 = case caseCall one of
      ObjectService _ owner -> (dfId (domainFacts facts owner) `shiftL` 16) .|. 1
      OwnerRelease owner -> recordAddress facts executing owner
      HelperEntry helper -> case helper of
        WaitRelease owner -> recordAddress facts executing owner
        WaitBlock owner -> dfId (domainFacts facts owner)
        WaitBlockTimed owner -> dfId (domainFacts facts owner)
        Check _ -> flagControl facts executing 1
      DirectEntry direct -> (dfId (domainFacts facts (directDomain direct)) `shiftL` 16) .|. 1
    value register = maybe (seed register) id (lookup register (caseArguments one))

emittedId :: Domain -> Case -> String
emittedId executing one = caseIdText (caseName one) ++ "-" ++ domainLabel executing

readWord :: Facts -> Case -> NativeObservation -> GuestAddress -> Maybe Word32
readWord facts one observation address@(GuestAddress raw) = case
  [(base, bytes) | page@(Page ident (GuestAddress base) _ _ _) <- pages facts one, contains address 4 page, Just bytes <- [lookup ident (nativeMemory observation)]] of
  (base, bytes) : _ -> Just (readWordAt bytes (fromIntegral (raw - base)))
  [] -> Nothing

readByte :: Facts -> Case -> NativeObservation -> GuestAddress -> Maybe Word8
readByte facts one observation address@(GuestAddress raw) = case
  [(base, bytes) | page@(Page ident (GuestAddress base) _ _ _) <- pages facts one, contains address 1 page, Just bytes <- [lookup ident (nativeMemory observation)]] of
  (base, bytes) : _ -> Just (B.index bytes (fromIntegral (raw - base)))
  [] -> Nothing

compareFamily :: Facts -> Domain -> Payload -> Case -> CaseSpec -> NativeObservation -> NativeObservation -> [String]
compareFamily facts executing payload one description stock owned =
  checks StockObservation stock ++ checks OwnedObservation owned ++ roleDifference
  where
    roleName StockObservation = "stock"
    roleName OwnedObservation = "owned"
    checks role observation = map ((roleName role ++ ": ") ++) (exitChecks role observation ++ expectedChecks role observation ++ registerChecks role observation ++ continuationChecks role observation ++ effectChecks role observation ++ timerIndexErrors facts one role observation)
    budgeted role = case (role, caseStockBudget one) of
      (StockObservation, Just limit) -> Just limit
      _ -> Nothing
    wantedOutcome role observation = case budgeted role of
      Just limit -> OutcomeNoReturnWithin (InstructionLimit (fromIntegral limit)) (nativeKernel observation >>= observedReserved)
      Nothing -> if caseBoundary one == CallerReturn then OutcomeReturned else OutcomeStopped (wireAddress (GuestAddress stopAddress))
    exitChecks role observation = ["wrong exit " ++ show (caseOutcome description observation) | caseOutcome description observation /= wantedOutcome role observation]
    expectedChecks role observation = ["missing effect " ++ show expectation | expectation <- caseExpect one ++ (if role == OwnedObservation then caseOwnedExpect one else []), not (holds observation expectation)]
    holds observation (Equals address value) = readWord facts one observation address == Just value
    holds observation (ByteEquals address value) = readByte facts one observation address == Just value
    holds observation (Result value) = N.lookupRegister R0 observation == Just value
    registerChecks role observation
      | budgeted role /= Nothing = ["caller frame changed" | lookup (RegionId "caller-frame") (nativeMemory observation) /= Just (callerBytes one)]
    registerChecks _ observation =
      ["caller frame changed" | lookup (RegionId "caller-frame") (nativeMemory observation) /= Just (callerBytes one)]
        ++ ["CPSR control changed" | fmap (.&. 0x1ff) (N.lookupRegister Cpsr observation) /= Just expectedControl]
        ++ if caseBoundary one == CallerReturn
          then ["caller register " ++ show register ++ " changed" | (register, value) <- [(r, initial r) | r <- [R4 .. R11]] ++ [(Sp, callerSp one)], N.lookupRegister register observation /= Just value]
          else ["selected-task register " ++ show register ++ " differs" | (register, value) <- [(Sp,targetStackPointer),(Pc,stopAddress)], N.lookupRegister register observation /= Just value]
            ++ ["selected thread pointer differs" | (nativeKernel observation >>= observedTpidrurw) /= selectedThread observation]
            ++ ["selected-task FPEXC differs" | (nativeKernel observation >>= observedFpexc) /= Just 0]
    expectedControl = if caseBoundary one == TaskArrival || caseInvocation one == HandlerCallback then 0x93 else 0x13
    initial register = maybe (seed register) id (lookup register (caseArguments one))
    selectedThread observation = do
      selected <- readWord facts one observation (GuestAddress (dfSelected (domainFacts facts executing)))
      definition <- readWord facts one observation (GuestAddress (selected + factsTaskDefinition facts))
      readWord facts one observation (GuestAddress (definition + 0x20))
    continuationChecks role observation
      | caseBoundary one == CallerReturn = []
      | otherwise = case readWord facts one observation (GuestAddress savedSpAddress) of
          Nothing -> ["missing suspended caller SP"]
          Just saved ->
            ["suspended caller frame is outside its live stack" | saved < callerSp one - 0x400 || toInteger saved + 36 > toInteger (callerSp one) || saved .&. 3 /= 0]
              ++ ["suspended caller resume entry differs" | readWord facts one observation (GuestAddress savedPcAddress) /= expectedResume role]
              ++ ["suspended callee-saved register " ++ show register ++ " differs" | (offset,register) <- zip [12,16 .. 28] [R7 .. R11], readWord facts one observation (GuestAddress (saved+offset)) /= N.lookupRegister register observation]
              ++ ["suspended continuation is not in the caller's code" | maybe True (not . inOwnedCode role) (readWord facts one observation (GuestAddress (saved+32)))]
    savedSpAddress = taskAt facts executing runningSlot + factsTaskSavedSp facts
    savedPcAddress = taskAt facts executing runningSlot + factsTaskResumePc facts
    expectedResume OwnedObservation | Aggregate aggregate <- payload =
      case [Agg.symbolAddress symbol | symbol <- aggregateSymbols aggregate, Agg.symbolName symbol == "xm1_rtos_scheduler_resume", Agg.symbolSize symbol > 0] of
        [address] -> Just address
        _ -> Nothing
    expectedResume _ = Just (resumeEntry executing)
    inOwnedCode role address = any (\(base,size) -> base <= address && address < base+size) (functionSpans executing role payload)
    compared = [ident | Page ident _ _ _ _ <- pages facts one, ident `notElem` map RegionId (["stack","target-stack","waker-stack"] ++ ["timer-seam" | Just _ <- [caseWaker one]])]
    memory observation = M.fromList [(ident, normalizeTimer ident (normalize ident bytes)) | (ident,bytes) <- nativeMemory observation, ident `elem` compared]
    normalize ident bytes
      | caseBoundary one == TaskArrival && ident == region =
          let offset = offsetOf savedSpAddress
              resumeOffset = offsetOf savedPcAddress
              withoutSp = B.take offset bytes <> word32le 0 <> B.drop (offset+4) bytes
           in B.take resumeOffset withoutSp <> word32le 0 <> B.drop (resumeOffset+4) withoutSp
      | Just waker <- caseWaker one, ident == region =
          -- Declared difference, timer index: see replacement/rtos.md, Declared differences.
          foldl (\current address -> clear current (offsetOf address)) bytes
            ([ savedSpAddress, savedPcAddress
             , taskAt facts executing wakerSlot + factsTaskSavedSp facts
             , taskAt facts executing wakerSlot + factsTaskResumePc facts
             ] ++ [dfFlagTimers (domainFacts facts executing) | WakerTick _ <- [wakerTarget waker]])
      | otherwise = bytes
    region = RegionId (if executing == DomainA then "kernel-a" else "kernel-b")
    (GuestAddress kernelBase, _) = kernelBounds facts executing
    offsetOf address = fromIntegral (address - kernelBase)
    clear bytes offset = B.take offset bytes <> word32le 0 <> B.drop (offset + 4) bytes
    normalizeTimer ident bytes = case caseTimerIndex one of
      Nothing -> bytes
      Just address -> case [fromIntegral (pointerWord address - pointerWord pageBase) | page@(Page actual pageBase _ _ _) <- pages facts one, ident == actual, contains address 4 page] of
        [offset] -> B.take offset bytes <> word32le 0 <> B.drop (offset+4) bytes
        _ -> bytes
    memoryDifferences =
      ["missing state region " ++ show ident | ident <- compared, M.notMember ident (memory stock) || M.notMember ident (memory owned)]
        ++ ["state region " ++ show ident ++ " differs" | ident <- compared, M.lookup ident (memory stock) /= M.lookup ident (memory owned)]
    -- A declared difference compares each role with its own expectation; the owned role must then
    -- differ from the stock expectation, and the two final states are not compared with each other.
    roleDifference = case caseStockBudget one of
      Nothing -> memoryDifferences
      Just _ -> ["the owned role did not differ from the stock expectation" | null (exitChecks StockObservation owned)]
    effectChecks role observation = case nativeKernel observation >>= observedOrder of
      Nothing -> ["ordered provenance is missing or truncated"]
      Just events -> lockChecks events ++ notificationChecks role events ++ timerChecks events
    writes events address = [(index,fromIntegral value :: Word32) | (index,OrderedWrite (Write actual _) (WriteValue value)) <- zip [0::Int ..] events, actual == wireAddress address]
    privateRegions =
      [(toInteger base,toInteger base + toInteger size) | Page ident (GuestAddress base) size _ _ <- pages facts one, ident `elem` map RegionId ["stack","caller-frame","target-stack"]]
    privateWrite (Write (N.GuestAddress address) (N.WriteBytes size)) =
      any (\(lo,hi) -> lo <= toInteger address && toInteger address + toInteger size <= hi) privateRegions
    advanceFence state event = case event of
      OrderedBarrier Dmb -> case state of
        AwaitAcquireFence -> AwaitReleaseFence
        AwaitReleaseFence -> PublishedLock
        _ -> state
      OrderedWrite write _ | not (privateWrite write) -> case state of
        AwaitAcquireFence -> InvalidLockOrder
        InvalidLockOrder -> InvalidLockOrder
        _ -> AwaitReleaseFence
      _ -> state
    lockInterval events address = do
      acquired <- fst <$> first ((==1) . snd) (writes events address)
      released <- fst <$> first ((==0) . snd) (writes events address)
      let finalFence = foldl' (\state (index,event) -> if acquired < index && index < released then advanceFence state event else state) AwaitAcquireFence (zip [0::Int ..] events)
      if acquired < released && finalFence == PublishedLock then Just (acquired,released) else Nothing
    lockChecks events = case caseCall one of
      OwnerRelease _ -> []
      HelperEntry _ -> []
      ObjectService family objectDomain
        | family `elem` [Create, Delete] -> []
        | otherwise -> objectInterval objectDomain ++ nesting objectDomain
      DirectEntry direct -> objectInterval (directDomain direct)
      where
        objectInterval objectDomain =
          let objectLock = GuestAddress (dfObjectLock (domainFacts facts objectDomain))
           in ["object lock or its release barrier differs" | lockInterval events objectLock == Nothing]
        nesting objectDomain =
          let objectLock = GuestAddress (dfObjectLock (domainFacts facts objectDomain))
              scheduler = case caseWake one of NoWaiter -> Nothing; Unsatisfied owner -> Just owner; Released owner -> Just owner
           in case scheduler of
                Nothing -> []
                Just owner -> case (lockInterval events objectLock, lockInterval events (GuestAddress (dfSchedulerLock (domainFacts facts owner)))) of
                  (Just (oa,oz), Just (sa,sz)) | oa < sa && sz < oz -> []
                  _ -> ["waiter scheduler lock is not nested inside the object lock"]
    notificationChecks role events =
      let target = case (caseCall one,caseWake one) of (ObjectService _ _,Released owner) | owner /= executing -> Just owner; _ -> Nothing
          expected domain = if target == Just domain then [0x01000000] else []
          bankChecks = ["notification bank " ++ show domain ++ " differs" | domain <- [DomainA,DomainB], map snd (writes events (bank domain)) /= expected domain]
       in bankChecks ++ case target of
          Nothing -> []
          Just owner -> case (writes events (bank owner), lockInterval events (GuestAddress (dfSchedulerLock (domainFacts facts owner)))) of
            ([(index,_)], Just (acquired,released)) ->
              ["notification is outside the waiter scheduler lock" | not (acquired < index && index < released)]
                ++ ["notification has no completion DSB" | not (any (\(after,event) -> released < after && event == OrderedBarrier Dsb) (zip [0::Int ..] events))]
                ++ ["owned notification has no preceding DSB" | role == OwnedObservation, not (any (\(before,event) -> before < index && event == OrderedBarrier Dsb) (zip [0::Int ..] events))]
            _ -> ["notification or waiter scheduler lock is missing"]
    timerChecks events = case caseTimer one of
      Nothing -> ["unexpected timer removal" | not (null (writes events (GuestAddress receiptAddress))) || not (null (writes events (GuestAddress (receiptAddress+4))))]
      Just token ->
        let owner = case caseWake one of Released domain -> domain; _ -> error "timer case without owner"
            expectedOwner = recordAddress facts executing owner
         in ["timer removal arguments differ" | map snd (writes events (GuestAddress receiptAddress)) /= [expectedOwner] || map snd (writes events (GuestAddress (receiptAddress+4))) /= [pointerWord token]]
    first predicate values = case filter predicate values of value:_ -> Just value; [] -> Nothing

timerIndexErrors :: Facts -> Case -> ObservationRole -> NativeObservation -> [String]
timerIndexErrors facts one role observation = case caseTimerIndex one of
  Nothing -> []
  Just address ->
    let expected = if role == OwnedObservation then 0 else 1
        expectedWrites = [(4,0) | role == OwnedObservation]
        writes = [(width,fromIntegral value :: Word32)
          | Just kernel <- [nativeKernel observation], Just events <- [observedOrder kernel]
          , OrderedWrite (Write actual (N.WriteBytes width)) (WriteValue value) <- events, actual == wireAddress address]
     in ["timer index violates the named Detached-index contract" | readWord facts one observation address /= Just expected || writes /= expectedWrites]

mutateWord :: Facts -> Case -> GuestAddress -> Word32 -> NativeObservation -> NativeObservation
mutateWord facts one address value observation = observation
  { nativeMemory = [(ident, case [base | page@(Page actual base _ _ _) <- pages facts one, ident == actual, contains address 4 page] of
      [base] -> let offset = fromIntegral (pointerWord address - pointerWord base)
                 in B.take offset bytes <> word32le value <> B.drop (offset+4) bytes
      _ -> bytes) | (ident,bytes) <- nativeMemory observation]
  }

semanticControls :: Facts -> Domain -> Payload -> Case -> CaseSpec -> NativeObservation -> NativeObservation -> [(String, Either String [String])]
semanticControls facts domain payload one spec stock owned =
  let check name changed = (caseIdText (caseName one) ++ "-" ++ name,
        Right ["semantic mutation was accepted" | null (compareFamily facts domain payload one spec stock changed)])
      state = taskAt facts domain waiterSlot + factsTaskState facts
      baseControls
        | Just _ <- caseStockBudget one =
            [ check "wait-result-refused" (mutateWord facts one (GuestAddress (flagWait facts domain 1 + factsWaitBlockResult facts)) 0 owned)
            , check "running-state-refused" (mutateWord facts one (GuestAddress (taskAt facts domain runningSlot + factsTaskState facts)) 0x32 owned)
            , check "queue-head-refused" (mutateWord facts one (GuestAddress (flagControl facts domain 1)) 0 owned)
            ]
        | otherwise =
            [check "caller-frame-refused" (mutateWord facts one (GuestAddress (callerSp one)) 0xdeadbeef owned),
             check "device-write-refused" (mutateWord facts one (bank domain) 0xffffffff owned),
             check "task-state-refused" (mutateWord facts one (GuestAddress state) 0xffffffff owned),
             check "caller-message-refused" (mutateWord facts one (GuestAddress 0x40000000) 0xffffffff owned)]
              ++ [check "saved-resume-refused" (mutateWord facts one (GuestAddress (taskAt facts domain runningSlot + factsTaskResumePc facts)) 0 owned) | caseBoundary one == TaskArrival]
   in baseControls ++ case caseTimerIndex one of
      Nothing -> []
      Just address ->
        [check "detached-index-refused" (mutateWord facts one address 1 owned),
         (caseIdText (caseName one) ++ "-stock-index-refused",
           Right ["stock timer-index mutation was accepted" | null (compareFamily facts domain payload one spec (mutateWord facts one address 0 stock) owned)]),
         check "timer-effect-refused" (owned {nativeKernel = fmap (\kernel -> kernel {observedOrder = fmap (++ [OrderedWrite (Write (wireAddress address) (N.WriteBytes 4)) (WriteValue 0)]) (observedOrder kernel)}) (nativeKernel owned)}),
         check "timer-missing-write-refused" (owned {nativeKernel = fmap (\kernel -> kernel {observedOrder = fmap (filter (\event -> case event of OrderedWrite (Write actual _) _ -> actual /= wireAddress address; _ -> True)) (observedOrder kernel)}) (nativeKernel owned)})]

storageGuards :: Payload -> CaseSpec -> [CaseSpec]
storageGuards (Standalone _) _ = []
storageGuards (Aggregate aggregate) spec =
  let probeAddress = 0x00957000
      readonly = [Agg.placementAddress p | p <- aggregatePlacements aggregate, Agg.placementBytes p >= 4, not (Agg.placementSection p == ".text" || ".text." `isPrefixOf` Agg.placementSection p)]
      targets = [("code-write-refused", Agg.imageBase (aggregateImage aggregate), False)]
        ++ [(name,address,fetch) | address <- take 1 readonly, (name,fetch) <- [("readonly-write-refused",False),("readonly-fetch-refused",True)]]
      probe (name,address,fetch) =
        let bytes = assemble (if fetch then [0xe12fff11] else [0xe5810000,0xe12fff1e])
         in spec
          { caseId = caseId spec ++ "-" ++ name
          , caseEntry = wireAddress (GuestAddress probeAddress)
          , caseRegions = mkRegion (RegionId "storage-probe") (wireAddress (GuestAddress probeAddress)) (B.length bytes) ReadExecute bytes : caseRegions spec
          , caseFunctions = (wireAddress (GuestAddress probeAddress),fromIntegral (B.length bytes)) : caseFunctions spec
          , caseInitial = (R1,address) : filter ((/= R1) . fst) (caseInitial spec)
          , caseExpectedStatus = if fetch then FetchGuard else MemoryGuard
          , caseExpectedRegisters = [(R1,address), (Pc,if fetch then address else probeAddress)]
          , caseExpectedMemory = []
          }
   in map probe targets
storageGuardResult :: CaseSpec -> NativeObservation -> [String]
storageGuardResult spec observation =
  ["readonly storage access was not refused" | caseOutcome spec observation /= OutcomeRefused (caseExpectedStatus spec)]

runFamily :: FilePath -> FilePath -> FilePath -> Facts -> Domain -> Image -> Payload -> IO FamilyRun
runFamily runner cacheRoot directory facts executing stock owned = do
  let selected = map (bindTimer facts executing owned) (cases facts executing)
      stockPayload = case owned of Standalone _ -> Standalone stock; _ -> owned
      stockCases = [(one,caseFor facts executing StockObservation stock stockPayload one) | one <- selected]
      ownedCases = [(one,caseFor facts executing OwnedObservation stock owned one) | one <- selected]
  stockBatch <- writeBatch (directory </> "stock") (map snd stockCases)
  ownedBatch <- writeBatch (directory </> "owned") (map snd ownedCases)
  (stockReuse, stockRaw) <- cachedObservedBatchWith WriteAllResults cacheRoot runner stockBatch
  (ownedReuse, ownedRaw) <- cachedObservedBatchWith WriteAllResults cacheRoot runner ownedBatch
  let reference = M.fromList [(caseIdText ident,observation) | (ident,observation) <- stockRaw]
      subject = M.fromList [(caseIdText ident,observation) | (ident,observation) <- ownedRaw]
  results <- forM stockCases $ \(one,description) -> do
    let ident = emittedId executing one
    pure (caseIdText (caseName one),case (M.lookup ident reference,M.lookup ident subject) of
      (Just original,Just replacement) -> Right (compareFamily facts executing owned one description original replacement)
      (Nothing,_) -> Left "stock case did not run"
      (_,Nothing) -> Left "owned case did not run")
  let guardSpecs = case ownedCases of
        (_,description):_ -> storageGuards owned description
        [] -> []
  guardResults <- if null guardSpecs then pure [] else do
    guardBatch <- writeBatch (directory </> "storage-guards") guardSpecs
    (_, observations) <- cachedObservedBatchWith WriteAllResults cacheRoot runner guardBatch
    pure [(caseId spec, case lookup (CaseId (caseId spec)) observations of
      Nothing -> Left "storage guard did not run"
      Just observation -> Right (storageGuardResult spec observation)) | spec <- guardSpecs]
  unless (length stockRaw == length selected && length ownedRaw == length selected) (fail "native observations do not cover the current F2 case set")
  let controls = concat
        [semanticControls facts executing owned one description original replacement
        | (one,description) <- stockCases
        , Just original <- [M.lookup (emittedId executing one) reference]
        , Just replacement <- [M.lookup (emittedId executing one) subject]]
      controlRows = [(name, result == Right []) | (name, result) <- controls]
      stockInstructions =
        [ (caseIdText (caseName one), instructionCount (nativeInstructions original))
        | (one, _) <- stockCases
        , Just original <- [M.lookup (emittedId executing one) reference]
        ]
      allResults = results ++ controls ++ guardResults
  evidence <- forM stockCases $ \(one, _) -> do
    let ident = emittedId executing one
        entry = callEntry executing (caseCall one)
        size = entryExtent executing entry
    found <- evidenceAt (takeDirectory stockBatch </> (ident ++ ".native-result.json")) entry size
    pure (caseIdText (caseName one), (entry, size, evidenceTrace found + evidenceProvenance found))
  pure (FamilyRun executing results guardResults (length [() | (_,Right []) <- allResults]) (stockReuse,ownedReuse) controlRows stockInstructions evidence)
