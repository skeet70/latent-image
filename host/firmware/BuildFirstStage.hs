{-# LANGUAGE OverloadedStrings #-}
-- Stage an owned first-record image into a reviewed X-M1 container.
module Main where

import Control.Monad (forM_, unless, when)
import qualified Data.ByteString as B
import Data.Bits (complement)
import Data.Char (toLower)
import Data.Word (Word32)
import FujiAF.FirstStage
import qualified FujiAF.Host as H
import qualified FujiAF.UpdateContainer as U
import Numeric (readHex)
import System.Environment (getArgs)
import System.Exit (exitFailure)
import System.IO (hPutStrLn, stderr)

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

usage :: String
usage =
  "usage: build-first-stage STOCK_DAT CANDIDATE_BIN CANDIDATE_SYMBOLS OUT_DAT "
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

parseOptions :: [String] -> Either String Options
parseOptions (stock : image : symbols : output : rest) =
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
parseOptions _ = Left usage

eitherToMaybe :: Either String a -> Maybe a
eitherToMaybe = either (const Nothing) Just

retainedContract :: [Symbol] -> Either String [RetainedExtent]
retainedContract symbols = do
  vectorStart <- symbolSlotOffset symbols "xm1_vector_table_start"
  beforeReset <- symbolSlotOffset symbols "xm1_vector_before_reset_end"
  afterReset <- symbolSlotOffset symbols "xm1_vector_after_reset_start"
  vectorEnd <- symbolSlotOffset symbols "xm1_vector_table_end"
  bodyStart <- symbolSlotOffset symbols "xm1_retained_body_start"
  bodyEnd <- symbolSlotOffset symbols "xm1_retained_body_end"
  dataStart <- symbolSlotOffset symbols "xm1_retained_data_start"
  dataEnd <- symbolSlotOffset symbols "xm1_retained_data_end"
  beforeExtent <- retainedExtent vectorStart beforeReset
  afterExtent <- retainedExtent afterReset vectorEnd
  bodyExtent <- retainedExtent bodyStart bodyEnd
  dataExtent <- retainedExtent dataStart dataEnd
  let bodyRange = (slotOffsetBytes bodyStart, slotOffsetBytes bodyEnd)
  forM_
    [ "xm1_retained_core_init"
    , "xm1_retained_storage_init"
    , "xm1_retained_skip_scan"
    , "xm1_retained_segment_table"
    , "xm1_retained_trailer_check"
    , "xm1_retained_stage_images"
    , "xm1_retained_storage_fini"
    ]
    $ \name -> do
      symbol <- lookupSymbol symbols name
      let address = fromIntegral (symbolAddress symbol)
      if address < fst bodyRange || address >= snd bodyRange
        then Left (name ++ " is outside the retained body extent")
        else pure ()
  pure [beforeExtent, afterExtent, bodyExtent, dataExtent]

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

main :: IO ()
main = do
  arguments <- getArgs
  options <- either H.failH pure (parseOptions arguments)
  entry <- maybe (H.failH "--entry is required") pure (optEntry options)
  loadOffset <- maybe (H.failH "--load-offset is required") pure (optLoadOffset options)
  stock <- H.readRegular maxContainer (optStock options)
  parsed <- either H.failH pure (U.parseContainer stock)
  verifySource parsed
  let parts = U.parsedParts parsed
  slot <- either H.failH pure (firstRecordSlotBytes parts)
  digest <- H.sha256 slot
  slotDigest <- either H.failH pure (parseSha256 digest)
  expectedDigest <- maybe (H.failH "--expect-slot-sha256 is required") pure (optExpectedSlotSha256 options)
  H.ensure
    (slotDigest == expectedDigest)
    ("reviewed first-record digest mismatch: source slot is " ++ sha256Hex slotDigest)
  image <- H.readRegular firstRecordSlotLength (optImage options)
  symbols <- H.readRegular 0x10000 (optSymbols options)
  symbolRows <- either H.failH pure (parseSymbols symbols)
  entrySymbol <- either H.failH pure (lookupSymbol symbolRows "xm1_first_stage_entry")
  H.ensure
    (symbolAddress entrySymbol == armEntryAddress entry)
    "entry symbol address differs from --entry"
  H.ensure
    (armEntryAddress entry == fromIntegral (slotOffsetBytes loadOffset))
    "entry is not the candidate image start"
  H.ensure (symbolSize entrySymbol > 0) "entry symbol has zero size"
  handoffSymbol <- either H.failH pure (lookupSymbol symbolRows "xm1_first_stage_handoff")
  handoffVectorSymbol <- either H.failH pure (lookupSymbol symbolRows "xm1_first_stage_handoff_vector")
  let load = slotOffsetBytes loadOffset
      codeEnd = load + B.length image
      withinCandidate symbol = fromIntegral (symbolAddress symbol) >= load && fromIntegral (symbolAddress symbol) < codeEnd
  H.ensure (withinCandidate handoffSymbol) "handoff symbol is outside the candidate image"
  H.ensure (withinCandidate handoffVectorSymbol) "handoff vector symbol is outside the candidate image"
  retained <- either H.failH pure (retainedContract symbolRows)
  plan <- either H.failH pure (firstStagePlan entry loadOffset (U.MediaImage image) retained)
  patched <- either H.failH pure (replaceFirstRecordSlot plan parts)
  rebuilt <- either H.failH pure (U.rebuildContainer parsed patched)
  reparsed <- either H.failH pure (U.parseContainer rebuilt)
  verifySource reparsed
  let rebuiltParts = U.parsedParts reparsed
      sourcePre = U.partPreTable parts
      rebuiltPre = U.partPreTable rebuiltParts
      slotStart = firstRecordSlotOffset
      slotEnd = slotStart + firstRecordSlotLength
  H.ensure
    (B.take slotStart sourcePre == B.take slotStart rebuiltPre)
    "carrier before the first-record slot changed"
  H.ensure
    (B.drop slotEnd sourcePre == B.drop slotEnd rebuiltPre)
    "carrier or record 2 after the first-record slot changed"
  forM_ (U.diffParts parsed reparsed) $ \difference ->
    when
      (U.diffChangedBytes difference /= 0 && U.diffPart difference /= "main pre-table")
      (H.failH ("unexpected changed part: " ++ U.diffPart difference))
  let stagedSlot = B.take firstRecordSlotLength (B.drop slotStart rebuiltPre)
      imageOffset = load
  patches <- either H.failH pure (planPatches plan)
  forM_ [0 .. firstRecordSlotLength - 1] $ \index ->
    when
      ( B.index slot index /= B.index stagedSlot index
          && not (any (\(offset, bytes) -> index >= offset && index < offset + B.length bytes) patches)
      )
      (H.failH ("first-record byte changed outside the declared patches: " ++ show index))
  H.ensure
    (U.mediaWordAt stagedSlot resetVectorWordOffset == armEntryAddress entry)
    "staged reset vector does not name the candidate entry"
  H.ensure
    (B.map complement (B.take (B.length image) (B.drop imageOffset stagedSlot)) == image)
    "staged first-stage bytes differ from the candidate image"
  digestOut <- H.sha256 rebuilt
  B.writeFile (optOutput options) rebuilt
  putStrLn
    ( "staged first stage: "
        ++ show (B.length image)
        ++ " bytes at slot +"
        ++ U.showHexWord (fromIntegral imageOffset)
        ++ ", entry "
        ++ U.showHexWord (armEntryAddress entry)
        ++ ", retained "
        ++ show (map retainedExtentRange retained)
        ++ ", output "
        ++ optOutput options
        ++ ", sha256 "
        ++ digestOut
    )
