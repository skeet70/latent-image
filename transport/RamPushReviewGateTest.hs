-- | ram-push-review-gate-tests GATE
-- Runs the review gate's bytes mode in fixture trees holding the bank's
-- built Rust archive and build facts at their runfiles paths.
module Main where

import Control.Monad (forM_, unless)
import Data.IORef
import System.Directory (createDirectoryIfMissing, makeAbsolute)
import System.Environment (getArgs, getEnv)
import System.Exit (ExitCode (..), exitFailure)
import System.FilePath ((</>))
import System.Process (cwd, proc, readCreateProcessWithExitCode)

archiveBytes, changedArchiveBytes, factsBytes :: String
archiveBytes = "!<arch>\nfixture archive\n"
changedArchiveBytes = "!<arch>\nfixture archivf\n"
factsBytes = "rustc_version=fixture\n"

archiveDigest, factsDigest :: String
archiveDigest = "24647b1a4667a0f8551c3b27eb60ebc26eb40f02afefe933b2681670a8515e91"
factsDigest = "c758fe871befbba5fed0a0e5756cfd8d36fde35bb8f01b9f0e8798ba20a722be"

record :: String
record = concat
  [ "{\"schema\":1,\"kind\":\"xm1-ram-push-review-record\",\"reviews\":[],\"content_checks\":[],\"pins\":["
  , pin "rust_object_replacement_bank" "rust-bank-archive" "bank-content" archiveDigest, ","
  , pin "rust_toolchain_replacement_bank" "rust-bank-build-facts" "mechanism" factsDigest
  , "],\"publication_contract\":{\"sha256\":\"", replicate 64 'd', "\",\"status\":", unreviewed, "}}" ]
  where
    unreviewed = "{\"verdict\":\"unreviewed\",\"commit\":\"" ++ replicate 40 'b' ++ "\"}"
    pin identity source component digest = "{\"identity\":\"" ++ identity ++ "\",\"source\":\"" ++ source
      ++ "\",\"component\":\"" ++ component ++ "\",\"sha256\":\"" ++ digest ++ "\",\"status\":" ++ unreviewed ++ "}"

main :: IO ()
main = do
  [gateArgument] <- getArgs
  gate <- makeAbsolute gateArgument
  scratch <- getEnv "TEST_TMPDIR"
  failures <- newIORef (0 :: Int)
  let expect label ok = unless ok (putStrLn ("FAIL: " ++ label) >> modifyIORef' failures (+ 1))
      runCase name archive facts = do
        let root = scratch </> name
        createDirectoryIfMissing True (root </> "replacement" </> "bank")
        writeFile (root </> "record.json") record
        forM_ archive (writeFile (root </> "replacement" </> "bank" </> "bank.a"))
        forM_ facts (writeFile (root </> "replacement" </> "bank" </> "build-facts.txt"))
        (code, _, _) <- readCreateProcessWithExitCode (proc gate ["bytes", "record.json"]) { cwd = Just root } ""
        pure code

  matching <- runCase "matching" (Just archiveBytes) (Just factsBytes)
  expect ("matching built artifacts pass; got " ++ show matching) (matching == ExitSuccess)
  changedArchive <- runCase "changed-archive" (Just changedArchiveBytes) (Just factsBytes)
  expect ("a changed Rust archive is refused; got " ++ show changedArchive) (changedArchive == ExitFailure 1)
  missingFacts <- runCase "missing-facts" (Just archiveBytes) Nothing
  expect ("missing Rust build facts are refused; got " ++ show missingFacts) (missingFacts == ExitFailure 1)
  missingArchive <- runCase "missing-archive" Nothing (Just factsBytes)
  expect ("a missing Rust archive is refused; got " ++ show missingArchive) (missingArchive == ExitFailure 1)

  count <- readIORef failures
  unless (count == 0) exitFailure
  putStrLn "ram-push review gate: built Rust artifact checks passed"
