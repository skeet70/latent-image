{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}

-- | The stock census: one graph over the declared function starts of the
-- declared listings, with task, interrupt and callback roots. Prints a summary
-- and writes the graph as JSON; with @--reach-from@ and @--extents@ it writes
-- the reach of those entries in the extent format @//emulation:coverage@ reads.
--
-- A census without its firmware inputs reports @not-run@ and exits 2. It never
-- reports an empty reach as a result.
module Main (main) where

import Control.Monad (foldM, unless, when)
import Data.Aeson (encode, object, (.=))
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as BL
import Data.List (isPrefixOf)
import Data.Maybe (fromMaybe)
import Data.Word (Word32)
import Emulation.Census
import Emulation.Sha256 (sha256File, sha256Hex)
import FujiAF.Host (failH)
import Numeric (readHex)
import System.Directory (doesFileExist)
import System.Environment (getArgs)
import System.Exit (exitWith, ExitCode (..))
import System.FilePath (takeFileName)
import System.IO (hPutStrLn, stderr)

data Options = Options
  { optionListings :: [(String, FilePath)]
  , optionFunctions :: Maybe FilePath
  , optionJson :: Maybe FilePath
  , optionExtents :: Maybe FilePath
  , optionReachFrom :: [Word32]
  , optionEdgeKinds :: EdgeSelection
  }

main :: IO ()
main = do
  arguments <- getArgs
  options <- parseOptions arguments
  when (null (optionListings options) || optionFunctions options == Nothing) $ do
    BL.putStr (encode (object
      [ "status" .= ("not-run" :: String)
      , "reason" .= ("the firmware listings and a function-start table are declared inputs" :: String)
      ]))
    hPutStrLn stderr "census: not run: no --listing or no --functions"
    exitWith (ExitFailure 2)
  listings <- mapM readListing (optionListings options)
  functions <- case optionFunctions options of
    Nothing -> failH "--functions is required"
    Just path -> do
      exists <- doesFileExist path
      unless exists (failH ("--functions is not a file: " ++ path))
      bytes <- B.readFile path
      either failH pure (parseFunctions bytes)
  assigned <- either failH pure (assignImages listings functions)
  let census = buildCensus (optionEdgeKinds options) listings assigned
  putStr (censusTable census)
  inputs <- mapM inputBinding (optionListings options)
  case optionJson options of
    Nothing -> pure ()
    Just path -> BL.writeFile path (encode (object
      [ "inputs" .= inputs
      , "census" .= censusJson census
      ]))
  case optionExtents options of
    Nothing -> pure ()
    Just path -> do
      when (null (optionReachFrom options)) (failH "--extents needs --reach-from")
      value <- either failH pure (extentsJson census listings (optionReachFrom options))
      BL.writeFile path (encode value)
  where
    readListing (name, path) = do
      exists <- doesFileExist path
      unless exists (failH ("--listing is not a file: " ++ path))
      bytes <- B.readFile path
      either failH pure (parseListing name bytes)
    inputBinding (name, path) = do
      digest <- sha256File path
      pure (object ["name" .= name, "path" .= takeFileName path, "sha256" .= sha256Hex digest])

parseOptions :: [String] -> IO Options
parseOptions = go (Options [] Nothing Nothing Nothing [] allEdgeKinds)
  where
    go options [] = pure options
    go options ("--listing" : value : rest) = case break (== '=') value of
      (name, '=' : path) | not (null name) -> go options { optionListings = optionListings options ++ [(name, path)] } rest
      _ -> failH "--listing needs NAME=PATH"
    go options ("--functions" : value : rest) = go options { optionFunctions = Just value } rest
    go options ("--json" : value : rest) = go options { optionJson = Just value } rest
    go options ("--extents" : value : rest) = go options { optionExtents = Just value } rest
    go options ("--reach-from" : value : rest) = do
      entries <- mapM parseAddress (splitCommas value)
      go options { optionReachFrom = optionReachFrom options ++ entries } rest
    go options ("--edge-kinds" : value : rest) = do
      selection <- parseEdgeKinds (splitCommas value)
      go options { optionEdgeKinds = selection } rest
    go _ (unknown : _) = failH ("unknown argument: " ++ unknown)

    splitCommas value = case break (== ',') value of
      (headValue, ',' : tailValue) -> headValue : splitCommas tailValue
      (headValue, _) -> [headValue]

parseAddress :: String -> IO Word32
parseAddress text = case readHex (strip text) of
  [(value, "")] | value <= toInteger (maxBound :: Word32) -> pure (fromIntegral (value :: Integer))
  _ -> failH ("not an address: " ++ text)
  where strip value = fromMaybe value (stripPrefix "0x" value)
        stripPrefix prefix value = if prefix `isPrefixOf` value then Just (drop (length prefix) value) else Nothing

parseEdgeKinds :: [String] -> IO EdgeSelection
parseEdgeKinds kinds = do
  selection <- foldM add (EdgeSelection False False False) kinds
  when (not (selectDirect selection || selectTable selection || selectCodeWord selection)) $
    failH "--edge-kinds names none of direct, table, code-word"
  pure selection
  where
    add selection kind = case kind of
      "direct" -> pure selection { selectDirect = True }
      "table" -> pure selection { selectTable = True }
      "code-word" -> pure selection { selectCodeWord = True }
      "all" -> pure allEdgeKinds
      _ -> failH ("unknown edge kind: " ++ kind)
