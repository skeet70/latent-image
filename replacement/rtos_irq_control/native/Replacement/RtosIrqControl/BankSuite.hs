{-# LANGUAGE OverloadedStrings #-}

-- | Stock against the fixed-bank IRQ, mask and cache bodies on the native runner. The owned role
-- runs the loader with the probe carrier patched in and the bank body mapped; every comparison and
-- every mutant uses 'compareCase'.
module Replacement.RtosIrqControl.BankSuite
  ( Image (..)
  , Owned (..)
  , Mutant (..)
  , SuiteRun (..)
  , runBankSuite
  ) where

import Control.Monad (forM, unless)
import qualified Data.ByteString as B
import Data.Bits (shiftL, (.|.))
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

-- | Words are A addresses; 'atDomain' maps them.
data Case = Case
  { caseName :: String
  , caseEntryA :: Word32
  , caseIdentity :: Word32
  , caseCpsr :: Word32
  , caseArguments :: [(Register, Word32)]
  , caseKernelWords :: [(Word32, Word32)]
  , caseDeviceWords :: [(Word32, Word32)]
  , casePacketWords :: [Word32]
  , caseCache :: Maybe CacheRegisters
  , caseTpidrurw :: Maybe Word32
  }

-- | Loader code addresses; define is the only entry above the controller extent.
atDomain :: Domain -> Word32 -> Word32
atDomain DomainA address = address
atDomain DomainB address
  | address >= 0x4d000 = address + 0x8ffc4
  | otherwise = address + 0x90000

-- | Kernel RAM addresses.
ramAt :: Domain -> Word32 -> Word32
ramAt DomainA address = address
ramAt DomainB address = address + 0x8ffc0

marker :: Domain -> Word32
marker domain = atDomain domain 0x42840

kernelBase, kernelSize, logFlag, packetAddress, stackPointer, exitAddress, deviceBase :: Word32
kernelBase = 0x400538fc
kernelSize = 0x400545a8 - 0x400538fc
logFlag = 0x4004f25c
packetAddress = 0x0096c000
stackPointer = 0x0095_9800
exitAddress = 0x0095_8f00
deviceBase = 0xfffb3000

table, active, unhandled, nesting :: Word32
table = 0x400538fc
active = 0x40054500
unhandled = 0x400544fc
nesting = 0x400545a4

packetSize, stackSize, exitSize, deviceSize :: Int
packetSize = 0x100
stackSize = 0x800
exitSize = 0x100
deviceSize = 0x2000

primary, secondary :: Word32
primary = 0x4003
secondary = 0x4004

bankOf :: Word32 -> Word32
bankOf identity = if identity == primary then 0xfffb3000 else 0xfffb4000

svc, svcMasked, svcIrqMasked :: Word32
svc = 0x13
svcMasked = 0xd3
svcIrqMasked = 0x93

base :: String -> Word32 -> Case
base name entry = Case name entry primary svc [] [] [] [] Nothing Nothing

with :: [(Register, Word32)] -> Case -> Case
with arguments one = one {caseArguments = arguments}

onCore :: Word32 -> Case -> Case
onCore identity one = one {caseIdentity = identity}

entryWords :: Word32 -> [Word32] -> [(Word32, Word32)]
entryWords irq values = [(table + 0x18 * irq + 4 * i, value) | (i, value) <- zip [0 ..] values]

-- | Registered `{handler, +0x4, priority, +0xc, mode}` for a line.
registered :: Word32 -> Word32 -> Word32 -> Word32 -> Word32 -> [(Word32, Word32)]
registered irq handler priority flag mode = entryWords irq [handler, 1, priority, flag, mode]

geometry :: Word32 -> [(Word32, Word32)] -> CacheRegisters
geometry clidr ccsidr = CacheRegisters {cacheSctlr = 0x00c5_1878, cacheClidr = clidr, cacheCsselr = 0, cacheCcsidr = ccsidr}

-- | CCSIDR for `ways` ways, `sets` sets and `line`-byte lines.
ccsidrOf :: Word32 -> Word32 -> Word32 -> Word32
ccsidrOf ways sets line = ((sets - 1) `shiftL` 13) .|. ((ways - 1) `shiftL` 3) .|. (log2 line - 4)
  where
    log2 value = fromIntegral (length (takeWhile (< value) (iterate (* 2) 1)))

-- | One level of data cache below the level of coherency.
oneLevel :: Word32 -> CacheRegisters
oneLevel ccsidr = geometry 0x0100_0002 [(0, ccsidr)]

cacheCase :: String -> Word32 -> [(Register, Word32)] -> CacheRegisters -> Case
cacheCase name entry arguments registers = (base name entry) {caseArguments = arguments, caseCache = Just registers}

cases :: Domain -> [Case]
cases domain =
  maskCases ++ cacheCases ++ coreCases ++ controllerCases domain ++ entryCases domain ++ slotCases domain

maskCases :: [Case]
maskCases =
  [ (base (name ++ "-" ++ suffix) entry) {caseCpsr = cpsr, caseArguments = [(R0, saved)]}
  | (name, entry, saved) <-
      [ ("save-irq-fiq", 0x42560, 0)
      , ("restore-irq-fiq-clear", 0x42568, 0)
      , ("restore-irq-fiq-set", 0x42568, 0xc0)
      , ("restore-irq-fiq-i", 0x42568, 0x80)
      , ("save-irq", 0x42574, 0)
      , ("restore-irq-clear", 0x4257c, 0)
      , ("restore-irq-set", 0x4257c, 0xffff_ffff)
      , ("save-fiq", 0x42588, 0)
      , ("restore-fiq-clear", 0x42590, 0)
      , ("restore-fiq-set", 0x42590, 0x40)
      ]
  , (suffix, cpsr) <- [("open", svc), ("masked", svcMasked), ("irq", svcIrqMasked), ("fiq", 0x53), ("sys-flags", 0xf00000df)]
  ]

cacheCases :: [Case]
cacheCases =
  [ cacheCase ("enable-" ++ name ++ "-" ++ state) entry [] (one {cacheSctlr = sctlr})
  | (name, entry) <- [("icache", 0x425c8), ("dcache", 0x425d0)]
  , (state, sctlr) <- [("off", 0x00c5_0078), ("on", 0x00c5_107c)]
  ]
    ++ [ cacheCase ("disable-" ++ name ++ "-" ++ state) entry [] (one {cacheSctlr = sctlr})
       | (name, entry) <- [("icache", 0x425d8), ("dcache", 0x425e0)]
       , (state, sctlr) <- [("off", 0x00c5_0078), ("on", 0x00c5_107c)]
       ]
    ++ [ cacheCase (name ++ "-" ++ range) entry [(R0, start), (R1, size)] one
       | (name, entry) <- [("icache-invalidate", 0x4262c), ("dcache-clean-invalidate", 0x42634), ("dcache-clean", 0x4263c), ("dcache-invalidate", 0x42644)]
       , (range, start, size) <-
           [ ("aligned", 0x0096_bd20, 0x20)
           , ("unaligned", 0x0096_bd24, 0x40)
           , ("installer", 0x0096_bd20, 32)
           , ("span", 0x0100_0010, 0x1f0)
           , ("signed-end", 0x7fff_ffc0, 0x80)
           , ("zero-start", 0, 0x40)
           , ("zero-length", 0x0096_bd20, 0)
           , ("zero-both", 0, 0)
           ]
       ]
    ++ [ cacheCase (name ++ "-whole-" ++ shape) entry [(R0, 0), (R1, 0)] registers
       | (name, entry) <- [("dcache-clean-invalidate", 0x42634), ("dcache-clean", 0x4263c)]
       , (shape, registers) <-
           [ ("one-way", oneLevel (ccsidrOf 1 4 32))
           , ("four-way", oneLevel (ccsidrOf 4 8 32))
           , ("r4-shaped", oneLevel (ccsidrOf 4 128 32))
           , ("two-levels", geometry 0x0200_0013 [(0, ccsidrOf 2 4 32), (2, ccsidrOf 8 4 64)])
           , ("instruction-only-level", geometry 0x0200_0011 [(2, ccsidrOf 2 2 32)])
           , ("no-coherency", geometry 0x0000_0002 [])
           ]
       ]
  where
    one = oneLevel (ccsidrOf 4 8 32)

coreCases :: [Case]
coreCases =
  [ onCore identity (base (name ++ "-" ++ label) entry)
  | (name, entry) <- [("identity", 0x427b8), ("core-index", 0x42854)]
  , (label, identity) <- [("primary", primary), ("secondary", secondary), ("other", 0x1234)]
  ]

controllerCases :: Domain -> [Case]
controllerCases domain =
  [ onCore identity ((base ("initialize-" ++ label) 0x4286c) {caseKernelWords = registered 5 0x1234_5678 3 1 2 ++ [(active + 8, 7), (unhandled, 9)]})
  | (label, identity) <- cores
  ]
    ++ [ with [(R0, irq), (R1, packetAddress)] ((base ("configure-" ++ label) 0x429ac) {casePacketWords = packet})
       | (label, irq, packet) <-
           [ ("valid", 5, [0x1234_5678, 1, 3, 1, 2])
           , ("flag-zero-mode-3", 127, [0x1234_5678, 0, 15, 0, 3])
           , ("irq-128", 128, [0x1234_5678, 1, 3, 1, 2])
           , ("zero-handler", 5, [0, 1, 3, 1, 2])
           , ("bit4-2", 5, [0x1234_5678, 2, 3, 1, 2])
           , ("priority-16", 5, [0x1234_5678, 1, 16, 1, 2])
           , ("flag-2", 5, [0x1234_5678, 1, 3, 2, 2])
           , ("flag-one-mode-4", 5, [0x1234_5678, 1, 3, 1, 4])
           , ("flag-zero-mode-4", 5, [0x1234_5678, 1, 3, 0, 4])
           ]
       ]
    ++ [ with [(R0, 5), (R1, packetAddress)] ((base "configure-masked" 0x429ac) {caseCpsr = svcIrqMasked, casePacketWords = [0x1234_5678, 1, 3, 1, 2]})
       , with [(R0, 5)] ((base "reset" 0x42a98) {caseKernelWords = registered 5 0x1234_5678 3 1 2})
       , with [(R0, 9)] ((base "handler-lookup" 0x42adc) {caseKernelWords = registered 9 0x1234_5678 3 1 2})
       ]
    ++ [ onCore identity (with [(R0, irq)] ((base ("enable-" ++ name ++ "-" ++ label) 0x42b00) {caseKernelWords = words'}))
       | (label, identity) <- cores
       , (name, irq, words') <-
           [ ("flag-zero", 5, registered 5 0x1234_5678 3 0 2)
           , ("flag-one", 37, registered 37 0x1234_5678 9 1 3)
           , ("line-127", 127, registered 127 0x1234_5678 15 0 1)
           , ("empty", 5, entryWords 5 [marker domain, 0, 15, 0, 1])
           , ("line-128", 128, [])
           ]
       ]
    ++ [ onCore identity (with [(R0, irq)] (base ("disable-" ++ name ++ "-" ++ label) 0x42eb0))
       | (label, identity) <- cores
       , (name, irq) <- [("line-5", 5), ("line-120", 120), ("line-128", 128)]
       ]
    ++ [ with [(R0, core), (R1, irq)] (base (name ++ "-" ++ show core ++ "-" ++ show irq) entry)
       | (name, entry) <- [("enable-set", 0x432f8), ("clear", 0x43358), ("raise", 0x433e4)]
       , core <- [0, 1]
       , irq <- [5, 120]
       ]
    ++ [ onCore identity (with [(R0, irq)] (base ("clear-pending-" ++ label ++ "-" ++ show irq) 0x43444))
       | (label, identity) <- cores
       , irq <- [5, 120]
       ]
    ++ [ with [(R0, packetAddress)] ((base "active-snapshot" 0x434a4) {caseKernelWords = [(active + 4 * i, 0x100 + i) | i <- [0 .. 31]]})
       ]
  where
    cores = [("primary", primary), ("secondary", secondary)]

-- | The initial controller bytes under every case's own words: a distinct word everywhere in both banks.
devicePattern :: [(Word32, Word32)]
devicePattern = [(deviceBase + 4 * i, 0x5a00_0000 .|. (i * 0x0101)) | i <- [0 .. fromIntegral deviceSize `div` 4 - 1]]

-- | The active bitmap with `irq` the highest set bit of its group, lower groups empty; all empty
-- without one.
activeBitmap :: Word32 -> Maybe Word32 -> [(Word32, Word32)]
activeBitmap bank irq = [(bank + 0x230 + 4 * group, groupWord group) | group <- [0 .. 3]]
  where
    groupWord group = case irq of
      Nothing -> 0
      Just line
        | group == line `div` 32 -> (1 `shiftL` fromIntegral (line `mod` 32)) .|. 1
        | group < line `div` 32 -> 0
        | otherwise -> 0x8000_0001

entryCases :: Domain -> [Case]
entryCases domain =
  [ onCore identity ((base ("enter-" ++ name ++ "-" ++ label) 0x430a8) {caseKernelWords = registered irq 0x1234_5678 6 0 1 ++ [(unhandled, 4)], caseDeviceWords = [(bankOf identity + 0x200, vector), (bankOf identity + 0x1c0, 0x77)] ++ activeBitmap (bankOf identity) line, caseTpidrurw = Just 0x0000_1234})
  | (label, identity) <- cores
  , (name, irq, vector, line) <-
      [ ("line-37", 37, 0x1234_5678, Just 37)
      , ("line-0", 0, 0x1234_5678, Just 0)
      , ("no-active", 37, 0x1234_5678, Nothing)
      , ("zero-vector", 37, 0, Just 37)
      , ("marker-vector", 37, marker domain, Just 37)
      ]
  ]
    ++ [ onCore identity ((base ("leave-" ++ name ++ "-" ++ label) 0x431d8) {caseKernelWords = registered 99 0x1234_5678 6 0 1 ++ [(nesting, depth), (active + 8 * 6, 0x77), (active + 8 * 6 + 4, 99)], caseDeviceWords = activeBitmap (bankOf identity) line, caseTpidrurw = Just 0x8000_1234})
       | (label, identity) <- cores
       , (name, depth, line) <- [("outer", 1, Just 99), ("nested", 2, Just 99), ("no-active", 1, Nothing)]
       ]
  where
    cores = [("primary", primary), ("secondary", secondary)]

slotCases :: Domain -> [Case]
slotCases domain =
  [ with [(R0, irq)] ((base ("slot-enable-" ++ name) 0x41108) {caseKernelWords = words'})
  | (name, irq, words') <- [("valid", 5, registered 5 0x1234_5678 3 1 2), ("empty", 5, entryWords 5 [marker domain, 0, 15, 0, 1]), ("line-128", 128, [])]
  ]
    ++ [ with [(R0, irq)] (base ("slot-disable-" ++ name) 0x41124)
       | (name, irq) <- [("valid", 5), ("line-128", 128)]
       ]
    ++ [ with [(R0, irq), (R1, packet)] ((base ("define-" ++ name) 0x4d8fc) {casePacketWords = words', caseKernelWords = registered 5 0x1234_5678 3 1 2})
       | (name, irq, packet, words') <-
           [ ("handler", 5, packetAddress, [0xdead_beef, 0x1234_5678, 3, 1, 2])
           , ("null", 5, 0, [])
           , ("null-line-128", 128, 0, [])
           , ("line-10005", 0x1_0005, packetAddress, [0xdead_beef, 0x1234_5678, 3, 1, 2])
           , ("zero-handler", 5, packetAddress, [0xdead_beef, 0, 3, 1, 2])
           , ("priority-16", 5, packetAddress, [0xdead_beef, 0x1234_5678, 16, 1, 2])
           , ("mode-4", 5, packetAddress, [0xdead_beef, 0x1234_5678, 3, 1, 4])
           ]
       ]

word32Bytes :: [(Word32, Word32)] -> Word32 -> Int -> B.ByteString
word32Bytes words' start size = foldl put (B.replicate size 0) [(address - start, value) | (address, value) <- words', address >= start, address - start + 4 <= fromIntegral size]
  where
    put bytes (offset, value) = B.take (fromIntegral offset) bytes <> word32le value <> B.drop (fromIntegral offset + 4) bytes

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

caseSpec :: Domain -> Role -> Image -> Owned -> Maybe Image -> String -> Case -> CaseSpec
caseSpec domain role loader owned bank suffix one =
  CaseSpec
    { caseId = caseName one ++ suffix ++ "-" ++ domainLabel domain
    , caseProvenance = "X-M1 1.50 IRQ, mask and cache services, x70view 55436fc85c0560dd252b0410af6f2427fe198cd3 and loader disassembly; synthetic kernel, controller and cache-register state"
    , caseExclusion = "Stack bytes, R1-R3, R12, LR and NZCV are excluded. Controller banks are memory-backed and cache registers are case inputs; neither is a hardware model."
    , caseEntry = wire entry
    , caseExit = wire exitAddress
    , caseRegions =
        [ region "code" (imageBase code) (B.length (imageBytes code)) ReadExecute (imageBytes code)
        , region "kernel" kernelAt (fromIntegral kernelSize) ReadWrite (word32Bytes [(ramAt domain address, value) | (address, value) <- caseKernelWords one] kernelAt (fromIntegral kernelSize))
        , region "log-flag" (ramAt domain logFlag) 4 ReadOnly (B.replicate 4 0)
        , region "packet" packetAddress packetSize ReadWrite (word32Bytes (zip [packetAddress, packetAddress + 4 ..] (casePacketWords one)) packetAddress packetSize)
        , region "stack" (stackPointer - fromIntegral stackSize) stackSize ReadWrite (B.replicate stackSize 0)
        , region "exit" exitAddress exitSize ReadExecute (B.replicate exitSize 0)
        , region "controller" deviceBase deviceSize Device (word32Bytes (devicePattern ++ caseDeviceWords one) deviceBase deviceSize)
        ]
          ++ [region "bank" (imageBase body) (B.length (imageBytes body)) ReadExecute (imageBytes body) | Just body <- [bankFor role]]
    , caseFunctions =
        [(wire (imageBase code), entry - imageBase code) | entry > imageBase code]
          ++ [(wire entry, imageBase code + fromIntegral (B.length (imageBytes code)) - entry)]
          ++ [(wire (imageBase body), fromIntegral (B.length (imageBytes body))) | Just body <- [bankFor role]]
    , caseInitial =
        [(Sp, stackPointer), (Lr, exitAddress), (Cpsr, caseCpsr one)]
          ++ [(register, M.findWithDefault (seed register) register (M.fromList (caseArguments one))) | register <- [R0 .. R12]]
    , caseExpectedStatus = Returned
    , caseExpectedRegisters = []
    , caseExpectedMemory = []
    , caseIgnoredMemory = []
    , caseCp14Identity = Just (caseIdentity one)
    , caseKernel = Just (KernelCase (InstructionLimit 40000) Nothing (caseTpidrurw one) Nothing Nothing [R1, R2, R3, R12, Lr] [] (caseCache one))
    }
  where
    entry = atDomain domain (caseEntryA one)
    kernelAt = ramAt domain kernelBase
    code = case role of
      Stock -> loader
      OwnedRole -> foldl patchImage loader (ownedCarrier owned)
    bankFor Stock = Nothing
    bankFor OwnedRole = Just (maybe (ownedBank owned) id bank)

seed :: Register -> Word32
seed register = 0xa5a5_0000 + fromIntegral (fromEnum register)

-- | The ordered writes into the kernel region.
kernelWrites :: Domain -> NativeObservation -> Maybe [(Word32, Word32)]
kernelWrites domain observation = do
  kernel <- nativeKernel observation
  order <- observedOrder kernel
  pure
    [ (address, fromIntegral value)
    | OrderedWrite (Write (N.GuestAddress wide) _) (WriteValue value) <- order
    , let address = fromIntegral wide :: Word32
    , address >= ramAt domain kernelBase
    , address < ramAt domain kernelBase + kernelSize
    ]

compareCase :: Domain -> CaseSpec -> NativeObservation -> NativeObservation -> [String]
compareCase domain spec stock owned =
  ["stock did not return: " ++ show (caseOutcome spec stock) | caseOutcome spec stock /= OutcomeReturned]
    ++ ["owned did not return: " ++ show (caseOutcome spec owned) | caseOutcome spec owned /= OutcomeReturned]
    ++ ["R0 differs" | N.lookupRegister R0 stock /= N.lookupRegister R0 owned]
    ++ ["callee-saved " ++ show register ++ " differs" | register <- [R4 .. R11], N.lookupRegister register stock /= N.lookupRegister register owned]
    ++ ["SP differs" | N.lookupRegister Sp stock /= N.lookupRegister Sp owned]
    ++ ["CPSR control differs" | fmap cpsrControl (N.lookupRegister Cpsr stock) /= fmap cpsrControl (N.lookupRegister Cpsr owned)]
    ++ [name ++ " bytes differ" | name <- ["kernel", "packet", "controller"], lookup (RegionId name) (nativeMemory stock) /= lookup (RegionId name) (nativeMemory owned)]
    ++ ["ordered kernel writes differ" | kernelWrites domain stock /= kernelWrites domain owned]
    ++ ["ordered kernel writes truncated" | kernelWrites domain stock == Nothing]
    ++ ["ordered device and CP15 operations differ" | platformOrder spec stock /= platformOrder spec owned]
    ++ ["ordered device and CP15 operations truncated" | platformOrder spec stock == Nothing]
    ++ ["TPIDRURW differs" | fmap observedTpidrurw (nativeKernel stock) /= fmap observedTpidrurw (nativeKernel owned)]

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
  unless (length (M.keys byName) == length selected) (fail "duplicate IRQ bank case name")
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
      judge spec stockId = case (M.lookup stockId reference, M.lookup (caseId spec) subject) of
        (Just stockObservation, Just ownedObservation) -> Right (compareCase domain spec stockObservation ownedObservation)
        (Nothing, _) -> Left "stock case did not run"
        (_, Nothing) -> Left "owned case did not run"
  results <- forM (zip selected stockSpecs) $ \(one, stockSpec) ->
    pure (caseName one, judge (caseSpec domain OwnedRole loader owned Nothing "" one) (caseId stockSpec))
  let mutantResults =
        [ ( name
          , case plan of
              Left problem -> Left problem
              Right (one, spec) -> case judge spec (caseId (caseSpec domain Stock loader owned Nothing "" one)) of
                Left problem -> Left problem
                Right [] -> Left "mutant was not detected"
                Right differences -> Right differences
          )
        | (name, plan) <- planned
        ]
  unless (length stockRaw == length selected && length ownedRaw == length selected && length mutantRaw == length mutantSpecs) (fail "native observations do not cover the IRQ bank case set")
  pure (SuiteRun domain results mutantResults (stockReuse, ownedReuse, mutantReuse))
