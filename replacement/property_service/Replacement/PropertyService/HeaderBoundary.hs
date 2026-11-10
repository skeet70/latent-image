{-# LANGUAGE GHC2021 #-}
module Replacement.PropertyService.HeaderBoundary where

import Data.Bits ((.&.), (.|.), shiftR, complement)
import qualified Data.ByteString as B
import Data.Word (Word16, Word32)
import Emulation.OwnedCase (assemble, word32le)

newtype BackupKey = BackupKey Word16 deriving (Eq, Show)
data BoundaryEvent = BoundaryEvent Word32 Word32 Word32 Word32 deriving (Eq, Show)

keysBase, backingBase, callsBase, helperBase :: Word32
keysBase = 0x7cb7d4
backingBase = 0x409c281c
callsBase = 0x958000
helperBase = 0x580000

callsBytes :: Int
callsBytes = 0x800

keysFromImage :: B.ByteString -> [BackupKey]
keysFromImage image = [BackupKey (fromIntegral (B.index bytes offset) + 256 * fromIntegral (B.index bytes (offset+1))) | offset <- [0,2..B.length bytes-2]]
  where bytes = B.take 32 (B.drop (0x7cb7d4-0x61c400) image)

keyBytes :: [BackupKey] -> B.ByteString
keyBytes = B.pack . concatMap (\(BackupKey key) -> [fromIntegral key,fromIntegral (key `shiftR` 8)])

initialBacking :: B.ByteString -> B.ByteString
initialBacking banks = B.take 0x19a banks <> B.pack [0x6d,0x7e] <> B.map complement (B.take 16 banks)

sealedHeader :: B.ByteString -> B.ByteString
sealedHeader header = B.take 15 header <> B.singleton (negate (B.foldl' (+) 0 (B.take 15 header)))

requestEvents :: [BoundaryEvent] -> [BoundaryEvent]
requestEvents middle = BoundaryEvent 0x724b68 0x25 0xffffffff 0 : middle ++ [BoundaryEvent 0x724c00 0x25 0 0]

encodeEvents :: [BoundaryEvent] -> B.ByteString
encodeEvents events = word32le (fromIntegral (length events)) <> B.concat [B.concat (map word32le [site,a,b,c]) | BoundaryEvent site a b c <- events] <> B.replicate (callsBytes-4-16*length events) 0

mov :: Int -> Word32 -> [Word32]
mov register value = [0xe3000000 .|. fromIntegral register * 0x1000 .|. ((value .&. 0xf000) * 16) .|. (value .&. 0xfff),0xe3400000 .|. fromIntegral register * 0x1000 .|. (((value `shiftR` 16) .&. 0xf000) * 16) .|. ((value `shiftR` 16) .&. 0xfff)]

branch :: Word32 -> Word32 -> Word32
branch site target = 0xea000000 .|. ((target-site-8) `shiftR` 2 .&. 0xffffff)

callback :: Word32 -> [Word32]
callback address = prefix ++ mov 12 address ++ [branch (address+fromIntegral (4*(length prefix+2))) helperBase]
  where
    prefix
      | address == 0x6a59b8 = mov 3 0x409c29b8 ++ [0xe240c00f,0xe7d3300c,0xe1822403] ++ mov 12 0x96cd8a ++ [0xe5dcc000,0xe182280c]
      | address == 0x724b68 = [0xe3a02000]
      | address == 0x724c00 = [0xe3a01000,0xe3a02000]
      | address == 0x726c74 = [0xe3a02000]
      | address == 0x633594 = [0xe3a01000,0xe3a02000]
      | otherwise = []

callbacks :: [Word32]
callbacks = [0x6a59b8,0x724b68,0x724c00,0x726c74,0x633594]

helperWords :: [Word32]
helperWords = [0xe92d4038] ++ mov 3 callsBase ++ [0xe5934000,0xe2835004,0xe0855204,0xe585c000,0xe5850004,0xe5851008,0xe585200c,0xe2844001,0xe5834000,0xe3a00000,0xe8bd8038]

copyWords :: [Word32]
copyWords = [0xe3520000,0x012fff1e,0xe4d13001,0xe4c03001,0xe2522001,0x1afffffb,0xe12fff1e]

patches :: [(Word32,[Word32])]
patches = [(address,callback address) | address <- callbacks] ++ [(0x722054,copyWords),(0x723cb8,[0xe3a00001,0xe12fff1e])]

codeWindow :: B.ByteString -> Word32 -> Int -> B.ByteString
codeWindow image low size = foldl place original [(address,words') | (address,words') <- patches,address >= low,toInteger address + 4 * toInteger (length words') <= toInteger low + toInteger size]
  where
    original = B.take size (B.drop (fromIntegral (low-0x61c400)) image)
    place bytes (address,words') = let at = fromIntegral (address-low); value = assemble words' in B.take at bytes <> value <> B.drop (at+B.length value) bytes

functionSpans :: [(Word32,Word32)]
functionSpans = [(0x61d944,0x61d994),(0x61d994,0x61d9ec),(0x61d9ec,0x61da3c),(0x61da3c,0x61da8c),(0x6a6ddc,0x6a6e68),(0x6a6e68,0x6a6f3c),(0x6a6f3c,0x6a6fd8),(0x72b3a4,0x72b3d0),(0x72b3d0,0x72b3fc),(0x722054,0x722070),(0x723cb8,0x723cc0)] ++ [(address,address+fromIntegral (length (callback address)*4)) | address <- callbacks] ++ [(helperBase,helperBase+fromIntegral (length helperWords*4))]
