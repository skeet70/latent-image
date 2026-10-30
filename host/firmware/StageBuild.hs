{-# LANGUAGE OverloadedStrings #-}
-- Shared command flow for staging an owned image into one boot-record slot of
-- a reviewed X-M1 container.
module StageBuild
  ( StageContract (..)
  , StageDeclarations (..)
  , runStage
  ) where

import Control.Monad (forM_, unless)
import qualified Data.ByteString as B
import Data.Char (toLower)
import Data.Word (Word32)
import FujiAF.FirstStage
import qualified FujiAF.Host as H
import qualified FujiAF.UpdateContainer as U
import Numeric (readHex)
import System.Environment (getArgs)
import System.Exit (exitFailure)
import System.IO (hPutStrLn, stderr)

data StageDeclarations = StageDeclarations
  { declaredRetained :: [RetainedExtent]
  , declaredSummary :: [String]
  }

data StageContract = StageContract
  { contractRecord :: BootRecord
  , contractTitle :: String
  , contractProgram :: String
  , contractEntrySymbol :: String
  , contractCandidateSymbols :: [String]
  , contractDeclarations :: [Symbol] -> Either String StageDeclarations
  }

maxContainer :: Int
maxContainer = 64 * 1024 * 1024

data Options = Options
  { optStock :: FilePath
  , optImage :: FilePath
  , optSymbols :: FilePath
  , optOutput :: FilePath
  , optEntry :: Maybe ArmEntry
  , optLoadOffset :: Maybe SlotOffset
  , optExpectedSlotSha256 :: Maybe Sha256
  }

usage :: StageContract -> String
usage contract =
  "usage: " ++ contractProgram contract ++ " STOCK_DAT CANDIDATE_BIN CANDIDATE_SYMBOLS OUT_DAT "
    ++ "--load-offset HEX --entry HEX --expect-slot-sha256 HEX"

parseNumber :: String -> Maybe Word32
parseNumber text =
  case readHex (dropPrefix text) of
    [(value, "")]
      | value >= 0 && value <= 0xffffffff -> Just (fromIntegral (value :: Integer))
    _ -> Nothing
  where
    dropPrefix value
      | take 2 value `elem` ["0x", "0X"] = drop 2 value
      | otherwise = value

parseOptions :: StageContract -> [String] -> Either String Options
parseOptions _ (stock : image : symbols : output : rest) =
  go
    ( Options
        { optStock = stock
        , optImage = image
        , optSymbols = symbols
        , optOutput = output
        , optEntry = Nothing
        , optLoadOffset = Nothing
        , optExpectedSlotSha256 = Nothing
        }
    )
    rest
  where
    go options ("--load-offset" : value : more) =
      case parseNumber value >>= eitherToMaybe . slotOffset . fromIntegral of
        Just offset -> go (options {optLoadOffset = Just offset}) more
        Nothing -> Left ("--load-offset is not a slot offset: " ++ value)
    go options ("--entry" : value : more) =
      case parseNumber value >>= eitherToMaybe . armEntry of
        Just entry -> go (options {optEntry = Just entry}) more
        Nothing -> Left ("--entry is not a word-aligned address: " ++ value)
    go options ("--expect-slot-sha256" : value : more) =
      case parseSha256 (map toLower value) of
        Right digest -> go (options {optExpectedSlotSha256 = Just digest}) more
        Left message -> Left message
    go options [] =
      case (optEntry options, optLoadOffset options, optExpectedSlotSha256 options) of
        (Just _, Just _, Just _) -> Right options
        (Nothing, _, _) -> Left "--entry is required"
        (_, Nothing, _) -> Left "--load-offset is required"
        (_, _, Nothing) -> Left "--expect-slot-sha256 is required"
    go _ (unexpected : _) = Left ("unexpected argument: " ++ unexpected)
parseOptions contract _ = Left (usage contract)

eitherToMaybe :: Either String a -> Maybe a
eitherToMaybe = either (const Nothing) Just

runChecks :: String -> [U.Check] -> IO ()
runChecks title results = do
  putStrLn (title ++ ": " ++ show (length (filter U.checkPassed results)) ++ "/" ++ show (length results) ++ " checks pass")
  forM_ [check | check <- results, not (U.checkPassed check)] $ \check ->
    hPutStrLn
      stderr
      ("FAIL " ++ U.checkField check ++ ": stored " ++ U.checkStored check ++ ", expected " ++ U.checkExpected check)
  unless (all U.checkPassed results) exitFailure

verifySource :: U.Parsed -> IO ()
verifySource parsed = do
  runChecks "source contract" (U.checks parsed)
  runChecks "source carried parts" (U.carriedChecks parsed)

runStage :: StageContract -> IO ()
runStage contract = do
  arguments <- getArgs
  options <- either H.failH pure (parseOptions contract arguments)
  entry <- maybe (H.failH "--entry is required") pure (optEntry options)
  loadOffset <- maybe (H.failH "--load-offset is required") pure (optLoadOffset options)
  let record = contractRecord contract
  stock <- H.readRegular maxContainer (optStock options)
  parsed <- either H.failH pure (U.parseContainer stock)
  verifySource parsed
  slot <- either H.failH pure (recordSlotBytes record (U.parsedParts parsed))
  digest <- H.sha256 slot
  slotDigest <- either H.failH pure (parseSha256 digest)
  expectedDigest <- maybe (H.failH "--expect-slot-sha256 is required") pure (optExpectedSlotSha256 options)
  H.ensure
    (slotDigest == expectedDigest)
    ("reviewed record digest mismatch: source slot is " ++ sha256Hex slotDigest)
  image <- H.readRegular firstRecordSlotLength (optImage options)
  symbols <- H.readRegular 0x10000 (optSymbols options)
  symbolRows <- either H.failH pure (parseSymbols symbols)
  entrySymbol <- either H.failH pure (lookupSymbol symbolRows (contractEntrySymbol contract))
  let load = recordGuestBase record + slotOffsetBytes loadOffset
      codeEnd = load + B.length image
      withinCandidate symbol = fromIntegral (symbolAddress symbol) >= load && fromIntegral (symbolAddress symbol) < codeEnd
  H.ensure
    (symbolAddress entrySymbol == armEntryAddress entry)
    "entry symbol address differs from --entry"
  H.ensure
    (fromIntegral (armEntryAddress entry) == load)
    "entry is not the candidate image start"
  H.ensure (symbolSize entrySymbol > 0) "entry symbol has zero size"
  forM_ (contractCandidateSymbols contract) $ \name -> do
    symbol <- either H.failH pure (lookupSymbol symbolRows name)
    H.ensure (withinCandidate symbol) (name ++ " is outside the candidate image")
  declarations <- either H.failH pure (contractDeclarations contract symbolRows)
  let retained = declaredRetained declarations
  plan <- either H.failH pure (stagePlan record entry loadOffset (U.MediaImage image) retained)
  patched <- either H.failH pure (replaceRecordSlot plan (U.parsedParts parsed))
  rebuilt <- either H.failH pure (U.rebuildContainer parsed patched)
  reparsed <- either H.failH pure (U.parseContainer rebuilt)
  verifySource reparsed
  either H.failH pure (verifyStagedRecord plan parsed reparsed)
  stagedSlot <- either H.failH pure (recordSlotBytes record (U.parsedParts reparsed))
  stagedDigest <- H.sha256 stagedSlot
  digestOut <- H.sha256 rebuilt
  B.writeFile (optOutput options) rebuilt
  mapM_ putStrLn (declaredSummary declarations)
  putStrLn
    ( "staged "
        ++ contractTitle contract
        ++ ": "
        ++ show (B.length image)
        ++ " bytes at slot +"
        ++ U.showHexWord (fromIntegral (slotOffsetBytes loadOffset))
        ++ ", entry "
        ++ U.showHexWord (armEntryAddress entry)
        ++ ", retained "
        ++ show (map retainedExtentRange retained)
        ++ ", staged slot sha256 "
        ++ stagedDigest
        ++ ", output "
        ++ optOutput options
        ++ ", sha256 "
        ++ digestOut
    )
