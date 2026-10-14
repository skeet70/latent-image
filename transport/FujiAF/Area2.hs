-- Area-2 maintenance-boot RAM read over D0 opcode 0x200001 function 2: C0
-- open, the cache-only adjustment index 0xf7 enable, its config readback, the
-- fixed area-2 window list, the restore write, its readback and C4.
--
-- The window list is the future-evidence set of the area-2 invalidation
-- analysis (private-data/analysis/x-m1-1.50/end-setting/followup/
-- area2-invalidation.md section 9): the pending and snapshot settings records,
-- the area-2 and area-3 objects, the fixed area-2 record buffer, the
-- read-cache state, the backend bank and offset words, the storage-readiness
-- words, both cache slot buffers and the block-0x28 directory journal. Those
-- pages hold RAM copies and working state whose expected values are unproved,
-- so the operation reads the whole list and judges the contents host-side. Its
-- in-session checks are the reply contracts, the two gate readbacks and the
-- journaled hashes, so no window here is an oracle.
module FujiAF.Area2 where

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

-- The fixed window list: 27 256-byte windows in strictly increasing address
-- order, each read as two 128-byte halves at +0x00 and +0x80. The entries are
-- the section 9 evidence table and its journal extension: storage readiness at
-- 0x00828500, the lower-object pool with the area-2 object at 0x0089f900 and
-- its tail and the area-3 object at 0x0089fa00, cache slot 0 at
-- 0x008a0000-0x008a0100 and cache slot 1 at 0x008c0000-0x008c0100, the
-- read-cache tags and ages at 0x008e0000, the fixed area-2 record buffer at
-- 0x008e1700-0x008e1800, the backend bank and offset words at 0x00917f00, the
-- eleven block-0x28 directory journal windows 0x00919b00-0x0091a500, and the
-- pending and snapshot settings records 0x409c2b00-0x409c2f00.
area2Windows :: [Int]
area2Windows =
  [ 0x00828500
  , 0x0089f900
  , 0x0089fa00
  , 0x008a0000
  , 0x008a0100
  , 0x008c0000
  , 0x008c0100
  , 0x008e0000
  , 0x008e1700
  , 0x008e1800
  , 0x00917f00
  , 0x00919b00
  , 0x00919c00
  , 0x00919d00
  , 0x00919e00
  , 0x00919f00
  , 0x0091a000
  , 0x0091a100
  , 0x0091a200
  , 0x0091a300
  , 0x0091a400
  , 0x0091a500
  , 0x409c2b00
  , 0x409c2c00
  , 0x409c2d00
  , 0x409c2e00
  , 0x409c2f00 ]

area2WindowCount :: Int
area2WindowCount = length area2Windows

-- The session's request sequence, so the offline rehearsal builds its script
-- from the operation's own shape.
area2Requests :: [ProbeRequest]
area2Requests =
  [ GateWrite gateEnabled, GateRead gateReadbackEnabled ]
  ++ concatMap (\base -> [Half base, Half (base + probeHalfBytes)]) area2Windows
  ++ [ GateWrite gateDisabled, GateRead gateReadbackRestored, Close ]

-- 8192 journal bytes per window, the rate behind read-log's 4 MiB over 512
-- windows and probe-bootrom's 512 KiB over 64; 27 windows bound each journal
-- file at 221184 bytes. The measured session volume is 4,340 bytes per window.
area2JournalCapBytes :: Int
area2JournalCapBytes = 221184

area2AggregateDeadlineMs :: Int
area2AggregateDeadlineMs = transferTimeoutMs
  *(sessionFixedTransfers + transfersPerRequest*probeConfigSteps + transfersPerRequest*2*area2WindowCount)

runProbeArea2 :: Clock -> UsbApi -> FilePath -> IO Int
runProbeArea2 clock api dir = runProbeArea2Core clock api dir `catch` \(e::SomeException) -> do
  _ <- try (recordWithin area2JournalCapBytes dir "session-uncertain" (take 4096 (displayException e))) :: IO (Either SomeException ())
  throwIO e

runProbeArea2Core :: Clock -> UsbApi -> FilePath -> IO Int
runProbeArea2Core clock api dir = do
  let captured = dir </> "probes"
  s <- newSession clock api dir area2JournalCapBytes area2AggregateDeadlineMs
  held <- newHeld
  openedSession <- newIORef Nothing
  completed <- newIORef (0::Int)
  note s "prepared" ("probe-area2;windows="++show area2WindowCount++";gate-index="++show gateIndex)
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
            note s "area2-refused" (label++";reply="++P.hexBytes response++";"++reason)
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
          area2Half address = do
            _ <- tx s (P.controlTransfer (P.control 0xd0 debugRequestBytes sid)) 12
            _ <- tx s (P.BulkOut output (debugRequestFrame address)) debugRequestBytes
            _ <- tx s (P.controlTransfer (P.control 0xd1 debugReplyForHalf sid)) 12
            bytes <- tx s (P.BulkIn input debugReplyForHalf) debugReplyForHalf
            either (refuse (probeName address) bytes) pure (decodeDebugReply address bytes)
      writeGate gateEnabled
      enabled <- readGateWindow gateReadbackEnabled
      unless (gateByteOf enabled == fromIntegral gateEnabled) (P.reject "gate enable is not visible in the config readback")
      forM_ area2Windows $ \base -> do
        first <- area2Half base
        second <- area2Half (base + probeHalfBytes)
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
      (Right (),[]) -> note s "area2-complete-stock-unverified" (show windows++" windows") >> pure windows
