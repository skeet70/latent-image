{-# LANGUAGE OverloadedStrings #-}
{-# OPTIONS_GHC -Wno-orphans #-}
module Emulation.SchedulerPairCheck where

import Control.Monad (foldM, forM_, unless)
import Data.Aeson (FromJSON (..), Value, object, withObject, (.:), (.:?), (.=))
import qualified Data.Aeson as J
import Data.Aeson.Types (Parser)
import Data.Bits ((.&.), (.|.), complement, shiftL)
import Data.Either (partitionEithers)
import qualified Data.ByteString as B
import Data.List (sort, sortOn)
import qualified Data.Map.Strict as M
import qualified Data.Set as S
import qualified Data.Text as T
import Data.Word (Word32, Word64)
import qualified Emulation.OwnFirstPairCheck as Q
import qualified Emulation.OwnFirstPairPlan as P
import Emulation.SchedulerPairModel
import Emulation.SchedulerPairPlan
import qualified Emulation.SchedulerPairS2 as D
import qualified Emulation.SchedulerPairService as V
import qualified Emulation.SchedulerPairS3a as A
import qualified Emulation.SchedulerPairS3b as C
import qualified Emulation.SchedulerPairS3c as E
import Emulation.SchedulerPairFamily (FamilyCall (..))
import qualified Emulation.SchedulerPairFamily as F
import Emulation.SchedulerPairSemaphore (SemaphoreCall (..))
import qualified Emulation.SchedulerPairSemaphore as Sem
import Emulation.Sha256 (sha256, sha256Hex)
import Replacement.RtosOwnFirst.Regions (Domain (..), domains, domainLabel)
import Replacement.RtosScheduler.Regions (Helper (..))

data Written = Written Address Int Word32 deriving (Eq,Ord,Show)
data WriteOrigin = WriteOrigin { writerCore :: Int, writerPc :: Address, writerSp :: Address, writerCpsr :: Word32,
  writerTask :: Address, writerContext :: Word32, writerInstructions :: Word64 } deriving (Eq,Show)
data TaskStack = TaskStack Address Address deriving (Eq,Show)
instance FromJSON TaskStack where
  parseJSON = withObject "task stack" $ \o -> TaskStack <$> (Address <$> (o .: "start" >>= parseHex)) <*> (Address <$> (o .: "end" >>= parseHex))
instance FromJSON WriteOrigin where
  parseJSON = withObject "writer envelope" $ \o -> WriteOrigin <$> (o .: "core" >>= coreOf)
    <*> (Address <$> (o .: "pc" >>= parseHex)) <*> (Address <$> (o .: "sp" >>= parseHex)) <*> o .: "cpsr"
    <*> (Address <$> (o .: "task" >>= parseHex)) <*> o .: "kernel_context" <*> o .: "instructions"
originsOf :: Value -> Parser [WriteOrigin]
originsOf value = J.parseJSON value >>= mapM (withObject "attributed write" (.: "origin"))
data Call = Call { callDomain :: Domain, callAddress :: Address, callSemantic :: Semantic, callCore :: Int, callCpsr :: Word32, callArguments :: Arguments,
  callBefore :: Memory, callAfter :: Memory, callR0 :: Word32, callReturnCpsr :: Word32, callWrites :: [Written], callBank :: [Written], callCount :: Int, callFirst :: Word64, callTrace :: Maybe CallTrace, callTask :: Word32,
  callKernelContext :: Word32, callTaskStack :: Maybe TaskStack, callWriteOrigins :: [WriteOrigin], callBankOrigins :: [WriteOrigin], callUnreadable :: S.Set Address } deriving (Eq,Show)
data PcWatch = PcWatch Int Address Word64 deriving (Eq,Show)
data BankCounts = BankCounts { primaryBankWrites :: Word64, secondaryBankWrites :: Word64 } deriving (Eq,Show)
instance FromJSON BankCounts where
  parseJSON value = do
    values <- J.parseJSON value
    case values of [primary,secondary] -> pure (BankCounts primary secondary); _ -> fail "bank counts must name both cores"
data DemandContext = DemandContext { demandCore :: Int, demandPc :: Word32, demandSp :: Word32, demandLr :: Word32,
  demandCpsr :: Word32, demandRegisters :: [Word32], demandInstructions :: Word64, demandTask :: Word32 } deriving (Eq,Show)
data DemandService = DemandService DemandContext Word32 Address Word32 deriving (Eq,Show)
data DemandPaging = DemandPaging { demandLoad :: DemandContext, demandTarget :: Word32, demandLoadCount :: Word64,
  demandAbort :: DemandContext, demandAbortCount :: Word64, demandSpsr :: Word32, demandIfar :: Word32, demandIfsr :: Word32,
  demandService :: Maybe DemandService } deriving (Eq,Show)
instance FromJSON DemandContext where
  parseJSON = withObject "demand context" $ \o -> do
    values <- o .: "registers"
    unless (length values==16) (fail "demand context requires all registers")
    DemandContext <$> (o .: "core" >>= coreOf) <*> o .: "pc" <*> o .: "sp" <*> o .: "lr"
      <*> o .: "cpsr" <*> pure values <*> o .: "instructions" <*> (o .: "current" >>= wordRead)
instance FromJSON DemandService where
  parseJSON = withObject "demand service" $ \o -> do
    root <- o .: "root"
    DemandService <$> o .: "context" <*> (o .: "context_word" >>= wordRead)
      <*> (Address <$> (root .: "address" >>= parseHex)) <*> (root .: "value" >>= wordRead)
instance FromJSON DemandPaging where
  parseJSON = withObject "demand paging" $ \o -> do
    loaded <- o .: "load"
    DemandPaging <$> loaded .: "context" <*> loaded .: "target" <*> loaded .: "core_instructions"
      <*> o .: "abort" <*> o .: "core_instructions" <*> o .: "spsr" <*> o .: "ifar" <*> o .: "ifsr" <*> o .:? "service"
data LockEvent = LockEvent { lockTask :: Word32, lockCore :: Int, lockCall :: Word64, lockAt :: Word64, lockAddress :: Address, lockValue :: Word32 } deriving (Eq,Show)
instance FromJSON LockEvent where
  parseJSON = withObject "lock event" $ \e -> LockEvent <$> e .: "task" <*> (e .: "core" >>= coreOf) <*> e .: "call" <*> e .: "at"
    <*> (Address <$> (e .: "address" >>= parseHex)) <*> e .: "value"
data Observation = Observation { obsRole :: Role, obsInputs :: [String], obsInstructions :: Word64, obsEntries :: Int, obsReturns :: Int,
  obsUntracked :: Int, obsUnarmed :: Int, obsFill :: Maybe Value, obsArmings :: [Q.Arming], obsSegments :: [Q.Segment], obsFinal :: Maybe Q.Readback,
  obsCalls :: [Call], obsLockEvents :: [LockEvent], obsPending :: ([F.Pending],[Word64]), obsMilestones :: [Q.Milestone], obsError :: Maybe T.Text, obsMachineError :: Maybe T.Text,
  obsBankCounts :: BankCounts, obsPcWatches :: [PcWatch], obsDemandPaging :: [DemandPaging] } deriving (Eq,Show)
data BudgetKind = TimerWitnessBudget | DeferredCallBudget | ControlBudget deriving (Eq,Show)
data Refusal = Binding | Arming | Readback | Watch | FillFetch | Incomplete | Untracked | Unarmed | WrongCore | CallOutsidePlan | EntryUnreadable | Model ModelFailure | ReturnValue | ProcessorState | MemoryResult | WrittenWords | BankWrites | QueueInvariant | Route | ObserverError | UnsupportedSemantic Semantic | MissingTrace | CompletionMismatch | DispatchModel D.DispatchFailure | DemandPagingState
  | BudgetExceeded BudgetKind Int Int
  | FamilyReturn | FamilyEffect | FamilyLifetime | FamilyTimer | FamilyDestructor | FamilyContinuation | FamilyAssociation deriving (Eq,Show)

data Completion = Returned | Discarded | Dispatched deriving (Eq,Show)
data BoundaryKind = SaveBoundary | ResumeBoundary | NoSaveBoundary | RetryBoundary | CallbackBoundary | CallbackReturnBoundary | SelectedBoundary | DiscardedBoundary | EnabledBoundary | GlobalLockBoundary | AcquiredBoundary | SelectCompleteBoundary | FirstEntryBoundary | IrqTailBoundary | ReturnBoundary | PrefetchAbortBoundary | TimerInsertBoundary | TimerRemoveBoundary | TimerConsumeBoundary deriving (Eq,Show)
data Boundary = Boundary { boundaryKind :: BoundaryKind, boundaryAddress :: Address, boundaryInstructions :: Word64, boundaryCore :: Int, boundaryRegisters :: D.Registers, boundaryMemory :: Memory, boundaryWrites :: [Written], boundaryBank :: [Written], boundaryValue :: Maybe Word32, boundaryTask :: Word32,
  boundaryWriteOrigins :: [WriteOrigin], boundaryBankOrigins :: [WriteOrigin], boundaryUnreadable :: S.Set Address } deriving (Eq,Show)
data CallTrace = CallTrace { traceBefore :: D.Registers, traceAfter :: D.Registers, traceCompletion :: Completion, traceHistory :: [Boundary], traceOrigin :: Maybe DispatchPoint } deriving (Eq,Show)

registersOf :: J.Object -> Parser D.Registers
registersOf o = do
  values <- o .: "registers"
  unless (length values==16) (fail "register snapshot must contain r0-r15")
  D.Registers values <$> o .: "cpsr" <*> o .: "tpidrurw" <*> o .: "cpacr" <*> o .: "fpexc"
instance FromJSON BoundaryKind where
  parseJSON = J.withText "boundary kind" $ \kind -> case kind of
    "save" -> pure SaveBoundary
    "resume" -> pure ResumeBoundary
    "no-save" -> pure NoSaveBoundary
    "retry" -> pure RetryBoundary
    "callback" -> pure CallbackBoundary
    "callback-return" -> pure CallbackReturnBoundary
    "selected-read" -> pure SelectedBoundary
    "discarded-read" -> pure DiscardedBoundary
    "enabled-read" -> pure EnabledBoundary
    "global-lock-read" -> pure GlobalLockBoundary
    "select-complete" -> pure SelectCompleteBoundary
    "first-entry-exit" -> pure FirstEntryBoundary
    "acquired" -> pure AcquiredBoundary
    "irq-tail" -> pure IrqTailBoundary
    "return" -> pure ReturnBoundary
    "prefetch-abort" -> pure PrefetchAbortBoundary
    "timer-insert" -> pure TimerInsertBoundary
    "timer-remove" -> pure TimerRemoveBoundary
    "timer-consume" -> pure TimerConsumeBoundary
    _ -> fail "unsupported scheduler boundary"
instance FromJSON Completion where
  parseJSON = J.withText "completion" $ \kind -> case kind of
    "return" -> pure Returned
    "discard" -> pure Discarded
    "dispatched" -> pure Dispatched
    _ -> fail "unsupported scheduler completion"
instance FromJSON Boundary where
  parseJSON = withObject "boundary" $ \o -> do
    memory <- o .: "memory" >>= memoryOf
    Boundary <$> o .: "kind" <*> (Address <$> (o .: "address" >>= parseHex))
      <*> o .: "instructions" <*> (o .: "core" >>= coreOf) <*> registersOf o <*> pure (fst memory)
      <*> (o .: "writes" >>= mapM writtenOf) <*> (o .: "bank" >>= mapM writtenOf) <*> o .:? "value" <*> (o .: "task" >>= wordRead)
      <*> (o .: "writes" >>= originsOf) <*> (o .: "bank" >>= originsOf) <*> pure (snd memory)

coreOf :: Value -> Parser Int
coreOf = J.withText "core" $ \core -> case core of "primary" -> pure 0; "secondary" -> pure 1; _ -> fail "unknown core"
wordRead :: Value -> Parser Word32
wordRead = withObject "read" $ \o -> do
  view <- o .: "view"
  unless (view == ("committed"::String)) (fail ("EntryUnreadable: " ++ view))
  o .: "value"
memoryOf :: Value -> Parser (Memory, S.Set Address)
memoryOf = J.withArray "memory" $ \rows -> do
  pairs <- mapM (withObject "word" $ \o -> do
    address <- Address <$> (o .: "address" >>= parseHex)
    word <- o .: "value"
    view <- withObject "read" (.: "view") word
    if view == ("committed"::String)
      then Left <$> ((,) address <$> withObject "read" (.: "value") word)
      else pure (Right address)) (foldr (:) [] rows)
  let committed = [pair | Left pair <- pairs]
      unreadable = S.fromList [address | Right address <- pairs]
  unless (M.size (M.fromList committed) == length committed) (fail "duplicate word")
  pure (M.fromList committed, unreadable)
writtenOf :: Value -> Parser Written
writtenOf = withObject "write" $ \o -> Written <$> (Address <$> (o .: "address" >>= parseHex)) <*> o .: "size" <*> o .: "value"
instance FromJSON Call where
  parseJSON = withObject "call" $ \o -> do
    e <- o .: "entry"
    r <- o .: "result"
    identity <- e .: "slot"
    d <- identity .: "domain" >>= parseDomain
    address <- Address <$> (identity .: "entry" >>= parseHex)
    semantic <- identity .: "semantic"
    origin <- e .:? "origin" >>= traverse (J.withText "dispatch origin" (named dispatchName . T.unpack))
    trace <- CallTrace <$> registersOf e <*> registersOf r <*> r .: "completion" <*> r .: "history" <*> pure origin
    before <- e .: "memory" >>= memoryOf
    after <- r .: "memory" >>= memoryOf
    let unreadable = S.unions (snd before : snd after : map boundaryUnreadable (traceHistory trace))
    Call d address semantic <$> (e .: "core" >>= coreOf) <*> e .: "cpsr" <*> (Arguments <$> e .: "arguments")
      <*> pure (fst before) <*> pure (fst after) <*> r .: "r0" <*> r .: "cpsr"
      <*> (r .: "writes" >>= mapM writtenOf) <*> (r .: "bank" >>= mapM writtenOf) <*> o .: "count" <*> o .: "first" <*> pure (Just trace) <*> (e .: "task" >>= wordRead)
      <*> e .: "kernel_context" <*> e .:? "task_stack" <*> (r .: "writes" >>= originsOf) <*> (r .: "bank" >>= originsOf) <*> pure unreadable
instance FromJSON Observation where
  parseJSON = withObject "scheduler observation" $ \o -> do
    schema <- o .: "schema"
    unless (schema == (3::Int)) (fail "observation schema differs")
    role <- o .: "role" >>= either fail pure . parseRole
    inputs <- o .: "inputs" >>= mapM (withObject "binding" (.: "sha256"))
    let count name = o .: name >>= withObject "count" (.: "count")
    armings <- o .: "armings" >>= mapM (withObject "arming" $ \a -> Q.Arming <$> (a .: "domain" >>= parseDomain) <*> (a .: "readback" >>= Q.readbackOf))
    segments <- o .: "segments" >>= mapM (withObject "segment" $ \s -> do
      state <- s .: "state"
      rows <- state .: "records" :: Parser [Value]
      classes <- state .: "classes" >>= mapM (withObject "class" $ \c -> do
        bounds <- c .: "range"
        case bounds of [lo,hi] -> (,,) lo hi <$> c .: "kinds"; _ -> fail "invalid class range")
      Q.Segment <$> (s .: "domains" >>= mapM parseDomain) <*> s .: "started" <*> s .: "ended" <*> pure (length rows) <*> state .:? "violation" <*> pure classes)
    final <- o .:? "final_readback" >>= traverse Q.readbackOf
    lockEvents <- o .:? "lock_events" >>= maybe (pure []) (mapM parseJSON)
    pendingRows <- o .: "pending" >>= mapM pendingOf
    let (familyPending, otherPending) = partitionEithers pendingRows
    let pending = (familyPending, otherPending)
    milestones <- o .: "milestones" >>= mapM (withObject "milestone" $ \m -> do
      pulse <- m .:? "pulse" >>= traverse (withObject "pulse" (.: "key"))
      let menu = withObject "menu" $ \v -> Q.MenuTriple <$> v .: "menu" <*> v .: "setting" <*> v .: "cursor"
      reached <- m .:? "reached" :: Parser (Maybe Word64)
      Q.Milestone <$> m .: "phase" <*> pure pulse <*> (m .: "expected" >>= menu) <*> (m .:? "observed" >>= traverse menu) <*> pure (reached /= Nothing) <*> m .: "finished")
    watches <- o .: "pc_watches" >>= mapM (withObject "PC watch" $ \w -> PcWatch <$> (w .: "core" >>= coreOf) <*> (Address <$> (w .: "address" >>= parseHex)) <*> w .: "count")
    Observation role inputs <$> o .: "instructions" <*> o .: "entries" <*> o .: "returns" <*> count "untracked" <*> count "unarmed_entries"
      <*> o .:? "fill_fetch" <*> pure armings <*> pure segments <*> pure final <*> o .: "classes" <*> pure lockEvents <*> pure pending <*> pure milestones
      <*> o .:? "error" <*> o .:? "machine_failure" <*> o .: "bank_counts" <*> pure watches <*> o .: "demand_paging"

pendingOf :: Value -> Parser (Either F.Pending Word64)
pendingOf = withObject "pending" $ \p -> do
  entry <- p .: "entry"
  slot <- entry .: "slot"
  semantic <- withObject "slot" (.: "semantic") slot
  entered <- p .: "entered"
  let typed = do
        domain <- withObject "slot" (.: "domain") slot >>= parseDomain
        arguments <- entry .: "arguments"
        before <- entry .: "memory" >>= memoryOf
        task <- entry .: "task" >>= wordRead
        spValue <- entry .: "sp"
        sp <- parseHex spValue
        cpsr <- entry .: "cpsr"
        entryValue <- withObject "slot" (.: "entry") slot
        entryAddress <- parseHex entryValue
        entryCore <- entry .: "core" >>= coreOf
        entryOrder <- entry .: "order"
        terminal <- p .:? "terminal_memory" >>= traverse memoryOf
        history <- p .:? "history" >>= maybe (pure []) (mapM (withObject "boundary" $ \b -> (,,,) <$> b .: "kind" <*> b .: "cpsr" <*> b .: "instructions" <*> b .: "order"))
        let saves = [(savedCpsr,savedAt,savedOrder) | (kind,savedCpsr,savedAt,savedOrder) <- history, kind == ("save" :: T.Text)]
            saveCpsr = case saves of { [] -> Nothing; values -> Just ((\(c,_,_) -> c) (last values)) }
            saveAt = case saves of { [] -> Nothing; values -> Just ((\(_,a,_) -> a) (last values)) }
            saveOrder = case saves of { [] -> Nothing; values -> Just ((\(_,_,o) -> o) (last values)) }
        pure (Left (F.Pending semantic domain arguments (fst before) (fst <$> terminal) task sp cpsr entryAddress entered saveCpsr saveAt saveOrder entryOrder entryCore
          (S.union (snd before) (maybe S.empty snd terminal))))
  case semantic of
    FamilySemantic _ -> typed
    SemaphoreSemantic _ -> typed
    _ -> pure (Right entered)

expect :: Refusal -> Bool -> Either Refusal ()
expect refusal valid = unless valid (Left refusal)

checkDemandPaging :: DemandPaging -> Either Refusal ()
checkDemandPaging demand = do
  let before = demandLoad demand
      after = demandAbort demand
      target = demandTarget demand
      validContext context = drop 13 (demandRegisters context) == [demandSp context,demandLr context,demandPc context]
      sameTask a b = demandCore a==demandCore b && demandTask a==demandTask b
      low pc = pc .&. complement 0x40000000
      loadPc = if demandCore before==0 then 0x43758 else 0xd3758
  expect DemandPagingState (validContext before && validContext after && sameTask before after && demandTask before/=0)
  expect DemandPagingState (low (demandPc before)==loadPc && demandPc after==0xc && demandCpsr after .&. 0x1f==0x17)
  expect DemandPagingState (demandAbortCount demand==demandLoadCount demand+1 && demandInstructions after>demandInstructions before)
  expect DemandPagingState (demandSpsr demand==demandCpsr before && demandSpsr demand .&. 0xff==0x53)
  expect DemandPagingState (demandIfsr demand .&. 0x140f==13 && demandIfar demand==target && demandLr after>=4 && demandLr after-4==target)
  expect DemandPagingState (take 13 (demandRegisters before)==take 13 (demandRegisters after))
  case demandService demand of
    Nothing -> Left DemandPagingState
    Just (DemandService service contextWord (Address root) published) -> do
      let rootBase = if demandCore before==0 then 0x4004dae0 else 0x400ddaa0
      expect DemandPagingState (validContext service && sameTask before service && demandInstructions service>demandInstructions after)
      expect DemandPagingState (contextWord==0 && demandCpsr service .&. 0x9f==0x13)
      expect DemandPagingState (root>=rootBase && root<=rootBase+0x124 && (root-rootBase) `mod` 4==0 && low published==low (demandPc service))

data DemandWitness = DemandWitness
  { witnessSemantic :: !Semantic, witnessCore :: !Int, witnessTask :: !Word32
  , witnessReturned :: !D.Registers, witnessBoundaryCore :: !Int, witnessBoundaryTask :: !Word32
  , witnessInstructions :: !Word64, witnessAddress :: !Address, witnessTarget :: !(Maybe Word32)
  , witnessRegisters :: !D.Registers
  } deriving (Eq,Show)

demandWitnesses :: Call -> [DemandWitness]
demandWitnesses call =
  [DemandWitness (callSemantic call) (callCore call) (callTask call) (traceAfter trace)
    (boundaryCore boundary) (boundaryTask boundary) (boundaryInstructions boundary)
    (boundaryAddress boundary) (boundaryValue boundary) (boundaryRegisters boundary)
  | Just trace <- [callTrace call], boundary <- traceHistory trace, boundaryKind boundary==PrefetchAbortBoundary]

checkDemandCalls :: Observation -> Either Refusal ()
checkDemandCalls observation = checkDemandWitnesses (obsDemandPaging observation) (concatMap demandWitnesses (obsCalls observation))

checkDemandWitnesses :: [DemandPaging] -> [DemandWitness] -> Either Refusal ()
checkDemandWitnesses demands witnessed = do
  mapM_ checkDemandPaging demands
  let matches demand witness =
        witnessSemantic witness==DispatchSemantic FirstEntry && witnessCore witness==demandCore (demandLoad demand)
        && witnessTask witness==demandTask (demandLoad demand) && witnessBoundaryTask witness==witnessTask witness
        && witnessBoundaryCore witness==witnessCore witness && witnessInstructions witness==demandInstructions (demandAbort demand)
        && witnessAddress witness==Address 0xc && witnessTarget witness==Just (demandTarget demand)
        && D.registers (witnessRegisters witness)==demandRegisters (demandAbort demand)
        && D.processorState (witnessRegisters witness)==demandCpsr (demandAbort demand)
        && D.registers (witnessReturned witness)==take 15 (demandRegisters (demandLoad demand))++[demandTarget demand]
        && D.processorState (witnessReturned witness)==demandSpsr demand
  expect DemandPagingState (length witnessed==length demands)
  forM_ demands $ \demand -> expect DemandPagingState (length (filter (matches demand) witnessed)==1)
  forM_ witnessed $ \witness -> expect DemandPagingState (length (filter (\demand -> matches demand witness) demands)==1)

naturalControlMask :: Call -> Word32
naturalControlMask call = if restored then 0xff else 0x1ff
 where
  history = maybe [] traceHistory (callTrace call)
  loader = if callDomain call==DomainA then 0x40000 else 0xd0000
  sp boundary = case drop 13 (D.registers (boundaryRegisters boundary)) of value':_ -> Just value'; _ -> Nothing
  at boundary pc = boundaryAddress boundary==Address pc && drop 15 (D.registers (boundaryRegisters boundary))==[pc]
  frame saved resumed = case (sp saved,sp resumed) of
    (Just savedSp,Just resumedSp) -> resumedSp<=maxBound-36 && resumedSp+36==savedSp
    _ -> False
  restored = or [boundaryCore saved==callCore call && boundaryCore resumed==callCore call
    && callTask call/=0 && boundaryTask saved==callTask call && boundaryTask resumed==callTask call
    && at saved (loader+0x3640) && at resumed (loader+0x3668)
    && boundaryInstructions saved<boundaryInstructions resumed
    && D.processorState (boundaryRegisters saved) .&. 0x9f==0x93
    && D.processorState (boundaryRegisters saved) .&. 0xff==D.processorState (boundaryRegisters resumed) .&. 0xff
    && frame saved resumed
    | (index,saved) <- zip [0::Int ..] history,boundaryKind saved==SaveBoundary
    , resumed <- drop (index+1) history,boundaryKind resumed==ResumeBoundary]

ownsOrigin :: Call -> WriteOrigin -> Bool
ownsOrigin call origin = writerCore origin==callCore call && writerTask origin==Address (callTask call)
  && writerContext origin==callKernelContext call && writerCpsr origin .&. 0x1f==callCpsr call .&. 0x1f
  && inStack && not transition
 where
  inStack = case callTaskStack call of Nothing -> True; Just (TaskStack lo hi) -> lo<=writerSp origin && writerSp origin<=hi
  Address address = writerPc origin
  pc = address .&. complement 0x40000000
  transition = case callSemantic call of
    DispatchSemantic _ -> False
    _ -> any (\loader -> (loader+0x204<=pc && pc<loader+0x368) || (loader+0x3640<=pc && pc<loader+0x3810)) [0x40000,0xd0000]

ownedWrites :: Call -> [Written] -> [WriteOrigin] -> Either Refusal [Written]
ownedWrites call = go
 where
  restored = naturalControlMask call==0xff
  go [] [] = Right []
  go (write:writes) (origin:origins) = do
    let owned = ownsOrigin call origin
    expect MissingTrace (owned || writerCore origin/=callCore call || restored)
    remaining <- go writes origins
    pure (if owned then write:remaining else remaining)
  go _ _ = Left WrittenWords

ownsBoundary :: Call -> Boundary -> Bool
ownsBoundary call boundary = boundaryCore boundary==callCore call && boundaryTask boundary==callTask call
  && D.processorState (boundaryRegisters boundary) .&. 0x1f==callCpsr call .&. 0x1f
  && M.lookup (Address (V.contextWord (V.domainWords (callDomain call)))) (boundaryMemory boundary)==Just (callKernelContext call)
  && case (callTaskStack call,drop 13 (D.registers (boundaryRegisters boundary))) of
    (Nothing,_) -> True
    (Just (TaskStack (Address lo) (Address hi)),sp:_) -> lo<=sp && sp<=hi
    _ -> False
checkHelperCall :: Call -> Either Refusal ()
checkHelperCall = checkHelperCallWith V.PerDomainPredicate

checkHelperCallWith :: V.NotifyPredicate -> Call -> Either Refusal ()
checkHelperCallWith predicate call = do
  expect WrongCore (callCore call == if callDomain call == DomainA then 0 else 1)
  helper <- case callSemantic call of HelperSemantic helper -> pure helper; semantic -> Left (UnsupportedSemantic semantic)
  contract <- either (Left . Model) Right (stockCall (callDomain call) helper (callArguments call) (callBefore call))
  let expected = case (predicate,helper,callArguments call) of
        (V.PrimaryPredicate,Notify,Arguments [owner,_,_,_]) ->
          let (result,bank)=V.notifyWithPredicate predicate (callDomain call) (V.NotificationTarget owner)
          in contract {expectedR0=result,expectedBank=bank}
        _ -> contract
  expect ReturnValue (callR0 call == expectedR0 expected)
  let controlMask = naturalControlMask call
  expect ProcessorState (callCpsr call .&. controlMask == callReturnCpsr call .&. controlMask)
  expect MemoryResult (M.keys (callAfter call) == M.keys (callBefore call))
  writes <- ownedWrites call (callWrites call) (callWriteOrigins call)
  bank <- ownedWrites call (callBank call) (callBankOrigins call)
  let normalized (Address a) = Address (a .&. complement 0x40000000 .&. complement 3)
      written = S.fromList [normalized a | Written a _ _ <- writes]
      wanted = S.map normalized (expectedWrites expected)
      canonical = M.fromList [(normalized address,address) | address <- M.keys (callBefore call)]
      apply memory (Written address@(Address a) size value) = do
        expect WrittenWords (size `elem` [1,2,4] && a `mod` fromIntegral size == 0)
        key <- maybe (Left WrittenWords) Right (M.lookup (normalized address) canonical)
        old <- maybe (Left WrittenWords) Right (M.lookup key memory)
        let shift = fromIntegral (a .&. 3)*8
            mask = if size==4 then maxBound else (1 `shiftL` (size*8))-1
        expect WrittenWords (value .&. complement mask == 0)
        pure (M.insert key ((old .&. complement (mask `shiftL` shift)) .|. (value `shiftL` shift)) memory)
  replayed <- foldM apply (callBefore call) writes
  expect WrittenWords (written == wanted && replayed == expectedMemory expected)
  concurrent <- foldM apply (callBefore call) (callWrites call)
  expect MemoryResult (concurrent == callAfter call)
  expect BankWrites (sort bank == sort [Written address 4 value | (address,value) <- expectedBank expected])
  whenQueue call
 where
  whenQueue c | callSemantic c `elem` map HelperSemantic [Insert,Remove,Prepare,Requeue,Rotate] = do
    let Arguments args = callArguments c
    case args of
      record:_ -> do
        let get a = maybe (Left EntryUnreadable) Right (M.lookup (Address a) (callAfter c))
        heads <- get (record+0x20)
        bitmapAddress <- get (record+0x24)
        bitmap <- get bitmapAddress
        pairs <- mapM (\p -> do first <- get (heads+p*8); pure (p,first /= heads+p*8,first)) [0..15]
        expect QueueInvariant (all (\(p,nonempty,_) -> (bitmap .&. (2^p) /= 0) == nonempty) pairs)
        selectedAddress <- get (record+0x2c)
        selected <- get selectedAddress
        let ready = [first | (_,True,first) <- pairs]
        expect QueueInvariant (selected == case ready of [] -> 0; first:_ -> first)
      _ -> Left EntryUnreadable
  whenQueue _ = pure ()

sharedPlan :: Plan -> P.Plan
sharedPlan plan = P.Plan P.OwnedRole (planDat plan) [] []
  [P.GuardedRange d lo hi digest | Guard d (Address lo) (Address hi) digest <- planGuards plan] []
watchPlan :: Plan -> P.Plan
watchPlan plan = sharedPlan plan {planGuards=filter outermost (planGuards plan)}
 where
  outermost guard = not (any (\outer -> guardDomain outer==guardDomain guard && guardStart outer<=guardStart guard &&
    guardEnd guard<=guardEnd outer && (guardStart outer/=guardStart guard || guardEnd outer/=guardEnd guard)) (planGuards plan))
trajectorySha256 :: String
trajectorySha256 = "43134e99c48715ca58e6838347f164e3458dbd986ad2f854e6788371422f0a8d"
data CallTotals = CallTotals { hasCompletedClasses :: !Bool, completedCallCount :: !Int } deriving (Eq,Show)

qualify :: B.ByteString -> Plan -> Observation -> Either Refusal Value
qualify bytes plan observation = do
  qualifyPrefix bytes plan observation
  checkDemandCalls observation
  let calls = sortOn callFirst (obsCalls observation)
  forM_ calls $ \call -> checkCallEnvelope plan call >> checkCallForIn calls (obsRole observation) call
  let releases = [F.ReleaseWitness (lockTask event) (lockCore event) (lockCall event) (lockAt event) (lockAddress event) (lockValue event)
        | event <- obsLockEvents observation]
  qualifySuffix plan releases observation (CallTotals (not (null calls)) (sum (map callCount calls)))
  pure (summary observation)

qualifyPrefix :: B.ByteString -> Plan -> Observation -> Either Refusal ()
qualifyPrefix bytes plan observation = do
  forM_ (planEntries plan) $ \entry -> expect (UnsupportedSemantic (entrySemantic entry)) (modelled (entrySemantic entry))
  expect Binding (obsRole observation == planRole plan && length (obsInputs observation) == 6 && take 5 (obsInputs observation) ==
    [sha256Hex (sha256 bytes),Q.runDocumentSha256,planDat plan,Q.bootRomSha256,trajectorySha256])
  expect FillFetch (obsFill observation == Nothing)
  expect Untracked (obsUntracked observation == 0)
  expect Unarmed (obsUnarmed observation == 0)
  forM_ (obsArmings observation) $ \arming -> either (const (Left Readback)) Right
    (Q.checkReadback (sharedPlan plan) Q.ArmingReadback (Q.armingReadback arming) (Q.armingDomain arming))
  expect Arming (sort (map Q.armingDomain (obsArmings observation)) == domains)
  final <- maybe (Left Readback) Right (obsFinal observation)
  forM_ domains $ \domain -> either (const (Left Readback)) Right (Q.checkReadback (sharedPlan plan) Q.FinalReadback final domain)
  either (const (Left Watch)) Right (Q.checkSegments (watchPlan plan) (obsArmings observation) final (obsSegments observation))

checkCallEnvelope :: Plan -> Call -> Either Refusal ()
checkCallEnvelope plan call = do
  expect CallOutsidePlan (any (\entry -> entryDomain entry == callDomain call && entryAddress entry == callAddress call && entrySemantic entry == callSemantic call) (planEntries plan))
  expect Incomplete (callCount call > 0)

checkPendingEnvelope :: Plan -> F.Pending -> Either Refusal ()
checkPendingEnvelope plan pending =
  expect CallOutsidePlan (any (\entry -> entryDomain entry == F.pendingDomain pending && entryAddress entry == Address (F.pendingEntry pending) && entrySemantic entry == F.pendingSemantic pending) (planEntries plan))

qualifySuffix :: Plan -> [F.ReleaseWitness] -> Observation -> CallTotals -> Either Refusal ()
qualifySuffix plan releases observation totals = do
  let (familyPending, otherPending) = obsPending observation
  expect Incomplete (hasCompletedClasses totals && obsReturns observation == completedCallCount totals
    && obsEntries observation == obsReturns observation + length familyPending + length otherPending
    && all (\entered -> entered <= obsInstructions observation && obsInstructions observation-entered <= Q.pendingWindow) otherPending)
  forM_ familyPending $ \pending -> do
    checkPendingEnvelope plan pending
    expect Incomplete (F.pendingEntered pending <= obsInstructions observation)
    case F.pendingSemantic pending of
      FamilySemantic _ -> either (Left . familyRefusal) Right (F.checkPendingFamily releases pending)
      SemaphoreSemantic _ -> either (Left . familyRefusal) Right (Sem.checkPendingSemaphore releases pending)
      _ -> Left FamilyLifetime
  expect ObserverError (obsError observation == Nothing && obsMachineError observation == Nothing)
  let milestones = obsMilestones observation
      phases = S.fromList (map Q.milestonePhase milestones)
  expect Route (all (\m -> Q.milestoneReached m && Q.milestoneObserved m == Just (Q.milestoneExpected m)) milestones &&
    all (`S.member` phases) ["cold-start","shooting-menu","modal75-film","modal75-returned","modal75-film-repeated","modal75-returned-repeated","modal75-handback"])

routeSignature :: Observation -> [(T.Text,Maybe T.Text,Q.MenuTriple,Maybe Q.MenuTriple)]
routeSignature = map (\m -> (Q.milestonePhase m,Q.milestoneKey m,Q.milestoneExpected m,Q.milestoneObserved m)) . obsMilestones
compareRoutes :: Observation -> Observation -> Either Refusal ()
compareRoutes stock owned = do
  expect Binding (drop 3 (obsInputs stock) == drop 3 (obsInputs owned))
  expect Route (obsRole stock == Stock && obsRole owned == Owned && routeSignature stock == routeSignature owned)
data SummaryEntry = SummaryEntry !Domain !Address !Semantic deriving (Eq,Ord,Show)
data RedesignCounts = RedesignCounts
  { nullSelectionCalls :: !Int, nullLoopCalls :: !Int, remoteRestartCalls :: !Int } deriving (Eq,Show)
data CoreCallCounts = CoreCallCounts { crossDomainCalls :: !Int, ringingCalls :: !Int } deriving (Eq,Show)
data CallSummary = CallSummary
  { summaryEntries :: !(M.Map SummaryEntry Int), summaryRedesigns :: !RedesignCounts
  , primaryCalls :: !CoreCallCounts, secondaryCalls :: !CoreCallCounts
  } deriving (Eq,Show)

emptyCallSummary :: CallSummary
emptyCallSummary = CallSummary M.empty (RedesignCounts 0 0 0) (CoreCallCounts 0 0) (CoreCallCounts 0 0)

summarizeCall :: Role -> CallSummary -> Call -> CallSummary
summarizeCall role previous call =
  CallSummary (M.insertWith (+) entry count (summaryEntries previous)) redesigned
    (if callCore call==0 then bumpCore (primaryCalls previous) else primaryCalls previous)
    (if callCore call==1 then bumpCore (secondaryCalls previous) else secondaryCalls previous)
 where
  count = callCount call
  entry = SummaryEntry (callDomain call) (callAddress call) (callSemantic call)
  tags = case callSemantic call of
    DispatchSemantic point -> either (const []) id (dispatchTags role point call)
    _ -> []
  old = summaryRedesigns previous
  bump item prior = prior + if item `elem` tags then count else 0
  redesigned = RedesignCounts (bump D.NullSelectionOwnRegisters (nullSelectionCalls old))
    (bump D.NullLoopReleasesGlobalLock (nullLoopCalls old)) (bump D.RemoteTerminateRestarts (remoteRestartCalls old))
  cross = callSemantic call==HelperSemantic Insert && case callArguments call of
    Arguments (record:_) -> M.lookup (Address (record+4)) (callBefore call) /= Just (if callDomain call==DomainA then 1 else 2)
    _ -> False
  rings = callSemantic call==HelperSemantic Notify && any (ownsOrigin call) (callBankOrigins call)
  bumpCore prior = CoreCallCounts (crossDomainCalls prior + if cross then count else 0)
    (ringingCalls prior + if rings then count else 0)

summary :: Observation -> Value
summary observation = summaryWith observation (foldl' (summarizeCall (obsRole observation)) emptyCallSummary (obsCalls observation))

summaryWith :: Observation -> CallSummary -> Value
summaryWith observation counts = object
  [ "role" .= roleName (obsRole observation), "calls" .= obsReturns observation
  , "per_entry" .= [object ["domain" .= domainLabel domain,"entry" .= hex address,"semantic" .= semanticValue semantic,"calls" .= count]
      | (SummaryEntry domain (Address address) semantic,count) <- M.toList (summaryEntries counts)]
  , "redesign_items" .= [object ["id" .= D.redesignName item,"calls" .= count]
      | (item,count) <- [(D.NullSelectionOwnRegisters,nullSelectionCalls redesigned),
          (D.NullLoopReleasesGlobalLock,nullLoopCalls redesigned),(D.RemoteTerminateRestarts,remoteRestartCalls redesigned)]]
  , "demand_paging_transfers" .= length (obsDemandPaging observation)
  , "per_core" .= map metrics [0,1::Int] ]
 where
  redesigned = summaryRedesigns counts
  metrics core = object ["core" .= core,"cross_domain_inserts" .= crossDomainCalls selected,"ringing_notifies" .= ringingCalls selected,
    "bank_writes" .= (if core==0 then primaryBankWrites (obsBankCounts observation) else secondaryBankWrites (obsBankCounts observation)),
    "doorbell_handler_runs" .= sum [n | PcWatch c (Address pc) n <- obsPcWatches observation,c==core,pc `elem` [0x40b00,0xd0b00]],
    "pc_watches" .= [object ["address" .= hex pc,"count" .= sum [n | PcWatch c address n <- obsPcWatches observation,c==core,address==Address pc]]
      | pc <- [0x436ec,0xd36ec,0xec8014,0x40b00,0xd0b00]]]
   where
    selected = if core==0 then primaryCalls counts else secondaryCalls counts

firstSemanticFailure :: Observation -> Maybe (Call,Refusal)
firstSemanticFailure observation = case [(call,refusal) | call <- sortOn callFirst (obsCalls observation),Left refusal <- [checkCallFor (obsRole observation) call]] of
  first:_ -> Just first
  [] -> Nothing

callerFramePreserved :: Call -> Bool
callerFramePreserved call = case callTrace call of
  Nothing -> False
  Just trace -> same 8 (drop 4 (D.registers (traceBefore trace))) (drop 4 (D.registers (traceAfter trace)))
 where
  same :: Int -> [Word32] -> [Word32] -> Bool
  same 0 (_:sp:lr:_:[]) (_:returnedSp:_:pc:[]) =
    sp==returnedSp && lr .&. complement 0x40000000==pc .&. complement 0x40000000
  same remaining (before:rest) (after:following)
    | remaining>0 = before==after && same (remaining-1) rest following
  same _ _ _ = False

controlRefusals :: Role -> [Refusal]
controlRefusals role = case role of
  Restored -> [Readback]
  SilentNotify -> [BankWrites]
  WrongPredicate -> [ReturnValue,BankWrites,ProcessorState]
  WakeNoInsert -> [MemoryResult,WrittenWords,ReturnValue]
  _ -> []

controlTarget :: Role -> Call -> Bool
controlTarget role call = case role of
  SilentNotify -> callSemantic call==HelperSemantic Notify && case stockCall (callDomain call) Notify (callArguments call) M.empty of
    Right expected -> not (null (expectedBank expected))
    Left _ -> False
  WrongPredicate -> callDomain call==DomainB && callSemantic call==HelperSemantic Notify
    && case callArguments call of Arguments (1:_) -> True; _ -> False
  WakeNoInsert -> callSemantic call==ServiceSemantic WakeTask
  _ -> False

controlFailure :: Role -> Observation -> Either Refusal ()
controlFailure role observation = case role of
  SilentNotify -> firstTarget (controlTarget role) BankWrites
  WrongPredicate -> firstTarget (controlTarget role) ReturnValue
  WakeNoInsert -> case firstSemanticFailure observation of
    Just (call,refusal) -> expect MemoryResult (callSemantic call==ServiceSemantic WakeTask && refusal `elem` [MemoryResult,WrittenWords,ReturnValue])
    Nothing -> Left Incomplete
  _ -> pure ()
 where
  ordered = sortOn callFirst (obsCalls observation)
  firstTarget predicate refusal = case filter predicate ordered of
    target:_ -> do
      expect refusal (checkCallFor (obsRole observation) target==Left refusal)
      expect refusal (callerFramePreserved target)
      case role of
        WrongPredicate -> expect refusal (checkHelperCallWith V.PrimaryPredicate target==Right ())
        _ -> pure ()
      let missing = case stockCall (callDomain target) Notify (callArguments target) M.empty of
            Right expected -> case expectedBank expected of [(address,value)] -> Just (Written address 4 value); _ -> Nothing
            Left _ -> Nothing
      forM_ (takeWhile ((<=callFirst target) . callFirst) ordered) $ \call ->
        unless (call==target) $ case checkCallFor (obsRole observation) call of
          Right () -> pure ()
          Left actual -> expect refusal (actual `elem` controlRefusals role && maybe False (\transaction -> enclosesNotifyDefect target transaction call) missing)
    _ -> Left Incomplete
  enclosesNotifyDefect target transaction call = case (callSemantic call,callTrace call) of
    (ServiceSemantic service,Just trace) ->
      let controlMask=naturalControlMask call
      in callTask call/=0 && callTask call==callTask target && callCore call==callCore target
      && callDomain call==callDomain target && callKernelContext call==callKernelContext target
      && callCpsr call .&. 0x1f==callCpsr target .&. 0x1f && callFirst call<callFirst target
      && callerFramePreserved call
      && callCpsr call .&. controlMask==callReturnCpsr call .&. controlMask
      && (role/=WrongPredicate || controlMask==0xff)
      && all (not . ownsOrigin call) (callBankOrigins call)
      && all (all (not . ownsOrigin call) . boundaryBankOrigins) (traceHistory trace)
      && any (\boundary -> boundaryKind boundary==ReturnBoundary && ownsBoundary call boundary
        && boundaryInstructions boundary>callFirst target
        && case boundaryAddress boundary of Address pc -> drop 15 (D.registers (boundaryRegisters boundary))==[pc]) (traceHistory trace)
      && checkServiceCallWith (if role==WrongPredicate then WrongNotifyService else MissingNotifyAtReturn transaction) service call==Right ()
    _ -> False

failureValue :: Observation -> Value
failureValue observation = case firstSemanticFailure observation of
  Nothing -> J.Null
  Just (call,refusal) -> object ["first" .= callFirst call,"domain" .= domainLabel (callDomain call),"entry" .= show (callAddress call),"semantic" .= semanticValue (callSemantic call),
    "core" .= callCore call,"refusal" .= show refusal]

modelled :: Semantic -> Bool
modelled semantic = case semantic of
  HelperSemantic _ -> True
  DispatchSemantic _ -> True
  DoorbellSemantic -> True
  FamilySemantic _ -> True
  SemaphoreSemantic _ -> True
  ServiceSemantic service -> A.supportsService service || C.supportsService service || E.supportsService service

checkCall :: Call -> Either Refusal ()
checkCall = checkCallFor Owned

checkUnreadableWrites :: Call -> Either Refusal ()
checkUnreadableWrites call = forM_ (callWrites call) $ \(Written (Address address) _ _) ->
  let aligned = Address (address .&. complement 3)
      inStack = case callTaskStack call of
        Just (TaskStack (Address lo) (Address hi)) -> lo<=address && address<hi
        Nothing -> False
  in expect EntryUnreadable (inStack || not (S.member aligned (callUnreadable call)))

-- | Remove an unreadable word and its alias from a fallback snapshot so a
-- required acquisition read cannot borrow an older committed value.
maskUnreadable :: S.Set Address -> Memory -> Memory
maskUnreadable unreadable memory = foldr remove memory (S.toList unreadable)
 where
  remove (Address address) = M.delete (Address (address .&. complement 0x40000000 .&. complement 3)) . M.delete (Address address)

checkCallFor :: Role -> Call -> Either Refusal ()
checkCallFor = checkCallForInWitnesses []

checkCallForIn :: [Call] -> Role -> Call -> Either Refusal ()
checkCallForIn calls = checkCallForInWitnesses (timerWitnesses calls)

checkCallForInWitnesses :: [F.TimerWitness] -> Role -> Call -> Either Refusal ()
checkCallForInWitnesses witnesses role call = do
  checkUnreadableWrites call
  case callSemantic call of
    FamilySemantic _ -> pure ()
    _ -> expect WrongCore (callCore call == if callDomain call==DomainA then 0 else 1)
  case callSemantic call of
    HelperSemantic _ -> checkHelperCall call
    DoorbellSemantic -> do
      bank <- ownedWrites call (callBank call) (callBankOrigins call)
      writes <- ownedWrites call (callWrites call) (callWriteOrigins call)
      either (Left . DispatchModel) Right
        (D.checkDoorbell (callDomain call) (callAfter call) [(address,value) | Written address size value <- bank,size==4])
      let (_,_,reschedule,_,_) = D.stateAddresses (callDomain call)
      expect WrittenWords (any (\(Written (Address address) size value) -> address .&. complement 0x40000000==reschedule .&. complement 0x40000000 && size==4 && value==1) writes)
      expect ProcessorState (callCpsr call .&. 0x1ff==callReturnCpsr call .&. 0x1ff)
    DispatchSemantic point -> dispatchTags role point call >> pure ()
    FamilySemantic _ | role == Stock -> pure ()
    FamilySemantic _ -> checkFamilyCall call witnesses
    SemaphoreSemantic _ | role == Stock -> pure ()
    SemaphoreSemantic _ -> checkSemaphoreCall role call witnesses
    ServiceSemantic service -> checkServiceCall service call

timerWitnesses :: [Call] -> [F.TimerWitness]
timerWitnesses calls =
  [ F.TimerWitness entry task (callFirst call) (boundaryInstructions boundary) kind
  | call <- calls
  , boundary <- maybe [] traceHistory (callTrace call)
  , (kind, entry) <- case boundaryKind boundary of
      TimerInsertBoundary -> [(F.TimerInserted, value) | Just value <- [boundaryValue boundary]]
      TimerRemoveBoundary -> [(F.TimerRemoved, value) | Just value <- [boundaryValue boundary]]
      TimerConsumeBoundary -> [(F.TimerConsumed, value) | Just value <- [boundaryValue boundary]]
      _ -> []
  , let task = case kind of
          F.TimerConsumed -> maybe (boundaryTask boundary) id (M.lookup (Address (entry+8)) (boundaryMemory boundary))
          F.TimerRemoved -> maybe (boundaryTask boundary) id (M.lookup (Address (entry+8)) (boundaryMemory boundary))
          _ -> boundaryTask boundary
  ]
checkFamilyCall :: Call -> [F.TimerWitness] -> Either Refusal ()
checkFamilyCall call witnesses = case F.checkFamilyCall call witnesses of
  Right () -> Right ()
  Left refusal -> Left (familyRefusal refusal)

checkSemaphoreCall :: Role -> Call -> [F.TimerWitness] -> Either Refusal ()
checkSemaphoreCall _role call witnesses = case Sem.checkSemaphoreCall call witnesses of
  Right () -> Right ()
  Left refusal -> Left (familyRefusal refusal)

familyRefusal :: F.FamilyRefusal -> Refusal
familyRefusal F.FamilyReturn = FamilyReturn
familyRefusal F.FamilyEffect = FamilyEffect
familyRefusal F.FamilyLifetime = FamilyLifetime
familyRefusal F.FamilyTimer = FamilyTimer
familyRefusal F.FamilyDestructor = FamilyDestructor
familyRefusal F.FamilyContinuation = FamilyContinuation
familyRefusal F.FamilyAssociation = FamilyAssociation

instance Sem.SemaphoreCall Call where
  semaphoreDomain = callDomain
  semaphoreCore = callCore
  semaphoreArguments call = case callArguments call of Arguments arguments -> arguments
  semaphoreBefore = callBefore
  semaphoreAfter = callAfter
  semaphoreR0 = callR0
  semaphoreCpsr = callCpsr
  semaphoreWrites call = [(address,size,value) | Written address size value <- callWrites call]
  semaphoreTask = callTask
  semaphoreEpoch = callFirst
  semaphoreService call = case callSemantic call of SemaphoreSemantic service -> Just service; _ -> Nothing
  semaphoreTrace call = fmap familyTraceOf (callTrace call)
  semaphoreWriters call = [if ownsOrigin call origin then Sem.SignalCaller else Sem.OtherWriter | origin <- callWriteOrigins call]
  semaphoreSuspension call = do
    trace <- callTrace call
    let (prefix, rest) = break (\boundary -> boundaryKind boundary == SaveBoundary && ownsBoundary call boundary) (traceHistory trace)
    case rest of
      saved : _ -> Just (boundaryMemory saved,
        [(address, size, value) | boundary <- prefix ++ [saved], Written address size value <- boundaryWrites boundary])
      [] -> Nothing

instance F.FamilyCall Call where
  familyDomain = callDomain
  familyCore = callCore
  familyArguments call = case callArguments call of Arguments arguments -> arguments
  familyBefore = callBefore
  familyAfter = callAfter
  familyR0 = callR0
  familyWrites call = [(address,size,value) | Written address size value <- callWrites call]
  familyTask = callTask
  familyEpoch = callFirst
  familyService call = case callSemantic call of FamilySemantic service -> Just service; _ -> Nothing
  familyTrace call = fmap familyTraceOf (callTrace call)

familyTraceOf :: CallTrace -> F.FamilyTrace
familyTraceOf trace = F.FamilyTrace
  { F.familyTraceCompletion = case traceCompletion trace of
      Returned -> F.FamilyReturnedCompletion
      Discarded -> F.FamilyDiscardedCompletion
      Dispatched -> F.FamilyDispatchedCompletion
  , F.familyTraceBefore = D.registers (traceBefore trace)
  , F.familyTraceAfter = D.registers (traceAfter trace)
  , F.familyTraceBeforeCpsr = D.processorState (traceBefore trace)
  , F.familyTraceAfterCpsr = D.processorState (traceAfter trace)
  , F.familyTraceHistory = map familyBoundaryOf (traceHistory trace)
  }

familyBoundaryOf :: Boundary -> F.FamilyBoundary
familyBoundaryOf boundary = F.FamilyBoundary
  { F.familyBoundaryKind = case boundaryKind boundary of
      SaveBoundary -> F.FamilySaveKind
      ResumeBoundary -> F.FamilyResumeKind
      ReturnBoundary -> F.FamilyReturnKind
      TimerInsertBoundary -> F.FamilyTimerInsertKind
      TimerRemoveBoundary -> F.FamilyTimerRemoveKind
      TimerConsumeBoundary -> F.FamilyTimerConsumeKind
      _ -> F.FamilyOtherKind
  , F.familyBoundaryAddress = boundaryAddress boundary
  , F.familyBoundaryInstructions = boundaryInstructions boundary
  , F.familyBoundaryCore = boundaryCore boundary
  , F.familyBoundaryRegisters = D.registers (boundaryRegisters boundary)
  , F.familyBoundaryMemory = boundaryMemory boundary
  , F.familyBoundaryValue = boundaryValue boundary
  , F.familyBoundaryTask = boundaryTask boundary
  }

dispatchTags :: Role -> DispatchPoint -> Call -> Either Refusal [D.Redesign]
dispatchTags role point call = do
  trace <- maybe (Left MissingTrace) Right (callTrace call)
  let history=traceHistory trace
      value kind = case reverse [value' | boundary <- history,boundaryKind boundary==kind,Just value' <- [boundaryValue boundary]] of [] -> Nothing; value':_ -> Just value'
      initial=traceBefore trace
      final=traceAfter trace
      (_,_,_,loaderBase,_) = D.stateAddresses (callDomain call)
      pc = case drop 15 (D.registers final) of address:_ -> address .&. complement 0x40000000; _ -> 0
      selected = value SelectedBoundary
      dest
        | pc==loaderBase+(if role==Stock then 0x36ac else 0x36a4) = D.SelectStage
        | pc==loaderBase+0x37a8 = D.ExceptionTail
        | point==FirstEntry = D.TaskEntry
        | point==Select && selected==Just 0 = D.NullWindow
        | point==Select = D.SelectedTask
        | pc==loaderBase+(if callDomain call==DomainA then 0x4288 else 0x4248) = D.ExceptionDelivery
        | otherwise = D.CallerReturn
      values=D.Loaded selected (value DiscardedBoundary) (value EnabledBoundary) (value GlobalLockBoundary) (maybe point id (traceOrigin trace))
      event=D.DispatchEvent (callDomain call) point (if role==Stock then D.StockDispatch else D.OwnedDispatch) initial final (callBefore call) (callAfter call) values dest
  either (Left . DispatchModel) Right (D.checkDispatch event)

data ServiceCheckPolicy = StockService | MissingNotifyAtReturn Written | WrongNotifyService

checkServiceCall :: Service -> Call -> Either Refusal ()
checkServiceCall = checkServiceCallWith StockService

checkServiceCallWith :: ServiceCheckPolicy -> Service -> Call -> Either Refusal ()
checkServiceCallWith policy service call = do
  trace <- maybe (Left MissingTrace) Right (callTrace call)
  let domain=callDomain call
      initial = case policy of
        WrongNotifyService -> C.serviceCallWithNotify (C.PredicateNotify (V.NotificationTarget 1) V.PrimaryPredicate)
          domain service (callCpsr call) (callArguments call)
        _ -> serviceModel domain service (callCpsr call) (callArguments call)
      controlMask = naturalControlMask call
      significant boundary = boundaryKind boundary `elem` [SaveBoundary,NoSaveBoundary,RetryBoundary,CallbackBoundary,ReturnBoundary] && ownsBoundary call boundary
      seek wanted boundaries = case dropWhile (\boundary -> boundaryKind boundary/=wanted || not (ownsBoundary call boundary)) boundaries of
        [] -> Left CompletionMismatch
        boundary:rest -> Right (boundary,rest)
      loop predicted previous boundaries = do
        let (prefix,rest)=break significant boundaries
        atAcquisition <- case reverse [boundary | boundary <- prefix,boundaryKind boundary==AcquiredBoundary,ownsBoundary call boundary] of
          [] -> Right previous
          acquired:_ -> do
            acquiredWrites <- ownedWrites call (boundaryWrites acquired) (boundaryWriteOrigins acquired)
            let memory=maskUnreadable (boundaryUnreadable acquired) (M.union (boundaryMemory acquired) previous)
                words'=V.domainWords domain
                records=V.contextTable words':[V.contextTable words'+0x84] ++
                  [record | Just record <- [M.lookup (Address (V.currentContext words')) memory]]
                locks=S.fromList [Address lock | record <- records,Just lock <- [M.lookup (Address (record+8)) memory]]
                taken=[address | Written address 4 1 <- acquiredWrites,S.member address locks]
            case taken of
              [address] -> Right (M.insert address 0 memory)
              _ -> Left CompletionMismatch
        expected <- either (Left . Model) Right (predicted atAcquisition)
        (boundary,remaining) <- case rest of [] -> Left CompletionMismatch; boundary:remaining -> Right (boundary,remaining)
        let state=boundaryRegisters boundary
            args=take 4 (D.registers state)
            rawWrites=concatMap boundaryWrites (prefix++[boundary])
            actualKind=boundaryKind boundary
            wanted=V.serviceExpected expected
            getReg index = maybe (Left MissingTrace) Right (case drop index (D.registers state) of value':_ -> Just value'; _ -> Nothing)
        writes <- concat <$> mapM (\event -> ownedWrites call (boundaryWrites event) (boundaryWriteOrigins event)) (prefix++[boundary])
        bank <- concat <$> mapM (\event -> ownedWrites call (boundaryBank event) (boundaryBankOrigins event)) (prefix++[boundary])
        expect WrongCore (boundaryCore boundary==callCore call)
        expect ProcessorState (D.processorState state .&. controlMask==V.serviceCpsr expected .&. controlMask)
        forM_ (V.serviceR0 expected) $ \result -> getReg 0 >>= expect ReturnValue . (==result)
        let normalize (Address address)=Address (address .&. complement 0x40000000 .&. complement 3)
            scratch=[(normalize (Address start),normalize (Address (min (start+size) sp)))
              | Just task <- [M.lookup (Address (V.runningCell (V.domainWords domain))) (callBefore call)]
              , Just config <- [M.lookup (Address (task+8)) (callBefore call)]
              , Just size <- [M.lookup (Address (config+0x10)) (callBefore call)]
              , Just start <- [M.lookup (Address (config+0x14)) (callBefore call)]
              , sp <- take 1 (drop 13 (D.registers (traceBefore trace)))]
            inScratch address = any (\(lo,hi) -> lo<=address && address<hi) scratch
            known=M.fromList ([(normalize address,address) | address <- M.keys atAcquisition,
              S.member address (expectedWrites wanted) || not (inScratch (normalize address))]
              ++ [(normalize address,address) | address <- S.toList (callUnreadable call), not (inScratch (normalize address))])
            actual=S.fromList [normalize address | Written address _ _ <- writes,M.member (normalize address) known]
            apply memory (Written address@(Address raw) size value') = case M.lookup (normalize address) known of
              Nothing -> Right memory
              Just key -> do
                expect WrittenWords (size `elem` [1,2,4] && raw `mod` fromIntegral size==0)
                old <- maybe (Left WrittenWords) Right (M.lookup key memory)
                let shift=fromIntegral (raw .&. 3)*8
                    mask=if size==4 then maxBound else (1 `shiftL` (size*8))-1
                expect WrittenWords (value' .&. complement mask==0)
                pure (M.insert key ((old .&. complement (mask `shiftL` shift)) .|. (value' `shiftL` shift)) memory)
        replayed <- foldM apply atAcquisition writes
        expect WrittenWords (S.map normalize (expectedWrites wanted)==actual)
        expect MemoryResult (all (\address -> M.lookup address replayed==M.lookup address (expectedMemory wanted)) (S.toList (expectedWrites wanted)))
        case (policy,actualKind,bank,expectedBank wanted) of
          (MissingNotifyAtReturn (Written address 4 value'),ReturnBoundary,[],[(wantedAddress,wantedValue)])
            | address==wantedAddress && value'==wantedValue -> pure ()
          _ -> expect BankWrites (sort bank==sort [Written address 4 value' | (address,value') <- expectedBank wanted])
        concurrent <- foldM apply previous rawWrites
        expect MemoryResult (all (\address -> M.lookup address concurrent==M.lookup address (boundaryMemory boundary)) (M.elems known))
        case V.serviceCompletion expected of
          V.ServiceReturn -> expect CompletionMismatch (actualKind==ReturnBoundary && traceCompletion trace==Returned && boundaryMemory boundary==callAfter call)
          V.ServiceDispatch point reason -> do
            expect CompletionMismatch (actualKind==if point==Save then SaveBoundary else NoSaveBoundary)
            getReg 0 >>= expect ReturnValue . (==reason)
            if point==NoSave then expect CompletionMismatch (traceCompletion trace==Discarded)
            else if traceCompletion trace==Discarded then
              expect CompletionMismatch (any (\witness -> boundaryKind witness==DiscardedBoundary && boundaryValue witness==Just 1) remaining)
            else do
              (resume,following) <- seek ResumeBoundary remaining
              loop (resumeModel domain service (D.processorState (boundaryRegisters resume)) (callArguments call)) (boundaryMemory resume) following
          V.ServiceRetry -> do
            expect CompletionMismatch (actualKind==RetryBoundary)
            loop (serviceModel domain service (D.processorState state) (callArguments call)) (boundaryMemory boundary) remaining
          V.ServiceCallback target arguments -> do
            expect CompletionMismatch (actualKind==CallbackBoundary && args==take 4 (arguments++drop (length arguments) args))
            getReg 15 >>= expect CompletionMismatch . (==target)
            (returned,following) <- seek CallbackReturnBoundary remaining
            loop (resumeModel domain service (D.processorState (boundaryRegisters returned)) (callArguments call)) (boundaryMemory returned) following
  loop initial (callBefore call) (traceHistory trace)

serviceModel :: Domain -> Service -> Word32 -> Arguments -> Memory -> Either ModelFailure V.ServiceExpected
serviceModel domain service
  | A.supportsService service = A.serviceCall domain service
  | C.supportsService service = C.serviceCall domain service
  | E.supportsService service = E.serviceCall domain service
  | otherwise = \_ _ _ -> Left InvalidArguments

resumeModel :: Domain -> Service -> Word32 -> Arguments -> Memory -> Either ModelFailure V.ServiceExpected
resumeModel domain service
  | A.supportsService service = A.resumeService domain service
  | C.supportsService service = C.resumeService domain service
  | E.supportsService service = E.resumeService domain service
  | otherwise = \_ _ _ -> Left InvalidArguments
