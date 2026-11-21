{-# LANGUAGE GHC2021 #-}

-- | Image4 table bytes in [0x7c2dfc, 0x7da0d0) that the property scan reads as words into retired
-- property code. Only the halfword-pair gap between the LCD record table and the HDMI handler table
-- closes; the imaging-configuration records and the sound clips stay retained.
module Replacement.PropertyService.RetainedImage4EarlyTables
  ( image4EarlyTableClosures
  ) where

import Control.Monad (guard)
import Data.Bits (shiftL, (.|.))
import qualified Data.ByteString as B
import Data.Maybe (isJust)
import Data.Word (Word32)
import qualified Emulation.Sha256 as SHA
import Replacement.PropertyService.Placement (GuestAddress (..), ScanImage (..))
import Replacement.PropertyService.RetainedClosure
import ServiceInventory (Image (..), imageWord)

-- | Halfword pairs in [0x7cf710, 0x7cfb10), between the LCD record table and the HDMI handler table.
data EarlyTableRole = LcdHdmiGapPair

data GapSite = GapSite GuestAddress EarlyTableRole

image4EarlyTableClosures :: [(ScanImage, Image)] -> [ClosedSite]
image4EarlyTableClosures images = case imageFor ScanImage4 images of
  Nothing -> []
  Just image
    | neighboursBound image ->
        [ ClosedSite ScanImage4 site word UnreferencedData
        | GapSite site@(GuestAddress address) role <- gapSites
        , address >= gapStart && address + 4 <= gapEnd
        , Just word <- [imageWord image address]
        , word == roleWord role
        ]
    | otherwise -> []

roleWord :: EarlyTableRole -> Word32
roleWord LcdHdmiGapPair = 0x00620006

gapSites :: [GapSite]
gapSites = [GapSite (GuestAddress 0x7cfac0) LcdHdmiGapPair]

-- | Neither the code images nor image2/image3 hold a movw/movt pair, pc-relative form or data word
-- naming an address in the gap.
gapStart, gapEnd :: Word32
gapStart = 0x7cf710
gapEnd = 0x7cfb10

neighboursBound :: Image -> Bool
neighboursBound image =
  extentMatches image (GuestAddress 0x7cf488) (GuestAddress gapStart) (digest "c5b541b5baf0b01c1903f1760ec149fca725d989a83190b76cfe6cc59ec72717")
    && extentMatches image (GuestAddress gapStart) (GuestAddress gapEnd) (digest "01608ee2862a1fd8ebd060faec8ae334de23da41e7f01da23afb56fb8bdfee15")
    && extentMatches image (GuestAddress gapEnd) (GuestAddress hdmiTableEnd) (digest "be31e64277a1e3efd651c5c398d15cb875ed85e2fe9ea32b3bbf07a9f6a98925")
    && all (isJust . reachableInstructions image) [lcdRecordsByPointer, lcdRecordsByIndex, hdmiTask]
    && hdmiKeyCount image == Just 6

-- | hdmi_task's key loop reads the halfword at 0x7cfb10 + 36 * n for n = 1.. and stops at a zero key;
-- the result is that n, the largest record index the handler read uses.
hdmiKeyCount :: Image -> Maybe Int
hdmiKeyCount image = go 1
  where
    go n = do
      let address = gapEnd + 36 * fromIntegral n
      guard (address + 2 <= hdmiTableEnd)
      key <- halfword image address
      if key == 0 then pure n else go (n + 1)

hdmiTableEnd :: Word32
hdmiTableEnd = 0x7cfc0c

halfword :: Image -> Word32 -> Maybe Word32
halfword image address = do
  guard (address >= imageBase image)
  let offset = fromIntegral (address - imageBase image)
  guard (offset + 2 <= B.length (imageBytes image))
  pure (fromIntegral (B.index (imageBytes image) offset) .|. (fromIntegral (B.index (imageBytes image) (offset + 1)) `shiftL` 8))

digest :: String -> SHA.Sha256
digest = either (error . ("retained image4 early table digest: " ++)) id . SHA.parseSha256Hex

-- x70view 55436fc85c0560dd252b0410af6f2427fe198cd3, app/drivers/lcd/tu_006d3768.c, FUN_006d3f38
-- (NONMATCHING). id4.dis [6d3f38,6d4050): r4 runs from 0x7cf488 to 0x7cf708 (add r7, r4, #640) in
-- steps of 8, reading [r4+8], [r4+12] and [r4+14]; the last byte read is 0x7cf70e. The second loop
-- reads [0x7cf3ec, 0x7cf434).
lcdRecordsByPointer :: FunctionBinding
lcdRecordsByPointer = FunctionBinding 0x61c400 (GuestAddress 0x6d3f38)
  [ 0xe92d49f8, 0xe28db01c, 0xe30f4488, 0xe340407c, 0xe2847d0a, 0xe30f5fff, 0xe30b8d58, 0xe3408096
  , 0xe5946008, 0xe1560005, 0x1a000006, 0xe1d400bc, 0xe1500005, 0x05d4000e, 0x0a000000, 0xebff2e60
  , 0xebff1e91, 0xea000013, 0xe3560003, 0x1a000008, 0xe598324c, 0xe3530000, 0x03000eb8, 0x13000eb9
  , 0xebff2e57, 0xe1a01000, 0xe3a00003, 0xebffffae, 0xea000008, 0xe6ef6076, 0xe1d400bc, 0xe1500005
  , 0x05d4100e, 0x0a000001, 0xebff2e4d, 0xe1a01000, 0xe1a00006, 0xebffffa4, 0xe2844008, 0xe1540007
  , 0x1affffde, 0xe3a04000, 0xe30f53ec, 0xe340507c, 0xe30f6fff, 0xe7947005, 0xe1570006, 0x1a000007
  , 0xe0853004, 0xe1d300b4, 0xe1500006, 0x05d30006, 0x0a000000, 0xebff2e3a, 0xebff1e6b, 0xea000009
  , 0xe6ef7077, 0xe0853004, 0xe1d300b4, 0xe1500006, 0x05d31006, 0x0a000001, 0xebff2e31, 0xe1a01000
  , 0xe1a00007, 0xebffff88, 0xe2844008, 0xe3540048, 0x1affffe7, 0xe8bd89f8
  ]

-- Same file, FUN_006d4554 (NONMATCHING). id4.dis [6d4554,6d467c): r4 runs from 0 to 648 in steps of
-- 8, reading [0x7cf488+r4], [+4] and [+6]; the last byte read is 0x7cf70e. The second loop reads
-- [0x7cf3ec, 0x7cf434).
lcdRecordsByIndex :: FunctionBinding
lcdRecordsByIndex = FunctionBinding 0x61c400 (GuestAddress 0x6d4554)
  [ 0xe92d48f0, 0xe28db014, 0xe3a04000, 0xe30f5488, 0xe340507c, 0xe30b7d58, 0xe3407096, 0xe7946005
  , 0xe30f3fff, 0xe1560003, 0x1a000008, 0xe0853004, 0xe1d300b4, 0xe30f2fff, 0xe1500002, 0x05d30006
  , 0x0a000000, 0xebff2cd7, 0xebff1d08, 0xea000015, 0xe3560003, 0x1a000008, 0xe597324c, 0xe3530000
  , 0x03000eb8, 0x13000eb9, 0xebff2cce, 0xe1a01000, 0xe3a00003, 0xebfffe25, 0xea00000a, 0xe6ef6076
  , 0xe0853004, 0xe1d300b4, 0xe30f2fff, 0xe1500002, 0x05d31006, 0x0a000001, 0xebff2cc2, 0xe1a01000
  , 0xe1a00006, 0xebfffe19, 0xe2844008, 0xe3540fa2, 0x1affffd9, 0xe3a04000, 0xe30f53ec, 0xe340507c
  , 0xe30f6fff, 0xe7947005, 0xe1570006, 0x1a000007, 0xe0853004, 0xe1d300b4, 0xe1500006, 0x05d30006
  , 0x0a000000, 0xebff2caf, 0xebff1ce0, 0xea000009, 0xe6ef7077, 0xe0853004, 0xe1d300b4, 0xe1500006
  , 0x05d31006, 0x0a000001, 0xebff2ca6, 0xe1a01000, 0xe1a00007, 0xebfffdfd, 0xe2844008, 0xe3540048
  , 0x1affffe7, 0xe8bd88f0
  ]

-- Same pin, app/drivers/hdmi/tu_006d4900.c, hdmi_task (NONMATCHING). id4.dis [6d6c9c,6d6d9c), literal
-- 0x7cfb10 at 6d6d98: 36-byte records, keys read at 0x7cfb10 + 36 * n and handlers at
-- 0x7cfb10 + 36 * n + 4 * byte + 4 with n and byte unsigned, so no read falls below 0x7cfb10.
hdmiTask :: FunctionBinding
hdmiTask = FunctionBinding 0x61c400 (GuestAddress 0x6d6c9c)
  [ 0xe92d4830, 0xe28db00c, 0xe5905000, 0xe59f40e4, 0xe1a00004, 0xe3a01000, 0xe3a020ac, 0xeb012d42
  , 0xe5845094, 0xe284300c, 0xe5843008, 0xe3a05000, 0xe5845014, 0xe3a03002, 0xe5843020, 0xe5843024
  , 0xe584502c, 0xe584504c, 0xe3a0300c, 0xe5843018, 0xe3a0000b, 0xe284103c, 0xebfdcbd4, 0xe1a00005
  , 0xebfffa7d, 0xe3e03a76, 0xe5133feb, 0xebfffee8, 0xebffffce, 0xe59f5080, 0xe3a00045, 0xe1a01004
  , 0xe3e02000, 0xeb01397c, 0xe3500000, 0x1a000017, 0xe1d400b0, 0xe3500c55, 0x11a03005, 0x13a01000
  , 0x1a000003, 0xea000007, 0xe2833024, 0xe1520000, 0x0a000005, 0xe2811001, 0xe1d322b4, 0xe3520000
  , 0x1afffff8, 0xea000000, 0xe3a01000, 0xe5d43014, 0xe0811181, 0xe0811003, 0xe0851101, 0xe5913004
  , 0xe3530000, 0x0affffe3, 0xe12fff33, 0xeaffffe1, 0xebfff72a, 0xeaffffdf, 0x0093a760, 0x007cfb10
  ]
