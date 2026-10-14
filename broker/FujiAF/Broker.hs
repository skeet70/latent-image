-- Packet types and the pinned product-identity frames, shared by the camera
-- operations, their tests and the offline rehearsal. No constructor opens USB.
module FujiAF.Broker where

import Control.Exception
import Control.Monad (unless, when)
import qualified Data.ByteString as B
import Data.Word (Word8)
import Numeric (showHex)
import System.IO (hClose)
import System.Posix.Files
import System.Posix.IO
import System.Posix.Unistd (fileSynchronise)

data Transfer = ControlWrite Int Int Int Int [Word8] | BulkOut Int [Word8] | BulkIn Int Int
  deriving (Eq, Show, Read)

-- Scripted transfer outcomes for the offline rehearsal and the synthetic tests.
data Outcome = Transferred Int | Reply [Word8] | LostReply deriving (Eq, Show, Read)
data Step = Step { expectedTransfer :: Transfer, fakeOutcome :: Outcome } deriving (Eq, Show, Read)

data BrokerFailure = BrokerFailure String deriving (Eq, Show)
instance Exception BrokerFailure

reject :: String -> IO a
reject = throwIO . BrokerFailure

readBounded :: Int -> FilePath -> IO B.ByteString
readBounded limit path = bracket acquire hClose $ \h -> do
  bytes <- B.hGet h (limit + 1)
  when (B.length bytes > limit) (reject "file exceeds input budget")
  pure bytes
  where
    acquire = do
      status <- getSymbolicLinkStatus path
      unless (isRegularFile status && not (isSymbolicLink status)) (reject "input must be a regular file without symlinks")
      when (fileSize status > fromIntegral limit) (reject "file exceeds input budget")
      bracketOnError (openFd path ReadOnly defaultFileFlags { nofollow = True, nonBlock = True, cloexec = True }) closeFd $ \fd -> do
        opened <- getFdStatus fd
        unless (isRegularFile opened) (reject "input changed to a non-regular file")
        fdToHandle fd

hexBytes :: [Word8] -> String
hexBytes = concatMap (\x -> let s = showHex x "" in replicate (2-length s) '0' ++ s)

-- Durable append, no truncation.
appendDurable :: FilePath -> String -> IO ()
appendDurable path content = bracket (openFd path WriteOnly flags) closeFd $ \fd -> do
  wrote <- fdWrite fd content
  unless (fromIntegral wrote == length content) (reject "short journal write")
  fileSynchronise fd
  where flags = defaultFileFlags { append = True, creat = Just 0o600, nofollow = True, cloexec = True }

controlTransfer :: [Word8] -> Transfer
controlTransfer = ControlWrite 0x43 0 0 0

control :: Word8 -> Int -> Int -> [Word8]
control op len sid = [op,0,0,0,0,0,0,0,fromIntegral (len `div` 256),fromIntegral len,fromIntegral (sid `div` 256),fromIntegral sid]

identityPacket :: [Word8]
identityPacket = [0,0,0x15,0] ++ replicate 12 0 ++ [4,0,0,0] ++ replicate 12 0

decodeProduct :: [Word8] -> Either String String
decodeProduct bytes
  | length bytes /= 128 = Left "identity reply must be exactly 128 bytes"
  | take 4 bytes /= [0,0,0x15,0] = Left "identity opcode mismatch"
  | take 4 (drop 4 bytes) /= replicate 4 0 = Left "identity status error"
  | take 8 (drop 8 bytes) /= replicate 8 0 = Left "unknown identity header fields (local policy)"
  | otherwise = case pairs (drop 16 bytes) of
      xs | take 5 xs == [88,45,77,49,0] -> Right "X-M1"
         | otherwise -> Left "identity product does not exactly match X-M1 UTF-16LE"
  where pairs [] = []
        pairs (a:b:rest) = (fromIntegral a + 256*fromIntegral b :: Int) : pairs rest
        pairs _ = []
