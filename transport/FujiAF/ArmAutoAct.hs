-- Arm the AUTO_ACT gate byte 0xa2, confirm it, watch the camera's own
-- witnesses and end the session by power cycle.
--
-- Design and evidence: reports/auto-act-arming.md section 13 and the session
-- document reports/auto-act-session-script.md. The arm is a config 0x1a0002
-- function 3 write to adjustment index 0xa2 with one appended byte 0x02: the
-- callback 0x00dc7564 stores through 0x0069fc94 before any other test, so the
-- byte lands in the RAM mirror of the adjustment area at 0x41acd8a2, and the
-- queue post that reaches storage exists only for a start index of 0x80,
-- which this command cannot name. The boot load 0x0069f204 refills the mirror
-- from storage, where the byte reads 0x00, so the power cycle at the end
-- clears it and no disarm write is sent.
--
-- With the byte set, the mujin task's prologue reads it once per boot after
-- its first wait, and on equality with 2 waits again and calls the sender
-- FUN_00ed2c34, which re-tests the byte, tests the guard 0x1667048, posts
-- 0x3300 and 0x3302, sets the guard and waits its own tail. The posts are
-- processed after that tail: 0x3300's handler 0xed16e8 sets the selector
-- 0x16777c0 to 1, and 0x3302 reaches the loader 0x00ed1de0, which clears the
-- 0x10764-byte script area at 0x166704c, opens <volume>:\DCAA\auto_act.scr,
-- reads it under the 1 MiB cap into 0x43e6ff00, stores the size at 0x16777ac
-- when the completion count equals the requested size, parses, sets the
-- selector to 2 and posts 0x3305; END sets the selector back to 1 and the
-- statement index 0x1667054 to -1.
--
-- The command opens the RAM-read gate of probe-ram (adjustment index 0xf7),
-- reads the witness windows 0x1667000 and 0x1677700 through the reviewed
-- 0x200001 function 2 requests, and restores the gate on every exit path.
-- The witness cells are the guard 0x1667048, the statement words, the input
-- length word 0x16777ac, the loader's async block 0x16777b0..0x16777bc and
-- the selector 0x16777c0; reports/auto-act-session-script.md carries the
-- interpretation table.
--
-- The limits live in the types: WriteTarget holds the two start indices this
-- command writes, and mkWriteTarget refuses every other index with 0x80
-- named as the whole-area commit or reload trigger. Every config write frame
-- is the 37-byte length-1 form with one appended byte, so no request is
-- zero-length and none spans an index.
module FujiAF.ArmAutoAct
  ( WriteTarget(..), writeTargetIndex, writeTargetName, writeTargetIndexes, mkWriteTarget
  , ArmWrites(..), armWrites
  , armValueByte, gateEnabledByte, gateDisabledByte
  , configWriteFrame, decodeConfigWriteReply
  , guardAddress, sentinelAddress, statementIndexAddress, statementCountAddress
  , statementCodeAddress, statementTextAddress, statementTextBytes
  , inputLengthAddress, inFlightAddress, statusAddress, openResultAddress
  , completionCountAddress, selectorAddress
  , witnessWindowA, witnessWindowB, witnessHalves
  , Witness(..), witnessOf, statementTextOf, witnessSummary
  , witnessSettled, baselineClear
  , watchPollCap, watchSpacingMs
  , ArmOutcome(..), armOutcomeName, deriveOutcome
  , ArmReport(..), armReportLine
  , DeviceStart(..), deviceWaitBoundMs, deviceWaitSpacingMs, CameraTick(..), tickSeconds
  , armAutoActRequestCount, armAutoActAggregateDeadlineMs, armAutoActJournalCapBytes
  , runArmAutoAct
  ) where

import Control.Concurrent (threadDelay)
import Control.Exception
import Control.Monad (forM_, unless, when)
import qualified Data.ByteString as B
import Data.IORef
import Data.Word (Word8, Word32)
import Numeric (showHex)
import Text.Printf (printf)
import System.IO (hPutStrLn, stderr, hFlush)
import System.FilePath ((</>))
import FujiAF.Transport
import FujiAF.Read
  ( ReadWindow(..), decodeReadReply, readOpcode, readReplyBytes
  , readRequestBytes, readRequestFrame
  , replyError1Offset, replyError2Offset
  , replyFunctionOffset, replyLengthOffset, replyMailboxOffset
  , replyOffsetOffset, replyPageOffset, replyReservedOffset
  , sessionFixedTransfers, word32At, word32le )
import FujiAF.Probe
  ( configReplyBytes, configWriteBytes, configWriteFunction, configMailbox
  , configWriteLength, debugReplyForHalf, debugRequestBytes, debugRequestFrame
  , decodeDebugReply, gateDisabled, gateEnabled, gateIndex
  , gateReadbackEnabled, gateReadbackRestored, probeHalfBytes, probeName
  , readbackIndex, transfersPerRequest )
import FujiAF.SHA256 (sha256)
import qualified FujiAF.Broker as P

-- A config function 3 start index. The whole writable set is here: the byte
-- this command arms and the RAM-read gate it borrows from probe-ram.
data WriteTarget = ArmByte | GateByte
  deriving (Eq, Ord, Show, Read, Enum, Bounded)

writeTargetIndex :: WriteTarget -> Int
writeTargetIndex ArmByte = 0xa2
writeTargetIndex GateByte = gateIndex

writeTargetName :: WriteTarget -> String
writeTargetName ArmByte = "arm index 0xa2"
writeTargetName GateByte = "gate index 0xf7"

writeTargetIndexes :: [Int]
writeTargetIndexes = map writeTargetIndex [minBound .. maxBound]

-- The refusing builder. 0x80 is named because a write that starts there runs
-- the whitelist scan that posts the whole-area commit (value 0x22) or the
-- reload (value 0x21); every other index is refused as one this command does
-- not own.
mkWriteTarget :: Int -> Either String WriteTarget
mkWriteTarget value
  | value == 0x80 =
      Left "a config write starting at index 0x80 posts the whole-area commit or the reload and is never sent"
  | otherwise = case [target | target <- [minBound .. maxBound], writeTargetIndex target == value] of
      [target] -> Right target
      _ -> Left ("config write start index 0x"++showHex value ""
                 ++" is outside the two indices this command writes (0x"
                 ++showHex (writeTargetIndex ArmByte) ""++", 0x"++showHex (writeTargetIndex GateByte) ""++")")

-- The session's two start indices, resolved before the device is selected.
data ArmWrites = ArmWrites { armWriteTarget :: WriteTarget, gateWriteTarget :: WriteTarget }
  deriving (Eq, Show)

armWrites :: Either String ArmWrites
armWrites = ArmWrites <$> mkWriteTarget (writeTargetIndex ArmByte) <*> mkWriteTarget (writeTargetIndex GateByte)

armValueByte :: Word8
armValueByte = 0x02

gateEnabledByte, gateDisabledByte :: Word8
gateEnabledByte = fromIntegral gateEnabled
gateDisabledByte = fromIntegral gateDisabled

-- Two hex digits, the way the byte values are written in the documents.
hexByte :: Word8 -> String
hexByte value = let digits = showHex value "" in replicate (2 - length digits) '0' ++ digits

-- Config opcode 0x1a0002 function 3: function at +0x10, mb +0x14, page
-- +0x18, in-page offset +0x1c, length +0x20, one appended data byte. The
-- frame is 37 bytes for every target, so no request can be zero-length or
-- span an index.
configWriteFrame :: WriteTarget -> Word8 -> [Word8]
configWriteFrame target value = concat
  [ word32le readOpcode, replicate 12 0
  , word32le configWriteFunction, word32le configMailbox
  , word32le 0, word32le (writeTargetIndex target)
  , word32le configWriteLength, [value] ]

-- The write reply echoes page, offset and length and carries the status
-- words; it does not carry the written byte, which is why the confirm is a
-- config read. The three status pairs are the read path's.
decodeConfigWriteReply :: WriteTarget -> (Int,Int) -> [Word8] -> Either String ()
decodeConfigWriteReply target expected bytes
  | length bytes /= configReplyBytes = Left ("reply length "++show (length bytes)++" differs from the requested "++show configReplyBytes)
  | word32At 0 bytes /= readOpcode = Left "reply opcode differs from the request"
  | word32At replyReservedOffset bytes /= 0 = Left "reply reserved word is nonzero"
  | word32At replyFunctionOffset bytes /= configWriteFunction = Left "reply function differs from the request"
  | word32At replyMailboxOffset bytes /= configMailbox = Left "reply mailbox differs from the request"
  | echoed /= requested = Left ("echoed page/offset/length "++show echoed++" differs from the request "++show requested)
  | status /= expected = Left ("status "++show status++" differs from the expected "++show expected)
  | otherwise = Right ()
  where
    echoed = (word32At replyPageOffset bytes, word32At replyOffsetOffset bytes, word32At replyLengthOffset bytes)
    requested = (0, writeTargetIndex target, configWriteLength)
    status = (word32At replyError1Offset bytes, word32At replyError2Offset bytes)

-- The mujin module's state words. The loader's clear runs from 0x166704c
-- through 0x16777af, over the statement words and the input length word; the
-- guard 0x1667048 sits below it, and the async block 0x16777b0 and the
-- selector 0x16777c0 at and above its end.
guardAddress, sentinelAddress, statementIndexAddress, statementCountAddress :: Int
guardAddress = 0x01667048
sentinelAddress = 0x01667050
statementIndexAddress = 0x01667054
statementCountAddress = 0x01667058

statementCodeAddress, statementTextAddress :: Int
statementCodeAddress = 0x0166705c
statementTextAddress = 0x01667060

-- The parser copies each token into a 20-byte slot and truncates it at ten
-- characters.
statementTextBytes :: Int
statementTextBytes = 20

inputLengthAddress, inFlightAddress, statusAddress, openResultAddress :: Int
inputLengthAddress = 0x016777ac
inFlightAddress = 0x016777b0
statusAddress = 0x016777b4
openResultAddress = 0x016777b8

-- The file-completion callbacks (0x00ed17b0 for a script's own file
-- operations, 0x00ed17d8 for the loader's read) store their transferred byte
-- count here, so the cell shows the last completed file operation's count;
-- the input length word above is the load's size evidence.
completionCountAddress, selectorAddress :: Int
completionCountAddress = 0x016777bc
selectorAddress = 0x016777c0

-- The two 256-byte witness windows and their 128-byte halves. The 0x200001
-- function 2 handler returns 284 bytes complete at most, so a window is two
-- requests, the split probe-ram and read-log use.
witnessWindowA, witnessWindowB :: Int
witnessWindowA = 0x01667000
witnessWindowB = 0x01677700

witnessHalves :: Int -> [Int]
witnessHalves base = [base, base + probeHalfBytes]

data Witness = Witness
  { witnessGuard :: Int
  , witnessSentinel :: Int
  , witnessStatementIndex :: Int
  , witnessStatementCount :: Int
  , witnessStatementCode :: Int
  , witnessStatementText :: String
  , witnessInFlight :: Int
  , witnessStatus :: Int
  , witnessOpenResult :: Int
  , witnessCompletionCount :: Int
  , witnessInputLength :: Int
  , witnessSelector :: Int
  } deriving (Eq, Show, Read)

-- The loader's two -1 words and the statement index are signed 32-bit
-- values; -1 is the loader's initial sentinel and END's index.
signedWord32At :: Int -> [Word8] -> Int
signedWord32At at bytes =
  let value = word32At at bytes in if value >= 0x80000000 then value - 0x100000000 else value

statementTextOf :: [Word8] -> String
statementTextOf bytes = map (toEnum . fromIntegral) (takeWhile (\byte -> byte /= 0 && byte >= 0x20 && byte < 0x7f) (take statementTextBytes bytes))

-- The two windows are indexed from their own base, so every cell address is
-- reduced to its in-window offset.
witnessOf :: [Word8] -> [Word8] -> Witness
witnessOf windowA windowB = Witness
  { witnessGuard = byteAt guardAddress windowA
  , witnessSentinel = signedWord32At (sentinelAddress - witnessWindowA) windowA
  , witnessStatementIndex = signedWord32At (statementIndexAddress - witnessWindowA) windowA
  , witnessStatementCount = word32At (statementCountAddress - witnessWindowA) windowA
  , witnessStatementCode = word32At (statementCodeAddress - witnessWindowA) windowA
  , witnessStatementText = statementTextOf (drop (statementTextAddress - witnessWindowA) windowA)
  , witnessInFlight = word32At (inFlightAddress - witnessWindowB) windowB
  , witnessStatus = word32At (statusAddress - witnessWindowB) windowB
  , witnessOpenResult = word32At (openResultAddress - witnessWindowB) windowB
  , witnessCompletionCount = word32At (completionCountAddress - witnessWindowB) windowB
  , witnessInputLength = word32At (inputLengthAddress - witnessWindowB) windowB
  , witnessSelector = word32At (selectorAddress - witnessWindowB) windowB
  }
  where byteAt address window = fromIntegral (window !! (address - witnessWindowA))

witnessSummary :: Witness -> String
witnessSummary witness = concat
  [ "guard="++show (witnessGuard witness)
  , ";sentinel="++show (witnessSentinel witness)
  , ";index="++show (witnessStatementIndex witness)
  , ";count="++show (witnessStatementCount witness)
  , ";code=0x"++showHex (witnessStatementCode witness) ""
  , ";text="++witnessStatementText witness
  , ";in-flight="++show (witnessInFlight witness)
  , ";status="++show (witnessStatus witness)
  , ";open="++show (witnessOpenResult witness)
  , ";completion-count="++show (witnessCompletionCount witness)
  , ";length="++show (witnessInputLength witness)
  , ";selector="++show (witnessSelector witness) ]

-- The loader has acted and no file operation is pending. A nonzero status is
-- a failed file operation, the loader's own or one of the script's; a length
-- word with the statement index at END is a parsed and completed script;
-- selector 2 with a zero length word is the executor started over a
-- zero-length parse, which is an oversized or empty file.
witnessSettled :: Witness -> Bool
witnessSettled witness =
  witnessGuard witness == 1
    && witnessInFlight witness == 0
    && ( witnessStatus witness /= 0
      || (witnessInputLength witness /= 0 && witnessStatementIndex witness == -1)
      || (witnessSelector witness == 2 && witnessInputLength witness == 0) )

-- The boot state: the sender has not run, the loader has not cleared the
-- script area and no file operation has touched the async block.
baselineClear :: Witness -> Bool
baselineClear witness = all (== 0)
  [ witnessGuard witness, witnessSentinel witness, witnessStatementIndex witness
  , witnessStatementCount witness, witnessStatementCode witness, witnessInFlight witness
  , witnessStatus witness, witnessOpenResult witness, witnessCompletionCount witness
  , witnessInputLength witness, witnessSelector witness ]

-- One poll per second, ninety polls: the corrected delay unit puts the
-- sender about 5 s after the task starts (E-AUTOACT-UNIT-150), which
-- follows the image-5 load inside the boot, so the cap is far larger than
-- the run needs; it is unchanged with the operation's other bytes.
watchPollCap :: Int
watchPollCap = 90

watchSpacingMs :: Int
watchSpacingMs = 1000

data ArmOutcome
  = ArmNoSend
  | ArmLoadFailed
  | ArmLoadRunning
  | ArmRanToEnd
  | ArmFileOpFailed
  | ArmNoLoad
  deriving (Eq, Show, Read)

armOutcomeName :: ArmOutcome -> String
armOutcomeName ArmNoSend = "no-send"
armOutcomeName ArmLoadFailed = "load-failed"
armOutcomeName ArmLoadRunning = "load-running"
armOutcomeName ArmRanToEnd = "ran-to-end"
armOutcomeName ArmFileOpFailed = "file-op-failed"
armOutcomeName ArmNoLoad = "no-load"

-- The guard separates a boot whose gate test saw the byte from one whose
-- test did not; the statement index and the count separate a script that
-- reached END from a load that never did; only then does the status separate
-- a completed script whose own file operation failed from a failed load; the
-- selector separates a running or stalled script from a settled one.
deriveOutcome :: Witness -> ArmOutcome
deriveOutcome witness
  | witnessGuard witness /= 1 = ArmNoSend
  | witnessStatementIndex witness == -1 && witnessStatementCount witness > 0 =
      if witnessStatus witness == 0 then ArmRanToEnd else ArmFileOpFailed
  | witnessStatus witness /= 0 = ArmLoadFailed
  | witnessSelector witness == 2 = ArmLoadRunning
  | otherwise = ArmNoLoad

data ArmReport = ArmReport
  { reportFirstTick :: CameraTick
  , reportEndTick :: CameraTick
  , reportOutcome :: ArmOutcome
  , reportWitness :: Witness
  , reportSettled :: Bool
  , reportPolls :: Int
  } deriving (Eq, Show, Read)

armReportLine :: ArmReport -> String
armReportLine report = concat
  [ armOutcomeName (reportOutcome report)
  , ";guard="++show (witnessGuard witness)
  , ";length="++show (witnessInputLength witness)
  , ";selector="++show (witnessSelector witness)
  , ";status="++show (witnessStatus witness)
  , ";open="++show (witnessOpenResult witness)
  , ";completion-count="++show (witnessCompletionCount witness)
  , ";index="++show (witnessStatementIndex witness)
  , ";count="++show (witnessStatementCount witness)
  , ";settled="++(if reportSettled report then "yes" else "no")
  , ";polls="++show (reportPolls report)
  , ";tick-first="++show (unCameraTick (reportFirstTick report))
  , ";uptime-first-s="++tickSeconds (reportFirstTick report)
  , ";tick-end="++show (unCameraTick (reportEndTick report))
  , ";uptime-end-s="++tickSeconds (reportEndTick report)
  ]
  where witness = reportWitness report

-- Two tick reads accompany the preconditions, arm, watch and cleanup.
armAutoActFixedRequests :: Int
armAutoActFixedRequests = 17

armAutoActRequestCount :: Int
armAutoActRequestCount = armAutoActFixedRequests + 2*watchPollCap

armAutoActAggregateDeadlineMs :: Int
armAutoActAggregateDeadlineMs = transferTimeoutMs*(sessionFixedTransfers + transfersPerRequest*armAutoActRequestCount)

armAutoActJournalFixedBytes, armAutoActJournalPerRequestBytes, armAutoActJournalCapBytes :: Int
armAutoActJournalFixedBytes = 1048576
armAutoActJournalPerRequestBytes = 4096
armAutoActJournalCapBytes = armAutoActJournalFixedBytes + armAutoActJournalPerRequestBytes*armAutoActRequestCount

-- The 8-byte C0 reply's constant tail: a zero halfword and 65536
-- little-endian (E-DBG-C0-FIELD-150).
openReplyTail :: [Word8]
openReplyTail = [0,0,0,0,1,0]

data ArmLink = ArmLink { linkSession :: Session, linkSid :: Int, linkInput :: Int, linkOutput :: Int }

-- The page-0 config window carries both adjustment bytes this command
-- touches, in-page offsets 0xa2 and 0xf7.
pageByte :: Int -> [Word8] -> Word8
pageByte index window = window !! index

armRefuse :: Session -> String -> [Word8] -> String -> IO a
armRefuse s label response reason = do
  note s "arm-auto-act-refused" (label++";reply="++P.hexBytes response++";"++reason)
  P.reject reason

armPrecondition :: Session -> String -> IO a
armPrecondition s reason = do
  note s "arm-auto-act-precondition" reason
  P.reject ("precondition failure: "++reason)

data DeviceStart = RequirePresent | WaitForDevice
  deriving (Eq, Show)

deviceWaitBoundMs, deviceWaitSpacingMs :: Int
deviceWaitBoundMs = 120000
deviceWaitSpacingMs = 50

newtype CameraTick = CameraTick { unCameraTick :: Word32 }
  deriving (Eq, Show, Read)

tickSeconds :: CameraTick -> String
tickSeconds (CameraTick tick) = printf "%.3f" (fromIntegral tick / 99.88 :: Double)

acquireArmDevice :: DeviceStart -> Session -> Held -> IO Candidate
acquireArmDevice RequirePresent s held = acquireDevice s held
acquireArmDevice WaitForDevice s held = do
  start <- nowNS (sessionClock s)
  let deadline = start + fromIntegral deviceWaitBoundMs*1000000
      waiting = s { sessionDeadline = deadline }
      select = do
        tick <- nowNS (sessionClock s)
        when (tick >= deadline) (P.reject "wait-for-device bound expired after 120 s; no camera requests sent")
        devices <- bounded waiting "wait-list" (usbList (sessionApi s))
        case selectDevice devices of
          Right device -> pure device
          Left NoEligibleDevice -> do
            ms <- remaining (sessionClock s) deadline
            threadDelay (min deviceWaitSpacingMs ms*1000)
            select
          Left refusal -> P.reject (refusalMessage refusal)
  note s "waiting-for-device" ("04cb:ff80;bound-ms="++show deviceWaitBoundMs++";poll-spacing-ms="++show deviceWaitSpacingMs)
  hPutStrLn stderr "arm-auto-act: waiting for 04cb:ff80, up to 120 s; power the camera into maintenance mode now"
  hFlush stderr
  acquireDeviceSelecting waiting held select

readTick :: ArmLink -> String -> IO CameraTick
readTick link label = do
  bytes <- ramHalf link label 0x400c0800
  let tick = CameraTick (fromIntegral (word32At 0 bytes))
  note (linkSession link) "camera-tick" (label++";address=0x400c0800;tick="++show (unCameraTick tick)++";uptime-s="++tickSeconds tick++";rate-hz=99.88")
  pure tick

runArmAutoAct :: DeviceStart -> Clock -> UsbApi -> FilePath -> IO ArmReport
runArmAutoAct mode clock api dir = runArmAutoActCore mode clock api dir `catch` \(e::SomeException) -> do
  _ <- try (recordWithin armAutoActJournalCapBytes dir "session-uncertain" (take 4096 (displayException e))) :: IO (Either SomeException ())
  throwIO e

runArmAutoActCore :: DeviceStart -> Clock -> UsbApi -> FilePath -> IO ArmReport
runArmAutoActCore mode clock api dir = do
  writes <- either P.reject pure armWrites
  s <- newSession clock api dir armAutoActJournalCapBytes (armAutoActAggregateDeadlineMs + if mode == WaitForDevice then deviceWaitBoundMs else 0)
  activeSession <- newIORef s
  held <- newHeld
  openedSession <- newIORef Nothing
  gateOpened <- newIORef False
  let captured = dir </> "witness"
  note s "prepared" ("arm-auto-act;arm-index=0x"++showHex (writeTargetIndex (armWriteTarget writes)) ""
                     ++";arm-value=0x"++hexByte armValueByte
                     ++";gate-index=0x"++showHex (writeTargetIndex (gateWriteTarget writes)) ""
                     ++";windows=0x"++probeName witnessWindowA++",0x"++probeName witnessWindowB
                     ++";watch-cap="++show watchPollCap++";watch-spacing-ms="++show watchSpacingMs)
  ensurePrivateDirectory captured
  mask $ \restore -> do
    result <- try $ restore $ do
      device <- acquireArmDevice mode s held
      activeStart <- nowNS clock
      let active = s { sessionDeadline = activeStart + fromIntegral armAutoActAggregateDeadlineMs*1000000 }
      writeIORef activeSession active
      let input = inputEndpoint (descriptors device)
          output = outputEndpoint (descriptors device)
      _ <- tx active (P.controlTransfer (P.control 0xc0 8 0)) 12
      reply <- tx active (P.BulkIn input 8) 8
      let replyTail = drop 2 reply
      unless (replyTail == openReplyTail)
        (armRefuse s "open" reply ("the C0 reply tail "++P.hexBytes replyTail++" differs from the constant "++P.hexBytes openReplyTail))
      let sid = fromIntegral (reply!!0) + 256*fromIntegral (reply!!1)
          link = ArmLink active sid input output
      writeIORef openedSession (Just (device,sid))
      -- Both adjustment bytes this session writes must hold the boot value
      -- 0x00 before the first write, so the restore's 0x00 is a value this
      -- read established.
      start <- windowRead link "precondition"
      unless (pageByte (writeTargetIndex ArmByte) start == 0)
        (armPrecondition s ("adjustment index 0x"++showHex (writeTargetIndex ArmByte) ""
                            ++" reads 0x"++hexByte (pageByte (writeTargetIndex ArmByte) start)
                            ++" at session start; the boot value is 0x00"))
      unless (pageByte (writeTargetIndex GateByte) start == 0)
        (armPrecondition s ("adjustment index 0x"++showHex (writeTargetIndex GateByte) ""
                            ++" reads 0x"++hexByte (pageByte (writeTargetIndex GateByte) start)
                            ++" at session start; the value every capture shows is 0x00"))
      writeIORef gateOpened True
      configWrite link (gateWriteTarget writes) gateEnabledByte
      enabled <- windowRead link gateReadbackEnabled
      unless (pageByte (writeTargetIndex GateByte) enabled == gateEnabledByte)
        (armRefuse s gateReadbackEnabled enabled "the gate enable is not visible in the config readback")
      firstTick <- readTick link "first"
      beforeA <- concat <$> mapM (ramHalf link "before-a") (witnessHalves witnessWindowA)
      beforeB <- concat <$> mapM (ramHalf link "before-b") (witnessHalves witnessWindowB)
      let before = witnessOf beforeA beforeB
      unless (baselineClear before)
        (armPrecondition s ("the witness cells are not the boot state before the arm: "++witnessSummary before))
      configWrite link (armWriteTarget writes) armValueByte
      writePrivateBytes (dir </> "precondition.bin") (B.pack start)
      note s "precondition-window" ("sha256="++sha256 (B.pack start)++";arm=0x00;gate=0x00")
      writePrivateBytes (dir </> (gateReadbackEnabled++".bin")) (B.pack enabled)
      note s "readback-complete" (gateReadbackEnabled++";sha256="++sha256 (B.pack enabled))
      forM_ [("before",witnessWindowA,beforeA),("before",witnessWindowB,beforeB)] $ \(label,base,bytes) -> do
        writePrivateBytes (captured </> (label++"-"++probeName base++".bin")) (B.pack bytes)
        note s "witness-window" (label++";window=0x"++probeName base++";sha256="++sha256 (B.pack bytes))
      note s "witness-cells" ("before;"++witnessSummary before)
      confirmed <- windowRead link "arm-readback"
      writePrivateBytes (dir </> "arm-readback.bin") (B.pack confirmed)
      note s "arm-confirmed" ("sha256="++sha256 (B.pack confirmed)
                              ++";index=0x"++showHex (writeTargetIndex (armWriteTarget writes)) ""
                              ++";value=0x"++hexByte (pageByte (writeTargetIndex ArmByte) confirmed))
      unless (pageByte (writeTargetIndex ArmByte) confirmed == armValueByte)
        (armRefuse s "arm-readback" confirmed "the arm write is not visible in the config readback")
      polls <- watchForFire link
      after <- readWitnesses link dir "after"
      endTick <- readTick link "end"
      let report = ArmReport firstTick endTick (deriveOutcome after) after (witnessSettled after) polls
      note s "arm-auto-act-observed" (armReportLine report)
      pure report
    -- The gate restore and its readback run on every exit path, including a
    -- stop, and the protocol close follows them. An unwritten gate is not
    -- restored.
    cleanupSession <- readIORef activeSession
    errors <- newIORef ([] :: [String])
    let attempt label action = action `catch` \(e::SomeException) -> modifyIORef' errors (++[label++": "++displayException e])
    opened <- readIORef openedSession
    case opened of
      Nothing -> note cleanupSession "cleanup-unavailable" "unknown SID; no invented C4"
      Just (device,sid) -> do
        let link = ArmLink cleanupSession sid (inputEndpoint (descriptors device)) (outputEndpoint (descriptors device))
        didWriteGate <- readIORef gateOpened
        when didWriteGate $ do
          attempt "gate restore" (configWrite link (gateWriteTarget writes) gateDisabledByte)
          attempt "gate restore readback" $ do
            payload <- windowRead link gateReadbackRestored
            writePrivateBytes (dir </> (gateReadbackRestored++".bin")) (B.pack payload)
            note s "readback-complete" (gateReadbackRestored++";sha256="++sha256 (B.pack payload))
            unless (pageByte (writeTargetIndex GateByte) payload == gateDisabledByte)
              (P.reject "the gate restore is not visible in the config readback")
        attempt "protocol close" $ do
          _ <- tx cleanupSession (P.controlTransfer (P.control 0xc4 2 sid)) 12
          _ <- tx cleanupSession (P.BulkIn (inputEndpoint (descriptors device)) 2) 2
          note cleanupSession "close-reply" "opaque bytes; stock restoration unobserved"
    releaseHeld held api (\label action -> attempt label (bounded cleanupSession label action))
    failures <- readIORef errors
    case (result,failures) of
      (_,_:_) -> P.reject ("cleanup uncertain: "++show failures)
      (Left (e::SomeException),[]) -> throwIO e
      (Right report,[]) -> do
        note s "arm-auto-act-complete-stock-unverified" (armReportLine report)
        pure report

-- One config write: the D0 control, the 37-byte frame, the D1 control and
-- the reply.
configWrite :: ArmLink -> WriteTarget -> Word8 -> IO ()
configWrite link target value = do
  let s = linkSession link
  _ <- tx s (P.controlTransfer (P.control 0xd0 configWriteBytes (linkSid link))) 12
  _ <- tx s (P.BulkOut (linkOutput link) (configWriteFrame target value)) configWriteBytes
  _ <- tx s (P.controlTransfer (P.control 0xd1 configReplyBytes (linkSid link))) 12
  bytes <- tx s (P.BulkIn (linkInput link) configReplyBytes) configReplyBytes
  let label = "config-write-"++writeTargetName target
  either (armRefuse s label bytes) pure (decodeConfigWriteReply target (0,0) bytes)
  note s "config-write-complete" (writeTargetName target++";value=0x"++hexByte value++";status=(0,0)")

-- The page-0 window read of the bounded config path: page 0, in-page offset
-- 0, 256 bytes, data at reply +0x24.
windowRead :: ArmLink -> String -> IO [Word8]
windowRead link label = do
  let s = linkSession link
      window = ReadWindow readbackIndex 0 label
  _ <- tx s (P.controlTransfer (P.control 0xd0 readRequestBytes (linkSid link))) 12
  _ <- tx s (P.BulkOut (linkOutput link) (readRequestFrame window)) readRequestBytes
  _ <- tx s (P.controlTransfer (P.control 0xd1 readReplyBytes (linkSid link))) 12
  bytes <- tx s (P.BulkIn (linkInput link) readReplyBytes) readReplyBytes
  either (armRefuse s label bytes) pure (decodeReadReply window bytes)

-- One 128-byte RAM read of the witness windows.
ramHalf :: ArmLink -> String -> Int -> IO [Word8]
ramHalf link label address = do
  let s = linkSession link
  _ <- tx s (P.controlTransfer (P.control 0xd0 debugRequestBytes (linkSid link))) 12
  _ <- tx s (P.BulkOut (linkOutput link) (debugRequestFrame address)) debugRequestBytes
  _ <- tx s (P.controlTransfer (P.control 0xd1 debugReplyForHalf (linkSid link))) 12
  bytes <- tx s (P.BulkIn (linkInput link) debugReplyForHalf) debugReplyForHalf
  either (armRefuse s label bytes) pure (decodeDebugReply address bytes)

-- One full 256-byte witness window, two halves, captured under witness/.
windowBytes :: ArmLink -> FilePath -> String -> Int -> IO [Word8]
windowBytes link dir label base = do
  let s = linkSession link
  halves <- mapM (ramHalf link (label++"-"++probeName base)) (witnessHalves base)
  let bytes = concat halves
  writePrivateBytes (dir </> "witness" </> (label++"-"++probeName base++".bin")) (B.pack bytes)
  note s "witness-window" (label++";window=0x"++probeName base++";sha256="++sha256 (B.pack bytes))
  pure bytes

-- Both witness windows as the pair witnessOf reads.
readWitnesses :: ArmLink -> FilePath -> String -> IO Witness
readWitnesses link dir label = do
  windowA <- windowBytes link dir label witnessWindowA
  windowB <- windowBytes link dir label witnessWindowB
  let cells = witnessOf windowA windowB
  note (linkSession link) "witness-cells" (label++";"++witnessSummary cells)
  pure cells

-- The watch: the guard byte and the loader's async block, one poll per
-- spacing, bounded by the cap. The first poll is immediate, so a run whose
-- send already happened settles at once. The poll count is returned; the
-- settled state is the final read's own verdict.
pollWitness :: ArmLink -> Int -> IO Witness
pollWitness link poll = do
  let s = linkSession link
  halfA <- ramHalf link ("guard-poll-"++show poll) witnessWindowA
  halfB <- ramHalf link ("loader-poll-"++show poll) (witnessWindowB + probeHalfBytes)
  let cells = witnessOf (halfA ++ replicate probeHalfBytes 0) (replicate probeHalfBytes 0 ++ halfB)
  note s "watch-poll" ("poll="++show poll++";"++witnessSummary cells)
  pure cells

watchForFire :: ArmLink -> IO Int
watchForFire link = do
  first <- pollWitness link 0
  if witnessSettled first then finish 1 else go 1
  where
    s = linkSession link
    finish polls = do
      note s "watch-settled" ("poll="++show (polls-1))
      pure polls
    go poll
      | poll >= watchPollCap = do
          note s "watch-bound-expired" (show watchPollCap++" polls; the run reports the last observed state")
          pure poll
      | otherwise = do
          threadDelay (watchSpacingMs*1000)
          cells <- pollWitness link poll
          if witnessSettled cells then finish (poll+1) else go (poll+1)
