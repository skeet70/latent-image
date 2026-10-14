module Main where

import Control.Exception
import Control.Monad (forM_, unless)
import Data.List (isInfixOf, sort)
import qualified Data.ByteString as B
import Data.IORef
import Data.Word (Word8)
import System.Directory
import System.FilePath
import System.Posix.Process (getProcessID)
import FujiAF.Transport
import FujiAF.Read
import FujiAF.Probe
import FujiAF.Straps
import qualified FujiAF.Broker as P

check :: String -> Bool -> IO ()
check label yes = unless yes (error label)

syntheticDescriptors :: Descriptors
syntheticDescriptors = Descriptors 1 0 0 255 0 0 0x81 2 512 512 0 1 []

sessionSid :: Int
sessionSid = 0x117c

-- The capture contract from the safety analysis, written out here rather than
-- taken from the operation: seven reads, the two MMIO registers last.
expectedReads :: [(Int,Int)]
expectedReads =
  [ (0x400a0000,64)
  , (0x80010000,128), (0x80010080,128)
  , (0x80100000,128), (0x80100080,128)
  , (0xfff89014,4), (0xfff98000,4) ]

payloadFor :: Int -> Int -> [Word8]
payloadFor address size = replicate size (fromIntegral (address `mod` 251))

strapSteps :: (StrapRequest -> P.Outcome) -> [P.Step]
strapSteps outcomeFor = openSteps ++ concatMap step strapRequests
  where
    step action = case action of
      StrapClose ->
        [ P.Step (P.controlTransfer (P.control 0xc4 2 sessionSid)) (P.Transferred 12)
        , P.Step (P.BulkIn 0x81 2) (outcomeFor action) ]
      StrapGateWrite value ->
        [ P.Step (P.controlTransfer (P.control 0xd0 configWriteBytes sessionSid)) (P.Transferred 12)
        , P.Step (P.BulkOut 2 (configWriteFrame value)) (P.Transferred configWriteBytes)
        , P.Step (P.controlTransfer (P.control 0xd1 configReplyBytes sessionSid)) (P.Transferred 12)
        , P.Step (P.BulkIn 0x81 configReplyBytes) (outcomeFor action) ]
      StrapGateRead _ ->
        [ P.Step (P.controlTransfer (P.control 0xd0 readRequestBytes sessionSid)) (P.Transferred 12)
        , P.Step (P.BulkOut 2 (readRequestFrame readbackWindow)) (P.Transferred readRequestBytes)
        , P.Step (P.controlTransfer (P.control 0xd1 readReplyBytes sessionSid)) (P.Transferred 12)
        , P.Step (P.BulkIn 0x81 readReplyBytes) (outcomeFor action) ]
      StrapRead address size ->
        [ P.Step (P.controlTransfer (P.control 0xd0 debugRequestBytes sessionSid)) (P.Transferred 12)
        , P.Step (P.BulkOut 2 (strapRequestFrame address size)) (P.Transferred debugRequestBytes)
        , P.Step (P.controlTransfer (P.control 0xd1 (strapReplyBytes size) sessionSid)) (P.Transferred 12)
        , P.Step (P.BulkIn 0x81 (strapReplyBytes size)) (outcomeFor action) ]

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

configReadReply :: Word8 -> [Word8]
configReadReply value = concat
  [ word32le readOpcode, word32le 0, word32le 0, word32le 0
  , word32le 2, word32le 1, word32le 0, word32le 0, word32le 256 ]
  ++ readbackPayload value

readbackPayload :: Word8 -> [Word8]
readbackPayload value = take gateIndex (replicate 256 0x5a) ++ [value] ++ drop (gateIndex+1) (replicate 256 0x5a)

debugReply :: Int -> Int -> [Word8] -> [Word8]
debugReply address size payload = concat
  [ word32le debugOpcode, word32le 0, word32le 0, word32le 0
  , word32le 2, word32le 0, word32le 0
  , word32le address, word32le size ]
  ++ payload

debugReplyWith :: Int -> Int -> Int -> Int -> Int -> [Word8] -> [Word8]
debugReplyWith opcode err1 err2 address size payload = concat
  [ word32le opcode, word32le err1, word32le err2, word32le 0
  , word32le 2, word32le 0, word32le 0
  , word32le address, word32le size ]
  ++ payload

happyOutcome :: StrapRequest -> P.Outcome
happyOutcome action = case action of
  StrapClose -> P.Reply [0x7c,0x11]
  StrapGateWrite _ -> P.Reply configWriteReply
  StrapGateRead label -> P.Reply (configReadReply (if label == gateReadbackEnabled then 1 else 0))
  StrapRead address size -> P.Reply (debugReply address size (payloadFor address size))

mutate :: StrapRequest -> (P.Outcome -> P.Outcome) -> StrapRequest -> P.Outcome
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
  outcome <- try (runProbeStraps (Clock (pure 0)) api dir)
  packets <- readIORef emitted
  cleanupEvents <- readIORef events
  pure (outcome,packets,cleanupEvents)

isStrapBulkOut :: P.Transfer -> Bool
isStrapBulkOut (P.BulkOut _ frame) = word32At 0 frame == debugOpcode
isStrapBulkOut _ = False

readPairs :: [P.Transfer] -> [(Int,Int)]
readPairs packets =
  [ (word32At 0x1c frame, word32At 0x20 frame)
  | P.BulkOut _ frame <- packets, word32At 0 frame == debugOpcode ]

controlLength :: [Word8] -> Int
controlLength bytes = 256*fromIntegral (bytes!!8) + fromIntegral (bytes!!9)

expectStop :: FilePath -> (String,String) -> Int -> [P.Step] -> IO ()
expectStop dir (label,fragment) expectedReadsSent steps = do
  (outcome,packets,cleanupEvents) <- runCase dir steps
  let readsSent = length (readPairs packets)
  check (label++": session must stop") (either (const True) (const False) outcome)
  check (label++": stop message") (either (isInfixOf fragment . displayException) (const False) outcome)
  check (label++": "++show readsSent++" reads sent, expected "++show expectedReadsSent) (readsSent == expectedReadsSent)
  check (label++": exactly one protocol close") (length [() | P.ControlWrite _ _ _ _ (0xc4:_) <- packets] == 1)
  check (label++": every host resource released once") (cleanupEvents == [Released,Closed,Exited])
  journal <- readFile (dir </> "journal.v1")
  check (label++": refusal or uncertainty is durable") (("strap-refused" `isInfixOf` journal) || ("session-uncertain" `isInfixOf` journal))

main :: IO ()
main = do
  pid <- getProcessID
  tmp <- getTemporaryDirectory
  let base = tmp </> ("fuji-af-straps-tests-"++show pid)
  createDirectory base

  let happySteps = strapSteps happyOutcome
  (happy,packets,cleanupEvents) <- runCase (base </> "happy") happySteps
  let total = sum (map snd expectedReads)
  check ("happy session captures "++show total++" bytes: "++show happy) (either (const False) (==total) happy)
  check "happy session releases every resource once" (cleanupEvents == [Released,Closed,Exited])

  -- Every read the session emitted, address and length, in order.
  check "the emitted reads are exactly the fixed capture list" (readPairs packets == expectedReads)
  check "the two MMIO registers are read last" (drop (length expectedReads - 2) (readPairs packets) == [(0xfff89014,4),(0xfff98000,4)])
  check "the mirror is read first" (take 1 (readPairs packets) == [(0x400a0000,64)])
  check "no read asks for more than 128 bytes and none covers a 256-byte MMIO window"
    (all (\(address,size) -> size <= 128 && (size <= 4 || address < 0xfff89000)) (readPairs packets))

  let controlBytes = [bytes | P.ControlWrite _ _ _ _ bytes <- packets]
      bulkFrames = [bytes | P.BulkOut _ bytes <- packets]
      writeFrames = [frame | frame <- bulkFrames, word32At 0 frame == readOpcode && word32At 0x10 frame == 3]
      readbackFrames = [frame | frame <- bulkFrames, word32At 0 frame == readOpcode && word32At 0x10 frame == 2]
      strapFrames = [frame | frame <- bulkFrames, word32At 0 frame == debugOpcode]
  check "every emitted control frame is twelve bytes" (all (\bytes -> length bytes == 12) controlBytes)
  check "control writes use only the audited session opcodes"
    (all (\bytes -> take 1 bytes `elem` [[0xc0],[0xd0],[0xd1],[0xc4]]) controlBytes)
  check "every D1 length is the reply size its handler needs"
    ([controlLength bytes | bytes <- controlBytes, take 1 bytes == [0xd1]]
      == [configReplyBytes, readReplyBytes]
         ++ map (strapReplyBytes . snd) expectedReads
         ++ [configReplyBytes, readReplyBytes])
  check "every emitted bulk frame has its declared length"
    (map length writeFrames == replicate 2 configWriteBytes
      && map length readbackFrames == replicate 2 readRequestBytes
      && map length strapFrames == replicate (length expectedReads) debugRequestBytes)
  check "every strap frame carries function 2 and its read pair"
    (map (word32At 0x10) strapFrames == replicate (length expectedReads) 2
      && map (\frame -> (word32At 0x1c frame, word32At 0x20 frame)) strapFrames == expectedReads)
  check "config writes carry function 3, mailbox 1 and index 0xf7, and never index 0x80"
    (map (word32At 0x10) writeFrames == replicate 2 3
      && map (word32At 0x14) writeFrames == replicate 2 1
      && all (\frame -> 256*word32At 0x18 frame + word32At 0x1c frame == gateIndex) writeFrames
      && map (word32At 0x20) writeFrames == replicate 2 configWriteLength
      && all (\frame -> 256*word32At 0x18 frame + word32At 0x1c frame /= 0x80) (writeFrames++readbackFrames))
  check "the emitted gate values are 1 then 0"
    ([last frame | frame <- writeFrames] == [fromIntegral gateEnabled, fromIntegral gateDisabled])
  check "the config readbacks read index 0"
    (all (\frame -> 256*word32At 0x18 frame + word32At 0x1c frame == 0) readbackFrames)

  -- The captured files: one per target plus the two gate readbacks.
  mirror <- B.readFile (base </> "happy" </> "windows" </> "400a0000.bin")
  check "the mirror file is the 64 bytes the device sent" (B.unpack mirror == payloadFor 0x400a0000 64)
  romFirst <- B.readFile (base </> "happy" </> "windows" </> "80010000.bin")
  check "the first alias file is its two 128-byte halves in order"
    (B.unpack romFirst == payloadFor 0x80010000 128 ++ payloadFor 0x80010080 128)
  romSecond <- B.readFile (base </> "happy" </> "windows" </> "80100000.bin")
  check "the second alias file is its two 128-byte halves in order"
    (B.unpack romSecond == payloadFor 0x80100000 128 ++ payloadFor 0x80100080 128)
  high <- B.readFile (base </> "happy" </> "windows" </> "fff89014.bin")
  low <- B.readFile (base </> "happy" </> "windows" </> "fff98000.bin")
  check "the two strap files are four bytes each" (B.unpack high == payloadFor 0xfff89014 4 && B.unpack low == payloadFor 0xfff98000 4)
  enabled <- B.readFile (base </> "happy" </> "windows" </> (gateReadbackEnabled++".bin"))
  restored <- B.readFile (base </> "happy" </> "windows" </> (gateReadbackRestored++".bin"))
  check "enable readback window" (B.unpack enabled == readbackPayload 1)
  check "restore readback window" (B.unpack restored == readbackPayload 0)
  files <- listDirectory (base </> "happy" </> "windows")
  check ("five target files and two readbacks, saw "++show (length files)) (length files == 7)

  happyJournal <- readFile (base </> "happy" </> "journal.v1")
  check "one completion record per target and one session record"
    (length (filter ("target-complete" `isInfixOf`) (lines happyJournal)) == length strapTargets
      && length (filter ("strap-read-complete-stock-unverified" `isInfixOf`) (lines happyJournal)) == 1)
  outgoingLog <- readFile (base </> "happy" </> "outgoing.v1")
  check "the enable frame is durable before dispatch" (P.hexBytes (configWriteFrame gateEnabled) `isInfixOf` outgoingLog)
  check "the restore frame is durable before dispatch" (P.hexBytes (configWriteFrame gateDisabled) `isInfixOf` outgoingLog)
  check "each strap frame is durable before dispatch"
    (all (\pair -> P.hexBytes (strapRequestFrame (fst pair) (snd pair)) `isInfixOf` outgoingLog) expectedReads)

  -- Stop conditions, each against the exact byte that must cause it.
  let firstRom = (0x80010000,128)
      firstStrap = (0xfff89014,4)
      stopCases =
        [ ("gate enable is not visible", "gate enable is not visible", 0, strapSteps (mutate (StrapGateRead gateReadbackEnabled) (const (P.Reply (configReadReply 0)))))
        , ("readback refused", "rejected arguments", 0, strapSteps (mutate (StrapGateRead gateReadbackEnabled) (const (P.Reply (concat [word32le readOpcode, word32le 9, word32le 3, word32le 0, word32le 2, word32le 1, word32le 0, word32le 0, word32le 256] ++ replicate 256 0)))))
        , ("closed gate at the first alias", "closed gate", 2, strapSteps (mutate (StrapRead (fst firstRom) (snd firstRom)) (const (P.Reply (debugReplyWith debugOpcode 1 0xffff (fst firstRom) (snd firstRom) (replicate (snd firstRom) 0))))))
        , ("nonzero reserved word", "reply reserved word is nonzero", 2, strapSteps (mutate (StrapRead (fst firstRom) (snd firstRom)) (const (P.Reply (take 12 (debugReply (fst firstRom) (snd firstRom) (payloadFor (fst firstRom) (snd firstRom))) ++ word32le 1 ++ drop 16 (debugReply (fst firstRom) (snd firstRom) (payloadFor (fst firstRom) (snd firstRom))))))))
        , ("wrong opcode", "reply opcode differs", 2, strapSteps (mutate (StrapRead (fst firstRom) (snd firstRom)) (const (P.Reply (word32le 0x00200002 ++ drop 4 (debugReply (fst firstRom) (snd firstRom) (payloadFor (fst firstRom) (snd firstRom))))))))
        , ("unproducible status", "status the handler cannot produce", 2, strapSteps (mutate (StrapRead (fst firstRom) (snd firstRom)) (const (P.Reply (debugReplyWith debugOpcode 2 7 (fst firstRom) (snd firstRom) (replicate (snd firstRom) 0))))))
        , ("echoed address mismatch", "differs from the request", 2, strapSteps (mutate (StrapRead (fst firstRom) (snd firstRom)) (const (P.Reply (debugReply (fst firstRom + 4) (snd firstRom) (payloadFor (fst firstRom) (snd firstRom)))))))
        , ("short strap reply", "short/partial/oversized transfer", 6, strapSteps (mutate (StrapRead (fst firstStrap) (snd firstStrap)) (const (P.Reply (debugReply (fst firstStrap) (snd firstStrap) (replicate 3 0x11))))))
        , ("closed gate at the first strap read", "closed gate", 6, strapSteps (mutate (StrapRead (fst firstStrap) (snd firstStrap)) (const (P.Reply (debugReplyWith debugOpcode 1 0xffff (fst firstStrap) (snd firstStrap) (replicate (snd firstStrap) 0)))))) ]
  forM_ (zip [(0::Int)..] stopCases) $ \(index,(label,fragment,expected,steps)) ->
    expectStop (base </> ("stop-"++show index)) (label,fragment) expected steps

  -- A refusal at the first strap read keeps the RAM and ROM captures and
  -- writes no strap file.
  let strapRefusal = strapSteps (mutate (StrapRead (fst firstStrap) (snd firstStrap)) (const (P.Reply (debugReplyWith debugOpcode 1 0xffff (fst firstStrap) (snd firstStrap) (replicate (snd firstStrap) 0)))))
  _ <- runCase (base </> "strap-refused") strapRefusal
  kept <- listDirectory (base </> "strap-refused" </> "windows")
  check ("a refused strap read keeps the mirror, both aliases and the enable readback, saw "++show kept)
    (sort kept == sort [gateReadbackEnabled++".bin", "400a0000.bin", "80010000.bin", "80100000.bin"])

  -- The restore readback must show the restored value before C4.
  (restoreOutcome,restorePackets,restoreCleanup) <- runCase (base </> "restore-not-visible")
    (strapSteps (mutate (StrapGateRead gateReadbackRestored) (const (P.Reply (configReadReply 1)))))
  check "restore not visible stops the session"
    (either (isInfixOf "gate restore is not visible" . displayException) (const False) restoreOutcome)
  check "restore not visible keeps every capture and attempts one close"
    (readPairs restorePackets == expectedReads
      && length [() | P.ControlWrite _ _ _ _ (0xc4:_) <- restorePackets] == 1
      && restoreCleanup == [Released,Closed,Exited])
  restoreFiles <- listDirectory (base </> "restore-not-visible" </> "windows")
  check "restore not visible keeps every target file and both readbacks"
    (length restoreFiles == length strapTargets + 2)

  -- A transport failure mid-session leaves the uncertainty record.
  (lostOutcome,lostPackets,lostCleanup) <- runCase (base </> "lost-reply")
    (strapSteps (mutate (StrapRead 0x80100080 128) (const P.LostReply)))
  check "lost read reply stops the session"
    (either (isInfixOf "injected transport failure" . displayException) (const False) lostOutcome)
  check "lost read reply still attempts one close and releases everything"
    (length [() | P.ControlWrite _ _ _ _ (0xc4:_) <- lostPackets] == 1 && lostCleanup == [Released,Closed,Exited])

  removeDirectoryRecursive base
  putStrLn "probe-straps: frame-byte, gate, stop and capture checks passed"
