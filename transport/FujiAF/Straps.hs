-- Strap read over D0 opcode 0x200001 function 2, in the session shape
-- FujiAF.BootRom uses: the cache-only adjustment index 0xf7 gate enable and
-- readback, the fixed read list, the restore write and readback, C4.
--
-- The list is section 8 of private-data/analysis/x-m1-1.50/bootrom/
-- strap-read-safety.md: the boot-time mirror of the 16 port registers at
-- 0x400a0000 first, the two ROM alias windows 0x80010000 and 0x80100000 that
-- separate address-bit aliasing from a bounded repeated image, then 4 bytes
-- each at 0xfff89014 and 0xfff98000, the two registers the boot ROM's mode
-- detect reads, last so that the RAM and ROM bytes are durable before any
-- MMIO byte access. No target has a known content, so no read is an oracle:
-- the in-session checks are the reply contracts, the two gate readbacks and
-- the journaled hashes.
module FujiAF.Straps where

import Control.Exception
import Control.Monad (forM_, unless)
import qualified Data.ByteString as B
import Data.IORef
import Data.Word (Word8)
import System.FilePath ((</>))
import FujiAF.Transport
import FujiAF.Read
  ( decodeReadReply, readReplyBytes, readRequestBytes, readRequestFrame
  , replyError1Offset, replyError2Offset, replyFunctionOffset, replyLengthOffset
  , replyOffsetOffset, replyReservedOffset, sessionFixedTransfers, word32At, word32le )
import FujiAF.Probe
  ( configReplyBytes, configWriteBytes, configWriteFrame, debugDataOffset, debugFunction
  , debugOpcode, debugRequestBytes, decodeConfigWriteReply, gateByteOf, gateDisabled
  , gateEnabled, gateIndex, gateReadbackEnabled, gateReadbackRestored, probeConfigSteps
  , probeName, readbackWindow, transfersPerRequest )
import FujiAF.SHA256 (sha256)
import qualified FujiAF.Broker as P

-- The mirror is word k of 0x400a0000 for port k, so word 0 is 0xfff89014, and
-- the boot record and image 5 fill it with 16 strided word reads
-- (strap-read-safety.md section 7).
data StrapTarget
  = PortMirror
  | RomAliasFirst
  | RomAliasSecond
  | StrapRegisterHigh
  | StrapRegisterLow
  deriving (Eq, Show, Read, Enum, Bounded)

strapTargets :: [StrapTarget]
strapTargets = [minBound .. maxBound]

strapTargetName :: StrapTarget -> String
strapTargetName target = case target of
  PortMirror -> "port-mirror"
  RomAliasFirst -> "rom-alias-80010000"
  RomAliasSecond -> "rom-alias-80100000"
  StrapRegisterHigh -> "strap-fff89014"
  StrapRegisterLow -> "strap-fff98000"

strapTargetAddress :: StrapTarget -> Int
strapTargetAddress target = case target of
  PortMirror -> 0x400a0000
  RomAliasFirst -> 0x80010000
  RomAliasSecond -> 0x80100000
  StrapRegisterHigh -> 0xfff89014
  StrapRegisterLow -> 0xfff98000

strapTargetBytes :: StrapTarget -> Int
strapTargetBytes target = case target of
  PortMirror -> 64
  RomAliasFirst -> 256
  RomAliasSecond -> 256
  StrapRegisterHigh -> 4
  StrapRegisterLow -> 4

-- The step is the longest request the 1..256 byte handler bound and the
-- existing ROM reads use.
strapChunkBytes :: Int
strapChunkBytes = 128

strapChunks :: Int -> [Int]
strapChunks bytes = takeWhile (>0) [min strapChunkBytes left | left <- [bytes, bytes-strapChunkBytes ..]]

strapTargetReads :: StrapTarget -> [(Int,Int)]
strapTargetReads target = go (strapTargetAddress target) (strapChunks (strapTargetBytes target))
  where
    go _ [] = []
    go address (size:sizes) = (address,size) : go (address+size) sizes

strapReadRequests :: [(Int,Int)]
strapReadRequests = concatMap strapTargetReads strapTargets

strapRequestCount :: Int
strapRequestCount = length strapReadRequests

data StrapRequest
  = StrapGateWrite Int
  | StrapGateRead String
  | StrapRead Int Int
  | StrapClose
  deriving (Eq, Show, Read)

strapRequests :: [StrapRequest]
strapRequests =
  [ StrapGateWrite gateEnabled, StrapGateRead gateReadbackEnabled ]
  ++ concatMap (map (uncurry StrapRead) . strapTargetReads) strapTargets
  ++ [ StrapGateWrite gateDisabled, StrapGateRead gateReadbackRestored, StrapClose ]

strapJournalCapBytes :: Int
strapJournalCapBytes = 1048576

strapAggregateDeadlineMs :: Int
strapAggregateDeadlineMs = transferTimeoutMs
  *(sessionFixedTransfers + transfersPerRequest*probeConfigSteps + transfersPerRequest*strapRequestCount)

-- Function 2 of opcode 0x200001 reads the address at +0x1c and the length at
-- +0x20 and copies the source to reply+0x24, so the reply is 0x24 + length for
-- length up to 248: the handler advertises 284 at 0xd80608 and the D1 path
-- transfers the smaller of the advertised and requested sizes, so a request
-- above 248 bytes comes back short. Every request here is 4, 64 or 128 bytes.
strapRequestFrame :: Int -> Int -> [Word8]
strapRequestFrame address size = concat
  [ word32le debugOpcode, replicate 12 0
  , word32le debugFunction, word32le 0, word32le 0
  , word32le address, word32le size ]

strapReplyBytes :: Int -> Int
strapReplyBytes size = debugDataOffset + size

decodeStrapReply :: Int -> Int -> [Word8] -> Either String [Word8]
decodeStrapReply address size bytes
  | length bytes /= strapReplyBytes size = Left ("reply length "++show (length bytes)++" differs from the requested "++show (strapReplyBytes size))
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

runProbeStraps :: Clock -> UsbApi -> FilePath -> IO Int
runProbeStraps clock api dir = runProbeStrapsCore clock api dir `catch` \(e::SomeException) -> do
  _ <- try (recordWithin strapJournalCapBytes dir "session-uncertain" (take 4096 (displayException e))) :: IO (Either SomeException ())
  throwIO e

runProbeStrapsCore :: Clock -> UsbApi -> FilePath -> IO Int
runProbeStrapsCore clock api dir = do
  let captured = dir </> "windows"
  s <- newSession clock api dir strapJournalCapBytes strapAggregateDeadlineMs
  held <- newHeld
  openedSession <- newIORef Nothing
  completed <- newIORef (0::Int)
  note s "prepared" ("probe-straps;targets="++show (length strapTargets)++";read-requests="++show strapRequestCount++";gate-index="++show gateIndex)
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
            note s "strap-refused" (label++";reply="++P.hexBytes response++";"++reason)
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
          readChunk label address size = do
            _ <- tx s (P.controlTransfer (P.control 0xd0 debugRequestBytes sid)) 12
            _ <- tx s (P.BulkOut output (strapRequestFrame address size)) debugRequestBytes
            _ <- tx s (P.controlTransfer (P.control 0xd1 (strapReplyBytes size) sid)) 12
            bytes <- tx s (P.BulkIn input (strapReplyBytes size)) (strapReplyBytes size)
            either (refuse label bytes) pure (decodeStrapReply address size bytes)
      writeGate gateEnabled
      enabled <- readGateWindow gateReadbackEnabled
      unless (gateByteOf enabled == fromIntegral gateEnabled) (P.reject "gate enable is not visible in the config readback")
      forM_ strapTargets $ \target -> do
        let label = probeName (strapTargetAddress target)
        parts <- traverse (\(chunk,size) -> readChunk label chunk size) (strapTargetReads target)
        let bytes = concat parts
        unless (length bytes == strapTargetBytes target) (P.reject "target assembled a length other than its fixed size")
        writePrivateBytes (captured </> (label++".bin")) (B.pack bytes)
        note s "target-complete" (strapTargetName target++";address=0x"++label++";length="++show (length bytes)++";sha256="++sha256 (B.pack bytes))
        modifyIORef' completed (+strapTargetBytes target)
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
    capturedBytes <- readIORef completed
    case (result,failures) of
      (_, _:_) -> P.reject ("cleanup uncertain: "++show failures)
      (Left (e::SomeException),[]) -> throwIO e
      (Right (),[]) -> note s "strap-read-complete-stock-unverified" (show capturedBytes++" bytes") >> pure capturedBytes
