{-# LANGUAGE OverloadedStrings #-}
-- | What the property-service natural pair arms and watches, derived from the bound images,
-- linked artifact and staging contract of one role.
module Emulation.PropertyPairPlan
  ( Role (..)
  , roleWire
  , parseRole
  , ImageName (..)
  , imageNameWire
  , codeImages
  , image4Base
  , analysisPath
  , PropertyClass (..)
  , ownedClass
  , PropertyArms (..)
  , PatchWord (..)
  , Layout (..)
  , Hull (..)
  , wrapperIsland
  , ScanImage (..)
  , Contract (..)
  , Publication (..)
  , PublishedElfIdentity
  , publishedElfSha256
  , decodeContract
  , controlPatch
  , controlWord
  , ControlWord (..)
  , setterEntryControl
  , decodeControlManifest
  , controlManifestValue
  , SiteLabel (..)
  , siteLabels
  , siteLabelWire
  , parseSiteLabel
  , StoreAccessor (..)
  , storeAccessorName
  , storeRange
  , storeWidth
  , storeEffect
  , persistenceStatusExecutes
  , GuardKind (..)
  , WriterClass (..)
  , PhysicalRange (..)
  , Plan (..)
  , PlanInputs (..)
  , derivePlan
  , deriveStoragePlan
  , roleImage4
  , classifyWriter
  , getterEntry
  , setterEntry
  , area3Append
  , propertyCommit
  , settingsRanges
  , recordSettingsRanges
  , specValue
  , hitLimit
  , watchLimit
  , preKeyIdle
  , postSettle
  , Tick (..)
  , maxIdle
  , Instructions (..)
  , word32At
  ) where

import Control.Monad (forM, forM_, unless, when)
import Data.Aeson (FromJSON (..), Value, object, withObject, (.:), (.=))
import qualified Data.Aeson as A
import qualified Data.Aeson.Key as K
import qualified Data.Aeson.KeyMap as KM
import Data.Aeson.Types (Parser)
import Data.Bits (complement, shiftL, shiftR, testBit, (.&.), (.|.))
import qualified Data.ByteString as B
import Data.Int (Int32)
import Data.List (find)
import qualified Data.Map.Strict as M
import qualified Data.Set as S
import qualified Data.Text as T
import Data.Word (Word32)
import qualified Replacement.PropertyService.Artifact as StorageEncoding
import CustomAdmission (CustomStoreWidth(..), customStoreName, customStoreWords, customStoreIndex)
import qualified CustomAdmission as StorageBoundaries
import qualified Replacement.PropertyService.Placement as Placement
import Emulation.Sha256 (Sha256, sha256, sha256Hex, parseSha256Hex)
import Emulation.WatchObservation (Instructions (..), Tick (..))
import Emulation.SettingsStorageMatrix
import Numeric (readHex, showHex)
import System.FilePath ((</>))

data Role = RetainedRole | OwnedRole | ControlRole deriving (Eq, Ord, Show, Enum, Bounded)

roleWire :: Role -> String
roleWire role = case role of
  RetainedRole -> "retained"
  OwnedRole -> "owned"
  ControlRole -> "control"

parseRole :: String -> Either String Role
parseRole text = maybe (Left ("unknown role " ++ text)) Right (lookup text [(roleWire role, role) | role <- [minBound .. maxBound]])

data ImageName = Image4 | Image5 | Plain | LoaderA | LoaderB deriving (Eq, Ord, Show, Enum, Bounded)

imageNameWire :: ImageName -> T.Text
imageNameWire name = case name of
  Image4 -> "image4"
  Image5 -> "image5"
  Plain -> "plain"
  LoaderA -> "loader_a"
  LoaderB -> "loader_b"

codeImages :: [ImageName]
codeImages = [minBound .. maxBound]

-- | The decoded canonical image under the X-M1 1.50 analysis root.
analysisPath :: FilePath -> ImageName -> FilePath
analysisPath root name = root </> case name of
  Image4 -> "lz/id4-0061c400.bin"
  Image5 -> "lz/id5-00bf1a00.bin"
  Plain -> "lz/plain-01100000.bin"
  LoaderA -> "decode/loader-40000.bin"
  LoaderB -> "decode/loader-d0000.bin"

data PropertyClass = Delegated | OwnedField | OwnedSlotField | OwnedReadOnlyField | OwnedWriteOnlyPair | OwnedUnassigned deriving (Eq, Show)

-- | The classes whose arms the owned roles retire: every owned class but the unassigned, whose arms
-- are the stock fallbacks.
ownedClass :: PropertyClass -> Bool
ownedClass kind = kind `notElem` [Delegated, OwnedUnassigned]

data PropertyArms = PropertyArms
  { armsProperty :: Word32
  , armsClass :: PropertyClass
  , armsGetter :: Word32
  , armsSetter :: Word32
  } deriving (Eq, Show)

data PatchWord = PatchWord
  { patchAddress :: Word32
  , patchOriginal :: Word32
  , patchPatched :: Word32
  , patchProperty :: Word32
  , patchSetterTable :: Bool
  } deriving (Eq, Show)

data ScanImage = ScanImage { scanName :: T.Text, scanBase :: Word32, scanBytes :: Int, scanSha256 :: String } deriving (Eq, Show)

-- | How the staged image4 differs from the canonical one.
data Layout
  -- | Schema 1: the carrier and repointed getter and setter table words.
  = TablePatched [PatchWord]
  -- | Schema 2: the whole getter and setter hull is owned.
  | HullOwned Hull
  deriving (Eq, Show)

-- | Every hull word is the fill word except the two entry adapters, the carrier and the retained
-- regions, which keep their canonical bytes.
data Hull = Hull
  { hullStart :: Word32
  , hullEnd :: Word32
  , hullAdapters :: [(Word32, Word32)]
  , hullRetained :: [PhysicalRange]
  } deriving (Eq, Show)

-- | The three stock wrappers between the getter and the setter, live code a schema-2 hull retains.
wrapperIsland :: PhysicalRange
wrapperIsland = PhysicalRange 0x6245f8 0x624694

-- | The fields of the property-service staging contract the pair reads.
data Contract = Contract
  { contractCandidateDat :: String
  , contractSourceDat :: String
  , contractLinkedSha256 :: String
  , contractLinkedBytes :: Int
  , contractSymbolsSha256 :: String
  , contractStagedImage4 :: String
  , contractCarrierStart :: Word32
  , contractCarrierEnd :: Word32
  , contractFillWord :: Word32
  , contractLayout :: Layout
  , contractProperties :: [PropertyArms]
  , contractImages :: [ScanImage]
  -- | Each linked function's symbol name and entry address.
  , contractEntries :: [(T.Text, Word32)]
  , contractPublication :: Maybe Publication
  , contractPublishedElf :: Maybe PublishedElfIdentity
  } deriving (Eq, Show)

newtype PublishedElfIdentity = PublishedElfIdentity Sha256 deriving (Eq)
instance Show PublishedElfIdentity where
  showsPrec precedence identity = showParen (precedence > 10) (showString "PublishedElfIdentity " . shows (publishedElfSha256 identity))
publishedElfSha256 :: PublishedElfIdentity -> String
publishedElfSha256 (PublishedElfIdentity digest) = sha256Hex digest

data Publication = Publication
  { publicationCarriers :: [PhysicalRange]
  , publicationRetired :: [PhysicalRange]
  , publicationAdapters :: [(Word32, Word32)]
  , publicationPropertyCodeBytes :: Int
  } deriving (Eq, Show)
instance FromJSON Publication where
  parseJSON = withObject "settings publication" $ \fields -> Publication
    <$> (fields .: "carriers" >>= mapM region)
    <*> (fields .: "retired_regions" >>= mapM region)
    <*> (fields .: "adapters" >>= mapM (withObject "published adapter" $ \entry -> (,) <$> entry .: "address" <*> entry .: "word"))
    <*> pure 0
    where region = withObject "published region" $ \entry -> PhysicalRange <$> entry .: "start" <*> entry .: "end"

hexAddress :: T.Text -> Parser Word32
hexAddress text = case T.unpack text of
  '0' : 'x' : digits | [(value, "")] <- readHex digits, value <= (0xffffffff :: Integer) -> pure (fromIntegral value)
  _ -> fail ("not a hexadecimal word: " ++ T.unpack text)

instance FromJSON Contract where
  parseJSON value = withObject "property-service contract" (\fields -> do
    schema <- fields .: "schema"
    if (schema :: Int) /= 3 then parsePropertyContract value else do
      original <- fields .: "property_contract" >>= parsePropertyContract
      case contractLayout original of
        HullOwned _ -> pure ()
        _ -> fail "composed settings publication requires the stage4 owned hull"
      let propertyNames = map fst (contractEntries original)
      unless (all (`elem` propertyNames) ["xm1_storage_store8","xm1_storage_store16"]
        && all (`notElem` propertyNames) ["xm1_bank_store8","xm1_bank_store16","xm1_save_store8","xm1_save_store16"])
        (fail "composed property contract still uses raw storage shims")
      publication <- fields .: "publication"
      candidate <- fields .: "candidate_dat_sha256"
      linkedHash <- fields .: "linked_sha256"
      linkedSize <- fields .: "linked_bytes"
      symbolsHash <- fields .: "symbols_sha256"
      elfText <- fields .: "elf_sha256"
      elfDigest <- either fail (pure . PublishedElfIdentity) (parseSha256Hex elfText)
      imageHash <- fields .: "staged_image4_sha256"
      stack <- fields .: "stack"
      entries <- stack .: "entries" >>= mapM (withObject "published entry" $ \entry -> (,) <$> entry .: "name" <*> entry .: "entry")
      pure original { contractCandidateDat = candidate, contractLinkedSha256 = linkedHash, contractLinkedBytes = linkedSize
        , contractSymbolsSha256 = symbolsHash, contractStagedImage4 = imageHash, contractEntries = entries
        , contractPublication = Just (publication { publicationPropertyCodeBytes = contractLinkedBytes original }), contractPublishedElf = Just elfDigest }) value

parsePropertyContract :: Value -> Parser Contract
parsePropertyContract = withObject "property-service contract" $ \fields -> do
    schema <- fields .: "schema"
    carrier <- fields .: "carrier"
    classification <- fields .: "classification"
    sources <- fields .: "source_sha256"
    scan <- fields .: "scan"
    stack <- fields .: "stack"
    entries <- stack .: "entries" >>= mapM (withObject "stack entry" $ \entry -> (,) <$> entry .: "name" <*> entry .: "entry")
    layout <- case schema :: Int of
      1 -> do
        patch <- fields .: "patch"
        fmap TablePatched $ patch .: "words" >>= mapM (withObject "patch word" $ \word -> do
          table <- word .: "table"
          setter <- case table :: T.Text of
            "getter" -> pure False
            "setter" -> pure True
            _ -> fail "patch word table is neither getter nor setter"
          PatchWord <$> word .: "address" <*> word .: "original" <*> word .: "patched" <*> word .: "property" <*> pure setter)
      2 -> do
        hull <- fields .: "hull"
        adapters <- fields .: "adapters" >>= mapM (withObject "adapter" $ \adapter -> (,) <$> adapter .: "address" <*> adapter .: "word")
        retained <- fields .: "retained_regions" >>= mapM (withObject "retained region" $ \region -> PhysicalRange <$> region .: "start" <*> region .: "end")
        HullOwned <$> (Hull <$> hull .: "start" <*> hull .: "end" <*> pure adapters <*> pure retained)
      _ -> fail "unsupported property-service contract schema"
    properties <- classification .: "properties" >>= mapM (withObject "property class" $ \property -> do
      kind <- property .: "class"
      decided <- case kind :: T.Text of
        "delegated" -> pure Delegated
        "owned-field" -> pure OwnedField
        "owned-slot-field" -> pure OwnedSlotField
        "owned-read-only-field" -> pure OwnedReadOnlyField
        "owned-write-only-pair" -> pure OwnedWriteOnlyPair
        "owned-unassigned" -> pure OwnedUnassigned
        _ -> fail "unknown property class"
      PropertyArms <$> (property .: "property" >>= hexAddress) <*> pure decided
        <*> (property .: "getterArm" >>= hexAddress) <*> (property .: "setterArm" >>= hexAddress))
    images <- scan .: "images" >>= mapM (withObject "scan image" $ \image ->
      ScanImage <$> image .: "image" <*> image .: "base" <*> image .: "bytes" <*> image .: "sha256")
    Contract <$> fields .: "candidate_dat_sha256" <*> sources .: "dat" <*> fields .: "linked_sha256"
      <*> fields .: "linked_bytes" <*> fields .: "symbols_sha256" <*> fields .: "staged_image4_sha256"
      <*> carrier .: "start" <*> carrier .: "end" <*> carrier .: "fill_word" <*> pure layout <*> pure properties <*> pure images <*> pure entries <*> pure Nothing <*> pure Nothing

decodeContract :: B.ByteString -> Either String Contract
decodeContract = A.eitherDecodeStrict'

-- | The guest control's patched words: each `(address, word)` replaces the staged word, so the
-- owned role fetches the guarded `controlTarget`, entered with r0 `controlProperty` when the control
-- names one property.
data ControlWord = ControlWord
  { controlProperty :: Maybe Word32
  , controlPatches :: [(Word32, Word32)]
  , controlTarget :: Word32
  } deriving (Eq, Show)

-- | The staged setter table word of the owned property a schema-1 guest control restores.
controlPatch :: Contract -> Word32 -> Either String PatchWord
controlPatch contract property = do
  words' <- case contractLayout contract of
    TablePatched words' -> Right words'
    HullOwned _ -> Left "a schema-2 contract has no table words; its control restores the setter entry"
  arms <- maybe (Left "PROPERTY is not classified") Right (find ((== property) . armsProperty) (contractProperties contract))
  unless (ownedClass (armsClass arms)) (Left "PROPERTY is not of an owned class with its own arms")
  unless (armsSetter arms < contractCarrierStart contract || armsSetter arms >= contractCarrierEnd contract)
    (Left "PROPERTY's stock setter arm lies in the carrier")
  maybe (Left "PROPERTY has no staged setter table word") Right
    (find (\word -> patchSetterTable word && patchProperty word == property) words')

controlWord :: PatchWord -> ControlWord
controlWord patch = ControlWord (Just (patchProperty patch)) [(patchAddress patch, patchOriginal patch)] (patchOriginal patch)

-- | A schema-2 guest control: the setter entry adapter becomes `mov r0, r0` and the next, retired
-- word the adapter's branch, so every set fetches that guarded word once and then runs as owned.
setterEntryControl :: Contract -> Either String ControlWord
setterEntryControl contract = case contractLayout contract of
  TablePatched _ -> Left "a schema-1 contract's control restores one property's setter table word"
  HullOwned hull -> do
    adapter <- maybe (Left "the contract has no setter entry adapter") Right (lookup setterEntry (hullAdapters hull))
    unless (adapter .&. 0xff000000 == 0xea000000) (Left "the setter entry adapter is not an unconditional B")
    let detour = setterEntry + 4
        displacement = adapter .&. 0xffffff
        target = setterEntry + 8 + 4 * (if testBit displacement 23 then displacement .|. 0xff000000 else displacement)
    Right (ControlWord Nothing [(setterEntry, 0xe1a00000), (detour, 0xea000000 .|. (((target - (detour + 8)) `shiftR` 2) .&. 0xffffff))] detour)

-- | The control builder's manifest.
controlManifestValue :: ControlWord -> String -> String -> String -> String -> Value
controlManifestValue control dat image4 ownedDat stagedImage4 = object
  [ "schema" .= (2 :: Int)
  , "property" .= fmap hexText (controlProperty control)
  , "words" .= [object ["address" .= hexText address, "word" .= hexText word] | (address, word) <- controlPatches control]
  , "target" .= hexText (controlTarget control)
  , "dat_sha256" .= dat
  , "image4_sha256" .= image4
  , "owned_dat_sha256" .= ownedDat
  , "staged_image4_sha256" .= stagedImage4
  ]
 where
  hexText value = "0x" ++ replicate (8 - length (showHex value "")) '0' ++ showHex value ""

-- | The control and the control's DAT and image4 digests. A schema-1 manifest restores its target
-- word at its address.
decodeControlManifest :: B.ByteString -> Either String (ControlWord, String, String)
decodeControlManifest bytes = do
  value <- A.eitherDecodeStrict' bytes
  let fieldIn name (A.Object fields) = KM.lookup (K.fromText name) fields
      fieldIn _ _ = Nothing
      textIn object' name = case fieldIn name object' of
        Just (A.String content) -> Right (T.unpack content)
        _ -> Left ("control manifest lacks " ++ T.unpack name)
      wordIn object' name = textIn object' name >>= \content -> case content of
        '0' : 'x' : digits | [(parsed, "")] <- readHex digits, parsed <= (0xffffffff :: Integer) -> Right (fromIntegral parsed)
        _ -> Left ("control manifest " ++ T.unpack name ++ " is not a hexadecimal word")
      word = wordIn value
  control <- case fieldIn "schema" value of
    Just (A.Number 1) -> (\property address target -> ControlWord (Just property) [(address, target)] target)
      <$> word "property" <*> word "address" <*> word "target"
    Just (A.Number 2) -> do
      property <- case fieldIn "property" value of
        Just A.Null -> Right Nothing
        _ -> Just <$> word "property"
      patches <- case fieldIn "words" value of
        Just (A.Array entries) -> mapM (\entry -> (,) <$> wordIn entry "address" <*> wordIn entry "word") (foldr (:) [] entries)
        _ -> Left "control manifest lacks words"
      ControlWord property patches <$> word "target"
    _ -> Left "control manifest schema differs"
  (,,) control <$> textIn value "dat_sha256" <*> textIn value "image4_sha256"

data GuardKind = RetiredGetterArm | RetiredSetterArm | CarrierFill | RetiredStorage deriving (Eq, Ord, Show, Enum, Bounded)
data SiteLabel = GetterEntry | SetterEntry | Area3Append | PropertyCommit | GetterReturn | SetterReturn
  | RecordRead | RecordProgram | RecordProgramResult | RecordErase | RecordEraseResult
  | AccessorStore StoreAccessor | Guard GuardKind
  | StorageSite LifecycleSite
  deriving (Eq, Ord, Show)

siteLabels :: [SiteLabel]
siteLabels = [GetterEntry, SetterEntry, Area3Append, PropertyCommit, GetterReturn, SetterReturn, RecordRead, RecordProgram, RecordProgramResult, RecordErase, RecordEraseResult]
  ++ map AccessorStore [minBound .. maxBound] ++ map Guard [minBound .. maxBound] ++ map StorageSite [minBound .. maxBound]

siteLabelWire :: SiteLabel -> String
siteLabelWire label = case label of
  GetterEntry -> "pp-getter-entry"
  SetterEntry -> "pp-setter-entry"
  Area3Append -> "pp-area3-append"
  PropertyCommit -> "pp-property-commit"
  GetterReturn -> "pp-getter-return"
  SetterReturn -> "pp-setter-return"
  AccessorStore accessor -> "pp-store-" ++ T.unpack (storeAccessorName accessor)
  Guard RetiredGetterArm -> "pp-guard-retired-getter-arm"
  Guard RetiredSetterArm -> "pp-guard-retired-setter-arm"
  Guard CarrierFill -> "pp-guard-carrier-fill"
  Guard RetiredStorage -> "pp-guard-retired-storage"
  RecordRead -> "pp-record-read"
  RecordProgram -> "pp-record-program"
  RecordProgramResult -> "pp-record-program-result"
  RecordErase -> "pp-record-erase"
  RecordEraseResult -> "pp-record-erase-result"
  StorageSite site -> lifecycleSiteWire site

parseSiteLabel :: String -> Maybe SiteLabel
parseSiteLabel text = lookup text [(siteLabelWire label, label) | label <- siteLabels]

data StoreAccessor = BankStore8 | BankStore16 | SaveStore8 | SaveStore16
  | StorageByte | StorageHalf | CustomByte | CustomHalf | CustomWord | OutputByte | OutputHalf | StatusWord deriving (Eq, Ord, Show, Enum, Bounded)

storeAccessorName :: StoreAccessor -> T.Text
storeAccessorName accessor = case accessor of
  BankStore8 -> "xm1_bank_store8"
  BankStore16 -> "xm1_bank_store16"
  SaveStore8 -> "xm1_save_store8"
  SaveStore16 -> "xm1_save_store16"
  StorageByte -> "xm1_storage_store8"
  StorageHalf -> "xm1_storage_store16"
  CustomByte -> T.pack (customStoreName CustomStoreByte)
  CustomHalf -> T.pack (customStoreName CustomStoreHalf)
  CustomWord -> T.pack (customStoreName CustomStoreWord)
  OutputByte -> "xm1_storage_output8"
  OutputHalf -> "xm1_storage_output16"
  StatusWord -> "xm1_storage_status32"

storeWidth :: StoreAccessor -> Int
storeWidth accessor = case accessor of
  BankStore16 -> 2
  SaveStore16 -> 2
  StorageHalf -> 2
  OutputHalf -> 2
  CustomHalf -> 2
  CustomWord -> 4
  StatusWord -> 4
  _ -> 1

-- | The settings range r12 addresses.
storeRange :: StoreAccessor -> PhysicalRange
storeRange accessor = if accessor `elem` [BankStore8, BankStore16] then PhysicalRange 0x96cc64 0x96cfe8 else PhysicalRange 0x89ec00 0x89edc2

-- | `strb r1, [r12, r0]` or `strh r1, [r12, r0]`.
storeInstruction :: StoreAccessor -> Word32
storeInstruction accessor = if storeWidth accessor == 1 then 0xe7cc1000 else 0xe18c10b0

storeEffect :: StoreAccessor -> [Word32] -> Maybe (Word32, Int, Word32)
storeEffect accessor arguments = case (accessor, arguments) of
  (StatusWord, value:_) -> Just (0x89edc4, 4, value)
  (OutputByte, base:count:index:value:_) | count <= 0x19a, toInteger base + toInteger count <= 0xffffffff, index < count -> Just (base+index, 1, value)
  (OutputHalf, base:count:index:value:_) | count <= 0x19a, toInteger base + toInteger count <= 0xffffffff, toInteger index + 2 <= toInteger count, even (base+index) -> Just (base+index, 2, value)
  (StorageByte, address:value:_) -> absolute address value shared
  (StorageHalf, address:value:_) -> absolute address value shared
  (CustomByte, address:value:_) -> absolute address value [PhysicalRange 0x89f834 0x89f870, PhysicalRange 0x89f874 0x89f8a8, PhysicalRange 0x409c295c 0x409c2984]
  (CustomHalf, address:value:_) | even address -> absolute address value [PhysicalRange 0x89f838 0x89f86c, PhysicalRange 0x89f874 0x89f8a8, PhysicalRange 0x409c295c 0x409c2984]
  (CustomWord, address:value:_) | address .&. 3 == 0 -> absolute address value [PhysicalRange 0x89f834 0x89f870]
  (_, offset:value:_) | accessor `elem` [BankStore8,BankStore16,SaveStore8,SaveStore16] ->
    let range = storeRange accessor in if toInteger offset + toInteger width <= toInteger (rangeEnd range-rangeStart range) then Just (rangeStart range+offset,width,value) else Nothing
  _ -> Nothing
  where
    width = storeWidth accessor
    shared = [PhysicalRange 0x96cc64 0x96d1aa, PhysicalRange 0x89ec00 0x89ede0, PhysicalRange 0x409c281c 0x409c29b6, PhysicalRange 0x409c29b8 0x409c29c8]
    absolute address value ranges = if any (\range -> address >= rangeStart range && toInteger address + toInteger width <= toInteger (rangeEnd range)) ranges then Just (address,width,value) else Nothing

-- | `CarrierBody` is any carrier word other than an accessor's store instruction.
data WriterClass = StockCode | OwnedAccessor | CarrierBody | OtherWriter deriving (Eq, Ord, Show)

-- | A half-open physical range.
data PhysicalRange = PhysicalRange { rangeStart :: Word32, rangeEnd :: Word32 } deriving (Eq, Ord, Show)

getterEntry, setterEntry, area3Append, propertyCommit :: Word32
getterEntry = 0x61e110
setterEntry = 0x624694
area3Append = 0x6ac7d4
propertyCommit = 0xd4fc74

-- | The settings-storage lifecycle's settings boundary, including its custom record areas.
settingsRanges :: [PhysicalRange]
settingsRanges = [PhysicalRange 0x89ec00 0x89ede0, PhysicalRange 0x96cc64 0x96d1aa, PhysicalRange 0x9c281c (0x9c281c + 1144), PhysicalRange 0x89f834 0x89f870, PhysicalRange 0x89f874 0x89f8a8]

recordSettingsRanges :: [PhysicalRange]
recordSettingsRanges = [PhysicalRange 0x89ec00 0x89edc2, PhysicalRange 0x96cc64 0x96cfe8, PhysicalRange 0x9c281c (0x9c281c + 1144)]

hitLimit, watchLimit :: Int
hitLimit = 1500000
watchLimit = 262144

-- | Guest instructions run with no input before every press, ahead of the bridge's render-drain gate.
preKeyIdle :: Instructions
preKeyIdle = Instructions 20000000

-- | Board ticks without input after each press, bounding the compared effect prefix.
postSettle :: Tick
postSettle = Tick 833334

-- | The bridge's largest `idle` request, its MAX_SETTLE.
maxIdle :: Instructions
maxIdle = Instructions 90000000

data Plan = Plan
  { planRole :: Role
  , planSites :: M.Map Word32 SiteLabel
  , planStock :: [PhysicalRange]
  -- | The owned roles' store instruction of each accessor, at its physical address.
  , planStores :: M.Map Word32 StoreAccessor
  , planCarriers :: [PhysicalRange]
  , planReads :: M.Map Word32 [(Word32,Int)]
  , planBankBoundary :: Maybe StorageBoundaries.BankApplyBoundary
  } deriving (Eq, Show)

-- | The observer's `--reset-observation` document for the plan.
specValue :: Plan -> Value
specValue plan = object
  ([ "schema" .= (1 :: Int)
  , "sites" .= object [K.fromString (hexWord address) .= siteLabelWire label | (address, label) <- M.toList (planSites plan)]
  , "hit_limit" .= hitLimit
  , "write_ranges" .= [[rangeStart range, rangeEnd range] | range <- settingsRanges]
  , "watch_limit" .= watchLimit
  ] ++ ["reads" .= object [K.fromString (hexWord address) .= [[toInteger start,toInteger count] | (start,count) <- ranges] | (address,ranges) <- M.toList (planReads plan)] | not (M.null (planReads plan))]
    ++ ["registers" .= ("general"::T.Text) | any (\site -> StorageSite site `elem` M.elems (planSites plan)) [EeCompare,S1ActionEntry]]
    ++ ["task" .= ("logical"::T.Text) | StorageSite BankApplyEntry `elem` M.elems (planSites plan)])
 where
  hexWord value = "0x" ++ replicate (8 - length (showHex value "")) '0' ++ showHex value ""

recordResultSites :: [(Word32,SiteLabel,Word32)]
recordResultSites = [(0x6ab314,RecordProgramResult,0x18bd8800),(0x6ab328,RecordProgramResult,0x18bd8800)
  ,(0x6ab348,RecordProgramResult,0x88bd8800),(0x6ab390,RecordProgramResult,0xe8bd8800),(0x6ab3ac,RecordProgramResult,0xe8bd8800)
  ,(0x6ab474,RecordEraseResult,0x18bd8800),(0x6ab4c0,RecordEraseResult,0xe8bd8800),(0x6ab4c8,RecordEraseResult,0xe8bd8800)]
persistenceStatusExecutes :: Word32 -> Word32 -> Bool
persistenceStatusExecutes address flags = case [instruction `shiftR` 28 | (site,_,instruction) <- recordResultSites, site == address .&. complement 0x40000000] of
  [1] -> not (testBit flags 30)
  [8] -> testBit flags 29 && not (testBit flags 30)
  [14] -> True
  _ -> False

data PlanInputs = PlanInputs
  { inputRole :: Role
  , inputContract :: Contract
  , inputImages :: M.Map ImageName B.ByteString
  , inputLinked :: B.ByteString
  , inputControl :: Maybe ControlWord
  , inputBankBoundary :: Maybe StorageBoundaries.BankApplyBoundary
  }

word32At :: B.ByteString -> Int -> Word32
word32At bytes offset = foldr (\index acc -> (acc `shiftL` 8) .|. fromIntegral (B.index bytes (offset + index))) 0 [0 .. 3]

putWord32 :: Int -> Word32 -> B.ByteString -> B.ByteString
putWord32 offset value bytes = B.take offset bytes <> B.pack [fromIntegral (value `shiftR` (8 * index)) | index <- [0 .. 3]] <> B.drop (offset + 4) bytes

image4Base :: Word32
image4Base = 0x61c400

-- | The decoded image4 the role's DAT carries: the canonical image, or the staged image with the
-- control's word restored.
roleImage4 :: PlanInputs -> Either String B.ByteString
roleImage4 inputs = do
  let contract = inputContract inputs
  canonical <- maybe (Left "image4 is not bound") Right (M.lookup Image4 (inputImages inputs))
  let offset address = fromIntegral (address - image4Base)
      fill = B.pack [fromIntegral (contractFillWord contract `shiftR` (8 * index)) | index <- [0 .. 3 :: Int]]
      carrierWords = (fromIntegral (contractCarrierEnd contract - contractCarrierStart contract) - B.length (inputLinked inputs)) `div` 4
      carrier = inputLinked inputs <> B.concat (replicate carrierWords fill)
      withCarrier base = B.take (offset (contractCarrierStart contract)) base <> carrier <> B.drop (offset (contractCarrierEnd contract)) base
      inRange range address = rangeStart range <= address && address < rangeEnd range
      propertyStaged = case contractLayout contract of
        TablePatched words' -> foldl' (\bytes patch -> putWord32 (offset (patchAddress patch)) (patchPatched patch) bytes) (withCarrier canonical) words'
        HullOwned hull ->
          let hullWord address = if any (`inRange` address) (hullRetained hull) then B.take 4 (B.drop (offset address) canonical) else fill
              filled = B.take (offset (hullStart hull)) canonical
                <> B.concat (map hullWord [hullStart hull, hullStart hull + 4 .. hullEnd hull - 4])
                <> B.drop (offset (hullEnd hull)) canonical
          in foldl' (\bytes (address, word) -> putWord32 (offset address) word bytes) (withCarrier filled) (hullAdapters hull)
      staged = case contractPublication contract of
        Nothing -> propertyStaged
        Just _ -> inputLinked inputs
  unless (B.length (inputLinked inputs) == contractLinkedBytes contract && (B.length (inputLinked inputs) `mod` 4) == 0)
    (Left "linked artifact length differs from its contract")
  unless (B.length staged == B.length canonical) (Left "staged image4 changes its length")
  unless (sha256Hex (sha256 staged) == contractStagedImage4 contract) (Left "rebuilt staged image4 differs from its contract digest")
  forM_ (contractPublication contract) $ \publication -> do
    let propertyCarrier = PhysicalRange (contractCarrierStart contract) (contractCarrierEnd contract)
        carriers = publicationCarriers publication
        adapters = publicationAdapters publication
        expectedEntries = map fst (StorageBoundaries.bankEntries ++ StorageBoundaries.customEntries ++ StorageBoundaries.recordEntries)
        validRegion region = rangeStart region >= image4Base && rangeEnd region > rangeStart region && rangeStart region `mod` 4 == 0 && rangeEnd region `mod` 4 == 0
          && toInteger (rangeEnd region) <= toInteger image4Base + toInteger (B.length staged)
        overlaps a b = rangeStart a < rangeEnd b && rangeStart b < rangeEnd a
    unless (not (null carriers) && all validRegion (carriers ++ publicationRetired publication)) (Left "invalid composed publication extents")
    unless (all (not . overlaps propertyCarrier) carriers && and [not (overlaps a b) | (index,a) <- zip [0::Int ..] carriers, b <- drop (index+1) carriers]) (Left "composed carriers overlap")
    unless (S.fromList (map fst adapters) == S.fromList expectedEntries && length adapters == length expectedEntries) (Left "composed publication does not pin every bank, custom and record entry")
    unless (S.fromList (publicationRetired publication) == S.fromList (map (uncurry PhysicalRange) StorageBoundaries.retiredSettingsRegions))
      (Left "composed publication omits a retired settings region")
    forM_ adapters $ \(address,instruction) -> do
      unless (address >= image4Base && toInteger address+4 <= toInteger image4Base+toInteger (B.length staged) && word32At staged (offset address) == instruction && instruction .&. 0xff000000 == 0xea000000) (Left "composed entry encoding differs")
      let displacement = instruction .&. 0xffffff
          target = address+8+4*(if testBit displacement 23 then displacement .|. 0xff000000 else displacement)
      unless (any (`inRange` target) carriers) (Left "composed entry leaves its published carriers")
    forM_ (publicationRetired publication) $ \region ->
      forM_ [rangeStart region,rangeStart region+4..rangeEnd region-4] $ \address ->
        unless (address `elem` map fst adapters || any (`inRange` address) carriers || word32At staged (offset address) == contractFillWord contract) (Left "composed retired interior is not trapped")
  case (inputRole inputs, inputControl inputs) of
    (RetainedRole, Nothing) -> Right canonical
    (OwnedRole, Nothing) -> Right staged
    (ControlRole, Just control) -> do
      expected <- case contractLayout contract of
        TablePatched words' -> do
          property <- maybe (Left "a schema-1 control names its property") Right (controlProperty control)
          address <- case controlPatches control of
            [(address, _)] -> Right address
            _ -> Left "a schema-1 control restores one word"
          patch <- maybe (Left "control word is not a staged setter table word") Right
            (find (\word -> patchSetterTable word && patchAddress word == address && patchProperty word == property) words')
          Right (controlWord patch)
        HullOwned _ -> setterEntryControl contract
      unless (expected == control) (Left "control word is not the contract's control")
      Right (foldl' (\bytes (address, word) -> putWord32 (offset address) word bytes) staged (controlPatches control))
    _ -> Left "only the control role takes a control word"

-- | Every aligned A32 `bl` in the image whose target is in `targets`, with its return address.
branchLinks :: Word32 -> B.ByteString -> S.Set Word32 -> [(Word32, Word32)]
branchLinks base bytes targets =
  [ (target, address + 4)
  | offset <- [0, 4 .. B.length bytes - 4]
  , let word = word32At bytes offset
        address = base + fromIntegral offset
  , word `shiftR` 28 /= 0xf && (word `shiftR` 24) .&. 0xf == 0xb
  , let displacement = fromIntegral (word .&. 0xffffff) :: Int32
        signed = if testBit displacement 23 then displacement .|. complement 0xffffff else displacement
        target = address + 8 + fromIntegral (signed * 4)
  , S.member target targets ]

derivePlan :: PlanInputs -> Either String Plan
derivePlan inputs = do
  let contract = inputContract inputs
      role = inputRole inputs
  bankBoundary <- case (contractPublication contract,contractPublishedElf contract,inputBankBoundary inputs) of
    (Nothing,Nothing,Nothing) -> Right Nothing
    (Just publication,Just (PublishedElfIdentity elfDigest),Just boundary) -> do
      linkedDigest <- parseSha256Hex (contractLinkedSha256 contract)
      symbolsDigest <- parseSha256Hex (contractSymbolsSha256 contract)
      let expected=StorageBoundaries.BankApplyBinding elfDigest linkedDigest symbolsDigest (Placement.GuestAddress image4Base)
            [Placement.Carrier (Placement.GuestAddress low) (Placement.GuestAddress high) | PhysicalRange low high <- publicationCarriers publication]
      unless (StorageBoundaries.bankApplyBinding boundary == expected) (Left "bank apply proof is bound to another ELF, binary, symbol list, base, or carrier set")
      unless ([Placement.GuestAddress address | ("xm1_storage_apply",address) <- contractEntries contract]
        == [StorageBoundaries.bankApplyEntry boundary]) (Left "bank apply proof entry differs from the composed contract")
      Right (Just boundary)
    _ -> Left "bank apply proof and composed publication differ"
  forM_ codeImages $ \name -> do
    scan <- maybe (Left ("contract scans no " ++ T.unpack (imageNameWire name))) Right
      (find ((== imageNameWire name) . scanName) (contractImages contract))
    bytes <- maybe (Left (T.unpack (imageNameWire name) ++ " is not bound")) Right (M.lookup name (inputImages inputs))
    unless (sha256Hex (sha256 bytes) == scanSha256 scan && B.length bytes == scanBytes scan)
      (Left (T.unpack (imageNameWire name) ++ " differs from the contract's canonical image"))
  unless (sha256Hex (sha256 (inputLinked inputs)) == contractLinkedSha256 contract) (Left "linked artifact differs from its contract")
  image4 <- roleImage4 inputs
  images <- forM codeImages $ \name -> do
    scan <- maybe (Left "scan image disappeared") Right (find ((== imageNameWire name) . scanName) (contractImages contract))
    bytes <- if name == Image4 then Right image4 else maybe (Left "image disappeared") Right (M.lookup name (inputImages inputs))
    pure (scanBase scan, bytes)
  let returns = concat [branchLinks base bytes (S.fromList [getterEntry, setterEntry]) | (base, bytes) <- images]
      bothSpellings sites = concat [[(address, label), (address + 0x40000000, label)] | (address, label) <- sites]
      returnSites = bothSpellings [(site, if target == getterEntry then GetterReturn else SetterReturn) | (target, site) <- returns]
      entries = bothSpellings [(getterEntry, GetterEntry), (setterEntry, SetterEntry), (area3Append, Area3Append), (propertyCommit, PropertyCommit)]
      carrier = PhysicalRange (contractCarrierStart contract) (contractCarrierEnd contract)
      propertyBytes = maybe (contractLinkedBytes contract) publicationPropertyCodeBytes (contractPublication contract)
      carrierCode = PhysicalRange (contractCarrierStart contract) (contractCarrierStart contract + fromIntegral propertyBytes)
  persistence <- case contractPublication contract of
    Nothing -> Right []
    Just publication -> do
      forM_ recordResultSites $ \(address,_,instruction) ->
        unless (word32At image4 (fromIntegral (address-image4Base)) == instruction) (Left "retained medium status boundary changed")
      own <- if role == RetainedRole then Right [] else do
        let candidates = [address | ("settings_custom_append",address) <- contractEntries contract]
        case candidates of
          [address] | any (\region -> address >= rangeStart region && address < rangeEnd region) (publicationCarriers publication) -> Right [(address,Area3Append)]
          _ -> Left "composed contract lacks the actual typed custom append boundary"
      pure (own ++ [(0x6aad94,RecordRead),(0x6ab2f4,RecordProgram),(0x6ab458,RecordErase)] ++ [(address,label) | (address,label,_) <- recordResultSites])
  (guards, stores) <- if role == RetainedRole then Right ([], []) else
    (,) <$> guardSites contract carrier carrierCode <*> storeSites contract (inputLinked inputs)
  bankApply <- case contractPublication contract of
    Nothing -> Right []
    Just publication -> do
      boundary <- maybe (Left "composed plan lacks its admitted bank apply boundary") Right bankBoundary
      target <- if role == RetainedRole then Right 0x6309c4 else case [address | ("xm1_storage_apply",address) <- contractEntries contract] of
        [address] | any (\region -> address >= rangeStart region && address < rangeEnd region) (publicationCarriers publication) -> Right address
        _ -> Left "composed contract lacks the actual bank apply boundary"
      let inherited=if role == RetainedRole then [] else [address | Placement.GuestAddress address <- StorageBoundaries.bankApplyRoots boundary]
          roots = S.fromList ([0x6309c4,target] ++ inherited)
          protected = S.fromList (map fst guards)
          completed = [(site,StorageSite BankApplyReturn) | (base,bytes) <- images,(_,site) <- branchLinks base bytes roots,S.notMember site protected]
      pure ((target,StorageSite BankApplyEntry):completed)
  customRefresh <- case contractPublication contract of
    Nothing -> Right []
    Just publication -> do
      target <- if role == RetainedRole then Right 0x6a86c4 else case [address | ("settings_custom_refresh",address) <- contractEntries contract] of
        [address] | any (\region -> address >= rangeStart region && address < rangeEnd region) (publicationCarriers publication) -> Right address
        _ -> Left "composed contract lacks the actual custom refresh boundary"
      let roots = S.fromList [0x6a86c4,target]
          protected = S.fromList (map fst guards)
          completed = [(site,StorageSite CustomRefreshReturn) | (base,bytes) <- images,(_,site) <- branchLinks base bytes roots,S.notMember site protected]
      pure ((target,StorageSite CustomRefreshEntry):completed)
  let allSites = entries ++ returnSites ++ bothSpellings persistence ++ bothSpellings bankApply ++ bothSpellings customRefresh ++ bothSpellings guards ++ bothSpellings [(site, AccessorStore accessor) | (site, accessor) <- stores]
      sites = M.fromList allSites
  when (M.size sites /= length allSites) (Left "two trace sites share an address")
  unless (length returnSites >= 2) (Left "no getter or setter call site was found")
  pure Plan
    { planRole = role
    , planSites = sites
    , planStock = [PhysicalRange base (base + fromIntegral (B.length bytes)) | (base, bytes) <- images]
    , planStores = M.fromList stores
    , planCarriers = if role == RetainedRole then [] else carrier : maybe [] publicationCarriers (contractPublication contract)
    , planBankBoundary = bankBoundary
    , planReads = case contractPublication contract of
        Nothing -> M.empty
        Just _ -> M.fromList [(address,ranges) | (address,label) <- M.toList sites
          , let ranges = case label of
                  Area3Append -> [(0x89f874,52),(0x89f838,52),(0x9c295c,40),(0x89f86c,4)]
                  RecordProgram -> [(0x89fa80,0x600)]
                  StorageSite site -> lifecycleReads site
                  _ -> []
          , not (null ranges)]
    }

deriveStoragePlan :: PlanInputs -> StorageMatrix -> Either String Plan
deriveStoragePlan inputs matrix = do
  plan <- derivePlan inputs
  unless (case contractPublication (inputContract inputs) of Just _ -> True; Nothing -> False) (Left "storage lifecycle needs a composed publication")
  let source = matrixSource matrix
  unless (sourceCommitHex (matrixDecompCommit source) == "c44204c8c1c87e00ed15a7d33572968afadd1b29") (Left "storage lifecycle decomp source differs")
  unless (sourceCommitHex (matrixCatalogCommit source) == "1a92977f50f5bed7cd77a9bb1cb23271fdb0800c"
    && sourceCommitHex (matrixControlsCommit source) == "1a92977f50f5bed7cd77a9bb1cb23271fdb0800c") (Left "storage lifecycle control-map source differs")
  image4 <- maybe (Left "storage lifecycle image4 missing") Right (M.lookup Image4 (inputImages inputs))
  image5 <- maybe (Left "storage lifecycle image5 missing") Right (M.lookup Image5 (inputImages inputs))
  unless (sha256 image4 == matrixImage4 source && sha256 image5 == matrixImage5 source) (Left "storage lifecycle source images differ")
  let wordsAt address expected =
        let (base,bytes) = if address < 0xbf1a00 then (image4Base,image4) else (0xbf1a00,image5)
            offset = fromIntegral (address-base)
        in unless (address >= base && toInteger offset+4 <= toInteger (B.length bytes) && word32At bytes offset == expected) (Left "storage lifecycle source instruction differs")
  forM_ lifecycleWords (uncurry wordsAt)
  forM_ (zip [0x8284bc,0x8284c0..] [0x317,0,0x101,3,0x37243726,8,0]) (uncurry wordsAt)
  let added = [(address+alias,StorageSite site) | (address,site) <- lifecycleSites,alias <- [0,0x40000000]]
      readRanges = [(address+alias,ranges) | (address,site) <- lifecycleSites,let ranges = lifecycleReads site,not (null ranges),alias <- [0,0x40000000]]
  forM_ added $ \(address,_) -> unless (M.notMember address (planSites plan)) (Left "storage lifecycle trace site collides with the property plan")
  pure plan {planSites=M.union (M.fromList added) (planSites plan),planReads=M.union (M.fromList readRanges) (planReads plan)}

storeSites :: Contract -> B.ByteString -> Either String [(Word32, StoreAccessor)]
storeSites contract linked = case contractPublication contract of
  Nothing -> legacyStoreSites contract linked
  Just publication -> concat <$> forM [StorageByte,StorageHalf,CustomByte,CustomHalf,CustomWord,OutputByte,OutputHalf,StatusWord] (\accessor -> do
    let name = storeAccessorName accessor
        entries = [entry | (candidate,entry) <- contractEntries contract, candidate == name]
        (expected,index) = case accessor of
          StorageByte -> (StorageEncoding.storageStoreWords StorageEncoding.StorageByte,33)
          StorageHalf -> (StorageEncoding.storageStoreWords StorageEncoding.StorageHalf,33)
          CustomByte -> (customStoreWords CustomStoreByte,customStoreIndex CustomStoreByte)
          CustomHalf -> (customStoreWords CustomStoreHalf,customStoreIndex CustomStoreHalf)
          CustomWord -> (customStoreWords CustomStoreWord,customStoreIndex CustomStoreWord)
          OutputByte -> (StorageEncoding.storageOutputWords StorageEncoding.StorageByte,7)
          OutputHalf -> (StorageEncoding.storageOutputWords StorageEncoding.StorageHalf,12)
          StatusWord -> (StorageEncoding.storageStatusWords,2)
          _ -> ([],0)
        carriers = PhysicalRange (contractCarrierStart contract) (contractCarrierEnd contract) : publicationCarriers publication
    unless (not (null entries) && length entries == S.size (S.fromList entries)) (Left (T.unpack name ++ ": missing or duplicate boundary instance"))
    forM entries $ \entry -> do
      let size = 4*length expected
          offset = fromIntegral (entry-image4Base)
      unless (entry >= image4Base && offset+size <= B.length linked && any (\region -> entry >= rangeStart region && toInteger entry+toInteger size <= toInteger (rangeEnd region)) carriers) (Left (T.unpack name ++ ": boundary outside admitted carriers"))
      unless ([word32At linked (offset+word*4) | word <- [0..length expected-1]] == expected) (Left (T.unpack name ++ ": checked boundary encoding differs"))
      pure (entry+fromIntegral (index*4),accessor))

legacyStoreSites :: Contract -> B.ByteString -> Either String [(Word32, StoreAccessor)]
legacyStoreSites contract linked = forM [BankStore8,BankStore16,SaveStore8,SaveStore16] $ \accessor -> do
  let name = T.unpack (storeAccessorName accessor)
  entry <- maybe (Left ("the contract has no entry for " ++ name)) Right (lookup (storeAccessorName accessor) (contractEntries contract))
  let offset = fromIntegral (entry - contractCarrierStart contract)
  unless (entry >= contractCarrierStart contract && offset + 16 <= B.length linked) (Left (name ++ " lies outside the linked code"))
  let low = word32At linked offset
      high = word32At linked (offset + 4)
      immediate word = ((word `shiftR` 4) .&. 0xf000) .|. (word .&. 0xfff)
  unless (low .&. 0xfff0f000 == 0xe300c000 && high .&. 0xfff0f000 == 0xe340c000
          && (immediate high `shiftL` 16) .|. immediate low == rangeStart (storeRange accessor))
    (Left (name ++ " does not load its settings range into r12"))
  unless (word32At linked (offset + 8) == storeInstruction accessor) (Left (name ++ " does not store with its width"))
  pure (entry + 8, accessor)

-- | Schema 1: the words of the arms `ownedClass` retires that no delegated or unassigned property
-- still reaches, each arm running to the next arm target of either table, outside the carrier.
-- Schema 2: every hull word but the two entry adapters, the retained regions and the carrier, on the
-- getter's or the setter's side of the wrapper island. Both: the carrier words after the linked code.
guardSites :: Contract -> PhysicalRange -> PhysicalRange -> Either String [(Word32, SiteLabel)]
guardSites contract carrier carrierCode = do
  let properties = contractProperties contract
      inside range address = rangeStart range <= address && address < rangeEnd range
      fill = [(word, Guard CarrierFill) | word <- [rangeEnd carrierCode, rangeEnd carrierCode + 4 .. rangeEnd carrier - 4]]
      published = contractPublication contract
      publishedCarriers = maybe [] publicationCarriers published
      publishedEntries = maybe [] (map fst . publicationAdapters) published
      additional = [(word, Guard RetiredStorage) | region <- maybe [] publicationRetired published
        , word <- [rangeStart region, rangeStart region+4..rangeEnd region-4]
        , word `notElem` publishedEntries, not (any (`inside` word) (carrier:publishedCarriers))]
  unless (length properties == 645) (Left "contract classifies other than 645 properties")
  case contractLayout contract of
    HullOwned hull -> do
      unless (wrapperIsland `elem` hullRetained hull) (Left "the hull does not retain the wrapper island")
      let adapters = S.fromList (map fst (hullAdapters hull))
          retiredWord word = not (S.member word adapters || word `elem` publishedEntries || any (`inside` word) (hullRetained hull) || any (`inside` word) (carrier:publishedCarriers))
          side word = Guard (if word < rangeStart wrapperIsland then RetiredGetterArm else RetiredSetterArm)
      pure (S.toList (S.fromList ([(word, side word) | word <- [hullStart hull, hullStart hull + 4 .. hullEnd hull - 4], retiredWord word] ++ fill ++ additional)))
    TablePatched _ -> do
      let live select = S.fromList [select arms | arms <- properties, not (ownedClass (armsClass arms))]
          retired select = S.fromList [select arms | arms <- properties, ownedClass (armsClass arms)] `S.difference` live select
          bounds = S.fromList (concat [[armsGetter arms, armsSetter arms] | arms <- properties])
                     `S.union` S.fromList [rangeStart carrier, rangeEnd carrier]
          extent arm = case S.lookupGT arm bounds of
            Just end -> Right [arm, arm + 4 .. end - 4]
            Nothing -> Left "a retired arm has no following arm target"
      getters <- concat <$> mapM extent (S.toList (retired armsGetter))
      setters <- concat <$> mapM extent (filter (not . inside carrier) (S.toList (retired armsSetter)))
      when (any (inside carrier) (getters ++ setters)) (Left "a retired arm outside the carrier runs into it")
      pure ( [(word, Guard RetiredGetterArm) | word <- getters]
          ++ [(word, Guard RetiredSetterArm) | word <- setters]
          ++ fill )

-- | Classes the writer's PC with its bit-30 alias cleared.
classifyWriter :: Plan -> Word32 -> WriterClass
classifyWriter plan aliased
  | M.member pc (planStores plan) = OwnedAccessor
  | any (`contains` pc) (planCarriers plan) = CarrierBody
  | any (`contains` pc) (planStock plan) = StockCode
  | otherwise = OtherWriter
 where
  pc = aliased .&. complement 0x40000000
  contains range address = rangeStart range <= address && address < rangeEnd range
