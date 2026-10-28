{-# LANGUAGE OverloadedStrings #-}
-- | Compares the observations of two receipt directories of one emulator route
-- ('Emulation.ObservationCompare' names the fields left out).
--
-- A RAM-push e2e directory compares every e2e-observation-N.json and the menu state of every
-- capture in e2e-result.json. A usb-toggle-emu directory (one holding summary.json) compares
-- each scenario's result and failed checks and its whole receipt.json.
module Main (main) where

import Control.Monad (forM, unless)
import Data.Aeson (Value (..), eitherDecodeFileStrict')
import qualified Data.Aeson.Key as K
import qualified Data.Aeson.KeyMap as KM
import Data.List (isPrefixOf, isSuffixOf, sort)
import Data.Maybe (fromMaybe)
import qualified Data.Text as T
import qualified Data.Vector as V
import Emulation.ObservationCompare (differences, e2eIgnored, normalize, toggleIgnored)
import System.Directory (doesFileExist, listDirectory)
import System.Environment (getArgs, lookupEnv)
import System.Exit (exitFailure)
import System.FilePath ((</>))

load :: [K.Key] -> FilePath -> IO Value
load keys path = eitherDecodeFileStrict' path >>= either (ioError . userError . ((path ++ ": ") ++)) (pure . normalize keys)

member :: K.Key -> Value -> Value
member key (Object fields) = fromMaybe Null (KM.lookup key fields)
member _ _ = Null

compareE2E :: FilePath -> FilePath -> IO (Int, [String])
compareE2E old new = do
  split <- doesFileExist (old </> "e2e-segments.json")
  if split then compareSegmented old new else compareFlat old new

segmentNamesOf :: FilePath -> IO [String]
segmentNamesOf directory = do
  parsed <- load [] (directory </> "e2e-segments.json")
  pure [ T.unpack name
       | Object entry <- V.toList (case member "segments" parsed of Array items -> items; _ -> V.empty)
       , String name <- [fromMaybe Null (KM.lookup "segment" entry)] ]

compareSegmented :: FilePath -> FilePath -> IO (Int, [String])
compareSegmented old new = do
  names <- segmentNamesOf old
  afterNames <- segmentNamesOf new
  let listed = differences "e2e-segments" (Array (V.fromList (map (String . T.pack) names))) (Array (V.fromList (map (String . T.pack) afterNames)))
  compared <- forM names $ \name -> do
    let before = old </> ("segment-" ++ name)
        after = new </> ("segment-" ++ name)
    present <- doesFileExist (after </> "e2e-result.json")
    if not present then pure [ "segment-" ++ name ++ ": missing" ] else do
      observed <- observationFiles before after
      captured <- differences ("segment-" ++ name ++ ".e2e-result.menu_receipts")
        <$> (member "menu_receipts" <$> load e2eIgnored (before </> "e2e-result.json"))
        <*> (member "menu_receipts" <$> load e2eIgnored (after </> "e2e-result.json"))
      pure (observed ++ captured)
  pure (length names, listed ++ concat compared)

observationFiles :: FilePath -> FilePath -> IO [String]
observationFiles before after = do
  names <- sort . filter (\name -> "e2e-observation-" `isPrefixOf` name && ".json" `isSuffixOf` name) <$> listDirectory before
  concat <$> forM names (\name -> do
    present <- doesFileExist (after </> name)
    if present then differences name <$> load e2eIgnored (before </> name) <*> load e2eIgnored (after </> name) else pure [name ++ ": missing"])

compareFlat :: FilePath -> FilePath -> IO (Int, [String])
compareFlat old new = do
  names <- sort . filter (\name -> "e2e-observation-" `isPrefixOf` name && ".json" `isSuffixOf` name) <$> listDirectory old
  observed <- observationFiles old new
  let result = load e2eIgnored . (</> "e2e-result.json")
  captured <- differences "e2e-result.menu_receipts" <$> (member "menu_receipts" <$> result old) <*> (member "menu_receipts" <$> result new)
  pure (length names, observed ++ captured)

compareToggle :: FilePath -> FilePath -> IO (Int, [String])
compareToggle old new = do
  let outcomes directory = do
        summary <- load toggleIgnored (directory </> "summary.json")
        pure [ (name, Object (KM.filterWithKey (\key _ -> key `elem` ["result", "failed_checks"]) fields))
             | Object fields <- V.toList (case member "scenarios" summary of Array items -> items; _ -> V.empty)
             , String name <- [fromMaybe Null (KM.lookup "scenario" fields)] ]
  before <- outcomes old
  after <- outcomes new
  let listed = differences "summary.scenarios" (Array (V.fromList (map (String . fst) before))) (Array (V.fromList (map (String . fst) after)))
      results = concat [ differences ("summary." ++ T.unpack name) outcome (fromMaybe Null (lookup name after)) | (name, outcome) <- before ]
  receipts <- forM (map fst before) $ \name -> do
    let file = T.unpack name </> "receipt.json"
    present <- doesFileExist (new </> file)
    if present then differences file <$> load toggleIgnored (old </> file) <*> load toggleIgnored (new </> file) else pure [file ++ ": missing"]
  pure (length before, listed ++ results ++ concat receipts)

main :: IO ()
main = do
  arguments <- getArgs
  (reference, candidate) <- case arguments of
    [a, b] -> pure (a, b)
    _ -> ioError (userError "usage: e2e-observation-compare RETAINED_RECEIPTS FUJIMOD_RECEIPTS")
  workspace <- lookupEnv "BUILD_WORKSPACE_DIRECTORY"
  let resolve path = if "/" `isPrefixOf` path then path else maybe path (</> path) workspace
      (old, new) = (resolve reference, resolve candidate)
  toggle <- doesFileExist (old </> "summary.json")
  (count, found) <- (if toggle then compareToggle else compareE2E) old new
  mapM_ (putStrLn . ("e2e-observation-compare: " ++)) found
  putStrLn ("e2e-observation-compare: " ++ show count ++ (if toggle then " scenario receipts, " else " observation files, ")
    ++ show (length found) ++ " differences")
  unless (null found) exitFailure
