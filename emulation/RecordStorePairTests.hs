{-# LANGUAGE OverloadedStrings #-}
module Main (main) where

import Control.Monad (unless)
import Data.Aeson (Value (..), object, (.=))
import qualified Data.Aeson as A
import qualified Data.Aeson.Key as K
import qualified Data.Aeson.KeyMap as KM
import Data.Bits (shiftR)
import qualified Data.ByteString as B
import qualified Data.ByteString.Builder as BB
import qualified Data.ByteString.Char8 as C
import qualified Data.ByteString.Lazy as BL
import Data.Either (isLeft, isRight)
import qualified Data.Map.Strict as M
import qualified Data.Text as T
import qualified Data.Text.Encoding as TE
import Data.Word (Word32)
import Emulation.LocalUiMenu (MenuNodeAddress (..), SettingIdentifier (..), MenuCursor (..), MenuDepth (..))
import Emulation.PropertyPairPlan (PhysicalRange (..), settingsRanges)
import Emulation.PropertyPairRoutes
import Emulation.RecordStorePairCheck
import Emulation.Sha256 (sha256, sha256Hex)
import EmulatorUsb (CrawlKey (..))
import Numeric (showHex)
import System.Exit (exitFailure)

data Mutation = Original | PayloadByte | ReorderCalls | StrayFetch | ForeignRead | SavedDestinationWrite | ObsoleteTagWrite | BadSettings | BadMedium | MissingCapture | BadBinding | CollectionTail | ReturnedRecord | CloseScratch | OutputStorage | MissingResetRegion | RetiredSnapshotDifference | PreEntryPublication | PostEntryPublication | PreEntryBssWrite | PreEntryRead | MissingMarker | LaterPhaseWrite | MappedMedium | TruncatedSpare | ChangedSpareByte | PreservedLatest | ChangedPreservedPointer | ClearedPreservedPointer | ChangedPreservedSkipped | WrittenNullLatest | MissingWrittenNullLatest | ZeroSkippedLatest | ChangedZeroSkippedPreimage | PreservedRegister | ChangedPreservedRegister | InnerFailedRegister | UnopenedRegister | UnopenedRegisterNonNull | OutOfBankRead | ChangedOutOfBankTail | InvalidKindRead | FailedCollect | ChangedFailedCollectCount | ChangedFailedCollectBuffer deriving (Eq)

data RecordSequence = WholeService | OpenAndRegister | ReadAndAppend

changedObject :: K.Key -> Value -> Value -> Value
changedObject name value (Object fields) = Object (KM.insert name value fields)
changedObject _ _ value = value

rejectBinding :: (Value -> Value) -> (B.ByteString -> B.ByteString) -> Bool
rejectBinding changeContract changeSymbols =
  case (A.eitherDecodeStrict' (contractBytes bindings), A.eitherDecodeStrict' (receiptBytes original)) of
    (Right contract,Right receipt) ->
      let symbols = changeSymbols (symbolBytes bindings)
          newContract = encode (changeContract (changedObject "symbols_sha256" (A.toJSON (hash symbols)) contract))
          bound = bindings { contractBytes = newContract, symbolBytes = symbols }
          binding bytes = object ["path" .= ("fixture"::T.Text),"sha256" .= hash bytes]
          inputs = case receipt of Object fields -> KM.lookup "inputs" fields; _ -> Nothing
       in case inputs of
            Just values ->
              let updated = changedObject "contract" (binding newContract) (changedObject "symbols" (binding symbols) values)
               in isLeft (qualify Owned bound original { receiptBytes = encode (changedObject "inputs" updated receipt) })
            _ -> False
    _ -> False
  where original = fixture Owned Original

rejectOwnership :: Value -> Bool
rejectOwnership ram =
  case A.eitherDecodeStrict' (receiptBytes original) of
    Right receipt@(Object fields) ->
      let extent = changedObject "ram" ram (changedObject "unwatched_owned_ram" ram extents)
          bytes = encode extent
          files = case KM.lookup "files" fields of
            Just value -> changedObject (K.fromString (sidecarName Extents)) (A.toJSON (hash bytes)) value
            Nothing -> Null
          updated = changedObject "files" files (changedObject "ram_rule" extent receipt)
       in isLeft (qualify Owned bindings original { receiptBytes = encode updated, sidecars = M.insert Extents bytes (sidecars original) })
    _ -> False
  where original = fixture Owned Original
encode :: Value -> B.ByteString
encode = BL.toStrict . A.encode
hash :: B.ByteString -> String
hash = sha256Hex . sha256
hex :: B.ByteString -> T.Text
hex = TE.decodeUtf8 . BL.toStrict . BB.toLazyByteString . BB.byteStringHex
wordBytes :: Word32 -> B.ByteString
wordBytes value = B.pack [fromIntegral (value `shiftR` n) | n <- [0,8,16,24]]
hexWord :: Word32 -> String
hexWord value = "0x" ++ showHex value ""
entries :: [Word32]
entries = [0x6ac2b4,0x6ac328,0x6ac478,0x6ac4c8,0x6ac7d4,0x6ac984,0x6acab8,0x6acda0,0x6ae0fc]
code :: [[Word32]]
code = [[entry,entry+4] | entry <- entries]
fetches :: Role -> [[Word32]]
fetches Stock = [[0x6ac2b4,0x6ae114]]
fetches Owned = [0x6ac2b4,0x6ac328] : zipWith (\low high -> [low,high]) (map (+4) (drop 1 entries)) (drop 2 entries ++ [0x6ae114])
retired :: Role -> [[Word32]]
retired _ = [[0x89f8f0,0x89f8f8],[0x917fa8,0x917fe4]]
writes :: Role -> [[Word32]]
writes role = [[rangeStart range,rangeEnd range] | range <- settingsRanges] ++ retired role
extents :: Value
extents = object ["code" .= code,"ram" .= ram,"unwatched_owned_ram" .= ram,"activation_fetch" .= ([0x6ac2b4,0x6ac2b8]::[Word32]),"rule" .= ("fixture"::T.Text)] where
  ram = [[0x89f8f8,0x89fa60],[0x89fa60,0x89fa68],[0x8a0080,0x8e0080],[0x8e0080,0x8e008c]]::[[Word32]]
bindings :: Bindings
bindings = Bindings contract (B.replicate 7776 0) symbols routes where
  symbols = C.pack (unlines ([showHex entry "" ++ " 4 T entry" ++ show n | (n,entry) <- zip [0::Int ..] entries] ++ [showHex address " 0 A " ++ name | (name,address) <- cacheSymbolLayout]))
  routes = BL.toStrict (encodeRouteFile (RouteFile (replicate 64 '0') "fixture" CrawlEffectsSource [] phaseRoutes))
  contract = encode (object ["source_sha256" .= hash "stock-DAT","candidate_sha256" .= hash "owned-DAT","linked_sha256" .= hash (B.replicate 7776 0),"symbols_sha256" .= hash symbols,"owned_cache" .= cacheContract,"reference_scan" .= object ["raw_candidates" .= ([]::[Value])]])
regions :: Mutation -> Value
regions = A.toJSON . settingRows
settingRows :: Mutation -> [Value]
settingRows mutation = [object ["start" .= rangeStart range,"bytes" .= hex (B.replicate (fromIntegral (rangeEnd range-rangeStart range)) (if mutation == BadSettings then 1 else 0))] | range <- settingsRanges]
resetRegions :: Role -> Mutation -> Value
resetRegions role mutation = A.toJSON (settingRows mutation ++
  [object ["start" .= low,"bytes" .= hex (B.replicate (fromIntegral (high-low)) (if mutation == RetiredSnapshotDifference then 1 else 0))]
  | [low,high] <- if mutation == MissingResetRegion then drop 1 (retired role) else retired role])
medium :: Mutation -> Value
medium mutation = if mutation `elem` [BadMedium,MappedMedium,TruncatedSpare,ChangedSpareByte] then mediumValue mutation else erasedMedium
erasedMedium :: Value
erasedMedium = mediumValue Original
mediumValue :: Mutation -> Value
mediumValue mutation = A.toJSON
  [object ["logical" .= logical,"physical" .= (if mapped logical then A.toJSON (0x380::Word32) else Null)
    ,"data" .= (if mutation == BadMedium && logical == 0x54 then zeroBlock else erasedBlock)
    ,"spare" .= (if mapped logical then A.toJSON spare else Null)]
  | logical <- ([0x52..0x59] ++ [0x68,0x69]::[Word32])]
 where
  mapped logical = logical == 0x54 && mutation `elem` [MappedMedium,TruncatedSpare,ChangedSpareByte]
  spare = hex (B.cons (if mutation == ChangedSpareByte then 0 else 255) (B.replicate ((if mutation == TruncatedSpare then 4096 else 32768)-1) 255))
  zeroBlock = hex (B.replicate 131072 0)
  erasedBlock = hex (B.replicate 131072 255)
readback :: Word32 -> B.ByteString -> Value
readback address bytes = object ["address" .= address,"bytes" .= hex bytes]
hit :: Word32 -> Word32 -> Word32 -> [Word32] -> T.Text -> [Value] -> Value
hit entry address lr args event captures = object
  ["site" .= ("record-entry"::T.Text),"address" .= hexWord address,"core" .= (0::Int),"instructions" .= (10::Int)
  ,"registers" .= object [K.fromString name .= hexWord value | (name,value) <- zip ["r0","r1","r2","r3"] args]
  ,"sp" .= hexWord 0x930000,"lr" .= hexWord lr,"cpsr" .= (0::Int)
  ,"observation" .= object ["event" .= event,"site" .= entry,"reads" .= captures]]
call :: Word32 -> [Word32] -> [Value] -> Word32 -> [Value] -> [Value]
call entry args input result output = [hit entry entry 0x700100 args "entry" input,hit entry 0x700100 0x700100 [result,0,0,0] "return" output]
trace :: Role -> Mutation -> RecordSequence -> [Value]
trace role mutation scenario = case scenario of
  OpenAndRegister -> open ++ register
  ReadAndAppend -> latest ++ append
  WholeService
    | registerFailure -> open ++ register
    | otherwise -> open ++ register ++ latest ++ order ++ erase ++ collect ++ positional ++ close
 where
  residue = if role == Stock then 0x11112222 else 0x33334444
  registerFailure = mutation `elem` [PreservedRegister,ChangedPreservedRegister,InnerFailedRegister,UnopenedRegister,UnopenedRegisterNonNull]
  unopened = mutation `elem` [UnopenedRegister,UnopenedRegisterNonNull]
  open = call 0x6ac2b4 [123,0,0,0] [] (if unopened then 0xfffffff2 else 0) []
  registerStatus | mutation == InnerFailedRegister = 0xffffffe2
                 | registerFailure = 0xfffffff2
                 | otherwise = 0
  clientAfter | mutation `elem` [UnopenedRegister,ChangedPreservedRegister] = 0
              | registerFailure = residue
              | otherwise = 0x89f8b4
  register = call 0x6ac328 [0,96,16,0]
    [readback 0x930000 (B.concat (map wordBytes [0x710000,0x910000,0x710004,0x910004,0x910008])),readback 0x910008 (wordBytes residue)]
    registerStatus [readback 0x910008 (wordBytes clientAfter)]
  pointerStorage = if mutation == OutputStorage then 0x910030 else 0x910010
  failedLatest = mutation `elem` [PreservedLatest,ChangedPreservedPointer,ClearedPreservedPointer,ChangedPreservedSkipped,WrittenNullLatest,MissingWrittenNullLatest,ZeroSkippedLatest,ChangedZeroSkippedPreimage]
  skippedPointer = if failedLatest then 0x910028 else 0
  pointerBefore = if role == Stock then 0x817978 else 0x400572cc
  pointerAfter | mutation `elem` [ClearedPreservedPointer,WrittenNullLatest] = 0
               | mutation == ChangedPreservedPointer = pointerBefore+4
               | failedLatest = pointerBefore
               | otherwise = 0x8e16a0
  zeroSkipped = mutation `elem` [ZeroSkippedLatest,ChangedZeroSkippedPreimage]
  skippedBefore | mutation == ChangedZeroSkippedPreimage = 1
                | zeroSkipped = 0
                | otherwise = 129
  skippedAfter = if mutation `elem` [ChangedPreservedSkipped,WrittenNullLatest,MissingWrittenNullLatest] || zeroSkipped then 0 else skippedBefore
  skippedBytes value = if skippedPointer == 0 then B.empty else B.take 2 (wordBytes value)
  latest = call 0x6ac4c8 [0x89f8b4,skippedPointer,pointerStorage,0]
    [readback pointerStorage (wordBytes pointerBefore),readback skippedPointer (skippedBytes skippedBefore)]
    (if failedLatest then 0xffffff01 else 0)
    [readback pointerStorage (wordBytes pointerAfter),readback pointerAfter (if failedLatest then B.empty else B.cons (if mutation == ReturnedRecord then 18 else 17) (B.replicate 4095 17)),readback skippedPointer (skippedBytes skippedAfter)]
  append = [hit 0x6ac7d4 0x6ac7d4 0x700100 [0x89f8b4,1,0x910100,0] "entry" [readback 0x910100 (B.replicate 4096 34)]]
    ++ [hit 0x6fb7c8 0x6fb7c8 0x6a8d00 [0x910200,0,0,0] "entry" [readback 0x910200 (B.concat (map wordBytes [0x3d4,0,96,0x408e16a0])),readback 0x8e16a0 (B.cons (if mutation == PayloadByte then 50 else 51) (B.replicate 95 51))]]
    ++ [hit 0x6ac7d4 0x700100 0x700100 [0,0,0,0] "return" []]
  zero = call 0x6ae0fc [99,98,97,96] [] 0 []
  order = if mutation == ReorderCalls then zero ++ append else append ++ zero
  erase = [hit 0x6ac984 0x6ac984 0x700100 [0x89f8b4,0,0,0] "entry" []
    ,hit 0x6fb57c 0x6fb57c 0x6a8c40 [0x910200,0,0,0] "entry" [readback 0x910200 (wordBytes 0x3d4 <> wordBytes 1)]
    ,hit 0x6ac984 0x700100 0x700100 [0,0,0,0] "return" []]
  collected = B.replicate 1535 68 <> B.singleton (if mutation == CollectionTail then 69 else 68) <> B.replicate (131072-1536) 68
  collectFailure = mutation `elem` [FailedCollect,ChangedFailedCollectCount,ChangedFailedCollectBuffer]
  collectAfter = if mutation == ChangedFailedCollectBuffer then B.cons 69 (B.drop 1 collected) else collected
  countBefore = if collectFailure then residue else 0
  countAfter = if mutation == ChangedFailedCollectCount then 0 else if collectFailure then countBefore else 96
  collect = call 0x6acab8 [0x89f8b4,0x920000,0x910014,0]
    [readback 0x910014 (wordBytes countBefore),readback 0x920000 (B.replicate 131072 68)]
    (if collectFailure then 0xffffff01 else 0) [readback 0x920000 collectAfter,readback 0x910014 (wordBytes countAfter)]
  outside = mutation `elem` [OutOfBankRead,ChangedOutOfBankTail]
  readBefore = B.pack (if role == Stock then [7,8,9,10] else [17,18,19,20])
  readAfter | mutation == InvalidKindRead = B.replicate 4 0
            | mutation == ChangedOutOfBankTail = B.pack [0,0,19,20]
            | outside = B.cons 0 (B.drop 1 readBefore)
            | otherwise = B.pack [1,2,3,4]
  positional = call 0x6acda0 [if mutation == InvalidKindRead then 5 else 0,if outside then 0x40000 else 32,4,0x910020]
    [readback 0x910020 readBefore] (if outside || mutation == InvalidKindRead then 0xffffffe2 else 0) [readback 0x910020 readAfter]
  close = call 0x6ac478 [0x89f8b4,0,0,0] [] (if mutation == CloseScratch then 999 else 123) []

captureDeclarations :: Value
captureDeclarations = object [K.fromString (hexWord entry) .= declared entry | entry <- entries ++ [0x6fb7c8,0x6fb57c]] where
  direct reg count = object ["base" .= object ["register" .= (reg::Int)],"count" .= (count::Int),"indirect" .= False]
  declared entry
    | entry == 0x6fb7c8 = object ["entry" .= [direct 0 16,object ["base" .= object ["register-word" .= object ["register" .= (0::Int),"offset" .= (12::Int)]],"count" .= (0::Int),"count_base" .= object ["register-word" .= object ["register" .= (0::Int),"offset" .= (8::Int)]]]]]
    | entry == 0x6fb57c = object ["entry" .= [direct 0 8]]
    | otherwise = object ["entry" .= inputs entry,"returns" .= outputs entry]
  inputs entry | entry == 0x6ac328 = [direct 13 20,object ["base" .= object ["stack-word" .= (16::Int)],"count" .= (4::Int)]]
               | entry == 0x6ac7d4 = [direct 2 4096]
               | entry == 0x6ac4c8 = [direct 2 4,direct 1 2]
               | entry == 0x6acab8 = [direct 2 4,direct 1 131072]
               | entry == 0x6acda0 = [object ["base" .= object ["register" .= (3::Int)],"count" .= (0::Int),"count_register" .= (2::Int)]]
               | otherwise = []
  outputs entry
    | entry == 0x6ac328 = [object ["base" .= object ["stack-word" .= (16::Int)],"count" .= (4::Int)]]
    | entry == 0x6ac4c8 = [direct 2 4,object ["base" .= object ["register" .= (2::Int)],"count" .= (4096::Int),"indirect" .= True,"on_success" .= True],direct 1 2]
    | entry == 0x6acab8 = [direct 1 131072,direct 2 4]
    | entry == 0x6acda0 = [object ["base" .= object ["register" .= (3::Int)],"count" .= (0::Int),"count_register" .= (2::Int)]]
    | otherwise = []

watch :: Role -> AttachPhase -> Mutation -> Value
watch role phase mutation = object ["enabled" .= True,"ranges" .= ordered,"records" .= records,"violation" .= Null,"views" .= ("observe"::T.Text),"classes" .= classes] where
  ordered = fetches role ++ take 1 (writes role) ++ retired role ++ drop 1 (take 3 (writes role))
  classes = [object ["range" .= range,"kinds" .= (["cpu-write","host-write","model-bus-write"]::[T.Text])] | range <- writes role]
    ++ [object ["range" .= range,"kinds" .= (["cpu-read"]::[T.Text])] | range <- retired role]
    ++ [object ["range" .= range,"kinds" .= (["cpu-fetch"]::[T.Text])] | range <- fetches role]
  records = before ++ marker ++ after
  marker = [cpu "cpu-fetch" 0x6ac2b4 0x6ac2b4 Null Null | phase == FirstStartPhase && mutation /= MissingMarker]
  before
    | phase /= FirstStartPhase = []
    | mutation == PreEntryPublication = [publication]
    | mutation == PreEntryBssWrite = [store]
    | mutation == PreEntryRead = [load]
    | otherwise = []
  after
    | phase == LiveViewPhase && mutation == LaterPhaseWrite = [store]
    | phase /= FirstStartPhase = []
    | otherwise = case mutation of
        StrayFetch -> [cpu "cpu-fetch" 0x6ac2c0 0x6ac2c0 Null Null]
        ForeignRead -> [load]
        SavedDestinationWrite -> [store]
        ObsoleteTagWrite -> [cpu "cpu-write" 0x89f8f0 0x700000 (A.toJSON (0::Int)) (A.toJSON ([0,0,0,0]::[Int]))]
        PostEntryPublication -> [publication]
        _ -> []
  load = cpu "cpu-read" 0x89f8f4 0x700000 (A.toJSON (0::Int)) (A.toJSON ([0,0,0,0]::[Int]))
  store = cpu "cpu-write" 0x917fa8 0x69ce00 (A.toJSON (0::Int)) (A.toJSON ([0,0,0,0]::[Int]))
  publication = object ["source" .= ("model-bus-write"::T.Text),"address" .= (0x89ec00::Word32),"physical" .= (0x89ec00::Word32),"size" .= (32768::Int),"core" .= Null,"pc" .= Null,"instructions" .= Null,"tick" .= Null]
  cpu source address pc value beforeBytes = object ["source" .= (source::T.Text),"address" .= (address::Word32),"physical" .= address,"size" .= (4::Int),"value" .= value,"before" .= beforeBytes,"byte_order" .= ("little"::T.Text),"core" .= (0::Int),"pc" .= (pc::Word32),"instructions" .= (1::Int),"tick" .= (1::Int)]

fixture :: Role -> Mutation -> Observation
fixture role mutation = Observation receipt files where
  binding bytes = object ["path" .= ("fixture"::T.Text),"sha256" .= hash bytes]
  spec = object ["schema" .= (1::Int),"sites" .= object ([K.fromString (hexWord entry) .= ("record-entry"::T.Text) | entry <- entries] ++ ["0x6fb7c8" .= ("nand-write"::T.Text),"0x6fb57c" .= ("nand-erase"::T.Text)])
    ,"captures" .= (if mutation == MissingCapture then object [] else captureDeclarations),"hit_limit" .= (1500000::Int),"watch_limit" .= (1048576::Int),"write_ranges" .= writes role,"read_ranges" .= retired role,"fetch_ranges" .= fetches role]
  events = trace role mutation WholeService
  reset = object ["schema" .= (1::Int),"reset" .= object ["instructions" .= (0::Int)],"regions_at_reset" .= resetRegions role Original
    ,"segments" .= [object ["phase" .= phase,"instructions" .= (1::Int),"hits_end" .= length events,"watch" .= watch role phase mutation,"regions" .= resetRegions role mutation] | phase <- [minBound..maxBound]::[AttachPhase]]]
  files = M.fromList [(Spec,encode spec),(Extents,encode extents),(Reset,encode reset),(Hits,B.intercalate "\n" (map encode events) <> "\n"),(Steps,"[]"),(MediumFile,encode (medium mutation)),(Session,"{}")]
  receipt = encode (object ["schema" .= (2::Int),"role" .= (if role == Stock then "stock" else "owned"::T.Text),"outcome" .= object ["status" .= ("completed"::T.Text)]
    ,"endpoint_contract" .= ("finite-post-settle-prefix-v2"::T.Text),"post_settle_prefix_ticks" .= (833334::Int)
    ,"output_rule" .= ("Compare outputs written by the stock contract across roles; require each contract-untouched output to equal its own entry preimage, without comparing residue across roles."::T.Text)
    ,"inputs" .= object (["update" .= binding (if role == Stock then "stock-DAT" else "owned-DAT"),"routes" .= binding (routeBytes bindings),"contract" .= binding (contractBytes bindings),"linked" .= binding (linkedBytes bindings),"symbols" .= binding (symbolBytes bindings)]
      ++ [K.fromString name .= binding (if mutation == BadBinding && name == "run" then "different-run" else "cohort") | name <- ["run","boot-rom","adjustment-nand","observer","bridge"]])
    ,"files" .= object [K.fromString (sidecarName name) .= hash bytes | (name,bytes) <- M.toList files],"ram_rule" .= extents,"raw_residual_count" .= (0::Int),"settings_at_attach" .= regions mutation,"medium_at_attach" .= medium mutation])

data ResetEvidence = ResetAppends | ResetWithoutAppend | BudgetedKey deriving (Eq)

data PrefixEvidence = Sampled | MissingPrefix | MissingUi | MissingMenu | MissingDepth | MissingNode | MenuAddressDiffers | MenuAliasDisagrees | MissingMode | ModeAddressDiffers | ModeAliasDisagrees | ShortMode | ChangedCurrent | ChangedPrevious | ChangedPending | ChangedRequest | ChangedLatch | ShortTicks | ExcessTicks | ExcessRan | TickOverflow | InstructionOverflow | RanOverflow | SnapshotInstructionDiffers | SnapshotTickOverflow | UnlatchedPress | PriorRenderPending | IncompleteRecordCall deriving (Eq, Show, Enum, Bounded)

prefixEndpoint :: PrefixEvidence -> Word32 -> Value
prefixEndpoint evidence cursor = object
  ["elapsed" .= object
    ["instructions" .= (if evidence == InstructionOverflow then 2^(64::Int) else 30000000::Integer)
    ,"ran" .= (case evidence of ExcessRan -> 30000001; RanOverflow -> 2^(64::Int); _ -> 1200000::Integer)
    ,"ticks" .= (case evidence of ShortTicks -> 833333; ExcessTicks -> 833335; TickOverflow -> 2^(64::Int); _ -> 833334::Integer)]
  ,"state" .= object
    (["instructions" .= (if evidence == SnapshotInstructionDiffers then 30000001 else 30000000::Integer)
     ,"tick" .= (if evidence == SnapshotTickOverflow then 2^(64::Int) else 1000000::Integer)]
     ++ ["ui" .= object
       (["menu" .= view (if evidence == MenuAddressDiffers then 0x9571d4 else 0x9571d0) (wordBytes 0 <> wordBytes 0 <> wordBytes cursor) | evidence /= MissingMenu]
       ++ ["depth" .= view 0x9575f4 (wordBytes 0) | evidence /= MissingDepth]
       ++ ["node" .= Null | evidence /= MissingNode]) | evidence /= MissingUi]
     ++ ["source_mode" .= view (if evidence == ModeAddressDiffers then 0x9500f0 else 0x9500ec)
       (if evidence == ShortMode then B.take 36 mode else mode) | evidence /= MissingMode])]
 where
  view address bytes = object ["address" .= (address::Word32),"physical" .= hex bytes
    ,"spellings" .= [[hex bytes,hex bytes],[hex bytes,hex (if evidence == MenuAliasDisagrees && address == 0x9571d0 || evidence == ModeAliasDisagrees && address == 0x9500ec then B.map (+1) bytes else bytes)]]]
  changed which = if evidence == which then 1 else 0
  mode = B.concat [wordBytes (changed ChangedCurrent),wordBytes (changed ChangedPrevious),wordBytes 0,wordBytes (changed ChangedPending),wordBytes 0,wordBytes (changed ChangedRequest),B.replicate 12 0,wordBytes (changed ChangedLatch)]

resetFixture :: ResetEvidence -> (Bindings, Observation)
resetFixture evidence = keyFixture Owned evidence Sampled

keyFixture :: Role -> ResetEvidence -> PrefixEvidence -> (Bindings, Observation)
keyFixture = keyFixtureAtCursor 0

keyFixtureAtCursor :: Word32 -> Role -> ResetEvidence -> PrefixEvidence -> (Bindings, Observation)
keyFixtureAtCursor cursor role evidence prefixEvidence = (bound, Observation receipt files) where
  bound = bindings { routeBytes = BL.toStrict (encodeRouteFile (RouteFile (replicate 64 '0') "reset fixture" CrawlEffectsSource []
    (phaseRoutes ++ [Route (RouteName (if evidence == BudgetedKey then "latched-key" else "custom-reset-defaults")) (KeyRoute [KeyOk] (if evidence == BudgetedKey then CrawlTrial 0 [CommitEffect] else Declared CustomResetDefaults))]))) }
  events = filter keep (trace role Original WholeService)
  keep event = (evidence /= ResetWithoutAppend || not (any (\site -> get "site" (get "observation" event) == A.toJSON (site::Word32)) [0x6ac7d4,0x6fb7c8]))
    && not (prefixEvidence == IncompleteRecordCall && get "site" (get "observation" event) == A.toJSON (0x6ac7d4::Word32) && get "event" (get "observation" event) == String "return")
  Observation originalReceipt originalFiles = fixture role Original
  original = decode originalReceipt
  decode bytes = either error id (A.eitherDecodeStrict' bytes)
  get key (Object fields) = maybe (error "fixture field missing") id (KM.lookup key fields)
  get _ _ = error "fixture object required"
  set key value (Object fields) = Object (KM.insert key value fields)
  set _ _ _ = error "fixture object required"
  reset = object ["schema" .= (1::Int),"reset" .= object ["instructions" .= (0::Int)],"regions_at_reset" .= resetRegions role Original
    ,"segments" .= [object ["phase" .= phase,"instructions" .= (1::Int),"hits_end" .= (0::Int),"watch" .= watch role phase Original,"regions" .= resetRegions role Original] | phase <- [minBound..maxBound]::[AttachPhase]]]
  press = object ["failure" .= (if prefixEvidence == UnlatchedPress then String "unlatched" else Null),"settle" .= object ["status" .= (if prefixEvidence == PriorRenderPending then "prior-render-pending" else if evidence == BudgetedKey then "budget-exhausted" else "packet-consumed"::T.Text)]
    ,"menu" .= object ["cursor" .= (0::Int),"menu_id" .= (1::Int),"setting_id" .= (5::Int)]]
  step = object (["step" .= (1::Int),"parent" .= (0::Int),"keys" .= [KeyOk],"hits" .= ([0,length events]::[Int])
    ,"idle" .= object ["ran" .= (20000000::Int)],"press" .= press,"regions_before" .= regions Original,"regions_after" .= regions Original
    ,"medium_after" .= medium Original,"watch" .= set "views" (String "fixed") (watch role LiveViewPhase Original)]
    ++ ["bounded_post_settle_prefix" .= prefixEndpoint prefixEvidence cursor | prefixEvidence /= MissingPrefix])
  files = M.insert Reset (encode reset) (M.insert Hits (B.intercalate "\n" (map encode events) <> "\n") (M.insert Steps (encode (A.toJSON [step])) originalFiles))
  inputs = set "routes" (object ["path" .= ("fixture"::T.Text),"sha256" .= hash (routeBytes bound)]) (get "inputs" original)
  receipt = encode (set "inputs" inputs (set "files" (object [K.fromString (sidecarName name) .= hash bytes | (name,bytes) <- M.toList files]) original))

data ImageSizePoint = RootPoint | ParentPoint | EntryPoint | DownPoint | UpPoint | BackPoint deriving (Eq, Show, Enum, Bounded)
data ImageSizeEvidence = IntendedChoice | StalledChoice | ErasedStalledChoice | NoDownChange | WrongEntryCursor | WrongBackCursor | ChangedOtherSetting | ChangedAspect | UnpublishedBackup | ChangedImageSizeMode | WrongImageSizeTrial | WrongImageSizeFixture | ChangedUpRecord | ChangedBackRecord | ChangedUpNand | ChangedBackNand | ImageSizeRetiredFetch deriving (Eq, Show)
data FixtureMenuFrame = FixtureMenuFrame MenuNodeAddress SettingIdentifier MenuCursor MenuDepth

imageSizePointKeys :: ImageSizePoint -> [CrawlKey]
imageSizePointKeys point = case point of
  RootPoint -> [KeyOk]
  ParentPoint -> [KeyOk,KeyDown]
  EntryPoint -> [KeyOk,KeyDown,KeyOk]
  DownPoint -> [KeyOk,KeyDown,KeyOk,KeyDown]
  UpPoint -> [KeyOk,KeyDown,KeyOk,KeyDown,KeyUp]
  BackPoint -> [KeyOk,KeyDown,KeyOk,KeyDown,KeyBack]

imageSizeComparisonBytes :: B.ByteString
imageSizeComparisonBytes = encode (object
  ["schema" .= (1::Int),"policy" .= ("image-size-pending-choice-v1"::T.Text)
  ,"record_nand_sha256" .= ("eb33ae15ec676f5dfd9465223500c89179af5c90f14047b4e528a7db741e8682"::T.Text)
  ,"source_evidence" .= object ["path" .= ("fixture-source-evidence"::T.Text),"sha256" .= hash "fixture-source-proof"]
  ,"cases" .= [object ["crawl_trial" .= trial,"keys" .= imageSizePointKeys point] | (trial,point) <- [(368::Int,UpPoint),(371,BackPoint)]]])

imageSizeComparison :: ComparisonContract
imageSizeComparison = either error id (parseComparisonContract imageSizeComparisonBytes)

imageSizeFixture :: Role -> ImageSizeEvidence -> (Bindings,Observation)
imageSizeFixture role evidence = (bound,Observation receipt files)
 where
  (initialBounds,Observation initialReceipt initialFiles) = keyFixture role BudgetedKey Sampled
  bound = initialBounds { routeBytes = BL.toStrict (encodeRouteFile (RouteFile (replicate 64 '0') "image-size intended behavior"
    (RetainedObservationSource (sha256 "crawl-receipt") (sha256 "crawl-steps") (sha256 "crawl-hits")) []
    (phaseRoutes ++ [Route (RouteName name) (KeyRoute (imageSizePointKeys point) (CrawlTrial trial []))
      | (name,trial,point) <- [("crawl-trial-00368",if evidence == WrongImageSizeTrial then 369 else 368,UpPoint),("crawl-trial-00371",371,BackPoint)]]))) }
  decode bytes = either error id (A.eitherDecodeStrict' bytes)
  get key (Object fields) = maybe (error "fixture field missing") id (KM.lookup key fields)
  get _ _ = error "fixture object required"
  original = decode initialReceipt
  rootEvents = trace role Original OpenAndRegister
  effectEvents point = trace role (traceMutation point) ReadAndAppend
  traceMutation point
    | point == UpPoint && evidence == ChangedUpRecord || point == BackPoint && evidence == ChangedBackRecord = ReturnedRecord
    | point == UpPoint && evidence == ChangedUpNand || point == BackPoint && evidence == ChangedBackNand = PayloadByte
    | otherwise = Original
  upEvents = effectEvents UpPoint
  backEvents = effectEvents BackPoint
  events = rootEvents ++ upEvents ++ backEvents
  hitRange RootPoint = [0,length rootEvents]
  hitRange UpPoint = [length rootEvents,length rootEvents+length upEvents]
  hitRange BackPoint = [length rootEvents+length upEvents,length events]
  hitRange _ = [length rootEvents,length rootEvents]
  stalled = evidence `elem` [StalledChoice,ErasedStalledChoice]
  pending point = point == DownPoint && evidence /= NoDownChange || point `elem` [UpPoint,BackPoint] && stalled
  selecting point = point `elem` [EntryPoint,DownPoint,UpPoint] || point == BackPoint && stalled
  rows point = [object ["start" .= rangeStart range,"bytes" .= hex (B.pack [at range index | index <- [0::Int ..fromIntegral (rangeEnd range-rangeStart range)-1]])] | range <- settingsRanges]
   where
    at range index
      | index == 0x6e = if selecting point then 0x20 else 0
      | index == 0x6f && evidence == ChangedAspect && point == UpPoint = 0x20
      | index == 0x7d = if pending point then 0x40 else 0x20
      | rangeStart range == 0x96cc64 && index == 450+0x7d = if evidence == UnpublishedBackup then 0x40 else 0x20
      | index == 5 && evidence == ChangedOtherSetting && point == UpPoint = 0x5a
      | otherwise = 0
  region point = A.toJSON (rows point)
  resetRows = A.toJSON (rows RootPoint ++ [object ["start" .= low,"bytes" .= hex (B.replicate (fromIntegral (high-low)) 0)] | [low,high] <- retired role])
  reset = object ["schema" .= (1::Int),"reset" .= object ["instructions" .= (0::Int)],"regions_at_reset" .= resetRows
    ,"segments" .= [object ["phase" .= phase,"instructions" .= (1::Int),"hits_end" .= (0::Int),"watch" .= watch role phase Original,"regions" .= resetRows] | phase <- [minBound..maxBound]::[AttachPhase]]]
  frame :: ImageSizePoint -> FixtureMenuFrame
  frame point
    | point `elem` [EntryPoint,DownPoint,UpPoint] || point == BackPoint && stalled =
        FixtureMenuFrame (MenuNodeAddress 0x784014) (SettingIdentifier 246)
          (MenuCursor (if pending point || point == EntryPoint && evidence == WrongEntryCursor then 1 else 0)) (MenuDepth 2)
    | otherwise = FixtureMenuFrame (MenuNodeAddress 0x78ea98) (SettingIdentifier 5)
        (MenuCursor (if point == RootPoint || point == BackPoint && evidence == WrongBackCursor then 2 else 3)) (MenuDepth 1)
  view address bytes = object ["address" .= (address::Word32),"physical" .= hex bytes,"spellings" .= [[hex bytes,hex bytes],[hex bytes,hex bytes]]]
  endpoint point = changedObject "state" state originalEndpoint
   where
    originalEndpoint = prefixEndpoint (if evidence == ChangedImageSizeMode && point == UpPoint then ChangedCurrent else Sampled) 0
    FixtureMenuFrame (MenuNodeAddress node) (SettingIdentifier setting) (MenuCursor cursor) (MenuDepth depth) = frame point
    state = changedObject "ui" (object
      ["menu" .= view 0x9571d0 (wordBytes node <> wordBytes 0 <> wordBytes cursor)
      ,"depth" .= view 0x9575f4 (wordBytes depth)
      ,"node" .= view node (wordBytes 1 <> wordBytes 0 <> wordBytes setting)]) (get "state" originalEndpoint)
  steps = [step number point | (number,point) <- zip [1::Int ..] [minBound..maxBound]]
  step number point = object
    ["step" .= number,"parent" .= (case point of RootPoint -> 0; ParentPoint -> 1; EntryPoint -> 2; DownPoint -> 3; UpPoint -> 4; BackPoint -> 4::Int)
    ,"keys" .= imageSizePointKeys point,"hits" .= hitRange point
    ,"idle" .= object ["ran" .= (20000000::Int)]
    ,"press" .= object ["failure" .= Null,"settle" .= object ["status" .= ("packet-consumed"::T.Text)]
      ,"menu" .= let FixtureMenuFrame _ (SettingIdentifier setting) (MenuCursor cursor) _ = frame point
                 in object ["menu_id" .= (1::Int),"setting_id" .= setting,"cursor" .= cursor]]
    ,"bounded_post_settle_prefix" .= endpoint point
    ,"regions_before" .= region (case point of RootPoint -> RootPoint; ParentPoint -> RootPoint; EntryPoint -> ParentPoint; DownPoint -> EntryPoint; UpPoint -> DownPoint; BackPoint -> DownPoint)
    ,"regions_after" .= region point,"medium_after" .= (if point `elem` [UpPoint,BackPoint] then medium Original else Null)
    ,"watch" .= changedObject "views" (String "fixed") (if point == UpPoint && evidence == ImageSizeRetiredFetch
      then watch role FirstStartPhase StrayFetch else watch role LiveViewPhase Original)]
  files = M.insert Reset (encode reset) (M.insert Hits (B.intercalate "\n" (map encode events) <> "\n") (M.insert Steps (encode (A.toJSON steps)) initialFiles))
  sourceNand = if evidence == WrongImageSizeFixture then T.pack (hash "other-populated-fixture") else "eb33ae15ec676f5dfd9465223500c89179af5c90f14047b4e528a7db741e8682"
  routeInputs = changedObject "routes" (object ["path" .= ("fixture"::T.Text),"sha256" .= hash (routeBytes bound)]) (get "inputs" original)
  inputs = if evidence == ErasedStalledChoice then routeInputs
    else changedObject "record-nand" (object ["path" .= ("fixture-record-nand"::T.Text),"sha256" .= sourceNand]) routeInputs
  receipt = encode (changedObject "settings_at_attach" (region RootPoint)
    (changedObject "inputs" inputs (changedObject "files" (object [K.fromString (sidecarName name) .= hash bytes | (name,bytes) <- M.toList files]) original)))

imageSizeReport :: ComparisonContract -> ImageSizeEvidence -> ImageSizeEvidence -> Either String Value
imageSizeReport comparison retained candidate = do
  let (stockBindings,stockObservation) = imageSizeFixture Stock retained
      (ownedBindings,ownedObservation) = imageSizeFixture Owned candidate
  stock <- qualify Stock stockBindings stockObservation
  owned <- qualify Owned ownedBindings ownedObservation
  let result = comparePair comparison stock owned
  either (Left . show) (const (Right (comparisonValue comparison stock owned result))) result

imageSizeEffectsCompared :: ImageSizeEvidence -> Bool
imageSizeEffectsCompared evidence =
  let (stockBindings,stockObservation) = imageSizeFixture Stock StalledChoice
      (ownedBindings,ownedObservation) = imageSizeFixture Owned evidence
   in case (qualify Stock stockBindings stockObservation,qualify Owned ownedBindings ownedObservation) of
        (Right stock,Right owned) -> isLeft (comparePair imageSizeComparison stock owned)
        _ -> False

reportField :: K.Key -> Either String Value -> Maybe Value
reportField name result = case result of
  Right (Object fields) -> KM.lookup name fields
  _ -> Nothing

changedImageSizeContract :: K.Key -> Value -> Either String ComparisonContract
changedImageSizeContract name value = do
  original <- A.eitherDecodeStrict' imageSizeComparisonBytes
  parseComparisonContract (encode (changedObject name value original))

strictComparison :: ComparisonContract
strictComparison = either error id (parseComparisonContract "{\"schema\":1,\"policy\":\"stock-equality-v1\"}")

main :: IO ()
main = do
  let stock = qualify Stock bindings (fixture Stock Original)
      intendedDifference = imageSizeReport imageSizeComparison StalledChoice IntendedChoice
      intendedMatch = imageSizeReport imageSizeComparison IntendedChoice IntendedChoice
      erasedFiniteMatch = imageSizeReport imageSizeComparison ErasedStalledChoice ErasedStalledChoice
      intendedCasesReported = case reportField "intended_cases" intendedDifference of
        Just (Array cases) -> length cases == 2 && all intended cases && any restoredSynchronization cases
        _ -> False
       where
        intended (Object fields) = KM.lookup "owned" fields == Just (String "intended")
          && KM.lookup "expected_choice" fields == Just (object ["size" .= ("L"::T.Text),"aspect" .= ("3:2"::T.Text)])
        intended _ = False
        restoredSynchronization (Object fields) = case KM.lookup "property_161" fields of
          Just (Object property) -> KM.lookup "crawl_trial" fields == Just (Number 371)
            && KM.lookup "classification" property == Just (String "derived-image-size-synchronization")
            && KM.lookup "after" property == Just (Number 0) && KM.lookup "native_leave_restore" property == Just (Bool True)
          _ -> False
        restoredSynchronization _ = False
      endpointCursorDiffers = case (keyFixtureAtCursor 0 Stock ResetAppends Sampled,keyFixtureAtCursor 1 Owned ResetAppends Sampled) of
        ((stockBindings,stockObservation),(ownedBindings,ownedObservation)) ->
          case (qualify Stock stockBindings stockObservation,qualify Owned ownedBindings ownedObservation) of
            (Right a,Right b) -> isLeft (comparePair strictComparison a b)
            _ -> False
      owned mutation = qualify Owned bindings (fixture Owned mutation)
      matchedAgainst baseline mutation = case (qualify Stock bindings (fixture Stock baseline),owned mutation) of
        (Right a,Right b) -> not (isLeft (comparePair strictComparison a b))
        _ -> False
      failedAgainst baseline mutation = case (qualify Stock bindings (fixture Stock baseline),owned mutation) of
        (Right _,Left _) -> True
        (Right a,Right b) -> isLeft (comparePair strictComparison a b)
        _ -> False
      matched mutation = case (stock,owned mutation) of
        (Right a,Right b) -> not (isLeft (comparePair strictComparison a b))
        _ -> False
      differs mutation = case (stock,owned mutation) of
        (Right a,Right b) -> isLeft (comparePair strictComparison a b)
        _ -> False
      refused mutation = isLeft (owned mutation)
      mappedMatched = case (qualify Stock bindings (fixture Stock MappedMedium),owned MappedMedium) of
        (Right a,Right b) -> not (isLeft (comparePair strictComparison a b))
        _ -> False
      spareDifference = case (qualify Stock bindings (fixture Stock MappedMedium),owned ChangedSpareByte) of
        (Right a,Right b) -> case comparisonValue strictComparison a b (comparePair strictComparison a b) of
          Object fields -> case KM.lookup "path" fields of
            Just (String path) -> ".spare.0" `T.isSuffixOf` path
            _ -> False
          _ -> False
        _ -> False
      reportedCoverage = case (stock,owned Original) of
        (Right a,Right b) ->
          let expected role = object
                [ "role" .= (role::T.Text), "record_nand_sha256" .= Null, "external_entry_total" .= (9::Int)
                , "external_entries" .= object
                    [ "record_store_open" .= (1::Int), "record_store_register" .= (1::Int)
                    , "record_store_close" .= (1::Int), "record_store_read_latest" .= (1::Int)
                    , "record_store_append" .= (1::Int), "record_store_erase_all" .= (1::Int)
                    , "record_store_collect" .= (1::Int), "record_store_read_at" .= (1::Int)
                    , "record_store_zero" .= (1::Int)]
                , "nand" .= object ["program" .= (1::Int),"erase" .= (1::Int)]]
              report = comparisonValue strictComparison a b (comparePair strictComparison a b)
           in case report of
                Object fields -> KM.lookup "coverage" fields == Just (object ["stock" .= expected "stock","owned" .= expected "owned"])
                _ -> False
        _ -> False
      prefixAdmitted role evidence = let (bound,observation) = keyFixture role ResetAppends evidence in not (isLeft (qualify role bound observation))
      sampledPair = case (keyFixture Stock ResetAppends Sampled,keyFixture Owned ResetAppends Sampled) of
        ((bound,a),(_,b)) -> case (qualify Stock bound a,qualify Owned bound b) of
          (Right left,Right right) -> not (isLeft (comparePair strictComparison left right))
          _ -> False
      changedSteps transform =
        let (bound,observation) = keyFixture Owned ResetAppends Sampled
            steps = transform (maybe (error "missing steps") id (M.lookup Steps (sidecars observation)))
            receipt = either error id (A.eitherDecodeStrict' (receiptBytes observation))
            files = case receipt of Object fields -> maybe (error "missing files") id (KM.lookup "files" fields); _ -> error "receipt object"
            updated = changedObject "files" (changedObject "steps.json" (A.toJSON (hash steps)) files) receipt
         in qualify Owned bound observation {receiptBytes = encode updated, sidecars = M.insert Steps steps (sidecars observation)}
      lateStepRefused = isLeft (changedSteps (\bytes -> case A.eitherDecodeStrict' bytes :: Either String [Value] of
        Right (row:rows) -> encode (A.toJSON (row : rows ++ [changedObject "step" (Number 2) row]))
        _ -> bytes))
      duplicateStepAccepted = not (isLeft (changedSteps (\bytes ->
        let (before,after) = B.breakSubstring "\"step\":1" bytes
         in if B.null after then bytes else before <> "\"step\":1,\"step\":9" <> B.drop 8 after)))
      wrongPrefixPolicy role policy =
        let (bound,observation) = keyFixture role ResetAppends Sampled
         in case A.eitherDecodeStrict' (receiptBytes observation) of
              Right (Object fields) ->
                let changed = maybe (KM.delete "post_settle_prefix_ticks" fields) (\value -> KM.insert "post_settle_prefix_ticks" value fields) policy
                 in isLeft (qualify role bound observation { receiptBytes = encode (Object changed) })
              _ -> False
      wrongEndpointVersion role key value =
        let (bound,observation) = keyFixture role ResetAppends Sampled
            receipt = either error id (A.eitherDecodeStrict' (receiptBytes observation))
        in isLeft (qualify role bound observation {receiptBytes = encode (changedObject key value receipt)})
      changedModeDiffers evidence = case (keyFixture Stock ResetAppends Sampled,keyFixture Owned ResetAppends evidence) of
        ((bound,a),(_,b)) -> case (qualify Stock bound a,qualify Owned bound b) of
          (Right left,Right right) -> isLeft (comparePair strictComparison left right)
          _ -> False
      validEvidence = [Sampled,ChangedCurrent,ChangedPrevious,ChangedPending,ChangedRequest,ChangedLatch]
      wrongCache value = rejectBinding (changedObject "owned_cache" value) id
      wrongCacheField name value = wrongCache (changedObject name value cacheContract)
      wrongSymbols extra = rejectBinding id (<> C.pack extra)
      tests = [("nine-entry observation matches",matched Original)
        ,("changed endpoint cursor fails with equal press-time menus",endpointCursorDiffers)
        ,("defaults reset with successful append and program request admits comparison",let (bound,observation) = resetFixture ResetAppends in not (isLeft (qualify Owned bound observation)))
        ,("defaults reset without a record append refuses",let (bound,observation) = resetFixture ResetWithoutAppend in isLeft (qualify Owned bound observation))
        ,("a late invalid step refuses the observation",lateStepRefused)
        ,("duplicate step keys keep the first occurrence",duplicateStepAccepted)
        ,("missing cache metadata policy refuses",rejectBinding (\value -> case value of Object fields -> Object (KM.delete "owned_cache" fields); _ -> value) id)
        ,("wrong open initializer refuses",wrongCacheField "initialized_by" (A.toJSON (0x6ac328::Word32)))
        ,("wrong tags initializer refuses",wrongCacheField "tags" (object ["address" .= (0x8e0080::Word32),"bytes" .= (8::Int),"initial_values" .= ([0,0]::[Word32])]))
        ,("wrong timestamp initializer refuses",wrongCacheField "timestamp" (object ["address" .= (0x8e0088::Word32),"bytes" .= (4::Int),"initial_value" .= (1::Word32)]))
        ,("wrong ages initializer refuses",wrongCacheField "ages" (object ["address" .= (0x89fa60::Word32),"bytes" .= (8::Int),"initial_values" .= ([1,0]::[Word32])]))
        ,("one-buffer ownership refuses",wrongCacheField "buffers" (A.toJSON ([[0x8a0080,0x8c0080]]::[[Word32]])))
        ,("obsolete tag binding refuses",wrongSymbols "0089f8f0 0 A record_store_cache_tag\n")
        ,("obsolete gap alias refuses",wrongSymbols "0089f8f4 0 A unexpected_alias\n")
        ,("duplicated cache bindings refuse",and [wrongSymbols (showHex address " 0 A " ++ name ++ "\n") | (name,address) <- cacheSymbolLayout])
        ,("rebound unchanged contract remains admitted",not (rejectBinding id id))
        ,("cache interior alias refuses",wrongSymbols "008c0084 0 A unexpected_alias\n")
        ,("ages alias refuses",wrongSymbols "0089fa64 0 A unexpected_alias\n")
        ,("moved cache and staging bindings refuse",and [rejectBinding id (C.unlines . map (\line -> if C.words line == [C.pack (showHex address ""), "0", "A", C.pack name] then C.pack (showHex (address+4) " 0 A " ++ name) else line) . C.lines) | (name,address) <- cacheSymbolLayout])
        ,("obsolete gap cannot be declared owned",rejectOwnership (A.toJSON ([[0x89f8f0,0x89fa68],[0x8a0080,0x8e008c]]::[[Word32]])))
        ,("second buffer ownership cannot be omitted",rejectOwnership (A.toJSON ([[0x89f8f8,0x89fa60],[0x89fa60,0x89fa68],[0x8a0080,0x8c0080],[0x8e0080,0x8e008c]]::[[Word32]])))
        ,("report names exercised entries and NAND requests per role/media",reportedCoverage)
        ,("changed program payload fails comparison",differs PayloadByte)
        ,("reordered external calls fail comparison",differs ReorderCalls)
        ,("changed returned record fails comparison",differs ReturnedRecord)
        ,("changed collection tail fails comparison",differs CollectionTail)
        ,("void-close scratch return is excluded",matched CloseScratch)
        ,("output storage stack address is excluded",matched OutputStorage)
        ,("failed latest preserves different per-role pointer residues",matchedAgainst PreservedLatest PreservedLatest)
        ,("changed untouched pointer nonnull refuses",failedAgainst PreservedLatest ChangedPreservedPointer)
        ,("changed untouched pointer null refuses",failedAgainst PreservedLatest ClearedPreservedPointer)
        ,("changed untouched skipped count refuses",failedAgainst PreservedLatest ChangedPreservedSkipped)
        ,("late failure null writes remain compared",matchedAgainst WrittenNullLatest WrittenNullLatest)
        ,("missing late failure null write fails",failedAgainst WrittenNullLatest MissingWrittenNullLatest)
        ,("ambiguous unchanged zero skipped values satisfy both constraints",matchedAgainst ZeroSkippedLatest ZeroSkippedLatest)
        ,("ambiguous changed zero skipped preimage refuses",failedAgainst ZeroSkippedLatest ChangedZeroSkippedPreimage)
        ,("opened register failure preserves per-role client word",matchedAgainst PreservedRegister PreservedRegister)
        ,("changed preserved register client word refuses",failedAgainst PreservedRegister ChangedPreservedRegister)
        ,("inner registration failure preserves client word",matchedAgainst InnerFailedRegister InnerFailedRegister)
        ,("unopened registration null writes remain compared",matchedAgainst UnopenedRegister UnopenedRegister)
        ,("missing unopened registration null write fails",failedAgainst UnopenedRegister UnopenedRegisterNonNull)
        ,("out-of-bank read preserves different per-role tail residues",matchedAgainst OutOfBankRead OutOfBankRead)
        ,("changed out-of-bank read tail refuses",failedAgainst OutOfBankRead ChangedOutOfBankTail)
        ,("invalid-kind read zero-fills remain compared",matchedAgainst InvalidKindRead InvalidKindRead)
        ,("failed collect preserves byte-count residue",matchedAgainst FailedCollect FailedCollect)
        ,("changed failed collect byte count refuses",failedAgainst FailedCollect ChangedFailedCollectCount)
        ,("partial failed collect buffer effects remain checked",failedAgainst FailedCollect ChangedFailedCollectBuffer)
        ,("missing retired reset snapshot refuses qualification",refused MissingResetRegion)
        ,("private reset bytes are excluded from settings comparison",matched RetiredSnapshotDifference)
        ,("stray retired fetch refuses qualification",refused StrayFetch)
        ,("post-entry stock-only read refuses qualification",refused ForeignRead)
        ,("stock saved-destination access refuses qualification",refused SavedDestinationWrite)
        ,("obsolete cache-tag write refuses qualification",refused ObsoleteTagWrite)
        ,("pre-entry image publication is admitted",matched PreEntryPublication)
        ,("post-entry image publication refuses qualification",refused PostEntryPublication)
        ,("pre-entry BSS write is admitted",matched PreEntryBssWrite)
        ,("pre-entry retired-data read refuses qualification",refused PreEntryRead)
        ,("missing activation marker refuses qualification",refused MissingMarker)
        ,("retirement remains active across phases",refused LaterPhaseWrite)
        ,("changed settings fail comparison",differs BadSettings)
        ,("unmapped non-erased medium refuses qualification",refused BadMedium)
        ,("mapped medium admits complete bridge spare blocks",mappedMatched)
        ,("truncated mapped spare block refuses qualification",refused TruncatedSpare)
        ,("mapped spare bytes remain compared exactly",spareDifference)
        ,("missing capture declaration refuses qualification",refused MissingCapture)
        ,("changed cohort input fails comparison",differs BadBinding)
        ,("both roles include the finite post-settle interval",sampledPair)
        ,("latched budget-exhausted keys admit finite input observations",all (\role -> let (bound,observation) = keyFixture role BudgetedKey Sampled in isRight (qualify role bound observation)) [Stock,Owned])
        ,("missing or changed exact prefix ticks refuse both roles",and [wrongPrefixPolicy role policy | role <- [Stock,Owned], policy <- [Nothing,Just (Number 833333)]])
        ,("old receipt schema refuses both roles",all (\role -> wrongEndpointVersion role "schema" (Number 1)) [Stock,Owned])
        ,("old endpoint contract refuses both roles",all (\role -> wrongEndpointVersion role "endpoint_contract" (String "local-ui-drain")) [Stock,Owned])]
        ++ [("intended image-size selection accepts the known stock stall without calling it equality",reportField "outcome" intendedDifference == Just (String "accepted-functional-difference"))
           ,("intended image-size selection reports both declared cases and derived rollback",intendedCasesReported)
           ,("a responsive stock image-size selection also satisfies the intended oracle",reportField "outcome" intendedMatch == Just (String "matched"))
           ,("ordinary stock equality still detects the image-size difference",isLeft (imageSizeReport strictComparison StalledChoice IntendedChoice))
           ,("an erased finite match makes no intended image-size claim",reportField "outcome" erasedFiniteMatch == Just (String "matched") && reportField "intended_cases" erasedFiniteMatch == Just (A.toJSON ([]::[Value])))
           ,("stock image-size output must be intended or the exact stalled DOWN state",isLeft (imageSizeReport imageSizeComparison ChangedOtherSetting IntendedChoice))
           ,("an undeclared populated fixture cannot bypass the intended oracle",isLeft (imageSizeReport imageSizeComparison WrongImageSizeFixture WrongImageSizeFixture))
           ,("comparison source evidence binds its bytes",isRight (validateComparisonEvidence imageSizeComparison "fixture-source-proof") && isLeft (validateComparisonEvidence imageSizeComparison "changed-proof"))
           ,("unknown comparison policies refuse",isLeft (parseComparisonContract "{\"schema\":1,\"policy\":\"unknown\"}"))
           ,("changed intended-case paths refuse",isLeft (changedImageSizeContract "cases" (A.toJSON
             [object ["crawl_trial" .= (368::Int),"keys" .= [KeyOk,KeyUp]],object ["crawl_trial" .= (371::Int),"keys" .= imageSizePointKeys BackPoint]])))]
        ++ [("intended image-size oracle rejects " ++ show evidence,isLeft (imageSizeReport imageSizeComparison StalledChoice evidence))
           | evidence <- [StalledChoice,NoDownChange,WrongEntryCursor,WrongBackCursor,ChangedOtherSetting,ChangedAspect,UnpublishedBackup,ChangedImageSizeMode,WrongImageSizeTrial,ImageSizeRetiredFetch]]
        ++ [("protected image-size interval compares " ++ show evidence,imageSizeEffectsCompared evidence)
           | evidence <- [ChangedUpRecord,ChangedBackRecord,ChangedUpNand,ChangedBackNand]]
        ++ [("changed source mode fails comparison: " ++ show evidence,changedModeDiffers evidence) | evidence <- drop 1 validEvidence]
        ++ [("finite prefix rejects " ++ show evidence ++ " for " ++ show role,not (prefixAdmitted role evidence))
           | role <- [Stock,Owned], evidence <- [minBound..maxBound], evidence `notElem` validEvidence]
  case (stock,owned Original) of
    (Left failure,_) -> putStrLn ("stock fixture: " ++ failure)
    (_,Left failure) -> putStrLn ("owned fixture: " ++ failure)
    _ -> pure ()
  case intendedDifference of
    Left failure -> putStrLn ("image-size fixture: " ++ failure)
    _ -> pure ()
  mapM_ (\(name,passed) -> putStrLn ((if passed then "PASS " else "FAIL ") ++ name)) tests
  unless (all snd tests) exitFailure
