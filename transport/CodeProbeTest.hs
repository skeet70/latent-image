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
import FujiAF.CodeProbe (runProbeCode, codeAggregateDeadlineMs, codeJournalCapBytes)
import qualified FujiAF.Broker as P

check :: String -> Bool -> IO ()
check label yes = unless yes (error label)

syntheticDescriptors :: Descriptors
syntheticDescriptors = Descriptors 1 0 0 255 0 0 0x81 2 512 512 0 1 []

sessionSid :: Int
sessionSid = 0x117c

-- The seven window bases in the order the operation must read them: the four
-- windows of the G3 closure path plus the three that close the parity
-- builder's, validator's and memset's tails.
specWindows :: [Int]
specWindows = [0x006a8300, 0x006a8400, 0x006a8500, 0x006a8600, 0x00722100, 0x00722200, 0x00722300]

-- The four windows the G3 residual named, before the gapped tails were added.
namedWindows :: [Int]
namedWindows = [0x006a8400, 0x006a8500, 0x00722100, 0x00722300]

addedWindows :: [Int]
addedWindows = [0x006a8300, 0x006a8600, 0x00722200]

-- The five executed function spans from the stock case manifest, written out
-- here rather than taken from the operation: every byte of them must lie in a
-- window.
executedSpans :: [(String,Int,Int)]
executedSpans =
  [ ("parity builder 0x006a83f4", 0x006a83f4, 0x90)
  , ("validator 0x006a84d4", 0x006a84d4, 0x188)
  , ("word-fill 0x007221a0", 0x007221a0, 0x28)
  , ("memset 0x007221c8", 0x007221c8, 0x50)
  , ("memcmp 0x0072232c", 0x0072232c, 0xb8) ]

covered :: [Int] -> Int -> Bool
covered windows address = any (\base -> address >= base && address < base + 256) windows

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
  [ [0x01,0x00,0x20,0x00,0,0,0,0,0,0,0,0,0,0,0,0,0x02,0x00,0x00,0x00,0,0,0,0,0,0,0,0,0x00,0x83,0x6a,0x00,0x80,0x00,0x00,0x00]
  , [0x01,0x00,0x20,0x00,0,0,0,0,0,0,0,0,0,0,0,0,0x02,0x00,0x00,0x00,0,0,0,0,0,0,0,0,0x80,0x83,0x6a,0x00,0x80,0x00,0x00,0x00] ]
lastWindowFrames =
  [ [0x01,0x00,0x20,0x00,0,0,0,0,0,0,0,0,0,0,0,0,0x02,0x00,0x00,0x00,0,0,0,0,0,0,0,0,0x00,0x23,0x72,0x00,0x80,0x00,0x00,0x00]
  , [0x01,0x00,0x20,0x00,0,0,0,0,0,0,0,0,0,0,0,0,0x02,0x00,0x00,0x00,0,0,0,0,0,0,0,0,0x80,0x23,0x72,0x00,0x80,0x00,0x00,0x00] ]

-- Synthetic window content: byte i of a window is (base+i) mod 251, so the
-- two halves of a window differ and a swapped half composition fails the
-- capture assertion.
windowFor :: Int -> [Word8]
windowFor base = [fromIntegral ((base+i) `mod` 251) | i <- [0..255]]

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
  outcome <- try (runProbeCode clock api dir)
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
  check (label++": refusal or uncertainty is durable") (("code-probe-refused" `isInfixOf` journal) || ("session-uncertain" `isInfixOf` journal))

main :: IO ()
main = do
  pid <- getProcessID
  tmp <- getTemporaryDirectory
  let base = tmp </> ("fuji-af-code-probe-tests-"++show pid)
  createDirectory base

  (happy,packets,cleanupEvents) <- runCase (base </> "happy") happySteps
  check ("happy session completes 7 windows: "++show happy) (either (const False) (==7) happy)
  check "happy session releases every resource once" (cleanupEvents == [Released,Closed,Exited])

  let bulkFrames = [bytes | P.BulkOut _ bytes <- packets]
      debugFrames = [frame | frame <- bulkFrames, take 4 frame == [0x01,0x00,0x20,0x00]]
      observed = map (word32At 0x1c) debugFrames

  -- The window list and order, over the bytes the camera receives.
  check "the emitted halves are the 7 windows at +0x00 and +0x80 in order"
    (observed == concatMap (\window -> [window, window + 0x80]) specWindows)

  -- The window list is 256-aligned and strictly increasing, and it is exactly
  -- the four named windows plus the three that close the gapped tails.
  check "the windows are 256-aligned and strictly increasing"
    (all (\w -> w `mod` 256 == 0) specWindows && and (zipWith (<) specWindows (drop 1 specWindows)))
  check "the seven windows are the four named windows plus the three gap-closers"
    (specWindows == sort (namedWindows ++ addedWindows))

  -- Coverage: every byte of the five executed spans lies in a window, the
  -- four originally named windows leave the three tails unread, and every
  -- window carries at least one function byte.
  forM_ executedSpans $ \(label,base',size) ->
    check ("every byte of the "++label++" span lies in a window")
      (all (covered specWindows) [base' .. base'+size-1])
  check "the named windows leave the parity builder head 0x006a83f4-0x006a83ff unread"
    (not (covered namedWindows 0x006a83f4) && not (covered namedWindows 0x006a83ff)
      && covered namedWindows 0x006a8400)
  check "the named windows leave the validator tail 0x006a8600-0x006a865b unread"
    (not (covered namedWindows 0x006a8600) && not (covered namedWindows 0x006a865b)
      && covered namedWindows 0x006a85ff)
  check "the named windows leave the memset tail 0x00722200-0x00722217 unread"
    (not (covered namedWindows 0x00722200) && not (covered namedWindows 0x00722217)
      && covered namedWindows 0x007221ff)
  forM_ specWindows $ \window ->
    check ("window "++nameOf window++" carries at least one function byte")
      (any (\(_,w,size) -> window < w+size && w < window+256) executedSpans)

  -- The audited request frames of the first and last window.
  check "the first window's two request frames are the audited bytes"
    (take 2 debugFrames == firstWindowFrames)
  check "the last window's two request frames are the audited bytes"
    (drop (length debugFrames-2) debugFrames == lastWindowFrames)
  check "every window frame carries function 2 and length 128"
    (map (word32At 0x10) debugFrames == replicate (2*length specWindows) 2
      && map (word32At 0x20) debugFrames == replicate (2*length specWindows) 128)

  -- The gate enable and its readback precede every window request; the
  -- restore and its readback follow every one and precede the close.
  check "the gate enable and its readback precede every window request"
    (take 2 bulkFrames == [gateWriteFrame 1, readbackFrame])
  check "the restore and its readback follow every window request"
    (drop (length debugFrames+2) bulkFrames == [gateWriteFrame 0, readbackFrame])
  check "the session emits only the two gate writes, two readbacks and 14 window requests"
    (length bulkFrames == 18 && length debugFrames == 14)
  check "the close is the final pair of the session"
    (drop (length packets-2) packets == [P.controlTransfer (P.control 0xc4 2 sessionSid), P.BulkIn 0x81 2])

  -- Config writes carry function 3, mailbox 1 and index 0xf7, the readbacks
  -- index 0, and no frame carries index 0x80.
  let writeFrames = [frame | frame <- bulkFrames, word32At 0 frame == 0x1a0002 && word32At 0x10 frame == 3]
      readbackFrames = [frame | frame <- bulkFrames, word32At 0 frame == 0x1a0002 && word32At 0x10 frame == 2]
  check "config writes carry function 3, mailbox 1 and index 0xf7"
    (map (word32At 0x14) writeFrames == replicate 2 1
      && all (\frame -> 256*word32At 0x18 frame + word32At 0x1c frame == 0xf7) writeFrames)
  check "the emitted gate values are 1 then 0"
    ([last frame | frame <- writeFrames] == [fromIntegral (1::Int), fromIntegral (0::Int)])
  check "the config readbacks read index 0"
    (length readbackFrames == 2
      && all (\frame -> 256*word32At 0x18 frame + word32At 0x1c frame == 0) readbackFrames)
  check "no config frame carries index 0x80"
    (all (\frame -> 256*word32At 0x18 frame + word32At 0x1c frame /= 0x80) (writeFrames++readbackFrames))

  -- Output layout and naming match probe-ram so the analysis reads the
  -- captures the same way, and the captured bytes are the served bytes.
  captures <- listDirectory (base </> "happy" </> "windows")
  check "the captures are the 7 window files and the two gate readbacks"
    (sort captures == sort (map (\window -> nameOf window++".bin") specWindows
      ++ ["gate-readback-enabled.bin","gate-readback-restored.bin"]))
  forM_ specWindows $ \window -> do
    bytes <- B.readFile (base </> "happy" </> "windows" </> (nameOf window++".bin"))
    check ("window bytes "++nameOf window) (B.unpack bytes == windowFor window)
  enabledBytes <- B.readFile (base </> "happy" </> "windows" </> "gate-readback-enabled.bin")
  restoredBytes <- B.readFile (base </> "happy" </> "windows" </> "gate-readback-restored.bin")
  check "enable readback capture" (B.unpack enabledBytes == readbackPayload 1)
  check "restore readback capture" (B.unpack restoredBytes == readbackPayload 0)

  happyJournal <- readFile (base </> "happy" </> "journal.v1")
  check "one completion record per window and one session record"
    (length (filter ("window-complete" `isInfixOf`) (lines happyJournal)) == length specWindows
      && length (filter ("code-probe-complete-stock-unverified" `isInfixOf`) (lines happyJournal)) == 1)
  outgoingLog <- readFile (base </> "happy" </> "outgoing.v1")
  check "each of the fourteen window frames is durable before dispatch"
    (all (\address -> P.hexBytes (halfFrame address) `isInfixOf` outgoingLog)
      (concatMap (\window -> [window, window + 0x80]) specWindows))

  -- The documented bounds, derived from the seven-window shape.
  check "aggregate deadline bound" (codeAggregateDeadlineMs == 400000)
  check "journal bound" (codeJournalCapBytes == 57344)

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
  expectStop (base </> "stop-restore") ("restore not visible", "gate restore is not visible") 14
    (stepsFor (mutate (GateReadReq 0) (P.Reply (configReadReply 1))))
  restoreFiles <- listDirectory (base </> "stop-restore" </> "windows")
  check "restore not visible keeps every window and both readbacks" (length restoreFiles == 9)

  -- A lost reply mid-session still attempts the one close and leaves the
  -- uncertainty record.
  (lostOutcome,lostPackets,lostCleanup) <- runCase (base </> "lost-reply")
    (stepsFor (mutate (HalfReq (specWindows !! 3)) P.LostReply))
  check "lost read reply stops the session"
    (either (isInfixOf "injected transport failure" . displayException) (const False) lostOutcome)
  check "lost read reply still attempts one close and releases everything"
    (length [() | P.ControlWrite _ _ _ _ (0xc4:_) <- lostPackets] == 1 && lostCleanup == [Released,Closed,Exited])

  -- An expired aggregate deadline stops the session before any transfer.
  expiredClock <- expiringClock
  (expired,expiredPackets,_) <- runCaseWith expiredClock (base </> "deadline-expired") happySteps
  check "an expired aggregate deadline stops the session"
    (either (const True) (const False) expired)
  check "an expired aggregate deadline dispatches nothing" (null expiredPackets)
  expiredJournal <- readFile (base </> "deadline-expired" </> "journal.v1")
  check "the expired deadline is durable" ("session-uncertain" `isInfixOf` expiredJournal)

  removeDirectoryRecursive base
  putStrLn "probe-code: window-list, coverage, frame-byte, gate-order, capture, stop and bound checks passed"
