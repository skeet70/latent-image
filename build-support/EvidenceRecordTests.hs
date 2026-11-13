{-# LANGUAGE OverloadedStrings #-}
module Main (main) where

import Control.Exception (IOException, try)
import Control.Monad (forM_, unless)
import Data.Aeson
import Data.Aeson.KeyMap qualified as KM
import Data.ByteString qualified as BS
import Data.ByteString.Lazy qualified as BL
import EvidenceRecord qualified
import System.Directory (createDirectoryIfMissing, removeFile)
import System.Environment (getEnv)
import System.FilePath ((</>))

assert :: Bool -> String -> IO ()
assert condition message = unless condition (fail message)

main :: IO ()
main = do
  root <- getEnv "TEST_TMPDIR"
  let result = "fixture/runs/one/result/summary.json"
      metadataPath = "fixture/runs/one/run.json"
      metadata = root </> metadataPath
      verdictPath = "fixture/runs/one/result/verdict.json"
      putJson path = BL.writeFile (root </> path) . encode
      invoke mode = EvidenceRecord.run [mode,root,"recipes.json","ledger.jsonl","README"]
      refuses mode = do
        outcome <- try (invoke mode) :: IO (Either IOException ())
        assert (either (const True) (const False) outcome) (mode <> " accepted stale or malformed evidence")
      marker = "<!-- evidence-record x-test -->\n\n<!-- /evidence-record x-test -->\n"
      unrelated = "{\"id\":\"x-other\",\"claim\":\"untouched\"}"
      recipe pointer = [object
        [ "id" .= ("x-test" :: String)
        , "claim" .= ("Cases {{cases}}; verdict {{verdict}}." :: String)
        , "scope" .= ("Host {{cases}} cases." :: String)
        , "notes" .= ("No hardware qualification." :: String)
        , "artifacts" .= [result]
        , "facts" .=
          [ object ["name" .= ("cases" :: String), "path" .= result, "pointer" .= pointer, "operation" .= ("length" :: String)]
          , object ["name" .= ("verdict" :: String), "path" .= result, "pointer" .= ["verdict" :: String]]
          , object ["name" .= ("firmware_sha256" :: String), "path" .= metadataPath, "pointer" .= ["firmware_sha256" :: String]]
          , object ["name" .= ("checks" :: String), "path" .= ("checks.log" :: String), "operation" .= ("decimal-between" :: String), "prefix" .= ("suite: " :: String), "suffix" .= (" checks passed" :: String)]
          ]
        ]]
      provenance = object
        [ "commit" .= replicate 40 'a', "firmware_sha256" .= replicate 64 'b'
        , "outcome" .= ("passed" :: String), "label" .= ("fixture-label" :: String)
        , "opened_at" .= ("2026-11-12T01:00:00Z" :: String)
        , "closed_at" .= ("2026-11-12T01:00:05Z" :: String)
        , "exit_status" .= (0 :: Int), "command" .= ["fixture", "two words", "single'quote", "" :: String]
        ]
      replace key value (Object o) = Object (KM.insert key value o)
      replace _ _ other = other
      readRecord = do
        bytes <- BS.readFile (root </> "ledger.jsonl")
        case BS.split 10 bytes of
          line:_ -> either fail pure (eitherDecodeStrict' line)
          [] -> fail "empty generated ledger"
      writeRecord value = BS.writeFile (root </> "ledger.jsonl") (BL.toStrict (encode value) <> "\n" <> unrelated <> "\n")
  createDirectoryIfMissing True (root </> "fixture/runs/one/result")
  BS.writeFile (root </> "checks.log") "suite: 42 checks passed\n"
  BS.writeFile (root </> "README") marker
  BS.writeFile (root </> "ledger.jsonl") ("{\"id\":\"x-test\"}\n" <> unrelated <> "\n")
  putJson "recipes.json" (recipe ["cases" :: String])
  putJson metadataPath provenance
  putJson result (object ["cases" .= [1 :: Int,2],"verdict" .= ("equal" :: String)])
  putJson verdictPath (object ["verdict" .= ("passed" :: String)])
  invoke "write"
  invoke "check"
  output <- BS.readFile (root </> "ledger.jsonl")
  assert (unrelated `elem` BS.split 10 output) "unrelated record changed"
  first <- readRecord
  case first of
    Object o -> do
      assert (KM.lookup "claim" o == Just (String "Cases 2; verdict equal.")) "claim not derived from result"
      assert (KM.lookup "commands" o == Just (toJSON ["run-output exec --label 'fixture-label' -- 'fixture' 'two words' 'single'\\''quote' ''" :: String])) "commands did not preserve argument boundaries"
    _ -> fail "record was not an object"
  writeRecord (replace "claim" (String "hand-edited claim") first)
  refuses "check"
  invoke "write"
  let editedFacts = object ["cases" .= (99 :: Int)]
  writeRecord (replace "figures" (object ["facts" .= editedFacts]) first)
  refuses "check"
  invoke "write"
  writeRecord (replace "commands" (toJSON ["hand-edited command" :: String]) first)
  refuses "check"
  invoke "write"
  forM_ [("outcome",String "open"),("commit",String "abc"),("closed_at",String "2026-11-12T00:59:00Z")] $ \(key,value) -> do
    putJson metadataPath (replace key value provenance)
    refuses "write"
  putJson metadataPath provenance
  readme <- BS.readFile (root </> "README")
  assert ("Cases 2; verdict equal." `BS.isInfixOf` readme) "README figures not generated"
  BS.writeFile (root </> "README") marker
  refuses "check"
  invoke "write"
  BS.appendFile (root </> result) " "
  refuses "check"
  invoke "write"
  BS.appendFile metadata " "
  refuses "check"
  invoke "write"
  putJson verdictPath (object ["verdict" .= ("failed" :: String)])
  refuses "check"
  invoke "write"
  putJson result (object ["cases" .= [1 :: Int],"verdict" .= ("different" :: String)])
  refuses "check"
  invoke "write"
  BS.appendFile (root </> "README") "<!-- evidence-record x-test -->"
  refuses "check"
  BS.writeFile (root </> "README") marker
  invoke "write"
  putJson "recipes.json" (recipe ["missing" :: String])
  refuses "write"
  putJson "recipes.json" ([] :: [Value])
  refuses "check"
  refuses "check-available"
  BS.writeFile (root </> "ledger.jsonl") (unrelated <> "\n")
  refuses "check"
  putJson "recipes.json" (recipe ["cases" :: String])
  writeRecord first
  invoke "write"
  removeFile metadata
  refuses "check-available"
  putJson metadataPath provenance
  removeFile (root </> result)
  refuses "check"
  refuses "check-available"
  removeFile metadata
  removeFile (root </> verdictPath)
  removeFile (root </> "checks.log")
  beforeUnavailable <- BS.readFile (root </> "ledger.jsonl")
  invoke "check-available"
  afterUnavailable <- BS.readFile (root </> "ledger.jsonl")
  assert (beforeUnavailable == afterUnavailable) "unavailable check rewrote the ledger"
  putStrLn "evidence-record behavior tests passed"
