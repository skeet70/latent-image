{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}
module Main (main) where

import Control.Monad (unless)
import Data.Bits (xor)
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as BC
import Data.List (nub)
import qualified Data.Map.Strict as M
import Data.Maybe (fromMaybe)
import Data.Word (Word32)
import Emulation.NativeObservation
import Replacement.RecordStore.Cases
import Replacement.RecordStore.Execution
import Replacement.RecordStore.Generate (oracleCase, validCase)
import Replacement.RecordStore.Harness (Function (..))
import Replacement.RecordStore.Hardware (hardware)
import Replacement.RecordStore.Minimize
import Replacement.RecordStore.Model hiding (Medium)

assert :: String -> Bool -> IO ()
assert message condition = unless condition (error message)

baseCase :: OracleCase
baseCase =
  OracleCase
    (CaseName "counterexample-test")
    Kind0
    (mediumOf (geometry Kind0) [(10, False)] [])
    [Open, Register Normal, Latest]
    MarkerCallback
    NoFault

modelObservation :: OracleCase -> NativeObservation
modelObservation c =
  NativeObservation
    { nativeStatus = if stopped (persistence state) then MemoryGuard else Returned
    , nativeRegisters =
        M.fromList
          ( [(register, 0) | register <- registerLabels]
              ++ [(Sp, stackTop)]
              ++ zip [R4, R5, R6, R7, R8, R9, R10, R11] [0x44440004 .. 0x4444000b]
          )
    , nativeMemory =
        [ (RegionId "results", expectedResults expected)
        , (RegionId "caller", expectedCaller expected)
        , (RegionId "medium", expectedMedium c expected)
        , (RegionId "source", sourceBytes (geometry (kind c)))
        , (RegionId "requests", requestBytes <> B.replicate (requestSize - B.length requestBytes) 0)
        , (RegionId "callbacks", callbackBytes <> B.replicate (callbackSize - B.length callbackBytes) 0)
        ]
    , nativeProvenance = Complete (ProvenanceTotal 0) []
    , nativeInstructions = InstructionCount 1
    , nativeEngineError = EngineError 0
    , nativeKernel = Nothing
    }
  where
    expected = model hardware c
    state = expectedStore expected
    requestBytes = B.concat (map encodeRequest (requests (persistence state)))
    callbackBytes = B.concat (map encodeCallback (callbackEvents state))

withRegion :: RegionId -> B.ByteString -> NativeObservation -> NativeObservation
withRegion ident bytes observation =
  observation
    { nativeMemory =
        [(name, if name == ident then bytes else current) | (name, current) <- nativeMemory observation]
    }

mutateRegion :: RegionId -> NativeObservation -> NativeObservation
mutateRegion ident observation =
  let bytes = fromMaybe (error "missing observation region") (lookup ident (nativeMemory observation))
   in withRegion ident (B.cons (B.index bytes 0 `xor` 0xff) (B.tail bytes)) observation

withRegister :: Register -> Word32 -> NativeObservation -> NativeObservation
withRegister register value observation =
  observation {nativeRegisters = M.insert register value (nativeRegisters observation)}

withStatus :: RunnerStatus -> NativeObservation -> NativeObservation
withStatus status observation = observation {nativeStatus = status}

sampleDifference :: RegionDifference
sampleDifference = RegionDifference 0 (Just 1) (Just 2) 1 1

main :: IO ()
main = do
  let valid = either (error . show) id (validCase baseCase)
      observation = modelObservation baseCase
  assert "model-matching stock and candidate match" (evaluateCase baseCase observation observation == CaseMatches)
  assert "replay uses the same comparator for a matching case" (replayVerdict valid observation observation == CaseMatches)
  assert
    "stock model mismatch is a prerequisite, not candidate evidence"
    ( case evaluateCase baseCase (mutateRegion (RegionId "results") observation) observation of
        CaseStockModelMismatch problems -> not (null problems)
        _ -> False
    )
  assert
    "non-returning stock is unsupported before the model comparison"
    ( case evaluateCase baseCase (withStatus MemoryGuard observation) observation of
        CaseUnsupported _ -> True
        _ -> False
    )
  assert
    "stock instruction budget is unsupported, not model semantics"
    ( case evaluateCase baseCase (withStatus InstructionBudget observation) observation of
        CaseUnsupported _ -> True
        _ -> False
    )
  assert
    "candidate result difference reports the results boundary"
    ( case evaluateCase baseCase observation (mutateRegion (RegionId "results") observation) of
        CaseDiffers differences -> map roleBoundary differences == [Results]
        _ -> False
    )
  assert
    "candidate register difference reports the preserved register"
    ( case evaluateCase baseCase observation (withRegister R4 0xdeadbeef observation) of
        CaseDiffers differences -> map roleBoundary differences == [Preserved R4]
        _ -> False
    )
  let twoBoundaries = mutateRegion (RegionId "medium") (mutateRegion (RegionId "results") observation)
  assert
    "verdict exposes every differing boundary in comparison order"
    ( case evaluateCase baseCase observation twoBoundaries of
        CaseDiffers differences -> nub (map roleBoundary differences) == [Results, Medium]
        _ -> False
    )
  assert
    "replay exposes every differing boundary"
    ( case replayVerdict valid observation twoBoundaries of
        CaseDiffers differences -> nub (map roleBoundary differences) == [Results, Medium]
        _ -> False
    )
  assert
    "candidate memory guard against returning stock is an outcome failure"
    ( case evaluateCase baseCase observation (withStatus MemoryGuard observation) of
        CaseDiffers differences -> map roleBoundary differences == [Outcome]
        _ -> False
    )
  assert
    "candidate fetch guard against returning stock is an outcome failure"
    ( case evaluateCase baseCase observation (withStatus FetchGuard observation) of
        CaseDiffers differences -> map roleBoundary differences == [Outcome]
        _ -> False
    )
  assert
    "candidate instruction budget is unsupported, not a difference"
    ( case evaluateCase baseCase observation (withStatus InstructionBudget observation) of
        CaseUnsupported _ -> True
        _ -> False
    )
  assert
    "role comparison reports status before regions"
    ( case compareRoles observation (withStatus InstructionBudget observation) of
        RoleStatus Returned InstructionBudget : _ -> True
        _ -> False
    )
  assert
    "role comparison reports result and medium differences in order"
    ( case compareRoles observation twoBoundaries of
        [RoleRegion results _, RoleRegion medium _] -> results == RegionId "results" && medium == RegionId "medium"
        _ -> False
    )
  assert
    "differing verdict maps to typed boundaries"
    ( case trialFromVerdict (CaseDiffers [RoleRegion (RegionId "results") sampleDifference, RoleRegion (RegionId "medium") sampleDifference]) of
        Differs boundaries -> boundaries == [Results, Medium]
        _ -> False
    )
  assert
    "unsupported verdict maps to an inconclusive native status"
    ( case trialFromVerdict (CaseUnsupported "candidate status instruction_budget") of
        Inconclusive (UnsupportedNativeStatus _) -> True
        _ -> False
    )
  assert
    "stock model verdict maps to an inconclusive stock/model disagreement"
    ( case trialFromVerdict (CaseStockModelMismatch [ModelStatus Returned MemoryGuard 0]) of
        Inconclusive (StockModelDisagreement _) -> True
        _ -> False
    )
  assert
    "boundary labels name each comparison predicate"
    ( map boundaryLabel [Outcome, Results, Caller, Medium, Source, Requests, Callbacks, Preserved Sp]
        == ["outcome", "results", "caller", "medium", "source", "requests", "callbacks", "preserved:sp"]
    )
  assert
    "first difference reports the first differing byte"
    ( case firstDifference (B.pack [1, 2, 3]) (B.pack [1, 2, 4]) of
        Just difference ->
          differenceOffset difference == 2
            && differenceExpected difference == Just 3
            && differenceActual difference == Just 4
            && differenceExpectedSize difference == 3
            && differenceActualSize difference == 3
        Nothing -> False
    )
  assert
    "first difference reports a missing trailing byte"
    ( case firstDifference (B.pack [1]) (B.pack [1, 2]) of
        Just difference ->
          differenceOffset difference == 1
            && differenceExpected difference == Nothing
            && differenceActual difference == Just 2
        Nothing -> False
    )
  assert "equal bytes have no first difference" (firstDifference (B.pack [1, 2]) (B.pack [1, 2]) == Nothing)
  let rows =
        [ "range\tunit\tsection\t0x0061c400\t" ++ show (0x700000 :: Word32) ++ "\t32\t" ++ replicate 64 'a'
        , "range\tunit\tsection\t0x00000000\t" ++ show (0x1000 :: Word32) ++ "\t8\t" ++ replicate 64 'b'
        , "range\tunit\tsection\t0x0089aa90\t" ++ show (0x89aa90 :: Word32) ++ "\t16\t" ++ replicate 64 'c'
        ]
  assert
    "row input reads the decimal start and size after the hex column"
    ( readFunctionsBytes (BC.pack (unlines rows))
        == [Function 0x89aa90 0x128, Function 0x89abb8 0x20, Function 0x700000 32]
    )
  assert
    "trial blocks name stable evidence classes"
    ( map trialBlockClass [StockModelDisagreement "", UnsupportedNativeStatus "", DomainRefusal "", InfrastructureFailure ""]
        == ["stock-model-disagreement", "unsupported-native-status", "domain-refusal", "infrastructure-failure"]
    )
  putStrLn "record-store counterexample tests passed"
