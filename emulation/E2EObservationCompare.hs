{-# LANGUAGE OverloadedStrings #-}
-- | Compares the observations of two RAM-push e2e receipt directories: every
-- e2e-observation-N.json and the menu state of every capture in e2e-result.json, leaving out
-- wall seconds, guest instruction counts, stack pointers, file paths, frame numbers and capture
-- PNG digests, and comparing Classic Chrome site entries as develop passes (see 'passes').
module Main (main) where

import Control.Monad (forM, unless)
import Data.Aeson (Value (..), eitherDecodeFileStrict', encode)
import qualified Data.Aeson.Key as K
import qualified Data.Aeson.KeyMap as KM
import qualified Data.ByteString.Lazy.Char8 as BL
import Data.List (dropWhileEnd, isPrefixOf, isSuffixOf, sort)
import qualified Data.List.NonEmpty as NE
import Data.Maybe (fromMaybe)
import qualified Data.Text as T
import qualified Data.Vector as V
import System.Directory (doesFileExist, listDirectory)
import System.Environment (getArgs, lookupEnv)
import System.Exit (exitFailure)
import System.FilePath ((</>))

ignored :: [K.Key]
ignored = ["seconds", "instructions", "sp", "path", "frame_number", "sha256"]

-- | A segment's Classic Chrome site entries as live-view develop passes, each starting at
-- `cc-entry-marker`. Consecutive identical passes collapse to one, and the passes before the
-- first and after the last pass that enters `cc-entry-settings-converter` are dropped.
passes :: [T.Text] -> [[T.Text]]
passes entries = trim (map NE.head (NE.group (split entries)))
  where
    split [] = []
    split (first : rest) = let (pass, more) = break (== "cc-entry-marker") rest in (first : pass) : split more
    converts = elem "cc-entry-settings-converter"
    trim = dropWhileEnd (not . converts) . dropWhile (not . converts)

strip :: Value -> Value
strip (Object fields) = Object (KM.mapWithKey site (KM.filterWithKey (\key _ -> key `notElem` ignored) fields))
  where
    site "classic_chrome_site_entries" (Array items) | Just names <- mapM text (V.toList items) =
      Array (V.fromList [Array (V.fromList (map String pass)) | pass <- passes names])
    site _ value = strip value
    text (String name) = Just name
    text _ = Nothing
strip (Array items) = Array (V.map strip items)
strip value = value

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

load :: FilePath -> IO Value
load path = eitherDecodeFileStrict' path >>= either (ioError . userError . ((path ++ ": ") ++)) (pure . strip)

main :: IO ()
main = do
  arguments <- getArgs
  (reference, candidate) <- case arguments of
    [a, b] -> pure (a, b)
    _ -> ioError (userError "usage: e2e-observation-compare RETAINED_RECEIPTS FUJIMOD_RECEIPTS")
  workspace <- lookupEnv "BUILD_WORKSPACE_DIRECTORY"
  let resolve path = if "/" `isPrefixOf` path then path else maybe path (</> path) workspace
      (old, new) = (resolve reference, resolve candidate)
  names <- sort . filter (\name -> "e2e-observation-" `isPrefixOf` name && ".json" `isSuffixOf` name) <$> listDirectory old
  observed <- forM names $ \name -> do
    present <- doesFileExist (new </> name)
    if present then differences name <$> load (old </> name) <*> load (new </> name) else pure [name ++ ": missing"]
  let captures result = case result of
        Object fields -> fromMaybe Null (KM.lookup "menu_receipts" fields)
        _ -> Null
  captured <- differences "e2e-result.menu_receipts" <$> (captures <$> load (old </> "e2e-result.json")) <*> (captures <$> load (new </> "e2e-result.json"))
  let found = concat observed ++ captured
  mapM_ (putStrLn . ("e2e-observation-compare: " ++)) found
  putStrLn ("e2e-observation-compare: " ++ show (length names) ++ " observation files, " ++ show (length found) ++ " differences")
  unless (null found) exitFailure
