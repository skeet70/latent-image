{-# LANGUAGE OverloadedStrings #-}
module Main (main) where

import Control.Monad (foldM, unless)
import Data.Aeson (Value, object, (.=))
import qualified Data.ByteString as B
import qualified Data.Map.Strict as M
import Data.Word (Word32, Word8)
import Emulation.NativeObservation
import Emulation.OwnedCase
import GHC.Stats (getRTSStats, max_mem_in_use_bytes)
import Replacement.RtosScheduler.S3bTally
import System.Exit (exitFailure)

fixture :: CaseSpec
fixture = CaseSpec
  { caseId = "stock0", caseProvenance = "synthetic", caseExclusion = "", caseEntry = GuestAddress 0x44f2c, caseExit = GuestAddress 0x958f00
  , caseRegions =
      [ mkRegion (RegionId "code-0") (GuestAddress 0x44f2c) 4 ReadExecute (B.replicate 4 0)
      , mkRegion (RegionId "stack") (GuestAddress 0x959700) 0x110 ReadWrite (B.replicate 0x110 0)
      , mkRegion (RegionId "tasks-a") (GuestAddress 0x4005ff0c) 0x40 ReadWrite (B.replicate 0x40 0)
      , mkRegion (RegionId "globals-a") (GuestAddress 0x40054000) 0x40 ReadWrite (B.replicate 0x40 0)
      , mkRegion (RegionId "bank0") (GuestAddress 0xfffb30cc) 4 Device (B.replicate 4 0)
      ]
  , caseFunctions = [], caseInitial = [(Sp, 0x959800)], caseExpectedStatus = Returned, caseExpectedRegisters = [], caseExpectedMemory = []
  , caseIgnoredMemory = [], caseCp14Identity = Nothing, caseKernel = Nothing }

data Shape = Shape {shapeStatus :: RunnerStatus, shapeR0 :: Word32, shapeGlobals :: Word8, shapeDeadStack :: Word8, shapeDevice :: Word32, shapeInstructions :: Int}

base :: Shape
base = Shape Returned 0 0 0 1 10

-- | An observation carrying a non-writable region of @bank@ bytes filled with @fill@.
observation :: Int -> Word8 -> Shape -> NativeObservation
observation bank fill shape = NativeObservation
  { nativeStatus = shapeStatus shape
  , nativeRegisters = M.fromList ([(register, 0) | register <- registerLabels] ++ [(R0, shapeR0 shape), (Sp, 0x959800), (Cpsr, 0x1d3)])
  , nativeMemory =
      [ (RegionId "code-0", B.replicate 4 0)
      , (RegionId "bank", B.replicate bank fill)
      , (RegionId "stack", B.replicate 0x100 (shapeDeadStack shape) <> B.replicate 0x10 7)
      , (RegionId "tasks-a", B.replicate 0x40 3)
      , (RegionId "globals-a", B.replicate 0x40 (shapeGlobals shape))
      , (RegionId "bank0", B.replicate 4 0)
      ]
  , nativeProvenance = Complete (ProvenanceTotal 0) []
  , nativeInstructions = InstructionCount (fromIntegral (shapeInstructions shape))
  , nativeEngineError = EngineError 0
  , nativeKernel = Just (KernelObservation (Just [OrderedWrite (Write (GuestAddress 0xfffb30cc) (WriteBytes 4)) (WriteValue (fromIntegral (shapeDevice shape)))]) Nothing Nothing Nothing)
  }

-- | Builds and evaluates each case-major observation only when it is folded, as 'foldObservedBatch' hands them over.
feed :: Int -> (Int -> Role -> NativeObservation) -> IO Tally
feed cases produce = foldM step (emptyTally (const fixture)) [(i, role) | i <- [0 .. cases - 1], role <- [minBound .. maxBound]]
 where
  step tally (i, role) = let value = produce i role in evaluateObservation value >> (pure $! observe tally i role value)

-- | Stock and per-role shapes of a small service covering matching, differing, dead-stack and status cases.
smallService :: [(Shape, Shape, Shape, Shape)]
smallService =
  [ (base, base, base, base {shapeR0 = 0xffffffef})
  , (base {shapeR0 = 5}, base {shapeR0 = 5, shapeGlobals = 9}, base {shapeR0 = 5}, base {shapeR0 = 5})
  , (base, base {shapeDeadStack = 4}, base {shapeDevice = 2}, base {shapeStatus = StopAddress})
  , (base {shapeStatus = StopAddress, shapeR0 = 0xffffffed, shapeInstructions = 3}, base {shapeStatus = StopAddress, shapeR0 = 0xffffffed}, base {shapeStatus = InstructionBudget}, base {shapeStatus = StopAddress, shapeR0 = 0xffffffed})
  ]

-- | The verdict as the oracle computed it before folding: every observation kept, then compared.
wholeVerdict :: Bool -> String -> [(RunnerStatus, Word32)] -> M.Map Int (Word32, Word32, Int) -> Int -> (Int -> CaseSpec) -> M.Map String NativeObservation -> (Value, [Value], Int, [Value], [Value], [(Int, RunnerStatus, RunnerStatus)])
wholeVerdict b service cases evidenceMap reused stockSpecOf observed = (report, caseRows, failures, mutantRows, observationRows, ownedDifferences)
 where
  at role i=observed M.! (role++show i)
  completed=and [nativeStatus (at "stock" i) == status && M.lookup R0 (nativeRegisters (at "stock" i)) == Just value | (i,(status,value)) <- zip [0 :: Int ..] cases]
  stockSpec i _=stockSpecOf i
  matches role=length [() | i<-[0..length cases-1],comparable (stockSpec i (cases!!i)) (at "stock" i) (at role i)]
  corruptByte name offset o = o { nativeMemory = [(ident, if regionName ident == name then B.take offset bytes <> B.singleton (B.index bytes offset + 1) <> B.drop (offset + 1) bytes else bytes) | (ident, bytes) <- nativeMemory o] }
  controls = case cases of
    [] -> []
    row : _ ->
      let reference = at "stock" (0 :: Int)
          fixture' = stockSpec 0 row
          refuses change = not (comparable fixture' reference (change reference))
       in [refuses (corruptByte "tasks-a" 0x0c), refuses (corruptByte "globals-a" 0), refuses (corruptByte "stack" 0x100), comparable fixture' reference (corruptByte "stack" 0 reference)]
  negativeControl = length controls == 4 && and controls
  controlRows :: [(String, Bool)]
  controlRows = zip ["task-state", "globals-word", "stack-byte", "stack-base"] controls
  stockInstructions = [object ["case".=i,"instructions".=instructionCount (nativeInstructions (at "stock" i))] | i<-[0..length cases-1]]
  caseDifferences i = [role | role <- ["owned","equivalent"], not (comparable (stockSpec i (cases !! i)) (at "stock" i) (at role i))] :: [String]
  reportedCases = [i | i <- [0 .. length cases - 1], Just _ <- [M.lookup i evidenceMap]]
  caseRows = [ object
      [ "id" .= ("stock" ++ show i)
      , "case" .= i
      , "entry" .= handler
      , "extent" .= size
      , "stock_instructions_inside" .= inside
      , "stock_instructions" .= instructionCount (nativeInstructions (at "stock" i))
      , "status" .= show (nativeStatus (at "stock" i))
      , "differences" .= caseDifferences i
      ]
    | i <- reportedCases
    , Just (handler, size, inside) <- [M.lookup i evidenceMap]
    ]
  failures = length [() | i <- reportedCases, not (null (caseDifferences i))]
  mutantRows = [ object
      [ "name" .= ("mutant-" ++ service)
      , "expected" .= ("the service entry replaced by mvn r0,#16; bx lr is reported different" :: String)
      , "reported" .= (matches "mutant" < length cases)
      , "differences" .= ([] :: [String])
      , "diagnostic_differences" .= ([] :: [String])
      ] ]
  observationRows = [object ["name".=name, "reported".=held] | (name,held)<-controlRows]
  holds=completed && matches "owned"==length cases && matches "equivalent"==length cases && matches "mutant"<length cases && negativeControl
  report=object ["domain_b".=b,"service".=service,"cases".=length cases,"case_rows".=caseRows,"reused_observations".=reused,"stock_completed".=completed,"owned_matches".=matches "owned","equivalent_matches".=matches "equivalent","mutant_matches".=matches "mutant","negative_control".=negativeControl,"controls".=mutantRows,"observation_controls".=observationRows,"negative_controls".=[object ["control".=name,"holds".=held] | (name,held)<-controlRows],"stock_instructions".=stockInstructions,"passed".=holds]
  ownedDifferences = [(i,nativeStatus (at "stock" i),nativeStatus (at "owned" i)) | i<-[0..length cases-1],not (comparable (stockSpec i (cases!!i)) (at "stock" i) (at "owned" i))]

main :: IO ()
main = do
  let bank = 64 * 1024
      smallObservation i role = let (stock, owned, equivalent, mutant) = smallService !! i in observation bank (fromIntegral i) (case role of Stock -> stock; Owned -> owned; Equivalent -> equivalent; Mutant -> mutant)
      expectations = [(Returned, 0), (Returned, 5), (Returned, 1), (StopAddress, 0xffffffed)]
      evidence = M.fromList [(i, (0x44f2c, 452, i + 1)) | i <- [0, 1, 2]]
  small <- feed (length smallService) smallObservation
  folded <- either (\problem -> putStrLn problem >> exitFailure) pure (serviceVerdict True "Wake" expectations evidence 2 small)
  let whole = M.fromList [(roleLabel role ++ show i, smallObservation i role) | i <- [0 .. length smallService - 1], role <- [minBound .. maxBound]]
      (report, caseRows, failures, mutantRows, observationRows, ownedDifferences) = wholeVerdict True "Wake" expectations evidence 2 (const fixture) whole
      cases = 2048
      manyObservation i role = observation bank (fromIntegral (i * 4 + fromEnum role)) (if role == Mutant then base {shapeR0 = 1} else base)
  large <- feed cases manyObservation
  manyVerdict <- either (\problem -> putStrLn problem >> exitFailure) pure (serviceVerdict False "Activate" (replicate cases (Returned, 0)) M.empty 0 large)
  peak <- max_mem_in_use_bytes <$> getRTSStats
  let incomplete = serviceVerdict False "Activate" (replicate 5 (Returned, 0)) M.empty 0 small
      checks =
        [ ("small service verdict equals the whole-observation verdict", verdictReport folded == report)
        , ("small service case rows equal", verdictCaseRows folded == caseRows)
        , ("small service failures equal", verdictFailures folded == failures)
        , ("small service controls equal", verdictControls folded == mutantRows && verdictObservationControls folded == observationRows)
        , ("small service owned differences equal", verdictOwnedDifferences folded == ownedDifferences)
        , ("small service exercises a failing verdict", not (verdictHolds folded) && failures == 2)
        , ("an unobserved case is refused", either (const True) (const False) incomplete)
        , ("many-case service holds", verdictHolds manyVerdict && verdictOwnedMatches manyVerdict == cases)
        , ("many-case peak stays under 64 MiB with 512 MiB observed", peak < 64 * 1024 * 1024)
        ]
      failed = [name | (name, False) <- checks]
  unless (null failed) $ do
    mapM_ putStrLn failed
    putStrLn ("peak bytes " ++ show peak)
    exitFailure
