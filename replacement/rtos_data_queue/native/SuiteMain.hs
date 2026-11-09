{-# LANGUAGE OverloadedStrings #-}

-- | Runs the data-queue cases in both entry domains against the stock loaders and the owned
-- A32 entries, through the cached native batch, and writes a terminal and review JSON.
module Main (main) where

import Control.Monad (forM_, unless)
import Data.Aeson (object, (.=), encode)
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as BL
import Data.Word (Word32)
import Emulation.NativeObservation (parseWord32)
import Emulation.Sha256 (sha256File, sha256Hex)
import FujiAF.Host (failH)
import FujiAF.StorageBudget (checkStorageBudget)
import Replacement.RtosDataQueue.Layout (parseFacts)
import qualified Replacement.RtosPlatform.Aggregate as Agg
import Replacement.RtosDataQueue.Suite (Aggregate (..), CaseRun (..), ControlRun (..), Image (..), runDomain)
import Replacement.RtosOwnFirst.Regions (Domain (..), domainLabel)
import System.Directory (createDirectoryIfMissing, makeAbsolute)
import System.Environment (getArgs)
import System.Exit (exitFailure)
import System.FilePath ((</>), takeDirectory)

main :: IO ()
main = do
  runner <- required "--runner" >>= makeAbsolute
  cacheRoot <- required "--cache-root"
  layoutPath <- required "--layout"
  loaderA <- required "--loader-a"
  loaderB <- required "--loader-b"
  ownedA <- required "--owned-a-bin"
  ownedB <- required "--owned-b-bin"
  timerA <- required "--timer-a-bin"
  timerB <- required "--timer-b-bin"
  symbolsAPath <- required "--owned-a-symbols"
  symbolsBPath <- required "--owned-b-symbols"
  aggregateABin <- optional "--aggregate-a-bin"
  aggregateASymbols <- optional "--aggregate-a-symbols"
  aggregateBBin <- optional "--aggregate-b-bin"
  aggregateBSymbols <- optional "--aggregate-b-symbols"
  aggregateAPlacements <- optional "--aggregate-a-placements"
  aggregateBPlacements <- optional "--aggregate-b-placements"
  resultsPath <- required "--results"
  reviewPath <- required "--review"
  facts <- either failH pure . parseFacts =<< B.readFile layoutPath
  loaderABytes <- B.readFile loaderA
  loaderBBytes <- B.readFile loaderB
  ownedABytes <- B.readFile ownedA
  ownedBBytes <- B.readFile ownedB
  timerABytes <- B.readFile timerA
  timerBBytes <- B.readFile timerB
  symbolsA <- parseOwnedSymbols symbolsAPath
  symbolsB <- parseOwnedSymbols symbolsBPath
  baseA <- ownedBase symbolsAPath
  baseB <- ownedBase symbolsBPath
  layoutDigest <- sha256File layoutPath
  runnerDigest <- sha256File runner
  loaderADigest <- sha256File loaderA
  loaderBDigest <- sha256File loaderB
  ownedADigest <- sha256File ownedA
  ownedBDigest <- sha256File ownedB
  timerADigest <- sha256File timerA
  timerBDigest <- sha256File timerB
  unless (sha256Hex loaderADigest == "21366ffff7cebc6176b8a8125bf3a9974835a51070a58ea325f7eed8aabd8882") (failH (loaderA ++ " is not the pinned X-M1 1.50 loader"))
  unless (sha256Hex loaderBDigest == "8fb0c322a22443a3783cdab9071db17212a5629f6375646218ac1e043dc672ec") (failH (loaderB ++ " is not the pinned X-M1 1.50 loader"))
  checkStorageBudget (takeDirectory resultsPath)
  createDirectoryIfMissing True cacheRoot
  createDirectoryIfMissing True (takeDirectory resultsPath)
  createDirectoryIfMissing True (takeDirectory reviewPath)
  let stockA = Image 0x40000 loaderABytes
      stockB = Image 0xd0000 loaderBBytes
      ownedAImage = Image baseA ownedABytes
      ownedBImage = Image baseB ownedBBytes
      timerAImage = Image 0x44a50 timerABytes
      timerBImage = Image 0xd4a10 timerBBytes
  aggregateA <- aggregateOf aggregateABin aggregateASymbols aggregateAPlacements
  aggregateB <- aggregateOf aggregateBBin aggregateBSymbols aggregateBPlacements
  (runA, controlsA) <- runDomain runner cacheRoot (takeDirectory resultsPath </> "domain-a") facts DomainA aggregateA stockA ownedAImage timerAImage symbolsA
  (runB, controlsB) <- runDomain runner cacheRoot (takeDirectory resultsPath </> "domain-b") facts DomainB aggregateB stockB ownedBImage timerBImage symbolsB
  let runs = runA ++ runB
      controls = controlsA ++ controlsB
      failures = [(domainLabel (runEntryDomain run), runName run, message) | run <- runs, message <- runDifferences run]
      failedChecks = [(domainLabel (runEntryDomain run), runName run, description) | run <- runs, (description, False) <- runChecks run]
      failedControls = [(domainLabel (controlDomain control), controlCase control, controlName control) | control <- controls, not (controlHolds control)]
      caseValue run =
        object
          [ "domain" .= domainLabel (runEntryDomain run)
          , "case" .= runName run
          , "blocking" .= runBlocking run
          , "differences" .= runDifferences run
          , "reuse" .= show (runReuse run)
          ]
      controlValue control =
        object
          [ "domain" .= domainLabel (controlDomain control)
          , "case" .= controlCase control
          , "control" .= controlName control
          , "baseline" .= controlBaseline control
          , "refused" .= controlRefused control
          , "guard_needed" .= controlGuardNeeded control
          , "holds" .= controlHolds control
          ]
      document =
        object
          [ "schema" .= (1 :: Int)
          , "qualification" .= ("native-subset" :: String)
          , "inputs" .=
              object
                [ "runner" .= runner
                , "runner_sha256" .= sha256Hex runnerDigest
                , "layout_sha256" .= sha256Hex layoutDigest
                , "loader_a_sha256" .= sha256Hex loaderADigest
                , "loader_b_sha256" .= sha256Hex loaderBDigest
                , "owned_a_bin_sha256" .= sha256Hex ownedADigest
                , "owned_b_bin_sha256" .= sha256Hex ownedBDigest
                , "timer_a_bin_sha256" .= sha256Hex timerADigest
                , "timer_b_bin_sha256" .= sha256Hex timerBDigest
                ]
          , "cases" .= map caseValue runs
          , "controls" .= map controlValue controls
          , "case_count" .= length runs
          , "blocking_count" .= length (filter runBlocking runs)
          , "control_count" .= length controls
          , "failures" .= (length failures + length failedChecks + length failedControls)
          ]
      review =
        object
          [ "schema" .= (1 :: Int)
          , "kind" .= ("data-queue-native-review" :: String)
          , "inputs" .= object
              [ "runner" .= runner
              , "runner_sha256" .= sha256Hex runnerDigest
              , "layout" .= layoutPath
              , "loader_a" .= loaderA
              , "loader_b" .= loaderB
              , "owned_a" .= ownedA
              , "owned_b" .= ownedB
              , "timer_a" .= timerA
              , "timer_b" .= timerB
              ]
          , "counterexamples" .=
              [ object ["domain" .= domain, "case" .= name, "difference" .= message]
              | (domain, name, message) <- failures
              ]
          , "controls" .= map controlValue controls
          , "qualification" .= ("native-subset" :: String)
          , "cases" .= map caseValue runs
          , "case_count" .= length runs
          , "blocking_count" .= length (filter runBlocking runs)
          , "control_count" .= length controls
          , "failures" .= (length failures + length failedChecks + length failedControls)
          ]
  BL.writeFile resultsPath (encode document)
  BL.writeFile reviewPath (encode review)
  forM_ runs $ \run -> forM_ (runDifferences run) $ \message ->
    putStrLn (domainLabel (runEntryDomain run) ++ " " ++ runName run ++ ": " ++ message)
  forM_ controls $ \control ->
    putStrLn (domainLabel (controlDomain control) ++ " " ++ controlCase control ++ ": control " ++ controlName control ++ (if controlHolds control then " ok" else " FAILED"))
  forM_ failures $ \(domain, name, message) -> putStrLn (domain ++ " " ++ name ++ ": " ++ message)
  forM_ failedChecks $ \(domain, name, description) -> putStrLn (domain ++ " " ++ name ++ ": check failed: " ++ description)
  forM_ failedControls $ \(domain, name, control) -> putStrLn (domain ++ " " ++ name ++ ": control failed: " ++ control)
  unless (null failures && null failedChecks && null failedControls) exitFailure

aggregateOf :: Maybe FilePath -> Maybe FilePath -> Maybe FilePath -> IO (Maybe Aggregate)
aggregateOf Nothing Nothing _ = pure Nothing
aggregateOf (Just binPath) (Just symbolsPath) placementsPath = do
  bytes <- B.readFile binPath
  symbols <- either failH pure . Agg.parseAggregateSymbols =<< B.readFile symbolsPath
  placements <- case placementsPath of
    Nothing -> pure []
    Just path -> either failH pure . Agg.parsePlacements =<< B.readFile path
  image <- either failH pure (Agg.aggregateImage symbols bytes)
  pure (Just (Aggregate image symbols placements))
aggregateOf _ _ _ = failH "an aggregate bin needs its symbols"

optional :: String -> IO (Maybe FilePath)
optional flag = do
  arguments <- getArgs
  case dropWhile (/= flag) arguments of
    [] -> pure Nothing
    [_] -> failH (flag ++ " needs a value")
    _ : value : _ -> Just <$> makeAbsolute value

parseOwnedSymbols :: FilePath -> IO [(Word32, Word32)]
parseOwnedSymbols path = do
  rows <- lines <$> readFile path
  pure (concatMap row rows)
  where
    row line = case words line of
      [a, z, kind, _] | kind == "T" || kind == "t" ->
        case (parseWord32 a, parseWord32 z) of
          (Just address, Just size) | size > 0 && address >= 0x40000 -> [(address, size)]
          _ -> []
      _ -> []

ownedBase :: FilePath -> IO Word32
ownedBase path = do
  rows <- lines <$> readFile path
  let addresses = [address | row <- rows, Just address <- [rowAddress row], address >= 0x40000, address < 0x01000000]
  case addresses of
    [] -> failH (path ++ " has no owned code symbols")
    _ -> pure (minimum addresses)
  where
    rowAddress row = case words row of
      [a, _, kind, _] | kind == "T" || kind == "t" -> parseWord32 a
      _ -> Nothing

required :: String -> IO FilePath
required flag = do
  arguments <- getArgs
  case dropWhile (/= flag) arguments of
    [] -> failH (flag ++ " is required")
    [_] -> failH (flag ++ " needs a value")
    _ : value : _ -> pure value
