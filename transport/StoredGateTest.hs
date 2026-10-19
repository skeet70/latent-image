module Main where

import Control.Exception
import Control.Monad (forM_, unless, when)
import qualified Data.ByteString as B
import Data.IORef
import Data.List (isInfixOf)
import Data.Word (Word8, Word64)
import System.Directory
import System.FilePath
import System.Posix.Files (setFileMode)
import System.Posix.Process (getProcessID)
import FujiAF.Transport
import FujiAF.StoredGate
import FujiAF.SHA256 (sha256)
import qualified FujiAF.Broker as P

check :: String -> Bool -> IO ()
check label yes = unless yes (error label)

syntheticDescriptors :: Descriptors
syntheticDescriptors = Descriptors 1 0 0 255 0 0 0x81 2 512 512 0 1 []

le32 :: Int -> [Word8]
le32 value = [fromIntegral ((value `div` 256^index) `mod` 256) | index <- [0..3::Int]]

u32 :: Int -> [Word8] -> Int
u32 at bytes = sum [fromIntegral byte * 256^index | (index,byte) <- zip [0..3::Int] (take 4 (drop at bytes))]

-- Wire shapes written out from the documented layouts.
identityFrame :: [Word8]
identityFrame = [0,0,0x15,0] ++ replicate 12 0 ++ [4,0,0,0] ++ replicate 12 0

windowFrame :: Int -> [Word8]
windowFrame offset =
  [0x02,0x00,0x1a,0x00] ++ replicate 12 0
  ++ [0x02,0,0,0, 0x01,0,0,0] ++ le32 (offset `div` 256) ++ [0,0,0,0, 0x00,0x01,0,0]

writeFrame :: Int -> Word8 -> [Word8]
writeFrame index value =
  [0x02,0x00,0x1a,0x00] ++ replicate 12 0
  ++ [0x03,0,0,0, 0x01,0,0,0, 0,0,0,0] ++ le32 index ++ [0x01,0,0,0, value]

stateFrame :: [Word8]
stateFrame =
  [0x01,0x00,0x20,0x00] ++ replicate 12 0
  ++ [0x02,0,0,0, 0,0,0,0, 0,0,0,0, 0xb0,0x20,0x5c,0x01, 0x02,0,0,0]

data Req = ReqIdentity | ReqWindow Int | ReqWrite Int Word8 | ReqState | ReqClose | ReqOpen
  deriving (Eq, Show)

-- The trigger's D1 held for holdMs of session clock; the commit's result
-- lands at 0x81 while it is held, and the D1 then fails or completes.
data Hold = Hold { holdMs :: Int, holdResult :: Maybe Word8, holdFails :: Bool }

-- The synthetic body: window 0 as the cache's first page, the two oracle
-- windows, the group-2 state, and the 0x81 values an unheld commit presents
-- on successive window-0 reads.
data Fake = Fake
  { fakeWindow :: [Word8]
  , fakeReference :: [Word8]
  , fakeOracle :: [Word8]
  , fakeSignature :: [Word8]
  , fakeState :: Int
  , fakeResults :: [Word8]
  , fakeHold :: Maybe Hold
  , fakeTriggerStatus :: (Int,Int)
  , fakeOpenTail :: [Word8]
  , fakeIdentityText :: [Word8]
  , fakeSideEffect :: (Int,Word8) -> [(Int,Word8)]
  , fakeLost :: Req -> Int -> Bool
  }

windowWith :: [(Int,Word8)] -> [Word8]
windowWith patches = [maybe 0x5a id (lookup index patches) | index <- [0..255]]

oracleWindow, signatureWindow :: [Word8]
oracleWindow = replicate 0x5c 0x11 ++ [0x58,0x2d,0x4d,0x31,0x00] ++ replicate (256-0x5c-5) 0x11
signatureWindow = [0x11,0x11,0x55,0xaa] ++ replicate 252 0x11

atRest, enableStart, restoreStart :: [Word8]
atRest = windowWith [(0x80,0x00),(0x81,0x00),(0xa2,0x00),(0xf7,0x00)]
enableStart = atRest
restoreStart = windowWith [(0x80,0x22),(0x81,0xff),(0xa2,0x00),(0xf7,0x01)]

reference :: [Word8] -> ReferenceWindow
reference bytes = either error id (mkReferenceWindow (B.pack bytes))

fakeFor :: CommitDirection -> Fake
fakeFor direction = Fake
  { fakeWindow = if direction == EnableStoredGate then enableStart else restoreStart
  , fakeReference = atRest
  , fakeOracle = oracleWindow
  , fakeSignature = signatureWindow
  , fakeState = 1
  , fakeResults = [0xff,0xff,0x00]
  , fakeHold = Nothing
  , fakeTriggerStatus = (0,0)
  , fakeOpenTail = [0,0,0,0,1,0]
  , fakeIdentityText = [88,0,45,0,77,0,49,0]
  , fakeSideEffect = const []
  , fakeLost = \_ _ -> False
  }

replace :: Int -> Word8 -> [Word8] -> [Word8]
replace at value bytes = take at bytes ++ [value] ++ drop (at+1) bytes

data CleanupEvent = Released | Closed | Exited deriving (Eq, Show)

data Run = Run
  { runOutcome :: Either SomeException CommitOutcome
  , runPackets :: [P.Transfer]
  , runBudgets :: [(P.Transfer,Int)]
  , runCleanup :: [CleanupEvent]
  , runDurable :: Bool
  , runTimes :: [(P.Transfer,Word64)]
  }

-- The fake answers by request content, not by position, and records whether
-- each outgoing frame was in outgoing.v1 at its dispatch and the budget each
-- transfer was given.
runCase :: CommitDirection -> Fake -> FilePath -> IO Run
runCase direction fake dir = do
  createDirectoryIfMissing True dir
  cache <- newIORef (fakeWindow fake)
  triggered <- newIORef False
  results <- newIORef (fakeResults fake)
  pending <- newIORef ReqOpen
  seen <- newIORef ([] :: [Req])
  emitted <- newIORef []
  budgets <- newIORef []
  times <- newIORef []
  events <- newIORef []
  durable <- newIORef True
  clockRef <- newIORef (0 :: Word64)
  let device = Candidate "001:2.3" 0x04cb 0xff80 syntheticDescriptors
      recordDurable bytes = do
        outgoing <- readFile (dir </> "outgoing.v1")
        unless (P.hexBytes bytes `isInfixOf` outgoing) (writeIORef durable False)
      parse frame
        | frame == identityFrame = ReqIdentity
        | u32 0 frame == 0x1a0002 && u32 0x10 frame == 2 = ReqWindow (256 * u32 0x18 frame)
        | u32 0 frame == 0x1a0002 && u32 0x10 frame == 3 && length frame == 37 && u32 0x18 frame == 0 && u32 0x20 frame == 1 =
            ReqWrite (u32 0x1c frame) (last frame)
        | frame == stateFrame = ReqState
        | otherwise = error ("unexpected frame "++P.hexBytes frame)
      apply request = case request of
        ReqWrite index value -> do
          modifyIORef' cache (replace index value)
          when (index == 0x80) (writeIORef triggered (value == 0x22 && fakeTriggerStatus fake == (0,0)))
          forM_ (fakeSideEffect fake (index,value)) $ \(at,byte) -> modifyIORef' cache (replace at byte)
        ReqWindow 0 -> do
          running <- readIORef triggered
          when running $ readIORef results >>= \remainingResults -> case remainingResults of
            value:rest -> modifyIORef' cache (replace 0x81 value) >> writeIORef results rest
            [] -> pure ()
        _ -> pure ()
      reply request = case request of
        ReqOpen -> pure ([0x61,0x2b] ++ fakeOpenTail fake)
        ReqClose -> pure [0x61,0x2b]
        ReqIdentity -> pure ([0,0,0x15,0] ++ replicate 12 0 ++ fakeIdentityText fake ++ replicate (112 - length (fakeIdentityText fake)) 0)
        ReqWindow offset -> do
          window <- readIORef cache
          let payload = case offset of
                0 -> window
                0x100 -> fakeOracle fake
                0x200 -> fakeSignature fake
                _ -> error "unexpected window"
          pure (concatMap le32 [0x1a0002,0,0,0,2,1,offset `div` 256,0,256] ++ payload)
        ReqWrite index value ->
          let (first,second)
                | index == 0x80 && value == 0x33 = (1,0xffff)
                | index == 0x80 && value == 0x22 = fakeTriggerStatus fake
                | otherwise = (0,0)
          in pure (concatMap le32 [0x1a0002,first,second,0,3,1,0,index,1] ++ replicate 256 0)
        ReqState -> pure (concatMap le32 [0x200001,0,0,0,2,0,0,0x015c20b0,2] ++ take 2 (le32 (fakeState fake)))
      api = UsbApi (\_ -> pure ()) (\_ -> pure [device]) (\_ _ -> pure ()) (\_ _ -> pure ())
        (\packet ms -> do
          modifyIORef' emitted (++[packet])
          modifyIORef' budgets (++[(packet,ms)])
          readIORef clockRef >>= \now -> modifyIORef' times (++[(packet,now)])
          case packet of
            P.ControlWrite 0x43 0 0 0 bytes -> do
              recordDurable bytes
              case bytes of
                0xc0:_ -> writeIORef pending ReqOpen
                0xc4:_ -> writeIORef pending ReqClose
                0xd1:_ -> do
                  request <- readIORef pending
                  case fakeHold fake of
                    Just hold | request == ReqWrite 0x80 0x22 -> do
                      modifyIORef' clockRef (+ fromIntegral (holdMs hold) * 1000000)
                      forM_ (holdResult hold) $ \value -> modifyIORef' cache (replace 0x81 value)
                      when (holdFails hold) (P.reject "injected held reply failure")
                    _ -> pure ()
                _ -> pure ()
              pure (12,[])
            P.BulkOut _ frame -> do
              recordDurable frame
              let request = parse frame
              writeIORef pending request
              apply request
              pure (length frame,[])
            P.BulkIn _ count -> do
              request <- readIORef pending
              modifyIORef' seen (++[request])
              ordinal <- length . filter (== request) <$> readIORef seen
              when (fakeLost fake request ordinal) (P.reject "injected transport failure")
              bytes <- reply request
              check ("reply size for "++show request) (length bytes == count)
              pure (count,bytes)
            _ -> error "unexpected transfer")
        (\_ _ -> modifyIORef' events (++[Released]))
        (\_ -> modifyIORef' events (++[Closed]))
        (\_ -> modifyIORef' events (++[Exited]))
  let clock = TimedClock (readIORef clockRef) (\ms -> modifyIORef' clockRef (+ fromIntegral ms * 1000000))
  outcome <- try (runCommitF7 direction (reference (fakeReference fake)) clock api dir)
  Run outcome <$> readIORef emitted <*> readIORef budgets <*> readIORef events <*> readIORef durable <*> readIORef times

frames :: Run -> [[Word8]]
frames run = [frame | P.BulkOut _ frame <- runPackets run]

writes :: Run -> [(Int,Word8)]
writes run = [(u32 0x1c frame, last frame) | frame <- frames run, u32 0 frame == 0x1a0002, u32 0x10 frame == 3]

closes :: Run -> Int
closes run = length [() | P.ControlWrite _ _ _ _ (0xc4:_) <- runPackets run]

outcomeOf :: Run -> Maybe CommitOutcome
outcomeOf run = either (const Nothing) Just (runOutcome run)

isRefused :: Run -> Bool
isRefused run = case outcomeOf run of Just (Refused _) -> True; _ -> False

isUncertain :: CommitState -> Run -> Bool
isUncertain state run = case outcomeOf run of Just (Uncertain observed _) -> observed == state; _ -> False

reasonHas :: String -> Run -> Bool
reasonHas fragment run = case outcomeOf run of
  Just (Refused reason) -> fragment `isInfixOf` reason
  Just (Uncertain _ reason) -> fragment `isInfixOf` reason
  _ -> False

lineHas :: CommitDirection -> String -> Run -> Bool
lineHas direction fragment run = maybe False (isInfixOf fragment . commitOutcomeLine direction) (outcomeOf run)

cutPower, waitFirst :: String
cutPower = "end the session by cutting the dummy-battery supply"
waitFirst = "do not cut power"

allowedWrites :: CommitDirection -> [(Int,Word8)]
allowedWrites EnableStoredGate = [(0xf7,0x01),(0x81,0xff),(0x80,0x22),(0x80,0x33)]
allowedWrites RestoreStoredGate = [(0xf7,0x00),(0x81,0xff),(0x80,0x22),(0x80,0x33)]

-- Every run: every write frame is the 37-byte length-1 form, its pair is in
-- the direction's set (plus the enable refusal's 0xf7 = 0x00 where named),
-- each outgoing byte was durable before its dispatch, only the D1 after the
-- trigger frame is given more than 5000 ms, one C4 went out and every host
-- resource was released.
common :: String -> CommitDirection -> [(Int,Word8)] -> Run -> IO ()
common label direction extra run = do
  let budgets = runBudgets run
      heldD1 = [ position+1 | (position,(P.BulkOut _ frame,_)) <- zip [0::Int ..] budgets, frame == writeFrame 0x80 0x22 ]
  check (label++": every write frame is the 37-byte length-1 form")
    (all (\frame -> length frame == 37 && u32 0x14 frame == 1 && u32 0x18 frame == 0 && u32 0x20 frame == 1)
         [frame | frame <- frames run, u32 0 frame == 0x1a0002, u32 0x10 frame == 3])
  check (label++": only the direction's index/value pairs are written: "++show (writes run))
    (all (`elem` (allowedWrites direction ++ extra)) (writes run))
  check (label++": journal before send") (runDurable run)
  check (label++": only the D1 after the trigger frame gets 33000 ms "++show [ms | (_,ms) <- budgets, ms /= 5000])
    (and [ ms == (if position `elem` heldD1 then 33000 else 5000) | (position,(_,ms)) <- zip [0..] budgets ]
      && all (< length budgets) heldD1)
  check (label++": exactly one C4") (closes run == 1)
  check (label++": host resources released once") (runCleanup run == [Released,Closed,Exited])

successFrames :: CommitDirection -> Int -> [[Word8]]
successFrames direction polls =
  [identityFrame, windowFrame 0, windowFrame 0x100, windowFrame 0x200]
  ++ [stateFrame | direction == RestoreStoredGate]
  ++ [writeFrame 0xf7 target, windowFrame 0]
  ++ [stateFrame | direction == EnableStoredGate]
  ++ [writeFrame 0x81 0xff, windowFrame 0, writeFrame 0x80 0x22]
  ++ replicate polls (windowFrame 0)
  ++ [writeFrame 0x80 0x33, windowFrame 0]
  where target = if direction == EnableStoredGate then 0x01 else 0x00

main :: IO ()
main = do
  pid <- getProcessID
  tmp <- getTemporaryDirectory
  let base = tmp </> ("fuji-af-stored-gate-tests-"++show pid)
  createDirectory base

  -- The audited frames, byte for byte.
  check "enable gate frame"
    (commitWriteFrame (SetGate GateOpen) ==
      [0x02,0x00,0x1a,0x00, 0,0,0,0, 0,0,0,0, 0,0,0,0, 0x03,0,0,0, 0x01,0,0,0, 0,0,0,0, 0xf7,0,0,0, 0x01,0,0,0, 0x01])
  check "restore gate frame" (commitWriteFrame (SetGate GateClosed) == writeFrame 0xf7 0x00)
  check "arm frame" (commitWriteFrame ArmResult == writeFrame 0x81 0xff)
  check "trigger frame" (commitWriteFrame TriggerCommit == writeFrame 0x80 0x22)
  check "disarm frame" (commitWriteFrame DisarmCommit == writeFrame 0x80 0x33)
  check "allowance and poll bound from 33 units" (commitAllowanceMs == 33000 && commitPollCap == 330 && commitPollSpacingMs == 100)
  check "the held reply fits the transfer ceiling" (commitAllowanceMs <= transferCeilingMs)
  check "aggregate deadline bound" (commitAggregateDeadlineMs == 5000*(8+4*342) + 33000 + 100*330)
  check "journal bound" (commitJournalCapBytes == 65536 + 4096*342)

  -- The reference window file.
  check "a reference of the wrong length is refused"
    (either (const True) (const False) (mkReferenceWindow (B.pack (take 255 atRest)))
      && either (const True) (const False) (mkReferenceWindow (B.pack (atRest ++ [0]))))
  let referencePath = base </> "reference.bin"
  B.writeFile referencePath (B.pack atRest)
  setFileMode referencePath 0o600
  readBack <- readReferenceWindow referencePath
  check "a private 256-byte reference reads back with its digest"
    (referenceBytes readBack == atRest && referenceDigest readBack == sha256 (B.pack atRest))
  setFileMode referencePath 0o640
  groupReadable <- try (readReferenceWindow referencePath) :: IO (Either SomeException ReferenceWindow)
  check "a group-readable reference is refused"
    (either (isInfixOf "private owned reference window required" . displayException) (const False) groupReadable)
  relative <- try (readReferenceWindow "reference.bin") :: IO (Either SomeException ReferenceWindow)
  check "a relative reference path is refused"
    (either (isInfixOf "absolute reference window path required" . displayException) (const False) relative)

  forM_ [minBound .. maxBound] $ \direction -> do
    let name = directionName direction
        fake = fakeFor direction
        target = if direction == EnableStoredGate then 0x01 else 0x00
        full = [(0xf7,target),(0x81,0xff),(0x80,0x22),(0x80,0x33)]
        caseDir label = base </> (name++"-"++label)

    -- Success: the exact frames, the outcome and the captures.
    happy <- runCase direction fake (caseDir "happy")
    common (name++" happy") direction [] happy
    check (name++" happy: committed after three polls") (outcomeOf happy == Just (Committed 3))
    check (name++" happy: the line ends with the supply cut") (lineHas direction cutPower happy)
    check (name++" happy: exact frames") (frames happy == successFrames direction 3)
    check (name++" happy: emitted write set") (writes happy == full)
    startCapture <- B.readFile (caseDir "happy" </> "config-000000.bin")
    check (name++" happy: start window captured") (B.unpack startCapture == fakeWindow fake)
    disarmCapture <- B.readFile (caseDir "happy" </> "disarm-readback.bin")
    check (name++" happy: disarm readback captured")
      (B.unpack disarmCapture == replace 0x80 0x33 (replace 0x81 0x00 (replace 0xf7 target (fakeWindow fake))))
    journal <- readFile (caseDir "happy" </> "journal.v1")
    check (name++" happy: outcome journaled") ("commit-f7-outcome" `isInfixOf` journal && "committed;" `isInfixOf` journal)

    -- The held reply: the trigger's D1 returns after 20 s of commit, which
    -- the 5000 ms budget would cut off, and the first poll is terminal.
    held <- runCase direction fake { fakeResults = [], fakeHold = Just (Hold 20000 (Just 0x00) False) } (caseDir "held")
    common (name++" held") direction [] held
    check (name++" held: committed on the first poll") (outcomeOf held == Just (Committed 1) && writes held == full)
    check (name++" held: exact frames") (frames held == successFrames direction 1)

    -- Refusals before any write.
    let refusals =
          [ ("product", fake { fakeOracle = replace 0x5e 0x41 oracleWindow }, "X-M1")
          , ("signature", fake { fakeSignature = replace 3 0xab signatureWindow }, "55 aa")
          , ("start-f7", fake { fakeWindow = replace 0xf7 target (fakeWindow fake) }, "0xf7 reads")
          , ("start-f7-two", fake { fakeWindow = replace 0xf7 0x02 (fakeWindow fake) }, "0xf7 reads")
          , ("trigger-33", fake { fakeWindow = replace 0x80 0x33 (fakeWindow fake) }, "power-cycle first")
          , ("trigger-35", fake { fakeWindow = replace 0x80 0x35 (fakeWindow fake) }, "0x80 reads 0x35")
          , ("result-01", fake { fakeWindow = replace 0x81 0x01 (fakeWindow fake) }, "0x81 reads 0x01")
          , ("auto-act-arm", fake { fakeWindow = replace 0xa2 0x02 (fakeWindow fake), fakeReference = replace 0xa2 0x02 atRest }, "0xa2 reads 0x02")
          , ("residue", fake { fakeWindow = replace 0x10 0x00 (fakeWindow fake) }, "differs from the reference at 0x10;")
          , ("open-tail", fake { fakeOpenTail = [0,0,0,0,2,0] }, "C0 reply tail")
          , ("identity", fake { fakeIdentityText = [88,0,45,0,69,0,49,0] }, "X-M1")
          , ("lost-window", fake { fakeLost = \request _ -> request == ReqWindow 0x200 }, "injected transport failure") ]
          ++ [ ("state-4", fake { fakeState = 4 }, "state 4") | direction == RestoreStoredGate ]
    forM_ refusals $ \(label,variant,fragment) -> do
      run <- runCase direction variant (caseDir ("refuse-"++label))
      common (name++" refuse "++label) direction [] run
      check (name++" refuse "++label++": refused with reason "++show (outcomeOf run)) (isRefused run && reasonHas fragment run)
      check (name++" refuse "++label++": no write") (null (writes run))
      check (name++" refuse "++label++": the line ends with the supply cut") (lineHas direction cutPower run)

    -- Poll outcomes.
    failed <- runCase direction fake { fakeResults = [0xff,0x01] } (caseDir "result-01")
    common (name++" commit failure") direction [] failed
    check (name++" commit failure: reported with the disarm") (outcomeOf failed == Just (CommitFailed 2) && writes failed == full)
    garbage <- runCase direction fake { fakeResults = [0xff,0x07] } (caseDir "result-garbage")
    common (name++" garbage") direction [] garbage
    check (name++" garbage: uncertain, commit may be running, disarm attempted")
      (isUncertain CommitMayBeRunning garbage && reasonHas "0x07" garbage && writes garbage == full
        && lineHas direction waitFirst garbage)
    pollTimeout <- runCase direction fake { fakeResults = [] } (caseDir "poll-timeout")
    common (name++" poll timeout") direction [] pollTimeout
    check (name++" poll timeout: uncertain after the bound, disarm attempted")
      (isUncertain CommitMayBeRunning pollTimeout && reasonHas "never left 0xff" pollTimeout
        && writes pollTimeout == full && frames pollTimeout == successFrames direction 330
        && lineHas direction waitFirst pollTimeout)

    -- A held reply that fails after acceptance: the run waits out the
    -- allowance from the trigger frame, reads window 0 once, then disarms.
    let heldFailure result = fake { fakeResults = [], fakeHold = Just (Hold 1000 result True) }
        bulkTimes run = [(frame,at) | (P.BulkOut _ frame,at) <- runTimes run]
        waitedFromTrigger run = case dropWhile ((/= writeFrame 0x80 0x22) . fst) (bulkTimes run) of
          (_,sent):(_,next):_ -> next - sent >= 33000*1000000
          _ -> False
        afterTrigger run = drop 1 (dropWhile (/= writeFrame 0x80 0x22) (frames run))
    heldDone <- runCase direction (heldFailure (Just 0x00)) (caseDir "held-fails-done")
    common (name++" held fails, done") direction [] heldDone
    check (name++" held fails, done: uncertain and settled, supply cut allowed")
      (isUncertain CommitSettled heldDone && writes heldDone == full && lineHas direction cutPower heldDone)
    check (name++" held fails, done: one read, then the disarm")
      (afterTrigger heldDone == [windowFrame 0, writeFrame 0x80 0x33, windowFrame 0])
    check (name++" held fails, done: the read waits out the allowance from the trigger frame") (waitedFromTrigger heldDone)
    heldRunning <- runCase direction (heldFailure Nothing) (caseDir "held-fails-running")
    common (name++" held fails, running") direction [] heldRunning
    check (name++" held fails, running: uncertain, told not to cut power")
      (isUncertain CommitMayBeRunning heldRunning && writes heldRunning == full
        && lineHas direction waitFirst heldRunning && not (lineHas direction cutPower heldRunning))
    check (name++" held fails, running: one read, then the disarm")
      (afterTrigger heldRunning == [windowFrame 0, writeFrame 0x80 0x33, windowFrame 0] && waitedFromTrigger heldRunning)
    lostTrigger <- runCase direction fake { fakeResults = [], fakeHold = Just (Hold 20000 (Just 0x00) False)
                                          , fakeLost = \request _ -> request == ReqWrite 0x80 0x22 } (caseDir "lost-trigger")
    common (name++" lost trigger reply") direction [] lostTrigger
    check (name++" lost trigger reply: waited, read, one disarm")
      (isUncertain CommitSettled lostTrigger && writes lostTrigger == full
        && afterTrigger lostTrigger == [windowFrame 0, writeFrame 0x80 0x33, windowFrame 0] && waitedFromTrigger lostTrigger)

    -- A trigger answered with a status other than (0,0): no commit was
    -- posted, so no wait, one disarm, and 0x81 never settles.
    badStatus <- runCase direction fake { fakeTriggerStatus = (9,3) } (caseDir "trigger-status")
    common (name++" trigger status") direction [] badStatus
    check (name++" trigger status: uncertain, one disarm straight after")
      (isUncertain CommitMayBeRunning badStatus && writes badStatus == full
        && afterTrigger badStatus == [writeFrame 0x80 0x33, windowFrame 0])

    -- Mid-sequence failures.
    lostPoll <- runCase direction fake { fakeLost = \request ordinal -> request == ReqWindow 0 && ordinal == 4 } (caseDir "lost-poll")
    common (name++" lost poll") direction [] lostPoll
    check (name++" lost poll: uncertain, one disarm") (isUncertain CommitMayBeRunning lostPoll && writes lostPoll == full)
    lostArm <- runCase direction fake { fakeLost = \request _ -> request == ReqWrite 0x81 0xff } (caseDir "lost-arm")
    common (name++" lost arm") direction [] lostArm
    check (name++" lost arm: uncertain, no trigger and no disarm")
      (isUncertain CommitSettled lostArm && writes lostArm == [(0xf7,target),(0x81,0xff)] && lineHas direction cutPower lostArm)
    armMismatch <- runCase direction fake { fakeSideEffect = \write -> [(0x81,0x00) | write == (0x81,0xff)] } (caseDir "arm-readback")
    common (name++" arm readback") direction [] armMismatch
    check (name++" arm readback: uncertain, no trigger")
      (isUncertain CommitSettled armMismatch && reasonHas "0x81 reads 0x00" armMismatch
        && writes armMismatch == [(0xf7,target),(0x81,0xff)])
    lostDisarmReadback <- runCase direction fake { fakeLost = \request ordinal -> request == ReqWindow 0 && ordinal == 7 } (caseDir "lost-disarm-readback")
    common (name++" lost disarm readback") direction [] lostDisarmReadback
    check (name++" lost disarm readback: uncertain, the disarm sent once")
      (isUncertain CommitSettled lostDisarmReadback && writes lostDisarmReadback == full)
    forM_ [ ("disarm-trigger", (0x80,0x22)), ("disarm-gate", (0xf7,0x02)), ("disarm-result", (0x81,0x01)) ] $ \(label,effect) -> do
      run <- runCase direction fake { fakeSideEffect = \write -> [effect | write == (0x80,0x33)] } (caseDir label)
      common (name++" "++label) direction [] run
      check (name++" "++label++": uncertain after the one disarm")
        (isUncertain CommitSettled run && reasonHas "disarm readback" run && writes run == full)
    closeLost <- runCase direction fake { fakeLost = \request _ -> request == ReqClose } (caseDir "close-lost")
    common (name++" close lost") direction [] closeLost
    check (name++" close lost after a good result: uncertain, not committed")
      (isUncertain CommitSettled closeLost && reasonHas "cleanup failed" closeLost && writes closeLost == full)
    sideEffect <- runCase direction fake { fakeSideEffect = \(index,_) -> [(0x10,0x00) | index == 0xf7] } (caseDir "readback-other")
    common (name++" readback other byte") direction [] sideEffect
    check (name++" readback other byte: uncertain after the gate write only")
      (isUncertain CommitSettled sideEffect && reasonHas "other than the written index" sideEffect && writes sideEffect == [(0xf7,target)])
    wrongGate <- runCase direction fake { fakeSideEffect = \(index,_) -> [(0xf7,0x02) | index == 0xf7] } (caseDir "readback-gate")
    common (name++" readback gate") direction [] wrongGate
    check (name++" readback gate: uncertain after the gate write only")
      (isUncertain CommitSettled wrongGate && writes wrongGate == [(0xf7,target)])

  -- The enable state-4 refusal writes the gate back and confirms it.
  stateFour <- runCase EnableStoredGate (fakeFor EnableStoredGate) { fakeState = 4 } (base </> "enable-state-4")
  common "enable state 4" EnableStoredGate [(0xf7,0x00)] stateFour
  check "enable state 4: refused with the gate written back"
    (isRefused stateFour && reasonHas "state 4" stateFour && writes stateFour == [(0xf7,0x01),(0xf7,0x00)])
  check "enable state 4: the write-back is read back before C4"
    (drop 4 (frames stateFour) == [writeFrame 0xf7 0x01, windowFrame 0, stateFrame, writeFrame 0xf7 0x00, windowFrame 0])
  stateFourLost <- runCase EnableStoredGate (fakeFor EnableStoredGate)
    { fakeState = 4, fakeLost = \request ordinal -> request == ReqWindow 0 && ordinal == 3 } (base </> "enable-state-4-lost")
  common "enable state 4 lost readback" EnableStoredGate [(0xf7,0x00)] stateFourLost
  check "enable state 4 with an unconfirmed write-back is uncertain" (isUncertain CommitSettled stateFourLost)

  removeDirectoryRecursive base
  putStrLn "commit-f7: frame, reference, refusal, held-reply, poll, disarm and journal checks passed"
