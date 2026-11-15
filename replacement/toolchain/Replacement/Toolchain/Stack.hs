module Replacement.Toolchain.Stack
  ( Guest(..), StackBytes, stackBytes, stackByteCount, Image(..), Failure(..), check
  ) where

import qualified A32 as A
import Control.Monad (forM, forM_, unless, when)
import qualified Data.ByteString as B
import qualified Data.Map.Strict as M
import qualified Data.Set as S
import Data.Word (Word32)
import qualified FujiAF.FirstStage as F

newtype Guest = Guest Word32 deriving (Eq, Ord, Show)
newtype StackBytes = StackBytes Int deriving (Eq, Ord, Show)

stackBytes :: Int -> Maybe StackBytes
stackBytes bytes = if bytes >= 0 then Just (StackBytes bytes) else Nothing

stackByteCount :: StackBytes -> Int
stackByteCount (StackBytes bytes) = bytes

data Image = Image
  { imageBase :: Guest
  , imageBytes :: B.ByteString
  , imageSymbols :: [F.Symbol]
  , imageInstructions :: S.Set Guest
  }

data Failure
  = OutsideImage Guest
  | InvalidFunction Guest
  | FrameNotDerivable Guest
  | RecursiveCall Guest Guest
  | BudgetExceeded Guest StackBytes
  deriving (Eq, Show)

wordAt :: Image -> Word32 -> Either Failure Word32
wordAt image address = do
  let Guest base = imageBase image
      offset = toInteger address - toInteger base
  unless (offset >= 0 && offset + 4 <= toInteger (B.length (imageBytes image))) (Left (OutsideImage (Guest address)))
  let index = fromInteger offset
      byte shift = fromIntegral (B.index (imageBytes image) (index + shift)) :: Word32
  pure (byte 0 + byte 1 * 0x100 + byte 2 * 0x10000 + byte 3 * 0x1000000)

prologueInstruction :: A.Instruction -> Bool
prologueInstruction instruction = case instruction of
  A.StoreMultiple (A.Cond 14) base A.DecrementBefore True registers -> base == A.sp && not (null registers)
  A.DataProcessing (A.Cond 14) _ _ destination base (A.Immediate _) -> destination == A.sp || base == A.sp
  A.DataProcessing (A.Cond 14) A.MOV _ destination _ (A.ShiftedImmediate source A.LSL 0) -> destination == A.sp || source == A.sp
  _ -> False

spWrite :: A.Instruction -> Bool
spWrite instruction = case instruction of
  A.DataProcessing _ _ _ destination _ _ -> destination == A.sp
  A.LoadMultiple _ base _ writeback _ -> base == A.sp && writeback
  A.StoreMultiple _ base _ writeback _ -> base == A.sp && writeback
  _ -> False

-- A leaf exit before a shrink-wrapped frame leaves SP unchanged.
framePreludeInstruction :: A.Instruction -> Bool
framePreludeInstruction instruction = not (spWrite instruction) && case instruction of
  A.Branch {} -> False
  A.BranchExchange condition False register -> condition /= A.Cond 14 && register == A.lr
  A.BranchExchange {} -> False
  A.Rejected {} -> False
  A.Load _ _ _ destination _ _ _ -> destination /= A.pc && destination /= A.sp
  A.LoadMultiple _ _ _ _ registers -> A.pc `notElem` registers && A.sp `notElem` registers
  A.DataProcessing _ _ _ destination _ _ -> destination /= A.pc
  A.MoveWide _ destination _ -> destination /= A.pc && destination /= A.sp
  A.MoveTop _ destination _ -> destination /= A.pc && destination /= A.sp
  A.ReadStatus _ destination -> destination /= A.pc && destination /= A.sp
  A.ZeroExtend _ destination _ _ _ -> destination /= A.pc && destination /= A.sp
  A.Multiply _ destination _ _ _ -> destination /= A.pc && destination /= A.sp
  A.Compute _ destinations -> all (`notElem` destinations) [A.pc, A.sp]
  _ -> True

frameBytes :: Image -> F.Symbol -> Either Failure Int
frameBytes image symbol = do
  let start = F.symbolAddress symbol
      size = F.symbolSize symbol
      end = toInteger start + toInteger size
  let Guest firstByte = imageBase image
  unless (size > 0 && size `mod` 4 == 0 && start `mod` 4 == 0 && end <= toInteger (maxBound :: Word32)
    && start >= firstByte && end <= toInteger firstByte + toInteger (B.length (imageBytes image)))
    (Left (InvalidFunction (Guest start)))
  instructions <- traverse (fmap (uncurry A.decode) . instructionAt)
    [address | address <- [start, start + 4 .. fromInteger end - 4], S.member (Guest address) (imageInstructions image)]
  let (_, framed) = span (\instruction -> not (prologueInstruction instruction) && framePreludeInstruction instruction) instructions
      prologue = takeWhile prologueInstruction framed
      pushed = sum [4 * length registers | A.StoreMultiple (A.Cond 14) base A.DecrementBefore True registers <- prologue, base == A.sp]
      adjusted = sum [fromIntegral count | A.DataProcessing (A.Cond 14) A.SUB False destination base (A.Immediate count) <- prologue, destination == A.sp, base == A.sp]
  if null prologue && any spWrite instructions then Left (FrameNotDerivable (Guest start))
    else pure (pushed + adjusted)
  where
    instructionAt address = (,) address <$> wordAt image address

data Edge = Call Word32 Word32 | Tail Word32 Word32

data Frame = Frame
  { frameEntry :: Word32
  , frameSize :: Int
  , frameCallees :: [Edge]
  }

check :: StackBytes -> Image -> Either Failure ()
check (StackBytes budget) image = do
  forM_ (S.toList (imageInstructions image)) $ \(Guest address) -> wordAt image address >> pure ()
  let symbols = [symbol | symbol <- imageSymbols image, F.symbolSize symbol > 0, S.member (Guest (F.symbolAddress symbol)) (imageInstructions image)]
      entries = S.fromList (map F.symbolAddress symbols)
  frames <- forM symbols $ \symbol -> do
    size <- frameBytes image symbol
    let start = F.symbolAddress symbol
        end = start + F.symbolSize symbol
    callees <- fmap concat $ forM [start, start + 4 .. end - 4] $ \address ->
      if not (S.member (Guest address) (imageInstructions image)) then pure []
      else do
        instruction <- A.decode address <$> wordAt image address
        pure [if linked then Call address target else Tail address target | A.Branch _ linked target <- [instruction], S.member target entries]
    pure (Frame start size callees)
  let byAddress = M.fromList [(frameEntry frame, frame) | frame <- frames]
      destination edge = case edge of Call _ target -> target; Tail _ target -> target
      site edge = case edge of Call address _ -> address; Tail address _ -> address
      grows edge = case edge of Call _ _ -> True; Tail _ _ -> False
      visit path calls address = case M.lookup address byAddress of
        Nothing -> Right ()
        Just frame -> forM_ (frameCallees frame) $ \edge -> do
          let target = destination edge
              nextCalls = calls + if grows edge then 1 else 0
          case lookup target path of
            Just previousCalls -> when (nextCalls > previousCalls) (Left (RecursiveCall (Guest (site edge)) (Guest target)))
            Nothing -> visit ((target, nextCalls) : path) nextCalls target
      chain path frame
        | frameEntry frame `elem` path = 0
        | otherwise = maximum (frameSize frame :
            [ (if grows edge then frameSize frame else 0) + chain (frameEntry frame : path) callee
            | edge <- frameCallees frame, Just callee <- [M.lookup (destination edge) byAddress]])
  forM_ frames $ \frame -> visit [(frameEntry frame, 0 :: Int)] 0 (frameEntry frame)
  forM_ frames $ \frame -> when (chain [] frame > budget)
    (Left (BudgetExceeded (Guest (frameEntry frame)) (StackBytes (chain [] frame))))
