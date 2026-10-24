-- Linked-section comparison for MATCHING-C-01.
module FujiAF.Matching.Compare
  ( RelocKind (..)
  , relocKind
  , relocKindName
  , RelocatedField (..)
  , ByteComparison (..)
  , compareLinked
  , comparisonDetail
  ) where

import Data.Bits (complement, shiftR, xor, (.&.))
import qualified Data.ByteString as B
import qualified Data.Map.Strict as M
import Data.Word (Word32, Word8)

data RelocKind = RelCall | RelMovw | RelMovt | RelAbs32 | RelOther String deriving (Eq, Show)

relocKind :: String -> RelocKind
relocKind t = case t of
  "R_ARM_CALL" -> RelCall
  "R_ARM_JUMP24" -> RelCall
  "R_ARM_PC24" -> RelCall
  "R_ARM_MOVW_ABS_NC" -> RelMovw
  "R_ARM_MOVT_ABS" -> RelMovt
  "R_ARM_ABS32" -> RelAbs32
  other -> RelOther other

relocKindName :: RelocKind -> String
relocKindName k = case k of
  RelCall -> "branch-24"
  RelMovw -> "movw"
  RelMovt -> "movt"
  RelAbs32 -> "abs32"
  RelOther t -> t

-- Bits of the relocated word that hold the link-time value.
relocMask :: RelocKind -> Word32
relocMask k = case k of
  RelCall -> 0x00ffffff
  RelMovw -> 0x000f0fff
  RelMovt -> 0x000f0fff
  _ -> 0xffffffff

-- A relocated word at a byte offset within the emitted section.
data RelocatedField = RelocatedField Int RelocKind deriving (Show)

data ByteComparison
  = Exact
  | Normalized Int
  | PrefixOnly Int
  | Mismatch Int Int
  deriving (Eq, Show)

-- Compares the linked section with the stock bytes at the same addresses,
-- where stock holds at least as many bytes as the section. The stock body
-- length decides whether a matching section covers the whole function.
compareLinked :: [RelocatedField] -> Int -> B.ByteString -> B.ByteString -> ByteComparison
compareLinked fields body emitted stock
  | null differing && B.length emitted >= body = Exact
  | null differing = PrefixOnly (body - B.length emitted)
  | (first : _) <- unmasked = Mismatch (length unmasked) first
  | B.length emitted >= body = Normalized (length differing)
  | otherwise = PrefixOnly (body - B.length emitted)
  where
    keep = M.fromListWith (.&.) [(offset, complement (relocMask kind)) | RelocatedField offset kind <- fields]
    keepByte i = case M.lookup (i - i `mod` 4) keep of
      Nothing -> 0xff :: Word8
      Just m -> fromIntegral ((m `shiftR` (8 * (i `mod` 4))) .&. 0xff)
    differing = [i | i <- [0 .. B.length emitted - 1], B.index emitted i /= B.index stock i]
    unmasked = [i | i <- differing, (B.index emitted i `xor` B.index stock i) .&. keepByte i /= 0]

comparisonDetail :: ByteComparison -> String
comparisonDetail c = case c of
  Exact -> "linked section equals stock"
  Normalized n -> show n ++ " bytes differ, all inside relocated fields"
  PrefixOnly n -> "linked section agrees with stock but stops " ++ show n ++ " bytes short of the stock body"
  Mismatch n off -> show n ++ " bytes differ outside relocated fields, first at +" ++ show off
