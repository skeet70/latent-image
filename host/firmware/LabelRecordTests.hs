{-# LANGUAGE OverloadedStrings #-}

-- Behavioural tests for the id-0x83d special label record inside the pinned
-- 1.50 image-2 extent: the inline buffer a compiled recipe name takes, the
-- layout halfwords it carries and the load-relative geometry the container
-- supplies. No artefact is read: the media image is built here with the stock
-- record at the offset the stock load word places it. Every assertion reads
-- stored bytes or a refusal.
module Main where

import Control.Exception (SomeException, displayException, try)
import qualified Data.ByteString as B
import Data.IORef (IORef, modifyIORef', newIORef, readIORef)
import Data.List (isInfixOf)
import Data.Word (Word32)
import qualified FujiAF.Host as H
import qualified FujiAF.LabelRecord as L
import FujiAF.UpdateContainer (MediaImage (..), putWord16)
import System.Exit (exitFailure)
import System.IO (hPutStrLn, stderr)

-- The stock geometry: the id-2 image loads at RAM 0x00130000 and its extent is
-- 0x24bac8 bytes, so the id-0x83d record's buffer (RAM 0x0037b7c6, reports/special-label-resolution.md
-- section 3) sits at image offset 0x24b7c6.
stockLoad :: Word32
stockLoad = 0x00130000

stockExtentBytes :: Int
stockExtentBytes = 0x24bac8

stockRecordOffset :: Int
stockRecordOffset = 0x24b7c6

stockBufferBytes :: Int
stockBufferBytes = 56

-- A media-form image of the stock extent: pattern bytes with the stock record
-- laid in, its buffer carrying the '?' and 23 glyphs the stock record holds,
-- a zero terminator and the pinned layout halfwords.
stockImage :: MediaImage
stockImage =
  MediaImage
    ( B.take stockRecordOffset image
        <> B.concat (map putWord16 ([0x003f] ++ replicate 23 0xe150 ++ [0, 0, 0, 0]))
        <> B.concat (map putWord16 L.labelLayoutWords)
        <> B.drop (stockRecordOffset + 0x42) image
    )
  where
    image = B.pack [fromIntegral ((i * 7 + 5) `mod` 251) | i <- [0 .. stockExtentBytes - 1]]

-- The UTF-16LE buffer a name takes: its code units, a zero terminator, zero
-- fill to the buffer's 56 bytes. Built from the literal text, not from the
-- encoder under test.
bufferFor :: String -> B.ByteString
bufferFor name =
  B.concat (map putWord16 (map (fromIntegral . fromEnum) name ++ [0]))
    <> B.replicate (stockBufferBytes - 2 * (length name + 1)) 0

atImage :: Int -> Int -> MediaImage -> B.ByteString
atImage offset length_ media = B.take length_ (B.drop offset (mediaImageBytes media))

expectTrue :: IORef Int -> String -> Bool -> IO ()
expectTrue checked label ok = do
  H.ensure ok label
  modifyIORef' checked (+ 1)

expectEqual :: IORef Int -> String -> B.ByteString -> B.ByteString -> IO ()
expectEqual checked label expected actual = do
  H.ensure (expected == actual) (label ++ ": " ++ show actual ++ " against " ++ show expected)
  modifyIORef' checked (+ 1)

expectWriteRefused :: IORef Int -> String -> MediaImage -> String -> String -> IO ()
expectWriteRefused checked label media name fragment = case L.writeLabelName stockLoad media name of
  Left message -> do
    H.ensure (fragment `isInfixOf` message) (label ++ ": wrong refusal: " ++ message)
    modifyIORef' checked (+ 1)
  Right _ -> H.failH (label ++ ": the write was allowed")

main :: IO ()
main = do
  outcome <- try run
  case outcome :: Either SomeException () of
    Left err -> hPutStrLn stderr (displayException err) >> exitFailure
    Right () -> pure ()

run :: IO ()
run = do
  checked <- newIORef (0 :: Int)
  expectTrue checked "buffer address" (L.labelBufferAddress == 0x0037b7c6)
  expectTrue checked "name limit" (L.labelNameMax == 27)
  expectTrue checked "buffer bytes" (L.labelBufferBytes == 56)
  expectTrue checked "record bytes" (L.labelRecordBytes == 0x42)
  expectTrue checked "layout words" (L.labelLayoutWords == [0x14, 0x14, 0x1e, 0x24, 0x30])

  -- The geometry comes from the container's load word: the stock load places
  -- the record at 0x24b7c6, and a load or extent that cannot hold it refuses.
  expectTrue checked "stock record offset" (L.labelRecordOffset stockLoad stockExtentBytes == Right stockRecordOffset)
  expectTrue
    checked
    "offset below the load"
    ( case L.labelRecordOffset 0x0037c000 stockExtentBytes of
        Left message -> "lies below the image-2 load" `isInfixOf` message
        Right _ -> False
    )
  expectTrue
    checked
    "offset past the extent"
    ( case L.labelRecordOffset 0x0037b000 0x100 of
        Left message -> "does not fit the image-2 extent" `isInfixOf` message
        Right _ -> False
    )
  expectTrue checked "stock layout words at the record" (L.labelLayoutAt stockRecordOffset stockImage == L.labelLayoutWords)

  -- The write changes the 56-byte buffer to UTF-16LE, terminator and zero fill,
  -- and nothing else: the bytes around the buffer and the layout halfwords are
  -- read back from the written image.
  let name = "RECIPE 01"
  written <- case L.writeLabelName stockLoad stockImage name of
    Left message -> H.failH ("write refused: " ++ message)
    Right value -> pure value
  expectEqual checked "stored buffer" (bufferFor name) (atImage stockRecordOffset stockBufferBytes written)
  expectEqual
    checked
    "bytes outside the buffer"
    (B.take stockRecordOffset (mediaImageBytes stockImage) <> B.drop (stockRecordOffset + stockBufferBytes) (mediaImageBytes stockImage))
    (B.take stockRecordOffset (mediaImageBytes written) <> B.drop (stockRecordOffset + stockBufferBytes) (mediaImageBytes written))
  expectTrue checked "layout words carried" (L.labelLayoutAt stockRecordOffset written == L.labelLayoutWords)
  expectTrue checked "extent length carried" (B.length (mediaImageBytes written) == stockExtentBytes)
  expectTrue
    checked
    "read back"
    (L.readLabelName stockLoad written == Right name)

  -- The 27-code-unit boundary fits; a longer name, an empty name, a character
  -- outside printable ASCII and a record without the pinned layout refuse.
  let boundary = replicate L.labelNameMax 'A'
  expectTrue
    checked
    "boundary name fits"
    ( case L.writeLabelName stockLoad stockImage boundary of
        Left _ -> False
        Right full -> L.readLabelName stockLoad full == Right boundary
    )
  expectWriteRefused checked "too long" stockImage (replicate (L.labelNameMax + 1) 'A') "plus a terminator"
  expectWriteRefused checked "empty" stockImage "" "the recipe name is empty"
  expectWriteRefused checked "non-ascii" stockImage "CAF\xc9" "printable ASCII"
  let layoutOffset = stockRecordOffset + stockBufferBytes + 2
      badLayout =
        MediaImage
          ( B.take layoutOffset (mediaImageBytes stockImage)
              <> putWord16 0x15
              <> B.drop (layoutOffset + 2) (mediaImageBytes stockImage)
          )
  expectWriteRefused checked "layout" badLayout "text" "pinned layout halfwords"

  -- A buffer with no terminator reads back as a refusal rather than a name.
  let unterminated =
        MediaImage
          ( B.take stockRecordOffset (mediaImageBytes stockImage)
              <> B.concat (map putWord16 (replicate (stockBufferBytes `div` 2) 0xe150))
              <> B.drop (stockRecordOffset + stockBufferBytes) (mediaImageBytes stockImage)
          )
  expectTrue
    checked
    "no terminator"
    ( case L.readLabelName stockLoad unterminated of
        Left message -> "no zero terminator" `isInfixOf` message
        Right _ -> False
    )

  total <- readIORef checked
  putStrLn (show total ++ " label-record checks passed")
