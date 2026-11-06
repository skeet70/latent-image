{-# LANGUAGE OverloadedStrings #-}

-- | The ten F2 family-native cases over both entry domains, each run against the stock
-- loader bytes and the owned image and compared observation by observation.
--
-- Addresses come from 'Facts', which parses @layout.json@. The +0xd4 contract stays
-- refused in the manifest; these cases are the oracle hypothesis.
module Replacement.RtosObjects.Suite
  ( Family (..)
  , Case (..)
  , cases
  , caseFor
  , extents
  , compareFamily
  , runFamily
  , FamilyRun (..)
  ) where

import Control.Monad (forM)
import qualified Data.ByteString as B
import Data.Bits (shiftR, (.&.))
import qualified Data.Map.Strict as M
import Data.List (nub)
import Data.Maybe (fromMaybe)
import Data.Word (Word32)
import Emulation.NativeObservation (GuestAddress (..), NativeObservation (..), RegionId (..), Register (..))
import Emulation.OwnedCase (CaseRegion (..), CaseSpec (..), Permission (..), mkRegion)
import Replacement.RtosObjects.Layout
import Replacement.RtosOwnFirst.Regions (Domain (..))
import Replacement.RtosScheduler.Oracle (runCases, stripScratch)
import System.FilePath ((</>))

-- | Which of the two stock entries the case calls.
data Family = SetFlag | Send
  deriving (Eq, Show)

-- | One case: the words it plants and the registers it enters with. The stock and owned
-- bodies run the same setup and must observe the same result.
data Case = Case
  { caseName :: String
  , caseFamily :: Family
  , caseSlot :: Word32
  , caseRegisters :: [(Register, Word32)]
  , caseWords :: [(Word32, Word32)]
  , caseTimer :: Word32
  }

-- | The image a body is taken from: the stock loader at its load base, or the owned stage
-- bin at its lowest region.
data Image = Image
  { imageBase :: Word32
  , imageBytes :: B.ByteString
  }

-- | The ten cases of reports/rtos-platform-options.md 4.14.6.
cases :: Facts -> Domain -> [Case]
cases facts domain =
  let df = domainFacts facts domain
      r = record facts domain
      flag = flagConfig facts domain 1
      control = flagControl facts domain 1
      wait = flagWait facts domain 1
      mailbox = mailboxConfig facts domain 1
      mailControl = mailboxControl facts domain 1
      mailWait = mailboxWait facts domain 1
      t1 = task facts domain 1
      own = case domain of DomainA -> 1; DomainB -> 2
      peer = case domain of DomainA -> 2; DomainB -> 1
      head = readyHead facts domain 3
      dispatch = dfDispatchEnabled df
      doorbell = case domain of DomainA -> 0xfffb40cc; DomainB -> 0xfffb40cc
      waitTask state = [(t1 + factsTaskNext facts, t1), (t1 + factsTaskPrev facts, t1), (t1 + factsTaskState facts, state), (t1 + factsTaskPriority facts, 3), (t1 + factsTaskDomain facts, own), (t1 + factsTaskWait facts, wait)]
      flagBase = [(flag + factsFlagConfigExists facts, 1), (control + factsWaitQueueNext facts, control), (control + factsWaitQueuePrev facts, control), (control + factsFlagControlPattern facts, 0)]
   in [ Case "set_no_waiter" SetFlag 1 [(R1, 1)] (flagBase ++ [(head, head), (head + 4, head), (r + 0x20, dfReadyHeads df), (r + 0x24, dfReadyBitmap df), (r + 0x28, dfRunning df), (r + 0x2c, dfSelected df), (r + 0x30, dispatch), (dispatch, 1)]) 0
      , Case "set_satisfied" SetFlag 1 [(R1, 1)] (flagBase ++ [(control + factsWaitQueueNext facts, t1), (wait + factsWaitBlockQueue facts, control), (wait + factsFlagWaitPattern facts, 1), (wait + factsFlagWaitMode facts, 1)] ++ waitTask (fromIntegral (factsStateWait facts) + fromIntegral (factsStateObjectQueued facts)) ++ [(head, head), (head + 4, head), (r + 0x20, dfReadyHeads df), (r + 0x24, dfReadyBitmap df), (r + 0x28, dfRunning df), (r + 0x2c, dfSelected df), (r + 0x30, dispatch), (dispatch, 1)]) 0
      , Case "set_unsatisfied" SetFlag 1 [(R1, 1)] (flagBase ++ [(control + factsWaitQueueNext facts, t1), (wait + factsWaitBlockQueue facts, control), (wait + factsFlagWaitPattern facts, 2), (wait + factsFlagWaitMode facts, 1)] ++ waitTask (fromIntegral (factsStateWait facts) + fromIntegral (factsStateObjectQueued facts)) ++ [(head, head), (head + 4, head), (r + 0x20, dfReadyHeads df), (r + 0x24, dfReadyBitmap df), (r + 0x28, dfRunning df), (r + 0x2c, dfSelected df), (r + 0x30, dispatch), (dispatch, 1)]) 0
      , Case "set_clear_on_match" SetFlag 1 [(R1, 1)] (flagBase ++ [(flag + factsFlagConfigAttributes facts, fromIntegral (factsFlagClearOnMatch facts)), (control + factsWaitQueueNext facts, t1), (wait + factsWaitBlockQueue facts, control), (wait + factsFlagWaitPattern facts, 1), (wait + factsFlagWaitMode facts, 1)] ++ waitTask (fromIntegral (factsStateWait facts) + fromIntegral (factsStateObjectQueued facts)) ++ [(head, head), (head + 4, head), (r + 0x20, dfReadyHeads df), (r + 0x24, dfReadyBitmap df), (r + 0x28, dfRunning df), (r + 0x2c, dfSelected df), (r + 0x30, dispatch), (dispatch, 1)]) 0
      , Case "set_cross_domain" SetFlag 1 [(R1, 1)] (flagBase ++ [(control + factsWaitQueueNext facts, t1), (wait + factsWaitBlockQueue facts, control), (wait + factsFlagWaitPattern facts, 1), (wait + factsFlagWaitMode facts, 1)] ++ waitTask (fromIntegral (factsStateWait facts) + fromIntegral (factsStateObjectQueued facts)) ++ [(t1 + factsTaskDomain facts, peer), (head, head), (head + 4, head), (r + 0x20, dfReadyHeads df), (r + 0x24, dfReadyBitmap df), (r + 0x28, dfRunning df), (r + 0x2c, dfSelected df), (r + 0x30, dispatch), (dispatch, 1), (doorbell, 0)]) 0
      , Case "set_timer_cancel" SetFlag 1 [(R1, 1)] (flagBase ++ [(control + factsWaitQueueNext facts, t1), (wait + factsWaitBlockResult facts, timerEntry domain), (wait + factsWaitBlockQueue facts, control), (wait + factsFlagWaitPattern facts, 1), (wait + factsFlagWaitMode facts, 1)] ++ waitTask (fromIntegral (factsStateWait facts) + fromIntegral (factsStateObjectQueued facts)) ++ [(head, head), (head + 4, head), (r + 0x20, dfReadyHeads df), (r + 0x24, dfReadyBitmap df), (r + 0x28, dfRunning df), (r + 0x2c, dfSelected df), (r + 0x30, dispatch), (dispatch, 1)]) 0
      , Case "send_blocked" Send 1 [(R1, messageAddress)] ([(mailbox + factsMailboxConfigExists facts, 1), (mailControl + factsWaitQueueNext facts, t1), (mailWait + factsWaitBlockQueue facts, mailControl), (messageAddress, 0), (messageAddress + 4, 0)] ++ waitTask (fromIntegral (factsStateWait facts) + fromIntegral (factsStateObjectQueued facts)) ++ [(head, head), (head + 4, head), (r + 0x20, dfReadyHeads df), (r + 0x24, dfReadyBitmap df), (r + 0x28, dfRunning df), (r + 0x2c, dfSelected df), (r + 0x30, dispatch), (dispatch, 1)]) 0
      , Case "send_empty_fifo" Send 1 [(R1, messageAddress)] ([(mailbox + factsMailboxConfigExists facts, 1), (mailControl + factsMailboxControlHead facts, 0), (mailControl + factsMailboxControlTail facts, 0), (mailControl + factsWaitQueueNext facts, mailControl), (mailControl + factsWaitQueuePrev facts, mailControl), (messageAddress, 0), (messageAddress + 4, 0)] ++ [(head, head), (head + 4, head), (r + 0x20, dfReadyHeads df), (r + 0x24, dfReadyBitmap df), (r + 0x28, dfRunning df), (r + 0x2c, dfSelected df), (r + 0x30, dispatch), (dispatch, 1)]) 0
      , Case "send_priority_equal" Send 1 [(R1, messageAddress)] ([(mailbox + factsMailboxConfigExists facts, 1), (mailbox + factsMailboxConfigAttributes facts, fromIntegral (factsMailboxMessagePriority facts)), (mailControl + factsMailboxControlHead facts, queued), (mailControl + factsMailboxControlTail facts, queued), (mailControl + factsWaitQueueNext facts, mailControl), (queued, 0), (queued + 4, 5), (messageAddress, 0), (messageAddress + 4, 5)] ++ [(head, head), (head + 4, head), (r + 0x20, dfReadyHeads df), (r + 0x24, dfReadyBitmap df), (r + 0x28, dfRunning df), (r + 0x2c, dfSelected df), (r + 0x30, dispatch), (dispatch, 1)]) 0
      , Case "send_cross_domain" Send 1 [(R1, messageAddress)] ([(mailbox + factsMailboxConfigExists facts, 1), (mailControl + factsWaitQueueNext facts, t1), (mailWait + factsWaitBlockQueue facts, mailControl), (messageAddress, 0)] ++ waitTask (fromIntegral (factsStateWait facts) + fromIntegral (factsStateObjectQueued facts)) ++ [(t1 + factsTaskDomain facts, peer), (head, head), (head + 4, head), (r + 0x20, dfReadyHeads df), (r + 0x24, dfReadyBitmap df), (r + 0x28, dfRunning df), (r + 0x2c, dfSelected df), (r + 0x30, dispatch), (dispatch, 1), (doorbell, 0)]) 0
      , Case "send_message_next" Send 1 [(R1, messageAddress)] ([(mailbox + factsMailboxConfigExists facts, 1), (mailControl + factsMailboxControlHead facts, 0), (mailControl + factsMailboxControlTail facts, 0), (mailControl + factsWaitQueueNext facts, mailControl), (mailControl + factsWaitQueuePrev facts, mailControl), (messageAddress, 0), (messageAddress + 4, 0)] ++ [(head, head), (head + 4, head), (r + 0x20, dfReadyHeads df), (r + 0x24, dfReadyBitmap df), (r + 0x28, dfRunning df), (r + 0x2c, dfSelected df), (r + 0x30, dispatch), (dispatch, 1)]) 0
      ]
  where
    queued = messageAddress + 8

-- | The message a send case passes in r1.
messageAddress :: Word32
messageAddress = 0x40000000

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

-- | The stock extents of S1, S2 and the two F2 bodies.
extents :: Domain -> [(Word32, Word32)]
extents DomainA =
  [ (0x40b00, 0x24), (0x43640, 0x28), (0x43668, 0x20), (0x43688, 0xa0), (0x43728, 0x34), (0x4375c, 0x48), (0x437a4, 0x38), (0x437fc, 0x14)
  , (0x439d8, 0x3c), (0x43a14, 0x28), (0x43c48, 0x5c), (0x43ca4, 0x7c), (0x43d20, 0xb0), (0x43dd0, 0x48), (0x43f70, 0x2c), (0x44018, 0x2c), (0x44044, 0xf0), (0x44134, 0x80)
  , (0x4813c, 0x260), (0x4aa44, 0x304)
  ]
extents DomainB =
  [ (0xd0b00, 0x24), (0xd3640, 0x28), (0xd3668, 0x20), (0xd3688, 0xa0), (0xd3728, 0x34), (0xd375c, 0x48), (0xd37a4, 0x38), (0xd37fc, 0x14)
  , (0xd398c, 0x3c), (0xd39c8, 0x28), (0xd3c04, 0x5c), (0xd3c60, 0x7c), (0xd3cdc, 0xb0), (0xd3d8c, 0x48), (0xd3f2c, 0x2c), (0xd3fd4, 0x30), (0xd4004, 0xf0), (0xd40f4, 0x80)
  , (0xd80fc, 0x260), (0xdaa04, 0x304)
  ]

-- | The page a case word falls in, with every word of the case that falls in it.
page :: Word32 -> [(Word32, Word32)] -> CaseRegion
page base words =
  let planted = [(address, value) | (address, value) <- words, address >= base, address < base + 0x1000]
   in mkRegion (RegionId ("page-" ++ show base)) (GuestAddress (fromIntegral base)) 0x1000 ReadWrite (plant base 0x1000 planted)

plant :: Word32 -> Int -> [(Word32, Word32)] -> B.ByteString
plant base size words =
  B.pack
    [ fromIntegral (value `shiftR` shift .&. 0xff)
    | offset <- [0 .. size - 1]
    , let address = base + fromIntegral offset
    , let value = fromMaybe 0 (lookup address words)
    , let shift = 8 * (offset `mod` 4)
    ]

-- | The case's spec over one image: the stock loader or the owned stage bin.
caseFor :: Facts -> Domain -> Image -> Case -> CaseSpec
caseFor facts domain image one =
  CaseSpec
    { caseId = caseName one ++ "-" ++ show domain
    , caseProvenance = "X-M1 1.50 loader " ++ show domain ++ " " ++ show (caseFamily one) ++ " over layout.json and the rtos_layout stock data layout"
    , caseExclusion = ""
    , caseEntry = GuestAddress (fromIntegral (entryAddress domain (caseFamily one)))
    , caseExit = GuestAddress 0x958f00
    , caseRegions =
        [mkRegion (RegionId ("code-" ++ show base)) (GuestAddress (fromIntegral base)) (fromIntegral size) ReadExecute (slice base size) | (base, size) <- extents domain]
          ++ [ mkRegion (RegionId "timer") (GuestAddress (fromIntegral (timerEntry domain))) 8 ReadExecute (stub (caseTimer one))
             , mkRegion (RegionId "raise") (GuestAddress (fromIntegral (raiseIrqEntry domain))) 8 ReadExecute (stub 0)
             ]
          ++ [page base (caseWords one) | base <- nub (map (\address -> address .&. 0xfffff000) (map fst (caseWords one)))]
          ++ [mkRegion (RegionId "doorbell") (GuestAddress 0xfffb4000) 0x1000 Device (B.replicate 0x1000 0)]
          ++ [mkRegion (RegionId "stack") (GuestAddress 0x959700) 0x110 ReadWrite (B.replicate 0x110 0)]
    , caseFunctions = [(GuestAddress (fromIntegral base), size) | (base, size) <- extents domain]
    , caseInitial = [(R0, caseSlot one)] ++ caseRegisters one ++ [(Sp, 0x959800), (Lr, 0x958f00), (Cpsr, 0x13)]
    , caseExpectedStatus = Returned
    , caseExpectedRegisters = []
    , caseExpectedMemory = []
    , caseCp14Identity = Nothing
    , caseKernel = Nothing
    }
  where
    slice base size = B.take (fromIntegral size) (B.drop (fromIntegral (base - imageBase image)) (imageBytes image))

stub :: Word32 -> B.ByteString
stub code = B.pack (concatMap littleEndian [0xe3a00000 .|. (code .&. 0xff), 0xe12fff1e])
  where
    littleEndian word = [fromIntegral (word `shiftR` shift) | shift <- [0, 8, 16, 24]]

-- | The stock and owned observations of one case.
compareFamily :: NativeObservation -> NativeObservation -> [String]
compareFamily stock owned =
  statusDifference ++ registerDifferences ++ memoryDifferences
  where
    statusDifference = ["status " ++ show (nativeStatus owned) ++ " expected " ++ show (nativeStatus stock) | nativeStatus owned /= nativeStatus stock]
    registerDifferences =
      [ "r" ++ show r ++ " is " ++ show (M.findWithDefault 0 r (nativeRegisters owned)) ++ " expected " ++ show expected
      | r <- [R0, R1, R2, R3, R12]
      , let expected = M.findWithDefault 0 r (nativeRegisters stock)
      , M.findWithDefault 0 r (nativeRegisters owned) /= expected
      ]
    memoryDifferences =
      [ "page " ++ regionName ident ++ " differs"
      | (ident, expected) <- nativeMemory stock
      , not ("code-" `prefix` regionName ident || regionName ident == "stack" || regionName ident == "timer" || regionName ident == "raise")
      , Just actual <- [lookup ident (nativeMemory owned)]
      , actual /= expected
      ]
    prefix pre text = take (length pre) text == pre

data FamilyRun = FamilyRun
  { familyDomain :: Domain
  , familyCases :: [(String, Either String [String])]
  , familyMatches :: Int
  }

-- | Runs the ten cases against both images in one domain.
runFamily :: FilePath -> FilePath -> Facts -> Domain -> Image -> Image -> IO FamilyRun
runFamily runner directory facts domain stock owned = do
  let stockCases = [(caseName one, caseFor facts domain stock one) | one <- cases facts domain]
      ownedCases = [(caseName one, caseFor facts domain owned one) | one <- cases facts domain]
  stockObserved <- runCases runner (directory </> "stock") stockCases
  ownedObserved <- runCases runner (directory </> "owned") ownedCases
  results <- forM stockCases $ \(label, _) ->
    pure
      ( label
      , case (M.lookup label stockObserved, M.lookup label ownedObserved) of
          (Just s, Just o) -> Right (compareFamily (stripScratch s) (stripScratch o))
          (Nothing, _) -> Left "stock case did not run"
          (_, Nothing) -> Left "owned case did not run"
      )
  pure
    FamilyRun
      { familyDomain = domain
      , familyCases = results
      , familyMatches = length [() | (_, Right []) <- results]
      }
