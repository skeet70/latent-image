-- Boot-ROM lead read over D0 opcode 0x200001 function 2: C0 open, the
-- cache-only adjustment index 0xf7 enable, its config readback, the fixed
-- boot-ROM window list, the restore write, its readback and C4.
--
-- The window list is the pinned fffw bootrom lead: 0x4000 bytes at
-- 0x80000000 with a signature at +0x3ff0 and a digest over the whole range
-- (ff80/ff80_cmd/cmd.py at fffw bedc091e). That command's model allowlist
-- names X-M1, but its address, extent and markers are upstream leads rather
-- than X-M1 evidence, so the operation reads the whole range and judges the
-- contents host-side against the lead. Its in-session checks are the reply
-- contracts, the two gate readbacks and the journaled hashes, so no window
-- here is an oracle.
module FujiAF.BootRom where

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

-- The lead range: base, extent and window size. The windows tile the whole
-- 0x4000 bytes, so the +0x3ff0 signature falls inside the last window.
-- The repeat windows tile the suspected second copy at 0x80004000-0x80007fff,
-- the period window at 0x80008000 discriminates wrap from wider content, and
-- the handoff window at 0x00000000 holds the boot record the ROM jumps to.
bootRomOrigin, bootRomBytes, bootRomWindowBytes :: Int
bootRomOrigin = 0x80000000
bootRomBytes = 0x4000
bootRomWindowBytes = 256

bootRomRepeatWindows :: [Int]
bootRomRepeatWindows = [bootRomOrigin+bootRomBytes, bootRomOrigin+bootRomBytes+bootRomWindowBytes .. bootRomOrigin+2*bootRomBytes-bootRomWindowBytes]

bootRomPeriodWindows :: [Int]
bootRomPeriodWindows = [bootRomOrigin+2*bootRomBytes]

bootRomHandoffWindows :: [Int]
bootRomHandoffWindows = [0x00000000]

bootRomWindows :: [Int]
bootRomWindows =
  [bootRomOrigin, bootRomOrigin+bootRomWindowBytes .. bootRomOrigin+bootRomBytes-bootRomWindowBytes]
  ++ bootRomRepeatWindows ++ bootRomPeriodWindows ++ bootRomHandoffWindows

bootRomWindowCount :: Int
bootRomWindowCount = length bootRomWindows

-- The session's request sequence, so the tests and the offline rehearsal build
-- their scripts from the operation's own shape.
bootRomRequests :: [ProbeRequest]
bootRomRequests =
  [ GateWrite gateEnabled, GateRead gateReadbackEnabled ]
  ++ concatMap (\base -> [Half base, Half (base + probeHalfBytes)]) bootRomWindows
  ++ [ GateWrite gateDisabled, GateRead gateReadbackRestored, Close ]

bootRomJournalCapBytes :: Int
bootRomJournalCapBytes = 1048576

bootRomAggregateDeadlineMs :: Int
bootRomAggregateDeadlineMs = transferTimeoutMs
  *(sessionFixedTransfers + transfersPerRequest*probeConfigSteps + transfersPerRequest*2*bootRomWindowCount)

runProbeBootRom :: Clock -> UsbApi -> FilePath -> IO Int
runProbeBootRom clock api dir = runProbeBootRomCore clock api dir `catch` \(e::SomeException) -> do
  _ <- try (recordWithin bootRomJournalCapBytes dir "session-uncertain" (take 4096 (displayException e))) :: IO (Either SomeException ())
  throwIO e

runProbeBootRomCore :: Clock -> UsbApi -> FilePath -> IO Int
runProbeBootRomCore clock api dir = do
  let captured = dir </> "windows"
  s <- newSession clock api dir bootRomJournalCapBytes bootRomAggregateDeadlineMs
  held <- newHeld
  openedSession <- newIORef Nothing
  completed <- newIORef (0::Int)
  note s "prepared" ("probe-bootrom;windows="++show bootRomWindowCount++";gate-index="++show gateIndex)
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
            note s "boot-rom-refused" (label++";reply="++P.hexBytes response++";"++reason)
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
          bootRomHalf address = do
            _ <- tx s (P.controlTransfer (P.control 0xd0 debugRequestBytes sid)) 12
            _ <- tx s (P.BulkOut output (debugRequestFrame address)) debugRequestBytes
            _ <- tx s (P.controlTransfer (P.control 0xd1 debugReplyForHalf sid)) 12
            bytes <- tx s (P.BulkIn input debugReplyForHalf) debugReplyForHalf
            either (refuse (probeName address) bytes) pure (decodeDebugReply address bytes)
      writeGate gateEnabled
      enabled <- readGateWindow gateReadbackEnabled
      unless (gateByteOf enabled == fromIntegral gateEnabled) (P.reject "gate enable is not visible in the config readback")
      forM_ bootRomWindows $ \base -> do
        first <- bootRomHalf base
        second <- bootRomHalf (base + probeHalfBytes)
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
      (Right (),[]) -> note s "boot-rom-complete-stock-unverified" (show windows++" windows") >> pure windows
