{-# LANGUAGE OverloadedStrings #-}

-- | The catalog-count worker cases: an independent model of the stock workers at 0x718de4
-- (code 0x2c0e) and 0x718a8c (code 0x2c0f), read from the bytes of [0x718a8c,0x719098),
-- gives each case's result, the ordered calls into the retained path, extension, cluster and
-- position services, and the final caller words.
module Replacement.FileService.Counts
  ( code
  , scenarios
  ) where

import Data.Bits (shiftL, (.&.), (.|.))
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import Data.Word (Word32, Word8)
import Replacement.FileService.Native
import Replacement.FileService.Requests (translation)

code :: Code
code = Code
  { codeImageBase = 0x61c400
  , codeExtents =
      [ ("counts-code", Extent 0x718a8c 0x719098)
      , ("translate-code", Extent 0x71d450 0x71d66c)
      ]
  , codeStockFunctions = [(0x718a8c, 0x358), (0x718de4, 0x2b4), (0x71d450, 0x21c)]
  , codeRetained = []
  , codeRetainedFunctions = []
  }

-- Addresses ------------------------------------------------------------------

countsWorker, positionWorker, workerTable :: Word32
countsWorker = 0x718a8c
positionWorker = 0x718de4
workerTable = 0x814740

fillMemory, splitPath, extensionKey, extensionByte4, extensionByte5, directoryNumber :: Word32
fillMemory = 0x70e664
splitPath = 0x71543c
extensionKey = 0x713264
extensionByte4 = 0x713508
extensionByte5 = 0x713488
directoryNumber = 0x712dac

reservedCluster, clusterPath, textDifference, directoryPosition, fileNumber, mediaReport :: Word32
reservedCluster = 0x713d60
clusterPath = 0x713db8
textDifference = 0x70e884
directoryPosition = 0x7189b4
fileNumber = 0x713360
mediaReport = 0x726c74

callbackStub, contextSlot, contextBase, requestBase, payloadBase, callerFrame :: Word32
callbackStub = 0x380000
contextSlot = 0x910204
contextBase = 0xa00000
requestBase = 0xc10000
payloadBase = requestBase + 0x10
callerFrame = 0xc20000

-- | The context bytes the workers read, split around the unread middle.
lowBytes, highOffset, highBytes :: Int
lowBytes = 0x3d00
highOffset = 0x12700
highBytes = 0x3300

-- Environment ------------------------------------------------------------------

data Env = Env
  { envContext :: Bool
  , envEntries :: [(Word32, Word32)]
  , envMarked :: (Word32, Word32)
  , envUnmarked :: (Word32, Word32)
  , envGroups :: [(Word32, Word8)]
  , envStarts :: [(Word8, Word32)]
  , envFlags :: [Word8]
  , envPageFiles :: [(Word32, Word32)]
  , envDirectoryFiles :: [(Word32, Word32)]
  , envSplit :: Word32
  , envMarker :: Word32
  , envKey :: Word32
  , envByte4 :: Word32
  , envByte5 :: Word32
  , envNumber :: Word32
  , envClusters :: [(Word32, (Word32, Word32))]
  , envPaths :: [(Word32, (Word32, B.ByteString))]
  , envFound :: (Word32, Word32)
  , envFileNumber :: Word32
  }

env :: Env
env = Env
  { envContext = True
  , envEntries = [(0x67, 1)]
  , envMarked = (300, 302)
  , envUnmarked = (200, 201)
  , envGroups = []
  , envStarts = []
  , envFlags = []
  , envPageFiles = []
  , envDirectoryFiles = []
  , envSplit = 0
  , envMarker = 0
  , envKey = 0x5a10
  , envByte4 = 1
  , envByte5 = 0
  , envNumber = 0x10067
  , envClusters = [(0x67, (0, 0x900))]
  , envPaths = [(0x900, (0, prefixText))]
  , envFound = (0, 5)
  , envFileNumber = 30
  }

pathText, directoryText, patternText, prefixText :: B.ByteString
pathText = C.pack "DCIM\\100_FUJI\\DSCF3097.JPG"
directoryText = C.pack "100_FUJI\\\0"
patternText = C.pack "DSCF3097JPG\0"
prefixText = C.pack "DCIM\\100_FUJI\\\0"

zeros :: Int -> B.ByteString
zeros n = B.replicate n 0

pad32 :: B.ByteString -> B.ByteString
pad32 text = B.take 32 (text <> zeros 32)

put :: Int -> B.ByteString -> B.ByteString -> B.ByteString
put at value bytes = B.take at bytes <> value <> B.drop (at + B.length value) bytes

half :: Word32 -> B.ByteString
half value = B.take 2 (word32le value)

byteAt :: B.ByteString -> Int -> Word32
byteAt bytes at = fromIntegral (B.index bytes at)

halfAt :: B.ByteString -> Int -> Word32
halfAt bytes at = byteAt bytes at .|. (byteAt bytes (at + 1) `shiftL` 8)

lowRegion :: Env -> B.ByteString
lowRegion e = foldr ($) (zeros lowBytes) $
  [put (0x2604 + 4 * fromIntegral d) (word32le w) | (d, w) <- envEntries e]
    ++ [ put 0x2178 (words32le [fst (envMarked e), snd (envMarked e)])
       , put 0x218c (words32le [fst (envUnmarked e), snd (envUnmarked e)])
       ]
    ++ [put (0x35a4 + 2 * fromIntegral (d - 100)) (half n) | (d, n) <- envDirectoryFiles e]

highRegion :: Env -> B.ByteString
highRegion e = foldr ($) (zeros highBytes) $
  [put (at (0x127d6 + 2 * fromIntegral (p - 100))) (half n) | (p, n) <- envPageFiles e]
    ++ [put (at (0x12fa6 + fromIntegral n)) (B.singleton g) | (n, g) <- envGroups e]
    ++ [put (at (0x1577e + 2 * fromIntegral g)) (half n) | (g, n) <- envStarts e]
    ++ [put (at 0x15846) (B.pack (envFlags e))]
  where
    at offset = offset - highOffset

-- Model ------------------------------------------------------------------------

data Kind = Position | Counts
  deriving (Eq)

call :: Word32 -> [(String, Word32)] -> Word32 -> Service
call address inputs result = Service address inputs [("r0", result)] [] []

fill :: Service
fill = Service fillMemory [("r1", 0), ("r2", 32)] [("r0", 0)] [] [AtRegister "r0" 0 (zeros 32)]

-- | The retained calls up to the lookup and either the raw status the worker translates or
-- the directory number, the page and the position 0x7189b4 found.
locate :: Kind -> Env -> Word32 -> ([Service], Either Word32 (Word32, Word32, Word32))
locate kind e path
  | not (envContext e) = ([], Left 2)
  | envSplit e /= 0 = (parsed, Left (envSplit e))
  | envKey e == 0 = (keyed, Left 4)
  | envByte4 e == 0 = (keyed ++ [byte4], Left 4)
  | kind == Counts && envByte5 e /= 0 = (checked, Left 4)
  | windowed && first > final = (numbered, Left 7)
  | otherwise = let (walked, outcome) = walk first in (numbered ++ walked, outcome)
  where
    split =
      Service splitPath [("r0", path)] [("r0", envSplit e)] [AtIndirect "sp" 0 (zeros 32)]
        [ AtRegister "r1" 0 directoryText
        , AtRegister "r2" 0 patternText
        , AtRegister "r3" 0 (word32le (envMarker e))
        , AtIndirect "sp" 0 prefixText
        , AtIndirect "sp" 4 (word32le 0)
        ]
    parsed = [fill, split]
    keyed = parsed ++ [Service extensionKey [] [("r0", envKey e)] [AtRegister "r0" 0 (B.drop 8 patternText)] []]
    byte4 = call extensionByte4 [("r0", envKey e)] (envByte4 e)
    checked = keyed ++ [byte4] ++ [call extensionByte5 [("r0", envKey e)] (envByte5 e) | kind == Counts]
    numbered = checked ++ [Service directoryNumber [("r1", envMarker e)] [("r0", envNumber e)] [AtRegister "r0" 0 directoryText] []]
    directory = envNumber e .&. 0xffff
    entry = maybe 0 id (lookup directory (envEntries e))
    listedEntry = entry /= 0 && entry /= 0xffffffff
    (first, final, listed, windowed)
      | directory > 99 && listedEntry = (directory, directory, 1, False)
      | envMarker e /= 0 = (fst (envMarked e), snd (envMarked e), 0, True)
      | otherwise = (fst (envUnmarked e), snd (envUnmarked e), if directory /= 0 && listedEntry then 1 else 0, True)
    walk page =
      let (status, cluster) = maybe (0, 0) id (lookup page (envClusters e))
          clustered = Service reservedCluster [("r0", page)] [("r0", status)] [] [AtRegister "r1" 0 (word32le cluster)]
          (pathResult, text) = maybe (7, "") id (lookup cluster (envPaths e))
          query = pad32 text
          difference = if query == pad32 prefixText then 0 else 1
          pathed = [clustered, fill, Service clusterPath [("r0", cluster), ("r2", envMarker e), ("r3", 0)] [("r0", pathResult)] [AtRegister "r1" 0 (zeros 32)] [AtRegister "r1" 0 text | not (B.null text)]]
          compared = pathed ++ [Service textDifference [] [("r0", difference)] [AtRegister "r0" 0 query, AtRegister "r1" 0 (pad32 prefixText)] []]
          (foundStatus, position) = envFound e
          record = words32le [1, cluster, listed, 0, 0x1387, 0xffffffff, page]
          found =
            Service directoryPosition [] [("r0", foundStatus)] [AtRegister "r0" 0 record, AtIndirect "r0" 0x1c patternText]
              [AtRegister "r1" 0 (word32le (if foundStatus == 0 then position else 0))]
          next made lastStatus
            | page + 1 > final = (made, Left (if lastStatus == 0 then 7 else lastStatus))
            | otherwise = let (more, outcome) = walk (page + 1) in (made ++ more, outcome)
       in if status /= 0 || cluster == 0
            then next [clustered] status
            else
              if pathResult /= 0
                then next pathed 0
                else
                  if difference /= 0
                    then next compared 0
                    else (compared ++ [found], if foundStatus == 0 then Right (directory, page, position) else Left foundStatus)

-- | 0x71d450 with its media report, then the callback with the status.
finish :: B.ByteString -> Word32 -> Word32 -> ([Service], Word32)
finish image callback raw = (reports ++ [call callback [("r0", status)] 0 | callback /= 0], status)
  where
    (status, reported) = translation image raw
    reports = [call mediaReport [("r0", 2), ("r1", value)] 0 | Just value <- [reported]]

-- | Directory-file halves at context + 0x35a4 + 2 * (k - 100), k from 100 to below `directory`.
positionValue :: Env -> Word32 -> Word32 -> Word32
positionValue e directory position
  | directory > 100 = position + sum [halfAt low (0x35a4 + 2 * fromIntegral (k - 100)) | k <- [100 .. directory - 1]]
  | otherwise = position
  where
    low = lowRegion e

groupRank :: Env -> Word32 -> Word32
groupRank e number
  | group == 0 || start == number = 0
  | otherwise = fromIntegral (length [() | k <- [start .. number - 1], groupOf k == group])
  where
    high = highRegion e
    groupOf n = byteAt high (0x12fa6 + fromIntegral n - highOffset)
    group = groupOf number
    start = halfAt high (0x1577e + 2 * fromIntegral group - highOffset)

folded :: Env -> Word32 -> Word32 -> Word32
folded e position page = flagged - 1 + pages
  where
    high = highRegion e
    flagReads = max 1 position
    flagged = fromIntegral (length [() | i <- [0 .. flagReads - 1], let b = byteAt high (0x15846 + fromIntegral i - highOffset), b == 0 || odd b])
    pages = if page > 100 then sum [halfAt high (0x127d6 + 2 * fromIntegral (k - 100) - highOffset) | k <- [100 .. page - 1]] else 0

-- | Where a case's request sits: the header, the payload, the first and second caller words.
data Request = Request
  { requestRegionName :: String
  , requestRegionBase :: Word32
  , requestHeader :: Word32
  , requestPayload :: Word32
  , requestFirst :: Word32
  , requestSecond :: Word32
  , requestBytes :: Word32 -> B.ByteString -> B.ByteString
  }

-- | A request the media task passes: header, 0x40 payload bytes, then the two caller words.
posted :: Word32 -> Request
posted requestCode =
  Request "request" requestBase requestBase payloadBase (requestBase + 0x50) (requestBase + 0x54) $ \callback results ->
    words32le [requestCode .|. 0x65 `shiftL` 16 .|. 0x1c `shiftL` 24, 0x3c, payloadBase, 0]
      <> word32le callback
      <> B.take 48 (pathText <> zeros 48)
      <> words32le [requestBase + 0x50, requestBase + 0x54, 0]
      <> results

-- | 0x71a93c's frame from fp-0x17c: the payload, its header at +0x54 with an unset code
-- half, and the result word at +0x60.
direct :: Request
direct =
  Request "caller-frame" callerFrame (callerFrame + 0x54) callerFrame (callerFrame + 0x60) 0 $ \callback results ->
    word32le callback
      <> B.take 48 (pathText <> zeros 48)
      <> word32le (callerFrame + 0x60)
      <> B.replicate 0x1c 0xa5
      <> words32le [0xa5a5a5a5, 0xa5a5a5a5, callerFrame]
      <> B.take 4 results

workerCase :: B.ByteString -> String -> Kind -> Request -> Bool -> Word32 -> Env -> Scenario
workerCase image name kind request mediaTask callback e =
  Scenario
    { scenarioName = name
    , scenarioEntry = entry
    , scenarioData =
        [ DataRegion "context-slot" contextSlot (word32le slot) (word32le slot)
        , DataRegion "context-low" contextBase (lowRegion e) (lowRegion e)
        , DataRegion "context-high" (contextBase + fromIntegral highOffset) (highRegion e) (highRegion e)
        , DataRegion (requestRegionName request) (requestRegionBase request) (requestBytes request callback initial) (requestBytes request callback final)
        ]
    , scenarioCalls =
        [ if mediaTask
            then Call entry [requestHeader request, workerTable, requestCode, entry] [] True
            else Call entry [requestHeader request] [] True
        ]
    , scenarioServices = located ++ finished
    , scenarioResults = [if kind == Counts then 0 else status]
    , scenarioStop = Nothing
    }
  where
    (entry, requestCode) = if kind == Counts then (countsWorker, 0x2c0f) else (positionWorker, 0x2c0e)
    slot = if envContext e then contextBase else 0
    initial = words32le [0x5a5a5a5a, 0x6b6b6b6b]
    (located, outcome) = locate kind e (requestPayload request + 4)
    (stores, raw) = case outcome of
      Left failed -> ([], failed)
      Right (directory, page, position)
        | kind == Position -> ([(requestFirst request, positionValue e directory position)], 0)
        | otherwise ->
            ( [ (requestSecond request, groupRank e (envFileNumber e))
              , (requestFirst request, folded e position page)
              ]
            , 0
            )
    numbered = case outcome of
      Right _ | kind == Counts -> [Service fileNumber [] [("r0", envFileNumber e)] [AtRegister "r0" 0 patternText] []]
      _ -> []
    (reported, status) = finish image callback raw
    finished = numbered ++ reported
    final = words32le [maybe 0x5a5a5a5a id (lookup (requestFirst request) stores), maybe 0x6b6b6b6b id (lookup (requestSecond request) stores)]

-- Cases ------------------------------------------------------------------------------

walkEnv :: Env
walkEnv =
  env
    { envNumber = 50
    , envEntries = [(50, 0xffffffff)]
    , envUnmarked = (200, 204)
    , envClusters = [(200, (3, 0x901)), (201, (0, 0)), (202, (0, 0x902)), (203, (0, 0x903)), (204, (0, 0x904))]
    , envPaths = [(0x903, (0, "DCIM\\OTHER\\\0")), (0x904, (0, prefixText))]
    }

countsEnv :: Env
countsEnv =
  env
    { envFound = (0, 4)
    , envFileNumber = 30
    , envGroups = [(30, 3), (26, 3), (27, 2), (28, 3), (29, 0)]
    , envStarts = [(3, 26)]
    , envFlags = [0, 1, 2, 3, 4, 5]
    , envPageFiles = [(100, 10), (101, 20), (102, 30), (103, 0x7000)]
    }

scenarios :: B.ByteString -> [Scenario]
scenarios image =
  [ position "listed-directory" env {envDirectoryFiles = [(100, 4), (101, 6), (102, 1), (103, 0x7000)]}
  , position "unmarked-window-walk" walkEnv {envDirectoryFiles = [(100, 9)]}
  , position "listed-below-100" env {envNumber = 50, envEntries = [(50, 1)], envClusters = [(200, (0, 0x900))]}
  , position "marked-window" env {envMarker = 1, envNumber = 0x1ff, envClusters = [(301, (0, 0x900))], envDirectoryFiles = [(100, 2), (0x1fe, 3)]}
  , position "empty-window" env {envNumber = 0, envUnmarked = (5, 4)}
  , position "exhausted-status" env {envNumber = 0, envClusters = [(200, (0, 0x900)), (201, (9, 0))], envPaths = []}
  , position "exhausted-zero" env {envNumber = 0, envClusters = []}
  , position "lookup-failed" env {envFound = (13, 5)}
  , position "no-context" env {envContext = False}
  , position "split-refused" env {envSplit = 4}
  , position "no-extension" env {envKey = 0}
  , position "extension-byte4-clear" env {envByte4 = 0}
  , position "extension-byte5-set" env {envByte5 = 1}
  , workerCase image "position-no-callback" Position (posted 0x2c0e) True 0 env
  , workerCase image "position-direct-found" Position direct False 0 env {envDirectoryFiles = [(100, 4), (102, 1)]}
  , workerCase image "position-direct-not-found" Position direct False 0 env {envNumber = 0, envUnmarked = (5, 4)}
  , counts "listed-directory" countsEnv
  , counts "group-start" countsEnv {envStarts = [(3, 30)]}
  , counts "no-group" countsEnv {envGroups = []}
  , counts "single-flag-page-100" countsEnv {envNumber = 100, envEntries = [(100, 1)], envClusters = [(100, (0, 0x900))], envFound = (0, 0)}
  , counts "unmarked-window-walk" walkEnv {envFound = (0, 2), envFlags = [7, 0, 9], envPageFiles = [(100, 1), (150, 2), (203, 3)], envGroups = [(30, 1)], envStarts = [(1, 28)]}
  , counts "marked-window" countsEnv {envMarker = 1, envNumber = 0x1ff, envClusters = [(301, (0, 0x900))]}
  , counts "extension-byte5-set" countsEnv {envByte5 = 1}
  , counts "no-extension" countsEnv {envKey = 0}
  , counts "lookup-failed" countsEnv {envFound = (7, 0)}
  , counts "no-context" countsEnv {envContext = False}
  , workerCase image "counts-no-callback" Counts (posted 0x2c0f) True 0 countsEnv
  ]
  where
    position name = workerCase image ("position-" ++ name) Position (posted 0x2c0e) True callbackStub
    counts name = workerCase image ("counts-" ++ name) Counts (posted 0x2c0f) True callbackStub
