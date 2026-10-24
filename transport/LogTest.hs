module Main where

import Control.Exception
import Control.Monad (forM_, unless)
import Data.List (isInfixOf)
import qualified Data.ByteString as B
import Data.IORef
import Data.Word (Word8)
import System.Directory
import System.FilePath
import System.Posix.Temp (mkdtemp)
import FujiAF.Transport
import FujiAF.Read
import FujiAF.Probe
import FujiAF.Log
import qualified FujiAF.Broker as P

check :: String -> Bool -> IO ()
check label yes = unless yes (error label)

syntheticDescriptors :: Descriptors
syntheticDescriptors = Descriptors 1 0 0 255 0 0 0x81 2 512 512 0 1 []

sessionSid :: Int
sessionSid = 0x117c

logSteps :: (ProbeRequest -> P.Outcome) -> [P.Step]
logSteps outcomeFor = openSteps ++ concatMap step logRequests
  where
    step action = case action of
      Close -> [ P.Step (P.controlTransfer (P.control 0xc4 2 sessionSid)) (P.Transferred 12)
               , P.Step (P.BulkIn 0x81 2) (outcomeFor action) ]
      GateWrite value ->
        [ P.Step (P.controlTransfer (P.control 0xd0 configWriteBytes sessionSid)) (P.Transferred 12)
        , P.Step (P.BulkOut 2 (configWriteFrame value)) (P.Transferred configWriteBytes)
        , P.Step (P.controlTransfer (P.control 0xd1 configReplyBytes sessionSid)) (P.Transferred 12)
        , P.Step (P.BulkIn 0x81 configReplyBytes) (outcomeFor action) ]
      GateRead _ ->
        [ P.Step (P.controlTransfer (P.control 0xd0 readRequestBytes sessionSid)) (P.Transferred 12)
        , P.Step (P.BulkOut 2 (readRequestFrame (ReadWindow 0 0 "readback"))) (P.Transferred readRequestBytes)
        , P.Step (P.controlTransfer (P.control 0xd1 readReplyBytes sessionSid)) (P.Transferred 12)
        , P.Step (P.BulkIn 0x81 readReplyBytes) (outcomeFor action) ]
      Half address ->
        [ P.Step (P.controlTransfer (P.control 0xd0 debugRequestBytes sessionSid)) (P.Transferred 12)
        , P.Step (P.BulkOut 2 (debugRequestFrame address)) (P.Transferred debugRequestBytes)
        , P.Step (P.controlTransfer (P.control 0xd1 debugReplyForHalf sessionSid)) (P.Transferred 12)
        , P.Step (P.BulkIn 0x81 debugReplyForHalf) (outcomeFor action) ]

openSteps :: [P.Step]
openSteps =
  [ P.Step (P.controlTransfer (P.control 0xc0 8 0)) (P.Transferred 12)
  , P.Step (P.BulkIn 0x81 8) (P.Reply [0x7c,0x11,0,0,0,0,1,0]) ]

configWriteReply :: [Word8]
configWriteReply = concat
  [ word32le readOpcode, word32le 0, word32le 0, word32le 0
  , word32le 3, word32le 1
  , word32le 0, word32le gateIndex, word32le 1 ]
  ++ replicate 256 0

readbackPayload :: Word8 -> [Word8]
readbackPayload value = take gateIndex (replicate 256 0x5a) ++ [value] ++ drop (gateIndex+1) (replicate 256 0x5a)

configReadReply :: Word8 -> [Word8]
configReadReply value = concat
  [ word32le readOpcode, word32le 0, word32le 0, word32le 0
  , word32le 2, word32le 1
  , word32le 0, word32le 0, word32le 256 ]
  ++ readbackPayload value

debugReply :: Int -> [Word8] -> [Word8]
debugReply address payload = concat
  [ word32le debugOpcode, word32le 0, word32le 0, word32le 0
  , word32le 2, word32le 0, word32le 0
  , word32le address, word32le probeHalfBytes ]
  ++ payload

-- Synthetic log content: 0x10-byte entries with a tick word, so the window
-- bytes are deterministic per address.
windowFor :: Int -> [Word8]
windowFor base = concatMap entry [0 .. probeHalfBytes*2 `div` 0x10 - 1]
  where entry index = [0x72,0x11, fromIntegral (index `mod` 0x100), 0]
                    ++ word32le (base + index*0x10)
                    ++ replicate 8 0

happyOutcome :: ProbeRequest -> P.Outcome
happyOutcome action = case action of
  GateWrite _ -> P.Reply configWriteReply
  GateRead label -> P.Reply (configReadReply (if label == gateReadbackEnabled then 1 else 0))
  Half address ->
    let base = if address `elem` map snd logWindows then address else address - probeHalfBytes
        window = windowFor base
    in P.Reply (debugReply address (if address == base then take probeHalfBytes window else drop probeHalfBytes window))
  Close -> P.Reply [0x7c,0x11]

mutate :: ProbeRequest -> (P.Outcome -> P.Outcome) -> ProbeRequest -> P.Outcome
mutate target change action = if action == target then change (happyOutcome target) else happyOutcome action

data CleanupEvent = Released | Closed | Exited deriving (Eq, Show)

runCase :: FilePath -> [P.Step] -> IO (Either SomeException Int,[P.Transfer],[CleanupEvent])
runCase dir steps = do
  createDirectoryIfMissing True dir
  emitted <- newIORef []
  events <- newIORef []
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
  outcome <- try (runReadLog (Clock (pure 0)) api dir)
  packets <- readIORef emitted
  cleanupEvents <- readIORef events
  pure (outcome,packets,cleanupEvents)

isLogBulkOut :: P.Transfer -> Bool
isLogBulkOut (P.BulkOut _ frame) = word32At 0 frame == debugOpcode
isLogBulkOut _ = False

expectStop :: FilePath -> (String,String) -> Int -> [P.Step] -> IO ()
expectStop dir (label,fragment) expectedLogRequests steps = do
  (outcome,packets,cleanupEvents) <- runCase dir steps
  let logRequestCount = length (filter isLogBulkOut packets)
  check (label++": session must stop") (either (const True) (const False) outcome)
  check (label++": stop message") (either (isInfixOf fragment . displayException) (const False) outcome)
  check (label++": "++show logRequestCount++" log requests, expected "++show expectedLogRequests)
    (logRequestCount == expectedLogRequests)
  check (label++": exactly one protocol close") (length [() | P.ControlWrite _ _ _ _ (0xc4:_) <- packets] == 1)
  check (label++": every host resource released once") (cleanupEvents == [Released,Closed,Exited])
  journal <- readFile (dir </> "journal.v1")
  check (label++": refusal or uncertainty is durable") (("log-refused" `isInfixOf` journal) || ("session-uncertain" `isInfixOf` journal))

main :: IO ()
main = do
  -- Every journal schema line is made durable: minutes of fsync on a
  -- disk-backed scratch directory, under a second on tmpfs. Concurrent Bazel
  -- runs share /dev/shm and every sandbox reports pid 12.
  base <- mkdtemp "/dev/shm/fuji-af-log-tests-XXXXXX"

  let happySteps = logSteps happyOutcome
  (happy,packets,cleanupEvents) <- runCase (base </> "happy") happySteps
  check ("happy session completes "++show (length logWindows)++" windows: "++show happy) (either (const False) (==length logWindows) happy)
  check "happy session releases every resource once" (cleanupEvents == [Released,Closed,Exited])
  forM_ logWindows $ \(label, windowBase) -> do
    bytes <- B.readFile (base </> "happy" </> "windows" </> (label++".bin"))
    check ("log window bytes "++label) (B.unpack bytes == windowFor windowBase)
  enabled <- B.readFile (base </> "happy" </> "windows" </> (gateReadbackEnabled++".bin"))
  restored <- B.readFile (base </> "happy" </> "windows" </> (gateReadbackRestored++".bin"))
  check "enable readback window" (B.unpack enabled == readbackPayload 1)
  check "restore readback window" (B.unpack restored == readbackPayload 0)
  happyJournal <- readFile (base </> "happy" </> "journal.v1")
  check "one completion record per window and one session record"
    (length (filter ("window-complete" `isInfixOf`) (lines happyJournal)) == length logWindows
      && length (filter ("log-complete-stock-unverified" `isInfixOf`) (lines happyJournal)) == 1)
  outgoingLog <- readFile (base </> "happy" </> "outgoing.v1")
  check "the enable frame is durable before dispatch" (P.hexBytes (configWriteFrame gateEnabled) `isInfixOf` outgoingLog)
  check "the restore frame is durable before dispatch" (P.hexBytes (configWriteFrame gateDisabled) `isInfixOf` outgoingLog)

  -- Every byte the operation emits, audited over the session's own packets.
  let controlBytes = [bytes | P.ControlWrite _ _ _ _ bytes <- packets]
      bulkFrames = [bytes | P.BulkOut _ bytes <- packets]
      writeFrames = [frame | frame <- bulkFrames, word32At 0 frame == readOpcode && word32At 0x10 frame == 3]
      readbackFrames = [frame | frame <- bulkFrames, word32At 0 frame == readOpcode && word32At 0x10 frame == 2]
      logFrames = [frame | frame <- bulkFrames, word32At 0 frame == debugOpcode]
      logAddresses = map (word32At 0x1c) logFrames
  check "every emitted control frame is twelve bytes"
    (all (\bytes -> length bytes == 12) controlBytes)
  check "every emitted bulk frame has its declared length"
    (map length writeFrames == replicate 2 configWriteBytes
      && map length readbackFrames == replicate 2 readRequestBytes
      && map length logFrames == replicate (length logWindows * 2) debugRequestBytes)
  check "control writes use only the audited session opcodes"
    (all (\bytes -> take 1 bytes `elem` [[0xc0],[0xd0],[0xd1],[0xc4]]) controlBytes)
  check "config writes carry function 3 and mailbox 1"
    (map (word32At 0x10) writeFrames == replicate 2 3
      && map (word32At 0x14) writeFrames == replicate 2 1)
  check "config writes carry index 0xf7 and no config frame carries index 0x80"
    (all (\frame -> 256*word32At 0x18 frame + word32At 0x1c frame == gateIndex) writeFrames
      && all (\frame -> 256*word32At 0x18 frame + word32At 0x1c frame /= 0x80) (writeFrames++readbackFrames))
  check "the config readbacks read index 0"
    (all (\frame -> 256*word32At 0x18 frame + word32At 0x1c frame == 0) readbackFrames)
  check "the emitted gate values are 1 then 0"
    ([last frame | frame <- writeFrames] == [fromIntegral gateEnabled, fromIntegral gateDisabled])
  check "every log frame carries function 2 and a 128-byte length"
    (map (word32At 0x10) logFrames == replicate (length logWindows * 2) 2
      && map (word32At 0x20) logFrames == replicate (length logWindows * 2) probeHalfBytes)
  let tickHalves = [logTickAddress, logTickAddress + probeHalfBytes]
      storeHalves w = w >= logOrigin && w + probeHalfBytes <= logOrigin + logWindowCount*logWindowBytes
      headPairs = take 4 logAddresses
      storePart = take (2*logWindowCount) (drop 4 logAddresses)
      tailPair = drop (4 + 2*logWindowCount) logAddresses
  check "the tick cell is sampled twice in a row first and once last"
    (headPairs == tickHalves ++ tickHalves && tailPair == tickHalves
      && length (filter (`elem` tickHalves) logAddresses) == 6)
  check "every emitted log address is a store half or a tick half"
    (all (\w -> storeHalves w || w `elem` tickHalves) logAddresses)
  check "the emitted store addresses tile the store ascending by one half-window"
    (length storePart == 2*logWindowCount
      && take 1 storePart == [logOrigin]
      && and (zipWith (\a b -> a + probeHalfBytes == b) storePart (drop 1 storePart)))
  check "the read extent covers the store and overshoots it by less than one window"
    (logWindowCount*logWindowBytes >= logStoreBytes
      && logWindowCount*logWindowBytes - logStoreBytes < logWindowBytes)
  check "no emitted frame carries any other opcode"
    (all (\frame -> word32At 0 frame `elem` [readOpcode, debugOpcode]) bulkFrames)

  -- Stop conditions, each against the exact byte that must cause it.
  let firstHalf = snd (logWindows !! 0)
      stopCases =
        [ ("gate enable is not visible", "gate enable is not visible", 0, logSteps (mutate (GateRead gateReadbackEnabled) (const (P.Reply (configReadReply 0)))))
        , ("readback refused", "rejected arguments", 0, logSteps (mutate (GateRead gateReadbackEnabled) (const (P.Reply (configReplyWith 9 3 2 0 256 (replicate 256 0))))))
        , ("closed gate", "closed gate", 1, logSteps (mutate (Half firstHalf) (const (P.Reply (debugReplyWith debugOpcode 1 0xffff 0 0 (replicate probeHalfBytes 0))))))
        , ("nonzero reserved word", "reply reserved word is nonzero", 1, logSteps (mutate (Half firstHalf) (const (P.Reply (take 12 (debugReply firstHalf (replicate probeHalfBytes 0)) ++ word32le 1 ++ drop 16 (debugReply firstHalf (replicate probeHalfBytes 0)))))))
        , ("wrong opcode", "reply opcode differs", 1, logSteps (mutate (Half firstHalf) (const (P.Reply (word32le 0x00200002 ++ drop 4 (debugReply firstHalf (replicate probeHalfBytes 0)))))))
        , ("unproducible status", "status the handler cannot produce", 1, logSteps (mutate (Half firstHalf) (const (P.Reply (debugReplyWith debugOpcode 2 7 0 0 (replicate probeHalfBytes 0))))))
        , ("echoed address mismatch", "differs from the request", 1, logSteps (mutate (Half firstHalf) (const (P.Reply (debugReply (firstHalf+4) (replicate probeHalfBytes 0)))))) ]
  forM_ (zip [(0::Int)..] stopCases) $ \(index,(label,fragment,expected,steps)) ->
    expectStop (base </> ("stop-"++show index)) (label,fragment) expected steps

  -- A transport failure mid-session leaves the uncertainty record.
  (lostOutcome,lostPackets,lostCleanup) <- runCase (base </> "lost-reply")
    (logSteps (mutate (Half (snd (logWindows !! 5))) (const P.LostReply)))
  check "lost log reply stops the session"
    (either (isInfixOf "injected transport failure" . displayException) (const False) lostOutcome)
  check "lost log reply still attempts one close and releases everything"
    (length [() | P.ControlWrite _ _ _ _ (0xc4:_) <- lostPackets] == 1 && lostCleanup == [Released,Closed,Exited])

  removeDirectoryRecursive base
  putStrLn "read-log: frame-byte, gate, stop and decoding checks passed"

-- A config reply with an arbitrary status pair, used for the refusal cases.
configReplyWith :: Int -> Int -> Int -> Int -> Int -> [Word8] -> [Word8]
configReplyWith err1 err2 function offset size payload = concat
  [ word32le readOpcode, word32le err1, word32le err2, word32le 0
  , word32le function, word32le 1, word32le 0, word32le offset, word32le size ]
  ++ payload

debugReplyWith :: Int -> Int -> Int -> Int -> Int -> [Word8] -> [Word8]
debugReplyWith opcode err1 err2 address size payload = concat
  [ word32le opcode, word32le err1, word32le err2, word32le 0
  , word32le 2, word32le 0, word32le 0
  , word32le address, word32le size ]
  ++ payload
