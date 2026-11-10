{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}
module Main (main) where

import Control.Monad (unless)
import Data.Aeson (object, (.=))
import qualified Data.ByteString as B
import Data.Either (isLeft)
import qualified Data.Set as S
import Emulation.CachedNativeBatch (CacheVerdict (..))
import Emulation.NativeCase (CaseId (..), CaseStatus (..))
import Emulation.PrimaryInitializerSource (ByteCount (..))
import Emulation.Sha256 (sha256)
import FujiAF.JSON (encodeCanonical)
import RetirementPlanner
import System.Exit (ExitCode (..))

main :: IO ()
main = do
  let raw = object ["status" .= ("returned" :: String)]
      rawBytes = encodeCanonical raw
      rawSha = sha256 rawBytes
      otherSha = sha256 (B.singleton 1)
      old = OldEvidence "old/case.native-result.json" (ByteCount 100) rawSha (CaseId "case") rawSha rawSha ExitSuccess raw DeadReturnedFrame Nothing
      manifest = ManifestEvidence "cache/entry" (sha256 B.empty) (CaseId "case") rawSha rawSha ExitSuccess rawSha
      current = CurrentEvidence (CaseId "case") (CacheVerdict CasePassed []) rawSha rawBytes True
      classify old' binding current' = classifyRetirement [] S.empty old' binding current'
      expectCandidate label result = case result of
        RetirementCandidate _ -> pure ()
        RetirementPreserved reason -> fail (label ++ ": expected candidate, got " ++ show reason)
      expectPreserved label expected result = case result of
        RetirementPreserved reason -> unless (reason == expected) (fail (label ++ ": expected " ++ show expected ++ ", got " ++ show reason))
        RetirementCandidate _ -> fail (label ++ ": expected refusal")
      expectWindow label expected result = case result of
        Right actual | actual == expected -> pure ()
        _ -> fail (label ++ ": unexpected window")
  let caseNames = ["a", "b", "c"] :: [String]
  expectWindow "offset-zero" ([(0, "a"), (1, "b")] :: [(Int, String)]) (selectCaseWindow (CaseOffset 0) 2 caseNames)
  expectWindow "offset-two" ([(2, "c")] :: [(Int, String)]) (selectCaseWindow (CaseOffset 2) 1 caseNames)
  unless (isLeft (selectCaseWindow (CaseOffset 3) 1 caseNames)) (fail "out-of-range offset was admitted")
  unless (isLeft (selectCaseWindow (CaseOffset (-1)) 1 caseNames)) (fail "negative offset was admitted")
  unless (isLeft (selectCaseWindow (CaseOffset 0) 0 caseNames)) (fail "zero window was admitted")
  unless (isLeft (selectCaseWindow (CaseOffset 0) 4097 caseNames)) (fail "oversized window was admitted")
  expectCandidate "valid" (classify old (Just manifest) current)
  expectCandidate "same-wire-recorded-name" (classify old (Just manifest {manifestRecordedCaseId = CaseId "stored-case"}) current)
  expectPreserved "manifest-wire" WireMismatch (classify old (Just manifest {manifestWireSha = otherSha}) current)
  expectPreserved "wire" WireMismatch (classify old {oldWireSha = otherSha} (Just manifest) current)
  expectPreserved "raw" RawMismatch (classify old {oldRaw = object ["status" .= ("failed" :: String)]} (Just manifest) current)
  expectPreserved "runner" RunnerMismatch (classify old {oldRunnerSha = otherSha} (Just manifest) current)
  expectPreserved "caller" CallerOrOtherFailure (classify old {oldFailureClass = OtherFailure ["caller frame"]} (Just manifest) current)
  expectPreserved "unknown-singleton" CallerOrOtherFailure (classify old {oldFailureClass = parseLegacyFailure ["unrelated diagnostic"]} (Just manifest) current)
  expectPreserved "caller-singleton" CallerOrOtherFailure (classify old {oldFailureClass = parseLegacyFailure ["memory caller-frame: differs from exact allowed final bytes"]} (Just manifest) current)
  expectPreserved "mixed-diagnostics" CallerOrOtherFailure (classify old {oldFailureClass = parseLegacyFailure [deadFrameFailure, "caller frame: differs"]} (Just manifest) current)
  expectPreserved "expectation" ExpectationMismatch (classify old (Just manifest) current {currentExpectationMatches = False})
  expectPreserved "cited" CitedResult (classifyRetirement [oldPath old] S.empty old (Just manifest) current)
  expectPreserved "cited-raw" RawMismatch (classifyRetirement [oldPath old] S.empty old {oldRaw = object ["status" .= ("failed" :: String)]} (Just manifest) current)
  expectPreserved "cited-expectation" ExpectationMismatch (classifyRetirement [oldPath old] S.empty old (Just manifest) current {currentExpectationMatches = False})
  expectPreserved "cited-abi" CurrentAbiFailure (classifyRetirement [oldPath old] S.empty old (Just manifest) current {currentVerdict = CacheVerdict CaseFailed []})
  expectPreserved "missing-binding" MissingBinding (classify old Nothing current)
  expectPreserved "abi" CurrentAbiFailure (classify old (Just manifest) current {currentVerdict = CacheVerdict CaseFailed []})
  expectPreserved "witness" FailureClassWitness (classifyRetirement [] (S.singleton DeadReturnedFrame) old (Just manifest) current)
  let expectedCase stackHex liveHex = object
        [ "ignored_memory" .= [object ["region" .= ("stack" :: String), "offset" .= (0 :: Int), "length" .= (2 :: Int)]]
        , "expected" .= object
            [ "memory" .=
                [ object ["region" .= ("stack" :: String), "offset" .= (0 :: Int), "hex" .= stackHex]
                , object ["region" .= ("stack" :: String), "offset" .= (2 :: Int), "hex" .= liveHex]
                ]
            ]
        ]
  let oldCase = expectedCase ("0000" :: String) ("aa" :: String)
  let ignoredDifference = expectedCase ("ffff" :: String) ("aa" :: String)
  let liveDifference = expectedCase ("ffff" :: String) ("ab" :: String)
  unless (expectationsMatch oldCase ignoredDifference) (fail "only newly ignored bytes did not match")
  unless (not (expectationsMatch oldCase liveDifference)) (fail "changed live byte adjacent to ignored bytes was admitted")
  let overlapping = object
        [ "expected" .= object
            [ "memory" .=
                [ object ["region" .= ("stack" :: String), "offset" .= (0 :: Int), "hex" .= ("0000" :: String)]
                , object ["region" .= ("stack" :: String), "offset" .= (1 :: Int), "hex" .= ("00" :: String)]
                ]
            ]
        ]
  let malformed = object
        [ "expected" .= object
            [ "memory" .= [object ["region" .= ("stack" :: String), "offset" .= (0 :: Int), "hex" .= ("0" :: String)]]
            ]
        ]
  unless (not (expectationsMatch oldCase overlapping)) (fail "overlapping expectations were admitted")
  unless (not (expectationsMatch oldCase malformed)) (fail "malformed expectations were admitted")
