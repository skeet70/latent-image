module Main where
import Control.Exception
import Control.Monad (unless, forM_)
import Data.List (isInfixOf)
import qualified Data.ByteString as B
import Data.IORef
import Data.Word (Word8, Word64)
import System.Directory
import System.FilePath
import System.Posix.Process (getProcessID)
import FujiAF.Transport
import FujiAF.Read
import qualified FujiAF.Broker as P

check :: String -> Bool -> IO ()
check label yes = unless yes (error label)

syntheticDescriptors :: Descriptors
syntheticDescriptors = Descriptors 1 0 0 255 0 0 0x81 2 512 512 0 1 []

syntheticWindows :: [ReadWindow]
syntheticWindows = [ReadWindow 0 0x000000 "page 0", ReadWindow 1 0x000100 "product string", ReadWindow 2 0x00ac00 "colour matrix"]

syntheticPlan :: ReadPlan
syntheticPlan = ReadPlan "synthetic-plan" "synthetic 1.50 basis" syntheticWindows (ReadOracle 1 0x5c) "synthetic generator" (replicate 64 '0')

-- Synthetic reply header: opcode/status/reserved at 0x00..0x0f,
-- function/mailbox at 0x10/0x14, page/offset/length at 0x18/0x1c/0x20.
replyFrameWith :: Int -> Int -> (Int,Int,Int) -> [Word8] -> [Word8]
replyFrameWith first second (page,offset,len) payload =
  [0x02,0x00,0x1a,0x00] ++ word32le first ++ word32le second ++ replicate 4 0
  ++ [2,0,0,0,1,0,0,0]
  ++ word32le page ++ word32le offset ++ word32le len ++ payload

-- The 0x000100 window carries the product bytes 0x5c into it.
payloadFor :: ReadWindow -> [Word8]
payloadFor window
  | windowOffset window == 0x000100 = replicate 0x5c 0 ++ productOracleBytes ++ replicate (readWindowLength-0x5c-length productOracleBytes) 0
  | otherwise = replicate readWindowLength (fromIntegral (windowIndex window + 1))

successReply :: ReadWindow -> [Word8]
successReply window = replyFrameWith 0 0 (windowOffset window `div` readWindowLength,0,readWindowLength) (payloadFor window)

sessionSteps :: [ReadWindow] -> (ReadWindow -> P.Outcome) -> [P.Step]
-- C0 bytes from S03-stopped-cap-02; configuration replies are synthetic.
sessionSteps windows replyFor =
  [ P.Step (P.controlTransfer (P.control 0xc0 8 0)) (P.Transferred 12)
  , P.Step (P.BulkIn 0x81 8) (P.Reply [0x7c,0x11,0,0,0,0,1,0]) ]
  ++ concatMap window windows
  ++ [ P.Step (P.controlTransfer (P.control 0xc4 2 0x117c)) (P.Transferred 12)
     , P.Step (P.BulkIn 0x81 2) (P.Reply [0x7c,0x11]) ]
  where
    window w =
      [ P.Step (P.controlTransfer (P.control 0xd0 readRequestBytes 0x117c)) (P.Transferred 12)
      , P.Step (P.BulkOut 0x02 (readRequestFrame w)) (P.Transferred readRequestBytes)
      , P.Step (P.controlTransfer (P.control 0xd1 readReplyBytes 0x117c)) (P.Transferred 12)
      , P.Step (P.BulkIn 0x81 readReplyBytes) (replyFor w) ]

data CleanupEvent = Released | Closed | Exited deriving (Eq, Show)

data ProviderEvent
  = InitAttempt Int | ListAttempt Int | OpenAttempt Int | ClaimAttempt Int
  | TransferAttempt P.Transfer Int | ReleaseAttempt Int | CloseAttempt Int | ExitAttempt Int
  deriving (Eq, Show)

sessionDeadlineNS :: Int -> Word64
sessionDeadlineNS windows = fromIntegral (aggregateDeadlineMs transferTimeoutMs windows)*1000000

-- C4 cleanup has its own script, independent of the completed window prefix.
runCase :: FilePath -> ReadPlan -> [P.Step] -> IO (Either SomeException Int,[P.Transfer],[CleanupEvent])
runCase dir plan steps = do
  (outcome,packets,cleanup,_) <- runCaseWithClock (Clock (pure 0)) dir plan steps
  pure (outcome,packets,cleanup)

runCaseWithClock :: Clock -> FilePath -> ReadPlan -> [P.Step] -> IO (Either SomeException Int,[P.Transfer],[CleanupEvent],[ProviderEvent])
runCaseWithClock clock dir plan steps = do
  createDirectoryIfMissing True dir
  emitted <- newIORef []
  events <- newIORef []
  providers <- newIORef []
  script <- newIORef (take (length steps-2) steps)
  closeScript <- newIORef (drop (length steps-2) steps)
  let device = Candidate "001:2.3" 0x04cb 0xff80 syntheticDescriptors
      observe event = modifyIORef' providers (++[event])
      api = UsbApi (\ms -> observe (InitAttempt ms)) (\ms -> observe (ListAttempt ms) >> pure [device])
        (\_ ms -> observe (OpenAttempt ms)) (\_ ms -> observe (ClaimAttempt ms))
        (\packet ms -> do
          observe (TransferAttempt packet ms)
          modifyIORef' emitted (++[packet])
          let activeScript = if packet == P.controlTransfer (P.control 0xc4 2 0x117c) || packet == P.BulkIn 0x81 2 then closeScript else script
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
        (\_ ms -> observe (ReleaseAttempt ms) >> modifyIORef' events (++[Released]))
        (\ms -> observe (CloseAttempt ms) >> modifyIORef' events (++[Closed]))
        (\ms -> observe (ExitAttempt ms) >> modifyIORef' events (++[Exited]))
  outcome <- try (runRead clock api plan dir)
  packets <- readIORef emitted
  cleanupEvents <- readIORef events
  providerEvents <- readIORef providers
  pure (outcome,packets,cleanupEvents,providerEvents)

data IntentBoundary = InitIntent | SessionDispatchIntent deriving (Eq, Show)

intentLabel :: IntentBoundary -> String
intentLabel InitIntent = "init"
intentLabel SessionDispatchIntent = "dispatch "++show (P.controlTransfer (P.control 0xc0 8 0))

intentClock :: FilePath -> IntentBoundary -> Word64 -> Clock
intentClock dir boundary tick = Clock $ do
  let path = dir </> "journal.v1"
      intent = show ("stage-intent" :: String) ++ "," ++ show (intentLabel boundary)
  exists <- doesPathExist path
  if not exists then pure 0 else do
    journal <- readFile path
    length journal `seq` pure (if intent `isInfixOf` journal then tick else 0)

intentDeadlineTests :: FilePath -> ReadPlan -> IO ()
intentDeadlineTests base plan = do
  let steps = sessionSteps (planWindows plan) (P.Reply . successReply)
      deadline = sessionDeadlineNS (length (planWindows plan))
      acquired = [InitAttempt budget,ListAttempt budget,OpenAttempt budget,ClaimAttempt budget]
      budget = transferTimeoutMs
  forM_ [(InitIntent,[]),(SessionDispatchIntent,acquired)] $ \(boundary,expectedProviders) -> do
    let label = intentLabel boundary
        dir = base </> case boundary of
          InitIntent -> "init-intent-deadline"
          SessionDispatchIntent -> "dispatch-intent-deadline"
    (outcome,packets,cleanupEvents,providers) <- runCaseWithClock (intentClock dir boundary deadline) dir plan steps
    check (label++": aggregate crossing at durable intent rejects before provider entry")
      (either (isInfixOf "aggregate deadline expired" . displayException) (const False) outcome
        && providers == expectedProviders && null packets && null cleanupEvents)
    journal <- readFile (dir </> "journal.v1")
    check (label++": rejected intent remains durable without stage completion")
      ((show ("stage-intent" :: String)++","++show label) `isInfixOf` journal
        && not ((show ("stage-complete" :: String)++","++show label) `isInfixOf` journal))
  let dir = base </> "intent-reduced-remaining"
      clock = intentClock dir SessionDispatchIntent (deadline-25000000)
  (outcome,_,cleanupEvents,providers) <- runCaseWithClock clock dir plan steps
  check "intent delay is outside the provider's transfer timer"
    (either (const False) (==length (planWindows plan)) outcome)
  check "provider timeout uses remaining budget after durable intent"
    (providers == acquired
      ++ [TransferAttempt packet 25 | P.Step packet _ <- steps]
      ++ [ReleaseAttempt 25,CloseAttempt 25,ExitAttempt 25])
  check "reduced remaining budget preserves independent host cleanup"
    (cleanupEvents == [Released,Closed,Exited])

data HeaderMismatch = NonzeroReserved | WrongFunction | WrongMailbox deriving (Eq, Show)

mismatchedReply :: HeaderMismatch -> ReadWindow -> [Word8]
mismatchedReply mismatch window =
  concatMap word32le [0x1a0002,0,0,reserved,function,mailbox,windowOffset window `div` 256,0,256]
  ++ payloadFor window
  where
    (reserved,function,mailbox) = case mismatch of
      NonzeroReserved -> (1,2,1)
      WrongFunction -> (0,3,1)
      WrongMailbox -> (0,2,0)

headerMismatchTests :: FilePath -> IO ()
headerMismatchTests base = do
  let target = syntheticWindows !! 2
      later = ReadWindow 3 0x00ad00 "after header refusal"
      plan = syntheticPlan{planWindows=syntheticWindows++[later]}
      expectedPackets = [packet | P.Step packet _ <- sessionSteps syntheticWindows (P.Reply . successReply)]
  forM_ [NonzeroReserved,WrongFunction,WrongMailbox] $ \mismatch -> do
    let dir = base </> show mismatch
        rawReply = mismatchedReply mismatch target
        replyFor window
          | window == target = P.Reply rawReply
          | otherwise = P.Reply (successReply window)
        reason = case mismatch of
          NonzeroReserved -> "reply reserved word is nonzero"
          WrongFunction -> "reply function differs from the request"
          WrongMailbox -> "reply mailbox differs from the request"
    (outcome,packets,cleanupEvents) <- runCase dir plan (sessionSteps (planWindows plan) replyFor)
    check (show mismatch++": non-oracle header mismatch stops without a later D0 or second C4")
      (either (isInfixOf reason . displayException) (const False) outcome && packets == expectedPackets)
    check (show mismatch++": every host resource is released once")
      (cleanupEvents == [Released,Closed,Exited])
    forM_ [target,later] $ \window -> do
      exists <- doesPathExist (dir </> "windows" </> (offsetName (windowOffset window)++".bin"))
      check (show mismatch++": refused and later windows are not captured") (not exists)
    forM_ (take 2 syntheticWindows) $ \window -> do
      bytes <- B.readFile (dir </> "windows" </> (offsetName (windowOffset window)++".bin"))
      check (show mismatch++": earlier completed windows remain captured") (B.unpack bytes == payloadFor window)
    journal <- readFile (dir </> "journal.v1")
    check (show mismatch++": raw reply and refusal remain durable, only earlier windows complete")
      (P.hexBytes rawReply `isInfixOf` journal && reason `isInfixOf` journal
        && "window-refused" `isInfixOf` journal && "session-uncertain" `isInfixOf` journal
        && length (filter ("window-complete" `isInfixOf`) (lines journal)) == 2)

main :: IO ()
main = do
  pid <- getProcessID
  tmp <- getTemporaryDirectory
  let base = tmp </> ("fuji-af-read-tests-"++show pid)
  createDirectory base
  check "read plan valid" (validatePlan syntheticPlan == Right ())
  intentDeadlineTests base syntheticPlan
  headerMismatchTests base

  (success,packets,successCleanup) <- runCase (base </> "success") syntheticPlan (sessionSteps syntheticWindows (P.Reply . successReply))
  check ("observed C0 must permit full config replies: "++show success) (either (const False) (==3) success)
  forM_ syntheticWindows $ \w -> do
    bytes <- B.readFile (base </> "success" </> "windows" </> (offsetName (windowOffset w)++".bin"))
    check "captured window bytes" (B.unpack bytes == payloadFor w)
  outgoingLog <- readFile (base </> "success" </> "outgoing.v1")
  check "every request frame is durable before dispatch"
    (all (\w -> P.hexBytes (readRequestFrame w) `isInfixOf` outgoingLog) syntheticWindows)
  successJournal <- readFile (base </> "success" </> "journal.v1")
  check "one completion record per window"
    (length (filter ("window-complete" `isInfixOf`) (lines successJournal)) == length syntheticWindows)
  check "one close attempt" (length [() | P.ControlWrite _ _ _ _ (0xc4:_) <- packets] == 1)
  check "successful read releases every resource once" (successCleanup == [Released,Closed,Exited])
  check "full 292-byte D1 reply requested per window"
    ([p | p@(P.ControlWrite _ _ _ _ (0xd1:_)) <- packets]
      == replicate 3 (P.ControlWrite 0x43 0 0 0 [0xd1,0,0,0,0,0,0,0,1,0x24,0x11,0x7c])
      && [n | P.BulkIn _ n <- packets, n /= 8, n /= 2] == [292,292,292])

  -- Every byte the operation can emit, proven over the session's own packets.
  let outgoing = concatMap (\p -> case p of
        P.ControlWrite _ _ _ _ bytes -> [(12::Int,bytes)]
        P.BulkOut _ bytes -> [(readRequestBytes,bytes)]
        P.BulkIn _ _ -> []) packets
      requests = [bytes | P.BulkOut _ bytes <- packets]
  check "every emitted frame has its declared length" (all (\(n,bytes) -> length bytes == n) outgoing)
  check "one request frame per window" (length requests == length syntheticWindows)
  check "every request frame carries opcode 0x1a0002" (all ((==readOpcode) . word32At requestOpcodeOffset) requests)
  check "every request frame carries function 2" (all ((==2) . word32At requestFunctionOffset) requests)
  check "no request frame carries function 3" (all ((/=3) . word32At requestFunctionOffset) requests)
  check "every request frame carries mb 1" (all ((==1) . word32At requestMailboxOffset) requests)
  check "every request frame requests 256 bytes" (all ((==256) . word32At requestLengthOffset) requests)
  check "no request frame reads in-page" (all ((==0) . word32At requestInPageOffset) requests)
  check "no emitted frame carries opcode 0x200001" (all ((/=0x200001) . word32At 0 . snd) outgoing)
  check "control writes use only the audited session bytes" (all (\p -> case p of P.ControlWrite _ _ _ _ (op:_) -> op `elem` [0xc0,0xd0,0xd1,0xc4]; P.ControlWrite{} -> False; _ -> True) packets)
  let sweep = [readRequestFrame (ReadWindow 0 offset "sweep") | offset <- [0,readWindowLength..adjustmentCacheLimit-readWindowLength]]
  check "the frame builder emits function 2 at every admissible offset" (all (\frame -> word32At requestOpcodeOffset frame == readOpcode && word32At requestFunctionOffset frame == 2 && word32At requestMailboxOffset frame == 1 && word32At requestLengthOffset frame == 256 && length frame == readRequestBytes) sweep)

  let oracleWindow = syntheticWindows !! 1
      mutate target replace w = if windowOffset w == windowOffset target then replace w else P.Reply (successReply w)
      stopCases =
        [ ("oracle mismatch", mutate oracleWindow (\w -> P.Reply (replyFrameWith 0 0 (windowOffset w `div` readWindowLength,0,readWindowLength) (replicate readWindowLength 0))))
        , ("rejected arguments", mutate oracleWindow (\w -> P.Reply (replyFrameWith 9 3 (windowOffset w `div` readWindowLength,0,readWindowLength) (payloadFor w))))
        , ("callback status 2", mutate oracleWindow (\w -> P.Reply (replyFrameWith 1 0xffff (windowOffset w `div` readWindowLength,0,readWindowLength) (payloadFor w))))
        , ("status the handler cannot produce", mutate oracleWindow (\w -> P.Reply (replyFrameWith 2 2 (windowOffset w `div` readWindowLength,0,readWindowLength) (payloadFor w))))
        , ("echoed parameter mismatch", mutate oracleWindow (\w -> P.Reply (replyFrameWith 0 0 (windowOffset w `div` readWindowLength+1,0,readWindowLength) (payloadFor w))))
        , ("echoed length mismatch", mutate oracleWindow (\w -> P.Reply (replyFrameWith 0 0 (windowOffset w `div` readWindowLength,0,128) (payloadFor w))))
        , ("short reply", mutate oracleWindow (\w -> P.Reply (take (readReplyBytes-1) (successReply w))))
        , ("256-byte reply excludes prefix", mutate oracleWindow (\w -> P.Reply (take 256 (successReply w))))
        , ("unexpected disconnect", mutate oracleWindow (const P.LostReply)) ]
  forM_ (zip [(0::Int)..] stopCases) $ \(index,(label,replyFor)) -> do
    let dir = base </> ("stop-"++show index)
    (outcome,stoppedPackets,cleanupEvents) <- runCase dir syntheticPlan (sessionSteps syntheticWindows replyFor)
    check (label++" must stop the session") (either (const True) (const False) outcome)
    check (label++" must not request the next window")
      (length [() | P.BulkOut{} <- stoppedPackets] == 2)
    check (label++" must close the protocol once")
      (length [() | P.ControlWrite _ _ _ _ (0xc4:_) <- stoppedPackets] == 1)
    check (label++" must release every resource once") (cleanupEvents == [Released,Closed,Exited])
    check (label++" cleanup must not obscure the read failure")
      (either (not . isInfixOf "cleanup uncertain" . displayException) (const False) outcome)
    -- The window before the stop is already captured; the ones after are not.
    before <- doesPathExist (dir </> "windows" </> "000000.bin")
    after <- doesPathExist (dir </> "windows" </> "00ac00.bin")
    check (label++" must record the completed windows and refuse the rest") (before && not after)
    journal <- readFile (dir </> "journal.v1")
    check (label++" must journal the raw reply and the refusal") (("reply-before-deadline-check" `isInfixOf` journal) && (("window-refused" `isInfixOf` journal) || ("session-uncertain" `isInfixOf` journal)))

  let goodSteps = sessionSteps syntheticWindows (P.Reply . successReply)
      openReply bytes = take 1 goodSteps ++ [P.Step (P.BulkIn 0x81 8) (P.Reply bytes)] ++ drop 2 goodSteps
  forM_ [("short C0",replicate 7 0),("oversized C0",replicate 9 0)] $ \(label,bytes) -> do
    (outcome,stoppedPackets,cleanupEvents) <- runCase (base </> label) syntheticPlan (openReply bytes)
    check (label++" stops before D0 and cannot invent C4")
      (either (const True) (const False) outcome
        && stoppedPackets == [P.controlTransfer (P.control 0xc0 8 0),P.BulkIn 0x81 8])
    check (label++" releases every resource once") (cleanupEvents == [Released,Closed,Exited])
  let lostClose = take (length goodSteps-1) goodSteps ++ [P.Step (P.BulkIn 0x81 2) P.LostReply]
  (closeFailure,closePackets,closeEvents) <- runCase (base </> "lost-close") syntheticPlan lostClose
  check "lost C4 reply leaves uncertainty without retry"
    (either (isInfixOf "cleanup uncertain" . displayException) (const False) closeFailure
      && length [() | P.ControlWrite _ _ _ _ (0xc4:_) <- closePackets] == 1)
  check "failed C4 still releases every resource once" (closeEvents == [Released,Closed,Exited])

  -- Decoding is checked separately from the session so each refusal is exact.
  let window = syntheticWindows !! 0
      decode = decodeReadReply window
  check "exact success decode" (decode (successReply window) == Right (payloadFor window))
  forM_ [ replyFrameWith 9 3 (0,0,256) (payloadFor window)
        , replyFrameWith 1 0xffff (0,0,256) (payloadFor window)
        , replyFrameWith 3 0 (0,0,256) (payloadFor window)
        , replyFrameWith 0 0 (1,0,256) (payloadFor window)
        , replyFrameWith 0 0 (0,1,256) (payloadFor window)
        , replyFrameWith 0 0 (0,0,255) (payloadFor window)
        , take (readReplyBytes-1) (successReply window)
        , successReply window ++ [0]
        , word32le 0x200001 ++ drop 4 (successReply window) ] $ \bytes ->
    check "reply refusal" (either (const True) (const False) (decode bytes))

  let badPlans =
        [ ("empty window list", syntheticPlan{planWindows=[]})
        , ("unaligned offset", syntheticPlan{planWindows=[ReadWindow 0 0x000001 "unaligned"],planOracle=ReadOracle 0 0})
        , ("non-increasing offsets", syntheticPlan{planWindows=[ReadWindow 0 0x000200 "second",ReadWindow 1 0x000100 "first"]})
        , ("repeated offset", syntheticPlan{planWindows=[ReadWindow 0 0x000100 "first",ReadWindow 1 0x000100 "again"]})
        , ("index differs from position", syntheticPlan{planWindows=[ReadWindow 1 0x000100 "misindexed"],planOracle=ReadOracle 0 0})
        , ("window at the adjustment bound", syntheticPlan{planWindows=[ReadWindow 0 0x420000 "out of range"],planOracle=ReadOracle 0 0})
        , ("oracle outside the window list", syntheticPlan{planOracle=ReadOracle 3 0x5c})
        , ("oracle bytes past the window", syntheticPlan{planOracle=ReadOracle 1 0xfc})
        , ("empty purpose", syntheticPlan{planWindows=[ReadWindow 0 0x000100 ""],planOracle=ReadOracle 0 0})
        , ("unsafe plan alias", syntheticPlan{planId="../escape"})
        , ("short generator digest", syntheticPlan{planToolDigest=replicate 63 '0'}) ]
  forM_ badPlans $ \(label,plan) ->
    check (label++" must be refused") (either (const True) (const False) (validatePlan plan))

  -- The journal ceiling bounds the window count, not the schema shape cap.
  let sized count = syntheticPlan{planWindows=[ReadWindow index (index*readWindowLength) "sized" | index <- [0..count-1]],planOracle=ReadOracle 0 0x5c}
      admissible = (journalCeilingBytes-journalFixedBytes) `div` journalPerWindowBytes
  check "the ceiling admits more than the 1309-window scope" (admissible >= 1309 && admissible < planWindowCap)
  check "a plan at the ceiling is accepted" (validatePlan (sized admissible) == Right ())
  check "a plan one window over the ceiling is refused" (either (const True) (const False) (validatePlan (sized (admissible+1))))
  check "journal and deadline scale with the plan"
    (journalCapFor 1309 == journalFixedBytes+journalPerWindowBytes*1309
      && aggregateDeadlineMs transferTimeoutMs 1309 == transferTimeoutMs*(sessionFixedTransfers+transfersPerWindow*1309))

  removeDirectoryRecursive base
  putStrLn "bounded read: emitted-byte, oracle, status-pair, header-correlation, echo, short-reply, intent-deadline, remaining-budget, plan-invariant and ceiling checks passed"
