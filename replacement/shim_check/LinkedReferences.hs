module LinkedReferences (linkedReferences, allocatedAddress) where

import qualified A32 as A
import qualified ArmElf as E
import Data.Bits ((.&.), (.|.), complement, shiftL, xor)
import Data.List (isPrefixOf, sortOn)
import qualified Data.Map.Strict as M
import qualified Data.Set as S
import Data.Word (Word32)
import qualified StackWalk as W

linkedReferences :: E.Elf -> Either String (S.Set Word32)
linkedReferences elf = do
  relocations <- E.addressRelocations elf
  pure (S.unions (S.fromList (map E.addressTarget relocations) : direct : map walk functions))
  where
    globalNames = S.fromList (map E.symbolName (E.globalSymbols elf))
    functions = S.toList (S.fromList [(E.symbolValue symbol,E.symbolSize symbol) | symbol <- E.elfSymbols elf, E.symbolKind symbol == E.FunctionSymbol || E.symbolKind symbol == E.OtherSymbol && S.member (E.symbolName symbol) globalNames, E.symbolSize symbol > 0, E.symbolSection symbol /= E.Undefined, E.symbolSection symbol /= E.Absolute])
    sections = zip [0 :: Int ..] (E.elfSections elf)
    code = M.fromList
      [ (address,word)
      | (index,section) <- sections, E.sectionExecutable section, E.sectionAllocated section, E.sectionSize section >= 4
      , let start = E.sectionAddress section
      , address <- [start,start+4 .. start+E.sectionSize section-4]
      , isCode index address, Just word <- [E.wordAt elf address]
      ]
    mappings = M.fromList
      [(index,sortOn fst [(E.symbolValue symbol,"$a" `isPrefixOf` E.symbolName symbol) | symbol <- E.elfSymbols elf, E.symbolSection symbol == E.InSection index, any (`isPrefixOf` E.symbolName symbol) ["$a","$d","$t"]]) | (index,_) <- sections]
    isCode index address = case takeWhile ((<= address) . fst) (M.findWithDefault [] index mappings) of
      [] -> True
      markers -> snd (last markers)
    direct = S.fromList [target | (address,word) <- M.toList code, Just target <- [E.directBranchTarget address word]]
    walk (start,size) = visit M.empty [(start,M.empty)] S.empty
      where
        inside address = start <= address && toInteger address < toInteger start + toInteger size && M.member address code
        visit _ [] targets = targets
        visit seen ((address,incoming):rest) targets
          | not (inside address) = visit seen rest targets
          | otherwise =
              let old = M.lookup address seen
                  registers = maybe incoming (`meet` incoming) old
               in if old == Just registers then visit seen rest targets else
                    case M.lookup address code of
                      Nothing -> visit seen rest targets
                      Just word ->
                        let instruction = A.decode address word
                            (taken,after) = transfer address word instruction registers
                            destinations = successors address instruction registers taken after
                            found = transferTargets address instruction registers taken
                         in visit (M.insert address registers seen) (filter (inside . fst) destinations ++ rest) (S.union targets (S.fromList found))
    meet left right = M.filterWithKey (\register constant -> M.lookup register right == Just constant) left
    value address registers register
      | register == A.pc = Just (address+8)
      | otherwise = M.lookup register registers
    transfer address word instruction before = case instruction of
      A.MoveWide condition destination immediate -> assign condition destination (Just immediate)
      A.MoveTop condition destination immediate -> assign condition destination ((\low -> (low .&. 0xffff) .|. (immediate `shiftL` 16)) <$> M.lookup destination before)
      A.DataProcessing condition operation _ destination source operand
        | operation `elem` [A.CMP,A.CMN,A.TST,A.TEQ] -> (before,before)
        | otherwise -> assign condition destination (calculate operation (value address before source) (operandValue address before operand))
      A.Load condition A.Word False destination base (A.OffsetImmediate offset) A.OffsetOnly
        | base == A.pc -> assign condition destination (E.wordAt elf (address+8+fromIntegral offset))
      _ -> let after = foldr M.delete before (writes address word instruction) in (after,after)
      where
        assign condition destination known =
          let after = maybe (M.delete destination before) (\constant -> M.insert destination constant before) known
           in (after,if condition == A.Cond 14 then after else meet before after)
    writes address word instruction = W.writes address word ++ case instruction of
      A.Load _ _ _ _ base _ indexing | indexing /= A.OffsetOnly -> [base]
      A.Store _ _ _ base _ indexing | indexing /= A.OffsetOnly -> [base]
      A.LoadMultiple _ base _ True _ -> [base]
      A.StoreMultiple _ base _ True _ -> [base]
      _ -> []
    operandValue address registers operand = case operand of
      A.Immediate immediate -> Just immediate
      A.ShiftedImmediate register A.LSL 0 -> value address registers register
      _ -> Nothing
    calculate operation left right = case operation of
      A.MOV -> right
      A.MVN -> complement <$> right
      A.ADD -> (+) <$> left <*> right
      A.SUB -> (-) <$> left <*> right
      A.ORR -> (.|.) <$> left <*> right
      A.AND -> (.&.) <$> left <*> right
      A.BIC -> (\a b -> a .&. complement b) <$> left <*> right
      A.EOR -> xor <$> left <*> right
      _ -> Nothing
    successors address instruction before taken after = case instruction of
      A.Branch condition False target -> (target,before) : fallthrough condition
      A.Branch _ True _ -> [(address+4,after)]
      A.BranchExchange _ True _ -> [(address+4,after)]
      A.BranchExchange condition False register -> [(target,before) | Just target <- [value address before register]] ++ fallthrough condition
      A.Load condition _ _ destination _ _ _ | destination == A.pc -> [(target,taken) | Just target <- [M.lookup A.pc taken]] ++ fallthrough condition
      A.LoadMultiple condition _ _ _ registers | A.pc `elem` registers -> fallthrough condition
      A.DataProcessing condition _ _ destination _ _ | destination == A.pc -> [(target,taken) | Just target <- [M.lookup A.pc taken]] ++ fallthrough condition
      A.Rejected _ -> []
      _ -> [(address+4,after)]
      where fallthrough condition = if condition == A.Cond 14 then [] else [(address+4,before)]
    transferTargets address instruction before after = case instruction of
      A.Branch _ _ target -> [target]
      A.BranchExchange _ _ register -> maybe [] pure (value address before register)
      A.Load _ _ _ destination _ _ _ | destination == A.pc -> maybe [] pure (M.lookup A.pc after)
      A.DataProcessing _ _ _ destination _ _ | destination == A.pc -> maybe [] pure (M.lookup A.pc after)
      _ -> []

allocatedAddress :: E.Elf -> Word32 -> Bool
allocatedAddress elf address = any contains (E.elfSections elf)
  where contains section = E.sectionAllocated section && E.sectionAddress section <= address && toInteger address < toInteger (E.sectionAddress section) + toInteger (E.sectionSize section)
