{-# LANGUAGE OverloadedStrings #-}
module Replacement.RtosObjects.Suite
  ( Image (..), FamilyRun (..), JoinedRun (..), Entries, buildEntries, extents, s3bBase, runFamily, runJoined ) where

import Control.Monad (forM, unless)
import Data.Bits ((.&.), (.|.), shiftL, shiftR, xor)
import qualified Data.ByteString as B
import Data.List (find)
import qualified Data.Map.Strict as M
import Data.Maybe (fromMaybe, isJust, mapMaybe, maybeToList)
import Data.Word (Word32, Word8)
import Emulation.CachedNativeBatch (BatchReuse (..), cachedObservedBatch)
import Emulation.NativeCase (CaseId (..), caseIdText)
import qualified Emulation.NativeObservation as N
import Emulation.NativeObservation (Barrier (..), KernelObservation (..), NativeObservation (..), Ordered (..), RegionId (..), Register (..), RunnerStatus (..), Write (..), WriteValue (..))
import Emulation.OwnedCase (CaseSpec (..), InstructionLimit (..), KernelCase (..), Permission (..), assemble, mkRegion, mkStackContext, plainLimit, readWordAt, word32le, writeBatch)
import Replacement.RtosObjects.Layout hiding (record)
import Replacement.RtosOwnFirst.Regions (Domain (..), GuestAddress (..), domainLabel)
import System.FilePath ((</>))

data Slot
  = FlagCreate | FlagDelete | FlagSet | FlagSetHandler | FlagClear
  | FlagWait | FlagPoll | FlagTimedWait
  | MailboxCreate | MailboxDelete | MailboxSend
  | MailboxReceive | MailboxPoll | MailboxTimedReceive
  deriving (Eq, Ord, Show, Enum, Bounded)

type TaskSlot = Word32

data RetainedKind
  = RetainedSemaphoreWait | RetainedSemaphoreTimedWait
  | RetainedDataQueueReceive | RetainedDataQueueTimedReceive
  | RetainedFixedPoolGet | RetainedFixedPoolTimedGet
  deriving (Eq, Show)

data Family = FlagFamily | MailboxFamily deriving (Eq, Show)

data Role = StockRole | OwnedRole deriving (Eq, Show)

data Boundary = CallerReturn | TaskArrival deriving (Eq, Show)
data Invocation = TaskInvocation | HandlerCallback deriving (Eq, Show)


data StockDifference
  = NoDifference
  | DeleteStrandsWaiter
  | HandlerWaiterDomain
  | AliasNextLink
  | AliasAttributes
  | AliasPatternClear
  | CreatePublication
  | OutOfRangeWrite
  | DeleteLeavesQueueTail
  deriving (Eq, Show)

data Expect
  = Equals GuestAddress Word32
  | ByteEquals GuestAddress Word8
  | Result Word32
  | RegisterEquals Register Word32
  | ControlEquals Word32
  | OrderBefore GuestAddress GuestAddress
  | OrderBarrierBefore Barrier GuestAddress
  | NoReacquire GuestAddress
  | TimerLog TimerLogExpect
  deriving Show

-- Storage beyond the timer-log cursor is not an observed event.
data TimerLogExpect = TimerLogExpect Int [(Word32, Word32)] deriving Show

data Step = StepCall Slot [(Register, Word32)] | StepStore GuestAddress deriving Show

data WakerTarget = TargetSlot Slot | TargetAddress Word32 | TargetTimerTick Int deriving Show

data Waker = Waker
  { wakerTarget :: WakerTarget
  , wakerArgs :: [(Register, Word32)]
  } deriving Show

data PreWaiter = PreWaiter
  { pwSlot :: TaskSlot
  , pwDomain :: Domain
  , pwPriority :: Word32
  , pwWant :: Word32
  , pwMode :: Word32
  , pwTimer :: Bool
  , pwChain :: Maybe Waker
  } deriving Show

data Case = Case
  { caseName :: CaseId
  , caseSlot :: Slot
  , caseEntryAddress :: Maybe Word32
  , caseRetained :: Maybe RetainedKind
  , caseObject :: Domain
  , caseObjectSlot :: Word32
  , caseInvocation :: Invocation
  , caseCpsr :: Word32
  , caseArgs :: [(Register, Word32)]
  , caseBoundary :: Boundary
  , caseExpect :: [Expect]
  , caseStockExpect :: Maybe [Expect]
  , caseDifference :: StockDifference
  , caseSteps :: [Step]
  , caseWaker :: Maybe Waker
  , caseWaiters :: [PreWaiter]
  , caseFlags :: [(Domain, Word32, Word32, Word32, Word32)]
  , caseMailboxes :: [(Domain, Word32, Word32, Word32, Word32, Word32)]
  , caseMessages :: [(Word32, Word32, Word32)]
  , caseFlagCounts :: (Word32, Word32)
  , caseMailboxCounts :: (Word32, Word32)
  , caseDispatch :: Word32
  , caseHandler :: Word32
  , caseTick :: Word32
  , caseReschedule :: Word32
  , caseRunningState :: Word8
  , caseThreadValue :: Maybe Word32
  , caseWords :: [(GuestAddress, Word32)]
  , caseLimit :: InstructionLimit
  , caseStockStatus :: Maybe RunnerStatus
  , caseSkipTimerLog :: Bool
  -- | The S3b caller's scheduler-lock domain; only joined cases set it.
  , caseSchedulerLock :: Maybe Domain
  -- | A direct entry's initial registers; absent cases enter through the wrapper.
  , caseDirectInitial :: Maybe [(Register, Word32)]
  -- | A selected-task stub that blocks through the family wait at this entry.
  , caseSelectedWait :: Maybe Word32
  }

data Image = Image { imageBase :: Word32, imageBytes :: B.ByteString }
data Page = Page RegionId GuestAddress Int Permission B.ByteString

data FamilyRun = FamilyRun
  { familyDomain :: Domain
  , familyCases :: [(String, Either String [String])]
  , familyMatches :: Int
  , familyReuse :: (BatchReuse, BatchReuse)
  , familyControls :: [(String, Bool)]
  , familyDifferences :: [(String, String)]
  }

newtype Entries = Entries (M.Map (Slot, Domain) Word32)

slotOperation :: Slot -> String
slotOperation slot = case slot of
  FlagCreate -> "eventflag-create"
  FlagDelete -> "eventflag-delete"
  FlagSet -> "eventflag-set"
  FlagSetHandler -> "eventflag-interrupt-set"
  FlagClear -> "eventflag-clear"
  FlagWait -> "eventflag-wait"
  FlagPoll -> "eventflag-poll"
  FlagTimedWait -> "eventflag-timed-wait"
  MailboxCreate -> "mailbox-create"
  MailboxDelete -> "mailbox-reset-to-default"
  MailboxSend -> "mailbox-send"
  MailboxReceive -> "mailbox-receive-blocking"
  MailboxPoll -> "mailbox-receive-poll"
  MailboxTimedReceive -> "mailbox-receive-timeout"

slotFunction :: Slot -> String
slotFunction slot = case slot of
  FlagCreate -> "kernel_create_flag"
  FlagDelete -> "kernel_delete_flag"
  FlagSet -> "event_flag_set"
  FlagSetHandler -> "event_flag_interrupt_set"
  FlagClear -> "event_flag_clear"
  FlagWait -> "event_flag_wait"
  FlagPoll -> "event_flag_poll_wait"
  FlagTimedWait -> "event_flag_timed_wait"
  MailboxCreate -> "kernel_create_mailbox"
  MailboxDelete -> "kernel_delete_mailbox"
  MailboxSend -> "mailbox_send"
  MailboxReceive -> "mailbox_receive"
  MailboxPoll -> "mailbox_poll_receive"
  MailboxTimedReceive -> "mailbox_timed_receive"

slotFamily :: Slot -> Family
slotFamily slot = case slot of
  FlagCreate -> FlagFamily
  FlagDelete -> FlagFamily
  FlagSet -> FlagFamily
  FlagSetHandler -> FlagFamily
  FlagClear -> FlagFamily
  FlagWait -> FlagFamily
  FlagPoll -> FlagFamily
  FlagTimedWait -> FlagFamily
  _ -> MailboxFamily

slotPlacement :: [(Slot, Domain, Word32, Word32)]
slotPlacement =
  [ (FlagCreate, DomainA, 0x4d39c, 0x98), (FlagDelete, DomainA, 0x4d434, 0x68)
  , (FlagSet, DomainA, 0x4813c, 0x260), (FlagSetHandler, DomainA, 0x4839c, 0x224)
  , (FlagClear, DomainA, 0x485c0, 0x138), (FlagWait, DomainA, 0x486f8, 0x2b4)
  , (FlagPoll, DomainA, 0x489ac, 0x184), (FlagTimedWait, DomainA, 0x48b30, 0x308)
  , (MailboxCreate, DomainA, 0x4d5bc, 0x98), (MailboxDelete, DomainA, 0x4d654, 0x70)
  , (MailboxSend, DomainA, 0x4aa44, 0x304), (MailboxReceive, DomainA, 0x4ad48, 0x254)
  , (MailboxPoll, DomainA, 0x4af9c, 0x148), (MailboxTimedReceive, DomainA, 0x4b0e4, 0x2a8)
  , (FlagCreate, DomainB, 0xdd360, 0x98), (FlagDelete, DomainB, 0xdd3f8, 0x68)
  , (FlagSet, DomainB, 0xd80fc, 0x260), (FlagSetHandler, DomainB, 0xd835c, 0x224)
  , (FlagClear, DomainB, 0xd8580, 0x138), (FlagWait, DomainB, 0xd86b8, 0x2b4)
  , (FlagPoll, DomainB, 0xd896c, 0x184), (FlagTimedWait, DomainB, 0xd8af0, 0x308)
  , (MailboxCreate, DomainB, 0xdd580, 0x98), (MailboxDelete, DomainB, 0xdd618, 0x70)
  , (MailboxSend, DomainB, 0xdaa04, 0x304), (MailboxReceive, DomainB, 0xdad08, 0x254)
  , (MailboxPoll, DomainB, 0xdaf5c, 0x148), (MailboxTimedReceive, DomainB, 0xdb0a4, 0x2a8)
  ]

slotExtent :: Slot -> Domain -> (Word32, Word32)
slotExtent slot domain =
  case [ (origin, size) | (candidate, candidateDomain, origin, size) <- slotPlacement, candidate == slot, candidateDomain == domain ] of
    [extent] -> extent
    _ -> error ("missing placement for " ++ show slot ++ " " ++ show domain)

placementOther :: Domain -> [(Word32, Word32)]
placementOther DomainA =
  [ (0x40b00, 0x24), (0x43640, 0x28), (0x43668, 0x20), (0x43688, 0xa0)
  , (0x43728, 0x34), (0x4375c, 0x48), (0x437a4, 0x38), (0x437fc, 0x14)
  , (0x439d8, 0x3c), (0x43a14, 0x28), (0x43c48, 0x5c), (0x43ca4, 0x7c)
  , (0x43d20, 0xb0), (0x43dd0, 0x48), (0x43f70, 0x2c), (0x44018, 0x2c)
  , (0x44044, 0xf0), (0x44134, 0x80), (0x44568, 0xcc)
  , (0x44744, 0xbc), (0x44800, 0xe4), (0x448e4, 0xdc), (0x480cc, 0x70)
  ]
placementOther DomainB =
  [ (0xd0b00, 0x24), (0xd3640, 0x28), (0xd3668, 0x20), (0xd3688, 0xa0)
  , (0xd3728, 0x34), (0xd375c, 0x48), (0xd37a4, 0x38), (0xd37fc, 0x14)
  , (0xd398c, 0x3c), (0xd39c8, 0x28), (0xd3c04, 0x5c), (0xd3c60, 0x7c)
  , (0xd3cdc, 0xb0), (0xd3d8c, 0x48), (0xd3f2c, 0x2c), (0xd3fd4, 0x30)
  , (0xd4004, 0xf0), (0xd40f4, 0x80), (0xd4528, 0xcc)
  , (0xd4704, 0xbc), (0xd47c0, 0xe4), (0xd48a4, 0xdc), (0xd808c, 0x70)
  ]

extents :: Domain -> [(Word32, Word32)]
extents domain =
  [ slotExtent slot domain | slot <- [minBound .. maxBound] ] ++ placementOther domain

-- | The six qualified S3b service extents, from @regions-s3b.json@. The linked bin is
-- contiguous from its first extent and fills the gaps with the UDF word.
s3bPlacement :: Domain -> [(Word32, Word32)]
s3bPlacement DomainA =
  [ (0x44f2c, 0x1c4), (0x450f0, 0x180), (0x45270, 0x180)
  , (0x46004, 0x1fc), (0x46200, 0x198), (0x46398, 0x19c)
  ]
s3bPlacement DomainB =
  [ (0xd4eec, 0x1c4), (0xd50b0, 0x180), (0xd5230, 0x180)
  , (0xd5fc4, 0x1fc), (0xd61c0, 0x198), (0xd6358, 0x19c)
  ]

s3bBase :: Domain -> Word32
s3bBase domain = minimum (map fst (s3bPlacement domain))

data S3bService = S3bWake | S3bInterruptWake deriving (Eq, Show)

s3bEntry :: Domain -> S3bService -> Word32
s3bEntry DomainA S3bWake = 0x46004
s3bEntry DomainA S3bInterruptWake = 0x46200
s3bEntry DomainB S3bWake = 0xd5fc4
s3bEntry DomainB S3bInterruptWake = 0xd61c0

-- | The wait release the qualified S3b callers resolve.
s3bRelease :: Domain -> Word32
s3bRelease DomainA = 0x44568
s3bRelease DomainB = 0xd4528

data JoinedRun = JoinedRun
  { joinedDomain :: Domain
  , joinedRows :: [(String, Either String [String])]
  , joinedReuse :: (BatchReuse, BatchReuse, BatchReuse)
  , joinedReturnReuse :: (BatchReuse, BatchReuse, BatchReuse)
  , joinedFalseReuse :: BatchReuse
  , joinedControls :: [(String, Bool)]
  , joinedCounterexamples :: [(String, String)]
  , joinedFalseCounterexamples :: [(String, String)]
  }

retained :: Domain -> [(Word32, Word32)]
retained DomainA = [(0x4252c, 0x28), (0x433e4, 0x60), (0x43444, 0x60)]
retained DomainB = [(0xd252c, 0x28), (0xd33e4, 0x60), (0xd3444, 0x60)]

data Seam = Seam String Domain Word32 Word32

retainedSeams :: Domain -> [Seam]
retainedSeams domain = case domain of
  DomainA ->
    [ Seam "stock-mask-pair" domain 0x42560 0x68
    , Seam "stock-flag-initialize" domain 0x48044 0x88
    , Seam "stock-mailbox-initialize" domain 0x4a9bc 0x88
    , Seam "stock-timeout-callback" domain 0x44318 0x1cc
    , Seam "stock-timer-arm" domain 0x444e4 0x84
    , Seam "stock-delay-expiry" domain 0x44634 0xc8
    , Seam "stock-remove-timer-helper" domain 0x446fc 0x48
    , Seam "stock-initialize-timers" domain 0x44a50 0x134
    , Seam "stock-timer-sift" domain 0x44b84 0xdc
    , Seam "stock-insert-timer" domain 0x44c60 0x48
    , Seam "stock-timer-tick" domain 0x44d8c 0x1a0
    , Seam "stock-remove-timer" domain 0x44ca8 (fromIntegral (B.length timerStub))
    ]
  DomainB ->
    [ Seam "stock-mask-pair" domain 0xd2560 0x68
    , Seam "stock-flag-initialize" domain 0xd8004 0x88
    , Seam "stock-mailbox-initialize" domain 0xda97c 0x88
    , Seam "stock-timeout-callback" domain 0xd42d8 0x1cc
    , Seam "stock-timer-arm" domain 0xd44a4 0x84
    , Seam "stock-delay-expiry" domain 0xd45f4 0xc8
    , Seam "stock-remove-timer-helper" domain 0xd46bc 0x48
    , Seam "stock-initialize-timers" domain 0xd4a10 0x134
    , Seam "stock-timer-sift" domain 0xd4b44 0xdc
    , Seam "stock-insert-timer" domain 0xd4c20 0x48
    , Seam "stock-timer-tick" domain 0xd4d4c 0x1a0
    , Seam "stock-remove-timer" domain 0xd4c68 (fromIntegral (B.length timerStub))
    ]

data RetainedTables = RetainedTables
  { rtSemConfig, rtSemControl, rtSemWait, rtSemTimer :: Word32
  , rtDqConfig, rtDqRuntime, rtDqWait, rtDqTimer :: Word32
  , rtPoolConfig, rtPoolControl, rtPoolWait, rtPoolTimer :: Word32
  }

retainedTables :: Domain -> RetainedTables
retainedTables DomainA = RetainedTables 0x4005dfd8 0x4005af0c 0x40054efc 0x4005549c 0x4005988c 0x4005a00c 0x400570bc 0x40056b1c 0x4006194c 0x4005f584 0x4005855c 0x40058cdc
retainedTables DomainB = RetainedTables 0x400edf98 0x400eaecc 0x400e4ebc 0x400e545c 0x400e984c 0x400e9fcc 0x400e707c 0x400e6adc 0x400f190c 0x400ef544 0x400e851c 0x400e8c9c

retainedEntry :: RetainedKind -> Domain -> Word32
retainedEntry kind domain = case (kind, domain) of
  (RetainedSemaphoreWait, DomainA) -> 0x47a54
  (RetainedSemaphoreWait, DomainB) -> 0xd7a14
  (RetainedSemaphoreTimedWait, DomainA) -> 0x47dd0
  (RetainedSemaphoreTimedWait, DomainB) -> 0xd7d90
  (RetainedDataQueueReceive, DomainA) -> 0x49f2c
  (RetainedDataQueueReceive, DomainB) -> 0xd9eec
  (RetainedDataQueueTimedReceive, DomainA) -> 0x4a56c
  (RetainedDataQueueTimedReceive, DomainB) -> 0xda52c
  (RetainedFixedPoolGet, DomainA) -> 0x4b480
  (RetainedFixedPoolGet, DomainB) -> 0xdb440
  (RetainedFixedPoolTimedGet, DomainA) -> 0x4b814
  (RetainedFixedPoolTimedGet, DomainB) -> 0xdb7d4

retainedExtents :: Domain -> [(Word32, Word32)]
retainedExtents DomainA =
  [ (0x47604, 0x240), (0x47844, 0x210), (0x47a54, 0x240), (0x47c94, 0x13c), (0x47dd0, 0x274)
  , (0x48e38, 0xa4), (0x48edc, 0xc8), (0x48fa4, 0x54), (0x49308, 0x220)
  , (0x49f2c, 0x3c4), (0x4a2f0, 0x27c), (0x4a56c, 0x44c)
  , (0x4b38c, 0x90), (0x4b41c, 0x64), (0x4b480, 0x250), (0x4b6d0, 0x144), (0x4b814, 0x2a4), (0x4bab8, 0x30c)
  ]
retainedExtents DomainB =
  [ (0xd75c4, 0x240), (0xd7804, 0x210), (0xd7a14, 0x240), (0xd7c54, 0x13c), (0xd7d90, 0x274)
  , (0xd8df8, 0xa4), (0xd8e9c, 0xc8), (0xd8f64, 0x54), (0xd92c8, 0x220)
  , (0xd9eec, 0x3c4), (0xda2b0, 0x27c), (0xda52c, 0x44c)
  , (0xdb34c, 0x90), (0xdb3dc, 0x64), (0xdb440, 0x250), (0xdb690, 0x144), (0xdb7d4, 0x2a4), (0xdba78, 0x30c)
  ]

retainedSignal, retainedSend, retainedRelease :: Domain -> Word32
retainedSignal DomainA = 0x47604
retainedSignal DomainB = 0xd75c4
retainedSend DomainA = 0x49308
retainedSend DomainB = 0xd92c8
retainedRelease DomainA = 0x4bab8
retainedRelease DomainB = 0xdba78

poolStart :: Word32
poolStart = 0x40064000

seam :: Domain -> String -> Word32
seam domain name = case [address | Seam candidate domain' address _ <- retainedSeams domain, candidate == name, domain' == domain] of
  [address] -> address
  _ -> error ("missing retained seam " ++ name)

buildEntries :: Manifest -> Either String Entries
buildEntries manifest = Entries . M.fromList <$> mapM one [ (slot, domain) | slot <- [minBound .. maxBound], domain <- [DomainA, DomainB] ]
  where
    one (slot, domain) = do
      address <- manifestAddress manifest (slotOperation slot) (domainTag domain) (slotFunction slot)
      let (origin, _) = slotExtent slot domain
      if address == origin
        then pure ((slot, domain), address)
        else Left ("manifest entry " ++ show slot ++ " " ++ show domain ++ " is " ++ show address ++ ", placement origin is " ++ show origin)

domainTag :: Domain -> String
domainTag DomainA = "A"
domainTag DomainB = "B"

entryAddress :: Entries -> Domain -> Slot -> Word32
entryAddress (Entries table) domain slot =
  fromMaybe (error ("no entry for " ++ show slot ++ " " ++ show domain)) (M.lookup (slot, domain) table)

wireAddress :: GuestAddress -> N.GuestAddress
wireAddress (GuestAddress address) = N.GuestAddress (fromIntegral address)

pointerWord :: GuestAddress -> Word32
pointerWord (GuestAddress address) = address

peer :: Domain -> Domain
peer DomainA = DomainB
peer DomainB = DomainA

runningSlot, waiterSlot, idleSlot, wakerSlot :: TaskSlot
runningSlot = 1
waiterSlot = 2
idleSlot = 3
wakerSlot = 4

definitionBase, definitionStride, stackPointer, handlerStackPointer, targetStackPointer, wakerStack, chainStack, exitAddress, stopAddress, receiptAddress, globalLock :: Word32
definitionBase = 0x0096c000
definitionStride = 0x40
stackPointer = 0x00970000
handlerStackPointer = 0x00970200
targetStackPointer = 0x00970400
wakerStack = 0x00970a00
chainStack = wakerStack + 0x400
exitAddress = 0x00958f00
stopAddress = 0x00959200
receiptAddress = 0x00960000
globalLock = 0x400c0804

wrapperBase, wakerStubBase, chainStubBase :: Word32
wrapperBase = 0x00959000
wakerStubBase = 0x00959100
chainStubBase = 0x00959180

actorBase, actorStackLow, actorStackHigh :: Word32
actorBase = 0x00959180
actorStackLow = wakerStack - 0x400
actorStackHigh = wakerStack

outWord, resultCell, requestAddress, unreadable :: Word32
outWord = 0x40000080
resultCell = 0x40000088
requestAddress = 0x40000090
unreadable = 0xdead0000

messageAddress :: Int -> Word32
messageAddress index = 0x40000000 + fromIntegral index * 0x10

timerLogBase :: Word32
timerLogBase = receiptAddress

timerStub :: B.ByteString
timerStub = assemble [movw R2 0, movt R2 (receiptAddress `div` 0x10000), 0xe592c000, 0xe082318c, 0xe5830004, 0xe5831008, 0xe28cc001, 0xe582c000, 0xe12fff1e]

timerEntry, trampoline, resumeEntry :: Domain -> Word32
timerEntry domain = seam domain "stock-remove-timer"
trampoline DomainA = 0x43728
trampoline DomainB = 0xd3728
resumeEntry DomainA = 0x43668
resumeEntry DomainB = 0xd3668

s2Save :: Domain -> Word32
s2Save DomainA = 0x43640
s2Save DomainB = 0xd3640

lockEntries :: Domain -> (Word32, Word32)
lockEntries DomainA = (0x439d8, 0x43a14)
lockEntries DomainB = (0xd398c, 0xd39c8)

bank :: Domain -> GuestAddress
bank DomainA = GuestAddress 0xfffb30cc
bank DomainB = GuestAddress 0xfffb40cc

recordAddress :: Facts -> Domain -> Domain -> Word32
recordAddress facts executing described = case described of
  DomainA -> dfContextA (domainFacts facts executing)
  DomainB -> dfContextB (domainFacts facts executing)

definitionAddress :: Facts -> Domain -> TaskSlot -> Word32
definitionAddress facts domain slot = definitionBase + (index * factsTaskSlots facts + slot - 1) * definitionStride
  where index = if domain == DomainA then 0 else 1

taskAt :: Facts -> Domain -> TaskSlot -> Word32
taskAt facts domain slot = dfTaskTable (domainFacts facts domain) + (slot - 1) * factsTaskSize facts

-- The stock reads a one-based local slot from definition +0x20 and from TPIDRURW.
threadValue :: Domain -> TaskSlot -> Word32
threadValue _ slot = slot

selectedThreadValue :: Case -> Word32
selectedThreadValue one = case caseWaiters one of
  (pw : _) -> threadValue (pwDomain pw) (pwSlot pw)
  [] -> threadValue (caseObject one) waiterSlot

packedState :: Word8 -> Word32 -> Word32
packedState state priority = fromIntegral state .|. (priority `shiftL` 8)

poke :: Word32 -> Word32 -> (GuestAddress, Word32)
poke address value = (GuestAddress address, value)

eq :: Word32 -> Word32 -> Expect
eq address value = Equals (GuestAddress address) value

byte :: Word32 -> Word8 -> Expect
byte address value = ByteEquals (GuestAddress address) value

res :: Word32 -> Expect
res value = Result value

control :: Word32 -> Expect
control value = ControlEquals value

timerLog :: Int -> [(Word32, Word32)] -> Expect
timerLog count entries = TimerLog (TimerLogExpect count entries)

id0 :: Word32 -> Word32
id0 slot = slot

idA :: Word32 -> Word32
idA slot = 0x00010000 .|. slot

idB :: Word32 -> Word32
idB slot = 0x00020000 .|. slot

baseCase :: Domain -> Case
baseCase executing =
  Case
    { caseName = CaseId ""
    , caseSlot = FlagSet
    , caseEntryAddress = Nothing
    , caseRetained = Nothing
    , caseObject = executing
    , caseObjectSlot = 1
    , caseInvocation = TaskInvocation
    , caseCpsr = 0x13
    , caseArgs = []
    , caseBoundary = CallerReturn
    , caseExpect = []
    , caseStockExpect = Nothing
    , caseDifference = NoDifference
    , caseSteps = []
    , caseWaker = Nothing
    , caseWaiters = []
    , caseFlags = []
    , caseMailboxes = []
    , caseMessages = []
    , caseFlagCounts = (120, 120)
    , caseMailboxCounts = (1, 1)
    , caseDispatch = 1
    , caseHandler = 0
    , caseTick = 0
    , caseReschedule = 0
    , caseRunningState = 1
    , caseThreadValue = Nothing
    , caseWords = []
    , caseLimit = plainLimit
    , caseStockStatus = Nothing
    , caseSkipTimerLog = False
    , caseSchedulerLock = Nothing
    , caseDirectInitial = Nothing
    , caseSelectedWait = Nothing
    }

retainedWords :: Facts -> Domain -> Case -> [(GuestAddress, Word32)]
retainedWords facts executing one = case caseRetained one of
  Nothing -> []
  Just kind ->
    let t = retainedTables executing
        timed = kind `elem` [RetainedSemaphoreTimedWait, RetainedDataQueueTimedReceive, RetainedFixedPoolTimedGet]
        callerTask = taskAt facts executing runningSlot
        timerCallback = seam executing "stock-timeout-callback"
        timerWords timer = [poke (timer + 4) timerCallback, poke (timer + 8) callerTask]
     in case kind of
          RetainedSemaphoreWait -> semaphoreWords t ++ (if timed then timerWords (rtSemTimer t) else [])
          RetainedSemaphoreTimedWait -> semaphoreWords t ++ timerWords (rtSemTimer t)
          RetainedDataQueueReceive -> dataQueueWords t ++ (if timed then timerWords (rtDqTimer t) else [])
          RetainedDataQueueTimedReceive -> dataQueueWords t ++ timerWords (rtDqTimer t)
          RetainedFixedPoolGet -> fixedPoolWords t ++ (if timed then timerWords (rtPoolTimer t) else [])
          RetainedFixedPoolTimedGet -> fixedPoolWords t ++ timerWords (rtPoolTimer t)
  where
    classRecord = recordAddress facts executing executing
    semaphoreWords t =
      [ poke (rtSemConfig t + 0x0) 0, poke (rtSemConfig t + 0x4) 0, poke (rtSemConfig t + 0x8) 1, poke (rtSemConfig t + 0xc) 1
      , poke (rtSemControl t) (rtSemControl t), poke (rtSemControl t + 0x4) (rtSemControl t)
      , poke (rtSemControl t + 0x8) (rtSemConfig t), poke (rtSemControl t + 0xc) 0
      , poke (rtSemWait t) 0, poke (rtSemWait t + 0x4) 0, poke (rtSemWait t + 0x8) 0
      , poke (rtSemTimer t) 0
      , poke (classRecord + 0x4c) (rtSemControl t), poke (classRecord + 0x50) 1
      ]
    dataQueueWords t =
      [ poke (rtDqConfig t + 0x0) 0, poke (rtDqConfig t + 0x4) 1, poke (rtDqConfig t + 0x8) 0, poke (rtDqConfig t + 0xc) 1
      , poke (rtDqRuntime t) (rtDqRuntime t), poke (rtDqRuntime t + 0x4) (rtDqRuntime t)
      , poke (rtDqRuntime t + 0x8) (rtDqConfig t)
      , poke (rtDqRuntime t + 0xc) (rtDqRuntime t + 0xc), poke (rtDqRuntime t + 0x10) (rtDqRuntime t + 0xc)
      , poke (rtDqRuntime t + 0x14) 0, poke (rtDqRuntime t + 0x18) 0, poke (rtDqRuntime t + 0x1c) 0
      , poke (rtDqWait t) 0, poke (rtDqWait t + 0x4) 0, poke (rtDqWait t + 0x8) 0, poke (rtDqWait t + 0xc) 0
      , poke (rtDqTimer t) 0
      , poke (classRecord + 0x5c) (rtDqRuntime t), poke (classRecord + 0x60) 1
      ]
    -- The fixed-pool caller loads r0 from [class_record+4], the one-based owner word, and
    -- passes r2 = 0x4005855c + (slot-1)*16; r0 is the word stored at WaitBlock+8, not the record address.
    fixedPoolWords t =
      [ poke (rtPoolConfig t + 0x0) 0, poke (rtPoolConfig t + 0x4) 0x10
      , poke (rtPoolConfig t + 0x8) poolStart, poke (rtPoolConfig t + 0xc) (poolStart + 0x10), poke (rtPoolConfig t + 0x10) 1
      , poke (rtPoolControl t) (rtPoolControl t), poke (rtPoolControl t + 0x4) (rtPoolControl t)
      , poke (rtPoolControl t + 0x8) (rtPoolConfig t)
      , poke (rtPoolControl t + 0xc) (poolStart + 0x10), poke (rtPoolControl t + 0x10) 0
      , poke (rtPoolWait t) 0, poke (rtPoolWait t + 0x4) 0, poke (rtPoolWait t + 0x8) 0, poke (rtPoolWait t + 0xc) 0
      , poke (rtPoolTimer t) 0
      , poke (classRecord + 0x6c) (rtPoolControl t), poke (classRecord + 0x70) 1
      ]

retainedCases :: Facts -> Domain -> [Case]
retainedCases facts executing =
  [ caseForRetained "semaphore-wait-signal" RetainedSemaphoreWait FlagWait
      [(R0, id0 1)] (TargetAddress (retainedSignal executing)) [(R0, id0 1)] False
      (semaphoreWaitChecks False)
  , caseForRetained "semaphore-timed-tick" RetainedSemaphoreTimedWait FlagTimedWait
      [(R0, id0 1), (R1, 5)] (TargetTimerTick 2) [] True
      (semaphoreWaitChecks True)
  , caseForRetained "semaphore-timed-forever" RetainedSemaphoreTimedWait FlagTimedWait
      [(R0, id0 1), (R1, 0xffffffff)] (TargetAddress (retainedSignal executing)) [(R0, id0 1)] False
      (semaphoreWaitChecks False ++ [eq (rtSemTimer t) 0])
  , caseForRetained "dataqueue-receive-send" RetainedDataQueueReceive FlagWait
      [(R0, id0 1), (R1, outWord)] (TargetAddress (retainedSend executing)) [(R0, id0 1), (R1, 0x1234)] False
      (dataQueueChecks False)
  , caseForRetained "dataqueue-timed-tick" RetainedDataQueueTimedReceive FlagTimedWait
      [(R0, id0 1), (R1, outWord), (R2, 5)] (TargetTimerTick 2) [] True
      (dataQueueChecks True)
  , caseForRetained "fixedpool-get-release" RetainedFixedPoolGet FlagWait
      [(R0, id0 1), (R1, outWord)] (TargetAddress (retainedRelease executing)) [(R0, id0 1), (R1, poolStart)] False
      (fixedPoolChecks False)
  , caseForRetained "fixedpool-timed-tick" RetainedFixedPoolTimedGet FlagTimedWait
      [(R0, id0 1), (R1, outWord), (R2, 5)] (TargetTimerTick 2) [] True
      (fixedPoolChecks True)
  ]
  where
    t = retainedTables executing
    objectLock = GuestAddress (dfObjectLock (domainFacts facts executing))
    schedulerLock = GuestAddress (dfSchedulerLock (domainFacts facts executing))
    locks = [NoReacquire objectLock, NoReacquire schedulerLock]
    taskWait = taskAt facts executing runningSlot + factsTaskWait facts
    ready = eq (readyHead facts executing 10) (taskAt facts executing runningSlot)
    caseForRetained name kind slot args target wakerArgs timed checks =
      (baseCase executing)
        { caseName = CaseId name
        , caseSlot = slot
        , caseEntryAddress = Just (retainedEntry kind executing)
        , caseRetained = Just kind
        , caseArgs = args
        , caseWaker = Just (Waker target wakerArgs)
        , caseExpect = checks ++ locks
        , caseSkipTimerLog = timed
        , caseTick = 100
        , caseThreadValue = Just 1
        , caseWords = if kind `elem` [RetainedDataQueueReceive, RetainedDataQueueTimedReceive, RetainedFixedPoolGet, RetainedFixedPoolTimedGet] then [poke outWord 0xdead] else []
        }
    semaphoreWaitChecks timed =
      [ if timed then res 0xffffffce else res 0
      , eq (rtSemWait t) (if timed then 0xffffffce else 0)
      , eq (rtSemWait t + 4) (rtSemControl t)
      , eq (rtSemWait t + 8) (domainId executing)
      , eq (rtSemControl t + 0xc) 0
      , eq taskWait (rtSemWait t)
      , ready
      ] ++ if timed then [eq (rtSemTimer t + 4) (seam executing "stock-timeout-callback"), eq (rtSemTimer t + 8) (taskAt facts executing runningSlot)] else []
    dataQueueChecks timed =
      [ if timed then res 0xffffffce else res 0
      , eq outWord (if timed then 0xdead else 0x1234)
      , eq (rtDqWait t) (if timed then 0xffffffce else 0)
      , eq (rtDqWait t + 4) (rtDqRuntime t)
      , eq (rtDqWait t + 8) (domainId executing)
      , eq (rtDqWait t + 0xc) (if timed then 0 else 0x1234)
      , eq taskWait (rtDqWait t)
      , ready
      ] ++ if timed then [eq (rtDqTimer t + 4) (seam executing "stock-timeout-callback"), eq (rtDqTimer t + 8) (taskAt facts executing runningSlot)] else []
    fixedPoolChecks timed =
      [ if timed then res 0xffffffce else res 0
      , eq outWord (if timed then 0xdead else poolStart)
      , eq (rtPoolWait t) (if timed then 0xffffffce else 0)
      , eq (rtPoolWait t + 4) (rtPoolControl t)
      , eq (rtPoolWait t + 8) (domainId executing)
      , eq (rtPoolWait t + 0xc) (if timed then 0 else poolStart)
      , eq taskWait (rtPoolWait t)
      , ready
      ] ++ if timed then [eq (rtPoolTimer t + 4) (seam executing "stock-timeout-callback"), eq (rtPoolTimer t + 8) (taskAt facts executing runningSlot)] else []

caseStockExpectations :: Case -> [Expect]
caseStockExpectations one = fromMaybe (caseExpect one) (caseStockExpect one)

caseBoundaryStatus :: Case -> RunnerStatus
caseBoundaryStatus one = case caseBoundary one of
  CallerReturn -> Returned
  TaskArrival -> StopAddress

roleStatus :: Role -> Case -> RunnerStatus
roleStatus StockRole one = fromMaybe (caseBoundaryStatus one) (caseStockStatus one)
roleStatus OwnedRole one = caseBoundaryStatus one

isBudgetRole :: Role -> Case -> Bool
isBudgetRole role one = role == StockRole && caseStockStatus one == Just InstructionBudget

callerSp :: Case -> Word32
callerSp one = if caseInvocation one == HandlerCallback then handlerStackPointer else stackPointer

callerBytes :: B.ByteString
callerBytes = B.concat [word32le (0xca11e500 + value) | value <- [0 .. 15]]

data Code = Code { codeNext :: Word32, codeWords :: [Word32] }

codeAt :: Word32 -> Code
codeAt base = Code base []

emit :: Code -> Word32 -> Code
emit code word = Code (codeNext code + 4) (word : codeWords code)

blTo :: Code -> Word32 -> Code
blTo code target = emit code (0xeb000000 .|. (((target - codeNext code - 8) `div` 4) .&. 0xffffff))

assembleCode :: Code -> B.ByteString
assembleCode = assemble . reverse . codeWords

-- | AAPCS call arguments: r0-r3 in registers, further arguments on the stack at [sp].
data CallArg = RegArg Register Word32 | StackArg Int Word32

loadArgsCode :: Code -> [CallArg] -> Code
loadArgsCode = foldl one
  where
    one code (RegArg register value) = emit (emit code (movw register (value .&. 0xffff))) (movt register (value `div` 0x10000))
    one code (StackArg offset value) =
      let withValue = loadAddressCode code R12 value
       in emit withValue (0xe58dc000 .|. (fromIntegral offset .&. 0xfff))

-- | The fifth and later arguments of a stock slot live on the stack.
slotArgs :: Slot -> [(Register, Word32)] -> [CallArg]
slotArgs slot args = case slot of
  FlagTimedWait -> regs ++ stack
  _ -> map (\(register, value) -> RegArg register value) args
  where
    regs = [RegArg register value | (register, value) <- args, registerNumber register <= 3]
    stack = [StackArg (4 * (i - 4)) value | (i, (_, value)) <- zip [0 ..] args, i >= 4]

loadAddressCode :: Code -> Register -> Word32 -> Code
loadAddressCode code register address =
  emit (emit code (movw register (address .&. 0xffff))) (movt register (address `div` 0x10000))

movw :: Register -> Word32 -> Word32
movw register immediate = 0xe3000000 .|. ((immediate .&. 0xf000) `shiftL` 4) .|. (registerNumber register `shiftL` 12) .|. (immediate .&. 0xfff)

movt :: Register -> Word32 -> Word32
movt register immediate = 0xe3400000 .|. ((immediate .&. 0xf000) `shiftL` 4) .|. (registerNumber register `shiftL` 12) .|. (immediate .&. 0xfff)

registerNumber :: Register -> Word32
registerNumber register = case register of
  R0 -> 0
  R1 -> 1
  R2 -> 2
  R3 -> 3
  R4 -> 4
  R5 -> 5
  R6 -> 6
  R7 -> 7
  R8 -> 8
  R9 -> 9
  R10 -> 10
  R11 -> 11
  R12 -> 12
  _ -> error "only r0-r12 are loaded by a stub"

wrapperCode :: Facts -> Entries -> Domain -> Case -> B.ByteString
wrapperCode _ entries executing one = assembleCode (emit (emit (emit called 0xe28dd00c) 0xe49de004) 0xe12fff1e)
  where
    start = emit (emit (codeAt wrapperBase) 0xe52de004) 0xe24dd00c
    called = foldl step (blTo (loadArgsCode start (slotArgs (caseSlot one) (caseArgs one))) (entryAddress entries executing (caseSlot one))) (caseSteps one)
    step code (StepCall slot args) = blTo (loadArgsCode code (slotArgs slot args)) (entryAddress entries executing slot)
    step code (StepStore address) = emit (loadAddressCode code R12 (pointerWord address)) 0xe58c0000

stubCode :: Facts -> Entries -> Domain -> Word32 -> WakerTarget -> [(Register, Word32)] -> B.ByteString
stubCode facts entries executing base target args = assembleCode (case target of TargetSlot slot -> singleCode (entryAddress entries executing slot) args; TargetAddress address -> singleCode address args; TargetTimerTick count -> tickCode count)
  where
    singleCode targetAddress args' =
      let c0 = emit (codeAt base) 0xf1080080
          c1 = loadArgsCode c0 (map (\(register, value) -> RegArg register value) args')
          c2 = blTo c1 targetAddress
          c3 = emit c2 0xe3a00000
          c4 = blTo c3 (s2Save executing)
       in emit c4 0xeafffffe
    tickCode count =
      let handlerContext = dfHandlerContext (domainFacts facts executing)
          c0 = emit (codeAt base) 0xf1080080
          c1 = emit (emit c0 (movw R11 (handlerContext .&. 0xffff))) (movt R11 (handlerContext `div` 0x10000))
          c2 = emit c1 0xe3a00001
          c3 = emit c2 0xe58b0000
          c4 = foldl (\c _ -> blTo c (seam executing "stock-timer-tick")) c3 [1 .. count]
          c5 = emit c4 0xe3a00000
          c6 = emit c5 0xe58b0000
          c7 = emit c6 0xe3a00000
          c8 = blTo c7 (s2Save executing)
       in emit c8 0xeafffffe

selectedWaitCodeAt :: Word32 -> Word32 -> B.ByteString
selectedWaitCodeAt base entry = assembleCode stubTail
  where
    start = emit (emit (emit (codeAt base) 0xf1080080) 0xe52de004) 0xe24dd00c
    called = blTo (loadArgsCode start (map (\(register, value) -> RegArg register value) [(R0, id0 1), (R1, 1), (R2, 0), (R3, outWord)])) entry
    stubTail = emit (emit (emit called 0xe28dd00c) 0xe49de004) 0xe12fff1e

continuationBytes :: Facts -> Entries -> Domain -> Case -> B.ByteString
continuationBytes facts entries executing one = installedSelected
  where
    installedSelected = case caseSelectedWait one of
      Nothing -> installedChain
      Just entry -> install actorBase (selectedWaitCodeAt actorBase entry) installedChain
    zero = B.replicate (fromIntegral (stopAddress - exitAddress + 4)) 0
    installedExit = install exitAddress (assemble [0xe12fff1e]) zero
    installedWrapper = install wrapperBase (wrapperCode facts entries executing one) installedExit
    installedWaker = case caseWaker one of
      Nothing -> installedWrapper
      Just waker -> install wakerStubBase (stubCode facts entries executing wakerStubBase (wakerTarget waker) (wakerArgs waker)) installedWrapper
    installedChain = case [chain | Just chain <- map pwChain (caseWaiters one)] of
      (chain : _) -> install chainStubBase (stubCode facts entries executing chainStubBase (wakerTarget chain) (wakerArgs chain)) installedWaker
      [] -> installedWaker


install :: Word32 -> B.ByteString -> B.ByteString -> B.ByteString
install address bytes image = B.take offset image <> bytes <> B.drop (offset + B.length bytes) image
  where offset = fromIntegral (address - exitAddress)

pages :: Facts -> Case -> [Page]
pages facts one =
  [ let (address, size) = kernelBounds facts domain in page (if domain == DomainA then "kernel-a" else "kernel-b") address size ReadWrite
  | domain <- [DomainA, DomainB]
  ]
  ++ [ page "definitions" (GuestAddress definitionBase) (fromIntegral (2 * factsTaskSlots facts * definitionStride)) ReadWrite
     , page "messages" (GuestAddress 0x40000000) 0x100 ReadWrite
     , page "timer-seam" (GuestAddress receiptAddress) 0x100 ReadWrite
     , page "global-lock" (GuestAddress globalLock) 4 ReadWrite
     , Page (RegionId "stack") (GuestAddress (callerSp one - 0x400)) (0x400 + B.length callerBytes) ReadWrite
         (B.replicate 0x400 0 <> callerBytes)
     , page "target-stack" (GuestAddress (targetStackPointer - 0x100)) 0x100 ReadWrite
     , page "waker-stack" (GuestAddress (wakerStack - 0x400)) 0x400 ReadWrite
     , page "chain-stack" (GuestAddress wakerStack) (fromIntegral (chainStack - wakerStack)) ReadWrite
     , page "pool" (GuestAddress poolStart) 0x100 ReadWrite
     , page "bank-a" (bank DomainA) 4 Device
     , page "bank-b" (bank DomainB) 4 Device
     , let outOfRange = dfFlagControls (domainFacts facts (caseObject one)) + 122 * factsFlagControlSize facts
       in page (if caseObject one == DomainA then "out-of-range-a" else "out-of-range-b") (GuestAddress outOfRange) 0x100 ReadWrite
     ]
  where page name address size permission = Page (RegionId name) address size permission (B.replicate size 0)

contains :: GuestAddress -> Int -> Page -> Bool
contains (GuestAddress address) width (Page _ (GuestAddress base) size _ _) =
  toInteger base <= toInteger address && toInteger address + toInteger width <= toInteger base + toInteger size

overlay :: GuestAddress -> B.ByteString -> [(GuestAddress, Word32)] -> B.ByteString
overlay (GuestAddress start) initial words' = B.concat (pieces 0 ordered)
  where
    ordered = M.toAscList (M.fromList [(address, value) | (address@(GuestAddress raw), value) <- words', raw >= start, toInteger raw + 4 <= toInteger start + toInteger (B.length initial)])
    pieces cursor [] = [B.drop cursor initial]
    pieces cursor ((GuestAddress address, value) : rest) =
      let offset = fromIntegral (address - start)
       in B.take (offset - cursor) (B.drop cursor initial) : word32le value : pieces (offset + 4) rest

kernelBounds :: Facts -> Domain -> (GuestAddress, Int)
kernelBounds facts domain = (GuestAddress low, fromIntegral (high - low))
  where
    words' = domainFacts facts domain
    low = minimum [dfHandlerContext words', dfCurrentContext words', dfContexts words', dfSchedulerLock words', dfObjectLock words']
    high = maximum
      [ dfContextA words' + factsContextSize facts, dfContextB words' + factsContextSize facts
      , dfReadyHeads words' + factsPriorities facts * factsHeadSize facts, dfSelected words' + 4
      , dfRunning words' + 4, dfDispatchEnabled words' + 4, dfReschedule words' + 4
      , dfTickNow words' + 4, dfTimerBase words' + 4, dfTimerCount words' + 4
      , dfTimerHeap words' + 0x100
      , dfTaskTable words' + factsTaskSlots facts * factsTaskSize facts
      , dfFlagConfigs words' + 120 * factsFlagConfigSize facts
      , dfFlagControls words' + 120 * factsFlagControlSize facts
      , dfFlagWaits words' + factsTaskSlots facts * factsFlagWaitSize facts
      , dfFlagTimers words' + factsTaskSlots facts * factsTimerEntrySize facts
      , dfMailboxConfigs words' + factsMailboxConfigSize facts
      , dfMailboxControls words' + factsMailboxControlSize facts
      , dfMailboxWaits words' + factsTaskSlots facts * factsMailboxWaitSize facts
      , dfMailboxTimers words' + factsTaskSlots facts * factsTimerEntrySize facts
      ]

codeBytes :: Domain -> Image -> Image -> B.ByteString
codeBytes domain loader owned = codeBytesWith domain loader [(owned, extents domain)]

-- | The staged family candidate plus the qualified S3b callers, each over its own image.
codeBytesJoined :: Domain -> Image -> Image -> Image -> B.ByteString
codeBytesJoined domain loader owned s3b =
  codeBytesWith domain loader [(owned, extents domain), (s3b, s3bPlacement domain)]

codeBytesWith :: Domain -> Image -> [(Image, [(Word32, Word32)])] -> B.ByteString
codeBytesWith domain loader patches = installAt timerAddress timerStub (foldl patchImage original patches)
  where
    base = imageBase loader
    timerAddress = timerEntry domain
    slice image address size =
      if address < imageBase image || toInteger address + toInteger size > toInteger (imageBase image) + toInteger (B.length (imageBytes image))
        then error "family code slice is outside its image"
        else B.take (fromIntegral size) (B.drop (fromIntegral (address - imageBase image)) (imageBytes image))
    original = slice loader base (B.length (imageBytes loader))
    installAt address bytes current = let offset = fromIntegral (address - base) in B.take offset current <> bytes <> B.drop (offset + B.length bytes) current
    patchImage current (image, placement) = foldl (\bytes (address, size) -> installAt address (slice image address size) bytes) current placement

fixtureWords :: Facts -> Domain -> Case -> [(GuestAddress, Word32)]
fixtureWords facts executing one =
  layoutWords facts executing one
    ++ concatMap flagObject (caseFlags one)
    ++ concatMap mailboxObject (caseMailboxes one)
    ++ concatMap messageObject (caseMessages one)
    ++ queueLinks facts one (caseWaiters one)
    ++ concatMap (waiterWords facts executing one) (caseWaiters one)
    ++ retainedWords facts executing one
    ++ caseWords one
  where
    flagObject (domain, slot, attributes, initial, pattern) =
      let config = flagConfig facts domain slot
          controlAddress = flagControl facts domain slot
       in [ poke (config + factsFlagConfigAttributes facts) attributes
          , poke (config + factsFlagConfigInitial facts) initial
          , poke (config + factsFlagConfigExists facts) 1
          , poke (controlAddress + factsFlagControlPattern facts) pattern
          , poke controlAddress controlAddress
          , poke (controlAddress + factsWaitQueuePrev facts) controlAddress
          , poke (controlAddress + factsWaitQueueConfig facts) config
          ]
    mailboxObject (domain, slot, attributes, limit, headValue, tailValue) =
      let config = mailboxConfig facts domain slot
          controlAddress = mailboxControl facts domain slot
       in [ poke (config + factsMailboxConfigAttributes facts) attributes
          , poke (config + factsMailboxConfigPriorityLimit facts) limit
          , poke (config + factsMailboxConfigExists facts) 1
          , poke (controlAddress + factsMailboxControlHead facts) headValue
          , poke (controlAddress + factsMailboxControlTail facts) tailValue
          , poke controlAddress controlAddress
          , poke (controlAddress + factsWaitQueuePrev facts) controlAddress
          , poke (controlAddress + factsWaitQueueConfig facts) config
          ]
    messageObject (address, next, priority) = [poke address next, poke (address + 4) priority]

queueLinks :: Facts -> Case -> [PreWaiter] -> [(GuestAddress, Word32)]
queueLinks _ _ [] = []
queueLinks facts one waiters@(firstWaiter : _) =
  let queue = objectQueue facts one
      tasks = [taskAt facts (pwDomain pw) (pwSlot pw) | pw <- waiters]
      firstTask = taskAt facts (pwDomain firstWaiter) (pwSlot firstWaiter)
      lastWaiter = foldl (\_ pw -> pw) firstWaiter (drop 1 waiters)
      lastTask = taskAt facts (pwDomain lastWaiter) (pwSlot lastWaiter)
      links = zip3 (queue : tasks) tasks (drop 1 tasks ++ [queue])
   in [ poke (queue + factsWaitQueueNext facts) firstTask
      , poke (queue + factsWaitQueuePrev facts) lastTask
      ]
        ++ concat
          [ [ poke (queueTask + factsTaskNext facts) next
            , poke (queueTask + factsTaskPrev facts) previous
            ]
          | (previous, queueTask, next) <- links
          ]

waiterWords :: Facts -> Domain -> Case -> PreWaiter -> [(GuestAddress, Word32)]
waiterWords facts executing one pw =
  [ poke (waitTask + factsTaskDomain facts) (domainId (pwDomain pw))
  , poke (waitTask + factsTaskState facts) (packedState (factsStateWait facts .|. factsStateObjectQueued facts .|. factsStateOwnerLockQueued facts) (pwPriority pw))
  , poke (waitTask + factsTaskWait facts) wait
  , poke (waitTask + factsTaskSavedSp facts) (if isJust (pwChain pw) then chainStack else targetStackPointer)
  , poke (waitTask + factsTaskResumePc facts) (if isJust (pwChain pw) then chainStubBase else stopAddress)
  , poke (wait + factsWaitBlockQueue facts) queue
  , poke (wait + factsWaitBlockOwner facts) (dfId (domainFacts facts (caseObject one)))
  , poke (wait + factsWaitBlockResult facts) (if pwTimer pw then timerEntryAddress else 0)
  , poke (definitionAddress facts (pwDomain pw) (pwSlot pw) + factsDefinitionPriority facts) (pwPriority pw)
  ]
    ++ ( if slotFamily (caseSlot one) == FlagFamily
           then [poke (wait + factsFlagWaitPattern facts) (pwWant pw), poke (wait + factsFlagWaitMode facts) (pwMode pw)]
           else []
       )
    ++ ( if pwTimer pw
           then [ poke (timerEntryAddress + 4) (seam executing "stock-timeout-callback")
                , poke (timerEntryAddress + 8) waitTask
                ]
           else []
       )
  where
    waitTask = taskAt facts (pwDomain pw) (pwSlot pw)
    wait = objectWait facts (caseSlot one) (pwDomain pw) (pwSlot pw)
    queue = objectQueue facts one
    timerEntryAddress = objectTimer facts (caseSlot one) (pwDomain pw) (pwSlot pw)

objectQueue :: Facts -> Case -> Word32
objectQueue facts one = case slotFamily (caseSlot one) of
  FlagFamily -> flagControl facts (caseObject one) (caseObjectSlot one)
  MailboxFamily -> mailboxControl facts (caseObject one) (caseObjectSlot one)

objectWait :: Facts -> Slot -> Domain -> TaskSlot -> Word32
objectWait facts slot domain taskSlot = case slotFamily slot of
  FlagFamily -> flagWait facts domain taskSlot
  MailboxFamily -> mailboxWait facts domain taskSlot

objectTimer :: Facts -> Slot -> Domain -> TaskSlot -> Word32
objectTimer facts slot domain taskSlot = case slotFamily slot of
  FlagFamily -> dfFlagTimers (domainFacts facts domain) + (taskSlot - 1) * factsTimerEntrySize facts
  MailboxFamily -> dfMailboxTimers (domainFacts facts domain) + (taskSlot - 1) * factsTimerEntrySize facts

domainId :: Domain -> Word32
domainId DomainA = 1
domainId DomainB = 2

layoutWords :: Facts -> Domain -> Case -> [(GuestAddress, Word32)]
layoutWords facts executing one =
  concat [contextWords block described | block <- domains, described <- domains]
    ++ concatMap taskWords domains
    ++ concatMap objectBase domains
    ++ concatMap domainState domains
  where
    domains = [DomainA, DomainB]
    countFor (a, b) domain = if domain == DomainA then a else b
    contextWords block described =
      let record = recordAddress facts block described
          words' = domainFacts facts described
          (acquire, release) = lockEntries block
          lock offset address = [poke (record + offset + factsLockWord facts) address, poke (record + offset + factsLockAcquire facts) acquire, poke (record + offset + factsLockRelease facts) release]
       in [ poke (record + factsContextId facts) (if block == described then factsOwnId facts else dfId words')
          , poke (record + factsContextOwner facts) (dfId words')
          , poke (record + factsContextReadyHeads facts) (dfReadyHeads words')
          , poke (record + factsContextReadyBitmap facts) (dfReadyBitmap words')
          , poke (record + factsContextRunning facts) (dfRunning words')
          , poke (record + factsContextSelected facts) (dfSelected words')
          , poke (record + factsContextDispatchEnabled facts) (dfDispatchEnabled words')
          , poke (record + factsContextRunning facts + 0xc) (dfTimerHeap words')
          , poke (record + factsContextRunning facts + 0x10) (dfTimerCount words')
          , poke (record + factsContextRunning facts + 0x14) (dfTimerBase words')
          , poke (record + factsContextRunning facts + 0x18) (dfTickNow words')
          , poke (record + factsContextFirstEntry facts) (trampoline described)
          , poke (record + factsContextTasks facts) (dfTaskTable words')
          , poke (record + factsContextTaskCount facts) (factsTaskSlots facts)
          , poke (record + factsContextFlags facts) (dfFlagControls words')
          , poke (record + factsContextFlagCount facts) (countFor (caseFlagCounts one) described)
          , poke (record + factsContextMailboxes facts) (dfMailboxControls words')
          , poke (record + factsContextMailboxCount facts) (countFor (caseMailboxCounts one) described)
          ]
            ++ lock (factsContextSchedulerLock facts) (dfSchedulerLock words') ++ lock (factsContextObjectLock facts) (dfObjectLock words')
    taskWords domain = concat
      [ let address = taskAt facts domain slot
            definition = definitionAddress facts domain slot
            priority = taskPriority slot
         in [ poke address address, poke (address + factsTaskPrev facts) address
            , poke (address + factsTaskDefinition facts) definition, poke (address + factsTaskDomain facts) (dfId (domainFacts facts domain))
            , poke (address + factsTaskState facts) (packedState (factsStateDormant facts) priority)
            , poke (address + factsTaskSavedSp facts)
                (if domain == executing && slot == waiterSlot && isJust (caseSelectedWait one) then actorStackHigh
                 else if slot == wakerSlot && isJust (caseWaker one) && domain == executing then wakerStack
                 else targetStackPointer)
            , poke (address + factsTaskResumePc facts)
                (if domain == executing && slot == waiterSlot && isJust (caseSelectedWait one) then actorBase
                 else if slot == wakerSlot && isJust (caseWaker one) && domain == executing then wakerStubBase
                 else stopAddress)
            , poke (definition + factsDefinitionPriority facts) priority
            , poke (definition + 4) (0xa5510000 + slot), poke (definition + 8) stopAddress
            , poke (definition + 0x10) (targetStackPointer - 0x100), poke (definition + 0x14) 0x100
            , poke (definition + 0x20) (if slot == runningSlot && domain == executing then fromMaybe (threadValue domain slot) (caseThreadValue one) else threadValue domain slot)
            ]
      | slot <- [1 .. factsTaskSlots facts]
      ]
    taskPriority slot
      | slot == runningSlot = 10
      | slot == idleSlot = 15
      | slot == wakerSlot && isJust (caseWaker one) = 14
      | slot == waiterSlot = 3
      | otherwise = slot `mod` factsPriorities facts
    objectBase domain =
      concat
        [ let config = flagConfig facts domain slot
              controlAddress = flagControl facts domain slot
           in [ poke (config + factsFlagConfigExists facts) 0
              , poke (config + factsFlagConfigAttributes facts) 0
              , poke (config + factsFlagConfigInitial facts) 0
              , poke controlAddress controlAddress, poke (controlAddress + factsWaitQueuePrev facts) controlAddress
              , poke (controlAddress + factsWaitQueueConfig facts) config
              , poke (controlAddress + factsFlagControlPattern facts) 0
              ]
        | slot <- [1 .. 120]
        ]
        ++ let config = mailboxConfig facts domain 1
               controlAddress = mailboxControl facts domain 1
            in [ poke (config + factsMailboxConfigExists facts) 0
               , poke (config + factsMailboxConfigAttributes facts) 0
               , poke (config + factsMailboxConfigPriorityLimit facts) 0
               , poke controlAddress controlAddress, poke (controlAddress + factsWaitQueuePrev facts) controlAddress
               , poke (controlAddress + factsWaitQueueConfig facts) config
               , poke (controlAddress + factsMailboxControlHead facts) 0
               , poke (controlAddress + factsMailboxControlTail facts) 0
               ]
    domainState domain =
      let words' = domainFacts facts domain
          ready = [ (runningSlot, 10), (idleSlot, 15) ] ++ [ (wakerSlot, 14) | domain == executing, isJust (caseWaker one) ]
          headAt priority = dfReadyHeads words' + priority * factsHeadSize facts
          member slot priority =
            let address = taskAt facts domain slot
                headAddress = headAt priority
             in [ poke headAddress address, poke (headAddress + factsHeadPrev facts) address
                , poke address headAddress, poke (address + factsTaskPrev facts) headAddress
                , poke (address + factsTaskState facts) (packedState (if slot == runningSlot then caseRunningState one else factsStateReady facts) priority)
                ]
          bitmap = foldl' (.|.) 0 [1 `shiftL` fromIntegral priority | (_, priority) <- ready]
       in concat [[poke (headAt priority) (headAt priority), poke (headAt priority + factsHeadPrev facts) (headAt priority)] | priority <- [0 .. factsPriorities facts - 1]]
            ++ concat [member slot priority | (slot, priority) <- ready]
            ++ [ poke (dfReadyBitmap words') bitmap
               , poke (dfRunning words') (taskAt facts domain runningSlot)
               , poke (dfSelected words') (taskAt facts domain runningSlot)
               , poke (dfDispatchEnabled words') (if domain == executing then caseDispatch one else 1)
               , poke (dfSchedulerLock words') 0, poke (dfObjectLock words') 0
               , poke (dfCurrentContext words') (recordAddress facts domain domain)
               , poke (dfHandlerContext words') (if domain == executing then caseHandler one else 0)
               , poke (dfReschedule words') (if domain == executing then caseReschedule one else 0)
               , poke (dfTickNow words') (if domain == executing then caseTick one else 0)
               , poke (dfTimerBase words') 0, poke (dfTimerCount words') 0
               ]

caseFor :: Role -> Facts -> Entries -> Domain -> Image -> Image -> Case -> CaseSpec
caseFor role facts entries executing loader owned = caseForCode role facts entries executing loader (codeBytes executing loader owned) []

caseForCode :: Role -> Facts -> Entries -> Domain -> Image -> B.ByteString -> [(Word32, Word32)] -> Case -> CaseSpec
caseForCode role facts entries executing loader code extraFunctions one = CaseSpec
  { caseId = emittedId executing one
  , caseProvenance = provenance
  , caseExclusion = exclusion one
  , caseEntry = wireAddress (GuestAddress (fromMaybe wrapperBase (caseEntryAddress one)))
  , caseExit = wireAddress (GuestAddress exitAddress)
  , caseRegions =
      [ mkRegion (RegionId "code") (wireAddress (GuestAddress (imageBase loader))) (fromIntegral (B.length (imageBytes loader))) ReadExecute code
      ]
        ++ [ mkRegion ident (wireAddress address) size permission (overlay address background words') | Page ident address size permission background <- dataPages ]
        ++ [ mkRegion (RegionId "continuations") (wireAddress (GuestAddress exitAddress)) (fromIntegral (stopAddress - exitAddress + 4)) ReadExecute (continuationBytes facts entries executing one) ]
  , caseFunctions = functionList
  , caseInitial = case caseDirectInitial one of
      Just registers -> registers
      Nothing -> if isJust (caseRetained one) then retainedInitialRegisters one else initialRegisters one
  , caseExpectedStatus = roleStatus role one
  , caseExpectedRegisters = expectedRegisters role one
  , caseExpectedMemory = []
  , caseCp14Identity = Just (if executing == DomainA then 0x4003 else 0x4004)
  , caseKernel = Just (KernelCase (caseLimit one) (if caseBoundary one == TaskArrival then Just (wireAddress (GuestAddress stopAddress)) else Nothing) (Just (initialThread one)) (Just 0) (Just 0) ignored (continuationStacks one))
  }
  where
    dataPages = pages facts one
    -- The loader's divide helper, called by the retained pool release.
    baseExtents
      | role == StockRole && isJust (caseSelectedWait one) = filter (\(address, _) -> address /= 0x44744 && address /= 0xd4704) (extents executing)
      | otherwise = extents executing
    stockHelpers DomainA = [(0x4da6c, 0x34), (0x4d978, 0xe8)]
    stockHelpers DomainB = [(0xdda30, 0x30), (0xdd93c, 0xe8)]
    stockWaitHelpers DomainA = [(0x4471c, 0xc8), (0x447e4, 0x1c)]
    stockWaitHelpers DomainB = [(0xd46dc, 0xc8), (0xd47a4, 0x1c)]
    words' =
      let fixture = fixtureWords facts executing one
          outside = [address | (address, _) <- fixture, not (any (contains address 4) dataPages)]
       in if null outside then fixture else error (caseIdText (caseName one) ++ ": unmapped fixture words " ++ show outside)
    functionList =
      [ (wireAddress (GuestAddress address), size) | (address, size) <- baseExtents ++ retained executing ++ retainedExtents executing ++ extraFunctions ]
        ++ [ (wireAddress (GuestAddress address), size) | (address, size) <- stockHelpers executing ]
        ++ [ (wireAddress (GuestAddress address), size) | role == StockRole, isJust (caseSelectedWait one), (address, size) <- stockWaitHelpers executing ]
        ++ [ (wireAddress (GuestAddress address), size)
           | Seam name _ address size <- retainedSeams executing
           , not (role == StockRole && isJust (caseSelectedWait one) && name == "stock-remove-timer-helper")
           ]
        ++ [ (wireAddress (GuestAddress wrapperBase), 0x100) ]
        ++ [ (wireAddress (GuestAddress wakerStubBase), 0x80) | isJust (caseWaker one) ]
        ++ [ (wireAddress (GuestAddress actorBase), 0x80) | isJust (caseSelectedWait one) ]
        ++ [ (wireAddress (GuestAddress chainStubBase), 0x80) | not (null (mapMaybe pwChain (caseWaiters one))) ]
    ignored = if caseBoundary one == CallerReturn then [R1, R2, R3, R12, Lr] else [R0 .. R12] ++ [Lr]
    -- TPIDRURW holds the one-based local slot; bit 31 marks the handler context.
    initialThread one'
      | caseBoundary one' == TaskArrival = selectedThreadValue one'
      | caseInvocation one' == HandlerCallback = runningSlot .|. 0x80000000
      | otherwise = runningSlot
    -- A waker resumes the blocked caller through the saved frame, so its four synthetic
    -- stacks are declared; a single-context or declared-stop case keeps the empty list.
    continuationStacks one'
      | isJust (caseWaker one') =
          map stack
            [ (callerSp one' - 0x400, callerSp one')
            , (targetStackPointer - 0x100, targetStackPointer)
            , (wakerStack - 0x400, wakerStack)
            , (wakerStack, chainStack)
            ]
      | isJust (caseSelectedWait one') =
          map stack
            [ (callerSp one' - 0x400, callerSp one')
            , (actorStackLow, actorStackHigh)
            ]
      | otherwise = []
    stack (low, high) = either error id (mkStackContext (N.GuestAddress (fromIntegral low)) (N.GuestAddress (fromIntegral high)))
    provenance = "X-M1 1.50 stock slot ABI at pinned x70view 55436fc85c0560dd252b0410af6f2427fe198cd3; manifest entries from replacement/media_permit/rtos_slot_contracts.json, extents from stage/family-{a,b}.ld; synthetic caller, waker, waiter, message and timer-entry fixtures; retained stock insert-timer, remove-timer, timeout-callback and timer-tick seams; retained semaphore, data-queue and fixed-pool caller entries, wait/timer tables and the fixed-pool wait table recovered from the pinned disassembly (A 0x4005855c / B 0x400e851c, stride 16)"
      ++ (if isJust (caseSchedulerLock one) then "; the six S3b service extents from regions-s3b.json and the qualified s3b-{a,b}.bin" else "")
    exclusion one' = baseExclusion ++ differenceExclusion (caseDifference one') ++ retainedExclusion one' ++ joinedExclusion one'
    joinedExclusion one'
      | isJust (caseSchedulerLock one') = " The qualified S3b caller executes over the staged family release at the stock wait-release entry; the release's timer removal runs through the stock remove-timer logging seam. The task is a fixture sleeping task; the S3b caller, not the fixture, performs the release."
      | otherwise = ""
    retainedExclusion one'
      | isJust (caseRetained one') = " The retained caller executes at its pinned stock slot entry and uses the stock caller body; the shared wait/check helper is stock or owned at the same ABI. The object wait/timer tables are the pinned slot-contract addresses. The fixed-pool wait table is recovered from the pinned disassembly, not guessed."
      | otherwise = ""
    baseExclusion = "Private compiler frames are not byte-compared. Synthetic waker/waiter tasks and messages are fixture state; the operation under test performs the wake or teardown. Timed expiry executes the retained stock timer tick through a synthetic handler-context waker: the tick pops the inserted entry and calls the timeout callback under the scheduler lock. Direct callback-only invocation is not used as expiry or lifetime evidence. The tick's entry-clearing hazard and heap capacity remain retained. Handler callbacks enter SVC with the handler-context word, not IRQ mode. Caller-frame bytes at callerSp are compared; scratch below SP is not."
    differenceExclusion NoDifference = ""
    differenceExclusion DeleteStrandsWaiter = " Scoped difference: stock delete strands the queued waiter; owned unlinks, completes it with -49 and cancels its timer."
    differenceExclusion HandlerWaiterDomain = " Scoped difference: stock +0x8c releases through the flag's domain; owned uses the waiter's domain."
    differenceExclusion AliasNextLink = " Scoped difference: stock reads the message link after the output write; owned reads the successor first."
    differenceExclusion AliasAttributes = " Scoped difference: stock reads the clear attribute after the output write; owned decides the clear first."
    differenceExclusion AliasPatternClear = " Scoped difference: stock event_flag_check stores the caller output before the clear; owned clears first, so an output word aliasing the pattern cell differs. Mirrors the host wait_flag_clear_on_match_writes_the_caller_output_after_the_clear behavior."
    differenceExclusion CreatePublication = " Scoped difference: the publication order and its barrier are compared from the ordered write log."
    differenceExclusion DeleteLeavesQueueTail = " Scoped difference: stock leaves the mailbox queue tail word at its last message when invalidating the object; owned clears it. Only that four-byte tail word is excluded; every other kernel word is compared."
    differenceExclusion OutOfRangeWrite = " Scoped difference: stock indexes outside the table; owned refuses -18 without a write."

expectedRegisters :: Role -> Case -> [(Register, Word32)]
expectedRegisters role one
  | isBudgetRole role one = [(R0, 0)]
  | caseBoundary one == TaskArrival = [(Pc, stopAddress), (Sp, targetStackPointer)]
  | otherwise =
      (R0, roleResult role one) : [(register, seed register) | register <- [R4 .. R11]] ++ [(Sp, callerSp one)]

roleResult :: Role -> Case -> Word32
roleResult role one = case [value | Result value <- roleExpect role one] of
  [value] -> value
  _ -> 0

roleExpect :: Role -> Case -> [Expect]
roleExpect StockRole one = caseStockExpectations one
roleExpect OwnedRole one = caseExpect one

initialRegisters :: Case -> [(Register, Word32)]
initialRegisters one =
  [(register, seed register) | register <- [R0 .. R12]]
    ++ [(Sp, callerSp one), (Lr, exitAddress), (Cpsr, caseCpsr one)]

retainedInitialRegisters :: Case -> [(Register, Word32)]
retainedInitialRegisters one =
  [ (register, maybe (seed register) id (lookup register (caseArgs one))) | register <- [R0 .. R12] ]
    ++ [(Sp, callerSp one), (Lr, exitAddress), (Cpsr, caseCpsr one)]

seed :: Register -> Word32
seed register = 0xa5a50000 + fromIntegral (fromEnum register)

emittedId :: Domain -> Case -> String
emittedId executing one = caseIdText (caseName one) ++ "-" ++ domainLabel executing

readWord :: Facts -> Case -> NativeObservation -> GuestAddress -> Maybe Word32
readWord facts one observation address@(GuestAddress raw) = case
  [(base, bytes) | page@(Page ident (GuestAddress base) _ _ _) <- pages facts one, contains address 4 page, Just bytes <- [lookup ident (nativeMemory observation)]] of
  (base, bytes) : _ -> Just (readWordAt bytes (fromIntegral (raw - base)))
  [] -> Nothing

readByte :: Facts -> Case -> NativeObservation -> GuestAddress -> Maybe Word8
readByte facts one observation address@(GuestAddress raw) = case
  [(base, bytes) | page@(Page ident (GuestAddress base) _ _ _) <- pages facts one, contains address 1 page, Just bytes <- [lookup ident (nativeMemory observation)]] of
  (base, bytes) : _ -> Just (B.index bytes (fromIntegral (raw - base)))
  [] -> Nothing

observedWrites :: NativeObservation -> Maybe [(Int, GuestAddress, Word32)]
observedWrites observation = do
  events <- nativeKernel observation >>= observedOrder
  pure [ (index, GuestAddress (fromIntegral raw), fromIntegral value :: Word32) | (index, OrderedWrite (Write (N.GuestAddress raw) _) (WriteValue value)) <- zip [0 :: Int ..] events ]

writeValues :: NativeObservation -> GuestAddress -> [Word32]
writeValues observation address = [value | Just writes <- [observedWrites observation], (_, actual, value) <- writes, actual == address]

writeIndices :: NativeObservation -> GuestAddress -> [(Int, Word32)]
writeIndices observation address = [(index, value) | Just writes <- [observedWrites observation], (index, actual, value) <- writes, actual == address]

firstWriteIndex :: NativeObservation -> GuestAddress -> Maybe Int
firstWriteIndex observation address = case writeIndices observation address of
  ((index, _) : _) -> Just index
  [] -> Nothing

hasBarrierBefore :: NativeObservation -> Barrier -> GuestAddress -> Bool
hasBarrierBefore observation barrier address = case (nativeKernel observation >>= observedOrder, firstWriteIndex observation address) of
  (Just events, Just index) -> any (\(position, event) -> position < index && event == OrderedBarrier barrier) (zip [0 :: Int ..] events)
  _ -> False

lockReacquired :: NativeObservation -> GuestAddress -> Bool
lockReacquired observation address = case writeIndices observation address of
  ((_, 1) : rest) -> let (beforeRelease, _) = break ((== 0) . snd) rest in any ((== 1) . snd) beforeRelease
  _ -> False

checkExpectation :: Facts -> Case -> NativeObservation -> Expect -> [String]
checkExpectation facts one observation expectation = case expectation of
  Equals address value -> case readWord facts one observation address of
    Just actual | actual == value -> []
    _ -> [missingEffect "missing effect " address 4 value]
  ByteEquals address value -> case readByte facts one observation address of
    Just actual | actual == value -> []
    _ -> [missingEffect "missing effect byte " address 1 value]
  Result value -> ["result differs: expected " ++ show value | N.lookupRegister R0 observation /= Just value]
  RegisterEquals register value -> ["register " ++ show register ++ " differs: expected " ++ show value | N.lookupRegister register observation /= Just value]
  ControlEquals value -> ["control differs: expected " ++ show value | fmap (.&. 0x1ff) (N.lookupRegister Cpsr observation) /= Just value]
  OrderBefore first second -> case (firstWriteIndex observation first, firstWriteIndex observation second) of
    (Just before, Just after) | before < after -> []
    _ -> ["order " ++ show first ++ " before " ++ show second ++ " is missing"]
  OrderBarrierBefore barrier address -> ["barrier " ++ show barrier ++ " before " ++ show address ++ " is missing" | not (hasBarrierBefore observation barrier address)]
  NoReacquire address -> ["lock reacquired at " ++ show address | lockReacquired observation address]
  TimerLog (TimerLogExpect count entries) ->
    let cursor = fromMaybe 0 (readWord facts one observation (GuestAddress timerLogBase))
        actual = [(fromMaybe 0 (readWord facts one observation (GuestAddress (timerLogBase + 4 + fromIntegral index * 8))), fromMaybe 0 (readWord facts one observation (GuestAddress (timerLogBase + 8 + fromIntegral index * 8)))) | index <- [0 .. count - 1]]
     in ["timer log differs: cursor " ++ show cursor ++ ", entries " ++ show actual | fromIntegral cursor /= count || actual /= entries]
  where
    missingEffect label address width value =
      case [ident | page@(Page ident _ _ permission _) <- pages facts one, contains address width page, writable permission, Nothing <- [lookup ident (nativeMemory observation)]] of
        ident : _ -> "missing mutable capture " ++ show ident ++ " at " ++ show address ++ " = " ++ show value
        [] -> label ++ show address ++ " = " ++ show value
    writable ReadWrite = True
    writable Device = True
    writable _ = False

effectiveExpectations :: Case -> [Expect]
effectiveExpectations one
  | caseSkipTimerLog one = caseExpect one
  | any isTimer (caseExpect one) = caseExpect one
  | otherwise = caseExpect one ++ [timerLog 0 []]
  where
    isTimer (TimerLog _) = True
    isTimer _ = False

effectiveStockExpectations :: Case -> [Expect]
effectiveStockExpectations one
  | caseSkipTimerLog one = caseStockExpectations one
  | any isTimer (caseStockExpectations one) = caseStockExpectations one
  | otherwise = caseStockExpectations one ++ [timerLog 0 []]
  where
    isTimer (TimerLog _) = True
    isTimer _ = False

roleExpectations :: Role -> Case -> [Expect]
roleExpectations StockRole one = effectiveStockExpectations one
roleExpectations OwnedRole one = effectiveExpectations one

registerChecks :: Role -> Domain -> Facts -> Case -> NativeObservation -> [String]
registerChecks role executing _facts one observation
  | isBudgetRole role one = callerFrameChecks
  | otherwise = callerFrameChecks ++ controlChecks ++ boundaryChecks
  where
    callerFrameChecks = ["caller frame changed" | stackBytes <- maybeToList (lookup (RegionId "stack") (nativeMemory observation)), B.take (B.length callerBytes) (B.drop 0x400 stackBytes) /= callerBytes]
    expectedControl = if caseBoundary one == TaskArrival then 0x93 else caseCpsr one .&. 0x1ff
    controlChecks = ["CPSR control changed" | fmap (.&. 0x1ff) (N.lookupRegister Cpsr observation) /= Just expectedControl]
    boundaryChecks = case caseBoundary one of
      CallerReturn ->
        ["caller register " ++ show register ++ " changed" | (register, value) <- [(r, seed r) | r <- [R4 .. R11]] ++ [(Sp, callerSp one)], N.lookupRegister register observation /= Just value]
      TaskArrival ->
        ["selected-task register " ++ show register ++ " differs" | (register, value) <- [(Sp, targetStackPointer), (Pc, stopAddress)], N.lookupRegister register observation /= Just value]
          ++ ["selected thread pointer differs" | (nativeKernel observation >>= observedTpidrurw) /= Just selectedThread]
          ++ ["selected-task FPEXC differs" | (nativeKernel observation >>= observedFpexc) /= Just 0]
    selectedThread = case caseWaiters one of
      (pw : _) -> threadValue (pwDomain pw) (pwSlot pw)
      [] -> threadValue executing waiterSlot

continuationChecks :: Domain -> Facts -> Case -> NativeObservation -> [String]
continuationChecks executing facts one observation
  | caseBoundary one /= TaskArrival = []
  | otherwise = case readWord facts one observation (GuestAddress savedSpAddress) of
      Nothing -> ["missing suspended caller SP"]
      Just saved ->
        ["suspended caller frame is outside its live stack" | saved < callerSp one - 0x400 || toInteger saved + 36 > toInteger (callerSp one) || saved .&. 3 /= 0]
          ++ ["suspended caller resume entry differs" | readWord facts one observation (GuestAddress savedPcAddress) /= Just (resumeEntry executing)]
          ++ ["suspended continuation is not in the owned code" | maybe True (not . inOwnedCode) (readWord facts one observation (GuestAddress (saved + 32)))]
  where
    savedSpAddress = taskAt facts executing runningSlot + factsTaskSavedSp facts
    savedPcAddress = taskAt facts executing runningSlot + factsTaskResumePc facts
    inOwnedCode address = any (\(base, size) -> base <= address && address < base + size) ownedExtents
    ownedExtents = extents executing ++ (if isJust (caseSchedulerLock one) then s3bPlacement executing else [])

data LockFence = AwaitAcquireFence | AwaitReleaseFence | PublishedLock | InvalidLockOrder deriving Eq

lockInterval :: NativeObservation -> GuestAddress -> Maybe (Int, Int)
lockInterval observation address = do
  acquired <- fst <$> find ((== 1) . snd) (writeIndices observation address)
  released <- fst <$> find ((== 0) . snd) (writeIndices observation address)
  let finalFence = foldl' (\state (index, event) -> if acquired < index && index < released then advanceFence state event else state) AwaitAcquireFence (zip [0 :: Int ..] events)
  if acquired < released && finalFence == PublishedLock then Just (acquired, released) else Nothing
  where
    events = fromMaybe [] (nativeKernel observation >>= observedOrder)

advanceFence :: LockFence -> Ordered -> LockFence
advanceFence state event = case event of
  OrderedBarrier Dmb -> case state of
    AwaitAcquireFence -> AwaitReleaseFence
    AwaitReleaseFence -> PublishedLock
    _ -> state
  OrderedWrite write _ -> if privateWrite write then state else case state of
    AwaitAcquireFence -> InvalidLockOrder
    InvalidLockOrder -> InvalidLockOrder
    _ -> AwaitReleaseFence
  _ -> state

privateWrite :: Write -> Bool
privateWrite (Write (N.GuestAddress address) (N.WriteBytes width)) =
  any (\(lo, hi) -> lo <= toInteger address && toInteger address + toInteger width <= hi) privateRegions
  where
    privateRegions :: [(Integer, Integer)]
    privateRegions =
      [ (toInteger base, toInteger base + toInteger spanBytes)
      | (base, spanBytes) <- [(stackPointer - 0x400, 0x400), (handlerStackPointer - 0x400, 0x400), (targetStackPointer - 0x100, 0x100), (wakerStack - 0x400, 0x400), (chainStack - 0x400, 0x400)] :: [(Word32, Word32)]
      ]

effectChecks :: Facts -> Role -> Case -> NativeObservation -> [String]
effectChecks facts role one observation = lockChecks ++ notificationChecks
  where
    expectations = roleExpectations role one
    resultValue = roleResult role one
    refusal = resultValue `elem` [0xffffffe7, 0xffffffee, 0xffffffef, 0xffffffd6]
    takesLock =
      caseSlot one `notElem` [FlagCreate, MailboxCreate]
        && not refusal
        && (role /= StockRole || caseSlot one `notElem` [FlagDelete, MailboxDelete])
    multiStep = isJust (caseWaker one) || not (null (caseSteps one))
    objectLock = GuestAddress (dfObjectLock (domainFacts facts (caseObject one)) + factsLockWord facts)
    lockChecks
      | not takesLock = []
      | multiStep = []
      | Just lockDomain <- caseSchedulerLock one =
          case lockInterval observation (GuestAddress (dfSchedulerLock (domainFacts facts lockDomain))) of
            Nothing -> ["scheduler lock or its release barrier differs"]
            Just _ -> []
      | otherwise = case lockInterval observation objectLock of
          Nothing -> ["object lock or its release barrier differs"]
          Just (objectAcquire, objectRelease) ->
            concat
              [ case lockInterval observation (GuestAddress (dfSchedulerLock (domainFacts facts domain))) of
                  Nothing -> []
                  Just (schedulerAcquire, schedulerRelease)
                    | objectAcquire < schedulerAcquire && schedulerRelease < objectRelease -> []
                    | otherwise -> ["waiter scheduler lock is not nested inside the object lock"]
              | domain <- [DomainA, DomainB]
              ]
    expectedBank domain = any (isBank domain) expectations
    isBank domain (Equals (GuestAddress address) 0x01000000) = address == pointerWord (bank domain)
    isBank _ _ = False
    events = fromMaybe [] (nativeKernel observation >>= observedOrder)
    notificationChecks
      | caseDifference one == HandlerWaiterDomain && role == StockRole = []
      | otherwise = concat
      [ let values = writeValues observation (bank domain)
         in if expectedBank domain
              then
                ["notification bank " ++ show domain ++ " differs" | values /= [0x01000000]]
                  ++ case lockInterval observation (GuestAddress (dfSchedulerLock (domainFacts facts domain))) of
                    Nothing -> ["notification has no waiter scheduler lock"]
                    Just (acquired, released) ->
                      let writeIndex = firstWriteIndex observation (bank domain)
                          inside = maybe False (\index -> acquired < index && index < released) writeIndex
                          afterDsb = maybe False (\index -> any (\(after, event) -> index < after && event == OrderedBarrier Dsb) (zip [0 :: Int ..] events)) writeIndex
                          beforeDsb = maybe False (\index -> any (\(before, event) -> before < index && event == OrderedBarrier Dsb) (zip [0 :: Int ..] events)) writeIndex
                       in ["notification is outside the waiter scheduler lock" | not inside]
                            ++ ["notification has no completion DSB" | not afterDsb]
                            ++ ["owned notification has no preceding DSB" | role == OwnedRole && not beforeDsb]
              else ["unexpected notification bank " ++ show domain | not (null values)]
      | domain <- [DomainA, DomainB]
      ]

data FamilyDifference
  = RoleDifference Role String
  | MissingRegion RegionId
  | StateDifference RegionId
  deriving (Eq, Show)

data ScopeControl = BudgetStockState | UnusedSavedSp | AdjacentStateByte deriving (Eq, Show)

scopeControlName :: ScopeControl -> String
scopeControlName BudgetStockState = "must-fail-budget-stock-state"
scopeControlName UnusedSavedSp = "must-fail-unused-task-stack"
scopeControlName AdjacentStateByte = "must-fail-adjacent-state-byte"

renderDifference :: FamilyDifference -> String
renderDifference difference = case difference of
  RoleDifference OwnedRole message -> "owned: " ++ message
  RoleDifference StockRole message -> "stock: " ++ message
  MissingRegion ident -> "missing state region " ++ show ident
  StateDifference ident -> "state region " ++ show ident ++ " differs"

compareFamily :: Role -> Domain -> Facts -> Case -> NativeObservation -> NativeObservation -> [FamilyDifference]
compareFamily _role executing facts one stock owned =
  map (RoleDifference OwnedRole) ownedChecks ++ map (RoleDifference StockRole) stockChecks ++ memoryDifferences
  where
    ownedChecks = roleChecks OwnedRole owned
    stockChecks = roleChecks StockRole stock
    roleChecks currentRole observation =
      exitChecks currentRole observation
        ++ concatMap (checkExpectation facts one observation) expectations
        ++ registerChecks currentRole executing facts one observation
        ++ (if incomplete then [] else continuationChecks executing facts one observation)
        ++ effectChecks facts currentRole one observation
      where
        incomplete = isBudgetRole currentRole one
        expectations = filter (\expectation -> not incomplete || atIncompleteStop expectation) (roleExpectations currentRole one)
    atIncompleteStop expectation = case expectation of
      Result _ -> False
      RegisterEquals _ _ -> False
      ControlEquals _ -> False
      _ -> True
    exitChecks currentRole observation =
      let wanted = roleStatus currentRole one
          actual = nativeStatus observation
       in ["wrong exit " ++ show actual ++ ", expected " ++ show wanted | actual /= wanted]
            ++ ["stop address differs" | wanted == StopAddress, N.lookupRegister Pc observation /= Just stopAddress]
    memoryDifferences = compareSelected owned stock
    compareSelected reference subject =
      [MissingRegion ident | ident <- compared, M.notMember ident (memory reference) || M.notMember ident (memory subject)]
        ++ [StateDifference ident | ident <- compared, M.lookup ident (memory reference) /= M.lookup ident (memory subject)]
    compared =
      [ident | Page ident _ _ _ _ <- pages facts one, ident `notElem` map RegionId ["stack", "target-stack", "waker-stack", "chain-stack", "actor-stack"]]
    memory observation = M.fromList [(ident, normalize ident bytes) | (ident, bytes) <- nativeMemory observation, ident `elem` compared]
    declaredBytes role = M.fromList (concatMap expectationBytes (roleExpectations role one))
    expectationBytes expectation = case expectation of
      Equals (GuestAddress address) value ->
        [(GuestAddress (address + fromIntegral offset), fromIntegral (value `shiftR` (offset * 8))) | offset <- [0 .. 3]]
      ByteEquals address value -> [(address, value)]
      TimerLog (TimerLogExpect count entries) ->
        expectationBytes (Equals (GuestAddress timerLogBase) (fromIntegral count))
          ++ concat
            [ expectationBytes (Equals (GuestAddress (timerLogBase + 4 + index * 8)) entry)
                ++ expectationBytes (Equals (GuestAddress (timerLogBase + 8 + index * 8)) argument)
            | (index, (entry, argument)) <- zip [0 :: Word32 ..] entries
            ]
      _ -> []
    ownedDeclared = declaredBytes OwnedRole
    stockDeclared = declaredBytes StockRole
    logExpectations role = [(count, entries) | TimerLog (TimerLogExpect count entries) <- roleExpectations role one]
    timerPayloadBytes = case (logExpectations OwnedRole, logExpectations StockRole) of
      ([ownedLog@(_, ownedEntries)], [stockLog@(_, stockEntries)])
        | ownedLog /= stockLog ->
            [GuestAddress (timerLogBase + 4 + offset) | offset <- take (8 * max (length ownedEntries) (length stockEntries)) [0 ..]]
      _ -> []
    deltaBytes =
      [ address
      | (address, ownedByte) <- M.toList ownedDeclared
      , Just stockByte <- [M.lookup address stockDeclared]
      , ownedByte /= stockByte
      ] ++ timerPayloadBytes
    kernelRegion = RegionId (if executing == DomainA then "kernel-a" else "kernel-b")
    privateWords ident
      | ident /= kernelRegion = []
      | caseBoundary one == TaskArrival = [taskAt facts executing runningSlot + factsTaskSavedSp facts]
      | isJust (caseSelectedWait one) =
          [taskAt facts executing slot + factsTaskSavedSp facts | slot <- [runningSlot, waiterSlot]]
      | isJust (caseWaker one) =
          [taskAt facts executing slot + factsTaskSavedSp facts | slot <- runningSlot : wakerSlot : [pwSlot waiter | waiter <- caseWaiters one, pwDomain waiter == executing, isJust (pwChain waiter)]]
      | otherwise = []
    normalize ident bytes = case [base | Page current (GuestAddress base) _ _ _ <- pages facts one, current == ident] of
      base : _ ->
        let addresses = deltaBytes ++ [GuestAddress (address + offset) | address <- privateWords ident, offset <- [0 .. 3]]
            mask = M.fromList [(address, ()) | address@(GuestAddress value) <- addresses, value >= base, value - base < fromIntegral (B.length bytes)]
         in if M.null mask then bytes else snd (B.mapAccumL (\address octet -> (address + 1, if M.member (GuestAddress address) mask then 0 else octet)) base bytes)
      [] -> bytes

controls :: Domain -> Facts -> Case -> NativeObservation -> NativeObservation -> [(String, Bool)]
controls executing facts one stock owned =
  [ ("must-pass", null (compareFamily OwnedRole executing facts one stock owned))
  , ("must-fail-behavior", not (holdsObservation owned (Result (roleResult OwnedRole one `xor` 1))))
  , ("must-fail-effect", not (holdsObservation owned wrongEffect))
  , ("must-fail-lifetime", not (holdsObservation owned (RegisterEquals R4 (seed R4 + 1))))
  ]
    ++ ( if caseDifference one == NoDifference
           then []
           else case caseStockStatus one of
             Just InstructionBudget ->
               [ ("must-fail-stock-difference", nativeStatus stock == InstructionBudget && nativeStatus owned /= InstructionBudget)
               , ("must-fail-owned-difference", not (null (stockAgainstOwnedExpectations stock)))
               ]
             _ ->
               [ ("must-fail-stock-difference", not (null (ownedAgainstStockExpectations owned)))
               , ("must-fail-owned-difference", not (null (stockAgainstOwnedExpectations stock)))
               ]
       )
    ++ [ ("must-fail-unrelated-kernel-word", any (\difference -> case difference of StateDifference ident -> ident == RegionId region; _ -> False) (compareFamily OwnedRole executing facts one stock (perturbKernel owned)))
       | caseDifference one /= NoDifference
       ]
    ++ [ (scopeControlName BudgetStockState, any stockStateRejected (compareFamily OwnedRole executing facts wrongStock stock owned))
       | isBudgetRole StockRole one
       , (address, value) : _ <- [[(a, v) | Equals a v <- caseStockExpectations one]]
       , let wrongStock = one {caseStockExpect = Just (Equals address (value `xor` 0x80000000) : filter (not . sameAddress address) (caseStockExpectations one))}
       ]
    ++ [ (scopeControlName UnusedSavedSp, kernelStateRejected (compareFamily OwnedRole executing facts one stock (perturbByte (GuestAddress (taskAt facts executing (factsTaskSlots facts) + factsTaskSavedSp facts)) owned)))
       | isJust (caseWaker one)
       ]
    ++ [ (scopeControlName AdjacentStateByte, kernelStateRejected (compareFamily OwnedRole executing facts one stock (perturbByte (GuestAddress (taskAt facts executing (if isJust (caseWaker one) then runningSlot else waiterSlot) + factsTaskState facts + 1)) owned)))
       | caseDifference one /= NoDifference
       ]
  where
    stockStateRejected (RoleDifference StockRole _) = True
    stockStateRejected _ = False
    sameAddress address (Equals actual _) = actual == address
    sameAddress _ _ = False
    kernelStateRejected = any (\difference -> case difference of StateDifference ident -> ident == RegionId region; _ -> False)
    perturbByte (GuestAddress address) observation =
      observation {nativeMemory =
        [ (ident, case [base | Page current (GuestAddress base) size _ _ <- pages facts one, current == ident, address >= base, address - base < fromIntegral size] of
            base : _ -> let offset = fromIntegral (address - base) in B.take offset bytes <> B.singleton (B.index bytes offset `xor` 1) <> B.drop (offset + 1) bytes
            [] -> bytes)
        | (ident, bytes) <- nativeMemory observation
        ]}
    region = if executing == DomainA then "kernel-a" else "kernel-b"
    perturbKernel observation =
      observation
        { nativeMemory =
            [ (ident, if ident == RegionId region then flipWord 0x20 bytes else bytes)
            | (ident, bytes) <- nativeMemory observation
            ]
        }
    flipWord offset bytes = B.take offset bytes <> word32le (readWordAt bytes offset `xor` 1) <> B.drop (offset + 4) bytes
    wrongEffect = case caseExpect one of
      (Equals address value : _) -> Equals address (value + 1)
      (ByteEquals address value : _) -> ByteEquals address (value + 1)
      _ -> Equals (GuestAddress (callerSp one)) 0
    holdsObservation observation expectation = null (checkExpectation facts one observation expectation)
    ownedAgainstStockExpectations observation = concatMap (checkExpectation facts one observation) (effectiveStockExpectations one)
    stockAgainstOwnedExpectations observation = concatMap (checkExpectation facts one observation) (effectiveExpectations one)

runFamily :: FilePath -> FilePath -> FilePath -> Facts -> Entries -> Domain -> Image -> Image -> IO FamilyRun
runFamily runner cacheRoot directory facts entries executing stock owned = do
  let selected = cases facts entries executing ++ retainedCases facts executing
      specs role image = [(one, caseFor role facts entries executing stock image one) | one <- selected]
      stockCases = specs StockRole stock
      ownedCases = specs OwnedRole owned
  stockBatch <- writeBatch (directory </> "stock") (map snd stockCases)
  ownedBatch <- writeBatch (directory </> "owned") (map snd ownedCases)
  (stockReuse, stockRaw) <- cachedObservedBatch cacheRoot runner stockBatch
  (ownedReuse, ownedRaw) <- cachedObservedBatch cacheRoot runner ownedBatch
  let reference = M.fromList [(caseIdText ident, observation) | (ident, observation) <- stockRaw]
      subject = M.fromList [(caseIdText ident, observation) | (ident, observation) <- ownedRaw]
  results <- forM stockCases $ \(one, _) -> do
    let ident = emittedId executing one
    pure (caseIdText (caseName one), case (M.lookup ident reference, M.lookup ident subject) of
      (Just original, Just replacement) ->
        let differences = map renderDifference (compareFamily OwnedRole executing facts one original replacement)
            caseControls = controls executing facts one original replacement
         in Right (differences ++ ["control " ++ name ++ " not detected" | (name, False) <- caseControls])
      (Nothing, _) -> Left "stock case did not run"
      (_, Nothing) -> Left "owned case did not run")
  let allControls = concat
        [ [(caseIdText (caseName one) ++ "/" ++ domainLabel executing ++ "/" ++ name, ok) | (name, ok) <- controls executing facts one original replacement]
        | (one, _) <- stockCases
        , Just original <- [M.lookup (emittedId executing one) reference]
        , Just replacement <- [M.lookup (emittedId executing one) subject]
        ]
      tagged = [(caseIdText (caseName one), show (caseDifference one)) | one <- selected, caseDifference one /= NoDifference]
  unless (length stockRaw == length selected && length ownedRaw == length selected) (fail "native observations do not cover the current family case set")
  pure (FamilyRun executing results (length [() | (_, Right []) <- results]) (stockReuse, ownedReuse) allControls tagged)

-- | The S3b task entry's definition-existence word, read at @definition + 0x24@.
definitionExistsOffset :: Word32
definitionExistsOffset = 0x24

joinedPriority :: Facts -> TaskSlot -> Word32
joinedPriority facts slot
  | slot == runningSlot = 10
  | slot == idleSlot = 15
  | slot == waiterSlot = 3
  | otherwise = slot `mod` factsPriorities facts

joinedHandle :: Domain -> Domain -> TaskSlot -> Word32
joinedHandle executing target slot
  | target == executing = slot
  | otherwise = (domainId target `shiftL` 16) .|. slot

-- | One qualified S3b caller over a sleeping task. The task's wait block carries the timer entry
-- while waiting; the release stores zero there. A changed selection notifies, and the task entry
-- dispatches while the interrupt entry defers through the reschedule word.
joinedCase :: Facts -> Domain -> String -> Domain -> TaskSlot -> S3bService -> Bool -> Boundary -> Case
joinedCase facts executing name target slot service timed boundary = (baseCase executing)
  { caseName = CaseId name
  , caseSlot = FlagWait
  , caseEntryAddress = Just (s3bEntry executing service)
  , caseObject = executing
  , caseInvocation = if service == S3bInterruptWake then HandlerCallback else TaskInvocation
  , caseHandler = if service == S3bInterruptWake then 1 else 0
  , caseBoundary = boundary
  , caseSchedulerLock = Just target
  , caseArgs = []
  , caseDirectInitial = Just directRegisters
  , caseWaiters = []
  , caseWords = caseWords'
  , caseExpect = expectations
  , caseLimit = InstructionLimit 50000
  }
  where
    priority = joinedPriority facts slot
    changed = priority < joinedPriority facts runningSlot
    remote = target /= executing
    handler = service == S3bInterruptWake
    targetTask = taskAt facts target slot
    wait = flagWait facts target slot
    timer = objectTimer facts FlagWait target slot
    definition = definitionAddress facts target slot
    directRegisters =
      [(R0, joinedHandle executing target slot)]
        ++ [(register, seed register) | register <- [R1 .. R12]]
        ++ [ (Sp, if handler then handlerStackPointer else stackPointer)
           , (Lr, exitAddress)
           , (Cpsr, 0x13)
           ]
    caseWords' =
      [ poke (targetTask + factsTaskState facts) (packedState (factsStateWait facts .|. factsStateSleeping facts) priority)
      , poke (targetTask + factsTaskWait facts) wait
      , poke (wait + factsWaitBlockResult facts) (if timed then timer else 0)
      , poke (definition + definitionExistsOffset) 1
      ]
        ++ ( if timed
               then [ poke (timer + 4) (seam target "stock-timeout-callback")
                    , poke (timer + 8) targetTask
                    ]
               else []
           )
    expectations =
      (if boundary == TaskArrival then [] else [res 0])
        ++ [ eq (wait + factsWaitBlockResult facts) 0
           , byte (targetTask + factsTaskState facts) (factsStateReady facts)
           , eq (readyHead facts target priority) targetTask
           , eq (readyHead facts target priority + factsHeadPrev facts) targetTask
           , eq (targetTask + factsTaskNext facts) (readyHead facts target priority)
           , eq (targetTask + factsTaskPrev facts) (readyHead facts target priority)
           , eq (dfReadyBitmap (domainFacts facts target)) expectedBitmap
           , eq (dfRunning (domainFacts facts target)) expectedRunning
           , eq (dfSelected (domainFacts facts target)) expectedSelected
           , eq (pointerWord (bank target)) (if remote then 0x01000000 else 0)
           , eq (pointerWord (bank executing)) 0
           , eq (dfReschedule (domainFacts facts executing)) (if handler && not remote then 1 else 0)
           , timerLog (if timed then 1 else 0) (if timed then [(recordAddress facts executing target, timer)] else [])
           ]
        ++ [control 0x13 | handler]
    expectedBitmap = (1 `shiftL` 10) .|. (1 `shiftL` 15) .|. (1 `shiftL` fromIntegral priority)
    expectedRunning = if boundary == TaskArrival then targetTask else taskAt facts target runningSlot
    expectedSelected = if changed then targetTask else taskAt facts target runningSlot

joinedReturnCases :: Facts -> Entries -> Domain -> [Case]
joinedReturnCases facts entries executing =
  [ joinedReturnCase "s3b-wake-local-timed-return" True
  , joinedReturnCase "s3b-wake-local-untimed-return" False
  ]
  where
    joinedReturnCase name timed =
      (joinedCase facts executing name executing waiterSlot S3bWake timed CallerReturn)
        { caseSelectedWait = Just (entryAddress entries executing FlagWait)
        , caseFlags = [(executing, 1, 0, 0, 0)]
        , caseExpect = returnExpectations timed
        }
    targetTask = taskAt facts executing waiterSlot
    wait = flagWait facts executing waiterSlot
    timer = objectTimer facts FlagWait executing waiterSlot
    flagControlAddress = flagControl facts executing 1
    returnExpectations timed =
      [ res 0
      , byte (targetTask + factsTaskState facts) (factsStateWait facts .|. factsStateObjectQueued facts .|. factsStateOwnerLockQueued facts)
      , eq (wait + factsWaitBlockResult facts) 0
      , eq (flagControlAddress + factsWaitQueueNext facts) targetTask
      , eq (flagControlAddress + factsWaitQueuePrev facts) targetTask
      , eq (targetTask + factsTaskNext facts) flagControlAddress
      , eq (targetTask + factsTaskPrev facts) flagControlAddress
      , eq (dfSelected (domainFacts facts executing)) (taskAt facts executing runningSlot)
      , eq (dfRunning (domainFacts facts executing)) (taskAt facts executing runningSlot)
      , eq (dfReadyBitmap (domainFacts facts executing)) ((1 `shiftL` 10) .|. (1 `shiftL` 15))
      , eq (pointerWord (bank executing)) 0
      , eq (dfReschedule (domainFacts facts executing)) 0
      , timerLog (if timed then 1 else 0) (if timed then [(recordAddress facts executing executing, timer)] else [])
      ]

joinedCases :: Facts -> Domain -> [Case]
joinedCases facts executing =
  [ joinedCase facts executing "s3b-wake-local-timed" executing waiterSlot S3bWake True TaskArrival
  , joinedCase facts executing "s3b-wake-local-untimed" executing waiterSlot S3bWake False TaskArrival
  , joinedCase facts executing "s3b-wake-unchanged-timed" executing 11 S3bWake True CallerReturn
  , joinedCase facts executing "s3b-wake-unchanged-untimed" executing 11 S3bWake False CallerReturn
  , joinedCase facts executing "s3b-wake-remote-timed" (peer executing) waiterSlot S3bWake True CallerReturn
  , joinedCase facts executing "s3b-wake-remote-untimed" (peer executing) waiterSlot S3bWake False CallerReturn
  , joinedCase facts executing "s3b-interrupt-wake-local-timed" executing waiterSlot S3bInterruptWake True CallerReturn
  , joinedCase facts executing "s3b-interrupt-wake-local-untimed" executing waiterSlot S3bInterruptWake False CallerReturn
  , joinedCase facts executing "s3b-interrupt-wake-remote-timed" (peer executing) waiterSlot S3bInterruptWake True CallerReturn
  , joinedCase facts executing "s3b-interrupt-wake-remote-untimed" (peer executing) waiterSlot S3bInterruptWake False CallerReturn
  ]

replaceImage :: Word32 -> B.ByteString -> Image -> Image
replaceImage address bytes image =
  let offset = fromIntegral (address - imageBase image)
   in image { imageBytes = B.take offset (imageBytes image) <> bytes <> B.drop (offset + B.length bytes) (imageBytes image) }

-- | One joined cohort's rows, reuse and counterexamples.
data CohortRun = CohortRun
  { cohortRows :: [(String, Either String [String])]
  , cohortReuse :: (BatchReuse, BatchReuse, BatchReuse)
  , cohortFalseReuse :: Maybe BatchReuse
  , cohortControls :: [(String, Bool)]
  , cohortCounterexamples :: [(String, String)]
  , cohortFalseCounterexamples :: [(String, String)]
  }

firstOwnedDifference :: Domain -> Facts -> Case -> NativeObservation -> NativeObservation -> Maybe String
firstOwnedDifference executing facts one original subject =
  case filter ownedFailure (compareFamily OwnedRole executing facts one original subject) of
    difference : _ -> Just (renderDifference difference)
    [] -> Nothing
  where
    ownedFailure difference = case difference of RoleDifference OwnedRole _ -> True; _ -> False

runCohort :: FilePath -> FilePath -> FilePath -> Facts -> Entries -> Domain -> Image -> Image -> Image -> Bool -> [Case] -> IO CohortRun
runCohort runner cacheRoot directory facts entries executing stock owned s3b withFalse selected = do
  let extra = s3bPlacement executing
      stockCases = [(one, caseForCode StockRole facts entries executing stock (codeBytes executing stock stock) extra one) | one <- selected]
      ownedCases = [(one, caseForCode OwnedRole facts entries executing stock (codeBytesJoined executing stock owned s3b) extra one) | one <- selected]
      stubOwned = replaceImage (s3bRelease executing) (assemble [0xe3a00001, 0xe12fff1e]) owned
      stubCases = [(one, caseForCode OwnedRole facts entries executing stock (codeBytesJoined executing stock stubOwned s3b) extra one) | one <- selected]
  stockBatch <- writeBatch (directory </> "stock") (map snd stockCases)
  ownedBatch <- writeBatch (directory </> "owned") (map snd ownedCases)
  stubBatch <- writeBatch (directory </> "stub") (map snd stubCases)
  (stockReuse, stockRaw) <- cachedObservedBatch cacheRoot runner stockBatch
  (ownedReuse, ownedRaw) <- cachedObservedBatch cacheRoot runner ownedBatch
  (stubReuse, stubRaw) <- cachedObservedBatch cacheRoot runner stubBatch
  let reference = M.fromList [(caseIdText ident, observation) | (ident, observation) <- stockRaw]
      subject = M.fromList [(caseIdText ident, observation) | (ident, observation) <- ownedRaw]
      stub = M.fromList [(caseIdText ident, observation) | (ident, observation) <- stubRaw]
  results <- forM stockCases $ \(one, _) -> do
    let ident = emittedId executing one
    pure (caseIdText (caseName one), case (M.lookup ident reference, M.lookup ident subject, M.lookup ident stub) of
      (Just original, Just replacement, Just stubObservation) ->
        let differences = map renderDifference (compareFamily OwnedRole executing facts one original replacement)
            caseControls = controls executing facts one original replacement
            counterexample = case firstOwnedDifference executing facts one original stubObservation of
              Just _ -> []
              Nothing -> ["joined counterexample not rejected"]
         in Right (differences ++ ["control " ++ name ++ " not detected" | (name, False) <- caseControls] ++ counterexample)
      (Nothing, _, _) -> Left "stock joined case did not run"
      (_, Nothing, _) -> Left "owned joined case did not run"
      (_, _, Nothing) -> Left "stub joined case did not run")
  let allControls = concat
        [ [(caseIdText (caseName one) ++ "/" ++ domainLabel executing ++ "/" ++ name, ok) | (name, ok) <- controls executing facts one original replacement]
        | (one, _) <- stockCases
        , Just original <- [M.lookup (emittedId executing one) reference]
        , Just replacement <- [M.lookup (emittedId executing one) subject]
        ]
      counterexamples =
        [ (caseIdText (caseName one), difference)
        | (one, _) <- stockCases
        , Just original <- [M.lookup (emittedId executing one) reference]
        , Just stubObservation <- [M.lookup (emittedId executing one) stub]
        , Just difference <- [firstOwnedDifference executing facts one original stubObservation]
        ]
  unless (length stockRaw == length selected && length ownedRaw == length selected && length stubRaw == length selected) (fail "native observations do not cover the current joined case set")
  (falseReuse, falseCounterexamples) <-
    if withFalse
      then do
        let falseOwned = replaceImage (s3bRelease executing) (assemble [0xe3a00000, 0xe12fff1e]) owned
            falseCases = [(one, caseForCode OwnedRole facts entries executing stock (codeBytesJoined executing stock falseOwned s3b) extra one) | one <- selected]
        falseBatch <- writeBatch (directory </> "false") (map snd falseCases)
        (reuse, falseRaw) <- cachedObservedBatch cacheRoot runner falseBatch
        let falseMap = M.fromList [(caseIdText ident, observation) | (ident, observation) <- falseRaw]
            differences =
              [ (caseIdText (caseName one), difference)
              | (one, _) <- stockCases
              , Just original <- [M.lookup (emittedId executing one) reference]
              , Just falseObservation <- [M.lookup (emittedId executing one) falseMap]
              , Just difference <- [firstOwnedDifference executing facts one original falseObservation]
              ]
        unless (length falseRaw == length selected) (fail "false-stub observations do not cover the current joined case set")
        pure (Just reuse, differences)
      else pure (Nothing, [])
  pure (CohortRun results (stockReuse, ownedReuse, stubReuse) falseReuse allControls counterexamples falseCounterexamples)

-- | The qualified S3b callers over the staged family code. The original twenty rows keep their
-- batches. The return cohort adds a selected-task wait body that resumes the original caller, and
-- the false stub returns unchanged without releasing the task.
runJoined :: FilePath -> FilePath -> FilePath -> Facts -> Entries -> Domain -> Image -> Image -> Image -> IO JoinedRun
runJoined runner cacheRoot directory facts entries executing stock owned s3b = do
  original <- runCohort runner cacheRoot directory facts entries executing stock owned s3b True (joinedCases facts executing)
  returns <- runCohort runner cacheRoot (directory </> "return") facts entries executing stock owned s3b False (joinedReturnCases facts entries executing)
  pure (JoinedRun executing
    (cohortRows original ++ cohortRows returns)
    (cohortReuse original)
    (cohortReuse returns)
    (fromMaybe BatchReused (cohortFalseReuse original))
    (cohortControls original ++ cohortControls returns)
    (cohortCounterexamples original ++ cohortCounterexamples returns)
    (cohortFalseCounterexamples original))

cases :: Facts -> Entries -> Domain -> [Case]
cases facts _entries executing =
  [ -- Admission and refusal precedence.
    (mk "flag-set-handler-context" FlagSet) { caseHandler = 1, caseExpect = [res (0xffffffe7)] }
  , (mk "flag-set-irq-masked" FlagSet) { caseCpsr = 0x93, caseExpect = [res (0xffffffe7)] }
  , (mk "flag-set-invalid-class" FlagSet) { caseArgs = [(R0, 0x00030001), (R1, 1)], caseExpect = [res (0xffffffee)] }
  , (mk "flag-set-not-created" FlagSet) { caseArgs = [(R0, id0 1), (R1, 1)], caseExpect = [res (0xffffffd6)] }
  , (mk "flag-wait-zero-pattern" FlagWait) { caseArgs = [(R0, id0 1), (R1, 0), (R2, 0), (R3, outWord)], caseFlags = [flag 1 0 0 0], caseExpect = [res (0xffffffef)] }
  , (mk "flag-wait-bad-mode" FlagWait) { caseArgs = [(R0, id0 1), (R1, 1), (R2, 2), (R3, outWord)], caseFlags = [flag 1 0 0 0], caseExpect = [res (0xffffffef)] }
  , (mk "flag-wait-not-created-before-overflow" FlagWait) { caseArgs = [(R0, id0 1), (R1, 1), (R2, 0), (R3, outWord)], caseWaiters = [waiterWith 14 1 0], caseExpect = [res (0xffffffd6)] }
  , (mk "flag-timed-wait-bad-timeout" FlagTimedWait) { caseArgs = [(R0, id0 1), (R1, 1), (R2, 0), (R3, outWord), (R4, 0xfffffffe)], caseFlags = [flag 1 0 0 0], caseExpect = [res (0xffffffef)] }
  , (mk "flag-timed-wait-dispatch-disabled" FlagTimedWait) { caseArgs = [(R0, id0 1), (R1, 1), (R2, 0), (R3, outWord), (R4, 1)], caseFlags = [flag 1 0 0 0], caseDispatch = 0, caseExpect = [res (0xffffffe7)] }
  , (mk "flag-set-handler-task-context" FlagSetHandler) { caseExpect = [res (0xffffffe7)] }
  , (mk "flag-set-handler-irq-masked" FlagSetHandler) { caseHandler = 1, caseCpsr = 0x93, caseExpect = [res (0xffffffe7)] }
  , (mk "mailbox-send-not-created" MailboxSend) { caseArgs = [(R0, id0 1), (R1, messageAddress 0)], caseExpect = [res (0xffffffd6)] }
  , (mk "mailbox-send-not-created-priority" MailboxSend) { caseArgs = [(R0, id0 1), (R1, messageAddress 0)], caseWords = [poke (mailboxConfig facts executing 1 + factsMailboxConfigAttributes facts) 2], caseExpect = [res (0xffffffd6)] }
  , (mk "mailbox-send-not-created-unreadable" MailboxSend) { caseArgs = [(R0, id0 1), (R1, unreadable)], caseWords = [poke (mailboxConfig facts executing 1 + factsMailboxConfigAttributes facts) 2], caseExpect = [res (0xffffffd6)] }
  , (mk "mailbox-receive-bad-timeout" MailboxTimedReceive) { caseArgs = [(R0, id0 1), (R1, outWord), (R2, 0xfffffffe)], caseExpect = [res (0xffffffef)] }
  , (mk "mailbox-receive-dispatch-disabled" MailboxReceive) { caseArgs = [(R0, id0 1), (R1, outWord)], caseMailboxes = [mailbox 1 0 32 0 0], caseDispatch = 0, caseExpect = [res (0xffffffe7)] }
  , (mk "flag-create-handler-context" FlagCreate) { caseArgs = [(R0, id0 2), (R1, requestAddress)], caseHandler = 1, caseCpsr = 0x93, caseWords = [poke requestAddress 0, poke (requestAddress + 4) 1], caseExpect = [res 0, eq (flagConfig facts executing 2 + factsFlagConfigExists facts) 1] }
  , (mk "flag-create-already-exists-unreadable" FlagCreate) { caseArgs = [(R0, id0 1), (R1, unreadable)], caseFlags = [flag 1 0 0 0], caseExpect = [res (0xffffffd7), eq (flagConfig facts executing 1 + factsFlagConfigAttributes facts) 0] }
  , (mk "mailbox-create-already-exists-unreadable" MailboxCreate) { caseArgs = [(R0, id0 1), (R1, unreadable)], caseMailboxes = [mailbox 1 0 32 0 0], caseExpect = [res (0xffffffd7), eq (mailboxConfig facts executing 1 + factsMailboxConfigAttributes facts) 0] }
  , (mk "unequal-flags-b-out-of-range" FlagSet) { caseArgs = [(R0, idB 120), (R1, 1)], caseFlagCounts = (120, 119), caseExpect = [res (0xffffffee)] }
  , (mk "unequal-flags-a-valid-b-refused" FlagSet) { caseArgs = [(R0, idA 120), (R1, 1)], caseFlagCounts = (120, 119), caseExpect = [res (0xffffffd6)] }
  , (mk "unequal-flags-a-out-of-range" FlagSet) { caseArgs = [(R0, idA 120), (R1, 1)], caseFlagCounts = (119, 120), caseExpect = [res (0xffffffee)] }
  , (mk "unequal-mailboxes-a-valid-b-refused" MailboxSend) { caseArgs = [(R0, idB 1), (R1, messageAddress 0)], caseMailboxCounts = (1, 0), caseExpect = [res (0xffffffee)] }
  , (mk "flag-set-f-clear" FlagSet) { caseArgs = [(R0, id0 1), (R1, 2)], caseFlags = [flag 1 0 0 1], caseCpsr = 0x13, caseExpect = [res 0, eq (patternOf 1) 3, control 0x13] }
  , (mk "flag-set-f-set" FlagSet) { caseArgs = [(R0, id0 1), (R1, 2)], caseFlags = [flag 1 0 0 1], caseCpsr = 0x53, caseExpect = [res 0, eq (patternOf 1) 3, control 0x53] }
  , (mk "flag-create-f-set" FlagCreate) { caseArgs = [(R0, id0 2), (R1, requestAddress)], caseCpsr = 0x53, caseWords = [poke requestAddress 0, poke (requestAddress + 4) 1], caseExpect = [res 0, eq (flagConfig facts executing 2 + factsFlagConfigExists facts) 1, control 0x53] }

    -- Create and delete, including publication and out-of-range differences.
  , (mk "flag-create" FlagCreate) { caseArgs = [(R0, id0 2), (R1, requestAddress)], caseWords = [poke requestAddress 4, poke (requestAddress + 4) 5], caseExpect = createFlagOwned 2, caseStockExpect = Just (createFlagStock 2), caseDifference = CreatePublication }
  , (mk "mailbox-create" MailboxCreate) { caseArgs = [(R0, id0 1), (R1, requestAddress)], caseWords = [poke requestAddress 0, poke (requestAddress + 4) 32], caseExpect = createMailboxOwned, caseStockExpect = Just createMailboxStock, caseDifference = CreatePublication }
  , (mk "flag-create-recreate" FlagCreate) { caseArgs = [(R0, id0 1), (R1, requestAddress)], caseWords = [poke requestAddress 4, poke (requestAddress + 4) 5], caseExpect = [res 0, eq (flagConfig facts executing 1 + factsFlagConfigExists facts) 1, eq (patternOf 1) 5] }
  , (mk "flag-delete-plain" FlagDelete) { caseArgs = [(R0, id0 1)], caseFlags = [flag 1 4 5 5], caseExpect = deleteFlagState 1 }
  , (mk "flag-delete-class-ignored" FlagDelete) { caseArgs = [(R0, foreignId 1)], caseFlags = [flag 1 0 0 0, (peer executing, 1, 0, 0, 0)], caseExpect = [res 0, eq (flagConfig facts executing 1 + factsFlagConfigExists facts) 0, eq (flagConfig facts (peer executing) 1 + factsFlagConfigExists facts) 1] }
  , (mk "flag-delete-stranded" FlagDelete) { caseArgs = [(R0, id0 1)], caseFlags = [flag 1 0 0 1], caseWaiters = [timedWaiter 14], caseExpect = deleteStrandedOwned FlagFamily, caseStockExpect = Just (deleteStrandedStock FlagFamily), caseDifference = DeleteStrandsWaiter }
  , (mk "mailbox-delete-stranded" MailboxDelete) { caseArgs = [(R0, id0 1)], caseMailboxes = [mailbox 1 0 32 0 0], caseWaiters = [timedWaiter 14], caseExpect = deleteStrandedOwned MailboxFamily, caseStockExpect = Just (deleteStrandedStock MailboxFamily), caseDifference = DeleteStrandsWaiter }
  , (mk "mailbox-delete-drains" MailboxDelete) { caseArgs = [(R0, id0 1)], caseMailboxes = [mailbox 1 0 32 (messageAddress 0) (messageAddress 1)], caseMessages = [msg 0 (messageAddress 1) 0, msg 1 0 0], caseExpect = [res 0, eq (mailboxConfig facts executing 1 + factsMailboxConfigExists facts) 0, eq (mailboxConfig facts executing 1 + factsMailboxConfigAttributes facts) 2, eq (mailboxConfig facts executing 1 + factsMailboxConfigPriorityLimit facts) 32, eq (mailControl 1 + factsMailboxControlHead facts) 0, eq (mailControl 1 + factsMailboxControlTail facts) 0, eq (messageAddress 0) (messageAddress 1), eq (messageAddress 1) 0], caseStockExpect = Just [res 0, eq (mailboxConfig facts executing 1 + factsMailboxConfigExists facts) 0, eq (mailboxConfig facts executing 1 + factsMailboxConfigAttributes facts) 2, eq (mailboxConfig facts executing 1 + factsMailboxConfigPriorityLimit facts) 32, eq (mailControl 1 + factsMailboxControlHead facts) 0, eq (mailControl 1 + factsMailboxControlTail facts) (messageAddress 1), eq (messageAddress 0) (messageAddress 1), eq (messageAddress 1) 0], caseDifference = DeleteLeavesQueueTail }
  , (mk "flag-delete-recreate" FlagDelete) { caseArgs = [(R0, id0 1)], caseFlags = [flag 1 0 0 1], caseSteps = [StepStore (GuestAddress resultCell), StepCall FlagCreate [(R0, id0 1), (R1, requestAddress)]], caseWords = [poke requestAddress 0, poke (requestAddress + 4) 5], caseExpect = [res 0, eq resultCell 0, eq (flagConfig facts executing 1 + factsFlagConfigExists facts) 1, eq (patternOf 1) 5] }
  , (mk "mailbox-delete-recreate" MailboxDelete) { caseArgs = [(R0, id0 1)], caseMailboxes = [mailbox 1 0 32 0 0], caseSteps = [StepStore (GuestAddress resultCell), StepCall MailboxCreate [(R0, id0 1), (R1, requestAddress)]], caseWords = [poke requestAddress 0, poke (requestAddress + 4) 32], caseExpect = [res 0, eq resultCell 0, eq (mailboxConfig facts executing 1 + factsMailboxConfigExists facts) 1, eq (mailboxConfig facts executing 1 + factsMailboxConfigPriorityLimit facts) 32] }
  , (mk "flag-create-out-of-range" FlagCreate) { caseArgs = [(R0, 121), (R1, requestAddress)], caseWords = [poke requestAddress 4, poke (requestAddress + 4) 5], caseExpect = [res (0xffffffee), eq (flagConfig facts executing 121 + factsFlagConfigAttributes facts) 0, eq (flagConfig facts executing 121 + factsFlagConfigInitial facts) 0, eq (flagConfig facts executing 121 + factsFlagConfigExists facts) 0, eq (flagControl facts executing 121 + factsWaitQueueNext facts) (flagControl facts executing 121), eq (flagControl facts executing 121 + factsWaitQueuePrev facts) (flagControl facts executing 121), eq (flagControl facts executing 121 + factsWaitQueueConfig facts) (mailboxConfig facts executing 1), eq (flagControl facts executing 121 + factsFlagControlPattern facts) 0], caseStockExpect = Just [res 0, eq (flagConfig facts executing 121 + factsFlagConfigAttributes facts) 4, eq (flagConfig facts executing 121 + factsFlagConfigInitial facts) 5, eq (flagConfig facts executing 121 + factsFlagConfigExists facts) 1, eq (flagControl facts executing 121 + factsWaitQueueNext facts) (flagControl facts executing 121), eq (flagControl facts executing 121 + factsWaitQueuePrev facts) (flagControl facts executing 121), eq (flagControl facts executing 121 + factsWaitQueueConfig facts) (mailboxConfig facts executing 1), eq (flagControl facts executing 121 + factsFlagControlPattern facts) 0], caseDifference = OutOfRangeWrite }
  , (mk "flag-delete-out-of-range" FlagDelete) { caseArgs = [(R0, 121)], caseExpect = [res (0xffffffee), eq (flagConfig facts executing 121 + factsFlagConfigAttributes facts) 0, eq (flagConfig facts executing 121 + factsFlagConfigInitial facts) 0, eq (flagConfig facts executing 121 + factsFlagConfigExists facts) 0, eq (flagControl facts executing 121 + factsWaitQueueNext facts) (flagControl facts executing 121), eq (flagControl facts executing 121 + factsWaitQueuePrev facts) (flagControl facts executing 121), eq (flagControl facts executing 121 + factsWaitQueueConfig facts) (mailboxConfig facts executing 1), eq (flagControl facts executing 121 + factsFlagControlPattern facts) 0], caseStockExpect = Just [res 0, eq (flagConfig facts executing 121 + factsFlagConfigAttributes facts) 0, eq (flagConfig facts executing 121 + factsFlagConfigInitial facts) 0, eq (flagConfig facts executing 121 + factsFlagConfigExists facts) 0, eq (flagControl facts executing 121 + factsWaitQueueNext facts) (flagControl facts executing 121), eq (flagControl facts executing 121 + factsWaitQueuePrev facts) (flagControl facts executing 121), eq (flagControl facts executing 121 + factsWaitQueueConfig facts) (mailboxConfig facts executing 1), eq (flagControl facts executing 121 + factsFlagControlPattern facts) 0], caseDifference = OutOfRangeWrite }

    -- Flag set, handler, clear, poll.
  , (mk "flag-set-no-waiter" FlagSet) { caseArgs = [(R0, id0 1), (R1, 2)], caseFlags = [flag 1 0 0 1], caseExpect = [res 0, eq (patternOf 1) 3] }
  , (mk "flag-set-satisfied-arrival" FlagSet) { caseArgs = [(R0, id0 1), (R1, 2)], caseFlags = [flag 1 0 0 1], caseWaiters = [waiterWith 5 3 0], caseBoundary = TaskArrival, caseExpect = [eq (flagPattern executing 2) 3, readyCheck executing 5 (taskOf executing 2)] }
  , (mk "flag-set-unsatisfied" FlagSet) { caseArgs = [(R0, id0 1), (R1, 2)], caseFlags = [flag 1 0 0 1], caseWaiters = [waiterWith 5 4 0], caseExpect = [res 0, eq (patternOf 1) 3, byte (taskOf executing 2 + factsTaskState facts) (factsStateWait facts .|. factsStateObjectQueued facts .|. factsStateOwnerLockQueued facts), eq (flagWaitResult executing 2) 0] }
  , (mk "flag-set-clear-on-match" FlagSet) { caseArgs = [(R0, id0 1), (R1, 1)], caseFlags = [flag 1 4 1 1], caseWaiters = [waiterWith 5 1 0], caseBoundary = TaskArrival, caseExpect = [eq (patternOf 1) 0, eq (flagPattern executing 2) 1, readyCheck executing 5 (taskOf executing 2)] }
  , (mk "flag-set-waiter-peer" FlagSet) { caseArgs = [(R0, id0 1), (R1, 2)], caseFlags = [flag 1 0 0 1], caseWaiters = [waiterPeer 5 3 0], caseExpect = [res 0, eq (flagPattern (peer executing) 2) 3, readyCheck (peer executing) 5 (taskOf (peer executing) 2), eq (bankAddress (peer executing)) 0x01000000, eq (bankAddress executing) 0] }
  , (mk "flag-set-handler-same" FlagSetHandler) { caseInvocation = HandlerCallback, caseHandler = 1, caseArgs = [(R0, id0 1), (R1, 2)], caseFlags = [flag 1 0 0 1], caseWaiters = [waiterWith 5 3 0], caseExpect = [res 0, eq (flagPattern executing 2) 3, readyCheck executing 5 (taskOf executing 2), eq (rescheduleAddress executing) 1, eq (bankAddress executing) 0, eq (bankAddress (peer executing)) 0] }
  , (mk "flag-set-handler-cross" FlagSetHandler) { caseInvocation = HandlerCallback, caseHandler = 1, caseArgs = [(R0, id0 1), (R1, 2)], caseFlags = [flag 1 0 0 1], caseWaiters = [waiterPeer 5 3 0], caseExpect = handlerOwned, caseStockExpect = Just handlerStock, caseDifference = HandlerWaiterDomain }
  , (mk "flag-clear" FlagClear) { caseArgs = [(R0, id0 1), (R1, 1)], caseFlags = [flag 1 0 0 3], caseExpect = [res 0, eq (patternOf 1) 1] }
  , (mk "flag-poll-satisfied" FlagPoll) { caseArgs = [(R0, id0 1), (R1, 1), (R2, 0), (R3, outWord)], caseFlags = [flag 1 0 0 3], caseWords = [poke outWord 0xdead], caseExpect = [res 0, eq outWord 3, eq (patternOf 1) 3] }
  , (mk "flag-poll-unsatisfied" FlagPoll) { caseArgs = [(R0, id0 1), (R1, 4), (R2, 0), (R3, outWord)], caseFlags = [flag 1 0 0 3], caseWords = [poke outWord 0xdead], caseExpect = [res (0xffffffce), eq outWord 0xdead] }
  , (mk "flag-poll-overflow" FlagPoll) { caseArgs = [(R0, id0 1), (R1, 1), (R2, 0), (R3, outWord)], caseFlags = [flag 1 0 0 3], caseWaiters = [waiterWith 5 1 0], caseExpect = [res (0xffffffe4)] }
  , (mk "flag-poll-clear-on-match" FlagPoll) { caseArgs = [(R0, id0 1), (R1, 1), (R2, 0), (R3, outWord)], caseFlags = [flag 1 4 3 3], caseExpect = [res 0, eq outWord 3, eq (patternOf 1) 0] }
  , (mk "flag-poll-alias-attributes" FlagPoll) { caseArgs = [(R0, id0 1), (R1, 1), (R2, 0), (R3, attributesOf 1)], caseSteps = [StepStore (GuestAddress resultCell), StepCall FlagPoll [(R0, id0 1), (R1, 1), (R2, 0), (R3, outWord)]], caseFlags = [flag 1 4 0x31 0x31], caseWords = [poke outWord 0xdead], caseExpect = [res (0xffffffce), eq resultCell 0, eq (patternOf 1) 0, eq outWord 0xdead, eq (attributesOf 1) 0x31], caseStockExpect = Just [res 0, eq resultCell 0, eq (patternOf 1) 0x31, eq outWord 0x31, eq (attributesOf 1) 0x31], caseDifference = AliasAttributes }
  , (mk "flag-wait-alias-pattern" FlagWait) { caseArgs = [(R0, id0 1), (R1, 1), (R2, 0), (R3, patternOf 1)], caseFlags = [flag 1 4 0x11 0x11], caseExpect = [res 0, eq (patternOf 1) 0x11], caseStockExpect = Just [res 0, eq (patternOf 1) 0], caseDifference = AliasPatternClear }
  , (mk "flag-poll-alias-pattern" FlagPoll) { caseArgs = [(R0, id0 1), (R1, 1), (R2, 0), (R3, patternOf 1)], caseFlags = [flag 1 4 0x11 0x11], caseExpect = [res 0, eq (patternOf 1) 0x11], caseStockExpect = Just [res 0, eq (patternOf 1) 0], caseDifference = AliasPatternClear }

    -- Flag waits, including the real waker and S2 caller-frame return.
  , (mk "flag-wait-untimed-wake" FlagWait) { caseArgs = [(R0, id0 1), (R1, 3), (R2, 0), (R3, outWord)], caseFlags = [flag 1 0 0 1], caseWords = [poke outWord 0xdead], caseWaker = Just (Waker (TargetSlot FlagSet) [(R0, id0 1), (R1, 2)]), caseExpect = [res 0, eq outWord 3, eq (patternOf 1) 3, eq (flagWaitResult executing 1) 0, readyCheck executing 10 (taskOf executing 1)] }
  , (mk "flag-wait-untimed-delete" FlagWait) { caseArgs = [(R0, id0 1), (R1, 3), (R2, 0), (R3, outWord)], caseFlags = [flag 1 0 0 1], caseWords = [poke outWord 0xdead], caseWaker = Just (Waker (TargetSlot FlagDelete) [(R0, id0 1)]), caseExpect = [res (0xffffffcf), eq outWord 0xdead, eq (flagWaitResult executing 1) (0xffffffcf), eq (flagConfig facts executing 1 + factsFlagConfigExists facts) 0, readyCheck executing 10 (taskOf executing 1), eq (readyHead facts executing 10 + factsHeadPrev facts) (taskOf executing 1), eq (dfReadyBitmap (domainFacts facts executing)) ((1 `shiftL` 14) .|. (1 `shiftL` 15) .|. (1 `shiftL` 10)), eq (dfRunning (domainFacts facts executing)) (taskAt facts executing runningSlot), eq (dfSelected (domainFacts facts executing)) (taskAt facts executing runningSlot), eq (taskOf executing 1 + factsTaskNext facts) (readyHead facts executing 10), eq (taskOf executing 1 + factsTaskPrev facts) (readyHead facts executing 10), byte (taskOf executing 1 + factsTaskState facts) (factsStateReady facts)], caseStockExpect = Just [eq outWord 0xdead, eq (flagWaitResult executing 1) 0, eq (readyHead facts executing 10) (readyHead facts executing 10), eq (readyHead facts executing 10 + factsHeadPrev facts) (readyHead facts executing 10), eq (dfReadyBitmap (domainFacts facts executing)) ((1 `shiftL` 14) .|. (1 `shiftL` 15)), eq (dfRunning (domainFacts facts executing)) (taskAt facts executing 4), eq (dfSelected (domainFacts facts executing)) (taskAt facts executing 4), eq (taskOf executing 1 + factsTaskNext facts) (flagControl facts executing 1), eq (taskOf executing 1 + factsTaskPrev facts) (flagControl facts executing 1), byte (taskOf executing 1 + factsTaskState facts) (factsStateWait facts .|. factsStateObjectQueued facts .|. factsStateOwnerLockQueued facts)], caseStockStatus = Just InstructionBudget, caseDifference = DeleteStrandsWaiter, caseLimit = InstructionLimit 50000 }
  , (mk "flag-wait-timed-wake" FlagTimedWait) { caseArgs = [(R0, id0 1), (R1, 3), (R2, 0), (R3, outWord), (R4, 5)], caseFlags = [flag 1 0 0 1], caseTick = 100, caseWords = [poke outWord 0xdead], caseWaker = Just (Waker (TargetSlot FlagSet) [(R0, id0 1), (R1, 2)]), caseExpect = [res 0, eq outWord 3, eq (flagWaitResult executing 1) 0, eq (flagTimerOf executing 1 + 4) (seam executing "stock-timeout-callback"), eq (flagTimerOf executing 1 + 8) (taskOf executing 1), timerLog 1 [(recordAddress facts executing executing, flagTimerOf executing 1)]] }
  , (mk "flag-wait-timed-expire" FlagTimedWait) { caseArgs = [(R0, id0 1), (R1, 3), (R2, 0), (R3, outWord), (R4, 5)], caseFlags = [flag 1 0 0 1], caseTick = 100, caseWords = [poke outWord 0xdead], caseWaker = Just (Waker (TargetTimerTick 2) []), caseExpect = [res (0xffffffce), eq outWord 0xdead, eq (flagWaitResult executing 1) (0xffffffce), eq (flagWait facts executing 1 + factsWaitBlockOwner facts) (dfId (domainFacts facts executing)), eq (flagWait facts executing 1 + factsWaitBlockQueue facts) (flagControl facts executing 1), eq (taskAt facts executing runningSlot + factsTaskWait facts) (flagWait facts executing 1)], caseSkipTimerLog = True }
  , (mk "flag-wait-timed-cross-expire" FlagTimedWait) { caseObject = DomainB, caseArgs = [(R0, idB 1), (R1, 3), (R2, 0), (R3, outWord), (R4, 5)], caseFlags = [(DomainB, 1, 0, 0, 1)], caseTick = 100, caseWords = [poke outWord 0xdead], caseWaker = Just (Waker (TargetTimerTick 2) []), caseExpect = [res (0xffffffce), eq outWord 0xdead, eq (flagWait facts executing 1 + factsWaitBlockOwner facts) (dfId (domainFacts facts DomainB)), eq (flagWait facts executing 1 + factsWaitBlockQueue facts) (flagControl facts DomainB 1), eq (taskAt facts executing runningSlot + factsTaskWait facts) (flagWait facts executing 1), eq (flagWaitResult executing 1) (0xffffffce), eq (flagTimerOf executing 1 + 4) (seam executing "stock-timeout-callback"), eq (flagTimerOf executing 1 + 8) (taskOf executing 1)], caseSkipTimerLog = True }
  , (mk "flag-wait-timed-zero" FlagTimedWait) { caseArgs = [(R0, id0 1), (R1, 3), (R2, 0), (R3, outWord), (R4, 0)], caseFlags = [flag 1 0 0 1], caseWords = [poke outWord 0xdead], caseExpect = [res (0xffffffce), eq outWord 0xdead] }
  , (mk "flag-wait-overflow" FlagWait) { caseArgs = [(R0, id0 1), (R1, 3), (R2, 0), (R3, outWord)], caseFlags = [flag 1 0 0 1], caseWaiters = [waiterWith 14 3 0], caseExpect = [res (0xffffffe4)] }

    -- Mailbox send, receive and poll.
  , (mk "mailbox-send-empty" MailboxSend) { caseArgs = [(R0, id0 1), (R1, messageAddress 0)], caseMailboxes = [mailbox 1 0 32 0 0], caseMessages = [msg 0 0 0], caseExpect = [res 0, eq (mailControl 1 + factsMailboxControlHead facts) (messageAddress 0), eq (mailControl 1 + factsMailboxControlTail facts) (messageAddress 0), eq (messageAddress 0) 0] }
  , (mk "mailbox-send-fifo" MailboxSend) { caseArgs = [(R0, id0 1), (R1, messageAddress 1)], caseMailboxes = [mailbox 1 0 32 (messageAddress 0) (messageAddress 0)], caseMessages = [msg 0 0 0, msg 1 0 0], caseExpect = [res 0, eq (messageAddress 0) (messageAddress 1), eq (messageAddress 1) 0] }
  , (mk "mailbox-send-priority" MailboxSend) { caseArgs = [(R0, id0 1), (R1, messageAddress 1)], caseMailboxes = [mailbox 1 2 32 (messageAddress 0) (messageAddress 2)], caseMessages = [msg 0 (messageAddress 2) 5, msg 1 0 6, msg 2 0 7], caseExpect = [res 0, eq (messageAddress 0) (messageAddress 1), eq (messageAddress 1) (messageAddress 2), eq (mailControl 1 + factsMailboxControlTail facts) (messageAddress 2), eq (messageAddress 2) 0] }
  , (mk "mailbox-send-priority-equal" MailboxSend) { caseArgs = [(R0, id0 1), (R1, messageAddress 1)], caseMailboxes = [mailbox 1 2 32 (messageAddress 0) (messageAddress 0)], caseMessages = [msg 0 0 5, msg 1 0 5], caseExpect = [res 0, eq (messageAddress 0) (messageAddress 1), eq (messageAddress 1) 0] }
  , (mk "mailbox-send-bad-priority" MailboxSend) { caseArgs = [(R0, id0 1), (R1, messageAddress 0)], caseMailboxes = [mailbox 1 2 32 0 0], caseMessages = [msg 0 0 0], caseExpect = [res (0xffffffef), eq (mailControl 1 + factsMailboxControlHead facts) 0] }
  , (mk "mailbox-send-waiter-arrival" MailboxSend) { caseArgs = [(R0, id0 1), (R1, messageAddress 0)], caseMailboxes = [mailbox 1 0 32 0 0], caseMessages = [msg 0 0 0], caseWaiters = [waiterPeer 5 0 0], caseExpect = [res 0, eq (mailboxWait facts (peer executing) 2 + factsMailboxWaitMessage facts) (messageAddress 0), readyCheck (peer executing) 5 (taskOf (peer executing) 2), eq (bankAddress (peer executing)) 0x01000000, eq (bankAddress executing) 0] }
  , (mk "mailbox-receive-ready" MailboxReceive) { caseArgs = [(R0, id0 1), (R1, outWord)], caseMailboxes = [mailbox 1 0 32 (messageAddress 0) (messageAddress 0)], caseMessages = [msg 0 0 0], caseWords = [poke outWord 0xdead], caseExpect = [res 0, eq outWord (messageAddress 0), eq (mailControl 1 + factsMailboxControlHead facts) 0] }
  , (mk "mailbox-poll-ready" MailboxPoll) { caseArgs = [(R0, id0 1), (R1, outWord)], caseMailboxes = [mailbox 1 0 32 (messageAddress 0) (messageAddress 0)], caseMessages = [msg 0 0 0], caseWords = [poke outWord 0xdead], caseExpect = [res 0, eq outWord (messageAddress 0), eq (mailControl 1 + factsMailboxControlHead facts) 0] }
  , (mk "mailbox-poll-empty" MailboxPoll) { caseArgs = [(R0, id0 1), (R1, outWord)], caseMailboxes = [mailbox 1 0 32 0 0], caseWords = [poke outWord 0xdead], caseExpect = [res (0xffffffce), eq outWord 0xdead] }
  , (mk "mailbox-poll-alias-next" MailboxPoll) { caseArgs = [(R0, id0 1), (R1, messageAddress 0)], caseSteps = [StepStore (GuestAddress resultCell), StepCall MailboxPoll [(R0, id0 1), (R1, outWord)]], caseMailboxes = [mailbox 1 0 32 (messageAddress 0) (messageAddress 1)], caseMessages = [msg 0 (messageAddress 1) 0, msg 1 0 0], caseWords = [poke outWord 0xdead], caseExpect = [res 0, eq resultCell 0, eq (mailControl 1 + factsMailboxControlHead facts) 0, eq outWord (messageAddress 1), eq (messageAddress 0) (messageAddress 0)], caseStockExpect = Just [res 0, eq resultCell 0, eq (mailControl 1 + factsMailboxControlHead facts) (messageAddress 0), eq outWord (messageAddress 0), eq (messageAddress 0) (messageAddress 0)], caseDifference = AliasNextLink }
  , (mk "mailbox-receive-untimed-wake" MailboxReceive) { caseArgs = [(R0, id0 1), (R1, outWord)], caseMailboxes = [mailbox 1 0 32 0 0], caseMessages = [msg 0 0 0], caseWords = [poke outWord 0xdead], caseWaker = Just (Waker (TargetSlot MailboxSend) [(R0, id0 1), (R1, messageAddress 0)]), caseExpect = [res 0, eq outWord (messageAddress 0), eq (mailControl 1 + factsMailboxControlHead facts) 0, eq (mailboxWaitResult executing 1) 0, readyCheck executing 10 (taskOf executing 1)] }
  , (mk "mailbox-receive-timed-wake" MailboxTimedReceive) { caseArgs = [(R0, id0 1), (R1, outWord), (R2, 5)], caseMailboxes = [mailbox 1 0 32 0 0], caseMessages = [msg 0 0 0], caseTick = 100, caseWords = [poke outWord 0xdead], caseWaker = Just (Waker (TargetSlot MailboxSend) [(R0, id0 1), (R1, messageAddress 0)]), caseExpect = [res 0, eq outWord (messageAddress 0), eq (mailControl 1 + factsMailboxControlHead facts) 0, eq (mailboxTimerOf executing 1 + 4) (seam executing "stock-timeout-callback"), eq (mailboxTimerOf executing 1 + 8) (taskOf executing 1), timerLog 1 [(recordAddress facts executing executing, mailboxTimerOf executing 1)]] }
  , (mk "mailbox-receive-timed-expire" MailboxTimedReceive) { caseArgs = [(R0, id0 1), (R1, outWord), (R2, 5)], caseMailboxes = [mailbox 1 0 32 0 0], caseTick = 100, caseWords = [poke outWord 0xdead], caseWaker = Just (Waker (TargetTimerTick 2) []), caseExpect = [res (0xffffffce), eq outWord 0xdead, eq (mailboxWaitResult executing 1) (0xffffffce)], caseSkipTimerLog = True }
  , (mk "mailbox-receive-timed-zero" MailboxTimedReceive) { caseArgs = [(R0, id0 1), (R1, outWord), (R2, 0)], caseMailboxes = [mailbox 1 0 32 0 0], caseWords = [poke outWord 0xdead], caseExpect = [res (0xffffffce), eq outWord 0xdead] }
  , (mk "mailbox-receive-fifo" MailboxReceive) { caseArgs = [(R0, id0 1), (R1, outWord)], caseMailboxes = [mailbox 1 0 32 0 0], caseMessages = [msg 0 0 0, msg 1 0 0], caseWaiters = [chainWaiter 12], caseWords = [poke outWord 0xdead], caseWaker = Just (Waker (TargetSlot MailboxSend) [(R0, id0 1), (R1, messageAddress 0)]), caseExpect = [res 0, eq outWord (messageAddress 1), eq (mailControl 1 + factsMailboxControlHead facts) 0, eq (mailboxWait facts executing 2 + factsMailboxWaitMessage facts) (messageAddress 0), byte (taskOf executing 2 + factsTaskState facts) (factsStateReady facts)] }
  , (mk "mailbox-receive-priority" MailboxReceive) { caseArgs = [(R0, id0 1), (R1, outWord)], caseMailboxes = [mailbox 1 1 32 0 0], caseMessages = [msg 0 0 0], caseWaiters = [waiterWith 12 0 0], caseWords = [poke outWord 0xdead], caseWaker = Just (Waker (TargetSlot MailboxSend) [(R0, id0 1), (R1, messageAddress 0)]), caseExpect = [res 0, eq outWord (messageAddress 0), eq (mailControl 1 + factsMailboxControlHead facts) 0, byte (taskOf executing 2 + factsTaskState facts) (factsStateWait facts .|. factsStateObjectQueued facts .|. factsStateOwnerLockQueued facts), eq (mailboxWait facts executing 2 + factsMailboxWaitMessage facts) 0] }
  , (mk "mailbox-delete-multiwaiter" MailboxReceive) { caseArgs = [(R0, id0 1), (R1, outWord)], caseMailboxes = [mailbox 1 0 32 0 0], caseWaiters = [waiterPeerTimed 5], caseWords = [poke outWord 0xdead, poke requestAddress 0, poke (requestAddress + 4) 32], caseWaker = Just (Waker (TargetSlot MailboxDelete) [(R0, id0 1)]), caseSteps = [StepStore (GuestAddress resultCell), StepCall MailboxCreate [(R0, id0 1), (R1, requestAddress)]], caseExpect = multiwaiterOwned, caseStockExpect = Just multiwaiterStock, caseStockStatus = Just InstructionBudget, caseDifference = DeleteStrandsWaiter, caseLimit = InstructionLimit 50000 }

    -- Remaining class-ignored mailbox row.
  , (mk "mailbox-delete-class-ignored" MailboxDelete) { caseArgs = [(R0, foreignId 1)], caseMailboxes = [mailbox 1 0 32 0 0, (peer executing, 1, 0, 32, 0, 0)], caseExpect = [res 0, eq (mailboxConfig facts executing 1 + factsMailboxConfigExists facts) 0, eq (mailboxConfig facts (peer executing) 1 + factsMailboxConfigExists facts) 1] }
  ]
  where
    mk name slot = (baseCase executing) { caseName = CaseId name, caseSlot = slot, caseObject = executing, caseArgs = [] }
    foreignId slot = if executing == DomainA then idB slot else idA slot
    flag slot attributes initial pattern = (executing, slot, attributes, initial, pattern)
    mailbox slot attributes limit headValue tailValue = (executing, slot, attributes, limit, headValue, tailValue)
    msg index next priority = (messageAddress index, next, priority)
    patternOf slot = flagControl facts executing slot + factsFlagControlPattern facts
    attributesOf slot = flagConfig facts executing slot + factsFlagConfigAttributes facts
    mailControl slot = mailboxControl facts executing slot
    flagWaitResult domain slot = flagWait facts domain slot + factsWaitBlockResult facts
    flagPattern domain slot = flagWait facts domain slot + factsFlagWaitResult facts
    mailboxWaitResult domain slot = mailboxWait facts domain slot + factsWaitBlockResult facts
    flagTimerOf domain slot = objectTimer facts FlagWait domain slot
    mailboxTimerOf domain slot = objectTimer facts MailboxReceive domain slot
    bankAddress domain = pointerWord (bank domain)
    rescheduleAddress domain = dfReschedule (domainFacts facts domain)
    taskOf domain slot = taskAt facts domain slot
    readyCheck domain priority expectedTask = eq (readyHead facts domain priority) expectedTask
    waiterWith priority want mode = PreWaiter 2 executing priority want mode False Nothing
    waiterPeer priority want mode = PreWaiter 2 (peer executing) priority want mode False Nothing
    timedWaiter priority = PreWaiter 2 executing priority 0 0 True Nothing
    waiterPeerTimed priority = PreWaiter 2 (peer executing) priority 0 0 True Nothing
    chainWaiter priority = PreWaiter 2 executing priority 0 0 False (Just (Waker (TargetSlot MailboxSend) [(R0, id0 1), (R1, messageAddress 1)]))
    createFlagOwned slot = [res 0, eq (flagConfig facts executing slot + factsFlagConfigAttributes facts) 4, eq (flagConfig facts executing slot + factsFlagConfigInitial facts) 5, eq (flagConfig facts executing slot + factsFlagConfigExists facts) 1, eq (patternOf slot) 5, OrderBefore (GuestAddress (flagControl facts executing slot)) (GuestAddress (flagConfig facts executing slot + factsFlagConfigExists facts)), OrderBarrierBefore Dmb (GuestAddress (flagConfig facts executing slot + factsFlagConfigExists facts))]
    createFlagStock slot = [res 0, eq (flagConfig facts executing slot + factsFlagConfigAttributes facts) 4, eq (flagConfig facts executing slot + factsFlagConfigInitial facts) 5, eq (flagConfig facts executing slot + factsFlagConfigExists facts) 1, eq (patternOf slot) 5, OrderBefore (GuestAddress (flagConfig facts executing slot + factsFlagConfigExists facts)) (GuestAddress (flagControl facts executing slot))]
    createMailboxOwned = [res 0, eq (mailboxConfig facts executing 1 + factsMailboxConfigAttributes facts) 0, eq (mailboxConfig facts executing 1 + factsMailboxConfigPriorityLimit facts) 32, eq (mailboxConfig facts executing 1 + factsMailboxConfigExists facts) 1, eq (mailControl 1 + factsMailboxControlHead facts) 0, eq (mailControl 1 + factsMailboxControlTail facts) 0, OrderBefore (GuestAddress (mailControl 1)) (GuestAddress (mailboxConfig facts executing 1 + factsMailboxConfigExists facts)), OrderBarrierBefore Dmb (GuestAddress (mailboxConfig facts executing 1 + factsMailboxConfigExists facts))]
    createMailboxStock = [res 0, eq (mailboxConfig facts executing 1 + factsMailboxConfigAttributes facts) 0, eq (mailboxConfig facts executing 1 + factsMailboxConfigPriorityLimit facts) 32, eq (mailboxConfig facts executing 1 + factsMailboxConfigExists facts) 1, eq (mailControl 1 + factsMailboxControlHead facts) 0, eq (mailControl 1 + factsMailboxControlTail facts) 0, OrderBefore (GuestAddress (mailboxConfig facts executing 1 + factsMailboxConfigExists facts)) (GuestAddress (mailControl 1))]
    deleteFlagState slot = [res 0, eq (flagConfig facts executing slot + factsFlagConfigAttributes facts) 0, eq (flagConfig facts executing slot + factsFlagConfigInitial facts) 0, eq (flagConfig facts executing slot + factsFlagConfigExists facts) 0, eq (flagControl facts executing slot) (flagControl facts executing slot), eq (patternOf slot) 0]
    strandedBitmap = (1 `shiftL` 10) .|. (1 `shiftL` 15)
    deleteStrandedOwned family' =
      let timer = case family' of FlagFamily -> flagTimerOf executing 2; MailboxFamily -> mailboxTimerOf executing 2
          waitResult = case family' of FlagFamily -> flagWaitResult executing 2; MailboxFamily -> mailboxWaitResult executing 2
       in [res 0, byte (taskOf executing 2 + factsTaskState facts) (factsStateReady facts), eq waitResult (0xffffffcf), readyCheck executing 14 (taskOf executing 2), eq (readyHead facts executing 14 + factsHeadPrev facts) (taskOf executing 2), eq (taskOf executing 2 + factsTaskNext facts) (readyHead facts executing 14), eq (taskOf executing 2 + factsTaskPrev facts) (readyHead facts executing 14), eq (dfReadyBitmap (domainFacts facts executing)) (strandedBitmap .|. (1 `shiftL` 14)), timerLog 1 [(recordAddress facts executing executing, timer)]]
    deleteStrandedStock family' =
      let timer = case family' of FlagFamily -> flagTimerOf executing 2; MailboxFamily -> mailboxTimerOf executing 2
          waitResult = case family' of FlagFamily -> flagWaitResult executing 2; MailboxFamily -> mailboxWaitResult executing 2
          queue = case family' of FlagFamily -> flagControl facts executing 1; MailboxFamily -> mailControl 1
       in [res 0, byte (taskOf executing 2 + factsTaskState facts) (factsStateWait facts .|. factsStateObjectQueued facts .|. factsStateOwnerLockQueued facts), eq waitResult timer, readyCheck executing 14 (readyHead facts executing 14), eq (readyHead facts executing 14 + factsHeadPrev facts) (readyHead facts executing 14), eq (taskOf executing 2 + factsTaskNext facts) queue, eq (taskOf executing 2 + factsTaskPrev facts) queue, eq (dfReadyBitmap (domainFacts facts executing)) strandedBitmap]
    multiwaiterStock =
      [ eq (mailboxConfig facts executing 1 + factsMailboxConfigAttributes facts) 2
      , eq (mailboxConfig facts executing 1 + factsMailboxConfigPriorityLimit facts) 32
      , eq (mailboxConfig facts executing 1 + factsMailboxConfigExists facts) 0
      , eq (mailControl 1 + factsMailboxControlHead facts) 0
      , eq (mailControl 1 + factsMailboxControlTail facts) 0
      , eq (mailboxWaitResult executing 1) 0
      , eq (mailboxWaitResult (peer executing) 2) (mailboxTimerOf (peer executing) 2)
      , byte (taskOf executing 1 + factsTaskState facts) (factsStateWait facts .|. factsStateObjectQueued facts .|. factsStateOwnerLockQueued facts)
      , byte (taskOf (peer executing) 2 + factsTaskState facts) (factsStateWait facts .|. factsStateObjectQueued facts .|. factsStateOwnerLockQueued facts)
      , eq (readyHead facts executing 10) (readyHead facts executing 10)
      , eq (readyHead facts executing 10 + factsHeadPrev facts) (readyHead facts executing 10)
      , eq (readyHead facts (peer executing) 5) (readyHead facts (peer executing) 5)
      , eq (readyHead facts (peer executing) 5 + factsHeadPrev facts) (readyHead facts (peer executing) 5)
      , eq (dfReadyBitmap (domainFacts facts executing)) ((1 `shiftL` 14) .|. (1 `shiftL` 15))
      , eq (dfReadyBitmap (domainFacts facts (peer executing))) strandedBitmap
      , eq (dfRunning (domainFacts facts executing)) (taskAt facts executing 4)
      , eq (dfSelected (domainFacts facts executing)) (taskAt facts executing 4)
      , eq (dfRunning (domainFacts facts (peer executing))) (taskAt facts (peer executing) runningSlot)
      , eq (dfSelected (domainFacts facts (peer executing))) (taskAt facts (peer executing) runningSlot)
      , eq (taskOf executing 1 + factsTaskNext facts) (mailControl 1)
      , eq (taskOf executing 1 + factsTaskPrev facts) (taskOf (peer executing) 2)
      , eq (taskOf (peer executing) 2 + factsTaskNext facts) (taskOf executing 1)
      , eq (taskOf (peer executing) 2 + factsTaskPrev facts) (mailControl 1)
      , eq (outWord + 8) 0
      , eq (bankAddress executing) 0
      , eq (bankAddress (peer executing)) 0
      ]
    handlerOwned = [res 0, eq (flagPattern (peer executing) 2) 3, readyCheck (peer executing) 5 (taskOf (peer executing) 2), eq (bankAddress (peer executing)) 0x01000000, eq (rescheduleAddress executing) 0, eq (bankAddress executing) 0, eq (readyHead facts executing 5) (readyHead facts executing 5), eq (readyHead facts executing 5 + factsHeadPrev facts) (readyHead facts executing 5), eq (dfReadyBitmap (domainFacts facts executing)) strandedBitmap, eq (dfSelected (domainFacts facts executing)) (taskAt facts executing runningSlot), eq (readyHead facts (peer executing) 5 + factsHeadPrev facts) (taskOf (peer executing) 2), eq (dfReadyBitmap (domainFacts facts (peer executing))) (strandedBitmap .|. (1 `shiftL` 5)), eq (dfSelected (domainFacts facts (peer executing))) (taskOf (peer executing) 2), eq (taskOf (peer executing) 2 + factsTaskNext facts) (readyHead facts (peer executing) 5), eq (taskOf (peer executing) 2 + factsTaskPrev facts) (readyHead facts (peer executing) 5)]
    handlerStock = [res 0, eq (flagPattern (peer executing) 2) 3, readyCheck executing 5 (taskOf (peer executing) 2), eq (rescheduleAddress executing) 0, eq (bankAddress executing) 0, eq (bankAddress (peer executing)) 0x01000000, eq (readyHead facts (peer executing) 5) (readyHead facts (peer executing) 5), eq (readyHead facts executing 5 + factsHeadPrev facts) (taskOf (peer executing) 2), eq (dfReadyBitmap (domainFacts facts executing)) (strandedBitmap .|. (1 `shiftL` 5)), eq (dfSelected (domainFacts facts executing)) (taskOf (peer executing) 2), eq (readyHead facts (peer executing) 5 + factsHeadPrev facts) (readyHead facts (peer executing) 5), eq (dfReadyBitmap (domainFacts facts (peer executing))) strandedBitmap, eq (dfSelected (domainFacts facts (peer executing))) (taskAt facts (peer executing) runningSlot), eq (taskOf (peer executing) 2 + factsTaskNext facts) (readyHead facts executing 5), eq (taskOf (peer executing) 2 + factsTaskPrev facts) (readyHead facts executing 5)]
    multiwaiterOwned =
      [ res 0, eq resultCell (0xffffffcf), eq outWord 0xdead
      , eq (mailboxConfig facts executing 1 + factsMailboxConfigExists facts) 1
      , eq (mailboxConfig facts executing 1 + factsMailboxConfigAttributes facts) 0
      , eq (mailboxConfig facts executing 1 + factsMailboxConfigPriorityLimit facts) 32
      , eq (mailControl 1 + factsMailboxControlHead facts) 0
      , eq (mailControl 1 + factsMailboxControlTail facts) 0
      , eq (mailboxWaitResult executing 1) (0xffffffcf)
      , eq (mailboxWaitResult (peer executing) 2) (0xffffffcf)
      , byte (taskOf (peer executing) 2 + factsTaskState facts) (factsStateReady facts)
      , readyCheck (peer executing) 5 (taskOf (peer executing) 2)
      , readyCheck executing 10 (taskOf executing 1)
      , eq (bankAddress (peer executing)) 0x01000000
      , eq (bankAddress executing) 0
      , eq (readyHead facts executing 10 + factsHeadPrev facts) (taskOf executing 1)
      , eq (dfReadyBitmap (domainFacts facts executing)) ((1 `shiftL` 14) .|. (1 `shiftL` 15) .|. (1 `shiftL` 10))
      , eq (dfRunning (domainFacts facts executing)) (taskAt facts executing runningSlot)
      , eq (dfSelected (domainFacts facts executing)) (taskAt facts executing runningSlot)
      , eq (taskOf executing 1 + factsTaskNext facts) (readyHead facts executing 10)
      , eq (taskOf executing 1 + factsTaskPrev facts) (readyHead facts executing 10)
      , byte (taskOf executing 1 + factsTaskState facts) (factsStateReady facts)
      , eq (outWord + 8) 0xffffffcf
      , eq (readyHead facts (peer executing) 5 + factsHeadPrev facts) (taskOf (peer executing) 2)
      , eq (dfReadyBitmap (domainFacts facts (peer executing))) (strandedBitmap .|. (1 `shiftL` 5))
      , eq (dfSelected (domainFacts facts (peer executing))) (taskOf (peer executing) 2)
      , eq (taskOf (peer executing) 2 + factsTaskNext facts) (readyHead facts (peer executing) 5)
      , eq (taskOf (peer executing) 2 + factsTaskPrev facts) (readyHead facts (peer executing) 5)
      , timerLog 1 [(recordAddress facts executing (peer executing), mailboxTimerOf (peer executing) 2)]
      ]
