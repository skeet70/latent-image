module Fixtures (goodCarrier, compactCarrier, spillCarrier, mutate, symbolsFor, wordsFor, callAt) where

import Data.Bits ((.&.), (.|.))
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import Data.Word (Word32)
import qualified FujiAF.UpdateContainer as U
import Numeric (showHex)

-- Original synthetic A32 fixtures, not copied firmware bytes.
wordsFor :: [Word32] -> B.ByteString
wordsFor = B.concat . map U.putWord32

callAt :: Int -> Word32 -> Word32
callAt index target = 0xeb000000 .|. (fromInteger ((toInteger target - toInteger site - 8) `div` 4) .&. 0xffffff)
  where site = 0xbfbd20 + fromIntegral (index*4) :: Word32

goodCarrier :: B.ByteString
goodCarrier = wordsFor
  [0xe92d4c10,0xe28db008,callAt 2 0xbfb1b8,callAt 3 0xbfb20c
  ,0xe1a04000,callAt 5 0xbfb1e0,0xe1a00004,0xe8bd8c10]

compactCarrier :: B.ByteString
compactCarrier = wordsFor
  [0xe92d4010,callAt 1 0xbfb1b8,callAt 2 0xbfb20c,0xe1a04000
  ,callAt 4 0xbfb1e0,0xe1a00004,0xe8bd8010]

spillCarrier :: B.ByteString
spillCarrier = wordsFor
  [0xe92d4001,callAt 1 0xbfb1b8,callAt 2 0xbfb20c,0xe58d0000
  ,callAt 4 0xbfb1e0,0xe8bd8001]

mutate :: Int -> Word32 -> B.ByteString -> B.ByteString
mutate index word bytes = B.take (index*4) bytes <> U.putWord32 word <> B.drop (index*4+4) bytes

symbolsFor :: B.ByteString -> B.ByteString
symbolsFor bytes = C.pack (concat
  [ row 0xbfbd20 0 "T" "__application_pump_start"
  , row (0xbfbd20 + fromIntegral (B.length bytes)) 0 "T" "__application_pump_end"
  , row 0xbfbd20 (fromIntegral (B.length bytes)) "T" "xm1_application_pump"
  , row 0xbfb1b8 0 "A" "xm1_application_prehook"
  , row 0xbfb20c 0 "A" "xm1_application_normalizer"
  , row 0xbfb1e0 0 "A" "xm1_application_posthook"
  , row 0xbfbd20 0 "t" "$a.0"
  ])
  where
    row :: Word32 -> Word32 -> String -> String -> String
    row address size kind name = showHex address "" ++ " " ++ showHex size "" ++ " " ++ kind ++ " " ++ name ++ "\n"
