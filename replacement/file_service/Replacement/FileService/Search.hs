{-# LANGUAGE OverloadedStrings #-}

-- | The directory search cases: an independent model of the stock find first (0x712700), find
-- next (0x7127e0), entry-record first (0x71e3a4) and entry-record next (0x71e550) workers,
-- read from their bytes, gives each case's result, the ordered calls into the retained search,
-- name test, divide and time-format services, and the caller's final output bytes.
module Replacement.FileService.Search
  ( code
  , scenarios
  ) where

import Data.Bits (shiftL, (.|.))
import qualified Data.ByteString as B
import Data.Word (Word32, Word8)
import Replacement.FileService.Native
import Replacement.FileService.Requests (translation)

code :: Code
code = Code
  { codeImageBase = 0x61c400
  , codeExtents =
      [ ("search-code", Extent 0x712700 0x712890)
      , ("translate-code", Extent 0x71d450 0x71d66c)
      , ("entry-code", Extent 0x71e3a4 0x71e6f0)
      ]
  , codeStockFunctions = [(0x712700, 0xe0), (0x7127e0, 0xb0), (0x71d450, 0x21c), (0x71e3a4, 0x1ac), (0x71e550, 0x1a0)]
  , codeRetained = [("memory-code", Extent 0x70e650 0x70e748)]
  , codeRetainedFunctions = [(0x70e650, 0x14), (0x70e664, 0x60), (0x70e6c4, 0x84)]
  }

-- Addresses ------------------------------------------------------------------

findFirstWorker, findNextWorker, entryFirstWorker, entryNextWorker :: Word32
findFirstWorker = 0x712700
findNextWorker = 0x7127e0
entryFirstWorker = 0x71e3a4
entryNextWorker = 0x71e550

findFirst, findNext, findFirstEntry, findNextEntry, nameTest, divide, formatTime, mediaReport :: Word32
findFirst = 0x7008a4
findNext = 0x700b10
findFirstEntry = 0x7004b0
findNextEntry = 0x700768
nameTest = 0x7131a0
divide = 0x4d978
formatTime = 0x71d214
mediaReport = 0x726c74

callbackStub, contextPointer, contextBase, requestBase, payloadBase, outputBase, workerTable :: Word32
callbackStub = 0x380000
contextPointer = 0x910204
contextBase = 0xa00000
requestBase = 0xc10000
payloadBase = requestBase + 0x10
outputBase = 0xc20000
workerTable = 0x814740

-- | The context word the entry-record workers round the size up to a multiple of.
roundingOffset :: Word32
roundingOffset = 0x16bf8

outputBytes :: Int
outputBytes = 0x80

-- Model ------------------------------------------------------------------------

-- | One scripted search: the raw result, the 80 bytes it leaves in the caller's buffer and
-- 0x7131a0's answer for them.
data Answer = Answer Word32 B.ByteString Word32

data Env = Env
  { envContext :: Bool
  , envRounding :: Word32
  , envCallback :: Word32
  , envFlag :: Word32
  , envAnswers :: [Answer]
  }

data Kind = FindFirst | FindNext | EntryFirst | EntryNext
  deriving (Eq)

wordAt :: B.ByteString -> Int -> Word32
wordAt bytes offset = sum [fromIntegral (B.index bytes (offset + index)) `shiftL` (8 * index) | index <- [0 .. 3]]

slice :: Int -> Int -> B.ByteString -> B.ByteString
slice offset count = B.take count . B.drop offset

put :: Int -> B.ByteString -> B.ByteString -> B.ByteString
put offset value bytes = B.take offset bytes <> value <> B.drop (offset + B.length value) bytes

zeros :: B.ByteString
zeros = B.replicate 80 0

path, pattern :: B.ByteString
path = "B:/DCIM/100_FUJI"
pattern = "*.JPG"

payloadFor :: Env -> B.ByteString
payloadFor e =
  put 0x38 (word32le (envFlag e)) (put 0x28 pattern (put 8 path (put 4 (word32le outputBase) (put 0 (word32le (envCallback e)) (B.replicate 0x40 0)))))

searchCall :: Kind -> Env -> Answer -> Service
searchCall kind e (Answer raw entry _) = case kind of
  FindFirst ->
    Service findFirst [("r0", payloadBase + 8), ("r1", payloadBase + 0x28)] [("r0", raw)]
      [AtRegister "r0" 0 (slice 8 32 payload), AtRegister "r1" 0 (slice 0x28 16 payload), AtRegister "r2" 0 zeros] [AtRegister "r2" 0 entry]
  EntryFirst ->
    Service findFirstEntry [("r0", payloadBase + 8), ("r1", payloadBase + 0x28), ("r3", envFlag e)] [("r0", raw)]
      [AtRegister "r0" 0 (slice 8 32 payload), AtRegister "r1" 0 (slice 0x28 16 payload), AtRegister "r2" 0 zeros] [AtRegister "r2" 0 entry]
  FindNext -> Service findNext [] [("r0", raw)] [AtRegister "r0" 0 zeros] [AtRegister "r0" 0 entry]
  EntryNext -> Service findNextEntry [] [("r0", raw)] [AtRegister "r0" 0 zeros] [AtRegister "r0" 0 entry]
  where
    payload = payloadFor e

-- | 0x712700 and 0x7127e0: each pass zeroes 100 output bytes and the 80-byte buffer, searches,
-- and on raw 0 copies 48 buffer bytes out unless 0x7131a0(buffer + 0x30, 11) is nonzero.
search :: Env -> Kind -> [Answer] -> B.ByteString -> (Word32, B.ByteString, [Service])
search _ _ [] _ = error "a search case ran out of scripted answers"
search e kind (answer@(Answer raw entry rejected) : rest) output
  | raw /= 0 = (raw, cleared, [call])
  | rejected /= 0 = let (result, final, calls) = search e FindNext rest cleared in (result, final, call : test : calls)
  | otherwise = (0, put 0 (B.take 48 entry) cleared, [call, test])
  where
    cleared = put 0 (B.replicate 100 0) output
    call = searchCall kind e answer
    test = Service nameTest [("r1", 11)] [("r0", rejected)] [AtRegister "r0" 0 (slice 0x30 11 entry)] []

-- | The text 0x71d214 leaves at its destination in these cases.
timeText :: Word8 -> B.ByteString
timeText mark = B.replicate 20 mark

-- | 0x71e3a4 and 0x71e550: one search; on raw 0 the record takes 48 buffer bytes, byte +0x30 =
-- buffer +0x3b, +0x34 = buffer word +0x4c, +0x38 = that size rounded up to the context's
-- multiple through 0x4d978, and 0x71d214 fills +0x3c from buffer +0x3e with byte +0x3d and
-- +0x50 from buffer +0x46 with 0.
entrySearch :: Env -> Kind -> B.ByteString -> (Word32, B.ByteString, [Service])
entrySearch e kind output = case envAnswers e of
  answer@(Answer raw entry _) : _
    | raw /= 0 -> (raw, output, [searchCall kind e answer])
    | otherwise ->
        let size = wordAt entry 0x4c
            unit = envRounding e
            dividend = unit - 1 + size
            quotient = dividend `div` unit
            record =
              put 0x50 (timeText 0x4d) . put 0x3c (timeText 0x43) . put 0x38 (word32le (unit * quotient))
                . put 0x34 (word32le size) . put 0x30 (B.singleton (B.index entry 0x3b)) . put 0 (B.take 48 entry)
         in ( 0
            , record output
            , [ searchCall kind e answer
              , Service divide [("r0", dividend), ("r1", unit)] [("r0", quotient)] [] []
              , Service formatTime [("r0", outputBase + 0x3c), ("r2", fromIntegral (B.index entry 0x3d))] [("r0", 0)]
                  [AtRegister "r1" 0 (slice 0x3e 4 entry)] [AtRegister "r0" 0 (timeText 0x43)]
              , Service formatTime [("r0", outputBase + 0x50), ("r2", 0)] [("r0", 0)]
                  [AtRegister "r1" 0 (slice 0x46 4 entry)] [AtRegister "r0" 0 (timeText 0x4d)]
              ]
            )
  [] -> error "an entry-record case has no scripted answer"

-- | Every worker reads the callback first, gives raw 2 without a context, and delivers
-- 0x71d450(code, raw) to the callback and as its result.
worker :: B.ByteString -> Kind -> Env -> B.ByteString -> (Word32, B.ByteString, [Service])
worker image kind e output = (status, final, calls ++ reports ++ callback)
  where
    (raw, final, calls)
      | not (envContext e) = (2, output, [])
      | kind `elem` [FindFirst, FindNext] = search e kind (envAnswers e) output
      | otherwise = entrySearch e kind output
    (status, reported) = translation image raw
    reports = [Service mediaReport [("r0", 2), ("r1", value)] [("r0", 0)] [] [] | Just value <- [reported]]
    callback = [Service (envCallback e) [("r0", status)] [("r0", 0)] [] [] | envCallback e /= 0]

-- Cases ------------------------------------------------------------------------------

entryBytes :: B.ByteString -> B.ByteString -> Word8 -> Word8 -> Word32 -> B.ByteString
entryBytes long short attribute mark size =
  B.take 0x30 (long <> B.replicate 0x30 0) <> B.take 11 (short <> B.replicate 11 0x20)
    <> B.pack [attribute, 0, mark] <> word32le 0x5b756400 <> B.pack [0x11, 0x22, 0x33, 0x44]
    <> word32le 0x5b768a10 <> B.pack [0x55, 0x66] <> word32le size

found :: B.ByteString -> B.ByteString -> Answer
found long short = Answer 0 (entryBytes long short 0x20 150 0x12345) 0

rejectedEntry :: B.ByteString -> Answer
rejectedEntry long = Answer 0 (entryBytes long (B.pack [0x83, 0x41, 0x83, 0x43, 0x4a, 0x50, 0x47]) 0x20 3 0x400) 1

failed :: Word32 -> Answer
failed raw = Answer raw (entryBytes "PARTIAL" "PARTIAL" 0x10 7 7) 0

env :: Env
env = Env {envContext = True, envRounding = 0x8000, envCallback = callbackStub, envFlag = 1, envAnswers = []}

requestRegion :: Word32 -> B.ByteString -> B.ByteString
requestRegion requestCode payload =
  words32le [requestCode .|. 0x65 `shiftL` 16 .|. 0x1c `shiftL` 24, fromIntegral (B.length payload), payloadBase, 0] <> payload

workerCase :: B.ByteString -> String -> Kind -> Env -> Scenario
workerCase image name kind e =
  Scenario
    { scenarioName = "search-" ++ name
    , scenarioEntry = entry
    , scenarioData =
        [ DataRegion "context-pointer" contextPointer (word32le context) (word32le context)
        , DataRegion "request" requestBase request request
        , DataRegion "output" outputBase initial final
        ]
          ++ [DataRegion "rounding" (contextBase + roundingOffset) (word32le (envRounding e)) (word32le (envRounding e)) | envContext e]
    , scenarioCalls = [Call entry [requestBase, workerTable, requestCode, entry] [] True]
    , scenarioServices = services
    , scenarioResults = [status]
    , scenarioStop = Nothing
    }
  where
    (entry, requestCode) = case kind of
      FindFirst -> (findFirstWorker, 0x2c08)
      FindNext -> (findNextWorker, 0x2c09)
      EntryFirst -> (entryFirstWorker, 0x2c06)
      EntryNext -> (entryNextWorker, 0x2c07)
    context = if envContext e then contextBase else 0
    request = requestRegion requestCode (payloadFor e)
    initial = B.replicate outputBytes 0xee
    (status, final, services) = worker image kind e initial

scenarios :: B.ByteString -> [Scenario]
scenarios image =
  [ case' "find-first-accepted" FindFirst env {envAnswers = [found "DSCF0001.JPG" "DSCF0001JPG"]}
  , case' "find-first-skips-rejected" FindFirst env {envAnswers = [rejectedEntry "KANA.JPG", found "DSCF0002.JPG" "DSCF0002JPG"]}
  , case' "find-first-skips-then-ends" FindFirst env {envAnswers = [rejectedEntry "KANA.JPG", rejectedEntry "KANA2.JPG", failed 3]}
  , case' "find-first-failed-reported" FindFirst env {envAnswers = [failed 9]}
  , case' "find-first-silent" FindFirst env {envCallback = 0, envAnswers = [found "DSCF0003.JPG" "DSCF0003JPG"]}
  , case' "find-first-no-context" FindFirst env {envContext = False}
  , case' "find-next-accepted" FindNext env {envAnswers = [found "DSCF0004.JPG" "DSCF0004JPG"]}
  , case' "find-next-skips-rejected" FindNext env {envAnswers = [rejectedEntry "KANA.JPG", found "DSCF0005.JPG" "DSCF0005JPG"]}
  , case' "find-next-ends" FindNext env {envAnswers = [failed 3]}
  , case' "find-next-no-context" FindNext env {envContext = False}
  , case' "entry-first-found" EntryFirst env {envAnswers = [found "DSCF0006.JPG" "DSCF0006JPG"]}
  , case' "entry-first-empty-file" EntryFirst env {envFlag = 0, envAnswers = [Answer 0 (entryBytes "EMPTY.TXT" "EMPTY   TXT" 0x21 99 0) 0]}
  , case' "entry-first-whole-units" EntryFirst env {envRounding = 0x4000, envAnswers = [Answer 0 (entryBytes "DIR100" "100_FUJI   " 0x10 0 0x10000) 0]}
  , case' "entry-first-failed" EntryFirst env {envAnswers = [failed 3]}
  , case' "entry-first-no-context" EntryFirst env {envContext = False}
  , case' "entry-next-found" EntryNext env {envAnswers = [found "DSCF0007.RAF" "DSCF0007RAF"]}
  , case' "entry-next-failed-reported" EntryNext env {envAnswers = [failed 9]}
  , case' "entry-next-no-context" EntryNext env {envContext = False}
  ]
  where
    case' = workerCase image
