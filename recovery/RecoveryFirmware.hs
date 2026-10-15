module RecoveryFirmware
  ( FileOffset (..), ByteCount (..), LogicalBlock (..), FirmwareDigest (..)
  , IdentityDigest (..), RamAddress (..), FirmwareVersion (..), SegmentKind (..)
  , Segment (..), Firmware (..), resolve, slice, readFirmware
  ) where

import Control.Monad (forM)
import qualified Data.ByteString as B
import qualified FujiAF.Host as H
import qualified FujiAF.UpdateContainer as U
import Data.Word (Word32)
import System.Directory (getCurrentDirectory)
import System.Environment (lookupEnv)
import System.FilePath ((</>), isAbsolute)

newtype FileOffset = FileOffset Int
newtype ByteCount = ByteCount Int
newtype LogicalBlock = LogicalBlock Int
newtype FirmwareDigest = FirmwareDigest String
newtype IdentityDigest = IdentityDigest String deriving (Eq)
newtype RamAddress = RamAddress Word32 deriving (Eq, Show)
data FirmwareVersion = FirmwareVersion Word32 Word32
data SegmentKind = PrimaryKernel | CompanionKernel | Resource2 | Resource3 | Application4 | Application5 | PlainImage | ReservedImage
  deriving (Eq, Enum, Show)
data Segment = Segment SegmentKind U.SegmentEntry

data Firmware = Firmware
  { firmwarePath :: FilePath
  , firmwareDigest :: FirmwareDigest
  , firmwareVersion :: FirmwareVersion
  , firmwareIdentity :: IdentityDigest
  , firmwareBytes :: B.ByteString
  , firmwareMain :: B.ByteString
  , firmwareAux1 :: B.ByteString
  , firmwareAux2 :: B.ByteString
  , firmwareSegments :: [Segment]
  , firmwarePlainDestination :: RamAddress
  , firmwarePlainDestinationOffset :: FileOffset
  }

slice :: B.ByteString -> FileOffset -> ByteCount -> IO B.ByteString
slice bytes (FileOffset offset) (ByteCount count) = do
  H.ensure (offset >= 0 && count >= 0 && offset <= B.length bytes && count <= B.length bytes-offset) "Declared range extends outside the file"
  pure (B.take count (B.drop offset bytes))

resolve :: FilePath -> IO FilePath
resolve path = do
  root <- lookupEnv "BUILD_WORKSPACE_DIRECTORY" >>= maybe getCurrentDirectory pure
  pure (if isAbsolute path then path else root </> path)

readFirmware :: FilePath -> IO Firmware
readFirmware given = do
  path <- resolve given
  bytes <- H.readRegular (64*1024*1024) path
  H.ensure (B.length bytes >= U.stagingLength) "Truncated DAT header"
  H.ensure (U.word32At bytes 0 == 4) "Unsupported DAT container type"
  H.ensure (U.word32At bytes 0x23c == 0) "Unsupported nonzero main-range origin"
  let word = U.word32At bytes
      range at = slice bytes (FileOffset (U.stagingLength + fromIntegral (word at))) (ByteCount (fromIntegral (word (at+4))))
      mainLength = fromIntegral (word 0x240) :: Integer
      aux1Start = fromIntegral (word 0x244) :: Integer
      aux1Length = fromIntegral (word 0x248) :: Integer
      aux2Start = fromIntegral (word 0x250) :: Integer
      aux2Length = fromIntegral (word 0x254) :: Integer
  H.ensure (aux1Start == mainLength && aux2Start == aux1Start+aux1Length && fromIntegral (B.length bytes) == fromIntegral U.stagingLength+aux2Start+aux2Length) "DAT ranges overlap, have gaps or leave undeclared bytes"
  mainBytes <- range 0x23c
  aux1 <- range 0x244
  aux2 <- range 0x250
  H.ensure (not (B.null mainBytes) && U.mainChecksum mainBytes == word 0x20c) "Main checksum mismatch"
  H.ensure (U.auxChecksum aux1 == word 0x24c) "Auxiliary-1 checksum mismatch"
  H.ensure (U.auxChecksum aux2 == word 0x258) "Auxiliary-2 checksum mismatch"
  H.ensure (B.length aux1 `mod` 4 == 0 && B.length aux2 `mod` 8 == 0) "Invalid restore-list alignment"
  _ <- slice mainBytes (FileOffset 0x140000) (ByteCount 0x400)
  let tableWord offset = U.mediaWordAt mainBytes (0x140000+offset)
      root = tableWord 0
      descriptor = tableWord 4
  H.ensure (root `elem` [0x4061c040,0x4061c010]) "Unsupported segment-table root pointer"
  H.ensure (descriptor >= 0x4061c010 && descriptor <= 0x4061c3f4 && descriptor `mod` 4 == 0) "Plain-image descriptor pointer extends outside its table"
  let entriesOffset = 0x140000+fromIntegral (root-0x4061c000)
      descriptorOffset = 0x140000+fromIntegral (descriptor-0x4061c000)
  _ <- slice mainBytes (FileOffset entriesOffset) (ByteCount (U.segmentCount*U.segmentEntryBytes+4))
  H.ensure (U.mediaWordAt mainBytes (entriesOffset+U.segmentCount*U.segmentEntryBytes) == 0xffffffff) "Unsupported segment-table terminator"
  segments <- forM [PrimaryKernel .. ReservedImage] $ \kind -> do
    let index = fromEnum kind
        offset = entriesOffset + index*U.segmentEntryBytes
        field at = U.mediaWordAt mainBytes (offset+at*4)
        entry = U.SegmentEntry (field 0) (field 1) (field 2) (field 3) (field 4) (field 5) (field 6) (field 7) (field 8)
    H.ensure (U.segId entry == fromIntegral index) "Unsupported segment numbering"
    _ <- slice mainBytes (FileOffset (fromIntegral (U.segFlash entry))) (ByteCount (fromIntegral (U.segFlashLen entry)))
    pure (Segment kind entry)
  identity <- slice bytes (FileOffset 4) (ByteCount U.identityBytes)
  digest <- FirmwareDigest <$> H.sha256 bytes
  identityDigest <- IdentityDigest <$> H.sha256 identity
  pure (Firmware path digest (FirmwareVersion (word 0x204) (word 0x208)) identityDigest bytes mainBytes aux1 aux2 segments (RamAddress (U.mediaWordAt mainBytes (descriptorOffset+4))) (FileOffset (descriptorOffset+4)))
