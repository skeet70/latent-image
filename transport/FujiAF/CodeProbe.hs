-- Bounded RAM read of the G3 stock case's executed closure over D0 opcode
-- 0x200001 function 2: C0 open, the cache-only adjustment index 0xf7 enable,
-- its config readback, the fixed code window list, the restore write, its
-- readback and C4.
--
-- The list covers every byte of the five function spans the stock case
-- executes (private-data/analysis/x-m1-1.50/native-case/area3-validator/):
-- the parity builder at 0x006a83f4 size 0x90 and the validator at 0x006a84d4
-- size 0x188, then the word-fill at 0x007221a0 size 0x28, memset at
-- 0x007221c8 size 0x50 and memcmp at 0x0072232c size 0xb8. The four windows
-- first named as the closure path leave three tails unread: the parity
-- builder's first 12 bytes 0x006a83f4-0x006a83ff, the validator's last 92
-- bytes 0x006a8600-0x006a865b and memset's last 24 bytes
-- 0x00722200-0x00722217. The added windows 0x006a8300, 0x006a8600 and
-- 0x00722200 close those gaps, so every function byte lies in a window.
--
-- The correspondence of these bytes to the image-4 decode is already
-- established by E-DUMP-RAM-150: the reviewed 2026-10-08 id4-image dumps
-- match the decode with zero differences over exactly these seven windows
-- (dump offset 0x8c300 against decode offset 0x8bf00 for 1024 bytes, and
-- dump offset 0x6100 against decode offset 0x105d00 for 768 bytes), and the
-- case's own code-validator.bin and code-runtime.bin are identical to the
-- decode at +0x8bff0 and +0x105da0 of lz/id4-0061c400.bin. This operation
-- re-confirms the installed bytes on a fresh boot in one bounded session;
-- the window bytes are judged host-side against the decode and no window
-- here is an oracle: the in-session checks are the reply contracts, the two
-- gate readbacks and the journaled hashes. A stop before the restore leaves
-- adjustment index 0xf7 at 1 until the next power cycle.
module FujiAF.CodeProbe where

import Control.Exception
import Control.Monad (forM_, unless)
import qualified Data.ByteString as B
import Data.IORef
import System.FilePath ((</>))
import FujiAF.Transport
import FujiAF.Read
  ( ReadWindow(..), decodeReadReply, readReplyBytes, readRequestBytes
  , readRequestFrame, sessionFixedTransfers )
import FujiAF.Probe
  ( ProbeRequest(..), configReplyBytes, configWriteBytes, configWriteFrame
  , debugReplyForHalf, debugRequestBytes, debugRequestFrame
  , decodeConfigWriteReply, decodeDebugReply, gateByteOf, gateDisabled
  , gateEnabled, gateIndex, gateReadbackEnabled, gateReadbackRestored
  , probeConfigSteps, probeHalfBytes, probeName, transfersPerRequest )
import FujiAF.SHA256 (sha256)
import qualified FujiAF.Broker as P

-- The fixed window list: the four windows named as the G3 closure path
-- (0x006a8400, 0x006a8500, 0x00722100 and 0x00722300) plus the three that
-- close the remaining function tails, in strictly increasing address order,
-- each read as two 128-byte halves at +0x00 and +0x80.
codeWindows :: [Int]
codeWindows =
  [ 0x006a8300
  , 0x006a8400
  , 0x006a8500
  , 0x006a8600
  , 0x00722100
  , 0x00722200
  , 0x00722300 ]

codeWindowCount :: Int
codeWindowCount = length codeWindows

-- The session's request sequence, so the offline rehearsal builds its script
-- from the operation's own shape.
codeRequests :: [ProbeRequest]
codeRequests =
  [ GateWrite gateEnabled, GateRead gateReadbackEnabled ]
  ++ concatMap (\base -> [Half base, Half (base + probeHalfBytes)]) codeWindows
  ++ [ GateWrite gateDisabled, GateRead gateReadbackRestored, Close ]

-- 8192 journal bytes per window, the rate behind read-log's 4 MiB over 512
-- windows; 7 windows bound each journal file at 57344 bytes.
codeJournalCapBytes :: Int
codeJournalCapBytes = 8192*codeWindowCount

codeAggregateDeadlineMs :: Int
codeAggregateDeadlineMs = transferTimeoutMs
  *(sessionFixedTransfers + transfersPerRequest*probeConfigSteps + transfersPerRequest*2*codeWindowCount)

runProbeCode :: Clock -> UsbApi -> FilePath -> IO Int
runProbeCode clock api dir = runProbeCodeCore clock api dir `catch` \(e::SomeException) -> do
  _ <- try (recordWithin codeJournalCapBytes dir "session-uncertain" (take 4096 (displayException e))) :: IO (Either SomeException ())
  throwIO e

runProbeCodeCore :: Clock -> UsbApi -> FilePath -> IO Int
runProbeCodeCore clock api dir = do
  let captured = dir </> "windows"
  s <- newSession clock api dir codeJournalCapBytes codeAggregateDeadlineMs
  held <- newHeld
  openedSession <- newIORef Nothing
  completed <- newIORef (0::Int)
  note s "prepared" ("probe-code;windows="++show codeWindowCount++";gate-index="++show gateIndex)
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
            note s "code-probe-refused" (label++";reply="++P.hexBytes response++";"++reason)
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
            _ <- tx s (P.BulkOut output (readRequestFrame (ReadWindow 0 0 label))) readRequestBytes
            _ <- tx s (P.controlTransfer (P.control 0xd1 readReplyBytes sid)) 12
            bytes <- tx s (P.BulkIn input readReplyBytes) readReplyBytes
            payload <- either (refuse label bytes) pure (decodeReadReply (ReadWindow 0 0 label) bytes)
            writePrivateBytes (captured </> (label++".bin")) (B.pack payload)
            note s "readback-complete" (label++";sha256="++sha256 (B.pack payload))
            pure payload
          codeHalf address = do
            _ <- tx s (P.controlTransfer (P.control 0xd0 debugRequestBytes sid)) 12
            _ <- tx s (P.BulkOut output (debugRequestFrame address)) debugRequestBytes
            _ <- tx s (P.controlTransfer (P.control 0xd1 debugReplyForHalf sid)) 12
            bytes <- tx s (P.BulkIn input debugReplyForHalf) debugReplyForHalf
            either (refuse (probeName address) bytes) pure (decodeDebugReply address bytes)
      writeGate gateEnabled
      enabled <- readGateWindow gateReadbackEnabled
      unless (gateByteOf enabled == fromIntegral gateEnabled) (P.reject "gate enable is not visible in the config readback")
      forM_ codeWindows $ \base -> do
        first <- codeHalf base
        second <- codeHalf (base + probeHalfBytes)
        let window = first++second
            label = probeName base
        writePrivateBytes (captured </> (label++".bin")) (B.pack window)
        note s "window-complete" (label++";sha256="++sha256 (B.pack window))
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
    windows <- readIORef completed
    case (result,failures) of
      (_, _:_) -> P.reject ("cleanup uncertain: "++show failures)
      (Left (e::SomeException),[]) -> throwIO e
      (Right (),[]) -> note s "code-probe-complete-stock-unverified" (show windows++" windows") >> pure windows
