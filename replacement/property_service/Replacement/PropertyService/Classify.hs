{-# LANGUAGE GHC2021 #-}

-- | Which properties of the X-M1 1.50 getter 0x61e110 and setter 0x624694 the owned service takes,
-- derived from PropertyFields' per-path records of each jump-table arm.
module Replacement.PropertyService.Classify
  ( GuestAddress (..)
  , BankOffset (..)
  , RecordOffset (..)
  , TailOffset (..)
  , Width (..)
  , BitRange (..)
  , FieldDefault (..)
  , SavePolicy (..)
  , FieldArm (..)
  , BankDefault (..)
  , BankBranch (..)
  , RecordBranch (..)
  , SlotArm (..)
  , Owned (..)
  , Side (..)
  , Explored (..)
  , ShapeFault (..)
  , DelegationReason (..)
  , Class (..)
  , Property (..)
  , bankBase
  , saveBase
  , slotDefaultsBase
  , tailDefaultsBase
  , recordBase
  , recordStride
  , maxDefault
  , classify
  ) where

import Control.Monad (unless)
import Data.List (nub)
import qualified Data.Map.Strict as M
import Data.Word (Word16, Word32, Word8)
import PropertyFields
import ServiceInventory (Image)

newtype GuestAddress = GuestAddress Word32 deriving (Eq, Ord, Show)

-- | Offset of a field within one 0x1c2-byte bank image.
newtype BankOffset = BankOffset Word16 deriving (Eq, Ord, Show)

-- | Offset of a field within one 0x28-byte slot record, below 0x28.
newtype RecordOffset = RecordOffset Word8 deriving (Eq, Ord, Show)

-- | Offset of a byte from the defaults table 0x81b074.
newtype TailOffset = TailOffset Word16 deriving (Eq, Ord, Show)

data Width = Byte | Half deriving (Eq, Ord, Show)

-- | Unsigned bit range of a field within its byte or halfword.
data BitRange = BitRange { rangeLsb :: Int, rangeBits :: Int } deriving (Eq, Ord, Show)

-- | The constant the getter returns when its defaults flag (argument 2) is 1, which is also the value
-- the setter stores when argument 1 is 0x1affffff.
newtype FieldDefault = FieldDefault Word32 deriving (Eq, Ord, Show)

-- | Whether a bank-0 store first copies the field's current bank-0 bits into 0x89ec00 at the same offset.
data SavePolicy = SaveBank0Previous | NoSave deriving (Eq, Ord, Show)

data FieldArm = FieldArm
  { fieldOffset :: BankOffset
  , fieldWidth :: Width
  , fieldBits :: BitRange
  , fieldDefault :: FieldDefault
  , fieldSave :: SavePolicy
  }
  deriving (Eq, Show)

-- | The slot == 1 default: a constant, or the byte at 0x81b074 + offset read at run time.
data BankDefault = BankConstant FieldDefault | BankTailByte TailOffset deriving (Eq, Show)

-- | Slot == 1: the field at 0x96cc64 + 0x1c2 * bank + offset.
data BankBranch = BankBranch
  { bankOffset :: BankOffset
  , bankWidth :: Width
  , bankBits :: BitRange
  , bankDefault :: BankDefault
  }
  deriving (Eq, Show)

-- | Slot /= 1: the field at 0x96cc64 + 0x1c2 * bank + 0x140 + 0x28 * slot + offset, whose default
-- is the same bits at 0x7cb804 + 0x28 * slot + offset.
data RecordBranch = RecordBranch
  { recordOffset :: RecordOffset
  , recordWidth :: Width
  , recordBits :: BitRange
  }
  deriving (Eq, Show)

-- | A property whose field the slot argument (argument 3) selects. The save policy holds for both
-- branches; a record save goes to 0x89ec00 + 0x140 + 0x28 * slot + offset.
data SlotArm = SlotArm
  { bankArm :: BankBranch
  , recordArm :: RecordBranch
  , slotSave :: SavePolicy
  }
  deriving (Eq, Show)

-- | Owned behaviour: a field, a slot-selected field, or the shared fallback of both tables (get
-- returns 0, set returns 0 and stores nothing).
data Owned = OwnedField FieldArm | OwnedSlotField SlotArm | OwnedUnassigned deriving (Eq, Show)

data Side = Getter | Setter deriving (Eq, Ord, Show)

data Explored = GetterArms | SetterArms | SetterExits deriving (Eq, Ord, Show)

data ShapeFault
  = UnmodelledGuard
  | UnexpectedCall
  | UnexpectedStore
  | UnexpectedEnd
  | ModeNotCovered
  | FieldDisagrees
  | SignedField
  | DefaultFormUnrecognized
  | DefaultsDiffer
  | DefaultOutOfRange
  | -- | The bit range is empty or extends past its byte or halfword.
    FieldOutOfRange
  | -- | A slot record field does not lie within the 0x28-byte record.
    RecordOutOfRange
  | -- | The getter's slot default is not the record field's bits in the slot-defaults record.
    RecordDefaultDisagrees
  deriving (Eq, Ord, Show)

data DelegationReason
  = ExplorationIncomplete Explored
  | OneSidedFallback Side
  | ExitAdjusts
  | Shape Side ShapeFault
  deriving (Eq, Ord, Show)

data Class = Owned Owned | Delegated DelegationReason deriving (Eq, Show)

data Property = Property
  { propertyId :: PropertyId
  , propertyGetterArm :: GuestAddress
  , propertySetterArm :: GuestAddress
  , propertyClass :: Class
  }
  deriving (Eq, Show)

bankBase, bankStride, saveBase, slotDefaultsBase, tailDefaultsBase, recordBase, recordStride, maxDefault :: Word32
bankBase = 0x96cc64
bankStride = 0x1c2
saveBase = 0x89ec00
slotDefaultsBase = 0x7cb804
tailDefaultsBase = 0x81b074
-- | Offset of slot record 0 within a bank image.
recordBase = 0x140
recordStride = 0x28
-- | The largest default or tail offset the table encoding holds.
maxDefault = 0x3ff

-- | The getter post-dispatch the owned getter stub branches to, and the setter shared exit.
getterContinuationAddress, setterContinuationAddress :: Word32
getterContinuationAddress = 0x624400
setterContinuationAddress = 0x630200

-- | Every property of both tables with its class, or why the tables are not the expected form.
classify :: Image -> Either String [Property]
classify image = do
  (getterFallback, getters) <- either (Left . ("getter table: " ++)) Right (tableArms image getterTable)
  (setterFallback, setters) <- either (Left . ("setter table: " ++)) Right (tableArms image setterTable)
  if map armProperty getters /= map armProperty setters
    then Left "getter and setter tables cover different properties"
    else Right (zipWith (one getterFallback setterFallback) getters setters)
  where
    one getterFallback setterFallback getter setter =
      let getterShared = armTarget getter == getterFallback
          setterShared = armTarget setter == setterFallback
          identifier = armProperty getter
          verdict = decide getterShared setterShared (getterArm image getterTable getter) (setterArm image setterTable setter)
            (getterContinuation image identifier getterContinuationAddress) (setterContinuation image identifier setterContinuationAddress)
      in Property (armProperty getter) (GuestAddress (armTarget getter)) (GuestAddress (armTarget setter)) verdict

-- | The last two outcomes are the getter post-dispatch and the setter shared exit run alone for the
-- property; a slot arm needs both to pop at once, since its stock record branch pops without them.
decide :: Bool -> Bool -> Outcome -> Outcome -> Outcome -> Outcome -> Class
decide getterShared setterShared getter setter getterExit setterExit
  | not (outcomeComplete getter) = Delegated (ExplorationIncomplete GetterArms)
  | not (outcomeComplete setter) = Delegated (ExplorationIncomplete SetterArms)
  | not (all exitComplete exits) = Delegated (ExplorationIncomplete SetterExits)
  | not (all plainExit [(path, exit) | path <- outcomePaths setter, Just exit <- [pathExit path]]) = Delegated ExitAdjusts
  | getterShared && setterShared = either (Delegated . uncurry Shape) (const (Owned OwnedUnassigned)) (unassigned getter setter)
  | getterShared = Delegated (OneSidedFallback Getter)
  | setterShared = Delegated (OneSidedFallback Setter)
  | slotSelected getter, not (bareExit getterExit) || (any (returns . pathEnd) (outcomePaths setter) && not (bareExit setterExit)) = Delegated ExitAdjusts
  | slotSelected getter = either (Delegated . uncurry Shape) (Owned . OwnedSlotField) (slotField getter setter)
  | otherwise = either (Delegated . uncurry Shape) (Owned . OwnedField) (field getter setter)
  where
    exits = [exit | path <- outcomePaths setter, Just exit <- [pathExit path]]
    returns end = case end of
      EndReturn _ -> True
      _ -> False
    bareExit outcome = outcomeComplete outcome && outcomePaths outcome == [PathRecord [] [] [] (EndReturn ReturnsIncoming) Nothing]

-- | The exit pops at once with r0 as the arm left it, deciding nothing beyond the arm path's guards.
plainExit :: (PathRecord, ExitRecord) -> Bool
plainExit (path, exit) = case exitPaths exit of
  [PathRecord guards [] [] (EndReturn ReturnsIncoming) Nothing] -> guards == pathGuards path
  _ -> False

-- | A path's stores when every one is modelled.
modelledStores :: PathRecord -> Either ShapeFault [StoreRecord]
modelledStores path = mapM modelled (pathStores path)
  where
    modelled stored = case stored of
      Modelled record -> Right record
      Unmodelled _ -> Left UnexpectedStore

unassigned :: Outcome -> Outcome -> Either (Side, ShapeFault) ()
unassigned getter setter = do
  mapM_ (bare Getter (EndReturn (ReturnsConstant 0))) (outcomePaths getter)
  mapM_ (bare Setter (EndExit (ReturnsConstant 0))) (outcomePaths setter)
  where
    bare side end (PathRecord _ stores calls actual _)
      | not (null calls) = Left (side, UnexpectedCall)
      | not (null stores) = Left (side, UnexpectedStore)
      | actual /= end = Left (side, UnexpectedEnd)
      | otherwise = Right ()

-- | A comparison a path assumed, keyed so that two paths' assumptions about one comparison can be compared.
data Assumption = Equals GuardSubject Word32 Bool | RawCondition GuardSubject (Maybe Word32) GuardCondition Bool deriving (Eq, Ord)

assumption :: Guard -> Assumption
assumption (Guard subject constant cond holds) = case (cond, constant) of
  (CondEqual, Just c) -> Equals subject c holds
  (CondNotEqual, Just c) -> Equals subject c (not holds)
  _ -> RawCondition subject constant cond holds

-- | The truth of `subject == constant` on a path, when the path compared it.
truthOf :: GuardSubject -> Word32 -> PathRecord -> Either ShapeFault (Maybe Bool)
truthOf subject constant path = case nub [t | Equals s c t <- map assumption (pathGuards path), s == subject, c == constant] of
  [] -> Right Nothing
  [t] -> Right (Just t)
  _ -> Left UnmodelledGuard

-- | Only the listed equality comparisons may decide a path.
onlyGuards :: [(GuardSubject, Word32)] -> PathRecord -> Bool
onlyGuards allowed path = all permitted (map assumption (pathGuards path))
  where
    permitted (Equals s c _) = (s, c) `elem` allowed
    permitted _ = False

-- | The paths covering each value of the listed comparisons, or a fault when a combination has none.
-- A path that does not compare a subject covers both of its values.
covering :: [(GuardSubject, Word32)] -> [PathRecord] -> Either ShapeFault [([Bool], [PathRecord])]
covering subjects paths = do
  truths <- mapM (\path -> (,) path <$> mapM (\(s, c) -> truthOf s c path) subjects) paths
  let combinations = sequence [[False, True] | _ <- subjects]
      matches combination = [path | (path, known) <- truths, and (zipWith (\want got -> maybe True (== want) got) combination known)]
      grouped = [(combination, matches combination) | combination <- combinations]
  if any (null . snd) grouped then Left ModeNotCovered else Right grouped

-- | The one end the paths share.
single :: [PathEnd] -> Either ShapeFault PathEnd
single ends = case nub ends of
  [end] -> Right end
  _ -> Left UnexpectedEnd

-- | The offset of a location's address from a region base and its width, when its terms are exactly `terms`.
offsetIn :: Word32 -> [(ArgumentIndex, Word32)] -> Loc -> Maybe (Word32, Width)
offsetIn base terms (Loc locationBase actual offset bytes)
  | actual == terms = (,) (locationBase + fromIntegral offset - base) <$> widthOf bytes
  | otherwise = Nothing

bankTerm :: ArgumentIndex -> [(ArgumentIndex, Word32)]
bankTerm index = [(index, bankStride)]

slotArgument :: ArgumentIndex
slotArgument = ArgumentIndex 3

slotTerm :: [(ArgumentIndex, Word32)]
slotTerm = [(slotArgument, recordStride)]

-- | Bank and slot terms in argument order; the bank argument is 1 or 2, below the slot's 3.
recordTerms :: ArgumentIndex -> [(ArgumentIndex, Word32)]
recordTerms index = bankTerm index ++ slotTerm

bankField :: ArgumentIndex -> Loc -> Maybe (BankOffset, Width)
bankField index loc = case offsetIn bankBase (bankTerm index) loc of
  Just (offset, width) | offset < bankStride -> Just (BankOffset (fromIntegral offset), width)
  _ -> Nothing

widthOf :: Int -> Maybe Width
widthOf bytes = case bytes of
  1 -> Just Byte
  2 -> Just Half
  _ -> Nothing

widthBytes :: Width -> Word32
widthBytes width = case width of
  Byte -> 1
  Half -> 2

rangeOf :: Extract -> Either ShapeFault BitRange
rangeOf (Extract lsb bits signed)
  | signed = Left SignedField
  | otherwise = Right (BitRange lsb bits)

-- | The bit range when it is non-empty and within its byte or halfword.
inContainer :: Width -> BitRange -> Either ShapeFault BitRange
inContainer width bits
  | rangeBits bits < 1 || rangeLsb bits + rangeBits bits > 8 * fromIntegral (widthBytes width) = Left FieldOutOfRange
  | otherwise = Right bits

extractOfRange :: BitRange -> Extract
extractOfRange (BitRange lsb count) = Extract lsb count False

-- | A store inserts the low bits of its source, so a source supplies the field when its extract
-- starts at the field's lsb and is at least as wide.
supplies :: BitRange -> Extract -> Bool
supplies bits (Extract sourceLsb sourceBits signed) = not signed && sourceLsb == rangeLsb bits && sourceBits >= rangeBits bits

field :: Outcome -> Outcome -> Either (Side, ShapeFault) FieldArm
field getter setter = do
  (offset, width, bits, getDefault) <- tag Getter (getterField (outcomePaths getter))
  _ <- tag Getter (inContainer width bits)
  (setDefault, save) <- tag Setter (setterField offset width bits (outcomePaths setter))
  if getDefault /= setDefault then Left (Setter, DefaultsDiffer)
    else if getDefault > maxDefault then Left (Getter, DefaultOutOfRange)
    else pure (FieldArm offset width bits (FieldDefault getDefault) save)

tag :: Side -> Either ShapeFault a -> Either (Side, ShapeFault) a
tag side = either (Left . (,) side) Right

-- | The live field read with argument 2 /= 1 and the default read with argument 2 == 1.
getterField :: [PathRecord] -> Either ShapeFault (BankOffset, Width, BitRange, Word32)
getterField paths = do
  mapM_ (\path -> if not (null (pathCalls path)) then Left UnexpectedCall else if not (null (pathStores path)) then Left UnexpectedStore else Right ()) paths
  if all (onlyGuards [(GuardArgument (ArgumentIndex 2), 1)]) paths then Right () else Left UnmodelledGuard
  grouped <- covering [(GuardArgument (ArgumentIndex 2), 1)] paths
  results <- mapM (\(combination, matching) -> (,) combination <$> single (map pathEnd matching)) grouped
  (live, defaulted) <- case results of
    [([False], l), ([True], d)] -> Right (l, d)
    _ -> Left ModeNotCovered
  (offset, width, bits) <- case live of
    EndReturn (ReturnsField loc extract) | Just (o, w) <- bankField (ArgumentIndex 1) loc -> (,,) o w <$> rangeOf extract
    _ -> Left UnexpectedEnd
  getDefault <- case defaulted of
    EndReturn (ReturnsConstant c) -> Right c
    _ -> Left DefaultFormUnrecognized
  pure (offset, width, bits, getDefault)

-- | The setter's store sequence for each value of (argument 1 is the default request, argument 2 == 0).
setterField :: BankOffset -> Width -> BitRange -> [PathRecord] -> Either ShapeFault (Word32, SavePolicy)
setterField (BankOffset offsetWord) width bits paths = do
  mapM_ (\path -> if not (null (pathCalls path)) then Left UnexpectedCall else if pathEnd path /= EndExit (ReturnsConstant 1) then Left UnexpectedEnd else Right ()) paths
  if all (onlyGuards [(GuardDefaultRequested, 0), (GuardArgument (ArgumentIndex 2), 0)]) paths then Right () else Left UnmodelledGuard
  grouped <- covering [(GuardDefaultRequested, 0), (GuardArgument (ArgumentIndex 2), 0)] paths
  combinationsOf <- storeSequences grouped
  -- The default-request flag == 0 holds when no default was requested.
  let at notDefault bank0 = M.findWithDefault [] [notDefault, bank0] combinationsOf
  setDefault <- case reverse (at False True) of
    StoreRecord _ _ (FromConstant c) : _ -> Right c
    _ -> Left DefaultFormUnrecognized
  let target = StoreTarget (offsetIn bankBase (bankTerm (ArgumentIndex 2))) (offsetIn saveBase []) (offsetIn bankBase []) (fromIntegral offsetWord) width bits
      fromValue actual = actual == FromArgument (ArgumentIndex 1)
      fromDefault actual = actual == FromConstant setDefault
      matches policy = and [sequenceMatches target policy (if notDefault then fromValue else fromDefault) bank0 (at notDefault bank0) | notDefault <- [False, True], bank0 <- [False, True]]
  if matches SaveBank0Previous then Right (setDefault, SaveBank0Previous)
    else if matches NoSave then Right (setDefault, NoSave)
    else Left FieldDisagrees

-- | The one modelled store sequence of each guard combination.
storeSequences :: [([Bool], [PathRecord])] -> Either ShapeFault (M.Map [Bool] [StoreRecord])
storeSequences grouped = M.fromList <$> mapM (\(combination, matching) -> mapM modelledStores matching >>= \stores -> case nub stores of
  [one] -> Right (combination, one)
  _ -> Left UnexpectedStore) grouped

-- | Where a setter branch stores and saves: the store's location, the save's location and its
-- source's location, each matched to the same offset, width and bit range.
data StoreTarget = StoreTarget
  { targetStore :: Loc -> Maybe (Word32, Width)
  , targetSave :: Loc -> Maybe (Word32, Width)
  , targetSaveSource :: Loc -> Maybe (Word32, Width)
  , targetOffset :: Word32
  , targetWidth :: Width
  , targetBits :: BitRange
  }

-- | A combination's stores under a save policy: on bank 0 with a save, the save and then the store,
-- otherwise the store alone, which inserts the source `value` accepts.
sequenceMatches :: StoreTarget -> SavePolicy -> (Source -> Bool) -> Bool -> [StoreRecord] -> Bool
sequenceMatches target policy value bank0 stores = case (policy, bank0, stores) of
  (SaveBank0Previous, True, [save, store]) -> isSave save && isStore store
  (_, _, [store]) | policy == NoSave || not bank0 -> isStore store
  _ -> False
  where
    placed locate loc = locate loc == Just (targetOffset target, targetWidth target)
    extract = extractOfRange (targetBits target)
    isSave (StoreRecord loc e (FromField source se)) =
      placed (targetSave target) loc && e == extract && placed (targetSaveSource target) source && supplies (targetBits target) se
    isSave _ = False
    isStore (StoreRecord loc e actual) = placed (targetStore target) loc && e == extract && value actual

-- | Whether the getter compares the slot argument.
slotSelected :: Outcome -> Bool
slotSelected getter = or [s == GuardArgument slotArgument | path <- outcomePaths getter, Guard s _ _ _ <- pathGuards path]

slotField :: Outcome -> Outcome -> Either (Side, ShapeFault) SlotArm
slotField getter setter = do
  (bank, record) <- tag Getter (slotGetter (outcomePaths getter))
  save <- tag Setter (slotSetter bank record (outcomePaths setter))
  pure (SlotArm bank record save)

-- | The four ends of (argument 3 == 1, argument 2 == 1): record field, record default, bank field, bank default.
slotGetter :: [PathRecord] -> Either ShapeFault (BankBranch, RecordBranch)
slotGetter paths = do
  mapM_ (\path -> if not (null (pathCalls path)) then Left UnexpectedCall else if not (null (pathStores path)) then Left UnexpectedStore else Right ()) paths
  let subjects = [(GuardArgument slotArgument, 1), (GuardArgument (ArgumentIndex 2), 1)]
  unless (all (onlyGuards subjects) paths) (Left UnmodelledGuard)
  grouped <- covering subjects paths
  ends <- mapM (\(combination, matching) -> (,) combination <$> single (map pathEnd matching)) grouped
  (recordLive, recordDefault, bankLive, bankDefaulted) <- case ends of
    [([False, False], a), ([False, True], b), ([True, False], c), ([True, True], d)] -> Right (a, b, c, d)
    _ -> Left ModeNotCovered
  (offset, width, bits) <- case bankLive of
    EndReturn (ReturnsField loc extract) | Just (o, w) <- bankField (ArgumentIndex 1) loc -> (,,) o w <$> (rangeOf extract >>= inContainer w)
    _ -> Left UnexpectedEnd
  fallback <- case bankDefaulted of
    EndReturn (ReturnsConstant c)
      | c <= maxDefault -> Right (BankConstant (FieldDefault c))
      | otherwise -> Left DefaultOutOfRange
    EndReturn (ReturnsField loc extract)
      | Just (t, Byte) <- offsetIn tailDefaultsBase [] loc, extract == Extract 0 8 False ->
          if t <= maxDefault then Right (BankTailByte (TailOffset (fromIntegral t))) else Left DefaultOutOfRange
    _ -> Left DefaultFormUnrecognized
  (recordAt, recordWidth', recordBits') <- case recordLive of
    EndReturn (ReturnsField loc extract) | Just (o, w) <- offsetIn bankBase (recordTerms (ArgumentIndex 1)) loc -> do
      r <- rangeOf extract >>= inContainer w
      let b = o - recordBase
      if o >= recordBase && b < recordStride && b + widthBytes w <= recordStride then Right (b, w, r) else Left RecordOutOfRange
    _ -> Left UnexpectedEnd
  case recordDefault of
    EndReturn (ReturnsField loc extract)
      | offsetIn slotDefaultsBase slotTerm loc == Just (recordAt, recordWidth'), extract == extractOfRange recordBits' -> Right ()
    _ -> Left RecordDefaultDisagrees
  pure (BankBranch offset width bits fallback, RecordBranch (RecordOffset (fromIntegral recordAt)) recordWidth' recordBits')

-- | The setter's store sequence for each value of (argument 3 == 1, argument 1 is the default
-- request, argument 2 == 0), under one save policy for both branches.
slotSetter :: BankBranch -> RecordBranch -> [PathRecord] -> Either ShapeFault SavePolicy
slotSetter (BankBranch (BankOffset bankAt) bankWidth' bankBits' fallback) (RecordBranch (RecordOffset recordAt) recordWidth' recordBits') paths = do
  mapM_ (\path -> if not (null (pathCalls path)) then Left UnexpectedCall else if pathEnd path `notElem` [EndExit (ReturnsConstant 1), EndReturn (ReturnsConstant 1)] then Left UnexpectedEnd else Right ()) paths
  let subjects = [(GuardArgument slotArgument, 1), (GuardDefaultRequested, 0), (GuardArgument (ArgumentIndex 2), 0)]
  unless (all (onlyGuards subjects) paths) (Left UnmodelledGuard)
  combinationsOf <- covering subjects paths >>= storeSequences
  let at slot1 notDefault bank0 = M.findWithDefault [] [slot1, notDefault, bank0] combinationsOf
      bank = StoreTarget (offsetIn bankBase (bankTerm (ArgumentIndex 2))) (offsetIn saveBase []) (offsetIn bankBase []) (fromIntegral bankAt) bankWidth' bankBits'
      recordOffsetValue = recordBase + fromIntegral recordAt
      record = StoreTarget (offsetIn bankBase (recordTerms (ArgumentIndex 2))) (offsetIn saveBase slotTerm) (offsetIn bankBase slotTerm) recordOffsetValue recordWidth' recordBits'
      fromValue actual = actual == FromArgument (ArgumentIndex 1)
      bankDefaultSource actual = case (fallback, actual) of
        (BankConstant (FieldDefault c), FromConstant stored) -> stored == c
        (BankTailByte (TailOffset t), FromField loc extract) -> offsetIn tailDefaultsBase [] loc == Just (fromIntegral t, Byte) && extract == Extract 0 8 False
        _ -> False
      recordDefaultSource actual = case actual of
        FromField loc extract -> offsetIn slotDefaultsBase slotTerm loc == Just (fromIntegral recordAt, recordWidth') && supplies recordBits' extract
        _ -> False
      branch slot1 = if slot1 then (bank, bankDefaultSource) else (record, recordDefaultSource)
      matches policy = and
        [ sequenceMatches target policy (if notDefault then fromValue else defaulted) bank0 (at slot1 notDefault bank0)
        | slot1 <- [False, True], let (target, defaulted) = branch slot1, notDefault <- [False, True], bank0 <- [False, True]
        ]
  if matches SaveBank0Previous then Right SaveBank0Previous
    else if matches NoSave then Right NoSave
    else Left FieldDisagrees
