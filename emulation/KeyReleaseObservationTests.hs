{-# LANGUAGE OverloadedStrings #-}
module Main (main) where

import Control.Exception (IOException, try)
import Control.Monad (unless)
import Data.Aeson (Value, eitherDecodeStrict', object, (.=))
import qualified Data.ByteString.Char8 as C
import Emulation.KeyReleaseObservation
import System.Directory (createDirectoryIfMissing, getTemporaryDirectory)
import System.Environment (lookupEnv)
import System.Exit (exitFailure)
import System.FilePath ((</>))

check :: String -> Bool -> IO Bool
check name passed = do
  unless passed (putStrLn ("FAIL: " ++ name))
  pure passed

main :: IO ()
main = do
  scratch <- lookupEnv "TEST_TMPDIR" >>= maybe getTemporaryDirectory pure
  let root = scratch </> "key-release-observations"
      logPath = root </> "events.jsonl"
      receiptPath = root </> "receipt.json"
      tableEvent = "{\"route\":\"ram-push-cc\",\"segment\":\"cc-film-list-open\",\"key\":\"ok\",\"row\":18,\"cap\":6000000,\"instructions_used\":6000594,\"instructions\":514905236}"
      dialEvent = "{\"route\":\"ram-push-cc\",\"segment\":null,\"key\":\"dial-left\",\"row\":null,\"cap\":1000000,\"instructions_used\":1000000,\"instructions\":515905236}"
  createDirectoryIfMissing True root
  C.writeFile logPath (tableEvent <> "\n" <> dialEvent <> "\n")
  observations <- readReleaseObservationLog logPath
  writeReleaseObservationReceipt receiptPath observations
  receipt <- eitherDecodeStrict' <$> C.readFile receiptPath
  let expected = do
        table <- eitherDecodeStrict' tableEvent
        dial <- eitherDecodeStrict' dialEvent
        pure (object
          [ "schema" .= (1 :: Int)
          , "scope" .= ("incomplete debounced key releases; observational, not an acceptance gate" :: String)
          , "incomplete_releases" .= ([table, dial] :: [Value])
          ])
  retained <- readReleaseObservationReceipt receiptPath
  absent <- readReleaseObservationLog (root </> "no-events.jsonl")
  C.writeFile logPath "{\"route\":null,\"segment\":null,\"key\":\"ok\",\"row\":null,\"cap\":1,\"instructions_used\":1,\"instructions\":1}\n"
  missingRow <- try (readReleaseObservationLog logPath) :: IO (Either IOException [KeyReleaseObservation])
  C.writeFile logPath "{\"route\":null,\"segment\":null,\"key\":\"ok\",\"row\":18,\"cap\":1,\"instructions_used\":-1,\"instructions\":1}\n"
  negativeInstructions <- try (readReleaseObservationLog logPath) :: IO (Either IOException [KeyReleaseObservation])
  outcomes <- sequence
    [ check "receipt preserves all incomplete-release fields and nullable segment/row" (receipt == expected)
    , check "each logged event survives receipt serialization" (length retained == 2)
    , check "a session with no events has no incomplete releases" (null absent)
    , check "a table key cannot be recorded without its row" (case missingRow of Left _ -> True; Right _ -> False)
    , check "instruction counts cannot be negative" (case negativeInstructions of Left _ -> True; Right _ -> False)
    ]
  unless (and outcomes) exitFailure
  putStrLn ("passed " ++ show (length outcomes) ++ " checks")
