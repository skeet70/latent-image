{-# LANGUAGE OverloadedStrings #-}
module Main (main) where

import Control.Monad (unless)
import Data.Aeson (Value, object, (.=))
import qualified Data.Text as T
import Emulation.ObservationCompare (differences, e2eIgnored, normalize, rendezvousEntryFiles, toggleIgnored, withoutOverlayExit)
import System.Exit (exitFailure)

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

step :: T.Text -> [Int] -> Value
step key state = object ["key" .= key, "expected" .= state, "actual" .= state, "seconds" .= (0.5 :: Double)]

observation :: [Value] -> [T.Text] -> Value
observation steps messages = object ["phase" .= ("cc-film-list-open" :: T.Text), "steps" .= steps, "menu_messages" .= messages]

-- | An observation that follows a rendezvous, RETAINED against CANDIDATE.
entryCompared :: Value -> Value -> [String]
entryCompared retained candidate =
  let (a, b) = (normalize e2eIgnored retained, normalize e2eIgnored candidate)
   in differences "observation" (withoutOverlayExit a b) b

opened :: [Value]
opened = [step "menu" [1, 5, 2], step "down" [1, 5, 3]]

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
  , ( "a retained MENU that leaves the overlay before the candidate's steps is left out"
    , null (entryCompared (observation (step "menu" [67, 56, 0] : opened) []) (observation opened [])) )
  , ( "a retained leading key other than MENU is compared"
    , not (null (entryCompared (observation (step "up" [67, 56, 0] : opened) []) (observation opened []))) )
  , ( "a retained leading MENU with different later steps is compared"
    , not (null (entryCompared (observation (step "menu" [67, 56, 0] : opened) []) (observation [step "menu" [1, 5, 2]] []))) )
  , ( "an observation whose only step is the MENU is compared"
    , not (null (entryCompared (observation [step "menu" [67, 56, 0]] []) (observation [] []))) )
  , ( "other fields of an entry observation are still compared"
    , not (null (entryCompared (observation (step "menu" [67, 56, 0] : opened) ["0x12"]) (observation opened []))) )
  , ( "equal entry observations stay equal"
    , null (entryCompared (observation opened []) (observation opened [])) )
  , ( "a result's rendezvous entries name their observation files"
    , rendezvousEntryFiles (object ["rendezvous_entry_observations" .= [0, 6 :: Int], "observations" .= ([] :: [Value])])
        == ["e2e-observation-0.json", "e2e-observation-6.json"] )
  , ( "a result without rendezvous entries names no observation file"
    , null (rendezvousEntryFiles (object ["observations" .= ([] :: [Value])])) )
  ]

main :: IO ()
main = do
  let failed = [name | (name, ok) <- cases, not ok]
  mapM_ (putStrLn . ("FAIL: " ++)) failed
  unless (null failed) exitFailure
  putStrLn (show (length cases) ++ " observation-compare cases passed")
