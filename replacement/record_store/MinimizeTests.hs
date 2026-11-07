{-# LANGUAGE GHC2021 #-}
module Main (main) where

import Control.Monad (unless)
import qualified Data.ByteString as B
import Data.IORef (newIORef, readIORef, writeIORef)
import qualified Data.Map.Strict as M
import Data.Word (Word16)
import Replacement.RecordStore.Cases hiding (bank)
import Replacement.RecordStore.Generate (ValidCase, oracleCase, validCase)
import Replacement.RecordStore.Minimize hiding (Medium)
import Replacement.RecordStore.Model

assert :: String -> Bool -> IO ()
assert message condition = unless condition (error message)

fixture :: String -> [Operation] -> [(Word16, Bool)] -> [(Word16, Bool)] -> Callback -> OracleCase
fixture label bodyOperations bank0 bank1 callback = OracleCase (CaseName ("minimize-" ++ label)) Kind0 (mediumFor bank0 bank1) (Open : Register Normal : bodyOperations) callback NoFault
  where
    g = geometry Kind0
    mediumFor a b = putBank g Bank1 (bankBytesFor b) (putBank g Bank0 (bankBytesFor a) (Medium M.empty))
    bankBytesFor records = B.concat [recordBytesFor sequenceNumber rejected | (sequenceNumber, rejected) <- records] <> B.replicate (blockBytes - length records * recordBytes g) 0xff
    recordBytesFor sequenceNumber rejected = half sequenceNumber <> B.singleton (if rejected then 0xee else 0x22) <> B.replicate (recordBytes g - 3) 0x33

valid :: String -> OracleCase -> ValidCase
valid label c = case validCase c of
  Left refusal -> error (label ++ ": " ++ show refusal)
  Right accepted -> accepted

-- Every oracle input is re-validated here, so an invalid proposal reaching the
-- oracle fails the test instead of being minimized.
checkedOracle :: (OracleCase -> Bool) -> ValidCase -> IO Trial
checkedOracle predicate candidate = do
  case validCase (oracleCase candidate) of
    Left refusal -> error ("oracle received invalid proposal: " ++ show refusal)
    Right _ -> pure ()
  pure (if predicate (oracleCase candidate) then Differs [Results] else Matches)

constantOracle :: Trial -> ValidCase -> IO Trial
constantOracle result candidate = do
  case validCase (oracleCase candidate) of
    Left refusal -> error ("oracle received invalid proposal: " ++ show refusal)
    Right _ -> pure ()
  pure result

occupancy :: Bank -> OracleCase -> Int
occupancy bank c = length (takeWhile occupied [0 .. recordCount g - 1])
  where
    g = geometry (kind c)
    bytes = bankBytes g bank (initialMedium c)
    occupied position
      | (position + 1) * recordBytes g > B.length bytes = False
      | otherwise = sequenceOf (recordAt g bank (Position position) (initialMedium c)) /= Sequence 0xffff

recordSequence :: Bank -> Int -> OracleCase -> Word16
recordSequence bank position c =
  let Sequence sequenceNumber = sequenceOf (recordAt g bank (Position position) (initialMedium c))
  in sequenceNumber
  where
    g = geometry (kind c)

isStagingAppend :: Operation -> Bool
isStagingAppend (Append _ StagingSource) = True
isStagingAppend _ = False

body :: [Operation]
body =
  [ Latest
  , Append (Seed 0x1234) StagingSource
  , Latest
  , ReadAt (BytePosition 0x20000) 16
  , Zero
  , Close
  , Register Normal
  , Latest
  ]

originalCase :: ValidCase
originalCase = valid "original" (fixture "original" body [(10, False), (11, False), (12, False)] [(20, False)] CorrectingCallback)

-- Synthetic predicates isolate reducer behavior; they are not firmware oracle
-- evidence. The domain cases are validated by the shared smart constructor.
main :: IO ()
main = do
  budgetValidation
  stagingReduction
  readPreservation
  mediumReduction
  callbackReduction
  baselineNonreproduction
  differentFailure
  baselineInconclusive
  completeRejectionReachesFixedPoint
  inconclusiveRejection
  unresolvedClasses
  unresolvedAfterPartialReduction
  acceptanceResetsUnresolved
  budgetEnforcement
  idempotentFixedPoint
  putStrLn "record-store minimization tests passed"

budgetValidation :: IO ()
budgetValidation = do
  assert "budget zero refused" (case mkShrinkBudget 0 of Left _ -> True; Right _ -> False)
  assert "budget above maximum refused" (case mkShrinkBudget 257 of Left _ -> True; Right _ -> False)
  assert "budget one accepted" (case mkShrinkBudget 1 of Right _ -> True; Left _ -> False)
  assert "budget maximum accepted" (case mkShrinkBudget 256 of Right _ -> True; Left _ -> False)

stagingReduction :: IO ()
stagingReduction = do
  budget <- either error pure (mkShrinkBudget 256)
  let predicate = any isStagingAppend . operations
  result <- shrinkCase budget Results (checkedOracle predicate) originalCase
  let minimized = oracleCase (minimizedCase result)
  assert "staging predicate survives reduction" (predicate minimized)
  assert "reduction removes operations" (length (operations minimized) < length (operations (oracleCase originalCase)))
  assert "reduction removes every occupied record" (occupancy Bank0 minimized == 0 && occupancy Bank1 minimized == 0)
  assert "minimal staging case keeps only the mandatory prefix and append" (operations minimized == [Open, Register Normal, Append (Seed 0) StagingSource])
  assert "seed simplification preserves the source" (case operations minimized of [_, _, Append (Seed 0) StagingSource] -> True; _ -> False)
  assert "reduction reaches fixed point" (reductionStatus result == FixedPoint)
  assert "reduction stays inside budget" (trialCount result <= 256)
  assert "reduction tries more than the baseline" (trialCount result > 1)
  assert "reduction preserves kind" (kind minimized == kind (oracleCase originalCase))
  assert "reduction preserves name" (name minimized == name (oracleCase originalCase))
  assert "reduction preserves fault domain" (injection minimized == NoFault)
  assert "reduction preserves mandatory prefix" (take 2 (operations minimized) == [Open, Register Normal])

readPreservation :: IO ()
readPreservation = do
  budget <- either error pure (mkShrinkBudget 256)
  let predicate = any (== ReadAt (BytePosition 0x20000) 16) . operations
  result <- shrinkCase budget Results (checkedOracle predicate) originalCase
  let minimized = oracleCase (minimizedCase result)
  assert "read predicate survives reduction" (predicate minimized)
  assert "position and length are not simplified away from the predicate" (operations minimized == [Open, Register Normal, ReadAt (BytePosition 0x20000) 16])
  assert "read reduction removes every occupied record" (occupancy Bank0 minimized == 0 && occupancy Bank1 minimized == 0)
  assert "read reduction reaches fixed point" (reductionStatus result == FixedPoint)

mediumReduction :: IO ()
mediumReduction = do
  budget <- either error pure (mkShrinkBudget 256)
  let predicate c = occupancy Bank0 c > 0 || occupancy Bank1 c > 0
  result <- shrinkCase budget Results (checkedOracle predicate) originalCase
  let minimized = oracleCase (minimizedCase result)
  assert "record predicate survives reduction" (predicate minimized)
  assert "record reduction removes the body" (operations minimized == [Open, Register Normal])
  assert "record reduction keeps one occupied record" (occupancy Bank0 minimized + occupancy Bank1 minimized == 1)
  assert "remaining record sequence simplifies to zero" (recordSequence Bank1 0 minimized == 0 || recordSequence Bank0 0 minimized == 0)
  assert "medium reduction reaches fixed point" (reductionStatus result == FixedPoint)

callbackReduction :: IO ()
callbackReduction = do
  budget <- either error pure (mkShrinkBudget 256)
  let predicate c = callbacks c /= NoCallback
  result <- shrinkCase budget Results (checkedOracle predicate) originalCase
  let minimized = oracleCase (minimizedCase result)
  assert "callback predicate survives reduction" (callbacks minimized == MarkerCallback)
  assert "callback reduction reaches fixed point" (reductionStatus result == FixedPoint)
  assert "callback reduction removes every occupied record" (occupancy Bank0 minimized == 0 && occupancy Bank1 minimized == 0)

baselineNonreproduction :: IO ()
baselineNonreproduction = do
  budget <- either error pure (mkShrinkBudget 256)
  result <- shrinkCase budget Results (constantOracle Matches) originalCase
  assert "matching baseline reports nonreproduction" (reductionStatus result == OriginalDoesNotReproduce)
  assert "matching baseline is the only trial" (trialCount result == 1)
  assert "matching baseline is returned unchanged" (minimizedCase result == originalCase)

differentFailure :: IO ()
differentFailure = do
  budget <- either error pure (mkShrinkBudget 256)
  result <- shrinkCase budget Results (constantOracle (Differs [Caller])) originalCase
  assert "different boundary reports nonreproduction" (reductionStatus result == OriginalDoesNotReproduce)
  assert "different boundary is the only trial" (trialCount result == 1)
  assert "different boundary is returned unchanged" (minimizedCase result == originalCase)

-- A blocked original trial neither reproduces nor fails to reproduce the
-- boundary; the block cause is reported and no proposal is tried.
baselineInconclusive :: IO ()
baselineInconclusive = mapM_ check
  [ StockModelDisagreement "synthetic stock/model disagreement"
  , UnsupportedNativeStatus "synthetic unsupported native status"
  , DomainRefusal "synthetic refusal"
  , InfrastructureFailure "synthetic infrastructure failure"
  ]
  where
    check block = do
      budget <- either error pure (mkShrinkBudget 256)
      calls <- newIORef (0 :: Int)
      let oracle _ = do
            writeIORef calls . (+ 1) =<< readIORef calls
            pure (Inconclusive block)
      result <- shrinkCase budget Results oracle originalCase
      observed <- readIORef calls
      assert ("baseline block is reported: " ++ show block) (reductionStatus result == BaselineInconclusive block)
      assert ("baseline block stops after one trial: " ++ show block) (trialCount result == 1 && observed == 1)
      assert ("baseline block keeps the original: " ++ show block) (minimizedCase result == originalCase)

-- Complete rejected outcomes are sufficient for FixedPoint.
completeRejectionReachesFixedPoint :: IO ()
completeRejectionReachesFixedPoint = do
  budget <- either error pure (mkShrinkBudget 256)
  let oracle candidate
        | oracleCase candidate == oracleCase originalCase = pure (Differs [Results])
        | even (length (operations (oracleCase candidate))) = pure (Differs [Caller])
        | otherwise = pure Matches
  result <- shrinkCase budget Results oracle originalCase
  assert "complete rejections establish fixed point" (reductionStatus result == FixedPoint)
  assert "complete rejections keep the original" (minimizedCase result == originalCase)
  assert "complete rejections were tried" (trialCount result > 1)

inconclusiveRejection :: IO ()
inconclusiveRejection = do
  budget <- either error pure (mkShrinkBudget 256)
  let oracle candidate
        | oracleCase candidate == oracleCase originalCase = pure (Differs [Results])
        | otherwise = pure (Inconclusive (DomainRefusal "synthetic refusal"))
  result <- shrinkCase budget Results oracle originalCase
  assert "inconclusive candidate is never accepted" (minimizedCase result == originalCase)
  assert "inconclusive candidates leave minimality unresolved" (reductionStatus result == UnresolvedTrials)
  assert "inconclusive candidates were tried" (trialCount result > 1)
  let different candidate
        | oracleCase candidate == oracleCase originalCase = pure (Differs [Results])
        | otherwise = pure (Differs [Caller])
  result' <- shrinkCase budget Results different originalCase
  assert "different candidate failure is never accepted" (minimizedCase result' == originalCase)
  assert "different candidate failure still exhausts proposals" (reductionStatus result' == FixedPoint)
  assert "different candidate failure was tried" (trialCount result' > 1)

-- Every way a trial can stay unresolved blocks local minimality.
unresolvedClasses :: IO ()
unresolvedClasses = do
  budget <- either error pure (mkShrinkBudget 256)
  mapM_ (check budget)
    [ StockModelDisagreement "synthetic stock/model disagreement"
    , UnsupportedNativeStatus "synthetic unsupported native status"
    , DomainRefusal "synthetic refusal"
    , InfrastructureFailure "synthetic infrastructure failure"
    ]
  where
    check budget block = do
      let oracle candidate
            | oracleCase candidate == oracleCase originalCase = pure (Differs [Results])
            | otherwise = pure (Inconclusive block)
      result <- shrinkCase budget Results oracle originalCase
      assert ("blocked class is never accepted: " ++ show block) (minimizedCase result == originalCase)
      assert ("blocked class leaves minimality unresolved: " ++ show block) (reductionStatus result == UnresolvedTrials)
      assert ("blocked class was tried: " ++ show block) (trialCount result > 1)

-- The last proven case is retained when remaining proposals stay unresolved.
unresolvedAfterPartialReduction :: IO ()
unresolvedAfterPartialReduction = do
  budget <- either error pure (mkShrinkBudget 256)
  let predicate = any isStagingAppend . operations
      oracle candidate
        | predicate (oracleCase candidate) = pure (Differs [Results])
        | otherwise = pure (Inconclusive (DomainRefusal "synthetic refusal"))
  result <- shrinkCase budget Results oracle originalCase
  let minimized = minimizedCase result
  reproduced <- oracle minimized
  assert "partial reduction keeps a proven counterexample"
    (case reproduced of Differs boundaries -> Results `elem` boundaries; _ -> False)
  assert "partial reduction is strictly smaller"
    (length (operations (oracleCase minimized)) < length (operations (oracleCase originalCase)))
  assert "partial reduction reports unresolved minimality" (reductionStatus result == UnresolvedTrials)
  assert "partial reduction stays inside budget" (trialCount result <= 256)
  assert "partial reduction keeps the mandatory prefix" (take 2 (operations (oracleCase minimized)) == [Open, Register Normal])

-- Unresolved proposals for an earlier current case do not affect the final
-- status.
acceptanceResetsUnresolved :: IO ()
acceptanceResetsUnresolved = do
  budget <- either error pure (mkShrinkBudget 256)
  blocked <- newIORef False
  let latestCount c = length (filter (== Latest) (operations c))
      oracle candidate
        | oracleCase candidate == oracleCase originalCase = pure (Differs [Results])
        | latestCount (oracleCase candidate) >= 2 =
            writeIORef blocked True >> pure (Inconclusive (DomainRefusal "synthetic refusal"))
        | any isStagingAppend (operations (oracleCase candidate)) = pure (Differs [Results])
        | otherwise = pure Matches
  result <- shrinkCase budget Results oracle originalCase
  sawBlocked <- readIORef blocked
  let minimized = minimizedCase result
  assert "an unresolved proposal was tried during reduction" sawBlocked
  assert "accepting a smaller case clears earlier unresolved status" (reductionStatus result == FixedPoint)
  assert "reset run keeps a proven counterexample" (any isStagingAppend (operations (oracleCase minimized)))
  assert "reset run is strictly smaller"
    (length (operations (oracleCase minimized)) < length (operations (oracleCase originalCase)))

budgetEnforcement :: IO ()
budgetEnforcement = do
  one <- either error pure (mkShrinkBudget 1)
  result <- shrinkCase one Results (checkedOracle (const True)) originalCase
  assert "budget one exhausts at the baseline" (reductionStatus result == BudgetExhausted)
  assert "budget one counts only the baseline" (trialCount result == 1)
  assert "budget one returns the original" (minimizedCase result == originalCase)
  three <- either error pure (mkShrinkBudget 3)
  result' <- shrinkCase three Results (checkedOracle (const True)) originalCase
  assert "budget three exhausts after two accepted trials" (reductionStatus result' == BudgetExhausted)
  assert "budget three counts exactly three trials" (trialCount result' == 3)
  let blocked candidate
        | oracleCase candidate == oracleCase originalCase = pure (Differs [Results])
        | otherwise = pure (Inconclusive (DomainRefusal "synthetic refusal"))
  result'' <- shrinkCase three Results blocked originalCase
  assert "budget takes precedence over unresolved proposals" (reductionStatus result'' == BudgetExhausted)
  assert "blocked budget run counts exactly three trials" (trialCount result'' == 3)
  assert "blocked budget run keeps the original" (minimizedCase result'' == originalCase)

idempotentFixedPoint :: IO ()
idempotentFixedPoint = do
  budget <- either error pure (mkShrinkBudget 256)
  let predicate = any isStagingAppend . operations
  first <- shrinkCase budget Results (checkedOracle predicate) originalCase
  second <- shrinkCase budget Results (checkedOracle predicate) (minimizedCase first)
  assert "fixed point is stable under another run" (reductionStatus second == FixedPoint)
  assert "fixed point returns the same case" (minimizedCase second == minimizedCase first)
  assert "fixed point baseline reproduces" (trialCount second >= 1)
