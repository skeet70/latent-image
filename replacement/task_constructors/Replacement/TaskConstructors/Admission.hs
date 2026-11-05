module Replacement.TaskConstructors.Admission
  ( CodeAddress (..), PackedOffset (..), Constructor (..), constructors, symbolStem
  , admitCode, admitDescriptorFrame
  ) where

import Control.Monad (forM_)
import qualified A32 as A
import Control.Monad (unless)
import qualified Data.ByteString as B
import qualified Data.Set as S
import Data.Word (Word32)
import qualified FujiAF.UpdateContainer as U
import Numeric (showHex)

newtype CodeAddress = CodeAddress Word32 deriving (Eq, Ord, Show)
newtype PackedOffset = PackedOffset Word32 deriving (Eq, Ord, Show)
data Constructor = Constructor { constructorStart :: CodeAddress, constructorEnd :: CodeAddress } deriving (Eq, Show)

-- x70view 55436fc85c0560dd252b0410af6f2427fe198cd3 function-ranges.json.
constructors :: [Constructor]
constructors = [Constructor (CodeAddress low) (CodeAddress high) | (low, high) <-
  [(0xd80d44,0xd80dd4),(0xec5350,0xec53c8),(0xdca604,0xdca6b0),(0xdb63e0,0xdb6458)
  ,(0xed10b4,0xed112c),(0xef8c90,0xef8db4),(0xed2b00,0xed2b78),(0x1055d10,0x1055d84)
  ,(0x69f000,0x69f080),(0x10726f4,0x107277c),(0x1066280,0x1066304),(0x1062090,0x1062114)
  ,(0xd0a238,0xd0a2b0)]]

symbolStem :: Constructor -> String
symbolStem (Constructor (CodeAddress start) _) = showHex start ""

-- Layout/control-flow admission; effects and calling conventions require native qualification.
admitCode :: Constructor -> B.ByteString -> [CodeAddress] -> Either String ()
admitCode (Constructor (CodeAddress start) (CodeAddress end)) bytes services = walk S.empty [start]
  where
    require condition message = unless condition (Left message)
    inside pc = pc >= start && pc < end && pc `mod` 4 == 0
    walk _ [] = Right ()
    walk seen (pc:pending)
      | S.member pc seen = walk seen pending
      | otherwise = do
          require (inside pc) "constructor control flow leaves its carrier"
          let instruction = A.decode pc (U.word32At bytes (fromIntegral (pc-start)))
              next = pc+4
              conditional (A.Cond condition) destinations = if condition == 14 then destinations else next:destinations
              ordinary = Right [next]
          successors <- case instruction of
            A.Rejected reason -> Left ("constructor instruction rejected: " ++ show reason)
            A.Branch condition linked target -> do
              require (inside target || (linked && CodeAddress target `elem` services)) "constructor branch target is not admitted"
              pure (if linked then next : [target | inside target] else conditional condition [target])
            A.BranchExchange condition False register | register == A.lr -> pure (conditional condition [])
            A.BranchExchange (A.Cond 14) True (A.Reg 8)
              | start == 0xef8c90 && pc == 0xef8da8 -> pure [next]
            A.BranchExchange {} -> Left "constructor indirect branch is not admitted"
            A.LoadMultiple condition base A.IncrementAfter True registers | base == A.sp && A.pc `elem` registers -> pure (conditional condition [])
            A.LoadMultiple _ _ _ _ registers | A.pc `elem` registers -> Left "constructor computed return is not admitted"
            A.Load _ _ _ destination _ _ _ | destination == A.pc -> Left "constructor load into PC is not admitted"
            A.DataProcessing _ _ _ destination _ _ | destination == A.pc -> Left "constructor computed PC is not admitted"
            A.MoveWide _ destination _ | destination == A.pc -> Left "constructor PC write is not admitted"
            A.MoveTop _ destination _ | destination == A.pc -> Left "constructor PC write is not admitted"
            _ -> ordinary
          walk (S.insert pc seen) (successors ++ pending)

-- SP/FP-relative stores leave the descriptor tail at its stock entry-SP offset.
admitDescriptorFrame :: Constructor -> B.ByteString -> Either String ()
admitDescriptorFrame (Constructor (CodeAddress start) _) bytes = do
  let require condition message = unless condition (Left message)
      decode offset = A.decode (start + fromIntegral offset) (U.word32At bytes offset)
      instructions = [(offset,decode offset) | offset <- [0,4..B.length bytes-4]]
      expectedFrame = if start == 0xef8c90 then 64 else 48
  require (B.length bytes >= 20 && B.length bytes `mod` 4 == 0) "constructor code cannot hold a descriptor frame"
  saved <- case decode 0 of
    A.StoreMultiple (A.Cond 14) base A.DecrementBefore True registers
      | base == A.sp && A.lr `elem` registers && A.Reg 11 `elem` registers -> Right (4 * length registers)
    _ -> Left "constructor entry does not save its frame"
  fpAdjustment <- case decode 4 of
    A.DataProcessing (A.Cond 14) A.ADD False (A.Reg 11) base (A.Immediate amount)
      | base == A.sp -> Right (fromIntegral amount)
    A.DataProcessing (A.Cond 14) A.MOV False (A.Reg 11) _ (A.ShiftedImmediate base A.LSL 0)
      | base == A.sp -> Right 0
    _ -> Left "constructor frame pointer is not established"
  allocated <- case decode 8 of
    A.DataProcessing (A.Cond 14) A.SUB False destination base (A.Immediate amount)
      | destination == A.sp && base == A.sp -> Right (fromIntegral amount)
    _ -> Left "constructor descriptor stack allocation differs"
  require (saved + allocated == expectedFrame) "constructor descriptor frame size differs"
  require (saved <= expectedFrame-28) "constructor entry push overwrites its retained descriptor tail"
  let epilogue = if fpAdjustment == 0
        then A.DataProcessing (A.Cond 14) A.MOV False A.sp (A.Reg 0) (A.ShiftedImmediate (A.Reg 11) A.LSL 0)
        else A.DataProcessing (A.Cond 14) A.SUB False A.sp (A.Reg 11) (A.Immediate (fromIntegral fpAdjustment))
  require (decode (B.length bytes-8) == epilogue) "constructor frame restoration differs"
  let submissions = [offset | (offset,A.Branch (A.Cond 14) True 0x725dd8) <- instructions]
      auxiliary = 28 - expectedFrame
      overlaps address size = address < auxiliary+4 && address+size > auxiliary
        || (start == 0x69f000 && address < auxiliary && address+size > auxiliary-4)
  case submissions of
    [offset] -> require (decode (offset-4) == A.DataProcessing (A.Cond 14) A.ADD False (A.Reg 0) A.sp (A.Immediate 4))
      "constructor submission descriptor is not SP+4"
    _ -> Left "constructor does not have exactly one submission call"
  forM_ instructions $ \(offset,instruction) -> case instruction of
    A.Store _ width _ base (A.OffsetImmediate displacement) A.OffsetOnly
      | base == A.sp || base == A.Reg 11 -> do
          let address = (if base == A.sp then negate expectedFrame else negate saved + fpAdjustment) + fromIntegral displacement
              size = case width of A.Byte -> 1; A.Half -> 2; A.Word -> 4; A.Dual -> 8
          require (not (overlaps address size)) "constructor initializes a retained descriptor tail word"
    A.Store _ _ _ base _ _ | base == A.sp || base == A.Reg 11 -> Left "constructor indexed stack store is not admitted"
    A.StoreMultiple {} | offset /= 0 -> Left "constructor additional stack push is not admitted"
    A.DataProcessing _ _ _ destination _ _
      | destination == A.sp && offset /= 8 -> require (offset == B.length bytes-8) "constructor changes SP inside its descriptor lifetime"
      | destination == A.Reg 11 && offset /= 4 -> Left "constructor changes its frame pointer"
    A.Load _ _ _ destination base _ indexing
      | destination `elem` [A.sp,A.Reg 11] || (base `elem` [A.sp,A.Reg 11] && indexing /= A.OffsetOnly) ->
          Left "constructor load changes its stack registers"
    A.LoadMultiple _ _ _ _ registers
      | any (`elem` [A.sp,A.Reg 11]) registers -> require (offset == B.length bytes-4) "constructor stack restore occurs before its epilogue"
    A.MoveWide _ destination _ | destination `elem` [A.sp,A.Reg 11] -> Left "constructor changes its stack registers"
    A.MoveTop _ destination _ | destination `elem` [A.sp,A.Reg 11] -> Left "constructor changes its stack registers"
    A.Compute _ destinations | any (`elem` [A.sp,A.Reg 11]) destinations -> Left "constructor changes its stack registers"
    _ -> pure ()
