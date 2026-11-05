{-# LANGUAGE GHC2021 #-}
module Main (main) where

import Control.Monad (unless)
import qualified Data.ByteString as B
import qualified Data.Map.Strict as M
import Replacement.RecordStore.Cases
import Replacement.RecordStore.Hardware
import Replacement.RecordStore.Model

assert :: String -> Bool -> IO ()
assert message condition = unless condition (error message)
main :: IO ()
main = do
  let g = geometry Kind3
      empty = initialStore Kind3 (mediumOf g [] []) NoFault MarkerCallback
      (status,written) = append hardware (Seed 0x9234) (CallerRecord (sourceBytes g)) empty
      persisted = medium (persistence written)
      (latest,_) = readLatest hardware written
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
      (accepted,_) = readLatest hardware rejected
  assert "rejected latest steps back" (fmap (sequenceOf . validRecord) accepted == Just (Sequence 10))
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
