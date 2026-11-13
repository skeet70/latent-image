{-# LANGUAGE OverloadedStrings #-}

-- | Runs the F2 family cases in both entry domains against the stock loaders and the owned
-- stage bins, through the cached native batch, and writes a terminal JSON.
module Main (main) where

import Control.Monad (forM_, unless)
import Data.Aeson (object, (.=), encode)
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as BL
import Data.List (intercalate, isPrefixOf)
import Data.Word (Word32)
import Emulation.CachedNativeBatch (defaultNativeCacheRoot)
import Emulation.Sha256 (sha256File, sha256Hex)
import FujiAF.Host (failH)
import FujiAF.StorageBudget (checkStorageBudget)
import FujiModNativePaths (fujimodNativeCase)
import System.Directory (createDirectoryIfMissing, makeAbsolute)
import Numeric (showHex)
import System.Environment (getArgs, getExecutablePath)
import System.Exit (exitFailure)
import System.FilePath ((</>), takeDirectory)
import Replacement.RtosObjects.Layout (parseFacts)
import Replacement.RtosObjects.Suite (AggregatePayload (..), FamilyRun (..), Image (..), Payload (..), extents, retainedStock, runFamily)
import Replacement.RtosOwnFirst.Regions (Domain (..), GuestAddress (..), Extent (..), hexText)
import qualified Replacement.RtosPlatform.Aggregate as Agg
import qualified Replacement.RtosPlatform.Union as Union

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
  aggregate <- aggregateInputs arguments
  wrong <- wrongInputs arguments
  wrongDigests <- case wrong of
    Nothing -> pure Nothing
    Just _ -> do
      a <- sha256Hex <$> (sha256File =<< required "--wrong-a-bin")
      b <- sha256Hex <$> (sha256File =<< required "--wrong-b-bin")
      pure (Just (a, b))
  resultsPath <- required "--results"
  facts <- either fail pure . parseFacts =<< B.readFile layoutPath
  loaderABytes <- B.readFile loaderA
  loaderBBytes <- B.readFile loaderB
  (ownedAImage, ownedBImage) <- case aggregate of
    Just images -> pure images
    Nothing -> do
      ownedA <- required "--owned-a-bin"
      ownedB <- required "--owned-b-bin"
      a <- B.readFile ownedA
      b <- B.readFile ownedB
      pure (Standalone (Image (minimum (map fst (extents DomainA))) a),
            Standalone (Image (minimum (map fst (extents DomainB))) b))
  executableDigest <- sha256Hex <$> (sha256File =<< getExecutablePath)
  runnerDigest <- sha256Hex <$> sha256File runner
  loaderDigestA <- sha256Hex <$> sha256File loaderA
  loaderDigestB <- sha256Hex <$> sha256File loaderB
  (imageDigestA, imageDigestB) <- case aggregate of
    Just _ -> do
      a <- required "--aggregate-a-bin"
      b <- required "--aggregate-b-bin"
      (,) <$> (sha256Hex <$> sha256File a) <*> (sha256Hex <$> sha256File b)
    Nothing -> do
      a <- required "--owned-a-bin"
      b <- required "--owned-b-bin"
      (,) <$> (sha256Hex <$> sha256File a) <*> (sha256Hex <$> sha256File b)
  checkStorageBudget (takeDirectory resultsPath)
  createDirectoryIfMissing True cacheRoot
  let stockA = Image 0x40000 loaderABytes
      stockB = Image 0xd0000 loaderBBytes
  runA <- runFamily runner cacheRoot (takeDirectory resultsPath </> "domain-a") facts DomainA stockA ownedAImage
  runB <- runFamily runner cacheRoot (takeDirectory resultsPath </> "domain-b") facts DomainB stockB ownedBImage
  wrongRuns <- case wrong of
    Nothing -> pure []
    Just (wrongA, wrongB) -> do
      wrongRunA <- runFamily runner cacheRoot (takeDirectory resultsPath </> "wrong-domain-a") facts DomainA stockA wrongA
      wrongRunB <- runFamily runner cacheRoot (takeDirectory resultsPath </> "wrong-domain-b") facts DomainB stockB wrongB
      pure [(DomainA, wrongRunA), (DomainB, wrongRunB)]
  let runs = [runA, runB]
      requiredVariantCases = ["helper-release-queued"]
      variantDiffers (Left _) = True
      variantDiffers (Right differences) = not (null differences)
      variantDiffering run = [name | (name, result) <- familyCases run, variantDiffers result]
      variantDetected run = all (`elem` variantDiffering run) requiredVariantCases
      variantRow (domain, run) =
        object
          [ "name" .= ("wrong-variant-pre-correction-" ++ show domain)
          , "domain" .= show domain
          , "image_sha256" .= fmap (\(a, b) -> if domain == DomainA then a else b) wrongDigests
          , "expected" .= ("the pre-correction release body is reported different: " ++ intercalate ", " requiredVariantCases)
          , "reported" .= variantDetected run
          , "differences" .= variantDiffering run
          , "diagnostic_differences" .= ([] :: [String])
          ]
      variantRows = map variantRow wrongRuns
      observationRow run (name, passed) =
        object
          [ "name" .= (name ++ "-" ++ show (familyDomain run))
          , "domain" .= show (familyDomain run)
          , "expected" .= ("the mutation is reported different" :: String)
          , "reported" .= passed
          , "differences" .= (if passed then [] else ["the mutation was accepted"] :: [String])
          , "diagnostic_differences" .= ([] :: [String])
          ]
      failures =
        [(name, message) | run <- runs, (name, Left message) <- familyCases run]
          ++ [(name, unlines differences) | run <- runs, (name, Right differences) <- familyCases run, not (null differences)]
          ++ [(name, unlines differences) | run <- runs, (name, Right differences) <- familyGuards run, not (null differences)]
          ++ [(name, "control did not reject its mutation") | run <- runs, (name, False) <- familyControls run]
          ++ [(show domain, "the pre-correction wrong variant was not detected")
             | (domain, run) <- wrongRuns, not (variantDetected run)]
      rows run =
        [ object
            [ "id" .= (name ++ "-" ++ show (familyDomain run))
            , "domain" .= show (familyDomain run)
            , "entry" .= hex32 entry
            , "extent" .= extent
            , "stock_instructions_inside" .= inside
            , "stock_instructions" .= lookup name (familyStockInstructions run)
            , "result" .= outcomeText result
            , "differences" .= differenceList result
            , "diagnostic_differences" .= ([] :: [String])
            ]
        | (name, result) <- familyCases run
        , Just (entry, extent, inside) <- [lookup name (familyEvidence run)]
        ]
      differenceList (Left message) = [message]
      differenceList (Right []) = []
      differenceList (Right differences) = differences
      outcomeText (Left message) = "failed: " ++ message
      outcomeText (Right []) = "ok"
      outcomeText (Right differences) = "failed: " ++ unlines differences
      families =
        [ object
            [ "domain" .= show (familyDomain run)
            , "family" .= family
            , "cases" .= length [() | (other, _) <- familyCases run, familyOf other == family]
            ]
        | run <- runs
        , family <- unique (map (familyOf . fst) (familyCases run))
        ]
      reuse =
        [ object ["domain" .= show (familyDomain run), "stock" .= show (fst (familyReuse run)), "owned" .= show (snd (familyReuse run))]
        | run <- runs
        ]
      retained =
        [ object
            [ "domain" .= show domain
            , "address" .= hexText address
            , "bytes" .= size
            , "role" .= ("retained-stock" :: String)
            ]
        | (domain, payload) <- [(DomainA, ownedAImage), (DomainB, ownedBImage)]
        , (address, size) <- retainedStock domain payload
        ]
      inputs =
        object
          [ "loader_a_sha256" .= loaderDigestA
          , "loader_b_sha256" .= loaderDigestB
          , "image_a_sha256" .= imageDigestA
          , "image_b_sha256" .= imageDigestB
          , "runner_sha256" .= runnerDigest
          , "suite_sha256" .= executableDigest
          ]
      controls = variantRows
      observationControls = concatMap (\run -> map (observationRow run) (familyControls run)) runs
      guards =
        [ object
            [ "name" .= (name ++ "-" ++ show (familyDomain run))
            , "domain" .= show (familyDomain run)
            , "result" .= outcomeText result
            , "differences" .= differenceList result
            ]
        | run <- runs
        , (name, result) <- familyGuards run
        ]
      verdict = if null failures then "passed" else "failed" :: String
      document =
        object
          [ "verdict" .= verdict
          , "cases" .= concatMap rows runs
          , "controls" .= controls
          , "observation_controls" .= observationControls
          , "guards" .= guards
          , "families" .= families
          , "inputs" .= inputs
          , "reuse" .= reuse
          , "retained" .= retained
          , "failures" .= length failures
          ]
      judgeReport label domain run loaderDigest imageDigest =
        object
          [ "suite" .= label
          , "domain" .= show domain
          , "firmware_sha256" .= loaderDigest
          , "owned_sha256" .= imageDigest
          , "inputs" .= inputs
          , "cases" .= rows run
          , "controls" .= [row | (variantDomain, variantRun) <- wrongRuns, variantDomain == domain, row <- [variantRow (variantDomain, variantRun)]]
          , "observation_controls" .= map (observationRow run) (familyControls run)
          , "guards" .= [object ["name" .= (name ++ "-" ++ show (familyDomain run)), "result" .= outcomeText result, "differences" .= differenceList result] | (name, result) <- familyGuards run]
          , "failures" .= length failures
          ]
      suiteName :: String -> String
      suiteName domainLabel = case aggregate of
        Just _ -> "rtos-objects-aggregate-" ++ domainLabel
        Nothing -> "rtos-objects-" ++ domainLabel
      reportDirectory = takeDirectory resultsPath
  BL.writeFile (reportDirectory </> "judge-report-a.json") (encode (judgeReport (suiteName "a") DomainA runA loaderDigestA imageDigestA))
  BL.writeFile (reportDirectory </> "judge-report-b.json") (encode (judgeReport (suiteName "b") DomainB runB loaderDigestB imageDigestB))
  BL.writeFile resultsPath (encode document)
  forM_ runs $ \run -> forM_ (familyCases run ++ familyGuards run) $ \(name, result) ->
    putStrLn (show (familyDomain run) ++ " " ++ name ++ ": " ++ outcomeText result)
  forM_ failures $ \(name, message) -> putStrLn (name ++ ": " ++ message)
  unless (null failures) exitFailure

hex32 :: Word32 -> String
hex32 value = "0x" ++ replicate (8 - length text) '0' ++ text
  where text = showHex value ""

unique :: Eq a => [a] -> [a]
unique [] = []
unique (first : rest) = first : unique (filter (/= first) rest)

familyOf :: String -> String
familyOf name
  | "set-handler-" `isPrefixOf` name = "set-handler"
  | "set-" `isPrefixOf` name = "set"
  | "clear-" `isPrefixOf` name = "clear"
  | "wait-" `isPrefixOf` name = "wait"
  | "poll-" `isPrefixOf` name = "poll"
  | "timed-wait-" `isPrefixOf` name = "timed-wait"
  | "create-" `isPrefixOf` name = "create"
  | "delete-" `isPrefixOf` name = "delete"
  | "send-" `isPrefixOf` name = "send"
  | "release-" `isPrefixOf` name = "release"
  | "helper-" `isPrefixOf` name = "helper"
  | "mailbox-" `isPrefixOf` name = "mailbox"
  | otherwise = name

required :: String -> IO FilePath
required flag = do
  arguments <- getArgs
  case [value | (key,value) <- zip arguments (drop 1 arguments), key == flag] of
    [] -> failH (flag ++ " is required and needs a value")
    values -> pure (last values)

optional :: String -> IO (Maybe FilePath)
optional flag = do
  arguments <- getArgs
  case [value | (key,value) <- zip arguments (drop 1 arguments), key == flag] of
    [] -> pure Nothing
    values -> pure (Just (last values))

aggregateInputs :: [String] -> IO (Maybe (Payload, Payload))
aggregateInputs = aggregateInputsWith "aggregate"

-- | The same load for the deliberately wrong owned variant, named with its own flags.
wrongInputs :: [String] -> IO (Maybe (Payload, Payload))
wrongInputs arguments
  | not (any (`elem` arguments) wrongFlags) = pure Nothing
  | otherwise = aggregateInputsWith "wrong" arguments
  where
    wrongFlags =
      [ "--wrong-a-bin", "--wrong-a-symbols", "--wrong-a-placements"
      , "--wrong-b-bin", "--wrong-b-symbols", "--wrong-b-placements"
      ]

aggregateInputsWith :: String -> [String] -> IO (Maybe (Payload, Payload))
aggregateInputsWith prefix arguments
  | not (any (`elem` arguments) flags') = pure Nothing
  | otherwise = do
      paths <- mapM required flags'
      case paths of
        [aBin,aSymbols,aPlacements,bBin,bSymbols,bPlacements,carriersPath] -> do
          union <- either (fail . show) pure . Union.parseUnion =<< B.readFile carriersPath
          a <- load DomainA union aBin aSymbols aPlacements
          b <- load DomainB union bBin bSymbols bPlacements
          pure (Just (a,b))
        _ -> fail "aggregate input arity differs"
  where
    flags' = flags prefix ++ ["--aggregate-carriers"]
    flags name =
      [ "--" ++ name ++ "-a-bin", "--" ++ name ++ "-a-symbols", "--" ++ name ++ "-a-placements"
      , "--" ++ name ++ "-b-bin", "--" ++ name ++ "-b-symbols", "--" ++ name ++ "-b-placements"
      ]
    load domain union bin symbolsPath placementsPath = do
      symbols <- either fail pure . Agg.parseAggregateSymbols =<< B.readFile symbolsPath
      placements <- either fail pure . Agg.parsePlacements =<< B.readFile placementsPath
      image <- either fail pure . Agg.aggregateImage symbols =<< B.readFile bin
      let domainMap = Union.domainMap union domain
          carriers = map Union.carrierExtent (Union.mapCarriers domainMap)
          within :: Word32 -> Word32 -> Bool
          within address size = any (\(Extent (GuestAddress lo) (GuestAddress hi)) ->
            lo <= address && toInteger address + toInteger size <= toInteger hi) carriers
          spans = [(Agg.symbolAddress s, Agg.symbolSize s) | s <- symbols]
            ++ [(Agg.placementAddress p, Agg.placementBytes p) | p <- placements, Agg.placementBytes p > 0]
      forM_ carriers $ \(Extent (GuestAddress lo) (GuestAddress hi)) ->
        either fail (const (pure ())) (Agg.sliceAggregate image lo (hi-lo))
      unless (any ((== "xm1_rtos_timer_remove_body") . Agg.symbolName) symbols)
        (fail "aggregate objects cases require the owned timer removal body")
      forM_ spans $ \(address,size) -> do
        unless (within address size) (fail ("aggregate extent outside admitted carrier: " ++ show (address,size)))
        either fail (const (pure ())) (Agg.sliceAggregate image address size)
      forM_ (Union.mapEntries domainMap) $ \entry -> do
        let GuestAddress address = Union.entryAddress entry
        unless (within address 4 && any (\s -> Agg.symbolAddress s == address && Agg.symbolSize s > 0) symbols)
          (fail ("aggregate entry missing live veneer: " ++ Union.entryName entry))
      pure (Aggregate (AggregatePayload image symbols placements carriers (Union.unionFill union)))
