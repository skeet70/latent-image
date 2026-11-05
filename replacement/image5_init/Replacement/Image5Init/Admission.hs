module Replacement.Image5Init.Admission
  ( AdmissionError(..), FrameProof, CallSite(..), deriveFrameProof, proofCallSites, returnInstruction ) where

import Prelude hiding (reads)
import Control.Monad (unless)
import Data.Bits
import Data.Int (Int32)
import qualified Data.ByteString as B
import qualified Data.Map.Strict as M
import Data.Word (Word32)
import qualified FujiAF.UpdateContainer as U

data AdmissionError = CarrierRejected | UnsupportedInstruction | StackRejected | MemoryEffectRejected | CallOrderRejected | ArgumentRejected | CalleeSavedRejected | ReturnRejected | CheckRejected | HookRejected deriving (Eq, Show)
data CallSite = CallSite { callInstruction :: Word32, callTarget :: Word32, callReturn :: Word32 } deriving (Eq, Show)
data FrameProof = FrameProof [CallSite] Word32 deriving (Eq, Show)
proofCallSites :: FrameProof -> [CallSite]
proofCallSites (FrameProof sites _) = sites
returnInstruction :: FrameProof -> Word32
returnInstruction (FrameProof _ pc) = pc

data Scalar = Incoming Int | Constant Word32 | Stack Int | Result Int | SavedReturn Word32 deriving (Eq, Show)
data Machine = Machine { regs :: M.Map Int Scalar, memory :: M.Map Int Scalar, sites :: [CallSite], reads :: [Word32], hooks :: [Word32], zero :: Maybe Bool, frames :: Int } deriving (Eq, Show)
get :: Machine -> Int -> Scalar
get m r = M.findWithDefault (Incoming r) r (regs m)
put :: Machine -> Int -> Scalar -> Machine
put m r v = m { regs = M.insert r v (regs m) }
require :: Bool -> AdmissionError -> Either AdmissionError ()
require b e = unless b (Left e)
targets :: [Word32]
targets = [0x72872c,0x6c62a8,0x73aca4,0xed4acc,0xed4afc,0x69cf84,0,0xd986b0,0xd80d44,0x1056c24,0xdb01fc,0xec5350,0xdca604,0xdb63e0,0xed10b4,0xef8c90,0xed2b00,0x1055d10,0x6b7df8,0x69f000,0x72872c]
checked :: [Int]
checked = [4,9,10,12,13,14,15,17,18,19,20]
arguments :: [(Int,Word32)]
arguments = [(1,0x1010100),(3,7),(6,0xed4a8c),(10,0),(11,1),(16,0),(21,0x11010100)]

deriveFrameProof :: B.ByteString -> Either AdmissionError FrameProof
deriveFrameProof bytes = do
  require (B.length bytes >= 60 && B.length bytes <= 324 && B.length bytes `mod` 4 == 0) CarrierRejected
  require (all (==0xe320f000) [word pc | pc <- [0xd9856c,0xd98570..0xd985a0]]) CarrierRejected
  proof <- run Nothing
  mapM_ run (map Just checked)
  pure proof
  where
    end = 0xd9856c + fromIntegral (B.length bytes)
    word :: Word32 -> Word32
    word pc = U.word32At bytes (fromIntegral (pc-0xd9856c))
    initial = Machine (M.fromList [(13,Stack 0),(14,SavedReturn 0x69d2e4)]) M.empty [] [] [] Nothing 0
    run failure = walk failure 0 [] 0xd985a4 initial
    walk failure fuel seen pc m = do
      require (fuel < (1000 :: Int) && pc >= 0xd985a4 && pc < end && pc .&. 3 == 0) UnsupportedInstruction
      if (pc,m) `elem` seen then do
        require (failure == Just (length (sites m)) && frames m == 0 && word pc .&. 0x0fffffff == 0x0afffffe) CheckRejected
        pure (FrameProof (sites m) pc)
      else step failure fuel ((pc,m):seen) pc m
    step failure fuel seen pc m = do
      let w = word pc
          cond = w `shiftR` 28
          r = fromIntegral ((w `shiftR` 12) .&. 15)
          rn = fromIntegral ((w `shiftR` 16) .&. 15)
          next = walk failure (fuel+1) seen (pc+4)
          jump = walk failure (fuel+1) seen
          reglist = [i | i <- [0..15], testBit w i]
          imm = rotateR (w .&. 255) (fromIntegral (((w `shiftR` 8) .&. 15)*2))
          wide = ((w `shiftR` 4) .&. 0xf000) .|. (w .&. 0xfff)
      execute <- case cond of
        14 -> pure True
        0 -> maybe (Left CheckRejected) pure (zero m)
        1 -> maybe (Left CheckRejected) (pure . not) (zero m)
        _ -> Left UnsupportedInstruction
      if not execute then next m else
        if w .&. 0x0fff0000 == 0x092d0000 then do
          base <- case get m 13 of Stack n -> pure n; _ -> Left StackRejected
          require ((frames m == 0 && null (sites m) && reglist == [11,14] && base == 0) || (frames m == 1 && base == -8 && length reglist == 4 && 14 `elem` reglist && 11 `elem` reglist)) StackRejected
          let bottom = base-4*length reglist
          next ((put m 13 (Stack bottom)) { memory = foldr (uncurry M.insert) (memory m) (zip [bottom,bottom+4..] (map (get m) reglist)) })
        else if w .&. 0x0fff0000 == 0x08bd0000 then do
          base <- case get m 13 of Stack n -> pure n; _ -> Left StackRejected
          vals <- mapM (\offset -> maybe (Left StackRejected) pure (M.lookup offset (memory m))) (take (length reglist) [base,base+4..])
          let restored = foldl (\state (reg,v) -> put state reg v) (put m 13 (Stack (base+4*length reglist))) (zip reglist vals)
          case lookup 15 (zip reglist vals) of
            Just (SavedReturn dest) | frames m == 1 -> do
              require (get restored 13 == Stack (-8) && all (\i -> get restored i == Incoming i) [4..10] && reads m == [0xbf1a2c,0xbf1a28,0xbf1a24,0xbf1a20] && hooks m == [0x107277c,0x1066304,0x1062114]) HookRejected
              jump dest (restored { frames = 0 })
            Just (SavedReturn 0x69d2e4) -> do
              require (failure == Nothing && length (sites m) == 21 && get restored 0 == Result 21 && get restored 13 == Stack 0 && all (\i -> get restored i == Incoming i) [4..11]) ReturnRejected
              pure (FrameProof (sites m) pc)
            _ -> Left ReturnRejected
        else if w .&. 0x0e000000 == 0x0a000000 then do
          let off = fromIntegral ((fromIntegral (w .&. 0xffffff) `shiftL` 8 :: Int32) `shiftR` 6)
              dest = pc+8+off
          if testBit w 24 then do
            let n = length (sites m)+1
            require (n <= 21 && frames m == 0 && (if n == 7 then dest >= 0xd985a4 && dest < end else dest == targets !! (n-1))) CallOrderRejected
            require (get m 13 == Stack (-8) && get m 11 == Stack (-8)) StackRejected
            require (all (\i -> get m i == Incoming i) [4..10]) CalleeSavedRejected
            case lookup n arguments of
              Just arg -> require (get m 0 == Constant arg) ArgumentRejected
              Nothing -> pure ()
            let changed = foldl (\state i -> put state i (Result n)) m [0,1,2,3,12,14]
                called = changed { sites = sites m ++ [CallSite pc dest (pc+4)], zero = Nothing }
            if n == 7 then jump dest ((put called 14 (SavedReturn (pc+4))) {frames = 1}) else next called
          else jump dest m
        else if w .&. 0x0ffffff0 == 0x012fff30 then do
          require (frames m == 1 && get m 13 == Stack (-24)) HookRejected
          require (all (\i -> get m i == Incoming i) [5..10]) CalleeSavedRejected
          dest <- case get m (fromIntegral (w .&. 15)) of Constant v -> pure v; _ -> Left HookRejected
          require (take (length (hooks m)+1) [0x107277c,0x1066304,0x1062114] == hooks m ++ [dest]) HookRejected
          next ((foldl (\state i -> put state i (Result 7)) m [0,1,2,3,12,14]) {hooks = hooks m ++ [dest], zero = Nothing})
        else if w .&. 0x0ff00000 == 0x03000000 then do
          require (r /= 15 && r /= 13) UnsupportedInstruction
          next (put m r (Constant wide))
        else if w .&. 0x0ff00000 == 0x03400000 then do
          require (r /= 15 && r /= 13) UnsupportedInstruction
          case get m r of
            Constant v -> next (put m r (Constant ((v .&. 0xffff) .|. (wide `shiftL` 16))))
            _ -> Left UnsupportedInstruction
        else if w .&. 0x0c500000 == 0x04100000 then do
          require (not (testBit w 25) && r /= 15 && r /= 13 && rn /= 15 && rn /= 13) UnsupportedInstruction
          base <- case get m rn of Constant v -> pure v; _ -> Left MemoryEffectRejected
          let delta = w .&. 4095
              adjusted = if testBit w 23 then base+delta else base-delta
              address = if testBit w 24 then adjusted else base
          value <- maybe (Left MemoryEffectRejected) pure (lookup address [(0xbf1a2c,0x107277c),(0xbf1a28,0x1066304),(0xbf1a24,0x1062114),(0xbf1a20,0xffffffff)])
          require (frames m == 1 && address == 0xbf1a2c - fromIntegral (4*length (reads m))) HookRejected
          let loaded = put m r (Constant value)
              updated = if not (testBit w 24) || testBit w 21 then put loaded rn (Constant adjusted) else loaded
          next (updated {reads = reads m ++ [address]})
        else if w .&. 0x0c000000 == 0 then do
          let op = (w `shiftR` 21) .&. 15
          rhs <- if testBit w 25 then pure (Constant imm) else if w .&. 0xff0 == 0 then pure (get m (fromIntegral (w .&. 15))) else Left UnsupportedInstruction
          case (op,get m rn,rhs) of
            (13,_,v) | r /= 15 && r /= 13 && not (testBit w 20) -> next (put m r v)
            (4,Stack n,Constant v) | (r == 11 || r == 13) && not (testBit w 20) -> next (put m r (Stack (n+fromIntegral v)))
            (2,Constant n,Constant v) | r /= 15 && not (testBit w 20) -> next (put m r (Constant (n-v)))
            (10,Result n,Constant 0) | n `elem` checked -> next (m {zero = Just (failure /= Just n)})
            (10,Constant n,Constant v) -> next (m {zero = Just (n == v)})
            (11,Constant n,Constant v) -> next (m {zero = Just (n+v == 0)})
            _ -> Left UnsupportedInstruction
        else Left UnsupportedInstruction
