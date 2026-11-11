{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}

-- | Runs the static suite audit: entries bound, entries entered, and whether
-- the stock role maps the firmware's bytes at every entered entry.
module Main (main) where

import Control.Monad (forM, unless)
import Data.Aeson (encode)
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as BL
import Data.List ((\\))
import Emulation.Sha256 (sha256, sha256Hex)
import Emulation.SuiteAudit
import FujiAF.Host (failH)
import System.Directory (doesFileExist)
import System.Environment (getArgs, lookupEnv)
import System.Exit (exitFailure)
import System.FilePath ((</>))

main :: IO ()
main = do
  arguments <- getArgs
  manifestPath <- required "--manifest" arguments
  workspace <- lookupEnv "BUILD_WORKSPACE_DIRECTORY"
  root <- maybe (maybe "." id workspace) id <$> optional "--root" arguments
  firmwareOverride <- optional "--firmware" arguments
  jsonPath <- optional "--json" arguments
  let only = values "--suite" arguments
  manifestBytes <- B.readFile manifestPath
  Manifest suites <- either failH pure (parseManifest manifestBytes)
  let selected = if null only then suites else filter (\suite -> suiteName suite `elem` only) suites
  unless (null only || length selected == length only)
    (failH ("unknown suite selector: " ++ show (only \\ map suiteName selected)))
  override <- case firmwareOverride of
    Nothing -> pure Nothing
    Just path -> (\bytes -> Just (path, bytes)) <$> B.readFile path
  let overrideBytes = fmap snd override
  audits <- forM selected $ \suite -> do
    bound <- either failH pure =<< readBoundEntries root (suiteBound suite)
    caseBytes <- B.readFile (root </> suiteCasesPath suite)
    rows <- either failH pure (parseCaseTable (suiteName suite) caseBytes)
    gaps <- case suiteGapsPath suite of
      Nothing -> pure []
      Just path -> do
        exists <- doesFileExist (root </> path)
        if exists then either failH pure . parseGaps =<< B.readFile (root </> path) else pure []
    firmware <- case overrideBytes of
      Just bytes -> pure (Just bytes)
      Nothing -> case suiteImage suite of
        Nothing -> pure Nothing
        Just image -> do
          exists <- doesFileExist (root </> imagePath image)
          if not exists then pure Nothing else Just <$> B.readFile (root </> imagePath image)
    case firmware of
      Just bytes -> case suiteImage suite of
        Just image -> case imageSha256 image of
          Just expected | expected /= sha256Hex (sha256 bytes) ->
            failH ("firmware digest differs for " ++ suiteName suite ++ ": " ++ sha256Hex (sha256 bytes) ++ " is not " ++ expected)
          _ -> pure ()
        Nothing -> pure ()
      Nothing -> pure ()
    let named = case firmware of
          Nothing -> Nothing
          Just bytes -> case (override, suiteImage suite) of
            (Just (path, _), _) -> Just (path, bytes)
            (_, Just image) -> Just (imagePath image, bytes)
            _ -> Just ("(override)", bytes)
    pure (auditOneSuite named suite bound rows gaps)
  putStr (auditTable audits)
  case jsonPath of
    Nothing -> pure ()
    Just path -> BL.writeFile path (encode (auditsJson audits))
  unless (all ((/= AuditFailed) . auditStatus) audits) exitFailure

required :: String -> [String] -> IO String
required flag arguments = case values flag arguments of
  [] -> failH (flag ++ " is required")
  values' -> pure (last values')

optional :: String -> [String] -> IO (Maybe String)
optional flag arguments = case values flag arguments of
  [] -> pure Nothing
  values' -> pure (Just (last values'))

values :: String -> [String] -> [String]
values flag arguments = [value | (key, value) <- zip arguments (drop 1 arguments), key == flag]
