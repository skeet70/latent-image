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

  let flashReceipt milestoneJson count = C.concat
        [ "{\"instructions\":", count, ",\"frame_sha256\":\"frame-a\",\"milestones\":", milestoneJson, "}" ]
      milestones = "[{\"phase\":\"language\",\"instructions\":100,\"frame_sha256\":\"language-a\"},{\"phase\":\"shooting-menu\",\"instructions\":200,\"frame_sha256\":\"menu-a\"}]"
      countOnly = "[{\"phase\":\"language\",\"instructions\":10,\"frame_sha256\":\"language-a\"},{\"phase\":\"shooting-menu\",\"instructions\":20,\"frame_sha256\":\"menu-a\"}]"
      reordered = "[{\"phase\":\"shooting-menu\",\"instructions\":20,\"frame_sha256\":\"menu-a\"},{\"phase\":\"language\",\"instructions\":10,\"frame_sha256\":\"language-a\"}]"
      flashPath = root </> "flash-boot-receipt.json"
  C.writeFile flashPath (flashReceipt milestones "300")
  flashBaseline <- observe flashBootSpec {observationExclusions = []} receipt
  C.writeFile flashPath (flashReceipt countOnly "30")
  flashCountsChanged <- observe flashBootSpec receipt
  C.writeFile flashPath (flashReceipt reordered "30")
  flashMilestonesReordered <- observe flashBootSpec receipt
  C.writeFile flashPath (flashReceipt "[]" "30")
  flashMilestonesMissing <- observe flashBootSpec receipt
  C.writeFile flashPath (flashReceipt "[{\"phase\":\"language\",\"instructions\":10,\"frame_sha256\":\"language-a\"},{\"phase\":\"shooting-menu\",\"instructions\":20,\"frame_sha256\":\"menu-b\"}]" "30")
  flashHashesChanged <- observe flashBootSpec receipt

  let traceSpec = ObservationSpec [filePattern "result.json"] (instructionTimingExclusions (filePattern "result.json"))
      tracePath = root </> "result.json"
  C.writeFile tracePath "{\"observations\":[{\"trace_hits\":[{\"instructions\":100,\"label\":\"press\"},{\"instructions\":200,\"label\":\"release\"}]}]}"
  traceBaseline <- observe traceSpec {observationExclusions = []} receipt
  C.writeFile tracePath "{\"observations\":[{\"trace_hits\":[{\"instructions\":10,\"label\":\"press\"},{\"instructions\":20,\"label\":\"release\"}]}]}"
  traceCountsChanged <- observe traceSpec receipt
  C.writeFile tracePath "{\"observations\":[{\"trace_hits\":[{\"instructions\":10,\"label\":\"release\"},{\"instructions\":20,\"label\":\"press\"}]}]}"
  traceOrderChanged <- observe traceSpec receipt

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
        (null (compareObservations (withoutExcluded spec recordedWithoutExclusions) excludedOnly))
    , check "flash instruction counts alone do not differ from an existing baseline"
        (null (compareObservations (withoutExcluded flashBootSpec flashBaseline) flashCountsChanged))
    , check "flash milestone order remains compared"
        (not (null (compareObservations (withoutExcluded flashBootSpec flashBaseline) flashMilestonesReordered)))
    , check "flash milestone set remains compared"
        (not (null (compareObservations (withoutExcluded flashBootSpec flashBaseline) flashMilestonesMissing)))
    , check "flash milestone hashes remain compared"
        (not (null (compareObservations (withoutExcluded flashBootSpec flashBaseline) flashHashesChanged)))
    , check "trace instruction counts alone do not differ from an existing baseline"
        (null (compareObservations (withoutExcluded traceSpec traceBaseline) traceCountsChanged))
    , check "trace event identity and order remain compared"
        (not (null (compareObservations (withoutExcluded traceSpec traceBaseline) traceOrderChanged))) ]
  unless (and outcomes) exitFailure
  putStrLn ("passed " ++ show (length outcomes) ++ " checks")
