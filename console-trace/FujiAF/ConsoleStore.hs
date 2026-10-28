-- SPDX-License-Identifier: AGPL-3.0-only
{-# LANGUAGE GeneralizedNewtypeDeriving #-}
module FujiAF.ConsoleStore
  ( Entry(..), Store(..), Input(..), RecordType(..), ThreadId(..), MessageCode(..)
  , Slot(..), SlotCount(..), Tick(..), Argument(..), Parameter(..), parseInput, parseThread
  , loadStore, hexRun, ascii, typeByte, word16le
  , ErrorPayload(..), ErrorCategory(..), ErrorCode(..)
  ) where

import Control.Monad (forM, unless)
import Data.Bits (Bits, shiftL, shiftR, (.&.), (.|.))
import qualified Data.ByteString as B
import Data.Char (isPrint)
import Data.List (sort)
import Data.Word (Word8, Word16, Word32)
import qualified FujiAF.Host as H
import Numeric (readHex)
import System.Directory (listDirectory)
import System.FilePath ((</>), takeExtension, dropExtension)
import Text.Printf (PrintfArg, printf)

newtype Slot = Slot Int deriving (Eq, Ord, Show, PrintfArg)
newtype ThreadId = ThreadId Word8 deriving (Eq, Ord, Show, PrintfArg)
newtype SlotCount = SlotCount Int deriving (Eq, Ord, Show, PrintfArg)
newtype MessageCode = MessageCode Word16 deriving (Eq, Ord, Show, Num, Bits, PrintfArg)
newtype Parameter = Parameter Word16 deriving (Eq, Ord, Show, PrintfArg)
newtype Tick = Tick Word32 deriving (Eq, Ord, Show, Num, Enum, PrintfArg)
newtype Argument = Argument Word32 deriving (Eq, Ord, Show, PrintfArg)
newtype ErrorCategory = ErrorCategory Word32 deriving (Eq, Ord, Show, PrintfArg)
newtype ErrorCode = ErrorCode Word32 deriving (Eq, Ord, Show, PrintfArg)
data ErrorPayload = ErrorPayload !ErrorCategory !ErrorCode deriving (Eq, Show)

data RecordType = Message | Marker | Failure | Retained | ErrorReport | Unknown Word8 deriving (Eq, Ord, Show)
data Input = StoreFile FilePath | HighKnownFile FilePath | WindowDirectory FilePath

data Entry = Entry
  { enSlot :: !Slot
  , enType :: !RecordType
  , enThread :: !ThreadId
  , enArg :: !Argument
  , enCode :: !MessageCode
  , enParam :: !Parameter
  , enTick :: !(Maybe Tick)
  , enErrorPayload :: !(Maybe ErrorPayload)
  , enRaw :: !B.ByteString
  }
data Store = Store { capacity :: !SlotCount, nextIndex :: !Slot, entries :: [Entry] }

parseInput :: [String] -> Either String (Input, [String])
parseInput ("store":path:rest) = Right (StoreFile path, rest)
parseInput ("high-known":path:rest) = Right (HighKnownFile path, rest)
parseInput ("windows":path:rest) = Right (WindowDirectory path, rest)
parseInput _ = Left "Expected store FILE | high-known FILE | windows DIRECTORY"

parseThread :: String -> Either String ThreadId
parseThread text = case readHex text of
  [(value, "")] | value >= (0 :: Integer) && value <= 255 -> Right (ThreadId (fromInteger value))
  _ -> Left "Thread must be an unprefixed hexadecimal byte"

word16le :: B.ByteString -> Int -> Word16
word16le bytes offset = fromIntegral (B.index bytes offset) .|. (fromIntegral (B.index bytes (offset+1)) `shiftL` 8)
word32le :: B.ByteString -> Int -> Word32
word32le bytes offset = fromIntegral (word16le bytes offset) .|. (fromIntegral (word16le bytes (offset+2)) `shiftL` 16)

typeByte :: RecordType -> Word8
typeByte Message = 0x0f
typeByte Marker = 0x72
typeByte Failure = 0x09
typeByte Retained = 0x71
typeByte ErrorReport = 0x81
typeByte (Unknown value) = value

recordType :: Word8 -> RecordType
recordType 0x0f = Message
recordType 0x72 = Marker
recordType 0x09 = Failure
recordType 0x71 = Retained
recordType 0x81 = ErrorReport
recordType value = Unknown value

hexRun :: B.ByteString -> String
hexRun = concatMap (printf "%02x") . B.unpack
ascii :: B.ByteString -> String
ascii = map (\byte -> let c = toEnum (fromIntegral byte) in if isPrint c then c else '.') . B.unpack

loadStore :: Input -> IO Store
loadStore source = do
  bytes <- case source of
    StoreFile path -> H.readRegular 0x20000 path
    HighKnownFile path -> B.take 0x20000 . B.drop 0x800 <$> H.readRegular (64*1024*1024) path
    WindowDirectory path -> do
      names <- sort . filter (\name -> take 1 name == "4" && takeExtension name == ".bin") <$> listDirectory path
      H.ensure (not (null names) && length names <= 512) "Expected 1..512 console window files"
      chunks <- forM (zip [(0 :: Int)..] names) $ \(index,name) -> do
        let expected = 0x400a0800 + index*256
        H.ensure (readHex (dropExtension name) == [(expected, "")]) "Console windows must start at 400a0800 and advance by 256 bytes"
        chunk <- H.readRegular 0x20000 (path </> name)
        unless (index == length names-1) (H.ensure (B.length chunk == 256) "Non-final console window must contain 256 bytes")
        pure chunk
      pure (B.concat chunks)
  H.ensure (B.length bytes >= 0x34 && B.length bytes <= 0x20000) "Console store requires a bounded header and slot prefix"
  let cap = fromIntegral (word16le bytes 0x1c)
      index = fromIntegral (word16le bytes 0x1e)
  H.ensure (cap == 8188 && index < cap) "Unsupported console capacity or next index"
  let complete = min cap ((B.length bytes-0x34) `div` 16)
      decode slot =
        let raw = B.take 16 (B.drop (0x34+slot*16) bytes)
            kind = recordType (B.index raw 0)
            packed = word32le raw 8
            tick = case kind of
              Message -> Just (word32le raw 12)
              Marker -> Just (word32le raw 4)
              Failure -> Just (word32le raw 8)
              Retained -> Just (word32le raw 4)
              ErrorReport -> Just (word32le raw 4)
              Unknown _ -> Nothing
        in Entry (Slot slot) kind (ThreadId (B.index raw 1)) (Argument (word32le raw 4))
             (MessageCode (if kind == Message then fromIntegral (packed `shiftR` 16) else 0))
             (Parameter (if kind == Message then fromIntegral (packed .&. 0xffff) else 0))
             (Tick <$> tick)
             (if kind == ErrorReport
                then Just (ErrorPayload (ErrorCategory (word32le raw 8)) (ErrorCode (word32le raw 12)))
                else Nothing) raw
      occupied = [decode slot | slot <- [0..complete-1], B.index bytes (0x34+slot*16) /= 0]
  pure (Store (SlotCount cap) (Slot index) occupied)
