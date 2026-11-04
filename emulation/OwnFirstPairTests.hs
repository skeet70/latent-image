{-# LANGUAGE OverloadedStrings #-}
module Main (main) where

import Control.Monad (unless)
import Data.Aeson (Value (..), object, (.=))
import qualified Data.Aeson as A
import qualified Data.Aeson.KeyMap as KM
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import qualified Data.ByteString.Lazy as BL
import qualified Data.Map.Strict as M
import qualified Data.Vector as V
import Data.Word (Word32)
import Emulation.OwnFirstPairCheck
import Emulation.OwnFirstPairPlan
import Emulation.Sha256 (sha256, sha256Hex)
import Numeric (showHex)
import Replacement.RtosOwnFirst.Regions (Domain (..))
import System.Exit (exitFailure)

hex :: Word32 -> String
hex value = "0x" ++ showHex value ""

digestOf :: String -> String
digestOf = sha256Hex . sha256 . C.pack

ranges :: [(Domain, Word32, Word32)]
ranges = [(DomainA, 0x41140, 0x41164), (DomainB, 0xd1140, 0xd1164)]

planFor :: Role -> Plan
planFor role = Plan
  { planRole = role
  , planDatSha256 = digestOf "dat"
  , planEntries = [PlanEntry DomainA Slot164 0x41140, PlanEntry DomainB Slot164 0xd1140]
  , planTails = []
  , planGuarded = [GuardedRange domain start end (digestOf (hex start)) | (domain, start, end) <- ranges]
  , planFills = []
  }

planBytesFor :: Role -> B.ByteString
planBytesFor role = BL.toStrict (A.encode (planValue (planFor role)))

domainText :: Domain -> String
domainText domain = if domain == DomainA then "a" else "b"

readback :: Integer -> [Domain] -> Value
readback instructions armed = object
  [ "instructions" .= instructions
  , "ranges" .=
      [ object
          [ "domain" .= domainText domain, "start" .= hex start, "end" .= hex end, "physical_sha256" .= digestOf (hex start)
          , "views" .= [object ["core" .= core, "address" .= hex address, "sha256" .= digestOf (hex start)] | core <- [0, 1 :: Int], address <- [start, start + 0x40000000]] ]
      | (domain, start, end) <- ranges, domain `elem` armed ] ]

segment :: [Domain] -> Integer -> Integer -> Value
segment armed started ended = object
  [ "domains" .= map domainText armed, "started" .= started, "ended" .= ended
  , "state" .= object
      [ "enabled" .= True, "records" .= ([] :: [Value]), "violation" .= Null
      , "classes" .= [object ["range" .= [start, end], "kinds" .= ["cpu-write", "host-write", "model-bus-write" :: String]] | (domain, start, end) <- ranges, domain `elem` armed] ] ]

-- | +0x164 entered on core 0.
callFrom :: Word32 -> Word32 -> Word32 -> Value
callFrom entryCpsr resultR0 resultCpsr = object
  [ "entry" .= object
      [ "slot" .= object ["domain" .= ("a" :: String), "slot" .= ("0x164" :: String)], "core" .= ("primary" :: String)
      , "cpsr" .= entryCpsr, "r0" .= (0 :: Word32), "lr" .= ("0x41a60" :: String), "identity" .= (0x4003 :: Word32)
      , "words" .= object [] ]
  , "result" .= object ["r0" .= resultR0, "cpsr" .= resultCpsr, "words" .= object []]
  , "tail_hits" .= (0 :: Int), "count" .= (3 :: Int), "first" .= (300 :: Int), "last" .= (400 :: Int) ]

-- | Entered with I clear.
call :: Word32 -> Word32 -> Value
call = callFrom 0x6000001f

reachedRow :: String -> Value
reachedRow slot = object ["slot" .= object ["domain" .= ("a" :: String), "slot" .= slot], "calls" .= (3 :: Int), "returns" .= (3 :: Int), "pending" .= (0 :: Int)]

-- | Inputs: plan, run document, DAT, boot ROM, trajectory, observer executable; `replaced` swaps digests by index.
inputsFor :: Role -> [(Int, String)] -> Value
inputsFor role replaced = toJSONList
  [ object ["path" .= name, "sha256" .= maybe digest id (lookup index replaced)]
  | (index, name, digest) <- zip3 [0 ..] ["plan", "run", "dat", "rom", "trajectory", "observer" :: String]
      [sha256Hex (sha256 (planBytesFor role)), runDocumentSha256, digestOf "dat", bootRomSha256, digestOf "trajectory", digestOf "observer"] ]

observationFor :: Role -> Value
observationFor role = object
  [ "schema" .= (1 :: Int), "role" .= roleWire role
  , "inputs" .= inputsFor role []
  , "entries" .= (3 :: Int), "returns" .= (3 :: Int)
  , "untracked" .= emptySample, "unarmed_entries" .= emptySample, "unmatched_tails" .= emptySample
  , "fill_fetch" .= Null
  , "armings" .= [ object ["domain" .= ("a" :: String), "stop" .= object [], "readback" .= readback 100 [DomainA]]
                 , object ["domain" .= ("b" :: String), "stop" .= object [], "readback" .= readback 200 [DomainB]] ]
  , "segments" .= [segment [DomainA] 100 200, segment [DomainA, DomainB] 200 900]
  , "final_readback" .= readback 900 [DomainA, DomainB]
  , "milestones" .= [object ["phase" .= ("cold-start" :: String), "pulse" .= Null, "expected" .= menu, "reached" .= (5 :: Int), "observed" .= menu, "finished" .= (800 :: Int)]]
  , "reached" .= map reachedRow ["0x164", "0x168", "0x1618"]
  , "classes" .= [call 0 0x6000009f]
  , "pending" .= ([] :: [Value]), "error" .= Null, "machine_failure" .= Null ]
 where
  emptySample = object ["count" .= (0 :: Int), "sample" .= ([] :: [Value])]
  menu = object ["menu" .= (69 :: Int), "setting" .= (52 :: Int), "cursor" .= (12 :: Int)]

valid :: Value
valid = observationFor OwnedRole

toJSONList :: [Value] -> Value
toJSONList = Array . V.fromList

set :: KM.Key -> Value -> Value -> Value
set key value (Object fields) = Object (KM.insert key value fields)
set _ _ other = other

outcomeFor :: Role -> Value -> Either String (Either Refusal Qualified)
outcomeFor role value = qualify (planBytesFor role) (planFor role) <$> decodeObservation (BL.toStrict (A.encode value))

outcome :: Value -> Either String (Either Refusal Qualified)
outcome = outcomeFor OwnedRole

refusedAs :: RefusalKind -> Value -> Bool
refusedAs kind value = case outcome value of
  Right (Left refusal) -> refusalKind refusal == kind
  _ -> False

entryState :: Slot -> Word32 -> Word32 -> [(WordName, Word32)] -> Maybe Word32 -> EntryState
entryState slot cpsr r0 named pointee = EntryState DomainA slot 0 cpsr r0 0x41a60 0x4003
  (M.fromList [(name, Committed value) | (name, value) <- named]) Nothing (Committed <$> pointee)

callClass :: EntryState -> Word32 -> Word32 -> Maybe Word32 -> CallClass
callClass entry r0 cpsr pointee = CallClass entry (ResultState r0 cpsr M.empty (Committed <$> pointee)) 0 1 1

main :: IO ()
main = do
  let current = 0x4005ff0c + 59 * 56
      index = taskIndex DomainA current
      checks =
        [ ("a correct +0x164 call qualifies", either (const False) (either (const False) ((== 3) . qualifiedCalls)) (outcome valid))
        , ("+0x164 returning the set I bit for a clear prior I refuses", refusedAs SemanticRefusal (set "classes" (Array (V.fromList [call 0x80 0x6000009f])) valid))
        , ("+0x164 leaving I clear refuses", refusedAs SemanticRefusal (set "classes" (Array (V.fromList [call 0 0x6000001f])) valid))
        , ("an arming readback differing from the plan refuses", refusedAs ReadbackRefusal
            (set "armings" (toJSONList [object ["domain" .= ("a" :: String), "stop" .= object [], "readback" .= swapDigest (readback 100 [DomainA])]]) valid))
        , ("an alias view differing at the end refuses", refusedAs ReadbackRefusal (set "final_readback" (aliasDiffers (readback 900 [DomainA, DomainB])) valid))
        , ("a UDF fill fetch refuses", refusedAs FillRefusal
            (set "error" (String "fill word at 0x4115c fetched") (set "fill_fetch" (object ["core" .= ("primary" :: String), "address" .= ("0x4115c" :: String)]) valid)))
        , ("an immutable-extent write refuses", refusedAs ImmutableRefusal
            (set "segments" (toJSONList [segment [DomainA] 100 200, withRow (segment [DomainA, DomainB] 200 900)]) valid))
        , ("a watch gap refuses", refusedAs ImmutableRefusal (set "segments" (toJSONList [segment [DomainA] 100 150, segment [DomainA, DomainB] 200 900]) valid))
        , ("+0x128 stores the multiply index of T[59]", index == 60 && checkCall 0 (callClass (entryState Slot128 0x13 0x1000 [(ContextWord, 0), (CurrentTask, current)] (Just 7)) 0 0x13 (Just index)) == Right ())
        , ("+0x128 storing the divide-by-56 index for base + 8 refuses", checkCall 0 (callClass (entryState Slot128 0x13 0x1000 [(ContextWord, 0), (CurrentTask, 0x4005ff0c + 8)] (Just 7)) 0 0x13 (Just 1)) /= Right ())
        , ("+0x128 under I returns -25 with *r0 untouched", checkCall 0 (callClass (entryState Slot128 0x93 0x1000 [(ContextWord, 0), (CurrentTask, current)] (Just 7)) contextError 0x93 (Just 7)) == Right ())
        , ("+0x168 returns the entry flags", checkCall 0 (callClass (entryState Slot168 0xf0000093 0 [] Nothing) 0xf0000013 0x13 Nothing) == Right ())
        , ("+0x168 dropping the flags refuses", checkCall 0 (callClass (entryState Slot168 0xf0000093 0 [] Nothing) 0x13 0x13 Nothing) /= Right ())
        , ("+0x1618 returns 1 for core 1's identity", checkCall 0 (callClass ((entryState Slot1618 0x13 0 [] Nothing) {entryIdentity = 0x8003}) 1 0x13 Nothing) == Right ())
        , ("an owned +0x1618 call that skipped its tail refuses", checkCall 1 (callClass (entryState Slot1618 0x13 0 [] Nothing) 0 0x13 Nothing) /= Right ())
        , ("the restored control is satisfied by A's arming readback at 0x41140", controlCheck ControlRestoredRole
            (set "armings" (toJSONList [arming "a" (swapDigest (readback 100 [DomainA])), arming "b" (readback 200 [DomainB])]) (observationFor ControlRestoredRole)))
        , ("the restored control is not satisfied by a final readback difference", not (controlCheck ControlRestoredRole
            (set "final_readback" (swapDigest (readback 900 [DomainA, DomainB])) (observationFor ControlRestoredRole))))
        , ("the restored control is not satisfied by an unarmed loader", not (controlCheck ControlRestoredRole
            (set "armings" (toJSONList [arming "a" (readback 100 [DomainA])]) (observationFor ControlRestoredRole))))
        , ("the restored control is not satisfied by a missing final readback", not (controlCheck ControlRestoredRole
            (set "final_readback" Null (observationFor ControlRestoredRole))))
        , ("the restored control is not satisfied by a qualifying run", not (controlCheck ControlRestoredRole (observationFor ControlRestoredRole)))
        , ("the wrong-I control is satisfied by A +0x164 entered with I clear", controlCheck ControlWrongIRole
            (set "classes" (toJSONList [call 0x80 0x6000009f]) (observationFor ControlWrongIRole)))
        , ("the wrong-I control is not satisfied by A +0x164 entered with I set", not (controlCheck ControlWrongIRole
            (set "classes" (toJSONList [callFrom 0x6000009f 0 0x6000009f]) (observationFor ControlWrongIRole))))
        , ("the wrong-I control is not satisfied by a readback difference", not (controlCheck ControlWrongIRole
            (set "armings" (toJSONList [arming "a" (swapDigest (readback 100 [DomainA])), arming "b" (readback 200 [DomainB])]) (observationFor ControlWrongIRole))))
        , ("a run document other than the pinned one refuses", refusedAs BindingRefusal (set "inputs" (inputsFor OwnedRole [(1, digestOf "run")]) valid))
        , ("a boot ROM other than the pinned one refuses", refusedAs BindingRefusal (set "inputs" (inputsFor OwnedRole [(3, digestOf "rom")]) valid))
        , ("compare accepts runs with equal run, ROM, trajectory and observer", compared [] == Just True)
        , ("compare refuses runs with different trajectories", compared [(4, digestOf "other trajectory")] == Nothing)
        , ("compare refuses runs with different observers", compared [(5, digestOf "other observer")] == Nothing)
        , ("a call pending since before the route ended refuses", refusedAs TrackingRefusal (pendingAt 700))
        , ("a call entered at the route end may stay pending", either (const False) (either (const False) ((== 1) . qualifiedPending)) (outcome (pendingAt 800)))
        , ("+0xfc accepts the returned timer base", checkCall 0 (getTime 120 (1000 - 120)) == Right ())
        , ("+0xfc accepts the entry timer base", checkCall 0 (getTime 120 (1000 - 100)) == Right ())
        , ("+0xfc refuses an offset from neither base", checkCall 0 (getTime 120 (1000 - 130)) /= Right ())
        , ("+0x100 accepts the returned timer base", checkCall 0 (setTime 120 125) == Right ())
        , ("+0x100 accepts the entry timer base", checkCall 0 (setTime 120 105) == Right ())
        , ("+0x100 refuses a time from neither base", checkCall 0 (setTime 120 200) /= Right ())
        , ("a run with no checked call refuses", refusedAs RouteRefusal
            (set "entries" (Number 0) (set "returns" (Number 0) (set "classes" (toJSONList []) valid))))
        , ("a run that never reached A +0x168 refuses", refusedAs RouteRefusal (set "reached" (toJSONList (map reachedRow ["0x164", "0x1618"])) valid))
        , ("a run that never reached A +0x1618 refuses", refusedAs RouteRefusal (set "reached" (toJSONList (map reachedRow ["0x164", "0x168"])) valid))
        ]
  mapM_ (\(name, passed) -> putStrLn ((if passed then "ok   " else "FAIL ") ++ name)) checks
  unless (all snd checks) exitFailure
 where
  arming domain value = object ["domain" .= (domain :: String), "stop" .= object [], "readback" .= value]
  controlCheck role value = either (const False) (controlSatisfied role) (outcomeFor role value)
  compared replaced = do
    stock <- either (const Nothing) (either (const Nothing) Just) (outcomeFor StockRole (observationFor StockRole))
    owned <- either (const Nothing) (either (const Nothing) Just) (outcome (set "inputs" (inputsFor OwnedRole replaced) valid))
    either (const Nothing) (Just . null) (compareRoutes stock owned)
  pendingAt :: Int -> Value
  pendingAt entered = set "entries" (Number 4) (set "pending" (toJSONList [object ["entered" .= entered]]) valid)
  timeClass slot entryWords pointee returnedWords returnedPointee = CallClass
    (EntryState DomainA slot 0 0x13 0x1000 0x41a60 0x4003 (M.fromList [(name, Committed value) | (name, value) <- entryWords]) Nothing (Just (Committed pointee)))
    (ResultState 0 0x13 (M.fromList [(name, Committed value) | (name, value) <- returnedWords]) (Committed <$> returnedPointee)) 0 1 1
  getTime returnedBase offset = timeClass SlotFc [(ContextWord, 0), (TimerBase, 100)] 1000 [(TimeOffset, offset), (TimerBase, returnedBase)] Nothing
  setTime returnedBase time = timeClass Slot100 [(ContextWord, 0), (TimerBase, 100), (TimeOffset, 5)] 7 [(TimerBase, returnedBase)] (Just time)
  swapDigest = mapRanges (set "physical_sha256" (String "00"))
  aliasDiffers = mapRanges (\range -> case range of
    Object fields | Just (Array views) <- KM.lookup "views" fields ->
      Object (KM.insert "views" (Array (V.imap (\position view -> if position == 1 then set "sha256" (String "00") view else view) views)) fields)
    other -> other)
  mapRanges change value = case value of
    Object fields | Just (Array items) <- KM.lookup "ranges" fields ->
      Object (KM.insert "ranges" (Array (V.imap (\position item -> if position == 0 then change item else item) items)) fields)
    other -> other
  withRow value = case value of
    Object fields | Just (Object state) <- KM.lookup "state" fields ->
      Object (KM.insert "state" (Object (KM.insert "records" (toJSONList [object ["source" .= ("host-write" :: String), "physical" .= (0x41150 :: Int)]]) state)) fields)
    other -> other
