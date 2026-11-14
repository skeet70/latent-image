-- | Reports linked image bytes per crate and section.
module Main where

import Control.Monad (forM_, unless)
import Data.Char (isDigit, isHexDigit)
import Data.List (isPrefixOf, sort)
import qualified Data.Map.Strict as M
import Numeric (readHex)
import System.Environment (getArgs)
import System.Exit (exitFailure)

data Row = Row
  { rowCrate :: String
  , rowSection :: String
  , rowBytes :: Int
  }

check :: String -> Bool -> IO ()
check label passed = unless passed $ do
  putStrLn ("FAIL: " ++ label)
  exitFailure

hexValue :: String -> Maybe Int
hexValue text = case readHex text of
  [(value, "")] | all isHexDigit text -> Just value
  _ -> Nothing

splitMember :: String -> (String, String)
splitMember text =
  let (_, after) = break (== '(') text
      member = takeWhile (/= ')') (drop 1 after)
      tail' = dropWhile (/= ')') after
      section = takeWhile (/= ')') (drop 3 tail')
  in (member, section)

crateOf :: String -> String
crateOf member = case break (== '.') member of
  (first, '.' : rest) | "lib" `isPrefixOf` first -> takeWhile (/= '.') rest
  (first, _) -> takeWhile (/= '-') first

outputOf :: String -> String
outputOf section
  | ".menu_engine_data." `isPrefixOf` section = ".menu_engine_data"
  | ".menu_engine." `isPrefixOf` section = ".menu_engine"
  | ".text" `isPrefixOf` section = ".text"
  | ".rodata" `isPrefixOf` section = ".text"
  | ".data" `isPrefixOf` section = ".data"
  | ".bss" `isPrefixOf` section = ".data"
  | section == "COMMON" = ".data"
  | otherwise = "other"

parseRow :: String -> Maybe Row
parseRow line = case words line of
  vma : lma : size : align : rest
    | not (null rest)
    , Just bytes <- hexValue size
    , Just _ <- hexValue vma
    , Just _ <- hexValue lma
    , all isDigit align
    , let (member, section) = splitMember (unwords rest)
    , not (null member)
    , "." `isPrefixOf` section
    , bytes > 0 ->
        Just (Row (crateOf member) (outputOf section) bytes)
  _ -> Nothing

aggregate :: [Row] -> M.Map (String, String) Int
aggregate = foldl' (\table row -> M.insertWith (+) (rowCrate row, rowSection row) (rowBytes row) table) M.empty

sections :: [String]
sections = [".menu_engine", ".menu_engine_data", ".text", ".data", "other"]

renderTable :: String -> M.Map (String, String) Int -> IO ()
renderTable variant table = do
  let crates = sort (map fst (M.keys (M.fromList [((crate, ()), ()) | ((crate, _), _) <- M.toList table])))
  putStrLn ("owned image bytes per crate and section: " ++ variant ++ " at base 0xa81be0")
  putStrLn (pad 30 "crate" ++ concatMap (pad 14) sections ++ pad 14 "total")
  forM_ crates $ \crate -> do
    let bytes section = M.findWithDefault 0 (crate, section) table
        total = sum (map bytes sections)
    putStrLn (pad 30 crate ++ concatMap (pad 14 . show . bytes) sections ++ pad 14 (show total))
  let bytes section = sum [n | ((_, found), n) <- M.toList table, found == section]
  putStrLn (pad 30 "total" ++ concatMap (pad 14 . show . bytes) sections ++ pad 14 (show (sum (map bytes sections))))

pad :: Int -> String -> String
pad width text = text ++ replicate (max 1 (width - length text)) ' '


runVariant :: String -> FilePath -> IO ()
runVariant variant path = do
  contents <- readFile path
  let rows = [row | line <- lines contents, Just row <- [parseRow line]]
      table = aggregate rows
  check (variant ++ ": the link map names input sections") (not (null rows))
  renderTable variant table

main :: IO ()
main = do
  arguments <- getArgs
  case arguments of
    [engineMap, renderMap] -> do
      runVariant "modal75,menu-engine" engineMap
      runVariant "modal75,menu-engine,render" renderMap
    _ -> do
      putStrLn "usage: owned-image-bytes ENGINE.map RENDER.map"
      exitFailure
