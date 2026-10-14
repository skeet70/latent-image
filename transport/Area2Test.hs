module Main where

import Control.Exception
import Control.Monad (forM_, unless)
import Data.List (isInfixOf, sort)
import qualified Data.ByteString as B
import Data.IORef
import Data.Word (Word8)
import Numeric (showHex)
import System.Directory
import System.FilePath
import System.Posix.Process (getProcessID)
import FujiAF.Transport
import FujiAF.Read (word32At)
import FujiAF.Area2 (runProbeArea2, area2AggregateDeadlineMs, area2JournalCapBytes)
import qualified FujiAF.Broker as P

check :: String -> Bool -> IO ()
check label yes = unless yes (error label)

syntheticDescriptors :: Descriptors
syntheticDescriptors = Descriptors 1 0 0 255 0 0 0x81 2 512 512 0 1 []

sessionSid :: Int
sessionSid = 0x117c

-- The 27 window bases of area2-invalidation.md section 9, in the order the
-- operation must read them.
specWindows :: [Int]
specWindows =
  [ 0x00828500, 0x0089f900, 0x0089fa00, 0x008a0000, 0x008a0100, 0x008c0000
  , 0x008c0100, 0x008e0000, 0x008e1700, 0x008e1800, 0x00917f00, 0x00919b00
  , 0x00919c00, 0x00919d00, 0x00919e00, 0x00919f00, 0x0091a000, 0x0091a100
  , 0x0091a200, 0x0091a300, 0x0091a400, 0x0091a500, 0x409c2b00, 0x409c2c00
  , 0x409c2d00, 0x409c2e00, 0x409c2f00 ]

-- The request sequence the session must emit: gate enable and its readback,
-- two halves per window, the restore and its readback.
data Request = GateWriteReq Int | GateReadReq Word8 | HalfReq Int deriving (Eq, Show)

requestPlan :: [Request]
requestPlan =
  [ GateWriteReq 1, GateReadReq 1 ]
  ++ concatMap (\window -> [HalfReq window, HalfReq (window + 0x80)]) specWindows
  ++ [ GateWriteReq 0, GateReadReq 0 ]

-- The wire shapes the session must emit, written out from the documented
-- request layouts rather than taken from the operation.
gateWriteFrame :: Int -> [Word8]
gateWriteFrame value =
  [0x02,0x00,0x1a,0x00] ++ replicate 12 0
  ++ [0x03,0x00,0x00,0x00, 0x01,0x00,0x00,0x00, 0x00,0x00,0x00,0x00, 0xf7,0x00,0x00,0x00, 0x01,0x00,0x00,0x00]
  ++ [fromIntegral value]

readbackFrame :: [Word8]
readbackFrame =
  [0x02,0x00,0x1a,0x00] ++ replicate 12 0
  ++ [0x02,0x00,0x00,0x00, 0x01,0x00,0x00,0x00, 0x00,0x00,0x00,0x00, 0x00,0x00,0x00,0x00, 0x00,0x01,0x00,0x00]

halfFrame :: Int -> [Word8]
halfFrame address =
  [0x01,0x00,0x20,0x00] ++ replicate 12 0
  ++ [0x02,0x00,0x00,0x00, 0x00,0x00,0x00,0x00, 0x00,0x00,0x00,0x00]
  ++ le32 address ++ [0x80,0x00,0x00,0x00]

le32 :: Int -> [Word8]
le32 value = [at 0, at 1, at 2, at 3]
  where
    at :: Int -> Word8
    at index = fromIntegral ((value `div` 256^index) `mod` 256)

-- The audited wire bytes of the first window's and the last window's two half
-- requests: opcode 0x00200001, function 2, the address at +0x1c and length
-- 128 at +0x20.
firstWindowFrames, lastWindowFrames :: [[Word8]]
firstWindowFrames =
  [ [0x01,0x00,0x20,0x00,0,0,0,0,0,0,0,0,0,0,0,0,0x02,0x00,0x00,0x00,0,0,0,0,0,0,0,0,0x00,0x85,0x82,0x00,0x80,0x00,0x00,0x00]
  , [0x01,0x00,0x20,0x00,0,0,0,0,0,0,0,0,0,0,0,0,0x02,0x00,0x00,0x00,0,0,0,0,0,0,0,0,0x80,0x85,0x82,0x00,0x80,0x00,0x00,0x00] ]
lastWindowFrames =
  [ [0x01,0x00,0x20,0x00,0,0,0,0,0,0,0,0,0,0,0,0,0x02,0x00,0x00,0x00,0,0,0,0,0,0,0,0,0x00,0x2f,0x9c,0x40,0x80,0x00,0x00,0x00]
  , [0x01,0x00,0x20,0x00,0,0,0,0,0,0,0,0,0,0,0,0,0x02,0x00,0x00,0x00,0,0,0,0,0,0,0,0,0x80,0x2f,0x9c,0x40,0x80,0x00,0x00,0x00] ]

-- Synthetic window content: one byte value per window base, so the captured
-- bytes are deterministic per address.
windowFor :: Int -> [Word8]
windowFor base = replicate 256 (fromIntegral (base `mod` 251))

readbackPayload :: Word8 -> [Word8]
readbackPayload value = take 0xf7 (replicate 256 0x5a) ++ [value] ++ drop 0xf8 (replicate 256 0x5a)

-- Reply shapes, written out from the documented reply layout: opcode, the two
-- error words, the reserved word, function, mailbox, page, in-page offset,
-- length, then the payload.
configReplyWith :: Int -> Int -> Int -> Int -> Int -> [Word8] -> [Word8]
configReplyWith err1 err2 function offset size payload =
  [0x02,0x00,0x1a,0x00] ++ le32 err1 ++ le32 err2 ++ [0,0,0,0]
  ++ le32 function ++ le32 1 ++ le32 0 ++ le32 offset ++ le32 size
  ++ payload

configWriteReply :: [Word8]
configWriteReply = configReplyWith 0 0 3 0xf7 1 (replicate 256 0)

configReadReply :: Word8 -> [Word8]
configReadReply value = configReplyWith 0 0 2 0 256 (readbackPayload value)

halfReplyWith :: Int -> Int -> Int -> [Word8] -> [Word8]
halfReplyWith err1 err2 address payload =
  [0x01,0x00,0x20,0x00] ++ le32 err1 ++ le32 err2 ++ [0,0,0,0]
  ++ [0x02,0x00,0x00,0x00, 0,0,0,0, 0,0,0,0] ++ le32 address ++ [0x80,0x00,0x00,0x00]
  ++ payload

halfReply :: Int -> [Word8] -> [Word8]
halfReply = halfReplyWith 0 0

-- A window reply whose reserved word at +0x0c is nonzero.
reservedHalfReply :: [Word8]
reservedHalfReply = take 12 good ++ [1,0,0,0] ++ drop 16 good
  where good = halfReply 0 (replicate 128 0)

openSteps :: [P.Step]
openSteps =
  [ P.Step (P.controlTransfer (P.control 0xc0 8 0)) (P.Transferred 12)
  , P.Step (P.BulkIn 0x81 8) (P.Reply [0x7c,0x11,0,0,0,0,1,0]) ]

closeSteps :: [P.Step]
closeSteps =
  [ P.Step (P.controlTransfer (P.control 0xc4 2 sessionSid)) (P.Transferred 12)
  , P.Step (P.BulkIn 0x81 2) (P.Reply [0x7c,0x11]) ]

gateSteps :: Int -> P.Outcome -> [P.Step]
gateSteps value outcome =
  [ P.Step (P.controlTransfer (P.control 0xd0 37 sessionSid)) (P.Transferred 12)
  , P.Step (P.BulkOut 2 (gateWriteFrame value)) (P.Transferred 37)
  , P.Step (P.controlTransfer (P.control 0xd1 292 sessionSid)) (P.Transferred 12)
  , P.Step (P.BulkIn 0x81 292) outcome ]

readbackSteps :: P.Outcome -> [P.Step]
readbackSteps outcome =
  [ P.Step (P.controlTransfer (P.control 0xd0 36 sessionSid)) (P.Transferred 12)
  , P.Step (P.BulkOut 2 readbackFrame) (P.Transferred 36)
  , P.Step (P.controlTransfer (P.control 0xd1 292 sessionSid)) (P.Transferred 12)
  , P.Step (P.BulkIn 0x81 292) outcome ]

halfSteps :: Int -> P.Outcome -> [P.Step]
halfSteps address outcome =
  [ P.Step (P.controlTransfer (P.control 0xd0 36 sessionSid)) (P.Transferred 12)
  , P.Step (P.BulkOut 2 (halfFrame address)) (P.Transferred 36)
  , P.Step (P.controlTransfer (P.control 0xd1 164 sessionSid)) (P.Transferred 12)
  , P.Step (P.BulkIn 0x81 164) outcome ]

happyOutcome :: Request -> P.Outcome
happyOutcome request = case request of
  GateWriteReq _ -> P.Reply configWriteReply
  GateReadReq value -> P.Reply (configReadReply value)
  HalfReq address -> P.Reply (halfReply address payload)
    where
      window = windowFor (address - address `mod` 256)
      payload = if address `mod` 256 == 0 then take 128 window else drop 128 window

mutate :: Request -> P.Outcome -> Request -> P.Outcome
mutate target replacement request = if request == target then replacement else happyOutcome request

stepsFor :: (Request -> P.Outcome) -> [P.Step]
stepsFor outcomeFor = openSteps ++ concatMap step requestPlan ++ closeSteps
  where
    step request = case request of
      GateWriteReq value -> gateSteps value (outcomeFor request)
      GateReadReq _ -> readbackSteps (outcomeFor request)
      HalfReq address -> halfSteps address (outcomeFor request)

happySteps :: [P.Step]
happySteps = stepsFor happyOutcome

data CleanupEvent = Released | Closed | Exited deriving (Eq, Show)

runCaseWith :: Clock -> FilePath -> [P.Step] -> IO (Either SomeException Int,[P.Transfer],[CleanupEvent])
runCaseWith clock dir steps = do
  createDirectoryIfMissing True dir
  emitted <- newIORef []
  events <- newIORef []
  -- The C4 cleanup has its own script, independent of how far the session ran.
  script <- newIORef (take (length steps-2) steps)
  closeScript <- newIORef (drop (length steps-2) steps)
  let device = Candidate "001:2.3" 0x04cb 0xff80 syntheticDescriptors
      api = UsbApi (\_ -> pure ()) (\_ -> pure [device]) (\_ _ -> pure ()) (\_ _ -> pure ())
        (\packet _ -> do
          modifyIORef' emitted (++[packet])
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
  outcome <- try (runProbeArea2 clock api dir)
  packets <- readIORef emitted
  cleanupEvents <- readIORef events
  pure (outcome,packets,cleanupEvents)

runCase :: FilePath -> [P.Step] -> IO (Either SomeException Int,[P.Transfer],[CleanupEvent])
runCase = runCaseWith (Clock (pure 0))

-- A clock whose first reading fixes the session start and whose later readings
-- lie past any aggregate deadline.
expiringClock :: IO Clock
expiringClock = do
  calls <- newIORef (0::Int)
  pure $ Clock $ do
    modifyIORef' calls (+1)
    count <- readIORef calls
    pure (if count <= 1 then 0 else 3600*1000000000)

nameOf :: Int -> String
nameOf address = replicate (8-length digits) '0' ++ digits
  where digits = showHex address ""

isWindowRequest :: P.Transfer -> Bool
isWindowRequest (P.BulkOut _ frame) = word32At 0 frame == 0x00200001
isWindowRequest _ = False

-- A stop is checked for its message, the exact number of window requests the
-- session may have emitted before refusing, one protocol close, complete host
-- cleanup and a durable refusal or uncertainty record.
expectStop :: FilePath -> (String,String) -> Int -> [P.Step] -> IO ()
expectStop dir (label,fragment) expectedWindowRequests steps = do
  (outcome,packets,cleanupEvents) <- runCase dir steps
  let windowRequestCount = length (filter isWindowRequest packets)
  check (label++": session must stop") (either (const True) (const False) outcome)
  check (label++": stop message") (either (isInfixOf fragment . displayException) (const False) outcome)
  check (label++": "++show windowRequestCount++" window requests, expected "++show expectedWindowRequests)
    (windowRequestCount == expectedWindowRequests)
  check (label++": exactly one protocol close") (length [() | P.ControlWrite _ _ _ _ (0xc4:_) <- packets] == 1)
  check (label++": every host resource released once") (cleanupEvents == [Released,Closed,Exited])
  journal <- readFile (dir </> "journal.v1")
  check (label++": refusal or uncertainty is durable") (("area2-refused" `isInfixOf` journal) || ("session-uncertain" `isInfixOf` journal))

main :: IO ()
main = do
  pid <- getProcessID
  tmp <- getTemporaryDirectory
  let base = tmp </> ("fuji-af-area2-tests-"++show pid)
  createDirectory base

  (happy,packets,cleanupEvents) <- runCase (base </> "happy") happySteps
  check ("happy session completes 27 windows: "++show happy) (either (const False) (==27) happy)
  check "happy session releases every resource once" (cleanupEvents == [Released,Closed,Exited])

  let bulkFrames = [bytes | P.BulkOut _ bytes <- packets]
      debugFrames = [frame | frame <- bulkFrames, take 4 frame == [0x01,0x00,0x20,0x00]]
      observed = map (word32At 0x1c) debugFrames

  -- The window list and order, over the bytes the camera receives.
  check "the emitted halves are the 27 windows at +0x00 and +0x80 in order"
    (observed == concatMap (\window -> [window, window + 0x80]) specWindows)

  -- The audited request frames of the first and last window.
  check "the first window's two request frames are the audited bytes"
    (take 2 debugFrames == firstWindowFrames)
  check "the last window's two request frames are the audited bytes"
    (drop (length debugFrames-2) debugFrames == lastWindowFrames)

  -- The gate enable and its readback precede every window request; the
  -- restore and its readback follow every one and precede the close.
  check "the gate enable and its readback precede every window request"
    (take 2 bulkFrames == [gateWriteFrame 1, readbackFrame])
  check "the restore and its readback follow every window request"
    (drop (length debugFrames+2) bulkFrames == [gateWriteFrame 0, readbackFrame])
  check "the session emits only the two gate writes, two readbacks and 54 window requests"
    (length bulkFrames == 58 && length debugFrames == 54)
  check "the close is the final pair of the session"
    (drop (length packets-2) packets == [P.controlTransfer (P.control 0xc4 2 sessionSid), P.BulkIn 0x81 2])

  -- Output layout and naming match probe-ram so the analysis reads the
  -- captures the same way, and the captured bytes are the served bytes.
  captures <- listDirectory (base </> "happy" </> "probes")
  check "the captures are the 27 window files and the two gate readbacks"
    (sort captures == sort (map (\window -> nameOf window++".bin") specWindows
      ++ ["gate-readback-enabled.bin","gate-readback-restored.bin"]))
  forM_ specWindows $ \window -> do
    bytes <- B.readFile (base </> "happy" </> "probes" </> (nameOf window++".bin"))
    check ("window bytes "++nameOf window) (B.unpack bytes == windowFor window)
  enabledBytes <- B.readFile (base </> "happy" </> "probes" </> "gate-readback-enabled.bin")
  restoredBytes <- B.readFile (base </> "happy" </> "probes" </> "gate-readback-restored.bin")
  check "enable readback capture" (B.unpack enabledBytes == readbackPayload 1)
  check "restore readback capture" (B.unpack restoredBytes == readbackPayload 0)

  -- The documented bounds.
  check "aggregate deadline bound" (area2AggregateDeadlineMs == 1200000)
  check "journal bound" (area2JournalCapBytes == 221184)

  -- Stop conditions, each against the exact reply byte that must cause it and
  -- the exact number of window requests that may precede the refusal.
  let firstWindow = specWindows !! 0
      zeros128 = replicate 128 0
      stopCases =
        [ ("gate enable is not visible", "gate enable is not visible", 0, stepsFor (mutate (GateReadReq 1) (P.Reply (configReadReply 0))))
        , ("readback refused", "rejected arguments", 0, stepsFor (mutate (GateReadReq 1) (P.Reply (configReplyWith 9 3 2 0 256 (replicate 256 0)))))
        , ("closed gate", "closed gate", 1, stepsFor (mutate (HalfReq firstWindow) (P.Reply (halfReplyWith 1 0xffff firstWindow zeros128))))
        , ("nonzero reserved word", "reply reserved word is nonzero", 1, stepsFor (mutate (HalfReq firstWindow) (P.Reply reservedHalfReply)))
        , ("wrong opcode", "reply opcode differs", 1, stepsFor (mutate (HalfReq firstWindow) (P.Reply ([0x02,0x00,0x20,0x00] ++ drop 4 (halfReply firstWindow zeros128)))))
        , ("unproducible status", "status the handler cannot produce", 1, stepsFor (mutate (HalfReq firstWindow) (P.Reply (halfReplyWith 2 7 firstWindow zeros128))))
        , ("echoed address mismatch", "differs from the request", 1, stepsFor (mutate (HalfReq firstWindow) (P.Reply (halfReply (firstWindow+4) zeros128)))) ]
  forM_ (zip [(0::Int)..] stopCases) $ \(index,(label,fragment,expected,steps)) ->
    expectStop (base </> ("stop-"++show index)) (label,fragment) expected steps

  -- The restore readback must show the restored value before C4; it is checked
  -- only after every window has run.
  expectStop (base </> "stop-restore") ("restore not visible", "gate restore is not visible") 54
    (stepsFor (mutate (GateReadReq 0) (P.Reply (configReadReply 1))))
  restoreFiles <- listDirectory (base </> "stop-restore" </> "probes")
  check "restore not visible keeps every window and both readbacks" (length restoreFiles == 29)

  -- An expired aggregate deadline stops the session before any transfer.
  expiredClock <- expiringClock
  (expired,expiredPackets,_) <- runCaseWith expiredClock (base </> "deadline-expired") happySteps
  check "an expired aggregate deadline stops the session"
    (either (const True) (const False) expired)
  check "an expired aggregate deadline dispatches nothing" (null expiredPackets)
  expiredJournal <- readFile (base </> "deadline-expired" </> "journal.v1")
  check "the expired deadline is durable" ("session-uncertain" `isInfixOf` expiredJournal)

  removeDirectoryRecursive base
  putStrLn "probe-area2: window-list, frame-byte, gate-order, capture, stop and bound checks passed"
