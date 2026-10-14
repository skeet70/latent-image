-- The two ownership reads over D0 opcode 0x200001 function 2, in the session
-- shape FujiAF.Probe uses: C0 open, the cache-only adjustment index 0xf7 gate
-- enable and readback, the fixed window list, the restore write and readback,
-- C4.
--
-- The list covers the two structures the recipe-bank boot hook's free-id claim
-- and the 64 KiB carve's headroom argument rest on. The semaphore-namespace
-- table has base 0x0093e6f4 with one word per id at base + 4*(id-1) for ids
-- 1..169, so its bytes run 0x0093e6f4..0x0093e997 and the windows
-- 0x0093e600, 0x0093e700, 0x0093e800 and 0x0093e900 carry ids 1-3, 4-67,
-- 68-131 and 132-169. The bump allocator cursor cell at 0x00829a08 sits at
-- +8 of window 0x00829a00, and the adjacent window 0x00829900 is read for
-- allocator context.
--
-- Namespace words record routing owners, not a live-object bitmap.
-- Destruction clears entries, and some fixed-id creation paths do not update
-- them. A late zero does not establish that an id was free at hook time.
-- The cursor is the current allocation break, not a proven high-water mark.
-- These sequential reads do not establish hook-time ownership or shooting
-- heap demand. No window has a content oracle. A stop before the restore
-- leaves adjustment index 0xf7 at 1 until the next power cycle.
module FujiAF.Ownership where

import Control.Exception
import Control.Monad (forM_, unless)
import qualified Data.ByteString as B
import Data.IORef
import System.FilePath ((</>))
import FujiAF.Transport
import FujiAF.Read
  ( decodeReadReply, readReplyBytes, readRequestBytes, readRequestFrame
  , sessionFixedTransfers )
import FujiAF.Probe
  ( ProbeRequest(..), configReplyBytes, configWriteBytes, configWriteFrame
  , debugReplyForHalf, debugRequestBytes, debugRequestFrame
  , decodeConfigWriteReply, decodeDebugReply, gateByteOf, gateDisabled
  , gateEnabled, gateIndex, gateReadbackEnabled, gateReadbackRestored
  , probeConfigSteps, probeHalfBytes, probeName, readbackWindow
  , transfersPerRequest )
import FujiAF.SHA256 (sha256)
import qualified FujiAF.Broker as P

-- The fixed window list, in strictly increasing address order, each read as
-- two 128-byte halves at +0x00 and +0x80.
ownershipWindows :: [Int]
ownershipWindows =
  [ 0x00829900
  , 0x00829a00
  , 0x0093e600
  , 0x0093e700
  , 0x0093e800
  , 0x0093e900 ]

ownershipWindowCount :: Int
ownershipWindowCount = length ownershipWindows

-- The session's request sequence, so the tests and the offline rehearsal
-- build their scripts from the operation's own shape.
ownershipRequests :: [ProbeRequest]
ownershipRequests =
  [ GateWrite gateEnabled, GateRead gateReadbackEnabled ]
  ++ concatMap (\base -> [Half base, Half (base + probeHalfBytes)]) ownershipWindows
  ++ [ GateWrite gateDisabled, GateRead gateReadbackRestored, Close ]

-- 8192 journal bytes per window, the rate behind read-log's 4 MiB over 512
-- windows; 6 windows bound each journal file at 49152 bytes.
ownershipJournalCapBytes :: Int
ownershipJournalCapBytes = 8192*ownershipWindowCount

ownershipAggregateDeadlineMs :: Int
ownershipAggregateDeadlineMs = transferTimeoutMs
  *(sessionFixedTransfers + transfersPerRequest*probeConfigSteps + transfersPerRequest*2*ownershipWindowCount)

runProbeOwnership :: Clock -> UsbApi -> FilePath -> IO Int
runProbeOwnership clock api dir = runProbeOwnershipCore clock api dir `catch` \(e::SomeException) -> do
  _ <- try (recordWithin ownershipJournalCapBytes dir "session-uncertain" (take 4096 (displayException e))) :: IO (Either SomeException ())
  throwIO e

runProbeOwnershipCore :: Clock -> UsbApi -> FilePath -> IO Int
runProbeOwnershipCore clock api dir = do
  let captured = dir </> "windows"
  s <- newSession clock api dir ownershipJournalCapBytes ownershipAggregateDeadlineMs
  held <- newHeld
  openedSession <- newIORef Nothing
  completed <- newIORef (0::Int)
  note s "prepared" ("probe-ownership;windows="++show ownershipWindowCount++";gate-index="++show gateIndex)
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
            note s "ownership-refused" (label++";reply="++P.hexBytes response++";"++reason)
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
          ownershipHalf address = do
            _ <- tx s (P.controlTransfer (P.control 0xd0 debugRequestBytes sid)) 12
            _ <- tx s (P.BulkOut output (debugRequestFrame address)) debugRequestBytes
            _ <- tx s (P.controlTransfer (P.control 0xd1 debugReplyForHalf sid)) 12
            bytes <- tx s (P.BulkIn input debugReplyForHalf) debugReplyForHalf
            either (refuse (probeName address) bytes) pure (decodeDebugReply address bytes)
      writeGate gateEnabled
      enabled <- readGateWindow gateReadbackEnabled
      unless (gateByteOf enabled == fromIntegral gateEnabled) (P.reject "gate enable is not visible in the config readback")
      forM_ ownershipWindows $ \base -> do
        first <- ownershipHalf base
        second <- ownershipHalf (base + probeHalfBytes)
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
      (Right (),[]) -> note s "ownership-read-complete-stock-unverified" (show windows++" windows") >> pure windows
