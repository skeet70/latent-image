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
  , analysisPath
  , PropertyClass (..)
  , ownedClass
  , PropertyArms (..)
  , PatchWord (..)
  , ScanImage (..)
  , Contract (..)
  , decodeContract
  , controlPatch
  , controlWord
  , ControlWord (..)
  , SiteLabel (..)
  , siteLabels
  , siteLabelWire
  , parseSiteLabel
  , StoreAccessor (..)
  , storeAccessorName
  , storeRange
  , storeWidth
  , GuardKind (..)
  , WriterClass (..)
  , PhysicalRange (..)
  , Plan (..)
  , PlanInputs (..)
  , derivePlan
  , roleImage4
  , classifyWriter
  , getterEntry
  , setterEntry
  , area3Append
  , propertyCommit
  , settingsRanges
  , specValue
  , hitLimit
  , watchLimit
  , preKeyIdle
  , maxIdle
  , Instructions (..)
  , word32At
  ) where

import Control.Monad (forM, forM_, unless, when)
import Data.Aeson (FromJSON (..), Value, object, withObject, (.:), (.=))
import qualified Data.Aeson as A
import qualified Data.Aeson.Key as K
import Data.Aeson.Types (Parser)
import Data.Bits (complement, shiftL, shiftR, testBit, (.&.), (.|.))
import qualified Data.ByteString as B
import Data.Int (Int32)
import Data.List (find)
import qualified Data.Map.Strict as M
import qualified Data.Set as S
import qualified Data.Text as T
import Data.Word (Word32)
import Emulation.Sha256 (sha256, sha256Hex)
import Emulation.WatchObservation (Instructions (..))
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

data PropertyClass = Delegated | OwnedField | OwnedSlotField | OwnedUnassigned deriving (Eq, Show)

-- | The classes whose arms the owned roles retire: a value in a bank record field or a slot table field.
ownedClass :: PropertyClass -> Bool
ownedClass kind = kind == OwnedField || kind == OwnedSlotField

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
  , contractPatchWords :: [PatchWord]
  , contractProperties :: [PropertyArms]
  , contractImages :: [ScanImage]
  -- | Each linked function's symbol name and entry address.
  , contractEntries :: [(T.Text, Word32)]
  } deriving (Eq, Show)

hexAddress :: T.Text -> Parser Word32
hexAddress text = case T.unpack text of
  '0' : 'x' : digits | [(value, "")] <- readHex digits, value <= (0xffffffff :: Integer) -> pure (fromIntegral value)
  _ -> fail ("not a hexadecimal word: " ++ T.unpack text)

instance FromJSON Contract where
  parseJSON = withObject "property-service contract" $ \fields -> do
    schema <- fields .: "schema"
    unless (schema == (1 :: Int)) (fail "unsupported property-service contract schema")
    carrier <- fields .: "carrier"
    patch <- fields .: "patch"
    classification <- fields .: "classification"
    sources <- fields .: "source_sha256"
    scan <- fields .: "scan"
    stack <- fields .: "stack"
    entries <- stack .: "entries" >>= mapM (withObject "stack entry" $ \entry -> (,) <$> entry .: "name" <*> entry .: "entry")
    words' <- patch .: "words" >>= mapM (withObject "patch word" $ \word -> do
      table <- word .: "table"
      setter <- case table :: T.Text of
        "getter" -> pure False
        "setter" -> pure True
        _ -> fail "patch word table is neither getter nor setter"
      PatchWord <$> word .: "address" <*> word .: "original" <*> word .: "patched" <*> word .: "property" <*> pure setter)
    properties <- classification .: "properties" >>= mapM (withObject "property class" $ \property -> do
      kind <- property .: "class"
      decided <- case kind :: T.Text of
        "delegated" -> pure Delegated
        "owned-field" -> pure OwnedField
        "owned-slot-field" -> pure OwnedSlotField
        "owned-unassigned" -> pure OwnedUnassigned
        _ -> fail "unknown property class"
      PropertyArms <$> (property .: "property" >>= hexAddress) <*> pure decided
        <*> (property .: "getterArm" >>= hexAddress) <*> (property .: "setterArm" >>= hexAddress))
    images <- scan .: "images" >>= mapM (withObject "scan image" $ \image ->
      ScanImage <$> image .: "image" <*> image .: "base" <*> image .: "bytes" <*> image .: "sha256")
    Contract <$> fields .: "candidate_dat_sha256" <*> sources .: "dat" <*> fields .: "linked_sha256"
      <*> fields .: "linked_bytes" <*> fields .: "symbols_sha256" <*> fields .: "staged_image4_sha256"
      <*> carrier .: "start" <*> carrier .: "end" <*> carrier .: "fill_word" <*> pure words' <*> pure properties <*> pure images <*> pure entries

decodeContract :: B.ByteString -> Either String Contract
decodeContract = A.eitherDecodeStrict'

-- | The guest control's one setter table word left at its retired stock arm.
data ControlWord = ControlWord { controlProperty :: Word32, controlAddress :: Word32, controlTarget :: Word32 } deriving (Eq, Show)

-- | The staged setter table word of the owned property the guest control restores.
controlPatch :: Contract -> Word32 -> Either String PatchWord
controlPatch contract property = do
  arms <- maybe (Left "PROPERTY is not classified") Right (find ((== property) . armsProperty) (contractProperties contract))
  unless (ownedClass (armsClass arms)) (Left "PROPERTY is neither an owned field nor an owned slot field")
  unless (armsSetter arms < contractCarrierStart contract || armsSetter arms >= contractCarrierEnd contract)
    (Left "PROPERTY's stock setter arm lies in the carrier")
  maybe (Left "PROPERTY has no staged setter table word") Right
    (find (\word -> patchSetterTable word && patchProperty word == property) (contractPatchWords contract))

controlWord :: PatchWord -> ControlWord
controlWord patch = ControlWord (patchProperty patch) (patchAddress patch) (patchOriginal patch)

data GuardKind = RetiredGetterArm | RetiredSetterArm | CarrierFill deriving (Eq, Ord, Show, Enum, Bounded)
data SiteLabel = GetterEntry | SetterEntry | Area3Append | PropertyCommit | GetterReturn | SetterReturn | AccessorStore StoreAccessor | Guard GuardKind
  deriving (Eq, Ord, Show)

siteLabels :: [SiteLabel]
siteLabels = [GetterEntry, SetterEntry, Area3Append, PropertyCommit, GetterReturn, SetterReturn]
  ++ map AccessorStore [minBound .. maxBound] ++ map Guard [minBound .. maxBound]

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

parseSiteLabel :: String -> Maybe SiteLabel
parseSiteLabel text = lookup text [(siteLabelWire label, label) | label <- siteLabels]

-- | The linked store accessors: `movw r12`, `movt r12`, the store of r1 to r12 + r0, `bx lr`.
data StoreAccessor = BankStore8 | BankStore16 | SaveStore8 | SaveStore16 deriving (Eq, Ord, Show, Enum, Bounded)

storeAccessorName :: StoreAccessor -> T.Text
storeAccessorName accessor = case accessor of
  BankStore8 -> "xm1_bank_store8"
  BankStore16 -> "xm1_bank_store16"
  SaveStore8 -> "xm1_save_store8"
  SaveStore16 -> "xm1_save_store16"

storeWidth :: StoreAccessor -> Int
storeWidth accessor = if accessor `elem` [BankStore8, SaveStore8] then 1 else 2

-- | The settings range r12 addresses.
storeRange :: StoreAccessor -> PhysicalRange
storeRange accessor = if accessor `elem` [BankStore8, BankStore16] then PhysicalRange 0x96cc64 0x96cfe8 else PhysicalRange 0x89ec00 0x89edc2

-- | `strb r1, [r12, r0]` or `strh r1, [r12, r0]`.
storeInstruction :: StoreAccessor -> Word32
storeInstruction accessor = if storeWidth accessor == 1 then 0xe7cc1000 else 0xe18c10b0

-- | `CarrierBody` is any carrier word other than an accessor's store instruction.
data WriterClass = StockCode | OwnedAccessor | CarrierBody | OtherWriter deriving (Eq, Ord, Show)

-- | A half-open physical range.
data PhysicalRange = PhysicalRange { rangeStart :: Word32, rangeEnd :: Word32 } deriving (Eq, Ord, Show)

getterEntry, setterEntry, area3Append, propertyCommit :: Word32
getterEntry = 0x61e110
setterEntry = 0x624694
area3Append = 0x6ac7d4
propertyCommit = 0xd4fc74

-- | The bank images, the bank-0 previous-value image and the settings image (guest `0x409c281c`).
settingsRanges :: [PhysicalRange]
settingsRanges = [PhysicalRange 0x89ec00 0x89edc2, PhysicalRange 0x96cc64 0x96cfe8, PhysicalRange 0x9c281c (0x9c281c + 1144)]

hitLimit, watchLimit :: Int
hitLimit = 1500000
watchLimit = 262144

-- | Guest instructions run with no input before every press, ahead of the bridge's render-drain gate.
preKeyIdle :: Instructions
preKeyIdle = Instructions 20000000

-- | The bridge's largest `idle` request, its MAX_SETTLE.
maxIdle :: Instructions
maxIdle = Instructions 90000000

data Plan = Plan
  { planRole :: Role
  , planSites :: M.Map Word32 SiteLabel
  , planStock :: [PhysicalRange]
  -- | The owned roles' store instruction of each accessor, at its physical address.
  , planStores :: M.Map Word32 StoreAccessor
  , planCarrier :: Maybe PhysicalRange
  } deriving (Eq, Show)

-- | The observer's `--reset-observation` document for the plan.
specValue :: Plan -> Value
specValue plan = object
  [ "schema" .= (1 :: Int)
  , "sites" .= object [K.fromString (hexWord address) .= siteLabelWire label | (address, label) <- M.toList (planSites plan)]
  , "hit_limit" .= hitLimit
  , "write_ranges" .= [[rangeStart range, rangeEnd range] | range <- settingsRanges]
  , "watch_limit" .= watchLimit
  ]
 where
  hexWord value = "0x" ++ replicate (8 - length (showHex value "")) '0' ++ showHex value ""

data PlanInputs = PlanInputs
  { inputRole :: Role
  , inputContract :: Contract
  , inputImages :: M.Map ImageName B.ByteString
  , inputLinked :: B.ByteString
  , inputControl :: Maybe ControlWord
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
      carrierWords = (fromIntegral (contractCarrierEnd contract - contractCarrierStart contract) - B.length (inputLinked inputs)) `div` 4
      carrier = inputLinked inputs <> B.concat (replicate carrierWords (B.pack [fromIntegral (contractFillWord contract `shiftR` (8 * index)) | index <- [0 .. 3 :: Int]]))
      staged = foldl' (\bytes patch -> putWord32 (offset (patchAddress patch)) (patchPatched patch) bytes)
        (B.take (offset (contractCarrierStart contract)) canonical <> carrier <> B.drop (offset (contractCarrierEnd contract)) canonical)
        (contractPatchWords contract)
  unless (B.length (inputLinked inputs) == contractLinkedBytes contract && (B.length (inputLinked inputs) `mod` 4) == 0)
    (Left "linked artifact length differs from its contract")
  unless (B.length staged == B.length canonical) (Left "staged image4 changes its length")
  unless (sha256Hex (sha256 staged) == contractStagedImage4 contract) (Left "rebuilt staged image4 differs from its contract digest")
  case (inputRole inputs, inputControl inputs) of
    (RetainedRole, Nothing) -> Right canonical
    (OwnedRole, Nothing) -> Right staged
    (ControlRole, Just control) -> do
      patch <- maybe (Left "control word is not a staged setter table word") Right
        (find (\word -> patchSetterTable word && patchAddress word == controlAddress control && patchProperty word == controlProperty control)
          (contractPatchWords contract))
      unless (patchOriginal patch == controlTarget control) (Left "control word target is not the property's stock setter arm")
      Right (putWord32 (offset (controlAddress control)) (controlTarget control) staged)
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
      carrierCode = PhysicalRange (contractCarrierStart contract) (contractCarrierStart contract + fromIntegral (contractLinkedBytes contract))
  (guards, stores) <- if role == RetainedRole then Right ([], []) else
    (,) <$> guardSites contract carrier carrierCode <*> storeSites contract (inputLinked inputs)
  let allSites = entries ++ returnSites ++ bothSpellings guards ++ bothSpellings [(site, AccessorStore accessor) | (site, accessor) <- stores]
      sites = M.fromList allSites
  when (M.size sites /= length allSites) (Left "two trace sites share an address")
  unless (length returnSites >= 2) (Left "no getter or setter call site was found")
  pure Plan
    { planRole = role
    , planSites = sites
    , planStock = [PhysicalRange base (base + fromIntegral (B.length bytes)) | (base, bytes) <- images]
    , planStores = M.fromList stores
    , planCarrier = if role == RetainedRole then Nothing else Just carrier
    }

-- | Each store accessor's store instruction, refused unless the linked accessor loads its settings
-- range's start into r12 and stores with the accessor's width.
storeSites :: Contract -> B.ByteString -> Either String [(Word32, StoreAccessor)]
storeSites contract linked = forM [minBound .. maxBound] $ \accessor -> do
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

-- | The words of the owned-field and owned-slot-field arms no delegated or unassigned property still
-- reaches, each arm running to the next arm target of either table, outside the carrier; and the
-- carrier words after the linked code.
guardSites :: Contract -> PhysicalRange -> PhysicalRange -> Either String [(Word32, SiteLabel)]
guardSites contract carrier carrierCode = do
  let properties = contractProperties contract
      live select = S.fromList [select arms | arms <- properties, not (ownedClass (armsClass arms))]
      retired select = S.fromList [select arms | arms <- properties, ownedClass (armsClass arms)] `S.difference` live select
      bounds = S.fromList (concat [[armsGetter arms, armsSetter arms] | arms <- properties])
                 `S.union` S.fromList [rangeStart carrier, rangeEnd carrier]
      inside range address = rangeStart range <= address && address < rangeEnd range
      extent arm = case S.lookupGT arm bounds of
        Just end -> Right [arm, arm + 4 .. end - 4]
        Nothing -> Left "a retired arm has no following arm target"
  getters <- concat <$> mapM extent (S.toList (retired armsGetter))
  setters <- concat <$> mapM extent (filter (not . inside carrier) (S.toList (retired armsSetter)))
  when (any (inside carrier) (getters ++ setters)) (Left "a retired arm outside the carrier runs into it")
  unless (length properties == 645) (Left "contract classifies other than 645 properties")
  pure ( [(word, Guard RetiredGetterArm) | word <- getters]
      ++ [(word, Guard RetiredSetterArm) | word <- setters]
      ++ [(word, Guard CarrierFill) | word <- [rangeEnd carrierCode, rangeEnd carrierCode + 4 .. rangeEnd carrier - 4]] )

-- | Classes the writer's PC with its bit-30 alias cleared.
classifyWriter :: Plan -> Word32 -> WriterClass
classifyWriter plan aliased
  | M.member pc (planStores plan) = OwnedAccessor
  | maybe False (`contains` pc) (planCarrier plan) = CarrierBody
  | any (`contains` pc) (planStock plan) = StockCode
  | otherwise = OtherWriter
 where
  pc = aliased .&. complement 0x40000000
  contains range address = rangeStart range <= address && address < rangeEnd range
