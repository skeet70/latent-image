{-# LANGUAGE OverloadedStrings #-}
module Emulation.SchedulerPairCheck where

import Control.Monad (foldM, forM_, unless)
import Data.Aeson (FromJSON (..), Value, object, withObject, (.:), (.:?), (.=))
import qualified Data.Aeson as J
import Data.Aeson.Types (Parser)
import Data.Bits ((.&.), (.|.), complement, shiftL)
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
import Emulation.Sha256 (sha256, sha256Hex)
import Replacement.RtosOwnFirst.Regions (Domain (..), domains, domainLabel)
import Replacement.RtosScheduler.Regions (Helper (..))

data Written = Written Address Int Word32 deriving (Eq,Ord,Show)
data Call = Call { callDomain :: Domain, callAddress :: Address, callSemantic :: Semantic, callCore :: Int, callCpsr :: Word32, callArguments :: Arguments,
  callBefore :: Memory, callAfter :: Memory, callR0 :: Word32, callReturnCpsr :: Word32, callWrites :: [Written], callBank :: [Written], callCount :: Int, callFirst :: Word64, callTrace :: Maybe CallTrace } deriving (Eq,Show)
data PcWatch = PcWatch Int Address Word64 deriving (Eq,Show)
data BankCounts = BankCounts { primaryBankWrites :: Word64, secondaryBankWrites :: Word64 } deriving (Eq,Show)
instance FromJSON BankCounts where
  parseJSON value = do
    values <- J.parseJSON value
    case values of [primary,secondary] -> pure (BankCounts primary secondary); _ -> fail "bank counts must name both cores"
data Observation = Observation { obsRole :: Role, obsInputs :: [String], obsInstructions :: Word64, obsEntries :: Int, obsReturns :: Int,
  obsUntracked :: Int, obsUnarmed :: Int, obsFill :: Maybe Value, obsArmings :: [Q.Arming], obsSegments :: [Q.Segment], obsFinal :: Maybe Q.Readback,
  obsCalls :: [Call], obsPending :: [Word64], obsMilestones :: [Q.Milestone], obsError :: Maybe T.Text, obsMachineError :: Maybe T.Text,
  obsBankCounts :: BankCounts, obsPcWatches :: [PcWatch] } deriving (Eq,Show)
data Refusal = Binding | Arming | Readback | Watch | FillFetch | Incomplete | Untracked | Unarmed | WrongCore | CallOutsidePlan | EntryUnreadable | Model ModelFailure | ReturnValue | ProcessorState | MemoryResult | WrittenWords | BankWrites | QueueInvariant | Route | ObserverError | UnsupportedSemantic Semantic | MissingTrace | CompletionMismatch | DispatchModel D.DispatchFailure deriving (Eq,Show)

data Completion = Returned | Discarded | Dispatched deriving (Eq,Show)
data BoundaryKind = SaveBoundary | ResumeBoundary | NoSaveBoundary | RetryBoundary | CallbackBoundary | CallbackReturnBoundary | SelectedBoundary | DiscardedBoundary | EnabledBoundary | GlobalLockBoundary | AcquiredBoundary | SelectCompleteBoundary | FirstEntryBoundary | IrqTailBoundary | ReturnBoundary deriving (Eq,Show)
data Boundary = Boundary { boundaryKind :: BoundaryKind, boundaryAddress :: Address, boundaryInstructions :: Word64, boundaryCore :: Int, boundaryRegisters :: D.Registers, boundaryMemory :: Memory, boundaryWrites :: [Written], boundaryBank :: [Written], boundaryValue :: Maybe Word32 } deriving (Eq,Show)
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
    _ -> fail "unsupported scheduler boundary"
instance FromJSON Completion where
  parseJSON = J.withText "completion" $ \kind -> case kind of
    "return" -> pure Returned
    "discard" -> pure Discarded
    "dispatched" -> pure Dispatched
    _ -> fail "unsupported scheduler completion"
instance FromJSON Boundary where
  parseJSON = withObject "boundary" $ \o -> Boundary <$> o .: "kind" <*> (Address <$> (o .: "address" >>= parseHex))
    <*> o .: "instructions" <*> (o .: "core" >>= coreOf) <*> registersOf o <*> (o .: "memory" >>= memoryOf)
    <*> (o .: "writes" >>= mapM writtenOf) <*> (o .: "bank" >>= mapM writtenOf) <*> o .:? "value"

coreOf :: Value -> Parser Int
coreOf = J.withText "core" $ \core -> case core of "primary" -> pure 0; "secondary" -> pure 1; _ -> fail "unknown core"
wordRead :: Value -> Parser Word32
wordRead = withObject "read" $ \o -> do
  view <- o .: "view"
  unless (view == ("committed"::String)) (fail "unreadable scheduler state")
  o .: "value"
memoryOf :: Value -> Parser Memory
memoryOf = J.withArray "memory" $ \rows -> do
  pairs <- mapM (withObject "word" $ \o -> (,) <$> (Address <$> (o .: "address" >>= parseHex)) <*> (o .: "value" >>= wordRead)) (foldr (:) [] rows)
  unless (M.size (M.fromList pairs) == length pairs) (fail "duplicate word")
  pure (M.fromList pairs)
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
    Call d address semantic <$> (e .: "core" >>= coreOf) <*> e .: "cpsr" <*> (Arguments <$> e .: "arguments")
      <*> (e .: "memory" >>= memoryOf) <*> (r .: "memory" >>= memoryOf) <*> r .: "r0" <*> r .: "cpsr"
      <*> (r .: "writes" >>= mapM writtenOf) <*> (r .: "bank" >>= mapM writtenOf) <*> o .: "count" <*> o .: "first" <*> pure (Just trace)
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
    pending <- o .: "pending" >>= mapM (withObject "pending" (.: "entered"))
    milestones <- o .: "milestones" >>= mapM (withObject "milestone" $ \m -> do
      pulse <- m .:? "pulse" >>= traverse (withObject "pulse" (.: "key"))
      let menu = withObject "menu" $ \v -> Q.MenuTriple <$> v .: "menu" <*> v .: "setting" <*> v .: "cursor"
      reached <- m .:? "reached" :: Parser (Maybe Word64)
      Q.Milestone <$> m .: "phase" <*> pure pulse <*> (m .: "expected" >>= menu) <*> (m .:? "observed" >>= traverse menu) <*> pure (reached /= Nothing) <*> m .: "finished")
    watches <- o .: "pc_watches" >>= mapM (withObject "PC watch" $ \w -> PcWatch <$> (w .: "core" >>= coreOf) <*> (Address <$> (w .: "address" >>= parseHex)) <*> w .: "count")
    Observation role inputs <$> o .: "instructions" <*> o .: "entries" <*> o .: "returns" <*> count "untracked" <*> count "unarmed_entries"
      <*> o .:? "fill_fetch" <*> pure armings <*> pure segments <*> pure final <*> o .: "classes" <*> pure pending <*> pure milestones
      <*> o .:? "error" <*> o .:? "machine_failure" <*> o .: "bank_counts" <*> pure watches

expect :: Refusal -> Bool -> Either Refusal ()
expect refusal valid = unless valid (Left refusal)
checkHelperCall :: Call -> Either Refusal ()
checkHelperCall call = do
  expect WrongCore (callCore call == if callDomain call == DomainA then 0 else 1)
  helper <- case callSemantic call of HelperSemantic helper -> pure helper; semantic -> Left (UnsupportedSemantic semantic)
  expected <- either (Left . Model) Right (stockCall (callDomain call) helper (callArguments call) (callBefore call))
  expect ReturnValue (callR0 call == expectedR0 expected)
  expect ProcessorState (callCpsr call .&. 0x1ff == callReturnCpsr call .&. 0x1ff)
  expect MemoryResult (M.keys (callAfter call) == M.keys (callBefore call))
  let normalized (Address a) = Address (a .&. complement 0x40000000 .&. complement 3)
      written = S.fromList [normalized a | Written a _ _ <- callWrites call]
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
  replayed <- foldM apply (callBefore call) (callWrites call)
  expect WrittenWords (written == wanted && replayed == expectedMemory expected)
  -- The other core can acquire the released lock before this core's continuation.
  unless (helper == Release) $
    expect MemoryResult (all (\address -> M.lookup address (callAfter call) == M.lookup address (expectedMemory expected)) (S.toList (expectedWrites expected)))
  expect BankWrites (sort (callBank call) == sort [Written address 4 value | (address,value) <- expectedBank expected])
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
trajectorySha256 = "7497182560f1d971cf17b23429dee5d3257a0263267ddcb3680da9ba78eb2002"
qualify :: B.ByteString -> Plan -> Observation -> Either Refusal Value
qualify bytes plan observation = do
  forM_ (planEntries plan) $ \entry -> expect (UnsupportedSemantic (entrySemantic entry)) (modelled (entrySemantic entry))
  expect Binding (obsRole observation == planRole plan && length (obsInputs observation) == 6 && take 5 (obsInputs observation) ==
    [sha256Hex (sha256 bytes),Q.runDocumentSha256,planDat plan,Q.bootRomSha256,trajectorySha256])
  expect FillFetch (obsFill observation == Nothing)
  let calls = sortOn callFirst (obsCalls observation)
  expect Untracked (obsUntracked observation == 0)
  expect Unarmed (obsUnarmed observation == 0)
  forM_ (obsArmings observation) $ \arming -> either (const (Left Readback)) Right
    (Q.checkReadback (sharedPlan plan) Q.ArmingReadback (Q.armingReadback arming) (Q.armingDomain arming))
  expect Arming (sort (map Q.armingDomain (obsArmings observation)) == domains)
  final <- maybe (Left Readback) Right (obsFinal observation)
  forM_ domains $ \domain -> either (const (Left Readback)) Right (Q.checkReadback (sharedPlan plan) Q.FinalReadback final domain)
  either (const (Left Watch)) Right (Q.checkSegments (watchPlan plan) (obsArmings observation) final (obsSegments observation))
  forM_ calls $ \call -> do
    expect CallOutsidePlan (any (\entry -> entryDomain entry == callDomain call && entryAddress entry == callAddress call && entrySemantic entry == callSemantic call) (planEntries plan))
    expect Incomplete (callCount call > 0)
    checkCallFor (obsRole observation) call
  expect Incomplete (not (null calls) && obsReturns observation == sum (map callCount calls) && obsEntries observation == obsReturns observation + length (obsPending observation)
    && all (\entered -> entered <= obsInstructions observation && obsInstructions observation-entered <= Q.pendingWindow) (obsPending observation))
  expect ObserverError (obsError observation == Nothing && obsMachineError observation == Nothing)
  let milestones = obsMilestones observation
      phases = S.fromList (map Q.milestonePhase milestones)
  expect Route (all (\m -> Q.milestoneReached m && Q.milestoneObserved m == Just (Q.milestoneExpected m)) milestones &&
    all (`S.member` phases) ["cold-start","shooting-menu","modal75-film","modal75-returned","modal75-film-repeated","modal75-returned-repeated","modal75-handback"])
  pure (summary observation)

routeSignature :: Observation -> [(T.Text,Maybe T.Text,Q.MenuTriple,Maybe Q.MenuTriple)]
routeSignature = map (\m -> (Q.milestonePhase m,Q.milestoneKey m,Q.milestoneExpected m,Q.milestoneObserved m)) . obsMilestones
compareRoutes :: Observation -> Observation -> Either Refusal ()
compareRoutes stock owned = do
  expect Binding (drop 3 (obsInputs stock) == drop 3 (obsInputs owned))
  expect Route (obsRole stock == Stock && obsRole owned == Owned && routeSignature stock == routeSignature owned)
summary :: Observation -> Value
summary observation = object
  [ "role" .= roleName (obsRole observation), "calls" .= obsReturns observation
  , "per_entry" .= [object ["domain" .= domainLabel domain,"entry" .= hex address,"semantic" .= semanticValue semantic,"calls" .= count]
      | ((domain,Address address,semantic),count) <- M.toList (M.fromListWith (+) [((callDomain call,callAddress call,callSemantic call),callCount call) | call <- obsCalls observation])]
  , "redesign_items" .= [object ["id" .= D.redesignName item,"calls" .= sum [callCount call | call <- obsCalls observation,DispatchSemantic point <- [callSemantic call],Right tags <- [dispatchTags (obsRole observation) point call],item `elem` tags]]
      | item <- [D.NullSelectionOwnRegisters,D.NullLoopReleasesGlobalLock,D.RemoteTerminateRestarts]]
  , "per_core" .= map metrics [0,1::Int] ]
 where
  metrics core = object ["core" .= core,"cross_domain_inserts" .= total cross,"ringing_notifies" .= total rings,
    "bank_writes" .= (if core==0 then primaryBankWrites (obsBankCounts observation) else secondaryBankWrites (obsBankCounts observation)),
    "doorbell_handler_runs" .= sum [n | PcWatch c (Address pc) n <- obsPcWatches observation,c==core,pc `elem` [0x40b00,0xd0b00]],
    "pc_watches" .= [object ["address" .= hex pc,"count" .= sum [n | PcWatch c address n <- obsPcWatches observation,c==core,address==Address pc]]
      | pc <- [0x436ec,0xd36ec,0xec8014,0x40b00,0xd0b00]]]
   where
    total predicate = sum [callCount c | c <- obsCalls observation,callCore c==core,predicate c]
    cross c = callSemantic c==HelperSemantic Insert && case callArguments c of Arguments (record:_) -> M.lookup (Address (record+4)) (callBefore c) /= Just (if callDomain c==DomainA then 1 else 2); _ -> False
    rings c = callSemantic c==HelperSemantic Notify && not (null (callBank c))

firstSemanticFailure :: Observation -> Maybe (Call,Refusal)
firstSemanticFailure observation = case [(call,refusal) | call <- sortOn callFirst (obsCalls observation),Left refusal <- [checkCallFor (obsRole observation) call]] of
  first:_ -> Just first
  [] -> Nothing

controlFailure :: Role -> Observation -> Either Refusal ()
controlFailure role observation = case role of
  SilentNotify -> firstTarget isRemoteNotify BankWrites
  WrongPredicate -> firstTarget (\call -> callDomain call==DomainB && callSemantic call==HelperSemantic Notify && case callArguments call of Arguments (1:_) -> True; _ -> False) ReturnValue
  WakeNoInsert -> case firstSemanticFailure observation of
    Just (call,refusal) -> expect MemoryResult (callSemantic call==ServiceSemantic WakeTask && refusal `elem` [MemoryResult,WrittenWords,ReturnValue])
    Nothing -> Left Incomplete
  _ -> pure ()
 where
  isRemoteNotify call = callSemantic call==HelperSemantic Notify && case stockCall (callDomain call) Notify (callArguments call) M.empty of
    Right expected -> not (null (expectedBank expected))
    Left _ -> False
  firstTarget predicate refusal = case (filter predicate (sortOn callFirst (obsCalls observation)),firstSemanticFailure observation) of
    (target:_,Just (failed,actual)) -> expect refusal (target==failed && actual==refusal)
    _ -> Left Incomplete

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
  ServiceSemantic service -> A.supportsService service || C.supportsService service || E.supportsService service

checkCall :: Call -> Either Refusal ()
checkCall = checkCallFor Owned

checkCallFor :: Role -> Call -> Either Refusal ()
checkCallFor role call = do
  expect WrongCore (callCore call == if callDomain call==DomainA then 0 else 1)
  case callSemantic call of
    HelperSemantic _ -> checkHelperCall call
    DoorbellSemantic -> do
      either (Left . DispatchModel) Right
        (D.checkDoorbell (callDomain call) (callAfter call) [(address,value) | Written address size value <- callBank call,size==4])
      let (_,_,reschedule,_,_) = D.stateAddresses (callDomain call)
      expect WrittenWords (any (\(Written (Address address) size value) -> address .&. complement 0x40000000==reschedule .&. complement 0x40000000 && size==4 && value==1) (callWrites call))
      expect ProcessorState (callCpsr call .&. 0x1ff==callReturnCpsr call .&. 0x1ff)
    DispatchSemantic point -> dispatchTags role point call >> pure ()
    ServiceSemantic service -> checkServiceCall service call

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

checkServiceCall :: Service -> Call -> Either Refusal ()
checkServiceCall service call = do
  trace <- maybe (Left MissingTrace) Right (callTrace call)
  let domain=callDomain call
      initial = serviceModel domain service (callCpsr call) (callArguments call)
      significant kind = kind `elem` [SaveBoundary,NoSaveBoundary,RetryBoundary,CallbackBoundary,ReturnBoundary]
      seek wanted boundaries = case dropWhile ((/=wanted) . boundaryKind) boundaries of
        [] -> Left CompletionMismatch
        boundary:rest -> Right (boundary,rest)
      loop predicted previous boundaries = do
        let (prefix,rest)=break (significant . boundaryKind) boundaries
        atAcquisition <- case reverse [boundary | boundary <- prefix,boundaryKind boundary==AcquiredBoundary] of
          [] -> Right previous
          acquired:_ -> do
            let memory=M.union (boundaryMemory acquired) previous
                words'=V.domainWords domain
                records=V.contextTable words':[V.contextTable words'+0x84] ++
                  [record | Just record <- [M.lookup (Address (V.currentContext words')) memory]]
                locks=S.fromList [Address lock | record <- records,Just lock <- [M.lookup (Address (record+8)) memory]]
                taken=[address | Written address 4 1 <- boundaryWrites acquired,S.member address locks]
            case taken of
              [address] -> Right (M.insert address 0 memory)
              _ -> Left CompletionMismatch
        expected <- either (Left . Model) Right (predicted atAcquisition)
        (boundary,remaining) <- case rest of [] -> Left CompletionMismatch; boundary:remaining -> Right (boundary,remaining)
        let state=boundaryRegisters boundary
            args=take 4 (D.registers state)
            writes=concatMap boundaryWrites (prefix++[boundary])
            bank=concatMap boundaryBank (prefix++[boundary])
            actualKind=boundaryKind boundary
            wanted=V.serviceExpected expected
            getReg index = maybe (Left MissingTrace) Right (case drop index (D.registers state) of value':_ -> Just value'; _ -> Nothing)
        expect WrongCore (boundaryCore boundary==callCore call)
        expect ProcessorState (D.processorState state .&. 0x1ff==V.serviceCpsr expected .&. 0x1ff)
        forM_ (V.serviceR0 expected) $ \result -> getReg 0 >>= expect ReturnValue . (==result)
        let normalize (Address address)=Address (address .&. complement 0x40000000 .&. complement 3)
            scratch=[(normalize (Address start),normalize (Address (min (start+size) sp)))
              | Just task <- [M.lookup (Address (V.runningCell (V.domainWords domain))) (callBefore call)]
              , Just config <- [M.lookup (Address (task+8)) (callBefore call)]
              , Just size <- [M.lookup (Address (config+0x10)) (callBefore call)]
              , Just start <- [M.lookup (Address (config+0x14)) (callBefore call)]
              , sp <- take 1 (drop 13 (D.registers (traceBefore trace)))]
            known=M.fromList [(normalize address,address) | address <- M.keys atAcquisition,
              S.member address (expectedWrites wanted) || not (any (\(lo,hi) -> lo<=normalize address && normalize address<hi) scratch)]
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
        expect BankWrites (sort bank==sort [Written address 4 value' | (address,value') <- expectedBank wanted])
        case V.serviceCompletion expected of
          V.ServiceReturn -> expect CompletionMismatch (actualKind==ReturnBoundary && traceCompletion trace==Returned)
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
