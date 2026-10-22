{-# LANGUAGE OverloadedStrings #-}
-- | The Classic Chrome icon RAM-push payload over the RC0-installed X-M1 1.50
-- images: the reviewed Classic Chrome publications, the U+F073 icon words,
-- the Q-node pointers and the image-5 fragment words (live view, frames
-- remaining), with the glyph record and the Q arrays held in the allocation
-- after the installer. No I/O.
module FujiAF.ClassicChromeIconRam
  ( FontWindow
  , fontWindowFromDat
  , Image5Fragment (..)
  , fragmentPath
  , Image5Word (..)
  , parseImage5Fragment
  , IconAllocation (..)
  , iconAllocation
  , iconContent
  , iconContentSize
  , CcCarry (..)
  , directoryWord
  , iconRamPublications
  , ccIconSetupCount
  , ccIconActivationCount
  , ccIconPublicationCount
  , ccIconInstallerExtent
  , ccIconContentOffset
  , ccIconAllocationSize
  ) where

import Control.Monad (forM, forM_, unless, when)
import Data.Aeson (Value (..))
import qualified Data.Aeson.Key as K
import qualified Data.Aeson.KeyMap as KM
import Data.Bits (complement, shiftR, (.&.), (.|.))
import qualified Data.ByteString as B
import Data.List (nub, sortOn)
import Data.Scientific (toBoundedInteger)
import qualified Data.Text as T
import qualified Data.Vector as V
import Data.Word (Word32)
import FujiAF.ClassicChrome (Address (..), image5DecodeBase, stockFirmwareSha256)
import FujiAF.ClassicChromeIcon (Image4Write (..), iconGlyphRecord, iconImage4Writes, iconPerMille, label83dRecord, qLabelsArray, qValuesArray)
import FujiAF.ClassicChromeRam (CcPublication (..), PublicationPhase (..), Rc0Images (..), ccActivationCount, ccInstallerStart, ccPublicationCount, ccSetupCount)
import FujiAF.JSON (decodeStrictIntegers)
import Numeric (readHex, showHex)

-- | RAM [0x003bc000, 0x0043c000): segment 3 (FONT_ICON.FDL) as its loader
-- copies the whole 0x80000-byte window.
newtype FontWindow = FontWindow B.ByteString

fontRamBase, fontWindowSize, fontDatOffset, fontCommandsRam :: Word32
fontRamBase = 0x003bc000
fontWindowSize = 0x80000
fontDatOffset = 0x0106025c
fontCommandsRam = 0x003f9ced

-- | DAT raw bytes [0x0106025c, +0x80000), each the complement of the media byte.
fontWindowFromDat :: B.ByteString -> Either String FontWindow
fontWindowFromDat dat = do
  let raw = B.take (fromIntegral fontWindowSize) (B.drop (fromIntegral fontDatOffset) dat)
      window = B.map complement raw
  unless (B.length raw == fromIntegral fontWindowSize) $ Left "DAT is shorter than the font window"
  unless (B.take 8 window == "FDL Font") $ Left "font window does not start with the FDL header"
  pure (FontWindow window)

-- | The image-5 fragments the payload publishes, in activation order.
data Image5Fragment = LiveViewFragment | FramesFragment
  deriving (Eq, Show, Enum, Bounded)

fragmentPath :: Image5Fragment -> FilePath
fragmentPath LiveViewFragment = "host/firmware/classic-chrome-liveview-image5.json"
fragmentPath FramesFragment = "host/firmware/classic-chrome-frames-image5.json"

fragmentId :: Image5Fragment -> String
fragmentId LiveViewFragment = "CLASSIC-CHROME-LIVEVIEW-01-IMAGE5"
fragmentId FramesFragment = "CLASSIC-CHROME-FRAMES-01-IMAGE5"

fragmentWordCount :: Image5Fragment -> Int
fragmentWordCount LiveViewFragment = 5
fragmentWordCount FramesFragment = 3

data Image5Word = Image5Word
  { image5WordAddress :: Address
  , image5WordCurrent :: Word32
  , image5WordNew :: Word32
  , image5WordRole :: String
  }
  deriving (Eq, Show)

-- | An in-place image-5 word patch set for X-M1 1.50 ('fragmentPath'), its
-- words in `order`.
parseImage5Fragment :: Image5Fragment -> B.ByteString -> Either String [Image5Word]
parseImage5Fragment fragment document = do
  value <- either (Left . (label ++)) Right (decodeStrictIntegers document)
  identifier <- text =<< key "id" value
  unless (identifier == fragmentId fragment) $ Left (label ++ "id mismatch")
  target <- key "target" value
  firmware <- text =<< key "firmware_sha256" target
  unless (firmware == stockFirmwareSha256) $ Left (label ++ "target firmware is not X-M1 1.50")
  image <- key "image" value
  segment <- integer =<< key "segment_id" image
  decodeBase <- hexWord =<< key "decode_base" image
  let Address image5Base = image5DecodeBase
  unless (segment == 5 && decodeBase == image5Base) $ Left (label ++ "not an image-5 patch set at 0x00bf1a00")
  blobs <- array =<< key "blobs" value
  unless (null blobs) $ Left (label ++ "blobs present; only words are published")
  entries <- array =<< key "words" value
  parsed <- forM entries $ \entry -> do
    order <- integer =<< key "order" entry
    address <- hexWord =<< key "address" entry
    current <- hexWord =<< key "current" entry
    new <- hexWord =<< key "new" entry
    role <- text =<< key "role" entry
    pure (order, Image5Word (Address address) current new role)
  let ordered = map snd (sortOn fst parsed)
      count = fragmentWordCount fragment
  unless (map fst (sortOn fst parsed) == [1 .. length parsed] && length parsed == count) $
    Left (label ++ show count ++ " words ordered 1.." ++ show count ++ " required")
  unless (all (\w -> let Address a = image5WordAddress w in a .&. 3 == 0) ordered) $
    Left (label ++ "unaligned word")
  pure ordered
  where
    label = fragmentPath fragment ++ ": "
    key name (Object object) = maybe (Left (label ++ "missing " ++ name)) Right (KM.lookup (K.fromString name) object)
    key name _ = Left (label ++ "object required around " ++ name)
    text (String t) = Right (T.unpack t)
    text _ = Left (label ++ "text required")
    integer :: Value -> Either String Int
    integer (Number n) = maybe (Left (label ++ "integer required")) Right (toBoundedInteger n)
    integer _ = Left (label ++ "integer required")
    array (Array values) = Right (V.toList values)
    array _ = Left (label ++ "list required")
    hexWord :: Value -> Either String Word32
    hexWord v = do
      t <- text v
      case t of
        '0' : 'x' : digits | length digits == 8, [(w, "")] <- readHex digits -> Right w
        _ -> Left (label ++ "eight-digit hex word required, got " ++ t)

-- | Allocation addresses of the glyph record and the two Q arrays.
data IconAllocation = IconAllocation
  { allocationRecord :: Address
  , allocationQValues :: Address
  , allocationQLabels :: Address
  }
  deriving (Eq, Show)

-- Offsets from 'ccIconContentOffset': the record, then the Q values and the
-- Q labels, each start rounded up to 8 bytes, so the arrays follow a record
-- of any length.
recordOffset, qValuesOffset, qLabelsOffset :: Word32
recordOffset = 0
qValuesOffset = align8 (recordOffset + byteLength iconGlyphRecord)
qLabelsOffset = align8 (qValuesOffset + byteLength qValuesArray)

align8 :: Word32 -> Word32
align8 n = (n + 7) `div` 8 * 8

byteLength :: B.ByteString -> Word32
byteLength = fromIntegral . B.length

-- | The record, the Q values and the Q labels at their offsets from
-- 'ccIconContentOffset'.
iconContent :: B.ByteString
iconContent =
  pad qValuesOffset iconGlyphRecord <> pad (qLabelsOffset - qValuesOffset) qValuesArray <> qLabelsArray
  where
    pad n bytes = bytes <> B.replicate (fromIntegral n - B.length bytes) 0

iconContentSize :: Word32
iconContentSize = qLabelsOffset + byteLength qLabelsArray

iconAllocation :: Word32 -> IconAllocation
iconAllocation base =
  let at offset = Address (base + ccIconContentOffset + offset)
   in IconAllocation (at recordOffset) (at qValuesOffset) (at qLabelsOffset)

ccIconSetupCount, ccIconActivationCount, ccIconPublicationCount :: Int
ccIconSetupCount = ccSetupCount + 16
ccIconActivationCount = ccActivationCount + 6 + sum (map fragmentWordCount [minBound .. maxBound])
ccIconPublicationCount = ccIconSetupCount + ccIconActivationCount

-- | From the installer start, with the 161-entry publication table.
ccIconInstallerExtent :: Word32
ccIconInstallerExtent = 0x15a0

-- | The content follows the installer at every 8-byte-aligned base.
ccIconContentOffset :: Word32
ccIconContentOffset = ccInstallerStart 0 + ccIconInstallerExtent

ccIconAllocationSize :: Word32
ccIconAllocationSize = ccIconContentOffset + iconContentSize

-- | The U+F073 directory word naming the allocation's glyph record: begin
-- relative to the font commands at RAM 0x003f9ced, flags 1.
directoryWord :: Address -> Either String Word32
directoryWord (Address record) = do
  when (record < fontCommandsRam) $ Left ("glyph record " ++ hex record ++ " lies below the font commands")
  let begin = record - fontCommandsRam
  unless (begin < 0x01000000) $ Left ("glyph record offset " ++ hex begin ++ " from the font commands does not fit 24 bits")
  pure (begin .|. 0x01000000)

-- | A Classic Chrome publication whose new word the icon replaces, kept at
-- its index, phase and old word.
data CcCarry = CcCarry
  { carryIndex :: Int
  , carryAddress :: Address
  , carryOld :: Word32
  , carryCcNew :: Word32
  , carryNew :: Word32
  }
  deriving (Eq, Show)

-- | The one Classic Chrome publication the icon may share: field 1 of the
-- CLASSIC CHROME RAW-conversion quadruple.
carriedAddresses :: [Address]
carriedAddresses = [Address 0x0061c3dc]

rc0Image4Base, windowEnd, image2Base, record83d, record83dEnd, directoryByte, perMilleByte :: Word32
rc0Image4Base = 0x0061c340
windowEnd = 0x0061c400
image2Base = 0x00130000
record83d = 0x0037b7c6
record83dEnd = record83d + 66
directoryByte = 0x003f821d
perMilleByte = 0x0043826b

-- | The label-icon words inside the Classic Chrome labels arrays.
labelIconWords :: [Image4Write]
labelIconWords = [w | w <- iconImage4Writes, i4Address w < windowEnd]

-- | The Q node pointers in the flash write order, each retargeted from the
-- font tail to its allocation copy.
qPointers :: IconAllocation -> Either String [(Image4Write, Address)]
qPointers allocation = forM [w | w <- iconImage4Writes, i4Address w >= windowEnd] $ \w -> case i4Address w of
  0x0078b024 -> Right (w, allocationQLabels allocation)
  0x0078b020 -> Right (w, allocationQValues allocation)
  other -> Left ("icon image-4 write at " ++ hex other ++ " is not a Q node pointer")

-- | The Classic Chrome publications with the icon's: setup gains the 0x83d
-- record and per-mille words, activation the directory, label-icon, Q
-- pointer and image-5 fragment words. Refuses any address the Classic Chrome
-- publications already hold, except 'carriedAddresses'.
iconRamPublications
  :: Rc0Images -> FontWindow -> [Image5Word] -> IconAllocation -> [CcPublication]
  -> Either String ([CcPublication], [CcCarry])
iconRamPublications rc0 (FontWindow font) image5Words allocation cc = do
  unless (length cc == ccPublicationCount) $ Left "Classic Chrome publication count differs from the reviewed payload"
  let (ccSetup, ccActivation) = splitAt ccSetupCount cc
  unless (all ((== Setup) . publicationPhase) ccSetup && all ((== Activation) . publicationPhase) ccActivation) $
    Left "Classic Chrome publications are not setup then activation"
  unless (B.length font == fromIntegral fontWindowSize) $ Left "font window size"

  let image4Rc0 a = if a < rc0Image4Base then 0 else wordAt (rc0Image4 rc0) rc0Image4Base a
      image5Rc0 = wordAt (rc0Image5 rc0) image5Base
      Address image5Base = image5DecodeBase
      image2Old = wordAt (rc0Image2 rc0) image2Base
      image2New = wordAt (patchBytes image2Base (rc0Image2 rc0) record83d label83dRecord) image2Base
      fontOld = wordAt font fontRamBase
      changedWords old new lo hi = [(a, old a, new a) | a <- [lo, lo + 4 .. hi - 4], old a /= new a]

  unless (B.length label83dRecord == 66) $ Left "0x83d record is not 66 bytes"
  unless (B.length iconGlyphRecord == fromIntegral qValuesOffset - fromIntegral recordOffset && B.length qValuesArray == fromIntegral (qLabelsOffset - qValuesOffset)) $
    Left "glyph record or Q values length differs from the allocation layout"
  let record83dWords = changedWords image2Old image2New (record83d .&. complement 3) ((record83dEnd + 3) .&. complement 3)

  unless (B.unpack (slice font (directoryByte - fontRamBase) 4) == [0x80, 0xd6, 0x00, 0x00]) $
    Left "U+F073 directory word is not the stock 80 d6 00 00"
  unless (B.unpack (slice font (perMilleByte - fontRamBase) 2) == [0xe8, 0x03]) $
    Left "U+F073 per-mille is not the stock 0x03e8"
  directory <- directoryWord (allocationRecord allocation)
  let perMilleFont = patchBytes fontRamBase font perMilleByte (B.pack [fromIntegral iconPerMille, fromIntegral (iconPerMille `shiftR` 8)])
      directoryFont = patchBytes fontRamBase font directoryByte (B.pack [fromIntegral (directory `shiftR` s) | s <- [0, 8, 16, 24]])
      perMilleWords = changedWords fontOld (wordAt perMilleFont fontRamBase) (perMilleByte .&. complement 3) ((perMilleByte + 5) .&. complement 3)
      directoryWords = changedWords fontOld (wordAt directoryFont fontRamBase) (directoryByte .&. complement 3) ((directoryByte + 7) .&. complement 3)

  pointers <- qPointers allocation
  qWords <- forM pointers $ \(w, Address target) -> do
    let a = i4Address w
    unless (image4Rc0 a == i4Old w) $ Left ("RC0 Q pointer at " ++ hex a ++ " is not the stock " ++ hex (i4Old w))
    pure (a, i4Old w, target, i4Role w ++ " (allocation copy)")
  fragmentWords <- forM image5Words $ \w -> do
    let Address a = image5WordAddress w
    unless (image5Rc0 a == image5WordCurrent w) $ Left ("RC0 image-5 word at " ++ hex a ++ " is not the fragment's current word")
    pure (a, image5WordCurrent w, image5WordNew w, image5WordRole w)

  let ccAt a = [(i, p) | (i, p) <- zip [0 :: Int ..] cc, publicationAddress p == Address a]
  labelWords <- fmap concat $ forM labelIconWords $ \w -> let a = i4Address w in case ccAt a of
    [] -> pure [(a, image4Rc0 a, i4New w, i4Role w)]
    [_] | Address a `elem` carriedAddresses -> pure []
    _ -> Left ("label-icon word " ++ hex a ++ " overlaps a Classic Chrome publication")
  carries <- forM carriedAddresses $ \address@(Address a) -> case (ccAt a, [w | w <- labelIconWords, i4Address w == a]) of
    ([(i, p)], [w]) -> do
      unless (publicationOld p == image4Rc0 a && publicationPhase p == Activation) $
        Left ("carried Classic Chrome publication at " ++ hex a ++ " is not an RC0 activation word")
      pure (CcCarry i address (publicationOld p) (publicationNew p) (i4New w))
    _ -> Left ("carried address " ++ hex a ++ " is not one Classic Chrome publication and one label-icon word")

  let publication phase (a, old, new, role) = CcPublication phase (Address a) old new role
      setup =
        map (\(a, o, n) -> publication Setup (a, o, n, "image-2 record 0x83d U+F073 icon")) record83dWords
          ++ map (\(a, o, n) -> publication Setup (a, o, n, "U+F073 per-mille 0x0320")) perMilleWords
      activation =
        map (\(a, o, n) -> publication Activation (a, o, n, "U+F073 directory word to the allocation's glyph record")) directoryWords
          ++ map (publication Activation) labelWords
          ++ map (publication Activation) qWords
          ++ map (publication Activation) fragmentWords
      carried = [maybe p (\c -> p {publicationNew = carryNew c}) (lookup i [(carryIndex c, c) | c <- carries]) | (i, p) <- zip [0 :: Int ..] cc]
      (carriedSetup, carriedActivation) = splitAt ccSetupCount carried
      combined = carriedSetup ++ setup ++ carriedActivation ++ activation
      ccAddresses = map publicationAddress cc
      overlapping = [a | p <- setup ++ activation, let a = publicationAddress p, a `elem` ccAddresses]
  unless (null overlapping) $
    Left ("icon publications overlap Classic Chrome publications at " ++ unwords [hex a | Address a <- overlapping])
  forM_ carries $ \c -> when (carryOld c == carryNew c) $ Left "carried publication does not change its word"
  unless (length (nub (map publicationAddress combined)) == length combined && all (\p -> publicationOld p /= publicationNew p) combined) $
    Left "publications must be distinct changed words"
  pure (combined, carries)

patchBytes :: Word32 -> B.ByteString -> Word32 -> B.ByteString -> B.ByteString
patchBytes base bytes address fresh =
  let offset = fromIntegral (address - base)
   in B.take offset bytes <> fresh <> B.drop (offset + B.length fresh) bytes

slice :: B.ByteString -> Word32 -> Int -> B.ByteString
slice bytes offset n = B.take n (B.drop (fromIntegral offset) bytes)

wordAt :: B.ByteString -> Word32 -> Word32 -> Word32
wordAt bytes base address =
  let offset = fromIntegral (address - base)
   in if address < base || offset + 4 > B.length bytes
        then error ("address " ++ hex address ++ " lies outside the image at " ++ hex base)
        else sum [fromIntegral (B.index bytes (offset + i)) * 256 ^ i | i <- [0 .. 3 :: Int]]

hex :: Word32 -> String
hex w = let digits = showHex w "" in "0x" ++ replicate (8 - length digits) '0' ++ digits
