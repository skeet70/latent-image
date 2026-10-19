-- Persist adjustment index 0xf7 through the whole-area commit, or put the
-- stored byte back to 0.
--
-- Evidence: reports/stored-f7-ordinary-usb-read.md sections 4 and 5,
-- private-data/analysis/x-m1-1.50/stored-f7/writer-recovery/findings.md
-- section 6 and reports/end-setting-live-protocol.md (E-ENABLE-COMMIT-150).
-- No host-reachable range writer exists. A config 0x1a0002 function 3 write
-- of 0x22 starting at index 0x80 matches whitelist record 26 at 0x00dc7a84
-- in the callback 0x00dc7564, which posts queue-7 0x1707 and returns the
-- async marker; group-2 row 26 runs 0x00dbc8d8, which writes the whole
-- 0x420000-byte cache through 0x0069fea8 in 33 units of 128 KiB, stores
-- cache[0x81] = (result != 0) and only then posts the 0x3408 completion that
-- releases the held trigger reply. So the trigger's D1 is held for the whole
-- commit: it gets commitAllowanceMs instead of the 5000 ms budget, and its
-- reply status says nothing about the commit. Cache byte 0x81, armed to 0xff
-- first, is the completion signal; the window-0 poll after the reply reads
-- it.
--
-- If the trigger frame went out and its reply did not come back, the commit
-- may still be programming units. The run then waits until commitAllowanceMs
-- after the frame, reads window 0 once and only then disarms and closes, and
-- the outcome tells the owner to cut power only if 0x81 was seen at 0x00 or
-- 0x01 after the trigger.
--
-- The commit stores the cache as it is, so every byte of [0, 0x420000)
-- reaches storage: 0xf7 at its new value, 0x80 = 0x22 because the trigger
-- itself writes it, 0x81 = 0xff because the arm precedes the commit, and any
-- RAM residue. Window 0 must equal the owner's at-rest reference outside
-- 0x80, 0x81 and 0xf7, and 0xa2, which arm-auto-act leaves at 0x02 until the
-- power cycle, must be 0x00. Row 26 is inert in group-2 state 4, so the state
-- halfword at 0x015c20b0 is a precondition read through the 0x200001
-- function 2 path, which needs 0xf7 nonzero: the restore reads it before its
-- first write, the enable right after its gate readback and, on state 4,
-- writes 0xf7 back to 0 and confirms it before the refusal.
--
-- After the result, 0x80 = 0x33, the flag-0 record read-media uses, makes a
-- later zero-length or repeated-0x33 request at 0x80 in this boot inert; a
-- later 0x22 there would still start another commit. It posts nothing and
-- storage keeps 0x22.
--
-- The limits live in the types: CommitWrite is the whole write set, each
-- constructor one fixed (index, value) pair except the gate, whose value is a
-- GateValue, and every frame is the 37-byte length-1 form ReadMedia emits.
-- Nothing is retried; any stop after the first write is reported uncertain,
-- and only the 0x80 = 0x33 disarm is sent after a stop, when the trigger
-- frame went out and no disarm had been attempted.
module FujiAF.StoredGate
  ( CommitDirection(..), directionName, directionStart, directionTarget
  , GateValue(..), gateValueByte
  , CommitWrite(..), commitWriteIndex, commitWriteValue, commitWriteFrame, commitWriteStatus
  , commitTriggerByte, resultPendingByte, acceptedTriggerBytes, acceptedResultBytes
  , ReferenceWindow, mkReferenceWindow, referenceBytes, referenceDigest, readReferenceWindow
  , autoActArmIndex, referenceFreeIndexes
  , commitUnits, commitUnitAllowanceMs, commitAllowanceMs, commitPollSpacingMs, commitPollCap
  , commitRequestCount, commitAggregateDeadlineMs, commitJournalCapBytes
  , CommitState(..), CommitOutcome(..), commitOutcomeName, commitOutcomeLine
  , runCommitF7, openReplyTail
  ) where

import Control.Exception
import Control.Monad (unless, when)
import qualified Data.ByteString as B
import Data.IORef
import Data.Word (Word8, Word64)
import Numeric (showHex)
import System.FilePath ((</>))
import FujiAF.Transport
import FujiAF.Read
  ( ReadWindow(..), decodeReadReply, offsetName, productOracleBytes, readWindowLength
  , readReplyBytes, readRequestBytes, readRequestFrame, sessionFixedTransfers, word32At )
import FujiAF.Probe
  ( configReplyBytes, configWriteBytes, debugRequestBytes
  , productByteOffset, signatureByteOffset, transfersPerRequest )
import FujiAF.ReadMedia
  ( CacheIndex(..), TriggerValue(..), cacheIndexOf, cacheWriteFrame, decodeCacheWriteReply
  , decodeMediaReadReply, mediaReadFrame, mediaReadReplyBytes, stateAddress
  , triggerByte, triggerStatus )
import FujiAF.PlanDocument (privateRegular)
import FujiAF.SHA256 (sha256)
import qualified FujiAF.Broker as P

data CommitDirection = EnableStoredGate | RestoreStoredGate
  deriving (Eq, Show, Read, Enum, Bounded)

directionName :: CommitDirection -> String
directionName EnableStoredGate = "commit-f7-enable"
directionName RestoreStoredGate = "commit-f7-restore"

data GateValue = GateClosed | GateOpen
  deriving (Eq, Show, Read)

gateValueByte :: GateValue -> Word8
gateValueByte GateClosed = 0x00
gateValueByte GateOpen = 0x01

-- The value 0xf7 must hold at session start and the value the commit stores.
directionStart, directionTarget :: CommitDirection -> GateValue
directionStart EnableStoredGate = GateClosed
directionStart RestoreStoredGate = GateOpen
directionTarget EnableStoredGate = GateOpen
directionTarget RestoreStoredGate = GateClosed

data CommitWrite = SetGate GateValue | ArmResult | TriggerCommit | DisarmCommit
  deriving (Eq, Show, Read)

commitTriggerByte, resultPendingByte :: Word8
commitTriggerByte = 0x22
resultPendingByte = 0xff

commitWriteTarget :: CommitWrite -> CacheIndex
commitWriteTarget (SetGate _) = RamGateIndex
commitWriteTarget ArmResult = SpareGateIndex
commitWriteTarget TriggerCommit = TriggerIndex
commitWriteTarget DisarmCommit = TriggerIndex

commitWriteIndex :: CommitWrite -> Int
commitWriteIndex = cacheIndexOf . commitWriteTarget

commitWriteValue :: CommitWrite -> Word8
commitWriteValue (SetGate value) = gateValueByte value
commitWriteValue ArmResult = resultPendingByte
commitWriteValue TriggerCommit = commitTriggerByte
commitWriteValue DisarmCommit = triggerByte TriggerDisarm

commitWriteFrame :: CommitWrite -> [Word8]
commitWriteFrame write = cacheWriteFrame (commitWriteTarget write) (commitWriteValue write)

-- The flag-0 disarm posts nothing and answers (1,0xffff), as in read-media;
-- every other write answers (0,0).
commitWriteStatus :: CommitWrite -> (Int,Int)
commitWriteStatus DisarmCommit = triggerStatus TriggerDisarm
commitWriteStatus _ = (0,0)

-- A fresh boot loads 0x80 from storage: 0x00 in every 1.50 capture
-- (E-ADJ-GATE-BYTE-150, E-ADJ-BACKUP-GAP-READ-150) or 0x22, which every
-- commit stores and the 1.00 body held at rest. 0x33 or 0x35 is RAM left by
-- an earlier 0x80 write in the same boot, and read-media's action 37 leaves
-- the storage driver closed until a power cycle.
acceptedTriggerBytes :: [Word8]
acceptedTriggerBytes = [0x00, commitTriggerByte]

-- 0x81 reads 0x00 on 1.50 and 0xff where an armed commit stored it.
acceptedResultBytes :: [Word8]
acceptedResultBytes = [0x00, resultPendingByte]

-- The owner's at-rest capture of config window 0x000000. The constructor is
-- not exported; the only builder takes exactly one window.
newtype ReferenceWindow = ReferenceWindow [Word8]
  deriving (Eq, Show)

mkReferenceWindow :: B.ByteString -> Either String ReferenceWindow
mkReferenceWindow bytes
  | B.length bytes /= readWindowLength =
      Left ("reference window must be exactly "++show readWindowLength++" bytes, not "++show (B.length bytes))
  | otherwise = Right (ReferenceWindow (B.unpack bytes))

referenceBytes :: ReferenceWindow -> [Word8]
referenceBytes (ReferenceWindow bytes) = bytes

referenceDigest :: ReferenceWindow -> String
referenceDigest = sha256 . B.pack . referenceBytes

readReferenceWindow :: FilePath -> IO ReferenceWindow
readReferenceWindow path =
  privateRegular "reference window" path readWindowLength >>= either P.reject pure . mkReferenceWindow

-- arm-auto-act's arm byte, refused nonzero whatever the reference holds.
autoActArmIndex :: Int
autoActArmIndex = 0xa2

-- The window-0 indices the commit itself changes or that the start checks
-- judge on their own.
referenceFreeIndexes :: [Int]
referenceFreeIndexes = [cacheIndexOf TriggerIndex, cacheIndexOf SpareGateIndex, cacheIndexOf RamGateIndex]

-- The commit allowance: 33 units of 0x20000 bytes, each allowed 1000 ms for
-- its block program, verify and old-block erase. It is the held trigger
-- reply's budget and the bound of the poll, 330 reads 100 ms apart.
commitUnits, commitUnitAllowanceMs, commitAllowanceMs, commitPollSpacingMs, commitPollCap :: Int
commitUnits = 33
commitUnitAllowanceMs = 1000
commitAllowanceMs = commitUnits*commitUnitAllowanceMs
commitPollSpacingMs = 100
commitPollCap = commitAllowanceMs `div` commitPollSpacingMs

-- Identity, the three start windows, the state read, the gate write and
-- readback, the arm and readback, the trigger, the disarm and readback,
-- plus the poll bound. The held-reply path replaces the polls with one read.
commitFixedRequests :: Int
commitFixedRequests = 12

commitRequestCount :: Int
commitRequestCount = commitFixedRequests + commitPollCap

-- Every transfer at 5000 ms, plus the held trigger reply's allowance and
-- the poll spacing.
commitAggregateDeadlineMs :: Int
commitAggregateDeadlineMs =
  transferTimeoutMs*(sessionFixedTransfers + transfersPerRequest*commitRequestCount)
  + commitAllowanceMs + commitPollSpacingMs*commitPollCap

commitJournalCapBytes :: Int
commitJournalCapBytes = 65536 + 4096*commitRequestCount

-- CommitMayBeRunning: the trigger frame went out and 0x81 was never seen at
-- 0x00 or 0x01 afterwards.
data CommitState = CommitSettled | CommitMayBeRunning
  deriving (Eq, Show)

-- committed and commit-failed are the result byte's 0x00 and 0x01 with the
-- disarm confirmed and the session closed; refused means no cache byte
-- differs from session start; uncertain is every other end.
data CommitOutcome
  = Committed Int
  | CommitFailed Int
  | Refused String
  | Uncertain CommitState String
  deriving (Eq, Show)

commitOutcomeName :: CommitOutcome -> String
commitOutcomeName (Committed _) = "committed"
commitOutcomeName (CommitFailed _) = "commit-failed"
commitOutcomeName (Refused _) = "refused"
commitOutcomeName (Uncertain _ _) = "uncertain"

commitOutcomeLine :: CommitDirection -> CommitOutcome -> String
commitOutcomeLine direction outcome = concat
  [ commitOutcomeName outcome
  , ";f7=0x"++hexByte (gateValueByte (directionStart direction))++"->0x"++hexByte (gateValueByte (directionTarget direction))
  , detail outcome
  , ending outcome ]
  where
    detail (Committed polls) = ";result=0x00;polls="++show polls
    detail (CommitFailed polls) = ";result=0x01;polls="++show polls++";storage may hold a mix of old and new units"
    detail (Refused reason) = ";reason="++reason
    detail (Uncertain _ reason) = ";reason="++reason
    ending (Uncertain CommitMayBeRunning _) =
      ";the commit may still be running: do not cut power; leave the camera untouched for at least one minute,"
      ++" then cut the dummy-battery supply and report this output"
    ending _ = ";end the session by cutting the dummy-battery supply"

hexByte :: Word8 -> String
hexByte value = let digits = showHex value "" in replicate (2 - length digits) '0' ++ digits

-- RestoredUnchanged is the enable state-4 refusal after its confirmed gate
-- write-back. TriggerOutstanding carries the session-clock time the trigger
-- frame was last known in flight.
data Progress
  = NothingWritten
  | Written
  | RestoredUnchanged
  | TriggerOutstanding Word64
  | TriggerAnswered
  | DisarmAttempted
  deriving (Eq, Show)

progressName :: Progress -> String
progressName NothingWritten = "nothing-written"
progressName Written = "gate-written"
progressName RestoredUnchanged = "gate-restored"
progressName (TriggerOutstanding _) = "trigger-reply-outstanding"
progressName TriggerAnswered = "trigger-answered"
progressName DisarmAttempted = "disarm-attempted"

-- linkTriggered is set before the trigger frame goes out; linkSettled once a
-- window-0 read after it shows 0x81 at 0x00 or 0x01.
data CommitLink = CommitLink
  { linkSession :: Session, linkSid :: Int, linkInput :: Int, linkOutput :: Int
  , linkTriggered :: IORef Bool, linkSettled :: IORef Bool }

openReplyTail :: [Word8]
openReplyTail = [0,0,0,0,1,0]

commitStop :: Session -> String -> [Word8] -> String -> IO a
commitStop s label response reason = do
  note s "commit-f7-stop" (label++";reply="++P.hexBytes response++";"++reason)
  P.reject reason

runCommitF7 :: CommitDirection -> ReferenceWindow -> Clock -> UsbApi -> FilePath -> IO CommitOutcome
runCommitF7 direction reference clock api dir = runCommitCore direction reference clock api dir `catch` \(e::SomeException) -> do
  _ <- try (recordWithin commitJournalCapBytes dir "session-uncertain" (take 4096 (displayException e))) :: IO (Either SomeException ())
  throwIO e

runCommitCore :: CommitDirection -> ReferenceWindow -> Clock -> UsbApi -> FilePath -> IO CommitOutcome
runCommitCore direction reference clock api dir = do
  s <- newSession clock api dir commitJournalCapBytes commitAggregateDeadlineMs
  held <- newHeld
  openedSession <- newIORef Nothing
  progress <- newIORef NothingWritten
  triggered <- newIORef False
  settled <- newIORef False
  note s "prepared" (directionName direction
                     ++";start=0x"++hexByte (gateValueByte (directionStart direction))
                     ++";target=0x"++hexByte (gateValueByte (directionTarget direction))
                     ++";reference-sha256="++referenceDigest reference
                     ++";allowance-ms="++show commitAllowanceMs
                     ++";poll-cap="++show commitPollCap++";poll-spacing-ms="++show commitPollSpacingMs)
  mask $ \restore -> do
    result <- try $ restore $ do
      device <- acquireDevice s held
      let input = inputEndpoint (descriptors device)
          output = outputEndpoint (descriptors device)
      _ <- tx s (P.controlTransfer (P.control 0xc0 8 0)) 12
      reply <- tx s (P.BulkIn input 8) 8
      let sid = fromIntegral (reply!!0) + 256*fromIntegral (reply!!1)
          link = CommitLink s sid input output triggered settled
      writeIORef openedSession (Just (device,sid))
      unless (drop 2 reply == openReplyTail)
        (commitStop s "open" reply ("the C0 reply tail "++P.hexBytes (drop 2 reply)++" differs from the constant "++P.hexBytes openReplyTail))
      identify link
      start <- windowRead link 0x000000
      oracle <- windowRead link 0x000100
      signature <- windowRead link 0x000200
      mapM_ (\(offset,payload) -> writeCapture link ("config-"++offsetName offset) payload)
        [(0x000000,start),(0x000100,oracle),(0x000200,signature)]
      either (commitStop s "preconditions" []) pure (startRefusal direction reference start oracle signature)
      note s "preconditions" ("f7=0x"++hexByte (start !! gateOffset)++";0x80=0x"++hexByte (start !! triggerOffset)
                              ++";0x81=0x"++hexByte (start !! resultOffset)++";product=X-M1;signature=55aa;reference=equal")
      when (direction == RestoreStoredGate) (requireState link)
      writeIORef progress Written
      commitWrite link (SetGate (directionTarget direction))
      gated <- confirmWindow link "gate-readback" start gateOffset (gateValueByte (directionTarget direction))
      when (direction == EnableStoredGate) $ do
        state <- readState link
        when (state == stateRowInert) $ do
          commitWrite link (SetGate (directionStart direction))
          _ <- confirmWindow link "gate-restore-readback" gated gateOffset (gateValueByte (directionStart direction))
          writeIORef progress RestoredUnchanged
          commitStop s "group-2-state" [] "precondition failure: group-2 state 4 makes row 26 inert; 0xf7 written back to 0x00 and confirmed"
      _ <- confirmWrite link ArmResult gated "arm-readback" resultOffset
      triggerCommit link progress
      (code,polls) <- pollCommit link 0
      writeIORef progress DisarmAttempted
      disarm link (directionTarget direction) (Just code)
      pure (code,polls)
    errors <- newIORef ([] :: [String])
    let attempt label action = action `catch` \(e::SomeException) -> modifyIORef' errors (++[label++": "++displayException e])
    opened <- readIORef openedSession
    case opened of
      Nothing -> note s "cleanup-unavailable" "unknown SID; no invented C4"
      Just (device,sid) -> do
        let link = CommitLink s sid (inputEndpoint (descriptors device)) (outputEndpoint (descriptors device)) triggered settled
        reached <- readIORef progress
        case reached of
          TriggerOutstanding sent -> do
            attempt "held-reply wait" (awaitAllowance link sent)
            writeIORef progress DisarmAttempted
            attempt "disarm" (disarm link (directionTarget direction) Nothing)
          TriggerAnswered -> do
            writeIORef progress DisarmAttempted
            attempt "disarm" (disarm link (directionTarget direction) Nothing)
          _ -> pure ()
        attempt "protocol close" $ do
          _ <- tx s (P.controlTransfer (P.control 0xc4 2 sid)) 12
          _ <- tx s (P.BulkIn (inputEndpoint (descriptors device)) 2) 2
          note s "close-reply" "opaque bytes; stock restoration unobserved"
    releaseHeld held api (\label action -> attempt label (bounded s label action))
    failures <- readIORef errors
    reached <- readIORef progress
    wasTriggered <- readIORef triggered
    wasSettled <- readIORef settled
    let cleanup = if null failures then "" else "; cleanup failed: "++show failures
        state = if wasTriggered && not wasSettled then CommitMayBeRunning else CommitSettled
        outcome = case result of
          Right (code,polls)
            | not (null failures) -> Uncertain state ("result 0x"++hexByte code++" after "++show polls++" polls"++cleanup)
            | code == 0 -> Committed polls
            | otherwise -> CommitFailed polls
          Left (e::SomeException)
            | reached `elem` [NothingWritten, RestoredUnchanged] -> Refused (displayException e++cleanup)
            | otherwise -> Uncertain state (displayException e++"; progress "++progressName reached++cleanup)
    note s "commit-f7-outcome" (commitOutcomeLine direction outcome)
    pure outcome
  where
    disarm link target code = do
      commitWrite link DisarmCommit
      payload <- windowRead link 0x000000
      writeCapture link "disarm-readback" payload
      let s = linkSession link
          observed = maybe "" (\value -> ";result=0x"++hexByte value) code
      note s "disarm-readback" ("0x80=0x"++hexByte (payload !! triggerOffset)++";0x81=0x"++hexByte (payload !! resultOffset)
                               ++";f7=0x"++hexByte (payload !! gateOffset)++observed)
      unless (payload !! triggerOffset == triggerByte TriggerDisarm && payload !! gateOffset == gateValueByte target)
        (commitStop s "disarm-readback" payload "the disarm readback does not show 0x80 = 0x33 with the target 0xf7")
      case code of
        Just value | payload !! resultOffset /= value ->
          commitStop s "disarm-readback" payload "the disarm readback's 0x81 differs from the polled result"
        _ -> pure ()

-- The trigger exchange with the held D1. The frame counts as in flight from
-- before its bulk OUT; the allowance runs from after it.
triggerCommit :: CommitLink -> IORef Progress -> IO ()
triggerCommit link progress = do
  let s = linkSession link
      clock = sessionClock s
  _ <- tx s (P.controlTransfer (P.control 0xd0 configWriteBytes (linkSid link))) 12
  writeIORef (linkTriggered link) True
  nowNS clock >>= writeIORef progress . TriggerOutstanding
  _ <- tx s (P.BulkOut (linkOutput link) (commitWriteFrame TriggerCommit)) configWriteBytes
  nowNS clock >>= writeIORef progress . TriggerOutstanding
  _ <- txWithin commitAllowanceMs s (P.controlTransfer (P.control 0xd1 configReplyBytes (linkSid link))) 12
  bytes <- tx s (P.BulkIn (linkInput link) configReplyBytes) configReplyBytes
  writeIORef progress TriggerAnswered
  either (commitStop s "cache-write-0x80" bytes) pure
    (decodeCacheWriteReply TriggerIndex (commitWriteStatus TriggerCommit) bytes)
  note s "cache-write-complete" ("index=0x80;value=0x"++hexByte commitTriggerByte++";status=(0,0)")

-- The trigger reply did not come back: wait until commitAllowanceMs after
-- the frame, then read window 0 once.
awaitAllowance :: CommitLink -> Word64 -> IO ()
awaitAllowance link sent = do
  let s = linkSession link
  now <- nowNS (sessionClock s)
  let elapsedMs = fromIntegral ((now - min now sent) `div` 1000000) :: Int
      waitMs = max 0 (commitAllowanceMs - elapsedMs)
  note s "held-reply-wait" ("elapsed-ms="++show elapsedMs++";wait-ms="++show waitMs)
  pauseMs (sessionClock s) waitMs
  payload <- windowRead link 0x000000
  writeCapture link "held-reply-readback" payload
  note s "held-reply-readback" ("0x81=0x"++hexByte (payload !! resultOffset))

gateOffset, triggerOffset, resultOffset :: Int
gateOffset = cacheIndexOf RamGateIndex
triggerOffset = cacheIndexOf TriggerIndex
resultOffset = cacheIndexOf SpareGateIndex

stateRowInert :: Int
stateRowInert = 4

-- The window 0x000100 oracle carries the product string at +0x5c (cache
-- +0x15c) and the window 0x000200 the 0xaa55 signature at +0x02 (cache
-- +0x202), so the cache is the body's and not the template fallback.
startRefusal :: CommitDirection -> ReferenceWindow -> [Word8] -> [Word8] -> [Word8] -> Either String ()
startRefusal direction reference start oracle signature
  | take (length productOracleBytes) (drop productByteOffset oracle) /= productOracleBytes =
      Left "precondition failure: cache +0x15c does not carry the X-M1 product string"
  | take 2 (drop signatureByteOffset signature) /= [0x55,0xaa] =
      Left "precondition failure: cache +0x202 is not 55 aa"
  | start !! autoActArmIndex /= 0 =
      Left ("precondition failure: 0xa2 reads 0x"++hexByte (start !! autoActArmIndex)
            ++", the AUTO_ACT arm arm-auto-act leaves in RAM; the commit would store it; power-cycle first")
  | not (null residue) =
      Left ("precondition failure: window 0 differs from the reference at "++unwords (map (\at -> "0x"++showHex at "") (take 16 residue))
            ++"; the commit would store the difference")
  | start !! gateOffset /= gateValueByte (directionStart direction) =
      Left ("precondition failure: 0xf7 reads 0x"++hexByte (start !! gateOffset)++", not the start value 0x"
            ++hexByte (gateValueByte (directionStart direction)))
  | start !! triggerOffset == triggerByte TriggerDisarm =
      Left "precondition failure: 0x80 reads 0x33, RAM from an earlier 0x80 write in this boot; power-cycle first"
  | (start !! triggerOffset) `notElem` acceptedTriggerBytes =
      Left ("precondition failure: 0x80 reads 0x"++hexByte (start !! triggerOffset)++", not 0x00 or 0x22")
  | (start !! resultOffset) `notElem` acceptedResultBytes =
      Left ("precondition failure: 0x81 reads 0x"++hexByte (start !! resultOffset)++", not 0x00 or 0xff")
  | otherwise = Right ()
  where
    residue = [ at | (at,observed,expected) <- zip3 [0..] start (referenceBytes reference)
                   , at `notElem` referenceFreeIndexes, observed /= expected ]

identify :: CommitLink -> IO ()
identify link = do
  let s = linkSession link
  _ <- tx s (P.controlTransfer (P.control 0xd0 32 (linkSid link))) 12
  _ <- tx s (P.BulkOut (linkOutput link) P.identityPacket) 32
  _ <- tx s (P.controlTransfer (P.control 0xd1 128 (linkSid link))) 12
  bytes <- tx s (P.BulkIn (linkInput link) 128) 128
  model <- either (commitStop s "identity" bytes) pure (P.decodeProduct bytes)
  note s "identity" model

-- One 256-byte config window at a 256-aligned offset. A window-0 read after
-- the trigger frame records whether 0x81 has settled.
windowRead :: CommitLink -> Int -> IO [Word8]
windowRead link offset = do
  let s = linkSession link
      window = ReadWindow 0 offset (offsetName offset)
  _ <- tx s (P.controlTransfer (P.control 0xd0 readRequestBytes (linkSid link))) 12
  _ <- tx s (P.BulkOut (linkOutput link) (readRequestFrame window)) readRequestBytes
  _ <- tx s (P.controlTransfer (P.control 0xd1 readReplyBytes (linkSid link))) 12
  bytes <- tx s (P.BulkIn (linkInput link) readReplyBytes) readReplyBytes
  payload <- either (commitStop s ("config-"++offsetName offset) bytes) pure (decodeReadReply window bytes)
  sent <- readIORef (linkTriggered link)
  when (offset == 0 && sent && payload !! resultOffset `elem` [0x00,0x01]) (writeIORef (linkSettled link) True)
  pure payload

writeCapture :: CommitLink -> String -> [Word8] -> IO ()
writeCapture link label payload = do
  let s = linkSession link
  writePrivateBytes (sessionDirectory s </> (label++".bin")) (B.pack payload)
  note s "window-complete" (label++";sha256="++sha256 (B.pack payload))

commitWrite :: CommitLink -> CommitWrite -> IO ()
commitWrite link write = do
  let s = linkSession link
  _ <- tx s (P.controlTransfer (P.control 0xd0 configWriteBytes (linkSid link))) 12
  _ <- tx s (P.BulkOut (linkOutput link) (commitWriteFrame write)) configWriteBytes
  _ <- tx s (P.controlTransfer (P.control 0xd1 configReplyBytes (linkSid link))) 12
  bytes <- tx s (P.BulkIn (linkInput link) configReplyBytes) configReplyBytes
  either (commitStop s ("cache-write-0x"++showHex (commitWriteIndex write) "") bytes) pure
    (decodeCacheWriteReply (commitWriteTarget write) (commitWriteStatus write) bytes)
  note s "cache-write-complete" ("index=0x"++showHex (commitWriteIndex write) ""++";value=0x"++hexByte (commitWriteValue write)
                                 ++";status="++show (commitWriteStatus write))

-- Window 0 read back after a write: the written index holds the value and no
-- other byte differs from the previous window.
confirmWindow :: CommitLink -> String -> [Word8] -> Int -> Word8 -> IO [Word8]
confirmWindow link label previous index value = do
  payload <- windowRead link 0x000000
  writeCapture link label payload
  let changed = [at | (at,before,after) <- zip3 [0::Int ..] previous payload, at /= index, before /= after]
  unless (payload !! index == value)
    (commitStop (linkSession link) label payload ("index 0x"++showHex index ""++" reads 0x"++hexByte (payload !! index)
                                                 ++", not the written 0x"++hexByte value))
  unless (null changed)
    (commitStop (linkSession link) label payload ("bytes other than the written index changed: "++show changed))
  pure payload

confirmWrite :: CommitLink -> CommitWrite -> [Word8] -> String -> Int -> IO [Word8]
confirmWrite link write previous label index = do
  commitWrite link write
  confirmWindow link label previous index (commitWriteValue write)

readState :: CommitLink -> IO Int
readState link = do
  let s = linkSession link
      size = 2
  _ <- tx s (P.controlTransfer (P.control 0xd0 debugRequestBytes (linkSid link))) 12
  _ <- tx s (P.BulkOut (linkOutput link) (mediaReadFrame stateAddress size)) debugRequestBytes
  _ <- tx s (P.controlTransfer (P.control 0xd1 (mediaReadReplyBytes size) (linkSid link))) 12
  bytes <- tx s (P.BulkIn (linkInput link) (mediaReadReplyBytes size)) (mediaReadReplyBytes size)
  payload <- either (commitStop s "group-2-state" bytes) pure (decodeMediaReadReply stateAddress size bytes)
  let state = word32At 0 payload
  note s "group-2-state" ("state="++show state)
  pure state

requireState :: CommitLink -> IO ()
requireState link = do
  state <- readState link
  when (state == stateRowInert)
    (commitStop (linkSession link) "group-2-state" [] "precondition failure: group-2 state 4 makes row 26 inert")

-- Window 0 until 0x81 leaves the armed 0xff: 0x00 is success and 0x01 the
-- commit's failure; any other value stops. After a held reply the first read
-- is expected to be terminal.
pollCommit :: CommitLink -> Int -> IO (Word8,Int)
pollCommit link count
  | count >= commitPollCap =
      P.reject ("commit result 0x81 never left 0xff after "++show commitPollCap++" polls")
  | otherwise = do
      when (count > 0) (pauseMs (sessionClock (linkSession link)) commitPollSpacingMs)
      payload <- windowRead link 0x000000
      let code = payload !! resultOffset
          s = linkSession link
      note s "commit-poll" ("poll="++show count++";result=0x"++hexByte code++";sha256="++sha256 (B.pack payload))
      case code of
        0xff -> pollCommit link (count+1)
        0x00 -> pure (code,count+1)
        0x01 -> pure (code,count+1)
        other -> commitStop s ("commit-poll-"++show count) payload ("commit result byte left 0xff to 0x"++hexByte other)
