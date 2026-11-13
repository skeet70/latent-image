{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}
module Main (main) where

import Control.Monad (unless)
import Data.Aeson (Value (..), encode, object, (.=))
import qualified Data.Aeson.KeyMap as KM
import qualified Data.Aeson.Key as K
import qualified Data.ByteString.Lazy as L
import Emulation.InterfaceJudge
import Emulation.Judge

main :: IO ()
main = do
  let firmware = Just (Firmware "stock.bin" stockDigest)
      stock = run stockDigest True 16 32 12 ["message", "reply"]
      owned = run ownedDigest True 64 96 9 ["message", "reply"]
      wrong = run wrongDigest True 64 96 8 ["message", "wrong-reply"]
      report :: Value -> Value -> [Value] -> Value -> Value
      report left right controls marker = object
        [ "schema" .= (1 :: Int), "mode" .= ("interface-stream" :: String)
        , "subsystem" .= ("synthetic" :: String), "scenario" .= ("round-trip" :: String)
        , "retired_extents" .= [spanValue 16 32], "owned_extents" .= [spanValue 64 96]
        , "stock" .= left, "owned" .= right, "controls" .= controls, "milestone" .= marker
        ]
      control value = object ["name" .= ("wrong-reply" :: String), "run" .= value]
      complete = report stock owned [control wrong] Null
      judge image value = do
        parsed <- either fail pure (parseInterfaceReport (L.toStrict (encode value)))
        pure (judgeInterfaceReport image (ReportDigest "report-digest") parsed)
      status value expected = do
        verdict <- judge firmware value
        check (verdictStatus verdict == expected)
        pure verdict
  equal <- status complete VerdictEqual
  check (verdictOwned equal == Just ownedDigest && verdictFirmware equal == Just stockDigest)
  check (null (verdictUnmet equal) && null (verdictSurvivors equal))
  different <- status (report stock wrong [control wrong] Null) VerdictDifferent
  check (fmap differenceAt (verdictFirstDifference different) == Just (ObservationIndex 1))
  check (maybe False ((== Just (Observation (String "reply"))) . differenceStock) (verdictFirstDifference different))
  _ <- status (report stock (run ownedDigest True 64 96 9 ["message", "reply", "extra"]) [control wrong] Null) VerdictDifferent
  let marker = object ["name" .= ("message-received" :: String), "observation" .= (0 :: Int)]
  milestoneVerdict <- status (report stock (run ownedDigest True 64 96 9 ["message", "reply", "extra"]) [control wrong] marker) VerdictEqualUpToMilestone
  check (verdictMilestone milestoneVerdict == Just (Milestone "message-received" (ObservationIndex 0)))
  _ <- status (report stock owned [control (run wrongDigest True 64 96 8 ["message", "reply", "extra"])] marker) VerdictNoVerdict
  _ <- status (report stock (run ownedDigest True 64 96 9 ["different", "reply"]) [control wrong] marker) VerdictDifferent
  _ <- status (report stock (run ownedDigest True 64 96 9 []) [control wrong] marker) VerdictDifferent
  _ <- status (report (run stockDigest True 16 32 0 ["message", "reply"]) owned [control wrong] Null) VerdictNoVerdict
  _ <- status (report stock (run ownedDigest True 64 96 0 ["message", "reply"]) [control wrong] Null) VerdictNoVerdict
  _ <- status (report stock owned [control (run wrongDigest True 64 96 0 ["wrong"])] Null) VerdictNoVerdict
  _ <- status (report stock owned [] Null) VerdictNoVerdict
  survived <- status (report stock owned [control owned] Null) VerdictNoVerdict
  check (verdictSurvivors survived == ["wrong-reply"])
  _ <- status (report (run stockDigest False 16 32 12 ["message", "reply"]) owned [control wrong] Null) VerdictNoVerdict
  _ <- status (report (run stockDigest True 12 32 12 ["message", "reply"]) owned [control wrong] Null) VerdictNoVerdict
  _ <- status (report stock (run ownedDigest True 64 100 9 ["message", "reply"]) [control wrong] Null) VerdictNoVerdict
  _ <- status (report stock owned [control wrong] Null `without` "retired_extents") VerdictNoVerdict
  _ <- status (report stock owned [control wrong] Null `without` "owned_extents") VerdictNoVerdict
  missingFirmware <- judge Nothing complete
  check (verdictStatus missingFirmware == VerdictNotRun)
  otherFirmware <- judge (Just (Firmware "other.bin" wrongDigest)) complete
  check (verdictStatus otherFirmware == VerdictNoVerdict)
  check (case parseInterfaceReport "{\"mode\":\"interface-stream\",\"schema\":1}" of Left _ -> True; Right _ -> False)
  putStrLn "interface stream judge behavioral tests passed"

check :: Bool -> IO ()
check yes = unless yes (fail "interface stream verdict differs")

spanValue :: Int -> Int -> Value
spanValue start end = object ["start" .= start, "end" .= end]

run :: String -> Bool -> Int -> Int -> Int -> [String] -> Value
run digest complete start end fetches events = object
  [ "image_sha256" .= digest, "complete" .= complete, "events" .= events
  , "fetch_receipts" .= [object ["start" .= start, "end" .= end, "fetches" .= fetches]]
  ]

without :: Value -> K.Key -> Value
without (Object value) key = Object (KM.delete key value)
without value _ = value

stockDigest, ownedDigest, wrongDigest :: String
stockDigest = replicate 64 'a'
ownedDigest = replicate 64 'b'
wrongDigest = replicate 64 'c'
