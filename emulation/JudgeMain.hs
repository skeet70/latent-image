{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}

-- | Turn a comparison report into a verdict: report path and firmware image
-- in, verdict document out.
module Main (main) where

import Control.Exception (IOException, try)
import Data.Aeson (encode)
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as BL
import Emulation.Judge
import Emulation.Sha256 (sha256, sha256Hex)
import Emulation.SuiteAudit
import FujiAF.Host (failH)
import System.Directory (doesFileExist)
import System.Environment (getArgs, lookupEnv)
import System.Exit (exitWith)
import System.FilePath ((</>))
import System.IO (hPutStrLn, stderr)

main :: IO ()
main = do
  arguments <- getArgs
  reportPath <- required "--report" arguments
  imagePath <- required "--image" arguments
  workspace <- lookupEnv "BUILD_WORKSPACE_DIRECTORY"
  root <- maybe (maybe "." id workspace) id <$> optional "--root" arguments
  manifestPath <- maybe (pure (root </> "emulation" </> "suite-audit.json")) pure =<< optional "--manifest" arguments
  outPath <- optional "--out" arguments
  reportBytes <- B.readFile reportPath
  report <- either failH pure (parseReport reportBytes)
  suiteFlag <- optional "--suite" arguments
  selectedSuite <- case (suiteFlag, reportSuite report) of
    (Just name, _) -> pure name
    (Nothing, Just name) -> pure name
    _ -> failH "--suite is required when the report names no suite"
  Manifest suites <- either failH pure . parseManifest =<< B.readFile manifestPath
  suite <- case [spec | spec <- suites, suiteName spec == selectedSuite] of
    [spec] -> pure spec
    [] -> failH ("unknown suite " ++ show selectedSuite)
    _ -> failH ("suite " ++ show selectedSuite ++ " is listed twice")
  bound <- either failH pure =<< readBoundEntries root (suiteBound suite)
  firmware <- do
    exists <- doesFileExist imagePath
    if not exists then pure Nothing else do
      bytes <- try (B.readFile imagePath) :: IO (Either IOException B.ByteString)
      pure $ case bytes of
        Left _ -> Nothing
        Right imageBytes -> Just (Firmware imagePath (sha256Hex (sha256 imageBytes)))
  case (suiteImage suite, firmware) of
    (Just declared, Just supplied) -> case imageSha256 declared of
      Just expected | expected /= firmwareDigest supplied ->
        failH ("supplied image " ++ firmwarePath supplied ++ " is not the suite's declared image " ++ expected)
      _ -> pure ()
    _ -> pure ()
  let verdict = judgeReport
        JudgeInput
          { inputSuite = selectedSuite
          , inputBound = bound
          , inputFirmware = firmware
          , inputReportDigest = sha256Hex (sha256 reportBytes)
          }
        report
  case outPath of
    Nothing -> BL.putStr (encode (verdictJson verdict))
    Just path -> BL.writeFile path (encode (verdictJson verdict))
  hPutStrLn stderr (verdictText verdict)
  exitWith (verdictExitCode (verdictStatus verdict))

required :: String -> [String] -> IO String
required flag arguments = case values flag arguments of
  [] -> failH (flag ++ " is required")
  found -> pure (last found)

optional :: String -> [String] -> IO (Maybe String)
optional flag arguments = case values flag arguments of
  [] -> pure Nothing
  found -> pure (Just (last found))

values :: String -> [String] -> [String]
values flag arguments = [value | (key, value) <- zip arguments (drop 1 arguments), key == flag]
