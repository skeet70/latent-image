{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}
-- | Per-cycle stock-step comparison for Root (issue 117).
--
-- The library holds the parts a host test can judge without private captures: the
-- census/wake-dump pairing, the declared retained state, the native case plan and its
-- JSON, and the byte comparison. The main program owns the runner and the dumps.
module Replacement.Saclac.CycleStep
  ( -- * Captured inputs
    Dump (..)
  , parseDumps
  , Event (..)
  , TimedEvent (..)
  , parseEvents
    -- * Cycles
  , Cycle (..)
  , Refusal (..)
  , pairCycles
  , cycleBoundary
    -- * State
  , State (..)
  , Segment (..)
  , splitState
  , joinState
  , sameLayout
  , stateSlice
  , stateWord
  , stateHolds
  , overlayState
  , overlayStates
  , netChanged
    -- * Spans and comparison
  , Span (..)
  , spanAt
  , spanBytes
  , compareState
  , Difference (..)
    -- * Windows
  , Window (..)
  , windowContains
  , windowExtend
  , windowsBytes
  , mergeWindows
    -- * Native case
  , RegionPlan (..)
  , StubPlan (..)
  , StubCall (..)
  , CasePlan (..)
  , caseValue
  , trampoline
  , loopFrame
  , findLoopStack
  , hexValue
  , hexTextOf
  , hexBytes
  , stackLow, stackHigh
  , entryAddress, postAddress, receiveAddress, taskIdAddress
  , recordBase, recordSize, trampolineBase, trampolineSize
  , levelAddress, decadeAddress
  , groupBase
  ) where

import Data.Aeson (Value (..))
import qualified Data.Set as Set
import qualified Data.ByteString as B
import Data.Bits (shiftL, shiftR, (.|.))
import Data.List (sortOn)
import Data.Word (Word32, Word8)
import Emulation.Sha256 (sha256, sha256Hex)
import FujiAF.Host (arr, get, list, n, num, obj, s, str)
import Numeric (showHex)

-- ---------------------------------------------------------------------------
-- Captured inputs

-- | One row of @RESULT\/ROLE.json@'s @diagnostic.dumps@, with the ranges its plan declares,
-- concatenated in that order in the dump file.
data Dump = Dump
  { dumpLabel :: String
  , dumpIndex :: Int
  , dumpInstructions :: Integer
  , dumpPath :: FilePath
  , dumpByteCount :: Integer
  , dumpSha256 :: String
  , dumpRanges :: [Span]
  }
  deriving (Eq, Show)

parseDumps :: Value -> [Dump]
parseDumps value =
  [ Dump label (fromIntegral (num (get row "index"))) (num (get row "instructions"))
      (str (get row "file")) (num (get row "bytes")) (str (get row "sha256")) (rangesOf label)
  | row <- list (get diagnostic "dumps")
  , let label = str (get row "label")
  ]
  where
    diagnostic = get value "diagnostic"
    rangesOf label =
      [ Span (addressOf (get range "start")) (addressOf (get range "end"))
      | plan <- list (get diagnostic "dump_plans")
      , str (get plan "label") == label
      , range <- list (get plan "ranges")
      ]
    addressOf v = case v of Number _ -> num v; _ -> parseHex (str v)

-- | The census interface events this comparison reads.
data Event
  = EventQueueIn Int Int [Word8]
  | EventPeerRequest Int Int [Word8]
  | EventCallback Integer
  | EventClientReply Integer Integer
  | EventStore Integer [Word8]
  | EventOther String
  deriving (Eq, Show)

data TimedEvent = TimedEvent {eventInstructions :: Integer, eventEvent :: Event} deriving (Eq, Show)

parseEvents :: Value -> [TimedEvent]
parseEvents value =
  [ TimedEvent (num (get row "instructions")) (parseEvent (get row "event"))
  | row <- list (get (get value "interface") "events")
  ]

parseEvent :: Value -> Event
parseEvent event = case str (get event "kind") of
  "queue-in" -> EventQueueIn (fromIntegral (num (get event "operation"))) (fromIntegral (num (get event "sender"))) (payload (get event "payload"))
  "peer-request" -> EventPeerRequest (fromIntegral (num (get event "queue"))) (fromIntegral (num (get event "operation"))) (payload (get event "payload"))
  "callback" -> EventCallback (addressOf (get event "entry"))
  "client-reply" -> EventClientReply (addressOf (get event "entry")) (addressOf (get event "target"))
  "store" -> EventStore (addressOf (get event "address")) (payload (get event "bytes"))
  other -> EventOther other
  where
    payload = map (fromIntegral . num) . list
    addressOf v = case v of Number _ -> num v; _ -> parseHex (str v)

parseHex :: String -> Integer
parseHex text = case text of
  '0' : 'x' : digits -> foldl' (\acc c -> acc * 16 + digit c) 0 digits
  _ -> 0
  where
    digit c
      | c >= '0' && c <= '9' = fromIntegral (fromEnum c - fromEnum '0')
      | c >= 'a' && c <= 'f' = fromIntegral (fromEnum c - fromEnum 'a' + 10)
      | otherwise = 0

-- ---------------------------------------------------------------------------
-- Cycle pairing

data Cycle = Cycle
  { cycleWake :: Dump
  , cycleFrame :: Dump
  , cycleCompletion :: Dump
  , cycleWakeEvent :: TimedEvent
  , cyclePosts :: [TimedEvent]
  , cycleOthers :: [TimedEvent]
  }
  deriving (Eq, Show)

data Refusal = Refusal {refusalWhere :: String, refusalReason :: String} deriving (Eq, Show)

-- | The census instruction a cycle's step ends at: the completion dump's own instruction.
-- The dump is taken at the window callback in every lane, before the callback posts the
-- 0x3b8d completion, so it is the state instant and not a kernel post.
cycleBoundary :: Cycle -> Integer
cycleBoundary = dumpInstructions . cycleCompletion

wakeOperation :: Int
wakeOperation = 0x3b4b

isWake :: TimedEvent -> Bool
isWake (TimedEvent _ (EventQueueIn operation _ _)) = operation == wakeOperation
isWake _ = False

-- | Pairs each wake dump with the frame dump and window-completion dump that follow it,
-- refusing a wake the next wake dump supersedes, a dump without its wake, and a window
-- whose census events do not bracket the dumps. Dump order is the plan's execution order.
pairCycles :: [Dump] -> [TimedEvent] -> ([Cycle], [Refusal])
pairCycles dumps events = go Nothing [] [] ordered
  where
    ordered = sortOn (\d -> (dumpInstructions d, rank (dumpLabel d))) dumps
    rank "wake" = 0 :: Int
    rank "frame" = 1
    rank "window-completion" = 2
    rank _ = 3
    wakeEvents = [t | t <- events, isWake t]
    go pending cycles refusals [] = case pending of
      Nothing -> (reverse cycles, reverse refusals)
      Just (wake, frame) ->
        ( reverse cycles
        , reverse (Refusal (label wake) (if frame == Nothing then "wake without a frame dump before the observation ended" else "wake without a window-completion dump before the observation ended") : refusals)
        )
    go pending cycles refusals (dump : rest) = case dumpLabel dump of
      "wake" ->
        let refusals' = case pending of
              Just (wake, _) -> Refusal (label wake) "wake superseded by the next wake dump before a window completion" : refusals
              Nothing -> refusals
         in go (Just (dump, Nothing)) cycles refusals' rest
      "frame" -> case pending of
        Just (wake, Nothing) -> go (Just (wake, Just dump)) cycles refusals rest
        Just (wake, Just _) -> go pending cycles (Refusal (label wake) "second frame dump before the window completion" : refusals) rest
        Nothing -> go pending cycles (Refusal (label dump) "frame dump without a preceding wake dump" : refusals) rest
      "window-completion" -> case pending of
        Just (wake, Just frame) ->
          case qualify wake frame dump of
            Left refusal -> go Nothing cycles (refusal : refusals) rest
            Right paired -> go Nothing (paired : cycles) refusals rest
        Just (wake, Nothing) -> go Nothing cycles (Refusal (label wake) "window completion without a frame dump" : refusals) rest
        Nothing -> go Nothing cycles (Refusal (label dump) "window completion without a preceding wake dump" : refusals) rest
      other -> go pending cycles (Refusal (label dump) ("unknown dump label " ++ other) : refusals) rest
    qualify wake frame completion = do
      wakeEvent <- case [t | t <- wakeEvents, near (eventInstructions t) (dumpInstructions wake)] of
        [] -> Left (Refusal (label wake) "no 0x3b4b queue-in event at the wake dump's instruction")
        found -> Right (last (sortOn eventInstructions found))
      let end = dumpInstructions completion
      if not (eventInstructions wakeEvent < dumpInstructions frame && dumpInstructions frame < end)
        then Left (Refusal (label wake) "dump order is not wake < frame < window completion")
        else case [t | t <- wakeEvents, eventInstructions wakeEvent < eventInstructions t, eventInstructions t < end] of
          _ : _ -> Left (Refusal (label wake) "a later 0x3b4b wake precedes this window completion")
          [] ->
            let window = [t | t <- events, eventInstructions wakeEvent < eventInstructions t, eventInstructions t < end]
                posts = [t | t <- window, isPeer t]
                others = [t | t <- window, not (isPeer t)]
             in Right Cycle
                  { cycleWake = wake
                  , cycleFrame = frame
                  , cycleCompletion = completion
                  , cycleWakeEvent = wakeEvent
                  , cyclePosts = posts
                  , cycleOthers = others
                  }
    isPeer (TimedEvent _ EventPeerRequest {}) = True
    isPeer _ = False
    near a b = abs (a - b) <= 2
    label dump = dumpLabel dump ++ "-" ++ show (dumpIndex dump)

-- ---------------------------------------------------------------------------
-- Retained state: the ranges a dump declares, one segment each.

-- | The low RAM range holding the task-43 stack.
stackLow, stackHigh :: Integer
stackLow = 0x00900000
stackHigh = 0x00b00000

-- | The task-43 loop's stack frame signature: r5, r6 and r7 pushed at entry.
loopFrame :: [Word32]
loopFrame = [0x0169d370, 0x0169d380, 0x0169d374]

-- | The stack word of the loop frame: r5, r6 and r7 are the three constants the loop keeps,
-- pushed at task entry and preserved across the kernel receive. Exactly one eight-byte
-- aligned address in the dumped low range may hold them.
findLoopStack :: State -> Either String Word32
findLoopStack state = case matches of
  [sp] -> Right (fromIntegral sp)
  [] -> Left "no task-43 loop frame signature (r5=0x169d370, r6=0x169d380, r7=0x169d374) in the dumped stack range"
  found -> Left ("ambiguous task-43 loop frame signature at " ++ unwords (map hexTextOf found) ++ " (" ++ show (length found) ++ " candidates)")
  where
    signature = B.concat (map (leWord . fromIntegral) loopFrame)
    matches =
      [ sp
      | sp <- [stackLow + 8, stackLow + 16 .. stackHigh - 32]
      , frameAt sp
      ]
    frameAt sp = stateSlice state (sp + 4) 12 == Just signature

leWord :: Integer -> B.ByteString
leWord value = B.pack [fromIntegral (value `shiftR` shift) | shift <- [0, 8, 16, 24]]

entryAddress, postAddress, receiveAddress, taskIdAddress :: Word32
entryAddress = 0x00f01be4
postAddress = 0x0072503c
receiveAddress = 0x00725318
taskIdAddress = 0x00723cb8

recordBase :: Word32
recordBase = 0x00500000
recordSize :: Int
recordSize = 0x1000

trampolineBase :: Word32
trampolineBase = postAddress
trampolineSize :: Int
trampolineSize = 0x40

-- | The group-27 shared level and decade words.
levelAddress, decadeAddress :: Integer
levelAddress = 0x00919758
decadeAddress = 0x00919760

groupBase :: Integer
groupBase = 0x0091904c

data Segment = Segment {segmentBase :: Integer, segmentBytes :: B.ByteString} deriving (Eq, Show)

newtype State = State {stateSegments :: [Segment]} deriving (Eq, Show)

-- | Splits a dump into its declared ranges.
splitState :: [Span] -> B.ByteString -> Either String State
splitState spans bytes
  | null spans = Left "a dump declares no ranges"
  | fromIntegral (B.length bytes) /= sum (map spanBytes spans) =
      Left ("a dump of " ++ show (B.length bytes) ++ " bytes does not fill its declared ranges of " ++ show (sum (map spanBytes spans)) ++ " bytes")
  | otherwise = Right (State (go spans bytes))
  where
    go [] _ = []
    go (span' : rest) remaining =
      let (here, later) = B.splitAt (fromIntegral (spanBytes span')) remaining
       in Segment (spanLow span') here : go rest later

joinState :: State -> B.ByteString
joinState = B.concat . map segmentBytes . stateSegments

-- | Two states hold the same ranges in the same order.
sameLayout :: State -> State -> Bool
sameLayout left right = shape left == shape right
  where
    shape = map (\segment -> (segmentBase segment, B.length (segmentBytes segment))) . stateSegments

segmentOf :: State -> Integer -> Int -> Maybe (Int, Segment)
segmentOf state address count =
  case [ (index, segment)
       | (index, segment) <- zip [0 ..] (stateSegments state)
       , address >= segmentBase segment
       , address + fromIntegral count <= segmentBase segment + fromIntegral (B.length (segmentBytes segment))
       ] of
    found : _ -> Just found
    [] -> Nothing

-- | The state holds the byte at an address.
stateHolds :: State -> Integer -> Bool
stateHolds state address = segmentOf state address 1 /= Nothing

stateSlice :: State -> Integer -> Int -> Maybe B.ByteString
stateSlice state address count = do
  (_, segment) <- segmentOf state address count
  pure (B.take count (B.drop (fromIntegral (address - segmentBase segment)) (segmentBytes segment)))

stateWord :: State -> Integer -> Maybe Word32
stateWord state address = do
  bytes <- stateSlice state address 4
  case B.unpack bytes of
    [a, b, c, d] -> Just (fromIntegral a .|. (fromIntegral b `shiftL` 8) .|. (fromIntegral c `shiftL` 16) .|. (fromIntegral d `shiftL` 24))
    _ -> Nothing

overlayState :: State -> Integer -> B.ByteString -> Either String State
overlayState state address bytes = case segmentOf state address (B.length bytes) of
  Nothing -> Left ("overlay leaves the dumped ranges at " ++ hexTextOf address)
  Just (index, segment) ->
    let offset = fromIntegral (address - segmentBase segment)
        source = segmentBytes segment
        replaced = segment {segmentBytes = B.take offset source <> bytes <> B.drop (offset + B.length bytes) source}
     in Right (State [if position == index then replaced else other | (position, other) <- zip [0 ..] (stateSegments state)])

-- | Every segment of the second state written over the first.
overlayStates :: State -> State -> Either String State
overlayStates base top = foldl' (\acc segment -> acc >>= \state -> overlayState state (segmentBase segment) (segmentBytes segment)) (Right base) (stateSegments top)

-- | The addresses whose byte differs between two states of the same layout.
netChanged :: State -> State -> [Integer]
netChanged left right = concat (zipWith differences (stateSegments left) (stateSegments right))
  where
    differences a b =
      map ((segmentBase a +) . fromIntegral)
        (B.findIndices (/= 0) (B.pack (B.zipWith (\x y -> if x == y then 0 else 1 :: Word8) (segmentBytes a) (segmentBytes b))))

byteAt :: State -> Integer -> Word8
byteAt state address = case stateSlice state address 1 of
  Just bytes -> B.head bytes
  Nothing -> 0

-- ---------------------------------------------------------------------------
-- Spans and comparison

data Span = Span {spanLow :: Integer, spanHigh :: Integer} deriving (Eq, Ord, Show)

spanAt :: Span -> Integer -> Bool
spanAt span' address = spanLow span' <= address && address < spanHigh span'

spanBytes :: Span -> Integer
spanBytes span' = spanHigh span' - spanLow span'

data Difference = Difference
  { differenceAddress :: Integer
  , differencePredicted :: Word8
  , differenceObserved :: Word8
  , differenceDeclared :: Bool
  }
  deriving (Eq, Show)

-- | Compares the predicted next state with the observed one over the declared spans and
-- every byte the step changed. A declared span is compared byte for byte; outside the
-- spans only the step's net changes are compared, so a foreign writer's byte is not a
-- difference.
compareState :: [Span] -> [Integer] -> State -> State -> [Difference]
compareState spans changed predicted observed =
  [ Difference address (byteAt predicted address) (byteAt observed address) (any (`spanAt` address) spans)
  | address <- Set.toAscList addresses
  , byteAt predicted address /= byteAt observed address
  ]
  where
    addresses = Set.fromList changed `Set.union` Set.fromList (concatMap (\span' -> [spanLow span' .. spanHigh span' - 1]) spans)

-- ---------------------------------------------------------------------------
-- Code windows

data Window = Window {windowBase :: Word32, windowSize :: Word32} deriving (Eq, Show)

windowContains :: Window -> Word32 -> Bool
windowContains window address = windowBase window <= address && address < windowBase window + windowSize window

windowExtend :: Word32 -> Word32 -> Window -> Window
windowExtend low high window = Window (min (windowBase window) low) (max (windowBase window + windowSize window) high - min (windowBase window) low)

windowsBytes :: [Window] -> Integer
windowsBytes = sum . map (fromIntegral . windowSize)

mergeWindows :: [Window] -> [Window]
mergeWindows = reverse . foldl' add [] . sortOn windowBase
  where
    add (window : rest) next
      | windowBase next <= end window = Window (windowBase window) (max (end window) (end next) - windowBase window) : rest
    add merged next = next : merged
    end window = windowBase window + windowSize window

-- ---------------------------------------------------------------------------
-- The native case

data RegionPlan = RegionPlan
  { regionId :: String
  , regionBase :: Word32
  , regionSize :: Word32
  , regionPermission :: String
  , regionArtifact :: Maybe String
  }
  deriving (Eq, Show)

data StubCall = StubCall
  { stubInputs :: [(String, Word32)]
  , stubOutputs :: [(String, Word32)]
  , stubWrites :: [(Word32, B.ByteString)]
  }
  deriving (Eq, Show)

data StubPlan = LegacyStub Word32 [(String, Word32)] | ScriptStub Word32 [StubCall] deriving (Eq, Show)

data CasePlan = CasePlan
  { casePlanId :: String
  , casePlanProvenance :: String
  , casePlanExclusions :: [String]
  , casePlanEntry :: Word32
  , casePlanExit :: Word32
  , casePlanRegisters :: [(String, Word32)]
  , casePlanRegions :: [RegionPlan]
  , casePlanFunctions :: [(Word32, Word32)]
  , casePlanStubs :: [StubPlan]
  , casePlanArtifacts :: [(String, B.ByteString)]
  , casePlanBudget :: Word32
  }
  deriving (Eq, Show)

hexValue :: Word32 -> Value
hexValue value = s (hexTextOf (fromIntegral value))

hexTextOf :: Integer -> String
hexTextOf value = let digits = showHex value "" in "0x" ++ replicate (8 - length digits) '0' ++ digits

hexBytes :: B.ByteString -> String
hexBytes = concatMap byte . B.unpack
  where
    byte value = let digits = showHex value "" in replicate (2 - length digits) '0' ++ digits

regionValue :: RegionPlan -> Value
regionValue region = obj
  [ ("id", s (regionId region))
  , ("address", hexValue (regionBase region))
  , ("size", n (fromIntegral (regionSize region)))
  , ("permissions", s (regionPermission region))
  , ("initial", case regionArtifact region of
      Nothing -> obj [("zero", Bool True)]
      Just artifact -> obj [("artifact", s artifact), ("offset", n 0), ("length", n (fromIntegral (regionSize region)))])
  ]

stubValue :: StubPlan -> Value
stubValue (LegacyStub address outputs) = obj
  [ ("address", hexValue address)
  , ("inputs", obj [])
  , ("outputs", obj [(name, hexValue value) | (name, value) <- outputs])
  , ("side_effects", s "registers-only")
  ]
stubValue (ScriptStub address calls) = obj
  [ ("address", hexValue address)
  , ("calls", arr (map callValue calls))
  ]
  where
    callValue call = obj
      [ ("inputs", obj [(name, hexValue value) | (name, value) <- stubInputs call])
      , ("outputs", obj [(name, hexValue value) | (name, value) <- stubOutputs call])
      , ("reads", arr [])
      , ("writes", arr [obj [("address", hexValue address'), ("offset", n 0), ("hex", s (hexBytes bytes))] | (address', bytes) <- stubWrites call])
      ]

caseValue :: CasePlan -> Value
caseValue plan = obj
  [ ("schema_version", n 1)
  , ("id", s (casePlanId plan))
  , ("origin", s "synthetic")
  , ("provenance", s (casePlanProvenance plan))
  , ("cpu", obj [("architecture", s "arm"), ("model", s "cortex-r5f"), ("mode", s "arm"), ("endian", s "little")])
  , ("memory_profile", s "resource8")
  , ("artifacts", arr [obj [("id", s ident), ("path", s (ident ++ ".bin")), ("size", n (fromIntegral (B.length bytes))), ("sha256", s (sha256Hex (sha256 bytes)))] | (ident, bytes) <- casePlanArtifacts plan])
  , ("entry", hexValue (casePlanEntry plan))
  , ("exit", hexValue (casePlanExit plan))
  , ("registers", obj [(name, hexValue value) | (name, value) <- casePlanRegisters plan])
  , ("regions", arr (map regionValue (casePlanRegions plan)))
  , ("functions", arr [obj [("address", hexValue address), ("size", n (fromIntegral size))] | (address, size) <- casePlanFunctions plan])
  , ("stubs", arr (map stubValue (casePlanStubs plan)))
  , ("limits", obj [("instructions", n (fromIntegral (casePlanBudget plan))), ("wall_ms", n 4000)])
  , ("expected", obj [("status", s "returned"), ("registers", obj [("r0", hexValue 0)]), ("memory", arr []), ("events", arr [])])
  , ("exclusions", arr (map s (casePlanExclusions plan)))
  ]

-- | The recording trampoline that replaces the kernel post. It saves r4/r5, advances the
-- call counter, records r0-r3 and the descriptor's header, length and payload pointer in
-- one 32-byte slot, returns zero and restores the callee-saved registers. Slot @n@ (from
-- one) starts at @recordBase + 32*n@.
trampoline :: [Word32]
trampoline =
  [ 0xE92D0030 -- push {r4, r5}
  , 0xE59F4044 -- ldr r4, [pc, #0x44]   ; recordBase
  , 0xE594C000 -- ldr r12, [r4]         ; call counter
  , 0xE28CC001 -- add r12, r12, #1
  , 0xE584C000 -- str r12, [r4]
  , 0xE1A0C28C -- mov r12, r12, lsl #5
  , 0xE084400C -- add r4, r4, r12       ; slot base
  , 0xE5840000 -- str r0, [r4, #0]      ; queue
  , 0xE5841004 -- str r1, [r4, #4]      ; descriptor
  , 0xE5842008 -- str r2, [r4, #8]
  , 0xE584300C -- str r3, [r4, #12]
  , 0xE5910000 -- ldr r0, [r1, #0]      ; header
  , 0xE5840010 -- str r0, [r4, #16]
  , 0xE5910004 -- ldr r0, [r1, #4]      ; length
  , 0xE5840014 -- str r0, [r4, #20]
  , 0xE5910008 -- ldr r0, [r1, #8]      ; payload pointer
  , 0xE5840018 -- str r0, [r4, #24]
  , 0xE3A00000 -- mov r0, #0
  , 0xE8BD0030 -- pop {r4, r5}
  , 0xE12FFF1E -- bx lr
  , fromIntegral recordBase
  ]
