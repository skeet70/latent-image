-- | The Exif block layout the writer must leave, read back from its output bytes independently
-- of either implementation: markers and TIFF header, then IFD0, Exif, maker note,
-- interoperability, GPS and IFD1 parsed as TIFF.
module Replacement.ExifWriter.Structure (checkHeader) where

import Control.Monad (forM_, unless, when)
import Data.Bits (shiftL, (.|.))
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as BC
import Data.Word (Word16, Word32)

data Entry = Entry {entryTag :: Word16, entryType :: Word16, entryCount :: Word32, entryField :: Word32}

type Check = Either String

half :: B.ByteString -> Int -> Check Word16
half bytes at
  | at < 0 || at + 2 > B.length bytes = Left ("halfword at " ++ show at ++ " lies outside the block")
  | otherwise = Right (fromIntegral (B.index bytes at) .|. fromIntegral (B.index bytes (at + 1)) `shiftL` 8)

word :: B.ByteString -> Int -> Check Word32
word bytes at
  | at < 0 || at + 4 > B.length bytes = Left ("word at " ++ show at ++ " lies outside the block")
  | otherwise = Right (foldr (\index acc -> acc `shiftL` 8 .|. fromIntegral (B.index bytes (at + index))) 0 [0 .. 3])

typeBytes :: Word16 -> Int
typeBytes valueType = case valueType of
  1 -> 1
  2 -> 1
  3 -> 2
  4 -> 4
  5 -> 8
  7 -> 1
  9 -> 4
  10 -> 8
  _ -> 0

-- | An IFD at @offset@ in @tiff@ whose value offsets count from @base@; every out-of-line value
-- must end inside @limit@.
directory :: String -> B.ByteString -> Int -> Int -> Int -> Check ([Entry], Word32)
directory name tiff limit base offset = do
  count <- fromIntegral <$> half tiff offset
  when (count == 0) (Left (name ++ ": empty directory"))
  entries <- mapM (\index -> entryAt (offset + 2 + 12 * index)) [0 .. count - 1]
  next <- word tiff (offset + 2 + 12 * count)
  let tags = map entryTag entries
  unless (and (zipWith (<) tags (drop 1 tags))) (Left (name ++ ": tags are not strictly ascending"))
  forM_ entries $ \entry -> do
    let size = fromIntegral (entryCount entry) * typeBytes (entryType entry)
    when (size > 4 && base + fromIntegral (entryField entry) + size > limit) $
      Left (name ++ ": value of tag " ++ show (entryTag entry) ++ " ends past the written block")
  pure (entries, next)
  where
    entryAt at = Entry <$> half tiff at <*> half tiff (at + 2) <*> word tiff (at + 4) <*> word tiff (at + 8)

find :: String -> Word16 -> [Entry] -> Check Entry
find name tag entries = case [entry | entry <- entries, entryTag entry == tag] of
  [entry] -> Right entry
  _ -> Left (name ++ ": tag " ++ show tag ++ " is not present once")

text :: B.ByteString -> Int -> Entry -> B.ByteString
text tiff base entry = B.take (fromIntegral (entryCount entry)) (B.drop (base + fromIntegral (entryField entry)) tiff)

-- | Problems with an Exif block of @size@ bytes written with directory @format@ and GPS
-- @option@; empty when the layout holds.
checkHeader :: Word32 -> Word32 -> Word32 -> B.ByteString -> [String]
checkHeader format option size output = either pure (const []) $ do
  unless (B.take 4 output == B.pack [0xff, 0xd8, 0xff, 0xe1]) (Left "SOI and APP1 markers differ")
  unless (B.take 6 (B.drop 6 output) == BC.pack "Exif\0\0") (Left "Exif identifier differs")
  unless (B.take 8 (B.drop 12 output) == B.pack [0x49, 0x49, 0x2a, 0, 8, 0, 0, 0]) (Left "TIFF header differs")
  let tiff = B.drop 12 output
      limit = fromIntegral size - 12
  (primary, next) <- directory "IFD0" tiff limit 0 8
  make <- find "IFD0" 0x10f primary
  unless (text tiff 0 make == BC.pack "FUJIFILM\0") (Left "IFD0 Make differs")
  model <- find "IFD0" 0x110 primary
  unless (text tiff 0 model == BC.pack "X-M1\0") (Left "IFD0 Model differs")
  exifPointer <- find "IFD0" 0x8769 primary
  (exif, _) <- directory "Exif" tiff limit 0 (fromIntegral (entryField exifPointer))
  forM_ [entry | entry <- exif, entryTag entry == 0x927c] $ \maker -> do
    let start = fromIntegral (entryField maker)
    unless (B.take 12 (B.drop start tiff) == BC.pack "FUJIFILM" <> B.pack [12, 0, 0, 0]) (Left "maker-note header differs")
    directory "maker note" tiff limit start (start + 12)
  interopPointer <- find "Exif" 0xa005 exif
  (interop, interopNext) <- directory "interoperability" tiff limit 0 (fromIntegral (entryField interopPointer))
  index <- find "interoperability" 1 interop
  unless (entryField index `elem` [0x00383952, 0x00333052]) (Left "interoperability index is neither R98 nor R03")
  version <- find "interoperability" 2 interop
  unless (entryField version == 0x30303130) (Left "interoperability version differs")
  unless (interopNext == 0) (Left "interoperability IFD has a successor")
  let gps = [entry | entry <- primary, entryTag entry == 0x8825]
  when (format == 0 && (option == 1) /= not (null gps)) (Left "GPS pointer presence differs from the GPS option")
  when (option == 1) $ forM_ gps $ \pointer -> do
    (fields, _) <- directory "GPS" tiff limit 0 (fromIntegral (entryField pointer))
    versionId <- find "GPS" 0 fields
    unless (entryField versionId == 0x00000202) (Left "GPS version differs")
  when (next == 0) (Left "IFD0 has no IFD1")
  (thumbnail, thumbnailNext) <- directory "IFD1" tiff limit 0 (fromIntegral next)
  mapM_ (\tag -> find "IFD1" tag thumbnail) [0x201, 0x202]
  compression <- find "IFD1" 0x103 thumbnail
  unless (entryField compression == 6) (Left "IFD1 compression is not JPEG")
  unless (thumbnailNext == 0) (Left "IFD1 has a successor")
