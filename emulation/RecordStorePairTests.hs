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
import Numeric (showHex)
import System.Exit (exitFailure)

data Mutation = Original | PayloadByte | ReorderCalls | StrayFetch | ForeignRead | CacheAgeWrite | BadSettings | BadMedium | MissingCapture | BadBinding | CollectionTail | ReturnedRecord | CloseScratch | OutputStorage | MissingResetRegion | RetiredSnapshotDifference | PreEntryPublication | PostEntryPublication | PreEntryBssWrite | PreEntryRead | MissingMarker | LaterPhaseWrite | MappedMedium | TruncatedSpare | ChangedSpareByte deriving (Eq)
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
retired role = [[0x89f8f0,0x89f8f8],[0x89fa60,0x89fa68],[0x8e0080,0x8e008c],[0x917fa8,0x917fe4]]
  ++ [[0x8c0080,0x8e0080] | role == Owned]
writes :: Role -> [[Word32]]
writes role = [[rangeStart range,rangeEnd range] | range <- settingsRanges] ++ retired role
extents :: Value
extents = object ["code" .= code,"ram" .= ram,"unwatched_owned_ram" .= ram,"activation_fetch" .= ([0x6ac2b4,0x6ac2b8]::[Word32]),"rule" .= ("fixture"::T.Text)] where
  ram = [[0x89f8f8,0x89fa60],[0x8a0080,0x8c0080]]::[[Word32]]
bindings :: Bindings
bindings = Bindings contract (B.replicate 7776 0) symbols routes where
  symbols = C.pack (unlines ([showHex entry "" ++ " 4 T entry" ++ show n | (n,entry) <- zip [0::Int ..] entries] ++ ["0089f8f8 168 B record_store_state","008a0080 20000 B record_store_block"]))
  routes = BL.toStrict (encodeRouteFile (RouteFile (replicate 64 '0') "fixture" CrawlEffectsSource [] phaseRoutes))
  contract = encode (object ["source_sha256" .= hash "stock-DAT","candidate_sha256" .= hash "owned-DAT","linked_sha256" .= hash (B.replicate 7776 0),"symbols_sha256" .= hash symbols,"reference_scan" .= object ["raw_candidates" .= ([]::[Value])]])
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
trace :: Mutation -> [Value]
trace mutation = open ++ register ++ latest ++ order ++ erase ++ collect ++ positional ++ close where
  open = call 0x6ac2b4 [123,0,0,0] [] 0 []
  register = call 0x6ac328 [0,96,16,0] [readback 0x930000 (B.concat (map wordBytes [0x710000,0x910000,0x710004,0x910004,0x910008]))] 0 [readback 0x910008 (wordBytes 0x89f8b4)]
  pointerStorage = if mutation == OutputStorage then 0x910030 else 0x910010
  latest = call 0x6ac4c8 [0x89f8b4,0,pointerStorage,0] [] 0 [readback pointerStorage (wordBytes 0x8e16a0),readback 0x8e16a0 (B.cons (if mutation == ReturnedRecord then 18 else 17) (B.replicate 4095 17)),readback 0 B.empty]
  append = [hit 0x6ac7d4 0x6ac7d4 0x700100 [0x89f8b4,1,0x910100,0] "entry" [readback 0x910100 (B.replicate 4096 34)]]
    ++ [hit 0x6fb7c8 0x6fb7c8 0x6a8d00 [0x910200,0,0,0] "entry" [readback 0x910200 (B.concat (map wordBytes [0x3d4,0,96,0x408e16a0])),readback 0x8e16a0 (B.cons (if mutation == PayloadByte then 50 else 51) (B.replicate 95 51))]]
    ++ [hit 0x6ac7d4 0x700100 0x700100 [0,0,0,0] "return" []]
  zero = call 0x6ae0fc [99,98,97,96] [] 0 []
  order = if mutation == ReorderCalls then zero ++ append else append ++ zero
  erase = [hit 0x6ac984 0x6ac984 0x700100 [0x89f8b4,0,0,0] "entry" []
    ,hit 0x6fb57c 0x6fb57c 0x6a8c40 [0x910200,0,0,0] "entry" [readback 0x910200 (wordBytes 0x3d4 <> wordBytes 1)]
    ,hit 0x6ac984 0x700100 0x700100 [0,0,0,0] "return" []]
  collected = B.replicate 1535 68 <> B.singleton (if mutation == CollectionTail then 69 else 68) <> B.replicate (131072-1536) 68
  collect = call 0x6acab8 [0x89f8b4,0x920000,0x910014,0] [] 0 [readback 0x920000 collected,readback 0x910014 (wordBytes 96)]
  positional = call 0x6acda0 [0,32,4,0x910020] [] 0 [readback 0x910020 (B.pack [1,2,3,4])]
  close = call 0x6ac478 [0x89f8b4,0,0,0] [] (if mutation == CloseScratch then 999 else 123) []

captureDeclarations :: Value
captureDeclarations = object [K.fromString (hexWord entry) .= declared entry | entry <- entries ++ [0x6fb7c8,0x6fb57c]] where
  direct reg count = object ["base" .= object ["register" .= (reg::Int)],"count" .= (count::Int),"indirect" .= False]
  declared entry
    | entry == 0x6fb7c8 = object ["entry" .= [direct 0 16,object ["base" .= object ["register-word" .= object ["register" .= (0::Int),"offset" .= (12::Int)]],"count" .= (0::Int),"count_base" .= object ["register-word" .= object ["register" .= (0::Int),"offset" .= (8::Int)]]]]]
    | entry == 0x6fb57c = object ["entry" .= [direct 0 8]]
    | otherwise = object ["entry" .= inputs entry,"returns" .= outputs entry]
  inputs entry | entry == 0x6ac328 = [direct 13 20]
               | entry == 0x6ac7d4 = [direct 2 4096]
               | otherwise = []
  outputs entry
    | entry == 0x6ac328 = [object ["base" .= object ["stack-word" .= (16::Int)],"count" .= (4::Int)]]
    | entry == 0x6ac4c8 = [direct 2 4,object ["base" .= object ["register" .= (2::Int)],"count" .= (4096::Int),"indirect" .= True,"on_success" .= True],direct 1 2]
    | entry == 0x6acab8 = [direct 1 131072,direct 2 4]
    | entry == 0x6acda0 = [object ["base" .= object ["register" .= (3::Int)],"count" .= (0::Int),"count_register" .= (2::Int)]]
    | otherwise = []

watch :: Role -> AttachPhase -> Mutation -> Value
watch role phase mutation = object ["enabled" .= True,"ranges" .= ordered,"records" .= records,"violation" .= Null,"views" .= ("observe"::T.Text),"classes" .= classes] where
  ordered = fetches role ++ take 1 (writes role) ++ [[0x89f8f0,0x89f8f8],[0x89fa60,0x89fa68]]
    ++ (if role == Owned then [[0x8c0080,0x8e008c]] else [[0x8e0080,0x8e008c]])
    ++ [[0x917fa8,0x917fe4]] ++ drop 1 (take 3 (writes role))
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
        CacheAgeWrite -> [store]
        PostEntryPublication -> [publication]
        _ -> []
  load = cpu "cpu-read" 0x89f8f0 0x700000 (A.toJSON (0::Int)) (A.toJSON ([0,0,0,0]::[Int]))
  store = cpu "cpu-write" 0x89fa60 0x69ce00 (A.toJSON (0::Int)) (A.toJSON ([0,0,0,0]::[Int]))
  publication = object ["source" .= ("model-bus-write"::T.Text),"address" .= (0x89ec00::Word32),"physical" .= (0x89ec00::Word32),"size" .= (32768::Int),"core" .= Null,"pc" .= Null,"instructions" .= Null,"tick" .= Null]
  cpu source address pc value beforeBytes = object ["source" .= (source::T.Text),"address" .= (address::Word32),"physical" .= address,"size" .= (4::Int),"value" .= value,"before" .= beforeBytes,"byte_order" .= ("little"::T.Text),"core" .= (0::Int),"pc" .= (pc::Word32),"instructions" .= (1::Int),"tick" .= (1::Int)]

fixture :: Role -> Mutation -> Observation
fixture role mutation = Observation receipt files where
  binding bytes = object ["path" .= ("fixture"::T.Text),"sha256" .= hash bytes]
  spec = object ["schema" .= (1::Int),"sites" .= object ([K.fromString (hexWord entry) .= ("record-entry"::T.Text) | entry <- entries] ++ ["0x6fb7c8" .= ("nand-write"::T.Text),"0x6fb57c" .= ("nand-erase"::T.Text)])
    ,"captures" .= (if mutation == MissingCapture then object [] else captureDeclarations),"hit_limit" .= (1500000::Int),"watch_limit" .= (1048576::Int),"write_ranges" .= writes role,"read_ranges" .= retired role,"fetch_ranges" .= fetches role]
  events = trace mutation
  reset = object ["schema" .= (1::Int),"reset" .= object ["instructions" .= (0::Int)],"regions_at_reset" .= resetRegions role Original
    ,"segments" .= [object ["phase" .= phase,"instructions" .= (1::Int),"hits_end" .= length events,"watch" .= watch role phase mutation,"regions" .= resetRegions role mutation] | phase <- [minBound..maxBound]::[AttachPhase]]]
  files = M.fromList [(Spec,encode spec),(Extents,encode extents),(Reset,encode reset),(Hits,B.intercalate "\n" (map encode events) <> "\n"),(Steps,"[]"),(MediumFile,encode (medium mutation)),(Session,"{}")]
  receipt = encode (object ["schema" .= (1::Int),"role" .= (if role == Stock then "stock" else "owned"::T.Text),"outcome" .= object ["status" .= ("completed"::T.Text)]
    ,"inputs" .= object (["update" .= binding (if role == Stock then "stock-DAT" else "owned-DAT"),"routes" .= binding (routeBytes bindings),"contract" .= binding (contractBytes bindings),"linked" .= binding (linkedBytes bindings),"symbols" .= binding (symbolBytes bindings)]
      ++ [K.fromString name .= binding (if mutation == BadBinding && name == "run" then "different-run" else "cohort") | name <- ["run","boot-rom","adjustment-nand","observer","bridge"]])
    ,"files" .= object [K.fromString (sidecarName name) .= hash bytes | (name,bytes) <- M.toList files],"ram_rule" .= extents,"raw_residual_count" .= (0::Int),"settings_at_attach" .= regions mutation,"medium_at_attach" .= medium mutation])

main :: IO ()
main = do
  let stock = qualify Stock bindings (fixture Stock Original)
      owned mutation = qualify Owned bindings (fixture Owned mutation)
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
      tests = [("nine-entry observation matches",matched Original)
        ,("report names exercised entries and NAND requests per role/media",reportedCoverage)
        ,("changed program payload fails comparison",differs PayloadByte)
        ,("reordered external calls fail comparison",differs ReorderCalls)
        ,("changed returned record fails comparison",differs ReturnedRecord)
        ,("changed collection tail fails comparison",differs CollectionTail)
        ,("void-close scratch return is excluded",matched CloseScratch)
        ,("output storage stack address is excluded",matched OutputStorage)
        ,("missing retired reset snapshot refuses qualification",refused MissingResetRegion)
        ,("private reset bytes are excluded from settings comparison",matched RetiredSnapshotDifference)
        ,("stray retired fetch refuses qualification",refused StrayFetch)
        ,("post-entry stock-only read refuses qualification",refused ForeignRead)
        ,("stock cache-age access refuses qualification",refused CacheAgeWrite)
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
        ,("changed cohort input fails comparison",differs BadBinding)]
  case (stock,owned Original) of
    (Left failure,_) -> putStrLn ("stock fixture: " ++ failure)
    (_,Left failure) -> putStrLn ("owned fixture: " ++ failure)
    _ -> pure ()
  mapM_ (\(name,passed) -> putStrLn ((if passed then "PASS " else "FAIL ") ++ name)) tests
  unless (all snd tests) exitFailure
