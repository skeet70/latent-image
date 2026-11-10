{-# LANGUAGE BangPatterns #-}
{-# LANGUAGE OverloadedStrings #-}
module Emulation.RecordStorePairCheck
  ( Role (..), Sidecar (..), sidecarName, Observation (..), Bindings (..)
  , Qualified, qualify, ComparisonContract, parseComparisonContract, comparisonEvidencePath, validateComparisonEvidence, comparePair, comparisonValue, validateInputs, cacheContract, cacheSymbolLayout
  ) where

import Control.Monad (foldM, forM, forM_, unless, when)
import Data.Aeson (Value (..), object, (.=))
import qualified Data.Aeson as A
import Data.Aeson.Decoding (toEitherValue)
import Data.Aeson.Decoding.ByteString.Lazy (lbsToTokens)
import Data.Aeson.Decoding.Tokens (Tokens (..), TkArray (..))
import qualified Data.Aeson.Key as K
import qualified Data.Aeson.KeyMap as KM
import Data.Bits ((.&.), (.|.), shiftL, shiftR)
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import qualified Data.ByteString.Lazy as L
import Data.List (sort, sortOn)
import qualified Data.Map.Strict as M
import Data.Scientific (toBoundedInteger)
import qualified Data.Set as S
import qualified Data.Text as T
import qualified Data.Text.Encoding as TE
import qualified Data.Vector as V
import Data.Word (Word32, Word64)
import Emulation.LocalUiMenu (LocalUiMenu (..), MenuNodeAddress (..), MenuIdentifier (..), SettingIdentifier (..), MenuCursor (..), MenuDepth (..), parseLocalUiMenu, LocalUiMode, parseLocalUiMode)
import Emulation.PropertyPairPlan (PhysicalRange (..), recordSettingsRanges, preKeyIdle)
import Emulation.PropertyPairRoutes
import Emulation.Sha256 (sha256, sha256Hex)
import Emulation.WatchObservation hiding (checkWatch)
import EmulatorUsb (CrawlKey (..))
import Numeric (readHex, showHex)

data Role = Stock | Owned deriving (Eq, Show)
data Sidecar = Spec | Extents | Reset | Hits | Steps | MediumFile | Session deriving (Eq, Ord, Show, Enum, Bounded)
sidecarName :: Sidecar -> String
sidecarName file = case file of
  Spec -> "spec.json"
  Extents -> "owned-extents.json"
  Reset -> "reset-observation.json"
  Hits -> "hits.jsonl"
  Steps -> "steps.json"
  MediumFile -> "medium.json"
  Session -> "emulator-session.json"
data Observation = Observation { receiptBytes :: B.ByteString, sidecars :: M.Map Sidecar B.ByteString }
data Bindings = Bindings { contractBytes :: B.ByteString, linkedBytes :: B.ByteString, symbolBytes :: B.ByteString, routeBytes :: B.ByteString }
newtype Address = Address Word32 deriving (Eq, Ord, Show)
newtype Handle = Handle Address deriving (Eq, Ord, Show)
newtype RecordSize = RecordSize Int deriving (Eq, Show)
newtype TotalRecords = TotalRecords Int deriving (Eq, Show)
newtype SequenceOffset = SequenceOffset Int deriving (Eq, Show)
newtype ByteCount = ByteCount Int deriving (Eq, Show)
data Layout = Layout RecordSize TotalRecords SequenceOffset deriving (Eq, Show)
data Entry = Open | Register | Close | Latest | Append | EraseAll | Collect | ReadAt | Zero deriving (Eq, Ord, Show, Enum, Bounded)
newtype InputName = InputName T.Text deriving (Eq, Ord, Show)
newtype Digest = Digest T.Text deriving (Eq, Ord, Show)
newtype Regions = Regions [(Address, B.ByteString)] deriving (Eq, Show)
data Block = Block Word32 (Maybe Word32) B.ByteString (Maybe B.ByteString) deriving (Eq, Show)
newtype Medium = Medium [Block] deriving (Eq, Show)
data Menu = NoMenu | Menu Word32 Word32 Word32 deriving (Eq, Show)
data Press = Press Menu LocalUiMenu LocalUiMode deriving (Eq, Show)
data OutputRule = CompareBytes | PreserveBytes | UnresolvedBytes | CollectionBytes RecordSize SequenceOffset ByteCount deriving (Eq, Show)
data Output = AbsentOutput | Output OutputRule B.ByteString B.ByteString deriving (Eq, Show)
data Call = Call Entry [Word32] (Maybe Word32) [Output] deriving (Eq, Show)
data Nand = Program [Word32] B.ByteString | Erase Word32 Word32 deriving (Eq, Show)
data Readback = Readback Address B.ByteString deriving (Eq, Show)
data Event = Enter | Return deriving (Eq, Show)
data Registers = Registers { register0, register1, register2, register3 :: Word32 } deriving (Eq, Show)
registerWords :: Registers -> [Word32]
registerWords registers = [register0 registers,register1 registers,register2 registers,register3 registers]
data Hit = Hit Int Address Address Address Registers (Maybe (Event, Address, [Readback])) deriving (Eq, Show)
data Service = Unopened | Initialized deriving (Eq, Show)
data CallContext = CallContext Service (Maybe Layout) deriving (Eq, Show)
data Pending = Pending Int Entry Hit CallContext deriving (Eq, Show)
data Retirement = BeforeOpen | AfterOpen deriving (Eq, Show)
data State = State Retirement Service (M.Map Handle Layout) (M.Map (Int, Address, Address) Pending) deriving (Eq, Show)
data Interval = Interval T.Text [CrawlKey] [Call] [Nand] Regions Regions (Maybe Press) (Maybe Medium) deriving (Eq, Show)
data Qualified = Qualified Role [(InputName,Digest)] [Route] [Interval] Medium deriving (Eq, Show)
data Difference = Difference T.Text Value Value | AmbiguousOutput T.Text Value Value | PreservationViolation T.Text Value Value | OracleViolation Role T.Text Value Value deriving (Eq, Show)
newtype EvidencePath = EvidencePath FilePath deriving (Eq, Show)
data SourceEvidence = SourceEvidence EvidencePath Digest deriving (Eq, Show)
data ComparisonContract = StockEquality Digest | ImageSizePendingChoice Digest SourceEvidence deriving (Eq, Show)
data ImageSizeCase = ImageSizeUp | ImageSizeBack deriving (Eq, Show, Enum, Bounded)
data ImageSizeChoice = LargeThreeTwo | MediumThreeTwo deriving (Eq, Show)
data ImageSizeFrame = ParentFrame | EntryFrame | PendingFrame deriving (Eq, Show)
data ImageSizeTransition = EnterImageSize | PreviewMedium deriving (Eq, Show)
data UiProjection = UiProjection Regions Menu LocalUiMenu deriving (Eq, Show)
data ImageSizeBaseline = ImageSizeBaseline UiProjection UiProjection UiProjection
data ReferenceDisposition = ReferenceIntended | ReferenceStalled deriving (Eq, Show)
data ImageSizeCaseResult = ImageSizeCaseResult ImageSizeCase ReferenceDisposition deriving (Eq, Show)

imageSizeKeys :: ImageSizeCase -> [CrawlKey]
imageSizeKeys action = [KeyOk,KeyDown,KeyOk,KeyDown,case action of ImageSizeUp -> KeyUp; ImageSizeBack -> KeyBack]

imageSizeTrial :: ImageSizeCase -> Int
imageSizeTrial ImageSizeUp = 368
imageSizeTrial ImageSizeBack = 371

imageSizeFixture :: Digest
imageSizeFixture = Digest "eb33ae15ec676f5dfd9465223500c89179af5c90f14047b4e528a7db741e8682"

type Parse = Either String
need :: Bool -> String -> Parse ()
need condition message = unless condition (Left message)
field :: T.Text -> Value -> Parse Value
field key (Object fields) = maybe (Left ("missing " ++ T.unpack key)) Right (KM.lookup (K.fromText key) fields)
field _ _ = Left "expected object"
array :: Value -> Parse [Value]
array (Array values) = Right (V.toList values)
array _ = Left "expected array"
text :: Value -> Parse T.Text
text (String value) = Right value
text _ = Left "expected text"
word :: Value -> Parse Word32
word (Number value) = maybe (Left "expected uint32") Right (toBoundedInteger value)
word (String value) | Just digits <- T.stripPrefix "0x" value, [(n,"")] <- readHex (T.unpack digits), n <= toInteger (maxBound :: Word32) = Right (fromInteger n)
word _ = Left "expected uint32"
word64 :: Value -> Parse Word64
word64 (Number value) = maybe (Left "expected uint64") Right (toBoundedInteger value)
word64 _ = Left "expected uint64"
int :: Value -> Parse Int
int value = fromIntegral <$> word value
nullable :: (Value -> Parse a) -> Value -> Parse (Maybe a)
nullable _ Null = Right Nothing
nullable parse value = Just <$> parse value
json :: B.ByteString -> Parse Value
json = A.eitherDecodeStrict'
hex :: Value -> Parse B.ByteString
hex value = do
  encoded <- TE.encodeUtf8 <$> text value
  need (even (B.length encoded) && B.all valid encoded) "invalid hexadecimal bytes"
  pure (fst (B.unfoldrN (B.length encoded `div` 2) pair encoded))
 where
  valid byte = byte >= 48 && byte <= 57 || byte >= 65 && byte <= 70 || byte >= 97 && byte <= 102
  nibble byte | byte <= 57 = byte-48
              | byte <= 70 = byte-55
              | otherwise = byte-87
  pair remaining
    | B.null remaining = Nothing
    | otherwise = Just ((nibble (B.index remaining 0) `shiftL` 4) .|. nibble (B.index remaining 1),B.drop 2 remaining)
digest :: B.ByteString -> Digest
digest = Digest . T.pack . sha256Hex . sha256
parseDigest :: Value -> Parse Digest
parseDigest value = do
  bytes <- hex value
  need (B.length bytes == 32) "digest is not SHA256"
  Digest . T.toLower <$> text value
exact :: [T.Text] -> Value -> Parse ()
exact keys (Object fields) = need (sort (map K.toText (KM.keys fields)) == sort keys) "object fields differ"
exact _ _ = Left "expected object"
parseComparisonContract :: B.ByteString -> Parse ComparisonContract
parseComparisonContract bytes = do
  value <- json bytes
  need (field "schema" value == Right (Number 1)) "unsupported comparison contract schema"
  policy <- field "policy" value >>= text
  case policy of
    "stock-equality-v1" -> exact ["schema","policy"] value >> pure (StockEquality (digest bytes))
    "image-size-pending-choice-v1" -> do
      exact ["schema","policy","record_nand_sha256","source_evidence","cases"] value
      fixture <- field "record_nand_sha256" value >>= parseDigest
      need (fixture == imageSizeFixture) "image-size oracle fixture differs"
      source <- field "source_evidence" value
      exact ["path","sha256"] source
      path <- field "path" source >>= text
      need (not (T.null path) && not (T.any (== '\0') path)) "invalid source evidence path"
      identity <- field "sha256" source >>= parseDigest
      let evidence = SourceEvidence (EvidencePath (T.unpack path)) identity
      cases <- field "cases" value >>= array >>= mapM parseCase
      need (cases == [ImageSizeUp,ImageSizeBack]) "image-size oracle cases differ"
      pure (ImageSizePendingChoice (digest bytes) evidence)
    _ -> Left "unknown comparison policy"
 where
  parseCase value = do
    exact ["crawl_trial","keys"] value
    trial <- field "crawl_trial" value >>= int
    action <- case trial of
      368 -> Right ImageSizeUp
      371 -> Right ImageSizeBack
      _ -> Left "unknown image-size oracle case"
    need (field "keys" value == Right (A.toJSON (imageSizeKeys action))) "image-size oracle key path differs"
    pure action

comparisonEvidencePath :: ComparisonContract -> Maybe FilePath
comparisonEvidencePath (StockEquality _) = Nothing
comparisonEvidencePath (ImageSizePendingChoice _ (SourceEvidence (EvidencePath path) _)) = Just path

validateComparisonEvidence :: ComparisonContract -> B.ByteString -> Parse ()
validateComparisonEvidence (StockEquality _) _ = pure ()
validateComparisonEvidence (ImageSizePendingChoice _ (SourceEvidence _ expected)) bytes =
  need (digest bytes == expected) "comparison source evidence digest differs"

entryAddress :: Entry -> Address
entryAddress entry = Address ([0x6ac2b4,0x6ac328,0x6ac478,0x6ac4c8,0x6ac7d4,0x6ac984,0x6acab8,0x6acda0,0x6ae0fc] !! fromEnum entry)
entryAt :: Address -> Parse Entry
entryAt address = maybe (Left "unknown record entry") Right (lookup address [(entryAddress entry,entry) | entry <- [minBound..maxBound]])
addressValue :: Address -> Word32
addressValue (Address address) = address
physical :: Word32 -> Address
physical address = Address (address .&. 0xbfffffff)
retired :: Role -> [(Word32,Word32)]
retired _ = [(0x89f8f0,0x89f8f8),(0x917fa8,0x917fe4)]
guardFetches :: Role -> [(Word32,Word32)] -> [(Word32,Word32)]
guardFetches Stock _ = [(0x6ac2b4,0x6ae114)]
guardFetches Owned code = merge ((0x6ac2b4,0x6ac2b8) : complement code)
within :: [(Word32,Word32)] -> Word32 -> Int -> Bool
within ranges address count = any (\(low,high) -> low <= address && toInteger address + toInteger count <= toInteger high) ranges
overlaps :: [(Word32,Word32)] -> Word32 -> Int -> Bool
overlaps ranges address count = any (\(low,high) -> toInteger address < toInteger high && toInteger low < toInteger address + toInteger count) ranges
merge :: [(Word32,Word32)] -> [(Word32,Word32)]
merge = foldr combine [] . sortOn fst where
  combine range [] = [range]
  combine (low,high) ((next,last'):rest) | high >= next = combine (low,max high last') rest
  combine range rest = range:rest
complement :: [(Word32,Word32)] -> [(Word32,Word32)]
complement = go 0x6ac2b4 where
  go low [] = [(low,0x6ae114) | low < 0x6ae114]
  go low ((start,end):rest) = [(low,start) | low < start] ++ go end rest
parseRanges :: Value -> Parse [(Word32,Word32)]
parseRanges value = array value >>= mapM (\row -> do
  values <- array row >>= mapM word
  case values of
    [low,high] | low < high -> Right (low,high)
    _ -> Left "invalid range")
rangesValue :: [(Word32,Word32)] -> Value
rangesValue values = A.toJSON [[low,high] | (low,high) <- values]

validateInputs :: Bindings -> Role -> Value -> Parse [(InputName,Digest)]
validateInputs bindings role receipt = do
  inputs <- field "inputs" receipt
  let required = ["update","routes","contract","linked","symbols","run","boot-rom","adjustment-nand","observer","bridge"]
  names <- case inputs of Object fields -> Right (map K.toText (KM.keys fields)); _ -> Left "inputs are not an object"
  need (all (`elem` names) required && all (`elem` ("record-nand":required)) names) "input set differs"
  parsed <- forM names $ \name -> do
    binding <- field name inputs
    exact ["path","sha256"] binding
    _ <- field "path" binding >>= text
    value <- field "sha256" binding >>= parseDigest
    pure (InputName name,value)
  contract <- json (contractBytes bindings)
  let expected name bytes = need (lookup (InputName name) parsed == Just (digest bytes)) ("input digest differs: " ++ T.unpack name)
  expected "contract" (contractBytes bindings)
  expected "linked" (linkedBytes bindings)
  expected "symbols" (symbolBytes bindings)
  expected "routes" (routeBytes bindings)
  linked <- field "linked_sha256" contract >>= parseDigest
  symbols <- field "symbols_sha256" contract >>= parseDigest
  need (linked == digest (linkedBytes bindings) && symbols == digest (symbolBytes bindings)) "contract does not bind linked carrier and symbols"
  cache <- field "owned_cache" contract
  need (cache == cacheContract) "owned two-slot cache contract differs"
  update <- field (if role == Stock then "source_sha256" else "candidate_sha256") contract >>= parseDigest
  need (lookup (InputName "update") parsed == Just update) "DAT does not match role contract"
  pure (filter ((/= InputName "update") . fst) parsed)

ownedRanges :: Bindings -> Parse ([(Word32,Word32)],[(Word32,Word32)])
ownedRanges bindings = do
  rows <- forM (C.lines (symbolBytes bindings)) $ \line -> case words (C.unpack line) of
    [address,size,kind,name] | [(low,"")] <- readHex address, [(count,"")] <- readHex size,
      low >= (0 :: Integer), low+count <= toInteger (maxBound :: Word32) -> Right (fromInteger low,fromInteger count,kind,name)
    _ -> Left "invalid symbol row"
  let code = merge [(low,low+count) | (low,count,kind,_) <- rows, kind `elem` ["t","T"], count > 0, low >= 0x6ac2b4, low+count <= 0x6ae114]
      symbol name = case [low | (low,_,_,label) <- rows, label == name] of [low] -> Right low; _ -> Left ("missing or repeated symbol " ++ name)
  forM_ [minBound..maxBound] $ \entry -> need (within code (addressValue (entryAddress entry)) 4) "carrier misses public veneer"
  forM_ cacheSymbolLayout $ \(name,address) -> symbol name >>= \actual -> need (actual == address) ("carrier placement differs: " ++ name)
  need (all (\(address,_,_,name) -> name /= "record_store_cache_tag" && (address < 0x89f8f0 || address >= 0x89f8f8)) rows) "obsolete cache symbol or gap alias remains"
  need (all (\(address,count,_,name) -> not (overlaps [(0x89fa60,0x89fa68),(0x8a0080,0x8e008c)] address (max 1 (fromIntegral count))) || lookup name cacheSymbolLayout == Just address) rows) "undeclared cache ownership alias"
  need (B.length (linkedBytes bindings) == 7776) "carrier extent differs"
  pure (code,[(0x89f8f8,0x89fa60),(0x89fa60,0x89fa68),(0x8a0080,0x8e0080),(0x8e0080,0x8e008c)])

cacheSymbolLayout :: [(String,Word32)]
cacheSymbolLayout =
  [("record_store_initialized",0x89f8b0),("record_store_wrappers",0x89f8b4),("record_store_state",0x89f8f8)
  ,("record_store_block",0x8a0080),("record_store_second_block",0x8c0080)
  ,("record_store_cache_tags",0x8e0080),("record_store_cache_timestamp",0x8e0088),("record_store_cache_ages",0x89fa60)
  ,("record_store_staging_0",0x8e16a0),("record_store_staging_1",0x8e00a0),("record_store_staging_2",0x8e1700)
  ,("record_store_staging_3",0x89fa80),("record_store_staging_4",0x8e10a0)]

cacheContract :: Value
cacheContract = object
  ["buffers" .= ([[0x8a0080,0x8c0080],[0x8c0080,0x8e0080]]::[[Word32]])
  ,"tags" .= object ["address" .= (0x8e0080::Word32),"bytes" .= (8::Int),"initial_values" .= ([0xffffffff,0xffffffff]::[Word32])]
  ,"timestamp" .= object ["address" .= (0x8e0088::Word32),"bytes" .= (4::Int),"initial_value" .= (0::Word32)]
  ,"ages" .= object ["address" .= (0x89fa60::Word32),"bytes" .= (8::Int),"initial_values" .= ([0,0]::[Word32])]
  ,"initialized_by" .= (0x6ac2b4::Word32)]

data RegionSnapshot = SettingsSnapshot | ResetSnapshot Role deriving (Eq, Show)
parseRegions :: RegionSnapshot -> Value -> Parse Regions
parseRegions snapshot value = do
  rows <- array value >>= mapM (\row -> do
    exact ["start","bytes"] row
    (,) <$> (Address <$> (field "start" row >>= word)) <*> (field "bytes" row >>= hex))
  let ordered = sortOn fst rows
      settings = [(rangeStart range,rangeEnd range) | range <- recordSettingsRanges]
      expected = case snapshot of
        SettingsSnapshot -> settings
        ResetSnapshot role -> settings ++ retired role
  need ([(addressValue start,B.length bytes) | (start,bytes) <- ordered] == sort [(start,fromIntegral (end-start)) | (start,end) <- expected]) ("snapshot regions differ: " ++ show snapshot)
  pure (Regions [(start,bytes) | (start,bytes) <- ordered, addressValue start `elem` map fst settings])
parseMedium :: Value -> Parse Medium
parseMedium value = do
  rows <- array value >>= mapM (\row -> do
    exact ["logical","physical","data","spare"] row
    logical <- field "logical" row >>= word
    mapped <- field "physical" row >>= nullable word
    bytes <- field "data" row >>= hex
    spare <- field "spare" row >>= nullable hex
    need (B.length bytes == 131072) "medium block has incorrect size"
    case (mapped,spare) of
      (Nothing,Nothing) -> need (B.all (==255) bytes) "unmapped block is not erased"
      (Just _,Just extra) -> need (B.length extra == 32768) "spare block has incorrect size"
      _ -> Left "medium map/spare disagree"
    pure (Block logical mapped bytes spare))
  need ([logical | Block logical _ _ _ <- rows] == [0x52..0x59] ++ [0x68,0x69]) "record pool blocks differ"
  pure (Medium rows)
parseMenu :: Value -> Parse Menu
parseMenu value = do
  exact ["menu_id","setting_id","cursor"] value
  values <- mapM (\name -> field name value >>= nullable word) ["menu_id","setting_id","cursor"]
  case values of
    [Nothing,Nothing,Nothing] -> Right NoMenu
    [Just node,Just setting,Just cursor] -> Right (Menu node setting cursor)
    _ -> Left "partially absent menu"

parseHit :: Value -> Parse Hit
parseHit value = do
  core <- field "core" value >>= int
  need (core <= 1) "invalid hit core"
  address <- Address <$> (field "address" value >>= word)
  sp <- Address <$> (field "sp" value >>= word)
  lr <- Address <$> (field "lr" value >>= word)
  registers <- field "registers" value
  args <- Registers <$> (field "r0" registers >>= word) <*> (field "r1" registers >>= word) <*> (field "r2" registers >>= word) <*> (field "r3" registers >>= word)
  observation <- case value of
    Object fields -> case KM.lookup "observation" fields of
      Nothing -> Right Nothing
      Just Null -> Right Nothing
      Just observed -> do
        exact ["event","site","reads"] observed
        event <- field "event" observed >>= text >>= \name -> case name of "entry" -> Right Enter; "return" -> Right Return; _ -> Left "unknown capture event"
        site <- Address <$> (field "site" observed >>= word)
        captures <- field "reads" observed >>= array >>= mapM (\row -> do
          exact ["address","bytes"] row
          Readback <$> (Address <$> (field "address" row >>= word)) <*> (field "bytes" row >>= hex))
        pure (Just (event,site,captures))
    _ -> Left "hit is not an object"
  pure (Hit core address sp lr args observation)
readAtAddress :: Address -> Int -> [Readback] -> Parse B.ByteString
readAtAddress address count captures = case [bytes | Readback at bytes <- captures, physical (addressValue at) == physical (addressValue address), B.length bytes == count] of
  [bytes] -> Right bytes
  _ -> Left ("missing or ambiguous capture at " ++ show address ++ " length " ++ show count)
leWord :: B.ByteString -> Word32
leWord = B.foldr (\byte acc -> fromIntegral byte .|. (acc `shiftL` 8)) 0 . B.take 4
wordsOf :: B.ByteString -> [Word32]
wordsOf bytes | B.null bytes = []
              | otherwise = leWord bytes : wordsOf (B.drop 4 bytes)
handle :: Word32 -> Handle
handle = Handle . Address
handleLayout :: State -> Word32 -> Maybe Layout
handleLayout (State _ _ layouts _) value = M.lookup (handle value) layouts

complete :: Pending -> Hit -> Parse Call
complete (Pending _ entry (Hit _ _ sp _ args observed) (CallContext service layout)) (Hit _ _ _ _ results returned) = do
  input <- maybe (Left "entry has no observation") (Right . (\(_,_,captures) -> captures)) observed
  output <- maybe (Left "return has no observation") (Right . (\(_,_,captures) -> captures)) returned
  let Registers r0 r1 r2 r3 = args
      status = register0 results
      at captures pointer count = readAtAddress (Address pointer) count captures
      extent = maybe (Left "missing registered geometry") Right layout
      bool value = if value == 0 then 0 else 1
      result semantic outputs = Right (Call entry semantic (if entry == Close then Nothing else Just status) outputs)
      captured rule before after = do
        when (rule == PreserveBytes) $ need (before == after) ("contract-untouched " ++ show entry ++ " output changed")
        pure (Output rule before after)
      optional pointer count rule = if pointer == 0 then pure AbsentOutput else do
        before <- at input pointer count
        after <- at output pointer count
        captured rule before after
      possible predicate before after
        | not (predicate after) = captured PreserveBytes before after
        | before == after = captured UnresolvedBytes before after
        | otherwise = captured CompareBytes before after
      optionalPossible pointer count predicate = if pointer == 0 then pure AbsentOutput else do
        before <- at input pointer count
        after <- at output pointer count
        possible predicate before after
  case entry of
    Open -> result [] []
    Register -> do
      stack <- at input (addressValue sp) 20
      let client = leWord (B.drop 16 stack)
          rule | status == 0 || service == Unopened && status == 0xfffffff2 = CompareBytes
               | otherwise = PreserveBytes
      clientOutput <- optional client 4 rule
      result (registerWords args ++ wordsOf (B.take 16 stack)) [clientOutput]
    Close -> result [r0] []
    Latest -> do
      pointerBefore <- at input r2 4
      pointerAfter <- at output r2 4
      let invalid = service == Unopened || layout == Nothing
      (pointerOutput,skippedOutput,record) <- if status == 0 then do
        Layout (RecordSize size) _ _ <- extent
        pointer <- captured CompareBytes pointerBefore pointerAfter
        skipped <- optional r1 2 CompareBytes
        bytes <- B.take size <$> at output (leWord pointerAfter) 4096
        pure (pointer,skipped,Output CompareBytes B.empty bytes)
      else if invalid || status == 0xffffffe2 then do
        need (status `elem` [0xfffffff2,0xffffffe2] && leWord pointerAfter == 0) "early latest failure did not write a null pointer"
        pointer <- captured CompareBytes pointerBefore pointerAfter
        skipped <- optional r1 2 PreserveBytes
        pure (pointer,skipped,AbsentOutput)
      else do
        Layout _ (TotalRecords count) _ <- extent
        need (status `elem` [0xfffffff2,0xffffff01]) "unclassified latest failure status"
        pointer <- if status == 0xfffffff2 then captured PreserveBytes pointerBefore pointerAfter
          else possible ((==0) . leWord) pointerBefore pointerAfter
        skipped <- optionalPossible r1 2 (\bytes -> fromIntegral (leWord bytes) <= max 0 (count `div` 2 - 1))
        pure (pointer,skipped,AbsentOutput)
      result [r0,bool r1,bool r2] [pointerOutput,record,skippedOutput]
    Append -> do
      Layout (RecordSize size) _ _ <- extent
      bytes <- B.take size <$> at input r2 4096
      result [r0,r1,r2] [Output CompareBytes B.empty bytes]
    EraseAll -> result [r0] []
    Collect -> do
      before <- at input r1 131072
      after <- at output r1 131072
      if status == 0xfffffff2 then do
        buffer <- captured PreserveBytes before after
        count <- optional r2 4 PreserveBytes
        result [r0,bool r1,bool r2] [buffer,count]
      else do
        Layout size@(RecordSize stride) (TotalRecords total) offset <- extent
        let capacity = total*stride
        need (capacity <= 131072 && stride > 0) "collect capture does not cover caller extent"
        countBefore <- if r2 == 0 then pure B.empty else at input r2 4
        countAfter <- if r2 == 0 then pure B.empty else at output r2 4
        let compacted = fromIntegral (leWord countAfter)
            finished = status == 0 || r2 /= 0 && compacted == 0 && countBefore /= countAfter
        need (status `elem` [0,0xffffff01]) "unclassified collect status"
        when (status == 0 && r2 /= 0) $ need (compacted > 0 && compacted <= capacity && compacted `mod` stride == 0) "invalid collected byte count"
        count <- if r2 == 0 then pure AbsentOutput else if status == 0
          then captured CompareBytes countBefore countAfter else possible ((==0) . leWord) countBefore countAfter
        let prefix = if status == 0 && r2 /= 0 then compacted else 0
            rule = if finished then CollectionBytes size offset (ByteCount prefix) else UnresolvedBytes
        buffer <- captured rule (B.take capacity before) (B.take capacity after)
        result [r0,bool r1,bool r2] [buffer,count]
    ReadAt -> do
      before <- at input r3 (fromIntegral r2)
      after <- at output r3 (fromIntegral r2)
      if status == 0xffffffe2 && service == Initialized && r0 <= 4 && r1 >= 0x40000 then do
        need (r2 > 0 && B.index after 0 == 0) "out-of-bank read lacks its written first byte"
        first <- captured CompareBytes (B.take 1 before) (B.take 1 after)
        rest <- captured PreserveBytes (B.drop 1 before) (B.drop 1 after)
        result [r0,r1,r2,bool r3] [first,rest]
      else result [r0,r1,r2,bool r3] [Output CompareBytes before after]
    Zero -> result [] []

consume :: State -> [Hit] -> Parse (State,[Call],[Nand])
consume state hits = do
  (final,calls,nand,_) <- foldM step (state,[],[],0::Int) hits
  pure (final,map snd (sortOn fst calls),reverse nand)
 where
  step (current@(State activation service layouts pending),calls,nand,index) hit@(Hit core address sp lr args observation) = case observation of
    Nothing -> Right (current,calls,nand,index+1)
    Just (event,site,captures)
      | site == Address 0x6fb7c8 -> do
          need (event == Enter && address == site) "invalid NAND write event"
          descriptor <- readAtAddress (Address (register0 args)) 16 captures
          let block = leWord descriptor
              offset = leWord (B.drop 4 descriptor)
              count = leWord (B.drop 8 descriptor)
              buffer = leWord (B.drop 12 descriptor)
          payload <- readAtAddress (physical buffer) (fromIntegral count) captures
          Right (current,calls,Program [block,offset,count,buffer] payload:nand,index+1)
      | site == Address 0x6fb57c -> do
          need (event == Enter && address == site) "invalid NAND erase event"
          descriptor <- readAtAddress (Address (register0 args)) 8 captures
          let block = leWord descriptor
              count = leWord (B.drop 4 descriptor)
          Right (current,calls,Erase block count:nand,index+1)
      | otherwise -> do
          entry <- entryAt site
          case event of
            Enter -> do
              need (address == site) "entry address differs from capture site"
              let internal = within [(0x6ac2b4,0x6ae114)] (addressValue lr) 1
              if internal then Right (current,calls,nand,index+1) else do
                let geometry = if entry `elem` [Latest,Append,Collect] then handleLayout current (register0 args) else Nothing
                let key = (core,sp,lr)
                need (M.notMember key pending) "duplicate active call identity"
                let marker = Pending index entry hit (CallContext service geometry)
                Right (State activation service layouts (M.insert key marker pending),calls,nand,index+1)
            Return -> do
              let key = (core,sp,address)
              case M.lookup key pending of
                Nothing | within [(0x6ac2b4,0x6ae114)] (addressValue address) 1 -> Right (current,calls,nand,index+1)
                Just started@(Pending entered expected (Hit _ _ _ _ original _) _) -> do
                  need (entry == expected) "return site differs from pending entry"
                  call@(Call _ _ _ buffers) <- complete started hit
                  updated <- case (entry,register0 args,buffers) of
                    (Register,0,[Output CompareBytes _ client]) -> do
                      let returnedHandle = leWord client
                          size = fromIntegral (register1 original)
                          offset = fromIntegral (register3 original)
                      need (service == Initialized && size >= 2 && offset+2 <= size && even offset && register2 original <= 0x7ffe) "successful registration contradicts its entry context"
                      need (returnedHandle `elem` [0x89f8b4 + 12*i | i <- [0..4]]) "returned handle is not a stock wrapper"
                      pure (M.insert (handle returnedHandle) (Layout (RecordSize size) (TotalRecords (2 * (fromIntegral (register2 original) `div` 2))) (SequenceOffset offset)) layouts)
                    (Close,_,_) -> pure (M.delete (handle (register0 original)) layouts)
                    _ -> pure layouts
                  let initialized = if entry == Open && register0 args == 0 then Initialized else service
                  Right (State activation initialized updated (M.delete key pending),(entered,call):calls,nand,index+1)
                _ -> Left "return has no matching external call"

checkWatch :: Role -> [(Word32,Word32)] -> Retirement -> T.Text -> Value -> Parse Retirement
checkWatch role code activation views value = do
  actualViews <- field "views" value >>= text
  need (views == actualViews) "watch decoder policy differs"
  classes <- field "classes" value >>= array
  let dataRanges = retired role
      writes = [(rangeStart range,rangeEnd range) | range <- recordSettingsRanges] ++ dataRanges
      fetches = guardFetches role code
      expected kind | kind `elem` ["cpu-write","host-write","model-bus-write"] = merge writes
                    | kind == "cpu-read" = merge dataRanges
                    | otherwise = merge fetches
      kinds = ["cpu-write","host-write","model-bus-write","cpu-read","cpu-fetch"]
  parsedClasses <- forM classes $ \row -> do
    exact ["range","kinds"] row
    range <- field "range" row >>= \single -> parseRanges (A.toJSON [single])
    accesses <- field "kinds" row >>= array >>= mapM text
    need (not (null accesses) && all (`elem` kinds) accesses) "unknown watch access class"
    pure (range,accesses)
  forM_ kinds $ \kind -> need (merge (concat [range | (range,accesses) <- parsedClasses, kind `elem` accesses]) == expected kind) "watch classes omit a required guard"
  limit <- watchLimit 1048576
  let stripped = case value of Object fields -> Object (KM.delete "classes" (KM.delete "views" fields)); _ -> value
  observed <- parseWatchObservation limit stripped
  need (watchEnabled observed && watchViolation observed == Nothing) "watch disabled or overflowed"
  need (map watchRangeBounds (watchRanges observed) == [(fromIntegral low,fromIntegral high) | (low,high) <- merge (writes ++ dataRanges ++ fetches)]) "watch coverage differs"
  if role == Stock then pure activation else foldM checkRow activation (watchRecords observed)
 where
  checkRow current row = case row of
    CpuRecord access _ at count _ _ _ _ _ _ _ -> do
      let address = watchAddressValue at
          size = watchBytesValue count
      if access == CpuFetch then do
        need (not (overlaps [(0x6ac2b4,0x6ae114)] address size) || within code address size) ("stray retired-code fetch: " ++ show row)
        pure (if address == 0x6ac2b4 && size == 4 then AfterOpen else current)
      else do
        when (overlaps (retired role) address size) $
          need (current == BeforeOpen && access == CpuWrite) ("retired-data CPU access: " ++ show row)
        pure current
    TransferRecord access _ at count -> do
      when (overlaps (retired role) (watchAddressValue at) (watchBytesValue count)) $
        need (current == BeforeOpen && access `elem` [HostWrite,ModelBusWrite]) ("retired-data transfer: " ++ show row)
      pure current

captureSpec :: Value
captureSpec = object [K.fromString ("0x" ++ showHex site "") .= capture site | site <- map (addressValue . entryAddress) [minBound..maxBound] ++ [0x6fb7c8,0x6fb57c]]
 where
  readSpec reg count indirect = object ["base" .= object ["register" .= (reg::Int)],"count" .= (count::Int),"indirect" .= indirect]
  capture site
    | site == 0x6fb7c8 = object ["entry" .= [readSpec 0 16 False,object ["base" .= object ["register-word" .= object ["register" .= (0::Int),"offset" .= (12::Int)]],"count" .= (0::Int),"count_base" .= object ["register-word" .= object ["register" .= (0::Int),"offset" .= (8::Int)]]]]]
    | site == 0x6fb57c = object ["entry" .= [readSpec 0 8 False]]
    | otherwise = object ["entry" .= inputs site,"returns" .= outputs site]
  inputs site
    | site == 0x6ac328 = [readSpec 13 20 False,object ["base" .= object ["stack-word" .= (16::Int)],"count" .= (4::Int)]]
    | site == 0x6ac7d4 = [readSpec 2 4096 False]
    | site == 0x6ac4c8 = [readSpec 2 4 False,readSpec 1 2 False]
    | site == 0x6acab8 = [readSpec 2 4 False,readSpec 1 131072 False]
    | site == 0x6acda0 = [object ["base" .= object ["register" .= (3::Int)],"count" .= (0::Int),"count_register" .= (2::Int)]]
    | otherwise = []
  outputs site
    | site == 0x6ac328 = [object ["base" .= object ["stack-word" .= (16::Int)],"count" .= (4::Int)]]
    | site == 0x6ac4c8 = [readSpec 2 4 False,object ["base" .= object ["register" .= (2::Int)],"count" .= (4096::Int),"indirect" .= True,"on_success" .= True],readSpec 1 2 False]
    | site == 0x6acab8 = [readSpec 1 131072 False,readSpec 2 4 False]
    | site == 0x6acda0 = [object ["base" .= object ["register" .= (3::Int)],"count" .= (0::Int),"count_register" .= (2::Int)]]
    | otherwise = []

parsePostSettlePrefix :: Value -> Parse (LocalUiMenu, LocalUiMode)
parsePostSettlePrefix prefix = do
  exact ["elapsed","state"] prefix
  elapsed <- field "elapsed" prefix
  ticks <- field "ticks" elapsed >>= word64
  need (ticks == tickValue (Tick 833334)) "post-settle prefix differs from its exact tick boundary"
  instructions <- field "instructions" elapsed >>= word64
  ran <- field "ran" elapsed >>= word64
  need (ran <= instructions) "post-settle prefix instruction range is invalid"
  state <- field "state" prefix
  snapshotInstructions <- field "instructions" state >>= word64
  _ <- field "tick" state >>= word64
  need (snapshotInstructions == instructions) "UI snapshot advanced beyond the prefix boundary"
  menu <- field "ui" state >>= parseLocalUiMenu
  mode <- field "source_mode" state >>= parseLocalUiMode
  pure (menu,mode)

foldSteps :: (state -> Value -> Parse state) -> state -> B.ByteString -> Parse state
foldSteps step initial bytes = case lbsToTokens (L.fromStrict bytes) of
  TkArrayOpen items -> loop initial items
  TkErr failure -> Left failure
  _ -> Left "expected steps array"
 where
  loop !state items = case items of
    TkItem tokens -> do
      (value,next) <- toEitherValue tokens
      state' <- step state value
      loop state' next
    TkArrayEnd remaining -> if L.all (`elem` [9,10,13,32]) remaining then Right state else Left "trailing steps data"
    TkArrayErr failure -> Left failure

forceScalar :: a -> ()
forceScalar value = value `seq` ()

forceList :: (a -> ()) -> [a] -> ()
forceList force = foldl' (\() value -> force value) ()

forceMaybe :: (a -> ()) -> Maybe a -> ()
forceMaybe _ Nothing = ()
forceMaybe force (Just value) = force value

forceKeys :: [CrawlKey] -> ()
forceKeys = forceList forceScalar

forceRegions :: Regions -> ()
forceRegions (Regions rows) = forceList (\(Address address, bytes) -> address `seq` B.length bytes `seq` ()) rows

forceMenu :: Menu -> ()
forceMenu NoMenu = ()
forceMenu (Menu node setting cursor) = node `seq` setting `seq` cursor `seq` ()

forcePress :: Press -> ()
forcePress (Press menu ui mode) = forceMenu menu `seq` ui `seq` mode `seq` ()

forceRule :: OutputRule -> ()
forceRule CompareBytes = ()
forceRule PreserveBytes = ()
forceRule UnresolvedBytes = ()
forceRule (CollectionBytes (RecordSize size) (SequenceOffset offset) (ByteCount count)) = size `seq` offset `seq` count `seq` ()

forceOutput :: Output -> ()
forceOutput AbsentOutput = ()
forceOutput (Output rule bytes residue) = forceRule rule `seq` B.length bytes `seq` B.length residue `seq` ()

forceCall :: Call -> ()
forceCall (Call entry arguments returned outputs) =
  entry `seq` forceList forceScalar arguments `seq` forceMaybe forceScalar returned `seq` forceList forceOutput outputs `seq` ()

forceNand :: Nand -> ()
forceNand (Program arguments bytes) = forceList forceScalar arguments `seq` B.length bytes `seq` ()
forceNand (Erase block count) = block `seq` count `seq` ()

forceBlock :: Block -> ()
forceBlock (Block logical mapped bytes spare) =
  logical `seq` forceMaybe forceScalar mapped `seq` B.length bytes `seq` forceMaybe (\extra -> B.length extra `seq` ()) spare `seq` ()

forceMedium :: Medium -> ()
forceMedium (Medium blocks) = forceList forceBlock blocks

forceState :: State -> ()
forceState (State retirement service layouts pending) =
  retirement `seq` service `seq` M.size layouts `seq` M.size pending `seq` ()

forceInterval :: Interval -> ()
forceInterval (Interval name keys calls nand before after press medium) =
  T.length name `seq` forceKeys keys `seq` forceList forceCall calls `seq` forceList forceNand nand `seq`
  forceRegions before `seq` forceRegions after `seq` forceMaybe forcePress press `seq` forceMaybe forceMedium medium `seq` ()

forceQualified :: Qualified -> ()
forceQualified (Qualified _ cohort routes intervals medium) =
  forceList (\(InputName name, Digest digestText) -> T.length name `seq` T.length digestText `seq` ()) cohort
  `seq` length routes `seq` forceList forceInterval intervals `seq` forceMedium medium `seq` ()

qualify :: Role -> Bindings -> Observation -> Parse Qualified
qualify role bindings observation = do
  receipt <- json (receiptBytes observation)
  need (field "schema" receipt == Right (Number 2)) "unsupported receipt schema"
  need (field "endpoint_contract" receipt == Right (String "finite-post-settle-prefix-v2")) "endpoint contract differs"
  need (field "post_settle_prefix_ticks" receipt == Right (A.toJSON (tickValue (Tick 833334)))) "post-settle prefix policy differs"
  need (field "role" receipt == Right (String (if role == Stock then "stock" else "owned"))) "receipt role differs"
  outcome <- field "outcome" receipt >>= field "status" >>= text
  need (outcome == "completed") "observer did not complete"
  need (field "output_rule" receipt == Right (String "Compare outputs written by the stock contract across roles; require each contract-untouched output to equal its own entry preimage, without comparing residue across roles.")) "caller-output preservation rule differs"
  cohort <- validateInputs bindings role receipt
  fileDigests <- field "files" receipt
  exact (map (T.pack . sidecarName) [minBound..maxBound]) fileDigests
  forM_ [minBound..maxBound] $ \file -> do
    bytes <- maybe (Left "missing sidecar") Right (M.lookup file (sidecars observation))
    expected <- field (T.pack (sidecarName file)) fileDigests >>= parseDigest
    need (digest bytes == expected) ("sidecar digest differs: " ++ sidecarName file)
  let load file = maybe (Left "missing sidecar") json (M.lookup file (sidecars observation))
  (code,ram) <- ownedRanges bindings
  extent <- load Extents
  need (field "code" extent == Right (rangesValue code) && field "ram" extent == Right (rangesValue ram)) "owned extents differ from linked symbols"
  exact ["code","ram","unwatched_owned_ram","activation_fetch","rule"] extent
  need (field "unwatched_owned_ram" extent == Right (rangesValue ram)
    && field "activation_fetch" extent == Right (A.toJSON ([0x6ac2b4,0x6ac2b8]::[Word32]))) "ownership-watch exclusions or activation marker differ"
  _ <- field "rule" extent >>= text
  need (field "ram_rule" receipt == Right extent) "receipt RAM rule differs"
  spec <- load Spec
  _ <- load Session
  need (field "captures" spec == Right captureSpec) "record capture declarations differ"
  contract <- json (contractBytes bindings)
  residuals <- field "reference_scan" contract >>= field "raw_candidates" >>= array
  residualSites <- mapM (\row -> field "site" row >>= word) residuals
  let declaredSites = M.fromList ([(site,("residual"::T.Text)) | site <- residualSites]
        ++ [(addressValue (entryAddress entry),"record-entry") | entry <- [minBound..maxBound]]
        ++ [(0x6fb7c8,"nand-write"),(0x6fb57c,"nand-erase")])
      sitesValue = object [K.fromString ("0x" ++ showHex site "") .= label | (site,label) <- M.toList declaredSites]
  need (field "sites" spec == Right sitesValue && field "raw_residual_count" receipt == Right (A.toJSON (length residuals))) "record observation sites differ from carrier reference scan"
  let writes = [(rangeStart range,rangeEnd range) | range <- recordSettingsRanges] ++ retired role
      fetches = guardFetches role code
  need (field "write_ranges" spec == Right (rangesValue writes) && field "read_ranges" spec == Right (rangesValue (retired role)) && field "fetch_ranges" spec == Right (rangesValue fetches)) "observation guard specification differs"
  need (field "hit_limit" spec == Right (Number 1500000) && field "watch_limit" spec == Right (Number 1048576)) "observation limits differ"
  routes <- decodeRouteFile (routeBytes bindings)
  hitsBytes <- maybe (Left "missing hits") Right (M.lookup Hits (sidecars observation))
  hits <- mapM (json >=> parseHit) (C.lines hitsBytes)
  need (length hits < 1500000) "trace limit reached"
  let bridgeSites = [0x1469b20,0x1469b38,0xd806e8,0xd8073c,0x6b0aa8,0x6b0c3c,0x61c490,0x6c6b5c,0x6b0704]
      capturedSites = map (addressValue . entryAddress) [minBound..maxBound] ++ [0x6fb7c8,0x6fb57c]
  forM_ hits $ \(Hit _ address _ _ _ captured) -> case captured of
    Nothing -> need (addressValue address `notElem` capturedSites && (addressValue address `elem` bridgeSites || M.member (addressValue address) declaredSites)) "trace hit lacks required capture or has an undeclared site"
    Just _ -> pure ()
  reset <- load Reset
  resetPoint <- field "reset" reset
  need (field "instructions" resetPoint == Right (Number 0)) "observation did not begin at reset instruction zero"
  initial <- field "regions_at_reset" reset >>= parseRegions (ResetSnapshot role)
  segments <- field "segments" reset >>= array
  need (length segments == 3) "reset phases missing"
  let empty = State BeforeOpen Unopened M.empty M.empty
      slice start end = do
        need (start <= end && end <= length hits) "hit interval outside trace"
        pure (take (end-start) (drop start hits))
      interval (State activation service layouts pending) name keys before after start end press medium watch views = do
        nextActivation <- checkWatch role code activation views watch
        window <- slice start end
        let opens (Hit _ address _ lr _ captured) = address == entryAddress Open && not (within [(0x6ac2b4,0x6ae114)] (addressValue lr) 1)
              && maybe False (\(event,_,_) -> event == Enter) captured
        when (role == Owned && activation == BeforeOpen && any opens window) $
          need (nextActivation == AfterOpen) "open entry lacks its activation fetch"
        (next,calls,nand) <- consume (State nextActivation service layouts pending) window
        let State _ _ _ remaining = next
        need (M.null remaining) "interval ends inside an external record call"
        pure (next,Interval name keys calls nand before after press medium)
  (rootState,rootRegions,rootEnd,phases) <- foldM (\(state,before,start,acc) (phase,segment) -> do
    name <- field "phase" segment >>= text
    need (name == attachPhaseWire phase) "reset phases out of order"
    end <- field "hits_end" segment >>= int
    after <- field "regions" segment >>= parseRegions (ResetSnapshot role)
    watch <- field "watch" segment
    (next,result) <- interval state name [] before after start end Nothing Nothing watch "observe"
    pure (next,after,end,acc ++ [result])) (empty,initial,0,[]) (zip [minBound..maxBound] segments)
  let State rootActivation _ _ _ = rootState
  when (role == Owned) $ need (rootActivation == AfterOpen) "missing first open-veneer fetch"
  attachedSettings <- field "settings_at_attach" receipt >>= parseRegions SettingsSnapshot
  need (attachedSettings == rootRegions) "attach settings differ from reset endpoint"
  attachedMedium <- field "medium_at_attach" receipt >>= parseMedium
  stepsBytes <- maybe (Left "missing steps") Right (M.lookup Steps (sidecars observation))
  let expectedPaths = S.fromList [take n keys | route <- routeList routes, let keys = routeKeys route, n <- [1..length keys]]
  (_,lastEnd,reversedResults,_,paths) <- foldSteps (\(number,start,acc,known,seen) step -> do
    actual <- field "step" step >>= int
    need (actual == number) "steps are not sequential"
    parent <- field "parent" step >>= int
    need (parent < number) "invalid step parent"
    (parentState,parentRegions,parentKeys) <- if parent == 0 then Right (rootState,rootRegions,[]) else maybe (Left "missing step parent") Right (M.lookup parent known)
    keysValue <- field "keys" step
    keys <- case A.fromJSON keysValue of A.Success parsed -> Right parsed; A.Error failure -> Left failure
    need (length keys == length parentKeys + 1 && take (length parentKeys) keys == parentKeys && S.member keys expectedPaths && S.notMember keys seen) "step path differs from routes"
    before <- field "regions_before" step >>= parseRegions SettingsSnapshot
    need (before == parentRegions) "step settings do not start from parent"
    after <- field "regions_after" step >>= parseRegions SettingsSnapshot
    window <- field "hits" step >>= array >>= mapM int
    (first,last') <- case window of [first,last'] | first == start -> Right (first,last'); _ -> Left "step hit intervals do not partition trace"
    ran <- field "idle" step >>= field "ran" >>= word
    need (toInteger ran == toInteger (instructionsValue preKeyIdle)) "pre-key idle differs"
    press <- field "press" step
    need (field "failure" press == Right Null) "key press failed"
    completion <- field "settle" press >>= field "status" >>= text
    need (completion `elem` ["packet-consumed","budget-exhausted"]) ("key injection was not established: " ++ T.unpack completion)
    menu <- field "menu" press >>= parseMenu
    (endpointMenu,endpointMode) <- field "bounded_post_settle_prefix" step >>= parsePostSettlePrefix
    finalMedium <- field "medium_after" step >>= nullable parseMedium
    let leaf = not (any (\candidate -> length candidate > length keys && take (length keys) candidate == keys) (S.toList expectedPaths))
    need (leaf == maybe False (const True) finalMedium) "leaf medium capture missing or misplaced"
    watch <- field "watch" step
    (next,result) <- interval parentState (T.pack (show number)) keys before after first last' (Just (Press menu endpointMenu endpointMode)) finalMedium watch "fixed"
    let acc' = result:acc
        accumulator = (number+1,last',acc',M.insert number (next,after,keys) known,S.insert keys seen)
    forceInterval result `seq` forceState next `seq` forceRegions after `seq` forceKeys keys `seq` acc' `seq` accumulator `seq` pure accumulator) (1,rootEnd,[],M.empty,S.empty) stepsBytes
  let results = reverse reversedResults
  need (paths == expectedPaths && lastEnd == length hits) "routes or trace tail unobserved"
  forM_ [keys | Route _ (KeyRoute keys (Declared CustomResetDefaults)) <- routeList routes] $ \keys -> do
    case [observed | observed@(Interval _ path _ _ _ _ _ _) <- results, path == keys] of
      [Interval _ _ calls requests _ _ (Just (Press _ _ _)) _] ->
        need (any (\call -> case call of Call Append _ (Just 0) _ -> True; _ -> False) calls
          && any (\request -> case request of Program _ _ -> True; _ -> False) requests) "defaults reset did not append a record"
      _ -> Left "defaults reset confirmation did not settle"
  terminal <- load MediumFile >>= parseMedium
  let lastMedium = case reversedResults of
        Interval _ _ _ _ _ _ _ (Just blocks):_ -> blocks
        _ -> attachedMedium
  need (terminal == lastMedium) "terminal medium differs from final leaf"
  let qualified = Qualified role cohort (routeList routes) (phases ++ [Interval "attach-medium" [] [] [] rootRegions rootRegions Nothing (Just attachedMedium)] ++ results) terminal
  forceQualified qualified `seq` pure qualified
 where
  (>=>) first second value = first value >>= second

coverageValue :: Qualified -> Value
coverageValue (Qualified role cohort _ intervals _) = object
  [ "role" .= (if role == Stock then "stock" else "owned" :: T.Text)
  , "record_nand_sha256" .= fmap (\(Digest value) -> value) (lookup (InputName "record-nand") cohort)
  , "external_entries" .= object [K.fromText (entryName entry) .= length [() | Call called _ _ _ <- calls, called == entry] | entry <- [minBound..maxBound]]
  , "external_entry_total" .= length calls
  , "nand" .= object ["program" .= length [() | Program _ _ <- requests],"erase" .= length [() | Erase _ _ <- requests]]
  ]
 where
  calls = concat [observed | Interval _ _ observed _ _ _ _ _ <- intervals]
  requests = concat [observed | Interval _ _ _ observed _ _ _ _ <- intervals]
  entryName entry = case entry of
    Open -> "record_store_open"
    Register -> "record_store_register"
    Close -> "record_store_close"
    Latest -> "record_store_read_latest"
    Append -> "record_store_append"
    EraseAll -> "record_store_erase_all"
    Collect -> "record_store_collect"
    ReadAt -> "record_store_read_at"
    Zero -> "record_store_zero"

comparisonValue :: ComparisonContract -> Qualified -> Qualified -> Either Difference [ImageSizeCaseResult] -> Value
comparisonValue contract stock owned result = object
  (["schema" .= (2::Int),"comparison_contract" .= contractValue,"coverage" .= object ["stock" .= coverageValue stock,"owned" .= coverageValue owned]] ++ case result of
    Right cases -> ["outcome" .= (if any stalled cases then "accepted-functional-difference" else "matched"::T.Text),"intended_cases" .= map caseValue cases]
    Left (Difference path expected actual) -> ["outcome" .= ("different"::T.Text),"path" .= path,"stock" .= expected,"owned" .= actual]
    Left (AmbiguousOutput path expected actual) -> ["outcome" .= ("refused"::T.Text),"error" .= ("ambiguous stock output-write effect"::T.Text),"path" .= path,"stock" .= expected,"owned" .= actual]
    Left (PreservationViolation path before after) -> ["outcome" .= ("refused"::T.Text),"error" .= ("contract-untouched owned output changed"::T.Text),"path" .= path,"before" .= before,"after" .= after]
    Left (OracleViolation role path expected actual) -> ["outcome" .= ("different"::T.Text),"error" .= ("intended image-size oracle"::T.Text),"role" .= roleValue role,"path" .= path,"expected" .= expected,"actual" .= actual])
 where
  roleValue Stock = "stock" :: T.Text
  roleValue Owned = "owned"
  contractValue = case contract of
    StockEquality (Digest identity) -> object ["policy" .= ("stock-equality-v1"::T.Text),"sha256" .= identity]
    ImageSizePendingChoice (Digest identity) (SourceEvidence (EvidencePath path) (Digest evidence)) ->
      let Digest fixture = imageSizeFixture in object
        ["policy" .= ("image-size-pending-choice-v1"::T.Text),"sha256" .= identity,"record_nand_sha256" .= fixture
        ,"source_evidence" .= object ["path" .= path,"sha256" .= evidence]]
  stalled (ImageSizeCaseResult _ reference) = reference == ReferenceStalled
  caseValue (ImageSizeCaseResult action reference) = object
    ["crawl_trial" .= imageSizeTrial action,"keys" .= imageSizeKeys action
    ,"owned" .= ("intended"::T.Text),"stock" .= (if reference == ReferenceStalled then "known-stalled-down-state" else "intended"::T.Text)
    ,"expected_choice" .= object ["size" .= ("L"::T.Text),"aspect" .= ("3:2"::T.Text)]
    ,"public_property" .= (0xf6::Word32),"ordinary_properties" .= ([0xf6,0xf9]::[Word32])
    ,"property_161" .= object
      ["classification" .= ("derived-image-size-synchronization"::T.Text),"before" .= (1::Int)
      ,"after" .= (case action of ImageSizeUp -> 1; ImageSizeBack -> 0::Int)
      ,"native_leave_restore" .= (action == ImageSizeBack)]]
showValue :: Show a => a -> Value
showValue = String . T.pack . show
comparePair :: ComparisonContract -> Qualified -> Qualified -> Either Difference [ImageSizeCaseResult]
comparePair contract (Qualified Stock stockInputs stockRoutes stock stockMedium) (Qualified Owned ownedInputs ownedRoutes owned ownedMedium) = do
  equal "cohort" stockInputs ownedInputs
  equal "interval-count" (length stock) (length owned)
  cases <- case contract of
    StockEquality _ -> pure []
    ImageSizePendingChoice _ _ -> case lookup (InputName "record-nand") stockInputs of
      Nothing -> pure []
      Just fixture -> do
        equal "image-size-oracle.fixture" imageSizeFixture fixture
        retained <- baseline Stock stockRoutes stock
        candidate <- baseline Owned ownedRoutes owned
        forM [ImageSizeUp,ImageSizeBack] $ \action -> do
          retainedUi <- findInterval Stock (imageSizeKeys action) stock >>= projection Stock
          candidateUi <- findInterval Owned (imageSizeKeys action) owned >>= projection Owned
          let target (ImageSizeBaseline parent entry _) = case action of ImageSizeUp -> entry; ImageSizeBack -> parent
              ImageSizeBaseline _ _ retainedPending = retained
          expectProjection Owned "image-size-oracle.owned" (target candidate) candidateUi
          reference <- if retainedUi == target retained then pure ReferenceIntended
            else if retainedUi == retainedPending then pure ReferenceStalled
            else Left (OracleViolation Stock "image-size-oracle.stock" (showValue [target retained,retainedPending]) (showValue retainedUi))
          pure (ImageSizeCaseResult action reference)
  forM_ (zip3 [0::Int ..] stock owned) $ \(number,Interval name keys calls nand before after press medium,Interval other otherKeys otherCalls otherNand otherBefore otherAfter otherPress otherMedium) -> do
    let path suffix = "intervals." <> T.pack (show number) <> "." <> suffix
    equal (path "identity") (name,keys) (other,otherKeys)
    orderedWith compareCall (path "calls") calls otherCalls
    orderedWith compareNand (path "nand") nand otherNand
    compareRegions (path "settings-before") before otherBefore
    unless (any (\(ImageSizeCaseResult action _) -> keys == imageSizeKeys action) cases) $ do
      compareRegions (path "settings-after") after otherAfter
      equal (path "press") (fmap (\(Press menu _ _) -> menu) press) (fmap (\(Press menu _ _) -> menu) otherPress)
      equal (path "endpoint-menu") (fmap (\(Press _ menu _) -> menu) press) (fmap (\(Press _ menu _) -> menu) otherPress)
    equal (path "endpoint-source-mode") (fmap (\(Press _ _ mode) -> mode) press) (fmap (\(Press _ _ mode) -> mode) otherPress)
    compareMaybeMedium (path "medium") medium otherMedium
  compareMedium "terminal-medium" stockMedium ownedMedium
  pure cases
 where
  asOracle role result = case result of
    Left (Difference path expected actual) -> Left (OracleViolation role path expected actual)
    other -> other
  oracleEqual role path expected actual = asOracle role (equal path expected actual)
  findInterval role keys intervals = case [interval | interval@(Interval _ path _ _ _ _ _ _) <- intervals, path == keys] of
    [interval] -> pure interval
    found -> Left (OracleViolation role "image-size-oracle.interval" (A.toJSON keys) (showValue found))
  projection _ (Interval _ _ _ _ _ after (Just (Press menu endpoint _)) _) = pure (UiProjection after menu endpoint)
  projection role interval = Left (OracleViolation role "image-size-oracle.press" (String "key interval") (showValue interval))
  expectProjection role path (UiProjection expected menu endpoint) (UiProjection actual otherMenu otherEndpoint) = do
    asOracle role (compareRegions (path <> ".settings") expected actual)
    oracleEqual role (path <> ".press") menu otherMenu
    oracleEqual role (path <> ".endpoint") endpoint otherEndpoint
  expectFrame role frame (UiProjection _ menu endpoint) = do
    let setting = SettingIdentifier (if frame == ParentFrame then 5 else 246)
        cursor = MenuCursor (case frame of ParentFrame -> 3; EntryFrame -> 0; PendingFrame -> 1)
        node = MenuNodeAddress (if frame == ParentFrame then 0x78ea98 else 0x784014)
        depth = MenuDepth (if frame == ParentFrame then 1 else 2)
        SettingIdentifier settingWord = setting
        MenuCursor cursorWord = cursor
    oracleEqual role "image-size-oracle.press-frame" (Menu 1 settingWord cursorWord) menu
    oracleEqual role "image-size-oracle.endpoint-frame"
      (LocalUiMenu node (Just (MenuIdentifier 1)) (Just setting) cursor depth) endpoint
  expectChoice role choice (Regions regions) = forM_ regions $ \(address,image) -> do
    oracleEqual role "image-size-oracle.image-extent" True (B.length image >= 450)
    let size = case choice of LargeThreeTwo -> 1; MediumThreeTwo -> 2
    oracleEqual role ("image-size-oracle.ordinary-f6." <> T.pack (show address)) 0 ((B.index image 0x6f `shiftR` 5) .&. 3)
    oracleEqual role ("image-size-oracle.ordinary-f9." <> T.pack (show address)) size ((B.index image 0x7d `shiftR` 5) .&. 3)
  expectTransition role transition (Regions before) (Regions after) = asOracle role (orderedWith transitionRegion "image-size-oracle.transition" before after)
   where
    transitionRegion path (address,old) (other,new) = do
      equal (path <> ".address") address other
      equal (path <> ".length") (B.length old) (B.length new)
      let offset = case transition of EnterImageSize -> 0x6e; PreviewMedium -> 0x7d
          prior = case transition of EnterImageSize -> 0; PreviewMedium -> 1
          next = case transition of EnterImageSize -> 1; PreviewMedium -> 2
      equal (path <> ".prior-setting") prior ((B.index old offset `shiftR` 5) .&. 3)
      forM_ [0..B.length old-1] $ \index -> do
        let original = B.index old index
            expected = if index == offset then original .&. 0x9f .|. (next `shiftL` 5) else original
        equal (path <> ".bytes." <> T.pack (show index)) expected (B.index new index)
  baseline role routes intervals = do
    forM_ [ImageSizeUp,ImageSizeBack] $ \action -> do
      let expected = Route (RouteName (case action of ImageSizeUp -> "crawl-trial-00368"; ImageSizeBack -> "crawl-trial-00371"))
            (KeyRoute (imageSizeKeys action) (CrawlTrial (imageSizeTrial action) []))
          declared = [route | route@(Route _ (KeyRoute _ (CrawlTrial trial _))) <- routes, trial == imageSizeTrial action]
      oracleEqual role "image-size-oracle.declaration" [expected] declared
    root@(Interval _ _ _ _ rootBefore rootAfter _ _) <- findInterval role [KeyOk] intervals
    _ <- projection role root
    parent <- findInterval role [KeyOk,KeyDown] intervals >>= projection role
    entry <- findInterval role [KeyOk,KeyDown,KeyOk] intervals >>= projection role
    pending <- findInterval role [KeyOk,KeyDown,KeyOk,KeyDown] intervals >>= projection role
    let UiProjection parentSettings _ _ = parent
        UiProjection entrySettings _ _ = entry
        UiProjection pendingSettings _ _ = pending
    asOracle role (compareRegions "image-size-oracle.root-entry" rootBefore rootAfter)
    asOracle role (compareRegions "image-size-oracle.backup-baseline" rootAfter parentSettings)
    case parentSettings of
      Regions regions -> case lookup (Address 0x96cc64) regions of
        Just image -> do
          oracleEqual role "image-size-oracle.bank-pair-extent" 900 (B.length image)
          forM_ [0..449] $ \index -> oracleEqual role ("image-size-oracle.published-backup." <> T.pack (show index))
            (B.index image index) (B.index image (index+450))
        Nothing -> Left (OracleViolation role "image-size-oracle.bank-pair" (String "0x96cc64") Null)
    expectFrame role ParentFrame parent
    expectFrame role EntryFrame entry
    expectFrame role PendingFrame pending
    expectChoice role LargeThreeTwo parentSettings
    expectChoice role LargeThreeTwo entrySettings
    expectChoice role MediumThreeTwo pendingSettings
    expectTransition role EnterImageSize parentSettings entrySettings
    expectTransition role PreviewMedium entrySettings pendingSettings
    pure (ImageSizeBaseline parent entry pending)
  equal :: (Eq a, Show a) => T.Text -> a -> a -> Either Difference ()
  equal path a b = unless (a == b) (Left (Difference path (showValue a) (showValue b)))
  orderedWith :: (T.Text -> a -> a -> Either Difference ()) -> T.Text -> [a] -> [a] -> Either Difference ()
  orderedWith compareItem path a b = do
    equal (path <> ".length") (length a) (length b)
    forM_ (zip3 [0::Int ..] a b) $ \(index,x,y) -> compareItem (path <> "." <> T.pack (show index)) x y
  bytes path a b = do
    equal (path <> ".length") (B.length a) (B.length b)
    let first index
          | index == B.length a = Nothing
          | B.index a index /= B.index b index = Just index
          | otherwise = first (index+1)
    case first 0 of
      Nothing -> pure ()
      Just index -> equal (path <> "." <> T.pack (show index)) (B.index a index) (B.index b index)
  compareOutput _ AbsentOutput AbsentOutput = pure ()
  compareOutput path (Output rule before after) (Output _ otherBefore otherAfter) = case rule of
    CompareBytes -> bytes path after otherAfter
    PreserveBytes -> preserveOwned path otherBefore otherAfter
    _ -> do
      equal (path <> ".length") (B.length after) (B.length otherAfter)
      equal (path <> ".stock-preimage-length") (B.length before) (B.length after)
      equal (path <> ".owned-preimage-length") (B.length otherBefore) (B.length otherAfter)
      forM_ [0..B.length after-1] $ \index -> do
        let at = path <> "." <> T.pack (show index)
            old = B.index before index
            value = B.index after index
            otherOld = B.index otherBefore index
            otherValue = B.index otherAfter index
            written = case rule of
              CollectionBytes (RecordSize stride) (SequenceOffset offset) (ByteCount prefix) ->
                index < prefix || index `mod` stride >= offset && index `mod` stride < offset+2
              _ -> False
        if written || old /= value then equal at value otherValue
        else if value == otherValue && otherOld == otherValue then pure ()
        else if value /= otherValue && otherOld /= otherValue then equal at value otherValue
        else Left (AmbiguousOutput at (object ["before" .= old,"after" .= value]) (object ["before" .= otherOld,"after" .= otherValue]))
  compareOutput path a b = equal (path <> ".present") (present a) (present b)
  present AbsentOutput = False
  present (Output _ _ _) = True
  preserveOwned path before after
    | B.length before /= B.length after = Left (PreservationViolation (path <> ".length") (A.toJSON (B.length before)) (A.toJSON (B.length after)))
    | otherwise = loop 0
   where
    loop index
      | index == B.length before = pure ()
      | B.index before index == B.index after index = loop (index+1)
      | otherwise = Left (PreservationViolation (path <> "." <> T.pack (show index)) (A.toJSON (B.index before index)) (A.toJSON (B.index after index)))
  compareCall path (Call entry args result buffers) (Call other otherArgs otherResult otherBuffers) = do
    equal (path <> ".entry") entry other
    equal (path <> ".arguments") args otherArgs
    equal (path <> ".result") result otherResult
    orderedWith compareOutput (path <> ".buffers") buffers otherBuffers
  compareNand path (Program descriptor payload) (Program other otherPayload) = do
    equal (path <> ".descriptor") descriptor other
    bytes (path <> ".payload") payload otherPayload
  compareNand path (Erase logical count) (Erase other otherCount) = equal path (logical,count) (other,otherCount)
  compareNand path a b = equal (path <> ".operation") (nandKind a) (nandKind b)
  nandKind (Program _ _) = True
  nandKind (Erase _ _) = False
  compareRegions path (Regions a) (Regions b) = orderedWith region path a b
  region path (address,a) (other,b) = equal (path <> ".address") address other >> bytes (path <> ".bytes") a b
  compareMedium path (Medium a) (Medium b) = orderedWith block path a b
  block path (Block logical mapped payload spare) (Block other otherMapped otherPayload otherSpare) = do
    equal (path <> ".logical") logical other
    equal (path <> ".physical") mapped otherMapped
    bytes (path <> ".data") payload otherPayload
    case (spare,otherSpare) of
      (Just x,Just y) -> bytes (path <> ".spare") x y
      _ -> equal (path <> ".spare-present") (fmap B.length spare) (fmap B.length otherSpare)
  compareMaybeMedium path (Just a) (Just b) = compareMedium path a b
  compareMaybeMedium path a b = equal (path <> ".present") (fmap (const True) a) (fmap (const True) b)
comparePair _ _ _ = Left (Difference "roles" (String "stock") (String "owned"))
