{-# LANGUAGE GHC2021 #-}
module Replacement.RecordStore.Cases where

import Data.Bits (xor)
import qualified Data.ByteString as B
import qualified Data.Map.Strict as M
import Data.Word
import Emulation.OwnedCase (word32le)
import Replacement.RecordStore.Model

newtype CaseName = CaseName { caseName :: String } deriving (Eq, Ord, Show)
data Source = CallerSource | StagingSource deriving (Eq, Show)
data Registration = Normal | OddOffset | TooSmall | TooMany | Oversized deriving (Eq, Show)
data Operation = Open | Register Registration | Close | Latest | Append Seed Source | EraseAll | Collect | ReadAt BytePosition Int | Zero | On Kind Operation | CacheBoundary Word32 Word32 Word32 deriving (Eq, Show)
data OracleCase = OracleCase { name :: CaseName, kind :: Kind, initialMedium :: Medium, operations :: [Operation], callbacks :: Callback, injection :: Fault } deriving (Eq, Show)
operationKind :: Kind -> Operation -> Kind
operationKind _ (On k _) = k
operationKind k _ = k
operationBody :: Operation -> Operation
operationBody (On _ op) = operationBody op
operationBody op = op
isSequence :: OracleCase -> Bool
isSequence = any selected . operations
  where
    selected (On _ _) = True
    selected _ = False
sequenceCases :: [OracleCase]
sequenceCases =
  [ make "ab-a-nofault" NoFault [a,b,a]
  , make "ab-a-second-a-fails" (fails Kind0 Bank0 2) [a,b,a]
  , make "first-fill-failure-memoized" (fails Kind0 Bank0 1) [a,a]
  , make "a0-a1-a0-second-a0-fails" (fails Kind0 Bank0 2) [a,a1,a]
  , make "three-kind-evicts-a" (fails Kind0 Bank0 2) [a,b,c,a]
  , make "equal-age-tie-evicts-slot-zero" (fails Kind0 Bank0 2) [a,b,CacheBoundary 9 7 7,c,a]
  , make "unsigned-age-wrap" (fails Kind0 Bank0 2) [a,b,CacheBoundary 0xffffffff 0xfffffffe 0xffffffff,a,c,b,a]
  , make "latest-check-order" (fails Kind0 Bank0 2) [On Kind0 (Register Normal),On Kind0 Latest, b,On Kind0 Latest]
  ]
  where
    a = On Kind0 (ReadAt (BytePosition 0) 16)
    a1 = On Kind0 (ReadAt (BytePosition 0x20000) 16)
    b = On Kind1 (ReadAt (BytePosition 0) 16)
    c = On Kind2 (ReadAt (BytePosition 0) 16)
    fails k bankNo occurrence = FailRead (ReadFault (poolBlock (geometry k) bankNo) (ReadOccurrence occurrence))
    bytes = foldl (\m k -> putBank (geometry k) Bank1 (bank (geometry k) [(20,False)]) (putBank (geometry k) Bank0 (bank (geometry k) [(10 + fromIntegral (fromEnum k),False),(11 + fromIntegral (fromEnum k),True)]) m)) (Medium M.empty) [Kind0,Kind1,Kind2]
    make label fault ops = OracleCase (CaseName ("sequence-" ++ label)) Kind0 bytes (Open:ops) MarkerCallback fault
record :: Geometry -> Word16 -> Bool -> B.ByteString
record g seqNo rejected = half seqNo <> B.singleton (if rejected then 0xee else 0x22) <> B.replicate (recordBytes g - 3) 0x33
bank :: Geometry -> [(Word16, Bool)] -> B.ByteString
bank g records = B.concat [record g seqNo rejected | (seqNo,rejected) <- records] <> B.replicate (blockBytes - length records * recordBytes g) 0xff
mediumOf :: Geometry -> [(Word16, Bool)] -> [(Word16, Bool)] -> Medium
mediumOf g a b = putBank g Bank1 (bank g b) (putBank g Bank0 (bank g a) (Medium M.empty))
caseGrid :: [OracleCase]
caseGrid = concatMap families [minBound .. maxBound]
  where
    families k =
      let g = geometry k
          n = recordCount g
          run tag a b ops = OracleCase (CaseName ("kind-" ++ show (fromEnum k) ++ "-" ++ tag)) k (mediumOf g a b) (Open : Register Normal : ops) MarkerCallback NoFault
          partial = [(10,False),(11,False),(12,False)]
          full start = [(fromIntegral (start + p),False) | p <- [0 .. n - 1]]
          appendOp = Append (Seed 0x1234) CallerSource
          empty = run "empty" [] [] [Latest]
          appendCase tag a b = run tag a b [appendOp, Latest]
          basic =
            [ empty
            , run "partial" partial [] [Latest]
            , run "full" (full 0) [] [Latest]
            , run "both-full" (full 0) (full n) [Latest]
            , run "wrap" [(0x7ffe,False),(0x7fff,False)] [(0,False),(1,False)] [Latest]
            , run "newest-rejected" [(10,False),(11,True)] [] [Latest]
            , run "all-rejected" [(10,True),(11,True)] [(12,True)] [Latest, Latest]
            , run "reject-zero" [(10,True)] [] [Latest]
            , run "start-not-newest" [(10,False)] [(11,False)] [Latest]
            , appendCase "append-room" partial []
            , appendCase "append-rotate" (full 0) []
            , appendCase "append-queried-different" [(0,False)] (full 1)
            , appendCase "append-both-full" (full 0) (full n)
            , appendCase "append-rejected-predecessor" [(10,False),(11,True)] []
            , appendCase "append-mask" [(0x7fff,False)] []
            , appendCase "append-rotate-mask" (full (0x8000-n)) []
            , appendCase "append-seed" [] []
            , run "append-unmasked-seed" [] [] [Append (Seed 0x9234) CallerSource,Latest]
            , run "append-staging-source" partial [] [Append (Seed 0) StagingSource, Latest]
            , run "fresh-append" [] [] [appendOp]
            , (run "correcting-check" partial [] [Latest]) { callbacks = CorrectingCallback }
            , run "erase-all" partial [(20,False)] [EraseAll, Latest]
            , run "erase-current-one" partial [(20,False)] [Latest, EraseAll]
            , run "collect-empty" [] [] [Collect]
            , run "collect-partial" partial [] [Collect]
            , run "collect-out-of-order" [(20,False),(21,True)] partial [Collect]
            , run "collect-wrap-numeric" [(0x7fff,False)] [(0,False)] [Collect]
            , run "read-at-zero" partial [] [ReadAt (BytePosition 0) 16]
            , run "read-at-bank-one" [] partial [ReadAt (BytePosition 0x20000) 16]
            , run "read-at-last" [] [] [ReadAt (BytePosition 0x3ffff) 1]
            , run "read-at-crossing" [] [] [ReadAt (BytePosition 0x1ffff) 2]
            , run "read-at-past" [] [] [ReadAt (BytePosition 0x40000) 16]
            , (run "read-at-unregistered" partial [] []) {operations = [Open,ReadAt (BytePosition 0) 16]}
            , run "open-twice" [] [] [Open, Zero]
            , run "close-reregister" partial [] [Close, Register Normal, Latest]
            , run "inactive" partial [] [Close, Latest, appendOp, EraseAll, Collect]
            , run "registration-exhaustion" [] [] (replicate 5 (Register Normal))
            ]
          refusals = [OracleCase (CaseName ("kind-" ++ show (fromEnum k) ++ "-register-" ++ label)) k (mediumOf g [] []) [Open, Register refusal] NoCallback NoFault | (refusal,label) <- [(OddOffset,"odd-offset"),(TooSmall,"too-small"),(TooMany,"too-many"),(Oversized,"oversized")]]
          unopened = [OracleCase (CaseName ("kind-" ++ show (fromEnum k) ++ "-unopened")) k (mediumOf g [] []) [Register Normal, Latest, Append (Seed 0) CallerSource, EraseAll, Collect, Close, Zero, ReadAt (BytePosition 0) 16] NoCallback NoFault]
          faultCases = concat
            [ [ (run ("fault-" ++ tag ++ "-" ++ show step) a b ops) { injection = FailRequest (PersistenceStep step) } | step <- [0 .. stepsCount - 1] ]
              ++ [(run ("stop-" ++ tag ++ "-" ++ show step) a b ops) { injection = StopAfter (PersistenceStep step) } | step <- [0 .. stepsCount]]
            | (tag,a,b,ops,stepsCount) <-
                [("append",partial,[],[appendOp],1),("empty-append",[],[],[appendOp],1),("rotate-room",full 0,[],[appendOp],1),("rotate",full 0,full n,[appendOp],2),("erase",partial,[(20,False)],[EraseAll],2)]]
      in basic ++ refusals ++ unopened ++ faultCases

hardwareCases :: B.ByteString -> B.ByteString -> B.ByteString -> [OracleCase]
hardwareCases area2a area2b area3 =
  [ OracleCase (CaseName "hardware-kind2-banks") Kind2 (putBank g2 Bank1 area2b (putBank g2 Bank0 area2a (Medium M.empty))) [Open,Register Normal,Latest,Append (Seed 0) CallerSource,Latest] Hardware2 NoFault ]
  ++ [ OracleCase (CaseName ("hardware-kind3-" ++ tag)) Kind3 (putBank g3 Bank1 erased (putBank g3 Bank0 (bytes <> B.replicate (blockBytes - B.length bytes) 0xff) (Medium M.empty))) [Open,Register Normal,Latest] Hardware3 NoFault
     | (tag, bytes) <- [("accept",area3),("bitflip",flipByte 18 8 area3),("byteflip",flipByte 18 0xff area3)] ]
  ++ [ OracleCase (CaseName "hardware-kind3-prepare") Kind3 (mediumOf g3 [] []) [Open,Register Normal,Append (Seed 0x1234) CallerSource,Latest] Hardware3 NoFault ]
  where
    g2 = geometry Kind2
    g3 = geometry Kind3
    flipByte offset mask bytes = replace offset (B.singleton (B.index bytes offset `xor` mask)) bytes

resultBase, callerBase, sourceBase, mediumBase, requestBase, callbackBase, cellsBase, stackBase, stackTop, driverBase, exitAddress :: Word32
resultBase = 0x32000000
callerBase = 0x31000000
sourceBase = 0x30000000
mediumBase = 0x35000000
requestBase = 0x33000000
callbackBase = 0x34000000
cellsBase = resultBase + 0x10000
stackBase = 0x3b000000
stackTop = stackBase + 0x10000
driverBase = 0x500000
exitAddress = driverBase + 0x8000
resultSize, callerSize, requestSize, callbackSize, eventStride, requestStride :: Int
resultSize = 0x10000
callerSize = 0x20000
requestSize = 0x20000
callbackSize = 0x20000
eventStride = 0x1100
requestStride = 0x1010
initialResults :: B.ByteString
initialResults = B.replicate resultSize 0xa5
initialCaller :: B.ByteString
initialCaller = B.replicate callerSize 0xa5
sourceBytes :: Geometry -> B.ByteString
sourceBytes g = B.replicate (recordBytes g) 0x33

-- RSC status and caller-output branches; slot internals are not observations.
data Expected = Expected { expectedResults :: B.ByteString, expectedCaller :: B.ByteString, expectedStore :: Store, expectedStatus :: Bool } deriving Show
model :: HardwareCheck -> OracleCase -> Expected
model hardware c = finish (go 0 False [] Nothing initialResults initialCaller (initialStore (kind c) (initialMedium c) (injection c) (callbacks c)) (operations c))
  where
    finish (results, caller, state) = Expected results caller state (stopped (persistence state))
    go :: Int -> Bool -> [Int] -> Maybe Int -> B.ByteString -> B.ByteString -> Store -> [Operation] -> (B.ByteString, B.ByteString, Store)
    go _ _ _ _ results caller state [] = (results,caller,state)
    go i opened active current results caller state (op:rest)
      | On selected nested <- op = go i opened active current results caller (state {storeKind = selected}) (nested:rest)
      | stopped (persistence state) = (results,caller,state)
      | otherwise =
        let g = geometry (storeKind state)
            offset = 0x100 + i * eventStride
            save value bytes = replace offset (word32le value) bytes
            valid = opened && maybe False (`elem` active) current
            saveABI bytes = foldl (\b (j,value) -> replace (offset + 12 + j * 4) (word32le value) b) bytes (zip [0..] ([0x44440004 + fromIntegral j | j <- [0..7::Int]] ++ [stackTop - 32]))
            continue status nextResults nextCaller nextState nextOpened nextActive nextCurrent =
              if stopped (persistence nextState) then (results,caller,nextState)
              else go (i+1) nextOpened nextActive nextCurrent (saveABI (save status nextResults)) nextCaller nextState rest
            same status = continue status results caller state opened active current
        in case op of
          Open -> continue (if opened then statusUnavailable else 0) results caller state True active current
          CacheBoundary timestamp age0 age1 ->
            let CachePair slot0 slot1 = cacheSlots state
            in continue 0 results caller (state {cacheTimestamp = timestamp, cacheSlots = CachePair (slot0 {cacheAge = age0}) (slot1 {cacheAge = age1})}) opened active current
          Register refusal
            | not opened -> continue statusUnavailable (replace 0 (word32le 0) results) caller state opened active current
            | refusal /= Normal -> same statusInvalid
            | length active == 5 -> same statusInvalid
            | otherwise -> let slot = case [index | index <- [0..4], index `notElem` active] of first:_ -> first; [] -> error "wrapper allocation invariant"
                               handle = 0x89f8b4 + fromIntegral slot * 12
                           in continue 0 (replace 0 (word32le handle) results) caller (state {currentBank = Bank0}) opened (active ++ [slot]) (Just slot)
          Close -> go (i+1) opened (maybe active (\slot -> filter (/=slot) active) current) current (saveABI results) caller state rest
          Zero -> same 0
          Latest | not valid -> continue statusUnavailable (replace (offset + 4) (word32le 0) results) caller state opened active current
                 | otherwise ->
              let (latest, next) = readLatest WriteRecoveryCount hardware state
                  written = case latest of
                    Nothing -> results
                    Just v -> replace (offset + 64) (validRecord v) (replace (offset+8) (half (validSkipped v)) (replace (offset+4) (word32le (stagingAddress g)) results))
              in continue (if latest == Nothing then statusFailed else 0) written caller next opened active current
          Append seed source | not valid -> same statusUnavailable
                             | otherwise ->
              let input = if source == CallerSource then CallerRecord (sourceBytes g) else StagingRecord
                  (status,next) = append hardware seed input state
              in continue status results caller next opened active current
          EraseAll | not valid -> same statusUnavailable
                   | otherwise -> continue 0 results caller (eraseAll state) opened active current
          Collect | not valid -> same statusUnavailable
                  | otherwise -> let (status,count,bytes,next) = collect caller state
                                 in continue status (replace (offset+8) (word32le count) results) bytes next opened active current
          ReadAt position lengthBytes | not opened -> continue statusUnavailable results (replace 0 (B.replicate lengthBytes 0) caller) state opened active current
                                      | isSequence c -> let (status,bytes,next) = readCachedAt position lengthBytes caller state in continue status (replace (offset + 64) (B.take lengthBytes bytes) results) bytes next opened active current
                                      | otherwise -> let (status,bytes) = readAt position lengthBytes caller state in continue status results bytes state opened active current

encodeRequest :: Request -> B.ByteString
encodeRequest request = case request of
  Erase (PoolBlock block) -> B.concat (map word32le [2,block,0,fromIntegral blockBytes]) <> B.replicate (requestStride-16) 0
  Program (PoolBlock block) offset payload -> B.concat (map word32le [1,block,fromIntegral offset,fromIntegral (B.length payload)]) <> payload <> B.replicate (requestStride-16-B.length payload) 0
encodeCallback :: CallbackEvent -> B.ByteString
encodeCallback (CallbackEvent role before after status) = word32le (if role == Check then 1 else 2) <> word32le status <> before <> after
expectedMedium :: OracleCase -> Expected -> B.ByteString
expectedMedium c expected
  | isSequence c = mediumBytes (medium (persistence (expectedStore expected)))
  | otherwise = B.concat [bankBytes (geometry (kind c)) b (medium (persistence (expectedStore expected))) | b <- [Bank0,Bank1]]
mediumBytes :: Medium -> B.ByteString
mediumBytes (Medium blocks) = B.concat [M.findWithDefault erased (PoolBlock block) blocks | block <- [0x52..0x57]]
