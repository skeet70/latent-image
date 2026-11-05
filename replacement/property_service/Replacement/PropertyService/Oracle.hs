{-# LANGUAGE GHC2021 #-}

-- | Native cases of the X-M1 1.50 property getter 0x61e110 and setter 0x624694: the case grid, the
-- shared fixture, the owned-semantics model, the per-case comparison and the guest-control patches.
module Replacement.PropertyService.Oracle
  ( Bank (..)
  , ReadMode (..)
  , SlotArgument (..)
  , slotArguments
  , SetChoice (..)
  , Operation (..)
  , OracleCase (..)
  , CaseLabel (..)
  , Standing (..)
  , propertyIds
  , operations
  , caseGrid
  , caseLabel
  , standingOf
  , modelled
  , setValue
  , defaultRequest
  , Fixture (..)
  , fixture
  , FixtureVariant (..)
  , variantFixture
  , variantTag
  , variantNote
  , Effect (..)
  , Layout (..)
  , RecordSave (..)
  , stockLayout
  , modelEffect
  , modelEffectIn
  , Expectation (..)
  , modelExpectation
  , placeholderExpectation
  , observedExpectation
  , WriteEvent (..)
  , Run (..)
  , Difference (..)
  , differenceKind
  , differenceDetail
  , expectationDifferences
  , modelDifferences
  , compareRuns
  , reportWrites
  , runOf
  , forceRun
  , comparedRegisters
  , comparedRegions
  , FunctionSpan (..)
  , stockFunctions
  , splitFunctions
  , caseSpec
  , Patch (..)
  , applyPatches
  , EntryControl (..)
  , mutateArm
  , entryControlOperations
  , exposingField
  , SlotControl (..)
  , SlotMutation (..)
  , slotControlOperations
  , exposingSlot
  , branchWord
  , movwWord
  , movtWord
  , image4Base
  ) where

import Control.Monad (foldM, unless)
import Data.Aeson (Value)
import Data.Bits (complement, shiftL, shiftR, xor, (.&.), (.|.))
import qualified Data.ByteString as B
import Data.List (sortOn)
import qualified Data.Map.Strict as M
import Data.Maybe (fromMaybe, isNothing, listToMaybe)
import Data.Word (Word16, Word32, Word64, Word8)
import qualified Emulation.NativeObservation as N
import Emulation.NativeObservation (Register (..), RegionId (..), RunnerStatus (..))
import Emulation.OwnedCase (CaseRegion, CaseSpec (..), Permission (..), mkRegion, readWordAt, word32le)
import FujiAF.Host (get, list, num, str)
import Numeric (readHex, showHex)
import PropertyFields (PropertyId (..))
import Replacement.PropertyService.Classify

data Bank = Bank0 | Bank1 deriving (Eq, Ord, Show, Enum, Bounded)

-- | The getter's argument 2: 0 reads the live field, 1 the default.
data ReadMode = LiveRead | DefaultRead deriving (Eq, Ord, Show, Enum, Bounded)

-- | Argument 3 of both functions; a slot field takes its bank branch for 1 and its record branch otherwise.
newtype SlotArgument = SlotArgument Word32 deriving (Eq, Ord, Show)

-- | 0xffffffff addresses the record one stride below slot 0.
slotArguments :: [SlotArgument]
slotArguments = map SlotArgument [0, 1, 2, 3, 0xffffffff]

-- | The setter's argument 1, resolved against the field by 'setValue'.
data SetChoice = SetZero | SetOne | SetFieldMax | SetAboveMax | SetAllOnes | SetDefaultRequest
  deriving (Eq, Ord, Show, Enum, Bounded)

data Operation = Get Bank ReadMode SlotArgument | Set Bank SlotArgument SetChoice deriving (Eq, Ord, Show)

data OracleCase = OracleCase { oracleProperty :: PropertyId, oracleOperation :: Operation } deriving (Eq, Ord, Show)

-- | A case's identifier within a batch and its file stem.
newtype CaseLabel = CaseLabel { caseLabelText :: String } deriving (Eq, Ord, Show)

-- | What the oracle knows of a property id: a classified class, or outside the 645-entry tables.
data Standing = Classified Class | OutOfRange deriving (Eq, Show)

propertyIds :: [PropertyId]
propertyIds = [PropertyId n | n <- [0 .. 646]]

operations :: [Operation]
operations =
  [Get bank mode slot | bank <- [minBound .. maxBound], mode <- [minBound .. maxBound], slot <- slotArguments]
    ++ [Set bank slot choice | bank <- [minBound .. maxBound], slot <- slotArguments, choice <- [minBound .. maxBound]]

caseGrid :: [OracleCase]
caseGrid = [OracleCase identifier operation | identifier <- propertyIds, operation <- operations]

caseLabel :: OracleCase -> CaseLabel
caseLabel (OracleCase (PropertyId n) operation) = CaseLabel ("p" ++ pad 3 (showHex n "") ++ "-" ++ operationLabel)
  where
    operationLabel = case operation of
      Get bank mode slot -> "get-" ++ bankLabel bank ++ "-" ++ modeLabel mode ++ "-" ++ slotLabel slot
      Set bank slot choice -> "set-" ++ bankLabel bank ++ "-" ++ slotLabel slot ++ "-" ++ choiceLabel choice
    bankLabel bank = "b" ++ show (fromEnum bank)
    slotLabel (SlotArgument slot) = "s" ++ showHex slot ""
    modeLabel mode = case mode of
      LiveRead -> "live"
      DefaultRead -> "default"
    choiceLabel choice = case choice of
      SetZero -> "zero"
      SetOne -> "one"
      SetFieldMax -> "max"
      SetAboveMax -> "above-max"
      SetAllOnes -> "all-ones"
      SetDefaultRequest -> "default-request"

pad :: Int -> String -> String
pad width text = replicate (width - length text) '0' ++ text

standingOf :: M.Map PropertyId Property -> PropertyId -> Standing
standingOf properties identifier = maybe OutOfRange (Classified . propertyClass) (M.lookup identifier properties)

-- | The owned behaviour the stock code must show: ids outside the tables take both shared fallbacks.
modelled :: Standing -> Maybe Owned
modelled standing = case standing of
  Classified (Owned owned) -> Just owned
  Classified (Delegated _) -> Nothing
  OutOfRange -> Just OwnedUnassigned

defaultRequest :: Word32
defaultRequest = 0x1affffff

rangeMask :: BitRange -> Word32
rangeMask (BitRange _ bits) = (1 `shiftL` bits) - 1

bankBranchSelected :: SlotArgument -> Bool
bankBranchSelected (SlotArgument slot) = slot == 1

-- | Argument 1 of a set: the field's maximum and maximum + 1 use the bits the slot selects.
setValue :: Standing -> SlotArgument -> SetChoice -> Word32
setValue standing slot choice = case choice of
  SetZero -> 0
  SetOne -> 1
  SetFieldMax -> maximal
  SetAboveMax -> maximal + 1
  SetAllOnes -> 0xffffffff
  SetDefaultRequest -> defaultRequest
  where
    maximal = case standing of
      Classified (Owned (OwnedField arm)) -> rangeMask (fieldBits arm)
      Classified (Owned (OwnedSlotField arm))
        | bankBranchSelected slot -> rangeMask (bankBits (bankArm arm))
        | otherwise -> rangeMask (recordBits (recordArm arm))
      _ -> 0x7f

-- | Initial bytes of the rw images every case shares, and the image4 windows of the slot-defaults
-- records and the defaults table.
data Fixture = Fixture
  { fixtureBanks :: B.ByteString
  , fixturePrevious :: B.ByteString
  , fixtureConfig :: B.ByteString
  , fixtureSlotDefaults :: B.ByteString
  , fixtureTailDefaults :: B.ByteString
  }
  deriving (Eq, Show)

bankStrideBytes :: Int
bankStrideBytes = 0x1c2

-- | Three bank images, since slot 3 of bank 1 reaches into the third.
bankCount :: Int
bankCount = 3

previousBytes, slotDefaultsBytes, tailDefaultsBytes :: Int
previousBytes = 0x1e0
slotDefaultsBytes = 0xc8
tailDefaultsBytes = 0x1c2

-- | The slot-defaults window starts one record below 0x7cb804, for slot 0xffffffff.
slotDefaultsWindow :: Word32
slotDefaultsWindow = slotDefaultsBase - recordStride

-- | Bank 0 is an xorshift stream, bank 1 its complement and bank 2 another stream; the
-- previous-value and configuration images are streams of other seeds. The defaults windows are image4's.
fixture :: B.ByteString -> Fixture
fixture image4 =
  Fixture
    (bank0 <> B.map complement bank0 <> stream bankStrideBytes 0x85ebca6b)
    (stream previousBytes 0x2545f491)
    (stream 0x200 0x6c8e9cf5)
    (slice image4 slotDefaultsWindow slotDefaultsBytes)
    (slice image4 tailDefaultsBase tailDefaultsBytes)
  where
    bank0 = stream bankStrideBytes 0x9e3779b9

-- | The shared fixture, or one with a changed window: `TailWindow` sets the defaults-table byte at
-- 0x81b075, the high byte of 0x1e6's halfword default, to 0x5a.
data FixtureVariant = CanonicalFixture | TailWindow deriving (Eq, Ord, Show, Enum, Bounded)

variantFixture :: Fixture -> FixtureVariant -> Fixture
variantFixture fx variant = case variant of
  CanonicalFixture -> fx
  TailWindow -> fx {fixtureTailDefaults = replaceByte 1 0x5a (fixtureTailDefaults fx)}
  where
    replaceByte at value bytes = B.take at bytes <> B.singleton value <> B.drop (at + 1) bytes

variantTag :: FixtureVariant -> String
variantTag variant = case variant of
  CanonicalFixture -> "canonical"
  TailWindow -> "tail-window"

variantNote :: FixtureVariant -> String
variantNote variant = case variant of
  CanonicalFixture -> "the shared fixture with image4's defaults windows"
  TailWindow -> "defaults-table byte 0x81b075 set to 0x5a, so 0x1e6's halfword default differs from its low byte"

stream :: Int -> Word32 -> B.ByteString
stream count seed = B.pack (take count (map (\x -> fromIntegral (x `shiftR` 24) :: Word8) (drop 1 (iterate step seed))))
  where
    step x0 =
      let x1 = x0 `xor` (x0 `shiftL` 13)
          x2 = x1 `xor` (x1 `shiftR` 17)
       in x2 `xor` (x2 `shiftL` 5)

widthBytes :: Width -> Int
widthBytes width = case width of
  Byte -> 1
  Half -> 2

-- | The little-endian container at an absolute address of a window starting at `start`, when it lies inside.
load :: Word32 -> B.ByteString -> Word32 -> Width -> Maybe Word32
load start bytes address width
  | at + toInteger (widthBytes width) <= toInteger (B.length bytes) = Just $ case width of
      Byte -> fromIntegral (B.index bytes index)
      Half -> fromIntegral (B.index bytes index) .|. (fromIntegral (B.index bytes (index + 1)) `shiftL` 8)
  | otherwise = Nothing
  where
    at = toInteger (address - start)
    index = fromInteger at

store :: Word32 -> B.ByteString -> Word32 -> Width -> Word32 -> Maybe B.ByteString
store start bytes address width value = do
  _ <- load start bytes address width
  let at = fromIntegral (address - start)
  pure (B.take at bytes <> B.pack (take (widthBytes width) [fromIntegral value, fromIntegral (value `shiftR` 8)]) <> B.drop (at + widthBytes width) bytes)

extractRange :: BitRange -> Word32 -> Word32
extractRange bits@(BitRange lsb _) value = (value `shiftR` lsb) .&. rangeMask bits

-- | `container` with the range replaced by the low bits of `value`, as BFI does.
insertRange :: BitRange -> Word32 -> Word32 -> Word32
insertRange bits@(BitRange lsb _) container value =
  let mask = rangeMask bits `shiftL` lsb
   in (container .&. complement mask) .|. ((value `shiftL` lsb) .&. mask)

data Effect = Effect
  { effectR0 :: Word32
  , effectBanks :: B.ByteString
  , effectPrevious :: B.ByteString
  }
  deriving (Eq, Show)

-- | Where a bank-0 save of a slot record goes: its own record offset in 0x89ec00, or the bank
-- branch's offset there (a control).
data RecordSave = RecordSaveAtRecord | RecordSaveAtBankOffset deriving (Eq, Show)

-- | The defaults bases and record save the owned code uses; controls change one of them.
data Layout = Layout
  { layoutSlotDefaults :: Word32
  , layoutTailDefaults :: Word32
  , layoutRecordSave :: RecordSave
  }
  deriving (Eq, Show)

stockLayout :: Layout
stockLayout = Layout slotDefaultsBase tailDefaultsBase RecordSaveAtRecord

modelEffect :: Fixture -> Owned -> Operation -> Maybe Effect
modelEffect = modelEffectIn stockLayout

-- | The owned semantics: the return value and the final bank and previous-value images, or Nothing
-- when an access falls outside the fixture.
modelEffectIn :: Layout -> Fixture -> Owned -> Operation -> Maybe Effect
modelEffectIn layout (Fixture banks previous _ slotDefaults tailDefaults) owned operation = case (owned, operation) of
  (OwnedUnassigned, _) -> Just (unchanged 0)
  (OwnedField (FieldArm offset width bits fallback _), Get bank mode _) -> unchanged <$> case mode of
    DefaultRead -> fieldDefault fallback
    LiveRead -> extractRange bits <$> load bankBase banks (bankAddress bank offset) width
  (OwnedField (FieldArm offset width bits fallback save), Set bank slot choice) ->
    let stored = resolved slot choice (fieldDefault fallback)
     in write width bits save bank (bankAddress bank offset) (saveAddress offset) (saveAddress offset) =<< stored
  (OwnedSlotField (SlotArm (BankBranch offset width bits fallback) (RecordBranch record recordWidth' recordBits') _), Get bank mode slot)
    | bankBranchSelected slot -> unchanged <$> case mode of
        DefaultRead -> bankDefault fallback
        LiveRead -> extractRange bits <$> load bankBase banks (bankAddress bank offset) width
    | otherwise -> unchanged . extractRange recordBits' <$> case mode of
        DefaultRead -> load slotDefaultsWindow slotDefaults (slotDefaultsAddress slot record) recordWidth'
        LiveRead -> load bankBase banks (recordAddress bank slot record) recordWidth'
  (OwnedSlotField (SlotArm (BankBranch offset width bits fallback) (RecordBranch record recordWidth' recordBits') save), Set bank slot choice)
    | bankBranchSelected slot ->
        write width bits save bank (bankAddress bank offset) (saveAddress offset) (saveAddress offset) =<< resolved slot choice (bankDefault fallback)
    | otherwise ->
        let recordSaved = saveBase + recordBase + slotStride slot + recordOffsetWord record
            savedTo = case layoutRecordSave layout of
              RecordSaveAtRecord -> recordSaved
              RecordSaveAtBankOffset -> saveAddress offset
            recordDefault = extractRange recordBits' <$> load slotDefaultsWindow slotDefaults (slotDefaultsAddress slot record) recordWidth'
         in write recordWidth' recordBits' save bank (recordAddress bank slot record) recordSaved savedTo =<< resolved slot choice recordDefault
  where
    unchanged r0 = Effect r0 banks previous
    resolved slot choice fallback =
      let value = setValue (Classified (Owned owned)) slot choice
       in if value == defaultRequest then fallback else Just value
    bankAddress bank (BankOffset offset) = bankBase + fromIntegral (bankStrideBytes * fromEnum bank) + fromIntegral offset
    saveAddress (BankOffset offset) = saveBase + fromIntegral offset
    slotStride (SlotArgument slot) = slot * recordStride
    recordOffsetWord (RecordOffset record) = fromIntegral record
    recordAddress bank slot record = bankAddress bank (BankOffset 0) + recordBase + slotStride slot + recordOffsetWord record
    slotDefaultsAddress slot record = layoutSlotDefaults layout + slotStride slot + recordOffsetWord record
    bankDefault fallback = case fallback of
      BankConstant value -> Just value
      BankTailByte (TailOffset tail') -> load tailDefaultsBase tailDefaults (layoutTailDefaults layout + fromIntegral tail') Byte
    fieldDefault fallback = case fallback of
      DefaultConstant value -> Just value
      DefaultTail (TailOffset tail') width -> load tailDefaultsBase tailDefaults (layoutTailDefaults layout + fromIntegral tail') width
    -- On bank 0 with a save, the old bits go from the live container into the previous-value
    -- container read at `savedFrom` and written at `savedTo`; then the value is inserted.
    write width bits save bank live savedFrom savedTo stored = do
      saved <-
        if bank == Bank0 && save == SaveBank0Previous
          then do
            old <- load bankBase banks live width
            container <- load saveBase previous savedFrom width
            store saveBase previous savedTo width (insertRange bits container (extractRange bits old))
          else Just previous
      container <- load bankBase banks live width
      updated <- store bankBase banks live width (insertRange bits container stored)
      pure (Effect 1 updated saved)

-- | What a case's JSON claims; the runner reports a mismatch but still returns the observation.
data Expectation = Expectation
  { expectedStatus :: RunnerStatus
  , expectedRegisters :: [(Register, Word32)]
  , expectedMemory :: [(RegionId, B.ByteString)]
  }
  deriving (Eq, Show)

calleeSaved :: [Register]
calleeSaved = [R4, R5, R6, R7, R8, R9, R10, R11]

sentinel :: Register -> Word32
sentinel register = 0xa5a50000 + fromIntegral (fromEnum register)

-- | r0, the callee-saved registers and sp at exit.
comparedRegisters :: [Register]
comparedRegisters = R0 : calleeSaved ++ [Sp]

banksId, previousId, configId, codeId, slotDefaultsId, tailDefaultsId, stackId, callerFrameId, exitId :: RegionId
banksId = RegionId "banks"
previousId = RegionId "previous"
configId = RegionId "config"
codeId = RegionId "code"
slotDefaultsId = RegionId "slot-defaults"
tailDefaultsId = RegionId "tail-defaults"
stackId = RegionId "stack"
callerFrameId = RegionId "caller-frame"
exitId = RegionId "exit"

-- | The rw regions whose final bytes and writes the verdict compares; the stack below entry sp is scratch.
comparedRegions :: [(RegionId, Word32, Int)]
comparedRegions =
  [ (banksId, bankBase, bankCount * bankStrideBytes)
  , (previousId, saveBase, previousBytes)
  , (configId, configBase, 0x200)
  , (callerFrameId, stackPointer, fromIntegral (stackTop - stackPointer))
  ]

configBase, stackBase, stackTop, exitAddress, image4Base :: Word32
configBase = 0x96b400
stackBase = 0x959000
stackTop = 0x959800
exitAddress = 0x958f00
image4Base = 0x61c400

stackPointer :: Word32
stackPointer = stackTop - 0x10

-- | The model's claim for a case, or Nothing when the model reaches outside the fixture.
modelExpectation :: Fixture -> Owned -> Operation -> Maybe Expectation
modelExpectation fx owned operation = expected <$> modelEffect fx owned operation
  where
    expected effect =
      Expectation Returned ((R0, effectR0 effect) : preserved)
        [(banksId, effectBanks effect), (previousId, effectPrevious effect), (configId, fixtureConfig fx), (callerFrameId, B.replicate (fromIntegral (stackTop - stackPointer)) 0)]
    preserved = [(register, sentinel register) | register <- calleeSaved] ++ [(Sp, stackPointer)]

-- | A delegated case before any observation: the runner needs a claim, which is not judged.
placeholderExpectation :: Expectation
placeholderExpectation = Expectation Returned [(R0, 0)] []

observedExpectation :: Run -> Expectation
observedExpectation run = Expectation (runStatus run) (runRegisters run) (runMemory run)

data WriteEvent = WriteEvent
  { eventAddress :: !Word32
  , eventWidth :: !Int
  , eventValue :: !Word64
  }
  deriving (Eq, Show)

-- | The compared part of one observation. 'runWrites' is Nothing when the provenance log was truncated.
data Run = Run
  { runStatus :: !RunnerStatus
  , runRegisters :: ![(Register, Word32)]
  , runMemory :: ![(RegionId, B.ByteString)]
  , runWrites :: !(Maybe [WriteEvent])
  }
  deriving (Eq, Show)

forceRun :: Run -> Run
forceRun run =
  sum (map snd (runRegisters run)) `seq` sum (map (B.length . snd) (runMemory run)) `seq` maybe 0 length (runWrites run) `seq` run

data Difference
  = StatusDiffers RunnerStatus RunnerStatus
  | RegisterDiffers Register Word32 Word32
  | MemoryDiffers RegionId
  | WritesDiffer
  | ReferenceTruncated
  | SubjectTruncated
  | -- | The model reads or writes outside the fixture for this case.
    ModelOutsideFixture
  deriving (Eq, Show)

differenceKind :: Difference -> String
differenceKind difference = case difference of
  StatusDiffers _ _ -> "status"
  RegisterDiffers _ _ _ -> "register"
  MemoryDiffers _ -> "memory"
  WritesDiffer -> "writes"
  ReferenceTruncated -> "truncated"
  SubjectTruncated -> "truncated"
  ModelOutsideFixture -> "model"

differenceDetail :: Difference -> String
differenceDetail difference = case difference of
  StatusDiffers expected actual -> N.runnerStatusLabel actual ++ ", expected " ++ N.runnerStatusLabel expected
  RegisterDiffers register expected actual -> N.registerLabel register ++ " " ++ N.hexWord32 actual ++ ", expected " ++ N.hexWord32 expected
  MemoryDiffers ident -> "final bytes of " ++ regionName ident
  WritesDiffer -> "ordered write sequence"
  ReferenceTruncated -> "reference provenance truncated"
  SubjectTruncated -> "subject provenance truncated"
  ModelOutsideFixture -> "model access outside the fixture"

expectationDifferences :: Expectation -> Run -> [Difference]
expectationDifferences expected run =
  [StatusDiffers (expectedStatus expected) (runStatus run) | expectedStatus expected /= runStatus run]
    ++ [ RegisterDiffers register value actual
       | (register, value) <- expectedRegisters expected
       , let actual = fromMaybe (complement value) (lookup register (runRegisters run))
       , actual /= value
       ]
    ++ [MemoryDiffers ident | (ident, bytes) <- expectedMemory expected, lookup ident (runMemory run) /= Just bytes]

-- | A stock run of an owned id against the model's claim.
modelDifferences :: Fixture -> Owned -> Operation -> Run -> [Difference]
modelDifferences fx owned operation run = maybe [ModelOutsideFixture] (\expected -> expectationDifferences expected run) (modelExpectation fx owned operation)

-- | Every compared field of the subject against the reference run of the same case.
compareRuns :: Run -> Run -> [Difference]
compareRuns reference subject =
  expectationDifferences (observedExpectation reference) subject
    ++ [ReferenceTruncated | isNothing (runWrites reference)]
    ++ [SubjectTruncated | isNothing (runWrites subject)]
    ++ [WritesDiffer | Just expected <- [runWrites reference], Just actual <- [runWrites subject], expected /= actual]

-- | The write events of a case report's @runner.memory_provenance@, in order.
reportWrites :: Value -> Either String [WriteEvent]
reportWrites report = do
  entries <- mapM entry (list (get (get report "runner") "memory_provenance"))
  pure [event | Just event <- entries]
  where
    entry value = case str (get value "kind") of
      "read" -> Right Nothing
      "write" -> do
        address <- hexField value "address"
        width <- case num (get value "size") of
          size | size >= 1 && size <= 8 -> Right (fromInteger size)
          _ -> Left "provenance size"
        written <- hexField value "value"
        Right (Just (WriteEvent (fromIntegral address) width written))
      other -> Left ("provenance kind " ++ other)
    hexField :: Value -> String -> Either String Word64
    hexField value key = case str (get value key) of
      '0' : 'x' : digits | not (null digits), length digits <= 16, [(n, "")] <- readHex digits -> Right n
      text -> Left ("provenance " ++ key ++ " " ++ text)

-- | The compared part of an observation and its report's write events.
runOf :: N.NativeObservation -> [WriteEvent] -> Either String Run
runOf observation writes = do
  registers <- mapM (\register -> maybe (Left ("register " ++ N.registerLabel register)) (Right . (,) register) (N.lookupRegister register observation)) comparedRegisters
  memory <- mapM (\(ident, _, _) -> maybe (Left ("region " ++ regionName ident)) (Right . (,) ident . B.copy) (lookup ident (N.nativeMemory observation))) comparedRegions
  retained <- case N.nativeProvenance observation of
    N.Truncated _ -> Right Nothing
    N.Complete _ logged -> do
      unless (map (\(N.Write (N.GuestAddress a) (N.WriteBytes w)) -> (fromIntegral a, fromIntegral w)) logged == [(eventAddress e, eventWidth e) | e <- writes])
        (Left "report writes differ from the parsed provenance")
      Right (Just [event | event <- writes, compared (eventAddress event)])
  pure (Run (N.nativeStatus observation) registers memory retained)
  where
    compared address = or [base <= address && address < base + fromIntegral size | (_, base, size) <- comparedRegions]

-- | A declared function: the half-open span [spanStart, spanEnd).
data FunctionSpan = FunctionSpan { spanStart :: Word32, spanEnd :: Word32 } deriving (Eq, Ord, Show)

getterSpan, setterSpan :: FunctionSpan
getterSpan = FunctionSpan 0x61e110 0x624620
setterSpan = FunctionSpan 0x624694 0x6309c4

stockFunctions :: [FunctionSpan]
stockFunctions = [getterSpan, setterSpan]

-- | The stock spans with the setter cut around [start, end), plus the functions declared inside it.
splitFunctions :: (Word32, Word32) -> [FunctionSpan] -> [FunctionSpan]
splitFunctions (start, end) inner =
  sortOn spanStart ([getterSpan, FunctionSpan (spanStart setterSpan) start, FunctionSpan end (spanEnd setterSpan)] ++ inner)

slice :: B.ByteString -> Word32 -> Int -> B.ByteString
slice image address size = B.take size (B.drop (fromIntegral (address - image4Base)) image)

region :: RegionId -> Word32 -> Int -> Permission -> B.ByteString -> CaseRegion
region ident address = mkRegion ident (N.GuestAddress (fromIntegral address))

-- | One native case of a role's image4 bytes and declared functions.
caseSpec :: B.ByteString -> [FunctionSpan] -> Fixture -> CaseLabel -> Standing -> OracleCase -> Expectation -> CaseSpec
caseSpec image functions fx (CaseLabel label) standing (OracleCase (PropertyId identifier) operation) expected =
  CaseSpec
    { caseId = label
    , caseProvenance = "X-M1 1.50 image4 " ++ entryName ++ " for property " ++ hex identifier ++ " over the shared property fixture"
    , caseExclusion = "one call of the property " ++ entryName ++ "; arms reaching code or data outside the mapped regions refuse"
    , caseEntry = N.GuestAddress (fromIntegral entry)
    , caseExit = N.GuestAddress (fromIntegral exitAddress)
    , caseRegions =
        [ region codeId (spanStart getterSpan) (fromIntegral (spanEnd setterSpan - spanStart getterSpan)) ReadExecute (slice image (spanStart getterSpan) (fromIntegral (spanEnd setterSpan - spanStart getterSpan)))
        , region slotDefaultsId slotDefaultsWindow (B.length (fixtureSlotDefaults fx)) ReadOnly (fixtureSlotDefaults fx)
        , region tailDefaultsId tailDefaultsBase (B.length (fixtureTailDefaults fx)) ReadOnly (fixtureTailDefaults fx)
        , region banksId bankBase (B.length (fixtureBanks fx)) ReadWrite (fixtureBanks fx)
        , region previousId saveBase (B.length (fixturePrevious fx)) ReadWrite (fixturePrevious fx)
        , region configId configBase (B.length (fixtureConfig fx)) ReadWrite (fixtureConfig fx)
        , region stackId stackBase (fromIntegral (stackPointer - stackBase)) ReadWrite (B.replicate (fromIntegral (stackPointer - stackBase)) 0)
        , region callerFrameId stackPointer (fromIntegral (stackTop - stackPointer)) ReadWrite (B.replicate (fromIntegral (stackTop - stackPointer)) 0)
        , region exitId exitAddress 4 ReadExecute (word32le 0xe12fff1e)
        ]
    , caseFunctions = [(N.GuestAddress (fromIntegral start), end - start) | FunctionSpan start end <- functions]
    , caseInitial =
        zip [R0, R1, R2, R3] arguments
          ++ [(register, sentinel register) | register <- calleeSaved]
          ++ [(R12, 0), (Sp, stackPointer), (Lr, exitAddress), (Cpsr, 0x10)]
    , caseExpectedStatus = expectedStatus expected
    , caseExpectedRegisters = expectedRegisters expected
    , caseExpectedMemory = expectedMemory expected
    }
  where
    hex value = "0x" ++ showHex value ""
    (entry, entryName, arguments) = case operation of
      Get bank mode (SlotArgument slot) -> (0x61e110 :: Word32, "getter 0x61e110", [identifier, bankWord bank, fromIntegral (fromEnum mode), slot])
      Set bank slot@(SlotArgument slotWord) choice -> (0x624694, "setter 0x624694", [identifier, setValue standing slot choice, bankWord bank, slotWord])
    bankWord = fromIntegral . fromEnum

-- | One image4 word replaced; 'applyPatches' refuses when the word is not 'patchBefore'.
data Patch = Patch { patchAddress :: Word32, patchBefore :: Word32, patchAfter :: Word32 } deriving (Eq, Show)

applyPatches :: [Patch] -> B.ByteString -> Either String B.ByteString
applyPatches patches image = foldM one image patches
  where
    one bytes (Patch address before after) = do
      let at = fromIntegral (address - image4Base)
      unless (address >= image4Base && address .&. 3 == 0 && at + 4 <= B.length bytes) (Left ("patch outside image4 at 0x" ++ showHex address ""))
      let found = readWordAt bytes at
      unless (found == before) (Left ("word at 0x" ++ showHex address "" ++ " is 0x" ++ showHex found "" ++ ", not 0x" ++ showHex before ""))
      Right (B.take at bytes <> word32le after <> B.drop (at + 4) bytes)

-- | A change to one owned field's PROPERTIES entry.
data EntryControl = OffsetMoved | SaveCleared | DefaultFlipped | TailOffsetMoved deriving (Eq, Show, Enum, Bounded)

mutateArm :: EntryControl -> FieldArm -> Maybe FieldArm
mutateArm control arm = case control of
  OffsetMoved ->
    let step = widthBytes (fieldWidth arm)
        BankOffset offsetWord = fieldOffset arm
        offset = fromIntegral offsetWord
        moved = if offset + 2 * step <= bankStrideBytes then offset + step else offset - step
     in Just arm {fieldOffset = BankOffset (fromIntegral moved :: Word16)}
  SaveCleared -> if fieldSave arm == SaveBank0Previous then Just arm {fieldSave = NoSave} else Nothing
  DefaultFlipped -> case fieldDefault arm of
    DefaultConstant value -> Just arm {fieldDefault = DefaultConstant (value `xor` 1)}
    DefaultTail _ _ -> Nothing
  TailOffsetMoved -> case fieldDefault arm of
    DefaultTail (TailOffset tail') width -> Just arm {fieldDefault = DefaultTail (TailOffset (tail' + 1)) width}
    DefaultConstant _ -> Nothing

entryControlOperations :: EntryControl -> [Operation]
entryControlOperations control = filter relevant operations
  where
    relevant operation = case (control, operation) of
      (OffsetMoved, Get _ mode _) -> mode == LiveRead
      (OffsetMoved, Set _ _ _) -> True
      (SaveCleared, Set bank _ _) -> bank == Bank0
      (SaveCleared, Get _ _ _) -> False
      (DefaultFlipped, Get _ mode _) -> mode == DefaultRead
      (DefaultFlipped, Set _ _ choice) -> choice == SetDefaultRequest
      (TailOffsetMoved, Get _ mode _) -> mode == DefaultRead
      (TailOffsetMoved, Set _ _ choice) -> choice == SetDefaultRequest

-- | The first owned field whose mutated entry changes the model on one of the control's operations.
exposingField :: Fixture -> EntryControl -> [Property] -> Maybe (Property, FieldArm)
exposingField fx control properties = listToMaybe
  [ (property, mutated)
  | property@(Property _ _ _ (Owned (OwnedField arm))) <- properties
  , Just mutated <- [mutateArm control arm]
  , any (\operation -> modelEffect fx (OwnedField arm) operation /= modelEffect fx (OwnedField mutated) operation) (entryControlOperations control)
  ]

-- | A change to one slot field: to its table words (record offset, branches swapped, slot-1 default
-- taken from slot record 0) or to an accessor (record save at the bank offset, slot-defaults or
-- tail-defaults base moved).
data SlotControl
  = RecordOffsetMoved
  | BranchesSwapped
  | Slot1DefaultFromRecord0
  | RecordSavedAtBankOffset
  | SlotDefaultsMoved
  | TailDefaultsMoved
  deriving (Eq, Show, Enum, Bounded)

data SlotMutation = MutatedArm SlotArm | MutatedLayout Layout deriving (Eq, Show)

-- | The slot field's mutation, when the control applies to it.
mutateSlot :: Fixture -> SlotControl -> SlotArm -> Maybe SlotMutation
mutateSlot fx control arm@(SlotArm bank@(BankBranch _ bankWidth' bankBits' fallback) record@(RecordBranch (RecordOffset offset) width bits) save) = case control of
  RecordOffsetMoved ->
    let step = fromIntegral (widthBytes width)
        moved = if fromIntegral offset + 2 * step <= recordStride then fromIntegral offset + step else fromIntegral offset - step
     in Just (MutatedArm arm {recordArm = record {recordOffset = RecordOffset (fromIntegral (moved :: Word32))}})
  BranchesSwapped
    | (bankWidth', bankBits') /= (width, bits) ->
        Just (MutatedArm arm {bankArm = bank {bankWidth = width, bankBits = bits}, recordArm = record {recordWidth = bankWidth', recordBits = bankBits'}})
    | otherwise -> Nothing
  Slot1DefaultFromRecord0 -> case (fallback, load slotDefaultsWindow (fixtureSlotDefaults fx) (slotDefaultsBase + fromIntegral offset) width) of
    (BankConstant value, Just container)
      | let recordZero = extractRange bits container, recordZero /= value, recordZero <= maxDefault ->
          Just (MutatedArm arm {bankArm = bank {bankDefault = BankConstant recordZero}})
    _ -> Nothing
  RecordSavedAtBankOffset
    | save == SaveBank0Previous -> Just (MutatedLayout stockLayout {layoutRecordSave = RecordSaveAtBankOffset})
    | otherwise -> Nothing
  SlotDefaultsMoved -> Just (MutatedLayout stockLayout {layoutSlotDefaults = slotDefaultsBase + recordStride})
  TailDefaultsMoved -> case fallback of
    BankTailByte _ -> Just (MutatedLayout stockLayout {layoutTailDefaults = tailDefaultsBase + 1})
    BankConstant _ -> Nothing

-- | The cases a control runs: those its mutation can change, within the fixture.
slotControlOperations :: SlotControl -> [Operation]
slotControlOperations control = filter relevant operations
  where
    record slot = not (bankBranchSelected slot)
    defaultRead operation = case operation of
      Get _ DefaultRead _ -> True
      Set _ _ SetDefaultRequest -> True
      _ -> False
    slotOf operation = case operation of
      Get _ _ slot -> slot
      Set _ slot _ -> slot
    relevant operation = case control of
      RecordOffsetMoved -> record (slotOf operation)
      BranchesSwapped -> True
      Slot1DefaultFromRecord0 -> defaultRead operation && bankBranchSelected (slotOf operation)
      RecordSavedAtBankOffset -> case operation of
        Set Bank0 (SlotArgument 0) _ -> True
        _ -> False
      SlotDefaultsMoved -> defaultRead operation && slotOf operation `elem` map SlotArgument [0, 2, 0xffffffff]
      TailDefaultsMoved -> defaultRead operation && bankBranchSelected (slotOf operation)

-- | The first slot field whose mutation changes the model, both inside the fixture, on one of the control's operations.
exposingSlot :: Fixture -> SlotControl -> [Property] -> Maybe (Property, SlotArm, SlotMutation)
exposingSlot fx control properties = listToMaybe
  [ (property, arm, mutation)
  | property@(Property _ _ _ (Owned (OwnedSlotField arm))) <- properties
  , Just mutation <- [mutateSlot fx control arm]
  , or [before /= after | operation <- slotControlOperations control, Just before <- [modelEffect fx (OwnedSlotField arm) operation], Just after <- [mutated mutation arm operation]]
  ]
  where
    mutated mutation arm operation = case mutation of
      MutatedArm changed -> modelEffect fx (OwnedSlotField changed) operation
      MutatedLayout layout -> modelEffectIn layout fx (OwnedSlotField arm) operation

-- | A32 @b target@ at @site@.
branchWord :: Word32 -> Word32 -> Word32
branchWord site target = 0xea000000 .|. (((target - site - 8) `shiftR` 2) .&. 0xffffff)

-- | A32 @movw r12, #low@ and @movt r12, #high@ for the low and high halves of an address.
movwWord, movtWord :: Word32 -> Word32
movwWord address = 0xe300c000 .|. ((address `shiftR` 12 .&. 0xf) `shiftL` 16) .|. (address .&. 0xfff)
movtWord address = 0xe340c000 .|. ((address `shiftR` 28) `shiftL` 16) .|. ((address `shiftR` 16) .&. 0xfff)

