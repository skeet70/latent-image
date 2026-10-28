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
import Data.List (isPrefixOf, isSuffixOf, nub, sort)
import Data.Maybe (fromMaybe)
import qualified Data.Text as T
import Control.Exception (IOException, try)
import qualified Data.ByteString as B
import qualified Data.Vector as V
import Emulation.SegmentReceipt (SegmentName, decodeDeclaredSegments, segmentNameText)
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
  beforeSplit <- doesFileExist (old </> "e2e-segments.json")
  afterSplit <- doesFileExist (new </> "e2e-segments.json")
  if beforeSplit || afterSplit then compareSegmented old new else compareFlat old new

segmentNamesOf :: FilePath -> IO (Either String [SegmentName])
segmentNamesOf directory = do
  loaded <- try (B.readFile (directory </> "e2e-segments.json")) :: IO (Either IOException B.ByteString)
  pure $ case loaded of
    Left problem -> Left ("e2e-segments.json: " ++ show problem)
    Right bytes -> decodeDeclaredSegments bytes

compareSegmented :: FilePath -> FilePath -> IO (Int, [String])
compareSegmented old new = do
  before <- segmentNamesOf old
  after <- segmentNamesOf new
  case (before, after) of
    (Left problem, _) -> pure (0, ["e2e-segments: " ++ problem])
    (_, Left problem) -> pure (0, ["e2e-segments: " ++ problem])
    (Right names, Right afterNames) -> do
      let listed = differences "e2e-segments" (Array (V.fromList (map String (map segmentNameText names))))
            (Array (V.fromList (map String (map segmentNameText afterNames))))
      compared <- forM names $ \name -> do
        let label = "segment-" ++ T.unpack (segmentNameText name)
            beforeDirectory = old </> label
            afterDirectory = new </> label
        present <- doesFileExist (afterDirectory </> "e2e-result.json")
        retained <- doesFileExist (beforeDirectory </> "e2e-result.json")
        if not present then pure (0, [label ++ "/e2e-result.json: missing from the candidate"])
        else if not retained then pure (0, [label ++ "/e2e-result.json: missing from the retained receipt"])
        else do
          (count, found) <- compareFlat beforeDirectory afterDirectory
          pure (count, map ((label ++ "/") ++) found)
      pure (sum (map fst compared), listed ++ concatMap snd compared)

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
