module Replacement.PropertyService.RetainedScalar
  ( ScalarProof
  , deriveInstructionProof
  , scalarProvesNonPointer
  ) where

import Data.Bits ((.&.), shiftR)
import qualified Data.Set as S
import Data.Word (Word32)
import Replacement.PropertyService.Classify (GuestAddress (..))
import ServiceInventory (Image (..), imageWord)

-- Only the original numerical WordInto match is covered by this token.
newtype ScalarProof = ScalarProof [(GuestAddress, Word32, RsbRole)]

data RsbRole
  = TimerChannelStride
  | XmlRemainingCount
  | Logarithm64Difference
  | Logarithm32Difference
  | HorizontalPixelAlignment
  | VerticalPixelAlignment

-- Every word in the half-open retained function extent is bound below.
data FunctionBinding = FunctionBinding
  { bindingImageBase :: Word32
  , bindingEntry :: GuestAddress
  , bindingWords :: [Word32]
  , bindingSites :: [(GuestAddress, RsbRole)]
  }

deriveInstructionProof :: Image -> Maybe ScalarProof
deriveInstructionProof image = case concatMap derive bindings of
  [] -> Nothing
  sites -> Just (ScalarProof sites)
  where
    derive binding
      | imageBase image /= bindingImageBase binding = []
      | not (and [imageWord image address == Just expected | (address, expected) <- addressedWords binding]) = []
      | otherwise =
          [ (site, word, role)
          | (site@(GuestAddress address), role) <- bindingSites binding
          , S.member address (reachable image binding)
          , Just word <- [imageWord image address]
          , word == roleEncoding role
          ]

scalarProvesNonPointer :: ScalarProof -> GuestAddress -> Word32 -> Bool
scalarProvesNonPointer (ScalarProof sites) address original =
  any (\(site, word, _) -> site == address && word == original) sites

roleEncoding :: RsbRole -> Word32
roleEncoding role = case role of
  TimerChannelStride -> 0x00621182
  XmlRemainingCount -> 0x00626006
  Logarithm64Difference -> 0x00630000
  Logarithm32Difference -> 0x00630000
  HorizontalPixelAlignment -> 0x00623003
  VerticalPixelAlignment -> 0x00623003

addressedWords :: FunctionBinding -> [(Word32, Word32)]
addressedWords binding = zip [entry, entry + 4 ..] (bindingWords binding)
  where
    GuestAddress entry = bindingEntry binding

-- CFG edges are direct B targets, call returns, conditional-return fallthrough
-- and ordinary fallthrough. These five bound extents contain no indirect jumps.
reachable :: Image -> FunctionBinding -> S.Set Word32
reachable image binding = visit S.empty [entry]
  where
    GuestAddress entry = bindingEntry binding
    end = toInteger entry + 4 * toInteger (length (bindingWords binding))
    inside address = address >= entry && toInteger address < end && address .&. 3 == 0
    visit seen [] = seen
    visit seen (address : pending)
      | not (inside address) || S.member address seen = visit seen pending
      | otherwise = case imageWord image address of
          Nothing -> visit seen pending
          Just word -> visit (S.insert address seen) (successors address word ++ pending)
    successors address word
      | word .&. 0x0f000000 == 0x0a000000 =
          branchDestination address word : [address + 4 | conditional word]
      | word .&. 0x0e108000 == 0x08108000 = [address + 4 | conditional word]
      | otherwise = [address + 4]
    conditional word = word `shiftR` 28 /= 14
    branchDestination address word =
      let encoded = toInteger (word .&. 0x00ffffff)
          displacement = if encoded >= 0x800000 then encoded - 0x1000000 else encoded
      in fromInteger (toInteger address + 8 + displacement * 4)

bindings :: [FunctionBinding]
bindings = [timerToggle, xmlSelect, logarithm64, logarithm32, alignedPixel]

-- x70view 55436fc85c0560dd252b0410af6f2427fe198cd3, xm1-150:
-- app/drivers/gpio/output_timer_recovered.c, output_timer_toggle.
-- id4.dis [6a4918,6a4a1c): 6a49ac computes 7 * channel before the
-- 6a49b0 scale by four selects a typed output_timer_anchor channel.
timerToggle :: FunctionBinding
timerToggle = FunctionBinding 0x61c400 (GuestAddress 0x6a4918)
  [ 0xe92d4818, 0xe28db00c, 0xe1a04000, 0xe3500010, 0x88bd8818, 0xe30936e8, 0xe340307c, 0xe0602180
  , 0xe7932182, 0xe3520007, 0x08bd8818, 0xe3063560, 0xe3403091, 0xe0621182, 0xe0833101, 0xe5933008
  , 0xe3530000, 0x08bd8818, 0xe3063560, 0xe3403091, 0xe0833101, 0xe5931010, 0xe5933014, 0xe1510003
  , 0x0a00001b, 0xe3063560, 0xe3403091, 0xe0621182, 0xe0833101, 0xe5933018, 0xe3530000, 0xe3063560
  , 0xe3403091, 0x10833101, 0x13a01000, 0x15831018, 0x15930010, 0x00621182, 0x00833101, 0x03a01001
  , 0x05831018, 0x05930014, 0xe308e538, 0xe340e082, 0xe062c182, 0xe1a0c10c, 0xe28cc008, 0xe3063560
  , 0xe3403091, 0xe3a01000, 0xe79e2102, 0xe08c3003, 0xebffda4e, 0xe0644184, 0xe30936e8, 0xe340307c
  , 0xe0834184, 0xe1a00004, 0xebffff2e, 0xe3500001, 0xe1a00004, 0x03a01000, 0x13a01001, 0xebffff5b
  , 0xe8bd8818
  ]
  [(GuestAddress 0x6a49ac, TimerChannelStride)]

-- Same pin, app/connectivity/ptp/ptp_transfer.c, ptp_xml_select_element.
-- id5.dis [d96f60,d96fbc): d96fac subtracts the current count at
-- ptp_trace + 4 from the saved count; d96fb0 writes the resulting count.
xmlSelect :: FunctionBinding
xmlSelect = FunctionBinding 0xbf1a00 (GuestAddress 0xd96f60)
  [ 0xe92d4878, 0xe28db014, 0xe1a04000, 0xe2800001, 0xebffff75, 0xe3500000, 0x13e00000, 0x18bd8878
  , 0xe30a3f60, 0xe3403146, 0xe5935010, 0xe5936004, 0xe1a00004, 0xebffffba, 0xe3500000, 0x030a3f60
  , 0x03403146, 0x05835010, 0x05932004, 0x00626006, 0x05836004, 0x13e00000, 0xe8bd8878
  ]
  [(GuestAddress 0xd96fac, XmlRemainingCount)]

-- Same pin, app/storage/dcf/dcf_numeric_operations.c.
-- id5.dis [ee9998,ee99f0): ee99e0 subtracts the two int logarithm
-- outputs at r11 - 20 and r11 - 16 on the successful-call path.
logarithm64 :: FunctionBinding
logarithm64 = FunctionBinding 0xbf1a00 (GuestAddress 0xee9998)
  [ 0xe92d4830, 0xe28db00c, 0xe24dd008, 0xe1a04002, 0xe1a05003, 0xe24b2010, 0xebffff8e, 0xe3500000
  , 0x130f0c18, 0x134f0fff, 0x1a000008, 0xe1a00004, 0xe1a01005, 0xe24b2014, 0xebffff86, 0xe3500000
  , 0x051b3014, 0x051b0010, 0x00630000, 0x13a00ffa, 0xe24bd00c, 0xe8bd8830
  ]
  [(GuestAddress 0xee99e0, Logarithm64Difference)]

-- id5.dis [ee99f0,ee9a44): ee9a2c subtracts the two short logarithm
-- outputs at r11 - 16 and r11 - 14; ee9a30 truncates to unsigned short.
logarithm32 :: FunctionBinding
logarithm32 = FunctionBinding 0xbf1a00 (GuestAddress 0xee99f0)
  [ 0xe92d4810, 0xe28db008, 0xe24dd00c, 0xe1a04001, 0xe24b100e, 0xebffffa5, 0xe3500000, 0x130f0c18
  , 0x1a000008, 0xe1a00004, 0xe24b1010, 0xebffff9f, 0xe3500000, 0x015b00be, 0x015b31b0, 0x00630000
  , 0x06ff0070, 0x13a00ffa, 0xe6bf0070, 0xe24bd008, 0xe8bd8810
  ]
  [(GuestAddress 0xee9a2c, Logarithm32Difference)]

-- Same pin, app/ui/render/ov44_overlay_drawing_api_rest.c,
-- overlay_draw_aligned_pixel_resource (function-ranges name FUN_0111262c).
-- plain.dis [111262c,111278c): 11126e0/1112764 subtract resource_extent
-- width/height halfwords from the x/y coordinates for alignment mode 3.
alignedPixel :: FunctionBinding
alignedPixel = FunctionBinding 0x1100000 (GuestAddress 0x111262c)
  [ 0xe92d4ff0, 0xe28db020, 0xe24dd00c, 0xe1a06000, 0xe1a04001, 0xe1a07002, 0xe1a08003, 0xe59ba004
  , 0xe59b500c, 0xe3550000, 0x0a00004a, 0xe1a00005, 0xe24b1028, 0xebffffed, 0xe300383d, 0xe1550003
  , 0x83a09000, 0x93a09001, 0xe1550003, 0x935a0000, 0x1a000007, 0xe3760014, 0x13a04001, 0x1a000018
  , 0xe1a00005, 0xeb01ad5c, 0xe1a06000, 0xe3a04001, 0xea000013, 0xe6ff3076, 0xe0634004, 0xe6ff4074
  , 0xe35a0002, 0x1a000008, 0xe15b22f8, 0xe0822fa2, 0xe04330c2, 0xe6bf4074, 0xe0844fa4, 0xe08360c4
  , 0xe6bf6076, 0xe3a04000, 0xea000005, 0xe35a0003, 0x015b22b8, 0x00623003, 0x00844003, 0x06bf6074
  , 0xe3a04000, 0xe59b2008, 0xe3520000, 0x13a09000, 0x02099001, 0xe3590000, 0x0a000005, 0xe3770014
  , 0x1a000017, 0xe1a00005, 0xeb01ad56, 0xe1a07000, 0xea000013, 0xe6ff3077, 0xe0638008, 0xe6ff8078
  , 0xe59b2008, 0xe3520002, 0x1a000007, 0xe15b22f6, 0xe0822fa2, 0xe04330c2, 0xe6bf8078, 0xe0888fa8
  , 0xe08370c8, 0xe6bf7077, 0xea000005, 0xe59b2008, 0xe3520003, 0x015b22b6, 0x00623003, 0x00888003
  , 0x06bf7078, 0xe1a00006, 0xe1a01007, 0xe1a02005, 0xe1a03004, 0xebfffefe, 0xe24bd020, 0xe8bd8ff0
  ]
  [ (GuestAddress 0x11126e0, HorizontalPixelAlignment)
  , (GuestAddress 0x1112764, VerticalPixelAlignment)
  ]
