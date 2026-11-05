-- | The grid, the data layout, the comparison and the controls against the owned bodies: the owned
-- body ends as each row expects and keeps its barrier order, each mutant differs from it on at
-- least one case, and a repeat and the equivalents match it on every case. Stock code needs the
-- loaders, so the stock role runs only in the scheduler-oracle binary.
module Main (main) where

import Control.Monad (forM, unless)
import qualified Data.ByteString as B
import Data.List (nub)
import qualified Data.Map.Strict as M
import Data.Maybe (isJust)
import Data.Word (Word32)
import Emulation.NativeObservation
  ( Barrier (..)
  , EngineError (..)
  , GuestAddress (..)
  , InstructionCount (..)
  , KernelObservation (..)
  , NativeObservation (..)
  , Ordered (..)
  , Provenance (..)
  , ProvenanceTotal (..)
  , Register (..)
  , RegionId (..)
  , RunnerStatus (..)
  , Write (..)
  , WriteBytes (..)
  , WriteValue (..)
  , registerLabels
  )
import Emulation.OwnedCase (CaseSpec (..), KernelCase (..), parseSymbols, word32le)
import Replacement.RtosOwnFirst.Assembly
import Replacement.RtosOwnFirst.Regions (Domain (..), domainLabel, domains)
import Replacement.RtosScheduler.Controls
import Replacement.RtosScheduler.Layout
import Replacement.RtosScheduler.Oracle
import Replacement.RtosScheduler.Regions
import Replacement.RtosScheduler.Suite
import Replacement.RtosScheduler.Verdict
import System.Directory (removeDirectoryRecursive)
import System.Environment (getArgs, getEnv)
import System.Exit (exitFailure)
import System.FilePath ((</>))
import System.IO (BufferMode (..), hSetBuffering, stdout)

check :: String -> Bool -> IO Bool
check label result = do
  putStrLn ((if result then "ok   " else "FAIL ") ++ label)
  pure result

-- | Writes @1 << (r1 & 31)@ to word @r1 >> 5@ past core r0's bank at 0xfffb30c0 or 0xfffb40c0, as stock
-- raise-IRQ does for a nonnegative IRQ; stands in for the stock code the tests have no loader for.
raiseIrqStandIn :: Word32 -> Either String (Word32, B.ByteString)
raiseIrqStandIn at = do
  code <-
    assembleAt
      at
      ( [movShift r2 r1 Lsr 5, andI r1 r1 31, movI r3 1, movShiftRegister r3 r3 r1]
          ++ loadAddress r12 0xfffb30c0
          ++ [cmpI r0 0, on CondNe (addI r12 r12 0x1000), strIndexed r3 r12 r2 2, bxLr]
      )
  pure (at, B.concat (map word32le code))

kernelObservation :: [Ordered] -> NativeObservation
kernelObservation order =
  NativeObservation
    { nativeStatus = Returned
    , nativeRegisters = M.fromList [(register, 0) | register <- registerLabels]
    , nativeMemory = [(RegionId "block-a", B.replicate 8 0), (RegionId "stack", B.replicate 8 0)]
    , nativeProvenance = Complete (ProvenanceTotal 0) []
    , nativeInstructions = InstructionCount 1
    , nativeEngineError = EngineError 0
    , nativeKernel = Just (KernelObservation (Just order) Nothing Nothing Nothing)
    }

write :: Word32 -> Word32 -> Ordered
write address value = OrderedWrite (Write (GuestAddress (fromIntegral address)) (WriteBytes 4)) (WriteValue (fromIntegral value))

withRegister :: Register -> Word32 -> NativeObservation -> NativeObservation
withRegister register value observation = observation {nativeRegisters = M.insert register value (nativeRegisters observation)}

rowWhere :: Layout -> Helper -> (Row -> Bool) -> Maybe (CaseKey, Row)
rowWhere layout helper predicate = case filter (predicate . snd) (zip (map CaseKey [0 ..]) (caseGrid layout helper)) of
  found : _ -> Just found
  [] -> Nothing

pureChecks :: Env -> SchedulerRegions -> [IO Bool]
pureChecks env regions =
  let layout = envLayout env
      grid = caseGrid layout
      rowsWith dimension helper = nub [drop 1 rest | row <- grid helper, item <- rowTags row, let (d, rest) = break (== '=') item, d == dimension]
      slotsOf row = concat (M.elems (rowLists row))
      validRow row = nub (slotsOf row) == slotsOf row && all (\slot -> slot >= 1 && slot <= 12) (slotsOf row) && maybe True (`elem` slotsOf row) (rowSelected row)
      insertRow = maybe (error "no prepare row") snd (rowWhere layout Prepare (const True))
      prepareSpec = caseSpec env DomainA (Body 0x43f70 [] [(0x43f70, 4)] [] []) "probe" Prepare insertRow
      task = taskAddress layout DomainA 1
      t = layoutTask layout
      reference = kernelObservation [write (task + taskSavedSp t) 1, write (task + taskResumePc t) 2, OrderedBarrier Dmb, write (task + taskState t) 1]
      unfenced = kernelObservation [write (task + taskSavedSp t) 1, write (task + taskResumePc t) 2, write (task + taskState t) 1]
      compareTo = compareCase env DomainA Prepare insertRow prepareSpec False reference
      notifyRow = maybe (error "no notify row for id 2") snd (rowWhere layout Notify ((== 2) . rowArgument))
      notifyOrder order = orderHolds env DomainA Notify notifyRow (kernelObservation order)
      prepareOrder order = orderHolds env DomainA Prepare insertRow (kernelObservation order)
   in [ check "every helper has a grid" (all (not . null . grid) helpers)
      , check "the grid is deterministic" (map grid helpers == map grid helpers)
      , check "rows link each slot at most once, inside the mapped window, and select a listed task" (all validRow (concatMap grid helpers))
      , check "every row's data lies in the mapped regions in both loaders" (and [null (pokesOutside env domain row) | helper <- helpers, domain <- domains, row <- grid helper])
      , check "dispatch-enabled values cover 0, 1, 2 and 0x80000000" (and [rowsWith "dispatch" helper == ["0x0", "0x1", "0x2", "0x80000000"] | helper <- [Insert, Remove, Prepare, Requeue, Rotate]])
      , check "notify ids cover 0, 1, 2, 3, 0xffffffff and 0x80000000" (map rowArgument (grid Notify) == notifyIds)
      , check "the lock pair takes free and held words of both locks in both records" (and [length (grid helper) == 16 && length (filter (rowSpins Acquire) (grid helper)) == 12 | helper <- [Acquire, Release]])
      , check "insert, prepare and reset take every T+0x0c state" (and [length (rowsWith "state" helper) == length (stateBytes layout) | helper <- [Insert, Prepare, Reset]])
      , check "remove and requeue take the task at head, middle, tail and alone, and every selected relation" (and [rowsWith "pos" helper == ["Only", "AtHead", "AtMiddle", "AtTail"] && length (rowsWith "sel" helper) >= 4 | helper <- [Remove, Requeue]])
      , check "list shapes cover empty, one and many" (and [rowsWith "shape" helper == ["Empty", "One", "Many"] | helper <- [Insert, Prepare, Rotate]])
      , check "each sample covers every tag in at most 60 rows" (and [let keys = sampleKeys layout helper in not (null keys) && length keys <= 60 | helper <- helpers])
      , check "regions.json names every helper in both loaders" (and [all (\helper -> either (const False) (const True) (regionOf (schedulerDomain regions domain) helper)) helpers | domain <- domains])
      , check "an identical run has no difference" (null (compareTo reference))
      , check "lr, r1-r3, r12 and the CPSR flags are not compared" (null (compareTo (foldr (\(r, v) -> withRegister r v) reference [(Lr, 7), (R1, 7), (R2, 7), (R3, 7), (R12, 7), (Cpsr, 0xf0000000)])))
      , check "r0, r4-r11, sp and the CPSR control bits are compared" (and [not (null (compareTo (withRegister r v reference))) | (r, v) <- [(R0, 7), (R4, 7), (R11, 7), (Sp, 7), (Cpsr, 0x80)]])
      , check "stack scratch writes and bytes are not compared" (null (compareTo (stripScratch (kernelObservation [write stackBase 9, write (task + taskSavedSp t) 1, write (task + taskResumePc t) 2, OrderedBarrier Dmb, write (task + taskState t) 1]) {nativeMemory = [(RegionId "block-a", B.replicate 8 0), (RegionId "stack", B.replicate 8 1)]})))
      , check "a data write's value is compared" (not (null (compareTo (kernelObservation [write (task + taskSavedSp t) 3, write (task + taskResumePc t) 2, OrderedBarrier Dmb, write (task + taskState t) 1]))))
      , check "prepare's order needs the dmb after both stores and before insert's" (prepareOrder [write (task + taskSavedSp t) 1, write (task + taskResumePc t) 2, OrderedBarrier Dmb, write (task + taskState t) 1] && not (prepareOrder [write (task + taskSavedSp t) 1, write (task + taskResumePc t) 2, write (task + taskState t) 1]) && not (prepareOrder [write (task + taskSavedSp t) 1, OrderedBarrier Dmb, write (task + taskResumePc t) 2, write (task + taskState t) 1]))
      , check "notify's order needs a dsb before the bank write" (notifyOrder [OrderedBarrier Dsb, write 0xfffb40cc 0x1000000] && not (notifyOrder [write 0xfffb40cc 0x1000000, OrderedBarrier Dsb]) && notifyOrder [])
      , check "the order assertion runs only when asked" (BarrierOrder `notElem` compareCase env DomainA Prepare insertRow prepareSpec False reference unfenced && BarrierOrder `elem` compareCase env DomainA Prepare insertRow prepareSpec True reference unfenced)
      , check "a spinning acquire leaves r11, sp and pc uncompared too" (case rowWhere layout Acquire (rowSpins Acquire) of Just (_, row) -> all (`elem` uncomparedRegisters Acquire row) [R11, Sp, Pc]; Nothing -> False)
      , check "the stand-in raise-IRQ assembles" (isJust (either (const Nothing) Just (raiseIrqStandIn 0x433e4)))
      , check "one mutant per helper and two equivalents" (map controlHelper mutants == helpers && length equivalents == 2 && all ((== MustFail) . controlExpectation) mutants && all ((== MustPass) . controlExpectation) equivalents)
      , check "kernel cases name their uncompared registers" (fmap kernelUncompared (caseKernel prepareSpec) == Just [R1, R2, R3, R12, Lr, Cpsr])
      ]

data Scope = FullGrid | Sample deriving (Eq)

-- | The owned body as reference, against a repeat of itself and each control.
nativeChecks :: Scope -> FilePath -> FilePath -> Env -> Domain -> SchedulerDomain -> Word32 -> OwnedImage -> Helper -> IO [String]
nativeChecks scope runner temporary env domain table fill image helper = do
  let name = domainLabel domain ++ " " ++ helperName helper
      layout = envLayout env
      keys = case scope of
        FullGrid -> map CaseKey [0 .. length (caseGrid layout helper) - 1]
        Sample -> sampleKeys layout helper
      controls = [control | control <- mutants ++ equivalents, controlHelper control == helper]
  case raiseIrqStandIn (domainRaiseIrq table) >>= \raise -> ownedBody table image raise helper of
    Left problem -> pure [name ++ ": " ++ problem]
    Right owned -> case mapM (\control -> (,) control <$> controlBody fill layout control owned) controls of
      Left problem -> pure [name ++ ": " ++ problem]
      Right built -> do
        let directory = temporary </> domainLabel domain ++ "-" ++ helperName helper
        run <- runHelper env runner directory domain helper keys (Role "owned" owned True) (Role "owned-repeat" owned True : [Role (controlName control) body True | (control, body) <- built])
        removeDirectoryRecursive directory
        let outcomes = M.fromList (runSubjects run)
            results = ControlResult "owned-repeat" MustPass "the owned body against itself" (outcomes M.! "owned-repeat") : [ControlResult (controlName control) (controlExpectation control) (controlNote control) (outcomes M.! controlName control) | (control, _) <- built]
        mapM_ (\result -> let o = resultOutcome result in putStrLn ("     " ++ name ++ " " ++ resultControl result ++ ": " ++ show (outcomeCases o - outcomeMatches o) ++ " of " ++ show (outcomeCases o) ++ " differ, " ++ show (outcomeDifferingCompleted o) ++ " of them completed")) results
        pure $
          [name ++ ": an owned case did not end as its row expects" | not (runReferenceEnds run)]
            ++ [name ++ ": an owned case breaks the barrier order" | not (runReferenceOrder run)]
            ++ [name ++ " " ++ resultControl result ++ " does not hold" | result <- results, not (holds result)]
            ++ bankChecks env domain helper run

-- | Owned notify rings exactly the bank of the remote core in the sample's id 2 (A) or 1 (B) row.
bankChecks :: Env -> Domain -> Helper -> HelperRun -> [String]
bankChecks env domain helper run
  | helper /= Notify = []
  | otherwise =
      [ domainLabel domain ++ " notify id " ++ show ident ++ ": bank writes " ++ show found
      | (ident, expected) <- case domain of
          DomainA -> [(0, []), (2, [(0xfffb40cc, 0x01000000)])]
          DomainB -> [(2, []), (1, [(0xfffb30cc, 0x01000000)])]
      , Just (key, _) <- [rowWhere (envLayout env) Notify ((== ident) . rowArgument)]
      , Just observation <- [M.lookup key (runReference run)]
      , let found = [(fromIntegral address, fromIntegral value) | Just order <- [nativeKernel observation >>= observedOrder], OrderedWrite (Write (GuestAddress address) _) (WriteValue value) <- order, fromIntegral address `elem` bankWords] :: [(Word32, Word32)]
      , found /= expected
      ]

main :: IO ()
main = do
  hSetBuffering stdout LineBuffering
  arguments <- getArgs
  temporary <- getEnv "TEST_TMPDIR"
  case arguments of
    [scopeName, runner, regionsPath, layoutPath, binA, symbolsA, binB, symbolsB] -> do
      scope <- case scopeName of
        "full" -> pure FullGrid
        "sample" -> pure Sample
        _ -> fail ("unknown scope " ++ scopeName)
      regions <- either fail pure . parseSchedulerRegions =<< B.readFile regionsPath
      layout <- either fail pure . parseLayout =<< B.readFile layoutPath
      env <- either fail pure (mkEnv layout regions)
      images <- forM (zip domains [(binA, symbolsA), (binB, symbolsB)]) $ \(domain, (bin, symbolsPath)) -> do
        let table = schedulerDomain regions domain
        symbols <- parseSymbols (map regionSymbol (domainRegions table) ++ map shimBodySymbol (domainShims table)) symbolsPath
        bytes <- B.readFile bin
        either (fail . ((domainLabel domain ++ ": ") ++)) pure (ownedImage table bytes symbols)
      pureResults <- sequence (pureChecks env regions)
      failures <- fmap concat . forM (zip domains images) $ \(domain, image) ->
        concat <$> mapM (nativeChecks scope runner temporary env domain (schedulerDomain regions domain) (regionsFill regions) image) helpers
      nativeResult <- check "owned bodies end as expected and keep their barrier order, and the controls hold, in both loaders" (null failures)
      mapM_ (putStrLn . ("     " ++)) failures
      unless (and (nativeResult : pureResults)) exitFailure
    _ -> fail "usage: scheduler-oracle-tests full|sample RUNNER REGIONS LAYOUT BIN_A SYMBOLS_A BIN_B SYMBOLS_B"
