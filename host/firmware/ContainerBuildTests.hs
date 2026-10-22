{-# LANGUAGE OverloadedStrings #-}

-- Behavioural tests for the two CLI paths that built the filmsim prefix
-- container (reports/filmsim-patch-01.md section 5): apply-patch-set's
-- schema-3 decode-prefix extension and rebuild-update replace-record4
-- --load-address. The fixtures are synthetic containers and decode images
-- built the way update-container-tests builds its container, so the target
-- runs in a fresh clone and reads no private-data/ artefact.
--
-- Every assertion reads a written file or the stored words of an emitted
-- container and compares them against a literal or against another input
-- artefact's bytes. No case re-derives a stored value the way the tool under
-- test derives it.
module Main where

import Control.Exception (SomeException, displayException, try)
import Control.Monad (forM_, guard, unless)
import Data.Bits (complement, xor)
import qualified Data.Aeson as A
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import qualified Data.ByteString.Lazy as BL
import Data.IORef (IORef, modifyIORef', newIORef, readIORef)
import Data.List (isInfixOf, sortOn)
import Data.Word (Word16, Word32)
import qualified FujiAF.Host as H
import qualified FujiAF.Lz.Encode as Encode
import qualified FujiAF.UpdateContainer as U
import System.Directory (canonicalizePath, createDirectoryIfMissing, doesFileExist, doesPathExist)
import System.Environment (getArgs)
import System.Exit (ExitCode (..), exitFailure)
import System.FilePath ((</>))
import System.IO (hPutStrLn, stderr)
import System.Process (CreateProcess (..), proc, readCreateProcessWithExitCode)

data ToolRun = ToolRun
  { toolStopped :: Bool
  , toolCode :: ExitCode
  , toolOut :: String
  , toolErr :: String
  }

-- One bounded CLI invocation. Nothing here feeds the tools stdin.
runTool :: FilePath -> [String] -> IO ToolRun
runTool executable arguments = do
  (stopped, code, out, err) <- H.runBounded 60000000 1048576 executable arguments B.empty
  pure (ToolRun stopped code (C.unpack out) (C.unpack err))

expectTrue :: IORef Int -> String -> Bool -> IO ()
expectTrue checked label ok = do
  unless ok (H.failH (label ++ ": failed"))
  modifyIORef' checked (+ 1)

expectEqual :: IORef Int -> String -> B.ByteString -> B.ByteString -> IO ()
expectEqual checked label expected actual = do
  unless (expected == actual) $ do
    let (changed, first) = U.firstDiff expected actual
    H.failH
      ( label
          ++ ": "
          ++ show changed
          ++ " bytes differ of "
          ++ show (B.length expected)
          ++ "/"
          ++ show (B.length actual)
          ++ ", first at "
          ++ maybe "-" show first
      )
  modifyIORef' checked (+ 1)

patchedWord :: Int -> Word32 -> B.ByteString -> B.ByteString
patchedWord offset value bytes = B.take offset bytes <> U.putWord32 value <> B.drop (offset + 4) bytes

-- ---------------------------------------------------------------------------
-- apply-patch-set: schema-3 decode-prefix extension and schema-1 identity
--
-- decode_base 0x1000, output_base 0x0f40, so the extension is 0xc0 bytes: a
-- write at file offset 0x10 sits in the template prefix and one at 0xc0 is the
-- decode's first word of the constructed output.
-- ---------------------------------------------------------------------------

applierDecode :: B.ByteString
applierDecode = B.pack [fromIntegral (0x20 + i) | i <- [0 .. 63 :: Int]]

applierTemplate :: B.ByteString
applierTemplate = B.replicate 0xc0 0x11

applierOutputBase :: Word32
applierOutputBase = 0x0f40

syntheticManifest :: Integer -> Maybe Word32 -> Maybe A.Value -> [A.Value] -> String -> A.Value
syntheticManifest version outputBase extensionBlock wordList digest =
  H.obj
    ( [ ("schema_version", H.n version)
      , ("id", H.s "PIN-APPLIER-01")
      , ("tier", H.s "synthetic")
      , ("recorded_at", H.s "2026-10-09")
      , ("target", H.s "synthetic decode image")
      , ("image", H.obj (imageFields ++ [("output_base", H.s (U.showHexWord base)) | Just base <- [outputBase]]))
      , ("free_space", H.s "synthetic")
      , ("array_source", H.s "synthetic")
      , ("order_rule", H.s "ascending order")
      , ("blobs", H.arr [])
      , ("words", H.arr wordList)
      , ("totals", H.s "synthetic")
      , ("unchanged_surfaces", H.s "synthetic")
      , ("container_effect", H.s "synthetic")
      , ("emulation_case_parameters", H.s "synthetic")
      ]
        ++ [("extension", block) | Just block <- [extensionBlock]]
    )
  where
    imageFields =
      [ ("segment_id", H.n 4)
      , ("decode_base", H.s (U.showHexWord 0x00001000))
      , ("decode_file", H.s "synthetic-decode.bin")
      , ("decode_sha256", H.s digest)
      , ("file_offset_rule", H.s "file_offset = address - output_base")
      , ("dump_correspondence", H.s "synthetic")
      ]

prefixExtension :: Integer -> String -> A.Value
prefixExtension size currentHex =
  H.obj [("kind", H.s "prefix"), ("length", H.n size), ("current_hex", H.s currentHex)]

wordEntry :: Integer -> Word32 -> Int -> Word32 -> Word32 -> A.Value
wordEntry order address offset current fresh =
  H.obj
    [ ("order", H.n order)
    , ("address", H.s (U.showHexWord address))
    , ("file_offset", H.s (U.showHexWord (fromIntegral offset)))
    , ("current", H.s (U.showHexWord current))
    , ("new", H.s (U.showHexWord fresh))
    , ("role", H.s "synthetic word")
    ]

applierCases :: IORef Int -> FilePath -> IO ()
applierCases checked applier = H.withTemporary "fuji-af-applier-" $ \root -> do
  let inputPath = root </> "decode.bin"
      manifestPath name = root </> (name ++ "-patch-set.json")
      outputPath name = root </> (name ++ "-output.bin")
  B.writeFile inputPath applierDecode
  digest <- H.sha256 applierDecode
  let expectAccepted name manifest expected = do
        B.writeFile (manifestPath name) (BL.toStrict (A.encode manifest))
        result <- runTool applier [manifestPath name, inputPath, outputPath name]
        unless (not (toolStopped result) && toolCode result == ExitSuccess) $
          H.failH (name ++ ": apply-patch-set refused: " ++ toolErr result)
        actual <- B.readFile (outputPath name)
        expectEqual checked (name ++ " output bytes") expected actual
        putStrLn ("accept " ++ name ++ ": " ++ show (B.length actual) ++ " bytes")
        pure result
      expectRefused name manifest fragment = do
        B.writeFile (manifestPath name) (BL.toStrict (A.encode manifest))
        result <- runTool applier [manifestPath name, inputPath, outputPath name]
        unless (not (toolStopped result) && toolCode result /= ExitSuccess) $
          H.failH (name ++ ": apply-patch-set accepted it")
        unless (fragment `isInfixOf` toolErr result) $
          H.failH (name ++ ": wrong refusal: " ++ toolErr result)
        leftover <- doesFileExist (outputPath name)
        unless (not leftover) $ H.failH (name ++ ": refused run wrote an output file")
        modifyIORef' checked (+ 1)
        putStrLn ("refuse " ++ name)
  let prefixWrite = wordEntry 1 0x0f50 0x10 0x11111111 0xdeadbeef
      bodyWrite = wordEntry 2 0x1000 0xc0 0x23222120 0xcafebabe
      templateHex = concat (replicate 192 "11")
      expectedExtension =
        patchedWord 0x10 0xdeadbeef applierTemplate <> patchedWord 0x0 0xcafebabe applierDecode
      expectedSchema1Write = patchedWord 0x0 0x01020304 applierDecode
  -- The constructed output is the template prefix plus the input decode, and
  -- each write's current bytes are checked against that constructed output.
  prefixRun <-
    expectAccepted
      "schema3-prefix"
      (syntheticManifest 3 (Just applierOutputBase) (Just (prefixExtension 0xc0 templateHex)) [prefixWrite, bodyWrite] digest)
      expectedExtension
  expectTrue checked "schema3-prefix reports the extension" ("extension: prefix 192 bytes" `isInfixOf` toolOut prefixRun)
  -- Extension length disagreeing with the decode_base - output_base shift.
  expectRefused
    "schema3-length-vs-shift"
    (syntheticManifest 3 (Just applierOutputBase) (Just (prefixExtension 0x40 (concat (replicate 64 "11")))) [] digest)
    "patch set extension length must equal decode_base - output_base"
  -- current_hex not the declared extension length.
  expectRefused
    "schema3-hex-length"
    (syntheticManifest 3 (Just applierOutputBase) (Just (prefixExtension 0xc0 (concat (replicate 128 "11")))) [] digest)
    "patch set extension length must match current_hex"
  -- Prefix-offset current bytes taken from the decode instead of the template.
  expectRefused
    "schema3-prefix-current"
    (syntheticManifest 3 (Just applierOutputBase) (Just (prefixExtension 0xc0 templateHex)) [wordEntry 1 0x0f50 0x10 0x33323130 0xdeadbeef] digest)
    "write order 1 at file offset 0x10: current bytes disagree"
  -- Body-offset current bytes taken from the template instead of the decode.
  expectRefused
    "schema3-body-current"
    (syntheticManifest 3 (Just applierOutputBase) (Just (prefixExtension 0xc0 templateHex)) [wordEntry 1 0x1000 0xc0 0x11111111 0xdeadbeef] digest)
    "write order 1 at file offset 0xc0: current bytes disagree"
  -- An extension block is schema-3 only.
  expectRefused
    "schema3-version"
    (syntheticManifest 2 (Just applierOutputBase) (Just (prefixExtension 0xc0 templateHex)) [] digest)
    "patch set extension requires schema_version 3"
  -- The no-extension path reproduces its input byte for byte.
  identityRun <- expectAccepted "schema1-identity" (syntheticManifest 1 Nothing Nothing [] digest) applierDecode
  expectTrue checked "schema1-identity builds no extension" (not ("extension" `isInfixOf` toolOut identityRun))
  -- A no-extension write lands at the decode's own offsets, with no prefix.
  _ <-
    expectAccepted
      "schema1-write"
      (syntheticManifest 1 Nothing Nothing [wordEntry 1 0x1000 0x0 0x23222120 0x01020304] digest)
      expectedSchema1Write
  -- output_base without an extension block has no meaning.
  expectRefused
    "schema1-output-base"
    (syntheticManifest 1 (Just applierOutputBase) Nothing [] digest)
    "patch set output_base requires an extension block"
  -- A later write over one blob word applies on top of the blob's new bytes.
  let blob =
        H.obj
          [ ("id", H.s "synthetic-blob")
          , ("role", H.s "synthetic blob")
          , ("address", H.s (U.showHexWord 0x1000))
          , ("file_offset", H.s (U.showHexWord 0))
          , ("length", H.n 8)
          , ("order_range", H.arr [H.n 1, H.n 2])
          , ("current_hex", H.s "2021222324252627")
          , ("new_hex", H.s "a0a1a2a3a4a5a6a7")
          ]
      withBlob manifest = H.set "blobs" (H.arr [blob]) manifest
  _ <-
    expectAccepted
      "overlay-contained"
      (withBlob (syntheticManifest 1 Nothing Nothing [wordEntry 3 0x1004 0x4 0xa7a6a5a4 0x0badf00d] digest))
      (B.pack [0xa0, 0xa1, 0xa2, 0xa3, 0x0d, 0xf0, 0xad, 0x0b] <> B.drop 8 applierDecode)
  -- The overlay's current bytes are the blob's new bytes, not the decode's.
  expectRefused
    "overlay-current"
    (withBlob (syntheticManifest 1 Nothing Nothing [wordEntry 3 0x1004 0x4 0x27262524 0x0badf00d] digest))
    "write order 3 at file offset 0x4: current bytes disagree"
  -- A write straddling two blob words is refused.
  expectRefused
    "overlay-partial"
    (withBlob (syntheticManifest 1 Nothing Nothing [wordEntry 3 0x1002 0x2 0xa5a4a3a2 0x0badf00d] digest))
    "patch set write words overlap"

-- ---------------------------------------------------------------------------
-- rebuild-update replace-record4 --load-address
--
-- The fixture carries the pinned image-4 geometry (load 0x0061c400, decoded
-- size 0x282800, size 0x282828, bss 0x0089ec00/0x00352e00, image-5 load
-- 0x00bf1a00) with a synthetic record-4 stream, and a flash window big enough
-- for the record this target encodes. The decoded image is the pinned
-- geometry extended by the 0xc0-byte prefix with the pinned stage header at
-- the new decode offset.
-- ---------------------------------------------------------------------------

patternBytes :: Int -> B.ByteString
patternBytes n = B.pack [fromIntegral ((i * 7 + 3) `mod` 251) | i <- [0 .. n - 1]]

patchMediaWord :: Int -> Word32 -> B.ByteString -> B.ByteString
patchMediaWord offset value bytes = B.take offset bytes <> U.putMediaWord value <> B.drop (offset + 4) bytes

tableEnd :: Int
tableEnd = U.tableEndOffset U.Stock

record4Media :: Int
record4Media = 0x140200

mainSum :: B.ByteString -> Word32
mainSum body = complement (B.foldl' (\acc b -> acc + fromIntegral (complement b)) 0 body)

auxSum :: B.ByteString -> Word32
auxSum bytes = complement (B.foldl' (\acc b -> acc + fromIntegral b) 0 bytes)

fixtureIdentity :: B.ByteString
fixtureIdentity = "IDENTITYRECORD01" <> patternBytes (U.identityBytes - 16)

-- The loader destination word and the stock plain-extent pins sit in the
-- table tail at their own offsets, as in the stock container.
fixtureTail :: B.ByteString
fixtureTail =
  foldl
    (\accumulated (offset, value) -> patchMediaWord (offset - tableEnd) value accumulated)
    (patternBytes (record4Media - tableEnd))
    ( [(0x140184,0x1234),(0x1401a4,0x4061c1b0), U.stockPlainLoad, (0x1401ac,0x360000)]
        ++ zip [0x1401b0,0x1401b4 .. 0x1401f8]
          [0,0x01100000,0xac5a0,0,0,0x44,2,0x011c0000,0x1cca0,0,0,0x46,3,0x011e0000,0x26a5e0,0,0,0x47,0xffffffff]
    )

fixtureAux1Bytes :: B.ByteString
fixtureAux1Bytes = B.concat [U.putWord16 (U.pair16Offset p) <> U.putWord16 (U.pair16Length p) | p <- U.stockAux1]

fixtureAux2Bytes :: B.ByteString
fixtureAux2Bytes = B.concat [U.putWord32 (U.pair32Offset p) <> U.putWord32 (U.pair32Length p) | p <- U.stockAux2]

fixtureRecord4 :: U.CompressedRecord
fixtureRecord4 = U.CompressedRecord (U.RecordHeader 0x282800 0x34 1 0x4000 4) (patternBytes 0x20) Nothing

fixtureRecord5 :: U.CompressedRecord
fixtureRecord5 = U.CompressedRecord (U.RecordHeader 0x4f1e60 0x4015 2 0x4000 4) (patternBytes 0x4001) Nothing

serializeEntry :: U.SegmentEntry -> B.ByteString
serializeEntry e =
  B.concat
    [ U.putMediaWord (U.segId e)
    , U.putMediaWord (U.segLoad e)
    , U.putMediaWord (U.segSize e)
    , U.putMediaWord (U.segBss e)
    , U.putMediaWord (U.segBssLen e)
    , U.putMediaWord (U.segFlash e)
    , U.putMediaWord (U.segFlashLen e)
    , U.putMediaWord (U.segWord7 e)
    , U.putMediaWord (U.segFlag e)
    ]

serializeRecordBytes :: U.CompressedRecord -> B.ByteString
serializeRecordBytes r =
  B.concat
    [ U.putMediaWord (U.recDecodedSize (U.recHeader r))
    , U.putMediaWord (U.recTotalLen (U.recHeader r))
    , U.putMediaWord (U.recChunkCount (U.recHeader r))
    , U.putMediaWord (U.recChunkSize (U.recHeader r))
    , U.putMediaWord (U.recMode (U.recHeader r))
    , U.recStream r
    ]

-- The fixture variants differ only in the values the load-address case must
-- discriminate (the bss base/length pair and the image-4 flash window), in
-- the id-2 extent's load and size, which the image-2 cases vary, in the
-- id-6 plain window and the post-head gap that carries it, and in the id-3
-- font window and the post-tail span that carries it.
data Fixture = Fixture
  { fxBss :: Word32
  , fxBssLen :: Word32
  , fxFlashLen4 :: Word32
  , fxRecord5Media :: Int
  , fxImage2Load :: Word32
  , fxImage2Size :: Int
  , fxPlainFlash :: Word32
  , fxPlainLen :: Word32
  , fxImage2Gap :: Int
  , fxFontFlash :: Word32
  , fxPostTail :: Int
  , fxReservedFlash :: Word32
  }

baseFixture :: Fixture
baseFixture =
  Fixture
    { fxBss = 0x0089ec00
    , fxBssLen = 0x00352e00
    , fxFlashLen4 = 0x00100000
    , fxRecord5Media = 0x240200
    , fxImage2Load = 0x3000
    , fxImage2Size = 0x200
    , fxPlainFlash = 0x00360000
    , fxPlainLen = 0x00360000
    , fxImage2Gap = 0x6c0000 - (0x240200 + U.recordHeaderBytes + 0x4001) + 11
    , fxFontFlash = 0x40000
    , fxPostTail = 5
    , fxReservedFlash = 0xb40000
    }

-- The id-0x83d special label record's buffer address (reports/special-label-resolution.md
-- section 3): 0x0037b784 plus one 0x42-byte record. The label fixture loads
-- the id-2 image at 0x0037b000, so the record sits at image offset 0x7c6.
labelBufferAddress :: Word32
labelBufferAddress = 0x0037b7c6

labelRecordLength :: Int
labelRecordLength = 0x42

labelLayoutWords :: [Word16]
labelLayoutWords = [0x14, 0x14, 0x1e, 0x24, 0x30]

labelFixture :: Fixture
labelFixture = baseFixture {fxImage2Load = 0x0037b000, fxImage2Size = 0x900}

-- The extraction-only fixture places image 2 eleven bytes after record 5.
extractionRefusalFixture :: Fixture
extractionRefusalFixture = baseFixture {fxImage2Gap = 11}

-- The font fixture places the id-3 window at media 0x6e0000, 0x20000 bytes,
-- inside the post-tail span after the id-2 extent (media 0x6c000b + 0x200),
-- and the id-7 window after it, so every segment window lies inside the main
-- range as build-extension's firmware reader requires.
fontFixture :: Fixture
fontFixture = baseFixture {fxFontFlash = 0x6e0000, fxReservedFlash = 0x700000, fxPostTail = 0x720000 - (0x6c000b + 0x200) + 5}

fontWindowLength :: Int
fontWindowLength = 0x20000

-- The font window's media bytes: the complement of the fill run the body
-- carries at the window's offset in the post-tail span.
fixtureFontMedia :: B.ByteString
fixtureFontMedia =
  B.map
    complement
    (B.take fontWindowLength (B.drop (fromIntegral (fxFontFlash fontFixture) - (image2Media fontFixture + fxImage2Size fontFixture)) (patternBytes (fxPostTail fontFixture))))

-- The plain window's media bytes (the loader's raw copy): the complement of
-- the fill run the body carries at the window's offset in the post-head gap.
fixturePlainMedia :: B.ByteString
fixturePlainMedia =
  B.map
    complement
    ( B.take (fromIntegral (fxPlainLen baseFixture))
        (B.drop (fromIntegral (fxPlainFlash baseFixture) - record5EndMedia baseFixture) (patternBytes (fxImage2Gap baseFixture)))
    )

-- The id-2 extent's media offset: fxImage2Gap bytes past the image-5 record,
-- so the variants that move record 5 or carry a plain window move the extent
-- with it.
image2Media :: Fixture -> Int
image2Media fixture = record5EndMedia fixture + fxImage2Gap fixture

record5EndMedia :: Fixture -> Int
record5EndMedia fixture = fxRecord5Media fixture + B.length (serializeRecordBytes fixtureRecord5)

-- The fixture's id-2 extent in media form (the loader's raw copy), with the
-- stock id-0x83d record laid in at the offset the fixture's load word places
-- it; the body carries the complement of these bytes.
fixtureImage2 :: Fixture -> B.ByteString
fixtureImage2 fixture =
  case recordOffset of
    Nothing -> base
    Just offset -> B.take offset base <> record <> B.drop (offset + labelRecordLength) base
  where
    base = patternBytes (fxImage2Size fixture)
    recordOffset = do
      guard (labelBufferAddress >= fxImage2Load fixture)
      let offset = fromIntegral (labelBufferAddress - fxImage2Load fixture)
      guard (offset + labelRecordLength <= fxImage2Size fixture)
      pure offset
    record =
      B.concat (map U.putWord16 ([0x003f] ++ replicate 23 0xe150 ++ [0, 0, 0, 0]))
        <> B.concat (map U.putWord16 labelLayoutWords)

fixtureSegments :: Fixture -> [U.SegmentEntry]
fixtureSegments fixture =
  [ U.SegmentEntry 0 0x1000 0x200 0 0 0x30000 0x10000 0 0
  , U.SegmentEntry 1 0x2000 0x200 0 0 0x31000 0x10000 0 0
  , U.SegmentEntry 2 (fxImage2Load fixture) (fromIntegral (fxImage2Size fixture)) 0 0 (fromIntegral (image2Media fixture)) 0x10000 0 0
  , U.SegmentEntry 3 0x4000 0x200 0 0 (fxFontFlash fixture) (fromIntegral fontWindowLength) 0 0
  , U.SegmentEntry 4 0x0061c400 0x00282828 (fxBss fixture) (fxBssLen fixture) (fromIntegral record4Media) (fxFlashLen4 fixture) 0x20 1
  , U.SegmentEntry 5 0x00bf1a00 0x004f1e60 0x01460000 0x002bbe40 (fromIntegral (fxRecord5Media fixture)) 0x00100000 0x20 1
  , U.SegmentEntry 6 0x00bf1a00 0x004f1e60 0x01460000 0x002bbe40 (fxPlainFlash fixture) (fxPlainLen fixture) 0x20 0
  , U.SegmentEntry 7 0 0 0 0 (fxReservedFlash fixture) 0x20000 0 0
  ]

fixtureBodyPrefix :: Fixture -> B.ByteString
fixtureBodyPrefix fixture =
  B.concat
    [ foldl (\bytes (off, value) -> patchMediaWord off value bytes)
        (patternBytes (U.segmentTableOffset U.Stock))
        [(0x140000,0x4061c040),(0x140004,0x4061c1a4),(0x140008,0),(0x14000c,0x4061c184)]
    , B.concat [serializeEntry e | e <- fixtureSegments fixture]
    , U.putMediaWord 0xffffffff
    , fixtureTail
    , serializeRecordBytes fixtureRecord4
    , patternBytes (fxRecord5Media fixture - record4Media - B.length (serializeRecordBytes fixtureRecord4))
    , serializeRecordBytes fixtureRecord5
    , patternBytes (image2Media fixture - fxRecord5Media fixture - B.length (serializeRecordBytes fixtureRecord5))
    , B.map complement (fixtureImage2 fixture)
    , patternBytes (fxPostTail fixture)
    ]

fixtureBody :: Fixture -> B.ByteString
fixtureBody fixture =
  let prefix = fixtureBodyPrefix fixture
      entry4 = case [e | e <- fixtureSegments fixture, U.segId e == 4] of
        (entry : _) -> entry
        [] -> error "fixture segment 4 missing"
   in prefix
        <> B.concat
          [ U.putMediaWord (fromIntegral (B.length prefix))
          , U.putMediaWord (fromIntegral (B.length prefix))
          , U.putMediaWord 0x20
          , U.putMediaWord 0x00000000
          , U.putMediaWord 0x00000000
          , U.putMediaWord (U.segLoad entry4)
          , U.putMediaWord (U.segSize entry4)
          , U.putMediaHalf 0xffff
          , U.putMediaHalf 0xaaaa
          ]

fixtureBytes :: Fixture -> B.ByteString
fixtureBytes fixture =
  let body = fixtureBody fixture
      aux1 = fixtureAux1Bytes
      aux2 = fixtureAux2Bytes
      mainLength = fromIntegral (B.length body)
      aux1Length = fromIntegral (B.length aux1)
   in B.concat
        [ U.putWord32 4
        , fixtureIdentity
        , U.putWord32 1
        , U.putWord32 0x50
        , U.putWord32 (mainSum body)
        , U.putWord32 1
        , B.replicate 0x28 0xff
        , U.putWord32 0
        , U.putWord32 mainLength
        , U.putWord32 mainLength
        , U.putWord32 aux1Length
        , U.putWord32 (auxSum aux1)
        , U.putWord32 (mainLength + aux1Length)
        , U.putWord32 (fromIntegral (B.length aux2))
        , U.putWord32 (auxSum aux2)
        , body
        , aux1
        , aux2
        ]

-- The pinned 32-byte stage header the loader reads at 0x0061c40c
-- (reports/filmsim-patch-01.md section 5.1); the load-address step must keep it
-- at the decode offset the prefix shift creates.
pinnedStageHeader :: B.ByteString
pinnedStageHeader =
  B.concat (map U.putWord32 [0x00000000, 0x0061c400, 0x00282828, 0x0069cdf8, 0x00000000, 0x00000000, 0x00000000, 0xaaaaffff])

-- A decoded image of the pinned geometry: 0x2828c0 bytes, the pinned stage
-- header at decode offset 0xc0, and a repeating body so the record stays far
-- inside the flash window.
decodedImage :: B.ByteString
decodedImage = B.take 0xc0 raw <> pinnedStageHeader <> B.drop 0xe0 raw
  where
    raw = patternBytes 0x2828c0

corruptStageHeader :: B.ByteString -> B.ByteString
corruptStageHeader bytes = B.take 0xc0 bytes <> B.singleton 0x5a <> B.drop 0xc1 bytes

-- Encode the image through the record encoder. The encoder refuses a total_len
-- on a feed-page multiple, so a tail byte bounds the retry; the returned image
-- is the one actually encoded.
encodeFixture :: B.ByteString -> IO (B.ByteString, B.ByteString)
encodeFixture = go (0 :: Int)
  where
    go attempts candidate
      | attempts > 64 = H.failH "synthetic decoded image cannot be encoded outside the feed-aligned total_len class"
      | otherwise = case Encode.encodeRecord 0x400000 candidate of
          Right record -> pure (record, candidate)
          Left Encode.FeedAlignedTotalLen -> go (attempts + 1) (B.take (B.length candidate - 1) candidate <> B.singleton (B.last candidate `xor` 0xff))
          Left err -> H.failH ("synthetic decoded image refused by the encoder: " ++ show err)

decodeRecordBytes :: FilePath -> FilePath -> String -> B.ByteString -> Int -> IO B.ByteString
decodeRecordBytes decoder root name record expectedLength = do
  let payloadPath = root </> (name ++ "-payload.bin")
      outputPath = root </> (name ++ "-decoded.bin")
  B.writeFile payloadPath record
  result <- runTool decoder [payloadPath, "0", outputPath, show expectedLength]
  unless (not (toolStopped result) && toolCode result == ExitSuccess) $
    H.failH (name ++ ": fflzd refused the emitted record: " ++ toolErr result)
  B.readFile outputPath

loadAddressCases :: IORef Int -> FilePath -> FilePath -> IO ()
loadAddressCases checked rebuilder decoder = H.withTemporary "fuji-af-load-" $ \root -> do
  (recordBytes, image) <- encodeFixture decodedImage
  (badRecordBytes, badImage) <- encodeFixture (corruptStageHeader decodedImage)
  let baseRecordPath = root </> "record4.bin"
      baseDecodedPath = root </> "decoded4.bin"
      badRecordPath = root </> "record4-bad-header.bin"
      badDecodedPath = root </> "decoded4-bad-header.bin"
  B.writeFile baseRecordPath recordBytes
  B.writeFile baseDecodedPath image
  B.writeFile badRecordPath badRecordBytes
  B.writeFile badDecodedPath badImage
  putStrLn
    ( "synthetic record 4: "
        ++ show (B.length recordBytes)
        ++ " bytes carrying "
        ++ show (B.length image)
        ++ " decoded bytes"
    )
  let go name fixture recordPath decodedPath loadHex = do
        let stockPath = root </> (name ++ "-stock.dat")
            outputPath = root </> (name ++ "-output.dat")
        B.writeFile stockPath (fixtureBytes fixture)
        result <-
          runTool
            rebuilder
            ["replace-record4", "--decoder", decoder, "--load-address", loadHex, stockPath, recordPath, decodedPath, outputPath]
        pure (result, outputPath)
      expectRefused name fixture recordPath decodedPath loadHex fragment = do
        (result, outputPath) <- go name fixture recordPath decodedPath loadHex
        unless (not (toolStopped result) && toolCode result /= ExitSuccess) $
          H.failH (name ++ ": accepted a wrong load-address input")
        unless (fragment `isInfixOf` toolErr result) $
          H.failH (name ++ ": wrong refusal: " ++ toolErr result)
        leftover <- doesFileExist outputPath
        unless (not leftover) $ H.failH (name ++ ": refused run wrote an output file")
        modifyIORef' checked (+ 1)
        putStrLn ("refuse " ++ name)
  (accepted, acceptedPath) <- go "load-address" baseFixture baseRecordPath baseDecodedPath "0x0061c340"
  unless (not (toolStopped accepted) && toolCode accepted == ExitSuccess) $
    H.failH ("load-address: refused: " ++ toolErr accepted)
  emitted <- B.readFile acceptedPath
  parsed <- case U.parseContainer emitted of
    Left message -> H.failH ("load-address: emitted container does not parse: " ++ message)
    Right value -> pure value
  let parts = U.parsedParts parsed
      storedHeader = U.parsedHeader parsed
      entry wanted = case [e | e <- U.partSegments parts, U.segId e == wanted] of
        [one] -> one
        _ -> error ("emitted container does not carry exactly one image-" ++ show wanted ++ " entry")
      entry4 = entry 4
      entry5 = entry 5
      record4 = U.partRecord4 parts
      trailer = U.partTrailer parts
      mainStart = U.stagingLength + fromIntegral (U.hMainOffset storedHeader)
      carriedRecord = B.take (B.length recordBytes) (B.drop (mainStart + record4Media) emitted)
  expectTrue checked "load-address image 4 load" (U.segLoad entry4 == 0x0061c340)
  expectTrue checked "load-address image 4 size" (U.segSize entry4 == 0x002828e8)
  expectTrue checked "load-address image 4 bss" (U.segBss entry4 == 0x0089ec00 && U.segBssLen entry4 == 0x00352e00)
  expectTrue checked "load-address bss end meets image 5 load" (U.segBss entry4 + U.segBssLen entry4 == U.segLoad entry5 && U.segLoad entry5 == 0x00bf1a00)
  expectTrue checked "load-address record decoded size word" (U.recDecodedSize (U.recHeader record4) == 0x002828c0)
  expectTrue checked "load-address trailer image words" (U.trImageLoad trailer == 0x0061c340 && U.trImageSize trailer == 0x002828e8)
  expectTrue checked "load-address trailer span" (U.trSpan trailer == U.hMainLength storedHeader - fromIntegral U.trailerBytes && U.trSpan2 trailer == U.trSpan trailer)
  expectTrue checked "load-address container extent" (B.length emitted == B.length (fixtureBytes baseFixture))
  expectEqual checked "load-address carried record bytes" (B.map complement recordBytes) carriedRecord
  decodedOutput <- decodeRecordBytes decoder root "emitted-record" (B.map complement carriedRecord) (B.length image)
  expectEqual checked "load-address decoded payload" image decodedOutput
  expectTrue
    checked
    "load-address stage header at decode offset"
    (B.take (B.length pinnedStageHeader) (B.drop 0xc0 decodedOutput) == pinnedStageHeader)
  putStrLn ("accept load-address: emitted " ++ show (B.length emitted) ++ " bytes")
  -- A load whose gap is not the decode growth.
  expectRefused "gap-mismatch" baseFixture baseRecordPath baseDecodedPath "0x0061c2c0" "load gap does not equal the decode growth"
  -- A load that is not below the stock image-4 load.
  expectRefused "load-not-below-stock" baseFixture baseRecordPath baseDecodedPath "0x0061c400" "is not below the stock image-4 load"
  -- The stock bss base the load and the decoded size must land on.
  expectRefused
    "bss-base"
    (baseFixture {fxBss = 0x0089ec10, fxBssLen = 0x00352df0})
    baseRecordPath
    baseDecodedPath
    "0x0061c340"
    "load + decoded does not reach the stock bss start"
  -- The emitter's own gate on the bss end meeting the next image's load.
  expectRefused
    "bss-end"
    (baseFixture {fxBssLen = 0x00352e04})
    baseRecordPath
    baseDecodedPath
    "0x0061c340"
    "image 4 bss relation"
  -- The stream must fit the image-4 flash window tail.
  expectRefused
    "flash-window-tail"
    (baseFixture {fxRecord5Media = 0x141234})
    baseRecordPath
    baseDecodedPath
    "0x0061c340"
    "the stream does not fit the image-4 flash window tail"
  -- The emitter's own gate on the declared image-4 flash window.
  expectRefused
    "flash-window-declared"
    (baseFixture {fxFlashLen4 = 0x10000})
    baseRecordPath
    baseDecodedPath
    "0x0061c340"
    "image 4 record flash window"
  -- The stage header must sit at the decode offset the shift creates.
  expectRefused
    "stage-header"
    baseFixture
    badRecordPath
    badDecodedPath
    "0x0061c340"
    "pinned stage header at decode offset 192"

-- ---------------------------------------------------------------------------
-- rebuild-update extract-image2 / replace-image2 / write-label
--
-- The fixture's segment words describe the id-2 extent the body carries, and
-- the label fixture carries the stock id-0x83d record inside it. Assertions
-- read the written files: the extent in media form, the bytes carried outside
-- the extent and the checksum word, and the id-0x83d buffer the file stores.
-- ---------------------------------------------------------------------------

-- Byte identity of `rebuilt` against `source` outside the given half-open
-- regions, so a case states exactly which regions may differ.
expectCarriedIdentity :: IORef Int -> String -> B.ByteString -> B.ByteString -> [(Int, Int)] -> IO ()
expectCarriedIdentity checked label source rebuilt allowed = do
  expectTrue checked (label ++ " length unchanged") (B.length source == B.length rebuilt)
  let spans = complementSpans (B.length source) (sortOn fst allowed)
  expectTrue
    checked
    (label ++ " carried bytes identical outside " ++ show allowed)
    (all (\(start, end) -> B.take (end - start) (B.drop start source) == B.take (end - start) (B.drop start rebuilt)) spans)
  where
    complementSpans total regions = go 0 regions
      where
        go cursor [] = [(cursor, total) | cursor < total]
        go cursor ((start, len) : rest) = [(cursor, start) | cursor < start] ++ go (start + len) rest

image2Cases :: IORef Int -> FilePath -> IO ()
image2Cases checked rebuilder = H.withTemporary "fuji-af-image2-" $ \root -> do
  let stockPath = root </> "image2-stock.dat"
      labelStockPath = root </> "image2-label-stock.dat"
      extentPath = root </> "image2.bin"
      reproPath = root </> "image2-repro.dat"
      changedOutPath = root </> "image2-changed-out.dat"
      labelOutPath = root </> "image2-label-out.dat"
      stockBytes = fixtureBytes baseFixture
      labelBytes = fixtureBytes labelFixture
      recordOffset = fromIntegral (labelBufferAddress - fxImage2Load labelFixture)
      labelExtentStart = U.stagingLength + image2Media labelFixture
  B.writeFile stockPath stockBytes
  B.writeFile labelStockPath labelBytes
  putStrLn ("image-2 fixture: extent media " ++ U.showHexWord (fromIntegral (image2Media baseFixture)) ++ ", " ++ show (fxImage2Size baseFixture) ++ " bytes; label fixture record at image offset " ++ U.showHexWord (fromIntegral recordOffset))

  -- extract-image2 writes the extent in media form.
  extracted <- runTool rebuilder ["extract-image2", stockPath, extentPath]
  unless (not (toolStopped extracted) && toolCode extracted == ExitSuccess) $
    H.failH ("extract-image2 refused: " ++ toolErr extracted)
  extent <- B.readFile extentPath
  expectEqual checked "extract-image2 extent" (fixtureImage2 baseFixture) extent

  -- The stock extent reproduces the source container byte for byte.
  reproduced <- runTool rebuilder ["replace-image2", stockPath, extentPath, reproPath]
  unless (not (toolStopped reproduced) && toolCode reproduced == ExitSuccess) $
    H.failH ("replace-image2 refused the stock extent: " ++ toolErr reproduced)
  unless ("byte-identical" `isInfixOf` toolOut reproduced) $
    H.failH ("replace-image2 did not report the stock reproduction: " ++ toolOut reproduced)
  expectEqual checked "replace-image2 stock bytes" stockBytes =<< B.readFile reproPath

  -- A changed extent changes the extent and the main checksum word only.
  let changedExtent = B.take 0x100 extent <> B.singleton (B.index extent 0x100 `xor` 0xff) <> B.drop 0x101 extent
      changedPath = root </> "image2-changed.bin"
  B.writeFile changedPath changedExtent
  changedRun <- runTool rebuilder ["replace-image2", stockPath, changedPath, changedOutPath]
  unless (not (toolStopped changedRun) && toolCode changedRun == ExitSuccess) $
    H.failH ("replace-image2 refused the changed extent: " ++ toolErr changedRun)
  changed <- B.readFile changedOutPath
  parsedChanged <- case U.parseContainer changed of
    Left message -> H.failH ("replace-image2 emitted an unparseable container: " ++ message)
    Right value -> pure value
  let changedHeader = U.parsedHeader parsedChanged
      (changedOffset, changedSize, _) = U.image2Extent (U.parsedParts parsedChanged)
      changedExtentStart = U.stagingLength + fromIntegral (U.hMainOffset changedHeader) + changedOffset
      mainStart = U.stagingLength + fromIntegral (U.hMainOffset changedHeader)
      mainRange = B.take (fromIntegral (U.hMainLength changedHeader)) (B.drop mainStart changed)
  expectEqual checked "replace-image2 stored extent" (B.map complement changedExtent) (B.take changedSize (B.drop changedExtentStart changed))
  expectTrue checked "replace-image2 stored main checksum" (U.hMainChecksum changedHeader == mainSum mainRange)
  expectTrue checked "replace-image2 checksum word moved" (U.word32At changed U.mainChecksumOffset /= U.word32At stockBytes U.mainChecksumOffset)
  expectCarriedIdentity
    checked
    "replace-image2"
    stockBytes
    changed
    [(U.mainChecksumOffset, 4), (changedExtentStart, changedSize)]
  putStrLn "accept replace-image2: the store extent and the main checksum word moved, nothing else"

  -- write-label writes the name into the id-0x83d inline buffer.
  let name = "RECIPE 01"
      expectedBuffer =
        B.concat (map U.putWord16 (map (fromIntegral . fromEnum) name ++ [0]))
          <> B.replicate (56 - 2 * (length name + 1)) 0
      stockLabelMedia = fixtureImage2 labelFixture
  labelled <- runTool rebuilder ["write-label", labelStockPath, name, labelOutPath]
  unless (not (toolStopped labelled) && toolCode labelled == ExitSuccess) $
    H.failH ("write-label refused: " ++ toolErr labelled)
  unless (("read back from the emitted container: " ++ show name) `isInfixOf` toolOut labelled) $
    H.failH ("write-label did not read the name back: " ++ toolOut labelled)
  emittedLabel <- B.readFile labelOutPath
  let labelMedia = B.map complement (B.take (fxImage2Size labelFixture) (B.drop labelExtentStart emittedLabel))
  expectEqual checked "write-label buffer" expectedBuffer (B.take 56 (B.drop recordOffset labelMedia))
  expectEqual
    checked
    "write-label carried extent"
    (B.take recordOffset stockLabelMedia <> B.drop (recordOffset + 56) stockLabelMedia)
    (B.take recordOffset labelMedia <> B.drop (recordOffset + 56) labelMedia)
  expectEqual checked "write-label layout words" (B.concat (map U.putWord16 labelLayoutWords)) (B.take 10 (B.drop (recordOffset + 56) labelMedia))
  expectCarriedIdentity
    checked
    "write-label"
    labelBytes
    emittedLabel
    [(U.mainChecksumOffset, 4), (labelExtentStart, fxImage2Size labelFixture)]
  putStrLn ("accept write-label: " ++ show name ++ " in the id-0x83d buffer at image offset " ++ U.showHexWord (fromIntegral recordOffset))

  -- Refusals: every one fails closed and writes no output file.
  let refusal label output arguments fragment = do
        result <- runTool rebuilder (arguments ++ [output])
        unless (not (toolStopped result) && toolCode result /= ExitSuccess) $
          H.failH (label ++ ": accepted")
        unless (fragment `isInfixOf` toolErr result) $
          H.failH (label ++ ": wrong refusal: " ++ toolErr result)
        leftover <- doesFileExist output
        unless (not leftover) $ H.failH (label ++ ": refused run wrote an output file")
        modifyIORef' checked (+ 1)
        putStrLn ("refuse " ++ label)
      shortPath = root </> "image2-short.bin"
  B.writeFile shortPath (B.take (B.length extent - 1) extent)
  refusal "replace-image2 short extent" (root </> "image2-short-out.dat") ["replace-image2", stockPath, shortPath] "against the stored size word"
  refusal "write-label too long" (root </> "label-too-long.dat") ["write-label", labelStockPath, replicate 28 'A'] "the 56-byte buffer holds 27 plus a terminator"
  refusal "write-label empty" (root </> "label-empty.dat") ["write-label", labelStockPath, ""] "empty"
  refusal "write-label record outside" (root </> "label-outside.dat") ["write-label", stockPath, name] "does not fit the image-2 extent"
  let layoutFileOffset = labelExtentStart + recordOffset + 56 + 2
      badLayoutPath = root </> "image2-label-bad.dat"
  B.writeFile badLayoutPath (B.take layoutFileOffset labelBytes <> U.putMediaHalf 0x15 <> B.drop (layoutFileOffset + 2) labelBytes)
  refusal "write-label layout" (root </> "label-layout.dat") ["write-label", badLayoutPath, name] "pinned layout halfwords"

-- ---------------------------------------------------------------------------
-- rebuild-update extract-plain / replace-plain (the plain-extent carrier)
--
-- The plain fixture carries the id-6 window in the post-head gap past the
-- image-5 flash window, so the window the segment words name is the fill run
-- the body carries. Assertions read the written files: the window in media
-- form, the span bytes at their declared flash offset, and the bytes carried
-- outside the span and the main checksum word.
-- ---------------------------------------------------------------------------

plainCases :: IORef Int -> FilePath -> IO ()
plainCases checked rebuilder = H.withTemporary "fuji-af-plain-" $ \root -> do
  let stockPath = root </> "plain-stock.dat"
      baseStockPath = root </> "plain-base-stock.dat"
      windowPath = root </> "plain-window.bin"
      reproPath = root </> "plain-repro.dat"
      changedPath = root </> "plain-changed.bin"
      changedOutPath = root </> "plain-changed-out.dat"
      stockBytes = fixtureBytes baseFixture
      windowStart = fromIntegral (fxPlainFlash baseFixture)
      windowLength = fromIntegral (fxPlainLen baseFixture)
      spanFileStart = U.stagingLength + windowStart + 0x1000
  B.writeFile stockPath stockBytes
  B.writeFile baseStockPath (fixtureBytes extractionRefusalFixture)
  putStrLn
    ( "plain fixture: window media "
        ++ U.showHexWord (fromIntegral windowStart)
        ++ ", "
        ++ show windowLength
        ++ " bytes"
    )

  -- extract-plain writes the window in media form.
  extracted <- runTool rebuilder ["extract-plain", stockPath, windowPath]
  unless (not (toolStopped extracted) && toolCode extracted == ExitSuccess) $
    H.failH ("extract-plain refused: " ++ toolErr extracted)
  window <- B.readFile windowPath
  expectEqual checked "extract-plain window" fixturePlainMedia window

  -- The stock window reproduces the source container byte for byte.
  reproduced <- runTool rebuilder ["replace-plain", stockPath, windowPath, reproPath]
  unless (not (toolStopped reproduced) && toolCode reproduced == ExitSuccess) $
    H.failH ("replace-plain refused the stock window: " ++ toolErr reproduced)
  unless ("byte-identical" `isInfixOf` toolOut reproduced) $
    H.failH ("replace-plain did not report the stock reproduction: " ++ toolOut reproduced)
  expectEqual checked "replace-plain stock bytes" stockBytes =<< B.readFile reproPath

  -- A one-byte span at a flash offset changes that byte and the main checksum
  -- word only, and the emitted container keeps every contract field.
  let spanByte = B.singleton (B.index fixturePlainMedia 0x1000 `xor` 0xff)
  B.writeFile changedPath spanByte
  changedRun <-
    runTool
      rebuilder
      ["replace-plain", "--flash-offset", U.showHexWord (fromIntegral (windowStart + 0x1000)), stockPath, changedPath, changedOutPath]
  unless (not (toolStopped changedRun) && toolCode changedRun == ExitSuccess) $
    H.failH ("replace-plain refused the changed span: " ++ toolErr changedRun)
  changed <- B.readFile changedOutPath
  expectEqual checked "replace-plain span byte" (B.map complement spanByte) (B.take 1 (B.drop spanFileStart changed))
  expectTrue checked "replace-plain checksum word moved" (U.word32At changed U.mainChecksumOffset /= U.word32At stockBytes U.mainChecksumOffset)
  expectCarriedIdentity
    checked
    "replace-plain span"
    stockBytes
    changed
    [(U.mainChecksumOffset, 4), (spanFileStart, 1)]
  verified <- runTool rebuilder ["verify", changedOutPath]
  unless (not (toolStopped verified) && toolCode verified == ExitSuccess) $
    H.failH ("verify refused the changed container: " ++ toolErr verified)
  putStrLn "accept replace-plain: one changed window byte and the main checksum word moved, nothing else"

  -- Refusals: every one fails closed and writes no output file.
  let refusal label output arguments fragment = do
        result <- runTool rebuilder (arguments ++ [output])
        unless (not (toolStopped result) && toolCode result /= ExitSuccess) $
          H.failH (label ++ ": accepted")
        unless (fragment `isInfixOf` toolErr result) $
          H.failH (label ++ ": wrong refusal: " ++ toolErr result)
        leftover <- doesFileExist output
        unless (not leftover) $ H.failH (label ++ ": refused run wrote an output file")
        modifyIORef' checked (+ 1)
        putStrLn ("refuse " ++ label)
      longPath = root </> "plain-long.bin"
      emptyPath = root </> "plain-empty.bin"
  B.writeFile longPath (window <> B.singleton 0x5a)
  B.writeFile emptyPath B.empty
  refusal "replace-plain long window" (root </> "plain-long-out.dat") ["replace-plain", stockPath, longPath] "against the stored flash length"
  refusal
    "replace-plain span past window"
    (root </> "plain-past-out.dat")
    ["replace-plain", "--flash-offset", U.showHexWord (fromIntegral (windowStart + windowLength)), stockPath, changedPath]
    "runs past the plain window"
  refusal
    "replace-plain span before window"
    (root </> "plain-before-out.dat")
    ["replace-plain", "--flash-offset", U.showHexWord (fromIntegral (windowStart - 1)), stockPath, changedPath]
    "starts before the plain window"
  refusal
    "replace-plain empty span"
    (root </> "plain-empty-out.dat")
    ["replace-plain", "--flash-offset", U.showHexWord (fromIntegral (windowStart + 0x1000)), stockPath, emptyPath]
    "the plain span is empty"
  refusal
    "extract-plain window past extent"
    (root </> "plain-past-extent.bin")
    ["extract-plain", baseStockPath]
    "runs past the image-2 extent"

-- ---------------------------------------------------------------------------
-- rebuild-update extract-font / replace-font (the font-window carrier) and
-- build-extension's font-patch-set step
--
-- The font fixture carries the id-3 window in the post-tail span after the
-- id-2 extent. Assertions read the written files: the window in media form,
-- the window bytes carried back, and the bytes outside the window and the
-- main checksum word.
-- ---------------------------------------------------------------------------

fontCases :: IORef Int -> FilePath -> FilePath -> FilePath -> FilePath -> FilePath -> IO ()
fontCases checked rebuilder applier decoder encoder builder = H.withTemporary "fuji-af-font-" $ \root -> do
  let stockPath = root </> "font-stock.dat"
      windowPath = root </> "font-window.bin"
      reproPath = root </> "font-repro.dat"
      changedPath = root </> "font-changed.bin"
      changedOutPath = root </> "font-changed-out.dat"
      stockBytes = fixtureBytes fontFixture
      windowFileStart = U.stagingLength + fromIntegral (fxFontFlash fontFixture)
  B.writeFile stockPath stockBytes

  -- extract-font writes the window in media form.
  extracted <- runTool rebuilder ["extract-font", stockPath, windowPath]
  unless (not (toolStopped extracted) && toolCode extracted == ExitSuccess) $
    H.failH ("extract-font refused: " ++ toolErr extracted)
  window <- B.readFile windowPath
  expectEqual checked "extract-font window" fixtureFontMedia window

  -- The stock window reproduces the source container byte for byte.
  reproduced <- runTool rebuilder ["replace-font", stockPath, windowPath, reproPath]
  unless (not (toolStopped reproduced) && toolCode reproduced == ExitSuccess) $
    H.failH ("replace-font refused the stock window: " ++ toolErr reproduced)
  expectEqual checked "replace-font stock bytes" stockBytes =<< B.readFile reproPath

  -- A changed window byte changes that byte and the main checksum word only.
  let changedWindow = B.take 0x1000 window <> B.singleton (B.index window 0x1000 `xor` 0xff) <> B.drop 0x1001 window
  B.writeFile changedPath changedWindow
  changedRun <- runTool rebuilder ["replace-font", stockPath, changedPath, changedOutPath]
  unless (not (toolStopped changedRun) && toolCode changedRun == ExitSuccess) $
    H.failH ("replace-font refused the changed window: " ++ toolErr changedRun)
  changed <- B.readFile changedOutPath
  expectEqual checked "replace-font window byte" (B.singleton (complement (B.index changedWindow 0x1000))) (B.take 1 (B.drop (windowFileStart + 0x1000) changed))
  expectCarriedIdentity checked "replace-font" stockBytes changed [(U.mainChecksumOffset, 4), (windowFileStart + 0x1000, 1)]
  verified <- runTool rebuilder ["verify", changedOutPath]
  unless (not (toolStopped verified) && toolCode verified == ExitSuccess) $
    H.failH ("verify refused the changed container: " ++ toolErr verified)

  -- Refusals: a window of the wrong length, and a container whose font window
  -- is not carried after the image-2 extent.
  let refusal label output arguments fragment = do
        result <- runTool rebuilder (arguments ++ [output])
        unless (not (toolStopped result) && toolCode result /= ExitSuccess) $
          H.failH (label ++ ": accepted")
        unless (fragment `isInfixOf` toolErr result) $
          H.failH (label ++ ": wrong refusal: " ++ toolErr result)
        leftover <- doesFileExist output
        unless (not leftover) $ H.failH (label ++ ": refused run wrote an output file")
        modifyIORef' checked (+ 1)
        putStrLn ("refuse " ++ label)
      longPath = root </> "font-long.bin"
      outsidePath = root </> "font-outside.dat"
  B.writeFile longPath (window <> B.singleton 0)
  B.writeFile outsidePath (fixtureBytes baseFixture)
  refusal "replace-font long window" (root </> "font-long-out.dat") ["replace-font", stockPath, longPath] "against the stored flash length"
  refusal "replace-font window outside the post-tail span" (root </> "font-outside-out.dat") ["replace-font", outsidePath, windowPath] "lies outside the span carried after the image-2 extent"
  refusal "extract-font window outside the post-tail span" (root </> "font-outside.bin") ["extract-font", outsidePath] "lies outside the span carried after the image-2 extent"

  -- build-extension: a font-patch-set step writes its word into the window;
  -- the same step with a wrong current word is refused and leaves no output.
  let workspace = root </> "workspace"
  createDirectoryIfMissing True (workspace </> "private-data")
  stockSha <- H.sha256 stockBytes
  windowSha <- H.sha256 window
  let fontSet current =
        H.obj
          [ ("schema_version", H.n 1)
          , ("id", H.s "SYNTHETIC-FONT")
          , ("tier", H.s "synthetic")
          , ("recorded_at", H.s "2026-10-22")
          , ("target", H.obj [("firmware_sha256", H.s stockSha)])
          , ( "image"
            , H.obj
                [ ("segment_id", H.n 3)
                , ("decode_base", H.s "0x003bc000")
                , ("decode_file", H.s "synthetic")
                , ("decode_sha256", H.s windowSha)
                , ("file_offset_rule", H.s "file_offset = address - 0x003bc000")
                , ("dump_correspondence", H.s "synthetic")
                ]
            )
          , ("free_space", H.s "synthetic")
          , ("array_source", H.s "synthetic")
          , ("order_rule", H.s "synthetic")
          , ("blobs", H.arr [])
          , ("words", H.arr [wordEntry 1 (0x003bc000 + 0x1000) 0x1000 current 0xdeadbeef])
          , ("totals", H.s "synthetic")
          , ("unchanged_surfaces", H.s "synthetic")
          , ("container_effect", H.s "synthetic")
          , ("emulation_case_parameters", H.s "synthetic")
          ]
      runExtension name current = do
        let setPath = workspace </> (name ++ "-font.json")
            manifestPath = workspace </> (name ++ ".json")
            output = workspace </> "private-data" </> name
            setBytes = BL.toStrict (A.encode (fontSet current))
        B.writeFile setPath setBytes
        setSha <- H.sha256 setBytes
        B.writeFile
          manifestPath
          ( BL.toStrict
              ( A.encode
                  ( H.obj
                      [ ("schema_version", H.n 1)
                      , ("id", H.s name)
                      , ("title", H.s "synthetic font step")
                      , ("base", H.obj [("model", H.s "X-M1"), ("firmware_version", H.s "1.50"), ("sha256", H.s stockSha)])
                      , ("steps", H.arr [H.obj [("kind", H.s "font-patch-set"), ("path", H.s (name ++ "-font.json")), ("sha256", H.s setSha)]])
                      , ("output_sha256", H.s (replicate 64 '0'))
                      ]
                  )
              )
          )
        (code, _, err) <-
          readCreateProcessWithExitCode
            (proc builder [decoder, applier, encoder, rebuilder, "--unpinned", manifestPath, stockPath, output]) {env = Just [("BUILD_WORKSPACE_DIRECTORY", workspace)]}
            ""
        pure (code, err, output)
  (acceptCode, acceptErr, acceptOutput) <- runExtension "font-accept" (U.word32At window 0x1000)
  unless (acceptCode == ExitSuccess) $ H.failH ("build-extension refused the font step: " ++ acceptErr)
  built <- B.readFile (acceptOutput </> "FWUP0002.DAT")
  expectEqual checked "build-extension font word" (B.map complement (U.putWord32 0xdeadbeef)) (B.take 4 (B.drop (windowFileStart + 0x1000) built))
  expectCarriedIdentity checked "build-extension font step" stockBytes built [(U.mainChecksumOffset, 4), (windowFileStart + 0x1000, 4)]
  (refuseCode, refuseErr, refuseOutput) <- runExtension "font-refuse" (U.word32At window 0x1000 `xor` 1)
  unless (refuseCode /= ExitSuccess && "current bytes disagree" `isInfixOf` refuseErr) $
    H.failH ("build-extension accepted a wrong font current word: " ++ refuseErr)
  leftover <- doesPathExist refuseOutput
  unless (not leftover) $ H.failH "build-extension left an output directory after the refusal"
  modifyIORef' checked (+ 1)
  putStrLn "accept font: extract-font, replace-font round trip, one-byte window change, build-extension font step and its wrong-current refusal"

-- ---------------------------------------------------------------------------
-- rebuild-update write-record: the UTF-16 unit text form and the ASCII form
-- ---------------------------------------------------------------------------

recordCases :: IORef Int -> FilePath -> IO ()
recordCases checked rebuilder = H.withTemporary "fuji-af-record-" $ \root -> do
  let stockPath = root </> "record-stock.dat"
      labelBytes = fixtureBytes labelFixture
      recordOffset = fromIntegral (labelBufferAddress - fxImage2Load labelFixture)
      extentStart = U.stagingLength + image2Media labelFixture
      stockRecord = B.take labelRecordLength (B.drop recordOffset (fixtureImage2 labelFixture))
      currentHex = concatMap (\b -> [hexDigit (b `div` 16), hexDigit (b `mod` 16)]) (B.unpack stockRecord)
      hexDigit d = "0123456789abcdef" !! fromIntegral d
      layoutWords = B.concat (map U.putWord16 [540, 24, 30, 36, 48])
      run name arguments expected = do
        let output = root </> (name ++ ".dat")
        result <- runTool rebuilder (["write-record", "--id", "0x83d", "--layout", "540,24,30,36,48", "--current-hex", currentHex] ++ arguments ++ [output])
        unless (not (toolStopped result) && toolCode result == ExitSuccess) $
          H.failH (name ++ ": write-record refused: " ++ toolErr result)
        written <- B.readFile output
        let media = B.map complement (B.take (fxImage2Size labelFixture) (B.drop extentStart written))
        expectEqual checked (name ++ " record") expected (B.take labelRecordLength (B.drop recordOffset media))
        expectCarriedIdentity checked name labelBytes written [(U.mainChecksumOffset, 4), (extentStart + recordOffset, labelRecordLength)]
  B.writeFile stockPath labelBytes
  run "record-utf16" ["--text-utf16", stockPath, "f073"] (B.concat (map U.putWord16 [0xf073, 0]) <> B.replicate 52 0 <> layoutWords)
  run "record-ascii" [stockPath, "CC"] (B.concat (map U.putWord16 [0x43, 0x43, 0]) <> B.replicate 50 0 <> layoutWords)
  result <- runTool rebuilder ["write-record", "--id", "0x83d", "--layout", "540,24,30,36,48", "--current-hex", currentHex, "--text-utf16", stockPath, "F073", root </> "upper.dat"]
  unless (not (toolStopped result) && toolCode result /= ExitSuccess && "lowercase" `isInfixOf` toolErr result) $
    H.failH ("write-record accepted uppercase unit hex: " ++ toolErr result)
  modifyIORef' checked (+ 1)
  putStrLn "accept write-record: UTF-16 unit and ASCII texts"

data WindowImage = Compressed4 | Compressed5 | Raw6

changeWindow :: WindowImage -> (U.SegmentEntry -> U.SegmentEntry) -> U.Parts -> U.Parts
changeWindow image change parts =
  parts {U.partSegments = map alter (U.partSegments parts)}
  where
    wanted = case image of
      Compressed4 -> 4
      Compressed5 -> 5
      Raw6 -> 6
    alter entry = if U.segId entry == wanted then change entry else entry

geometrySourceBytes :: Fixture -> (U.Parts -> U.Parts) -> IO B.ByteString
geometrySourceBytes fixture change = do
  let sourceBytes = fixtureBytes fixture
  source <- either H.failH pure (U.parseContainer sourceBytes)
  let parts = change (U.parsedParts source)
      tableStart = U.stagingLength + U.segmentTableOffset (U.partTableLayout parts)
      tableBytes = B.concat (map serializeEntry (U.partSegments parts))
      changedBytes = B.take tableStart sourceBytes <> tableBytes <> B.drop (tableStart + B.length tableBytes) sourceBytes
      mainStart = U.stagingLength + fromIntegral (U.hMainOffset (U.parsedHeader source))
      mainLength = fromIntegral (U.hMainLength (U.parsedHeader source))
      checksum = mainSum (B.take mainLength (B.drop mainStart changedBytes))
  pure (patchedWord U.mainChecksumOffset checksum changedBytes)

windowAdmissionCases :: IORef Int -> FilePath -> IO ()
windowAdmissionCases checked rebuilder = H.withTemporary "fuji-af-window-admission-" $ \root -> do
  let controlPath = root </> "valid-source.dat"
      controlOutput = root </> "valid-roundtrip.dat"
      stockBytes = fixtureBytes baseFixture
  B.writeFile controlPath stockBytes
  control <- runTool rebuilder ["round-trip", controlPath, controlOutput]
  unless (not (toolStopped control) && toolCode control == ExitSuccess) $
    H.failH ("valid geometry round-trip refused: " ++ toolErr control)
  expectEqual checked "valid geometry round-trip bytes" stockBytes =<< B.readFile controlOutput
  let refuse name fixture change = do
        let sourcePath = root </> (name ++ "-source.dat")
            outputPath = root </> (name ++ "-out.dat")
        malformed <- geometrySourceBytes fixture change
        _ <- either H.failH pure (U.parseContainer malformed)
        B.writeFile sourcePath malformed
        result <- runTool rebuilder ["round-trip", sourcePath, outputPath]
        unless (not (toolStopped result) && toolCode result /= ExitSuccess) $
          H.failH (name ++ ": malformed geometry round-trip was accepted")
        leftover <- doesFileExist outputPath
        unless (not leftover) $
          H.failH (name ++ ": refused round-trip wrote an output file")
        modifyIORef' checked (+ 1)
      trailerStart = B.length (fixtureBodyPrefix baseFixture)
      mainEnd = trailerStart + U.trailerBytes
  forM_ [(Compressed4, record4Media, fxRecord5Media baseFixture), (Compressed5, fxRecord5Media baseFixture, fromIntegral (fxPlainFlash baseFixture))] $ \(image, start, nextRegion) -> do
    let imageLabel = case image of
          Compressed4 -> "image4"
          Compressed5 -> "image5"
          Raw6 -> "image6"
        cases =
          [ ("oversized-declared-window", 0xffffffff)
          , ("crosses-next-region", fromIntegral (nextRegion - start + 1))
          , ("crosses-trailer-boundary", fromIntegral (trailerStart - start + 1))
          , ("crosses-main-boundary", fromIntegral (mainEnd - start + 1))
          , ("address-wrap", fromInteger (0x100000000 - toInteger start + 1))
          ]
    forM_ cases $ \(name, declaredLength) ->
      refuse (imageLabel ++ "-" ++ name) baseFixture (changeWindow image (\entry -> entry {U.segFlashLen = declaredLength}))
  forM_
    [ ("plain-start-zero", changeWindow Raw6 (\entry -> entry {U.segFlash = 0}))
    , ("plain-short-aligned-length", changeWindow Raw6 (\entry -> entry {U.segFlashLen = 0x20000}))
    , ("plain-final-extent-outside-window", changeWindow Raw6 (\entry -> entry {U.segFlashLen = 0x340000}))
    , ("plain-unaligned-start", changeWindow Raw6 (\entry -> entry {U.segFlash = fxPlainFlash baseFixture + 1}))
    , ("plain-unaligned-length", changeWindow Raw6 (\entry -> entry {U.segFlashLen = 0x360001}))
    ]
    $ \(name, change) -> refuse name baseFixture change
  refuse
    "plain-nominal-header-disagreement"
    (baseFixture {fxImage2Gap = fxImage2Gap baseFixture + 0x20000})
    (changeWindow Raw6 (\entry -> entry {U.segFlashLen = 0x380000}))
  refuse "plain-outside-carrier" extractionRefusalFixture id

main :: IO ()
main = do
  outcome <- try $ do
    arguments <- getArgs
    (applier, rebuilder, decoder, encoder, builder) <- case arguments of
      [applyPath, rebuildPath, decoderPath, encoderPath, builderPath] -> do
        applyExe <- canonicalizePath applyPath
        rebuildExe <- canonicalizePath rebuildPath
        decoderExe <- canonicalizePath decoderPath
        encoderExe <- canonicalizePath encoderPath
        builderExe <- canonicalizePath builderPath
        pure (applyExe, rebuildExe, decoderExe, encoderExe, builderExe)
      _ -> H.failH "usage: container-build-tests APPLY_PATCH_SET REBUILD_UPDATE FFLZD LZ_ENCODE BUILD_EXTENSION"
    checked <- newIORef 0
    applierCases checked applier
    loadAddressCases checked rebuilder decoder
    image2Cases checked rebuilder
    plainCases checked rebuilder
    fontCases checked rebuilder applier decoder encoder builder
    recordCases checked rebuilder
    windowAdmissionCases checked rebuilder
    total <- readIORef checked
    putStrLn (show total ++ " container-build path checks passed")
  case outcome :: Either SomeException () of
    Left err -> hPutStrLn stderr (displayException err) >> exitFailure
    Right () -> pure ()
