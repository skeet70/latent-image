{-# LANGUAGE GHC2021 #-}

-- | Native cases of the X-M1 1.50 property getter 0x61e110 and setter 0x624694: the case grid, the
-- shared fixture, the owned-semantics model, the per-case comparison and the guest-control patches.
module Replacement.PropertyService.Oracle
  ( Bank (..)
  , ReadMode (..)
  , readModeValue
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
  , prefixedCaseLabel
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
  , configVariantsOf
  , ConfigRead (..)
  , configReads
  , configVariants
  , withConfigVariants
  , stockRead
  , stockResult
  , Effect (..)
  , Layout (..)
  , RecordSave (..)
  , OpenOrder (..)
  , stockLayout
  , backupWindow
  , callsBytes
  , modelEffect
  , modelEffectIn
  , Expectation (..)
  , modelExpectation
  , placeholderExpectation
  , observedExpectation
  , WriteEvent (..)
  , ApprovedRamHeader (..)
  , WriteProjectionError (..)
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
  , leafSpans
  , headerSpans
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
  , defaultOperations
  , variantOperations
  , allStockDefaults
  , withStock
  , stockOf
  , exposingStockSwap
  , exposingOpenAfterSet
  , bracketedOwned
  , branchLinkWord
  , branchWord
  , movwWord
  , movtWord
  , CallMode (..)
  , EntryCase (..)
  , entryCases
  , entryLabel
  , entryCaseSpec
  , ownedFunctions
  , Wrapper (..)
  , wrapperCaseSpec
  , image4Base
  ) where

import Control.Monad (foldM, unless)
import Data.Aeson (Value)
import Data.Bits (complement, shiftL, shiftR, xor, (.&.), (.|.))
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as BC
import Data.List (nub, sortOn)
import qualified Data.Map.Strict as M
import Data.Maybe (fromMaybe, isNothing, listToMaybe)
import Data.Word (Word16, Word32, Word64, Word8)
import qualified Emulation.NativeObservation as N
import Emulation.NativeObservation (Register (..), RegionId (..), RunnerStatus (..))
import Emulation.OwnedCase (CaseRegion, CaseSpec (..), Permission (..), mkRegion, readWordAt, word32le)
import Emulation.Sha256 (sha256, sha256Hex)
import FujiAF.Host (get, list, num, str)
import Numeric (readHex, showHex)
import PropertyFields (PropertyId (..))
import Replacement.PropertyService.Classify
import qualified Replacement.PropertyService.HeaderBoundary as H

data Bank = Bank0 | Bank1 deriving (Eq, Ord, Show, Enum, Bounded)

-- | The getter selects defaults only for argument 2 equal to 1.
data ReadMode = LiveRead | DefaultRead | LiveReadTwo | LiveReadAllOnes deriving (Eq, Ord, Show, Enum, Bounded)

readModeValue :: ReadMode -> Word32
readModeValue mode = case mode of
  LiveRead -> 0
  DefaultRead -> 1
  LiveReadTwo -> 2
  LiveReadAllOnes -> 0xffffffff

-- | Argument 3 of both functions; a slot field takes its bank branch for 1 and its record branch otherwise.
newtype SlotArgument = SlotArgument Word32 deriving (Eq, Ord, Show)

-- | 0xffffffff addresses the record one stride below slot 0.
slotArguments :: [SlotArgument]
slotArguments = map SlotArgument [0, 1, 2, 3, 0xffffffff]

-- | The setter's argument 1, resolved against the field by 'setValue'.
data SetChoice = SetZero | SetOne | SetFieldMax | SetAboveMax | SetAllOnes | SetDefaultRequest | SetBelowDefaultRequest | SetAboveDefaultRequest | SetRaw Word32
  deriving (Eq, Ord, Show)

data Operation = Get Bank ReadMode SlotArgument | Set Bank SlotArgument SetChoice deriving (Eq, Ord, Show)

data OracleCase = OracleCase { oracleProperty :: PropertyId, oracleOperation :: Operation } deriving (Eq, Ord, Show)

-- | A case's identifier within a batch and its file stem.
newtype CaseLabel = CaseLabel { caseLabelText :: String } deriving (Eq, Ord, Show)

prefixedCaseLabel :: CaseLabel -> CaseLabel -> CaseLabel
prefixedCaseLabel (CaseLabel prefix) (CaseLabel suffix) =
  CaseLabel (if length label <= 64 then label else 'c' : take 63 (sha256Hex (sha256 (BC.pack label))))
  where
    label = prefix ++ "-" ++ suffix

-- | What the oracle knows of a property id: a classified class, or outside the 645-entry tables.
data Standing = Classified Class | OutOfRange deriving (Eq, Show)

propertyIds :: [PropertyId]
propertyIds = map PropertyId ([0 .. 646] ++ [0x80000000, 0xffffffff])

operations :: [Operation]
operations =
  [Get bank mode slot | bank <- [minBound .. maxBound], mode <- [minBound .. maxBound], slot <- slotArguments]
    ++ [Set bank slot choice | bank <- [minBound .. maxBound], slot <- slotArguments, choice <- [SetZero, SetOne, SetFieldMax, SetAboveMax, SetAllOnes, SetDefaultRequest, SetBelowDefaultRequest, SetAboveDefaultRequest]]

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
      LiveReadTwo -> "live-two"
      LiveReadAllOnes -> "live-all-ones"
    choiceLabel choice = case choice of
      SetZero -> "zero"
      SetOne -> "one"
      SetFieldMax -> "max"
      SetAboveMax -> "above-max"
      SetAllOnes -> "all-ones"
      SetDefaultRequest -> "default-request"
      SetBelowDefaultRequest -> "below-default-request"
      SetAboveDefaultRequest -> "above-default-request"
      SetRaw value -> "raw-" ++ showHex value ""

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
  SetBelowDefaultRequest -> 0x1afffffe
  SetAboveDefaultRequest -> 0x1b000000
  SetRaw value -> value
  where
    maximal = case standing of
      Classified (Owned (OwnedField arm)) -> rangeMask (fieldBits arm)
      Classified (Owned (OwnedReadOnlyField arm)) -> rangeMask (fieldBits arm)
      Classified (Owned (OwnedWriteOnlyPair _)) -> 0xff
      Classified (Owned (OwnedSlotField arm))
        | bankBranchSelected slot -> rangeMask (bankBits (bankArm arm))
        | otherwise -> rangeMask (recordBits (recordArm arm))
      _ -> 0x7f

-- | Initial bytes of the rw images every case shares, the image4 windows of the slot-defaults
-- records and the defaults table, and the variant configuration images V1-V3.
data Fixture = Fixture
  { fixtureBanks :: B.ByteString
  , fixturePrevious :: B.ByteString
  , fixtureConfig :: B.ByteString
  , fixtureSlotDefaults :: B.ByteString
  , fixtureTailDefaults :: B.ByteString
  , fixtureConfigVariants :: [B.ByteString]
  , fixtureBackupKeys :: [H.BackupKey]
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
-- previous-value and configuration images are streams of other seeds. The defaults windows are
-- image4's. The configuration variants are the stream until `withConfigVariants` chooses them.
fixture :: B.ByteString -> Fixture
fixture image4 =
  Fixture
    (bank0 <> B.map complement bank0 <> stream bankStrideBytes 0x85ebca6b)
    (stream previousBytes 0x2545f491)
    config
    (slice image4 slotDefaultsWindow slotDefaultsBytes)
    (slice image4 tailDefaultsBase tailDefaultsBytes)
    (replicate 3 config)
    (H.keysFromImage image4)
  where
    bank0 = stream bankStrideBytes 0x9e3779b9
    config = stream 0x200 0x6c8e9cf5

-- | A configuration byte an owned arm reads, as an offset from 0x96b400, what its value decides,
-- and the values a variant tries for it, in order.
data ConfigRead = ConfigRead
  { readOffset :: Int
  , readOutcome :: Word8 -> Word32
  , readPreference :: [Word8]
  , readLeaf :: Bool
  }

-- | The stock leaves' bytes, each gate byte (zero or not), and each configuration default's byte,
-- tried with bits 5-7 set first.
configReads :: [Owned] -> [ConfigRead]
configReads owned =
  [ConfigRead offset leaf [minBound .. maxBound] True | stock <- nub (concatMap stocks owned), let (offset, leaf) = stockRead stock]
    ++ [ConfigRead (fromIntegral gate) (\b -> if b == 0 then 0 else 1) [minBound .. maxBound] False | gate <- nub (concatMap gates owned)]
    ++ [ConfigRead (fromIntegral config) fromIntegral ([0xe0 .. 0xff] ++ [0 .. 0xdf]) False | config <- nub (concatMap configs owned)]
  where
    stocks arm = case arm of
      OwnedField (FieldArm _ _ _ (FieldStock stock _) _ _) -> [stock]
      OwnedSlotField (SlotArm _ _ _ (SlotStock stock) _) -> [stock]
      _ -> []
    gates arm = case arm of
      OwnedField (FieldArm _ _ _ _ _ (ConfigGatedResave (ConfigOffset gate))) -> [gate]
      _ -> []
    configs arm = case arm of
      OwnedField (FieldArm _ _ _ (FieldConfig (ConfigOffset config) _) _ _) -> [config]
      _ -> []

-- | Three variants of `base`, each setting every read byte to the first value in its preference
-- whose outcome no earlier image (the base or an earlier variant) gave that byte and, for a leaf's
-- byte, that no other leaf's byte gives in the same variant; failing both, the first new outcome,
-- then the first outcome distinct within the variant, then the first value.
configVariants :: [ConfigRead] -> B.ByteString -> [B.ByteString]
configVariants wanted base = take 3 (go (M.fromList [(readOffset r, maybe [] (\b -> [readOutcome r b]) (configByteAt base (readOffset r))) | r <- wanted]))
  where
    go seen =
      let (chosen, seen') = foldl choose ([], seen) wanted
       in foldl (\bytes (offset, value) -> B.take offset bytes <> B.singleton value <> B.drop (offset + 1) bytes) base chosen : go seen'
    choose (chosen, seen) r =
      let earlier = M.findWithDefault [] (readOffset r) seen
          sameVariant = [outcome | readLeaf r, (offset, value) <- chosen, Just other <- [lookup offset leaves], let outcome = other value]
          new b = readOutcome r b `notElem` earlier
          distinct b = readOutcome r b `notElem` sameVariant
          pick = case [b | b <- readPreference r, new b, distinct b] ++ [b | b <- readPreference r, new b] ++ [b | b <- readPreference r, distinct b] ++ readPreference r of
            b : _ -> b
            [] -> 0
       in (chosen ++ [(readOffset r, pick)], M.insertWith (++) (readOffset r) [readOutcome r pick] seen)
    leaves = [(readOffset r, readOutcome r) | r <- wanted, readLeaf r]

-- | The fixture with configuration variants chosen for the owned arms' configuration reads.
withConfigVariants :: [Owned] -> Fixture -> Fixture
withConfigVariants owned fx = fx {fixtureConfigVariants = configVariants (configReads owned) (fixtureConfig fx)}

-- | The shared fixture, or one with a changed window: `TailWindow` sets the defaults-table byte at
-- 0x81b075, the high byte of 0x1e6's halfword default, to 0x5a; `ConfigV1`-`ConfigV3` replace the
-- configuration image.
data FixtureVariant = CanonicalFixture | TailWindow | ConfigV1 | ConfigV2 | ConfigV3 deriving (Eq, Ord, Show, Enum, Bounded)

configVariantsOf :: [FixtureVariant]
configVariantsOf = [ConfigV1, ConfigV2, ConfigV3]

variantFixture :: Fixture -> FixtureVariant -> Fixture
variantFixture fx variant = case variant of
  CanonicalFixture -> fx
  TailWindow -> fx {fixtureTailDefaults = replaceByte 1 0x5a (fixtureTailDefaults fx)}
  _ -> case drop (fromEnum variant - fromEnum ConfigV1) (fixtureConfigVariants fx) of
    config : _ -> fx {fixtureConfig = config}
    [] -> fx
  where
    replaceByte at value bytes = B.take at bytes <> B.singleton value <> B.drop (at + 1) bytes

variantTag :: FixtureVariant -> String
variantTag variant = case variant of
  CanonicalFixture -> "canonical"
  TailWindow -> "tail-window"
  ConfigV1 -> "config-v1"
  ConfigV2 -> "config-v2"
  ConfigV3 -> "config-v3"

variantNote :: FixtureVariant -> String
variantNote variant = case variant of
  CanonicalFixture -> "the shared fixture with image4's defaults windows"
  TailWindow -> "defaults-table byte 0x81b075 set to 0x5a, so 0x1e6's halfword default differs from its low byte"
  _ -> "configuration image " ++ show (fromEnum variant - fromEnum ConfigV1 + 1) ++ " of 3, chosen so the stock leaves, gate bytes and configuration defaults take outcomes the stream and the earlier variants do not"

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

-- | r0 and the final bank, previous-value and calls images.
data Effect = Effect
  { effectR0 :: Word32
  , effectBanks :: B.ByteString
  , effectPrevious :: B.ByteString
  , effectCalls :: B.ByteString
  }
  deriving (Eq, Show)

-- | Where a bank-0 save of a slot record goes: its own record offset in 0x89ec00, or the bank
-- branch's offset there (a control).
data RecordSave = RecordSaveAtRecord | RecordSaveAtBankOffset deriving (Eq, Show)

-- | Whether a bracketed bank-0 set's open call comes before its first save and store, or after them
-- (a control).
data OpenOrder = OpenBeforeSet | OpenAfterSet deriving (Eq, Show)

-- | The defaults bases, record save and open order the owned code uses; controls change one of them.
data Layout = Layout
  { layoutSlotDefaults :: Word32
  , layoutTailDefaults :: Word32
  , layoutRecordSave :: RecordSave
  , layoutOpen :: OpenOrder
  }
  deriving (Eq, Show)

stockLayout :: Layout
stockLayout = Layout slotDefaultsBase tailDefaultsBase RecordSaveAtRecord OpenBeforeSet

-- | Internal model summary: open and close markers, then the header observed at close.
callsBytes :: Int
callsBytes = 0x20

-- | The read-only window the open copies over bank-0 bytes [0, 16): their complement.
backupWindow :: Fixture -> B.ByteString
backupWindow fx = B.map complement (B.take 16 (fixtureBanks fx))

-- | Refresh bank 0 from the independently supplied shadow header.
openCall :: B.ByteString -> (B.ByteString, B.ByteString) -> (B.ByteString, B.ByteString)
openCall backup (banks, calls) = (backup <> B.drop 16 banks, placeAt 0 (word32le bankBase) calls)

closeCall :: (B.ByteString, B.ByteString) -> B.ByteString
closeCall (banks, calls) = placeAt 0x10 (B.take 16 banks) (placeAt 4 (word32le bankBase) calls)

placeAt :: Int -> B.ByteString -> B.ByteString -> B.ByteString
placeAt at replacement bytes = B.take at bytes <> replacement <> B.drop (at + B.length replacement) bytes

modelEffect :: Fixture -> Owned -> Operation -> Maybe Effect
modelEffect = modelEffectIn stockLayout

-- | The configuration byte a stock leaf reads, as an offset from 0x96b400, and its result for that
-- byte, transcribed from image4 (0x61df44: `0x61df58`-`0x61e100`; 0x61d8a4: `0x61d8b4`-`0x61d8cc`;
-- 0x61d59c: `0x61d5ac`-`0x61d5e0`; 0x61d618: `0x61d628`-`0x61d64c`; 0x61d5f0: `0x61d600`-`0x61d608`;
-- 0x61d65c: `0x61d688`-`0x61d894` and its two twins for selectors 3 and 4).
stockRead :: StockDefault -> (Int, Word8 -> Word32)
stockRead stock = case stock of
  Stock61df44 -> (0x35, \b -> if b <= 34 then fromIntegral b else 1)
  Stock61d8a4 -> (0xeb, \b -> if b == 1 || b == 2 then fromIntegral b else 0)
  Stock61d59c -> (0x27, \b -> if b <= 3 then 2 else if b <= 8 then 1 else if b == 0x81 then 1 else if b == 0x82 then 2 else 0)
  Stock61d618 -> (0x84, \b -> if b >= 1 && b <= 3 then fromIntegral b else 0)
  Stock61d5f0 -> (0x28, \b -> if b == 1 then 0 else 1)
  Stock61d65c selector -> (0x81 + fromEnum selector, profile)
  where
    profile b
      | b .&. 0x80 /= 0 = case b of
          0x86 -> 2
          0x87 -> 3
          0x88 -> 4
          _ -> 1
      | b <= 10 = [7, 7, 7, 7, 8, 11, 14, 17, 20, 23, 25] !! fromIntegral b
      | otherwise = 8

configByteAt :: B.ByteString -> Int -> Maybe Word8
configByteAt config offset
  | offset >= 0 && offset < B.length config = Just (B.index config offset)
  | otherwise = Nothing

stockResult :: StockDefault -> B.ByteString -> Maybe Word32
stockResult stock config = let (offset, leaf) = stockRead stock in leaf <$> configByteAt config offset

-- | One field a set writes: its container width and bits, the save policy and bank, its address in
-- the bank images, and the addresses a bank-0 save reads and writes the previous-value container at.
data WriteTarget = WriteTarget Width BitRange SavePolicy Bank Word32 Word32 Word32

withoutSave :: WriteTarget -> WriteTarget
withoutSave (WriteTarget width bits _ bank live savedFrom savedTo) = WriteTarget width bits NoSave bank live savedFrom savedTo

-- | The bank images and the previous-value image.
type Images = (B.ByteString, B.ByteString)

-- | On bank 0 with a save, the old bits go from the live container into the previous-value
-- container read at `savedFrom` and written at `savedTo`; then the value is inserted.
writeField :: WriteTarget -> Word32 -> Images -> Maybe Images
writeField (WriteTarget width bits save bank live savedFrom savedTo) stored (banks, previous) = do
  saved <-
    if bank == Bank0 && save == SaveBank0Previous
      then do
        old <- load bankBase banks live width
        container <- load saveBase previous savedFrom width
        store saveBase previous savedTo width (insertRange bits container (extractRange bits old))
      else Just previous
  container <- load bankBase banks live width
  updated <- store bankBase banks live width (insertRange bits container stored)
  pure (updated, saved)

-- | The owned semantics: the return value and the final bank, previous-value and calls images, or
-- Nothing when an access falls outside the fixture.
modelEffectIn :: Layout -> Fixture -> Owned -> Operation -> Maybe Effect
modelEffectIn layout fx@(Fixture banks previous config slotDefaults tailDefaults _ _) owned operation = case (owned, operation) of
  (OwnedUnassigned, _) -> Just (unchanged 0)
  (OwnedReadOnlyField arm, Get bank mode _) -> getField arm bank mode
  (OwnedReadOnlyField _, Set {}) -> Just (unchanged 0)
  (OwnedWriteOnlyPair _, Get {}) -> Just (unchanged 0)
  (OwnedWriteOnlyPair (PairArm first second value save), Set bank slot choice) -> do
    let stored = fromMaybe value (requested slot choice)
        byteAt offset = writeField (WriteTarget Byte (BitRange 0 8) save bank (bankAddress bank offset) (saveAddress offset) (saveAddress offset)) stored
    case bank of
      Bank0 -> bracketed (byteAt first) (byteAt second)
      Bank1 -> done <$> (byteAt first initial >>= byteAt second)
  (OwnedField arm, Get bank mode _) -> getField arm bank mode
  (OwnedField (FieldArm offset width bits source save hook), Set bank slot choice) -> do
    let target = WriteTarget width bits save bank (bankAddress bank offset) (saveAddress offset) (saveAddress offset)
        writes from = do
          (images, stored) <- case (requested slot choice, source) of
            (Just value, _) -> (,) <$> writeField target value from <*> pure value
            (Nothing, FieldDefaultOnly fallback) -> do
              value <- fieldDefault fallback
              (,) <$> writeField target value from <*> pure value
            (Nothing, FieldStock stock provisional) -> do
              result <- stockResult stock config
              (,) <$> (writeField target provisional from >>= writeField target result) <*> pure result
            (Nothing, FieldConfig offset' provisional) -> do
              byte <- configValue offset'
              (,) <$> (writeField target provisional from >>= writeField (withoutSave target) byte) <*> pure byte
          case hook of
            ConfigGatedResave gate -> configValue gate >>= \byte -> if byte == 0 then writeField target stored images else Just images
            _ -> Just images
    case (hook, bank) of
      (Bank0Bracket, Bank0) -> bracketed writes Just
      _ -> done <$> writes initial
  (OwnedSlotField (SlotArm (BankBranch offset width bits fallback) (RecordBranch record recordWidth' recordBits') _ slotDefault' mirror), Get bank mode requestedSlot) ->
    let slot = if mirror == MirrorRecord0 then SlotArgument 0 else requestedSlot
     in unchanged <$> case (mode, slotDefault') of
          (DefaultRead, SlotStock stock) -> stockResult stock config
          _
            | bankBranchSelected slot -> case mode of
                DefaultRead -> bankDefault fallback
                _ -> extractRange bits <$> load bankBase banks (bankAddress bank offset) width
            | otherwise -> extractRange recordBits' <$> case mode of
                DefaultRead -> load slotDefaultsWindow slotDefaults (slotDefaultsAddress slot record) recordWidth'
                _ -> load bankBase banks (recordAddress bank slot record) recordWidth'
  (OwnedSlotField (SlotArm (BankBranch offset width bits fallback) (RecordBranch record recordWidth' recordBits') save slotDefault' mirror), Set bank slot choice) -> do
    let recordTarget recordSlot =
          let recordSaved = saveBase + recordBase + slotStride recordSlot + recordOffsetWord record
              savedTo = case layoutRecordSave layout of
                RecordSaveAtRecord -> recordSaved
                RecordSaveAtBankOffset -> saveAddress offset
           in WriteTarget recordWidth' recordBits' save bank (recordAddress bank recordSlot record) recordSaved savedTo
        recordDefault recordSlot = extractRange recordBits' <$> load slotDefaultsWindow slotDefaults (slotDefaultsAddress recordSlot record) recordWidth'
        (target, stage2Default)
          | bankBranchSelected slot = (WriteTarget width bits save bank (bankAddress bank offset) (saveAddress offset) (saveAddress offset), bankDefault fallback)
          | otherwise = (recordTarget slot, recordDefault slot)
    images <- case requested slot choice of
      Just value -> writeField target value initial
      Nothing -> do
        provisional <- stage2Default
        case slotDefault' of
          SlotDefaultsRecord -> writeField target provisional initial
          SlotStock stock -> stockResult stock config >>= \result -> writeField target provisional initial >>= writeField target result
    final <- case mirror of
      NoMirror -> Just images
      MirrorRecord0 -> do
        value <- maybe (recordDefault (SlotArgument 0)) Just (requested slot choice)
        writeField (recordTarget (SlotArgument 0)) value images
    pure (done final)
  where
    initial = (banks, previous)
    initialCalls = B.replicate callsBytes 0
    unchanged r0 = Effect r0 banks previous initialCalls
    finished (banks', previous') = Effect 1 banks' previous'
    done images = finished images initialCalls
    -- A bank-0 set between the open and close calls, then `after`.
    bracketed inside after = do
      (written, logged) <- case layoutOpen layout of
        OpenBeforeSet -> do
          let (opened, logged) = openCall (backupWindow fx) (banks, initialCalls)
          written <- inside (opened, previous)
          pure (written, logged)
        OpenAfterSet -> do
          (written, saved) <- inside initial
          let (opened, logged) = openCall (backupWindow fx) (written, initialCalls)
          pure ((opened, saved), logged)
      final <- after written
      pure (finished final (closeCall (fst written, logged)))
    requested slot choice =
      let value = setValue (Classified (Owned owned)) slot choice
       in if value == defaultRequest then Nothing else Just value
    getField (FieldArm offset width bits source _ _) bank mode = unchanged <$> case (mode, source) of
      (DefaultRead, FieldDefaultOnly fallback) -> fieldDefault fallback
      (DefaultRead, FieldStock stock _) -> stockResult stock config
      (DefaultRead, FieldConfig offset' _) -> configValue offset'
      _ -> extractRange bits <$> load bankBase banks (bankAddress bank offset) width
    configValue :: ConfigOffset -> Maybe Word32
    configValue (ConfigOffset offset) = fromIntegral <$> configByteAt config (fromIntegral offset)
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

banksId, previousId, configId, slotDefaultsId, tailDefaultsId, stackId, callerFrameId, exitId, backingId, callsId :: RegionId
banksId = RegionId "banks"
previousId = RegionId "previous"
configId = RegionId "config"
slotDefaultsId = RegionId "slot-defaults"
tailDefaultsId = RegionId "tail-defaults"
stackId = RegionId "stack"
callerFrameId = RegionId "caller-frame"
exitId = RegionId "exit"
backingId = RegionId "backing"
callsId = RegionId "calls"

-- | The rw regions whose final bytes and writes the verdict compares; the stack below entry sp is scratch.
comparedRegions :: [(RegionId, Word32, Int)]
comparedRegions =
  [ (banksId, bankBase, bankCount * bankStrideBytes)
  , (previousId, saveBase, previousBytes)
  , (configId, configBase, 0x200)
  , (callsId, H.callsBase, H.callsBytes)
  , (backingId, H.backingBase, 0x1ac)
  , (callerFrameId, stackPointer, fromIntegral (stackTop - stackPointer))
  ]

stackBase, stackTop, exitAddress, image4Base :: Word32
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
      let (backing,events,previous) = headerEffects fx effect
      in Expectation Returned ((R0, effectR0 effect) : preserved)
        [(banksId, effectBanks effect), (previousId, previous), (configId, fixtureConfig fx), (callsId, H.encodeEvents events), (backingId, backing), (callerFrameId, B.replicate (fromIntegral (stackTop - stackPointer)) 0)]
    preserved = [(register, sentinel register) | register <- calleeSaved] ++ [(Sp, stackPointer)] ++ case operation of
      Set _ (SlotArgument slot) _ -> [(R3, slot)]
      Get {} -> []

headerEffects :: Fixture -> Effect -> (B.ByteString,[H.BoundaryEvent],B.ByteString)
headerEffects fx effect = (finalBacking,events,previous)
  where
    summary = effectCalls effect
    opened = readWordAt summary 0 /= 0
    closed = readWordAt summary 4 /= 0
    initial = H.initialBacking (fixtureBanks fx)
    sealed = H.sealedHeader (B.take 16 (B.drop 16 summary))
    finalBacking = if closed then placeAt 0x19c sealed (placeAt 0 sealed initial) else initial
    drivers = [H.BoundaryEvent 0x6a59b8 (fromIntegral key) (fromIntegral value) (fromIntegral old * 256 + fromIntegral deferred * 65536)
      | (index,H.BackupKey key) <- zip [0..15] (fixtureBackupKeys fx)
      , let value = B.index sealed index
      , let old = B.index (backupWindow fx) index
      , value /= old]
    deferred = B.index (fixtureBanks fx) 0x126
    events = (if opened then H.requestEvents [] else []) ++ (if closed then H.requestEvents drivers else [])
    previous = if opened || closed then placeAt 0x1c4 (word32le 0) (effectPrevious effect) else effectPrevious effect

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

data ApprovedRamHeader = ActiveHeader | BackupHeader deriving (Eq, Show)

data WriteProjectionError
  = InvalidWriteWidth WriteEvent
  | WriteAddressOverflow WriteEvent
  | HeaderBoundaryCrossing ApprovedRamHeader WriteEvent
  deriving (Eq, Show)

data WriteWidth = WriteByte | WriteHalf | WriteWord | WriteDouble deriving (Eq, Show)

newtype RamByteAddress = RamByteAddress Word32 deriving (Eq, Show)

data ComparedWrite
  = PhysicalWrite WriteEvent
  | RamByteEffect RamByteAddress Word8
  deriving (Eq, Show)

-- | HeaderBoundary.initialBacking and headerEffects expose the first 16 bytes at each base.
-- settings_storage's HEADER_BYTES and NativeHeaderBackend use these ordinary-RAM arrays.
-- This fixture contract establishes no interrupt, concurrent-access or silicon atomic-width equivalence.
approvedRamHeaders :: [(ApprovedRamHeader, Word32, Word32)]
approvedRamHeaders =
  [(ActiveHeader, bankBase, bankBase + 16), (BackupHeader, H.backingBase, H.backingBase + 16)]

projectWrites :: [WriteEvent] -> Either WriteProjectionError [ComparedWrite]
projectWrites = fmap concat . mapM project
  where
    project event = do
      width <- case eventWidth event of
        1 -> Right WriteByte
        2 -> Right WriteHalf
        4 -> Right WriteWord
        8 -> Right WriteDouble
        _ -> Left (InvalidWriteWidth event)
      let size = case width of
            WriteByte -> 1
            WriteHalf -> 2
            WriteWord -> 4
            WriteDouble -> 8
          start = toInteger (eventAddress event)
          end = start + size
          overlaps = [(header, toInteger low, toInteger high) | (header, low, high) <- approvedRamHeaders, start < toInteger high, end > toInteger low]
      if end > toInteger (maxBound :: Word32) + 1
        then Left (WriteAddressOverflow event)
        else case overlaps of
          [] -> Right [PhysicalWrite event]
          [(header, low, high)]
            | start >= low && end <= high ->
                Right [RamByteEffect (RamByteAddress (fromInteger (start + offset))) (fromIntegral (eventValue event `shiftR` fromInteger (8 * offset))) | offset <- [0 .. size - 1]]
            | otherwise -> Left (HeaderBoundaryCrossing header event)
          (header, _, _) : _ -> Left (HeaderBoundaryCrossing header event)

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
  sum (map snd (runRegisters run)) `seq` sum (map (B.length . snd) (runMemory run)) `seq` maybe () forceEvents (runWrites run) `seq` run
  where
    forceEvents = foldr (\event rest -> event `seq` rest) ()

data Difference
  = StatusDiffers RunnerStatus RunnerStatus
  | RegisterDiffers Register Word32 Word32
  | MemoryDiffers RegionId
  | WritesDiffer
  | ReferenceTruncated
  | SubjectTruncated
  | ReferenceWriteProjectionFailed WriteProjectionError
  | SubjectWriteProjectionFailed WriteProjectionError
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
  ReferenceWriteProjectionFailed _ -> "writes"
  SubjectWriteProjectionFailed _ -> "writes"
  ModelOutsideFixture -> "model"

differenceDetail :: Difference -> String
differenceDetail difference = case difference of
  StatusDiffers expected actual -> N.runnerStatusLabel actual ++ ", expected " ++ N.runnerStatusLabel expected
  RegisterDiffers register expected actual -> N.registerLabel register ++ " " ++ N.hexWord32 actual ++ ", expected " ++ N.hexWord32 expected
  MemoryDiffers ident -> "final bytes of " ++ regionName ident
  WritesDiffer -> "ordered write effects (approved RAM bytes; physical events elsewhere)"
  ReferenceTruncated -> "reference provenance truncated"
  SubjectTruncated -> "subject provenance truncated"
  ReferenceWriteProjectionFailed problem -> "reference write projection: " ++ show problem
  SubjectWriteProjectionFailed problem -> "subject write projection: " ++ show problem
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
    ++ case (projectWrites <$> runWrites reference, projectWrites <$> runWrites subject) of
      (expected, actual) ->
        [ReferenceWriteProjectionFailed problem | Just (Left problem) <- [expected]]
          ++ [SubjectWriteProjectionFailed problem | Just (Left problem) <- [actual]]
          ++ [WritesDiffer | Just (Right wanted) <- [expected], Just (Right observed) <- [actual], wanted /= observed]

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
        unless (address <= fromIntegral (maxBound :: Word32)) (Left "provenance address outside 32-bit guest space")
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
  registers <- mapM (\register -> maybe (Left ("register " ++ N.registerLabel register)) (Right . (,) register) (N.lookupRegister register observation)) (R3 : comparedRegisters)
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

-- | The six stock leaves owned code calls, in the callee region [0x61d59c, 0x61e110).
leafSpans :: [FunctionSpan]
leafSpans =
  [ FunctionSpan 0x61d59c 0x61d5f0
  , FunctionSpan 0x61d5f0 0x61d618
  , FunctionSpan 0x61d618 0x61d65c
  , FunctionSpan 0x61d65c 0x61d8a4
  , FunctionSpan 0x61d8a4 0x61d8dc
  , FunctionSpan 0x61df44 0x61e110
  ]

-- | Stock header/request code executes unchanged; only device/runtime leaves are modeled.
headerSpans :: [FunctionSpan]
headerSpans = [FunctionSpan low high | (low,high) <- H.functionSpans]

-- | The mapped stock code: the callee region and the getter and setter.
codeStart :: Word32
codeStart = 0x61d59c

stockFunctions :: [FunctionSpan]
stockFunctions = leafSpans ++ headerSpans ++ [getterSpan, setterSpan]

-- | The stock spans with the setter cut around [start, end), plus the functions declared inside it.
splitFunctions :: (Word32, Word32) -> [FunctionSpan] -> [FunctionSpan]
splitFunctions (start, end) inner =
  sortOn spanStart (leafSpans ++ headerSpans ++ [getterSpan, FunctionSpan (spanStart setterSpan) start, FunctionSpan end (spanEnd setterSpan)] ++ inner)

-- | The three retained wrappers between the getter and the setter, one function each.
wrapperSpans :: [FunctionSpan]
wrapperSpans = [FunctionSpan 0x6245f8 0x624620, FunctionSpan 0x624620 0x62464c, FunctionSpan 0x62464c 0x624694]

ownedFunctions :: [FunctionSpan] -> [FunctionSpan]
ownedFunctions inner = sortOn spanStart (leafSpans ++ headerSpans ++ [FunctionSpan 0x61e110 0x61e114] ++ wrapperSpans ++ [FunctionSpan 0x624694 0x624698] ++ inner)

data CallMode = DirectCall | RegisterCall | TailCall deriving (Eq, Ord, Show, Enum, Bounded)

data EntryCase = EntryCase
  { entryOperation :: OracleCase
  , entryCallMode :: CallMode
  , entrySelector :: Word32
  , entryBank :: Word32
  , entrySlot :: SlotArgument
  , entryReturnVariant :: Bool
  } deriving (Eq, Ord, Show)

entryLabel :: EntryCase -> CaseLabel
entryLabel c = prefixedCaseLabel (CaseLabel "entry") (CaseLabel (caseLabelText (caseLabel (entryOperation c)) ++ "-" ++ show (fromEnum (entryCallMode c)) ++ "-" ++ showHex (entrySelector c) "" ++ "-" ++ showHex (entryBank c) "" ++ "-" ++ if entryReturnVariant c then "v1" else "v0"))

entryCases :: [EntryCase]
entryCases =
  [ EntryCase (OracleCase identifier operation) mode selector bank slot alternate
  | identifier <- nub (propertyIds ++ map PropertyId [647, 0x7fffffff, 0x80000000, 0xfffffffe, 0xffffffff])
  , bank <- if invalid identifier then [0, 1, 0x7fffffff, 0x80000000, 0xffffffff] else [0, 1]
  , slot <- if invalid identifier then map SlotArgument [0, 1, 2, 3, 0x7fffffff, 0x80000000, 0xffffffff] else slotArguments
  , (selector, operation) <- [(s, Get (normalBank bank) (if s == 1 then DefaultRead else LiveRead) slot) | s <- [0, 1, 2, 0xffffffff]]
      ++ [(0, Set (normalBank bank) slot (SetRaw value)) | value <- [0, 1, 0x1afffffe, 0x1affffff, 0x1b000000, 0xffffffff]]
  , mode <- [minBound .. maxBound]
  , alternate <- [False, True]
  ]
  where
    invalid (PropertyId identifier) = identifier == 0 || identifier > 645
    normalBank bank = if bank == 1 then Bank1 else Bank0

entryCaseSpec :: B.ByteString -> [FunctionSpan] -> Fixture -> CaseLabel -> Standing -> EntryCase -> Expectation -> CaseSpec
entryCaseSpec image functions fx label standing c expected =
  base
    { caseEntry = N.GuestAddress (fromIntegral caller)
    , caseExit = N.GuestAddress (fromIntegral returnTarget)
    , caseRegions = caseRegions base ++ [region (RegionId "entry-caller") caller 32 ReadExecute (B.concat (map word32le words'))]
    , caseFunctions = caseFunctions base ++ [(N.GuestAddress (fromIntegral caller), 24)]
    , caseInitial = map initial (caseInitial base)
    }
  where
    base = caseSpec image functions fx label standing (entryOperation c) expected
    caller = 0x958a00
    returnTarget = caller + if entryReturnVariant c then 28 else 24
    target = case oracleOperation (entryOperation c) of Get {} -> 0x61e110; Set {} -> 0x624694
    words' = case entryCallMode c of
      DirectCall -> [0xe92d4010, branchLinkWord (caller + 4) target, 0xe8bd8010, 0xe1a00000, target, 0xe1a00000, 0xe12fff1e, 0xe12fff1e]
      RegisterCall -> [0xe92d4010, 0xe59fc004, 0xe12fff3c, 0xe8bd8010, target, 0xe1a00000, 0xe12fff1e, 0xe12fff1e]
      TailCall -> [branchWord caller target, 0xe1a00000, 0xe1a00000, 0xe1a00000, target, 0xe1a00000, 0xe12fff1e, 0xe12fff1e]
    initial pair@(register, _) = case register of
      R12 -> (R12, 0x5a5a1234)
      Lr -> (Lr, returnTarget)
      R1 | Get {} <- oracleOperation (entryOperation c) -> (R1, entryBank c)
      R2 -> case oracleOperation (entryOperation c) of Get {} -> (R2, entrySelector c); Set {} -> (R2, entryBank c)
      R3 -> let SlotArgument value = entrySlot c in (R3, value)
      _ -> pair

data Wrapper = DefaultWrapper | LiveWrapper | CompareWrapper deriving (Eq, Ord, Show, Enum, Bounded)

wrapperCaseSpec :: Wrapper -> B.ByteString -> [FunctionSpan] -> Fixture -> CaseLabel -> Standing -> OracleCase -> CaseSpec
wrapperCaseSpec wrapper image functions fx label standing c =
  base
    { caseEntry = N.GuestAddress (fromIntegral target)
    , caseFunctions = [(N.GuestAddress (fromIntegral start), end - start) | FunctionSpan start end <- wrapperFunctions]
    , caseRegions = caseRegions base ++ [region (RegionId "wrapper-selector") 0x6439e0 8 ReadExecute (word32le 0xe3a00002 <> word32le 0xe12fff1e)]
    , caseInitial = map argument (caseInitial base)
    }
  where
    base = caseSpec image wrapperFunctions fx label standing c placeholderExpectation
    target :: Word32
    target = case wrapper of DefaultWrapper -> 0x6245f8; LiveWrapper -> 0x624620; CompareWrapper -> 0x62464c
    wrapperFunctions = nub (FunctionSpan 0x6439e0 0x6439e8 : wrapperSpans ++ [if f == getterSpan then FunctionSpan 0x61e110 0x6245f8 else f | f <- functions, f `notElem` [FunctionSpan 0x6245f8 0x624694]])
    argument pair@(register, _) = case (wrapper, register) of
      (DefaultWrapper, _) -> pair
      (LiveWrapper, R1) -> (R1, 1)
      (CompareWrapper, R1) -> (R1, 2)
      (CompareWrapper, R2) -> (R2, 1)
      (CompareWrapper, R3) -> (R3, 3)
      _ -> pair

codeBytes :: B.ByteString -> B.ByteString
codeBytes image = H.codeWindow image codeStart (fromIntegral (spanEnd setterSpan - codeStart))

slice :: B.ByteString -> Word32 -> Int -> B.ByteString
slice image address size = B.take size (B.drop (fromIntegral (address - image4Base)) image)

region :: RegionId -> Word32 -> Int -> Permission -> B.ByteString -> CaseRegion
region ident address = mkRegion ident (N.GuestAddress (fromIntegral address))

data CodeRegionSpan = CodeRegionSpan Word32 Word32 Permission

codeRegions :: B.ByteString -> [FunctionSpan] -> [CaseRegion]
codeRegions image functions =
  [ region (RegionId ("code-" ++ show start)) start (fromIntegral (end - start)) permission
      (B.take (fromIntegral (end - start)) (B.drop (fromIntegral (start - codeStart)) bytes))
  | CodeRegionSpan start end permission <- foldr join [] spans
  ]
  where
    bytes = codeBytes image
    boundaries = sortOn id (nub (codeStart : spanEnd setterSpan : [boundary | declared <- functions, boundary <- [spanStart declared, spanEnd declared], codeStart < boundary, boundary < spanEnd setterSpan]))
    spans =
      [ CodeRegionSpan start end (if any (\declared -> spanStart declared <= start && end <= spanEnd declared) functions then ReadExecute else ReadOnly)
      | (start, end) <- zip boundaries (drop 1 boundaries)
      ]
    join (CodeRegionSpan start end permission) (CodeRegionSpan nextStart nextEnd nextPermission : rest)
      | end == nextStart && samePermission permission nextPermission = CodeRegionSpan start nextEnd permission : rest
    join current rest = current : rest
    samePermission ReadExecute ReadExecute = True
    samePermission ReadOnly ReadOnly = True
    samePermission _ _ = False

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
        codeRegions image functions ++
        [ region slotDefaultsId H.keysBase (8 + B.length (fixtureSlotDefaults fx)) ReadOnly (B.take 8 (H.keyBytes (fixtureBackupKeys fx)) <> fixtureSlotDefaults fx)
        , region tailDefaultsId tailDefaultsBase (B.length (fixtureTailDefaults fx)) ReadOnly (fixtureTailDefaults fx)
        , region banksId bankBase (B.length (fixtureBanks fx)) ReadWrite (fixtureBanks fx)
        , region previousId saveBase (B.length (fixturePrevious fx)) ReadWrite (fixturePrevious fx)
        , region configId configBase (B.length (fixtureConfig fx)) ReadWrite (fixtureConfig fx)
        , region backingId H.backingBase 0x1ac ReadWrite (H.initialBacking (fixtureBanks fx))
        , region callsId H.callsBase H.callsBytes ReadWrite (B.replicate H.callsBytes 0)
        , region (RegionId "header-mirror-code") 0x6a59b8 (0x6a6fd8-0x6a59b8) ReadExecute (H.codeWindow image 0x6a59b8 (0x6a6fd8-0x6a59b8))
        , region (RegionId "header-runtime-code") 0x722054 (0x72b3fc-0x722054) ReadExecute (H.codeWindow image 0x722054 (0x72b3fc-0x722054))
        , region (RegionId "header-reporter") 0x633594 (4 * length (H.callback 0x633594)) ReadExecute (B.concat (map word32le (H.callback 0x633594)))
        , region (RegionId "header-log") H.helperBase (4 * length H.helperWords) ReadExecute (B.concat (map word32le H.helperWords))
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
    , caseIgnoredMemory = []
    , caseCp14Identity = Nothing
    , caseKernel = Nothing
    }
  where
    hex value = "0x" ++ showHex value ""
    (entry, entryName, arguments) = case operation of
      Get bank mode (SlotArgument slot) -> (0x61e110 :: Word32, "getter 0x61e110", [identifier, bankWord bank, readModeValue mode, slot])
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
  DefaultFlipped -> case fieldSource arm of
    FieldDefaultOnly (DefaultConstant value) -> Just arm {fieldSource = FieldDefaultOnly (DefaultConstant (value `xor` 1))}
    _ -> Nothing
  TailOffsetMoved -> case fieldSource arm of
    FieldDefaultOnly (DefaultTail (TailOffset tail') width) -> Just arm {fieldSource = FieldDefaultOnly (DefaultTail (TailOffset (tail' + 1)) width)}
    _ -> Nothing

entryControlOperations :: EntryControl -> [Operation]
entryControlOperations control = filter relevant operations
  where
    relevant operation = case (control, operation) of
      (OffsetMoved, Get _ mode _) -> mode /= DefaultRead
      (OffsetMoved, Set _ _ _) -> True
      (SaveCleared, Set bank _ _) -> bank == Bank0
      (SaveCleared, Get _ _ _) -> False
      (DefaultFlipped, Get _ mode _) -> mode == DefaultRead
      (DefaultFlipped, Set _ _ choice) -> choice == SetDefaultRequest
      (TailOffsetMoved, Get _ mode _) -> mode == DefaultRead
      (TailOffsetMoved, Set _ _ choice) -> choice == SetDefaultRequest

-- | The first field the property word holds whose mutated entry changes the model on one of the
-- control's operations.
exposingField :: Fixture -> EntryControl -> [Property] -> Maybe (Property, FieldArm)
exposingField fx control properties = listToMaybe
  [ (property, mutated)
  | property@(Property _ _ _ (Owned (OwnedField arm@(FieldArm _ _ _ (FieldDefaultOnly _) _ NoHook)))) <- properties
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
mutateSlot fx control arm@(SlotArm bank@(BankBranch _ bankWidth' bankBits' fallback) record@(RecordBranch (RecordOffset offset) width bits) save _ _) = case control of
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

-- | The first stage-2 slot field whose mutation changes the model, both inside the fixture, on one
-- of the control's operations.
exposingSlot :: Fixture -> SlotControl -> [Property] -> Maybe (Property, SlotArm, SlotMutation)
exposingSlot fx control properties = listToMaybe
  [ (property, arm, mutation)
  | property@(Property _ _ _ (Owned (OwnedSlotField arm@(SlotArm _ _ _ SlotDefaultsRecord NoMirror)))) <- properties
  , Just mutation <- [mutateSlot fx control arm]
  , or [before /= after | operation <- slotControlOperations control, Just before <- [modelEffect fx (OwnedSlotField arm) operation], Just after <- [mutated mutation arm operation]]
  ]
  where
    mutated mutation arm operation = case mutation of
      MutatedArm changed -> modelEffect fx (OwnedSlotField changed) operation
      MutatedLayout layout -> modelEffectIn layout fx (OwnedSlotField arm) operation

-- | The getter's default reads and the setter's default requests: 10 each.
defaultOperations :: [Operation]
defaultOperations = [o | o@(Get _ DefaultRead _) <- operations] ++ [o | o@(Set _ _ SetDefaultRequest) <- operations]

-- | The operations a property runs under each configuration variant: the default operations of a
-- stock or configuration default, every set of a gated field.
variantOperations :: Owned -> [Operation]
variantOperations owned = case owned of
  OwnedField (FieldArm _ _ _ _ _ (ConfigGatedResave _)) -> [o | o@(Set {}) <- operations]
  OwnedField (FieldArm _ _ _ (FieldStock _ _) _ _) -> defaultOperations
  OwnedField (FieldArm _ _ _ (FieldConfig _ _) _ _) -> defaultOperations
  OwnedSlotField (SlotArm _ _ _ (SlotStock _) _) -> defaultOperations
  _ -> []

allStockDefaults :: [StockDefault]
allStockDefaults = [Stock61df44, Stock61d8a4, Stock61d59c, Stock61d618, Stock61d5f0] ++ map Stock61d65c [minBound .. maxBound]

-- | The arm with its stock default replaced, when it has one.
withStock :: StockDefault -> Owned -> Maybe Owned
withStock stock owned = case owned of
  OwnedField arm@(FieldArm _ _ _ (FieldStock _ provisional) _ _) -> Just (OwnedField arm {fieldSource = FieldStock stock provisional})
  OwnedSlotField arm@(SlotArm _ _ _ (SlotStock _) _) -> Just (OwnedSlotField arm {slotDefault = SlotStock stock})
  _ -> Nothing

stockOf :: Owned -> Maybe StockDefault
stockOf owned = case owned of
  OwnedField (FieldArm _ _ _ (FieldStock stock _) _ _) -> Just stock
  OwnedSlotField (SlotArm _ _ _ (SlotStock stock) _) -> Just stock
  _ -> Nothing

-- | The first property with a stock default and the first other leaf whose result changes the
-- model on one of its default operations under the stream or a configuration variant.
exposingStockSwap :: Fixture -> [Property] -> Maybe (Property, Owned, Owned)
exposingStockSwap fx properties = listToMaybe
  [ (property, owned, swapped)
  | property@(Property _ _ _ (Owned owned)) <- properties
  , Just current <- [stockOf owned]
  , other <- allStockDefaults, other /= current
  , Just swapped <- [withStock other owned]
  , or [modelEffect variantFx owned operation /= modelEffect variantFx swapped operation | variant <- CanonicalFixture : configVariantsOf, let variantFx = variantFixture fx variant, operation <- defaultOperations]
  ]

-- | Whether the setter stub brackets the property's bank-0 sets: a bracketed field or a write-only pair.
bracketedOwned :: Owned -> Bool
bracketedOwned owned = case owned of
  OwnedField arm -> fieldHook arm == Bank0Bracket
  OwnedWriteOnlyPair _ -> True
  _ -> False

-- | The first bracketed property of the class `chosen` accepts whose model on a bank-0 set changes
-- when the open call follows its first save and store.
exposingOpenAfterSet :: Fixture -> (Owned -> Bool) -> [Property] -> Maybe Property
exposingOpenAfterSet fx chosen properties = listToMaybe
  [ property
  | property@(Property _ _ _ (Owned owned)) <- properties
  , bracketedOwned owned, chosen owned
  , or [modelEffect fx owned operation /= modelEffectIn stockLayout {layoutOpen = OpenAfterSet} fx owned operation | operation@(Set Bank0 _ _) <- operations]
  ]

-- | A32 @bl target@ at @site@.
branchLinkWord :: Word32 -> Word32 -> Word32
branchLinkWord site target = 0xeb000000 .|. (((target - site - 8) `shiftR` 2) .&. 0xffffff)

-- | A32 @b target@ at @site@.
branchWord :: Word32 -> Word32 -> Word32
branchWord site target = 0xea000000 .|. (((target - site - 8) `shiftR` 2) .&. 0xffffff)

-- | A32 @movw r12, #low@ and @movt r12, #high@ for the low and high halves of an address.
movwWord, movtWord :: Word32 -> Word32
movwWord address = 0xe300c000 .|. ((address `shiftR` 12 .&. 0xf) `shiftL` 16) .|. (address .&. 0xfff)
movtWord address = 0xe340c000 .|. ((address `shiftR` 28) `shiftL` 16) .|. ((address `shiftR` 16) .&. 0xfff)

