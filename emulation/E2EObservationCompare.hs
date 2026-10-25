{-# LANGUAGE OverloadedStrings #-}
-- | Compares the observations of two receipt directories of one emulator route,
-- leaving out wall seconds, guest instruction counts and ticks, stack pointers, file paths,
-- frame numbers and capture PNG digests.
--
-- A RAM-push e2e directory compares every e2e-observation-N.json and the menu state of every
-- capture in e2e-result.json. A usb-toggle-emu directory (one holding summary.json) compares
-- each scenario's result and failed checks and its whole receipt.json, also leaving out the
-- engine name, the emulator's checkpoint identity, checkpoint directories and the access
-- probe's device-model transfers, which each engine's models make differently.
module Main (main) where

import Control.Monad (forM, unless)
import Data.Aeson (Value (..), eitherDecodeFileStrict', encode)
import qualified Data.Aeson.Key as K
import qualified Data.Aeson.KeyMap as KM
import qualified Data.ByteString.Lazy.Char8 as BL
import Data.List (isPrefixOf, isSuffixOf, sort)
import Data.Maybe (fromMaybe)
import qualified Data.Text as T
import qualified Data.Vector as V
import System.Directory (doesFileExist, listDirectory)
import System.Environment (getArgs, lookupEnv)
import System.Exit (exitFailure)
import System.FilePath ((</>))

ignored :: [K.Key]
ignored = ["seconds", "instructions", "virtual_ticks", "sp", "path", "frame_number", "sha256"]

toggleIgnored :: [K.Key]
toggleIgnored = ignored ++ ["engine", "emulator", "directory", "image_wide_transfers_covering_extent"]

strip :: [K.Key] -> Value -> Value
strip keys (Object fields) = Object (KM.map (strip keys) (KM.filterWithKey (\key _ -> key `notElem` keys) fields))
strip keys (Array items) = Array (V.map (strip keys) items)
strip _ value = value

-- | Paths below AT where the two values differ.
differences :: String -> Value -> Value -> [String]
differences at (Object a) (Object b) =
  concat [ differences (at ++ "." ++ K.toString key) (field a) (field b)
         | key <- sort (KM.keys (KM.union a b)), let field = fromMaybe Null . KM.lookup key ]
differences at (Array a) (Array b)
  | V.length a == V.length b = concat (zipWith3 (\index x y -> differences (at ++ "[" ++ show index ++ "]") x y) [0 :: Int ..] (V.toList a) (V.toList b))
differences at a b
  | a == b = []
  | otherwise = [at ++ ": " ++ short a ++ " != " ++ short b]
  where short = take 160 . BL.unpack . encode

load :: [K.Key] -> FilePath -> IO Value
load keys path = eitherDecodeFileStrict' path >>= either (ioError . userError . ((path ++ ": ") ++)) (pure . strip keys)

member :: K.Key -> Value -> Value
member key (Object fields) = fromMaybe Null (KM.lookup key fields)
member _ _ = Null

-- | RAM-push e2e: the observation files and the capture menu states.
compareE2E :: FilePath -> FilePath -> IO (Int, [String])
compareE2E old new = do
  names <- sort . filter (\name -> "e2e-observation-" `isPrefixOf` name && ".json" `isSuffixOf` name) <$> listDirectory old
  observed <- forM names $ \name -> do
    present <- doesFileExist (new </> name)
    if present then differences name <$> load ignored (old </> name) <*> load ignored (new </> name) else pure [name ++ ": missing"]
  let result = load ignored . (</> "e2e-result.json")
  captured <- differences "e2e-result.menu_receipts" <$> (member "menu_receipts" <$> result old) <*> (member "menu_receipts" <$> result new)
  pure (length names, concat observed ++ captured)

-- | usb-toggle-emu: the scenario list with results and failed checks, then each receipt.
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
