-- The X-M1 1.50 hardware-compressed record format: the settled grammar
-- (reports/firmware-extraction.md, E-LZ-GRAMMAR) plus the header and feed
-- arithmetic read from the boot record and image 4 driver copies. The encoder
-- is FujiAF.Lz.Encode; the reference decoder is fflzd.c.
module FujiAF.Lz.Format
  ( Header (..)
  , headerSize
  , chunkSize
  , encodeHeader
  , parseHeader
  , Token (..)
  , Distance
  , MatchLen
  , mkDistance
  , mkMatchLen
  , distanceValue
  , matchLenValue
  , encodeToken
  , tokenize
  , tokenSize
  , verifyRecord
  , pageBoundaries
  , windowSize
  ) where

import Data.Bits ((.&.), (.|.), shiftL, shiftR)
import qualified Data.ByteString as B
import Data.Word (Word16, Word32, Word8)

-- The record header: five little-endian words. Word 3 is the engine feed size
-- and word 4 the mode byte; both stock records carry 0x4000 and 4.
data Header = Header
  { headerOutSize :: !Word32
  , headerTotalLen :: !Word32
  , headerPages :: !Word32
  , headerChunk :: !Word32
  , headerMode :: !Word32
  }
  deriving (Show, Eq)

headerSize :: Int
headerSize = 0x14

chunkSize :: Int
chunkSize = 0x4000

-- A back-reference distance. The grammar admits 1..2047; both stock records
-- use 3..2047 only, and the encoder emits 3..2047 (FujiAF.Lz.Encode).
newtype Distance = Distance Word16
  deriving (Show, Eq)

-- A back-reference length: 4 bits, no bias. Length 0 never occurs in the
-- stock records and is refused here.
newtype MatchLen = MatchLen Word8
  deriving (Show, Eq)

mkDistance :: Word16 -> Maybe Distance
mkDistance v
  | v >= 1 && v <= 2047 = Just (Distance v)
  | otherwise = Nothing

mkMatchLen :: Word8 -> Maybe MatchLen
mkMatchLen v
  | v >= 1 && v <= 15 = Just (MatchLen v)
  | otherwise = Nothing

distanceValue :: Distance -> Word16
distanceValue (Distance v) = v

matchLenValue :: MatchLen -> Word8
matchLenValue (MatchLen v) = v

-- A stream token: a literal run is 1 control byte (the length) plus that many
-- data bytes; a match is 2 bytes.
data Token
  = LiteralRun !Word8 !B.ByteString
  | Match !Distance !MatchLen
  deriving (Show, Eq)

encodeHeader :: Header -> B.ByteString
encodeHeader h =
  B.concat
    [ word (headerOutSize h)
    , word (headerTotalLen h)
    , word (headerPages h)
    , word (headerChunk h)
    , word (headerMode h)
    ]
  where
    word w =
      B.pack
        [ fromIntegral (w .&. 0xff)
        , fromIntegral ((w `shiftR` 8) .&. 0xff)
        , fromIntegral ((w `shiftR` 16) .&. 0xff)
        , fromIntegral ((w `shiftR` 24) .&. 0xff)
        ]

parseHeader :: B.ByteString -> Maybe Header
parseHeader bytes
  | B.length bytes < headerSize = Nothing
  | otherwise =
      Just
        Header
          { headerOutSize = word 0
          , headerTotalLen = word 4
          , headerPages = word 8
          , headerChunk = word 12
          , headerMode = word 16
          }
  where
    word i =
      fromIntegral (B.index bytes i)
        .|. (fromIntegral (B.index bytes (i + 1)) `shiftL` 8)
        .|. (fromIntegral (B.index bytes (i + 2)) `shiftL` 16)
        .|. (fromIntegral (B.index bytes (i + 3)) `shiftL` 24)

encodeToken :: Token -> B.ByteString
encodeToken (LiteralRun len bytes) = B.cons len bytes
encodeToken (Match distance len) =
  let v = (distanceValue distance `shiftL` 4) .|. fromIntegral (matchLenValue len)
   in B.pack [0x80 .|. fromIntegral ((v `shiftR` 8) .&. 0x7f), fromIntegral (v .&. 0xff)]

-- Segment a stream body into tokens, reporting each token's body offset. A
-- malformed body (control 0x00, zero distance or length, truncated token) is
-- reported; the strict decoding rules live in fflzd-codec.c.
tokenize :: B.ByteString -> Either String [(Int, Token)]
tokenize body = go 0
  where
    n = B.length body
    go ip
      | ip >= n = Right []
      | ip + 1 > n = Left "stream ends after a control byte"
      | t == 0 = Left "control byte 0x00"
      | t < 0x80 =
          if ip + 1 + fromIntegral t > n
            then Left "literal run overruns the stream"
            else
              ((ip, LiteralRun t (B.take (fromIntegral t) (B.drop (ip + 1) body))) :) <$> go (ip + 1 + fromIntegral t)
      | ip + 2 > n = Left "match token with no following byte"
      | otherwise =
          let v = ((fromIntegral t .&. 0x7f :: Int) `shiftL` 8) .|. fromIntegral (B.index body (ip + 1))
           in case (mkDistance (fromIntegral (v `shiftR` 4)), mkMatchLen (fromIntegral (v .&. 0xf))) of
                (Just d, Just l) -> ((ip, Match d l) :) <$> go (ip + 2)
                _ -> Left "match token with zero distance or length"
      where
        t = B.index body ip

-- The interior feed boundaries as record offsets: the loader kicks the engine
-- at offsets that are multiples of the header chunk word, and the engine is
-- fed exactly total_len - 0x14 stream bytes, so every token must end at each
-- of these offsets. The header's own bytes sit in the first page.
pageBoundaries :: Header -> [Int]
pageBoundaries h =
  [k * fromIntegral (headerChunk h) | k <- [1 .. fromIntegral (headerPages h) - 1]]

tokenSize :: Token -> Int
tokenSize (LiteralRun len _) = 1 + fromIntegral len
tokenSize (Match _ _) = 2

-- The invariants a record must satisfy before anything presents it as
-- shippable, over what tokenize already enforces (contiguous legal tokens, no
-- control 0x00, no zero distance or length): the header arithmetic the strict
-- decoder and both driver copies read, the stock token domain
-- (reports/firmware-extraction.md "The encoder": distance >= 3, length <=
-- distance, short matches only as feed-edge tails) and the feed-boundary rule.
-- Returns every violation, empty when the record is structurally sound.
verifyRecord :: B.ByteString -> [String]
verifyRecord record = case parseHeader record of
  Nothing -> ["unparseable record header"]
  Just h -> headerViolations h ++ tokenViolations h
  where
    stream = B.drop headerSize record
    headerViolations h =
      concat
        [ ["chunk word is not 0x4000" | headerChunk h /= fromIntegral chunkSize]
        , ["mode word is not 4" | headerMode h /= 4]
        , ["total_len is not the record length" | fromIntegral (headerTotalLen h) /= B.length record]
        , [ "page count is not ceil(total_len/chunk)"
          | headerPages h /= (headerTotalLen h + fromIntegral chunkSize - 1) `div` fromIntegral chunkSize
          ]
        ]
    tokenViolations h = case tokenize stream of
      Left message -> [message]
      Right tokens ->
        let entries = [(headerSize + s, t, tokenSize t) | (s, t) <- tokens]
            starts = scanl (+) headerSize [len | (_, _, len) <- entries]
            misaligned =
              [ "token at record offset " ++ show start ++ " does not continue the sequence"
              | ((start, _, _), scanned) <- zip entries starts
              , start /= scanned
              ]
            streamEnd = case [start + len | (start, _, len) <- entries] of
              [] -> headerSize
              reversedEnds -> case reverse reversedEnds of
                lastEnd : _ -> lastEnd
                [] -> headerSize
            tailGap =
              [ "token sequence ends at record offset " ++ show streamEnd ++ ", record length is " ++ show (B.length record)
              | streamEnd /= B.length record
              ]
            bounds = pageBoundaries h
            domain = concatMap atDomain entries
            atDomain (start, token, len) = case token of
              LiteralRun _ _ -> []
              Match d l ->
                let dv = fromIntegral (distanceValue d) :: Int
                    lv = fromIntegral (matchLenValue l) :: Int
                 in [ "match at record offset " ++ show start ++ " is outside the stock domain"
                    | dv < 3 || lv > dv
                    ]
                      ++ [ "short match at record offset " ++ show start ++ " away from a feed boundary"
                         | lv < 3
                         , not (any (\b -> start == b || start + len == b) bounds)
                         ]
            crossing =
              [ "token at record offset " ++ show start ++ " crosses feed boundary " ++ show b
              | (start, _, len) <- entries
              , b <- bounds
              , start < b
              , b < start + len
              ]
            unended =
              [ "no token ends at feed boundary " ++ show b
              | b <- bounds
              , not (any (\(start, _, len) -> start + len == b) entries)
              ]
         in misaligned ++ tailGap ++ domain ++ crossing ++ unended

-- The back-reference window: both stock records reach exactly this distance.
windowSize :: Int
windowSize = 2047
