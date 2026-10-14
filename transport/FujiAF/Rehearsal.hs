-- Offline rehearsal: print the exact transfer plan an operation will emit and
-- run that operation against a scripted reply. The native provider is never
-- constructed here, so no libusb is loaded.
module FujiAF.Rehearsal (rehearseIdentify, rehearseReadConfig, rehearseEndSetting, rehearseProbeRam, rehearseReadLog, rehearseProbeBootRom, rehearseProbeArea2, rehearseProbeStraps, rehearseProbeCode, rehearseProbeOwnership, rehearseProbeRoutesFixed, rehearseProbeRoutesDerived, rehearseDumpRam, rehearseReadMedia, rehearseArmAutoAct) where

import Control.Exception (bracket)
import Control.Monad (forM_, unless)
import Data.IORef
import Data.Word (Word8)
import System.Directory (createDirectory, getTemporaryDirectory, removeDirectoryRecursive)
import System.FilePath ((</>))
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
identifySteps = openSteps ++
  [ P.Step (P.controlTransfer (P.control 0xd0 32 rehearsalSid)) (P.Transferred 12)
  , P.Step (P.BulkOut 2 P.identityPacket) (P.Transferred 32)
  , P.Step (P.controlTransfer (P.control 0xd1 128 rehearsalSid)) (P.Transferred 12)
  , P.Step (P.BulkIn 0x81 128) (P.Reply identityReply) ] ++ closeSteps
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

runRehearsal :: String -> [P.Step] -> (UsbApi -> FilePath -> IO String) -> IO ()
runRehearsal operation steps run = do
  printPlan operation steps
  script <- newIORef steps
  result <- withRehearsalDirectory (run (scriptedApi script))
  leftover <- readIORef script
  unless (null leftover) (P.reject "rehearsal left planned transfers unsent")
  putStrLn ("rehearsed " ++ operation ++ ": " ++ result)

rehearseIdentify :: IO ()
rehearseIdentify = runRehearsal "identify" identifySteps $ \api dir -> do
  ProductModel model <- runIdentify realClock api dir
  pure model

rehearseReadConfig :: ReadPlan -> IO ()
rehearseReadConfig plan = runRehearsal "read-config" (readSteps plan) $ \api dir -> do
  windows <- runRead realClock api plan dir
  pure (show windows ++ " windows")

rehearseEndSetting :: IO ()
rehearseEndSetting = runRehearsal "end-setting" endSettingSteps $ \api dir ->
  outcomeName <$> runEndSetting realClock api dir

rehearseProbeRam :: IO ()
rehearseProbeRam = runRehearsal "probe-ram" probeSteps $ \api dir -> do
  probes <- runProbe realClock api dir
  pure (show probes ++ " probes")

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

rehearseReadLog :: IO ()
rehearseReadLog = runRehearsal "read-log" logSteps $ \api dir -> do
  windows <- runReadLog realClock api dir
  pure (show windows ++ " windows")

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

rehearseProbeBootRom :: IO ()
rehearseProbeBootRom = runRehearsal "probe-bootrom" bootRomSteps $ \api dir -> do
  windows <- runProbeBootRom realClock api dir
  pure (show windows ++ " windows")

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

rehearseProbeArea2 :: IO ()
rehearseProbeArea2 = runRehearsal "probe-area2" area2Steps $ \api dir -> do
  windows <- runProbeArea2 realClock api dir
  pure (show windows ++ " windows")

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

rehearseProbeStraps :: IO ()
rehearseProbeStraps = runRehearsal "probe-straps" strapSteps $ \api dir -> do
  bytes <- runProbeStraps realClock api dir
  pure (show bytes ++ " bytes")

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

rehearseProbeCode :: IO ()
rehearseProbeCode = runRehearsal "probe-code" codeSteps $ \api dir -> do
  windows <- runProbeCode realClock api dir
  pure (show windows ++ " windows")

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

rehearseProbeOwnership :: IO ()
rehearseProbeOwnership = runRehearsal "probe-ownership" ownershipSteps $ \api dir -> do
  windows <- runProbeOwnership realClock api dir
  pure (show windows ++ " windows")

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
rehearseProbeRoutesFixed = rehearseProbeRoutes "probe-routes fixed" routeFixedWindows

rehearseProbeRoutesDerived :: [RouteWindow] -> IO ()
rehearseProbeRoutesDerived windows = rehearseProbeRoutes "probe-routes derived" windows

rehearseProbeRoutes :: String -> [RouteWindow] -> IO ()
rehearseProbeRoutes label windows = runRehearsal label (routeSteps windows) $ \api dir -> do
  requests <- runProbeRoutes realClock api dir windows
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

rehearseDumpRam :: DumpSegment -> IO ()
rehearseDumpRam segment = runRehearsal ("dump-ram "++segmentName segment) (dumpSteps segment) $ \api dir -> do
  requests <- runDumpRam segment realClock api dir
  pure (show requests ++ " requests")

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

rehearseReadMedia :: IO ()
rehearseReadMedia = runRehearsal "read-media" mediaSteps $ \api dir -> do
  bytes <- runReadMedia realClock api dir
  pure (show bytes ++ " bytes")

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

rehearseArmAutoAct :: IO ()
rehearseArmAutoAct = runRehearsal "arm-auto-act" armSteps $ \api dir -> do
  report <- Arm.runArmAutoAct Arm.RequirePresent realClock api dir
  pure (Arm.armReportLine report)
