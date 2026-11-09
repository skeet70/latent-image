{-# LANGUAGE OverloadedStrings #-}

-- | Timer heap, membership and callback cases over the stock loaders and the owned A32 entries.
module Replacement.RtosTimer.Suite
  ( Image (..)
  , CaseRun (..)
  , ControlRun (..)
  , runDomain
  , parseOwnedSymbols
  ) where

import Control.Monad (forM, forM_, unless)
import Data.Bits ((.&.), (.|.), complement, shiftL, shiftR, xor)
import qualified Data.ByteString as B
import qualified Data.Map.Strict as M
import Data.Maybe (fromMaybe)
import Data.Word (Word32, Word8)
import Emulation.CachedNativeBatch (BatchReuse, cachedObservedBatch)
import Emulation.NativeObservation
import Emulation.OwnedCase
import Replacement.RtosOwnFirst.Regions (Domain (..), domainLabel)
import Replacement.RtosTimer.Layout
import System.FilePath ((</>))

data Image = Image { imageBase :: Word32, imageBytes :: B.ByteString }

data Role = Stock | Owned deriving (Eq, Show)

data Call = Initialize | Insert | Remove | Tick deriving (Eq, Show)

data Callback = Receipt | Cancel | Rearm | Switch | SwitchDistinct | WaitRelease deriving (Eq, Show)

data Kind = Equal | Repair String | Fixture String deriving (Eq, Show)

data Check
  = KernelWord Word32 Word32
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
      [] [(2, 1), (4, 2)]
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
    [ RegisterIs R4 (seedRegister R4)
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
  KernelWord address value -> expectWord (nodeRegion address) address value
  EntryWord slot offset value -> expectWord "entries" (entryAddress slot + offset) value
  CountIs value -> expectWord "kernel" (dfTimerCount (domainFacts facts domain)) value
  BaseIs value -> expectWord "kernel" (dfTimerBase (domainFacts facts domain)) value
  TickIs value -> expectWord "kernel" (dfTickNow (domainFacts facts domain)) value
  RegisterIs register value ->
    case lookupRegister register observation of
      Just actual | actual == value -> Right ()
      Just actual -> Left (registerLabel register ++ " is " ++ showHexWord actual ++ ", expected " ++ showHexWord value)
      Nothing -> Left ("missing register " ++ registerLabel register)
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
  CpsrControlIs value ->
    case lookupRegister Cpsr observation of
      Just actual
        | cpsrControl actual == cpsrControl value -> Right ()
        | otherwise -> Left ("cpsr control " ++ showHexWord (cpsrControl actual) ++ ", expected " ++ showHexWord (cpsrControl value))
      Nothing -> Left "missing register cpsr"
  WordIs region address value -> expectWord region address value
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

caseFor :: Facts -> Domain -> Role -> Image -> Image -> [(Word32, Word32)] -> Case -> CaseSpec
caseFor facts domain role stockImage ownedImage ownedSymbols one =
  CaseSpec
    { caseId = caseName one ++ "-" ++ domainLabel domain
    , caseProvenance = "X-M1 1.50 timer stock ABI, x70view 55436fc85c0560dd252b0410af6f2427fe198cd3; synthetic heap, entry and callback state"
    , caseExclusion = "Synthetic timer heap and callback stubs; the wait-release case runs the stock wait-release body over a synthetic suspended task. No cyclic or firmware integration."
    , caseEntry = GuestAddress (fromIntegral entry)
    , caseExit = GuestAddress (fromIntegral exitAddress)
    , caseRegions =
        [ mkRegion (RegionId "code") (GuestAddress (fromIntegral codeBase)) (fromIntegral (B.length codeBytes)) ReadExecute codeBytes
        , mkRegion (RegionId "stubs") (GuestAddress (fromIntegral stubBase)) (fromIntegral stubSize) ReadExecute (stubBytes facts domain one)
        , mkRegion (RegionId "wait-release") (GuestAddress (fromIntegral (waitReleaseAddress domain))) (fromIntegral waitReleaseSize) ReadExecute (slice (imageBase stockImage) (imageBytes stockImage) (waitReleaseAddress domain) waitReleaseSize)
        , mkRegion (RegionId "kernel") (GuestAddress (fromIntegral (kernelLow facts domain))) (fromIntegral (kernelSize facts domain role)) ReadWrite (overlay (kernelLow facts domain) (kernelSize facts domain role) (kernelWords facts domain role one))
        , mkRegion (RegionId "entries") (GuestAddress (fromIntegral entriesBase)) (fromIntegral entriesSize) ReadWrite (overlay entriesBase entriesSize (entriesWords facts domain role one))
        , mkRegion (RegionId "stack") (GuestAddress (fromIntegral stackBase)) (fromIntegral stackSize) ReadWrite (B.replicate (fromIntegral stackSize) 0)
        , mkRegion (RegionId "exit") (GuestAddress (fromIntegral exitAddress)) 0x10 ReadExecute (B.replicate 0x10 0)
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
      Owned -> ownedSymbols ++ [(stubBase, stubSize), (waitReleaseAddress domain, waitReleaseSize)]
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

parseOwnedSymbols :: FilePath -> IO [(Word32, Word32)]
parseOwnedSymbols path = do
  rows <- lines <$> readFile path
  pure (concatMap row rows)
  where
    row line = case words line of
      [a, z, kind, _] -> case (parseWord32 a, parseWord32 z) of
        (Just address, Just size)
          | kind `elem` ["T", "t"], size > 0
          , (address >= 0x44a50 && address < 0x44f2c && size <= 0x44f2c - address)
            || (address >= 0xd4a10 && address < 0xd4eec && size <= 0xd4eec - address) -> [(address, size)]
        _ -> []
      _ -> []

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

runDomain :: FilePath -> FilePath -> FilePath -> Facts -> Domain -> Image -> Image -> [(Word32, Word32)] -> IO ([CaseRun], [ControlRun])
runDomain runner cacheRoot directory facts domain stockImage ownedImage ownedSymbols = do
  let selected = cases facts domain
  (coreRuns, coreControls) <- runCohort "core" (filter ((== "core") . cohortOf) selected) directory
  (extraRuns, extraControls) <- runCohort "extra" (filter ((== "extra") . cohortOf) selected) (directory </> "extra")
  (initializeRuns, initializeControls) <- runCohort "initialize" (filter ((== "initialize") . cohortOf) selected) (directory </> "initialize")
  pure (coreRuns ++ extraRuns ++ initializeRuns, coreControls ++ extraControls ++ initializeControls)
  where
    runCohort cohort chosen base = do
      let stockSpecs = map (caseFor facts domain Stock stockImage ownedImage ownedSymbols) chosen
          ownedSpecs = map (caseFor facts domain Owned stockImage ownedImage ownedSymbols) chosen
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
            statusCheck observation = case nativeStatus observation of
              Returned -> Right ()
              status -> Left ("status " ++ runnerStatusLabel status)
            errorsFor role observation = [message | Left message <- statusCheck observation : roleChecks role observation]
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

-- | The checks must refuse a cleared I, a changed F and a changed byte in every declared
-- unchanged region.
controlRuns :: Facts -> Domain -> String -> Case -> NativeObservation -> [ControlRun]
controlRuns facts domain cohort one observation =
  [ ControlRun cohort "cpsr-i" (caseName one) (refused (CpsrControlIs expected) (mutateCpsr (`xor` 0x80) observation))
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
  RegionsUnchanged regions -> "unchanged " ++ unwords regions
  ConfigurationPublished -> "timer configuration pointers"
  ArenaOwned -> "owned timer arena publication"
