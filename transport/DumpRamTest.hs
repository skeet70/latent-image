module Main where

import Control.Exception
import Control.Monad (forM_, unless)
import qualified Data.ByteString as B
import Data.IORef
import Data.List (isInfixOf, isPrefixOf, tails)
import Data.Word (Word8)
import Numeric (showHex)
import System.Directory
import System.FilePath
import System.Posix.Process (getProcessID)
import FujiAF.Transport
import FujiAF.Read (readOpcode, readRequestBytes, readReplyBytes, word32At, word32le)
import FujiAF.Probe
  ( configReplyBytes, configWriteBytes, configWriteLength, debugOpcode
  , debugRequestBytes, gateIndex, gateReadbackEnabled, gateReadbackRestored )
import FujiAF.DumpRam
import FujiAF.SHA256 (sha256)
import qualified FujiAF.Broker as P

check :: String -> Bool -> IO ()
check label yes = unless yes (error label)

syntheticDescriptors :: Descriptors
syntheticDescriptors = Descriptors 1 0 0 255 0 0 0x81 2 512 512 0 1 []

sessionSid :: Int
sessionSid = 0x117c

-- The exact table the command ships: name, start, end in address order.
expectedTable :: [(String,Int,Int)]
expectedTable =
  [ ("loaders", 0x00040000, 0x0004db00)
  , ("loaders", 0x000d0000, 0x000ddb00)
  , ("id4-image-0", 0x0061c000, 0x0071c000)
  , ("id4-image-1", 0x0071c000, 0x0081c000)
  , ("id4-image-2", 0x0081c000, 0x0089ec00)
  , ("id4-bss-0", 0x0089ec00, 0x0099ec00)
  , ("id4-bss-1", 0x0099ec00, 0x00a9ec00)
  , ("id4-bss-2", 0x00a9ec00, 0x00b9ec00)
  , ("id4-bss-3", 0x00b9ec00, 0x00bf1a00)
  , ("id5-image-0", 0x00bf1a00, 0x00cf1a00)
  , ("id5-image-1", 0x00cf1a00, 0x00df1a00)
  , ("id5-image-2", 0x00df1a00, 0x00ef1a00)
  , ("id5-image-3", 0x00ef1a00, 0x00ff1a00)
  , ("id5-image-4", 0x00ff1a00, 0x010e3900)
  , ("plain-0", 0x01100000, 0x01200000)
  , ("plain-1", 0x01200000, 0x01300000)
  , ("plain-2", 0x01300000, 0x01400000)
  , ("plain-3", 0x01400000, 0x01460000)
  , ("id5-bss-0", 0x01460000, 0x01560000)
  , ("id5-bss-1", 0x01560000, 0x01660000)
  , ("id5-bss-2", 0x01660000, 0x0171bf00)
  , ("high-known", 0x400a0000, 0x400c0800)
  , ("high-known", 0x409c2800, 0x409c3000)
  , ("high-known", 0x41acd800, 0x41acdc00) ]

-- The listed ranges the split parts must tile exactly.
listedRanges :: [(String,[String],Int,Int)]
listedRanges =
  [ ("id4-bss", ["id4-bss-0","id4-bss-1","id4-bss-2","id4-bss-3"], 0x0089ec00, 0x00bf1a00)
  , ("id5-bss", ["id5-bss-0","id5-bss-1","id5-bss-2"], 0x01460000, 0x0171bf00)
  , ("id4-image", ["id4-image-0","id4-image-1","id4-image-2"], 0x0061c000, 0x0089ec00)
  , ("id5-image", ["id5-image-0","id5-image-1","id5-image-2","id5-image-3","id5-image-4"], 0x00bf1a00, 0x010e3900)
  , ("plain", ["plain-0","plain-1","plain-2","plain-3"], 0x01100000, 0x01460000) ]

expectedCounts :: [(String,Int)]
expectedCounts =
  [ ("loaders",454)
  , ("id4-image-0",4229), ("id4-image-1",4229), ("id4-image-2",2160)
  , ("id4-bss-0",4229), ("id4-bss-1",4229), ("id4-bss-2",4229), ("id4-bss-3",1369)
  , ("id5-image-0",4229), ("id5-image-1",4229), ("id5-image-2",4229), ("id5-image-3",4229), ("id5-image-4",3996)
  , ("plain-0",4229), ("plain-1",4229), ("plain-2",4229), ("plain-3",1586)
  , ("id5-bss-0",4229), ("id5-bss-1",4229), ("id5-bss-2",3104)
  , ("high-known",551) ]

rowsOf :: String -> [(Int,Int)]
rowsOf name = [(start,end) | (rowName,start,end) <- expectedTable, rowName == name]

-- One request reads dumpRequestLength bytes and each range ends with its
-- remainder, so a range needs ceil(size / dumpRequestLength) requests.
formulaChunks :: (Int,Int) -> [(Int,Int)]
formulaChunks (start,end) =
  [ (address, min dumpRequestLength (end - address))
  | address <- takeWhile (< end) [start, start + dumpRequestLength ..] ]

segmentNamed :: String -> DumpSegment
segmentNamed name = maybe (error ("unknown segment "++name)) id (readSegment name)

-- Synthetic content: one filler byte value per chunk base, so the captured
-- bytes are deterministic per address.
chunkFor :: Int -> Int -> [Word8]
chunkFor address size = replicate size (fromIntegral (address `mod` 251))

-- Reply builders, sized from the request's own length.
configReplyWith :: Int -> Int -> Int -> Int -> Int -> [Word8] -> [Word8]
configReplyWith err1 err2 function offset size payload = concat
  [ word32le readOpcode, word32le err1, word32le err2, word32le 0
  , word32le function, word32le 1, word32le 0, word32le offset, word32le size ]
  ++ payload

configWriteReply :: [Word8]
configWriteReply = configReplyWith 0 0 3 gateIndex configWriteLength (replicate 256 0)

configReadReply :: Word8 -> [Word8]
configReadReply value = configReplyWith 0 0 2 0 256 (readbackPayload value)

readbackPayload :: Word8 -> [Word8]
readbackPayload value = take gateIndex (replicate 256 0x5a) ++ [value] ++ drop (gateIndex+1) (replicate 256 0x5a)

dumpReplyWith :: Int -> Int -> Int -> Int -> Int -> [Word8] -> [Word8]
dumpReplyWith opcode err1 err2 address size payload = concat
  [ word32le opcode, word32le err1, word32le err2, word32le 0
  , word32le 2, word32le 0, word32le 0
  , word32le address, word32le size ]
  ++ payload

dumpReply :: Int -> Int -> [Word8] -> [Word8]
dumpReply address size payload = dumpReplyWith debugOpcode 0 0 address size payload

openSteps :: [P.Step]
openSteps =
  [ P.Step (P.controlTransfer (P.control 0xc0 8 0)) (P.Transferred 12)
  , P.Step (P.BulkIn 0x81 8) (P.Reply [0x7c,0x11,0,0,0,0,1,0]) ]

dumpSteps :: DumpSegment -> (DumpRequest -> P.Outcome) -> [P.Step]
dumpSteps segment outcomeFor = openSteps ++ concatMap step (dumpRequests segment)
  where
    step request = case request of
      DumpClose ->
        [ P.Step (P.controlTransfer (P.control 0xc4 2 sessionSid)) (P.Transferred 12)
        , P.Step (P.BulkIn 0x81 2) (outcomeFor request) ]
      DumpGateWrite value ->
        [ P.Step (P.controlTransfer (P.control 0xd0 configWriteBytes sessionSid)) (P.Transferred 12)
        , P.Step (P.BulkOut 2 (configWriteFrameBytes value)) (P.Transferred configWriteBytes)
        , P.Step (P.controlTransfer (P.control 0xd1 configReplyBytes sessionSid)) (P.Transferred 12)
        , P.Step (P.BulkIn 0x81 configReplyBytes) (outcomeFor request) ]
      DumpGateRead _ ->
        [ P.Step (P.controlTransfer (P.control 0xd0 readRequestBytes sessionSid)) (P.Transferred 12)
        , P.Step (P.BulkOut 2 gateReadbackFrame) (P.Transferred readRequestBytes)
        , P.Step (P.controlTransfer (P.control 0xd1 readReplyBytes sessionSid)) (P.Transferred 12)
        , P.Step (P.BulkIn 0x81 readReplyBytes) (outcomeFor request) ]
      DumpChunk address size ->
        [ P.Step (P.controlTransfer (P.control 0xd0 debugRequestBytes sessionSid)) (P.Transferred 12)
        , P.Step (P.BulkOut 2 (requestFrame address size)) (P.Transferred debugRequestBytes)
        , P.Step (P.controlTransfer (P.control 0xd1 (dumpReplyBytes size) sessionSid)) (P.Transferred 12)
        , P.Step (P.BulkIn 0x81 (dumpReplyBytes size)) (outcomeFor request) ]

-- The audited frame layouts, written out as the expectation.
configWriteFrameBytes :: Int -> [Word8]
configWriteFrameBytes value = concat
  [ word32le readOpcode, replicate 12 0
  , word32le 3, word32le 1
  , word32le 0, word32le gateIndex
  , word32le configWriteLength, [fromIntegral value] ]

gateReadbackFrame :: [Word8]
gateReadbackFrame = concat
  [ word32le readOpcode, replicate 12 0
  , word32le 2, word32le 1
  , word32le 0, word32le 0, word32le 256 ]

requestFrame :: Int -> Int -> [Word8]
requestFrame address size = concat
  [ word32le debugOpcode, replicate 12 0
  , word32le 2, word32le 0, word32le 0
  , word32le address, word32le size ]

c0Control, d0Control, d0WriteControl, d1WindowControl, closeControl :: [Word8]
c0Control = [0xc0,0,0,0,0,0,0,0,0,0x08,0,0]
d0Control = [0xd0,0,0,0,0,0,0,0,0,0x24,0x11,0x7c]
d0WriteControl = [0xd0,0,0,0,0,0,0,0,0,0x25,0x11,0x7c]
d1WindowControl = [0xd1,0,0,0,0,0,0,0,0x01,0x24,0x11,0x7c]
closeControl = [0xc4,0,0,0,0,0,0,0,0,2,0x11,0x7c]

d1Control :: Int -> [Word8]
d1Control size = [0xd1,0,0,0,0,0,0,0,fromIntegral (bytes `div` 256),fromIntegral (bytes `mod` 256),0x11,0x7c]
  where bytes = dumpReplyBytes size

-- The first and last request of the loaders segment, byte for byte.
firstChunkFrame, lastChunkFrame :: [Word8]
firstChunkFrame =
  [ 0x01,0x00,0x20,0x00, 0,0,0,0, 0,0,0,0, 0,0,0,0
  , 0x02,0x00,0x00,0x00, 0,0,0,0, 0,0,0,0
  , 0x00,0x00,0x04,0x00, 0xf8,0x00,0x00,0x00 ]
lastChunkFrame =
  [ 0x01,0x00,0x20,0x00, 0,0,0,0, 0,0,0,0, 0,0,0,0
  , 0x02,0x00,0x00,0x00, 0,0,0,0, 0,0,0,0
  , 0xf0,0xda,0x0d,0x00, 0x10,0x00,0x00,0x00 ]

happyOutcome :: DumpRequest -> P.Outcome
happyOutcome request = case request of
  DumpGateWrite _ -> P.Reply configWriteReply
  DumpGateRead label -> P.Reply (configReadReply (if label == gateReadbackEnabled then 1 else 0))
  DumpChunk address size -> P.Reply (dumpReply address size (chunkFor address size))
  DumpClose -> P.Reply [0x7c,0x11]

mutate :: DumpRequest -> (P.Outcome -> P.Outcome) -> DumpRequest -> P.Outcome
mutate target change action = if action == target then change (happyOutcome target) else happyOutcome action

data CleanupEvent = Released | Closed | Exited deriving (Eq, Show)

runCase :: Clock -> FilePath -> [P.Step] -> IO (Either SomeException Int,[P.Transfer],[CleanupEvent])
runCase = runCaseWith (\_ -> pure ())

-- The dispatch hook lets a test mark the deadline at an exact transfer.
runCaseWith :: (P.Transfer -> IO ()) -> Clock -> FilePath -> [P.Step] -> IO (Either SomeException Int,[P.Transfer],[CleanupEvent])
runCaseWith onDispatch clock dir steps = do
  createDirectoryIfMissing True dir
  emitted <- newIORef []
  events <- newIORef []
  script <- newIORef (take (length steps-2) steps)
  closeScript <- newIORef (drop (length steps-2) steps)
  let device = Candidate "001:2.3" 0x04cb 0xff80 syntheticDescriptors
      api = UsbApi (\_ -> pure ()) (\_ -> pure [device]) (\_ _ -> pure ()) (\_ _ -> pure ())
        (\packet _ -> do
          modifyIORef' emitted (++[packet])
          onDispatch packet
          let isClose = packet == P.controlTransfer (P.control 0xc4 2 sessionSid) || packet == P.BulkIn 0x81 2
              activeScript = if isClose then closeScript else script
          remainingSteps <- readIORef activeScript
          case remainingSteps of
            [] -> P.reject "script exhausted"
            P.Step expected outcome:rest -> do
              writeIORef activeScript rest
              check ("packet differs from the audited sequence: "++show packet) (packet == expected)
              case outcome of
                P.Transferred n -> pure (n,[])
                P.Reply bytes -> pure (length bytes,bytes)
                P.LostReply -> P.reject "injected transport failure")
        (\_ _ -> modifyIORef' events (++[Released]))
        (\_ -> modifyIORef' events (++[Closed]))
        (\_ -> modifyIORef' events (++[Exited]))
  outcome <- try (runDumpRam Loaders clock api dir)
  packets <- readIORef emitted
  cleanupEvents <- readIORef events
  pure (outcome,packets,cleanupEvents)

isChunkBulkOut :: P.Transfer -> Bool
isChunkBulkOut (P.BulkOut _ frame) = word32At 0 frame == debugOpcode
isChunkBulkOut _ = False

expectStop :: FilePath -> (String,String) -> Int -> [P.Step] -> IO ()
expectStop dir (label,fragment) expectedChunkRequests steps = do
  (outcome,packets,cleanupEvents) <- runCase (Clock (pure 0)) dir steps
  let chunkRequestCount = length (filter isChunkBulkOut packets)
  check (label++": session must stop") (either (const True) (const False) outcome)
  check (label++": stop message") (either (isInfixOf fragment . displayException) (const False) outcome)
  check (label++": "++show chunkRequestCount++" chunk requests, expected "++show expectedChunkRequests)
    (chunkRequestCount == expectedChunkRequests)
  check (label++": exactly one protocol close") (length [() | P.ControlWrite _ _ _ _ (0xc4:_) <- packets] == 1)
  check (label++": every host resource released once") (cleanupEvents == [Released,Closed,Exited])
  journal <- readFile (dir </> "journal.v1")
  check (label++": refusal or uncertainty is durable") (("dump-ram-refused" `isInfixOf` journal) || ("session-uncertain" `isInfixOf` journal))

quoted :: String -> [String]
quoted [] = []
quoted ('"':rest) = let (part,rest') = break (== '"') rest in part : quoted (drop 1 rest')
quoted (_:rest) = quoted rest

splitOnChar :: Char -> String -> [String]
splitOnChar c text = case break (== c) text of
  (part, []) -> [part]
  (part, _:rest) -> part : splitOnChar c rest

frameHex :: String -> String
frameHex line = case reverse (filter (isPrefixOf ";bytes=") (tails line)) of
  marker:_ -> takeWhile (`elem` ("0123456789abcdef" :: String)) (drop 7 marker)
  [] -> ""

requestLineFields :: String -> [String]
requestLineFields line = case quoted line of
  [_origin,_clock,_kind,body] -> splitOnChar ';' body
  _ -> []

main :: IO ()
main = do
  pid <- getProcessID
  tmp <- getTemporaryDirectory
  let base = tmp </> ("fuji-af-dump-ram-tests-"++show pid)
  createDirectory base

  -- Segment table invariants over the shipped rows.
  check "the segment table is exactly the listed table"
    ([(segmentName (rangeSegment range), rangeStart range, rangeEnd range) | range <- segmentTable] == expectedTable)
  check "every bound is 256-aligned and nonempty"
    (all (\(_,start,end) -> start `mod` 256 == 0 && end `mod` 256 == 0 && start < end) expectedTable)
  check "every range is at most one part of 1 MiB"
    (all (\(_,start,end) -> end - start <= 0x100000) expectedTable)
  check "the rows are ordered and non-overlapping"
    (and (zipWith (\(_,_,end) (_,start',_) -> end <= start') expectedTable (drop 1 expectedTable)))
  forM_ listedRanges $ \(_,names,start,end) -> do
    let rows = concatMap rowsOf names
    check ("split parts are contiguous within "++show names)
      (and (zipWith (\(_,end') (start',_) -> end' == start') rows (drop 1 rows)))
    check ("split parts tile the listed range exactly "++show (start,end))
      (case (rows, reverse rows) of
         ((firstStart,_):_, (_,lastEnd):_) -> firstStart == start && lastEnd == end
         _ -> False)
  check "multi-range segments keep their listed sub-ranges"
    (rowsOf "loaders" == [(0x00040000,0x0004db00),(0x000d0000,0x000ddb00)]
      && rowsOf "high-known" == [(0x400a0000,0x400c0800),(0x409c2800,0x409c3000),(0x41acd800,0x41acdc00)])

  -- Request counts and the chunk walk.
  forM_ expectedCounts $ \(name,count) -> do
    let segment = segmentNamed name
        rows = rowsOf name
    check ("request count of "++name) (dumpRequestCount segment == count)
    check ("ceil(size / "++show dumpRequestLength++") requests for "++name)
      (sum [((end - start) + dumpRequestLength - 1) `div` dumpRequestLength | (start,end) <- rows] == count)
    check ("chunks follow the "++show dumpRequestLength++"-byte walk for "++name)
      (segmentChunks segment == concatMap formulaChunks rows)
  check "every chunk carries 1..dumpRequestLength bytes"
    (all (\(_,size) -> size >= 1 && size <= dumpRequestLength) (concatMap segmentChunks [minBound..maxBound]))
  check "the loaders walk covers a full request and the final short request"
    (take 1 (segmentChunks Loaders) == [(0x00040000, dumpRequestLength)]
      && take 1 (drop 226 (segmentChunks Loaders)) == [(0x0004daf0, 16)]
      && take 1 (drop 227 (segmentChunks Loaders)) == [(0x000d0000, dumpRequestLength)]
      && take 1 (reverse (segmentChunks Loaders)) == [(0x000ddaf0, 16)])
  check "the first and last request frames are the audited bytes"
    (dumpRequestFrame 0x00040000 dumpRequestLength == firstChunkFrame
      && dumpRequestFrame 0x000ddaf0 16 == lastChunkFrame)
  check "the reply size derives from the request length"
    (dumpReplyBytes dumpRequestLength == 284 && dumpReplyBytes 16 == 52)

  -- Journal and deadline bounds from the request count.
  check "the journal cap for a 1 MiB segment"
    (dumpJournalCapFor (dumpRequestCount Id4Bss0) == 4396032)
  check "the journal cap for the loaders segment"
    (dumpJournalCapFor (dumpRequestCount Loaders) == 530432)
  check "the aggregate deadline for a 1 MiB segment"
    (dumpAggregateDeadlineFor (dumpRequestCount Id4Bss0) == 84700000)
  check "the aggregate deadline for the loaders segment"
    (dumpAggregateDeadlineFor (dumpRequestCount Loaders) == 9200000)

  -- Unknown segments and extra arguments are rejected at the CLI boundary.
  check "every segment name round-trips"
    (all (\segment -> readSegment (segmentName segment) == Just segment) [minBound..maxBound])
  check "unknown segment names are rejected"
    (all (\name -> readSegment name == Nothing) ["id4-bss-4","high-known2","Loaders","","0x00040000","loaders "])
  check "the command shape is SEGMENT and at most one OUTDIR"
    (dumpSegmentArguments ["loaders"] == Just (Loaders, Nothing)
      && dumpSegmentArguments ["loaders","/tmp/x"] == Just (Loaders, Just "/tmp/x"))
  check "missing segments, unknown segments and extra arguments are rejected"
    (dumpSegmentArguments [] == Nothing
      && dumpSegmentArguments ["bogus"] == Nothing
      && dumpSegmentArguments ["loaders","/a","/b"] == Nothing)

  -- One full session over the smallest multi-range segment.
  (happy,packets,cleanupEvents) <- runCase (Clock (pure 0)) (base </> "happy") (dumpSteps Loaders happyOutcome)
  check ("happy session completes 454 requests: "++show happy) (either (const False) (== 454) happy)
  check "happy session releases every resource once" (cleanupEvents == [Released,Closed,Exited])
  captured <- B.readFile (base </> "happy" </> "loaders.bin")
  let expectedCapture = B.pack (concatMap (uncurry chunkFor) (segmentChunks Loaders))
  check "the segment file holds the ranges in address order" (captured == expectedCapture)
  check "the segment file size is the segment's byte count" (B.length captured == 112128)
  enabled <- B.readFile (base </> "happy" </> "gate-readback-enabled.bin")
  restored <- B.readFile (base </> "happy" </> "gate-readback-restored.bin")
  check "enable readback window" (B.unpack enabled == readbackPayload 1)
  check "restore readback window" (B.unpack restored == readbackPayload 0)

  -- The outgoing file is the deterministic frame sequence, in send order.
  outgoingLog <- readFile (base </> "happy" </> "outgoing.v1")
  let expectedOutgoing =
        [ c0Control, d0WriteControl, configWriteFrameBytes 1, d1WindowControl
        , d0Control, gateReadbackFrame, d1WindowControl ]
        ++ concatMap (\(address,size) -> [d0Control, requestFrame address size, d1Control size]) (segmentChunks Loaders)
        ++ [ d0WriteControl, configWriteFrameBytes 0, d1WindowControl
           , d0Control, gateReadbackFrame, d1WindowControl
           , closeControl ]
  check "the outgoing file equals the deterministic frame sequence"
    (map frameHex (lines outgoingLog) == map P.hexBytes expectedOutgoing)

  -- The per-request summary lines.
  happyJournal <- readFile (base </> "happy" </> "journal.v1")
  let requestLines = filter ("request-complete" `isInfixOf`) (lines happyJournal)
      summaries =
        [ (index, "index="++show index++";address=0x"++hex8 address++";length="++show size
                  ++";status=(0,0);sha256="++sha256 (B.pack (chunkFor address size))++";time=")
        | (index,(address,size)) <- zip [0::Int ..] (segmentChunks Loaders) ]
  check "one summary line per request" (length requestLines == 454)
  forM_ summaries $ \(index,prefix) ->
    check ("summary line of request "++show index++" carries its address, length, status and hash")
      (any (prefix `isInfixOf`) requestLines)
  check "the summary schema carries index, address, length, status, sha256 and time"
    (all (\line -> map (takeWhile (/= '=')) (requestLineFields line)
                    == ["index","address","length","status","sha256","time"]) requestLines)
  check "the summary time is a decimal monotonic clock"
    (all (\line -> case requestLineFields line of
           [_index,_address,_length,_status,_sha,time] -> all (`elem` ['0'..'9']) (drop 5 time)
           _ -> False) requestLines)
  check "one segment-complete record with the file hash"
    (length (filter ("segment-complete" `isInfixOf`) (lines happyJournal)) == 1
      && ("segment=loaders;bytes=112128;sha256="++sha256 captured) `isInfixOf` happyJournal)

  -- The gate enable and readback precede the first request; the restore and
  -- its readback follow the last request and precede C4.
  let indexed = zip [0::Int ..] packets
      chunkIndexes = [index | (index,packet) <- indexed, isChunkBulkOut packet]
      writeIndexes = [index | (index,P.BulkOut _ frame) <- indexed, word32At 0 frame == readOpcode && word32At 0x10 frame == 3]
      readbackIndexes = [index | (index,P.BulkOut _ frame) <- indexed, word32At 0 frame == readOpcode && word32At 0x10 frame == 2]
      closeIndexes = [index | (index,P.ControlWrite _ _ _ _ (byte:_)) <- indexed, byte == 0xc4]
      writeValues = [value | (_,P.BulkOut _ frame) <- indexed, word32At 0 frame == readOpcode && word32At 0x10 frame == 3, value <- take 1 (reverse frame)]
  check "the gate enable and its readback precede the first request"
    (case (writeIndexes, readbackIndexes, chunkIndexes) of
       (enableWrite:_, enableRead:_, firstRequest:_) -> enableWrite < firstRequest && enableRead < firstRequest
       _ -> False)
  check "the emitted gate values are 1 then 0" (writeValues == [1,0])
  check "the restore write and its readback follow the last request and precede C4"
    (case (writeIndexes, readbackIndexes, reverse chunkIndexes, closeIndexes) of
       (_:restoreWrite:_, _:restoreRead:_, lastRequest:_, firstClose:_) ->
         restoreWrite > lastRequest && restoreRead > lastRequest
           && restoreWrite < firstClose && restoreRead < firstClose
       _ -> False)
  let emittedFrames = [frame | (_,P.BulkOut _ frame) <- indexed, word32At 0 frame == debugOpcode]
  check "the first and last emitted request frames are the audited bytes"
    (case (emittedFrames, reverse emittedFrames) of
       (firstFrame:_, lastFrame:_) -> firstFrame == firstChunkFrame && lastFrame == lastChunkFrame
       _ -> False)

  -- Stop conditions, each against the exact reply that must cause it.
  let firstChunk = DumpChunk 0x00040000 dumpRequestLength
      fourthChunk = DumpChunk 0x000402e8 dumpRequestLength
      lostChunk = DumpChunk 0x000404d8 dumpRequestLength
      stopCases =
        [ ("gate enable is not visible", "gate enable is not visible", 0, dumpSteps Loaders (mutate (DumpGateRead gateReadbackEnabled) (const (P.Reply (configReadReply 0)))))
        , ("readback refused", "rejected arguments", 0, dumpSteps Loaders (mutate (DumpGateRead gateReadbackEnabled) (const (P.Reply (configReplyWith 9 3 2 0 256 (readbackPayload 1))))))
        , ("closed gate", "closed gate", 1, dumpSteps Loaders (mutate firstChunk (const (P.Reply (dumpReplyWith debugOpcode 1 0xffff 0 0 (replicate dumpRequestLength 0))))))
        , ("echoed address mismatch", "differs from the request", 4, dumpSteps Loaders (mutate fourthChunk (const (P.Reply (dumpReply (0x000402e8+4) dumpRequestLength (replicate dumpRequestLength 7)))))) ]
  forM_ (zip [(0::Int)..] stopCases) $ \(index,(label,fragment,expected,steps)) ->
    expectStop (base </> ("stop-"++show index)) (label,fragment) expected steps

  -- The failing reply is journaled in full with its request context.
  refusedJournal <- readFile (base </> "stop-3" </> "journal.v1")
  let badReply = dumpReply (0x000402e8+4) dumpRequestLength (replicate dumpRequestLength 7)
  check "the failing reply is journaled in full with its context"
    (("dump-ram-refused" `isInfixOf` refusedJournal)
      && ("chunk=3;address=0x000402e8;length=248;" `isInfixOf` refusedJournal)
      && (P.hexBytes badReply `isInfixOf` refusedJournal))
  refusedCapture <- B.readFile (base </> "stop-3" </> "loaders.bin")
  check "the completed requests before the refusal stay captured"
    (B.length refusedCapture == 3*dumpRequestLength
      && length (filter ("request-complete" `isInfixOf`) (lines refusedJournal)) == 3)

  -- The restore readback must show the restored value before C4.
  (restoreOutcome,restorePackets,restoreCleanup) <- runCase (Clock (pure 0)) (base </> "restore-not-visible")
    (dumpSteps Loaders (mutate (DumpGateRead gateReadbackRestored) (const (P.Reply (configReadReply 1)))))
  check "restore not visible stops the session"
    (either (isInfixOf "gate restore is not visible" . displayException) (const False) restoreOutcome)
  restoreCapture <- B.readFile (base </> "restore-not-visible" </> "loaders.bin")
  check "restore not visible keeps the segment file and attempts one close"
    (length [() | P.ControlWrite _ _ _ _ (0xc4:_) <- restorePackets] == 1
      && restoreCleanup == [Released,Closed,Exited]
      && B.length restoreCapture == 112128)

  -- A transport failure mid-session leaves the uncertainty record.
  (lostOutcome,lostPackets,lostCleanup) <- runCase (Clock (pure 0)) (base </> "lost-reply")
    (dumpSteps Loaders (mutate lostChunk (const P.LostReply)))
  check "a lost reply stops the session"
    (either (isInfixOf "injected transport failure" . displayException) (const False) lostOutcome)
  check "a lost reply still attempts one close and releases everything"
    (length [() | P.ControlWrite _ _ _ _ (0xc4:_) <- lostPackets] == 1 && lostCleanup == [Released,Closed,Exited])
  lostJournal <- readFile (base </> "lost-reply" </> "journal.v1")
  check "a lost reply journals the uncertainty with its context"
    (("dump-ram-refused" `isInfixOf` lostJournal) && ("chunk=5;" `isInfixOf` lostJournal))

  -- An expired deadline stops before the next dispatch and, as in every
  -- command, prevents further I/O including the close and the host cleanup.
  jump <- newIORef (0::Int)
  let jumpingClock = Clock (do
        modifyIORef' jump (+1)
        calls <- readIORef jump
        pure (if calls <= 90 then 0 else 1000000000000000000))
  (deadlineOutcome,deadlinePackets,deadlineCleanup) <- runCase jumpingClock (base </> "deadline") (dumpSteps Loaders happyOutcome)
  check "an expired deadline stops the session"
    (either (isInfixOf "deadline" . displayException) (const False) deadlineOutcome)
  check "an expired deadline prevents further transfers and cleanup I/O"
    (length (filter isChunkBulkOut deadlinePackets) < 454 && deadlineCleanup == [])
  deadlineJournal <- readFile (base </> "deadline" </> "journal.v1")
  check "an expired deadline is durable" ("deadline" `isInfixOf` deadlineJournal)

  -- A reply that arrives at full count but past the deadline is journaled in
  -- full with its request context before the stop.
  lateJump <- newIORef False
  let lateClock = Clock (readIORef lateJump >>= \late -> pure (if late then 1000000000000000000 else 0))
      markLate packet = case packet of
        P.BulkIn _ size | size == dumpReplyBytes dumpRequestLength -> writeIORef lateJump True
        _ -> pure ()
  (lateOutcome,_latePackets,lateCleanup) <- runCaseWith markLate lateClock (base </> "late-reply") (dumpSteps Loaders happyOutcome)
  check "a full reply past the deadline stops the session"
    (either (isInfixOf "deadline" . displayException) (const False) lateOutcome)
  let lateReply = dumpReply 0x00040000 dumpRequestLength (chunkFor 0x00040000 dumpRequestLength)
  lateJournal <- readFile (base </> "late-reply" </> "journal.v1")
  check "the late reply is journaled in full with its request context"
    (("chunk deadline exceeded" `isInfixOf` lateJournal)
      && ("chunk=0;address=0x00040000;length=248;" `isInfixOf` lateJournal)
      && (P.hexBytes lateReply `isInfixOf` lateJournal))
  check "a late reply prevents further transfers and cleanup I/O" (lateCleanup == [])

  -- The C0 reply decides the session id; a wrong length stops before any D0.
  let happySteps = dumpSteps Loaders happyOutcome
  (shortOpen,shortOpenPackets,shortOpenCleanup) <- runCase (Clock (pure 0)) (base </> "short-open")
    (take 1 happySteps ++ [P.Step (P.BulkIn 0x81 8) (P.Reply (replicate 7 0))] ++ drop 2 happySteps)
  check "short C0 reply stops before D0 and cannot invent a close"
    (either (const True) (const False) shortOpen && shortOpenPackets == [P.controlTransfer (P.control 0xc0 8 0),P.BulkIn 0x81 8])
  check "short C0 reply releases every resource once" (shortOpenCleanup == [Released,Closed,Exited])

  -- Decoding is exact, checked apart from the session.
  check "exact request decode"
    (decodeDumpReply 0x00040000 dumpRequestLength (dumpReply 0x00040000 dumpRequestLength (replicate dumpRequestLength 7))
      == Right (DumpReply (0,0) (replicate dumpRequestLength 7)))
  forM_ [ dumpReplyWith debugOpcode 1 0xffff 0 0 (replicate dumpRequestLength 0)
        , dumpReplyWith debugOpcode 9 3 0 0 (replicate dumpRequestLength 0)
        , dumpReplyWith debugOpcode 2 7 0 0 (replicate dumpRequestLength 0)
        , dumpReplyWith 0x00200002 0 0 0 dumpRequestLength (replicate dumpRequestLength 0)
        , take (dumpReplyBytes dumpRequestLength - 1) (dumpReply 0x00040000 dumpRequestLength (replicate dumpRequestLength 7))
        , dumpReply (0x00040000+4) dumpRequestLength (replicate dumpRequestLength 7)
        , dumpReply 0x00040000 (dumpRequestLength-1) (replicate (dumpRequestLength-1) 7)
        , take 12 (dumpReply 0x00040000 dumpRequestLength (replicate dumpRequestLength 7)) ++ word32le 1 ++ drop 16 (dumpReply 0x00040000 dumpRequestLength (replicate dumpRequestLength 7)) ] $ \bytes ->
    check "request reply refusal" (either (const True) (const False) (decodeDumpReply 0x00040000 dumpRequestLength bytes))
  check "the short final request decodes at its own length"
    (decodeDumpReply 0x000ddaf0 16 (dumpReply 0x000ddaf0 16 (replicate 16 7))
      == Right (DumpReply (0,0) (replicate 16 7)))

  removeDirectoryRecursive base
  putStrLn "dump-ram: table, count, frame, gate, journal, deadline and decoding checks passed"

hex8 :: Int -> String
hex8 value = replicate (8-length digits) '0' ++ digits
  where digits = showHex value ""
