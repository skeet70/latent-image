-- Read one DRAM window through the stock ff80 bulk path (FF80-BULK-READ-01,
-- reports/h2-read-scope-proposal.md section 2.5, E-BULK-READ-PATH-150):
-- C0 with the constant reply tail, the tag-4 identity, config window
-- 0x000000 refused unless 0xf7 != 0, one D0 opcode 0x450006 tag-0x80 query,
-- a 0x200001 function 2 read of the published source pair at 0x01469878,
-- then per chunk transport command E1 and one bulk IN, then C4.
--
-- The query stores pointer 0x4b2ece00 and its capacity at 0x01469878 and
-- 0x0146987c. E1 stores an offset and a length; the bulk-IN source callback
-- 0x00d7efd4 then returns pointer + offset, a 32-bit sum, for
-- min(length, capacity) bytes. An address A is read with offset
-- (A - 0x4b2ece00) mod 2^32. No routine on the path reads 0xf7; the gate
-- check and the function 2 read pin the pointer the offsets are computed
-- against before any E1 is sent.
--
-- The bulk OUT frames are the identity request, one config function 2 read,
-- the query and one function 2 RAM read; the control frames are C0, D0, D1,
-- E1 and C4. No config write, RAM write or gate write exists here.
module FujiAF.ReadBulk
  ( BulkWindow, mkBulkWindow, bulkWindowAddress, bulkWindowBytes
  , bulkWindowRanges, bulkPhysicalEnd, bulkAddressAlignment, bulkLengthGranule
  , bulkChunkBytes, bulkChunks, bulkChunkCount
  , bulkSourcePointer, bulkSourceCapacities, bulkSourceStateAddress, bulkSourceStateBytes
  , bulkQueryOpcode, bulkQueryTag, bulkQueryRequestBytes, bulkQueryReplyBytes
  , bulkQueryFrame, decodeBulkQueryReply, decodeBulkSourceState
  , bulkOffsetFor, bulkCommand, bulkCommandFrame
  , readBulkAggregateDeadlineMs, readBulkJournalCapBytes
  , readBulkArguments, bulkCaptureName
  , ReadBulkReport (..), readBulkResultLine, runReadBulk
  ) where

import Control.Exception
import Control.Monad (forM_, unless, when)
import qualified Crypto.Hash.SHA256 as H
import qualified Data.ByteString as B
import Data.Bits ((.&.), shiftR)
import Data.Char (isHexDigit)
import Data.IORef
import Data.List (intercalate)
import Data.Word (Word8, Word64)
import Foreign.Ptr (castPtr)
import Numeric (readHex)
import System.FilePath ((</>))
import System.Posix.IO
import System.Posix.Unistd (fileSynchronise)
import FujiAF.Transport
import FujiAF.Read (decodeReadReply, readReplyBytes, readRequestBytes, readRequestFrame, sessionFixedTransfers, word32At, word32le)
import FujiAF.Probe (debugRequestBytes, gateIndex, probeName, transfersPerRequest)
import FujiAF.Routes (decodeRouteReply, routeReplyBytes, routeRequestFrame)
import FujiAF.ReadRam (gateWindow, readRamDramRanges)
import FujiAF.DumpRam (openCapture)
import FujiAF.StoredGate (openReplyTail)
import FujiAF.SHA256 (sha256)
import qualified FujiAF.Broker as P

-- A window inside one DRAM range whose physical end is at most bulkPhysicalEnd.
data BulkWindow = BulkWindow { bulkWindowAddress :: Int, bulkWindowBytes :: Int }
  deriving (Eq, Show)

-- The read grant's DRAM backing and its 0x40000000 alias (spec/07 3.4). The
-- USB DMA source register drops bits 31:30, so both reach one physical range.
bulkWindowRanges :: [(Int,Int)]
bulkWindowRanges = readRamDramRanges

-- The end of the stock bulk source's largest window, 0x4b2ece00 + 0x108000
-- at physical 0x0b2ece00 (E-BULK-READ-BOUND-150). Populated DRAM above it is
-- not established.
bulkPhysicalEnd :: Int
bulkPhysicalEnd = 0x0b3f6e00

-- The DMA source register drops bits 1:0 and 0x00d7c92c rounds the DMA
-- length up to 64 bytes, so a 64-byte multiple fetches nothing past the
-- window.
bulkAddressAlignment, bulkLengthGranule :: Int
bulkAddressAlignment = 4
bulkLengthGranule = 64

mkBulkWindow :: Int -> Int -> Either String BulkWindow
mkBulkWindow address bytes
  | bytes < bulkLengthGranule =
      Left ("read-bulk length must be at least "++show bulkLengthGranule++" bytes, not "++show bytes)
  | address `mod` bulkAddressAlignment /= 0 =
      Left ("read-bulk address must be a multiple of "++show bulkAddressAlignment)
  | bytes `mod` bulkLengthGranule /= 0 =
      Left ("read-bulk length must be a multiple of "++show bulkLengthGranule)
  | not (any (\(low,high) -> low <= address && address + bytes <= high) bulkWindowRanges) =
      Left ("read-bulk window [0x"++probeName address++",0x"++probeName (address+bytes)++") is outside "
            ++intercalate ", " ["[0x"++probeName low++",0x"++probeName high++")" | (low,high) <- bulkWindowRanges])
  | (address .&. 0x3fffffff) + bytes > bulkPhysicalEnd =
      Left ("read-bulk window ends at physical 0x"++probeName ((address .&. 0x3fffffff) + bytes)
            ++", above 0x"++probeName bulkPhysicalEnd++", the highest DRAM end the record establishes")
  | otherwise = Right (BulkWindow address bytes)

-- The query's base capacity; every chunk fits either published capacity.
bulkChunkBytes :: Int
bulkChunkBytes = 0x100000

-- (address, length) per E1, in increasing address order.
bulkChunks :: BulkWindow -> [(Int,Int)]
bulkChunks (BulkWindow address bytes) =
  [ (start, min bulkChunkBytes (address + bytes - start)) | start <- [address, address + bulkChunkBytes .. address + bytes - 1] ]

bulkChunkCount :: BulkWindow -> Int
bulkChunkCount window = (bulkWindowBytes window + bulkChunkBytes - 1) `div` bulkChunkBytes

-- What 0x00dbd09c stores for tag 0x80: the staging window and 0x100000,
-- plus 0x8000 when 0x006fbaa4 returns 1.
bulkSourcePointer :: Int
bulkSourcePointer = 0x4b2ece00

bulkSourceCapacities :: [Int]
bulkSourceCapacities = [0x100000, 0x108000]

-- The pointer and capacity words the query publishes.
bulkSourceStateAddress, bulkSourceStateBytes :: Int
bulkSourceStateAddress = 0x01469878
bulkSourceStateBytes = 8

bulkQueryOpcode, bulkQueryTag, bulkQueryRequestBytes, bulkQueryReplyBytes :: Int
bulkQueryOpcode = 0x450006
bulkQueryTag = 0x80
bulkQueryRequestBytes = 32
bulkQueryReplyBytes = 32

-- Opcode, twelve zero bytes, the tag at +0x10, twelve zero bytes.
bulkQueryFrame :: [Word8]
bulkQueryFrame = word32le bulkQueryOpcode ++ replicate 12 0 ++ word32le bulkQueryTag ++ replicate 12 0

-- The capacity the reply carries at +0x14.
decodeBulkQueryReply :: [Word8] -> Either String Int
decodeBulkQueryReply bytes
  | length bytes /= bulkQueryReplyBytes = Left ("query reply length "++show (length bytes)++" differs from "++show bulkQueryReplyBytes)
  | word32At 0 bytes /= bulkQueryOpcode = Left "query reply opcode differs from 0x00450006"
  | (word32At 4 bytes, word32At 8 bytes) /= (0,0) =
      Left ("query reply status error1 "++show (word32At 4 bytes)++", error2 "++show (word32At 8 bytes))
  | any (/= 0) [word32At at bytes | at <- [0x0c,0x10,0x18,0x1c]] = Left "query reply carries a nonzero word outside opcode and capacity"
  | capacity `notElem` bulkSourceCapacities = Left ("query capacity 0x"++probeName capacity++" is neither 0x100000 nor 0x108000")
  | otherwise = Right capacity
  where capacity = word32At 0x14 bytes

-- The function 2 bytes of 0x01469878: the pointer and the capacity the
-- reply named.
decodeBulkSourceState :: Int -> [Word8] -> Either String ()
decodeBulkSourceState capacity bytes
  | length bytes /= bulkSourceStateBytes = Left "source state read must be 8 bytes"
  | word32At 0 bytes /= bulkSourcePointer =
      Left ("published source pointer 0x"++probeName (word32At 0 bytes)++" differs from 0x"++probeName bulkSourcePointer)
  | word32At 4 bytes /= capacity =
      Left ("published capacity 0x"++probeName (word32At 4 bytes)++" differs from the reply's 0x"++probeName capacity)
  | otherwise = Right ()

bulkOffsetFor :: Int -> Int
bulkOffsetFor address = (address - bulkSourcePointer) .&. 0xffffffff

-- E1, a zero byte, big-endian offset at +2, big-endian length at +6 and the
-- big-endian session at +10.
bulkCommand :: Int -> Int -> Int -> [Word8]
bulkCommand offset size sid = 0xe1 : 0 : be32 offset ++ be32 size ++ [fromIntegral (sid `shiftR` 8), fromIntegral sid]
  where be32 v = [fromIntegral (v `shiftR` n) | n <- [24,16,8,0]]

bulkCommandFrame :: Int -> (Int,Int) -> P.Transfer
bulkCommandFrame sid (address,size) = P.controlTransfer (bulkCommand (bulkOffsetFor address) size sid)

-- C0/C4 and the native stages, identity, config window, query and source
-- read at four transfers each, two per chunk.
readBulkAggregateDeadlineMs :: BulkWindow -> Int
readBulkAggregateDeadlineMs window = transferTimeoutMs*(sessionFixedTransfers + transfersPerRequest*4 + 2*bulkChunkCount window)

-- The fixed session's lines plus one outgoing frame line and one chunk line
-- per chunk.
readBulkJournalCapBytes :: BulkWindow -> Int
readBulkJournalCapBytes window = 65536 + 1024*bulkChunkCount window

-- ADDRESS LENGTH [OUTDIR], each number 0x and one to eight hex digits.
readBulkArguments :: [String] -> Maybe (Int, Int, Maybe FilePath)
readBulkArguments tokens = case tokens of
  [address,size] -> (,,) <$> hex address <*> hex size <*> Just Nothing
  [address,size,directory] | take 1 directory /= "-" -> (,,) <$> hex address <*> hex size <*> Just (Just directory)
  _ -> Nothing
  where
    hex ('0':'x':digits) | not (null digits) && length digits <= 8 && all isHexDigit digits =
      case readHex digits of [(value,"")] -> Just value; _ -> Nothing
    hex _ = Nothing

bulkCaptureName :: BulkWindow -> String
bulkCaptureName window = "ram-"++probeName (bulkWindowAddress window)++"-"++probeName (bulkWindowBytes window)++".bin"

data ReadBulkReport = ReadBulkReport
  { reportChunks :: Int, reportBytes :: Int, reportCapacity :: Int
  , reportBulkNanoseconds :: Word64, reportSha256 :: String }
  deriving (Eq, Show)

-- The rate covers the E1 and bulk-IN transfers and the capture writes.
readBulkResultLine :: ReadBulkReport -> String
readBulkResultLine r =
  "read-bulk: "++show (reportChunks r)++" chunks;bytes="++show (reportBytes r)++";capacity=0x"++probeName (reportCapacity r)
  ++";sha256="++reportSha256 r++";bulk-ms="++show (reportBulkNanoseconds r `div` 1000000)
  ++";bytes-per-s="++show (fromIntegral (reportBytes r) * 1000000000 `div` max 1 (toInteger (reportBulkNanoseconds r)))
  ++";no write sent, so the session needs no supply cut"

runReadBulk :: BulkWindow -> Clock -> UsbApi -> FilePath -> IO ReadBulkReport
runReadBulk window clock api dir = runReadBulkCore window clock api dir `catch` \(e::SomeException) -> do
  _ <- try (recordWithin (readBulkJournalCapBytes window) dir "session-uncertain" (take 4096 (displayException e))) :: IO (Either SomeException ())
  throwIO e

runReadBulkCore :: BulkWindow -> Clock -> UsbApi -> FilePath -> IO ReadBulkReport
runReadBulkCore window clock api dir = do
  let chunks = bulkChunks window
      cap = readBulkJournalCapBytes window
      capturePath = dir </> bulkCaptureName window
  s <- newSessionJournaling FailingOutcomes clock api dir cap (readBulkAggregateDeadlineMs window)
  held <- newHeld
  openedSession <- newIORef Nothing
  completed <- newIORef (0::Int)
  report <- newIORef Nothing
  note s "prepared" ("read-bulk;address=0x"++probeName (bulkWindowAddress window)++";length="++show (bulkWindowBytes window)
                     ++";chunks="++show (length chunks)++";chunk-bytes="++show bulkChunkBytes++";writes=none")
  mask $ \restore -> do
    result <- try $ restore $ do
      device <- acquireDevice s held
      let input = inputEndpoint (descriptors device)
          output = outputEndpoint (descriptors device)
      _ <- tx s (P.controlTransfer (P.control 0xc0 8 0)) 12
      reply <- tx s (P.BulkIn input 8) 8
      let sid = fromIntegral (reply!!0) + 256*fromIntegral (reply!!1)
          stop label response reason = do
            note s "read-bulk-stop" (label++";reply="++P.hexBytes response++";"++reason)
            P.reject reason
      writeIORef openedSession (Just (device,sid))
      unless (drop 2 reply == openReplyTail)
        (stop "open" reply ("the C0 reply tail "++P.hexBytes (drop 2 reply)++" differs from the constant "++P.hexBytes openReplyTail))
      _ <- tx s (P.controlTransfer (P.control 0xd0 32 sid)) 12
      _ <- tx s (P.BulkOut output P.identityPacket) 32
      _ <- tx s (P.controlTransfer (P.control 0xd1 128 sid)) 12
      identity <- tx s (P.BulkIn input 128) 128
      model <- either (stop "identity" identity) pure (P.decodeProduct identity)
      note s "identity" model
      _ <- tx s (P.controlTransfer (P.control 0xd0 readRequestBytes sid)) 12
      _ <- tx s (P.BulkOut output (readRequestFrame gateWindow)) readRequestBytes
      _ <- tx s (P.controlTransfer (P.control 0xd1 readReplyBytes sid)) 12
      configReply <- tx s (P.BulkIn input readReplyBytes) readReplyBytes
      config <- either (stop "config-000000" configReply) pure (decodeReadReply gateWindow configReply)
      writePrivateBytes (dir </> "config-000000.bin") (B.pack config)
      note s "window-complete" ("config-000000;sha256="++sha256 (B.pack config))
      let gate = config !! gateIndex
      when (gate == 0)
        (stop "gate" [] "0xf7 reads 0x00: the source pointer cannot be read back; no query sent")
      note s "gate" ("f7=0x"++P.hexBytes [gate])
      _ <- tx s (P.controlTransfer (P.control 0xd0 bulkQueryRequestBytes sid)) 12
      _ <- tx s (P.BulkOut output bulkQueryFrame) bulkQueryRequestBytes
      _ <- tx s (P.controlTransfer (P.control 0xd1 bulkQueryReplyBytes sid)) 12
      queryReply <- tx s (P.BulkIn input bulkQueryReplyBytes) bulkQueryReplyBytes
      capacity <- either (stop "query" queryReply) pure (decodeBulkQueryReply queryReply)
      note s "query" ("tag=0x80;capacity=0x"++probeName capacity)
      _ <- tx s (P.controlTransfer (P.control 0xd0 debugRequestBytes sid)) 12
      _ <- tx s (P.BulkOut output (routeRequestFrame bulkSourceStateAddress bulkSourceStateBytes)) debugRequestBytes
      _ <- tx s (P.controlTransfer (P.control 0xd1 (routeReplyBytes bulkSourceStateBytes) sid)) 12
      stateReply <- tx s (P.BulkIn input (routeReplyBytes bulkSourceStateBytes)) (routeReplyBytes bulkSourceStateBytes)
      state <- either (stop "source-state" stateReply) pure (decodeRouteReply bulkSourceStateAddress bulkSourceStateBytes stateReply)
      either (stop "source-state" stateReply) pure (decodeBulkSourceState capacity state)
      note s "source-state" ("pointer=0x"++probeName bulkSourcePointer++";capacity=0x"++probeName capacity)
      -- Every E1 frame is durable, in send order, before the first is sent.
      planTick <- nowNS clock
      recordBatchWithin cap dir "outgoing-before-dispatch"
        [ (show planTick, show (bulkCommandFrame sid chunk)++";bytes="++P.hexBytes (bulkCommand (bulkOffsetFor address) size sid))
        | chunk@(address,size) <- chunks ]
      let chunkTransfer packet count = do
            ms <- remaining (sessionClock s) (sessionDeadline s)
            before <- nowNS clock
            (n,bytes) <- dispatchTransfer s packet (min ms transferTimeoutMs)
            after <- nowNS clock
            when (after >= sessionDeadline s || after >= before + fromIntegral transferTimeoutMs*1000000)
              (P.reject ("chunk deadline exceeded;count="++show n))
            unless (n == count && (case packet of P.BulkIn _ _ -> length bytes == count; _ -> null bytes))
              (P.reject ("short/partial/oversized transfer: count "++show n++" of "++show count++"; no retry"))
            pure bytes
          appendCapture fd payload = B.useAsCStringLen payload $ \(buffer,size) -> do
            written <- fdWriteBuf fd (castPtr buffer) (fromIntegral size)
            unless (written == fromIntegral size) (P.reject "short capture write")
      bracket (openCapture capturePath) closeFd $ \fd -> do
        started <- nowNS clock
        whole <- newIORef H.init
        forM_ (zip [0::Int ..] chunks) $ \(index,chunk@(address,size)) ->
          (do
            _ <- chunkTransfer (bulkCommandFrame sid chunk) 12
            payload <- B.pack <$> chunkTransfer (P.BulkIn input size) size
            appendCapture fd payload
            modifyIORef' whole (`H.update` payload)
            time <- nowNS clock
            note s "chunk-complete" ("index="++show index++";address=0x"++probeName address++";length="++show size
                                     ++";sha256="++P.hexBytes (B.unpack (H.hash payload))++";time="++show time)
            modifyIORef' completed (+1))
          `catch` \(e::SomeException) -> do
            fileSynchronise fd `catch` \(_::SomeException) -> pure ()
            note s "read-bulk-stop" ("chunk="++show index++";address=0x"++probeName address++";length="++show size++";"++displayException e)
            throwIO e
        fileSynchronise fd
        finished <- nowNS clock
        digest <- P.hexBytes . B.unpack . H.finalize <$> readIORef whole
        note s "window-complete" (bulkCaptureName window++";bytes="++show (bulkWindowBytes window)++";sha256="++digest
                                  ++";bulk-ns="++show (finished - started))
        writeIORef report (Just (ReadBulkReport (length chunks) (bulkWindowBytes window) capacity (finished - started) digest))
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
    outcome <- readIORef report
    case (result,failures,outcome) of
      (_, _:_, _) -> P.reject ("cleanup uncertain: "++show failures)
      (Left (e::SomeException),[],_) -> throwIO e
      (Right (),[],Just finished) -> do
        note s "read-bulk-complete-stock-unverified" (show done++" chunks;no write sent")
        pure finished
      (Right (),[],Nothing) -> P.reject "read-bulk ended without its window record"
