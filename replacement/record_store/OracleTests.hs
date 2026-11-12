{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}
module Main (main) where

import Control.Monad (unless)
import Data.Aeson (Value(..), object, toJSON, (.=))
import Data.String (fromString)
import qualified Data.Text as T
import qualified Data.ByteString as B
import qualified Data.Map.Strict as M
import Emulation.NativeObservation
import Emulation.Sha256 (sha256, sha256Hex)
import Replacement.RecordStore.CachedObservation
import Replacement.RecordStore.Cases
import Replacement.RecordStore.Hardware
import Replacement.RecordStore.Model

assert :: String -> Bool -> IO ()
assert message condition = unless condition (error message)
main :: IO ()
main = do
  cachedObservationTests
  sequenceTests
  let g = geometry Kind3
      empty = initialStore Kind3 (mediumOf g [] []) NoFault MarkerCallback
      (status,written) = append hardware (Seed 0x9234) (CallerRecord (sourceBytes g)) empty
      persisted = medium (persistence written)
      (latest,_) = readLatest WriteRecoveryCount hardware written
  assert "empty-bank seed remains unmasked" (status == 0 && sequenceOf (recordAt g Bank0 (Position 0) persisted) == Sequence 0x9234)
  assert "latest returns stored record" (fmap (sequenceOf . validRecord) latest == Just (Sequence 0x9234))
  assert "kind3 program retains fresh-open tail" (case requests (persistence written) of [Program (PoolBlock 0x58) 0 payload] -> B.length payload == 0x600 && B.all (==0xff) (B.drop 0x34 payload); _ -> False)
  let full = initialStore Kind3 (mediumOf g [(fromIntegral p,False) | p <- [0..126::Int]] []) NoFault NoCallback
      (rotatedStatus,rotated) = append hardware (Seed 0) (CallerRecord (sourceBytes g)) full
  assert "full bank rotates without erase of an empty destination" (rotatedStatus == 0 && case requests (persistence rotated) of [Program (PoolBlock 0x59) 0 payload] -> sequenceOf payload == Sequence 127; _ -> False)
  let wrapped = initialStore Kind3 (mediumOf g [(0x7fff,False)] []) NoFault NoCallback
      (_,masked) = append hardware (Seed 0) (CallerRecord (sourceBytes g)) wrapped
  assert "occupied predecessor successor masks bit15" (case requests (persistence masked) of [Program _ 0x34 payload] -> sequenceOf payload == Sequence 0; _ -> False)
  let rejected = initialStore Kind3 (mediumOf g [(10,False),(11,True)] []) NoFault MarkerCallback
      (accepted,_) = readLatest WriteRecoveryCount hardware rejected
  assert "rejected latest steps back" (fmap (sequenceOf . validRecord) accepted == Just (Sequence 10))
  let bothFull = initialStore Kind3 (mediumOf g [(fromIntegral p,False) | p <- [0..126::Int]] [(fromIntegral p,False) | p <- [127..253::Int]]) NoFault MarkerCallback
      (_,afterAppend) = append hardware (Seed 0) (CallerRecord (sourceBytes g)) bothFull
      (_,withCount) = readLatest WriteRecoveryCount hardware bothFull
      sequences state = [sequenceOf before | CallbackEvent Check before _ _ <- callbackEvents state]
  assert "append omits the recovery-count following-bank check" (sequences afterAppend == map Sequence [126,253,126,253])
  assert "requested recovery count checks the following bank" (sequences withCount == map Sequence [126,253,126,126,253])
  let (_,count,collected,_) = collect initialCaller rejected
  assert "collect keeps rejected occupied records" (count == 2 * 0x34 && B.index collected (0x34+2) == 0xee)
  assert "collect leaves caller bytes past both bank chunks unchanged" (B.length collected == callerSize && B.drop (2 * 127 * 0x34) collected == B.replicate (callerSize - 2 * 127 * 0x34) 0xa5)
  let original = B.replicate 0x34 0
      (_,sealed) = hardware Prepare Hardware3 (replace 0 (B.singleton 1) original)
  assert "row and column parity of one low bit" (B.take 8 (B.drop 0x2c sealed) == B.pack [1,0,0,0,1,0,0,0])
  let corrupted = replace 0 (B.singleton 0) sealed
  assert "one-bit data error corrects to sealed record" (hardware Check Hardware3 corrupted == (Accept,sealed))
  assert "erased medium defaults to ff" (bankBytes g Bank0 (Medium M.empty) == B.replicate blockBytes 0xff)
  putStrLn "record-store oracle model tests passed"

sequenceTests :: IO ()
sequenceTests = do
  let expected label = case [model hardware c | c <- sequenceCases, caseName (name c) == "sequence-" ++ label] of
        [result] -> result
        _ -> error ("missing sequence " ++ label)
      snapshot label event = B.take 16 (B.drop (0x100 + event * eventStride + 64) (expectedResults (expected label)))
      status label event = B.take 4 (B.drop (0x100 + event * eventStride) (expectedResults (expected label)))
      a = B.take 16 (record (geometry Kind0) 10 False)
      b = B.take 16 (record (geometry Kind1) 11 False)
      c = B.take 16 (record (geometry Kind2) 12 False)
  assert "original generated grid retains 380 identities with recovery and hardware cases" (length caseGrid + length [() | row <- caseGrid, StopAfter _ <- [injection row]] + 5 == 380)
  assert "multi-kind nofault returns A after B" (snapshot "ab-a-nofault" 3 == a)
  assert "resident A survives failure of its second physical read" (snapshot "ab-a-second-a-fails" 1 == a && snapshot "ab-a-second-a-fails" 2 == b && snapshot "ab-a-second-a-fails" 3 == a && status "ab-a-second-a-fails" 3 == B.replicate 4 0)
  assert "failed fill publishes unchanged slot bytes without retry" (snapshot "first-fill-failure-memoized" 1 == B.replicate 16 0 && snapshot "first-fill-failure-memoized" 2 == B.replicate 16 0)
  assert "both banks remain resident on A0 A1 A0" (snapshot "a0-a1-a0-second-a0-fails" 3 == a)
  assert "three kinds evict A and failed refill exposes prior B bytes" (snapshot "three-kind-evicts-a" 4 == b)
  assert "equal ages evict slot zero" (snapshot "equal-age-tie-evicts-slot-zero" 5 == b)
  assert "unsigned wrapped age changes eviction" (snapshot "unsigned-age-wrap" 7 == c)
  let checks = [sequenceOf before | CallbackEvent Check before _ _ <- callbackEvents (expectedStore (expected "latest-check-order"))]
  assert "latest checks rejected predecessor and final selected bank in order" (take 6 checks == map Sequence [11,10,20,11,10,20])

cachedObservationTests :: IO ()
cachedObservationTests = do
  let runnerDigest = sha256 (B.pack [1,2,3])
      runner = object
        [ "status" .= ("returned" :: String)
        , "unicorn_error" .= (0 :: Int)
        , "instructions" .= ("1" :: String)
        , "memory_provenance_total" .= ("0" :: String)
        , "registers" .= object [fromString (registerLabel register) .= hexWord32 (if register == R0 then 42 else 0) | register <- registerLabels]
        , "memory" .= ([] :: [Value])
        , "memory_provenance" .= ([] :: [Value])
        ]
      report identity = object (identity ++ ["runner_exit" .= (0 :: Int), "runner" .= runner])
      admit = observeCachedStock runnerDigest []
      matching = report ["runner_sha256" .= sha256Hex runnerDigest]
      expectRefusal label expected value = case admit value of
        Left actual -> assert label (actual == expected)
        Right _ -> error label
  case admit matching of
    Left problem -> error ("matching cached runner refused: " ++ show problem)
    Right observed -> assert "matching runner reuses returned observation" (nativeStatus observed == Returned && lookupRegister R0 observed == Just 42)
  expectRefusal "different runner cannot reuse cached observations" RunnerIdentityMismatch
    (report ["runner_sha256" .= sha256Hex (sha256 (B.pack [4,5,6]))])
  expectRefusal "missing runner identity cannot reuse cached observations" MissingRunnerIdentity (report [])
  mapM_ (\identity -> expectRefusal "malformed runner identity cannot reuse cached observations" MalformedRunnerIdentity
    (report ["runner_sha256" .= identity]))
    [toJSON (7 :: Int), String "", String (T.replicate 64 "g"), String (T.replicate 64 "A"), String (T.replicate 63 "0")]
