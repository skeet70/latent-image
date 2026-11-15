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
import qualified StackWalk as W

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

newtype FrameOffset = FrameOffset Integer deriving (Eq)

data FrameState = FrameState
  { frameSp :: FrameOffset
  , frameFp :: Maybe FrameOffset
  } deriving (Eq)

data FrameVisit = FrameVisit Guest FrameState

-- Only declared instruction paths prove nonreturning calls. Unknown transfers and uncovered
-- fall-throughs remain returning possibilities; names and symbol-end data are not evidence.
nonReturningEntries :: Image -> [F.Symbol] -> S.Set Guest
nonReturningEntries image symbols = fixed (S.fromList (map (Guest . F.symbolAddress) symbols))
  where
    fixed candidates =
      let remaining = S.fromList [Guest (F.symbolAddress symbol) | symbol <- symbols,
            S.member (Guest (F.symbolAddress symbol)) candidates, neverReturns candidates symbol]
      in if remaining == candidates then remaining else fixed remaining
    neverReturns candidates symbol = walk S.empty [F.symbolAddress symbol]
      where
        start = F.symbolAddress symbol
        end = toInteger start + toInteger (F.symbolSize symbol)
        inside address = start <= address && toInteger address < end
        present address = inside address && S.member (Guest address) (imageInstructions image)
        walk _ [] = True
        walk seen (address : rest)
          | S.member address seen = walk seen rest
          | not (present address) = False
          | otherwise = case wordAt image address of
              Left _ -> False
              Right word ->
                let next = address + 4
                    proceed visits = walk (S.insert address seen) (visits ++ rest)
                    jump condition target
                      | inside target = proceed (target : [next | condition /= A.Cond 14])
                      | S.member (Guest target) candidates = proceed [next | condition /= A.Cond 14]
                      | otherwise = False
                in case W.classify W.Strict address word of
                  W.Call target
                    | S.member (Guest target) candidates,
                      A.Branch (A.Cond 14) True _ <- A.decode address word -> proceed []
                    | otherwise -> proceed [next]
                  W.Jump condition target -> jump condition target
                  W.RegisterCall _ -> proceed [next]
                  W.Reserve _ -> proceed [next]
                  W.Release _ -> proceed [next]
                  W.FromFrame _ -> proceed [next]
                  W.SetFrame _ -> proceed [next]
                  W.Neutral -> proceed [next]
                  _ -> False

frameBytes :: S.Set Guest -> S.Set Guest -> Image -> F.Symbol -> Either Failure Int
frameBytes entries nonReturning image symbol = do
  let start = F.symbolAddress symbol
      size = F.symbolSize symbol
      end = toInteger start + toInteger size
      Guest firstByte = imageBase image
      refused = Left (FrameNotDerivable (Guest start))
      inside address = start <= address && toInteger address < end
      present address = inside address && S.member (Guest address) (imageInstructions image)
      knownCallee target =
        let Guest base = imageBase image
            inImage = toInteger base <= toInteger target
              && toInteger target < toInteger base + toInteger (B.length (imageBytes image))
        in unless (not inImage || S.member (Guest target) entries) refused
      offset (FrameOffset value) = value
      sp state = offset (frameSp state)
      adjusted value state
        | value < 0 = refused
        | otherwise = Right state { frameSp = FrameOffset value }
      frameWriteback instruction = case instruction of
        A.Load _ _ _ _ base _ indexing -> base == W.framePointer && indexing /= A.OffsetOnly
        A.Store _ _ _ base _ indexing -> base == W.framePointer && indexing /= A.OffsetOnly
        A.LoadMultiple _ base _ writeback _ -> base == W.framePointer && writeback
        A.StoreMultiple _ base _ writeback _ -> base == W.framePointer && writeback
        _ -> False
      clobber instruction writtenRegisters state
        | W.framePointer `elem` writtenRegisters || frameWriteback instruction = state { frameFp = Nothing }
        | otherwise = state
      next address state = do
        unless (toInteger address + 4 < end && present (address + 4)) refused
        pure [FrameVisit (Guest (address + 4)) state]
      terminal state = unless (sp state == 0) refused
      conditional condition address state visits
        | condition == A.Cond 14 = Right visits
        | otherwise = (visits ++) <$> next address state
      jump target state
        | inside target = if present target then Right [FrameVisit (Guest target) state] else refused
        | otherwise = do
            knownCallee target
            terminal state
            pure []
      walk _ best [] = Right best
      walk seen best (FrameVisit point@(Guest address) incoming : rest) =
        case M.lookup point seen of
          Just old | frameSp old /= frameSp incoming -> refused
            | frameFp old == Nothing || frameFp old == frameFp incoming -> walk seen best rest
            | otherwise -> step incoming { frameFp = Nothing }
          Nothing -> step incoming
        where
          step state = do
            unless (present address) refused
            word <- wordAt image address
            let instruction = A.decode address word
                writtenRegisters = W.writes address word
            case instruction of
              A.Load {} | A.sp `elem` writtenRegisters -> refused
              _ -> pure ()
            visits <- case W.classify W.Strict address word of
              W.Reserve amount -> adjusted (sp state + toInteger amount) state >>= next address
              W.Release amount -> adjusted (sp state - toInteger amount) state
                >>= next address . clobber instruction writtenRegisters
              W.FromFrame displacement -> case frameFp state of
                Just position -> adjusted (offset position - toInteger displacement) state >>= next address
                Nothing -> refused
              W.SetFrame amount -> next address state
                { frameFp = Just (FrameOffset (sp state - toInteger amount)) }
              W.Neutral
                | any (`elem` writtenRegisters) [A.sp, A.pc] -> refused
                | otherwise -> next address (clobber instruction writtenRegisters state)
              W.Call target -> do
                knownCallee target
                case instruction of
                  A.Branch (A.Cond 14) True _ | S.member (Guest target) nonReturning -> pure []
                  _ -> next address state
              W.RegisterCall _ -> next address state
              W.Jump condition target -> jump target state >>= conditional condition address state
              W.Return condition amount -> do
                returned <- adjusted (sp state - toInteger amount) state
                terminal returned
                conditional condition address state []
              W.RegisterJump condition _ -> do
                terminal state
                conditional condition address state []
              _ -> refused
            walk (M.insert point state seen) (max best (sp state)) (visits ++ rest)
  unless (size > 0 && size `mod` 4 == 0 && start `mod` 4 == 0
    && end <= toInteger (maxBound :: Word32) && start >= firstByte
    && end <= toInteger firstByte + toInteger (B.length (imageBytes image)))
    (Left (InvalidFunction (Guest start)))
  peak <- walk M.empty 0 [FrameVisit (Guest start) (FrameState (FrameOffset 0) Nothing)]
  unless (peak <= toInteger (maxBound :: Int)) refused
  pure (fromInteger peak)

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
      entries = S.fromList (map (Guest . F.symbolAddress) symbols)
      nonReturning = nonReturningEntries image symbols
  frames <- forM symbols $ \symbol -> do
    size <- frameBytes entries nonReturning image symbol
    let start = F.symbolAddress symbol
        end = start + F.symbolSize symbol
    callees <- fmap concat $ forM [start, start + 4 .. end - 4] $ \address ->
      if not (S.member (Guest address) (imageInstructions image)) then pure []
      else do
        instruction <- A.decode address <$> wordAt image address
        pure [if linked then Call address target else Tail address target | A.Branch _ linked target <- [instruction], S.member (Guest target) entries]
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
