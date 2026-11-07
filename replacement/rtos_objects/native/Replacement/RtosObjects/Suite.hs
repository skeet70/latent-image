{-# LANGUAGE OverloadedStrings #-}

-- | The ten F2 family-native cases over both entry domains, each run against the stock
-- loader bytes and the owned image and compared observation by observation.
--
-- The data layout is the scheduler oracle's: every context record, lock record, ready cell
-- and task record is planted from layout.json. The +0xd4 contract stays refused in the
-- manifest; these cases are the oracle hypothesis.
module Replacement.RtosObjects.Suite
  ( Family (..)
  , Case (..)
  , Image (..)
  , Expect (..)
  , cases
  , caseFor
  , compareFamily
  , runFamily
  , FamilyRun (..)
  , extents
  ) where

import Control.Monad (forM)
import qualified Data.ByteString as B
import Data.Bits ((.&.), (.|.), shiftL, shiftR)
import qualified Data.Map.Strict as M
import Data.Word (Word32)
import Emulation.NativeObservation (GuestAddress (..), NativeObservation (..), RegionId (..), Register (..), RunnerStatus (..), regionIdOf)
import Emulation.OwnedCase (CaseRegion (..), CaseSpec (..), Permission (..), assemble, mkRegion, word32le)
import Replacement.RtosObjects.Layout
import Replacement.RtosOwnFirst.Regions (Domain (..))
import Replacement.RtosScheduler.Oracle (runCases, stripScratch)
import System.FilePath ((</>))

data Family = SetFlag | Send
  deriving (Eq, Show)

-- | One observable the owned run must show.
data Expect
  = Equals Word32 Word32
  | Nonzero Word32
  deriving (Eq, Show)

-- | One case: the words it plants over the layout, the registers it enters with, the timer
-- stub's return, and the effects it must show.
data Case = Case
  { caseName :: String
  , caseFamily :: Family
  , caseSlot :: Word32
  , caseRegisters :: [(Register, Word32)]
  , caseWords :: [(Word32, Word32)]
  , caseTimer :: Word32
  , caseExpect :: [Expect]
  }

-- | The image a body is taken from.
data Image = Image
  { imageBase :: Word32
  , imageBytes :: B.ByteString
  }

-- | Synthetic addresses shared with the scheduler oracle.
definitionBase, definitionStride, stackBase, stackPointer, callerTop, exitAddress, receiptAddress :: Word32
definitionBase = 0x0096c000
definitionStride = 0x20
stackBase = 0x00959700
stackPointer = 0x00959800
callerTop = 0x00959810
exitAddress = 0x00958f00
receiptAddress = 0x00960000

-- | The waiter is task 2; task 1 is running.
waiterSlot, runningSlot :: Word32
waiterSlot = 2
runningSlot = 1

recordAddress :: Facts -> Domain -> Domain -> Word32
recordAddress facts executing of' = case of' of
  DomainA -> dfContextA (domainFacts facts executing)
  DomainB -> dfContextB (domainFacts facts executing)

definitionAddress :: Facts -> Domain -> Word32 -> Word32
definitionAddress facts domain slot =
  definitionBase + fromIntegral ((index domain * 120 + slot - 1) * fromIntegral definitionStride)
  where
    index DomainA = 0
    index DomainB = 1

trampoline :: Domain -> Word32
trampoline DomainA = 0x43728
trampoline DomainB = 0xd3728

lockEntries :: Domain -> (Word32, Word32)
lockEntries DomainA = (0x439d8, 0x43a14)
lockEntries DomainB = (0xd398c, 0xd39c8)

timerEntry :: Domain -> Word32
timerEntry DomainA = 0x44ca8
timerEntry DomainB = 0xd4c68

raiseIrqEntry :: Domain -> Word32
raiseIrqEntry DomainA = 0x433e4
raiseIrqEntry DomainB = 0xd33e4

releaseEntry :: Domain -> Word32
releaseEntry DomainA = 0x44568
releaseEntry DomainB = 0xd4528

releaseSize :: Word32
releaseSize = 0xcc

entryAddress :: Domain -> Family -> Word32
entryAddress DomainA SetFlag = 0x4813c
entryAddress DomainA Send = 0x4aa44
entryAddress DomainB SetFlag = 0xd80fc
entryAddress DomainB Send = 0xdaa04

-- | The stock extents of S1, S2, the wait release and the two F2 bodies.
extents :: Domain -> [(Word32, Word32)]
extents DomainA =
  [ (0x40b00, 0x24), (0x43640, 0x28), (0x43668, 0x20), (0x43688, 0xa0), (0x43728, 0x34), (0x4375c, 0x48), (0x437a4, 0x38), (0x437fc, 0x14)
  , (0x439d8, 0x3c), (0x43a14, 0x28), (0x43c48, 0x5c), (0x43ca4, 0x7c), (0x43d20, 0xb0), (0x43dd0, 0x48), (0x43f70, 0x2c), (0x44018, 0x2c), (0x44044, 0xf0), (0x44134, 0x80)
  , (0x44568, releaseSize)
  , (0x4813c, 0x260), (0x4aa44, 0x304)
  ]
extents DomainB =
  [ (0xd0b00, 0x24), (0xd3640, 0x28), (0xd3668, 0x20), (0xd3688, 0xa0), (0xd3728, 0x34), (0xd375c, 0x48), (0xd37a4, 0x38), (0xd37fc, 0x14)
  , (0xd398c, 0x3c), (0xd39c8, 0x28), (0xd3c04, 0x5c), (0xd3c60, 0x7c), (0xd3cdc, 0xb0), (0xd3d8c, 0x48), (0xd3f2c, 0x2c), (0xd3fd4, 0x30), (0xd4004, 0xf0), (0xd40f4, 0x80)
  , (0xd4528, releaseSize)
  , (0xd80fc, 0x260), (0xdaa04, 0x304)
  ]

-- | The record, lock, cell and task pokes, as the scheduler oracle plants them.
layoutPokes :: Facts -> Domain -> [(Word32, Word32)]
layoutPokes facts executing =
  concat [recordPokes block of' | block <- [DomainA, DomainB], of' <- [DomainA, DomainB]]
    ++ cellPokes
    ++ taskPokes
  where
    recordPokes block of' =
      let record = recordAddress facts executing of'
          w = domainFacts facts of'
          (acquire, release) = lockEntries block
          lockRecord offset =
            [ (record + offset + factsLockWord facts, 0)
            , (record + offset + factsLockAcquire facts, acquire)
            , (record + offset + factsLockRelease facts, release)
            ]
       in [ (record + factsContextId facts, if block == of' then factsOwnId facts else dfId w)
          , (record + factsContextOwner facts, dfId w)
          , (record + factsContextReadyHeads facts, dfReadyHeads w)
          , (record + factsContextReadyBitmap facts, dfReadyBitmap w)
          , (record + factsContextRunning facts, dfRunning w)
          , (record + factsContextSelected facts, dfSelected w)
          , (record + factsContextDispatchEnabled facts, dfDispatchEnabled w)
          , (record + factsContextFirstEntry facts, trampoline of')
          ]
            ++ lockRecord (factsContextSchedulerLock facts)
            ++ lockRecord (factsContextObjectLock facts)
    cellPokes =
      let w = domainFacts facts executing
          headAt q = dfReadyHeads w + q * factsHeadSize facts
       in concat [[(headAt q, headAt q), (headAt q + factsHeadPrev facts, headAt q)] | q <- [0 .. factsPriorities facts - 1]]
            ++ [ (dfReadyBitmap w, 0)
               , (dfSelected w, 0)
               , (dfRunning w, taskAt runningSlot)
               , (dfDispatchEnabled w, 1)
               , (dfSchedulerLock w, 0)
               , (dfObjectLock w, 0)
               ]
    taskPokes =
      concat
        [ [ (taskAt slot + factsTaskDefinition facts, definitionAddress facts executing slot)
          , (taskAt slot + factsTaskDomain facts, dfId (domainFacts facts executing))
          , (taskAt slot + factsTaskState facts, fromIntegral (factsStateDormant facts))
          , (taskAt slot + factsTaskPriority facts, slot `mod` factsPriorities facts)
          , (definitionAddress facts executing slot + factsDefinitionPriority facts, slot `mod` factsPriorities facts)
          , (definitionAddress facts executing slot + 0x10, 0x00950000 + slot * 0x100)
          , (definitionAddress facts executing slot + 0x14, 0x100)
          ]
        | slot <- [1 .. 120]
        ]
    taskAt slot = task facts executing slot

-- | The ten cases of reports/rtos-platform-options.md 4.14.6.
cases :: Facts -> Domain -> [Case]
cases facts domain =
  let w = domainFacts facts domain
      own = dfId w
      peer = case domain of DomainA -> 2; DomainB -> 1
      flag = flagConfig facts domain 1
      control = flagControl facts domain 1
      wait = flagWait facts domain waiterSlot
      mailbox = mailboxConfig facts domain 1
      mailControl = mailboxControl facts domain 1
      mailWait = mailboxWait facts domain waiterSlot
      t = task facts domain waiterSlot
      head = readyHead facts domain 3
      dispatch = dfDispatchEnabled w
      doorbell = 0xfffb40cc
      flagBase = [(flag + factsFlagConfigExists facts, 1), (control + factsWaitQueueNext facts, control), (control + factsWaitQueuePrev facts, control), (control + factsFlagControlPattern facts, 0)]
      waiter state = [(t + factsTaskNext facts, t), (t + factsTaskPrev facts, t), (t + factsTaskState facts, state), (t + factsTaskPriority facts, 3), (t + factsTaskDomain facts, own), (t + factsTaskWait facts, wait)]
      queued = fromIntegral (factsStateWait facts) + fromIntegral (factsStateObjectQueued facts)
   in [ Case "set_no_waiter" SetFlag 1 [(R1, 1)] flagBase 0 [Equals (control + factsFlagControlPattern facts) 1]
      , Case "set_satisfied" SetFlag 1 [(R1, 1)] (flagBase ++ [(control + factsWaitQueueNext facts, t), (wait + factsWaitBlockQueue facts, control), (wait + factsFlagWaitPattern facts, 1), (wait + factsFlagWaitMode facts, 1)] ++ waiter queued) 0 [Equals (wait + factsFlagWaitResult facts) 1, Equals (t + factsTaskState facts) (fromIntegral (factsStateReady facts)), Nonzero (dfSelected w), Equals (dfReadyBitmap w .&. (1 `shiftL` 3)) (1 `shiftL` 3)]
      , Case "set_unsatisfied" SetFlag 1 [(R1, 1)] (flagBase ++ [(control + factsWaitQueueNext facts, t), (wait + factsWaitBlockQueue facts, control), (wait + factsFlagWaitPattern facts, 2), (wait + factsFlagWaitMode facts, 1)] ++ waiter queued) 0 [Equals (control + factsFlagControlPattern facts) 1, Equals (t + factsTaskState facts) queued]
      , Case "set_clear_on_match" SetFlag 1 [(R1, 1)] (flagBase ++ [(flag + factsFlagConfigAttributes facts, fromIntegral (factsFlagClearOnMatch facts)), (control + factsWaitQueueNext facts, t), (wait + factsWaitBlockQueue facts, control), (wait + factsFlagWaitPattern facts, 1), (wait + factsFlagWaitMode facts, 1)] ++ waiter queued) 0 [Equals (control + factsFlagControlPattern facts) 0, Equals (wait + factsFlagWaitResult facts) 1]
      , Case "set_cross_domain" SetFlag 1 [(R1, 1)] (flagBase ++ [(control + factsWaitQueueNext facts, t), (wait + factsWaitBlockQueue facts, control), (wait + factsFlagWaitPattern facts, 1), (wait + factsFlagWaitMode facts, 1)] ++ waiter queued ++ [(t + factsTaskDomain facts, peer)]) 0 [Nonzero doorbell]
      , Case "set_timer_cancel" SetFlag 1 [(R1, 1)] (flagBase ++ [(control + factsWaitQueueNext facts, t), (wait + factsWaitBlockResult facts, timerEntry domain), (wait + factsWaitBlockQueue facts, control), (wait + factsFlagWaitPattern facts, 1), (wait + factsFlagWaitMode facts, 1)] ++ waiter queued) 0 [Equals (wait + factsWaitBlockResult facts) 0, Equals (receiptAddress + 4) (timerEntry domain)]
      , Case "send_blocked" Send 1 [(R1, messageAddress)] ([(mailbox + factsMailboxConfigExists facts, 1), (mailControl + factsWaitQueueNext facts, t), (mailWait + factsWaitBlockQueue facts, mailControl), (messageAddress, sentinel), (messageAddress + 4, 0)] ++ [(t + factsTaskNext facts, t), (t + factsTaskPrev facts, t), (t + factsTaskState facts, queued), (t + factsTaskPriority facts, 3), (t + factsTaskDomain facts, own), (t + factsTaskWait facts, mailWait)]) 0 [Equals (mailWait + factsMailboxWaitMessage facts) messageAddress, Equals messageAddress sentinel]
      , Case "send_empty_fifo" Send 1 [(R1, messageAddress)] ([(mailbox + factsMailboxConfigExists facts, 1), (mailControl + factsMailboxControlHead facts, 0), (mailControl + factsMailboxControlTail facts, 0), (mailControl + factsWaitQueueNext facts, mailControl), (mailControl + factsWaitQueuePrev facts, mailControl), (messageAddress, 0), (messageAddress + 4, 0)]) 0 [Equals (mailControl + factsMailboxControlHead facts) messageAddress, Equals (mailControl + factsMailboxControlTail facts) messageAddress, Equals (messageAddress, 0)]
      , Case "send_priority_equal" Send 1 [(R1, messageAddress)] ([(mailbox + factsMailboxConfigExists facts, 1), (mailbox + factsMailboxConfigAttributes facts, fromIntegral (factsMailboxMessagePriority facts)), (mailControl + factsMailboxControlHead facts, queuedMessage), (mailControl + factsMailboxControlTail facts, queuedMessage), (mailControl + factsWaitQueueNext facts, mailControl), (queuedMessage, 0), (queuedMessage + 4, 5), (messageAddress, 0), (messageAddress + 4, 5)]) 0 [Equals (queuedMessage, messageAddress), Equals (messageAddress, 0)]
      , Case "send_cross_domain" Send 1 [(R1, messageAddress)] ([(mailbox + factsMailboxConfigExists facts, 1), (mailControl + factsWaitQueueNext facts, t), (mailWait + factsWaitBlockQueue facts, mailControl), (messageAddress, 0)] ++ [(t + factsTaskNext facts, t), (t + factsTaskPrev facts, t), (t + factsTaskState facts, queued), (t + factsTaskPriority facts, 3), (t + factsTaskDomain facts, peer), (t + factsTaskWait facts, mailWait)]) 0 [Nonzero doorbell]
      ]
  where
    queuedMessage = messageAddress + 8

messageAddress :: Word32
messageAddress = 0x40000000

-- | The sentinel a send to a blocked receiver must leave in the message's link word.
sentinel :: Word32
sentinel = 0x5e4d1e00

-- | Little-endian words, the construction the scheduler oracle uses.
overlay :: Word32 -> B.ByteString -> [(Word32, Word32)] -> B.ByteString
overlay base = foldl apply
  where
    apply bytes (address, value) =
      let at = fromIntegral (address - base)
          encoded = word32le value
       in B.take at bytes <> encoded <> B.drop (at + B.length encoded) bytes

background :: Int -> B.ByteString
background size = B.replicate size 0

data Page = Page
  { pageId :: RegionId
  , pageBase :: Word32
  , pageSize :: Int
  , pagePermission :: Permission
  , pagePokes :: [(Word32, Word32)]
  }

-- | The data regions of one case, each with its pokes.
dataRegions :: Facts -> Domain -> Case -> [CaseRegion]
dataRegions facts domain one = map build (casePages facts domain one)

build :: Page -> CaseRegion
build region =
  mkRegion (pageId region) (GuestAddress (fromIntegral (pageBase region))) (pageSize region) (pagePermission region) (overlay (pageBase region) (background (pageSize region)) [(address, value) | (address, value) <- pagePokes region, address >= pageBase region, address < pageBase region + fromIntegral (pageSize region)])

casePages :: Facts -> Domain -> Case -> [Page]
casePages facts domain one =
  regions
  where
    w = domainFacts facts domain
    headAt q = dfReadyHeads w + q * factsHeadSize facts
    blockLow = minimum [dfSchedulerLock w, dfObjectLock w, dfContextA w, dfContextB w, dfReadyHeads w, dfReadyBitmap w, dfRunning w, dfSelected w, dfDispatchEnabled w]
    blockHigh = maximum [dfSchedulerLock w + 4, dfObjectLock w + 4, dfContextA w + factsContextSize facts, dfContextB w + factsContextSize facts, headAt (factsPriorities facts - 1) + factsHeadSize facts, dfReadyBitmap w + 4, dfRunning w + 4, dfSelected w + 4, dfDispatchEnabled w + 4]
    regions =
      [ Page (RegionId "block") blockLow (fromIntegral (blockHigh - blockLow)) ReadWrite (layoutPokes facts domain ++ caseWords one)
      , Page (RegionId "tasks") (dfTaskTable w) (120 * fromIntegral (factsTaskSize facts)) ReadWrite (layoutPokes facts domain ++ caseWords one)
      , Page (RegionId "definitions") definitionBase (240 * fromIntegral definitionStride) ReadWrite (layoutPokes facts domain ++ caseWords one)
      , Page (RegionId "flags") (dfFlagConfigs w) (120 * fromIntegral (factsFlagConfigSize facts)) ReadWrite (caseWords one)
      , Page (RegionId "flag-controls") (dfFlagControls w) (120 * fromIntegral (factsFlagControlSize facts)) ReadWrite (caseWords one)
      , Page (RegionId "flag-waits") (dfFlagWaits w) (120 * fromIntegral (factsFlagWaitSize facts)) ReadWrite (caseWords one)
      , Page (RegionId "flag-timers") (dfFlagTimers w) (120 * fromIntegral (factsTimerEntrySize facts)) ReadWrite (caseWords one)
      , Page (RegionId "mailbox-configs") (dfMailboxConfigs w) (fromIntegral (factsMailboxConfigSize facts)) ReadWrite (caseWords one)
      , Page (RegionId "mailbox-controls") (dfMailboxControls w) (fromIntegral (factsMailboxControlSize facts)) ReadWrite (caseWords one)
      , Page (RegionId "mailbox-waits") (dfMailboxWaits w) (120 * fromIntegral (factsMailboxWaitSize facts)) ReadWrite (caseWords one)
      , Page (RegionId "mailbox-timers") (dfMailboxTimers w) (120 * fromIntegral (factsTimerEntrySize facts)) ReadWrite (caseWords one)
      , Page (RegionId "stack") stackBase (fromIntegral (stackPointer - stackBase)) ReadWrite []
      , Page (RegionId "caller-frame") stackPointer (fromIntegral (callerTop - stackPointer)) ReadWrite []
      , Page (RegionId "exit") exitAddress 4 ReadExecute []
      , Page (RegionId "receipt") receiptAddress 8 ReadWrite []
      , Page (RegionId "doorbell") 0xfffb4000 0x1000 Device []
      , Page (RegionId "doorbell-1") 0xfffb3000 0x1000 Device []
      ]
    build region =
      mkRegion (pageId region) (GuestAddress (fromIntegral (pageBase region))) (pageSize region) (pagePermission region) (overlay (pageBase region) (background (pageSize region)) [(address, value) | (address, value) <- pagePokes region, address >= pageBase region, address < pageBase region + fromIntegral (pageSize region)])

-- | The timer stub records its two arguments in the receipt page and returns 0.
timerStub :: B.ByteString
timerStub = assemble [movw 2 (receiptAddress .&. 0xffff), movt 2 (receiptAddress `shiftR` 16), 0xe5820000, 0xe5821004, 0xe3a00000, 0xe12fff1e]

movw :: Word32 -> Word32 -> Word32
movw rd imm = 0xe3000000 .|. (rd `shiftL` 12) .|. ((imm .&. 0xf000) `shiftL` 4) .|. (imm .&. 0xfff)

movt :: Word32 -> Word32 -> Word32
movt rd imm = 0xe3400000 .|. (rd `shiftL` 12) .|. ((imm .&. 0xf000) `shiftL` 4) .|. (imm .&. 0xfff)

-- | The case's spec over one image.
caseFor :: Facts -> Domain -> Image -> Case -> CaseSpec
caseFor facts domain image one =
  CaseSpec
    { caseId = caseName one ++ "-" ++ show domain
    , caseProvenance = "X-M1 1.50 loader " ++ show domain ++ " " ++ show (caseFamily one) ++ " over layout.json and the rtos_layout stock data layout"
    , caseExclusion = ""
    , caseEntry = GuestAddress (fromIntegral (entryAddress domain (caseFamily one)))
    , caseExit = GuestAddress (fromIntegral exitAddress)
    , caseRegions =
        [mkRegion (RegionId ("code-" ++ show base)) (GuestAddress (fromIntegral base)) (fromIntegral size) ReadExecute (slice base size) | (base, size) <- extents domain]
          ++ [ mkPage (RegionId "code-timer") (GuestAddress (fromIntegral (timerEntry domain))) (B.length timerStub) ReadExecute timerStub
             , mkPage (RegionId "code-raise") (GuestAddress (fromIntegral (raiseIrqEntry domain))) 8 ReadExecute (assemble [0xe3a00000, 0xe12fff1e])
             ]
          ++ dataRegions facts domain one
    , caseFunctions = [(GuestAddress (fromIntegral base), size) | (base, size) <- extents domain]
    , caseInitial = [(R0, caseSlot one)] ++ caseRegisters one ++ [(Sp, stackPointer), (Lr, exitAddress), (Cpsr, 0x13)]
    , caseExpectedStatus = Returned
    , caseExpectedRegisters = []
    , caseExpectedMemory = []
    , caseCp14Identity = Nothing
    , caseKernel = Nothing
    }
  where
    slice base size = B.take (fromIntegral size) (B.drop (fromIntegral (base - imageBase image)) (imageBytes image))

-- | The independent requirements, then the comparison of the preserved state and effects.
compareFamily :: Facts -> Domain -> Case -> NativeObservation -> NativeObservation -> [String]
compareFamily facts domain one stock owned =
  requireStatus ++ requireExpectations ++ requireReceipt ++ compareRegisters ++ compareMemory
  where
    bases = regionBases facts domain one
    requireStatus =
      [ "stock status " ++ show (nativeStatus stock) | nativeStatus stock /= Returned]
        ++ [ "owned status " ++ show (nativeStatus owned) | nativeStatus owned /= Returned]
    requireExpectations =
      [ "owned " ++ show expectation ++ " not observed" | expectation <- caseExpect one, not (holds (caseExpect one) owned)]
    requireReceipt =
      [ "timer receipt differs: " ++ show (receipt stock) ++ " against " ++ show (receipt owned)
      | receipt stock /= receipt owned
      ]
    holds expectations observation = all (`holdsOne` observation) expectations
    holdsOne (Equals address expected) observation = observedWord bases address observation == Just expected
    holdsOne (Nonzero address) observation = maybe False (/= 0) (observedWord bases address observation)
    receipt observation = (observedWord bases receiptAddress observation, observedWord bases (receiptAddress + 4) observation)
    compareRegisters =
      [ "r" ++ show r ++ " is " ++ show (M.lookup r (nativeRegisters owned)) ++ " expected " ++ show (M.lookup r (nativeRegisters stock))
      | r <- [R4, R5, R6, R7, R8, R9, R10, R11, Sp]
      , M.lookup r (nativeRegisters owned) /= M.lookup r (nativeRegisters stock)
      ]
    compareMemory =
      [ "region " ++ regionName ident ++ " is missing from the owned observation" | (ident, _) <- nativeMemory stock, ident `notElem` map fst (nativeMemory owned)]
        ++ [ "region " ++ regionName ident ++ " is extra in the owned observation" | (ident, _) <- nativeMemory owned, ident `notElem` map fst (nativeMemory stock)]
        ++ [ "region " ++ regionName ident ++ " differs" | (ident, expected) <- nativeMemory stock, Just actual <- [lookup ident (nativeMemory owned)], actual /= expected]

-- | A region's base, so an observation word can be found in its page.
regionBases :: Facts -> Domain -> Case -> [(String, Word32)]
regionBases facts domain one =
  [ (regionName (pageId region), pageBase region) | region <- casePages facts domain one ]

observedWord :: [(String, Word32)] -> Word32 -> NativeObservation -> Maybe Word32
observedWord bases address observation =
  case [wordAt bytes (address - base) | (ident, base) <- bases, base <= address, (ident', bytes) <- nativeMemory observation, regionName ident' == ident, address < base + fromIntegral (B.length bytes)] of
    word : _ -> Just word
    [] -> Nothing

wordAt :: B.ByteString -> Word32 -> Word32
wordAt bytes offset =
  let at = fromIntegral offset
   in B.foldl' (\value byte -> value `shiftL` 8 .|. fromIntegral byte) 0 (B.take 4 (B.drop at bytes))

data FamilyRun = FamilyRun
  { familyDomain :: Domain
  , familyCases :: [(String, Either String [String])]
  , familyMatches :: Int
  }

runFamily :: FilePath -> FilePath -> Facts -> Domain -> Image -> Image -> IO FamilyRun
runFamily runner directory facts domain stock owned = do
  let stockCases = [(caseName one, caseFor facts domain stock one) | one <- cases facts domain]
      ownedCases = [(caseName one, caseFor facts domain owned one) | one <- cases facts domain]
      byName = M.fromList [(caseName one, one) | one <- cases facts domain]
  stockObserved <- runCases runner (directory </> "stock") stockCases
  ownedObserved <- runCases runner (directory </> "owned") ownedCases
  results <- forM stockCases $ \(label, _) ->
    pure
      ( label
      , case (M.lookup label stockObserved, M.lookup label ownedObserved) of
          (Just s, Just o) -> Right (compareFamily facts domain (byName M.! label) (stripScratch s) (stripScratch o))
          (Nothing, _) -> Left "stock case did not run"
          (_, Nothing) -> Left "owned case did not run"
      )
  pure
    FamilyRun
      { familyDomain = domain
      , familyCases = results
      , familyMatches = length [() | (_, Right []) <- results]
      }
