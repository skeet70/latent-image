{-# LANGUAGE OverloadedStrings #-}
module PropertyPairFixture where

import BankApplyFixture (bankApplyFixtureAt)
import Control.Monad (unless)
import Data.Aeson (Value (..), object, (.=))
import qualified Data.Aeson as A
import qualified Data.Aeson.Key as K
import qualified Data.Aeson.KeyMap as KM
import Data.Aeson.Types (parseMaybe)
import Data.Bits (shiftR, xor, (.&.), (.|.))
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as BL
import Data.Either (isLeft)
import Data.List (zip5)
import Data.Maybe (fromMaybe)
import qualified Data.Map.Strict as M
import qualified Data.Text as T
import qualified Data.Vector as V
import Data.Word (Word32, Word8)
import qualified Emulation.LocalUiMenu as LocalUi
import Emulation.PropertyPairCheck
import Emulation.PropertyPairPlan hiding (image4Base)
import Emulation.PropertyPairRoutes
import Emulation.Sha256 (sha256, sha256Hex)
import qualified Emulation.SettingsStorageMatrix as Storage
import EmulatorUsb (CrawlKey (..), ModeDial (..))
import Numeric (showHex)
import qualified Replacement.PropertyService.Artifact as StorageEncoding
import qualified CustomAdmission as StorageBoundaries
import qualified Replacement.PropertyService.Placement as Placement

check :: String -> Bool -> IO Bool
check name passed = do
  unless passed (putStrLn ("FAIL " ++ name))
  pure passed

digestOf :: B.ByteString -> String
digestOf = sha256Hex . sha256

hex :: Word32 -> String
hex value = "0x" ++ showHex value ""

littleWords :: [Word32] -> B.ByteString
littleWords values = B.pack [fromIntegral (value `shiftR` (8 * index)) | value <- values, index <- [0 .. 3 :: Int]]

putWord :: Word32 -> Word32 -> B.ByteString -> B.ByteString
putWord address value bytes = let offset = fromIntegral (address - image4Base) in
  B.take offset bytes <> littleWords [value] <> B.drop (offset + 4) bytes

-- A synthetic image4, carrier and contract.

image4Base, carrierStart, carrierEnd, fillWord, rustBody, retiredGetter, retiredSetter, slotGetter, slotSetter, getterCall, setterCall :: Word32
image4Base = 0x61c400
carrierStart = 0x62c4b0
carrierEnd = 0x62c530
fillWord = 0xe7f000f0
rustBody = 0x62c4f0
retiredGetter = 0x621100
retiredSetter = 0x622100
slotGetter = 0x621180
slotSetter = 0x622280
getterCall = 0x620000
setterCall = 0x620010

branchLink :: Word32 -> Word32 -> Word32
branchLink site target = 0xeb000000 .|. (((target - (site + 8)) `shiftR` 2) .&. 0xffffff)

-- | The four store accessors, then one body word.
linked :: B.ByteString
linked = littleWords
  [ 0xe30ccc64, 0xe340c096, 0xe7cc1000, 0xe12fff1e
  , 0xe30ccc64, 0xe340c096, 0xe18c10b0, 0xe12fff1e
  , 0xe30ecc00, 0xe340c089, 0xe7cc1000, 0xe12fff1e
  , 0xe30ecc00, 0xe340c089, 0xe18c10b0, 0xe12fff1e
  , 0xe1a00000 ]

storeSite :: StoreAccessor -> Word32
storeSite accessor = carrierStart + 16 * fromIntegral (fromEnum accessor) + 8

canonical4 :: B.ByteString
canonical4 = foldr (uncurry putWord) (B.replicate 0x20000 0)
  [ (getterCall, branchLink getterCall getterEntry), (setterCall, branchLink setterCall setterEntry)
  , (0x623000, retiredGetter), (0x623004, retiredSetter), (0x623008, slotGetter), (0x62300c, slotSetter) ]

staged4 :: B.ByteString
staged4 = foldr (uncurry putWord) spliced
  [(0x623000, carrierStart), (0x623004, carrierStart + 0x10), (0x623008, carrierStart), (0x62300c, carrierStart + 0x10)]
 where
  offset address = fromIntegral (address - image4Base)
  fill = littleWords (replicate ((fromIntegral (carrierEnd - carrierStart) - B.length linked) `div` 4) fillWord)
  spliced = B.take (offset carrierStart) canonical4 <> linked <> fill <> B.drop (offset carrierEnd) canonical4

otherImages :: [(ImageName, Word32)]
otherImages = [(Image5, 0xbf1a00), (Plain, 0x1100000), (LoaderA, 0x40000), (LoaderB, 0xd0000)]

images :: M.Map ImageName B.ByteString
images = M.fromList ((Image4, canonical4) : [(name, B.replicate 16 0) | (name, _) <- otherImages])

sourceDat, ownedDat, symbols, observer :: String
sourceDat = replicate 64 'a'
ownedDat = replicate 64 'b'
symbols = digestOf "synthetic symbols"
observer = replicate 64 'd'

contractBytes :: B.ByteString
contractBytes = BL.toStrict (A.encode (object
  [ "schema" .= (1 :: Int)
  , "carrier" .= object ["start" .= carrierStart, "end" .= carrierEnd, "fill_word" .= fillWord]
  , "patch" .= object ["words" .=
      [ object ["address" .= (0x623000 :: Word32), "original" .= retiredGetter, "patched" .= carrierStart, "property" .= (5 :: Int), "table" .= ("getter" :: T.Text)]
      , object ["address" .= (0x623004 :: Word32), "original" .= retiredSetter, "patched" .= (carrierStart + 0x10), "property" .= (5 :: Int), "table" .= ("setter" :: T.Text)]
      , object ["address" .= (0x623008 :: Word32), "original" .= slotGetter, "patched" .= carrierStart, "property" .= (7 :: Int), "table" .= ("getter" :: T.Text)]
      , object ["address" .= (0x62300c :: Word32), "original" .= slotSetter, "patched" .= (carrierStart + 0x10), "property" .= (7 :: Int), "table" .= ("setter" :: T.Text)] ]]
  , "classification" .= object ["properties" .= [property index | index <- [0 .. 644 :: Word32]]]
  , "source_sha256" .= object ["dat" .= sourceDat]
  , "scan" .= object ["images" .= (scanImage "image4" image4Base canonical4 : [scanImage (imageNameWire name) base (images M.! name) | (name, base) <- otherImages])]
  , "candidate_dat_sha256" .= ownedDat
  , "linked_sha256" .= digestOf linked
  , "linked_bytes" .= B.length linked
  , "symbols_sha256" .= symbols
  , "staged_image4_sha256" .= digestOf staged4
  , "stack" .= object ["entries" .= [object ["name" .= storeAccessorName accessor, "entry" .= (storeSite accessor - 8)] | accessor <- [minBound .. maxBound]]]
  ]))
 where
  scanImage name base bytes = object ["image" .= (name :: T.Text), "base" .= base, "bytes" .= B.length bytes, "sha256" .= digestOf bytes]
  property index = let (kind, getter, setter) = arms index in
    object ["class" .= (kind :: T.Text), "property" .= hex index, "getterArm" .= hex getter, "setterArm" .= hex setter]
  arms index
    | index == 5 = ("owned-field", retiredGetter, retiredSetter)
    | index == 6 = ("delegated", 0x621140, 0x622200)
    | index == 7 = ("owned-slot-field", slotGetter, slotSetter)
    | index == 8 = ("delegated", 0x6211c0, 0x6222c0)
    | index == 9 = ("owned-read-only-field", 0x621000, 0x622000)
    | index == 10 = ("owned-write-only-pair", 0x621000, 0x622000)
    | otherwise = ("delegated", 0x621000, 0x622000)

-- A synthetic schema-2 contract: the hull [getterEntry, 0x6309c4) is owned.

hullEnd', getterAdapter, setterAdapter, islandWord, stockSetterPush :: Word32
hullEnd' = 0x6309c4
getterAdapter = 0xea003ce6
setterAdapter = 0xea001f86
islandWord = 0xe92d4818
stockSetterPush = 0xe92d49f8

retainedSpans :: [(Word32, Word32)]
retainedSpans = [(0x6245f8, 0x624694), (0x620000, 0x620020)]

canonicalHull4 :: B.ByteString
canonicalHull4 = putWord setterEntry stockSetterPush (putWord 0x6245f8 islandWord canonical4)

stagedHull4 :: B.ByteString
stagedHull4 = putWord getterEntry getterAdapter (putWord setterEntry setterAdapter spliced)
 where
  offset address = fromIntegral (address - image4Base)
  kept address = any (\(start, end) -> start <= address && address < end) retainedSpans
  hullWords = B.concat [if kept address then B.take 4 (B.drop (offset address) canonicalHull4) else littleWords [fillWord] | address <- [getterEntry, getterEntry + 4 .. hullEnd' - 4]]
  filled = B.take (offset getterEntry) canonicalHull4 <> hullWords <> B.drop (offset hullEnd') canonicalHull4
  fill = littleWords (replicate ((fromIntegral (carrierEnd - carrierStart) - B.length linked) `div` 4) fillWord)
  spliced = B.take (offset carrierStart) filled <> linked <> fill <> B.drop (offset carrierEnd) filled

hullImages :: M.Map ImageName B.ByteString
hullImages = M.insert Image4 canonicalHull4 images

hullContractBytes :: B.ByteString
hullContractBytes = case A.decodeStrict contractBytes of
  Just (Object fields) -> BL.toStrict (A.encode (Object (KM.union (KM.fromList overrides) (KM.delete "patch" fields))))
  _ -> error "contractBytes is not an object"
 where
  overrides =
    [ "schema" .= (2 :: Int)
    , "hull" .= object ["start" .= getterEntry, "end" .= hullEnd']
    , "adapters" .= [object ["address" .= getterEntry, "word" .= getterAdapter], object ["address" .= setterEntry, "word" .= setterAdapter]]
    , "retained_regions" .= [object ["start" .= start, "end" .= end] | (start, end) <- retainedSpans]
    , "staged_image4_sha256" .= digestOf stagedHull4
    , "scan" .= object ["images" .= (scanImage "image4" image4Base canonicalHull4 : [scanImage (imageNameWire name) base (images M.! name) | (name, base) <- otherImages])]
    ]
  scanImage name base bytes = object ["image" .= (name :: T.Text), "base" .= base, "bytes" .= B.length bytes, "sha256" .= digestOf bytes]

composedCodeStart :: Word32
composedCodeStart = 0x627578

bankApplyBody, bankApplyRootCall, customRefreshBody :: Word32
bankApplyBody = composedCodeStart + 0x1f0
bankApplyRootCall = 0x61d000
customRefreshBody = composedCodeStart + 0x1a0

bankApplyFixture :: StorageBoundaries.BankApplyElfInput
bankApplyFixture = bankApplyFixtureAt (Placement.GuestAddress bankApplyBody)
composedBoundaries :: [(StoreAccessor,Word32,[Word32],Int)]
composedBoundaries =
  [ (StorageByte,carrierStart,StorageEncoding.storageStoreWords StorageEncoding.StorageByte,33)
  , (StorageHalf,carrierStart+0x100,StorageEncoding.storageStoreWords StorageEncoding.StorageHalf,33)
  , (OutputByte,carrierStart+0x200,StorageEncoding.storageOutputWords StorageEncoding.StorageByte,7)
  , (StatusWord,carrierStart+0x240,StorageEncoding.storageStatusWords,2)
  , (OutputHalf,carrierStart+0x280,StorageEncoding.storageOutputWords StorageEncoding.StorageHalf,12)
  , (CustomByte,composedCodeStart,StorageBoundaries.customStoreWords StorageBoundaries.CustomStoreByte,fromIntegral (StorageBoundaries.customStoreIndex StorageBoundaries.CustomStoreByte))
  , (CustomHalf,composedCodeStart+0x60,StorageBoundaries.customStoreWords StorageBoundaries.CustomStoreHalf,fromIntegral (StorageBoundaries.customStoreIndex StorageBoundaries.CustomStoreHalf))
  , (CustomWord,composedCodeStart+0xc0,StorageBoundaries.customStoreWords StorageBoundaries.CustomStoreWord,fromIntegral (StorageBoundaries.customStoreIndex StorageBoundaries.CustomStoreWord))
  , (StorageByte,composedCodeStart+0x100,StorageEncoding.storageStoreWords StorageEncoding.StorageByte,33) ]
composedCanonical :: B.ByteString
composedCanonical = foldl' (\image (address,instruction) -> putWord address instruction image)
  (canonicalHull4 <> B.replicate (0x100000-B.length canonicalHull4) 0)
  (composedStatusWords ++ [(bankApplyRootCall,branchLink bankApplyRootCall 0x6309c4),(0x630a00,branchLink 0x630a00 setterEntry)])
composedEntries :: [(Word32,Word32)]
composedEntries = [(address,0xea000000 .|. (((target-address-8) `shiftR` 2) .&. 0xffffff))
  | (address,_) <- StorageBoundaries.bankEntries ++ StorageBoundaries.customEntries ++ StorageBoundaries.recordEntries
  , let target = case address of
          0x6309c4 -> bankApplyBody
          0x6a86c4 -> customRefreshBody
          _ -> composedCodeStart]
composedStatusWords :: [(Word32,Word32)]
composedStatusWords =
  [(0x6ab314,0x18bd8800),(0x6ab328,0x18bd8800),(0x6ab348,0x88bd8800),(0x6ab390,0xe8bd8800),(0x6ab3ac,0xe8bd8800)
  ,(0x6ab474,0x18bd8800),(0x6ab4c0,0xe8bd8800),(0x6ab4c8,0xe8bd8800)]
composedImage :: B.ByteString
composedImage = foldl' (\image (address,instruction) -> putWord address instruction image) withApply
  (composedEntries ++ composedStatusWords
    ++ [(bankApplyRootCall,branchLink bankApplyRootCall 0x6309c4),(customRefreshBody,0xe12fff1e)])
  where
    filled = foldl' (\image (start,end) -> let offset = fromIntegral (start-image4Base)
      in B.take offset image <> littleWords (replicate (fromIntegral ((end-start) `div` 4)) fillWord) <> B.drop (fromIntegral (end-image4Base)) image)
      (stagedHull4 <> B.replicate (0x100000-B.length stagedHull4) 0) StorageBoundaries.retiredSettingsRegions
    bounded = foldl' (\image (_,address,instructions,_) -> foldl' (\bytes (index,instruction) -> putWord (address+index*4) instruction bytes) image (zip [0..] instructions)) filled composedBoundaries
    withApply = let offset = fromIntegral (bankApplyBody-image4Base)
                    bytes = StorageBoundaries.bankApplyLinkedBytes bankApplyFixture
                in B.take offset bounded <> bytes <> B.drop (offset+B.length bytes) bounded
composedContractBytes :: B.ByteString
composedContractBytes = BL.toStrict (A.encode (object
  [ "schema" .= (3::Int), "property_contract" .= property
  , "candidate_dat_sha256" .= ownedDat, "linked_sha256" .= digestOf composedImage, "linked_bytes" .= B.length composedImage
  , "symbols_sha256" .= digestOf (StorageBoundaries.bankApplySymbolBytes bankApplyFixture), "staged_image4_sha256" .= digestOf composedImage
  , "elf_sha256" .= digestOf (StorageBoundaries.bankApplyElfBytes bankApplyFixture)
  , "stack" .= object ["entries" .= ([object ["name" .= (name::String),"entry" .= address] | (name,address) <-
      [("settings_custom_append",composedCodeStart+0x1e0),("settings_custom_refresh",customRefreshBody),("xm1_storage_apply",bankApplyBody)]]
    ++ [object ["name" .= storeAccessorName accessor,"entry" .= address] | (accessor,address,_,_) <- composedBoundaries])]
  , "publication" .= object
    [ "carriers" .= [region composedCodeStart (composedCodeStart+0x220)]
    , "retired_regions" .= [region start end | (start,end) <- StorageBoundaries.retiredSettingsRegions]
    , "adapters" .= [object ["address" .= address,"word" .= instruction] | (address,instruction) <- composedEntries] ] ]))
  where
    region start end = object ["start" .= start,"end" .= end]
    property = case A.decodeStrict hullContractBytes of
      Just (Object fields) -> Object (KM.union (KM.fromList
        [ "carrier" .= object ["start" .= carrierStart,"end" .= (carrierStart+0x400),"fill_word" .= fillWord]
        , "linked_bytes" .= (0x400::Int)
        , "scan" .= object ["images" .= (scanImage "image4" image4Base composedCanonical : [scanImage (imageNameWire name) base (images M.! name) | (name,base) <- otherImages])]
        , "stack" .= object ["entries" .= [object ["name" .= storeAccessorName accessor,"entry" .= address] | (accessor,address,_,_) <- take 5 composedBoundaries]] ]) fields)
      _ -> error "hull contract fixture"
    scanImage name base bytes = object ["image" .= (name::T.Text),"base" .= base,"bytes" .= B.length bytes,"sha256" .= digestOf bytes]
composedInputs :: Either String PlanInputs
composedInputs = fixturePlanInputs OwnedRole composedBindings

crawlRoute :: RouteName
crawlRoute = RouteName "crawl-trial-00001"

routesBytes :: B.ByteString
routesBytes = routesWith [SetterEffect 5, CommitEffect]

-- | A route file whose crawl route records the given effects.
routesWith :: [CrawlEffect] -> B.ByteString
routesWith effects = BL.toStrict (encodeRouteFile (RouteFile (replicate 64 '0') "test" CrawlEffectsSource []
  (phaseRoutes ++ [Route crawlRoute (KeyRoute [KeyOk] (CrawlTrial 1 effects))])))

bindings :: Bindings
bindings = Bindings sourceDat ownedDat routesBytes contractBytes linked "synthetic symbols" images observer Nothing

composedBindings :: Bindings
composedBindings = bindings
  { bindRoutes = routesWith [SetterEffect 5]
  , bindContract = composedContractBytes
  , bindLinked = composedImage
  , bindSymbols = StorageBoundaries.bankApplySymbolBytes bankApplyFixture
  , bindImages = M.insert Image4 composedCanonical images
  , bindElf = Just (StorageBoundaries.bankApplyElfBytes bankApplyFixture)
  }

bindingsAdmitted :: AdmittedBindings
bindingsAdmitted = either (error . show) id (admitBindings bindings)

composedAdmitted :: AdmittedBindings
composedAdmitted = either (error . show) id (admitBindings composedBindings)

fixtureBankBoundary :: Contract -> Bindings -> Either String (Maybe StorageBoundaries.BankApplyBoundary)
fixtureBankBoundary contract bound =
  case contractPublication contract of
    Nothing -> pure Nothing
    Just publication -> do
      elf <- maybe (Left "composed test fixture has no actual ELF") Right (bindElf bound)
      Just <$> StorageBoundaries.admitBankApplyElf StorageBoundaries.BankApplyElfInput
        { StorageBoundaries.bankApplyImageBase=Placement.GuestAddress image4Base
        , StorageBoundaries.bankApplyCarriers=[Placement.Carrier (Placement.GuestAddress low) (Placement.GuestAddress high) | PhysicalRange low high <- publicationCarriers publication]
        , StorageBoundaries.bankApplyLinkedBytes=bindLinked bound
        , StorageBoundaries.bankApplySymbolBytes=bindSymbols bound
        , StorageBoundaries.bankApplyElfBytes=elf
        }

fixturePlanInputs :: Role -> Bindings -> Either String PlanInputs
fixturePlanInputs role bound = do
  contract <- decodeContract (bindContract bound)
  boundary <- fixtureBankBoundary contract bound
  pure (PlanInputs role contract (bindImages bound) (bindLinked bound) Nothing boundary)

-- Synthetic observations.

type Regions = M.Map Word32 B.ByteString

zeroRegions :: Regions
zeroRegions = M.fromList [(rangeStart range, B.replicate (fromIntegral (rangeEnd range - rangeStart range)) 0) | range <- settingsRanges]

poke :: [(Word32, Word8)] -> Regions -> Regions
poke bytes regions = foldl set regions bytes
 where
  set current (address, byte) = case M.lookupLE address current of
    Just (start, region) | address - start < fromIntegral (B.length region) ->
      let offset = fromIntegral (address - start) in M.insert start (B.take offset region <> B.singleton byte <> B.drop (offset + 1) region) current
    _ -> current

regionsValue :: Regions -> Value
regionsValue regions = toArray [object ["start" .= start, "bytes" .= concatMap byteHex (B.unpack bytes)] | (start, bytes) <- M.toList regions]
 where
  byteHex byte = let text = showHex byte "" in replicate (2 - length text) '0' ++ text

toArray :: [Value] -> Value
toArray = A.toJSON

hit :: SiteLabel -> Word32 -> [Word32] -> Word32 -> Word32 -> Value
hit label address arguments sp lr = object
  [ "site" .= siteLabelWire label, "address" .= hex address, "core" .= (0 :: Int), "instructions" .= (0 :: Int)
  , "registers" .= object [K.fromString register .= hex value | (register, value) <- zip ["r0", "r1", "r2", "r3"] (arguments ++ repeat 0)]
  , "sp" .= hex sp, "lr" .= hex lr, "cpsr" .= (0 :: Int) ]

memoryHit :: SiteLabel -> Word32 -> [Word32] -> [B.ByteString] -> Value
memoryHit label address arguments snapshots = case hit label address arguments stackPointer 0 of
  Object fields -> Object (KM.insert "memory" (A.toJSON (map bytesHex snapshots)) fields)
  _ -> error "hit fixture"
  where bytesHex = concatMap (\byte -> let value = showHex byte "" in replicate (2-length value) '0' ++ value) . B.unpack

hitFlags :: Word32 -> Value -> Value
hitFlags flags (Object fields) = Object (KM.insert "cpsr" (A.toJSON flags) fields)
hitFlags _ _ = error "hit fixture"

customWire, customPrevious, customProgramBytes :: B.ByteString
customWire = B.pack [0..51]
customPrevious = B.replicate 52 0x39
customProgramBytes = B.pack [8,0] <> B.drop 2 customWire <> B.replicate (0x600-52) 0xa7

customAppendAt :: Word32 -> [Word32] -> Value
customAppendAt address arguments = memoryHit Area3Append address arguments
  [customWire,customPrevious,B.take 40 (B.drop 4 customWire),littleWords [0x89f8f8]]

customAppend, customProgram, customProgramResult, customRead :: Value
customAppend = customAppendAt (composedCodeStart+0x1e0) [0x1000,0x2000,0x89f874]
customProgram = memoryHit RecordProgram 0x6ab2f4 [0x89fa80,0,52,0x600] [customProgramBytes]
customProgramResult = hit RecordProgramResult 0x6ab390 [0] stackPointer 0
customRead = hit RecordRead 0x6aad94 [0xb00000,0x8a0080] stackPointer 0

customPersistence :: [Value] -> Either String [PersistenceEvent]
customPersistence events = do
  inputs <- composedInputs
  either (Left . show) Right (persistenceEvents (inputContract inputs) (A.toJSON events))

cpuWrite :: Word32 -> Int -> Maybe Integer -> [Word8] -> Word32 -> Value
cpuWrite address width value before pc = object
  [ "source" .= ("cpu-write" :: T.Text), "address" .= address, "physical" .= address, "size" .= width
  , "value" .= value, "before" .= before, "byte_order" .= ("little" :: T.Text), "core" .= (0 :: Int)
  , "pc" .= pc, "instructions" .= (0 :: Int), "tick" .= (0 :: Int) ]

hostWrite :: Word32 -> Int -> Value
hostWrite address width = object ["source" .= ("host-write" :: T.Text), "physical" .= address, "size" .= width]

watch :: T.Text -> [Value] -> Value
watch views rows = object
  [ "enabled" .= True, "ranges" .= [[rangeStart range, rangeEnd range] | range <- settingsRanges]
  , "records" .= rows, "violation" .= Null, "classes" .= ([] :: [Value]), "views" .= views ]

-- | Step n of a chain presses OK n times from the attached state; `stepBefore` overrides its
-- parent's after regions.
data Step = Step { stepHits :: [Value], stepRows :: [Value], stepAfter :: Regions, stepStatus :: T.Text, stepMenu :: Value, stepBefore :: Maybe Regions, stepPrefix :: Value }

observation :: Role -> Step -> Observation
observation role step = chain role [step]

chain :: Role -> [Step] -> Observation
chain = chainWith routesBytes

chainWith :: B.ByteString -> Role -> [Step] -> Observation
chainWith routes = chainBound bindings { bindRoutes = routes }

chainBound :: Bindings -> Role -> [Step] -> Observation
chainBound bound role chained = Observation
  { obsReceipt = receipt, obsSpec = spec, obsReset = reset, obsHits = hits
  , obsSteps = steps, obsSession = session, obsBootResets = M.empty
  }
 where
  inputs = either error id (fixturePlanInputs role bound)
  plan = either error id (derivePlan inputs)
  spec = BL.toStrict (A.encode (specValue plan))
  segment phase = object ["phase" .= phase, "instructions" .= (0 :: Int), "hits_end" .= (0 :: Int), "watch" .= watch "observe" [], "regions" .= regionsValue zeroRegions]
  reset = BL.toStrict (A.encode (object
    [ "schema" .= (1 :: Int), "reset" .= object ["instructions" .= (0 :: Int)], "regions_at_reset" .= regionsValue zeroRegions
    , "segments" .= [segment phase | phase <- [minBound .. maxBound :: AttachPhase]] ]))
  hits = B.concat [BL.toStrict (A.encode value) <> "\n" | value <- concatMap stepHits chained]
  ends = scanl (+) 0 (map (length . stepHits) chained)
  befores = zipWith fromMaybe (zeroRegions : map stepAfter chained) (map stepBefore chained)
  steps = BL.toStrict (A.encode [ object
    [ "step" .= number, "parent" .= (number - 1), "keys" .= replicate number KeyOk, "hits" .= [from, to]
    , "idle" .= object ["instructions" .= (0 :: Int), "ran" .= instructionsValue preKeyIdle]
    , "bounded_post_settle_prefix" .= stepPrefix step
    , "press" .= object ["failure" .= Null, "settle" .= object ["status" .= stepStatus step], "menu" .= stepMenu step]
    , "watch" .= watch "fixed" (stepRows step), "regions_before" .= regionsValue before, "regions_after" .= regionsValue (stepAfter step) ]
    | (number, step, before, from, to) <- zip5 [1 :: Int ..] chained befores ends (drop 1 ends) ])
  session = BL.toStrict (A.encode (object ["schema" .= (1 :: Int), "session" .= ("test" :: T.Text)]))
  image4 = either error id (roleImage4 inputs)
  digest bytes = object ["sha256" .= digestOf bytes]
  same text = object ["sha256" .= (text :: String)]
  receipt = BL.toStrict (A.encode (object
    [ "role" .= roleWire role
    , "inputs" .= object
        ([ "dat" .= same (if role == RetainedRole then bindSourceDat bound else bindOwnedDat bound), "routes" .= digest (bindRoutes bound)
        , "contract" .= digest (bindContract bound), "linked" .= digest (bindLinked bound), "symbols" .= digest (bindSymbols bound), "observer" .= same (bindObserver bound)
        , "run" .= digest "run", "boot_rom" .= digest "rom", "adjustment_nand" .= digest "nand", "bridge" .= digest "bridge"
        , "analysis" .= object [K.fromText (imageNameWire name) .= digestOf bytes | (name, bytes) <- M.toList (bindImages bound)]
        , "role_image4_sha256" .= digestOf image4 ] ++ ["elf" .= digest bytes | Just bytes <- [bindElf bound]])
    , "files" .= object
        [ "spec.json" .= digestOf spec, "reset-observation.json" .= digestOf reset, "hits.jsonl" .= digestOf hits
        , "steps.json" .= digestOf steps, "emulator-session.json" .= digestOf session ]
    , "outcome" .= object ["status" .= ("completed" :: T.Text)]
    , "bounds" .= object ["hit_limit" .= hitLimit, "watch_limit" .= watchLimit]
    , "probe" .= Null
    , "pre_key_idle_instructions" .= instructionsValue preKeyIdle
    , "post_settle_prefix_ticks" .= tickValue postSettle ]))

uiView :: Word32 -> [Word32] -> Value
uiView address words' = uiViewBytes address (littleWords words')

uiViewBytes :: Word32 -> B.ByteString -> Value
uiViewBytes address raw = object ["address" .= address, "physical" .= bytes, "spellings" .= replicate 2 (replicate 2 bytes)]
 where
  bytes = concatMap (\byte -> let text = showHex byte "" in replicate (2 - length text) '0' ++ text) (B.unpack raw)

localPrefix :: Value
localPrefix = object
  [ "elapsed" .= object ["instructions" .= (10 :: Int), "ran" .= (0 :: Int), "ticks" .= tickValue postSettle]
  , "state" .= object
      [ "instructions" .= (10 :: Int), "tick" .= (42 :: Int)
      , "source_mode" .= uiView 0x9500ec [3, 0, 0, 0, 0, 0, 0, 0, 0, 0]
      , "ui" .= object
          [ "queues" .= [uiView 0x949dd0 [0, 0], uiView 0x94f5d8 [0, 0]]
          , "menu" .= uiView 0x9571d0 [0x1000, 0, 0]
          , "depth" .= uiView 0x9575f4 [0]
          , "node" .= uiView 0x1000 [1, 0, 5]
          , "words" .= [uiView address [0] | address <- [0x950098, 0x950148, 0x951424, 0x951528]] ] ] ]

prefixWith :: [K.Key] -> Value -> Value
prefixWith path replacement = change path localPrefix
 where
  change [] _ = replacement
  change (key : rest) (Object fields) = case KM.lookup key fields of
    Just value -> Object (KM.insert key (change rest value) fields)
    Nothing -> error "missing prefix fixture field"
  change _ _ = error "invalid prefix fixture path"

stackPointer, written :: Word32
stackPointer = 0x1000
written = 0x96cc74

setterIn, commit, setterOut, getterIn, getterOut :: Value
setterIn = hit SetterEntry setterEntry [5, 1] stackPointer (setterCall + 4)
commit = hit PropertyCommit propertyCommit [] stackPointer 0
setterOut = hit SetterReturn (setterCall + 4) [0] stackPointer 0
getterIn = hit GetterEntry getterEntry [5] stackPointer (getterCall + 4)
getterOut = hit GetterReturn (getterCall + 4) [0xab] stackPointer 0

storeHit :: StoreAccessor -> Word32 -> Word32 -> Value
storeHit accessor offset value = hit (AccessorStore accessor) (storeSite accessor) [offset, value] stackPointer 0

menu :: Value
menu = object ["node" .= (1 :: Int)]

-- | The owned setter stores 0xab through the bank accessor; the retained one from its stock arm.
ownedStep, retainedStep :: Step
ownedStep = Step [setterIn, commit, storeHit BankStore8 0x10 0xab, setterOut, getterIn, getterOut]
  [cpuWrite written 1 (Just 0xab) [0] (storeSite BankStore8)] (poke [(written, 0xab)] zeroRegions) "packet-consumed" menu Nothing localPrefix
retainedStep = ownedStep
  { stepHits = [setterIn, commit, setterOut, getterIn, getterOut]
  , stepRows = [cpuWrite written 1 (Just 0xab) [0] retiredSetter] }

qualified :: Role -> Step -> Either Refusal Qualified
qualified role step = qualify bindingsAdmitted role (observation role step)

refusal :: Role -> Step -> Maybe Refusal
refusal role step = either Just (const Nothing) (qualified role step)

crawlResult :: Role -> Step -> Maybe RouteResult
crawlResult role step = either (const Nothing) (lookup crawlRoute . map (\result -> (resultRoute result, result)) . qualifiedRoutes) (qualified role step)

compared :: Step -> Step -> Either Refusal ()
compared retained owned = comparedChains [retained] [owned]

comparedChains :: [Step] -> [Step] -> Either Refusal ()
comparedChains retained owned = do
  left <- qualify bindingsAdmitted RetainedRole (chain RetainedRole retained)
  right <- qualify bindingsAdmitted OwnedRole (chain OwnedRole owned)
  () <$ compareRoles left right

-- | A crawl route's comparison output from its ordered setter calls alone.
comparedSetters :: [Call] -> [Call] -> Either Refusal Value
comparedSetters retained owned = comparisonValue left right <$> compareRoles left right
 where
  left = qualifiedWith RetainedRole retained
  right = qualifiedWith OwnedRole owned
  qualifiedWith role setters = Qualified role [] 0 1 [RouteResult crawlRoute CrawlClass [] setters [] [] [] [] [] []] [] Nothing

comparedPersistence :: [Value] -> [Value] -> Either String ()
comparedPersistence retained owned = do
  left <- customPersistence retained
  right <- customPersistence owned
  let qualifiedWith role events = Qualified role [] 0 1 [RouteResult crawlRoute CrawlClass [] [] events [] [] [] [] []] [] Nothing
  either (Left . show) (const (Right ())) (compareRoles (qualifiedWith RetainedRole left) (qualifiedWith OwnedRole right))

getterKey5, getterKey6 :: GetterKey
getterKey5 = GetterKey (getterCall + 4) [5, 0, 0, 0]
getterKey6 = GetterKey (getterCall + 4) [6, 0, 0, 0]

-- | A getter call of the property returning the value.
getterPair :: Word32 -> Word32 -> [Value]
getterPair property value = [hit GetterEntry getterEntry [property] stackPointer (getterCall + 4), hit GetterReturn (getterCall + 4) [value] stackPointer 0]

-- | An empty child step of a one-step chain, with the given hits.
childWith :: [Value] -> Step
childWith hits = retainedStep { stepHits = hits, stepRows = [] }

-- | The retained crawl route's reported effect block runs, as (position, first effect, repeats), when
-- its route file records `recorded` and its step's hits end with `extra` after the standard setter,
-- commit and getter calls.
effectBlocksWith :: [CrawlEffect] -> [Value] -> Either Refusal (Maybe [(Int, Int, Int)])
effectBlocksWith recorded extra = do
  let routes = routesWith recorded
  context <- admitBindings (bindings { bindRoutes = routes })
  result <- qualify context RetainedRole (chainWith routes RetainedRole [retainedStep { stepHits = stepHits retainedStep ++ extra }])
  pure $ case qualifiedValue result of
    Object top | Just (Array summaries) <- KM.lookup "per_route" top ->
      case [runs | Object summary <- V.toList summaries, KM.lookup "route" summary == Just (A.toJSON crawlRoute), Just runs <- [KM.lookup "effect_block_runs" summary]] of
        [runs] -> parseMaybe (A.withArray "runs" (mapM (A.withObject "run" (\run -> (,,) <$> run A..: "position" <*> run A..: "first_effect" <*> run A..: "repeats")) . V.toList)) runs
        _ -> Nothing
    _ -> Nothing

-- | A setter entry for the property and its return.
setterPair :: Word32 -> [Value]
setterPair property = [hit SetterEntry setterEntry [property, 0] stackPointer (setterCall + 4), hit SetterReturn (setterCall + 4) [0] stackPointer 0]

blockHits :: [Value]
blockHits = concatMap setterPair timeDrivenSequence

-- | The retained and owned setter block runs of the first compared route, as (position, first call, repeats).
blockRunsOf :: Value -> Maybe ([(Int, Int, Int)], [(Int, Int, Int)])
blockRunsOf comparison = case comparison of
  Object top | Just (Array routes) <- KM.lookup "per_route" top, Just (Object route) <- routes V.!? 0
             , Just (Object runs) <- KM.lookup "setter_block_runs" route ->
    (,) <$> (KM.lookup "retained" runs >>= decodeRuns) <*> (KM.lookup "owned" runs >>= decodeRuns)
  _ -> Nothing
 where
  decodeRuns = parseMaybe (A.withArray "runs" (mapM (A.withObject "run" (\run -> (,,) <$> run A..: "position" <*> run A..: "first_call" <*> run A..: "repeats")) . V.toList))

blockCall :: Word32 -> Word32 -> Call
blockCall value property = Call [property, value, 0, 1] 0x630a04 (Just 1) ExactCaller

timeBlock, changedBlock :: [Call]
timeBlock = map (blockCall 0) timeDrivenSequence
changedBlock = map (blockCall 1) timeDrivenSequence

otherSetter :: Call
otherSetter = Call [5, 1, 0, 0] (setterCall + 4) (Just 0) ExactCaller

retainedSidecars :: Role -> T.Text -> RetainedObservation
retainedSidecars role status = retainedSidecarsWith role status [retainedStep]

-- | Sidecars for a chain whose steps carry the given hits.
retainedSidecarsWith :: Role -> T.Text -> [Step] -> RetainedObservation
retainedSidecarsWith role status steps' = RetainedObservation receipt (obsSteps base) (obsHits base)
 where
  base = chain role steps'
  receipt = case A.eitherDecodeStrict' (obsReceipt base) of
    Right (Object fields) -> BL.toStrict (A.encode (Object (KM.insert "outcome" (object ["status" .= status]) fields)))
    _ -> error "test receipt is not an object"

refusedAs :: Either Refusal a -> Refusal -> Bool
refusedAs result expected = either (== expected) (const False) result

wireField :: [K.Key] -> Value -> Value -> Value
wireField [] replacement _ = replacement
wireField (key : rest) replacement (Object fields) = case KM.lookup key fields of
  Just value -> Object (KM.insert key (wireField rest replacement value) fields)
  Nothing -> error "missing wire fixture field"
wireField _ _ _ = error "wire fixture is not an object"

traceAt :: Int -> (Value -> Value) -> [Value] -> [Value]
traceAt wanted change = zipWith (\index value -> if index == wanted then change value else value) [0 ..]

taskHit :: Role -> SiteLabel -> Word32 -> [Word32] -> Word32 -> Word32 -> Value
taskHit role label address arguments sp lr = case hit label address arguments sp lr of
  Object fields -> Object (KM.union (KM.fromList
    [ "cpsr" .= (0x13 :: Word32)
    , "task" .= object
        [ "kind" .= ("available" :: T.Text), "kernel" .= ("a" :: T.Text), "task_id" .= (5 :: Word32)
        , "node" .= hex node, "creation" .= hex (node + 0x1000)
        , "stack" .= object ["base" .= hex base, "bytes" .= (0x1000 :: Word32)]
        , "context_depth" .= (0 :: Word32), "cpsr" .= (0x13 :: Word32), "sp" .= hex sp
        ]
    ]) fields)
  _ -> error "task hit fixture"
  where
    node = if role == RetainedRole then 0x880000 else 0x990000
    base = if role == RetainedRole then 0x1000 else 0x3000

bankHits :: Role -> [Value]
bankHits role =
  [ taskHit role (StorageSite Storage.BankApplyEntry) entry [] top (bankApplyRootCall + 4)
  , taskHit role SetterEntry setterEntry [5,1] nested caller
  , taskHit role SetterReturn caller [0] nested 0
  , taskHit role (StorageSite Storage.BankApplyReturn) (bankApplyRootCall + 4) [0] top 0
  ]
  where
    entry = if role == RetainedRole then 0x6309c4 else bankApplyBody
    top = if role == RetainedRole then 0x2000 else 0x4000
    nested = top - if role == RetainedRole then 32 else 8
    caller = if role == RetainedRole then 0x630a04 else bankApplyBody + 8

bankStep :: [Value] -> Step
bankStep hits = Step hits [] zeroRegions "packet-consumed" menu Nothing localPrefix

bankQualified :: Role -> [Value] -> Either Refusal Qualified
bankQualified role hits = qualify composedAdmitted role (chainBound composedBindings role [bankStep hits])

bankCompared :: [Value] -> [Value] -> Either Refusal ()
bankCompared retained owned = do
  left <- bankQualified RetainedRole retained
  right <- bankQualified OwnedRole owned
  () <$ compareRoles left right

bankArtifactBindings :: B.ByteString -> B.ByteString -> B.ByteString -> Bindings
bankArtifactBindings bytes elf names = composedBindings
  { bindLinked = bytes, bindElf = Just elf, bindSymbols = names
  , bindContract = case A.decodeStrict composedContractBytes of
      Just (Object fields) -> BL.toStrict (A.encode (Object (KM.union (KM.fromList
        [ "linked_sha256" .= digestOf bytes, "staged_image4_sha256" .= digestOf bytes
        , "elf_sha256" .= digestOf elf, "symbols_sha256" .= digestOf names ]) fields)))
      _ -> error "bank artifact contract fixture"
  }

bankCallerCases :: [(String,Bool)]
bankCallerCases =
  [ ("public qualification normalizes the bank apply caller but preserves raw LR", case (bankQualified RetainedRole retained,bankQualified OwnedRole owned) of
      (Right left,Right right) -> bankCompared retained owned == Right () && case (setter left,setter right) of
        (Just old,Just new) -> callReturn old == 0x630a04 && callReturn new == bankApplyBody + 8
          && case (callContext old,callContext new) of (BankApplyCaller _ _,BankApplyCaller _ _) -> True; _ -> False
        _ -> False
      _ -> False)
  , ("bank apply allows IRQ masking in task-mode critical sections", bankCompared retained (map (flags 0x93) owned) == Right ())
  , ("task SP may equal its allocation's lower boundary", bankCompared retained
      (traceAt 1 (sp 0x3000) (traceAt 2 (sp 0x3000) owned)) == Right ())
  , ("cached task SP is checked in physical stack bounds", bankCompared retained
      (traceAt 1 (sp 0x40003ff8) (traceAt 2 (sp 0x40003ff8) owned)) == Right ())
  , ("logical task endpoints remain valid callers", all (\ident ->
      let captured = map (wireField ["task","task_id"] (A.toJSON (ident::Word32)))
      in bankCompared (captured retained) (captured owned) == Right ()) [1,120])
  , ("within-role cached identity spellings denote the same task", bankCompared retained
      (traceAt 3 (wireField ["task","node"] (A.toJSON (hex 0x40990000))
        . wireField ["task","creation"] (A.toJSON (hex 0x40991000))
        . wireField ["task","stack","base"] (A.toJSON (hex 0x40003000))) owned) == Right ())
  , ("different logical task IDs do not compare as one bank caller", isLeft (bankCompared retained (map (wireField ["task","task_id"] (A.toJSON (6 :: Word32))) owned)))
  , ("different execution cores do not compare as one bank caller", isLeft (bankCompared retained (map coreB owned)))
  , ("matching core-B logical callers compare", bankCompared (map coreB retained) (map coreB owned) == Right ())
  , ("ordinary setters keep exact caller comparison", bankCompared
      (ordinary RetainedRole (setterCall + 4)) (ordinary OwnedRole (setterCall + 4)) == Right ()
      && isLeft (bankCompared (ordinary RetainedRole (setterCall + 4)) (ordinary OwnedRole (bankApplyBody + 20))))
  , ("public qualification refuses an ELF with the wrong property callee", refusedBound wrongCallee)
  , ("public qualification refuses missing sized bank apply metadata", refusedBound missingFunction)
  , ("public qualification refuses missing ELF evidence", refusedBound composedBindings {bindElf=Nothing})
  , ("a retained bank caller also requires its root entry", isLeft (bankQualified RetainedRole (drop 1 retained)))
  , ("retained caller LR must follow its actual property BL", isLeft (bankQualified RetainedRole
      (traceAt 1 (wireField ["lr"] (A.toJSON (hex 0x630a08))) retained)))
  ] ++ [(label,isLeft (bankQualified OwnedRole changed)) | (label,changed) <- negatives]
  where
    retained = bankHits RetainedRole
    owned = bankHits OwnedRole
    setter routesValue = case [call | route <- qualifiedRoutes routesValue, resultRoute route == crawlRoute, call <- resultSetters route] of
      [call] -> Just call
      _ -> Nothing
    flags :: Word32 -> Value -> Value
    flags value = wireField ["cpsr"] (A.toJSON value) . wireField ["task","cpsr"] (A.toJSON value)
    coreB = wireField ["core"] (A.toJSON (1 :: Int)) . wireField ["task","kernel"] (String "b")
    sp value = wireField ["sp"] (A.toJSON (hex value)) . wireField ["task","sp"] (A.toJSON (hex value))
    taskField path value = traceAt 1 (wireField ("task":path) value) owned
    unavailable reason = map (wireField ["task"] (object ["kind" .= ("unavailable"::T.Text),"reason" .= (reason::T.Text)])) owned
    ordinary role caller =
      let nested = if role == RetainedRole then 0x1fe0 else 0x3ff8
      in [taskHit role SetterEntry setterEntry [5,1] nested caller,taskHit role SetterReturn caller [0] nested 0]
    refusedBound bound = isLeft (do
      context <- admitBindings bound
      qualify context OwnedRole (reboundArtifactObservation bound (chainBound composedBindings OwnedRole [bankStep owned])))
    names = StorageBoundaries.bankApplySymbolBytes bankApplyFixture
    elf = StorageBoundaries.bankApplyElfBytes bankApplyFixture
    wrongWord = branchLink (bankApplyBody + 4) 0x624698
    wrongCallee = bankArtifactBindings (putWord (bankApplyBody + 4) wrongWord composedImage)
      (B.take 0x104 elf <> littleWords [wrongWord] <> B.drop 0x108 elf) names
    missingFunction = bankArtifactBindings composedImage elf B.empty
    negatives =
      [ ("bank caller without a root entry refuses", drop 1 owned)
      , ("bank caller without a root return refuses", take 3 owned)
      , ("bank caller without its setter return refuses", take 2 owned ++ drop 3 owned)
      , ("an unexpected root return refuses", owned ++ drop 3 owned)
      , ("a return to another root caller refuses", traceAt 3 (wireField ["address"] (A.toJSON (hex (bankApplyRootCall + 8)))) owned)
      , ("root LR must pair with its actual return", traceAt 0 (wireField ["lr"] (A.toJSON (hex (bankApplyRootCall + 8)))) owned)
      , ("a non-call instruction is not a bank caller", traceAt 1 (wireField ["lr"] (A.toJSON (hex (bankApplyBody + 12)))) owned)
      , ("a retained interior LR is not an owned bank caller", traceAt 1 (wireField ["lr"] (A.toJSON (hex 0x630a04))) owned)
      , ("a fixed veneer is not the owned root entry", traceAt 0 (wireField ["address"] (A.toJSON (hex 0x6309c4))) owned)
      , ("bank setter must belong to its root logical task", taskField ["task_id"] (A.toJSON (6::Word32)))
      , ("bank setter must belong to its root task node", taskField ["node"] (A.toJSON (hex 0x990004)))
      , ("bank setter must belong to its root task creation", taskField ["creation"] (A.toJSON (hex 0x991004)))
      , ("bank setter must use its root stack allocation", taskField ["stack","bytes"] (A.toJSON (0x1004::Word32)))
      , ("paired setter return must preserve task node", traceAt 2 (wireField ["task","node"] (A.toJSON (hex 0x990004))) owned)
      , ("paired setter return must preserve task creation", traceAt 2 (wireField ["task","creation"] (A.toJSON (hex 0x991004))) owned)
      , ("paired setter return must preserve task stack", traceAt 2 (wireField ["task","stack","bytes"] (A.toJSON (0x1004::Word32))) owned)
      , ("root return must preserve its task identity", traceAt 3 (wireField ["task","node"] (A.toJSON (hex 0x990004))) owned)
      , ("root return cannot migrate execution core", traceAt 3 coreB owned)
      , ("root return cannot migrate logical task", traceAt 3 (wireField ["task","task_id"] (A.toJSON (6::Word32))) owned)
      , ("paired setter return must remain in task execution", traceAt 2 (flags (0x12::Word32)) owned)
      , ("paired setter return must have zero context depth", traceAt 2 (wireField ["task","context_depth"] (A.toJSON (1::Word32))) owned)
      , ("root return must have zero context depth", traceAt 3 (wireField ["task","context_depth"] (A.toJSON (1::Word32))) owned)
      , ("IRQ execution is not task-mode execution", traceAt 1 (flags (0x12::Word32)) owned)
      , ("Thumb execution is not admitted A32 task execution", traceAt 1 (flags (0x33::Word32)) owned)
      , ("Jazelle state cannot project an A32 task caller", traceAt 1 (flags (0x01000013::Word32)) owned)
      , ("big-endian state cannot project a task caller", traceAt 1 (flags (0x213::Word32)) owned)
      , ("nonzero context depth cannot project a caller", taskField ["context_depth"] (A.toJSON (1::Word32)))
      , ("unaligned task SP cannot project a caller", traceAt 1 (sp 0x3ff9) owned)
      , ("task SP below the stack cannot project a caller", traceAt 1 (sp 0x2ffc) owned)
      , ("task SP beyond the stack cannot project a caller", traceAt 1 (sp 0x4004) owned)
      , ("nested flags must be the actual trace snapshot", taskField ["cpsr"] (A.toJSON (0x93::Word32)))
      , ("nested SP must be the actual trace snapshot", taskField ["sp"] (A.toJSON (hex 0x3ff4)))
      , ("captured task identity cannot omit the SDK capture", traceAt 1 (\value -> case value of Object fields -> Object (KM.delete "task" fields); _ -> error "task wire fixture") owned)
      , ("task kernel must agree with the trace core", taskField ["kernel"] (String "b"))
      , ("logical task zero is invalid", taskField ["task_id"] (A.toJSON (0::Word32)))
      , ("logical task 121 is invalid", taskField ["task_id"] (A.toJSON (121::Word32)))
      , ("task node cannot be null", taskField ["node"] (A.toJSON (hex 0)))
      , ("task creation cannot be null", taskField ["creation"] (A.toJSON (hex 0)))
      , ("task stack cannot be empty", taskField ["stack","bytes"] (A.toJSON (0::Word32)))
      , ("task stack cannot wrap the address space", taskField ["stack","base"] (A.toJSON (hex 0xfffffff0)))
      , ("no-current-task capture cannot project a caller", unavailable "no-current-task")
      , ("invalid-task-identity capture cannot project a caller", unavailable "invalid-task-identity")
      , ("unavailable-task-memory capture cannot project a caller", unavailable "unavailable-task-memory")
      , ("unknown unavailable reasons refuse", unavailable "unknown")
      , ("unavailable task captures reject extra identity fields", map (wireField ["task"] (object
          ["kind" .= ("unavailable"::T.Text),"reason" .= ("no-current-task"::T.Text),"task_id" .= (5::Word32)])) owned)
      ]

matrixSourceValue :: Value
matrixSourceValue = object
  [ "decomp_commit" .= replicate 40 'a', "catalog_commit" .= replicate 40 'b', "controls_commit" .= replicate 40 'c'
  , "image4_sha256" .= digestOf canonical4, "image5_sha256" .= digestOf (images M.! Image5) ]

matrixInput :: [Value] -> Value
matrixInput steps = object ["schema" .= (1::Int), "kind" .= ("settings-storage-lifecycle"::T.Text), "source" .= matrixSourceValue, "steps" .= steps]

matrixStep :: T.Text -> Value -> [Value] -> Value
matrixStep name action checks = object ["name" .= name, "action" .= action, "checks" .= checks]

matrixParserCases :: [(String,Bool)]
matrixParserCases =
  [ ("storage matrix preserves typed actions, source identities, and every check", case decode valid of
       Right matrix -> map Storage.storageStepAction (Storage.matrixSteps matrix)
           == [Storage.PressKey KeyOk, Storage.SetModeDial ModeDialCustom, Storage.ColdReboot]
         && Storage.sourceCommitHex (Storage.matrixDecompCommit (Storage.matrixSource matrix)) == replicate 40 'a'
         && Storage.storageMatrixValue matrix == valid
       Left _ -> False)
  , ("storage matrix preserves a half-press action", case decode (matrixInput [matrixStep "halfpress" (object ["kind" .= ("key"::T.Text),"key" .= ("half-press"::T.Text)]) []]) of
       Right matrix -> map Storage.storageStepAction (Storage.matrixSteps matrix) == [Storage.PressHalfShutter]
       Left _ -> False)
  , ("storage matrix rejects an empty workload", refused (matrixInput []))
  , ("storage matrix rejects duplicate step names", refused (matrixInput [save,save]))
  , ("storage matrix rejects a forward payload reference", refused (matrixInput [matrixStep "save" key [payload "reboot"],reboot]))
  , ("storage matrix rejects a self payload reference", refused (matrixInput [matrixStep "save" key [payload "save"]]))
  , ("storage matrix rejects a missing cold-boot reference", refused (matrixInput [save,matrixStep "reboot" cold [coldCheck "missing"]]))
  , ("storage matrix rejects a cold-boot check on a keypress", refused (matrixInput [save,matrixStep "verify" key [coldCheck "save"]]))
  , ("storage matrix rejects direct property injection", refused (matrixInput [matrixStep "inject" (object ["kind" .= ("set-property"::T.Text),"property" .= (0xee::Int),"value" .= (1::Int)]) []]))
  , ("storage matrix rejects an uncalibrated dial name", refused (matrixInput [matrixStep "dial" (object ["kind" .= ("mode-dial"::T.Text),"position" .= ("custom"::T.Text)]) []]))
  , ("storage matrix rejects unknown payload targets", refused (matrixInput [save,matrixStep "verify" key [object ["kind" .= ("custom-payload"::T.Text),"relation" .= ("same"::T.Text),"target" .= ("cache"::T.Text),"reference" .= reference "save"]]]))
  , ("storage matrix rejects unknown checks", refused (matrixInput [matrixStep "verify" key [object ["kind" .= ("assert-count"::T.Text),"count" .= (8::Int)]]]))
  , ("storage matrix rejects extra root fields", refused (extra "fallback" Null valid))
  , ("storage matrix rejects extra action fields", refused (matrixInput [matrixStep "save" (extra "property" (A.toJSON (0xee::Int)) key) []]))
  , ("storage matrix rejects extra check fields", refused (matrixInput [matrixStep "shift" key [extra "allow_missing" (Bool True) matrixCheck]]))
  , ("storage matrix rejects uppercase source commits", refused (extra "source" (extra "decomp_commit" (A.toJSON (replicate 40 'A')) matrixSourceValue) valid))
  , ("storage matrix rejects a malformed image digest", refused (extra "source" (extra "image4_sha256" (String "bad") matrixSourceValue) valid))
  , ("storage matrix rejects uppercase image digests", refused (extra "source" (extra "image5_sha256" (A.toJSON (replicate 64 'A')) matrixSourceValue) valid))
  , ("storage matrix rejects step-name punctuation", refused (matrixInput [matrixStep "save/profile" key []]))
  ]
  where
    decode = Storage.decodeStorageMatrix . BL.toStrict . A.encode
    refused = isLeft . decode
    extra name value (Object fields) = Object (KM.insert name value fields)
    extra _ _ _ = error "matrix object fixture"
    key = object ["kind" .= ("key"::T.Text),"key" .= ("ok"::T.Text)]
    matrixCheck = object ["kind" .= ("custom-commit"::T.Text),"change" .= ("changed"::T.Text)]
    dial = object ["kind" .= ("mode-dial"::T.Text),"position" .= ("C"::T.Text)]
    cold = object ["kind" .= ("cold-reboot"::T.Text)]
    reference step = object ["step" .= (step::T.Text),"target" .= ("active-bank"::T.Text)]
    payload step = object ["kind" .= ("custom-payload"::T.Text),"relation" .= ("same"::T.Text),"target" .= ("working-record"::T.Text),"reference" .= reference step]
    coldCheck step = object ["kind" .= ("cold-boot-record"::T.Text),"reference" .= (step::T.Text)]
    save = matrixStep "save" key
      [ object ["kind" .= ("custom-commit"::T.Text),"change" .= ("changed"::T.Text)]
      , object ["kind" .= ("bank-copy"::T.Text),"family" .= ("previous"::T.Text),"direction" .= ("capture"::T.Text)]
      , object ["kind" .= ("current-slot"::T.Text),"slot" .= ("live"::T.Text)]
      , object ["kind" .= ("ui-handback"::T.Text)]
      , object ["kind" .= ("repeat-setting"::T.Text),"setting" .= ("film-simulation"::T.Text)]
      ]
    custom = matrixStep "custom" dial
      [ payload "save"
      , object ["kind" .= ("custom-commit"::T.Text),"change" .= ("unchanged"::T.Text)]
      , object ["kind" .= ("bank-copy"::T.Text),"family" .= ("alternate"::T.Text),"direction" .= ("restore"::T.Text)]
      , object ["kind" .= ("current-slot"::T.Text),"slot" .= ("custom"::T.Text)]
      , object ["kind" .= ("repeat-setting"::T.Text),"setting" .= ("white-balance-shift"::T.Text)]
      ]
    reboot = matrixStep "reboot" cold [coldCheck "save"]
    valid = matrixInput [save,custom,reboot]


flippedByte :: Int -> B.ByteString -> B.ByteString
flippedByte index bytes = B.take index bytes <> B.singleton (B.index bytes index `xor` 1) <> B.drop (index+1) bytes

sourceModeCases :: [(String,Bool)]
sourceModeCases =
  [ ("source-mode history at byte8 is typed and serialized",case LocalUi.parseLocalUiMode history of
      Right mode -> A.toJSON (LocalUi.previousPrevious mode) == A.toJSON (5::Word32)
        && case A.toJSON mode of
          Object fields -> KM.lookup "previous_previous" fields == Just (A.toJSON (5::Word32))
          _ -> False
      Left _ -> False)
  ] ++
  [ let bytes=flippedByte index baselineBytes
    in ("authentic source-mode byte " ++ show index ++ " survives raw view and report",case LocalUi.parseLocalUiMode (uiViewBytes 0x9500ec bytes) of
      Right mode -> A.toJSON (LocalUi.sourceModeView mode) == A.toJSON (B.unpack bytes)
        && case A.toJSON mode of
          Object fields -> KM.lookup "raw_source_mode" fields == Just (A.toJSON (B.unpack bytes))
          _ -> False
      Left _ -> False)
  | index <- [0..39]
  ] ++
  [ ("uninterpreted source-mode byte " ++ show index ++ " is preserved without inventing semantic inequality"
    ,case (LocalUi.parseLocalUiMode baseline,LocalUi.parseLocalUiMode (uiViewBytes 0x9500ec (flippedByte index baselineBytes))) of
      (Right original,Right changed) -> original == changed
        && LocalUi.sourceModeView original /= LocalUi.sourceModeView changed && A.toJSON original /= A.toJSON changed
      _ -> False)
  | index <- [16..19] ++ [24..35] ++ [37..39]
  ] ++
  [ ("source-mode alias " ++ show (row,column) ++ " cannot disagree"
    ,isLeft (LocalUi.parseLocalUiMode (wireField ["spellings"] (A.toJSON
      [[if (r,c)==(row,column) then String "00" else rawBytes | c <- [0::Int,1]] | r <- [0::Int,1]]) baseline)))
  | row <- [0::Int,1],column <- [0::Int,1]
  ] ++
  [ ("source-mode parsing has no missing-view fallback",isLeft (LocalUi.parseLocalUiMode (object [])))
  , ("source-mode parsing requires its physical bytes",isLeft (LocalUi.parseLocalUiMode (without "physical")))
  , ("source-mode parsing requires all alias captures",isLeft (LocalUi.parseLocalUiMode (without "spellings")))
  , ("source-mode parsing requires exactly40 authentic bytes",isLeft (LocalUi.parseLocalUiMode (uiViewBytes 0x9500ec (B.take 39 baselineBytes))))
  ]
  where
    baselineBytes=littleWords [3,0,0,0,0,0,0,0,0,0]
    baseline=uiViewBytes 0x9500ec baselineBytes
    history=uiView 0x9500ec [3,4,5,6,0,7,0,0,0,1]
    rawBytes=case baseline of Object fields -> fromMaybe (error "mode fixture has no physical bytes") (KM.lookup "physical" fields); _ -> error "mode fixture is not an object"
    without key=case baseline of Object fields -> Object (KM.delete key fields); _ -> error "mode fixture is not an object"

reboundArtifactObservation :: Bindings -> Observation -> Observation
reboundArtifactObservation bound sourceObservation = sourceObservation {obsReceipt=BL.toStrict (A.encode receipt)}
  where
    receipt=foldl' (\value (path,fieldValue) -> wireField path fieldValue value) original
      [ (["inputs","contract","sha256"],A.toJSON (digestOf (bindContract bound)))
      , (["inputs","linked","sha256"],A.toJSON (digestOf (bindLinked bound)))
      , (["inputs","symbols","sha256"],A.toJSON (digestOf (bindSymbols bound)))
      , (["inputs","elf"],maybe Null (\bytes -> object ["sha256" .= digestOf bytes]) (bindElf bound))
      , (["inputs","role_image4_sha256"],A.toJSON (digestOf (bindLinked bound)))
      ]
    original=case A.decodeStrict (obsReceipt sourceObservation) of Just value -> value; Nothing -> error "artifact fixture receipt is invalid"

bankProofBindingCases :: [(String,Bool)]
bankProofBindingCases =
  [ ("composed " ++ roleWire role ++ " planning requires its admitted bank ELF proof"
    ,case fixturePlanInputs role composedBindings of
      Right inputs -> isLeft (derivePlan inputs {inputBankBoundary=Nothing})
      Left _ -> False)
  | role <- [RetainedRole,OwnedRole]
  ] ++
  [ ("a legacy plan cannot borrow a composed bank proof",case (fixturePlanInputs OwnedRole bindings,composedInputs) of
      (Right legacy,Right composed) -> isLeft (derivePlan legacy {inputBankBoundary=inputBankBoundary composed})
      _ -> False)
  , ("a bank proof is bound to the complete raw ELF", staleProof (bankArtifactBindings composedImage (elf <> B.singleton 0) names))
  , ("a bank proof is bound to complete raw nm bytes", staleProof (bankArtifactBindings composedImage elf
      (names <> "00600000 00000000 A unreferenced_bank_alias\n")))
  , ("a bank proof cannot enlarge the publication's permitted carriers",case composedInputs of
      Right inputs -> case StorageBoundaries.admitBankApplyElf bankApplyFixture
        { StorageBoundaries.bankApplyImageBase=Placement.GuestAddress image4Base
        , StorageBoundaries.bankApplyLinkedBytes=composedImage
        , StorageBoundaries.bankApplyCarriers=[Placement.Carrier (Placement.GuestAddress composedCodeStart) (Placement.GuestAddress (composedCodeStart+0x224))]
        } of
          Right proof -> isLeft (derivePlan inputs {inputBankBoundary=Just proof})
          Left _ -> False
      Left _ -> False)
  ]
  where
    elf=StorageBoundaries.bankApplyElfBytes bankApplyFixture
    names=StorageBoundaries.bankApplySymbolBytes bankApplyFixture
    staleProof bound=case (composedInputs,fixturePlanInputs OwnedRole bound) of
      (Right original,Right changed) -> not (isLeft (derivePlan changed))
        && isLeft (derivePlan changed {inputBankBoundary=inputBankBoundary original})
      _ -> False
