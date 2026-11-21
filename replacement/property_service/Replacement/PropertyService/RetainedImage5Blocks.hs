{-# LANGUAGE GHC2021 #-}

-- | Image5 sites inside model tables. Roles rest on the bound bytes: the x70view
-- 55436fc85c0560dd252b0410af6f2427fe198cd3 xm1-150 C of every consumer cited here is
-- NONMATCHING and served only as a reading aid.
module Replacement.PropertyService.RetainedImage5Blocks
  ( image5BlockClosures
  ) where

import Control.Monad (guard)
import Data.Bits (complement, popCount, rotateR, shiftL, shiftR, (.&.), (.|.))
import qualified Data.ByteString as B
import Data.List (sortOn)
import Data.Maybe (catMaybes)
import Data.Word (Word32)
import qualified Emulation.Sha256 as SHA
import Replacement.PropertyService.Placement (GuestAddress (..), ScanImage (..))
import Replacement.PropertyService.RetainedClosure
import ServiceInventory (Image (..), imageWord)

data BlockRole
  = -- | One u16 of the 24 x 64 score table, read by `ldrh` and summed.
    ScoreHalfword
  | -- | Signed weight byte of a mask-coded layer, read by `ldrsb` once per set mask bit.
    SparseWeight
  | -- | Mask byte of a mask-coded layer, read by `ldrb` and tested bit by bit.
    SparseMask
  | -- | Signed weight byte of a dense layer, read by `ldrsb`.
    DenseWeight
  | -- | Unsigned weight byte of the 100 x 147 x 14 regressor matrix, read by `ldrb`.
    RegressorWeight
  deriving (Eq, Show)

-- | A half-open byte array and the role its bound readers give each of its bytes.
data RoleExtent = RoleExtent
  { extentStart :: Word32
  , extentEnd :: Word32
  , extentRole :: BlockRole
  }

data BlockSite = BlockSite GuestAddress Word32 BlockRole

blockSites :: [BlockSite]
blockSites =
  [ BlockSite (GuestAddress 0x1082fa0) 0x00630035 ScoreHalfword
  , BlockSite (GuestAddress 0x1092bb0) 0xdbd67414 SparseWeight
  , BlockSite (GuestAddress 0x1098f34) 0x00620a40 SparseMask
  , BlockSite (GuestAddress 0x109fcbc) 0xfbd60cda DenseWeight
  , BlockSite (GuestAddress 0x10a3e8c) 0x2bd5fff3 DenseWeight
  , BlockSite (GuestAddress 0x10a41f8) 0x2bd5f607 DenseWeight
  , BlockSite (GuestAddress 0x10bfafc) 0x8bd59396 RegressorWeight
  , BlockSite (GuestAddress 0x10c4e90) 0x9ad58486 RegressorWeight
  , BlockSite (GuestAddress 0x10dce2c) 0x00629eac RegressorWeight
  , BlockSite (GuestAddress 0x10dee14) 0x6ad51452 RegressorWeight
  ]

image5BlockClosures :: [(ScanImage, Image)] -> [ClosedSite]
image5BlockClosures images = case imageFor ScanImage5 images of
  Nothing -> []
  Just image5 ->
    let extents = concat (catMaybes [scoreTable image5, layerTables image5, imageFor ScanImage4 images >>= regressor image5])
     in [ ClosedSite ScanImage5 site word ConsumedData
        | BlockSite site@(GuestAddress address) word role <- blockSites
        , imageWord image5 address == Just word
        , any (covers address role) extents
        ]
  where
    covers address role extent =
      role == extentRole extent && extentStart extent <= address && toInteger address + 4 <= toInteger (extentEnd extent)

-- id5 FUN_01075d9c [1075d9c,1078f50) and FUN_01078f50 [1078f50,107bce4) (tu_01075d9c.c,
-- tu_01078f50.c) are the only code forming an address in the table. At 1078844 and 107b6dc a
-- movw/movt pair loads r1 and `ldrh r3, [r1, r3]` reads the halfword at (row * 64 + column) * 2,
-- row < 24; every path rewrites r1 before reading it again.
scoreTable :: Image -> Maybe [RoleExtent]
scoreTable image = do
  bindExtent image 0x1075d9c 0x1078f50 "c8d16628690df2b4a424e254583785847630dcfadfdc81321d03cfddcbc0b6b5"
  bindExtent image 0x1078f50 0x107bce4 "dd55642cd3e8da91a16dbcb037975bca0afeb7a76e7fb2a2ab32e40f58d48767"
  bases <- mapM (scoreReader image) [(0x1078838, 0x107887c), (0x107b6d0, 0x107b714)]
  base <- case bases of
    [first, second] | first == second -> Just first
    _ -> Nothing
  let end = base + 24 * 64 * 2
  bindExtent image base end "27fb5c8a2608ffbc3fa28ec7f4d83275261653d72fa83a7dcd77306d7b3fe192"
  pure [RoleExtent base end ScoreHalfword]

-- | `lsl r3, r7, #6` at the row shift, then movw/movt r1 and the halfword load; `cmp r7, #24`
-- at the row bound.
scoreReader :: Image -> (Word32, Word32) -> Maybe Word32
scoreReader image (rowShift, rowBound) = do
  expectWord image rowShift 0xe1a03307
  low <- imageWord image (rowShift + 12)
  high <- imageWord image (rowShift + 16)
  expectWord image (rowShift + 20) 0xe19130b3
  expectWord image rowBound 0xe3570018
  guard (low .&. 0xfff0f000 == 0xe3001000 && high .&. 0xfff0f000 == 0xe3401000)
  pure (immediate16 high `shiftL` 16 .|. immediate16 low)

-- Mask-coded layers: FUN_0108d93c [108d93c,108dbdc) copies the pool words at 108db64 + 20 * case
-- into descriptor fields {+12 weights, +40 mask, +24 bias, +36 scale, +44 shift} of a 10-row
-- layer whose input count FUN_010a9214 sets to 940; FUN_0108dcc0 [108dcc0,108dd50) fills the
-- same fields from the six-entry tables at 10a8a50, 10a8a80, 10a8a68, 10a8a98, 10a8ab0 for a
-- 10-row layer of 705 inputs (FUN_010a950c). FUN_01089d24 passes them to FUN_010aad9c, which
-- reads (inputs + 7) / 8 mask bytes per row with `ldrb` and one `ldrsb` weight per set bit.
-- Dense layers: FUN_0108dd50 [108dd50,108de00) fills {+12 weights, +24 bias, +36 scale,
-- +44 shift} of a 6-row layer with row stride 378 (FUN_0108cdc8) from the 36-entry tables at
-- 10a8ac8, 10a8b58, 10a8be8, 10a8c78; FUN_01089d6c passes them to FUN_010aaee4, which reads the
-- weights with `ldrsb`. Each caller keeps the descriptor in a 48-byte frame slot addressed only
-- by the getter call and the hand-off call.
layerTables :: Image -> Maybe [RoleExtent]
layerTables image = do
  mapM_ (\(start, end, digest) -> bindExtent image start end digest) layerConsumers
  bindExtent image 0x108df34 0x10a8d08 "d3b037193be3d8dce7c71f2e488b4b008d00b4b950ef03a39ec78602ea26d23b"
  pooled <- mapM (\index -> mapM (\field -> imageWord image (0x108db64 + 20 * index + 4 * field)) [0 .. 4]) [0 .. 5]
  indexed <- mapM (tableRow image [0x10a8a50, 0x10a8a80, 0x10a8a68, 0x10a8a98, 0x10a8ab0]) [0 .. 5]
  dense <- mapM (tableRow image [0x10a8ac8, 0x10a8b58, 0x10a8be8, 0x10a8c78]) [0 .. 35]
  guard (all (\row -> row == [0, 0, 0, 0] || notElem 0 row) dense)
  sparseExtents <- mapM (uncurry (sparseLayer image)) ([(940, row) | row <- pooled] ++ [(705, row) | row <- indexed])
  denseExtents <- mapM denseLayer (filter (/= [0, 0, 0, 0]) dense)
  let extents = concat sparseExtents ++ concat denseExtents
      ordered = sortOn extentStart extents
  guard (and (zipWith (\earlier later -> extentEnd earlier <= extentStart later) ordered (drop 1 ordered)))
  pure extents

-- | Getters, their callers and constant tables, hand-off wrappers and the two layer kernels.
layerConsumers :: [(Word32, Word32, String)]
layerConsumers =
  [ (0x108d93c, 0x108dbdc, "c0656f155dd70dbb740de871167d65b8c09b2b0cc194a5cf9b59a719af73b5b4")
  , (0x108dcc0, 0x108dd50, "b3999ec29b012ff211845a06150b23060dd45c186ca8a590e02ca36fc2908e7b")
  , (0x108dd50, 0x108de00, "7e27a80350be64922405ec0c5a005be4a62d410f19137916aba8fa6cea890552")
  , (0x10a9214, 0x10a940c, "208e2bcaba01cec3edf03cea74b33d1f20a1fbd0a4e01deb016d538e75c32fa8")
  , (0x10a950c, 0x10a9714, "4b6dbe23868bb9dab0f8cd8bae4d49eb975e24320f99273b0c65f7182640281a")
  , (0x108cdc8, 0x108d0b4, "bb7b1195ca467f914d5f0e66b83cd95daf053bdc0eb5d793047aa8c920cbf431")
  , (0x108c9b0, 0x108ca6c, "6de64ed6978bd0728d0ae79373c09f507ee552aa9900d7d3a77251d8e5fe9173")
  , (0x1089d24, 0x1089db0, "fd7f7cdf4322d6bacb2daa91311f47cd1a92583fb013f85c881e1fb7371c507a")
  , (0x10aad9c, 0x10aafa4, "e991da396ab0f1fd15d2c1932dbf2716e71c073340863fce7058f3b769d13b2a")
  ]

tableRow :: Image -> [Word32] -> Word32 -> Maybe [Word32]
tableRow image tables index = mapM (\table -> imageWord image (table + 4 * index)) tables

-- | The weight count is the mask's set bits; one pad byte separates weights from mask.
sparseLayer :: Image -> Word32 -> [Word32] -> Maybe [RoleExtent]
sparseLayer image inputs [weights, mask, bias, scale, shift] = do
  let rows = 10
      maskBytes = rows * ((inputs + 7) `div` 8)
  bits <- bytesAt image mask maskBytes
  let weightCount = fromIntegral (sum (map popCount (B.unpack bits)))
  guard (mask == weights + weightCount + 1)
  guard (bias == align4 (mask + maskBytes) && scale == bias + 4 * rows && shift == scale + 4 * rows)
  pure [RoleExtent weights (weights + weightCount) SparseWeight, RoleExtent mask (mask + maskBytes) SparseMask]
sparseLayer _ _ _ = Nothing

denseLayer :: [Word32] -> Maybe [RoleExtent]
denseLayer [weights, bias, scale, shift] = do
  let rows = 6
      end = weights + rows * 378
  guard (bias == align4 end && scale == bias + 4 * rows && shift == scale + 4 * rows)
  pure [RoleExtent weights end DenseWeight]
denseLayer _ = Nothing

-- id5 FUN_010e3160 [10e3160,10e341c) and FUN_010e341c [10e341c,10e3680) (tu_010e21c4.c,
-- tu_010e341c.c) hold the only pool words naming the container: 10e3418 and 10e366c the header,
-- 10e3414 the matrix. FUN_010e3160 walks the matrix with `ldrb` at a cursor advanced by 14
-- (10e3344) for 147 groups (10e3350) in each of 100 rows (10e33b4); both read the header with
-- `ldrb`/`ldr` into arithmetic and never pass a container address to a call.
regressor :: Image -> Image -> Maybe [RoleExtent]
regressor image5 image4 = do
  bindExtent image5 0x10e3160 0x10e341c "74a77c6dd8eeba3f83f28098535cf3f88a3b9f2faac0ea8310001f243237d9fc"
  bindExtent image5 0x10e341c 0x10e3680 "3fd52240c100cdf32a4f86e9597f827c7f9dc385fa82ebddc5d09d243ce45926"
  matrix <- imageWord image5 0x10e3414
  header <- imageWord image5 0x10e3418
  expectWord image5 0x10e366c header
  expectWord image5 0x10e3344 0xe28aa00e
  expectWord image5 0x10e3350 0xe3590092
  expectWord image5 0x10e33b4 0xe3510063
  let rowBytes = 147 * 14
      end = matrix + 100 * rowBytes
      starts = header : [matrix + row * rowBytes | row <- [0 .. 100]]
  bindExtent image5 header end "af350b0fa9cd767eace8dcd1a3ad1b6109b8381bd2a0a9b71260c810632fb0ba"
  bindExtent image4 traceSink 0x7287a4 "1a82c880495fcac5a7d78559761026dc8b5af7d2e006692bfd9123562cc57cda"
  mapM_ (traceCode image4 header end starts) image4TraceSites
  mapM_ (traceCode image5 header end starts) image5TraceSites
  pure [RoleExtent matrix end RegressorWeight]

-- | id4 diagnostic_trace_operation (app/platform/diag/diagnostic_trace_callbacks.c) stores r0
-- into a trace record without dereferencing it.
traceSink :: Word32
traceSink = 0x72872c

-- | The movt of each `mov r0, #low; movt r0, #high; bl diagnostic_trace_operation` whose value
-- lies in the regressor container.
image4TraceSites :: [Word32]
image4TraceSites = [0x73b4a4, 0x73b4cc, 0x73b4f4, 0x73b540, 0x73b580, 0x73b5c4, 0x73b608, 0x73b630, 0x73b66c]

image5TraceSites :: [Word32]
image5TraceSites =
  [ 0xfb28d4, 0xfb2988, 0xfb2a18, 0xfb2a94, 0xfb2b84, 0xfb2bdc, 0xfb2c3c, 0xfb2cbc, 0xfb2d50
  , 0xfb2da4, 0xfb2e14, 0xfb2e98, 0xfb2f38, 0xfb57d4, 0xfb5df0, 0xfb8128, 0xfb8188, 0x1031130
  , 0x1037d70, 0x1037db0, 0x1037f3c, 0x1037f9c, 0x1037ff0, 0x1038050, 0x1038120, 0x10383b0
  , 0x1051abc, 0x1051b9c, 0x1051c90, 0x1052480, 0x1052878, 0x1052980, 0x1052c10, 0x1052c8c
  , 0x1052ef8
  ]

-- | The traced value lies in [start, end) and names no object start.
traceCode :: Image -> Word32 -> Word32 -> [Word32] -> Word32 -> Maybe ()
traceCode image start end starts site = do
  low <- imageWord image (site - 4)
  high <- imageWord image site
  call <- imageWord image (site + 4)
  guard (low .&. 0xfffff000 == 0xe3a00000 && high .&. 0xfff0f000 == 0xe3400000)
  target <- directCall (site + 4) call
  guard (target == traceSink)
  let value = immediate16 high `shiftL` 16 .|. (rotatedImmediate low .&. 0xffff)
  guard (value >= start && value < end && notElem value starts)

directCall :: Word32 -> Word32 -> Maybe Word32
directCall address word = do
  guard (word .&. 0xff000000 == 0xeb000000)
  let encoded = word .&. 0x00ffffff
      displacement = if encoded >= 0x800000 then encoded .|. 0xff000000 else encoded
  pure (address + 8 + displacement `shiftL` 2)

immediate16 :: Word32 -> Word32
immediate16 word = (word `shiftR` 4) .&. 0xf000 .|. word .&. 0xfff

rotatedImmediate :: Word32 -> Word32
rotatedImmediate word = (word .&. 0xff) `rotateR` fromIntegral (2 * ((word `shiftR` 8) .&. 15))

align4 :: Word32 -> Word32
align4 value = (value + 3) .&. complement 3

expectWord :: Image -> Word32 -> Word32 -> Maybe ()
expectWord image address expected = guard (imageWord image address == Just expected)

bytesAt :: Image -> Word32 -> Word32 -> Maybe B.ByteString
bytesAt image start count = do
  guard (start >= imageBase image)
  let offset = toInteger (start - imageBase image)
  guard (offset + toInteger count <= toInteger (B.length (imageBytes image)))
  pure (B.take (fromIntegral count) (B.drop (fromInteger offset) (imageBytes image)))

bindExtent :: Image -> Word32 -> Word32 -> String -> Maybe ()
bindExtent image start end digest = do
  expected <- either (const Nothing) Just (SHA.parseSha256Hex digest)
  guard (extentMatches image (GuestAddress start) (GuestAddress end) expected)
