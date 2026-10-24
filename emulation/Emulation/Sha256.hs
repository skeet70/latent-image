-- | In-process SHA-256; FujiAF.Host.sha256 forks a `sha256sum` per digest.
module Emulation.Sha256 (Sha256, sha256, sha256File, sha256Hex) where

import Control.Exception (evaluate)
import qualified Crypto.Hash.SHA256 as SHA256
import qualified Data.ByteString as B
import qualified Data.ByteString.Builder as BB
import qualified Data.ByteString.Lazy as BL
import qualified Data.ByteString.Lazy.Char8 as BLC
import Data.Word (Word8)
import Numeric (readHex)
import System.Directory (getFileSize)
import System.Environment (lookupEnv)
import System.Process (readProcess)

newtype Sha256 = Sha256 B.ByteString
  deriving Eq

sha256 :: B.ByteString -> Sha256
sha256 = Sha256 . SHA256.hash

-- | Files from 256 MiB, such as 4 GiB card images, go to the devenv OpenSSL
-- (`XM1_OPENSSL`), about ten times faster than this library on hosts with SHA
-- instructions. Smaller files are hashed here in chunks.
sha256File :: FilePath -> IO Sha256
sha256File path = do
  size <- getFileSize path
  if size < 256 * 1024 * 1024
    then BL.readFile path >>= evaluate . Sha256 . SHA256.hashlazy
    else do
      openssl <- lookupEnv "XM1_OPENSSL" >>= maybe (ioError (userError "XM1_OPENSSL is missing; run inside the checked-in devenv shell")) pure
      output <- readProcess openssl ["dgst", "-sha256", "-r", path] ""
      case hexBytes (takeWhile (/= ' ') output) of
        Just bytes | length bytes == 32 -> pure (Sha256 (B.pack bytes))
        _ -> ioError (userError ("unexpected openssl digest output for " ++ path))

hexBytes :: String -> Maybe [Word8]
hexBytes text = case text of
  [] -> Just []
  high : low : rest | [(byte, "")] <- readHex [high, low] -> (fromInteger byte :) <$> hexBytes rest
  _ -> Nothing

-- | Lowercase hexadecimal, as `sha256sum` prints it.
sha256Hex :: Sha256 -> String
sha256Hex (Sha256 digest) = BLC.unpack (BB.toLazyByteString (BB.byteStringHex digest))
