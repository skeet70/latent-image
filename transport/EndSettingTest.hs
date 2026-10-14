module Main where

import Control.Exception (SomeException, bracket, displayException, try)
import Control.Monad (forM_, unless, when)
import Data.Bits (shiftL, xor)
import Data.Either (isLeft)
import Data.IORef
import Data.List (isInfixOf)
import Data.Word (Word8, Word64)
import System.Directory
import System.FilePath ((</>))
import System.Posix.Process (getProcessID)
import FujiAF.EndSetting
import FujiAF.Transport
import qualified FujiAF.Broker as P

check :: String -> Bool -> IO ()
check label yes = unless yes (error label)

syntheticDescriptors :: Descriptors
syntheticDescriptors = Descriptors 1 0 0 255 0 0 0x81 2 512 512 0 1 []

literalRequest :: [Word8]
literalRequest =
  [ 0x01,0x00,0x20,0x00
  , 0,0,0,0,0,0,0,0,0,0,0,0
  , 0x05,0,0,0
  , 0,0,0,0,0,0,0,0,0,0,0,0
  , 0x0a,0,0,0
  , 0x50,0x4f,0x57,0x45,0x52,0x20,0x4f,0x46,0x46,0x0a ]

literalPlan :: [P.Transfer]
literalPlan =
  [ P.ControlWrite 0x43 0 0 0 [0xc0,0,0,0,0,0,0,0,0,8,0,0]
  , P.BulkIn 0x81 8
  , P.ControlWrite 0x43 0 0 0 [0xd0,0,0,0,0,0,0,0,0,0x2e,0x12,0x34]
  , P.BulkOut 2 literalRequest
  , P.ControlWrite 0x43 0 0 0 [0xd1,0,0,0,0,0,0,0,0,0x80,0x12,0x34]
  , P.BulkIn 0x81 128 ]

wordAtLE :: Int -> [Word8]
wordAtLE value = [fromIntegral (value `div` (256 ^ power)) | power <- [0..3 :: Int]]

replyWith :: Int -> Int -> [Word8]
replyWith first second =
  [0x01,0,0x20,0] ++ wordAtLE first ++ wordAtLE second ++ [0,0,0,0,5,0,0,0] ++ replicate 108 0

replaceAt :: Int -> a -> [a] -> [a]
replaceAt at value values = take at values ++ [value] ++ drop (at+1) values

mutateAt :: Int -> [Word8] -> [Word8]
mutateAt at bytes = replaceAt at (bytes !! at `xor` 1) bytes

data TransferFailure = TransferTimeout | DeviceLost | TransferCancelled deriving (Eq, Show)
data Response
  = Returned Int [Word8]
  | TransferFailed TransferFailure
  | ClockAdvanced Word64 Int [Word8]
  deriving (Eq, Show)
data HostStage = Initialize | ListDevices | OpenDevice | ClaimInterface | ReleaseInterface | CloseHandle | ExitContext
  deriving (Eq, Show)
data Event = HostEvent HostStage Int | CameraEvent P.Transfer Int deriving (Eq, Show)
data Expected = Completed EndSettingOutcome | Uncertain deriving (Eq, Show)
data TestCase = TestCase
  { caseLabel :: String
  , caseResponses :: [Response]
  , caseCleanupFailures :: [HostStage]
  , caseExpected :: Expected
  , caseTransferCount :: Int
  }

responsesFor :: [Word8] -> [Response]
responsesFor bytes =
  [ Returned 12 []
  , Returned 8 [0x34,0x12,0,0,0,0,1,0]
  , Returned 12 []
  , Returned 46 []
  , Returned 12 []
  , Returned (length bytes) bytes ]

expectedEvents :: Int -> [Event]
expectedEvents count =
  map (`HostEvent` transferTimeoutMs) [Initialize,ListDevices,OpenDevice,ClaimInterface]
  ++ map (`CameraEvent` transferTimeoutMs) (take count literalPlan)
  ++ map (`HostEvent` transferTimeoutMs) [ReleaseInterface,CloseHandle,ExitContext]

assertOutcome :: String -> Expected -> Either SomeException EndSettingOutcome -> IO ()
assertOutcome label expected actual = case (expected,actual) of
  (Completed wanted,Right observed) -> check (label++": decoded outcome") (wanted == observed)
  (Uncertain,Left _) -> pure ()
  (_,Left failure) -> error (label++": "++displayException failure)
  (_,Right observed) -> error (label++": unexpected outcome "++show observed)

runCase :: FilePath -> TestCase -> IO ()
runCase = runCaseWithClock (\_ tick -> Clock (readIORef tick))

runCaseWithClock :: (FilePath -> IORef Word64 -> Clock) -> FilePath -> TestCase -> IO ()
runCaseWithClock makeClock base testCase = do
  createDirectory base
  let dir = base </> "session"
      label = caseLabel testCase
  ensurePrivateDirectory dir
  events <- newIORef []
  script <- newIORef (zip literalPlan (caseResponses testCase))
  tick <- newIORef 0
  let host stage milliseconds = do
        modifyIORef' events (++[HostEvent stage milliseconds])
        check (label++": finite host timeout") (milliseconds > 0 && milliseconds <= transferTimeoutMs)
        when (stage `elem` caseCleanupFailures testCase) (P.reject ("injected "++show stage++" failure"))
      candidate = Candidate "001:2.3" 0x04cb 0xff80 syntheticDescriptors
      transfer packet milliseconds = do
        modifyIORef' events (++[CameraEvent packet milliseconds])
        check (label++": finite camera timeout") (milliseconds > 0 && milliseconds <= transferTimeoutMs)
        case packet of
          P.ControlWrite _ _ _ _ bytes -> assertOutgoing packet bytes
          P.BulkOut _ bytes -> assertOutgoing packet bytes
          P.BulkIn _ _ -> pure ()
        remainingSteps <- readIORef script
        case remainingSteps of
          [] -> P.reject "unexpected extra camera transfer"
          (expected,response):rest -> do
            check (label++": exact ordered camera transfer") (packet == expected)
            writeIORef script rest
            case response of
              Returned count bytes -> pure (count,bytes)
              TransferFailed failure -> P.reject ("injected "++show failure)
              ClockAdvanced advance count bytes -> modifyIORef' tick (+advance) >> pure (count,bytes)
      assertOutgoing packet bytes = do
        outgoing <- readFile (dir </> "outgoing.v1")
        check (label++": outgoing bytes durable before dispatch")
          (show packet `isInfixOf` outgoing && P.hexBytes bytes `isInfixOf` outgoing)
      api = UsbApi
        (host Initialize)
        (\milliseconds -> host ListDevices milliseconds >> pure [candidate])
        (\device milliseconds -> check (label++": exact selected device") (device == candidate) >> host OpenDevice milliseconds)
        (\interface milliseconds -> check (label++": exact claimed interface") (interface == 0) >> host ClaimInterface milliseconds)
        transfer
        (\interface milliseconds -> check (label++": exact released interface") (interface == 0) >> host ReleaseInterface milliseconds)
        (host CloseHandle)
        (host ExitContext)
      clock = makeClock dir tick
  result <- try (runEndSetting clock api dir)
  assertOutcome label (caseExpected testCase) result
  observedEvents <- readIORef events
  check (label++": six-transfer plan prefix and host cleanup exactly once")
    (observedEvents == expectedEvents (caseTransferCount testCase))
  remainingScript <- readIORef script
  check (label++": no retry or skipped scripted transfer")
    (length remainingScript == 6-caseTransferCount testCase)
  when (caseTransferCount testCase == 6) $ case last (caseResponses testCase) of
    Returned _ bytes -> assertRawReply dir label bytes
    ClockAdvanced _ _ bytes -> assertRawReply dir label bytes
    TransferFailed _ -> pure ()

intentDeadlineTest :: FilePath -> IO ()
intentDeadlineTest base = runCaseWithClock intentClock base
  (TestCase "aggregate deadline crossed during durable dispatch intent" (responsesFor (replyWith 0 0)) [] Uncertain 0)
  where
    intentClock dir tick = Clock $ do
      let path = dir </> "journal.v1"
          intent = show ("stage-intent" :: String) ++ "," ++ show ("dispatch " ++ show (P.controlTransfer (P.control 0xc0 8 0)))
      exists <- doesPathExist path
      when exists $ do
        journal <- readFile path
        length journal `seq` when (intent `isInfixOf` journal) (writeIORef tick (fromIntegral endAggregateDeadlineMs*1000000))
      readIORef tick

assertRawReply :: FilePath -> String -> [Word8] -> IO ()
assertRawReply dir label bytes = do
  journal <- readFile (dir </> "journal.v1")
  check (label++": raw reply journaled") (P.hexBytes bytes `isInfixOf` journal)

packetTests :: IO ()
packetTests = do
  check "literal request, no trailing bytes" (endRequestPayload == literalRequest && endRequestBytes == 46 && length literalRequest == 46)
  check "literal text length and bytes" (drop 36 endRequestPayload == [0x50,0x4f,0x57,0x45,0x52,0x20,0x4f,0x46,0x46,0x0a])
  check "reply length" (endReplyBytes == 128)
  check "aggregate includes acquisition, transfers and host cleanup" (endAggregateDeadlineMs == transferTimeoutMs*13)
  check "fixed journal bound" (endJournalCapBytes == 65536)
  let packets = buildEndSettingPackets (EndSettingSessionId 0x1234) syntheticDescriptors
  check "literal D0 control" (endD0Control packets == literalPlan !! 2)
  check "literal D0 bulk" (endD0Bulk packets == literalPlan !! 3)
  check "literal D1 control" (endD1Control packets == literalPlan !! 4)
  check "literal D1 bulk" (endD1Bulk packets == literalPlan !! 5)
  forM_ [0,1,0x00ff,0x0100,0xffff] $ \sid -> do
    let built = buildEndSettingPackets (EndSettingSessionId sid) syntheticDescriptors
        sidBytes = [fromIntegral (sid `div` 256),fromIntegral sid]
        control opcode size = P.ControlWrite 0x43 0 0 0 ([opcode,0,0,0,0,0,0,0,0,size]++sidBytes)
    check "SID is big endian in both control frames"
      (endD0Control built == control 0xd0 46 && endD1Control built == control 0xd1 128)
    check "SID does not affect bulk request" (endD0Bulk built == P.BulkOut 2 literalRequest)
  let alternate = buildEndSettingPackets (EndSettingSessionId 0x1234)
        syntheticDescriptors{inputEndpoint=0x83,outputEndpoint=4}
  check "bulk endpoints use the live descriptors"
    (endD0Bulk alternate == P.BulkOut 4 literalRequest && endD1Bulk alternate == P.BulkIn 0x83 128)

malformedReplies :: [(String,[Word8])]
malformedReplies =
  [ ("wrong "++field++" byte "++show offset++" on "++show outcome, mutateAt offset (replyWith first second))
  | (outcome,first,second) <- [(EndSettingAccepted,0,0),(EndSettingRejected,1,3)]
  , (field,offsets) <- [("opcode",[0..3]),("reserved word",[12..15]),("function",[16..19])]
  , offset <- offsets ]
  ++ [ ("unsupported error pair "++show (first,second),replyWith first second)
     | first <- [0,1,3,9,0xffff]
     , second <- [0,1,3,9,0xffff]
     , (first,second) `notElem` [(0,0),(1,3)] ]

parserTests :: IO ()
parserTests = do
  check "accepted decode" (decodeEndSettingReply (replyWith 0 0) == Right EndSettingAccepted)
  check "rejected decode" (decodeEndSettingReply (replyWith 1 3) == Right EndSettingRejected)
  forM_ [(EndSettingAccepted,0,0),(EndSettingRejected,1,3)] $ \(wanted,first,second) ->
    check "opaque reply tail is not an extra echo"
      (decodeEndSettingReply (take 20 (replyWith first second) ++ [1..108]) == Right wanted)
  forM_ malformedReplies $ \(label,bytes) -> check label (isLeft (decodeEndSettingReply bytes))
  forM_ [0..127] $ \size -> check ("short reply length "++show size) (isLeft (decodeEndSettingReply (take size (replyWith 0 0))))
  check "oversized reply" (isLeft (decodeEndSettingReply (replyWith 0 0++[0])))
  forM_ [0..31] $ \bit -> do
    check "single-bit first error is unsupported" (isLeft (decodeEndSettingReply (replyWith (1 `shiftL` bit) 0)))
    check "single-bit second error is unsupported" (isLeft (decodeEndSettingReply (replyWith 0 (1 `shiftL` bit))))
  forM_ [(0,0),(1,3)] $ \(first,second) -> do
    let original = replyWith first second
    forM_ ([0..3]++[12..19]) $ \offset ->
      forM_ [0..255 :: Int] $ \value ->
        when (fromIntegral value /= original !! offset) $
          check "every one-byte opcode/reserved/function mutation is refused"
            (isLeft (decodeEndSettingReply (replaceAt offset (fromIntegral value) original)))

runtimeCases :: [TestCase]
runtimeCases =
  [ complete "accepted" EndSettingAccepted (replyWith 0 0)
  , complete "rejected" EndSettingRejected (replyWith 1 3)
  , complete "accepted opaque tail" EndSettingAccepted (take 20 (replyWith 0 0) ++ [1..108])
  , complete "rejected opaque tail" EndSettingRejected (take 20 (replyWith 1 3) ++ [1..108]) ]
  ++ [ TestCase label (responsesFor bytes) [] Uncertain 6 | (label,bytes) <- malformedReplies ]
  ++ [ TestCase (show fault++" at transfer "++show index)
         (replaceAt index (TransferFailed fault) accepted) [] Uncertain (index+1)
     | fault <- [TransferTimeout,DeviceLost,TransferCancelled], index <- [0..5] ]
  ++ [ TestCase (sizeLabel++" transfer "++show index)
         (replaceAt index (Returned (count+delta) (resize (count+delta) bytes)) accepted) [] Uncertain (index+1)
     | (index,Returned count bytes) <- zip [0..] accepted
     , (sizeLabel,delta) <- [("short",-1),("oversized",1)] ]
  ++ [ TestCase ("count/bytes mismatch at transfer "++show index)
         (replaceAt index (Returned count wrongBytes) accepted) [] Uncertain (index+1)
     | (index,Returned count bytes) <- zip [0..] accepted
     , let wrongBytes = if null bytes then [0] else init bytes ]
  ++ [ TestCase ("transfer deadline at "++show index)
         (replaceAt index (ClockAdvanced (fromIntegral transferTimeoutMs*1000000+1000000) count bytes) accepted) [] Uncertain (index+1)
     | (index,Returned count bytes) <- zip [0..] accepted ]
  ++ [ TestCase ("aggregate deadline at "++show index)
         (replaceAt index (ClockAdvanced (fromIntegral endAggregateDeadlineMs*1000000) count bytes) accepted) [] Uncertain (index+1)
     | (index,Returned count bytes) <- zip [0..] accepted ]
  ++ [ TestCase ("cleanup failure "++show stages++" after "++show outcome)
         (responsesFor (replyWith first second)) stages Uncertain 6
     | stages <- [[ReleaseInterface],[CloseHandle],[ExitContext],[ReleaseInterface,CloseHandle,ExitContext]]
     , (outcome,first,second) <- [(EndSettingAccepted,0,0),(EndSettingRejected,1,3)] ]
  ++ [ TestCase "cleanup failure after device loss"
         (replaceAt 3 (TransferFailed DeviceLost) accepted) [ReleaseInterface,CloseHandle,ExitContext] Uncertain 4
     , TestCase "cleanup failure after malformed reply"
         (responsesFor (replyWith 9 3)) [ReleaseInterface,CloseHandle,ExitContext] Uncertain 6
     , TestCase "short final reply" (responsesFor (take 127 (replyWith 0 0))) [] Uncertain 6
     , TestCase "oversized final reply" (responsesFor (replyWith 0 0++[0])) [] Uncertain 6
     , TestCase "C0 tail below the constant" (replaceAt 1 (Returned 8 [0x34,0x12,0,0,0,0,0,127]) accepted) [] Uncertain 2
     , TestCase "C0 tail above the constant" (replaceAt 1 (Returned 8 [0x34,0x12,0,0,0,0x10,0,1]) accepted) [] Uncertain 2
     , TestCase "C0 tail byte-swapped" (replaceAt 1 (Returned 8 [0x34,0x12,0,0,0,1,0,0]) accepted) [] Uncertain 2 ]
  where
    accepted = responsesFor (replyWith 0 0)
    complete label outcome bytes = TestCase label (responsesFor bytes) [] (Completed outcome) 6
    resize count bytes = if null bytes then [] else take count (bytes++repeat 0)

withTestDirectory :: String -> (FilePath -> IO a) -> IO a
withTestDirectory name action = do
  process <- getProcessID
  temporary <- getTemporaryDirectory
  let base = temporary </> (name++"-"++show process)
  bracket (createDirectory base >> pure base) removeDirectoryRecursive action

main :: IO ()
main = withTestDirectory "fuji-af-end-setting-tests" $ \base -> do
  packetTests
  parserTests
  intentDeadlineTest (base </> "intent-deadline")
  forM_ (zip [(0::Int)..] runtimeCases) $ \(index,testCase) ->
    runCase (base </> ("case-"++show index)) testCase
  putStrLn "End Setting: literal packets, reply safety boundaries, finite deadlines, no retries, no C4 and host cleanup checks passed"
