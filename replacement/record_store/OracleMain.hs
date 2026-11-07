{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}
module Main (main) where

import Control.DeepSeq (force)
import Control.Exception (evaluate)
import Control.Monad
import Data.Aeson (object, (.=))
import qualified Data.ByteString as B
import Data.List (isInfixOf, isPrefixOf)
import qualified Data.Map.Strict as M
import Emulation.NativeCase (loadCase)
import Emulation.NativeObservation
import Emulation.OwnedCase (caseId, loadObservedBatch, writeBatch)
import Emulation.Sha256
import FujiAF.JSON (encodeCanonical)
import Replacement.RecordStore.Cases
import Replacement.RecordStore.Execution
import Replacement.RecordStore.Hardware
import Replacement.RecordStore.Harness
import Replacement.RecordStore.Model
import System.Directory
import System.Environment
import System.Exit
import System.FilePath

chunks :: Int -> [a] -> [[a]]
chunks _ [] = []
chunks size items = let (first, rest) = splitAt size items in first : chunks size rest

main :: IO ()
main = do
  args <- getArgs
  case optional args "--native-jobs" of
    Nothing -> pure ()
    Just "1" -> pure ()
    Just _ -> error "record oracle executes one native job at a time"
  runner <- traverse makeAbsolute (optional args "--runner")
  outputPath <- required args "--out" >>= makeAbsolute
  candidate <- traverse makeAbsolute (optional args "--candidate")
  candidateSymbols <- traverse makeAbsolute (optional args "--symbols")
  cachedStock <- traverse makeAbsolute (optional args "--stock-output")
  cachedCandidate <- traverse makeAbsolute (optional args "--candidate-output")
  lookupEnv "BUILD_WORKSPACE_DIRECTORY" >>= mapM_ setCurrentDirectory
  runnerDigest <- traverse sha256File runner
  outputRoot <- canonicalizePath outputPath
  forM_ [root | Just root <- [cachedStock, cachedCandidate]] $ \root -> do
    cachedRoot <- canonicalizePath root
    when
      ( splitDirectories outputRoot `isPrefixOf` splitDirectories cachedRoot
          || splitDirectories cachedRoot `isPrefixOf` splitDirectories outputRoot
      )
      (error "output directory overlaps cached observations")
  createDirectoryIfMissing True outputPath
  image <- pinned (stockImagePath defaultStockPaths) "f2e2acab892dafeed85171f6870a411f0a2d5eae10921b86a24630d401dcff16"
  loader <- pinned (stockLoaderPath defaultStockPaths) "21366ffff7cebc6176b8a8125bf3a9974835a51070a58ea325f7eed8aabd8882"
  area2a <- pinned "private-data/sessions/20261009T030736Z-read-media/bank-0.bin" "1bedd3273d3fe7ec5a3b17d732b9f63e2420db61074290bf636a5b91bfa0c072"
  area2b <- pinned "private-data/sessions/20261009T030736Z-read-media/bank-1.bin" "924ed517da238a81c5e61b2ecd17ecb76250b67236921be7bdc711a4509635c7"
  area3 <- pinned "private-data/analysis/x-m1-1.50/native-case/area3-validator/record-accept.bin" "87b1e44d39a7ad591ccae07e953f6c8e11b2eb3e0e118eabac44bc2c0976f34b"
  ranges <- readFunctions (stockIndexPath defaultStockPaths)
  let recover c =
        c
          { name = CaseName (caseName (name c) ++ "-recover")
          , initialMedium = medium (persistence (expectedStore (model hardware c)))
          , operations = [Open, Register Normal, Latest]
          , injection = NoFault
          }
      recoveryCases = [recover c | c <- caseGrid, StopAfter _ <- [injection c]]
      allCases = caseGrid ++ recoveryCases ++ hardwareCases area2a area2b area3 ++ sequenceCases
      selected = maybe allCases (\pattern' -> filter (isInfixOf pattern' . caseName . name) allCases) (optional args "--filter")
      specs = map (caseSpec image loader ranges) selected
  when (null selected) (error "no cases selected")
  candidateInputs <- case (candidate, candidateSymbols) of
    (Nothing, Nothing) -> pure Nothing
    (Just bin, Just symbols) -> do
      bytes <- B.readFile bin
      functions <- symbolFunctions symbols
      unless (B.length bytes == 0x1e60) (error "candidate span size")
      pure (Just (bytes, functions))
    _ -> error "--candidate and --symbols are required together"
  when ("--control" `elem` args && candidateInputs == Nothing) (error "--control requires a candidate")
  when (cachedCandidate /= Nothing && candidateInputs == Nothing) (error "--candidate-output requires a candidate")
  cachedInputs <- cachedCaseInputs "stock" cachedStock
  cachedCandidateInputs <- cachedCaseInputs "candidate" cachedCandidate
  batches <- forM (zip [0 :: Int ..] (chunks 4 (zip selected specs))) $ \(batchIndex, rows) -> do
    let directory = outputPath </> "stock-" ++ show batchIndex
    batch <- writeBatch directory (map snd rows)
    patchStubs directory (map snd rows)
    candidateBatch <- forM candidateInputs $ \(bytes, functions) -> do
      let candidateSpecs = map (candidateSpec bytes functions . snd) rows
          candidateDirectory = outputPath </> "candidate-" ++ show batchIndex
      path <- writeBatch candidateDirectory candidateSpecs
      patchStubs candidateDirectory candidateSpecs
      pure (path, candidateSpecs)
    case runner of
      Nothing -> do
        let inputs = (directory, map snd rows) : [(takeDirectory path, candidateSpecs) | (path, candidateSpecs) <- maybe [] pure candidateBatch]
        forM_ inputs $ \(inputDirectory, inputSpecs) -> forM_ inputSpecs $ \spec -> void (loadCase (inputDirectory </> caseId spec ++ ".case.json"))
        pure []
      Just executable -> do
        observed <- case cachedStock of
          Nothing -> loadObservedBatch executable batch
          Just _ -> cachedObserved "stock" runnerDigest cachedInputs directory (map snd rows)
        candidateObserved <- case candidateBatch of
          Nothing -> pure M.empty
          Just (path, candidateSpecs) -> M.fromList <$> case cachedCandidate of
            Nothing -> loadObservedBatch executable path
            Just _ -> cachedObserved "candidate" runnerDigest cachedCandidateInputs (takeDirectory path) candidateSpecs
        compared <- forM (zip rows observed) $ \((c, _), (ident, observation)) -> do
          let differences = map modelDifferenceJson (modelDifferences c observation)
              candidateDifferences = maybe [] (map roleDifferenceJson . compareRoles observation) (M.lookup ident candidateObserved)
          evaluate
            ( force
                ( object
                    [ "case" .= caseName (name c)
                    , "stock_status" .= runnerStatusLabel (nativeStatus observation)
                    , "model_differences" .= differences
                    , "candidate_differences" .= candidateDifferences
                    ]
                , not (null differences)
                , not (null candidateDifferences)
                )
            )
        B.writeFile (outputPath </> "batch-" ++ show batchIndex ++ ".json") (encodeCanonical (object ["results" .= [row | (row, _, _) <- compared]]))
        pure compared
  let rows = concat batches
      modelFailures = length [() | (_, failed, _) <- rows, failed]
      candidateFailures = length [() | (_, _, failed) <- rows, failed]
      controlMode = "--control" `elem` args
      failures = modelFailures + if controlMode then (if candidateFailures == 0 then 1 else 0) else candidateFailures
      summary =
        object $
          [ "schema_version" .= (1 :: Int)
          , "profile" .= ("native-cortex-r5f" :: String)
          , "image4_sha256" .= sha256Hex (sha256 image)
          , "loader_sha256" .= sha256Hex (sha256 loader)
          , "candidate_sha256" .= fmap (sha256Hex . sha256 . fst) candidateInputs
          , "cases" .= length selected
          , "executed" .= length rows
          , "failed" .= failures
          , "model_failed" .= modelFailures
          , "candidate_failed" .= candidateFailures
          , "control" .= controlMode
          , "control_detected" .= (controlMode && candidateFailures > 0)
          , "candidate" .= maybe False (const True) candidateInputs
          , "results" .= [row | (row, _, _) <- rows]
          ]
            <> ["runner_sha256" .= fmap sha256Hex runnerDigest]
            <> ["cached_stock_output" .= cachedStock, "cached_candidate_output" .= cachedCandidate]
  B.writeFile (outputPath </> "summary.json") (encodeCanonical summary)
  putStrLn ("cases=" ++ show (length selected) ++ " executed=" ++ show (length rows) ++ " failed=" ++ show failures)
  when (failures > 0) exitFailure
  where
    optional arguments key = case dropWhile (/= key) arguments of _ : value : _ -> Just value; _ -> Nothing
