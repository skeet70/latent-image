{-# LANGUAGE OverloadedStrings #-}

-- | Emitted A32 qualification of the six S3d lifecycle services against the pinned loaders.
module Main (main) where

import Control.Category ((>>>))
import Control.Monad (forM, forM_, unless)
import Data.Aeson ((.=))
import Data.Bits ((.&.), (.|.), complement, shiftL, shiftR)
import qualified Data.ByteString as B
import Data.List (find)
import qualified Data.Map.Strict as M
import Data.Word (Word32)
import Emulation.CachedNativeBatch (cachedObservedBatch)
import Emulation.NativeObservation
import Emulation.OwnedCase
import Emulation.Sha256 (sha256File, sha256Hex)
import FujiAF.JSON (encodeCanonical)
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

data Service = Terminate | ChangePriority | TimedSleep | ReleaseWait | HandlerRelease | Delay
  deriving (Eq, Show)

data Case = Case
  { caseName :: String
  , caseService :: Service
  , caseArgument :: Word32
  , caseArgument1 :: Word32
  , caseSetup :: Domain -> W -> W
  , caseStop :: Bool
  , caseCpsr :: Word32
  , caseHandler :: Bool
  , caseLifecycle :: Maybe Word32
  , caseExpect :: Word32
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

extentOf :: Service -> Word32
extentOf service = case service of
  Terminate -> 0x33c
  ChangePriority -> 0x314
  TimedSleep -> 0x1b0
  ReleaseWait -> 0x280
  HandlerRelease -> 0x24c
  Delay -> 0x184

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
  _delayCallback :: Domain -> Word32
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
_delayCallback d = if d == DomainA then 0x44634 else 0xd45f4

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
   in setW (record + 0x00) (domainId target)
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
      timer = 0x40059200
      heap = timerHeap d
   in setW timer 1
      >>> setW (timer + 4) (timeoutCallback d)
      >>> setW (timer + 8) task
      >>> setW (timerCount d) 1
      >>> setW heap 0
      >>> setW (heap + 4) 0
      >>> setW (heap + 8) timer
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
  [ Case "terminate-ready" Terminate (idOf d 2) 0 (\e -> setupRunning e >>> makeReady d 2 3 False) False 0x53 False Nothing 0
  , Case "terminate-dormant" Terminate (idOf d 2) 0 setupRunning False 0x53 False Nothing 0xffffffd7
  , Case "terminate-invalid-class" Terminate (3 `shiftL` 16) 0 setupRunning False 0x53 False Nothing 0xffffffee
  , Case "terminate-masked" Terminate (idOf d 2) 0 (\e -> setupRunning e >>> makeReady d 2 3 False) False 0xd3 False Nothing 0xffffffe7
  , Case "priority-ready" ChangePriority (idOf d 2) 2 (\e -> setupRunning e >>> makeReady d 2 5 False) False 0x53 False Nothing 0
  , Case "priority-invalid" ChangePriority (idOf d 2) 17 setupRunning False 0x53 False Nothing 0xffffffef
  , Case "timed-sleep-wakeup" TimedSleep 5 0 (\e -> setupRunning e >>> setB (taskField (taskAt d 1) 0x0e) 2) False 0x53 False Nothing 0
  , Case "timed-sleep-zero" TimedSleep 0 0 setupRunning False 0x53 False Nothing 0xffffffce
  , Case "timed-sleep-block" TimedSleep 50 0 setupRunning True 0x53 False Nothing 0
  , Case "release-sleeping" ReleaseWait (idOf d 2) 0 (\e -> setupRunning e >>> setWait d 2 0x40059100 0x40059000 (domainId d) >>> armTimer d 2) False 0x53 False Nothing 0
  , Case "release-ready" ReleaseWait (idOf d 2) 0 (\e -> setupRunning e >>> makeReady d 2 3 False) False 0x53 False Nothing 0xffffffd7
  , Case "handler-release" HandlerRelease (idOf d 2) 0 (\e -> setupRunning e >>> setWait d 2 0x40059100 0x40059000 (domainId d) >>> armTimer d 2 >>> setW (handlerCell d) 1) False 0x53 True Nothing 0
  , Case "delay-block" Delay 30 0 setupRunning True 0x53 False Nothing 0
  , Case "delay-negative" Delay 0xffffffff 0 setupRunning False 0x53 False Nothing 0xffffffef
  , Case "timed-sleep-return" TimedSleep 50 0 setupRunning False 0x53 False (Just 50) 0xffffffce
  , Case "delay-return" Delay 30 0 setupRunning False 0x53 False (Just 0) 0
  , Case "release-cross-domain" Terminate (idOf DomainB 2) 0 (\e -> setupRunning e >>> makeReady DomainB 2 3 True >>> setW (dispatchCell DomainB) 1) False 0x53 False Nothing 0
  ]

-- | A four-byte branch to the stub, and the stub's save/wake/resume body.
stubWords :: Domain -> Word32 -> [Word32]
stubWords d result = prefix ++ [0xe8bd4ff0, 0xe12fff1e]
  where
    prefix =
      [ 0xe92d4ff0
      , movw 1 (runningCell d)
      , movt 1 (runningCell d)
      , 0xe5911000
      , 0xe5811018
      , movw 2 resume
      , movt 2 resume
      , 0xe581201c
      , 0xe5912014
      , 0xe3a03000
      ] ++ (if result == 0 then [] else [0xe2433000 .|. (result .&. 0xff)]) ++
      [ 0xe5823000
      , 0xe5d1300c
      , 0xe3833001
      , 0xe5c1300c
      , 0xe591d018
      , 0xe591f01c
      ]
    resume = stubBase + fromIntegral (length prefix) * 4

movw, movt :: Word32 -> Word32 -> Word32
movw register immediate = 0xe3000000 .|. (register `shiftL` 12) .|. ((immediate .&. 0xf000) `shiftL` 4) .|. (immediate .&. 0xfff)
movt register immediate = 0xe3400000 .|. (register `shiftL` 12) .|. ((immediate .&. 0xf000) `shiftL` 4) .|. (immediate .&. 0xfff)

poke :: B.ByteString -> Word32 -> B.ByteString -> B.ByteString
poke bytes offset value = B.take index bytes <> value <> B.drop (index + B.length value) bytes
  where index = fromIntegral offset

slice :: B.ByteString -> Word32 -> Word32 -> Word32 -> B.ByteString
slice owned ownedBase address size = B.take (fromIntegral size) (B.drop (fromIntegral (address - ownedBase)) owned)

patchOwned :: Domain -> B.ByteString -> Word32 -> B.ByteString -> B.ByteString
patchOwned d loader ownedBase owned =
  let base = loaderBase d
      patches =
        [(entryOf d service, 4, entryOf d service) | service <- [Terminate, ChangePriority, TimedSleep, ReleaseWait, HandlerRelease, Delay]]
        ++ [(0x46e4c + if d == DomainB then 0x8ffc0 else 0, 0x180, 0x46e4c + if d == DomainB then 0x8ffc0 else 0)]
        ++ [(if d == DomainA then 0x50000 else 0xe0000, 0x1300, if d == DomainA then 0x50000 else 0xe0000)]
   in foldl (\bytes (address, size, source) -> poke bytes (address - base) (slice owned ownedBase source size)) (loader <> B.replicate 0x12000 0) patches

loaderBytes :: Domain -> B.ByteString -> B.ByteString
loaderBytes _ loader = loader

dataCode :: Domain -> W -> B.ByteString
dataCode d m = serialize (globalsBase d) globalsSize m

serialize :: Word32 -> Word32 -> W -> B.ByteString
serialize base size m = foldl write (B.replicate (fromIntegral size) 0) (M.toList m)
  where
    write bytes (address, value)
      | address >= base && address + 4 <= base + size = poke bytes (address - base) (word32le value)
      | otherwise = bytes

specFor :: Domain -> Role -> Case -> B.ByteString -> B.ByteString -> CaseSpec
specFor d role one loader owned = CaseSpec
  { caseId = caseName one ++ "-" ++ domainLabel d
  , caseProvenance = "X-M1 1.50 S3d lifecycle services, x70view 55436fc85c0560dd252b0410af6f2427fe198cd3; synthetic scheduler state"
  , caseExclusion = "One service call. Blocking cases stop at the dispatcher entry; lifecycle cases replace the dispatcher with a save/wake/resume stub so the original caller returns."
  , caseEntry = GuestAddress (fromIntegral (entryOf d (caseService one)))
  , caseExit = GuestAddress (fromIntegral exitAddress)
  , caseRegions =
      [ mkRegion (RegionId "code") (GuestAddress (fromIntegral (loaderBase d))) 0x10000 ReadExecute (B.take 0x10000 code)
      , mkRegion (RegionId "bodies") (GuestAddress (fromIntegral bodyBase)) 0x2000 ReadExecute bodies
      , mkRegion (RegionId "stub") (GuestAddress (fromIntegral stubBase)) 0x100 ReadExecute stub
      , mkRegion (RegionId "data-a") (GuestAddress (fromIntegral (globalsBase DomainA))) (fromIntegral globalsSize) ReadWrite (dataCode DomainA fixture)
      , mkRegion (RegionId "data-b") (GuestAddress (fromIntegral (globalsBase DomainB))) (fromIntegral globalsSize) ReadWrite (dataCode DomainB fixture)
      , mkRegion (RegionId "stack") (GuestAddress (fromIntegral stackBase)) 0x1000 ReadWrite (B.replicate 0x1000 0)
      , mkRegion (RegionId "exit") (GuestAddress (fromIntegral exitAddress)) 0x10 ReadExecute (word32le 0xe7f000f0)
      , mkRegion (RegionId "bank-a") (GuestAddress 0xfffb30cc) 4 Device (word32le 0)
      , mkRegion (RegionId "bank-b") (GuestAddress 0xfffb40cc) 4 Device (word32le 0)
      ]
  , caseFunctions =
      [(GuestAddress (fromIntegral (entryOf d service)), extentOf service) | service <- [Terminate, ChangePriority, TimedSleep, ReleaseWait, HandlerRelease, Delay]]
      ++ [(GuestAddress (fromIntegral bodyBase), 0x1300), (GuestAddress (fromIntegral stubBase), 0x100)]
      ++ [(GuestAddress (fromIntegral (acquireFn d)), 0x3c), (GuestAddress (fromIntegral (releaseFn d)), 0x40)]
      ++ [(GuestAddress (fromIntegral (base + (if d == DomainB then 0x8ffc0 else 0))), size)
         | (base, size) <- ([(0x43ca4, 0x7c), (0x43d20, 0xb0), (0x43dd0, 0x1a0), (0x43f70, 0xa8), (0x44018, 0x2c), (0x44318, 0x1cc), (0x444e4, 0x84), (0x44568, 0xcc), (0x44634, 0xc8), (0x446fc, 0x48), (0x44744, 0x90), (0x447d4, 0x1ec), (0x449c0, 0x100), (0x44ac0, 0xc4), (0x44b84, 0xdc), (0x44c60, 0x48), (0x44ca8, 0xe4)] :: [(Word32, Word32)])]
  , caseInitial =
      [(R0, caseArgument one), (R1, caseArgument1 one), (R2, 0), (R3, 0), (R12, 0)]
      ++ [(register, 0xabc00000 + fromIntegral (fromEnum register)) | register <- [R4 .. R11]]
      ++ [(Sp, stackTop), (Lr, exitAddress), (Cpsr, caseCpsr one)]
  , caseExpectedStatus = Returned
  , caseExpectedRegisters = [(R0, caseExpect one)]
  , caseExpectedMemory = []
  , caseCp14Identity = Just (if d == DomainA then 0x4003 else 0x4004)
  , caseKernel = Just (KernelCase (InstructionLimit 4000) stop (Just 0x80001234) (Just 0) (Just 0) uncompared [])
  }
  where
    fixture = setW (handlerCell d) (if caseHandler one then 1 else 0) (caseSetup one d (baseFixture d))
    bodyBase = if d == DomainA then 0x50000 else 0xe0000
    code = case role of
      Stock -> patchStub d (loaderBytes d loader) Nothing
      Owned -> patchStub d (patchOwned d loader (if d == DomainA then 0x454f0 else 0xd54b0) owned) (caseLifecycle one)
    bodies = if role == Owned then slice owned (if d == DomainA then 0x454f0 else 0xd54b0) bodyBase 0x1300 else B.replicate 0x2000 0
    stub = case caseLifecycle one of
      Nothing -> B.replicate 0x100 0
      Just result -> B.concat (map word32le (stubWords d result)) <> B.replicate 0x40 0
    stop = if caseStop one then Just (GuestAddress (fromIntegral (switchOf d))) else Nothing
    uncompared
      | caseStop one = [R1, R2, R3, R12, Lr, Pc, Cpsr]
      | otherwise = [R1, R2, R3, R12, Lr, Pc]

patchStub :: Domain -> B.ByteString -> Maybe Word32 -> B.ByteString
patchStub d loader lifecycle = case lifecycle of
  Nothing -> loader
  Just _ -> poke loader (switchOf d - loaderBase d) (word32le (branch (switchOf d) stubBase))

branch :: Word32 -> Word32 -> Word32
branch from to = 0xea000000 .|. (((to - from - 8) `shiftR` 2) .&. 0xffffff)

firstDifference :: NativeObservation -> NativeObservation -> String
firstDifference stock owned =
  case [ (ident, index, x, y) | ident <- [RegionId "data-a", RegionId "data-b"], Just (index, x, y) <- [firstByte ident] ] of
    ((ident, index, x, y) : _) -> show (regionName ident, index, x, y)
    [] -> "no data difference"
  where
    firstByte ident = do
      a <- lookup ident (nativeMemory stock)
      b <- lookup ident (nativeMemory owned)
      (index, (x, y)) <- find (\(_, (x, y)) -> x /= y) (zip [0 :: Int ..] (zip (B.unpack a) (B.unpack b)))
      pure (index, x, y)

normalize :: NativeObservation -> NativeObservation
normalize observation = observation
  { nativeMemory = filter ((`notElem` [RegionId "stack", RegionId "stub"]) . fst) (nativeMemory observation)
  , nativeKernel = fmap clean (nativeKernel observation)
  }
  where
    clean kernel = kernel { observedOrder = fmap (filter keep) (observedOrder kernel) }
    keep (OrderedWrite (Write (GuestAddress address) _) _) = not (address >= fromIntegral stackBase && address < fromIntegral (stackBase + 0x1000))
    keep _ = True

main :: IO ()
main = do
  runner <- argument "--runner" >>= makeAbsolute
  loaderA <- argument "--loader-a"
  loaderB <- argument "--loader-b"
  ownedA <- argument "--owned-a-bin"
  ownedB <- argument "--owned-b-bin"
  cacheRoot <- argument "--cache-root"
  results <- argument "--results"
  loaderABytes <- B.readFile loaderA
  loaderBBytes <- B.readFile loaderB
  ownedABytes <- B.readFile ownedA
  ownedBBytes <- B.readFile ownedB
  digestA <- sha256File loaderA
  digestB <- sha256File loaderB
  unless (sha256Hex digestA == "21366ffff7cebc6176b8a8125bf3a9974835a51070a58ea325f7eed8aabd8882") (fail "loader A is not the pinned image")
  unless (sha256Hex digestB == "8fb0c322a22443a3783cdab9071db17212a5629f6375646218ac1e043dc672ec") (fail "loader B is not the pinned image")
  createDirectoryIfMissing True cacheRoot
  createDirectoryIfMissing True (takeDirectory results)
  reports <- forM [(DomainA, loaderABytes, ownedABytes), (DomainB, loaderBBytes, ownedBBytes)] $ \(d, loader, owned) -> do
    let grid = cases d
        directory = takeDirectory results </> ("s3d-" ++ domainLabel d)
    createDirectoryIfMissing True directory
    observed <- forM [Stock, Owned] $ \role -> do
      let specs = [specFor d role one loader owned | one <- grid]
      batch <- writeBatch (directory </> show role) specs
      outcomes <- cachedObservedBatch cacheRoot runner batch
      pure (specs, snd outcomes)
    case observed of
      [(specs, stock), (_, ownedObs)] -> do
        let stockObs = map snd stock
            ownedObservations = map snd ownedObs
            compareOne reference left right =
              let adjusted = case caseOutcome reference left of
                    OutcomeReturned -> reference
                    _ -> reference { caseKernel = fmap (\k -> k { kernelUncompared = kernelUncompared k ++ [R0, R4, R5, R6, R7, R8, R9, R10, R11, Sp] }) (caseKernel reference) }
               in compareKernelCase adjusted (normalize left) (normalize right)
            failures = [ (name, diff, firstDifference left right) | (name, diff, left, right) <- differences, not (null diff) ]
            differences = [ (caseName one, compareOne reference left right, left, right)
                          | (one, reference, left, right) <- zip4 grid specs stockObs ownedObservations ]
            contracts =
              [ (caseName one, nativeStatus observation, registerValue observation R0, caseExpect one)
              | (one, observation) <- zip grid stockObs
              , caseExpect one /= 0
              , registerValue observation R0 /= caseExpect one || nativeStatus observation /= Returned
              ]
        B.writeFile (directory </> "report.json") (encodeCanonical (A.object
          [ "domain" .= domainLabel d
          , "cases" .= length grid
          , "differences" .= length failures
          , "failures" .= [A.object ["case" .= name, "differences" .= show diff, "first" .= first] | (name, diff, first) <- failures]
          , "contract_failures" .= [A.object ["case" .= name, "status" .= show status, "r0" .= r0, "expected" .= expected] | (name, status, r0, expected) <- contracts]
          ]))
        pure (d, length grid, null failures, null contracts)
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
  case dropWhile (/= flag) arguments of
    [] -> fail (flag ++ " is required")
    [_] -> fail (flag ++ " needs a value")
    _ : value : _ -> pure value
