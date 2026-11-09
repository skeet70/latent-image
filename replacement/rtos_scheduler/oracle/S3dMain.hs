{-# LANGUAGE OverloadedStrings #-}

-- | Emitted A32 qualification of the six S3d lifecycle services against the pinned loaders.
module Main (main) where

import Control.Category ((>>>))
import Control.Monad (forM, forM_, unless)
import Data.Aeson ((.=))
import Data.Bits ((.&.), (.|.), complement, shiftL, shiftR, xor)
import qualified Data.ByteString as B
import Data.List (find, isPrefixOf, sortOn)
import qualified Data.Map.Strict as M
import Data.Word (Word32)
import Emulation.CachedNativeBatch (cachedObservedBatch)
import Emulation.NativeObservation
import Emulation.OwnedCase
import Emulation.Sha256 (sha256File, sha256Hex)
import FujiAF.JSON (encodeCanonical)
import qualified Replacement.RtosPlatform.Aggregate as Agg
import qualified Replacement.RtosPlatform.Union as Union
import qualified Replacement.RtosOwnFirst.Regions as Regions
import System.Directory (createDirectoryIfMissing, makeAbsolute)
import System.Environment (getArgs)
import System.FilePath ((</>), takeDirectory)
import System.Exit (exitFailure)
import qualified Data.Aeson as A

data Domain = DomainA | DomainB deriving (Eq, Show)

domainLabel :: Domain -> String
domainLabel DomainA = "a"
domainLabel DomainB = "b"

type W = M.Map Word32 Word32

data Role = Stock | Owned deriving (Eq, Show)

data FetchLayout = OriginalFunctions | AllocatedFunctions deriving (Eq, Show)

data TimerIndexState = LinkedIndex | DetachedIndex deriving (Eq, Show)
data TimerContract = RetainedTimer | OwnedDetachedTimer
data TimerIndexCell = TimerIndexCell Domain GuestAddress

armedTimerIndex :: TimerIndexCell
armedTimerIndex = TimerIndexCell DomainA (GuestAddress 0x40059200)

data Aggregate = Aggregate Agg.AggregateImage [Agg.AggregateSymbol] [Agg.SectionPlacement] [Regions.Extent]

data Service = Terminate | ChangePriority | TimedSleep | ReleaseWait | HandlerRelease | Delay
  | Timeout | DelayCallback
  deriving (Eq, Show)

data Expected = ReturnedR0 Word32 | StoppedAt Word32 Word32 deriving (Eq, Show)

data Case = Case
  { caseName :: String
  , caseService :: Service
  , caseArgument :: Word32
  , caseArgument1 :: Word32
  , caseSetup :: Domain -> W -> W
  , caseCpsr :: Word32
  , caseHandler :: Bool
  , caseLifecycle :: Maybe Word32
  , caseExpected :: Expected
  }

entryOf :: Domain -> Service -> Word32
entryOf domain service = base + if domain == DomainB then 0x8ffc0 else 0
  where
    base = case service of
      Terminate -> 0x454f0
      ChangePriority -> 0x4582c
      TimedSleep -> 0x45e54
      ReleaseWait -> 0x46534
      HandlerRelease -> 0x467b4
      Delay -> 0x46e48
      Timeout -> 0x44318
      DelayCallback -> 0x44634

extentOf :: Service -> Word32
extentOf service = case service of
  Terminate -> 0x33c
  ChangePriority -> 0x314
  TimedSleep -> 0x1b0
  ReleaseWait -> 0x280
  HandlerRelease -> 0x24c
  Delay -> 0x184
  Timeout -> 0x1cc
  DelayCallback -> 0xc8

switchOf :: Domain -> Word32
switchOf domain = (if domain == DomainA then 0x43640 else 0xd3640)

stubBase, exitAddress, stackBase, stackTop :: Word32
stubBase = 0x910000
exitAddress = 0x920000
stackBase = 0x40070000
stackTop = stackBase + 0x800

loaderBase :: Domain -> Word32
loaderBase DomainA = 0x40000
loaderBase DomainB = 0xd0000

globalsBase :: Domain -> Word32
globalsBase DomainA = 0x40050000
globalsBase DomainB = 0x400e0000

globalsSize :: Word32
globalsSize = 0x14000

contexts, currentCell, handlerCell, _schedulerLock, _objectLock, readyHeads, bitmapCell,
  runningCell, dispatchCell, selectedCell, tickCell, timerCount, timerHeap, taskBase,
  definitionBase, _sleepResults, _sleepTimers, _rescheduleCell, lockWord, objectWord,
  acquireFn, releaseFn, firstEntry, timeoutCallback,
  delayCallback :: Domain -> Word32
contexts d = (if d == DomainA then 0x400545cc else 0x400e458c)
currentCell d = contexts d - 4
handlerCell d = if d == DomainA then 0x400545a4 else 0x400e4564
_schedulerLock d = if d == DomainA then 0x400545c0 else 0x400e4580
_objectLock d = _schedulerLock d + 4
readyHeads d = if d == DomainA then 0x400546dc else 0x400e469c
bitmapCell d = readyHeads d + 0x80
runningCell d = bitmapCell d + 4
dispatchCell d = runningCell d + 4
selectedCell d = runningCell d + 8
tickCell d = if d == DomainA then 0x40054770 else 0x400e4730
timerCount d = tickCell d + 8
timerHeap d = if d == DomainA then 0x4005d718 else 0x400ed6d8
taskBase d = if d == DomainA then 0x4005ff0c else 0x400efecc
definitionBase d = if d == DomainA then 0x4005beac else 0x400ebe6c
_sleepResults d = if d == DomainA then 0x4005477c else 0x400e473c
_sleepTimers d = if d == DomainA then 0x4005495c else 0x400e491c
_rescheduleCell d = if d == DomainA then 0x400546d8 else 0x400e4698
lockWord d = if d == DomainA then 0x40056000 else 0x400e6000
objectWord d = lockWord d + 4
acquireFn d = if d == DomainA then 0x439d8 else 0xd398c
releaseFn d = if d == DomainA then 0x43a14 else 0xd39c8
firstEntry d = if d == DomainA then 0x43728 else 0xd3728
timeoutCallback d = if d == DomainA then 0x44318 else 0xd42d8
delayCallback d = if d == DomainA then 0x44634 else 0xd45f4

domainId :: Domain -> Word32
domainId DomainA = 1
domainId DomainB = 2

taskAt, definitionAt :: Domain -> Word32 -> Word32
taskAt d slot = taskBase d + (slot - 1) * 0x38
definitionAt d slot = definitionBase d + (slot - 1) * 0x28

taskField, defField :: Word32 -> Word32 -> Word32
taskField task offset = task + offset
defField def offset = def + offset

setW :: Word32 -> Word32 -> W -> W
setW at value = M.insert at value

setB :: Word32 -> Word32 -> W -> W
setB at value m =
  let wordAt = at .&. complement 3
      shift = (at .&. 3) * 8
      old = M.findWithDefault 0 wordAt m
      mask = complement (0xff `shiftL` fromIntegral shift)
      new = (old .&. mask) .|. ((value .&. 0xff) `shiftL` fromIntegral shift)
   in M.insert wordAt new m

wordOf :: Word32 -> W -> Word32
wordOf at = M.findWithDefault 0 at

_byteOf :: Word32 -> W -> Word32
_byteOf at m = (wordOf (at .&. complement 3) m `shiftR` fromIntegral ((at .&. 3) * 8)) .&. 0xff

seedGlobals :: Domain -> W -> W
seedGlobals d =
  foldl (\f (address, value) -> f >>> setW address value) id
    ([ (readyHeads d + priority * 8, readyHeads d + priority * 8) | priority <- [0 .. 15] ]
      ++ [ (readyHeads d + priority * 8 + 4, readyHeads d + priority * 8) | priority <- [0 .. 15] ])
    >>> setW (bitmapCell d) 0
    >>> setW (selectedCell d) 0
    >>> setW (runningCell d) 0
    >>> setW (dispatchCell d) 0
    >>> setW (timerCount d) 0
    >>> setW (tickCell d) 1000

seedTasks :: Domain -> W -> W
seedTasks d =
  foldl (\m slot ->
    let task = taskAt d slot
        def = definitionAt d slot
     in m
          >>> setW (taskField task 0x08) def
          >>> setB (taskField task 0x0c) 0
          >>> setW (taskField task 0x00) 0
          >>> setW (taskField task 0x04) 0
          >>> setW (taskField task 0x14) 0
          >>> setW (taskField task 0x20) (domainId d)
          >>> setW (defField def 0x24) 1
          >>> setW (defField def 0x0c) 15
          >>> setW (defField def 0x10) 0x100
          >>> setW (defField def 0x14) stackBase
          >>> setW (defField def 0x20) slot)
    id [1 .. 120]

seedRecord :: Domain -> Domain -> W -> W
seedRecord executing target =
  let record = contexts executing + if target == DomainA then 0 else 0x84
      recordId = if target == executing then 0 else domainId target
   in setW (record + 0x00) recordId
      >>> setW (record + 0x08) (lockWord target)
      >>> setW (record + 0x0c) (acquireFn target)
      >>> setW (record + 0x10) (releaseFn target)
      >>> setW (record + 0x14) (objectWord target)
      >>> setW (record + 0x18) (acquireFn target)
      >>> setW (record + 0x1c) (releaseFn target)
      >>> setW (record + 0x20) (readyHeads target)
      >>> setW (record + 0x24) (bitmapCell target)
      >>> setW (record + 0x28) (runningCell target)
      >>> setW (record + 0x2c) (selectedCell target)
      >>> setW (record + 0x30) (dispatchCell target)
      >>> setW (record + 0x34) (timerHeap target)
      >>> setW (record + 0x38) (timerCount target)
      >>> setW (record + 0x3c) (tickCell target - 4)
      >>> setW (record + 0x40) (tickCell target)
      >>> setW (record + 0x44) (taskBase target)
      >>> setW (record + 0x48) 120
      >>> setW (record + 0x7c) (firstEntry target)

baseFixture :: Domain -> W
baseFixture executing =
  foldl (\m f -> f m) M.empty
    [ seedGlobals DomainA
    , seedGlobals DomainB
    , seedTasks DomainA
    , seedTasks DomainB
    , seedRecord executing DomainA
    , seedRecord executing DomainB
    , setW (currentCell executing) (contexts executing + if executing == DomainA then 0 else 0x84)
    , setW (handlerCell executing) 0
    , setW (dispatchCell executing) 1
    , setW (lockWord DomainA) 0
    , setW (lockWord DomainB) 0
    , setW (objectWord DomainA) 0
    , setW (objectWord DomainB) 0
    ]

makeRunning :: Domain -> Word32 -> W -> W
makeRunning d slot =
  makeReady d slot 0 True >>> setW (runningCell d) (taskAt d slot)

makeReady :: Domain -> Word32 -> Word32 -> Bool -> W -> W
makeReady d slot priority selected m =
  let task = taskAt d slot
      readyHead = readyHeads d + priority * 8
      previous = wordOf (readyHead + 4) m
      m1 = (setB (taskField task 0x0c) 1
        >>> setB (taskField task 0x0d) priority
        >>> setW (taskField task 0x04) previous
        >>> setW (taskField task 0x00) readyHead
        >>> setW previous task
        >>> setW (readyHead + 4) task
        >>> setW (bitmapCell d) (wordOf (bitmapCell d) m .|. (1 `shiftL` fromIntegral priority))) m
   in if selected then setW (selectedCell d) task m1 else m1

setWait :: Domain -> Word32 -> Word32 -> Word32 -> Word32 -> W -> W
setWait d slot wait queue owner =
  setB (taskField (taskAt d slot) 0x0c) 0x0a
  >>> setW (taskField (taskAt d slot) 0x14) wait
  >>> setW (wait + 0x04) queue
  >>> setW (wait + 0x08) owner
  >>> setW queue queue
  >>> setW (queue + 4) queue

_linkQueue :: Domain -> Word32 -> W -> W
_linkQueue d slot =
  let task = taskAt d slot
      queue = 0x40059000
   in setW (taskField task 0x00) queue
      >>> setW (taskField task 0x04) queue
      >>> setW queue task
      >>> setW (queue + 4) task

armTimer :: Domain -> Word32 -> W -> W
armTimer d slot =
  let task = taskAt d slot
      TimerIndexCell _ (GuestAddress address) = armedTimerIndex
      timer = fromIntegral address
      heap = timerHeap d
   in setW timer 1
      >>> setW (timer + 4) (timeoutCallback d)
      >>> setW (timer + 8) task
      >>> setW (timerCount d) 1
      >>> setW heap 0
      >>> setW (heap + 4) timer
      >>> setW (taskField task 0x14) 0x40059100
      >>> setW (0x40059100) timer
      >>> setW (0x40059100 + 4) 0x40059000
      >>> setW (0x40059100 + 8) (domainId d)

idOf :: Domain -> Word32 -> Word32
idOf d slot = (domainId d `shiftL` 16) .|. slot

setupRunning :: Domain -> W -> W
setupRunning d = makeRunning d 1

cases :: Domain -> [Case]
cases d =
  [ Case "terminate-ready" Terminate (idOf d 2) 0 (\e -> setupRunning e >>> makeReady d 2 3 False) 0x53 False Nothing (ReturnedR0 0)
  , Case "terminate-dormant" Terminate (idOf d 2) 0 setupRunning 0x53 False Nothing (ReturnedR0 0xffffffd7)
  , Case "terminate-invalid-class" Terminate (3 `shiftL` 16) 0 setupRunning 0x53 False Nothing (ReturnedR0 0xffffffee)
  , Case "terminate-masked" Terminate (idOf d 2) 0 (\e -> setupRunning e >>> makeReady d 2 3 False) 0xd3 False Nothing (ReturnedR0 0xffffffe7)
  , Case "priority-ready" ChangePriority (idOf d 2) 2 (\e -> setupRunning e >>> makeReady d 2 5 False) 0x53 False Nothing (ReturnedR0 0)
  , Case "priority-invalid" ChangePriority (idOf d 2) 17 setupRunning 0x53 False Nothing (ReturnedR0 0xffffffef)
  , Case "timed-sleep-wakeup" TimedSleep 5 0 (\e -> setupRunning e >>> setB (taskField (taskAt d 1) 0x0e) 2) 0x53 False Nothing (ReturnedR0 0)
  , Case "timed-sleep-zero" TimedSleep 0 0 setupRunning 0x53 False Nothing (ReturnedR0 0xffffffce)
  , Case "timed-sleep-block" TimedSleep 50 0 setupRunning 0x53 False Nothing (StoppedAt (switchOf d) (stopReason TimedSleep))
  , Case "release-sleeping" ReleaseWait (idOf d 2) 0 (\e -> setupRunning e >>> setWait d 2 0x40059100 0x40059000 (domainId d) >>> armTimer d 2) 0x53 False Nothing (ReturnedR0 0)
  , Case "release-ready" ReleaseWait (idOf d 2) 0 (\e -> setupRunning e >>> makeReady d 2 3 False) 0x53 False Nothing (ReturnedR0 0xffffffd7)
  , Case "handler-release" HandlerRelease (idOf d 2) 0 (\e -> setupRunning e >>> setWait d 2 0x40059100 0x40059000 (domainId d) >>> armTimer d 2 >>> setW (handlerCell d) 1) 0x53 True Nothing (ReturnedR0 0)
  , Case "delay-block" Delay 30 0 setupRunning 0x53 False Nothing (StoppedAt (switchOf d) (stopReason Delay))
  , Case "delay-negative" Delay 0xffffffff 0 setupRunning 0x53 False Nothing (ReturnedR0 0xffffffef)
  , Case "timed-sleep-return" TimedSleep 50 0 setupRunning 0x53 False (Just 50) (ReturnedR0 0xffffffce)
  , Case "delay-return" Delay 30 0 setupRunning 0x53 False (Just 0) (ReturnedR0 0)
  , Case "release-cross-domain" Terminate (idOf (other d) 2) 0 (\e -> setupRunning e >>> makeReady (other d) 2 3 True >>> setW (dispatchCell (other d)) 1) 0x53 False Nothing (ReturnedR0 0)
  , Case "terminate-local-dispatch" Terminate (idOf d 2) 0 (\e -> setupRunning e >>> makeReady d 2 0 True) 0x53 False Nothing (StoppedAt (switchOf d) (stopReason Terminate))
  , Case "release-local-dispatch" ReleaseWait (idOf d 2) 0 (\e -> setupRunning e >>> setB (taskField (taskAt e 1) 0x0d) 5 >>> setWait d 2 0x40059100 0x40059000 (domainId d) >>> armTimer d 2) 0x53 False Nothing (StoppedAt (switchOf d) (stopReason ReleaseWait))
  , Case "release-cross-dispatch" ReleaseWait (idOf (other d) 2) 0 (\e -> setupRunning e >>> setWait (other d) 2 0x40059100 0x40059000 (domainId (other d)) >>> armTimer (other d) 2 >>> setW (dispatchCell (other d)) 1) 0x53 False Nothing (ReturnedR0 0)
  ] ++ timeoutCases d
  where
    other domain = if domain == DomainA then DomainB else DomainA

fixtureWait, fixtureQueue, fixtureTimer :: Domain -> Word32
fixtureWait d = globalsBase d + 0xa100
fixtureQueue d = globalsBase d + 0xa200
fixtureTimer d = globalsBase d + 0xa300

objectWaitTask :: Domain -> Word32 -> W -> W
objectWaitTask d slot =
  let task = taskAt d slot
      wait = fixtureWait d
      queue = fixtureQueue d
      timer = fixtureTimer d
   in setB (taskField task 0x0c) 0x12
      >>> setB (taskField task 0x0d) 3
      >>> setW (taskField task 0x14) wait
      >>> setW (taskField task 0x00) queue
      >>> setW (taskField task 0x04) queue
      >>> setW (taskField task 0x24) 0
      >>> setW (taskField task 0x28) 0
      >>> setW (taskField task 0x2c) 0
      >>> setW (taskField task 0x30) 0
      >>> setW wait timer
      >>> setW (wait + 4) queue
      >>> setW (wait + 8) (domainId d)
      >>> setW queue task
      >>> setW (queue + 4) task
      >>> setW timer 1
      >>> setW (timer + 4) (timeoutCallback d)
      >>> setW (timer + 8) task
      >>> setW (lockWord d) 1

sleepingWait :: Domain -> Word32 -> Word32 -> W -> W
sleepingWait d slot callback =
  let task = taskAt d slot
      wait = fixtureWait d
      timer = fixtureTimer d
   in setB (taskField task 0x0c) 0x0a
      >>> setB (taskField task 0x0d) 3
      >>> setW (taskField task 0x14) wait
      >>> setW (taskField task 0x24) 0
      >>> setW (taskField task 0x28) 0
      >>> setW (taskField task 0x2c) 0
      >>> setW (taskField task 0x30) 0
      >>> setW wait timer
      >>> setW (wait + 4) 0
      >>> setW (wait + 8) (domainId d)
      >>> setW timer 1
      >>> setW (timer + 4) callback
      >>> setW (timer + 8) task
      >>> setW (lockWord d) 1

timeoutCases :: Domain -> [Case]
timeoutCases d =
  [ Case "timeout-object-queued" Timeout (taskAt d 2) 0 objectTimeout 0x93 False Nothing (ReturnedR0 1)
  , Case "timeout-sleeping" Timeout (taskAt d 2) 0 sleepingTimeout 0x93 False Nothing (ReturnedR0 1)
  , Case "timeout-terminate-pending" Timeout (taskAt d 2) 0 terminatePending 0x93 False Nothing (ReturnedR0 1)
  , Case "timeout-priority-pending" Timeout (taskAt d 2) 0 priorityPending 0x93 False Nothing (ReturnedR0 1)
  , Case "delay-elapsed" DelayCallback (taskAt d 2) 0 delayElapsed 0x93 False Nothing (ReturnedR0 1)
  ]
  where
    objectTimeout e = setupRunning e >>> setW (selectedCell e) 0 >>> objectWaitTask e 2
    sleepingTimeout e = setupRunning e >>> setW (selectedCell e) 0 >>> sleepingWait e 2 (timeoutCallback e)
    terminatePending e = objectTimeout e >>> setW (taskField (taskAt e 2) 0x24) 1 >>> setB (taskField (taskAt e 2) 0x0e) 1
    priorityPending e = objectTimeout e >>> setW (taskField (taskAt e 2) 0x28) 1 >>> setW (taskField (taskAt e 2) 0x34) 7
    delayElapsed e = setupRunning e >>> setW (selectedCell e) 0 >>> sleepingWait e 2 (delayCallback e)

data ControlRun = ControlRun
  { controlName :: String
  , controlHolds :: Bool
  }

timerDetachment :: TimerContract -> Domain -> Case -> Maybe TimerIndexCell
timerDetachment RetainedTimer _ _ = Nothing
timerDetachment OwnedDetachedTimer d one
  | caseService one `elem` [ReleaseWait, HandlerRelease]
  , succeeded (caseExpected one)
  , let TimerIndexCell _ (GuestAddress address) = armedTimerIndex
  , M.lookup (fromIntegral address) (caseSetup one d (baseFixture d)) == Just 1 = Just armedTimerIndex
  | otherwise = Nothing
  where
    succeeded (ReturnedR0 value) = value == 0
    succeeded (StoppedAt _ _) = True

timerIndexState :: TimerIndexCell -> NativeObservation -> Maybe TimerIndexState
timerIndexState (TimerIndexCell domain (GuestAddress address)) observation = do
  bytes <- lookup (RegionId (if domain == DomainA then "data-a" else "data-b")) (nativeMemory observation)
  let offset = fromIntegral (address - fromIntegral (globalsBase domain))
      encoded = B.take 4 (B.drop offset bytes)
  if encoded == word32le 0 then Just DetachedIndex
    else if encoded == word32le 1 then Just LinkedIndex else Nothing

indexView :: TimerIndexCell -> TimerIndexState -> NativeObservation -> NativeObservation
indexView (TimerIndexCell domain (GuestAddress address)) state observation = observation
  { nativeMemory =
      [(ident, if ident == region then B.take offset bytes <> encoded <> B.drop (offset + 4) bytes else bytes)
      | (ident, bytes) <- nativeMemory observation]
  }
  where
    region = RegionId (if domain == DomainA then "data-a" else "data-b")
    offset = fromIntegral (address - fromIntegral (globalsBase domain))
    encoded = word32le (case state of LinkedIndex -> 1; DetachedIndex -> 0)

indexWrites :: TimerIndexCell -> NativeObservation -> Maybe [(WriteBytes, WriteValue)]
indexWrites (TimerIndexCell _ address) observation = do
  kernel <- nativeKernel observation
  events <- observedOrder kernel
  pure [(bytes, value) | OrderedWrite (Write target bytes) value <- events, target == address]

detachmentView :: Maybe TimerIndexCell -> NativeObservation -> NativeObservation
detachmentView Nothing observation = observation
detachmentView (Just cell@(TimerIndexCell _ address)) observation =
  (indexView cell LinkedIndex observation) { nativeKernel = fmap clean (nativeKernel observation) }
  where
    clean kernel = kernel { observedOrder = fmap (filter keep) (observedOrder kernel) }
    keep (OrderedWrite (Write target (WriteBytes 4)) (WriteValue 0)) = target /= address
    keep _ = True

detachmentControls :: Maybe TimerIndexCell -> NativeObservation -> NativeObservation -> [ControlRun]
detachmentControls Nothing _ _ = []
detachmentControls (Just cell) stock owned =
  [ ControlRun "stock-linked-timer-index" (timerIndexState cell stock == Just LinkedIndex && indexWrites cell stock == Just [])
  , ControlRun "owned-detached-timer-index" (detached owned)
  , ControlRun "timer-index-corruption-refused" (not (detached (indexView cell LinkedIndex owned)))
  ]
  where
    detached observation = timerIndexState cell observation == Just DetachedIndex && indexWrites cell observation == Just [(WriteBytes 4, WriteValue 0)]

controlRuns :: TimerContract -> Domain -> Case -> CaseSpec -> NativeObservation -> NativeObservation -> [ControlRun]
controlRuns timerContract d one spec stock owned =
  frameControls ++ detachmentControls (timerDetachment timerContract d one) stock owned
  ++ [ControlRun name (refused (mutate offset owned)) | caseService one `elem` [Timeout, DelayCallback], (name, offset) <- targets]
  where
    refused mutated = not (null (compareKernelCase adjusted (normalize stock) (normalize mutated)))
    frameOffset = fromIntegral (stackTop - stackBase)
    callerFramePreserved observation = case lookup (RegionId "stack") (nativeMemory observation) of
      Just bytes -> B.length bytes == 0x1000 && B.drop frameOffset bytes == B.replicate (0x1000 - frameOffset) 0
      Nothing -> False
    mutateFrame offset = owned { nativeMemory =
      [(ident, if ident == RegionId "stack" then flipAt offset bytes else bytes) | (ident, bytes) <- nativeMemory owned] }
    frameControls =
      [ControlRun "stock-caller-frame" (callerFramePreserved stock), ControlRun "owned-caller-frame" (callerFramePreserved owned)]
      ++ case lookup (RegionId "stack") (nativeMemory owned) of
        Just bytes | B.length bytes > frameOffset ->
          [ ControlRun "incoming-frame-mutation" (not (null (compareKernelCase adjusted (normalize owned) (normalize (mutateFrame frameOffset)))))
          , ControlRun "private-stack-insensitive" (null (compareKernelCase adjusted (normalize owned) (normalize (mutateFrame 0))))
          ]
        _ -> [ControlRun "incoming-frame-observation" False]
    adjusted = case caseKernel spec of
      Just kernel | null (kernelStop kernel) -> spec
      _ -> spec { caseKernel = fmap (\k -> k { kernelUncompared = kernelUncompared k ++ [R4, R5, R6, R7, R8, R9, R10, R11, Sp] }) (caseKernel spec) }
    offsetOf address = fromIntegral (address - globalsBase d)
    targets =
      [ ("wait-result-mutation", offsetOf (fixtureWait d))
      , ("task-state-mutation", offsetOf (taskAt d 2 + 0x0c))
      , ("object-lock-mutation", offsetOf (objectWord d))
      ]
    mutate offset observed =
      observed
        { nativeMemory =
            [ (ident, if ident == fixtureRegion then flipAt offset bytes else bytes)
            | (ident, bytes) <- nativeMemory observed
            ]
        }
    fixtureRegion = RegionId (if d == DomainA then "data-a" else "data-b")
    flipAt offset bytes = B.take offset bytes <> B.cons (B.index bytes offset `xor` 0xff) (B.drop (offset + 1) bytes)

-- | A four-byte branch to the stub, and the stub's save/wake/resume body.
stubWords :: Domain -> Word32 -> [Word32]
stubWords d result = prefix
  where
    prefix =
      [ 0xe92d4ff0
      , movw 1 (runningCell d .&. 0xffff)
      , movt 1 (runningCell d `shiftR` 16)
      , 0xe5911000
      , 0xe5811018
      , 0xe5912014
      , 0xe3a03000
      ] ++ (if result == 0 then [] else [0xe2433000 .|. (result .&. 0xff)]) ++
      [ 0xe5823000
      , 0xe5d1300c
      , 0xe3833001
      , 0xe5c1300c
      , 0xe8bd4ff0
      , 0xe12fff1e
      ]

movw, movt :: Word32 -> Word32 -> Word32
movw register immediate = 0xe3000000 .|. (register `shiftL` 12) .|. ((immediate .&. 0xf000) `shiftL` 4) .|. (immediate .&. 0xfff)
movt register immediate = 0xe3400000 .|. (register `shiftL` 12) .|. ((immediate .&. 0xf000) `shiftL` 4) .|. (immediate .&. 0xfff)

poke :: B.ByteString -> Word32 -> B.ByteString -> B.ByteString
poke bytes offset value = B.take index bytes <> value <> B.drop (index + B.length value) bytes
  where index = fromIntegral offset

slice :: B.ByteString -> Word32 -> Word32 -> Word32 -> B.ByteString
slice owned ownedBase address size = B.take (fromIntegral size) (B.drop (fromIntegral (address - ownedBase)) owned)

ownedBaseOf :: Domain -> Word32
ownedBaseOf DomainA = 0x44318
ownedBaseOf DomainB = 0xd42d8

patchOwned :: Domain -> B.ByteString -> Word32 -> B.ByteString -> B.ByteString
patchOwned d loader ownedBase owned =
  let base = loaderBase d
      patches =
        [(entryOf d service, 4, entryOf d service) | service <- [Terminate, ChangePriority, TimedSleep, ReleaseWait, HandlerRelease, Delay, Timeout, DelayCallback]]
        ++ [(0x46e4c + if d == DomainB then 0x8ffc0 else 0, 0x180, 0x46e4c + if d == DomainB then 0x8ffc0 else 0)]
        ++ [(if d == DomainA then 0x50000 else 0xe0000, 0x4000, if d == DomainA then 0x50000 else 0xe0000)]
   in foldl (\bytes (address, size, source) -> poke bytes (address - base) (slice owned ownedBase source size)) (loader <> B.replicate 0x12000 0) patches


dataCode :: Domain -> W -> B.ByteString
dataCode d m = serialize (globalsBase d) globalsSize m

serialize :: Word32 -> Word32 -> W -> B.ByteString
serialize base size m = foldl write (B.replicate (fromIntegral size) 0) (M.toList m)
  where
    write bytes (address, value)
      | address >= base && address + 4 <= base + size = poke bytes (address - base) (word32le value)
      | otherwise = bytes

stopReason :: Service -> Word32
stopReason service = case service of
  Terminate -> 0xfffffff4
  ChangePriority -> 0xfffffff3
  TimedSleep -> 0xffffffee
  ReleaseWait -> 0xffffffeb
  HandlerRelease -> 0
  Delay -> 0xffffffe7
  Timeout -> 0
  DelayCallback -> 0

helperRanges :: Domain -> [(Word32, Word32)]
helperRanges d =
  [(acquireFn d, 0x3c), (releaseFn d, 0x40),
   (if d == DomainA then 0x44744 else 0xd4704, if d == DomainA then 0xbc else 0xc0),
   (if d == DomainA then 0x43c48 else 0xd3c04, 0x5c),
   (if d == DomainA then 0x433e4 else 0xd33e4, 0x60),
   (if d == DomainA then 0x44018 else 0xd3fd4, if d == DomainA then 0x2c else 0x30)]
  ++ [(base + early, size)
     | (base, size) <- ([(0x43ca4, 0x7c), (0x43d20, 0xb0), (0x43dd0, 0x1a0), (0x43f70, 0xa8)] :: [(Word32, Word32)])]
  ++ [(base + late, size)
     | (base, size) <- ([(0x44044, 0xf0), (0x444e4, 0x84), (0x44568, 0xcc), (0x446fc, 0x48), (0x449c0, 0x100), (0x44ac0, 0xc4), (0x44b84, 0xdc), (0x44c60, 0x48), (0x44ca8, 0xe4)] :: [(Word32, Word32)])]
  where
    early = if d == DomainB then 0x8ffbc else 0
    late = if d == DomainB then 0x8ffc0 else 0

installAggregate :: Domain -> B.ByteString -> Aggregate -> B.ByteString
installAggregate d code (Aggregate image _ _ carriers) = foldl install padded carriers
  where
    top = maximum (loaderBase d + fromIntegral (B.length code) :
      [high | Regions.Extent _ (Regions.GuestAddress high) <- carriers])
    padded = code <> B.replicate (fromIntegral (top - loaderBase d) - B.length code) 0
    install current (Regions.Extent (Regions.GuestAddress low) (Regions.GuestAddress high)) =
      poke current (low - loaderBase d) (either error id (Agg.sliceAggregate image low (high - low)))

mergeSpans :: FetchLayout -> [(Word32, Word32)] -> [(Word32, Word32)]
mergeSpans layout spans = go (sortOn fst spans)
  where
    go []=[]
    go ((start,size):rest)=extend start (start+size) rest
    extend start end []=[(start,end-start)]
    extend start end ((lo,size):rest)
      | lo < end || (layout == AllocatedFunctions && lo == end) = extend start (max end (lo + size)) rest
      | otherwise=(start,end-start):extend lo (lo+size) rest

specFor :: Domain -> Role -> Case -> B.ByteString -> B.ByteString -> Maybe (Aggregate, Aggregate) -> B.ByteString -> CaseSpec
specFor d role one loaderA loaderB aggregates owned = CaseSpec
  { caseId = caseName one ++ "-" ++ domainLabel d
  , caseProvenance = "X-M1 1.50 S3d lifecycle services, x70view 55436fc85c0560dd252b0410af6f2427fe198cd3; synthetic scheduler state"
  , caseExclusion = "One service call. Blocking cases stop at the dispatcher entry; lifecycle cases replace the dispatcher with a save/wake/return stub so the original caller returns."
  , caseEntry = GuestAddress (fromIntegral (entryOf d (caseService one)))
  , caseExit = GuestAddress (fromIntegral exitAddress)
  , caseRegions =
      [ mkRegion (RegionId "code") (GuestAddress (fromIntegral (loaderBase d))) (fromIntegral (B.length code)) ReadExecute code
      , mkRegion (RegionId "code-other") (GuestAddress (fromIntegral (loaderBase other))) (fromIntegral (B.length otherCode)) ReadExecute otherCode
      , mkRegion (RegionId "stub") (GuestAddress (fromIntegral stubBase)) 0x100 ReadExecute stub
      , mkRegion (RegionId "data-a") (GuestAddress (fromIntegral (globalsBase DomainA))) (fromIntegral globalsSize) ReadWrite (dataCode DomainA fixture)
      , mkRegion (RegionId "data-b") (GuestAddress (fromIntegral (globalsBase DomainB))) (fromIntegral globalsSize) ReadWrite (dataCode DomainB fixture)
      , mkRegion (RegionId "stack") (GuestAddress (fromIntegral stackBase)) 0x1000 ReadWrite (B.replicate 0x1000 0)
      , mkRegion (RegionId "exit") (GuestAddress (fromIntegral exitAddress)) 0x10 ReadExecute (word32le 0xe7f000f0)
      , mkRegion (RegionId "bank-a") (GuestAddress 0xfffb30cc) 4 Device (word32le 0)
      , mkRegion (RegionId "bank-b") (GuestAddress 0xfffb40cc) 4 Device (word32le 0)
      ] ++ bodyRegions
  , caseFunctions =
      [(GuestAddress (fromIntegral start), end - start)
      | (base, size) <- mergeSpans (if null aggregateList then OriginalFunctions else AllocatedFunctions) (ownedFunctions ++ aggregateFunctions)
      , let boundaries = base : filter (\address -> base < address && address < base + size) publicEntries ++ [base + size]
      , (start, end) <- zip boundaries (drop 1 boundaries)]
  , caseInitial =
      [(R0, caseArgument one), (R1, caseArgument1 one), (R2, 0), (R3, 0), (R12, 0)]
      ++ [(register, 0xabc00000 + fromIntegral (fromEnum register)) | register <- [R4 .. R11]]
      ++ [(Sp, stackTop), (Lr, exitAddress), (Cpsr, caseCpsr one)]
  , caseExpectedStatus = expectedStatus
  , caseExpectedRegisters = expectedRegisters
  , caseExpectedMemory = []
  , caseCp14Identity = Just (if d == DomainA then 0x4003 else 0x4004)
  , caseKernel = Just (KernelCase (InstructionLimit 4000) stop (Just 0x80001234) (Just 0) (Just 0) uncompared [])
  }
  where
    other = if d == DomainA then DomainB else DomainA
    loader = if d == DomainA then loaderA else loaderB
    otherLoader = if other == DomainA then loaderA else loaderB
    fixture = setW (handlerCell d) (if caseHandler one then 1 else 0) (caseSetup one d (baseFixture d))
    publicEntries = M.keys (M.fromList
      [(address, ()) | address <- entryOf d (caseService one) :
        [Agg.symbolAddress symbol | aggregate <- aggregateList, let Aggregate _ symbols _ _ = aggregate, symbol <- symbols, "xm1_platform_veneer_" `isPrefixOf` Agg.symbolName symbol]])
    aggregateList = case (role, aggregates) of
      (Owned, Just (a, b)) -> [a, b]
      _ -> []
    carriers = concat [extents | Aggregate _ _ _ extents <- aggregateList]
    retained (address, size) = all (\(Regions.Extent (Regions.GuestAddress low) (Regions.GuestAddress high)) ->
      toInteger address + toInteger size <= toInteger low || high <= address) carriers
    ownedFunctions =
      [(stubBase, 0x80)]
      ++ filter retained (helperRanges d ++ helperRanges other)
      ++ [(entryOf d service, extentOf service) | service <- [Terminate, ChangePriority, TimedSleep, ReleaseWait, HandlerRelease, Delay, Timeout, DelayCallback], null aggregateList]
      ++ [(switchOf d, 4) | null aggregateList]
      ++ [(bodyBase, 0x4000) | role == Owned, Nothing <- [aggregates]]
    aggregateFunctions = case (role, aggregates) of
      (Owned, Just (Aggregate _ symbolsA placementsA _, Aggregate _ symbolsB placementsB _)) ->
        [(Agg.symbolAddress symbol, Agg.symbolSize symbol) | symbol <- symbolsA ++ symbolsB]
        ++ [(Agg.placementAddress placement, Agg.placementBytes placement) | placement <- placementsA ++ placementsB, Agg.placementBytes placement > 0, let section = Agg.placementSection placement, section == ".text" || ".text." `isPrefixOf` section]
      _ -> []
    bodyBase = if d == DomainA then 0x50000 else 0xe0000
    aggregateFor domain = fmap (\(a, b) -> if domain == DomainA then a else b) aggregates
    ownedCode domain original = case aggregateFor domain of
      Just aggregate -> installAggregate domain original aggregate
      Nothing | domain == d -> B.take 0x10000 (patchOwned d original (ownedBaseOf d) owned)
      Nothing -> original
    code = patchStub d (if role == Stock then loader else ownedCode d loader) (caseLifecycle one)
    otherCode = if role == Stock then otherLoader else ownedCode other otherLoader
    bodyRegions = case (role, aggregates) of
      (Owned, Nothing) ->
        [mkRegion (RegionId "bodies") (GuestAddress (fromIntegral bodyBase)) 0x4000 ReadExecute
          (slice owned (ownedBaseOf d) bodyBase 0x4000)]
      _ -> []
    stub = case caseLifecycle one of
      Nothing -> B.replicate 0x100 0
      Just result ->
        let stubCode = B.concat (map word32le (stubWords d result))
         in stubCode <> B.replicate (0x100 - B.length stubCode) 0
    (stop, expectedStatus, expectedRegisters) = case caseExpected one of
      ReturnedR0 value -> (Nothing, Returned, [(R0, value)])
      StoppedAt address reason -> (Just (GuestAddress (fromIntegral address)), StopAddress, [(R0, reason)])
    uncompared = [R1, R2, R3, R12, Lr, Pc, Cpsr]

patchStub :: Domain -> B.ByteString -> Maybe Word32 -> B.ByteString
patchStub d loader lifecycle = case lifecycle of
  Nothing -> loader
  Just _ -> poke loader (switchOf d - loaderBase d) (word32le (branch (switchOf d) stubBase))

branch :: Word32 -> Word32 -> Word32
branch from to = 0xea000000 .|. (((to - from - 8) `shiftR` 2) .&. 0xffffff)

firstDifference :: NativeObservation -> NativeObservation -> String
firstDifference stock owned =
  case [ (ident, index, x, y) | ident <- [RegionId "data-a", RegionId "data-b"], Just (index, x, y) <- [firstByte ident] ] of
    ((ident, index, x, y) : _) -> show (regionName ident, index, x, y) ++ cpsr
    [] -> "no data difference" ++ cpsr
  where
    cpsr = " cpsr " ++ show (M.lookup Cpsr (nativeRegisters stock)) ++ " / " ++ show (M.lookup Cpsr (nativeRegisters owned))
    firstByte ident = do
      a <- lookup ident (nativeMemory stock)
      b <- lookup ident (nativeMemory owned)
      (index, (x, y)) <- find (\(_, (x, y)) -> x /= y) (zip [0 :: Int ..] (zip (B.unpack a) (B.unpack b)))
      pure (index, x, y)

normalize :: NativeObservation -> NativeObservation
normalize observation = observation
  { nativeMemory =
      [(ident, if ident == RegionId "stack" then B.drop (fromIntegral (stackTop - stackBase)) bytes else bytes)
      | (ident, bytes) <- nativeMemory observation, ident /= RegionId "stub"]
  , nativeKernel = fmap clean (nativeKernel observation)
  }
  where
    clean kernel = kernel { observedOrder = fmap (filter keep) (observedOrder kernel) }
    keep (OrderedWrite (Write (GuestAddress address) (WriteBytes bytes)) _) =
      not (address >= fromIntegral stackBase && toInteger address + toInteger bytes <= toInteger stackTop)
    keep _ = True

judgeOutcome :: Expected -> RunnerStatus -> Word32 -> Word32 -> Bool
judgeOutcome (ReturnedR0 value) Returned r0 _ = r0 == value
judgeOutcome (StoppedAt address reason) StopAddress r0 pc = pc == address && r0 == reason
judgeOutcome _ _ _ _ = False

judgeSelfTest :: Bool
judgeSelfTest = placementSelfTest && and
  [ not (judgeOutcome (ReturnedR0 0) UnknownCall 0 0)
  , not (judgeOutcome (ReturnedR0 0) Returned 1 0)
  , not (judgeOutcome (StoppedAt 0x1000 0xffffff18) StopAddress 0xffffff18 0x1004)
  , not (judgeOutcome (StoppedAt 0x1000 0xffffff18) StopAddress 0xffffff19 0x1000)
  , not (judgeOutcome (StoppedAt 0x1000 0xffffff18) Returned 0xffffff18 0x1000)
  , judgeOutcome (ReturnedR0 0) Returned 0 0
  , judgeOutcome (StoppedAt 0x1000 0xffffff18) StopAddress 0xffffff18 0x1000
  ]

aggregateOf :: Maybe Union.DomainMap -> Maybe FilePath -> Maybe FilePath -> Maybe FilePath -> IO (Maybe Aggregate)
aggregateOf _ Nothing Nothing Nothing = pure Nothing
aggregateOf (Just domain) (Just binPath) (Just symbolsPath) (Just placementsPath) = do
  bytes <- B.readFile binPath
  symbols <- either fail pure . Agg.parseAggregateSymbols =<< B.readFile symbolsPath
  placements <- either fail pure . Agg.parsePlacements =<< B.readFile placementsPath
  unless (not (null placements)) (fail "aggregate placements are empty")
  image <- either fail pure (Agg.aggregateImage symbols bytes)
  forM_ symbols $ \symbol ->
    unless (symbolAllocated (Union.mapEntries domain) placements symbol)
      (fail ("aggregate symbol has no allocated placement or fixed ABI entry: " ++ Agg.symbolName symbol))
  forM_ placements $ \placement ->
    either fail (const (pure ())) (Agg.sliceAggregate image (Agg.placementAddress placement) (Agg.placementBytes placement))
  pure (Just (Aggregate image symbols placements (map Union.carrierExtent (Union.mapCarriers domain))))
aggregateOf _ _ _ _ = fail "aggregate mode requires binary, symbols and placements for both domains"

symbolAllocated :: [Union.Entry] -> [Agg.SectionPlacement] -> Agg.AggregateSymbol -> Bool
symbolAllocated entries placements symbol =
  any contains placements || any fixedEntry entries
  where
    contains placement =
      Agg.symbolAddress symbol >= Agg.placementAddress placement &&
      toInteger (Agg.symbolAddress symbol) + toInteger (Agg.symbolSize symbol) <=
        toInteger (Agg.placementAddress placement) + toInteger (Agg.placementBytes placement)
    fixedEntry entry =
      Agg.symbolName symbol == Union.entryVeneer entry &&
      Regions.GuestAddress (Agg.symbolAddress symbol) == Union.entryAddress entry &&
      Agg.symbolSize symbol == 4

placementSelfTest :: Bool
placementSelfTest = and
  [ symbolAllocated [entry] [] veneer
  , not (symbolAllocated [entry] [] (veneer { Agg.symbolAddress = 0x50000 }))
  , not (symbolAllocated [entry] [] (veneer { Agg.symbolSize = 8 }))
  , not (symbolAllocated [entry] [] (veneer { Agg.symbolName = "body" }))
  , symbolAllocated [] [placement] (Agg.AggregateSymbol "body" 0x454f4 8)
  , not (symbolAllocated [] [placement] (Agg.AggregateSymbol "body" 0x454f4 12))
  , not (symbolAllocated [] [] (Agg.AggregateSymbol "body" 0x50000 4))
  ]
  where
    entry = Union.Entry Union.S3d "terminate" (Regions.GuestAddress 0x454f0) "body" "veneer"
    veneer = Agg.AggregateSymbol "veneer" 0x454f0 4
    placement = Agg.SectionPlacement 0x454f4 8 ".text.body"

optionalArgument :: String -> IO (Maybe FilePath)
optionalArgument name = do
  arguments <- getArgs
  case reverse (argumentValues name arguments) of
    [] | name `elem` arguments -> fail (name ++ " needs a value")
       | otherwise -> pure Nothing
    value : _ -> Just <$> makeAbsolute value

data Cohort = BoundedServices | ContextTransfer deriving (Eq, Show)

workerBase, workerTop :: Word32
workerBase = 0x40072000
workerTop = workerBase + 0x800

selectedSymbol :: Aggregate -> String -> Word32
selectedSymbol (Aggregate _ symbols _ _) name =
  case filter ((== name) . Agg.symbolName) symbols of
    [symbol] -> Agg.symbolAddress symbol
    _ -> error ("missing or ambiguous context-transfer symbol: " ++ name)

resumeAddress :: Role -> Domain -> Aggregate -> Word32
resumeAddress Stock d _ = switchOf d + 0x28
resumeAddress Owned _ aggregate = selectedSymbol aggregate "xm1_rtos_scheduler_resume"

transferCases :: Domain -> [Case]
transferCases d =
  [one { caseLifecycle = Nothing } | one <- cases d, caseName one `elem` ["timed-sleep-return", "delay-return"]]

transferSpec :: Domain -> Role -> Case -> B.ByteString -> B.ByteString -> (Aggregate, Aggregate) -> CaseSpec
transferSpec d role one loaderA loaderB pair = base
  { caseId = "context-transfer-" ++ caseId base
  , caseExclusion = "Two explicit task stacks; foreign worker drives handler/clock state, calls the real timer tick and dispatcher. No owned instruction is replaced."
  , caseRegions = map replace (caseRegions base) ++
      [mkRegion (RegionId "worker-stack") (guest workerBase) 0x1000 ReadWrite workerStack]
  , caseFunctions = filter (\(address, _) -> (address /= guest (switchOf d) || role /= Stock) && address /= guest stubBase) (caseFunctions base)
      ++ [(guest stubBase, fromIntegral (4 * length worker))] ++ stockScheduler ++ [(guest (tickEntry d), 0x1a0) | role == Stock]
  , caseExpectedRegisters = caseExpectedRegisters base ++
      [(r, initialCallee r) | r <- [R4 .. R11]] ++ [(Sp, stackTop)]
  , caseKernel = fmap (\k -> k
      { kernelLimit = InstructionLimit 20000
      , kernelStackContexts = map context [(stackBase, stackTop), (workerBase, workerTop)]
      }) (caseKernel base)
  }
  where
    guest = GuestAddress . fromIntegral
    context (low, high) = either error id (mkStackContext (guest low) (guest high))
    base = specFor d role one loaderA loaderB (Just pair) B.empty
    fixture = (makeReady d 2 15 False
      >>> setW (taskAt d 2 + 0x18) workerTop
      >>> setW (taskAt d 2 + 0x1c) stubBase
      >>> setW (definitionAt d 2 + 0x14) workerBase
      >>> setW (definitionAt d 2 + 0x10) 0x800) (caseSetup one d (baseFixture d))
    workerStack = B.replicate 0x1000 0xa5
    worker = workerWords d (1000 + caseArgument one)
    replace region@(CaseRegion mapped _) =
      case regionIdOf mapped of
        RegionId "data-a" -> mkRegion (RegionId "data-a") (guest (globalsBase DomainA)) (fromIntegral globalsSize) ReadWrite (dataCode DomainA fixture)
        RegionId "data-b" -> mkRegion (RegionId "data-b") (guest (globalsBase DomainB)) (fromIntegral globalsSize) ReadWrite (dataCode DomainB fixture)
        RegionId "stack" -> mkRegion (RegionId "stack") (guest stackBase) 0x1000 ReadWrite (B.replicate 0x1000 0xa5)
        RegionId "stub" -> mkRegion (RegionId "stub") (guest stubBase) 0x100 ReadExecute
          (B.concat (map word32le worker) <> B.replicate (0x100 - 4 * length worker) 0)
        _ -> region
    stockScheduler = [(guest (switchOf d + low), high - low) | role == Stock, (low, high) <- [(0,0x28),(0x28,0x48),(0x48,0x6c),(0x6c,0xe8)]]
      ++ [(guest (loaderBase d + 0x252c), 40) | role == Stock]

initialCallee, workerCallee :: Register -> Word32
initialCallee r = 0xabc00000 + fromIntegral (fromEnum r)
workerCallee r = 0xdef00000 + fromIntegral (fromEnum r)

tickEntry :: Domain -> Word32
tickEntry DomainA = 0x44d8c
tickEntry DomainB = 0xd4d4c

workerWords :: Domain -> Word32 -> [Word32]
workerWords d expiry =
  concat [[movw (fromIntegral (fromEnum r)) (workerCallee r .&. 0xffff), movt (fromIntegral (fromEnum r)) (workerCallee r `shiftR` 16)] | r <- [R4 .. R11]] ++
  store (tickCell d) expiry ++ store (handlerCell d) 1 ++
  [0xf1080080, movw 12 (tickEntry d .&. 0xffff), movt 12 (tickEntry d `shiftR` 16), 0xe12fff3c] ++
  store (handlerCell d) 0 ++
  [0xf10c0080, movw 12 (switchOf d .&. 0xffff), movt 12 (switchOf d `shiftR` 16), 0xe12fff3c, 0xe7f000f0]
  where
    store address value =
      [movw 0 (address .&. 0xffff), movt 0 (address `shiftR` 16),
       movw 1 (value .&. 0xffff), movt 1 (value `shiftR` 16), 0xe5801000]

observationWord :: RegionId -> Word32 -> NativeObservation -> Maybe Word32
observationWord ident offset observation = do
  bytes <- lookup ident (nativeMemory observation)
  let encoded = B.take 4 (B.drop (fromIntegral offset) bytes)
  if B.length encoded /= 4 then Nothing else
    Just (sum [fromIntegral byte `shiftL` (8 * n) | (n, byte) <- zip [0 ..] (B.unpack encoded)])

transferFrame :: Domain -> Role -> Case -> Aggregate -> (Word32, [Word32], [Word32])
transferFrame d role one aggregate = (stackTop - frameBytes - 36, scheduler, service)
  where
    sleep = caseService one == TimedSleep
    frameBytes = case role of Stock -> if sleep then 24 else 32; Owned -> 40
    serviceRegisters = case role of
      Stock -> if sleep then [R4, R5, R6, R7, R11] else [R3, R4, R5, R6, R7, R8, R11]
      Owned -> [R3, R4, R5, R6, R7, R8, R9, R10, R11]
    service = [if r == R3 then if role == Owned && not sleep then _sleepResults d else 0 else initialCallee r | r <- serviceRegisters] ++ [exitAddress]
    result = _sleepResults d
    timer = _sleepTimers d
    expiry = 1000 + caseArgument one
    continuation = case role of
      Stock -> entryOf d (caseService one) + if sleep then 0x194 else 0x168
      Owned -> selectedSymbol aggregate (if sleep then "s3d_timed_sleep_body" else "s3d_delay_body") + if sleep then 0x1a4 else 0x140
    scheduler = case role of
      Stock | sleep -> [0, result, caseArgument one, 1, initialCallee R8, initialCallee R9, initialCallee R10, stackTop - 4, continuation]
      Stock -> [currentCell d, runningCell d, timer, result, caseArgument one, initialCallee R9, initialCallee R10, stackTop - 4, continuation]
      Owned | sleep -> [0x40, result, expiry, handlerCell d, timer, record, caseArgument one, stackTop - 8, continuation]
      Owned -> [expiry, taskAt d 1, 0x40, handlerCell d, timer, record, caseArgument one, stackTop - 8, continuation]
    record = contexts d + if d == DomainA then 0 else 0x84

transferControls :: Domain -> Role -> Case -> Aggregate -> NativeObservation -> [ControlRun]
transferControls d role one aggregate observation =
  checks ++ [ControlRun name (not (valid (corrupt ident offset observation))) | (name, ident, offset) <- mutations]
  where
    dataId = RegionId (if d == DomainA then "data-a" else "data-b")
    word address = observationWord dataId (address - globalsBase d)
    stackWord address = observationWord (RegionId "stack") (address - stackBase)
    (savedSp, savedFrame, serviceFrame) = transferFrame d role one aggregate
    timerState = if role == Stock then 1 else 0
    writes observed = nativeKernel observed >>= observedOrder
    values address observed = do
      events <- writes observed
      pure [fromIntegral value :: Word32 | OrderedWrite (Write (GuestAddress target) (WriteBytes 4)) (WriteValue value) <- events, target == fromIntegral address]
    memoryFrame observed = and [stackWord (savedSp + offset) observed == Just value | (offset, value) <- zip [0,4 ..] (savedFrame ++ serviceFrame)]
    workerFrame observed = and
      [observationWord (RegionId "worker-stack") (workerTop - workerBase - 36 + offset) observed == Just value
      | (offset, value) <- zip [0,4 ..] ([workerCallee r | r <- [R4 .. R11]] ++ [workerReturn])]
    workerReturn = stubBase + fromIntegral (4 * (length (workerWords d (1000 + caseArgument one)) - 1))
    guards observed = and
      [case lookup ident (nativeMemory observed) of
        Just bytes -> B.take 0x600 bytes == B.replicate 0x600 0xa5 && B.drop 0x800 bytes == B.replicate 0x800 0xa5
        Nothing -> False
      | ident <- [RegionId "stack", RegionId "worker-stack"]]
    checksFor :: NativeObservation -> [(String, Bool)]
    checksFor observed =
      [ ("initial-caller-return", nativeStatus observed == Returned && registerValue observed R0 == if caseService one == TimedSleep then 0xffffffce else 0)
      , ("caller-callee-registers", all (\r -> registerValue observed r == initialCallee r) [R4 .. R11])
      , ("caller-sp-cpsr", registerValue observed Sp == stackTop && registerValue observed Cpsr .&. 0x1ff == 0x53)
      , ("caller-saved-sp-pc", word (taskAt d 1 + 0x18) observed == Just savedSp && word (taskAt d 1 + 0x1c) observed == Just (resumeAddress role d aggregate))
      , ("caller-save-frame", memoryFrame observed)
      , ("worker-saved-sp-pc", word (taskAt d 2 + 0x18) observed == Just (workerTop - 36) && word (taskAt d 2 + 0x1c) observed == Just (resumeAddress role d aggregate))
      , ("worker-save-frame", workerFrame observed)
      , ("both-task-selections", values (runningCell d) observed == Just [taskAt d 2, taskAt d 1])
      , ("both-task-thread-identities", fmap (\events -> [value | OrderedSystemWrite Tpidrurw value <- events]) (writes observed) == Just [2,1])
      , ("caller-live-frame-and-stack-guards", guards observed)
      , ("timer-completion", word (timerCount d) observed == Just 0 && word (tickCell d - 4) observed == Just (1000 + caseArgument one) && word (tickCell d) observed == Just (1010 + caseArgument one) && word (_sleepTimers d) observed == Just timerState)
      , ("timer-index-write-contract", values (_sleepTimers d) observed == Just (if role == Stock then [1] else [1,0]))
      , ("wait-result", word (_sleepResults d) observed == Just (if caseService one == TimedSleep then 0xffffffce else 0))
      , ("ready-queues", word (selectedCell d) observed == Just (taskAt d 1) && word (runningCell d) observed == Just (taskAt d 1) && word (bitmapCell d) observed == Just 0x8001 && word (readyHeads d) observed == Just (taskAt d 1) && word (readyHeads d + 15 * 8) observed == Just (taskAt d 2))
      , ("private-stack-write-replay", all (stackReplay observed) [RegionId "stack", RegionId "worker-stack"])
      ]
    checks = [ControlRun name holds | (name, holds) <- checksFor observation]
    valid = all snd . checksFor
    mutations =
      [ ("saved-sp-corruption-refused", dataId, taskAt d 1 + 0x18 - globalsBase d)
      , ("saved-pc-corruption-refused", dataId, taskAt d 1 + 0x1c - globalsBase d)
      , ("saved-callee-corruption-refused", RegionId "stack", savedSp - stackBase)
      , ("service-frame-corruption-refused", RegionId "stack", stackTop - 4 - stackBase)
      , ("worker-frame-corruption-refused", RegionId "worker-stack", workerTop - 36 - workerBase)
      , ("timer-effect-corruption-refused", dataId, timerCount d - globalsBase d)
      , ("incoming-frame-corruption-refused", RegionId "stack", stackTop - stackBase)
      , ("worker-incoming-frame-corruption-refused", RegionId "worker-stack", workerTop - workerBase)
      , ("private-scratch-corruption-refused", RegionId "stack", 0x600)
      ]
    corrupt ident offset observed = observed { nativeMemory =
      [(key, if key == ident then poke bytes offset (word32le 0xbad0bad0) else bytes) | (key, bytes) <- nativeMemory observed] }

stackReplay :: NativeObservation -> RegionId -> Bool
stackReplay observation ident = case (lookup ident (nativeMemory observation), nativeKernel observation >>= observedOrder) of
  (Just actual, Just events) -> actual == foldl apply initial events
  _ -> False
  where
    base = if ident == RegionId "stack" then stackBase else workerBase
    initial = B.replicate 0x1000 0xa5
    apply bytes (OrderedWrite (Write (GuestAddress address) (WriteBytes size)) (WriteValue value))
      | address >= fromIntegral base && address + fromIntegral size <= fromIntegral base + 0x1000 =
          poke bytes (fromIntegral address - base) (B.pack [fromIntegral (value `shiftR` (8 * n)) | n <- [0 .. fromIntegral size - 1]])
    apply bytes _ = bytes

transferView :: Domain -> NativeObservation -> NativeObservation
transferView d observation = observation
  { nativeMemory = [(ident, project ident bytes) | (ident, bytes) <- nativeMemory observation]
  , nativeKernel = fmap (\k -> k { observedOrder = fmap (filter keep) (observedOrder k) }) (nativeKernel observation)
  }
  where
    private = [(stackTop - 0x200, stackTop), (workerTop - 0x200, workerTop),
      (taskAt d 1 + 0x18, taskAt d 1 + 0x20), (taskAt d 2 + 0x18, taskAt d 2 + 0x20),
      (_sleepTimers d, _sleepTimers d + 4)]
    project ident bytes = foldl clear bytes private
      where
        base = case ident of
          RegionId "stack" -> stackBase
          RegionId "worker-stack" -> workerBase
          RegionId "data-a" -> globalsBase DomainA
          RegionId "data-b" -> globalsBase DomainB
          _ -> 0
        clear current (low, high)
          | low >= base && toInteger high <= toInteger base + toInteger (B.length current) =
              poke current (low - base) (B.replicate (fromIntegral (high - low)) 0)
          | otherwise = current
    keep (OrderedWrite (Write (GuestAddress address) (WriteBytes size)) _) =
      not (any (\(low, high) -> address >= fromIntegral low && address + fromIntegral size <= fromIntegral high) private)
    keep _ = True

readonlyTransfer :: CaseSpec -> CaseSpec
readonlyTransfer spec = spec
  { caseId = caseId spec ++ "-readonly-scheduler"
  , caseEntry = GuestAddress 0x930000
  , caseExclusion = "A disjoint permission probe attempts a readonly scheduler write; it does not execute a lifecycle service."
  , caseRegions = mkRegion (RegionId "readonly-probe") (GuestAddress 0x930000) 20 ReadExecute probe
      : [CaseRegion mapped (if regionIdOf mapped `elem` [RegionId "data-a", RegionId "data-b"] then ReadOnly else permission)
        | CaseRegion mapped permission <- caseRegions spec]
  , caseFunctions = (GuestAddress 0x930000, 20) : caseFunctions spec
  , caseExpectedStatus = MemoryGuard
  , caseExpectedRegisters = [(R4, initialCallee R4), (Cpsr, 0x53), (Pc, 0x93000c)]
  }
  where
    probe = B.concat (map word32le [0xe30405a4, 0xe3440005, 0xe3a01000, 0xe5801000, 0xe12fff1e])

readonlyHolds :: NativeObservation -> Bool
readonlyHolds observation =
  nativeStatus observation == MemoryGuard
    && registerValue observation R4 == initialCallee R4
    && registerValue observation Cpsr == 0x53
    && registerValue observation Pc == 0x93000c
    && maybe False (all noWrite) (nativeKernel observation >>= observedOrder)
  where
    noWrite (OrderedWrite _ _) = False
    noWrite _ = True

main :: IO ()
main = do
  arguments <- getArgs
  cohort <- case argumentValues "--cohort" arguments of
    [] -> pure BoundedServices
    ["bounded-services"] -> pure BoundedServices
    ["context-transfer"] -> pure ContextTransfer
    _ -> fail "--cohort must be bounded-services or context-transfer"
  runner <- argument "--runner" >>= makeAbsolute
  loaderA <- argument "--loader-a"
  loaderB <- argument "--loader-b"
  ownedA <- optionalArgument "--owned-a-bin"
  ownedB <- optionalArgument "--owned-b-bin"
  aggregateABin <- optionalArgument "--aggregate-a-bin"
  aggregateASymbols <- optionalArgument "--aggregate-a-symbols"
  aggregateAPlacements <- optionalArgument "--aggregate-a-placements"
  aggregateBBin <- optionalArgument "--aggregate-b-bin"
  aggregateBSymbols <- optionalArgument "--aggregate-b-symbols"
  aggregateBPlacements <- optionalArgument "--aggregate-b-placements"
  maps <- if all (== Nothing) [aggregateABin, aggregateASymbols, aggregateAPlacements, aggregateBBin, aggregateBSymbols, aggregateBPlacements]
    then pure Nothing
    else do
      carriers <- argument "--aggregate-carriers"
      Just <$> (either (fail . show) pure . Union.parseUnion =<< B.readFile carriers)
  cacheRoot <- argument "--cache-root"
  results <- argument "--results"
  loaderABytes <- B.readFile loaderA
  loaderBBytes <- B.readFile loaderB
  ownedABytes <- standaloneBytes aggregateABin ownedA
  ownedBBytes <- standaloneBytes aggregateBBin ownedB
  digestA <- sha256File loaderA
  digestB <- sha256File loaderB
  unless (sha256Hex digestA == "21366ffff7cebc6176b8a8125bf3a9974835a51070a58ea325f7eed8aabd8882") (fail "loader A is not the pinned image")
  unless (sha256Hex digestB == "8fb0c322a22443a3783cdab9071db17212a5629f6375646218ac1e043dc672ec") (fail "loader B is not the pinned image")
  createDirectoryIfMissing True cacheRoot
  createDirectoryIfMissing True (takeDirectory results)
  aggregateA <- aggregateOf (Union.unionA <$> maps) aggregateABin aggregateASymbols aggregateAPlacements
  aggregateB <- aggregateOf (Union.unionB <$> maps) aggregateBBin aggregateBSymbols aggregateBPlacements
  aggregates <- case (aggregateA, aggregateB) of
    (Nothing, Nothing) -> pure Nothing
    (Just a, Just b) -> pure (Just (a, b))
    _ -> fail "aggregate mode requires both domains"
  unless (cohort /= ContextTransfer || case aggregates of Just _ -> True; Nothing -> False)
    (fail "context-transfer requires the actual aggregate binary, symbols, placements and carriers for both domains")
  forM_ [aggregate | cohort == ContextTransfer, Just (a, b) <- [aggregates], aggregate <- [a, b]] $ \(Aggregate _ symbols placements carriers) -> do
    forM_ ["xm1_rtos_scheduler_save", "xm1_rtos_scheduler_resume", "xm1_rtos_scheduler_select", "xm1_rtos_timer_tick_body",
           "s3d_timed_sleep_body", "s3d_delay_body"] $ \name ->
      unless (any (\symbol -> Agg.symbolName symbol == name && Agg.symbolSize symbol > 0) symbols)
        (fail ("context-transfer requires owned implementation: " ++ name))
    let foreignIntervals = [(stubBase, stubBase + 0x100), (exitAddress, exitAddress + 0x10),
                            (stackBase, stackBase + 0x1000), (workerBase, workerBase + 0x1000)]
        allocated = [(Agg.placementAddress p, Agg.placementAddress p + Agg.placementBytes p) | p <- placements]
          ++ [(Agg.symbolAddress s, Agg.symbolAddress s + Agg.symbolSize s) | s <- symbols]
          ++ [(low, high) | Regions.Extent (Regions.GuestAddress low) (Regions.GuestAddress high) <- carriers]
    unless (and [high <= low' || high' <= low | (low, high) <- foreignIntervals, (low', high') <- allocated])
      (fail "context-transfer instrumentation overlaps an owned carrier, symbol or allocation")
  forM_ [(DomainA, aggregateA), (DomainB, aggregateB)] $ \(domain, candidate) ->
    case candidate of
      Nothing -> pure ()
      Just (Aggregate _ symbols _ _) ->
        forM_ [Terminate, ChangePriority, TimedSleep, ReleaseWait, HandlerRelease, Delay, Timeout, DelayCallback] $ \service ->
          unless (any (\symbol -> Agg.symbolAddress symbol == entryOf domain service && Agg.symbolSize symbol > 0) symbols)
            (fail ("aggregate is missing public S3d entry: " ++ show (domain, service)))
  let timerContract = case aggregates of
        Just (a, b) | all detachedTimer [a, b] -> OwnedDetachedTimer
        _ -> RetainedTimer
      detachedTimer (Aggregate _ symbols _ _) =
        all (\name -> any ((== name) . Agg.symbolName) symbols)
          ["xm1_rtos_timer_remove_body", "xm1_rtos_timer_tick_body"]
  reports <- forM [(DomainA, ownedABytes), (DomainB, ownedBBytes)] $ \(d, owned) -> do
    let grid = if cohort == ContextTransfer then transferCases d else cases d
        directory = takeDirectory results </> ((if cohort == ContextTransfer then "context-transfer-" else "s3d-") ++ domainLabel d)
        domainAggregate = case aggregates of
          Just (a, b) -> if d == DomainA then a else b
          Nothing -> error "context-transfer aggregate required"
    createDirectoryIfMissing True directory
    observed <- forM [Stock, Owned] $ \role -> do
      let specs = [case (cohort, aggregates) of
            (ContextTransfer, Just pair) -> transferSpec d role one loaderABytes loaderBBytes pair
            _ -> specFor d role one loaderABytes loaderBBytes aggregates owned
            | one <- grid]
      batch <- writeBatch (directory </> show role) specs
      outcomes <- cachedObservedBatch cacheRoot runner batch
      readonlyControls <- if cohort == ContextTransfer then do
        readonlyBatch <- writeBatch (directory </> (show role ++ "-readonly")) (map readonlyTransfer specs)
        (_, readonlyOutcomes) <- cachedObservedBatch cacheRoot runner readonlyBatch
        pure [(caseName one, ControlRun (show role ++ "-readonly-scheduler-refused") (readonlyHolds observation))
             | (one, (_, observation)) <- zip grid readonlyOutcomes]
        else pure []
      pure (specs, snd outcomes, readonlyControls)
    case observed of
      [(specs, stock, stockReadonly), (_, ownedObs, ownedReadonly)] -> do
        let stockObs = map snd stock
            ownedObservations = map snd ownedObs
            compareOne one reference left right =
              let adjusted = case caseKernel reference of
                    Just kernel | null (kernelStop kernel) -> reference
                    _ -> reference { caseKernel = fmap (\k -> k { kernelUncompared = kernelUncompared k ++ [R4, R5, R6, R7, R8, R9, R10, R11, Sp] }) (caseKernel reference) }
               in if cohort == ContextTransfer
                  then let valid role observation = all controlHolds (transferControls d role one domainAggregate observation)
                        in if valid Stock left && valid Owned right
                           then compareKernelCase adjusted (transferView d left) (transferView d right)
                           else compareKernelCase adjusted left right
                  else compareKernelCase adjusted (normalize left) (normalize (detachmentView (timerDetachment timerContract d one) right))
            failures = [ (name, diff, firstDifference left right) | (name, diff, left, right) <- differences, not (null diff) ]
            differences = [ (caseName one, compareOne one reference left right, left, right)
                          | (one, reference, left, right) <- zip4 grid specs stockObs ownedObservations ]
            contracts =
              [ (caseName one, roleName, caseExpected one, nativeStatus observation, registerValue observation R0, registerValue observation Pc)
              | (one, roleName, observation) <-
                  [ (one, "stock" :: String, observation) | (one, observation) <- zip grid stockObs ]
                  ++ [ (one, "owned" :: String, observation) | (one, observation) <- zip grid ownedObservations ]
              , not (judgeOutcome (caseExpected one) (nativeStatus observation) (registerValue observation R0) (registerValue observation Pc))
              ]
            controls =
              [ (caseName one, control)
              | (one, spec, left, right) <- zip4 grid specs stockObs ownedObservations
              , control <- if cohort == ContextTransfer
                  then [control { controlName = show role ++ "-" ++ controlName control }
                       | (role, observation) <- [(Stock, left), (Owned, right)]
                       , control <- transferControls d role one domainAggregate observation]
                  else controlRuns timerContract d one spec left right
              ] ++ stockReadonly ++ ownedReadonly
            failedControls = [ (name, controlName control) | (name, control) <- controls, not (controlHolds control) ]
        B.writeFile (directory </> "report.json") (encodeCanonical (A.object
          [ "domain" .= domainLabel d
          , "cohort" .= show cohort
          , "cases" .= length grid
          , "differences" .= length failures
          , "failures" .= [A.object ["case" .= name, "differences" .= show diff, "first" .= first] | (name, diff, first) <- failures]
          , "contract_failures" .= [A.object ["case" .= name, "role" .= roleName, "expected" .= show expected, "status" .= show status, "r0" .= r0, "pc" .= pc] | (name, roleName, expected, status, r0, pc) <- contracts]
          , "controls" .= [A.object ["case" .= name, "control" .= controlName control, "holds" .= controlHolds control] | (name, control) <- controls]
          , "control_failures" .= [A.object ["case" .= name, "control" .= control] | (name, control) <- failedControls]
          , "judge_self_test" .= judgeSelfTest
          ]))
        pure (d, length grid, null failures, null contracts && null failedControls && judgeSelfTest)
      _ -> error "role count"
  let passed = all (\(_,_,a,b) -> a && b) reports
  B.writeFile results (encodeCanonical (A.object
    [ "passed" .= passed
    , "domains" .= [A.object ["domain" .= domainLabel d, "cases" .= n, "comparisons" .= a, "contracts" .= b] | (d,n,a,b) <- reports]
    ]))
  forM_ reports print
  unless passed exitFailure

zip4 :: [a] -> [b] -> [c] -> [d] -> [(a,b,c,d)]
zip4 (a:as) (b:bs) (c:cs) (d:ds) = (a,b,c,d) : zip4 as bs cs ds
zip4 _ _ _ _ = []

argument :: String -> IO FilePath
argument flag = do
  arguments <- getArgs
  case reverse (argumentValues flag arguments) of
    [] -> fail (flag ++ " is required and needs a value")
    value : _ -> pure value

argumentValues :: String -> [String] -> [String]
argumentValues _ [] = []
argumentValues _ [_] = []
argumentValues flag (name : value : rest)
  | name == flag = value : argumentValues flag rest
  | otherwise = argumentValues flag (value : rest)

standaloneBytes :: Maybe FilePath -> Maybe FilePath -> IO B.ByteString
standaloneBytes (Just _) _ = pure B.empty
standaloneBytes Nothing (Just path) = B.readFile path
standaloneBytes Nothing Nothing = fail "standalone mode requires --owned-a-bin and --owned-b-bin"
