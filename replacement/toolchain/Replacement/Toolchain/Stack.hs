module Replacement.Toolchain.Stack
  ( Guest(..), StackBytes, stackBytes, stackByteCount, Image(..), Failure(..), check, checkEntry
  ) where

import qualified A32 as A
import Control.Monad (forM, forM_, unless, when)
import qualified Data.Graph as G
import qualified Data.ByteString as B
import Data.Bits ((.&.))
import qualified Data.Map.Lazy as LM
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
  , frameFallthrough :: Int
  } deriving (Eq)

data FrameVisit = FrameVisit Guest FrameState

data Function = Function
  { functionEntry :: Word32
  , functionSize :: Word32
  }

-- Only declared instruction paths prove nonreturning calls. Unknown transfers and uncovered
-- fall-throughs remain returning possibilities; names and symbol-end data are not evidence.
nonReturningEntries :: Image -> [Function] -> S.Set Guest
nonReturningEntries image functions = fixed (S.fromList (map (Guest . functionEntry) functions))
  where
    fixed candidates =
      let remaining = S.fromList [Guest (functionEntry function) | function <- functions,
            S.member (Guest (functionEntry function)) candidates, neverReturns candidates function]
      in if remaining == candidates then remaining else fixed remaining
    neverReturns candidates function = walk S.empty [functionEntry function]
      where
        start = functionEntry function
        end = toInteger start + toInteger (functionSize function)
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

frameBytes :: S.Set Guest -> S.Set Guest -> Image -> Function -> Either Failure Int
frameBytes entries nonReturning image function = do
  let start = functionEntry function
      size = functionSize function
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
      advanced state = state { frameFallthrough = min 4 (frameFallthrough state + 1) }
      interrupted state = state { frameFallthrough = 0 }
      next address state = do
        unless (toInteger address + 4 < end && present (address + 4)) refused
        pure [FrameVisit (Guest (address + 4)) (advanced state)]
      nextInterrupted address state = do
        unless (toInteger address + 4 < end && present (address + 4)) refused
        pure [FrameVisit (Guest (address + 4)) (interrupted state)]
      terminal state = unless (sp state == 0) refused
      conditional condition address state visits
        | condition == A.Cond 14 = Right visits
        | otherwise = (visits ++) <$> nextInterrupted address state
      jump target state
        | inside target = if present target then Right [FrameVisit (Guest target) (interrupted state)] else refused
        | otherwise = do
            knownCallee target
            terminal state
            pure []
      modeWritePreserves address state
        | address < start + 8 = pure False
        | otherwise = do
            current <- wordAt image address
            previous <- wordAt image (address - 4)
            beforePrevious <- wordAt image (address - 8)
            exactRestore <- if address >= start + 16 && frameFallthrough state >= 4
              then (== [0xe2001080, 0xe10f2000, 0xe3c22080, 0xe1811002, 0xe121f001])
                <$> mapM (wordAt image) [address - 16, address - 12 .. address]
              else pure False
            let currentStatus destination word =
                  word .&. 0x00400000 == 0
                    && A.decode (address - 8) word == A.ReadStatus (A.Cond 14) destination
                copiedControl source = case A.decode (address - 4) previous of
                  A.DataProcessing (A.Cond 14) operation False destination status (A.Immediate mask) ->
                    destination == source && operation `elem` [A.ORR, A.BIC]
                      && mask .&. 0x1f == 0 && currentStatus status beforePrevious
                  _ -> False
                insertedControl source = case A.bitFieldInsert previous of
                  Just (A.BitFieldInsert (A.Cond 14) destination _ lsb _) ->
                    destination == source && lsb >= 5 && currentStatus source beforePrevious
                  _ -> False
            pure $ case A.statusWrite current of
              Just (A.StatusWrite (A.Cond 14) A.CurrentStatus fields (A.StatusFromRegister source)) ->
                A.ControlBits `elem` fields
                  && ((frameFallthrough state >= 2 && (copiedControl source || insertedControl source)) || exactRestore)
              _ -> False
      conditionalEpilogue condition displacement address state = do
        let returnAddress = address + 4
        unless (present returnAddress) refused
        returnWord <- wordAt image returnAddress
        case W.classify W.Strict returnAddress returnWord of
          W.Return returnCondition amount
            | returnCondition == condition -> do
                restored <- case frameFp state of
                  Just position -> adjusted (offset position - displacement) state
                  Nothing -> refused
                returned <- adjusted (sp restored - toInteger amount) restored
                terminal returned
                visits <- nextInterrupted returnAddress state
                pure (sp restored, visits)
          _ -> refused
      walk _ best [] = Right best
      walk seen best (FrameVisit point@(Guest address) incoming : rest) =
        case M.lookup point seen of
          Just old
            | frameSp old /= frameSp incoming -> refused
            | otherwise ->
                let merged = FrameState
                      (frameSp old)
                      (if frameFp old == frameFp incoming then frameFp old else Nothing)
                      (min (frameFallthrough old) (frameFallthrough incoming))
                in if merged == old then walk seen best rest else step merged
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
            (stepPeak, visits) <- case instruction of
              A.DataProcessing condition A.MOV setsFlags destination _ (A.ShiftedImmediate source A.LSL 0)
                | destination == A.sp && source == W.framePointer && condition /= A.Cond 14
                , not setsFlags ->
                    conditionalEpilogue condition 0 address state
              A.DataProcessing condition operation setsFlags destination source (A.Immediate amount)
                | destination == A.sp && source == W.framePointer && condition /= A.Cond 14
                , operation `elem` [A.ADD, A.SUB]
                , not setsFlags ->
                    conditionalEpilogue condition
                      (if operation == A.ADD then toInteger amount else negate (toInteger amount))
                      address state
              _ -> do
                ordinary <- case W.classify W.Strict address word of
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
                      _ -> nextInterrupted address state
                  W.RegisterCall _ -> nextInterrupted address state
                  W.Jump condition target -> jump target state >>= conditional condition address state
                  W.Return condition amount -> do
                    returned <- adjusted (sp state - toInteger amount) state
                    terminal returned
                    conditional condition address state []
                  W.RegisterJump condition _ -> do
                    terminal state
                    conditional condition address state []
                  W.ModeWrite _ -> do
                    preserving <- modeWritePreserves address state
                    if preserving then next address state else refused
                  _ -> refused
                pure (sp state, ordinary)
            walk (M.insert point state seen) (max best stepPeak) (visits ++ rest)
  unless (size > 0 && size `mod` 4 == 0 && start `mod` 4 == 0
    && end <= toInteger (maxBound :: Word32) && start >= firstByte
    && end <= toInteger firstByte + toInteger (B.length (imageBytes image)))
    (Left (InvalidFunction (Guest start)))
  peak <- walk M.empty 0 [FrameVisit (Guest start) (FrameState (FrameOffset 0) Nothing 0)]
  unless (peak <= toInteger (maxBound :: Int)) refused
  pure (fromInteger peak)

data Edge = Call Word32 Word32 | Tail Word32 Word32

data Frame = Frame
  { frameEntry :: Word32
  , frameSize :: Int
  , frameCallees :: [Edge]
  }

check :: StackBytes -> Image -> Either Failure ()
check budget = checkWith budget Nothing

checkEntry :: StackBytes -> Guest -> Image -> Either Failure ()
checkEntry budget entry = checkWith budget (Just entry)

checkWith :: StackBytes -> Maybe Guest -> Image -> Either Failure ()
checkWith (StackBytes budget) selected image = do
  forM_ (S.toList (imageInstructions image)) $ \(Guest address) -> wordAt image address >> pure ()
  let Guest imageStart = imageBase image
      imageEnd = toInteger imageStart + toInteger (B.length (imageBytes image))
      declaredEntries = S.fromList
        [ F.symbolAddress symbol
        | symbol <- imageSymbols image
        , S.member (Guest (F.symbolAddress symbol)) (imageInstructions image)
        ]
      instructionSpan address =
        let limit = maybe imageEnd toInteger (S.lookupGT address declaredEntries)
        in 4 * fromIntegral (length (takeWhile
          (\point -> S.member (Guest point) (imageInstructions image))
          [fromInteger value | value <- [toInteger address, toInteger address + 4 .. limit - 4]]))
      functionSizes = M.fromListWith max
        [ (address, max (F.symbolSize symbol) (instructionSpan address))
        | symbol <- imageSymbols image
        , let address = F.symbolAddress symbol
        , S.member (Guest address) (imageInstructions image)
        ]
      declaredFunctions = [Function address size | (address, size) <- M.toList functionSizes, size > 0]
      sameFunction source target = any
        (\function -> let low = toInteger (functionEntry function)
                          high = low + toInteger (functionSize function)
                          point value = let integer = toInteger value in low <= integer && integer < high
          in point source && point target)
        declaredFunctions
  syntheticEntries <- fmap concat $ forM (S.toList (imageInstructions image)) $ \(Guest address) -> do
    instruction <- A.decode address <$> wordAt image address
    pure [target | A.Branch _ _ target <- [instruction]
      , S.member (Guest target) (imageInstructions image)
      , not (S.member target declaredEntries)
      , not (sameFunction address target)]
  let allFunctionSizes = M.unionWith max functionSizes
        (M.fromListWith max [(target, instructionSpan target) | target <- syntheticEntries])
      functions = [Function address size | (address, size) <- M.toList allFunctionSizes, size > 0]
      entries = S.fromList (map (Guest . functionEntry) functions)
      nonReturning = nonReturningEntries image functions
  frames <- forM functions $ \function -> do
    size <- frameBytes entries nonReturning image function
    let start = functionEntry function
        end = start + functionSize function
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
      components = zip [0 :: Int ..] (map G.flattenSCC (G.stronglyConnComp
        [(frame, frameEntry frame, map destination (frameCallees frame)) | frame <- frames]))
      componentByEntry = M.fromList
        [(frameEntry frame, component) | (component, members) <- components, frame <- members]
      internalGrowingEdges =
        [ (component, edge)
        | (component, members) <- components
        , frame <- members
        , edge <- frameCallees frame
        , grows edge
        , M.lookup (destination edge) componentByEntry == Just component
        ]
      componentSuccessors = M.fromList
        [(component, S.fromList
          [ targetComponent
          | frame <- members
          , edge <- frameCallees frame
          , Just targetComponent <- [M.lookup (destination edge) componentByEntry]
          , targetComponent /= component
          ])
        | (component, members) <- components]
      reachable seen [] = seen
      reachable seen (component : rest)
        | S.member component seen = reachable seen rest
        | otherwise = reachable (S.insert component seen)
            (S.toList (M.findWithDefault S.empty component componentSuccessors) ++ rest)
      componentDemands = LM.fromList [(component, demand component members) | (component, members) <- components]
      demand component members = maximum
        (map frameSize members ++
          [ (if grows edge then frameSize frame else 0) + componentDemands LM.! targetComponent
          | frame <- members
          , edge <- frameCallees frame
          , Just targetComponent <- [M.lookup (destination edge) componentByEntry]
          , targetComponent /= component
          ])
      frameDemand frame = componentDemands LM.! (componentByEntry M.! frameEntry frame)
  roots <- case selected of
    Nothing -> pure frames
    Just entry@(Guest address) -> case M.lookup address byAddress of
      Nothing -> Left (InvalidFunction entry)
      Just frame -> pure [frame]
  let reachableComponents = reachable S.empty
        [componentByEntry M.! frameEntry frame | frame <- roots]
  case [edge | (component, edge) <- internalGrowingEdges, S.member component reachableComponents] of
    edge : _ -> Left (RecursiveCall (Guest (site edge)) (Guest (destination edge)))
    [] -> pure ()
  forM_ roots $ \frame -> when (frameDemand frame > budget)
    (Left (BudgetExceeded (Guest (frameEntry frame)) (StackBytes (frameDemand frame))))
