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
