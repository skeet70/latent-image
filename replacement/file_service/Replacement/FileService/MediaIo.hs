{-# LANGUAGE OverloadedStrings #-}

-- | The storage-worker cases: an independent model of the stock capacity query 0x715a58,
-- pattern test 0x715de4, device query 0x71ded4, page read 0x71e828 and write 0x71e8c4 and
-- file read 0x71edbc, read from their bytes, with the retained pattern pass 0x715b34 run as
-- stock code. It gives each case's result, the ordered calls into the retained volume, page,
-- filesystem and local-source services, and the final payload, result slot, page-state word,
-- media-state context and destination buffer. The pass cases enter the page workers through
-- the pass's `bl` at 0x715c30 and 0x715cd4.
module Replacement.FileService.MediaIo
  ( code
  , scenarios
  ) where

import Data.Bits (shiftL, xor, (.&.), (.|.))
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import Data.Word (Word32)
import Replacement.FileService.Native
import Replacement.FileService.Requests (translation)

code :: Code
code = Code
  { codeImageBase = 0x61c400
  , codeExtents =
      [ ("capacity-code", Extent 0x715a58 0x715b34)
      , ("pattern-code", Extent 0x715de4 0x715f0c)
      , ("done-code", Extent 0x71c634 0x71c648)
      , ("translate-code", Extent 0x71d450 0x71d66c)
      , ("query-code", Extent 0x71ded4 0x71df20)
      , ("pages-code", Extent 0x71e828 0x71e960)
      , ("lookup-code", Extent 0x71eb28 0x71eba0)
      , ("read-code", Extent 0x71edbc 0x71ef18)
      ]
  , codeStockFunctions =
      [ (0x715a58, 0xdc), (0x715de4, 0x128), (0x71c634, 0x14), (0x71d450, 0x21c), (0x71ded4, 0x4c)
      , (0x71e828, 0x9c), (0x71e8c4, 0x9c), (0x71eb28, 0x78), (0x71edbc, 0x15c)
      ]
  , codeRetained = [("pattern-pass", Extent 0x715b34 0x715de4)]
  , codeRetainedFunctions = [(0x715b34, 0x2b0)]
  }

-- Addresses ------------------------------------------------------------------

capacityWorker, patternWorker, queryWorker, readPagesWorker, writePagesWorker, readWorker, patternPass :: Word32
capacityWorker = 0x715a58
patternWorker = 0x715de4
queryWorker = 0x71ded4
readPagesWorker = 0x71e828
writePagesWorker = 0x71e8c4
readWorker = 0x71edbc
patternPass = 0x715b34

semaphoreSignal, mediaReport, volumeGeometry, volumeFlag, volumeUnits, queryPath :: Word32
semaphoreSignal = 0x724c00
mediaReport = 0x726c74
volumeGeometry = 0x701090
volumeFlag = 0x7012a0
volumeUnits = 0x7012c0
queryPath = 0x701e18

readPages, writePages, fsRead, localRecord, localSize, localRead, localPosition :: Word32
readPages = 0x700cd8
writePages = 0x700d20
fsRead = 0x6feeec
localRecord = 0x7111c4
localSize = 0x6aa4ec
localRead = 0x6aa52c
localPosition = 0x7111e0

callbackStub, stateBase, resultSlot, contextBase, requestBase, payloadBase, sectorModeBase, destinationBase, workerTable :: Word32
callbackStub = 0x380000
stateBase = 0x910134
resultSlot = 0x910138
contextBase = 0xa00000
requestBase = 0xc10000
payloadBase = requestBase + 0x10
sectorModeBase = 0x9738c4
destinationBase = 0xd00000
workerTable = 0x814740

contextBytes, payloadBytes, destinationBytes, passBuffer, capacityAt :: Int
contextBytes = 0x17480
payloadBytes = 0x40
destinationBytes = 0x40
passBuffer = 0x16c74
capacityAt = 0x16c50

-- Model ------------------------------------------------------------------------

-- | The stock state the workers and the pass read and write.
data M = M
  { mPayload :: B.ByteString
  , mSlot :: (Word32, Word32)
  , mPageState :: Word32
  , mContext :: B.ByteString
  , mDestination :: B.ByteString
  , mTransfers :: Int
  , mServices :: [Service]
  }

-- | Scripted answers of the retained services and the initial context.
data Env = Env
  { envContext :: Bool
  , envDevice :: Word32
  , envCapacity :: Word32
  , envSectorMode :: Word32
  , envTransferRaw :: [(Int, Word32)]
  , envCorrupt :: Maybe Int
  , envGeometry :: (Word32, Word32)
  , envFlag :: (Word32, Word32)
  , envUnits :: Word32
  , envQuery :: Word32
  , envPairs :: [(Word32, Word32)]
  , envRaw :: Word32
  , envCount :: Word32
  , envLocal :: [Word32]
  , envLocalSize :: (Word32, Word32)
  , envLocalRead :: Word32
  }

env :: Env
env = Env
  { envContext = True
  , envDevice = 7
  , envCapacity = 0x14
  , envSectorMode = 0
  , envTransferRaw = []
  , envCorrupt = Nothing
  , envGeometry = (0x3b9ac000, 0)
  , envFlag = (0, 0)
  , envUnits = 0
  , envQuery = 0
  , envPairs = [(3, 0x41), (6, 0x42)] ++ replicate 14 (0, 0)
  , envRaw = 0
  , envCount = 0
  , envLocal = [0, 0, 0, 0]
  , envLocalSize = (0, 0x1000)
  , envLocalRead = 0
  }

newtype S a = S {runS :: Env -> B.ByteString -> M -> (a, M)}

instance Functor S where
  fmap f (S g) = S (\e i m -> let (a, m') = g e i m in (f a, m'))

instance Applicative S where
  pure a = S (\_ _ m -> (a, m))
  S f <*> S g = S (\e i m -> let (h, m') = f e i m; (a, m'') = g e i m' in (h a, m''))

instance Monad S where
  S g >>= k = S (\e i m -> let (a, m') = g e i m in runS (k a) e i m')

ask :: S Env
ask = S (\e _ m -> (e, m))

image :: S B.ByteString
image = S (\_ i m -> (i, m))

gets :: (M -> a) -> S a
gets f = S (\_ _ m -> (f m, m))

modify :: (M -> M) -> S ()
modify f = S (\_ _ m -> ((), f m))

made :: Service -> S ()
made service = modify (\m -> m {mServices = mServices m ++ [service]})

call :: Word32 -> [(String, Word32)] -> Word32 -> Service
call address inputs result = Service address inputs [("r0", result)] [] []

wordAt :: B.ByteString -> Int -> Word32
wordAt bytes offset = sum [fromIntegral (B.index bytes (offset + index)) `shiftL` (8 * index) | index <- [0 .. 3]]

put :: Int -> B.ByteString -> B.ByteString -> B.ByteString
put offset value bytes = B.take offset bytes <> value <> B.drop (offset + B.length value) bytes

word :: Int -> S Word32
word offset = gets (\m -> wordAt (mPayload m) offset)

byte :: Int -> S Word32
byte offset = gets (\m -> fromIntegral (B.index (mPayload m) offset))

setWord :: Int -> Word32 -> S ()
setWord offset value = modify (\m -> m {mPayload = put offset (word32le value) (mPayload m)})

inside :: Word32 -> Int -> Word32 -> Bool
inside base size address = base <= address && address < base + fromIntegral size

-- | Guest bytes at an address in the context or the destination buffer.
readMemory :: Word32 -> Int -> S B.ByteString
readMemory address count
  | inside contextBase contextBytes address = gets (B.take count . B.drop (fromIntegral (address - contextBase)) . mContext)
  | inside destinationBase destinationBytes address = gets (B.take count . B.drop (fromIntegral (address - destinationBase)) . mDestination)
  | otherwise = error "a case reads outside its regions"

writeMemory :: Word32 -> B.ByteString -> S ()
writeMemory address bytes
  | inside contextBase contextBytes address = modify (\m -> m {mContext = put (fromIntegral (address - contextBase)) bytes (mContext m)})
  | inside destinationBase destinationBytes address = modify (\m -> m {mDestination = put (fromIntegral (address - destinationBase)) bytes (mDestination m)})
  | otherwise = error "a case writes outside its regions"

-- | 0x71d450 through its jump table, with the media report the case block makes.
translate :: Word32 -> S Word32
translate raw = do
  bytes <- image
  let (status, reported) = translation bytes raw
  mapM_ (\value -> made (call mediaReport [("r0", 2), ("r1", value)] 0)) reported
  pure status

callback2 :: Word32 -> Word32 -> Word32 -> S ()
callback2 0 _ _ = pure ()
callback2 function status value = made (call function [("r0", status), ("r1", value)] 0)

callback1 :: Word32 -> Word32 -> S ()
callback1 0 _ = pure ()
callback1 function status = made (call function [("r0", status)] 0)

store :: Word32 -> Word32 -> S ()
store 0 _ = pure ()
store address value
  | address == resultSlot = modify (\m -> m {mSlot = (value, snd (mSlot m))})
  | address == resultSlot + 4 = modify (\m -> m {mSlot = (fst (mSlot m), value)})
  | otherwise = error "a case stores outside the result slot"

-- | 0x71eb28: the index of the first pair holding a nonzero tag.
lookupTag :: Word32 -> S Word32
lookupTag 0 = pure 0
lookupTag tag = do
  pairs <- envPairs <$> ask
  pure (case filter ((== tag) . snd) pairs of
    (index, _) : _ -> index
    [] -> 0)

-- | Bytes a scripted service writes, distinct per offset.
filler :: Word32 -> Int -> B.ByteString
filler seed count = B.pack [fromIntegral (seed + fromIntegral index * 7) | index <- [0 .. count - 1]]

-- Workers -------------------------------------------------------------------------

-- | 0x715a58: the callback receives the translated status and the size; it returns 0.
capacityW :: S Word32
capacityW = do
  callback <- word 0
  e <- ask
  let device = envDevice e
      (low, high) = envGeometry e
      (flagRaw, flag) = envFlag e
      record = put 0x18 (word32le high) (put 0x14 (word32le low) (filler 0x31 68))
  (raw, size) <-
    if not (envContext e)
      then pure (2, 0)
      else do
        made (Service volumeGeometry [("r0", device)] [] [] [AtRegister "r1" 0 record])
        made (Service volumeFlag [("r0", device)] [("r0", flagRaw)] [] [AtRegister "r1" 0 (word32le flag)])
        if flagRaw /= 0
          then pure (flagRaw, 0)
          else
            if flag /= 0
              then do
                made (call volumeUnits [("r0", device)] (envUnits e))
                pure (0, if envUnits e < 0x4000 then envUnits e `shiftL` 18 else 0xffffffff)
              else pure (0, if high == 0 then low else 0xffffffff)
  status <- translate raw
  callback2 callback status size
  pure 0

-- | 0x715de4: the payload's repeat count of six passes; the first nonzero pass status ends it.
patternW :: S Word32
patternW = do
  callback <- word 0
  repeats <- word 8
  present <- envContext <$> ask
  raw <- if present then repeatPasses repeats else pure 5
  status <- translate raw
  callback1 callback status
  pure status
  where
    repeatPasses 0 = pure 0
    repeatPasses n = do
      status <- sixPasses [(0x55, 1), (0xaa, 1), (0xffff, 1), (0x55, 256), (0xaa, 256), (0xffff, 256)]
      if status /= 0 then pure status else repeatPasses (n - 1)
    sixPasses [] = pure 0
    sixPasses ((pattern, step) : rest) = do
      status <- pass pattern step
      if status /= 0 then pure status else sixPasses rest

-- | 0x715b34: fills the sector buffer at context + 0x16c74, writes it to sectors 0, step,
-- 2 step, ... below the capacity, then reads each back and compares; a mismatch returns 8.
pass :: Word32 -> Word32 -> S Word32
pass pattern step = do
  e <- ask
  let size = if envSectorMode e == 5 then 2048 else 512
      expected
        | pattern == 0xffff = B.pack [fromIntegral index | index <- [0 .. size - 1 :: Int]]
        | otherwise = B.replicate size (fromIntegral pattern)
      buffer = contextBase + fromIntegral passBuffer
      cap = envCapacity e
      prepare sector = do
        setWord 4 sector
        setWord 8 1
        setWord 0xc buffer
        setWord 0 0
      writeOut sector = do
        prepare sector
        status <- pagesW Write
        let next = sector + step
        if cap > next && status == 0 then writeOut next else pure status
      readBack sector = do
        prepare sector
        status <- pagesW Read
        same <- (== expected) <$> readMemory buffer size
        if status == 0 && not same
          then pure 8
          else do
            let next = sector + step
            if cap > next && status == 0 then readBack next else pure status
  writeMemory buffer expected
  if cap == 0
    then pure 0
    else do
      written <- writeOut 0
      if written /= 0 then pure written else readBack 0

data Direction = Read | Write
  deriving (Eq)

-- | 0x71e828 and 0x71e8c4: a range past the capacity or of no sectors refuses with 19; both
-- set 0x910208 to all ones.
pagesW :: Direction -> S Word32
pagesW direction = do
  callback <- word 0
  e <- ask
  raw <-
    if not (envContext e)
      then pure 2
      else do
        first <- word 4
        count <- word 8
        if first + count > envCapacity e || count == 0
          then pure 19
          else do
            buffer <- word 0xc
            ordinal <- gets mTransfers
            modify (\m -> m {mTransfers = ordinal + 1})
            let raw = maybe 0 id (lookup ordinal (envTransferRaw e))
                inputs = [("r0", envDevice e), ("r1", first), ("r2", count), ("r3", buffer)]
                size
                  | inside contextBase contextBytes buffer = if envSectorMode e == 5 then 2048 else 512
                  | otherwise = 32
            current <- readMemory buffer size
            case direction of
              Write -> made (Service writePages inputs [("r0", raw)] [AtRegister "r3" 0 current] [])
              Read -> do
                let back
                      | inside contextBase contextBytes buffer =
                          if envCorrupt e == Just ordinal then put 0x10 (B.singleton (B.index current 0x10 `xor` 0xff)) current else current
                      | otherwise = filler 0x80 size
                made (Service readPages inputs [("r0", raw)] [] [AtRegister "r3" 0 back])
                writeMemory buffer back
            pure raw
  status <- translate raw
  modify (\m -> m {mPageState = 0xffffffff})
  callback1 callback status
  pure status

-- | 0x71ded4: 0x701e18 over the payload path and flag byte; any failure is 11.
queryW :: S Word32
queryW = do
  callback <- word 0
  flag <- byte 0x24
  path <- gets (B.take 0x20 . B.drop 4 . mPayload)
  result <- envQuery <$> ask
  made (Service queryPath [("r0", payloadBase + 4), ("r2", flag)] [("r0", result)] [AtRegister "r0" 0 path] [AtRegister "r1" 0 (filler 0x51 68)])
  let status = if result /= 0 then 11 else 0
  callback1 callback status
  pure status

-- | 0x71edbc. The local-mode redirect delivers its untranslated status and the length to the
-- callback and signals nobody.
readW :: S Word32
readW = do
  index <- word 4 >>= lookupTag
  setWord 4 index
  callback <- word 0
  e <- ask
  made (Service localRecord [] [] [] [AtRegister "r0" 0 (words32le (envLocal e))])
  case envLocal e of
    [1, handle, cursor, position] -> do
      buffer <- word 0xc
      len <- word 8
      let (sizeStatus, size) = envLocalSize e
      made (Service localSize [("r0", handle)] [("r0", sizeStatus)] [] [AtRegister "r1" 0 (words32le [0x40, size])])
      (status, delivered) <-
        if sizeStatus /= 0
          then pure (5, len)
          else do
            let destination = if buffer .&. 1 /= 0 then buffer + 1 else buffer
                clipped = if position + len > size then size - position else len
                answer = envLocalRead e
            made
              ( Service localRead [("r0", handle), ("r1", cursor), ("r2", destination), ("r3", position)] [("r0", answer)]
                  [AtRegister "sp" 0 (word32le clipped)] [AtRegister "r2" 0 (filler 0xa0 16)]
              )
            writeMemory destination (filler 0xa0 16)
            if answer /= 0
              then pure (5, clipped)
              else do
                made (call localPosition [("r0", clipped + position)] 0)
                pure (0, clipped)
      callback2 callback status delivered
      pure status
    _ -> do
      (raw, count) <-
        if not (envContext e)
          then pure (2, 0)
          else do
            buffer <- word 0xc
            len <- word 8
            made (Service fsRead [("r0", index), ("r1", buffer), ("r2", len)] [("r0", envCount e)] [] [AtRegister "r3" 0 (word32le (envRaw e)), AtRegister "r1" 0 (filler 0xc0 16)])
            writeMemory buffer (filler 0xc0 16)
            pure (envRaw e, envCount e)
      status <- translate raw
      callback2 callback status count
      flag <- word 0x10
      if flag == 0
        then pure ()
        else do
          word 0x14 >>= \at -> store at status
          word 0x18 >>= \at -> store at count
          made (call semaphoreSignal [("r0", 0x3b)] 0)
      pure status

-- Cases ------------------------------------------------------------------------------

stateRegion :: Bool -> (Word32, Word32) -> [(Word32, Word32)] -> Word32 -> B.ByteString
stateRegion present (status, value) pairs pageState =
  words32le ([0x31, status, value] ++ concat [[index, tag] | (index, tag) <- pairs] ++ replicate 16 0 ++ [0x70, if present then contextBase else 0, pageState])

contextRegion :: Env -> B.ByteString
contextRegion e = put capacityAt (word32le (envCapacity e)) (put 0x114 (word32le (envDevice e)) (B.replicate contextBytes 0xee))

requestRegion :: Word32 -> B.ByteString -> B.ByteString
requestRegion requestCode payload = words32le [requestCode .|. 0x65 `shiftL` 16 .|. 0x1c `shiftL` 24, fromIntegral (B.length payload), payloadBase, 0] <> payload

-- | A case entered at `entry` with the request header in r0 and `rest` in r1..r3.
storageCase :: B.ByteString -> String -> Word32 -> Word32 -> [Word32] -> S Word32 -> Env -> [(Int, Word32)] -> [(Int, B.ByteString)] -> Scenario
storageCase bytes name requestCode entry rest model e words' strings =
  Scenario
    { scenarioName = name
    , scenarioEntry = entry
    , scenarioData =
        [ DataRegion "storage-state" stateBase (stateRegion (envContext e) initialSlot (envPairs e) initialPageState) (stateRegion (envContext e) (mSlot final) (envPairs e) (mPageState final))
        , DataRegion "request" requestBase (requestRegion requestCode payload) (requestRegion requestCode (mPayload final))
        , DataRegion "sector-mode" sectorModeBase (words32le [0, envSectorMode e]) (words32le [0, envSectorMode e])
        , DataRegion "destination" destinationBase initialDestination (mDestination final)
        ]
          ++ [DataRegion "context" contextBase (contextRegion e) (mContext final) | envContext e]
    , scenarioCalls = [Call entry (requestBase : rest) [] True]
    , scenarioServices = mServices final
    , scenarioResults = [result]
    , scenarioStop = Nothing
    }
  where
    initialSlot = (0x5a5a5a5a, 0x6b6b6b6b)
    initialPageState = 0x12345678
    initialDestination = B.replicate destinationBytes 0x33
    payload = foldl (\b (offset, value) -> put offset value b) (B.replicate payloadBytes 0)
      ([(offset, word32le value) | (offset, value) <- words'] ++ strings)
    start = M payload initialSlot initialPageState (contextRegion e) initialDestination 0 []
    (result, final) = runS model e bytes start

scenarios :: B.ByteString -> [Scenario]
scenarios bytes =
  [ capacity "geometry" env [(0, callbackStub)]
  , capacity "geometry-large" env {envGeometry = (0x1000, 1)} [(0, callbackStub)]
  , capacity "units" env {envFlag = (0, 1), envUnits = 0x3a0} [(0, callbackStub)]
  , capacity "units-silent" env {envFlag = (0, 1), envUnits = 0x3a0} []
  , capacity "units-overflow" env {envFlag = (0, 1), envUnits = 0x4000} [(0, callbackStub)]
  , capacity "flag-failed" env {envFlag = (0x102, 1)} [(0, callbackStub)]
  , capacity "no-context" env {envContext = False} [(0, callbackStub)]
  , capacity "silent" env []
  , patternCase "one-repeat" env {envCapacity = 1} [(0, callbackStub), (8, 1)]
  , patternCase "two-repeats" env {envCapacity = 1} [(0, callbackStub), (8, 2)]
  , patternCase "mismatch" env {envCapacity = 1, envCorrupt = Just 3} [(0, callbackStub), (8, 2)]
  , patternCase "write-failed" env {envCapacity = 1, envTransferRaw = [(0, 13)]} [(0, callbackStub), (8, 1)]
  , patternCase "read-failed" env {envCapacity = 1, envTransferRaw = [(5, 9)]} [(0, callbackStub), (8, 1)]
  , patternCase "no-repeats" env [(0, callbackStub)]
  , patternCase "no-context" env {envContext = False} [(0, callbackStub), (8, 1)]
  , patternCase "capacity-zero" env {envCapacity = 0} [(0, callbackStub), (8, 1)]
  , passCase "fill-step-one" env {envCapacity = 3} 0x55 1
  , passCase "sequence-step-two" env {envCapacity = 3} 0xffff 2
  , passCase "large-sectors" env {envCapacity = 2, envSectorMode = 5} 0xffff 1
  , passCase "write-refused" env {envCapacity = 3, envTransferRaw = [(1, 3)]} 0xaa 1
  , passCase "mismatch" env {envCapacity = 2, envCorrupt = Just 3} 0x55 1
  , query "found" env [(0, callbackStub)] 1
  , query "failed" env {envQuery = 4} [(0, callbackStub)] 0
  , query "silent" env {envQuery = 14} [] 0
  , pages "read" Read env (pagesRequest 0x10 4)
  , pages "read-failed" Read env {envTransferRaw = [(0, 9)]} (pagesRequest 0x10 4)
  , pages "read-past-capacity" Read env (pagesRequest 0x12 4)
  , pages "read-no-sectors" Read env (pagesRequest 0 0)
  , pages "read-no-context" Read env {envContext = False} (pagesRequest 0x10 4)
  , pages "write" Write env (pagesRequest 0x10 4)
  , pages "write-failed" Write env {envTransferRaw = [(0, 39)]} (pagesRequest 0 1)
  , pages "write-wrapped-range" Write env (pagesRequest 0xffffffff 2)
  , pages "write-silent" Write env (pagesRequest 0 1 ++ [(0, 0)])
  , readCase "blocking" env {envCount = 0x180} (readRequest True)
  , readCase "posted-failed" env {envRaw = 37} (readRequest False)
  , readCase "unknown-handle" env {envRaw = 15} (readRequest True ++ [(4, 0x99)])
  , readCase "no-context" env {envContext = False} (readRequest True)
  , readCase "blocking-no-pointers" env {envCount = 0x10} (readRequest False ++ [(0x10, 1)])
  , readCase "local" env {envLocal = [1, 2, 9, 0x100]} (readRequest True)
  , readCase "local-clipped" env {envLocal = [1, 2, 9, 0xf00]} (readRequest False)
  , readCase "local-size-refused" env {envLocal = [1, 2, 9, 0], envLocalSize = (0xffffffe2, 0)} (readRequest True)
  , readCase "local-read-refused" env {envLocal = [1, 2, 9, 0xf00], envLocalRead = 0xffffffe2} (readRequest False)
  , readCase "local-even-buffer" env {envLocal = [1, 0, 4, 0]} (readRequest False ++ [(0xc, destinationBase + 4)])
  ]
  where
    worker name requestCode entry model e request strings = storageCase bytes name requestCode entry [workerTable, requestCode, entry] model e request strings
    capacity name e request = worker ("capacity-" ++ name) 0x2c26 capacityWorker capacityW e request []
    patternCase name e request = worker ("pattern-" ++ name) 0x2c2a patternWorker patternW e request []
    passCase name e fill step = storageCase bytes ("pass-" ++ name) 0x2c2a patternPass [fill, step] (pass fill step) e [(0, callbackStub)] []
    query name e request flag = worker ("query-" ++ name) 0x2c1e queryWorker queryW e (request ++ [(0x24, flag)]) [(4, C.pack "B:/DCIM/100_FUJI/DSCF3097.JPG")]
    pages name Read e request = worker ("pages-" ++ name) 0x2c13 readPagesWorker (pagesW Read) e request []
    pages name Write e request = worker ("pages-" ++ name) 0x2c14 writePagesWorker (pagesW Write) e request []
    readCase name e request = worker ("read-" ++ name) 0x2c18 readWorker readW e request []
    pagesRequest first count = [(0, callbackStub), (4, first), (8, count), (0xc, destinationBase)]
    readRequest blocked =
      [(0, callbackStub), (4, 0x41), (8, 0x200), (0xc, destinationBase + 1)] ++ (if blocked then [(0x10, 1), (0x14, resultSlot), (0x18, resultSlot + 4)] else [])
