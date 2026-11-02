{-# LANGUAGE GHC2021 #-}

module StockInventory
  ( GuestAddress (..)
  , FunctionWindow (..)
  , FunctionSize (..)
  , FunctionDeclaration (..)
  , checkDeclarations
  , deriveFunction
  , checkedStockFunctions
  , CodeFootprint (..), FootprintError (..), deriveRetainedFootprint
  , stockFunctions
  ) where

import qualified A32 as A
import Control.Monad (forM, forM_, unless)
import Data.Bits (shiftL, (.&.))
import qualified Data.ByteString as B
import Data.List (foldl', sort)
import qualified Data.Map.Strict as M
import qualified Data.Set as S
import Data.Word (Word32)
import Numeric (showHex)

newtype GuestAddress = GuestAddress Word32 deriving (Eq, Ord, Show)
data FunctionWindow = FunctionWindow GuestAddress GuestAddress deriving (Eq, Show)
newtype FunctionSize = FunctionSize Int deriving (Eq, Ord, Show)
data FunctionDeclaration = FunctionDeclaration GuestAddress FunctionSize deriving (Eq, Ord, Show)
data CodeFootprint = CodeFootprint
  { footprintInstructions :: [GuestAddress]
  , footprintLiterals :: [GuestAddress]
  , footprintUnresolvedTransfers :: [GuestAddress]
  , footprintDependencies :: [GuestAddress]
  } deriving (Eq, Show)
data FootprintError = FootprintSourceMissing GuestAddress | FootprintWordLimit GuestAddress
  | FootprintSwitchCondition GuestAddress | FootprintSwitchGuard GuestAddress
  | FootprintSwitchComparison GuestAddress | FootprintSwitchBranches GuestAddress | FootprintSwitchBypassed GuestAddress | FootprintUnalignedTarget GuestAddress
  | FootprintComputedPc GuestAddress | FootprintStatusPc GuestAddress | FootprintThumb GuestAddress
  | FootprintForbidden GuestAddress A.Forbidden deriving (Eq, Show)

checkDeclarations :: [FunctionDeclaration] -> [FunctionDeclaration] -> Either String ()
checkDeclarations declared derived = do
  unless (unique declared && unique derived) (Left "duplicate function declaration")
  unless (sort declared == sort derived) (Left ("declared stock functions differ from derived code: " ++ show derived))
  where
    unique entries = length entries == S.size (S.fromList [address | FunctionDeclaration address _ <- entries])

-- Fences follow the next exported function, except the data objects after
-- 0xe8659c and 0xe88ed0. The pinned listing retains those object boundaries.
windows :: [FunctionWindow]
windows = [FunctionWindow (GuestAddress low) (GuestAddress high) | (low, high) <-
  [ (0xe67750,0xe677bc), (0xe73a74,0xe73ac0), (0xe74a34,0xe74a84)
  , (0xe748c8,0xe74954), (0xe74c68,0xe74cc8), (0xe75490,0xe754f4)
  , (0xe755d0,0xe75668), (0xe77168,0xe772ec), (0xe7e95c,0xe7ebd0)
  , (0xe7ee84,0xe7f324), (0xe7f324,0xe7f348), (0xe8659c,0xe86bec)
  , (0xe86d30,0xe86d50), (0xe87114,0xe871c4), (0xe871c4,0xe87264)
  , (0xe87ad4,0xe87b0c), (0xe886ec,0xe88804), (0xe88804,0xe888a0)
  , (0xe88e1c,0xe88e84), (0xe88ed0,0xe88f60), (0xeb0ea0,0xeb0ed8)
  ]]

stockFunctions :: [(Word32, Int)]
stockFunctions =
  [ (0xe67750,0x6c), (0xe73a74,0x4c), (0xe74a34,0x50)
  , (0xe748c8,0x8c), (0xe74c68,0x60), (0xe75490,0x64)
  , (0xe755d0,0x98), (0xe77168,0x180), (0xe7e95c,0x274)
  , (0xe7ee84,0x4a0), (0xe7f324,0x24), (0xe8659c,0x64c)
  , (0xe86d30,0x20), (0xe87114,0xb0), (0xe871c4,0xa0)
  , (0xe87ad4,0x38), (0xe886ec,0x118), (0xe88804,0x9c)
  , (0xe88e1c,0x60), (0xe88ed0,0x8c), (0xeb0ea0,0x38)
  ]

checkedStockFunctions :: B.ByteString -> Either String [(Word32, Int)]
checkedStockFunctions image = do
  derived <- mapM (\window -> deriveFunction (GuestAddress 0xbf1a00) image window []) windows
  let declaration (address, size) = FunctionDeclaration (GuestAddress address) (FunctionSize size)
  checkDeclarations (map declaration stockFunctions) (map declaration derived)
  pure derived

deriveFunction :: GuestAddress -> B.ByteString -> FunctionWindow -> [GuestAddress] -> Either String (Word32, Int)
deriveFunction (GuestAddress imageBase) image (FunctionWindow (GuestAddress low) (GuestAddress high)) externalTails = do
  unless (low < high && low .&. 3 == 0 && high .&. 3 == 0) (Left "invalid function window")
  (code, literals) <- walk [low] S.empty S.empty
  graph <- forM (S.toList code) $ \address -> do
    instruction <- A.decode address <$> word address
    (successors, _) <- edges address instruction
    pure (address, instruction, successors)
  forM_ graph $ \(address, instruction, _) -> case instruction of
    A.Load _ _ _ destination base (A.OffsetRegister _ _ _ _) _ | destination == A.pc && base == A.pc -> do
      let predecessors = S.fromList [source | (source, _, successors) <- graph, address `elem` successors]
      unless (address /= low && predecessors == S.singleton (address-4)) (Left "dispatch bound is bypassed")
    _ -> pure ()
  unless (S.null (S.intersection code literals)) (Left "instruction also used as a literal")
  let missing = S.fromList [low,low+4..high-4] S.\\ S.union code literals
  unless (S.null missing) (Left ("unclassified function words at " ++ show (S.toList missing)))
  pure (low, fromIntegral (S.findMax code + 4 - low))
  where
    inside address = low <= address && address < high && address .&. 3 == 0
    word address
      | address < imageBase || toInteger address + 4 > toInteger imageBase + toInteger (B.length image) = Left ("unreadable instruction at " ++ showHex address "")
      | otherwise = Right (foldr (\index value -> (value `shiftL` 8) + fromIntegral (B.index image (fromIntegral (address - imageBase) + index))) 0 [0..3])
    walk [] code literals = Right (code, literals)
    walk (address:pending) code literals
      | S.member address code = walk pending code literals
      | not (inside address) = Left ("fallthrough leaves function window at " ++ showHex address "")
      | otherwise = do
          instruction <- A.decode address <$> word address
          (successors, pools) <- edges address instruction
          unless (all inside pools) (Left "literal leaves function window")
          walk (successors ++ pending) (S.insert address code) (S.union literals (S.fromList pools))
    fallthrough address (A.Cond condition) = [address + 4 | condition /= 14]
    branch address condition target = do
      unless (target .&. 3 == 0 && (inside target || GuestAddress target `elem` externalTails))
        (Left ("undeclared external tail at " ++ showHex target ""))
      pure ([target | inside target] ++ fallthrough address condition)
    edges address instruction = case instruction of
      A.Branch _ True _ -> pure ([address+4], [])
      A.Branch condition False target -> do
        successors <- branch address condition target
        pure (successors, [])
      A.BranchExchange _ True _ -> pure ([address+4], [])
      A.BranchExchange condition False register
        | register == A.lr -> pure (fallthrough address condition, [])
        | otherwise -> Left "unresolved register tail transfer"
      A.LoadMultiple condition base _ _ registers | A.pc `elem` registers -> do
        unless (base == A.sp) (Left "unresolved load-multiple destination")
        pure (fallthrough address condition, [])
      A.Load condition A.Word _ destination base (A.OffsetRegister True index A.LSL 2) A.OffsetOnly
        | destination == A.pc && base == A.pc -> do
            previous <- A.decode (address-4) <$> word (address-4)
            count <- case previous of
              A.DataProcessing (A.Cond 14) A.CMP _ _ compared (A.Immediate bound)
                | compared == index && condition == A.Cond 9 && bound < 256 -> pure (bound+1)
              _ -> Left "unbounded PC-relative dispatch"
            let pools = [address+8+4*index' | index' <- [0..count-1]]
            targets <- mapM word pools
            unless (all inside targets) (Left "dispatch target leaves function window")
            pure (targets ++ fallthrough address condition, pools)
      A.Load condition width _ destination base (A.OffsetImmediate offset) A.OffsetOnly | base == A.pc -> do
        let target = fromIntegral (toInteger address + 8 + toInteger offset)
            pools = case width of
              A.Word -> [target]
              A.Dual -> [target,target+4]
              _ -> [target .&. 0xfffffffc]
        if destination == A.pc
          then do
            targetWord <- word target
            successors <- branch address condition targetWord
            pure (successors, pools)
          else pure ([address+4], pools)
      A.Load _ _ _ destination _ _ _ | destination == A.pc -> Left "unresolved loaded destination"
      A.DataProcessing condition A.MOV _ destination _ (A.ShiftedImmediate source A.LSL 0)
        | destination == A.pc && source == A.lr -> pure (fallthrough address condition, [])
      A.DataProcessing _ _ _ destination _ _ | destination == A.pc -> Left "unresolved computed destination"
      A.Rejected problem -> Left ("unadmitted code at " ++ showHex address "" ++ ": " ++ show problem)
      _ -> pure ([address+4], [])

deriveRetainedFootprint :: GuestAddress -> B.ByteString -> GuestAddress -> Either FootprintError CodeFootprint
deriveRetainedFootprint (GuestAddress imageBase) image (GuestAddress entry) = do
  (code,literals,unresolved,dependencies,predecessors,switches) <- walk [entry] S.empty S.empty S.empty S.empty M.empty M.empty
  forM_ (M.toList switches) $ \(address,condition) -> do
    let onlyPrevious location = location /= entry && M.lookup location predecessors == Just (S.singleton (location-4))
    unless (onlyPrevious address && (condition /= A.Cond 14 || onlyPrevious (address-4)))
      (Left (FootprintSwitchBypassed (GuestAddress address)))
  pure (CodeFootprint (map GuestAddress (S.toAscList code)) (map GuestAddress (S.toAscList literals))
    (map GuestAddress (S.toAscList unresolved)) (map GuestAddress (S.toAscList dependencies)))
  where
    inside address = address >= imageBase && address .&. 3 == 0
      && toInteger address+4 <= toInteger imageBase+toInteger (B.length image)
    word address
      | not (inside address) = Left (FootprintSourceMissing (GuestAddress address))
      | otherwise = Right (foldr (\index value -> (value `shiftL` 8)
          + fromIntegral (B.index image (fromIntegral (address-imageBase)+index))) 0 [0..3])
    walk [] code literals unresolved dependencies predecessors switches = Right (code,literals,unresolved,dependencies,predecessors,switches)
    walk (address:pending) code literals unresolved dependencies predecessors switches
      | S.member address code = walk pending code literals unresolved dependencies predecessors switches
      | S.size code >= 16384 = Left (FootprintWordLimit (GuestAddress address))
      | otherwise = do
          raw <- word address
          let instruction = A.decode address raw
              switch = case instruction of
                A.DataProcessing condition A.ADD False destination base (A.ShiftedImmediate _ A.LSL 2) | destination == A.pc && base == A.pc -> Just condition
                A.Load condition A.Word _ destination base (A.OffsetRegister True _ A.LSL 2) A.OffsetOnly | destination == A.pc && base == A.pc -> Just condition
                _ -> Nothing
          (next,pools,unknown,called) <- edges address raw instruction
          mapM_ word pools
          let incoming = foldl' (\seen destination -> M.insertWith S.union destination (S.singleton address) seen) predecessors next
              dispatches = maybe switches (\condition -> M.insert address condition switches) switch
          walk (next++pending) (S.insert address code) (S.union literals (S.fromList pools))
            (if unknown then S.insert address unresolved else unresolved) (S.union dependencies (S.fromList called)) incoming dispatches
    conditional address (A.Cond condition) = [address+4 | condition /= 14]
    unalias address = address .&. 0xbfffffff
    branch address condition target
      | inside (unalias target) = pure (unalias target:conditional address condition,[],False,[])
      | otherwise = pure (conditional address condition,[],False,[unalias target])
    switchBound address condition index = do
      compared <- case condition of
        A.Cond 9 -> A.decode (address-4) <$> word (address-4)
        A.Cond 14 -> do
          guard <- A.decode (address-4) <$> word (address-4)
          case guard of
            A.Branch (A.Cond 8) False _ -> A.decode (address-8) <$> word (address-8)
            _ -> Left (FootprintSwitchGuard (GuestAddress address))
        _ -> Left (FootprintSwitchCondition (GuestAddress address))
      case compared of
        A.DataProcessing (A.Cond 14) A.CMP _ _ register (A.Immediate bound)
          | register == index && bound <= 1024 -> Right bound
        _ -> Left (FootprintSwitchComparison (GuestAddress address))
    edges address raw instruction = case instruction of
      A.Branch _ True target -> pure ([address+4],[],False,[unalias target])
      A.Branch condition False target -> branch address condition target
      A.BranchExchange _ True _ -> pure ([address+4],[],True,[])
      A.BranchExchange condition False register ->
        pure (conditional address condition,[],register /= A.lr,[])
      A.LoadMultiple condition base _ _ registers | A.pc `elem` registers ->
        pure (conditional address condition,[],base /= A.sp,[])
      A.DataProcessing condition A.ADD False destination base (A.ShiftedImmediate index A.LSL 2)
        | destination == A.pc && base == A.pc -> do
            bound <- switchBound address condition index
            let targets = [address+8+4*index' | index' <- [0..bound]]
            instructions <- mapM word targets
            unless (and [case A.decode target instruction of A.Branch (A.Cond 14) False _ -> True; _ -> False
              | (target,instruction) <- zip targets instructions]) (Left (FootprintSwitchBranches (GuestAddress address)))
            pure (targets++conditional address condition,[],False,[])
      A.Load condition A.Word _ destination base (A.OffsetRegister True index A.LSL 2) A.OffsetOnly
        | destination == A.pc && base == A.pc -> do
            bound <- switchBound address condition index
            let pools = [address+8+4*index' | index' <- [0..bound]]
            targets <- map (unalias) <$> mapM word pools
            unless (all (\target -> target .&. 3 == 0) targets) (Left (FootprintUnalignedTarget (GuestAddress address)))
            pure (filter inside targets++conditional address condition,pools,False,filter (not . inside) targets)
      A.Load condition width _ destination base (A.OffsetImmediate offset) A.OffsetOnly | base == A.pc -> do
        let target = fromIntegral (toInteger address+8+toInteger offset)
            bytes = case width of A.Byte -> 1; A.Half -> 2; A.Word -> 4; A.Dual -> 8
            pools = [target .&. 0xfffffffc, (target .&. 0xfffffffc)+4..(target+bytes-1) .&. 0xfffffffc]
        if destination == A.pc then do
          targetWord <- word target
          unless (targetWord .&. 3 == 0) (Left (FootprintUnalignedTarget (GuestAddress address)))
          (next,_,unknown,called) <- branch address condition targetWord
          pure (next,pools,unknown,called)
        else pure ([address+4],pools,False,[])
      A.Load condition _ _ destination base _ _ | destination == A.pc ->
        pure (conditional address condition,[],base /= A.sp,[])
      A.DataProcessing condition A.MOV _ destination _ (A.ShiftedImmediate source A.LSL 0)
        | destination == A.pc && source == A.lr -> pure (conditional address condition,[],False,[])
      A.DataProcessing condition _ _ destination _ _ | destination == A.pc ->
        pure (conditional address condition,[],True,[])
      A.Compute _ destinations | A.pc `elem` destinations -> Left (FootprintComputedPc (GuestAddress address))
      A.ReadStatus _ destination | destination == A.pc -> Left (FootprintStatusPc (GuestAddress address))
      A.Rejected problem -> case problem of
        A.Coprocessor _ -> linear address
        A.SystemRegister -> linear address
        A.InterruptMask -> linear address
        A.Barrier -> linear address
        A.WaitForEvent -> linear address
        A.Exclusive -> linear address
        A.SupervisorCall -> linear address
        A.ThumbBranch -> Left (FootprintThumb (GuestAddress address))
        A.ExceptionReturn -> pure (conditional address (A.Cond (raw `div` 0x10000000)),[],True,[])
        _ -> Left (FootprintForbidden (GuestAddress address) problem)
      _ -> linear address
    linear address = pure ([address+4],[],False,[])
