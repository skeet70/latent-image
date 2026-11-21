{-# LANGUAGE OverloadedStrings #-}

-- | The catalog-worker cases: an independent model of the stock group-selection (0x2c2b),
-- catalog-rebuild (0x2c02), catalog-find (0x2c0d), request-completion (0x2c05) and
-- file-index resolution (0x2c10) workers, read from the bytes of [0x719614,0x71985c),
-- [0x71a834,0x71a93c) and [0x71b8c0,0x71b9e8), gives each case's result, the ordered calls
-- into the retained catalog, directory and media-state services, and the final payload,
-- media-state context, output record and caller words.
module Replacement.FileService.Catalog
  ( code
  , scenarios
  ) where

import Data.Bits (complement, shiftL, shiftR, (.&.), (.|.))
import qualified Data.ByteString as B
import Data.Word (Word32)
import Replacement.FileService.Native
import Replacement.FileService.Requests (translation)

code :: Code
code = Code
  { codeImageBase = 0x61c400
  , codeExtents =
      [ ("resolve-code", Extent 0x719614 0x71985c)
      , ("groups-code", Extent 0x71a834 0x71a93c)
      , ("find-code", Extent 0x71b8c0 0x71b9e8)
      , ("done-code", Extent 0x71c634 0x71c648)
      , ("translate-code", Extent 0x71d450 0x71d66c)
      ]
  , codeStockFunctions =
      [ (0x719614, 0x248), (0x71a834, 0x6c), (0x71a8a0, 0x9c), (0x71b8c0, 0xf0), (0x71b9b0, 0x38)
      , (0x71c634, 0x14), (0x71d450, 0x21c)
      ]
  , codeRetained = [("fill-memory", Extent 0x70e664 0x70e6c4)]
  , codeRetainedFunctions = [(0x70e664, 0x60)]
  }

-- Addresses ------------------------------------------------------------------

groupsWorker, rebuildWorker, findWorker, completeWorker, resolveWorker :: Word32
groupsWorker = 0x71a834
rebuildWorker = 0x71a8a0
findWorker = 0x71b8c0
completeWorker = 0x71b9b0
resolveWorker = 0x719614

catalogRebuild, catalogFind, completeRequest, semaphoreSignal, mediaReport :: Word32
catalogRebuild = 0x719de0
catalogFind = 0x71a93c
completeRequest = 0x71985c
semaphoreSignal = 0x724c00
mediaReport = 0x726c74

directoryMarked, reservedCluster, directoryReserved, clusterPath, catalogWindow, windowVisit :: Word32
directoryMarked = 0x715918
reservedCluster = 0x713d60
directoryReserved = 0x7158a8
clusterPath = 0x713db8
catalogWindow = 0x717a24
windowVisit = 0x712a4c

callbackStub, stateBase, contextSlot, resultSlot, firstOut, secondOut, contextBase, requestBase, payloadBase, recordBase :: Word32
callbackStub = 0x380000
stateBase = 0x910134
resultSlot = 0x910138
firstOut = 0x910140
secondOut = 0x910144
contextSlot = 0x910204
contextBase = 0xa00000
requestBase = 0xc10000
payloadBase = requestBase + 0x10
recordBase = 0xd00000

contextBytes, payloadBytes, recordBytes, lowBytes :: Int
contextBytes = 0x3d00
payloadBytes = 0x48
recordBytes = 0xc8
lowBytes = 0x2604

workerTable :: Word32
workerTable = 0x814740

-- Memory -------------------------------------------------------------------------

-- | The mapped data regions a case reads and writes, by name, base and bytes, and the
-- service calls made so far.
data M = M
  { mRegions :: [(String, Word32, B.ByteString)]
  , mServices :: [Service]
  }

-- | Scripted answers of the retained services.
data Env = Env
  { envRebuild :: Word32
  , envFind :: Word32
  , envComplete :: Word32
  , envMarkedRaw :: Word32
  , envMarked :: Word32
  , envClusterRaw :: Word32
  , envCluster :: Word32
  , envReserved :: Word32
  , envPathRaw :: Word32
  , envName :: B.ByteString
  , envScanRaw :: Word32
  , envScanned :: (Word32, Word32)
  }

env :: Env
env = Env
  { envRebuild = 0
  , envFind = 0
  , envComplete = 0
  , envMarkedRaw = 0
  , envMarked = 1
  , envClusterRaw = 0
  , envCluster = 0x33
  , envReserved = 1
  , envPathRaw = 0
  , envName = "B:DSCF3097.JPG"
  , envScanRaw = 0
  , envScanned = (0x10, 0x20)
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

modify :: (M -> M) -> S ()
modify f = S (\_ _ m -> ((), f m))

gets :: (M -> a) -> S a
gets f = S (\_ _ m -> (f m, m))

made :: Service -> S ()
made service = modify (\m -> m {mServices = mServices m ++ [service]})

located :: Word32 -> Int -> [(String, Word32, B.ByteString)] -> (String, Int)
located address size regions = case [(name, fromIntegral (address - base)) | (name, base, bytes) <- regions, base <= address, address + fromIntegral size <= base + fromIntegral (B.length bytes)] of
  found : _ -> found
  [] -> error ("a case touches unmapped memory at " ++ hexWord address)

readBytes :: Word32 -> Int -> S B.ByteString
readBytes address size = gets $ \m ->
  let (name, offset) = located address size (mRegions m)
   in mconcat [B.take size (B.drop offset bytes) | (name', _, bytes) <- mRegions m, name' == name]

writeBytes :: Word32 -> B.ByteString -> S ()
writeBytes address value = modify $ \m ->
  let (name, offset) = located address (B.length value) (mRegions m)
   in m {mRegions = [(name', base, if name' == name then B.take offset bytes <> value <> B.drop (offset + B.length value) bytes else bytes) | (name', base, bytes) <- mRegions m]}

littleEndian :: B.ByteString -> Word32
littleEndian bytes = sum [fromIntegral byte `shiftL` (8 * index) | (index, byte) <- zip [0 ..] (B.unpack bytes)]

rd8, rd16, rd32 :: Word32 -> S Word32
rd8 address = littleEndian <$> readBytes address 1
rd16 address = littleEndian <$> readBytes address 2
rd32 address = littleEndian <$> readBytes address 4

wr8, wr32 :: Word32 -> Word32 -> S ()
wr8 address value = writeBytes address (B.singleton (fromIntegral value))
wr32 address value = writeBytes address (word32le value)

call :: Word32 -> [(String, Word32)] -> Word32 -> Service
call address inputs result = Service address inputs [("r0", result)] [] []

-- | 0x71d450 through its jump table, with the media report the case block makes.
translate :: Word32 -> S Word32
translate raw = do
  bytes <- image
  let (status, reported) = translation bytes raw
  mapM_ (\value -> made (call mediaReport [("r0", 2), ("r1", value)] 0)) reported
  pure status

callback1 :: Word32 -> Word32 -> S ()
callback1 0 _ = pure ()
callback1 function status = made (call function [("r0", status)] 0)

rebuildCall :: Word32 -> Word32 -> S Word32
rebuildCall full mode = do
  e <- ask
  made (call catalogRebuild [("r0", full), ("r1", mode)] (envRebuild e))
  pure (envRebuild e)

-- Workers ---------------------------------------------------------------------------

-- | 0x71a834: the context's group count byte is read without a context check; it returns
-- the delivered status.
groupsW :: S Word32
groupsW = do
  mask <- rd32 (payloadBase + 4)
  callback <- rd32 payloadBase
  context <- rd32 contextSlot
  count <- rd8 (context + 0x2600)
  let above = if count >= 32 then 0 else 0xffffffff `shiftL` fromIntegral count
  status <-
    if mask .&. complement above == 0
      then pure 8
      else do
        wr32 (context + 0x2210) mask
        _ <- rebuildCall 0 0
        pure 0
  callback1 callback status
  pure status

-- | 0x71a8a0: the context byte +0x1f5 is cleared after translation, also with no context.
rebuildW :: S Word32
rebuildW = do
  callback <- rd32 payloadBase
  full <- rd8 (payloadBase + 0xc)
  mode <- rd32 (payloadBase + 8)
  context <- rd32 contextSlot
  raw <-
    if context == 0
      then pure 2
      else do
        raw <- rebuildCall (if full == 1 then 1 else 0) mode
        context' <- rd32 contextSlot
        wr32 (context' + 0x21cc) (if mode == 1 then 1 else 0)
        pure raw
  status <- translate raw
  context' <- rd32 contextSlot
  wr8 (context' + 0x1f5) 0
  callback1 callback status
  pure 0

-- | 0x71b8c0 with 0x70e664 clearing 188 record bytes.
findW :: S Word32
findW = do
  fields <- mapM (rd32 . (payloadBase +)) [0, 4, 8, 0xc, 0x10, 0x14]
  let (callback, record, a, b, c, d) = case fields of
        [f0, f1, f2, f3, f4, f5] -> (f0, f1, f2, f3, f4, f5)
        _ -> error "six payload words"
  context <- rd32 contextSlot
  raw <-
    if context == 0
      then pure 2
      else do
        mark <- rd32 (context + 0x21cc)
        rebuilt <-
          if mark == 1 || (a == 2 && b == 1)
            then do
              raw <- rebuildCall 0 0
              context' <- rd32 contextSlot
              wr32 (context' + 0x21cc) 0
              pure raw
            else pure 0
        if rebuilt /= 0
          then pure rebuilt
          else do
            writeBytes record (B.replicate 188 0)
            first <- rd8 (payloadBase + 0x18)
            let name = if first == 0 then 0 else payloadBase + 0x18
            nameBytes <- readBytes (payloadBase + 0x18) 48
            e <- ask
            made
              ( Service catalogFind [("r0", a), ("r1", b), ("r2", c), ("r3", d)] [("r0", envFind e)]
                  ([AtRegister "sp" 0 (words32le [name, record]), AtAddress record 0 (B.replicate 188 0)] ++ [AtAddress name 0 nameBytes | name /= 0])
                  [AtAddress record 0x6c (words32le [0x00650067])]
              )
            writeBytes (record + 0x6c) (words32le [0x00650067])
            pure (envFind e)
  status <- translate raw
  callback1 callback status
  pure 0

-- | 0x71b9b0.
completeW :: S Word32
completeW = do
  callback <- rd32 payloadBase
  e <- ask
  made (call completeRequest [] (envComplete e))
  status <- translate (envComplete e)
  callback1 callback status
  pure 0

-- | 0x719614: the found directory's window and every later directory's two context words are
-- summed to the payload's two pointers; refusals deliver 8 untranslated.
resolveW :: S Word32
resolveW = do
  callback <- rd32 payloadBase
  context <- rd32 contextSlot
  status <- if context == 0 then pure 8 else resolveIn context
  callback1 callback status
  blocked <- rd32 (payloadBase + 0x10)
  if blocked == 0
    then pure ()
    else do
      at <- rd32 (payloadBase + 0x14)
      if at == 0 then pure () else wr32 at status
      made (call semaphoreSignal [("r0", 0x3b)] 0)
  pure 0

resolveIn :: Word32 -> S Word32
resolveIn context = do
  index <- rd32 (payloadBase + 4)
  found <- search index 100 0
  case found of
    Nothing -> pure 8
    Just (directory, first) -> do
      e <- ask
      let refused raw next = if raw /= 0 then pure 8 else next
          name = B.take 32 (envName e <> B.replicate 32 0)
          sumFrom = sum (map fromIntegral (B.unpack (B.takeWhile (/= 0) (B.drop 8 name))))
          position = index - first
          windowFirst = (position `div` 5000) * 5000
          (scannedFirst, scannedSecond) = envScanned e
      made (Service directoryMarked [("r0", directory)] [("r0", envMarkedRaw e)] [] [AtRegister "r1" 0 (word32le (envMarked e))])
      refused (envMarkedRaw e) $ do
        made (Service reservedCluster [("r0", directory)] [("r0", envClusterRaw e)] [] [AtRegister "r1" 0 (word32le (envCluster e))])
        refused (envClusterRaw e) $ do
          made (Service directoryReserved [("r0", directory)] [("r0", 0)] [] [AtRegister "r1" 0 (word32le (envReserved e))])
          made
            ( Service clusterPath [("r0", envCluster e), ("r2", envReserved e), ("r3", 0)] [("r0", envPathRaw e)]
                [AtRegister "r1" 0 (B.replicate 32 0)] [AtRegister "r1" 0 name]
            )
          refused (envPathRaw e) $
            if B.elem 0 (B.take 8 name)
              then pure 8
              else do
                made
                  ( Service catalogWindow [("r1", windowVisit)] [("r0", envScanRaw e)]
                      [ AtRegister "r0" 0 (words32le [0, envCluster e, envMarked e, windowFirst, windowFirst + 4999, 0, directory])
                      , AtRegister "r2" 0 (words32le [0, 0, position, sumFrom])
                      ]
                      [AtRegister "r2" 0 (words32le [scannedFirst, scannedSecond])]
                  )
                refused (envScanRaw e) $ do
                  later <- mapM (\number -> (,) <$> rd32 (context + 0x208 + 4 * number) <*> rd32 (context + 0x11a8 + 4 * number)) [directory + 1 .. 999]
                  firstAt <- rd32 (payloadBase + 8)
                  wr32 firstAt (scannedFirst + sum (map fst later))
                  secondAt <- rd32 (payloadBase + 0xc)
                  wr32 secondAt (scannedSecond + sum (map snd later))
                  pure 0
  where
    search index directory total
      | directory == 1000 = pure Nothing
      | otherwise = do
          count <- rd16 (context + 0x35a4 + 2 * (directory - 100))
          if total <= index && index < total + count
            then pure (Just (directory, total))
            else search index (directory + 1) (total + count)

-- Cases ------------------------------------------------------------------------------

data Worker = Worker {workerEntry :: Word32, workerCode :: Word32, workerModel :: S Word32}

groupsWorkerW, rebuildWorkerW, findWorkerW, completeWorkerW, resolveWorkerW :: Worker
groupsWorkerW = Worker groupsWorker 0x2c2b groupsW
rebuildWorkerW = Worker rebuildWorker 0x2c02 rebuildW
findWorkerW = Worker findWorker 0x2c0d findW
completeWorkerW = Worker completeWorker 0x2c05 completeW
resolveWorkerW = Worker resolveWorker 0x2c10 resolveW

-- | The media-state context a case maps; a null context maps the low bytes stock reaches
-- through a null context instead.
data Context = Context [(Int, B.ByteString)] | NullContext [(Int, B.ByteString)]

placed :: Int -> [(Int, B.ByteString)] -> B.ByteString
placed size = foldl (\bytes (offset, value) -> B.take offset bytes <> value <> B.drop (offset + B.length value) bytes) (B.replicate size 0)

-- | Context fields every case starts with, under the case's own fields.
baseFields :: [(Int, B.ByteString)]
baseFields = [(0x1f5, B.singleton 0x77), (0x2210, word32le 0xdeadbeef), (0x2600, B.singleton 4)]

halfwords :: [Word32] -> B.ByteString
halfwords values = B.pack (concat [[fromIntegral value, fromIntegral (value `shiftR` 8)] | value <- values])

workerCase :: B.ByteString -> String -> Worker -> Env -> Context -> [(Int, Word32)] -> [(Int, B.ByteString)] -> Scenario
workerCase bytes name worker e context words' strings =
  Scenario
    { scenarioName = "catalog-" ++ name
    , scenarioEntry = workerEntry worker
    , scenarioData = [DataRegion region base initial final | ((region, base, initial), (_, _, final)) <- zip regions (mRegions done)]
    , scenarioCalls = [Call (workerEntry worker) [requestBase, workerTable, workerCode worker, workerEntry worker] [] True]
    , scenarioServices = mServices done
    , scenarioResults = [result]
    , scenarioStop = Nothing
    }
  where
    payload = placed payloadBytes ([(offset, word32le value) | (offset, value) <- words'] ++ strings)
    header = words32le [workerCode worker .|. 0x65 `shiftL` 16 .|. 0x1c `shiftL` 24, fromIntegral payloadBytes, payloadBase, 0]
    (contextWord, contextRegion) = case context of
      Context fields -> (contextBase, ("context", contextBase, placed contextBytes (baseFields ++ fields)))
      NullContext fields -> (0, ("null-context", 0, placed lowBytes (baseFields ++ fields)))
    state = placed (fromIntegral (contextSlot + 4 - stateBase)) [(fromIntegral (resultSlot - stateBase), words32le [0x5a5a5a5a, 0x6b6b6b6b, 0x11111111, 0x22222222]), (fromIntegral (contextSlot - stateBase), word32le contextWord)]
    regions =
      [ ("state", stateBase, state)
      , ("request", requestBase, header <> payload)
      , contextRegion
      , ("record", recordBase, B.replicate recordBytes 0xa5)
      ]
    (result, done) = runS (workerModel worker) e bytes (M regions [])

scenarios :: B.ByteString -> [Scenario]
scenarios bytes =
  [ groups "selected" env (Context []) [(0, callbackStub), (4, 0x18)]
  , groups "selected-rebuild-failed" env {envRebuild = 5} (Context []) [(0, callbackStub), (4, 0xf)]
  , groups "bit-at-count-refused" env (Context []) [(0, callbackStub), (4, 0x10)]
  , groups "wide-count" env (Context [(0x2600, B.singleton 40)]) [(0, callbackStub), (4, 0x80000000)]
  , groups "count-31" env (Context [(0x2600, B.singleton 31)]) [(0, callbackStub), (4, 0x80000000)]
  , groups "zero-count" env (Context [(0x2600, B.singleton 0)]) [(0, callbackStub), (4, 1)]
  , groups "silent" env (Context []) [(4, 2)]
  , groups "null-context" env (NullContext [(0x2600, B.singleton 2)]) [(0, callbackStub), (4, 3)]
  , rebuild "mode-one-full" env (Context [(0x21cc, word32le 2)]) [(0, callbackStub), (8, 1), (0xc, 1)]
  , rebuild "mode-zero-failed" env {envRebuild = 9} (Context [(0x21cc, word32le 1)]) [(0, callbackStub), (8, 0)]
  , rebuild "flag-byte-two" env {envRebuild = 3} (Context []) [(0, callbackStub), (8, 2), (0xc, 2)]
  , rebuild "silent" env (Context []) [(8, 1), (0xc, 1)]
  , rebuild "no-context" env (NullContext []) [(0, callbackStub), (8, 1), (0xc, 1)]
  , find "named" env (Context []) (findRequest 1 0) [(0x18, "100_FUJI")]
  , find "unnamed-failed" env {envFind = 4} (Context []) (findRequest 1 0) []
  , find "mark-pending" env (Context [(0x21cc, word32le 1)]) (findRequest 1 0) [(0x18, "100_FUJI")]
  , find "mark-two" env (Context [(0x21cc, word32le 2)]) (findRequest 1 0) []
  , find "rewind-commit" env (Context []) (findRequest 2 1) []
  , find "rewind-no-commit" env (Context []) (findRequest 2 0) []
  , find "step-commit" env (Context []) (findRequest 1 1) []
  , find "rebuild-failed" env {envRebuild = 7} (Context [(0x21cc, word32le 1)]) (findRequest 1 0) []
  , find "reported" env {envFind = 9} (Context []) (findRequest 3 0) [(0x18, "DSCF3097.JPG")]
  , find "no-context" env (NullContext []) (findRequest 1 0) []
  , find "silent" env (Context []) (drop 1 (findRequest 1 0)) []
  , complete "ok" env [(0, callbackStub)]
  , complete "failed" env {envComplete = 13} [(0, callbackStub)]
  , complete "reported" env {envComplete = 9} [(0, callbackStub)]
  , complete "silent" env {envComplete = 2} []
  , resolve "blocking" env counts (resolveRequest 5010 True)
  , resolve "posted-first" env counts (resolveRequest 0 False)
  , resolve "second-directory" env counts (resolveRequest 4 False)
  , resolve "blocking-no-status" env counts (resolveRequest 3 False ++ [(0x10, 1)])
  , resolve "last-directory" env lastDirectory (resolveRequest 7 True)
  , resolve "past-end" env counts (resolveRequest 6007 True)
  , resolve "marked-refused" env {envMarkedRaw = 1} counts (resolveRequest 5010 True)
  , resolve "cluster-refused" env {envClusterRaw = 2} counts (resolveRequest 5010 True)
  , resolve "path-refused" env {envPathRaw = 7} counts (resolveRequest 5010 True)
  , resolve "short-name" env {envName = "B:DSC"} counts (resolveRequest 5010 True)
  , resolve "eight-byte-name" env {envName = "B:DSCF30"} counts (resolveRequest 5010 True)
  , resolve "scan-refused" env {envScanRaw = 1} counts (resolveRequest 5010 True)
  , resolve "no-context" env (NullContext []) (resolveRequest 5010 True)
  ]
  where
    groups name e context request = workerCase bytes ("groups-" ++ name) groupsWorkerW e context request []
    rebuild name e context request = workerCase bytes ("rebuild-" ++ name) rebuildWorkerW e context request []
    find name e context request strings = workerCase bytes ("find-" ++ name) findWorkerW e context request strings
    complete name e request = workerCase bytes ("complete-" ++ name) completeWorkerW e (Context []) request []
    resolve name e context request = workerCase bytes ("resolve-" ++ name) resolveWorkerW e context request []
    findRequest a b = [(0, callbackStub), (4, recordBase), (8, a), (0xc, b), (0x10, 3), (0x14, 0xffffffff)]
    resolveRequest index blocked = [(0, callbackStub), (4, index), (8, firstOut), (0xc, secondOut)] ++ (if blocked then [(0x10, 1), (0x14, resultSlot)] else [])
    totals = [(0x208 + 4 * number, word32le value) | (number, value) <- [(102, 1000), (103, 5), (500, 0x100), (999, 2)]]
      ++ [(0x11a8 + 4 * number, word32le value) | (number, value) <- [(102, 1), (103, 7), (500, 0x200), (999, 3)]]
    counts = Context ((0x35a4, halfwords [3, 0, 6000, 4]) : totals)
    lastDirectory = Context ((0x35a4 + 2 * 899, halfwords [8]) : totals)
