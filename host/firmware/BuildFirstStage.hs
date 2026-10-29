{-# LANGUAGE OverloadedStrings #-}
-- Stage an owned first-record image into a reviewed X-M1 container.
module Main where

import Control.Monad (forM_, unless, when)
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import Data.Bits (complement)
import Data.Char (toLower)
import Data.List (find)
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
  , optPlan :: FirstStagePlan
  , optExpectedSlotSha256 :: String
  }

data Symbol = Symbol
  { symbolAddress :: Word32
  , symbolSize :: Word32
  , symbolName :: String
  }

usage :: String
usage =
  "usage: build-first-stage STOCK_DAT CANDIDATE_BIN CANDIDATE_SYMBOLS OUT_DAT "
    ++ "--load-offset HEX --entry HEX --retained START:END ... --expect-slot-sha256 HEX"

parseNumber :: String -> Maybe Word32
parseNumber text =
  case readHex (dropPrefix text) of
    [(value, "")] -> Just value
    _ -> Nothing
  where
    dropPrefix value
      | take 2 value `elem` ["0x", "0X"] = drop 2 value
      | otherwise = value

parseRange :: String -> Maybe (Int, Int)
parseRange text = do
  let (startText, rest) = break (== ':') text
  case rest of
    ':' : endText -> do
      start <- parseNumber startText
      end <- parseNumber endText
      pure (fromIntegral start, fromIntegral end)
    _ -> Nothing

parseOptions :: [String] -> Either String Options
parseOptions (stock : image : symbols : output : rest) =
  go
    ( Options
        { optStock = stock
        , optImage = image
        , optSymbols = symbols
        , optOutput = output
        , optPlan = FirstStagePlan 0 0 (U.MediaImage B.empty) []
        , optExpectedSlotSha256 = ""
        }
    )
    rest
  where
    go options ("--load-offset" : value : more) =
      case parseNumber value of
        Just offset -> go (options {optPlan = (optPlan options) {planLoadOffset = fromIntegral offset}}) more
        Nothing -> Left ("--load-offset is not a number: " ++ value)
    go options ("--entry" : value : more) =
      case parseNumber value of
        Just entry -> go (options {optPlan = (optPlan options) {planEntry = entry}}) more
        Nothing -> Left ("--entry is not a number: " ++ value)
    go options ("--retained" : value : more) =
      case parseRange value of
        Just extent ->
          let plan = optPlan options
           in go (options {optPlan = plan {planRetainedExtents = planRetainedExtents plan ++ [extent]}}) more
        Nothing -> Left ("--retained is not START:END: " ++ value)
    go options ("--expect-slot-sha256" : value : more) =
      go (options {optExpectedSlotSha256 = map toLower value}) more
    go options [] =
      if length (optExpectedSlotSha256 options) /= 64
        then Left "--expect-slot-sha256 must be 64 hexadecimal characters"
        else
          if planLoadOffset (optPlan options) <= 0
            then Left "--load-offset is required"
            else
              if planEntry (optPlan options) == 0
                then Left "--entry is required"
                else
                  if null (planRetainedExtents (optPlan options))
                    then Left "at least one --retained extent is required"
                    else Right options
    go _ (unexpected : _) = Left ("unexpected argument: " ++ unexpected)
parseOptions _ = Left usage

parseSymbols :: B.ByteString -> Either String [Symbol]
parseSymbols bytes = mapM parseLine (filter (not . C.null) (C.lines bytes))
  where
    parseLine line =
      case C.words line of
        [address, size, _kind, name] ->
          case (parseNumber (C.unpack address), parseNumber (C.unpack size)) of
            (Just addressValue, Just sizeValue) ->
              Right (Symbol addressValue sizeValue (C.unpack name))
            _ -> Left ("symbol row is not numeric: " ++ C.unpack line)
        _ -> Left ("symbol row has no expected shape: " ++ C.unpack line)

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
  stock <- H.readRegular maxContainer (optStock options)
  parsed <- either H.failH pure (U.parseContainer stock)
  verifySource parsed
  let parts = U.parsedParts parsed
  slot <- either H.failH pure (firstRecordSlotBytes parts)
  digest <- H.sha256 slot
  H.ensure
    (map toLower digest == optExpectedSlotSha256 options)
    ("reviewed first-record digest mismatch: source slot is " ++ digest)
  image <- H.readRegular firstRecordSlotLength (optImage options)
  symbols <- H.readRegular 0x10000 (optSymbols options)
  symbolRows <- either H.failH pure (parseSymbols symbols)
  entrySymbol <-
    case find ((== "xm1_first_stage_entry") . symbolName) symbolRows of
      Nothing -> H.failH "candidate symbols lack xm1_first_stage_entry"
      Just symbol -> pure symbol
  H.ensure (symbolAddress entrySymbol == planEntry (optPlan options)) "entry symbol address differs from --entry"
  H.ensure (symbolSize entrySymbol > 0) "entry symbol has zero size"
  let plan = (optPlan options) {planImage = U.MediaImage image}
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
      imageOffset = planLoadOffset plan
  patches <- either H.failH pure (planPatches plan)
  forM_ [0 .. firstRecordSlotLength - 1] $ \index ->
    when
      ( B.index slot index /= B.index stagedSlot index
          && not (any (\(offset, bytes) -> index >= offset && index < offset + B.length bytes) patches)
      )
      (H.failH ("first-record byte changed outside the declared patches: " ++ show index))
  H.ensure
    (U.mediaWordAt stagedSlot resetVectorWordOffset == planEntry plan)
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
        ++ U.showHexWord (planEntry plan)
        ++ ", output "
        ++ optOutput options
        ++ ", sha256 "
        ++ digestOut
    )
