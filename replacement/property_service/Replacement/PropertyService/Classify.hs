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
  , ProfileSelector (..)
  , StockDefault (..)
  , ConfigOffset (..)
  , FieldHook (..)
  , FieldSource (..)
  , SavePolicy (..)
  , FieldArm (..)
  , BankDefault (..)
  , BankBranch (..)
  , RecordBranch (..)
  , SlotDefault (..)
  , MirrorPolicy (..)
  , SlotArm (..)
  , PairArm (..)
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
  , configBase
  , configBytes
  , recordBase
  , recordStride
  , maxDefault
  , maxTailOffset
  , stockDefaultOf
  , classify
  ) where

import Control.Applicative ((<|>))
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

-- | What the getter returns when its defaults flag (argument 2) is 1, which is also the value the
-- setter stores when argument 1 is 0x1affffff: a constant, or the field's container read at
-- 0x81b074 + offset at run time.
data FieldDefault = DefaultConstant Word32 | DefaultTail TailOffset Width deriving (Eq, Ord, Show)

-- | 0x61d65c's argument, which selects the byte it reads: 0x96b481, 0x96b482 or 0x96b483.
data ProfileSelector = Profile2 | Profile3 | Profile4 deriving (Eq, Ord, Show, Enum, Bounded)

-- | A stock leaf function whose result is a default. Each reads one byte of 0x96b400 and nothing else.
data StockDefault = Stock61df44 | Stock61d8a4 | Stock61d59c | Stock61d618 | Stock61d5f0 | Stock61d65c ProfileSelector
  deriving (Eq, Ord, Show)

-- | Offset of a byte from 0x96b400, below 0x200.
newtype ConfigOffset = ConfigOffset Word16 deriving (Eq, Ord, Show)

-- | What surrounds or follows a set's save and store: nothing; on bank 0, the stock open 0x61da3c
-- before and close 0x61d944 after, both with r0 = 0x96cc64; or, when the byte at 0x96b400 + offset
-- is 0, the same save and store again.
data FieldHook = NoHook | Bank0Bracket | ConfigGatedResave ConfigOffset deriving (Eq, Ord, Show)

-- | Where the default comes from. For a stock or configuration default, a default request first
-- stores the provisional constant (the Word32) with the field's save; then a stock default saves
-- again and stores the leaf's result, and a configuration default stores the byte's low bits without
-- a save. The getter's default is the leaf's result or the whole configuration byte.
data FieldSource = FieldDefaultOnly FieldDefault | FieldStock StockDefault Word32 | FieldConfig ConfigOffset Word32
  deriving (Eq, Ord, Show)

-- | Whether a bank-0 store first copies the field's current bank-0 bits into 0x89ec00 at the same offset.
data SavePolicy = SaveBank0Previous | NoSave deriving (Eq, Ord, Show)

data FieldArm = FieldArm
  { fieldOffset :: BankOffset
  , fieldWidth :: Width
  , fieldBits :: BitRange
  , fieldSource :: FieldSource
  , fieldSave :: SavePolicy
  , fieldHook :: FieldHook
  }
  deriving (Eq, Show)

-- | The slot == 1 default: a constant, or the byte at 0x81b074 + offset read at run time.
data BankDefault = BankConstant Word32 | BankTailByte TailOffset deriving (Eq, Show)

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

-- | The default of both branches: the stage-2 defaults (bank constant or tail byte, slot-defaults
-- record), or a stock leaf's result. With a stock default, a default request stores the stage-2
-- default as a provisional value, then saves again and stores the leaf's result.
data SlotDefault = SlotDefaultsRecord | SlotStock StockDefault deriving (Eq, Show)

-- | Whether the getter reads slot record 0 for every slot and a set ends by writing record 0 the
-- way the record branch writes its slot's record.
data MirrorPolicy = NoMirror | MirrorRecord0 deriving (Eq, Show)

-- | A property whose field the slot argument (argument 3) selects. The save policy holds for both
-- branches; a record save goes to 0x89ec00 + 0x140 + 0x28 * slot + offset.
data SlotArm = SlotArm
  { bankArm :: BankBranch
  , recordArm :: RecordBranch
  , slotSave :: SavePolicy
  , slotDefault :: SlotDefault
  , slotMirror :: MirrorPolicy
  }
  deriving (Eq, Show)

-- | Two whole bytes a set stores with one value: on bank 0 the first between the open and close
-- calls and the second after the close. Get returns 0.
data PairArm = PairArm
  { pairFirst :: BankOffset
  , pairSecond :: BankOffset
  , pairDefault :: Word32
  , pairSave :: SavePolicy
  }
  deriving (Eq, Show)

-- | Owned behaviour: a field, a slot-selected field, a field whose set is the shared fallback (it
-- returns 0 and stores nothing), a write-only byte pair, or the shared fallback of both tables (get
-- returns 0 too).
data Owned = OwnedField FieldArm | OwnedSlotField SlotArm | OwnedReadOnlyField FieldArm | OwnedWriteOnlyPair PairArm | OwnedUnassigned deriving (Eq, Show)

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
-- | The largest constant default the table encoding holds.
maxDefault = 0x1ff

-- | The largest tail offset a property word holds.
maxTailOffset :: Word32
maxTailOffset = 0xff

-- | The configuration bytes the stock leaves and the configuration defaults read: [0x96b400, 0x96b600).
configBase :: Word32
configBase = 0x96b400

configBytes :: Word32
configBytes = 0x200

-- | The stock leaf a call record names, with 0x61d65c's selector from its r0.
stockDefaultOf :: CallRecord -> Maybe StockDefault
stockDefaultOf (CallRecord target arguments) = case target of
  0x61df44 -> Just Stock61df44
  0x61d8a4 -> Just Stock61d8a4
  0x61d59c -> Just Stock61d59c
  0x61d618 -> Just Stock61d618
  0x61d5f0 -> Just Stock61d5f0
  0x61d65c -> case lookup 0 arguments of
    Just (Const 2) -> Just (Stock61d65c Profile2)
    Just (Const 3) -> Just (Stock61d65c Profile3)
    Just (Const 4) -> Just (Stock61d65c Profile4)
    _ -> Nothing
  _ -> Nothing

-- | The getter post-dispatch and the setter shared exit.
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
  | not (all plainExit [(path, exit) | path <- outcomePaths setter, Just exit <- [pathExit path]]) =
      if slotSelected getter
        then either (Delegated . uncurry Shape) (Owned . OwnedSlotField) (adjustedSlot getter setter)
        else either (Delegated . uncurry Shape) (Owned . OwnedField) (adjustedField getter setter)
  | any (any (callsTo bracketOpen) . pathEvents) (outcomePaths setter) =
      if getterShared
        then either (Delegated . uncurry Shape) (Owned . OwnedWriteOnlyPair) (writeOnlyPair getter setter)
        else either (Delegated . uncurry Shape) (Owned . OwnedField) (bracketedField getter setter)
  | getterShared && setterShared = either (Delegated . uncurry Shape) (const (Owned OwnedUnassigned)) (unassigned getter setter)
  | getterShared = Delegated (OneSidedFallback Getter)
  | setterShared = either (Delegated . uncurry Shape) (Owned . OwnedReadOnlyField) (readOnly getter setter)
  | slotSelected getter, not (bareExit getterExit) || (any (returns . pathEnd) (outcomePaths setter) && not (bareExit setterExit)) = Delegated ExitAdjusts
  | slotSelected getter = either (Delegated . uncurry Shape) (Owned . OwnedSlotField) (slotField getter setter)
  | otherwise = either (Delegated . uncurry Shape) (Owned . OwnedField) (field getter setter)
  where
    exits = [exit | path <- outcomePaths setter, Just exit <- [pathExit path]]
    returns end = case end of
      EndReturn _ -> True
      _ -> False
    bareExit outcome = outcomeComplete outcome && outcomePaths outcome == [PathRecord [] [] (EndReturn ReturnsIncoming) Nothing]

-- | The exit pops at once with r0 as the arm left it, deciding nothing beyond the arm path's guards.
plainExit :: (PathRecord, ExitRecord) -> Bool
plainExit (path, exit) = case exitPaths exit of
  [PathRecord guards [] (EndReturn ReturnsIncoming) Nothing] -> guards == pathGuards path
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
  sharedSetter setter

-- | The setter's shared fallback: r0 = 0 and no store or call.
sharedSetter :: Outcome -> Either (Side, ShapeFault) ()
sharedSetter setter = mapM_ (bare Setter (EndExit (ReturnsConstant 0))) (outcomePaths setter)

bare :: Side -> PathEnd -> PathRecord -> Either (Side, ShapeFault) ()
bare side end path@(PathRecord _ _ actual _)
  | not (null (pathCalls path)) = Left (side, UnexpectedCall)
  | not (null (pathStores path)) = Left (side, UnexpectedStore)
  | actual /= end = Left (side, UnexpectedEnd)
  | otherwise = Right ()

-- | Class 9: a field getter with a constant default and the shared setter fallback.
readOnly :: Outcome -> Outcome -> Either (Side, ShapeFault) FieldArm
readOnly getter setter = do
  (offset, width, bits, getDefault) <- tag Getter (getterField (outcomePaths getter))
  _ <- tag Getter (inContainer width bits)
  value <- case getDefault of
    GetsDefault (DefaultConstant c) | c <= maxDefault -> Right c
    GetsDefault (DefaultConstant _) -> Left (Getter, DefaultOutOfRange)
    _ -> Left (Getter, DefaultFormUnrecognized)
  sharedSetter setter
  pure (FieldArm offset width bits (FieldDefaultOnly (DefaultConstant value)) NoSave NoHook)

-- | A comparison a path assumed, keyed so that two paths' assumptions about one comparison can be
-- compared. The default-request flag is 0 or 1, so a comparison with 1 is keyed as the opposite
-- comparison with 0.
data Assumption = Equals GuardSubject Word32 Bool | RawCondition GuardSubject (Maybe Word32) GuardCondition Bool deriving (Eq, Ord)

assumption :: Guard -> Assumption
assumption (Guard subject constant cond holds) = case (cond, constant) of
  (CondEqual, Just c) -> equals c holds
  (CondNotEqual, Just c) -> equals c (not holds)
  _ -> RawCondition subject constant cond holds
  where
    equals c truth
      | subject == GuardDefaultRequested && c == 1 = Equals subject 0 (not truth)
      | otherwise = Equals subject c truth

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
  (offset, width, bits, gets) <- tag Getter (getterField (outcomePaths getter))
  _ <- tag Getter (inContainer width bits)
  getDefault <- case gets of
    GetsDefault fallback -> Right fallback
    _ -> Left (Getter, DefaultFormUnrecognized)
  (setDefault, save) <- tag Setter (setterField offset width bits (outcomePaths setter))
  if getDefault /= setDefault then Left (Setter, DefaultsDiffer) else Right ()
  case getDefault of
    DefaultConstant c | c > maxDefault -> Left (Getter, DefaultOutOfRange)
    DefaultTail (TailOffset t) w
      | w /= width -> Left (Getter, DefaultFormUnrecognized)
      | fromIntegral t > maxTailOffset -> Left (Getter, DefaultOutOfRange)
    _ -> pure (FieldArm offset width bits (FieldDefaultOnly getDefault) save NoHook)

-- | Classes 2, 4 and 5: a field whose shared exit computes the default or writes the field again.
adjustedField :: Outcome -> Outcome -> Either (Side, ShapeFault) FieldArm
adjustedField getter setter = do
  (offset, width, bits, gets) <- tag Getter (getterField (outcomePaths getter))
  _ <- tag Getter (inContainer width bits)
  tag Setter (adjustedSetter offset width bits gets (concatMap joined (outcomePaths setter)))

tag :: Side -> Either ShapeFault a -> Either (Side, ShapeFault) a
tag side = either (Left . (,) side) Right

-- | The stock functions that refresh bank-0 bytes [0x96cc64, 0x96cc74) from backup memory and write
-- them back.
bracketOpen, bracketClose :: Word32
bracketOpen = 0x61da3c
bracketClose = 0x61d944

callsTo :: Word32 -> PathEvent -> Bool
callsTo target event = case event of
  EventCall called -> callTarget called == target
  EventStore _ -> False

-- | A call of `target` with r0 = 0x96cc64.
bracketCall :: Word32 -> PathEvent -> Bool
bracketCall target event = callsTo target event && case event of
  EventCall called -> lookup 0 (callArguments called) == Just (Const bankBase)
  EventStore _ -> False

-- | The joined setter paths' event sequence for each value of (argument 1 is not the default
-- request, argument 2 == 0), every path returning 1.
bracketSequences :: [PathRecord] -> Either ShapeFault ([Bool] -> [PathEvent])
bracketSequences arms = do
  let paths = concatMap joined arms
      subjects = [(GuardDefaultRequested, 0), (GuardArgument (ArgumentIndex 2), 0)]
  mapM_ (\path -> if pathEnd path /= EndReturn (ReturnsConstant 1) then Left UnexpectedEnd else Right ()) paths
  unless (all (onlyGuards subjects) paths) (Left UnmodelledGuard)
  sequences <- covering subjects paths >>= eventSequences
  pure (\combination -> M.findWithDefault [] combination sequences)

-- | On bank 0, the open call, the events `inner` consumes and the close call, giving what follows the
-- close; on other banks what follows `inner`'s events.
bracketed :: Bool -> ([PathEvent] -> Maybe [PathEvent]) -> [PathEvent] -> Maybe [PathEvent]
bracketed bank0 inner events
  | bank0 = case events of
      open : rest | bracketCall bracketOpen open -> inner rest >>= \after -> case after of
        close : remaining | bracketCall bracketClose close -> Just remaining
        _ -> Nothing
      _ -> Nothing
  | otherwise = inner events

-- | Class 6: a field with a constant default whose bank-0 save and store run between the open and
-- close calls.
bracketedField :: Outcome -> Outcome -> Either (Side, ShapeFault) FieldArm
bracketedField getter setter = do
  (offset, width, bits, gets) <- tag Getter (getterField (outcomePaths getter))
  _ <- tag Getter (inContainer width bits)
  value <- case gets of
    GetsDefault (DefaultConstant c)
      | c <= maxDefault -> Right c
      | otherwise -> Left (Getter, DefaultOutOfRange)
    _ -> Left (Getter, DefaultFormUnrecognized)
  at <- tag Setter (bracketSequences (outcomePaths setter))
  let target bank0 = if bank0 then bank0Target offset width bits else fieldTarget offset width bits
      source notDefault = if notDefault then fromValue else (== FromConstant value)
      matches policy = and
        [ bracketed bank0 (writeEvents (target bank0) policy bank0 (source notDefault)) (at [notDefault, bank0]) == Just []
        | notDefault <- [False, True], bank0 <- [False, True]
        ]
  policy <- tag Setter (firstPolicy matches)
  pure (FieldArm offset width bits (FieldDefaultOnly (DefaultConstant value)) policy Bank0Bracket)

-- | Class 7: the getter's shared fallback, and a setter storing one value to two whole bytes, the
-- first inside the bank-0 bracket and the second after it.
writeOnlyPair :: Outcome -> Outcome -> Either (Side, ShapeFault) PairArm
writeOnlyPair getter setter = do
  mapM_ (bare Getter (EndReturn (ReturnsConstant 0))) (outcomePaths getter)
  at <- tag Setter (bracketSequences (outcomePaths setter))
  (first, second) <- case at [True, False] of
    [EventStore (Modelled (StoreRecord a _ _)), EventStore (Modelled (StoreRecord b _ _))]
      | Just (f, Byte) <- bankField (ArgumentIndex 2) a, Just (s, Byte) <- bankField (ArgumentIndex 2) b -> Right (f, s)
    _ -> Left (Setter, UnexpectedStore)
  value <- case at [False, False] of
    EventStore (Modelled (StoreRecord _ _ (FromConstant c))) : _
      | c <= maxDefault -> Right c
      | otherwise -> Left (Setter, DefaultOutOfRange)
    _ -> Left (Setter, DefaultFormUnrecognized)
  let whole = BitRange 0 8
      target bank0 offset = if bank0 then bank0Target offset Byte whole else fieldTarget offset Byte whole
      matches policy = and
        [ (bracketed bank0 (writeEvents (target bank0 first) policy bank0 source) (at [notDefault, bank0]) >>= writeEvents (target bank0 second) policy bank0 source) == Just []
        | notDefault <- [False, True], bank0 <- [False, True]
        , let source = if notDefault then fromValue else (== FromConstant value)
        ]
  policy <- tag Setter (firstPolicy matches)
  pure (PairArm first second value policy)

-- | A whole container read from the defaults table 0x81b074.
tailDefault :: Loc -> Extract -> Maybe FieldDefault
tailDefault loc (Extract lsb bits signed) = case offsetIn tailDefaultsBase [] loc of
  Just (t, w) | lsb == 0, not signed, bits == 8 * fromIntegral (widthBytes w), t <= 0xffff -> Just (DefaultTail (TailOffset (fromIntegral t)) w)
  _ -> Nothing

-- | The default a setter store's source supplies.
storedDefault :: Source -> Maybe FieldDefault
storedDefault source = case source of
  FromConstant c -> Just (DefaultConstant c)
  FromField loc extract -> tailDefault loc extract
  _ -> Nothing

-- | The value the getter returns with argument 2 == 1: a stage-2 default, a stock leaf's result, or
-- the whole configuration byte.
data GetterDefault = GetsDefault FieldDefault | GetsStock StockDefault | GetsConfig ConfigOffset deriving (Eq, Show)

-- | A whole configuration byte at 0x96b400 + offset.
configByte :: Loc -> Extract -> Maybe ConfigOffset
configByte loc extract = case offsetIn configBase [] loc of
  Just (o, Byte) | o < configBytes, extract == Extract 0 8 False -> Just (ConfigOffset (fromIntegral o))
  _ -> Nothing

-- | A path's events are none, or the one call whose result it returns.
getterEvents :: PathRecord -> Either ShapeFault ()
getterEvents path = case (pathEvents path, pathEnd path) of
  ([], _) -> Right ()
  ([EventCall called], EndReturn (ReturnsCall returned)) | called == returned -> Right ()
  (events, _)
    | any isStore events -> Left UnexpectedStore
    | otherwise -> Left UnexpectedCall
  where
    isStore event = case event of
      EventStore _ -> True
      EventCall _ -> False

getterDefault :: PathEnd -> Either ShapeFault GetterDefault
getterDefault end = case end of
  EndReturn (ReturnsConstant c) -> Right (GetsDefault (DefaultConstant c))
  EndReturn (ReturnsField loc extract)
    | Just fallback <- tailDefault loc extract -> Right (GetsDefault fallback)
    | Just offset <- configByte loc extract -> Right (GetsConfig offset)
  EndReturn (ReturnsCall called) | Just stock <- stockDefaultOf called -> Right (GetsStock stock)
  EndReturn (ReturnsCall _) -> Left UnexpectedCall
  _ -> Left DefaultFormUnrecognized

-- | The live field read with argument 2 /= 1 and the default read with argument 2 == 1.
getterField :: [PathRecord] -> Either ShapeFault (BankOffset, Width, BitRange, GetterDefault)
getterField paths = do
  mapM_ getterEvents paths
  if all (onlyGuards [(GuardArgument (ArgumentIndex 2), 1)]) paths then Right () else Left UnmodelledGuard
  grouped <- covering [(GuardArgument (ArgumentIndex 2), 1)] paths
  results <- mapM (\(combination, matching) -> (,) combination <$> single (map pathEnd matching)) grouped
  (live, defaulted) <- case results of
    [([False], l), ([True], d)] -> Right (l, d)
    _ -> Left ModeNotCovered
  (offset, width, bits) <- case live of
    EndReturn (ReturnsField loc extract) | Just (o, w) <- bankField (ArgumentIndex 1) loc -> (,,) o w <$> rangeOf extract
    _ -> Left UnexpectedEnd
  getDefault <- getterDefault defaulted
  pure (offset, width, bits, getDefault)

-- | A setter path continued through each path of its shared exit: the exit path's guards, the arm's
-- events then the exit's, and the exit's end with r0 as the arm left it where the exit keeps it.
-- A comparison of argument 1 with 0x1affffff is the default-request flag compared with 1.
joined :: PathRecord -> [PathRecord]
joined path = case (pathEnd path, pathExit path) of
  (EndExit result, Just exit) ->
    [ PathRecord (map linked (pathGuards continued)) (pathEvents path ++ pathEvents continued) (kept result (pathEnd continued)) Nothing
    | continued <- exitPaths exit
    ]
  _ -> [path {pathGuards = map linked (pathGuards path)}]
  where
    kept result end = case end of
      EndReturn ReturnsIncoming -> EndReturn result
      other -> other
    linked guard = case guard of
      Guard (GuardArgument (ArgumentIndex 1)) (Just 0x1affffff) cond holds -> Guard GuardDefaultRequested (Just 1) cond holds
      other -> other

-- | The one event sequence of each guard combination.
eventSequences :: [([Bool], [PathRecord])] -> Either ShapeFault (M.Map [Bool] [PathEvent])
eventSequences grouped = M.fromList <$> mapM (\(combination, matching) -> case nub (map pathEvents matching) of
  [one] -> Right (combination, one)
  _ -> Left UnexpectedStore) grouped

-- | The configuration byte every gate guard of the paths compares, when there is exactly one.
gateOf :: [PathRecord] -> Either ShapeFault (Maybe (GuardSubject, ConfigOffset))
gateOf paths = case nub [(subject, loc, extract) | path <- paths, Guard subject@(GuardField loc extract) _ _ _ <- pathGuards path] of
  [] -> Right Nothing
  [(subject, loc, extract)] | Just offset <- configByte loc extract -> Right (Just (subject, offset))
  _ -> Left UnmodelledGuard

-- | The setter of an adjusted field over (argument 1 is not the default request, argument 2 == 0,
-- and for a gated field the gate byte == 0), each path joined with its exit.
adjustedSetter :: BankOffset -> Width -> BitRange -> GetterDefault -> [PathRecord] -> Either ShapeFault FieldArm
adjustedSetter offset width bits gets paths = do
  mapM_ (\path -> if pathEnd path /= EndReturn (ReturnsConstant 1) then Left UnexpectedEnd else Right ()) paths
  gate <- gateOf paths
  let subjects = [(GuardDefaultRequested, 0), (GuardArgument (ArgumentIndex 2), 0)] ++ [(subject, 0) | Just (subject, _) <- [gate]]
  unless (all (onlyGuards subjects) paths) (Left UnmodelledGuard)
  sequences <- covering subjects paths >>= eventSequences
  let at combination = M.findWithDefault [] combination sequences
      target = fieldTarget offset width bits
      provisional = case at [False, False] of
        EventStore (Modelled (StoreRecord _ _ (FromConstant p))) : _ | p <= maxDefault -> Right p
        _ -> Left DefaultFormUnrecognized
      plain policy value bank0 events = writeEvents target policy bank0 value events == Just []
  case (gets, gate) of
    (GetsStock stock, Nothing) -> do
      p <- provisional
      let computed policy bank0 events = case writeEvents target policy bank0 (== FromConstant p) events of
            Just (EventCall called : rest) | stockDefaultOf called == Just stock -> writeEvents target policy bank0 (== FromCall called) rest == Just []
            _ -> False
      policy <- firstPolicy (\policy -> and [if notDefault then plain policy fromValue bank0 (at [notDefault, bank0]) else computed policy bank0 (at [notDefault, bank0]) | notDefault <- [False, True], bank0 <- [False, True]])
      pure (FieldArm offset width bits (FieldStock stock p) policy NoHook)
    (GetsConfig config, Nothing) -> do
      p <- provisional
      let fromConfig actual = case actual of
            FromField loc extract -> configByte loc (Extract 0 8 False) == Just config && supplies bits extract
            _ -> False
          configured policy bank0 events = (writeEvents target policy bank0 (== FromConstant p) events >>= writeEvents target NoSave bank0 fromConfig) == Just []
      policy <- firstPolicy (\policy -> and [if notDefault then plain policy fromValue bank0 (at [notDefault, bank0]) else configured policy bank0 (at [notDefault, bank0]) | notDefault <- [False, True], bank0 <- [False, True]])
      pure (FieldArm offset width bits (FieldConfig config p) policy NoHook)
    (GetsDefault fallback, Just (_, config)) -> do
      checkDefault fallback
      let fromDefault actual = storedDefault actual == Just fallback
          gated policy notDefault bank0 gateZero events =
            let value = if notDefault then fromValue else fromDefault
                once = writeEvents target policy bank0 value events
             in (if gateZero then once >>= writeEvents target policy bank0 value else once) == Just []
      policy <- firstPolicy (\policy -> and [gated policy notDefault bank0 gateZero (at [notDefault, bank0, gateZero]) | notDefault <- [False, True], bank0 <- [False, True], gateZero <- [False, True]])
      pure (FieldArm offset width bits (FieldDefaultOnly fallback) policy (ConfigGatedResave config))
    _ -> Left FieldDisagrees
  where
    checkDefault fallback = case fallback of
      DefaultConstant c | c > maxDefault -> Left DefaultOutOfRange
      DefaultTail (TailOffset t) w | w /= width || fromIntegral t > maxTailOffset -> Left DefaultOutOfRange
      _ -> Right ()

-- | The setter's store sequence for each value of (argument 1 is the default request, argument 2 == 0).
setterField :: BankOffset -> Width -> BitRange -> [PathRecord] -> Either ShapeFault (FieldDefault, SavePolicy)
setterField (BankOffset offsetWord) width bits paths = do
  mapM_ (\path -> if not (null (pathCalls path)) then Left UnexpectedCall else if pathEnd path /= EndExit (ReturnsConstant 1) then Left UnexpectedEnd else Right ()) paths
  if all (onlyGuards [(GuardDefaultRequested, 0), (GuardArgument (ArgumentIndex 2), 0)]) paths then Right () else Left UnmodelledGuard
  grouped <- covering [(GuardDefaultRequested, 0), (GuardArgument (ArgumentIndex 2), 0)] paths
  combinationsOf <- storeSequences grouped
  -- The default-request flag == 0 holds when no default was requested.
  let at notDefault bank0 = M.findWithDefault [] [notDefault, bank0] combinationsOf
  setDefault <- case reverse (at False True) of
    StoreRecord _ _ source : _ | Just fallback <- storedDefault source -> Right fallback
    _ -> Left DefaultFormUnrecognized
  let target = fieldTarget (BankOffset offsetWord) width bits
      fromDefault actual = storedDefault actual == Just setDefault
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

-- | The bank field at `offset` stored for the bank argument 2, saved to 0x89ec00 from bank 0.
fieldTarget :: BankOffset -> Width -> BitRange -> StoreTarget
fieldTarget (BankOffset offset) = StoreTarget (offsetIn bankBase (bankTerm (ArgumentIndex 2))) (offsetIn saveBase []) (offsetIn bankBase []) (fromIntegral offset)

-- | The bank field at `offset` of bank 0, stored at a bank-0 address or through the bank argument 2.
bank0Target :: BankOffset -> Width -> BitRange -> StoreTarget
bank0Target (BankOffset offset) =
  StoreTarget (\loc -> offsetIn bankBase [] loc <|> offsetIn bankBase (bankTerm (ArgumentIndex 2)) loc) (offsetIn saveBase []) (offsetIn bankBase []) (fromIntegral offset)

-- | A combination's stores under a save policy: on bank 0 with a save, the save and then the store,
-- otherwise the store alone, which inserts the source `value` accepts.
sequenceMatches :: StoreTarget -> SavePolicy -> (Source -> Bool) -> Bool -> [StoreRecord] -> Bool
sequenceMatches target policy value bank0 stores = writeEvents target policy bank0 value (map (EventStore . Modelled) stores) == Just []

-- | The events after one save and store at `target` that start `events`: on bank 0 with a save,
-- the save and then the store, otherwise the store alone, which inserts the source `value` accepts.
writeEvents :: StoreTarget -> SavePolicy -> Bool -> (Source -> Bool) -> [PathEvent] -> Maybe [PathEvent]
writeEvents target policy bank0 value events = case (policy == SaveBank0Previous && bank0, events) of
  (True, EventStore (Modelled save) : EventStore (Modelled store) : rest) | isSave save && isStore store -> Just rest
  (False, EventStore (Modelled store) : rest) | isStore store -> Just rest
  _ -> Nothing
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
  pure (SlotArm bank record save SlotDefaultsRecord NoMirror)

-- | The four ends of (argument 3 == 1, argument 2 == 1): record field, record default, bank field, bank default.
slotEnds :: [PathRecord] -> Either ShapeFault (PathEnd, PathEnd, PathEnd, PathEnd)
slotEnds paths = do
  mapM_ getterEvents paths
  let subjects = [(GuardArgument slotArgument, 1), (GuardArgument (ArgumentIndex 2), 1)]
  unless (all (onlyGuards subjects) paths) (Left UnmodelledGuard)
  grouped <- covering subjects paths
  ends <- mapM (\(combination, matching) -> (,) combination <$> single (map pathEnd matching)) grouped
  case ends of
    [([False, False], a), ([False, True], b), ([True, False], c), ([True, True], d)] -> Right (a, b, c, d)
    _ -> Left ModeNotCovered

bankLiveOf :: PathEnd -> Either ShapeFault (BankOffset, Width, BitRange)
bankLiveOf end = case end of
  EndReturn (ReturnsField loc extract) | Just (o, w) <- bankField (ArgumentIndex 1) loc -> (,,) o w <$> (rangeOf extract >>= inContainer w)
  _ -> Left UnexpectedEnd

-- | A record field's offset within the record, at `recordBase` + the offset of a location with `terms`.
recordFieldOf :: [(ArgumentIndex, Word32)] -> Loc -> Extract -> Either ShapeFault (Word32, Width, BitRange)
recordFieldOf terms loc extract = case offsetIn bankBase terms loc of
  Just (o, w) -> do
    r <- rangeOf extract >>= inContainer w
    let b = o - recordBase
    if o >= recordBase && b < recordStride && b + widthBytes w <= recordStride then Right (b, w, r) else Left RecordOutOfRange
  Nothing -> Left UnexpectedEnd

recordLiveOf :: PathEnd -> Either ShapeFault (Word32, Width, BitRange)
recordLiveOf end = case end of
  EndReturn (ReturnsField loc extract) -> recordFieldOf (recordTerms (ArgumentIndex 1)) loc extract
  _ -> Left UnexpectedEnd

recordBranchOf :: (Word32, Width, BitRange) -> RecordBranch
recordBranchOf (b, w, r) = RecordBranch (RecordOffset (fromIntegral b)) w r

slotGetter :: [PathRecord] -> Either ShapeFault (BankBranch, RecordBranch)
slotGetter paths = do
  (recordLive, recordDefault, bankLive, bankDefaulted) <- slotEnds paths
  (offset, width, bits) <- bankLiveOf bankLive
  fallback <- case bankDefaulted of
    EndReturn (ReturnsConstant c)
      | c <= maxDefault -> Right (BankConstant c)
      | otherwise -> Left DefaultOutOfRange
    EndReturn (ReturnsField loc extract)
      | Just (t, Byte) <- offsetIn tailDefaultsBase [] loc, extract == Extract 0 8 False ->
          if t <= maxDefault then Right (BankTailByte (TailOffset (fromIntegral t))) else Left DefaultOutOfRange
    _ -> Left DefaultFormUnrecognized
  record@(recordAt, recordWidth', recordBits') <- recordLiveOf recordLive
  case recordDefault of
    EndReturn (ReturnsField loc extract)
      | offsetIn slotDefaultsBase slotTerm loc == Just (recordAt, recordWidth'), extract == extractOfRange recordBits' -> Right ()
    _ -> Left RecordDefaultDisagrees
  pure (BankBranch offset width bits fallback, recordBranchOf record)

-- | The record branch: the slot's record of the bank argument 2, saved at the slot's record offset of 0x89ec00.
recordTarget :: RecordBranch -> StoreTarget
recordTarget (RecordBranch (RecordOffset b) w r) =
  StoreTarget (offsetIn bankBase (recordTerms (ArgumentIndex 2))) (offsetIn saveBase slotTerm) (offsetIn bankBase slotTerm) (recordBase + fromIntegral b) w r

-- | Record 0 of the bank argument 2, saved at record 0's offset of 0x89ec00.
record0Target :: RecordBranch -> StoreTarget
record0Target (RecordBranch (RecordOffset b) w r) =
  StoreTarget (offsetIn bankBase (bankTerm (ArgumentIndex 2))) (offsetIn saveBase []) (offsetIn bankBase []) (recordBase + fromIntegral b) w r

-- | The record's bits in the slot's (`terms` == slotTerm) or record 0's (`terms` == []) slot-defaults record.
recordDefaultFrom :: [(ArgumentIndex, Word32)] -> RecordBranch -> Source -> Bool
recordDefaultFrom terms (RecordBranch (RecordOffset b) w r) actual = case actual of
  FromField loc extract -> offsetIn slotDefaultsBase terms loc == Just (fromIntegral b, w) && supplies r extract
  _ -> False

-- | Classes 3 and 8: a slot field whose shared exit computes the default (both branches' default is
-- a stock leaf's result) or whose every read and write also goes to record 0.
adjustedSlot :: Outcome -> Outcome -> Either (Side, ShapeFault) SlotArm
adjustedSlot getter setter = do
  ends@(_, recordDefault, _, bankDefaulted) <- tag Getter (slotEnds (outcomePaths getter))
  let paths = concatMap joined (outcomePaths setter)
  tag Setter (mapM_ (\path -> if pathEnd path /= EndReturn (ReturnsConstant 1) then Left UnexpectedEnd else Right ()) paths)
  let subjects = [(GuardArgument slotArgument, 1), (GuardDefaultRequested, 0), (GuardArgument (ArgumentIndex 2), 0)]
  tag Setter (unless (all (onlyGuards subjects) paths) (Left UnmodelledGuard))
  sequences <- tag Setter (covering subjects paths >>= eventSequences)
  let at combination = M.findWithDefault [] combination sequences
  case (recordDefault, bankDefaulted) of
    (EndReturn (ReturnsCall recordCall), EndReturn (ReturnsCall bankCall)) -> do
      stock <- case (stockDefaultOf recordCall, stockDefaultOf bankCall) of
        (Just a, Just b) | a == b -> Right a
        (Just _, Just _) -> Left (Getter, DefaultsDiffer)
        _ -> Left (Getter, UnexpectedCall)
      stockSlot ends stock at
    _ -> mirroredSlot ends at

firstPolicy :: (SavePolicy -> Bool) -> Either ShapeFault SavePolicy
firstPolicy accepted = case filter accepted [SaveBank0Previous, NoSave] of
  policy : _ -> Right policy
  [] -> Left FieldDisagrees

fromValue :: Source -> Bool
fromValue actual = actual == FromArgument (ArgumentIndex 1)

-- | Class 3. Combinations are (argument 3 == 1, argument 1 is not the default request, argument 2 == 0).
stockSlot :: (PathEnd, PathEnd, PathEnd, PathEnd) -> StockDefault -> ([Bool] -> [PathEvent]) -> Either (Side, ShapeFault) SlotArm
stockSlot (recordLive, _, bankLive, _) stock at = do
  (offset, width, bits) <- tag Getter (bankLiveOf bankLive)
  record <- recordBranchOf <$> tag Getter (recordLiveOf recordLive)
  provisional <- case at [True, False, False] of
    EventStore (Modelled (StoreRecord _ _ (FromConstant p))) : _ | p <= maxDefault -> Right p
    _ -> Left (Setter, DefaultFormUnrecognized)
  let branch slot1 = if slot1 then (fieldTarget offset width bits, (== FromConstant provisional)) else (recordTarget record, recordDefaultFrom slotTerm record)
      computed target policy bank0 provisionalSource events = case writeEvents target policy bank0 provisionalSource events of
        Just (EventCall called : rest) | stockDefaultOf called == Just stock -> writeEvents target policy bank0 (== FromCall called) rest == Just []
        _ -> False
      matches policy = and
        [ if notDefault then writeEvents target policy bank0 fromValue events == Just [] else computed target policy bank0 provisionalSource events
        | slot1 <- [False, True], let (target, provisionalSource) = branch slot1, notDefault <- [False, True], bank0 <- [False, True], let events = at [slot1, notDefault, bank0]
        ]
  policy <- tag Setter (firstPolicy matches)
  pure (SlotArm (BankBranch offset width bits (BankConstant provisional)) record policy (SlotStock stock) NoMirror)

-- | Class 8: every getter end reads record 0, and every set ends with record 0's save and store.
mirroredSlot :: (PathEnd, PathEnd, PathEnd, PathEnd) -> ([Bool] -> [PathEvent]) -> Either (Side, ShapeFault) SlotArm
mirroredSlot (recordLive, recordDefault, bankLive, bankDefaulted) at = do
  unless (recordLive == bankLive && recordDefault == bankDefaulted) (Left (Getter, ModeNotCovered))
  record@(RecordBranch (RecordOffset recordAt) recordWidth' recordBits') <- case recordLive of
    EndReturn (ReturnsField loc extract) -> recordBranchOf <$> tag Getter (recordFieldOf (bankTerm (ArgumentIndex 1)) loc extract)
    _ -> Left (Getter, UnexpectedEnd)
  case recordDefault of
    EndReturn (ReturnsField loc extract)
      | offsetIn slotDefaultsBase [] loc == Just (fromIntegral recordAt, recordWidth'), extract == extractOfRange recordBits' -> Right ()
    _ -> Left (Getter, RecordDefaultDisagrees)
  (offset, width, bits) <- case at [True, True, False] of
    EventStore (Modelled (StoreRecord loc extract _)) : _ | Just (o, w) <- bankField (ArgumentIndex 2) loc -> (,,) o w <$> tag Setter (rangeOf extract >>= inContainer w)
    _ -> Left (Setter, UnexpectedStore)
  fallback <- case at [True, False, False] of
    EventStore (Modelled (StoreRecord _ _ (FromConstant c))) : _ | c <= maxDefault -> Right (BankConstant c)
    EventStore (Modelled (StoreRecord _ _ (FromField loc extract))) : _
      | Just (t, Byte) <- offsetIn tailDefaultsBase [] loc, extract == Extract 0 8 False, t <= maxDefault -> Right (BankTailByte (TailOffset (fromIntegral t)))
    _ -> Left (Setter, DefaultFormUnrecognized)
  let bankDefaultSource actual = case (fallback, actual) of
        (BankConstant c, FromConstant stored) -> stored == c
        (BankTailByte (TailOffset t), FromField loc extract) -> offsetIn tailDefaultsBase [] loc == Just (fromIntegral t, Byte) && extract == Extract 0 8 False
        _ -> False
      branch slot1 = if slot1 then (fieldTarget offset width bits, bankDefaultSource) else (recordTarget record, recordDefaultFrom slotTerm record)
      matches policy = and
        [ (writeEvents target policy bank0 (if notDefault then fromValue else defaulted) events
            >>= writeEvents (record0Target record) policy bank0 (if notDefault then fromValue else recordDefaultFrom [] record)) == Just []
        | slot1 <- [False, True], let (target, defaulted) = branch slot1, notDefault <- [False, True], bank0 <- [False, True], let events = at [slot1, notDefault, bank0]
        ]
  policy <- tag Setter (firstPolicy matches)
  pure (SlotArm (BankBranch offset width bits fallback) record policy SlotDefaultsRecord MirrorRecord0)

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
      bankDefaultSource actual = case (fallback, actual) of
        (BankConstant c, FromConstant stored) -> stored == c
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
