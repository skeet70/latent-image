{-# LANGUAGE OverloadedStrings #-}
-- Behavioral tests for the update-container parts/checks/gated-rebuild contract:
-- hand-built synthetic containers in, accept/reject and parsed values out.
module Main where
import qualified FujiAF.Host as H
import Control.Exception (SomeException, displayException, try)
import Control.Monad (forM_, unless)
import Data.Bits (complement, xor)
import qualified Data.ByteString as B
import Data.IORef (IORef, modifyIORef', newIORef, readIORef)
import Data.List (isInfixOf)
import Data.Word (Word32)
import qualified FujiAF.UpdateContainer as U
import System.Exit (exitFailure)
import System.IO (hPutStrLn, stderr)

record4Media :: Int
record4Media = 0x140200

record5Media :: Int
record5Media = 0x140240

-- The fixture's plain-image base, the id-6 entry's flash word the image-5
-- window abuts (stock: 0x5a0000, the plain image's first byte). Both id-6
-- flash words are 0x20000-aligned, as the loader's block copy requires.
plainMedia :: Int
plainMedia = 0x160000

-- The fixture's id-2 extent: the id-2 entry describes it and the loader reads
-- it raw, so the body carries it between the plain window and the trailer.
image2Media :: Int
image2Media = 0x4d0000

-- The window-refusal fixture carries the id-2 extent inside the plain window
-- [plainMedia, plainMedia + 0x360000), and the carrier refuses it as a window
-- the carried span does not contain.
windowRefusalImage2Media :: Int
windowRefusalImage2Media = 0x146000

image2Size :: Int
image2Size = 0x200

record5End :: Int
record5End = record5Media + B.length (serializeRecordBytes fixtureRecord5)

-- The fixture's extent bytes in media form (the loader's raw copy); the body
-- carries their complement, as every main-range byte is stored.
fixtureImage2 :: B.ByteString
fixtureImage2 = B.pack [fromIntegral ((i * 5 + 7) `mod` 251) | i <- [0 .. image2Size - 1]]

tableEnd :: Int
tableEnd = U.segmentTableOffset + U.segmentEntryBytes * U.segmentCount + 4

patternBytes :: Int -> B.ByteString
patternBytes n = B.pack [fromIntegral ((i * 7 + 3) `mod` 251) | i <- [0 .. n - 1]]

-- Bytes that differ from patternBytes at every index.
differingBytes :: Int -> B.ByteString
differingBytes n = B.pack [255 - fromIntegral ((i * 7 + 3) `mod` 251) | i <- [0 .. n - 1]]

patchMediaWord :: Int -> Word32 -> B.ByteString -> B.ByteString
patchMediaWord off w bytes = B.take off bytes <> U.putMediaWord w <> B.drop (off + 4) bytes

fixtureSegments :: [U.SegmentEntry]
fixtureSegments = fixtureSegmentsAt image2Media

fixtureSegmentsAt :: Int -> [U.SegmentEntry]
fixtureSegmentsAt image2At =
  [ U.SegmentEntry 0 0x1000 0x200 0 0 0x30000 0x10000 0 0
  , U.SegmentEntry 1 0x2000 0x200 0 0 0x31000 0x10000 0 0
  , U.SegmentEntry 2 0x3000 (fromIntegral image2Size) 0 0 (fromIntegral image2At) 0x10000 0 0
  , U.SegmentEntry 3 0x4000 0x200 0 0 0x33000 0x10000 0 0
  , U.SegmentEntry 4 0x61c400 0x282828 0x89ec00 0x352e00 (fromIntegral record4Media) 0x10000 0x20 1
  , -- The image-5 flash window closes at the id-6 base, as the stock
    -- container's closes at the plain image's flash base; the id-2 extent
    -- starts past it, as in the stock container, so a declared window overrun
    -- reaches the region bound before it reaches the extent.
    U.SegmentEntry 5 0xbf1a00 0x4f1e60 0x1460000 0x2bbe40 (fromIntegral record5Media) (fromIntegral (plainMedia - record5Media)) 0x20 1
  , U.SegmentEntry 6 0xbf1a00 0x4f1e60 0x1460000 0x2bbe40 (fromIntegral plainMedia) 0x360000 0x20 0
  , U.SegmentEntry 7 0 0 0 0 0xb40000 0x20000 0 0
  ]

fixtureRecord4 :: U.CompressedRecord
fixtureRecord4 = U.CompressedRecord (U.RecordHeader 0x282800 0x34 1 0x4000 4) (patternBytes 0x20) Nothing

fixtureRecord5 :: U.CompressedRecord
fixtureRecord5 = U.CompressedRecord (U.RecordHeader 0x4f1e60 0x4015 2 0x4000 4) (patternBytes 0x4001) Nothing

-- The image-5 decoded image a replacement attaches: the fixture's declared
-- decoded size, so the decoded-size binding holds. The bytes are a pattern;
-- the library checks the length and the strict decoder gates the content in
-- the CLI chain.
fixtureDecoded5 :: B.ByteString
fixtureDecoded5 = patternBytes 0x4f1e60

fixtureIdentity :: B.ByteString
fixtureIdentity = "IDENTITYRECORD01" <> patternBytes (U.identityBytes - 16)

-- The fixture is a container for this body, so it carries the stock restore
-- lists; the loader destination word and the stock plain-extent pins sit in
-- its table tail, each at its own offset.
fixtureTail :: B.ByteString
fixtureTail =
  foldl
    (\t (off, w) -> patchMediaWord (off - tableEnd) w t)
    (patternBytes (record4Media - tableEnd))
    ( [(baseOffset, base) | ((baseOffset, base), _) <- U.stockPlainExtents]
        ++ [(lengthOffset, len) | (_, (lengthOffset, len)) <- U.stockPlainExtents]
        ++ [U.stockPlainLoad]
    )

fixtureAux1Bytes :: B.ByteString
fixtureAux1Bytes = B.concat [U.putWord16 (U.pair16Offset p) <> U.putWord16 (U.pair16Length p) | p <- U.stockAux1]

fixtureAux2Bytes :: B.ByteString
fixtureAux2Bytes = B.concat [U.putWord32 (U.pair32Offset p) <> U.putWord32 (U.pair32Length p) | p <- U.stockAux2]

fixtureBodyAt :: Int -> B.ByteString
fixtureBodyAt image2At =
  let prefix = fixtureBodyPrefixAt image2At
   in prefix
        <> B.concat
          [ U.putMediaWord (fromIntegral (B.length prefix))
          , U.putMediaWord (fromIntegral (B.length prefix))
          , U.putMediaWord 0x20
          , U.putMediaWord 0
          , U.putMediaWord 0
          , U.putMediaWord (U.segLoad (fixtureSegmentsAt image2At !! 4))
          , U.putMediaWord (U.segSize (fixtureSegmentsAt image2At !! 4))
          , U.putMediaHalf 0xffff
          , U.putMediaHalf 0xaaaa
          ]

fixtureBodyPrefix :: B.ByteString
fixtureBodyPrefix = fixtureBodyPrefixAt image2Media

fixtureBodyPrefixAt :: Int -> B.ByteString
fixtureBodyPrefixAt image2At =
  B.concat
    [ patternBytes U.segmentTableOffset
    , B.concat [serializeEntry e | e <- fixtureSegmentsAt image2At]
    , U.putMediaWord 0xffffffff
    , fixtureTail
    , serializeRecordBytes fixtureRecord4
    , patternBytes (record5Media - record4Media - B.length (serializeRecordBytes fixtureRecord4))
    , serializeRecordBytes fixtureRecord5
    , patternBytes (image2At - record5End)
    , B.map complement fixtureImage2
    , patternBytes 5
    ]

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

fixtureBytes :: B.ByteString
fixtureBytes = fixtureBytesAt image2Media

fixtureBytesAt :: Int -> B.ByteString
fixtureBytesAt image2At =
  let mainBody = fixtureBodyAt image2At
      aux1 = fixtureAux1Bytes
      aux2 = fixtureAux2Bytes
      mainLength = fromIntegral (B.length mainBody)
      aux1Length = fromIntegral (B.length aux1)
   in B.concat
        [ U.putWord32 4
        , fixtureIdentity
        , U.putWord32 1
        , U.putWord32 0x50
        , U.putWord32 (mainSum mainBody)
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
        , mainBody
        , aux1
        , aux2
        ]

mainSum :: B.ByteString -> Word32
mainSum body = complement (B.foldl' (\acc b -> acc + fromIntegral (complement b)) 0 body)

auxSum :: B.ByteString -> Word32
auxSum bytes = complement (B.foldl' (\acc b -> acc + fromIntegral b) 0 bytes)

expectCheck :: IORef Int -> String -> Bool -> [U.Check] -> IO ()
expectCheck checked wantField wantPass results = do
  case filter ((== wantField) . U.checkField) results of
    [one] ->
      H.ensure
        (U.checkPassed one == wantPass)
        ("check " ++ wantField ++ " was " ++ show (U.checkPassed one) ++ ", expected " ++ show wantPass)
    [] -> H.failH ("check missing: " ++ wantField)
    many -> H.failH ("check duplicated: " ++ wantField ++ show (length many))
  modifyIORef' checked (+ 1)

expectTrue :: IORef Int -> String -> Bool -> IO ()
expectTrue checked label ok = do
  H.ensure ok label
  modifyIORef' checked (+ 1)

expectAccepted :: IORef Int -> String -> B.ByteString -> IO U.Parsed
expectAccepted checked label bytes = case U.parseContainer bytes of
  Left message -> H.failH (label ++ ": unexpected parse failure: " ++ message)
  Right parsed -> do
    modifyIORef' checked (+ 1)
    pure parsed

expectRejected :: IORef Int -> String -> String -> B.ByteString -> IO ()
expectRejected checked label fragment bytes = case U.parseContainer bytes of
  Left message -> do
    H.ensure (fragment `isInfixOf` message) (label ++ ": wrong parse failure: " ++ message)
    modifyIORef' checked (+ 1)
  Right _ -> H.failH (label ++ ": unexpected parse success")

-- A check-level rejection over mutated parts: the named check fails while the
-- recomputed main checksum stays clean.
expectPartsFailure :: IORef Int -> String -> (U.Parts -> U.Parts) -> String -> IO ()
expectPartsFailure checked label change field = do
  parsed <- case U.parseContainer fixtureBytes of
    Left message -> H.failH (label ++ ": fixture does not parse: " ++ message)
    Right value -> pure value
  let parts = change (U.parsedParts parsed)
      results = U.checks (U.Parsed (B.length fixtureBytes) (U.derivedHeader parts) parts)
  expectCheck checked field False results
  unless (field == "main checksum") (expectCheck checked "main checksum" True results)

-- A check-level rejection over a corrupted stored header word.
expectStoredFailure :: IORef Int -> String -> (U.Header -> U.Header) -> String -> IO ()
expectStoredFailure checked label change field = do
  parsed <- expectAccepted checked (label ++ " base") fixtureBytes
  let parts = U.parsedParts parsed
      results = U.checks (U.Parsed (B.length fixtureBytes) (change (U.derivedHeader parts)) parts)
  expectCheck checked field False results
  unless (field == "main checksum") (expectCheck checked "main checksum" True results)

-- The emission gate must refuse, naming the offending field.
expectEmitRefused :: IORef Int -> String -> U.Parsed -> (U.Parts -> U.Parts) -> String -> IO ()
expectEmitRefused checked label source change fragment = do
  case U.rebuildContainer source (change (U.parsedParts source)) of
    Left message -> do
      H.ensure (fragment `isInfixOf` message) (label ++ ": wrong refusal: " ++ message)
      modifyIORef' checked (+ 1)
    Right _ -> H.failH (label ++ ": emission was allowed")

withHeader :: (U.RecordHeader -> U.RecordHeader) -> U.CompressedRecord -> U.CompressedRecord
withHeader change record = record {U.recHeader = change (U.recHeader record)}

main :: IO ()
main = do
  outcome <- try run
  case outcome :: Either SomeException () of
    Left err -> hPutStrLn stderr (displayException err) >> exitFailure
    Right () -> pure ()

run :: IO ()
run = do
  checked <- newIORef (0 :: Int)
  parsed <- expectAccepted checked "fixture" fixtureBytes
  let parts = U.parsedParts parsed
  expectTrue checked "identity bytes" (U.partIdentity parts == fixtureIdentity)
  expectTrue checked "version words" (U.partVersionMajor parts == 1 && U.partVersionMinor parts == 0x50)
  expectTrue checked "selector word" (U.partSelector parts == 1)
  expectTrue checked "segment entries" (U.partSegments parts == fixtureSegments)
  expectTrue checked "image 2 extent bytes" (U.partImage2 parts == B.map complement fixtureImage2)
  expectTrue checked "image 2 extent geometry" (U.image2Extent parts == (image2Media, image2Size, 0x3000))
  expectTrue checked "image 4 record" (U.partRecord4 parts == fixtureRecord4)
  expectTrue checked "image 5 record" (U.partRecord5 parts == fixtureRecord5)
  expectTrue checked "trailer" (U.trMarkerHalf (U.partTrailer parts) == 0xaaaa && U.trSize (U.partTrailer parts) == 0x20)
  expectTrue checked "trailer span" (U.trSpan (U.partTrailer parts) == fromIntegral (B.length fixtureBodyPrefix))
  expectTrue checked "restore pairs" (U.partAux1 parts == U.stockAux1 && U.partAux2 parts == U.stockAux2)
  expectTrue checked "file length" (U.parsedFileLength parsed == B.length fixtureBytes)
  expectTrue checked "derived header" (U.derivedHeader parts == U.parsedHeader parsed)
  -- The gated emitter reproduces the container byte for byte from its parts.
  case U.rebuildContainer parsed parts of
    Right rebuilt -> expectTrue checked "round trip" (rebuilt == fixtureBytes)
    Left message -> H.failH ("round trip refused: " ++ message)
  -- The image-2 rebuild: the extent the stored words describe takes any
  -- same-length image, and the emitted container is checked against the words
  -- read back from the assembly.
  case U.replaceImage2 parsed (U.MediaImage fixtureImage2) of
    Left message -> H.failH ("image-2 identity rebuild refused: " ++ message)
    Right imageParts -> case U.rebuildContainer parsed imageParts of
      Left message -> H.failH ("image-2 identity rebuild refused: " ++ message)
      Right rebuilt -> expectTrue checked "image-2 identity rebuild" (rebuilt == fixtureBytes)
  case U.replaceImage2 parsed (U.MediaImage (B.cons 0x5a fixtureImage2)) of
    Left message -> expectTrue checked "image-2 length refusal" ("against the stored size word" `isInfixOf` message)
    Right _ -> H.failH "an image-2 replacement of the wrong length was accepted"
  -- The fixture's stored extent bytes are complements of values at most 250,
  -- so 1 differs from every one of them.
  case U.rebuildContainer parsed ((U.parsedParts parsed) {U.partImage2 = B.replicate image2Size 0x01}) of
    Left message -> H.failH ("image-2 content change refused: " ++ message)
    Right bytes -> do
      changed <- expectAccepted checked "image-2 content change" bytes
      case filter ((== "image 2 extent") . U.diffPart) (U.diffParts parsed changed) of
        [d] -> expectTrue checked "image-2 extent diff" (U.diffChangedBytes d == image2Size)
        _ -> H.failH "image-2 extent diff missing"
  -- A shorter extent shifts the trailer and the stored words no longer
  -- describe it; an extent moved inside the body is caught by the emitter's
  -- read-back against the stored words.
  expectEmitRefused
    checked
    "image-2 short drift"
    parsed
    (\p -> p {U.partImage2 = B.take (image2Size - 0x10) (U.partImage2 p)})
    "image 2 extent overlaps the trailer"
  expectEmitRefused
    checked
    "image-2 length drift"
    parsed
    ( \p ->
        p
          { U.partImage2 = U.partImage2 p <> B.take 4 (U.partPostTail p)
          , U.partPostTail = B.drop 4 (U.partPostTail p)
          }
    )
    "assembled image 2 extent"
  expectEmitRefused
    checked
    "image-2 offset drift"
    parsed
    ( \p ->
        p
          { U.partPostHead = U.partPostHead p <> B.singleton 0x2a
          , U.partImage2 = B.drop 1 (U.partImage2 p) <> B.take 1 (U.partPostTail p)
          , U.partPostTail = B.drop 1 (U.partPostTail p)
          }
    )
    "assembled image 2 extent"
  -- The image-5 record replacement: the identity rebuild reproduces the
  -- fixture, a changed stream slides inside the window with every media offset
  -- beyond it keeping the stock byte, and the gates refuse a record the
  -- window, the decoded size or the carried set does not describe.
  let stock5 = U.partRecord5 parts
      (window5Start, window5End) = U.record5Window parts
      tail5Bytes = window5End - window5Start - U.recordHeaderBytes - B.length (U.recStream stock5)
      record5Of stream =
        fixtureRecord5
          { U.recHeader =
              (U.recHeader fixtureRecord5)
                { U.recTotalLen = fromIntegral (U.recordHeaderBytes + B.length stream)
                , U.recChunkCount = fromIntegral ((U.recordHeaderBytes + B.length stream + 0x3fff) `div` 0x4000)
                }
          , U.recStream = stream
          , U.recDecoded = Just fixtureDecoded5
          }
      source5 change =
        let changed = change parts
         in U.Parsed (B.length fixtureBytes) (U.derivedHeader changed) changed
      withFlashLen5 value =
        source5 (\p -> p {U.partSegments = [if U.segId e == 5 then e {U.segFlashLen = value} else e | e <- U.partSegments p]})
      expectReplacement5 label source replacement fragment = case U.replaceRecord5 source replacement of
        Left message -> do
          H.ensure (fragment `isInfixOf` message) (label ++ ": wrong refusal: " ++ message)
          modifyIORef' checked (+ 1)
        Right _ -> H.failH (label ++ ": replacement was accepted")
      rebuildReplacement5 label replacement = case U.replaceRecord5 parsed replacement of
        Left message -> H.failH (label ++ ": replacement refused: " ++ message)
        Right replaced -> case U.rebuildContainer parsed replaced of
          Left message -> H.failH (label ++ ": rebuild refused: " ++ message)
          Right bytes -> pure bytes
      windowRule label stream rebuilt = do
        parsedRebuilt <- case U.parseContainer rebuilt of
          Left message -> H.failH (label ++ ": emitted container does not parse: " ++ message)
          Right value -> pure value
        let stored5 = U.partRecord5 (U.parsedParts parsedRebuilt)
            newEnd = U.stagingLength + window5Start + U.recordHeaderBytes + B.length stream
            allowed i =
              (i >= U.mainChecksumOffset && i < U.mainChecksumOffset + 4)
                || (i >= U.stagingLength + window5Start && i < newEnd)
            differing = [i | i <- [0 .. B.length fixtureBytes - 1], B.index fixtureBytes i /= B.index rebuilt i]
        expectTrue checked (label ++ " stored stream") (U.recStream stored5 == stream)
        expectTrue
          checked
          (label ++ " stored length word")
          (U.recTotalLen (U.recHeader stored5) == fromIntegral (U.recordHeaderBytes + B.length stream))
        expectTrue checked (label ++ " window rule") (all allowed differing)
        expectTrue checked (label ++ " bytes moved") (not (null differing))
      identityReplacement = record5Of (U.recStream stock5)
  case U.replaceRecord5 parsed identityReplacement of
    Left message -> H.failH ("image-5 identity replacement refused: " ++ message)
    Right identityParts -> do
      expectTrue
        checked
        "image-5 identity record"
        ( U.recHeader (U.partRecord5 identityParts) == U.recHeader stock5
            && U.recStream (U.partRecord5 identityParts) == U.recStream stock5
        )
      case U.rebuildContainer parsed identityParts of
        Left message -> H.failH ("image-5 identity rebuild refused: " ++ message)
        Right rebuilt -> expectTrue checked "image-5 identity rebuild" (rebuilt == fixtureBytes)
  shortStream <- rebuildReplacement5 "image-5 short stream" (record5Of (B.take 0x4000 (U.recStream stock5)))
  windowRule "image-5 short stream" (B.take 0x4000 (U.recStream stock5)) shortStream
  longStream <- rebuildReplacement5 "image-5 long stream" (record5Of (U.recStream stock5 <> differingBytes 4))
  windowRule "image-5 long stream" (U.recStream stock5 <> differingBytes 4) longStream
  filledStream <- rebuildReplacement5 "image-5 window filled" (record5Of (U.recStream stock5 <> differingBytes tail5Bytes))
  windowRule "image-5 window filled" (U.recStream stock5 <> differingBytes tail5Bytes) filledStream
  let movedSize = identityReplacement {U.recHeader = (U.recHeader identityReplacement) {U.recDecodedSize = U.recDecodedSize (U.recHeader fixtureRecord5) + 1}}
      pastWindow = record5Of (U.recStream stock5 <> patternBytes (tail5Bytes + 1))
  expectReplacement5 "image-5 no decoded" parsed fixtureRecord5 "requires the decoded image"
  expectReplacement5 "image-5 decoded size" parsed movedSize "changes the decoded size"
  expectReplacement5 "image-5 stream past window" parsed pastWindow "does not fit the image-5 flash window"
  expectReplacement5
    "image-5 window past region"
    (withFlashLen5 (fromIntegral (window5End - window5Start + 0x1000)))
    identityReplacement
    "ends past the next segment region"
  expectReplacement5
    "image-5 window past extent"
    (withFlashLen5 (fromIntegral (image2Media - record5Media + 0x1000)))
    identityReplacement
    "ends past the image-2 extent"
  expectReplacement5
    "image-5 window overrun"
    (withFlashLen5 0x4000)
    identityReplacement
    "overruns its declared flash window"
  forM_ (U.checks parsed ++ U.carriedChecks parsed) $ \c ->
    expectTrue checked ("fixture check " ++ U.checkField c) (U.checkPassed c)
  -- Stored-header words that disagree with the parts.
  expectStoredFailure checked "header type" (\h -> h {U.hType = 5}) "header type"
  expectStoredFailure checked "main offset" (\h -> h {U.hMainOffset = 1}) "main offset"
  expectStoredFailure checked "main length" (\h -> h {U.hMainLength = U.hMainLength h - 1}) "main length"
  expectStoredFailure checked "main checksum" (\h -> h {U.hMainChecksum = 0}) "main checksum"
  expectStoredFailure checked "auxiliary 1 offset" (\h -> h {U.hAux1Offset = U.hAux1Offset h - 0x10}) "auxiliary 1 offset"
  expectStoredFailure checked "auxiliary 1 length" (\h -> h {U.hAux1Length = U.hAux1Length h + 4}) "auxiliary 1 length"
  expectStoredFailure checked "auxiliary 1 checksum" (\h -> h {U.hAux1Checksum = 0}) "auxiliary 1 checksum"
  expectStoredFailure checked "auxiliary 2 offset" (\h -> h {U.hAux2Offset = U.hAux2Offset h + 8}) "auxiliary 2 offset"
  expectStoredFailure checked "auxiliary 2 length" (\h -> h {U.hAux2Length = 0}) "auxiliary 2 length"
  expectStoredFailure checked "auxiliary 2 checksum" (\h -> h {U.hAux2Checksum = 0}) "auxiliary 2 checksum"
  parsedExtent <- expectAccepted checked "file extent base" fixtureBytes
  expectCheck
    checked
    "file extent"
    False
    (U.checks (U.Parsed (B.length fixtureBytes - 1) (U.parsedHeader parsedExtent) (U.parsedParts parsedExtent)))
  -- Parts that disagree with each other or with the pinned relations.
  expectPartsFailure checked "identity" (\p -> p {U.partIdentity = B.replicate U.identityBytes 0}) "identity records"
  expectPartsFailure checked "version format" (\p -> p {U.partVersionMajor = 0x100}) "version format"
  expectPartsFailure checked "version policy" (\p -> p {U.partVersionMinor = 0x4f}) "version policy"
  expectPartsFailure checked "selector" (\p -> p {U.partSelector = 0}) "selector"
  expectPartsFailure checked "range fill" (\p -> p {U.partRangeFill = B.replicate 0x28 0}) "range fill"
  expectPartsFailure checked "auxiliary 1 bounds" (\p -> p {U.partAux1 = U.Pair16 0xffff 0xffff : U.partAux1 p}) "auxiliary 1 pair bounds"
  expectPartsFailure checked "auxiliary 2 bounds" (\p -> p {U.partAux2 = U.Pair32 0x40000 0x20000 : U.partAux2 p}) "auxiliary 2 pair bounds"
  expectPartsFailure checked "segment table" (\p -> p {U.partSegments = [if U.segId e == 3 then e {U.segId = 9} else e | e <- U.partSegments p]}) "segment table"
  expectPartsFailure checked "segment flags" (\p -> p {U.partSegments = [if U.segId e == 4 then e {U.segFlag = 0} else e | e <- U.partSegments p]}) "segment flags"
  expectPartsFailure checked "image 4 size" (\p -> p {U.partSegments = [if U.segId e == 4 then e {U.segSize = 0x282830} else e | e <- U.partSegments p]}) "image 4 size relation"
  expectPartsFailure checked "image 5 size" (\p -> p {U.partSegments = [if U.segId e == 5 then e {U.segSize = 0x4f1e61} else e | e <- U.partSegments p]}) "image 5 size relation"
  expectPartsFailure checked "image 4 bss" (\p -> p {U.partSegments = [if U.segId e == 4 then e {U.segBss = 0x89ec04} else e | e <- U.partSegments p]}) "image 4 bss relation"
  expectPartsFailure
    checked
    "image 4 decoded"
    (\p -> p {U.partRecord4 = (U.partRecord4 p) {U.recDecoded = Just (patternBytes 0x20)}})
    "image 4 record decoded size"
  expectPartsFailure
    checked
    "image 5 decoded"
    (\p -> p {U.partRecord5 = (U.partRecord5 p) {U.recDecoded = Just (patternBytes 0x4000)}})
    "image 5 record decoded size"
  expectPartsFailure
    checked
    "image 4 chunk"
    (\p -> p {U.partRecord4 = withHeader (\h -> h {U.recChunkCount = 3}) (U.partRecord4 p)})
    "image 4 record header arithmetic"
  expectPartsFailure
    checked
    "image 5 chunk"
    (\p -> p {U.partRecord5 = withHeader (\h -> h {U.recChunkCount = 1}) (U.partRecord5 p)})
    "image 5 record header arithmetic"
  expectPartsFailure
    checked
    "image 4 stream"
    -- Word 1 set to the stream length: the conflated value the record-length
    -- check exists to refuse.
    (\p -> p {U.partRecord4 = withHeader (\h -> h {U.recTotalLen = 0x20}) (U.partRecord4 p)})
    "image 4 record length"
  expectPartsFailure
    checked
    "image 5 stream"
    (\p -> p {U.partRecord5 = withHeader (\h -> h {U.recTotalLen = 0x4001}) (U.partRecord5 p)})
    "image 5 record length"
  expectPartsFailure
    checked
    "image 2 window"
    (\p -> p {U.partSegments = [if U.segId e == 2 then e {U.segFlashLen = 0x100} else e | e <- U.partSegments p]})
    "image 2 size relation"
  expectPartsFailure
    checked
    "image 4 window"
    (\p -> p {U.partSegments = [if U.segId e == 4 then e {U.segFlashLen = 0x20} else e | e <- U.partSegments p]})
    "image 4 record flash window"
  expectPartsFailure
    checked
    "image 5 window"
    (\p -> p {U.partSegments = [if U.segId e == 5 then e {U.segFlashLen = 0x20} else e | e <- U.partSegments p]})
    "image 5 record flash window"
  expectPartsFailure checked "trailer marker" (\p -> p {U.partTrailer = (U.partTrailer p) {U.trMarkerHalf = 0xaaab}}) "trailer marker"
  expectPartsFailure checked "trailer size" (\p -> p {U.partTrailer = (U.partTrailer p) {U.trSize = 0x21}}) "trailer size"
  expectPartsFailure checked "trailer span" (\p -> p {U.partTrailer = (U.partTrailer p) {U.trSpan = 0x20}}) "trailer span"
  expectPartsFailure checked "trailer image" (\p -> p {U.partTrailer = (U.partTrailer p) {U.trImageSize = 0x1234}}) "trailer image words"
  expectPartsFailure
    checked
    "plain extent"
    (\p -> p {U.partTableTail = patchMediaWord (0x1401b4 - tableEnd) 0 (U.partTableTail p)})
    "plain extent 0x01100000"
  expectPartsFailure
    checked
    "plain load"
    (\p -> p {U.partTableTail = patchMediaWord (0x1401a8 - tableEnd) 0 (U.partTableTail p)})
    "plain load 0x01100000"
  -- Emission refuses drifted carried parts and stale size words.
  expectEmitRefused checked "drop pair" parsed (\p -> p {U.partAux1 = init (U.partAux1 p)}) "carried part changed: restore list 1"
  expectEmitRefused checked "alter pair" parsed (\p -> p {U.partAux2 = [U.Pair32 0x179d5 0x8, U.Pair32 0x179df 0x4, U.Pair32 0x18ac1 0x1]}) "carried part changed: restore list 2"
  expectEmitRefused checked "identity change" parsed (\p -> p {U.partIdentity = B.replicate U.identityBytes 0x41}) "carried part changed: identity records"
  expectEmitRefused checked "selector change" parsed (\p -> p {U.partSelector = 0}) "carried part changed: selector word"
  expectEmitRefused checked "fill change" parsed (\p -> p {U.partRangeFill = B.replicate 0x28 0}) "carried part changed: range fill"
  expectEmitRefused checked "downgrade" parsed (\p -> p {U.partVersionMinor = 0x4f}) "version policy"
  expectEmitRefused
    checked
    "stale size"
    parsed
    (\p -> p {U.partSegments = [if U.segId e == 4 then e {U.segSize = 0x282830} else e | e <- U.partSegments p]})
    "image 4 size relation"
  expectEmitRefused
    checked
    "stale decoded"
    parsed
    (\p -> p {U.partRecord4 = (U.partRecord4 p) {U.recDecoded = Just (patternBytes 0x20)}})
    "image 4 record decoded size"
  -- A source that already carries wrong lists cannot emit through either layer.
  let badParts = (U.parsedParts parsed) {U.partAux1 = init U.stockAux1}
      badSource = U.Parsed (B.length fixtureBytes) (U.derivedHeader badParts) badParts
  expectEmitRefused checked "non-stock source" badSource id "restore list 1 carried"
  -- A legitimate version bump and a same-length image change emit.
  bumped <- case U.rebuildContainer parsed ((U.parsedParts parsed) {U.partVersionMinor = 0x51}) of
    Left message -> H.failH ("version bump refused: " ++ message)
    Right bytes -> expectAccepted checked "version bump" bytes
  expectCheck checked "version policy" True (U.checks bumped)
  expectTrue checked "bump diff policy" (all (\d -> U.diffPart d /= "version words" || U.diffPolicy d == U.CarriedPolicyChecked) (U.diffParts parsed bumped))
  changedStream <- case U.rebuildContainer parsed ((U.parsedParts parsed) {U.partRecord4 = (U.partRecord4 (U.parsedParts parsed)) {U.recStream = B.replicate 0x20 0x5a}}) of
    Left message -> H.failH ("stream change refused: " ++ message)
    Right bytes -> expectAccepted checked "stream change" bytes
  case filter ((== "image 4 stream") . U.diffPart) (U.diffParts parsed changedStream) of
    [d] -> expectTrue checked "image stream diff" (U.diffChangedBytes d == 0x20 && U.diffPolicy d == U.ImageContent)
    _ -> H.failH "image stream diff missing"
  -- The pinned stock checks distinguish the synthetic container from stock.
  let stockResults = U.stockChecks parsed
  expectCheck checked "stock version words" True stockResults
  expectCheck checked "stock restore coverage" True stockResults
  expectCheck checked "stock identity records" False stockResults
  expectCheck checked "stock main checksum" False stockResults
  expectCheck checked "stock segment entry 4" False stockResults
  expectCheck checked "stock image 4 header" False stockResults
  expectCheck checked "stock trailer" False stockResults
  -- Parse refusals.
  expectRejected checked "short container" "shorter" (B.take 100 fixtureBytes)
  expectRejected checked "main range past end" "main range" (B.take 0x300 fixtureBytes)
  expectRejected checked "auxiliary 2 past end" "auxiliary 2 range" (B.take (B.length fixtureBytes - 8) fixtureBytes)
  expectRejected checked "auxiliary 1 pair width" "multiple of 4" (patchWord 0x248 (U.word32At fixtureBytes 0x248 + 1) fixtureBytes)
  -- The id-2 extent the stored words describe must lie in the body, clear of
  -- the image-5 record and the trailer.
  let entry2Offset = U.stagingLength + U.segmentTableOffset + U.segmentEntryBytes * 2
      entry2Flash = entry2Offset + 0x14
      entry2Size = entry2Offset + 0x8
      patchMediaWordFile offset value bytes = B.take offset bytes <> U.putMediaWord value <> B.drop (offset + 4) bytes
      mainLength = B.length fixtureBytes - U.stagingLength - B.length fixtureAux1Bytes - B.length fixtureAux2Bytes
      trailerStart = mainLength - U.trailerBytes
  expectRejected
    checked
    "image 2 before record 5"
    "starts before the image-5 record"
    (patchMediaWordFile entry2Flash (fromIntegral record5Media) fixtureBytes)
  expectRejected
    checked
    "image 2 in the trailer"
    "overlaps the trailer"
    (patchMediaWordFile entry2Flash (fromIntegral (trailerStart - 0x10)) fixtureBytes)
  expectRejected checked "image 2 empty" "extent is empty" (patchMediaWordFile entry2Size 0 fixtureBytes)
  expectRejected checked "auxiliary 2 pair width" "multiple of 8" (patchWord 0x254 (U.word32At fixtureBytes 0x254 - 1) fixtureBytes)
  let terminatorOffset = U.stagingLength + 0x140160
  expectRejected
    checked
    "terminator missing"
    "terminator"
    (B.take terminatorOffset fixtureBytes <> B.replicate 4 0xff <> B.drop (terminatorOffset + 4) fixtureBytes)
  -- Body selector 0 version policy.
  expectTrue checked "sentinel version" (U.acceptsVersion (0xffffffff, 0xffffffff) (1, 0x50))
  expectTrue checked "equal version" (U.acceptsVersion (1, 0x50) (1, 0x50))
  expectTrue checked "higher major" (U.acceptsVersion (2, 0) (1, 0x50))
  expectTrue checked "higher minor" (U.acceptsVersion (1, 0x51) (1, 0x50))
  expectTrue checked "downgrade rejected" (not (U.acceptsVersion (1, 0x4f) (1, 0x50)))
  expectTrue checked "bad format rejected" (not (U.acceptsVersion (0x100, 0x50) (1, 0x50)))
  expectTrue checked "non-decimal nibble rejected" (not (U.acceptsVersion (1, 0x5a) (1, 0x50)))
  -- Part diff policy: identical clean, image content allowed, carried guarded.
  forM_ (U.diffParts parsed parsed) $ \d ->
    expectTrue checked ("clean diff " ++ U.diffPart d) (U.diffChangedBytes d == 0 && U.diffFirstOffset d == Nothing)
  let listChanged = U.Parsed (B.length fixtureBytes) (U.derivedHeader ((U.parsedParts parsed) {U.partAux1 = init (U.partAux1 (U.parsedParts parsed))})) ((U.parsedParts parsed) {U.partAux1 = init (U.partAux1 (U.parsedParts parsed))})
  case filter ((== "restore list 1") . U.diffPart) (U.diffParts parsed listChanged) of
    [d] -> expectTrue checked "restore diff" (U.diffChangedBytes d == 4 && U.diffPolicy d == U.CarriedMustMatch)
    _ -> H.failH "restore diff missing"
  -- The plain-extent carrier: the id-6 window as the segment words, the
  -- loader destination word and the linked extent descriptors govern it. The
  -- fixture carries the whole aligned window in the post-head span; the
  -- window-refusal fixture carries its id-2 extent inside the window.
  (plainStart, plainStop) <- case U.plainWindow parts of
    Left message -> H.failH ("plain window refused: " ++ message)
    Right value -> pure value
  expectTrue checked "plain window geometry" (plainStart == plainMedia && plainStop == plainMedia + 0x360000)
  expectTrue
    checked
    "plain extents"
    (U.plainExtents parts == [(0x01100000, 0x000ac5a0), (0x011c0000, 0x0001cca0), (0x011e0000, 0x0026a5e0)])
  stockPlain <- case U.extractPlain parts of
    Left message -> H.failH ("plain extraction refused: " ++ message)
    Right value -> pure value
  expectTrue checked "plain window bytes" (B.length (U.mediaImageBytes stockPlain) == 0x360000)
  -- extract -> replace composes to the identity on the fixture.
  case U.replacePlain parsed stockPlain of
    Left message -> H.failH ("plain identity replacement refused: " ++ message)
    Right replaced -> case U.rebuildContainer parsed replaced of
      Left message -> H.failH ("plain identity rebuild refused: " ++ message)
      Right rebuilt -> expectTrue checked "plain identity rebuild" (rebuilt == fixtureBytes)
  -- A bounded same-size change at a flash offset lands at that offset and
  -- nowhere else outside the main checksum word.
  let spanOffset = plainMedia + 0x1000
      spanFileStart = U.stagingLength + spanOffset
      spanMedia = U.MediaImage (B.singleton (B.index (U.mediaImageBytes stockPlain) 0x1000 `xor` 0xff))
  spanParts <- case U.replacePlainAt parsed spanOffset spanMedia of
    Left message -> H.failH ("plain span replacement refused: " ++ message)
    Right value -> pure value
  spanRebuilt <- case U.rebuildContainer parsed spanParts of
    Left message -> H.failH ("plain span rebuild refused: " ++ message)
    Right bytes -> pure bytes
  spanParsed <- expectAccepted checked "plain span rebuild" spanRebuilt
  changedWindow <- case U.extractPlain (U.parsedParts spanParsed) of
    Left message -> H.failH ("plain span window refused: " ++ message)
    Right value -> pure value
  let (windowChanged, windowFirst) = U.firstDiff (U.mediaImageBytes stockPlain) (U.mediaImageBytes changedWindow)
      differing = [i | i <- [0 .. B.length fixtureBytes - 1], B.index fixtureBytes i /= B.index spanRebuilt i]
      allowed i = (i >= U.mainChecksumOffset && i < U.mainChecksumOffset + 4) || i == spanFileStart
  expectTrue checked "plain span window change" (windowChanged == 1 && windowFirst == Just 0x1000)
  expectTrue checked "plain span stored byte" (B.take 1 (B.drop spanFileStart spanRebuilt) == B.map complement (U.mediaImageBytes spanMedia))
  expectTrue checked "plain span carried identity" (not (null differing) && all allowed differing)
  forM_ (U.checks spanParsed ++ U.carriedChecks spanParsed) $ \c ->
    expectTrue checked ("plain span check " ++ U.checkField c) (U.checkPassed c)
  -- Refusals: a wrong span or input is refused rather than carried.
  let expectPlainRefused label result fragment = case result of
        Left message -> do
          H.ensure (fragment `isInfixOf` message) (label ++ ": wrong refusal: " ++ message)
          modifyIORef' checked (+ 1)
        Right _ -> H.failH (label ++ ": replacement was accepted")
  expectPlainRefused
    "plain wrong length"
    (U.replacePlain parsed (U.MediaImage (B.cons 0x5a (U.mediaImageBytes stockPlain))))
    "against the stored flash length"
  expectPlainRefused "plain empty span" (U.replacePlainAt parsed spanOffset (U.MediaImage B.empty)) "the plain span is empty"
  expectPlainRefused
    "plain span before window"
    (U.replacePlainAt parsed (plainMedia - 1) (U.MediaImage (B.singleton 0x5a)))
    "starts before the plain window"
  expectPlainRefused
    "plain span past window"
    (U.replacePlainAt parsed (plainStop - 1) (U.MediaImage (B.replicate 2 0x5a)))
    "runs past the plain window"
  -- An offset near maxBound is refused rather than wrapping into the window.
  expectPlainRefused
    "plain span at maxBound"
    (U.replacePlainAt parsed maxBound (U.MediaImage (B.singleton 0x5a)))
    "starts after the plain window"
  expectPlainRefused
    "plain span near maxBound"
    (U.replacePlainAt parsed (maxBound - 1) (U.MediaImage (B.replicate 4 0x5a)))
    "starts after the plain window"
  -- The window must sit in the carried span: the window-refusal fixture's
  -- window runs past its image-2 extent.
  windowRefusalParsed <- expectAccepted checked "window refusal fixture" (fixtureBytesAt windowRefusalImage2Media)
  case U.extractPlain (U.parsedParts windowRefusalParsed) of
    Left message -> expectTrue checked "plain window past extent" ("runs past the image-2 extent" `isInfixOf` message)
    Right _ -> H.failH "a plain window past the carried span was accepted"
  -- The window must carry every extent run: a shortened (aligned) stored
  -- flash length leaves the pinned descriptors outside it.
  let shortWindowParts =
        parts {U.partSegments = [if U.segId e == 6 then e {U.segFlashLen = 0x20000} else e | e <- U.partSegments parts]}
  case U.extractPlain shortWindowParts of
    Left message -> expectTrue checked "plain extent past window" ("runs outside the plain window" `isInfixOf` message)
    Right _ -> H.failH "an extent run outside the plain window was accepted"
  -- A malformed descriptor is refused through its own word: the linked base
  -- at 0x1401b4 and the loader destination at 0x1401a8 are separate words.
  let badBaseParts = parts {U.partTableTail = patchMediaWord (0x1401b4 - tableEnd) 0x00f00000 (U.partTableTail parts)}
      badLoadParts = parts {U.partTableTail = patchMediaWord (0x1401a8 - tableEnd) 0x01180000 (U.partTableTail parts)}
  case U.extractPlain badBaseParts of
    Left message -> expectTrue checked "plain base outside window" ("runs outside the plain window" `isInfixOf` message)
    Right _ -> H.failH "a linked base outside the plain window was accepted"
  case U.extractPlain badLoadParts of
    Left message -> expectTrue checked "plain load above base" ("runs outside the plain window" `isInfixOf` message)
    Right _ -> H.failH "a loader destination above the first linked base was accepted"
  -- Flash geometry the loader would round to different byte offsets is
  -- refused.
  let unalignedFlash =
        parts {U.partSegments = [if U.segId e == 6 then e {U.segFlash = fromIntegral (plainMedia + 1)} else e | e <- U.partSegments parts]}
      unalignedLength =
        parts {U.partSegments = [if U.segId e == 6 then e {U.segFlashLen = 0x360001} else e | e <- U.partSegments parts]}
  case U.extractPlain unalignedFlash of
    Left message -> expectTrue checked "plain unaligned flash" ("not 0x20000-aligned" `isInfixOf` message)
    Right _ -> H.failH "an unaligned plain flash word was accepted"
  case U.extractPlain unalignedLength of
    Left message -> expectTrue checked "plain unaligned length" ("not 0x20000-aligned" `isInfixOf` message)
    Right _ -> H.failH "an unaligned plain flash length was accepted"
  total <- readIORef checked
  putStrLn (show total ++ " update-container contract checks passed")

patchWord :: Int -> Word32 -> B.ByteString -> B.ByteString
patchWord off w bytes = B.take off bytes <> U.putWord32 w <> B.drop (off + 4) bytes
