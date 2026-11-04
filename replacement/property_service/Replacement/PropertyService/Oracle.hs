{-# LANGUAGE GHC2021 #-}

-- | Native cases of the X-M1 1.50 property getter 0x61e110 and setter 0x624694: the case grid, the
-- shared fixture, the owned-semantics model, the per-case comparison and the guest-control patches.
module Replacement.PropertyService.Oracle
  ( Bank (..)
  , ReadMode (..)
  , Slot (..)
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
  , Effect (..)
  , modelEffect
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
  , branchWord
  , movR12FromR4
  , movR12Zero
  , movR4FromR12
  , movR6FromR12
  , movR12FromR6
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

data Slot = Slot0 | Slot1 deriving (Eq, Ord, Show, Enum, Bounded)

-- | The setter's argument 1, resolved against the field by 'setValue'.
data SetChoice = SetZero | SetOne | SetFieldMax | SetAboveMax | SetAllOnes | SetDefaultRequest
  deriving (Eq, Ord, Show, Enum, Bounded)

data Operation = Get Bank ReadMode Slot | Set Bank Slot SetChoice deriving (Eq, Ord, Show)

data OracleCase = OracleCase { oracleProperty :: PropertyId, oracleOperation :: Operation } deriving (Eq, Ord, Show)

-- | A case's identifier within a batch and its file stem.
newtype CaseLabel = CaseLabel { caseLabelText :: String } deriving (Eq, Ord, Show)

-- | What the oracle knows of a property id: a classified class, or outside the 645-entry tables.
data Standing = Classified Class | OutOfRange deriving (Eq, Show)

propertyIds :: [PropertyId]
propertyIds = [PropertyId n | n <- [0 .. 646]]

operations :: [Operation]
operations =
  [Get bank mode slot | bank <- [minBound .. maxBound], mode <- [minBound .. maxBound], slot <- [minBound .. maxBound]]
    ++ [Set bank slot choice | bank <- [minBound .. maxBound], slot <- [minBound .. maxBound], choice <- [minBound .. maxBound]]

caseGrid :: [OracleCase]
caseGrid = [OracleCase identifier operation | identifier <- propertyIds, operation <- operations]

caseLabel :: OracleCase -> CaseLabel
caseLabel (OracleCase (PropertyId n) operation) = CaseLabel ("p" ++ pad 3 (showHex n "") ++ "-" ++ operationLabel)
  where
    operationLabel = case operation of
      Get bank mode slot -> "get-" ++ bankLabel bank ++ "-" ++ modeLabel mode ++ "-" ++ slotLabel slot
      Set bank slot choice -> "set-" ++ bankLabel bank ++ "-" ++ slotLabel slot ++ "-" ++ choiceLabel choice
    bankLabel bank = "b" ++ show (fromEnum bank)
    slotLabel slot = "s" ++ show (fromEnum slot)
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

fieldMask :: FieldArm -> Word32
fieldMask arm = case fieldBits arm of BitRange _ bits -> (1 `shiftL` bits) - 1

setValue :: Standing -> SetChoice -> Word32
setValue standing choice = case choice of
  SetZero -> 0
  SetOne -> 1
  SetFieldMax -> maximal
  SetAboveMax -> maximal + 1
  SetAllOnes -> 0xffffffff
  SetDefaultRequest -> defaultRequest
  where
    maximal = case standing of
      Classified (Owned (OwnedField arm)) -> fieldMask arm
      _ -> 0x7f

-- | Initial bytes of the rw images every case shares.
data Fixture = Fixture
  { fixtureBanks :: B.ByteString
  , fixturePrevious :: B.ByteString
  , fixtureConfig :: B.ByteString
  }
  deriving (Eq, Show)

bankStrideBytes :: Int
bankStrideBytes = 0x1c2

-- | Bank 0 is an xorshift stream and bank 1 its complement; the previous-value and configuration
-- images are streams of other seeds.
fixture :: Fixture
fixture = Fixture (bank0 <> B.map complement bank0) (stream 0x1c2 0x2545f491) (stream 0x200 0x6c8e9cf5)
  where
    bank0 = stream bankStrideBytes 0x9e3779b9

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

offsetOf :: FieldArm -> Int
offsetOf arm = case fieldOffset arm of BankOffset offset -> fromIntegral offset

container :: B.ByteString -> Int -> Width -> Word32
container bytes at width = case width of
  Byte -> fromIntegral (B.index bytes at)
  Half -> fromIntegral (B.index bytes at) .|. (fromIntegral (B.index bytes (at + 1)) `shiftL` 8)

putContainer :: B.ByteString -> Int -> Width -> Word32 -> B.ByteString
putContainer bytes at width value = B.take at bytes <> encoded <> B.drop (at + widthBytes width) bytes
  where
    encoded = B.pack (take (widthBytes width) [fromIntegral value, fromIntegral (value `shiftR` 8)])

extractField :: FieldArm -> Word32 -> Word32
extractField arm value = case fieldBits arm of BitRange lsb _ -> (value `shiftR` lsb) .&. fieldMask arm

insertField :: FieldArm -> Word32 -> Word32 -> Word32
insertField arm value field = case fieldBits arm of
  BitRange lsb _ ->
    let mask = fieldMask arm `shiftL` lsb
     in (value .&. complement mask) .|. ((field `shiftL` lsb) .&. mask)

defaultWord :: FieldArm -> Word32
defaultWord arm = case fieldDefault arm of FieldDefault value -> value

data Effect = Effect
  { effectR0 :: Word32
  , effectBanks :: B.ByteString
  , effectPrevious :: B.ByteString
  }
  deriving (Eq, Show)

-- | The owned semantics: the return value and the final bank and previous-value images.
modelEffect :: Fixture -> Owned -> Operation -> Effect
modelEffect (Fixture banks previous _) owned operation = case (owned, operation) of
  (OwnedUnassigned, _) -> unchanged 0
  (OwnedField arm, Get bank mode _) -> unchanged $ case mode of
    DefaultRead -> defaultWord arm
    LiveRead -> extractField arm (container banks (bankAt bank arm) (fieldWidth arm))
  (OwnedField arm, Set bank _ choice) ->
    let width = fieldWidth arm
        value = setValue (Classified (Owned owned)) choice
        stored = if value == defaultRequest then defaultWord arm else value
        saved = case (bank, fieldSave arm) of
          (Bank0, SaveBank0Previous) ->
            let saveAt = offsetOf arm
             in putContainer previous saveAt width (insertField arm (container previous saveAt width) (extractField arm (container banks saveAt width)))
          _ -> previous
        storeAt = bankAt bank arm
     in Effect 1 (putContainer banks storeAt width (insertField arm (container banks storeAt width) stored)) saved
  where
    unchanged r0 = Effect r0 banks previous
    bankAt bank arm = bankStrideBytes * fromEnum bank + offsetOf arm

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
  [ (banksId, bankBase, 2 * bankStrideBytes)
  , (previousId, saveBase, bankStrideBytes)
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

modelExpectation :: Fixture -> Owned -> Operation -> Expectation
modelExpectation fx owned operation =
  Expectation Returned ((R0, effectR0 effect) : preserved)
    [(banksId, effectBanks effect), (previousId, effectPrevious effect), (configId, fixtureConfig fx), (callerFrameId, B.replicate (fromIntegral (stackTop - stackPointer)) 0)]
  where
    effect = modelEffect fx owned operation
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
  deriving (Eq, Show)

differenceKind :: Difference -> String
differenceKind difference = case difference of
  StatusDiffers _ _ -> "status"
  RegisterDiffers _ _ _ -> "register"
  MemoryDiffers _ -> "memory"
  WritesDiffer -> "writes"
  ReferenceTruncated -> "truncated"
  SubjectTruncated -> "truncated"

differenceDetail :: Difference -> String
differenceDetail difference = case difference of
  StatusDiffers expected actual -> N.runnerStatusLabel actual ++ ", expected " ++ N.runnerStatusLabel expected
  RegisterDiffers register expected actual -> N.registerLabel register ++ " " ++ N.hexWord32 actual ++ ", expected " ++ N.hexWord32 expected
  MemoryDiffers ident -> "final bytes of " ++ regionName ident
  WritesDiffer -> "ordered write sequence"
  ReferenceTruncated -> "reference provenance truncated"
  SubjectTruncated -> "subject provenance truncated"

expectationDifferences :: Expectation -> Run -> [Difference]
expectationDifferences expected run =
  [StatusDiffers (expectedStatus expected) (runStatus run) | expectedStatus expected /= runStatus run]
    ++ [ RegisterDiffers register value actual
       | (register, value) <- expectedRegisters expected
       , let actual = fromMaybe (complement value) (lookup register (runRegisters run))
       , actual /= value
       ]
    ++ [MemoryDiffers ident | (ident, bytes) <- expectedMemory expected, lookup ident (runMemory run) /= Just bytes]

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
        , region slotDefaultsId 0x7cb804 0x78 ReadOnly (slice image 0x7cb804 0x78)
        , region tailDefaultsId 0x81b074 0x1c2 ReadOnly (slice image 0x81b074 0x1c2)
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
      Get bank mode slot -> (0x61e110 :: Word32, "getter 0x61e110", [identifier, bankWord bank, fromIntegral (fromEnum mode), slotWord slot])
      Set bank slot choice -> (0x624694, "setter 0x624694", [identifier, setValue standing choice, bankWord bank, slotWord slot])
    bankWord = fromIntegral . fromEnum
    slotWord = fromIntegral . fromEnum

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
data EntryControl = OffsetMoved | SaveCleared | DefaultFlipped deriving (Eq, Show, Enum, Bounded)

mutateArm :: EntryControl -> FieldArm -> Maybe FieldArm
mutateArm control arm = case control of
  OffsetMoved ->
    let step = widthBytes (fieldWidth arm)
        offset = offsetOf arm
        moved = if offset + 2 * step <= bankStrideBytes then offset + step else offset - step
     in Just arm {fieldOffset = BankOffset (fromIntegral moved :: Word16)}
  SaveCleared -> if fieldSave arm == SaveBank0Previous then Just arm {fieldSave = NoSave} else Nothing
  DefaultFlipped -> Just arm {fieldDefault = FieldDefault (defaultWord arm `xor` 1)}

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

-- | The first owned field whose mutated entry changes the model on one of the control's operations.
exposingField :: Fixture -> EntryControl -> [Property] -> Maybe (Property, FieldArm)
exposingField fx control properties = listToMaybe
  [ (property, mutated)
  | property@(Property _ _ _ (Owned (OwnedField arm))) <- properties
  , Just mutated <- [mutateArm control arm]
  , any (\operation -> modelEffect fx (OwnedField arm) operation /= modelEffect fx (OwnedField mutated) operation) (entryControlOperations control)
  ]

-- | A32 @b target@ at @site@.
branchWord :: Word32 -> Word32 -> Word32
branchWord site target = 0xea000000 .|. (((target - site - 8) `shiftR` 2) .&. 0xffffff)

-- | @mov r12, r4@, @mov r12, #0@, @mov r4, r12@, @mov r6, r12@ and @mov r12, r6@.
movR12FromR4, movR12Zero, movR4FromR12, movR6FromR12, movR12FromR6 :: Word32
movR12FromR4 = 0xe1a0c004
movR12Zero = 0xe3a0c000
movR4FromR12 = 0xe1a0400c
movR6FromR12 = 0xe1a0600c
movR12FromR6 = 0xe1a0c006
