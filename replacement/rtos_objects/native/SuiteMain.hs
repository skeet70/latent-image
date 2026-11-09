{-# LANGUAGE OverloadedStrings #-}

-- | Runs the F2 family cases in both entry domains against the stock loaders and the owned
-- stage bins, through the cached native batch, and writes a terminal JSON.
module Main (main) where

import Control.Monad (forM_, unless)
import Data.Aeson (object, (.=), encode)
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as BL
import FujiAF.Host (failH)
import FujiAF.StorageBudget (checkStorageBudget)
import FujiModNativePaths (fujimodNativeCase)
import System.Directory (createDirectoryIfMissing, makeAbsolute)
import System.Environment (getArgs)
import System.Exit (exitFailure)
import System.FilePath ((</>), takeDirectory)
import Replacement.RtosObjects.Layout (parseFacts)
import Replacement.RtosObjects.Suite (FamilyRun (..), Image (..), Payload (..), extents, runFamily)
import Replacement.RtosOwnFirst.Regions (Domain (..), GuestAddress (..), Extent (..))
import qualified Replacement.RtosPlatform.Aggregate as Agg
import qualified Replacement.RtosPlatform.Union as Union

main :: IO ()
main = do
  arguments <- getArgs
  runner <- case dropWhile (/= "--runner") arguments of
    [] -> pure fujimodNativeCase
    [_] -> failH "--runner needs a value"
    _ : value : _ -> makeAbsolute value
  cacheRoot <- required "--cache-root"
  layoutPath <- required "--layout"
  loaderA <- required "--loader-a"
  loaderB <- required "--loader-b"
  aggregate <- aggregateInputs arguments
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
  checkStorageBudget (takeDirectory resultsPath)
  createDirectoryIfMissing True cacheRoot
  let stockA = Image 0x40000 loaderABytes
      stockB = Image 0xd0000 loaderBBytes
  runA <- runFamily runner cacheRoot (takeDirectory resultsPath </> "domain-a") facts DomainA stockA ownedAImage
  runB <- runFamily runner cacheRoot (takeDirectory resultsPath </> "domain-b") facts DomainB stockB ownedBImage
  let runs = [runA, runB]
      failures =
        [(name, message) | run <- runs, (name, Left message) <- familyCases run]
          ++ [(name, unlines differences) | run <- runs, (name, Right differences) <- familyCases run, not (null differences)]
      cases =
        [ object
            [ "domain" .= show (familyDomain run)
            , "case" .= name
            , "result" .= outcomeText result
            ]
        | run <- runs
        , (name, result) <- familyCases run
        ]
      outcomeText (Left message) = "failed: " ++ message
      outcomeText (Right []) = "ok"
      outcomeText (Right differences) = "failed: " ++ unlines differences
      reuse =
        [ object ["domain" .= show (familyDomain run), "stock" .= show (fst (familyReuse run)), "owned" .= show (snd (familyReuse run))]
        | run <- runs
        ]
      document = object ["cases" .= cases, "reuse" .= reuse, "failures" .= length failures]
  BL.writeFile resultsPath (encode document)
  forM_ runs $ \run -> forM_ (familyCases run) $ \(name, result) ->
    putStrLn (show (familyDomain run) ++ " " ++ name ++ ": " ++ outcomeText result)
  forM_ failures $ \(name, message) -> putStrLn (name ++ ": " ++ message)
  unless (null failures) exitFailure

required :: String -> IO FilePath
required flag = do
  arguments <- getArgs
  case [value | (key,value) <- zip arguments (drop 1 arguments), key == flag] of
    [] -> failH (flag ++ " is required and needs a value")
    values -> pure (last values)

aggregateInputs :: [String] -> IO (Maybe (Payload, Payload))
aggregateInputs arguments
  | not (any (`elem` arguments) flags) = pure Nothing
  | otherwise = do
      paths <- mapM required flags
      case paths of
        [aBin,aSymbols,aPlacements,bBin,bSymbols,bPlacements,carriersPath] -> do
          union <- either (fail . show) pure . Union.parseUnion =<< B.readFile carriersPath
          a <- load DomainA union aBin aSymbols aPlacements
          b <- load DomainB union bBin bSymbols bPlacements
          pure (Just (a,b))
        _ -> fail "aggregate input arity differs"
  where
    flags = ["--aggregate-a-bin","--aggregate-a-symbols","--aggregate-a-placements",
             "--aggregate-b-bin","--aggregate-b-symbols","--aggregate-b-placements","--aggregate-carriers"]
    load domain union bin symbolsPath placementsPath = do
      symbols <- either fail pure . Agg.parseAggregateSymbols =<< B.readFile symbolsPath
      placements <- either fail pure . Agg.parsePlacements =<< B.readFile placementsPath
      image <- either fail pure . Agg.aggregateImage symbols =<< B.readFile bin
      let domainMap = Union.domainMap union domain
          carriers = map Union.carrierExtent (Union.mapCarriers domainMap)
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
      pure (Aggregate image symbols placements carriers)
