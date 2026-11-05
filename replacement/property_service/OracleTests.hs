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
import Data.Word (Word32)
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

-- | Bytes at the given offsets of a zero image of the given size.
placed :: Int -> [(Int, Int)] -> B.ByteString
placed size bytes = B.pack [maybe 0 fromIntegral (lookup at bytes) | at <- [0 .. size - 1]]

-- | Banks: bank 0 0x10-0x11 hold 0x34 0x12, bank 1 offset 5 holds 0xb4, and slot-record byte 4 holds
-- 0x5a (bank 0 slot 0), 0x7e (bank 0 slot 0xffffffff), 0xc3 (bank 1 slot 2) and 0x91 (bank 1 slot 3).
-- Previous offsets 5 and 0x144 hold 0xff. Slot-defaults byte 4 holds 0x30 (slot 0), 0x90 (slot 2) and
-- 0xe0 (slot 0xffffffff); defaults-table byte 0xc holds 0x7f.
handFixture :: Fixture
handFixture =
  Fixture
    (placed 0x546 [(0x10, 0x34), (0x11, 0x12), (0x1c2 + 5, 0xb4), (0x144, 0x5a), (0x11c, 0x7e), (0x1c2 + 0x194, 0xc3), (0x1c2 + 0x1bc, 0x91)])
    (placed 0x1e0 [(5, 0xff), (0x144, 0xff)])
    (B.replicate 0x200 0)
    (placed 0xc8 [(0x2c, 0x30), (0x7c, 0x90), (0x04, 0xe0)])
    (placed 0x1c2 [(0xc, 0x7f)])

-- | Slot 1 selects bits 2-4 of bank offset 5 (default 6); other slots bits 4-7 of record byte 4.
slotArm :: BankDefault -> SlotArm
slotArm fallback = SlotArm (BankBranch (BankOffset 5) Byte (BitRange 2 3) fallback) (RecordBranch (RecordOffset 4) Byte (BitRange 4 4)) SaveBank0Previous

slotOwned :: Owned
slotOwned = OwnedSlotField (slotArm (BankConstant 6))

slot :: Word32 -> SlotArgument
slot = SlotArgument

byteField :: SavePolicy -> FieldArm
byteField = FieldArm (BankOffset 5) Byte (BitRange 2 3) (DefaultConstant 6)

halfField :: FieldArm
halfField = FieldArm (BankOffset 0x10) Half (BitRange 4 8) (DefaultConstant 0) NoSave

-- | Whole containers at bank offset 0x10 whose default is the defaults-table container at 0xc or 0xb.
tailByteField, tailHalfField :: FieldArm
tailByteField = FieldArm (BankOffset 0x10) Byte (BitRange 0 8) (DefaultTail (TailOffset 0xc) Byte) SaveBank0Previous
tailHalfField = FieldArm (BankOffset 0x10) Half (BitRange 0 16) (DefaultTail (TailOffset 0xb) Half) SaveBank0Previous

byteAt :: B.ByteString -> Int -> Int
byteAt bytes at = fromIntegral (B.index bytes at)

handProperty :: FieldArm -> Property
handProperty arm = Property (PropertyId 1) (C.GuestAddress 0) (C.GuestAddress 0) (Owned (OwnedField arm))

slotProperty :: Owned -> Property
slotProperty = Property (PropertyId 0x214) (C.GuestAddress 0) (C.GuestAddress 0) . Owned

third :: (a, b, c) -> c
third (_, _, c) = c

layoutOf :: SlotMutation -> Layout
layoutOf mutation = case mutation of
  MutatedLayout layout -> layout
  MutatedArm _ -> stockLayout

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
  let model owned operation = modelEffect handFixture owned operation
      r0Of owned operation = effectR0 <$> model owned operation
      live bank = model (OwnedField (byteField SaveBank0Previous)) (Get bank LiveRead (slot 0))
      setting bank choice = model (OwnedField (byteField SaveBank0Previous)) (Set bank (slot 1) choice)
      slotSet bank s choice = model slotOwned (Set bank (slot s) choice)
      r0Of' layout operation = effectR0 <$> modelEffectIn layout handFixture slotOwned operation
      byteIn select at effect = (\e -> byteAt (select e) at) <$> effect
      stockFixture = fixture B.empty
      banks = fixtureBanks handFixture
      previous = fixturePrevious handFixture
      labels = map (caseLabelText . caseLabel) caseGrid
      validId text = case text of
        first : rest -> isAsciiLower first && length rest <= 63 && all (\c -> isAsciiLower c || isDigit c || c == '-') rest
        [] -> False
      bank0 = B.take 0x1c2 (fixtureBanks stockFixture)
      bank1 = B.take 0x1c2 (B.drop 0x1c2 (fixtureBanks stockFixture))
      bank2 = B.drop 0x384 (fixtureBanks stockFixture)
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
      slotStanding = Classified (Owned slotOwned)
  results <- sequence
    [ check "the grid is 80 cases per id over ids 0..646" (length operations == 80 && length caseGrid == 51760 && map oracleProperty caseGrid == concatMap (replicate 80) propertyIds && propertyIds == map PropertyId [0 .. 646])
    , check "slot arguments are 0, 1, 2, 3 and 0xffffffff" (slotArguments == map slot [0, 1, 2, 3, 0xffffffff])
    , check "case labels are distinct runner ids" (length (nub labels) == length labels && all validId labels)
    , check "three banks: bank 1 is bank 0's complement, bank 2 a third stream" (bank1 == B.map complement bank0 && bank2 /= bank0 && bank2 /= bank1 && B.length bank2 == 0x1c2 && bank0 /= B.take 0x1c2 (fixturePrevious stockFixture))
    , check "the previous-value image covers slot 3's record" (B.length (fixturePrevious stockFixture) == 0x1e0)
    , check "no 4-byte window of bank 0 repeats" (length (nub (windows bank0)) == length (windows bank0))
    , check "getter live read extracts bank 1's bits 2-4" (fmap effectR0 (live Bank1) == Just 5 && fmap effectBanks (live Bank1) == Just banks)
    , check "getter live read of bank 0 is 0" (fmap effectR0 (live Bank0) == Just 0)
    , check "getter default read returns the default" (r0Of (OwnedField (byteField NoSave)) (Get Bank1 DefaultRead (slot 1)) == Just 6)
    , check "halfword field reads the little-endian container" (r0Of (OwnedField halfField) (Get Bank0 LiveRead (slot 0)) == Just 0x23)
    , check "bank-0 set with save copies the old bits, then inserts the value" (fmap effectR0 (setting Bank0 SetFieldMax) == Just 1 && byteIn effectBanks 5 (setting Bank0 SetFieldMax) == Just 0x1c && byteIn effectPrevious 5 (setting Bank0 SetFieldMax) == Just 0xe3)
    , check "bank-1 set does not save" (fmap effectPrevious (setting Bank1 SetOne) == Just previous && byteIn effectBanks (0x1c2 + 5) (setting Bank1 SetOne) == Just 0xa4)
    , check "0x1affffff stores the default" (byteIn effectBanks (0x1c2 + 5) (setting Bank1 SetDefaultRequest) == Just 0xb8)
    , check "value above the field keeps only its low bits" (byteIn effectBanks (0x1c2 + 5) (setting Bank1 SetAboveMax) == Just 0xa0 && byteIn effectBanks (0x1c2 + 5) (setting Bank1 SetAllOnes) == Just 0xbc)
    , check "unassigned get and set return 0 and store nothing" (model OwnedUnassigned (Set Bank0 (slot 0) SetOne) == Just (Effect 0 banks previous))
    , check "slot 1 reads the bank branch" (r0Of slotOwned (Get Bank1 LiveRead (slot 1)) == Just 5)
    , check "slots 0, 2, 3 and 0xffffffff read their record" (map (\(bank, s) -> r0Of slotOwned (Get bank LiveRead (slot s))) [(Bank0, 0), (Bank1, 2), (Bank1, 3), (Bank0, 0xffffffff)] == map Just [5, 0xc, 9, 7])
    , check "slot 1 default is the constant" (r0Of slotOwned (Get Bank0 DefaultRead (slot 1)) == Just 6)
    , check "record defaults come from the slot's defaults record" (map (\s -> r0Of slotOwned (Get Bank0 DefaultRead (slot s))) [0, 2, 0xffffffff] == map Just [3, 9, 0xe])
    , check "a tail-byte default reads 0x81b074 + 0xc" (r0Of (OwnedSlotField (slotArm (BankTailByte (TailOffset 0xc)))) (Get Bank1 DefaultRead (slot 1)) == Just 0x7f)
    , check "bank-0 record set saves the record bits at the record's previous offset" (byteIn effectBanks 0x144 (slotSet Bank0 0 SetFieldMax) == Just 0xfa && byteIn effectPrevious 0x144 (slotSet Bank0 0 SetFieldMax) == Just 0x5f && byteIn effectPrevious 5 (slotSet Bank0 0 SetFieldMax) == Just 0xff)
    , check "a record default request stores the slot's default" (byteIn effectBanks (0x1c2 + 0x194) (slotSet Bank1 2 SetDefaultRequest) == Just 0x93 && fmap effectPrevious (slotSet Bank1 2 SetDefaultRequest) == Just previous)
    , check "slot 1 set inserts into the bank branch" (byteIn effectBanks (0x1c2 + 5) (slotSet Bank1 1 SetFieldMax) == Just 0xbc)
    , check "set values use the bits the slot selects" (map (setValue slotStanding (slot 1)) [SetFieldMax, SetAboveMax] == [7, 8] && map (setValue slotStanding (slot 0)) [SetFieldMax, SetAboveMax] == [0xf, 0x10])
    , check "a read outside the fixture is not modelled" (model slotOwned (Get Bank0 DefaultRead (slot 0x10)) == Nothing)
    , check "ids outside the tables are modelled as unassigned" (modelled OutOfRange == Just OwnedUnassigned && modelled delegated == Nothing)
    , check "set values use the field mask, else 0x7f" (map (setValue owned3 (slot 0)) [SetFieldMax, SetAboveMax] == [7, 8] && map (setValue delegated (slot 0)) [SetFieldMax, SetAboveMax, SetDefaultRequest] == [0x7f, 0x80, 0x1affffff])
    , check "a model outside the fixture is a model difference" (modelDifferences handFixture slotOwned (Get Bank0 DefaultRead (slot 0x10)) baseRun == [ModelOutsideFixture])
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
    , check "a patch replaces exactly its word" (applyPatches [Patch patchAt 0xe1a0c004 0xe3a0c000] image == Right (B.take 0x100 image <> B.pack [0, 0xc0, 0xa0, 0xe3] <> B.drop 0x104 image))
    , check "a patch refuses an unexpected word" (isLeft (applyPatches [Patch patchAt 0xe3a0c000 0xe1a0c004] image))
    , check "branch words decode to their targets" (all (\target -> A.decode 0x62c4d8 (branchWord 0x62c4d8 target) == A.Branch (A.Cond 14) False target) [0x630200, 0x62ff60, 0x61e110])
    , check "a tail-byte default reads the defaults-table byte" (r0Of (OwnedField tailByteField) (Get Bank1 DefaultRead (slot 1)) == Just 0x7f)
    , check "a tail-halfword default reads the little-endian container" (r0Of (OwnedField tailHalfField) (Get Bank0 DefaultRead (slot 0)) == Just 0x7f00)
    , check "the tail-window variant gives offset 0's halfword the high byte 0x5a" (fmap effectR0 (modelEffect (variantFixture handFixture TailWindow) (OwnedField tailHalfField {fieldDefault = DefaultTail (TailOffset 0) Half}) (Get Bank0 DefaultRead (slot 0))) == Just 0x5a00)
    , check "a tail default request stores the defaults-table container" (byteIn effectBanks (0x1c2 + 0x10) (model (OwnedField tailByteField) (Set Bank1 (slot 0) SetDefaultRequest)) == Just 0x7f)
    , check "a moved tail offset reads the next defaults-table byte" (fmap snd (exposingField handFixture TailOffsetMoved [handProperty tailByteField]) == Just tailByteField {fieldDefault = DefaultTail (TailOffset 0xd) Byte})
    , check "a constant default has no tail offset to move" (exposingField handFixture TailOffsetMoved [handProperty (byteField NoSave)] == Nothing)
    , check "a moved offset steps one container" (fmap snd (exposingField handFixture OffsetMoved [handProperty (byteField SaveBank0Previous)]) == Just (FieldArm (BankOffset 6) Byte (BitRange 2 3) (DefaultConstant 6) SaveBank0Previous))
    , check "a flipped default returns 7" ((exposingField handFixture DefaultFlipped [handProperty (byteField NoSave)] >>= \(_, arm) -> r0Of (OwnedField arm) (Get Bank0 DefaultRead (slot 0))) == Just 7)
    , check "a cleared save bit stops the bank-0 copy" ((exposingField handFixture SaveCleared [handProperty (byteField SaveBank0Previous)] >>= \(_, arm) -> byteIn effectPrevious 5 (model (OwnedField arm) (Set Bank0 (slot 0) SetOne))) == Just 0xff)
    , check "clearing an absent save bit is not a control" (exposingField handFixture SaveCleared [handProperty (byteField NoSave)] == Nothing)
    , check "a moved record offset steps one byte" (fmap third (exposingSlot handFixture RecordOffsetMoved [slotProperty slotOwned]) == Just (MutatedArm (SlotArm (BankBranch (BankOffset 5) Byte (BitRange 2 3) (BankConstant 6)) (RecordBranch (RecordOffset 5) Byte (BitRange 4 4)) SaveBank0Previous)))
    , check "swapped branches exchange their bits" (fmap third (exposingSlot handFixture BranchesSwapped [slotProperty slotOwned]) == Just (MutatedArm (SlotArm (BankBranch (BankOffset 5) Byte (BitRange 4 4) (BankConstant 6)) (RecordBranch (RecordOffset 4) Byte (BitRange 2 3)) SaveBank0Previous)))
    , check "the slot-1 default from record 0 is 3" (fmap third (exposingSlot handFixture Slot1DefaultFromRecord0 [slotProperty slotOwned]) == Just (MutatedArm (slotArm (BankConstant 3))))
    , check "a record save at the bank offset lands at previous offset 5" (byteIn effectPrevious 5 (modelEffectIn stockLayout {layoutRecordSave = RecordSaveAtBankOffset} handFixture slotOwned (Set Bank0 (slot 0) SetOne)) == Just 0x5f && fmap (layoutRecordSave . layoutOf . third) (exposingSlot handFixture RecordSavedAtBankOffset [slotProperty slotOwned]) == Just RecordSaveAtBankOffset)
    , check "a moved slot-defaults base reads the next record" (r0Of' (stockLayout {layoutSlotDefaults = 0x7cb82c}) (Get Bank0 DefaultRead (slot 0)) == Just 0 && fmap (layoutSlotDefaults . layoutOf . third) (exposingSlot handFixture SlotDefaultsMoved [slotProperty slotOwned]) == Just 0x7cb82c)
    , check "a moved tail base needs a tail-byte default" (exposingSlot handFixture TailDefaultsMoved [slotProperty slotOwned] == Nothing && fmap (layoutTailDefaults . layoutOf . third) (exposingSlot handFixture TailDefaultsMoved [slotProperty (OwnedSlotField (slotArm (BankTailByte (TailOffset 0xc))))]) == Just 0x81b075)
    , check "movw and movt r12 decode to the address halves" (A.decode 0x62c4b0 (movwWord 0x7cb82c) == A.MoveWide (A.Cond 14) (A.Reg 12) 0xb82c && A.decode 0x62c4b4 (movtWord 0x7cb82c) == A.MoveTop (A.Cond 14) (A.Reg 12) 0x7c)
    , check "the split leaves the carrier to its own functions" (splitFunctions (0x62c4b0, 0x62d674) [FunctionSpan 0x62c4b0 0x62c4c4] == [FunctionSpan 0x61e110 0x624620, FunctionSpan 0x624694 0x62c4b0, FunctionSpan 0x62c4b0 0x62c4c4, FunctionSpan 0x62d674 0x6309c4])
    ]
  unless (and results) exitFailure
