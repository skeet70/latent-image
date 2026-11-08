{-# LANGUAGE OverloadedStrings #-}
module Main (main) where

import Control.Monad (unless)
import Data.Aeson (Value, encode, object, (.=))
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as BL
import qualified Data.Text as T
import Emulation.ObservationCompare (compareObservationFiles, differences, e2eIgnored, normalize, toggleIgnored)
import Emulation.Sha256 (sha256, sha256Hex)
import System.Directory (createDirectoryIfMissing, getTemporaryDirectory, removeDirectoryRecursive)
import System.Exit (exitFailure)
import System.FilePath (takeFileName, (</>))
import System.Posix.Process (getProcessID)

segment :: [T.Text] -> Value
segment entries = object ["segment" .= ("cc-film-list" :: T.Text), "classic_chrome_site_entries" .= entries]

compared :: Value -> Value -> [String]
compared a b = differences "observation" (normalize e2eIgnored a) (normalize e2eIgnored b)

-- Synthetic sequences use the X-M1 Classic Chrome trace-site labels.
builderPass :: [T.Text]
builderPass = ["cc-entry-marker", "cc-entry-builder"]

idlePass :: [T.Text]
idlePass = ["cc-entry-marker", "cc-hook-colour", "cc-hook-tone-1", "cc-hook-tone-2", "cc-hook-chroma-2"]

-- | The same pass without its chroma hook.
shortPass :: [T.Text]
shortPass = ["cc-entry-marker", "cc-hook-colour", "cc-hook-tone-1", "cc-hook-tone-2"]

-- | One settings sync: two converter entries.
sync :: [T.Text]
sync = ["cc-entry-settings-converter", "cc-entry-settings-converter"]

capture :: [T.Text]
capture = ["cc-entry-capture-converter"]

cases :: [(String, Bool)]
cases =
  [ ( "converter-free segments with different entries differ"
    , not (null (compared (segment builderPass) (segment (builderPass ++ ["cc-entry-builder"])))) )
  , ( "converter-free segments with equal entries are equal"
    , null (compared (segment builderPass) (segment builderPass)) )
  , ( "converter-free segments keep their pass count"
    , not (null (compared (segment idlePass) (segment (idlePass ++ idlePass)))) )
  , ( "a converter-free segment differs from one that converts"
    , not (null (compared (segment idlePass) (segment (idlePass ++ sync)))) )
  , ( "a sync before the passes equals the sync inside a pass"
    , null (compared (segment (sync ++ idlePass ++ idlePass)) (segment (idlePass ++ idlePass ++ sync))) )
  , ( "two syncs before any pass equal two syncs in separate passes"
    , null (compared (segment (sync ++ sync ++ idlePass ++ shortPass ++ idlePass))
                     (segment (idlePass ++ sync ++ idlePass ++ sync ++ shortPass ++ idlePass))) )
  , ( "repeated identical passes count once"
    , null (compared (segment (idlePass ++ sync ++ idlePass)) (segment (idlePass ++ idlePass ++ idlePass ++ sync))) )
  , ( "a missing converter entry differs"
    , not (null (compared (segment (idlePass ++ sync ++ sync)) (segment (idlePass ++ sync ++ ["cc-entry-settings-converter"])))) )
  , ( "an extra sync differs"
    , not (null (compared (segment (idlePass ++ sync)) (segment (idlePass ++ sync ++ idlePass ++ sync)))) )
  , ( "converter entries in a different order differ"
    , not (null (compared (segment (idlePass ++ capture ++ sync)) (segment (idlePass ++ sync ++ capture)))) )
  , ( "a pass with a different hook set differs"
    , not (null (compared (segment (idlePass ++ sync)) (segment (shortPass ++ sync)))) )
  , ( "passes in a different order differ"
    , not (null (compared (segment (idlePass ++ sync ++ shortPass)) (segment (shortPass ++ sync ++ idlePass)))) )
  , ( "a non-converter entry beside a sync differs"
    , not (null (compared (segment (idlePass ++ sync)) (segment (idlePass ++ ["cc-entry-builder"] ++ sync)))) )
  , ( "a non-converter entry before the first marker differs"
    , not (null (compared (segment (sync ++ idlePass)) (segment (["cc-entry-builder"] ++ sync ++ idlePass)))) )
  , ( "timing fields are left out"
    , null (compared (object ["instructions" .= (1 :: Int), "r0" .= ("0x1" :: T.Text)])
                     (object ["instructions" .= (2 :: Int), "r0" .= ("0x1" :: T.Text)])) )
  , ( "a register value is compared"
    , not (null (compared (object ["r0" .= ("0x1" :: T.Text)]) (object ["r0" .= ("0x2" :: T.Text)]))) )
  , ( "a menu receipt with scanout evidence equals one without it"
    , null (compared
        (object ["menu" .= [1, 254, 0 :: Int], "scanout_evidence" .= object ["source_bytes" .= object ["bytes" .= (2073600 :: Int)]]])
        (object ["menu" .= [1, 254, 0 :: Int]])) )
  , ( "menu receipts with different menu states still differ beside scanout evidence"
    , not (null (compared
        (object ["menu" .= [1, 254, 0 :: Int], "scanout_evidence" .= object ["source_bytes" .= object ["bytes" .= (2073600 :: Int)]]])
        (object ["menu" .= [1, 5, 6 :: Int]]))) )
  , ( "toggle receipts leave out the engine and checkpoint identity"
    , null (differences "receipt"
        (normalize toggleIgnored (object ["engine" .= ("retained" :: T.Text), "emulator" .= ("a" :: T.Text), "result" .= ("pass" :: T.Text)]))
        (normalize toggleIgnored (object ["engine" .= ("fujimod" :: T.Text), "emulator" .= ("b" :: T.Text), "result" .= ("pass" :: T.Text)]))) )
  , ( "toggle receipts compare results"
    , not (null (differences "receipt"
        (normalize toggleIgnored (object ["result" .= ("pass" :: T.Text)]))
        (normalize toggleIgnored (object ["result" .= ("blocked" :: T.Text)])))) )
  ]

row :: T.Text -> T.Text -> Value
row site memory = object
  [ "site" .= site, "core" .= (0 :: Int), "instructions" .= (10 :: Int)
  , "r0" .= ("0x1" :: T.Text), "r1" .= ("0x2" :: T.Text), "r2" .= ("0x3" :: T.Text), "r3" .= ("0x4" :: T.Text)
  , "lr" .= ("0x5" :: T.Text), "sp" .= ("0x6" :: T.Text), "memory" .= [memory]
  , "render_object_provenance_sequence" .= (7 :: Int) ]

observation :: [Value] -> Value
observation hits = object ["phase" .= ("replay" :: T.Text), "reads" .= ([] :: [Value]), "trace_hits" .= hits]

sidecarObservation :: FilePath -> [Value] -> IO Value
sidecarObservation path hits = do
  let bytes = BL.toStrict (BL.intercalate "\n" (map encode hits) <> "\n")
  B.writeFile path bytes
  pure (object
    [ "phase" .= ("replay" :: T.Text), "reads" .= ([] :: [Value])
    , "trace_hits" .= object
        [ "schema" .= (1 :: Int), "path" .= takeFileName path, "sha256" .= sha256Hex (sha256 bytes), "count" .= length hits ] ])

writeObservation :: FilePath -> Value -> IO ()
writeObservation path value = B.writeFile path (BL.toStrict (encode value) <> "\n")

rowCases :: IO Bool
rowCases = do
  temporary <- getTemporaryDirectory
  pid <- getProcessID
  let root = temporary </> ("observation-compare-rows-" ++ show pid)
  createDirectoryIfMissing True root
  let base = row "render-pane-entry" "01020304"
      other = row "render-pane-entry" "05060708"
      inlinePath = root </> "inline.json"
      sidecarPath = root </> "sidecar.json"
      renamedPath = root </> "renamed.json"
      changedPath = root </> "changed.json"
      badDigestPath = root </> "bad-digest.json"
      badCountPath = root </> "bad-count.json"
  writeObservation inlinePath (observation [base, base])
  sidecar <- sidecarObservation (root </> "a.trace-hits.jsonl") [base, base]
  writeObservation sidecarPath sidecar
  renamed <- sidecarObservation (root </> "b.trace-hits.jsonl") [base, base]
  writeObservation renamedPath renamed
  changed <- sidecarObservation (root </> "changed.trace-hits.jsonl") [base, other]
  writeObservation changedPath changed
  let validDigest = sha256Hex (sha256 (BL.toStrict (BL.intercalate "\n" (map encode [base, base]) <> "\n")))
      ref digest count = object ["schema" .= (1 :: Int), "path" .= ("a.trace-hits.jsonl" :: T.Text), "sha256" .= digest, "count" .= count]
  writeObservation badDigestPath (object ["phase" .= ("replay" :: T.Text), "reads" .= ([] :: [Value]), "trace_hits" .= ref (T.replicate 64 "0") (2 :: Int)])
  writeObservation badCountPath (object ["phase" .= ("replay" :: T.Text), "reads" .= ([] :: [Value]), "trace_hits" .= ref validDigest (1 :: Int)])
  results <- mapM (\(label, left, right) -> do
    found <- compareObservationFiles left right "observation"
    pure (label, found)) ([
      ("inline equals sidecar", inlinePath, sidecarPath)
    , ("equal rows under a different sidecar name", sidecarPath, renamedPath)
    , ("changed read byte fails", sidecarPath, changedPath)
    , ("bad sidecar digest fails", sidecarPath, badDigestPath)
    , ("bad sidecar count fails", sidecarPath, badCountPath)
    ] :: [(String, FilePath, FilePath)])
  mapM_ (\(label, found) -> putStrLn (label ++ ": " ++ show (null found) ++ (if null found then "" else " " ++ show found))) results
  removeDirectoryRecursive root
  pure (all check results)
 where
  check ("inline equals sidecar", found) = null found
  check ("equal rows under a different sidecar name", found) = null found
  check (_, found) = not (null found)
main :: IO ()
main = do
  let failed = [name | (name, ok) <- cases, not ok]
  mapM_ (putStrLn . ("FAIL: " ++)) failed
  unless (null failed) exitFailure
  rows <- rowCases
  unless rows exitFailure
  putStrLn (show (length cases) ++ " observation-compare cases and 5 row-reference cases passed")
