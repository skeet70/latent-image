{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}
module Main (main) where

import Control.Monad (unless)
import Data.Bits (shiftR)
import Data.Word (Word8, Word32)
import Data.IORef (IORef, atomicModifyIORef', newIORef)
import System.IO.Unsafe (unsafePerformIO)
import Data.Aeson (Value, encode, object, (.=))
import qualified Data.ByteString.Lazy as L
import qualified Data.ByteString as B
import System.Environment (getArgs)
import Replacement.Saclac.PayloadProjection

main :: IO ()
main = do
  declaration <- parse (policy [field 0 2, field 4 1] [padding 2 2])
  check (projectPayload declaration Outgoing (QueueId 47) (Operation 16166) [1,2,3,4,5] == Right [1,2,5])
  check (projectPayload declaration Outgoing (QueueId 47) (Operation 16166) [1,2,9,8,5] == Right [1,2,5])
  check (projectPayload declaration Outgoing (QueueId 47) (Operation 16166) [1,7,3,4,5] == Right [1,7,5])
  check (isLeft (projectPayload declaration Incoming (QueueId 47) (Operation 16166) [1,2,3,4,5]))
  check (isLeft (projectPayload declaration Outgoing (QueueId 50) (Operation 16166) [1,2,3,4,5]))
  check (isLeft (projectPayload declaration Outgoing (QueueId 47) (Operation 16167) [1,2,3,4,5]))
  check (isLeft (projectPayload declaration Outgoing (QueueId 47) (Operation 16166) [1,2,3,4]))
  check (isLeft (parseDeclaration (L.toStrict (encode (policy [field 0 2] [padding 3 2])))))
  check (isLeft (parseDeclaration (L.toStrict (encode (policy [field 0 3, field 2 3] [])))))
  check (isLeft (parseDeclaration (L.toStrict (encode (policy [field 0 6] [])))))
  roundTrip <- either fail pure (parseDeclaration (L.toStrict (encode (declarationJson declaration))))
  check (projectPayload roundTrip Outgoing (QueueId 47) (Operation 16166) [1,2,3,4,5] == Right [1,2,5])
  arguments <- getArgs
  source <- case arguments of
    [path] -> B.readFile path
    _ -> fail "expected payload field declaration path"
  recovered <- either fail pure (parseDeclaration source)
  let image bytes = projectPayload recovered Outgoing (QueueId 47) (Operation 0x3f26) bytes
      zeros = replicate 232 0
      changed offset = take offset zeros ++ 1 : drop (offset + 1) zeros
  check (image zeros == image (changed 94))
  check (image zeros /= image (changed 96))
  check (isLeft (image (replicate 231 0)))
  let startRecord = projectPayload recovered Outgoing (QueueId 47) (Operation 0x3f56)
      startBytes = replicate 68 0
      mutate offset bytes = take offset bytes ++ 1 : drop (offset + 1) bytes
  mapM_ (\offset -> check (startRecord startBytes == startRecord (mutate offset startBytes))) ([20..23] ++ [48..63])
  mapM_ (\offset -> check (startRecord startBytes /= startRecord (mutate offset startBytes))) ([0..19] ++ [24..47] ++ [64..67])
  check (isLeft (startRecord (replicate 67 0)))
  let updateRecord = projectPayload recovered Outgoing (QueueId 47) (Operation 0x3f57)
      updateBytes = replicate 20 0
  mapM_ (\offset -> check (updateRecord updateBytes == updateRecord (mutate offset updateBytes))) [10,11]
  mapM_ (\offset -> check (updateRecord updateBytes /= updateRecord (mutate offset updateBytes))) ([0..9] ++ [12..19])
  let captureRecord = projectPayload recovered Outgoing (QueueId 47) (Operation 0x3f49)
      captureBytes = replicate 176 0
  mapM_ (\offset -> check (captureRecord captureBytes /= captureRecord (mutate offset captureBytes))) [0..175]
  check (isLeft (captureRecord (replicate 175 0)))
  let windowRequest = projectPayload recovered Outgoing (QueueId 44) (Operation 0x3c0c)
      windowBytes = replicate 80 0
      windowUnwritten = [10,11] ++ [17..19] ++ [28..31] ++ [34,35]
  mapM_ (\offset -> check (windowRequest windowBytes == windowRequest (mutate offset windowBytes))) windowUnwritten
  mapM_ (\offset -> check (windowRequest windowBytes /= windowRequest (mutate offset windowBytes))) (filter (`notElem` windowUnwritten) [0..79])
  let sourceRequest = projectPayload recovered Outgoing (QueueId 44) (Operation 0x3c11)
      sourceBytes = replicate 56 0
      sourceUnwritten = [18,19,26,27]
  mapM_ (\offset -> check (sourceRequest sourceBytes == sourceRequest (mutate offset sourceBytes))) sourceUnwritten
  mapM_ (\offset -> check (sourceRequest sourceBytes /= sourceRequest (mutate offset sourceBytes))) (filter (`notElem` sourceUnwritten) [0..55])
  let through = projectPayload recovered Outgoing (QueueId 48) (Operation 0x4002)
      throughBytes = replicate 208 0
      nonzero = take 32 throughBytes ++ [8] ++ drop 33 throughBytes
      zeroAlternate = take 32 throughBytes ++ [1] ++ drop 33 throughBytes
      nonzeroAlternate = take 32 throughBytes ++ [9] ++ drop 33 throughBytes
  mapM_ (\offset -> check (through throughBytes == through (mutate offset throughBytes))) ([36..39] ++ [148..156] ++ [162,163,182,183])
  mapM_ (\offset -> check (through nonzero /= through (mutate offset nonzero))) [36..39]
  check (through throughBytes /= through nonzero)
  check (through throughBytes /= through (mutate 180 throughBytes))
  check (through zeroAlternate == through (mutate 36 zeroAlternate))
  check (through nonzeroAlternate /= through (mutate 36 nonzeroAlternate))
  recoveredRoundTrip <- either fail pure (parseDeclaration (L.toStrict (encode (declarationJson recovered))))
  check (projectPayload recoveredRoundTrip Outgoing (QueueId 48) (Operation 0x4002) (mutate 36 nonzero) == through (mutate 36 nonzero))
  let half = projectPayload recovered Outgoing (QueueId 44) (Operation 0x3c0c)
      halfBytes = replicate 80 0
      holes = [10,11,17,18,19,28,29,30,31,34,35]
  mapM_ (\offset -> check (half halfBytes == half (mutate offset halfBytes))) holes
  mapM_ (\offset -> check (half halfBytes /= half (mutate offset halfBytes))) (filter (`notElem` holes) [0..79])
  let record = projectPayload recovered Outgoing (QueueId 45) (Operation 0x3d00)
      liveRecord = take 168 zeros ++ [0x68,0x35,0xf3,0x00]
      s1Record = take 168 zeros ++ [0xa8,0x41,0xf2,0x00]
      s1Padding = [4,5,6,7] ++ [30,31] ++ [38,39] ++ [54,55] ++ [109] ++ [114,115]
  check (fmap length (record liveRecord) == Right 172)
  check (fmap length (record s1Record) == Right 159)
  mapM_ (\offset -> check (record liveRecord /= record (mutate offset liveRecord))) [0..171]
  mapM_ (\offset -> check (record s1Record == record (mutate offset s1Record))) s1Padding
  mapM_ (\offset -> check (record s1Record /= record (mutate offset s1Record)))
    (filter (`notElem` s1Padding) [0..171])
  check (isLeft (record (take 168 zeros ++ [0,0,0,0])))
  check (isLeft (record (take 171 zeros)))
  let command402c = projectPayload recovered Outgoing (QueueId 48) (Operation 0x402c)
      commandBytes = take 56 (replicate 60 0) ++ [0xe4,0xa7,0xf3,0x00]
      commandUnwritten = [20..28] ++ [34,35] ++ [54,55]
  mapM_ (\offset -> check (command402c commandBytes == command402c (mutate offset commandBytes))) commandUnwritten
  mapM_ (\offset -> check (command402c commandBytes /= command402c (mutate offset commandBytes)))
    (filter (`notElem` commandUnwritten) [0..59])
  check (isLeft (command402c (take 56 (replicate 60 0))))
  let alternateBytes = take 56 (replicate 60 0) ++ [0x08,0x27,0xf2,0x00]
      alternateUnwritten = [20..28] ++ [30] ++ [33,34,35] ++ [54,55]
  mapM_ (\offset -> check (command402c alternateBytes == command402c (mutate offset alternateBytes))) alternateUnwritten
  mapM_ (\offset -> check (command402c alternateBytes /= command402c (mutate offset alternateBytes)))
    (filter (`notElem` alternateUnwritten) [0..59])
  discriminating
  internalCompletions
  recoveredInternal recovered
  recoveredLayouts recovered
  putStrLn "payload field projection behavioral tests passed"
  where parse = either fail pure . parseDeclaration . L.toStrict . encode

-- A saclac-internal completion projects every payload to nothing, so a
-- comparison sees only its operation, sender and position; any other
-- address is still undeclared.
internalCompletions :: IO ()
internalCompletions = do
  declaration <- parse (internalPolicy [internal 47 0x3b4e])
  let project = projectPayload declaration Incoming (QueueId 47) (Operation 0x3b4e)
  check (project [] == Right [])
  check (project [1,2,3,4] == Right [])
  check (project [] == project [9,8,7,6,5,4,3,2,1])
  check (isLeft (projectPayload declaration Incoming (QueueId 44) (Operation 0x3b4e) []))
  check (isLeft (projectPayload declaration Incoming (QueueId 47) (Operation 0x3b4f) []))
  check (isLeft (projectPayload declaration Outgoing (QueueId 47) (Operation 0x3b4e) []))
  roundTrip <- either fail pure (parseDeclaration (L.toStrict (encode (declarationJson declaration))))
  check (projectPayload roundTrip Incoming (QueueId 47) (Operation 0x3b4e) [5,6] == Right [])
  let refused rows = check (isLeft (parseDeclaration (L.toStrict (encode (internalPolicy rows)))))
      internalWith extra = object
        ([ "direction" .= ("incoming" :: String), "queue" .= (47 :: Int), "operation" .= (0x3b4e :: Int)
         , "saclac_internal" .= object ["sources" .= (["synthetic fixture"] :: [String])] ] ++ extra)
  refused [internalWith ["bytes" .= (0 :: Int)]]
  refused [internalWith ["fields" .= ([] :: [Value])]]
  refused [internalWith ["padding" .= ([] :: [Value])]]
  refused [object
    [ "direction" .= ("outgoing" :: String), "queue" .= (47 :: Int), "operation" .= (0x3b4e :: Int)
    , "saclac_internal" .= object ["sources" .= (["synthetic fixture"] :: [String])] ]]
  refused [object
    [ "direction" .= ("incoming" :: String), "queue" .= (47 :: Int), "operation" .= (0x3b4e :: Int)
    , "saclac_internal" .= object ["sources" .= ([] :: [String])] ]]
  refused [object
    [ "direction" .= ("incoming" :: String), "queue" .= (47 :: Int), "operation" .= (0x3b4e :: Int)
    , "saclac_internal" .= object [] ]]
  refused [internal 47 0x3b4e, internal 47 0x3b4e]
  refused [internal 47 0x3b4e, object
    [ "direction" .= ("incoming" :: String), "queue" .= (47 :: Int), "operation" .= (0x3b4e :: Int)
    , "bytes" .= (0 :: Int), "fields" .= ([] :: [Value]) ]]
  where parse = either fail pure . parseDeclaration . L.toStrict . encode

internal :: Int -> Int -> Value
internal queue operation = object
  [ "direction" .= ("incoming" :: String), "queue" .= queue, "operation" .= operation
  , "saclac_internal" .= object ["sources" .= (["synthetic fixture"] :: [String])] ]

internalPolicy :: [Value] -> Value
internalPolicy entries = object
  [ "schema" .= (1 :: Int), "source_commit" .= ("synthetic-fixture" :: String), "messages" .= entries ]

-- The callback completions are declared saclac-internal for the sender
-- whose task enters the callback; another sender stays undeclared.
recoveredInternal :: Declaration -> IO ()
recoveredInternal recovered = do
  let completion queue operation = projectPayload recovered Incoming (QueueId queue) (Operation operation)
  mapM_ (\(queue, operation) -> do
      check (completion queue operation [] == Right [])
      check (completion queue operation [] == completion queue operation (replicate 36 7)))
    [ (47, 0x3b4e), (47, 0x3b50), (47, 0x3b51), (44, 0x3b91), (44, 0x3b92), (44, 0x3b93)
    , (44, 0x3b8e), (44, 0x3b8f), (47, 0x3b61), (47, 0x3b62), (44, 0x3b97), (44, 0x3b98), (44, 0x3b9a)
    , (47, 0x3b4b), (44, 0x3b8d), (45, 0x3b87), (48, 0x3b80), (50, 0x3b3c) ]
  check (isLeft (completion 44 0x3b4e []))
  check (isLeft (completion 47 0x3b91 []))
  check (projectPayload recovered Incoming (QueueId 3) (Operation 0x3b0b) [1,2,3,4] == Right [1,2,3,4])
  check (isLeft (projectPayload recovered Incoming (QueueId 3) (Operation 0x3b0b) []))

-- Every byte the stock sender stores before the post reaches the
-- projection, every byte it leaves unwritten does not, and a payload of
-- another length is refused.
layout :: ([Word8] -> Either String [Word8]) -> [Word8] -> [Int] -> IO ()
layout project base unwritten = do
  check (not (isLeft (project base)))
  mapM_ (\offset -> check (project base == project (flipByte offset base))) unwritten
  mapM_ (\offset -> check (project base /= project (flipByte offset base)))
    (filter (`notElem` unwritten) [0 .. length base - 1])
  check (isLeft (project (base ++ [0])))
  unless (null base) (check (isLeft (project (init base))))

flipByte :: Int -> [Word8] -> [Word8]
flipByte offset bytes = take offset bytes ++ (bytes !! offset + 1) : drop (offset + 1) bytes

-- Little-endian word stored at an offset of a payload.
withWord :: Int -> Word32 -> [Word8] -> [Word8]
withWord offset value bytes =
  take offset bytes ++ [fromIntegral (value `shiftR` (8 * index)) | index <- [0 .. 3]] ++ drop (offset + 4) bytes

recoveredLayouts :: Declaration -> IO ()
recoveredLayouts recovered = do
  let outgoing queue operation = projectPayload recovered Outgoing (QueueId queue) (Operation operation)
      zeros size = replicate size 0
      request queue operation size unwritten = layout (outgoing queue operation) (zeros size) unwritten
  -- calc requests (queue 44)
  request 44 0x3c03 232 [88..231]
  request 44 0x3c04 68 []
  request 44 0x3c05 120 [118,119]
  request 44 0x3c09 124 []
  request 44 0x3c0a 24 []
  request 44 0x3c0f 44 ([6,7] ++ [28..43])
  request 44 0x3c10 224 ([26,27,38,39,62,63] ++ [144..151] ++ [183])
  request 44 0x3c12 0 []
  -- 0x3c0d copies 8 and 4 bytes per area, area count at 143
  let window = outgoing 44 0x3c0d
      areas count = take 143 (zeros 220) ++ [count] ++ drop 144 (zeros 220)
      areaBytes index = [144 + 8 * index .. 151 + 8 * index] ++ [168 + 4 * index .. 171 + 4 * index]
      unwrittenFor count = [74,75,182,183,215] ++ concatMap areaBytes [count .. 2]
  layout window (areas 0) (unwrittenFor 0)
  layout window (areas 1) (unwrittenFor 1)
  layout window (areas 3) (unwrittenFor 3)
  -- image processor requests (queue 47)
  request 47 0x3f34 20 []
  request 47 0x3f46 116 [34,35,70,71,114,115]
  request 47 0x3f47 8 []
  request 47 0x3f4e 20 []
  request 47 0x3f1d 52 [39]
  request 47 0x3f1e 56 [14,15,49,50,51]
  request 47 0x3f1f 128 []
  request 47 0x3f20 8 [5,6,7]
  request 47 0x3f30 20 []
  request 47 0x3f31 4 []
  request 47 0x3f32 4 []
  request 47 0x3f33 16 [10,11]
  request 47 0x3f2e 44 ([18,19] ++ [40..43])
  request 47 0x3f44 24 [6,7]
  request 47 0x3f42 136 ([6,7,27,82,83] ++ [93..95])
  mapM_ (\operation -> request 47 operation 0 []) [0x3f21, 0x3f2f, 0x3f3b, 0x3f45]
  mapM_ (\operation -> request 47 operation 4 []) [0x3f22, 0x3f23, 0x3f28]
  mapM_ (\operation -> request 47 operation 8 []) [0x3f18, 0x3f29, 0x3f2a]
  request 47 0x3f2d 44 []
  request 47 0x3f3a 40 [9,10,11]
  -- lens requests (queue 50)
  mapM_ (\operation -> request 50 operation 12 []) [0x4204, 0x4205, 0x4206, 0x4208]
  request 50 0x4207 24 []
  request 50 0x4211 16 []
  request 50 0x4209 0 []
  request 50 0x4212 0 []
  -- statistics requests (queue 48)
  let familyGaps = [127] ++ [148..156] ++ [162,163,182,183]
      setupGaps = [127] ++ [144..156] ++ [162,163,182,183]
  request 48 0x4003 176 ([107] ++ [124..136] ++ [142..159] ++ [162,163])
  request 48 0x4004 208 ([36..39] ++ setupGaps)
  request 48 0x4002 208 ([36..39] ++ familyGaps)
  let selected selector = take 32 (zeros 248) ++ [selector] ++ drop 33 (zeros 248)
      buffered = outgoing 48 0x4008
      bufferedGaps = familyGaps ++ [200..203] ++ [208..223]
  layout buffered (selected 0) ([36..39] ++ bufferedGaps)
  layout buffered (selected 8) bufferedGaps
  let cyclic = withWord 200 0x00f3aa18 (zeros 204)
      startS1 = withWord 200 0x00f227f8 (zeros 204)
      repeated = outgoing 48 0x4006
  layout repeated cyclic ([36..39] ++ familyGaps)
  layout repeated (take 32 cyclic ++ [9] ++ drop 33 cyclic) familyGaps
  layout repeated (withWord 132 1 startS1) setupGaps
  layout repeated (withWord 132 2 startS1) ([36..39] ++ setupGaps)
  check (isLeft (repeated (zeros 204)))
  request 48 0x4009 8 []
  request 48 0x4014 12 []
  request 48 0x4041 12 [5,6,7]
  request 48 0x400a 24 []
  request 48 0x400c 0 []
  request 48 0x4017 92 [91]
  request 48 0x4013 20 []
  request 48 0x4011 20 []
  layout (outgoing 48 0x4010) (zeros 28) [20..23]
  layout (outgoing 48 0x4010) (take 16 (zeros 28) ++ [8] ++ drop 17 (zeros 28)) []
  request 48 0x4012 60 [55]
  request 48 0x402a 20 []
  request 48 0x402e 28 []
  request 48 0x400b 4 []
  request 48 0x400e 32 []
  request 48 0x400f 4 []
  request 48 0x4043 0 []

-- Two senders of one operation are separated by the word each copies at the
-- same offset, and a payload matching neither is refused.
discriminating :: IO ()
discriminating = do
  declaration <- parse (twoSenders
    [ sender 0 0x01020304 [field 0 4, field 4 1] [padding 5 3]
    , sender 0 0x01020305 [field 0 4] [padding 4 4] ])
  let project = projectPayload declaration Outgoing (QueueId 47) (Operation 16166)
  check (project [4,3,2,1,9,0,0,0] == Right [4,3,2,1,9])
  check (project [5,3,2,1,9,0,0,0] == Right [5,3,2,1])
  check (isLeft (project [6,3,2,1,9,0,0,0]))
  check (isLeft (project [4,3,2,1,9,0,0]))
  check (isLeft (project [4,3,2,1,9,0,0,0,0]))
  roundTrip <- either fail pure (parseDeclaration (L.toStrict (encode (declarationJson declaration))))
  check (projectPayload roundTrip Outgoing (QueueId 47) (Operation 16166) [5,3,2,1,9,0,0,0] == Right [5,3,2,1])
  check (isLeft (parseDeclaration (L.toStrict (encode (twoSenders
    [sender 0 0x01020304 [field 0 4] [padding 4 4], sender 0 0x01020304 [field 0 4] [padding 4 4]])))))
  check (isLeft (parseDeclaration (L.toStrict (encode (twoSenders
    [sender 0 0x01020304 [field 0 4] [padding 4 4], plain [field 0 4] [padding 4 4]])))))
  check (isLeft (parseDeclaration (L.toStrict (encode (twoSenders
    [sender 5 0x01020304 [field 0 4] [padding 4 4]])))))
  check (isLeft (parseDeclaration (L.toStrict (encode (twoSenders
    [sender 7 0x01020304 [field 0 4] [padding 4 4]])))))
  where parse = either fail pure . parseDeclaration . L.toStrict . encode

twoSenders :: [Value] -> Value
twoSenders entries = object
  [ "schema" .= (1 :: Int), "source_commit" .= ("synthetic-fixture" :: String)
  , "messages" .= entries
  ]

sender :: Int -> Int -> [Value] -> [Value] -> Value
sender discriminatorOffset value fields paddingRows = object
  [ "direction" .= ("outgoing" :: String), "queue" .= (47 :: Int), "operation" .= (16166 :: Int)
  , "bytes" .= (8 :: Int), "fields" .= fields, "padding" .= paddingRows
  , "discriminator" .= object ["offset" .= discriminatorOffset, "value" .= value]
  ]

plain :: [Value] -> [Value] -> Value
plain fields paddingRows = object
  [ "direction" .= ("outgoing" :: String), "queue" .= (47 :: Int), "operation" .= (16166 :: Int)
  , "bytes" .= (8 :: Int), "fields" .= fields, "padding" .= paddingRows
  ]

policy :: [Value] -> [Value] -> Value
policy fields paddingRows = object
  [ "schema" .= (1 :: Int), "source_commit" .= ("synthetic-fixture" :: String)
  , "messages" .= [object
      [ "direction" .= ("outgoing" :: String), "queue" .= (47 :: Int), "operation" .= (16166 :: Int)
      , "bytes" .= (5 :: Int), "fields" .= fields, "padding" .= paddingRows
      ]]
  ]

field :: Int -> Int -> Value
field offset width = object ["name" .= ("synthetic-field" :: String), "offset" .= offset, "bytes" .= width, "sources" .= (["synthetic fixture"] :: [String])]

padding :: Int -> Int -> Value
padding offset width = object ["offset" .= offset, "bytes" .= width, "sources" .= (["synthetic fixture"] :: [String])]

isLeft :: Either a b -> Bool
isLeft (Left _) = True
isLeft (Right _) = False

check :: Bool -> IO ()
check yes = do
  index <- atomicModifyIORef' checkIndex (\value -> (value + 1, value))
  unless yes (fail ("payload projection behavior differs at check " ++ show index))

{-# NOINLINE checkIndex #-}
checkIndex :: IORef Int
checkIndex = unsafePerformIO (newIORef 0)
