{-# LANGUAGE OverloadedStrings #-}

-- | The select-image worker cases: an independent model of the stock worker at 0x719098
-- (code 0x2c1f), read from the bytes of [0x719098,0x719614), gives each case's ordered calls
-- into the retained path, catalog and numbering services, the delivered status and the final
-- media-state context.
module Replacement.FileService.Selection
  ( code
  , scenarios
  ) where

import Control.Monad (foldM, unless, when)
import Data.Bits (shiftL, shiftR, (.&.), (.|.))
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import Data.Word (Word32)
import Replacement.FileService.Native
import Replacement.FileService.Requests (translation)

code :: Code
code = Code
  { codeImageBase = 0x61c400
  , codeExtents =
      [ ("worker-code", Extent 0x719098 0x719614)
      , ("translate-code", Extent 0x71d450 0x71d66c)
      ]
  , codeStockFunctions = [(0x719098, 0x57c), (0x71d450, 0x21c)]
  , codeRetained = [("fill-memory", Extent 0x70e664 0x70e6c4), ("text-difference", Extent 0x70e884 0x70e8d8)]
  , codeRetainedFunctions = [(0x70e664, 0x60), (0x70e884, 0x54)]
  }

-- Addresses ------------------------------------------------------------------

selectWorker, splitPath, extensionKind, kindSupported, directoryNumber, pageCluster, directoryPath :: Word32
selectWorker = 0x719098
splitPath = 0x71543c
extensionKind = 0x713264
kindSupported = 0x713508
directoryNumber = 0x712dac
pageCluster = 0x713d60
directoryPath = 0x713db8

catalogFind, markedBefore, pageCounted, snapshot, pageSelected, windowLoad, mediaReport :: Word32
catalogFind = 0x7189b4
markedBefore = 0x7153a8
pageCounted = 0x715f80
snapshot = 0x712b4c
pageSelected = 0x715918
windowLoad = 0x717a24
mediaReport = 0x726c74

callbackStub, contextSlot, contextBase, requestBase, payloadBase, workerTable :: Word32
callbackStub = 0x380000
contextSlot = 0x910204
contextBase = 0xa00000
requestBase = 0xc10000
payloadBase = requestBase + 0x10
workerTable = 0x814740

contextBytes, payloadBytes :: Int
contextBytes = 0x16c64
payloadBytes = 0x34

-- | The context ranges the worker reads or writes in these cases.
contextRegions :: [(String, Int, Int)]
contextRegions =
  [ ("context-state", 0x2140, 0x3600)
  , ("context-tags", 0xb2a0, 0xb300)
  , ("context-counts", 0x12700, 0x12800)
  , ("context-files", 0x15600, contextBytes)
  ]

-- Model ------------------------------------------------------------------------

-- | Scripted answers of the retained services and the initial context bytes.
data Env = Env
  { envContext :: Bool
  , envCallback :: Word32
  , envBytes :: [(Int, B.ByteString)]
  , envSplit :: Word32
  , envMarker :: Word32
  , envKind :: Word32
  , envSupported :: Word32
  , envNumber :: Word32
  , envClusters :: [(Word32, (Word32, Word32))]
  , envPaths :: [(Word32, (Word32, B.ByteString))]
  , envFind :: Word32
  , envPosition :: Word32
  , envMarked :: Word32
  , envCounted :: [Word32]
  , envSelected :: (Word32, Word32)
  }

env :: Env
env = Env
  { envContext = True
  , envCallback = callbackStub
  , envBytes = []
  , envSplit = 0
  , envMarker = 0
  , envKind = 0x10
  , envSupported = 1
  , envNumber = 0x66
  , envClusters = []
  , envPaths = []
  , envFind = 0
  , envPosition = 7
  , envMarked = 3
  , envCounted = []
  , envSelected = (0, 1)
  }

-- | The context, the frame words at r11-0x94 (cluster) and r11-0x88 (selected), whether the
-- query at r11-0xb4 has been handed to 0x7189b4, and the service calls.
data M = M
  { mContext :: B.ByteString
  , mSelected :: Word32
  , mQueried :: Bool
  , mServices :: [Service]
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

littleEndian :: B.ByteString -> Word32
littleEndian bytes = sum [fromIntegral value `shiftL` (8 * index) | (index, value) <- zip [0 ..] (B.unpack bytes)]

ldr, ldrh, ldrb :: Word32 -> S Word32
ldr offset = gets (littleEndian . B.take 4 . B.drop (fromIntegral offset) . mContext)
ldrh offset = gets (littleEndian . B.take 2 . B.drop (fromIntegral offset) . mContext)
ldrb offset = gets (littleEndian . B.take 1 . B.drop (fromIntegral offset) . mContext)

poke :: Int -> B.ByteString -> B.ByteString -> B.ByteString
poke at value bytes = B.take at bytes <> value <> B.drop (at + B.length value) bytes

str, strh :: Word32 -> Word32 -> S ()
str offset value = modify (\m -> m {mContext = poke (fromIntegral offset) (word32le value) (mContext m)})
strh offset value = modify (\m -> m {mContext = poke (fromIntegral offset) (B.take 2 (word32le value)) (mContext m)})

pattern', prefix, directory, zeros :: B.ByteString
pattern' = "DSCF3097JPG\0"
prefix = "B:\\DCIM\\100_FUJI\\\0"
directory = "100_FUJI\0"
zeros = B.replicate 32 0

-- | 0x70e884 returns 0 exactly when the two strings are equal up to their NULs.
sameText :: B.ByteString -> B.ByteString -> Bool
sameText a b = B.takeWhile (/= 0) a == B.takeWhile (/= 0) b

-- | 0x71d450 through its jump table, with the media report the case block makes.
translate :: Word32 -> S Word32
translate raw = do
  bytes <- image
  let (status, reported) = translation bytes raw
  mapM_ (\value -> made (call mediaReport [("r0", 2), ("r1", value)] 0)) reported
  pure status

-- Worker -----------------------------------------------------------------------

-- | 0x719098: [r11-0xc0] is the raw status, 2 without a context; 0x7195e8 translates it and
-- calls the callback saved from payload +0 at entry. It returns 0.
selectW :: S Word32
selectW = do
  e <- ask
  raw <- if envContext e then parsed else pure 2
  status <- translate raw
  unless (envCallback e == 0) (made (call (envCallback e) [("r0", status)] 0))
  pure 0

-- | 0x7190d0: zero the prefix, split the path; any status after a split of 0 reaches 0x7193b0.
parsed :: S Word32
parsed = do
  e <- ask
  made
    ( Service splitPath [("r0", payloadBase + 4)] [("r0", envSplit e)] [AtIndirect "sp" 0 zeros]
        [ AtRegister "r1" 0 directory
        , AtRegister "r2" 0 pattern'
        , AtRegister "r3" 0 (word32le (envMarker e))
        , AtIndirect "sp" 0 prefix
        , AtIndirect "sp" 4 (word32le 0)
        ]
    )
  if envSplit e /= 0
    then pure (envSplit e)
    else do
      raw <- located
      when (raw /= 0) reload
      pure raw

-- | 0x719110 to 0x7193ac.
located :: S Word32
located = do
  e <- ask
  made (Service extensionKind [] [("r0", envKind e)] [AtRegister "r0" 0 "JPG\0"] [])
  if envKind e == 0
    then pure 4
    else do
      made (call kindSupported [("r0", envKind e)] (envSupported e))
      if envSupported e == 0
        then pure 4
        else do
          modify (\m -> m {mSelected = 0})
          made (Service directoryNumber [("r1", envMarker e)] [("r0", envNumber e)] [AtRegister "r0" 0 directory] [])
          let r10 = envNumber e .&. 0xffff
          entry <- ldr (0x2604 + 4 * r10)
          let live = entry /= 0 && entry /= 0xffffffff
          if r10 > 99 && live
            then modify (\m -> m {mSelected = 1}) >> pages r10 r10 r10
            else do
              (r4, r5) <-
                if envMarker e /= 0
                  then (,) <$> ldr 0x2178 <*> ldr 0x217c
                  else do
                    when (r10 /= 0 && live) (modify (\m -> m {mSelected = 1}))
                    (,) <$> ldr 0x218c <*> ldr 0x2190
              if r4 > r5 then pure 7 else pages r10 r4 r5

-- | The page loop at 0x719224: r4 runs from its first page while r4 <= r5.
pages :: Word32 -> Word32 -> Word32 -> S Word32
pages r10 r4 r5 = do
  e <- ask
  (looked, cluster) <- lookupCluster r4
  matched <-
    if looked /= 0 || cluster == 0
      then pure False
      else do
        let (answer, path) = maybe (7, "") id (lookup cluster (envPaths e))
        made (Service directoryPath [("r0", cluster), ("r2", envMarker e), ("r3", 0)] [("r0", answer)] [AtRegister "r1" 0 zeros] [AtRegister "r1" 0 path])
        pure (answer == 0 && sameText path prefix)
  if matched
    then do
      selected <- gets mSelected
      made
        ( Service catalogFind [] [("r0", envFind e)]
            [AtRegister "r0" 0 (words32le [1, cluster, selected, 0, 4999, 0xffffffff, r4]), AtIndirect "r0" 0x1c pattern']
            [AtRegister "r1" 0 (word32le (envPosition e))]
        )
      modify (\m -> m {mQueried = True})
      if envFind e /= 0 then pure (envFind e) else current r10 r4 >> pure 0
    else if r4 + 1 <= r5 then pages r10 (r4 + 1) r5 else pure (if looked == 0 then 7 else looked)

lookupCluster :: Word32 -> S (Word32, Word32)
lookupCluster page = do
  e <- ask
  let (looked, cluster) = maybe (0, 0) id (lookup page (envClusters e))
  made (Service pageCluster [("r0", page)] [("r0", looked)] [] [AtRegister "r1" 0 (word32le cluster)])
  pure (looked, cluster)

counted :: Word32 -> S Word32
counted page = do
  e <- ask
  let answer = if page `elem` envCounted e then 1 else 0
  made (call pageCounted [("r0", page)] answer)
  pure answer

-- | 0x7192dc to 0x7195e4.
current :: Word32 -> Word32 -> S ()
current r10 page = do
  e <- ask
  let position = envPosition e
  str 0x2174 position
  str 0x2170 page
  ldr 0x21a4 >>= str 0x2148 . (+ 1)
  str 0x214c position
  ldr 0x21b0 >>= str 0x2158
  made (call markedBefore [("r0", position)] (envMarked e))
  ldr 0x21b4 >>= str 0x215c . (+ envMarked e)
  ldr (0xb2a4 + 4 * (position `mod` 5000)) >>= str 0x2164 . (.&. 0xffffff)
  here <- ldr 0x2170
  flag <- counted here
  when (flag == 0) $ do
    counter <- ldrb (0x2218 + here)
    when (counter /= 0xff) (str 0x2210 (if counter >= 32 then 0 else 1 `shiftL` fromIntegral counter))
  total <- ldr 0x2174
  str 0x2160 total
  str 0x2214 total
  group <- (`shiftR` 1) <$> (ldr 0x214c >>= ldrb . (+ 0x15846))
  if group == 0 then str 0x16c60 0 else ldrh (0x156b6 + 2 * group) >>= str 0x16c60
  final <- ldr 0x2174
  flags <- mapM (ldrb . (+ 0x15846)) [0 .. final]
  let start = (0xffff + fromIntegral (length [f | f <- flags, f == 0 || f .&. 1 == 1])) .&. 0xffff
  ldr 0x2170 >>= ldrh . (+ 0x1270e) . (* 2) >>= str 0x2150
  str 0x2154 start
  r6 <-
    if r10 <= 100
      then pure start
      else foldM numbered start [100 .. r10 - 1]
  strh 0x16bce r6
  made (call snapshot [] 0)
  where
    numbered r6 index = do
      size <- ldrh (0x34dc + 2 * index)
      ldr 0x2160 >>= str 0x2160 . (+ size)
      files <- ldrh (0x1270e + 2 * index)
      flag <- counted index
      when (flag == 1) (ldr 0x2214 >>= str 0x2214 . (+ size))
      pure ((r6 + files) .&. 0xffff)

-- | 0x7193b0: reload the 5000-position window of the current page. The query words at
-- r11-0xa0 and r11-0x98 keep what an earlier 0x7189b4 call saw.
reload :: S ()
reload = do
  e <- ask
  (looked, cluster) <- ldr 0x2170 >>= lookupCluster
  unless (looked /= 0 || cluster == 0) $ do
    page <- ldr 0x2170
    let (answer, selected) = envSelected e
    made (Service pageSelected [("r0", page)] [("r0", answer)] [] [AtRegister "r1" 0 (word32le selected)])
    modify (\m -> m {mSelected = selected})
    unless (answer /= 0) $ do
      position <- ldr 0x2174
      let low = position - position `mod` 5000
      queried <- gets mQueried
      made
        ( Service windowLoad [("r1", 0), ("r2", 0)] [("r0", 0)]
            ( [AtRegister "r0" 0 (words32le [0, cluster, selected, low, low + 4999]), AtRegister "r0" 0x18 (word32le page)]
                ++ (if queried then [AtRegister "r0" 0x14 (word32le 0xffffffff), AtIndirect "r0" 0x1c pattern'] else [])
            )
            []
        )

-- Cases ------------------------------------------------------------------------------

initialContext :: Env -> B.ByteString
initialContext e = foldl (\bytes (at, value) -> poke at value bytes) (B.replicate contextBytes 0) (envBytes e)

requestRegion :: B.ByteString -> B.ByteString
requestRegion payload = words32le [0x2c1f .|. 0x65 `shiftL` 16 .|. 0x1c `shiftL` 24, fromIntegral payloadBytes, payloadBase, 0] <> payload

selectCase :: B.ByteString -> String -> Env -> Scenario
selectCase bytes name e =
  Scenario
    { scenarioName = "worker-select-" ++ name
    , scenarioEntry = selectWorker
    , scenarioData =
        [ DataRegion "context-slot" contextSlot slot slot
        , DataRegion "request" requestBase (requestRegion payload) (requestRegion payload)
        ]
          ++ [ DataRegion region (contextBase + fromIntegral low) (slice low high start) (slice low high (mContext final))
             | envContext e
             , (region, low, high) <- contextRegions
             ]
    , scenarioCalls = [Call selectWorker [requestBase, workerTable, 0x2c1f, selectWorker] [] True]
    , scenarioServices = mServices final
    , scenarioResults = [result]
    , scenarioStop = Nothing
    }
  where
    slot = word32le (if envContext e then contextBase else 0)
    path = C.pack "B:\\DCIM\\100_FUJI\\DSCF3097.JPG"
    payload = word32le (envCallback e) <> path <> B.replicate (payloadBytes - 4 - B.length path) 0
    start = initialContext e
    slice low high = B.take (high - low) . B.drop low
    (result, final) = runS selectW e bytes (M start 0 False [])

word :: Int -> Word32 -> (Int, B.ByteString)
word at value = (at, word32le value)

half :: Int -> Word32 -> (Int, B.ByteString)
half at value = (at, B.take 2 (word32le value))

byte :: Int -> Word32 -> (Int, B.ByteString)
byte at value = (at, B.singleton (fromIntegral value))

matching, other :: B.ByteString
matching = prefix
other = "B:\\DCIM\\101_FUJI\\\0"

-- | Context words every success case reads besides its pages.
loaded :: [(Int, B.ByteString)]
loaded =
  [ word 0x21a4 40, word 0x21b0 9, word 0x21b4 2, word 0x2210 0x80, word 0x16c60 0x5555
  , word (0xb2a4 + 4 * 7) 0x55123456, word (0xb2a4 + 4 * 20) 0x66abcdef
  , half (0x156b6 + 2 * 2) 0x31, half (0x1270e + 2 * 0x66) 12, half (0x1270e + 2 * 0x67) 13
  , half (0x1270e + 2 * 100) 5, half (0x1270e + 2 * 101) 6, half (0x34dc + 2 * 100) 20, half (0x34dc + 2 * 101) 30
  ]
    ++ [byte (0x15846 + index) flags | (index, flags) <- zip [0 ..] [0, 1, 4, 3, 2, 0, 6, 5, 9, 0x11, 0, 8, 0x80, 0x81, 2, 3, 0, 0, 1, 4, 7, 9]]

scenarios :: B.ByteString -> [Scenario]
scenarios bytes =
  [ selectCase bytes "no-context" env {envContext = False}
  , selectCase bytes "no-context-silent" env {envContext = False, envCallback = 0}
  , selectCase bytes "split-refused" env {envSplit = 4}
  , selectCase bytes "unknown-extension-reload" env
      { envKind = 0
      , envBytes = [word 0x2170 0x65, word 0x2174 5003]
      , envClusters = [(0x65, (0, 0x880))]
      }
  , selectCase bytes "unsupported-kind-no-cluster" env {envSupported = 0, envBytes = [word 0x2170 0x65]}
  , selectCase bytes "known-directory" env
      { envNumber = 0x10066
      , envBytes = loaded ++ [word (0x2604 + 4 * 0x66) 1, byte (0x2218 + 0x66) 3]
      , envClusters = [(0x66, (0, 0x900))]
      , envPaths = [(0x900, (0, matching))]
      , envCounted = [101]
      }
  , selectCase bytes "directory-range-last-page" env
      { envNumber = 0x2a
      , envPosition = 20
      , envBytes = loaded ++ [word (0x2604 + 4 * 0x2a) 1, word 0x218c 0x64, word 0x2190 0x67, byte (0x2218 + 0x67) 0xff]
      , envClusters = [(0x64, (9, 0)), (0x65, (0, 0x901)), (0x66, (0, 0x902)), (0x67, (0, 0x903))]
      , envPaths = [(0x901, (3, matching)), (0x902, (0, other)), (0x903, (0, matching))]
      }
  , selectCase bytes "marked-range-counted-page" env
      { envMarker = 1
      , envNumber = 0x64
      , envBytes = loaded ++ [word 0x2178 0x66, word 0x217c 0x66]
      , envClusters = [(0x66, (0, 0x900))]
      , envPaths = [(0x900, (0, matching))]
      , envCounted = [0x66]
      }
  , selectCase bytes "wide-counter" env
      { envNumber = 0
      , envBytes = loaded ++ [word 0x218c 0x67, word 0x2190 0x67, byte (0x2218 + 0x67) 40]
      , envClusters = [(0x67, (0, 0x903))]
      , envPaths = [(0x903, (0, matching))]
      }
  , selectCase bytes "marked-range-lookup-failed" env
      { envMarker = 1
      , envBytes = [word 0x2178 3, word 0x217c 4, word 0x2170 4]
      , envClusters = [(3, (0, 0x901)), (4, (9, 0))]
      , envPaths = [(0x901, (0, other))]
      }
  , selectCase bytes "unknown-directory-not-found" env
      { envBytes = [word (0x2604 + 4 * 0x66) 0xffffffff, word 0x218c 3, word 0x2190 3, word 0x2170 0x65]
      , envClusters = [(0x65, (0, 0x880))]
      , envSelected = (6, 0)
      }
  , selectCase bytes "empty-range-reload" env
      { envBytes = [word 0x218c 5, word 0x2190 4, word 0x2170 0x65, word 0x2174 12000]
      , envClusters = [(0x65, (0, 0x880))]
      , envSelected = (0, 0)
      }
  , selectCase bytes "find-failed-reload" env
      { envFind = 7
      , envBytes = [word (0x2604 + 4 * 0x66) 1, word 0x2170 0x66, word 0x2174 12]
      , envClusters = [(0x66, (0, 0x900))]
      , envPaths = [(0x900, (0, matching))]
      , envSelected = (0, 0)
      }
  ]
