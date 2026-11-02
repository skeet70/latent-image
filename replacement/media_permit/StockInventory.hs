{-# LANGUAGE GHC2021 #-}

module StockInventory
  ( GuestAddress (..)
  , FunctionWindow (..)
  , FunctionSize (..)
  , FunctionDeclaration (..)
  , FunctionDerivation (..)
  , FunctionInspection (..)
  , ProgramCounter (..)
  , PremiseUse (..)
  , ReturnPremise (..)
  , requiredReturnPremises
  , returnPremiseLabel
  , checkDeclarations
  , deriveFunction
  , inspectFunction
  , checkedStockFunctions
  , stockFunctions
  ) where

import qualified A32 as A
import Control.Monad (forM, forM_, unless)
import Data.Bits (shiftL, shiftR, (.&.))
import qualified Data.ByteString as B
import Data.List (intercalate, sort)
import qualified Data.Map.Strict as M
import qualified Data.Set as S
import Data.Word (Word32)
import Numeric (showHex)
import ReturnProvenance (CodeNode (..), LiteralWord (..), PremiseUse (..), ProgramCounter (..), ReturnPremise (..), requiredReturnPremises, returnPremiseLabel, verifyReturns)

newtype GuestAddress = GuestAddress Word32 deriving (Eq, Ord, Show)
data FunctionWindow = FunctionWindow GuestAddress GuestAddress deriving (Eq, Show)
newtype FunctionSize = FunctionSize Int deriving (Eq, Ord, Show)
data FunctionDeclaration = FunctionDeclaration GuestAddress FunctionSize deriving (Eq, Ord, Show)
data FunctionDerivation = FunctionDerivation
  { derivedDeclaration :: FunctionDeclaration
  , derivationPremises :: [ReturnPremise]
  , derivationPremiseUses :: [PremiseUse]
  } deriving (Eq, Show)

data FunctionInspection = FunctionInspection
  { inspectedDeclaration :: FunctionDeclaration
  , inspectedCodeWords :: [GuestAddress]
  , inspectedLiteralWords :: [GuestAddress]
  , inspectedReturn :: Either String [PremiseUse]
  } deriving (Eq, Show)

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

checkedStockFunctions :: B.ByteString -> Either String [FunctionDerivation]
checkedStockFunctions image = do
  let outcomes = [(low, deriveFunction (GuestAddress 0xbf1a00) image window []) | window@(FunctionWindow (GuestAddress low) _) <- windows]
      failures = [showHex low "" ++ ": " ++ problem | (low, Left problem) <- outcomes]
      derived = [extent | (_, Right extent) <- outcomes]
  unless (null failures) (Left ("required premises: " ++ intercalate "," (map returnPremiseLabel requiredReturnPremises) ++ "\n" ++ intercalate "\n" failures))
  let declaration (address, size) = FunctionDeclaration (GuestAddress address) (FunctionSize size)
  checkDeclarations (map declaration stockFunctions) (map derivedDeclaration derived)
  pure derived

deriveFunction :: GuestAddress -> B.ByteString -> FunctionWindow -> [GuestAddress] -> Either String FunctionDerivation
deriveFunction imageBase image window externalTails = do
  inspection <- inspectFunction imageBase image window externalTails
  dependencies <- inspectedReturn inspection
  pure (FunctionDerivation
    (inspectedDeclaration inspection)
    (S.toList (S.fromList [premise | PremiseUse _ premise <- dependencies]))
    dependencies)

inspectFunction :: GuestAddress -> B.ByteString -> FunctionWindow -> [GuestAddress] -> Either String FunctionInspection
inspectFunction (GuestAddress imageBase) image (FunctionWindow (GuestAddress low) (GuestAddress high)) externalTails = do
  unless (low < high && low .&. 3 == 0 && high .&. 3 == 0) (Left "invalid function window")
  (code, literals) <- walk [low] S.empty S.empty
  graph <- forM (S.toList code) $ \address -> do
    encoded <- word address
    let instruction = A.decode address encoded
    (successors, _) <- edges address instruction
    pure (address, instruction, successors, A.Cond (encoded `shiftR` 28))
  forM_ graph $ \(address, instruction, _, _) -> case instruction of
    A.Load _ _ _ destination base (A.OffsetRegister _ _ _ _) _ | destination == A.pc && base == A.pc -> do
      let predecessors = S.fromList [source | (source, _, successors, _) <- graph, address `elem` successors]
      unless (address /= low && predecessors == S.singleton (address-4)) (Left "dispatch bound is bypassed")
    _ -> pure ()
  unless (S.null (S.intersection code literals)) (Left "instruction also used as a literal")
  let missing = S.fromList [low,low+4..high-4] S.\\ S.union code literals
  unless (S.null missing) (Left ("unclassified function words at " ++ show (S.toList missing)))
  literalWords <- forM (S.toList literals) $ \address -> do
    value <- word address
    pure (ProgramCounter address, LiteralWord value)
  let dependencies = verifyReturns (M.fromList literalWords) (ProgramCounter low)
        [CodeNode (ProgramCounter address) instruction (map ProgramCounter successors) condition | (address, instruction, successors, condition) <- graph]
  pure (FunctionInspection
    (FunctionDeclaration (GuestAddress low) (FunctionSize (fromIntegral (S.findMax code + 4 - low))))
    (map GuestAddress (S.toList code))
    (map GuestAddress (S.toList literals))
    dependencies)
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
