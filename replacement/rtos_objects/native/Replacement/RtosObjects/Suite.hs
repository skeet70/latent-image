{-# LANGUAGE OverloadedStrings #-}
module Replacement.RtosObjects.Suite
  ( Image (..), FamilyRun (..), extents, runFamily ) where

import Control.Monad (forM, unless)
import Data.Bits ((.&.), (.|.), shiftL)
import qualified Data.ByteString as B
import qualified Data.Map.Strict as M
import Data.Word (Word32, Word8)
import Emulation.CachedNativeBatch (BatchReuse, cachedObservedBatch)
import Emulation.NativeCase (CaseId (..), caseIdText)
import qualified Emulation.NativeObservation as N
import Emulation.NativeObservation (Barrier (..), KernelObservation (..), NativeObservation (..), Ordered (..), RegionId (..), Register (..), RunnerStatus (..), Write (..), WriteValue (..))
import Emulation.OwnedCase (CaseOutcome (..), CaseSpec (..), KernelCase (..), Permission (..), assemble, caseOutcome, mkRegion, plainLimit, readWordAt, word32le, writeBatch)
import Replacement.RtosObjects.Layout hiding (record)
import Replacement.RtosOwnFirst.Regions (Domain (..), GuestAddress (..), domainLabel)
import System.FilePath ((</>))

newtype Slot = Slot Word32 deriving (Eq, Show)
data Family = SetFlag | Send deriving (Eq, Show)
data Call = ObjectService Family Domain | OwnerRelease Domain deriving (Eq, Show)
data Invocation = TaskInvocation | HandlerCallback deriving (Eq, Show)
data Boundary = CallerReturn | TaskArrival deriving (Eq, Show)
data WakeOutcome = NoWaiter | Unsatisfied Domain | Released Domain deriving (Eq, Show)
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
  }

data Image = Image { imageBase :: Word32, imageBytes :: B.ByteString }
data Page = Page RegionId GuestAddress Int Permission B.ByteString

data FamilyRun = FamilyRun
  { familyDomain :: Domain
  , familyCases :: [(String, Either String [String])]
  , familyMatches :: Int
  , familyReuse :: (BatchReuse, BatchReuse)
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

entryAddress :: Domain -> Family -> Word32
entryAddress DomainA SetFlag = 0x4813c
entryAddress DomainA Send = 0x4aa44
entryAddress DomainB SetFlag = 0xd80fc
entryAddress DomainB Send = 0xdaa04

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
  , (0x44044, 0xf0), (0x44134, 0x80), (0x44568, 0xcc), (0x4813c, 0x260), (0x4aa44, 0x304)
  ]
extents DomainB =
  [ (0xd0b00, 0x24), (0xd3640, 0x28), (0xd3668, 0x20), (0xd3688, 0xa0)
  , (0xd3728, 0x34), (0xd375c, 0x48), (0xd37a4, 0x38), (0xd37fc, 0x14)
  , (0xd398c, 0x3c), (0xd39c8, 0x28), (0xd3c04, 0x5c), (0xd3c60, 0x7c)
  , (0xd3cdc, 0xb0), (0xd3d8c, 0x48), (0xd3f2c, 0x2c), (0xd3fd4, 0x30)
  , (0xd4004, 0xf0), (0xd40f4, 0x80), (0xd4528, 0xcc), (0xd80fc, 0x260), (0xdaa04, 0x304)
  ]

retained :: Domain -> [(Word32, Word32)]
retained DomainA = [(0x480cc, 0x70), (0x4252c, 0x28), (0x433e4, 0x60), (0x43444, 0x60)]
retained DomainB = [(0xd808c, 0x70), (0xd252c, 0x28), (0xd33e4, 0x60), (0xd3444, 0x60)]

recordAddress :: Facts -> Domain -> Domain -> Word32
recordAddress facts executing described = case described of
  DomainA -> dfContextA (domainFacts facts executing)
  DomainB -> dfContextB (domainFacts facts executing)

definitionAddress :: Facts -> Domain -> Slot -> Word32
definitionAddress facts domain slot = definitionBase + (index * factsTaskSlots facts + slotWord slot - 1) * definitionStride
  where index = if domain == DomainA then 0 else 1

taskAt :: Facts -> Domain -> Slot -> Word32
taskAt facts domain = task facts domain . slotWord

threadValue :: Domain -> Slot -> Word32
threadValue domain slot = ((if domain == DomainA then 1 else 2) `shiftL` 16) .|. slotWord slot

packedState :: Word8 -> Word32 -> Word32
packedState state priority = fromIntegral state .|. (priority `shiftL` 8)

poke :: Word32 -> Word32 -> (GuestAddress, Word32)
poke address value = (GuestAddress address, value)

cases :: Facts -> Domain -> [Case]
cases facts executing =
  [ service "set-no-waiter" SetFlag executing NoWaiter CallerReturn [] [Result 0, eq patternCell 1]
  , service "set-satisfied" SetFlag executing (Released executing) TaskArrival (flagWaiter executing executing 1) [eq reportCell 1, ready executing]
  , service "set-unsatisfied" SetFlag executing (Unsatisfied executing) CallerReturn (flagWaiter executing executing 2) [Result 0, eq patternCell 1, state executing queued]
  , service "set-clear-on-match" SetFlag executing (Released executing) TaskArrival (flagWaiter executing executing 1 ++ [poke (flag + factsFlagConfigAttributes facts) (factsFlagClearOnMatch facts)]) [eq patternCell 0, eq reportCell 1]
  , service "set-waiter-peer" SetFlag executing (Released remote) CallerReturn (flagWaiter remote executing 1)
      [Result 0, ready remote, eq (dfSelected remoteWords) remoteTask, Equals (bank remote) 0x01000000, Equals (bank executing) 0]
  , service "set-object-peer" SetFlag remote (Released executing) TaskArrival (flagWaiter executing remote 1) [ready executing, eq (dfSelected ownWords) localTask, Equals (bank remote) 0, Equals (bank executing) 0]
  , withTimer (service "set-timer-cancel" SetFlag executing (Released executing) TaskArrival (flagWaiter executing executing 1) [ready executing])
  , service "send-blocked" Send executing (Released executing) TaskArrival (mailWaiter executing ++ [poke message sentinel]) [eq mailResult message, eq message sentinel]
  , service "send-empty-fifo" Send executing NoWaiter CallerReturn [poke message sentinel] [Result 0, eq (mailControl + factsMailboxControlHead facts) message, eq (mailControl + factsMailboxControlTail facts) message, eq message 0]
  , service "send-priority-equal" Send executing NoWaiter CallerReturn
      [ poke (mailbox + factsMailboxConfigAttributes facts) (factsMailboxMessagePriority facts)
      , poke (mailControl + factsMailboxControlHead facts) oldMessage
      , poke (mailControl + factsMailboxControlTail facts) oldMessage
      , poke oldMessage 0, poke (oldMessage + 4) 5, poke message sentinel, poke (message + 4) 5
      ] [Result 0, eq oldMessage message, eq message 0, eq (mailControl + factsMailboxControlTail facts) oldMessage]
  , service "send-waiter-peer" Send executing (Released remote) CallerReturn (mailWaiter remote) [Result 0, ready remote, eq (mailboxWait facts remote 2 + factsMailboxWaitMessage facts) message, Equals (bank remote) 0x01000000, Equals (bank executing) 0]
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
  , service "set-satisfied-no-dispatch" SetFlag executing (Released executing) CallerReturn (flagWaiter executing executing 1 ++ noDispatch) [Result 0, ready executing, eq reportCell 1]
  , service "send-blocked-no-dispatch" Send executing (Released executing) CallerReturn (mailWaiter executing ++ [poke message sentinel] ++ noDispatch) [Result 0, ready executing, eq mailResult message, eq message sentinel]
  ]
  where
    ownWords = domainFacts facts executing
    remote = peer executing
    remoteWords = domainFacts facts remote
    localTask = taskAt facts executing waiterSlot
    remoteTask = taskAt facts remote waiterSlot
    flag = flagConfig facts executing 1
    patternCell = flagControl facts executing 1 + factsFlagControlPattern facts
    reportCell = flagWait facts executing 2 + factsFlagWaitResult facts
    mailbox = mailboxConfig facts executing 1
    mailControl = mailboxControl facts executing 1
    mailResult = mailboxWait facts executing 2 + factsMailboxWaitMessage facts
    message = 0x40000000
    oldMessage = message + 8
    sentinel = 0x5e4d1e00
    queued = factsStateWait facts .|. factsStateObjectQueued facts
    noDispatch = [poke (dfDispatchEnabled ownWords) 0]
    eq address value = Equals (GuestAddress address) value
    byte address value = ByteEquals (GuestAddress address) value
    state owner value = byte (taskAt facts owner waiterSlot + factsTaskState facts) value
    ready owner = state owner (factsStateReady facts)
    service name family objectDomain outcome boundary words' checks =
      Case (CaseId name) (ObjectService family objectDomain) TaskInvocation outcome [(R1, if family == SetFlag then 1 else message)] words' boundary checks Nothing
    release name owner invocation words' checks =
      Case (CaseId name) (OwnerRelease owner) invocation (Released owner) [(R1, taskAt facts owner waiterSlot)] (releaseWords owner ++ words') CallerReturn checks Nothing
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
    withTimer one =
      let owner = case caseWake one of Released domain -> domain; _ -> error "timer case has no waiter"
          wait = flagWait facts owner 2
       in one {caseTimer = Just timerToken, caseWords = caseWords one ++ [poke (wait + factsWaitBlockResult facts) (pointerWord timerToken)], caseExpect = caseExpect one ++ [eq (wait + factsWaitBlockResult facts) 0, eq receiptAddress (recordAddress facts executing owner), eq (receiptAddress + 4) (pointerWord timerToken)]}

layoutWords :: Facts -> [(GuestAddress, Word32)]
layoutWords facts =
  concat [recordPokes block described | block <- domains, described <- domains]
    ++ concatMap taskPokes domains ++ concatMap objectPokes domains ++ concatMap readyPokes domains
  where
    domains = [DomainA, DomainB]
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
          , poke (record + factsContextFlags facts) (dfFlagControls words')
          , poke (record + factsContextFlagCount facts) 120
          , poke (record + factsContextMailboxes facts) (dfMailboxControls words')
          , poke (record + factsContextMailboxCount facts) 1
          ] ++ lock (factsContextSchedulerLock facts) (dfSchedulerLock words') ++ lock (factsContextObjectLock facts) (dfObjectLock words')
    taskPokes domain = concat
      [ let address = taskAt facts domain slot
            definition = definitionAddress facts domain slot
            priority = if slot == runningSlot then 10 else if slot == idleSlot then 15 else if slot == waiterSlot then 3 else slotWord slot `mod` factsPriorities facts
         in [ poke address address, poke (address + factsTaskPrev facts) address
            , poke (address + factsTaskDefinition facts) definition, poke (address + factsTaskDomain facts) (dfId (domainFacts facts domain))
            , poke (address + factsTaskState facts) (packedState (factsStateDormant facts) priority)
            , poke (address + factsTaskSavedSp facts) targetStackPointer, poke (address + factsTaskResumePc facts) stopAddress
            , poke (definition + factsDefinitionPriority facts) priority
            , poke (definition + 4) (0xa5510000 + slotWord slot), poke (definition + 8) stopAddress
            , poke (definition + 0x10) (targetStackPointer - 0x100), poke (definition + 0x14) 0x100
            , poke (definition + 0x20) (threadValue domain slot)
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
            ++ [ poke (dfReadyBitmap words') ((1 `shiftL` 10) .|. (1 `shiftL` 15))
               , poke (dfRunning words') (taskAt facts domain runningSlot), poke (dfSelected words') (taskAt facts domain runningSlot)
               , poke (dfDispatchEnabled words') 1, poke (dfSchedulerLock words') 0, poke (dfObjectLock words') 0
               , poke (dfCurrentContext words') (recordAddress facts domain domain)
               ]

kernelBounds :: Facts -> Domain -> (GuestAddress, Int)
kernelBounds facts domain = (GuestAddress low, fromIntegral (high - low))
  where
    words' = domainFacts facts domain
    low = minimum [dfHandlerContext words', dfCurrentContext words', dfContexts words', dfSchedulerLock words', dfObjectLock words']
    high = maximum
      [ dfContextA words' + factsContextSize facts, dfContextB words' + factsContextSize facts
      , dfReadyHeads words' + factsPriorities facts * factsHeadSize facts, dfSelected words' + 4
      , dfTaskTable words' + factsTaskSlots facts * factsTaskSize facts
      , dfFlagConfigs words' + 120 * factsFlagConfigSize facts, dfFlagControls words' + 120 * factsFlagControlSize facts
      , dfFlagWaits words' + factsTaskSlots facts * factsFlagWaitSize facts, dfFlagTimers words' + factsTaskSlots facts * factsTimerEntrySize facts
      , dfMailboxConfigs words' + factsMailboxConfigSize facts, dfMailboxControls words' + factsMailboxControlSize facts
      , dfMailboxWaits words' + factsTaskSlots facts * factsMailboxWaitSize facts, dfMailboxTimers words' + factsTaskSlots facts * factsTimerEntrySize facts
      ]

callerSp :: Case -> Word32
callerSp one = if caseInvocation one == HandlerCallback then handlerStackPointer else stackPointer

callerBytes :: B.ByteString
callerBytes = B.concat [word32le (0xca11e500 + value) | value <- [0 .. 15]]

pages :: Facts -> Case -> [Page]
pages facts one =
  [ let (address, size) = kernelBounds facts domain in page (if domain == DomainA then "kernel-a" else "kernel-b") address size ReadWrite
  | domain <- [DomainA, DomainB]
  ]
  ++ [ page "definitions" (GuestAddress definitionBase) (fromIntegral (2 * factsTaskSlots facts * definitionStride)) ReadWrite
     , page "messages" (GuestAddress 0x40000000) 16 ReadWrite
     , page "timer-seam" (GuestAddress receiptAddress) 0x100 ReadWrite
     , page "global-lock" (GuestAddress globalLock) 4 ReadWrite
     , page "stack" (GuestAddress (callerSp one - 0x400)) 0x400 ReadWrite
     , Page (RegionId "caller-frame") (GuestAddress (callerSp one)) (B.length callerBytes) ReadWrite callerBytes
     , page "target-stack" (GuestAddress (targetStackPointer - 0x100)) 0x100 ReadWrite
     , page "bank-a" (bank DomainA) 4 Device
     , page "bank-b" (bank DomainB) 4 Device
     ]
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

codeBytes :: Domain -> Image -> Image -> B.ByteString
codeBytes domain loader owned = install timerAddress timerStub (foldl patch original (extents domain))
  where
    base = minimum (map fst (extents domain))
    top = maximum [address + size | (address, size) <- extents domain]
    timerAddress = timerEntry domain
    slice image address size =
      if address < imageBase image || toInteger address + toInteger size > toInteger (imageBase image) + toInteger (B.length (imageBytes image))
        then error "F2 code slice is outside its image"
        else B.take (fromIntegral size) (B.drop (fromIntegral (address - imageBase image)) (imageBytes image))
    original = slice loader base (top - base)
    install address bytes current = let offset = fromIntegral (address - base) in B.take offset current <> bytes <> B.drop (offset + B.length bytes) current
    patch current (address, size) = install address (slice owned address size) current

caseFor :: Facts -> Domain -> Image -> Image -> Case -> CaseSpec
caseFor facts executing loader owned one
  | not (null outside) = error (caseIdText (caseName one) ++ ": unmapped fixture words " ++ show outside)
  | otherwise = CaseSpec
      { caseId = emittedId executing one
      , caseProvenance = "X-M1 1.50 F2 stock ABI, pinned x70view 55436fc85c0560dd252b0410af6f2427fe198cd3; synthetic task/object state and timer argument seam"
      , caseExclusion = "Private compiler frames are not byte-compared. Dispatch checks the selected-task arrival and live S1 continuation, not the original service or receiver return. Handler callbacks enter SVC, not IRQ mode. Timer receipts prove arguments, not heap removal."
      , caseEntry = wireAddress (GuestAddress entry)
      , caseExit = wireAddress (GuestAddress exitAddress)
      , caseRegions =
          [mkRegion (RegionId "code") (wireAddress (GuestAddress codeBase)) (fromIntegral (codeTop - codeBase)) ReadExecute (codeBytes executing loader owned)]
            ++ [mkRegion ident (wireAddress address) size permission (overlay address background words') | Page ident address size permission background <- dataPages]
            ++ [mkRegion (RegionId "continuations") (wireAddress (GuestAddress exitAddress)) (fromIntegral (stopAddress - exitAddress + 4)) ReadExecute (B.replicate (fromIntegral (stopAddress - exitAddress + 4)) 0)]
      , caseFunctions = [(wireAddress (GuestAddress address), size) | (address, size) <- extents executing ++ retained executing ++ [(timerEntry executing, fromIntegral (B.length timerStub))]]
      , caseInitial = initialRegisters facts executing one
      , caseExpectedStatus = if caseBoundary one == CallerReturn then Returned else StopAddress
      , caseExpectedRegisters = if caseBoundary one == CallerReturn
          then (R0, expectedResult one) : [(register, seed register) | register <- [R4 .. R11]] ++ [(Sp, callerSp one)]
          else [(Pc, stopAddress), (Sp, targetStackPointer)]
      , caseExpectedMemory = [(RegionId "caller-frame", callerBytes)]
      , caseCp14Identity = Just (if executing == DomainA then 0x4003 else 0x4004)
      , caseKernel = Just (KernelCase plainLimit (if caseBoundary one == TaskArrival then Just (wireAddress (GuestAddress stopAddress)) else Nothing) (Just 0x80001234) (Just 0) (Just 0) ignored)
      }
  where
    entry = case caseCall one of ObjectService family _ -> entryAddress executing family; OwnerRelease _ -> releaseEntry executing
    codeBase = minimum (map fst (extents executing))
    codeTop = maximum [address + size | (address, size) <- extents executing]
    dataPages = pages facts one
    words' = layoutWords facts ++ [poke (dfHandlerContext (domainFacts facts executing)) (if caseInvocation one == HandlerCallback then 1 else 0)] ++ caseWords one
    outside = [address | (address, _) <- words', not (any (contains address 4) dataPages)]
    ignored = if caseBoundary one == CallerReturn then [R1,R2,R3,R12,Lr] else [R0 .. R12] ++ [Lr]

seed :: Register -> Word32
seed register = 0xa5a50000 + fromIntegral (fromEnum register)

expectedResult :: Case -> Word32
expectedResult one = case [value | Result value <- caseExpect one] of
  [value] -> value
  _ -> error "return case must state one ABI result"

initialRegisters :: Facts -> Domain -> Case -> [(Register, Word32)]
initialRegisters facts executing one =
  [(register, value register) | register <- [R0 .. R12]] ++ [(Sp, callerSp one), (Lr, exitAddress), (Cpsr, if caseInvocation one == HandlerCallback then 0x93 else 0x13)]
  where
    value R0 = case caseCall one of
      ObjectService _ owner -> (dfId (domainFacts facts owner) `shiftL` 16) .|. 1
      OwnerRelease owner -> recordAddress facts executing owner
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

compareFamily :: Facts -> Domain -> Case -> CaseSpec -> NativeObservation -> NativeObservation -> [String]
compareFamily facts executing one description stock owned =
  checks "stock" stock ++ checks "owned" owned ++ memoryDifferences
  where
    checks role observation = map ((role ++ ": ") ++) (exitChecks observation ++ expectedChecks observation ++ registerChecks observation ++ continuationChecks observation ++ effectChecks observation)
    wantedOutcome = if caseBoundary one == CallerReturn then OutcomeReturned else OutcomeStopped (wireAddress (GuestAddress stopAddress))
    exitChecks observation = ["wrong exit " ++ show (caseOutcome description observation) | caseOutcome description observation /= wantedOutcome]
    expectedChecks observation = ["missing effect " ++ show expectation | expectation <- caseExpect one, not (holds observation expectation)]
    holds observation (Equals address value) = readWord facts one observation address == Just value
    holds observation (ByteEquals address value) = readByte facts one observation address == Just value
    holds observation (Result value) = N.lookupRegister R0 observation == Just value
    registerChecks observation =
      ["caller frame changed" | lookup (RegionId "caller-frame") (nativeMemory observation) /= Just callerBytes]
        ++ ["CPSR control changed" | fmap (.&. 0x1ff) (N.lookupRegister Cpsr observation) /= Just expectedControl]
        ++ if caseBoundary one == CallerReturn
          then ["caller register " ++ show register ++ " changed" | (register, value) <- [(r, seed r) | r <- [R4 .. R11]] ++ [(Sp, callerSp one)], N.lookupRegister register observation /= Just value]
          else ["selected-task register " ++ show register ++ " differs" | (register, value) <- [(Sp,targetStackPointer),(Pc,stopAddress)], N.lookupRegister register observation /= Just value]
            ++ ["selected thread pointer differs" | (nativeKernel observation >>= observedTpidrurw) /= Just (threadValue executing waiterSlot)]
            ++ ["selected-task FPEXC differs" | (nativeKernel observation >>= observedFpexc) /= Just 0]
    expectedControl = if caseBoundary one == TaskArrival || caseInvocation one == HandlerCallback then 0x93 else 0x13
    continuationChecks observation
      | caseBoundary one == CallerReturn = []
      | otherwise = case readWord facts one observation (GuestAddress savedSpAddress) of
          Nothing -> ["missing suspended caller SP"]
          Just saved ->
            ["suspended caller frame is outside its live stack" | saved < callerSp one - 0x400 || toInteger saved + 36 > toInteger (callerSp one) || saved .&. 3 /= 0]
              ++ ["suspended caller resume entry differs" | readWord facts one observation (GuestAddress savedPcAddress) /= Just (resumeEntry executing)]
              ++ ["suspended S1 frame prefix differs" | (offset,value) <- [(0,0x13),(4,0x93),(8,dfHandlerContext (domainFacts facts executing))], readWord facts one observation (GuestAddress (saved+offset)) /= Just value]
              ++ ["suspended continuation is not in the caller's code" | maybe True (not . inOwnedCode) (readWord facts one observation (GuestAddress (saved+32)))]
    savedSpAddress = taskAt facts executing runningSlot + factsTaskSavedSp facts
    savedPcAddress = taskAt facts executing runningSlot + factsTaskResumePc facts
    inOwnedCode address = any (\(base,size) -> base <= address && address < base+size) (extents executing)
    compared = [ident | Page ident _ _ _ _ <- pages facts one, ident `notElem` map RegionId ["stack","target-stack"]]
    memory observation = M.fromList [(ident, normalize ident bytes) | (ident,bytes) <- nativeMemory observation, ident `elem` compared]
    normalize ident bytes
      | caseBoundary one == TaskArrival && ident == RegionId (if executing == DomainA then "kernel-a" else "kernel-b") =
          let (GuestAddress base,_) = kernelBounds facts executing
              offset = fromIntegral (savedSpAddress-base)
           in B.take offset bytes <> word32le 0 <> B.drop (offset+4) bytes
      | otherwise = bytes
    memoryDifferences =
      ["missing state region " ++ show ident | ident <- compared, M.notMember ident (memory stock) || M.notMember ident (memory owned)]
        ++ ["state region " ++ show ident ++ " differs" | ident <- compared, M.lookup ident (memory stock) /= M.lookup ident (memory owned)]
    effectChecks observation = case nativeKernel observation >>= observedOrder of
      Nothing -> ["ordered provenance is missing or truncated"]
      Just events -> lockChecks events ++ notificationChecks events ++ timerChecks events
    writes events address = [(index,fromIntegral value :: Word32) | (index,OrderedWrite (Write actual _) (WriteValue value)) <- zip [0::Int ..] events, actual == wireAddress address]
    lockChecks events = case caseCall one of
      OwnerRelease _ -> []
      ObjectService _ objectDomain ->
        let objectLock = GuestAddress (dfObjectLock (domainFacts facts objectDomain))
            scheduler = case caseWake one of NoWaiter -> Nothing; Unsatisfied owner -> Just owner; Released owner -> Just owner
            interval address = do
              acquired <- fst <$> first ((==1) . snd) (writes events address)
              released <- fst <$> first ((==0) . snd) (writes events address)
              if acquired < released && any (\(index,event) -> acquired < index && index < released && event == OrderedBarrier Dmb) (zip [0::Int ..] events) then Just (acquired,released) else Nothing
         in ["object lock or its release barrier differs" | interval objectLock == Nothing]
              ++ case scheduler of
                Nothing -> []
                Just owner -> case (interval objectLock, interval (GuestAddress (dfSchedulerLock (domainFacts facts owner)))) of
                  (Just (oa,oz), Just (sa,sz)) | oa < sa && sz < oz -> []
                  _ -> ["waiter scheduler lock is not nested inside the object lock"]
    notificationChecks events =
      let target = case (caseCall one,caseWake one) of (ObjectService _ _,Released owner) | owner /= executing -> Just owner; _ -> Nothing
          expected domain = if target == Just domain then [0x01000000] else []
          bankChecks = ["notification bank " ++ show domain ++ " differs" | domain <- [DomainA,DomainB], map snd (writes events (bank domain)) /= expected domain]
       in bankChecks ++ case target of
          Nothing -> []
          Just owner -> case writes events (bank owner) of
            [(index,_)] | any (\(before,event) -> before < index && event == OrderedBarrier Dsb) (zip [0::Int ..] events) -> []
            _ -> ["notification has no preceding DSB"]
    timerChecks events = case caseTimer one of
      Nothing -> ["unexpected timer removal" | not (null (writes events (GuestAddress receiptAddress))) || not (null (writes events (GuestAddress (receiptAddress+4))))]
      Just token ->
        let owner = case caseWake one of Released domain -> domain; _ -> error "timer case without owner"
            expectedOwner = recordAddress facts executing owner
         in ["timer removal arguments differ" | map snd (writes events (GuestAddress receiptAddress)) /= [expectedOwner] || map snd (writes events (GuestAddress (receiptAddress+4))) /= [pointerWord token]]
    first predicate values = case filter predicate values of value:_ -> Just value; [] -> Nothing

runFamily :: FilePath -> FilePath -> FilePath -> Facts -> Domain -> Image -> Image -> IO FamilyRun
runFamily runner cacheRoot directory facts executing stock owned = do
  let selected = cases facts executing
      specs image = [(one,caseFor facts executing stock image one) | one <- selected]
      stockCases = specs stock
      ownedCases = specs owned
  stockBatch <- writeBatch (directory </> "stock") (map snd stockCases)
  ownedBatch <- writeBatch (directory </> "owned") (map snd ownedCases)
  (stockReuse, stockRaw) <- cachedObservedBatch cacheRoot runner stockBatch
  (ownedReuse, ownedRaw) <- cachedObservedBatch cacheRoot runner ownedBatch
  let reference = M.fromList [(caseIdText ident,observation) | (ident,observation) <- stockRaw]
      subject = M.fromList [(caseIdText ident,observation) | (ident,observation) <- ownedRaw]
  results <- forM stockCases $ \(one,description) -> do
    let ident = emittedId executing one
    pure (caseIdText (caseName one),case (M.lookup ident reference,M.lookup ident subject) of
      (Just original,Just replacement) -> Right (compareFamily facts executing one description original replacement)
      (Nothing,_) -> Left "stock case did not run"
      (_,Nothing) -> Left "owned case did not run")
  unless (length stockRaw == length selected && length ownedRaw == length selected) (fail "native observations do not cover the current F2 case set")
  pure (FamilyRun executing results (length [() | (_,Right []) <- results]) (stockReuse,ownedReuse))
