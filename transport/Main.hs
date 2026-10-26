module Main where

import Control.Exception
import Control.Monad (unless, when)
import Data.Time.Clock (getCurrentTime)
import Data.Time.Format (defaultTimeLocale, formatTime)
import System.Directory
import System.Environment
import System.Exit (exitFailure)
import System.FilePath
import System.IO (hPutStrLn, stderr, stdout, hFlush)
import System.Posix.Files (setFileMode)
import System.Posix.IO
import System.Posix.Unistd (fileSynchronise)
import FujiAF.Transport
import FujiAF.Identify
import FujiAF.Read
import FujiAF.EndSetting
import FujiAF.Probe
import FujiAF.Log
import FujiAF.BootRom
import FujiAF.Area2
import FujiAF.ArmAutoAct
import FujiAF.CodeProbe
import FujiAF.DumpRam
import FujiAF.ReadMedia
import FujiAF.Straps
import FujiAF.Ownership
import FujiAF.StoredGate
import FujiAF.Routes
import FujiAF.ReadRam
import FujiAF.ReadBulk
import FujiAF.RamPush
import FujiAF.PlanDocument (readPlanFile, readRoutePlanFile)
import FujiAF.Native (nativeApi)
import FujiAF.Rehearsal
import qualified FujiAF.Broker as P
import RamPushReviewedPins (reviewedContract)
import RamPushCcReviewedPins (reviewedCcContract)
import RamPushCcIconReviewedPins (reviewedCcIconContract)
import RamPushToggleReviewedPins (reviewedToggleContract)

data RouteSource = RoutesFixed | RoutesDerived FilePath
data Operation = Identify | ReadConfig FilePath | EndSetting | ProbeRam | ReadLog | ProbeBootRom | ProbeArea2 | ProbeStraps | ProbeCode | ProbeOwnership | ProbeRoutes RouteSource | DumpRam DumpSegment | ReadMedia | ArmAutoAct DeviceStart | CommitF7 CommitDirection FilePath | ReadRam FilePath (Maybe (Int,Int)) | ProbeSdHost (Maybe (Int,Int)) | ReadBulk Int Int | RamAlloc Int | RamRelink FilePath | RamPush FilePath FilePath | RamFree FilePath | RamUnload FilePath
data Command = Help | Live Operation (Maybe FilePath) | Rehearse Operation

operationName :: Operation -> String
operationName Identify = "identify"
operationName (ReadConfig _) = "read-config"
operationName EndSetting = "end-setting"
operationName ProbeRam = "probe-ram"
operationName ReadLog = "read-log"
operationName ProbeBootRom = "probe-bootrom"
operationName ProbeArea2 = "probe-area2"
operationName ProbeStraps = "probe-straps"
operationName ProbeCode = "probe-code"
operationName ProbeOwnership = "probe-ownership"
operationName (ProbeRoutes _) = "probe-routes"
operationName (DumpRam segment) = "dump-ram-" ++ segmentName segment
operationName ReadMedia = "read-media"
operationName (ArmAutoAct _) = "arm-auto-act"
operationName (CommitF7 direction _) = directionName direction
operationName (ReadRam _ _) = "read-ram"
operationName (ProbeSdHost _) = "probe-sd-host"
operationName (ReadBulk _ _) = "read-bulk"

operationName (RamAlloc _) = "ram-alloc"
operationName (RamRelink _) = "ram-relink"
operationName (RamPush _ _) = "ram-push"
operationName (RamFree _) = "ram-free"
operationName (RamUnload _) = "ram-unload"
main :: IO ()
main = (getArgs >>= run) `catch` \(e::SomeException) -> hPutStrLn stderr ("camera: "++displayException e) >> exitFailure

run :: [String] -> IO ()
run arguments = maybe (P.reject "unsupported command; see --help") dispatch (parse arguments)

parse :: [String] -> Maybe Command
parse ["--help"] = Just Help
parse ("identify":rest) = Live Identify <$> output rest
parse ("read-config":plan:rest) = Live (ReadConfig plan) <$> output rest
parse ("end-setting":rest) = Live EndSetting <$> output rest
parse ("probe-ram":rest) = Live ProbeRam <$> output rest
parse ("read-log":rest) = Live ReadLog <$> output rest
parse ("probe-bootrom":rest) = Live ProbeBootRom <$> output rest
parse ("probe-area2":rest) = Live ProbeArea2 <$> output rest
parse ("probe-straps":rest) = Live ProbeStraps <$> output rest
parse ("probe-code":rest) = Live ProbeCode <$> output rest
parse ("probe-ownership":rest) = Live ProbeOwnership <$> output rest
parse ("probe-routes":"fixed":rest) = Live (ProbeRoutes RoutesFixed) <$> output rest
parse ("probe-routes":"derived":plan:rest) = Live (ProbeRoutes (RoutesDerived plan)) <$> output rest
parse ("dump-ram":tokens) = (\(segment,directory) -> Live (DumpRam segment) directory) <$> dumpSegmentArguments tokens
parse ("read-media":rest) = Live ReadMedia <$> output rest
parse ("arm-auto-act":"--wait-for-device":rest) = Live (ArmAutoAct WaitForDevice) <$> output rest
parse ("arm-auto-act":rest) = Live (ArmAutoAct RequirePresent) <$> output rest
parse ("commit-f7-enable":reference:rest) = Live (CommitF7 EnableStoredGate reference) <$> output rest
parse ("commit-f7-restore":reference:rest) = Live (CommitF7 RestoreStoredGate reference) <$> output rest
parse ("read-ram":tokens) = (\(plan,pacing,directory) -> Live (ReadRam plan pacing) directory) <$> readRamArguments tokens
parse ("probe-sd-host":tokens) = (\(pacing,directory) -> Live (ProbeSdHost pacing) directory) <$> pacingArguments tokens
parse ("read-bulk":tokens) = (\(address,size,directory) -> Live (ReadBulk address size) directory) <$> readBulkArguments tokens
parse ("ram-alloc":tokens) = (\(size,directory) -> Live (RamAlloc size) directory) <$> ramAllocArguments tokens
parse ("ram-relink":tokens) = (\(state,directory) -> Live (RamRelink state) (Just directory)) <$> ramRelinkArguments tokens
parse ("ram-push":tokens) = (\(state,manifest,directory) -> Live (RamPush state manifest) directory) <$> ramPushArguments tokens
parse ("ram-free":tokens) = (\(state,directory) -> Live (RamFree state) directory) <$> ramFreeArguments tokens
parse ("ram-unload":tokens) = (\(state,directory) -> Live (RamUnload state) directory) <$> ramUnloadArguments tokens
parse ["rehearse","identify"] = Just (Rehearse Identify)
parse ["rehearse","read-config",plan] = Just (Rehearse (ReadConfig plan))
parse ["rehearse","end-setting"] = Just (Rehearse EndSetting)
parse ["rehearse","probe-ram"] = Just (Rehearse ProbeRam)
parse ["rehearse","read-log"] = Just (Rehearse ReadLog)
parse ["rehearse","probe-bootrom"] = Just (Rehearse ProbeBootRom)
parse ["rehearse","probe-area2"] = Just (Rehearse ProbeArea2)
parse ["rehearse","probe-straps"] = Just (Rehearse ProbeStraps)
parse ["rehearse","probe-code"] = Just (Rehearse ProbeCode)
parse ["rehearse","probe-ownership"] = Just (Rehearse ProbeOwnership)
parse ["rehearse","probe-routes","fixed"] = Just (Rehearse (ProbeRoutes RoutesFixed))
parse ["rehearse","probe-routes","derived",plan] = Just (Rehearse (ProbeRoutes (RoutesDerived plan)))
parse ["rehearse","dump-ram",name] = Rehearse . DumpRam <$> readSegment name
parse ["rehearse","read-media"] = Just (Rehearse ReadMedia)
parse ["rehearse","arm-auto-act"] = Just (Rehearse (ArmAutoAct RequirePresent))
parse ["rehearse","commit-f7-enable",reference] = Just (Rehearse (CommitF7 EnableStoredGate reference))
parse ["rehearse","commit-f7-restore",reference] = Just (Rehearse (CommitF7 RestoreStoredGate reference))
parse ("rehearse":"read-ram":tokens) = case readRamArguments tokens of
  Just (plan,pacing,Nothing) -> Just (Rehearse (ReadRam plan pacing))
  _ -> Nothing
parse ("rehearse":"probe-sd-host":tokens) = case pacingArguments tokens of
  Just (pacing,Nothing) -> Just (Rehearse (ProbeSdHost pacing))
  _ -> Nothing
parse ("rehearse":"read-bulk":tokens) = case readBulkArguments tokens of
  Just (address,size,Nothing) -> Just (Rehearse (ReadBulk address size))
  _ -> Nothing
parse ["rehearse","ram-alloc",size] = case ramAllocArguments [size] of
  Just (bytes,Nothing) -> Just (Rehearse (RamAlloc bytes))
  _ -> Nothing
parse ["rehearse","ram-push",state,manifest] = Just (Rehearse (RamPush state manifest))
parse ["rehearse","ram-free",state] = Just (Rehearse (RamFree state))
parse ["rehearse","ram-unload",state] = Just (Rehearse (RamUnload state))
parse _ = Nothing

output :: [String] -> Maybe (Maybe FilePath)
output [] = Just Nothing
output [directory] = Just (Just directory)
output _ = Nothing

dispatch :: Command -> IO ()
dispatch Help = putStr $ unlines
  [ "fuji-af camera: the closed X-M1 service operations and their offline rehearsal"
  , "identify [OUTDIR]              product identity over C0/D0 tag 4/D1/C4"
  , "read-config PLAN_JSON [OUTDIR] bounded adjustment-area read of the plan's windows"
  , "end-setting [OUTDIR]           the fixed POWER OFF End Setting candidate, no C4"
  , "probe-ram [OUTDIR]             the gate enable, fixed RAM probe list and restore"
  , "read-log [OUTDIR]              the fixed 0x400a0800 log store windows, gate enable and restore"
  , "probe-bootrom [OUTDIR]         the fixed boot-ROM window list: the 0x80000000 lead range, the 0x80004000 repeat block, the 0x80008000 period probe and the 0x00000000 handoff window, gate enable and restore"
  , "probe-area2 [OUTDIR]           the fixed area-2 evidence windows, gate enable and restore"
  , "probe-straps [OUTDIR]          the fixed capture: the 0x400a0000 port mirror, the 0x80010000 and 0x80100000 alias windows and the 0xfff89014/0xfff98000 strap registers, gate enable and restore"
  , "probe-code [OUTDIR]             the seven windows covering the G3 stock native-case closure, gate enable and restore"
  , "probe-ownership [OUTDIR]        the fixed ownership capture: the semaphore-namespace table windows 0x0093e600-0x0093e900 and the bump-allocator cursor windows 0x00829900/0x00829a00, gate enable and restore"
  , "probe-routes fixed [OUTDIR]     the class descriptor table and adjacent objects at 0x0096bd20 (256 B, two requests) and the allocation-search cursor cell 0x00a0f86c (4 B), gate enable and restore"
  , "probe-routes derived PLAN_JSON [OUTDIR] the phase-2 RAM window list the plan file names, read one request per short window and two per 256-byte window, gate enable and restore"
  , "dump-ram SEGMENT [OUTDIR]      one named fixed RAM segment, gate enable and restore"
  , "read-media [OUTDIR]            one bounded raw storage read of the two area-2 banks, one request per bank: the cache snapshot, the gate, the state precondition, the map header (start word and entry count) and the two map entries at their own addresses, then per bank the map-derived selector, the result poll and the bank capture, with the disarm and restores on every exit path"
  , "arm-auto-act [--wait-for-device] [OUTDIR] the reviewed AUTO_ACT preconditions, arm and readback, first/end boot ticks and bounded witness watch, gate restore and C4; --wait-for-device polls for up to 120 s before the session; end by supply cut"
  , "commit-f7-enable REFERENCE_BIN [OUTDIR] stores adjustment index 0xf7 = 0x01 through the whole-area commit, which also stores 0x80 = 0x22, 0x81 = 0xff and every other cache byte: identity, start windows equal to the private at-rest window-0 capture REFERENCE_BIN outside 0x80/0x81/0xf7 with 0xa2 = 0, state precondition, 0xf7 write, 0x81 = 0xff arm, 0x80 = 0x22 trigger with its reply held up to 33 s, bounded result poll, 0x80 = 0x33 disarm, C4. Afterwards every ordinary-boot USB attach starts ff80 service mode 3 instead of PTP, runs a 41 MB 0xff fill and gives any host the debug RAM write, and a cable at a button-less boot gives mode 3; an ordinary firmware update with its refresh flags set returns 0xf7/0x80/0x81 to 0x00 and is the only reverse route not marked unproved; whether a jig session starts with 0xf7 stored, for verification or restore, is unproved. End by dummy-battery supply cut unless the result says to wait"
  , "commit-f7-restore REFERENCE_BIN [OUTDIR] the same sequence from stored 0xf7 = 0x01, storing 0xf7 = 0x00; stored 0x80 stays 0x22; needs a session to start with 0xf7 stored, which is unproved"
  , "read-ram PLAN_JSON [--repeat N --interval-ms M] [OUTDIR] read-only RAM watch on a body with 0xf7 already nonzero, in a jig boot or an ordinary boot's ff80 route: identity, config window 0x000000 refused unless 0xf7 != 0, then one 0x200001 function 2 request per plan window (1..248 bytes) for N passes M ms apart (N 1..600, M 100..10000, N * windows <= 2032), C4; no write is sent, so no supply cut is needed"
  , "probe-sd-host [--repeat N --interval-ms M] [OUTDIR] read-only capture of the SD slot's host-controller registers on a body with 0xf7 already nonzero: identity, config window 0x000000 refused unless 0xf7 != 0, then per pass 0xfff50000 +0x00 (32 B), +0x24 (6 B), +0x2c (16 B) and +0x3e (2 B), never the +0x20 data port, for N passes M ms apart (N 1..508, M 100..10000), C4; no write is sent"
  , "read-bulk ADDRESS LENGTH [OUTDIR] read one DRAM window through the stock ff80 bulk path on a body with 0xf7 already nonzero: identity, config window 0x000000 refused unless 0xf7 != 0, one 0x450006 tag-0x80 query, a function 2 read pinning the published source pointer 0x4b2ece00, then one E1 and one bulk IN per 1 MiB chunk, C4; ADDRESS is 0x-hex and 4-byte aligned, LENGTH 0x-hex and a multiple of 64, the window inside [0x00000000,0x20000000) or [0x40000000,0x60000000) with its physical end at most 0x0b3f6e00; no write is sent"
  , "ram-alloc SIZE [OUTDIR]       allocate and peer-prepare an X-M1 RAM workspace; SIZE is a payload's fixed bound, 0x1dc98 (recipe bank, D3 and Classic Chrome), 0x1560 (Classic Chrome), 0x21fc (Classic Chrome with icon) or 0xd20 (USB toggle); retain ram-alloc-state.json"
  , "ram-relink STATE OUTDIR        build the reviewed manifest and runtime of the allocation's payload at the retained base with the camera's pinned relinker; no USB"
  , "ram-push STATE MANIFEST [OUTDIR] upload a manifest linked to the retained allocation, install and write ram-push-state.json"
  , "ram-free STATE [OUTDIR]       free a retained allocation that was not installed"
  , "ram-unload STATE [OUTDIR]     rollback and verify the exact active publication set; allocation remains retained"
  , "rehearse identify|read-config PLAN_JSON|end-setting|probe-ram|read-log|probe-bootrom|probe-area2|probe-straps|probe-code|probe-ownership|probe-routes fixed|probe-routes derived PLAN_JSON|dump-ram SEGMENT|read-media|arm-auto-act|commit-f7-enable REFERENCE_BIN|commit-f7-restore REFERENCE_BIN|read-ram PLAN_JSON [--repeat N --interval-ms M]|probe-sd-host [--repeat N --interval-ms M]|read-bulk ADDRESS LENGTH|ram-alloc SIZE|ram-push STATE MANIFEST|ram-free STATE|ram-unload STATE"
  , "--help"
  , "OUTDIR must be absolute and must not exist; it is created with mode 0700."
  , "Without OUTDIR a live operation writes to"
  , "$BUILD_WORKSPACE_DIRECTORY/private-data/sessions/<UTC>-<operation>."
  , "Help and rehearse never load libusb, enumerate or open USB."
  ]
dispatch (Rehearse Identify) = rehearseIdentify
dispatch (Rehearse (ReadConfig path)) = readPlanFile path >>= rehearseReadConfig
dispatch (Rehearse EndSetting) = rehearseEndSetting
dispatch (Rehearse ProbeRam) = rehearseProbeRam
dispatch (Rehearse ReadLog) = rehearseReadLog
dispatch (Rehearse ProbeBootRom) = rehearseProbeBootRom
dispatch (Rehearse ProbeArea2) = rehearseProbeArea2
dispatch (Rehearse ProbeStraps) = rehearseProbeStraps
dispatch (Rehearse ProbeCode) = rehearseProbeCode
dispatch (Rehearse ProbeOwnership) = rehearseProbeOwnership
dispatch (Rehearse (ProbeRoutes RoutesFixed)) = rehearseProbeRoutesFixed
dispatch (Rehearse (ProbeRoutes (RoutesDerived path))) = readRoutePlanFile path >>= rehearseProbeRoutesDerived
dispatch (Rehearse (DumpRam segment)) = rehearseDumpRam segment
dispatch (Rehearse ReadMedia) = rehearseReadMedia
dispatch (Rehearse (ArmAutoAct _)) = rehearseArmAutoAct
dispatch (Rehearse (CommitF7 direction path)) = do
  reference <- readReferenceWindow path
  putStrLn ("reference: "++path++" sha256="++referenceDigest reference)
  rehearseCommitF7 direction reference
dispatch (Rehearse (ReadRam path pacing)) = ramWatch path pacing >>= rehearseReadRam
dispatch (Rehearse (ProbeSdHost pacing)) = sdHostPacedWatch pacing >>= rehearseReadRam
dispatch (Rehearse (ReadBulk address size)) = either P.reject pure (mkBulkWindow address size) >>= rehearseReadBulk
dispatch (Rehearse (RamAlloc size)) = rehearseRamAlloc size
dispatch (Rehearse (RamPush state manifest)) = do
  allocation <- readRamAllocState state
  plan <- readRamPushManifest manifest
  rehearseRamPush allocation plan
dispatch (Rehearse (RamFree path)) = readRamAllocState path >>= rehearseRamFree
dispatch (Rehearse (RamUnload path)) = readRamPushState path >>= rehearseRamUnload
dispatch (Rehearse (RamRelink _)) = P.reject "ram-relink has no rehearsal"
dispatch (Live operation requested) = case operation of
  Identify -> withOutput operation requested $ \directory -> do
    ProductModel model <- runIdentify realClock nativeApi directory
    pure ("identify: "++model)
  ReadConfig path -> do
    -- The plan is parsed and validated before the output directory exists.
    plan <- readPlanFile path
    withOutput operation requested $ \directory -> do
      windows <- runRead realClock nativeApi plan directory
      pure ("read-config: "++show windows++" windows")
  EndSetting -> withOutput operation requested $ \directory -> do
    outcome <- runEndSetting realClock nativeApi directory
    pure ("end-setting: "++outcomeName outcome)
  ProbeRam -> withOutput operation requested $ \directory -> do
    probes <- runProbe realClock nativeApi directory
    pure ("probe-ram: "++show probes++" windows")
  ReadLog -> withOutput operation requested $ \directory -> do
    windows <- runReadLog realClock nativeApi directory
    pure ("read-log: "++show windows++" windows")
  ProbeBootRom -> withOutput operation requested $ \directory -> do
    windows <- runProbeBootRom realClock nativeApi directory
    pure ("probe-bootrom: "++show windows++" windows")
  ProbeArea2 -> withOutput operation requested $ \directory -> do
    windows <- runProbeArea2 realClock nativeApi directory
    pure ("probe-area2: "++show windows++" windows")
  ProbeStraps -> withOutput operation requested $ \directory -> do
    bytes <- runProbeStraps realClock nativeApi directory
    pure ("probe-straps: "++show bytes++" bytes")
  ProbeCode -> withOutput operation requested $ \directory -> do
    windows <- runProbeCode realClock nativeApi directory
    pure ("probe-code: "++show windows++" windows")
  ProbeOwnership -> withOutput operation requested $ \directory -> do
    windows <- runProbeOwnership realClock nativeApi directory
    pure ("probe-ownership: "++show windows++" windows")
  ProbeRoutes source -> do
    -- The derived plan is parsed and validated before the output directory exists.
    windows <- case source of
      RoutesFixed -> pure routeFixedWindows
      RoutesDerived path -> readRoutePlanFile path
    withOutput operation requested $ \directory -> do
      requests <- runProbeRoutes realClock nativeApi directory windows
      pure ("probe-routes: "++show requests++" read requests")
  DumpRam segment -> withOutput operation requested $ \directory -> do
    requests <- runDumpRam segment realClock nativeApi directory
    pure ("dump-ram "++segmentName segment++": "++show requests++" requests")
  ReadMedia -> withOutput operation requested $ \directory -> do
    bytes <- runReadMedia realClock nativeApi directory
    pure ("read-media: "++show bytes++" bytes")
  ArmAutoAct mode -> withOutput operation requested $ \directory -> do
    report <- runArmAutoAct mode realClock nativeApi directory
    pure ("arm-auto-act: "++armReportLine report)
  CommitF7 direction path -> do
    -- The reference is read and validated before the output directory exists.
    reference <- readReferenceWindow path
    putStrLn ("reference: "++path++" sha256="++referenceDigest reference)
    withOutput operation requested $ \directory -> do
      outcome <- runCommitF7 direction reference realClock nativeApi directory
      let line = directionName direction++": "++commitOutcomeLine direction outcome
      case outcome of
        Committed _ -> pure line
        _ -> P.reject line
  ReadRam path pacing -> do
    -- The plan and pacing are validated before the output directory exists.
    watch <- ramWatch path pacing
    withOutput operation requested $ \directory -> do
      requests <- runReadRam watch realClock nativeApi directory
      pure ("read-ram: "++show requests++" read requests;passes="++show (pacingPasses (watchPacing watch))
            ++";no write sent, so the session needs no supply cut")
  ProbeSdHost pacing -> do
    watch <- sdHostPacedWatch pacing
    withOutput operation requested $ \directory -> do
      requests <- runReadRam watch realClock nativeApi directory
      pure ("probe-sd-host: "++show requests++" read requests;passes="++show (pacingPasses (watchPacing watch))
            ++";no write sent, so the session needs no supply cut")
  ReadBulk address size -> do
    -- The window is validated before the output directory exists.
    window <- either P.reject pure (mkBulkWindow address size)
    withOutput operation requested $ \directory ->
      readBulkResultLine <$> runReadBulk window realClock nativeApi directory
  RamAlloc size -> withOutput operation requested $ \directory -> do
    outcome <- runRamAlloc size realClock nativeApi directory
    case outcome of
      RamAllocated state -> pure ("ram-alloc: retained;state="++state++";relink before ram-push")
      RamAllocRefused reason -> P.reject ("ram-alloc refused: "++reason)
      RamAllocUncertain reason -> P.reject ("ram-alloc uncertain: "++reason++";send nothing further and power off")
  RamRelink path -> do
    allocation <- readRetainedAllocation path
    refuseAfRingLive (allocSize (retainedAllocation allocation))
    withOutput operation requested $ \directory -> do
      manifest <- runRamRelink (contractFor (allocSize (retainedAllocation allocation))) allocation directory
      pure ("ram-relink: manifest="++manifest)
  RamPush statePath manifestPath -> do
    allocation <- readRetainedAllocation statePath
    manifest <- readRamPushManifest manifestPath
    refuseAfRingLive (allocSize (retainedAllocation allocation))
    withOutput operation requested $ \directory -> do
      outcome <- runRamPush (contractFor (allocSize (retainedAllocation allocation))) allocation manifest realClock nativeApi directory
      case outcome of
        RamPushInstalled state -> pure ("ram-push: installed;state="++state++";exit requires full power removal and stock recovery checks")
        RamPushRefused reason -> P.reject ("ram-push refused: "++reason)
        RamPushUncertain reason -> P.reject ("ram-push uncertain: "++reason++";send nothing further and power off")
  RamFree path -> do
    allocation <- readRetainedAllocation path
    withOutput operation requested $ \directory -> do
      outcome <- runRamFree allocation realClock nativeApi directory
      case outcome of
        RamFreed -> pure "ram-free: allocation freed"
        RamFreeRefused reason -> P.reject ("ram-free refused: "++reason)
        RamFreeUncertain reason -> P.reject ("ram-free uncertain: "++reason++";send nothing further and power off")
  RamUnload path -> do
    state <- readRamPushState path
    refuseAfRingLive (stateSize state)
    when (fmap payloadKind (payloadOfSize (stateSize state)) == Just UsbToggle)
      (P.reject "ram-unload refused: USB toggle requires full battery removal; a suspended stub frame cannot be rolled back in place")
    withOutput operation requested $ \directory -> do
      outcome <- runRamUnload (contractFor (stateSize state)) state realClock nativeApi directory
      case outcome of
        RamUnloaded -> pure "ram-unload: rollback verified; allocation retained; full power removal and stock recovery checks required"
        RamUnloadRefused reason -> P.reject ("ram-unload refused: "++reason)
        RamUnloadUncertain reason -> P.reject ("ram-unload uncertain: "++reason++";send nothing further and power off")

-- | The compiled contract of the payload whose allocation has `size` bytes.
contractFor :: Int -> ReviewedContract
contractFor size = case payloadKind <$> payloadOfSize size of
  Just ClassicChrome -> reviewedCcContract
  Just ClassicChromeIcon -> reviewedCcIconContract
  Just UsbToggle -> reviewedToggleContract
  _ -> reviewedContract

refuseAfRingLive :: Int -> IO ()
refuseAfRingLive size = when (fmap payloadKind (payloadOfSize size) == Just AfRing)
  (P.reject "AF ring has no approved live reviewed contract; use the host-only AF ring harness")

ramWatch :: FilePath -> Maybe (Int,Int) -> IO RamWatch
ramWatch path pacing = do
  windows <- readRoutePlanFile path
  either P.reject pure (mkRamWatch windows =<< maybe (Right singlePass) (uncurry mkPacing) pacing)

sdHostPacedWatch :: Maybe (Int,Int) -> IO RamWatch
sdHostPacedWatch pacing = either P.reject pure (sdHostWatch =<< maybe (Right singlePass) (uncurry mkPacing) pacing)

withOutput :: Operation -> Maybe FilePath -> (FilePath -> IO String) -> IO ()
withOutput operation requested action = do
  directory <- resolveOutput operation requested
  createOutput directory
  putStrLn ("output: "++directory)
  hFlush stdout
  action directory >>= putStrLn

resolveOutput :: Operation -> Maybe FilePath -> IO FilePath
resolveOutput _ (Just directory) = do
  unless (isAbsolute directory) (P.reject "OUTDIR must be an absolute path")
  pure directory
resolveOutput operation Nothing = do
  workspace <- lookupEnv "BUILD_WORKSPACE_DIRECTORY"
  root <- maybe (P.reject "no OUTDIR given and BUILD_WORKSPACE_DIRECTORY is unset") pure workspace
  unless (isAbsolute root) (P.reject "BUILD_WORKSPACE_DIRECTORY must be an absolute path")
  stamp <- formatTime defaultTimeLocale "%Y%m%dT%H%M%SZ" <$> getCurrentTime
  pure (root </> "private-data" </> "sessions" </> (stamp++"-"++operationName operation))

createOutput :: FilePath -> IO ()
createOutput directory = do
  exists <- doesPathExist directory
  when exists (P.reject ("output directory already exists: "++directory))
  createDirectoryIfMissing True (takeDirectory directory)
  createDirectory directory
  -- The new directory inherits a possibly permissive umask; private mode is
  -- established before any journal line is written.
  setFileMode directory 0o700
  ensurePrivateDirectory directory
  bracket (openFd (takeDirectory directory) ReadOnly defaultFileFlags {directory=True,cloexec=True}) closeFd fileSynchronise
