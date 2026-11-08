{-# LANGUAGE OverloadedStrings #-}

-- | Timer heap, membership and callback cases over the stock loaders and the owned A32 entries.
module Replacement.RtosTimer.Suite
  ( Image (..)
  , CaseRun (..)
  , runDomain
  , parseOwnedSymbols
  ) where

import Control.Monad (forM, unless)
import Data.Bits ((.&.), (.|.), shiftL, shiftR)
import qualified Data.ByteString as B
import qualified Data.Map.Strict as M
import Data.Word (Word32)
import Emulation.CachedNativeBatch (BatchReuse, cachedObservedBatch)
import Emulation.NativeObservation
import Emulation.OwnedCase
import Replacement.RtosOwnFirst.Regions (Domain (..), domainLabel)
import Replacement.RtosTimer.Layout
import System.FilePath ((</>))

data Image = Image { imageBase :: Word32, imageBytes :: B.ByteString }

data Role = Stock | Owned deriving (Eq, Show)

data Call = Insert | Remove | Tick deriving (Eq, Show)

data Callback = Receipt | Cancel | Rearm deriving (Eq, Show)

data Kind = Equal | Repair String deriving (Eq, Show)

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
  }

data CaseRun = CaseRun
  { caseDomain :: Domain
  , runName :: String
  , runKind :: Kind
  , runChecks :: [(String, Bool)]
  , runDifferences :: [String]
  , runReuse :: (BatchReuse, BatchReuse)
  }

stubBase, stubAcquire, stubRelease, stubReceipt, stubCancel, stubRearm, stubSize :: Word32
stubBase = 0x01000000
stubAcquire = stubBase
stubRelease = stubBase + 0x18
stubReceipt = stubBase + 0x30
stubCancel = stubBase + 0x80
stubRearm = stubBase + 0xa0
stubSize = 0x100

exitAddress, stackBase, stackSize, stackPointer, entriesBase, entriesSize :: Word32
exitAddress = 0x01000200
stackBase = 0x300000
stackSize = 0x1000
stackPointer = stackBase + 0x800
entriesBase = 0x40100000
entriesSize = 0x1000

contextBase, receiptBase, counterBase, entryTableBase, entryStride :: Word32
contextBase = entriesBase + 0x800
receiptBase = entriesBase + 0x200
counterBase = entriesBase + 0x300
entryTableBase = entriesBase + 0x100
entryStride = 0x10

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

stubBytes :: Facts -> Domain -> B.ByteString
stubBytes facts domain = fillStubs
  [ (0, [ldrI 1 0, movImm 2 1, strI 2 1, movImm 0 0, bxLr])
  , (0x18, [ldrI 1 0, movImm 2 0, strI 2 1, movImm 0 0, bxLr])
  , (0x30, receiptCode)
  , (0x80, [pushLr, movReg 3 0, movwI 0 (currentContext .&. 0xffff), movtI 0 (currentContext `shiftR` 16), ldrI 0 0, movReg 1 3, blWord (stubCancel + 4 * 6) (removeEntry domain), popPc])
  , (0xa0, [pushLr, movReg 3 0, movwI 0 (currentContext .&. 0xffff), movtI 0 (currentContext `shiftR` 16), ldrI 0 0, movReg 1 3, movwI 2 (rearmExpiry .&. 0xffff), movtI 2 (rearmExpiry `shiftR` 16), blWord (stubRearm + 4 * 8) (insertEntry domain), popPc])
  ]
  where
    currentContext = dfCurrentContext (domainFacts facts domain)
    lock = dfSchedulerLock (domainFacts facts domain)
    receiptCode =
      [ pushLr
      , movwI 1 (counterBase .&. 0xffff), movtI 1 (counterBase `shiftR` 16)
      , ldrI 2 1
      , addImm 3 2 1, strI 3 1
      , movwI 1 (receiptBase .&. 0xffff), movtI 1 (receiptBase `shiftR` 16)
      , addRegLsl2 1 1 2, strI 0 1
      , movwI 3 (lock .&. 0xffff), movtI 3 (lock `shiftR` 16), ldrI 3 3
      , movwI 1 ((receiptBase + 0x80) .&. 0xffff), movtI 1 ((receiptBase + 0x80) `shiftR` 16)
      , addRegLsl2 1 1 2, strI 3 1
      , popPc
      ]

rearmExpiry :: Word32
rearmExpiry = 40

callbackAddress :: Case -> Word32
callbackAddress one = case caseCallback one of
  Receipt -> stubReceipt
  Cancel -> stubCancel
  Rearm -> stubRearm

kernelLow, kernelHigh :: Facts -> Domain -> Word32
kernelLow facts domain = dfHandlerContext (domainFacts facts domain)
kernelHigh facts domain = dfTimerHeap (domainFacts facts domain) + 0x5c0 + 0x20

kernelSize :: Facts -> Domain -> Word32
kernelSize facts domain = kernelHigh facts domain - kernelLow facts domain

heapNode :: Facts -> Domain -> Int -> Word32
heapNode facts domain index = dfTimerHeap (domainFacts facts domain) + fromIntegral (index - 1) * 8

entryIndexIn :: Case -> Int -> Word32
entryIndexIn one slot = case [fromIntegral index | (index, (_, heapSlot)) <- zip [1 :: Int ..] (caseHeap one), heapSlot == slot] of
  (index : _) -> index
  [] -> 0

kernelWords :: Facts -> Domain -> Case -> [(Word32, Word32)]
kernelWords facts domain one =
  [ (dfHandlerContext d, caseHandler one)
  , (dfCurrentContext d, contextBase)
  , (dfSchedulerLock d, 0)
  , (dfTimerCount d, caseCount one)
  , (dfTimerBase d, caseBase one)
  , (dfTickNow d, caseTick one)
  ]
  ++ concat
    [ [(heapNode facts domain index, expiry), (heapNode facts domain index + 4, entryAddress slot)]
    | (index, (expiry, slot)) <- zip [1 :: Int ..] (caseHeap one)
    ]
  where
    d = domainFacts facts domain

entriesWords :: Facts -> Domain -> Case -> [(Word32, Word32)]
entriesWords facts domain one =
  contextWords
    ++ concat
      [ [ (entryAddress slot, entryIndexIn one slot)
        , (entryAddress slot + 4, callback)
        , (entryAddress slot + 8, entryAddress slot)
        ]
      | slot <- [1 .. 8]
      ]
    ++ [(counterBase, 0)]
    ++ [(receiptBase + fromIntegral index * 4, 0) | index <- [0 .. 64 :: Int]]
    ++ [(receiptBase + 0x80 + fromIntegral index * 4, 0) | index <- [0 .. 64 :: Int]]
  where
    callback = if caseCall one == Tick then callbackAddress one else 0xdeadbeef
    contextWords =
      let c = factsContext facts
       in [ (contextBase + contextSchedulerLock c, dfSchedulerLock (domainFacts facts domain))
          , (contextBase + contextSchedulerLock c + lockWord (factsLock facts), dfSchedulerLock (domainFacts facts domain))
          , (contextBase + contextSchedulerLock c + lockAcquire (factsLock facts), stubAcquire)
          , (contextBase + contextSchedulerLock c + lockRelease (factsLock facts), stubRelease)
          , (contextBase + contextTimerHeap c, dfTimerHeap (domainFacts facts domain))
          , (contextBase + contextTimerCount c, dfTimerCount (domainFacts facts domain))
          , (contextBase + contextTimerBase c, dfTimerBase (domainFacts facts domain))
          , (contextBase + contextTickNow c, dfTickNow (domainFacts facts domain))
          ]

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
cases facts domain =
  [ Case "insert-first" Insert [] 0 1 10 0 10 0 0x13 Receipt Equal
      [CountIs 1, EntryIndexIs 1 1, KernelWord (heapNode facts domain 1) 10, KernelWord (heapNode facts domain 1 + 4) (entryAddress 1)]
      [] []
  , Case "insert-second-up" Insert [(10, 1)] 1 2 5 0 10 0 0x13 Receipt Equal
      [CountIs 2, EntryIndexIs 2 1, EntryIndexIs 1 2, KernelWord (heapNode facts domain 1) 5, KernelWord (heapNode facts domain 2) 10]
      [] []
  , Case "insert-third-up" Insert [(10, 1), (20, 2)] 2 3 5 0 10 0 0x13 Receipt Equal
      [CountIs 3, EntryIndexIs 3 1, EntryIndexIs 1 3, KernelWord (heapNode facts domain 1) 5, KernelWord (heapNode facts domain 2) 20, KernelWord (heapNode facts domain 3) 10]
      [] []
  , Case "insert-third-stable" Insert [(10, 1), (20, 2)] 2 3 15 0 10 0 0x13 Receipt Equal
      [CountIs 3, EntryIndexIs 3 3, KernelWord (heapNode facts domain 1) 10, KernelWord (heapNode facts domain 2) 20, KernelWord (heapNode facts domain 3) 15]
      [] []
  , Case "insert-wrap" Insert [(0xfffffff5, 1), (0x00000000, 2)] 2 3 0x00000010 0xfffffff0 0x00000020 0 0x13 Receipt Equal
      [CountIs 3, KernelWord (heapNode facts domain 1) 0xfffffff5, KernelWord (heapNode facts domain 2) 0x00000000, KernelWord (heapNode facts domain 3) 0x00000010]
      [] []
  , Case "remove-middle" Remove [(10, 1), (20, 2), (30, 3)] 3 2 0 0 10 0 0x13 Receipt (Repair "a removed entry is detached")
      []
      [CountIs 2, EntryIndexIs 2 2, EntryIndexIs 1 1, EntryIndexIs 3 2, KernelWord (heapNode facts domain 1) 10, KernelWord (heapNode facts domain 2) 30]
      [CountIs 2, EntryIndexIs 2 0, EntryIndexIs 1 1, EntryIndexIs 3 2, KernelWord (heapNode facts domain 1) 10, KernelWord (heapNode facts domain 2) 30]
  , Case "remove-last" Remove [(10, 1), (20, 2), (30, 3)] 3 3 0 0 10 0 0x13 Receipt (Repair "a removed entry is detached")
      []
      [CountIs 2, EntryIndexIs 3 3, KernelWord (heapNode facts domain 1) 10, KernelWord (heapNode facts domain 2) 20]
      [CountIs 2, EntryIndexIs 3 0, KernelWord (heapNode facts domain 1) 10, KernelWord (heapNode facts domain 2) 20]
  , Case "tick-empty" Tick [] 0 1 0 0 10 1 0x13 Receipt Equal
      [CountIs 0, BaseIs 10, TickIs 20, RegisterIs R0 0]
      [] []
  , Case "tick-callback-rearm" Tick [(10, 1)] 1 1 0 0 10 1 0x13 Rearm Equal
      [CountIs 1, BaseIs 10, TickIs 20, RegisterIs R0 0, EntryIndexIs 1 1, KernelWord (heapNode facts domain 1) 40]
      [] []
  , Case "tick-context" Tick [(10, 1)] 1 1 0 0 10 0 0x13 Receipt Equal
      [CountIs 1, BaseIs 0, TickIs 10, RegisterIs R0 (0xffffffe7)]
      [] []
  , Case "tick-masked" Tick [(10, 1)] 1 1 0 0 10 1 0x93 Receipt Equal
      [CountIs 1, BaseIs 0, TickIs 10, RegisterIs R0 (0xffffffe7)]
      [] []
  , Case "tick-ordered" Tick [(10, 1), (20, 2), (30, 3)] 3 1 0 0 30 1 0x13 Receipt (Repair "the tick invalidates a popped entry before its callback")
      []
      [CountIs 0, BaseIs 30, TickIs 40, RegisterIs R0 0, CallbacksAre [entryAddress 1, entryAddress 2, entryAddress 3], CallbackLocksAre [1, 1, 1], EntryIndexIs 1 1]
      [CountIs 0, BaseIs 30, TickIs 40, RegisterIs R0 0, CallbacksAre [entryAddress 1, entryAddress 2, entryAddress 3], CallbackLocksAre [1, 1, 1], EntryIndexIs 1 0]
  , Case "tick-pop-then-cancel" Tick [(10, 1), (20, 2)] 2 1 0 0 10 1 0x13 Cancel (Repair "the popped entry is detached before the callback can cancel it")
      []
      [CountIs 0, EntryIndexIs 1 1, EntryIndexIs 2 1]
      [CountIs 1, EntryIndexIs 1 0, EntryIndexIs 2 1, KernelWord (heapNode facts domain 1) 20, KernelWord (heapNode facts domain 1 + 4) (entryAddress 2)]
  , Case "insert-already-linked" Insert [(10, 1)] 1 1 20 0 10 0 0x13 Receipt (Repair "a linked entry is refused without a write")
      []
      [CountIs 2, EntryIndexIs 1 2, KernelWord (heapNode facts domain 2) 20, KernelWord (heapNode facts domain 2 + 4) (entryAddress 1)]
      [CountIs 1, EntryIndexIs 1 1]
  , Case "insert-full" Insert (replicate 184 (100, 1)) 184 2 200 0 10 0 0x13 Receipt (Repair "the 184-node span is a hard capacity")
      []
      [CountIs 185, EntryIndexIs 2 185, KernelWord (heapNode facts domain 185) 200]
      [CountIs 184, EntryIndexIs 2 0, KernelWord (heapNode facts domain 185) 0]
  ]

checksFor :: Role -> Case -> [Check]
checksFor role one =
  [ RegisterIs R4 (seedRegister R4)
  , RegisterIs R5 (seedRegister R5)
  , RegisterIs R6 (seedRegister R6)
  , RegisterIs R7 (seedRegister R7)
  , RegisterIs R8 (seedRegister R8)
  , RegisterIs R9 (seedRegister R9)
  , RegisterIs R10 (seedRegister R10)
  , RegisterIs R11 (seedRegister R11)
  , RegisterIs Sp stackPointer
  ]
    ++ case caseKind one of
      Equal -> caseEqualChecks one
      Repair _ -> if role == Stock then caseStockChecks one else caseOwnedChecks one

check :: Facts -> Domain -> Case -> Role -> NativeObservation -> Check -> Either String ()
check facts domain _one _role observation check' = case check' of
  KernelWord address value -> expectWord "kernel" address value
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
  where
    readCounter = readWord "entries" counterBase
    expectWord region address value = do
      actual <- readWord region address
      unless (actual == value) (Left (region ++ " " ++ showHexWord address ++ " is " ++ showHexWord actual ++ ", expected " ++ showHexWord value))
    readWord region address = case lookup (RegionId region) (nativeMemory observation) of
      Nothing -> Left ("missing region " ++ region)
      Just bytes ->
        let base = regionBase region
         in if address < base || address + 4 > base + fromIntegral (B.length bytes)
              then Left ("address " ++ showHexWord address ++ " is outside " ++ region)
              else Right (readWordAt bytes (fromIntegral (address - base)))
    regionBase "kernel" = kernelLow facts domain
    regionBase "entries" = entriesBase
    regionBase _ = 0

showHexWord :: Word32 -> String
showHexWord value = "0x" ++ showHex' value
  where
    showHex' 0 = "0"
    showHex' n = reverse (go n)
    go 0 = []
    go n = let (q, r) = n `divMod` 16 in "0123456789abcdef" !! fromIntegral r : go q

regionDifferences :: Facts -> Domain -> [RegionId] -> NativeObservation -> NativeObservation -> [String]
regionDifferences _ _ regions stock owned =
  [ "region " ++ regionName ident ++ " differs"
  | ident <- regions
  , M.lookup ident (M.fromList (nativeMemory stock)) /= M.lookup ident (M.fromList (nativeMemory owned))
  ]

caseFor :: Facts -> Domain -> Role -> Image -> Image -> [(Word32, Word32)] -> Case -> CaseSpec
caseFor facts domain role stockImage ownedImage ownedSymbols one =
  CaseSpec
    { caseId = caseName one ++ "-" ++ domainLabel domain
    , caseProvenance = "X-M1 1.50 timer stock ABI, x70view 55436fc85c0560dd252b0410af6f2427fe198cd3; synthetic heap, entry and callback state"
    , caseExclusion = "Synthetic timer heap and callback stubs; no wait-result, cyclic or firmware integration."
    , caseEntry = GuestAddress (fromIntegral entry)
    , caseExit = GuestAddress (fromIntegral exitAddress)
    , caseRegions =
        [ mkRegion (RegionId "code") (GuestAddress (fromIntegral codeBase)) (fromIntegral (B.length codeBytes)) ReadExecute codeBytes
        , mkRegion (RegionId "stubs") (GuestAddress (fromIntegral stubBase)) (fromIntegral stubSize) ReadExecute (stubBytes facts domain)
        , mkRegion (RegionId "kernel") (GuestAddress (fromIntegral (kernelLow facts domain))) (fromIntegral (kernelSize facts domain)) ReadWrite (overlay (kernelLow facts domain) (kernelSize facts domain) (kernelWords facts domain one))
        , mkRegion (RegionId "entries") (GuestAddress (fromIntegral entriesBase)) (fromIntegral entriesSize) ReadWrite (overlay entriesBase entriesSize (entriesWords facts domain one))
        , mkRegion (RegionId "stack") (GuestAddress (fromIntegral stackBase)) (fromIntegral stackSize) ReadWrite (B.replicate (fromIntegral stackSize) 0)
        , mkRegion (RegionId "exit") (GuestAddress (fromIntegral exitAddress)) 0x10 ReadExecute (B.replicate 0x10 0)
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
      Insert -> insertEntry domain
      Remove -> removeEntry domain
      Tick -> tickEntry domain
    (codeBase, codeBytes) = case role of
      Stock -> let (base, size) = stockBase domain in (base, slice (imageBase stockImage) (imageBytes stockImage) base size)
      Owned -> (imageBase ownedImage, imageBytes ownedImage)
    functions = case role of
      Stock ->
        let (base, _) = stockBase domain
         in [(base, 0xc4), (base + 0xc4, 0xdc), (insertEntry domain, 0x48), (removeEntry domain, 0xe4), (tickEntry domain, 0x1a0), (stubBase, stubSize)]
      Owned -> ownedSymbols ++ [(stubBase, stubSize)]
    initialRegisters =
      [ (register, case lookup register arguments of
          Just value -> value
          Nothing -> seedRegister register)
      | register <- [R0 .. R12]
      ]
        ++ [(Sp, stackPointer), (Lr, exitAddress), (Cpsr, caseCpsr one)]
    arguments = case caseCall one of
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
      [a, z, _, _] -> case (parseWord32 a, parseWord32 z) of
        (Just address, Just size)
          | size > 0 && address >= 0x44c60 && address < stubBase -> [(address, size)]
        _ -> []
      _ -> []

runDomain :: FilePath -> FilePath -> FilePath -> Facts -> Domain -> Image -> Image -> [(Word32, Word32)] -> IO [CaseRun]
runDomain runner cacheRoot directory facts domain stockImage ownedImage ownedSymbols = do
  let selected = cases facts domain
      stockSpecs = map (caseFor facts domain Stock stockImage ownedImage ownedSymbols) selected
      ownedSpecs = map (caseFor facts domain Owned stockImage ownedImage ownedSymbols) selected
  stockBatch <- writeBatch (directory </> "stock") stockSpecs
  ownedBatch <- writeBatch (directory </> "owned") ownedSpecs
  (stockReuse, stockRaw) <- cachedObservedBatch cacheRoot runner stockBatch
  (ownedReuse, ownedRaw) <- cachedObservedBatch cacheRoot runner ownedBatch
  let reference = M.fromList [(caseIdText ident, observation) | (ident, observation) <- stockRaw]
      subject = M.fromList [(caseIdText ident, observation) | (ident, observation) <- ownedRaw]
  forM selected $ \one -> do
    let ident = caseName one ++ "-" ++ domainLabel domain
        stock = M.lookup ident reference
        owned = M.lookup ident subject
        roleChecks role observation = map (check facts domain one role observation) (checksFor role one)
        statusCheck observation = case nativeStatus observation of
          Returned -> Right ()
          status -> Left ("status " ++ runnerStatusLabel status)
        errorsFor role observation = [message | Left message <- statusCheck observation : roleChecks role observation]
        equalDifferences stock' owned' = case caseKind one of
          Equal -> regionDifferences facts domain [RegionId "kernel", RegionId "entries"] stock' owned'
          Repair _ -> []
    pure CaseRun
      { caseDomain = domain
      , runName = caseName one
      , runKind = caseKind one
      , runChecks = case (stock, owned) of
          (Just stock', Just owned') ->
            [ ("stock: " ++ describe check', either (const False) (const True) (check facts domain one Stock stock' check'))
            | check' <- checksFor Stock one
            ]
              ++ [ ("owned: " ++ describe check', either (const False) (const True) (check facts domain one Owned owned' check'))
                 | check' <- checksFor Owned one
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
