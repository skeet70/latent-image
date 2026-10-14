-- The marker-0 route capture over D0 opcode 0x200001 function 2, in the
-- session shape FujiAF.Ownership uses: C0 open, the cache-only adjustment
-- index 0xf7 gate enable and readback, the window list, the restore write and
-- readback, C4.
--
-- Phase 1 (routeFixedWindows) reads the class descriptor table and its
-- adjacent objects at 0x0096bd20, 256 bytes, and the allocation-search cursor
-- cell at 0x00a0f86c. Phase 2 reads the cursor cell again plus, per distinct
-- class descriptor word p, the method windows p+0x60 (32 B) and p+0x140 (40 B)
-- that its plan file lists; the host derives that list from phase 1's first two
-- words, deduplicated and skipping cursor collisions
-- (transport/marker0-read-session.sh, reports/marker0-read-plan.md).
--
-- One request per short window and two per 256-byte window: function 2 copies
-- the source to reply+0x24 and advertises 0x11c on every exit, so only 248
-- data bytes per reply are observable and a 256-byte window cannot take one
-- request. The 4, 32 and 40-byte windows take one request at their own length,
-- the shape the 4-byte probe-straps reads use.
--
-- The windows are RAM snapshots, not an oracle. Only the class descriptor pair
-- in window 0 is dereferenced as struct pointers host-side, and the cursor
-- cell's value is classified by the two search families' domains. A stop before
-- the restore leaves adjustment index 0xf7 at 1 until the next power cycle.
module FujiAF.Routes where

import Control.Exception
import Control.Monad (forM_, unless)
import qualified Data.ByteString as B
import Data.IORef
import Data.Word (Word8)
import System.FilePath ((</>))
import FujiAF.Transport
import FujiAF.Read
  ( decodeReadReply, journalCapFor, journalCeilingBytes
  , readReplyBytes, readRequestBytes, readRequestFrame
  , replyError1Offset, replyError2Offset, replyFunctionOffset, replyLengthOffset
  , replyOffsetOffset, replyReservedOffset, sessionFixedTransfers, word32At, word32le )
import FujiAF.Probe
  ( configReplyBytes, configWriteBytes, configWriteFrame, debugDataOffset, debugFunction
  , debugOpcode, debugRequestBytes, decodeConfigWriteReply, gateByteOf, gateDisabled
  , gateEnabled, gateIndex, gateReadbackEnabled, gateReadbackRestored, probeConfigSteps
  , probeHalfBytes, probeName, probeWindowBytes, readbackWindow, transfersPerRequest )
import FujiAF.SHA256 (sha256)
import qualified FujiAF.Broker as P

-- The plan's window table: the descriptor-table window and the cursor cell.
routeDescriptorTableAddress, routeCursorAddress, routeCursorBytes :: Int
routeDescriptorTableAddress = 0x0096bd20
routeCursorAddress = 0x00a0f86c
routeCursorBytes = 4

-- p+0x60 covers the +0x64 check and +0x7c wait words; p+0x140 the +0x148
-- release and +0x164 create words.
routeMethodFirstOffset, routeMethodFirstBytes :: Int
routeMethodFirstOffset = 0x60
routeMethodFirstBytes = 32
routeMethodSecondOffset, routeMethodSecondBytes :: Int
routeMethodSecondOffset = 0x140
routeMethodSecondBytes = 40

routeFixedWindows :: [RouteWindow]
routeFixedWindows =
  [ RouteWindow 0 routeDescriptorTableAddress probeWindowBytes "class descriptor table and adjacent objects"
  , RouteWindow 1 routeCursorAddress routeCursorBytes "allocation-search cursor cell" ]

-- Phase 2 takes the window list from its plan file.
data RouteWindow = RouteWindow
  { routeWindowIndex :: Int
  , routeWindowAddress :: Int
  , routeWindowBytes :: Int
  , routeWindowPurpose :: String
  } deriving (Eq, Show, Read)

-- A plan at the window cap with maximum-length purposes stays under this.
routePlanDocumentCap :: Int
routePlanDocumentCap = 1024*1024

validateRoutePlan :: [RouteWindow] -> Either String ()
validateRoutePlan windows = do
  let count = length windows
      boundedText low high s = length s >= low && length s <= high && all (>= ' ') s
  unless (count >= 1) (Left "route plan must list at least one window")
  unless (journalCapFor count <= journalCeilingBytes) (Left "planned window count exceeds the journal ceiling")
  unless (map routeWindowIndex windows == [0..count-1]) (Left "window index must equal its position in the plan")
  unless (all (\window -> routeWindowBytes window >= 1 && routeWindowBytes window <= probeWindowBytes) windows)
    (Left "window length must be 1..256 bytes")
  unless (all (\window -> let address = routeWindowAddress window
                          in address >= 0 && address + routeWindowBytes window <= 0x100000000) windows)
    (Left "window must lie inside the 32-bit address space")
  -- The capture path creates each window file exclusively.
  unless (length (map routeWindowAddress windows) == length (dedupe (map routeWindowAddress windows)))
    (Left "window address must be distinct")
  unless (all (boundedText 1 256 . routeWindowPurpose) windows) (Left "bounded window purpose required")
  where
    dedupe = foldr (\x seen -> if x `elem` seen then seen else x:seen) []

-- One request per 128-byte step: a 256-byte window takes two, a 4, 32 or
-- 40-byte window one.
routeChunks :: Int -> [Int]
routeChunks bytes = takeWhile (>0) [min probeHalfBytes left | left <- [bytes, bytes-probeHalfBytes ..]]

routeWindowReads :: RouteWindow -> [(Int,Int)]
routeWindowReads window = go (routeWindowAddress window) (routeChunks (routeWindowBytes window))
  where
    go _ [] = []
    go address (size:sizes) = (address,size) : go (address+size) sizes

routeReads :: [RouteWindow] -> [(Int,Int)]
routeReads = concatMap routeWindowReads

data RouteRequest
  = RouteGateWrite Int
  | RouteGateRead String
  | RouteRead Int Int
  | RouteClose
  deriving (Eq, Show, Read)

-- The session's request sequence, so the tests and the offline rehearsal build
-- their scripts from the operation's own shape.
routeRequests :: [RouteWindow] -> [RouteRequest]
routeRequests windows =
  [ RouteGateWrite gateEnabled, RouteGateRead gateReadbackEnabled ]
  ++ concatMap (map (uncurry RouteRead) . routeWindowReads) windows
  ++ [ RouteGateWrite gateDisabled, RouteGateRead gateReadbackRestored, RouteClose ]

routeReadRequestCount :: [RouteWindow] -> Int
routeReadRequestCount = length . routeReads

-- Four transfers per request, four per config step and two per C0/C4 pair:
-- three requests carry 32 transfers and 1 + 2k carry 24 + 8k.
sessionPairTransfers :: Int
sessionPairTransfers = 4

routeTransferCount :: Int -> Int
routeTransferCount requests = transfersPerRequest*(probeConfigSteps+requests) + sessionPairTransfers

routeAggregateDeadlineMs :: Int -> Int
routeAggregateDeadlineMs requests = transferTimeoutMs
  *(sessionFixedTransfers + transfersPerRequest*probeConfigSteps + transfersPerRequest*requests)

-- Function 2 reads the address at +0x1c and the length at +0x20 and copies the
-- source to reply+0x24, so the reply is 0x24 + length for length up to 248;
-- the frame and decoder are the strap reads' shape.
routeRequestFrame :: Int -> Int -> [Word8]
routeRequestFrame address size = concat
  [ word32le debugOpcode, replicate 12 0
  , word32le debugFunction, word32le 0, word32le 0
  , word32le address, word32le size ]

routeReplyBytes :: Int -> Int
routeReplyBytes size = debugDataOffset + size

decodeRouteReply :: Int -> Int -> [Word8] -> Either String [Word8]
decodeRouteReply address size bytes
  | length bytes /= routeReplyBytes size = Left ("reply length "++show (length bytes)++" differs from the requested "++show (routeReplyBytes size))
  | word32At 0 bytes /= debugOpcode = Left "reply opcode differs from the request"
  | word32At replyReservedOffset bytes /= 0 = Left "reply reserved word is nonzero"
  | word32At replyFunctionOffset bytes /= debugFunction = Left "reply function differs from the request"
  | otherwise = case status of
      (0,0) | echoed == (address,size) -> Right (take size (drop debugDataOffset bytes))
            | otherwise -> Left ("echoed address/length "++show echoed++" differs from the request "++show (address,size))
      (9,3) -> Left "rejected arguments: error1 9, error2 3"
      (1,0xffff) -> Left "closed gate: error1 1, error2 0xffff"
      (first,second) -> Left ("status the handler cannot produce: error1 "++show first++", error2 "++show second)
  where
    echoed = (word32At replyOffsetOffset bytes, word32At replyLengthOffset bytes)
    status = (word32At replyError1Offset bytes, word32At replyError2Offset bytes)

-- The read requests executed: the plan's 3 in phase 1 and 1 + 2k in phase 2.
-- The journal cap is the read-config allowance, journalCapFor, the operation's
-- policy: a fixed session's notes run to about 14 KiB whatever the window list,
-- and a derived list may be the cursor cell alone.
runProbeRoutes :: Clock -> UsbApi -> FilePath -> [RouteWindow] -> IO Int
runProbeRoutes clock api dir windows = runProbeRoutesCore clock api dir windows `catch` \(e::SomeException) -> do
  _ <- try (recordWithin (journalCapFor (length windows)) dir "session-uncertain" (take 4096 (displayException e))) :: IO (Either SomeException ())
  throwIO e

runProbeRoutesCore :: Clock -> UsbApi -> FilePath -> [RouteWindow] -> IO Int
runProbeRoutesCore clock api dir windows = do
  let captured = dir </> "windows"
      requests = routeReads windows
  s <- newSession clock api dir (journalCapFor (length windows)) (routeAggregateDeadlineMs (length requests))
  held <- newHeld
  openedSession <- newIORef Nothing
  completed <- newIORef (0::Int)
  note s "prepared" ("probe-routes;windows="++show (length windows)++";read-requests="++show (length requests)++";gate-index="++show gateIndex)
  ensurePrivateDirectory captured
  mask $ \restore -> do
    result <- try $ restore $ do
      device <- acquireDevice s held
      let d = descriptors device
          input = inputEndpoint d
          output = outputEndpoint d
      _ <- tx s (P.controlTransfer (P.control 0xc0 8 0)) 12
      reply <- tx s (P.BulkIn input 8) 8
      -- C0 bytes 2..7 are opaque; each D1 length is its request's own reply length.
      let sid = fromIntegral (reply!!0) + 256*fromIntegral (reply!!1)
      writeIORef openedSession (Just (device,sid))
      let refuse label response reason = do
            note s "route-refused" (label++";reply="++P.hexBytes response++";"++reason)
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
          routeChunk label address size = do
            _ <- tx s (P.controlTransfer (P.control 0xd0 debugRequestBytes sid)) 12
            _ <- tx s (P.BulkOut output (routeRequestFrame address size)) debugRequestBytes
            _ <- tx s (P.controlTransfer (P.control 0xd1 (routeReplyBytes size) sid)) 12
            bytes <- tx s (P.BulkIn input (routeReplyBytes size)) (routeReplyBytes size)
            payload <- either (refuse label bytes) pure (decodeRouteReply address size bytes)
            modifyIORef' completed (+1)
            pure payload
      writeGate gateEnabled
      enabled <- readGateWindow gateReadbackEnabled
      unless (gateByteOf enabled == fromIntegral gateEnabled) (P.reject "gate enable is not visible in the config readback")
      forM_ windows $ \window -> do
        let label = probeName (routeWindowAddress window)
        parts <- traverse (\(address,size) -> routeChunk label address size) (routeWindowReads window)
        let bytes = concat parts
        unless (length bytes == routeWindowBytes window) (P.reject "window assembled a length other than its planned size")
        writePrivateBytes (captured </> (label++".bin")) (B.pack bytes)
        note s "window-complete" ("index="++show (routeWindowIndex window)
          ++";address=0x"++label
          ++";length="++show (length bytes)
          ++";purpose="++routeWindowPurpose window
          ++";sha256="++sha256 (B.pack bytes))
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
    sent <- readIORef completed
    case (result,failures) of
      (_, _:_) -> P.reject ("cleanup uncertain: "++show failures)
      (Left (e::SomeException),[]) -> throwIO e
      (Right (),[]) -> do
        note s "route-read-complete-stock-unverified"
          (show sent++" read requests;"++show (routeTransferCount sent)++" transfers")
        pure sent
