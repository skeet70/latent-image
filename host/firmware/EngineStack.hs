-- | A static stack-depth bound over a linked bank ELF: each function's frame is the sum of the
-- stack reservations on its reachable instructions, and a function's depth is its frame plus the
-- deepest function it calls or branches to. Calls and branches outside the runtime are stock
-- callees, whose own depth the budget's margin covers.
module EngineStack
  ( StackBudget (..)
  , taskOneBudget
  , StackReport (..)
  , stackDepth
  , checkEngineStack
  ) where

import qualified A32
import qualified ArmElf as ELF
import Control.Monad (foldM, unless, when)
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

data Function = Function {functionName :: String, functionStart :: Word32, functionEnd :: Word32}

data Exit = Call Word32 | StockCall deriving (Eq, Ord)

-- | Bank functions whose register branch calls the controller action the stock dispatcher's
-- tables select; the action runs where a stock dispatch would run it, so it counts as stock.
actionCallers :: [String]
actionCallers = ["rb_modal_call"]

-- | The reachable instructions of one function from its entry: the frame and the exits. Code
-- reaches data only after a call that does not return.
walk :: ELF.Elf -> (Word32 -> Bool) -> (Word32 -> Bool) -> Function -> Either String (Word32, [Exit])
walk elf inRuntime isData function = go S.empty [functionStart function] 0 []
 where
  inside address = functionStart function <= address && address < functionEnd function
  always (A32.Cond c) = c == 14
  name = functionName function
  go _ [] frame exits = Right (frame, exits)
  go seen (address : rest) frame exits
    | S.member address seen = go seen rest frame exits
    | inside address && isData address = go seen rest frame exits
    | not (inside address) = Left (name ++ ": falls through its end at " ++ hex address)
    | otherwise = do
        word <- maybe (Left (name ++ ": no code at " ++ hex address)) Right (ELF.wordAt elf address)
        let seen' = S.insert address seen
            next = address + 4
            continue extra more = go seen' (more ++ [next] ++ rest) (frame + extra) exits
            stopIf cond extra more newExits = if always cond
              then go seen' (more ++ rest) (frame + extra) (newExits ++ exits)
              else go seen' (more ++ [next] ++ rest) (frame + extra) (newExits ++ exits)
            call target
              | inRuntime target = Call target
              | otherwise = StockCall
            exitsThrough register = case constantsIn register of
              Left _ | name `elem` actionCallers -> Right [StockCall]
              other -> map call <$> other
            -- A register branch's targets: a movw/movt pair or a literal load in the eight
            -- instructions before it, or for r4-r11 every constant the function writes to it.
            constantsIn register = case resolve register (address - 4) (8 :: Int) Nothing of
              Right target -> Right [target]
              Left _ | register `elem` map A32.Reg [4 .. 11] -> everywhere register
              Left problem -> Left problem
            everywhere register = do
              let code = [at | at <- [functionStart function, functionStart function + 4 .. functionEnd function - 4], not (isData at)]
                  decoded = [(at, instruction) | at <- code, Just instruction <- [A32.decode at <$> ELF.wordAt elf at]]
                  restore instruction = case instruction of
                    A32.LoadMultiple _ base _ _ _ -> base == A32.sp
                    _ -> False
                  writes = [(at, instruction) | (at, instruction) <- decoded, register `elem` W.written instruction, not (restore instruction)]
                  topAfter at = case [value | (later, A32.MoveTop _ rd value) <- decoded, rd == register, later > at, later <= at + 32] of
                    value : _ -> Just value
                    [] -> Nothing
              targets <- fmap concat . mapM (\(at, instruction) -> case instruction of
                  A32.MoveWide _ _ value -> Right [maybe value (\top -> top * 0x10000 + value) (topAfter at)]
                  A32.MoveTop {} -> Right []
                  _ -> (: []) <$> resolve register at (1 :: Int) Nothing) $ writes
              if null targets then unresolved else Right targets
            unresolved = Left (name ++ ": register branch at " ++ hex address ++ " has no constant target")
            resolve register at budget high
              | budget == 0 || not (inside at) = unresolved
              | otherwise = case A32.decode at <$> ELF.wordAt elf at of
                  Nothing -> unresolved
                  Just instruction -> case instruction of
                    A32.MoveTop _ rd value | rd == register && high == Nothing -> resolve register (at - 4) (budget - 1) (Just value)
                    A32.MoveWide _ rd value | rd == register -> Right (maybe value (\top -> top * 0x10000 + value) high)
                    A32.Load _ A32.Word False rt base (A32.OffsetImmediate offset) A32.OffsetOnly
                      | rt == register && base == A32.pc && high == Nothing ->
                          maybe unresolved Right (ELF.wordAt elf (fromIntegral (toInteger at + 8 + toInteger offset)))
                    _ | register `elem` W.written instruction -> unresolved
                    _ -> resolve register (at - 4) (budget - 1) high
            unaccounted = Left (name ++ ": stack pointer write at " ++ hex address ++ " is neither an immediate adjustment nor a restore from r11")
        case W.classify W.Permissive address word of
          W.Reserve amount -> continue amount []
          W.Release _ -> continue 0 []
          W.FromFrame _ -> continue 0 []
          W.SetFrame _ -> continue 0 []
          W.Neutral -> continue 0 []
          W.ModeWrite _ -> continue 0 []
          W.Return cond _ -> stopIf cond 0 [] []
          W.Call target -> go seen' ([next | inside next] ++ rest) frame (call target : exits)
          W.Jump cond target
            | inside target -> stopIf cond 0 [target] []
            | otherwise -> stopIf cond 0 [] [call target]
          W.RegisterCall register -> do
            targets <- exitsThrough register
            go seen' ([next | inside next] ++ rest) frame (targets ++ exits)
          W.RegisterJump cond register -> do
            targets <- exitsThrough register
            stopIf cond 0 [] targets
          W.JumpTable _ _ -> Left (name ++ ": loaded branch at " ++ hex address)
          W.Stop -> go seen' rest frame exits
          W.Refuse reason -> case reason of
            W.VariableReservation -> Left (name ++ ": variable stack reservation at " ++ hex address)
            W.ComputedBranch -> Left (name ++ ": computed branch at " ++ hex address)
            W.LoadedBranch -> Left (name ++ ": loaded branch at " ++ hex address)
            W.FloatingPoint -> Left (name ++ ": floating-point instruction at " ++ hex address ++ " is not accounted")
            _ -> unaccounted

-- | The depth of `root`: its frame plus the deepest of its callees, recursively.
stackDepth :: ELF.Elf -> (Word32 -> Bool) -> String -> Either String Word32
stackDepth elf inRuntime root = do
  let functions =
        [ Function (ELF.symbolName symbol) (ELF.symbolValue symbol) (ELF.symbolValue symbol + ELF.symbolSize symbol)
        | symbol <- ELF.elfSymbols elf, ELF.symbolKind symbol == ELF.FunctionSymbol, ELF.symbolSize symbol > 0
        , inRuntime (ELF.symbolValue symbol) ]
      byStart = M.fromList [(functionStart function, function) | function <- functions]
      -- ARM mapping symbols: `$d` starts data such as a literal pool, `$a` starts code again.
      mapping = M.fromList [(ELF.symbolValue symbol, ELF.symbolName symbol == "$d")
        | symbol <- ELF.elfSymbols elf, ELF.symbolName symbol `elem` ["$a", "$d"]]
      isData address = maybe False snd (M.lookupLE address mapping)
  entry <- case [function | function <- functions, functionName function == root] of
    [function] -> Right function
    _ -> Left ("stack bound: one function named " ++ root ++ " required")
  let depthOf memo visiting function
        | Just depth <- M.lookup (functionStart function) memo = Right (depth, memo)
        | S.member (functionStart function) visiting = Left ("stack bound: recursion through " ++ functionName function)
        | otherwise = do
            (frame, exits) <- walk elf inRuntime isData function
            (deepest, memo') <- foldM (\(best, current) exit -> case exit of
                StockCall -> Right (best, current)
                Call target -> do
                  callee <- maybe (Left (functionName function ++ ": call to " ++ hex target ++ " enters no function")) Right
                    (M.lookup target byStart)
                  (depth, after) <- depthOf current (S.insert (functionStart function) visiting) callee
                  Right (max best depth, after))
              (0, memo) exits
            let depth = frame + deepest
            Right (depth, M.insert (functionStart function) depth memo')
  fst <$> depthOf M.empty S.empty entry

-- | The engine's depth from `rb_menu_engine_entry` plus the owned modal dispatcher's depth from
-- `rb_modal_entry`, which can be above it, plus the stock margin, within the headroom.
checkEngineStack :: StackBudget -> ELF.Elf -> (Word32 -> Bool) -> Either String StackReport
checkEngineStack budget elf inRuntime = do
  engine <- stackDepth elf inRuntime "rb_menu_engine_entry"
  modal <- stackDepth elf inRuntime "rb_modal_entry"
  let limit = budgetHeadroom budget - budgetStockMargin budget
      report = StackReport engine modal limit
  when (budgetStockMargin budget >= budgetHeadroom budget) (Left "stack bound: the margin exceeds the headroom")
  unless (engine + modal <= limit)
    (Left ("stack bound: engine " ++ show engine ++ " + modal dispatcher " ++ show modal ++ " bytes exceed " ++ show limit))
  pure report

hex :: Word32 -> String
hex value = "0x" ++ go value ""
 where
  go v acc = let (q, r) = v `divMod` 16; digit = "0123456789abcdef" !! fromIntegral r
             in if q == 0 then digit : acc else go q (digit : acc)
