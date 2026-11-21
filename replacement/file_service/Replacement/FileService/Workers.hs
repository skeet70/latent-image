{-# LANGUAGE OverloadedStrings #-}

-- | The handle-worker cases: an independent model of the stock open, write, seek, close,
-- close-with-entry-update and release workers, read from the bytes of [0x7115a4,0x7118fc),
-- [0x71eca4,0x71edbc) and [0x71f078,0x71f418), gives each case's result, the ordered calls
-- into the retained filesystem, path and media-state services, and the final payload, tag map
-- and media-state context.
module Replacement.FileService.Workers
  ( code
  , scenarios
  ) where

import Data.Bits (shiftL, (.&.), (.|.))
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import Data.List (findIndex)
import Data.Word (Word32)
import Replacement.FileService.Native
import Replacement.FileService.Requests (translation)

code :: Code
code = Code
  { codeImageBase = 0x61c400
  , codeExtents =
      [ ("worker-code", Extent 0x7115a4 0x7118fc)
      , ("done-code", Extent 0x71c634 0x71c648)
      , ("translate-code", Extent 0x71d450 0x71d66c)
      , ("map-code", Extent 0x71ea34 0x71eba0)
      , ("seek-code", Extent 0x71eca4 0x71edbc)
      , ("tag-code", Extent 0x71ef18 0x71f078)
      , ("close-code", Extent 0x71f078 0x71f418)
      ]
  , codeStockFunctions =
      [ (0x7115a4, 0x140), (0x7116e4, 0x84), (0x711768, 0xd8), (0x711840, 0xbc)
      , (0x71c634, 0x14), (0x71d450, 0x21c)
      , (0x71ea34, 0x78), (0x71eaac, 0x7c), (0x71eb28, 0x78)
      , (0x71eca4, 0x90), (0x71ed34, 0x88)
      , (0x71ef18, 0xdc), (0x71eff4, 0x84)
      , (0x71f078, 0xbc), (0x71f134, 0xa0), (0x71f1d4, 0xd4), (0x71f2a8, 0x170)
      ]
  , codeRetained = [("copy-memory", Extent 0x70e6c4 0x70e748)]
  , codeRetainedFunctions = [(0x70e6c4, 0x84)]
  }

-- Addresses ------------------------------------------------------------------

openWorker, writeWorker, closeWorker, closeEntryWorker, seekWorker, releaseWorker :: Word32
openWorker = 0x7115a4
writeWorker = 0x7116e4
closeWorker = 0x711768
closeEntryWorker = 0x711840
seekWorker = 0x71eca4
releaseWorker = 0x71f134

semaphoreWait, semaphoreSignal, mediaReport, cacheClaim, cacheRelease :: Word32
semaphoreWait = 0x724b68
semaphoreSignal = 0x724c00
mediaReport = 0x726c74
cacheClaim = 0x71e960
cacheRelease = 0x71e9ec

parsePath, directorySlot, splitPath, directoryNumber, makeDirectory, fsOpen, registerWritten :: Word32
parsePath = 0x7160b8
directorySlot = 0x715f0c
splitPath = 0x71543c
directoryNumber = 0x712dac
makeDirectory = 0x716184
fsOpen = 0x701634
registerWritten = 0x71565c

fsWrite, fsSeek, fsClose, fsCloseNamed, fsCloseEntry, fsRelease, mediaRefresh, afterClose, rescan :: Word32
fsWrite = 0x6ff000
fsSeek = 0x6ff138
fsClose = 0x6ffff4
fsCloseNamed = 0x6ffdcc
fsCloseEntry = 0x6ffb10
fsRelease = 0x6ffc68
mediaRefresh = 0x71d66c
afterClose = 0x71141c
rescan = 0x71985c

callbackStub, stateBase, resultSlot, contextBase, requestBase, payloadBase :: Word32
callbackStub = 0x380000
stateBase = 0x910134
resultSlot = 0x910138
contextBase = 0xa00000
requestBase = 0xc10000
payloadBase = requestBase + 0x10

contextBytes, payloadBytes :: Int
contextBytes = 0x3600
payloadBytes = 0x60

-- Model ------------------------------------------------------------------------

-- | The stock state a worker reads and writes: the payload, the result-slot words, the tag map
-- pairs, the tag slots and the media-state context's file records, flag word and directories.
data M = M
  { mPayload :: B.ByteString
  , mSlot :: (Word32, Word32)
  , mPairs :: [(Word32, Word32)]
  , mTags :: [Word32]
  , mModes :: [(Word32, Word32)]
  , mFlagged :: Word32
  , mServices :: [Service]
  }

-- | Scripted answers of the retained services and the initial context.
data Env = Env
  { envContext :: Bool
  , envModes :: [(Word32, Word32)]
  , envKinds :: [(Word32, Word32)]
  , envKnown :: [Word32]
  , envDevice :: Word32
  , envPairs :: [(Word32, Word32)]
  , envTags :: [Word32]
  , envClaim :: Word32
  , envRaw :: Word32
  , envCount :: Word32
  , envParse :: Word32
  , envSlotNumber :: Word32
  , envNumber :: Word32
  , envRescan :: Word32
  }

env :: Env
env = Env
  { envContext = True
  , envModes = []
  , envKinds = []
  , envKnown = []
  , envDevice = 7
  , envPairs = [(3, 0x41), (6, 0x42)] ++ replicate 14 (0, 0)
  , envTags = [0x41, 0x61, 0x42] ++ replicate 13 0
  , envClaim = 5
  , envRaw = 0
  , envCount = 0
  , envParse = 0
  , envSlotNumber = 2
  , envNumber = 0x10067
  , envRescan = 0x77
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

word :: Int -> S Word32
word offset = gets (\m -> wordAt (mPayload m) offset)

wordAt :: B.ByteString -> Int -> Word32
wordAt bytes offset = sum [fromIntegral (B.index bytes (offset + index)) `shiftL` (8 * index) | index <- [0 .. 3]]

byte :: Int -> S Word32
byte offset = gets (\m -> fromIntegral (B.index (mPayload m) offset))

setWord :: Int -> Word32 -> S ()
setWord offset value = modify (\m -> m {mPayload = B.take offset (mPayload m) <> word32le value <> B.drop (offset + 4) (mPayload m)})

replace :: Int -> a -> [a] -> [a]
replace index value values = take index values ++ [value] ++ drop (index + 1) values

context :: S Bool
context = envContext <$> ask

fileMode :: Word32 -> S Word32
fileMode index = gets (maybe 0 id . lookup index . mModes)

fileKind :: Word32 -> S Word32
fileKind index = maybe 0 id . lookup index . envKinds <$> ask

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

done :: S ()
done = made (call semaphoreSignal [("r0", 0x3b)] 0)

store :: Word32 -> Word32 -> S ()
store 0 _ = pure ()
store address value
  | address == resultSlot = modify (\m -> m {mSlot = (value, snd (mSlot m))})
  | address == resultSlot + 4 = modify (\m -> m {mSlot = (fst (mSlot m), value)})
  | otherwise = error "a case stores outside the result slot"

-- | 0x71eb28: the index of the first pair holding a nonzero tag.
lookupTag :: Word32 -> S Word32
lookupTag 0 = pure 0
lookupTag tag = gets (\m -> maybe 0 fst (lookupPair tag (mPairs m)))
  where
    lookupPair t pairs = case filter ((== t) . snd) pairs of
      pair : _ -> Just pair
      [] -> Nothing

-- | 0x71eaac.
unbind :: Word32 -> S ()
unbind 0 = pure ()
unbind tag = modify (\m -> m {mPairs = maybe (mPairs m) (\slot -> replace slot (0, 0) (mPairs m)) (findIndex ((== tag) . snd) (mPairs m))})

-- | 0x71ea34: claims an index and binds it in the first free pair.
bind :: Word32 -> S Word32
bind tag = do
  index <- envClaim <$> ask
  made (call cacheClaim [] index)
  if index == 0
    then pure 0
    else do
      modify (\m -> m {mPairs = maybe (mPairs m) (\slot -> replace slot (index, tag) (mPairs m)) (findIndex ((== 0) . snd) (mPairs m))})
      pure index

-- | 0x71eff4 with a granted wait.
releaseTag :: Word32 -> S ()
releaseTag 0 = pure ()
releaseTag tag = do
  made (call semaphoreWait [("r0", 0x3c), ("r1", 0xffffffff)] 0)
  modify (\m -> m {mTags = maybe (mTags m) (\slot -> replace slot 0 (mTags m)) (findIndex (== tag) (mTags m))})
  made (call semaphoreSignal [("r0", 0x3c)] 0)

-- Workers -------------------------------------------------------------------------

-- | 0x7115a4; it returns the status.
openW :: S Word32
openW = do
  callback <- word 0
  present <- context
  status <-
    if not present
      then pure 5
      else do
        setWord 0 0
        mode <- word 0x38
        if mode - 1 <= 2 then prepareDirectory else pure ()
        openFile
  word 0x34 >>= callback2 callback status
  blocking 0x40 0x44 (Just 0x48) status (word 0x34)
  pure status

prepareDirectory :: S ()
prepareDirectory = do
  e <- ask
  let path = payloadBase + 4
      directory = B.take 32 ("100_FUJI" <> B.replicate 32 0)
  made (Service parsePath [("r1", path)] [("r0", envParse e)] [] [AtRegister "r0" 0 (word32le 9)])
  if envParse e /= 0
    then pure ()
    else do
      made (call directorySlot [("r0", 9)] (envSlotNumber e))
      if envSlotNumber e == 0xff
        then pure ()
        else do
          made
            ( Service splitPath [("r0", path)] [("r0", 0)] [AtRegister "sp" 0 (word32le 0)]
                [AtRegister "r1" 0 directory, AtRegister "r3" 0 (word32le 4), AtIndirect "sp" 4 (word32le 0)]
            )
          made (Service directoryNumber [("r1", 4)] [("r0", envNumber e)] [AtRegister "r0" 0 directory] [])
          let number = envNumber e .&. 0xffff
          if (number - 100) .&. 0xffff >= 900 || number `elem` envKnown e
            then pure ()
            else made (call makeDirectory [("r0", number), ("r1", contextBase + 16 * envSlotNumber e + 0x21da)] 0)

-- | 0x71f2a8.
openFile :: S Word32
openFile = do
  tag <- word 0x3c
  setWord 0x34 tag
  bind tag >>= setWord 0x3c
  callback <- word 0
  name <- gets (B.take 48 . B.drop 4 . mPayload)
  e <- ask
  mode <- word 0x38
  index <- word 0x3c
  opened <- word 0x4c
  raw <- case lookup mode [(0, 0x72), (1, 0x77), (2, 0x61), (3, 0x73)] of
    Just character -> do
      made (Service fsOpen [("r1", character), ("r2", index), ("r3", opened)] [("r0", envRaw e)] [AtRegister "r0" 0 name] [])
      pure (envRaw e)
    Nothing -> pure 19
  if raw == 0
    then do
      modify (\m -> m {mModes = (index, mode) : filter ((/= index) . fst) (mModes m)})
      if mode == 1 || mode == 3 then made (Service registerWritten [("r0", index)] [("r0", 0)] [AtRegister "r1" 0 name] []) else pure ()
    else do
      made (call cacheRelease [("r0", index)] 0)
      word 0x34 >>= unbind
      word 0x34 >>= releaseTag
      setWord 0x34 0
  status <- translate raw
  word 0x34 >>= callback2 callback status
  pure status

blocking :: Int -> Int -> Maybe Int -> Word32 -> S Word32 -> S ()
blocking flag statusAt valueAt status value = do
  set <- word flag
  if set == 0
    then pure ()
    else do
      word statusAt >>= \at -> store at status
      case valueAt of
        Just offset -> do
          at <- word offset
          if at == 0 then pure () else value >>= store at
        Nothing -> pure ()
      done

-- | 0x7116e4 and its 0x71ed34.
writeW :: S Word32
writeW = do
  callback <- word 0
  present <- context
  (status, count) <-
    if not present
      then pure (5, 0)
      else do
        setWord 0 0
        index <- word 4 >>= lookupTag
        setWord 4 index
        e <- ask
        source <- word 0xc
        len <- word 8
        made (Service fsWrite [("r0", index), ("r1", source), ("r2", len)] [("r0", envCount e)] [] [AtRegister "r3" 0 (word32le (envRaw e))])
        status <- translate (envRaw e)
        setWord 8 (envCount e)
        count <- word 8
        pure (status, count)
  callback2 callback status count
  blocking 0x10 0x14 (Just 0x18) status (pure count)
  pure status

-- | 0x71eca4.
seekW :: S Word32
seekW = do
  index <- word 4 >>= lookupTag
  setWord 4 index
  callback <- word 0
  present <- context
  e <- ask
  raw <-
    if not present
      then pure 2
      else do
        whence <- word 0x10
        low <- word 8
        high <- word 0xc
        made (Service fsSeek [("r0", index), ("r1", whence), ("r2", low), ("r3", high)] [("r0", envRaw e)] [AtRegister "sp" 0 (word32le whence)] [])
        pure (envRaw e)
  status <- translate raw
  callback1 callback status
  blocking 0x14 0x18 Nothing status (pure 0)
  pure status

-- | The tag release, lookup and unbind each handle worker begins with.
retire :: S Word32
retire = do
  tag <- word 4
  releaseTag tag
  index <- lookupTag tag
  setWord 4 index
  unbind tag
  pure index

refresh :: S ()
refresh = ask >>= \e -> made (call mediaRefresh [("r0", envDevice e), ("r1", 0)] 0)

-- | 0x71f1d4.
closeFile :: S Word32
closeFile = do
  index <- retire
  e <- ask
  mode <- fileMode index
  value <- word 8
  first <- byte 0xc
  if mode /= 0 && (value /= 0 || first /= 0)
    then made (call fsCloseNamed [("r0", index), ("r1", value), ("r2", payloadBase + 0xc)] (envRaw e))
    else made (call fsClose [("r0", index)] (envRaw e))
  made (call cacheRelease [("r0", index)] 0)
  status <- translate (envRaw e)
  refresh
  pure status

-- | 0x71f078.
closeEntryFile :: S Word32
closeEntryFile = do
  index <- retire
  e <- ask
  mode <- fileMode index
  raw <-
    if mode == 0
      then pure 36
      else do
        attribute <- byte 8
        made (call fsCloseEntry [("r0", index), ("r1", attribute), ("r2", payloadBase + 9), ("r3", payloadBase + 0x1d)] (envRaw e))
        made (call cacheRelease [("r0", index)] 0)
        pure (envRaw e)
  status <- translate raw
  refresh
  pure status

-- | The post-close actions of 0x711768 and 0x711840.
afterClosing :: S Word32
afterClosing = do
  index <- word 4
  mode <- fileMode index
  if mode /= 3 && mode /= 1
    then pure 0
    else do
      made (call afterClose [("r0", index)] 0)
      kind <- fileKind index
      if kind == 1 then modify (\m -> m {mFlagged = 1}) else pure ()
      if kind == 0x100
        then ask >>= \e -> made (call rescan [] (envRescan e)) >> pure (envRescan e)
        else pure 0

-- | 0x711768 (with the blocking tail) and 0x711840 (without); both return 0.
closeW :: S Word32 -> Bool -> S Word32
closeW file blocked = do
  callback <- word 0
  setWord 0 0
  present <- context
  status <-
    if not present
      then pure 5
      else do
        closed <- file
        if closed == 0 then afterClosing else pure closed
  callback1 callback status
  if blocked then blocking 0x20 0x24 Nothing status (pure 0) else pure ()
  pure 0

-- | 0x71f134.
releaseW :: S Word32
releaseW = do
  index <- retire
  callback <- word 0
  present <- context
  e <- ask
  raw <-
    if not present
      then pure 2
      else do
        made (call fsRelease [("r0", index)] (envRaw e))
        made (call cacheRelease [("r0", index)] 0)
        pure (envRaw e)
  status <- translate raw
  callback1 callback status
  blocking 8 0xc Nothing status (pure 0)
  pure status

-- Cases ------------------------------------------------------------------------------

data Worker = Worker {workerEntry :: Word32, workerCode :: Word32, workerModel :: S Word32}

workerTable :: Word32
workerTable = 0x814740

stateRegion :: Bool -> (Word32, Word32) -> [(Word32, Word32)] -> [Word32] -> B.ByteString
stateRegion present (status, value) pairs tags =
  words32le ([0x31, status, value] ++ concat [[index, tag] | (index, tag) <- pairs] ++ tags ++ [0x70, if present then contextBase else 0])

contextRegion :: Env -> [(Word32, Word32)] -> Word32 -> B.ByteString
contextRegion e modes flagged = foldl put (B.replicate contextBytes 0) fields
  where
    fields =
      [(0x114, envDevice e), (0x21cc, flagged)]
        ++ [(16 * index, mode) | (index, mode) <- modes]
        ++ [(16 * index + 0xc, kind) | (index, kind) <- envKinds e]
        ++ [(0x2604 + 4 * number, 1) | number <- envKnown e]
    put bytes (offset, value) = let at = fromIntegral offset in B.take at bytes <> word32le value <> B.drop (at + 4) bytes

requestRegion :: Word32 -> B.ByteString -> B.ByteString
requestRegion requestCode payload = words32le [requestCode .|. 0x65 `shiftL` 16 .|. 0x1c `shiftL` 24, fromIntegral (B.length payload), payloadBase, 0] <> payload

workerCase :: B.ByteString -> String -> Worker -> Env -> [(Int, Word32)] -> [(Int, B.ByteString)] -> Scenario
workerCase bytes name worker e words' strings =
  Scenario
    { scenarioName = "worker-" ++ name
    , scenarioEntry = workerEntry worker
    , scenarioData =
        [ DataRegion "handle-state" stateBase (stateRegion (envContext e) initialSlot (envPairs e) (envTags e)) (stateRegion (envContext e) (mSlot final) (mPairs final) (mTags final))
        , DataRegion "request" requestBase (requestRegion (workerCode worker) payload) (requestRegion (workerCode worker) (mPayload final))
        ]
          ++ [DataRegion "context" contextBase (contextRegion e (envModes e) 0) (contextRegion e (mModes final) (mFlagged final)) | envContext e]
    , scenarioCalls = [Call (workerEntry worker) [requestBase, workerTable, workerCode worker, workerEntry worker] [] True]
    , scenarioServices = mServices final
    , scenarioResults = [result]
    , scenarioStop = Nothing
    }
  where
    initialSlot = (0x5a5a5a5a, 0x6b6b6b6b)
    payload = foldl (\b (offset, value) -> B.take offset b <> value <> B.drop (offset + B.length value) b) (B.replicate payloadBytes 0)
      ([(offset, word32le value) | (offset, value) <- words'] ++ strings)
    start = M payload initialSlot (envPairs e) (envTags e) (envModes e) 0 []
    (result, final) = runS (workerModel worker) e bytes start

openWorkerW, writeWorkerW, closeWorkerW, closeEntryWorkerW, seekWorkerW, releaseWorkerW :: Worker
openWorkerW = Worker openWorker 0x2c17 openW
writeWorkerW = Worker writeWorker 0x2c19 writeW
closeWorkerW = Worker closeWorker 0x2c1a (closeW closeFile True)
closeEntryWorkerW = Worker closeEntryWorker 0x2c1b (closeW closeEntryFile False)
seekWorkerW = Worker seekWorker 0x2c1c seekW
releaseWorkerW = Worker releaseWorker 0x2c1d releaseW

imagePath :: B.ByteString
imagePath = C.pack "B:/DCIM/100_FUJI/DSCF3097.JPG"

openRequest :: Word32 -> Bool -> [(Int, Word32)]
openRequest mode blocked =
  [(0, callbackStub), (0x38, mode), (0x3c, 0x61), (0x4c, 0x9)] ++ (if blocked then [(0x40, 1), (0x44, resultSlot), (0x48, resultSlot + 4)] else [])

scenarios :: B.ByteString -> [Scenario]
scenarios bytes =
  [ open "read-posted" env (openRequest 0 False)
  , open "read-blocking" env (openRequest 0 True)
  , open "write-new-directory" env (openRequest 1 True)
  , open "update-known-directory" env {envKnown = [0x67]} (openRequest 3 False)
  , open "append-new-directory" env (openRequest 2 False)
  , open "write-parse-refused" env {envParse = 8} (openRequest 1 False)
  , open "write-no-directory-slot" env {envSlotNumber = 0xff} (openRequest 1 False)
  , open "write-number-out-of-range" env {envNumber = 99} (openRequest 1 False)
  , open "write-number-above-range" env {envNumber = 1000} (openRequest 1 False)
  , open "read-failed" env {envRaw = 3} (openRequest 0 True)
  , open "write-failed-reported" env {envRaw = 9} (openRequest 1 False)
  , open "bad-mode" env (openRequest 4 True)
  , open "no-context" env {envContext = False} (openRequest 1 True)
  , open "no-context-silent" env {envContext = False} (openRequest 0 False ++ [(0, 0)])
  , open "claim-refused" env {envClaim = 0} (openRequest 0 False)
  , open "pairs-full" env {envPairs = [(fromIntegral index, 0x100 + fromIntegral index) | index <- [1 .. 16 :: Int]]} (openRequest 0 False)
  , write "posted" env {envCount = 0x200} (writeRequest False)
  , write "blocking-short" env {envCount = 0x180, envRaw = 16} (writeRequest True)
  , write "blocking-full-card" env {envCount = 0, envRaw = 32} (writeRequest True)
  , write "unknown-handle" env {envCount = 0} (writeRequest True ++ [(4, 0x99)])
  , write "no-context" env {envContext = False} (writeRequest True)
  , write "silent" env {envCount = 0x200} (writeRequest False ++ [(0, 0)])
  , seek "blocking" env {envRaw = 7} [(0, callbackStub), (4, 0x41), (8, 0x1000), (0xc, 2), (0x10, 1), (0x14, 1), (0x18, resultSlot)]
  , seek "posted" env [(0, callbackStub), (4, 0x42), (8, 0x20), (0x10, 0)]
  , seek "no-context" env {envContext = False} [(0, callbackStub), (4, 0x41), (0x14, 1), (0x18, resultSlot)]
  , close "read-file" env {envModes = [(3, 0)]} (closeRequest [])
  , close "written-flagged" env {envModes = [(3, 1)], envKinds = [(3, 1)]} (closeRequest [(8, 0x1234)])
  , close "written-rescan" env {envModes = [(3, 3)], envKinds = [(3, 0x100)]} (closeRequest [])
  , close "appended-named" env {envModes = [(3, 2)]} (closeRequest [(8, 0)])
  , close "unopened-named" env {envModes = [(3, 0)]} (closeRequest [(8, 7)])
  , close "failed" env {envModes = [(3, 1)], envRaw = 13} (closeRequest [])
  , close "no-context" env {envContext = False} (closeRequest [])
  , close "unknown-handle" env {envModes = [(0, 1)]} (closeRequest [(4, 0x99)])
  , closeEntry "not-open" env (entryRequest)
  , closeEntry "written-rescan" env {envModes = [(3, 3)], envKinds = [(3, 0x100)]} entryRequest
  , closeEntry "read-file-failed" env {envModes = [(3, 1)], envRaw = 9} entryRequest
  , closeEntry "no-context" env {envContext = False} entryRequest
  , release "blocking" env [(0, callbackStub), (4, 0x41), (8, 1), (0xc, resultSlot)]
  , release "failed" env {envRaw = 13} [(0, callbackStub), (4, 0x42)]
  , release "no-context" env {envContext = False} [(0, callbackStub), (4, 0x41), (8, 1), (0xc, resultSlot)]
  ]
  where
    open name e request = workerCase bytes ("open-" ++ name) openWorkerW e request [(4, imagePath)]
    write name e request = workerCase bytes ("write-" ++ name) writeWorkerW e request []
    seek name e request = workerCase bytes ("seek-" ++ name) seekWorkerW e request []
    close name e request = workerCase bytes ("close-" ++ name) closeWorkerW e request [(0xc, "DSCF3097.JPG")]
    closeEntry name e request = workerCase bytes ("close-entry-" ++ name) closeEntryWorkerW e request [(9, "20261121"), (0x1d, "123456")]
    release name e request = workerCase bytes ("release-" ++ name) releaseWorkerW e request []
    writeRequest blocked = [(0, callbackStub), (4, 0x41), (8, 0x200), (0xc, 0xd00000)] ++ (if blocked then [(0x10, 1), (0x14, resultSlot), (0x18, resultSlot + 4)] else [])
    closeRequest extra = [(0, callbackStub), (4, 0x41), (0x20, 1), (0x24, resultSlot)] ++ extra
    entryRequest = [(0, callbackStub), (4, 0x41), (8, 0x21)]
