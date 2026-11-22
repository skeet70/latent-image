{-# LANGUAGE OverloadedStrings #-}
-- | The S3b verdict of one service, folded from its observations as they arrive. A case's
-- observations are held until its four roles are in, then reduced to an 'Outcome'.
module Replacement.RtosScheduler.S3bTally
  ( Role (..)
  , roleLabel
  , comparable
  , Tally
  , emptyTally
  , observe
  , ServiceVerdict (..)
  , serviceVerdict
  ) where

import Data.Aeson (Value, object, (.=))
import Data.Bits ((.&.))
import qualified Data.ByteString as B
import qualified Data.Map.Strict as M
import Data.Maybe (fromMaybe)
import qualified Data.Set as S
import Data.Word (Word32, Word64)
import qualified Emulation.NativeCase as C
import Emulation.NativeObservation
import Emulation.OwnedCase

data Role = Stock | Owned | Equivalent | Mutant deriving (Eq, Ord, Show, Enum, Bounded)

roleLabel :: Role -> String
roleLabel role = case role of Stock -> "stock"; Owned -> "owned"; Equivalent -> "equivalent"; Mutant -> "mutant"

comparable :: CaseSpec -> NativeObservation -> NativeObservation -> Bool
comparable spec a b = nativeStatus a == nativeStatus b && memoryHolds && all same registers && mode a == mode b && visibleWrites a == visibleWrites b
 where
  sp = fromMaybe 0 (lookup Sp (caseInitial spec))
  registers = [R0] ++ (if nativeStatus a==Returned then [R4 .. R11]++[Sp] else [])
  same r = M.lookup r (nativeRegisters a)==M.lookup r (nativeRegisters b)
  mode observation = (.&. 0x1ff) <$> M.lookup Cpsr (nativeRegisters observation)
  stackLow = 0x959700
  memoryHolds = all regionHolds (caseRegions spec)
  regionHolds (CaseRegion raw permission)
    | not (writable permission) = True
    | otherwise = case (M.lookup ident (M.fromList (nativeMemory a)), M.lookup ident (M.fromList (nativeMemory b))) of
        (Just x, Just y) -> liveBytes base x == liveBytes base y
        _ -> False
    where
      ident = regionIdOf raw
      base = fromIntegral (C.baseAddress raw)
  liveBytes base bytes
    | base == stackLow = B.drop (fromIntegral (min (fromIntegral (B.length bytes)) (sp - stackLow))) bytes
    | otherwise = bytes
  visibleWrites :: NativeObservation -> [(Integer, Integer, WriteValue)]
  visibleWrites observation = [(fromIntegral at, fromIntegral size, value) | Just kernel <- [nativeKernel observation], Just order <- [observedOrder kernel], OrderedWrite (Write (GuestAddress at) (WriteBytes size)) value <- order, deviceAt (fromIntegral at) (fromIntegral size)]
  deviceAt :: Integer -> Integer -> Bool
  deviceAt at size = any regionDevice (caseRegions spec)
   where
    regionDevice (CaseRegion raw permission) =
      let base = fromIntegral (C.baseAddress raw)
          top = base + fromIntegral (B.length (C.regionBytes raw))
       in case permission of Device -> base <= at && at + size <= top; _ -> False

writable :: Permission -> Bool
writable permission = case permission of ReadWrite -> True; Device -> True; _ -> False

data Outcome = Outcome
  { outcomeStockStatus :: !RunnerStatus
  , outcomeStockR0 :: !(Maybe Word32)
  , outcomeStockInstructions :: !Word64
  , outcomeOwnedStatus :: !RunnerStatus
  , outcomeOwned :: !Bool
  , outcomeEquivalent :: !Bool
  , outcomeMutant :: !Bool
  }

-- | Case @i@'s stock specification, the pending roles of incomplete cases, completed outcomes and
-- the negative controls, which case 0's stock observation decides.
data Tally = Tally
  { tallyStockSpec :: Int -> CaseSpec
  , tallyPending :: !(M.Map Int (M.Map Role NativeObservation))
  , tallyOutcomes :: !(M.Map Int Outcome)
  , tallyControls :: !(Maybe [Bool])
  }

emptyTally :: (Int -> CaseSpec) -> Tally
emptyTally stockSpec = Tally stockSpec M.empty M.empty Nothing

-- | Adds case @i@'s observation for the role. Only the regions writable in the stock specification are kept.
observe :: Tally -> Int -> Role -> NativeObservation -> Tally
observe tally i role observation = forced kept `seq` controls `seq` completed
 where
  spec = tallyStockSpec tally i
  keep = S.fromList [regionIdOf raw | CaseRegion raw permission <- caseRegions spec, writable permission]
  kept = observation {nativeMemory = [(ident, bytes) | (ident, bytes) <- nativeMemory observation, S.member ident keep]}
  controls = case (role, i) of
    (Stock, 0) -> let held = controlsOf spec kept in foldr seq () held `seq` Just held
    _ -> tallyControls tally
  pending = M.insert role kept (M.findWithDefault M.empty i (tallyPending tally))
  completed = case traverse (`M.lookup` pending) [minBound .. maxBound] of
    Just [stock, owned, equivalent, mutant] ->
      let outcome = Outcome
            { outcomeStockStatus = forcedStatus (nativeStatus stock)
            , outcomeStockR0 = M.lookup R0 (nativeRegisters stock)
            , outcomeStockInstructions = instructionCount (nativeInstructions stock)
            , outcomeOwnedStatus = forcedStatus (nativeStatus owned)
            , outcomeOwned = comparable spec stock owned
            , outcomeEquivalent = comparable spec stock equivalent
            , outcomeMutant = comparable spec stock mutant
            }
       in outcome `seq` Tally (tallyStockSpec tally) (M.delete i (tallyPending tally)) (M.insert i outcome (tallyOutcomes tally)) controls
    _ -> Tally (tallyStockSpec tally) (M.insert i pending (tallyPending tally)) (tallyOutcomes tally) controls
  forced value = foldr (\(RegionId name, bytes) rest -> length name `seq` bytes `seq` rest) () (nativeMemory value)
  forcedStatus status = case status of OtherStatus text -> length text `seq` status; _ -> status

controlsOf :: CaseSpec -> NativeObservation -> [Bool]
controlsOf fixture reference =
  [refuses (corruptByte "tasks-a" 0x0c), refuses (corruptByte "globals-a" 0), refuses (corruptByte "stack" 0x100), comparable fixture reference (corruptByte "stack" 0 reference)]
 where
  refuses change = not (comparable fixture reference (change reference))
  corruptByte name offset observation = observation { nativeMemory = [(ident, if regionName ident == name then B.take offset bytes <> B.singleton (B.index bytes offset + 1) <> B.drop (offset + 1) bytes else bytes) | (ident, bytes) <- nativeMemory observation] }

data ServiceVerdict = ServiceVerdict
  { verdictHolds :: Bool
  , verdictReport :: Value
  , verdictCaseRows :: [Value]
  , verdictFailures :: Int
  , verdictControls :: [Value]
  , verdictObservationControls :: [Value]
  , verdictCompleted :: Bool
  , verdictOwnedMatches :: Int
  -- | Case, stock status and owned status of each case whose owned observation differs.
  , verdictOwnedDifferences :: [(Int, RunnerStatus, RunnerStatus)]
  }

-- | The service's verdict from the expected stock result of each case, the handler, extent and
-- in-extent stock instructions of each case with stock evidence, and the reused observation count.
-- Left names a case not observed in every role.
serviceVerdict :: Bool -> String -> [(RunnerStatus, Word32)] -> M.Map Int (Word32, Word32, Int) -> Int -> Tally -> Either String ServiceVerdict
serviceVerdict domainB service expectations evidence reused tally = do
  observed <- traverse (\i -> maybe (Left ("S3b case " ++ show i ++ " was not observed in every role")) Right (M.lookup i (tallyOutcomes tally))) [0 .. count - 1]
  let indexed = zip [0 :: Int ..] observed
      completed = and [outcomeStockStatus o == status && outcomeStockR0 o == Just value | ((_, o), (status, value)) <- zip indexed expectations]
      matches field = length [() | (_, o) <- indexed, field o]
      controls = fromMaybe [] (tallyControls tally)
      negativeControl = length controls == 4 && and controls
      controlRows :: [(String, Bool)]
      controlRows = zip ["task-state", "globals-word", "stack-byte", "stack-base"] controls
      stockInstructions = [object ["case".=i,"instructions".=outcomeStockInstructions o] | (i, o) <- indexed]
      caseDifferences o = [label | (label, field) <- [("owned", outcomeOwned), ("equivalent", outcomeEquivalent)], not (field o)] :: [String]
      reportedCases = [(i, o, found) | (i, o) <- indexed, Just found <- [M.lookup i evidence]]
      caseRows = [ object
          [ "id" .= ("stock" ++ show i)
          , "case" .= i
          , "entry" .= handler
          , "extent" .= size
          , "stock_instructions_inside" .= inside
          , "stock_instructions" .= outcomeStockInstructions o
          , "status" .= show (outcomeStockStatus o)
          , "differences" .= caseDifferences o
          ]
        | (i, o, (handler, size, inside)) <- reportedCases
        ]
      failures = length [() | (_, o, _) <- reportedCases, not (null (caseDifferences o))]
      mutantRows = [ object
          [ "name" .= ("mutant-" ++ service)
          , "expected" .= ("the service entry replaced by mvn r0,#16; bx lr is reported different" :: String)
          , "reported" .= (matches outcomeMutant < count)
          , "differences" .= ([] :: [String])
          , "diagnostic_differences" .= ([] :: [String])
          ] ]
      observationRows = [object ["name".=name, "reported".=held] | (name,held)<-controlRows]
      holds = completed && matches outcomeOwned==count && matches outcomeEquivalent==count && matches outcomeMutant<count && negativeControl
      report = object ["domain_b".=domainB,"service".=service,"cases".=count,"case_rows".=caseRows,"reused_observations".=reused,"stock_completed".=completed,"owned_matches".=matches outcomeOwned,"equivalent_matches".=matches outcomeEquivalent,"mutant_matches".=matches outcomeMutant,"negative_control".=negativeControl,"controls".=mutantRows,"observation_controls".=observationRows,"negative_controls".=[object ["control".=name,"holds".=held] | (name,held)<-controlRows],"stock_instructions".=stockInstructions,"passed".=holds]
  pure ServiceVerdict
    { verdictHolds = holds
    , verdictReport = report
    , verdictCaseRows = caseRows
    , verdictFailures = failures
    , verdictControls = mutantRows
    , verdictObservationControls = observationRows
    , verdictCompleted = completed
    , verdictOwnedMatches = matches outcomeOwned
    , verdictOwnedDifferences = [(i, outcomeStockStatus o, outcomeOwnedStatus o) | (i, o) <- indexed, not (outcomeOwned o)]
    }
 where
  count = length expectations
