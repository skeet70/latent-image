{-# LANGUAGE OverloadedStrings #-}

-- | Runs the semaphore family cases in both entry domains against the stock loaders and the owned
-- stage bins, through the cached native batch, and writes a terminal JSON.
module Main (main) where

import Control.Monad (forM_, unless, when)
import Data.Aeson (object, (.=), encode)
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as BL
import Data.Maybe (isJust)
import Data.List (intercalate)
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
import Replacement.RtosSemaphore.Oracle (Domain (..), Image (..), Run (..), extents, parseFacts, runCases, runControls, runDomain, runFamily, runReuse)

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
  symbolsAPath <- optional "--symbols-a"
  symbolsBPath <- optional "--symbols-b"
  placementsAPath <- optional "--placements-a"
  placementsBPath <- optional "--placements-b"
  carriersPath <- optional "--carriers"
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
  symbolsA <- traverse (readSymbols) symbolsAPath
  symbolsB <- traverse (readSymbols) symbolsBPath
  placementsA <- traverse readPlacements placementsAPath
  placementsB <- traverse readPlacements placementsBPath
  carriers <- traverse readCarriers carriersPath
  wrongABytes <- traverse B.readFile wrongAPath
  wrongBBytes <- traverse B.readFile wrongBPath
  wrongSymbolsA <- traverse readSymbols wrongSymbolsAPath
  wrongSymbolsB <- traverse readSymbols wrongSymbolsBPath
  wrongPlacementsA <- traverse readPlacements wrongPlacementsAPath
  wrongPlacementsB <- traverse readPlacements wrongPlacementsBPath
  let wrongInputs = [fmap (const ()) wrongAPath, fmap (const ()) wrongBPath, fmap (const ()) wrongSymbolsAPath,
                     fmap (const ()) wrongSymbolsBPath, fmap (const ()) wrongPlacementsAPath, fmap (const ()) wrongPlacementsBPath]
  when (any isJust wrongInputs) $
    unless (all isJust wrongInputs) (failH "the wrong variant needs --wrong-a-bin, --wrong-a-symbols, --wrong-a-placements, --wrong-b-bin, --wrong-b-symbols and --wrong-b-placements")
  forM_ [symbolsA, symbolsB] $ \selected ->
    when (maybe False (const True) selected && maybe True (const False) carriers)
      (failH "--carriers is required with --symbols-a or --symbols-b")
  executableDigest <- sha256Hex <$> (sha256File =<< getExecutablePath)
  runnerDigest <- sha256Hex <$> sha256File runner
  loaderDigestA <- sha256Hex <$> sha256File loaderA
  loaderDigestB <- sha256Hex <$> sha256File loaderB
  ownedDigestA <- sha256Hex <$> sha256File ownedA
  ownedDigestB <- sha256Hex <$> sha256File ownedB
  layoutDigest <- sha256Hex <$> sha256File layoutPath
  symbolsDigestA <- traverse (fmap sha256Hex . sha256File) symbolsAPath
  symbolsDigestB <- traverse (fmap sha256Hex . sha256File) symbolsBPath
  placementsDigestA <- traverse (fmap sha256Hex . sha256File) placementsAPath
  placementsDigestB <- traverse (fmap sha256Hex . sha256File) placementsBPath
  wrongDigestA <- traverse (fmap sha256Hex . sha256File) wrongAPath
  wrongDigestB <- traverse (fmap sha256Hex . sha256File) wrongBPath
  checkStorageBudget (takeDirectory resultsPath)
  createDirectoryIfMissing True cacheRoot
  let stockA = Image 0x40000 loaderABytes [] [] [] 0
      stockB = Image 0xd0000 loaderBBytes [] [] [] 0
      ownedAImage = ownedImage DomainA ownedABytes symbolsA placementsA carriers
      ownedBImage = ownedImage DomainB ownedBBytes symbolsB placementsB carriers
      wrongAImage = case (wrongABytes, wrongSymbolsA, wrongPlacementsA) of
        (Just bytes, Just symbols, Just placements) -> Just (ownedImage DomainA bytes (Just symbols) (Just placements) carriers)
        _ -> Nothing
      wrongBImage = case (wrongBBytes, wrongSymbolsB, wrongPlacementsB) of
        (Just bytes, Just symbols, Just placements) -> Just (ownedImage DomainB bytes (Just symbols) (Just placements) carriers)
        _ -> Nothing
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
          ++ [(name, "semantic control did not reject corruption") | run <- runs, (name, False) <- runControls run]
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
      controls = map variantRow wrongRuns
      observationControls =
        [ object ["name" .= name, "domain" .= show (runDomain run), "reported" .= passed, "expected" .= ("the mutation is reported different" :: String)]
        | run <- runs, (name, passed) <- runControls run
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
      document =
        object
          [ "verdict" .= verdict
          , "cases" .= cases
          , "families" .= families
          , "inputs" .= inputs
          , "controls" .= controls
          , "observation_controls" .= observationControls
          , "reuse" .= reuse
          , "failures" .= length failures
          ]
      judgeReport label domain run loaderDigest imageDigest =
        object
          [ "suite" .= ("rtos-semaphore-" ++ label)
          , "domain" .= show domain
          , "firmware_sha256" .= loaderDigest
          , "owned_sha256" .= imageDigest
          , "inputs" .= inputs
          , "cases" .= map (caseRow run) (runCases run)
          , "controls" .= [row | (variantDomain, variantRun) <- wrongRuns, variantDomain == domain, row <- [variantRow (variantDomain, variantRun)]]
          , "observation_controls" .= [object ["name" .= name, "domain" .= show (runDomain run), "reported" .= passed] | (name, passed) <- runControls run]
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

ownedImage :: Domain -> B.ByteString -> Maybe [Agg.AggregateSymbol] -> Maybe [Agg.SectionPlacement] -> Maybe Agg.CarrierUnion -> Image
ownedImage domain bytes Nothing _ _ = Image (minimum (map fst (extents domain))) bytes [] [] [] 0
ownedImage domain bytes (Just symbols) placements carriers =
  let image = either error id (Agg.aggregateImage symbols bytes)
      union = maybe (error ("carrier extents are missing for " ++ show domain)) id carriers
      extents' = case domain of
        DomainA -> Agg.carrierExtentsA union
        DomainB -> Agg.carrierExtentsB union
      selected = [(lo, hi - lo) | (lo, hi) <- extents']
   in Image (Agg.imageBase image) (Agg.imageBytes image) symbols (maybe [] id placements) selected (Agg.carrierFill union)

readSymbols :: FilePath -> IO [Agg.AggregateSymbol]
readSymbols path = either fail pure . Agg.parseAggregateSymbols =<< B.readFile path

readPlacements :: FilePath -> IO [Agg.SectionPlacement]
readPlacements path = either fail pure . Agg.parsePlacements =<< B.readFile path

readCarriers :: FilePath -> IO Agg.CarrierUnion
readCarriers path = either fail pure . Agg.parseCarrierUnion =<< B.readFile path

optional :: String -> IO (Maybe FilePath)
optional flag = do
  arguments <- getArgs
  case dropWhile (/= flag) arguments of
    [] -> pure Nothing
    [_] -> failH (flag ++ " needs a value")
    _ : value : _ -> Just <$> makeAbsolute value

required :: String -> IO FilePath
required flag = do
  arguments <- getArgs
  case dropWhile (/= flag) arguments of
    [] -> failH (flag ++ " is required")
    [_] -> failH (flag ++ " needs a value")
    _ : value : _ -> pure value
