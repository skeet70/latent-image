-- | Which bytes of a posted file-service request payload the stock poster defines.
module Replacement.FileService.Payload (payloadDefined) where

import Data.Word (Word32, Word8)

-- | Fields a stock poster leaves partly undefined, as half-open payload offsets: strings
-- copied with strcpy (0x70e748) into an uninitialized stack payload, undefined after the
-- terminator, then spans the poster never stores.
data Layout = Layout [(Int, Int)] [(Int, Int)]

-- | Open 0x71c2b0: path +4..+0x34. Close 0x71c134: name +0xc..+0x20. Attribute set 0x70f55c
-- and 0x70f5f8 (0x2c04): path +4..+0x34, and +0x39..+0x3c behind the selector byte +0x38.
layout :: Word32 -> Layout
layout code
  | code `elem` [0x2c17, 0x2c2d] = Layout [(4, 0x34)] []
  | code `elem` [0x2c1a, 0x2c30] = Layout [(0xc, 0x20)] []
  | code == 0x2c04 = Layout [(4, 0x34)] [(0x39, 0x3c)]
  | otherwise = Layout [] []

-- | The defined flags of a request's payload bytes. Bytes at or past the payload length
-- belong to no request.
payloadDefined :: Word32 -> Int -> [Word8] -> [Bool]
payloadDefined code len bytes = map defined [0 .. length bytes - 1]
  where
    Layout strings unstored = layout code
    defined o
      | o >= len = False
      | any (within o) unstored = False
      | (s, _) : _ <- filter (within o) strings = notElem 0 (take (o - s) (drop s bytes))
      | otherwise = True
    within o (s, e) = s <= o && o < e
