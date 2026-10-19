{-# LANGUAGE OverloadedStrings #-}
-- Pinned X-M1 1.50 update container: parts, per-field checks, gated rebuild and
-- diff, and the image-2 and image-5 record rebuilds the patch chain rides.
--
-- The DAT stores every byte of the main range complemented relative to the
-- media image the writer programs. Structure fields below are media words;
-- opaque spans, the identity field, the range header and the restore lists are
-- file bytes. reports/update-container-rebuild.md is the field contract.
--
-- rebuildContainer is the only emitter. It is relative to a source container,
-- takes the carried set (identity, selector, fill, restore lists) from that
-- source, and refuses to return bytes whose contract checks fail, so no caller
-- can write drifted restore lists or size words that disagree with the parts.
-- It also refuses when the parts read back from its own assembly differ from
-- the parts it was handed (assemblyChecks).
module FujiAF.UpdateContainer
  ( Pair16 (..)
  , Pair32 (..)
  , MediaImage (..)
  , TableLayout (..)
  , SegmentEntry (..)
  , RecordHeader (..)
  , CompressedRecord (..)
  , Trailer (..)
  , Parts (..)
  , Header (..)
  , Parsed (..)
  , Check (..)
  , PartPolicy (..)
  , PartDiff (..)
  , parseContainer
  , rebuildContainer
  , compactRecorderControl
  , tableLayoutFromMedia
  , tableEndOffset
  , plainExtentTableOffset
  , plainLoadOffset
  , header3Offset
  , windowHeaderOffset
  , image2Extent
  , replaceImage2
  , plainExtents
  , plainWindow
  , extractPlain
  , replacePlain
  , replacePlainAt
  , record5Window
  , replaceRecord5
  , serializeRecord
  , assemblyChecks
  , derivedHeader
  , checks
  , carriedChecks
  , stockChecks
  , diffParts
  , firstDiff
  , acceptsVersion
  , showHexWord
  , stagingLength
  , identityBytes
  , rangeFillBytes
  , mainChecksumOffset
  , segmentEntryBytes
  , segmentCount
  , segmentTableOffset
  , recordHeaderBytes
  , trailerBytes
  , mediaWordAt
  , mediaHalfAt
  , putMediaWord
  , putMediaHalf
  , word16At
  , word32At
  , putWord16
  , putWord32
  , mainChecksum
  , auxChecksum
  , stockAux1
  , stockAux2
  , stockPlainExtents
  , stockPlainLoad
  ) where

import Data.Bits (complement, shiftL, shiftR, (.&.), (.|.))
import qualified Data.ByteString as B
import Data.List (find, intercalate)
import Data.Word (Word16, Word32, Word8)

data Pair16 = Pair16 { pair16Offset :: Word16, pair16Length :: Word16 } deriving (Eq, Show)

data Pair32 = Pair32 { pair32Offset :: Word32, pair32Length :: Word32 } deriving (Eq, Show)

-- An image in media form: the bytes the raw flash read copies to RAM. Every
-- main-range byte is stored in the container complemented, and the parts carry
-- file bytes, so replaceImage2 complements what it is handed; a caller cannot
-- pass a container-form image where a media image belongs.
newtype MediaImage = MediaImage {mediaImageBytes :: B.ByteString} deriving (Eq, Show)

data SegmentEntry = SegmentEntry
  { segId :: Word32
  , segLoad :: Word32
  , segSize :: Word32
  , segBss :: Word32
  , segBssLen :: Word32
  , segFlash :: Word32
  , segFlashLen :: Word32
  , segWord7 :: Word32
  , segFlag :: Word32
  } deriving (Eq, Show)

-- Word 1 is the record length the loader's feed arithmetic reads (total_len in
-- the record grammar): 0x14 plus the stream length. A short value feeds the
-- decompressor fewer bytes than the stream holds and lands in the engine's
-- hang-shaped failure, so every gate compares this word against the bytes
-- rather than re-deriving it (reports/update-container-rebuild.md section 6).
data RecordHeader = RecordHeader
  { recDecodedSize :: Word32
  , recTotalLen :: Word32
  , recChunkCount :: Word32
  , recChunkSize :: Word32
  , recMode :: Word32
  } deriving (Eq, Show)

-- A record part carries the decoded image only when an image replacement
-- supplies it; then the decoded-size word is checked against its length.
data CompressedRecord = CompressedRecord
  { recHeader :: RecordHeader
  , recStream :: B.ByteString
  , recDecoded :: Maybe B.ByteString
  } deriving (Eq, Show)

data Trailer = Trailer
  { trSpan :: Word32
  , trSpan2 :: Word32
  , trSize :: Word32
  , trReserved3 :: Word32
  , trReserved4 :: Word32
  , trImageLoad :: Word32
  , trImageSize :: Word32
  , trFillHalf :: Word16
  , trMarkerHalf :: Word16
  } deriving (Eq, Show)

data TableLayout = Stock | RecorderCompact deriving (Eq, Show)

data Parts = Parts
  { partIdentity :: B.ByteString
  , partVersionMajor :: Word32
  , partVersionMinor :: Word32
  , partSelector :: Word32
  , partRangeFill :: B.ByteString
  , partPreTable :: B.ByteString
  , partTableLayout :: TableLayout
  , partSegments :: [SegmentEntry]
  , partTableTail :: B.ByteString
  , partRecord4 :: CompressedRecord
  , partGap45 :: B.ByteString
  , partRecord5 :: CompressedRecord
  , partPostHead :: B.ByteString
  , partImage2 :: B.ByteString
  , partPostTail :: B.ByteString
  , partTrailer :: Trailer
  , partAux1 :: [Pair16]
  , partAux2 :: [Pair32]
  } deriving (Eq, Show)

data Header = Header
  { hType :: Word32
  , hMainChecksum :: Word32
  , hMainOffset :: Word32
  , hMainLength :: Word32
  , hAux1Offset :: Word32
  , hAux1Length :: Word32
  , hAux1Checksum :: Word32
  , hAux2Offset :: Word32
  , hAux2Length :: Word32
  , hAux2Checksum :: Word32
  } deriving (Eq, Show)

data Parsed = Parsed
  { parsedFileLength :: Int
  , parsedHeader :: Header
  , parsedParts :: Parts
  } deriving (Eq, Show)

data Check = Check
  { checkField :: String
  , checkStored :: String
  , checkExpected :: String
  , checkPassed :: Bool
  , checkSource :: String
  } deriving (Eq, Show)

data PartPolicy
  = CarriedMustMatch
  | CarriedPolicyChecked
  | ImageContent
  deriving (Eq, Show)

data PartDiff = PartDiff
  { diffPart :: String
  , diffChangedBytes :: Int
  , diffFirstOffset :: Maybe Int
  , diffPolicy :: PartPolicy
  } deriving (Eq, Show)

stagingLength :: Int
stagingLength = 0x25c

identityBytes :: Int
identityBytes = 512

rangeFillBytes :: Int
rangeFillBytes = 0x28

segmentEntryBytes :: Int
segmentEntryBytes = 36

segmentCount :: Int
segmentCount = 8

segmentTableOffset :: TableLayout -> Int
segmentTableOffset Stock = 0x140040
segmentTableOffset RecorderCompact = 0x140010

recordHeaderBytes :: Int
recordHeaderBytes = 20

trailerBytes :: Int
trailerBytes = 32

tableEndOffset :: TableLayout -> Int
tableEndOffset layout = segmentTableOffset layout + segmentEntryBytes * segmentCount + 4

plainExtentTableOffset :: TableLayout -> Int
plainExtentTableOffset Stock = 0x1401b0
plainExtentTableOffset RecorderCompact = 0x140160

plainLoadOffset :: TableLayout -> Int
plainLoadOffset Stock = 0x1401a8
plainLoadOffset RecorderCompact = 0x140158

header3Offset :: TableLayout -> Int
header3Offset Stock = 0x140184
header3Offset RecorderCompact = 0x140134

windowHeaderOffset :: TableLayout -> Int
windowHeaderOffset layout = plainLoadOffset layout - 4

aliasPointer :: Int -> Word32
aliasPointer offset = 0x4061c000 + fromIntegral (offset - 0x140000)

tableLayoutFromMedia :: B.ByteString -> Either String TableLayout
tableLayoutFromMedia body = do
  if B.length body < 0x140010 then Left "main range lacks the root header" else pure ()
  layout <- case mediaWordAt body 0x140000 of
    0x4061c040 -> Right Stock
    0x4061c010 -> Right RecorderCompact
    _ -> Left "unsupported segment-table root pointer"
  if B.length body < tableEndOffset layout + trailerBytes
    then Left "main range shorter than the segment table and trailer"
    else Right layout

validateMetadata :: TableLayout -> B.ByteString -> Either String ()
validateMetadata layout body = do
  let extents = plainExtentTableOffset layout
      header = windowHeaderOffset layout
      requiredEnd = extents + 76
  if B.length body < requiredEnd then Left "recognized metadata extends past the main range" else pure ()
  if mediaWordAt body 0x140004 /= aliasPointer header
      || mediaWordAt body 0x140008 /= 0
      || mediaWordAt body 0x14000c /= aliasPointer (header3Offset layout)
      || mediaWordAt body (header3Offset layout) /= 0x1234
      || mediaWordAt body header /= aliasPointer extents
      || mediaWordAt body (header + 8) /= 0x360000
    then Left "unsupported root or recognized-header words"
    else pure ()
  let row k = [mediaWordAt body (extents + k * 24 + w * 4) | w <- [0 .. 5]]
      valid k ident resource = case row k of
        [i, _, _, a, b, r] -> i == ident && a == 0 && b == 0 && r == resource
        _ -> False
  if not (valid 0 0 0x44 && valid 1 2 0x46 && valid 2 3 0x47)
    then Left "unsupported recognized extent records"
    else pure ()
  case mediaWordAt body (extents + 72) of
    0xffffffff -> Right ()
    1 | layout == RecorderCompact && B.length body >= extents + 100
        && row 3 == [1,0x0144a5e0,0x1000,0,0,0x45]
        && row 2 == [3,0x011e0000,0x26a5e0,0,0,0x47]
        && mediaWordAt body (extents + 96) == 0xffffffff -> Right ()
    _ -> Left "recognized extent terminator missing or unsupported"

identityOffset :: Int
identityOffset = 4

versionMajorOffset :: Int
versionMajorOffset = 0x204

mainChecksumOffset :: Int
mainChecksumOffset = 0x20c

selectorOffset :: Int
selectorOffset = 0x210

rangeFillOffset :: Int
rangeFillOffset = 0x214

mainOffsetOffset :: Int
mainOffsetOffset = 0x23c

aux1OffsetOffset :: Int
aux1OffsetOffset = 0x244

aux2OffsetOffset :: Int
aux2OffsetOffset = 0x250

word32At :: B.ByteString -> Int -> Word32
word32At b i =
  fromIntegral (B.index b i)
    .|. (fromIntegral (B.index b (i + 1)) `shiftL` 8)
    .|. (fromIntegral (B.index b (i + 2)) `shiftL` 16)
    .|. (fromIntegral (B.index b (i + 3)) `shiftL` 24)

word16At :: B.ByteString -> Int -> Word16
word16At b i = fromIntegral (B.index b i) .|. (fromIntegral (B.index b (i + 1)) `shiftL` 8)

putWord32 :: Word32 -> B.ByteString
putWord32 w =
  B.pack
    [ fromIntegral (w .&. 0xff)
    , fromIntegral ((w `shiftR` 8) .&. 0xff)
    , fromIntegral ((w `shiftR` 16) .&. 0xff)
    , fromIntegral ((w `shiftR` 24) .&. 0xff)
    ]

putWord16 :: Word16 -> B.ByteString
putWord16 w = B.pack [fromIntegral (w .&. 0xff), fromIntegral ((w `shiftR` 8) .&. 0xff)]

mediaWordAt :: B.ByteString -> Int -> Word32
mediaWordAt b i = complement (word32At b i)

mediaHalfAt :: B.ByteString -> Int -> Word16
mediaHalfAt b i = complement (word16At b i)

putMediaWord :: Word32 -> B.ByteString
putMediaWord w = putWord32 (complement w)

putMediaHalf :: Word16 -> B.ByteString
putMediaHalf w = putWord16 (complement w)

-- Camera arithmetic: complement of the unsigned byte sum, main over
-- complemented bytes (0xdb90ac), auxiliary over file bytes (0xdb89a4).
mainChecksum :: B.ByteString -> Word32
mainChecksum body = complement (B.foldl' (\acc b -> acc + fromIntegral (complement b)) 0 body)

auxChecksum :: B.ByteString -> Word32
auxChecksum bytes = complement (B.foldl' (\acc b -> acc + fromIntegral b) 0 bytes)

serializeSegment :: SegmentEntry -> B.ByteString
serializeSegment e =
  B.concat
    [ putMediaWord (segId e)
    , putMediaWord (segLoad e)
    , putMediaWord (segSize e)
    , putMediaWord (segBss e)
    , putMediaWord (segBssLen e)
    , putMediaWord (segFlash e)
    , putMediaWord (segFlashLen e)
    , putMediaWord (segWord7 e)
    , putMediaWord (segFlag e)
    ]

parseSegment :: B.ByteString -> Int -> SegmentEntry
parseSegment body i =
  SegmentEntry
    { segId = mediaWordAt body i
    , segLoad = mediaWordAt body (i + 4)
    , segSize = mediaWordAt body (i + 8)
    , segBss = mediaWordAt body (i + 12)
    , segBssLen = mediaWordAt body (i + 16)
    , segFlash = mediaWordAt body (i + 20)
    , segFlashLen = mediaWordAt body (i + 24)
    , segWord7 = mediaWordAt body (i + 28)
    , segFlag = mediaWordAt body (i + 32)
    }

serializeRecordHeader :: RecordHeader -> B.ByteString
serializeRecordHeader h =
  B.concat
    [ putMediaWord (recDecodedSize h)
    , putMediaWord (recTotalLen h)
    , putMediaWord (recChunkCount h)
    , putMediaWord (recChunkSize h)
    , putMediaWord (recMode h)
    ]

serializeRecord :: CompressedRecord -> B.ByteString
serializeRecord r = serializeRecordHeader (recHeader r) <> recStream r

parseRecord :: TableLayout -> String -> B.ByteString -> Int -> Either String CompressedRecord
parseRecord layout label body start = do
  let bodyLength = B.length body
  if start < tableEndOffset layout
    then Left (label ++ " starts inside the segment table")
    else pure ()
  if start + recordHeaderBytes > bodyLength
    then Left (label ++ " header extends past the main range")
    else pure ()
  let header =
        RecordHeader
          { recDecodedSize = mediaWordAt body start
          , recTotalLen = mediaWordAt body (start + 4)
          , recChunkCount = mediaWordAt body (start + 8)
          , recChunkSize = mediaWordAt body (start + 12)
          , recMode = mediaWordAt body (start + 16)
          }
      totalLen = fromIntegral (recTotalLen header)
      streamStart = start + recordHeaderBytes
  if totalLen < recordHeaderBytes
    then Left (label ++ " record length is shorter than its header")
    else pure ()
  if streamStart + (totalLen - recordHeaderBytes) > bodyLength
    then Left (label ++ " stream extends past the main range")
    else pure ()
  pure
    CompressedRecord
      { recHeader = header
      , recStream = B.take (totalLen - recordHeaderBytes) (B.drop streamStart body)
      , recDecoded = Nothing
      }

serializeTrailer :: Trailer -> B.ByteString
serializeTrailer t =
  B.concat
    [ putMediaWord (trSpan t)
    , putMediaWord (trSpan2 t)
    , putMediaWord (trSize t)
    , putMediaWord (trReserved3 t)
    , putMediaWord (trReserved4 t)
    , putMediaWord (trImageLoad t)
    , putMediaWord (trImageSize t)
    , putMediaHalf (trFillHalf t)
    , putMediaHalf (trMarkerHalf t)
    ]

parseTrailer :: B.ByteString -> Int -> Trailer
parseTrailer body i =
  Trailer
    { trSpan = mediaWordAt body i
    , trSpan2 = mediaWordAt body (i + 4)
    , trSize = mediaWordAt body (i + 8)
    , trReserved3 = mediaWordAt body (i + 12)
    , trReserved4 = mediaWordAt body (i + 16)
    , trImageLoad = mediaWordAt body (i + 20)
    , trImageSize = mediaWordAt body (i + 24)
    , trFillHalf = mediaHalfAt body (i + 28)
    , trMarkerHalf = mediaHalfAt body (i + 30)
    }

serializeAux1 :: [Pair16] -> B.ByteString
serializeAux1 pairs = B.concat [putWord16 (pair16Offset p) <> putWord16 (pair16Length p) | p <- pairs]

parseAux1 :: B.ByteString -> [Pair16]
parseAux1 bytes = [Pair16 (word16At bytes i) (word16At bytes (i + 2)) | i <- [0, 4 .. B.length bytes - 4]]

serializeAux2 :: [Pair32] -> B.ByteString
serializeAux2 pairs = B.concat [putWord32 (pair32Offset p) <> putWord32 (pair32Length p) | p <- pairs]

parseAux2 :: B.ByteString -> [Pair32]
parseAux2 bytes = [Pair32 (word32At bytes i) (word32At bytes (i + 4)) | i <- [0, 8 .. B.length bytes - 8]]

parseHeader :: B.ByteString -> Header
parseHeader bytes =
  Header
    { hType = word32At bytes 0
    , hMainChecksum = word32At bytes mainChecksumOffset
    , hMainOffset = word32At bytes mainOffsetOffset
    , hMainLength = word32At bytes (mainOffsetOffset + 4)
    , hAux1Offset = word32At bytes aux1OffsetOffset
    , hAux1Length = word32At bytes (aux1OffsetOffset + 4)
    , hAux1Checksum = word32At bytes (aux1OffsetOffset + 8)
    , hAux2Offset = word32At bytes aux2OffsetOffset
    , hAux2Length = word32At bytes (aux2OffsetOffset + 4)
    , hAux2Checksum = word32At bytes (aux2OffsetOffset + 8)
    }

-- Parses the main range into its named parts. The segment list is exactly the
-- ids 0..7 the checks and diff walk; parse rejects anything else. The image-2
-- extent is the id-2 entry's raw copy: the loader reads `size` bytes from
-- flash `flash` straight to `load` (no record header, unlike ids 4 and 5), so
-- the container's part is media [flash, flash + size) between the image-5
-- record and the trailer.
parseMain :: B.ByteString -> Either String (TableLayout, B.ByteString, [SegmentEntry], B.ByteString, CompressedRecord, B.ByteString, CompressedRecord, B.ByteString, B.ByteString, B.ByteString, Trailer)
parseMain body = do
  layout <- tableLayoutFromMedia body
  validateMetadata layout body
  let bodyLength = B.length body
      tableStart = segmentTableOffset layout
      tableEnd = tableEndOffset layout
  if mediaWordAt body (tableStart + segmentEntryBytes * segmentCount) /= 0xffffffff
    then Left "segment table terminator missing"
    else pure ()
  let segments = [parseSegment body (tableStart + segmentEntryBytes * k) | k <- [0 .. segmentCount - 1]]
  if map segId segments /= [0 .. 7]
    then Left "segment table does not hold ids 0 through 7"
    else pure ()
  let entry2 = entryById 2 segments
      entry4 = entryById 4 segments
      entry5 = entryById 5 segments
      record4Start = fromIntegral (segFlash entry4)
      record5Start = fromIntegral (segFlash entry5)
      metadataEnd = plainExtentTableOffset layout
        + if mediaWordAt body (plainExtentTableOffset layout + 72) == 1 then 100 else 76
  if record4Start < metadataEnd
    then Left "image 4 record starts inside recognized metadata"
    else pure ()
  record4 <- parseRecord layout "image 4 record" body record4Start
  record5 <- parseRecord layout "image 5 record" body record5Start
  let record4End = record4Start + B.length (serializeRecord record4)
      record5End = record5Start + B.length (serializeRecord record5)
      trailerStart = bodyLength - trailerBytes
      image2Start = fromIntegral (segFlash entry2)
      image2Length = fromIntegral (segSize entry2)
      image2End = image2Start + image2Length
  if record4End > record5Start
    then Left "image 4 record overlaps image 5"
    else pure ()
  if record5End > trailerStart
    then Left "image 5 record overlaps the trailer"
    else pure ()
  if image2Length == 0
    then Left "image 2 extent is empty"
    else pure ()
  if image2Start < record5End
    then Left "image 2 extent starts before the image-5 record ends"
    else pure ()
  if image2End > trailerStart
    then Left "image 2 extent overlaps the trailer"
    else pure ()
  pure
    ( layout
    , B.take tableStart body
    , segments
    , B.take (record4Start - tableEnd) (B.drop tableEnd body)
    , record4
    , B.take (record5Start - record4End) (B.drop record4End body)
    , record5
    , B.take (image2Start - record5End) (B.drop record5End body)
    , B.take image2Length (B.drop image2Start body)
    , B.take (trailerStart - image2End) (B.drop image2End body)
    , parseTrailer body trailerStart
    )

entryById :: Word32 -> [SegmentEntry] -> SegmentEntry
entryById wanted entries = case filter ((== wanted) . segId) entries of
  (entry : _) -> entry
  [] -> error "segment id missing from a parsed container"

parseContainer :: B.ByteString -> Either String Parsed
parseContainer bytes = do
  let fileLength = B.length bytes
  if fileLength < stagingLength
    then Left "container shorter than the 0x25c staging header"
    else pure ()
  let header = parseHeader bytes
      mainStart = stagingLength + fromIntegral (hMainOffset header)
      mainLength = fromIntegral (hMainLength header)
      aux1Start = stagingLength + fromIntegral (hAux1Offset header)
      aux1Length = fromIntegral (hAux1Length header)
      aux2Start = stagingLength + fromIntegral (hAux2Offset header)
      aux2Length = fromIntegral (hAux2Length header)
  if mainStart + mainLength > fileLength
    then Left "main range extends past the file"
    else pure ()
  if aux1Start + aux1Length > fileLength
    then Left "auxiliary 1 range extends past the file"
    else pure ()
  if aux2Start + aux2Length > fileLength
    then Left "auxiliary 2 range extends past the file"
    else pure ()
  if hAux1Length header `mod` 4 /= 0
    then Left "auxiliary 1 length is not a multiple of 4"
    else pure ()
  if hAux2Length header `mod` 8 /= 0
    then Left "auxiliary 2 length is not a multiple of 8"
    else pure ()
  (layout, preTable, segments, tableTail, record4, gap45, record5, postHead, image2, postTail, trailer) <-
    parseMain (B.take mainLength (B.drop mainStart bytes))
  pure
    Parsed
      { parsedFileLength = fileLength
      , parsedHeader = header
      , parsedParts =
          Parts
            { partIdentity = B.take identityBytes (B.drop identityOffset bytes)
            , partVersionMajor = word32At bytes versionMajorOffset
            , partVersionMinor = word32At bytes (versionMajorOffset + 4)
            , partSelector = word32At bytes selectorOffset
            , partRangeFill = B.take rangeFillBytes (B.drop rangeFillOffset bytes)
            , partPreTable = preTable
            , partTableLayout = layout
            , partSegments = segments
            , partTableTail = tableTail
            , partRecord4 = record4
            , partGap45 = gap45
            , partRecord5 = record5
            , partPostHead = postHead
            , partImage2 = image2
            , partPostTail = postTail
            , partTrailer = trailer
            , partAux1 = parseAux1 (B.take aux1Length (B.drop aux1Start bytes))
            , partAux2 = parseAux2 (B.take aux2Length (B.drop aux2Start bytes))
            }
      }

serializeMain :: Parts -> B.ByteString
serializeMain parts =
  B.concat
    [ partPreTable parts
    , B.concat (map serializeSegment (partSegments parts))
    , putMediaWord 0xffffffff
    , partTableTail parts
    , serializeRecord (partRecord4 parts)
    , partGap45 parts
    , serializeRecord (partRecord5 parts)
    , partPostHead parts
    , partImage2 parts
    , partPostTail parts
    , serializeTrailer (partTrailer parts)
    ]

compactRecorderControl :: Parsed -> Either String Parts
compactRecorderControl source = do
  let parts = parsedParts source
      body = partPreTable parts <> B.concat (map serializeSegment (partSegments parts))
        <> putMediaWord 0xffffffff <> partTableTail parts
      entry4 = entryById 4 (partSegments parts)
      failures = [checkField c | c <- checks source ++ carriedChecks source, not (checkPassed c)]
  if not (null failures) then Left ("compact control source refused: " ++ intercalate ", " failures) else pure ()
  if partTableLayout parts /= Stock then Left "compact control requires the stock table layout" else pure ()
  validateMetadata Stock body
  if fromIntegral (segFlash entry4) < (0x140400 :: Int)
      || segLoad entry4 < 0x61c1fc || segLoad entry4 > 0x61c400
      || segBss entry4 /= 0x89ec00 || segBssLen entry4 /= 0x352e00
      || segLoad (entryById 5 (partSegments parts)) /= 0xbf1a00
      || mediaWordAt body 0x1401f8 /= 0xffffffff
    then Left "compact control geometry is unresolved"
    else pure ()
  let root = B.concat (map putMediaWord [0x4061c010,0x4061c154,0,0x4061c134])
      window = B.concat (map putMediaWord [0x4061c160,0x01100000,0x360000])
      tailBytes = partTableTail parts
      oldEnd = tableEndOffset Stock
      relocatedTail = B.take 32 (B.drop (header3Offset Stock - oldEnd) tailBytes) <> window
        <> B.take 76 (B.drop (plainExtentTableOffset Stock - oldEnd) tailBytes)
        <> B.drop (0x1401ac - oldEnd) tailBytes
  pure parts
    { partTableLayout = RecorderCompact
    , partPreTable = B.take 0x140000 (partPreTable parts) <> root
    , partTableTail = relocatedTail
    }

-- The id-2 extent as the container's own segment words describe it: media
-- offset in the main range, byte length and RAM load.
image2Extent :: Parts -> (Int, Int, Word32)
image2Extent parts =
  let entry = entryById 2 (partSegments parts)
   in (fromIntegral (segFlash entry), fromIntegral (segSize entry), segLoad entry)

-- The image-2 rebuild: substitute the id-2 extent with `media` (the loader's
-- raw copy; the container stores its complement) and return the parts. The
-- replacement must be the length the container's own segment words describe;
-- emit through rebuildContainer, which reads those words back from the
-- assembly and compares them against this part.
replaceImage2 :: Parsed -> MediaImage -> Either String Parts
replaceImage2 source media =
  let parts = parsedParts source
      (_, storedLength, _) = image2Extent parts
      bytes = mediaImageBytes media
   in if B.length bytes /= storedLength
        then
          Left
            ( "image-2 extent is "
                ++ show (B.length bytes)
                ++ " bytes against the stored size word "
                ++ show storedLength
            )
        else Right parts {partImage2 = B.map complement bytes}

-- The carried post-head span's media start: the image-5 record's end. The
-- plain window and the fill around it live in this span (parseMain).
postHeadStart :: Parts -> Int
postHeadStart parts =
  let entry = entryById 5 (partSegments parts)
   in fromIntegral (segFlash entry) + recordHeaderBytes + B.length (recStream (partRecord5 parts))

plainExtents :: Parts -> [(Word32, Word32)]
plainExtents parts =
  [ (mediaWordAt tableTail (offset + 4), mediaWordAt tableTail (offset + 8))
  | k <- [0 .. count - 1]
  , let offset = first + k * 24
  ]
  where
    tableTail = partTableTail parts
    layout = partTableLayout parts
    first = plainExtentTableOffset layout - tableEndOffset layout
    count = if mediaWordAt tableTail (first + 72) == 1 then 4 else 3

-- The raw loader copies whole 0x20000-byte blocks to the destination in the
-- recognized window header (id4 0x6a987c-0x6a98b0).
-- The window must fit the carried post-head span and every recognized extent.
plainWindow :: Parts -> Either String (Int, Int)
plainWindow parts = do
  let entry = entryById 6 (partSegments parts)
      entry5 = entryById 5 (partSegments parts)
      start = toInteger (segFlash entry)
      windowLength = toInteger (segFlashLen entry)
      stop = start + windowLength
      postStart = toInteger (segFlash entry5) + toInteger recordHeaderBytes + toInteger (B.length (recStream (partRecord5 parts)))
      postStop = postStart + toInteger (B.length (partPostHead parts))
      extents = plainExtents parts
      layout = partTableLayout parts
      loadBase = mediaWordAt (partTableTail parts) (plainLoadOffset layout - tableEndOffset layout)
      nominalLength = mediaWordAt (partTableTail parts) (windowHeaderOffset layout + 8 - tableEndOffset layout)
      outside (base, extentLength) =
        let windowOffset = toInteger base - toInteger loadBase
         in windowOffset < 0 || windowOffset + toInteger extentLength > windowLength
  if start `mod` 0x20000 /= 0 || windowLength `mod` 0x20000 /= 0
    then Left ("the plain window is not 0x20000-aligned: flash " ++ showHexWord (segFlash entry) ++ " length " ++ showHexWord (segFlashLen entry))
    else pure ()
  if start < postStart
    then Left ("the plain window starts before the image-5 record ends: " ++ showHexWord (segFlash entry) ++ " against " ++ show postStart)
    else pure ()
  if stop > postStop
    then Left ("the plain window runs past the image-2 extent: " ++ show stop ++ " against " ++ show postStop)
    else pure ()
  if stop > 0x100000000 || stop > toInteger (maxBound :: Int)
    then Left ("the plain window end exceeds the flash or host address space: " ++ show stop)
    else pure ()
  case find outside extents of
    Just (base, _) ->
      Left ("plain extent " ++ showHexWord base ++ " runs outside the plain window of " ++ show windowLength ++ " bytes")
    Nothing -> pure ()
  if windowLength /= toInteger nominalLength
    then Left ("the plain window length disagrees with the recognized window header: " ++ showHexWord (segFlashLen entry) ++ " against " ++ showHexWord nominalLength)
    else pure ()
  pure (fromInteger start, fromInteger stop)

-- The loaded plain window in media form: the bytes the loader copies to RAM,
-- the linked extents and the runs between them. The container stores their
-- complement, like every main-range byte.
extractPlain :: Parts -> Either String MediaImage
extractPlain parts = do
  (start, stop) <- plainWindow parts
  let base = postHeadStart parts
  pure (MediaImage (B.map complement (B.take (stop - start) (B.drop (start - base) (partPostHead parts)))))

-- The plain-image rebuild: substitute the whole window in place and return
-- the parts. The replacement must be the length the stored flash word
-- describes; emit through rebuildContainer, which preserves the size and end
-- geometry (no descriptor, segment or trailer word moves).
replacePlain :: Parsed -> MediaImage -> Either String Parts
replacePlain source media = do
  let parts = parsedParts source
      bytes = mediaImageBytes media
  (start, stop) <- plainWindow parts
  if B.length bytes /= stop - start
    then
      Left
        ( "plain window is "
            ++ show (B.length bytes)
            ++ " bytes against the stored flash length "
            ++ show (stop - start)
        )
    else Right (writePlain parts start stop (B.map complement bytes))

-- A bounded in-place write at a flash (media) offset inside the plain window.
-- Every window byte the span does not cover keeps its source byte, so gap
-- writes between the linked extents need no descriptor change and cause none.
-- The offset is checked against the window before any addition and the upper
-- bound by subtraction, so an offset near maxBound is refused instead of
-- wrapping into the window.
replacePlainAt :: Parsed -> Int -> MediaImage -> Either String Parts
replacePlainAt source flashOffset media = do
  let parts = parsedParts source
      bytes = mediaImageBytes media
      spanLength = B.length bytes
  (start, stop) <- plainWindow parts
  if spanLength == 0
    then Left "the plain span is empty"
    else
      if flashOffset < start
        then
          Left
            ( "the plain span starts before the plain window: "
                ++ showHexWord (fromIntegral flashOffset)
                ++ " against "
                ++ showHexWord (fromIntegral start)
            )
        else
          if flashOffset > stop
            then
              Left
                ( "the plain span starts after the plain window: "
                    ++ showHexWord (fromIntegral flashOffset)
                    ++ " against "
                    ++ showHexWord (fromIntegral stop)
                )
            else
              if spanLength > stop - flashOffset
                then
                  Left
                    ( "the plain span runs past the plain window: "
                        ++ show spanLength
                        ++ " bytes at "
                        ++ showHexWord (fromIntegral flashOffset)
                        ++ " against "
                        ++ showHexWord (fromIntegral stop)
                    )
                else Right (writePlain parts flashOffset (flashOffset + spanLength) (B.map complement bytes))

-- Replace the post-head bytes at media [start, stop) with `fileBytes`.
writePlain :: Parts -> Int -> Int -> B.ByteString -> Parts
writePlain parts start stop fileBytes =
  let base = postHeadStart parts
      postHead = partPostHead parts
      at = start - base
   in parts {partPostHead = B.take at postHead <> fileBytes <> B.drop (at + (stop - start)) postHead}

-- The image-5 record window in media offsets: the record's own flash word and
-- its declared flash length. Every offset inside it holds the record or the
-- fill that follows it, and the plain image's flash begins at its end in the
-- stock container. replaceRecord5 refuses a container whose window runs past
-- the first segment region above it or past the image-2 extent.
record5Window :: Parts -> (Int, Int)
record5Window parts =
  let entry = entryById 5 (partSegments parts)
      start = fromIntegral (segFlash entry)
   in (start, start + fromIntegral (segFlashLen entry))

-- The image-5 rebuild: substitute the record (the encoder's file; the media
-- image stores its complement) and return the parts. The window rule is the
-- record-4 one: every media offset inside the image-5 flash window beyond the
-- new stream keeps the stock byte, so the bytes after the window (the plain
-- image, the id-2 extent and the trailer) stay at the offsets the segment
-- table describes. The decoded size must not move; the id-6 plain-image copy
-- carries the same image. A declared window may not end past the first segment
-- region above the record (stock: the plain image's base at 0x5a0000, 0x700000
-- below the image-2 extent), nor past the image-2 extent.
replaceRecord5 :: Parsed -> CompressedRecord -> Either String Parts
replaceRecord5 source replacement =
  let parts = parsedParts source
      stock = partRecord5 parts
      stockHeader = recHeader stock
      stockStream = recStream stock
      newHeader = recHeader replacement
      newStream = recStream replacement
      (windowStart, windowEnd) = record5Window parts
      (image2Start, _, _) = image2Extent parts
      -- The first segment region the window can run into: the lowest stored
      -- flash base past the window start among the other entries. On the stock
      -- container that is entry 6, the plain image's base at 0x5a0000. The
      -- scan is never empty on a parsed container, entry 2's base lying past
      -- the record (parseMain).
      (regionBase, regionId) =
        minimum
          [ (fromIntegral (segFlash entry), segId entry)
          | entry <- partSegments parts
          , segId entry /= 5
          , fromIntegral (segFlash entry) > windowStart
          ]
      stockEnd = windowStart + recordHeaderBytes + B.length stockStream
      windowBody = stockStream <> B.take (windowEnd - stockEnd) (partPostHead parts)
      beyond = B.drop (windowEnd - stockEnd) (partPostHead parts)
   in if recDecoded replacement == Nothing
        then Left "record-5 replacement requires the decoded image it must decode to"
        else
          if recDecodedSize newHeader /= recDecodedSize stockHeader
            then
              Left
                ( "record-5 replacement changes the decoded size: "
                    ++ showHexWord (recDecodedSize stockHeader)
                    ++ " -> "
                    ++ showHexWord (recDecodedSize newHeader)
                )
            else
              if windowEnd > image2Start
                then
                  Left
                    ( "the image-5 flash window ends past the image-2 extent: "
                        ++ showHexWord (fromIntegral windowEnd)
                        ++ " against "
                        ++ showHexWord (fromIntegral image2Start)
                    )
                else
                  if windowEnd > regionBase
                    then
                      Left
                        ( "the image-5 flash window ends past the next segment region: "
                            ++ showHexWord (fromIntegral windowEnd)
                            ++ " against "
                            ++ showHexWord (fromIntegral regionBase)
                            ++ " (entry "
                            ++ show regionId
                            ++ ")"
                        )
                    else
                      if windowEnd < stockEnd
                        then
                          Left
                            ( "the image-5 record overruns its declared flash window: record ends at "
                                ++ showHexWord (fromIntegral stockEnd)
                                ++ ", window ends at "
                                ++ showHexWord (fromIntegral windowEnd)
                            )
                        else
                          if B.length newStream > B.length windowBody
                            then
                              Left
                                ( "the record-5 stream does not fit the image-5 flash window: "
                                    ++ show (B.length newStream)
                                    ++ " bytes against "
                                    ++ show (B.length windowBody)
                                )
                            else
                              Right
                                parts
                                  { partRecord5 = replacement
                                  , partPostHead = B.drop (B.length newStream) windowBody <> beyond
                                  }

derivedHeader :: Parts -> Header
derivedHeader parts =
  let mainBody = serializeMain parts
      aux1 = serializeAux1 (partAux1 parts)
      aux2 = serializeAux2 (partAux2 parts)
      mainLength = fromIntegral (B.length mainBody)
      aux1Length = fromIntegral (B.length aux1)
   in Header
        { hType = 4
        , hMainChecksum = mainChecksum mainBody
        , hMainOffset = 0
        , hMainLength = mainLength
        , hAux1Offset = mainLength
        , hAux1Length = aux1Length
        , hAux1Checksum = auxChecksum aux1
        , hAux2Offset = mainLength + aux1Length
        , hAux2Length = fromIntegral (B.length aux2)
        , hAux2Checksum = auxChecksum aux2
        }

assembleBytes :: Parts -> B.ByteString
assembleBytes parts =
  let header = derivedHeader parts
   in B.concat
        [ putWord32 (hType header)
        , partIdentity parts
        , putWord32 (partVersionMajor parts)
        , putWord32 (partVersionMinor parts)
        , putWord32 (hMainChecksum header)
        , putWord32 (partSelector parts)
        , partRangeFill parts
        , putWord32 (hMainOffset header)
        , putWord32 (hMainLength header)
        , putWord32 (hAux1Offset header)
        , putWord32 (hAux1Length header)
        , putWord32 (hAux1Checksum header)
        , putWord32 (hAux2Offset header)
        , putWord32 (hAux2Length header)
        , putWord32 (hAux2Checksum header)
        , serializeMain parts
        , serializeAux1 (partAux1 parts)
        , serializeAux2 (partAux2 parts)
        ]

-- Body selector 0 policy: the 0xd0a590 format gate and the 0xd0a728 comparison.
acceptsVersion :: (Word32, Word32) -> (Word32, Word32) -> Bool
acceptsVersion (newMajor, newMinor) (currentMajor, currentMinor) =
  let formatOk =
        (newMajor == 0xffffffff && newMinor == 0xffffffff)
          || ( newMajor <= 0x99
                && newMinor <= 0x99
                && (newMajor .&. 0xf) <= 9
                && (newMinor .&. 0xf) <= 9
             )
      policyOk =
        (newMajor == 0xffffffff && newMinor == 0xffffffff)
          || newMajor > currentMajor
          || (newMajor == currentMajor && newMinor >= currentMinor)
   in formatOk && policyOk

-- The pinned current version of this body (E-STOCK-150-OWNER-REPORT).
stockCurrentVersion :: (Word32, Word32)
stockCurrentVersion = (1, 0x50)

-- The only emitter. The carried set comes from the source container (the
-- body's known-good values), and nothing is returned unless every contract
-- check passes on the bytes read back from the assembly: the checks take the
-- re-parsed container, not the derived header, so a wrong derivation cannot
-- vouch for itself. assemblyChecks compares the re-parsed parts against the
-- supplied ones, so a supplied part that the stored words do not describe
-- cannot be emitted. Parsing drops the decoded-image attachment, which is
-- carried over from the supplied parts for the decoded-size binding.
rebuildContainer :: Parsed -> Parts -> Either String B.ByteString
rebuildContainer source parts = do
  let sourceParts = parsedParts source
      same name left right = if left == right then Right () else Left ("carried part changed: " ++ name)
  _ <- same "identity records" (partIdentity sourceParts) (partIdentity parts)
  _ <- same "selector word" (partSelector sourceParts) (partSelector parts)
  _ <- same "range fill" (partRangeFill sourceParts) (partRangeFill parts)
  _ <- same "restore list 1" (partAux1 sourceParts) (partAux1 parts)
  _ <- same "restore list 2" (partAux2 sourceParts) (partAux2 parts)
  let bytes = assembleBytes parts
  reparsed <- parseContainer bytes
  let reParts = parsedParts reparsed
      attach old new = new {recDecoded = recDecoded old}
      merged =
        reParts
          { partRecord4 = attach (partRecord4 parts) (partRecord4 reParts)
          , partRecord5 = attach (partRecord5 parts) (partRecord5 reParts)
          }
      supplied = Parsed (B.length bytes) (parsedHeader reparsed) merged
  case [c | c <- checks supplied ++ carriedChecks supplied ++ assemblyChecks parts reParts, not (checkPassed c)] of
    [] -> Right bytes
    failures -> Left ("emission refused: " ++ intercalate ", " (map checkField failures))

hexByte :: Word8 -> String
hexByte b = [hexDigit (b `shiftR` 4), hexDigit (b .&. 0xf)]

hexDigit :: Word8 -> Char
hexDigit v = "0123456789abcdef" !! fromIntegral v

hex :: B.ByteString -> String
hex bytes = concatMap hexByte (B.unpack bytes)

showHexWord :: Word32 -> String
showHexWord w =
  "0x"
    ++ concatMap
      (\s -> if length s == 1 then '0' : s else s)
      (map (hexByte . fromIntegral) [(w `shiftR` 24) .&. 0xff, (w `shiftR` 16) .&. 0xff, (w `shiftR` 8) .&. 0xff, w .&. 0xff])

check :: String -> String -> String -> Bool -> String -> Check
check = Check

checks :: Parsed -> [Check]
checks parsed =
  let parts = parsedParts parsed
      stored = parsedHeader parsed
      derived = derivedHeader parts
      mainBody = serializeMain parts
      aux1 = serializeAux1 (partAux1 parts)
      aux2 = serializeAux2 (partAux2 parts)
      entry4 = entryById 4 (partSegments parts)
      entry5 = entryById 5 (partSegments parts)
      entry6 = entryById 6 (partSegments parts)
      entry2 = entryById 2 (partSegments parts)
      pair16Ok p = fromIntegral (pair16Offset p) + fromIntegral (pair16Length p) <= (0x10000 :: Int)
      pair32Ok p = fromIntegral (pair32Offset p) + fromIntegral (pair32Length p) <= (0x42000 :: Int)
      decodedChecks label r =
        [ check
            (label ++ " decoded size")
            ( case recDecoded r of
                Nothing -> showHexWord (recDecodedSize (recHeader r)) ++ " (no decoded image attached)"
                Just decoded -> showHexWord (recDecodedSize (recHeader r)) ++ "/" ++ showHexWord (fromIntegral (B.length decoded))
            )
            "decoded-size word equals the attached decoded image's length"
            ( case recDecoded r of
                Nothing -> True
                Just decoded -> recDecodedSize (recHeader r) == fromIntegral (B.length decoded)
            )
            "emission gate (image step must attach its decoded image)"
        ]
      recordChecks label r entry =
        let recordLength = toInteger recordHeaderBytes + toInteger (B.length (recStream r))
         in [ check
            (label ++ " header arithmetic")
            (showHexWord (recChunkCount (recHeader r)) ++ "/" ++ showHexWord (recChunkSize (recHeader r)) ++ "/" ++ showHexWord (recMode (recHeader r)))
            "ceil(record length/chunk), chunk 0x4000, mode 4"
            ( recChunkSize (recHeader r) == 0x4000
                && recMode (recHeader r) == 4
                && recChunkCount (recHeader r)
                  == (recTotalLen (recHeader r) + recChunkSize (recHeader r) - 1) `div` recChunkSize (recHeader r)
            )
            "decompressor feed"
        , check
            (label ++ " length")
            (showHexWord (recTotalLen (recHeader r)))
            (showHexWord (fromIntegral (recordHeaderBytes + B.length (recStream r))))
            (recTotalLen (recHeader r) == fromIntegral (recordHeaderBytes + B.length (recStream r)))
            "parser cut (word 1 fixes the stream length; assemblyChecks and the CLI enforce supplied parts)"
        , check
            (label ++ " flash window")
            (showHexWord (fromIntegral (recordHeaderBytes + B.length (recStream r))))
            ("<= " ++ showHexWord (segFlashLen entry))
            (recordLength <= toInteger (segFlashLen entry))
            "segment table layout"
        , check
            (label ++ " flash bounds")
            (showHexWord (segFlash entry) ++ "/" ++ showHexWord (segFlashLen entry))
            "actual record and declared window within the main body, next segment region and 32-bit flash address space"
            ( let start = toInteger (segFlash entry)
                  declaredEnd = start + toInteger (segFlashLen entry)
                  actualEnd = start + recordLength
                  trailerStart = toInteger (B.length mainBody) - toInteger trailerBytes
                  withinRegion region =
                    segId region == segId entry || segFlash region <= segFlash entry
                      || (declaredEnd <= toInteger (segFlash region) && actualEnd <= toInteger (segFlash region))
               in declaredEnd <= trailerStart && actualEnd <= trailerStart
                    && declaredEnd <= 0x100000000 && actualEnd <= 0x100000000
                    && all withinRegion (partSegments parts)
            )
            "segment table layout"
        ]
   in [ check "header type" (showHexWord (hType stored)) "0x4" (hType stored == 4) "parser 0xdb9cd4 (status 10)"
      , check
          "identity records"
          (hex (B.take 16 (partIdentity parts)))
          "512 bytes, first record not zero-leading"
          (B.length (partIdentity parts) == identityBytes && B.head (partIdentity parts) /= 0)
          "eligibility matcher 0xd0a2b4 (structure only; the matcher needs the body's runtime identity)"
      , check
          "version format"
          (showHexWord (partVersionMajor parts) ++ "/" ++ showHexWord (partVersionMinor parts))
          "both 0xffffffff or each <= 0x99 with decimal low nibble"
          ( (partVersionMajor parts == 0xffffffff && partVersionMinor parts == 0xffffffff)
              || ( partVersionMajor parts <= 0x99
                    && partVersionMinor parts <= 0x99
                    && (partVersionMajor parts .&. 0xf) <= 9
                    && (partVersionMinor parts .&. 0xf) <= 9
                 )
          )
          "0xd0a590"
      , check
          "version policy"
          (showHexWord (partVersionMajor parts) ++ "/" ++ showHexWord (partVersionMinor parts))
          "accepted against current 1.50 (body rule)"
          (acceptsVersion (partVersionMajor parts, partVersionMinor parts) stockCurrentVersion)
          "0xd0a590 + 0xd0a728"
      , check "selector" (showHexWord (partSelector parts)) "0x1 (selection 0 span)" (partSelector parts == 1) "0xec88c8"
      , check "range fill" (hex (partRangeFill parts)) "40 bytes of 0xff" (partRangeFill parts == B.replicate rangeFillBytes 0xff) "stock bytes"
      , check "main offset" (showHexWord (hMainOffset stored)) (showHexWord (hMainOffset derived)) (hMainOffset stored == hMainOffset derived) "0xdb99c8 seek"
      , check
          "main length"
          (showHexWord (hMainLength stored))
          (showHexWord (fromIntegral (B.length mainBody)))
          (hMainLength stored == fromIntegral (B.length mainBody))
          "0xdb99c8 read"
      , check "main checksum" (showHexWord (hMainChecksum stored)) (showHexWord (hMainChecksum derived)) (hMainChecksum stored == hMainChecksum derived) "0xdb90ac"
      , check "auxiliary 1 offset" (showHexWord (hAux1Offset stored)) (showHexWord (hAux1Offset derived)) (hAux1Offset stored == hAux1Offset derived) "0xdb99c8 read"
      , check
          "auxiliary 1 length"
          (showHexWord (hAux1Length stored))
          (showHexWord (fromIntegral (B.length aux1)))
          (hAux1Length stored == fromIntegral (B.length aux1) && hAux1Length stored `mod` 4 == 0)
          "0xdb89a4"
      , check "auxiliary 1 checksum" (showHexWord (hAux1Checksum stored)) (showHexWord (hAux1Checksum derived)) (hAux1Checksum stored == hAux1Checksum derived) "0xdb89a4"
      , check
          "auxiliary 1 pair bounds"
          (show (length (partAux1 parts)) ++ " pairs")
          "offset+length <= 0x10000 per pair (a violating pair is skipped, not fatal)"
          (all pair16Ok (partAux1 parts))
          "restore loop 0xdb91e0"
      , check "auxiliary 2 offset" (showHexWord (hAux2Offset stored)) (showHexWord (hAux2Offset derived)) (hAux2Offset stored == hAux2Offset derived) "0xdb99c8 read"
      , check
          "auxiliary 2 length"
          (showHexWord (hAux2Length stored))
          (showHexWord (fromIntegral (B.length aux2)))
          (hAux2Length stored == fromIntegral (B.length aux2) && hAux2Length stored `mod` 8 == 0)
          "0xdb89a4"
      , check "auxiliary 2 checksum" (showHexWord (hAux2Checksum stored)) (showHexWord (hAux2Checksum derived)) (hAux2Checksum stored == hAux2Checksum derived) "0xdb89a4"
      , check
          "auxiliary 2 pair bounds"
          (show (length (partAux2 parts)) ++ " pairs")
          "offset+length <= 0x42000 per pair (a violating pair is skipped, not fatal)"
          (all pair32Ok (partAux2 parts))
          "restore loop 0xdb91e0"
      , check
          "segment table"
          (show (length (partSegments parts)) ++ " entries")
          "ids 0..7 then 0xffffffff"
          (map segId (partSegments parts) == [0 .. 7])
          "loader 0x4174"
      , check
          "segment flags"
          (showHexWord (segFlag entry4) ++ "/" ++ showHexWord (segFlag entry5) ++ "/" ++ showHexWord (segFlag entry6))
          "0x1/0x1/0x0 for ids 4/5/6"
          (segFlag entry4 == 1 && segFlag entry5 == 1 && segFlag entry6 == 0)
          "stock bytes (no word-8 consumer in mapped load paths)"
      , check
          "image 4 size relation"
          (showHexWord (segSize entry4) ++ "/" ++ showHexWord (recDecodedSize (recHeader (partRecord4 parts))))
          "segment size = decoded size + 0x28"
          (segSize entry4 == recDecodedSize (recHeader (partRecord4 parts)) + 0x28)
          "stock relation (semantics unproved)"
      , check
          "image 5 size relation"
          (showHexWord (segSize entry5) ++ "/" ++ showHexWord (recDecodedSize (recHeader (partRecord5 parts))))
          "segment size = decoded size"
          (segSize entry5 == recDecodedSize (recHeader (partRecord5 parts)))
          "stock relation (semantics unproved)"
      , check
          "image 4 bss relation"
          (showHexWord (segBss entry4) ++ "/" ++ showHexWord (segBssLen entry4))
          "bss = load + decoded size, bss end = id-5 load"
          ( segBss entry4 == segLoad entry4 + recDecodedSize (recHeader (partRecord4 parts))
              && segBss entry4 + segBssLen entry4 == segLoad entry5
          )
          "stock relation (semantics unproved)"
      , check
          "image 2 size relation"
          (showHexWord (segSize entry2) ++ "/" ++ showHexWord (segFlashLen entry2))
          "segment size <= flash length"
          (segSize entry2 <= segFlashLen entry2)
          "segment table (raw copy; stock relation)"
      ]
      ++ decodedChecks "image 4 record" (partRecord4 parts)
      ++ decodedChecks "image 5 record" (partRecord5 parts)
      ++ recordChecks "image 4 record" (partRecord4 parts) entry4
      ++ recordChecks "image 5 record" (partRecord5 parts) entry5
      ++ [ check
             "trailer marker"
             (showHexWord (fromIntegral (trMarkerHalf (partTrailer parts))))
             "0xaaaa"
             (trMarkerHalf (partTrailer parts) == 0xaaaa)
             "boot record 0x1cc0 (halt otherwise)"
         , check "trailer size" (showHexWord (trSize (partTrailer parts))) "0x20" (trSize (partTrailer parts) == 0x20) "stock bytes"
         , check
             "trailer span"
             (showHexWord (trSpan (partTrailer parts)) ++ "/" ++ showHexWord (trSpan2 (partTrailer parts)))
             (showHexWord (hMainLength stored - fromIntegral trailerBytes))
             (trSpan (partTrailer parts) == hMainLength stored - fromIntegral trailerBytes && trSpan2 (partTrailer parts) == trSpan (partTrailer parts))
             "stock bytes (semantics unproved)"
         , check
             "trailer image words"
             (showHexWord (trImageLoad (partTrailer parts)) ++ "/" ++ showHexWord (trImageSize (partTrailer parts)))
             (showHexWord (segLoad entry4) ++ "/" ++ showHexWord (segSize entry4))
             (trImageLoad (partTrailer parts) == segLoad entry4 && trImageSize (partTrailer parts) == segSize entry4)
             "stock bytes (semantics unproved)"
         , check
             "file extent"
             (showHexWord (fromIntegral (parsedFileLength parsed)))
             (showHexWord (fromIntegral (stagingLength + fromIntegral (hAux2Offset stored + hAux2Length stored))))
             (fromIntegral (parsedFileLength parsed) == hAux2Offset stored + hAux2Length stored + fromIntegral stagingLength)
             "stock bytes"
         ]
      ++ (plainLoadCheck mainBody (plainLoadOffset (partTableLayout parts), snd stockPlainLoad)
          : [plainCheck mainBody pair | pair <- plainPins parts])
      ++ [ let window = plainWindow parts
            in check
                 "plain flash window"
                 (case window of
                    Left message -> message
                    Right (start, stop) -> showHexWord (fromIntegral start) ++ "/" ++ show (stop - start))
                 "aligned carried raw window containing every recognized extent, with the recognized header length"
                 (case window of Left _ -> False; Right _ -> True)
                 "raw loader geometry"
         ]

plainPins :: Parts -> [((Int, Word32), (Int, Word32))]
plainPins parts =
  [ ((start + k * 24 + 4, base), (start + k * 24 + 8, expectedLength k len))
  | (k, ((_, base), (_, len))) <- zip [0 ..] stockPlainExtents
  ]
  where
    start = plainExtentTableOffset (partTableLayout parts)
    expectedLength k len
      | partTableLayout parts == RecorderCompact && k == 2
          && mediaWordAt (partTableTail parts) (start + 56 - tableEndOffset RecorderCompact) == 0x26b5e0 = 0x26b5e0
      | otherwise = len

-- Carried-intact policy: a rebuild for this body must carry the stock restore
-- lists unchanged (E-ADJ-MOVERS-150); anything else wipes the affected
-- calibration ranges.
carriedChecks :: Parsed -> [Check]
carriedChecks parsed =
  let parts = parsedParts parsed
   in [ check
          "restore list 1 carried"
          (show (length (partAux1 parts)) ++ " pairs")
          (show (length stockAux1) ++ " stock pairs")
          (partAux1 parts == stockAux1)
          "carried-intact policy (E-ADJ-MOVERS-150)"
      , check
          "restore list 2 carried"
          (show (length (partAux2 parts)) ++ " pairs")
          (show (length stockAux2) ++ " stock pairs")
          (partAux2 parts == stockAux2)
          "carried-intact policy (E-ADJ-MOVERS-150)"
      ]

-- Plain-extent (base, length) words in the table tail: the three six-word
-- entries at media 0x1401b0/0x1401c8/0x1401e0 hold base at +4 and length at
-- +8. A legitimate plain-image move revises these pins deliberately.
stockPlainExtents :: [((Int, Word32), (Int, Word32))]
stockPlainExtents =
  [ ((0x1401b4, 0x01100000), (0x1401b8, 0x000ac5a0))
  , ((0x1401cc, 0x011c0000), (0x1401d0, 0x0001cca0))
  , ((0x1401e4, 0x011e0000), (0x1401e8, 0x0026a5e0))
  ]

-- The loader destination word at media 0x1401a8: the id-6 loader copies the
-- window here (id4 0x6a987c-0x6a98b0). It is a separate word from the first
-- entry's base at 0x1401b4; both read 0x01100000 on stock.
stockPlainLoad :: (Int, Word32)
stockPlainLoad = (0x1401a8, 0x01100000)

plainCheck :: B.ByteString -> ((Int, Word32), (Int, Word32)) -> Check
plainCheck mainBody ((baseOffset, base), (lengthOffset, expectedLength)) =
  check
    ("plain extent " ++ showHexWord base)
    (showHexWord (mediaWordAt mainBody baseOffset) ++ "/" ++ showHexWord (mediaWordAt mainBody lengthOffset))
    (showHexWord base ++ "/" ++ showHexWord expectedLength)
    (mediaWordAt mainBody baseOffset == base && mediaWordAt mainBody lengthOffset == expectedLength)
    "pinned plain-extent words (stock relation; a plain-image move revises these pins)"

plainLoadCheck :: B.ByteString -> (Int, Word32) -> Check
plainLoadCheck mainBody (offset, expected) =
  check
    ("plain load " ++ showHexWord expected)
    (showHexWord (mediaWordAt mainBody offset))
    (showHexWord expected)
    (mediaWordAt mainBody offset == expected)
    "pinned plain-load word (stock relation; a plain-image move revises these pins)"

stockIdentity :: B.ByteString
stockIdentity =
  "3030303239393731303030323939373230303032393937343030303239393739"
    <> B.concat (replicate 224 "30")

stockSegments :: [SegmentEntry]
stockSegments =
  [ SegmentEntry 0 0x00040000 0x0000dae0 0 0 0x00080000 0x00020000 0 0
  , SegmentEntry 1 0x000d0000 0x0000daa0 0 0 0x000a0000 0x00020000 0 0
  , SegmentEntry 2 0x00130000 0x0024bac8 0 0 0x00ca0000 0x00260000 0 0
  , SegmentEntry 3 0x003bc000 0x0007cfd4 0 0 0x01060000 0x00080000 0 0
  , SegmentEntry 4 0x0061c400 0x00282828 0x0089ec00 0x00352e00 0x00160000 0x00160000 0x20 1
  , SegmentEntry 5 0x00bf1a00 0x004f1e60 0x01460000 0x002bbe40 0x002c0000 0x002e0000 0x20 1
  , SegmentEntry 6 0x00bf1a00 0x004f1e60 0x01460000 0x002bbe40 0x005a0000 0x00360000 0x20 0
  , SegmentEntry 7 0 0 0 0 0x00b40000 0x00020000 0 0
  ]

stockRecord4 :: RecordHeader
stockRecord4 = RecordHeader 0x00282800 0x00146567 0x52 0x4000 4

stockRecord5 :: RecordHeader
stockRecord5 = RecordHeader 0x004f1e60 0x002c66d1 0xb2 0x4000 4

stockTrailer :: Trailer
stockTrailer = Trailer 0x024a0000 0x024a0000 0x20 0 0 0x0061c400 0x00282828 0xffff 0xaaaa

stockAux1 :: [Pair16]
stockAux1 =
  [ Pair16 0x0000 0x80, Pair16 0x00a2 0x1, Pair16 0x00a8 0x1, Pair16 0x00c4 0x1
  , Pair16 0x00d5 0x1, Pair16 0x0102 0x4, Pair16 0x011b 0x1e, Pair16 0x013e 0x2
  , Pair16 0x01ea 0x15, Pair16 0x0204 0x2, Pair16 0x020b 0x6, Pair16 0x0215 0x2
  , Pair16 0x0277 0x2, Pair16 0x0383 0x3, Pair16 0x038a 0x1, Pair16 0x03a8 0x1
  , Pair16 0x03b3 0x1, Pair16 0x03bc 0x1, Pair16 0x03d1 0x1, Pair16 0x108e 0xe
  , Pair16 0x1200 0x1, Pair16 0x1223 0x1, Pair16 0x16cc 0x1, Pair16 0x16d0 0x1
  , Pair16 0x16d4 0x1, Pair16 0x16d8 0x1, Pair16 0x16dc 0x1, Pair16 0x17c1 0x100
  , Pair16 0x18c8 0x2, Pair16 0x18cb 0x3, Pair16 0x18cf 0x2, Pair16 0x18d2 0x3
  , Pair16 0x18d6 0x2, Pair16 0x18d9 0x3, Pair16 0x18e4 0x2, Pair16 0x18e7 0x3
  , Pair16 0x18eb 0x2, Pair16 0x18ee 0x3, Pair16 0x18f2 0x2, Pair16 0x18f5 0x3
  , Pair16 0x18f9 0x2, Pair16 0x18fc 0x3, Pair16 0x1aa6 0x26, Pair16 0x1af2 0x26
  , Pair16 0x1c00 0xc, Pair16 0x1c10 0xc, Pair16 0x1d2a 0x20, Pair16 0x1d4c 0xc
  , Pair16 0x1d5a 0xc, Pair16 0x1d77 0x1e, Pair16 0x1d96 0x1c, Pair16 0x1e39 0x10
  , Pair16 0x1e5a 0x60, Pair16 0x1ec7 0x10, Pair16 0x1ef7 0x10, Pair16 0x1f27 0x20
  , Pair16 0x1f58 0x48, Pair16 0x1fe9 0x30, Pair16 0x2079 0x48, Pair16 0x226d 0x2
  , Pair16 0x2614 0xa, Pair16 0x2622 0x7, Pair16 0x2777 0x5, Pair16 0x277d 0x12
  , Pair16 0x2790 0x9, Pair16 0x28b2 0x1b, Pair16 0x28d0 0x2, Pair16 0x28d3 0xc
  , Pair16 0x28e0 0xc, Pair16 0x2a40 0xa, Pair16 0x2a81 0x2, Pair16 0x2b00 0x1
  , Pair16 0x2b02 0x1, Pair16 0x3867 0x50, Pair16 0x3da3 0x8, Pair16 0x65fd 0x2
  , Pair16 0x82c6 0x8, Pair16 0x96f0 0xa, Pair16 0x9706 0x11, Pair16 0x9750 0x4
  , Pair16 0x9796 0x1, Pair16 0x97b9 0x6, Pair16 0x97cc 0x2, Pair16 0x97d6 0xa
  , Pair16 0x97e5 0x1, Pair16 0x9819 0x2, Pair16 0x9821 0xc, Pair16 0x9830 0x4
  , Pair16 0x9904 0x5, Pair16 0x9918 0x2, Pair16 0x9920 0x5, Pair16 0x992f 0x2
  , Pair16 0x996b 0x14, Pair16 0x9994 0xc, Pair16 0x99fc 0x14, Pair16 0x9a13 0x8
  , Pair16 0x9a1d 0xb, Pair16 0x9ad8 0x1
  ]

stockAux2 :: [Pair32]
stockAux2 = [Pair32 0x000179d5 0x8, Pair32 0x000179df 0x4, Pair32 0x00018ac0 0x1]

pair16Coverage :: [Pair16] -> Int
pair16Coverage pairs = sum [fromIntegral (pair16Length p) | p <- pairs]

pair32Coverage :: [Pair32] -> Int
pair32Coverage pairs = sum [fromIntegral (pair32Length p) | p <- pairs]

-- Pinned stock values of the public X-M1 1.50 DAT, for the stock verification.
stockChecks :: Parsed -> [Check]
stockChecks parsed =
  let parts = parsedParts parsed
      header = parsedHeader parsed
      segmentStock e =
        check
          ("stock segment entry " ++ show (segId e))
          (show (entryById (segId e) (partSegments parts)))
          (show e)
          (entryById (segId e) (partSegments parts) == e)
          "pinned container bytes"
   in [ check "stock identity records" (hex (B.take 16 (partIdentity parts)) ++ "...") (hex (B.take 16 stockIdentity) ++ "...") (partIdentity parts == stockIdentity) "pinned container bytes"
      , check "stock table layout" (show (partTableLayout parts)) "Stock" (partTableLayout parts == Stock) "pinned root pointer"
      , check
          "stock version words"
          (showHexWord (partVersionMajor parts) ++ "/" ++ showHexWord (partVersionMinor parts))
          "0x1/0x50"
          (partVersionMajor parts == 1 && partVersionMinor parts == 0x50)
          "pinned container bytes"
      , check "stock main checksum" (showHexWord (hMainChecksum header)) "0x8f0ab18f" (hMainChecksum header == 0x8f0ab18f) "pinned container bytes"
      , check "stock main range" (showHexWord (hMainOffset header) ++ "/" ++ showHexWord (hMainLength header)) "0x0/0x24a0020" (hMainOffset header == 0 && hMainLength header == 0x24a0020) "pinned container bytes"
      , check
          "stock auxiliary 1 range"
          (showHexWord (hAux1Offset header) ++ "/" ++ showHexWord (hAux1Length header) ++ "/" ++ showHexWord (hAux1Checksum header))
          "0x24a0020/0x188/0xffffb2d6"
          (hAux1Offset header == 0x24a0020 && hAux1Length header == 0x188 && hAux1Checksum header == 0xffffb2d6)
          "pinned container bytes"
      , check
          "stock auxiliary 2 range"
          (showHexWord (hAux2Offset header) ++ "/" ++ showHexWord (hAux2Length header) ++ "/" ++ showHexWord (hAux2Checksum header))
          "0x24a01a8/0x18/0xfffffbff"
          (hAux2Offset header == 0x24a01a8 && hAux2Length header == 0x18 && hAux2Checksum header == 0xfffffbff)
          "pinned container bytes"
      , check
          "stock restore coverage"
          (show (pair16Coverage (partAux1 parts) + pair32Coverage (partAux2 parts)) ++ " bytes")
          "1487 bytes"
          (pair16Coverage (partAux1 parts) + pair32Coverage (partAux2 parts) == 1487)
          "E-ADJ-MOVERS-150 census"
      ]
      ++ map segmentStock stockSegments
      ++ [ check
             "stock image 4 header"
             (show (recHeader (partRecord4 parts)))
             (show stockRecord4)
             (recHeader (partRecord4 parts) == stockRecord4)
             "pinned container bytes"
         , check
             "stock image 5 header"
             (show (recHeader (partRecord5 parts)))
             (show stockRecord5)
             (recHeader (partRecord5 parts) == stockRecord5)
             "pinned container bytes"
         , check
             "stock trailer"
             (show (partTrailer parts))
             (show stockTrailer)
             (partTrailer parts == stockTrailer)
             "pinned container bytes"
         ]

firstDiff :: B.ByteString -> B.ByteString -> (Int, Maybe Int)
firstDiff a b =
  let common = min (B.length a) (B.length b)
      flags = zipWith (/=) (B.unpack (B.take common a)) (B.unpack (B.take common b))
      firstIndex = case lookup True (zip flags [0 ..]) of
        Just index -> Just index
        Nothing
          | B.length a == B.length b -> Nothing
          | otherwise -> Just common
      changed = length (filter id flags) + abs (B.length a - B.length b)
   in (changed, firstIndex)

partPairs :: [(String, Parts -> B.ByteString, PartPolicy)]
partPairs =
  [ ("identity records", partIdentity, CarriedMustMatch)
  , ("version words", \p -> putWord32 (partVersionMajor p) <> putWord32 (partVersionMinor p), CarriedPolicyChecked)
  , ("selector word", \p -> putWord32 (partSelector p), CarriedMustMatch)
  , ("range fill", partRangeFill, CarriedMustMatch)
  , ("table layout", putMediaWord . aliasPointer . segmentTableOffset . partTableLayout, ImageContent)
  , ("main pre-table", partPreTable, ImageContent)
  , ("segment table", \p -> B.concat (map serializeSegment (partSegments p)) <> putMediaWord 0xffffffff, ImageContent)
  , ("main table tail", partTableTail, ImageContent)
  , ("image 4 header", serializeRecordHeader . recHeader . partRecord4, ImageContent)
  , ("image 4 stream", recStream . partRecord4, ImageContent)
  , ("main gap 4-5", partGap45, ImageContent)
  , ("image 5 header", serializeRecordHeader . recHeader . partRecord5, ImageContent)
  , ("image 5 stream", recStream . partRecord5, ImageContent)
  , ("main post head", partPostHead, ImageContent)
  , ("image 2 extent", partImage2, ImageContent)
  , ("main post tail", partPostTail, ImageContent)
  , ("trailer", serializeTrailer . partTrailer, ImageContent)
  , ("restore list 1", serializeAux1 . partAux1, CarriedMustMatch)
  , ("restore list 2", serializeAux2 . partAux2, CarriedMustMatch)
  ]

-- Emission discipline: every named part read back from the assembled bytes must
-- equal the part that was supplied, so a supplied extent whose length or
-- position the stored words do not describe is refused rather than emitted
-- under words that describe something else (reports/update-container-rebuild.md
-- section 3).
assemblyChecks :: Parts -> Parts -> [Check]
assemblyChecks supplied stored =
  [ check
      ("assembled " ++ name)
      (partDigest (extract stored))
      (partDigest (extract supplied))
      (extract stored == extract supplied)
      "bytes read back from the assembly against the supplied part"
  | (name, extract, _) <- partPairs
  ]
  where
    partDigest bytes
      | B.length bytes <= 16 = hex bytes
      | otherwise = show (B.length bytes) ++ " bytes"

diffParts :: Parsed -> Parsed -> [PartDiff]
diffParts left right =
  [ PartDiff
      { diffPart = name
      , diffChangedBytes = changed
      , diffFirstOffset = firstOffset
      , diffPolicy = policy
      }
  | (name, extract, policy) <- partPairs
  , let (changed, firstOffset) = firstDiff (extract (parsedParts left)) (extract (parsedParts right))
  ]
