-- Installed-image correspondence probe over D0 opcode 0x200001 function 2.
--
-- Every routine of that opcode returns without acting while adjustment index
-- 0xf7 is zero, so the session opens the gate with a config opcode 0x1a0002
-- function 3 cache write, reads the fixed probe list, restores the byte and
-- closes. Function 3 writes the adjustment cache through 0x0069fc94 before
-- any other check and posts no service command for an index other than 0x80;
-- this operation never writes index 0x80, so the gate lives only in RAM.
module FujiAF.Probe where

import Control.Exception
import Control.Monad (forM_, unless)
import qualified Data.ByteString as B
import Data.IORef
import Data.Word (Word8)
import Numeric (showHex)
import System.FilePath ((</>))
import FujiAF.Transport
import FujiAF.Read
  ( ReadWindow(..), decodeReadReply, readOpcode, readReplyBytes, readRequestBytes
  , readRequestFrame, productOracleBytes, replyError1Offset, replyError2Offset
  , replyFunctionOffset, replyLengthOffset, replyMailboxOffset, replyOffsetOffset
  , replyPageOffset, replyReservedOffset, sessionFixedTransfers, word32At, word32le )
import FujiAF.SHA256 (sha256)
import qualified FujiAF.Broker as P

-- Adjustment index 0xf7: the byte every 0x200001 routine tests at entry and
-- the byte the USB connect edge tests before registering the debug callback.
gateIndex, gateEnabled, gateDisabled :: Int
gateIndex = 0xf7
gateEnabled = 1
gateDisabled = 0

-- Config opcode 0x1a0002 function 3 (0x00d801cc): function at +0x10, mb
-- +0x14, page +0x18, in-page offset +0x1c, length +0x20, write data from
-- +0x24. The echo and status contract is the read path's.
configWriteFunction, configMailbox, configWriteLength, configReplyBytes :: Int
configWriteFunction = 3
configMailbox = 1
configWriteLength = 1
configReplyBytes = 0x124

configWriteBytes :: Int
configWriteBytes = readRequestBytes + configWriteLength

configWriteFrame :: Int -> [Word8]
configWriteFrame value = concat
  [ word32le readOpcode, replicate 12 0
  , word32le configWriteFunction, word32le configMailbox
  , word32le (gateIndex `div` 256), word32le (gateIndex `mod` 256)
  , word32le configWriteLength, [fromIntegral value] ]

-- Debug opcode 0x200001 function 2 (0x00d803cc): function at +0x10, address
-- +0x1c, length +0x20. The handler copies the source to reply+0x24 and
-- advertises 0x11c on every exit, so only 248 data bytes per reply are
-- observable; each 256-byte window is read as two 128-byte halves. The
-- address and length are echoed at reply +0x1c and +0x20.
debugOpcode, debugFunction, debugRequestBytes, debugDataOffset :: Int
debugOpcode = 0x00200001
debugFunction = 2
debugRequestBytes = 0x24
debugDataOffset = 0x24

probeHalfBytes, probeWindowBytes, probeRequestsPerWindow :: Int
probeHalfBytes = 128
probeWindowBytes = probeHalfBytes*2
probeRequestsPerWindow = 2

debugRequestFrame :: Int -> [Word8]
debugRequestFrame address = concat
  [ word32le debugOpcode, replicate 12 0
  , word32le debugFunction, word32le 0, word32le 0
  , word32le address, word32le probeHalfBytes ]

debugReplyForHalf :: Int
debugReplyForHalf = debugDataOffset + probeHalfBytes

-- The readback is the config opcode's own 256-byte window at index 0, whose
-- handler advertises 0x124 on every exit, so the whole window is observable.
readbackIndex :: Int
readbackIndex = 0

readbackWindow :: ReadWindow
readbackWindow = ReadWindow 0 readbackIndex "gate readback"

-- The adjustment cache as oracle: +0x100 carries the product string the tag-4
-- reply copies from +0x15c, +0x000 must equal the same session's config
-- readback of the same address, and +0x200 carries the 0xaa55 signature at
-- +0x202. They come first so a wrong channel stops before a code window.
cacheProductProbe, cacheReadbackProbe, cacheSignatureProbe, cacheGatedByteProbe, cacheBase :: Int
cacheProductProbe = cacheBase + 0x100
cacheReadbackProbe = cacheBase
cacheSignatureProbe = cacheBase + 0x200
cacheGatedByteProbe = cacheBase + 0x300
cacheBase = 0x41acd800

productByteOffset, signatureByteOffset :: Int
productByteOffset = 0x5c
signatureByteOffset = 0x02

data Probe = Probe { probeAddress :: Int, probePurpose :: String } deriving (Eq, Show, Read)

-- The code windows cover the boot load path: loader bases, the 1 KiB segment
-- table the boot copies to 0x0061c000, both compressed images at their
-- decoded load addresses, and the plain image's linked base and second
-- extent. The settings windows cover the stores the boot gate 0x006a75a4
-- reads: the fallback flags, the live retained header at 0x409c281c and its
-- snapshot at 0x409c29b8, whose marker byte and checksum byte decide whether
-- an ordinary boot keeps the saved settings. Ascending below the cache block.
probeList :: [Probe]
probeList =
  [ Probe cacheProductProbe "adjustment cache +0x100, product oracle"
  , Probe cacheReadbackProbe "adjustment cache +0x000, readback comparison"
  , Probe cacheSignatureProbe "adjustment cache +0x200, signature oracle"
  , Probe cacheGatedByteProbe "adjustment cache +0x300, indices 0x3b9 and 0x3ba"
  , Probe 0x00040000 "loader A load base"
  , Probe 0x000d0000 "loader B load base"
  , Probe 0x0061c000 "segment table 0x000"
  , Probe 0x0061c100 "segment table 0x100"
  , Probe 0x0061c200 "segment table 0x200"
  , Probe 0x0061c300 "segment table 0x300"
  , Probe 0x0061c400 "image 4 decoded load base"
  , Probe 0x0062c400 "image 4 +0x10000"
  , Probe 0x0081c400 "image 4 +0x200000"
  , Probe 0x0089f800 "settings fallback flags 0x0089f814 to 0x0089f834"
  , Probe 0x00bf1a00 "image 5 decoded load base"
  , Probe 0x00c01a00 "image 5 +0x10000"
  , Probe 0x01100000 "plain image load base"
  , Probe 0x011c0000 "plain image second extent base"
  , Probe 0x409c2800 "retained header live bytes at 0x409c281c"
  , Probe 0x409c2900 "retained header snapshot at 0x409c29b8" ]

probeCount :: Int
probeCount = length probeList

-- The session's fixed request sequence. The tests and the offline rehearsal
-- build their scripts from it, so a drift in this module fails a scripted run.
data ProbeRequest = GateWrite Int | GateRead String | Half Int | Close deriving (Eq, Show, Read)

gateReadbackEnabled, gateReadbackRestored :: String
gateReadbackEnabled = "gate-readback-enabled"
gateReadbackRestored = "gate-readback-restored"

probeRequests :: [ProbeRequest]
probeRequests =
  [ GateWrite gateEnabled, GateRead gateReadbackEnabled ]
  ++ concatMap (\probe -> [Half (probeAddress probe), Half (probeAddress probe + probeHalfBytes)]) probeList
  ++ [ GateWrite gateDisabled, GateRead gateReadbackRestored, Close ]

probeName :: Int -> String
probeName address = replicate (8-length digits) '0' ++ digits
  where digits = showHex address ""

probeJournalCapBytes :: Int
probeJournalCapBytes = 262144

transfersPerRequest :: Int
transfersPerRequest = 4

probeConfigSteps :: Int
probeConfigSteps = 4

probeAggregateDeadlineMs :: Int
probeAggregateDeadlineMs = transferTimeoutMs
  *(sessionFixedTransfers + transfersPerRequest*probeConfigSteps + transfersPerRequest*probeRequestsPerWindow*probeCount)

decodeConfigWriteReply :: [Word8] -> Either String ()
decodeConfigWriteReply bytes
  | length bytes /= configReplyBytes = Left ("reply length "++show (length bytes)++" differs from the requested "++show configReplyBytes)
  | word32At 0 bytes /= readOpcode = Left "reply opcode differs from the request"
  | word32At replyReservedOffset bytes /= 0 = Left "reply reserved word is nonzero"
  | word32At replyFunctionOffset bytes /= configWriteFunction = Left "reply function differs from the request"
  | word32At replyMailboxOffset bytes /= configMailbox = Left "reply mailbox differs from the request"
  | otherwise = case (word32At replyError1Offset bytes, word32At replyError2Offset bytes) of
      (0,0) | echoed == requested -> Right ()
            | otherwise -> Left ("echoed page/offset/length "++show echoed++" differs from the request "++show requested)
      (9,3) -> Left "rejected arguments: error1 9, error2 3"
      (1,0xffff) -> Left "callback status 2: error1 1, error2 0xffff"
      (first,second) -> Left ("status the handler cannot produce: error1 "++show first++", error2 "++show second)
  where
    echoed = (word32At replyPageOffset bytes, word32At replyOffsetOffset bytes, word32At replyLengthOffset bytes)
    requested = (gateIndex `div` 256, gateIndex `mod` 256, configWriteLength)

decodeDebugReply :: Int -> [Word8] -> Either String [Word8]
decodeDebugReply address bytes
  | length bytes /= debugReplyForHalf = Left ("reply length "++show (length bytes)++" differs from the requested "++show debugReplyForHalf)
  | word32At 0 bytes /= debugOpcode = Left "reply opcode differs from the request"
  | word32At replyReservedOffset bytes /= 0 = Left "reply reserved word is nonzero"
  | word32At replyFunctionOffset bytes /= debugFunction = Left "reply function differs from the request"
  | otherwise = case (word32At replyError1Offset bytes, word32At replyError2Offset bytes) of
      (0,0) | echoed == (address,probeHalfBytes) -> Right (take probeHalfBytes (drop debugDataOffset bytes))
            | otherwise -> Left ("echoed address/length "++show echoed++" differs from the request "++show (address,probeHalfBytes))
      (9,3) -> Left "rejected arguments: error1 9, error2 3"
      (1,0xffff) -> Left "closed gate: error1 1, error2 0xffff"
      (first,second) -> Left ("status the handler cannot produce: error1 "++show first++", error2 "++show second)
  where
    echoed = (word32At 0x1c bytes, word32At 0x20 bytes)

gateByteOf :: [Word8] -> Word8
gateByteOf bytes = bytes !! gateIndex

checkProbeOracle :: [Word8] -> Probe -> [Word8] -> Either String ()
checkProbeOracle readback probe window
  | address == cacheProductProbe
  , take (length productOracleBytes) (drop productByteOffset window) == productOracleBytes = Right ()
  | address == cacheProductProbe = Left "product oracle mismatch; channel or cache differs"
  | address == cacheReadbackProbe
  , window == readback = Right ()
  | address == cacheReadbackProbe = Left "cache window differs from the same-session config readback"
  | address == cacheSignatureProbe
  , take 2 (drop signatureByteOffset window) == [0x55,0xaa] = Right ()
  | address == cacheSignatureProbe = Left "signature oracle mismatch"
  | otherwise = Right ()
  where address = probeAddress probe

runProbe :: Clock -> UsbApi -> FilePath -> IO Int
runProbe clock api dir = runProbeCore clock api dir `catch` \(e::SomeException) -> do
  _ <- try (recordWithin probeJournalCapBytes dir "session-uncertain" (take 4096 (displayException e))) :: IO (Either SomeException ())
  throwIO e

runProbeCore :: Clock -> UsbApi -> FilePath -> IO Int
runProbeCore clock api dir = do
  let captured = dir </> "probes"
  s <- newSession clock api dir probeJournalCapBytes probeAggregateDeadlineMs
  held <- newHeld
  openedSession <- newIORef Nothing
  completed <- newIORef (0::Int)
  note s "prepared" ("probe-ram;windows="++show probeCount++";gate-index="++show gateIndex)
  ensurePrivateDirectory captured
  mask $ \restore -> do
    result <- try $ restore $ do
      device <- acquireDevice s held
      let d = descriptors device
          input = inputEndpoint d
          output = outputEndpoint d
      _ <- tx s (P.controlTransfer (P.control 0xc0 8 0)) 12
      reply <- tx s (P.BulkIn input 8) 8
      -- C0 bytes 2..7 are opaque; every D1 length here is fixed by its handler.
      let sid = fromIntegral (reply!!0) + 256*fromIntegral (reply!!1)
      writeIORef openedSession (Just (device,sid))
      let refuse label response reason = do
            note s "probe-refused" (label++";reply="++P.hexBytes response++";"++reason)
            P.reject reason
          writeGate value = do
            _ <- tx s (P.controlTransfer (P.control 0xd0 configWriteBytes sid)) 12
            _ <- tx s (P.BulkOut output (configWriteFrame value)) configWriteBytes
            _ <- tx s (P.controlTransfer (P.control 0xd1 configReplyBytes sid)) 12
            bytes <- tx s (P.BulkIn input configReplyBytes) configReplyBytes
            either (refuse "gate-write" bytes) pure (decodeConfigWriteReply bytes)
            note s "gate-write-complete" ("index="++show gateIndex++";value="++show value)
          readGateWindow label = do
            _ <- tx s (P.controlTransfer (P.control 0xd0 readRequestBytes sid)) 12
            _ <- tx s (P.BulkOut output (readRequestFrame readbackWindow)) readRequestBytes
            _ <- tx s (P.controlTransfer (P.control 0xd1 readReplyBytes sid)) 12
            bytes <- tx s (P.BulkIn input readReplyBytes) readReplyBytes
            payload <- either (refuse label bytes) pure (decodeReadReply readbackWindow bytes)
            writePrivateBytes (captured </> (label++".bin")) (B.pack payload)
            note s "readback-complete" (label++";sha256="++sha256 (B.pack payload))
            pure payload
          debugHalf address = do
            _ <- tx s (P.controlTransfer (P.control 0xd0 debugRequestBytes sid)) 12
            _ <- tx s (P.BulkOut output (debugRequestFrame address)) debugRequestBytes
            _ <- tx s (P.controlTransfer (P.control 0xd1 debugReplyForHalf sid)) 12
            bytes <- tx s (P.BulkIn input debugReplyForHalf) debugReplyForHalf
            either (refuse (probeName address) bytes) pure (decodeDebugReply address bytes)
      writeGate gateEnabled
      enabled <- readGateWindow gateReadbackEnabled
      unless (gateByteOf enabled == fromIntegral gateEnabled) (P.reject "gate enable is not visible in the config readback")
      forM_ probeList $ \probe -> do
        first <- debugHalf (probeAddress probe)
        second <- debugHalf (probeAddress probe + probeHalfBytes)
        let window = first++second
            label = probeName (probeAddress probe)
        either (refuse label window) pure (checkProbeOracle enabled probe window)
        writePrivateBytes (captured </> (label++".bin")) (B.pack window)
        note s "window-complete" (label++";purpose="++probePurpose probe++";sha256="++sha256 (B.pack window))
        modifyIORef' completed (+1)
      writeGate gateDisabled
      restored <- readGateWindow gateReadbackRestored
      unless (gateByteOf restored == fromIntegral gateDisabled) (P.reject "gate restore is not visible in the config readback")
      note s "gate-restored" ("index="++show gateIndex)
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
    probes <- readIORef completed
    case (result,failures) of
      (_, _:_) -> P.reject ("cleanup uncertain: "++show failures)
      (Left (e::SomeException),[]) -> throwIO e
      (Right (),[]) -> note s "probe-complete-stock-unverified" (show probes++" probes") >> pure probes
