module Main where

import Control.Exception
import Control.Monad (forM_, unless)
import Data.List (intercalate, isInfixOf)
import qualified Data.ByteString as B
import Data.IORef
import Data.Word (Word8)
import System.Directory
import System.FilePath
import System.Posix.Files (setFileMode)
import System.Posix.Process (getProcessID)
import FujiAF.Transport
import FujiAF.Read
import FujiAF.Probe
import FujiAF.Routes
import FujiAF.PlanDocument (readRoutePlanFile)
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

-- Reply builders. The config reply layout is the read path's; the 0x200001
-- function 2 reply carries the address and length echoes at +0x1c/+0x20 and
-- data from +0x24, at the request's own length.
configReplyWith :: Int -> Int -> Int -> Int -> Int -> [Word8] -> [Word8]
configReplyWith err1 err2 function offset size payload = concat
  [ word32le readOpcode, word32le err1, word32le err2, word32le 0
  , word32le function, word32le 1, word32le 0, word32le offset, word32le size ]
  ++ payload

configWriteReply :: [Word8]
configWriteReply = configReplyWith 0 0 3 gateIndex configWriteLength (replicate 256 0)

configReadReply :: Word8 -> [Word8]
configReadReply value = configReplyWith 0 0 2 0 readWindowLength (readbackPayload value)

readbackPayload :: Word8 -> [Word8]
readbackPayload value = take gateIndex (replicate 256 0x33) ++ [value] ++ drop (gateIndex+1) (replicate 256 0x33)

debugReplyWith :: Int -> Int -> Int -> Int -> Int -> [Word8] -> [Word8]
debugReplyWith opcode err1 err2 address size payload = concat
  [ word32le opcode, word32le err1, word32le err2, word32le 0
  , word32le 2, word32le 0, word32le 0
  , word32le address, word32le size ]
  ++ payload

routeReply :: Int -> Int -> [Word8] -> [Word8]
routeReply address size payload = debugReplyWith debugOpcode 0 0 address size payload

-- Every captured byte follows its absolute address.
bytesFor :: Int -> Int -> [Word8]
bytesFor address size = [fromIntegral ((address+offset) `mod` 251) | offset <- [0 .. size-1]]

-- The literal output shape of the revised plan's od/awk derivation for the
-- phase-1 class descriptor pair 0x00a00060 and 0x00a00040 (the window's first
-- two words): the cursor cell first, then one +0x60 and one +0x140 window per
-- distinct descriptor word, in capture word order and not ascending.
derivedPlanJson :: String
derivedPlanJson = concat
  [ "{\"windows\":[{\"index\":0,\"address\":\"0x00a0f86c\",\"length\":4,\"purpose\":\"allocation-search cursor cell\"}"
  , ",{\"index\":1,\"address\":\"0x00a000c0\",\"length\":32,\"purpose\":\"class descriptor +0x60 method fields\"}"
  , ",{\"index\":2,\"address\":\"0x00a001a0\",\"length\":40,\"purpose\":\"class descriptor +0x140 method fields\"}"
  , ",{\"index\":3,\"address\":\"0x00a000a0\",\"length\":32,\"purpose\":\"class descriptor +0x60 method fields\"}"
  , ",{\"index\":4,\"address\":\"0x00a00180\",\"length\":40,\"purpose\":\"class descriptor +0x140 method fields\"}"
  , "]}" ]

-- The window list the plan's derivation rule writes for that pair: the cursor
-- cell first, then one +0x60 and one +0x140 window per distinct descriptor
-- word, in capture word order and not ascending.
derivedWindows :: [RouteWindow]
derivedWindows =
  RouteWindow 0 routeCursorAddress routeCursorBytes "allocation-search cursor cell"
  : concat [ [ RouteWindow (2*index+1) (pointer+routeMethodFirstOffset) routeMethodFirstBytes "class descriptor +0x60 method fields"
            , RouteWindow (2*index+2) (pointer+routeMethodSecondOffset) routeMethodSecondBytes "class descriptor +0x140 method fields" ]
           | (index,pointer) <- zip [0..] [0x00a00060,0x00a00040] ]

routeSteps :: [RouteWindow] -> (RouteRequest -> P.Outcome) -> [P.Step]
routeSteps windows outcomeFor = openSteps ++ concatMap step (routeRequests windows)
  where
    step action = case action of
      RouteClose ->
        [ P.Step (P.controlTransfer (P.control 0xc4 2 sessionSid)) (P.Transferred 12)
        , P.Step (P.BulkIn 0x81 2) (outcomeFor action) ]
      RouteGateWrite value ->
        [ P.Step (P.controlTransfer (P.control 0xd0 configWriteBytes sessionSid)) (P.Transferred 12)
        , P.Step (P.BulkOut 2 (configWriteFrame value)) (P.Transferred configWriteBytes)
        , P.Step (P.controlTransfer (P.control 0xd1 configReplyBytes sessionSid)) (P.Transferred 12)
        , P.Step (P.BulkIn 0x81 configReplyBytes) (outcomeFor action) ]
      RouteGateRead _ ->
        [ P.Step (P.controlTransfer (P.control 0xd0 readRequestBytes sessionSid)) (P.Transferred 12)
        , P.Step (P.BulkOut 2 (readRequestFrame readbackWindow)) (P.Transferred readRequestBytes)
        , P.Step (P.controlTransfer (P.control 0xd1 readReplyBytes sessionSid)) (P.Transferred 12)
        , P.Step (P.BulkIn 0x81 readReplyBytes) (outcomeFor action) ]
      RouteRead address size ->
        [ P.Step (P.controlTransfer (P.control 0xd0 debugRequestBytes sessionSid)) (P.Transferred 12)
        , P.Step (P.BulkOut 2 (routeRequestFrame address size)) (P.Transferred debugRequestBytes)
        , P.Step (P.controlTransfer (P.control 0xd1 (routeReplyBytes size) sessionSid)) (P.Transferred 12)
        , P.Step (P.BulkIn 0x81 (routeReplyBytes size)) (outcomeFor action) ]

happyOutcome :: RouteRequest -> P.Outcome
happyOutcome action = case action of
  RouteGateWrite _ -> P.Reply configWriteReply
  RouteGateRead label
    | label == gateReadbackEnabled -> P.Reply (configReadReply 1)
    | otherwise -> P.Reply (configReadReply 0)
  RouteRead address size -> P.Reply (routeReply address size (bytesFor address size))
  RouteClose -> P.Reply [0x7c,0x11]

mutate :: RouteRequest -> (P.Outcome -> P.Outcome) -> RouteRequest -> P.Outcome
mutate target change action = if action == target then change (happyOutcome target) else happyOutcome action

data CleanupEvent = Released | Closed | Exited deriving (Eq, Show)

runCase :: FilePath -> [RouteWindow] -> [P.Step] -> IO (Either SomeException Int,[P.Transfer],[CleanupEvent])
runCase dir windows steps = do
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
  outcome <- try (runProbeRoutes (Clock (pure 0)) api dir windows)
  packets <- readIORef emitted
  cleanupEvents <- readIORef events
  pure (outcome,packets,cleanupEvents)

isRouteBulkOut :: P.Transfer -> Bool
isRouteBulkOut (P.BulkOut _ frame) = word32At 0 frame == debugOpcode
isRouteBulkOut _ = False

-- A stop is checked for its message, the read frames the session emitted, one
-- protocol close and the release of every host resource.
expectStop :: FilePath -> (String,String) -> Int -> [RouteWindow] -> [P.Step] -> IO ()
expectStop dir (label,fragment) expectedReads windows steps = do
  (outcome,packets,cleanupEvents) <- runCase dir windows steps
  let readsSent = length (filter isRouteBulkOut packets)
  check (label++": session must stop") (either (const True) (const False) outcome)
  check (label++": stop message") (either (isInfixOf fragment . displayException) (const False) outcome)
  check (label++": "++show readsSent++" read requests sent, expected "++show expectedReads)
    (readsSent == expectedReads)
  check (label++": exactly one protocol close") (length [() | P.ControlWrite _ _ _ _ (0xc4:_) <- packets] == 1)
  check (label++": every host resource released once") (cleanupEvents == [Released,Closed,Exited])
  journal <- readFile (dir </> "journal.v1")
  check (label++": refusal or uncertainty is durable") (("route-refused" `isInfixOf` journal) || ("session-uncertain" `isInfixOf` journal))

expectPlanRejection :: FilePath -> (String,String,String) -> IO ()
expectPlanRejection dir (label,body,fragment) = do
  let path = dir </> (label++".json")
  writeFile path body
  setFileMode path 0o600
  outcome <- try (readRoutePlanFile path) :: IO (Either SomeException [RouteWindow])
  check (label++": plan must be rejected") (either (const True) (const False) outcome)
  check (label++": rejection message") (either (isInfixOf fragment . displayException) (const False) outcome)

main :: IO ()
main = do
  pid <- getProcessID
  tmp <- getTemporaryDirectory
  let root = tmp </> ("fuji-af-routes-tests-"++show pid)
  createDirectory root

  -- The fixed phase is the plan's own two-window table.
  check "the fixed window list is the plan's two fixed windows"
    (routeFixedWindows == [ RouteWindow 0 0x0096bd20 256 "class descriptor table and adjacent objects"
                          , RouteWindow 1 0x00a0f86c 4 "allocation-search cursor cell" ])
  check "window 0 is the plan's 64 four-byte words"
    (routeWindowBytes (routeFixedWindows !! 0) == 4*64)
  check "the fixed read requests are the plan's three"
    (routeReads routeFixedWindows == [(0x0096bd20,128),(0x0096bda0,128),(0x00a0f86c,4)])
  check "phase 1 is three read requests and the plan's 32 transfers"
    (routeReadRequestCount routeFixedWindows == 3 && routeTransferCount 3 == 32)
  check "phase 2's derived list takes one request per short window"
    (routeReads derivedWindows
      == [(0x00a0f86c,4),(0x00a000c0,32),(0x00a001a0,40),(0x00a000a0,32),(0x00a00180,40)])
  check "phase 2 is 1 + 2k read requests and the plan's 24 + 8k transfers"
    (routeReadRequestCount derivedWindows == 5 && routeTransferCount 5 == 40)
  check "a 256-byte window takes two requests in either phase"
    (routeReads [RouteWindow 0 0x0096bd20 256 "class descriptor table"] == [(0x0096bd20,128),(0x0096bda0,128)])

  -- The derived plan file is read as the plan's document, and the window list
  -- it names is what the session reads.
  let planPath = root </> "marker0-routes-phase2.json"
  writeFile planPath derivedPlanJson
  setFileMode planPath 0o600
  planWindows <- readRoutePlanFile planPath
  check ("the derived plan reads back as its window list: "++show planWindows) (planWindows == derivedWindows)

  -- Phase 1 against the scripted transport end: exactly the plan's request
  -- bytes, counts and capture.
  let fixedSteps = routeSteps routeFixedWindows happyOutcome
  (fixed,fixedPackets,fixedCleanup) <- runCase (root </> "fixed") routeFixedWindows fixedSteps
  check ("fixed session completes its three requests: "++show fixed)
    (either (const False) (==3) fixed)
  check "fixed session releases every resource once" (fixedCleanup == [Released,Closed,Exited])
  let fixedBulkFrames = [bytes | P.BulkOut _ bytes <- fixedPackets]
      fixedControlFrames = [bytes | P.ControlWrite _ _ _ _ bytes <- fixedPackets]
      fixedDebugFrames = [frame | frame <- fixedBulkFrames, word32At 0 frame == debugOpcode]
  check "the fixed bulk frames are exactly the plan's sequence"
    (fixedBulkFrames == [ configWriteFrame gateEnabled, readRequestFrame readbackWindow
                        , routeRequestFrame 0x0096bd20 128, routeRequestFrame 0x0096bda0 128
                        , routeRequestFrame 0x00a0f86c 4
                        , configWriteFrame gateDisabled, readRequestFrame readbackWindow ])
  check "one D1 control per request with the request's own reply length"
    ([bytes | bytes@(0xd1:_) <- fixedControlFrames]
      == [ P.control 0xd1 configReplyBytes sessionSid, P.control 0xd1 readReplyBytes sessionSid
         , P.control 0xd1 (routeReplyBytes 128) sessionSid, P.control 0xd1 (routeReplyBytes 128) sessionSid
         , P.control 0xd1 (routeReplyBytes 4) sessionSid
         , P.control 0xd1 configReplyBytes sessionSid, P.control 0xd1 readReplyBytes sessionSid ])
  check "every control frame is twelve bytes"
    (all ((== 12) . length) fixedControlFrames)
  check "every read frame carries function 2, its request's address and its own length"
    (map (word32At 0x10) fixedDebugFrames == replicate 3 2
      && map (word32At 0x1c) fixedDebugFrames == [0x0096bd20,0x0096bda0,0x00a0f86c]
      && map (word32At 0x20) fixedDebugFrames == [128,128,4]
      && all ((== debugRequestBytes) . length) fixedDebugFrames)
  check "no emitted bulk frame carries any other opcode"
    (all (\frame -> word32At 0 frame `elem` [readOpcode, debugOpcode]) fixedBulkFrames)
  check "config writes carry function 3, mailbox 1 and index 0xf7, enabled then disabled"
    (let writes = [frame | frame <- fixedBulkFrames, word32At 0 frame == readOpcode, word32At 0x10 frame == 3]
     in map (word32At 0x14) writes == [1,1]
        && all (\frame -> 256*word32At 0x18 frame + word32At 0x1c frame == gateIndex) writes
        && [last frame | frame <- writes] == [fromIntegral gateEnabled, fromIntegral gateDisabled])
  fixedWindow0 <- B.readFile (root </> "fixed" </> "windows" </> "0096bd20.bin")
  fixedCursor <- B.readFile (root </> "fixed" </> "windows" </> "00a0f86c.bin")
  check "window 0 is both halves in order"
    (B.unpack fixedWindow0 == bytesFor 0x0096bd20 256)
  check "the cursor cell window is its four bytes" (B.unpack fixedCursor == bytesFor 0x00a0f86c 4)
  fixedFiles <- listDirectory (root </> "fixed" </> "windows")
  check ("two window files and two readbacks, saw "++show (length fixedFiles)) (length fixedFiles == 4)
  fixedJournal <- readFile (root </> "fixed" </> "journal.v1")
  check ("a cursor-only plan's allowance covers a fixed session's journal: "++show (length fixedJournal))
    (length fixedJournal <= journalCapFor 1)
  check "the fixed journal carries the plan's counted completion record"
    ("route-read-complete-stock-unverified" `isInfixOf` fixedJournal
      && ("3 read requests;32 transfers" `isInfixOf` fixedJournal))
  check "the fixed journal carries the prepared record and both gate values"
    (("probe-routes;windows=2;read-requests=3;gate-index=247" `isInfixOf` fixedJournal)
      && ("index=247;value=1" `isInfixOf` fixedJournal) && ("index=247;value=0" `isInfixOf` fixedJournal))
  forM_ routeFixedWindows $ \window -> do
    let label = probeName (routeWindowAddress window)
        payload = B.pack (bytesFor (routeWindowAddress window) (routeWindowBytes window))
    check ("the fixed record shape of window "++label)
      (("index="++show (routeWindowIndex window)
        ++";address=0x"++label
        ++";length="++show (routeWindowBytes window)
        ++";purpose="++routeWindowPurpose window
        ++";sha256="++sha256 payload) `isInfixOf` fixedJournal)
  fixedOutgoing <- readFile (root </> "fixed" </> "outgoing.v1")
  check "every fixed read frame is durable before dispatch"
    (all (\frame -> P.hexBytes frame `isInfixOf` fixedOutgoing)
      (map (uncurry routeRequestFrame) (routeReads routeFixedWindows)))

  -- Phase 2 against the scripted transport end: one request per derived
  -- window, in the plan file's order, and a distinct capture.
  (derived,derivedPackets,derivedCleanup) <- runCase (root </> "derived") derivedWindows (routeSteps derivedWindows happyOutcome)
  check ("derived session completes its 1 + 2k requests: "++show derived)
    (either (const False) (==5) derived)
  check "derived session releases every resource once" (derivedCleanup == [Released,Closed,Exited])
  let derivedBulkFrames = [bytes | P.BulkOut _ bytes <- derivedPackets]
      derivedControlFrames = [bytes | P.ControlWrite _ _ _ _ bytes <- derivedPackets]
  check "the derived bulk frames are exactly the plan file's window sequence"
    (derivedBulkFrames == [ configWriteFrame gateEnabled, readRequestFrame readbackWindow ]
      ++ [routeRequestFrame address size | (address,size) <- routeReads derivedWindows]
      ++ [ configWriteFrame gateDisabled, readRequestFrame readbackWindow ])
  check "derived D1 controls carry each request's own reply length"
    ([bytes | bytes@(0xd1:_) <- derivedControlFrames]
      == [P.control 0xd1 configReplyBytes sessionSid, P.control 0xd1 readReplyBytes sessionSid]
      ++ [P.control 0xd1 (routeReplyBytes size) sessionSid | (_,size) <- routeReads derivedWindows]
      ++ [P.control 0xd1 configReplyBytes sessionSid, P.control 0xd1 readReplyBytes sessionSid])
  forM_ derivedWindows $ \window -> do
    let label = probeName (routeWindowAddress window)
    bytes <- B.readFile (root </> "derived" </> "windows" </> (label++".bin"))
    check ("the derived capture "++label) (B.unpack bytes == bytesFor (routeWindowAddress window) (routeWindowBytes window))
  derivedFiles <- listDirectory (root </> "derived" </> "windows")
  check ("five window files and two readbacks, saw "++show (length derivedFiles)) (length derivedFiles == 7)
  derivedJournal <- readFile (root </> "derived" </> "journal.v1")
  check "the derived journal carries the counted completion record"
    ("5 read requests;40 transfers" `isInfixOf` derivedJournal)
  forM_ derivedWindows $ \window -> do
    let label = probeName (routeWindowAddress window)
        payload = B.pack (bytesFor (routeWindowAddress window) (routeWindowBytes window))
    check ("the derived record shape of window "++label)
      (("index="++show (routeWindowIndex window)
        ++";address=0x"++label
        ++";length="++show (routeWindowBytes window)
        ++";purpose="++routeWindowPurpose window
        ++";sha256="++sha256 payload) `isInfixOf` derivedJournal)

  -- Stop conditions, each against the exact byte that must cause it and the
  -- exact number of read requests that may precede the refusal.
  let firstRead = case routeReads routeFixedWindows of
        first:_ -> first
        [] -> error "the fixed read list must not be empty"
      stopCases =
        [ ("gate enable is not visible", "gate enable is not visible", 0, routeFixedWindows
          , routeSteps routeFixedWindows (mutate (RouteGateRead gateReadbackEnabled) (const (P.Reply (configReadReply 0)))))
        , ("readback refused", "rejected arguments", 0, routeFixedWindows
          , routeSteps routeFixedWindows (mutate (RouteGateRead gateReadbackEnabled) (const (P.Reply (configReplyWith 9 3 2 0 256 (readbackPayload 1))))))
        , ("closed gate", "closed gate", 1, routeFixedWindows
          , routeSteps routeFixedWindows (mutate (uncurry RouteRead firstRead) (const (P.Reply (debugReplyWith debugOpcode 1 0xffff 0 0 (replicate 128 0))))))
        , ("unproducible status", "status the handler cannot produce", 1, routeFixedWindows
          , routeSteps routeFixedWindows (mutate (uncurry RouteRead firstRead) (const (P.Reply (debugReplyWith debugOpcode 2 7 0 0 (replicate 128 0))))))
        , ("wrong opcode", "reply opcode differs", 1, routeFixedWindows
          , routeSteps routeFixedWindows (mutate (uncurry RouteRead firstRead) (const (P.Reply (word32le 0x00200002 ++ drop 4 (routeReply (fst firstRead) (snd firstRead) (bytesFor (fst firstRead) (snd firstRead))))))))
        , ("echoed address mismatch", "differs from the request", 1, routeFixedWindows
          , routeSteps routeFixedWindows (mutate (uncurry RouteRead firstRead) (const (P.Reply (routeReply (fst firstRead + 4) (snd firstRead) (bytesFor (fst firstRead) (snd firstRead)))))))
        , ("echoed length mismatch", "differs from the request", 1, routeFixedWindows
          , routeSteps routeFixedWindows (mutate (uncurry RouteRead firstRead) (const (P.Reply (debugReplyWith debugOpcode 0 0 (fst firstRead) 4 (replicate 128 0))))))
        , ("short reply", "short/partial/oversized transfer; no retry", 1, routeFixedWindows
          , routeSteps routeFixedWindows (mutate (uncurry RouteRead firstRead) (const (P.Reply (take (routeReplyBytes (snd firstRead)-1) (routeReply (fst firstRead) (snd firstRead) (bytesFor (fst firstRead) (snd firstRead))))))))
        , ("lost reply", "injected transport failure", 3, routeFixedWindows
          , routeSteps routeFixedWindows (mutate (uncurry RouteRead (routeReads routeFixedWindows !! 2)) (const P.LostReply)))
        , ("closed gate on a derived window", "closed gate", 5, derivedWindows
          , routeSteps derivedWindows (mutate (uncurry RouteRead (routeReads derivedWindows !! 4)) (const (P.Reply (debugReplyWith debugOpcode 1 0xffff 0 0 (replicate 40 0)))))) ]
  forM_ (zip [(0::Int)..] stopCases) $ \(index,(label,fragment,expected,windows,steps)) ->
    expectStop (root </> ("stop-"++show index)) (label,fragment) expected windows steps

  -- The restore readback is only checked after every window has run, so a
  -- failed restore keeps the whole capture.
  (restoreOutcome,restorePackets,restoreCleanup) <- runCase (root </> "restore-not-visible") routeFixedWindows
    (routeSteps routeFixedWindows (mutate (RouteGateRead gateReadbackRestored) (const (P.Reply (configReadReply 1)))))
  check "restore not visible stops the session"
    (either (isInfixOf "gate restore is not visible" . displayException) (const False) restoreOutcome)
  check "restore not visible keeps every capture and attempts one close"
    (length (filter isRouteBulkOut restorePackets) == 3
      && length [() | P.ControlWrite _ _ _ _ (0xc4:_) <- restorePackets] == 1
      && restoreCleanup == [Released,Closed,Exited])
  restoreFiles <- listDirectory (root </> "restore-not-visible" </> "windows")
  check "every window and readback is captured before the stop" (length restoreFiles == 4)

  -- Decoding is exact, checked apart from the session.
  check "exact route decode"
    (decodeRouteReply 0x00a0f86c 4 (routeReply 0x00a0f86c 4 (bytesFor 0x00a0f86c 4)) == Right (bytesFor 0x00a0f86c 4))
  check "rejected arguments and the closed gate decode to their own messages"
    (decodeRouteReply 0 0 (debugReplyWith debugOpcode 9 3 0 0 []) == Left "rejected arguments: error1 9, error2 3"
      && decodeRouteReply 0 0 (debugReplyWith debugOpcode 1 0xffff 0 0 []) == Left "closed gate: error1 1, error2 0xffff")
  check "a short request reply length is refused by the decoder"
    (decodeRouteReply 0x00a0f86c 4 (routeReply 0x00a0f86c 8 (bytesFor 0x00a0f86c 8)) == Left "reply length 44 differs from the requested 40")

  -- The document reader is closed: a derived list must be the shape the
  -- derivation writes.
  let planDir = root </> "plan-rejections"
  createDirectoryIfMissing True planDir
  forM_
    [ ("duplicate address", "{\"windows\":[{\"index\":0,\"address\":\"0x00a0f86c\",\"length\":4,\"purpose\":\"cursor\"},{\"index\":1,\"address\":\"0x00a0f86c\",\"length\":4,\"purpose\":\"cursor again\"}]}", "window address must be distinct")
    , ("index not its position", "{\"windows\":[{\"index\":1,\"address\":\"0x00a0f86c\",\"length\":4,\"purpose\":\"cursor\"}]}", "window index must equal its position")
    , ("zero length", "{\"windows\":[{\"index\":0,\"address\":\"0x00a0f86c\",\"length\":0,\"purpose\":\"cursor\"}]}", "bounded exact integer required")
    , ("length above a window", "{\"windows\":[{\"index\":0,\"address\":\"0x00a0f86c\",\"length\":257,\"purpose\":\"cursor\"}]}", "bounded exact integer required")
    , ("float length is not an exact integer", "{\"windows\":[{\"index\":0,\"address\":\"0x00a0f86c\",\"length\":4.0,\"purpose\":\"cursor\"}]}", "Failed reading")
    , ("short address", "{\"windows\":[{\"index\":0,\"address\":\"0xa0f86c\",\"length\":4,\"purpose\":\"cursor\"}]}", "fixed-width lowercase hex address required")
    , ("uppercase address", "{\"windows\":[{\"index\":0,\"address\":\"0x00A0F86C\",\"length\":4,\"purpose\":\"cursor\"}]}", "fixed-width lowercase hex address required")
    , ("address beyond the 32-bit space", "{\"windows\":[{\"index\":0,\"address\":\"0xfffffff0\",\"length\":256,\"purpose\":\"cursor\"}]}", "inside the 32-bit address space")
    , ("empty window list", "{\"windows\":[]}", "bounded list required")
    , ("extra key", "{\"windows\":[{\"index\":0,\"address\":\"0x00a0f86c\",\"length\":4,\"purpose\":\"cursor\",\"offset\":0}]}", "closed document keys required")
    , ("missing key", "{\"windows\":[{\"index\":0,\"address\":\"0x00a0f86c\",\"length\":4}]}", "closed document keys required")
    , ("empty purpose", "{\"windows\":[{\"index\":0,\"address\":\"0x00a0f86c\",\"length\":4,\"purpose\":\"\"}]}", "bounded text required")
    , ("top-level extra key", "{\"windows\":[{\"index\":0,\"address\":\"0x00a0f86c\",\"length\":4,\"purpose\":\"cursor\"}],\"plan_id\":\"x\"}", "closed document keys required") ]
    (expectPlanRejection planDir)
  do
    -- A file must be an absolute path and private.
    let relative = "relative-plan.json"
    writeFile relative derivedPlanJson
    setFileMode relative 0o600
    outcome <- try (readRoutePlanFile relative) :: IO (Either SomeException [RouteWindow])
    check "a relative plan path is refused"
      (either (isInfixOf "absolute plan path required" . displayException) (const False) outcome)
    removeFile relative
    let readable = planDir </> "readable.json"
    writeFile readable derivedPlanJson
    setFileMode readable 0o644
    outcome' <- try (readRoutePlanFile readable) :: IO (Either SomeException [RouteWindow])
    check "a group-readable plan is refused"
      (either (isInfixOf "private owned plan required" . displayException) (const False) outcome')
    let tooMany = planDir </> "too-many.json"
    writeFile tooMany (concat
      [ "{\"windows\":["
      , intercalate "," [ "{\"index\":"++show index++",\"address\":\"0x00a0f86c\",\"length\":4,\"purpose\":\"cursor\"}" | index <- [0 .. planWindowCap] ]
      , "]}" ])
    setFileMode tooMany 0o600
    outcome'' <- try (readRoutePlanFile tooMany) :: IO (Either SomeException [RouteWindow])
    check "a window list above the plan cap is refused"
      (either (isInfixOf "bounded list required" . displayException) (const False) outcome'')
    let ceilingCount = length [() | count <- [0 .. planWindowCap], journalCapFor count <= journalCeilingBytes]
        aboveCeiling = planDir </> "above-ceiling.json"
    writeFile aboveCeiling (concat
      [ "{\"windows\":["
      , intercalate "," [ "{\"index\":"++show index++",\"address\":\"0x"++probeName (0x00100000+4*index)++"\",\"length\":4,\"purpose\":\"cursor\"}" | index <- [0 .. ceilingCount-1] ]
      , "]}" ])
    setFileMode aboveCeiling 0o600
    outcome''' <- try (readRoutePlanFile aboveCeiling) :: IO (Either SomeException [RouteWindow])
    check ("a "++show ceilingCount++"-window list above the journal ceiling is refused")
      (either (isInfixOf "planned window count exceeds the journal ceiling" . displayException) (const False) outcome''')

  putStrLn "probe-routes: all checks passed"
