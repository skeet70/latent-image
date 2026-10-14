-- Fixed-segment RAM dump over D0 opcode 0x200001 function 2: C0 open, the
-- cache-only adjustment index 0xf7 enable and its config readback, one named
-- segment's fixed ranges as bounded read requests in strictly increasing
-- address order, the restore write, its readback and C4.
--
-- Every routine of that opcode returns without acting while adjustment index
-- 0xf7 is zero, so the session opens the gate with a config opcode 0x1a0002
-- function 3 cache write, reads the segment, restores the byte and closes, the
-- sequence read-log and probe-bootrom run. The gate frames and decoders come
-- from FujiAF.Probe and the read request and reply offsets from FujiAF.Read.
--
-- The 1.50 handler for function 2 accepts lengths 1..256 and rejects anything
-- else with (9,3) (image 5, bound at 0xd80454-0xd8045c), and advertises a
-- 284-byte reply with data at +0x24 on every exit (0xd80608), the bulk IN
-- delivering min(284, D1 wLength). The largest fully delivered request is
-- therefore 248 bytes with D1 wLength 284, which is dumpRequestLength below.
-- That maximum is static inference from those addresses; the first live
-- dump-ram segment is the first observation of a 248-byte request on this
-- body.
module FujiAF.DumpRam where

import Control.Exception
import Control.Monad (forM_, unless, when)
import qualified Data.ByteString as B
import Data.IORef
import Data.Word (Word8)
import Foreign.Ptr (castPtr)
import System.FilePath ((</>))
import System.Posix.Files (getFdStatus, isRegularFile)
import System.Posix.IO
import System.Posix.Types (Fd)
import System.Posix.Unistd (fileSynchronise)
import FujiAF.Transport
import FujiAF.Read
  ( decodeReadReply, readReplyBytes, readRequestBytes, readRequestFrame
  , replyError1Offset, replyError2Offset, replyFunctionOffset, replyReservedOffset
  , sessionFixedTransfers, word32At, word32le )
import FujiAF.Probe
  ( configReplyBytes, configWriteBytes, configWriteFrame, decodeConfigWriteReply
  , debugDataOffset, debugFunction, debugOpcode, debugRequestBytes, gateByteOf
  , gateDisabled, gateEnabled, gateIndex, gateReadbackEnabled, gateReadbackRestored
  , probeConfigSteps, probeName, readbackWindow, transfersPerRequest )
import FujiAF.SHA256 (sha256)
import qualified FujiAF.Broker as P

-- The one request-size constant. Frames, reply sizes, chunk boundaries and
-- every bound below derive from it, so raising it is a change here plus the
-- test expectations.
dumpRequestLength :: Int
dumpRequestLength = 248

-- The named segments. The CLI argument maps to exactly one of these.
data DumpSegment
  = Loaders
  | Id4Image0 | Id4Image1 | Id4Image2
  | Id4Bss0 | Id4Bss1 | Id4Bss2 | Id4Bss3
  | Id5Image0 | Id5Image1 | Id5Image2 | Id5Image3 | Id5Image4
  | Plain0 | Plain1 | Plain2 | Plain3
  | Id5Bss0 | Id5Bss1 | Id5Bss2
  | HighKnown
  deriving (Eq, Show, Read, Enum, Bounded)

segmentName :: DumpSegment -> String
segmentName Loaders = "loaders"
segmentName Id4Image0 = "id4-image-0"
segmentName Id4Image1 = "id4-image-1"
segmentName Id4Image2 = "id4-image-2"
segmentName Id4Bss0 = "id4-bss-0"
segmentName Id4Bss1 = "id4-bss-1"
segmentName Id4Bss2 = "id4-bss-2"
segmentName Id4Bss3 = "id4-bss-3"
segmentName Id5Image0 = "id5-image-0"
segmentName Id5Image1 = "id5-image-1"
segmentName Id5Image2 = "id5-image-2"
segmentName Id5Image3 = "id5-image-3"
segmentName Id5Image4 = "id5-image-4"
segmentName Plain0 = "plain-0"
segmentName Plain1 = "plain-1"
segmentName Plain2 = "plain-2"
segmentName Plain3 = "plain-3"
segmentName Id5Bss0 = "id5-bss-0"
segmentName Id5Bss1 = "id5-bss-1"
segmentName Id5Bss2 = "id5-bss-2"
segmentName HighKnown = "high-known"

-- The CLI boundary: one segment name and at most one OUTDIR.
dumpSegmentArguments :: [String] -> Maybe (DumpSegment, Maybe FilePath)
dumpSegmentArguments (name:rest) = do
  segment <- readSegment name
  directory <- case rest of
    [] -> Just Nothing
    [one] -> Just (Just one)
    _ -> Nothing
  pure (segment, directory)
dumpSegmentArguments [] = Nothing

readSegment :: String -> Maybe DumpSegment
readSegment name = lookup name [(segmentName segment, segment) | segment <- [minBound..maxBound]]

data DumpRange = DumpRange { rangeSegment :: DumpSegment, rangeStart :: Int, rangeEnd :: Int }
  deriving (Eq, Show, Read)

-- The fixed table, in strictly increasing address order. Every bound is
-- 256-aligned and every range larger than one 1 MiB part is split into 1 MiB
-- parts with the remainder last. Id4-bss is image 4's bss from the live
-- segment table entry {4, 0x0061c400, 0x282828, 0x0089ec00, 0x352e00};
-- id5-bss is image 5's {0x01460000, 0x2bbe40} with the end rounded up to 256;
-- high-known holds the boot snapshots and syslog store, the retained settings
-- header and its snapshot, and the adjustment cache; loaders holds the two
-- loader images at their load bases.
segmentTable :: [DumpRange]
segmentTable =
  [ DumpRange Loaders 0x00040000 0x0004db00
  , DumpRange Loaders 0x000d0000 0x000ddb00
  , DumpRange Id4Image0 0x0061c000 0x0071c000
  , DumpRange Id4Image1 0x0071c000 0x0081c000
  , DumpRange Id4Image2 0x0081c000 0x0089ec00
  , DumpRange Id4Bss0 0x0089ec00 0x0099ec00
  , DumpRange Id4Bss1 0x0099ec00 0x00a9ec00
  , DumpRange Id4Bss2 0x00a9ec00 0x00b9ec00
  , DumpRange Id4Bss3 0x00b9ec00 0x00bf1a00
  , DumpRange Id5Image0 0x00bf1a00 0x00cf1a00
  , DumpRange Id5Image1 0x00cf1a00 0x00df1a00
  , DumpRange Id5Image2 0x00df1a00 0x00ef1a00
  , DumpRange Id5Image3 0x00ef1a00 0x00ff1a00
  , DumpRange Id5Image4 0x00ff1a00 0x010e3900
  , DumpRange Plain0 0x01100000 0x01200000
  , DumpRange Plain1 0x01200000 0x01300000
  , DumpRange Plain2 0x01300000 0x01400000
  , DumpRange Plain3 0x01400000 0x01460000
  , DumpRange Id5Bss0 0x01460000 0x01560000
  , DumpRange Id5Bss1 0x01560000 0x01660000
  , DumpRange Id5Bss2 0x01660000 0x0171bf00
  , DumpRange HighKnown 0x400a0000 0x400c0800
  , DumpRange HighKnown 0x409c2800 0x409c3000
  , DumpRange HighKnown 0x41acd800 0x41acdc00 ]

segmentRanges :: DumpSegment -> [DumpRange]
segmentRanges segment = filter ((== segment) . rangeSegment) segmentTable

segmentBytes :: DumpSegment -> Int
segmentBytes segment = sum [rangeEnd range - rangeStart range | range <- segmentRanges segment]

-- One request reads at most dumpRequestLength bytes. Each range is walked from
-- its start at dumpRequestLength stride, so its last request carries the
-- remainder (1..dumpRequestLength) bytes and a multi-range segment has one
-- remainder request per range. The reply echoes each request's exact address
-- and length.
rangeChunks :: Int -> Int -> [(Int,Int)]
rangeChunks start end =
  [ (address, min dumpRequestLength (end - address))
  | address <- takeWhile (< end) [start, start + dumpRequestLength ..] ]

segmentChunks :: DumpSegment -> [(Int,Int)]
segmentChunks segment = concatMap (\range -> rangeChunks (rangeStart range) (rangeEnd range)) (segmentRanges segment)

dumpRequestCount :: DumpSegment -> Int
dumpRequestCount segment = length (segmentChunks segment)

-- The session's fixed request sequence, so the tests and the offline rehearsal
-- build their scripts from the operation's own shape. DumpChunk carries the
-- address and the exact length (1..dumpRequestLength) of one request.
data DumpRequest
  = DumpGateWrite Int
  | DumpGateRead String
  | DumpChunk Int Int
  | DumpClose
  deriving (Eq, Show, Read)

dumpRequests :: DumpSegment -> [DumpRequest]
dumpRequests segment =
  [ DumpGateWrite gateEnabled, DumpGateRead gateReadbackEnabled ]
  ++ map (uncurry DumpChunk) (segmentChunks segment)
  ++ [ DumpGateWrite gateDisabled, DumpGateRead gateReadbackRestored, DumpClose ]

-- Debug opcode 0x200001 function 2 request: function at +0x10, address at
-- +0x1c, length at +0x20. The reply is 0x24 + length bytes, its status words
-- at +0x04/+0x08, its echoed address and length at +0x1c/+0x20 and its data
-- at +0x24.
dumpRequestFrame :: Int -> Int -> [Word8]
dumpRequestFrame address size = concat
  [ word32le debugOpcode, replicate 12 0
  , word32le debugFunction, word32le 0, word32le 0
  , word32le address, word32le size ]

dumpReplyBytes :: Int -> Int
dumpReplyBytes size = debugDataOffset + size

data DumpReply = DumpReply { dumpStatus :: (Int,Int), dumpData :: [Word8] }
  deriving (Eq, Show)

decodeDumpReply :: Int -> Int -> [Word8] -> Either String DumpReply
decodeDumpReply address size bytes
  | length bytes /= dumpReplyBytes size = Left ("reply length "++show (length bytes)++" differs from the requested "++show (dumpReplyBytes size))
  | word32At 0 bytes /= debugOpcode = Left "reply opcode differs from the request"
  | word32At replyReservedOffset bytes /= 0 = Left "reply reserved word is nonzero"
  | word32At replyFunctionOffset bytes /= debugFunction = Left "reply function differs from the request"
  | otherwise = case status of
      (0,0) | echoed == (address,size) -> Right (DumpReply status (take size (drop debugDataOffset bytes)))
            | otherwise -> Left ("echoed address/length "++show echoed++" differs from the request "++show (address,size))
      (9,3) -> Left "rejected arguments: error1 9, error2 3"
      (1,0xffff) -> Left "closed gate: error1 1, error2 0xffff"
      (first,second) -> Left ("status the handler cannot produce: error1 "++show first++", error2 "++show second)
  where
    echoed = (word32At 0x1c bytes, word32At 0x20 bytes)
    status = (word32At replyError1Offset bytes, word32At replyError2Offset bytes)

-- journal cap = dumpJournalFixedBytes + dumpJournalPerRequestBytes * requests,
-- applied to each journal file. One request costs three outgoing frame lines
-- (about 700 bytes) and one request summary line (about 250 bytes); the fixed
-- part carries the gate steps, the close and a failure record.
dumpJournalFixedBytes, dumpJournalPerRequestBytes :: Int
dumpJournalFixedBytes = 65536
dumpJournalPerRequestBytes = 1024

dumpJournalCapFor :: Int -> Int
dumpJournalCapFor requests = dumpJournalFixedBytes + dumpJournalPerRequestBytes*requests

-- 5000 ms per transfer as every command: four transfers per request, four per
-- gate step and eight fixed (C0, C4 and the native stages).
dumpAggregateDeadlineFor :: Int -> Int
dumpAggregateDeadlineFor requests = transferTimeoutMs
  *(sessionFixedTransfers + transfersPerRequest*probeConfigSteps + transfersPerRequest*requests)

-- The request summary lines and the capture file are written as they complete
-- and made durable every this many requests and at segment end.
dumpFsyncEvery :: Int
dumpFsyncEvery = 256

runDumpRam :: DumpSegment -> Clock -> UsbApi -> FilePath -> IO Int
runDumpRam segment clock api dir = runDumpRamCore segment clock api dir `catch` \(e::SomeException) -> do
  _ <- try (recordWithin (dumpJournalCapFor (dumpRequestCount segment)) dir "session-uncertain" (take 4096 (displayException e))) :: IO (Either SomeException ())
  throwIO e

runDumpRamCore :: DumpSegment -> Clock -> UsbApi -> FilePath -> IO Int
runDumpRamCore segment clock api dir = do
  let chunks = segmentChunks segment
      requests = length chunks
      cap = dumpJournalCapFor requests
      capturePath = dir </> (segmentName segment ++ ".bin")
  s <- newSession clock api dir cap (dumpAggregateDeadlineFor requests)
  held <- newHeld
  openedSession <- newIORef Nothing
  completed <- newIORef (0::Int)
  pending <- newIORef ([] :: [(String,String)])
  note s "prepared" ("dump-ram;segment="++segmentName segment++";requests="++show requests++";gate-index="++show gateIndex)
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
            note s "dump-ram-refused" (label++";reply="++P.hexBytes response++";"++reason)
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
            writePrivateBytes (dir </> (label++".bin")) (B.pack payload)
            note s "readback-complete" (label++";sha256="++sha256 (B.pack payload))
            pure payload
          -- The requests journal no per-frame lines: their outgoing frames are
          -- durable in the plan write and each request leaves one summary
          -- line. The deadline and transfer budgets are enforced without
          -- journaling.
          chunkTransfer packet count = do
            ms <- remaining (sessionClock s) (sessionDeadline s)
            before <- nowNS (sessionClock s)
            (n,bytes) <- usbTransfer (sessionApi s) packet (min ms transferTimeoutMs)
            after <- nowNS (sessionClock s)
            when (after >= sessionDeadline s || after >= before + fromIntegral transferTimeoutMs*1000000)
              (P.reject ("chunk deadline exceeded;reply="++P.hexBytes bytes))
            unless (n == count) (P.reject ("short/partial/oversized transfer "++P.hexBytes bytes++"; no retry"))
            case packet of
              P.BulkIn _ _ -> unless (length bytes == count) (P.reject ("read count/bytes mismatch "++P.hexBytes bytes))
              _ -> unless (null bytes) (P.reject ("write returned bytes "++P.hexBytes bytes))
            pure bytes
          flushJournal = do
            entries <- readIORef pending
            unless (null entries) $ do
              writeIORef pending []
              recordBatchWithin cap dir "request-complete" (reverse entries)
          syncPoint fd = flushJournal >> fileSynchronise fd
          appendCapture fd payload = B.useAsCStringLen (B.pack payload) $ \(buffer,size) -> do
            count <- fdWriteBuf fd (castPtr buffer) (fromIntegral size)
            unless (count == fromIntegral size) (P.reject "short capture write")
          chunk fd index (address,size) =
            (do
              _ <- chunkTransfer (P.controlTransfer (P.control 0xd0 debugRequestBytes sid)) 12
              _ <- chunkTransfer (P.BulkOut output (dumpRequestFrame address size)) debugRequestBytes
              _ <- chunkTransfer (P.controlTransfer (P.control 0xd1 (dumpReplyBytes size) sid)) 12
              bytes <- chunkTransfer (P.BulkIn input (dumpReplyBytes size)) (dumpReplyBytes size)
              DumpReply status payload <- either (\reason -> P.reject (reason++";reply="++P.hexBytes bytes)) pure (decodeDumpReply address size bytes)
              appendCapture fd payload
              time <- nowNS (sessionClock s)
              let line = (show time, "index="++show index++";address=0x"++probeName address++";length="++show size
                          ++";status="++show status++";sha256="++sha256 (B.pack payload)++";time="++show time)
              modifyIORef' pending (line:)
              done <- modifyIORef' completed (+1) >> readIORef completed
              when (done `mod` dumpFsyncEvery == 0) (syncPoint fd))
            `catch` \(e::SomeException) -> do
              syncPoint fd `catch` \(_::SomeException) -> pure ()
              note s "dump-ram-refused" ("chunk="++show index++";address=0x"++probeName address++";length="++show size++";"++displayException e)
              throwIO e
      writeGate gateEnabled
      enabled <- readGateWindow gateReadbackEnabled
      unless (gateByteOf enabled == fromIntegral gateEnabled) (P.reject "gate enable is not visible in the config readback")
      -- The whole request sequence is durable before the first request is
      -- dispatched, in send order.
      planTick <- nowNS (sessionClock s)
      let packets address size =
            [ P.controlTransfer (P.control 0xd0 debugRequestBytes sid)
            , P.BulkOut output (dumpRequestFrame address size)
            , P.controlTransfer (P.control 0xd1 (dumpReplyBytes size) sid) ]
          body packet = case packet of
            P.ControlWrite _ _ _ _ frame -> show packet++";bytes="++P.hexBytes frame
            P.BulkOut _ frame -> show packet++";bytes="++P.hexBytes frame
            P.BulkIn _ _ -> show packet
      recordBatchWithin cap dir "outgoing-before-dispatch"
        [(show planTick, body packet) | (address,size) <- chunks, packet <- packets address size]
      bracket (openCapture capturePath) closeFd $ \fd -> do
        forM_ (zip [0::Int ..] chunks) $ \(index,entry) -> chunk fd index entry
        syncPoint fd
      captured <- B.readFile capturePath
      note s "segment-complete" ("segment="++segmentName segment++";bytes="++show (B.length captured)++";sha256="++sha256 captured)
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
    done <- readIORef completed
    case (result,failures) of
      (_, _:_) -> P.reject ("cleanup uncertain: "++show failures)
      (Left (e::SomeException),[]) -> throwIO e
      (Right (),[]) -> note s "dump-complete-stock-unverified" (show done++" requests") >> pure done

-- The capture file is created exclusively and appended per request.
openCapture :: FilePath -> IO Fd
openCapture path = do
  fd <- openFd path WriteOnly defaultFileFlags {creat=Just 0o600,exclusive=True,nofollow=True,cloexec=True}
  status <- getFdStatus fd
  unless (isRegularFile status) (P.reject "capture must be regular")
  pure fd
