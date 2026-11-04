{-# LANGUAGE OverloadedStrings #-}

module Main (main) where

import qualified A32 as A
import Control.Monad (unless)
import Data.Aeson (Value, object, (.=))
import Data.Bits (complement)
import qualified Data.ByteString as B
import Data.Char (isAsciiLower, isDigit)
import Data.Either (isLeft)
import Data.List (nub)
import qualified Data.Map.Strict as M
import Emulation.NativeObservation
import PropertyFields (PropertyId (..))
import Replacement.PropertyService.Classify hiding (GuestAddress (..))
import qualified Replacement.PropertyService.Classify as C
import Replacement.PropertyService.Oracle
import System.Exit (exitFailure)

check :: String -> Bool -> IO Bool
check label holds = do
  putStrLn ((if holds then "ok   " else "FAIL ") ++ label)
  pure holds

-- | Bank 1 offset 5 holds 0xb4 and bank 0 offsets 0x10-0x11 hold 0x34 0x12; previous offset 5 holds 0xff.
handFixture :: Fixture
handFixture = Fixture banks previous (B.replicate 0x200 0)
  where
    banks = B.concat [B.replicate 0x10 0, B.pack [0x34, 0x12], B.replicate (0x1c2 - 0x12) 0, B.replicate 5 0, B.singleton 0xb4, B.replicate (0x1c2 - 6) 0]
    previous = B.replicate 5 0 <> B.singleton 0xff <> B.replicate (0x1c2 - 6) 0

byteField :: SavePolicy -> FieldArm
byteField = FieldArm (BankOffset 5) Byte (BitRange 2 3) (FieldDefault 6)

halfField :: FieldArm
halfField = FieldArm (BankOffset 0x10) Half (BitRange 4 8) (FieldDefault 0) NoSave

byteAt :: B.ByteString -> Int -> Int
byteAt bytes at = fromIntegral (B.index bytes at)

handProperty :: FieldArm -> Property
handProperty arm = Property (PropertyId 1) (C.GuestAddress 0) (C.GuestAddress 0) (Owned (OwnedField arm))

-- | @mov rD, rS@ as the decoder returns it.
mov :: Int -> Int -> A.Instruction
mov destination source = A.DataProcessing (A.Cond 14) A.MOV False (A.Reg destination) (A.Reg 0) (A.ShiftedImmediate (A.Reg source) A.LSL 0)

-- | A word store into the caller's frame [0x9597f0, 0x959800).
callerWrite :: WriteEvent
callerWrite = WriteEvent 0x9597f4 4 0x11223344

baseRun :: Run
baseRun = Run Returned [(R0, 1), (Sp, 0x9597f0)] [(RegionId "banks", B.pack [0xab])] (Just [WriteEvent 0x96cc64 1 0xab])

observation :: Word -> RunnerStatus -> [(Register, Word)] -> Provenance -> NativeObservation
observation instructions status overrides provenance = NativeObservation
  { nativeStatus = status
  , nativeRegisters = M.fromList [(register, fromIntegral (M.findWithDefault 0 register (M.fromList overrides))) | register <- registerLabels]
  , nativeMemory = [(RegionId "banks", B.replicate 0x384 1), (RegionId "previous", B.replicate 0x1c2 2), (RegionId "config", B.replicate 0x200 3), (RegionId "stack", B.replicate 0x7f0 4), (RegionId "caller-frame", B.replicate 0x10 0)]
  , nativeProvenance = provenance
  , nativeInstructions = InstructionCount (fromIntegral instructions)
  , nativeEngineError = EngineError 0
  }

report :: [Value] -> Value
report events = object ["runner" .= object ["memory_provenance" .= events]]

event :: String -> String -> Int -> String -> Value
event kind address size value = object ["kind" .= kind, "address" .= address, "size" .= size, "value" .= value]

main :: IO ()
main = do
  let live bank = modelEffect handFixture (OwnedField (byteField SaveBank0Previous)) (Get bank LiveRead Slot0)
      setting bank choice = modelEffect handFixture (OwnedField (byteField SaveBank0Previous)) (Set bank Slot1 choice)
      banks = fixtureBanks handFixture
      previous = fixturePrevious handFixture
      labels = map (caseLabelText . caseLabel) caseGrid
      validId text = case text of
        first : rest -> isAsciiLower first && length rest <= 63 && all (\c -> isAsciiLower c || isDigit c || c == '-') rest
        [] -> False
      bank0 = B.take 0x1c2 (fixtureBanks fixture)
      bank1 = B.drop 0x1c2 (fixtureBanks fixture)
      windows bytes = [B.take 4 (B.drop at bytes) | at <- [0 .. B.length bytes - 4]]
      stackWrite = WriteEvent 0x9597ec 4 0x12345678
      bankWrite = WriteEvent 0x96cc69 1 0x0c
      writesOf = map (\(WriteEvent address width _) -> Write (GuestAddress (fromIntegral address)) (WriteBytes (fromIntegral width)))
      complete events = Complete (ProvenanceTotal (fromIntegral (length events))) (writesOf events)
      stockLike = runOf (observation 40 Returned [(R0, 1), (R12, 0xe9)] (complete [stackWrite, bankWrite])) [stackWrite, bankWrite]
      ownedLike = runOf (observation 90 Returned [(R0, 1), (R12, 0)] (complete [stackWrite, stackWrite, bankWrite])) [stackWrite, stackWrite, bankWrite]
      image = B.replicate 0x100 0 <> B.pack [4, 0xc0, 0xa0, 0xe1] <> B.replicate 0x100 0
      patchAt = image4Base + 0x100
      delegated = Classified (Delegated ExitAdjusts)
      owned3 = Classified (Owned (OwnedField (byteField NoSave)))
  results <- sequence
    [ check "the grid is 32 cases per id over ids 0..646" (length operations == 32 && length caseGrid == 20704 && map oracleProperty caseGrid == concatMap (replicate 32) propertyIds && propertyIds == map PropertyId [0 .. 646])
    , check "case labels are distinct runner ids" (length (nub labels) == length labels && all validId labels)
    , check "bank 1 is bank 0's complement and the images differ" (bank1 == B.map complement bank0 && bank0 /= B.take 0x1c2 (fixturePrevious fixture))
    , check "no 4-byte window of bank 0 repeats" (length (nub (windows bank0)) == length (windows bank0))
    , check "getter live read extracts bank 1's bits 2-4" (effectR0 (live Bank1) == 5 && effectBanks (live Bank1) == banks)
    , check "getter live read of bank 0 is 0" (effectR0 (live Bank0) == 0)
    , check "getter default read returns the default" (effectR0 (modelEffect handFixture (OwnedField (byteField NoSave)) (Get Bank1 DefaultRead Slot1)) == 6)
    , check "halfword field reads the little-endian container" (effectR0 (modelEffect handFixture (OwnedField halfField) (Get Bank0 LiveRead Slot0)) == 0x23)
    , check "bank-0 set with save copies the old bits, then inserts the value" (let e = setting Bank0 SetFieldMax in effectR0 e == 1 && byteAt (effectBanks e) 5 == 0x1c && byteAt (effectPrevious e) 5 == 0xe3)
    , check "bank-1 set does not save" (effectPrevious (setting Bank1 SetOne) == previous && byteAt (effectBanks (setting Bank1 SetOne)) (0x1c2 + 5) == 0xa4)
    , check "0x1affffff stores the default" (byteAt (effectBanks (setting Bank1 SetDefaultRequest)) (0x1c2 + 5) == 0xb8)
    , check "value above the field keeps only its low bits" (byteAt (effectBanks (setting Bank1 SetAboveMax)) (0x1c2 + 5) == 0xa0 && byteAt (effectBanks (setting Bank1 SetAllOnes)) (0x1c2 + 5) == 0xbc)
    , check "unassigned get and set return 0 and store nothing" (modelEffect handFixture OwnedUnassigned (Set Bank0 Slot0 SetOne) == Effect 0 banks previous)
    , check "ids outside the tables are modelled as unassigned" (modelled OutOfRange == Just OwnedUnassigned && modelled delegated == Nothing)
    , check "set values use the field mask, else 0x7f" (map (setValue owned3) [SetFieldMax, SetAboveMax] == [7, 8] && map (setValue delegated) [SetFieldMax, SetAboveMax, SetDefaultRequest] == [0x7f, 0x80, 0x1affffff])
    , check "an identical run has no difference" (null (compareRuns baseRun baseRun))
    , check "a different write value differs" (compareRuns baseRun baseRun {runWrites = Just [WriteEvent 0x96cc64 1 0xac]} == [WritesDiffer])
    , check "a truncated log fails" (compareRuns baseRun baseRun {runWrites = Nothing} == [SubjectTruncated])
    , check "r0, memory and status differences are reported" (map differenceKind (compareRuns baseRun baseRun {runStatus = FetchGuard, runRegisters = [(R0, 0), (Sp, 0x9597f0)], runMemory = [(RegionId "banks", B.pack [0])]}) == ["status", "register", "memory"])
    , check "a clobbered callee-saved register differs" (compareRuns baseRun {runRegisters = [(R0, 1), (R6, 0xa5a50006), (Sp, 0x9597f0)]} baseRun {runRegisters = [(R0, 1), (R6, 0x4), (Sp, 0x9597f0)]} == [RegisterDiffers R6 0xa5a50006 0x4])
    , check "writes into the caller's frame are compared" (fmap runWrites (runOf (observation 1 Returned [] (complete [callerWrite, stackWrite])) [callerWrite, stackWrite]) == Right (Just [callerWrite]))
    , check "stack writes, r12 and instruction counts are not compared" (case (stockLike, ownedLike) of (Right a, Right b) -> null (compareRuns a b); _ -> False)
    , check "a truncated observation has no writes" (fmap runWrites (runOf (observation 1 Returned [] (Truncated (ProvenanceTotal 5000))) []) == Right Nothing)
    , check "report writes must match the provenance" (isLeft (runOf (observation 1 Returned [] (complete [bankWrite])) [stackWrite]))
    , check "report writes are the write events in order" (reportWrites (report [event "read" "0x0096cc64" 1 "0x00000000000000ab", event "write" "0x0096cc69" 1 "0x000000000000000c", event "write" "0x009597ec" 4 "0x0000000012345678"]) == Right [bankWrite, stackWrite])
    , check "a patch replaces exactly its word" (applyPatches [Patch patchAt movR12FromR4 movR12Zero] image == Right (B.take 0x100 image <> B.pack [0, 0xc0, 0xa0, 0xe3] <> B.drop 0x104 image))
    , check "a patch refuses an unexpected word" (isLeft (applyPatches [Patch patchAt movR12Zero movR12FromR4] image))
    , check "mov r12, r4 and mov r12, #0 decode as such" (A.decode 0x62c4bc movR12FromR4 == A.DataProcessing (A.Cond 14) A.MOV False (A.Reg 12) (A.Reg 0) (A.ShiftedImmediate (A.Reg 4) A.LSL 0) && A.decode 0x62c4bc movR12Zero == A.DataProcessing (A.Cond 14) A.MOV False (A.Reg 12) (A.Reg 0) (A.Immediate 0))
    , check "branch words decode to their targets" (all (\target -> A.decode 0x62c4d8 (branchWord 0x62c4d8 target) == A.Branch (A.Cond 14) False target) [0x630200, 0x62ff60, 0x61e110])
    , check "the getter-r6 control words decode as mov r4, r12, mov r6, r12 and mov r12, r6" (map (A.decode 0x62c4b0) [movR4FromR12, movR6FromR12, movR12FromR6] == [mov 4 12, mov 6 12, mov 12 6])
    , check "a moved offset steps one container" (fmap snd (exposingField handFixture OffsetMoved [handProperty (byteField SaveBank0Previous)]) == Just (FieldArm (BankOffset 6) Byte (BitRange 2 3) (FieldDefault 6) SaveBank0Previous))
    , check "a flipped default returns 7" (fmap (\(_, arm) -> effectR0 (modelEffect handFixture (OwnedField arm) (Get Bank0 DefaultRead Slot0))) (exposingField handFixture DefaultFlipped [handProperty (byteField NoSave)]) == Just 7)
    , check "a cleared save bit stops the bank-0 copy" (fmap (\(_, arm) -> byteAt (effectPrevious (modelEffect handFixture (OwnedField arm) (Set Bank0 Slot0 SetOne))) 5) (exposingField handFixture SaveCleared [handProperty (byteField SaveBank0Previous)]) == Just 0xff)
    , check "clearing an absent save bit is not a control" (exposingField handFixture SaveCleared [handProperty (byteField NoSave)] == Nothing)
    , check "the split leaves the carrier to its own functions" (splitFunctions (0x62c4b0, 0x62d674) [FunctionSpan 0x62c4b0 0x62c4c4] == [FunctionSpan 0x61e110 0x624620, FunctionSpan 0x624694 0x62c4b0, FunctionSpan 0x62c4b0 0x62c4c4, FunctionSpan 0x62d674 0x6309c4])
    ]
  unless (and results) exitFailure
