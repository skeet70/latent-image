{-# LANGUAGE OverloadedStrings #-}
module Main (main) where

import Control.Monad (unless)
import Data.Aeson (Value, object, (.=))
import qualified Data.Text as T
import Emulation.ObservationCompare (differences, e2eIgnored, normalize, toggleIgnored)
import System.Exit (exitFailure)

segment :: [T.Text] -> Value
segment entries = object ["segment" .= ("cc-film-list" :: T.Text), "classic_chrome_site_entries" .= entries]

compared :: Value -> Value -> [String]
compared a b = differences "observation" (normalize e2eIgnored a) (normalize e2eIgnored b)

pass' :: [T.Text]
pass' = ["cc-entry-marker", "cc-entry-builder"]

converting :: [T.Text]
converting = ["cc-entry-marker", "cc-entry-settings-converter"]

-- | A develop pass holding two converter entries, and the idle pass that splits it.
twoConverters :: [T.Text]
twoConverters = ["cc-entry-marker", "cc-entry-settings-converter", "cc-entry-settings-converter"]

idlePass :: [T.Text]
idlePass = ["cc-entry-marker"]

fourConverters :: [T.Text]
fourConverters = twoConverters ++ ["cc-entry-settings-converter", "cc-entry-settings-converter"]

-- | The same four converter entries with the capture converter first.
reordered :: [T.Text]
reordered =
  [ "cc-entry-marker", "cc-entry-capture-converter", "cc-entry-settings-converter"
  , "cc-entry-settings-converter", "cc-entry-settings-converter" ]

threeConverters :: [T.Text]
threeConverters = ["cc-entry-marker", "cc-entry-settings-converter", "cc-entry-settings-converter", "cc-entry-settings-converter"]

fiveConverters :: [T.Text]
fiveConverters = fourConverters ++ ["cc-entry-settings-converter"]

markedPass :: [T.Text]
markedPass =
  [ "cc-entry-marker", "cc-hook-chroma-2", "cc-entry-settings-converter", "cc-entry-settings-converter" ]

markedUnsplit :: [T.Text]
markedUnsplit = markedPass ++ ["cc-entry-settings-converter", "cc-entry-settings-converter"]

prefixExtra :: [T.Text]
prefixExtra =
  [ "cc-entry-marker", "cc-hook-chroma-2", "cc-entry-filmmode"
  , "cc-entry-settings-converter", "cc-entry-settings-converter" ]

bodyExtra :: [T.Text]
bodyExtra =
  [ "cc-entry-marker", "cc-hook-chroma-2", "cc-entry-settings-converter"
  , "cc-entry-filmmode", "cc-entry-settings-converter" ]

reorderedPrefix :: [T.Text]
reorderedPrefix =
  [ "cc-entry-marker", "cc-hook-tone-2", "cc-hook-tone-1", "cc-entry-settings-converter" ]

orderedPrefix :: [T.Text]
orderedPrefix = ["cc-entry-marker", "cc-hook-tone-1", "cc-hook-tone-2", "cc-entry-settings-converter"]

missingPrefix :: [T.Text]
missingPrefix = ["cc-entry-marker", "cc-entry-settings-converter"]

chromaPrefix :: [T.Text]
chromaPrefix = ["cc-entry-marker", "cc-hook-chroma-2", "cc-entry-settings-converter"]

cases :: [(String, Bool)]
cases =
  [ ( "converter-free segments with different entries differ"
    , not (null (compared (segment pass') (segment (pass' ++ ["cc-entry-builder"])))) )
  , ( "converter-free segments with equal entries are equal"
    , null (compared (segment pass') (segment pass')) )
  , ( "a converter-free segment differs from one whose only pass converts"
    , not (null (compared (segment pass') (segment converting))) )
  , ( "repeated develop passes around a converter pass are equal to one"
    , null (compared (segment (pass' ++ converting ++ pass')) (segment (pass' ++ pass' ++ converting ++ pass' ++ pass'))) )
  , ( "converting segments with different converter passes differ"
    , not (null (compared (segment converting) (segment (converting ++ ["cc-entry-builder"])))) )
  , ( "a converter pass split by idle passes equals the unsplit pass"
    , null (compared (segment (twoConverters ++ idlePass ++ twoConverters)) (segment fourConverters)) )
  , ( "a split whose converter entries run in a different order differs"
    , not (null (compared (segment (twoConverters ++ idlePass ++ twoConverters)) (segment reordered))) )
  , ( "a split with a different converter entry total differs"
    , not (null (compared (segment (twoConverters ++ idlePass ++ twoConverters)) (segment threeConverters))) )
  , ( "a continuation with an extra declared prefix entry differs"
    , not (null (compared (segment (markedPass ++ idlePass ++ prefixExtra)) (segment markedUnsplit))) )
  , ( "a continuation with an extra declared body entry differs"
    , not (null (compared (segment (markedPass ++ idlePass ++ bodyExtra)) (segment markedUnsplit))) )
  , ( "a continuation with a reordered prefix differs"
    , not (null (compared
        (segment (orderedPrefix ++ idlePass ++ reorderedPrefix))
        (segment (orderedPrefix ++ ["cc-entry-settings-converter"])))) )
  , ( "a continuation with a missing prefix entry differs"
    , not (null (compared
        (segment (chromaPrefix ++ idlePass ++ missingPrefix))
        (segment (chromaPrefix ++ ["cc-entry-settings-converter"])))) )
  , ( "a split with a converting pass in between differs"
    , not (null (compared (segment (twoConverters ++ converting ++ twoConverters)) (segment fiveConverters))) )
  , ( "timing fields are left out"
    , null (compared (object ["instructions" .= (1 :: Int), "r0" .= ("0x1" :: T.Text)])
                     (object ["instructions" .= (2 :: Int), "r0" .= ("0x1" :: T.Text)])) )
  , ( "a register value is compared"
    , not (null (compared (object ["r0" .= ("0x1" :: T.Text)]) (object ["r0" .= ("0x2" :: T.Text)]))) )
  , ( "toggle receipts leave out the engine and checkpoint identity"
    , null (differences "receipt"
        (normalize toggleIgnored (object ["engine" .= ("retained" :: T.Text), "emulator" .= ("a" :: T.Text), "result" .= ("pass" :: T.Text)]))
        (normalize toggleIgnored (object ["engine" .= ("fujimod" :: T.Text), "emulator" .= ("b" :: T.Text), "result" .= ("pass" :: T.Text)]))) )
  , ( "toggle receipts compare results"
    , not (null (differences "receipt"
        (normalize toggleIgnored (object ["result" .= ("pass" :: T.Text)]))
        (normalize toggleIgnored (object ["result" .= ("blocked" :: T.Text)])))) )
  ]

main :: IO ()
main = do
  let failed = [name | (name, ok) <- cases, not ok]
  mapM_ (putStrLn . ("FAIL: " ++)) failed
  unless (null failed) exitFailure
  putStrLn (show (length cases) ++ " observation-compare cases passed")
