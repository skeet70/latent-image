module Main where

import Control.Exception
import Control.Monad (forM_, unless)
import Data.List (isInfixOf)
import qualified Data.ByteString as B
import Data.IORef
import Data.Word (Word8)
import System.Directory
import System.FilePath
import System.Posix.Process (getProcessID)
import FujiAF.Transport
import FujiAF.Read
import FujiAF.Probe
import qualified FujiAF.Broker as P

check :: String -> Bool -> IO ()
check label yes = unless yes (error label)

syntheticDescriptors :: Descriptors
syntheticDescriptors = Descriptors 1 0 0 255 0 0 0x81 2 512 512 0 1 []

sessionSid :: Int
sessionSid = 0x117c

openSteps :: [P.Step]
openSteps =
  [ P.Step (P.controlTransfer (P.control 0xc0 8 0)) (P.Transferred 12)
  , P.Step (P.BulkIn 0x81 8) (P.Reply [0x7c,0x11,0,0,0,0,1,0]) ]

probeSteps :: (ProbeRequest -> P.Outcome) -> [P.Step]
probeSteps outcomeFor = openSteps ++ concatMap step probeRequests
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
        , P.Step (P.BulkOut 2 (readRequestFrame readbackWindow)) (P.Transferred readRequestBytes)
        , P.Step (P.controlTransfer (P.control 0xd1 readReplyBytes sessionSid)) (P.Transferred 12)
        , P.Step (P.BulkIn 0x81 readReplyBytes) (outcomeFor action) ]
      Half address ->
        [ P.Step (P.controlTransfer (P.control 0xd0 debugRequestBytes sessionSid)) (P.Transferred 12)
        , P.Step (P.BulkOut 2 (debugRequestFrame address)) (P.Transferred debugRequestBytes)
        , P.Step (P.controlTransfer (P.control 0xd1 debugReplyForHalf sessionSid)) (P.Transferred 12)
        , P.Step (P.BulkIn 0x81 debugReplyForHalf) (outcomeFor action) ]

-- Reply builders. The config reply layout is the read path's; the debug reply
-- carries function 2 echoes at +0x1c/+0x20 and data from +0x24.
configReplyWith :: Int -> Int -> Int -> Int -> Int -> [Word8] -> [Word8]
configReplyWith err1 err2 function offset size payload = concat
  [ word32le readOpcode, word32le err1, word32le err2, word32le 0
  , word32le function, word32le 1, word32le 0, word32le offset, word32le size ]
  ++ payload

configWriteReply :: [Word8]
configWriteReply = configReplyWith 0 0 3 gateIndex configWriteLength (replicate 256 0)

configReadReply :: [Word8] -> [Word8]
configReadReply = configReplyWith 0 0 2 0 256

debugReplyWith :: Int -> Int -> Int -> Int -> Int -> [Word8] -> [Word8]
debugReplyWith opcode err1 err2 address size payload = concat
  [ word32le opcode, word32le err1, word32le err2, word32le 0
  , word32le 2, word32le 0, word32le 0
  , word32le address, word32le size ]
  ++ payload

debugReply :: Int -> [Word8] -> [Word8]
debugReply address = debugReplyWith debugOpcode 0 0 address probeHalfBytes

readbackPayload :: Word8 -> [Word8]
readbackPayload value = take gateIndex (replicate 256 0x33) ++ [value] ++ drop (gateIndex+1) (replicate 256 0x33)

probeWindowFor :: [Word8] -> Int -> [Word8]
probeWindowFor readback start
  | start == cacheProductProbe = cacheBytes
  | start == cacheReadbackProbe = readback
  | start == cacheSignatureProbe = take signatureByteOffset cacheBytes ++ [0x55,0xaa] ++ drop (signatureByteOffset+2) cacheBytes
  | otherwise = replicate probeWindowBytes (fromIntegral (start `mod` 251))
  where cacheBytes = take productByteOffset filler ++ productOracleBytes ++ drop (productByteOffset + length productOracleBytes) filler
        filler = replicate probeWindowBytes 0x11

-- The claimed enable readback is what the +0x000 probe window must equal.
happyOutcome :: ProbeRequest -> P.Outcome
happyOutcome action = case action of
  GateWrite _ -> P.Reply configWriteReply
  GateRead label
    | label == gateReadbackEnabled -> P.Reply (configReadReply (readbackPayload 1))
    | otherwise -> P.Reply (configReadReply (readbackPayload 0))
  Half address ->
    let start = if address `elem` map probeAddress probeList then address else address - probeHalfBytes
        window = probeWindowFor (readbackPayload 1) start
    in P.Reply (debugReply address (if address == start then take probeHalfBytes window else drop probeHalfBytes window))
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
  outcome <- try (runProbe (Clock (pure 0)) api dir)
  packets <- readIORef emitted
  cleanupEvents <- readIORef events
  pure (outcome,packets,cleanupEvents)

isProbeBulkOut :: P.Transfer -> Bool
isProbeBulkOut (P.BulkOut _ frame) = word32At 0 frame == debugOpcode
isProbeBulkOut _ = False

-- A stop is checked for its message, the exact number of probe requests the
-- session may have emitted before refusing, and that no request follows it.
expectStop :: FilePath -> (String,String) -> Int -> [P.Step] -> IO ()
expectStop dir (label,fragment) expectedProbeRequests steps = do
  (outcome,packets,cleanupEvents) <- runCase dir steps
  let emittedProbeRequests = length (filter isProbeBulkOut packets)
  check (label++": session must stop") (either (const True) (const False) outcome)
  check (label++": stop message") (either (isInfixOf fragment . displayException) (const False) outcome)
  check (label++": "++show emittedProbeRequests++" probe requests, expected "++show expectedProbeRequests)
    (emittedProbeRequests == expectedProbeRequests)
  check (label++": exactly one protocol close") (length [() | P.ControlWrite _ _ _ _ (0xc4:_) <- packets] == 1)
  check (label++": every host resource released once") (cleanupEvents == [Released,Closed,Exited])
  journal <- readFile (dir </> "journal.v1")
  check (label++": refusal or uncertainty is durable") (("probe-refused" `isInfixOf` journal) || ("session-uncertain" `isInfixOf` journal))

main :: IO ()
main = do
  pid <- getProcessID
  tmp <- getTemporaryDirectory
  let base = tmp </> ("fuji-af-probe-tests-"++show pid)
  createDirectory base

  let happySteps = probeSteps happyOutcome
  (happy,packets,cleanupEvents) <- runCase (base </> "happy") happySteps
  check ("happy session completes "++show probeCount++" probes: "++show happy) (either (const False) (==probeCount) happy)
  check "happy session releases every resource once" (cleanupEvents == [Released,Closed,Exited])
  forM_ probeList $ \probe -> do
    let label = probeName (probeAddress probe)
    bytes <- B.readFile (base </> "happy" </> "probes" </> (label++".bin"))
    check ("probe window bytes "++label) (B.unpack bytes == probeWindowFor (readbackPayload 1) (probeAddress probe))
  enabled <- B.readFile (base </> "happy" </> "probes" </> (gateReadbackEnabled++".bin"))
  restored <- B.readFile (base </> "happy" </> "probes" </> (gateReadbackRestored++".bin"))
  check "enable readback window" (B.unpack enabled == readbackPayload 1)
  check "restore readback window" (B.unpack restored == readbackPayload 0)
  happyJournal <- readFile (base </> "happy" </> "journal.v1")
  check "one completion record per probe and one session record"
    (length (filter ("window-complete" `isInfixOf`) (lines happyJournal)) == probeCount
      && length (filter ("probe-complete-stock-unverified" `isInfixOf`) (lines happyJournal)) == 1)
  check "gate enable and restore are journaled with their values"
    (("index=247;value=1" `isInfixOf` happyJournal) && ("index=247;value=0" `isInfixOf` happyJournal)
      && ("gate-restored" `isInfixOf` happyJournal))
  outgoingLog <- readFile (base </> "happy" </> "outgoing.v1")
  check "the enable frame is durable before dispatch" (P.hexBytes (configWriteFrame gateEnabled) `isInfixOf` outgoingLog)
  check "the restore frame is durable before dispatch" (P.hexBytes (configWriteFrame gateDisabled) `isInfixOf` outgoingLog)

  -- Every byte the operation emits, audited over the session's own packets.
  let controlBytes = [bytes | P.ControlWrite _ _ _ _ bytes <- packets]
      bulkFrames = [bytes | P.BulkOut _ bytes <- packets]
      writeFrames = [frame | frame <- bulkFrames, word32At 0 frame == readOpcode && word32At 0x10 frame == 3]
      readbackFrames = [frame | frame <- bulkFrames, word32At 0 frame == readOpcode && word32At 0x10 frame == 2]
      debugFrames = [frame | frame <- bulkFrames, word32At 0 frame == debugOpcode]
  check "every emitted control frame is twelve bytes"
    (all (\bytes -> length bytes == 12) controlBytes)
  check "every emitted bulk frame has its declared length"
    (map length writeFrames == replicate 2 configWriteBytes
      && map length readbackFrames == replicate 2 readRequestBytes
      && map length debugFrames == replicate (probeCount*2) debugRequestBytes)
  check "control writes use only the audited session opcodes"
    (all (\bytes -> take 1 bytes `elem` [[0xc0],[0xd0],[0xd1],[0xc4]]) controlBytes)
  check "one D1 control per request with the request's reply length"
    ([bytes | P.ControlWrite _ _ _ _ bytes@(0xd1:_) <- packets]
      == [P.control 0xd1 configReplyBytes sessionSid, P.control 0xd1 readReplyBytes sessionSid]
      ++ replicate (probeCount*2) (P.control 0xd1 debugReplyForHalf sessionSid)
      ++ [P.control 0xd1 configReplyBytes sessionSid, P.control 0xd1 readReplyBytes sessionSid])
  check "config writes carry function 3, mailbox 1 and index 0xf7, never index 0x80"
    (map (word32At 0x10) writeFrames == replicate 2 3
      && map (word32At 0x14) writeFrames == replicate 2 1
      && map (word32At 0x1c) writeFrames == replicate 2 gateIndex
      && all ((/= 0x80) . word32At 0x1c) writeFrames
      && map (word32At 0x20) writeFrames == replicate 2 configWriteLength)
  check "the emitted gate values are 1 then 0"
    ([last frame | frame <- writeFrames] == [fromIntegral gateEnabled, fromIntegral gateDisabled])
  check "no config read leaves index 0 and every one requests the full window"
    (all ((== 0) . word32At 0x1c) readbackFrames && all ((== 256) . word32At 0x20) readbackFrames)
  check "every debug frame carries function 2, the probe address and a 128-byte length"
    (map (word32At 0x10) debugFrames == replicate (probeCount*2) 2
      && map (word32At 0x20) debugFrames == replicate (probeCount*2) probeHalfBytes
      && map (word32At 0x1c) debugFrames == concatMap (\p -> [probeAddress p, probeAddress p + probeHalfBytes]) probeList)
  check "no emitted frame carries any other opcode"
    (all (\frame -> word32At 0 frame `elem` [readOpcode, debugOpcode]) bulkFrames)

  -- Stop conditions, each against the exact byte that must cause it and the
  -- exact number of probe requests that may precede the refusal. Per-half
  -- failures stop after one request; window oracles are checked after both.
  let productProbe = probeAddress (probeList !! 0)
      stopCases =
        [ ("gate enable is not visible", "gate enable is not visible", 0, probeSteps (mutate (GateRead gateReadbackEnabled) (const (P.Reply (configReadReply (readbackPayload 0))))))
        , ("readback refused", "rejected arguments", 0, probeSteps (mutate (GateRead gateReadbackEnabled) (const (P.Reply (configReplyWith 9 3 2 0 256 (readbackPayload 1))))))
        , ("closed gate", "closed gate", 1, probeSteps (mutate (Half productProbe) (const (P.Reply (debugReplyWith debugOpcode 1 0xffff 0 0 (replicate probeHalfBytes 0))))))
        , ("product oracle mismatch", "product oracle mismatch", 2, probeSteps (mutate (Half productProbe) (const (P.Reply (debugReply productProbe (replicate probeHalfBytes 0))))))
        , ("cache window differs", "cache window differs", 2*2, probeSteps (mutate (Half cacheReadbackProbe) (const (P.Reply (debugReply cacheReadbackProbe (replicate probeHalfBytes 0))))))
        , ("signature oracle mismatch", "signature oracle mismatch", 2*3, probeSteps (mutate (Half cacheSignatureProbe) (const (P.Reply (debugReply cacheSignatureProbe (replicate probeHalfBytes 0))))))
        , ("unproducible status", "status the handler cannot produce", 1, probeSteps (mutate (Half productProbe) (const (P.Reply (debugReplyWith debugOpcode 2 7 0 0 (replicate probeHalfBytes 0))))))
        , ("nonzero reserved word", "reply reserved word is nonzero", 1, probeSteps (mutate (Half productProbe) (const (P.Reply (word32le debugOpcode ++ drop 4 reservedDebugReply)))))
        , ("wrong opcode", "reply opcode differs", 1, probeSteps (mutate (Half productProbe) (const (P.Reply (word32le 0x00200002 ++ drop 4 (debugReply productProbe (replicate probeHalfBytes 0)))))))
        -- A short reply stops in the transfer layer with this message; the
        -- decoder's own length check is covered below.
        , ("short debug reply", "short/partial/oversized transfer; no retry", 1, probeSteps (mutate (Half productProbe) (const (P.Reply (take (debugReplyForHalf-1) (debugReply productProbe (replicate probeHalfBytes 0)))))))
        , ("echoed address mismatch", "differs from the request", 1, probeSteps (mutate (Half productProbe) (const (P.Reply (debugReply (productProbe+4) (replicate probeHalfBytes 0)))))) ]
  forM_ (zip [(0::Int)..] stopCases) $ \(index,(label,fragment,expectedProbes,steps)) ->
    expectStop (base </> ("stop-"++show index)) (label,fragment) expectedProbes steps

  -- The restore readback is only checked after every probe has run.
  (restoreOutcome,restorePackets,restoreCleanup) <- runCase (base </> "restore-not-visible")
    (probeSteps (mutate (GateRead gateReadbackRestored) (const (P.Reply (configReadReply (readbackPayload 1))))))
  check "restore not visible stops the session"
    (either (isInfixOf "gate restore is not visible" . displayException) (const False) restoreOutcome)
  check "restore not visible keeps all probe captures and attempts one close"
    (length [() | P.BulkOut{} <- restorePackets] == 2 + 2 + probeCount*2
      && length [() | P.ControlWrite _ _ _ _ (0xc4:_) <- restorePackets] == 1
      && restoreCleanup == [Released,Closed,Exited])
  restoreProbeFiles <- listDirectory (base </> "restore-not-visible" </> "probes")
  check "every probe window and readback is captured before the stop" (length restoreProbeFiles == probeCount+2)

  -- A transport failure mid-session leaves the uncertainty record.
  (lostOutcome,lostPackets,lostCleanup) <- runCase (base </> "lost-reply")
    (probeSteps (mutate (Half 0x0061c000) (const P.LostReply)))
  check "lost probe reply stops the session"
    (either (isInfixOf "injected transport failure" . displayException) (const False) lostOutcome)
  check "lost probe reply still attempts one close and releases everything"
    (length [() | P.ControlWrite _ _ _ _ (0xc4:_) <- lostPackets] == 1 && lostCleanup == [Released,Closed,Exited])
  lostJournal <- readFile (base </> "lost-reply" </> "journal.v1")
  check "lost probe reply journals the uncertainty" ("session-uncertain" `isInfixOf` lostJournal)

  -- The C0 reply decides the session id; a wrong length stops before any D0.
  (shortOpen,shortOpenPackets,shortOpenCleanup) <- runCase (base </> "short-open")
    (take 1 happySteps ++ [P.Step (P.BulkIn 0x81 8) (P.Reply (replicate 7 0))] ++ drop 2 happySteps)
  check "short C0 reply stops before D0 and cannot invent a close"
    (either (const True) (const False) shortOpen && shortOpenPackets == [P.controlTransfer (P.control 0xc0 8 0),P.BulkIn 0x81 8])
  check "short C0 reply releases every resource once" (shortOpenCleanup == [Released,Closed,Exited])

  -- Decoding is exact, checked apart from the session.
  check "exact debug decode" (decodeDebugReply 0x0061c400 (debugReply 0x0061c400 (replicate probeHalfBytes 7)) == Right (replicate probeHalfBytes 7))
  forM_ [ debugReplyWith debugOpcode 1 0xffff 0 0 (replicate probeHalfBytes 0)
        , debugReplyWith debugOpcode 9 3 0 0 (replicate probeHalfBytes 0)
        , debugReplyWith debugOpcode 2 7 0 0 (replicate probeHalfBytes 0)
        , word32le debugOpcode ++ drop 4 reservedDebugReply
        , take (debugReplyForHalf-1) (debugReply 0x0061c400 (replicate probeHalfBytes 7))
        , debugReply 0x0061c401 (replicate probeHalfBytes 7)
        , debugReply 0x0061c400 (replicate (probeHalfBytes-1) 7) ] $ \bytes ->
    check "debug reply refusal" (either (const True) (const False) (decodeDebugReply 0x0061c400 bytes))
  check "exact write decode" (decodeConfigWriteReply configWriteReply == Right ())
  forM_ [ configReplyWith 1 0xffff 3 gateIndex 1 (replicate 256 0)
        , configReplyWith 9 3 3 gateIndex 1 (replicate 256 0)
        , configReplyWith 0 1 3 gateIndex 1 (replicate 256 0)
        , configReadReply (readbackPayload 1)
        , configReplyWith 0 0 3 (gateIndex+1) 1 (replicate 256 0) ] $ \bytes ->
    check "write reply refusal" (either (const True) (const False) (decodeConfigWriteReply bytes))

  removeDirectoryRecursive base
  putStrLn "probe-ram: frame-byte, gate, oracle, readback, stop and decoding checks passed"

-- A debug reply whose reserved word at +0x0c is nonzero.
reservedDebugReply :: [Word8]
reservedDebugReply = take 12 (debugReply 0 (replicate probeHalfBytes 0)) ++ word32le 1 ++ drop 16 (debugReply 0 (replicate probeHalfBytes 0))
