{-# LANGUAGE OverloadedStrings #-}
module Main (main) where

import Control.Monad (unless)
import Data.Aeson (Value (..))
import qualified Data.ByteString.Char8 as C
import qualified Data.Text as T
import Emulation.CandidateBaseline
import System.Directory (createDirectoryIfMissing, getTemporaryDirectory, removeDirectoryRecursive, removeFile)
import System.Environment (lookupEnv)
import System.Exit (exitFailure)
import System.FilePath ((</>))

spec :: ObservationSpec
spec = ObservationSpec
  (map filePattern ["result.json", "shots/*.png"])
  [ Exclusion (filePattern "result.json") (pointerPattern "/steps/*/seconds") Timing
  , Exclusion (filePattern "result.json") (pointerPattern "/start") CheckpointState ]

writeReceipt :: FilePath -> C.ByteString -> C.ByteString -> IO ()
writeReceipt root document png = do
  createDirectoryIfMissing True (root </> "shots")
  C.writeFile (root </> "result.json") document
  C.writeFile (root </> "shots" </> "menu.png") png
  C.writeFile (root </> "unobserved.log") "ignored"

result :: C.ByteString -> C.ByteString -> C.ByteString -> C.ByteString
result start seconds cursor = C.concat
  [ "{\"start\":\"", start, "\",\"steps\":[{\"key\":\"ok\",\"seconds\":", seconds
  , ",\"actual\":[1,5,", cursor, "]},{\"key\":\"down\",\"seconds\":0.5,\"actual\":[1,5,3]}]}" ]

check :: String -> Bool -> IO Bool
check name passed = do
  unless passed (putStrLn ("FAIL: " ++ name))
  pure passed

firstField :: [Difference] -> Maybe T.Text
firstField differences = case differences of
  first : _ -> Just (renderFieldName (differenceField first))
  [] -> Nothing

main :: IO ()
main = do
  scratch <- lookupEnv "TEST_TMPDIR" >>= maybe getTemporaryDirectory pure
  let root = scratch </> "candidate-baseline"
      receipt = ReceiptDirectory root
  createDirectoryIfMissing True root
  removeDirectoryRecursive root
  writeReceipt root (result "cold" "1.25" "2") "png-a"
  recorded <- observe spec receipt
  recordedWithoutExclusions <- observe spec {observationExclusions = []} receipt
  let baseline = Baseline "synthetic" "2026-10-24T00:00:00Z" root recorded
  stored <- either (\problem -> putStrLn problem >> exitFailure) pure (decodeBaseline (encodeBaseline baseline))

  writeReceipt root (result "restored" "9.75" "2") "png-a"
  excludedOnly <- observe spec receipt

  writeReceipt root (result "cold" "1.25" "4") "png-a"
  cursorMoved <- observe spec receipt

  writeReceipt root (result "cold" "1.25" "2") "png-b"
  pixelsChanged <- observe spec receipt

  removeFile (root </> "shots" </> "menu.png")
  screenshotMissing <- observe spec receipt

  outcomes <- sequence
    [ check "a stored baseline matches the run it was recorded from" (null (compareObservations (baselineFields stored) recorded))
    , check "unobserved files are not fields" (all ((/= "unobserved.log") . fieldFile . fieldName) recorded)
    , check "excluded timing and checkpoint fields do not differ" (null (compareObservations (baselineFields stored) excludedOnly))
    , check "a changed JSON leaf is named by file and pointer"
        (firstField (compareObservations (baselineFields stored) cursorMoved) == Just "result.json#/steps/0/actual/2")
    , check "a changed JSON leaf reports both values"
        (map differenceValue (compareObservations (baselineFields stored) cursorMoved) == [(Number 2, Number 4)])
    , check "a changed binary file is named by its path"
        (firstField (compareObservations (baselineFields stored) pixelsChanged) == Just "shots/menu.png")
    , check "a missing observed file is absent from the run"
        (case compareObservations (baselineFields stored) screenshotMissing of
           [AbsentFromRun name _] -> renderFieldName name == "shots/menu.png"
           _ -> False)
    , check "a field the baseline lacks is reported"
        (case compareObservations [field | field <- baselineFields stored, fieldFile (fieldName field) /= "shots/menu.png"] recorded of
           [AbsentFromBaseline name _] -> renderFieldName name == "shots/menu.png"
           _ -> False)
    , check "a baseline recorded before an exclusion compares under it"
        (null (compareObservations (withoutExcluded spec recordedWithoutExclusions) excludedOnly)) ]
  unless (and outcomes) exitFailure
  putStrLn ("passed " ++ show (length outcomes) ++ " checks")
