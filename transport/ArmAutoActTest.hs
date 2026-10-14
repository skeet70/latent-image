-- Behavioural tests for the arm-auto-act session: the exact transfer
-- sequence it emits for a given camera, its refusals, the files and journal
-- lines it writes, and the witness-cell to outcome table. The expected
-- frames are built here from the literals the review names, so a change in
-- the module's own builders cannot pass these tests.
module Main where

import Control.Exception (SomeException, displayException, try)
import Control.Monad (forM_, unless)
import qualified Data.ByteString as B
import Data.IORef
import Data.List (isInfixOf)
import Data.Word (Word8)
import System.Directory (createDirectory, createDirectoryIfMissing, getTemporaryDirectory, removeDirectoryRecursive)
import System.FilePath ((</>))
import System.Posix.Process (getProcessID)
import FujiAF.Transport
import FujiAF.Read (word32At, word32le)
import FujiAF.Probe
  ( configReplyBytes, configWriteBytes, debugReplyForHalf, debugRequestBytes
  , gateIndex, gateReadbackEnabled, gateReadbackRestored )
import FujiAF.ArmAutoAct
import qualified FujiAF.Broker as P

check :: String -> Bool -> IO ()
check label yes = unless yes (error label)

testDescriptors :: Descriptors
testDescriptors = Descriptors 1 0 0 255 0 0 0x81 2 512 512 0 1 []

sessionSid :: Int
sessionSid = 0x1234

armIndex, readWindowLength, readRequestBytes :: Int
armIndex = 0xa2
readWindowLength = 256
readRequestBytes = 0x24

configReadOpcode, ramReadOpcode, ramHalfBytes :: Int
configReadOpcode = 0x1a0002
ramReadOpcode = 0x00200001
ramHalfBytes = 128

-- The WT_LOG handler calls its writer with 0xfff (about 4 KiB); that write's
-- completion count is what a settled ran-to-end shows in 0x16777bc.
logWriteBytes :: Int
logWriteBytes = 0xfff

-- The request frames, from the literals: config function 3 at one index with
-- one appended byte, config read of page 0 offset 0 length 256, and the
-- 0x200001 function 2 RAM read of 128 bytes.
writeFrame :: Int -> Word8 -> [Word8]
writeFrame index value = concat
  [ [0x02,0x00,0x1a,0x00], replicate 12 0
  , [0x03,0x00,0x00,0x00], [0x01,0x00,0x00,0x00], [0x00,0x00,0x00,0x00]
  , word32le index, [0x01,0x00,0x00,0x00], [value] ]

readFrame :: [Word8]
readFrame = concat
  [ [0x02,0x00,0x1a,0x00], replicate 12 0
  , [0x02,0x00,0x00,0x00], [0x01,0x00,0x00,0x00], [0x00,0x00,0x00,0x00]
  , [0x00,0x00,0x00,0x00], [0x00,0x01,0x00,0x00] ]

debugFrame :: Int -> [Word8]
debugFrame address = concat
  [ [0x01,0x00,0x20,0x00], replicate 12 0
  , [0x02,0x00,0x00,0x00], [0x00,0x00,0x00,0x00], [0x00,0x00,0x00,0x00]
  , word32le address, [0x80,0x00,0x00,0x00] ]

-- The replies the camera sends, from the live-captured layouts: the config
-- reply's echoes at 0x18/0x1c/0x20 with data at 0x24, and the RAM read's
-- address and length echoes at 0x1c/0x20 with data at 0x24.
writeReply :: Int -> [Word8]
writeReply index = concat
  [ [0x02,0x00,0x1a,0x00], replicate 12 0
  , [0x03,0x00,0x00,0x00], [0x01,0x00,0x00,0x00], [0x00,0x00,0x00,0x00]
  , word32le index, [0x01,0x00,0x00,0x00] ] ++ replicate 256 0

pageReply :: [Word8] -> [Word8]
pageReply payload = concat
  [ [0x02,0x00,0x1a,0x00], replicate 12 0
  , [0x02,0x00,0x00,0x00], [0x01,0x00,0x00,0x00], [0x00,0x00,0x00,0x00]
  , [0x00,0x00,0x00,0x00], [0x00,0x01,0x00,0x00] ] ++ payload

ramReply :: Int -> [Word8] -> [Word8]
ramReply address payload = concat
  [ [0x01,0x00,0x20,0x00], replicate 12 0
  , [0x02,0x00,0x00,0x00], [0x00,0x00,0x00,0x00], [0x00,0x00,0x00,0x00]
  , word32le address, [0x80,0x00,0x00,0x00] ] ++ payload

-- The 256-byte page-0 window with the two adjustment bytes this session
-- touches.
pageBytes :: Word8 -> Word8 -> [Word8]
pageBytes arm gate = patch [(armIndex,[arm]),(gateIndex,[gate])] (replicate 256 0)

patch :: [(Int,[Word8])] -> [Word8] -> [Word8]
patch edits window = foldl apply window edits
  where apply bytes (offset,replacement) = take offset bytes ++ replacement ++ drop (offset + length replacement) bytes

-- The witness windows with the loader's own in-window offsets: guard 0x48,
-- the loader's -1 sentinel 0x50, the statement index 0x54, the count 0x58,
-- the first record's code 0x5c and text 0x60; input length 0xac, in flight
-- 0xb0, status 0xb4, open result 0xb8, completion count 0xbc and selector
-- 0xc0.
windowA :: [Word8] -> [Word8]
windowA = patch
  [ (0x48,[1]), (0x50,replicate 4 0xff), (0x54,replicate 4 0xff)
  , (0x58,word32le 3), (0x5c,word32le 0x18), (0x60,map (fromIntegral . fromEnum) "WT_LOG") ]

-- Window A with only the guard set: the sender posted and the loader has
-- produced nothing, the no-load state.
windowAGuard :: [Word8] -> [Word8]
windowAGuard = patch [(0x48,[1])]

windowBFired :: [Word8] -> [Word8]
windowBFired = patch [(0xac,word32le 13),(0xbc,word32le logWriteBytes),(0xc0,word32le 1)]

windowBFailed :: [Word8] -> [Word8]
windowBFailed = patch [(0xb4,word32le 5)]

-- A run that reached END with its own file operation failed: the log write's
-- status is nonzero and the statement index is at END.
windowBFileOpFailed :: [Word8] -> [Word8]
windowBFileOpFailed = patch [(0xac,word32le 13),(0xb4,word32le 5),(0xc0,word32le 1)]

windowBRunning :: [Word8] -> [Word8]
windowBRunning = patch [(0xac,word32le 13),(0xbc,word32le logWriteBytes),(0xc0,word32le 2)]

-- Window A with the statement index short of END: the script is still
-- executing, which keeps the watch polling.
windowARunning :: [Word8] -> [Word8]
windowARunning = patch
  [ (0x48,[1]), (0x50,replicate 4 0xff), (0x54,word32le 1)
  , (0x58,word32le 3), (0x5c,word32le 0x18), (0x60,map (fromIntegral . fromEnum) "WT_LOG") ]

zeros :: [Word8]
zeros = replicate 256 0

-- The transfer helpers.
openSteps :: [P.Step]
openSteps =
  [ P.Step (P.controlTransfer (P.control 0xc0 8 0)) (P.Transferred 12)
  , P.Step (P.BulkIn 0x81 8) (P.Reply [0x34,0x12,0,0,0,0,1,0]) ]

closeSteps :: [P.Step]
closeSteps =
  [ P.Step (P.controlTransfer (P.control 0xc4 2 sessionSid)) (P.Transferred 12)
  , P.Step (P.BulkIn 0x81 2) (P.Reply [0,0]) ]

pageSteps :: [Word8] -> [P.Step]
pageSteps payload =
  [ P.Step (P.controlTransfer (P.control 0xd0 readRequestBytes sessionSid)) (P.Transferred 12)
  , P.Step (P.BulkOut 2 readFrame) (P.Transferred readRequestBytes)
  , P.Step (P.controlTransfer (P.control 0xd1 configReplyBytes sessionSid)) (P.Transferred 12)
  , P.Step (P.BulkIn 0x81 configReplyBytes) (P.Reply (pageReply payload)) ]

writeSteps :: Int -> Word8 -> [P.Step]
writeSteps index value =
  [ P.Step (P.controlTransfer (P.control 0xd0 configWriteBytes sessionSid)) (P.Transferred 12)
  , P.Step (P.BulkOut 2 (writeFrame index value)) (P.Transferred configWriteBytes)
  , P.Step (P.controlTransfer (P.control 0xd1 configReplyBytes sessionSid)) (P.Transferred 12)
  , P.Step (P.BulkIn 0x81 configReplyBytes) (P.Reply (writeReply index)) ]

halfSteps :: Int -> [Word8] -> [P.Step]
halfSteps address payload =
  [ P.Step (P.controlTransfer (P.control 0xd0 debugRequestBytes sessionSid)) (P.Transferred 12)
  , P.Step (P.BulkOut 2 (debugFrame address)) (P.Transferred debugRequestBytes)
  , P.Step (P.controlTransfer (P.control 0xd1 debugReplyForHalf sessionSid)) (P.Transferred 12)
  , P.Step (P.BulkIn 0x81 debugReplyForHalf) (P.Reply (ramReply address payload)) ]

windowSteps :: Int -> [Word8] -> [P.Step]
windowSteps base window =
  halfSteps base (take ramHalfBytes window) ++ halfSteps (base + ramHalfBytes) (drop ramHalfBytes window)

-- One watch poll reads the first half of window A and the second half of
-- window B.
pollSteps :: [Word8] -> [Word8] -> [P.Step]
pollSteps a b = halfSteps 0x01667000 (take ramHalfBytes a) ++ halfSteps 0x01677780 (drop ramHalfBytes b)
tickSteps :: Int -> [P.Step]
tickSteps tick = halfSteps 0x400c0800 (word32le tick ++ replicate 124 0)

-- The session's fixed prefix up to and including the confirm read.
armPrefix :: [Word8] -> [P.Step]
armPrefix pagePrecondition =
  openSteps
  ++ pageSteps pagePrecondition
  ++ writeSteps gateIndex 1
  ++ pageSteps (pageBytes 0 1)
  ++ tickSteps 499
  ++ windowSteps 0x01667000 zeros
  ++ windowSteps 0x01677700 zeros
  ++ writeSteps armIndex 2
  ++ pageSteps (pageBytes 2 1)

-- The cleanup on every exit path: the gate restore, its readback and C4.
cleanupSteps :: Word8 -> [P.Step]
cleanupSteps armByte = writeSteps gateIndex 0 ++ pageSteps (pageBytes armByte 0) ++ closeSteps

-- The watch never settles: every poll reads the boot state and the watch ends
-- on its cap.
noSendSteps :: [P.Step]
noSendSteps = armPrefix zeros
  ++ concat (replicate watchPollCap (pollSteps zeros zeros))
  ++ windowSteps 0x01667000 zeros
  ++ windowSteps 0x01677700 zeros
  ++ tickSteps 9487
  ++ cleanupSteps 2

-- A failed file operation before END: the guard is set, the loader's clear
-- has left the statement area empty and the status is nonzero.
failedSteps :: [P.Step]
failedSteps = armPrefix zeros
  ++ pollSteps zeros zeros
  ++ pollSteps (windowAGuard zeros) (windowBFailed zeros)
  ++ windowSteps 0x01667000 (windowAGuard zeros)
  ++ windowSteps 0x01677700 (windowBFailed zeros)
  ++ tickSteps 9487
  ++ cleanupSteps 2

-- A script that reached END with its own file operation failed: the ended
-- statement window with a nonzero status, the new outcome.
fileOpFailedSteps :: [P.Step]
fileOpFailedSteps = armPrefix zeros
  ++ pollSteps zeros zeros
  ++ pollSteps (windowA zeros) (windowBFileOpFailed zeros)
  ++ windowSteps 0x01667000 (windowA zeros)
  ++ windowSteps 0x01677700 (windowBFileOpFailed zeros)
  ++ tickSteps 9487
  ++ cleanupSteps 2

-- A script still executing at one poll and ended at the next: the watch
-- keeps polling while the statement index has not reached END.
runningSteps :: [P.Step]
runningSteps = armPrefix zeros
  ++ pollSteps zeros zeros
  ++ pollSteps (windowARunning zeros) (windowBRunning zeros)
  ++ pollSteps (windowA zeros) (windowBFired zeros)
  ++ windowSteps 0x01667000 (windowA zeros)
  ++ windowSteps 0x01677700 (windowBFired zeros)
  ++ tickSteps 9487
  ++ cleanupSteps 2

happySteps :: [P.Step]
happySteps = armPrefix zeros
  ++ pollSteps zeros zeros
  ++ pollSteps (windowA zeros) (windowBFired zeros)
  ++ windowSteps 0x01667000 (windowA zeros)
  ++ windowSteps 0x01677700 (windowBFired zeros)
  ++ tickSteps 9487
  ++ cleanupSteps 2

runCase :: FilePath -> [P.Step] -> IO (Either SomeException ArmReport, [P.Transfer], [String])
runCase dir steps = runDeviceCase RequirePresent (Clock (pure 0)) (pure [testDevice]) dir steps

testDevice :: Candidate
testDevice = Candidate "001:2.3" 0x04cb 0xff80 testDescriptors

runDeviceCase :: DeviceStart -> Clock -> IO [Candidate] -> FilePath -> [P.Step] -> IO (Either SomeException ArmReport, [P.Transfer], [String])
runDeviceCase mode clock inventory dir steps = do
  createDirectoryIfMissing True dir
  emitted <- newIORef []
  events <- newIORef []
  script <- newIORef steps
  let api = UsbApi
        (\_ -> pure ())
        (\_ -> inventory)
        (\_ _ -> pure ())
        (\_ _ -> pure ())
        (\packet _ -> do
          modifyIORef' emitted (++[packet])
          restScript <- readIORef script
          case restScript of
            [] -> P.reject ("script exhausted at "++show packet)
            P.Step expected outcome:rest -> do
              unless (packet == expected) (P.reject ("emitted "++show packet++" differs from the expected "++show expected))
              writeIORef script rest
              case outcome of
                P.Transferred count -> pure (count,[])
                P.Reply bytes -> pure (length bytes,bytes)
                P.LostReply -> P.reject "injected transport failure")
        (\_ _ -> modifyIORef' events (++["released"]))
        (\_ -> modifyIORef' events (++["closed"]))
        (\_ -> modifyIORef' events (++["exited"]))
  outcome <- try (runArmAutoAct mode clock api dir)
  packets <- readIORef emitted
  cleanupEvents <- readIORef events
  pure (outcome, packets, cleanupEvents)

rejectedWriteReply :: Int -> [Word8]
rejectedWriteReply index = take 4 (writeReply index) ++ [1,0,0,0,0xff,0xff,0,0] ++ drop 12 (writeReply index)

main :: IO ()
main = do
  pid <- getProcessID
  tmp <- getTemporaryDirectory
  let base = tmp </> ("fuji-af-arm-auto-act-tests-"++show pid)
  createDirectory base

  -- The happy camera: the arm write takes, the guard sets at the second poll
  -- and the 13-byte script ran to END.
  (happy,packets,cleanupEvents) <- runCase (base </> "happy") happySteps
  case happy of
    Left e -> error ("happy session stopped: "++displayException e)
    Right report -> do
      check "happy outcome is ran-to-end" (reportOutcome report == ArmRanToEnd)
      check "happy settled on its own predicate" (reportSettled report)
      check "happy watch took two polls" (reportPolls report == 2)
      check "happy witness selector is 1" (witnessSelector (reportWitness report) == 1)
      check "happy witness index is -1" (witnessStatementIndex (reportWitness report) == -1)
      check "happy witness count is 3" (witnessStatementCount (reportWitness report) == 3)
      check "happy witness length is the 13-byte load" (witnessInputLength (reportWitness report) == 13)
      check "happy witness completion count is the log write's"
        (witnessCompletionCount (reportWitness report) == logWriteBytes)
      check "happy witness text is WT_LOG" (witnessStatementText (reportWitness report) == "WT_LOG")
      check "first and end tick readings are decoded"
        (reportFirstTick report == CameraTick 499 && reportEndTick report == CameraTick 9487)
      check "result line includes ticks and measured-rate seconds"
        (";tick-first=499;uptime-first-s=4.996;tick-end=9487;uptime-end-s=94.984" `isInfixOf` armReportLine report)
  check "happy releases every host resource once" (cleanupEvents == ["released","closed","exited"])

  -- The captures and the journal.
  precondition <- B.readFile (base </> "happy" </> "precondition.bin")
  armReadback <- B.readFile (base </> "happy" </> "arm-readback.bin")
  enabled <- B.readFile (base </> "happy" </> (gateReadbackEnabled++".bin"))
  restored <- B.readFile (base </> "happy" </> (gateReadbackRestored++".bin"))
  beforeA <- B.readFile (base </> "happy" </> "witness" </> "before-01667000.bin")
  afterA <- B.readFile (base </> "happy" </> "witness" </> "after-01667000.bin")
  afterB <- B.readFile (base </> "happy" </> "witness" </> "after-01677700.bin")
  check "precondition window is captured" (B.unpack precondition == zeros)
  check "arm readback shows 0xa2 = 2" (B.unpack armReadback == pageBytes 2 1)
  check "gate readback shows 0xf7 = 1" (B.unpack enabled == pageBytes 0 1)
  check "gate restore readback shows 0xf7 = 0" (B.unpack restored == pageBytes 2 0)
  check "before window is the boot state" (B.unpack beforeA == zeros)
  check "after window A is the ended state" (B.unpack afterA == windowA zeros)
  check "after window B is the settled state" (B.unpack afterB == windowBFired zeros)
  happyJournal <- readFile (base </> "happy" </> "journal.v1")
  forM_ [ "arm-confirmed", "witness-cells", "before;guard=0", "watch-poll", "poll=0;"
        , "watch-settled", "ran-to-end;guard=1", "arm-auto-act-complete-stock-unverified"
        , "first;address=0x400c0800;tick=499;uptime-s=4.996;rate-hz=99.88"
        , "end;address=0x400c0800;tick=9487;uptime-s=94.984;rate-hz=99.88" ] $ \needle ->
    check ("happy journal carries "++needle) (needle `isInfixOf` happyJournal)
  outgoing <- readFile (base </> "happy" </> "outgoing.v1")
  check "the arm frame is journaled before it is sent" (P.hexBytes (writeFrame armIndex 2) `isInfixOf` outgoing)
  check "the gate restore frame is journaled before it is sent" (P.hexBytes (writeFrame gateIndex 0) `isInfixOf` outgoing)

  -- Every byte the session emitted, audited over its own packets.
  let controlBytes = [bytes | P.ControlWrite _ _ _ _ bytes <- packets]
      bulkFrames = [bytes | P.BulkOut _ bytes <- packets]
      writeFrames = [frame | frame <- bulkFrames, word32At 0 frame == configReadOpcode, word32At 0x10 frame == 3]
      readFrames = [frame | frame <- bulkFrames, word32At 0 frame == configReadOpcode, word32At 0x10 frame == 2]
      ramFrames = [frame | frame <- bulkFrames, word32At 0 frame == ramReadOpcode]
  check "every control frame is twelve bytes" (all (\bytes -> length bytes == 12) controlBytes)
  check "control frames use only the audited opcodes"
    (all (\bytes -> take 1 bytes `elem` [[0xc0],[0xd0],[0xd1],[0xc4]]) controlBytes)
  check "one D1 control per request carrying the request's reply length"
    (length [() | P.ControlWrite _ _ _ _ (0xd1:_) <- packets] == length readFrames + length writeFrames + length ramFrames)
  check "the session emits three config writes, gate enable then arm then gate restore"
    (map (\frame -> (word32At 0x1c frame, last frame)) writeFrames == [(gateIndex,1),(armIndex,2),(gateIndex,0)])
  check "every config write frame is the 37-byte length-1 form"
    (all (\frame -> length frame == configWriteBytes && word32At 0x14 frame == 1
                    && word32At 0x18 frame == 0 && word32At 0x20 frame == 1) writeFrames)
  check "no config write starts at index 0x80" (all (\frame -> word32At 0x1c frame /= 0x80) writeFrames)
  check "every config write index is one this command owns"
    (all (\frame -> word32At 0x1c frame `elem` writeTargetIndexes) writeFrames)
  check "the arm frame is exactly the reviewed bytes"
    (P.hexBytes (writeFrame armIndex 2) == "02001a00000000000000000000000000030000000100000000000000a20000000100000002")
  check "every config read is page 0 offset 0 length 256"
    (all (\frame -> word32At 0x18 frame == 0 && word32At 0x1c frame == 0 && word32At 0x20 frame == readWindowLength) readFrames)
  check "every RAM frame is function 2 at a witness half or the boot tick with 128 bytes"
    (all (\frame -> word32At 0x10 frame == 2 && word32At 0x20 frame == ramHalfBytes
                    && word32At 0x1c frame `elem` [0x01667000,0x01667080,0x01677700,0x01677780,0x400c0800]) ramFrames)
  check "no emitted frame carries any other opcode"
    (all (\frame -> word32At 0 frame `elem` [configReadOpcode,ramReadOpcode]) bulkFrames)
  check "the happy session reads two watch requests, two before windows and two after windows"
    (length ramFrames == 2*2 + 4 + 4 + 2)

  -- A failed file operation before END: the guard set, the statement area
  -- empty and a nonzero status.
  (failed,failedPackets,failedEvents) <- runCase (base </> "failed") failedSteps
  check "a nonzero file status before END reports load-failed"
    (either (const False) ((==ArmLoadFailed) . reportOutcome) failed)
  check "a failed load leaves the statement index short of END"
    (either (const False) ((/= -1) . witnessStatementIndex . reportWitness) failed)
  check "load-failed releases every host resource once" (failedEvents == ["released","closed","exited"])
  check "load-failed still captures both after windows"
    (length [() | P.BulkOut _ frame <- failedPackets, word32At 0 frame == ramReadOpcode] == 2*2 + 4 + 4 + 2)

  -- A completed script whose own file operation failed: END reached with a
  -- nonzero status, the sixth outcome.
  (fileOp,fileOpPackets,fileOpEvents) <- runCase (base </> "file-op-failed") fileOpFailedSteps
  case fileOp of
    Left e -> error ("file-op-failed session stopped: "++displayException e)
    Right report -> do
      check "a completed script with a failed file operation reports file-op-failed"
        (reportOutcome report == ArmFileOpFailed)
      check "file-op-failed still shows the statement index at END"
        (witnessStatementIndex (reportWitness report) == -1)
      check "file-op-failed still shows the load's size"
        (witnessInputLength (reportWitness report) == 13)
  check "file-op-failed releases every host resource once" (fileOpEvents == ["released","closed","exited"])
  check "file-op-failed still captures both after windows"
    (length [() | P.BulkOut _ frame <- fileOpPackets, word32At 0 frame == ramReadOpcode] == 2*2 + 4 + 4 + 2)

  -- A script still executing at one poll and ended at the next: the watch
  -- keeps polling until the statement index reaches END.
  (running,_runningPackets,runningEvents) <- runCase (base </> "running") runningSteps
  case running of
    Left e -> error ("running session stopped: "++displayException e)
    Right report -> do
      check "a running script keeps the watch polling" (reportPolls report == 3)
      check "the running-then-ended script reports ran-to-end" (reportOutcome report == ArmRanToEnd)
  check "the running-then-ended session releases every host resource once" (runningEvents == ["released","closed","exited"])

  -- The guard never sets: the watch ends on its cap and the run reports
  -- no-send with the boot-state witnesses. The 90 polls cost about 89 s.
  (noSend,noSendPackets,noSendEvents) <- runCase (base </> "no-send") noSendSteps
  case noSend of
    Left e -> error ("no-send session stopped: "++displayException e)
    Right report -> do
      check "no-send outcome is no-send" (reportOutcome report == ArmNoSend)
      check "no-send did not settle" (not (reportSettled report))
      check "no-send watch ran the full cap" (reportPolls report == watchPollCap)
      check "no-send reports the boot-state guard" (witnessGuard (reportWitness report) == 0)
  check "no-send releases every host resource once" (noSendEvents == ["released","closed","exited"])
  check "the cap bounds the watch"
    (length [() | P.BulkOut _ frame <- noSendPackets, word32At 0 frame == ramReadOpcode] == 2 + 2*watchPollCap + 4 + 4)

  inventory <- newIORef [[],[],[testDevice]]
  let nextInventory = atomicModifyIORef' inventory $ \lists -> case lists of
        [] -> ([],[testDevice])
        devices:rest -> (rest,devices)
  (waited,waitedPackets,waitedCleanup) <- runDeviceCase WaitForDevice (Clock (pure 0)) nextInventory (base </> "wait-appear") happySteps
  check "a device appearing after empty inventories emits the reviewed session"
    (either (const False) ((== ArmRanToEnd) . reportOutcome) waited && waitedPackets == packets)
  check "waiting releases every acquired resource" (waitedCleanup == ["released","closed","exited"])
  waitJournal <- readFile (base </> "wait-appear" </> "journal.v1")
  check "waiting and its bound are journaled" ("waiting-for-device" `isInfixOf` waitJournal && "bound-ms=120000" `isInfixOf` waitJournal)
  waitTime <- newIORef 0
  (expired,expiredPackets,expiredCleanup) <- runDeviceCase WaitForDevice (Clock (readIORef waitTime))
    (writeIORef waitTime 120000000000 >> pure []) (base </> "wait-expired") []
  check "the overall wait bound refuses before camera requests"
    (either (const True) (const False) expired && null expiredPackets && expiredCleanup == ["exited"])
  forM_ [ ("multiple", [testDevice,testDevice])
        , ("unsupported", [testDevice { descriptors = testDescriptors { interfaceNumber = 1 } }])
        , ("inventory-overflow", replicate 65 testDevice) ] $ \(label,devices) -> do
    (refused,refusedPackets,refusedCleanup) <- runDeviceCase WaitForDevice (Clock (pure 0)) (pure devices) (base </> label) []
    check (label++": waiting preserves the selection refusal")
      (either (const True) (const False) refused && null refusedPackets && refusedCleanup == ["exited"])
  (absent,absentPackets,absentCleanup) <- runDeviceCase RequirePresent (Clock (pure 0)) (pure []) (base </> "no-wait-absent") []
  check "the default mode still refuses an absent camera"
    (either (const True) (const False) absent && null absentPackets && absentCleanup == ["exited"])
  let badTick = map corruptTickReply (tickSteps 499)
      corruptTickReply (P.Step request (P.Reply bytes)) =
        P.Step request (P.Reply (take 0x1c bytes ++ word32le 0x400c0804 ++ drop 0x20 bytes))
      corruptTickReply step = step
      firstTickFailure = openSteps ++ pageSteps zeros ++ writeSteps gateIndex 1 ++ pageSteps (pageBytes 0 1)
        ++ badTick ++ cleanupSteps 0
      endTickFailure = armPrefix zeros ++ pollSteps (windowA zeros) (windowBFired zeros)
        ++ windowSteps 0x01667000 (windowA zeros) ++ windowSteps 0x01677700 (windowBFired zeros)
        ++ badTick ++ cleanupSteps 2
  forM_ [("first-tick-refused",firstTickFailure,False),("end-tick-refused",endTickFailure,True)] $ \(label,steps,armed) -> do
    (refused,refusedPackets,refusedCleanup) <- runCase (base </> label) steps
    check (label++": refuses the wrong tick echo and restores the gate")
      (either (const True) (const False) refused
        && [frame | P.BulkOut _ frame <- refusedPackets, word32At 0x10 frame == 3]
          == [writeFrame gateIndex 1] ++ (if armed then [writeFrame armIndex 2] else []) ++ [writeFrame gateIndex 0]
        && refusedCleanup == ["released","closed","exited"])

  -- Refusals: each one stops the session, closes the protocol once, releases
  -- the host resources and leaves a durable record.
  (tailOutcome,tailPackets,tailEvents) <- runCase (base </> "bad-open")
    ([ P.Step (P.controlTransfer (P.control 0xc0 8 0)) (P.Transferred 12)
     , P.Step (P.BulkIn 0x81 8) (P.Reply [0x34,0x12,0,0,0,0,0,0]) ] ++ closeSteps)
  check "a C0 reply without the constant tail stops before any D0"
    (either (isInfixOf "C0 reply tail" . displayException) (const False) tailOutcome
      && tailPackets == [P.controlTransfer (P.control 0xc0 8 0), P.BulkIn 0x81 8])
  check "the refused open releases every host resource once" (tailEvents == ["released","closed","exited"])

  let stopCases =
        [ ("arm byte is not the boot value", "reads 0x01 at session start", 0
          , [openSteps, pageSteps (pageBytes 1 0), closeSteps])
        , ("gate byte is not the captured value", "reads 0x01 at session start", 0
          , [openSteps, pageSteps (pageBytes 0 1), closeSteps])
        , ("witness cells are not the boot state", "not the boot state before the arm", 2
          , [ openSteps, pageSteps (pageBytes 0 0), writeSteps gateIndex 1, pageSteps (pageBytes 0 1)
            , tickSteps 499
            , windowSteps 0x01667000 (windowAGuard zeros), windowSteps 0x01677700 zeros
            ] ++ [cleanupSteps 0])
        , ("gate enable is not visible", "gate enable is not visible", 2
          , [ openSteps, pageSteps (pageBytes 0 0), writeSteps gateIndex 1, pageSteps (pageBytes 0 0)
            ] ++ [cleanupSteps 0])
        , ("arm write echo differs", "differs from the request", 3
          , [ openSteps, pageSteps (pageBytes 0 0), writeSteps gateIndex 1, pageSteps (pageBytes 0 1)
            , tickSteps 499
            , windowSteps 0x01667000 zeros, windowSteps 0x01677700 zeros
            , [ P.Step (P.controlTransfer (P.control 0xd0 configWriteBytes sessionSid)) (P.Transferred 12)
              , P.Step (P.BulkOut 2 (writeFrame armIndex 2)) (P.Transferred configWriteBytes)
              , P.Step (P.controlTransfer (P.control 0xd1 configReplyBytes sessionSid)) (P.Transferred 12)
              , P.Step (P.BulkIn 0x81 configReplyBytes) (P.Reply (writeReply 0xa3)) ]
            ] ++ [cleanupSteps 0])
        , ("arm write is rejected", "status", 3
          , [ openSteps, pageSteps (pageBytes 0 0), writeSteps gateIndex 1, pageSteps (pageBytes 0 1)
            , tickSteps 499
            , windowSteps 0x01667000 zeros, windowSteps 0x01677700 zeros
            , [ P.Step (P.controlTransfer (P.control 0xd0 configWriteBytes sessionSid)) (P.Transferred 12)
              , P.Step (P.BulkOut 2 (writeFrame armIndex 2)) (P.Transferred configWriteBytes)
              , P.Step (P.controlTransfer (P.control 0xd1 configReplyBytes sessionSid)) (P.Transferred 12)
              , P.Step (P.BulkIn 0x81 configReplyBytes) (P.Reply (rejectedWriteReply armIndex)) ]
            ] ++ [cleanupSteps 0])
        , ("arm readback is not visible", "arm write is not visible", 3
          , [ openSteps, pageSteps (pageBytes 0 0), writeSteps gateIndex 1, pageSteps (pageBytes 0 1)
            , tickSteps 499
            , windowSteps 0x01667000 zeros, windowSteps 0x01677700 zeros
            , writeSteps armIndex 2, pageSteps (pageBytes 0 1)
            ] ++ [cleanupSteps 0]) ]
  forM_ (zip [(0::Int)..] stopCases) $ \(position,(label,fragment,expectedWrites,steps)) -> do
    let dir = base </> ("stop-"++show position)
    (outcome,emitted,stopEvents) <- runCase dir (concat steps)
    case outcome of
      Right _ -> error (label++": the session must stop")
      Left e -> do
        check (label++": stop message") (fragment `isInfixOf` displayException e)
        check (label++": the cleanup reports no failure") (not ("cleanup uncertain" `isInfixOf` displayException e))
    check (label++": exactly one protocol close") (length [() | P.ControlWrite _ _ _ _ (0xc4:_) <- emitted] == 1)
    check (label++": every host resource released once") (stopEvents == ["released","closed","exited"])
    journal <- readFile (dir </> "journal.v1")
    check (label++": refusal or precondition is durable")
      (("arm-auto-act-refused" `isInfixOf` journal) || ("arm-auto-act-precondition" `isInfixOf` journal))
    check (label++": the session stops at its scripted write")
      (length [() | P.BulkOut _ frame <- emitted, word32At 0x10 frame == 3] == expectedWrites)

  -- The boundary: the index set is the two the command owns, and 0x80 is
  -- refused by name.
  check "mkWriteTarget accepts only the two owned indices"
    (map (mkWriteTarget . writeTargetIndex) [ArmByte,GateByte] == [Right ArmByte, Right GateByte])
  check "mkWriteTarget refuses 0x80 by name"
    (case mkWriteTarget 0x80 of
       Left reason -> "0x80" `isInfixOf` reason && "whole-area commit" `isInfixOf` reason
       Right _ -> False)
  forM_ [0x00, 0x21, 0x22, 0x79, 0x81, 0x8a, 0xa1, 0xa3, 0xf6, 0xf8, 0x100, -1] $ \index ->
    check ("mkWriteTarget refuses index "++show index) (either (const True) (const False) (mkWriteTarget index))
  check "the resolved write plan is the two owned targets"
    (armWrites == Right (ArmWrites ArmByte GateByte))

  -- The decode contract.
  check "the exact write reply decodes"
    (decodeConfigWriteReply ArmByte (0,0) (writeReply armIndex) == Right ()
      && decodeConfigWriteReply GateByte (0,0) (writeReply gateIndex) == Right ())
  forM_ [ writeReply 0xa3
        , rejectedWriteReply armIndex
        , take (configReplyBytes-1) (writeReply armIndex)
        , word32le 0x00200002 ++ drop 4 (writeReply armIndex) -- a different opcode word
        , pageReply (pageBytes 2 1) ] $ \bytes ->
    check "a deviating write reply is refused" (either (const True) (const False) (decodeConfigWriteReply ArmByte (0,0) bytes))
  check "a wrong status pair is refused"
    (either (const True) (const False) (decodeConfigWriteReply ArmByte (1,0xffff) (writeReply armIndex)))

  -- The witness extraction and the outcome table.
  let ended = witnessOf (windowA zeros) (windowBFired zeros)
      fileOpWitness = witnessOf (windowA zeros) (windowBFileOpFailed zeros)
      failedWitness = witnessOf (windowAGuard zeros) (windowBFailed zeros)
      runningWitness = witnessOf (windowARunning zeros) (windowBRunning zeros)
      noSendWitness = witnessOf zeros zeros
      noLoadWitness = witnessOf (windowAGuard zeros) zeros
  check "the witness extraction reads every cell"
    (witnessGuard ended == 1 && witnessSentinel ended == -1 && witnessStatementIndex ended == -1
      && witnessStatementCount ended == 3 && witnessStatementCode ended == 0x18
      && witnessStatementText ended == "WT_LOG" && witnessInFlight ended == 0
      && witnessStatus ended == 0 && witnessOpenResult ended == 0
      && witnessCompletionCount ended == logWriteBytes
      && witnessInputLength ended == 13 && witnessSelector ended == 1)
  check "the baseline check accepts the boot state" (baselineClear noSendWitness)
  check "the baseline check refuses a set guard" (not (baselineClear noLoadWitness))
  check "the settle predicate accepts the ended state" (witnessSettled ended)
  check "the settle predicate accepts a completed script whose file operation failed"
    (witnessSettled fileOpWitness)
  check "the settle predicate accepts a failed load" (witnessSettled failedWitness)
  check "the settle predicate refuses a running loader" (not (witnessSettled runningWitness))
  check "the settle predicate refuses the boot state" (not (witnessSettled noSendWitness))
  check "the outcome table"
    (map deriveOutcome [ended, fileOpWitness, failedWitness, runningWitness, noSendWitness, noLoadWitness]
      == [ArmRanToEnd, ArmFileOpFailed, ArmLoadFailed, ArmLoadRunning, ArmNoSend, ArmNoLoad])
  check "a nonzero status at END is not a failed load"
    (deriveOutcome fileOpWitness == ArmFileOpFailed && witnessStatementIndex fileOpWitness == -1)
  check "the outcome names are the printed tokens"
    (map armOutcomeName [ArmNoSend, ArmLoadFailed, ArmLoadRunning, ArmRanToEnd, ArmFileOpFailed, ArmNoLoad]
      == ["no-send","load-failed","load-running","ran-to-end","file-op-failed","no-load"])

  -- The documented sizes.
  check "the watch is ninety polls of one second" (watchPollCap == 90 && watchSpacingMs == 1000)
  check "the arm value and the gate values are the reviewed bytes"
    (armValueByte == 2 && gateEnabledByte == 1 && gateDisabledByte == 0)
  check "the index constants are the two owned indices" (writeTargetIndexes == [armIndex,gateIndex])

  removeDirectoryRecursive base
  putStrLn "arm-auto-act: frame, watch, outcome, refusal and decode checks passed"
