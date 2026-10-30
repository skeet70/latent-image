-- The RAM-push pre-go gate over a payload's review record.
--   verdicts RECORD: every pin and the publication contract is approved by an
--     approving review, or content-checked (bank-content pins only).
--   bytes RECORD: every workspace pin's file, every built-artifact pin's file
--     and the builder and toolchain identities equal the recorded digests.
--     Private-data pins are compared by the relink inside the camera binary
--     and //transport:ram-push-reviewed-build-check.
module Main where

import Control.Exception (IOException, try)
import Control.Monad (forM, unless)
import qualified Data.ByteString as B
import Data.Maybe (catMaybes)
import System.Environment (getArgs)
import System.Exit (die, exitFailure)
import FujiAF.RamPushReview
import FujiAF.SHA256 (sha256)
import ClassicChromeRamBuilder (classicChromeRamBuilderDigest)
import ClassicChromeIconRamBuilder (classicChromeIconRamBuilderDigest)
import UsbToggleRamBuilder (usbToggleRamBuilderDigest)
import RamPushToolchain (ramPushToolchainDigest, recipeBankBuilderDigest)

data Gate = Verdicts | Bytes

currentDigest :: RecordKind -> Pin -> IO (Maybe GateFinding)
currentDigest kind pin = case pinSource pin of
  WorkspaceFile path -> fileDigest path
  BuilderSources -> pure (compareTo (case kind of
    BankD3Record -> recipeBankBuilderDigest
    ClassicChromeRecord -> classicChromeRamBuilderDigest
    ClassicChromeIconRecord -> classicChromeIconRamBuilderDigest
    UsbToggleRecord -> usbToggleRamBuilderDigest))
  Toolchain -> pure (compareTo ramPushToolchainDigest)
  PrivateFile _ -> pure Nothing
  UsbToggleStub -> fileDigest "host/firmware/usb-toggle-stub.bin"
  RustBankArchive -> fileDigest "replacement/bank/bank.a"
  RustBankBuildFacts -> fileDigest "replacement/bank/build-facts.txt"
  where
    fileDigest path = do
      bytes <- try (B.readFile path)
      pure $ case bytes of
        Left failure -> Just (BytesUnreadable (pinIdentity pin) (show (failure :: IOException)))
        Right contents -> compareTo (sha256 contents)
    compareTo current = case sha256Hex current of
      Nothing -> Just (BytesUnreadable (pinIdentity pin) ("not a SHA-256: "++current))
      Just digest
        | digest == pinDigest pin -> Nothing
        | otherwise -> Just (BytesDiffer (pinIdentity pin) (pinDigest pin) digest)

main :: IO ()
main = do
  arguments <- getArgs
  (gate, recordPath) <- case arguments of
    ["verdicts", path] -> pure (Verdicts, path)
    ["bytes", path] -> pure (Bytes, path)
    _ -> die "usage: ram-push-review-gate verdicts|bytes RECORD"
  record <- either (die . ((recordPath ++ ": ") ++)) pure . parseReviewRecord =<< B.readFile recordPath
  findings <- case gate of
    Verdicts -> pure (verdictFindings record)
    Bytes -> catMaybes <$> forM (recordPins record) (currentDigest (recordKind record))
  mapM_ (putStrLn . describeFinding) findings
  unless (null findings) $ do
    putStrLn (show (length findings) ++ " RAM-push review gate findings; ram-push is not ready for a go")
    exitFailure
  putStrLn "RAM-push review gate passed"
