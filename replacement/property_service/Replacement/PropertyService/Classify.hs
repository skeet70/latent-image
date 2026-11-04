{-# LANGUAGE GHC2021 #-}

-- | Which properties of the X-M1 1.50 getter 0x61e110 and setter 0x624694 the owned service takes,
-- derived from PropertyFields' per-path records of each jump-table arm.
module Replacement.PropertyService.Classify
  ( GuestAddress (..)
  , BankOffset (..)
  , Width (..)
  , BitRange (..)
  , FieldDefault (..)
  , SavePolicy (..)
  , FieldArm (..)
  , Owned (..)
  , Side (..)
  , Explored (..)
  , ShapeFault (..)
  , DelegationReason (..)
  , Class (..)
  , Property (..)
  , bankBase
  , saveBase
  , classify
  ) where

import Data.List (nub)
import qualified Data.Map.Strict as M
import Data.Word (Word16, Word32)
import PropertyFields
import ServiceInventory (Image)

newtype GuestAddress = GuestAddress Word32 deriving (Eq, Ord, Show)

-- | Offset of a field within one 0x1c2-byte bank image.
newtype BankOffset = BankOffset Word16 deriving (Eq, Ord, Show)

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

-- | Owned behaviour: a field, or the shared fallback of both tables (get returns 0, set returns 0 and stores nothing).
data Owned = OwnedField FieldArm | OwnedUnassigned deriving (Eq, Show)

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

bankBase, bankStride, saveBase, maxDefault :: Word32
bankBase = 0x96cc64
bankStride = 0x1c2
saveBase = 0x89ec00
-- | The largest default the table encoding holds.
maxDefault = 0x3ff

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
          verdict = decide getterShared setterShared (getterArm image getterTable getter) (setterArm image setterTable setter)
      in Property (armProperty getter) (GuestAddress (armTarget getter)) (GuestAddress (armTarget setter)) verdict

decide :: Bool -> Bool -> Outcome -> Outcome -> Class
decide getterShared setterShared getter setter
  | not (outcomeComplete getter) = Delegated (ExplorationIncomplete GetterArms)
  | not (outcomeComplete setter) = Delegated (ExplorationIncomplete SetterArms)
  | not (all exitComplete exits) = Delegated (ExplorationIncomplete SetterExits)
  | not (all plainExit [(path, exit) | path <- outcomePaths setter, Just exit <- [pathExit path]]) = Delegated ExitAdjusts
  | getterShared && setterShared = either (Delegated . uncurry Shape) (const (Owned OwnedUnassigned)) (unassigned getter setter)
  | getterShared = Delegated (OneSidedFallback Getter)
  | setterShared = Delegated (OneSidedFallback Setter)
  | otherwise = either (Delegated . uncurry Shape) (Owned . OwnedField) (field getter setter)
  where
    exits = [exit | path <- outcomePaths setter, Just exit <- [pathExit path]]

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

bankField :: ArgumentIndex -> Loc -> Maybe (BankOffset, Width)
bankField index (Loc base [(argument, stride)] offset bytes)
  | base == bankBase, stride == bankStride, argument == index, offset >= 0, offset < fromIntegral bankStride = (,) (BankOffset (fromIntegral offset)) <$> widthOf bytes
bankField _ _ = Nothing

widthOf :: Int -> Maybe Width
widthOf bytes = case bytes of
  1 -> Just Byte
  2 -> Just Half
  _ -> Nothing

rangeOf :: Extract -> Either ShapeFault BitRange
rangeOf (Extract lsb bits signed)
  | signed = Left SignedField
  | otherwise = Right (BitRange lsb bits)

field :: Outcome -> Outcome -> Either (Side, ShapeFault) FieldArm
field getter setter = do
  (offset, width, bits, getDefault) <- tag Getter (getterField (outcomePaths getter))
  if rangeBits bits < 1 || rangeLsb bits + rangeBits bits > containerBits width then Left (Getter, FieldOutOfRange) else pure ()
  (setDefault, save) <- tag Setter (setterField offset width bits (outcomePaths setter))
  if getDefault /= setDefault then Left (Setter, DefaultsDiffer)
    else if getDefault > maxDefault then Left (Getter, DefaultOutOfRange)
    else pure (FieldArm offset width bits (FieldDefault getDefault) save)
  where
    tag side = either (Left . (,) side) Right
    containerBits Byte = 8
    containerBits Half = 16

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
  where
    single ends = case nub ends of
      [end] -> Right end
      _ -> Left UnexpectedEnd

-- | The setter's store sequence for each value of (argument 1 is the default request, argument 2 == 0).
setterField :: BankOffset -> Width -> BitRange -> [PathRecord] -> Either ShapeFault (Word32, SavePolicy)
setterField offset width bits paths = do
  mapM_ (\path -> if not (null (pathCalls path)) then Left UnexpectedCall else if pathEnd path /= EndExit (ReturnsConstant 1) then Left UnexpectedEnd else Right ()) paths
  if all (onlyGuards [(GuardDefaultRequested, 0), (GuardArgument (ArgumentIndex 2), 0)]) paths then Right () else Left UnmodelledGuard
  grouped <- covering [(GuardDefaultRequested, 0), (GuardArgument (ArgumentIndex 2), 0)] paths
  sequences <- mapM (\(combination, matching) -> mapM modelledStores matching >>= \stores -> case nub stores of
      [one] -> Right (combination, one)
      _ -> Left UnexpectedStore) grouped
  let combinationsOf = M.fromList sequences
      -- The default-request flag == 0 holds when no default was requested.
      at notDefault bank0 = M.findWithDefault [] [notDefault, bank0] combinationsOf
      -- A store inserts the low bits of its source, so a source supplies the field when its extract
      -- starts at the field's lsb and is at least as wide.
      supplies (Extract sourceLsb sourceBits signed) = not signed && sourceLsb == rangeLsb bits && sourceBits >= rangeBits bits
      isSave (StoreRecord (Loc base [] o b) e (FromField (Loc sourceBase [] so sb) se)) =
        base == saveBase && o == offsetValue && b == bytes && e == extract && sourceBase == bankBase && so == offsetValue && sb == bytes && supplies se
      isSave _ = False
      isBank source (StoreRecord loc e actual) = loc == bankLoc (ArgumentIndex 2) && e == extract && source actual
      fromValue actual = actual == FromArgument (ArgumentIndex 1)
  setDefault <- case reverse (at False True) of
    StoreRecord _ _ (FromConstant c) : _ -> Right c
    _ -> Left DefaultFormUnrecognized
  let fromDefault actual = actual == FromConstant setDefault
      sequenceMatches policy notDefault bank0 = case (policy, bank0, at notDefault bank0) of
        (SaveBank0Previous, True, [save, store]) -> isSave save && isBank (if notDefault then fromValue else fromDefault) store
        (_, _, [store]) | policy == NoSave || not bank0 -> isBank (if notDefault then fromValue else fromDefault) store
        _ -> False
      matches policy = and [sequenceMatches policy notDefault bank0 | notDefault <- [False, True], bank0 <- [False, True]]
  if matches SaveBank0Previous then Right (setDefault, SaveBank0Previous)
    else if matches NoSave then Right (setDefault, NoSave)
    else Left FieldDisagrees
  where
    BankOffset offsetWord = offset
    offsetValue = fromIntegral offsetWord
    bytes = case width of
      Byte -> 1
      Half -> 2
    extract = case bits of
      BitRange lsb count -> Extract lsb count False
    bankLoc index = Loc bankBase [(index, bankStride)] offsetValue bytes
