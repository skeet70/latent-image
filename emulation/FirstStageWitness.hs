{-# LANGUAGE OverloadedStrings #-}
-- Natural-ROM first-stage witness on the pinned FujiMod driver.
module Main where

import Control.Monad (forM_)
import Data.Aeson (Value, encode, object, (.=))
import qualified Data.ByteString as B
import Data.Bits (complement)
import qualified Data.ByteString.Lazy as BL
import Data.Word (Word32)
import Emulation.EmulatorSlot (reexecInSlot, slotPolicyFromEnvironment)
import Emulation.FirstStageWitness
import Emulation.ViewerRuntime (DefaultRuntime (..), Firmware (..), Runtime (..), freshRuntime, resolve, validateFirmware, workspace)
import FujiModPaths (fujimodDriver, fujimodRun)
import FujiAF.FirstStage (firstRecordSlotLength, lookupSymbol, parseSymbols, symbolAddress)
import qualified FujiAF.Host as H
import qualified FujiAF.UpdateContainer as U
import System.Directory (createDirectoryIfMissing)
import System.Environment (getArgs)
import System.Exit (ExitCode (..))
import System.FilePath ((</>))
import System.IO (hPutStrLn, stderr)
import System.Process (proc, readCreateProcessWithExitCode)

maxContainer :: Int
maxContainer = 64 * 1024 * 1024

maxImage :: Int
maxImage = 16 * 1024 * 1024

data Options = Options
  { optFirmware :: FilePath
  , optSymbols :: FilePath
  , optCandidateImage :: FilePath
  , optFflzd :: FilePath
  , optOutput :: Maybe FilePath
  }

usage :: String
usage =
  "usage: first-stage-witness --firmware DAT --symbols SYMBOLS --candidate-image BIN --fflzd FFLZD [--output DIR]"

parseOptions :: [String] -> Either String Options
parseOptions = go Nothing Nothing Nothing Nothing Nothing
  where
    go firmware symbols image fflzd output ("--firmware" : value : rest) = go (Just value) symbols image fflzd output rest
    go firmware symbols image fflzd output ("--symbols" : value : rest) = go firmware (Just value) image fflzd output rest
    go firmware symbols image fflzd output ("--candidate-image" : value : rest) = go firmware symbols (Just value) fflzd output rest
    go firmware symbols image fflzd output ("--fflzd" : value : rest) = go firmware symbols image (Just value) output rest
    go firmware symbols image fflzd output ("--output" : value : rest) = go firmware symbols image fflzd (Just value) rest
    go (Just firmware) (Just symbols) (Just image) (Just fflzd) output [] = Right (Options firmware symbols image fflzd output)
    go Nothing _ _ _ _ [] = Left ("--firmware is required\n" ++ usage)
    go _ Nothing _ _ _ [] = Left ("--symbols is required\n" ++ usage)
    go _ _ Nothing _ _ [] = Left ("--candidate-image is required\n" ++ usage)
    go _ _ _ Nothing _ [] = Left ("--fflzd is required\n" ++ usage)
    go _ _ _ _ _ (unexpected : _) = Left ("unexpected argument: " ++ unexpected ++ "\n" ++ usage)

segmentById :: U.Parts -> Word32 -> IO U.SegmentEntry
segmentById parts wanted =
  case [entry | entry <- U.partSegments parts, U.segId entry == wanted] of
    [entry] -> pure entry
    _ -> H.failH ("container segment table lacks a unique id " ++ show wanted)

mediaSlice :: B.ByteString -> Int -> Int -> Int -> IO B.ByteString
mediaSlice dat mainStart offset lengthValue = do
  H.ensure (offset >= 0 && lengthValue >= 0 && mainStart + offset + lengthValue <= B.length dat) "expected media extent is outside the DAT main range"
  pure (B.map complement (B.take lengthValue (B.drop (mainStart + offset) dat)))

expectedImage4 :: FilePath -> U.Parts -> IO B.ByteString
expectedImage4 fflzd parts =
  H.withTemporary "first-stage-fflzd" $ \directory -> do
    let record = U.partRecord4 parts
        outSize = fromIntegral (U.recDecodedSize (U.recHeader record)) :: Int
        recordPath = directory </> "record.bin"
        outputPath = directory </> "image-4.bin"
        mediaRecord = B.map complement (U.serializeRecord record)
    H.ensure (outSize > 0 && outSize <= maxImage) "image-4 decoded size is outside the bounded witness"
    B.writeFile recordPath mediaRecord
    (code, out, err) <-
      readCreateProcessWithExitCode
        (proc fflzd [recordPath, "0", outputPath, show outSize])
        ""
    H.ensure (code == ExitSuccess) ("fflzd failed: " ++ out ++ err)
    H.readRegular maxImage outputPath

contractValue :: Expected -> Value
contractValue expected =
  object
    [ "schema" .= (1 :: Int)
    , "budget" .= (20000000 :: Int)
    , "stops"
        .= [ object ["label" .= stopName label, "address" .= fromIntegral (guestAddress address)]
           | (label, address) <- expectedStops expected
           ]
    , "reads"
        .= [ object ["label" .= readName label, "address" .= fromIntegral (guestAddress address), "length" .= B.length content]
           | ExpectedRead label address content <- expectedReads expected
           ]
    ]

main :: IO ()
main = do
  arguments <- getArgs
  policy <- slotPolicyFromEnvironment
  reexecInSlot policy arguments
  options <- either H.failH pure (parseOptions arguments)
  root <- workspace Nothing
  let dat = resolve root (optFirmware options)
      symbolsPath = resolve root (optSymbols options)
      candidatePath = resolve root (optCandidateImage options)
      fflzd = resolve root (optFflzd options)
      rom = resolve root "private-data/analysis/x-m1-1.50/bootrom/bootrom-0x80000000-4000.bin"
  validateFirmware (Firmware dat)
  datBytes <- H.readRegular maxContainer dat
  parsed <- either H.failH pure (U.parseContainer datBytes)
  forM_ [check | check <- U.checks parsed ++ U.carriedChecks parsed, not (U.checkPassed check)] $ \check ->
    H.failH ("container contract: " ++ U.checkField check)
  let parts = U.parsedParts parsed
      header = U.parsedHeader parsed
      mainStart = U.stagingLength + fromIntegral (U.hMainOffset header)
  entry0 <- segmentById parts 0
  entry1 <- segmentById parts 1
  entry4 <- segmentById parts 4
  loaderA <- mediaSlice datBytes mainStart (fromIntegral (U.segFlash entry0)) (fromIntegral (U.segSize entry0))
  loaderB <- mediaSlice datBytes mainStart (fromIntegral (U.segFlash entry1)) (fromIntegral (U.segSize entry1))
  segmentTable <-
    mediaSlice
      datBytes
      mainStart
      0x140000
      (U.segmentEntryBytes * U.segmentCount + 4)
  image4 <- expectedImage4 fflzd parts
  candidateImage <- H.readRegular firstRecordSlotLength candidatePath
  H.ensure (not (B.null candidateImage)) "candidate image is empty"
  symbolBytes <- H.readRegular 0x10000 symbolsPath
  symbols <- either H.failH pure (parseSymbols symbolBytes)
  entryAddress <- either H.failH pure (symbolAddress <$> lookupSymbol symbols "xm1_first_stage_entry")
  target <- either H.failH pure (leWordAt loaderA 0xc)
  argument <- either H.failH pure (leWordAt image4 0xc)
  H.ensure
    (target >= 0x40000 && target < 0x40000 + fromIntegral (U.segSize entry0))
    "loader A entry word is outside the loader extent"
  let bssLength = fromIntegral (U.segBssLen entry4) :: Int
      image4Address = U.segLoad entry4
      bssAddress = U.segBss entry4
      publicationAddress = GuestAddress 0x61c188
      expectedReads =
        [ ExpectedRead OwnedImage (GuestAddress (fromIntegral entryAddress)) candidateImage
        , ExpectedRead HandoffWords (GuestAddress 0x4300) (encodeLeWord target <> encodeLeWord argument)
        , ExpectedRead SkipList (GuestAddress 0x4310) (B.replicate 0x4f0 0)
        , ExpectedRead LoaderA (GuestAddress (fromIntegral (U.segLoad entry0))) loaderA
        , ExpectedRead LoaderB (GuestAddress (fromIntegral (U.segLoad entry1))) loaderB
        , ExpectedRead SegmentTable (GuestAddress 0x61c000) segmentTable
        , ExpectedRead Image4 (GuestAddress (fromIntegral image4Address)) image4
        , ExpectedRead Image4Bss (GuestAddress (fromIntegral bssAddress)) (B.replicate bssLength 0)
        , ExpectedRead Image4Arg (GuestAddress (fromIntegral (image4Address + 0xc))) (encodeLeWord argument)
        , ExpectedRead Image4Publication publicationAddress (encodeLeWord 0x4310)
        , ExpectedRead PeerCells (GuestAddress 0x40020000) (B.replicate 8 0)
        , ExpectedRead RetainedEventWord (GuestAddress 0x400a00c4) (B.replicate 4 0)
        ]
  expected <-
    either
      H.failH
      pure
      ( do
          storage <- symbolAddress <$> lookupSymbol symbols "xm1_retained_storage_init"
          trailer <- symbolAddress <$> lookupSymbol symbols "xm1_retained_trailer_check"
          stage <- symbolAddress <$> lookupSymbol symbols "xm1_retained_stage_images"
          handoff <- symbolAddress <$> lookupSymbol symbols "xm1_first_stage_handoff_vector"
          pure
            Expected
              { expectedStops =
                  [ (OwnedEntry, GuestAddress (fromIntegral entryAddress))
                  , (StorageInit, GuestAddress (fromIntegral storage))
                  , (TrailerGate, GuestAddress (fromIntegral trailer))
                  , (StageImages, GuestAddress (fromIntegral stage))
                  , (HandoffVector, GuestAddress (fromIntegral handoff))
                  ]
              , expectedReads = expectedReads
              , expectedHandoffTarget = GuestAddress (fromIntegral target)
              , expectedHandoffArgument = GuestAddress (fromIntegral argument)
              }
      )
  runtime <- freshRuntime root (Runtime <$> optOutput options) (UniqueRuntime "first-stage-witness")
  let Runtime directory = runtime
      contractPath = directory </> "first-stage-witness-contract.json"
  createDirectoryIfMissing True directory
  BL.writeFile contractPath (encode (contractValue expected))
  (code, out, err) <-
    readCreateProcessWithExitCode
      (proc fujimodDriver
        [ "--run", fujimodRun
        , "--update", dat
        , "--boot-rom", rom
        , "--output", directory
        , "--witness", contractPath
        ])
      ""
  putStr out
  hPutStrLn stderr err
  receiptBytes <- H.readRegular (2 * 1024 * 1024) (directory </> "first-stage-witness.json")
  receipt <- either H.failH pure (parseReceipt receiptBytes)
  either H.failH pure (validateReceipt expected receipt)
  H.ensure (code == ExitSuccess) "FujiMod witness run exited nonzero"
  putStrLn ("first-stage witness passed: " ++ directory </> "first-stage-witness.json")
