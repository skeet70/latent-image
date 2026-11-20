-- | The declared register-branch target sets of the heap-calling task walks (reports/general-heap-cutover.md#stack-headroom).
-- Addresses are read from the stock images whose digests each declaration names.
module Replacement.StartupArtifact.CallbackDeclarations (declarations) where

import Data.Word (Word32)
import Replacement.StartupArtifact.CallbackTargets
import Replacement.StartupArtifact.StackBounds (Watched (..))
import StockStack (CodeRange (..))

image4, image5, plain :: ImageDigest
image4 = ImageDigest 0x61c400 "f2e2acab892dafeed85171f6870a411f0a2d5eae10921b86a24630d401dcff16"
image5 = ImageDigest 0xbf1a00 "3cd501c07533938ae4d9347616986fd1649005a588c36b5abf05e36ef4fd892a"
plain = ImageDigest 0x1100000 "ee211bdaad39005b4f72e3443a76ca194905381a967cab787d6a4ef14e2d889d"

-- | Stores of single instructions.
at :: [Word32] -> [CodeRange]
at sites = [CodeRange site (site + 4) | site <- sites]

-- | Consecutive words from an address, each holding its value.
holding :: Word32 -> [Word32] -> [IndexWord]
holding address values = [IndexWord (Watched (address + 4 * index) 4) [value] | (index, value) <- zip [0 ..] values]

declarations :: [TargetDeclaration]
declarations =
  -- `dispatch_send_message` 0x72503c calls the message-trace word 0x4089ea94 unchecked (id4.dis 0x7250bc-0x7250cc);
  -- its one store is setter 0x724cbc's `str r0, [r3, #8]`.
  [ TargetDeclaration 0x7250cc [image4] (Slots [0x4089ea94] []) (at [0x724ccc])
  -- Task 35's handler search (0xed142c): table 0xed2cf4[[0x16777c0]] of {id, handler} records ending at id 26,
  -- the index stored by 0xed15c8 (1) and 0xed15ec (its argument).
  , TargetDeclaration 0xed1494 [image5]
      (Slots ([0xed39ec, 0xed39f4 .. 0xed39fc] ++ [0xed3c08, 0xed3c10 .. 0xed3c20] ++ [0xed3a04, 0xed3a0c .. 0xed3a7c])
        (IndexWord (Watched 0x16777c0 4) [0, 1, 2] : holding 0xed2cf4 [0xed39e8, 0xed3c04, 0xed3a00]))
      (at [0xed15dc, 0xed1600])
  -- Task 55 (0x106ceec): list 0x106f3b4[[0x1716590]] of {id, handler} entries ending at id 0xffff.
  , TargetDeclaration 0x106cfcc [image5]
      (Slots ([0x106f37c, 0x106f384 .. 0x106f394] ++ [0x106f3a8, 0x106f3b0] ++ [0x106f35c, 0x106f364 .. 0x106f374]
              ++ [0x106f344, 0x106f34c] ++ [0x106f32c, 0x106f334, 0x106f33c])
        (IndexWord (Watched 0x1716590 4) [0 .. 4] : holding 0x106f3b4 [0x106f378, 0x106f3a4, 0x106f358, 0x106f340, 0x106f328]))
      (at [0x106bd74, 0x106bf3c, 0x106cf1c, 0x106d05c, 0x106d850, 0x106d990, 0x106f150, 0x106f1b4, 0x106f2fc])
  -- Task 62 (0x1064224): list 0x10e37d0[[0x17162fc] clamped to 0..3] of {id, handler} entries ending at handler 0,
  -- the index stored by setter 0x1062960.
  ] ++ [ TargetDeclaration site [image5]
           (Slots ([0x10e37e4, 0x10e37ec] ++ [0x10e37f4, 0x10e37fc, 0x10e3804] ++ [0x10e380c, 0x10e3814 .. 0x10e383c] ++ [0x10e3844, 0x10e384c])
             (IndexWord (Watched 0x17162fc 4) [0 .. 3] : holding 0x10e37d0 [0x10e37e0, 0x10e37f0, 0x10e3808, 0x10e3840]))
           (at [0x1062970])
       | site <- [0x106444c, 0x1064464] ]
  ++ concat
  [ chart image5 0xec090c 0xec4cec 0xec52c0 0xec52f8 0xec4eb0 0x15b8078 0x15b8094 (0xec0990, 0xec09b4) (CodeRange 0xec0b08 0xec3f7c)
  , chart image5 0xdf74ec 0xe3c3a0 0xe3a444 0xe3b330 0xe3af60 0x1545850 0x1545790 (0xdf7570, 0xdf7594) (CodeRange 0xdf75f4 0xe39f4c)
  , chart plain 0x11e8bf8 0x1216fdc 0x1215510 0x12186d8 0x121a67c 0x14ea5ac 0x14ea4ec (0x11e8c7c, 0x11e8ca0) (CodeRange 0x11e8d08 0x1215474)
  , chart image5 0xec9b50 0xecf6d0 0xecf6e4 0xecf720 0xecdf70 0x15c20ac 0x15c20b8 (0xec9bd4, 0xec9bf8) (CodeRange 0xeca15c 0xeccd5c)
  , chart image5 0xefca6c 0xf12f70 0xf133ec 0xf12074 0xf1158c 0x169d4b4 0x169d468 (0xefcb20, 0xefcb58) (CodeRange 0xefcba8 0xf107b4)
  , chart image5 0x1054820 0x1055598 0x10555ac 0x10556b0 0x10555a4 0x1706e44 0x1706e4c (0x10548bc, 0x10548ec) (CodeRange 0x1054954 0x1055570)
  -- Task 36's USB chart (0xd80dd8): 12-byte records.
  , [ TargetDeclaration site [image5] (StateChart (Chart 0xd80dd8 RecordEntries call 0xd840a8 0 0 0xd834d4 0x1469cc4 0x1469cc8)) [CodeRange 0xd80ec4 0xd85108]
    | (site, call) <- [(0xd80e48, ActionCall), (0xd80e68, ChoiceCall)] ]
  ]
  where
    -- A pair-entry chart: its action and choice branches.
    chart digest entry transitions actions choices columns states events (actionSite, choiceSite) writers =
      [ TargetDeclaration site [digest] (StateChart (Chart entry PairEntries call transitions actions choices columns states events)) [writers]
      | (site, call) <- [(actionSite, ActionCall), (choiceSite, ChoiceCall)] ]
