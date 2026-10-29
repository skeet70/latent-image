module Main (main) where

import Data.List (nub)
import Data.Word (Word32)
import FujiAF.Host (ensure, failH)
import Replacement.Save.Domain

main :: IO ()
main = do
  let scenarios = scenarioDomain
      identifiers = map scenarioId scenarios
  ensure (length identifiers == length (nub identifiers)) "duplicate scenario identifiers"
  ensure (length scenarios >= 90) "scenario domain is incomplete"
  checkAdmission
  checkKinds
  checkCompletion
  putStrLn ("scenario domain: " ++ show (length scenarios) ++ " cases")

expectationOf :: String -> Expectation
expectationOf identifier =
  case [expectation scenario | scenario <- scenarioDomain, scenarioId scenario == identifier] of
    (value : _) -> value
    [] -> error ("missing scenario " ++ identifier)

hasEvent :: Word32 -> Expectation -> Bool
hasEvent address expected = any (\(Event site _ _) -> site == address) (expectedEvents expected)

shortcutOf :: String -> Bool
shortcutOf = expectedShortcut . expectationOf

checkAdmission :: IO ()
checkAdmission = do
  ensure (shortcutOf "s-admit-p-d1-qa2-st0") "profile a2 did not shortcut"
  ensure (shortcutOf "s-admit-p-d1-qa4-st0") "profile a4 did not shortcut"
  ensure (shortcutOf "s-admit-p-d1-q83-st0") "profile 0x83 did not shortcut"
  ensure (shortcutOf "s-admit-p-d1-q87-st0") "profile 0x87 did not shortcut"
  ensure (not (shortcutOf "s-admit-p-d1-q82-st0")) "profile 0x82 shortcut"
  ensure (not (shortcutOf "s-admit-p-d1-q88-st0")) "profile 0x88 shortcut"
  ensure (not (shortcutOf "s-admit-p-d1-q80-st0")) "profile 0x80 shortcut"
  ensure (not (shortcutOf "s-admit-p-d0-qa2-st0")) "dispatch byte 0 shortcut"
  ensure (shortcutOf "s-admit-p-d0-qa5-st0") "profile a5 did not shortcut"
  ensure (shortcutOf "s-admit-p-d0-qa7-st0") "profile a7 did not shortcut"
  ensure (shortcutOf "s-admit-p-d0-qa9-st0") "profile a9 did not shortcut"
  ensure (shortcutOf "s-admit-p-d0-qa8-st0") "profile a8 did not shortcut"
  let a2Events = length (expectedEvents (expectationOf "s-admit-p-d1-qa2-st0"))
      q83Events = length (expectedEvents (expectationOf "s-admit-p-d1-q83-st0"))
      q88Events = length (expectedEvents (expectationOf "s-admit-p-d1-q88-st0"))
      a5Events = length (expectedEvents (expectationOf "s-admit-p-d0-qa5-st0"))
  ensure (a2Events == 3) ("a2 selector events: " ++ show a2Events)
  ensure (q83Events == 5) ("0x83 selector events: " ++ show q83Events)
  ensure (q88Events == 19) ("0x88 selector and body events: " ++ show q88Events)
  ensure (a5Events == 4) ("a5 selector events: " ++ show a5Events)
  ensure (expectedStatus (expectationOf "s-admit-p-d1-qa2-st2") == 2) "stale status not handed back"

checkKinds :: IO ()
checkKinds = do
  ensure (expectedMode (expectationOf "s-kind-k2-code0-st0") == 3) "kind 2 mode"
  ensure (expectedMode (expectationOf "s-kind-k3-code0-st0") == 3) "kind 3 mode"
  ensure (expectedVariant (expectationOf "s-kind-k3-code0-st0") == Just 1) "kind 3 storage variant"
  ensure (expectedVariant (expectationOf "s-kind-k2-code0-st0") == Just 0) "kind 2 storage variant"
  ensure (expectedMode (expectationOf "s-kind-k4-code0-st0") == 1) "kind 4 mode"
  ensure (expectedMode (expectationOf "s-kind-k1-code0-st0") == 0) "kind 1 mode"
  ensure (expectedMetadata (expectationOf "s-kind-k0-code0-st0") == Just 0) "kind 0 metadata"
  ensure (expectedMetadata (expectationOf "s-gate-k0-s1-c0-p0-l2") == Just 1) "kind 0 state metadata"
  ensure (hasEvent 0x011d5934 (expectationOf "s-extent-k1-l2")) "kind 1 layout 2 lost the extent name"
  ensure (not (hasEvent 0x011d5934 (expectationOf "s-extent-k1-l0"))) "kind 1 layout 0 named an extent"
  ensure (hasEvent 0x011d591c (expectationOf "s-extent-k2-l2")) "kind 2 layout 2 lost the extent copy"
  ensure (not (hasEvent 0x011d591c (expectationOf "s-extent-k3-l2"))) "kind 3 copied the extent name"
  ensure (not (hasEvent 0x011d591c (expectationOf "s-extent-k2-l0"))) "kind 2 layout 0 copied the extent name"

checkCompletion :: IO ()
checkCompletion = do
  ensure (expectedStatus (expectationOf "s-kind-k2-code-minus4-st0") == 1) "code -4 status"
  ensure (expectedStatus (expectationOf "s-kind-k2-code-minus1-st0") == 2) "other code status"
  ensure (expectedStatus (expectationOf "s-kind-k2-code0-minus4-st0") == 1) "last completion did not win"
  ensure (expectedStatus (expectationOf "s-kind-k2-code-minus4-0-st0") == 0) "last completion did not win"
  ensure (expectedReturn (expectationOf "s-kind-k2-code0-interrupt-st0") == 0) "interruption returned success"
  ensure (expectedPhaseTwo (expectationOf "s-kind-k2-code0-interrupt-st0")) "interruption skipped phase two"
  ensure (expectedStatus (expectationOf "s-kind-k2-code-minus4-interrupt-st0") == 1) "failed status entered phase two"
  ensure (not (expectedPhaseTwo (expectationOf "s-kind-k2-lost-stale-st2"))) "stale failed status entered phase two"
  ensure (expectedPhaseTwo (expectationOf "s-kind-k2-lost-fresh-st0")) "fresh interruption skipped phase two"
  ensure (expectedReturn (expectationOf "lifecycle-late-callback") == 0) "lifecycle return"
  ensure (expectedPhaseTwo (expectationOf "lifecycle-late-callback")) "lifecycle skipped phase two"
  ensure (expectedStatus (expectationOf "lifecycle-late-callback") == 1) "late phase-one status"
  ensure (expectedHandback (expectationOf "lifecycle-late-callback") == 0) "late callback changed the handback"
  ensure (length (expectedEvents (expectationOf "reentry-interrupt-phase-two")) == 16) "reentry event count"
  case [() | scenario <- scenarioDomain, null (expectedEvents (expectationOf (scenarioId scenario))), not (expectedShortcut (expectationOf (scenarioId scenario)))] of
    [] -> pure ()
    _ -> failH "a non-shortcut scenario has no events"
