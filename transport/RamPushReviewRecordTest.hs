module Main where

import Control.Monad (unless)
import qualified Data.ByteString.Char8 as C
import Data.Either (isLeft)
import Data.IORef
import Data.List (intercalate, isInfixOf)
import System.Exit (exitFailure)
import FujiAF.RamPushReview

reviewedCommit, laterCommit :: String
reviewedCommit = replicate 40 'a'
laterCommit = replicate 40 'b'

digest :: Char -> String
digest = replicate 64

quoted :: String -> String
quoted s = "\"" ++ s ++ "\""

approvedBy :: String -> String
approvedBy reviewName = "{\"verdict\":\"approved\",\"review\":" ++ quoted reviewName ++ ",\"commit\":" ++ quoted reviewedCommit ++ "}"

unreviewed, contentChecked :: String
unreviewed = "{\"verdict\":\"unreviewed\",\"commit\":" ++ quoted laterCommit ++ "}"
contentChecked = "{\"verdict\":\"content-checked\",\"check\":\"Candidate1\"}"

review :: String -> String -> String
review identifier verdict = "{\"id\":" ++ quoted identifier ++ ",\"reviewer\":\"R (model)\",\"session\":\"s.jsonl\",\"verdict\":" ++ quoted verdict
  ++ ",\"scope\":\"installer\",\"date\":\"2026-10-16\",\"commit\":" ++ quoted reviewedCommit ++ "}"

check :: String
check = "{\"id\":\"Candidate1\",\"commit\":" ++ quoted laterCommit ++ ",\"date\":\"2026-10-18\""
  ++ ",\"elf_allowlist_receipt\":{\"path\":\"private-data/a.json\",\"sha256\":" ++ quoted (digest '1') ++ "}"
  ++ ",\"emulator_run\":{\"path\":\"private-data/e.json\",\"sha256\":" ++ quoted (digest '2') ++ "}"
  ++ ",\"owner_visual_check\":\"MAIN.jsonl user 2026-10-18T10:00\"}"

pinWithDigest :: String -> String -> String -> String -> String -> String
pinWithDigest sha identity source component status = "{\"identity\":" ++ quoted identity ++ ",\"source\":" ++ quoted source
  ++ ",\"component\":" ++ quoted component ++ ",\"sha256\":" ++ quoted sha ++ ",\"status\":" ++ status ++ "}"

pin :: String -> String -> String -> String -> String
pin = pinWithDigest (digest 'c')

installerPin, contentPin, builderPin :: String -> String
installerPin = pin "menu/ram_push_installer.c" "workspace" "mechanism"
contentPin = pin "menu/recipe_bank_storage.c" "workspace" "bank-content"
builderPin = pin "build-recipe-bank" "builder" "mechanism"

document :: [String] -> [String] -> [String] -> String -> C.ByteString
document = documentOfKind "xm1-ram-push-review-record"

documentOfKind :: String -> [String] -> [String] -> [String] -> String -> C.ByteString
documentOfKind kind reviews checks pins contract = C.pack $
  "{\"schema\":1,\"kind\":" ++ quoted kind ++ ",\"reviews\":[" ++ intercalate "," reviews
  ++ "],\"content_checks\":[" ++ intercalate "," checks ++ "],\"pins\":[" ++ intercalate "," pins
  ++ "],\"publication_contract\":{\"sha256\":" ++ quoted (digest 'd') ++ ",\"status\":" ++ contract ++ "}}"

subjects :: [GateFinding] -> [Subject]
subjects findings = [subject | Unapproved subject _ <- findings]

main :: IO ()
main = do
  failures <- newIORef (0 :: Int)
  let expect label ok = unless ok (putStrLn ("FAIL: " ++ label) >> modifyIORef' failures (+1))
      parsed = parseReviewRecord
      findingsOf bytes = either (const Nothing) (Just . subjects . verdictFindings) (parsed bytes)
      approvedRecord = document [review "R1" "approve"] [check]
        [installerPin (approvedBy "R1"), contentPin contentChecked, builderPin (approvedBy "R1")] (approvedBy "R1")

  expect "an all-approved record has no verdict findings" (findingsOf approvedRecord == Just [])
  expect "an unreviewed pin is a verdict finding"
    (findingsOf (document [review "R1" "approve"] [] [installerPin (approvedBy "R1"), contentPin unreviewed] (approvedBy "R1"))
      == Just [PinSubject (PinIdentity "menu/recipe_bank_storage.c")])
  expect "an unreviewed publication contract is a verdict finding"
    (findingsOf (document [review "R1" "approve"] [] [installerPin (approvedBy "R1")] unreviewed) == Just [ContractSubject])
  expect "approval by a changes-required review is a verdict finding"
    (findingsOf (document [review "R1" "changes-required"] [] [installerPin (approvedBy "R1")] (approvedBy "R1"))
      == Just [PinSubject (PinIdentity "menu/ram_push_installer.c"), ContractSubject])

  expect "content-checked on a mechanism pin is refused"
    (isLeft (parsed (document [] [check] [installerPin contentChecked] unreviewed)))
  expect "content-checked on the publication contract is refused"
    (isLeft (parsed (document [] [check] [contentPin contentChecked] contentChecked)))
  expect "an approval naming no recorded review is refused"
    (isLeft (parsed (document [] [] [installerPin (approvedBy "R9")] unreviewed)))
  expect "a content check naming no recorded check is refused"
    (isLeft (parsed (document [] [] [contentPin contentChecked] unreviewed)))
  expect "a duplicate pin identity is refused"
    (isLeft (parsed (document [] [] [installerPin unreviewed, installerPin unreviewed] unreviewed)))
  expect "an unknown verdict is refused"
    (isLeft (parsed (document [] [] [installerPin "{\"verdict\":\"looks-fine\",\"commit\":\"" ++ laterCommit ++ "\"}"] unreviewed)))
  expect "an abbreviated commit is refused"
    (isLeft (parsed (document [] [] [installerPin "{\"verdict\":\"unreviewed\",\"commit\":\"29c8ea3\"}"] unreviewed)))
  expect "a workspace pin under private-data is refused"
    (isLeft (parsed (document [] [] [pin "private-data/x.bin" "workspace" "mechanism" unreviewed] unreviewed)))
  expect "an uppercase digest is refused"
    (isLeft (parsed (document [] [] [pinWithDigest (replicate 64 'C') "build-recipe-bank" "builder" "mechanism" unreviewed] unreviewed)))
  expect "an unknown key is refused"
    (isLeft (parsed (document [] [] [init (builderPin unreviewed) ++ ",\"note\":\"x\"}"] unreviewed)))

  let toggleBuilder = pin "build-usb-toggle-ram" "builder" "mechanism" unreviewed
      toggleStub = pin "usb_toggle_stub" "usb-toggle-stub" "mechanism" unreviewed
      toggleRecord = documentOfKind "xm1-usb-toggle-ram-push-review-record" [] [] [installerPin unreviewed, toggleBuilder, toggleStub] unreviewed
  expect "the USB toggle stub source rejects substituted identities"
    (isLeft (parsed (document [] [] [pin "substituted_stub" "usb-toggle-stub" "mechanism" unreviewed] unreviewed)))
  case parsed toggleRecord of
    Left reason -> expect ("the USB toggle record parses: " ++ reason) False
    Right record -> do
      expect "USB toggle review records round trip"
        (recordKind record == UsbToggleRecord && parsed (renderReviewRecord record) == Right record)
      expect "USB toggle pins and publication contract remain unreviewed"
        (subjects (verdictFindings record) == [PinSubject (PinIdentity "menu/ram_push_installer.c"), PinSubject (PinIdentity "build-usb-toggle-ram"), PinSubject (PinIdentity "usb_toggle_stub"), ContractSubject])
      expect "USB toggle generates its camera contract binding"
        ("module RamPushToggleReviewedPins (reviewedToggleContract) where" `isInfixOf` pinsModuleSource record)
  case parsed approvedRecord of
    Left reason -> expect ("the approved fixture parses: " ++ reason) False
    Right record -> do
      expect "a rendered record reads back unchanged" (parsed (renderReviewRecord record) == Right record)
      let commit = either error id (commitId laterCommit)
          candidate = either error id (checkIdentifier "Candidate1")
          storage = PinSubject (PinIdentity "menu/recipe_bank_storage.c")
          fresh = maybe (error "digest") id (sha256Hex (digest 'e'))
      case repin (Unreviewed commit) [(storage, fresh), (ContractSubject, fresh)] record of
        Left reason -> expect ("repin succeeds: " ++ reason) False
        Right updated -> do
          expect "repin marks the named subjects with the new status" (subjects (verdictFindings updated) == [storage, ContractSubject])
          expect "repin sets the named pin's digest"
            ([pinDigest p | p <- recordPins updated, PinSubject (pinIdentity p) == storage] == [fresh])
          expect "repin sets the contract digest" (contractDigest (recordContract updated) == fresh)
      expect "repin of an unrecorded identity is refused"
        (isLeft (repin (Unreviewed commit) [(PinSubject (PinIdentity "menu/other.c"), fresh)] record))
      expect "repin to content-checked on a mechanism pin is refused"
        (isLeft (repin (ContentChecked candidate) [(PinSubject (PinIdentity "menu/ram_push_installer.c"), fresh)] record))

  count <- readIORef failures
  unless (count == 0) exitFailure
  putStrLn "ram-push review record tests passed"
