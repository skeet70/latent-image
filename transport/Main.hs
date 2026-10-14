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
import FujiAF.Routes
import FujiAF.PlanDocument (readPlanFile, readRoutePlanFile)
import FujiAF.Native (nativeApi, nativeApiJournaling, OutcomeJournal(..))
import FujiAF.Rehearsal
import qualified FujiAF.Broker as P

data RouteSource = RoutesFixed | RoutesDerived FilePath
data Operation = Identify | ReadConfig FilePath | EndSetting | ProbeRam | ReadLog | ProbeBootRom | ProbeArea2 | ProbeStraps | ProbeCode | ProbeOwnership | ProbeRoutes RouteSource | DumpRam DumpSegment | ReadMedia | ArmAutoAct DeviceStart
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
  , "rehearse identify|read-config PLAN_JSON|end-setting|probe-ram|read-log|probe-bootrom|probe-area2|probe-straps|probe-code|probe-ownership|probe-routes fixed|probe-routes derived PLAN_JSON|dump-ram SEGMENT|read-media|arm-auto-act"
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
dispatch (Live operation requested) = case operation of
  Identify -> withOutput operation requested $ \directory -> do
    ProductModel model <- runIdentify realClock (nativeApi identifyJournalCapBytes directory) directory
    pure ("identify: "++model)
  ReadConfig path -> do
    -- The plan is parsed and validated before the output directory exists.
    plan <- readPlanFile path
    withOutput operation requested $ \directory -> do
      let cap = journalCapFor (length (planWindows plan))
      windows <- runRead realClock (nativeApi cap directory) plan directory
      pure ("read-config: "++show windows++" windows")
  EndSetting -> withOutput operation requested $ \directory -> do
    outcome <- runEndSetting realClock (nativeApi endJournalCapBytes directory) directory
    pure ("end-setting: "++outcomeName outcome)
  ProbeRam -> withOutput operation requested $ \directory -> do
    probes <- runProbe realClock (nativeApi probeJournalCapBytes directory) directory
    pure ("probe-ram: "++show probes++" windows")
  ReadLog -> withOutput operation requested $ \directory -> do
    windows <- runReadLog realClock (nativeApi logJournalCapBytes directory) directory
    pure ("read-log: "++show windows++" windows")
  ProbeBootRom -> withOutput operation requested $ \directory -> do
    windows <- runProbeBootRom realClock (nativeApi bootRomJournalCapBytes directory) directory
    pure ("probe-bootrom: "++show windows++" windows")
  ProbeArea2 -> withOutput operation requested $ \directory -> do
    windows <- runProbeArea2 realClock (nativeApi area2JournalCapBytes directory) directory
    pure ("probe-area2: "++show windows++" windows")
  ProbeStraps -> withOutput operation requested $ \directory -> do
    bytes <- runProbeStraps realClock (nativeApi strapJournalCapBytes directory) directory
    pure ("probe-straps: "++show bytes++" bytes")
  ProbeCode -> withOutput operation requested $ \directory -> do
    windows <- runProbeCode realClock (nativeApi codeJournalCapBytes directory) directory
    pure ("probe-code: "++show windows++" windows")
  ProbeOwnership -> withOutput operation requested $ \directory -> do
    windows <- runProbeOwnership realClock (nativeApi ownershipJournalCapBytes directory) directory
    pure ("probe-ownership: "++show windows++" windows")
  ProbeRoutes source -> do
    -- The derived plan is parsed and validated before the output directory exists.
    windows <- case source of
      RoutesFixed -> pure routeFixedWindows
      RoutesDerived path -> readRoutePlanFile path
    withOutput operation requested $ \directory -> do
      requests <- runProbeRoutes realClock (nativeApi (journalCapFor (length windows)) directory) directory windows
      pure ("probe-routes: "++show requests++" read requests")
  DumpRam segment -> withOutput operation requested $ \directory -> do
    requests <- runDumpRam segment realClock (nativeApiJournaling FailingOutcomes (dumpJournalCapFor (dumpRequestCount segment)) directory) directory
    pure ("dump-ram "++segmentName segment++": "++show requests++" requests")
  ReadMedia -> withOutput operation requested $ \directory -> do
    bytes <- runReadMedia realClock (nativeApi mediaJournalCapBytes directory) directory
    pure ("read-media: "++show bytes++" bytes")
  ArmAutoAct mode -> withOutput operation requested $ \directory -> do
    report <- runArmAutoAct mode realClock (nativeApi armAutoActJournalCapBytes directory) directory
    pure ("arm-auto-act: "++armReportLine report)

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
