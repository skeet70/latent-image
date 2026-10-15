{-# LANGUAGE BangPatterns #-}
module Main (main) where

import Control.Exception (SomeException, displayException, try)
import Control.Monad (forM, forM_)
import Data.Bits (complement)
import qualified Data.ByteString as B
import Data.List (intercalate, isPrefixOf)
import qualified FujiAF.Host as H
import qualified FujiAF.UpdateContainer as U
import Numeric (showHex)
import System.Directory (canonicalizePath, createDirectory)
import System.Environment (getArgs)
import System.Exit (exitFailure)
import System.FilePath ((</>), addTrailingPathSeparator, takeDirectory, takeFileName)
import System.IO (hPutStrLn, stderr)
import RecoveryFirmware

hex :: Integral a => a -> String
hex value = "0x" ++ showHex (toInteger value) ""


printFirmware :: Firmware -> IO ()
printFirmware firmware = do
  let FirmwareDigest digest = firmwareDigest firmware
      IdentityDigest identity = firmwareIdentity firmware
      FirmwareVersion major minor = firmwareVersion firmware
  putStrLn ("File: " ++ firmwarePath firmware)
  putStrLn ("SHA-256: " ++ digest)
  putStrLn ("Version words: " ++ hex major ++ "/" ++ hex minor)
  putStrLn ("Identity-record SHA-256: " ++ identity)
  putStrLn "PASS: main and auxiliary checksums verified; bounded eight-entry segment table"
  putStrLn "Segment | Load word | Size word | BSS word | BSS size word | Logical flash offset | Flash length"
  forM_ (firmwareSegments firmware) $ \(Segment kind entry) ->
    putStrLn (intercalate " | " [show kind, hex (U.segLoad entry), hex (U.segSize entry), hex (U.segBss entry), hex (U.segBssLen entry), hex (U.segFlash entry), hex (U.segFlashLen entry)])
  let RamAddress destination = firmwarePlainDestination firmware
      FileOffset destinationOffset = firmwarePlainDestinationOffset firmware
  putStrLn ("Plain-image destination word at media " ++ hex destinationOffset ++ ": " ++ hex destination)
  putStrLn "The segment-6 load/BSS words are not the plain-image loader destination. All addresses above are file-declared fields, not runtime observations."
  putStrLn "This validates an X-M1-style DAT layout, not model/vendor authenticity, physical NAND mapping, installed-byte identity or a recovery image."

extractFirmware :: Firmware -> FilePath -> IO ()
extractFirmware firmware given = do
  output <- resolve given
  root <- resolve "private-data" >>= canonicalizePath
  parent <- canonicalizePath (takeDirectory output)
  H.ensure (takeFileName output `notElem` ["", ".", ".."] && addTrailingPathSeparator root `isPrefixOf` addTrailingPathSeparator parent) "Output must be a new directory beneath private-data"
  let media = B.map complement (firmwareMain firmware)
      component name offset count = do
        bytes <- slice media (FileOffset offset) (ByteCount count)
        pure (name, bytes)
  fixed <- sequence
    [ component "rom-configuration.bin" 0 1024
    , component "native-boot-records.bin" 0x40000 0x40000
    , component "section-table.bin" 0x140000 0x400
    ]
  segments <- forM (firmwareSegments firmware) $ \(Segment kind entry) ->
    component ("segment-" ++ show (fromEnum kind) ++ ".bin") (fromIntegral (U.segFlash entry)) (fromIntegral (U.segFlashLen entry))
  createDirectory output
  forM_ (fixed ++ segments) $ \(name,bytes) -> B.writeFile (output </> name) bytes
  putStrLn ("Extracted analysis components to " ++ output)
  putStrLn "These are firmware/media components, not an SD BOOT image or a physical NAND backup. Do not write them to a card or camera."

changedBytes :: B.ByteString -> B.ByteString -> Int
changedBytes left right = loop 0 (abs (B.length left-B.length right))
  where
    common = min (B.length left) (B.length right)
    loop !index !count
      | index == common = count
      | otherwise = loop (index+1) (if B.index left index == B.index right index then count else count+1)

compareFirmware :: Firmware -> Firmware -> IO ()
compareFirmware good bad = do
  printFirmware good
  printFirmware bad
  putStrLn "Reference-to-failed-input comparison (files, not a physical camera readback):"
  let left = firmwareMain good
      right = firmwareMain bad
      region offset count bytes = B.take count (B.drop offset bytes)
      same label l r = putStrLn (label ++ ": " ++ if l == r then "equal" else "DIFFERENT")
  same "Identity records" (firmwareIdentity good) (firmwareIdentity bad)
  same "Auxiliary restore list 1" (firmwareAux1 good) (firmwareAux1 bad)
  same "Auxiliary restore list 2" (firmwareAux2 good) (firmwareAux2 bad)
  forM_ [("ROM configuration",0,0x1000), ("Primary boot record",0x40000,0x10000), ("Companion boot record",0x50000,0x10000), ("Primary kernel",0x80000,0x20000), ("Companion kernel",0xa0000,0x20000), ("Adjustment template",0xe0000,0x42000), ("Segment table",0x140000,0x400)] $ \(label,offset,count) -> do
    l <- slice left (FileOffset offset) (ByteCount count)
    r <- slice right (FileOffset offset) (ByteCount count)
    same label l r
  let blockBytes = 0x20000
      blockCount = (max (B.length left) (B.length right)+blockBytes-1) `div` blockBytes
      differences = [(LogicalBlock index, changedBytes (region (index*blockBytes) blockBytes left) (region (index*blockBytes) blockBytes right)) | index <- [0..blockCount-1]]
  putStrLn "Changed logical 128-KiB firmware blocks | Differing bytes (including length differences)"
  forM_ differences $ \(LogicalBlock index,count) ->
    if count == 0 then pure () else putStrLn (show index ++ " | " ++ show count)
  putStrLn "Logical block numbers are not physical NAND targets. The DAT does not contain this body's calibration backup. Do not flash these offsets or substitute them into the v5 writer."

main :: IO ()
main = do
  result <- try $ do
    args <- getArgs
    case args of
      ["inspect", input] -> readFirmware input >>= printFirmware
      ["extract", input, output] -> readFirmware input >>= \firmware -> extractFirmware firmware output
      ["compare", good, bad] -> do
        reference <- readFirmware good
        failed <- readFirmware bad
        compareFirmware reference failed
      ["--help"] -> putStrLn "Usage: bazel run //recovery:inspect-firmware -- inspect DAT | compare KNOWN_GOOD_DAT FAILED_DAT | extract DAT NEW_PRIVATE_DIRECTORY\nHost-only DAT checksums, segment layout, logical-block differences and analysis-component extraction. Supports the observed X-M1-style layout without assuming 1.50 application addresses. Does not generate a flash plan or verify vendor/model authenticity."
      _ -> H.failH "Usage: inspect-firmware inspect DAT | compare KNOWN_GOOD_DAT FAILED_DAT | extract DAT NEW_PRIVATE_DIRECTORY"
  case result :: Either SomeException () of
    Left failure -> hPutStrLn stderr (displayException failure) >> exitFailure
    Right () -> pure ()
