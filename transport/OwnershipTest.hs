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
import FujiAF.Ownership
import FujiAF.SHA256 (sha256)
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

ownershipSteps :: (ProbeRequest -> P.Outcome) -> [P.Step]
ownershipSteps outcomeFor = openSteps ++ concatMap step ownershipRequests
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

configReadReply :: Word8 -> [Word8]
configReadReply value = configReplyWith 0 0 2 0 256 (readbackPayload value)

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

-- Synthetic deterministic window bytes: every byte follows its absolute
-- address, so a swapped or shifted composition cannot match the capture.
windowFor :: Int -> [Word8]
windowFor base = [fromIntegral ((base + offset) `mod` 251) | offset <- [0 .. probeWindowBytes-1]]

happyOutcome :: ProbeRequest -> P.Outcome
happyOutcome action = case action of
  GateWrite _ -> P.Reply configWriteReply
  GateRead label
    | label == gateReadbackEnabled -> P.Reply (configReadReply 1)
    | otherwise -> P.Reply (configReadReply 0)
  Half address ->
    let base = if address `elem` ownershipWindows then address else address - probeHalfBytes
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
  outcome <- try (runProbeOwnership (Clock (pure 0)) api dir)
  packets <- readIORef emitted
  cleanupEvents <- readIORef events
  pure (outcome,packets,cleanupEvents)

isOwnershipBulkOut :: P.Transfer -> Bool
isOwnershipBulkOut (P.BulkOut _ frame) = word32At 0 frame == debugOpcode
isOwnershipBulkOut _ = False

-- A stop is checked for its message, the exact number of window-half requests
-- the session may have emitted before refusing, and that no request follows it.
expectStop :: FilePath -> (String,String) -> Int -> [P.Step] -> IO ()
expectStop dir (label,fragment) expectedHalvesSent steps = do
  (outcome,packets,cleanupEvents) <- runCase dir steps
  let halvesSent = length (filter isOwnershipBulkOut packets)
  check (label++": session must stop") (either (const True) (const False) outcome)
  check (label++": stop message") (either (isInfixOf fragment . displayException) (const False) outcome)
  check (label++": "++show halvesSent++" halves sent, expected "++show expectedHalvesSent)
    (halvesSent == expectedHalvesSent)
  check (label++": exactly one protocol close") (length [() | P.ControlWrite _ _ _ _ (0xc4:_) <- packets] == 1)
  check (label++": every host resource released once") (cleanupEvents == [Released,Closed,Exited])
  journal <- readFile (dir </> "journal.v1")
  check (label++": refusal or uncertainty is durable") (("ownership-refused" `isInfixOf` journal) || ("session-uncertain" `isInfixOf` journal))

main :: IO ()
main = do
  pid <- getProcessID
  tmp <- getTemporaryDirectory
  let root = tmp </> ("fuji-af-ownership-tests-"++show pid)
  createDirectory root

  -- The window list is the pinned ownership scope: ascending, and every
  -- pinned byte lies inside exactly one window.
  check "the window list is exactly the six ownership windows"
    (ownershipWindows == [0x00829900, 0x00829a00, 0x0093e600, 0x0093e700, 0x0093e800, 0x0093e900])
  check "windows are strictly increasing" (and (zipWith (<) ownershipWindows (drop 1 ownershipWindows)))
  check "every semaphore entry word for ids 1..169 lies inside one window"
    (all (\entry -> length [() | base <- ownershipWindows, semaphoreEntry entry >= base, semaphoreEntry entry + 4 <= base + probeWindowBytes] == 1) [1..169::Int])
  check "the cursor cell 0x00829a08 lies at +8 inside one window"
    (case [base | base <- ownershipWindows, base <= cursorCell, cursorCell + 4 <= base + probeWindowBytes] of
       [base] -> cursorCell - base == 8
       _ -> False)

  let happySteps = ownershipSteps happyOutcome
  (happy,packets,cleanupEvents) <- runCase (root </> "happy") happySteps
  check ("happy session completes "++show ownershipWindowCount++" windows: "++show happy)
    (either (const False) (==ownershipWindowCount) happy)
  check "happy session releases every resource once" (cleanupEvents == [Released,Closed,Exited])
  check "happy session closes the protocol once" (length [() | P.ControlWrite _ _ _ _ (0xc4:_) <- packets] == 1)

  -- The captured windows carry the synthetic bytes, and the journal carries
  -- each window's hash.
  happyJournal <- readFile (root </> "happy" </> "journal.v1")
  forM_ ownershipWindows $ \base -> do
    let label = probeName base
    bytes <- B.readFile (root </> "happy" </> "windows" </> (label++".bin"))
    check ("window bytes "++label) (B.unpack bytes == windowFor base)
    check ("journaled hash "++label) ((label++";sha256="++sha256 bytes) `isInfixOf` happyJournal)
  enabled <- B.readFile (root </> "happy" </> "windows" </> (gateReadbackEnabled++".bin"))
  restored <- B.readFile (root </> "happy" </> "windows" </> (gateReadbackRestored++".bin"))
  check "enable readback window" (B.unpack enabled == readbackPayload 1)
  check "restore readback window" (B.unpack restored == readbackPayload 0)
  files <- listDirectory (root </> "happy" </> "windows")
  check ("six window files and two readbacks, saw "++show (length files)) (length files == ownershipWindowCount+2)
  check "one completion record per window and one session record"
    (length (filter ("window-complete" `isInfixOf`) (lines happyJournal)) == ownershipWindowCount
      && length (filter ("ownership-read-complete-stock-unverified" `isInfixOf`) (lines happyJournal)) == 1)
  check "gate enable and restore are journaled with their values"
    (("index=247;value=1" `isInfixOf` happyJournal) && ("index=247;value=0" `isInfixOf` happyJournal)
      && ("gate-restored" `isInfixOf` happyJournal))

  -- Every byte the operation emits, audited over the session's own packets.
  outgoingLog <- readFile (root </> "happy" </> "outgoing.v1")
  check "the enable frame is durable before dispatch" (P.hexBytes (configWriteFrame gateEnabled) `isInfixOf` outgoingLog)
  check "the restore frame is durable before dispatch" (P.hexBytes (configWriteFrame gateDisabled) `isInfixOf` outgoingLog)
  let halfAddresses = concatMap (\window -> [window, window + probeHalfBytes]) ownershipWindows
  check "every window-half frame is durable before dispatch"
    (all (\address -> P.hexBytes (debugRequestFrame address) `isInfixOf` outgoingLog) halfAddresses)
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
      && map length debugFrames == replicate (ownershipWindowCount*2) debugRequestBytes)
  check "control writes use only the audited session opcodes"
    (all (\bytes -> take 1 bytes `elem` [[0xc0],[0xd0],[0xd1],[0xc4]]) controlBytes)
  check "one D1 control per request with the request's reply length"
    ([bytes | P.ControlWrite _ _ _ _ bytes@(0xd1:_) <- packets]
      == [P.control 0xd1 configReplyBytes sessionSid, P.control 0xd1 readReplyBytes sessionSid]
      ++ replicate (ownershipWindowCount*2) (P.control 0xd1 debugReplyForHalf sessionSid)
      ++ [P.control 0xd1 configReplyBytes sessionSid, P.control 0xd1 readReplyBytes sessionSid])
  check "config writes carry function 3, mailbox 1 and index 0xf7, never index 0x80"
    (map (word32At 0x10) writeFrames == replicate 2 3
      && map (word32At 0x14) writeFrames == replicate 2 1
      && all (\frame -> 256*word32At 0x18 frame + word32At 0x1c frame == gateIndex) writeFrames
      && all (\frame -> 256*word32At 0x18 frame + word32At 0x1c frame /= 0x80) writeFrames
      && map (word32At 0x20) writeFrames == replicate 2 configWriteLength)
  check "the emitted gate values are 1 then 0"
    ([last frame | frame <- writeFrames] == [fromIntegral gateEnabled, fromIntegral gateDisabled])
  check "no config read leaves index 0 and every one requests the full window"
    (all ((== 0) . word32At 0x1c) readbackFrames && all ((== 256) . word32At 0x20) readbackFrames)
  check "every debug frame carries function 2, a window-half address and a 128-byte length"
    (map (word32At 0x10) debugFrames == replicate (ownershipWindowCount*2) 2
      && map (word32At 0x20) debugFrames == replicate (ownershipWindowCount*2) probeHalfBytes
      && map (word32At 0x1c) debugFrames == halfAddresses)
  check "no emitted frame carries any other opcode"
    (all (\frame -> word32At 0 frame `elem` [readOpcode, debugOpcode]) bulkFrames)
  check "the outgoing bulk frames are exactly the deterministic plan"
    (bulkFrames == [configWriteFrame gateEnabled, readRequestFrame readbackWindow]
      ++ map debugRequestFrame halfAddresses
      ++ [configWriteFrame gateDisabled, readRequestFrame readbackWindow])

  -- Stop conditions, each against the exact byte that must cause it and the
  -- exact number of window-half requests that may precede the refusal.
  let firstHalf = case ownershipWindows of
        first:_ -> first
        [] -> error "the fixed ownership window list must not be empty"
      stopCases =
        [ ("gate enable is not visible", "gate enable is not visible", 0, ownershipSteps (mutate (GateRead gateReadbackEnabled) (const (P.Reply (configReadReply 0)))))
        , ("readback refused", "rejected arguments", 0, ownershipSteps (mutate (GateRead gateReadbackEnabled) (const (P.Reply (configReplyWith 9 3 2 0 256 (readbackPayload 1))))))
        , ("closed gate", "closed gate", 1, ownershipSteps (mutate (Half firstHalf) (const (P.Reply (debugReplyWith debugOpcode 1 0xffff 0 0 (replicate probeHalfBytes 0))))))
        , ("unproducible status", "status the handler cannot produce", 1, ownershipSteps (mutate (Half firstHalf) (const (P.Reply (debugReplyWith debugOpcode 2 7 0 0 (replicate probeHalfBytes 0))))))
        , ("nonzero reserved word", "reply reserved word is nonzero", 1, ownershipSteps (mutate (Half firstHalf) (const (P.Reply (word32le debugOpcode ++ drop 4 reservedDebugReply)))))
        , ("wrong opcode", "reply opcode differs", 1, ownershipSteps (mutate (Half firstHalf) (const (P.Reply (word32le 0x00200002 ++ drop 4 (debugReply firstHalf (replicate probeHalfBytes 0)))))))
        -- A short reply stops in the transfer layer with this message; the
        -- decoder's own length check is covered below.
        , ("short debug reply", "short/partial/oversized transfer; no retry", 1, ownershipSteps (mutate (Half firstHalf) (const (P.Reply (take (debugReplyForHalf-1) (debugReply firstHalf (replicate probeHalfBytes 0)))))))
        , ("echoed address mismatch", "differs from the request", 1, ownershipSteps (mutate (Half firstHalf) (const (P.Reply (debugReply (firstHalf+4) (replicate probeHalfBytes 0))))))
        , ("echoed length mismatch", "differs from the request", 1, ownershipSteps (mutate (Half firstHalf) (const (P.Reply (debugReplyWith debugOpcode 0 0 firstHalf 64 (replicate probeHalfBytes 0)))))) ]
  forM_ (zip [(0::Int)..] stopCases) $ \(index,(label,fragment,expected,steps)) ->
    expectStop (root </> ("stop-"++show index)) (label,fragment) expected steps

  -- The restore readback is only checked after every window has run.
  (restoreOutcome,restorePackets,restoreCleanup) <- runCase (root </> "restore-not-visible")
    (ownershipSteps (mutate (GateRead gateReadbackRestored) (const (P.Reply (configReadReply 1)))))
  check "restore not visible stops the session"
    (either (isInfixOf "gate restore is not visible" . displayException) (const False) restoreOutcome)
  check "restore not visible keeps all window captures and attempts one close"
    (length [() | P.BulkOut{} <- restorePackets] == 2 + 2 + ownershipWindowCount*2
      && length [() | P.ControlWrite _ _ _ _ (0xc4:_) <- restorePackets] == 1
      && restoreCleanup == [Released,Closed,Exited])
  restoreFiles <- listDirectory (root </> "restore-not-visible" </> "windows")
  check "every window and readback is captured before the stop" (length restoreFiles == ownershipWindowCount+2)

  -- A transport failure mid-session leaves the uncertainty record.
  (lostOutcome,lostPackets,lostCleanup) <- runCase (root </> "lost-reply")
    (ownershipSteps (mutate (Half (ownershipWindows !! 2)) (const P.LostReply)))
  check "lost window-half reply stops the session"
    (either (isInfixOf "injected transport failure" . displayException) (const False) lostOutcome)
  check "lost window-half reply still attempts one close and releases everything"
    (length [() | P.ControlWrite _ _ _ _ (0xc4:_) <- lostPackets] == 1 && lostCleanup == [Released,Closed,Exited])
  lostJournal <- readFile (root </> "lost-reply" </> "journal.v1")
  check "lost window-half reply journals the uncertainty" ("session-uncertain" `isInfixOf` lostJournal)

  -- The C0 reply decides the session id; a wrong length stops before any D0.
  (shortOpen,shortOpenPackets,shortOpenCleanup) <- runCase (root </> "short-open")
    (take 1 happySteps ++ [P.Step (P.BulkIn 0x81 8) (P.Reply (replicate 7 0))] ++ drop 2 happySteps)
  check "short C0 reply stops before D0 and cannot invent a close"
    (either (const True) (const False) shortOpen && shortOpenPackets == [P.controlTransfer (P.control 0xc0 8 0),P.BulkIn 0x81 8])
  check "short C0 reply releases every resource once" (shortOpenCleanup == [Released,Closed,Exited])

  -- Decoding is exact, checked apart from the session.
  check "exact debug decode"
    (decodeDebugReply 0x00829a00 (debugReply 0x00829a00 (replicate probeHalfBytes 7)) == Right (replicate probeHalfBytes 7))
  forM_ [ debugReplyWith debugOpcode 1 0xffff 0 0 (replicate probeHalfBytes 0)
        , debugReplyWith debugOpcode 9 3 0 0 (replicate probeHalfBytes 0)
        , debugReplyWith debugOpcode 2 7 0 0 (replicate probeHalfBytes 0)
        , word32le debugOpcode ++ drop 4 reservedDebugReply
        , take (debugReplyForHalf-1) (debugReply 0x00829a00 (replicate probeHalfBytes 7))
        , debugReply 0x00829a04 (replicate probeHalfBytes 7)
        , debugReplyWith debugOpcode 0 0 0x00829a00 64 (replicate probeHalfBytes 0) ] $ \bytes ->
    check "debug reply refusal" (either (const True) (const False) (decodeDebugReply 0x00829a00 bytes))
  check "exact write decode" (decodeConfigWriteReply configWriteReply == Right ())
  forM_ [ configReplyWith 1 0xffff 3 gateIndex 1 (replicate 256 0)
        , configReplyWith 9 3 3 gateIndex 1 (replicate 256 0)
        , configReplyWith 0 1 3 gateIndex 1 (replicate 256 0)
        , configReadReply 1
        , configReplyWith 0 0 3 (gateIndex+1) 1 (replicate 256 0) ] $ \bytes ->
    check "write reply refusal" (either (const True) (const False) (decodeConfigWriteReply bytes))

  removeDirectoryRecursive root
  putStrLn "probe-ownership: frame-byte, gate, window, stop and decoding checks passed"

-- The pinned table: entry word for id n at base + 4*(n-1), and the bump
-- allocator cursor cell.
semaphoreEntry :: Int -> Int
semaphoreEntry entry = 0x0093e6f4 + 4*(entry-1)

cursorCell :: Int
cursorCell = 0x00829a08

-- A debug reply whose reserved word at +0x0c is nonzero.
reservedDebugReply :: [Word8]
reservedDebugReply = take 12 (debugReply 0 (replicate probeHalfBytes 0)) ++ word32le 1 ++ drop 16 (debugReply 0 (replicate probeHalfBytes 0))
