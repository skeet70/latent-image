{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}
module Main where

import Control.Exception (bracket)
import Control.Monad (forM_, unless)
import Data.Aeson
import qualified Data.ByteString.Lazy as BL
import Data.List (nub)
import qualified Data.Text as T
import Emulation.NativeEvidence
import Emulation.ParallelCases (mapBounded)
import Emulation.Sha256 (sha256, sha256Hex)
import FujiAF.Host (get, set, str)
import System.Directory
import System.FilePath
import System.IO (hClose, openTempFile)

ensure :: Bool -> String -> IO ()
ensure condition message = unless condition (fail message)

main :: IO ()
main = bracket temporary removeDirectoryRecursive $ \root -> do
  store <- newEvidenceStore CompactSuccesses
  first <- emit root store "first" caseFixture (report 1) passed
  ensure (first == report 1) "first diagnostic was not retained exactly"
  compact <- emit root store "repeat" caseFixture (report 100000) passed
  ensure (str (get compact "representation") == "compact-public-output-v1") "repeat was not compact"
  ensure (str (get compact "status") == "failed" && str (get compact "behavioral_status") == "passed") "raw and behavioral verdicts were conflated"
  ensure (get compact "mismatches" == get first "mismatches" && get compact "exclusions" == get first "exclusions") "diagnostic facts changed"
  ensure (get compact "case_sha256" == get first "case_sha256" && get compact "wire_sha256" == get first "wire_sha256") "input identities changed"
  let comparison = get compact "comparison"
      expectedHash = sha256Hex (sha256 "\xaa\xbb")
  ensure (str (get comparison "expected_r0") == "0x0000002a" && get comparison "observed_r0" == get comparison "expected_r0") "return facts changed"
  ensure (get comparison "memory" == toJSON [object
    ["region" .= ("public" :: T.Text), "offset" .= (1 :: Int), "length" .= (2 :: Int),
     "expected_sha256" .= expectedHash, "observed_sha256" .= expectedHash]]) "claimed output facts changed"
  ensure (get compact "runner" == Null) "compact evidence masquerades as a raw runner"
  exemplar <- readJson (str (get compact "full_diagnostic_exemplar"))
  ensure (exemplar == first) "exemplar does not resolve to the full diagnostic"
  writeEvidence store PublicSpans (root </> "first") caseFixture (report 2) passed
  retried <- readJson (root </> "first" </> "runner-report.json")
  ensure (retried == report 2) "representative retry replaced full diagnostics"
  small <- emit root store "small" caseFixture (report 1) passed
  ensure (BL.length (encode compact) == BL.length (encode small)) "raw arrays affected compact bytes"
  ensure (BL.length (encode compact) < 4096) "compact artifact is not bounded"
  other <- emit root store "other-class" caseFixture (set "mismatches" (toJSON (["stub events differ"] :: [T.Text])) (report 2)) passed
  ensure (get other "runner" /= Null) "distinct diagnostic class lost its exemplar"
  excluded <- emit root store "other-exclusion" caseFixture (set "exclusions" (toJSON (["different scope"] :: [T.Text])) (report 2)) passed
  ensure (get excluded "runner" /= Null) "distinct exclusion class lost its exemplar"
  let exactPass = set "status" (String "passed") (set "mismatches" (toJSON ([] :: [T.Text])) (report 1))
  _ <- emit root store "raw-pass-first" caseFixture exactPass passed
  exactCompact <- emit root store "raw-pass-repeat" caseFixture exactPass passed
  ensure (get exactCompact "status" == String "passed" && get exactCompact "runner" == Null) "raw pass repetition was not projected"
  forM_ (zip ["failed", "nonreturn", "error", "stderr", "schema", "wire", "exit", "changed-output"]
    [ (report 3, set "status" (String "failed") passed)
    , (runnerSet "status" (String "instruction_budget") (report 3), passed)
    , (runnerSet "unicorn_error" (toJSON (1 :: Int)) (report 3), passed)
    , (set "stderr" (String "synthetic runner warning") (report 3), passed)
    , (set "schema_version" (toJSON (2 :: Int)) (report 3), passed)
    , (runnerSet "wire_version" (toJSON (2 :: Int)) (report 3), passed)
    , (set "runner_exit" (toJSON (1 :: Int)) (report 3), passed)
    , (runnerSet "memory" (toJSON [object ["address" .= ("0x00002000" :: T.Text), "bytes" .= ("00000000" :: T.Text)]]) (report 3), passed)
    ]) $ \(name, (raw, behavior)) -> do
      kept <- emit root store name caseFixture raw behavior
      ensure (kept == raw) (name ++ " evidence was compacted")
  fullStore <- newEvidenceStore FullDiagnostics
  _ <- emit root fullStore "full-first" caseFixture (report 4) passed
  full <- emit root fullStore "full-repeat" caseFixture (report 5) passed
  ensure (full == report 5) "explicit full mode discarded diagnostics"
  direct <- emitScope root store ExactDiagnostics "direct" caseFixture (report 6) passed
  ensure (direct == report 6) "exact comparison scope was projected"
  parallelStore <- newEvidenceStore CompactSuccesses
  parallel <- mapBounded 8 (\i -> emit root parallelStore ("parallel-" ++ show i) caseFixture (report i) passed) ([1 .. 32] :: [Int])
  ensure (length [v | v <- parallel, get v "runner" /= Null] == 1) "parallel class did not retain exactly one representative"
  let references = nub [str (get v "full_diagnostic_exemplar") | v <- parallel, get v "runner" == Null]
  ensure (length references == 1) "parallel class references disagree"
  forM_ references $ \path -> do
    raw <- readJson path
    ensure (get raw "runner" /= Null) "parallel reference is not a full diagnostic"
  putStrLn "native evidence retention tests passed"

temporary :: IO FilePath
temporary = do
  root <- getTemporaryDirectory
  (path, handle) <- openTempFile root "native-evidence-tests"
  hClose handle
  removeFile path
  createDirectory path
  pure path

readJson :: FilePath -> IO Value
readJson path = BL.readFile path >>= either fail pure . eitherDecode

emit :: FilePath -> EvidenceStore -> FilePath -> Value -> Value -> Value -> IO Value
emit root store = emitScope root store PublicSpans

emitScope :: FilePath -> EvidenceStore -> ComparisonScope -> FilePath -> Value -> Value -> Value -> IO Value
emitScope root store scope name fixture raw behavior = do
  let dir = root </> name
  createDirectory dir
  BL.writeFile (dir </> "case.json") (encode fixture)
  BL.writeFile (dir </> "behavioral-result.json") (encode behavior)
  writeEvidence store scope dir fixture raw behavior
  descriptor <- readJson (dir </> "case.json")
  result <- readJson (dir </> "behavioral-result.json")
  ensure (descriptor == fixture && result == behavior) "retention changed descriptor or verdict"
  readJson (dir </> "runner-report.json")

runnerSet :: String -> Value -> Value -> Value
runnerSet key value raw = set "runner" (set key value (get raw "runner")) raw

passed :: Value
passed = object ["id" .= ("synthetic" :: T.Text), "status" .= ("passed" :: T.Text), "mismatches" .= ([] :: [T.Text])]

caseFixture :: Value
caseFixture = object
  [ "schema_version" .= (1 :: Int), "id" .= ("synthetic" :: T.Text)
  , "regions" .= [object ["id" .= ("public" :: T.Text), "address" .= ("0x00002000" :: T.Text)]]
  , "expected" .= object
      [ "status" .= ("returned" :: T.Text), "registers" .= object ["r0" .= ("0x0000002a" :: T.Text)]
      , "memory" .= [object ["region" .= ("public" :: T.Text), "offset" .= (1 :: Int), "hex" .= ("aabb" :: T.Text)]] ]
  ]

-- Synthetic public output with unrelated diagnostic arrays.
report :: Int -> Value
report count = object
  [ "schema_version" .= (1 :: Int), "status" .= ("failed" :: T.Text)
  , "mismatches" .= (["memory public: differs from exact allowed final bytes"] :: [T.Text])
  , "exclusions" .= (["public spans only"] :: [T.Text])
  , "case_sha256" .= T.replicate 64 "a", "wire_sha256" .= T.replicate 64 "b", "runner_sha256" .= T.replicate 64 "c"
  , "runner_exit" .= (0 :: Int)
  , "runner" .= object
      [ "wire_version" .= (1 :: Int), "status" .= ("returned" :: T.Text), "unicorn_error" .= (0 :: Int)
      , "registers" .= object ["r0" .= ("0x0000002a" :: T.Text)]
      , "memory" .= [object ["address" .= ("0x00002000" :: T.Text), "bytes" .= ("00aabbff" <> T.replicate count "dd")]]
      , "trace" .= replicate count (object ["pc" .= ("0x1000" :: T.Text)])
      , "events" .= replicate count (object ["address" .= ("0x3000" :: T.Text)])
      , "accesses" .= replicate count (object ["address" .= ("0x2000" :: T.Text)])
      , "memory_provenance" .= replicate count (object ["kind" .= ("read" :: T.Text)]) ]
  ]
