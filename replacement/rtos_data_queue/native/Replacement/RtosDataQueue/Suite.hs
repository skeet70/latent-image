{-# LANGUAGE OverloadedStrings #-}
module Replacement.RtosDataQueue.Suite
  ( Image (..), CaseRun (..), ControlRun (..), extents, runDomain ) where

import Control.Monad (forM, unless)
import Data.Aeson (FromJSON (..), eitherDecodeStrict', withObject, (.:), (.:?))
import Data.Bits ((.&.), (.|.), complement, shiftL, shiftR)
import qualified Data.ByteString as B
import qualified Data.Map.Strict as M
import Data.List (intercalate, isPrefixOf, isSuffixOf, sortOn)
import Data.Maybe (fromMaybe)
import Data.Word (Word32, Word8)
import Emulation.CachedNativeBatch (BatchReuse (..), cachedObservedBatch)
import Emulation.NativeCase (CaseId (..), caseIdText)
import qualified Emulation.NativeObservation as N
import Emulation.NativeObservation (Barrier (..), KernelObservation (..), NativeObservation (..), Ordered (..), RegionId (..), Register (..), RunnerStatus (..), Write (..), WriteValue (..), regionName, runnerStatusLabel)
import Emulation.OwnedCase (CaseOutcome (..), CaseSpec (..), KernelCase (..), Permission (..), assemble, caseOutcome, mkRegion, mkStackContext, plainLimit, readWordAt, word32le, writeBatch)
import Replacement.RtosDataQueue.Layout
import Replacement.RtosOwnFirst.Regions (Domain (..), GuestAddress (..), domainLabel)
import System.FilePath ((</>))

data Image = Image { imageBase :: Word32, imageBytes :: B.ByteString }

data Call = Create | Delete | Send | SendPoll | SendPollHandler | SendTimeout
          | ForceSend | ForceSendHandler | Receive | ReceivePoll | ReceiveTimeout
  deriving (Eq, Show)

data Expect = WordIs String Word32 Word32 | ByteIs String Word32 Word8 | MaskedIs String Word32 Word32 Word32
            | ResultIs Word32 | CpsrIs Word32
  deriving (Eq, Show)

-- | A second task the scheduler may select while the caller blocks.
data Actor = Actor { actorCode :: B.ByteString }

data Case = Case
  { caseName :: CaseId
  , caseCall :: Call
  , caseArgs :: [(Register, Word32)]
  , caseWords :: [(Word32, Word32)]
  , caseExpect :: [Expect]
  , caseStockExpect :: [Expect]
  , caseOwnedExpect :: [Expect]
  , caseCompareRegions :: Bool
  , caseHandler :: Bool
  , caseActor :: Maybe Actor
  }

timedRowNames, timeoutRowNames :: [String]
timedRowNames =
  [ "send-timeout-blocks"
  , "receive-timeout-blocks"
  , "send-timeout-wake"
  , "receive-timeout-wake"
  ]
timeoutRowNames = ["send-timeout-blocks", "receive-timeout-blocks"]

caseNameText :: Case -> String
caseNameText = caseIdText . caseName

data CaseRun = CaseRun
  { runEntryDomain :: Domain
  , runName :: String
  , runBlocking :: Bool
  , runChecks :: [(String, Bool)]
  , runDifferences :: [String]
  , runReuse :: (BatchReuse, BatchReuse)
  }

data ControlRun = ControlRun
  { controlDomain :: Domain
  , controlName :: String
  , controlCase :: String
  , controlBaseline :: Bool
  , controlRefused :: Bool
  , controlGuardNeeded :: Bool
  , controlHolds :: Bool
  }

data Page = Page RegionId GuestAddress Int Permission B.ByteString

ringBase, outBase, packetBase, stackBase, stackSize, stackPointer, exitAddress :: Word32
ringBase = 0x40000000
outBase = 0x40001000
packetBase = 0x40002000
stackBase = 0x00300000
stackSize = 0x1000
stackPointer = stackBase + 0x800
exitAddress = 0x01000200

stopAddress :: Word32
stopAddress = 0x01000300

actorBase, recordBase, wrapperBase, actorStackLow, actorStackHigh, callerStackLow, clockValue :: Word32
actorBase = 0x01000400
recordBase = 0x01000600
wrapperBase = 0x01000800
actorStackLow = 0x00300c00
actorStackHigh = 0x00301000
callerStackLow = 0x00300400
clockValue = 0x1000

recordSize, wrapperSize :: Int
recordSize = 0x100
wrapperSize = 0x100

armedCountOff, armedExpiryOff, armedEntryOff, armedIndexOff, armedCallbackOff, armedTaskOff :: Word32
armedCountOff = 0
armedExpiryOff = 4
armedEntryOff = 8
armedIndexOff = 12
armedCallbackOff = 16
armedTaskOff = 20

afterLockOff, afterCountOff, afterBaseOff, afterTickOff :: Word32
afterLockOff = 24
afterCountOff = 28
afterBaseOff = 32
afterTickOff = 36

cbCpsrOff, cbLockOff, cbCountOff, cbBaseOff, cbTickOff, cbEntryOff, cbIndexOff, cbCallsOff :: Word32
cbCpsrOff = 40
cbLockOff = 44
cbCountOff = 48
cbBaseOff = 52
cbTickOff = 56
cbEntryOff = 60
cbIndexOff = 64
cbCallsOff = 68

seedRegister :: Register -> Word32
seedRegister register = 0xa5a50000 + fromIntegral (fromEnum register)

callerBytes :: B.ByteString
callerBytes = B.concat [word32le (0xca11e500 + value) | value <- [0 .. 15]]

-- | The owned timer module sits at its stock extents; the first word of each entry is a branch.
timerModuleStart, timerModuleSpan, timerModuleBase, timerModuleSize :: Domain -> Word32
timerModuleStart DomainA = 0x44a50
timerModuleStart DomainB = 0xd4a10
timerModuleSpan _ = 0x4dc
timerModuleBase DomainA = 0x44ac0
timerModuleBase DomainB = 0xd4a80
timerModuleSize _ = 0x46c

tickEntry, saveAddress, acquireAddress, releaseAddress, timeoutCallback :: Domain -> Word32
tickEntry DomainA = 0x44d8c
tickEntry DomainB = 0xd4d4c
saveAddress DomainA = 0x43640
saveAddress DomainB = 0xd3640
acquireAddress DomainA = 0x439d8
acquireAddress DomainB = 0xd398c
releaseAddress DomainA = 0x43a14
releaseAddress DomainB = 0xd39c8
timeoutCallback DomainA = 0x44318
timeoutCallback DomainB = 0xd42d8

timeoutCallbackSize :: Int
timeoutCallbackSize = 0x1cc

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

extents :: Domain -> [(Word32, Word32)]
extents domain =
  [ (entryAddress domain call, entrySize call) | call <- [Create, Delete, Send, SendPoll, SendPollHandler, SendTimeout, ForceSend, ForceSendHandler, Receive, ReceivePoll, ReceiveTimeout] ]
    ++ helperFunctions domain
    ++ [(timerModuleBase domain, timerModuleSize domain)]

helperFunctions :: Domain -> [(Word32, Word32)]
helperFunctions DomainA =
  [ (0x4252c, 0x28), (0x42560, 0x68)
  , (0x43640, 0x28), (0x43668, 0x20), (0x43688, 0xa0), (0x43728, 0x34), (0x4375c, 0x48)
  , (0x437a4, 0x38), (0x437fc, 0x14), (0x439d8, 0x3c), (0x43a14, 0x28), (0x43c48, 0x5c)
  , (0x43ca4, 0x7c), (0x43d20, 0xb0), (0x43dd0, 0x48), (0x43f70, 0x2c), (0x44018, 0x2c)
  , (0x44044, 0xf0), (0x44134, 0x80), (0x44318, 0x1cc), (0x444e4, 0x84), (0x44568, 0xcc)
  , (0x4471c, 0xc8), (0x447e4, 0x1c), (0x44800, 0xe4), (0x448e4, 0xe8)
  , (0x48edc, 0x68), (0x48f44, 0x60), (0x48fa4, 0x54)
  ]
helperFunctions DomainB =
  [ (0xd252c, 0x28), (0xd2560, 0x68)
  , (0xd3640, 0x28), (0xd3668, 0x20), (0xd3688, 0xa0), (0xd3728, 0x34), (0xd375c, 0x48)
  , (0xd37a4, 0x38), (0xd37fc, 0x14), (0xd398c, 0x3c), (0xd39c8, 0x28), (0xd3c04, 0x5c)
  , (0xd3c60, 0x7c), (0xd3cdc, 0xb0), (0xd3d8c, 0x48), (0xd3f2c, 0x2c), (0xd3fd4, 0x30)
  , (0xd4004, 0xf0), (0xd40f4, 0x80), (0xd42d8, 0x1cc), (0xd44a4, 0x84), (0xd4528, 0xcc)
  , (0xd46dc, 0xc8), (0xd47a4, 0x1c), (0xd47c0, 0xe4), (0xd48a4, 0xe8)
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

-- Instruction encoders for the fixture actors and the timer instrument.
movSmall, movw, movt, movReg, movImm :: Word32 -> Word32 -> Word32
ldrI, strI, addImm :: Word32 -> Word32 -> Word32 -> Word32
mrsCpsr :: Word32 -> Word32
movSmall register value = 0xe3a00000 .|. (register `shiftL` 12) .|. (value .&. 0xff)
movw register value = 0xe3000000 .|. ((value `shiftR` 12) `shiftL` 16) .|. (register `shiftL` 12) .|. (value .&. 0xfff)
movt register value = 0xe3400000 .|. ((value `shiftR` 12) `shiftL` 16) .|. (register `shiftL` 12) .|. (value .&. 0xfff)
ldrI register base offset = 0xe5900000 .|. (base `shiftL` 16) .|. (register `shiftL` 12) .|. (offset .&. 0xfff)
strI register base offset = 0xe5800000 .|. (base `shiftL` 16) .|. (register `shiftL` 12) .|. (offset .&. 0xfff)
addImm register base value = 0xe2800000 .|. (base `shiftL` 16) .|. (register `shiftL` 12) .|. (value .&. 0xfff)
movReg register source = 0xe1a00000 .|. (register `shiftL` 12) .|. source
movImm register value = 0xe3a00000 .|. (register `shiftL` 12) .|. (value .&. 0xff)
mrsCpsr register = 0xe10f0000 .|. (register `shiftL` 12)

cpsieI, cpsidI, pushRegs, popRegs :: Word32
cpsieI = 0xf1080080
cpsidI = 0xf10c0080
pushRegs = 0xe92d48f0
popRegs = 0xe8bd88f0

data Item = Insn Word32 | JumpAbs Word32 | CallAbs Word32

build :: Word32 -> [Item] -> B.ByteString
build base items = assemble (go base items)
  where
    go _ [] = []
    go address (Insn word : rest) = word : go (address + 4) rest
    go address (JumpAbs target : rest) = jumpAt address target : go (address + 4) rest
    go address (CallAbs target : rest) = linkAt address target : go (address + 4) rest
    linkAt here target = 0xeb000000 .|. (((target - here - 8) `shiftR` 2) .&. 0xffffff)
    jumpAt here target = 0xea000000 .|. (((target - here - 8) `shiftR` 2) .&. 0xffffff)

-- | Reads the armed membership, patches the entry callback to the boundary instrument, sets the
-- | clock to the deadline, then runs the domain's tick and dispatches the released caller.
timeoutActor :: Facts -> Domain -> Word32 -> Actor
timeoutActor facts domain expiry = Actor (build actorBase
  [ Insn cpsieI
  , Insn (movw 0 (countAddr .&. 0xffff)), Insn (movt 0 (countAddr `shiftR` 16))
  , Insn (ldrI 1 0 0)
  , Insn (movw 2 (recordBase .&. 0xffff)), Insn (movt 2 (recordBase `shiftR` 16))
  , Insn (strI 1 2 armedCountOff)
  , Insn (movw 0 (contextAddr .&. 0xffff)), Insn (movt 0 (contextAddr `shiftR` 16))
  , Insn (ldrI 3 0 0x34)
  , Insn (ldrI 1 3 0)
  , Insn (strI 1 2 armedExpiryOff)
  , Insn (ldrI 1 3 4)
  , Insn (strI 1 2 armedEntryOff)
  , Insn (ldrI 4 1 0)
  , Insn (strI 4 2 armedIndexOff)
  , Insn (ldrI 4 1 4)
  , Insn (strI 4 2 armedCallbackOff)
  , Insn (ldrI 4 1 8)
  , Insn (strI 4 2 armedTaskOff)
  , Insn (movw 4 (wrapperBase .&. 0xffff)), Insn (movt 4 (wrapperBase `shiftR` 16))
  , Insn (strI 4 1 4)
  , Insn (movw 4 (handlerAddr .&. 0xffff)), Insn (movt 4 (handlerAddr `shiftR` 16))
  , Insn (movImm 5 1)
  , Insn (strI 5 4 0)
  , Insn (movw 4 (tickAddr .&. 0xffff)), Insn (movt 4 (tickAddr `shiftR` 16))
  , Insn (movw 5 (expiry .&. 0xffff)), Insn (movt 5 (expiry `shiftR` 16))
  , Insn (strI 5 4 0)
  , Insn (movImm 0 0)
  , Insn (movImm 1 0)
  , Insn (movImm 2 0)
  , CallAbs (tickEntry domain)
  , Insn (movw 4 (recordBase .&. 0xffff)), Insn (movt 4 (recordBase `shiftR` 16))
  , Insn (movw 0 (lockAddr .&. 0xffff)), Insn (movt 0 (lockAddr `shiftR` 16))
  , Insn (ldrI 1 0 0), Insn (strI 1 4 afterLockOff)
  , Insn (movw 0 (countAddr .&. 0xffff)), Insn (movt 0 (countAddr `shiftR` 16))
  , Insn (ldrI 1 0 0), Insn (strI 1 4 afterCountOff)
  , Insn (movw 0 (baseAddr .&. 0xffff)), Insn (movt 0 (baseAddr `shiftR` 16))
  , Insn (ldrI 1 0 0), Insn (strI 1 4 afterBaseOff)
  , Insn (movw 0 (tickAddr .&. 0xffff)), Insn (movt 0 (tickAddr `shiftR` 16))
  , Insn (ldrI 1 0 0), Insn (strI 1 4 afterTickOff)
  , Insn (movw 0 (handlerAddr .&. 0xffff)), Insn (movt 0 (handlerAddr `shiftR` 16))
  , Insn (movImm 1 0), Insn (strI 1 0 0)
  , Insn cpsidI
  , Insn (movImm 0 0)
  , CallAbs (saveAddress domain)
  , JumpAbs stopAddress
  ])
  where
    words' = domainFacts facts domain
    countAddr = dfTimerCount words'
    baseAddr = dfTimerBase words'
    tickAddr = dfTickNow words'
    lockAddr = dfSchedulerLock words'
    handlerAddr = dfHandlerContext words'
    contextAddr = contextRecord facts domain domain

-- | Reads the armed membership, then calls the stock service entry that releases the caller.
wakeActor :: Facts -> Domain -> Call -> Word32 -> Word32 -> Actor
wakeActor facts domain call id argument = Actor (build actorBase
  [ Insn cpsieI
  , Insn (movw 0 (countAddr .&. 0xffff)), Insn (movt 0 (countAddr `shiftR` 16))
  , Insn (ldrI 1 0 0)
  , Insn (movw 2 (recordBase .&. 0xffff)), Insn (movt 2 (recordBase `shiftR` 16))
  , Insn (strI 1 2 armedCountOff)
  , Insn (movw 0 (contextAddr .&. 0xffff)), Insn (movt 0 (contextAddr `shiftR` 16))
  , Insn (ldrI 3 0 0x34)
  , Insn (ldrI 1 3 0)
  , Insn (strI 1 2 armedExpiryOff)
  , Insn (ldrI 1 3 4)
  , Insn (strI 1 2 armedEntryOff)
  , Insn (ldrI 4 1 0)
  , Insn (strI 4 2 armedIndexOff)
  , Insn (ldrI 4 1 4)
  , Insn (strI 4 2 armedCallbackOff)
  , Insn (ldrI 4 1 8)
  , Insn (strI 4 2 armedTaskOff)
  , Insn (movw 4 (wrapperBase .&. 0xffff)), Insn (movt 4 (wrapperBase `shiftR` 16))
  , Insn (strI 4 1 4)
  , Insn (movSmall 0 id)
  , Insn (movw 1 (argument .&. 0xffff)), Insn (movt 1 (argument `shiftR` 16))
  , CallAbs (entryAddress domain call)
  , JumpAbs stopAddress
  ])
  where
    words' = domainFacts facts domain
    countAddr = dfTimerCount words'
    contextAddr = contextRecord facts domain domain

-- | Records the callback boundary, then calls the real timeout callback with its task argument.
wrapperBytes :: Facts -> Domain -> B.ByteString
wrapperBytes facts domain = build wrapperBase
  [ Insn pushRegs
  , Insn (movReg 7 0)
  , Insn (mrsCpsr 4)
  , Insn (movw 5 (recordBase .&. 0xffff)), Insn (movt 5 (recordBase `shiftR` 16))
  , Insn (strI 4 5 cbCpsrOff)
  , Insn (movw 6 (lockAddr .&. 0xffff)), Insn (movt 6 (lockAddr `shiftR` 16))
  , Insn (ldrI 4 6 0), Insn (strI 4 5 cbLockOff)
  , Insn (movw 6 (countAddr .&. 0xffff)), Insn (movt 6 (countAddr `shiftR` 16))
  , Insn (ldrI 4 6 0), Insn (strI 4 5 cbCountOff)
  , Insn (movw 6 (baseAddr .&. 0xffff)), Insn (movt 6 (baseAddr `shiftR` 16))
  , Insn (ldrI 4 6 0), Insn (strI 4 5 cbBaseOff)
  , Insn (movw 6 (tickAddr .&. 0xffff)), Insn (movt 6 (tickAddr `shiftR` 16))
  , Insn (ldrI 4 6 0), Insn (strI 4 5 cbTickOff)
  , Insn (ldrI 4 7 0x14)
  , Insn (ldrI 4 4 0)
  , Insn (strI 4 5 cbEntryOff)
  , Insn (ldrI 4 4 0)
  , Insn (strI 4 5 cbIndexOff)
  , Insn (ldrI 4 5 cbCallsOff)
  , Insn (addImm 4 4 1)
  , Insn (strI 4 5 cbCallsOff)
  , Insn (movReg 0 7)
  , CallAbs (timeoutCallback domain)
  , Insn popRegs
  ]
  where
    words' = domainFacts facts domain
    countAddr = dfTimerCount words'
    baseAddr = dfTimerBase words'
    tickAddr = dfTickNow words'
    lockAddr = dfSchedulerLock words'

-- | The stock service entry that releases the blocked caller in the untimed blocking rows.
serviceActor :: Domain -> Call -> Word32 -> Word32 -> Actor
serviceActor domain call id argument = Actor (assemble [cpsieI, movSmall 0 id, movw 1 (argument .&. 0xffff), movt 1 (argument `shiftR` 16), branchLink 4 (entryAddress domain call), branch 5 stopAddress])

branchLink, branch :: Int -> Word32 -> Word32
branchLink index target = 0xeb000000 .|. (((target - actorBase - fromIntegral index * 4 - 8) `shiftR` 2) .&. 0xffffff)
branch index target = 0xea000000 .|. (((target - actorBase - fromIntegral index * 4 - 8) `shiftR` 2) .&. 0xffffff)

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
  , blocking "send-blocks" Send [(R0, 1), (R1, 0x1234)] (queue 1 0 1 ++ full [0xaaaa] 1)
      (serviceActor executing Receive 1 outBase)
      [ ResultIs 0
      , eq "out" outBase 0xaaaa, eq "ring" ringBase 0x1234
      , eq "kernel" (count 1) 1, eq "kernel" (read 1) 0, eq "kernel" (write 1) 0
      , eq "kernel" (waitResult 1) 0, eq "kernel" (waitData 1) 0x1234
      , eq "kernel" (waitQueue 1) (sendSelf 1), eq "kernel" (waitOwner 1) (dfId (domainFacts facts executing))
      , eq "kernel" (sendNext 1) (sendSelf 1), eq "kernel" (sendPrev 1) (sendSelf 1)
      , ByteIs "kernel" (taskState 1) (stateReady facts)
      , eq "kernel" objectLock 0, eq "kernel" schedulerLock 0
      , CpsrIs 0x13
      ]
  , blocking "receive-blocks" Receive [(R0, 1), (R1, outBase)] (queue 1 0 2 ++ [poke outBase 0x5e4d1e00])
      (serviceActor executing Send 1 0x4321)
      [ ResultIs 0
      , eq "out" outBase 0x4321
      , eq "kernel" (count 1) 0, eq "kernel" (read 1) 0, eq "kernel" (write 1) 0
      , eq "kernel" (waitResult 1) 0, eq "kernel" (waitData 1) 0x4321
      , eq "kernel" (waitQueue 1) (sendSelf 1), eq "kernel" (waitOwner 1) (dfId (domainFacts facts executing))
      , eq "kernel" (recvNext 1) (recvSelf 1), eq "kernel" (recvPrev 1) (recvSelf 1)
      , ByteIs "kernel" (taskState 1) (stateReady facts)
      , eq "kernel" objectLock 0, eq "kernel" schedulerLock 0
      , CpsrIs 0x13
      ]
  , timed "send-timeout-blocks" SendTimeout [(R0, 1), (R1, 0x2222), (R2, 5)] (queue 1 0 1 ++ full [0xaaaa] 1)
      (timeoutActor facts executing (clockValue + 5))
      (timeoutChecks (-50) [eq "ring" ringBase 0xaaaa, eq "kernel" (count 1) 1, eq "kernel" (read 1) 0, eq "kernel" (write 1) 0, eq "kernel" (waitData 1) 0x2222, eq "kernel" (sendNext 1) (sendSelf 1), eq "kernel" (sendPrev 1) (sendSelf 1)])
      [recordEq cbIndexOff 1, eq "kernel" (timerEntry 1) 1]
      [recordEq cbIndexOff 0, eq "kernel" (timerEntry 1) 0]
  , timed "receive-timeout-blocks" ReceiveTimeout [(R0, 1), (R1, outBase), (R2, 5)] (queue 1 0 2 ++ [poke outBase 0x5e4d1e00])
      (timeoutActor facts executing (clockValue + 5))
      (timeoutChecks (-50) [eq "out" outBase 0x5e4d1e00, eq "kernel" (count 1) 0, eq "kernel" (read 1) 0, eq "kernel" (write 1) 0, eq "kernel" (recvNext 1) (recvSelf 1), eq "kernel" (recvPrev 1) (recvSelf 1)])
      [recordEq cbIndexOff 1, eq "kernel" (timerEntry 1) 1]
      [recordEq cbIndexOff 0, eq "kernel" (timerEntry 1) 0]
  , timed "send-timeout-wake" SendTimeout [(R0, 1), (R1, 0x3333), (R2, 5)] (queue 1 0 1 ++ full [0xbbbb] 1)
      (wakeActor facts executing Receive 1 outBase)
      (wakeChecks 0 [eq "out" outBase 0xbbbb, eq "ring" ringBase 0x3333, eq "kernel" (count 1) 1, eq "kernel" (read 1) 0, eq "kernel" (write 1) 0, eq "kernel" (sendNext 1) (sendSelf 1), eq "kernel" (sendPrev 1) (sendSelf 1)])
      [eq "kernel" (timerEntry 1) 1]
      [eq "kernel" (timerEntry 1) 0]
  , timed "receive-timeout-wake" ReceiveTimeout [(R0, 1), (R1, outBase), (R2, 5)] (queue 1 0 2 ++ [poke outBase 0x5e4d1e00])
      (wakeActor facts executing Send 1 0x4444)
      (wakeChecks 0 [eq "out" outBase 0x4444, eq "kernel" (count 1) 0, eq "kernel" (read 1) 0, eq "kernel" (write 1) 0, eq "kernel" (waitData 1) 0x4444, eq "kernel" (recvNext 1) (recvSelf 1), eq "kernel" (recvPrev 1) (recvSelf 1)])
      [eq "kernel" (timerEntry 1) 1]
      [eq "kernel" (timerEntry 1) 0]
  , difference "delete-stranded-receiver" Delete [(R0, 1)] (queue 1 0 2 ++ receiverWaiter)
      [ResultIs 0, eq "kernel" (configExists 1) 0]
      [ResultIs 0, eq "kernel" (configExists 1) 0, eq "kernel" (recvNext 1) (recvSelf 1), eq "kernel" (waitResult 2) 0, ByteIs "kernel" (taskState 2) (stateObjectQueued facts .|. stateWait facts)]
      [ResultIs 0, eq "kernel" (configExists 1) 0, eq "kernel" (recvNext 1) (recvSelf 1), eq "kernel" (waitResult 2) (-49), ByteIs "kernel" (taskState 2) (stateReady facts)]
  , difference "delete-timed-waiter" Delete [(R0, 1)] (queue 1 0 2 ++ timedReceiverWaiter)
      [ResultIs 0, eq "kernel" (configExists 1) 0]
      [ ResultIs 0, eq "kernel" (configExists 1) 0
      , eq "kernel" (dfTimerCount words') 1, eq "kernel" (dfTimerHeap words') (clockValue + 5)
      , eq "kernel" (dfTimerHeap words' + 4) (timerEntry 2), eq "kernel" (timerEntry 2) 1
      , eq "kernel" (waitResult 2) (timerEntry 2), ByteIs "kernel" (taskState 2) (stateObjectQueued facts .|. stateWait facts)
      ]
      [ ResultIs 0, eq "kernel" (configExists 1) 0
      , eq "kernel" (dfTimerCount words') 0, eq "kernel" (timerEntry 2) 0
      , eq "kernel" (waitResult 2) (-49), ByteIs "kernel" (taskState 2) (stateReady facts)
      ]
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
  , handler "handler-poll-handoff" SendPollHandler [(R0, 1), (R1, 0x5a5a)] (queue 1 0 2 ++ receiverWaiter ++ currentAtPriorityFive)
      [ ResultIs 0
      , eq "kernel" (waitData 2) 0x5a5a
      , ByteIs "kernel" (taskState 2) (stateReady facts)
      , eq "kernel" (recvNext 1) (recvSelf 1), eq "kernel" (recvPrev 1) (recvSelf 1)
      , eq "kernel" (dfReschedule (domainFacts facts executing)) 1
      , eq "kernel" (count 1) 0
      , eq "kernel" objectLock 0, eq "kernel" schedulerLock 0
      , CpsrIs 0x13
      ]
  , caseOf "timed-receive-poll" ReceiveTimeout [(R0, 1), (R1, outBase), (R2, 0)] (queue 1 0 2 ++ [poke outBase 0x5e4d1e00]) [ResultIs (-50), eq "out" outBase 0x5e4d1e00]
  , caseOf "timed-send-poll" SendTimeout [(R0, 1), (R1, 2), (R2, 0)] (queue 1 0 1 ++ full [0xaaaa] 1) [ResultIs (-50), eq "ring" ringBase 0xaaaa, eq "kernel" (count 1) 1]
  , caseOf "send-timeout-invalid" SendTimeout [(R0, 1), (R1, 2), (R2, -2)] (queue 1 0 2) [ResultIs (-17), eq "kernel" (count 1) 0]
  ]
  where
    words' = domainFacts facts executing
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
    full words'' count' = [poke (ringBase + fromIntegral index * 4) value | (index, value) <- zip [0 :: Int ..] words''] ++ [poke (count 1) count', poke (read 1) 0, poke (write 1) (count' `mod` fromIntegral (max 1 (length words'')))]
    receiverWaiter =
      let t = taskAt 2
          q = recvSelf 1
       in [ poke (recvNext 1) t, poke (recvPrev 1) t
          , poke (t + factsTaskNext facts) q, poke (t + factsTaskPrev facts) q
          , poke (t + factsTaskState facts) (packedState (stateObjectQueued facts .|. stateWait facts) 3)
          , poke (t + factsTaskWait facts) (wait 2)
          , poke (wait 2) 0, poke (waitQueue 2) q, poke (waitOwner 2) (contextRecord facts executing executing)
          ] ++ removeReady3
    timedReceiverWaiter = receiverWaiter
      ++ [ poke (wait 2) (timerEntry 2)
         , poke (timerEntry 2) 1
         , poke (timerEntry 2 + 4) (timeoutCallback executing)
         , poke (timerEntry 2 + 8) (taskAt 2)
         , poke (dfTimerCount words') 1
         , poke (dfTimerBase words') 0
         , poke (dfTickNow words') clockValue
         , poke (dfTimerHeap words') (clockValue + 5)
         , poke (dfTimerHeap words' + 4) (timerEntry 2)
         , poke (arenaBase facts executing) (clockValue + 5)
         , poke (arenaBase facts executing + 4) (timerEntry 2)
         ]
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
      let head3 = dfReadyHeads words' + 3 * 8
          bitmap = dfReadyBitmap words'
       in [poke head3 head3, poke (head3 + 4) head3, poke bitmap ((1 `shiftL` 0) .|. (1 `shiftL` 5))]
    -- Running task 1 READY at priority 5, alone in the ready set, so a released priority-3
    -- waiter outranks the selection.
    currentAtPriorityFive =
      let task1 = taskAt 1
          task3 = taskAt 3
          head0 = dfReadyHeads words'
          head5 = head0 + 5 * 8
       in [ poke head0 head0, poke (head0 + 4) head0
          , poke head5 task1, poke (head5 + 4) task1, poke task1 head5, poke (task1 + factsTaskPrev facts) head5
          , poke (task1 + factsTaskState facts) (packedState (stateReady facts) 5)
          , poke (dfReadyBitmap words') (1 `shiftL` 5)
          , poke (dfSelected words') task1, poke (dfRunning words') task1
          , poke task3 task3, poke (task3 + factsTaskPrev facts) task3
          , poke (task3 + factsTaskState facts) (packedState (factsStateDormant facts) 5)
          ]
    caseOf name call args words'' checks =
      Case (CaseId name) call args words'' checks [] [] True False Nothing
    difference name call args words'' common stockChecks ownedChecks =
      Case (CaseId name) call args words'' common stockChecks ownedChecks False False Nothing
    handler name call args words'' checks =
      Case (CaseId name) call args (words'' ++ [poke (dfHandlerContext words') 1]) checks [] [] True True Nothing
    blocking name call args words'' actor checks =
      Case (CaseId name) call args words'' checks [] [] True False (Just actor)
    timed name call args words'' actor checks stockChecks ownedChecks =
      Case (CaseId name) call args words'' checks stockChecks ownedChecks True False (Just actor)
    timeoutChecks result extra =
      [ ResultIs result
      , eq "kernel" (waitResult 1) result
      , eq "kernel" (waitQueue 1) (sendSelf 1)
      , ByteIs "kernel" (taskState 1) (stateReady facts)
      , eq "kernel" (dfTimerCount words') 0
      , recordEq armedCountOff 1
      , recordEq armedExpiryOff (clockValue + 5)
      , recordEq armedEntryOff (timerEntry 1)
      , recordEq armedIndexOff 1
      , recordEq armedCallbackOff (timeoutCallback executing)
      , recordEq armedTaskOff (taskAt 1)
      , recordEq afterLockOff 0
      , recordEq afterCountOff 0
      , recordEq afterBaseOff (clockValue + 5)
      , recordEq afterTickOff (clockValue + 15)
      , MaskedIs "timer-record" (recordBase + cbCpsrOff) 0x80 0x80
      , recordEq cbLockOff 1
      , recordEq cbCountOff 0
      , recordEq cbBaseOff 0
      , recordEq cbTickOff (clockValue + 5)
      , recordEq cbEntryOff (timerEntry 1)
      , recordEq cbCallsOff 1
      , eq "kernel" objectLock 0, eq "kernel" schedulerLock 0
      , CpsrIs 0x13
      ] ++ extra
    wakeChecks result extra =
      [ ResultIs result
      , eq "kernel" (waitResult 1) result
      , eq "kernel" (waitQueue 1) (sendSelf 1)
      , ByteIs "kernel" (taskState 1) (stateReady facts)
      , eq "kernel" (dfTimerCount words') 0
      , recordEq armedCountOff 1
      , recordEq armedExpiryOff (clockValue + 5)
      , recordEq armedEntryOff (timerEntry 1)
      , recordEq armedIndexOff 1
      , recordEq armedCallbackOff (timeoutCallback executing)
      , recordEq armedTaskOff (taskAt 1)
      , recordEq cbCallsOff 0
      , eq "kernel" objectLock 0, eq "kernel" schedulerLock 0
      , CpsrIs 0x13
      ] ++ extra
    timerEntry slot = dataQueueTimer facts executing slot
    recordEq offset value = eq "timer-record" (recordBase + offset) value
    objectLock = dfObjectLock words'
    schedulerLock = dfSchedulerLock words'
    peerDomain = peer executing
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

stateReady, stateWait, stateObjectQueued, stateOwnerLockQueued :: Facts -> Word8
stateReady = factsStateReady
stateWait = factsStateWait
stateObjectQueued = factsStateObjectQueued
stateOwnerLockQueued = factsStateOwnerLockQueued

layoutWords :: Facts -> Bool -> Case -> [(Word32, Word32)]
layoutWords facts owned one =
  concat [recordPokes block described | block <- domains, described <- domains]
    ++ concatMap taskPokes domains ++ concatMap objectPokes domains ++ concatMap readyPokes domains
    ++ [poke (dfTimerCount (domainFacts facts domain)) 0 | domain <- domains]
    ++ [poke (dfTimerBase (domainFacts facts domain)) 0 | domain <- domains]
    ++ [poke (dfTickNow (domainFacts facts domain)) clockValue | domain <- domains]
  where
    domains = [DomainA, DomainB]
    recordPokes block described =
      let record = contextRecord facts block described
          describedWords = domainFacts facts described
          (acquire, release) = lockEntries block
          lock offset address = [poke (record + offset + factsLockWord facts) address, poke (record + offset + factsLockAcquire facts) acquire, poke (record + offset + factsLockRelease facts) release]
       in [ poke (record + factsContextId facts) (if block == described then factsOwnId facts else dfId describedWords)
          , poke (record + factsContextOwner facts) (dfId describedWords)
          , poke (record + factsContextReadyHeads facts) (dfReadyHeads describedWords)
          , poke (record + factsContextReadyBitmap facts) (dfReadyBitmap describedWords)
          , poke (record + factsContextRunning facts) (dfRunning describedWords)
          , poke (record + factsContextSelected facts) (dfSelected describedWords)
          , poke (record + factsContextDispatchEnabled facts) (dfDispatchEnabled describedWords)
          , poke (record + factsContextTickNow facts) (dfTickNow describedWords)
          , poke (record + factsContextTimerHeap facts) (roleHeap described)
          , poke (record + factsContextTimerCount facts) (dfTimerCount describedWords)
          , poke (record + factsContextTimerBase facts) (dfTimerBase describedWords)
          , poke (record + factsContextFirstEntry facts) (trampoline described)
          , poke (record + factsContextTasks facts) (dfTaskTable describedWords)
          , poke (record + factsContextTaskCount facts) (factsTaskSlots facts)
          , poke (record + factsContextDataQueues facts) (dfDataQueueControls describedWords)
          , poke (record + factsContextDataQueueCount facts) 120
          ] ++ lock (factsContextSchedulerLock facts) (dfSchedulerLock describedWords) ++ lock (factsContextObjectLock facts) (dfObjectLock describedWords)
    roleHeap described = if owned then arenaBase facts described else dfTimerHeap (domainFacts facts described)
    taskPokes domain = concat
      [ let address = task facts domain slot
            definition' = definition facts domain slot
            priority = if slot == 1 then 0 else if slot == 2 then 3 else 5
            (savedSp, resumePc) = case (slot, caseActor one) of
              (2, Just _) -> (actorStackHigh, actorBase)
              _ -> (stackPointer, exitAddress)
         in [ poke address address, poke (address + factsTaskPrev facts) address
            , poke (address + factsTaskDefinition facts) definition'
            , poke (address + factsTaskDomain facts) (dfId (domainFacts facts domain))
            , poke (address + factsTaskState facts) (fromIntegral (factsStateDormant facts) .|. (priority `shiftL` 8))
            , poke (address + factsTaskSavedSp facts) savedSp
            , poke (address + factsTaskResumePc facts) resumePc
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
      let domainWords = domainFacts facts domain
          headAt priority = dfReadyHeads domainWords + priority * factsHeadSize facts
          member slot priority =
            let address = task facts domain slot
                headAddress = headAt priority
             in [poke headAddress address, poke (headAddress + factsHeadPrev facts) address, poke address headAddress, poke (address + factsTaskPrev facts) headAddress, poke (address + factsTaskState facts) (fromIntegral (factsStateReady facts) .|. (priority `shiftL` 8))]
       in concat [[poke (headAt priority) (headAt priority), poke (headAt priority + factsHeadPrev facts) (headAt priority)] | priority <- [0 .. factsPriorities facts - 1]]
            ++ member 1 0 ++ member 2 3 ++ member 3 5
            ++ [ poke (dfReadyBitmap domainWords) ((1 `shiftL` 0) .|. (1 `shiftL` 3) .|. (1 `shiftL` 5))
               , poke (dfRunning domainWords) (task facts domain 1), poke (dfSelected domainWords) (task facts domain 1)
               , poke (dfDispatchEnabled domainWords) 1, poke (dfSchedulerLock domainWords) 0, poke (dfObjectLock domainWords) 0
               , poke (dfCurrentContext domainWords) (contextRecord facts domain domain)
               , poke (dfHandlerContext domainWords) 0
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
      , dfTimerHeap words' + 0x1000
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

sliceImage :: Image -> Word32 -> Int -> B.ByteString
sliceImage image address size = B.take size (B.drop (fromIntegral (address - imageBase image)) (imageBytes image))

install :: Image -> Word32 -> B.ByteString -> B.ByteString -> B.ByteString
install image address bytes current =
  let offset = fromIntegral (address - imageBase image)
   in B.take offset current <> bytes <> B.drop (offset + B.length bytes) current

-- | The stock role runs the stock timer block; the owned role overlays the owned timer module at
-- | its stock extents and the stock timeout callback the family does not own.
codeBytes :: Facts -> Domain -> Bool -> Image -> Image -> Image -> B.ByteString
codeBytes _ domain owned timerImage image stockImage
  | not owned = imageBytes stockImage
  | otherwise =
      install image (timeoutCallback domain) (sliceImage stockImage (timeoutCallback domain) timeoutCallbackSize)
        (install image (timerModuleStart domain) (sliceImage timerImage (timerModuleStart domain) (fromIntegral (timerModuleSpan domain))) (imageBytes image))

pages :: Facts -> Domain -> Case -> [Page]
pages facts domain one =
  [ let (low, high) = kernelBounds facts domain in page "kernel" low (fromIntegral (high - low)) ReadWrite
  , let (low, high) = kernelBounds facts peerDomain in page "peer-kernel" low (fromIntegral (high - low)) ReadWrite
  , page "definitions" 0x0096c000 0x4000 ReadWrite
  , page "ring" ringBase 0x100 ReadWrite
  , page "out" outBase 0x10 ReadWrite
  , page "arena" (arenaBase facts domain) (fromIntegral (arenaBytes facts domain)) ReadWrite
  , page "timer-record" recordBase recordSize ReadWrite
  , pageWith "timer-wrapper" wrapperBase wrapperSize ReadExecute (wrapperBytes facts domain)
  , page "stack" (stackPointer - 0x400) 0x400 ReadWrite
  , page "actor-stack" actorStackLow (fromIntegral (actorStackHigh - actorStackLow)) ReadWrite
  , page "exit" exitAddress 0x10 ReadExecute
  , Page (RegionId "bank-a") (GuestAddress (fromIntegral (bank DomainA))) 4 Device (B.replicate 4 0)
  , Page (RegionId "bank-b") (GuestAddress (fromIntegral (bank DomainB))) 4 Device (B.replicate 4 0)
  , Page (RegionId "caller-frame") (GuestAddress (fromIntegral stackPointer)) (B.length callerBytes) ReadWrite callerBytes
  ]
    ++ [page "packet" packetBase 0x10 ReadWrite | caseCall one == Create]
    ++ [Page (RegionId "actor") (GuestAddress (fromIntegral actorBase)) (B.length code) ReadExecute code | Actor code <- maybe [] pure (caseActor one)]
  where
    peerDomain = peer domain
    page name address size permission = Page (RegionId name) (GuestAddress (fromIntegral address)) size permission (B.replicate size 0)
    pageWith name address size permission bytes = Page (RegionId name) (GuestAddress (fromIntegral address)) size permission bytes

initialRegisters :: Facts -> Domain -> Case -> [(Register, Word32)]
initialRegisters facts _ one =
  [(register, value register) | register <- [R0 .. R12]] ++ [(Sp, stackPointer), (Lr, exitAddress), (Cpsr, 0x13)]
  where
    value register = maybe (seedRegister register) id (lookup register (caseArgs one))

caseFor :: Facts -> Domain -> Bool -> Image -> [(Word32, Word32)] -> Image -> Image -> Case -> CaseSpec
caseFor facts domain owned timerImage ownedSymbols image stockImage one =
  CaseSpec
    { caseId = caseIdText (caseName one) ++ "-" ++ domainLabel domain
    , caseProvenance = "X-M1 1.50 data-queue stock ABI, x70view 55436fc85c0560dd252b0410af6f2427fe198cd3; synthetic queue, task, actor and timer-heap state"
    , caseExclusion = "Synthetic queue, task and timer-heap state. A second task calls the stock entry, runs the domain tick over the armed heap, or records the callback boundary and calls the stock timeout callback; no firmware integration."
    , caseEntry = N.GuestAddress (fromIntegral (entryAddress domain (caseCall one)))
    , caseExit = N.GuestAddress (fromIntegral exitAddress)
    , caseRegions = [mkRegion (RegionId "code") (N.GuestAddress (fromIntegral codeBase)) (fromIntegral (B.length code)) ReadExecute code]
        ++ [mkRegion ident (N.GuestAddress (fromIntegral address)) size permission (overlay address background (wordsFor owned ident)) | Page ident (GuestAddress address) size permission background <- dataPages]
    , caseFunctions = [(N.GuestAddress (fromIntegral address), size) | (address, size) <- functions]
    , caseInitial = initialRegisters facts domain one
    , caseExpectedStatus = Returned
    , caseExpectedRegisters = expectedRegisters one
    , caseExpectedMemory = []
    , caseCp14Identity = Just (if domain == DomainA then 0x4003 else 0x4004)
    , caseKernel = Just (KernelCase plainLimit Nothing (Just 0x80001234) (Just 0) (Just 0) [R0, R1, R2, R3, R12, Lr] stackContexts)
    }
  where
    dataPages = pages facts domain one
    code = codeBytes facts domain owned timerImage image stockImage
    codeBase = imageBase image
    actorFunction = [(actorBase, fromIntegral (B.length body)) | Actor body <- maybe [] pure (caseActor one)]
    wrapperFunction = [(wrapperBase, fromIntegral (B.length (wrapperBytes facts domain)))]
    functions = sortOn fst $
      if owned
        then ownedSymbols ++ actorFunction ++ wrapperFunction ++ [(timerModuleStart domain, timerModuleSpan domain), (timeoutCallback domain, fromIntegral timeoutCallbackSize)]
        else extents domain ++ actorFunction ++ wrapperFunction
    stackContexts = case caseActor one of
      Nothing -> []
      Just _ -> either (const []) id (sequence [mkStackContext (N.GuestAddress (fromIntegral callerStackLow)) (N.GuestAddress (fromIntegral stackPointer)), mkStackContext (N.GuestAddress (fromIntegral actorStackLow)) (N.GuestAddress (fromIntegral actorStackHigh))])
    wordsFor ownedRole ident = (if regionName ident `elem` ["kernel", "peer-kernel", "definitions"] then layoutWords facts ownedRole one else []) ++ caseWords one

-- | The returned caller's ABI: its result, its callee-saved registers and its stack pointer.
expectedRegisters :: Case -> [(Register, Word32)]
expectedRegisters one =
  [ (R0, value) | value : _ <- [[value | ResultIs value <- caseExpect one ++ caseStockExpect one]] ]
    ++ [(register, seedRegister register) | register <- [R4 .. R11]]
    ++ [(Sp, stackPointer)]

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
  MaskedIs region address mask value -> case readWord pages' observation region address of
    Just actual | actual .&. mask == value -> Right ()
    Just actual -> Left (region ++ " " ++ show address ++ " masked is " ++ show (actual .&. mask) ++ ", expected " ++ show value)
    Nothing -> Left ("missing " ++ region ++ " " ++ show address)
  ResultIs value -> case N.lookupRegister R0 observation of
    Just actual | actual == value -> Right ()
    Just actual -> Left ("r0 is " ++ show actual ++ ", expected " ++ show value)
    Nothing -> Left "missing r0"
  CpsrIs value -> case N.lookupRegister Cpsr observation of
    Just actual | actual .&. 0x1ff == value -> Right ()
    Just actual -> Left ("cpsr is " ++ show actual ++ ", expected control bits " ++ show value)
    Nothing -> Left "missing cpsr"

-- | Both roles against the declared expectations and each other. The runner's own report is
-- judged separately: its whole-region memory gate is not declared, so a report is only refused
-- for a status, register, event or kernel mismatch.
compareCase :: Facts -> Domain -> Case -> NativeObservation -> NativeObservation -> [String]
compareCase facts domain one stock owned = statusDifferences ++ declaredRegisterDifferences ++ expectedDifferences ++ resultDifferences ++ regionDifferences ++ registerDifferences
  where
    pages' = pages facts domain one
    expectedStatus = Returned
    returned observation = nativeStatus observation == expectedStatus
    valid = returned stock && returned owned
    statusDifferences =
      [ role ++ ": status " ++ runnerStatusLabel (nativeStatus observation) ++ " at pc " ++ show (N.lookupRegister Pc observation) ++ ", expected " ++ runnerStatusLabel expectedStatus
      | (role, observation) <- [("stock", stock), ("owned", owned)]
      , not (returned observation)
      ]
    declaredRegisterDifferences =
      [ role ++ ": " ++ N.registerLabel register ++ " is " ++ show actual ++ ", expected " ++ show value
      | (role, observation) <- [("stock", stock), ("owned", owned)]
      , (register, value) <- expectedRegisters one
      , actual <- [N.lookupRegister register observation]
      , actual /= Just value
      ]
    expectedDifferences =
      [ role ++ ": " ++ message
      | valid
      , (role, observation, checks) <-
          [ ("stock", stock, caseExpect one ++ caseStockExpect one)
          , ("owned", owned, caseExpect one ++ caseOwnedExpect one)
          ]
      , Left message <- [sequence [holds facts domain pages' observation check | check <- checks]]
      ]
    resultDifferences =
      [ "r0 differs: stock " ++ show (N.lookupRegister R0 stock) ++ ", owned " ++ show (N.lookupRegister R0 owned)
      | valid
      , N.lookupRegister R0 stock /= N.lookupRegister R0 owned
      ]
    regionDifferences =
      [ "region " ++ name ++ " differs"
      | valid
      , caseCompareRegions one
      , name <- ["kernel", "peer-kernel", "ring", "out", "timer-record"]
      , let exempt = if caseNameText one `elem` timedRowNames then [dataQueueTimer facts domain 1] else []
            view = case name of
              "kernel" -> fmap (normalizeKernel facts domain exempt)
              "peer-kernel" -> fmap (normalizeKernel facts (peer domain) [])
              "timer-record" -> fmap (normalizeRecord facts one)
              _ -> id
      , view (regionBytes stock name) /= view (regionBytes owned name)
      ]
        ++ (if caseCompareRegions one then relocatedHeapDifferences facts domain stock owned else [])
    registerDifferences =
      [ "caller register " ++ show register ++ " differs"
      | valid
      , register <- [R4 .. R11]
      , N.lookupRegister register stock /= N.lookupRegister register owned
      ]
        ++ [ "caller SP differs" | valid, N.lookupRegister Sp stock /= N.lookupRegister Sp owned ]
        ++ [ "caller control bits differ" | valid, fmap (.&. 0x1ff) (N.lookupRegister Cpsr stock) /= fmap (.&. 0x1ff) (N.lookupRegister Cpsr owned) ]

-- | Zero the role-specific words a comparison must not read: saved stack pointers, context heap
-- pointers, the declared exempt timer indices and the stock heap span.
normalizeKernel :: Facts -> Domain -> [Word32] -> B.ByteString -> B.ByteString
normalizeKernel facts domain exempt bytes = maskHeap (foldl clear bytes addresses)
  where
    (low, _) = kernelBounds facts domain
    addresses = [task facts domain slot + factsTaskSavedSp facts | slot <- [1 .. 3]]
      ++ [contextRecord facts domain described + factsContextTimerHeap facts | described <- [DomainA, DomainB]]
      ++ exempt
    clear current address = replaceWord (fromIntegral (address - low)) 0 current
    maskHeap current = maskRange (fromIntegral (dfTimerHeap (domainFacts facts domain) - low)) 0x5c0 current

-- | The callback index is exempt only where the case declares the stock/owned divergence; the
-- condition flags are incidental in every case.
normalizeRecord :: Facts -> Case -> B.ByteString -> B.ByteString
normalizeRecord _ one bytes =
  let withFlags = replaceWord (fromIntegral cbCpsrOff) (readWordAt bytes (fromIntegral cbCpsrOff) .&. 0x1ff) bytes
   in if caseNameText one `elem` timeoutRowNames
        then replaceWord (fromIntegral cbIndexOff) 0 withFlags
        else withFlags

-- | The stock heap and the owned arena hold the same nodes at different addresses.
relocatedHeapDifferences :: Facts -> Domain -> NativeObservation -> NativeObservation -> [String]
relocatedHeapDifferences facts domain stock owned =
  case (lookup (RegionId "kernel") (nativeMemory stock), lookup (RegionId "arena") (nativeMemory owned)) of
    (Just kernel, Just arena)
      | offset + heapLen <= B.length kernel ->
          [ "relocated timer heap differs" | B.take heapLen (B.drop offset kernel) /= B.take heapLen arena ]
    _ -> ["relocated timer heap region is missing"]
  where
    low = fst (kernelBounds facts domain)
    offset = fromIntegral (dfTimerHeap (domainFacts facts domain) - low)
    heapLen = 0x5c0

replaceWord :: Int -> Word32 -> B.ByteString -> B.ByteString
replaceWord offset value bytes
  | offset < 0 || offset + 4 > B.length bytes = bytes
  | otherwise = B.take offset bytes <> word32le value <> B.drop (offset + 4) bytes

maskRange :: Int -> Int -> B.ByteString -> B.ByteString
maskRange offset size bytes
  | offset < 0 || size < 0 || offset + size > B.length bytes = bytes
  | otherwise = B.take offset bytes <> B.replicate size 0 <> B.drop (offset + size) bytes

-- | The runner report's status and mismatches, as its judge wrote them.
data Report = Report { reportStatus :: String, reportMismatches :: [String] }

instance FromJSON Report where
  parseJSON = withObject "native report" $ \o ->
    Report <$> o .: "status" <*> (fromMaybe [] <$> o .:? "mismatches")

-- | The runner report's non-memory mismatches, which are the declarations the wire carries.
reportVerdicts :: FilePath -> String -> IO [String]
reportVerdicts directory ident = do
  let path = directory </> ident ++ ".native-result.json"
  bytes <- B.readFile path
  pure $ case eitherDecodeStrict' bytes of
    Left problem -> [path ++ ": " ++ problem]
    Right report ->
      let other = filter (not . memoryMismatch) (reportMismatches report)
       in [ path ++ ": " ++ reportStatus report ++ (if null other then "" else ": " ++ intercalate "; " other)
          | reportStatus report `notElem` ["passed", "failed"]
              || not (null other)
              || (reportStatus report == "failed" && null (reportMismatches report))
          ]

controlRuns :: Facts -> Domain -> Case -> NativeObservation -> NativeObservation -> [ControlRun]
controlRuns facts domain one stock owned = concat
  [ control "armed-membership" (guardEq armedCountOff 1) (mutateRecord armedCountOff (const 0))
  | name `elem` timedRowNames
  ]
    ++ concat
      [ control "callback-consumed" (guardEq cbCountOff 0) (mutateRecord cbCountOff (const 1))
          ++ control "callback-i-masked" (MaskedIs "timer-record" (recordBase + cbCpsrOff) 0x80 0x80) (mutateRecord cbCpsrOff (\current -> current .&. complement 0x80))
          ++ control "callback-lock-held" (guardEq cbLockOff 1) (mutateRecord cbLockOff (const 0))
          ++ control "released-lock" (guardEq afterLockOff 0) (mutateRecord afterLockOff (const 1))
      | name `elem` timeoutRowNames
      ]
    ++ concat [ control "callback-not-invoked" (guardEq cbCallsOff 0) (mutateRecord cbCallsOff (const 1))
              | name `elem` ["send-timeout-wake", "receive-timeout-wake"]
              ]
    ++ concat [ controlOwned "unaffected-membership" (\current -> current { caseCompareRegions = False }) (mutateKernel (dataQueueTimer facts domain 2) 1)
              | name `elem` timedRowNames
              ]
  where
    name = caseNameText one
    guardEq offset value = WordIs "timer-record" (recordBase + offset) value
    control label guard mutate = controlWith label (withoutExpect guard) mutate
    controlWith label transform mutate =
      let mutantStock = mutate stock
          mutantOwned = mutate owned
          baseline = null (compareCase facts domain one stock owned)
          refused = not (null (compareCase facts domain one mutantStock mutantOwned))
          guardNeeded = null (compareCase facts domain (transform one) mutantStock mutantOwned)
       in [ControlRun domain label name baseline refused guardNeeded (baseline && refused && guardNeeded)]
    controlOwned label transform mutate =
      let mutantOwned = mutate owned
          baseline = null (compareCase facts domain one stock owned)
          refused = not (null (compareCase facts domain one stock mutantOwned))
          guardNeeded = null (compareCase facts domain (transform one) stock mutantOwned)
       in [ControlRun domain label name baseline refused guardNeeded (baseline && refused && guardNeeded)]
    withoutExpect guard current =
      current
        { caseExpect = filter (/= guard) (caseExpect current)
        , caseStockExpect = filter (/= guard) (caseStockExpect current)
        , caseOwnedExpect = filter (/= guard) (caseOwnedExpect current)
        }
    mutateRecord offset f observation =
      observation
        { nativeMemory =
            [ (ident, if regionName ident == "timer-record" then replaceWord (fromIntegral offset) (f (readWordAt bytes (fromIntegral offset))) bytes else bytes)
            | (ident, bytes) <- nativeMemory observation
            ]
        }
    mutateKernel address value observation =
      observation
        { nativeMemory =
            [ (ident, if regionName ident == "kernel" then replaceWord (fromIntegral (address - low)) value bytes else bytes)
            | (ident, bytes) <- nativeMemory observation
            ]
        }
    low = fst (kernelBounds facts domain)

runDomain :: FilePath -> FilePath -> FilePath -> Facts -> Domain -> Image -> Image -> Image -> [(Word32, Word32)] -> IO ([CaseRun], [ControlRun])
runDomain runner cacheRoot directory facts domain stockImage ownedImage timerImage ownedSymbols = do
  let selected = cases facts domain
      stockSpecs = [(one, caseFor facts domain False timerImage [] stockImage stockImage one) | one <- selected]
      ownedSpecs = [(one, caseFor facts domain True timerImage ownedSymbols ownedImage stockImage one) | one <- selected]
  stockBatch <- writeBatch (directory </> "stock") (map snd stockSpecs)
  ownedBatch <- writeBatch (directory </> "owned") (map snd ownedSpecs)
  (stockReuse, stockRaw) <- cachedObservedBatch cacheRoot runner stockBatch
  (ownedReuse, ownedRaw) <- cachedObservedBatch cacheRoot runner ownedBatch
  let reference = M.fromList [(caseIdText ident, observation) | (ident, observation) <- stockRaw]
      subject = M.fromList [(caseIdText ident, observation) | (ident, observation) <- ownedRaw]
  results <- forM selected $ \one -> do
    let ident = caseIdText (caseName one) ++ "-" ++ domainLabel domain
        blocking = case caseActor one of
          Just _ -> True
          Nothing -> False
    stockVerdicts <- reportVerdicts (directory </> "stock") ident
    ownedVerdicts <- reportVerdicts (directory </> "owned") ident
    let verdicts = stockVerdicts ++ ownedVerdicts
        controls = case (M.lookup ident reference, M.lookup ident subject) of
          (Just original, Just replacement) -> controlRuns facts domain one original replacement
          _ -> []
    pure ( case (M.lookup ident reference, M.lookup ident subject) of
             (Just original, Just replacement) -> CaseRun domain (caseIdText (caseName one)) blocking [] (verdicts ++ compareCase facts domain one original replacement) (BatchExecuted, BatchExecuted)
             (Nothing, _) -> CaseRun domain (caseIdText (caseName one)) blocking [] ["stock case did not run"] (BatchExecuted, BatchExecuted)
             (_, Nothing) -> CaseRun domain (caseIdText (caseName one)) blocking [] ["owned case did not run"] (BatchExecuted, BatchExecuted)
         , controls
         )
  unless (length stockRaw == length selected && length ownedRaw == length selected) (fail "native observations do not cover the current data-queue case set")
  pure ( [ CaseRun (runEntryDomain run) (runName run) (runBlocking run) (runChecks run) (runDifferences run) (stockReuse, ownedReuse) | run <- map fst results ]
       , concatMap snd results
       )

memoryMismatch :: String -> Bool
memoryMismatch mismatch = "memory " `isPrefixOf` mismatch && "differs from exact allowed final bytes" `isSuffixOf` mismatch
