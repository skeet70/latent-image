{-# LANGUAGE OverloadedStrings #-}

-- | Runs the fixed-pool family cases in both entry domains against the stock loaders and the owned
-- stage bins, through the cached native batch, and writes a terminal JSON.
module Main (main) where

import Control.Monad (forM_, unless, when)
import Data.Aeson (object, (.=), encode)
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as BL
import qualified Data.Map.Strict as M
import Data.List (intercalate)
import Data.Maybe (isJust)
import Data.Word (Word32)
import Emulation.CachedNativeBatch (defaultNativeCacheRoot)
import Emulation.Sha256 (sha256File, sha256Hex)
import Emulation.SuiteAudit (word32Hex)
import FujiAF.Host (failH)
import FujiAF.StorageBudget (checkStorageBudget)
import FujiModNativePaths (fujimodNativeCase)
import System.Directory (createDirectoryIfMissing, makeAbsolute)
import System.Environment (getArgs, getExecutablePath)
import System.Exit (exitFailure)
import System.FilePath ((</>), takeDirectory)
import qualified Replacement.RtosPlatform.Aggregate as Agg
import Replacement.RtosFixedPool.Oracle (Domain (..), Image (..), Run (..), Service (..), bodyRegion, entries, extents, parseFacts, runFamily)

main :: IO ()
main = do
  arguments <- getArgs
  runner <- case dropWhile (/= "--runner") arguments of
    [] -> pure fujimodNativeCase
    [_] -> failH "--runner needs a value"
    _ : value : _ -> makeAbsolute value
  cacheRoot <- maybe defaultNativeCacheRoot id <$> optional "--cache-root"
  layoutPath <- required "--layout"
  loaderA <- required "--loader-a"
  loaderB <- required "--loader-b"
  ownedA <- required "--owned-a-bin"
  ownedB <- required "--owned-b-bin"
  placementsAPath <- optional "--placements-a"
  placementsBPath <- optional "--placements-b"
  carriersPath <- optional "--carriers"
  symbolsA <- required "--owned-a-symbols"
  symbolsB <- required "--owned-b-symbols"
  wrongAPath <- optional "--wrong-a-bin"
  wrongBPath <- optional "--wrong-b-bin"
  wrongSymbolsAPath <- optional "--wrong-a-symbols"
  wrongSymbolsBPath <- optional "--wrong-b-symbols"
  wrongPlacementsAPath <- optional "--wrong-a-placements"
  wrongPlacementsBPath <- optional "--wrong-b-placements"
  resultsPath <- required "--results"
  facts <- either fail pure . parseFacts =<< B.readFile layoutPath
  loaderABytes <- B.readFile loaderA
  loaderBBytes <- B.readFile loaderB
  ownedABytes <- B.readFile ownedA
  ownedBBytes <- B.readFile ownedB
  ownedASymbols <- parseSymbols <$> B.readFile symbolsA
  ownedBSymbols <- parseSymbols <$> B.readFile symbolsB
  checkStorageBudget (takeDirectory resultsPath)
  createDirectoryIfMissing True cacheRoot
  placementsA <- traverse readPlacements placementsAPath
  placementsB <- traverse readPlacements placementsBPath
  carriers <- traverse readCarriers carriersPath
  forM_ [placementsA, placementsB] $ \selected ->
    when (maybe False (const True) selected && maybe True (const False) carriers)
      (failH "--carriers is required with --placements-a or --placements-b")
  ownedAImage <- ownedImage DomainA ownedABytes ownedASymbols placementsA carriers
  ownedBImage <- ownedImage DomainB ownedBBytes ownedBSymbols placementsB carriers
  wrongABytes <- traverse B.readFile wrongAPath
  wrongBBytes <- traverse B.readFile wrongBPath
  wrongSymbolsA <- traverse (fmap parseSymbols . B.readFile) wrongSymbolsAPath
  wrongSymbolsB <- traverse (fmap parseSymbols . B.readFile) wrongSymbolsBPath
  wrongPlacementsA <- traverse readPlacements wrongPlacementsAPath
  wrongPlacementsB <- traverse readPlacements wrongPlacementsBPath
  let wrongInputs = [fmap (const ()) wrongAPath, fmap (const ()) wrongBPath, fmap (const ()) wrongSymbolsAPath,
                     fmap (const ()) wrongSymbolsBPath, fmap (const ()) wrongPlacementsAPath, fmap (const ()) wrongPlacementsBPath]
  when (any isJust wrongInputs) $
    unless (all isJust wrongInputs) (failH "the wrong variant needs --wrong-a-bin, --wrong-a-symbols, --wrong-a-placements, --wrong-b-bin, --wrong-b-symbols and --wrong-b-placements")
  wrongAImage <- case (wrongABytes, wrongSymbolsA, wrongPlacementsA) of
    (Just bytes, Just symbols, Just placements) -> Just <$> ownedImage DomainA bytes symbols (Just placements) carriers
    _ -> pure Nothing
  wrongBImage <- case (wrongBBytes, wrongSymbolsB, wrongPlacementsB) of
    (Just bytes, Just symbols, Just placements) -> Just <$> ownedImage DomainB bytes symbols (Just placements) carriers
    _ -> pure Nothing
  executableDigest <- sha256Hex <$> (sha256File =<< getExecutablePath)
  runnerDigest <- sha256Hex <$> sha256File runner
  loaderDigestA <- sha256Hex <$> sha256File loaderA
  loaderDigestB <- sha256Hex <$> sha256File loaderB
  ownedDigestA <- sha256Hex <$> sha256File ownedA
  ownedDigestB <- sha256Hex <$> sha256File ownedB
  layoutDigest <- sha256Hex <$> sha256File layoutPath
  symbolsDigestA <- sha256Hex <$> sha256File symbolsA
  symbolsDigestB <- sha256Hex <$> sha256File symbolsB
  placementsDigestA <- traverse (fmap sha256Hex . sha256File) placementsAPath
  placementsDigestB <- traverse (fmap sha256Hex . sha256File) placementsBPath
  wrongDigestA <- traverse (fmap sha256Hex . sha256File) wrongAPath
  wrongDigestB <- traverse (fmap sha256Hex . sha256File) wrongBPath
  let stockA = Image 0x40000 loaderABytes [] [] [] 0
      stockB = Image 0xd0000 loaderBBytes [] [] [] 0
  runA <- runFamily runner cacheRoot (takeDirectory resultsPath </> "domain-a") facts DomainA stockA ownedAImage
  runB <- runFamily runner cacheRoot (takeDirectory resultsPath </> "domain-b") facts DomainB stockB ownedBImage
  wrongRunA <- traverse (\image -> runFamily runner cacheRoot (takeDirectory resultsPath </> "wrong-domain-a") facts DomainA stockA image) wrongAImage
  wrongRunB <- traverse (\image -> runFamily runner cacheRoot (takeDirectory resultsPath </> "wrong-domain-b") facts DomainB stockB image) wrongBImage
  let runs = [runA, runB]
      wrongRuns = [(DomainA, run) | Just run <- [wrongRunA]] ++ [(DomainB, run) | Just run <- [wrongRunB]]
      requiredVariantCases = ["delete-waiter"]
      variantDiffers (Left _) = True
      variantDiffers (Right differences) = not (null differences)
      variantDiffering run = [name | (name, result) <- runCases run, variantDiffers result]
      variantDetected run = all (`elem` variantDiffering run) requiredVariantCases
      variantRow (domain, run) =
        object
          [ "name" .= ("wrong-variant-pre-correction-" ++ show domain)
          , "domain" .= show domain
          , "image_sha256" .= (if domain == DomainA then wrongDigestA else wrongDigestB)
          , "expected" .= ("the pre-correction release body is reported different: " ++ intercalate ", " requiredVariantCases)
          , "reported" .= variantDetected run
          , "differences" .= variantDiffering run
          , "diagnostic_differences" .= ([] :: [String])
          ]
      failures =
        [(name, message) | run <- runs, (name, Left message) <- runCases run]
          ++ [(name, unlines differences) | run <- runs, (name, Right differences) <- runCases run, not (null differences)]
          ++ [(name, unlines differences) | run <- runs, (name, Right differences) <- runGuards run, not (null differences)]
          ++ [(name, "control did not reject its mutation") | run <- runs, (name, False) <- runControls run]
          ++ [(show domain, "the pre-correction wrong variant was not detected")
             | (domain, run) <- wrongRuns, not (variantDetected run)]
      differenceList (Left message) = [message]
      differenceList (Right []) = []
      differenceList (Right differences) = differences
      outcomeText (Left message) = "failed: " ++ message
      outcomeText (Right []) = "ok"
      outcomeText (Right differences) = "failed: " ++ unlines differences
      caseRow run (name, result) = case lookup name (runEvidence run) of
        Just (entry, extent, inside) ->
          object
            [ "id" .= (name ++ "-" ++ show (runDomain run))
            , "domain" .= show (runDomain run)
            , "case" .= name
            , "entry" .= word32Hex entry
            , "extent" .= extent
            , "stock_instructions_inside" .= inside
            , "stock_instructions" .= lookup name (runStockInstructions run)
            , "result" .= outcomeText result
            , "differences" .= differenceList result
            , "diagnostic_differences" .= ([] :: [String])
            ]
        Nothing -> error ("stock evidence is missing for " ++ name)
      cases = concatMap (\run -> map (caseRow run) (runCases run)) runs
      families =
        [ object
            [ "domain" .= show (runDomain run)
            , "family" .= family
            , "cases" .= length [() | (other, _) <- runCases run, familyOf other == family]
            ]
        | run <- runs
        , family <- unique (map (familyOf . fst) (runCases run))
        ]
      reuse =
        [ object ["domain" .= show (runDomain run), "stock" .= show (fst (runReuse run)), "owned" .= show (snd (runReuse run))]
        | run <- runs
        ]
      inputs =
        object
          [ "loader_a_sha256" .= loaderDigestA
          , "loader_b_sha256" .= loaderDigestB
          , "image_a_sha256" .= ownedDigestA
          , "image_b_sha256" .= ownedDigestB
          , "layout_sha256" .= layoutDigest
          , "symbols_a_sha256" .= symbolsDigestA
          , "symbols_b_sha256" .= symbolsDigestB
          , "placements_a_sha256" .= placementsDigestA
          , "placements_b_sha256" .= placementsDigestB
          , "runner_sha256" .= runnerDigest
          , "suite_sha256" .= executableDigest
          ]
      verdict = if null failures then "passed" else "failed" :: String
      controls = map variantRow wrongRuns
      observationControls =
        [ object ["name" .= name, "domain" .= show (runDomain run), "reported" .= passed, "expected" .= ("the mutation is reported different" :: String)]
        | run <- runs, (name, passed) <- runControls run
        ]
      guards =
        [ object ["name" .= name, "domain" .= show (runDomain run), "result" .= outcomeText result, "differences" .= differenceList result]
        | run <- runs, (name, result) <- runGuards run
        ]
      document =
        object
          [ "verdict" .= verdict
          , "cases" .= cases
          , "controls" .= controls
          , "observation_controls" .= observationControls
          , "guards" .= guards
          , "families" .= families
          , "inputs" .= inputs
          , "reuse" .= reuse
          , "failures" .= length failures
          ]
      judgeReport label domain run loaderDigest imageDigest =
        object
          [ "suite" .= ("rtos-fixed-pool-" ++ label)
          , "domain" .= show domain
          , "firmware_sha256" .= loaderDigest
          , "owned_sha256" .= imageDigest
          , "inputs" .= inputs
          , "cases" .= map (caseRow run) (runCases run)
          , "controls" .= [row | (variantDomain, variantRun) <- wrongRuns, variantDomain == domain, row <- [variantRow (variantDomain, variantRun)]]
          , "observation_controls" .= [object ["name" .= name, "domain" .= show (runDomain run), "reported" .= passed] | (name, passed) <- runControls run]
          , "guards" .= [object ["name" .= name, "result" .= outcomeText result, "differences" .= differenceList result] | (name, result) <- runGuards run]
          , "failures" .= length failures
          ]
      reportDirectory = takeDirectory resultsPath
  BL.writeFile (reportDirectory </> "judge-report-a.json") (encode (judgeReport "a" DomainA runA loaderDigestA ownedDigestA))
  BL.writeFile (reportDirectory </> "judge-report-b.json") (encode (judgeReport "b" DomainB runB loaderDigestB ownedDigestB))
  BL.writeFile resultsPath (encode document)
  forM_ runs $ \run -> forM_ (runCases run) $ \(name, result) ->
    putStrLn (show (runDomain run) ++ " " ++ name ++ ": " ++ outcomeText result)
  forM_ failures $ \(name, message) -> putStrLn (name ++ ": " ++ message)
  unless (null failures) exitFailure

unique :: Eq a => [a] -> [a]
unique [] = []
unique (first : rest) = first : unique (filter (/= first) rest)

familyOf :: String -> String
familyOf = takeWhile (/= '-')

origin :: Domain -> M.Map String Word32 -> IO Word32
origin domain symbols = do
  let expected = minimum (bodyRegion : map fst (extents domain))
      entrySymbol = "xm1_rtos_fixed_pool_poll_unchecked"
      entryAddress = entries domain PollUnchecked
  case M.lookup entrySymbol symbols of
    Just address | address == entryAddress -> pure ()
    Just address -> failH (show domain ++ " owned entry is at " ++ show address ++ ", not " ++ show entryAddress)
    Nothing -> failH (show domain ++ " owned symbols lack " ++ entrySymbol)
  forM_ (expectedEntries domain) $ \(address, name) ->
    case M.lookup name symbols of
      Just actual | actual == address -> pure ()
      Just actual -> failH (show domain ++ " " ++ name ++ " is at " ++ show actual ++ ", not " ++ show address)
      Nothing -> failH (show domain ++ " owned symbols lack " ++ name)
  forM_ bodySymbols $ \name ->
    case M.lookup name symbols of
      Just address | address >= bodyRegion -> pure ()
      Just address -> failH (show domain ++ " " ++ name ++ " is at " ++ show address ++ ", below the body region")
      Nothing -> failH (show domain ++ " owned symbols lack " ++ name)
  pure expected

bodySymbols :: [String]
bodySymbols =
  [ "xm1_rtos_fixed_pool_create_body"
  , "xm1_rtos_fixed_pool_delete_body"
  , "xm1_rtos_fixed_pool_get_body"
  , "xm1_rtos_fixed_pool_poll_body"
  , "xm1_rtos_fixed_pool_timed_get_body"
  , "xm1_rtos_fixed_pool_release_body"
  , "xm1_rtos_fixed_pool_poll_unchecked_body"
  ]

expectedEntries :: Domain -> [(Word32, String)]
expectedEntries domain =
  [ (entries domain Create, "xm1_rtos_fixed_pool_create")
  , (entries domain Delete, "xm1_rtos_fixed_pool_delete")
  , (entries domain Get, "xm1_rtos_fixed_pool_get")
  , (entries domain Poll, "xm1_rtos_fixed_pool_poll")
  , (entries domain TimedGet, "xm1_rtos_fixed_pool_timed_get")
  , (entries domain Release, "xm1_rtos_fixed_pool_release")
  , (entries domain PollUnchecked, "xm1_rtos_fixed_pool_poll_unchecked")
  ]

parseSymbols :: B.ByteString -> [Agg.AggregateSymbol]
parseSymbols = either error id . Agg.parseAggregateSymbols

readPlacements :: FilePath -> IO [Agg.SectionPlacement]
readPlacements path = either failH pure . Agg.parsePlacements =<< B.readFile path

readCarriers :: FilePath -> IO Agg.CarrierUnion
readCarriers path = either failH pure . Agg.parseCarrierUnion =<< B.readFile path

ownedImage :: Domain -> B.ByteString -> [Agg.AggregateSymbol] -> Maybe [Agg.SectionPlacement] -> Maybe Agg.CarrierUnion -> IO Image
ownedImage domain bytes symbols placements carriers
  | Just sections <- placements = do
      unless (not (null sections)) (failH "aggregate placements are empty")
      union <- maybe (failH ("carrier extents are missing for " ++ show domain)) pure carriers
      image <- either failH pure (Agg.aggregateImage symbols bytes)
      forM_ sections $ \section -> do
        _ <- either failH pure (Agg.sliceAggregate image (Agg.placementAddress section) (Agg.placementBytes section))
        pure ()
      let declared = case domain of
            DomainA -> Agg.carrierExtentsA union
            DomainB -> Agg.carrierExtentsB union
          extents' = [(lo, hi - lo) | (lo, hi) <- declared]
      pure (Image (Agg.imageBase image) (Agg.imageBytes image) symbols sections extents' (Agg.carrierFill union))
  | any (isAggregate . Agg.symbolName) symbols =
      failH "aggregate fixed-pool qualification requires --placements-a and --placements-b"
  | otherwise = do
      base <- origin domain (M.fromList [(Agg.symbolName symbol, Agg.symbolAddress symbol) | symbol <- symbols])
      pure (Image base bytes [] [] [] 0)
  where
    isAggregate name = take 20 name == "xm1_platform_veneer_"

optional :: String -> IO (Maybe FilePath)
optional flag = do
  arguments <- getArgs
  case reverse [drop (index + 1) arguments | (index, argument) <- zip [0 :: Int ..] arguments, argument == flag] of
    [] -> pure Nothing
    [] : _ -> failH (flag ++ " needs a value")
    (value : _) : _ -> Just <$> makeAbsolute value

required :: String -> IO FilePath
required flag = optional flag >>= maybe (failH (flag ++ " is required")) pure
