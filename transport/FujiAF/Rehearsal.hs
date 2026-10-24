-- Offline rehearsals construct no native provider. Bound operations run
-- against scripted replies; unbound RAM-push operations prove their
-- pre-provider refusal.
module FujiAF.Rehearsal (rehearseIdentify, rehearseReadConfig, rehearseEndSetting, rehearseProbeRam, rehearseReadLog, rehearseProbeBootRom, rehearseProbeArea2, rehearseProbeStraps, rehearseProbeCode, rehearseProbeOwnership, rehearseProbeRoutesFixed, rehearseProbeRoutesDerived, rehearseDumpRam, rehearseReadMedia, rehearseArmAutoAct, rehearseCommitF7, rehearseReadRam, rehearseReadBulk, rehearseRamAlloc, rehearseRamPush, rehearseRamFree, rehearseRamUnload, ScriptedOperation (..), scriptedApi, rehearsalCandidate, identifyScript, readConfigScript, endSettingScript, probeRamScript, readLogScript, probeBootRomScript, probeArea2Script, probeStrapsScript, probeCodeScript, probeOwnershipScript, probeRoutesScript, dumpRamScript, readMediaScript, armAutoActScript, commitF7Script, readRamScript, BulkCamera (..), stockBulkCamera, bulkRehearsalMemory, bulkCameraDevice, bulkCameraApi) where

import Control.Exception (bracket)
import Control.Monad (forM_, unless)
import Data.Bits ((.&.), shiftR, xor)
import Data.IORef
import Data.Maybe (isNothing)
import Data.Word (Word8)
import System.Directory (createDirectory, getTemporaryDirectory, removeDirectoryRecursive)
import System.FilePath ((</>))
import GHC.Clock (getMonotonicTimeNSec)
import System.Posix.Process (getProcessID)
import FujiAF.Transport
import FujiAF.Identify
import FujiAF.Read
import FujiAF.EndSetting
import FujiAF.Probe
import FujiAF.Log
import FujiAF.BootRom
import FujiAF.Area2
import FujiAF.CodeProbe
import FujiAF.DumpRam
import FujiAF.ReadMedia
import FujiAF.Straps
import FujiAF.Ownership
import FujiAF.Routes
import FujiAF.StoredGate
import FujiAF.ReadRam
import FujiAF.ReadBulk
import FujiAF.RamPush
import qualified FujiAF.ArmAutoAct as Arm
import qualified FujiAF.Broker as P

rehearsalDescriptors :: Descriptors
rehearsalDescriptors = Descriptors 1 0 0 255 0 0 0x81 2 512 512 0 1 []

rehearsalCandidate :: Candidate
rehearsalCandidate = Candidate "001:2.3" 0x04cb 0xff80 rehearsalDescriptors

rehearsalSid :: Int
rehearsalSid = 0x1234

-- Little-endian SID followed by the reply's constant tail: a zero halfword and
-- 65536 little-endian, which is what the C0 handler writes (E-DBG-C0-FIELD-150).
openReply :: [Word8]
openReply = [0x34,0x12,0,0,0,0,1,0]

closeSteps :: [P.Step]
closeSteps =
  [ P.Step (P.controlTransfer (P.control 0xc4 2 rehearsalSid)) (P.Transferred 12)
  , P.Step (P.BulkIn 0x81 2) (P.Reply [0,0]) ]

openSteps :: [P.Step]
openSteps =
  [ P.Step (P.controlTransfer (P.control 0xc0 8 0)) (P.Transferred 12)
  , P.Step (P.BulkIn 0x81 8) (P.Reply openReply) ]

identifySteps :: [P.Step]
identifySteps = openSteps ++ identityExchange ++ closeSteps

identityExchange :: [P.Step]
identityExchange =
  [ P.Step (P.controlTransfer (P.control 0xd0 32 rehearsalSid)) (P.Transferred 12)
  , P.Step (P.BulkOut 2 P.identityPacket) (P.Transferred 32)
  , P.Step (P.controlTransfer (P.control 0xd1 128 rehearsalSid)) (P.Transferred 12)
  , P.Step (P.BulkIn 0x81 128) (P.Reply identityReply) ]
  where identityReply = [0,0,0x15,0] ++ replicate 12 0 ++ [88,0,45,0,77,0,49,0] ++ replicate 104 0

readSteps :: ReadPlan -> [P.Step]
readSteps plan = openSteps ++ concatMap window (planWindows plan) ++ closeSteps
  where
    window w =
      [ P.Step (P.controlTransfer (P.control 0xd0 readRequestBytes rehearsalSid)) (P.Transferred 12)
      , P.Step (P.BulkOut 2 (readRequestFrame w)) (P.Transferred readRequestBytes)
      , P.Step (P.controlTransfer (P.control 0xd1 readReplyBytes rehearsalSid)) (P.Transferred 12)
      , P.Step (P.BulkIn 0x81 readReplyBytes) (P.Reply (windowReply w)) ]
    windowReply w = concatMap word32le [readOpcode,0,0,0,readFunction,readMailbox,windowOffset w `div` readWindowLength,0,readWindowLength] ++ payload w
    payload w
      | windowIndex w == oracleWindowIndex (planOracle plan) =
          let at = oracleByteOffset (planOracle plan)
          in replicate at 0 ++ productOracleBytes ++ replicate (readWindowLength-at-length productOracleBytes) 0
      | otherwise = replicate readWindowLength 0

endSettingSteps :: [P.Step]
endSettingSteps = openSteps ++
  [ P.Step (endD0Control packets) (P.Transferred 12)
  , P.Step (endD0Bulk packets) (P.Transferred endRequestBytes)
  , P.Step (endD1Control packets) (P.Transferred 12)
  , P.Step (endD1Bulk packets) (P.Reply acceptedReply) ]
  where
    packets = buildEndSettingPackets (EndSettingSessionId (fromIntegral rehearsalSid)) rehearsalDescriptors
    acceptedReply = wordLE 0x00200001 ++ replicate 12 0 ++ wordLE 5 ++ replicate 108 0

-- The probe replies keep the gate window byte and the two bound byte strings
-- so the operation's own in-session checks pass against the script.
probeRehearsalWindow :: Int -> [Word8]
probeRehearsalWindow start
  | start == cacheProductProbe = filler
  | start == cacheReadbackProbe = probeReadbackPayload 1
  | start == cacheSignatureProbe = take signatureByteOffset filler ++ [0x55,0xaa] ++ drop (signatureByteOffset+2) filler
  | otherwise = replicate probeWindowBytes (fromIntegral (start `mod` 251))
  where filler = take productByteOffset (replicate probeWindowBytes 0x11)
                  ++ productOracleBytes
                  ++ drop (productByteOffset + length productOracleBytes) (replicate probeWindowBytes 0x11)

probeReadbackPayload :: Word8 -> [Word8]
probeReadbackPayload value = take gateIndex zeros ++ [value] ++ drop (gateIndex+1) zeros
  where zeros = replicate 256 0x33

probeWriteReply :: [Word8]
probeWriteReply = concat
  [ word32le readOpcode, word32le 0, word32le 0, word32le 0
  , word32le 3, word32le 1, word32le 0, word32le gateIndex, word32le configWriteLength ]
  ++ replicate 256 0

probeGateWindowReply :: [Word8] -> [Word8]
probeGateWindowReply payload = concat
  [ word32le readOpcode, word32le 0, word32le 0, word32le 0
  , word32le 2, word32le 1, word32le 0, word32le 0, word32le readWindowLength ]
  ++ payload

probeHalfReply :: Int -> [Word8] -> [Word8]
probeHalfReply address payload = concat
  [ word32le debugOpcode, word32le 0, word32le 0, word32le 0
  , word32le 2, word32le 0, word32le 0, word32le address, word32le probeHalfBytes ]
  ++ payload

probeSteps :: [P.Step]
probeSteps = openSteps ++ concatMap action probeRequests ++ closeSteps
  where
    action request = case request of
      Close -> []
      GateWrite value ->
        [ P.Step (P.controlTransfer (P.control 0xd0 configWriteBytes rehearsalSid)) (P.Transferred 12)
        , P.Step (P.BulkOut 2 (configWriteFrame value)) (P.Transferred configWriteBytes)
        , P.Step (P.controlTransfer (P.control 0xd1 configReplyBytes rehearsalSid)) (P.Transferred 12)
        , P.Step (P.BulkIn 0x81 configReplyBytes) (P.Reply probeWriteReply) ]
      GateRead label ->
        [ P.Step (P.controlTransfer (P.control 0xd0 readRequestBytes rehearsalSid)) (P.Transferred 12)
        , P.Step (P.BulkOut 2 (readRequestFrame readbackWindow)) (P.Transferred readRequestBytes)
        , P.Step (P.controlTransfer (P.control 0xd1 readReplyBytes rehearsalSid)) (P.Transferred 12)
        , P.Step (P.BulkIn 0x81 readReplyBytes) (P.Reply (probeGateWindowReply (probeReadbackPayload (if label == gateReadbackEnabled then 1 else 0)))) ]
      Half address ->
        let start = if address `elem` map probeAddress probeList then address else address - probeHalfBytes
            window = probeRehearsalWindow start
            payload = if address == start then take probeHalfBytes window else drop probeHalfBytes window
        in [ P.Step (P.controlTransfer (P.control 0xd0 debugRequestBytes rehearsalSid)) (P.Transferred 12)
           , P.Step (P.BulkOut 2 (debugRequestFrame address)) (P.Transferred debugRequestBytes)
           , P.Step (P.controlTransfer (P.control 0xd1 debugReplyForHalf rehearsalSid)) (P.Transferred 12)
           , P.Step (P.BulkIn 0x81 debugReplyForHalf) (P.Reply (probeHalfReply address payload)) ]

describeTransfer :: P.Transfer -> String
describeTransfer (P.ControlWrite requestType request value index bytes) =
  "control-write bmRequestType=" ++ hex requestType ++ " bRequest=" ++ show request
  ++ " wValue=" ++ show value ++ " wIndex=" ++ show index ++ " bytes=" ++ P.hexBytes bytes
  where hex n = "0x" ++ P.hexBytes [fromIntegral n]
describeTransfer (P.BulkOut endpoint bytes) =
  "bulk-out endpoint=0x" ++ P.hexBytes [fromIntegral endpoint] ++ " bytes=" ++ P.hexBytes bytes
describeTransfer (P.BulkIn endpoint count) =
  "bulk-in endpoint=0x" ++ P.hexBytes [fromIntegral endpoint] ++ " exact-reply-bytes=" ++ show count

printPlan :: String -> [P.Step] -> IO ()
printPlan operation steps = do
  putStrLn ("rehearsal: " ++ operation ++ "; no native provider, USB enumeration or camera execution")
  putStrLn ("synthetic SID: 0x1234; IN endpoint: 0x81; OUT endpoint: 0x02; transfer budget: " ++ show transferTimeoutMs ++ " ms")
  forM_ (zip [1::Int ..] steps) $ \(index,P.Step packet outcome) ->
    putStrLn (show index ++ ". " ++ describeTransfer packet ++ describeReply outcome)
  where
    describeReply (P.Reply bytes) = " fake-reply=" ++ P.hexBytes bytes
    describeReply _ = ""

-- Rejects a packet the operation emits out of order, so a drifting plan stops
-- the rehearsal instead of passing it.
scriptedApi :: IORef [P.Step] -> UsbApi
scriptedApi script = UsbApi
  (\_ -> pure ())
  (\_ -> pure [rehearsalCandidate])
  (\_ _ -> pure ())
  (\interface _ -> unless (interface == 0) (P.reject "interface not allowed"))
  dispatch
  (\interface _ -> unless (interface == 0) (P.reject "interface not allowed"))
  (\_ -> pure ())
  (\_ -> pure ())
  where
    dispatch packet _ = do
      steps <- readIORef script
      case steps of
        [] -> P.reject "rehearsal script exhausted"
        P.Step expected outcome:rest -> do
          unless (packet == expected) (P.reject ("rehearsed transfer "++show packet++" differs from the planned "++show expected))
          writeIORef script rest
          case outcome of
            P.Transferred n -> pure (n,[])
            P.Reply bytes -> pure (length bytes,bytes)
            P.LostReply -> P.reject "injected lost reply"

withRehearsalDirectory :: (FilePath -> IO a) -> IO a
withRehearsalDirectory action = do
  process <- getProcessID
  temporary <- getTemporaryDirectory
  let base = temporary </> ("fuji-af-rehearsal-"++show process)
  bracket (createDirectory base >> pure base) removeDirectoryRecursive $ \base' -> do
    let session = base' </> "session"
    ensurePrivateDirectory session
    action session

-- One bound operation as rehearsed: its planned transfers in order with their
-- scripted replies, and the run that must consume them.
data ScriptedOperation = ScriptedOperation
  { scriptedName :: String
  , scriptedSteps :: [P.Step]
  , scriptedRun :: Clock -> UsbApi -> FilePath -> IO String
  }

runRehearsal :: Clock -> ScriptedOperation -> IO ()
runRehearsal clock operation = do
  printPlan (scriptedName operation) (scriptedSteps operation)
  script <- newIORef (scriptedSteps operation)
  result <- withRehearsalDirectory (scriptedRun operation clock (scriptedApi script))
  leftover <- readIORef script
  unless (null leftover) (P.reject "rehearsal left planned transfers unsent")
  putStrLn ("rehearsed " ++ scriptedName operation ++ ": " ++ result)

rehearseRamAlloc :: Int -> IO ()
rehearseRamAlloc size = do
  putStrLn ("rehearsal: ram-alloc; no native provider, USB enumeration or camera execution;allocation-bytes="++show size)
  outcome <- withRehearsalDirectory (runRamAllocWith PeerCacheUnbound size realClock unavailableApi)
  unless (outcome == RamAllocRefused "X-M1 peer-core cache binding is unbound; no camera write sent")
    (P.reject ("RAM-alloc unbound rehearsal outcome changed: "++show outcome))

rehearseRamPush :: RamAllocState -> RamPushManifest -> IO ()
rehearseRamPush allocation manifest = do
  putStrLn ("rehearsal: ram-push; no native provider, USB enumeration or camera execution;allocation-bytes="++show (allocSize allocation)++";runtime-bytes="++show (allocationSize manifest)++";publications="++show (length (manifestPublications manifest)))
  outcome <- withRehearsalDirectory (runRamPushWith PeerCacheUnbound allocation manifest realClock unavailableApi)
  unless (outcome == RamPushRefused "X-M1 peer-core cache binding is unbound; no camera write sent")
    (P.reject ("RAM-push unbound rehearsal outcome changed: "++show outcome))

rehearseRamFree :: RamAllocState -> IO ()
rehearseRamFree state = do
  putStrLn ("rehearsal: ram-free; no native provider, USB enumeration or camera execution;allocation-bytes="++show (allocSize state))
  outcome <- withRehearsalDirectory (runRamFreeWith PeerCacheUnbound state realClock unavailableApi)
  unless (outcome == RamFreeRefused "X-M1 peer-core cache binding is unbound; no camera write sent")
    (P.reject ("RAM-free unbound rehearsal outcome changed: "++show outcome))

rehearseRamUnload :: RamPushState -> IO ()
rehearseRamUnload state = do
  putStrLn ("rehearsal: ram-unload; no native provider, USB enumeration or camera execution;allocation-bytes="++show (stateSize state)++";publications="++show (length (statePublications state)))
  outcome <- withRehearsalDirectory (runRamUnloadWith PeerCacheUnbound state realClock unavailableApi)
  unless (outcome == RamUnloadRefused "X-M1 peer-core cache binding is unbound; no camera write sent")
    (P.reject ("RAM-unload unbound rehearsal outcome changed: "++show outcome))

unavailableApi :: UsbApi
unavailableApi = UsbApi unavailable unavailableList unavailableOpen unavailableClaim unavailableTransfer unavailableRelease unavailableClose unavailableExit
  where
    unavailable _ = P.reject "rehearsal attempted native initialization"
    unavailableList _ = P.reject "rehearsal attempted USB enumeration"
    unavailableOpen _ _ = P.reject "rehearsal attempted USB open"
    unavailableClaim _ _ = P.reject "rehearsal attempted USB claim"
    unavailableTransfer _ _ = P.reject "rehearsal attempted a transfer"
    unavailableRelease _ _ = P.reject "rehearsal attempted USB release"
    unavailableClose _ = P.reject "rehearsal attempted USB close"
    unavailableExit _ = P.reject "rehearsal attempted native exit"

identifyScript :: ScriptedOperation
identifyScript = ScriptedOperation "identify" identifySteps $ \clock api dir -> do
  ProductModel model <- runIdentify clock api dir
  pure model

rehearseIdentify :: IO ()
rehearseIdentify = runRehearsal realClock identifyScript

readConfigScript :: ReadPlan -> ScriptedOperation
readConfigScript plan = ScriptedOperation "read-config" (readSteps plan) $ \clock api dir -> do
  windows <- runRead clock api plan dir
  pure (show windows ++ " windows")

rehearseReadConfig :: ReadPlan -> IO ()
rehearseReadConfig = runRehearsal realClock . readConfigScript

endSettingScript :: ScriptedOperation
endSettingScript = ScriptedOperation "end-setting" endSettingSteps $ \clock api dir ->
  outcomeName <$> runEndSetting clock api dir

rehearseEndSetting :: IO ()
rehearseEndSetting = runRehearsal realClock endSettingScript

probeRamScript :: ScriptedOperation
probeRamScript = ScriptedOperation "probe-ram" probeSteps $ \clock api dir -> do
  probes <- runProbe clock api dir
  pure (show probes ++ " probes")

rehearseProbeRam :: IO ()
rehearseProbeRam = runRehearsal realClock probeRamScript

-- The log windows have no oracle, so the scripted halves carry fixed filler.
gateRehearsalPayload :: Word8 -> [Word8]
gateRehearsalPayload value = take gateIndex (replicate 256 0x5a) ++ [value] ++ drop (gateIndex+1) (replicate 256 0x5a)

logSteps :: [P.Step]
logSteps = openSteps ++ concatMap action logRequests ++ closeSteps
  where
    action request = case request of
      Close -> []
      GateWrite value ->
        [ P.Step (P.controlTransfer (P.control 0xd0 configWriteBytes rehearsalSid)) (P.Transferred 12)
        , P.Step (P.BulkOut 2 (configWriteFrame value)) (P.Transferred configWriteBytes)
        , P.Step (P.controlTransfer (P.control 0xd1 configReplyBytes rehearsalSid)) (P.Transferred 12)
        , P.Step (P.BulkIn 0x81 configReplyBytes) (P.Reply probeWriteReply) ]
      GateRead label ->
        [ P.Step (P.controlTransfer (P.control 0xd0 readRequestBytes rehearsalSid)) (P.Transferred 12)
        , P.Step (P.BulkOut 2 (readRequestFrame readbackWindow)) (P.Transferred readRequestBytes)
        , P.Step (P.controlTransfer (P.control 0xd1 readReplyBytes rehearsalSid)) (P.Transferred 12)
        , P.Step (P.BulkIn 0x81 readReplyBytes) (P.Reply (probeGateWindowReply (gateRehearsalPayload (if label == gateReadbackEnabled then 1 else 0)))) ]
      Half address ->
        [ P.Step (P.controlTransfer (P.control 0xd0 debugRequestBytes rehearsalSid)) (P.Transferred 12)
        , P.Step (P.BulkOut 2 (debugRequestFrame address)) (P.Transferred debugRequestBytes)
        , P.Step (P.controlTransfer (P.control 0xd1 debugReplyForHalf rehearsalSid)) (P.Transferred 12)
        , P.Step (P.BulkIn 0x81 debugReplyForHalf) (P.Reply (probeHalfReply address (replicate probeHalfBytes 0x11))) ]

readLogScript :: ScriptedOperation
readLogScript = ScriptedOperation "read-log" logSteps $ \clock api dir -> do
  windows <- runReadLog clock api dir
  pure (show windows ++ " windows")

rehearseReadLog :: IO ()
rehearseReadLog = runRehearsal realClock readLogScript

-- The boot-ROM windows have no oracle, so the scripted halves carry fixed filler.
bootRomSteps :: [P.Step]
bootRomSteps = openSteps ++ concatMap action bootRomRequests ++ closeSteps
  where
    action request = case request of
      Close -> []
      GateWrite value ->
        [ P.Step (P.controlTransfer (P.control 0xd0 configWriteBytes rehearsalSid)) (P.Transferred 12)
        , P.Step (P.BulkOut 2 (configWriteFrame value)) (P.Transferred configWriteBytes)
        , P.Step (P.controlTransfer (P.control 0xd1 configReplyBytes rehearsalSid)) (P.Transferred 12)
        , P.Step (P.BulkIn 0x81 configReplyBytes) (P.Reply probeWriteReply) ]
      GateRead label ->
        [ P.Step (P.controlTransfer (P.control 0xd0 readRequestBytes rehearsalSid)) (P.Transferred 12)
        , P.Step (P.BulkOut 2 (readRequestFrame readbackWindow)) (P.Transferred readRequestBytes)
        , P.Step (P.controlTransfer (P.control 0xd1 readReplyBytes rehearsalSid)) (P.Transferred 12)
        , P.Step (P.BulkIn 0x81 readReplyBytes) (P.Reply (probeGateWindowReply (gateRehearsalPayload (if label == gateReadbackEnabled then 1 else 0)))) ]
      Half address ->
        [ P.Step (P.controlTransfer (P.control 0xd0 debugRequestBytes rehearsalSid)) (P.Transferred 12)
        , P.Step (P.BulkOut 2 (debugRequestFrame address)) (P.Transferred debugRequestBytes)
        , P.Step (P.controlTransfer (P.control 0xd1 debugReplyForHalf rehearsalSid)) (P.Transferred 12)
        , P.Step (P.BulkIn 0x81 debugReplyForHalf) (P.Reply (probeHalfReply address (replicate probeHalfBytes 0x22))) ]

probeBootRomScript :: ScriptedOperation
probeBootRomScript = ScriptedOperation "probe-bootrom" bootRomSteps $ \clock api dir -> do
  windows <- runProbeBootRom clock api dir
  pure (show windows ++ " windows")

rehearseProbeBootRom :: IO ()
rehearseProbeBootRom = runRehearsal realClock probeBootRomScript

-- The area-2 windows have no oracle, so the scripted halves carry fixed filler.
area2Steps :: [P.Step]
area2Steps = openSteps ++ concatMap action area2Requests ++ closeSteps
  where
    action request = case request of
      Close -> []
      GateWrite value ->
        [ P.Step (P.controlTransfer (P.control 0xd0 configWriteBytes rehearsalSid)) (P.Transferred 12)
        , P.Step (P.BulkOut 2 (configWriteFrame value)) (P.Transferred configWriteBytes)
        , P.Step (P.controlTransfer (P.control 0xd1 configReplyBytes rehearsalSid)) (P.Transferred 12)
        , P.Step (P.BulkIn 0x81 configReplyBytes) (P.Reply probeWriteReply) ]
      GateRead label ->
        [ P.Step (P.controlTransfer (P.control 0xd0 readRequestBytes rehearsalSid)) (P.Transferred 12)
        , P.Step (P.BulkOut 2 (readRequestFrame readbackWindow)) (P.Transferred readRequestBytes)
        , P.Step (P.controlTransfer (P.control 0xd1 readReplyBytes rehearsalSid)) (P.Transferred 12)
        , P.Step (P.BulkIn 0x81 readReplyBytes) (P.Reply (probeGateWindowReply (gateRehearsalPayload (if label == gateReadbackEnabled then 1 else 0)))) ]
      Half address ->
        [ P.Step (P.controlTransfer (P.control 0xd0 debugRequestBytes rehearsalSid)) (P.Transferred 12)
        , P.Step (P.BulkOut 2 (debugRequestFrame address)) (P.Transferred debugRequestBytes)
        , P.Step (P.controlTransfer (P.control 0xd1 debugReplyForHalf rehearsalSid)) (P.Transferred 12)
        , P.Step (P.BulkIn 0x81 debugReplyForHalf) (P.Reply (probeHalfReply address (replicate probeHalfBytes 0x33))) ]

probeArea2Script :: ScriptedOperation
probeArea2Script = ScriptedOperation "probe-area2" area2Steps $ \clock api dir -> do
  windows <- runProbeArea2 clock api dir
  pure (show windows ++ " windows")

rehearseProbeArea2 :: IO ()
rehearseProbeArea2 = runRehearsal realClock probeArea2Script

-- The strap targets have no oracle, so the scripted payloads carry filler and
-- every reply echoes its request's exact address and length.
strapChunkReply :: Int -> Int -> [Word8] -> [Word8]
strapChunkReply address size payload = concat
  [ word32le debugOpcode, word32le 0, word32le 0, word32le 0
  , word32le 2, word32le 0, word32le 0
  , word32le address, word32le size ]
  ++ payload

strapSteps :: [P.Step]
strapSteps = openSteps ++ concatMap action strapRequests ++ closeSteps
  where
    action request = case request of
      StrapClose -> []
      StrapGateWrite value ->
        [ P.Step (P.controlTransfer (P.control 0xd0 configWriteBytes rehearsalSid)) (P.Transferred 12)
        , P.Step (P.BulkOut 2 (configWriteFrame value)) (P.Transferred configWriteBytes)
        , P.Step (P.controlTransfer (P.control 0xd1 configReplyBytes rehearsalSid)) (P.Transferred 12)
        , P.Step (P.BulkIn 0x81 configReplyBytes) (P.Reply probeWriteReply) ]
      StrapGateRead label ->
        [ P.Step (P.controlTransfer (P.control 0xd0 readRequestBytes rehearsalSid)) (P.Transferred 12)
        , P.Step (P.BulkOut 2 (readRequestFrame readbackWindow)) (P.Transferred readRequestBytes)
        , P.Step (P.controlTransfer (P.control 0xd1 readReplyBytes rehearsalSid)) (P.Transferred 12)
        , P.Step (P.BulkIn 0x81 readReplyBytes) (P.Reply (probeGateWindowReply (gateRehearsalPayload (if label == gateReadbackEnabled then 1 else 0)))) ]
      StrapRead address size ->
        [ P.Step (P.controlTransfer (P.control 0xd0 debugRequestBytes rehearsalSid)) (P.Transferred 12)
        , P.Step (P.BulkOut 2 (strapRequestFrame address size)) (P.Transferred debugRequestBytes)
        , P.Step (P.controlTransfer (P.control 0xd1 (strapReplyBytes size) rehearsalSid)) (P.Transferred 12)
        , P.Step (P.BulkIn 0x81 (strapReplyBytes size)) (P.Reply (strapChunkReply address size (replicate size 0x55))) ]

probeStrapsScript :: ScriptedOperation
probeStrapsScript = ScriptedOperation "probe-straps" strapSteps $ \clock api dir -> do
  bytes <- runProbeStraps clock api dir
  pure (show bytes ++ " bytes")

rehearseProbeStraps :: IO ()
rehearseProbeStraps = runRehearsal realClock probeStrapsScript

-- The code windows have no oracle, so the scripted halves carry fixed filler.
codeSteps :: [P.Step]
codeSteps = openSteps ++ concatMap action codeRequests ++ closeSteps
  where
    action request = case request of
      Close -> []
      GateWrite value ->
        [ P.Step (P.controlTransfer (P.control 0xd0 configWriteBytes rehearsalSid)) (P.Transferred 12)
        , P.Step (P.BulkOut 2 (configWriteFrame value)) (P.Transferred configWriteBytes)
        , P.Step (P.controlTransfer (P.control 0xd1 configReplyBytes rehearsalSid)) (P.Transferred 12)
        , P.Step (P.BulkIn 0x81 configReplyBytes) (P.Reply probeWriteReply) ]
      GateRead label ->
        [ P.Step (P.controlTransfer (P.control 0xd0 readRequestBytes rehearsalSid)) (P.Transferred 12)
        , P.Step (P.BulkOut 2 (readRequestFrame readbackWindow)) (P.Transferred readRequestBytes)
        , P.Step (P.controlTransfer (P.control 0xd1 readReplyBytes rehearsalSid)) (P.Transferred 12)
        , P.Step (P.BulkIn 0x81 readReplyBytes) (P.Reply (probeGateWindowReply (gateRehearsalPayload (if label == gateReadbackEnabled then 1 else 0)))) ]
      Half address ->
        [ P.Step (P.controlTransfer (P.control 0xd0 debugRequestBytes rehearsalSid)) (P.Transferred 12)
        , P.Step (P.BulkOut 2 (debugRequestFrame address)) (P.Transferred debugRequestBytes)
        , P.Step (P.controlTransfer (P.control 0xd1 debugReplyForHalf rehearsalSid)) (P.Transferred 12)
        , P.Step (P.BulkIn 0x81 debugReplyForHalf) (P.Reply (probeHalfReply address (replicate probeHalfBytes 0x77))) ]

probeCodeScript :: ScriptedOperation
probeCodeScript = ScriptedOperation "probe-code" codeSteps $ \clock api dir -> do
  windows <- runProbeCode clock api dir
  pure (show windows ++ " windows")

rehearseProbeCode :: IO ()
rehearseProbeCode = runRehearsal realClock probeCodeScript

-- The ownership windows have no oracle, so the scripted halves carry fixed
-- filler.
ownershipSteps :: [P.Step]
ownershipSteps = openSteps ++ concatMap action ownershipRequests ++ closeSteps
  where
    action request = case request of
      Close -> []
      GateWrite value ->
        [ P.Step (P.controlTransfer (P.control 0xd0 configWriteBytes rehearsalSid)) (P.Transferred 12)
        , P.Step (P.BulkOut 2 (configWriteFrame value)) (P.Transferred configWriteBytes)
        , P.Step (P.controlTransfer (P.control 0xd1 configReplyBytes rehearsalSid)) (P.Transferred 12)
        , P.Step (P.BulkIn 0x81 configReplyBytes) (P.Reply probeWriteReply) ]
      GateRead label ->
        [ P.Step (P.controlTransfer (P.control 0xd0 readRequestBytes rehearsalSid)) (P.Transferred 12)
        , P.Step (P.BulkOut 2 (readRequestFrame readbackWindow)) (P.Transferred readRequestBytes)
        , P.Step (P.controlTransfer (P.control 0xd1 readReplyBytes rehearsalSid)) (P.Transferred 12)
        , P.Step (P.BulkIn 0x81 readReplyBytes) (P.Reply (probeGateWindowReply (gateRehearsalPayload (if label == gateReadbackEnabled then 1 else 0)))) ]
      Half address ->
        [ P.Step (P.controlTransfer (P.control 0xd0 debugRequestBytes rehearsalSid)) (P.Transferred 12)
        , P.Step (P.BulkOut 2 (debugRequestFrame address)) (P.Transferred debugRequestBytes)
        , P.Step (P.controlTransfer (P.control 0xd1 debugReplyForHalf rehearsalSid)) (P.Transferred 12)
        , P.Step (P.BulkIn 0x81 debugReplyForHalf) (P.Reply (probeHalfReply address (replicate probeHalfBytes 0x66))) ]

probeOwnershipScript :: ScriptedOperation
probeOwnershipScript = ScriptedOperation "probe-ownership" ownershipSteps $ \clock api dir -> do
  windows <- runProbeOwnership clock api dir
  pure (show windows ++ " windows")

rehearseProbeOwnership :: IO ()
rehearseProbeOwnership = runRehearsal realClock probeOwnershipScript

-- The route windows have no oracle, so every scripted payload follows its own
-- absolute address and each reply echoes its request's exact address and
-- length.
routeChunkReply :: Int -> Int -> [Word8] -> [Word8]
routeChunkReply address size payload = concat
  [ word32le debugOpcode, word32le 0, word32le 0, word32le 0
  , word32le 2, word32le 0, word32le 0
  , word32le address, word32le size ]
  ++ payload

routeRehearsalPayload :: Int -> Int -> [Word8]
routeRehearsalPayload address size = [fromIntegral ((address+offset) `mod` 251) | offset <- [0 .. size-1]]

routeSteps :: [RouteWindow] -> [P.Step]
routeSteps windows = openSteps ++ concatMap action (routeRequests windows) ++ closeSteps
  where
    action request = case request of
      RouteClose -> []
      RouteGateWrite value ->
        [ P.Step (P.controlTransfer (P.control 0xd0 configWriteBytes rehearsalSid)) (P.Transferred 12)
        , P.Step (P.BulkOut 2 (configWriteFrame value)) (P.Transferred configWriteBytes)
        , P.Step (P.controlTransfer (P.control 0xd1 configReplyBytes rehearsalSid)) (P.Transferred 12)
        , P.Step (P.BulkIn 0x81 configReplyBytes) (P.Reply probeWriteReply) ]
      RouteGateRead label ->
        [ P.Step (P.controlTransfer (P.control 0xd0 readRequestBytes rehearsalSid)) (P.Transferred 12)
        , P.Step (P.BulkOut 2 (readRequestFrame readbackWindow)) (P.Transferred readRequestBytes)
        , P.Step (P.controlTransfer (P.control 0xd1 readReplyBytes rehearsalSid)) (P.Transferred 12)
        , P.Step (P.BulkIn 0x81 readReplyBytes) (P.Reply (probeGateWindowReply (gateRehearsalPayload (if label == gateReadbackEnabled then 1 else 0)))) ]
      RouteRead address size ->
        [ P.Step (P.controlTransfer (P.control 0xd0 debugRequestBytes rehearsalSid)) (P.Transferred 12)
        , P.Step (P.BulkOut 2 (routeRequestFrame address size)) (P.Transferred debugRequestBytes)
        , P.Step (P.controlTransfer (P.control 0xd1 (routeReplyBytes size) rehearsalSid)) (P.Transferred 12)
        , P.Step (P.BulkIn 0x81 (routeReplyBytes size)) (P.Reply (routeChunkReply address size (routeRehearsalPayload address size))) ]

rehearseProbeRoutesFixed :: IO ()
rehearseProbeRoutesFixed = runRehearsal realClock (probeRoutesScript "probe-routes fixed" routeFixedWindows)

rehearseProbeRoutesDerived :: [RouteWindow] -> IO ()
rehearseProbeRoutesDerived = runRehearsal realClock . probeRoutesScript "probe-routes derived"

probeRoutesScript :: String -> [RouteWindow] -> ScriptedOperation
probeRoutesScript label windows = ScriptedOperation label (routeSteps windows) $ \clock api dir -> do
  requests <- runProbeRoutes clock api dir windows
  pure (show requests ++ " read requests")

-- The dump-ram requests have no oracle, so the scripted payloads carry fixed
-- filler and every reply echoes its request's exact address and length.
dumpChunkReply :: Int -> Int -> [Word8] -> [Word8]
dumpChunkReply address size payload = concat
  [ word32le debugOpcode, word32le 0, word32le 0, word32le 0
  , word32le 2, word32le 0, word32le 0
  , word32le address, word32le size ]
  ++ payload

dumpSteps :: DumpSegment -> [P.Step]
dumpSteps segment = openSteps ++ concatMap action (dumpRequests segment) ++ closeSteps
  where
    action request = case request of
      DumpClose -> []
      DumpGateWrite value ->
        [ P.Step (P.controlTransfer (P.control 0xd0 configWriteBytes rehearsalSid)) (P.Transferred 12)
        , P.Step (P.BulkOut 2 (configWriteFrame value)) (P.Transferred configWriteBytes)
        , P.Step (P.controlTransfer (P.control 0xd1 configReplyBytes rehearsalSid)) (P.Transferred 12)
        , P.Step (P.BulkIn 0x81 configReplyBytes) (P.Reply probeWriteReply) ]
      DumpGateRead label ->
        [ P.Step (P.controlTransfer (P.control 0xd0 readRequestBytes rehearsalSid)) (P.Transferred 12)
        , P.Step (P.BulkOut 2 (readRequestFrame readbackWindow)) (P.Transferred readRequestBytes)
        , P.Step (P.controlTransfer (P.control 0xd1 readReplyBytes rehearsalSid)) (P.Transferred 12)
        , P.Step (P.BulkIn 0x81 readReplyBytes) (P.Reply (probeGateWindowReply (gateRehearsalPayload (if label == gateReadbackEnabled then 1 else 0)))) ]
      DumpChunk address size ->
        [ P.Step (P.controlTransfer (P.control 0xd0 debugRequestBytes rehearsalSid)) (P.Transferred 12)
        , P.Step (P.BulkOut 2 (dumpRequestFrame address size)) (P.Transferred debugRequestBytes)
        , P.Step (P.controlTransfer (P.control 0xd1 (dumpReplyBytes size) rehearsalSid)) (P.Transferred 12)
        , P.Step (P.BulkIn 0x81 (dumpReplyBytes size)) (P.Reply (dumpChunkReply address size (replicate size 0x44))) ]

dumpRamScript :: DumpSegment -> ScriptedOperation
dumpRamScript segment = ScriptedOperation ("dump-ram "++segmentName segment) (dumpSteps segment) $ \clock api dir -> do
  requests <- runDumpRam segment clock api dir
  pure (show requests ++ " requests")

rehearseDumpRam :: DumpSegment -> IO ()
rehearseDumpRam = runRehearsal realClock . dumpRamScript

-- The synthetic camera for read-media: the map object at 0x0095dae0
-- carrying the fields the 2026-10-08 captures showed there: the start word
-- 0x380 at +0, the entry count 0x6c at +8 and entries 0x2c/0x32 at 0x56 and
-- 0x57, distinct and not the identity, so the two banks land in different
-- 8-block windows and each request derives its own selector and offset, and
-- a result byte that leaves the sentinel on the third poll.
syntheticMapBase :: Int
syntheticMapBase = 0x0095dae0

syntheticStart :: MapStart
syntheticStart = case mkMapStart 0x380 of
  Right start -> start
  Left reason -> error reason

syntheticEntries :: [(Int,Int)]
syntheticEntries = zip mapEntryIndexes [0x2c, 0x32]

syntheticEntryCount :: Int
syntheticEntryCount = 0x6c

syntheticBanks :: [BankPlan]
syntheticBanks = case deriveBankPlans syntheticStart syntheticEntryCount syntheticEntries of
  Right plans -> plans
  Left reason -> error reason

-- Ten distinct staging fields and a gate byte that is neither zero nor the
-- enable value, so a permutation or a constant in the restore cannot match.
snapshotPatches :: [(Int,Word8)]
snapshotPatches = [(0x80,0x77),(0xf7,0x02)] ++ zip [0x81..0x8a] [0x51..0x5a]

syntheticSnapshot :: CacheSnapshot
syntheticSnapshot = case cacheSnapshotOf (mediaWindow snapshotPatches) of
  Right snapshot -> snapshot
  Left reason -> error reason

mediaWindow :: [(Int,Word8)] -> [Word8]
mediaWindow patches = [maybe 0x5a id (lookup index patches) | index <- [0..255]]

mediaPollBytes :: BankPlan -> Int -> [(Int,Word8)]
mediaPollBytes plan count =
  [ (0x80, triggerByte TriggerArm)
  , (0x81, spareGateByte)
  , (0x82, blockSelectorByte (bankSelector plan))
  , (0x83, if count < 2 then resultSentinelByte else 0)
  , (0xf7, 1) ]

-- The synthetic staging bytes depend on the address and the in-slice offset,
-- so a swapped or shifted composition cannot match the capture.
mediaSliceBytes :: Int -> Int -> [Word8]
mediaSliceBytes address size = [fromIntegral ((address+i) `mod` 251) | i <- [0..size-1]]

mediaWriteReply :: CacheIndex -> Word8 -> [Word8]
mediaWriteReply index value = concat
  [ word32le readOpcode, word32le first, word32le second, word32le 0
  , word32le 3, word32le 1, word32le 0, word32le (cacheIndexOf index), word32le 1 ]
  ++ replicate 256 0
  where (first,second) = if index == TriggerIndex && value == triggerByte TriggerDisarm
                          then triggerStatus TriggerDisarm
                          else (0,0)

data MediaStep
  = MediaWindow String [(Int,Word8)]
  | MediaWrite CacheIndex Word8
  | MediaRam String Int Int [Word8]
  | MediaSlice Int Int
  deriving (Eq, Show)

mediaPlan :: [MediaStep]
mediaPlan =
  [ MediaWindow "cache-snapshot" snapshotPatches
  , MediaWrite RamGateIndex 1
  , MediaWindow gateReadbackEnabled [(0x80,0x77),(0xf7,1)]
  , MediaRam "group-2-state" stateAddress 2 [0x01,0x00]
  , MediaRam "map-pointer" mapPointerAddress 4 (word32le syntheticMapBase)
  , MediaRam "map-header" (mapStartAddress syntheticMapBase) mapHeaderBytes
      (word32le (mapStartBlock syntheticStart) ++ word32le 0 ++ word32le syntheticEntryCount)
  ]
  ++ [ MediaRam ("map-entry-"++show index) (mapEntryAddress syntheticMapBase index) 2 [fromIntegral entry, 0]
     | (index,entry) <- syntheticEntries ]
  ++ concatMap bankSteps (zip [0::Int ..] syntheticBanks)
  ++ [MediaWrite TriggerIndex (triggerByte TriggerDisarm)]
  ++ [MediaWrite index value | (index,value) <- restorePlan syntheticSnapshot]
  ++ [MediaWindow gateReadbackRestored [(0x80,0x33),(0xf7,snapshotGate syntheticSnapshot)]]
  where
    bankSteps (position,plan) =
      [ MediaWrite TriggerIndex (triggerByte TriggerDisarm)
      , MediaWrite SpareGateIndex spareGateByte
      , MediaWrite BlockIndex (blockSelectorByte (bankSelector plan))
      , MediaWrite ResultIndex resultSentinelByte
      , MediaWrite TriggerIndex (triggerByte TriggerArm) ]
      ++ [ MediaWindow ("result-poll-"++show position++"-"++show count) (mediaPollBytes plan count)
         | count <- [0..2] ]
      ++ map (uncurry MediaSlice) (bankSlices (bankOffset plan))

mediaSteps :: [P.Step]
mediaSteps = openSteps ++ concatMap step mediaPlan ++ closeSteps
  where
    step request = case request of
      MediaWindow _ patches ->
        [ P.Step (P.controlTransfer (P.control 0xd0 readRequestBytes rehearsalSid)) (P.Transferred 12)
        , P.Step (P.BulkOut 2 (readRequestFrame readbackWindow)) (P.Transferred readRequestBytes)
        , P.Step (P.controlTransfer (P.control 0xd1 readReplyBytes rehearsalSid)) (P.Transferred 12)
        , P.Step (P.BulkIn 0x81 readReplyBytes) (P.Reply (probeGateWindowReply (mediaWindow patches))) ]
      MediaWrite index value ->
        [ P.Step (P.controlTransfer (P.control 0xd0 configWriteBytes rehearsalSid)) (P.Transferred 12)
        , P.Step (P.BulkOut 2 (cacheWriteFrame index value)) (P.Transferred configWriteBytes)
        , P.Step (P.controlTransfer (P.control 0xd1 configReplyBytes rehearsalSid)) (P.Transferred 12)
        , P.Step (P.BulkIn 0x81 configReplyBytes) (P.Reply (mediaWriteReply index value)) ]
      MediaRam _ address size payload ->
        [ P.Step (P.controlTransfer (P.control 0xd0 debugRequestBytes rehearsalSid)) (P.Transferred 12)
        , P.Step (P.BulkOut 2 (mediaReadFrame address size)) (P.Transferred debugRequestBytes)
        , P.Step (P.controlTransfer (P.control 0xd1 (mediaReadReplyBytes size) rehearsalSid)) (P.Transferred 12)
        , P.Step (P.BulkIn 0x81 (mediaReadReplyBytes size)) (P.Reply (dumpChunkReply address size payload)) ]
      MediaSlice address size ->
        [ P.Step (P.controlTransfer (P.control 0xd0 debugRequestBytes rehearsalSid)) (P.Transferred 12)
        , P.Step (P.BulkOut 2 (mediaReadFrame address size)) (P.Transferred debugRequestBytes)
        , P.Step (P.controlTransfer (P.control 0xd1 (mediaReadReplyBytes size) rehearsalSid)) (P.Transferred 12)
        , P.Step (P.BulkIn 0x81 (mediaReadReplyBytes size)) (P.Reply (dumpChunkReply address size (mediaSliceBytes address size))) ]

readMediaScript :: ScriptedOperation
readMediaScript = ScriptedOperation "read-media" mediaSteps $ \clock api dir -> do
  bytes <- runReadMedia clock api dir
  pure (show bytes ++ " bytes")

rehearseReadMedia :: IO ()
rehearseReadMedia = runRehearsal realClock readMediaScript

-- The arm-auto-act rehearsal: the session as the camera answers it. The
-- witness windows are the boot state before the arm; the first watch poll
-- reads guard 0, and the second reads the settled state of the 13-byte
-- WT_LOG/END script, so the watch stops on its own predicate and the run
-- reports ran-to-end.
armPageBytes :: Word8 -> Word8 -> [Word8]
armPageBytes armByte gateByte = [ byte index | index <- [0 .. 255] ]
  where
    byte index
      | index == Arm.writeTargetIndex Arm.ArmByte = armByte
      | index == Arm.writeTargetIndex Arm.GateByte = gateByte
      | otherwise = 0

-- A 256-byte witness window with the given in-window patches.
patchWindow :: [(Int,[Word8])] -> [Word8]
patchWindow patches = foldl apply (replicate 256 0) patches
  where apply window (offset,bytes) = take offset window ++ bytes ++ drop (offset + length bytes) window

-- Window A as the loader and the END handler leave it: the guard set, the
-- loader's -1 sentinel, the statement index at -1, three records and the
-- first record's code and text.
armWindowFired :: [Word8]
armWindowFired = patchWindow
  [ (Arm.guardAddress - Arm.witnessWindowA, [1])
  , (Arm.sentinelAddress - Arm.witnessWindowA, replicate 4 0xff)
  , (Arm.statementIndexAddress - Arm.witnessWindowA, replicate 4 0xff)
  , (Arm.statementCountAddress - Arm.witnessWindowA, word32le 3)
  , (Arm.statementCodeAddress - Arm.witnessWindowA, word32le 0x18)
  , (Arm.statementTextAddress - Arm.witnessWindowA, map (fromIntegral . fromEnum) "WT_LOG") ]

-- The WT_LOG handler calls its writer with 0xfff (about 4 KiB).
armLogWriteBytes :: Int
armLogWriteBytes = 0xfff

-- Window B as the loader leaves it after a complete 13-byte read whose
-- WT_LOG statement ran: the input length at 13, the completion count at the
-- log write's 0xfff, in flight at 0 with status 0, and the selector back
-- at 1.
armWindowSettled :: [Word8]
armWindowSettled = patchWindow
  [ (Arm.inputLengthAddress - Arm.witnessWindowB, word32le 13)
  , (Arm.completionCountAddress - Arm.witnessWindowB, word32le armLogWriteBytes)
  , (Arm.selectorAddress - Arm.witnessWindowB, word32le 1) ]

armWriteReply :: Arm.WriteTarget -> [Word8]
armWriteReply target = concat
  [ word32le readOpcode, word32le 0, word32le 0, word32le 0
  , word32le configWriteFunction, word32le configMailbox, word32le 0
  , word32le (Arm.writeTargetIndex target), word32le configWriteLength ]
  ++ replicate 256 0

armSteps :: [P.Step]
armSteps =
  openSteps
  ++ pageRead (armPageBytes 0 0)
  ++ configStep Arm.GateByte Arm.gateEnabledByte
  ++ pageRead (armPageBytes 0 Arm.gateEnabledByte)
  ++ debugHalf 0x400c0800 (word32le 499 ++ replicate 124 0)
  ++ halves Arm.witnessWindowA (replicate probeWindowBytes 0)
  ++ halves Arm.witnessWindowB (replicate probeWindowBytes 0)
  ++ configStep Arm.ArmByte Arm.armValueByte
  ++ pageRead (armPageBytes Arm.armValueByte Arm.gateEnabledByte)
  ++ debugHalf Arm.witnessWindowA (replicate probeHalfBytes 0)
  ++ debugHalf (Arm.witnessWindowB + probeHalfBytes) (replicate probeHalfBytes 0)
  ++ debugHalf Arm.witnessWindowA (take probeHalfBytes armWindowFired)
  ++ debugHalf (Arm.witnessWindowB + probeHalfBytes) (drop probeHalfBytes armWindowSettled)
  ++ halves Arm.witnessWindowA armWindowFired
  ++ halves Arm.witnessWindowB armWindowSettled
  ++ debugHalf 0x400c0800 (word32le 9487 ++ replicate 124 0)
  ++ configStep Arm.GateByte Arm.gateDisabledByte
  ++ pageRead (armPageBytes Arm.armValueByte Arm.gateDisabledByte)
  ++ closeSteps
  where
    pageRead payload =
      [ P.Step (P.controlTransfer (P.control 0xd0 readRequestBytes rehearsalSid)) (P.Transferred 12)
      , P.Step (P.BulkOut 2 (readRequestFrame (ReadWindow readbackIndex 0 "arm rehearsal"))) (P.Transferred readRequestBytes)
      , P.Step (P.controlTransfer (P.control 0xd1 readReplyBytes rehearsalSid)) (P.Transferred 12)
      , P.Step (P.BulkIn 0x81 readReplyBytes) (P.Reply (probeGateWindowReply payload)) ]
    configStep target value =
      [ P.Step (P.controlTransfer (P.control 0xd0 configWriteBytes rehearsalSid)) (P.Transferred 12)
      , P.Step (P.BulkOut 2 (Arm.configWriteFrame target value)) (P.Transferred configWriteBytes)
      , P.Step (P.controlTransfer (P.control 0xd1 configReplyBytes rehearsalSid)) (P.Transferred 12)
      , P.Step (P.BulkIn 0x81 configReplyBytes) (P.Reply (armWriteReply target)) ]
    debugHalf address payload =
      [ P.Step (P.controlTransfer (P.control 0xd0 debugRequestBytes rehearsalSid)) (P.Transferred 12)
      , P.Step (P.BulkOut 2 (debugRequestFrame address)) (P.Transferred debugRequestBytes)
      , P.Step (P.controlTransfer (P.control 0xd1 debugReplyForHalf rehearsalSid)) (P.Transferred 12)
      , P.Step (P.BulkIn 0x81 debugReplyForHalf) (P.Reply (probeHalfReply address payload)) ]
    halves base window = concat [debugHalf address (slice address) | address <- Arm.witnessHalves base]
      where slice address = if address == base then take probeHalfBytes window else drop probeHalfBytes window

armAutoActScript :: ScriptedOperation
armAutoActScript = ScriptedOperation "arm-auto-act" armSteps $ \clock api dir -> do
  report <- Arm.runArmAutoAct Arm.RequirePresent clock api dir
  pure (Arm.armReportLine report)

rehearseArmAutoAct :: IO ()
rehearseArmAutoAct = runRehearsal realClock armAutoActScript

-- The commit-f7 rehearsal: window 0 is the reference with the direction's
-- start values at 0x80, 0x81 and 0xf7, group-2 state 1, and the trigger's
-- reply arrives after the commit, so the first poll reads 0x81 = 0x00.
data CommitStep
  = CommitWindow Int [(Int,Word8)]
  | CommitWriteStep CommitWrite
  | CommitState
  deriving (Eq, Show)

commitWindowBytes :: ReferenceWindow -> Int -> [(Int,Word8)] -> [Word8]
commitWindowBytes reference offset patches
  | offset == 0x000100 = take productByteOffset filler ++ productOracleBytes ++ drop (productByteOffset + length productOracleBytes) filler
  | offset == 0x000200 = take signatureByteOffset filler ++ [0x55,0xaa] ++ drop (signatureByteOffset+2) filler
  | otherwise = [maybe byte id (lookup index patches) | (index,byte) <- zip [0..] (referenceBytes reference)]
  where filler = replicate readWindowLength 0x11

commitPlan :: CommitDirection -> [CommitStep]
commitPlan direction =
  [ CommitWindow 0x000000 startBytes, CommitWindow 0x000100 [], CommitWindow 0x000200 [] ]
  ++ [CommitState | direction == RestoreStoredGate]
  ++ [ CommitWriteStep (SetGate (directionTarget direction)), CommitWindow 0 gated ]
  ++ [CommitState | direction == EnableStoredGate]
  ++ [ CommitWriteStep ArmResult, CommitWindow 0 armed, CommitWriteStep TriggerCommit
     , CommitWindow 0 (result 0x22 0), CommitWriteStep DisarmCommit, CommitWindow 0 (result 0x33 0) ]
  where
    startTrigger = if direction == EnableStoredGate then 0x00 else 0x22
    startResult = if direction == EnableStoredGate then 0x00 else 0xff
    target = gateValueByte (directionTarget direction)
    startBytes = [(0x80,startTrigger),(0x81,startResult),(0xf7,gateValueByte (directionStart direction))]
    gated = [(0x80,startTrigger),(0x81,startResult),(0xf7,target)]
    armed = [(0x80,startTrigger),(0x81,0xff),(0xf7,target)]
    result trigger code = [(0x80,trigger),(0x81,code),(0xf7,target)]

commitSteps :: ReferenceWindow -> CommitDirection -> [P.Step]
commitSteps reference direction = openSteps ++ identityExchange ++ concatMap step (commitPlan direction) ++ closeSteps
  where
    step request = case request of
      CommitWindow offset patches ->
        [ P.Step (P.controlTransfer (P.control 0xd0 readRequestBytes rehearsalSid)) (P.Transferred 12)
        , P.Step (P.BulkOut 2 (readRequestFrame (ReadWindow 0 offset ""))) (P.Transferred readRequestBytes)
        , P.Step (P.controlTransfer (P.control 0xd1 readReplyBytes rehearsalSid)) (P.Transferred 12)
        , P.Step (P.BulkIn 0x81 readReplyBytes) (P.Reply (commitWindowReply offset (commitWindowBytes reference offset patches))) ]
      CommitWriteStep write ->
        [ P.Step (P.controlTransfer (P.control 0xd0 configWriteBytes rehearsalSid)) (P.Transferred 12)
        , P.Step (P.BulkOut 2 (commitWriteFrame write)) (P.Transferred configWriteBytes)
        , P.Step (P.controlTransfer (P.control 0xd1 configReplyBytes rehearsalSid)) (P.Transferred 12)
        , P.Step (P.BulkIn 0x81 configReplyBytes) (P.Reply (commitWriteReply write)) ]
      CommitState ->
        [ P.Step (P.controlTransfer (P.control 0xd0 debugRequestBytes rehearsalSid)) (P.Transferred 12)
        , P.Step (P.BulkOut 2 (mediaReadFrame stateAddress 2)) (P.Transferred debugRequestBytes)
        , P.Step (P.controlTransfer (P.control 0xd1 (mediaReadReplyBytes 2) rehearsalSid)) (P.Transferred 12)
        , P.Step (P.BulkIn 0x81 (mediaReadReplyBytes 2)) (P.Reply (dumpChunkReply stateAddress 2 [0x01,0x00])) ]
    commitWindowReply offset payload = concat
      [ word32le readOpcode, word32le 0, word32le 0, word32le 0
      , word32le 2, word32le 1, word32le (offset `div` readWindowLength), word32le 0, word32le readWindowLength ]
      ++ payload
    commitWriteReply write = concat
      [ word32le readOpcode, word32le first, word32le second, word32le 0
      , word32le 3, word32le 1, word32le 0, word32le (commitWriteIndex write), word32le 1 ]
      ++ replicate 256 0
      where (first,second) = commitWriteStatus write

commitF7Script :: CommitDirection -> ReferenceWindow -> ScriptedOperation
commitF7Script direction reference = ScriptedOperation (directionName direction) (commitSteps reference direction) $ \clock api dir -> do
  outcome <- runCommitF7 direction reference clock api dir
  case outcome of
    Committed _ -> pure (commitOutcomeLine direction outcome)
    _ -> P.reject ("rehearsal did not commit: "++commitOutcomeLine direction outcome)

rehearseCommitF7 :: CommitDirection -> ReferenceWindow -> IO ()
rehearseCommitF7 direction reference = do
  putStrLn ("budget exception: the D1 control after the 0x80 = 0x22 frame waits up to " ++ show commitAllowanceMs ++ " ms")
  runRehearsal realClock (commitF7Script direction reference)

-- The read-ram rehearsal: window 0 carries 0xf7 = 0x01 and every RAM reply
-- echoes its request with payload that follows the address.
readRamSteps :: RamWatch -> [P.Step]
readRamSteps watch = openSteps ++ identityExchange ++ gateRead ++ concat (replicate passes ramReads) ++ closeSteps
  where
    passes = pacingPasses (watchPacing watch)
    gateRead =
      [ P.Step (P.controlTransfer (P.control 0xd0 readRequestBytes rehearsalSid)) (P.Transferred 12)
      , P.Step (P.BulkOut 2 (readRequestFrame gateWindow)) (P.Transferred readRequestBytes)
      , P.Step (P.controlTransfer (P.control 0xd1 readReplyBytes rehearsalSid)) (P.Transferred 12)
      , P.Step (P.BulkIn 0x81 readReplyBytes) (P.Reply (probeGateWindowReply (gateRehearsalPayload 1))) ]
    ramReads = concat
      [ [ P.Step (P.controlTransfer (P.control 0xd0 debugRequestBytes rehearsalSid)) (P.Transferred 12)
        , P.Step (P.BulkOut 2 (routeRequestFrame address size)) (P.Transferred debugRequestBytes)
        , P.Step (P.controlTransfer (P.control 0xd1 (routeReplyBytes size) rehearsalSid)) (P.Transferred 12)
        , P.Step (P.BulkIn 0x81 (routeReplyBytes size)) (P.Reply (routeChunkReply address size (routeRehearsalPayload address size))) ]
      | window <- watchWindows watch
      , let address = routeWindowAddress window
            size = routeWindowBytes window ]

readRamScript :: RamWatch -> ScriptedOperation
readRamScript watch = ScriptedOperation (watchOperationName (watchOperation watch)) (readRamSteps watch) $ \clock api dir -> do
  requests <- runReadRam watch clock api dir
  pure (show requests ++ " read requests; no write sent")

-- Pauses advance the clock without sleeping.
rehearseReadRam :: RamWatch -> IO ()
rehearseReadRam watch = do
  let pacing = watchPacing watch
  putStrLn ("pacing: passes=" ++ show (pacingPasses pacing) ++ " interval-ms=" ++ show (pacingIntervalMs pacing))
  skipped <- newIORef 0
  let clock = TimedClock ((+) <$> getMonotonicTimeNSec <*> readIORef skipped)
                         (\ms -> modifyIORef' skipped (+ fromIntegral ms * 1000000))
  runRehearsal clock (readRamScript watch)

-- The read-bulk camera. It answers C0, the tag-4 identity, config function 2,
-- the 0x450006 tag-0x80 query, 0x200001 function 2 and E1 from its memory and
-- the source pair the query publishes at 0x01469878, and refuses any other
-- frame or a transfer out of protocol order.
data BulkCamera = BulkCamera
  { bulkCameraGate :: Word8
  , bulkCameraPointer :: Int
  , bulkCameraCapacity :: Int
  , bulkCameraMemory :: Int -> Word8
  }

bulkRehearsalMemory :: Int -> Word8
bulkRehearsalMemory address = fromIntegral ((address `xor` (address `shiftR` 8) `xor` (address `shiftR` 16) `xor` (address `shiftR` 24)) .&. 0xff)

stockBulkCamera :: BulkCamera
stockBulkCamera = BulkCamera 1 bulkSourcePointer 0x100000 bulkRehearsalMemory

data BulkCameraState = BulkCameraState
  { pendingRequest :: Maybe Int, lastRequest :: [Word8], pendingReply :: Maybe [Word8], publishedSource :: Maybe (Int,Int) }

bulkCameraDevice :: BulkCamera -> IO (P.Transfer -> IO (Int,[Word8]))
bulkCameraDevice camera = do
  state <- newIORef (BulkCameraState Nothing [] Nothing Nothing)
  pure $ \packet -> do
    st <- readIORef state
    let refuse reason = P.reject ("bulk camera: "++reason++": "++show packet)
    case packet of
      P.ControlWrite 0x43 0 0 0 frame@(op:_) | length frame == 12 -> do
        unless (isNothing (pendingRequest st) && isNothing (pendingReply st)) (refuse "control frame while a transfer is pending")
        let be32 at = foldl (\acc byte -> acc*256 + fromIntegral byte) 0 (take 4 (drop at frame)) :: Int
            be16 at = fromIntegral (frame!!at)*256 + fromIntegral (frame!!(at+1)) :: Int
            session = be16 10 == rehearsalSid
        next <- case op of
          0xc0 -> pure st { pendingReply = Just openReply }
          0xd0 | session -> pure st { pendingRequest = Just (be16 8) }
          0xd1 | session -> do
            (reply,source) <- either refuse pure (bulkD1Reply camera (publishedSource st) (lastRequest st))
            unless (length reply == be16 8) (refuse ("D1 length differs from the "++show (length reply)++"-byte reply"))
            pure st { pendingReply = Just reply, publishedSource = source }
          0xe1 | session && be32 6 > 0 -> case publishedSource st of
            Nothing -> refuse "E1 before a tag-0x80 query"
            Just (pointer,capacity) ->
              let start = (pointer + be32 2) .&. 0xffffffff
              in pure st { pendingReply = Just [ bulkCameraMemory camera ((start + i) .&. 0xffffffff) | i <- [0 .. min (be32 6) capacity - 1] ] }
          0xc4 | session -> pure st { pendingReply = Just [0,0] }
          _ -> refuse "unknown control frame"
        writeIORef state next
        pure (12,[])
      P.BulkOut 2 bytes -> case pendingRequest st of
        Just count | count == length bytes -> writeIORef state st { pendingRequest = Nothing, lastRequest = bytes } >> pure (count,[])
        _ -> refuse "bulk OUT without a matching D0"
      P.BulkIn 0x81 count -> case pendingReply st of
        Just reply -> do
          writeIORef state st { pendingReply = Nothing }
          let sent = take count reply
          pure (length sent, sent)
        Nothing -> refuse "bulk IN without a reply"
      _ -> refuse "unsupported transfer"

-- The D1 reply to the last request and the source pair after it.
bulkD1Reply :: BulkCamera -> Maybe (Int,Int) -> [Word8] -> Either String ([Word8], Maybe (Int,Int))
bulkD1Reply camera source request
  | request == P.identityPacket = Right ([0,0,0x15,0] ++ replicate 12 0 ++ [88,0,45,0,77,0,49,0] ++ replicate 104 0, source)
  | request == readRequestFrame gateWindow = Right (probeGateWindowReply (gateRehearsalPayload (bulkCameraGate camera)), source)
  | request == bulkQueryFrame =
      Right (concatMap word32le [bulkQueryOpcode,0,0,0,0,bulkCameraCapacity camera,0,0], Just (bulkCameraPointer camera, bulkCameraCapacity camera))
  | length request == debugRequestBytes && word32At 0 request == debugOpcode && word32At 0x10 request == debugFunction =
      let address = word32At 0x1c request
          size = word32At 0x20 request
          published = maybe (replicate bulkSourceStateBytes 0) (\(pointer,capacity) -> word32le pointer ++ word32le capacity) source
          byteAt a | a >= bulkSourceStateAddress && a < bulkSourceStateAddress + bulkSourceStateBytes = published !! (a - bulkSourceStateAddress)
                   | otherwise = bulkCameraMemory camera a
      in if bulkCameraGate camera == 0
           then Right (concatMap word32le [debugOpcode,1,0xffff,0,debugFunction,0,0,address,size] ++ replicate size 0, source)
           else Right (routeChunkReply address size (map byteAt [address .. address+size-1]), source)
  | otherwise = Left "unknown D0 request"

bulkCameraApi :: BulkCamera -> IO UsbApi
bulkCameraApi camera = do
  device <- bulkCameraDevice camera
  pure (UsbApi
    (\_ -> pure ())
    (\_ -> pure [rehearsalCandidate])
    (\_ _ -> pure ())
    (\interface _ -> unless (interface == 0) (P.reject "interface not allowed"))
    (\packet _ -> device packet)
    (\interface _ -> unless (interface == 0) (P.reject "interface not allowed"))
    (\_ -> pure ())
    (\_ -> pure ()))

-- Against the bulk camera, whose memory byte at A is the xor of A's bytes.
rehearseReadBulk :: BulkWindow -> IO ()
rehearseReadBulk window = do
  putStrLn "rehearsal: read-bulk; no native provider, USB enumeration or camera execution"
  putStrLn ("synthetic SID: 0x1234; IN endpoint: 0x81; OUT endpoint: 0x02; transfer budget: " ++ show transferTimeoutMs ++ " ms")
  let frames =
        [ P.controlTransfer (P.control 0xc0 8 0), P.BulkIn 0x81 8 ]
        ++ exchange 32 P.identityPacket 128
        ++ exchange readRequestBytes (readRequestFrame gateWindow) readReplyBytes
        ++ exchange bulkQueryRequestBytes bulkQueryFrame bulkQueryReplyBytes
        ++ exchange debugRequestBytes (routeRequestFrame bulkSourceStateAddress bulkSourceStateBytes) (routeReplyBytes bulkSourceStateBytes)
        ++ concat [ [bulkCommandFrame rehearsalSid chunk, P.BulkIn 0x81 (snd chunk)] | chunk <- bulkChunks window ]
        ++ [ P.controlTransfer (P.control 0xc4 2 rehearsalSid), P.BulkIn 0x81 2 ]
      exchange requestBytes frame replyBytes =
        [ P.controlTransfer (P.control 0xd0 requestBytes rehearsalSid), P.BulkOut 2 frame
        , P.controlTransfer (P.control 0xd1 replyBytes rehearsalSid), P.BulkIn 0x81 replyBytes ]
  forM_ (zip [1::Int ..] frames) $ \(index,packet) -> putStrLn (show index ++ ". " ++ describeTransfer packet)
  api <- bulkCameraApi stockBulkCamera
  report <- withRehearsalDirectory (runReadBulk window realClock api)
  putStrLn ("rehearsed " ++ readBulkResultLine report)
