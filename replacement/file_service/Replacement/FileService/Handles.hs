-- | The tag allocator and tag map cases: an independent model of the five stock entries,
-- read from the bytes of [0x71ea34,0x71eba0) and [0x71ef18,0x71f078), gives each case's
-- results, service calls and final state.
module Replacement.FileService.Handles
  ( code
  , scenarios
  ) where

import Data.Bits (shiftR, xor, (.&.))
import qualified Data.ByteString as B
import Data.List (findIndex)
import Data.Word (Word32, Word64)
import Replacement.FileService.Native

allocateEntry, releaseEntry, bindEntry, unbindEntry, lookupEntry :: Word32
allocateEntry = 0x71ef18
releaseEntry = 0x71eff4
bindEntry = 0x71ea34
unbindEntry = 0x71eaac
lookupEntry = 0x71eb28

waitService, signalService, claimService :: Word32
waitService = 0x724b68
signalService = 0x724c00
claimService = 0x71e960

tagSemaphore :: Word32
tagSemaphore = 0x3c

code :: Code
code = Code
  { codeImageBase = 0x61c400
  , codeExtents = [("map-code", Extent 0x71ea34 0x71eba0), ("tag-code", Extent 0x71ef18 0x71f078)]
  , codeStockFunctions = [(0x71ea34, 0x78), (0x71eaac, 0x7c), (0x71eb28, 0x78), (0x71ef18, 0xdc), (0x71eff4, 0x84)]
  , codeRetained = []
  , codeRetainedFunctions = []
  }

-- Model -------------------------------------------------------------------

data State = State
  { stateTags :: [Word32]
  , stateSequence :: Word32
  , statePairs :: [(Word32, Word32)]
  }

empty :: State
empty = State (replicate 16 0) 0 (replicate 16 (0, 0))

stateBytes :: State -> B.ByteString
stateBytes state =
  words32le (concat [[index, tag] | (index, tag) <- statePairs state] ++ stateTags state ++ [stateSequence state])

data Op = Allocate | Release Word32 | Bind Word32 | Lookup Word32 | Unbind Word32

-- | Scripted answers: each wait's status and each file-cache claim's index, in call order.
data Script = Script {scriptWaits :: [Word32], scriptClaims :: [Word32]}

replace :: Int -> a -> [a] -> [a]
replace index value values = take index values ++ [value] ++ drop (index + 1) values

wait :: Script -> (Word32, Script, Service)
wait script = case scriptWaits script of
  status : rest -> (status, script {scriptWaits = rest}, call waitService [("r0", tagSemaphore), ("r1", 0xffffffff)] status)
  [] -> (0, script, call waitService [("r0", tagSemaphore), ("r1", 0xffffffff)] 0)

signal :: Service
signal = call signalService [("r0", tagSemaphore)] 0

call :: Word32 -> [(String, Word32)] -> Word32 -> Service
call address inputs result = Service address inputs [("r0", result)] [] []

nextCandidate :: Word32 -> Word32
nextCandidate previous = let next = previous + 1 in if next >= 0x80000000 then 1 else next

step :: (State, Script) -> Op -> ((State, Script), Maybe Word32, [Service])
step (state, script) op = case op of
  Allocate ->
    let (status, script', waited) = wait script
        candidate = until (`notElem` stateTags state) nextCandidate (nextCandidate (stateSequence state))
        free = findIndex (== 0) (stateTags state)
        tags = maybe (stateTags state) (\slot -> replace slot candidate (stateTags state)) free
     in if status /= 0
          then ((state, script'), Just 0, [waited])
          else ((state {stateTags = tags, stateSequence = candidate}, script'), Just (maybe 0 (const candidate) free), [waited, signal])
  Release 0 -> ((state, script), Nothing, [])
  Release tag ->
    let (status, script', waited) = wait script
        tags = case findIndex (== tag) (stateTags state) of
          Just slot | status == 0 -> replace slot 0 (stateTags state)
          _ -> stateTags state
     in ((state {stateTags = tags}, script'), Nothing, [waited, signal])
  Bind tag ->
    let (index, script') = case scriptClaims script of
          first : rest -> (first, script {scriptClaims = rest})
          [] -> (0, script)
        claimed = call claimService [] index
        pairs = case findIndex ((== 0) . snd) (statePairs state) of
          Just slot | index /= 0 -> replace slot (index, tag) (statePairs state)
          _ -> statePairs state
     in ((state {statePairs = pairs}, script'), Just index, [claimed])
  Lookup tag ->
    let found = if tag == 0 then 0 else maybe 0 fst (lookupPair tag (statePairs state))
     in ((state, script), Just found, [])
  Unbind 0 -> ((state, script), Nothing, [])
  Unbind tag ->
    let pairs = case findIndex ((== tag) . snd) (statePairs state) of
          Just slot -> replace slot (0, 0) (statePairs state)
          Nothing -> statePairs state
     in ((state {statePairs = pairs}, script), Nothing, [])
  where
    lookupPair tag pairs = case filter ((== tag) . snd) pairs of
      pair : _ -> Just pair
      [] -> Nothing

opCall :: Op -> Call
opCall op = case op of
  Allocate -> Call allocateEntry [] [] True
  Release tag -> Call releaseEntry [tag] [] False
  Bind tag -> Call bindEntry [tag] [] True
  Lookup tag -> Call lookupEntry [tag] [] True
  Unbind tag -> Call unbindEntry [tag] [] False

opEntry :: Op -> Word32
opEntry = callEntry . opCall

scenario :: String -> State -> Script -> [Op] -> Scenario
scenario name initial script ops = Scenario
  { scenarioName = name
  , scenarioEntry = case ops of
      op : _ -> opEntry op
      [] -> allocateEntry
  , scenarioData = [DataRegion "handles" 0x910140 (stateBytes initial) (stateBytes final)]
  , scenarioCalls = map opCall ops
  , scenarioServices = concat services
  , scenarioResults = [result | Just result <- results]
  , scenarioStop = Nothing
  }
  where
    ((final, _), results, services) = foldl run ((initial, script), [], []) ops
    run (current, done, called) op =
      let (next, result, made) = step current op in (next, done ++ [result], called ++ [made])

-- Scenarios ---------------------------------------------------------------

granted :: Script
granted = Script [] []

tagsFrom :: [Word32] -> [Word32]
tagsFrom values = take 16 (values ++ repeat 0)

pairsFrom :: [(Word32, Word32)] -> [(Word32, Word32)]
pairsFrom values = take 16 (values ++ repeat (0, 0))

fixed :: [Scenario]
fixed =
  [ scenario "allocate-first" empty granted [Allocate]
  , scenario "allocate-after-held" empty {stateSequence = 41, stateTags = tagsFrom [7]} granted [Allocate]
  , scenario "allocate-sequence-wrap" empty {stateSequence = 0x7fffffff} granted [Allocate]
  , scenario "allocate-sequence-minus-one" empty {stateSequence = 0xffffffff} granted [Allocate]
  , scenario "allocate-skip-allocated" empty {stateSequence = 9, stateTags = tagsFrom [0, 0, 0, 10]} granted [Allocate]
  , scenario "allocate-skip-last-slot" empty {stateSequence = 9, stateTags = replace 15 11 (tagsFrom [12, 0, 10])} granted [Allocate]
  , scenario "allocate-skip-wrap" empty {stateSequence = 0x7ffffffe, stateTags = tagsFrom [0x7fffffff, 1, 2]} granted [Allocate]
  , scenario "allocate-no-free-slot" empty {stateSequence = 100, stateTags = [200 .. 215]} granted [Allocate]
  , scenario "allocate-no-free-slot-skip" empty {stateSequence = 100, stateTags = [101 .. 116]} granted [Allocate]
  , scenario "allocate-no-free-slot-minus-one" empty {stateSequence = 0xffffffff, stateTags = [200 .. 215]} granted [Allocate]
  , scenario "allocate-failed-wait" empty {stateSequence = 5} (Script [1] []) [Allocate]
  , scenario "release-first-slot" empty {stateTags = tagsFrom [3, 4]} granted [Release 3]
  , scenario "release-first-duplicate" empty {stateTags = tagsFrom [0, 0, 9, 0, 9]} granted [Release 9]
  , scenario "release-missing" empty {stateTags = tagsFrom [3, 4]} granted [Release 5]
  , scenario "release-zero" empty {stateTags = tagsFrom [0, 4]} granted [Release 0]
  , scenario "release-after-failed-wait" empty {stateTags = tagsFrom [3, 4]} (Script [2] []) [Release 3]
  , scenario "bind-first" empty (Script [] [1]) [Bind 0x21]
  , scenario "bind-after-used" empty {statePairs = pairsFrom [(1, 77)]} (Script [] [2]) [Bind 78]
  , scenario "bind-without-claim" empty (Script [] [0]) [Bind 5]
  , scenario "bind-every-pair-used" empty {statePairs = [(n, n) | n <- [1 .. 16]]} (Script [] [3]) [Bind 99]
  , scenario "bind-zero-tag" empty {statePairs = pairsFrom [(4, 0)]} (Script [] [6]) [Bind 0]
  , scenario "lookup-first-pair" empty {statePairs = pairsFrom [(5, 8)]} granted [Lookup 8]
  , scenario "lookup-first-duplicate" empty {statePairs = pairsFrom [(0, 0), (4, 21), (6, 21)]} granted [Lookup 21]
  , scenario "lookup-missing" empty {statePairs = pairsFrom [(5, 8)]} granted [Lookup 9]
  , scenario "lookup-zero" empty {statePairs = pairsFrom [(5, 0)]} granted [Lookup 0]
  , scenario "unbind-first-duplicate" empty {statePairs = pairsFrom [(0, 0), (4, 21), (6, 21)]} granted [Unbind 21]
  , scenario "unbind-missing" empty {statePairs = pairsFrom [(5, 8)]} granted [Unbind 9]
  , scenario "unbind-zero" empty {statePairs = pairsFrom [(5, 0)]} granted [Unbind 0]
  , scenario "handle-lifetime" empty {stateSequence = 0x7ffffffd} (Script [] [1, 2])
      [Allocate, Allocate, Bind 0x7ffffffe, Bind 0x7fffffff, Lookup 0x7fffffff, Release 0x7ffffffe, Unbind 0x7ffffffe, Lookup 0x7ffffffe, Allocate, Lookup 0x7fffffff]
  ]

-- | A splitmix64 stream; the seed and the draw order fix every generated case.
next64 :: Word64 -> (Word64, Word64)
next64 state = (mix (state + 0x9e3779b97f4a7c15), state + 0x9e3779b97f4a7c15)
  where
    mix z0 =
      let z1 = (z0 `xor` (z0 `shiftR` 30)) * 0xbf58476d1ce4e5b9
          z2 = (z1 `xor` (z1 `shiftR` 27)) * 0x94d049bb133111eb
       in z2 `xor` (z2 `shiftR` 31)

draws :: Word64 -> [Word64]
draws seed = let (value, seed') = next64 seed in value : draws seed'

generated :: [Scenario]
generated = [one index (draws (20261119 + fromIntegral index)) | index <- [0 :: Int .. 31]]
  where
    pool = [1 .. 20] ++ [0x7ffffffe, 0x7fffffff]
    pick values value = values !! fromIntegral (value `mod` fromIntegral (length values))
    one index stream =
      let (header, rest) = splitAt 40 stream
          sequenceWord = pick ([0, 3, 17, 0x7ffffff8, 0x7ffffffe, 0x7fffffff, 0xfffffffe, 0xffffffff] :: [Word32]) (header !! 0)
          tags = [if value `mod` 3 == 0 then pick pool value else 0 | value <- take 16 (drop 1 header)]
          pairs = [if value `mod` 4 == 0 then (fromIntegral (value `shiftR` 8 .&. 15) + 1, pick pool (value `shiftR` 16)) else (0, 0) | value <- take 16 (drop 17 header)]
          waits = [if value `mod` 7 == 0 then 1 else 0 | value <- take 8 (drop 33 header)]
          ops = [opOf value | value <- take 8 rest]
          claims = [fromIntegral (value `mod` 17) | value <- take 8 (drop 8 rest)]
          opOf value = case value `mod` 5 of
            0 -> Allocate
            1 -> Release (pick pool (value `shiftR` 8))
            2 -> Bind (pick pool (value `shiftR` 8))
            3 -> Lookup (pick pool (value `shiftR` 8))
            _ -> Unbind (pick pool (value `shiftR` 8))
       in scenario ("generated-" ++ show index) (State tags sequenceWord pairs) (Script waits claims) ops

scenarios :: [Scenario]
scenarios = fixed ++ generated
