{-# LANGUAGE OverloadedStrings #-}

module Replacement.RtosIrqControl.Suite
  ( Image (..)
  , SuiteRun (..)
  , runSuite
  ) where

import Control.Monad (forM, unless)
import qualified Data.ByteString as B
import qualified Data.Map.Strict as M
import Data.Word (Word32)
import Emulation.CachedNativeBatch (BatchReuse, cachedObservedBatch)
import qualified Emulation.NativeObservation as N
import Emulation.NativeObservation hiding (GuestAddress)
import Emulation.OwnedCase
import Replacement.RtosOwnFirst.Regions (Domain (..), GuestAddress (..), domainLabel)
import System.FilePath ((</>))

data Image = Image { imageBase :: Word32, imageBytes :: B.ByteString }

data Service = Enable | Disable | Define deriving (Eq, Show)

data Packet = NoPacket | Packet Word32 Word32 Word32 Word32 Word32 deriving (Eq, Show)

data Kind = Match | Correction deriving (Eq, Show)

data Case = Case
  { caseName :: String
  , caseLine :: Word32
  , casePacket :: Packet
  , caseSlot :: Word32
  , caseTable :: Word32
  , caseIdentity :: Word32
  , caseCpsr :: Word32
  , caseEntryWords :: [Word32]
  , caseKind :: Kind
  , caseResult :: Word32
  }

data SuiteRun = SuiteRun
  { suiteDomain :: Domain
  , suiteCases :: [(String, Either String [String])]
  , suiteReuse :: (BatchReuse, BatchReuse)
  }

data Role = Stock | Owned deriving Eq

data Page = Page RegionId GuestAddress Int Permission B.ByteString

tableA, tableB, defaultA, defaultB :: Word32
tableA = 0x4005_38fc
tableB = 0x400e_38bc
defaultA = 0x0004_2840
defaultB = 0x000d_2840

enableA, disableA, defineA, enableB, disableB, defineB :: Word32
enableA = 0x41108
disableA = 0x41124
defineA = 0x4d8fc
enableB = 0xd1108
disableB = 0xd1124
defineB = 0xdd8c0

packetAddress, stackPointer, exitAddress, scratchAddress :: Word32
packetAddress = 0x0096_c000
stackPointer = 0x0095_9800
exitAddress = 0x0095_8f00
scratchAddress = 0x0005_0000

tableSize, scratchSize, stackSize, exitSize :: Int
tableSize = 0xc00
scratchSize = 0x2000
stackSize = 0x400
exitSize = 0x100

tableFor :: Domain -> Word32
tableFor DomainA = tableA
tableFor DomainB = tableB

defaultFor :: Domain -> Word32
defaultFor DomainA = defaultA
defaultFor DomainB = defaultB

entryFor :: Domain -> Service -> Word32
entryFor DomainA Enable = enableA
entryFor DomainA Disable = disableA
entryFor DomainA Define = defineA
entryFor DomainB Enable = enableB
entryFor DomainB Disable = disableB
entryFor DomainB Define = defineB

identityFor :: Domain -> Word32
identityFor DomainA = 0x4003
identityFor DomainB = 0x4004

validEntry :: Word32
validEntry = 0x1234_5678

entryWordsValid :: [Word32]
entryWordsValid = [validEntry, 1, 3, 1, 2]

cases :: Domain -> [Case]
cases domain =
  [ match "enable-empty-handler" Enable 5 NoPacket 0x13 entryWordsEmpty 0xffff_fffb
  , match "enable-line-128" Enable 128 NoPacket 0x13 entryWordsValid 0xffff_fffb
  , match "enable-masked-entry" Enable 5 NoPacket 0x93 entryWordsEmpty 0xffff_fffb
  , match "disable-line-128" Disable 128 NoPacket 0x13 entryWordsValid 0xffff_fffb
  , match "disable-masked-entry" Disable 128 NoPacket 0xd3 entryWordsValid 0xffff_fffb
  , match "define-handler" Define 5 (Packet 0xdead_beef validEntry 3 1 2) 0x13 entryWordsValid 0
  , match "define-null" Define 5 NoPacket 0x13 entryWordsValid 0
  , match "define-null-line-128" Define 128 NoPacket 0x13 entryWordsValid 0
  , match "define-null-line-ffff" Define 0xffff NoPacket 0x13 entryWordsValid 0
  , match "define-line-10000" Define 0x1_0000 (Packet 0xdead_beef validEntry 3 1 2) 0x13 entryWordsValid 0
  , match "define-line-ffff" Define 0xffff (Packet 0xdead_beef validEntry 3 1 2) 0x13 entryWordsValid 0xffff_ffef
  , match "define-zero-handler" Define 5 (Packet 0xdead_beef 0 3 1 2) 0x13 entryWordsValid 0xffff_ffef
  , match "define-priority-16" Define 5 (Packet 0xdead_beef validEntry 16 1 2) 0x13 entryWordsValid 0xffff_ffef
  , match "define-flag-2" Define 5 (Packet 0xdead_beef validEntry 3 2 2) 0x13 entryWordsValid 0xffff_ffef
  , match "define-mode-4" Define 5 (Packet 0xdead_beef validEntry 3 1 4) 0x13 entryWordsValid 0xffff_ffef
  , match "define-masked-entry" Define 5 (Packet 0xdead_beef validEntry 3 1 2) 0xd3 entryWordsValid 0
  , correction "enable-negative" Enable 0xffff_ffff NoPacket 0x13 entryWordsValid 0xffff_fffb
  , correction "disable-negative" Disable 0xffff_ffff NoPacket 0x13 entryWordsValid 0xffff_fffb
  , correction "define-negative-null" Define 0xffff_ffff NoPacket 0x13 entryWordsValid 0
  , correction "define-negative-packet" Define 0xffff_ffff (Packet 0xdead_beef validEntry 3 1 2) 0x13 entryWordsValid 0xffff_ffef
  ]
  where
    match name service line packet cpsr words' result =
      Case name line packet (entryFor domain service) (tableFor domain) (identityFor domain) cpsr words' Match result
    correction name service line packet cpsr words' result =
      Case name line packet (entryFor domain service) (tableFor domain) (identityFor domain) cpsr words' Correction result
    entryWordsEmpty = [defaultFor domain, 0, 15, 0, 1]

stockFunctions :: Domain -> [(Word32, Word32)]
stockFunctions DomainA =
  [ (0x41108, 0x1c), (0x41124, 0x1c), (0x4d8fc, 0x7c)
  , (0x42b00, 0x3b0), (0x42eb0, 0x1f8), (0x429ac, 0xec), (0x42a98, 0x44)
  , (0x42854, 0x18), (0x427b8, 0x1c)
  , (0x42560, 0x8), (0x42568, 0xc), (0x42574, 0x8), (0x4257c, 0xc)
  , (0x4259c, 0x14), (0x425b0, 0x18)
  ]
stockFunctions DomainB =
  [ (0xd1108, 0x1c), (0xd1124, 0x1c), (0xdd8c0, 0x7c)
  , (0xd2b00, 0x3b0), (0xd2eb0, 0x1f8), (0xd29ac, 0xec), (0xd2a98, 0x44)
  , (0xd2854, 0x18), (0xd27b8, 0x1c)
  , (0xd2560, 0x8), (0xd2568, 0xc), (0xd2574, 0x8), (0xd257c, 0xc)
  , (0xd259c, 0x14), (0xd25b0, 0x18)
  ]

bodyFunctions :: [(Word32, Word32)] -> [(GuestAddress, Word32)]
bodyFunctions symbols =
  [ (GuestAddress address, size)
  | (address, size) <- M.toList (M.fromListWith max [(address, size) | (address, size) <- symbols, address >= scratchAddress, address < scratchAddress + fromIntegral scratchSize])
  ]

wireAddress :: GuestAddress -> N.GuestAddress
wireAddress (GuestAddress address) = N.GuestAddress (fromIntegral address)

slice :: Image -> Word32 -> Word32 -> B.ByteString
slice (Image base bytes) address size
  | address < base || toInteger address + toInteger size > toInteger base + toInteger (B.length bytes) = error "IRQ image slice is outside its image"
  | otherwise = B.take (fromIntegral size) (B.drop (fromIntegral (address - base)) bytes)

patch :: B.ByteString -> Word32 -> [(Word32, B.ByteString)] -> B.ByteString
patch bytes base = foldl install bytes
  where
    install current (address, replacement) =
      let offset = fromIntegral (address - base)
       in B.take offset current <> replacement <> B.drop (offset + B.length replacement) current

replaceWord :: Word32 -> B.ByteString -> Word32 -> Word32 -> B.ByteString
replaceWord base bytes address value =
  let offset = fromIntegral (address - base)
   in B.take offset bytes <> word32le value <> B.drop (offset + 4) bytes

overlay :: Word32 -> B.ByteString -> [(Word32, Word32)] -> B.ByteString
overlay base bytes words' = foldl (\current (address, value) -> replaceWord base current address value) bytes words'

tableWords :: Case -> [(Word32, Word32)]
tableWords one =
  [ (caseTable one + 5 * 0x18 + fromIntegral (4 * index), value)
  | (index, value) <- zip [0 :: Int ..] (caseEntryWords one)
  ]

packetWords :: Case -> [(Word32, Word32)]
packetWords one = case casePacket one of
  NoPacket -> []
  Packet ignored handler priority flag mode ->
    [ (packetAddress, ignored)
    , (packetAddress + 0x04, handler)
    , (packetAddress + 0x08, priority)
    , (packetAddress + 0x0c, flag)
    , (packetAddress + 0x10, mode)
    ]

pages :: Case -> [Page]
pages one =
  [ Page (RegionId "table") (GuestAddress (caseTable one)) tableSize ReadWrite (overlay (caseTable one) (B.replicate tableSize 0) (tableWords one))
  , Page (RegionId "packet") (GuestAddress packetAddress) 0x20 ReadWrite (overlay packetAddress (B.replicate 0x20 0) (packetWords one))
  , Page (RegionId "stack") (GuestAddress (stackPointer - 0x400)) stackSize ReadWrite (B.replicate stackSize 0)
  , Page (RegionId "exit") (GuestAddress exitAddress) exitSize ReadExecute (B.replicate exitSize 0)
  ]

codeBase :: Domain -> Word32
codeBase domain = minimum (map fst (stockFunctions domain))

codeTop :: Domain -> Word32
codeTop domain = maximum [address + size | (address, size) <- stockFunctions domain]

codeBytes :: Domain -> Role -> Image -> Image -> Case -> B.ByteString
codeBytes domain role loader owned one =
  let base = codeBase domain
      stock = slice loader base (codeTop domain - base)
   in case role of
        Stock -> stock
        Owned -> patch stock base [(caseSlot one, slice owned (caseSlot one) 4)]

caseFor :: Domain -> Role -> Image -> Image -> [(Word32, Word32)] -> Case -> CaseSpec
caseFor domain role loader owned symbols one =
  CaseSpec
    { caseId = caseName one ++ "-" ++ domainLabel domain
    , caseProvenance = "X-M1 1.50 IRQ root slots +0x158/+0x15c/+0x160; pinned decomp 55436fc85c and loader disassembly; scratch body at 0x50000"
    , caseExclusion = "Stack frame bytes and caller-saved scratch are excluded. The controller-bank success paths are outside the native device allowlist and are not run natively. The scratch body at 0x50000 is not a live placement."
    , caseEntry = wireAddress (GuestAddress (caseSlot one))
    , caseExit = wireAddress (GuestAddress exitAddress)
    , caseRegions =
        [ mkRegion (RegionId "code") (wireAddress (GuestAddress (codeBase domain))) (fromIntegral (codeTop domain - codeBase domain)) ReadExecute (codeBytes domain role loader owned one)
        , mkRegion (RegionId "scratch") (wireAddress (GuestAddress scratchAddress)) scratchSize ReadExecute (slice owned scratchAddress (fromIntegral scratchSize))
        ]
          ++ [mkRegion ident (wireAddress address) size permission bytes | Page ident address size permission bytes <- pages one]
    , caseFunctions = [(wireAddress (GuestAddress address), size) | (address, size) <- stockFunctions domain] ++ [(wireAddress address, size) | (address, size) <- bodyFunctions symbols]
    , caseInitial = initialRegisters one
    , caseExpectedStatus = Returned
    , caseExpectedRegisters = [(R0, caseResult one)]
    , caseExpectedMemory = []
    , caseIgnoredMemory = []
    , caseCp14Identity = Just (caseIdentity one)
    , caseKernel = Just (KernelCase (InstructionLimit 2500) Nothing Nothing Nothing Nothing [R1, R2, R3, R12, Lr] [])
    }

seed :: Register -> Word32
seed register = 0xa5a5_0000 + fromIntegral (fromEnum register)

initialRegisters :: Case -> [(Register, Word32)]
initialRegisters one =
  [ (R0, caseLine one)
  , (R1, case casePacket one of NoPacket -> 0; Packet {} -> packetAddress)
  , (Sp, stackPointer)
  , (Lr, exitAddress)
  , (Cpsr, caseCpsr one)
  ]
    ++ [(register, seed register) | register <- [R2, R3] ++ [R4 .. R12]]

filteredWrites :: Case -> NativeObservation -> Either String [(Word32, Word32)]
filteredWrites one observation = case N.nativeKernel observation >>= N.observedOrder of
  Nothing -> Left "missing ordered provenance"
  Just events ->
    Right
      [ (address32, fromIntegral value)
      | OrderedWrite (Write (N.GuestAddress address) _) (WriteValue value) <- events
      , let address32 = fromIntegral address :: Word32
      , let base = caseTable one
      , base <= address32
      , address32 + 4 <= base + fromIntegral tableSize
      ]

compareCase :: Case -> CaseSpec -> NativeObservation -> NativeObservation -> [String]
compareCase one spec stock owned =
  ["stock did not return" | caseOutcome spec stock /= OutcomeReturned]
    ++ ["owned did not return" | caseOutcome spec owned /= OutcomeReturned]
    ++ ["R0 differs" | N.lookupRegister R0 stock /= N.lookupRegister R0 owned]
    ++ ["callee-saved register " ++ show register ++ " differs" | register <- [R4 .. R11], N.lookupRegister register stock /= N.lookupRegister register owned]
    ++ ["SP differs" | N.lookupRegister Sp stock /= N.lookupRegister Sp owned]
    ++ ["CPSR control differs" | fmap cpsrControl (N.lookupRegister Cpsr stock) /= fmap cpsrControl (N.lookupRegister Cpsr owned)]
    ++ ["table differs" | lookup (RegionId "table") (N.nativeMemory stock) /= lookup (RegionId "table") (N.nativeMemory owned)]
    ++ ["ordered table writes differ" | filteredWrites one stock /= filteredWrites one owned]

correctionChecks :: Case -> CaseSpec -> NativeObservation -> [String]
correctionChecks one spec owned =
  ["owned did not return" | caseOutcome spec owned /= OutcomeReturned]
    ++ ["R0 differs" | N.lookupRegister R0 owned /= Just (caseResult one)]
    ++ ["correction wrote the table" | filteredWrites one owned /= Right []]

runSuite :: FilePath -> FilePath -> Domain -> Image -> Image -> [(Word32, Word32)] -> IO SuiteRun
runSuite runner cacheRoot domain loader owned symbols = do
  let selected = cases domain
      matches = [one | one <- selected, caseKind one == Match]
      stockSpecs = [caseFor domain Stock loader owned symbols one | one <- matches]
      ownedSpecs = [caseFor domain Owned loader owned symbols one | one <- selected]
  stockBatch <- writeBatch (cacheRoot </> "stock") stockSpecs
  ownedBatch <- writeBatch (cacheRoot </> "owned") ownedSpecs
  (stockReuse, stockRaw) <- cachedObservedBatch cacheRoot runner stockBatch
  (ownedReuse, ownedRaw) <- cachedObservedBatch cacheRoot runner ownedBatch
  let reference = M.fromList [(caseIdText ident, observation) | (ident, observation) <- stockRaw]
      subject = M.fromList [(caseIdText ident, observation) | (ident, observation) <- ownedRaw]
  results <- forM selected $ \one -> do
    let spec = caseFor domain Owned loader owned symbols one
        ident = caseId spec
    pure
      ( caseName one
      , case (caseKind one, M.lookup ident subject) of
          (Match, Just ownedObservation) -> case M.lookup ident reference of
            Just stockObservation -> Right (compareCase one spec stockObservation ownedObservation)
            Nothing -> Left "stock case did not run"
          (Match, Nothing) -> Left "owned case did not run"
          (Correction, Just ownedObservation) -> Right (correctionChecks one spec ownedObservation)
          (Correction, Nothing) -> Left "owned case did not run"
      )
  unless (length stockRaw == length matches && length ownedRaw == length selected) (fail "native observations do not cover the current IRQ case set")
  pure (SuiteRun domain results (stockReuse, ownedReuse))
