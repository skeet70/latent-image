{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}

-- | Synthetic tests for the shared judge. They exercise the verdict rule
-- without a suite run, an image or a native execution.
module Main (main) where

import Control.Monad (unless)
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as BC
import Emulation.Judge
import Emulation.SuiteAudit (MissingEntry (..))
import System.Directory (getTemporaryDirectory, removeFile)
import System.Exit (ExitCode (..), exitFailure)
import System.FilePath ((</>))
import Data.Word (Word32)

main :: IO ()
main = do
  let bound = [MissingEntry 0x10 4 "entry_10", MissingEntry 0x20 4 "entry_20"]
      firmware = Just (Firmware "image.bin" "aaaa")
      complete = report
        { reportImageDigests = ["aaaa"]
        , reportOwnedDigest = Just "bbbb"
        , reportCases =
            [ caseRow "c1" (Just 0x10) (Just 3) [] []
            , caseRow "c2" (Just 0x20) (Just 5) [] []
            ]
        , reportControls = [control "wrong" True [] []]
        }
      judge = judgeReport (JudgeInput "synthetic" bound firmware "report-digest")

  check "complete report is equal" (verdictStatus (judge complete) == VerdictEqual)
  check "equal verdict names the firmware" (verdictFirmware (judge complete) == Just "aaaa")
  check "equal verdict names the owned image" (verdictOwned (judge complete) == Just "bbbb")
  check "equal verdict names the report digest" (verdictReportDigest (judge complete) == "report-digest")
  check "equal verdict names the judge version" (judgeVersion == "6")
  check "equal verdict lists every condition met" (all conditionMet (verdictConditions (judge complete)))
  check "equal verdict has no unmet condition" (null (verdictUnmet (judge complete)))

  -- (a) stock evidence for every case.
  let noStock = complete {reportCases = [caseRow "c1" (Just 0x10) (Just 0) [] [], caseRow "c2" (Just 0x20) (Just 5) [] []]}
      missingStock = complete {reportCases = [caseRow "c1" (Just 0x10) Nothing [] [], caseRow "c2" (Just 0x20) (Just 5) [] []]}
  check "a case with no stock instruction is no-verdict" (verdictStatus (judge noStock) == VerdictNoVerdict)
  check "the case with no stock instruction is named" (any ("c1" `isInfix`) (verdictUnmet (judge noStock)))
  check "a case with no stock count is no-verdict" (verdictStatus (judge missingStock) == VerdictNoVerdict)
  check "the no-stock condition is the unmet one" (unmetNames (judge noStock) == [StockInsideEntered])

  let emptyReport = complete {reportCases = []}
  check "a report with no case is no-verdict" (verdictStatus (judge emptyReport) == VerdictNoVerdict)
  check "a report with no case names the stock condition" (StockInsideEntered `elem` unmetNames (judge emptyReport))

  -- (b) a case for every export row.
  let uncovered = complete {reportCases = [caseRow "c1" (Just 0x10) (Just 3) [] []]}
  check "a missing export row is no-verdict" (verdictStatus (judge uncovered) == VerdictNoVerdict)
  check "the missing export row is named" (any ("0x00000020" `isInfix`) (verdictUnmet (judge uncovered)))
  check "the coverage condition is the unmet one" (unmetNames (judge uncovered) == [CaseForEveryExport])

  -- (c) a detected wrong variant, with survivors listed.
  let undetected = complete {reportControls = [control "wrong" False ["r0"] []]}
  check "an undetected wrong variant is no-verdict" (verdictStatus (judge undetected) == VerdictNoVerdict)
  check "the undetected wrong variant is a survivor" (verdictSurvivors (judge undetected) == ["wrong"])
  check "the control condition is the unmet one" (unmetNames (judge undetected) == [DetectedWrongVariant])
  let noControls = complete {reportControls = []}
  check "a report with no wrong variant is no-verdict" (verdictStatus (judge noControls) == VerdictNoVerdict)
  check "a report with no wrong variant names the condition" (unmetNames (judge noControls) == [DetectedWrongVariant])

  -- (d) the digests.
  let noOwned = complete {reportOwnedDigest = Nothing}
      noReportFirmware = complete {reportImageDigests = []}
      wrongFirmware = complete {reportImageDigests = ["cccc"]}
  check "a report without the owned digest is no-verdict" (verdictStatus (judge noOwned) == VerdictNoVerdict)
  check "the digest condition is the unmet one" (unmetNames (judge noOwned) == [NamedDigests])
  check "a report without a firmware digest is no-verdict" (verdictStatus (judge noReportFirmware) == VerdictNoVerdict)
  check "a report for another firmware is no-verdict" (verdictStatus (judge wrongFirmware) == VerdictNoVerdict)
  check "a report for another firmware names the named digests" (any ("cccc" `isInfix`) (verdictUnmet (judge wrongFirmware)))

  -- A report may name several images; the declared firmware is the one that
  -- matches the supplied image.
  let multi = judgeReport (JudgeInput "synthetic" bound (Just (Firmware "image5.bin" "3cd5")) "report-digest") (complete {reportImageDigests = ["f2e2", "3cd5"]})
  check "the declared firmware among several named digests is found" (verdictStatus multi == VerdictEqual)
  check "the verdict names the supplied firmware" (verdictFirmware multi == Just "3cd5")

  -- Without the firmware the comparison is not run.
  let unfed = judgeReport (JudgeInput "synthetic" bound Nothing "report-digest") complete
  check "no firmware is not-run" (verdictStatus unfed == VerdictNotRun)
  check "not-run is not a pass" (verdictExitCode (verdictStatus unfed) /= ExitSuccess)

  -- A receipt that records one wrong variant as an exit code is a control.
  receipt <- either fail pure (parseReport "{\"firmware_sha256\":\"aaaa\",\"owned_sha256\":\"bbbb\",\"mutant_exit\":1,\"mutant_failure\":\"first pair differs\",\"cases\":[{\"id\":\"c1\",\"entry\":\"0x10\",\"stock_instructions_inside\":2},{\"id\":\"c2\",\"entry\":\"0x20\",\"stock_instructions_inside\":2}]}")
  check "a mutant receipt is a detected control" (map reportControlDetected (reportControls receipt) == [True])
  check "a mutant receipt judges equal" (verdictStatus (judge receipt) == VerdictEqual)

  -- A case difference is different, with the cases named.
  let differs = complete {reportCases = [caseRow "c1" (Just 0x10) (Just 3) ["return r0"] [], caseRow "c2" (Just 0x20) (Just 5) [] []]}
  check "a differing case is different" (verdictStatus (judge differs) == VerdictDifferent)
  check "the differing case is named" (verdictDifferentCases (judge differs) == ["c1"])

  -- A diagnostic difference is listed and does not change the verdict.
  let diagnostic = complete {reportCases = [caseRow "c1" (Just 0x10) (Just 3) [] ["decoder state"], caseRow "c2" (Just 0x20) (Just 5) [] []]}
  check "a diagnostic difference keeps the verdict equal" (verdictStatus (judge diagnostic) == VerdictEqual)
  check "a diagnostic difference is printed" (verdictDiagnostics (judge diagnostic) == ["c1: decoder state"])
  check "a diagnostic difference is not a case difference" (null (verdictDifferentCases (judge diagnostic)))

  -- A cutover report declares no milestone: it compares every case.
  check "a native report carrying a milestone is refused"
    (case parseReport (BC.pack "{\"milestone\":{\"name\":\"machine-7\",\"observation\":2}}") of Left _ -> True; Right _ -> False)
  check "a native report with a null milestone is read"
    (case parseReport (BC.pack "{\"milestone\":null}") of Right _ -> True; Left _ -> False)

  -- Only equal exits zero.
  check "equal exits zero" (verdictExitCode VerdictEqual == ExitSuccess)
  check "equal-up-to-milestone does not exit zero" (verdictExitCode VerdictEqualUpToMilestone /= ExitSuccess)
  check "different does not exit zero" (verdictExitCode VerdictDifferent /= ExitSuccess)
  check "no-verdict does not exit zero" (verdictExitCode VerdictNoVerdict /= ExitSuccess)
  check "not-run does not exit zero" (verdictExitCode VerdictNotRun /= ExitSuccess)

  -- The parser reads the shape the suites write, including replies and the
  -- digest names older reports use.
  parsed <- either fail pure (parseReport lensShaped)
  check "the parser reads the suite name" (reportSuite parsed == Just "lens-actor")
  check "the parser reads the firmware digest" ("f2e2" `elem` reportImageDigests parsed)
  check "the parser reads the owned digest" (reportOwnedDigest parsed == Just "4cb9")
  check "the parser reads cases and replies" (length (reportCases parsed) == 2)
  check "the parser reads a case entry" (map reportCaseEntry (reportCases parsed) == [Just 0x6c27f8, Just 0x6c4c94])
  check "the parser reads the stock instruction count" (map reportCaseStockInside (reportCases parsed) == [Just 27, Just 190])
  check "the parser reads a detected control" (map reportControlDetected (reportControls parsed) == [True])
  check "the parser reads the case differences" (map reportCaseDifferences (reportCases parsed) == [[], []])
  check "the parser keeps missing fields missing" (map reportCaseStockInside (reportCases (either (const report) id (parseReport "{\"cases\":[{\"id\":\"c\"}]}"))) == [Nothing])

  -- The stock-evidence rule reads the runner's trace and provenance.
  directory <- getTemporaryDirectory
  let path = directory </> "judge-evidence-test.json"
  B.writeFile path "{\"runner\":{\"instructions\":40,\"status\":\"stop_address\",\"trace\":[{\"pc\":\"0x1000\"},{\"pc\":\"0x1004\"},{\"pc\":\"0x2000\"}],\"memory_provenance\":[{\"pc\":\"0x0ffc\"},{\"pc\":\"0x1008\"}]}}"
  evidence <- evidenceAt path 0x1000 0x10
  removeFile path
  check "trace instructions inside the extent are counted" (evidenceTrace evidence == 2)
  check "provenance instructions inside the extent are counted" (evidenceProvenance evidence == 1)
  check "stock instructions are kept" (evidenceInstructions evidence == 40)
  check "stock status is kept" (evidenceStatus evidence == "stop_address")
  check "a case with instructions has no evidence problem" (null (evidenceProblems evidence))
  check "a case without instructions has an evidence problem" (length (evidenceProblems (evidence {evidenceTrace = 0, evidenceProvenance = 0})) == 1)

  putStrLn "judge synthetic tests passed"

check :: String -> Bool -> IO ()
check label yes = unless yes (putStrLn ("FAIL: " ++ label) >> exitFailure)

report :: Report
report = Report
  { reportSuite = Just "synthetic"
  , reportImageDigests = []
  , reportOwnedDigest = Nothing
  , reportCases = []
  , reportControls = []
  , reportDiagnostics = []
  }

caseRow :: String -> Maybe Word32 -> Maybe Int -> [String] -> [String] -> ReportCase
caseRow name entry stock differences diagnostics = ReportCase
  { reportCaseName = name
  , reportCaseEntry = entry
  , reportCaseExtent = Just 4
  , reportCaseStockInside = stock
  , reportCaseDifferences = differences
  , reportCaseDiagnostics = diagnostics
  }

control :: String -> Bool -> [String] -> [String] -> ReportControl
control name detected differences diagnostics = ReportControl
  { reportControlName = name
  , reportControlDetected = detected
  , reportControlExpected = "r0"
  , reportControlDifferences = differences
  , reportControlDiagnostics = diagnostics
  }

unmetNames :: Verdict -> [ConditionName]
unmetNames verdict = [conditionName condition | condition <- verdictConditions verdict, not (conditionMet condition)]

isInfix :: String -> String -> Bool
isInfix needle haystack = any (prefix needle) (tails haystack)
  where
    prefix candidate text = take (length candidate) text == candidate
    tails [] = [[]]
    tails current@(_ : rest) = current : tails rest

lensShaped :: B.ByteString
lensShaped = "{\"suite\":\"lens-actor\",\"image_sha256\":\"f2e2\",\"owned_sha256\":\"4cb9\",\"cases\":[{\"id\":\"actor-register-idle\",\"stock_handler\":\"0x6c27f8\",\"stock_extent\":252,\"stock_handler_instructions\":27,\"stock_trace_instructions\":12,\"stock_provenance_instructions\":15,\"differences\":[]}],\"replies\":[{\"name\":\"valid\",\"stock_actor_handler\":\"0x6c4c94\",\"stock_actor_instructions\":190,\"differences\":[]}],\"controls\":[{\"name\":\"register-without-attach\",\"reported\":true,\"expected\":\"effect ring\",\"differences\":[\"effect ring\"]}]}"
