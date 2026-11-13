-- | Bounds owned frames and the direct call graph. Indirect owned callees are not covered.
module EngineStack
  ( StackBudget (..)
  , taskOneBudget
  , StackReport (..)
  , StackRefusal (..)
  , stackDepth
  , checkEngineStack
  ) where

import qualified A32
import qualified ArmElf as ELF
import Control.Monad (foldM, unless, when)
import Data.List (find)
import qualified Data.Map.Strict as M
import qualified Data.Set as S
import Data.Word (Word32)
import qualified StackWalk as W

-- | Bytes of task 1's stack below `0xbf95c8`'s entry and the part of them kept for stock.
data StackBudget = StackBudget
  { -- | Stack bytes below `0xbf95c8`'s entry.
    budgetHeadroom :: Word32
  , -- | Bytes kept for the stock functions the bank and the engine call.
    budgetStockMargin :: Word32
  }
  deriving (Eq, Show)

-- | Task 1's stack is `[0x00a4c200, 0x00a4ce00)` (`0xc00` bytes, task identity `stack` of
-- `private-data/scratch/ControllerStrategy/picker-runs-2/picker-15-owned-2/e2e-modal-observation.json`,
-- E-RECOVERY-MAINUI-CREATE-150). The ten menu-oracle receipts enter `0xbfa234` from `0xbf95d4`
-- with sp `0x00a4cc68`, so `0xbf95c8` is entered at `0x00a4cc78`: 2680 bytes above the stack's
-- low end. The deepest traced stock site under `0xbf95c8` in those receipts is 248 bytes below
-- its entry (`0xd52684`); the margin keeps four times that for stock callees.
taskOneBudget :: StackBudget
taskOneBudget = StackBudget 2680 1024

data StackReport = StackReport
  { reportEngine :: Word32
  , reportModal :: Word32
  , reportLimit :: Word32
  }
  deriving (Eq, Show)

data StackRefusal
  = InstructionRefused Word32 W.Refused
  | Recursion Word32
  | MissingExtent Word32 Word32
  | DepthOverflow Word32
  | MissingRoot String
  deriving (Eq, Show)

data Function = Function {functionName :: String, functionStart :: Word32, functionEnd :: Word32}

stackDepth :: ELF.Elf -> (Word32 -> Bool) -> String -> Either StackRefusal Word32
stackDepth elf inRuntime root = do
  let functions =
        [ Function (ELF.symbolName symbol) (ELF.symbolValue symbol) (ELF.symbolValue symbol + ELF.symbolSize symbol)
        | symbol <- ELF.elfSymbols elf, ELF.symbolKind symbol == ELF.FunctionSymbol, ELF.symbolSize symbol > 0
        , inRuntime (ELF.symbolValue symbol) ]
      byStart = M.fromList [(functionStart function, function) | function <- functions]
      mapping = M.fromList [(ELF.symbolValue symbol, take 2 (ELF.symbolName symbol) == "$d")
        | symbol <- ELF.elfSymbols elf, take 2 (ELF.symbolName symbol) `elem` ["$a", "$d"]]
      isData address = maybe False snd (M.lookupLE address mapping)
      owner target = case M.lookup target byStart of
        Just function -> Just function
        Nothing -> find (\function -> functionStart function <= target && target < functionEnd function) functions
      code function = [(address, word) | address <- [functionStart function, functionStart function + 4 .. functionEnd function - 4]
        , not (isData address), Just word <- [ELF.wordAt elf address]]
      frame function = foldM (\depth (address, word) -> case W.classify W.Permissive address word of
          W.Reserve amount -> pure (depth + toInteger amount)
          W.Refuse reason | reason `elem` [W.VariableReservation, W.UnaccountedSpWrite, W.FloatingPoint] ->
            Left (InstructionRefused address reason)
          _ -> pure depth) 0 (code function)
      callees function =
        [ (address, target)
        | (address, word) <- code function
        , Just target <- [A32.directTarget address word]
        , inRuntime target
        , let call = case A32.decode address word of A32.Branch _ linked _ -> linked; _ -> True
        , call || target < functionStart function || target >= functionEnd function ]
      depthOf memo visiting function
        | Just depth <- M.lookup (functionStart function) memo = Right (depth, memo)
        | S.member (functionStart function) visiting = Left (Recursion (functionStart function))
        | otherwise = do
            local <- frame function
            (deepest, memo') <- foldM (\(best, current) (address, target) -> do
                callee <- maybe (Left (MissingExtent address target)) Right (owner target)
                (depth, after) <- depthOf current (S.insert (functionStart function) visiting) callee
                pure (max best depth, after)) (0, memo) (callees function)
            let depth = local + deepest
            unless (depth <= toInteger (maxBound :: Word32)) (Left (DepthOverflow (functionStart function)))
            pure (depth, M.insert (functionStart function) depth memo')
  entry <- case [function | function <- functions, functionName function == root] of
    [function] -> Right function
    _ -> Left (MissingRoot root)
  fromInteger . fst <$> depthOf M.empty S.empty entry

-- | The engine's depth from `rb_menu_engine_entry` plus the owned modal dispatcher's depth from
-- `rb_modal_entry`, which can be above it, plus the stock margin, within the headroom.
checkEngineStack :: StackBudget -> ELF.Elf -> (Word32 -> Bool) -> Either String StackReport
checkEngineStack budget elf inRuntime = do
  engine <- either (Left . show) Right (stackDepth elf inRuntime "rb_menu_engine_entry")
  modal <- either (Left . show) Right (stackDepth elf inRuntime "rb_modal_entry")
  let limit = budgetHeadroom budget - budgetStockMargin budget
      report = StackReport engine modal limit
  when (budgetStockMargin budget >= budgetHeadroom budget) (Left "stack bound: the margin exceeds the headroom")
  unless (engine + modal <= limit)
    (Left ("stack bound: engine " ++ show engine ++ " + modal dispatcher " ++ show modal ++ " bytes exceed " ++ show limit))
  pure report

