{-# LANGUAGE GHC2021 #-}
module Replacement.RecordStore.Model where

import Data.Bits ((.&.))
import qualified Data.ByteString as B
import qualified Data.Map.Strict as M
import Data.Word

-- RSC geometry table and 0x6ad9a8..0x6ada08.
data Kind = Kind0 | Kind1 | Kind2 | Kind3 | Kind4 deriving (Eq, Ord, Show, Enum, Bounded)
data Bank = Bank0 | Bank1 deriving (Eq, Ord, Show, Enum, Bounded)
newtype PoolBlock = PoolBlock Word32 deriving (Eq, Ord, Show)
newtype Sequence = Sequence Word16 deriving (Eq, Ord, Show)
newtype Seed = Seed Word16 deriving (Eq, Show)
newtype Position = Position Int deriving (Eq, Show)
newtype BytePosition = BytePosition Word32 deriving (Eq, Show)
newtype PersistenceStep = PersistenceStep Int deriving (Eq, Show)
newtype ReadOccurrence = ReadOccurrence Word32 deriving (Eq, Show)
data ReadFault = ReadFault PoolBlock ReadOccurrence deriving (Eq, Show)
data Geometry = Geometry { recordBytes :: Int, recordCount :: Int, programBytes :: Int, stagingAddress :: Word32, firstBlock :: PoolBlock } deriving (Eq, Show)
geometry :: Kind -> Geometry
geometry kind = case kind of
  Kind0 -> Geometry 0x60 254 0x60 0x8e16a0 (PoolBlock 0x54)
  Kind1 -> Geometry 0x1000 16 0x1000 0x8e00a0 (PoolBlock 0x52)
  Kind2 -> Geometry 0x200 31 0x200 0x8e1700 (PoolBlock 0x56)
  Kind3 -> Geometry 0x34 127 0x600 0x89fa80 (PoolBlock 0x58)
  Kind4 -> Geometry 0x600 10 0x600 0x8e10a0 (PoolBlock 0x68)
blockBytes :: Int
blockBytes = 0x20000
nextBank :: Bank -> Bank
nextBank Bank0 = Bank1
nextBank Bank1 = Bank0
poolBlock :: Geometry -> Bank -> PoolBlock
poolBlock g bank = let PoolBlock first = firstBlock g in PoolBlock (first + fromIntegral (fromEnum bank))
newtype Medium = Medium (M.Map PoolBlock B.ByteString) deriving (Eq, Show)
data Request = Program PoolBlock Int B.ByteString | Erase PoolBlock deriving (Eq, Show)
data Fault = NoFault | FailRequest PersistenceStep | StopAfter PersistenceStep | FailRead ReadFault deriving (Eq, Show)
data Persistence = Persistence { medium :: Medium, requests :: [Request], fault :: Fault, steps :: Int, stopped :: Bool } deriving (Eq, Show)
erased :: B.ByteString
erased = B.replicate blockBytes 0xff
bankBytes :: Geometry -> Bank -> Medium -> B.ByteString
bankBytes g b (Medium m) = M.findWithDefault erased (poolBlock g b) m
putBank :: Geometry -> Bank -> B.ByteString -> Medium -> Medium
putBank g b bytes (Medium m) = Medium (M.insert (poolBlock g b) bytes m)
replace :: Int -> B.ByteString -> B.ByteString -> B.ByteString
replace offset value bytes = B.take offset bytes <> value <> B.drop (offset + B.length value) bytes
half :: Word16 -> B.ByteString
half x = B.pack [fromIntegral x, fromIntegral (x `div` 256)]
sequenceOf :: B.ByteString -> Sequence
sequenceOf bytes = Sequence (fromIntegral (B.index bytes 0) + 256 * fromIntegral (B.index bytes 1))
successor :: Sequence -> Sequence
successor (Sequence x) = Sequence ((x + 1) .&. 0x7fff)
recordAt :: Geometry -> Bank -> Position -> Medium -> B.ByteString
recordAt g b (Position p) = B.take (recordBytes g) . B.drop (p * recordBytes g) . bankBytes g b
persist :: Request -> Persistence -> Persistence
persist request state
  | stopped state = state
  | fault state == StopAfter (PersistenceStep (steps state)) = state { stopped = True }
  | otherwise = state { medium = landed, requests = requests state ++ [request], steps = steps state + 1 }
  where
    landed | fault state == FailRequest (PersistenceStep (steps state)) = medium state
           | otherwise = case (request, medium state) of
               (Erase block, Medium m) -> Medium (M.insert block erased m)
               (Program block offset payload, Medium m) ->
                 let old = M.findWithDefault erased block m
                     bytes = B.pack (B.zipWith (.&.) payload (B.take (B.length payload) (B.drop offset old)))
                 in Medium (M.insert block (replace offset bytes old) m)

data Callback = NoCallback | MarkerCallback | CorrectingCallback | Hardware2 | Hardware3 deriving (Eq, Show)
data CallbackRole = Check | Prepare deriving (Eq, Show)
data CallbackEvent = CallbackEvent CallbackRole B.ByteString B.ByteString Word32 deriving (Eq, Show)
data Verdict = Accept | Reject deriving (Eq, Show)
type HardwareCheck = CallbackRole -> Callback -> B.ByteString -> (Verdict, B.ByteString)
statusUnavailable, statusInvalid, statusFailed :: Word32
statusUnavailable = 0xfffffff2
statusInvalid = 0xffffffe2
statusFailed = 0xffffff01
data CacheSlot = CacheSlot { cacheBlock :: Maybe PoolBlock, cacheAge :: Word32, cacheBytes :: B.ByteString } deriving (Eq, Show)
data CachePair = CachePair CacheSlot CacheSlot deriving (Eq, Show)
data CacheIndex = CacheZero | CacheOne deriving (Eq, Show)
data Store = Store { storeKind :: Kind, currentBank :: Bank, staging :: B.ByteString, persistence :: Persistence, callback :: Callback, callbackEvents :: [CallbackEvent], cacheSlots :: CachePair, cacheTimestamp :: Word32, readOccurrences :: M.Map PoolBlock Word32 } deriving (Eq, Show)
initialStore :: Kind -> Medium -> Fault -> Callback -> Store
initialStore kind bytes injection cb = Store kind Bank0 (B.replicate (programBytes (geometry kind)) 0xff) (Persistence bytes [] injection 0 False) cb [] (CachePair emptySlot emptySlot) 0 M.empty
  where emptySlot = CacheSlot Nothing 0 (B.replicate blockBytes 0)
cachedBank :: Bank -> Store -> (B.ByteString, Store)
cachedBank bank state = (cacheBytes selected, state {cacheSlots = updated, cacheTimestamp = tick, readOccurrences = occurrences})
  where
    block = poolBlock (geometry (storeKind state)) bank
    CachePair slot0 slot1 = cacheSlots state
    index
      | cacheBlock slot1 == Just block = CacheOne
      | cacheBlock slot0 == Just block = CacheZero
      | cacheBlock slot0 == Nothing = CacheZero
      | cacheBlock slot1 == Nothing = CacheOne
      | cacheAge slot0 <= cacheAge slot1 = CacheZero
      | otherwise = CacheOne
    old = case index of CacheZero -> slot0; CacheOne -> slot1
    hit = cacheBlock old == Just block
    occurrence = M.findWithDefault 0 block (readOccurrences state) + 1
    occurrences = if hit then readOccurrences state else M.insert block occurrence (readOccurrences state)
    failed = fault (persistence state) == FailRead (ReadFault block (ReadOccurrence occurrence))
    bytes = if hit || failed then cacheBytes old else bankBytes (geometry (storeKind state)) bank (medium (persistence state))
    tick = cacheTimestamp state + 1
    selected = CacheSlot (Just block) (cacheTimestamp state) bytes
    updated = case index of CacheZero -> CachePair selected slot1; CacheOne -> CachePair slot0 selected
seek :: Bank -> Position -> Store -> Store
seek bank position@(Position p) state
  | not (isReadFault (fault (persistence state))) = state { staging = replace 0 (recordAt g bank position (medium (persistence state))) (staging state) }
  | otherwise = next { staging = replace 0 (B.take (recordBytes g) (B.drop (p * recordBytes g) bytes)) (staging state) }
  where
    g = geometry (storeKind state)
    (bytes, next) = cachedBank bank state
isReadFault :: Fault -> Bool
isReadFault (FailRead _) = True
isReadFault _ = False
-- RSC 0x6acfa8..0x6ad050 searches occupancy, including the final midpoint.
findLast :: Bank -> Store -> (Maybe Position, Store)
findLast bank state = finish (search 0 (recordCount g - 1) state)
  where
    g = geometry (storeKind state)
    search lo hi s =
      let p = (lo + hi) `quot` 2
          s' = seek bank (Position p) s
          occupied = sequenceOf (staging s') /= Sequence 0xffff
          lo' = if occupied then p + 1 else lo
          hi' = if occupied then hi else p - 1
      in if lo < hi then search lo' hi' s' else (p, occupied, s')
    finish (p, occupied, s)
      | occupied = (Just (Position p), s)
      | p > 0 = (Just (Position (p - 1)), seek bank (Position (p - 1)) s)
      | otherwise = (Nothing, s)
checkRecord :: HardwareCheck -> Store -> (Verdict, Store)
checkRecord hardware state
  | callback state == NoCallback = (Accept, state)
  | otherwise = (verdict, state { staging = replace 0 after (staging state), callbackEvents = callbackEvents state ++ [CallbackEvent Check before after result] })
  where
    size = recordBytes (geometry (storeKind state))
    before = B.take size (staging state)
    (verdict, after) = case callback state of
      MarkerCallback -> (if B.index before 2 == 0xee then Reject else Accept, before)
      CorrectingCallback -> (Accept, replace 3 (B.singleton 0x42) before)
      Hardware2 -> hardware Check Hardware2 before
      Hardware3 -> hardware Check Hardware3 before
      NoCallback -> (Accept, before)
    result = if verdict == Accept then 0 else 1
prepareRecord :: HardwareCheck -> Store -> Store
prepareRecord hardware state
  | callback state == NoCallback = state
  | otherwise = state { staging = replace 0 after (staging state), callbackEvents = callbackEvents state ++ [CallbackEvent Prepare before after 0] }
  where
    before = B.take (recordBytes (geometry (storeKind state))) (staging state)
    after = case callback state of
      Hardware2 -> snd (hardware Prepare Hardware2 before)
      Hardware3 -> snd (hardware Prepare Hardware3 before)
      _ -> replace 4 (B.singleton 0x5a) before

data Valid = Valid { validBank :: Bank, validPosition :: Position, validSkipped :: Word16, validRecord :: B.ByteString } deriving (Eq, Show)
findValid :: HardwareCheck -> Bank -> Store -> (Maybe Valid, Store, Word16)
findValid hardware bank state = case findLast bank state of
  (Nothing, s) -> (Nothing, s, 0)
  (Just position, s) -> step position 0 s
  where
    step position@(Position p) skipped s =
      let (verdict, s') = checkRecord hardware s
      in case verdict of
        Accept -> (Just (Valid bank position skipped (B.take (recordBytes (geometry (storeKind s'))) (staging s'))), s', skipped)
        Reject | p == 0 -> (Nothing, s', skipped)
               | otherwise -> step (Position (p - 1)) (skipped + 1) (seek bank (Position (p - 1)) s')
data RecoveryCount = OmitRecoveryCount | WriteRecoveryCount deriving (Eq, Show)
-- RSC 0x6ac57c..0x6ac7bc, including the third visit and final re-search.
readLatest :: RecoveryCount -> HardwareCheck -> Store -> (Maybe Valid, Store)
readLatest recoveryCount hardware initial = finish (scan 0 Nothing initial)
  where
    g = geometry (storeKind initial)
    total = 2 * recordCount g
    window = 0x8000 - total
    newer candidate best =
      let Sequence c = sequenceOf (validRecord candidate)
          Sequence b = sequenceOf (validRecord best)
          ci = fromIntegral c :: Int
          bi = fromIntegral b :: Int
      in if ci <= bi then bi - ci >= total else ci - bi <= window
    scan :: Int -> Maybe Valid -> Store -> (Maybe Valid, Store)
    scan n best s =
      let (candidate, s', _) = findValid hardware (currentBank s) s
      in case candidate of
        Just c | maybe False (not . newer c) best -> (best, s')
               | n == 2 -> (Just c, s')
               | otherwise -> scan (n + 1) (Just c) (s' { currentBank = nextBank (currentBank s') })
        Nothing | n == 2 -> (best, s')
                | otherwise -> scan (n + 1) best (s' { currentBank = nextBank (currentBank s') })
    finish (Nothing, s) = (Nothing, s { currentBank = nextBank (currentBank initial) })
    finish (Just best, s) =
      let s' = if recoveryCount == WriteRecoveryCount && validPosition best == Position (recordCount g - 1)
               then let (_, checked, _) = findValid hardware (nextBank (validBank best)) s in checked
               else s
          (lastBest, final, _) = findValid hardware (validBank best) (s' { currentBank = validBank best })
      in (fmap (\v -> v { validSkipped = validSkipped best }) lastBest, final)

data AppendSource = CallerRecord B.ByteString | StagingRecord deriving (Eq, Show)
innerAppend :: HardwareCheck -> Seed -> AppendSource -> Store -> (Bool, Store)
innerAppend hardware (Seed seed) source state = case findLast bank state of
  (Just (Position p), s) | p >= recordCount g - 1 -> (False, s)
  (lastPosition, s) ->
    let (position, Sequence seqNo) = case lastPosition of
          Nothing -> (Position 0, Sequence seed)
          Just (Position previous) -> (Position (previous + 1), successor (sequenceOf (staging s)))
        sought = seek bank position s
        input = case source of CallerRecord bytes -> bytes; StagingRecord -> B.take (recordBytes g) (staging sought)
        copied = replace 0 (half seqNo <> B.drop 2 input) (staging sought)
        prepared = prepareRecord hardware (sought { staging = copied })
        Position p = position
        request = Program (poolBlock g bank) (p * recordBytes g) (B.take (programBytes g) (staging prepared))
    in (True, prepared { persistence = persist request (persistence prepared) })
  where g = geometry (storeKind state); bank = currentBank state
append :: HardwareCheck -> Seed -> AppendSource -> Store -> (Word32, Store)
append hardware seed source initial =
  let queried = currentBank initial
      (latest, readState) = readLatest OmitRecoveryCount hardware initial
      (nextSeed, destination) = case latest of
        Nothing -> (seed, Bank0)
        Just v -> let Sequence seqNo = successor (sequenceOf (validRecord v)) in (Seed seqNo, validBank v)
      (success, first) = if latest == Nothing then (False, readState) else innerAppend hardware nextSeed source readState
  in if success then (0, first) else
       let bank = if latest == Nothing then destination else nextBank queried
           (success2, second) = innerAppend hardware nextSeed source (first { currentBank = bank })
       in if success2 then (0, second) else
            let erasedState = second { persistence = persist (Erase (poolBlock (geometry (storeKind initial)) bank)) (persistence second) }
                (success3, third) = innerAppend hardware nextSeed source erasedState
            in (if success3 then 0 else statusFailed, third)
eraseAll :: Store -> Store
eraseAll state = state { persistence = foldl (flip persist) (persistence state) [Erase (poolBlock g b), Erase (poolBlock g (nextBank b))] }
  where g = geometry (storeKind state); b = currentBank state
collect :: B.ByteString -> Store -> (Word32, Word32, B.ByteString, Store)
collect caller state = (if B.null compacted then statusFailed else 0, fromIntegral (B.length compacted), output, final)
  where
    g = geometry (storeKind state)
    size = recordBytes g
    count = recordCount g
    chunk = size * count
    banks = [currentBank state, nextBank (currentBank state)]
    readChunk bank old = go 0 old
      where
        go p bytes | p == count = bytes
                   | sequenceOf record == Sequence 0xffff = foldl (\buf q -> replace (q * size) (half 0xffff) buf) bytes [p .. count - 1]
                   | otherwise = go (p + 1) (replace (p * size) record bytes)
          where record = recordAt g bank (Position p) (medium (persistence state))
    chunks = [readChunk bank (B.take chunk (B.drop (i * chunk) caller)) | (i, bank) <- zip [0..] banks]
    sorted = case chunks of [a,b] | sequenceOf b < sequenceOf a -> [b,a]; _ -> chunks
    allBytes = B.concat sorted
    records = [B.take size (B.drop (p * size) allBytes) | p <- [0 .. 2 * count - 1]]
    compacted = B.concat [r | r <- records, sequenceOf r /= Sequence 0xffff]
    output = replace 0 compacted (replace 0 allBytes caller)
    final = state
readAt :: BytePosition -> Int -> B.ByteString -> Store -> (Word32, B.ByteString)
readAt (BytePosition position) lengthBytes caller state
  | bank >= 2 = (statusInvalid, replace 0 (B.singleton 0) caller)
  | offset + lengthBytes > blockBytes = (statusInvalid, replace 0 (B.replicate lengthBytes 0) caller)
  | otherwise = (0, replace 0 (B.take lengthBytes (B.drop offset bytes)) caller)
  where
    bank = position `div` fromIntegral blockBytes
    offset = fromIntegral (position `mod` fromIntegral blockBytes)
    bytes = bankBytes (geometry (storeKind state)) (if bank == 0 then Bank0 else Bank1) (medium (persistence state))

readCachedAt :: BytePosition -> Int -> B.ByteString -> Store -> (Word32, B.ByteString, Store)
readCachedAt position@(BytePosition address) size caller state
  | address >= 0x40000 || fromIntegral (address `mod` 0x20000) + size > blockBytes =
      let (status, bytes) = readAt position size caller state in (status, bytes, state)
  | otherwise =
      let (bytes, next) = cachedBank (if address < 0x20000 then Bank0 else Bank1) state
      in (0, replace 0 (B.take size (B.drop (fromIntegral (address `mod` 0x20000)) bytes)) caller, next)
