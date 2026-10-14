-- The 0x400a0800 log store read over D0 opcode 0x200001 function 2: C0 open,
-- the cache-only adjustment index 0xf7 enable, its config readback, the fixed
-- store-window list, the restore write, its readback and C4.
--
-- The store is loader A's: writer 0x42188 appends 0x10-byte entries at
-- 0x400a0800 + next_index*0x10 + 0x34, under a header holding the capacity
-- halfword 0x1ffc at +0x1c and the next index at +0x1e, which wraps to zero at
-- capacity. An entry holds {type byte 0x72, thread byte = the low byte of
-- TPIDRURW or 0 when its sign bit is set, delta halfword = [0xFFF91004] minus
-- [0xFFF91000], tick word = *(0x400c0800), one eight-character string prefix at
-- +0x3c}. The store is 0x1fff4 bytes and the operation reads it whole, 8188
-- slots in 512 windows, plus the tick cell at 0x400c0800 read twice in a row
-- before the store and once after it. The first store window carries both
-- header halfwords. The window bytes are judged host-side; the in-session
-- checks are the reply contracts, the two gate readbacks and the journaled
-- hashes, so no window here is an oracle. Whether this store is the console
-- ring that opcode 0x200001 functions 1 and 4 drain at *(0x409c1e2c) is
-- unestablished.
module FujiAF.Log where

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

-- The store's extent and the fixed read list. The capacity halfword 0x1ffc
-- bounds 8188 slots from +0x34, so the store runs to 0x400c07f3; 512 windows
-- cover it whole, the last window's final twelve bytes lying past its end. Two
-- maintenance boots occupied 819 and 4050 slots, so a bounded prefix cannot be
-- relied on. Window 0 carries both header halfwords, so a run shows the ring's
-- occupancy and any wrap.
--
-- The list also samples the tick cell at 0x400c0800 three times: twice in a
-- row before the store and once after it. The cell is ordinary RAM in the
-- store's region: every loader copy zeroes it and registers the increment
-- callback 0x40b24 as id 32 through 0x4d8fc, while the loader initializers at
-- 0x4089c/0xd089c publish its address into interface slot +0x174. The same
-- per-core check at 0x427b8, taken in the initializer and again in the
-- callback, leaves one core to derive the cell as [0xFFF91000] divided by 1875
-- and to bump it, so a debug read copies bytes and nothing latches or clears.
-- The adjacent pair measures the rate against the host clock over one window,
-- which keeps a 16-bit counter from aliasing, and the closing sample checks
-- that rate across the session.
logOrigin, logStoreBytes, logWindowBytes, logWindowCount, logTickAddress :: Int
logOrigin = 0x400a0800
logStoreBytes = 0x1fff4
logWindowBytes = 256
logWindowCount = 512
logTickAddress = 0x400c0800

storeWindows :: [Int]
storeWindows = take logWindowCount [logOrigin, logOrigin+logWindowBytes ..]

-- Labeled windows, because the three tick samples share one address and would
-- otherwise overwrite each other's capture file.
logWindows :: [(String, Int)]
logWindows =
  [ ("tick-a", logTickAddress), ("tick-b", logTickAddress) ]
  ++ [(probeName a, a) | a <- storeWindows]
  ++ [("tick-end", logTickAddress)]

-- The session's request sequence, so the tests and the offline rehearsal build
-- their scripts from the operation's own shape.
logRequests :: [ProbeRequest]
logRequests =
  [ GateWrite gateEnabled, GateRead gateReadbackEnabled ]
  ++ concatMap (\(_, base) -> [Half base, Half (base + probeHalfBytes)]) logWindows
  ++ [ GateWrite gateDisabled, GateRead gateReadbackRestored, Close ]

-- 4 MiB covers the whole-store session: 4,340 bytes per window measured on a
-- 32-window run, so 515 windows project to about 2.25 MB.
logJournalCapBytes :: Int
logJournalCapBytes = 4194304

logAggregateDeadlineMs :: Int
logAggregateDeadlineMs = transferTimeoutMs
  *(sessionFixedTransfers + transfersPerRequest*probeConfigSteps + transfersPerRequest*2*length logWindows)

runReadLog :: Clock -> UsbApi -> FilePath -> IO Int
runReadLog clock api dir = runReadLogCore clock api dir `catch` \(e::SomeException) -> do
  _ <- try (recordWithin logJournalCapBytes dir "session-uncertain" (take 4096 (displayException e))) :: IO (Either SomeException ())
  throwIO e

runReadLogCore :: Clock -> UsbApi -> FilePath -> IO Int
runReadLogCore clock api dir = do
  let captured = dir </> "windows"
  s <- newSession clock api dir logJournalCapBytes logAggregateDeadlineMs
  held <- newHeld
  openedSession <- newIORef Nothing
  completed <- newIORef (0::Int)
  note s "prepared" ("read-log;windows="++show (length logWindows)++";tick="++probeName logTickAddress++";gate-index="++show gateIndex)
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
            note s "log-refused" (label++";reply="++P.hexBytes response++";"++reason)
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
          logHalf address = do
            _ <- tx s (P.controlTransfer (P.control 0xd0 debugRequestBytes sid)) 12
            _ <- tx s (P.BulkOut output (debugRequestFrame address)) debugRequestBytes
            _ <- tx s (P.controlTransfer (P.control 0xd1 debugReplyForHalf sid)) 12
            bytes <- tx s (P.BulkIn input debugReplyForHalf) debugReplyForHalf
            either (refuse (probeName address) bytes) pure (decodeDebugReply address bytes)
      writeGate gateEnabled
      enabled <- readGateWindow gateReadbackEnabled
      unless (gateByteOf enabled == fromIntegral gateEnabled) (P.reject "gate enable is not visible in the config readback")
      forM_ logWindows $ \(label, base) -> do
        first <- logHalf base
        second <- logHalf (base + probeHalfBytes)
        let window = first++second
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
      (Right (),[]) -> note s "log-complete-stock-unverified" (show windows++" windows") >> pure windows
