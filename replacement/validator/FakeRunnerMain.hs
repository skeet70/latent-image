module Main where

import Control.Exception (evaluate)
import Data.List (intercalate)
import Text.Read (readMaybe)
import System.Environment (lookupEnv)
import System.Exit (ExitCode (..), exitWith)
import System.IO (hGetContents, stdin)

main :: IO ()
main = do
  wire <- hGetContents stdin
  _ <- evaluate (length wire)
  mode <- maybe "" id <$> lookupEnv "FAKE_RUNNER_MODE"
  case mode of
    "nonzero-exit-returned" -> do
      putStrLn "{\"wire_version\":1,\"status\":\"returned\",\"unicorn_error\":0}"
      exitWith (ExitFailure 1)
    "returned-nonzero-unicorn" ->
      putStrLn "{\"wire_version\":1,\"status\":\"returned\",\"unicorn_error\":1}"
    "fractional-unicorn" ->
      putStrLn "{\"wire_version\":1,\"status\":\"returned\",\"unicorn_error\":0.5}"
    "anchor-mismatch" -> case parseWire wire of
      Just (registers, exit, regions) -> putStrLn (mismatchJson registers exit regions)
      Nothing -> putStrLn "{\"wire_version\":1,\"status\":\"returned\",\"unicorn_error\":0}"
    "negative-counter" ->
      putStrLn "{\"wire_version\":1,\"status\":\"returned\",\"unicorn_error\":0,\"instructions\":\"-1\"}"
    _ -> putStrLn "{\"wire_version\":1,\"status\":\"returned\",\"unicorn_error\":0}"

parseWire :: String -> Maybe ([String], String, [[String]])
parseWire text = case lines text of
  (_ : header : registerLine : countLine : rest) ->
    case (words header, words registerLine, words countLine) of
      ([_, exit, _, _], registers, (count : _))
        | length registers >= 16
        , Just regions <- readMaybe count ->
            Just (registers, exit, take regions (map words rest))
      _ -> Nothing
  _ -> Nothing

mismatchJson :: [String] -> String -> [[String]] -> String
mismatchJson registers exit regions =
  "{\"wire_version\":1,\"status\":\"returned\",\"unicorn_error\":0,\"instructions\":\"1\",\"memory_provenance_total\":\"0\",\"memory_provenance\":[],\"registers\":{" ++ registerJson ++ "},\"memory\":[" ++ memoryJson ++ "]}"
  where
    labels = ["r0","r1","r2","r3","r4","r5","r6","r7","r8","r9","r10","r11","r12","sp","lr","cpsr"]
    overridden = case registers of
      (_ : rest) -> "0x00000001" : rest
      [] -> []
    registerJson =
      intercalate ","
        (["\"" ++ label ++ "\":\"" ++ value ++ "\"" | (label, value) <- zip labels overridden] ++ ["\"pc\":\"" ++ exit ++ "\""])
    memoryJson =
      intercalate ","
        [ "{\"address\":\"" ++ address ++ "\",\"bytes\":\"" ++ bytes ++ "\"}"
        | (address : _ : _ : bytes : _) <- regions
        ]
