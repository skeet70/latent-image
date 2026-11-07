{-# LANGUAGE OverloadedStrings #-}
module Main (main) where

import Control.Exception (bracket)
import Control.Monad (unless)
import Data.Aeson (FromJSON (parseJSON), eitherDecode, withObject, (.:), (.:?), (.!=))
import qualified Data.ByteString.Char8 as BC
import qualified Data.ByteString.Lazy as BL
import Data.List (isPrefixOf)
import System.Directory (canonicalizePath, createDirectory, createDirectoryIfMissing, getTemporaryDirectory, removeDirectoryRecursive, removeFile)
import System.Environment (getArgs)
import System.Exit (ExitCode (..))
import System.FilePath ((</>), takeDirectory)
import System.IO (hClose, openTempFile)
import System.Process (readProcessWithExitCode)

data Usage = Usage {uInput :: Integer, uOutput :: Integer, uCacheRead :: Integer, uCacheWrite :: Integer, uTotal :: Integer}
  deriving (Eq, Show)

instance FromJSON Usage where
  parseJSON = withObject "usage" $ \o -> Usage <$> o .: "input" <*> o .: "output" <*> o .: "cacheRead" <*> o .: "cacheWrite" <*> o .: "total"

data Counts = Counts {cMessages :: Integer, cUsage :: Usage, cGrants :: Integer}
  deriving (Eq, Show)

instance FromJSON Counts where
  parseJSON = withObject "counts" $ \o -> Counts <$> o .: "assistantMessages" <*> o .: "usage" <*> o .: "grants"

data Coverage = Coverage {covTranscripts :: Integer, covSkipped :: Integer, covAuxiliary :: Integer, covDuplicates :: Integer}
  deriving (Eq, Show)

instance FromJSON Coverage where
  parseJSON = withObject "coverage" $ \o -> Coverage <$> o .: "transcriptsRead" <*> o .: "malformedLinesSkipped" <*> o .: "auxiliaryUsageRecordsIgnored" <*> o .: "duplicateRecordsDropped"

data Increments = Increments {iRequested :: Integer, iTokens :: Double, iGrants :: Double}
  deriving (Eq, Show)

instance FromJSON Increments where
  parseJSON = withObject "increments" $ \o ->
    Increments
      <$> o .:? "requested" .!= 0
      <*> o .:? "tokensPerIncrement" .!= 0
      <*> o .:? "grantsPerIncrement" .!= 0

data Report = Report {rParent :: Counts, rChildren :: Counts, rCombined :: Counts, rCoverage :: Coverage, rIncrements :: Increments}

instance FromJSON Report where
  parseJSON = withObject "report" $ \o -> Report <$> o .: "parent" <*> o .: "children" <*> o .: "combined" <*> o .: "coverage" <*> o .: "increments"

check :: Bool -> String -> IO ()
check ok message = unless ok (ioError (userError message))

put :: FilePath -> String -> IO ()
put path text = createDirectoryIfMissing True (takeDirectory path) >> writeFile path text

assistant :: String -> String -> String -> String -> String -> String -> String -> String
assistant recordId responseId timestamp input output cacheRead cacheWrite =
  "{\"type\":\"message\",\"id\":\"" ++ recordId ++ "\",\"timestamp\":\"" ++ timestamp ++ "\",\"message\":{\"role\":\"assistant\",\"responseId\":\"" ++ responseId ++ "\",\"usage\":{\"input\":" ++ input ++ ",\"output\":" ++ output ++ ",\"cacheRead\":" ++ cacheRead ++ ",\"cacheWrite\":" ++ cacheWrite ++ ",\"totalTokens\":999999}}}"

userMessage :: String -> String -> String -> String -> String
userMessage recordId timestamp attribution text =
  "{\"type\":\"message\",\"id\":\"" ++ recordId ++ "\",\"timestamp\":\"" ++ timestamp ++ "\",\"message\":{\"role\":\"user\",\"attribution\":\"" ++ attribution ++ "\",\"content\":[{\"type\":\"text\",\"text\":\"" ++ text ++ "\"}]}}"

parentFixture :: String
parentFixture =
  unlines
    [ assistant "m1" "r1" "2026-10-01T00:00:00.000Z" "10" "20" "100" "5"
    , assistant "m1" "r1" "2026-10-01T00:00:00.000Z" "10" "20" "100" "5"
    , assistant "m2" "r2" "2026-10-01T00:10:00.000Z" "1" "2" "3" "4"
    , "{\"type\":\"model_usage\",\"id\":\"u1\",\"timestamp\":\"2026-10-01T00:10:01.000Z\",\"purpose\":\"task-completion-probe\",\"usage\":{\"input\":1000,\"output\":2000,\"cacheRead\":3000,\"cacheWrite\":4000}}"
    , "{\"type\":\"message\",\"id\":\"m3\",\"timestamp\":\"2026-10-01T00:20:00.000Z\",\"message\":{\"role\":\"assistant\",\"responseId\":\"r3\",\"stopReason\":\"aborted\",\"usage\":{\"input\":5,\"output\":6,\"cacheRead\":7,\"cacheWrite\":8}}}"
    , userMessage "m4" "2026-10-01T00:30:00.000Z" "user" "Granted: one host build may run now."
    , userMessage "m5" "2026-10-01T00:31:00.000Z" "user" "We discussed whether a grant would be needed later."
    , userMessage "m6" "2026-10-01T00:32:00.000Z" "user" "No grant: do not run the emulator until I say so."
    , "{\"type\":\"message\",\"id\":\"bad\",\"message\":"
    ]

childFixture :: String
childFixture =
  unlines
    [ assistant "c0" "r1" "2026-10-01T01:00:00.000Z" "10" "20" "100" "5"
    , assistant "c1" "r10" "2026-10-01T01:00:00.000Z" "100" "200" "300" "400"
    , userMessage "c2" "2026-10-01T01:05:00.000Z" "agent" "[Wait interrupted by message]\\n<irc from=\\\"parent\\\" agent=\\\"Main\\\">\\nExplicit host build grant: run the emulator now.\\n</irc>"
    , userMessage "c3" "2026-10-01T01:06:00.000Z" "agent" "A grant was discussed but nothing was approved."
    ]

nestedFixture :: String
nestedFixture = assistant "d1" "r20" "2026-10-01T02:00:00.000Z" "7" "8" "9" "10"

beforeMarker :: String -> String -> String
beforeMarker marker = go
 where
  go text
    | marker `isPrefixOf` text = ""
    | otherwise = case text of
        [] -> ""
        character : rest -> character : go rest

run :: FilePath -> [String] -> IO Report
run executable arguments = do
  (code, out, err) <- readProcessWithExitCode executable arguments ""
  check (code == ExitSuccess) ("model-usage failed: " ++ show code ++ " " ++ err)
  let json = BL.fromStrict (BC.pack (beforeMarker "\nmodel-usage: " out))
  case eitherDecode json of
    Left message -> ioError (userError ("cannot decode report: " ++ message))
    Right report -> pure report

main :: IO ()
main = do
  [argument] <- getArgs
  executable <- canonicalizePath argument
  base <- getTemporaryDirectory
  bracket (do (name, handle) <- openTempFile base "model-usage-tests"; hClose handle; removeFile name; createDirectory name; canonicalizePath name) removeDirectoryRecursive $ \workspace -> do
    let session = workspace </> "2026-10-01T00-00-00-000Z_session.jsonl"
        childDirectory = workspace </> "2026-10-01T00-00-00-000Z_session"
    put session parentFixture
    put (childDirectory </> "Child.jsonl") childFixture
    put (childDirectory </> "Child" </> "Deep.jsonl") nestedFixture
    report <- run executable [session]
    let parent = rParent report
        children = rChildren report
        combined = rCombined report
    check (parent == Counts 3 (Usage 16 28 110 17 171) 1) ("parent counts " ++ show parent)
    check (children == Counts 2 (Usage 107 208 309 410 1034) 1) ("child counts " ++ show children)
    check (combined == Counts 5 (Usage 123 236 419 427 1205) 2) ("combined counts " ++ show combined)
    check (rCoverage report == Coverage 3 1 1 2) ("coverage " ++ show (rCoverage report))
    check (rIncrements report == Increments 0 0 0) ("increments without the flag " ++ show (rIncrements report))
    halved <- run executable ["--increments", "2", session]
    check (rIncrements halved == Increments 2 602.5 1.0) ("increments divided by two " ++ show (rIncrements halved))
    fifths <- run executable ["--increments", "5", session]
    check (close 241.0 (iTokens (rIncrements fifths)) && close 0.4 (iGrants (rIncrements fifths)) && iRequested (rIncrements fifths) == 5) ("increments divided by five " ++ show (rIncrements fifths))
    windowed <- run executable ["--since", "2026-10-01T00:15:00Z", session]
    check (rParent windowed == Counts 1 (Usage 5 6 7 8 26) 1) ("windowed parent counts " ++ show (rParent windowed))
  putStrLn "model-usage behavioral tests passed"

close :: Double -> Double -> Bool
close expected actual = abs (expected - actual) < 1.0e-9
