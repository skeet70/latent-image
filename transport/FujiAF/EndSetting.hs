-- End Setting candidate: C0 open, the fixed D0 opcode 0x200001 function 5
-- POWER OFF request, its D1 reply, and no C4.
module FujiAF.EndSetting where

import Control.Exception
import Control.Monad (when)
import Data.Bits ((.&.), shiftR)
import Data.IORef
import Data.Word (Word8, Word16)
import FujiAF.Transport
import qualified FujiAF.Broker as P

newtype EndSettingSessionId = EndSettingSessionId Word16 deriving (Eq, Show, Read)
data EndSettingOutcome = EndSettingAccepted | EndSettingRejected deriving (Eq, Show, Read)
data EndSettingPackets = EndSettingPackets
  { endD0Control, endD0Bulk, endD1Control, endD1Bulk :: P.Transfer }
  deriving (Eq, Show, Read)

endRequestBytes, endReplyBytes, endJournalCapBytes :: Int
endRequestBytes = 46
endReplyBytes = 128
endJournalCapBytes = 65536

-- Four acquisition stages, six transfers and three host cleanup stages, each
-- held to one transfer budget.
endAggregateDeadlineMs :: Int
endAggregateDeadlineMs = transferTimeoutMs * 13

outcomeName :: EndSettingOutcome -> String
outcomeName EndSettingAccepted = "accepted"
outcomeName EndSettingRejected = "rejected"

wordLE :: Int -> [Word8]
wordLE value = map (\bit -> fromIntegral ((value `shiftR` bit) .&. 0xff)) [0,8,16,24]

wordAt :: Int -> [Word8] -> Int
wordAt offset bytes = foldr (\byte acc -> acc*256 + fromIntegral byte) 0 (take 4 (drop offset bytes))

endRequestPayload :: [Word8]
endRequestPayload = wordLE 0x00200001 ++ replicate 12 0 ++ wordLE 5 ++ replicate 12 0 ++ wordLE 10 ++ [0x50,0x4f,0x57,0x45,0x52,0x20,0x4f,0x46,0x46,0x0a]

buildEndSettingPackets :: EndSettingSessionId -> Descriptors -> EndSettingPackets
buildEndSettingPackets (EndSettingSessionId sid) d = EndSettingPackets
  (P.controlTransfer (P.control 0xd0 endRequestBytes (fromIntegral sid)))
  (P.BulkOut (outputEndpoint d) endRequestPayload)
  (P.controlTransfer (P.control 0xd1 endReplyBytes (fromIntegral sid)))
  (P.BulkIn (inputEndpoint d) endReplyBytes)

decodeEndSettingReply :: [Word8] -> Either String EndSettingOutcome
decodeEndSettingReply bytes
  | length bytes /= endReplyBytes = Left "End Setting reply must contain exactly 128 bytes"
  | wordAt 0 bytes /= 0x00200001 = Left "End Setting opcode echo mismatch"
  | wordAt 12 bytes /= 0 = Left "End Setting reserved reply word is nonzero"
  | wordAt 16 bytes /= 5 = Left "End Setting function echo mismatch"
  | otherwise = case (wordAt 4 bytes, wordAt 8 bytes) of
      (0,0) -> Right EndSettingAccepted
      (1,3) -> Right EndSettingRejected
      _ -> Left "unsupported End Setting error pair"

runEndSetting :: Clock -> UsbApi -> FilePath -> IO EndSettingOutcome
runEndSetting clock api dir = runEndSettingCore clock api dir `catch` \(e::SomeException) -> do
  _ <- try (recordWithin endJournalCapBytes dir "session-uncertain" (take 4096 (displayException e))) :: IO (Either SomeException ())
  throwIO e

runEndSettingCore :: Clock -> UsbApi -> FilePath -> IO EndSettingOutcome
runEndSettingCore clock api dir = do
  s <- newSession clock api dir endJournalCapBytes endAggregateDeadlineMs
  held <- newHeld
  note s "prepared" "end-setting"
  mask $ \restore -> do
    result <- try $ restore $ do
      device <- acquireDevice s held
      _ <- tx s (P.controlTransfer (P.control 0xc0 8 0)) 12
      reply <- tx s (P.BulkIn (inputEndpoint (descriptors device)) 8) 8
      let sid = EndSettingSessionId (fromIntegral (reply!!0) + 256*fromIntegral (reply!!1))
          replyTail = foldr (\b a -> fromIntegral b + 256*a) (0::Integer) (drop 4 reply)
      when (replyTail /= 65536) (P.reject "session reply tail is not the constant 65536")
      let packets = buildEndSettingPackets sid (descriptors device)
      _ <- tx s (endD0Control packets) 12
      _ <- tx s (endD0Bulk packets) endRequestBytes
      _ <- tx s (endD1Control packets) 12
      bytes <- tx s (endD1Bulk packets) endReplyBytes
      either P.reject pure (decodeEndSettingReply bytes)
    errors <- newIORef ([] :: [String])
    -- Host cleanup has its own finite budget after an expired camera deadline.
    let cleanup :: String -> (Int -> IO ()) -> IO ()
        cleanup label action = do
          before <- nowNS clock
          action transferTimeoutMs
          after <- nowNS clock
          when (after >= before + fromIntegral transferTimeoutMs*1000000) (P.reject (label++" cleanup deadline exceeded"))
          note s "host-cleanup-complete" label
        attempt label action = cleanup label action `catch` \(e::SomeException) -> modifyIORef' errors (++[label++": "++displayException e])
    releaseHeld held api attempt
    failures <- readIORef errors
    _ <- remaining clock (sessionDeadline s)
    case (result,failures) of
      (_, _:_) -> P.reject ("host cleanup uncertain: "++show failures)
      (Left (e::SomeException),[]) -> throwIO e
      (Right outcome,[]) -> note s "end-setting-complete-stock-unverified" (show outcome) >> pure outcome
