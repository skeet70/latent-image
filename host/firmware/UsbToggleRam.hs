module UsbToggleRam
  ( Publication (..)
  , Phase (..)
  , reviewedStub
  , stockExtent
  , extentWords
  , publications
  , publicationTable
  , allocationSize
  , installerStart
  , sourcePaths
  ) where

import Control.Monad (unless)
import qualified Crypto.Hash.SHA256 as SHA256
import Data.Bits ((.&.))
import qualified Data.ByteString as B
import Data.List (nub, sort)
import Data.Word (Word32)
import FujiAF.ClassicChrome (Address (..))
import qualified FujiAF.UsbToggle as Toggle
import Numeric (showHex)
import RamPushLayout (encodePublicationRecord)

data Phase = Setup | Activation deriving (Eq, Show)

data Publication = Publication
  { publicationAddress :: Address
  , publicationOld :: Word32
  , publicationNew :: Word32
  , publicationPhase :: Phase
  } deriving (Eq, Show)

newtype ReviewedStub = ReviewedStub B.ByteString
newtype ExtentWords = ExtentWords B.ByteString

allocationSize :: Word32
allocationSize = 0xd20

installerStart :: Address -> Address
installerStart (Address base) = Address ((base + 31) .&. 0xffffffe0)

reviewedStub :: B.ByteString -> Either String ReviewedStub
reviewedStub bytes = do
  unless (B.length bytes == 88 && digest bytes == "35a7e9597f06bdc1dda250d991df4a468d2401e5ae3c24c22e37403318746d62")
    (Left "USB toggle stub differs from the reviewed 88 bytes")
  let targets = Toggle.stubBlTargets bytes
  unless (sort (nub targets) == sort Toggle.stubCallees && all (`elem` Toggle.stubCallees) targets)
    (Left "USB toggle stub BL targets differ")
  pure (ReviewedStub bytes)

extentWords :: B.ByteString -> Either String ExtentWords
extentWords bytes = do
  unless (B.length bytes == Toggle.extentLength) (Left "USB toggle original extent must be 0x90 bytes")
  pure (ExtentWords bytes)

stockExtent :: B.ByteString -> Either String ExtentWords
stockExtent image = do
  unless (digest image == Toggle.image5Sha256) (Left "RC0 X-M1 1.50 image-5 identity differs")
  let at address size = B.take size (B.drop (fromIntegral (address - Toggle.image5Base)) image)
      extent = at Toggle.extentStart Toggle.extentLength
  unless (digest extent == Toggle.extentSha256) (Left "USB toggle stock extent identity differs")
  unless (at Toggle.hookSite 4 == le32 Toggle.hookCurrent) (Left "USB toggle original hook word differs")
  extentWords extent

publications :: Address -> ReviewedStub -> ExtentWords -> Either String [Publication]
publications allocation (ReviewedStub stub) (ExtentWords extent) = do
  writes <- Toggle.toggleWrites stub extent
  publicationsFromWrites allocation writes

publicationsFromWrites :: Address -> [Toggle.Write] -> Either String [Publication]
publicationsFromWrites (Address base) writes = do
  let wordsFor phase write =
        [ Publication (Address (Toggle.wAddress write + fromIntegral offset))
            (wordAt (Toggle.wCurrent write) offset) (wordAt (Toggle.wNew write) offset) phase
        | offset <- [0, 4 .. B.length (Toggle.wNew write) - 4] ]
      result = concat [wordsFor (if Toggle.wKind write == Toggle.BlobWrite then Setup else Activation) write | write <- writes]
  unless (base `mod` 8 == 0 && toInteger base + toInteger allocationSize <= 0x100000000)
    (Left "USB toggle allocation must be aligned and fit the address space")
  unless (length result == 23 && map publicationPhase result == replicate 22 Setup ++ [Activation])
    (Left "USB toggle publication count or phase order differs")
  unless (all (\p -> let Address address = publicationAddress p in toInteger address + 4 <= toInteger base || toInteger address >= toInteger base + toInteger allocationSize) result)
    (Left "USB toggle publication overlaps allocation")
  unless (map publicationAddress result == map Address ([Toggle.extentStart, Toggle.extentStart + 4 .. Toggle.extentStart + 84] ++ [Toggle.hookSite]))
    (Left "USB toggle publication address order differs")
  unless (publicationNew (last result) == 0xebf492d3) (Left "USB toggle activation branch differs")
  pure result

publicationTable :: [Publication] -> B.ByteString
publicationTable = B.concat . map encode
 where
  encode p = let Address address = publicationAddress p in
    encodePublicationRecord address (publicationOld p) (publicationNew p) (case publicationPhase p of Setup -> 0; Activation -> 1)

sourcePaths :: [FilePath]
sourcePaths =
  [ "menu/ram_push_installer.c", "menu/ram_push_abi.h"
  , "host/firmware/usb-toggle-ram.ld", "host/firmware/usb-toggle-stub.S", "host/firmware/usb-toggle-stub.ld"
  , "host/firmware/build-usb-toggle-ram.sh", "host/firmware/BuildUsbToggleRam.hs", "host/firmware/UsbToggleRam.hs"
  , "host/firmware/ArmElf.hs", "host/firmware/RamPushLayout.hs"
  , "host/FujiAF/Host.hs", "host/FujiAF/JSON.hs", "host/FujiAF/UsbToggle.hs", "host/FujiAF/ClassicChrome.hs"
  , "host/firmware/BUILD.bazel", "host/firmware/classic-chrome-ram-builder-digest.sh", "host/firmware/ram-push-toolchain.sh"
  ]

wordAt :: B.ByteString -> Int -> Word32
wordAt bytes offset = sum [fromIntegral (B.index bytes (offset + index)) * 256 ^ index | index <- [0 .. 3]]

le32 :: Word32 -> B.ByteString
le32 value = B.pack [fromIntegral (value `div` (256 ^ index)) | index <- [0 .. 3 :: Int]]

digest :: B.ByteString -> String
digest = concatMap (\byte -> let digits = showHex byte "" in replicate (2 - length digits) '0' ++ digits) . B.unpack . SHA256.hash
