-- | In-process SHA-256 for the native case suites; FujiAF.Host.sha256 forks a
-- `sha256sum` per digest.
module Emulation.Sha256 (Sha256, sha256, sha256Hex) where

import qualified Crypto.Hash.SHA256 as SHA256
import qualified Data.ByteString as B
import qualified Data.ByteString.Builder as BB
import qualified Data.ByteString.Lazy.Char8 as BLC

newtype Sha256 = Sha256 B.ByteString
  deriving Eq

sha256 :: B.ByteString -> Sha256
sha256 = Sha256 . SHA256.hash

-- | Lowercase hexadecimal, as `sha256sum` prints it.
sha256Hex :: Sha256 -> String
sha256Hex (Sha256 digest) = BLC.unpack (BB.toLazyByteString (BB.byteStringHex digest))
