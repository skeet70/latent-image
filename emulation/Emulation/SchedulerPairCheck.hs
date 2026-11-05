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
import Emulation.Sha256 (sha256, sha256Hex)
import Replacement.RtosOwnFirst.Regions (Domain (..), domains, domainLabel)
import Replacement.RtosScheduler.Regions (Helper (..))

data Written = Written Address Int Word32 deriving (Eq,Ord,Show)
data Call = Call { callDomain :: Domain, callAddress :: Address, callSemantic :: Semantic, callCore :: Int, callCpsr :: Word32, callArguments :: Arguments,
  callBefore :: Memory, callAfter :: Memory, callR0 :: Word32, callReturnCpsr :: Word32, callWrites :: [Written], callBank :: [Written], callCount :: Int, callFirst :: Word64 } deriving (Eq,Show)
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
data Refusal = Binding | Arming | Readback | Watch | FillFetch | Incomplete | Untracked | Unarmed | WrongCore | CallOutsidePlan | EntryUnreadable | Model ModelFailure | ReturnValue | ProcessorState | MemoryResult | WrittenWords | BankWrites | QueueInvariant | Route | ObserverError | UnsupportedSemantic Semantic deriving (Eq,Show)

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
    Call d address semantic <$> (e .: "core" >>= coreOf) <*> e .: "cpsr" <*> (Arguments <$> e .: "arguments")
      <*> (e .: "memory" >>= memoryOf) <*> (r .: "memory" >>= memoryOf) <*> r .: "r0" <*> r .: "cpsr"
      <*> (r .: "writes" >>= mapM writtenOf) <*> (r .: "bank" >>= mapM writtenOf) <*> o .: "count" <*> o .: "first"
instance FromJSON Observation where
  parseJSON = withObject "scheduler observation" $ \o -> do
    schema <- o .: "schema"
    unless (schema == (2::Int)) (fail "observation schema differs")
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
checkCall :: Call -> Either Refusal ()
checkCall call = do
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
  forM_ (planEntries plan) $ \entry -> case entrySemantic entry of HelperSemantic _ -> pure (); semantic -> Left (UnsupportedSemantic semantic)
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
    checkCall call
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
summary observation = object ["role" .= roleName (obsRole observation),"calls" .= obsReturns observation,"per_core" .= map metrics [0,1::Int]]
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
firstSemanticFailure observation = case [(call,refusal) | call <- sortOn callFirst (obsCalls observation),Left refusal <- [checkCall call]] of
  first:_ -> Just first
  [] -> Nothing

controlFailure :: Role -> Observation -> Either Refusal ()
controlFailure role observation = case role of
  SilentNotify -> firstTarget isRemoteNotify BankWrites
  WrongPredicate -> firstTarget (\call -> callDomain call==DomainB && callSemantic call==HelperSemantic Notify && case callArguments call of Arguments (1:_) -> True; _ -> False) ReturnValue
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
