{-# LANGUAGE OverloadedStrings #-}

-- | Cyclic creation, lifecycle and expiry cases over the stock loaders and the owned A32
-- entries. Expiry runs through the owned timer tick so the pop and re-arm are real.
module Replacement.RtosCyclic.Suite
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
import Data.Word (Word32, Word8)
import Emulation.CachedNativeBatch (BatchReuse, cachedObservedBatch)
import Emulation.NativeObservation
import Emulation.OwnedCase
import Replacement.RtosOwnFirst.Regions (Domain (..), domainLabel)
import Replacement.RtosCyclic.Layout
import System.FilePath ((</>))

data Image = Image { imageBase :: Word32, imageBytes :: B.ByteString }

data Role = Stock | Owned deriving (Eq, Show)

data Call = Configure | Schedule | Expire | Initialize | Start | Stop | Create | Delete | Tick
  deriving (Eq, Show)

data Callback = Receipt | Sink | HandlerDelete | HandlerDeleteRecreate deriving (Eq, Show)

data Kind = Equal | Repair String deriving (Eq, Show)

data Config = Config
  { cfgAttributes :: Word32
  , cfgArgument :: Word32
  , cfgHandler :: Word32
  , cfgPeriod :: Word32
  , cfgPhase :: Word32
  , cfgExists :: Word32
  }
  deriving (Eq, Show)

data Control = Control
  { ctlActive :: Word32
  , ctlExpiry :: Word32
  , ctlIndex :: Word32
  , ctlCallback :: Word32
  , ctlArgument :: Word32
  }
  deriving (Eq, Show)

data Check
  = ConfigWord Int Word32 Word32
  | ControlWord Int Word32 Word32
  | SecondControlWord Int Word32 Word32
  | HeapWord Int Word32 Word32
  | CountIs Word32
  | BaseIs Word32
  | TickIs Word32
  | RegisterIs Register Word32
  | ReceiptsAre [(Word32, Word32, Word32)]
  | CpsrControlIs Word32
  | WordIs String Word32 Word32
  | RegionsUnchanged [String]
  | ConfigurationPublished
  | ControlArgumentIs Int
  | HeapEntryIs Int
  deriving (Eq, Show)

data Case = Case
  { caseName :: String
  , caseCall :: Call
  , caseKind :: Kind
  , caseConfigs :: [(Int, Config)]
  , caseControls :: [(Int, Control)]
  , caseSecondControls :: [(Int, Control)]
  , caseCount :: Word32
  , caseTick :: Word32
  , caseHandlerContext :: Word32
  , caseCpsr :: Word32
  , caseNumber :: Word32
  , caseRequest :: Maybe (Word32, Word32, Word32, Word32, Word32)
  , caseCallback :: Callback
  , caseChecks :: [Check]
  , caseStockChecks :: [Check]
  , caseOwnedChecks :: [Check]
  , caseStockStatus :: RunnerStatus
  , caseOwnedStatus :: RunnerStatus
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

stubBase, stubAcquire, stubRelease, stubReceipt, stubDelete, stubDeleteRecreate, stubSize :: Word32
stubBase = 0x01000000
stubAcquire = stubBase
stubRelease = stubBase + 0x18
stubReceipt = stubBase + 0x30
stubDelete = stubBase + 0xd0
stubDeleteRecreate = stubBase + 0xf0
stubSize = 0x200

exitAddress, stackBase, stackSize, stackPointer :: Word32
exitAddress = 0x01000200
stackBase = 0x300000
stackSize = 0x1000
stackPointer = stackBase + 0x800

entriesBase, entriesSize :: Word32
entriesBase = 0x40100000
entriesSize = 0x1000

lockCell, counterBase, receiptBase :: Word32
lockCell = entriesBase + 0x10c
counterBase = entriesBase + 0x110
receiptBase = entriesBase + 0x200

heapAddress, timerCountWord, timerBaseWord, timerTickWord :: Facts -> Domain -> Word32
heapAddress facts domain = dfTimerHeap (domainFacts facts domain)
timerCountWord facts domain = dfTimerCount (domainFacts facts domain)
timerBaseWord facts domain = dfTimerBase (domainFacts facts domain)
timerTickWord facts domain = dfTickNow (domainFacts facts domain)

secondControls, requestBase :: Word32
secondControls = entriesBase + 0x600
requestBase = entriesBase + 0x700

wordAddress :: GuestAddress -> Word32
wordAddress (GuestAddress address) = fromIntegral address

configurationAddress :: Domain -> GuestAddress
configurationAddress DomainA = GuestAddress 0x400c_0818
configurationAddress DomainB = GuestAddress 0x400c_0888

configurationRegion :: RegionId
configurationRegion = RegionId "configuration"

configurationWordAddress :: Domain -> Word32
configurationWordAddress DomainA = 0x4005_46d4
configurationWordAddress DomainB = 0x400e_4694

seedRegister :: Register -> Word32
seedRegister register = 0xa5a50000 + fromIntegral (fromEnum register)

movwI, movtI, ldrI, strI, movReg, movImm, blWord :: Word32 -> Word32 -> Word32
addRegLsl2, addImm, subImm, andImm :: Word32 -> Word32 -> Word32 -> Word32
movwI rd imm = 0xe3000000 .|. (((imm `shiftR` 12) .&. 0xf) `shiftL` 16) .|. (rd `shiftL` 12) .|. (imm .&. 0xfff)
movtI rd imm = 0xe3400000 .|. (((imm `shiftR` 12) .&. 0xf) `shiftL` 16) .|. (rd `shiftL` 12) .|. (imm .&. 0xfff)
ldrI rd rn = 0xe5900000 .|. (rn `shiftL` 16) .|. (rd `shiftL` 12)
strI rd rn = 0xe5800000 .|. (rn `shiftL` 16) .|. (rd `shiftL` 12)
addRegLsl2 rd rn rm = 0xe0800000 .|. (rn `shiftL` 16) .|. (rd `shiftL` 12) .|. (2 `shiftL` 7) .|. rm
addImm rd rn imm = 0xe2800000 .|. (rn `shiftL` 16) .|. (rd `shiftL` 12) .|. (imm .&. 0xfff)
subImm rd rn imm = 0xe2400000 .|. (rn `shiftL` 16) .|. (rd `shiftL` 12) .|. (imm .&. 0xfff)
andImm rd rn imm = 0xe2000000 .|. (rn `shiftL` 16) .|. (rd `shiftL` 12) .|. (imm .&. 0xff)
movReg rd rm = 0xe1a00000 .|. (rd `shiftL` 12) .|. rm
movImm rd imm = 0xe3a00000 .|. (rd `shiftL` 12) .|. (imm .&. 0xfff)
blWord here target =
  let delta = fromIntegral target - fromIntegral here - 8 :: Int
   in 0xeb000000 .|. (fromIntegral ((delta `div` 4) .&. 0xffffff) :: Word32)

push5Lr, pop5, popPc, bxLr, mrsCpsr :: Word32
push5Lr = 0xe92d41f0
pop5 = 0xe8bd01f0
popPc = 0xe8bd8000
bxLr = 0xe12fff1e
mrsCpsr = 0xe10f7000

fillStubs :: [(Int, [Word32])] -> B.ByteString
fillStubs segments = B.concat (map word32le (go 0))
  where
    total = fromIntegral stubSize
    go offset = case lookup offset segments of
      Just words' -> words' ++ go (offset + 4 * length words')
      Nothing
        | offset < total -> 0xe7f000f0 : go (offset + 4)
        | otherwise -> []

receiptBody :: [Word32]
receiptBody =
  [ push5Lr
  , movReg 4 0
  , movwI 5 (counterBase .&. 0xffff), movtI 5 (counterBase `shiftR` 16)
  , ldrI 6 5
  , addImm 6 6 1
  , strI 6 5
  , subImm 6 6 1
  , movwI 7 (receiptBase .&. 0xffff), movtI 7 (receiptBase `shiftR` 16)
  , addRegLsl2 7 7 6
  , strI 4 7
  , movwI 7 (lockCell .&. 0xffff), movtI 7 (lockCell `shiftR` 16)
  , ldrI 7 7
  , movwI 8 ((receiptBase + 0x80) .&. 0xffff), movtI 8 ((receiptBase + 0x80) `shiftR` 16)
  , addRegLsl2 8 8 6
  , strI 7 8
  , mrsCpsr
  , andImm 7 7 0xff
  , movwI 8 ((receiptBase + 0x100) .&. 0xffff), movtI 8 ((receiptBase + 0x100) `shiftR` 16)
  , addRegLsl2 8 8 6
  , strI 7 8
  ]

stubBytes :: Domain -> Case -> B.ByteString
stubBytes domain one = fillStubs $
  [ (0, [ldrI 1 0, movImm 2 1, strI 2 1, movImm 0 0, bxLr])
  , (0x18, [ldrI 1 0, movImm 2 0, strI 2 1, movImm 0 0, bxLr])
  , (0x30, receiptBody ++ [pop5, popPc])
  , (0x180, [push5Lr, pop5, popPc])
  ]
    ++ [(0xd0, deleteStub) | caseCallback one == HandlerDelete]
    ++ [(0xf0, recreateStub) | caseCallback one == HandlerDeleteRecreate]
  where
    stores = drop 2 receiptBody
    deleteStub =
      [ push5Lr
      , movReg 4 0
      ]
        ++ stores
        ++ [ movwI 0 (caseNumber one .&. 0xffff), movtI 0 (caseNumber one `shiftR` 16)
           , blWord (stubDelete + fromIntegral (4 * (2 + length stores + 2))) (deleteEntry domain)
           , pop5, popPc
           ]
    recreateStub =
      [ push5Lr
      , movReg 4 0
      ]
        ++ stores
        ++ [ movwI 0 (caseNumber one .&. 0xffff), movtI 0 (caseNumber one `shiftR` 16)
           , blWord (stubDeleteRecreate + fromIntegral (4 * (2 + length stores + 2))) (deleteEntry domain)
           , movwI 0 (caseNumber one .&. 0xffff), movtI 0 (caseNumber one `shiftR` 16)
           , movwI 1 (requestBase .&. 0xffff), movtI 1 (requestBase `shiftR` 16)
           , blWord (stubDeleteRecreate + fromIntegral (4 * (2 + length stores + 7))) (createEntry domain)
           , pop5, popPc
           ]

expireEntry :: Domain -> Word32
expireEntry DomainA = 0x4beb0
expireEntry DomainB = 0xdbe70

deleteEntry :: Domain -> Word32
deleteEntry DomainA = 0x4d8a8
deleteEntry DomainB = 0xdd86c

createEntry :: Domain -> Word32
createEntry DomainA = 0x4d818
createEntry DomainB = 0xdd7dc

callEntry :: Domain -> Call -> Word32
callEntry domain call = case call of
  Configure -> configureEntry domain
  Schedule -> scheduleEntry domain
  Expire -> expireEntry domain
  Initialize -> initializeEntry domain
  Start -> startEntry domain
  Stop -> stopEntry domain
  Create -> createEntry domain
  Delete -> deleteEntry domain
  Tick -> tickEntry domain

configureEntry, scheduleEntry, initializeEntry, startEntry, stopEntry, tickEntry :: Domain -> Word32
configureEntry DomainA = 0x4be3c
configureEntry DomainB = 0xdbdfc
scheduleEntry DomainA = 0x4be84
scheduleEntry DomainB = 0xdbe44
initializeEntry DomainA = 0x4bf3c
initializeEntry DomainB = 0xdbefc
startEntry DomainA = 0x4bfcc
startEntry DomainB = 0xdbf8c
stopEntry DomainA = 0x4c13c
stopEntry DomainB = 0xdc0fc
tickEntry DomainA = 0x44d8c
tickEntry DomainB = 0xd4d4c

ownedRanges :: Domain -> [(Word32, Word32)]
ownedRanges DomainA = [(0x4be3c, 0x4d8fc), (0x44a50, 0x44f2c)]
ownedRanges DomainB = [(0xdbdfc, 0xdd8c0), (0xd4a10, 0xd4eec)]

parseOwnedSymbols :: Domain -> FilePath -> IO [(Word32, Word32)]
parseOwnedSymbols domain path = do
  rows <- lines <$> readFile path
  pure (concatMap row rows)
  where
    row line = case words line of
      [a, z, kind, _] -> case (parseWord32 a, parseWord32 z) of
        (Just address, Just size)
          | kind `elem` ["T", "t"], size > 0
          , any (\(low, high) -> address >= low && address < high && size <= high - address) (ownedRanges domain) ->
              [(address, size)]
        _ -> []
      _ -> []

cyclicLow, cyclicSize, timerLow, timerSize :: Domain -> Word32
cyclicLow DomainA = 0x4be3c
cyclicLow DomainB = 0xdbdfc
cyclicSize DomainA = 0x1ac0
cyclicSize DomainB = 0x1ac4
timerLow DomainA = 0x44a50
timerLow DomainB = 0xd4a10
timerSize _ = 0x4dc

wordsBase :: Domain -> Word32
wordsBase DomainA = 0x40054000
wordsBase DomainB = 0x400e4000

configBase :: Facts -> Domain -> Word32
configBase facts domain = dfCyclicConfigs (domainFacts facts domain)

controlBase :: Facts -> Domain -> Word32
controlBase facts domain = dfCyclicControls (domainFacts facts domain)

controlAddress :: Facts -> Domain -> Int -> Word32
controlAddress facts domain slot = controlBase facts domain + fromIntegral (slot - 1) * 0x18

controlTimer :: Facts -> Domain -> Int -> Word32
controlTimer facts domain slot = controlAddress facts domain slot + 0x0c

configAddress :: Facts -> Domain -> Int -> Word32
configAddress facts domain slot = configBase facts domain + fromIntegral (slot - 1) * 0x18

cases :: Facts -> Domain -> [Case]
cases facts domain = coreCases facts domain

coreCases :: Facts -> Domain -> [Case]
coreCases _facts domain =
  [ Case "create-empty" Create Equal [(1, emptyConfig)] [] [] 4 0 0 0x13 1
      (Just (0x2, 0x11111111, 0x22222222, 30, 0)) Receipt
      [ RegisterIs R0 0
      , ConfigWord 1 0x00 0x2
      , ConfigWord 1 0x04 0x11111111
      , ConfigWord 1 0x08 0x22222222
      , ConfigWord 1 0x0c 30
      , ConfigWord 1 0x10 0
      , ConfigWord 1 0x14 1
      , RegionsUnchanged ["words", "controls", "count", "heap", "entries"]
      ]
      [] [] Returned Returned
  , Case "create-exists" Create Equal [(1, Config 0x2 0x11111111 0x22222222 30 0 1)] [] [] 4 0 0 0x13 1
      (Just (0x2, 0x33333333, 0x44444444, 40, 0)) Receipt
      [ RegisterIs R0 0xffffffd7
      , ConfigWord 1 0x0c 30
      , ConfigWord 1 0x14 1
      , RegionsUnchanged ["words", "configs", "controls", "count", "heap", "entries"]
      ]
      [] [] Returned Returned
  , Case "create-zero-period" Create (Repair "the replacement refuses a zero period without a write") [(1, emptyConfig)] [] [] 4 0 0 0x13 1
      (Just (0x2, 0x11111111, 0x22222222, 0, 0)) Receipt
      [ RegisterIs R0 0, ConfigWord 1 0x00 0x2, ConfigWord 1 0x04 0x11111111, ConfigWord 1 0x08 0x22222222, ConfigWord 1 0x0c 0, ConfigWord 1 0x10 0, ConfigWord 1 0x14 1 ]
      [ RegisterIs R0 0, ConfigWord 1 0x00 0x2, ConfigWord 1 0x04 0x11111111, ConfigWord 1 0x08 0x22222222, ConfigWord 1 0x0c 0, ConfigWord 1 0x10 0, ConfigWord 1 0x14 1 ]
      [ RegisterIs R0 0xffffffef, ConfigWord 1 0x00 0, ConfigWord 1 0x04 0, ConfigWord 1 0x08 0, ConfigWord 1 0x0c 0, ConfigWord 1 0x10 0, ConfigWord 1 0x14 0 ]
      Returned Returned
  , Case "delete-inactive" Delete Equal [(1, Config 0x2 0x11111111 0x22222222 30 0 1)] [(1, Control 0 0 0 0 0)] [] 4 0 0 0x13 1
      Nothing Receipt
      [ RegisterIs R0 0, ConfigWord 1 0x14 0, ControlWord 1 0x04 0, ControlWord 1 0x0c 0, CountIs 0 ]
      [] [] Returned Returned
  , Case "delete-active" Delete (Repair "deleting an active handler stops its timer") [(1, Config 0x2 0x11111111 0x22222222 30 0 1)] [(1, Control 1 10 1 0 0)] [] 4 10 0 0x13 1
      Nothing Receipt
      [ RegisterIs R0 0, ConfigWord 1 0x14 0, ControlWord 1 0x04 1, ControlWord 1 0x0c 1, CountIs 1 ]
      [ RegisterIs R0 0, ConfigWord 1 0x14 0, ControlWord 1 0x04 1, ControlWord 1 0x0c 1, CountIs 1 ]
      [ RegisterIs R0 0, ConfigWord 1 0x14 0, ControlWord 1 0x04 0, ControlWord 1 0x0c 0, CountIs 0 ]
      Returned Returned
  , Case "start-inactive" Start Equal [(1, Config 0x2 0x11111111 0x22222222 30 0 1)] [(1, Control 0 0 0 0 0)] [] 4 100 0 0x13 1
      Nothing Receipt
      [ RegisterIs R0 0
      , ControlWord 1 0x04 1
      , ControlWord 1 0x08 130
      , ControlWord 1 0x0c 1
      , ControlWord 1 0x10 (expireEntry domain)
      , ControlArgumentIs 1
      , HeapWord 1 0x00 130
      , HeapEntryIs 1
      , CountIs 1
      ]
      [] [] Returned Returned
  , Case "start-restart" Start Equal [(1, Config 0x2 0x11111111 0x22222222 30 0 1)] [(1, Control 1 10 1 0 0)] [] 4 200 0 0x13 1
      Nothing Receipt
      [ RegisterIs R0 0, ControlWord 1 0x04 1, ControlWord 1 0x08 230, ControlWord 1 0x0c 1
      , HeapWord 1 0x00 230, HeapEntryIs 1, CountIs 1
      ]
      [] [] Returned Returned
  , Case "start-context" Start Equal [(1, Config 0x2 0x11111111 0x22222222 30 0 1)] [(1, Control 0 0 0 0 0)] [] 4 0 1 0x13 1
      Nothing Receipt
      [ RegisterIs R0 0xffffffe7, ControlWord 1 0x04 0, CountIs 0 ]
      [] [] Returned Returned
  , Case "start-masked" Start Equal [(1, Config 0x2 0x11111111 0x22222222 30 0 1)] [(1, Control 0 0 0 0 0)] [] 4 0 0 0x93 1
      Nothing Receipt
      [ RegisterIs R0 0xffffffe7, ControlWord 1 0x04 0, CountIs 0 ]
      [] [] Returned Returned
  , Case "start-invalid-class" Start Equal [(1, Config 0x2 0x11111111 0x22222222 30 0 1)] [(1, Control 0 0 0 0 0)] [] 4 0 0 0x13 0x00030001
      Nothing Receipt
      [ RegisterIs R0 0xffffffee, ControlWord 1 0x04 0, CountIs 0 ]
      [] [] Returned Returned
  , Case "start-not-created" Start Equal [(1, Config 0x0 0 0 0 0 0)] [(1, Control 0 0 0 0 0)] [] 4 0 0 0x13 1
      Nothing Receipt
      [ RegisterIs R0 0xffffffd6, ControlWord 1 0x04 0, CountIs 0 ]
      [] [] Returned Returned
  , Case "start-zero-period" Start (Repair "the replacement refuses a zero period without arming") [(1, Config 0x2 0x11111111 0x22222222 0 0 1)] [(1, Control 0 0 0 0 0)] [] 4 100 0 0x13 1
      Nothing Receipt
      [ RegisterIs R0 0, ControlWord 1 0x04 1, ControlWord 1 0x0c 1, CountIs 1 ]
      [ RegisterIs R0 0, ControlWord 1 0x04 1, ControlWord 1 0x0c 1, CountIs 1 ]
      [ RegisterIs R0 0xffffffef, ControlWord 1 0x04 0, ControlWord 1 0x0c 0, CountIs 0 ]
      Returned Returned
  , Case "stop-active" Stop (Repair "the owned removal detaches the entry") [(1, Config 0x2 0x11111111 0x22222222 30 0 1)] [(1, Control 1 10 1 0 0)] [] 4 10 0 0x13 1
      Nothing Receipt
      [ RegisterIs R0 0, ControlWord 1 0x04 0, ControlWord 1 0x0c 1, CountIs 0 ]
      [ RegisterIs R0 0, ControlWord 1 0x04 0, ControlWord 1 0x0c 1, CountIs 0 ]
      [ RegisterIs R0 0, ControlWord 1 0x04 0, ControlWord 1 0x0c 0, CountIs 0 ]
      Returned Returned
  , Case "stop-inactive" Stop Equal [(1, Config 0x2 0x11111111 0x22222222 30 0 1)] [(1, Control 0 0 0 0 0)] [] 4 0 0 0x13 1
      Nothing Receipt
      [ RegisterIs R0 0, ControlWord 1 0x04 0, ControlWord 1 0x0c 0, CountIs 0 ]
      [] [] Returned Returned
  , Case "stop-context" Stop Equal [(1, Config 0x2 0x11111111 0x22222222 30 0 1)] [(1, Control 0 0 0 0 0)] [] 4 0 1 0x13 1
      Nothing Receipt
      [ RegisterIs R0 0xffffffe7, ControlWord 1 0x04 0, CountIs 0 ]
      [] [] Returned Returned
  , Case "expire-rearm" Tick Equal [(1, Config 0x2 0x11111111 0x22222222 30 0 1)] [(1, Control 1 10 1 0 0)] [] 4 10 1 0x13 1
      Nothing Receipt
      [ RegisterIs R0 0, ReceiptsAre [(0x11111111, 0, 0x13)], ControlWord 1 0x04 1, ControlWord 1 0x08 40
      , ControlWord 1 0x0c 1, HeapWord 1 0x00 40, CountIs 1, BaseIs 10, TickIs 20
      ]
      [] [] Returned Returned
  , Case "expire-zero-period" Tick (Repair "the replacement delivers one expiry instead of looping") [(1, Config 0x2 0x11111111 0x22222222 0 0 1)] [(1, Control 1 10 1 0 0)] [] 4 10 1 0x13 1
      Nothing Receipt
      []
      [ RegisterIs R0 0, ReceiptsAre [(0x11111111, 0, 0x13)], ControlWord 1 0x04 0, ControlWord 1 0x0c 0, CountIs 0, BaseIs 10, TickIs 20 ]
      [ RegisterIs R0 0, ReceiptsAre [(0x11111111, 0, 0x13)], ControlWord 1 0x04 0, ControlWord 1 0x0c 0, CountIs 0, BaseIs 10, TickIs 20 ]
      InstructionBudget Returned
  , Case "expire-handler-delete" Tick (Repair "deleting from the handler stops the re-armed entry") [(1, Config 0x2 0x11111111 0x22222222 30 0 1)] [(1, Control 1 10 1 0 0)] [] 4 10 1 0x13 1
      Nothing HandlerDelete
      []
      [ RegisterIs R0 0, ReceiptsAre [(0x11111111, 0, 0x13)], ConfigWord 1 0x14 0, ControlWord 1 0x04 1, ControlWord 1 0x0c 1, CountIs 1 ]
      [ RegisterIs R0 0, ReceiptsAre [(0x11111111, 0, 0x13)], ConfigWord 1 0x14 0, ControlWord 1 0x04 0, ControlWord 1 0x0c 0, CountIs 0 ]
      Returned Returned
  , Case "expire-handler-delete-recreate" Tick (Repair "a recreated registration is not re-armed by the old expiry") [(1, Config 0x2 0x11111111 0x22222222 30 0 1)] [(1, Control 1 10 1 0 0)] [] 4 10 1 0x13 1
      (Just (0x2, 0x33333333, 0x9999, 50, 0)) HandlerDeleteRecreate
      []
      [ RegisterIs R0 0, ReceiptsAre [(0x11111111, 0, 0x13)], ConfigWord 1 0x0c 50, ConfigWord 1 0x08 0x9999, ControlWord 1 0x04 1, ControlWord 1 0x0c 1, CountIs 1 ]
      [ RegisterIs R0 0, ReceiptsAre [(0x11111111, 0, 0x13)], ConfigWord 1 0x0c 50, ConfigWord 1 0x08 0x9999, ControlWord 1 0x04 0, ControlWord 1 0x0c 0, CountIs 0 ]
      Returned Returned
  , Case "initialize" Initialize (Repair "a zero period is not started at boot") [(1, Config 0x2 0x11111111 0x22222222 30 7 1), (2, Config 0 0 0 30 0 1), (3, Config 0x2 0x33333333 0x44444444 0 0 1)] [] [] 3 100 0 0x13 0
      Nothing Receipt
      []
      [ ControlWord 1 0x04 1, ControlWord 1 0x08 7, ControlWord 1 0x0c 2
      , ControlWord 2 0x04 0, ControlWord 3 0x04 1, ControlWord 3 0x08 0, ControlWord 3 0x0c 1, CountIs 2
      ]
      [ ControlWord 1 0x04 1, ControlWord 1 0x08 7, ControlWord 1 0x0c 1
      , ControlWord 2 0x04 0, ControlWord 3 0x04 0, ControlWord 3 0x0c 0, CountIs 1
      ]
      Returned Returned
  , Case "configure" Configure Equal [] [] [] 4 0 0 0x13 0
      Nothing Receipt
      [ ConfigurationPublished, RegionsUnchanged ["words", "configs", "controls", "count", "heap", "entries"] ]
      [] [] Returned Returned
  , Case "schedule" Schedule Equal [(1, Config 0x2 0x11111111 0x22222222 30 0 1)] [(1, Control 0 0 0 0 0)] [] 4 0 0 0x13 0
      Nothing Receipt
      [ ControlWord 1 0x08 55, ControlWord 1 0x0c 1, ControlWord 1 0x10 (expireEntry domain)
      , ControlArgumentIs 1, HeapWord 1 0x00 55, CountIs 1
      ]
      [] [] Returned Returned
  , Case "configure-unpublished" Configure Equal [] [] [] 4 0 0 0x13 0
      Nothing Receipt
      [ ConfigurationPublished, RegionsUnchanged ["words", "configs", "controls", "count", "heap", "entries"] ]
      [] [] Returned Returned
  , Case "create-unreadable-existing" Create Equal [(1, Config 0x2 0x11111111 0x22222222 30 0 1)] [] [] 4 0 0 0x13 1
      Nothing Receipt
      [ RegisterIs R0 0xffffffd7
      , ConfigWord 1 0x0c 30
      , ConfigWord 1 0x14 1
      , RegionsUnchanged ["words", "configs", "controls", "count", "heap", "entries"]
      ]
      [] [] Returned Returned
  ]
  where
    emptyConfig = Config 0 0 0 0 0 0

checksFor :: Role -> Case -> [Check]
checksFor role one
  | expectedStatus /= Returned = []
  | otherwise =
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
          Equal -> caseChecks one
          Repair _ -> if role == Stock then caseStockChecks one else caseOwnedChecks one
  where
    expectedStatus = if role == Stock then caseStockStatus one else caseOwnedStatus one

expectedCpsrControl :: Case -> Word32
expectedCpsrControl one = case caseCall one of
  Start
    | caseCpsr one .&. 0x80 /= 0 -> cpsrControl (caseCpsr one)
    | otherwise -> cpsrControl (caseCpsr one .&. complement 0x80)
  Stop
    | caseCpsr one .&. 0x80 /= 0 -> cpsrControl (caseCpsr one)
    | otherwise -> cpsrControl (caseCpsr one .&. complement 0x80)
  Tick -> cpsrControl (caseCpsr one .&. complement 0x80)
  _ -> cpsrControl (caseCpsr one)

check :: Facts -> Domain -> Case -> Role -> NativeObservation -> Check -> Either String ()
check facts domain one role observation check' = case check' of
  ConfigWord slot offset value -> expectWord "configs" (configAddress facts domain slot + offset) value
  ControlWord slot offset value -> expectWord "controls" (controlAddress facts domain slot + offset) value
  SecondControlWord slot offset value -> expectWord "entries" (secondControls + fromIntegral (slot - 1) * 0x18 + offset) value
  HeapWord index offset value -> expectWord "heap" (heapAddress facts domain + fromIntegral (index - 1) * 8 + offset) value
  CountIs value -> expectWord "words" (timerCountWord facts domain) value
  BaseIs value -> expectWord "words" (timerBaseWord facts domain) value
  TickIs value -> expectWord "words" (timerTickWord facts domain) value
  RegisterIs register value ->
    case lookupRegister register observation of
      Just actual | actual == value -> Right ()
      Just actual -> Left (registerLabel register ++ " is " ++ showHexWord actual ++ ", expected " ++ showHexWord value)
      Nothing -> Left ("missing register " ++ registerLabel register)
  ReceiptsAre expected -> do
    count <- readWord "entries" counterBase
    unless (count == fromIntegral (length expected)) (Left ("callback count " ++ show count ++ ", expected " ++ show (length expected)))
    actual <- forM [0 .. fromIntegral count - 1] $ \index -> do
      argument <- readWord "entries" (receiptBase + index * 4)
      lock <- readWord "entries" (receiptBase + 0x80 + index * 4)
      status <- readWord "entries" (receiptBase + 0x100 + index * 4)
      pure (argument, lock, cpsrControl status)
    unless (actual == expected) (Left ("receipts " ++ show actual ++ ", expected " ++ show expected))
  CpsrControlIs value ->
    case lookupRegister Cpsr observation of
      Just actual
        | cpsrControl actual == cpsrControl value -> Right ()
        | otherwise -> Left ("cpsr control " ++ showHexWord (cpsrControl actual) ++ ", expected " ++ showHexWord (cpsrControl value))
      Nothing -> Left "missing register cpsr"
  WordIs region address value -> expectWord region address value
  RegionsUnchanged regions -> do
    forM_ regions $ \region -> do
      actual <- readRegion region
      unless (actual == initialRegion region) (Left ("region " ++ region ++ " changed"))
  ConfigurationPublished -> do
    let base = wordAddress (configurationAddress domain)
    expectWord "configuration" (base + 0x60) (controlBase facts domain)
    expectWord "configuration" (base + 0x64) (caseCount one)
    expectWord "configuration" (base + 0x6c) (expireEntry domain)
  ControlArgumentIs slot -> expectWord "controls" (controlAddress facts domain slot + 0x14) (controlAddress facts domain slot)
  HeapEntryIs index -> expectWord "heap" (heapAddress facts domain + fromIntegral (index - 1) * 8 + 4) (controlTimer facts domain index)
  where
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
      "words" -> overlay (wordsBase domain) 0x1000 (wordsWords facts domain one)
      "configs" -> overlay (configBase facts domain) 0x60 (configWords facts domain one (handlerFor role one))
      "controls" -> overlay (controlBase facts domain) 0x60 (controlWords facts domain one)
      "count" -> overlay (countWord facts domain) 4 [(countWord facts domain, caseCount one)]
      "heap" -> overlay (heapAddress facts domain) 0x100 (heapWords facts domain one)
      "entries" -> overlay entriesBase entriesSize (entriesWords facts domain one)
      "configuration" -> B.replicate 0x70 0xa5
      _ -> B.empty
    regionBase "words" = wordsBase domain
    regionBase "configs" = configBase facts domain
    regionBase "controls" = controlBase facts domain
    regionBase "count" = countWord facts domain
    regionBase "heap" = heapAddress facts domain
    regionBase "entries" = entriesBase
    regionBase "configuration" = wordAddress (configurationAddress domain)
    regionBase _ = 0

showHexWord :: Word32 -> String
showHexWord value = "0x" ++ showHex' value
  where
    showHex' 0 = "0"
    showHex' n = reverse (go n)
    go 0 = []
    go n = let (q, r) = n `divMod` 16 in "0123456789abcdef" !! fromIntegral r : go q

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

wordsWords :: Facts -> Domain -> Case -> [(Word32, Word32)]
wordsWords facts domain one =
  [ (dfHandlerContext d, caseHandlerContext one)
  , (dfCurrentContext d, if caseName one == "configure-unpublished" then 0 else dfContexts d)
  , (dfSchedulerLock d, 0)
  ]
    ++ contextWords (factsContext facts) d (dfContexts d) (controlBase facts domain) (caseCount one) (expireEntry domain)
    ++ contextWords (factsContext facts) d (dfContexts d + 0x84) (controlBase facts domain) (caseCount one) (expireEntry domain)
    ++ [(configurationWordAddress domain, wordAddress (configurationAddress domain)) | caseCall one == Configure]
    ++ [ (dfTimerCount d, fromIntegral (length linked))
       , (dfTimerBase d, 0)
       , (dfTickNow d, caseTick one)
       ]
  where
    d = domainFacts facts domain
    linked = [(slot, control) | (slot, control) <- caseControls one, ctlIndex control /= 0]

contextWords :: ContextOffsets -> DomainFacts -> Word32 -> Word32 -> Word32 -> Word32 -> [(Word32, Word32)]
contextWords offsets d context controls count callback =
  [ (context + contextSchedulerLock offsets, lockCell)
  , (context + contextSchedulerLock offsets + 4, stubAcquire)
  , (context + contextSchedulerLock offsets + 8, stubRelease)
  , (context + contextTimerHeap offsets, dfTimerHeap d)
  , (context + contextTimerCount offsets, dfTimerCount d)
  , (context + contextTimerBase offsets, dfTimerBase d)
  , (context + contextTickNow offsets, dfTickNow d)
  , (context + contextCyclics offsets, controls)
  , (context + contextCyclicCount offsets, count)
  , (context + contextCyclicCallback offsets, callback)
  ]

configWords :: Facts -> Domain -> Case -> Word32 -> [(Word32, Word32)]
configWords facts domain one handler =
  concat
    [ [ (configAddress facts domain slot, cfgAttributes config)
      , (configAddress facts domain slot + 0x04, cfgArgument config)
      , (configAddress facts domain slot + 0x08, if cfgExists config /= 0 then handler else cfgHandler config)
      , (configAddress facts domain slot + 0x0c, cfgPeriod config)
      , (configAddress facts domain slot + 0x10, cfgPhase config)
      , (configAddress facts domain slot + 0x14, cfgExists config)
      ]
    | (slot, config) <- caseConfigs one
    ]

handlerFor :: Role -> Case -> Word32
handlerFor role one
  | caseName one == "expire-zero-period" && role == Stock = stubBase + 0x180
  | otherwise = case caseCallback one of
      Receipt -> stubReceipt
      Sink -> stubBase + 0x180
      HandlerDelete -> stubDelete
      HandlerDeleteRecreate -> stubDeleteRecreate

controlWords :: Facts -> Domain -> Case -> [(Word32, Word32)]
controlWords facts domain one =
  concat
    [ [ (controlAddress facts domain slot, configAddress facts domain slot)
      , (controlAddress facts domain slot + 0x04, ctlActive control)
      , (controlAddress facts domain slot + 0x08, ctlExpiry control)
      , (controlAddress facts domain slot + 0x0c, ctlIndex control)
      , (controlAddress facts domain slot + 0x10, if ctlCallback control == 0 then expireEntry domain else ctlCallback control)
      , (controlAddress facts domain slot + 0x14, if ctlArgument control == 0 then controlAddress facts domain slot else ctlArgument control)
      ]
    | (slot, control) <- caseControls one
    ]

secondControlWords :: Facts -> Domain -> Case -> [(Word32, Word32)]
secondControlWords facts domain one =
  concat
    [ [ (secondControls + fromIntegral (slot - 1) * 0x18, configAddress facts domain slot)
      , (secondControls + fromIntegral (slot - 1) * 0x18 + 0x04, ctlActive control)
      , (secondControls + fromIntegral (slot - 1) * 0x18 + 0x08, ctlExpiry control)
      , (secondControls + fromIntegral (slot - 1) * 0x18 + 0x0c, ctlIndex control)
      , (secondControls + fromIntegral (slot - 1) * 0x18 + 0x10, if ctlCallback control == 0 then expireEntry domain else ctlCallback control)
      , (secondControls + fromIntegral (slot - 1) * 0x18 + 0x14, if ctlArgument control == 0 then secondControls + fromIntegral (slot - 1) * 0x18 else ctlArgument control)
      ]
    | (slot, control) <- caseSecondControls one
    ]

heapWords :: Facts -> Domain -> Case -> [(Word32, Word32)]
heapWords facts domain one =
  concat
      [ [ (heapAddress facts domain + fromIntegral (index - 1) * 8, ctlExpiry control)
        , (heapAddress facts domain + fromIntegral (index - 1) * 8 + 4, controlTimer facts domain slot)
        ]
      | (index, (slot, control)) <- zip [1 :: Int ..] linked
      ]
  where
    linked = [(slot, control) | (slot, control) <- caseControls one, ctlIndex control /= 0]

entriesWords :: Facts -> Domain -> Case -> [(Word32, Word32)]
entriesWords facts domain one =
  [ (lockCell, 0)
  , (counterBase, 0)
  ]
    ++ [ (receiptBase + fromIntegral index * 4, 0) | index <- [0 .. 7 :: Int] ]
    ++ [ (receiptBase + 0x80 + fromIntegral index * 4, 0) | index <- [0 .. 7 :: Int] ]
    ++ [ (receiptBase + 0x100 + fromIntegral index * 4, 0) | index <- [0 .. 7 :: Int] ]
    ++ [ (receiptBase + 0x180 + fromIntegral index * 4, 0) | index <- [0 .. 7 :: Int] ]
    ++ secondControlWords facts domain one
    ++ requestWords
  where
    requestWords = case caseRequest one of
      Nothing -> []
      Just (attributes, argument, handler, period, phase) ->
        [ (requestBase, attributes)
        , (requestBase + 4, argument)
        , (requestBase + 8, handler)
        , (requestBase + 0xc, period)
        , (requestBase + 0x10, phase)
        ]

caseArgs :: Facts -> Domain -> Case -> [(Register, Word32)]
caseArgs facts domain one = case caseCall one of
  Configure -> []
  Initialize -> []
  Tick -> []
  Schedule -> [(R0, dfContexts d), (R1, controlAddress facts domain 1), (R2, 55)]
  Expire -> [(R0, controlAddress facts domain 1)]
  Start -> [(R0, caseNumber one)]
  Stop -> [(R0, caseNumber one)]
  Create -> [(R0, caseNumber one), (R1, if caseName one == "create-unreadable-existing" then 0x0eadbe0 else requestBase)]
  Delete -> [(R0, caseNumber one)]
  where
    d = domainFacts facts domain

caseFor :: Facts -> Domain -> Role -> Image -> Image -> Image -> [(Word32, Word32)] -> [(Word32, Word32)] -> Case -> CaseSpec
caseFor facts domain role stockImage ownedImage timerOwned ownedSymbols timerSymbols one =
  CaseSpec
    { caseId = caseName one ++ "-" ++ domainLabel domain
    , caseProvenance = "X-M1 1.50 cyclic stock ABI, x70view 55436fc85c0560dd252b0410af6f2427fe198cd3; synthetic tables, heap and callback stubs"
    , caseExclusion = "Synthetic cyclic tables and callback stubs; no firmware heap, wait-result or system integration."
    , caseEntry = GuestAddress (fromIntegral entry)
    , caseExit = GuestAddress (fromIntegral exitAddress)
    , caseRegions =
        [ mkRegion (RegionId "code") (GuestAddress (fromIntegral codeBase)) (fromIntegral (B.length codeBytes)) ReadExecute codeBytes
        , mkRegion (RegionId "timer") (GuestAddress (fromIntegral timerBase)) (fromIntegral (B.length timerBytes)) ReadExecute timerBytes
        , mkRegion (RegionId "stubs") (GuestAddress (fromIntegral stubBase)) (fromIntegral stubSize) ReadExecute (stubBytes domain one)
        , mkRegion (RegionId "words") (GuestAddress (fromIntegral (wordsBase domain))) 0x1000 ReadWrite (overlay (wordsBase domain) 0x1000 (wordsWords facts domain one))
        , mkRegion (RegionId "configs") (GuestAddress (fromIntegral (configBase facts domain))) 0x60 ReadWrite (overlay (configBase facts domain) 0x60 (configWords facts domain one handler))
        , mkRegion (RegionId "controls") (GuestAddress (fromIntegral (controlBase facts domain))) 0x60 ReadWrite (overlay (controlBase facts domain) 0x60 (controlWords facts domain one))
        , mkRegion (RegionId "count") (GuestAddress (fromIntegral (countWord facts domain))) 4 ReadWrite (overlay (countWord facts domain) 4 [(countWord facts domain, caseCount one)])
        , mkRegion (RegionId "heap") (GuestAddress (fromIntegral (heapAddress facts domain))) 0x100 ReadWrite (overlay (heapAddress facts domain) 0x100 (heapWords facts domain one))
        , mkRegion (RegionId "entries") (GuestAddress (fromIntegral entriesBase)) (fromIntegral entriesSize) ReadWrite (overlay entriesBase entriesSize (entriesWords facts domain one))
        , mkRegion (RegionId "stack") (GuestAddress (fromIntegral stackBase)) (fromIntegral stackSize) ReadWrite (B.replicate (fromIntegral stackSize) 0)
        , mkRegion (RegionId "exit") (GuestAddress (fromIntegral exitAddress)) 0x10 ReadExecute (B.replicate 0x10 0)
        ]
          ++ [ mkRegion configurationRegion (configurationAddress domain) 0x70 ReadWrite (B.replicate 0x70 0xa5)
             | caseCall one == Configure
             ]
          ++ [ mkRegion (RegionId "support") (GuestAddress (fromIntegral (supportLow domain))) 0x68 ReadExecute
                 (slice (imageBase stockImage) (imageBytes stockImage) (supportLow domain) 0x68)
             | role == Stock
             ]
    , caseFunctions = [(GuestAddress (fromIntegral address), size) | (address, size) <- functions]
    , caseInitial =
        [ (register, case lookup register (caseArgs facts domain one) of
            Just value -> value
            Nothing -> seedRegister register)
        | register <- [R0 .. R12]
        ]
          ++ [(Sp, stackPointer), (Lr, exitAddress), (Cpsr, caseCpsr one)]
    , caseExpectedStatus = status
    , caseExpectedRegisters =
        if status == Returned
          then
            [ (R4, seedRegister R4), (R5, seedRegister R5), (R6, seedRegister R6), (R7, seedRegister R7)
            , (R8, seedRegister R8), (R9, seedRegister R9), (R10, seedRegister R10), (R11, seedRegister R11)
            , (Sp, stackPointer)
            ]
          else [(Sp, stackPointer)]
    , caseExpectedMemory = []
    , caseCp14Identity = Just (if domain == DomainA then 0x4003 else 0x4004)
    , caseKernel =
        if status == Returned
          then Just (KernelCase plainLimit Nothing (Just 0x80001234) (Just 0) (Just 0) [R1, R2, R3, R12, Lr] [])
          else Nothing
    }
  where
    entry = callEntry domain (caseCall one)
    status = if role == Stock then caseStockStatus one else caseOwnedStatus one
    handler = handlerFor role one
    (codeBase, codeBytes) = case role of
      Stock -> (cyclicLow domain, slice (imageBase stockImage) (imageBytes stockImage) (cyclicLow domain) (cyclicSize domain))
      Owned -> (imageBase ownedImage, imageBytes ownedImage)
    (timerBase, timerBytes) = case role of
      Stock -> (timerLow domain, slice (imageBase stockImage) (imageBytes stockImage) (timerLow domain) (timerSize domain))
      Owned -> (timerLow domain, imageBytes timerOwned)
    functions = case role of
      Stock -> stockFunctions domain ++ [(stubBase, stubSize)]
      Owned -> ownedSymbols ++ timerSymbols ++ [(stubBase, stubSize)]

stockFunctions :: Domain -> [(Word32, Word32)]
stockFunctions domain =
  [ (configureEntry domain, 0x48)
  , (scheduleEntry domain, 0x2c)
  , (expireEntry domain, 0x8c)
  , (initializeEntry domain, 0x90)
  , (startEntry domain, 0x170)
  , (stopEntry domain, 0x150)
  , (createEntry domain, 0x90)
  , (deleteEntry domain, 0x54)
  , (timerInsert domain, 0x48)
  , (timerRemove domain, 0xe4)
  , (tickEntry domain, 0x1a0)
  , (maskEntry domain, 0x08)
  , (restoreEntry domain, 0x60)
  , (siftUpEntry domain, 0xc4)
  , (siftDownEntry domain, 0xdc)
  ]

supportLow :: Domain -> Word32
supportLow DomainA = 0x42560
supportLow DomainB = 0xd2560

maskEntry, restoreEntry, siftUpEntry, siftDownEntry :: Domain -> Word32
maskEntry DomainA = 0x42560
maskEntry DomainB = 0xd2560
restoreEntry DomainA = 0x42568
restoreEntry DomainB = 0xd2568
siftUpEntry DomainA = 0x44ac0
siftUpEntry DomainB = 0xd4a80
siftDownEntry DomainA = 0x44b84
siftDownEntry DomainB = 0xd4b44

timerInsert, timerRemove :: Domain -> Word32
timerInsert DomainA = 0x44c60
timerInsert DomainB = 0xd4c20
timerRemove DomainA = 0x44ca8
timerRemove DomainB = 0xd4c68

slice :: Word32 -> B.ByteString -> Word32 -> Word32 -> B.ByteString
slice base bytes address size
  | address < base || address + size > base + fromIntegral (B.length bytes) =
      error ("cyclic code slice outside its image: base=" ++ show base ++ " length=" ++ show (B.length bytes) ++ " address=" ++ show address ++ " size=" ++ show size)
  | otherwise = B.take (fromIntegral size) (B.drop (fromIntegral (address - base)) bytes)

regionDifferences :: Domain -> [RegionId] -> NativeObservation -> NativeObservation -> [String]
regionDifferences _ regions stock owned =
  [ "region " ++ regionName ident ++ " differs"
  | ident <- regions
  , M.lookup ident (M.fromList (nativeMemory stock)) /= M.lookup ident (M.fromList (nativeMemory owned))
  ]

runDomain
  :: FilePath
  -> FilePath
  -> FilePath
  -> Facts
  -> Domain
  -> Image
  -> Image
  -> Image
  -> [(Word32, Word32)]
  -> [(Word32, Word32)]
  -> IO ([CaseRun], [ControlRun])
runDomain runner cacheRoot directory facts domain stockImage ownedImage timerOwned ownedSymbols timerSymbols = do
  let chosen = cases facts domain
      stockSpecs = map (caseFor facts domain Stock stockImage ownedImage timerOwned ownedSymbols timerSymbols) chosen
      ownedSpecs = map (caseFor facts domain Owned stockImage ownedImage timerOwned ownedSymbols timerSymbols) chosen
  stockBatch <- writeBatch (directory </> "stock") stockSpecs
  ownedBatch <- writeBatch (directory </> "owned") ownedSpecs
  (stockReuse, stockRaw) <- cachedObservedBatch cacheRoot runner stockBatch
  (ownedReuse, ownedRaw) <- cachedObservedBatch cacheRoot runner ownedBatch
  let reference = M.fromList [(caseIdText ident, observation) | (ident, observation) <- stockRaw]
      subject = M.fromList [(caseIdText ident, observation) | (ident, observation) <- ownedRaw]
  results <- forM chosen $ \one -> do
    let ident = caseName one ++ "-" ++ domainLabel domain
        stock = M.lookup ident reference
        owned = M.lookup ident subject
        expectedStatus role = if role == Stock then caseStockStatus one else caseOwnedStatus one
        roleChecks role observation =
          ("status " ++ runnerStatusLabel (expectedStatus role), nativeStatus observation == expectedStatus role)
            : map (\check' -> (describe check', either (const False) (const True) (check facts domain one role observation check')))
              (checksFor role one)
        errorsFor role observation =
          [ "status " ++ runnerStatusLabel (nativeStatus observation) ++ ", expected " ++ runnerStatusLabel (expectedStatus role)
          | nativeStatus observation /= expectedStatus role
          ]
            ++ [message | Left message <- map (check facts domain one role observation) (checksFor role one)]
        equalDifferences stock' owned' = case caseKind one of
          Equal ->
            regionDifferences domain
              ([RegionId "words", RegionId "configs", RegionId "controls", RegionId "count", RegionId "heap", RegionId "entries"]
                 ++ [configurationRegion | caseCall one == Configure])
              stock'
              owned'
          _ -> []
        run = CaseRun
          { caseDomain = domain
          , runCohort = "core"
          , runName = caseName one
          , runKind = caseKind one
          , runChecks = case (stock, owned) of
              (Just stock', Just owned') -> roleChecks Stock stock' ++ roleChecks Owned owned'
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
          Just observed -> controlRuns facts domain one observed
          Nothing -> []
    pure (run, controls)
  pure (map fst results, concatMap snd results)

controlRuns :: Facts -> Domain -> Case -> NativeObservation -> [ControlRun]
controlRuns facts domain one observation =
  [ ControlRun "core" "cpsr-i" (caseName one) (refused (CpsrControlIs expected) (mutateCpsr (`xor` 0x80) observation))
  , ControlRun "core" "cpsr-f" (caseName one) (refused (CpsrControlIs expected) (mutateCpsr (`xor` 0x40) observation))
  ]
    ++ [ ControlRun "core" ("unchanged-" ++ region) (caseName one) (refused (RegionsUnchanged [region]) (mutateRegion region observation))
       | region <- declaredRegions
       ]
    ++ [ ControlRun "core" "receipt-argument" (caseName one) (refused (ReceiptsAre expectedReceipt) (mutateReceipt observation))
       | ReceiptsAre expectedReceipt <- checksFor Owned one
       ]
  where
    expected = expectedCpsrControl one
    refused check' mutated = case check facts domain one Owned mutated check' of
      Left _ -> True
      Right () -> False
    mutateCpsr f observed = observed {nativeRegisters = M.adjust f Cpsr (nativeRegisters observed)}
    declaredRegions = [region | RegionsUnchanged regions <- checksFor Owned one, region <- regions]
    mutateRegion region observed =
      observed
        { nativeMemory =
            [ (ident, if regionName ident == region then flipHead bytes else bytes)
            | (ident, bytes) <- nativeMemory observed
            ]
        }
    mutateReceipt observed =
      observed
        { nativeMemory =
            [ (ident, if regionName ident == "entries" then flipAt 0x200 bytes else bytes)
            | (ident, bytes) <- nativeMemory observed
            ]
        }
    flipAt offset bytes = let (before, after) = B.splitAt offset bytes in before <> flipHead after
    flipHead bytes = case B.uncons bytes of
      Just (first, rest) -> B.cons (first `xor` (0xff :: Word8)) rest
      Nothing -> bytes

describe :: Check -> String
describe check' = case check' of
  ConfigWord slot offset value -> "config " ++ show slot ++ "+" ++ show offset ++ " = " ++ showHexWord value
  ControlWord slot offset value -> "control " ++ show slot ++ "+" ++ show offset ++ " = " ++ showHexWord value
  SecondControlWord slot offset value -> "second control " ++ show slot ++ "+" ++ show offset ++ " = " ++ showHexWord value
  HeapWord index offset value -> "heap " ++ show index ++ "+" ++ show offset ++ " = " ++ showHexWord value
  CountIs value -> "count = " ++ show value
  BaseIs value -> "base = " ++ show value
  TickIs value -> "tick = " ++ show value
  RegisterIs register value -> registerLabel register ++ " = " ++ showHexWord value
  ReceiptsAre values -> "receipts = " ++ show values
  CpsrControlIs value -> "cpsr control = " ++ showHexWord (cpsrControl value)
  WordIs region address value -> region ++ " " ++ showHexWord address ++ " = " ++ showHexWord value
  RegionsUnchanged regions -> "unchanged " ++ unwords regions
  ConfigurationPublished -> "configuration pointers"
  ControlArgumentIs slot -> "control " ++ show slot ++ " argument"
  HeapEntryIs index -> "heap " ++ show index ++ " entry"
