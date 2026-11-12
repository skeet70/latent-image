{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}
module Replacement.RecordStore.Harness where

import Data.Aeson (Value, object, (.=))
import Data.Bits
import qualified Data.ByteString as B
import qualified Data.Map.Strict as M
import Data.Maybe (listToMaybe)
import Data.Word
import Emulation.NativeObservation (GuestAddress(..), RegionId(..), Register(..), RunnerStatus(..), hexWord32)
import Emulation.OwnedCase (CaseSpec(..), Permission(..), mkRegion, readWordAt, word32le)
import qualified Replacement.RecordStore.Arm as A
import Replacement.RecordStore.Cases
import Replacement.RecordStore.Hardware (hardware)
import Replacement.RecordStore.Model

entryAddress :: Operation -> Word32
entryAddress op = case operationBody op of
  Open -> 0x6ac2b4
  Register _ -> 0x6ac328
  Close -> 0x6ac478
  Latest -> 0x6ac4c8
  Append _ _ -> 0x6ac7d4
  EraseAll -> 0x6ac984
  Collect -> 0x6acab8
  ReadAt _ _ -> 0x6acda0
  Zero -> 0x6ae0fc
  CacheBoundary _ _ _ -> 0x6ae0fc
  On _ _ -> error "selected entry"

-- | The nine export rows of the carrier's link script, in the suite's own
-- operation order. The extent is the entry veneer the stock role authenticates.
entryTable :: [Word32]
entryTable =
  map entryAddress
    [ Open
    , Register Normal
    , Close
    , Latest
    , Append (Seed 0) CallerSource
    , EraseAll
    , Collect
    , ReadAt (BytePosition 0) 16
    , Zero
    ]

-- | The static audit's entered side, emitted from this suite's own cases: one
-- row per entry a case ends in, in the entry table's order. An entry no case
-- ends in has no row, so the audit reports it rather than a declaration
-- covering it.
caseTableValue :: Value
caseTableValue = object
  [ "schema" .= (1 :: Int)
  , "suite" .= ("record-store" :: String)
  , "source" .= ("Replacement.RecordStore.Harness entryAddress" :: String)
  , "entries" .=
      [ object
          [ "case" .= caseName (name c)
          , "entry" .= hexWord32 address
          , "extent" .= (4 :: Int)
          , "stock" .= ("image" :: String)
          ]
      | address <- entryTable
      , Just c <- [firstCaseEndingAt address]
      ]
  ]
  where
    firstCaseEndingAt address =
      listToMaybe [c | c <- caseGrid, entryAddress (last (operations c)) == address]

modelBase :: Word32
modelBase = 0x510000

-- NAND request ABIs: id4.dis 0x6a8c5c..0x6a8c7c, 0x6a8ce8..0x6a8d28 and 0x6a8d90..0x6a8dc8.
mediumCode :: OracleCase -> [A.Ins]
mediumCode c = concat
  [ [A.at "fn_wait", A.mov 0 0 0, A.ret]
  , [A.at "fn_release", A.mov 0 0 0, A.ret]
  , [A.at "fn_read", A.push [4,5,6,14], A.movR 4 0] ++ readFaultCode ++
      [A.ldr 4 0 0, A.sub 0 0 first, A.lsl 0 0 17]
      ++ A.load 1 mediumBase ++ [A.addR 1 1 0, A.ldr 4 0 12, A.bic 0 0 0x40000000, A.ldr 4 2 8, A.bl "fn_copy", A.mov 0 0 0, A.pop [4,5,6,15]]
  , [A.at "fn_program", A.push [4,5,6,7,8,9,10,14], A.movR 4 0, A.mov 5 0 1, A.bl "fn_persist", A.pop [4,5,6,7,8,9,10,15]]
  , [A.at "fn_erase", A.push [4,5,6,7,8,9,10,14], A.movR 4 0, A.mov 5 0 2, A.bl "fn_persist", A.pop [4,5,6,7,8,9,10,15]]
  , [A.at "fn_persist"] ++ A.load 10 cellsBase ++
      [ A.ldr 10 6 0, A.ldr 10 7 8, A.cmpR 6 7, A.whenEq "persist_stop"
      , A.ldr 10 7 12, A.cmpR 6 7, A.whenEq "persist_stop"
      , A.ldr 10 7 16, A.movR 8 7, A.str 7 5 0, A.ldr 4 0 0, A.str 7 0 4
      , A.cmp 5 0 2, A.whenEq "journal_erase"
      , A.ldr 4 0 4, A.str 7 0 8, A.ldr 4 2 8, A.str 7 2 12
      , A.add 0 7 16, A.ldr 4 1 12, A.bic 1 1 0x40000000
      , A.push [14], A.bl "fn_copy", A.pop [14], A.jump "journal_done"
      , A.at "journal_erase", A.mov 0 0 0, A.str 7 0 8, A.ldr 4 0 4, A.lsl 0 0 17, A.str 7 0 12
      , A.at "journal_done" ] ++ A.load 0 (fromIntegral requestStride) ++
      [ A.addR 7 8 0, A.str 10 7 16, A.add 7 6 1, A.str 10 7 0
      , A.ldr 10 7 4, A.cmpR 6 7, A.whenEq "persist_fail"
      , A.ldr 4 0 0, A.sub 0 0 first, A.lsl 0 0 17 ] ++ A.load 1 mediumBase ++
      [ A.addR 0 1 0, A.cmp 5 0 2, A.whenEq "land_erase"
      , A.ldr 4 1 4, A.addR 0 0 1, A.ldr 4 1 12, A.bic 1 1 0x40000000, A.ldr 4 2 8
      , A.at "program_loop", A.cmp 2 0 0, A.whenEq "persist_done", A.ldrb 1 3 0, A.ldrb 0 12 0, A.andR 3 3 12, A.strb 0 3 0
      , A.add 0 0 1, A.add 1 1 1, A.sub 2 2 1, A.jump "program_loop"
      , A.at "land_erase", A.ldr 4 2 4, A.lsl 2 2 17 ] ++
      A.load 3 0xffffffff ++ [A.at "erase_loop", A.str 0 3 0, A.add 0 0 4, A.sub 2 2 4, A.cmp 2 0 0, A.whenNe "erase_loop"
      , A.at "persist_done", A.mov 0 0 0, A.ret
      , A.at "persist_fail", A.mov 0 0 1, A.ret
      , A.at "persist_stop" ] ++ A.load 0 0x5150 ++ [A.str 0 0 0, A.ret]
  , [A.at "fn_copy", A.cmp 2 0 0, A.whenEq "copy_done", A.ldr 1 3 0, A.str 0 3 0, A.add 0 0 4, A.add 1 1 4, A.sub 2 2 4, A.jump "fn_copy", A.at "copy_done", A.ret]
  ]
  where
    g = geometry (kind c)
    PoolBlock first = if isSequence c then PoolBlock 0x52 else firstBlock g
    readFaultCode = case injection c of
      FailRead (ReadFault (PoolBlock block) (ReadOccurrence occurrence)) ->
        [A.ldr 4 0 0] ++ A.load 1 block ++ [A.cmpR 0 1,A.whenNe "read_continue"] ++ A.load 5 (cellsBase + 28) ++
        [A.ldr 5 0 0,A.add 0 0 1,A.str 5 0 0] ++ A.load 1 occurrence ++
        [A.cmpR 0 1,A.whenNe "read_continue",A.mov 0 0 1,A.pop [4,5,6,15],A.at "read_continue"]
      _ -> []

callbackCode :: OracleCase -> [A.Ins]
callbackCode c = one "fn_check" 1 ++ one "fn_prepare" 2
  where
    g = geometry (kind c)
    size = fromIntegral (recordBytes g)
    one name role =
      [A.at name, A.push [4,5,6,7,8,9,10,14], A.movR 4 0, A.movR 9 1] ++ A.load 10 cellsBase ++
      [A.ldr 10 5 20, A.movR 6 5, A.mov 0 0 role, A.str 5 0 0, A.add 0 5 8, A.movR 1 4] ++ A.load 2 size ++
      [A.bl "fn_copy", A.mov 7 0 0] ++ mutate role ++
      [A.str 6 7 4] ++ A.load 0 (8+size) ++ [A.addR 0 6 0, A.movR 1 4] ++ A.load 2 size ++
      [A.bl "fn_copy"] ++ A.load 0 (8+2*size) ++
      [A.addR 6 6 0, A.str 10 6 20, A.ldr 10 0 24, A.add 0 0 1, A.str 10 0 24, A.movR 0 7, A.pop [4,5,6,7,8,9,10,15]]
    mutate role
      | callbacks c == Hardware2 = [A.movR 0 4, A.movR 1 9, A.bl (if role == 1 then "hardware_check2" else "hardware_prepare2")] ++ [A.movR 7 0 | role == 1]
      | callbacks c == Hardware3 = [A.movR 0 4, A.movR 1 9, A.bl (if role == 1 then "hardware_check3" else "hardware_prepare3")] ++ [A.movR 7 0 | role == 1]
      | role == 2 = [A.mov 0 0 0x5a, A.strb 4 0 4]
      | otherwise = case callbacks c of
          CorrectingCallback -> [A.mov 0 0 0x42, A.strb 4 0 3]
          _ -> [A.ldrb 4 0 2, A.cmp 0 0 0xee, A.Word 0x03a07001]

driverCode :: OracleCase -> [A.Ins]
driverCode c = [A.at "fn_driver", A.push [12,14], A.sub 13 13 24] ++ concat (zipWith operation [0..] (operations c)) ++ [A.add 13 13 24, A.pop [12,15]]
  where
    loadHandle = A.load 12 resultBase ++ [A.ldr 12 0 0]
    operation i wrapped =
      let op = operationBody wrapped
          selected = operationKind (kind c) wrapped
      in arguments selected i op ++ [A.bl ("entry_" ++ show i)] ++ save selected i op
    arguments selected i op = let g = geometry selected in case op of
      Open -> [A.mov 0 0 0]
      Register refusal ->
        let size = case refusal of TooSmall -> 1; Oversized -> 0x20001; _ -> fromIntegral (recordBytes g)
            count = if refusal == TooMany then 0x8000 else fromIntegral (2 * recordCount g)
            q = if refusal == OddOffset then 1 else 0
            cb name = if callbacks c == NoCallback then [A.mov 12 0 0] else [A.address 12 name]
            context = case callbacks c of Hardware2 -> A.load 12 0x828568; Hardware3 -> A.load 12 0x828570; _ -> [A.mov 12 0 0]
        in A.load 0 (fromIntegral (fromEnum selected)) ++ A.load 1 size ++ A.load 2 count ++ A.load 3 q ++ cb "fn_prepare" ++
           [A.str 13 12 0] ++ context ++ [A.str 13 12 4] ++ cb "fn_check" ++ [A.str 13 12 8] ++ context ++ [A.str 13 12 12] ++ A.load 12 resultBase ++ [A.str 13 12 16]
      Close -> loadHandle
      Latest -> loadHandle ++ A.load 1 (slot i + 8) ++ A.load 2 (slot i + 4)
      Append (Seed seed) source -> loadHandle ++ A.load 1 (fromIntegral seed) ++ A.load 2 (if source == CallerSource then sourceBase else stagingAddress g)
      EraseAll -> loadHandle
      Collect -> loadHandle ++ A.load 1 callerBase ++ A.load 2 (slot i + 8)
      ReadAt (BytePosition position) len -> A.load 0 (fromIntegral (fromEnum selected)) ++ A.load 1 position ++ A.load 2 (fromIntegral len) ++ A.load 3 callerBase
      Zero -> []
      On _ _ -> error "selected arguments"
      CacheBoundary timestamp age0 age1 ->
        A.load 12 0x8e0088 ++ A.load 0 timestamp ++ [A.str 12 0 0] ++ A.load 12 0x89fa60 ++ A.load 0 age0 ++ [A.str 12 0 0] ++ A.load 0 age1 ++ [A.str 12 0 4]
    slot i = resultBase + 0x100 + fromIntegral (i * eventStride)
    save selected i op = let g = geometry selected in A.load 12 (slot i) ++ [A.str 12 0 0 | op /= Close] ++
      [A.str 12 r (12 + 4*(r-4)) | r <- [4..11]] ++ [A.str 12 13 44] ++
      (if op == Latest then [A.cmp 0 0 0, A.Branch 1 False (A.label ("latest_done_" ++ show i)), A.ldr 12 1 4] ++ A.load 0 (slot i+64) ++ A.load 2 (fromIntegral (recordBytes g)) ++ [A.bl "fn_copy", A.at ("latest_done_" ++ show i)] else [])
      ++ (case op of ReadAt _ len | isSequence c -> A.load 0 (slot i + 64) ++ A.load 1 callerBase ++ A.load 2 (fromIntegral len) ++ [A.bl "fn_copy"]; _ -> [])

-- Range rows come from the pinned decomp index; only the executed closure is declared.
data Function = Function Word32 Word32 deriving (Eq, Show)
functionClosure :: B.ByteString -> [Function] -> [Word32] -> [Function]
functionClosure = functionClosureExcept []

functionClosureExcept :: [Word32] -> B.ByteString -> [Function] -> [Word32] -> [Function]
functionClosureExcept seams image ranges roots = go [] roots
  where
    go seen [] = seen
    go seen (entry:rest)
      | any (\(Function address size) -> entry >= address && entry < address+size) seen = go seen rest
      | entry `elem` seams || entry `elem` [0x724b68,0x724c00,0x42634,0x4263c,0x6faf00,0x6faf40,0x6fb904,0x6fb7c8,0x6fb57c] = go seen rest
      | otherwise = case [f | f@(Function address size) <- ranges, entry >= address && entry < address+size] of
          [] -> error ("missing stock function extent " ++ show entry)
          f@(Function start size):_ ->
            let targets = [target | offset <- [0,4..size-4], let pc = start+offset, let w = readWordAt image (fromIntegral (pc-0x61c400)), w .&. 0x0e000000 == 0x0a000000,
                  let displacement = fromIntegral (((w .&. 0xffffff) `xor` 0x800000) - 0x800000) :: Int,
                  let target = pc+8+fromIntegral (4*displacement), target < start || target >= start+size,
                  target >= 0x61c400, target < 0x89ec28]
            in go (f:seen) (targets ++ rest)

caseSpec :: B.ByteString -> B.ByteString -> [Function] -> OracleCase -> CaseSpec
caseSpec image loader ranges c = CaseSpec
  { caseId = caseName (name c)
  , caseProvenance = "RSC byte-read contract, reports/record-service-stock-contract.md; canonical image4 f2e2acab892dafeed85171f6870a411f0a2d5eae10921b86a24630d401dcff16. Medium requests execute declared synthetic A32 code."
  , caseExclusion = if isSequence c
      then "Host Cortex-R5F native profile. Physical read faults are block-specific and occurrence-specific, return failure without writing destination, and preserve RAM across operations. Only caller outputs, status, callbacks and persistence effects are compared. CacheBoundary writes the declared cache metadata at an explicit guest RAM boundary. Void close r0 is excluded."
      else "Host Cortex-R5F native profile. NAND erase=ff, program=AND, atomic persistence steps and injected request failure are declared medium laws, not device physics. Read requests/cache internals are excluded; raw read failure is stock diagnostic only. Void close r0 is excluded."
  , caseEntry = addr driverBase, caseExit = addr exitAddress
  , caseRegions =
      [region "record-code" 0x6a6b74 ReadExecute (slice 0x6a6b74 0x6ae114)
      , region "runtime-code" 0x722054 ReadExecute (slice 0x722054 0x7223e4)
      , region "divide-code" 0x89aa90 ReadExecute (slice 0x89aa90 0x89abd8)
      , region "loader-divide" 0x4d978 ReadExecute (B.take 0x114 (B.drop 0xd978 loader))
      , region "descriptors" 0x7c8c6c ReadOnly (slice 0x7c8c6c 0x7c8db4)
      , region "request-entries" 0x6faf00 ReadExecute trampolines
      , region "driver" driverBase ReadExecute (driver <> B.replicate (fromIntegral (modelBase-driverBase) - B.length driver) 0 <> modelBytes)
      , region "raw-state" 0x828568 ReadWrite (slice 0x828568 0x828574 <> rawState)
      , region "record-ram" 0x89f8b0 ReadWrite (B.replicate (0x917fe4-0x89f8b0) 0)
      , region "results" resultBase ReadWrite (initialResults <> cells), region "caller" callerBase ReadWrite initialCaller
      , region "source" sourceBase ReadWrite (sourceBytes g)
      , region "medium" mediumBase ReadWrite (if isSequence c then mediumBytes (initialMedium c) else B.concat [bankBytes g bankNo (initialMedium c) | bankNo <- [Bank0,Bank1]])
      , region "requests" requestBase ReadWrite (B.replicate requestSize 0)
      , region "callbacks" callbackBase ReadWrite (B.replicate callbackSize 0)
      , region "stack" stackBase ReadWrite (B.replicate 0x10000 0)
      ]
  , caseFunctions = [(addr a,size) | Function a size <- closure] ++ [(addr a,size) | (a,size) <- driverFunctions ++ modelFunctions ++ trampolineFunctions] ++ [(addr a,size) | (a,size) <- [(0x4d978,0xf4),(0x4da6c,0x20)]]
  , caseInitial = [(R0,0),(R1,0),(R2,0),(R3,0),(R12,0),(Sp,stackTop),(Lr,exitAddress),(Cpsr,0x10)] ++ zip [R4,R5,R6,R7,R8,R9,R10,R11] [0x44440004..0x4444000b]
  , caseExpectedStatus = if isStop then MemoryGuard else Returned
  , caseExpectedRegisters = [(R0,0x5150) | isStop] ++ [(Sp,stackTop) | not isStop] ++ [(r,value) | not isStop, (r,value) <- zip [R4,R5,R6,R7,R8,R9,R10,R11] [0x44440004..0x4444000b]]
  , caseExpectedMemory = [(RegionId ident,bytes) | (ident,bytes) <- [("results",expectedResults expected),("caller",expectedCaller expected),("medium",expectedMedium c expected),("requests",B.concat (map encodeRequest (requests expectedPersistence))),("callbacks",B.concat (map encodeCallback (callbackEvents (expectedStore expected))))], not (B.null bytes)]
  , caseIgnoredMemory = []
  , caseCp14Identity = Nothing, caseKernel = Nothing
  }
  where
    g = geometry (kind c)
    addr = GuestAddress . fromIntegral
    region ident base permission bytes = mkRegion (RegionId ident) (addr base) (B.length bytes) permission bytes
    driverInstructions = driverCode c
    seed = [("entry_" ++ show i, entryAddress op) | (i,op) <- zip [0::Int ..] (operations c)] ++ [("fn_copy",resolve "fn_copy"),("fn_check",resolve "fn_check"),("fn_prepare",resolve "fn_prepare")]
    (driver,_,driverFunctions) = A.assemble driverBase seed driverInstructions
    (modelBytes,modelSymbols,modelFunctions) = A.assemble modelBase [("hardware_check2",0x6a6c54),("hardware_check3",0x6a84d4),("hardware_prepare2",0x6a6c04),("hardware_prepare3",0x6a8484)] (mediumCode c ++ callbackCode c)
    resolve key = M.findWithDefault (error ("missing model symbol " ++ key)) (A.label key) modelSymbols
    trampolineFunctions = [(a,4) | (a,_) <- trampolineEntries]
    trampolineEntries = [(0x6faf00,"fn_wait"),(0x6faf40,"fn_release"),(0x6fb904,"fn_read"),(0x6fb7c8,"fn_program"),(0x6fb57c,"fn_erase")]
    trampolines = foldl (\bytes (a,key) -> replace (fromIntegral (a-0x6faf00)) (word32le (0xea000000 .|. ((resolve key-a-8) `shiftR` 2 .&. 0xffffff))) bytes) (B.replicate (0x6fb908-0x6faf00) 0) trampolineEntries
    slice :: Word32 -> Word32 -> B.ByteString
    slice start end = B.take (fromIntegral (end-start)) (B.drop (fromIntegral (start-0x61c400)) image)
    rawState = word32le 1 <> B.replicate 8 0 <> B.concat [half i | i <- [0..127]]
    cells = B.concat (map word32le [0,failAt,stopAt,0xffffffff,requestBase,callbackBase,0]) <> B.replicate (0x100-28) 0
    failAt = case injection c of FailRequest (PersistenceStep n) -> fromIntegral n; _ -> 0xffffffff
    stopAt = case injection c of StopAfter (PersistenceStep n) -> fromIntegral n; _ -> 0xffffffff
    expected = model hardware c
    expectedPersistence = persistence (expectedStore expected)
    isStop = stopped expectedPersistence
    roots = map entryAddress (operations c) ++ [entry | callbacks c == Hardware2, entry <- [0x6a6c54,0x6a6c04]] ++ [entry | callbacks c == Hardware3, entry <- [0x6a84d4,0x6a8484]]
    closure = functionClosure image ranges roots
