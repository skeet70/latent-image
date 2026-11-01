{-# LANGUAGE OverloadedStrings #-}
module Emulation.SaveLifecycle
  ( Source, Program, Snapshot, Observer, Registration, CallerCode (..), Refusal (..), RefusalCategory (..)
  , deriveSource, parseProgram, sourceValue, scopeValue, traceSitesValue, validateCallerCode, validateBegin
  , traceRegistrations, traceSiteAddress, traceSiteLabel
  , parseSnapshot, initialObserver, consumePage, consumePageWithMetadata, observeSnapshot, observationValue
  , replayJournal, refusalCategory
  ) where

import Control.Monad (foldM, forM, forM_, unless)
import Data.Aeson (FromJSON, Value (..), object, (.=))
import qualified Data.Aeson as J
import qualified Data.Aeson.Key as K
import qualified Data.Aeson.KeyMap as KM
import qualified Data.Aeson.Types as JT
import Data.Bits (shiftL, shiftR, (.&.), (.|.))
import qualified Data.ByteString as B
import Data.List (nub)
import qualified Data.Map.Strict as M
import Data.Maybe (catMaybes)
import qualified Data.Text as T
import Data.Word (Word8, Word32, Word64)
import qualified Emulation.CaptureStream as C
import Emulation.Sha256 (sha256, sha256Hex)
import qualified StockInventory as SI

newtype Address = Address Word32 deriving (Eq, Ord, Show)
newtype Slot = Slot Word32 deriving (Eq, Show)
data ReleaseArgument = ValidRelease Slot | InvalidRelease Word32 deriving (Eq, Show)
newtype Count = Count Word32 deriving (Eq, Show)
newtype Frame = Frame Address deriving (Eq, Show)
newtype Opcode = Opcode Word32 deriving (Eq, Show)
newtype Tick = Tick Word64 deriving (Eq, Ord, Show)
newtype Task = Task Word32 deriving (Eq, Show)
newtype Callback = Callback Address deriving (Eq, Show)
data Ownership = RequireAllocated | ObserveOwnership deriving (Eq)
data ProgramPath = StockPath | OwnedPath deriving (Eq)
data SaveKind = Kind0 | Kind1 | Kind2 | Kind3 | Kind4 | OtherKind deriving (Eq)
saveKind :: Word32 -> SaveKind
saveKind word = case word of 0 -> Kind0; 1 -> Kind1; 2 -> Kind2; 3 -> Kind3; 4 -> Kind4; _ -> OtherKind
newtype CatalogueMode = CatalogueMode Word32 deriving (Eq, Show)
newtype CompletionCode = CompletionCode Word32 deriving (Eq, Show)
data CatalogueRow = CatalogueRow CatalogueMode Count Address Address deriving (Eq, Show)
data Site
  = AllocatorEntry | AllocatorReturn | ProducerEntry | SlotPublication | FramePublication
  | InitializationCall | ProducerReturn | PopulationEntry | PopulationAllocated | PopulationReturn
  | InitializerEntry | InitializerReturn | ReleaseEntry | ReleaseClear | ReleaseReturn | ReleaseInvalid
  | SaveCaller | SaveContinuation | SaveEntry | SaveCompletion | ReleaseCompletion
  | SaveStatusCommitted | SaveSignal | ReleaseSignal
  | PermitCallerA | PermitCallerB | PermitEntry | Resolver
  | StorageDirect | StorageThree | ReleaseRegistration
  deriving (Eq, Ord, Show, Enum, Bounded)
data Source = Source [CatalogueRow] (M.Map Site (Address, Opcode))
data Owned = Owned Address Address Address Address Address Address B.ByteString
  deriving (Eq)
data Program = Retained | OwnedProgram Owned deriving (Eq)
data CallerCode = CallerCode
  { savePublicationCode :: B.ByteString
  , permitPublicationACode :: B.ByteString
  , permitPublicationBCode :: B.ByteString
  }
data RefusalCategory
  = WireMismatch | SourceMismatch | ProgramMismatch | MissingContext | MissingEvidence
  | CatalogueMismatch | RootMismatch | NamespaceMismatch | SelectedCountMismatch
  | FramePointerMismatch | OwnershipMismatch | MappingMismatch | OpcodeMismatch
  | InitializedFieldsMismatch | PopulatedFieldsMismatch | CallbackTaskMismatch
  | OutputCellMismatch | JournalMismatch | EvidenceBoundExceeded
  deriving (Eq, Ord, Show)
data Refusal = Refusal RefusalCategory String deriving (Eq, Show)
refusalCategory :: Refusal -> RefusalCategory
refusalCategory (Refusal category _) = category

refuse :: RefusalCategory -> String -> Either Refusal a
refuse category = Left . Refusal category
require :: RefusalCategory -> Bool -> String -> Either Refusal ()
require category condition message = unless condition (refuse category message)
wire :: FromJSON a => Value -> Either Refusal a
wire = either (refuse WireMismatch) Right . JT.parseEither J.parseJSON
field :: FromJSON a => K.Key -> Value -> Either Refusal a
field key (Object row) = maybe (refuse WireMismatch ("missing field " ++ K.toString key)) wire (KM.lookup key row)
field _ _ = refuse WireMismatch "expected object"
optional :: FromJSON a => K.Key -> Value -> Either Refusal (Maybe a)
optional key (Object row) = traverse wire (KM.lookup key row)
optional _ _ = refuse WireMismatch "expected object"
addressValue :: Address -> Word32
addressValue (Address address) = address
frameValue :: Frame -> Word32
frameValue (Frame address) = addressValue address

imageBase :: Word32
imageBase = 0xbf1a00
canonicalDigest :: String
canonicalDigest = "3cd501c07533938ae4d9347616986fd1649005a588c36b5abf05e36ef4fd892a"
wordAt :: B.ByteString -> Int -> Either Refusal Word32
wordAt bytes offset = do
  require MissingEvidence (offset >= 0 && offset + 4 <= B.length bytes) "word leaves supplied bytes"
  pure (foldr (\index value -> (value `shiftL` 8) .|. fromIntegral (B.index bytes (offset + index))) 0 [0..3])
sourceWord :: B.ByteString -> Word32 -> Either Refusal Word32
sourceWord bytes address = do
  require SourceMismatch (address >= imageBase) "source address precedes image5"
  wordAt bytes (fromIntegral (address - imageBase))

stockSites :: [(Site, Word32)]
stockSites =
  [ (AllocatorEntry,0xe88d88),(AllocatorReturn,0xe88e18)
  , (ProducerEntry,0xe80e10),(SlotPublication,0xe80e3c),(FramePublication,0xe80e44)
  , (InitializationCall,0xe80e78),(ProducerReturn,0xe80e7c),(PopulationEntry,0xe80e84)
  , (PopulationAllocated,0xe80ea0),(PopulationReturn,0xe81a24)
  , (InitializerEntry,0xe4e830),(InitializerReturn,0xe4eca4)
  , (ReleaseEntry,0xe87d80),(ReleaseClear,0xe87dc0),(ReleaseReturn,0xe87dc4),(ReleaseInvalid,0xe87dd0)
  , (SaveCaller,0xe99e8c),(SaveContinuation,0xe99e90),(SaveEntry,0xe518c0)
  , (SaveCompletion,0xe4e5dc),(ReleaseCompletion,0xe4e5a8)
  , (SaveStatusCommitted,0xe4e618),(SaveSignal,0xe4e63c),(ReleaseSignal,0xe4e5d4)
  , (PermitCallerA,0xe6a754),(PermitCallerB,0xe6bb90),(PermitEntry,0xe7e95c),(Resolver,0xe77168)
  , (StorageDirect,0x11d5600),(StorageThree,0x11d5688),(ReleaseRegistration,0x11d5714)
  ]
siteName :: Site -> T.Text
siteName site = "save-lifecycle-" <> T.pack (show site)

deriveSource :: B.ByteString -> B.ByteString -> Either Refusal Source
deriveSource bytes plain = do
  require SourceMismatch (B.length bytes == 5185120 && sha256Hex (sha256 bytes) == canonicalDigest)
    "canonical decoded image5 required"
  require SourceMismatch (B.length plain <= 8 * 1024 * 1024
    && sha256Hex (sha256 plain) == "ee211bdaad39005b4f72e3443a76ca194905381a967cab787d6a4ef14e2d889d")
    "canonical decoded image6 required"
  _ <- either (refuse SourceMismatch) Right (SI.checkedStockFunctions bytes)
  rows <- forM [0..12] $ \index -> do
    words' <- mapM (\offset -> sourceWord bytes (0xe8a338 + index*16 + offset)) [0,4,8,12]
    case words' of
      [mode,count,base,aux] -> do
        require SourceMismatch (count > 0 && count <= 201) "source catalogue count leaves namespace"
        pure (CatalogueRow (CatalogueMode mode) (Count count) (Address base) (Address aux))
      _ -> refuse SourceMismatch "source catalogue row width differs"
  sentinel <- sourceWord bytes 0xe8a408
  require SourceMismatch (sentinel == 12 && length (nub [mode | CatalogueRow mode _ _ _ <- rows]) == 13)
    "source catalogue sentinel or modes differ"
  sites <- forM stockSites $ \(site,address) -> do
    opcode <- if address >= 0x1100000
      then wordAt plain (fromIntegral (address-0x1100000))
      else sourceWord bytes address
    pure (site,(Address address,Opcode opcode))
  pure (Source rows (M.fromList sites))

parseProgram :: Source -> B.ByteString -> Value -> Either Refusal Program
parseProgram (Source _ stock) runtime metadata = do
  kind <- field "kind" metadata
  case (kind :: T.Text) of
    "retained" -> do
      require ProgramMismatch (B.null runtime) "retained metadata must not carry an owned runtime image"
      pure Retained
    "owned" -> do
      extent <- field "image" metadata
      (low,high) <- case extent :: [Word32] of
        [low,high] -> pure (low,high)
        _ -> refuse ProgramMismatch "owned image requires an exact half-open extent"
      require ProgramMismatch (low < high && low .&. 3 == 0 && high .&. 3 == 0
        && high - low <= 0x40000 && fromIntegral (high - low) == B.length runtime)
        "owned runtime bytes disagree with bounded image extent"
      _ <- physical (Context 0 0 0 0 0) (Address low) (fromIntegral (high-low))
      addresses <- mapM (\key -> Address <$> field key metadata)
        ["save_entry","save_completion","release_completion","permit_entry"]
      require ProgramMismatch (length (nub addresses) == 4 && all (\(Address a) -> a >= low && a + 4 <= high && a .&. 3 == 0) addresses)
        "owned symbols must be distinct aligned runtime instructions"
      require ProgramMismatch (all (\address -> address `notElem` map fst (M.elems stock)) addresses)
        "owned symbols overlap source trace instructions"
      forM_ addresses $ \(Address address) -> do
        opcode <- wordAt runtime (fromIntegral (address-low))
        require ProgramMismatch (opcode /= 0 && opcode /= 0xffffffff) "owned symbol has no runtime instruction"
      case addresses of
        [entry,completion,release,permit] -> do
          forM_ [(0xe99e8c,entry),(0xe6a754,permit),(0xe6bb90,permit)] $ \(pc,target) -> do
            _ <- branchWord (Address pc) target
            pure ()
          pure (OwnedProgram (Owned (Address low) (Address high) entry completion release permit runtime))
        _ -> refuse ProgramMismatch "owned symbol arity differs"
    _ -> refuse ProgramMismatch "unsupported lifecycle program kind"

programValue :: Program -> Value
programValue Retained = object ["kind" .= ("retained" :: T.Text)]
programValue (OwnedProgram (Owned low high entry completion release permit _)) = object
  ["kind" .= ("owned" :: T.Text),"image" .= map addressValue [low,high]
  ,"save_entry" .= addressValue entry,"save_completion" .= addressValue completion
  ,"release_completion" .= addressValue release,"permit_entry" .= addressValue permit]
scopeValue :: Program -> Value
scopeValue program = object ["kind" .= ("save-lifecycle" :: T.Text),"program" .= programValue program]

validateBegin :: Program -> Value -> Either Refusal ()
validateBegin program value = do
  either (refuse JournalMismatch) pure (C.validateBegin value)
  scope <- field "scope" value
  require ProgramMismatch (scope == scopeValue program) "capture begin changed the lifecycle scope"

sourceValue :: Source -> Value
sourceValue (Source rows sites) = object
  ["image5_sha256" .= canonicalDigest,"catalogue_address" .= (0xe8a338 :: Word32)
  ,"image6_sha256" .= ("ee211bdaad39005b4f72e3443a76ca194905381a967cab787d6a4ef14e2d889d" :: String)
  ,"catalogue" .= [J.toJSON [mode,count,addressValue base,addressValue aux] | CatalogueRow (CatalogueMode mode) (Count count) base aux <- rows]
  ,"stride" .= (0x326c :: Word32),"namespace" .= (201 :: Word32)
  ,"sites" .= [object ["site" .= siteName site,"address" .= addressValue address,"word" .= opcode]
    | (site,(address,Opcode opcode)) <- M.toList sites]]

branchWord :: Address -> Address -> Either Refusal Opcode
branchWord pc@(Address address) target@(Address destination) = do
  let delta = toInteger destination - toInteger address - 8
  require ProgramMismatch (delta `mod` 4 == 0 && delta >= -0x2000000 && delta <= 0x1fffffc)
    "owned caller target leaves A32 BL range"
  pure (branchOpcode pc target)
branchOpcode :: Address -> Address -> Opcode
branchOpcode (Address pc) (Address target) =
  Opcode (0xeb000000 .|. (fromInteger ((toInteger target-toInteger pc-8) `div` 4) .&. 0xffffff))
registrations :: Source -> Program -> [(Site,Address,Opcode)]
registrations (Source _ sites) program = stock ++ owned
  where
    stock = [(site,address,expected site address opcode) | (site,(address,opcode)) <- M.toList sites]
    expected site address opcode = case (program,site) of
      (OwnedProgram (Owned _ _ entry _ _ _ _),SaveCaller) -> branchOpcode address entry
      (OwnedProgram (Owned _ _ _ _ _ permit _),PermitCallerA) -> branchOpcode address permit
      (OwnedProgram (Owned _ _ _ _ _ permit _),PermitCallerB) -> branchOpcode address permit
      _ -> opcode
    owned = case program of
      Retained -> []
      OwnedProgram (Owned low _ entry completion release permit bytes) ->
        [(site,address,Opcode (runtimeWord bytes (fromIntegral (addressValue address-addressValue low))))
        | (site,address) <- [(SaveEntry,entry),(SaveCompletion,completion),(ReleaseCompletion,release),(PermitEntry,permit)]]
    runtimeWord bytes offset =
      foldr (\index value -> (value `shiftL` 8) .|. fromIntegral (B.index bytes (offset+index))) 0 [0..3]
data Registration = Registration Address T.Text

traceRegistrations :: Source -> Program -> [Registration]
traceRegistrations source program =
  [Registration address (label site address) | (site,address,_) <- registrations source program]
  where
    label site address = siteName site <> if lookup site stockSites == Just (addressValue address) then "-stock" else "-owned"

traceSiteAddress :: Registration -> Word32
traceSiteAddress (Registration address _) = addressValue address

traceSiteLabel :: Registration -> String
traceSiteLabel (Registration _ label) = T.unpack label

traceSitesValue :: Source -> Program -> Value
traceSitesValue source program = J.toJSON
  [object ["address" .= traceSiteAddress site,"site" .= traceSiteLabel site] | site <- traceRegistrations source program]

validateCallerCode :: Source -> Program -> CallerCode -> Either Refusal ()
validateCallerCode source program code = do
  let bindings = registrations source program
  forM_ [(SaveCaller,savePublicationCode code),(PermitCallerA,permitPublicationACode code),(PermitCallerB,permitPublicationBCode code)] $ \(site,bytes) -> do
    expected <- case [opcode | (registered,_,opcode) <- bindings, registered == site] of
      [opcode] -> pure opcode
      _ -> refuse SourceMismatch "caller publication source is not unique"
    require ProgramMismatch (B.length bytes == 4) "caller publication sample must contain one instruction"
    actual <- Opcode <$> wordAt bytes 0
    require ProgramMismatch (actual == expected) "live caller publication differs from the selected lifecycle program"

-- Complete RAM spans contain physical-address bytes.
data Context = Context Word32 Word32 Word32 Word32 Word32 deriving (Eq, Show)
data RamStatus = Complete | OutsideRam | ReadFailed | ByteLimit deriving (Eq, Show)
data Span = Span Address Int RamStatus B.ByteString deriving (Eq, Show)
data Sample = Sample Tick Context [Span] deriving (Eq, Show)
data Snapshot = Snapshot Sample [CatalogueRow] deriving (Eq, Show)
data Event = Event Site Address [Word32] Sample deriving (Eq, Show)

context :: Value -> Either Refusal Context
context value = do
  lifecycle <- optional "lifecycle" value
  raw <- maybe (refuse MissingContext "core0 lifecycle context unavailable") pure lifecycle
  r4 <- field "r4" raw
  r5 <- field "r5" raw
  r11 <- field "r11" raw
  banks <- field "tcm" raw
  case banks :: [Word32] of
    [atcm,btcm] -> do
      _ <- tcmWindow atcm
      _ <- tcmWindow btcm
      pure (Context r4 r5 r11 atcm btcm)
    _ -> refuse MissingContext "core0 lifecycle TCM requires two registers"
tcmWindow :: Word32 -> Either Refusal (Maybe (Word64,Word64))
tcmWindow register
  | register .&. 1 == 0 = pure Nothing
  | otherwise = do
      let size = (512 :: Word64) `shiftL` fromIntegral ((register `shiftR` 2) .&. 31)
          base = fromIntegral (register .&. 0xfffff000)
      require MappingMismatch (size >= 0x1000 && size <= 0x800000 && base `mod` size == 0 && base + size <= 0x100000000)
        "unsupported TCM size or alignment"
      pure (Just (base,base+size))
physical :: Context -> Address -> Int -> Either Refusal Address
physical (Context _ _ _ atcm btcm) (Address address) size = do
  let start = fromIntegral address :: Word64
      end = start + fromIntegral size
      alias = address .&. 0xc0000000
      offset = address - alias
  require MappingMismatch (size > 0 && alias `elem` [0,0x40000000] && end <= 0x100000000
    && toInteger offset + toInteger size <= 0x20000000) "unsupported DRAM range or alias"
  forM_ [atcm,btcm] $ \register -> do
    window <- tcmWindow register
    case window of
      Nothing -> pure ()
      Just (low,high) -> require MappingMismatch (end <= low || start >= high) "sampled guest range intersects recorded TCM"
  pure (Address offset)

hexBytes :: T.Text -> Either Refusal B.ByteString
hexBytes text = do
  require WireMismatch (even (T.length text) && T.all (`elem` ("0123456789abcdef" :: String)) text) "invalid lowercase RAM hex"
  pure (B.pack (go (T.unpack text)))
  where
    nibble c = fromIntegral (if c <= '9' then fromEnum c - fromEnum '0' else fromEnum c - fromEnum 'a' + 10) :: Word8
    go (a:b:rest) = (nibble a * 16 + nibble b) : go rest
    go _ = []
parseSpan :: Value -> Either Refusal Span
parseSpan raw = do
  address <- Address <$> field "address" raw
  requested <- field "requested" raw
  require EvidenceBoundExceeded (requested > (0 :: Int) && requested <= 65535) "RAM span request exceeds bound"
  status <- field "status" raw >>= \value -> case (value :: T.Text) of
    "complete" -> pure Complete
    "outside-ram" -> pure OutsideRam
    "read-failed" -> pure ReadFailed
    "byte-limit" -> pure ByteLimit
    _ -> refuse WireMismatch "unknown RAM span status"
  text <- field "bytes" raw
  require EvidenceBoundExceeded (T.length text <= 1024) "RAM span bytes exceed per-sample bound"
  bytes <- hexBytes text
  require WireMismatch (B.length bytes <= requested && (status /= Complete || B.length bytes == requested)) "RAM span bytes disagree with status"
  pure (Span address requested status bytes)
parseSample :: Value -> Either Refusal Sample
parseSample raw = do
  core <- field "core" raw
  require WireMismatch (core == (0 :: Word32)) "lifecycle evidence must identify core0"
  tick <- Tick <$> field "instructions" raw
  ctx <- context raw
  spans <- field "ram" raw >>= mapM parseSpan
  incomplete <- field "ram_incomplete" raw
  require EvidenceBoundExceeded (length spans <= 24
    && sum [B.length bytes | Span _ _ _ bytes <- spans] <= 512) "lifecycle RAM exceeds bounded sample"
  require MissingEvidence (not incomplete && all (\(Span _ _ status _) -> status == Complete) spans) "lifecycle RAM evidence is incomplete"
  forM_ spans $ \(Span address size _ _) -> do
    require MappingMismatch (toInteger (addressValue address) + toInteger size <= 0x20000000) "RAM span is not physical DRAM"
  forM_ (zip [0 :: Int ..] spans) $ \(index,Span address size _ bytes) ->
    forM_ (drop (index+1) spans) $ \(Span other otherSize _ otherBytes) -> do
      let low = max (addressValue address) (addressValue other)
          high = min (toInteger (addressValue address) + toInteger size) (toInteger (addressValue other) + toInteger otherSize)
      if toInteger low >= high then pure () else do
        let count = fromInteger (high-toInteger low)
        require WireMismatch (B.take count (B.drop (fromIntegral (low-addressValue address)) bytes)
          == B.take count (B.drop (fromIntegral (low-addressValue other)) otherBytes)) "overlapping RAM spans disagree"
  pure (Sample tick ctx spans)
readBytes :: Sample -> Address -> Int -> Either Refusal B.ByteString
readBytes (Sample _ ctx spans) guest size = do
  Address start <- physical ctx guest size
  chunks <- collect start size
  pure (case chunks of [bytes] -> bytes; _ -> B.concat chunks)
  where
    collect _ 0 = pure []
    collect at remaining = case
      [(start,count,bytes) | Span (Address start) count Complete bytes <- spans,
        at >= start && toInteger at < toInteger start + toInteger count] of
      (start,count,bytes):_ -> do
        let offset = fromIntegral (at-start)
            width = min remaining (count-offset)
            chunk = B.take width (B.drop offset bytes)
        rest <- collect (at+fromIntegral width) (remaining-width)
        pure (chunk:rest)
      [] -> refuse MissingEvidence ("missing physical RAM at " ++ show at)
readWord :: Sample -> Address -> Either Refusal Word32
readWord sample address = readBytes sample address 4 >>= (`wordAt` 0)

selectRow :: Source -> Sample -> Either Refusal CatalogueRow
selectRow (Source rows _) sample = do
  root <- Address <$> readWord sample (Address 0x157dd3c)
  let selected = [row | row@(CatalogueRow _ _ base _) <- rows, base == root]
  case selected of
    row:_ -> pure row
    [] -> refuse RootMismatch "actual frame root is absent from source catalogue"
slotFrame :: Source -> Sample -> Word32 -> Either Refusal (Slot,Frame)
slotFrame source sample index = do
  require NamespaceMismatch (index < 201) "frame slot leaves 201-entry namespace"
  CatalogueRow _ (Count count) (Address root) _ <- selectRow source sample
  require SelectedCountMismatch (index < count) "frame slot leaves selected catalogue backing count"
  let pointer = toInteger root + toInteger index * 0x326c
  require FramePointerMismatch (pointer + 0x326c <= 0x100000000) "derived frame wraps address space"
  let frame = Frame (Address (fromInteger pointer))
  _ <- physical (sampleContext sample) (Address (frameValue frame)) 0x326c
  pure (Slot index,frame)
sampleContext :: Sample -> Context
sampleContext (Sample _ ctx _) = ctx
pointerFrame :: Source -> Sample -> Word32 -> Either Refusal (Slot,Frame)
pointerFrame source sample pointer = do
  CatalogueRow _ _ (Address root) _ <- selectRow source sample
  require FramePointerMismatch (pointer >= root && (pointer-root) `mod` 0x326c == 0) "frame pointer differs from selected-root stride"
  pair@(_,frame) <- slotFrame source sample ((pointer-root) `div` 0x326c)
  require FramePointerMismatch (frameValue frame == pointer) "frame pointer differs from derived frame"
  pure pair
requirePointer :: Frame -> Word32 -> Either Refusal ()
requirePointer frame pointer = require FramePointerMismatch (frameValue frame == pointer) "actual frame pointer differs from independently derived frame"

frameFields :: [Word32]
frameFields = [0x78,0x4a8,0x578,0x5f0,0x604,0x275c,0x27c8,0x27cc,0x2fa0,0x2fb0,0x3260,0x3264,0x3268]
frameWord :: Sample -> Frame -> Word32 -> Either Refusal Word32
frameWord sample frame offset = readWord sample (Address (frameValue frame+offset))
frameEvidence :: Sample -> Frame -> Ownership -> Maybe SaveKind -> Either Refusal Value
frameEvidence sample frame ownership selectedKind = do
  let offsets = case selectedKind of
        Nothing -> frameFields
        Just _ -> [0x578,0x5f0,0x604,0x275c,0x3260,0x3264,0x3268]
  values <- mapM (frameWord sample frame) offsets
  marker <- frameWord sample frame 0x3268
  require OwnershipMismatch (marker `elem` [0,1] && (ownership == ObserveOwnership || marker == 1)) "frame allocation marker disagrees with observed operation"
  name <- readBytes sample (Address (frameValue frame+0x2f64)) 0x30
  tails <- case selectedKind of
    Just _ -> pure []
    Nothing -> forM [0..2] $ \bank -> do
      words' <- mapM (\offset -> frameWord sample frame (0x27d0 + bank*0x150 + offset)) [4,0x148,0x14c]
      pure (object ["bank" .= bank,"mode_and_tails" .= words'])
  descriptor <- case selectedKind of
    Nothing -> pure Nothing
    Just kind -> do
      let bank = case kind of Kind1 -> 1; Kind2 -> 2; Kind3 -> 2; Kind4 -> 2; _ -> 0
      bytes <- readBytes sample (Address (frameValue frame+0x27d0+bank*0x150)) 0x150
      pure (Just (object ["bank" .= bank,"bytes" .= B.unpack bytes]))
  pure (object ["frame" .= frameValue frame,"fields" .= values,"field_offsets" .= offsets
    ,"extent_name_bytes" .= B.unpack name,"descriptors" .= tails,"selected_descriptor" .= descriptor])

globalAddresses :: [Word32]
globalAddresses = [0x157dd3c,0x1581f50,0x15ad53c,0x154d970,0x155ccb8,0x155ccbc,0x14ddc1c,0x14ddc20,0x14ddc24,0x14ddbd8,0x1581f60,0x1581f64,0x1581f68]
globals :: Sample -> Either Refusal Value
globals sample = do
  words' <- mapM (readWord sample . Address) globalAddresses
  pure (object ["addresses" .= globalAddresses,"words" .= words'])
parseSnapshot :: Source -> Program -> Value -> Either Refusal Snapshot
parseSnapshot source@(Source rows _) _ raw = do
  sample <- parseSample raw
  decoded <- forM [0..12] $ \index -> do
    words' <- mapM (\offset -> readWord sample (Address (0xe8a338 + index*16 + offset))) [0,4,8,12]
    case words' of
      [mode,count,base,aux] -> pure (CatalogueRow (CatalogueMode mode) (Count count) (Address base) (Address aux))
      _ -> refuse CatalogueMismatch "snapshot catalogue row width differs"
  require CatalogueMismatch (decoded == rows) "live catalogue differs from canonical image5"
  _ <- selectRow source sample
  _ <- globals sample
  pure (Snapshot sample decoded)

parseEvent :: Source -> Program -> Value -> Either Refusal Event
parseEvent source program raw = do
  address <- Address <$> field "address" raw
  label <- field "site" raw
  let sites = registrations source program
      labels = [(site,pc,opcode,siteName site <> if lookup site stockSites == Just (addressValue pc) then "-stock" else "-owned") | (site,pc,opcode) <- sites]
  (site,expected) <- case [(site,opcode) | (site,pc,opcode,name) <- labels,pc == address,name == label] of
    [binding] -> pure binding
    _ -> refuse SourceMismatch "trace address or label is not registered by source and actual program"
  sample <- parseSample raw
  sampled <- Opcode <$> readWord sample address
  require OpcodeMismatch (sampled == expected) "sampled PC word differs from source or actual owned runtime target"
  registers <- field "registers" raw
  require WireMismatch (length (registers :: [Word32]) == 4) "lifecycle event requires r0-r3"
  _ <- globals sample
  _ <- selectRow source sample
  pure (Event site address registers sample)

data Producer = Producer Address Address deriving (Eq, Show)
data SaveCall = SaveCall Frame Address deriving (Eq, Show)
data Observer = Observer
  { nextCursor :: C.Cursor, lastTick :: Maybe Tick, observations :: [Value]
  , observedCounts :: M.Map Site Word64, allocatedSlot :: Maybe (Slot,Frame)
  , producers :: [Producer], initializerFrames :: [Frame], releaseSlots :: [ReleaseArgument]
  , saveCalls :: [SaveCall], snapshotCount :: Word64, observedDetails :: Word64
  , callbackInputs :: [CompletionCode]
  , unrelatedEvents :: Word64
  , saveOrigin :: Maybe ProgramPath
  , matchedSaveReturns :: Word64
  , retainedLoss :: Bool, lastPagePending :: Word64, boundaryComplete :: Bool
  }
initialObserver :: Observer
initialObserver = Observer C.initialCursor Nothing [] M.empty Nothing [] [] [] [] 0 0 [] 0 Nothing 0 False 0 False
pushObservation :: Tick -> Value -> Observer -> Either Refusal Observer
pushObservation tick value observer = do
  require EvidenceBoundExceeded (observedDetails observer < 4096) "lifecycle observation limit exceeded"
  require JournalMismatch (maybe True (<= tick) (lastTick observer)) "lifecycle evidence instruction count regressed"
  pure observer {lastTick=Just tick,observations=value:observations observer,observedDetails=observedDetails observer+1}

observeEvent :: Source -> Program -> Observer -> Event -> Either Refusal Observer
observeEvent source program observer (Event site address registers sample@(Sample tick (Context r4 r5 r11 _ _) _)) = do
  let r0 = registers !! 0
      r1 = registers !! 1
      r2 = registers !! 2
      record extra updated = do
        global <- globals sample
        pushObservation tick (object ["site" .= siteName site,"address" .= addressValue address
          ,"instructions" .= tickValue tick,"globals" .= global,"observation" .= extra])
          updated {observedCounts=M.insertWith (+) site 1 (observedCounts updated)}
      frameRecord pair@(_,frame) allocated kind updated = do
        let (Slot index,_) = pair
        (_,current) <- slotFrame source sample index
        requirePointer current (frameValue frame)
        detail <- frameEvidence sample frame allocated kind
        record detail updated {allocatedSlot=Just pair}
      localPair slotAddress frameAddress = do
        index <- readWord sample slotAddress
        pointer <- readWord sample frameAddress
        pair@(_,frame) <- slotFrame source sample index
        requirePointer frame pointer
        pure pair
      activeFrame = case allocatedSlot observer of
        Just pair -> pure pair
        Nothing -> refuse MissingEvidence "frame seam lacks a prior independently observed allocator slot"
      stockCallbacks = (Callback (Address 0xe4e5dc),Callback (Address 0xe4e5a8))
      ownedCallbacks = case program of
        Retained -> stockCallbacks
        OwnedProgram (Owned _ _ _ completion release _ _) -> (Callback completion,Callback release)
      expectedCallbacks = case saveOrigin observer of
        Just StockPath -> [stockCallbacks]
        Just OwnedPath -> [ownedCallbacks]
        Nothing -> nub [stockCallbacks,ownedCallbacks]
      callbackWord (Callback callback) = addressValue callback
  require EvidenceBoundExceeded (all (<=64) [length (producers observer),length (initializerFrames observer),length (releaseSlots observer),length (saveCalls observer),length (callbackInputs observer)])
    "lifecycle call nesting exceeds bound"
  case site of
    AllocatorReturn | r0 == 0xffffffff -> record (object ["allocator_exhausted" .= True]) observer {allocatedSlot=Nothing}
    AllocatorReturn -> do
      pair@(_,frame) <- slotFrame source sample r0
      marker <- frameWord sample frame 0x3268
      require OwnershipMismatch (marker == 1) "allocator returned a frame without allocation ownership"
      record (object ["slot" .= r0,"derived_frame" .= frameValue frame,"owned" .= True]) observer {allocatedSlot=Just pair}
    ProducerEntry -> do
      _ <- physical (sampleContext sample) (Address r0) 4
      _ <- physical (sampleContext sample) (Address r1) 4
      record (object ["slot_output_cell" .= r0,"frame_output_cell" .= r1]) observer {producers=Producer (Address r0) (Address r1):producers observer}
    SlotPublication -> do
      pair@(Slot index,_) <- activeFrame
      require OutputCellMismatch (r0 == index) "producer slot publication differs from allocator result"
      record (object ["slot" .= index]) observer {allocatedSlot=Just pair}
    FramePublication -> do
      pair@(_,frame) <- activeFrame
      requirePointer frame r0
      frameRecord pair RequireAllocated Nothing observer
    InitializationCall -> do
      pair@(_,frame) <- pointerFrame source sample r0
      requirePointer frame r0
      frameRecord pair RequireAllocated Nothing observer
    ProducerReturn -> do
      case producers observer of
        Producer slotOut frameOut:rest -> do
          require OutputCellMismatch (addressValue slotOut == r4 && addressValue frameOut == r5) "void producer output pointers changed"
          pair <- localPair slotOut frameOut
          frameRecord pair RequireAllocated Nothing observer {producers=rest}
        [] -> do
          pair <- localPair (Address r4) (Address r5)
          frameRecord pair RequireAllocated Nothing observer
    PopulationAllocated -> do
      require OutputCellMismatch (r11 >= 0x24) "population frame-pointer locals underflow"
      pair <- localPair (Address (r11-0x20)) (Address (r11-0x24))
      frameRecord pair RequireAllocated Nothing observer
    PopulationReturn -> do
      require OutputCellMismatch (r11 >= 0x24) "population frame-pointer locals underflow"
      pair@(_,frame) <- localPair (Address (r11-0x20)) (Address (r11-0x24))
      mode <- readWord sample (Address 0x1581f50)
      actual <- frameWord sample frame 0x78
      require PopulatedFieldsMismatch (actual == mode) "populated frame mode differs from selected mode global"
      frameRecord pair RequireAllocated Nothing observer
    InitializerEntry -> do
      pair@(_,frame) <- pointerFrame source sample r0
      frameRecord pair RequireAllocated Nothing observer {initializerFrames=frame:initializerFrames observer}
    InitializerReturn -> do
      pair@(_,frame) <- pointerFrame source sample r5
      case initializerFrames observer of
        entry:rest -> do
          requirePointer entry r5
          initialized pair frame rest
        [] -> initialized pair frame []
    ReleaseEntry | r0 >= 201 ->
      record (object ["namespace_invalid_slot" .= r0]) observer {releaseSlots=InvalidRelease r0:releaseSlots observer}
    ReleaseEntry -> do
      pair <- slotFrame source sample r0
      frameRecord pair RequireAllocated Nothing observer {releaseSlots=ValidRelease (fst pair):releaseSlots observer}
    ReleaseClear -> do
      pair <- pointerFrame source sample r4
      require OwnershipMismatch (r0 == 0 && registers !! 3 == 0x3268) "release clear-store operands differ"
      frameRecord pair RequireAllocated Nothing observer
    ReleaseReturn -> do
      require OwnershipMismatch (r0 == 0) "release success seam has nonzero status"
      pair@(_,frame) <- pointerFrame source sample r4
      case releaseSlots observer of
        ValidRelease (Slot index):rest -> do
          (_,expected) <- slotFrame source sample index
          requirePointer expected r4
          released pair frame rest
        InvalidRelease _:_ -> refuse NamespaceMismatch "release success followed a namespace-invalid slot"
        [] -> released pair frame []
    ReleaseInvalid -> do
      require OwnershipMismatch (r0 == 1) "invalid-slot release status differs"
      case releaseSlots observer of
        InvalidRelease index:rest ->
          record (object ["invalid_slot" .= index]) observer {releaseSlots=rest}
        ValidRelease _:_ -> refuse NamespaceMismatch "invalid-slot return followed a namespace-valid release"
        [] -> record (object ["invalid_slot_identity" .= (Nothing :: Maybe Word32)]) observer
    SaveCaller -> do
      pair@(_,frame) <- pointerFrame source sample r0
      if r2 == 0 then pure () else do
        _ <- readWord sample (Address r2)
        pure ()
      frameRecord pair RequireAllocated (Just (saveKind r1)) observer
        {saveCalls=SaveCall frame (Address r2):saveCalls observer
        ,saveOrigin=Just (case program of Retained -> StockPath; OwnedProgram _ -> OwnedPath)}
    SaveEntry -> do
      pair <- pointerFrame source sample r0
      frameRecord pair RequireAllocated (Just (saveKind r1)) observer
        {saveOrigin=Just (if address == Address 0xe518c0 then StockPath else OwnedPath)}
    SaveContinuation -> case saveCalls observer of
      SaveCall frame output:rest -> do
        require OutputCellMismatch (r0 `elem` [0,1]) "save caller return leaves source Boolean return domain"
        status <- if addressValue output == 0 then pure Nothing else Just <$> readWord sample output
        record (object ["frame" .= frameValue frame,"return_word" .= r0,"status_output" .= status
          ,"save_completion_proved" .= False]) observer
          {saveCalls=rest,saveOrigin=Nothing,matchedSaveReturns=matchedSaveReturns observer+1}
      [] -> record (object ["caller_entry_unobserved" .= True,"save_completion_proved" .= False]) observer
    StorageDirect -> storage expectedCallbacks r1 r2 record
    StorageThree -> storage expectedCallbacks r1 r2 record
    ReleaseRegistration -> do
      require CallbackTaskMismatch (any (\(_,release) -> r0 == callbackWord release) expectedCallbacks)
        "release callback differs from observed save path or declared program symbols"
      task <- Task <$> readWord sample (Address 0x155ccbc)
      record (object ["callback" .= r0,"task" .= taskValue task,"registration_only" .= True]) observer
    SaveCompletion -> do
      task <- Task <$> readWord sample (Address 0x155ccb8)
      record (object ["completion_input" .= r0,"sampled_task" .= taskValue task
        ,"expected_status_if_body_returns" .= completionStatus (CompletionCode r0),"callback_return_unobserved" .= True])
        observer {callbackInputs=CompletionCode r0:callbackInputs observer}
    ReleaseCompletion -> do
      task <- Task <$> readWord sample (Address 0x155ccbc)
      record (object ["sampled_task" .= taskValue task,"callback_return_unobserved" .= True]) observer
    SaveStatusCommitted -> do
      status <- readWord sample (Address 0x154d970)
      case callbackInputs observer of
        code:rest -> do
          require CallbackTaskMismatch (status == completionStatus code) "committed completion status differs from observed callback input"
          record (object ["committed_status" .= status,"callback_retirement_proved" .= False])
            observer {callbackInputs=rest}
        [] -> record (object ["committed_status" .= status,"callback_input_unobserved" .= True]) observer
    SaveSignal -> signal 0x155ccb8 (registers !! 0) (registers !! 1) record
    ReleaseSignal -> signal 0x155ccbc (registers !! 0) (registers !! 1) record
    _ -> record (object ["entry_observed" .= True]) observer
  where
    signal taskAddress actualTask mask record = do
      task <- readWord sample (Address taskAddress)
      require CallbackTaskMismatch (mask == 0x10) "callback signal mask differs from source"
      record (object ["signal_task" .= actualTask,"sampled_task_slot" .= task,"task_equals_sampled_slot" .= (actualTask == task)
        ,"signal_mask" .= mask,"scheduler_delivery_proved" .= False]) observer
    initialized pair frame rest = do
      values <- mapM (frameWord sample frame) [0x275c,0x604,0x2fb0,0x27cc,0x27c8]
      require InitializedFieldsMismatch (values == [0,0,0,0,1]) "initialized frame fields differ from source writes"
      detail <- frameEvidence sample frame RequireAllocated Nothing
      pushObservation tick (object ["site" .= siteName site,"instructions" .= tickValue tick,"observation" .= detail])
        observer {initializerFrames=rest,allocatedSlot=Just pair,observedCounts=M.insertWith (+) site 1 (observedCounts observer)}
    released pair frame rest = do
      marker <- frameWord sample frame 0x3268
      require OwnershipMismatch (marker == 0) "release returned before ownership marker cleared"
      detail <- frameEvidence sample frame ObserveOwnership Nothing
      pushObservation tick (object ["site" .= siteName site,"instructions" .= tickValue tick,"observation" .= detail
        ,"global_lifetime_proved" .= False]) observer {releaseSlots=rest,allocatedSlot=Just pair,observedCounts=M.insertWith (+) site 1 (observedCounts observer)}
    storage callbacks descriptor callback record = do
      require CallbackTaskMismatch (any (\(Callback completion,_) -> callback == addressValue completion) callbacks)
        "storage completion differs from observed save path or declared program symbols"
      CatalogueRow _ (Count count) (Address root) _ <- selectRow source sample
      let candidates = [(index,bank) | index <- [0..count-1],bank <- [0..2 :: Word32],root+index*0x326c+0x27d0+bank*0x150 == descriptor]
      (index,bank) <- case candidates of
        [candidate] -> pure candidate
        _ -> refuse FramePointerMismatch "storage descriptor is not a selected frame bank"
      (_,frame) <- slotFrame source sample index
      marker <- frameWord sample frame 0x3268
      require OwnershipMismatch (marker == 1) "storage descriptor belongs to an unallocated frame"
      _ <- readBytes sample (Address descriptor) 0x150
      task <- Task <$> readWord sample (Address 0x155ccb8)
      record (object ["frame" .= frameValue frame,"bank" .= bank,"completion" .= callback
        ,"task" .= taskValue task,"registration_entry_only" .= True]) observer

tickValue :: Tick -> Word64
tickValue (Tick tick) = tick
taskValue :: Task -> Word32
taskValue (Task task) = task
completionStatus :: CompletionCode -> Word32
completionStatus (CompletionCode code) | code == 0 = 0 | code == 0xfffffffc = 1 | otherwise = 2

consumePage :: Source -> Program -> Observer -> Value -> Either Refusal Observer
consumePage source program observer raw = fst <$> consumePageWithMetadata source program observer raw

consumePageWithMetadata :: Source -> Program -> Observer -> Value -> Either Refusal (Observer, C.Page)
consumePageWithMetadata source program observer raw = do
  page <- either (refuse JournalMismatch) Right (C.parsePage (nextCursor observer) raw)
  hits <- field "hits" raw
  let ticks = map (Tick . C.instructionValue . C.eventInstructions) (C.pageHits page)
      chronological = maybe ticks (:ticks) (lastTick observer)
      knownAddresses = [address | (_,address,_) <- registrations source program]
  require JournalMismatch (and (zipWith (<=) chronological (drop 1 chronological))) "capture page instruction chronology regressed"
  retained <- forM hits $ \hit -> do
    address <- Address <$> field "address" hit
    label <- field "site" hit
    if address `elem` knownAddresses || "save-lifecycle-" `T.isPrefixOf` label
      then Just <$> parseEvent source program hit
      else pure Nothing
  let events = catMaybes retained
  next <- foldM (observeEvent source program) observer events
  pure (next {nextCursor=C.pageNext page,retainedLoss=retainedLoss observer || C.pageIncomplete page || C.pageDropped page /= 0
    ,lastTick=foldl (\_ tick -> Just tick) (lastTick next) ticks
    ,unrelatedEvents=unrelatedEvents observer+fromIntegral (length retained-length events)
    ,lastPagePending=C.pagePending page-fromIntegral (length (C.pageHits page)),boundaryComplete=C.pageBoundaryComplete page},page)
observeSnapshot :: Source -> Program -> Observer -> Value -> Either Refusal Observer
observeSnapshot source program observer raw = do
  Snapshot sample@(Sample tick _ _) _ <- parseSnapshot source program raw
  global <- globals sample
  pushObservation tick (object ["snapshot" .= True,"instructions" .= tickValue tick,"globals" .= global])
    observer {snapshotCount=snapshotCount observer+1}

observationValue :: Observer -> Value
observationValue observer = object
  ["kind" .= ("source-bound-lifecycle-prefix" :: T.Text)
  ,"events" .= reverse (observations observer),"snapshots" .= snapshotCount observer
  ,"counts" .= [object ["site" .= siteName site,"observed" .= M.findWithDefault 0 site (observedCounts observer)] | site <- [minBound..maxBound]]
  ,"unobserved_paths" .= [siteName site | site <- [SaveCaller,SaveEntry,SaveCompletion,ReleaseCompletion,PermitEntry,StorageDirect,StorageThree,ReleaseRegistration]
    ,M.findWithDefault 0 site (observedCounts observer) == 0]
  ,"detail_loss" .= retainedLoss observer,"pending_details" .= lastPagePending observer,"boundary_complete" .= boundaryComplete observer
  ,"unrelated_events_skipped" .= unrelatedEvents observer
  ,"software_save_call_return_observed" .= (matchedSaveReturns observer > 0)
  ,"matched_save_call_returns" .= matchedSaveReturns observer
  ,"callback_status_commit_observed" .= (M.findWithDefault 0 SaveStatusCommitted (observedCounts observer) > 0)
  ,"save_completion_proved" .= False,"callback_retirement_proved" .= False,"global_free_or_reuse_proved" .= False
  ,"stop_context" .= ("prefix-only; execution stop is supplied by orchestration" :: T.Text)]

replayJournal :: Source -> Program -> [Value] -> [Value] -> Either Refusal Observer
replayJournal source program records snapshots = do
  require EvidenceBoundExceeded (not (null records) && length records <= 16384 && not (null snapshots) && length snapshots <= 256)
    "genuine journal and bounded snapshots are required"
  (begun,pages,observer) <- foldM step (False,0 :: Word64,initialObserver) records
  require JournalMismatch (begun && pages > 0 && lastPagePending observer == 0 && boundaryComplete observer && not (retainedLoss observer))
    "genuine journal must have an accepted begin and completely drained lossless pages"
  parsed <- mapM (parseSnapshot source program) snapshots
  let ticks = [tick | Snapshot (Sample tick _ _) _ <- parsed]
  require JournalMismatch (and (zipWith (<=) ticks (drop 1 ticks))) "snapshot instruction counts regressed"
  -- Snapshots and drained pages are independently ordered streams.
  foldM (\current raw -> observeSnapshot source program current {lastTick=Nothing} raw) observer snapshots
  where
    step (begun,pages,observer) record = do
      label <- field "label" record
      value <- field "value" record
      case (label :: T.Text) of
        "capture-observation-begin" -> do
          require JournalMismatch (not begun && pages == 0) "journal repeats capture begin"
          validateBegin program value
          pure (True,pages,observer)
        "capture-observation-page" -> do
          require JournalMismatch begun "capture page precedes accepted begin"
          next <- consumePage source program observer value
          pure (begun,pages+1,next)
        _ -> pure (begun,pages,observer)
