{-# LANGUAGE OverloadedStrings #-}

-- | Runs every S1 helper's case grid against the stock code cut from the pinned loaders and against
-- the owned body, compares them case by case and runs the controls. Exits 1 when a stock case ends
-- otherwise than returning or spinning on its held word, or a control misbehaves, and 2 when an
-- owned body differs from stock.
module Main (main) where

import Control.Exception (evaluate)
import Control.Monad (forM, forM_, when)
import Data.Aeson (Value, object, (.=))
import qualified Data.ByteString as B
import qualified Data.Map.Strict as M
import Data.String (fromString)
import Emulation.OwnedCase (parseSymbols, required)
import Emulation.Sha256 (sha256, sha256Hex)
import FujiAF.Host (ensure, failH, readRegular)
import FujiAF.JSON (encodeCanonical)
import Numeric (showHex)
import Replacement.RtosOwnFirst.Artifact (loaderSha256)
import Replacement.RtosOwnFirst.Regions (domainLabel, domains)
import qualified Replacement.RtosPlatform.Aggregate as Agg
import qualified Replacement.RtosPlatform.Union as Union
import Replacement.RtosScheduler.Controls
import Replacement.RtosScheduler.Layout (parseLayout)
import Replacement.RtosScheduler.Oracle
import Replacement.RtosScheduler.Regions
import Replacement.RtosScheduler.Suite
import Replacement.RtosScheduler.Verdict
import System.Directory (canonicalizePath, createDirectory, createDirectoryIfMissing, doesPathExist, makeAbsolute, removeDirectoryRecursive, setCurrentDirectory)
import System.Environment (getArgs, lookupEnv)
import System.Exit (ExitCode (..), exitWith)
import System.FilePath (takeDirectory, (</>))

digest :: B.ByteString -> String
digest = sha256Hex . sha256

orFail :: String -> Either String a -> IO a
orFail context = either (\problem -> failH (context ++ ": " ++ problem)) pure

hexText :: (Integral a, Show a) => a -> String
hexText value = "0x" ++ showHex value ""

statusesValue :: M.Map String Int -> Value
statusesValue counts = object [fromString status .= count | (status, count) <- M.toList counts]

firstValue :: Maybe (CaseKey, [Difference]) -> Maybe Value
firstValue = fmap (\(CaseKey index, differences) -> object ["row" .= index, "differences" .= map differenceText differences])

controlValue :: HelperReport -> ControlResult -> Value
controlValue report result =
  object
    [ "name" .= resultControl result
    , "expectation" .= (case resultExpectation result of MustPass -> "must-pass"; MustFail -> "must-fail" :: String)
    , "note" .= resultNote result
    , "domain" .= domainLabel (reportDomain report)
    , "helper" .= helperName (reportHelper report)
    , "cases" .= outcomeCases outcome
    , "matches" .= outcomeMatches outcome
    , "differing_completed" .= outcomeDifferingCompleted outcome
    , "differing_other" .= outcomeDifferingOther outcome
    , "statuses" .= statusesValue (outcomeStatuses outcome)
    , "first_difference" .= firstValue (outcomeFirst outcome)
    , "holds" .= holds result
    ]
  where
    outcome = resultOutcome result

helperValue :: HelperReport -> Value
helperValue report =
  object
    [ "helper" .= helperName (reportHelper report)
    , "entry" .= hexText (reportEntry report)
    , "stock_statuses" .= statusesValue (reportStockStatuses report)
    , "stock_ends_as_expected" .= reportStockHolds report
    , "owned_statuses" .= statusesValue (outcomeStatuses owned)
    , "cases" .= outcomeCases owned
    , "matches" .= outcomeMatches owned
    , "first_mismatch" .= firstValue (outcomeFirst owned)
    ]
  where
    owned = reportOwned report

chunks :: [a] -> [[a]]
chunks [] = []
chunks values = let (batch, rest) = splitAt 64 values in batch : chunks rest

mergeOutcome :: Outcome -> Outcome -> Outcome
mergeOutcome left right = Outcome
  { outcomeCases = outcomeCases left + outcomeCases right
  , outcomeMatches = outcomeMatches left + outcomeMatches right
  , outcomeFirst = case outcomeFirst left of Nothing -> outcomeFirst right; first -> first
  , outcomeDifferingCompleted = outcomeDifferingCompleted left + outcomeDifferingCompleted right
  , outcomeDifferingOther = outcomeDifferingOther left + outcomeDifferingOther right
  , outcomeStatuses = M.unionWith (+) (outcomeStatuses left) (outcomeStatuses right)
  }

mergeReports :: HelperReport -> HelperReport -> HelperReport
mergeReports left right = left
  { reportStockStatuses = M.unionWith (+) (reportStockStatuses left) (reportStockStatuses right)
  , reportStockHolds = reportStockHolds left && reportStockHolds right
  , reportOwned = mergeOutcome (reportOwned left) (reportOwned right)
  , reportControls = zipWith (\a b -> a {resultOutcome = mergeOutcome (resultOutcome a) (resultOutcome b)}) (reportControls left) (reportControls right)
  }

main :: IO ()
main = do
  arguments <- getArgs
  let input name = required arguments name >>= canonicalizePath
      output name = required arguments name >>= makeAbsolute
  runner <- input "--runner"
  regionsPath <- input "--regions"
  layoutPath <- input "--layout"
  let aggregateFlags = ["--aggregate-" ++ domain ++ "-" ++ kind | domain <- ["a","b"], kind <- ["bin","symbols","placements"]] ++ ["--aggregate-carriers"]
      aggregateMode = any (`elem` arguments) aggregateFlags
  when aggregateMode $ ensure (all (`elem` arguments) aggregateFlags) "aggregate inputs must be supplied together"
  ownedPaths <- mapM input (if aggregateMode then aggregateFlags else ["--owned-a-bin", "--owned-a-symbols", "--owned-b-bin", "--owned-b-symbols"])
  lookupEnv "BUILD_WORKSPACE_DIRECTORY" >>= mapM_ setCurrentDirectory
  loaderPaths <- mapM input ["--loader-a", "--loader-b"]
  work <- output "--work"
  summaryPath <- output "--summary"
  forM_ [work, summaryPath] $ \path -> do
    exists <- doesPathExist path
    when exists (failH (path ++ " already exists"))
  runnerBytes <- readRegular 0x10000000 runner
  regionsBytes <- readRegular 0x100000 regionsPath
  layoutBytes <- readRegular 0x100000 layoutPath
  ownedBytes <- mapM (readRegular 0x100000) ownedPaths
  loaders <- mapM (readRegular 0x20000) loaderPaths
  forM_ (zip domains loaders) $ \(domain, loader) ->
    ensure (digest loader == loaderSha256 domain) ("--loader-" ++ domainLabel domain ++ " is not the pinned X-M1 1.50 loader")
  regions <- orFail "regions.json" (parseSchedulerRegions regionsBytes)
  layout <- orFail "layout" (parseLayout layoutBytes)
  env <- orFail "layout" (mkEnv layout regions)
  images <- if aggregateMode then case ownedBytes of
    [binA,symbolsA,placementsA,binB,symbolsB,placementsB,carrierBytes] -> do
      union <- orFail "aggregate carriers" (either (Left . show) Right (Union.parseUnion carrierBytes))
      forM (zip domains [(binA,symbolsA,placementsA),(binB,symbolsB,placementsB)]) $ \(domain,(bin,symbolBytes,placementBytes)) -> do
        symbols <- orFail "aggregate symbols" (Agg.parseAggregateSymbols symbolBytes)
        placements <- orFail "aggregate placements" (Agg.parsePlacements placementBytes)
        image <- orFail "aggregate image" (Agg.aggregateImage symbols bin)
        let carrierMap = Union.domainMap union domain
        forM_ (Union.mapEntries carrierMap) $ \entry -> do
          _ <- orFail "aggregate entry binding" (Union.resolveEntryTarget image symbols placements entry)
          pure ()
        pure (Right (image,symbols,placements,map Union.carrierExtent (Union.mapCarriers carrierMap)))
    _ -> failH "unreachable aggregate inputs"
    else case ownedPaths of
      [binA, symbolsA, binB, symbolsB] -> forM (zip domains [(binA, symbolsA), (binB, symbolsB)]) $ \(domain, (bin, symbolsPath)) -> do
        let table = schedulerDomain regions domain
        symbols <- parseSymbols (map regionSymbol (domainRegions table) ++ map shimBodySymbol (domainShims table)) symbolsPath
        bytes <- B.readFile bin
        Left <$> orFail ("owned " ++ domainLabel domain) (ownedImage table bytes symbols)
      _ -> failH "unreachable owned inputs"
  createDirectoryIfMissing True (takeDirectory work)
  createDirectory work
  reports <- fmap concat . forM (zip3 domains loaders images) $ \(domain, loader, image) -> do
    let table = schedulerDomain regions domain
        fill = regionsFill regions
    code <- orFail ("loader " ++ domainLabel domain) (stockCode domain loader)
    raise <- orFail "raise-IRQ" (stockRaiseIrq table code)
    forM helpers $ \helper -> do
      let name = domainLabel domain ++ " " ++ helperName helper
      stock <- orFail (name ++ " stock") (stockBody table code helper)
      owned <- orFail (name ++ " owned") $ case image of
        Left standalone -> ownedBody table standalone raise helper
        Right (aggregate,symbols,placements,carriers) -> aggregateBody table code aggregate symbols placements carriers helper
      let controls = [control | control <- equivalents ++ mutants, controlHelper control == helper]
      controlRoles <- forM controls $ \control -> do
        body <- orFail (name ++ " " ++ controlName control) (controlBody fill layout control owned)
        pure (Role (controlName control) body True)
      let directory = work </> domainLabel domain ++ "-" ++ helperName helper
          keys = map CaseKey [0 .. length (caseGrid layout helper) - 1]
      batches <- forM (zip [0 :: Int ..] (chunks keys)) $ \(index, batchKeys) -> do
        let batchDirectory = directory </> show index
        run <- runHelper env runner batchDirectory domain helper batchKeys (Role "stock" stock False) (Role "owned" owned True : Role "stock-repeat" stock False : controlRoles)
        let outcomes = M.fromList (runSubjects run)
            result role expectation note = ControlResult role expectation note (outcomes M.! role)
            report =
              HelperReport
                { reportDomain = domain
                , reportHelper = helper
                , reportEntry = bodyEntry owned
                , reportStockStatuses = statusCounts (runReference run)
                , reportStockHolds = runReferenceEnds run
                , reportOwned = outcomes M.! "owned"
                , reportControls =
                    result "stock-repeat" MustPass "the stock code against itself"
                      : [result (controlName control) (controlExpectation control) (controlNote control) | control <- controls]
                }
        _ <- evaluate (B.length (encodeCanonical (object ["helper" .= helperValue report, "controls" .= map (controlValue report) (reportControls report)])))
        _ <- evaluate (outcomeDifferingCompleted (reportOwned report))
        _ <- evaluate (outcomeDifferingOther (reportOwned report))
        pure report
      let report = foldl1 mergeReports batches
      when (verdictExit [report] == ExitSuccess) (removeDirectoryRecursive directory)
      pure report
  let exit = verdictExit reports
      inputs =
        object
          ( [ "regions_sha256" .= digest regionsBytes
            , "layout_sha256" .= digest layoutBytes
            , "runner_sha256" .= digest runnerBytes
            ]
              ++ [fromString ("loader_" ++ domainLabel domain ++ "_sha256") .= digest loader | (domain, loader) <- zip domains loaders]
              ++ [fromString name .= digest bytes | (name, bytes) <- zip (if aggregateMode then aggregateFlags else ["owned_a_bin_sha256", "owned_a_symbols_sha256", "owned_b_bin_sha256", "owned_b_symbols_sha256"]) ownedBytes]
          )
      domainValue domain = object ["domain" .= domainLabel domain, "helpers" .= [helperValue report | report <- reports, reportDomain report == domain]]
  createDirectoryIfMissing True (takeDirectory summaryPath)
  B.writeFile summaryPath $
    encodeCanonical $
      object
        [ "schema" .= (1 :: Int)
        , "inputs" .= inputs
        , "uncompared" .= uncomparedReason
        , "expected_read_difference" .= findTableReason
        , "domains" .= map domainValue domains
        , "controls" .= [controlValue report result | report <- reports, result <- reportControls report]
        , "stock_ends_as_expected" .= all reportStockHolds reports
        , "controls_hold" .= all (all holds . reportControls) reports
        , "owned_matches_stock" .= all ownedMatches reports
        ]
  exitWith exit
