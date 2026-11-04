{-# LANGUAGE LambdaCase #-}
{-# LANGUAGE OverloadedStrings #-}
-- | Qualifies one own-first pair observation against its plan, and compares a stock and an owned
-- observation over route steps and menu states only.
module Emulation.OwnFirstPairCheck
  ( Read (..)
  , EntryState (..)
  , ResultState (..)
  , CallClass (..)
  , ViewHash (..)
  , RangeReadback (..)
  , Readback (..)
  , Arming (..)
  , Segment (..)
  , MenuTriple (..)
  , Milestone (..)
  , Reached (..)
  , Observation (..)
  , decodeObservation
  , RefusalKind (..)
  , Refusal (..)
  , refusalKind
  , checkCall
  , Qualified (..)
  , qualify
  , qualifiedValue
  , RouteDifference (..)
  , compareRoutes
  ) where

import Control.Monad (forM, forM_, unless, when)
import Data.Aeson (FromJSON (..), Value (..), object, withObject, withText, (.!=), (.:), (.:?), (.=))
import qualified Data.Aeson as A
import qualified Data.Aeson.Key as K
import Data.Aeson.Types (Parser)
import Data.Bits (complement, (.&.), (.|.))
import qualified Data.ByteString as B
import Data.List (sort)
import qualified Data.Map.Strict as M
import Data.Maybe (isJust)
import qualified Data.Text as T
import Data.Word (Word32, Word64)
import Emulation.OwnFirstPairPlan
import Emulation.Sha256 (sha256, sha256Hex)
import Numeric (readHex, showHex)
import Prelude hiding (Read)
import Replacement.RtosOwnFirst.Regions (Domain (..), domainLabel, domains)

-- | A word as the executing core saw it; anything but a committed value is unreadable.
data Read = Committed Word32 | Unreadable T.Text deriving (Eq, Show)

data EntryState = EntryState
  { entryDomain :: Domain
  , entrySlot :: Slot
  , entryCore :: Int
  , entryCpsr :: Word32
  , entryR0 :: Word32
  , entryLr :: Word32
  , entryIdentity :: Word32
  , entryWords :: M.Map WordName Read
  , entryTaskFlags :: Maybe Read
  , entryPointee :: Maybe Read
  } deriving (Eq, Show)

data ResultState = ResultState
  { resultR0 :: Word32
  , resultCpsr :: Word32
  , resultWords :: M.Map WordName Read
  , resultPointee :: Maybe Read
  } deriving (Eq, Show)

data CallClass = CallClass
  { classEntry :: EntryState
  , classResult :: ResultState
  , classTailHits :: Int
  , classCount :: Int
  , classFirst :: Word64
  } deriving (Eq, Show)

data ViewHash = ViewHash {viewCore :: Int, viewAddress :: Word32, viewDigest :: Either T.Text String} deriving (Eq, Show)

-- | One guarded extent, keyed by its physical range.
data RangeReadback = RangeReadback
  { readbackDomain :: Domain
  , readbackStart :: Word32
  , readbackEnd :: Word32
  , readbackPhysical :: String
  , readbackViews :: [ViewHash]
  } deriving (Eq, Show)

data Readback = Readback {readbackInstructions :: Word64, readbackRanges :: [RangeReadback]} deriving (Eq, Show)

data Arming = Arming {armingDomain :: Domain, armingReadback :: Readback} deriving (Eq, Show)

-- | One watch over the armed loaders' extents, with its write classes as (start, end, kinds).
data Segment = Segment
  { segmentDomains :: [Domain]
  , segmentStarted :: Word64
  , segmentEnded :: Word64
  , segmentRows :: Int
  , segmentViolation :: Maybe T.Text
  , segmentClasses :: [(Word32, Word32, [T.Text])]
  } deriving (Eq, Show)

data MenuTriple = MenuTriple {menuId :: Int, menuSetting :: Int, menuCursor :: Int} deriving (Eq, Ord, Show)

data Milestone = Milestone
  { milestonePhase :: T.Text
  , milestoneKey :: Maybe T.Text
  , milestoneExpected :: MenuTriple
  , milestoneObserved :: Maybe MenuTriple
  , milestoneReached :: Bool
  } deriving (Eq, Show)

data Reached = Reached {reachedDomain :: Domain, reachedSlot :: Slot, reachedCalls :: Int, reachedReturns :: Int, reachedPending :: Int}
  deriving (Eq, Show)

data Observation = Observation
  { obsRole :: Role
  , obsInputs :: [String]
  , obsEntries :: Int
  , obsReturns :: Int
  , obsUntracked :: Int
  , obsUnarmedEntries :: Int
  , obsUnmatchedTails :: Int
  , obsFillFetch :: Maybe Value
  , obsArmings :: [Arming]
  , obsSegments :: [Segment]
  , obsFinal :: Maybe Readback
  , obsMilestones :: [Milestone]
  , obsReached :: [Reached]
  , obsClasses :: [CallClass]
  , obsPending :: Int
  , obsError :: Maybe T.Text
  , obsMachineFailure :: Maybe T.Text
  } deriving (Eq, Show)

hexWord :: Value -> Parser Word32
hexWord = withText "hex word" $ \text -> case T.unpack text of
  '0' : 'x' : digits | [(value, "")] <- readHex digits, value <= toInteger (maxBound :: Word32) -> pure (fromInteger value)
  _ -> fail ("not a hexadecimal word: " ++ T.unpack text)

domainOf :: Value -> Parser Domain
domainOf = withText "domain" $ \text -> case text of
  "a" -> pure DomainA
  "b" -> pure DomainB
  _ -> fail "unknown domain"

slotId :: Value -> Parser (Domain, Slot)
slotId = withObject "slot id" $ \fields -> do
  domain <- fields .: "domain" >>= domainOf
  offset <- fields .: "slot" >>= hexWord
  slot <- maybe (fail "unknown slot") pure (slotFromOffset offset)
  pure (domain, slot)

readOf :: Value -> Parser Read
readOf = withObject "read" $ \fields -> do
  view <- fields .: "view"
  if view == ("committed" :: T.Text) then Committed <$> fields .: "value" else pure (Unreadable view)

wordsOf :: Value -> Parser (M.Map WordName Read)
wordsOf = withObject "words" $ \fields -> fmap (M.fromList . concat) . forM [minBound .. maxBound] $ \name ->
  fields .:? K.fromText (wordNameWire name) >>= maybe (pure []) (fmap (\value -> [(name, value)]) . readOf)

readbackOf :: Value -> Parser Readback
readbackOf = withObject "readback" $ \fields -> Readback <$> fields .: "instructions" <*> (fields .: "ranges" >>= mapM range)
 where
  range = withObject "range readback" $ \fields ->
    RangeReadback <$> (fields .: "domain" >>= domainOf) <*> (fields .: "start" >>= hexWord) <*> (fields .: "end" >>= hexWord)
      <*> fields .: "physical_sha256" <*> (fields .: "views" >>= mapM view)
  view = withObject "view hash" $ \fields -> do
    digest <- fields .:? "sha256"
    failure <- fields .:? "error"
    outcome <- case (digest, failure) of
      (Just value, Nothing) -> pure (Right value)
      (Nothing, Just value) -> pure (Left value)
      _ -> fail "a view hash holds a digest or an error"
    ViewHash <$> fields .: "core" <*> (fields .: "address" >>= hexWord) <*> pure outcome

menuOf :: Value -> Parser MenuTriple
menuOf = withObject "menu" $ \fields -> MenuTriple <$> fields .: "menu" <*> fields .: "setting" <*> fields .: "cursor"

instance FromJSON Observation where
  parseJSON = withObject "own-first pair observation" $ \fields -> do
    schema <- fields .: "schema"
    unless (schema == (1 :: Int)) (fail "unsupported observation schema")
    role <- fields .: "role" >>= either fail pure . parseRole
    inputs <- fields .: "inputs" >>= mapM (withObject "input" (.: "sha256"))
    let sampleCount name = fields .: name >>= withObject "sample" (.: "count")
    armings <- fields .: "armings" >>= mapM (withObject "arming" $ \arming ->
      Arming <$> (arming .: "domain" >>= domainOf) <*> (arming .: "readback" >>= readbackOf))
    segments <- fields .: "segments" >>= mapM (withObject "segment" $ \segment -> do
      state <- segment .: "state"
      rows <- state .: "records" >>= withArrayLength
      classes <- state .:? "classes" .!= [] >>= mapM (withObject "watch class" $ \entry -> do
        bounds <- entry .: "range"
        case bounds of
          [start, end] -> (,,) start end <$> entry .: "kinds"
          _ -> fail "watch class range is not a pair")
      Segment <$> (segment .: "domains" >>= mapM domainOf) <*> segment .: "started" <*> segment .: "ended"
        <*> pure rows <*> state .:? "violation" <*> pure classes)
    final <- fields .:? "final_readback" >>= traverse readbackOf
    milestones <- fields .: "milestones" >>= mapM (withObject "milestone" $ \milestone -> do
      pulse <- milestone .:? "pulse"
      key <- traverse (withObject "pulse" (.: "key")) pulse
      reached <- milestone .:? "reached" :: Parser (Maybe Word64)
      Milestone <$> milestone .: "phase" <*> pure key <*> (milestone .: "expected" >>= menuOf)
        <*> (milestone .:? "observed" >>= traverse menuOf) <*> pure (isJust reached))
    reached <- fields .: "reached" >>= mapM (withObject "reached slot" $ \row -> do
      (domain, slot) <- row .: "slot" >>= slotId
      Reached domain slot <$> row .: "calls" <*> row .: "returns" <*> row .: "pending")
    classes <- fields .: "classes" >>= mapM (withObject "call class" $ \row -> do
      entry <- row .: "entry" >>= withObject "entry" (\state -> do
        (domain, slot) <- state .: "slot" >>= slotId
        core <- state .: "core" >>= withText "core" (\text -> case text of
          "primary" -> pure 0
          "secondary" -> pure 1
          _ -> fail "unknown core")
        EntryState domain slot core <$> state .: "cpsr" <*> state .: "r0" <*> (state .: "lr" >>= hexWord)
          <*> state .: "identity" <*> (state .: "words" >>= wordsOf)
          <*> (state .:? "task_flags" >>= traverse readOf) <*> (state .:? "pointee" >>= traverse readOf))
      result <- row .: "result" >>= withObject "result" (\state ->
        ResultState <$> state .: "r0" <*> state .: "cpsr" <*> (state .: "words" >>= wordsOf)
          <*> (state .:? "pointee" >>= traverse readOf))
      CallClass entry result <$> row .: "tail_hits" <*> row .: "count" <*> row .: "first")
    pending <- fields .: "pending" >>= withArrayLength
    Observation role inputs <$> fields .: "entries" <*> fields .: "returns" <*> sampleCount "untracked"
      <*> sampleCount "unarmed_entries" <*> sampleCount "unmatched_tails" <*> fields .:? "fill_fetch"
      <*> pure armings <*> pure segments <*> pure final <*> pure milestones <*> pure reached <*> pure classes
      <*> pure pending <*> fields .:? "error" <*> fields .:? "machine_failure"
   where
    withArrayLength value = case value of
      Array items -> pure (length items)
      _ -> fail "expected an array"

decodeObservation :: B.ByteString -> Either String Observation
decodeObservation = A.eitherDecodeStrict'

-- | Refusals in the order qualification checks them.
data RefusalKind = BindingRefusal | ReadbackRefusal | ImmutableRefusal | FillRefusal | ObserverRefusal | TrackingRefusal | SemanticRefusal | RouteRefusal
  deriving (Eq, Ord, Show, Enum, Bounded)

data Refusal
  = PlanDigestDiffers
  | RoleDiffers Role Role
  | DatDiffers
  | ReadbackDiffers Domain Word32 T.Text
  | GuardNotArmed Domain
  | FinalReadbackMissing
  | ImmutableWrite Int
  | WatchViolation T.Text
  | WatchCoverage String
  | FillFetched Value
  | ObserverStopped T.Text
  | UntrackedCalls Int
  | UnarmedEntries Int
  | UnmatchedTails Int
  | CountsDiffer
  | CallMismatch Domain Slot String
  | RouteIncomplete T.Text
  deriving (Eq, Show)

refusalKind :: Refusal -> RefusalKind
refusalKind refusal = case refusal of
  PlanDigestDiffers -> BindingRefusal
  RoleDiffers _ _ -> BindingRefusal
  DatDiffers -> BindingRefusal
  ReadbackDiffers {} -> ReadbackRefusal
  GuardNotArmed _ -> ReadbackRefusal
  FinalReadbackMissing -> ReadbackRefusal
  ImmutableWrite _ -> ImmutableRefusal
  WatchViolation _ -> ImmutableRefusal
  WatchCoverage _ -> ImmutableRefusal
  FillFetched _ -> FillRefusal
  ObserverStopped _ -> ObserverRefusal
  UntrackedCalls _ -> TrackingRefusal
  UnarmedEntries _ -> TrackingRefusal
  UnmatchedTails _ -> TrackingRefusal
  CountsDiffer -> TrackingRefusal
  CallMismatch {} -> SemanticRefusal
  RouteIncomplete _ -> RouteRefusal

refuseWhen :: Bool -> Refusal -> Either Refusal ()
refuseWhen condition refusal = when condition (Left refusal)

-- | Mode, T, F, I and A.
control :: Word32 -> Word32
control cpsr = cpsr .&. 0x1ff

irqMasked :: Word32 -> Bool
irqMasked cpsr = cpsr .&. 0x80 /= 0

flag :: Bool -> Word32
flag condition = if condition then 1 else 0

-- | One class's result against the slot semantics at its entry state. `tails` is the tail hits a
-- +0x1618 call of this loader must show.
checkCall :: Int -> CallClass -> Either String ()
checkCall tails (CallClass entry result tailHits _ _) = do
  let domain = entryDomain entry
      cpsr = entryCpsr entry
      masked = irqMasked cpsr
      r0 = resultR0 result
      word name = case M.lookup name (entryWords entry) of
        Just (Committed value) -> Right value
        Just (Unreadable view) -> Left ("entry " ++ T.unpack (wordNameWire name) ++ " word is " ++ T.unpack view)
        Nothing -> Left ("entry " ++ T.unpack (wordNameWire name) ++ " word was not read")
      returned name = case M.lookup name (resultWords result) of
        Just (Committed value) -> Right value
        _ -> Left ("returned " ++ T.unpack (wordNameWire name) ++ " word is unreadable")
      committed what = \case
        Just (Committed value) -> Right value
        _ -> Left (what ++ " is unreadable")
      expect what condition = unless condition (Left what)
      expectR0 value = expect ("r0 " ++ hex r0 ++ " differs from " ++ hex value) (r0 == value)
      unchangedControl = expect "CPSR mode, T, F, I or A changed" (control (resultCpsr result) == control cpsr)
      withIrq masking = expect "CPSR differs from the entry with I set as the slot defines"
        (control (resultCpsr result) == (control cpsr .&. complement 0x80) .|. (if masking then 0x80 else 0))
      pointeeUnchanged = do
        before <- committed "entry *r0" (entryPointee entry)
        after <- committed "returned *r0" (resultPointee result)
        expect "*r0 changed on the error path" (before == after)
      pointeeIs value = do
        after <- committed "returned *r0" (resultPointee result)
        expect ("*r0 " ++ hex after ++ " differs from " ++ hex value) (after == value)
      guarded admits okPath = if admits && not masked
        then expectR0 0 >> unchangedControl >> okPath
        else expectR0 contextError >> unchangedControl
  expect "tail hits differ from the loader's +0x1618 routing"
    (tailHits == (if entrySlot entry == Slot1618 then tails else 0))
  case entrySlot entry of
    Slot60 -> do
      current <- word CurrentTask
      clear <- if current == 0 then pure True else (\flags -> flags .&. 4 == 0) <$> committed "task flags byte" (entryTaskFlags entry)
      expectR0 (flag clear) >> unchangedControl
    SlotFc -> do
      context <- word ContextWord
      base <- word TimerBase
      guarded (context == 0) $ do
        time <- committed "entry *r0" (entryPointee entry)
        offset <- returned TimeOffset
        expect "time offset differs from *r0 - timer base" (offset == time - base)
    Slot100 -> do
      context <- word ContextWord
      base <- word TimerBase
      offset <- word TimeOffset
      if context == 0 && not masked then pure () else pointeeUnchanged
      guarded (context == 0) (pointeeIs (base + offset))
    Slot128 -> do
      context <- word ContextWord
      current <- word CurrentTask
      if context == 0 && not masked then pure () else pointeeUnchanged
      guarded (context == 0) (pointeeIs (taskIndex domain current))
    Slot12c -> do
      context <- word ContextWord
      current <- word CurrentTask
      if context /= 0 && not masked then pure () else pointeeUnchanged
      guarded (context /= 0) (pointeeIs (if current == 0 then 0 else taskIndex domain current))
    Slot130 -> lockService (== 0) True
    Slot134 -> lockService (/= 0) True
    Slot138 -> lockService (== 0) False
    Slot13c -> lockService (/= 0) False
    Slot148 -> word ContextWord >>= \context -> expectR0 (flag (context /= 0)) >> unchangedControl
    Slot14c -> expectR0 (flag masked) >> unchangedControl
    Slot150 -> word DispatchEnabled >>= \dispatch -> expectR0 (flag (dispatch == 0)) >> unchangedControl
    Slot154 -> do
      context <- word ContextWord
      dispatch <- word DispatchEnabled
      expectR0 (flag (context /= 0 || masked || dispatch == 0)) >> unchangedControl
    Slot164 -> expectR0 (cpsr .&. 0x80) >> withIrq True
    Slot168 -> do
      let requested = entryR0 entry
      expectR0 ((requested .&. 0x80) .|. (cpsr .&. complement 0x80))
      withIrq (requested .&. 0x80 /= 0)
    Slot16c -> do
      context <- word ContextWord
      current <- word CurrentTask
      expectR0 (if context /= 0 || masked then 0 else taskIndex domain current) >> unchangedControl
    Slot1618 -> expectR0 (flag (entryIdentity entry /= primaryIdentity)) >> unchangedControl
 where
  hex value = "0x" ++ showHex value ""
  lockService admits masking = do
    context <- case M.lookup ContextWord (entryWords entry) of
      Just (Committed value) -> Right value
      _ -> Left "entry context word is unreadable"
    let cpsr = entryCpsr entry
        resultControl = control (resultCpsr result)
    if admits context
      then do
        unless (resultR0 result == 0) (Left "lock service r0 differs from 0")
        unless (resultControl == (control cpsr .&. complement 0x80) .|. (if masking then 0x80 else 0)) (Left "lock service CPSR differs from the entry with I as the service defines")
      else do
        unless (resultR0 result == contextError) (Left "lock service r0 differs from -25")
        unless (resultControl == control cpsr) (Left "lock service changed CPSR on the error path")

data Qualified = Qualified
  { qualifiedRole :: Role
  , qualifiedCalls :: Int
  , qualifiedClasses :: Int
  , qualifiedPending :: Int
  , qualifiedReached :: [Reached]
  , qualifiedMilestones :: [Milestone]
  } deriving (Eq, Show)

checkReadback :: Plan -> Readback -> Domain -> Either Refusal ()
checkReadback plan readback domain = do
  let expected = [range | range <- planGuarded plan, guardedDomain range == domain]
      observed = [range | range <- readbackRanges readback, readbackDomain range == domain]
  refuseWhen (map (\range -> (guardedStart range, guardedEnd range)) expected /= map (\range -> (readbackStart range, readbackEnd range)) observed)
    (ReadbackDiffers domain 0 "the readback does not cover the plan's extents")
  forM_ (zip expected observed) $ \(range, seen) -> do
    let digest = guardedSha256 range
        start = guardedStart range
        views = sort [(viewCore view, viewAddress view) | view <- readbackViews seen]
    refuseWhen (readbackPhysical seen /= digest) (ReadbackDiffers domain start "physical")
    refuseWhen (views /= sort [(core, address) | core <- [0, 1], address <- [start, start + 0x40000000]])
      (ReadbackDiffers domain start "views do not cover both cores at the low address and the alias")
    forM_ (readbackViews seen) $ \view -> case viewDigest view of
      Right value | value == digest -> pure ()
      Right _ -> Left (ReadbackDiffers domain start (T.pack ("core " ++ show (viewCore view) ++ " view at 0x" ++ showHex (viewAddress view) "")))
      Left failure -> Left (ReadbackDiffers domain start failure)

checkSegments :: Plan -> [Arming] -> Readback -> [Segment] -> Either Refusal ()
checkSegments plan armings final segments = do
  forM_ segments $ \segment -> do
    forM_ (segmentViolation segment) (Left . WatchViolation)
    refuseWhen (segmentRows segment /= 0) (ImmutableWrite (segmentRows segment))
  let armedPrefixes = drop 1 (scanl (\acc arming -> acc ++ [armingDomain arming]) [] armings)
      starts = map (readbackInstructions . armingReadback) armings
  refuseWhen (map segmentDomains segments /= armedPrefixes) (WatchCoverage "segments do not follow the armings")
  refuseWhen (map segmentStarted segments /= starts) (WatchCoverage "a segment does not start at its arming")
  refuseWhen (or (zipWith (\segment next -> segmentEnded segment /= segmentStarted next) segments (drop 1 segments)))
    (WatchCoverage "segments leave a gap")
  refuseWhen (fmap segmentEnded (lastMaybe segments) /= Just (readbackInstructions final)) (WatchCoverage "the last segment does not reach the final readback")
  forM_ segments $ \segment -> do
    let wanted = sort [ (guardedStart range, guardedEnd range)
                      | range <- planGuarded plan, guardedDomain range `elem` segmentDomains segment ]
        writes = ["cpu-write", "host-write", "model-bus-write"]
    refuseWhen (sort [(start, end) | (start, end, kinds) <- segmentClasses segment, all (`elem` kinds) writes] /= wanted)
      (WatchCoverage "a segment does not watch every armed extent for CPU, host and model-bus writes")
 where
  lastMaybe items = if null items then Nothing else Just (last items)

-- | `planBytes` and `datDigest` are what the observation's first and third inputs must hash to.
qualify :: B.ByteString -> Plan -> Observation -> Either Refusal Qualified
qualify planBytes plan observation = do
  refuseWhen (take 1 (obsInputs observation) /= [sha256Hex (sha256 planBytes)]) PlanDigestDiffers
  refuseWhen (obsRole observation /= planRole plan) (RoleDiffers (planRole plan) (obsRole observation))
  refuseWhen (take 1 (drop 2 (obsInputs observation)) /= [planDatSha256 plan]) DatDiffers
  forM_ (obsArmings observation) $ \arming -> checkReadback plan (armingReadback arming) (armingDomain arming)
  forM_ domains $ \domain -> refuseWhen (domain `notElem` map armingDomain (obsArmings observation)) (GuardNotArmed domain)
  refuseWhen (length (obsArmings observation) /= length domains) (WatchCoverage "a loader armed twice")
  final <- maybe (Left FinalReadbackMissing) Right (obsFinal observation)
  forM_ domains (checkReadback plan final)
  checkSegments plan (obsArmings observation) final (obsSegments observation)
  forM_ (obsFillFetch observation) (Left . FillFetched)
  forM_ (obsError observation) (Left . ObserverStopped)
  forM_ (obsMachineFailure observation) (Left . ObserverStopped)
  refuseWhen (obsUntracked observation /= 0) (UntrackedCalls (obsUntracked observation))
  refuseWhen (obsUnarmedEntries observation /= 0) (UnarmedEntries (obsUnarmedEntries observation))
  refuseWhen (obsUnmatchedTails observation /= 0) (UnmatchedTails (obsUnmatchedTails observation))
  let checked = sum (map classCount (obsClasses observation))
  refuseWhen (checked /= obsReturns observation || obsReturns observation + obsPending observation /= obsEntries observation) CountsDiffer
  let entries = [(planEntryDomain entry, planEntrySlot entry) | entry <- planEntries plan]
      tailsOf domain = length [() | tailEntry <- planTails plan, planTailDomain tailEntry == domain]
  forM_ (obsClasses observation) $ \call -> do
    let entry = classEntry call
        key = (entryDomain entry, entrySlot entry)
    refuseWhen (key `notElem` entries) (CallMismatch (entryDomain entry) (entrySlot entry) "the slot is not in the plan")
    either (Left . CallMismatch (entryDomain entry) (entrySlot entry)) pure (checkCall (tailsOf (entryDomain entry)) call)
  forM_ (obsMilestones observation) $ \milestone ->
    refuseWhen (not (milestoneReached milestone) || milestoneObserved milestone /= Just (milestoneExpected milestone))
      (RouteIncomplete (milestonePhase milestone))
  refuseWhen (null (obsMilestones observation)) (RouteIncomplete "no milestone")
  pure Qualified
    { qualifiedRole = obsRole observation
    , qualifiedCalls = checked
    , qualifiedClasses = length (obsClasses observation)
    , qualifiedPending = obsPending observation
    , qualifiedReached = filter ((> 0) . reachedCalls) (obsReached observation)
    , qualifiedMilestones = obsMilestones observation
    }

reachedValue :: Reached -> Value
reachedValue row = object
  [ "domain" .= domainLabel (reachedDomain row), "slot" .= ("0x" ++ showHex (slotOffset (reachedSlot row)) "")
  , "calls" .= reachedCalls row, "returns" .= reachedReturns row, "pending" .= reachedPending row ]

milestoneValue :: Milestone -> Value
milestoneValue milestone = object
  [ "phase" .= milestonePhase milestone, "key" .= milestoneKey milestone
  , "expected" .= menuValue (milestoneExpected milestone), "observed" .= fmap menuValue (milestoneObserved milestone) ]
 where
  menuValue (MenuTriple menu setting cursor) = [menu, setting, cursor]

qualifiedValue :: Qualified -> Value
qualifiedValue result = object
  [ "role" .= roleWire (qualifiedRole result)
  , "checked_calls" .= qualifiedCalls result
  , "classes" .= qualifiedClasses result
  , "pending" .= qualifiedPending result
  , "reached" .= map reachedValue (qualifiedReached result)
  , "milestones" .= map milestoneValue (qualifiedMilestones result)
  ]

-- | A route step whose key, expected menu state or observed menu state differs between the runs.
data RouteDifference = RouteDifference {differenceIndex :: Int, differenceStock :: Maybe Milestone, differenceOwned :: Maybe Milestone}
  deriving (Eq, Show)

compareRoutes :: Qualified -> Qualified -> [RouteDifference]
compareRoutes stock owned =
  [ RouteDifference index left right
  | index <- [0 .. max (length stockSteps) (length ownedSteps) - 1]
  , let left = at stockSteps index
        right = at ownedSteps index
  , fmap step left /= fmap step right ]
 where
  stockSteps = qualifiedMilestones stock
  ownedSteps = qualifiedMilestones owned
  step milestone = (milestonePhase milestone, milestoneKey milestone, milestoneExpected milestone, milestoneObserved milestone)
  at items index = if index < length items then Just (items !! index) else Nothing
