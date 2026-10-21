-- Read-only RAM watch over D0 opcode 0x200001 function 2 on a body whose
-- adjustment index 0xf7 is already nonzero, stored since
-- E-STORED-F7-COMMIT-RUN-150: C0 with the constant reply tail, the tag-4
-- identity, config window 0x000000 read to confirm 0xf7 != 0, the plan's
-- windows one request each for one or more passes, C4.
--
-- The bulk OUT frames are the identity request, one config function 2 read
-- and the function 2 RAM reads; no config write, RAM write or gate write
-- exists here. Function 2 checks only 0xf7 and its length, and neither the
-- D0 dispatcher nor the handler reads an application or service mode
-- (reports/stored-f7-ordinary-usb-read.md 1.4 and 2), so the same exchange
-- serves a jig boot and an ordinary boot's ff80 service mode 3.
--
-- Each copy runs with interrupts off on the reading core for at most 248
-- bytes, and a window the running camera is writing reads back torn.
module FujiAF.ReadRam
  ( Pacing, mkPacing, singlePass, pacingPasses, pacingIntervalMs
  , readRamMaxPasses, readRamMinIntervalMs, readRamMaxIntervalMs
  , RamWatch, mkRamWatch, watchWindows, watchPacing
  , WatchOperation (..), watchOperation, watchOperationName
  , sdHostWindows, sdHostWatch, pacingArguments
  , readRamRequestLength, readRamRequestCap, readRamRequestCount
  , readRamAggregateDeadlineMs, readRamJournalCapBytes
  , readRamArguments, ramCaptureName, gateWindow, readRamDramRanges
  , runReadRam
  ) where

import Control.Exception
import Control.Monad (forM_, unless, when)
import qualified Data.ByteString as B
import Data.Char (isDigit)
import Data.IORef
import Data.List (intercalate, isPrefixOf)
import System.FilePath ((</>))
import FujiAF.Transport
import FujiAF.Read
  ( ReadWindow(..), decodeReadReply, journalCapFor, journalCeilingBytes, journalFixedBytes
  , journalPerWindowBytes, readReplyBytes, readRequestBytes, readRequestFrame, sessionFixedTransfers )
import FujiAF.Probe (debugRequestBytes, gateIndex, probeName, transfersPerRequest)
import FujiAF.Routes (RouteWindow(..), decodeRouteReply, routeReplyBytes, routeRequestFrame, validateRoutePlan)
import FujiAF.DumpRam (dumpRequestLength)
import FujiAF.StoredGate (openReplyTail)
import FujiAF.SHA256 (sha256)
import qualified FujiAF.Broker as P

-- One request per plan window, so a window is at most the largest fully
-- delivered function 2 reply's data.
readRamRequestLength :: Int
readRamRequestLength = dumpRequestLength

readRamMaxPasses, readRamMinIntervalMs, readRamMaxIntervalMs :: Int
readRamMaxPasses = 600
readRamMinIntervalMs = 100
readRamMaxIntervalMs = 10000

-- The read requests one session may send: the read-config journal ceiling at
-- journalPerWindowBytes per request, 2032.
readRamRequestCap :: Int
readRamRequestCap = (journalCeilingBytes - journalFixedBytes) `div` journalPerWindowBytes

-- intervalMs is the pause between the last reply of one pass and the first
-- request of the next.
data Pacing = Pacing { pacingPasses :: Int, pacingIntervalMs :: Int }
  deriving (Eq, Show)

singlePass :: Pacing
singlePass = Pacing 1 0

mkPacing :: Int -> Int -> Either String Pacing
mkPacing passes intervalMs
  | passes < 1 || passes > readRamMaxPasses =
      Left ("--repeat must be 1.."++show readRamMaxPasses++", not "++show passes)
  | intervalMs < readRamMinIntervalMs || intervalMs > readRamMaxIntervalMs =
      Left ("--interval-ms must be "++show readRamMinIntervalMs++".."++show readRamMaxIntervalMs++", not "++show intervalMs)
  | otherwise = Right (Pacing passes intervalMs)

data WatchOperation = ReadRamOperation | SdHostOperation
  deriving (Eq, Show)

watchOperationName :: WatchOperation -> String
watchOperationName ReadRamOperation = "read-ram"
watchOperationName SdHostOperation = "probe-sd-host"

data RamWatch = RamWatch { watchOperation :: WatchOperation, watchWindows :: [RouteWindow], watchPacing :: Pacing }
  deriving (Eq, Show)

-- The DRAM backing and its 0x40000000 alias. Every other range is a peripheral,
-- boot ROM or unmapped, where a read can wedge the camera.
readRamDramRanges :: [(Int,Int)]
readRamDramRanges = [(0x00000000,0x20000000),(0x40000000,0x60000000)]

mkRamWatch :: [RouteWindow] -> Pacing -> Either String RamWatch
mkRamWatch = watchWithin ReadRamOperation readRamDramRanges

-- The slot's SD host controller at 0xfff50000, limited to the offsets the boot
-- ROM (reports/bootrom-window.md sections 1 and 3) or the application driver
-- accesses. +0x20..+0x23 is the buffer data port, where a read consumes FIFO data.
sdHostRanges :: [(Int,Int)]
sdHostRanges = [(0xfff50000,0xfff50020),(0xfff50024,0xfff5002a),(0xfff5002c,0xfff5003c),(0xfff5003e,0xfff50040)]

sdHostWindows :: [RouteWindow]
sdHostWindows =
  [ RouteWindow 0 0xfff50000 32 "SD host +0x00..+0x1f: DMA address, block size and count, argument, transfer mode and command, response"
  , RouteWindow 1 0xfff50024 6 "SD host +0x24..+0x29: present state, host and power control"
  , RouteWindow 2 0xfff5002c 16 "SD host +0x2c..+0x3b: clock, timeout and reset control, interrupt and error status and enables"
  , RouteWindow 3 0xfff5003e 2 "SD host +0x3e..+0x3f: host control 2" ]

sdHostWatch :: Pacing -> Either String RamWatch
sdHostWatch = watchWithin SdHostOperation sdHostRanges sdHostWindows

watchWithin :: WatchOperation -> [(Int,Int)] -> [RouteWindow] -> Pacing -> Either String RamWatch
watchWithin operation ranges windows pacing = do
  let name = watchOperationName operation
  validateRoutePlan windows
  unless (all ((<= readRamRequestLength) . routeWindowBytes) windows)
    (Left (name++" window length must be 1.."++show readRamRequestLength++" bytes, one request per window"))
  forM_ windows $ \window ->
    let start = routeWindowAddress window
        end = start + routeWindowBytes window
    in unless (any (\(low,high) -> low <= start && end <= high) ranges)
         (Left (name++" window 0x"++probeName start++" is outside "
                ++intercalate ", " ["[0x"++probeName low++",0x"++probeName high++")" | (low,high) <- ranges]))
  let requests = length windows * pacingPasses pacing
  unless (requests <= readRamRequestCap)
    (Left ("plan windows times passes is "++show requests++" read requests, above the cap of "++show readRamRequestCap))
  pure (RamWatch operation windows pacing)

readRamRequestCount :: RamWatch -> Int
readRamRequestCount watch = length (watchWindows watch) * pacingPasses (watchPacing watch)

-- Every transfer at 5000 ms: the fixed C0/C4 and native stages, the identity
-- and config requests, the RAM requests, plus the pauses.
readRamAggregateDeadlineMs :: RamWatch -> Int
readRamAggregateDeadlineMs watch =
  transferTimeoutMs*(sessionFixedTransfers + transfersPerRequest*(2 + readRamRequestCount watch))
  + pacingIntervalMs pacing*(pacingPasses pacing - 1)
  where pacing = watchPacing watch

readRamJournalCapBytes :: RamWatch -> Int
readRamJournalCapBytes = journalCapFor . readRamRequestCount

-- The CLI boundary: PLAN_JSON, then optionally --repeat N --interval-ms M,
-- then at most one OUTDIR. Numbers are decimal digits only.
readRamArguments :: [String] -> Maybe (FilePath, Maybe (Int,Int), Maybe FilePath)
readRamArguments (plan:rest)
  | not (isPrefixOf "-" plan) = (\(pacing,directory) -> (plan,pacing,directory)) <$> pacingArguments rest
readRamArguments _ = Nothing

-- Optionally --repeat N --interval-ms M, then at most one OUTDIR.
pacingArguments :: [String] -> Maybe (Maybe (Int,Int), Maybe FilePath)
pacingArguments tokens = case tokens of
  "--repeat":passes:"--interval-ms":interval:more ->
    (,) <$> (Just <$> ((,) <$> decimal passes <*> decimal interval)) <*> outdir more
  more -> (,) Nothing <$> outdir more
  where
    flag = isPrefixOf "-"
    decimal s
      | not (null s) && length s <= 6 && all isDigit s = Just (read s)
      | otherwise = Nothing
    outdir [] = Just Nothing
    outdir [directory] | not (flag directory) = Just (Just directory)
    outdir _ = Nothing

gateWindow :: ReadWindow
gateWindow = ReadWindow 0 0 "adjustment index 0xf7"

-- ram/PPP-AAAAAAAA.bin: three-digit pass, eight-digit lowercase hex address.
ramCaptureName :: Int -> Int -> String
ramCaptureName pass address = replicate (3 - length digits) '0' ++ digits ++ "-" ++ probeName address
  where digits = show pass

runReadRam :: RamWatch -> Clock -> UsbApi -> FilePath -> IO Int
runReadRam watch clock api dir = runReadRamCore watch clock api dir `catch` \(e::SomeException) -> do
  _ <- try (recordWithin (readRamJournalCapBytes watch) dir "session-uncertain" (take 4096 (displayException e))) :: IO (Either SomeException ())
  throwIO e

runReadRamCore :: RamWatch -> Clock -> UsbApi -> FilePath -> IO Int
runReadRamCore watch clock api dir = do
  let windows = watchWindows watch
      Pacing passes intervalMs = watchPacing watch
      captured = dir </> "ram"
  s <- newSession clock api dir (readRamJournalCapBytes watch) (readRamAggregateDeadlineMs watch)
  held <- newHeld
  openedSession <- newIORef Nothing
  completed <- newIORef (0::Int)
  let name = watchOperationName (watchOperation watch)
  note s "prepared" (name++";windows="++show (length windows)++";passes="++show passes
                     ++";interval-ms="++show intervalMs++";read-requests="++show (readRamRequestCount watch)++";writes=none")
  ensurePrivateDirectory captured
  mask $ \restore -> do
    result <- try $ restore $ do
      device <- acquireDevice s held
      let input = inputEndpoint (descriptors device)
          output = outputEndpoint (descriptors device)
      _ <- tx s (P.controlTransfer (P.control 0xc0 8 0)) 12
      reply <- tx s (P.BulkIn input 8) 8
      let sid = fromIntegral (reply!!0) + 256*fromIntegral (reply!!1)
          stop label response reason = do
            note s (name++"-stop") (label++";reply="++P.hexBytes response++";"++reason)
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
        (stop "gate" [] "0xf7 reads 0x00: function 2 answers (1,0xffff) while it is zero; no RAM request sent")
      note s "gate" ("f7=0x"++P.hexBytes [gate])
      forM_ [0 .. passes-1] $ \pass -> do
        when (pass > 0) (pauseMs clock intervalMs)
        tick <- nowNS clock
        note s "pass-start" ("pass="++show pass++";clock-ns="++show tick)
        forM_ windows $ \window -> do
          let address = routeWindowAddress window
              size = routeWindowBytes window
              label = ramCaptureName pass address
          _ <- tx s (P.controlTransfer (P.control 0xd0 debugRequestBytes sid)) 12
          _ <- tx s (P.BulkOut output (routeRequestFrame address size)) debugRequestBytes
          _ <- tx s (P.controlTransfer (P.control 0xd1 (routeReplyBytes size) sid)) 12
          bytes <- tx s (P.BulkIn input (routeReplyBytes size)) (routeReplyBytes size)
          payload <- either (stop label bytes) pure (decodeRouteReply address size bytes)
          writePrivateBytes (captured </> (label++".bin")) (B.pack payload)
          note s "ram-read" ("pass="++show pass
            ++";index="++show (routeWindowIndex window)
            ++";address=0x"++probeName address
            ++";length="++show size
            ++";purpose="++routeWindowPurpose window
            ++";sha256="++sha256 (B.pack payload))
          modifyIORef' completed (+1)
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
        note s (name++"-complete-stock-unverified") (show sent++" read requests;passes="++show passes++";no write sent")
        pure sent
