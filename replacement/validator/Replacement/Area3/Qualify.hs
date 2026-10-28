{-# LANGUAGE OverloadedStrings #-}
{-# LANGUAGE ScopedTypeVariables #-}
-- | Qualification orchestration for the area-3 validator contract.
module Replacement.Area3.Qualify
  ( Options (..)
  , Scope (..)
  , MutationSelection (..)
  , StockSelection (..)
  , defaultOptions
  , parseOptions
  , runQualification
  ) where

import Control.Concurrent (forkIO, newEmptyMVar, putMVar, takeMVar)
import Control.Exception (IOException, SomeException, catch, throwIO, try)
import Control.Monad (foldM, forM, (>=>))
import Data.Aeson (Value (..))
import Data.Bits (setBit, testBit, xor)
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as BC
import Data.Char (isDigit)
import Data.List (sortOn, subsequences, (\\))
import Data.Maybe (fromMaybe)
import Data.Word (Word16, Word32, Word8)
import Emulation.NativeCase (Region (..), checkResult)
import Emulation.Sha256 (sha256, sha256Hex)
import FujiAF.Host hiding (sha256)
import System.Exit (ExitCode (..))

import Replacement.Area3.Contract
import qualified Replacement.Area3.Domain as Domain
import Replacement.Area3.Native

data Scope = ScopeCapability | ScopeLogic
  deriving (Eq, Show)

data MutationSelection = MutationsAll | MutationsSingle | MutationsTwo
  deriving (Eq, Show)

data StockSelection = StockAll | StockNone | StockLimit Int
  deriving (Eq, Show)

data Options = Options
  { optionContract :: FilePath
  , optionCaseDir :: Maybe FilePath
  , optionRunner :: Maybe FilePath
  , optionCandidate :: FilePath
  , optionFaulty :: Maybe FilePath
  , optionArmImage :: Maybe FilePath
  , optionArmSymbols :: Maybe FilePath
  , optionArmBase :: Word32
  , optionArmInstructions :: Integer
  , optionArmWallMillis :: Integer
  , optionScope :: Scope
  , optionMutations :: MutationSelection
  , optionStock :: StockSelection
  , optionJobs :: Int
  , optionCounterexample :: Bool
  }

defaultOptions :: Options
defaultOptions =
  Options
    { optionContract = ""
    , optionCaseDir = Nothing
    , optionRunner = Nothing
    , optionCandidate = ""
    , optionFaulty = Nothing
    , optionArmImage = Nothing
    , optionArmSymbols = Nothing
    , optionArmBase = 0x00800000
    , optionArmInstructions = 20000
    , optionArmWallMillis = 2000
    , optionScope = ScopeCapability
    , optionMutations = MutationsAll
    , optionStock = StockAll
    , optionJobs = 4
    , optionCounterexample = True
    }

parseOptions :: [String] -> Either String Options
parseOptions = go defaultOptions
  where
    go options [] = Right options
    go options ("--contract" : value : rest) = go options {optionContract = value} rest
    go options ("--case-dir" : value : rest) = go options {optionCaseDir = Just value} rest
    go options ("--runner" : value : rest) = go options {optionRunner = Just value} rest
    go options ("--candidate" : value : rest) = go options {optionCandidate = value} rest
    go options ("--faulty" : value : rest) = go options {optionFaulty = Just value} rest
    go options ("--arm-image" : value : rest) = go options {optionArmImage = Just value} rest
    go options ("--arm-symbols" : value : rest) = go options {optionArmSymbols = Just value} rest
    go options ("--arm-base" : value : rest) =
      maybe (Left ("--arm-base: 32-bit address required: " ++ value)) (\base -> go options {optionArmBase = base} rest) (parseAddressOrNumber value)
    go options ("--arm-instructions" : value : rest) =
      case reads value of
        [(budget :: Integer, "")] | budget > 0 && budget <= 4000000 -> go options {optionArmInstructions = budget} rest
        _ -> Left ("--arm-instructions: positive budget required: " ++ value)
    go options ("--arm-wall-ms" : value : rest) =
      case reads value of
        [(wall :: Integer, "")] | wall > 0 && wall <= 10000 -> go options {optionArmWallMillis = wall} rest
        _ -> Left ("--arm-wall-ms: positive budget required: " ++ value)
    go options ("--scope" : value : rest) = case value of
      "capability" -> go options {optionScope = ScopeCapability} rest
      "logic" -> go options {optionScope = ScopeLogic} rest
      _ -> Left ("--scope: capability or logic required: " ++ value)
    go options ("--mutations" : value : rest) = case value of
      "all" -> go options {optionMutations = MutationsAll} rest
      "single" -> go options {optionMutations = MutationsSingle} rest
      "two" -> go options {optionMutations = MutationsTwo} rest
      _ -> Left ("--mutations: all, single or two required: " ++ value)
    go options ("--stock" : value : rest) = case value of
      "all" -> go options {optionStock = StockAll} rest
      "none" -> go options {optionStock = StockNone} rest
      _ | not (null value) && all isDigit value -> go options {optionStock = StockLimit (read value)} rest
      _ -> Left ("--stock: all, none or a count required: " ++ value)
    go options ("--jobs" : value : rest) = case reads value of
      [(jobs :: Int, "")] | jobs > 0 && jobs <= 32 -> go options {optionJobs = jobs} rest
      _ -> Left ("--jobs: 1..32 required: " ++ value)
    go options ("--no-counterexample" : rest) = go options {optionCounterexample = False} rest
    go _ (argument : _) = Left ("unknown argument: " ++ argument)

parseAddressOrNumber :: String -> Maybe Word32
parseAddressOrNumber value =
  case value of
    '0' : 'x' : digits -> readWord digits
    digits -> readWord digits
  where
    readWord digits
      | null digits || length digits > 8 || not (all isHex digits) = Nothing
      | otherwise = Just (foldl (\acc char -> acc * 16 + digit char) 0 digits)
    isHex char = isDigit char || (char >= 'a' && char <= 'f')
    digit char
      | isDigit char = fromIntegral (fromEnum char - fromEnum '0')
      | otherwise = fromIntegral (fromEnum char - fromEnum 'a' + 10)

data SubjectClass = ClassSingle | ClassTwo | ClassGround
  deriving (Eq, Ord, Show)

data Subject = Subject
  { subjectId :: String
  , subjectClass :: SubjectClass
  , subjectRecord :: Domain.Record
  , subjectOracle :: Domain.Oracle
  }

data CandidateReply = CandidateReply
  { replyOutcome :: Domain.Outcome
  , replyFinal :: Domain.Record
  }

-- | The command's result is the exit code and the canonical report.
runQualification :: Options -> IO (Int, Value)
runQualification options = do
  contractAttempt <- try (loadContract (optionContract options))
  case contractAttempt of
    Left (message :: IOException) -> pure (refusedReport (show message))
    Right contract ->
      case optionCaseDir options of
        Nothing -> pure (refusedReport "case directory not supplied")
        Just caseDir -> do
          anchorsAttempt <- try (loadAnchors contract caseDir)
          case anchorsAttempt of
            Left (message :: IOException) -> pure (refusedReport (show message))
            Right anchors ->
              case [anchor | anchor <- anchors, loadedOutcome anchor == Domain.Accepted] of
              [] -> pure (refusedReport "contract has no accepted anchor")
              (accepted : _) ->
                withTemporary "area3-qualify" $ \temporary -> do
                  workspace <-
                    prepareWorkspace temporary (loadedValue accepted) (loadedRegions accepted) (loadedArtifacts accepted)
                  candidateAttempt <- try (readRegular (8 * 1024 * 1024) (optionCandidate options))
                  case candidateAttempt of
                    Left (message :: IOException) -> pure (refusedReport (show message))
                    Right candidateBytes -> do
                      let digest = sha256Hex (sha256 candidateBytes)
                      grounds <- case Domain.groundCases (loadedRecord accepted) of
                        Left message -> pure (Left message)
                        Right cases -> pure (Right cases)
                      case grounds of
                        Left message -> pure (refusedReport message)
                        Right groundList -> do
                          let subjects = selectSubjects options (loadedRecord accepted) groundList
                              scratch = effectScratchRegions (contractEffects contract)
                          host <- evaluateCandidate (optionCandidate options) subjects
                          faultyRun <- case optionFaulty options of
                            Nothing -> pure Nothing
                            Just path -> fmap Just (evaluateCandidate path subjects)
                          (anchorResults, stockResults) <- case optionRunner options of
                            Nothing -> pure ([], [])
                            Just runner -> do
                              anchorResults' <- runAnchors contract anchors runner
                              stockChecks <- case optionStock options of
                                StockNone -> pure []
                                _ -> runStockBatch contract workspace runner options (selectStock options subjects)
                              pure (anchorResults', stockChecks)
                          armResult <- case (optionArmImage options, optionArmSymbols options, optionRunner options) of
                            (Nothing, _, _) | optionScope options == ScopeLogic -> pure Nothing
                            (Just imagePath, Just symbolsPath, Just runner) -> do
                              imageAttempt <- loadArmImage imagePath symbolsPath (optionArmBase options)
                              case imageAttempt of
                                Left message -> pure (Just (Left message))
                                Right image -> do
                                  installArmImage workspace image
                                  let profile =
                                        CandidateProfile
                                          { profileImage = image
                                          , profileInstructionBudget = optionArmInstructions options
                                          , profileWallMillis = optionArmWallMillis options
                                          }
                                  Just . Right <$> runArmBatch contract workspace profile runner options subjects
                            (_, _, Nothing) -> pure (Just (Left "ARM execution requires the native runner"))
                            _ -> pure (Just (Left "cross-compiled ARM candidate artifact not supplied"))
                          refusals <- probeRefusals (optionCandidate options) (loadedRecord accepted)
                          counterexample <- case (optionFaulty options, faultyRun) of
                            (Just faultyPath, Just (Right replies))
                              | optionCounterexample options
                              , (subject, _) : _ <- failingFaulty replies
                              , not (null (differenceSites (loadedRecord accepted) (subjectRecord subject))) ->
                                  Just <$> buildCounterexample contract scratch faultyPath workspace (optionRunner options) (loadedRecord accepted) (subjectRecord subject)
                            _ -> pure Nothing
                          let hostSummary = summarizeHost subjects host
                              anchorOutput = map anchorReport anchorResults
                              (anchorFailures, anchorBlockers, anchorUnknowns) = anchorChecks anchorResults
                              (stockFailures, stockBlockers, stockUnknowns) = outcomeChecks stockResults
                              (armFailures, armBlockers, armUnknowns) = case armResult of
                                Nothing -> ([], [], [])
                                Just (Left message) -> ([], [message], [])
                                Just (Right outcomes) -> outcomeChecks outcomes
                              faultyFailures' = case faultyRun of
                                Nothing -> []
                                Just (Left message) -> ["faulty candidate evaluation failed: " ++ message]
                                Just (Right replies)
                                  | null (failingFaulty replies) -> ["faulty candidate was not detected"]
                                  | otherwise -> []
                              probeFailures = refusalFailures refusals
                              prerequisites =
                                [ "native runner not supplied"
                                | optionScope options == ScopeCapability
                                , Nothing <- [optionRunner options]
                                ]
                                  ++ [ "ARM candidate artifact not supplied"
                                     | optionScope options == ScopeCapability
                                     , not (hasArm options)
                                     ]
                              failures =
                                hostMismatches hostSummary
                                  ++ anchorFailures
                                  ++ stockFailures
                                  ++ armFailures
                                  ++ faultyFailures'
                                  ++ probeFailures
                              blockers =
                                prerequisites
                                  ++ hostErrors hostSummary
                                  ++ anchorBlockers
                                  ++ stockBlockers
                                  ++ armBlockers
                              unknowns = anchorUnknowns ++ stockUnknowns ++ armUnknowns
                              status
                                | not (null failures) = "failed"
                                | not (null blockers) = "blocked"
                                | not (null unknowns) = "unknown"
                                | otherwise = "qualified"
                              capability = status == "qualified" && optionScope options == ScopeCapability
                              report =
                                obj
                                  [ ("schema_version", n 1)
                                  , ("contract", s (contractId contract))
                                  , ("scope", s (scopeName (optionScope options)))
                                  , ("status", s status)
                                  , ("capability", b capability)
                                  , ("candidate", candidateReport digest hostSummary)
                                  , ("coverage", coverageReport subjects host stockResults armResult)
                                  , ("anchors", arr anchorOutput)
                                  , ("stock", stockReport stockResults)
                                  , ("arm", armReport armResult)
                                  , ("faulty", faultyReport faultyRun counterexample)
                                  , ("refusals", refusalsReport refusals)
                                  , ("failures", arr (map s (take 20 failures)))
                                  , ("blockers", arr (map s (take 20 blockers)))
                                  , ("unknowns", arr (map s (take 20 unknowns)))
                                  , ("evidence_limits", arr (map s (evidenceLimits contract options subjects stockResults)))
                                  ]
                          pure (exitCodeFor status, report)

refusedReport :: String -> (Int, Value)
refusedReport message =
  ( 2
  , obj
      [ ("schema_version", n 1)
      , ("status", s "refused")
      , ("capability", b False)
      , ("reason", s message)
      ]
  )

exitCodeFor :: String -> Int
exitCodeFor "qualified" = 0
exitCodeFor "failed" = 1
exitCodeFor "refused" = 2
exitCodeFor _ = 3

scopeName :: Scope -> String
scopeName ScopeCapability = "capability"
scopeName ScopeLogic = "logic"

hasArm :: Options -> Bool
hasArm options = case (optionArmImage options, optionArmSymbols options) of
  (Just _, Just _) -> True
  _ -> False

selectSubjects :: Options -> Domain.Record -> [Domain.GroundCase] -> [Subject]
selectSubjects options record groundList = single ++ two ++ ground
  where
    single =
      [ Subject (Domain.mutationId mutation) ClassSingle (Domain.mutationRecord mutation) (Domain.mutationExpected mutation)
      | mutation <- Domain.singleBitMutations record
      , optionMutations options /= MutationsTwo
      ]
    two =
      [ Subject (Domain.mutationId mutation) ClassTwo (Domain.mutationRecord mutation) (Domain.mutationExpected mutation)
      | mutation <- Domain.twoBitMutations record
      , optionMutations options /= MutationsSingle
      ]
    ground =
      [ Subject (Domain.groundId item) ClassGround (Domain.groundRecord item) (Domain.groundExpected item)
      | item <- groundList
      ]

selectStock :: Options -> [Subject] -> [Subject]
selectStock options subjects = case optionStock options of
  StockNone -> []
  StockAll -> subjects
  StockLimit count -> [item | item <- subjects, subjectClass item /= ClassTwo] ++ take count [item | item <- subjects, subjectClass item == ClassTwo]

-- Host candidate --------------------------------------------------------------

evaluateCandidate :: FilePath -> [Subject] -> IO (Either String [(Subject, CandidateReply)])
evaluateCandidate adapter subjects = do
  results <- forM (chunked 2048 subjects) $ \chunk -> do
    let input =
          BC.pack
            ( concat
                ["validate " ++ Domain.recordHex (subjectRecord item) ++ " 0000002c\n" | item <- chunk]
            )
    (timedOut, exit, out, err) <- runBounded 120000000 (16 * 1024 * 1024) adapter [] input
    if timedOut
      then pure (Left "candidate driver timed out")
      else
        if exit /= ExitSuccess
          then pure (Left ("candidate driver failed: " ++ BC.unpack err))
          else case traverse parseReply (lines (BC.unpack out)) of
            Nothing -> pure (Left "candidate driver reply unparsed")
            Just replies
              | length replies /= length chunk -> pure (Left "candidate driver reply count mismatch")
              | otherwise -> pure (Right (zip chunk replies))
  pure (concat <$> sequence results)

chunked :: Int -> [a] -> [[a]]
chunked size items
  | null items = []
  | otherwise = take size items : chunked size (drop size items)

parseReply :: String -> Maybe CandidateReply
parseReply line = case words line of
  ["accepted", bytes] -> CandidateReply Domain.Accepted <$> finalRecord bytes
  ["corrected", row, column, bytes] -> do
    site <- siteOf row column
    CandidateReply (Domain.Corrected site) <$> finalRecord bytes
  ["rejected", row, column, bytes] -> do
    syndromeRow <- readHexValue row :: Maybe Word16
    syndromeColumn <- readHexValue column :: Maybe Word32
    CandidateReply (Domain.Rejected (Domain.Syndrome syndromeRow syndromeColumn)) <$> finalRecord bytes
  _ -> Nothing
  where
    finalRecord text = unhex text >>= either (const Nothing) Just . Domain.mkRecord
    siteOf row column = do
      rowValue <- readDecimal row
      columnValue <- readDecimal column
      if rowValue < Domain.rowCount && columnValue < Domain.columnCount
        then
          Just
            ( Domain.BitSite
                (Domain.RowIndex (fromIntegral rowValue))
                (Domain.ColumnIndex (fromIntegral columnValue))
            )
        else Nothing

readDecimal :: String -> Maybe Int
readDecimal text = case reads text of
  [(value, "")] -> Just value
  _ -> Nothing

readHexValue :: Integral a => String -> Maybe a
readHexValue text
  | null text = Nothing
  | otherwise = foldl step (Just 0) text
  where
    step Nothing _ = Nothing
    step (Just acc) char
      | isDigit char = Just (acc * 16 + fromIntegral (fromEnum char - fromEnum '0'))
      | char >= 'a' && char <= 'f' = Just (acc * 16 + fromIntegral (fromEnum char - fromEnum 'a' + 10))
      | otherwise = Nothing

data HostSummary = HostSummary
  { hostEvaluated :: Int
  , hostMismatches :: [String]
  , hostErrors :: [String]
  }

summarizeHost :: [Subject] -> Either String [(Subject, CandidateReply)] -> HostSummary
summarizeHost subjects result = case result of
  Left message -> HostSummary (length subjects) [] [message]
  Right replies ->
    HostSummary
      (length replies)
      [ subjectId item ++ ": expected " ++ Domain.describeOutcome (Domain.oracleOutcome (subjectOracle item))
          ++ ", observed " ++ Domain.describeOutcome (replyOutcome reply)
          ++ finalMismatch item reply
      | (item, reply) <- replies
      , replyOutcome reply /= Domain.oracleOutcome (subjectOracle item)
          || replyFinal reply /= Domain.oracleFinal (subjectOracle item)
      ]
      []
  where
    finalMismatch item reply
      | replyFinal reply /= Domain.oracleFinal (subjectOracle item) = " (final record differs)"
      | otherwise = ""

candidateReport :: String -> HostSummary -> Value
candidateReport digest summary =
  obj
    [ ("kind", s "host")
    , ("sha256", s digest)
    , ("evaluated", n (fromIntegral (hostEvaluated summary)))
    , ("matched", n (fromIntegral (hostEvaluated summary - length (hostMismatches summary))))
    , ("mismatches", arr (map s (take 10 (hostMismatches summary))))
    , ("errors", arr (map s (hostErrors summary)))
    ]

-- Anchors ---------------------------------------------------------------------

data AnchorResult = AnchorResult
  { resultCase :: CaseId
  , resultCheck :: Check
  , resultProblems :: [String]
  , resultRaw :: Maybe RawRun
  }

runAnchors :: Contract -> [LoadedAnchor] -> FilePath -> IO [AnchorResult]
runAnchors contract anchors runner = forM anchors $ \anchor -> do
  result <- runCaseFile runner (loadedPath anchor)
  case result of
    Left message -> pure (AnchorResult (loadedId anchor) (CheckBlocked message) [message] Nothing)
    Right (caseValue, regions, raw) -> do
      existing <- checkResult caseValue regions (rawValue raw)
      let observation = observe (map regionName regions) (rawValue raw)
          check = checkCase (expectationFromCase (effectScratchRegions (contractEffects contract)) caseValue regions) observation
      pure (AnchorResult (loadedId anchor) check existing (Just raw))

regionName :: Region -> String
regionName region = str (get (description region) "id")

anchorReport :: AnchorResult -> Value
anchorReport result =
  obj
    [ ("case", s (showCase (resultCase result)))
    , ("status", s (checkStatus (resultCheck result)))
    , ("runner_status", s (maybe "" rawStatus (resultRaw result)))
    , ("problems", arr (map s (take 10 (resultProblems result ++ checkProblems (resultCheck result)))))
    ]

showCase :: CaseId -> String
showCase (CaseId name) = name

checkStatus :: Check -> String
checkStatus CheckPass = "passed"
checkStatus (CheckMismatch _) = "failed"
checkStatus (CheckBlocked _) = "blocked"
checkStatus (CheckUnsupported _) = "unknown"
checkStatus (CheckUnknown _) = "unknown"
checkStatus (CheckTruncated _) = "unknown"

checkProblems :: Check -> [String]
checkProblems (CheckMismatch problems) = problems
checkProblems (CheckBlocked message) = [message]
checkProblems (CheckUnsupported message) = [message]
checkProblems (CheckUnknown message) = [message]
checkProblems (CheckTruncated message) = [message]
checkProblems CheckPass = []

anchorChecks :: [AnchorResult] -> ([String], [String], [String])
anchorChecks results =
  ( [showCase (resultCase result) ++ ": " ++ problem | result <- results, problem <- resultProblems result]
      ++ [showCase (resultCase result) ++ ": " ++ problem | result <- results, problem <- checkProblems (resultCheck result), checkStatus (resultCheck result) == "failed"]
  , [showCase (resultCase result) ++ ": " ++ problem | result <- results, problem <- checkProblems (resultCheck result), checkStatus (resultCheck result) == "blocked"]
  , [showCase (resultCase result) ++ ": " ++ problem | result <- results, problem <- checkProblems (resultCheck result), checkStatus (resultCheck result) == "unknown"]
  )

-- Stock and ARM cases ---------------------------------------------------------

data CaseOutcome = CaseOutcome
  { outcomeClass :: SubjectClass
  , outcomeId :: String
  , outcomeCheck :: Check
  }

runStockBatch :: Contract -> Workspace -> FilePath -> Options -> [Subject] -> IO [CaseOutcome]
runStockBatch contract workspace runner options subjects =
  parallelMap (optionJobs options) check subjects
  where
    check item = do
      path <- writeStockCase workspace (subjectId item) (provenance item) (subjectRecord item) (subjectOracle item)
      result <- runCaseFile runner path
      case result of
        Left message -> pure (CaseOutcome (subjectClass item) (subjectId item) (CheckBlocked message))
        Right (caseValue, regions, raw) -> do
          let observation = observe (map regionName regions) (rawValue raw)
          pure
            ( CaseOutcome
                (subjectClass item)
                (subjectId item)
                (checkCase (expectationFromCase (effectScratchRegions (contractEffects contract)) caseValue regions) observation)
            )
    provenance item =
      "Generated from the grounded record-accept input: " ++ classText (subjectClass item) ++ " " ++ subjectId item
        ++ "; expectation from the documented area-3 parity contract."

classText :: SubjectClass -> String
classText ClassSingle = "single-bit"
classText ClassTwo = "two-bit"
classText ClassGround = "ground"

runArmBatch :: Contract -> Workspace -> CandidateProfile -> FilePath -> Options -> [Subject] -> IO [CaseOutcome]
runArmBatch contract workspace profile runner options subjects =
  parallelMap (optionJobs options) check subjects
  where
    check item = do
      path <- writeArmCase workspace profile ("arm-" ++ subjectId item) (provenance item) (subjectRecord item) (subjectOracle item)
      result <- runCaseFile runner path
      case result of
        Left message -> pure (CaseOutcome (subjectClass item) (subjectId item) (CheckBlocked message))
        Right (caseValue, regions, raw) -> do
          let observation = observe (map regionName regions) (rawValue raw)
          pure
            ( CaseOutcome
                (subjectClass item)
                (subjectId item)
                (checkCase (expectationFromCase (effectScratchRegions (contractEffects contract)) caseValue regions) observation)
            )
    provenance item =
      "Replacement ARM candidate executed from the declared artifact identity: " ++ subjectId item
        ++ "; expectation from the documented area-3 parity contract."

outcomeChecks :: [CaseOutcome] -> ([String], [String], [String])
outcomeChecks outcomes =
  ( [outcomeId outcome ++ ": " ++ describeCheck (outcomeCheck outcome) | outcome <- outcomes, isMismatch (outcomeCheck outcome)]
  , [outcomeId outcome ++ ": " ++ describeCheck (outcomeCheck outcome) | outcome <- outcomes, isBlocked (outcomeCheck outcome)]
  , [outcomeId outcome ++ ": " ++ describeCheck (outcomeCheck outcome) | outcome <- outcomes, isUnknown (outcomeCheck outcome)]
  )
  where
    isMismatch (CheckMismatch _) = True
    isMismatch _ = False
    isBlocked (CheckBlocked _) = True
    isBlocked _ = False
    isUnknown (CheckUnsupported _) = True
    isUnknown (CheckUnknown _) = True
    isUnknown (CheckTruncated _) = True
    isUnknown _ = False

describeCheck :: Check -> String
describeCheck CheckPass = "passed"
describeCheck (CheckMismatch problems) = concatMap (++ "; ") problems
describeCheck (CheckBlocked message) = "blocked: " ++ message
describeCheck (CheckUnsupported message) = "unsupported: " ++ message
describeCheck (CheckUnknown message) = "unknown: " ++ message
describeCheck (CheckTruncated message) = "truncated evidence: " ++ message

stockReport :: [CaseOutcome] -> Value
stockReport outcomes =
  obj
    [ ("checked", n (fromIntegral (length outcomes)))
    , ("passed", n (fromIntegral (length [() | outcome <- outcomes, outcomeCheck outcome == CheckPass])))
    , ("failures", arr (map s (take 10 [outcomeId outcome ++ ": " ++ describeCheck (outcomeCheck outcome) | outcome <- outcomes, outcomeCheck outcome /= CheckPass])))
    ]

armReport :: Maybe (Either String [CaseOutcome]) -> Value
armReport Nothing = obj [("status", s "not-run"), ("reason", s "logic scope does not require the ARM artifact")]
armReport (Just (Left message)) = obj [("status", s "blocked"), ("reason", s message)]
armReport (Just (Right outcomes)) =
  obj
    [ ("status", s (if all ((== CheckPass) . outcomeCheck) outcomes then "passed" else "failed"))
    , ("checked", n (fromIntegral (length outcomes)))
    , ("passed", n (fromIntegral (length [() | outcome <- outcomes, outcomeCheck outcome == CheckPass])))
    , ("failures", arr (map s (take 10 [outcomeId outcome ++ ": " ++ describeCheck (outcomeCheck outcome) | outcome <- outcomes, outcomeCheck outcome /= CheckPass])))
    ]

coverageReport :: [Subject] -> Either String [(Subject, CandidateReply)] -> [CaseOutcome] -> Maybe (Either String [CaseOutcome]) -> Value
coverageReport subjects host stockOutcomes armOutcomes =
  obj
    [ ("single_bit", forClass ClassSingle)
    , ("two_bit", forClass ClassTwo)
    , ("ground", forClass ClassGround)
    ]
  where
    forClass kind =
      let selected = [item | item <- subjects, subjectClass item == kind]
          matched = case host of
            Left _ -> 0
            Right replies ->
              length
                [ ()
                | (item, reply) <- replies
                , subjectClass item == kind
                , replyOutcome reply == Domain.oracleOutcome (subjectOracle item)
                , replyFinal reply == Domain.oracleFinal (subjectOracle item)
                ]
          stockPassed = length [() | outcome <- stockOutcomes, outcomeClass outcome == kind, outcomeCheck outcome == CheckPass]
          armPassed = case armOutcomes of
            Nothing -> 0
            Just (Left _) -> 0
            Just (Right outcomes) -> length [() | outcome <- outcomes, outcomeClass outcome == kind, outcomeCheck outcome == CheckPass]
       in obj
            [ ("enumerated", n (fromIntegral (length selected)))
            , ("candidate_matched", n (fromIntegral matched))
            , ("stock_passed", n (fromIntegral stockPassed))
            , ("arm_passed", n (fromIntegral armPassed))
            ]

-- Faulty candidate and counterexample ----------------------------------------

failingFaulty :: [(Subject, CandidateReply)] -> [(Subject, CandidateReply)]
failingFaulty replies =
  sortOn (\(item, _) -> subjectClass item == ClassGround)
    [ pair
    | pair@(item, reply) <- replies
    , replyOutcome reply /= Domain.oracleOutcome (subjectOracle item)
    ]

faultyReport :: Maybe (Either String [(Subject, CandidateReply)]) -> Maybe Value -> Value
faultyReport Nothing _ = obj [("status", s "not-run")]
faultyReport (Just (Left message)) _ = obj [("status", s "blocked"), ("reason", s message)]
faultyReport (Just (Right replies)) counterexample =
  obj
    [ ("status", s (if null (failingFaulty replies) then "not-detected" else "detected"))
    , ("mismatches", n (fromIntegral (length (failingFaulty replies))))
    , ("counterexample", fromMaybe Null counterexample)
    ]

buildCounterexample
  :: Contract
  -> [String]
  -> FilePath
  -> Workspace
  -> Maybe FilePath
  -> Domain.Record
  -> Domain.Record
  -> IO Value
buildCounterexample _contract scratch faultyPath workspace runner base seed = do
  let evaluate = evaluateOne faultyPath
  augmented <- augment evaluate base seed
  reduced <- reduce evaluate base augmented
  case reduced of
    Left message -> pure (obj [("status", s "unreduced"), ("reason", s message)])
    Right (witness, initialBits, finalBits) -> do
      stockStatus <- case runner of
        Nothing -> pure "unchecked"
        Just runnerPath -> do
          path <-
            writeStockCase workspace "counterexample" "Reduced counterexample re-verification." witness (Domain.oracle witness)
          result <- runCaseFile runnerPath path
          pure $ case result of
            Left _ -> "unchecked"
            Right (caseValue, regions, raw) ->
              case checkCase (expectationFromCase scratch caseValue regions) (observe (map regionName regions) (rawValue raw)) of
                CheckPass -> "rejected"
                _ -> "mismatch"
      pure
        ( obj
            [ ("record", s (Domain.recordHex witness))
            , ("oracle", s (Domain.describeOutcome (Domain.oracleOutcome (Domain.oracle witness))))
            , ("candidate", s "accepted")
            , ("stock", s stockStatus)
            , ("differences", arr [obj [("byte", n (fromIntegral index)), ("protected", b (index < Domain.protectedSize)), ("bits", arr (map (n . fromIntegral) bits))] | (index, bits) <- allDifferenceSites base witness])
            , ("scope", s "protected-data")
            , ("initial_bits", n (fromIntegral initialBits))
            , ("final_bits", n (fromIntegral finalBits))
            , ( "preconditions"
              , arr
                  [ s "52-byte record with protected length 0x2c"
                  , s "stored row parity high bits zero"
                  , s "differences confined to the 44 protected bytes"
                  , s "contract oracle rejects the record"
                  , s "faulty candidate accepts it"
                  ]
              )
            ]
        )

evaluateOne :: FilePath -> Domain.Record -> IO (Either String Domain.Outcome)
evaluateOne adapter record = do
  let input = BC.pack ("validate " ++ Domain.recordHex record ++ " 0000002c\n")
  (timedOut, exit, out, err) <- runBounded 30000000 (1024 * 1024) adapter [] input
  if timedOut
    then pure (Left "candidate driver timed out")
    else
      if exit /= ExitSuccess
        then pure (Left ("candidate driver failed: " ++ BC.unpack err))
        else case traverse parseReply (lines (BC.unpack out)) of
          Just [reply] -> pure (Right (replyOutcome reply))
          _ -> pure (Left "candidate driver reply unparsed")

failureAt :: (Domain.Record -> IO (Either String Domain.Outcome)) -> Domain.Record -> IO Bool
failureAt evaluate record = do
  outcome <- evaluate record
  pure $ case outcome of
    Left _ -> False
    Right actual ->
      let expected = Domain.oracleOutcome (Domain.oracle record)
       in Domain.outcomeClass expected == "rejected" && Domain.outcomeClass actual == "accepted"

augment :: (Domain.Record -> IO (Either String Domain.Outcome)) -> Domain.Record -> Domain.Record -> IO Domain.Record
augment evaluate base seed = case differenceSites base seed of
  [] -> pure seed
  ((index, bits) : _) -> do
    let unused = take 2 ([0 .. 7] \\ bits)
        value = byteAt seed index `xor` foldl setBit 0 unused
        candidate = setRecordByte index value seed
    holds <- failureAt evaluate candidate
    pure (if holds then candidate else seed)

reduce :: (Domain.Record -> IO (Either String Domain.Outcome)) -> Domain.Record -> Domain.Record -> IO (Either String (Domain.Record, Int, Int))
reduce evaluate base seed = do
  final <- stabilize (bytePass evaluate base >=> bitPass evaluate base) seed
  pure (Right (final, differenceCount base seed, differenceCount base final))

stabilize :: Eq a => (a -> IO a) -> a -> IO a
stabilize step current = do
  next <- step current
  if next == current then pure current else stabilize step next

bytePass :: (Domain.Record -> IO (Either String Domain.Outcome)) -> Domain.Record -> Domain.Record -> IO Domain.Record
bytePass evaluate base record = foldM step record [0 .. Domain.protectedSize - 1]
  where
    step current index
      | byteAt current index == byteAt base index = pure current
      | otherwise = do
          let candidate = setRecordByte index (byteAt base index) current
          holds <- failureAt evaluate candidate
          pure (if holds then candidate else current)

bitPass :: (Domain.Record -> IO (Either String Domain.Outcome)) -> Domain.Record -> Domain.Record -> IO Domain.Record
bitPass evaluate base record = foldM step record [0 .. Domain.protectedSize - 1]
  where
    step current index
      | byteAt current index == byteAt base index = pure current
      | otherwise = do
          let differing = [bit | bit <- [0 .. 7], testBit (byteAt current index `xor` byteAt base index) bit]
              candidates = [keep | keep <- subsequences differing, not (null keep)]
          found <-
            firstM
              ( \keep -> do
                  let value = byteAt base index `xor` foldl setBit 0 keep
                      candidate = setRecordByte index value current
                  holds <- failureAt evaluate candidate
                  pure (if holds then Just candidate else Nothing)
              )
              (sortOn length candidates)
          pure (fromMaybe current found)

firstM :: (a -> IO (Maybe b)) -> [a] -> IO (Maybe b)
firstM _ [] = pure Nothing
firstM action (item : rest) = do
  result <- action item
  case result of
    Just _ -> pure result
    Nothing -> firstM action rest

differenceSites :: Domain.Record -> Domain.Record -> [(Int, [Int])]
differenceSites base record =
  [ (index, bits)
  | index <- [0 .. Domain.protectedSize - 1]
  , let bits = [bit | bit <- [0 .. 7], testBit (byteAt record index `xor` byteAt base index) bit]
  , not (null bits)
  ]

differenceCount :: Domain.Record -> Domain.Record -> Int
differenceCount base = sum . map (length . snd) . differenceSites base

allDifferenceSites :: Domain.Record -> Domain.Record -> [(Int, [Int])]
allDifferenceSites base record =
  [ (index, bits)
  | index <- [0 .. Domain.recordSize - 1]
  , let bits = [bit | bit <- [0 .. 7], testBit (byteAt record index `xor` byteAt base index) bit]
  , not (null bits)
  ]

byteAt :: Domain.Record -> Int -> Word8
byteAt record index = B.index (Domain.recordBytes record) index

setRecordByte :: Int -> Word8 -> Domain.Record -> Domain.Record
setRecordByte index value (Domain.Record bytes) =
  Domain.Record (B.take index bytes <> B.singleton value <> B.drop (index + 1) bytes)

-- Refusal probe ---------------------------------------------------------------

probeRefusals :: FilePath -> Domain.Record -> IO [(String, Maybe String)]
probeRefusals adapter record = do
  let recordHex = Domain.recordHex record
      highRecord =
        fromMaybe record $ do
          bytes <- unhex recordHex
          let offset = Domain.rowParityOffset + 1
              flipped =
                B.take offset bytes
                  <> B.singleton (B.index bytes offset `xor` 0x08)
                  <> B.drop (offset + 1) bytes
          either (const Nothing) Just (Domain.mkRecord flipped)
      requests =
        [ ("unsupported_length", "validate " ++ recordHex ++ " 0000002b")
        , ("row_parity_high_bits", "validate " ++ Domain.recordHex highRecord ++ " 0000002c")
        , ("bad_record_hex", "validate " ++ take 100 recordHex ++ " 0000002c")
        , ("bad_verb", "check " ++ recordHex ++ " 0000002c")
        ]
  (timedOut, exit, out, _) <- runBounded 30000000 (1024 * 1024) adapter [] (BC.pack (unlines (map snd requests)))
  if timedOut || exit /= ExitSuccess
    then pure [(token, Nothing) | (token, _) <- requests]
    else do
      let replies = lines (BC.unpack out)
      pure
        [ (token, replyToken reply)
        | ((token, _), reply) <- zip requests replies
        ]
  where
    replyToken reply = case words reply of
      ["refused", token] -> Just token
      _ -> Nothing

refusalFailures :: [(String, Maybe String)] -> [String]
refusalFailures results =
  [ "refusal probe " ++ expected ++ ": observed " ++ fromMaybe "no reply" observed
  | (expected, observed) <- results
  , observed /= Just expected
  ]

refusalsReport :: [(String, Maybe String)] -> Value
refusalsReport results =
  obj
    [ ( "probes"
      , arr [obj [("expected", s expected), ("observed", maybe Null s observed)] | (expected, observed) <- results]
      )
    ]

evidenceLimits :: Contract -> Options -> [Subject] -> [CaseOutcome] -> [String]
evidenceLimits contract options subjects stockOutcomes =
  [ "Mutation coverage is the selected record only: "
      ++ show (coverageSingleBit (contractCoverage contract))
      ++ " single-bit and "
      ++ show (coverageTwoBit (contractCoverage contract))
      ++ " distinct two-bit flips of the "
      ++ show (coverageProtectedBits (contractCoverage contract))
      ++ " protected bits; other records and parity-byte mutations are outside that enumeration"
  , "Stock execution covered " ++ show (length stockOutcomes) ++ " of " ++ show (length subjects) ++ " selected subjects"
  , "Stored row-parity bits 11..31 are required zero and are unmodeled"
  , "No storage read, ECC visibility, timing, writeback or camera action is modeled"
  , if optionScope options == ScopeCapability
      then "Capability requires the cross-compiled ARM candidate executed in the native harness; the host candidate is supplementary"
      else "Logic scope: the host candidate is compared with the contract oracle only; this is not capability evidence"
  ]
    ++ map ("Contract exclusion: " ++) (contractExclusions contract)

-- Parallel workers ------------------------------------------------------------

parallelMap :: Int -> (a -> IO b) -> [a] -> IO [b]
parallelMap workers action items = do
  let groups = max 1 (min workers (length items))
      indexed = zip [0 :: Int ..] items
      striped = [[pair | pair@(index, _) <- indexed, index `mod` groups == group] | group <- [0 .. groups - 1]]
  variables <- forM striped $ \group -> do
    variable <- newEmptyMVar
    _ <- forkIO $ do
      result <-
        try (mapM (\(index, item) -> do value <- action item; pure (index, value)) group)
          `catch` \(exception :: SomeException) -> pure (Left exception)
      putMVar variable result
    pure variable
  outcomes <- mapM takeMVar variables
  case sequence outcomes of
    Left exception -> throwIO exception
    Right rows -> pure (map snd (sortOn fst (concat rows)))

-- The record type's constructor is used by the reducer's one-byte replacement;
-- the length invariant is preserved because a byte is replaced, not added.
