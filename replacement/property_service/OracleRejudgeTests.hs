{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}
module OracleRejudgeTests (main) where

import Control.Exception (IOException, bracket, try)
import Control.Monad (forM_, unless, void)
import Data.Aeson (Value (..), eitherDecodeStrict', object, (.=))
import qualified Data.Aeson.Key as K
import qualified Data.Aeson.KeyMap as KM
import qualified Data.ByteString as B
import Data.Either (isLeft)
import qualified Data.Text as T
import Emulation.OwnedCase (CaseId (..))
import qualified Emulation.NativeCase as NC
import qualified Emulation.NativeObservation as N
import Emulation.Sha256 (sha256, sha256Hex)
import FujiAF.JSON (encodeCanonical)
import FujiAF.Host (get)
import qualified OracleMain as Oracle
import System.Directory (Permissions (..), canonicalizePath, createDirectory, createDirectoryIfMissing, doesPathExist, getPermissions, getTemporaryDirectory, removeDirectoryRecursive, removeFile, setPermissions)
import System.FilePath (takeDirectory, (</>))
import System.IO (hClose, openTempFile)

check :: String -> Bool -> IO ()
check label passed = unless passed (fail label)

set :: String -> Value -> Value -> Value
set key value (Object fields) = Object (KM.insert (K.fromString key) value fields)
set _ _ _ = error "fixture object required"

text :: String -> Value
text = String . T.pack

registers :: Value
registers = object [K.fromString name .= value name | name <- NC.regs ++ ["pc"]]
  where
    value "lr" = text "0x00001004"
    value "sp" = text "0x00002008"
    value "cpsr" = text "0x00000010"
    value _ = text "0x00000000"

caseFixture :: Value
caseFixture = object
  [ "schema_version" .= (1 :: Int), "id" .= text "offline", "origin" .= text "synthetic"
  , "provenance" .= text "OracleRejudgeTests: frozen host observation"
  , "cpu" .= object ["architecture" .= text "arm", "model" .= text "cortex-r5f", "mode" .= text "arm", "endian" .= text "little"]
  , "artifacts" .= [object ["id" .= text "code", "path" .= text "code.bin", "size" .= (8 :: Int), "sha256" .= sha256Hex (sha256 (B.replicate 8 0))]]
  , "entry" .= text "0x00001000", "exit" .= text "0x00001004"
  , "registers" .= (case registers of Object fields -> Object (KM.delete "pc" fields); _ -> registers)
  , "regions" .= [region "code" "0x00001000" "rx", region "stack" "0x00002000" "rw"]
  , "functions" .= [object ["address" .= text "0x00001000", "size" .= (4 :: Int)]]
  , "stubs" .= ([] :: [Value]), "limits" .= object ["instructions" .= (10 :: Int), "wall_ms" .= (100 :: Int)]
  , "expected" .= object ["status" .= text "returned", "registers" .= object ["r0" .= text "0x00000000"], "memory" .= ([] :: [Value]), "events" .= ([] :: [Value])]
  , "exclusions" .= [text "host-only observation"]
  ]
  where
    region name address permissions = object
      [ "id" .= text name, "address" .= text address, "size" .= (8 :: Int), "permissions" .= text permissions
      , "initial" .= (if name == "code" then object ["artifact" .= text "code", "offset" .= (0 :: Int), "length" .= (8 :: Int)] else object ["zero" .= True])
      ]

main :: IO ()
main = do
  check "execution defaults unchanged" . (== Oracle.ExecuteNative) =<< Oracle.executionMode []
  forM_ [["--rejudge-run-root"], ["--rejudge-run-root", ".", "--cache", "."], ["--rejudge-run-root", ".", "--batch-checkpoint", "1"]] $ \arguments ->
    refused "offline mode refuses fallback options" (Oracle.executionMode arguments)
  withFixture $ \root runner inputs fresh -> do
    frozen <- loadFrozenRun root runner inputs
    observed <- observeFrozen frozen fresh [CaseId "offline"]
    check "frozen observation preserves caller-visible status" (map (N.nativeStatus . snd) observed == [N.Returned])
    original <- B.readFile (root </> "batch-00/offline.native-result.json")
    copied <- B.readFile (fresh </> "offline.native-result.json")
    check "physical report is unchanged" (original == copied)
    refused "wrong case order is refused" (observeFrozen frozen fresh [CaseId "other"])
    let names = map Oracle.FrozenBatchName ["batch-00", "batch-01"]
    refused "finish refuses incomplete consumption" (Oracle.finishFrozenRun frozen names)
    _ <- observeFrozen frozen (takeDirectory fresh </> "batch-01") [CaseId "offline"]
    Oracle.finishFrozenRun frozen names
    refused "finish refuses reordered batch inventory" (Oracle.finishFrozenRun frozen (reverse names))
    refused "finish refuses missing batch inventory" (Oracle.finishFrozenRun frozen (take 1 names))
    _ <- observeFrozen frozen fresh [CaseId "offline"]
    refused "finish refuses duplicate observation consumption" (Oracle.finishFrozenRun frozen names)
  forM_ ["case_sha256", "wire_sha256", "runner_sha256", "case_id"] $ \name ->
    withFixtureMutation (set name (text "wrong")) id $ \root runner inputs fresh -> do
      frozen <- loadFrozenRun root runner inputs
      refused ("wrong " ++ name ++ " is refused without execution") (observeFrozen frozen fresh [CaseId "offline"])
  withFixture $ \root runner inputs _ -> refused "wrong input identities are refused" (loadFrozenRun root runner (set "image4" (text "wrong") inputs))
  withFixture $ \root runner inputs _ -> do
    B.appendFile runner "changed"
    refused "changed actual runner is refused" (loadFrozenRun root runner inputs)
  withFixture $ \root runner inputs fresh -> do
    frozen <- loadFrozenRun root runner inputs
    B.appendFile (root </> "batch-00/offline.native-result.json") " "
    refused "wrong raw report hash is refused" (observeFrozen frozen fresh [CaseId "offline"])
  withFixture $ \root runner inputs _ -> do
    B.appendFile (root </> "batch-00/summary.json") " "
    refused "wrong source summary hash is refused" (loadFrozenRun root runner inputs)
  withFixture $ \root runner inputs _ -> do
    removeFile (root </> "batch-00/batch.json")
    refused "missing source batch is refused" (loadFrozenRun root runner inputs)
  withFixture $ \root runner inputs _ -> do
    B.writeFile (root </> "results.json") "{"
    refused "malformed source results are refused" (loadFrozenRun root runner inputs)
  withFixtureMutation id (set "memory_provenance_total" (text "invalid")) $ \root runner inputs fresh -> do
    frozen <- loadFrozenRun root runner inputs
    refused "malformed physical provenance is refused" (observeFrozen frozen fresh [CaseId "offline"])
  withFixture $ \root runner inputs fresh -> do
    frozen <- loadFrozenRun root runner inputs
    B.writeFile (fresh </> "offline.case.json") (encodeCanonical (set "provenance" (text "changed current case") caseFixture))
    refused "changed regenerated case is refused" (observeFrozen frozen fresh [CaseId "offline"])
  withFixture $ \root runner inputs fresh -> do
    frozen <- loadFrozenRun root runner inputs
    removeFile (root </> "batch-00/offline.native-result.json")
    refused "missing raw report is refused" (observeFrozen frozen fresh [CaseId "offline"])
  withFixture $ \root runner inputs _ ->
    refused "wrong external results trust anchor is refused" (Oracle.loadFrozenRun root runner inputs (sha256 "wrong external anchor"))
  withFixtureMutation (set "runner_exit" (Number 1)) id $ \root runner inputs fresh -> do
    frozen <- loadFrozenRun root runner inputs
    refused "nonzero returned runner exit is refused" (observeFrozen frozen fresh [CaseId "offline"])
  withFixtureMutation (const Null) id $ \root runner inputs fresh -> do
    frozen <- loadFrozenRun root runner inputs
    refused "authenticated malformed report is refused" (observeFrozen frozen fresh [CaseId "offline"])
  withFixture $ \root runner inputs _ -> do
    let relocatedRunner = runner ++ "-other"
    B.readFile runner >>= B.writeFile relocatedRunner
    refused "same runner bytes at a different actual path are refused" (loadFrozenRun root relocatedRunner inputs)
  withFixture $ \root runner _ fresh -> do
    let relocated = object ["image4" .= object ["path" .= text "relocated-identical-input", "sha256" .= text (replicate 64 'a')]]
    frozen <- loadFrozenRun root runner relocated
    observed <- observeFrozen frozen fresh [CaseId "offline"]
    check "identical input bytes may relocate" (map (N.nativeStatus . snd) observed == [N.Returned])
  withFixtureMutation (set "artifacts" (Array mempty)) id $ \root runner inputs fresh -> do
    frozen <- loadFrozenRun root runner inputs
    refused "report artifact inventory mismatch is refused" (observeFrozen frozen fresh [CaseId "offline"])
  withFixture $ \root runner inputs fresh -> do
    frozen <- loadFrozenRun root runner inputs
    B.writeFile (root </> "batch-00/code.bin") "corrupt!"
    refused "source artifact bytes mismatch is refused" (observeFrozen frozen fresh [CaseId "offline"])
  withFixture $ \root runner inputs fresh -> do
    frozen <- loadFrozenRun root runner inputs
    B.writeFile (root </> "batch-00/summary.json") "{}"
    B.writeFile (root </> "batch-00/batch.json") "{}"
    observed <- observeFrozen frozen fresh [CaseId "offline"]
    check "judgment uses the authenticated batch and summary snapshot" (map (N.nativeStatus . snd) observed == [N.Returned])
  withFixture $ \root runner inputs fresh -> do
    frozen <- loadFrozenRun root runner inputs
    let execution = Oracle.RejudgeBatch frozen
        reportPath = fresh </> "offline.native-result.json"
    _ <- Oracle.observeBatch execution fresh [CaseId "offline"]
    original <- B.readFile reportPath >>= either fail pure . eitherDecodeStrict'
    changedWrites <- case get (get original "runner") "memory_provenance" of
      Array writes -> pure (Array (fmap (set "value" (text "0x0000000000000001")) writes))
      _ -> fail "fixture must contain physical writes"
    let changed = set "runner" (set "memory_provenance" changedWrites (get original "runner")) original
    B.writeFile reportPath (encodeCanonical changed)
    refused "fresh report write-value tampering is refused before judgment" (Oracle.consumeReport execution fresh (CaseId "offline"))

observeFrozen :: Oracle.FrozenRun -> FilePath -> [CaseId] -> IO [(CaseId, N.NativeObservation)]
observeFrozen source directory identifiers = do
  let execution = Oracle.RejudgeBatch source
  (_, observations) <- Oracle.observeBatch execution directory identifiers
  forM_ identifiers (Oracle.consumeReport execution directory)
  pure observations

loadFrozenRun :: FilePath -> FilePath -> Value -> IO Oracle.FrozenRun
loadFrozenRun root runner inputs = do
  anchor <- sha256 <$> B.readFile (root </> "results.json")
  Oracle.loadFrozenRun root runner inputs anchor

refused :: String -> IO a -> IO ()
refused label action = do
  result <- try (void action) :: IO (Either IOException ())
  check label (isLeft result)

withFixture :: (FilePath -> FilePath -> Value -> FilePath -> IO ()) -> IO ()
withFixture = withFixtureMutation id id

withFixtureMutation :: (Value -> Value) -> (Value -> Value) -> (FilePath -> FilePath -> Value -> FilePath -> IO ()) -> IO ()
withFixtureMutation changeReport changeRunner action = bracket temporary removeDirectoryRecursive $ \base -> do
  let root = base </> "source"
      old = root </> "batch-00"
      secondOld = root </> "batch-01"
      fresh = base </> "fresh/batch-00"
      secondFresh = base </> "fresh/batch-01"
      runner = base </> "runner-never-executed"
      inputs = object ["image4" .= object ["path" .= text "canonical", "sha256" .= text (replicate 64 'a')]]
      write path = B.writeFile path . encodeCanonical
      digest = text . sha256Hex . sha256 . encodeCanonical
      batch = object ["schema_version" .= (1 :: Int), "cases" .= [object ["id" .= text "offline", "path" .= text "offline.case.json"]]]
  forM_ [old, secondOld, fresh, secondFresh] (createDirectoryIfMissing True)
  B.writeFile runner "#!/bin/sh\nprintf invoked > \"$0.invoked\"\nexit 99\n"
  permissions <- getPermissions runner
  setPermissions runner permissions {executable = True}
  runnerPath <- canonicalizePath runner
  runnerSha <- sha256Hex . sha256 <$> B.readFile runner
  forM_ [old, secondOld, fresh, secondFresh] $ \directory -> do
    B.writeFile (directory </> "code.bin") (B.replicate 8 0)
    write (directory </> "offline.case.json") caseFixture
    write (directory </> "batch.json") batch
  (_, _, _, identities) <- NC.loadCase (fresh </> "offline.case.json")
  let payload = changeRunner $ object
        [ "status" .= text "returned", "unicorn_error" .= (0 :: Int), "instructions" .= text "1"
        , "registers" .= registers
        , "memory" .= [object ["address" .= text address, "bytes" .= text "0000000000000000"] | address <- ["0x00001000", "0x00002000"]]
        , "memory_provenance_total" .= text "1"
        , "memory_provenance" .= [object ["kind" .= text "write", "address" .= text "0x00002000", "size" .= (1 :: Int), "value" .= text "0x0000000000000000"]]
        ]
      report = changeReport $ object ([K.fromString key .= value | (key, value) <- identities] ++
        ["case_id" .= text "offline", "runner_sha256" .= runnerSha, "runner_exit" .= (0 :: Int), "runner" .= payload])
      summary = object ["batch_sha256" .= digest batch, "cases" .= [object ["id" .= text "offline", "status" .= text "returned", "result_sha256" .= digest report]]]
      results = object
        [ "schema_version" .= (1 :: Int), "passed" .= False, "dat_retirement_admitted" .= False, "inputs" .= inputs
        , "runner" .= object ["path" .= runnerPath, "sha256" .= runnerSha]
        , "execution" .= object ["cases_executed" .= (2 :: Int), "cases_reused" .= (0 :: Int), "batches_executed" .= (2 :: Int), "batches_reused" .= (0 :: Int)]
        , "batches" .= [object ["name" .= text name, "cases" .= (1 :: Int), "batch_sha256" .= digest batch, "summary_sha256" .= digest summary] | name <- ["batch-00", "batch-01"]]
        , "qualification_scope" .= text "native-only", "qualification_status" .= text "unqualified", "native_image4_view" .= object []
        , "retired_fetch_guard" .= True, "coverage" .= text "host-control"
        , "wrappers" .= object [], "entry_adapters" .= object [], "case_shape" .= object [], "totals" .= object []
        , "model_agreement" .= object [], "pass2" .= object [], "status_distribution" .= object [], "controls" .= ([] :: [Value]), "differences" .= object []
        ]
  forM_ [old, secondOld] $ \directory -> do
    write (directory </> "offline.native-result.json") report
    write (directory </> "summary.json") summary
  write (root </> "results.json") results
  action root runner inputs fresh
  invoked <- doesPathExist (runner ++ ".invoked")
  check "offline success and refusals never invoke the external runner" (not invoked)
  where
    temporary = do
      parent <- getTemporaryDirectory
      (path, handle) <- openTempFile parent "property-offline-rejudge"
      hClose handle
      removeFile path
      createDirectory path
      pure path
