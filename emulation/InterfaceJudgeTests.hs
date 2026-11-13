{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}
module Main (main) where

import Control.Monad (unless)
import Data.Aeson (Value (..), encode, object, (.=))
import qualified Data.Aeson.Key as K
import qualified Data.Aeson.KeyMap as KM
import qualified Data.ByteString.Lazy as L
import Data.List (find)
import Emulation.InterfaceJudge
import Emulation.InterfaceStream
  ( AllowanceBasis (..)
  , CausalViolation (..)
  , ChannelDifference (..)
  , ChannelName (..)
  , ChannelTolerance (..)
  , Occurrence (..)
  )
import Emulation.Judge

main :: IO ()
main = do
  let firmware = Just (Firmware "stock.bin" stockDigest)

  -- Without channel declarations the stream is one ordered channel.
  let legacyStock = runValues stockDigest 16 32 12 ["message", "reply"]
      legacyOwned = runValues ownedDigest 64 96 9 ["message", "reply"]
      legacyWrong = runValues wrongDigest 64 96 8 ["message", "wrong-reply"]
      legacy = report legacyStock legacyOwned [control "wrong-reply" legacyWrong] Null
  equal <- status firmware legacy VerdictEqual
  check (verdictOwned equal == Just ownedDigest && verdictFirmware equal == Just stockDigest)
  check (null (verdictUnmet equal) && null (verdictSurvivors equal))
  check (verdictChannelDifference equal == Nothing && verdictCausalViolation equal == Nothing)
  different <- status firmware (report legacyStock (runValues ownedDigest 64 96 9 ["message", "wrong-reply"]) [control "wrong-reply" legacyWrong] Null) VerdictDifferent
  check (verdictChannelDifference different == Just (ChannelDifference (ChannelName "stream") (Occurrence 1) (Just "reply") (Just "wrong-reply")))
  _ <- status firmware (report legacyStock (runValues ownedDigest 64 96 9 ["message", "reply", "extra"]) [control "wrong-reply" legacyWrong] Null) VerdictDifferent
  let settled = object ["name" .= ("settled" :: String), "observation" .= (2 :: Int)]
      settledWrong = control "wrong-reply" (runValues wrongDigest 64 96 8 ["message", "wrong-reply", at "settled"])
  legacyMilestone <- status firmware (report (runValues stockDigest 16 32 12 ["message", "reply", at "settled"])
    (runValues ownedDigest 64 96 9 ["message", "reply", at "settled"]) [settledWrong] settled) VerdictEqualUpToMilestone
  check (verdictMilestone legacyMilestone == Just (Milestone "settled" (ObservationIndex 2)))
  -- The one channel is ordered, so it allows no tail at the milestone.
  _ <- status firmware (report (runValues stockDigest 16 32 12 ["message", "reply", at "settled"])
    (runValues ownedDigest 64 96 9 ["message", "reply", "extra", at "settled"]) [settledWrong] settled) VerdictDifferent

  -- The channel declarations the producers use: declaration order is
  -- precedence and the timer channel is time-driven.
  let messages = channel "messages" (object ["kind" .= ("queue-in" :: String)]) False
      replies = channel "replies" (object ["kind" .= ("client-reply" :: String)]) False
      timer = channel "timer" (object ["kind" .= ("tick" :: String)]) True
      channels = [messages, replies, channel "presses" (object ["kind" .= ("half-press" :: String)]) False, timer]
      reportOver declared declaredCausal marker stockEvents ownedEvents controlEvents =
        withChannels declared declaredCausal $
          report (runValues stockDigest 16 32 12 stockEvents)
            (runValues ownedDigest 64 96 9 ownedEvents)
            [control "wrong" (runValues wrongDigest 64 96 8 controlEvents)]
            (maybe Null id marker)
      channelReport = reportOver channels

  -- A cross-channel reordering is equal: each channel carries the same events
  -- in the same order.
  interleaved <- status firmware (channelReport [] Nothing
    [message 1, reply 16, tick 1]
    [reply 16, message 1, tick 1]
    [reply 99, message 1, tick 1]) VerdictEqual
  check (verdictChannelDifference interleaved == Nothing)
  check (verdictCausalViolation interleaved == Nothing)

  -- A reordering within a channel is different, naming that channel's first
  -- differing event.
  (parsedWithin, within) <- judgeWith firmware (channelReport [] Nothing
    [message 1, message 2, reply 16]
    [message 2, message 1, reply 16]
    [message 1, message 2, reply 99]) (Just VerdictDifferent)
  check (fmap differenceChannel (verdictChannelDifference within) == Just (ChannelName "messages"))
  check (fmap differenceOccurrence (verdictChannelDifference within) == Just (Occurrence 0))
  check (fmap differenceChannelStock (verdictChannelDifference within) == Just (Just (message 1)))
  check (fmap differenceChannelOwned (verdictChannelDifference within) == Just (Just (message 2)))
  check (verdictCausalViolation within == Nothing)

  -- A declared causal pair holds when the reply follows the message, and is
  -- different when the reply comes first.
  let pair = causal (object ["kind" .= ("queue-in" :: String)]) (object ["kind" .= ("client-reply" :: String)])
  _ <- status firmware (channelReport [pair] Nothing
    [message 1, reply 16] [message 1, reply 16] [message 2, reply 16]) VerdictEqual
  (parsedCausal, broken) <- judgeWith firmware (channelReport [pair] Nothing
    [message 1, reply 16] [reply 16, message 1] [message 2, reply 16]) (Just VerdictDifferent)
  check (fmap violationObservation (verdictCausalViolation broken) == Just 0)
  check (maybe False (("owned" `isInfix`) . violationReason) (verdictCausalViolation broken))
  check (verdictChannelDifference broken == Nothing)
  let uncaused = causal (object ["kind" .= ("absent" :: String)]) (object ["kind" .= ("client-reply" :: String)])
  missingCause <- status firmware (channelReport [uncaused] Nothing
    [message 1, reply 16] [message 1, reply 16] [message 2, reply 16]) VerdictDifferent
  check (maybe False (("stock's observation 1 matches no earlier" `isInfix`) . violationReason) (verdictCausalViolation missingCause))

  -- Causal pairs are counted: a second reply whose message has not arrived
  -- yet is different even though an earlier message exists, while each reply
  -- following its own message holds.
  _ <- status firmware (channelReport [pair] Nothing
    [message 1, reply 16, message 2, reply 17]
    [message 1, message 2, reply 16, reply 17]
    [message 2, reply 16, message 2, reply 17]) VerdictEqual
  secondReply <- status firmware (channelReport [pair] Nothing
    [message 1, reply 16, message 2, reply 17]
    [message 1, reply 16, reply 17, message 2]
    [message 2, reply 16, message 2, reply 17]) VerdictDifferent
  check (fmap violationObservation (verdictCausalViolation secondReply) == Just 2)
  check (verdictChannelDifference secondReply == Nothing)

  -- A control that only reorders channels is a survivor; one that breaks a
  -- declared pair is detected.
  survived <- status firmware (channelReport [] Nothing
    [message 1, reply 16, tick 1] [message 1, reply 16, tick 1] [reply 16, message 1, tick 1]) VerdictNoVerdict
  check (verdictSurvivors survived == ["wrong"])
  _ <- status firmware (channelReport [pair] Nothing
    [message 1, reply 16, tick 1] [message 1, reply 16, tick 1] [reply 16, message 1, tick 1]) VerdictEqual

  -- A route resumed from a checkpoint is compared from its start marker:
  -- with the milestone at the run's end, an extra owned event on an ordered
  -- channel is different.
  let bound = object ["name" .= ("instruction-bound" :: String), "observation" .= (4 :: Int)]
      start = object ["name" .= ("post-boot" :: String), "observation" .= (0 :: Int)]
  resumed <- status firmware (channelReport [] (Just bound)
    [at "post-boot", message 1, press 1, tick 1, at "instruction-bound"]
    [at "post-boot", message 1, press 1, press 2, tick 1, at "instruction-bound"]
    [at "post-boot", message 2, press 1, tick 1, at "instruction-bound"]) VerdictDifferent
  check (verdictChannelDifference resumed == Just (ChannelDifference (ChannelName "presses") (Occurrence 1) Nothing (Just (press 2))))
  -- A start marker is not a milestone: declaring one leaves the report
  -- unable to judge the events after it.
  startDeclared <- status firmware (channelReport [] (Just start)
    [at "post-boot", message 1, press 1, tick 1]
    [at "post-boot", message 1, press 1, press 2, tick 1]
    [at "post-boot", message 2, press 1, tick 1]) VerdictNoVerdict
  check (any ("follows its milestone marker" `isInfix`) (verdictReasons startDeclared))
  _ <- status firmware (channelReport [] (Just start)
    [at "post-boot", message 1, press 1]
    [at "post-boot", message 1, press 1]
    [at "post-boot", message 2, press 1]) VerdictNoVerdict

  -- At an end milestone an ordered channel may carry a declared tail: within
  -- the allowance on either side it is equal up to the milestone, beyond it
  -- or with a different event it is different.
  let presses = channelWithin "presses" (object ["kind" .= ("half-press" :: String)]) False 1 "a press can land after the bound"
      allowed = reportOver [messages, replies, presses, timer]
      phaseAt :: Int -> Value
      phaseAt index = object ["name" .= ("phase" :: String), "observation" .= index]
  (parsedTail, withinTail) <- judgeWith firmware (allowed [] (Just (phaseAt 2))
    [message 1, press 1, at "phase"]
    [message 1, press 1, press 2, at "phase"]
    [message 2, press 1, at "phase"]) (Just VerdictEqualUpToMilestone)
  check (verdictChannelDifference withinTail == Nothing)
  check (toleranceOf "presses" withinTail == Just (ChannelTolerance (ChannelName "presses") False 1
    (DeclaredAllowance "a press can land after the bound") 1 2))
  check (toleranceOf "messages" withinTail == Just (ChannelTolerance (ChannelName "messages") False 0 OrderedDefault 1 1))
  check (toleranceOf "timer" withinTail == Just (ChannelTolerance (ChannelName "timer") True 1 TimeDrivenDefault 0 0))
  _ <- status firmware (allowed [] (Just (phaseAt 3))
    [message 1, press 1, press 2, at "phase"]
    [message 1, press 1, at "phase"]
    [message 2, press 1, at "phase"]) VerdictEqualUpToMilestone
  beyondTail <- status firmware (allowed [] (Just (phaseAt 2))
    [message 1, press 1, at "phase"]
    [message 1, press 1, press 2, press 3, at "phase"]
    [message 2, press 1, at "phase"]) VerdictDifferent
  check (verdictChannelDifference beyondTail == Just (ChannelDifference (ChannelName "presses") (Occurrence 2) Nothing (Just (press 3))))
  _ <- status firmware (allowed [] (Just (phaseAt 3))
    [message 1, press 1, press 2, at "phase"]
    [message 1, press 1, press 3, at "phase"]
    [message 2, press 1, at "phase"]) VerdictDifferent
  -- Without a declared allowance the ordered tail is different.
  _ <- status firmware (channelReport [] (Just (phaseAt 2))
    [message 1, press 1, at "phase"]
    [message 1, press 1, press 2, at "phase"]
    [message 2, press 1, at "phase"]) VerdictDifferent
  beforeMilestone <- status firmware (channelReport [] (Just (phaseAt 2))
    [message 1, press 1, at "phase"]
    [message 1, press 2, at "phase"]
    [message 2, press 1, at "phase"]) VerdictDifferent
  check (fmap differenceChannel (verdictChannelDifference beforeMilestone) == Just (ChannelName "presses"))
  check (fmap differenceOccurrence (verdictChannelDifference beforeMilestone) == Just (Occurrence 0))

  -- A time-driven channel's allowance defaults to one occurrence on either
  -- side.
  milestoneTimer <- status firmware (channelReport [] (Just (phaseAt 2))
    [message 1, tick 1, at "phase"]
    [message 1, tick 1, tick 2, at "phase"]
    [message 2, tick 1, at "phase"]) VerdictEqualUpToMilestone
  check (verdictMilestone milestoneTimer == Just (Milestone "phase" (ObservationIndex 2)))
  check (verdictChannelDifference milestoneTimer == Nothing)
  check (toleranceOf "timer" milestoneTimer == Just (ChannelTolerance (ChannelName "timer") True 1 TimeDrivenDefault 1 2))
  _ <- status firmware (channelReport [] (Just (phaseAt 3))
    [message 1, tick 1, tick 2, at "phase"]
    [message 1, tick 1, at "phase"]
    [message 2, tick 1, at "phase"]) VerdictEqualUpToMilestone
  _ <- status firmware (channelReport [] (Just (phaseAt 2))
    [message 1, tick 1, at "phase"]
    [message 1, tick 1, tick 2, tick 3, at "phase"]
    [message 2, tick 1, at "phase"]) VerdictDifferent
  _ <- status firmware (channelReport [] (Just (phaseAt 2))
    [message 1, tick 1, at "phase"]
    [message 1, tick 2, at "phase"]
    [message 2, tick 1, at "phase"]) VerdictDifferent
  -- An empty side does not satisfy a time-driven channel when the other side
  -- carries more than the allowance.
  emptyTimer <- status firmware (channelReport [] (Just (phaseAt 3))
    [message 1, tick 1, tick 2, at "phase"]
    [message 1, at "phase"]
    [message 2, tick 1, tick 2, at "phase"]) VerdictDifferent
  check (verdictChannelDifference emptyTimer == Just (ChannelDifference (ChannelName "timer") (Occurrence 1) (Just (tick 2)) Nothing))
  -- Without a milestone a time-driven channel must be equal, and no channel
  -- is judged under an allowance.
  _ <- status firmware (channelReport [] Nothing
    [message 1, tick 1] [message 1, tick 1, tick 2] [message 2, tick 1]) VerdictDifferent
  unbounded <- status firmware (channelReport [] Nothing
    [message 1, tick 1, tick 2] [message 1, tick 1] [message 2, tick 1]) VerdictDifferent
  check (toleranceOf "timer" unbounded == Just (ChannelTolerance (ChannelName "timer") True 0 NoMilestone 2 1))
  noMilestone <- status firmware (allowed [] Nothing
    [message 1, press 1] [message 1, press 1, press 2] [message 2, press 1]) VerdictDifferent
  check (fmap toleranceBasis (toleranceOf "presses" noMilestone) == Just NoMilestone)

  -- A milestone a side does not carry is different, never a milestone pass.
  missingOwned <- status firmware (channelReport [] (Just (phaseAt 2))
    [message 1, tick 1, at "phase"]
    [message 1, tick 1]
    [message 2, tick 1, at "phase"]) VerdictDifferent
  check (any ("owned does not reach milestone phase" `isInfix`) (verdictReasons missingOwned))

  -- An event no declared channel claims leaves the declaration unable to
  -- judge the stream.
  unclaimed <- status firmware (channelReport [] Nothing
    [message 1, object ["kind" .= ("unknown" :: String)]]
    [message 1] [message 2]) VerdictNoVerdict
  check (any ("matches no declared channel" `isInfix`) (verdictReasons unclaimed))

  -- A declaration with no source, an empty selector, an empty name, or a
  -- tail allowance that is negative or gives no reason is refused.
  refused (channelReportWith [withoutSource (channel "messages" (object ["kind" .= ("queue-in" :: String)]) False) ] [] Null)
  refused (channelReportWith [channel "messages" (object []) False ] [] Null)
  refused (channelReportWith [object ["name" .= ("" :: String), "match" .= object ["kind" .= ("queue-in" :: String)], "sources" .= (["test"] :: [String])] ] [] Null)
  refused (channelReportWith [] [object ["first" .= object ["kind" .= ("queue-in" :: String)], "then" .= object ["kind" .= ("client-reply" :: String)]]] Null)
  refused (channelReportWith [channelWithin "messages" (object ["kind" .= ("queue-in" :: String)]) False (-1) "late"] [] Null)
  refused (channelReportWith [channelWithin "messages" (object ["kind" .= ("queue-in" :: String)]) False 1 " "] [] Null)

  -- The verdict document names the differing channel, the violated pair and
  -- each channel's allowance.
  let document = objectOf (interfaceVerdictJson parsedWithin within)
  check (KM.lookup "mode" document == Just (String "interface-stream"))
  check (case KM.lookup "channel_difference" document of
    Just (Object row) -> KM.lookup "channel" row == Just (String "messages")
      && KM.lookup "occurrence" row == Just (Number 0)
    _ -> False)
  let causalDocument = objectOf (interfaceVerdictJson parsedCausal broken)
  check (case KM.lookup "causal_violation" causalDocument of
    Just (Object row) -> KM.member "observation" row && KM.member "first" row
      && KM.member "then" row && KM.member "reason" row
    _ -> False)
  let tailDocument = objectOf (interfaceVerdictJson parsedTail withinTail)
      pressRow = case KM.lookup "channel_tolerances" tailDocument of
        Just (Array rows) -> find (\row -> KM.lookup "channel" (objectOf row) == Just (String "presses")) rows
        _ -> Nothing
  check (fmap objectOf pressRow == Just (KM.fromList
    [ ("channel", String "presses"), ("time_driven", Bool False), ("tail_allowance", Number 1)
    , ("allowance_basis", String "declared"), ("allowance_reason", String "a press can land after the bound")
    , ("stock_events", Number 1), ("owned_events", Number 2), ("tail_used", Number 1) ]))

  -- The execution and digest conditions keep their verdicts.
  let incomplete = object
        [ "image_sha256" .= stockDigest, "complete" .= False
        , "events" .= (["message", "reply"] :: [Value])
        , "fetch_receipts" .= [object ["start" .= (16 :: Int), "end" .= (32 :: Int), "fetches" .= (12 :: Int)]]
        ]
  _ <- status firmware (report incomplete legacyOwned [control "wrong-reply" legacyWrong] Null) VerdictNoVerdict
  _ <- status firmware (report (runValues stockDigest 12 32 12 ["message", "reply"]) legacyOwned [control "wrong-reply" legacyWrong] Null) VerdictNoVerdict
  _ <- status firmware (report (runValues stockDigest 16 32 0 ["message", "reply"]) legacyOwned [control "wrong-reply" legacyWrong] Null) VerdictNoVerdict
  _ <- status firmware (report legacyStock (runValues ownedDigest 64 100 9 ["message", "reply"]) [control "wrong-reply" legacyWrong] Null) VerdictNoVerdict
  _ <- status firmware (report legacyStock legacyOwned [control "wrong-reply" (runValues wrongDigest 64 96 0 ["message", "wrong-reply"])] Null) VerdictNoVerdict
  _ <- status firmware (report legacyStock legacyOwned [] Null) VerdictNoVerdict
  _ <- status firmware (report legacyStock legacyOwned [control "wrong-reply" legacyOwned] Null) VerdictNoVerdict
  _ <- status firmware (without "retired_extents" legacy) VerdictNoVerdict
  _ <- status firmware (without "owned_extents" legacy) VerdictNoVerdict
  missingFirmware <- judge Nothing legacy
  check (verdictStatus missingFirmware == VerdictNotRun)
  otherFirmware <- judge (Just (Firmware "other.bin" wrongDigest)) legacy
  check (verdictStatus otherFirmware == VerdictNoVerdict)
  check (case parseInterfaceReport "{\"mode\":\"interface-stream\",\"schema\":1}" of Left _ -> True; Right _ -> False)

  putStrLn "interface stream judge behavioral tests passed"

check :: Bool -> IO ()
check yes = unless yes (fail "interface stream verdict differs")

judge :: Maybe Firmware -> Value -> IO Verdict
judge image value = snd <$> judgeWith image value Nothing

judgeWith :: Maybe Firmware -> Value -> Maybe VerdictStatus -> IO (InterfaceReport, Verdict)
judgeWith image value expected = do
  parsed <- either fail pure (parseInterfaceReport (L.toStrict (encode value)))
  let verdict = judgeInterfaceReport image (ReportDigest "report-digest") parsed
  case expected of
    Just wanted -> check (verdictStatus verdict == wanted)
    Nothing -> pure ()
  pure (parsed, verdict)

status :: Maybe Firmware -> Value -> VerdictStatus -> IO Verdict
status image value expected = snd <$> judgeWith image value (Just expected)

refused :: Value -> IO ()
refused value = check (case parseInterfaceReport (L.toStrict (encode value)) of
  Left _ -> True
  Right _ -> False)

toleranceOf :: String -> Verdict -> Maybe ChannelTolerance
toleranceOf label verdict =
  find ((== ChannelName (fromString label)) . toleranceChannel) (verdictChannelTolerances verdict)
  where fromString = K.toText . K.fromString

spanValue :: Int -> Int -> Value
spanValue start end = object ["start" .= start, "end" .= end]

report :: Value -> Value -> [Value] -> Value -> Value
report left right controls marker = object
  [ "schema" .= (1 :: Int)
  , "mode" .= ("interface-stream" :: String)
  , "subsystem" .= ("synthetic" :: String)
  , "scenario" .= ("round-trip" :: String)
  , "retired_extents" .= [spanValue 16 32]
  , "owned_extents" .= [spanValue 64 96]
  , "stock" .= left
  , "owned" .= right
  , "controls" .= controls
  , "milestone" .= marker
  ]

runValues :: String -> Int -> Int -> Int -> [Value] -> Value
runValues digest start end fetches events = object
  [ "image_sha256" .= digest, "complete" .= True, "events" .= events
  , "fetch_receipts" .= [object ["start" .= start, "end" .= end, "fetches" .= fetches]]
  ]

control :: String -> Value -> Value
control label value = object ["name" .= label, "run" .= value]

channel :: String -> Value -> Bool -> Value
channel label match driven = object
  [ "name" .= label, "match" .= match, "time_driven" .= driven
  , "sources" .= (["test"] :: [String]) ]

-- | A channel declaration with a tail allowance.
channelWithin :: String -> Value -> Bool -> Int -> String -> Value
channelWithin label match driven occurrences reason = case channel label match driven of
  Object value -> Object (KM.insert "tail_allowance"
    (object ["occurrences" .= occurrences, "reason" .= reason]) value)
  value -> value

causal :: Value -> Value -> Value
causal first then_ = object
  [ "first" .= first, "then" .= then_, "sources" .= (["test"] :: [String]) ]

withChannels :: [Value] -> [Value] -> Value -> Value
withChannels declared pairs (Object value) = Object (KM.insert "payload_projection"
  (object [ "schema" .= (1 :: Int), "messages" .= ([] :: [Value])
          , "channels" .= declared, "causal_pairs" .= pairs ]) value)
withChannels _ _ value = value

-- | A channel declaration with an empty source list.
withoutSource :: Value -> Value
withoutSource (Object value) = Object (KM.insert "sources" (Array mempty) value)
withoutSource value = value

channelReportWith :: [Value] -> [Value] -> Value -> Value
channelReportWith declared pairs marker = withChannels declared pairs $
  report (runValues stockDigest 16 32 12 [message 1]) (runValues ownedDigest 64 96 9 [message 1])
    [control "wrong" (runValues wrongDigest 64 96 8 [message 2])] marker

message :: Int -> Value
message operation = object ["kind" .= ("queue-in" :: String), "operation" .= operation]

reply :: Int -> Value
reply entry = object ["kind" .= ("client-reply" :: String), "entry" .= entry]

press :: Int -> Value
press entry = object ["kind" .= ("half-press" :: String), "entry" .= entry]

tick :: Int -> Value
tick n = object ["kind" .= ("tick" :: String), "n" .= n]

at :: String -> Value
at name = object ["milestone" .= name]

objectOf :: Value -> KM.KeyMap Value
objectOf (Object row) = row
objectOf _ = KM.empty

without :: String -> Value -> Value
without key (Object value) = Object (KM.delete (K.fromString key) value)
without _ value = value

isInfix :: String -> String -> Bool
isInfix needle haystack = any (prefix needle) (tails haystack)
  where
    prefix candidate text = take (length candidate) text == candidate
    tails [] = [[]]
    tails current@(_ : rest) = current : tails rest

stockDigest, ownedDigest, wrongDigest :: String
stockDigest = replicate 64 'a'
ownedDigest = replicate 64 'b'
wrongDigest = replicate 64 'c'
