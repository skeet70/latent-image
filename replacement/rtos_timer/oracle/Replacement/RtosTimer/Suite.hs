{-# LANGUAGE OverloadedStrings #-}

-- | Timer heap, membership and callback cases over the stock loaders and the owned A32 entries.
module Replacement.RtosTimer.Suite
  ( Image (..)
  , CaseRun (..)
  , ControlRun (..)
  , OwnedSymbol (..)
  , runDomain
  , parseOwnedSymbols
  , fixtureCodeExtents
  , parseS3dSymbols
  , parseAggregateSymbols
  ) where

import Control.Monad (forM, forM_, unless)
import Data.Bits ((.&.), (.|.), complement, shiftL, shiftR, xor)
import qualified Data.ByteString as B
import Data.List (isSubsequenceOf)
import qualified Data.Map.Strict as M
import Data.Maybe (fromMaybe)
import Data.Word (Word32, Word8)
import Emulation.CachedNativeBatch (BatchReuse, cachedObservedBatch)
import Emulation.NativeObservation
import Emulation.OwnedCase
import Replacement.RtosOwnFirst.Regions (Domain (..), domainLabel)
import Replacement.RtosTimer.Layout
import Replacement.RtosTimer.Aggregate (Aggregate, aggregateFunctions, aggregateRegions, aggregateControlRegions, admissionControls)
import System.FilePath ((</>))

data Image = Image { imageBase :: Word32, imageBytes :: B.ByteString }

data Role = Stock | Owned deriving (Eq, Show)

data Call = Initialize | Insert | Remove | Tick deriving (Eq, Show)

data Callback = Receipt | Cancel | Rearm | Switch | SwitchDistinct | WaitRelease deriving (Eq, Show)

data Kind = Equal | Repair String | Fixture String deriving (Eq, Show)

data Check
  = ReturnedSuccessfully
  | KernelWord Word32 Word32
  | EntryWord Int Word32 Word32
  | CountIs Word32
  | BaseIs Word32
  | TickIs Word32
  | RegisterIs Register Word32
  | CallbacksAre [Word32]
  | CallbackLocksAre [Word32]
  | EntryIndexIs Int Word32
  | CpsrControlIs Word32
  | WordIs String Word32 Word32
  | WordIsRole String Word32 Word32 Word32
  | ByteIs String Word32 Word32
  | WroteSequence Word32 [Word32]
  | WroteSequenceOwned Word32 [Word32]
  | RegionsUnchanged [String]
  | ConfigurationPublished
  | ArenaOwned
  deriving (Eq, Show)

data Case = Case
  { caseName :: String
  , caseCall :: Call
  , caseHeap :: [(Word32, Int)]
  , caseCount :: Word32
  , caseSlot :: Int
  , caseExpiry :: Word32
  , caseBase :: Word32
  , caseTick :: Word32
  , caseHandler :: Word32
  , caseCpsr :: Word32
  , caseCallback :: Callback
  , caseKind :: Kind
  , caseEqualChecks :: [Check]
  , caseStockChecks :: [Check]
  , caseOwnedChecks :: [Check]
  , caseSlotCallbacks :: [(Int, Word32)]
  , caseIndices :: [(Int, Word32)]
  }

data CaseRun = CaseRun
  { caseDomain :: Domain
  , runCohort :: String
  , runName :: String
  , runKind :: Kind
  , runChecks :: [(String, Bool)]
  , runDifferences :: [String]
  , runReuse :: (BatchReuse, BatchReuse)
  }

data ControlRun = ControlRun
  { controlCohort :: String
  , controlName :: String
  , controlCase :: String
  , controlHolds :: Bool
  }

stubBase, stubAcquire, stubRelease, stubReceipt, stubCancel, stubRearm, stubSwitch, stubWaitRelease, stubSize :: Word32
stubBase = 0x01000000
stubAcquire = stubBase
stubRelease = stubBase + 0x18
stubReceipt = stubBase + 0x30
stubCancel = stubBase + 0x80
stubRearm = stubBase + 0xa0
stubSwitch = stubBase + 0xc8
stubWaitRelease = stubBase + 0xe0
stubSize = 0x100

waitReleaseAddress :: Domain -> Word32
waitReleaseAddress DomainA = 0x44568
waitReleaseAddress DomainB = 0xd4528

waitReleaseSize :: Word32
waitReleaseSize = 0xcc

fixtureCodeExtents :: [(Word32, Word32)]
fixtureCodeExtents =
  [(stubBase, stubSize), (exitAddress, 0x10)] ++
  [(waitReleaseAddress domain, waitReleaseSize) | domain <- [DomainA, DomainB]]

domainId :: Domain -> Word32
domainId DomainA = 1
domainId DomainB = 2

secondLockCell, taskAddress, taskWait, taskQueue :: Word32
secondLockCell = entriesBase + 0x700
taskAddress = entriesBase + 0x400
taskWait = entriesBase + 0x440
taskQueue = entriesBase + 0x460

exitAddress, stackBase, stackSize, stackPointer, entriesBase, entriesSize :: Word32
exitAddress = 0x01000200
stackBase = 0x300000
stackSize = 0x1000
stackPointer = stackBase + 0x800
entriesBase = 0x40100000
entriesSize = 0x1000

contextBase, receiptBase, counterBase, entryTableBase, entryStride, secondContextBase, secondHeapBase :: Word32
contextBase = entriesBase + 0x800
receiptBase = entriesBase + 0x200
counterBase = entriesBase + 0x300
entryTableBase = entriesBase + 0x100
entryStride = 0x10
secondContextBase = entriesBase + 0x900
secondHeapBase = entriesBase + 0xa00

entryAddress :: Int -> Word32
entryAddress slot = entryTableBase + fromIntegral slot * entryStride

insertEntry, removeEntry, tickEntry :: Domain -> Word32
insertEntry DomainA = 0x44c60
insertEntry DomainB = 0xd4c20
removeEntry DomainA = 0x44ca8
removeEntry DomainB = 0xd4c68
tickEntry DomainA = 0x44d8c
tickEntry DomainB = 0xd4d4c

stockBase :: Domain -> (Word32, Word32)
stockBase DomainA = (0x44ac0, 0x46c)
stockBase DomainB = (0xd4a80, 0x46c)

initializeAddress, configurationAddress, configurationWordAddress :: Domain -> GuestAddress
initializeAddress DomainA = GuestAddress 0x44a50
initializeAddress DomainB = GuestAddress 0xd4a10
configurationAddress DomainA = GuestAddress 0x400c0818
configurationAddress DomainB = GuestAddress 0x400c0888
configurationWordAddress DomainA = GuestAddress 0x400546d4
configurationWordAddress DomainB = GuestAddress 0x400e4694

wordAddress :: GuestAddress -> Word32
wordAddress (GuestAddress address) = fromIntegral address

configurationRegion :: RegionId
configurationRegion = RegionId "configuration"

seedRegister :: Register -> Word32
seedRegister register = 0xa5a50000 + fromIntegral (fromEnum register)

-- Instruction encoders for the lock, receipt and callback stubs.
movwI, movtI, ldrI, strI, movReg, movImm, blWord :: Word32 -> Word32 -> Word32
addRegLsl2, addImm :: Word32 -> Word32 -> Word32 -> Word32
movwI rd imm = 0xe3000000 .|. (((imm `shiftR` 12) .&. 0xf) `shiftL` 16) .|. (rd `shiftL` 12) .|. (imm .&. 0xfff)
movtI rd imm = 0xe3400000 .|. (((imm `shiftR` 12) .&. 0xf) `shiftL` 16) .|. (rd `shiftL` 12) .|. (imm .&. 0xfff)
ldrI rd rn = 0xe5900000 .|. (rn `shiftL` 16) .|. (rd `shiftL` 12)
strI rd rn = 0xe5800000 .|. (rn `shiftL` 16) .|. (rd `shiftL` 12)
addRegLsl2 rd rn rm = 0xe0800000 .|. (rn `shiftL` 16) .|. (rd `shiftL` 12) .|. (2 `shiftL` 7) .|. rm
addImm rd rn imm = 0xe2800000 .|. (rn `shiftL` 16) .|. (rd `shiftL` 12) .|. (imm .&. 0xfff)
movReg rd rm = 0xe1a00000 .|. (rd `shiftL` 12) .|. rm
movImm rd imm = 0xe3a00000 .|. (rd `shiftL` 12) .|. (imm .&. 0xfff)
blWord here target =
  let delta = fromIntegral target - fromIntegral here - 8 :: Int
   in 0xeb000000 .|. (fromIntegral ((delta `div` 4) .&. 0xffffff) :: Word32)

pushLr, popPc, bxLr :: Word32
pushLr = 0xe52de004
popPc = 0xe8bd8000
bxLr = 0xe12fff1e

fillStubs :: [(Int, [Word32])] -> B.ByteString
fillStubs segments = B.concat (map word32le (go 0))
  where
    total = fromIntegral stubSize
    go offset = case lookup offset segments of
      Just words' -> words' ++ go (offset + 4 * length words')
      Nothing
        | offset < total -> 0xe7f000f0 : go (offset + 4)
        | otherwise -> []

-- | The receipt prologue: record the argument and the lock word, then increment the counter.
receiptBody :: Word32 -> [Word32]
receiptBody lock =
  [ pushLr
  , movwI 1 (counterBase .&. 0xffff), movtI 1 (counterBase `shiftR` 16)
  , ldrI 2 1
  , addImm 3 2 1, strI 3 1
  , movwI 1 (receiptBase .&. 0xffff), movtI 1 (receiptBase `shiftR` 16)
  , addRegLsl2 1 1 2, strI 0 1
  , movwI 3 (lock .&. 0xffff), movtI 3 (lock `shiftR` 16), ldrI 3 3
  , movwI 1 ((receiptBase + 0x80) .&. 0xffff), movtI 1 ((receiptBase + 0x80) `shiftR` 16)
  , addRegLsl2 1 1 2, strI 3 1
  ]

-- | Record the callback, then point the current-context word at the second context.
switchCode :: Facts -> Domain -> [Word32]
switchCode facts domain =
  [ pushLr
  , blWord (stubSwitch + 4) stubReceipt
  , movwI 1 (secondContextBase .&. 0xffff), movtI 1 (secondContextBase `shiftR` 16)
  , movwI 3 (currentContext .&. 0xffff), movtI 3 (currentContext `shiftR` 16)
  , strI 1 3
  , popPc
  ]
  where
    currentContext = dfCurrentContext (domainFacts facts domain)

stubBytes :: Facts -> Domain -> Case -> B.ByteString
stubBytes facts domain one = fillStubs $
  [ (0, [ldrI 1 0, movImm 2 1, strI 2 1, movImm 0 0, bxLr])
  , (0x18, [ldrI 1 0, movImm 2 0, strI 2 1, movImm 0 0, bxLr])
  , (0x30, receiptBody lock ++ [popPc])
  , (0x80, [pushLr, movReg 3 0, movwI 0 (currentContext .&. 0xffff), movtI 0 (currentContext `shiftR` 16), ldrI 0 0, movReg 1 3, blWord (stubCancel + 4 * 6) (removeEntry domain), popPc])
  , (0xa0, [pushLr, movReg 3 0, movwI 0 (currentContext .&. 0xffff), movtI 0 (currentContext `shiftR` 16), ldrI 0 0, movReg 1 3, movwI 2 (rearmExpiry .&. 0xffff), movtI 2 (rearmExpiry `shiftR` 16), blWord (stubRearm + 4 * 8) (insertEntry domain), popPc])
  ]
    ++ [(0xc8, switchCode facts domain) | caseCallback one `elem` [Switch, SwitchDistinct]]
    ++ [(0xe0, waitReleaseCode)]
  where
    currentContext = dfCurrentContext (domainFacts facts domain)
    lock = dfSchedulerLock (domainFacts facts domain)
    waitReleaseCode =
      [ pushLr
      , movwI 0 (contextBase .&. 0xffff), movtI 0 (contextBase `shiftR` 16)
      , movwI 1 (taskAddress .&. 0xffff), movtI 1 (taskAddress `shiftR` 16)
      , blWord (stubWaitRelease + 4 * 5) (waitReleaseAddress domain)
      , popPc
      ]

rearmExpiry :: Word32
rearmExpiry = 40

callbackAddress :: Case -> Word32
callbackAddress one = case caseCallback one of
  Receipt -> stubReceipt
  Cancel -> stubCancel
  Rearm -> stubRearm
  Switch -> stubSwitch
  SwitchDistinct -> stubSwitch
  WaitRelease -> stubWaitRelease

kernelLow :: Facts -> Domain -> Word32
kernelLow facts domain = dfHandlerContext (domainFacts facts domain)

-- | Both roles declare the same kernel extent; the owned role places its nodes in the arena
-- region, so its stock-heap subrange stays zero and is masked when the roles are compared.
kernelHigh :: Facts -> Domain -> Role -> Word32
kernelHigh facts domain _role = dfTimerHeap (domainFacts facts domain) + 0x5c0 + 0x20

kernelSize :: Facts -> Domain -> Role -> Word32
kernelSize facts domain role = kernelHigh facts domain role - kernelLow facts domain

arenaSpan :: Facts -> Domain -> (Word32, Word32)
arenaSpan facts domain = (spanStart (arenaGuest (arenaFor facts domain)), spanEnd (arenaGuest (arenaFor facts domain)))

arenaSize :: Facts -> Domain -> Word32
arenaSize facts domain = let (low, high) = arenaSpan facts domain in high - low

heapBase :: Facts -> Domain -> Role -> Word32
heapBase facts domain Stock = dfTimerHeap (domainFacts facts domain)
heapBase facts domain Owned = spanStart (arenaGuest (arenaFor facts domain))

heapNodeRole :: Facts -> Domain -> Role -> Int -> Word32
heapNodeRole facts domain role index = heapBase facts domain role + fromIntegral (index - 1) * 8

-- | Case checks are written against the stock heap; the owned role shifts only node addresses.
heapNode :: Facts -> Domain -> Int -> Word32
heapNode facts domain = heapNodeRole facts domain Stock

retargetCheck :: Facts -> Domain -> Role -> Check -> Check
retargetCheck facts domain role check' = case check' of
  KernelWord address value -> KernelWord (retarget address) value
  _ -> check'
  where
    stock = dfTimerHeap (domainFacts facts domain)
    delta = heapBase facts domain role - stock
    retarget address
      | address >= stock && address < stock + 0x5c0 + 8 = address + delta
      | otherwise = address

entryIndexIn :: Case -> Int -> Word32
entryIndexIn one slot = case lookup slot (caseIndices one) of
  Just index -> index
  Nothing -> case [fromIntegral index | (index, (_, heapSlot)) <- zip [1 :: Int ..] (caseHeap one), heapSlot == slot] of
    (index : _) -> index
    [] -> 0

kernelWords :: Facts -> Domain -> Role -> Case -> [(Word32, Word32)]
kernelWords facts domain role one =
  [ (dfHandlerContext d, caseHandler one)
  , (dfCurrentContext d, contextBase)
  , (dfSchedulerLock d, 0)
  , (dfTimerCount d, caseCount one)
  , (dfTimerBase d, caseBase one)
  , (dfTickNow d, caseTick one)
  ]
  ++ [ (wordAddress (configurationWordAddress domain), wordAddress (configurationAddress domain))
     | caseCall one == Initialize
     ]
  ++ [(dfTickNow d + 4, 0xa5a5a5a5) | caseCall one == Initialize]
  ++ concat
    [ [(heapNodeRole facts domain role index, expiry), (heapNodeRole facts domain role index + 4, entryAddress slot)]
    | (index, (expiry, slot)) <- zip [1 :: Int ..] (caseHeap one)
    ]
  where
    d = domainFacts facts domain

entriesWords :: Facts -> Domain -> Role -> Case -> [(Word32, Word32)]
entriesWords facts domain role one =
  contextWords
    ++ concat
      [ [ (entryAddress slot, entryIndexIn one slot)
        , (entryAddress slot + 4, entryCallback slot)
        , (entryAddress slot + 8, entryAddress slot)
        ]
      | slot <- [1 .. 8]
      ]
    ++ [(counterBase, 0)]
    ++ [(receiptBase + fromIntegral index * 4, 0) | index <- [0 .. 64 :: Int]]
    ++ [(receiptBase + 0x80 + fromIntegral index * 4, 0) | index <- [0 .. 64 :: Int]]
    ++ secondContextWords
    ++ secondMembership
    ++ taskWords
  where
    d = domainFacts facts domain
    entryCallback slot = fromMaybe fallback (lookup slot (caseSlotCallbacks one))
      where
        fallback = case caseCallback one of
          Switch -> if slot == 1 then stubSwitch else stubReceipt
          SwitchDistinct -> if slot == 1 then stubSwitch else stubReceipt
          _ -> callbackAddress one
    contextWords =
      [ (contextBase + contextSchedulerLock c, dfSchedulerLock d)
      , (contextBase + contextSchedulerLock c + lockWord (factsLock facts), dfSchedulerLock d)
      , (contextBase + contextSchedulerLock c + lockAcquire (factsLock facts), stubAcquire)
      , (contextBase + contextSchedulerLock c + lockRelease (factsLock facts), stubRelease)
      , (contextBase + contextTimerHeap c, heapBase facts domain role)
      , (contextBase + contextTimerCount c, dfTimerCount d)
      , (contextBase + contextTimerBase c, dfTimerBase d)
      , (contextBase + contextTickNow c, dfTickNow d)
      ]
    secondContextWords = case caseCallback one of
      Switch -> switchWords (dfSchedulerLock d)
      SwitchDistinct -> switchWords secondLockCell
      _ -> []
    secondMembership = case caseCallback one of
      SwitchDistinct -> [(entryAddress 2, 1), (entryAddress 4, 2)]
      _ -> []
    switchWords lockCell =
      [ (secondContextBase + contextSchedulerLock c, lockCell)
      , (secondContextBase + contextSchedulerLock c + lockWord (factsLock facts), lockCell)
      , (secondContextBase + contextSchedulerLock c + lockAcquire (factsLock facts), stubAcquire)
      , (secondContextBase + contextSchedulerLock c + lockRelease (factsLock facts), stubRelease)
      , (secondContextBase + contextTimerHeap c, secondHeapBase)
      , (secondContextBase + contextTimerCount c, dfTimerCount d)
      , (secondContextBase + contextTimerBase c, dfTimerBase d)
      , (secondContextBase + contextTickNow c, dfTickNow d)
      , (secondHeapBase, 10)
      , (secondHeapBase + 4, entryAddress 2)
      , (secondHeapBase + 8, 20)
      , (secondHeapBase + 12, entryAddress 4)
      ]
    taskWords = case caseCallback one of
      WaitRelease ->
        [ (taskAddress + 0x0c, 0x06)
        , (taskAddress + 0x14, taskWait)
        , (taskWait, entryAddress 1)
        , (taskWait + 4, taskQueue)
        , (taskWait + 8, domainId domain)
        ]
      _ -> []
    c = factsContext facts

overlay :: Word32 -> Word32 -> [(Word32, Word32)] -> B.ByteString
overlay base size words' = B.concat (pieces 0 ordered)
  where
    sizeI = fromIntegral size :: Int
    ordered =
      M.toAscList
        ( M.fromList
            [ (fromIntegral (address - base) :: Int, value)
            | (address, value) <- words'
            , address >= base
            , address + 4 <= base + size
            ]
        )
    pieces cursor [] = [B.replicate (sizeI - cursor) 0]
    pieces cursor ((offset, value) : rest) =
      B.replicate (offset - cursor) 0 : word32le value : pieces (offset + 4) rest

cases :: Facts -> Domain -> [Case]
cases facts domain = coreCases facts domain ++ extraCases facts domain ++ initializeCases facts domain

coreCases :: Facts -> Domain -> [Case]
coreCases facts domain =
  [ Case "insert-first" Insert [] 0 1 10 0 10 0 0x13 Receipt Equal
      [CountIs 1, EntryIndexIs 1 1, KernelWord (heapNode facts domain 1) 10, KernelWord (heapNode facts domain 1 + 4) (entryAddress 1)]
      [] [] [] []
  , Case "insert-second-up" Insert [(10, 1)] 1 2 5 0 10 0 0x13 Receipt Equal
      [CountIs 2, EntryIndexIs 2 1, EntryIndexIs 1 2, KernelWord (heapNode facts domain 1) 5, KernelWord (heapNode facts domain 2) 10
      , KernelWord (heapNode facts domain 1 + 4) (entryAddress 2), KernelWord (heapNode facts domain 2 + 4) (entryAddress 1)]
      [] [] [] []
  , Case "insert-third-up" Insert [(10, 1), (20, 2)] 2 3 5 0 10 0 0x13 Receipt Equal
      [CountIs 3, EntryIndexIs 3 1, EntryIndexIs 1 3, KernelWord (heapNode facts domain 1) 5, KernelWord (heapNode facts domain 2) 20, KernelWord (heapNode facts domain 3) 10]
      [] [] [] []
  , Case "insert-third-stable" Insert [(10, 1), (20, 2)] 2 3 15 0 10 0 0x13 Receipt Equal
      [CountIs 3, EntryIndexIs 3 3, KernelWord (heapNode facts domain 1) 10, KernelWord (heapNode facts domain 2) 20, KernelWord (heapNode facts domain 3) 15]
      [] [] [] []
  , Case "insert-wrap" Insert [(0xfffffff5, 1), (0x00000000, 2)] 2 3 0x00000010 0xfffffff0 0x00000020 0 0x13 Receipt Equal
      [CountIs 3, KernelWord (heapNode facts domain 1) 0xfffffff5, KernelWord (heapNode facts domain 2) 0x00000000, KernelWord (heapNode facts domain 3) 0x00000010]
      [] [] [] []
  , Case "remove-middle" Remove [(10, 1), (20, 2), (30, 3)] 3 2 0 0 10 0 0x13 Receipt (Repair "a removed entry is detached")
      []
      [CountIs 2, EntryIndexIs 2 2, EntryIndexIs 1 1, EntryIndexIs 3 2, KernelWord (heapNode facts domain 1) 10, KernelWord (heapNode facts domain 2) 30]
      [CountIs 2, EntryIndexIs 2 0, EntryIndexIs 1 1, EntryIndexIs 3 2, KernelWord (heapNode facts domain 1) 10, KernelWord (heapNode facts domain 2) 30]
      [] []
  , Case "remove-last" Remove [(10, 1), (20, 2), (30, 3)] 3 3 0 0 10 0 0x13 Receipt (Repair "a removed entry is detached")
      []
      [CountIs 2, EntryIndexIs 3 3, KernelWord (heapNode facts domain 1) 10, KernelWord (heapNode facts domain 2) 20]
      [CountIs 2, EntryIndexIs 3 0, KernelWord (heapNode facts domain 1) 10, KernelWord (heapNode facts domain 2) 20]
      [] []
  , Case "tick-empty" Tick [] 0 1 0 0 10 1 0x13 Receipt Equal
      [CountIs 0, BaseIs 10, TickIs 20, RegisterIs R0 0]
      [] [] [] []
  , Case "tick-callback-rearm" Tick [(10, 1)] 1 1 0 0 10 1 0x13 Rearm Equal
      [CountIs 1, BaseIs 10, TickIs 20, RegisterIs R0 0, EntryIndexIs 1 1, KernelWord (heapNode facts domain 1) 40]
      [] [] [] []
  , Case "tick-context" Tick [(10, 1)] 1 1 0 0 10 0 0x13 Receipt Equal
      [CountIs 1, BaseIs 0, TickIs 10, RegisterIs R0 (0xffffffe7)]
      [] [] [] []
  , Case "tick-masked" Tick [(10, 1)] 1 1 0 0 10 1 0x93 Receipt Equal
      [CountIs 1, BaseIs 0, TickIs 10, RegisterIs R0 (0xffffffe7)]
      [] [] [] []
  , Case "tick-ordered" Tick [(10, 1), (20, 2), (30, 3)] 3 1 0 0 30 1 0x13 Receipt (Repair "the tick invalidates a popped entry before its callback")
      []
      [CountIs 0, BaseIs 30, TickIs 40, RegisterIs R0 0, CallbacksAre [entryAddress 1, entryAddress 2, entryAddress 3], CallbackLocksAre [1, 1, 1], EntryIndexIs 1 1]
      [CountIs 0, BaseIs 30, TickIs 40, RegisterIs R0 0, CallbacksAre [entryAddress 1, entryAddress 2, entryAddress 3], CallbackLocksAre [1, 1, 1], EntryIndexIs 1 0]
      [] []
  , Case "tick-pop-then-cancel" Tick [(10, 1), (20, 2)] 2 1 0 0 10 1 0x13 Cancel (Repair "the popped entry is detached before the callback can cancel it")
      []
      [CountIs 0, EntryIndexIs 1 1, EntryIndexIs 2 1]
      [CountIs 1, EntryIndexIs 1 0, EntryIndexIs 2 1, KernelWord (heapNode facts domain 1) 20, KernelWord (heapNode facts domain 1 + 4) (entryAddress 2)]
      [] []
  , Case "insert-already-linked" Insert [(10, 1)] 1 1 20 0 10 0 0x13 Receipt (Repair "a linked entry is refused without a write")
      []
      [CountIs 2, EntryIndexIs 1 2, KernelWord (heapNode facts domain 2) 20, KernelWord (heapNode facts domain 2 + 4) (entryAddress 1)]
      [CountIs 1, EntryIndexIs 1 1, RegionsUnchanged ["kernel", "entries", "arena"]]
      [] []
  , Case "insert-full" Insert (replicate 184 (100, 1)) 184 2 200 0 10 0 0x13 Receipt (Fixture "the fixture refuses an insertion above its 184-node bound")
      []
      [CountIs 185, EntryIndexIs 2 185, KernelWord (heapNode facts domain 185) 200]
      [CountIs 184, EntryIndexIs 2 0, KernelWord (heapNode facts domain 185) 0, RegionsUnchanged ["kernel", "entries", "arena"]]
      [] []
  ]

extraCases :: Facts -> Domain -> [Case]
extraCases facts domain =
  [ Case "insert-second-fiq" Insert [(10, 1)] 1 2 5 0 10 0 0x53 Receipt Equal
      [CountIs 2, EntryIndexIs 2 1, EntryIndexIs 1 2, KernelWord (heapNode facts domain 1) 5, KernelWord (heapNode facts domain 2) 10]
      [] [] [] []
  , Case "tick-ordered-fiq" Tick [(10, 1), (20, 2), (30, 3)] 3 1 0 0 30 1 0x53 Receipt (Repair "the tick invalidates a popped entry before its callback")
      []
      [CountIs 0, BaseIs 30, TickIs 40, RegisterIs R0 0, CallbacksAre [entryAddress 1, entryAddress 2, entryAddress 3], CallbackLocksAre [1, 1, 1], EntryIndexIs 1 1]
      [CountIs 0, BaseIs 30, TickIs 40, RegisterIs R0 0, CallbacksAre [entryAddress 1, entryAddress 2, entryAddress 3], CallbackLocksAre [1, 1, 1], EntryIndexIs 1 0]
      [] []
  , Case "tick-masked-fiq" Tick [(10, 1)] 1 1 0 0 10 1 0xd3 Receipt Equal
      [CountIs 1, BaseIs 0, TickIs 10, RegisterIs R0 (0xffffffe7)]
      [] [] [] []
  , Case "tick-context-switch" Tick [(10, 1), (20, 3)] 2 1 0 0 10 1 0x13 Switch (Repair "the tick reloads the current context and detaches popped entries")
      []
      [ CountIs 0, BaseIs 10, TickIs 20, RegisterIs R0 0
      , CallbacksAre [entryAddress 1, entryAddress 2]
      , CallbackLocksAre [1, 1]
      , EntryIndexIs 1 1, EntryIndexIs 2 1, EntryIndexIs 3 1, EntryIndexIs 4 2
      , KernelWord (heapNode facts domain 1) 20
      , KernelWord (heapNode facts domain 1 + 4) (entryAddress 3)
      , WordIs "entries" secondHeapBase 10
      , WordIs "entries" (secondHeapBase + 4) (entryAddress 2)
      ]
      [ CountIs 0, BaseIs 10, TickIs 20, RegisterIs R0 0
      , CallbacksAre [entryAddress 1, entryAddress 2]
      , CallbackLocksAre [1, 1]
      , EntryIndexIs 1 0, EntryIndexIs 2 0, EntryIndexIs 3 1, EntryIndexIs 4 2
      , KernelWord (heapNode facts domain 1) 20
      , KernelWord (heapNode facts domain 1 + 4) (entryAddress 3)
      , WordIs "entries" secondHeapBase 10
      , WordIs "entries" (secondHeapBase + 4) (entryAddress 2)
      ]
      [] [(2, 1), (4, 2)]
  , Case "tick-pop-then-wait-release" Tick [(10, 1), (20, 2)] 2 1 0 0 10 1 0x13 WaitRelease (Repair "the tick detaches a popped entry before the wait release observes it")
      []
      [ CountIs 0, EntryIndexIs 1 1, EntryIndexIs 2 1
      , KernelWord (heapNode facts domain 1) 20, KernelWord (heapNode facts domain 1 + 4) (entryAddress 2)
      , WordIs "entries" taskWait 0, WordIs "entries" (taskAddress + 0x0c) 4
      ]
      [ CountIs 1, EntryIndexIs 1 0, EntryIndexIs 2 1
      , KernelWord (heapNode facts domain 1) 20, KernelWord (heapNode facts domain 1 + 4) (entryAddress 2)
      , WordIs "entries" taskWait 0, WordIs "entries" (taskAddress + 0x0c) 4
      ]
      [] []
  , Case "tick-context-switch-distinct" Tick [(10, 1), (20, 3)] 2 1 0 0 10 1 0x13 SwitchDistinct (Repair "the tick releases the lock it acquired before reloading the context")
      []
      [ CountIs 0, BaseIs 10, TickIs 20, RegisterIs R0 0
      , CallbacksAre [entryAddress 1, entryAddress 2]
      , CallbackLocksAre [1, 1]
      , EntryIndexIs 1 1, EntryIndexIs 2 1, EntryIndexIs 3 1, EntryIndexIs 4 2
      , KernelWord (dfSchedulerLock (domainFacts facts domain)) 1
      , WordIs "entries" secondLockCell 0
      ]
      [ CountIs 0, BaseIs 10, TickIs 20, RegisterIs R0 0
      , CallbacksAre [entryAddress 1, entryAddress 2]
      , CallbackLocksAre [1, 0]
      , EntryIndexIs 1 0, EntryIndexIs 2 0, EntryIndexIs 3 1, EntryIndexIs 4 2
      , KernelWord (dfSchedulerLock (domainFacts facts domain)) 0
      , WordIs "entries" secondLockCell 0
      ]
      [] []
  ]

initializeCases :: Facts -> Domain -> [Case]
initializeCases facts domain =
  [ Case "initialize" Initialize [] 9 1 0 77 99 0 0xd3 Receipt Equal
      [ CountIs 0, BaseIs 0, TickIs 10
      , KernelWord (dfTickNow (domainFacts facts domain) + 4) 0
      , ConfigurationPublished, RegionsUnchanged ["entries"]
      ]
      [] [] [] []
  ]

expectedCpsrControl :: Case -> Word32
expectedCpsrControl one = case caseCall one of
  Tick
    | caseHandler one == 0 || caseCpsr one .&. 0x80 /= 0 -> cpsrControl (caseCpsr one)
    | otherwise -> cpsrControl (caseCpsr one .&. complement 0x80)
  _ -> cpsrControl (caseCpsr one)

checksFor :: Facts -> Domain -> Role -> Case -> [Check]
checksFor facts domain role one =
  map (retargetCheck facts domain role) (
    [ ReturnedSuccessfully
    , RegisterIs R4 (seedRegister R4)
    , RegisterIs R5 (seedRegister R5)
    , RegisterIs R6 (seedRegister R6)
    , RegisterIs R7 (seedRegister R7)
    , RegisterIs R8 (seedRegister R8)
    , RegisterIs R9 (seedRegister R9)
    , RegisterIs R10 (seedRegister R10)
    , RegisterIs R11 (seedRegister R11)
    , RegisterIs Sp stackPointer
    , CpsrControlIs (expectedCpsrControl one)
    ]
      ++ case caseKind one of
        Equal -> caseEqualChecks one
        Repair _ -> if role == Stock then caseStockChecks one else caseOwnedChecks one
        Fixture _ -> if role == Stock then caseStockChecks one else caseOwnedChecks one)
    ++ (if role == Owned && caseCall one == Initialize then [RegionsUnchanged ["arena"], ArenaOwned] else [])

check :: Facts -> Domain -> Case -> Role -> NativeObservation -> Check -> Either String ()
check facts domain _one role observation check' = case check' of
  ReturnedSuccessfully -> unless (nativeStatus observation == Returned)
    (Left ("status " ++ runnerStatusLabel (nativeStatus observation)))
  KernelWord address value -> expectWord (nodeRegion address) address value
  EntryWord slot offset value -> expectWord "entries" (entryAddress slot + offset) value
  CountIs value -> expectWord "kernel" (dfTimerCount (domainFacts facts domain)) value
  BaseIs value -> expectWord "kernel" (dfTimerBase (domainFacts facts domain)) value
  TickIs value -> expectWord "kernel" (dfTickNow (domainFacts facts domain)) value
  RegisterIs register value -> checkObservation facts domain role observation (RegisterIs register value)
  CallbacksAre expected -> do
    count <- readCounter
    unless (count == fromIntegral (length expected)) (Left ("callback count " ++ show count ++ ", expected " ++ show (length expected)))
    actual <- forM [0 .. fromIntegral count - 1] $ \index -> readWord "entries" (receiptBase + index * 4)
    unless (actual == expected) (Left ("callbacks " ++ show (map showHexWord actual) ++ ", expected " ++ show (map showHexWord expected)))
  CallbackLocksAre expected -> do
    count <- readCounter
    unless (count == fromIntegral (length expected)) (Left ("callback count " ++ show count ++ ", expected " ++ show (length expected)))
    actual <- forM [0 .. fromIntegral count - 1] $ \index -> readWord "entries" (receiptBase + 0x80 + index * 4)
    unless (actual == expected) (Left ("callback lock words " ++ show actual ++ ", expected " ++ show expected))
  EntryIndexIs slot value -> expectWord "entries" (entryAddress slot) value
  CpsrControlIs value -> checkObservation facts domain role observation (CpsrControlIs value)
  WordIs region address value -> expectWord region address value
  ByteIs region address value -> checkObservation facts domain role observation (ByteIs region address value)
  WroteSequence address expected -> checkObservation facts domain role observation (WroteSequence address expected)
  ConfigurationPublished -> do
    let d = domainFacts facts domain
        region = regionName configurationRegion
        base = wordAddress (configurationAddress domain)
    expectWord region (base + 0x20) (heapBase facts domain role)
    expectWord region (base + 0x24) (dfTimerCount d)
    expectWord region (base + 0x28) (dfTimerBase d)
    expectWord region (base + 0x2c) (dfTickNow d)
  ArenaOwned -> do
    let arena = arenaFor facts domain
        (low, high) = arenaSpan facts domain
        pool = reservationTaskPool (factsReservation facts)
        region = regionName configurationRegion
        base = wordAddress (configurationAddress domain)
    unless (low == spanStart (arenaGuest arena)
      && spanSize (arenaGuest arena) == 0x800
      && spanStart (arenaGuest arena) `mod` 32 == 0
      && arenaNodeBytes arena <= spanSize (arenaGuest arena)
      && spanStart (arenaPhysical arena) >= spanEnd (poolAllocatablePhysical pool)
      && spanStart (poolReserved pool) <= low && high <= spanEnd (poolReserved pool))
      (Left "owned timer arena leaves its reserved span or capacity")
    expectWord region (base + 0x20) low
  RegionsUnchanged regions -> do
    forM_ regions $ \region -> do
      actual <- readRegion region
      unless (actual == initialRegion region) (Left ("region " ++ region ++ " changed"))
  other -> checkObservation facts domain role observation other
  where
    readCounter = readWord "entries" counterBase
    expectWord region address value = do
      actual <- readWord region address
      unless (actual == value) (Left (region ++ " " ++ showHexWord address ++ " is " ++ showHexWord actual ++ ", expected " ++ showHexWord value))
    readWord region address = do
      bytes <- readRegion region
      let base = regionBase region
      if address < base || address + 4 > base + fromIntegral (B.length bytes)
        then Left ("address " ++ showHexWord address ++ " is outside " ++ region)
        else Right (readWordAt bytes (fromIntegral (address - base)))
    readRegion region = case lookup (RegionId region) (nativeMemory observation) of
      Nothing -> Left ("missing region " ++ region)
      Just bytes -> Right bytes
    initialRegion region = case region of
      "kernel" -> overlay (kernelLow facts domain) (kernelSize facts domain role) (kernelWords facts domain role _one)
      "entries" -> overlay entriesBase entriesSize (entriesWords facts domain role _one)
      "arena" -> let (low, size) = (fst (arenaSpan facts domain), arenaSize facts domain)
                 in overlay low size (kernelWords facts domain role _one)
      _ -> B.empty
    regionBase "kernel" = kernelLow facts domain
    regionBase "entries" = entriesBase
    regionBase "arena" = fst (arenaSpan facts domain)
    regionBase region | RegionId region == configurationRegion = wordAddress (configurationAddress domain)
    regionBase _ = 0
    nodeRegion address
      | role == Owned, let (low, high) = arenaSpan facts domain, address >= low, address < high = "arena"
      | otherwise = "kernel"

-- | The checks that read only the observation, shared by the heap cases and the tick-to-completion
-- composition cases.
checkObservation :: Facts -> Domain -> Role -> NativeObservation -> Check -> Either String ()
checkObservation _facts _domain _role observation check' = case check' of
  RegisterIs register value ->
    case lookupRegister register observation of
      Just actual | actual == value -> Right ()
      Just actual -> Left (registerLabel register ++ " is " ++ showHexWord actual ++ ", expected " ++ showHexWord value)
      Nothing -> Left ("missing register " ++ registerLabel register)
  CpsrControlIs value ->
    case lookupRegister Cpsr observation of
      Just actual
        | cpsrControl actual == cpsrControl value -> Right ()
        | otherwise -> Left ("cpsr control " ++ showHexWord (cpsrControl actual) ++ ", expected " ++ showHexWord (cpsrControl value))
      Nothing -> Left "missing register cpsr"
  WordIs region address value -> expectWord region address value
  WordIsRole region address stockValue ownedValue -> expectWord region address (if _role == Stock then stockValue else ownedValue)
  ByteIs region address value -> do
    bytes <- readRegion region
    if address < regionLow region || address + 1 > regionHigh region
      then Left ("address " ++ showHexWord address ++ " is outside " ++ region)
      else do
        let actual = fromIntegral (B.index bytes (fromIntegral (address - regionLow region)))
        unless (actual == value) (Left (region ++ " " ++ showHexWord address ++ " is " ++ showHexWord actual ++ ", expected " ++ showHexWord value))
  WroteSequence address expected -> wroteSequence address expected
  WroteSequenceOwned address expected -> case _role of
    Owned -> wroteSequence address expected
    Stock -> Right ()
  _ -> Left ("check does not apply to this observation: " ++ describe check')
  where
    wroteSequence address expected =
      case nativeKernel observation >>= observedOrder of
        Nothing -> Left "the write log is truncated"
        Just order -> do
          let actual =
                [ fromIntegral value
                | OrderedWrite (Write (GuestAddress at) _) (WriteValue value) <- order
                , at == fromIntegral address
                ]
          unless (expected `isSubsequenceOf` actual) (Left ("writes at " ++ showHexWord address ++ " are " ++ show (map showHexWord actual) ++ ", expected the subsequence " ++ show (map showHexWord expected)))
    expectWord region address value = do
      actual <- readWord region address
      unless (actual == value) (Left (region ++ " " ++ showHexWord address ++ " is " ++ showHexWord actual ++ ", expected " ++ showHexWord value))
    readWord region address = do
      bytes <- readRegion region
      if address < regionLow region || address + 4 > regionHigh region
        then Left ("address " ++ showHexWord address ++ " is outside " ++ region)
        else Right (readWordAt bytes (fromIntegral (address - regionLow region)))
    readRegion region = case lookup (RegionId region) (nativeMemory observation) of
      Nothing -> Left ("missing region " ++ region)
      Just bytes -> Right bytes
    regionLow region = regionBaseOf region
    regionHigh region = regionLow region + fromIntegral (B.length (regionBytes' region))
    regionBytes' region = fromMaybe B.empty (lookup (RegionId region) (nativeMemory observation))
    regionBaseOf "data-a" = globalsBase DomainA
    regionBaseOf "data-b" = globalsBase DomainB
    regionBaseOf "entries" = entriesBase
    regionBaseOf "stack" = composeStackBase
    regionBaseOf _ = 0

showHexWord :: Word32 -> String
showHexWord value = "0x" ++ showHex' value
  where
    showHex' 0 = "0"
    showHex' n = reverse (go n)
    go 0 = []
    go n = let (q, r) = n `divMod` 16 in "0123456789abcdef" !! fromIntegral r : go q

regionDifferences :: Facts -> Domain -> [RegionId] -> NativeObservation -> NativeObservation -> [String]
regionDifferences facts domain regions stock owned =
  [ "region " ++ regionName ident ++ " differs"
  | ident <- regions
  , masked stock ident /= masked owned ident
  ]
  where
    masked observation ident = fmap (normalize ident) (M.lookup ident (M.fromList (nativeMemory observation)))
    normalize ident bytes = case regionName ident of
      "kernel" -> maskRange (fromIntegral (dfTimerHeap d - kernelLow facts domain)) (0x5c0 + 0x20) bytes
      "entries" -> maskRange (fromIntegral (contextBase + contextTimerHeap c - entriesBase)) 4 bytes
      _ -> bytes
    d = domainFacts facts domain
    c = factsContext facts

relocatedHeapDifferences :: Facts -> Domain -> NativeObservation -> NativeObservation -> [String]
relocatedHeapDifferences facts domain stock owned =
  case (lookup (RegionId "kernel") (nativeMemory stock), lookup (RegionId "arena") (nativeMemory owned)) of
    (Just kernel, Just arena)
      | offset + 0x5c0 <= B.length kernel ->
          [ "relocated owned heap differs" | B.take 0x5c0 (B.drop offset kernel) /= B.take 0x5c0 arena ]
    _ -> ["relocated owned heap region is missing"]
  where
    offset = fromIntegral (dfTimerHeap (domainFacts facts domain) - kernelLow facts domain)

-- | Zero a subrange so a role-specific pointer or the relocated node array compares equal.
maskRange :: Int -> Int -> B.ByteString -> B.ByteString
maskRange offset size bytes
  | offset < 0 || size < 0 || offset + size > B.length bytes = bytes
  | otherwise = B.take offset bytes <> B.replicate size 0 <> B.drop (offset + size) bytes

caseFor :: Facts -> Domain -> Role -> Image -> Image -> [OwnedSymbol] -> [Aggregate] -> Case -> CaseSpec
caseFor facts domain role stockImage ownedImage ownedSymbols aggregates one =
  CaseSpec
    { caseId = caseName one ++ "-" ++ domainLabel domain
    , caseProvenance = "X-M1 1.50 timer stock ABI, x70view 55436fc85c0560dd252b0410af6f2427fe198cd3; synthetic heap, entry and callback state"
    , caseExclusion = "Synthetic timer heap and foreign fixture callbacks; wait-release instrumentation uses the retained stock helper over a synthetic suspended task. No full-system qualification."
    , caseEntry = GuestAddress (fromIntegral entry)
    , caseExit = GuestAddress (fromIntegral exitAddress)
    , caseRegions =
        codeRegions ++
        [ mkRegion (RegionId "stubs") (GuestAddress (fromIntegral stubBase)) (fromIntegral stubSize) ReadExecute (stubBytes facts domain one)
        , mkRegion (RegionId "kernel") (GuestAddress (fromIntegral (kernelLow facts domain))) (fromIntegral (kernelSize facts domain role)) ReadWrite (overlay (kernelLow facts domain) (kernelSize facts domain role) (kernelWords facts domain role one))
        , mkRegion (RegionId "entries") (GuestAddress (fromIntegral entriesBase)) (fromIntegral entriesSize) ReadWrite (overlay entriesBase entriesSize (entriesWords facts domain role one))
        , mkRegion (RegionId "stack") (GuestAddress (fromIntegral stackBase)) (fromIntegral stackSize) ReadWrite (B.replicate (fromIntegral stackSize) 0)
        , mkRegion (RegionId "exit") (GuestAddress (fromIntegral exitAddress)) 0x10 ReadExecute (B.replicate 0x10 0)
        ]
          ++ [ mkRegion (RegionId "wait-release") (GuestAddress (fromIntegral (waitReleaseAddress domain))) (fromIntegral waitReleaseSize) ReadExecute (slice (imageBase stockImage) (imageBytes stockImage) (waitReleaseAddress domain) waitReleaseSize)
             | role == Stock || null aggregates
             ]
          ++ [ mkRegion (RegionId "arena") (GuestAddress (fromIntegral (fst (arenaSpan facts domain)))) (fromIntegral (arenaSize facts domain)) ReadWrite (overlay (fst (arenaSpan facts domain)) (arenaSize facts domain) (kernelWords facts domain role one))
             | role == Owned
             ]
          ++ [ mkRegion configurationRegion (configurationAddress domain) 0x70 ReadWrite (B.replicate 0x70 0xa5)
             | caseCall one == Initialize
             ]
    , caseFunctions = [(GuestAddress (fromIntegral address), size) | (address, size) <- functions]
    , caseInitial = initialRegisters
    , caseExpectedStatus = Returned
    , caseExpectedRegisters =
        [ (R4, seedRegister R4), (R5, seedRegister R5), (R6, seedRegister R6), (R7, seedRegister R7)
        , (R8, seedRegister R8), (R9, seedRegister R9), (R10, seedRegister R10), (R11, seedRegister R11)
        , (Sp, stackPointer)
        ]
          ++ case caseCall one of
            Tick -> [(R0, expectedTickResult one)]
            _ -> []
    , caseExpectedMemory = []
    , caseCp14Identity = Just (if domain == DomainA then 0x4003 else 0x4004)
    , caseKernel = Just (KernelCase plainLimit Nothing (Just 0x80001234) (Just 0) (Just 0) [R1, R2, R3, R12, Lr] [])
    }
  where
    codeRegions
      | role == Owned && not (null aggregates) =
          concat [aggregateRegions ("aggregate-" ++ show index) aggregate | (index, aggregate) <- zip [0 :: Int ..] aggregates]
      | otherwise = [mkRegion (RegionId "code") (GuestAddress (fromIntegral codeBase)) (B.length codeBytes) ReadExecute codeBytes]
    entry = case caseCall one of
      Initialize -> wordAddress (initializeAddress domain)
      Insert -> insertEntry domain
      Remove -> removeEntry domain
      Tick -> tickEntry domain
    (codeBase, codeBytes) = case role of
      Stock ->
        let (base, size) = stockBase domain
            (start, extent) = if caseCall one == Initialize then (base - 0x70, size + 0x70) else (base, size)
         in (start, slice (imageBase stockImage) (imageBytes stockImage) start extent)
      Owned -> (imageBase ownedImage, imageBytes ownedImage)
    functions = case role of
      Stock ->
        let (base, _) = stockBase domain
         in [(base, 0xc4), (base + 0xc4, 0xdc), (insertEntry domain, 0x48), (removeEntry domain, 0xe4), (tickEntry domain, 0x1a0), (stubBase, stubSize), (waitReleaseAddress domain, waitReleaseSize)]
              ++ [(wordAddress (initializeAddress domain), 0x70) | caseCall one == Initialize]
      Owned -> (if null aggregates then [(ownedAddress symbol, ownedSize symbol) | symbol <- ownedSymbols] else concatMap aggregateFunctions aggregates) ++ [(stubBase, stubSize), (waitReleaseAddress domain, waitReleaseSize)]
    initialRegisters =
      [ (register, case lookup register arguments of
          Just value -> value
          Nothing -> seedRegister register)
      | register <- [R0 .. R12]
      ]
        ++ [(Sp, stackPointer), (Lr, exitAddress), (Cpsr, caseCpsr one)]
    arguments = case caseCall one of
      Initialize -> []
      Insert -> [(R0, contextBase), (R1, entryAddress (caseSlot one)), (R2, caseExpiry one)]
      Remove -> [(R0, contextBase), (R1, entryAddress (caseSlot one))]
      Tick -> [(R0, 0), (R1, 0), (R2, 0)]

expectedTickResult :: Case -> Word32
expectedTickResult one = case [value | RegisterIs R0 value <- caseEqualChecks one ++ caseStockChecks one ++ caseOwnedChecks one] of
  (value : _) -> value
  [] -> 0

slice :: Word32 -> B.ByteString -> Word32 -> Word32 -> B.ByteString
slice base bytes address size
  | address < base || address + size > base + fromIntegral (B.length bytes) = error "timer code slice is outside its image"
  | otherwise = B.take (fromIntegral size) (B.drop (fromIntegral (address - base)) bytes)

data OwnedSymbol = OwnedSymbol
  { ownedName :: String
  , ownedAddress :: Word32
  , ownedSize :: Word32
  }

timerSpans, s3dSpans :: [(Word32, Word32)]
timerSpans = [(0x44a50, 0x44f2c), (0xd4a10, 0xd4eec)]
s3dSpans = [(0x44318, 0x54000), (0xd42d8, 0xe4000)]

parseSymbolsIn :: [(Word32, Word32)] -> FilePath -> IO [OwnedSymbol]
parseSymbolsIn spans path = do
  rows <- lines <$> readFile path
  pure (concatMap row rows)
  where
    row line = case words line of
      [a, z, kind, name] -> case (parseWord32 a, parseWord32 z) of
        (Just address, Just size)
          | kind `elem` ["T", "t"], size > 0
          , any (\(low, high) -> address >= low && address < high && size <= high - address) spans ->
              [OwnedSymbol name address size]
        _ -> []
      _ -> []

parseOwnedSymbols :: FilePath -> IO [OwnedSymbol]
parseOwnedSymbols = parseSymbolsIn timerSpans

parseS3dSymbols :: FilePath -> IO [OwnedSymbol]
parseS3dSymbols = parseSymbolsIn s3dSpans

parseAggregateSymbols :: FilePath -> IO [OwnedSymbol]
parseAggregateSymbols = parseSymbolsIn [(0, maxBound)]

cohortOf :: Case -> String
cohortOf one = case caseName one of
  "initialize" -> "initialize"
  "insert-second-fiq" -> "extra"
  "tick-ordered-fiq" -> "extra"
  "tick-masked-fiq" -> "extra"
  "tick-context-switch" -> "extra"
  "tick-pop-then-wait-release" -> "extra"
  "tick-context-switch-distinct" -> "extra"
  _ -> "core"

type W = M.Map Word32 Word32

data ComposeKind
  = TimeoutObject
  | TimeoutSleeping
  | TimeoutTerminate
  | TimeoutPriority
  | TimeoutCrossOwner
  | DelayElapsed
  deriving (Eq, Show)

data ComposeControl = ControlRemoveCallback | ControlWrongLock | ControlWrongDetach
  deriving (Eq, Show)

data ComposeCase = ComposeCase
  { composeName :: String
  , composeKind :: ComposeKind
  , composeSetup :: Domain -> W -> W
  , composeChecks :: [Check]
  }

composeStackBase, composeStackSize, composeStackTop, composeExit, globalsSize :: Word32
composeStackBase = 0x40070000
composeStackSize = 0x1000
composeStackTop = composeStackBase + 0x800
composeExit = 0x40080000
globalsSize = 0x14000

globalsBase :: Domain -> Word32
globalsBase DomainA = 0x40050000
globalsBase DomainB = 0x400e0000

composeLoaderBase :: Domain -> Word32
composeLoaderBase DomainA = 0x40000
composeLoaderBase DomainB = 0xd0000

composeBodyBase :: Domain -> Word32
composeBodyBase DomainA = 0x50000
composeBodyBase DomainB = 0xe0000

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

composeTask :: Facts -> Domain -> Int -> Word32
composeTask facts d slot =
  let sf = factsScheduler facts
      sd = sfDomain sf d
   in swTaskTable sd + (fromIntegral slot - 1) * tkSize (sfTask sf)

fixtureWait, fixtureQueue, fixtureTimer :: SchedulerDomain -> Word32
fixtureWait sd = swTimerHeap sd + 0x800
fixtureQueue sd = fixtureWait sd + 0x20
fixtureTimer sd = fixtureWait sd + 0x40

seedGlobals :: Facts -> Domain -> W -> W
seedGlobals facts d =
  let sf = factsScheduler facts
      sd = sfDomain sf d
      headSize = rhSize (sfReadyHead sf)
      priorities = sfPriorities sf
   in foldl (\f (address, value) -> f . setW address value) id
        ([ (swReadyHeads sd + priority * headSize, swReadyHeads sd + priority * headSize) | priority <- [0 .. priorities - 1] ]
          ++ [ (swReadyHeads sd + priority * headSize + rhPrev (sfReadyHead sf), swReadyHeads sd + priority * headSize) | priority <- [0 .. priorities - 1] ])
     . setW (swReadyBitmap sd) 0
     . setW (swSelected sd) 0
     . setW (swRunning sd) 0
     . setW (swDispatchEnabled sd) 0
     . setW (swTimerCount sd) 0
     . setW (swTimerBase sd) 0
     . setW (swTickNow sd) 1000

seedTasks :: Facts -> Domain -> W -> W
seedTasks facts d =
  let sf = factsScheduler facts
      sd = sfDomain sf d
      taskOffset = sfTask sf
      definitionOffset = sfDefinition sf
      (sp0, sp1) = dfSpTerms definitionOffset
   in foldl (\m slot ->
        let task = swTaskTable sd + (slot - 1) * tkSize taskOffset
            definition = swTaskDefinitions sd + (slot - 1) * dfSize definitionOffset
         in m
           . setW (task + tkDefinition taskOffset) definition
           . setB (task + tkState taskOffset) 0
           . setW (task + tkNext taskOffset) 0
           . setW (task + tkPrev taskOffset) 0
           . setW (task + tkWait taskOffset) 0
           . setW (task + tkDomain taskOffset) (domainId d)
           . setW (definition + dfExists definitionOffset) 1
           . setW (definition + dfPriority definitionOffset) 15
           . setW (definition + sp0) 0x100
           . setW (definition + sp1) composeStackBase) id [1 .. sfTaskSlots sf]

seedRecord :: Facts -> Domain -> Domain -> W -> W
seedRecord facts executing target =
  let sf = factsScheduler facts
      contextOffset = sfContext sf
      lockOffset = sfLock sf
      targetWords = sfDomain sf target
      helpers = sfHelpers sf target
      context = swContexts (sfDomain sf executing) + if target == DomainA then 0 else cxSize contextOffset
      recordId = if target == executing then 0 else domainId target
   in setW (context + cxId contextOffset) recordId
    . setW (context + cxSchedulerLock contextOffset) (swSchedulerLock targetWords)
    . setW (context + cxSchedulerLock contextOffset + lockAcquire lockOffset) (helpAcquire helpers)
    . setW (context + cxSchedulerLock contextOffset + lockRelease lockOffset) (helpRelease helpers)
    . setW (context + cxObjectLock contextOffset) (swObjectLock targetWords)
    . setW (context + cxObjectLock contextOffset + lockAcquire lockOffset) (helpAcquire helpers)
    . setW (context + cxObjectLock contextOffset + lockRelease lockOffset) (helpRelease helpers)
    . setW (context + cxReadyHeads contextOffset) (swReadyHeads targetWords)
    . setW (context + cxReadyBitmap contextOffset) (swReadyBitmap targetWords)
    . setW (context + cxRunning contextOffset) (swRunning targetWords)
    . setW (context + cxSelected contextOffset) (swSelected targetWords)
    . setW (context + cxDispatchEnabled contextOffset) (swDispatchEnabled targetWords)
    . setW (context + cxTimerHeap contextOffset) (swTimerHeap targetWords)
    . setW (context + cxTimerCount contextOffset) (swTimerCount targetWords)
    . setW (context + cxTimerBase contextOffset) (swTimerBase targetWords)
    . setW (context + cxTickNow contextOffset) (swTickNow targetWords)
    . setW (context + cxTasks contextOffset) (swTaskTable targetWords)
    . setW (context + cxTaskCount contextOffset) (sfTaskSlots sf)
    . setW (context + cxFirstEntry contextOffset) (helpFirstEntry helpers)

baseFixture :: Facts -> Domain -> W
baseFixture facts executing =
  let sf = factsScheduler facts
      sd = sfDomain sf executing
      contextOffset = sfContext sf
      executingContext = swContexts sd + if executing == DomainA then 0 else cxSize contextOffset
   in foldl (.) id
        [ setW (swCurrentContext sd) executingContext
        , setW (swHandlerContext sd) 1
        , setW (swDispatchEnabled sd) 1
        , setW (swSchedulerLock (sfDomain sf DomainA)) 0
        , setW (swSchedulerLock (sfDomain sf DomainB)) 0
        , setW (swObjectLock (sfDomain sf DomainA)) 0
        , setW (swObjectLock (sfDomain sf DomainB)) 0
        , seedRecord facts executing DomainA
        , seedRecord facts executing DomainB
        , seedTasks facts DomainA
        , seedTasks facts DomainB
        , seedGlobals facts DomainA
        , seedGlobals facts DomainB
        ] M.empty

makeReady :: Facts -> Domain -> Int -> Word32 -> Bool -> W -> W
makeReady facts d slot priority selected m =
  let sf = factsScheduler facts
      sd = sfDomain sf d
      taskOffset = sfTask sf
      headOffset = sfReadyHead sf
      task = composeTask facts d slot
      readyHead' = swReadyHeads sd + priority * rhSize headOffset
      previous = wordOf (readyHead' + rhPrev headOffset) m
      m1 = setB (task + tkState taskOffset) (stateReady (sfStates sf))
         . setB (task + tkPriority taskOffset) priority
         . setW (task + tkNext taskOffset) previous
         . setW (task + tkPrev taskOffset) readyHead'
         . setW previous task
         . setW (readyHead' + rhPrev headOffset) task
         . setW (swReadyBitmap sd) (wordOf (swReadyBitmap sd) m .|. (1 `shiftL` fromIntegral priority))
         $ m
   in if selected then setW (swSelected sd) task m1 else m1

makeRunning :: Facts -> Domain -> Int -> W -> W
makeRunning facts d slot =
  makeReady facts d slot 0 True
    . setW (swRunning (sfDomain (factsScheduler facts) d)) (composeTask facts d slot)

setupRunning :: Facts -> Domain -> W -> W
setupRunning facts d = setW (swSelected (sfDomain (factsScheduler facts) d)) 0 . makeRunning facts d 1

objectWaitTask :: Facts -> Domain -> Int -> Word32 -> Word32 -> W -> W
objectWaitTask facts d slot owner callback =
  let sf = factsScheduler facts
      sd = sfDomain sf d
      taskOffset = sfTask sf
      states = sfStates sf
      task = composeTask facts d slot
      wait = fixtureWait sd
      queue = fixtureQueue sd
      timer = fixtureTimer sd
   in setB (task + tkState taskOffset) (stateWait states .|. stateObjectQueued states)
    . setB (task + tkPriority taskOffset) 3
    . setW (task + tkWait taskOffset) wait
    . setW (task + tkNext taskOffset) queue
    . setW (task + tkPrev taskOffset) queue
    . setW (task + tkTerminatePending taskOffset) 0
    . setW (task + tkPriorityChangePending taskOffset) 0
    . setW (task + tkCancelPending taskOffset) 0
    . setW (task + tkContextDiscarded taskOffset) 0
    . setW wait timer
    . setW (wait + 4) queue
    . setW (wait + 8) owner
    . setW queue task
    . setW (queue + 4) task
    . setW timer 1
    . setW (timer + 4) callback
    . setW (timer + 8) task

sleepingWait :: Facts -> Domain -> Int -> Word32 -> W -> W
sleepingWait facts d slot callback =
  let sf = factsScheduler facts
      sd = sfDomain sf d
      taskOffset = sfTask sf
      states = sfStates sf
      task = composeTask facts d slot
      wait = fixtureWait sd
      timer = fixtureTimer sd
   in setB (task + tkState taskOffset) (stateWait states .|. stateSleeping states)
    . setB (task + tkPriority taskOffset) 3
    . setW (task + tkWait taskOffset) wait
    . setW (task + tkTerminatePending taskOffset) 0
    . setW (task + tkPriorityChangePending taskOffset) 0
    . setW (task + tkCancelPending taskOffset) 0
    . setW (task + tkContextDiscarded taskOffset) 0
    . setW wait timer
    . setW (wait + 4) 0
    . setW (wait + 8) (domainId d)
    . setW timer 1
    . setW (timer + 4) callback
    . setW (timer + 8) task

armTimer :: Facts -> Domain -> W -> W
armTimer facts d =
  let sd = sfDomain (factsScheduler facts) d
      timer = fixtureTimer sd
   in setW (swTimerCount sd) 1
    . setW (swTimerHeap sd) 0
    . setW (swTimerHeap sd + 4) timer

timeoutCallbackAddress, delayCallbackAddress :: Domain -> Word32
timeoutCallbackAddress DomainA = 0x44318
timeoutCallbackAddress DomainB = 0xd42d8
delayCallbackAddress DomainA = 0x44634
delayCallbackAddress DomainB = 0xd45f4

callbackOf :: Domain -> ComposeKind -> Word32
callbackOf d DelayElapsed = delayCallbackAddress d
callbackOf d _ = timeoutCallbackAddress d

composeCases :: Facts -> Domain -> [ComposeCase]
composeCases facts d =
  [ ComposeCase "tick-timeout-object" TimeoutObject setupTimeoutObject (composeChecksFor facts d TimeoutObject)
  , ComposeCase "tick-timeout-sleeping" TimeoutSleeping setupTimeoutSleeping (composeChecksFor facts d TimeoutSleeping)
  , ComposeCase "tick-timeout-terminate" TimeoutTerminate setupTimeoutTerminate (composeChecksFor facts d TimeoutTerminate)
  , ComposeCase "tick-timeout-priority" TimeoutPriority setupTimeoutPriority (composeChecksFor facts d TimeoutPriority)
  , ComposeCase "tick-timeout-cross-owner" TimeoutCrossOwner setupTimeoutCrossOwner (composeChecksFor facts d TimeoutCrossOwner)
  , ComposeCase "tick-delay-elapsed" DelayElapsed setupDelayElapsed (composeChecksFor facts d DelayElapsed)
  ]
  where
    other domain = if domain == DomainA then DomainB else DomainA
    setupTimeoutObject e =
      setupRunning facts e
        . objectWaitTask facts e 2 (domainId e) (callbackOf e TimeoutObject)
        . armTimer facts e
    setupTimeoutSleeping e =
      setupRunning facts e
        . sleepingWait facts e 2 (callbackOf e TimeoutSleeping)
        . armTimer facts e
    setupTimeoutTerminate e =
      let taskOffset = sfTask (factsScheduler facts)
          task = composeTask facts e 2
       in setupRunning facts e
        . setW (task + tkTerminatePending taskOffset) 1
        . setB (task + tkFlags taskOffset) 1
        . objectWaitTask facts e 2 (domainId e) (callbackOf e TimeoutTerminate)
        . armTimer facts e
    setupTimeoutPriority e =
      let taskOffset = sfTask (factsScheduler facts)
          task = composeTask facts e 2
       in setupRunning facts e
        . setW (task + tkPriorityChangePending taskOffset) 1
        . setW (task + tkPendingPriority taskOffset) 7
        . objectWaitTask facts e 2 (domainId e) (callbackOf e TimeoutPriority)
        . armTimer facts e
    setupTimeoutCrossOwner e =
      setupRunning facts e
        . objectWaitTask facts e 2 (domainId (other e)) (callbackOf e TimeoutCrossOwner)
        . armTimer facts e
    setupDelayElapsed e =
      setupRunning facts e
        . sleepingWait facts e 2 (callbackOf e DelayElapsed)
        . armTimer facts e

composeChecksFor :: Facts -> Domain -> ComposeKind -> [Check]
composeChecksFor facts d kind =
  [ RegisterIs R4 (seedRegister R4)
  , RegisterIs R5 (seedRegister R5)
  , RegisterIs R6 (seedRegister R6)
  , RegisterIs R7 (seedRegister R7)
  , RegisterIs R8 (seedRegister R8)
  , RegisterIs R9 (seedRegister R9)
  , RegisterIs R10 (seedRegister R10)
  , RegisterIs R11 (seedRegister R11)
  , RegisterIs Sp composeStackTop
  , RegisterIs R0 0
  , CpsrControlIs 0x53
  , WordIs region (swTimerCount sd) 0
  , WordIsRole region timer 1 0
  , WordIs region (swTimerBase sd) 1000
  , WordIs region (swTickNow sd) 1010
  , WordIs region lockWord 0
  , WordIs objectRegion objectWord 0
  , WordIs region (swReschedule sd) 1
  , ByteIs region (task + tkState taskOffset) (stateReady states)
  , WordIs region (task + tkCancelPending taskOffset) 0
  , WordIs region (task + tkWait taskOffset) wait
  , WordIs region wait finalResult
  , WordIs region (wait + 4) queueValue
  , WordIs region (wait + 8) (domainId owner)
  , WordIs region readyHead task
  , WordIs region (readyHead + rhPrev headOffset) task
  , WordIs region (swReadyBitmap sd) (1 .|. (1 `shiftL` fromIntegral priority))
  , WordIs region (swSelected sd) task
  , WroteSequenceOwned timer [0]
  , WroteSequence (swReschedule sd) [1]
  , WroteSequence lockWord lockWrites
  ]
    ++ [ WordIs region queue queue | objectQueued ]
    ++ [ WordIs region (queue + 4) queue | objectQueued ]
    ++ [ WroteSequence objectWord [1, 0] | objectQueued ]
    ++ [ WordIs region (task + tkContextDiscarded taskOffset) 1 | kind == TimeoutTerminate ]
    ++ [ ByteIs region (task + tkFlags taskOffset) 0 | kind == TimeoutTerminate ]
    ++ [ WordIs region (task + tkPriorityChangePending taskOffset) 0 | kind == TimeoutPriority ]
    ++ [ ByteIs region (task + tkPriority taskOffset) 7 | kind == TimeoutPriority ]
    ++ [ WordIs region (swObjectLock sd) 0 | kind == TimeoutCrossOwner ]
    ++ [WordIs "stack" (composeStackTop + offset) 0xa7a7a7a7 | offset <- [0, 4 .. 0x3c]]
    ++ [WordIs "stack" (composeStackBase + offset) 0 | offset <- [0, 4 .. 0x3c]]
    ++ [WordIs "stack" (composeStackBase + composeStackSize - 0x40 + offset) 0xa7a7a7a7 | offset <- [0, 4 .. 0x3c]]
  where
    sf = factsScheduler facts
    sd = sfDomain sf d
    taskOffset = sfTask sf
    headOffset = sfReadyHead sf
    states = sfStates sf
    region = if d == DomainA then "data-a" else "data-b"
    otherRegion = if d == DomainA then "data-b" else "data-a"
    other = if d == DomainA then DomainB else DomainA
    task = composeTask facts d 2
    wait = fixtureWait sd
    queue = fixtureQueue sd
    timer = fixtureTimer sd
    objectQueued = kind `elem` [TimeoutObject, TimeoutTerminate, TimeoutPriority, TimeoutCrossOwner]
    owner = if kind == TimeoutCrossOwner then other else d
    objectRegion = if kind == TimeoutCrossOwner then otherRegion else region
    lockWord = swSchedulerLock sd
    objectWord = swObjectLock (sfDomain sf owner)
    priority = case kind of
      TimeoutPriority -> 7
      TimeoutTerminate -> 15
      _ -> 3
    readyHead = swReadyHeads sd + priority * rhSize headOffset
    queueValue = if objectQueued then queue else 0
    finalResult = if kind == DelayElapsed then 0 else 0xffffffce
    lockWrites = if objectQueued then [1, 0, 1, 0] else [1, 0]

composeHelpers :: Domain -> [(Word32, Word32)]
composeHelpers DomainA =
  [ (0x439d8, 0x3c), (0x43a14, 0x40), (0x433e4, 0x60), (0x43c48, 0x5c)
  , (0x43ca4, 0x7c), (0x43d20, 0xb0), (0x43dd0, 0x1a0), (0x43f70, 0xa8), (0x44018, 0x2c)
  ]
composeHelpers DomainB =
  [ (0xd398c, 0x3c), (0xd39c8, 0x40), (0xd33e4, 0x60), (0xd3c04, 0x5c)
  , (0xd3c60, 0x7c), (0xd3cdc, 0xb0), (0xd3d8c, 0x1a0), (0xd3f2c, 0xa8), (0xd3fd4, 0x30)
  ]

stockComposeFunctions :: Domain -> ComposeKind -> [(Word32, Word32)]
stockComposeFunctions d kind =
  [ (tickEntry d, 0x1a0)
  , (callbackOf d kind, if kind == DelayElapsed then 0xc8 else 0x1cc)
  , (if d == DomainA then 0x44b84 else 0xd4b44, 0xdc)
  ]
    ++ composeHelpers d

ownedComposeFunctions :: Domain -> [OwnedSymbol] -> [OwnedSymbol] -> ComposeKind -> [(Word32, Word32)]
ownedComposeFunctions d timerSymbols s3dSymbols kind =
  [(ownedAddress symbol, ownedSize symbol) | symbol <- timerSymbols]
    ++ [(ownedAddress symbol, ownedSize symbol) | symbol <- s3dSymbols]
    ++ [(callbackOf d kind, 4)]
    ++ composeHelpers d

padTo :: Int -> B.ByteString -> B.ByteString
padTo size bytes = B.take size (bytes <> B.replicate size 0)

poke :: B.ByteString -> Word32 -> B.ByteString -> B.ByteString
poke bytes offset value = B.take (fromIntegral offset) bytes <> value <> B.drop (fromIntegral offset + B.length value) bytes

pokeSlice :: B.ByteString -> Word32 -> B.ByteString -> B.ByteString
pokeSlice bytes offset chunk = poke bytes offset chunk

serialize :: Word32 -> Word32 -> W -> B.ByteString
serialize base size m = foldl write (B.replicate (fromIntegral size) 0) (M.toList m)
  where
    write bytes (address, value)
      | address >= base && address + 4 <= base + size = poke bytes (address - base) (word32le value)
      | otherwise = bytes

s3dEntries :: Domain -> [Word32]
s3dEntries DomainA = [0x44318, 0x44634, 0x454f0, 0x4582c, 0x45e54, 0x46534, 0x467b4, 0x46e48]
s3dEntries DomainB = [0xd42d8, 0xd45f4, 0xd54b0, 0xd57ec, 0xd5e14, 0xd64f4, 0xd6774, 0xd6e08]

s3dDelayBody :: Domain -> Word32
s3dDelayBody DomainA = 0x46e4c
s3dDelayBody DomainB = 0xd6e0c

composeOwnedCode :: Domain -> B.ByteString -> Image -> [OwnedSymbol] -> Image -> [(Word32, Word32)] -> B.ByteString
composeOwnedCode d loader timerImage timerSymbols s3dImage extra =
  let base = composeLoaderBase d
      timerBase = minimum (map ownedAddress timerSymbols)
      s3dBase = imageBase s3dImage
      s3dBytes = imageBytes s3dImage
      withTimer = pokeSlice (padTo 0x10000 loader) (timerBase - base) (imageBytes timerImage)
      withS3d = foldl
        (\bytes (address, size, source) -> pokeSlice bytes (address - base) (slice s3dBase s3dBytes source size))
        withTimer
        ([(entry, 4, entry) | entry <- s3dEntries d]
          ++ [(s3dDelayBody d, 0x180, s3dDelayBody d)])
   in foldl (\bytes (address, word) -> poke bytes (address - base) (word32le word)) withS3d extra

composeControlPatches :: Domain -> Image -> [OwnedSymbol] -> ComposeControl -> Either String [(Word32, Word32)]
composeControlPatches _d timerImage timerSymbols control = do
  body <- case [symbol | symbol <- timerSymbols, ownedName symbol == "xm1_rtos_timer_tick_body"] of
    [symbol] -> Right symbol
    [] -> Left "the timer tick body symbol is missing"
    _ -> Left "the timer tick body symbol is duplicated"
  let candidates = case control of
        ControlRemoveCallback -> [(0x118, 0xe12fff38, 0xe1a00000), (0xd0, 0xe12fff35, 0xe1a00000)]
        ControlWrongLock -> [(0x124, 0xe1a00006, 0xe286000c), (0xd4, 0xe1a00006, 0xe286000c)]
        ControlWrongDetach -> [(0xd4, 0xe5812000, 0xe1a00000), (0xb4, 0xe5801000, 0xe1a00000)]
      matches =
        [(address, replacement)
        | (offset, expected, replacement) <- candidates
        , offset + 4 <= ownedSize body
        , let address = ownedAddress body + offset
        , address >= imageBase timerImage
        , toInteger address + 4 <= toInteger (imageBase timerImage) + toInteger (B.length (imageBytes timerImage))
        , readWordAt (imageBytes timerImage) (fromIntegral (address - imageBase timerImage)) == expected
        ]
  case matches of
    [patch] -> Right [patch]
    _ -> Left ("the emitted timer tick has no unique " ++ composeControlName control ++ " instruction site")

composeCaseFor :: Facts -> Domain -> Role -> Image -> Image -> Image -> [OwnedSymbol] -> Image -> [OwnedSymbol] -> [Aggregate] -> [(Word32, Word32)] -> String -> ComposeCase -> CaseSpec
composeCaseFor facts d role stockImage otherImage timerImage timerSymbols s3dImage s3dSymbols aggregates extra suffix one =
  CaseSpec
    { caseId = composeName one ++ suffix ++ "-" ++ domainLabel d
    , caseProvenance = "X-M1 1.50 timer tick plus S3d timeout/delay completion, x70view 55436fc85c0560dd252b0410af6f2427fe198cd3; synthetic scheduler state"
    , caseExclusion = "One tick with one due entry; synthetic single-core state. Not natural callback delivery, cross-core concurrency, cache ordering or the full dispatcher."
    , caseEntry = GuestAddress (fromIntegral (tickEntry d))
    , caseExit = GuestAddress (fromIntegral composeExit)
    , caseRegions =
        codeRegions ++
        [ mkRegion (RegionId "data-a") (GuestAddress (fromIntegral (globalsBase DomainA))) (fromIntegral globalsSize) ReadWrite (serialize (globalsBase DomainA) globalsSize fixture)
        , mkRegion (RegionId "data-b") (GuestAddress (fromIntegral (globalsBase DomainB))) (fromIntegral globalsSize) ReadWrite (serialize (globalsBase DomainB) globalsSize fixture)
        , mkRegion (RegionId "stack") (GuestAddress (fromIntegral composeStackBase)) (fromIntegral composeStackSize) ReadWrite (B.replicate 0x800 0 <> B.replicate 0x800 0xa7)
        , mkRegion (RegionId "exit") (GuestAddress (fromIntegral composeExit)) 0x10 ReadExecute (B.replicate 0x10 0)
        ]
          ++ [ mkRegion (RegionId "code-other") (GuestAddress (fromIntegral (composeLoaderBase other))) 0x10000 ReadExecute (padTo 0x10000 (imageBytes otherImage))
             | crossOwner, role == Stock || null aggregates
             ]
    , caseFunctions = [(GuestAddress (fromIntegral address), size) | (address, size) <- functions]
    , caseInitial =
        [ (R0, 0), (R1, 0), (R2, 0), (R3, 0)
        , (R4, seedRegister R4), (R5, seedRegister R5), (R6, seedRegister R6), (R7, seedRegister R7)
        , (R8, seedRegister R8), (R9, seedRegister R9), (R10, seedRegister R10), (R11, seedRegister R11)
        , (R12, 0), (Sp, composeStackTop), (Lr, composeExit), (Cpsr, 0x53)
        ]
    , caseExpectedStatus = Returned
    , caseExpectedRegisters =
        [ (R4, seedRegister R4), (R5, seedRegister R5), (R6, seedRegister R6), (R7, seedRegister R7)
        , (R8, seedRegister R8), (R9, seedRegister R9), (R10, seedRegister R10), (R11, seedRegister R11)
        , (Sp, composeStackTop), (R0, 0)
        ]
    , caseExpectedMemory = []
    , caseCp14Identity = Just (if d == DomainA then 0x4003 else 0x4004)
    , caseKernel = Just (KernelCase (InstructionLimit 40000) Nothing (Just 0x80001234) (Just 0) (Just 0) [R1, R2, R3, R12, Lr, Pc, Cpsr] [])
    }
  where
    fixture = composeSetup one d (baseFixture facts d)
    codeRegions
      | role == Owned, not (null aggregates) =
          concat [if index == selected then aggregateControlRegions label extra aggregate else aggregateRegions label aggregate
            | (index, aggregate) <- zip [0 :: Int ..] aggregates
            , let selected = if d == DomainA then 0 else 1
                  label = "aggregate-" ++ show index]
      | otherwise =
          [ mkRegion (RegionId "code") (GuestAddress (fromIntegral (composeLoaderBase d))) 0x10000 ReadExecute codeBytes
          , mkRegion (RegionId "bodies") (GuestAddress (fromIntegral (composeBodyBase d))) 0x4000 ReadExecute bodies
          ]
    codeBytes = case role of
      Stock -> padTo 0x10000 (imageBytes stockImage)
      Owned -> composeOwnedCode d (imageBytes stockImage) timerImage timerSymbols s3dImage extra
    bodies = case role of
      Stock -> B.replicate 0x4000 0
      Owned -> slice (imageBase s3dImage) (imageBytes s3dImage) (composeBodyBase d) 0x4000
    other = if d == DomainA then DomainB else DomainA
    crossOwner = composeKind one == TimeoutCrossOwner
    otherHelpers = sfHelpers (factsScheduler facts) other
    functions =
      (case role of
        Stock -> stockComposeFunctions d (composeKind one)
        Owned
          | null aggregates -> ownedComposeFunctions d timerSymbols s3dSymbols (composeKind one)
          | otherwise -> concatMap aggregateFunctions aggregates ++
              [(0x42560, 8), (0x42568, 12), (0xd2560, 8), (0xd2568, 12)])
        ++ (if crossOwner && (role == Stock || null aggregates) then [(helpAcquire otherHelpers, 0x3c), (helpRelease otherHelpers, 0x40)] else [])

composeControlName :: ComposeControl -> String
composeControlName ControlRemoveCallback = "callback-removed"
composeControlName ControlWrongLock = "wrong-lock"
composeControlName ControlWrongDetach = "wrong-detach"

composeControls :: ComposeKind -> [ComposeControl]
composeControls TimeoutObject = [ControlRemoveCallback, ControlWrongLock, ControlWrongDetach]
composeControls _ = []

composeRegionDifferences :: Facts -> Domain -> NativeObservation -> NativeObservation -> [String]
composeRegionDifferences facts domain stock owned =
  [ "region " ++ name ++ " differs"
  | name <- ["data-a", "data-b"]
  , masked name (lookup (RegionId name) (nativeMemory stock)) /= masked name (lookup (RegionId name) (nativeMemory owned))
  ]
  where
    timerAddress = fixtureTimer (sfDomain (factsScheduler facts) domain)
    masked name = fmap (maskEntry name)
    maskEntry name bytes
      | timerAddress >= base && timerAddress + 4 <= base + fromIntegral (B.length bytes) =
          B.take offset bytes <> B.replicate 4 0 <> B.drop (offset + 4) bytes
      | otherwise = bytes
      where
        base = if name == "data-a" then globalsBase DomainA else globalsBase DomainB
        offset = fromIntegral (timerAddress - base)

runDomain :: FilePath -> FilePath -> FilePath -> Facts -> Domain -> Image -> Image -> Image -> [OwnedSymbol] -> Image -> [OwnedSymbol] -> [Aggregate] -> IO ([CaseRun], [ControlRun])
runDomain runner cacheRoot directory facts domain stockImage otherImage ownedImage ownedSymbols s3dImage s3dSymbols aggregates = do
  let selected = cases facts domain
  (coreRuns, coreControls) <- runCohort "core" (filter ((== "core") . cohortOf) selected) directory
  (extraRuns, extraControls) <- runCohort "extra" (filter ((== "extra") . cohortOf) selected) (directory </> "extra")
  (initializeRuns, initializeControls) <- runCohort "initialize" (filter ((== "initialize") . cohortOf) selected) (directory </> "initialize")
  (composeRuns, composeControlRuns) <- runComposeCohort (composeCases facts domain) (directory </> "composition")
  pure
    ( coreRuns ++ extraRuns ++ initializeRuns ++ composeRuns
    , coreControls ++ extraControls ++ initializeControls ++ composeControlRuns ++
        [ControlRun "admission" name "aggregate" holds | (name, holds) <- admissionControls]
    )
  where
    runCohort cohort chosen base = do
      let stockSpecs = map (caseFor facts domain Stock stockImage ownedImage ownedSymbols aggregates) chosen
          ownedSpecs = map (caseFor facts domain Owned stockImage ownedImage ownedSymbols aggregates) chosen
      stockBatch <- writeBatch (base </> "stock") stockSpecs
      ownedBatch <- writeBatch (base </> "owned") ownedSpecs
      (stockReuse, stockRaw) <- cachedObservedBatch cacheRoot runner stockBatch
      (ownedReuse, ownedRaw) <- cachedObservedBatch cacheRoot runner ownedBatch
      let reference = M.fromList [(caseIdText ident, observation) | (ident, observation) <- stockRaw]
          subject = M.fromList [(caseIdText ident, observation) | (ident, observation) <- ownedRaw]
      results <- forM chosen $ \one -> do
        let ident = caseName one ++ "-" ++ domainLabel domain
            stock = M.lookup ident reference
            owned = M.lookup ident subject
            roleChecks role observation = map (check facts domain one role observation) (checksFor facts domain role one)
            errorsFor role observation = [message | Left message <- roleChecks role observation]
            equalDifferences stock' owned' = case caseKind one of
              Equal -> regionDifferences facts domain [RegionId "kernel", RegionId "entries"] stock' owned'
                ++ relocatedHeapDifferences facts domain stock' owned'
              _ -> []
        let run = CaseRun
              { caseDomain = domain
              , runCohort = cohort
              , runName = caseName one
              , runKind = caseKind one
              , runChecks = case (stock, owned) of
                  (Just stock', Just owned') ->
                    [ ("stock: " ++ describe check', either (const False) (const True) (check facts domain one Stock stock' check'))
                    | check' <- checksFor facts domain Stock one
                    ]
                      ++ [ ("owned: " ++ describe check', either (const False) (const True) (check facts domain one Owned owned' check'))
                         | check' <- checksFor facts domain Owned one
                         ]
                  _ -> []
              , runDifferences = case (stock, owned) of
                  (Just stock', Just owned') ->
                    [ "stock: " ++ message | message <- errorsFor Stock stock' ]
                      ++ [ "owned: " ++ message | message <- errorsFor Owned owned' ]
                      ++ equalDifferences stock' owned'
                  (Nothing, _) -> ["stock case did not run"]
                  (_, Nothing) -> ["owned case did not run"]
              , runReuse = (stockReuse, ownedReuse)
              }
            controls = case owned of
              Just observed -> controlRuns facts domain cohort one observed
              Nothing -> []
        pure (run, controls)
      pure (map fst results, concatMap snd results)
    runComposeCohort chosen base = do
      let stockSpecs = [composeCaseFor facts domain Stock stockImage otherImage ownedImage ownedSymbols s3dImage s3dSymbols aggregates [] "" one | one <- chosen]
          ownedSpecs = [composeCaseFor facts domain Owned stockImage otherImage ownedImage ownedSymbols s3dImage s3dSymbols aggregates [] "" one | one <- chosen]
          controls =
            [ (composeName one, control, composeCaseFor facts domain Owned stockImage otherImage ownedImage ownedSymbols s3dImage s3dSymbols aggregates (either error id (composeControlPatches domain ownedImage ownedSymbols control)) ("-control-" ++ composeControlName control) one)
            | one <- chosen
            , control <- composeControls (composeKind one)
            ]
      stockBatch <- writeBatch (base </> "stock") stockSpecs
      ownedBatch <- writeBatch (base </> "owned") ownedSpecs
      controlBatch <- writeBatch (base </> "controls") (map (\(_, _, spec) -> spec) controls)
      (stockReuse, stockRaw) <- cachedObservedBatch cacheRoot runner stockBatch
      (ownedReuse, ownedRaw) <- cachedObservedBatch cacheRoot runner ownedBatch
      (_, controlRaw) <- cachedObservedBatch cacheRoot runner controlBatch
      let reference = M.fromList [(caseIdText ident, observation) | (ident, observation) <- stockRaw]
          subject = M.fromList [(caseIdText ident, observation) | (ident, observation) <- ownedRaw]
          controlObservations = M.fromList [(caseIdText ident, observation) | (ident, observation) <- controlRaw]
          statusCheck observation = case nativeStatus observation of
            Returned -> Right ()
            status -> Left ("status " ++ runnerStatusLabel status)
          errorsFor role checks observation = [message | Left message <- statusCheck observation : map (checkObservation facts domain role observation) checks]
      results <- forM chosen $ \one -> do
        let ident = composeName one ++ "-" ++ domainLabel domain
            stock = M.lookup ident reference
            owned = M.lookup ident subject
            checks = composeChecks one
            run = CaseRun
              { caseDomain = domain
              , runCohort = "composition"
              , runName = composeName one
              , runKind = Equal
              , runChecks = case (stock, owned) of
                  (Just stock', Just owned') ->
                    [ ("stock: " ++ describe check', either (const False) (const True) (checkObservation facts domain Stock stock' check'))
                    | check' <- checks
                    ]
                      ++ [ ("owned: " ++ describe check', either (const False) (const True) (checkObservation facts domain Owned owned' check'))
                         | check' <- checks
                         ]
                  _ -> []
              , runDifferences = case (stock, owned) of
                  (Just stock', Just owned') ->
                    [ "stock: " ++ message | message <- errorsFor Stock checks stock' ]
                      ++ [ "owned: " ++ message | message <- errorsFor Owned checks owned' ]
                      ++ composeRegionDifferences facts domain stock' owned'
                  (Nothing, _) -> ["stock case did not run"]
                  (_, Nothing) -> ["owned case did not run"]
              , runReuse = (stockReuse, ownedReuse)
              }
            controlResults =
              [ ControlRun "composition" ("executed-" ++ composeControlName control) (composeName one)
                  (not (null (errorsFor Owned checks observed)))
              | (name, control, _) <- controls
              , name == composeName one
              , Just observed <- [M.lookup (composeName one ++ "-control-" ++ composeControlName control ++ "-" ++ domainLabel domain) controlObservations]
              ]
        pure (run, controlResults)
      pure (map fst results, concatMap snd results)

-- | The checks must refuse a cleared I, a changed F and a changed byte in every declared
-- unchanged region.
controlRuns :: Facts -> Domain -> String -> Case -> NativeObservation -> [ControlRun]
controlRuns facts domain cohort one observation =
  [ ControlRun cohort "native-unknown-call" (caseName one)
      (refused ReturnedSuccessfully (observation {nativeStatus = UnknownCall}))
  , ControlRun cohort "native-unsupported" (caseName one)
      (refused ReturnedSuccessfully (observation {nativeStatus = UnsupportedInstruction}))
  , ControlRun cohort "cpsr-i" (caseName one) (refused (CpsrControlIs expected) (mutateCpsr (`xor` 0x80) observation))
  , ControlRun cohort "cpsr-f" (caseName one) (refused (CpsrControlIs expected) (mutateCpsr (`xor` 0x40) observation))
  ]
    ++ [ ControlRun cohort "configuration-publication" (caseName one) (refused ConfigurationPublished (mutateConfiguration observation))
       | caseCall one == Initialize
       ]
    ++ [ ControlRun cohort name (caseName one) (refused ArenaOwned (mutateConfigurationWord value observation))
       | caseCall one == Initialize
       , (name, value) <-
           [ ("owned-stock-heap", dfTimerHeap d)
           , ("owned-arena-alias", spanStart (arenaGuest (arenaFor facts other)))
           , ("owned-arena-boundary", spanStart (arenaGuest (arenaFor facts domain)) + arenaSize facts domain)
           , ("owned-arena-capacity", spanStart (arenaGuest (arenaFor facts domain)) + 0x240)
           ]
       ]
    ++ [ ControlRun cohort ("unchanged-" ++ region) (caseName one) (refused (RegionsUnchanged [region]) (mutateRegion region observation))
       | region <- declaredRegions
       ]
    ++ [ ControlRun cohort "relocated-heap-entry" (caseName one)
           (refused (KernelWord (heapNodeRole facts domain Owned 1 + 4) (entryAddress 2)) (mutateArenaWord (heapNodeRole facts domain Owned 1 + 4) 0 observation))
       | caseName one == "insert-second-up"
       ]
  where
    expected = expectedCpsrControl one
    refused check' mutated = case check facts domain one Owned mutated check' of
      Left _ -> True
      Right () -> False
    mutateCpsr f observed = observed {nativeRegisters = M.adjust f Cpsr (nativeRegisters observed)}
    declaredRegions = [region | RegionsUnchanged regions <- checksFor facts domain Owned one, region <- regions]
    mutateConfiguration observed =
      observed
        { nativeMemory =
            [ (ident, if ident == configurationRegion then B.take 0x20 bytes <> flipHead (B.drop 0x20 bytes) else bytes)
            | (ident, bytes) <- nativeMemory observed
            ]
        }
    mutateArenaWord address value observed =
      let (low, _) = arenaSpan facts domain
          offset = fromIntegral (address - low)
      in observed
        { nativeMemory =
            [ (ident, if regionName ident == "arena" then B.take offset bytes <> word32le value <> B.drop (offset + 4) bytes else bytes)
            | (ident, bytes) <- nativeMemory observed
            ]
        }
    mutateConfigurationWord value observed =
      observed
        { nativeMemory =
            [ (ident, if ident == configurationRegion then B.take 0x20 bytes <> word32le value <> B.drop 0x24 bytes else bytes)
            | (ident, bytes) <- nativeMemory observed
            ]
        }
    d = domainFacts facts domain
    other = case domain of DomainA -> DomainB; DomainB -> DomainA
    mutateRegion region observed =
      observed
        { nativeMemory =
            [ (ident, if regionName ident == region then flipHead bytes else bytes)
            | (ident, bytes) <- nativeMemory observed
            ]
        }
    flipHead bytes = case B.uncons bytes of
      Just (first, rest) -> B.cons (first `xor` (0xff :: Word8)) rest
      Nothing -> bytes

describe :: Check -> String
describe check' = case check' of
  ReturnedSuccessfully -> "native returned"
  KernelWord address value -> "kernel " ++ showHexWord address ++ " = " ++ showHexWord value
  EntryWord slot offset value -> "entry " ++ show slot ++ "+" ++ show offset ++ " = " ++ showHexWord value
  CountIs value -> "count = " ++ show value
  BaseIs value -> "base = " ++ show value
  TickIs value -> "tick = " ++ show value
  RegisterIs register value -> registerLabel register ++ " = " ++ showHexWord value
  CallbacksAre values -> "callbacks = " ++ show (map showHexWord values)
  CallbackLocksAre values -> "callback locks = " ++ show values
  EntryIndexIs slot value -> "entry " ++ show slot ++ " index = " ++ show value
  CpsrControlIs value -> "cpsr control = " ++ showHexWord (cpsrControl value)
  WordIs region address value -> region ++ " " ++ showHexWord address ++ " = " ++ showHexWord value
  WordIsRole region address stockValue ownedValue -> region ++ " " ++ showHexWord address ++ " = stock " ++ showHexWord stockValue ++ " / owned " ++ showHexWord ownedValue
  ByteIs region address value -> region ++ " byte " ++ showHexWord address ++ " = " ++ showHexWord value
  WroteSequence address values -> "writes " ++ showHexWord address ++ " = " ++ show (map showHexWord values)
  WroteSequenceOwned address values -> "owned writes " ++ showHexWord address ++ " = " ++ show (map showHexWord values)
  RegionsUnchanged regions -> "unchanged " ++ unwords regions
  ConfigurationPublished -> "timer configuration pointers"
  ArenaOwned -> "owned timer arena publication"
