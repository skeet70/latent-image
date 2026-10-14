-- Bounded adjustment-area read over D0 opcode 0x1a0002 function 2, mb 1.
-- The firmware applies no length check on this opcode, so the 256-byte
-- per-request cap below is the only bound on the handler's byte loop.
-- Function 3 of the same opcode writes the adjustment cache and opcode
-- 0x200001 reads arbitrary RAM; neither value occurs in this module.
module FujiAF.Read where

import Control.Exception
import Control.Monad (forM_, unless)
import Data.Bits ((.&.), shiftR)
import qualified Data.ByteString as B
import Data.IORef
import Data.Word (Word8)
import Numeric (showHex)
import System.FilePath ((</>))
import FujiAF.Transport
import FujiAF.SHA256 (sha256)
import qualified FujiAF.Broker as P

readOpcode, readFunction, readMailbox, readWindowLength, adjustmentCacheLimit :: Int
readOpcode = 0x1a0002
readFunction = 2
readMailbox = 1
readWindowLength = 256
adjustmentCacheLimit = 0x420000

-- Request layout of the fixed service request buffer: opcode, reserved words,
-- function, mb, page, in-page offset, length. Appended data would start here.
readRequestBytes :: Int
readRequestBytes = 0x24
requestOpcodeOffset, requestFunctionOffset, requestMailboxOffset, requestPageOffset, requestInPageOffset, requestLengthOffset :: Int
requestOpcodeOffset = 0
requestFunctionOffset = 0x10
requestMailboxOffset = 0x14
requestPageOffset = 0x18
requestInPageOffset = 0x1c
requestLengthOffset = 0x20

-- Installed 1.00 S03 run-02 places the echoed page/in-page/length at
-- 0x18/0x1c/0x20 and data at 0x24. The handler advertises 0x124.
readReplyBytes :: Int
readReplyBytes = 0x124
replyError1Offset, replyError2Offset, replyReservedOffset, replyFunctionOffset, replyMailboxOffset, replyPageOffset, replyOffsetOffset, replyLengthOffset, replyDataOffset :: Int
replyError1Offset = 4
replyError2Offset = 8
replyReservedOffset = 0x0c
replyFunctionOffset = 0x10
replyMailboxOffset = 0x14
replyPageOffset = 0x18
replyOffsetOffset = 0x1c
replyLengthOffset = 0x20
replyDataOffset = 0x24

-- The adjustment bytes at offset 0x15c that 0x0069f388 copies for the tag-4
-- identity reply, so a window covering them tests the whole channel.
productOracleBytes :: [Word8]
productOracleBytes = [0x58,0x2d,0x4d,0x31,0x00]

-- journal cap = journalFixedBytes + journalPerWindowBytes * planned windows,
-- refused above journalCeilingBytes. The ceiling bounds the window count at
-- 2032, above the 1309-window complete-capture scope.
journalFixedBytes, journalPerWindowBytes, journalCeilingBytes :: Int
journalFixedBytes = 65536
journalPerWindowBytes = 4096
journalCeilingBytes = 8*1024*1024

journalCapFor :: Int -> Int
journalCapFor windows = journalFixedBytes + journalPerWindowBytes*windows

-- aggregate deadline = transfer budget * (sessionFixedTransfers + 4 * windows).
-- Four transfers carry one window; the fixed term covers the C0 and C4 pairs
-- and the native init/list/open/claim stages.
sessionFixedTransfers, transfersPerWindow :: Int
sessionFixedTransfers = 8
transfersPerWindow = 4

aggregateDeadlineMs :: Int -> Int -> Int
aggregateDeadlineMs transferMs windows = transferMs*(sessionFixedTransfers+transfersPerWindow*windows)

-- Shape cap shared with the plan schema. The journal ceiling binds first, at
-- 2032 windows, so a plan between the two is a schema pass and a compiled stop.
planWindowCap :: Int
planWindowCap = 2048

-- A plan at the window cap with maximum-length purposes stays under this.
planDocumentCap :: Int
planDocumentCap = 1024*1024

data ReadWindow = ReadWindow { windowIndex :: Int, windowOffset :: Int, windowPurpose :: String }
  deriving (Eq, Show, Read)
data ReadOracle = ReadOracle { oracleWindowIndex :: Int, oracleByteOffset :: Int }
  deriving (Eq, Show, Read)
data ReadPlan = ReadPlan
  { planId :: String, planFirmwareBasis :: String, planWindows :: [ReadWindow]
  , planOracle :: ReadOracle, planTool :: String, planToolDigest :: String }
  deriving (Eq, Show, Read)

validatePlan :: ReadPlan -> Either String ()
validatePlan plan = do
  let windows = planWindows plan
      count = length windows
      offsets = map windowOffset windows
      ReadOracle oracleIndex oracleByte = planOracle plan
      boundedText low high s = length s >= low && length s <= high && all (>= ' ') s
  unless (count >= 1) (Left "read plan must list at least one window")
  unless (journalCapFor count <= journalCeilingBytes) (Left "planned window count exceeds the journal ceiling")
  unless (map windowIndex windows == [0..count-1]) (Left "window index must equal its position in the plan")
  unless (all (\o -> o >= 0 && o `mod` readWindowLength == 0) offsets) (Left "window offset must be 256-aligned")
  unless (and (zipWith (<) offsets (drop 1 offsets))) (Left "window offsets must strictly increase")
  unless (all (\o -> o + readWindowLength - 1 < adjustmentCacheLimit) offsets) (Left "window must end below the adjustment-area bound")
  unless (all (boundedText 1 256 . windowPurpose) windows) (Left "bounded window purpose required")
  unless (oracleIndex >= 0 && oracleIndex < count) (Left "oracle names no window in the plan")
  unless (oracleByte >= 0 && oracleByte + length productOracleBytes <= readWindowLength) (Left "oracle bytes must lie inside their window")
  unless (safeIdentifier (planId plan)) (Left "plan alias required")
  unless (boundedText 1 256 (planFirmwareBasis plan)) (Left "bounded firmware basis required")
  unless (boundedText 1 256 (planTool plan)) (Left "bounded generator name required")
  unless (hexDigest (planToolDigest plan)) (Left "exact generator digest required")

word32le :: Int -> [Word8]
word32le value = map (\bit -> fromIntegral ((value `shiftR` bit) .&. 0xff)) [0,8,16,24]

word32At :: Int -> [Word8] -> Int
word32At at bytes = foldr (\byte acc -> acc*256 + fromIntegral byte) 0 (take 4 (drop at bytes))

readRequestFrame :: ReadWindow -> [Word8]
readRequestFrame window = concat
  [ word32le readOpcode, replicate 12 0
  , word32le readFunction, word32le readMailbox
  , word32le (windowOffset window `div` readWindowLength), word32le 0
  , word32le readWindowLength ]

offsetName :: Int -> String
offsetName offset = replicate (6-length digits) '0' ++ digits
  where digits = showHex offset ""

-- Success, rejected arguments and callback status 2 are the whole set this
-- handler produces; the fourth case is a stop rather than an error code.
decodeReadReply :: ReadWindow -> [Word8] -> Either String [Word8]
decodeReadReply window bytes
  | length bytes /= readReplyBytes = Left ("reply length "++show (length bytes)++" differs from the requested "++show readReplyBytes)
  | word32At 0 bytes /= readOpcode = Left "reply opcode differs from the request"
  | word32At replyReservedOffset bytes /= 0 = Left "reply reserved word is nonzero"
  | word32At replyFunctionOffset bytes /= readFunction = Left "reply function differs from the request"
  | word32At replyMailboxOffset bytes /= readMailbox = Left "reply mailbox differs from the request"
  | otherwise = case (word32At replyError1Offset bytes, word32At replyError2Offset bytes) of
      (0,0) | echoed == requested -> Right (take readWindowLength (drop replyDataOffset bytes))
            | otherwise -> Left ("echoed page/offset/length "++show echoed++" differs from the request "++show requested)
      (9,3) -> Left "rejected arguments: error1 9, error2 3"
      (1,0xffff) -> Left "callback status 2: error1 1, error2 0xffff"
      (first,second) -> Left ("status the handler cannot produce: error1 "++show first++", error2 "++show second)
  where
    echoed = (word32At replyPageOffset bytes, word32At replyOffsetOffset bytes, word32At replyLengthOffset bytes)
    requested = (windowOffset window `div` readWindowLength, 0, readWindowLength)

checkOracle :: ReadPlan -> ReadWindow -> [Word8] -> Either String ()
checkOracle plan window payload
  | windowIndex window /= oracleWindowIndex (planOracle plan) = Right ()
  | take (length productOracleBytes) (drop at payload) == productOracleBytes = Right ()
  | otherwise = Left "oracle window does not carry the bound product string; channel, translation or layout differs"
  where at = oracleByteOffset (planOracle plan)

runRead :: Clock -> UsbApi -> ReadPlan -> FilePath -> IO Int
runRead clock api plan dir = runReadCore clock api plan dir `catch` \(e::SomeException) -> do
  _ <- try (recordWithin (journalCapFor (length (planWindows plan))) dir "session-uncertain" (take 4096 (displayException e))) :: IO (Either SomeException ())
  throwIO e

runReadCore :: Clock -> UsbApi -> ReadPlan -> FilePath -> IO Int
runReadCore clock api plan dir = do
  either P.reject pure (validatePlan plan)
  let count = length (planWindows plan)
      captured = dir </> "windows"
  s <- newSession clock api dir (journalCapFor count) (aggregateDeadlineMs transferTimeoutMs count)
  held <- newHeld
  openedSession <- newIORef Nothing
  completed <- newIORef (0::Int)
  -- The journal carries one line per window instead of the whole plan.
  note s "prepared" ("plan="++planId plan++";basis="++planFirmwareBasis plan++";windows="++show count)
  ensurePrivateDirectory captured
  mask $ \restore -> do
    result <- try $ restore $ do
      device <- acquireDevice s held
      _ <- tx s (P.controlTransfer (P.control 0xc0 8 0)) 12
      reply <- tx s (P.BulkIn (inputEndpoint (descriptors device)) 8) 8
      -- C0 bytes 2..7 are opaque; D1 length is fixed by the config-read handler.
      let sid = fromIntegral (reply!!0) + 256*fromIntegral (reply!!1)
      writeIORef openedSession (Just (device,sid))
      forM_ (planWindows plan) $ \window -> do
        _ <- tx s (P.controlTransfer (P.control 0xd0 readRequestBytes sid)) 12
        _ <- tx s (P.BulkOut (outputEndpoint (descriptors device)) (readRequestFrame window)) readRequestBytes
        _ <- tx s (P.controlTransfer (P.control 0xd1 readReplyBytes sid)) 12
        bytes <- tx s (P.BulkIn (inputEndpoint (descriptors device)) readReplyBytes) readReplyBytes
        let label = offsetName (windowOffset window)
            stop reason = do
              note s "window-refused" (label++";reply="++P.hexBytes bytes++";"++reason)
              P.reject reason
        payload <- either stop pure (decodeReadReply window bytes)
        either stop pure (checkOracle plan window payload)
        writePrivateBytes (captured </> (label++".bin")) (B.pack payload)
        note s "window-complete" (label++";sha256="++sha256 (B.pack payload))
        modifyIORef' completed (+1)
    -- All independent resource cleanup is attempted once. A failed protocol
    -- close does not skip release/close/exit. An expired deadline prevents
    -- further I/O.
    errors <- newIORef ([] :: [String])
    let attempt label action = action `catch` \(e::SomeException) -> modifyIORef' errors (++[label++": "++displayException e])
    opened <- readIORef openedSession
    attempt "protocol close" $ case opened of
      Nothing -> note s "cleanup-unavailable" "unknown SID; no invented C4"
      Just (device,sid) -> do
        _ <- tx s (P.controlTransfer (P.control 0xc4 2 sid)) 12
        _ <- tx s (P.BulkIn (inputEndpoint (descriptors device)) 2) 2
        note s "close-reply" "opaque bytes; stock restoration unobserved"
    releaseHeld held api (\label action -> attempt label (bounded s label action))
    failures <- readIORef errors
    windows <- readIORef completed
    case (result,failures) of
      (_, _:_) -> P.reject ("cleanup uncertain: "++show failures)
      (Left (e::SomeException),[]) -> throwIO e
      (Right (),[]) -> note s "read-complete-stock-unverified" (show windows++" windows") >> pure windows
