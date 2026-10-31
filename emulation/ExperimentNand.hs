{-# LANGUAGE DeriveGeneric #-}
{-# LANGUAGE OverloadedStrings #-}
module Main (main) where

import Control.Exception (IOException, catch, displayException)
import Control.Monad (unless)
import Data.Aeson (ToJSON (..), encode)
import Data.Bits (complement, popCount, shiftR, (.&.), (.|.))
import qualified Data.ByteString as B
import qualified Data.ByteString.Builder as BB
import qualified Data.ByteString.Lazy as BL
import qualified Data.Text as T
import qualified Data.Text.Encoding as TE
import Data.List (isPrefixOf)
import Emulation.Sha256 (sha256Hex)
import qualified Emulation.Sha256 as Digest
import FujiAF.Transport (writePrivateBytes)
import GHC.Generics (Generic)
import System.Directory (canonicalizePath, doesPathExist, getCurrentDirectory, setCurrentDirectory)
import System.Environment (getArgs, lookupEnv)
import System.Exit (exitFailure)
import System.FilePath (splitDirectories, takeDirectory, takeFileName, (</>))
import System.IO (hPutStrLn, stderr)

newtype PhysicalPage = PhysicalPage Int
newtype HexData = HexData T.Text

data Page = Page PhysicalPage HexData
instance ToJSON Page where
  toJSON (Page (PhysicalPage number) (HexData bytes)) = toJSON (number, bytes)

data Geometry = Geometry
  { page_bytes :: Int, spare_bytes :: Int, pages_per_block :: Int, blocks :: Int
  } deriving (Generic)
instance ToJSON Geometry

data Base = Base { sha256 :: String, size :: Int } deriving (Generic)
instance ToJSON Base

data NandImage = NandImage
  { schema :: Int
  , kind :: T.Text
  , geometry :: Geometry
  , base :: Base
  , data_overlays :: [()]
  , spare_pages :: [()]
  , committed_data_pages :: [Page]
  , committed_spare_pages :: [Page]
  } deriving (Generic)
instance ToJSON NandImage

hexData :: B.ByteString -> HexData
hexData = HexData . TE.decodeUtf8 . BL.toStrict . BB.toLazyByteString . BB.byteStringHex

-- The stock updater's adjustment tags carry class 0x1000 and even parity.
spare :: Int -> B.ByteString
spare unit = B.pack [high, low, high, low] <> B.replicate 508 0xff
  where
    tag = 0x1000 .|. ((0x29 + unit) * 2)
    parity = popCount tag .&. 1
    value = tag .|. parity
    high = fromIntegral (value `shiftR` 8)
    low = fromIntegral value

fixture :: B.ByteString -> NandImage
fixture dat = NandImage 1 "type1-nand-committed-image" (Geometry 2048 512 64 1024)
  (Base (sha256Hex (Digest.sha256 logical)) (B.length logical)) [] [] pages tags
  where
    logical = B.map complement (B.drop 0x25c dat)
    template = B.take 0x42000 (B.drop 0xe0000 logical)
    adjustment = B.take 0xf7 template <> B.singleton 1 <> B.drop 0xf8 template <> B.replicate (0x420000 - 0x42000) 0
    pages = [Page (PhysicalPage (0x381 * 64 + page)) (hexData (B.take 2048 (B.drop (page * 2048) adjustment))) | page <- [0 .. 33 * 64 - 1]]
    tags = [Page (PhysicalPage (0x381 * 64 + page)) (hexData (spare (page `div` 64))) | page <- [0 .. 33 * 64 - 1]]

run :: IO ()
run = do
  root <- lookupEnv "BUILD_WORKSPACE_DIRECTORY" >>= maybe getCurrentDirectory pure
  setCurrentDirectory root
  args <- getArgs
  (datPath, supplied) <- case args of
    [dat, output] -> pure (dat, output)
    _ -> ioError (userError "usage: experiment-nand STOCK_150_DAT NEW_PRIVATE_OUTPUT.json (then gzip -n the JSON to f7-1.nand)")
  dat <- B.readFile datPath
  unless (sha256Hex (Digest.sha256 dat) == "43d67afa1a2bc9ae8481bc0acf02cb085ed0f515122dcbd26addcae102cd1490") $
    ioError (userError "experiment-nand requires the official X-M1 1.50 DAT")
  private <- canonicalizePath "private-data"
  parent <- canonicalizePath (takeDirectory supplied)
  let name = takeFileName supplied
      output = parent </> name
  exists <- doesPathExist output
  unless (splitDirectories private `isPrefixOf` splitDirectories parent && name `notElem` ["", ".", ".."] && not exists) $
    ioError (userError "output must be a fresh file beneath private-data/")
  writePrivateBytes output (BL.toStrict (encode (fixture dat)) <> "\n")
  putStrLn ("Synthetic emulator adjustment JSON: " ++ output)
  putStrLn "Firmware template, zero-filled remainder, stored debug flag 1; no body calibration or camera-write format."

main :: IO ()
main = run `catch` \(failure :: IOException) -> hPutStrLn stderr (displayException failure) >> exitFailure
