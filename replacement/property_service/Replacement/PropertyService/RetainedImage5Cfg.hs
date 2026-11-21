{-# LANGUAGE GHC2021 #-}

module Replacement.PropertyService.RetainedImage5Cfg
  ( image5CfgClosures
  ) where

import qualified Data.Set as S
import Data.Word (Word32)
import qualified Emulation.Sha256 as SHA
import Replacement.PropertyService.Placement (GuestAddress (..), ScanImage (..))
import Replacement.PropertyService.RetainedClosure
import ServiceInventory (Image, imageWord)

data RsbRole
  = -- | e42dd0: remaining count minus the per-slot allocation when arg5 is 15 and arg6 is 0.
    QueueRemainingAfterSlot
  | -- | ebc6b0: completes signed (r7 << 4) / 9 for mode 5; ebc6b8 compares the quotient with r6.
    SixteenNinthsOfR7
  | -- | ebc9a8: completes signed (r4 << 4) / 9 for mode 5.
    SixteenNinthsOfR4

roleEncoding :: RsbRole -> Word32
roleEncoding role = case role of
  QueueRemainingAfterSlot -> 0x00621001
  SixteenNinthsOfR7 -> 0x006230c3
  SixteenNinthsOfR4 -> 0x006230c3

image5Base :: Word32
image5Base = 0xbf1a00

image5CfgClosures :: [(ScanImage, Image)] -> [ClosedSite]
image5CfgClosures images = case imageFor ScanImage5 images of
  Nothing -> []
  Just image -> queueSelection image ++ cropGeometry image

sites :: [(GuestAddress, RsbRole)] -> [(GuestAddress, Word32)]
sites bound = [(site, roleEncoding role) | (site, role) <- bound]

-- x70view 55436fc85c0560dd252b0410af6f2427fe198cd3, xm1-150:
-- app/capture/reccont/tu_00e42908.c, FUN_00e42c94 (NONMATCHING), called from 0xdd045c.
-- id5.dis [e42c94,e42ed4): the walk reaches e42dd0 through e42d24, e42d44, e42d7c and e42dc0.
queueSelection :: Image -> [ClosedSite]
queueSelection image =
  instructionSites ScanImage5 image (FunctionBinding image5Base (GuestAddress 0xe42c94) queueSelectionWords)
    (sites [(GuestAddress 0xe42dd0, QueueRemainingAfterSlot)])

queueSelectionWords :: [Word32]
queueSelectionWords =
  [ 0xe92d4ff0, 0xe28db020, 0xe24dd08c, 0xe1a09000, 0xe50b10ac, 0xe50b20a0, 0xe1a06003, 0xe59b7004
  , 0xe59ba008, 0xe3a01000, 0xe14b12b6, 0xe24b0098, 0xe3a02070, 0xebe37d3e, 0xe24b0098, 0xeb01f66e
  , 0xe24b0026, 0xeb024ca8, 0xe51b3058, 0xe3530001, 0x13a02000, 0x150b20a8, 0x1a000005, 0xe51b304c
  , 0xe2733001, 0x33a03000, 0xe3530001, 0x150b30a8, 0x0a000001, 0xe3a06000, 0xea000000, 0xe50b30a8
  , 0xe3053910, 0xe3403154, 0xe59330ec, 0xe3530000, 0x0a000054, 0xe3054910, 0xe3404154, 0xe3a05000
  , 0xe1a08005, 0xe50b40a4, 0xe1a01006, 0xe51b60ac, 0xea000004, 0xe5943104, 0xe2855004, 0xe2844018
  , 0xe3530000, 0x0a000047, 0xe7893005, 0xe3570001, 0x13570007, 0x1a000002, 0xe3570001, 0x035a000f
  , 0x1a00002e, 0xe35a000f, 0x0a00000d, 0xe59430f4, 0xe1530001, 0x97861005, 0x959430f4, 0x90631001
  , 0x9a000035, 0xe3a03000, 0xe7863005, 0xe51b00a4, 0xe1a01003, 0xe1a02008, 0xebffffa1, 0xe3a01000
  , 0xea00002d, 0xe51b20a0, 0xe1520001, 0x8a000013, 0xe59b300c, 0xe3530000, 0x07862005, 0x00621001
  , 0x0a000025, 0xe51b20a0, 0xe2823001, 0xe1510003, 0x27862005, 0x21e03002, 0x20811003, 0x2a00001e
  , 0xe3a03000, 0xe7863005, 0xe51b00a4, 0xe1a01003, 0xe1a02008, 0xebffff8a, 0xe3a01000, 0xea000016
  , 0xe3a02000, 0xe7862005, 0xe51b00a4, 0xe1a01002, 0xe1a02008, 0xebffff82, 0xe3a01000, 0xea00000e
  , 0xe59430f4, 0xe1530001, 0x8a000006, 0xe51b30a0, 0xe1510003, 0x97861005, 0x87863005, 0xe59430f4
  , 0xe0631001, 0xea000004, 0xe7861005, 0xe51b00a4, 0xe1a02008, 0xebffff72, 0xe3a01000, 0xe2888001
  , 0xe355003c, 0x1affffb2, 0xe51b20a8, 0xe3520001, 0x1a00000b, 0xe3054910, 0xe3404154, 0xe3060ce4
  , 0xe3400154, 0xe1a01004, 0xe30123d4, 0xebe37c6b, 0xe1a00004, 0xe3a01000, 0xe30123d4, 0xebe37cc4
  , 0xea000004, 0xe3050910, 0xe3400154, 0xe3a01000, 0xe30123d4, 0xebe37cbe, 0xe24bd020, 0xe8bd8ff0
  ]

-- Same pin, app/capture/record/tu_00eb5170.c, FUN_00eb88a0 (NONMATCHING); function-ranges.json
-- gives [eb88a0,ebdd58) with this digest. The head [eb88a0,eb8a90) reaches the dispatch
-- eb8a8c `cmp r3, #11`, eb8a90 `ldrls pc, [pc, r3, lsl #2]`; table entry 0 at eb8a98 is
-- ebbf0c. The walk from ebbf0c over [ebbf0c,ebdd58) reaches ebc6b0 and ebc9a8.
cropGeometry :: Image -> [ClosedSite]
cropGeometry image
  | not (extentMatches image (GuestAddress functionStart) (GuestAddress functionEnd) functionDigest) = []
  | otherwise = case (headWalk, caseBinding) of
      (Just reached, Just binding)
        | S.member dispatchCompare reached
            && imageWord image dispatchCompare == Just 0xe353000b
            && imageWord image dispatch == Just 0x979ff103
            && imageWord image (dispatch + 8) == Just caseEntry ->
            instructionSites ScanImage5 image binding
              (sites [(GuestAddress 0xebc6b0, SixteenNinthsOfR7), (GuestAddress 0xebc9a8, SixteenNinthsOfR4)])
      _ -> []
  where
    functionStart = 0xeb88a0
    functionEnd = 0xebdd58
    dispatchCompare = 0xeb8a8c
    dispatch = 0xeb8a90
    caseEntry = 0xebbf0c
    headWalk = boundExtent image functionStart dispatch >>= reachableInstructions image
    caseBinding = boundExtent image caseEntry functionEnd
    functionDigest = reviewedHash "e187ed06e50e5b18e6a200e509cc916b095c1719616812a9b1ddfa6db28816a7"

-- | The image's words over [start, end) as a binding entered at start.
boundExtent :: Image -> Word32 -> Word32 -> Maybe FunctionBinding
boundExtent image start end =
  FunctionBinding image5Base (GuestAddress start) <$> traverse (imageWord image) [start, start + 4 .. end - 4]

reviewedHash :: String -> SHA.Sha256
reviewedHash = either (error . ("retained image5 CFG digest: " ++)) id . SHA.parseSha256Hex
