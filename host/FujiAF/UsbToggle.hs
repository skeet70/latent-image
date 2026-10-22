-- | The USB personality toggle (reports/usb-personality-toggle.md): its
-- image-5 sites and the merge of its two writes into a base image-5 write
-- set.
module FujiAF.UsbToggle
  ( Write (..)
  , WriteKind (..)
  , hookSite
  , hookCurrent
  , hookStockTarget
  , extentStart
  , extentLength
  , extentSha256
  , stubCallees
  , image5Base
  , image5Sha256
  , toggleWrites
  , mergeWrites
  , stubBlTargets
  , writeEnd
  ) where

import Data.Bits (shiftR, (.&.))
import qualified Data.ByteString as B
import Data.List (sortOn)
import Data.Word (Word32)
import FujiAF.ClassicChrome (Address (..), blTarget, encodeBl)

data WriteKind = BlobWrite | WordWrite deriving (Eq, Show)

-- | One in-place write. Word writes carry exactly four bytes.
data Write = Write
  { wKind :: WriteKind
  , wId :: String
  , wGroup :: String
  , wRole :: String
  , wAddress :: Word32
  , wCurrent :: B.ByteString
  , wNew :: B.ByteString
  }
  deriving (Eq, Show)

-- | Connect handler A's read of cache[0xf7]: @bl 0x0069f8fc@.
hookSite, hookCurrent, hookStockTarget :: Word32
hookSite = 0x00ecf900
hookCurrent = 0xebdf3ffd
hookStockTarget = 0x0069f8fc

-- | The unreferenced image-5 function extent the stub replaces from its entry.
extentStart :: Word32
extentStart = 0x00bf4454

extentLength :: Int
extentLength = 0x90

extentSha256 :: String
extentSha256 = "a862a878075d05fd011efb724fdd7503fb406bddac8c1a00f27998486efd4ec0"

-- | Byte getter, byte setter, debounced key state and system mode word.
stubCallees :: [Word32]
stubCallees = [0x0069f8fc, 0x0069fc94, 0x006a0d00, 0x0073c15c]

image5Base :: Word32
image5Base = 0x00bf1a00

image5Sha256 :: String
image5Sha256 = "3cd501c07533938ae4d9347616986fd1649005a588c36b5abf05e36ef4fd892a"

writeEnd :: Write -> Word32
writeEnd w = wAddress w + fromIntegral (B.length (wNew w))

-- | The stub blob over the extent's current bytes, and the hook word.
toggleWrites :: B.ByteString -> B.ByteString -> Either String [Write]
toggleWrites stub currentExtent
  | B.null stub || B.length stub `mod` 4 /= 0 = Left "stub blob must be whole words"
  | B.length stub > extentLength = Left "stub blob exceeds the extent"
  | B.length currentExtent /= extentLength = Left "current extent must be 0x90 bytes"
  | otherwise = do
      hook <- encodeBl (Address hookSite) (Address extentStart)
      if blTarget (Address hookSite) hookCurrent /= Just (Address hookStockTarget)
        then Left "hook current word does not branch to 0x0069f8fc"
        else
          Right
            [ Write BlobWrite "usb-toggle-stub" "TOGGLE" "stub usb_toggle in the unreferenced function extent [0x00bf4454, 0x00bf44e4)" extentStart (B.take (B.length stub) currentExtent) stub
            , Write WordWrite "usb-toggle-hook" "TOGGLE" "bl usb_toggle replaces connect handler A's bl 0x0069f8fc (cache[0xf7])" hookSite (le hookCurrent) (le hook)
            ]

-- | Disjoint union in address order; any byte overlap is refused.
mergeWrites :: [Write] -> [Write] -> Either String [Write]
mergeWrites base toggle =
  case [(a, b) | a <- toggle, b <- base, wAddress a < writeEnd b, wAddress b < writeEnd a] of
    [] -> Right (sortOn wAddress (base ++ toggle))
    (a, b) : _ -> Left ("base write " ++ wId b ++ " overlaps " ++ wId a)

-- | Targets of every A32 BL (condition AL) in a blob linked at the extent.
stubBlTargets :: B.ByteString -> [Word32]
stubBlTargets blob =
  [ t
  | (k, w) <- zip [0 ..] (words32 blob)
  , w .&. 0x0f000000 == 0x0b000000
  , Just (Address t) <- [blTarget (Address (extentStart + 4 * k)) (0xe0000000 + (w .&. 0x0fffffff))]
  ]

words32 :: B.ByteString -> [Word32]
words32 bytes
  | B.length bytes < 4 = []
  | otherwise = sum [fromIntegral (B.index bytes i) * 256 ^ i | i <- [0 .. 3]] : words32 (B.drop 4 bytes)

le :: Word32 -> B.ByteString
le w = B.pack [fromIntegral (w `shiftR` s) | s <- [0, 8, 16, 24]]
