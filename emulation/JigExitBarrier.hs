module Main where

import Control.Monad (forM_, unless, void)
import qualified Data.ByteString as B
import Data.Bits ((.&.), (.|.), shiftL)
import Data.Word (Word8, Word32)
import FujiAF.Host
import Numeric (readHex, showHex)
import System.Environment (getArgs)

newtype Address = Address Int deriving (Eq, Show)
newtype HeaderOffset = HeaderOffset Int deriving (Eq, Show)
newtype Header = Header B.ByteString deriving (Eq, Show)
newtype Ordinary = Ordinary B.ByteString deriving (Eq, Show)
newtype Custom = Custom B.ByteString deriving (Eq, Show)
newtype SavedCounters = SavedCounters B.ByteString deriving (Eq, Show)
data Image a = Supplied a | ConstructedUnknown deriving (Eq, Show)
data Phase = Active | AwaitBoth | AwaitUSB | AwaitMode | BackendReady | ResponseFailed | Rejected deriving (Eq, Show)
data Completion = USBStop | ModeStop deriving (Eq, Show)
data ReadOutcome = ReadSucceeded | ReadFailed deriving (Eq, Show)
data USBOutcome = Stopped | AlreadyDisconnected deriving (Eq, Show)
data ResetOrigin = DisconnectReset | StartupFallback | CustomReadFallback deriving (Eq, Show)
data Event = ProductQuery | SessionClose | Disconnect | Complete Completion | USBCompleted USBOutcome | FailedResponse | GeneralBootRead ReadOutcome | CustomBootRead ReadOutcome | ExplicitFullReset | RawRegisterNoop deriving (Eq, Show)
data Effect = HeaderByteAttempt HeaderOffset Word8 Word8 | OrdinaryDefaults | CustomDefaults | CustomCommitComparisonUnknown | ExtraConfigDirty | StartupDefaultFlagConsumed deriving (Eq, Show)
data State = State { phase :: Phase, sessionActive :: Bool, liveHeader :: Header, savedCounters :: SavedCounters, ordinary :: Image Ordinary, custom :: Image Custom, effects :: [Effect], resetCount :: Int } deriving (Eq, Show)

checksum :: B.ByteString -> Header
checksum bytes = Header (B.take 15 bytes <> B.singleton (negate (B.foldl' (+) 0 (B.take 15 bytes))))
validHeader :: Header -> Bool
validHeader (Header bytes) = B.length bytes == 16 && B.index bytes 14 == 0xaa && B.foldl' (+) 0 bytes == (0 :: Word8)

prestate :: State
prestate = State Active True header (SavedCounters (B.pack [0x35,1,0xbc,2])) (Supplied ordinaryBytes) (Supplied customBytes) [] 0
  where
    header = checksum (B.pack [0x35,1,0xbc,2,0x42,8,9,0x13,0x2a,0x80,0x17,4,0x28,0x39,0xaa])
    ordinaryBytes = Ordinary (B.pack [fromIntegral ((i * 13 + 17) `mod` 256) | i <- [0 :: Int .. 0x117]])
    customBytes = Custom (B.pack [fromIntegral ((i * 7 + 41) `mod` 256) | i <- [0 :: Int .. 0x27]])

reset :: ResetOrigin -> State -> State
reset origin state = state { liveHeader = nextHeader, ordinary = ConstructedUnknown, custom = ConstructedUnknown, effects = effects state ++ writes ++ [OrdinaryDefaults, CustomDefaults, CustomCommitComparisonUnknown, ExtraConfigDirty] ++ [StartupDefaultFlagConsumed | origin == StartupFallback], resetCount = resetCount state + 1 }
  where
    Header oldHeader = liveHeader state
    SavedCounters counters = savedCounters state
    nextHeader@(Header nextBytes) = checksum (counters <> B.pack [0xd0,7,0,0xff,0,0x80,0,4,0,0,0xaa])
    writes = [HeaderByteAttempt (HeaderOffset i) old new | (i, (old, new)) <- zip [0..] (B.zip oldHeader nextBytes), old /= new]

step :: State -> Event -> State
step state ProductQuery = state
step state SessionClose = state { sessionActive = False }
step state Disconnect
  | phase state == Active = state { phase = AwaitBoth }
  | otherwise = state { phase = Rejected }
step state (Complete completion) = case (phase state, completion) of
  (AwaitBoth, USBStop) -> state { phase = AwaitMode }
  (AwaitBoth, ModeStop) -> state { phase = AwaitUSB }
  (AwaitUSB, USBStop) -> (reset DisconnectReset state) { phase = BackendReady }
  (AwaitMode, ModeStop) -> (reset DisconnectReset state) { phase = BackendReady }
  _ -> state
step state (USBCompleted _) = step state (Complete USBStop)
step state FailedResponse
  | phase state == BackendReady = state { phase = ResponseFailed }
  | otherwise = state
step state (GeneralBootRead outcome)
  | outcome == ReadFailed || not (validHeader (liveHeader state)) = reset StartupFallback state
  | otherwise = state
step state (CustomBootRead ReadFailed) = reset CustomReadFallback state
step state (CustomBootRead ReadSucceeded) = state
step state ExplicitFullReset = reset DisconnectReset state
step state RawRegisterNoop = state

replay :: State -> [Event] -> State
replay = foldl step
preserved :: State -> Bool
preserved state = ordinary state == ordinary prestate && custom state == custom prestate
hex :: Int -> String
hex value = "0x" ++ showHex value ""

newtype Token = Token Int deriving (Eq, Show)
newtype BarrierMask = BarrierMask Word32 deriving (Eq, Show)
data CachedByte = CachedByteSet | CachedByteZero deriving (Eq, Show)
data Callback = CallbackRegistered | CallbackAbsent deriving (Eq, Show)
data CommitResult = CommitSucceeded | CommitFailed deriving (Eq, Show)
data ShutdownSource = Completion1 | Completion2 | Completion3 | Completion4 | Completion5 | Completion6 | Completion7 | Completion8 | Completion9 deriving (Eq, Show, Enum, Bounded)
data AppMode = ModeB | ModeD deriving (Eq, Show)
data Modal62 = Modal62Idle | Modal62Requesting | Modal62Ready deriving (Eq, Show)
data Lifecycle = LifecycleNone | LifecycleTwo | LifecycleFive deriving (Eq, Show)
data PowerPhase = AtStart | AtGate | AtKey | AtPendingMode | AtModeCommitted | AtSubsystemRequest | AtSubsystemAccepted | AtModalReady | AtPersistence | AtBarrier | AtBarrierFull | AtMatched | AtLifecycleFive | AtTerminalRequest | AtTerminalLoop deriving (Eq, Show)
data PowerEvent = PowerOffText CachedByte Callback | ModeRequest | ModeCommit | ModalSetup | SubsystemAccept | SubsystemComplete | PersistenceDispatch Token | OrdinaryCommit CommitResult | ExtraConfigCommit CommitResult | BarrierStart | ShutdownCompletion ShutdownSource | MatchingEvent Token | LifecycleCommit | TerminalRequest | TerminalAction | BackgroundWorkQueued | ElapsedWithoutPowerDrop deriving (Eq, Show)
data PowerEffect = PowerOffParsed | PendingModeInstalled | ModeCommitted | SubsystemRequested | ModalStateSeven | OrdinaryCommitAttempt CommitResult | ExtraConfigCommitAttempt CommitResult | BarrierArmed | BarrierComplete | MatchingEventEmitted | ExitActionSelected | TerminalPayloadWord | ChannelStateCleared | DefaultConstructorNotCalled | GateRejectedCachedByte | GateRejectedCallback | DuplicateCompletionIgnored ShutdownSource | LateCompletionRequests91B ShutdownSource | PrematureMatchIgnored | TokenMismatchIgnored | CommitFailureIgnored | QueueWorkAbandoned | PowerDropUnobserved deriving (Eq, Show)
data PowerState = PowerState { powerPhase :: PowerPhase, appMode :: AppMode, pendingMode :: Maybe AppMode, modal62 :: Modal62, lifecycle :: Lifecycle, barrierMask :: BarrierMask, commitToken :: Maybe Token, ordinaryResult :: Maybe CommitResult, extraResult :: Maybe CommitResult, backgroundWork :: Int, powerEffects :: [PowerEffect] } deriving (Eq, Show)
data PowerExpectation = PowerExpectation { expectPhase :: PowerPhase, expectMode :: AppMode, expectLifecycle :: Lifecycle, expectMask :: BarrierMask, expectSettings :: Bool, expectTerminal :: Bool, expectRequired :: [PowerEffect], expectAnomalies :: [PowerEffect] } deriving (Eq, Show)

sourceBit :: ShutdownSource -> Word32
sourceBit source = 1 `shiftL` fromEnum source
fullBarrier :: BarrierMask
fullBarrier = BarrierMask 0x1ff
hasSource :: BarrierMask -> ShutdownSource -> Bool
hasSource (BarrierMask mask) source = mask .&. sourceBit source /= 0
addSource :: BarrierMask -> ShutdownSource -> BarrierMask
addSource (BarrierMask mask) source = BarrierMask (mask .|. sourceBit source)

serviceEntry :: PowerState
serviceEntry = PowerState AtStart ModeB Nothing Modal62Idle LifecycleNone (BarrierMask 0) Nothing Nothing Nothing 0 []

record :: PowerState -> [PowerEffect] -> PowerState
record state added = state { powerEffects = powerEffects state ++ added }

powerStep :: PowerState -> PowerEvent -> PowerState
powerStep state event = case (powerPhase state, event) of
  (AtGate,_) -> state
  (AtStart, PowerOffText CachedByteZero _) -> record state { powerPhase = AtGate } [GateRejectedCachedByte]
  (AtStart, PowerOffText _ CallbackAbsent) -> record state { powerPhase = AtGate } [GateRejectedCallback]
  (AtStart, PowerOffText CachedByteSet CallbackRegistered) -> record state { powerPhase = AtKey } [PowerOffParsed]
  (AtKey, ModeRequest) -> record state { powerPhase = AtPendingMode, pendingMode = Just ModeD } [PendingModeInstalled]
  (AtPendingMode, ModeCommit) -> record state { powerPhase = AtModeCommitted, appMode = ModeD, pendingMode = Nothing, lifecycle = LifecycleTwo } [ModeCommitted]
  (AtModeCommitted, ModalSetup) -> record state { powerPhase = AtSubsystemRequest, modal62 = Modal62Requesting } [SubsystemRequested]
  (AtSubsystemRequest, SubsystemAccept) -> state { powerPhase = AtSubsystemAccepted }
  (AtSubsystemAccepted, SubsystemComplete) -> record state { powerPhase = AtModalReady, modal62 = Modal62Ready } [ModalStateSeven]
  (AtModalReady, PersistenceDispatch token) -> state { powerPhase = AtPersistence, commitToken = Just token }
  (AtPersistence, OrdinaryCommit result) -> record state { ordinaryResult = Just result } ([OrdinaryCommitAttempt result] ++ [CommitFailureIgnored | result == CommitFailed])
  (AtPersistence, ExtraConfigCommit result) -> record state { extraResult = Just result } ([ExtraConfigCommitAttempt result] ++ [CommitFailureIgnored | result == CommitFailed])
  (AtPersistence, BarrierStart) -> record state { powerPhase = AtBarrier, barrierMask = BarrierMask 0 } [BarrierArmed]
  (AtBarrier, ShutdownCompletion source)
    | hasSource (barrierMask state) source -> record state [DuplicateCompletionIgnored source]
    | addSource (barrierMask state) source == fullBarrier -> record state { powerPhase = AtBarrierFull, barrierMask = fullBarrier } [BarrierComplete, MatchingEventEmitted]
    | otherwise -> state { barrierMask = addSource (barrierMask state) source }
  -- A full-mask action resets group-4 state before 0xec6708; a later completion takes the state-0 0x91b route.
  (AtBarrierFull, ShutdownCompletion source) -> record state [LateCompletionRequests91B source]
  (AtBarrierFull, MatchingEvent token)
    | Just token == commitToken state -> record state { powerPhase = AtMatched, modal62 = Modal62Idle } [ExitActionSelected]
    | otherwise -> record state [TokenMismatchIgnored]
  (AtMatched, LifecycleCommit) -> state { powerPhase = AtLifecycleFive, lifecycle = LifecycleFive }
  (AtLifecycleFive, TerminalRequest) -> record state { powerPhase = AtTerminalRequest } [TerminalPayloadWord]
  (AtTerminalRequest, TerminalAction) -> record state { powerPhase = AtTerminalLoop } ([ChannelStateCleared, DefaultConstructorNotCalled] ++ [QueueWorkAbandoned | backgroundWork state > 0])
  (AtTerminalLoop, ElapsedWithoutPowerDrop) -> record state [PowerDropUnobserved]
  (_, BackgroundWorkQueued) -> state { backgroundWork = backgroundWork state + 1 }
  (_, MatchingEvent _) -> record state [PrematureMatchIgnored]
  _ -> state

powerReplay :: [PowerEvent] -> PowerState
powerReplay = foldl powerStep serviceEntry

anomalous :: PowerEffect -> Bool
anomalous effect = case effect of
  GateRejectedCachedByte -> True
  GateRejectedCallback -> True
  DuplicateCompletionIgnored _ -> True
  LateCompletionRequests91B _ -> True
  PrematureMatchIgnored -> True
  TokenMismatchIgnored -> True
  CommitFailureIgnored -> True
  QueueWorkAbandoned -> True
  PowerDropUnobserved -> True
  _ -> False

settingsCommitted :: PowerState -> Bool
settingsCommitted state = ordinaryResult state == Just CommitSucceeded && extraResult state == Just CommitSucceeded

-- The candidate chain contains no transition into the ordinary/custom default constructor.
powerConstructorRan :: PowerState -> Bool
powerConstructorRan _ = False

-- The terminal action clears low-level channel state before looping.
powerSession :: PowerState -> Bool
powerSession state = powerPhase state /= AtTerminalLoop

powerPersistence :: PowerState -> String
powerPersistence state = case [result | Just result <- [ordinaryResult state, extraResult state]] of
  [] -> "no-modeled-persistent-effect"
  results | CommitFailed `elem` results -> "write-failure-ignored-by-firmware-path"
          | otherwise -> "unknown-write-attempts-not-storage-success"

data Report = Report { reportCase :: String, reportEvents :: [String], reportPhase :: String, reportSession :: Bool, reportResets :: Int, reportPreserved :: Bool, reportHeaderValid :: Bool, reportPersistence :: String, reportEffects :: [String], reportSettingsCommitted :: Bool, reportConstructorRan :: Bool }

emitReport :: Report -> IO ()
emitReport report = jsonPrint (obj [("case",s (reportCase report)),("classification",s "byte-anchored-abstract-transition-and-effect-replay"),("events",arr (map s (reportEvents report))),("phase",s (reportPhase report)),("session_active",b (reportSession report)),("reset_count",n (fromIntegral (reportResets report))),("ordinary_custom_preserved",b (reportPreserved report)),("live_header_valid",b (reportHeaderValid report)),("persistent_completion",s (reportPersistence report)),("effect_obligations",arr (map s (reportEffects report))),("settings_committed",b (reportSettingsCommitted report)),("default_constructor_ran",b (reportConstructorRan report)),("normal_boot_exit_proved",b False)])

image :: FilePath -> Int -> String -> IO B.ByteString
image path size digest = do
  bytes <- readRegular size path
  ensure (B.length bytes == size) "mapped image length mismatch"
  actual <- sha256 bytes
  ensure (actual == digest) "mapped image digest mismatch"
  pure bytes

wordAt :: B.ByteString -> Int -> Address -> Word32
wordAt bytes base (Address address) = foldl (\value (offset, byte) -> value + (fromIntegral byte `shiftL` offset)) 0 (zip [0,8,16,24] (B.unpack (B.take 4 (B.drop (address-base) bytes))))
expectWord :: B.ByteString -> Int -> (Address, Word32) -> IO ()
expectWord bytes base (address@(Address value), expected) = do
  ensure (value >= base && value + 4 <= base + B.length bytes && value `mod` 4 == 0) "word anchor range"
  ensure (wordAt bytes base address == expected) ("byte anchor mismatch at " ++ hex value)
expectCell :: B.ByteString -> Int -> (Address, (Word8, Word8)) -> IO ()
expectCell bytes base (Address address, (action, next)) = do
  ensure (address >= base && address + 2 <= base + B.length bytes && address `mod` 2 == 0) "cell anchor range"
  ensure (B.take 2 (B.drop (address-base) bytes) == B.pack [action,next]) ("cell anchor mismatch at " ++ hex address)

parseHex :: String -> IO Word32
parseHex text = do
  ensure (length text == 10 && take 2 text == "0x" && all (`elem` "0123456789abcdef") (drop 2 text)) "canonical anchor hexadecimal required"
  case readHex (drop 2 text) of
    [(value,"")] -> pure value
    _ -> failH "invalid anchor hexadecimal"

loadAnchors :: FilePath -> IO ([(Address, Word32)], [(Address, Word32)], [(Address, (Word8, Word8))])
loadAnchors path = do
  bytes <- readRegular 65536 path
  digest <- sha256 bytes
  ensure (digest == "58a5868d171c3503cb87011f16777f2d271c09c330503abdd70034794f34f27f") "independent private anchor inventory hash mismatch"
  value <- jsonRead bytes
  fields value "schema_version provenance image4_words image5_words image5_cells" "anchors"
  void (integer (get value "schema_version") 1 1 "anchor schema")
  ensure (not (null (str (get value "provenance")))) "anchor provenance required"
  let address item = Address . fromIntegral <$> parseHex (str (get item "address"))
      word item = do
        fields item "address word" "word anchor"
        (,) <$> address item <*> parseHex (str (get item "word"))
      cell item = do
        fields item "address action next" "cell anchor"
        action <- fromInteger <$> integer (get item "action") 0 255 "cell action"
        next <- fromInteger <$> integer (get item "next") 0 255 "cell next"
        target <- address item
        pure (target,(action,next))
  words4 <- mapM word (list (get value "image4_words"))
  words5 <- mapM word (list (get value "image5_words"))
  cells5 <- mapM cell (list (get value "image5_cells"))
  ensure (length words4 == 19 && length words5 == 25 && length cells5 == 10) "anchor inventory counts"
  pure (words4,words5,cells5)

main :: IO ()
main = do
  arguments <- getArgs
  (path4,path5,anchorPath) <- case arguments of
    ["--image4",file4,"--image5",file5,"--anchors",anchorFile] -> pure (file4,file5,anchorFile)
    _ -> failH "usage: jig-exit-barrier --image4 PRIVATE-ID4 --image5 PRIVATE-ID5 --anchors PRIVATE-ANCHORS (abstract effects, no stock execution)"
  image4 <- image path4 0x282800 "f2e2acab892dafeed85171f6870a411f0a2d5eae10921b86a24630d401dcff16"
  image5 <- image path5 0x4f1e60 "3cd501c07533938ae4d9347616986fd1649005a588c36b5abf05e36ef4fd892a"
  (words4,words5,cells5) <- loadAnchors anchorPath
  mapM_ (expectWord image4 0x61c400) words4
  mapM_ (expectWord image5 0xbf1a00) words5
  mapM_ (expectCell image5 0xbf1a00) cells5
  ensure (validHeader (liveHeader prestate)) "synthetic header must be valid"
  ensure (liveHeader prestate == Header (B.pack [0x35,1,0xbc,2,0x42,8,9,0x13,0x2a,0x80,0x17,4,0x28,0x39,0xaa,0xd6])) "independent synthetic checksum expectation"
  let both = [Disconnect,USBCompleted Stopped,Complete ModeStop]
      Header goodHeader = liveHeader prestate
      badMarker = prestate { liveHeader = checksum (B.take 14 goodHeader <> B.singleton 0x55) }
      badChecksum = prestate { liveHeader = Header (B.take 15 goodHeader <> B.singleton (B.index goodHeader 15 + 1)) }
      cases = [("disconnect-no-query",prestate,both,BackendReady,1,False),
               ("disconnect-query",prestate,ProductQuery:both,BackendReady,1,False),
               ("disconnect-query-close",prestate,ProductQuery:SessionClose:both,BackendReady,1,False),
               ("reverse-completions",prestate,[Disconnect,Complete ModeStop,Complete USBStop],BackendReady,1,False),
               ("already-disconnected-status-minus13",prestate,[Disconnect,USBCompleted AlreadyDisconnected,Complete ModeStop],BackendReady,1,False),
               ("failed-response-after-reset",prestate,both ++ [FailedResponse],ResponseFailed,1,False),
               ("interrupted-before-completions",prestate,[Disconnect],AwaitBoth,0,True),
               ("missing-mode-completion",prestate,[Disconnect,Complete USBStop],AwaitMode,0,True),
               ("missing-usb-completion",prestate,[Disconnect,Complete ModeStop],AwaitUSB,0,True),
               ("c4-only-not-mode-exit",prestate,[SessionClose],Active,0,True),
               ("wrong-service-state-rejection",prestate {phase = Rejected},[Disconnect],Rejected,0,True),
               ("unsolicited-completions",prestate,[Complete USBStop,Complete ModeStop],Active,0,True),
               ("startup-valid-reads",prestate,[GeneralBootRead ReadSucceeded,CustomBootRead ReadSucceeded],Active,0,True),
               ("startup-marker-invalid-checksum-valid",badMarker,[GeneralBootRead ReadSucceeded],Active,1,False),
               ("startup-checksum-invalid-marker-valid",badChecksum,[GeneralBootRead ReadSucceeded],Active,1,False),
               ("startup-ordinary-read-failure",prestate,[GeneralBootRead ReadFailed],Active,1,False),
               ("startup-custom-read-failure",prestate,[CustomBootRead ReadFailed],Active,1,False),
               ("explicit-selector5-full-reset",prestate,[ExplicitFullReset],Active,1,False),
               ("raw-register-selector-noop",prestate,[RawRegisterNoop],Active,0,True)]
  forM_ cases $ \(name,initial,events,expectedPhase,expectedResets,expectedPreservation) -> do
    let result = replay initial events
    let expectedResetHeader = Header (B.pack [0x35,1,0xbc,2,0xd0,7,0,0xff,0,0x80,0,4,0,0,0xaa,8])
    ensure (expectedResets == 0 || liveHeader result == expectedResetHeader) "independent reset header/counter expectation"
    ensure (phase result == expectedPhase && resetCount result == expectedResets && preserved result == expectedPreservation && validHeader (liveHeader result)) ("abstract expectation mismatch: " ++ name)
    unless (expectedResets == 0) $ ensure (OrdinaryDefaults `elem` effects result && CustomDefaults `elem` effects result && ExtraConfigDirty `elem` effects result) "reset effect obligation missing"
    emitReport (Report name (map show events) (show (phase result)) (sessionActive result) (resetCount result) (preserved result) (validHeader (liveHeader result)) (if null (effects result) then "no-modeled-persistent-effect" else "unknown-write-attempts-not-storage-success") (map show (effects result)) False (resetCount result > 0))
  let liveToken = Token 0x4a
      foreignToken = Token 0x4b
      allSources = [minBound .. maxBound] :: [ShutdownSource]
      entry = [PowerOffText CachedByteSet CallbackRegistered,ModeRequest,ModeCommit,ModalSetup,SubsystemAccept,SubsystemComplete]
      dispatch ordinaryOutcome extraOutcome = [PersistenceDispatch liveToken,OrdinaryCommit ordinaryOutcome,ExtraConfigCommit extraOutcome,BarrierStart]
      exitTail token = [MatchingEvent token,LifecycleCommit,TerminalRequest,TerminalAction]
      nominal = entry ++ dispatch CommitSucceeded CommitSucceeded ++ map ShutdownCompletion allSources ++ exitTail liveToken
      reordered = entry ++ dispatch CommitSucceeded CommitSucceeded ++ map ShutdownCompletion (reverse allSources) ++ exitTail liveToken
      armed ordinaryOutcome extraOutcome = [PowerOffParsed,PendingModeInstalled,ModeCommitted,SubsystemRequested,ModalStateSeven,OrdinaryCommitAttempt ordinaryOutcome,ExtraConfigCommitAttempt extraOutcome,BarrierArmed]
      completed ordinaryOutcome extraOutcome = armed ordinaryOutcome extraOutcome ++ [BarrierComplete,MatchingEventEmitted]
      chain ordinaryOutcome extraOutcome = completed ordinaryOutcome extraOutcome ++ [ExitActionSelected,TerminalPayloadWord,ChannelStateCleared,DefaultConstructorNotCalled]
      terminalExpectation settings ordinaryOutcome extraOutcome anomalies = PowerExpectation AtTerminalLoop ModeD LifecycleFive fullBarrier settings True (chain ordinaryOutcome extraOutcome) anomalies
      gateExpectation anomaly = PowerExpectation AtGate ModeB LifecycleNone (BarrierMask 0) False False [] [anomaly]
      powerCases = [("power-off-nominal-chain",nominal,terminalExpectation True CommitSucceeded CommitSucceeded []),
                    ("power-off-gate-cached-byte-zero",PowerOffText CachedByteZero CallbackRegistered : drop 1 nominal,gateExpectation GateRejectedCachedByte),
                    ("power-off-gate-callback-unregistered",PowerOffText CachedByteSet CallbackAbsent : drop 1 nominal,gateExpectation GateRejectedCallback),
                    ("barrier-missing-one-completion",entry ++ dispatch CommitSucceeded CommitSucceeded ++ map ShutdownCompletion (filter (/= Completion5) allSources) ++ exitTail liveToken,PowerExpectation AtBarrier ModeD LifecycleTwo (BarrierMask 0x1ef) True False (armed CommitSucceeded CommitSucceeded) [PrematureMatchIgnored]),
                    ("barrier-duplicated-completion",entry ++ dispatch CommitSucceeded CommitSucceeded ++ [ShutdownCompletion Completion1,ShutdownCompletion Completion2,ShutdownCompletion Completion1] ++ map ShutdownCompletion [Completion3 .. Completion9] ++ exitTail liveToken,terminalExpectation True CommitSucceeded CommitSucceeded [DuplicateCompletionIgnored Completion1]),
                    ("barrier-alternate-completion-order",reordered,terminalExpectation True CommitSucceeded CommitSucceeded []),
                    ("barrier-late-completion-after-full",entry ++ dispatch CommitSucceeded CommitSucceeded ++ map ShutdownCompletion allSources ++ [ShutdownCompletion Completion1] ++ exitTail liveToken,PowerExpectation AtTerminalLoop ModeD LifecycleFive fullBarrier True True (completed CommitSucceeded CommitSucceeded ++ [ExitActionSelected,TerminalPayloadWord,ChannelStateCleared,DefaultConstructorNotCalled]) [LateCompletionRequests91B Completion1]),
                    ("persistence-ordinary-writer-failure",entry ++ dispatch CommitFailed CommitSucceeded ++ map ShutdownCompletion allSources ++ exitTail liveToken,terminalExpectation False CommitFailed CommitSucceeded [CommitFailureIgnored]),
                    ("persistence-extra-config-writer-failure",entry ++ dispatch CommitSucceeded CommitFailed ++ map ShutdownCompletion allSources ++ exitTail liveToken,terminalExpectation False CommitSucceeded CommitFailed [CommitFailureIgnored]),
                    ("persistence-both-writers-failure",entry ++ dispatch CommitFailed CommitFailed ++ map ShutdownCompletion allSources ++ exitTail liveToken,terminalExpectation False CommitFailed CommitFailed [CommitFailureIgnored,CommitFailureIgnored]),
                    ("matching-event-token-mismatch",entry ++ dispatch CommitSucceeded CommitSucceeded ++ map ShutdownCompletion allSources ++ exitTail foreignToken,PowerExpectation AtBarrierFull ModeD LifecycleTwo fullBarrier True False (completed CommitSucceeded CommitSucceeded) [TokenMismatchIgnored]),
                    ("terminal-reached-without-power-drop",nominal ++ [ElapsedWithoutPowerDrop],terminalExpectation True CommitSucceeded CommitSucceeded [PowerDropUnobserved]),
                    ("terminal-with-queue-work-outstanding",entry ++ dispatch CommitSucceeded CommitSucceeded ++ map ShutdownCompletion allSources ++ [MatchingEvent liveToken,LifecycleCommit,TerminalRequest,BackgroundWorkQueued,TerminalAction],terminalExpectation True CommitSucceeded CommitSucceeded [QueueWorkAbandoned])]
  ensure (powerReplay reordered == powerReplay nominal) "nine-source barrier must be completion-order independent"
  ensure (length allSources == 9 && foldl addSource (BarrierMask 0) allSources == fullBarrier) "nine completion bits must fill mask 0x1ff"
  forM_ powerCases $ \(name,events,expected) -> do
    let result = powerReplay events
    ensure (powerPhase result == expectPhase expected) ("power phase mismatch: " ++ name)
    ensure (appMode result == expectMode expected && lifecycle result == expectLifecycle expected) ("power mode/lifecycle mismatch: " ++ name)
    ensure (barrierMask result == expectMask expected) ("barrier mask mismatch: " ++ name)
    ensure (settingsCommitted result == expectSettings expected) ("settings expectation mismatch: " ++ name)
    ensure ((powerPhase result == AtTerminalLoop) == expectTerminal expected) ("terminal expectation mismatch: " ++ name)
    ensure (filter (not . anomalous) (powerEffects result) == expectRequired expected) ("power effect obligation mismatch: " ++ name)
    ensure (filter anomalous (powerEffects result) == expectAnomalies expected) ("power anomaly expectation mismatch: " ++ name)
    ensure (not (powerConstructorRan result)) ("default constructor must stay unreached: " ++ name)
    emitReport (Report name (map show events) (show (powerPhase result)) (powerSession result) 0 (not (powerConstructorRan result)) True (powerPersistence result) (map show (powerEffects result)) (settingsCommitted result) (powerConstructorRan result))
  jsonPrint (obj [("verdict",s "no-settings-preserving-caller-exit-justified"),("stock_instructions_executed",n 0),("cases",n (fromIntegral (length cases + length powerCases))),("limits",arr (map s ["reset ordinary/custom final bytes deliberately unresolved, constructor effects are not stubs", "later boot depends on actual retained writes and successful ordinary/custom reads", "driver/queue/hardware completion not simulated", "installed 1.00 and HyperFFW End Setting unbound", "backend-ready is not normal application boot", "no camera authorization or native admission change", "POWER OFF chain is the 1.50 static candidate, with runtime guards, HyperFFW naming and installed 1.00 unproved", "completion bits model callback arrival, not written record contents or power-failure recovery", "physical power drop, post-terminal interrupt/DMA activity and the following boot are outside the model"]))])
