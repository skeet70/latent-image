{-# LANGUAGE BangPatterns #-}
{-# LANGUAGE OverloadedStrings #-}
module Emulation.PlaybackDecode (Snapshot, snapshot, withEndpoint, SourceCopy, sourceCopy, withSourceCopy, inspect, physical, ByteRange, rangeAddress, rangeCount, sourceRange, outputRange, OutputEvent (..), CompletionBinding, completionBinding, completionRequestRange, inspectRelease) where

import Codec.Picture (convertRGB8, decodeJpeg, imageHeight, imageWidth, pixelAt, PixelRGB8 (..))
import Control.Monad (unless)
import Data.Aeson (Value, object, (.=))
import Data.Bits ((.&.), shiftR)
import qualified Data.ByteString as B
import Data.Word (Word32, Word8)

newtype Address = Address Word32 deriving (Eq, Show)
newtype ByteCount = ByteCount Int deriving (Eq, Show)
data Snapshot = Snapshot { parameters :: B.ByteString, registers :: B.ByteString, input :: B.ByteString, output :: B.ByteString, endpoint :: Maybe Word32, construction :: Maybe SourceCopy } deriving (Eq, Show)
data Job = Job { owner :: Word32, source :: Address, destination :: Address, count :: ByteCount, capacity :: ByteCount, scale :: Int } deriving (Eq, Show)
data Geometry = Geometry { width :: Int, height :: Int, pitch :: Int, rows :: Int, extent :: Int }
data ByteRange = ByteRange { rangeAddress :: Word32, rangeCount :: Int }
newtype OutputEvent = OutputEvent Word32
data CompletionBinding = CompletionBinding Job Geometry B.ByteString OutputEvent
data SourceCopy = SourceCopy Address B.ByteString B.ByteString deriving (Eq, Show)

physical :: Word32 -> Word32
physical = (.&. 0x3fffffff)

word :: B.ByteString -> Int -> Word32
word bytes offset = sum [fromIntegral (B.index bytes (offset + i)) * 256 ^ i | i <- [0..3]]

snapshot :: [B.ByteString] -> Either String Snapshot
snapshot [params, regs, src, dst] = do
  unless (B.length params == 0x78 && B.length regs == 16) (Left "missing event-time parameters or programmed store operands")
  pure (Snapshot params regs src dst Nothing Nothing)
snapshot _ = Left "missing event-time decode snapshot"

withEndpoint :: Word32 -> Snapshot -> Snapshot
withEndpoint measured s = s { endpoint = Just measured }

sourceCopy :: Word32 -> B.ByteString -> B.ByteString -> Either String SourceCopy
sourceCopy address copied beforeTail = do
  unless (not (B.null copied) && B.length copied <= 16*1024*1024 && B.length beforeTail == 4096
    && toInteger (physical address) + toInteger (B.length copied) + 4096 <= 0x20000000)
    (Left "preview copy observation exceeds source or page bounds")
  pure (SourceCopy (Address (physical address)) copied beforeTail)

withSourceCopy :: SourceCopy -> Snapshot -> Snapshot
withSourceCopy copied s = s { construction = Just copied }

sourceIdentity :: B.ByteString -> Job -> Snapshot -> Either String Value
sourceIdentity fixture j s = case uniqueOffset (input s) of
  Right offset -> pure (object ["kind" .= ("literal fixture subrange" :: String), "fixture_offset" .= offset
    , "fixture_bytes" .= B.length (input s), "programmed_bytes" .= B.length (input s), "whole_span_exact" .= True])
  Left _ -> do
    SourceCopy destination' copied beforeTail <- maybe (Left "actual source is not a literal fixture subrange and has no observed construction") Right (construction s)
    let tailBytes = B.drop (B.length copied) (input s)
    unless (destination' == source j && B.take (B.length copied) (input s) == copied
      && B.length copied <= B.length (input s) && B.length tailBytes <= B.length beforeTail
      && tailBytes == B.take (B.length tailBytes) beforeTail && B.all (== 0) tailBytes
      && B.take 2 copied == B.pack [255,216] && B.drop (B.length copied-2) copied == B.pack [255,217])
      (Left "programmed source does not equal the observed EOI-trimmed copy and preserved zero tail")
    offset <- uniqueOffset copied
    pure (object ["kind" .= ("observed EOI-trimmed fixture copy with preserved pre-copy zero tail" :: String)
      , "fixture_offset" .= offset, "fixture_bytes" .= B.length copied, "programmed_bytes" .= B.length (input s)
      , "whole_span_exact" .= False, "preserved_tail_bytes" .= B.length tailBytes, "preserved_tail_hex" .= concatMap byteHex (B.unpack tailBytes)])
  where
    uniqueOffset :: B.ByteString -> Either String Int
    uniqueOffset bytes =
      let (prefix,suffix) = B.breakSubstring bytes fixture
      in if B.null bytes || B.null suffix || not (B.null (snd (B.breakSubstring bytes (B.drop 1 suffix))))
           then Left "fixture source subrange is absent or ambiguous" else Right (B.length prefix)
    byteHex byte = let digits = "0123456789abcdef" in [digits !! fromIntegral (byte `div` 16), digits !! fromIntegral (byte `mod` 16)]

job :: Snapshot -> Either String Job
job s = do
  let p = parameters s
      r = registers s
      get = word p
      n = fromIntegral (get 12)
      cap = fromIntegral (get 0x1c)
      shift = get 0x24
      range address size = toInteger (physical address) + toInteger size <= 0x20000000
  unless (shift <= 3 && get 0x20 == 1 && all ((== 0) . get) [0x28,0x2c,0x30,0x34,0x38,0x3c,0x40,0x44])
    (Left "unsupported decode scaling, packing or window")
  unless (n > 0 && n <= 16 * 1024 * 1024 && cap > 0)
    (Left "decode source or allocation outside observation bound")
  unless (range (get 8) n && range (get 16) cap) (Left "decode range leaves physical DRAM")
  unless (word r 0 == get 16 && word r 4 == get 0x1c && word r 8 == get 8 && word r 12 == get 12)
    (Left "programmed source, destination or capacity differs from the job")
  pure (Job (get 0) (Address (physical (get 8))) (Address (physical (get 16))) (ByteCount n) (ByteCount cap) (fromIntegral shift))

sourceRange :: Snapshot -> Either String ByteRange
sourceRange s = do
  j <- job s
  let Address address = source j
      ByteCount size = count j
  pure (ByteRange address size)

jpegSize :: B.ByteString -> Either String (Int, Int)
jpegSize bytes = do
  unless (B.take 2 bytes == B.pack [255,216]) (Left "unsupported JPEG stream header")
  marker 2
  where
    byte i = fromIntegral (B.index bytes i) :: Int
    big i = byte i * 256 + byte (i+1)
    marker at
      | at + 4 > B.length bytes || byte at /= 255 = Left "truncated JPEG header"
      | byte (at+1) == 255 = marker (at+1)
      | byte (at+1) `elem` [0xc0,0xc1,0xc2] = do
          unless (at + 10 <= B.length bytes && big (at+2) >= 8 && byte (at+4) == 8 && byte (at+9) `elem` [1,3])
            (Left "unsupported JPEG sample precision or components")
          let w = big (at+7); h = big (at+5)
          unless (w > 0 && h > 0 && w <= 8192 && h <= 8192 && toInteger w * toInteger h <= 32000000)
            (Left "unsupported JPEG dimensions")
          pure (w,h)
      | byte (at+1) `elem` [0xd8,0xd9,0xda] || big (at+2) < 2 = Left "JPEG frame header is absent"
      | otherwise = marker (at + 2 + big (at+2))

outputRange :: Snapshot -> Either String ByteRange
outputRange s = do
  j <- job s
  (sw,sh) <- jpegSize (input s)
  let w = sw `shiftR` scale j; h = sh `shiftR` scale j
      written = (((w+15) `div` 16) * 16) * (((h+7) `div` 8) * 8) * 2
      ByteCount cap = capacity j
      Address address = destination j
  unless (w >= 2 && even w && h > 0) (Left "unsupported scaled JPEG dimensions")
  unless (written <= cap) (Left "decoded extent exceeds allocation")
  pure (ByteRange address written)

completionBinding :: Snapshot -> OutputEvent -> Either String CompletionBinding
completionBinding completed event = do
  j <- job completed
  planned <- outputRange completed
  (sw,sh) <- jpegSize (input completed)
  let contentWidth = sw `shiftR` scale j
      contentHeight = sh `shiftR` scale j
      outputPitch = ((contentWidth+15) `div` 16) * 16
      padded = ((contentHeight+7) `div` 8) * 8
      g = Geometry contentWidth contentHeight outputPitch padded (rangeCount planned)
  unless (owner j /= 0 && owner j .&. 3 == 0 && toInteger (physical (owner j)) + 0xb0 <= 0x20000000)
    (Left "completion request address leaves the observed ABI")
  unless (B.length (output completed) == extent g
    && fmap physical (endpoint completed) == Just (rangeAddress planned + fromIntegral (extent g)))
    (Left "completion binding lacks its measured output and endpoint")
  pure (CompletionBinding j g (B.drop 8 (parameters completed)) event)

completionRequestRange :: CompletionBinding -> ByteRange
completionRequestRange (CompletionBinding j _ _ _) = ByteRange (physical (owner j)) 0xb0

inspectRelease :: CompletionBinding -> B.ByteString -> B.ByteString -> Either String Value
inspectRelease (CompletionBinding j g expectedConfiguration (OutputEvent outputEvent)) shared request = do
  unless (B.length shared == 0x210 && B.length request == 0xb0) (Left "release snapshot layout differs")
  unless (word shared 4 == 0 && word shared 0x19c == 0) (Left "release retains a resource or interrupt token")
  unless (word shared 0x198 == owner j && B.drop 0x1a0 shared == expectedConfiguration)
    (Left "release belongs to another request or changed configuration")
  unless (and [word request requestOffset == word expectedConfiguration configurationOffset
    | (requestOffset,configurationOffset) <- [(0x18,0),(0x1c,4),(0x20,8),(8,12),(12,16)]])
    (Left "release source, destination or image requirements changed")
  unless (word request 0x10 == fromIntegral (pitch g) && word request 0x14 == fromIntegral (rows g))
    (Left "release output geometry differs from the sampled output")
  unless (word request 0x28 == 0 && word request 0x24 == fromIntegral (extent g)
    && word request 0x2c == 0 && word request 0x30 == outputEvent)
    (Left "release completion result differs from the measured device output")
  pure (object ["job" .= owner j, "produced_bytes" .= extent g, "output_width" .= pitch g, "output_height" .= rows g
    , "status" .= word request 0x28, "input_event" .= word request 0x2c, "output_event" .= word request 0x30
    , "resource_handle" .= word shared 4, "irq_token" .= word shared 0x19c])

inspect :: B.ByteString -> Snapshot -> Snapshot -> Either String Value
inspect fixture started completed = do
  a <- job started
  b <- job completed
  case construction started of
    Just (SourceCopy address _ _) -> unless (address == source a) (Left "preview copy belongs to another destination")
    Nothing -> pure ()
  let ByteCount inputCount = count a
  unless (B.length (input started) == inputCount && a == b && parameters started == parameters completed && input started == input completed)
    (Left "decode job or source changed before its completion")
  let bytes = input started
  identity <- sourceIdentity fixture a started
  planned <- outputRange started
  unless (B.length (output completed) == rangeCount planned) (Left "snapshot output byte range differs from decoded extent")
  unless (B.take 2 bytes == B.pack [255,216]) (Left "source encoding requires unsupported header normalization")
  expectedSize <- jpegSize bytes
  dynamic <- decodeJpeg bytes
  let image = convertRGB8 dynamic
      sw = imageWidth image
      sh = imageHeight image
  unless (expectedSize == (sw,sh)) (Left "independent JPEG dimensions disagree with header")
  let
      factor = 2 ^ scale a
      w = sw `div` factor
      h = sh `div` factor
      p = ((w + 15) `div` 16) * 16
      rr = ((h + 7) `div` 8) * 8
      g = Geometry w h p rr (p * rr * 2)
      ByteCount cap = capacity a
      Address dst = destination a
      Address src = source a
      ByteCount n = count a
  unless (sw <= 8192 && sh <= 8192 && toInteger sw * toInteger sh <= 32000000 && w >= 2 && even w && h > 0)
    (Left "unsupported JPEG dimensions")
  unless (extent g <= cap && toInteger dst + toInteger (extent g) <= 0x20000000)
    (Left "decoded extent exceeds capacity or physical DRAM")
  unless (toInteger src >= toInteger dst + toInteger (extent g) || toInteger dst >= toInteger src + toInteger n)
    (Left "decode input overlaps output")
  unless (fmap physical (endpoint completed) == Just (dst + fromIntegral (extent g)))
    (Left "completed physical endpoint differs from decoded extent")
  let actual = B.take (extent g) (output completed)
      padding = and [B.index actual (y * p * 2 + x) == 0 | y <- [0..rows g-1], x <- [0..pitch g*2-1], y >= height g || x >= width g*2]
  unless padding (Left "decoded row or column padding is nonzero")
  let rgb x y =
        let sums = foldl' (\(ar,ag,ab) (xx,yy) -> case pixelAt image xx yy of PixelRGB8 r gg bb -> (ar+fromIntegral r,ag+fromIntegral gg,ab+fromIntegral bb)) (0,0,0)
              [(xx,yy) | yy <- [y*factor..(y+1)*factor-1], xx <- [x*factor..(x+1)*factor-1]]
            area = factor * factor
            rounded v = (v + area `div` 2) `div` area
        in case sums of (r,gg,bb) -> ycc (rounded r) (rounded gg) (rounded bb)
      reference = B.pack [value | y <- [0..rr-1], x <- [0,2..p-2], value <-
        if y >= h || x >= w then [0,0,0,0] else
          let (ya,ua,va) = rgb x y; (yb,ub,vb) = rgb (x+1) y
          in [ya,yb,average ua ub,average va vb]]
      metrics positions =
        let step (!seen,!sumDelta,!maximumDelta) i =
              let delta = abs (fromIntegral (B.index actual i) - fromIntegral (B.index reference i)) :: Int
              in (seen+1,sumDelta+delta,max maximumDelta delta)
            (samples,total,peak) = foldl' step (0 :: Int,0 :: Int,0 :: Int)
              [ (y*p+x)*2+c | y <- [0..h-1], x <- [0,2..w-2], c <- positions ]
        in object ["samples" .= samples, "mean_absolute_difference" .= (fromIntegral total / fromIntegral samples :: Double), "maximum_absolute_difference" .= peak]
      (_,different) = B.foldl' (\(!i,!n') value -> (i+1,n' + if value == B.index reference i then 0 else 1)) (0 :: Int,0 :: Int) actual
      witness = take 16 [object ["byte_offset" .= i, "actual" .= B.index actual i, "reference" .= B.index reference i]
                        | i <- [0..B.length actual-1], B.index actual i /= B.index reference i]
  pure (object ["job" .= owner a, "source_physical" .= src, "source_bytes" .= n
    , "source_identity" .= identity, "destination_physical" .= dst
    , "source_width" .= sw, "source_height" .= sh, "shift" .= scale a
    , "width" .= w, "height" .= h, "pitch_pixels" .= p, "padded_rows" .= rr, "row_bytes" .= (p*2)
    , "written_bytes" .= extent g, "capacity_bytes" .= cap, "padding_zero" .= padding
    , "reference_decoder" .= ("JuicyPixels JPEG to RGB8; declared RGB box and packed422 conversion" :: String)
    , "exact_match" .= (actual == reference), "different_bytes" .= different, "first_differences" .= witness
    , "y" .= metrics [0,1], "cb" .= metrics [2], "cr" .= metrics [3]
    , "content_scope" .= ("exact-match/difference observation, not a tolerance or physical pixel agreement gate" :: String)])

ycc :: Int -> Int -> Int -> (Word8, Word8, Word8)
ycc r g b = (clip ((19595*r+38470*g+7471*b+32768) `shiftR` 16), clip (((-11059*r-21709*g+32768*b+32768) `shiftR` 16)+128), clip (((32768*r-27439*g-5329*b+32768) `shiftR` 16)+128))
  where clip = fromIntegral . max 0 . min 255

average :: Word8 -> Word8 -> Word8
average a b = fromIntegral ((fromIntegral a + fromIntegral b + 1 :: Int) `div` 2)
