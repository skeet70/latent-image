{-# LANGUAGE OverloadedStrings #-}
-- Rebuild the pinned X-M1 1.50 FWUP0002.DAT from its own parts: verify the
-- field contract, reproduce the stock container byte for byte, diff a rebuilt
-- container against stock with the restore lists guarded, and replace the
-- image-4 and image-5 records through the gated emitter. The replace-record
-- steps (reports/update-container-rebuild.md) ship nothing unless the strict
-- decoder accepts the record and the stream carried by the rebuilt container
-- decodes to the attached image.
module Main where
import qualified FujiAF.Host as H
import Control.Exception (SomeException, displayException, try)
import Control.Monad (forM_, unless)
import Data.Bits (complement)
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import Data.Word (Word32)
import qualified FujiAF.Lz.Format as F
import qualified FujiAF.LabelRecord as L
import qualified FujiAF.UpdateContainer as U
import System.Directory (removeFile)
import System.Environment (getArgs)
import System.Exit (ExitCode (..), exitFailure)
import System.FilePath ((</>))
import System.IO (hPutStrLn, stderr)

maxContainer :: Int
maxContainer = 64 * 1024 * 1024

readContainer :: FilePath -> IO B.ByteString
readContainer path = do
  bytes <- H.readRegular maxContainer path
  digest <- H.sha256 bytes
  putStrLn (path ++ ": " ++ show (B.length bytes) ++ " bytes, sha256 " ++ digest)
  pure bytes

printCheck :: U.Check -> IO ()
printCheck c =
  putStrLn
    ( (if U.checkPassed c then "pass " else "FAIL ")
        ++ U.checkField c
        ++ ": stored "
        ++ U.checkStored c
        ++ ", expected "
        ++ U.checkExpected c
        ++ " ("
        ++ U.checkSource c
        ++ ")"
    )

runChecks :: String -> [U.Check] -> IO ()
runChecks title results = do
  putStrLn (title ++ ": " ++ show (length (filter U.checkPassed results)) ++ "/" ++ show (length results) ++ " checks pass")
  forM_ results printCheck
  unless (all U.checkPassed results) exitFailure

parseOrFail :: String -> B.ByteString -> IO U.Parsed
parseOrFail label bytes = case U.parseContainer bytes of
  Left message -> hPutStrLn stderr (label ++ ": parse failed: " ++ message) >> exitFailure
  Right parsed -> pure parsed

verify :: Bool -> [FilePath] -> IO ()
verify withStock [input] = do
  bytes <- readContainer input
  parsed <- parseOrFail input bytes
  let results = U.checks parsed ++ U.carriedChecks parsed ++ if withStock then U.stockChecks parsed else []
  runChecks (if withStock then "contract and pinned stock fields" else "contract fields") results
verify _ _ = H.failH "usage: rebuild-update verify|verify-stock INPUT_DAT"

roundTrip :: [FilePath] -> IO ()
roundTrip [input, output] = do
  bytes <- readContainer input
  parsed <- parseOrFail input bytes
  rebuilt <- case U.rebuildContainer parsed (U.parsedParts parsed) of
    Left message -> hPutStrLn stderr ("rebuild refused: " ++ message) >> exitFailure
    Right value -> pure value
  rebuiltDigest <- H.sha256 rebuilt
  putStrLn ("rebuilt from parts: " ++ show (B.length rebuilt) ++ " bytes, sha256 " ++ rebuiltDigest)
  if rebuilt == bytes
    then putStrLn ("byte-for-byte round trip: identical over " ++ show (B.length bytes) ++ " bytes")
    else do
      let (changed, firstOffset) = U.firstDiff bytes rebuilt
      hPutStrLn stderr ("round trip differs: " ++ show changed ++ " bytes, first at " ++ show firstOffset)
      exitFailure
  H.ensure (B.length rebuilt == B.length bytes) "length changed"
  B.writeFile output rebuilt
  putStrLn ("wrote " ++ output)
roundTrip _ = H.failH "usage: rebuild-update round-trip STOCK_DAT OUTPUT_DAT"

diff :: [FilePath] -> IO ()
diff [stockPath, rebuiltPath] = do
  stockBytes <- readContainer stockPath
  rebuiltBytes <- readContainer rebuiltPath
  stock <- parseOrFail stockPath stockBytes
  rebuilt <- parseOrFail rebuiltPath rebuiltBytes
  runChecks "rebuilt contract fields" (U.checks rebuilt ++ U.carriedChecks rebuilt)
  let results = U.diffParts stock rebuilt
  putStrLn "part diff (stock -> rebuilt):"
  forM_ results $ \d ->
    putStrLn
      ( (case U.diffPolicy d of
           U.CarriedMustMatch -> "carried* "
           U.CarriedPolicyChecked -> "carried? "
           U.ImageContent -> "image   ")
          ++ U.diffPart d
          ++ ": "
          ++ show (U.diffChangedBytes d)
          ++ " bytes changed, first at "
          ++ maybe "-" show (U.diffFirstOffset d)
      )
  let unsafe = [d | d <- results, U.diffPolicy d == U.CarriedMustMatch, U.diffChangedBytes d /= 0]
  unless (null unsafe) $ do
    hPutStrLn stderr ("carried parts changed: " ++ unwords [U.diffPart d | d <- unsafe])
    exitFailure
  putStrLn "carried parts (identity, selector, fill, restore lists) unchanged; version words are policy-checked"
diff _ = H.failH "usage: rebuild-update diff STOCK_DAT REBUILT_DAT"

-- The stock decode's first 32 bytes, the body's stage header at RAM
-- 0x0061c400 (byte-read from the pinned 1.50 decode: 0, 0x0061c400,
-- 0x00282828, 0x0069cdf8, 0, 0, 0, 0xaaaaffff). A prefix keeps the original
-- body at its original RAM addresses, so these bytes must sit at the prefix
-- offset in the extended decode; both boot records read the header at
-- 0x0061c40c as a literal address after the image is decompressed.
pinnedStageHeader :: B.ByteString
pinnedStageHeader =
  B.concat (map U.putWord32 [0x00000000, 0x0061c400, 0x00282828, 0x0069cdf8, 0x00000000, 0x00000000, 0x00000000, 0xaaaaffff])

-- The decode-prefix contract (reports/filmsim-patch-01.md section 5): lower
-- the image-4 load by the decode growth and prepend that many bytes, keeping
-- every original byte at its original RAM address. The bss is fixed, so load
-- plus the new decoded size must still reach the stock bss start, and the
-- stock relations then fix the segment size (decoded size + 0x28) and the
-- trailer image words; the emitter's checks enforce the bss end against the
-- id-5 load on the assembled bytes.
applyLoadAddress :: U.Parts -> U.Parts -> B.ByteString -> Maybe Word32 -> IO U.Parts
applyLoadAddress _source parts _ Nothing = pure parts
applyLoadAddress source parts decoded (Just newLoad) = do
  entry4 <- case [e | e <- U.partSegments source, U.segId e == 4] of
    [e] -> pure e
    _ -> H.failH "load address: the source container does not carry exactly one image-4 segment entry"
  let stockLoad = U.segLoad entry4
      stockBss = U.segBss entry4
      stockDecoded = fromIntegral (U.recDecodedSize (U.recHeader (U.partRecord4 source))) :: Int
      decodedSize = B.length decoded
      shift = stockLoad - newLoad
      growth = decodedSize - stockDecoded
      size = fromIntegral decodedSize + 0x28
  H.ensure
    (newLoad < stockLoad)
    ("load address " ++ U.showHexWord newLoad ++ " is not below the stock image-4 load " ++ U.showHexWord stockLoad)
  H.ensure
    (fromIntegral shift == growth)
    ( "load gap does not equal the decode growth: " ++ show shift
        ++ " bytes against "
        ++ show growth
        ++ " bytes of decoded growth"
    )
  H.ensure
    ((fromIntegral newLoad :: Integer) + fromIntegral decodedSize == fromIntegral stockBss)
    ( "load + decoded does not reach the stock bss start: "
        ++ U.showHexWord newLoad
        ++ " + "
        ++ U.showHexWord (fromIntegral decodedSize)
        ++ " against "
        ++ U.showHexWord stockBss
    )
  H.ensure
    (B.take (B.length pinnedStageHeader) (B.drop (fromIntegral shift) decoded) == pinnedStageHeader)
    ("the extended decode does not carry the pinned stage header at decode offset " ++ show shift)
  putStrLn
    ( "image-4 extension: load "
        ++ U.showHexWord stockLoad
        ++ " -> "
        ++ U.showHexWord newLoad
        ++ " ("
        ++ show shift
        ++ "-byte prefix), decoded "
        ++ U.showHexWord (fromIntegral stockDecoded)
        ++ " -> "
        ++ U.showHexWord (fromIntegral decodedSize)
        ++ ", segment size -> "
        ++ U.showHexWord size
        ++ ", bss "
        ++ U.showHexWord stockBss
        ++ "/"
        ++ U.showHexWord (U.segBssLen entry4)
        ++ " unchanged, stage header preserved at decode offset "
        ++ show shift
    )
  pure
    parts
      { U.partSegments =
          [if U.segId e == 4 then e {U.segLoad = newLoad, U.segSize = size} else e | e <- U.partSegments parts]
      , U.partTrailer = (U.partTrailer parts) {U.trImageLoad = newLoad, U.trImageSize = size}
      }

-- The image-replacement step: substitute the image-4 record (the encoder's
-- record file plus the decoded image it must decode to) and emit through the
-- gated emitter. Every gate fails closed and no output is written unless all
-- pass: the record header arithmetic, the shipped-record structural
-- invariants, a strict-decoder round trip of the record, the emission gates
-- (carried set, size relations, decoded-size binding, flash window, checksums)
-- and a strict-decoder round trip of the stream the rebuilt container
-- actually carries. An optional load address extends the decoded image in
-- front of the stock body (applyLoadAddress) and moves the image-4 words with
-- it.
replaceRecord4 :: FilePath -> Maybe Word32 -> [FilePath] -> IO ()
replaceRecord4 decoder newLoad [stockPath, recordPath, decodedPath, outputPath] = do
  stockBytes <- readContainer stockPath
  stock <- parseOrFail stockPath stockBytes
  record <- H.readRegular (8 * 1024 * 1024) recordPath
  decoded <- H.readRegular (16 * 1024 * 1024) decodedPath
  header <- case F.parseHeader record of
    Nothing -> hPutStrLn stderr (recordPath ++ ": unparseable record header") >> exitFailure
    Just value -> pure value
  let streamLength = B.length record - F.headerSize
      stockRecord4 = U.recHeader (U.partRecord4 (U.parsedParts stock))
  H.ensure (fromIntegral (F.headerOutSize header) == B.length decoded) (recordPath ++ ": out_size is not the decoded image length")
  H.ensure (fromIntegral (F.headerTotalLen header) == B.length record) (recordPath ++ ": total_len is not the record length")
  H.ensure (F.headerChunk header == 0x4000) (recordPath ++ ": chunk word is not 0x4000")
  H.ensure (F.headerMode header == 4) (recordPath ++ ": mode word is not 4")
  H.ensure
    (F.headerPages header == (F.headerTotalLen header + 0x3fff) `div` 0x4000)
    (recordPath ++ ": page count is not ceil(total_len/chunk)")
  requireStructure recordPath record
  decodeRecord decoder record decoded "record"
  let newHeader =
        -- Words as the record file carries them (validated above); word 1 is
        -- the record length read from the artifact, never a re-derivation.
        U.RecordHeader
          { U.recDecodedSize = fromIntegral (B.length decoded)
          , U.recTotalLen = F.headerTotalLen header
          , U.recChunkCount = F.headerPages header
          , U.recChunkSize = F.headerChunk header
          , U.recMode = F.headerMode header
          }
      newStream = B.map complement (B.drop F.headerSize record)
      newRecord = U.CompressedRecord newHeader newStream (Just decoded)
      -- Record 5 and everything after it sit at pinned media offsets, so the
      -- record-4 stream grows and shrinks inside its flash window: the window
      -- tail [0x160014, 0x2c0000) keeps the stock bytes at each media offset,
      -- and the stream consumes or releases its slack head.
      stockParts = U.parsedParts stock
      windowBody = U.recStream (U.partRecord4 stockParts) <> U.partGap45 stockParts
  H.ensure
    (B.length newStream <= B.length windowBody)
    (recordPath ++ ": the stream does not fit the image-4 flash window tail")
  let newGap = B.drop (B.length newStream) windowBody
      parts0 = stockParts {U.partRecord4 = newRecord, U.partGap45 = newGap}
  parts <- applyLoadAddress stockParts parts0 decoded newLoad
  rebuilt <- case U.rebuildContainer stock parts of
    Left message -> hPutStrLn stderr ("rebuild refused: " ++ message) >> exitFailure
    Right value -> pure value
  rebuiltParsed <- parseOrFail outputPath rebuilt
  let rebuiltRecord4 = U.partRecord4 (U.parsedParts rebuiltParsed)
      rebuiltHeader = U.recHeader rebuiltRecord4
  H.ensure
    (U.recStream rebuiltRecord4 == newStream)
    (outputPath ++ ": the rebuilt container does not carry the encoded stream")
  H.ensure
    (U.recTotalLen rebuiltHeader == F.headerTotalLen header)
    (outputPath ++ ": the stored record-length word differs from the record file's")
  -- The gate reads the words the container stores, so a wrong stored word
  -- reaches the decoder and fails there instead of being replaced by a
  -- derivation that would agree with itself.
  let plain = B.map complement (U.recStream rebuiltRecord4)
      totalLen = fromIntegral (U.recTotalLen rebuiltHeader) :: Int
      rebuiltPlain =
        F.encodeHeader
          F.Header
            { F.headerOutSize = U.recDecodedSize rebuiltHeader
            , F.headerTotalLen = U.recTotalLen rebuiltHeader
            , F.headerPages = U.recChunkCount rebuiltHeader
            , F.headerChunk = U.recChunkSize rebuiltHeader
            , F.headerMode = U.recMode rebuiltHeader
            }
          <> plain
  requireStructure outputPath rebuiltPlain
  decodeRecord decoder rebuiltPlain decoded "rebuilt stream"
  let stockStream = U.recStream (U.partRecord4 (U.parsedParts stock))
      (streamChanged, streamFirst) = U.firstDiff stockStream newStream
      window = case [U.segFlashLen e | e <- U.partSegments parts, U.segId e == 4] of
        [value] -> value
        _ -> 0
      recordBytes = fromIntegral (U.recordHeaderBytes + streamLength)
      stockChecksum = U.hMainChecksum (U.parsedHeader stock)
      newChecksum = U.hMainChecksum (U.parsedHeader rebuiltParsed)
  putStrLn
    ( "record header word 0 (decoded size): "
        ++ U.showHexWord (U.recDecodedSize stockRecord4)
        ++ " -> "
        ++ U.showHexWord (U.recDecodedSize rebuiltHeader)
    )
  putStrLn
    ( "record header word 1 (record length, total_len): "
        ++ U.showHexWord (U.recTotalLen stockRecord4)
        ++ " -> "
        ++ U.showHexWord (U.recTotalLen rebuiltHeader)
    )
  putStrLn
    ( "stored record-length word against the stream it feeds: "
        ++ U.showHexWord (U.recTotalLen rebuiltHeader)
        ++ " = 0x14 + "
        ++ U.showHexWord (fromIntegral (B.length newStream))
        ++ " stream bytes"
    )
  putStrLn
    ( "record header word 2 (chunk count): "
        ++ U.showHexWord (U.recChunkCount stockRecord4)
        ++ " -> "
        ++ U.showHexWord (U.recChunkCount rebuiltHeader)
    )
  putStrLn
    ( "stream vs stock: "
        ++ show streamChanged
        ++ " bytes differ, first at stream offset "
        ++ maybe "-" show streamFirst
    )
  putStrLn
    ( "image-4 window tail: stock bytes kept at each media offset; slack head "
        ++ show (abs (B.length newStream - B.length stockStream))
        ++ " bytes "
        ++ (if B.length newStream >= B.length stockStream then "consumed by the longer stream" else "released to the slack")
    )
  putStrLn ("main checksum: " ++ U.showHexWord stockChecksum ++ " -> " ++ U.showHexWord newChecksum)
  putStrLn
    ( "flash window: 0x"
        ++ showHexInt recordBytes
        ++ " of "
        ++ U.showHexWord window
        ++ " ("
        ++ show ((recordBytes * 100) `div` fromIntegral window)
        ++ " per cent)"
    )
  putStrLn
    ( "total_len "
        ++ U.showHexWord (fromIntegral totalLen)
        ++ " mod 0x4000 = 0x"
        ++ showHexInt (totalLen `mod` 0x4000)
        ++ (if totalLen `mod` 0x4000 == 0 then " -> FLAG: feed-page multiple, unobserved engine geometry" else "")
    )
  putStrLn
    ( "gates: record header arithmetic, structural invariants, fflzd record round trip, emission gates, "
        ++ "stream-carried identity, structural invariants on the rebuilt stream, fflzd rebuilt-stream round trip: all pass"
    )
  B.writeFile outputPath rebuilt
  putStrLn ("wrote " ++ outputPath ++ ": " ++ show (B.length rebuilt) ++ " bytes")
replaceRecord4 _ _ _ =
  H.failH "usage: rebuild-update replace-record4 --decoder FFLZD [--load-address 0xLOAD] STOCK_DAT RECORD4 DECODED4 OUTPUT_DAT"

-- replace-record4 options: the decoder is required, the load address is the
-- decode-prefix extension (absent keeps the stock load layout).
parseReplaceRecord4 :: [String] -> IO (FilePath, Maybe Word32, [FilePath])
parseReplaceRecord4 arguments = do
  (decoder, afterDecoder) <- case arguments of
    "--decoder" : value : rest -> pure (value, rest)
    _ -> H.failH "usage: rebuild-update replace-record4 --decoder FFLZD [--load-address 0xLOAD] STOCK_DAT RECORD4 DECODED4 OUTPUT_DAT"
  (loadAddress, files) <- case afterDecoder of
    "--load-address" : text : rest -> do
      address <- hexValue "load address" text
      pure (Just address, rest)
    rest -> pure (Nothing, rest)
  pure (decoder, loadAddress, files)

hexValue :: String -> String -> IO Word32
hexValue label text = case text of
  '0' : 'x' : digits@(_ : _) | all (`elem` hexDigits) digits -> do
    let value = foldl (\acc c -> acc * 16 + toInteger (digit c)) 0 digits
    H.ensure (value <= 0xffffffff) (label ++ ": value does not fit 32 bits")
    pure (fromIntegral value)
  _ -> H.failH (label ++ ": expected a 0x-prefixed lowercase hex value")
  where
    digit c
      | '0' <= c && c <= '9' = fromEnum c - fromEnum '0'
      | otherwise = fromEnum c - fromEnum 'a' + 10

hexDigits :: String
hexDigits = "0123456789abcdef"

requireStructure :: String -> B.ByteString -> IO ()
requireStructure label record = case F.verifyRecord record of
  [] -> putStrLn (label ++ ": structural invariants pass (contiguous tokens, exact total_len, feed-boundary rule, stock token domain)")
  violations -> do
    hPutStrLn stderr (label ++ ": structural invariants FAIL")
    forM_ violations (hPutStrLn stderr . ("  " ++))
    exitFailure

decodeRecord :: FilePath -> B.ByteString -> B.ByteString -> String -> IO ()
decodeRecord decoder record expected label = H.withTemporary "fuji-af-replace-record-" $ \root -> do
  let recordPath = root </> "record.bin"
      outPath = root </> "decoded.bin"
  B.writeFile recordPath record
  (stopped, status, _, errors) <-
    H.runBounded 20000000 (1024 * 1024) decoder [recordPath, "0", outPath, show (B.length expected)] B.empty
  H.ensure (not stopped && status == ExitSuccess) (label ++ ": fflzd rejected the record: " ++ C.unpack errors)
  actual <- B.readFile outPath
  H.ensure (actual == expected) (label ++ ": fflzd output does not match the decoded image")
  putStrLn (label ++ ": fflzd round trip matches the decoded image (" ++ show (B.length expected) ++ " bytes)")

-- ---------------------------------------------------------------------------
-- The image-5 record replacement
--
-- Record 5 is compressed like record 4, so a decode change in image 5 needs
-- the same chain: the encoder's record file, the shipped-record invariants, an
-- fflzd round trip of the record, the emission gates, and a read-back of the
-- written file. The record's flash window is [flash, flash + flash_length)
-- (stock [0x2c0000, 0x5a0000), ending where the plain image's flash begins),
-- and the new stream slides inside it: every media offset inside the window
-- beyond the new stream keeps the stock byte. The decoded size is carried.
-- ---------------------------------------------------------------------------

-- The record as the encoder ships it: the five header words and the stream in
-- media form, the complement of the DAT's file bytes. extract-record5 writes
-- this form and replace-record5 reads it, so the pair composes to the identity
-- on the stock container.
recordFile :: U.CompressedRecord -> B.ByteString
recordFile = B.map complement . U.serializeRecord

extractRecord5 :: [FilePath] -> IO ()
extractRecord5 [stockPath, outputPath] = do
  stockBytes <- readContainer stockPath
  stock <- parseOrFail stockPath stockBytes
  let record = U.partRecord5 (U.parsedParts stock)
      (windowStart, windowEnd) = U.record5Window (U.parsedParts stock)
  B.writeFile outputPath (recordFile record)
  putStrLn
    ( "image-5 record: media "
        ++ U.showHexWord (fromIntegral windowStart)
        ++ " to "
        ++ U.showHexWord (fromIntegral windowEnd)
        ++ ", header "
        ++ show (U.recHeader record)
        ++ ", stream "
        ++ show (B.length (U.recStream record))
        ++ " bytes, written in record-file form"
    )
  putStrLn ("wrote " ++ outputPath)
extractRecord5 _ = H.failH "usage: rebuild-update extract-record5 STOCK_DAT OUT_RECORD5"

-- The record-5 replacement: substitute the image-5 record, slide it inside its
-- flash window and emit through the gated emitter. The gates are the record-4
-- chain (header arithmetic, shipped-record invariants, an fflzd record round
-- trip, the emission gates) plus a read-back of the written file: the stored
-- words against the record file's, the stream the file carries, the three
-- range checksums recomputed from the file's own bytes, and every byte outside
-- the rebuilt record and the main checksum word against the source container.
-- A file that fails the read-back is removed before the failure is reported.
replaceRecord5 :: FilePath -> [FilePath] -> IO ()
replaceRecord5 decoder [stockPath, recordPath, decodedPath, outputPath] = do
  stockBytes <- readContainer stockPath
  stock <- parseOrFail stockPath stockBytes
  record <- H.readRegular (16 * 1024 * 1024) recordPath
  decoded <- H.readRegular (16 * 1024 * 1024) decodedPath
  header <- case F.parseHeader record of
    Nothing -> hPutStrLn stderr (recordPath ++ ": unparseable record header") >> exitFailure
    Just value -> pure value
  let stockParts = U.parsedParts stock
      stockHeader = U.recHeader (U.partRecord5 stockParts)
      stockStream = U.recStream (U.partRecord5 stockParts)
      (windowStart, windowEnd) = U.record5Window stockParts
  H.ensure (fromIntegral (F.headerOutSize header) == B.length decoded) (recordPath ++ ": out_size is not the decoded image length")
  H.ensure (fromIntegral (F.headerTotalLen header) == B.length record) (recordPath ++ ": total_len is not the record length")
  H.ensure (F.headerChunk header == 0x4000) (recordPath ++ ": chunk word is not 0x4000")
  H.ensure (F.headerMode header == 4) (recordPath ++ ": mode word is not 4")
  H.ensure
    (F.headerPages header == (F.headerTotalLen header + 0x3fff) `div` 0x4000)
    (recordPath ++ ": page count is not ceil(total_len/chunk)")
  requireStructure recordPath record
  decodeRecord decoder record decoded "record"
  let newHeader =
        -- Words as the record file carries them (validated above); word 1 is
        -- the record length read from the artifact, never a re-derivation.
        U.RecordHeader
          { U.recDecodedSize = fromIntegral (B.length decoded)
          , U.recTotalLen = F.headerTotalLen header
          , U.recChunkCount = F.headerPages header
          , U.recChunkSize = F.headerChunk header
          , U.recMode = F.headerMode header
          }
      newStream = B.map complement (B.drop F.headerSize record)
      newRecord = U.CompressedRecord newHeader newStream (Just decoded)
  parts <- case U.replaceRecord5 stock newRecord of
    Left message -> hPutStrLn stderr ("record-5 replacement refused: " ++ message) >> exitFailure
    Right value -> pure value
  rebuilt <- case U.rebuildContainer stock parts of
    Left message -> hPutStrLn stderr ("rebuild refused: " ++ message) >> exitFailure
    Right value -> pure value
  let (streamChanged, streamFirst) = U.firstDiff stockStream newStream
      recordBytes = fromIntegral (U.recordHeaderBytes + B.length newStream) :: Int
      totalLen = fromIntegral (U.recTotalLen newHeader) :: Int
      windowBytes = windowEnd - windowStart
  putStrLn
    ( "record header word 0 (decoded size): "
        ++ U.showHexWord (U.recDecodedSize stockHeader)
        ++ " -> "
        ++ U.showHexWord (U.recDecodedSize newHeader)
    )
  putStrLn
    ( "record header word 1 (record length, total_len): "
        ++ U.showHexWord (U.recTotalLen stockHeader)
        ++ " -> "
        ++ U.showHexWord (U.recTotalLen newHeader)
    )
  putStrLn
    ( "stored record-length word against the stream it feeds: "
        ++ U.showHexWord (U.recTotalLen newHeader)
        ++ " = 0x14 + "
        ++ U.showHexWord (fromIntegral (B.length newStream))
        ++ " stream bytes"
    )
  putStrLn
    ( "record header word 2 (chunk count): "
        ++ U.showHexWord (U.recChunkCount stockHeader)
        ++ " -> "
        ++ U.showHexWord (U.recChunkCount newHeader)
    )
  putStrLn
    ( "stream vs stock: "
        ++ show streamChanged
        ++ " bytes differ, first at stream offset "
        ++ maybe "-" show streamFirst
    )
  putStrLn
    ( "image-5 window: 0x"
        ++ showHexInt recordBytes
        ++ " of "
        ++ U.showHexWord (fromIntegral windowBytes)
        ++ " ("
        ++ show ((recordBytes * 100) `div` windowBytes)
        ++ " per cent); stock bytes kept at each media offset, slack head "
        ++ show (abs (B.length newStream - B.length stockStream))
        ++ " bytes "
        ++ (if B.length newStream >= B.length stockStream then "consumed by the longer stream" else "released to the slack")
    )
  putStrLn
    ( "total_len "
        ++ U.showHexWord (fromIntegral (U.recTotalLen newHeader))
        ++ " mod 0x4000 = 0x"
        ++ showHexInt (totalLen `mod` 0x4000)
        ++ (if totalLen `mod` 0x4000 == 0 then " -> FLAG: feed-page multiple, unobserved engine geometry" else "")
    )
  B.writeFile outputPath rebuilt
  outcome <- try (verifyRecord5Emitted stockBytes decoder decoded header newStream (windowStart, windowEnd) outputPath rebuilt)
  case outcome of
    Left err -> do
      removeFile outputPath
      hPutStrLn stderr (displayException (err :: SomeException))
      hPutStrLn stderr (outputPath ++ ": removed after the read-back verification failed")
      exitFailure
    Right (written, reparsed) -> do
      putStrLn
        ( "gates: record header arithmetic, structural invariants, fflzd record round trip, emission gates, "
            ++ "written-file read-back (stored words, stream, checksums, carried identity), structural invariants "
            ++ "on the rebuilt stream, fflzd rebuilt-stream round trip: all pass"
        )
      putStrLn
        ( "main checksum word: "
            ++ U.showHexWord (U.hMainChecksum (U.parsedHeader stock))
            ++ " -> "
            ++ U.showHexWord (U.hMainChecksum (U.parsedHeader reparsed))
            ++ " (recomputed over the file's own main range)"
        )
      if U.serializeRecord newRecord == U.serializeRecord (U.partRecord5 stockParts)
        then do
          H.ensure (written == stockBytes) (outputPath ++ ": the stock record did not reproduce the source container byte for byte")
          putStrLn ("stock record: the emitted container is byte-identical to the source over " ++ show (B.length written) ++ " bytes")
        else pure ()
      digest <- H.sha256 written
      putStrLn ("wrote " ++ outputPath ++ ": " ++ show (B.length written) ++ " bytes, sha256 " ++ digest)
replaceRecord5 _ _ = H.failH "usage: rebuild-update replace-record5 --decoder FFLZD STOCK_DAT RECORD5 DECODED5 OUTPUT_DAT"

-- replace-record5 options: the decoder is required; the image-5 window is
-- fixed by the segment table, so there is no load-address form.
parseReplaceRecord5 :: [String] -> IO (FilePath, [FilePath])
parseReplaceRecord5 arguments = case arguments of
  "--decoder" : value : rest -> pure (value, rest)
  _ -> H.failH "usage: rebuild-update replace-record5 --decoder FFLZD STOCK_DAT RECORD5 DECODED5 OUTPUT_DAT"

-- Verification of the written file rather than of the bytes in memory: the
-- contract checks, the three range checksums recomputed from the file's own
-- bytes, the five stored record words against the record file's, the stream
-- the file carries, a strict-decoder round trip of the record it carries, and
-- byte identity with the source outside the rebuilt record and the main
-- checksum word.
verifyRecord5Emitted :: B.ByteString -> FilePath -> B.ByteString -> F.Header -> B.ByteString -> (Int, Int) -> FilePath -> B.ByteString -> IO (B.ByteString, U.Parsed)
verifyRecord5Emitted stockBytes decoder decoded recordHeader newStream (windowStart, windowEnd) outputPath emitted = do
  written <- H.readRegular maxContainer outputPath
  H.ensure (written == emitted) (outputPath ++ ": the written file differs from the emitted bytes")
  reparsed <- case U.parseContainer written of
    Left message -> H.failH (outputPath ++ ": the written container does not parse: " ++ message)
    Right value -> pure value
  runChecksSummary "emitted container contract fields" (U.checks reparsed ++ U.carriedChecks reparsed)
  verifyFileChecksums outputPath written reparsed
  let storedRecord = U.partRecord5 (U.parsedParts reparsed)
      storedHeader = U.recHeader storedRecord
      mainOffset = U.stagingLength + fromIntegral (U.hMainOffset (U.parsedHeader reparsed))
      recordLength = U.recordHeaderBytes + B.length newStream
  H.ensure
    (U.recStream storedRecord == newStream)
    (outputPath ++ ": the written container does not carry the encoded stream")
  H.ensure
    (U.recDecodedSize storedHeader == F.headerOutSize recordHeader)
    (outputPath ++ ": the stored decoded-size word is not the record file's")
  H.ensure
    (U.recTotalLen storedHeader == F.headerTotalLen recordHeader)
    (outputPath ++ ": the stored record-length word is not the record file's")
  H.ensure
    (U.recChunkCount storedHeader == F.headerPages recordHeader)
    (outputPath ++ ": the stored chunk-count word is not the record file's")
  H.ensure
    (U.recChunkSize storedHeader == F.headerChunk recordHeader)
    (outputPath ++ ": the stored chunk word is not the record file's")
  H.ensure
    (U.recMode storedHeader == F.headerMode recordHeader)
    (outputPath ++ ": the stored mode word is not the record file's")
  -- The gate reads the words the container stores, so a wrong stored word
  -- reaches the decoder and fails there instead of being replaced by a
  -- derivation that would agree with itself.
  let rebuiltPlain =
        F.encodeHeader
          F.Header
            { F.headerOutSize = U.recDecodedSize storedHeader
            , F.headerTotalLen = U.recTotalLen storedHeader
            , F.headerPages = U.recChunkCount storedHeader
            , F.headerChunk = U.recChunkSize storedHeader
            , F.headerMode = U.recMode storedHeader
            }
          <> B.map complement (U.recStream storedRecord)
  requireStructure outputPath rebuiltPlain
  decodeRecord decoder rebuiltPlain decoded "rebuilt stream"
  putStrLn
    ( "image-5 flash window: "
        ++ U.showHexWord (fromIntegral windowStart)
        ++ " to "
        ++ U.showHexWord (fromIntegral windowEnd)
    )
  requireCarriedIdentity "image-5 record" stockBytes written (mainOffset + windowStart) recordLength
  pure (written, reparsed)

-- ---------------------------------------------------------------------------
-- Image-2 rebuild and the id-0x83d inline label name
--
-- The loader reads ids 2 and 3 raw from flash to the entry's load address
-- (flashloader command 0x4c02), so the id-2 extent carries no record header:
-- the container part is media [flash, flash + size), stored complemented like
-- every main-range byte (reports/update-container-rebuild.md section 2.2). The
-- label record's buffer, layout halfwords and load-relative geometry are
-- FujiAF.LabelRecord.
-- ---------------------------------------------------------------------------

-- The byte offset of the main range inside a container file.
mainFileOffset :: U.Parsed -> Int
mainFileOffset parsed = U.stagingLength + fromIntegral (U.hMainOffset (U.parsedHeader parsed))

-- The id-2 extent's file offset in the container.
extentFileOffset :: U.Parsed -> Int
extentFileOffset parsed =
  let (mediaOffset, _, _) = U.image2Extent (U.parsedParts parsed)
   in mainFileOffset parsed + mediaOffset

-- The id-2 extent in media form, read from a parsed container: the loader's raw
-- copy, the form replace-image2 reads and extract-image2 writes.
mediaExtent :: U.Parsed -> U.MediaImage
mediaExtent parsed = U.MediaImage (B.map complement (U.partImage2 (U.parsedParts parsed)))

-- A compact result for the commands that emit: the pass count and any failure.
runChecksSummary :: String -> [U.Check] -> IO ()
runChecksSummary title results = do
  let failures = [c | c <- results, not (U.checkPassed c)]
  putStrLn (title ++ ": " ++ show (length results - length failures) ++ "/" ++ show (length results) ++ " checks pass")
  forM_ failures printCheck
  unless (null failures) (H.failH (title ++ ": " ++ show (length failures) ++ " failed"))

-- The three range checksum words recomputed from the file's own bytes over the
-- ranges the file's own header words describe, so the value that was written is
-- never trusted (reports/image2-record-operation.md section 2).
verifyFileChecksums :: FilePath -> B.ByteString -> U.Parsed -> IO ()
verifyFileChecksums label bytes parsed = do
  let header = U.parsedHeader parsed
      range offset lengthWord = B.take (fromIntegral lengthWord) (B.drop (U.stagingLength + fromIntegral offset) bytes)
      mainBody = range (U.hMainOffset header) (U.hMainLength header)
      aux1Body = range (U.hAux1Offset header) (U.hAux1Length header)
      aux2Body = range (U.hAux2Offset header) (U.hAux2Length header)
  H.ensure
    (U.mainChecksum mainBody == U.hMainChecksum header)
    (label ++ ": the stored main checksum word is not the sum over the file's main range")
  H.ensure
    (U.auxChecksum aux1Body == U.hAux1Checksum header)
    (label ++ ": the stored auxiliary 1 checksum word is not the sum over the file's bytes")
  H.ensure
    (U.auxChecksum aux2Body == U.hAux2Checksum header)
    (label ++ ": the stored auxiliary 2 checksum word is not the sum over the file's bytes")
  putStrLn
    ( "read back from "
        ++ label
        ++ ": main checksum "
        ++ U.showHexWord (U.hMainChecksum header)
        ++ ", auxiliary 1 "
        ++ U.showHexWord (U.hAux1Checksum header)
        ++ ", auxiliary 2 "
        ++ U.showHexWord (U.hAux2Checksum header)
        ++ " (recomputed over the file's own ranges)"
    )

-- Everything outside the rebuilt region and the main checksum word must equal
-- the source container byte for byte, so the emitted file proves the carried
-- parts (identity records, selector, range fill, image 3, the plain image, both
-- records, the trailer and both restore lists) identical rather than claiming
-- it.
requireCarriedIdentity :: String -> B.ByteString -> B.ByteString -> Int -> Int -> IO ()
requireCarriedIdentity region source rebuilt extentStart extentLength = do
  H.ensure
    (B.length source == B.length rebuilt)
    ("container length changed: " ++ show (B.length source) ++ " -> " ++ show (B.length rebuilt))
  H.ensure (extentStart >= U.mainChecksumOffset + 4) "the image-2 extent is not past the header"
  let spans =
        [ (0, U.mainChecksumOffset)
        , (U.mainChecksumOffset + 4, extentStart)
        , (extentStart + extentLength, B.length source)
        ]
      results =
        [ (start, U.firstDiff (B.take (end - start) (B.drop start source)) (B.take (end - start) (B.drop start rebuilt)))
        | (start, end) <- spans
        ]
      changed = sum [c | (_, (c, _)) <- results]
      drifted = [start + i | (start, (c, Just i)) <- results, c /= 0]
  putStrLn
    ( "carried parts: "
        ++ show changed
        ++ " bytes differ outside the "
        ++ region
        ++ " and the main checksum word"
    )
  case drifted of
    [] -> pure ()
    (first : _) -> H.failH ("carried data drifted at file offset " ++ show first)

-- The image-2 emission path: gate, emit, write, then verify from the written
-- file. `media` is the replacement extent in media form. A file that fails its
-- read-back verification is removed before the failure is reported.
emitImage2 :: U.Parsed -> B.ByteString -> U.MediaImage -> FilePath -> IO (B.ByteString, U.Parsed)
emitImage2 stock stockBytes media outputPath = do
  parts <- case U.replaceImage2 stock media of
    Left message -> hPutStrLn stderr ("image-2 replacement refused: " ++ message) >> exitFailure
    Right value -> pure value
  emitted <- case U.rebuildContainer stock parts of
    Left message -> hPutStrLn stderr ("rebuild refused: " ++ message) >> exitFailure
    Right value -> pure value
  B.writeFile outputPath emitted
  outcome <- try (verifyEmitted stock stockBytes media outputPath emitted)
  case outcome of
    Left err -> do
      removeFile outputPath
      hPutStrLn stderr (displayException (err :: SomeException))
      hPutStrLn stderr (outputPath ++ ": removed after the read-back verification failed")
      exitFailure
    Right result -> pure result

-- Verification of the written file rather than of the bytes in memory: the
-- three range checksums recomputed from the file, the extent the file carries
-- and the carried identity against the source container.
verifyEmitted :: U.Parsed -> B.ByteString -> U.MediaImage -> FilePath -> B.ByteString -> IO (B.ByteString, U.Parsed)
verifyEmitted stock stockBytes media outputPath emitted = do
  written <- H.readRegular maxContainer outputPath
  H.ensure (written == emitted) (outputPath ++ ": the written file differs from the emitted bytes")
  reparsed <- case U.parseContainer written of
    Left message -> H.failH (outputPath ++ ": the written container does not parse: " ++ message)
    Right value -> pure value
  runChecksSummary "emitted container contract fields" (U.checks reparsed ++ U.carriedChecks reparsed)
  verifyFileChecksums outputPath written reparsed
  H.ensure (mediaExtent reparsed == media) (outputPath ++ ": the emitted extent is not the supplied image")
  requireCarriedIdentity "image-2 extent" stockBytes written (extentFileOffset stock) (B.length (U.mediaImageBytes media))
  pure (written, reparsed)

extractImage2 :: [FilePath] -> IO ()
extractImage2 [stockPath, outputPath] = do
  stockBytes <- readContainer stockPath
  stock <- parseOrFail stockPath stockBytes
  let (mediaOffset, size, load) = U.image2Extent (U.parsedParts stock)
      media = mediaExtent stock
  B.writeFile outputPath (U.mediaImageBytes media)
  putStrLn
    ( "image-2 extent: media offset "
        ++ U.showHexWord (fromIntegral mediaOffset)
        ++ ", "
        ++ show size
        ++ " bytes, RAM load "
        ++ U.showHexWord load
        ++ " (raw flash read, no record header)"
    )
  putStrLn ("wrote " ++ outputPath ++ " (media form, the loader's raw copy)")
extractImage2 _ = H.failH "usage: rebuild-update extract-image2 STOCK_DAT OUT_IMAGE2"

replaceImage2 :: [FilePath] -> IO ()
replaceImage2 [stockPath, image2Path, outputPath] = do
  stockBytes <- readContainer stockPath
  stock <- parseOrFail stockPath stockBytes
  image <- H.readRegular (8 * 1024 * 1024) image2Path
  let media = U.MediaImage image
      stockMedia = mediaExtent stock
      (changed, first) = U.firstDiff (U.mediaImageBytes stockMedia) image
  putStrLn (image2Path ++ ": " ++ show (B.length image) ++ " bytes (media form)")
  got <- H.sha256 image
  putStrLn ("replacement image-2 extent: sha256 " ++ got)
  let (mediaOffset, size, load) = U.image2Extent (U.parsedParts stock)
  (written, reparsed) <- emitImage2 stock stockBytes media outputPath
  putStrLn
    ( "image-2 extent: media "
        ++ U.showHexWord (fromIntegral mediaOffset)
        ++ ", "
        ++ show size
        ++ " bytes, RAM load "
        ++ U.showHexWord load
    )
  putStrLn ("extent vs stock: " ++ show changed ++ " bytes differ, first at image offset " ++ maybe "-" show first)
  putStrLn
    ( "main checksum word: "
        ++ U.showHexWord (U.hMainChecksum (U.parsedHeader stock))
        ++ " -> "
        ++ U.showHexWord (U.hMainChecksum (U.parsedHeader reparsed))
    )
  if media == stockMedia
    then do
      H.ensure (written == stockBytes) (outputPath ++ ": the stock extent did not reproduce the source container byte for byte")
      putStrLn ("stock extent: the emitted container is byte-identical to the source over " ++ show (B.length written) ++ " bytes")
    else pure ()
  digest <- H.sha256 written
  putStrLn ("wrote " ++ outputPath ++ ": " ++ show (B.length written) ++ " bytes, sha256 " ++ digest)
replaceImage2 _ = H.failH "usage: rebuild-update replace-image2 STOCK_DAT IMAGE2_RAW OUTPUT_DAT"

-- ---------------------------------------------------------------------------
-- The plain-extent carrier: the id-6 plain image's loaded window
--
-- The window is entry 6's flash span, carried between the image-5 record and
-- the id-2 extent. extract-plain writes it in media form and replace-plain
-- carries it back whole or as a bounded span at a flash offset; both emit
-- through the gated emitter and read the written file back
-- (reports/update-container-rebuild.md section 8). The carrier moves bytes
-- only: no descriptor, segment or trailer word changes, so the loaded image's
-- size and end geometry are preserved, and no run between the linked extents
-- is claimed free.
-- ---------------------------------------------------------------------------

plainWindowOf :: U.Parts -> IO (Int, Int)
plainWindowOf parts = case U.plainWindow parts of
  Left message -> hPutStrLn stderr ("plain carrier refused: " ++ message) >> exitFailure
  Right value -> pure value

plainImageOf :: U.Parts -> IO U.MediaImage
plainImageOf parts = case U.extractPlain parts of
  Left message -> hPutStrLn stderr ("plain carrier refused: " ++ message) >> exitFailure
  Right value -> pure value

extractPlain :: [FilePath] -> IO ()
extractPlain [stockPath, outputPath] = do
  stockBytes <- readContainer stockPath
  stock <- parseOrFail stockPath stockBytes
  (windowStart, windowEnd) <- plainWindowOf (U.parsedParts stock)
  media <- plainImageOf (U.parsedParts stock)
  B.writeFile outputPath (U.mediaImageBytes media)
  putStrLn
    ( "plain window: flash "
        ++ U.showHexWord (fromIntegral windowStart)
        ++ " to "
        ++ U.showHexWord (fromIntegral windowEnd)
        ++ ", "
        ++ show (windowEnd - windowStart)
        ++ " bytes (the id-6 loader's raw copy)"
    )
  forM_ (U.plainExtents (U.parsedParts stock)) $ \(base, extentLength) ->
    putStrLn ("plain extent " ++ U.showHexWord base ++ ": " ++ show extentLength ++ " bytes")
  putStrLn ("wrote " ++ outputPath ++ " (media form, the loader's raw copy)")
extractPlain _ = H.failH "usage: rebuild-update extract-plain STOCK_DAT OUT_PLAIN"

-- replace-plain options: the flash offset turns the input into a bounded span
-- at that media offset inside the window; absent, the input is the whole
-- window and must be the stored flash length.
parseReplacePlain :: [String] -> IO (Maybe Int, [FilePath])
parseReplacePlain arguments = case arguments of
  "--flash-offset" : text : rest -> do
    offset <- hexValue "flash offset" text
    pure (Just (fromIntegral offset), rest)
  rest -> pure (Nothing, rest)

replacePlain :: Maybe Int -> [FilePath] -> IO ()
replacePlain flashOffset [stockPath, plainPath, outputPath] = do
  stockBytes <- readContainer stockPath
  stock <- parseOrFail stockPath stockBytes
  input <- H.readRegular (8 * 1024 * 1024) plainPath
  let media = U.MediaImage input
      stockParts = U.parsedParts stock
  (windowStart, windowEnd) <- plainWindowOf stockParts
  stockWindow <- plainImageOf stockParts
  let (spanStart, spanLength) = case flashOffset of
        Nothing -> (windowStart, windowEnd - windowStart)
        Just offset -> (offset, B.length input)
  putStrLn (plainPath ++ ": " ++ show (B.length input) ++ " bytes (media form)")
  got <- H.sha256 input
  putStrLn ("replacement plain bytes: sha256 " ++ got)
  case flashOffset of
    Nothing ->
      putStrLn
        ( "plain window: flash "
            ++ U.showHexWord (fromIntegral windowStart)
            ++ " to "
            ++ U.showHexWord (fromIntegral windowEnd)
            ++ ", "
            ++ show (windowEnd - windowStart)
            ++ " bytes"
        )
    Just offset ->
      putStrLn
        ( "plain span: flash "
            ++ U.showHexWord (fromIntegral offset)
            ++ ", "
            ++ show spanLength
            ++ " bytes of the "
            ++ show (windowEnd - windowStart)
            ++ "-byte window"
        )
  let stockSlice = B.take spanLength (B.drop (spanStart - windowStart) (U.mediaImageBytes stockWindow))
      (changed, first) = U.firstDiff stockSlice input
  putStrLn
    ( "plain bytes vs stock: "
        ++ show changed
        ++ " bytes differ, first at window offset "
        ++ maybe "-" (\i -> show (i + spanStart - windowStart)) first
    )
  parts <- case flashOffset of
    Nothing -> case U.replacePlain stock media of
      Left message -> hPutStrLn stderr ("plain replacement refused: " ++ message) >> exitFailure
      Right value -> pure value
    Just offset -> case U.replacePlainAt stock offset media of
      Left message -> hPutStrLn stderr ("plain replacement refused: " ++ message) >> exitFailure
      Right value -> pure value
  emitted <- case U.rebuildContainer stock parts of
    Left message -> hPutStrLn stderr ("rebuild refused: " ++ message) >> exitFailure
    Right value -> pure value
  B.writeFile outputPath emitted
  outcome <- try (verifyPlainEmitted stockBytes flashOffset media (spanStart, spanLength) outputPath emitted)
  case outcome of
    Left err -> do
      removeFile outputPath
      hPutStrLn stderr (displayException (err :: SomeException))
      hPutStrLn stderr (outputPath ++ ": removed after the read-back verification failed")
      exitFailure
    Right (written, reparsed) -> do
      putStrLn
        ( "main checksum word: "
            ++ U.showHexWord (U.hMainChecksum (U.parsedHeader stock))
            ++ " -> "
            ++ U.showHexWord (U.hMainChecksum (U.parsedHeader reparsed))
        )
      case (flashOffset, input == U.mediaImageBytes stockWindow) of
        (Nothing, True) -> do
          H.ensure (written == stockBytes) (outputPath ++ ": the stock plain window did not reproduce the source container byte for byte")
          putStrLn ("stock plain window: the emitted container is byte-identical to the source over " ++ show (B.length written) ++ " bytes")
        _ -> pure ()
      digest <- H.sha256 written
      putStrLn ("wrote " ++ outputPath ++ ": " ++ show (B.length written) ++ " bytes, sha256 " ++ digest)
replacePlain _ _ = H.failH "usage: rebuild-update replace-plain [--flash-offset 0xHEX] STOCK_DAT PLAIN OUTPUT_DAT"

-- Verification of the written file rather than of the bytes in memory: the
-- contract checks, the three range checksums recomputed from the file's own
-- ranges, the supplied bytes at the span they were carried to, and byte
-- identity with the source outside the carried span and the main checksum
-- word (reports/update-container-rebuild.md section 8).
verifyPlainEmitted :: B.ByteString -> Maybe Int -> U.MediaImage -> (Int, Int) -> FilePath -> B.ByteString -> IO (B.ByteString, U.Parsed)
verifyPlainEmitted stockBytes flashOffset media (spanStart, spanLength) outputPath emitted = do
  written <- H.readRegular maxContainer outputPath
  H.ensure (written == emitted) (outputPath ++ ": the written file differs from the emitted bytes")
  reparsed <- case U.parseContainer written of
    Left message -> H.failH (outputPath ++ ": the written container does not parse: " ++ message)
    Right value -> pure value
  runChecksSummary "emitted container contract fields" (U.checks reparsed ++ U.carriedChecks reparsed)
  verifyFileChecksums outputPath written reparsed
  let fileStart = mainFileOffset reparsed + spanStart
  H.ensure
    (B.take spanLength (B.drop fileStart written) == B.map complement (U.mediaImageBytes media))
    (outputPath ++ ": the written container does not carry the supplied plain bytes")
  requireCarriedIdentity
    (case flashOffset of
      Nothing -> "plain window"
      Just _ -> "plain span")
    stockBytes
    written
    fileStart
    spanLength
  pure (written, reparsed)

writeLabel :: [FilePath] -> IO ()
writeLabel [stockPath, text, outputPath] = do
  stockBytes <- readContainer stockPath
  stock <- parseOrFail stockPath stockBytes
  let (_, size, load) = U.image2Extent (U.parsedParts stock)
      stockMedia = mediaExtent stock
  offset <- case L.labelRecordOffset load size of
    Left message -> hPutStrLn stderr ("label write refused: " ++ message) >> exitFailure
    Right value -> pure value
  media <- case L.writeLabelName load stockMedia text of
    Left message -> hPutStrLn stderr ("label write refused: " ++ message) >> exitFailure
    Right value -> pure value
  putStrLn
    ( "id-0x83d record: image offset "
        ++ U.showHexWord (fromIntegral offset)
        ++ " (RAM "
        ++ U.showHexWord L.labelBufferAddress
        ++ ", image load "
        ++ U.showHexWord load
        ++ ")"
    )
  putStrLn
    ( "name: "
        ++ show text
        ++ " ("
        ++ show (length text)
        ++ " UTF-16 code units plus a terminator; the buffer holds "
        ++ show L.labelNameMax
        ++ ")"
    )
  let (changed, first) = U.firstDiff (U.mediaImageBytes stockMedia) (U.mediaImageBytes media)
  putStrLn ("extent vs stock: " ++ show changed ++ " bytes differ, first at image offset " ++ maybe "-" show first)
  (written, reparsed) <- emitImage2 stock stockBytes media outputPath
  let emittedMedia = mediaExtent reparsed
  readBack <- case L.readLabelName load emittedMedia of
    Left message -> hPutStrLn stderr ("the emitted container's id-0x83d buffer did not read back: " ++ message) >> exitFailure
    Right value -> pure value
  H.ensure (readBack == text) ("the emitted container reads back " ++ show readBack ++ " against " ++ show text)
  let layout = L.labelLayoutAt offset emittedMedia
  H.ensure (layout == L.labelLayoutWords) ("the emitted record's layout halfwords are " ++ show layout)
  putStrLn
    ( "read back from the emitted container: "
        ++ show readBack
        ++ " at image offset "
        ++ U.showHexWord (fromIntegral offset)
        ++ ", zero terminator, layout "
        ++ show layout
    )
  putStrLn
    ( "main checksum word: "
        ++ U.showHexWord (U.hMainChecksum (U.parsedHeader stock))
        ++ " -> "
        ++ U.showHexWord (U.hMainChecksum (U.parsedHeader reparsed))
    )
  digest <- H.sha256 written
  putStrLn ("wrote " ++ outputPath ++ ": " ++ show (B.length written) ++ " bytes, sha256 " ++ digest)
writeLabel _ = H.failH "usage: rebuild-update write-label STOCK_DAT TEXT OUTPUT_DAT"

writeRecordUsage :: String
writeRecordUsage = "usage: rebuild-update write-record --id 0xID --layout A,B,C,D,E --current-hex HEX INPUT_DAT TEXT OUTPUT_DAT"

-- write-record options: the special id, the five layout halfwords in decimal
-- and the record's current 66 bytes in media form as 132 lowercase hex digits.
parseWriteRecord :: [String] -> IO (L.SpecialLabelId, L.LabelLayout, L.SpecialRecordBytes, [String])
parseWriteRecord arguments = case arguments of
  "--id" : idText : "--layout" : layoutText : "--current-hex" : currentText : rest -> do
    value <- hexValue "special id" idText
    ident <- either H.failH pure (L.specialLabelId (toInteger value))
    values <- mapM decimal (splitCommas layoutText)
    layout <- either H.failH pure (L.labelLayout values)
    current <- either H.failH pure (L.specialRecordFromHex currentText)
    pure (ident, layout, current, rest)
  _ -> H.failH writeRecordUsage
  where
    decimal text = case reads text of
      [(v, "")] | not (null text), all (`elem` ("0123456789" :: String)) text -> pure v
      _ -> H.failH ("layout: decimal halfword required, got " ++ show text)
    splitCommas text = case break (== ',') text of
      (field, []) -> [field]
      (field, _ : rest) -> field : splitCommas rest

writeRecord :: L.SpecialLabelId -> L.LabelLayout -> L.SpecialRecordBytes -> [String] -> IO ()
writeRecord ident layout current [stockPath, text, outputPath] = do
  name <- either (\message -> H.failH ("record write refused: " ++ message)) pure (L.labelName text)
  stockBytes <- readContainer stockPath
  stock <- parseOrFail stockPath stockBytes
  let (_, size, load) = U.image2Extent (U.parsedParts stock)
      stockMedia = mediaExtent stock
      idName = "id-0x" ++ showHexInt (fromInteger (L.specialLabelIdValue ident))
  offset <- case L.specialRecordOffset ident load size of
    Left message -> hPutStrLn stderr ("record write refused: " ++ message) >> exitFailure
    Right value -> pure value
  media <- case L.writeSpecialRecord load stockMedia ident current name layout of
    Left message -> hPutStrLn stderr ("record write refused: " ++ message) >> exitFailure
    Right value -> pure value
  putStrLn
    ( idName
        ++ " record: image offset "
        ++ U.showHexWord (fromIntegral offset)
        ++ " (RAM "
        ++ U.showHexWord (L.specialRecordAddress ident)
        ++ ", image load "
        ++ U.showHexWord load
        ++ "), current bytes match "
        ++ L.specialRecordHex current
    )
  putStrLn ("name: " ++ show text ++ ", layout " ++ show (L.labelLayoutList layout))
  let (changed, first) = U.firstDiff (U.mediaImageBytes stockMedia) (U.mediaImageBytes media)
  putStrLn ("extent vs input: " ++ show changed ++ " bytes differ, first at image offset " ++ maybe "-" show first)
  (written, reparsed) <- emitImage2 stock stockBytes media outputPath
  (readName, readLayout) <- case L.readSpecialRecord load (mediaExtent reparsed) ident of
    Left message -> hPutStrLn stderr ("the emitted container's " ++ idName ++ " record did not read back: " ++ message) >> exitFailure
    Right value -> pure value
  H.ensure (readName == text) ("the emitted container reads back " ++ show readName ++ " against " ++ show text)
  H.ensure (readLayout == layout) ("the emitted record's layout halfwords are " ++ show (L.labelLayoutList readLayout))
  putStrLn
    ( "read back from the emitted container: "
        ++ show readName
        ++ " at image offset "
        ++ U.showHexWord (fromIntegral offset)
        ++ ", zero terminator, layout "
        ++ show (L.labelLayoutList readLayout)
    )
  putStrLn
    ( "main checksum word: "
        ++ U.showHexWord (U.hMainChecksum (U.parsedHeader stock))
        ++ " -> "
        ++ U.showHexWord (U.hMainChecksum (U.parsedHeader reparsed))
    )
  digest <- H.sha256 written
  putStrLn ("wrote " ++ outputPath ++ ": " ++ show (B.length written) ++ " bytes, sha256 " ++ digest)
writeRecord _ _ _ _ = H.failH writeRecordUsage

showHexInt :: Int -> String
showHexInt n
  | n < 16 = [digit n]
  | otherwise = showHexInt (n `div` 16) ++ [digit (n `mod` 16)]
  where
    digit d = if d < 10 then toEnum (fromEnum '0' + d) else toEnum (fromEnum 'a' + d - 10)

main :: IO ()
main = do
  outcome <- try $ do
    arguments <- getArgs
    case arguments of
      ("verify" : rest) -> verify False rest
      ("verify-stock" : rest) -> verify True rest
      ("round-trip" : rest) -> roundTrip rest
      ("diff" : rest) -> diff rest
      ("replace-record4" : rest) -> do
        (decoder, loadAddress, files) <- parseReplaceRecord4 rest
        replaceRecord4 decoder loadAddress files
      ("extract-record5" : rest) -> extractRecord5 rest
      ("replace-record5" : rest) -> do
        (decoder, files) <- parseReplaceRecord5 rest
        replaceRecord5 decoder files
      ("extract-image2" : rest) -> extractImage2 rest
      ("replace-image2" : rest) -> replaceImage2 rest
      ("extract-plain" : rest) -> extractPlain rest
      ("replace-plain" : rest) -> do
        (flashOffset, files) <- parseReplacePlain rest
        replacePlain flashOffset files
      ("write-label" : rest) -> writeLabel rest
      ("write-record" : rest) -> do
        (ident, layout, current, files) <- parseWriteRecord rest
        writeRecord ident layout current files
      _ ->
        H.failH
          "usage: rebuild-update verify|verify-stock|round-trip|diff|replace-record4|extract-record5|replace-record5|extract-image2|replace-image2|extract-plain|replace-plain|write-label|write-record ..."
  case outcome :: Either SomeException () of
    Left err -> hPutStrLn stderr (displayException err) >> exitFailure
    Right () -> pure ()
