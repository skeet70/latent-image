{-# LANGUAGE GHC2021 #-}

module Main (main) where

import Control.Monad (forM_, unless)
import qualified Data.ByteString as B
import qualified Data.Map.Strict as M
import Data.Word (Word32)
import Emulation.NativeObservation (Register (..), RegionId (..), RunnerStatus (..))
import PropertyFields (PropertyId (..))
import qualified Replacement.PropertyService.Classify as C
import Replacement.PropertyService.Oracle
import Replacement.PropertyService.Placement (GuestAddress (..))
import qualified Replacement.PropertyService.HeaderBoundary as H
import Replacement.PropertyService.RecipeEditOracle

assert :: String -> Bool -> IO ()
assert label condition = unless condition (error label)

main :: IO ()
main = do
  let fx = fixture (B.replicate 0x220000 0x5a)
      codes = StoredCodes (StoredCode 0x80) (StoredCode 0xffffffff) (StoredCode 0x102) (StoredCode 0x10003) (StoredCode 0x40000004) (StoredCode 0x80000005) (StoredCode 0) (StoredCode 0x1affffff) (StoredCode 0x12345678)
      activeValues = [(Host267, 0x80), (Host268, 0xffffffff), (Host269, 0x102), (Host26a, 0x10003), (Host26b, 0x40000004), (Host26c, 0x80000005), (Host26d, 0), (Host26e, 0x1affffff), (Host26f, 0x12345678)]
      expectedPayloads = [(0x267, 0x104), (0x268, 0x110), (0x269, 0x111), (0x26a, 0x10e), (0x26b, 0x10f), (0x26c, 0xef), (0x26d, 0xed), (0x26e, 0x141), (0x26f, 0xfe)]
  forM_ expectedPayloads $ \(host, payload) ->
    assert "legacy host maps to stored payload property" (fmap payloadProperty (recipeHost (PropertyId host)) == Just (PropertyId payload))
  forM_ [0, 0x266, 0x270, 0x80000000, 0xffffffff] $ \identifier ->
    assert "non-host is not reclassified" (recipeHost (PropertyId identifier) == Nothing)
  forM_ recipeCases $ \testCase@(RecipeCase host operation) -> do
    inactive <- required (recipeExpectation fx SessionInactive testCase)
    active <- required (recipeExpectation fx (SessionActive codes) testCase)
    assert "inactive returns zero" (lookup R0 (expectedRegisters inactive) == Just 0)
    assert "inactive no effects" (unchanged fx inactive)
    assert "active no effects" (unchanged fx active)
    case operation of
      RecipeGet {} -> assert "active returns stored code without conversion" (lookup R0 (expectedRegisters active) == lookup host activeValues)
      RecipeSet {} -> assert "active setter returns zero" (lookup R0 (expectedRegisters active) == Just 0)
    assert "callee saved registers survive" (all (\r -> lookup r (expectedRegisters active) == Just (0xa5a50000 + fromIntegral (fromEnum r))) [R4, R5, R6, R7, R8, R9, R10, R11])
    assert "stack pointer survives" (lookup Sp (expectedRegisters active) == Just 0x9597f0)
    assert "returns normally" (expectedStatus active == Returned)
  assert "getter probes include nonboolean raw banks" (all (\bank -> any (hasBank bank) recipeCases) [2, 0x80000000, 0xffffffff])
  assert "setter probes include default sentinel neighbors" (all (\value -> any (hasValue value) recipeCases) [0x1afffffe, 0x1affffff, 0x1b000000])
  let neighbors = M.fromList [(identifier, C.Property identifier (GuestAddress 0x6243e4) (GuestAddress 0x6301fc) (C.Owned C.OwnedUnassigned)) | identifier <- adjacentProperties]
  forM_ adjacentProperties $ \identifier -> forM_ operations $ \operation -> do
    expected <- required (adjacentExpectation neighbors fx (OracleCase identifier operation))
    assert "canonical adjacent id returns zero" (lookup R0 (expectedRegisters expected) == Just 0)
    assert "canonical adjacent id has no effect" (unchanged fx expected)
  assert "adjacent control excludes recipe hosts" (adjacentExpectation neighbors fx (OracleCase (PropertyId 0x267) (Get Bank0 LiveRead (SlotArgument 0))) == Nothing)
  expectedNoop <- required (recipeExpectation fx SessionInactive (RecipeCase Host267 (RecipeSet (SetterValue 0x1affffff) (BankArgument 0) (SlotArgumentRaw 0xffffffff))))
  let observed writes = Run Returned (expectedRegisters expectedNoop) (expectedMemory expectedNoop) writes 1
  assert "scratch stack writes are allowed" (null (noEffectDifferences expectedNoop (observed (Just [WriteEvent 0x9597ec 4 0]))))
  forM_ [0x96cc64, 0x89ec00, 0x958e00, 0x9597f0] $ \address ->
    assert "same final bytes do not excuse an external write" (WritesDiffer `elem` noEffectDifferences expectedNoop (observed (Just [WriteEvent address 4 0])))
  assert "scratch write crossing caller frame is rejected" (WritesDiffer `elem` noEffectDifferences expectedNoop (observed (Just [WriteEvent 0x9597ef 2 0])))
  assert "truncated write evidence is not accepted" (SubjectTruncated `elem` noEffectDifferences expectedNoop (observed Nothing))
  putStrLn "recipe-edit independent expectation tests passed"
  where
    required = maybe (error "expectation unavailable") pure

unchanged :: Fixture -> Expectation -> Bool
unchanged fx expected = expectedMemory expected ==
  [ (RegionId "banks", fixtureBanks fx)
  , (RegionId "previous", fixturePrevious fx)
  , (RegionId "config", fixtureConfig fx)
  , (RegionId "calls", B.replicate H.callsBytes 0)
  , (RegionId "backing", H.initialBacking (fixtureBanks fx))
  , (RegionId "caller-frame", B.replicate 0x10 0)
  ]

hasBank :: Word32 -> RecipeCase -> Bool
hasBank wanted (RecipeCase _ (RecipeGet (BankArgument bank) _ _)) = wanted == bank
hasBank _ _ = False

hasValue :: Word32 -> RecipeCase -> Bool
hasValue wanted (RecipeCase _ (RecipeSet (SetterValue value) _ _)) = wanted == value
hasValue _ _ = False
