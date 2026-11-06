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
import Data.Either (isLeft)
import qualified Data.Map.Strict as M
import qualified Data.Text as T
import qualified Data.Text.Encoding as TE
import Data.Word (Word32)
import Emulation.PropertyPairPlan (PhysicalRange (..), settingsRanges)
import Emulation.PropertyPairRoutes
import Emulation.RecordStorePairCheck
import Emulation.Sha256 (sha256, sha256Hex)
import EmulatorUsb (CrawlKey (..))
import Numeric (showHex)
import System.Exit (exitFailure)

data Mutation = Original | PayloadByte | ReorderCalls | StrayFetch | ForeignRead | SavedDestinationWrite | ObsoleteTagWrite | BadSettings | BadMedium | MissingCapture | BadBinding | CollectionTail | ReturnedRecord | CloseScratch | OutputStorage | MissingResetRegion | RetiredSnapshotDifference | PreEntryPublication | PostEntryPublication | PreEntryBssWrite | PreEntryRead | MissingMarker | LaterPhaseWrite | MappedMedium | TruncatedSpare | ChangedSpareByte | PreservedLatest | ChangedPreservedPointer | ClearedPreservedPointer | ChangedPreservedSkipped | WrittenNullLatest | MissingWrittenNullLatest | ZeroSkippedLatest | ChangedZeroSkippedPreimage | PreservedRegister | ChangedPreservedRegister | InnerFailedRegister | UnopenedRegister | UnopenedRegisterNonNull | OutOfBankRead | ChangedOutOfBankTail | InvalidKindRead | FailedCollect | ChangedFailedCollectCount | ChangedFailedCollectBuffer deriving (Eq)

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
trace :: Role -> Mutation -> [Value]
trace role mutation
  | registerFailure = open ++ register
  | otherwise = open ++ register ++ latest ++ order ++ erase ++ collect ++ positional ++ close
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
  events = trace role mutation
  reset = object ["schema" .= (1::Int),"reset" .= object ["instructions" .= (0::Int)],"regions_at_reset" .= resetRegions role Original
    ,"segments" .= [object ["phase" .= phase,"instructions" .= (1::Int),"hits_end" .= length events,"watch" .= watch role phase mutation,"regions" .= resetRegions role mutation] | phase <- [minBound..maxBound]::[AttachPhase]]]
  files = M.fromList [(Spec,encode spec),(Extents,encode extents),(Reset,encode reset),(Hits,B.intercalate "\n" (map encode events) <> "\n"),(Steps,"[]"),(MediumFile,encode (medium mutation)),(Session,"{}")]
  receipt = encode (object ["schema" .= (1::Int),"role" .= (if role == Stock then "stock" else "owned"::T.Text),"outcome" .= object ["status" .= ("completed"::T.Text)]
    ,"local_ui_drain_tick_bound" .= (833334::Int)
    ,"output_rule" .= ("Compare outputs written by the stock contract across roles; require each contract-untouched output to equal its own entry preimage, without comparing residue across roles."::T.Text)
    ,"inputs" .= object (["update" .= binding (if role == Stock then "stock-DAT" else "owned-DAT"),"routes" .= binding (routeBytes bindings),"contract" .= binding (contractBytes bindings),"linked" .= binding (linkedBytes bindings),"symbols" .= binding (symbolBytes bindings)]
      ++ [K.fromString name .= binding (if mutation == BadBinding && name == "run" then "different-run" else "cohort") | name <- ["run","boot-rom","adjustment-nand","observer","bridge"]])
    ,"files" .= object [K.fromString (sidecarName name) .= hash bytes | (name,bytes) <- M.toList files],"ram_rule" .= extents,"raw_residual_count" .= (0::Int),"settings_at_attach" .= regions mutation,"medium_at_attach" .= medium mutation])

data ResetEvidence = ResetAppends | ResetWithoutAppend | ResetUnsettled deriving (Eq)

data DrainEvidence = Drained | MissingDrain | MissingUi | MissingMenu | MissingDepth | MissingNode | MenuAddressDiffers | MenuAliasDisagrees | PendingQueue | SecondHeld | HeldMain | PendingPriority | PendingWord | AliasDisagrees | MissingViews | RenderUninitialized | RenderBusy | KeyHeld | TaskRunnable | GraphicsBusy | CommandPending | GraphicsControl | IncompleteJob | UnacknowledgedJob | ExcessTicks | ExcessRan | TickOverflow | InstructionOverflow | RanOverflow deriving (Eq, Show, Enum, Bounded)

drainEndpoint :: DrainEvidence -> Word32 -> Value
drainEndpoint evidence cursor = object
  (["instructions" .= (if evidence == InstructionOverflow then 2^(64::Int) else 30000000::Integer)
   ,"ran" .= (case evidence of ExcessRan -> 30000001; RanOverflow -> 2^(64::Int); _ -> 1200000::Integer)
   ,"ticks" .= (case evidence of ExcessTicks -> 833335; TickOverflow -> 2^(64::Int); _ -> 833334::Integer)
   ,"render" .= object
     ["initialized" .= (if evidence == RenderUninitialized then 0 else 1::Int)
     ,"idle" .= (if evidence == RenderBusy then [0,0,0,1,0,0,0] else replicate 7 0::[Int])
     ,"keys_released" .= (replicate 6 True ++ [evidence /= KeyHeld])
     ,"task_statuses" .= (replicate 3 (18::Int) ++ [if evidence == TaskRunnable then 1 else 18])
     ,"graphics" .= object ["busy" .= (evidence == GraphicsBusy),"command_pending" .= (evidence == CommandPending)
       ,"control" .= (if evidence == GraphicsControl then 1 else 0::Int)
       ,"jobs_started" .= (12::Int),"jobs_completed" .= (if evidence == IncompleteJob then 11 else 12::Int)
       ,"acknowledgments" .= (if evidence == UnacknowledgedJob then 11 else 12::Int)]]]
   ++ ["ui" .= object (["queues" .= queues,"words" .= pending]
     ++ ["menu" .= view (if evidence == MenuAddressDiffers then 0x9571d4 else 0x9571d0) (wordBytes 0 <> wordBytes 0 <> wordBytes cursor) | evidence /= MissingMenu]
     ++ ["depth" .= view 0x9575f4 (wordBytes 0) | evidence /= MissingDepth]
     ++ ["node" .= Null | evidence /= MissingNode]) | evidence /= MissingUi])
 where
  view address bytes = object ["address" .= (address::Word32),"physical" .= hex bytes
    ,"spellings" .= [[hex bytes,hex bytes],[hex bytes,hex (if evidence == AliasDisagrees || evidence == MenuAliasDisagrees && address == 0x9571d0 then B.map (+1) bytes else bytes)]]]
  queues = [view 0x949dd0 (B.pack [7,0,if evidence == PendingQueue then 8 else 7,0,if evidence == SecondHeld then 1 else 0,0,0,0])
    ,view 0x94f5d8 (B.pack [3,0,if evidence == HeldMain then 4 else 3,0,if evidence == HeldMain then 1 else 0,0,0,0])]
  pending = [view address (wordBytes value) | (address,value) <- take (if evidence == MissingViews then 3 else 4)
    [(0x950098,if evidence == PendingPriority then 1 else 0),(0x950148,if evidence == PendingWord then 1 else 0),(0x951424,2),(0x951528,0)]]

resetFixture :: ResetEvidence -> (Bindings, Observation)
resetFixture evidence = keyFixture Owned evidence Drained

keyFixture :: Role -> ResetEvidence -> DrainEvidence -> (Bindings, Observation)
keyFixture = keyFixtureAtCursor 0

keyFixtureAtCursor :: Word32 -> Role -> ResetEvidence -> DrainEvidence -> (Bindings, Observation)
keyFixtureAtCursor cursor role evidence drainEvidence = (bound, Observation receipt files) where
  bound = bindings { routeBytes = BL.toStrict (encodeRouteFile (RouteFile (replicate 64 '0') "reset fixture" CrawlEffectsSource []
    (phaseRoutes ++ [Route (RouteName "custom-reset-defaults") (KeyRoute [KeyOk] (Declared CustomResetDefaults))]))) }
  events = filter keep (trace role Original)
  keep event = evidence /= ResetWithoutAppend || not (any (\site -> get "site" (get "observation" event) == A.toJSON (site::Word32)) [0x6ac7d4,0x6fb7c8])
  Observation originalReceipt originalFiles = fixture role Original
  original = decode originalReceipt
  decode bytes = either error id (A.eitherDecodeStrict' bytes)
  get key (Object fields) = maybe (error "fixture field missing") id (KM.lookup key fields)
  get _ _ = error "fixture object required"
  set key value (Object fields) = Object (KM.insert key value fields)
  set _ _ _ = error "fixture object required"
  reset = object ["schema" .= (1::Int),"reset" .= object ["instructions" .= (0::Int)],"regions_at_reset" .= resetRegions role Original
    ,"segments" .= [object ["phase" .= phase,"instructions" .= (1::Int),"hits_end" .= (0::Int),"watch" .= watch role phase Original,"regions" .= resetRegions role Original] | phase <- [minBound..maxBound]::[AttachPhase]]]
  press = object ["failure" .= Null,"settle" .= object ["status" .= (if evidence == ResetUnsettled then "budget-exhausted" else "packet-consumed"::T.Text)]
    ,"menu" .= object ["cursor" .= (0::Int),"menu_id" .= (1::Int),"setting_id" .= (5::Int)]]
  step = object (["step" .= (1::Int),"parent" .= (0::Int),"keys" .= [KeyOk],"hits" .= ([0,length events]::[Int])
    ,"idle" .= object ["ran" .= (20000000::Int)],"press" .= press,"regions_before" .= regions Original,"regions_after" .= regions Original
    ,"medium_after" .= medium Original,"watch" .= set "views" (String "fixed") (watch role LiveViewPhase Original)]
    ++ ["local_ui_drained" .= drainEndpoint drainEvidence cursor | drainEvidence /= MissingDrain])
  files = M.insert Reset (encode reset) (M.insert Hits (B.intercalate "\n" (map encode events) <> "\n") (M.insert Steps (encode (A.toJSON [step])) originalFiles))
  inputs = set "routes" (object ["path" .= ("fixture"::T.Text),"sha256" .= hash (routeBytes bound)]) (get "inputs" original)
  receipt = encode (set "inputs" inputs (set "files" (object [K.fromString (sidecarName name) .= hash bytes | (name,bytes) <- M.toList files]) original))

main :: IO ()
main = do
  let stock = qualify Stock bindings (fixture Stock Original)
      endpointCursorDiffers = case (keyFixtureAtCursor 0 Stock ResetAppends Drained,keyFixtureAtCursor 1 Owned ResetAppends Drained) of
        ((stockBindings,stockObservation),(ownedBindings,ownedObservation)) ->
          case (qualify Stock stockBindings stockObservation,qualify Owned ownedBindings ownedObservation) of
            (Right a,Right b) -> isLeft (comparePair a b)
            _ -> False
      owned mutation = qualify Owned bindings (fixture Owned mutation)
      matchedAgainst baseline mutation = case (qualify Stock bindings (fixture Stock baseline),owned mutation) of
        (Right a,Right b) -> not (isLeft (comparePair a b))
        _ -> False
      failedAgainst baseline mutation = case (qualify Stock bindings (fixture Stock baseline),owned mutation) of
        (Right _,Left _) -> True
        (Right a,Right b) -> isLeft (comparePair a b)
        _ -> False
      matched mutation = case (stock,owned mutation) of
        (Right a,Right b) -> not (isLeft (comparePair a b))
        _ -> False
      differs mutation = case (stock,owned mutation) of
        (Right a,Right b) -> isLeft (comparePair a b)
        _ -> False
      refused mutation = isLeft (owned mutation)
      mappedMatched = case (qualify Stock bindings (fixture Stock MappedMedium),owned MappedMedium) of
        (Right a,Right b) -> not (isLeft (comparePair a b))
        _ -> False
      spareDifference = case (qualify Stock bindings (fixture Stock MappedMedium),owned ChangedSpareByte) of
        (Right a,Right b) -> case comparisonValue a b (comparePair a b) of
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
              report = comparisonValue a b (comparePair a b)
           in case report of
                Object fields -> KM.lookup "coverage" fields == Just (object ["stock" .= expected "stock","owned" .= expected "owned"])
                _ -> False
        _ -> False
      drainAdmitted role evidence = let (bound,observation) = keyFixture role ResetAppends evidence in not (isLeft (qualify role bound observation))
      drainedPair = case (keyFixture Stock ResetAppends Drained,keyFixture Owned ResetAppends Drained) of
        ((bound,a),(_,b)) -> case (qualify Stock bound a,qualify Owned bound b) of
          (Right left,Right right) -> not (isLeft (comparePair left right))
          _ -> False
      wrongDrainPolicy role policy =
        let (bound,observation) = keyFixture role ResetAppends Drained
         in case A.eitherDecodeStrict' (receiptBytes observation) of
              Right (Object fields) ->
                let changed = maybe (KM.delete "local_ui_drain_tick_bound" fields) (\value -> KM.insert "local_ui_drain_tick_bound" value fields) policy
                 in isLeft (qualify role bound observation { receiptBytes = encode (Object changed) })
              _ -> False
      wrongCache value = rejectBinding (changedObject "owned_cache" value) id
      wrongCacheField name value = wrongCache (changedObject name value cacheContract)
      wrongSymbols extra = rejectBinding id (<> C.pack extra)
      tests = [("nine-entry observation matches",matched Original)
        ,("changed drained cursor fails with equal press-time menus",endpointCursorDiffers)
        ,("defaults reset with a consumed append qualifies",let (bound,observation) = resetFixture ResetAppends in not (isLeft (qualify Owned bound observation)))
        ,("defaults reset without a record append refuses",let (bound,observation) = resetFixture ResetWithoutAppend in isLeft (qualify Owned bound observation))
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
        ,("defaults reset with unsettled confirmation refuses",let (bound,observation) = resetFixture ResetUnsettled in isLeft (qualify Owned bound observation))
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
        ,("both roles include the drained key interval",drainedPair)
        ,("main queue held with pending packets is drained",all (\role -> drainAdmitted role HeldMain) [Stock,Owned])
        ,("missing or changed endpoint policy refuses both roles",and [wrongDrainPolicy role policy | role <- [Stock,Owned], policy <- [Nothing,Just (Number 833333)]])]
        ++ [("local UI drain rejects " ++ show evidence ++ " for " ++ show role,not (drainAdmitted role evidence))
           | role <- [Stock,Owned], evidence <- [minBound..maxBound], evidence `notElem` [Drained,HeldMain]]
  case (stock,owned Original) of
    (Left failure,_) -> putStrLn ("stock fixture: " ++ failure)
    (_,Left failure) -> putStrLn ("owned fixture: " ++ failure)
    _ -> pure ()
  mapM_ (\(name,passed) -> putStrLn ((if passed then "PASS " else "FAIL ") ++ name)) tests
  unless (all snd tests) exitFailure
