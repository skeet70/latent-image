{-# LANGUAGE GHC2021 #-}

-- | The WLAN chip image in overlay 47 of the plain image and the scan sites inside it.
-- x70view 55436fc85c0560dd252b0410af6f2427fe198cd3, xm1-150, app/connectivity/wifi/wlan:
-- tu_012ae7b4.c FUN_012afab4 returns g_012b2cb0 with length g_012d4270; its callers hand the
-- pointer to FUN_012946c8, whose only read of it is the loader A byte copy FUN_0004357c.
module Replacement.PropertyService.RetainedPlainBlocks
  ( plainBlockClosures
  ) where

import Control.Monad (guard)
import Data.Bits (rotateR, shiftL, shiftR, (.&.), (.|.))
import qualified Data.ByteString as B
import Data.List (sort)
import qualified Data.Set as S
import Data.Word (Word32)
import qualified Emulation.Sha256 as SHA
import Replacement.PropertyService.Placement (GuestAddress (..), ScanImage (..))
import Replacement.PropertyService.RetainedClosure
import ServiceInventory (Image (..), imageWord)

-- | A byte of the chip image; the CPU reads it only with the `ldrb` of [0x4357c,0x435ac).
data PlainBlockRole = ChipImageByte

data Extent = Extent Word32 Word32 SHA.Sha256

-- | An instruction outside the chip image that forms a chip-image address or the getter's.
data Materialization = Materialization ScanImage Word32 Word32
  deriving (Eq, Ord)

data CallSite = CallSite ScanImage Word32
  deriving (Eq, Ord)

plainBlockClosures :: [(ScanImage, Image)] -> [ClosedSite]
plainBlockClosures images = case chipImageEnd images of
  Nothing -> []
  Just end ->
    [ ClosedSite ScanPlain (GuestAddress site) word (roleClosure role)
    | Just plain <- [imageFor ScanPlain images]
    , (site, word, role) <- sites
    , site >= chipImageBase
    , toInteger site + 4 <= toInteger end
    , imageWord plain site == Just word
    ]

roleClosure :: PlainBlockRole -> ClosedRole
roleClosure ChipImageByte = ConsumedData

sites :: [(Word32, Word32, PlainBlockRole)]
sites =
  [ (0x12b7004, 0xbacda046, ChipImageByte)
  , (0x12ba48c, 0xcacdc8b4, ChipImageByte)
  , (0x12bd64c, 0xfacdae43, ChipImageByte)
  , (0x12c0860, 0x2bcd81a1, ChipImageByte)
  , (0x12c43dc, 0x0acd76bb, ChipImageByte)
  , (0x12c5dd0, 0x1bcd7865, ChipImageByte)
  , (0x12cba78, 0x7bcd506b, ChipImageByte)
  , (0x12d1b88, 0x5acd3750, ChipImageByte)
  ]

chipImageBase, chipImageLength, chipImageGetter :: Word32
chipImageBase = 0x12b2cb0
-- g_012d4270, the byte count stored directly after the image.
chipImageLength = 0x12d4270
-- FUN_012afab4, [0x12afab4,0x12afb54).
chipImageGetter = 0x12afab4

-- | The image's end, when every bound byte holds and the census finds only the bound readers.
chipImageEnd :: [(ScanImage, Image)] -> Maybe Word32
chipImageEnd images = do
  scanned <- mapM (\label -> (,) label <$> imageFor label images) [minBound .. maxBound]
  plain <- lookup ScanPlain scanned
  loaderA <- lookup ScanLoaderA scanned
  guard (all (bound plain) (container : plainConsumers))
  guard (bound loaderA byteCopy)
  count <- imageWord plain chipImageLength
  let end = chipImageBase + count
  guard (end == chipImageLength)
  formed <- concat <$> mapM (uncurry materializations) scanned
  guard (sort formed == expectedMaterializations)
  guard (sort (concatMap (uncurry getterCalls) scanned) == expectedCalls)
  let named = handles end
  guard (null [() | (_, image) <- scanned, (_, value) <- indexedWords image, S.member value named])
  pure end

-- | The image and its length word, [0x12b2cb0,0x12d4274).
container :: Extent
container = extent 0x12b2cb0 0x12d4274 "ec1bfe6300e05b9eb106c127a12744c40a320d31c33c431bb81ba3a3e2306e76"

-- | tu_012ae7b4.c FUN_012afab4 (getter) and FUN_012afb54 (release, stores its arguments only),
-- FUN_012af3ac (pointer slot r11-0x44 read at 0x12af624 and 0x12af64c); tu_012a272c.c
-- FUN_012a4ddc (slot r11-0x3c read at 0x12a51d0); tu_01291f6c.c FUN_01292024 (r11-0x68 read at
-- 0x1292488) and FUN_012946c8 (r11-0x24 read at 0x1294780 as the copy source). Each loop bounds
-- the source to [base, base + count). Every listed function is NONMATCHING except FUN_012afb54.
plainConsumers :: [Extent]
plainConsumers =
  [ extent 0x12afab4 0x12afb54 "a6914ba7d35ace070f9dac0cf1773b93e35772efc59a5b1448b331ed22ed9f9f"
  , extent 0x12afb54 0x12afb88 "3598afb9ff4266c4ba685b7b6c1bab9d6243e24242179dadc61472fd4691d9e2"
  , extent 0x12af3ac 0x12af80c "ad32fda5a062bf25792cc11528bd1e971be33d09f20e8509e346da926afcc2e5"
  , extent 0x12a4ddc 0x12a562c "fc026f930f3587bdc42393d6a1a13035e568b37daf0b1f78909a4e7a3882e014"
  , extent 0x1292024 0x1292dcc "26899081d66797a016a274d68286316462e798515a6b453f06c3299133db8391"
  , extent 0x12946c8 0x12948dc "49f0de2ba672ac6098ed68bbbe40072e0da4a8ee0fc386eba8ed3ec26e0e7b3b"
  ]

-- | kernel0/src/native_recovery/native_memory.c FUN_0004357c, exact.
byteCopy :: Extent
byteCopy = extent 0x4357c 0x435ac "e50d121ff6c8881f53d3720e9570e6511103f7d253577e90f9665b9f038ba5e3"

-- | The getter's movw/movt pairs.
expectedMaterializations :: [Materialization]
expectedMaterializations =
  sort
    [ Materialization ScanPlain 0x12afb00 chipImageLength
    , Materialization ScanPlain 0x12afb14 chipImageLength
    , Materialization ScanPlain 0x12afb2c chipImageBase
    , Materialization ScanPlain 0x12afb38 chipImageBase
    ]

-- | The `bl` in FUN_012a4ddc and in FUN_012af3ac.
expectedCalls :: [CallSite]
expectedCalls = sort [CallSite ScanPlain 0x12a4e78, CallSite ScanPlain 0x12af608]

-- | Values a data word would hold to name the image, one of its 2048-byte copy chunks, its length
-- word or the getter.
handles :: Word32 -> S.Set Word32
handles end =
  S.fromList
    ([chipImageLength, chipImageGetter, chipImageGetter + 1] ++ takeWhile (< end) [chipImageBase, chipImageBase + 0x800 ..])

formsTarget :: Word32 -> Bool
formsTarget value =
  (value >= chipImageBase && value < chipImageLength + 4) || value == chipImageGetter || value == chipImageGetter + 1

-- | movw/movt pairs, word literal loads and `add`/`sub` of pc forming a target, outside the image
-- itself; Nothing when a movt with high half 0x12a..0x12d has no movw to the same register in the
-- 64 words before it.
materializations :: ScanImage -> Image -> Maybe [Materialization]
materializations label image = concat <$> mapM formed (indexed image)
  where
    bytes = imageBytes image
    count = B.length bytes `div` 4
    formed (index, address, word)
      | label == ScanPlain && address >= chipImageBase && address < chipImageLength + 4 = Just []
      | word `shiftR` 28 == 15 = Just []
      | word .&. 0x0ff00000 == 0x03400000 && high >= 0x12a && high <= 0x12d = do
          low <- pairedMovw index (destination word)
          pure (found ((high `shiftL` 16) .|. low))
      | word .&. 0x0f7f0000 == 0x051f0000 = Just (maybe [] found (literal address word))
      | word .&. 0x0fef0000 == 0x028f0000 = Just (found (address + 8 + immediate word))
      | word .&. 0x0fef0000 == 0x024f0000 = Just (found (address + 8 - immediate word))
      | otherwise = Just []
      where
        high = imm16 word
        found value = [Materialization label address value | formsTarget value]
    pairedMovw index register =
      case [ imm16 previous
           | back <- [index - 1, index - 2 .. max 0 (index - 64)]
           , let previous = wordAt bytes (4 * back)
           , previous `shiftR` 28 /= 15
           , previous .&. 0x0ff00000 == 0x03000000
           , destination previous == register
           ] of
        low : _ -> Just low
        [] -> Nothing
    literal address word =
      let offset = toInteger (word .&. 0xfff)
          target = toInteger address + 8 + (if word .&. 0x00800000 /= 0 then offset else negate offset)
          slot = target - toInteger (imageBase image)
      in if target `mod` 4 == 0 && slot >= 0 && slot `div` 4 < toInteger count
           then Just (wordAt bytes (fromInteger slot))
           else Nothing
    immediate word = (word .&. 0xff) `rotateR` fromIntegral (2 * ((word `shiftR` 8) .&. 15))
    imm16 word = ((word `shiftR` 4) .&. 0xf000) .|. (word .&. 0xfff)
    destination word = (word `shiftR` 12) .&. 15

-- | Direct B, BL and BLX whose destination is the getter.
getterCalls :: ScanImage -> Image -> [CallSite]
getterCalls label image =
  [ CallSite label address
  | (_, address, word) <- indexed image
  , word .&. 0x0e000000 == 0x0a000000
  , branchTarget address word == toInteger chipImageGetter
  ]
  where
    branchTarget address word =
      let encoded = toInteger (word .&. 0x00ffffff)
          displacement = if encoded >= 0x800000 then encoded - 0x1000000 else encoded
          halfword = if word `shiftR` 28 == 15 then 2 * toInteger ((word `shiftR` 24) .&. 1) else 0
      in toInteger address + 8 + 4 * displacement + halfword

indexed :: Image -> [(Int, Word32, Word32)]
indexed image =
  [ (index, imageBase image + fromIntegral (4 * index), wordAt (imageBytes image) (4 * index))
  | index <- [0 .. B.length (imageBytes image) `div` 4 - 1]
  ]

indexedWords :: Image -> [(Word32, Word32)]
indexedWords image = [(address, word) | (_, address, word) <- indexed image]

wordAt :: B.ByteString -> Int -> Word32
wordAt bytes offset =
  foldr (\index value -> (value `shiftL` 8) .|. fromIntegral (B.index bytes (offset + index))) 0 [0 .. 3]

bound :: Image -> Extent -> Bool
bound image (Extent start end expected) = extentMatches image (GuestAddress start) (GuestAddress end) expected

extent :: Word32 -> Word32 -> String -> Extent
extent start end expected = Extent start end (either (error . ("retained plain block digest: " ++)) id (SHA.parseSha256Hex expected))
