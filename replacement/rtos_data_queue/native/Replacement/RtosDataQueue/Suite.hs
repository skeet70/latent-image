{-# LANGUAGE OverloadedStrings #-}
module Replacement.RtosDataQueue.Suite
  ( Image (..), CaseRun (..), extents, runDomain ) where

import Control.Monad (forM, unless)
import Data.Bits ((.&.), (.|.), shiftL)
import qualified Data.ByteString as B
import qualified Data.Map.Strict as M
import Data.List (sortOn)
import Data.Word (Word32, Word8)
import Emulation.CachedNativeBatch (BatchReuse (..), cachedObservedBatch)
import Emulation.NativeCase (CaseId (..), caseIdText)
import qualified Emulation.NativeObservation as N
import Emulation.NativeObservation (Barrier (..), KernelObservation (..), NativeObservation (..), Ordered (..), RegionId (..), Register (..), RunnerStatus (..), Write (..), WriteValue (..), regionName, runnerStatusLabel)
import Emulation.OwnedCase (CaseOutcome (..), CaseSpec (..), KernelCase (..), Permission (..), assemble, caseOutcome, mkRegion, plainLimit, readWordAt, word32le, writeBatch)
import Replacement.RtosDataQueue.Layout
import Replacement.RtosOwnFirst.Regions (Domain (..), GuestAddress (..), domainLabel)
import System.FilePath ((</>))

data Image = Image { imageBase :: Word32, imageBytes :: B.ByteString }

data Call = Create | Delete | Send | SendPoll | SendPollHandler | SendTimeout
          | ForceSend | ForceSendHandler | Receive | ReceivePoll | ReceiveTimeout
  deriving (Eq, Show)

data Expect = WordIs String Word32 Word32 | ByteIs String Word32 Word8 | ResultIs Word32
  deriving (Eq, Show)

data Case = Case
  { caseName :: CaseId
  , caseCall :: Call
  , caseArgs :: [(Register, Word32)]
  , caseWords :: [(Word32, Word32)]
  , caseExpect :: [Expect]
  , caseStockExpect :: [Expect]
  , caseOwnedExpect :: [Expect]
  , caseTimerRemove :: Bool
  , caseTimerInsert :: Bool
  , caseHandler :: Bool
  , caseArrival :: Maybe (Word32, Word32)
  }

data CaseRun = CaseRun
  { runEntryDomain :: Domain
  , runName :: String
  , runChecks :: [(String, Bool)]
  , runDifferences :: [String]
  , runReuse :: (BatchReuse, BatchReuse)
  }

data Page = Page RegionId GuestAddress Int Permission B.ByteString

ringBase, outBase, packetBase, stubBase, stackBase, stackSize, stackPointer, exitAddress :: Word32
ringBase = 0x40000000
outBase = 0x40001000
packetBase = 0x40002000
stubBase = 0x01000000
stackBase = 0x00300000
stackSize = 0x1000
stackPointer = stackBase + 0x800
exitAddress = 0x01000200
stopAddress :: Word32
stopAddress = 0x01000300
targetStack :: Word32
targetStack = 0x00300900

seedRegister :: Register -> Word32
seedRegister register = 0xa5a50000 + fromIntegral (fromEnum register)

callerBytes :: B.ByteString
callerBytes = B.concat [word32le (0xca11e500 + value) | value <- [0 .. 15]]

timerStub :: B.ByteString
timerStub = assemble [0xe3003000, 0xe3403100, 0xe5830000, 0xe5831004, 0xe5832008, 0xe3a00000, 0xe12fff1e]

removeStub :: B.ByteString
removeStub = assemble [0xe3003000, 0xe3403100, 0xe5830010, 0xe5831014, 0xe3a00000, 0xe12fff1e]

entryAddress :: Domain -> Call -> Word32
entryAddress domain call = case (domain, call) of
  (DomainA, Create) -> 0x4d49c
  (DomainA, Delete) -> 0x4d540
  (DomainA, Send) -> 0x48ff8
  (DomainA, SendPoll) -> 0x49308
  (DomainA, SendPollHandler) -> 0x49528
  (DomainA, SendTimeout) -> 0x49718
  (DomainA, ForceSend) -> 0x49aec
  (DomainA, ForceSendHandler) -> 0x49d2c
  (DomainA, Receive) -> 0x49f2c
  (DomainA, ReceivePoll) -> 0x4a2f0
  (DomainA, ReceiveTimeout) -> 0x4a56c
  (DomainB, Create) -> 0xdd460
  (DomainB, Delete) -> 0xdd504
  (DomainB, Send) -> 0xd8fb8
  (DomainB, SendPoll) -> 0xd92c8
  (DomainB, SendPollHandler) -> 0xd94e8
  (DomainB, SendTimeout) -> 0xd96d8
  (DomainB, ForceSend) -> 0xd9aac
  (DomainB, ForceSendHandler) -> 0xd9cec
  (DomainB, Receive) -> 0xd9eec
  (DomainB, ReceivePoll) -> 0xda2b0
  (DomainB, ReceiveTimeout) -> 0xda52c

timerInsert, timerRemove :: Domain -> Word32
timerInsert DomainA = 0x44c60
timerInsert DomainB = 0xd4c20
timerRemove DomainA = 0x44ca8
timerRemove DomainB = 0xd4c68

extents :: Domain -> [(Word32, Word32)]
extents domain =
  [ (entryAddress domain call, entrySize call) | call <- [Create, Delete, Send, SendPoll, SendPollHandler, SendTimeout, ForceSend, ForceSendHandler, Receive, ReceivePoll, ReceiveTimeout] ]
    ++ helperFunctions domain
    ++ [(timerInsert domain, fromIntegral (B.length timerStub)), (timerRemove domain, fromIntegral (B.length removeStub))]

helperFunctions :: Domain -> [(Word32, Word32)]
helperFunctions DomainA =
  [ (0x4252c, 0x28), (0x42560, 0x68)
  , (0x43640, 0x28), (0x43668, 0x20), (0x43688, 0xa0), (0x43728, 0x34), (0x4375c, 0x48)
  , (0x437a4, 0x38), (0x437fc, 0x14), (0x439d8, 0x3c), (0x43a14, 0x28), (0x43c48, 0x5c)
  , (0x43ca4, 0x7c), (0x43d20, 0xb0), (0x43dd0, 0x48), (0x43f70, 0x2c), (0x44018, 0x2c)
  , (0x44044, 0xf0), (0x44134, 0x80), (0x44318, 0x1cc), (0x444e4, 0x84), (0x44568, 0xcc)
  , (0x4471c, 0xc8), (0x447e4, 0x1c), (0x44800, 0x2c), (0x4482c, 0xb8), (0x448e4, 0xe8)
  , (0x48edc, 0x68), (0x48f44, 0x60), (0x48fa4, 0x54)
  ]
helperFunctions DomainB =
  [ (0xd252c, 0x28), (0xd2560, 0x68)
  , (0xd3640, 0x28), (0xd3668, 0x20), (0xd3688, 0xa0), (0xd3728, 0x34), (0xd375c, 0x48)
  , (0xd37a4, 0x38), (0xd37fc, 0x14), (0xd398c, 0x3c), (0xd39c8, 0x28), (0xd3c04, 0x5c)
  , (0xd3c60, 0x7c), (0xd3cdc, 0xb0), (0xd3d8c, 0x48), (0xd3f2c, 0x2c), (0xd3fd4, 0x30)
  , (0xd4004, 0xf0), (0xd40f4, 0x80), (0xd42d8, 0x1cc), (0xd44a4, 0x84), (0xd4528, 0xcc)
  , (0xd46dc, 0xc8), (0xd47a4, 0x1c), (0xd47c0, 0x2c), (0xd47ec, 0xb8), (0xd48a4, 0xe8)
  , (0xd8e9c, 0x68), (0xd8f04, 0x60), (0xd8f64, 0x54)
  ]

entrySize :: Call -> Word32
entrySize Create = 0xa4
entrySize Delete = 0x7c
entrySize Send = 0x310
entrySize SendPoll = 0x220
entrySize SendPollHandler = 0x1f0
entrySize SendTimeout = 0x3d4
entrySize ForceSend = 0x240
entrySize ForceSendHandler = 0x200
entrySize Receive = 0x3c4
entrySize ReceivePoll = 0x27c
entrySize ReceiveTimeout = 0x450

bank :: Domain -> Word32
bank DomainA = 0xfffb30cc
bank DomainB = 0xfffb40cc

peer :: Domain -> Domain
peer DomainA = DomainB
peer DomainB = DomainA

poke :: Word32 -> Word32 -> (Word32, Word32)
poke address value = (address, value)

eq :: String -> Word32 -> Word32 -> Expect
eq region address value = WordIs region address value

dataQueueId :: Word32 -> Word32
dataQueueId slot = slot

cases :: Facts -> Domain -> [Case]
cases facts executing =
  [ caseOf "send-empty" Send [(R0, 1), (R1, 0x1111)] (queue 1 0 2 ++ empty) [ResultIs 0, eq "ring" ringBase 0x1111, eq "kernel" (count 1) 1, eq "kernel" (read 1) 0, eq "kernel" (write 1) 1]
  , caseOf "send-cross-domain" Send [(R0, if executing == DomainA then 0x20001 else 0x10001), (R1, 0x7777)] (peerQueue 1 0 2) [ResultIs 0, eq "peer-kernel" (peerCount 1) 1, eq "peer-kernel" (peerWrite 1) 1, eq "ring" ringBase 0x7777]
  , caseOf "send-poll-full" SendPoll [(R0, 1), (R1, 0xbbbb)] (queue 1 0 1 ++ full [0xaaaa] 1) [ResultIs (-50), eq "ring" ringBase 0xaaaa, eq "kernel" (count 1) 1]
  , caseOf "force-send-loss" ForceSend [(R0, 1), (R1, 3)] (queue 1 0 2 ++ full [1, 2] 2) [ResultIs 0, eq "ring" ringBase 3, eq "ring" (ringBase + 4) 2, eq "kernel" (read 1) 1, eq "kernel" (count 1) 2, eq "kernel" (write 1) 1]
  , caseOf "receive-wrap" Receive [(R0, 1), (R1, outBase)] (queue 1 0 2 ++ full [1, 2] 2) [ResultIs 0, eq "out" outBase 1, eq "kernel" (count 1) 1, eq "kernel" (read 1) 1, eq "kernel" (write 1) 0]
  , caseOf "receive-empty-poll" ReceivePoll [(R0, 1), (R1, outBase)] (queue 1 0 2 ++ [poke outBase 0x5e4d1e00]) [ResultIs (-50), eq "out" outBase 0x5e4d1e00]
  , caseOf "handoff-receiver" Send [(R0, 1), (R1, 0x1234)] (queue 1 0 2 ++ receiverWaiter) [ResultIs 0, eq "kernel" (waitData 2) 0x1234, eq "kernel" (waitResult 2) 0, ByteIs "kernel" (taskState 2) (stateReady facts), eq "kernel" (recvNext 1) (recvSelf 1)]
  , caseOf "handoff-sender" Receive [(R0, 1), (R1, outBase)] (queue 1 0 1 ++ full [0xaaaa] 1 ++ senderWaiter) [ResultIs 0, eq "out" outBase 0xaaaa, eq "ring" ringBase 0xbbbb, eq "kernel" (count 1) 1, ByteIs "kernel" (taskState 2) (stateReady facts), eq "kernel" (sendNext 1) (sendSelf 1)]
  , difference "delete-stranded-receiver" Delete [(R0, 1)] (queue 1 0 2 ++ receiverWaiter)
      [ResultIs 0, eq "kernel" (configExists 1) 0]
      [ResultIs 0, eq "kernel" (configExists 1) 0, eq "kernel" (recvNext 1) (recvSelf 1), eq "kernel" (waitResult 2) 0, ByteIs "kernel" (taskState 2) (stateObjectQueued facts .|. stateWait facts)]
      [ResultIs 0, eq "kernel" (configExists 1) 0, eq "kernel" (recvNext 1) (recvSelf 1), eq "kernel" (waitResult 2) (-49), ByteIs "kernel" (taskState 2) (stateReady facts)]
      False
  , difference "delete-timed-waiter" Delete [(R0, 1)] (queue 1 0 2 ++ timedReceiverWaiter)
      [ResultIs 0, eq "kernel" (configExists 1) 0]
      [ResultIs 0, eq "kernel" (configExists 1) 0]
      [ResultIs 0, eq "kernel" (configExists 1) 0, eq "kernel" (waitResult 2) (-49), ByteIs "kernel" (taskState 2) (stateReady facts)]
      True
  , caseOf "create" Create [(R0, 4), (R1, packetBase)]
      [ poke (configExists 4) 0, poke (configAttributes 4) 0x5a5a5a5a
      , poke packetBase 1, poke (packetBase + 4) 2, poke (packetBase + 8) ringBase
      ] [ResultIs 0, eq "kernel" (configAttributes 4) 1, eq "kernel" (configCapacity 4) 2, eq "kernel" (configRing 4) ringBase, eq "kernel" (configExists 4) 1, eq "kernel" (count 4) 0, eq "kernel" (sendNext 4) (sendSelf 4)]
  , caseOf "create-refuse" Create [(R0, 4), (R1, packetBase)]
      [ poke (configExists 4) 1, poke (configAttributes 4) 0x5a5a5a5a, poke packetBase 7 ]
      [ResultIs (-41), eq "kernel" (configAttributes 4) 0x5a5a5a5a]
  , caseOf "refuse-not-created" SendPoll [(R0, 5), (R1, 0xbbbb)] [poke (configExists 5) 0, poke ringBase 0x5e4d1e00] [ResultIs (-42), eq "ring" ringBase 0x5e4d1e00]
  , caseOf "refuse-invalid-id" SendPoll [(R0, 0x30001), (R1, 0xbbbb)] (queue 1 0 2) [ResultIs (-18), eq "kernel" (count 1) 0]
  , handler "handler-force" ForceSendHandler [(R0, 1), (R1, 3)] (queue 1 0 2 ++ full [1, 2] 2) [ResultIs 0, eq "ring" ringBase 3, eq "kernel" (read 1) 1, eq "kernel" (count 1) 2]
  , handler "handler-class-refused" ForceSendHandler [(R0, 0x10001), (R1, 3)] (queue 1 0 2 ++ full [1, 2] 2) [ResultIs (-18), eq "ring" ringBase 1, eq "kernel" (read 1) 0]
  , caseOf "timed-receive-poll" ReceiveTimeout [(R0, 1), (R1, outBase), (R2, 0)] (queue 1 0 2 ++ [poke outBase 0x5e4d1e00]) [ResultIs (-50), eq "out" outBase 0x5e4d1e00]
  , caseOf "timed-send-poll" SendTimeout [(R0, 1), (R1, 2), (R2, 0)] (queue 1 0 1 ++ full [0xaaaa] 1) [ResultIs (-50), eq "ring" ringBase 0xaaaa, eq "kernel" (count 1) 1]
  , caseOf "send-timeout-invalid" SendTimeout [(R0, 1), (R1, 2), (R2, -2)] (queue 1 0 2) [ResultIs (-17), eq "kernel" (count 1) 0]
  ]
  where
    queue slot attributes capacity =
      [ poke (configExists slot) 1
      , poke (configAttributes slot) attributes
      , poke (configCapacity slot) capacity
      , poke (configRing slot) ringBase
      , poke (sendNext slot) (sendSelf slot)
      , poke (sendPrev slot) (sendSelf slot)
      , poke (recvNext slot) (recvSelf slot)
      , poke (recvPrev slot) (recvSelf slot)
      , poke (count slot) 0
      , poke (read slot) 0
      , poke (write slot) 0
      ]
    empty = []
    full words' count' = [poke (ringBase + fromIntegral index * 4) value | (index, value) <- zip [0 :: Int ..] words'] ++ [poke (count 1) count', poke (read 1) 0, poke (write 1) (count' `mod` fromIntegral (max 1 (length words')))]
    receiverWaiter =
      let t = taskAt 2
          q = recvSelf 1
       in [ poke (recvNext 1) t, poke (recvPrev 1) t
          , poke (t + factsTaskNext facts) q, poke (t + factsTaskPrev facts) q
          , poke (t + factsTaskState facts) (packedState (stateObjectQueued facts .|. stateWait facts) 3)
          , poke (t + factsTaskWait facts) (wait 2)
          , poke (wait 2) 0, poke (waitQueue 2) q, poke (waitOwner 2) (contextRecord facts executing executing)
          ] ++ removeReady3
    timedReceiverWaiter = receiverWaiter ++ [poke (wait 2) (dataQueueTimer facts executing 2)]
    senderWaiter =
      let t = taskAt 2
          q = sendSelf 1
       in [ poke (sendNext 1) t, poke (sendPrev 1) t
          , poke (t + factsTaskNext facts) q, poke (t + factsTaskPrev facts) q
          , poke (t + factsTaskState facts) (packedState (stateObjectQueued facts .|. stateWait facts .|. stateOwnerLockQueued facts) 3)
          , poke (t + factsTaskWait facts) (wait 2)
          , poke (wait 2) 0, poke (waitData 2) 0xbbbb, poke (waitQueue 2) q, poke (waitOwner 2) (contextRecord facts executing executing)
          ] ++ removeReady3
    removeReady3 =
      let words' = domainFacts facts executing
          head3 = dfReadyHeads words' + 3 * 8
          bitmap = dfReadyBitmap words'
       in [poke head3 head3, poke (head3 + 4) head3, poke bitmap ((1 `shiftL` 0) .|. (1 `shiftL` 5))]
    caseOf name call args words' checks =
      Case (CaseId name) call args words' checks [] [] False False False Nothing
    difference name call args words' common stockChecks ownedChecks timer =
      Case (CaseId name) call args words' common stockChecks ownedChecks timer False False Nothing
    handler name call args words' checks =
      Case (CaseId name) call args (words' ++ [poke (dfHandlerContext (domainFacts facts executing)) 1]) checks [] [] False False True Nothing
    arrival name call args words' checks timerInsert =
      Case (CaseId name) call args words' checks [] [] False timerInsert False (Just (stopAddress, targetStack))
    peerDomain = if executing == DomainA then DomainB else DomainA
    peerQueue slot attributes capacity =
      let config = dataQueueConfig facts peerDomain slot
          control = dataQueueControl facts peerDomain slot
       in [ poke (config + rfExists (factsRecords facts)) 1
          , poke (config + rfAttributes (factsRecords facts)) attributes
          , poke (config + rfCapacity (factsRecords facts)) capacity
          , poke (config + rfRingBase (factsRecords facts)) ringBase
          , poke control control, poke (control + 4) control
          , poke (control + rfReceivers (factsRecords facts)) (control + rfReceivers (factsRecords facts))
          , poke (control + rfReceivers (factsRecords facts) + 4) (control + rfReceivers (factsRecords facts))
          , poke (control + rfCount (factsRecords facts)) 0
          , poke (control + rfRead (factsRecords facts)) 0
          , poke (control + rfWrite (factsRecords facts)) 0
          ]
    peerCount slot = dataQueueControl facts peerDomain slot + rfCount (factsRecords facts)
    peerWrite slot = dataQueueControl facts peerDomain slot + rfWrite (factsRecords facts)
    configExists slot = dataQueueConfig facts executing slot + rfExists (factsRecords facts)
    configAttributes slot = dataQueueConfig facts executing slot + rfAttributes (factsRecords facts)
    configCapacity slot = dataQueueConfig facts executing slot + rfCapacity (factsRecords facts)
    configRing slot = dataQueueConfig facts executing slot + rfRingBase (factsRecords facts)
    sendSelf slot = dataQueueControl facts executing slot + rfSenderQueue (factsRecords facts)
    sendNext slot = sendSelf slot
    sendPrev slot = sendSelf slot + 4
    recvSelf slot = dataQueueControl facts executing slot + rfReceivers (factsRecords facts)
    recvNext slot = recvSelf slot
    recvPrev slot = recvSelf slot + 4
    count slot = dataQueueControl facts executing slot + rfCount (factsRecords facts)
    read slot = dataQueueControl facts executing slot + rfRead (factsRecords facts)
    write slot = dataQueueControl facts executing slot + rfWrite (factsRecords facts)
    taskAt slot = task facts executing slot
    wait slot = dataQueueWait facts executing slot
    waitData slot = wait slot + rfWaitData (factsRecords facts)
    waitResult slot = wait slot
    waitQueue slot = wait slot + 4
    waitOwner slot = wait slot + 8
    taskState slot = taskAt slot + factsTaskState facts
    packedState state priority = fromIntegral state .|. (priority `shiftL` 8)

sendStateBits :: Word8
sendStateBits = 0x32

stateReady, stateWait, stateObjectQueued, stateOwnerLockQueued :: Facts -> Word8
stateReady = factsStateReady
stateWait = factsStateWait
stateObjectQueued = factsStateObjectQueued
stateOwnerLockQueued = factsStateOwnerLockQueued

layoutWords :: Facts -> [(Word32, Word32)]
layoutWords facts =
  concat [recordPokes block described | block <- domains, described <- domains]
    ++ concatMap taskPokes domains ++ concatMap objectPokes domains ++ concatMap readyPokes domains
  where
    domains = [DomainA, DomainB]
    recordPokes block described =
      let record = contextRecord facts block described
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
          , poke (record + factsContextDataQueues facts) (dfDataQueueControls words')
          , poke (record + factsContextDataQueueCount facts) 120
          ] ++ lock (factsContextSchedulerLock facts) (dfSchedulerLock words') ++ lock (factsContextObjectLock facts) (dfObjectLock words')
    taskPokes domain = concat
      [ let address = task facts domain slot
            definition' = definition facts domain slot
            priority = if slot == 1 then 0 else if slot == 2 then 3 else 5
         in [ poke address address, poke (address + factsTaskPrev facts) address
            , poke (address + factsTaskDefinition facts) definition'
            , poke (address + factsTaskDomain facts) (dfId (domainFacts facts domain))
            , poke (address + factsTaskState facts) (fromIntegral (factsStateDormant facts) .|. (priority `shiftL` 8))
            , poke (address + factsTaskSavedSp facts) (if slot == 3 then targetStack else stackPointer)
            , poke (address + factsTaskResumePc facts) (if slot == 3 then stopAddress else exitAddress)
            , poke (definition' + factsDefinitionPriority facts) priority
            , poke (definition' + 4) (0xa5510000 + slot)
            , poke (definition' + 8) exitAddress
            , poke (definition' + 0x10) (stackPointer - 0x100)
            , poke (definition' + 0x14) 0x100
            , poke (definition' + 0x20) slot
            ]
      | slot <- [1 .. 3]
      ]
    objectPokes domain = concat
      [ let config = dataQueueConfig facts domain slot
            control = dataQueueControl facts domain slot
         in [ poke (config + rfExists (factsRecords facts)) 0
            , poke (config + rfAttributes (factsRecords facts)) 0
            , poke (config + rfCapacity (factsRecords facts)) 0
            , poke (config + rfRingBase (factsRecords facts)) ringBase
            , poke (control + rfCreation (factsRecords facts)) config
            , poke control control, poke (control + 4) control
            , poke (control + rfReceivers (factsRecords facts)) (control + rfReceivers (factsRecords facts))
            , poke (control + rfReceivers (factsRecords facts) + 4) (control + rfReceivers (factsRecords facts))
            , poke (control + rfCount (factsRecords facts)) 0
            , poke (control + rfRead (factsRecords facts)) 0
            , poke (control + rfWrite (factsRecords facts)) 0
            ]
      | slot <- [1 .. 5]
      ] ++ concat
      [ [poke (dataQueueWait facts domain slot) 0, poke (dataQueueWait facts domain slot + 4) 0, poke (dataQueueWait facts domain slot + 8) 0, poke (dataQueueWait facts domain slot + 12) 0]
      | slot <- [1 .. 3]
      ] ++ concat
      [ [poke (dataQueueTimer facts domain slot + offset) 0 | offset <- [0, 4, 8]]
      | slot <- [1 .. 3]
      ]
    readyPokes domain =
      let words' = domainFacts facts domain
          headAt priority = dfReadyHeads words' + priority * factsHeadSize facts
          member slot priority =
            let address = task facts domain slot
                headAddress = headAt priority
             in [poke headAddress address, poke (headAddress + factsHeadPrev facts) address, poke address headAddress, poke (address + factsTaskPrev facts) headAddress, poke (address + factsTaskState facts) (fromIntegral (factsStateReady facts) .|. (priority `shiftL` 8))]
       in concat [[poke (headAt priority) (headAt priority), poke (headAt priority + factsHeadPrev facts) (headAt priority)] | priority <- [0 .. factsPriorities facts - 1]]
            ++ member 1 0 ++ member 2 3 ++ member 3 5
            ++ [ poke (dfReadyBitmap words') ((1 `shiftL` 0) .|. (1 `shiftL` 3) .|. (1 `shiftL` 5))
               , poke (dfRunning words') (task facts domain 1), poke (dfSelected words') (task facts domain 1)
               , poke (dfDispatchEnabled words') 1, poke (dfSchedulerLock words') 0, poke (dfObjectLock words') 0
               , poke (dfCurrentContext words') (contextRecord facts domain domain)
               , poke (dfHandlerContext words') 0
               ]

trampoline :: Domain -> Word32
trampoline DomainA = 0x43728
trampoline DomainB = 0xd3728

lockEntries :: Domain -> (Word32, Word32)
lockEntries DomainA = (0x439d8, 0x43a14)
lockEntries DomainB = (0xd398c, 0xd39c8)

kernelBounds :: Facts -> Domain -> (Word32, Word32)
kernelBounds facts domain = (low, high)
  where
    words' = domainFacts facts domain
    low = minimum [dfHandlerContext words', dfCurrentContext words', dfContexts words', dfSchedulerLock words', dfObjectLock words', dfDataQueueConfigs words', dfTaskTable words']
    high = maximum
      [ dfContextA words' + factsContextSize facts, dfContextB words' + factsContextSize facts
      , dfReadyHeads words' + factsPriorities facts * factsHeadSize facts
      , dfSelected words' + 4, dfTaskTable words' + 3 * factsTaskSize facts
      , dfDataQueueConfigs words' + 5 * rfSize (factsRecords facts)
      , dfDataQueueControls words' + 5 * rfControlSize (factsRecords facts)
      , dfDataQueueWaits words' + 3 * rfWaitSize (factsRecords facts)
      , dfDataQueueTimers words' + 3 * factsTimerEntrySize facts
      ]

contains :: GuestAddress -> Int -> Page -> Bool
contains (GuestAddress address) width (Page _ (GuestAddress base) size _ _) =
  toInteger base <= toInteger address && toInteger address + toInteger width <= toInteger base + toInteger size

overlay :: Word32 -> B.ByteString -> [(Word32, Word32)] -> B.ByteString
overlay start initial words' = B.concat (pieces 0 ordered)
  where
    ordered = M.toAscList (M.fromList [(address, value) | (address, value) <- words', address >= start, toInteger address + 4 <= toInteger start + toInteger (B.length initial)])
    pieces cursor [] = [B.drop cursor initial]
    pieces cursor ((address, value) : rest) =
      let offset = fromIntegral (address - start)
       in B.take (offset - cursor) (B.drop cursor initial) : word32le value : pieces (offset + 4) rest

codeBytes :: Domain -> Image -> B.ByteString
codeBytes domain image = install (timerRemove domain) removeStub (install (timerInsert domain) timerStub base)
  where
    base = imageBytes image
    install address bytes current =
      let offset = fromIntegral (address - imageBase image)
       in B.take offset current <> bytes <> B.drop (offset + B.length bytes) current

pages :: Facts -> Domain -> Case -> [Page]
pages facts domain _ =
  [ let (low, high) = kernelBounds facts domain in page "kernel" low (fromIntegral (high - low)) ReadWrite
  , let (low, high) = kernelBounds facts peerDomain in page "peer-kernel" low (fromIntegral (high - low)) ReadWrite
  , page "definitions" 0x0096c000 0x4000 ReadWrite
  , page "ring" ringBase 0x100 ReadWrite
  , page "out" outBase 0x10 ReadWrite
  , page "packet" packetBase 0x10 ReadWrite
  , page "timer-stub" stubBase 0x40 ReadWrite
  , page "stack" (stackPointer - 0x400) 0x400 ReadWrite
  , page "target-stack" targetStack 0x100 ReadWrite
  , page "exit" exitAddress 0x10 ReadExecute
  , page "stop" stopAddress 0x10 ReadExecute
  , Page (RegionId "bank-a") (GuestAddress (fromIntegral (bank DomainA))) 4 Device (B.replicate 4 0)
  , Page (RegionId "bank-b") (GuestAddress (fromIntegral (bank DomainB))) 4 Device (B.replicate 4 0)
  , Page (RegionId "caller-frame") (GuestAddress (fromIntegral stackPointer)) (B.length callerBytes) ReadWrite callerBytes
  ]
  where
    peerDomain = if domain == DomainA then DomainB else DomainA
    page name address size permission = Page (RegionId name) (GuestAddress (fromIntegral address)) size permission (B.replicate size 0)

initialRegisters :: Facts -> Domain -> Case -> [(Register, Word32)]
initialRegisters facts _ one =
  [(register, value register) | register <- [R0 .. R12]] ++ [(Sp, stackPointer), (Lr, exitAddress), (Cpsr, 0x13)]
  where
    value register = maybe (seedRegister register) id (lookup register (caseArgs one))

caseFor :: Facts -> Domain -> Bool -> [(Word32, Word32)] -> Image -> Case -> CaseSpec
caseFor facts domain owned ownedSymbols image one =
  CaseSpec
    { caseId = caseIdText (caseName one) ++ "-" ++ domainLabel domain
    , caseProvenance = "X-M1 1.50 data-queue stock ABI, x70view 55436fc85c0560dd252b0410af6f2427fe198cd3; synthetic queue, task and timer-seam state"
    , caseExclusion = "Synthetic queue state and timer stubs; no blocking continuation, dispatcher arrival or firmware integration."
    , caseEntry = N.GuestAddress (fromIntegral (entryAddress domain (caseCall one)))
    , caseExit = N.GuestAddress (fromIntegral exitAddress)
    , caseRegions =
        [mkRegion (RegionId "code") (N.GuestAddress (fromIntegral (imageBase image))) (fromIntegral (B.length (codeBytes domain image))) ReadExecute (codeBytes domain image)]
          ++ [mkRegion ident (N.GuestAddress (fromIntegral address)) size permission (overlay address background (wordsFor ident)) | Page ident (GuestAddress address) size permission background <- dataPages]
    , caseFunctions = [(N.GuestAddress (fromIntegral address), size) | (address, size) <- functions]
    , caseInitial = initialRegisters facts domain one
    , caseExpectedStatus = case caseArrival one of
        Just _ -> StopAddress
        Nothing -> Returned
    , caseExpectedRegisters = case caseArrival one of
        Just (stop, _) -> [(Pc, stop)]
        Nothing -> [(register, seedRegister register) | register <- [R4 .. R11]] ++ [(Sp, stackPointer)]
    , caseExpectedMemory = []
    , caseCp14Identity = Just (if domain == DomainA then 0x4003 else 0x4004)
    , caseKernel = Just (KernelCase plainLimit (fmap (N.GuestAddress . fromIntegral . fst) (caseArrival one)) (Just 0x80001234) (Just 0) (Just 0) (case caseArrival one of Just _ -> [R0 .. R12] ++ [Lr]; Nothing -> [R0, R1, R2, R3, R12, Lr]) [])
    }
  where
    dataPages = pages facts domain one
    functions = sortOn fst (if owned then ownedSymbols ++ [(timerInsert domain, fromIntegral (B.length timerStub)), (timerRemove domain, fromIntegral (B.length removeStub))] else extents domain)
    wordsFor ident = if regionName ident `elem` ["kernel", "peer-kernel", "definitions"] then layoutWords facts ++ caseWords one else caseWords one

readWord :: [Page] -> NativeObservation -> String -> Word32 -> Maybe Word32
readWord pages' observation name address = case
  [(base, bytes) | page@(Page (RegionId ident) (GuestAddress base) _ _ _) <- pages', ident == name, contains (GuestAddress address) 4 page, Just bytes <- [lookup (RegionId name) (nativeMemory observation)]] of
  (base, bytes) : _ -> Just (readWordAt bytes (fromIntegral (address - base)))
  [] -> Nothing

readByte :: [Page] -> NativeObservation -> String -> Word32 -> Maybe Word8
readByte pages' observation name address = case
  [(base, bytes) | page@(Page (RegionId ident) (GuestAddress base) _ _ _) <- pages', ident == name, contains (GuestAddress address) 1 page, Just bytes <- [lookup (RegionId name) (nativeMemory observation)]] of
  (base, bytes) : _ -> Just (B.index bytes (fromIntegral (address - base)))
  [] -> Nothing

regionBytes :: NativeObservation -> String -> Maybe B.ByteString
regionBytes observation name = lookup (RegionId name) (nativeMemory observation)

holds :: Facts -> Domain -> [Page] -> NativeObservation -> Expect -> Either String ()
holds _ _ pages' observation expectation = case expectation of
  WordIs region address value -> case readWord pages' observation region address of
    Just actual | actual == value -> Right ()
    Just actual -> Left (region ++ " " ++ show address ++ " is " ++ show actual ++ ", expected " ++ show value)
    Nothing -> Left ("missing " ++ region ++ " " ++ show address)
  ByteIs region address value -> case readByte pages' observation region address of
    Just actual | actual == value -> Right ()
    Just actual -> Left (region ++ " byte " ++ show address ++ " is " ++ show actual ++ ", expected " ++ show value)
    Nothing -> Left ("missing " ++ region ++ " byte " ++ show address)
  ResultIs value -> case N.lookupRegister R0 observation of
    Just actual | actual == value -> Right ()
    Just actual -> Left ("r0 is " ++ show actual ++ ", expected " ++ show value)
    Nothing -> Left "missing r0"

compareCase :: Facts -> Domain -> Case -> NativeObservation -> NativeObservation -> [String]
compareCase facts domain one stock owned = statusDifferences ++ resultDifferences ++ expectedDifferences ++ regionDifferences ++ registerDifferences ++ timerDifferences
  where
    pages' = pages facts domain one
    expectedStatus = case caseArrival one of
      Just _ -> StopAddress
      Nothing -> Returned
    returned observation = nativeStatus observation == expectedStatus
    valid = returned stock && returned owned
    statusDifferences =
      [ role ++ ": status " ++ runnerStatusLabel (nativeStatus observation) ++ " at pc " ++ show (N.lookupRegister Pc observation) ++ ", expected " ++ runnerStatusLabel expectedStatus
      | (role, observation) <- [("stock", stock), ("owned", owned)]
      , not (returned observation)
      ]
    expectedDifferences =
      [ role ++ ": " ++ message
      | valid
      , (role, observation, checks) <-
          [ ("stock", stock, if null (caseStockExpect one) then caseExpect one else caseStockExpect one)
          , ("owned", owned, if null (caseStockExpect one) then caseExpect one else caseOwnedExpect one)
          ]
      , Left message <- [sequence [holds facts domain pages' observation check | check <- checks]]
      ]
    resultDifferences =
      [ "r0 differs: stock " ++ show (N.lookupRegister R0 stock) ++ ", owned " ++ show (N.lookupRegister R0 owned)
      | valid
      , caseArrival one == Nothing
      , N.lookupRegister R0 stock /= N.lookupRegister R0 owned
      ]
    regionDifferences =
      [ "region " ++ name ++ " differs"
      | valid
      , caseArrival one == Nothing
      , null (caseStockExpect one)
      , name <- ["kernel", "ring", "out"]
      , regionBytes stock name /= regionBytes owned name
      ]
    registerDifferences = case caseArrival one of
      Just (stop, target) ->
        [ role ++ ": pc " ++ show (N.lookupRegister Pc observation) ++ ", expected " ++ show stop
        | (role, observation) <- [("stock", stock), ("owned", owned)]
        , N.lookupRegister Pc observation /= Just stop
        ]

      Nothing ->
        [ "caller register " ++ show register ++ " differs"
        | valid
        , register <- [R4 .. R11]
        , N.lookupRegister register stock /= N.lookupRegister register owned
        ]
          ++ [ "caller SP differs" | valid, N.lookupRegister Sp stock /= N.lookupRegister Sp owned ]
    timerDifferences =
      [ "timer remove receipt differs"
      | valid
      , caseTimerRemove one
      , let receipt name observation = (readWord pages' observation "timer-stub" (stubBase + 0x10), readWord pages' observation "timer-stub" (stubBase + 0x14))
      , receipt "stock" stock /= (Nothing, Nothing)
      , receipt "owned" owned /= (Just (contextRecord facts domain domain), Just (dataQueueTimer facts domain 2))
      ]

runDomain :: FilePath -> FilePath -> FilePath -> Facts -> Domain -> Image -> Image -> [(Word32, Word32)] -> IO [CaseRun]
runDomain runner cacheRoot directory facts domain stockImage ownedImage ownedSymbols = do
  let selected = cases facts domain
      stockSpecs = [(one, caseFor facts domain False [] stockImage one) | one <- selected]
      ownedSpecs = [(one, caseFor facts domain True ownedSymbols ownedImage one) | one <- selected]
  stockBatch <- writeBatch (directory </> "stock") (map snd stockSpecs)
  ownedBatch <- writeBatch (directory </> "owned") (map snd ownedSpecs)
  (stockReuse, stockRaw) <- cachedObservedBatch cacheRoot runner stockBatch
  (ownedReuse, ownedRaw) <- cachedObservedBatch cacheRoot runner ownedBatch
  let reference = M.fromList [(caseIdText ident, observation) | (ident, observation) <- stockRaw]
      subject = M.fromList [(caseIdText ident, observation) | (ident, observation) <- ownedRaw]
  results <- forM selected $ \one -> do
    let ident = caseIdText (caseName one) ++ "-" ++ domainLabel domain
    pure $ case (M.lookup ident reference, M.lookup ident subject) of
      (Just original, Just replacement) -> CaseRun domain (caseIdText (caseName one)) [] (compareCase facts domain one original replacement) (BatchExecuted, BatchExecuted)
      (Nothing, _) -> CaseRun domain (caseIdText (caseName one)) [] ["stock case did not run"] (BatchExecuted, BatchExecuted)
      (_, Nothing) -> CaseRun domain (caseIdText (caseName one)) [] ["owned case did not run"] (BatchExecuted, BatchExecuted)
  unless (length stockRaw == length selected && length ownedRaw == length selected) (fail "native observations do not cover the current data-queue case set")
  pure [CaseRun (runEntryDomain run) (runName run) (runChecks run) (runDifferences run) (stockReuse, ownedReuse) | run <- results]
