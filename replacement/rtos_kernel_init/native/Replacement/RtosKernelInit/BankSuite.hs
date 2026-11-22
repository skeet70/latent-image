{-# LANGUAGE OverloadedStrings #-}

-- | Stock against the fixed-bank kernel startup bodies on the native runner. The owned role
-- runs the loader with the probe carrier pieces written over it and the bank body mapped; every
-- comparison and every mutant uses 'compareCase'.
module Replacement.RtosKernelInit.BankSuite
  ( Image (..)
  , Owned (..)
  , Mutant (..)
  , SuiteRun (..)
  , runBankSuite
  ) where

import Control.Monad (forM, unless)
import qualified Data.ByteString as B
import Data.Bits (shiftL, (.&.), (.|.))
import qualified Data.Map.Strict as M
import Data.Word (Word32)
import Emulation.CachedNativeBatch (BatchReuse, cachedObservedBatch)
import qualified Emulation.NativeObservation as N
import Emulation.NativeObservation hiding (GuestAddress)
import Emulation.OwnedCase
import Replacement.RtosOwnFirst.Regions (Domain (..), domainLabel)
import System.FilePath ((</>))

data Image = Image {imageBase :: Word32, imageBytes :: B.ByteString}

-- | The owned role of one domain: carrier pieces written over the loader, and the bank body.
data Owned = Owned {ownedCarrier :: [Image], ownedBank :: Image}

-- | One instruction substituted in the bank body for one case; the comparison must fail.
data Mutant = Mutant
  { mutantName :: String
  , mutantCase :: String
  , mutantAddress :: Word32
  , mutantExpected :: Word32
  , mutantWord :: Word32
  }

data SuiteRun = SuiteRun
  { suiteDomain :: Domain
  , suiteCases :: [(String, Either String [String])]
  , suiteMutants :: [(String, Either String [String])]
  , suiteReuse :: (BatchReuse, BatchReuse, BatchReuse)
  }

data Role = Stock | OwnedRole deriving (Eq)

data Entry
  = Publish
  | Start
  | TickTimer
  | ConfigureCore
  | OwnerContexts
  | InitializeTasks
  | Autostart
  | Semaphores
  | EventFlags
  | DataQueues
  | Mailboxes
  | FixedPools
  | Objects

-- | A stop is a retained routine's entry; 'StopsWithArgument' also compares the R0 it receives.
data Ending = Returns | StopsAt Retained | StopsWithArgument Retained

data Retained = Rendezvous | Dispatch

-- | 'FinalState' cases log more accesses than the runner retains, so their write order and
-- platform order are not observed; every other part of the comparison is the same.
data Check = OrderedLog | FinalState deriving (Eq)

-- | Words are A addresses; 'ramAt' maps them. 'caseWords' override the pattern.
data Case = Case
  { caseName :: String
  , caseAt :: Entry
  , caseIdentity :: Word32
  , caseEnding :: Ending
  , caseCheck :: Check
  , caseWords :: [(Word32, Word32)]
  , caseTimerCount :: Word32
  }

entryAddress :: Domain -> Entry -> Word32
entryAddress domain entry = case (domain, entry) of
  (DomainA, Publish) -> 0x40430
  (DomainA, Start) -> 0x409ac
  (DomainA, TickTimer) -> 0x40b5c
  (DomainA, ConfigureCore) -> 0x43a58
  (DomainA, OwnerContexts) -> 0x43ac0
  (DomainA, InitializeTasks) -> 0x43e18
  (DomainA, Autostart) -> 0x43f9c
  (DomainA, Semaphores) -> 0x47580
  (DomainA, EventFlags) -> 0x48044
  (DomainA, DataQueues) -> 0x48e38
  (DomainA, Mailboxes) -> 0x4a9bc
  (DomainA, FixedPools) -> 0x4b38c
  (DomainA, Objects) -> 0x4cea0
  (DomainB, Publish) -> 0xd0430
  (DomainB, Start) -> 0xd09ac
  (DomainB, TickTimer) -> 0xd0b5c
  (DomainB, ConfigureCore) -> 0xd3a0c
  (DomainB, OwnerContexts) -> 0xd3a7c
  (DomainB, InitializeTasks) -> 0xd3dd4
  (DomainB, Autostart) -> 0xd3f58
  (DomainB, Semaphores) -> 0xd7540
  (DomainB, EventFlags) -> 0xd8004
  (DomainB, DataQueues) -> 0xd8df8
  (DomainB, Mailboxes) -> 0xda97c
  (DomainB, FixedPools) -> 0xdb34c
  (DomainB, Objects) -> 0xdce60

retainedAddress :: Domain -> Retained -> Word32
retainedAddress DomainA Rendezvous = 0x43934
retainedAddress DomainB Rendezvous = 0xd3934
retainedAddress DomainA Dispatch = 0x43688
retainedAddress DomainB Dispatch = 0xd3688

-- | `0x424f8`: reads and writes CPACR, which the runner refuses; `bx lr` replaces its first word in both roles.
vfpEnable :: Domain -> Word32
vfpEnable DomainA = 0x424f8
vfpEnable DomainB = 0xd24f8

-- | `g_vector_slots[0]`, the word `kernel_patch_vectors` stores through for a zeroed patch record.
vectorSlot :: Domain -> Word32
vectorSlot DomainA = 0x403a0
vectorSlot DomainB = 0xd03a0

ramAt :: Domain -> Word32 -> Word32
ramAt DomainA address = address
ramAt DomainB address = address + 0x8ffc0

kernelBase, kernelSize, sharedBase, sharedSize, timerBase, deviceBase, stackPointer, exitAddress, phaseCode, phaseLog :: Word32
kernelBase = 0x4004_d000
kernelSize = 0x3_7000
sharedBase = 0x400a_0000
sharedSize = 0x2_1000
timerBase = 0xfff9_1000
deviceBase = 0xfffb_3000
stackPointer = 0x0095_9800
exitAddress = 0x0095_8f00
phaseCode = 0x0095_8e00
phaseLog = 0x0096_c000

stackSize, exitSize, deviceSize, phaseLogSize :: Int
stackSize = 0x800
exitSize = 0x100
deviceSize = 0x2000
phaseLogSize = 0x40

primary, secondary :: Word32
primary = 0x4003
secondary = 0x4004

svcMasked :: Word32
svcMasked = 0xd3

-- | Records its R0 at the cursor word of the phase log and returns `0x5100 + R0`.
phaseRoutine :: [Word32]
phaseRoutine = [0xe59f_1010, 0xe591_2000, 0xe482_0004, 0xe581_2000, 0xe280_0c51, 0xe12f_ff1e, phaseLog]

coreSyncMagic :: Word32
coreSyncMagic = 0x8765_4321

-- | The executing loader's own config record and context.
ownConfig, ownContext :: Domain -> Word32
ownConfig DomainA = 0x400c_0818
ownConfig DomainB = 0x400c_0888
ownContext DomainA = 0x4005_45cc
ownContext DomainB = 0x400e_4610

configPointer :: Domain -> Word32
configPointer domain = ramAt domain 0x4005_46d4

base :: String -> Entry -> Case
base name entry = Case name entry primary Returns OrderedLog [] 0

configured :: Domain -> Case -> Case
configured domain one = one {caseWords = (configPointer domain, ownConfig domain) : caseWords one}

cases :: Domain -> [Case]
cases domain =
  [ base "publish" Publish
  , (base "start-primary" Start) {caseEnding = StopsAt Rendezvous, caseCheck = FinalState}
  , (base "start-peer" Start) {caseIdentity = secondary, caseEnding = StopsWithArgument Dispatch, caseCheck = FinalState, caseWords = peerReleased}
  , base "configure-core" ConfigureCore
  , base "owner-contexts" OwnerContexts
  , configured domain (base "initialize-tasks" InitializeTasks)
  , autostart domain
  , configured domain (base "semaphores" Semaphores)
  , configured domain (base "event-flags" EventFlags)
  , configured domain (base "data-queues" DataQueues)
  , configured domain (base "mailboxes" Mailboxes)
  , configured domain (base "fixed-pools" FixedPools)
  , (configured domain (base "initialize-objects" Objects)) {caseCheck = FinalState}
  , (base "tick-timer-peer" TickTimer) {caseIdentity = secondary, caseTimerCount = 1234}
  ]
    ++ [ (base ("tick-timer-primary-" ++ hexWord32 count) TickTimer) {caseTimerCount = count}
       | count <- [0, 1, 1874, 1875, 123456789, 0xffff_ffff]
       ]
  where
    -- Core 0 has released core 1 and published the core-sync magic; both rendezvous forms pass.
    peerReleased = [(0x400a_00c4, 1), (0x400c_0808, coreSyncMagic), (0x400c_080c, coreSyncMagic), (0x400c_0814, 1)]

-- | 120 slot ids in a stride-7 order; every fifth slot's definition has the autostart bit.
autostart :: Domain -> Case
autostart domain = (base "autostart" Autostart) {caseWords = context ++ heads ++ slots}
  where
    w = ramAt domain
    contextAt = ownContext domain
    readyHeads = w 0x4005_46dc
    context =
      [ (w 0x4005_45c8, contextAt)
      , (contextAt + 0x20, readyHeads)
      , (contextAt + 0x24, w 0x4005_475c)
      , (contextAt + 0x2c, w 0x4005_4768)
      , (contextAt + 0x30, w 0x4005_4764)
      , (contextAt + 0x7c, if domain == DomainA then 0x43728 else 0xd3728)
      , (w 0x4005_475c, 0)
      , (w 0x4005_4768, 0)
      , (w 0x4005_4764, 1)
      ]
    heads = concat [[(readyHeads + 8 * p, readyHeads + 8 * p), (readyHeads + 8 * p + 4, readyHeads + 8 * p)] | p <- [0 .. 15]]
    slots =
      concat
        [ [ (w 0x4006_22d4 + 4 * index, slot)
          , (task + 8, definition)
          , (task + 0xc, (slot .&. 15) `shiftL` 8)
          , (definition, if slot `mod` 5 == 0 then 3 else 1)
          , (definition + 0x10, 0x100)
          , (definition + 0x14, w 0x4007_0000 + slot * 0x100)
          ]
        | index <- [0 .. 119]
        , let slot = (index * 7) `mod` 120 + 1
              task = w 0x4005_ff0c + (slot - 1) * 0x38
              definition = w 0x4005_beac + (slot - 1) * 0x28
        ]

-- | A distinct word everywhere, with bit 1 clear: stock `cyclic_initialize` schedules a cyclic whose
-- attribute word has it set.
pattern :: Word32 -> Word32
pattern address = 0x5a5a_0000 .|. (address .&. 0xfffc)

patterned :: Word32 -> Int -> [(Word32, Word32)] -> B.ByteString
patterned start size words' = word32Bytes ([(a, pattern a) | a <- [start, start + 4 .. start + fromIntegral size - 4]] ++ words') start size

word32Bytes :: [(Word32, Word32)] -> Word32 -> Int -> B.ByteString
word32Bytes words' start size = B.pack (concatMap bytes [0 .. fromIntegral size `div` 4 - 1])
  where
    table = M.fromList [(address - start, value) | (address, value) <- words', address >= start, toInteger address - toInteger start + 4 <= toInteger size]
    bytes index = B.unpack (word32le (M.findWithDefault 0 (4 * index) table))

patchImage :: Image -> Image -> Image
patchImage (Image target bytes) (Image start piece)
  | start < target || toInteger start + toInteger (B.length piece) > toInteger target + toInteger (B.length bytes) = error "carrier piece is outside the loader"
  | otherwise =
      let offset = fromIntegral (start - target)
       in Image target (B.take offset bytes <> piece <> B.drop (offset + B.length piece) bytes)

mutate :: Mutant -> Image -> Either String Image
mutate mutant (Image start bytes)
  | address < start || toInteger address + 4 > toInteger start + toInteger (B.length bytes) = Left "mutant site is outside the bank body"
  | readWordAt bytes offset /= mutantExpected mutant = Left ("mutant site holds " ++ hexWord32 (readWordAt bytes offset) ++ ", not " ++ hexWord32 (mutantExpected mutant))
  | otherwise = Right (Image start (B.take offset bytes <> word32le (mutantWord mutant) <> B.drop (offset + 4) bytes))
  where
    address = mutantAddress mutant
    offset = fromIntegral (address - start)

wire :: Word32 -> N.GuestAddress
wire address = N.GuestAddress (fromIntegral address)

region :: String -> Word32 -> Int -> Permission -> B.ByteString -> CaseRegion
region name start size permission = mkRegion (RegionId name) (wire start) size permission

-- | The writable regions every comparison reads back, in order.
comparedRegions :: [String]
comparedRegions = ["kernel", "shared", "timer", "controller", "vector-slot", "phase-log"]

slice :: Image -> Word32 -> Word32 -> Image
slice (Image start bytes) from to = Image from (B.take (fromIntegral (to - from)) (B.drop (fromIntegral (from - start)) bytes))

caseSpec :: Domain -> Role -> Image -> Owned -> Maybe Image -> String -> Case -> CaseSpec
caseSpec domain role loader owned bank suffix one =
  CaseSpec
    { caseId = caseName one ++ suffix ++ "-" ++ domainLabel domain
    , caseProvenance = "X-M1 1.50 kernel startup, x70view 55436fc85c0560dd252b0410af6f2427fe198cd3 and loader disassembly; synthetic kernel, shared, timer and controller state"
    , caseExclusion = "Stack bytes, R0-R3, R12 and LR, NZCV, and at a stop R4-R11 and SP, are excluded. The VFP enable's first word is `bx lr` in both roles. Controller and timer words are memory-backed, not device models."
    , caseEntry = wire (entryAddress domain (caseAt one))
    , caseExit = wire exitAddress
    , caseRegions =
        [ region "code" (imageBase code) (fromIntegral (slot - imageBase code)) ReadExecute (imageBytes (slice code (imageBase code) slot))
        , region "vector-slot" slot 4 ReadWrite (imageBytes (slice code slot (slot + 4)))
        , region "code-tail" (slot + 4) (fromIntegral (codeEnd - slot - 4)) ReadExecute (imageBytes (slice code (slot + 4) codeEnd))
        , region "kernel" kernelAt (fromIntegral kernelSize) ReadWrite (patterned kernelAt (fromIntegral kernelSize) (caseWords one))
        , region "shared" sharedBase (fromIntegral sharedSize) ReadWrite (patterned sharedBase (fromIntegral sharedSize) (caseWords one))
        , region "timer" timerBase 0x10 ReadWrite (word32Bytes [(timerBase, caseTimerCount one), (timerBase + 4, 0x7530), (timerBase + 8, 0x23)] timerBase 0x10)
        , region "controller" deviceBase deviceSize Device (patterned deviceBase deviceSize [])
        , region "stack" (stackPointer - fromIntegral stackSize) stackSize ReadWrite (B.replicate stackSize 0)
        , region "exit" exitAddress exitSize ReadExecute (B.replicate exitSize 0)
        , region "phase" phaseCode 0x20 ReadExecute (word32Bytes (zip [phaseCode, phaseCode + 4 ..] phaseRoutine) phaseCode 0x20)
        , region "phase-log" phaseLog phaseLogSize ReadWrite (word32Bytes [(phaseLog, phaseLog + 4)] phaseLog phaseLogSize)
        ]
          ++ [region "bank" (imageBase body) (B.length (imageBytes body)) ReadExecute (imageBytes body) | Just body <- [bankFor role]]
    , caseFunctions =
        [(wire (imageBase code), slot - imageBase code), (wire (slot + 4), codeEnd - slot - 4), (wire phaseCode, 0x20)]
          ++ [(wire (imageBase body), fromIntegral (B.length (imageBytes body))) | Just body <- [bankFor role]]
    , caseInitial =
        [(Sp, stackPointer), (Lr, exitAddress), (Cpsr, svcMasked)]
          ++ [(register, if register == R0 then phaseCode else seed register) | register <- [R0 .. R12]]
    , caseExpectedStatus = case caseEnding one of
        Returns -> Returned
        _ -> StopAddress
    , caseExpectedRegisters = case caseEnding one of
        Returns -> (Sp, stackPointer) : [(register, seed register) | register <- [R4 .. R11]]
        _ -> []
    , caseExpectedMemory = []
    , caseIgnoredMemory = []
    , caseCp14Identity = Just (caseIdentity one)
    , caseKernel = Just (KernelCase (InstructionLimit 6000000) (wire . retainedAddress domain <$> stopOf (caseEnding one)) Nothing Nothing Nothing [R0, R1, R2, R3, R12, Lr] [] Nothing)
    }
  where
    kernelAt = ramAt domain kernelBase
    slot = vectorSlot domain
    stub = Image (vfpEnable domain) (word32le 0xe12f_ff1e)
    code = case role of
      Stock -> patchImage loader stub
      OwnedRole -> foldl patchImage loader (ownedCarrier owned ++ [stub])
    codeEnd = imageBase code + fromIntegral (B.length (imageBytes code))
    bankFor Stock = Nothing
    bankFor OwnedRole = Just (maybe (ownedBank owned) id bank)

stopOf :: Ending -> Maybe Retained
stopOf Returns = Nothing
stopOf (StopsAt retained) = Just retained
stopOf (StopsWithArgument retained) = Just retained

seed :: Register -> Word32
seed register = 0xa5a5_0000 + fromIntegral (fromEnum register)

inCompared :: Domain -> Word32 -> Bool
inCompared domain address =
  within (ramAt domain kernelBase) kernelSize
    || within sharedBase sharedSize
    || within timerBase 0x10
    || within (vectorSlot domain) 4
    || within phaseLog (fromIntegral phaseLogSize)
  where
    within :: Word32 -> Word32 -> Bool
    within start size = address >= start && toInteger address < toInteger start + toInteger size

-- | Every write into the compared regions, with its width and value, in execution order.
comparedWrites :: Domain -> NativeObservation -> Maybe [(Write, WriteValue)]
comparedWrites domain observation = do
  kernel <- nativeKernel observation
  order <- observedOrder kernel
  pure [(write, value) | OrderedWrite write@(Write (N.GuestAddress wide) _) value <- order, inCompared domain (fromIntegral wide)]

compareCase :: Domain -> Case -> CaseSpec -> NativeObservation -> NativeObservation -> [String]
compareCase domain one spec stock owned =
  ["stock ended " ++ show (caseOutcome spec stock) | caseOutcome spec stock /= expected]
    ++ ["owned ended " ++ show (caseOutcome spec owned) | caseOutcome spec owned /= expected]
    ++ registers
    ++ ["CPSR control differs" | fmap cpsrControl (N.lookupRegister Cpsr stock) /= fmap cpsrControl (N.lookupRegister Cpsr owned)]
    ++ [name ++ " bytes differ" | name <- comparedRegions, lookup (RegionId name) (nativeMemory stock) /= lookup (RegionId name) (nativeMemory owned)]
    ++ case caseCheck one of
      FinalState -> []
      OrderedLog ->
        ["ordered writes differ" | comparedWrites domain stock /= comparedWrites domain owned]
          ++ ["ordered writes truncated" | comparedWrites domain stock == Nothing || comparedWrites domain owned == Nothing]
          ++ ["ordered device operations and barriers differ" | platformOrder spec stock /= platformOrder spec owned]
  where
    expected = case stopOf (caseEnding one) of
      Nothing -> OutcomeReturned
      Just retained -> OutcomeStopped (wire (retainedAddress domain retained))
    registers = case caseEnding one of
      Returns ->
        ["callee-saved " ++ show register ++ " differs" | register <- [R4 .. R11], N.lookupRegister register stock /= N.lookupRegister register owned]
          ++ ["SP differs" | N.lookupRegister Sp stock /= N.lookupRegister Sp owned]
      StopsAt _ -> []
      StopsWithArgument _ -> ["R0 at the stop differs" | N.lookupRegister R0 stock /= N.lookupRegister R0 owned]

runBankSuite :: FilePath -> FilePath -> FilePath -> Domain -> Image -> Owned -> [Mutant] -> IO SuiteRun
runBankSuite runner cacheRoot batchRoot domain loader owned mutants = do
  let selected = cases domain
      byName = M.fromList [(caseName one, one) | one <- selected]
      stockSpecs = map (caseSpec domain Stock loader owned Nothing "") selected
      ownedSpecs = map (caseSpec domain OwnedRole loader owned Nothing "") selected
      planned =
        [ (mutantName mutant, mutate mutant (ownedBank owned) >>= \bank -> maybe (Left "mutant case is missing") (\one -> Right (one, caseSpec domain OwnedRole loader owned (Just bank) ("-mutant-" ++ mutantName mutant) one)) (M.lookup (mutantCase mutant) byName))
        | mutant <- mutants
        ]
      mutantSpecs = [spec | (_, Right (_, spec)) <- planned]
      directory = batchRoot </> domainLabel domain
  unless (length (M.keys byName) == length selected) (fail "duplicate kernel startup case name")
  stockBatch <- writeBatch (directory </> "stock") stockSpecs
  ownedBatch <- writeBatch (directory </> "owned") ownedSpecs
  (stockReuse, stockRaw) <- cachedObservedBatch cacheRoot runner stockBatch
  (ownedReuse, ownedRaw) <- cachedObservedBatch cacheRoot runner ownedBatch
  (mutantReuse, mutantRaw) <-
    if null mutantSpecs
      then pure (ownedReuse, [])
      else writeBatch (directory </> "mutants") mutantSpecs >>= cachedObservedBatch cacheRoot runner
  let reference = M.fromList [(caseIdText ident, observation) | (ident, observation) <- stockRaw]
      subject = M.fromList [(caseIdText ident, observation) | (ident, observation) <- ownedRaw ++ mutantRaw]
      judge one spec stockId = case (M.lookup stockId reference, M.lookup (caseId spec) subject) of
        (Just stockObservation, Just ownedObservation) -> Right (compareCase domain one spec stockObservation ownedObservation)
        (Nothing, _) -> Left "stock case did not run"
        (_, Nothing) -> Left "owned case did not run"
  results <- forM (zip selected stockSpecs) $ \(one, stockSpec) ->
    pure (caseName one, judge one (caseSpec domain OwnedRole loader owned Nothing "" one) (caseId stockSpec))
  let mutantResults =
        [ ( name
          , case plan of
              Left problem -> Left problem
              Right (one, spec) -> case judge one spec (caseId (caseSpec domain Stock loader owned Nothing "" one)) of
                Left problem -> Left problem
                Right [] -> Left "mutant was not detected"
                Right differences -> Right differences
          )
        | (name, plan) <- planned
        ]
  unless (length stockRaw == length selected && length ownedRaw == length selected && length mutantRaw == length mutantSpecs) (fail "native observations do not cover the kernel startup case set")
  pure (SuiteRun domain results mutantResults (stockReuse, ownedReuse, mutantReuse))
