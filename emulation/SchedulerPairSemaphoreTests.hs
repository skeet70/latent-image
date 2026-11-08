{-# LANGUAGE OverloadedStrings #-}
module Main (main) where

import qualified Data.Aeson as J
import qualified Data.ByteString as B
import qualified Data.Map.Strict as M
import Data.Word (Word32, Word64)
import Emulation.SchedulerPairCheck
import Emulation.SchedulerPairDecode (decodeCheckedObservation)
import Emulation.SchedulerPairEvidence (qualifyEvidence)
import Emulation.SchedulerPairFamily (Pending (..))
import Emulation.SchedulerPairModel (Arguments (..), Memory)
import Emulation.SchedulerPairPlan
import qualified Emulation.OwnFirstPairCheck as Q
import qualified Emulation.SchedulerPairS2 as D
import Emulation.Sha256 (sha256, sha256Hex)
import Replacement.RtosOwnFirst.Regions (Domain (..))
import Replacement.RtosScheduler.Regions (Helper (..))

planBytes :: B.ByteString
planBytes = "semaphore-plan"

semPlan :: Plan
semPlan = Plan Owned "dat" entries guards [] []
 where
  entries =
    [ Entry DomainA (HelperSemantic Acquire) (Address 0x439d8)
    , Entry DomainA (HelperSemantic Release) (Address 0x43a14)
    , Entry DomainA (SemaphoreSemantic SemaphoreSignal) (Address 0x47604)
    , Entry DomainA (SemaphoreSemantic SemaphoreInterruptSignal) (Address 0x47844)
    , Entry DomainA (SemaphoreSemantic SemaphoreWait) (Address 0x47a54)
    , Entry DomainA (SemaphoreSemantic SemaphorePoll) (Address 0x47c94)
    , Entry DomainA (SemaphoreSemantic SemaphoreTimedWait) (Address 0x47dd0)
    , Entry DomainB (SemaphoreSemantic SemaphorePoll) (Address 0xd7a14)
    ]
  guards =
    [ Guard DomainA (Address 0x439d8) (Address 0x43a14) "gac"
    , Guard DomainA (Address 0x43a14) (Address 0x43a50) "gr"
    , Guard DomainA (Address 0x47604) (Address 0x47844) "g-1"
    , Guard DomainA (Address 0x47844) (Address 0x47a54) "g0"
    , Guard DomainA (Address 0x47a54) (Address 0x47c94) "ga"
    , Guard DomainA (Address 0x47c94) (Address 0x47dd0) "gb"
    , Guard DomainA (Address 0x47dd0) (Address 0x48044) "gc"
    , Guard DomainB (Address 0xd7a14) (Address 0xd7c54) "gd"
    ]

semReadback :: Q.Readback
semReadback = Q.Readback 100 [range d lo hi digest | Guard d (Address lo) (Address hi) digest <- planGuards semPlan]
 where
  range d lo hi digest = Q.RangeReadback d lo hi digest
    [Q.ViewHash core a (Right digest) | core <- [0,1], a <- [lo,lo+0x40000000]]

registers :: [Word32] -> D.Registers
registers values = D.Registers (take 16 (values ++ repeat 0)) 0x13 0 0 0

traceOf :: [Word32] -> [Boundary] -> Completion -> CallTrace
traceOf values history completion = CallTrace (registers values) (registers values) completion history Nothing

callOf :: Domain -> Word32 -> Semantic -> [Word32] -> Memory -> Memory -> Word32 -> [Written] -> CallTrace -> Call
callOf = callOfTask 0x9000

callOfTask :: Word32 -> Domain -> Word32 -> Semantic -> [Word32] -> Memory -> Memory -> Word32 -> [Written] -> CallTrace -> Call
callOfTask task domain address semantic arguments before after r0 writes trace =
  Call domain (Address address) semantic (if domain==DomainA then 0 else 1) 0x13 (Arguments arguments)
    before after r0 0x13 writes [] 1 10 (Just trace) task 1 Nothing (map (const (originAtFor task address)) writes) []

originAtFor :: Word32 -> Word32 -> WriteOrigin
originAtFor task address = originAtForWith task 1 address

originAtForWith :: Word32 -> Word32 -> Word32 -> WriteOrigin
originAtForWith task instructions address = WriteOrigin 0 (Address address) (Address 0x9000) 0x13 (Address task) 1 (fromIntegral instructions)

signalAddress, pollAddress, timedAddress, callbackA, timerA :: Word32
signalAddress = 0x47604
pollAddress = 0x47c94
timedAddress = 0x47dd0
callbackA = 0x44318
timerA = 0x4005549c

handlerA, dispatchA, configA, controlA, waitA :: Word32
handlerA = 0x400545a4
dispatchA = 0x40054764
configA = 0x4005dfd8
controlA = 0x4005af0c
waitA = 0x40054efc

taskA, taskB :: Word32
taskA = 0x4005ff0c
taskB = 0x4005ff44

baseBefore :: Word32 -> Word32 -> Word32 -> Memory
baseBefore count maxCount queueHead = M.fromList
  [ (Address handlerA, 0)
  , (Address dispatchA, 1)
  , (Address configA, 0)
  , (Address (configA+4), 1)
  , (Address (configA+8), maxCount)
  , (Address (configA+0x0c), 1)
  , (Address controlA, queueHead)
  , (Address (controlA+4), queueHead)
  , (Address (controlA+8), configA)
  , (Address (controlA+0x0c), count)
  ]

pollCall :: Word32 -> Word32 -> Word32 -> Call
pollCall count r0 afterCount = callOf DomainA pollAddress (SemaphoreSemantic SemaphorePoll) [1,0,0,0]
  (baseBefore count 5 controlA) (baseBefore afterCount 5 controlA) r0 writes trace
 where
  writes = [Written (Address (controlA+0x0c)) 4 afterCount | afterCount /= count]
  trace = traceOf [0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0] [] Returned

signalOverflow :: Word32 -> Call
signalOverflow r0 = callOf DomainA signalAddress (SemaphoreSemantic SemaphoreSignal) [1,0,0,0]
  (baseBefore 5 5 controlA) (baseBefore 5 5 controlA) r0 [] (traceOf (replicate 16 0) [] Returned)

waiterBefore :: Word32 -> Memory
waiterBefore state = M.union (baseBefore 0 5 taskA) $ M.fromList
  [ (Address (taskA+0x0c), state)
  , (Address (taskA+0x24), 0)
  , (Address (taskA+0x14), waitA)
  , (Address taskA, controlA)
  , (Address (taskA+4), controlA)
  , (Address waitA, 0)
  , (Address (waitA+4), controlA)
  , (Address (waitA+8), 1)
  , (Address (timerA+4), callbackA)
  , (Address (timerA+8), taskA)
  ]

signalWake :: Memory -> Memory -> [Written] -> [Boundary] -> Call
signalWake before after writes history =
  (callOf DomainA signalAddress (SemaphoreSemantic SemaphoreSignal) [1,0,0,0] before after 0 writes
    (traceOf (replicate 16 0) history Returned)) { callFirst = 20 }

signalWakeCall :: Call
signalWakeCall = signalWake before after writes [timerRemove]
 where
  before = waiterBefore 1
  after = M.union (M.fromList
    [ (Address controlA, controlA)
    , (Address (controlA+4), controlA)
    ]) before
  writes = [Written (Address controlA) 4 controlA, Written (Address (controlA+4)) 4 controlA]
  timerRemove = Boundary TimerRemoveBoundary (Address 0x44ca8) 1 0 (registers (replicate 16 0)) timerMemory [] [] (Just timerA) 0x9000 [] []

interruptWake :: Call
interruptWake = (signalWake before after writes [timerRemove]) { callSemantic = SemaphoreSemantic SemaphoreInterruptSignal, callAddress = Address 0x47844 }
 where
  before = M.insert (Address handlerA) 1 (waiterBefore 1)
  after = M.union (M.fromList [(Address controlA, controlA), (Address (controlA+4), controlA)]) before
  writes = [Written (Address controlA) 4 controlA, Written (Address (controlA+4)) 4 controlA]
  timerRemove = Boundary TimerRemoveBoundary (Address 0x44ca8) 1 0 (registers (replicate 16 0)) timerMemory [] [] (Just timerA) 0x9000 [] []

signalWakeWithoutSibling :: Call
signalWakeWithoutSibling = signalWake (waiterBefore 1) after [Written (Address controlA) 4 controlA, Written (Address (controlA+4)) 4 controlA] [timerRemove]
 where
  after = M.union (M.fromList [(Address controlA, controlA), (Address (controlA+4), controlA)]) (waiterBefore 1)
  timerRemove = Boundary TimerRemoveBoundary (Address 0x44ca8) 1 0 (registers (replicate 16 0)) timerMemory [] [] (Just timerA) 0x9000 [] []

suspendedWake :: Word32 -> Call
suspendedWake afterState = signalWake before after [Written (Address controlA) 4 controlA, Written (Address (controlA+4)) 4 controlA] []
 where
  before = waiterBefore 4
  after = M.union (M.fromList [(Address controlA, controlA), (Address (controlA+4), controlA), (Address (taskA+0x0c), afterState)]) before

timedWaitCall :: Word32 -> [Boundary] -> Call
timedWaitCall timeout history = callOfTask taskA DomainA timedAddress (SemaphoreSemantic SemaphoreTimedWait) [1, timeout, 0, 0]
  (waiterBefore 1) (waiterBefore 1) 0 [] (traceOf (withLr 0x47dd0) history Returned)

withLr :: Word32 -> [Word32]
withLr lr = [0,0,0,0,0,0,0,0,0,0,0,0,0,0,lr,0]

saveBoundary :: Word32 -> Boundary
saveBoundary lr = Boundary SaveBoundary (Address 0x43640) 1 0 (registers (withLr lr)) M.empty [] [] Nothing taskA [] []

resumeBoundary :: Boundary
resumeBoundary = Boundary ResumeBoundary (Address 0x43668) 2 0 (registers (withLr 0x47dd0)) M.empty [] [] Nothing taskA [] []

timerInsertBoundary :: Boundary
timerInsertBoundary = Boundary TimerInsertBoundary (Address 0x44c60) 1 0 (registers (replicate 16 0)) timerMemory [] [] (Just timerA) taskA [] []

timerMemory :: Memory
timerMemory = M.fromList [(Address (timerA+4), callbackA), (Address (timerA+8), taskA)]

timerRemoveBoundary :: Boundary
timerRemoveBoundary = Boundary TimerRemoveBoundary (Address 0x44ca8) 1 0 (registers (replicate 16 0)) timerMemory [] [] (Just timerA) taskA [] []

expiryBoundary :: Boundary
expiryBoundary = Boundary TimerConsumeBoundary (Address callbackA) 3 0 (registers (replicate 16 0)) timerMemory [] [] (Just timerA) taskA [] []

zeroTimeoutCall :: Word32 -> [Boundary] -> Call
zeroTimeoutCall r0 history = callOfTask taskA DomainA timedAddress (SemaphoreSemantic SemaphoreTimedWait) [1, 0, 0, 0]
  (baseBefore 0 5 controlA) (baseBefore 0 5 controlA) r0 [] (traceOf (withLr 0x47dd0) history Returned)

timedAcquireWithTimer :: Call
timedAcquireWithTimer = timedWaitCall 100 [saveBoundary 0x47dd0, timerInsertBoundary]

restoredClobber :: Call
restoredClobber = (timedWaitCall 100 [saveBoundary 0x47dd0, timerInsertBoundary, timerRemoveBoundary])
  { callTrace = Just (CallTrace before after Returned [saveBoundary 0x47dd0, timerInsertBoundary, timerRemoveBoundary] Nothing) }
 where
  before = registers (replicate 16 0)
  after = registers (replicate 4 0 ++ [1] ++ replicate 11 0)

crossTimedCall :: Call
crossTimedCall = callOfTask taskA DomainA timedAddress (SemaphoreSemantic SemaphoreTimedWait) [0x00020001, 100, 0, 0]
  crossBefore crossBefore 0 [] (traceOf (withLr 0x47dd0) [saveBoundary 0x47dd0, timerInsertBoundary, timerRemoveBoundary] Returned)
 where
  configB = 0x400edf98
  controlB = 0x400eaecc
  crossBefore = M.fromList
    [ (Address handlerA, 0)
    , (Address dispatchA, 1)
    , (Address configB, 0)
    , (Address (configB+4), 1)
    , (Address (configB+8), 5)
    , (Address (configB+0x0c), 1)
    , (Address controlB, taskA)
    , (Address (controlB+4), taskA)
    , (Address (controlB+8), configB)
    , (Address (controlB+0x0c), 0)
    , (Address (taskA+0x0c), 1)
    , (Address (taskA+0x24), 0)
    , (Address (taskA+0x14), waitA)
    , (Address taskA, controlB)
    , (Address (taskA+4), controlB)
    , (Address waitA, 0)
    , (Address (waitA+4), controlB)
    , (Address (waitA+8), 2)
    , (Address (timerA+4), callbackA)
    , (Address (timerA+8), taskA)
    ]

timedExpiryCall :: Call
timedExpiryCall = callOfTask taskA DomainA timedAddress (SemaphoreSemantic SemaphoreTimedWait) [1, 100, 0, 0]
  (waiterBefore 1) (M.insert (Address waitA) (fromIntegral (-50 :: Int)) (waiterBefore 1)) (fromIntegral (-50 :: Int))
  [Written (Address waitA) 4 (fromIntegral (-50 :: Int))] (traceOf (withLr 0x47dd0) [saveBoundary 0x47dd0, timerInsertBoundary, expiryBoundary] Returned)

pendingForever :: Word32 -> Word32 -> Pending
pendingForever timeout stackWord = Pending (SemaphoreSemantic SemaphoreTimedWait) DomainA [1, timeout, 0, 0] before (Just terminal) taskA 0x7000 0x13 timedAddress 5 (Just 0x93) (Just 50) (Just 50) 5 0
 where
  before = M.fromList [(Address 0x7000, stackWord)]
  terminal = M.union (baseBefore 0 5 taskA) $ M.fromList
    [ (Address (taskA+0x0c), 0x32)
    , (Address (taskA+0x14), waitA)
    , (Address taskA, controlA)
    , (Address (taskA+4), controlA)
    , (Address waitA, 0)
    , (Address (waitA+4), controlA)
    , (Address (waitA+8), 1)
    , (Address (taskA+0x18), 0x8000)
    , (Address (taskA+0x1c), 0x43668)
    , (Address (taskA+8), 0x40050000)
    , (Address (0x40050000+0x10), 0x2000)
    , (Address (0x40050000+0x14), 0x7000)
    , (Address (0x8000+32), timedAddress)
    , (Address 0x400545c0, 0)
    , (Address 0x400545c4, 0)
    ]

releaseAddress, acquireAddress :: Word32
releaseAddress = 0x43a14
acquireAddress = 0x439d8

helperCall :: Word32 -> Word32 -> Semantic -> Word32 -> Word32 -> Word64 -> Word32 -> Word32 -> Call
helperCall task address semantic lock instructions callFirst value r0 = call
 where
  pointer = 0x9000
  before = M.fromList [(Address handlerA, 0), (Address dispatchA, 1), (Address pointer, lock), (Address lock, 1 - value)]
  after = M.insert (Address lock) value before
  call = (callOfTask task DomainA address semantic [pointer,0,0,0] before after r0 [Written (Address lock) 4 value]
    (traceOf (replicate 16 0) [] Returned)) { callFirst = callFirst, callWriteOrigins = [originAtForWith task instructions lock] }

releaseCall :: Word32 -> Word64 -> Call
releaseCall lock first = helperCall taskA releaseAddress (HelperSemantic Release) lock (fromIntegral first) first 0 0x9000

releaseEvents :: [J.Value]
releaseEvents = [lockEvent taskA 0 10 10 0x400545c0 0, lockEvent taskA 0 11 11 0x400545c4 0]

interleavedEvents :: [J.Value]
interleavedEvents =
  [ lockEvent taskA 0 100 100 0x400545c0 0
  , lockEvent 0x9000 0 101 101 0x400545c0 1
  , lockEvent taskA 0 102 102 0x400545c4 0
  ]

pendingInterleaved :: Pending
pendingInterleaved = (pendingForever (fromIntegral (-1 :: Int)) 0) { pendingSaveAt = Just 110, pendingSaveOrder = Just 110, pendingTerminal = Just terminal }
 where
  terminal = M.insert (Address 0x400545c0) 1 (maybe M.empty id (pendingTerminal (pendingForever (fromIntegral (-1 :: Int)) 0)))

pendingStaleRelease :: Pending
pendingStaleRelease = (pendingForever (fromIntegral (-1 :: Int)) 0) { pendingEntered = 50, pendingEntryOrder = 50 }

waitTimeoutUntimed :: Call
waitTimeoutUntimed = callOfTask taskA DomainA 0x47a54 (SemaphoreSemantic SemaphoreWait) [1,0,0,0]
  (waiterBefore 1) (M.insert (Address waitA) (fromIntegral (-50 :: Int)) (waiterBefore 1)) (fromIntegral (-50 :: Int))
  [Written (Address waitA) 4 (fromIntegral (-50 :: Int))] (traceOf (withLr 0x47dd0) [saveBoundary 0x47dd0] Returned)

pollRefusalWrite :: Call
pollRefusalWrite = callOf DomainA pollAddress (SemaphoreSemantic SemaphorePoll) [1,0,0,0] before after (fromIntegral (-42 :: Int)) [Written (Address (configA+0x0c)) 4 1] trace
 where
  before = M.insert (Address (configA+0x0c)) 0 (baseBefore 1 5 controlA)
  after = M.insert (Address (configA+0x0c)) 1 before
  trace = traceOf (replicate 16 0) [] Returned

waitAcquireEmpty :: Call
waitAcquireEmpty = callOf DomainA 0x47a54 (SemaphoreSemantic SemaphoreWait) [1,0,0,0] before after 0 [Written (Address (controlA+0x0c)) 4 0xffffffff] trace
 where
  before = baseBefore 0 5 controlA
  after = M.insert (Address (controlA+0x0c)) 0xffffffff before
  trace = traceOf (replicate 16 0) [] Returned

pendingBadTail :: Pending
pendingBadTail = (pendingForever (fromIntegral (-1 :: Int)) 0) { pendingTerminal = Just terminal }
 where
  terminal = M.insert (Address (controlA+4)) taskB (maybe M.empty id (pendingTerminal (pendingForever (fromIntegral (-1 :: Int)) 0)))

pendingStranded :: Pending
pendingStranded = (pendingForever (fromIntegral (-1 :: Int)) 0) { pendingTerminal = Just terminal }
 where
  terminal = M.union (M.fromList
    [ (Address controlA, taskB)
    , (Address (controlA+4), taskB)
    , (Address taskB, controlA)
    , (Address (taskB+4), controlA)
    , (Address taskA, taskA)
    , (Address (taskA+4), taskA)
    ]) (maybe M.empty id (pendingTerminal (pendingForever (fromIntegral (-1 :: Int)) 0)))

lockEvent :: Word32 -> Int -> Word64 -> Word64 -> Word32 -> Word32 -> J.Value
lockEvent task core call at address value = J.object
  ["task" J..= task, "core" J..= coreName core, "call" J..= call, "at" J..= at, "address" J..= hex address, "value" J..= value]

observation :: [J.Value] -> [Call] -> [Pending] -> J.Value
observation events calls pendings = J.object
  [ "schema" J..= (3::Int), "role" J..= roleName Owned
  , "inputs" J..= [J.object ["sha256" J..= digest]
      | digest <- [sha256Hex (sha256 planBytes), Q.runDocumentSha256, "dat", Q.bootRomSha256, trajectorySha256, "observer"]]
  , "instructions" J..= (100::Int), "entries" J..= (length calls + length pendings), "returns" J..= length calls
  , "untracked" J..= J.object ["count" J..= (0::Int)], "unarmed_entries" J..= J.object ["count" J..= (0::Int)]
  , "fill_fetch" J..= J.Null
  , "armings" J..= [arming DomainA, arming DomainB]
  , "segments" J..= [segment [DomainA] 10 20 [(0x439d8,0x43a14),(0x43a14,0x43a50),(0x47604,0x47844),(0x47844,0x47a54),(0x47a54,0x47c94),(0x47c94,0x47dd0),(0x47dd0,0x48044)]
                    , segment [DomainA,DomainB] 20 100 [(0x439d8,0x43a14),(0x43a14,0x43a50),(0x47604,0x47844),(0x47844,0x47a54),(0x47a54,0x47c94),(0x47c94,0x47dd0),(0x47dd0,0x48044),(0xd7a14,0xd7c54)]]
  , "final_readback" J..= rb semReadback
  , "classes" J..= map wireCall calls
  , "lock_events" J..= events
  , "pending" J..= map wirePending pendings
  , "milestones" J..= [milestone phase | phase <- phases]
  , "error" J..= J.Null, "machine_failure" J..= J.Null, "bank_counts" J..= [0::Int,1::Int]
  , "pc_watches" J..= [J.object ["core" J..= ("primary"::String),"address" J..= hex 0xd0b00,"count" J..= (1::Int)]]
  , "demand_paging" J..= ([]::[J.Value])
  ]
 where
  arming d = J.object ["domain" J..= domainName d
    , "readback" J..= rb (Q.Readback (if d==DomainA then 10 else 20) (filter ((==d) . Q.readbackDomain) (Q.readbackRanges semReadback)))]
  segment :: [Domain] -> Int -> Int -> [(Word32, Word32)] -> J.Value
  segment ds started ended ranges = J.object
    [ "domains" J..= map domainName ds, "started" J..= started, "ended" J..= ended
    , "state" J..= J.object
        [ "records" J..= ([]::[J.Value])
        , "classes" J..= [J.object ["range" J..= [lo,hi],"kinds" J..= (["cpu-write","host-write","model-bus-write"]::[String])] | (lo,hi) <- ranges]
        ]
    ]
  milestone phase = J.object
    [ "phase" J..= phase, "pulse" J..= J.Null, "expected" J..= menu, "observed" J..= menu
    , "reached" J..= (100::Int), "finished" J..= (100::Int)
    ]
  menu = J.object ["menu" J..= (1::Int),"setting" J..= (5::Int),"cursor" J..= (2::Int)]

phases :: [String]
phases = ["cold-start","shooting-menu","modal75-film","modal75-returned","modal75-film-repeated","modal75-returned-repeated","modal75-handback"]

streamingVerdict :: [J.Value] -> [Call] -> [Pending] -> Either String J.Value
streamingVerdict events calls pendings = case decodeCheckedObservation semPlan (J.encode (observation events calls pendings)) of
  Left failure -> Left failure
  Right (decoded, evidence) -> case qualifyEvidence planBytes semPlan decoded evidence of
    Left refusal -> Left (show refusal)
    Right value -> Right value

accepts :: String -> [Call] -> [Pending] -> IO ()
accepts label = acceptsWith label []

acceptsWith :: String -> [J.Value] -> [Call] -> [Pending] -> IO ()
acceptsWith label events calls pendings = case streamingVerdict events calls pendings of
  Right _ -> pure ()
  Left failure -> error (label ++ ": " ++ failure)

refuses :: String -> [Call] -> [Pending] -> IO ()
refuses label = refusesWith label []

refusesWith :: String -> [J.Value] -> [Call] -> [Pending] -> IO ()
refusesWith label events calls pendings = case streamingVerdict events calls pendings of
  Left _ -> pure ()
  Right _ -> error (label ++ ": expected refusal")

main :: IO ()
main = do
  accepts "poll acquires from one" [pollCall 1 0 0] []
  refuses "poll count one timeout refuses" [pollCall 1 (fromIntegral (-50 :: Int)) 1] []
  refuses "poll empty acquire refuses" [pollCall 0 0 0] []
  accepts "signal overflow returns -43" [signalOverflow (fromIntegral (-43 :: Int))] []
  refuses "signal overflow -28 refuses" [signalOverflow (fromIntegral (-28 :: Int))] []
  accepts "zero timeout returns -50 without save" [zeroTimeoutCall (fromIntegral (-50 :: Int)) []] []
  refuses "zero timeout with save refuses" [zeroTimeoutCall (fromIntegral (-50 :: Int)) [saveBoundary 0x47dd0]] []
  accepts "restored continuation ignores resume entry PC" [timedWaitCall 100 [saveBoundary 0x47dd0, timerInsertBoundary, timerRemoveBoundary, resumeBoundary]] []
  refuses "save without semaphore continuation refuses" [timedWaitCall 100 [saveBoundary 0x40000, timerInsertBoundary, timerRemoveBoundary]] []
  accepts "suspended waiter stays suspended" [suspendedWake 4] []
  refuses "suspended waiter ready refuses" [suspendedWake 1] []
  accepts "signal removes sibling timer" [timedAcquireWithTimer, signalWakeCall] []
  accepts "signal cancellation survives observer class order" [signalWakeCall, timedAcquireWithTimer] []
  refuses "signal cannot invent timer removal" [signalWakeWithoutSibling] []
  accepts "timed expiry uses callback evidence" [timedExpiryCall] []
  accepts "cross-domain wait uses entry task records" [crossTimedCall] []
  accepts "interrupt signal cancels armed timer" [timedAcquireWithTimer, interruptWake] []
  acceptsWith "negative r1 timeout is forever" releaseEvents [releaseCall 0x400545c0 10, releaseCall 0x400545c4 11, pollCall 1 0 0] [pendingForever (fromIntegral (-1 :: Int)) 0]
  refusesWith "positive r1 timeout is not forever" releaseEvents [releaseCall 0x400545c0 10, releaseCall 0x400545c4 11, pollCall 1 0 0] [pendingForever 5 (fromIntegral (-1 :: Int))]
  refusesWith "stranded caller is not a member" releaseEvents [releaseCall 0x400545c0 10, releaseCall 0x400545c4 11, pollCall 1 0 0] [pendingStranded]
  refusesWith "corrupted queue tail" releaseEvents [releaseCall 0x400545c0 10, releaseCall 0x400545c4 11, pollCall 1 0 0] [pendingBadTail]
  refuses "refused poll cannot write" [pollRefusalWrite] []
  refuses "empty wait cannot acquire" [waitAcquireEmpty] []
  refuses "completed call cannot clobber r4" [restoredClobber] []
  refusesWith "stale release does not satisfy pending" releaseEvents [releaseCall 0x400545c0 10, releaseCall 0x400545c4 11, pollCall 1 0 0] [pendingStaleRelease]
  refuses "untimed wait cannot time out" [waitTimeoutUntimed] []
  acceptsWith "interleaved cross-core lock acquisition" interleavedEvents
    [helperCall taskA releaseAddress (HelperSemantic Release) 0x400545c0 100 100 0 0x9000
    ,helperCall 0x9000 acquireAddress (HelperSemantic Acquire) 0x400545c0 101 101 1 0
    ,helperCall taskA releaseAddress (HelperSemantic Release) 0x400545c4 102 102 0 0x9000
    ,pollCall 1 0 0] [pendingInterleaved]
  refusesWith "pending save without I masked" releaseEvents [releaseCall 0x400545c0 10, releaseCall 0x400545c4 11, pollCall 1 0 0] [(pendingForever (fromIntegral (-1 :: Int)) 0) { pendingSaveCpsr = Just 0x13 }]
  putStrLn "scheduler-pair semaphore tests passed"

wireCall :: Call -> J.Value
wireCall value = J.object
  [ "entry" J..= J.object
      [ "slot" J..= J.object ["domain" J..= domainName (callDomain value),"entry" J..= hex (addressOf (callAddress value)),"semantic" J..= semanticValue (callSemantic value)]
      , "core" J..= coreName (callCore value), "cpsr" J..= callCpsr value, "arguments" J..= args (callArguments value)
      , "memory" J..= wordsOf (callBefore value)
      , "registers" J..= maybe (replicate (16::Int) (0::Int)) (map fromIntegral . D.registers . traceBefore) (callTrace value)
      , "cpacr" J..= (0::Int), "fpexc" J..= (0::Int), "tpidrurw" J..= (0::Int)
      , "task" J..= J.object ["view" J..= ("committed"::String),"value" J..= callTask value]
      , "kernel_context" J..= callKernelContext value, "task_stack" J..= J.Null
      ]
  , "result" J..= J.object
      [ "r0" J..= callR0 value, "cpsr" J..= callReturnCpsr value, "memory" J..= wordsOf (callAfter value)
      , "registers" J..= maybe (replicate (16::Int) (0::Int)) (map fromIntegral . D.registers . traceAfter) (callTrace value)
      , "cpacr" J..= (0::Int), "fpexc" J..= (0::Int), "tpidrurw" J..= (0::Int)
      , "completion" J..= ("return"::String)
      , "history" J..= map wireBoundary (maybe [] traceHistory (callTrace value))
      , "writes" J..= map writeOf (zip (callWrites value) (callWriteOrigins value))
      , "bank" J..= map writeOf (zip (callBank value) (callBankOrigins value))
      ]
  , "count" J..= callCount value, "first" J..= callFirst value
  ]

wireBoundary :: Boundary -> J.Value
wireBoundary boundary = J.object
  [ "kind" J..= kindName (boundaryKind boundary), "address" J..= hex (addressOf (boundaryAddress boundary))
  , "instructions" J..= boundaryInstructions boundary, "core" J..= coreName (boundaryCore boundary)
  , "registers" J..= D.registers (boundaryRegisters boundary), "cpsr" J..= D.processorState (boundaryRegisters boundary)
  , "cpacr" J..= (0::Int), "fpexc" J..= (0::Int), "tpidrurw" J..= (0::Int)
  , "memory" J..= wordsOf (boundaryMemory boundary), "writes" J..= ([]::[J.Value]), "bank" J..= ([]::[J.Value])
  , "value" J..= fmap J.toJSON (boundaryValue boundary)
  , "task" J..= J.object ["view" J..= ("committed"::String),"value" J..= boundaryTask boundary]
  ]

wirePending :: Pending -> J.Value
wirePending pending = J.object
  [ "entry" J..= J.object
      [ "slot" J..= J.object ["domain" J..= domainName (pendingDomain pending),"entry" J..= hex (pendingEntry pending),"semantic" J..= semanticValue (pendingSemantic pending)]
      , "core" J..= coreName 0, "order" J..= pendingEntryOrder pending
      , "arguments" J..= pendingArguments pending, "memory" J..= wordsOf (pendingBefore pending)
      , "task" J..= J.object ["view" J..= ("committed"::String),"value" J..= pendingTask pending]
      , "sp" J..= hex (pendingSp pending), "cpsr" J..= pendingCpsr pending
      ]
  , "entered" J..= pendingEntered pending, "terminal_memory" J..= maybe [] wordsOf (pendingTerminal pending)
  , "history" J..= [J.object ["kind" J..= ("save"::String),"cpsr" J..= cpsr,"instructions" J..= at,"order" J..= order]
      | cpsr <- maybe [] pure (pendingSaveCpsr pending), at <- maybe [] pure (pendingSaveAt pending), order <- maybe [] pure (pendingSaveOrder pending)]
  ]

kindName :: BoundaryKind -> String
kindName kind = case kind of
  SaveBoundary -> "save"
  ResumeBoundary -> "resume"
  NoSaveBoundary -> "no-save"
  RetryBoundary -> "retry"
  CallbackBoundary -> "callback"
  CallbackReturnBoundary -> "callback-return"
  SelectedBoundary -> "selected-read"
  DiscardedBoundary -> "discarded-read"
  EnabledBoundary -> "enabled-read"
  GlobalLockBoundary -> "global-lock-read"
  SelectCompleteBoundary -> "select-complete"
  FirstEntryBoundary -> "first-entry-exit"
  AcquiredBoundary -> "acquired"
  IrqTailBoundary -> "irq-tail"
  ReturnBoundary -> "return"
  PrefetchAbortBoundary -> "prefetch-abort"
  TimerInsertBoundary -> "timer-insert"
  TimerRemoveBoundary -> "timer-remove"
  TimerConsumeBoundary -> "timer-consume"

rb :: Q.Readback -> J.Value
rb value = J.object ["instructions" J..= Q.readbackInstructions value, "ranges" J..=
  [J.object ["domain" J..= domainName (Q.readbackDomain range),"start" J..= hex (Q.readbackStart range),"end" J..= hex (Q.readbackEnd range)
    ,"physical_sha256" J..= Q.readbackPhysical range
    ,"views" J..= [J.object ["core" J..= Q.viewCore view,"address" J..= hex (Q.viewAddress view)
      ,"sha256" J..= either (const "") id (Q.viewDigest view)] | view <- Q.readbackViews range]]
  | range <- Q.readbackRanges value]]

wordsOf :: Memory -> [J.Value]
wordsOf memory = [J.object ["address" J..= hex address,"value" J..= J.object ["view" J..= ("committed"::String),"value" J..= value]]
  | (Address address,value) <- M.toList memory]

writeOf :: (Written, WriteOrigin) -> J.Value
writeOf (Written (Address address) size value, origin) = J.object
  [ "address" J..= hex address, "size" J..= size, "value" J..= value
  , "origin" J..= J.object
      [ "core" J..= coreName (writerCore origin), "pc" J..= hex (addressOf (writerPc origin)), "sp" J..= hex (addressOf (writerSp origin))
      , "task" J..= hex (addressOf (writerTask origin)), "cpsr" J..= writerCpsr origin
      , "kernel_context" J..= writerContext origin, "instructions" J..= writerInstructions origin
      ]
  ]

domainName :: Domain -> String
domainName DomainA = "a"
domainName DomainB = "b"

coreName :: Int -> String
coreName 0 = "primary"
coreName _ = "secondary"

args :: Arguments -> [Word32]
args (Arguments values) = values

addressOf :: Address -> Word32
addressOf (Address address) = address
