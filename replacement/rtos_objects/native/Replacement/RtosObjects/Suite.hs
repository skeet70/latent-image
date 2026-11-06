{-# LANGUAGE OverloadedStrings #-}

-- | The ten F2 family-native cases over both entry domains: the setup words, the expected
-- observation and the per-case comparison, over the exported batch protocol.
--
-- The data addresses come from 'FamilyEnv', which the caller builds from the layout facts.
-- The +0xd4 contract stays refused in the manifest; these cases are the oracle hypothesis.
module Replacement.RtosObjects.Suite
  ( Family (..)
  , FamilyEnv (..)
  , Where (..)
  , Row (..)
  , rows
  , caseFor
  , compareFamily
  , runFamily
  , FamilyRun (..)
  ) where

import Control.Monad (forM)
import qualified Data.ByteString as B
import Data.Bits (shiftL, shiftR, (.&.), (.|.))
import qualified Data.Map.Strict as M
import Data.List (nub)
import Data.Maybe (fromMaybe)
import Data.Word (Word32)
import Emulation.NativeObservation (GuestAddress (..), NativeObservation (..), RegionId (..), Register (..), RunnerStatus (..))
import Emulation.OwnedCase (CaseRegion (..), CaseSpec (..), Permission (..), hexAddress, mkRegion)
import Replacement.RtosOwnFirst.Regions (Domain (..))
import Replacement.RtosScheduler.Oracle (runCases, stripScratch)

-- | Which of the two stock entries the case calls.
data Family = SetFlag | Send
  deriving (Eq, Show)

-- | The layout addresses one case's body reads.
data FamilyEnv = FamilyEnv
  { envDomain :: Domain
  , envEntryWords :: Word32
  , envOwnRecord :: Word32
  , envPeerRecordA :: Word32
  , envPeerRecordB :: Word32
  , envDispatchEnabled :: Word32
  , envFlagCount :: Word32
  , envMailboxCount :: Word32
  }

-- | A case address, either inside the own record or absolute.
data Where = InRecord Word32 | At Word32

resolve :: FamilyEnv -> Where -> Word32
resolve env (InRecord offset) = envOwnRecord env + offset
resolve _ (At address) = address

-- | One case: the registers and data words it enters with, the timer stub's return, and what
-- the body must leave.
data Row = Row
  { rowName :: String
  , rowFamily :: Family
  , rowSlot :: Word32
  , rowRegisters :: [(Register, Word32)]
  , rowWords :: [(Where, Word32)]
  , rowTimer :: Word32
  , rowReturn :: Word32
  , rowAfter :: [(Where, Word32)]
  , rowExclusion :: String
  }

flagConfig :: Word32 -> Word32
flagConfig slot = 0x5ac + 0x38 * slot

mailboxConfig :: Word32 -> Word32
mailboxConfig slot = 0x5b4 + 0x38 * slot

-- | The ten cases: three set-flag effects, the remote and selector selection, the refusal,
-- three send effects and the unknown-extent receipt.
rows :: [Row]
rows =
  [ Row "set_own_created" SetFlag 0 [(R1, 0x1)] [(InRecord (flagConfig 0), 1), (InRecord (flagConfig 0 + 0xc), 0)] 0 0 [(InRecord (flagConfig 0 + 4), 0x1)] ""
  , Row "set_bits_accumulate" SetFlag 0 [(R1, 0x5)] [(InRecord (flagConfig 0), 1), (InRecord (flagConfig 0 + 4), 0x2)] 0 0 [(InRecord (flagConfig 0 + 4), 0x7)] ""
  , Row "set_auto_clear" SetFlag 0 [(R1, 0x1)] [(InRecord (flagConfig 0), 1), (InRecord (flagConfig 0 + 0xc), 1), (InRecord 0x28, 0x40000014), (At (0x40000014 + 0x10), 2), (At (0x40000014 + 0x14), 3)] 0 0 [(InRecord (flagConfig 0 + 4), 0), (At (0x40000014 + 0x10), 0)] ""
  , Row "set_remote" SetFlag 0 [(R1, 0x1)] [(InRecord (flagConfig 0), 1)] 0 0 [] "the remote flag record is the peer record; the doorbell word is outside the mapped data and the case asserts only the return"
  , Row "set_waiter_selector" SetFlag 0 [(R1, 0x1)] [(InRecord (flagConfig 0), 1), (InRecord 0x28, 0x40000014), (At (0x40000014 + 0x18), 0x40000030), (At (0x40000014 + 0x14), 7), (At (0x40000030 + 0x14), 2), (At (0x40000030 + 0x10), 2)] 0 0 [(At (0x40000014 + 0x10), 0), (At (0x40000014 + 0x28), 0)] ""
  , Row "set_not_created" SetFlag 0 [(R1, 0x1)] [(InRecord (flagConfig 0), 0)] 0 0xffffffd6 [] ""
  , Row "send_unknown_extent" Send 0 [(R1, 0x40000000)] [] 0 0xffffffd6 [] "the manifest keeps the +0xd4 extent refused; this case is the oracle hypothesis"
  , Row "send_created" Send 0 [(R1, 0x40000000)] [(InRecord (mailboxConfig 0), 1), (At 0x40000000, 0xabcd)] 0 0xffffffcf [(InRecord (mailboxConfig 0 + 0x18), 0x40000000), (InRecord (mailboxConfig 0 + 0x1c), 0x40000000)] ""
  , Row "send_priority_too_high" Send 0 [(R1, 0x40000000)] [(InRecord (mailboxConfig 0), 1), (InRecord (mailboxConfig 0 + 4), 2), (InRecord (mailboxConfig 0 + 0x2c), 3), (At 0x40000004, 5)] 0 0xffffffef [] ""
  , Row "send_not_created" Send 0 [(R1, 0x40000000)] [(InRecord (mailboxConfig 0), 0)] 0 0xffffffd6 [] ""
  ]

-- | The stock extents the two bodies may reach, as (start, length).
extents :: Domain -> [(Word32, Word32)]
extents DomainA =
  [ (0x439d8, 0x3c), (0x43a14, 0x28), (0x43c48, 0x5c), (0x43ca4, 0x7c), (0x43d20, 0xb0)
  , (0x43dd0, 0x48), (0x43f70, 0x2c), (0x44018, 0x2c), (0x44044, 0xf0), (0x44134, 0x80)
  , (0x4813c, 0x260), (0x4aa44, 0x304)
  ]
extents DomainB =
  [ (0xd398c, 0x3c), (0xd39c8, 0x28), (0xd3c04, 0x5c), (0xd3c60, 0x7c), (0xd3cdc, 0xb0)
  , (0xd3d8c, 0x48), (0xd3f2c, 0x2c), (0xd3fd4, 0x30), (0xd4004, 0xf0), (0xd40f4, 0x80)
  , (0xd80fc, 0x260), (0xdaa04, 0x304)
  ]

imageBase :: Domain -> Word32
imageBase DomainA = 0x439d8
imageBase DomainB = 0xd398c

timerEntry :: Domain -> Word32
timerEntry DomainA = 0x44ca8
timerEntry DomainB = 0xd4c68

raiseIrqEntry :: Domain -> Word32
raiseIrqEntry DomainA = 0x433e4
raiseIrqEntry DomainB = 0xd33e4

entryAddress :: Domain -> Family -> Word32
entryAddress DomainA SetFlag = 0x4813c
entryAddress DomainA Send = 0x4aa44
entryAddress DomainB SetFlag = 0xd80fc
entryAddress DomainB Send = 0xdaa04

-- | @mov r0, #code@ then @bx lr@.
stub :: Word32 -> B.ByteString
stub code = B.pack (concatMap littleEndian [0xe3a00000 .|. (code .&. 0xff), 0xe12fff1e])
  where
    littleEndian word = [fromIntegral (word `shiftR` shift) | shift <- [0, 8, 16, 24]]

region :: RegionId -> Word32 -> Int -> Permission -> B.ByteString -> CaseRegion
region ident base size permission bytes = mkRegion ident (GuestAddress (fromIntegral base)) size permission bytes

-- | The entry table and the two counts the bodies read.
layoutWords :: FamilyEnv -> [(Where, Word32)]
layoutWords env =
  [ (At (envEntryWords env), 0)
  , (At (envEntryWords env + 4), envPeerRecordA env)
  , (At (envEntryWords env + 8), envPeerRecordB env)
  , (At (envFlagCount env), 8)
  , (At (envMailboxCount env), 8)
  ]

-- | A 4 KiB region at @base@, with the words of @entries@ that fall inside it.
pageOf :: Word32 -> [(Where, Word32)] -> FamilyEnv -> CaseRegion
pageOf base entries env =
  let words = [(resolved, value) | (where', value) <- entries, let resolved = resolve env where', resolved >= base, resolved < base + 0x1000]
   in region (RegionId ("page-" ++ show base)) base 0x1000 ReadWrite (plant base 0x1000 words)

-- | The pages the case's words touch, each once.
pages :: FamilyEnv -> Row -> [CaseRegion]
pages env row =
  let words' = layoutWords env ++ rowWords row
      bases = nub (map (\address -> address .&. 0xfffff000) (map (resolve env . fst) words') ++ [0x40000000])
   in [pageOf base words' env | base <- bases]

plant :: Word32 -> Int -> [(Word32, Word32)] -> B.ByteString
plant base size words =
  B.pack
    [ fromIntegral (value `shiftR` shift .&. 0xff)
    | offset <- [0 .. size - 1]
    , let address = base + fromIntegral offset
    , let value = fromMaybe 0 (lookup address words)
    , let shift = 8 * (offset `mod` 4)
    ]

-- | The case's spec over the stock entry, the owned image and the stub entries.
caseFor :: FamilyEnv -> B.ByteString -> Row -> CaseSpec
caseFor env bin row =
  CaseSpec
    { caseId = rowName row ++ "-" ++ show (envDomain env)
    , caseProvenance = "X-M1 1.50 loader " ++ show (envDomain env) ++ " " ++ show (rowFamily row) ++ " over the rtos_layout stock data layout"
    , caseExclusion = rowExclusion row
    , caseEntry = GuestAddress (fromIntegral (entryAddress (envDomain env) (rowFamily row)))
    , caseExit = GuestAddress 0x958f00
    , caseRegions =
        [region (RegionId ("code-" ++ show base)) base (fromIntegral size) ReadExecute (slice base size) | (base, size) <- extents (envDomain env)]
          ++ [ region (RegionId "timer") (timerEntry (envDomain env)) 8 ReadExecute (stub (rowTimer row))
             , region (RegionId "raise") (raiseIrqEntry (envDomain env)) 8 ReadExecute (stub 0)
             ]
          ++ pages env row
          ++ [region (RegionId "stack") 0x959700 0x110 ReadWrite (B.replicate 0x110 0)]
    , caseFunctions = [(GuestAddress (fromIntegral base), size) | (base, size) <- extents (envDomain env)]
    , caseInitial = [(R0, rowSlot row), (R1, 0)] ++ rowRegisters row ++ [(Sp, 0x959800), (Lr, 0x958f00), (Cpsr, 0x13)]
    , caseExpectedStatus = Returned
    , caseExpectedRegisters = [(R0, rowReturn row)]
    , caseExpectedMemory = []
    , caseCp14Identity = Nothing
    , caseKernel = Nothing
    }
  where
    slice base size = B.take (fromIntegral size) (B.drop (fromIntegral (base - imageBase (envDomain env))) bin)

wordAt :: B.ByteString -> Word32 -> Word32 -> Word32
wordAt bytes base address =
  let offset = fromIntegral (address - base)
   in B.foldl' (\value byte -> value `shiftL` 8 .|. fromIntegral byte) 0 (B.take 4 (B.drop offset bytes))

-- | The case's differences from its expected observation.
compareFamily :: FamilyEnv -> Row -> CaseSpec -> NativeObservation -> [String]
compareFamily env row spec observation =
  statusDifference ++ registerDifferences ++ memoryDifferences
  where
    statusDifference = ["status " ++ show (nativeStatus observation) ++ " expected " ++ show (caseExpectedStatus spec) | nativeStatus observation /= caseExpectedStatus spec]
    registerDifferences =
      [ "r0 " ++ hex (M.findWithDefault 0 r (nativeRegisters observation)) ++ " expected " ++ hex expected
      | (r, expected) <- caseExpectedRegisters spec
      , M.findWithDefault 0 r (nativeRegisters observation) /= expected
      ]
    memoryDifferences =
      [ "word " ++ hex address ++ " is " ++ hex actual ++ " expected " ++ hex expected
      | (where', expected) <- rowAfter row
      , let address = resolve env where'
      , let actual = fromMaybe 0 (observedWord address)
      , actual /= expected
      ]
    observedWord address =
      let page = address .&. 0xfffff000
       in do
            (_, bytes) <- lookup (RegionId ("page-" ++ show page)) (nativeMemory observation)
            pure (wordAt bytes page address)
    hex value = hexAddress (GuestAddress (fromIntegral value))

data FamilyRun = FamilyRun
  { familyDomain :: Domain
  , familyCases :: [(String, Either String [String])]
  , familyMatches :: Int
  }

-- | Runs the ten rows in one domain: the batch is written and observed once, then each case is
-- compared to its spec.
runFamily :: FilePath -> FilePath -> FamilyEnv -> B.ByteString -> IO FamilyRun
runFamily runner directory env bin = do
  let cases = [(rowName row, caseFor env bin row) | row <- rows]
      byName = M.fromList (zip (map rowName rows) rows)
  observed <- runCases runner directory cases
  results <- forM cases $ \(label, spec) -> do
    let row = byName M.! label
    pure (label, maybe (Left "no observation") (\observation -> Right (compareFamily env row spec (stripScratch observation))) (M.lookup label observed))
  pure
    FamilyRun
      { familyDomain = envDomain env
      , familyCases = results
      , familyMatches = length [() | (_, Right []) <- results]
      }
