{-# LANGUAGE OverloadedStrings #-}
-- | Behavioral admission tests against the host user manager, without large allocations.
module Main (main) where

import Control.Concurrent (threadDelay)
import Control.Exception (IOException, try)
import Control.Monad (forM, forM_, unless)
import Data.Aeson (FromJSON (parseJSON), eitherDecodeStrict', withObject, (.:))
import qualified Data.ByteString as B
import Data.Char (isHexDigit)
import Data.IORef (modifyIORef, newIORef, readIORef)
import Data.List (isInfixOf, stripPrefix)
import Data.Maybe (fromMaybe, listToMaybe, mapMaybe)
import System.Directory (copyFile, createDirectoryIfMissing, doesDirectoryExist, doesFileExist, findExecutable, getTemporaryDirectory, listDirectory, makeAbsolute, removeFile)
import System.Environment (getArgs, getEnvironment, lookupEnv)
import System.Exit (ExitCode (..), exitFailure)
import System.FilePath ((</>), takeFileName)
import System.IO (Handle, hClose, hGetLine)
import System.Posix.Files (setFileMode)
import System.Posix.Process (getProcessID)
import System.Posix.Signals (nullSignal, sigKILL, signalProcess)
import System.Posix.Types (ProcessID)
import System.Posix.User (getEffectiveUserID)
import System.Process (CreateProcess (..), ProcessHandle, StdStream (CreatePipe), createProcess, getPid, getProcessExitCode, proc, readCreateProcessWithExitCode, waitForProcess)
import System.Timeout (timeout)
import Text.Read (readMaybe)

expect :: String -> Bool -> IO Bool
expect name ok = putStrLn ((if ok then "ok   " else "FAIL ") ++ name) >> pure ok

data Probe = Probe Handle Handle ProcessHandle
data Admission = Admission [String] (Maybe String)
  deriving (Show)
newtype ObservedInvocation = ObservedInvocation String
  deriving (Eq, Show)
newtype RunLabel = RunLabel String

instance FromJSON RunLabel where
  parseJSON = withObject "run" $ \object' -> RunLabel <$> object' .: "label"

main :: IO ()
main = do
  arguments <- getArgs
  case arguments of
    [source, builder, wrapper] -> do
      sourcePath <- makeAbsolute source
      builderPath <- makeAbsolute builder
      wrapperPath <- makeAbsolute wrapper
      run sourcePath builderPath wrapperPath >>= \ok -> unless ok exitFailure
    _ -> ioError (userError "usage: emulator-slot-tests PROBE HOST-BUILD EMULATOR-SLOT")

run :: FilePath -> FilePath -> FilePath -> IO Bool
run source builder wrapper = do
  temporary <- getTemporaryDirectory
  base <- fromMaybe temporary <$> lookupEnv "TEST_TMPDIR"
  pid <- getProcessID
  let root = base </> ("emulator-slot-tests-" ++ show pid)
      home = root </> "home"
      replacement = root </> "probe"
      bin = root </> "bin"
  createDirectoryIfMissing True home
  createDirectoryIfMissing True bin
  writeFile (bin </> "bazel") "#!/bin/sh\necho 'probe-in-slot build'\ncat >/dev/null\nexit 0\n"
  setFileMode (bin </> "bazel") 0o700
  inherited <- probeEnvironment home
  realSystemctl <- findExecutable "systemctl" >>= maybe (ioError (userError "systemctl is required")) pure
  let failedBin = root </> "failed-state-bin"
  createDirectoryIfMissing True failedBin
  writeFile (failedBin </> "systemctl") (unlines
    [ "#!/bin/sh"
    , "matched=0"
    , "for argument do [ \"$argument\" != \"$XM1_FAILED_UNIT\" ] || matched=1; done"
    , "if [ \"$matched\" = 0 ]; then exec \"$XM1_REAL_SYSTEMCTL\" \"$@\"; fi"
    , "\"$XM1_REAL_SYSTEMCTL\" \"$@\" | while IFS= read -r line; do"
    , "case \"$line\" in"
    , "ActiveState=active|ActiveState=activating|ActiveState=deactivating) printf 'ActiveState=failed\\n';;"
    , "*) printf '%s\\n' \"$line\";;"
    , "esac"
    , "done"
    ])
  setFileMode (failedBin </> "systemctl") 0o700
  let environment = change "PATH" (bin ++ ":" ++ fromMaybe "" (lookup "PATH" inherited)) inherited
      native = change "XM1_PROBE_CLASS" "native" environment
      spawn env = start (proc source []) {env = Just env}
      build = start (proc builder ["build", "//fake:target"]) {env = Just environment}
      rawScope unit = start (proc "systemd-run" ["--user", "--scope", "--quiet", "--collect", "--unit=" ++ unit, "--property=MemoryMax=4G", "--property=MemorySwapMax=0", "--", source]) {env = Just (change "XM1_PROBE_LEGACY" "1" environment)}
      failedState unit env =
        change "XM1_FAILED_UNIT" unit
          (change "XM1_REAL_SYSTEMCTL" realSystemctl
            (change "PATH" (failedBin ++ ":" ++ fromMaybe "" (lookup "PATH" env)) env))
  nativeCli <- start (proc wrapper ["--native", source])
    {env = Just (change "XM1_PROBE_EXIT" "37" native)}
  Admission nativeCliLines nativeCliSlot <- admission nativeCli
  release nativeCli
  nativeCliCode <- finish nativeCli
  nativeCliOk <- expect "the actual --native wrapper admits its command under 12 GiB RAM and 8 GiB swap caps and propagates exit"
    (nativeCliSlot /= Nothing && "probe-memory-max 12884901888" `elem` nativeCliLines
      && "probe-memory-swap-max 8589934592" `elem` nativeCliLines && nativeCliCode == Just (ExitFailure 37))
  defaultFull <- spawn environment
  Admission defaultFullLines defaultFullSlot <- admission defaultFull
  release defaultFull
  defaultFullCode <- finish defaultFull
  defaultFullOk <- expect "full-system defaults enforce 8 GiB RAM and 8 GiB swap"
    (defaultFullSlot /= Nothing && "probe-memory-max 8589934592" `elem` defaultFullLines
      && "probe-memory-swap-max 8589934592" `elem` defaultFullLines && defaultFullCode == Just ExitSuccess)
  overrideCaps <- forM
    [ (change "XM1_EMULATOR_SWAP_MAX" "4G" (change "XM1_EMULATOR_MEMORY_MAX" "24G" environment), "25769803776", "4294967296")
    , (change "XM1_NATIVE_SWAP_MAX" "16G" (change "XM1_NATIVE_MEMORY_MAX" "6G" native), "6442450944", "17179869184")
    ] $ \(overrides, ramBytes, swapBytes) -> do
      job <- spawn overrides
      Admission seen slot <- admission job
      release job
      code <- finish job
      pure (slot /= Nothing && ("probe-memory-max " ++ ramBytes) `elem` seen
        && ("probe-memory-swap-max " ++ swapBytes) `elem` seen && code == Just ExitSuccess)
  overrideCapsOk <- expect "full-system and native overrides enforce independent RAM and swap caps above 4 GiB" (and overrideCaps)
  full <- forM [1 :: Int .. 4] $ \_ -> do
    job <- spawn (change "XM1_EMULATOR_SWAP_MAX" "0" environment)
    seen <- admission job
    pure (job, seen)
  fullOk <- expect "four full-system scopes overlap under whole-tree 8 GiB RAM caps with explicit zero swap"
    (all (\(_, Admission seen slot) -> slot /= Nothing && "probe-memory-max 8589934592" `elem` seen
      && "probe-memory-swap-max 0" `elem` seen) full)
  copyFile source replacement
  setFileMode replacement 0o700
  fifth <- start (proc replacement []) {env = Just (change "XM1_EMULATOR_SWAP_MAX" "0" environment)}
  fifthWaiting <- waiting fifth
  removeFile replacement
  writeFile replacement "#!/bin/sh\necho replacement-ran\nexit 42\n"
  setFileMode replacement 0o700
  largeNative <- spawn (change "XM1_NATIVE_MEMORY_MAX" "24G" native)
  Admission largeNativeLines largeNativeSlot <- admission largeNative
  largeNativeWaiter <- spawn native
  largeNativeBlocked <- waiting largeNativeWaiter
  release largeNative
  Admission nativeDefaultLines nativeDefaultSlot <- admission largeNativeWaiter
  release largeNativeWaiter
  largeNativeCode <- finish largeNative
  nativeDefaultCode <- finish largeNativeWaiter
  largeNativeOk <- expect "explicit 24 GiB native RAM plus default 8 GiB swap consumes the remaining shared budget"
    (largeNativeSlot /= Nothing && "probe-memory-max 25769803776" `elem` largeNativeLines
      && "probe-memory-swap-max 8589934592" `elem` largeNativeLines && largeNativeBlocked
      && nativeDefaultSlot /= Nothing && "probe-memory-max 12884901888" `elem` nativeDefaultLines
      && "probe-memory-swap-max 8589934592" `elem` nativeDefaultLines
      && largeNativeCode == Just ExitSuccess && nativeDefaultCode == Just ExitSuccess)
  firstNative <- spawn native
  firstSeen <- admission firstNative
  secondNative <- spawn (change "XM1_NATIVE_SWAP_MAX" "0" (change "XM1_NATIVE_MEMORY_MAX" "4G" native))
  secondSeen <- admission secondNative
  overlap <- expect "two native suites overlap four full-system scopes while a full-system waiter waits"
    (admitted firstSeen && admitted secondSeen && fifthWaiting)
  thirdNative <- spawn native
  overBudget <- waiting thirdNative
  smallDuringWait <- spawn (change "XM1_NATIVE_SWAP_MAX" "0" (change "XM1_NATIVE_MEMORY_MAX" "1G" native))
  smallDuringWaitSeen <- admission smallDuringWait
  release firstNative
  thirdSeen <- admission thirdNative
  atomicOk <- expect "over-budget admission waits without retaining partial tokens"
    (overBudget && admitted smallDuringWaitSeen && admitted thirdSeen)
  case full of
    (firstFull, _) : _ -> release firstFull
    [] -> ioError (userError "no full-system probes were started")
  fifthSeen@(Admission fifthLines _) <- admission fifth
  pinnedOk <- expect "a saturated full-system waiter runs its pinned executable after replacement"
    (admitted fifthSeen && not (any (isInfixOf "replacement-ran") fifthLines))
  mapM_ release (map fst (drop 1 full) ++ [fifth, secondNative, thirdNative, smallDuringWait])
  codes <- mapM finish (map fst full ++ [fifth, firstNative, secondNative, thirdNative, smallDuringWait])
  exitsOk <- expect "all concurrent probes preserve successful exit" (all (== Just ExitSuccess) codes)

  buildA <- build
  buildSeenA <- admission buildA
  buildB <- build
  buildSeenB <- admission buildB
  fullWithBuild <- spawn environment
  buildFullSeen <- admission fullWithBuild
  nativeWithBuild <- spawn (change "XM1_NATIVE_SWAP_MAX" "0" (change "XM1_NATIVE_MEMORY_MAX" "4G" native))
  buildNativeSeen <- admission nativeWithBuild
  nativeWithBuild2 <- spawn (change "XM1_NATIVE_SWAP_MAX" "0" (change "XM1_NATIVE_MEMORY_MAX" "4G" native))
  buildNativeSeen2 <- admission nativeWithBuild2
  buildOverlap <- expect "two builds, a full-system scope and two explicit native scopes overlap at 64 GiB RAM plus swap"
    (all admitted [buildSeenA, buildSeenB, buildFullSeen, buildNativeSeen, buildNativeSeen2])
  buildWaiter <- spawn native
  blockedByBuild <- waiting buildWaiter
  release buildA
  resumedBuildWaiter <- admission buildWaiter
  buildBudget <- expect "build memory participates in the same weighted budget"
    (blockedByBuild && admitted resumedBuildWaiter)
  mapM_ release [buildB, fullWithBuild, nativeWithBuild, nativeWithBuild2, buildWaiter]
  buildCodes <- mapM finish [buildA, buildB, fullWithBuild, nativeWithBuild, nativeWithBuild2, buildWaiter]
  buildExit <- expect "admitted build and verification exits propagate" (all (== Just ExitSuccess) buildCodes)

  small <- spawn (change "XM1_NATIVE_MEMORY_MAX" "256M" native)
  Admission smallLines smallSlot <- admission small
  release small
  smallCode <- finish small
  smallOk <- expect "explicit small-native caps constrain the whole scope"
    (smallSlot /= Nothing && "probe-memory-max 268435456" `elem` smallLines && smallCode == Just ExitSuccess)
  nativeClass <- forM [1 :: Int .. 8] $ \_ -> do
    job <- spawn (change "XM1_NATIVE_SWAP_MAX" "0" (change "XM1_NATIVE_MEMORY_MAX" "256M" native))
    seen <- admission job
    pure (job, seen)
  ninthNative <- spawn (change "XM1_NATIVE_SWAP_MAX" "0" (change "XM1_NATIVE_MEMORY_MAX" "256M" native))
  ninthWaiting <- waiting ninthNative
  case nativeClass of
    (firstNativeClass, _) : _ -> release firstNativeClass
    [] -> ioError (userError "no native class probes were started")
  ninthSeen <- admission ninthNative
  nativeLimit <- expect "eight small native scopes overlap and a ninth waits for its class"
    (all (admitted . snd) nativeClass && ninthWaiting && admitted ninthSeen)
  mapM_ release (map fst (drop 1 nativeClass) ++ [ninthNative])
  mapM_ finish (map fst nativeClass ++ [ninthNative])
  nested <- spawn (change "XM1_PROBE_NESTED" "1" native)
  Admission nestedLines nestedSlot <- admission nested
  release nested
  _ <- finish nested
  nestedOk <- expect "a child cannot reuse its parent's admission marker"
    (nestedSlot /= Nothing && "probe-nested-exit ExitFailure 1" `elem` nestedLines)
  grantChild <- spawn (change "XM1_PROBE_GRANT" "valid" environment)
  grantChildSeen@(Admission grantChildLines grantChildSlot) <- admission grantChild
  release grantChild
  _ <- finish grantChild
  grantOk <- expect "a child with its parent's grant runs in the parent's slot and cgroup"
    (grantChildSlot /= Nothing && "probe-grant-exit ExitSuccess True" `elem` grantChildLines
      && observedText "probe-in-slot " grantChildSeen /= Nothing
      && observedText "probe-grant-slot " grantChildSeen == observedText "probe-in-slot " grantChildSeen
      && observedText "probe-grant-unit " grantChildSeen == observedText "probe-unit " grantChildSeen
      && observedText "probe-grant-memory-max " grantChildSeen == observedText "probe-memory-max " grantChildSeen)
  grantSecond <- spawn (change "XM1_PROBE_GRANT" "second" environment)
  Admission grantSecondLines grantSecondSlot <- admission grantSecond
  release grantSecond
  _ <- finish grantSecond
  grantSecondOk <- expect "a second granted child is refused while one runs in the parent's slot"
    (grantSecondSlot /= Nothing && "probe-second-first True" `elem` grantSecondLines
      && "probe-second-exit ExitFailure 1 False" `elem` grantSecondLines
      && "probe-second-error True" `elem` grantSecondLines)
  grantWrongSlot <- spawn (change "XM1_PROBE_GRANT" "wrong-slot" environment)
  Admission grantWrongSlotLines grantWrongSlotSlot <- admission grantWrongSlot
  release grantWrongSlot
  _ <- finish grantWrongSlot
  grantWrongSlotOk <- expect "a grant naming a slot the owner does not hold is refused"
    (grantWrongSlotSlot /= Nothing && "probe-wrong-slot-exit ExitFailure 1 False" `elem` grantWrongSlotLines
      && "probe-wrong-slot-error True" `elem` grantWrongSlotLines)
  grantMoved <- spawn (change "XM1_PROBE_GRANT" "moved" environment)
  Admission grantMovedLines grantMovedSlot <- admission grantMoved
  release grantMoved
  _ <- finish grantMoved
  grantMovedOk <- expect "a granted child moved to another cgroup is refused"
    (grantMovedSlot /= Nothing && "probe-moved-exit ExitFailure 1 False" `elem` grantMovedLines
      && "probe-moved-error True" `elem` grantMovedLines)
  grantNoPurpose <- spawn (change "XM1_PROBE_GRANT" "no-purpose" environment)
  Admission grantNoPurposeLines grantNoPurposeSlot <- admission grantNoPurpose
  release grantNoPurpose
  _ <- finish grantNoPurpose
  grantNoPurposeOk <- expect "a grant without a purpose is refused"
    (grantNoPurposeSlot /= Nothing && "probe-no-purpose-exit ExitFailure 1 False" `elem` grantNoPurposeLines
      && "probe-no-purpose-error True" `elem` grantNoPurposeLines)
  grantRegrant <- spawn (change "XM1_PROBE_GRANT" "regrant" environment)
  Admission grantRegrantLines grantRegrantSlot <- admission grantRegrant
  release grantRegrant
  _ <- finish grantRegrant
  grantRegrantOk <- expect "a granted child cannot grant its own child"
    (grantRegrantSlot /= Nothing && "probe-grant-exit ExitSuccess True" `elem` grantRegrantLines
      && "probe-regrant-line True" `elem` grantRegrantLines)
  grantOther <- spawn (change "XM1_PROBE_GRANT" "other" environment)
  Admission grantOtherLines grantOtherSlot <- admission grantOther
  release grantOther
  _ <- finish grantOther
  grantOtherOk <- expect "a grant naming another process is refused"
    (grantOtherSlot /= Nothing && "probe-grant-exit ExitFailure 1 False" `elem` grantOtherLines
      && "probe-grant-error True" `elem` grantOtherLines)
  grandchild <- spawn (change "XM1_PROBE_GRANT" "grandchild" environment)
  Admission grandchildLines grandchildSlot <- admission grandchild
  release grandchild
  _ <- finish grandchild
  grandchildOk <- expect "a grandchild cannot use its parent's grant"
    (grandchildSlot /= Nothing && "probe-grant-exit ExitSuccess True" `elem` grandchildLines
      && "probe-grandchild-line True" `elem` grandchildLines)
  (nestedBuildCode, _, _) <- readCreateProcessWithExitCode
    (proc builder ["build"]) {env = Just (change "XM1_EMULATOR_SLOT" "1" environment)} ""
  nestedBuildOk <- expect "build admission refuses to wait inside verification" (nestedBuildCode == ExitFailure 2)
  failed <- spawn (change "XM1_PROBE_EXIT" "37" native)
  failedSeen <- admission failed
  release failed
  failedCode <- finish failed
  exitOk <- expect "verification propagates the command's exit status" (admitted failedSeen && failedCode == Just (ExitFailure 37))

  crashed <- spawn environment
  crashSeen <- admission crashed
  let owner = observedPid "probe-pid " crashSeen
  mapM_ (signalProcess sigKILL) owner
  _ <- finish crashed
  afterCrash <- spawn environment
  crashRecovery <- admission afterCrash
  release afterCrash
  _ <- finish afterCrash
  crashOk <- expect "a killed lock owner releases admission for the next scope" (owner /= Nothing && admitted crashRecovery)
  survivorJob <- spawn (change "XM1_PROBE_SURVIVOR" "1" environment)
  survivorSeen <- admission survivorJob
  release survivorJob
  _ <- finish survivorJob
  afterSurvivor <- spawn (failedState (fromMaybe "missing.scope" (observedText "probe-unit " survivorSeen)) native)
  recoveredSurvivor <- admission afterSurvivor
  gone <- maybe (pure False) (waitGone 50) (observedPid "probe-survivor " survivorSeen)
  release afterSurvivor
  _ <- finish afterSurvivor
  survivorOk <- expect "a failed-state scope retains its memory until its surviving descendants are reaped" (admitted recoveredSurvivor && gone)
  originalOwner <- spawn environment
  originalSeen <- admission originalOwner
  let reusedUnit = fromMaybe "missing.scope" (observedText "probe-unit " originalSeen)
  originalInvocation <- invocationOf environment reusedUnit
  release originalOwner
  _ <- finish originalOwner
  waitUnitEnded environment 50 reusedUnit
  unrelatedOwner <- rawScope reusedUnit
  unrelatedSeen <- admission unrelatedOwner
  unrelatedInvocation <- invocationOf environment reusedUnit
  nativeAfterReuse <- spawn native
  nativeAfterReuseSeen <- admission nativeAfterReuse
  fullAfterReuse <- spawn environment
  fullAfterReuseSeen <- admission fullAfterReuse
  unrelatedAlive <- processAlive unrelatedSeen
  reusedManagedOk <- expect "managed memory and class records never stop a reused unit's different invocation"
    (admitted originalSeen && admitted unrelatedSeen && originalInvocation /= unrelatedInvocation && admitted nativeAfterReuseSeen && admitted fullAfterReuseSeen && unrelatedAlive)
  unless reusedManagedOk $ putStrLn ("managed reuse observations: " ++
    show (originalInvocation, unrelatedInvocation, originalSeen, unrelatedSeen, nativeAfterReuseSeen, fullAfterReuseSeen, unrelatedAlive))
  mapM_ release [unrelatedOwner, nativeAfterReuse, fullAfterReuse]
  mapM_ finish [unrelatedOwner, nativeAfterReuse, fullAfterReuse]

  let legacyUnit = "xm1-test-legacy-" ++ show pid ++ ".scope"
  legacy <- rawScope legacyUnit
  legacySeen <- admission legacy
  originalLegacyInvocation <- invocationOf environment legacyUnit
  (_, _, Just ready, reservation) <- createProcess (proc builder ["--reserve-legacy", legacyUnit]) {env = Just environment, std_err = CreatePipe}
  reserved <- awaitLegacyReady ready
  getPid reservation >>= mapM_ (signalProcess sigKILL)
  reservationCode <- timeout 10_000_000 (waitForProcess reservation)
  let secondLegacyUnit = "xm1-test-legacy-second-" ++ show pid ++ ".scope"
  secondLegacy <- rawScope secondLegacyUnit
  secondLegacySeen <- admission secondLegacy
  (_, _, Just secondReady, secondReservation) <- createProcess
    (proc builder ["--reserve-legacy", secondLegacyUnit]) {env = Just environment, std_err = CreatePipe}
  secondReserved <- awaitLegacyReady secondReady
  getPid secondReservation >>= mapM_ (signalProcess sigKILL)
  secondReservationCode <- timeout 10_000_000 (waitForProcess secondReservation)
  legacyBuildA <- build
  legacyBuildSeenA <- admission legacyBuildA
  legacyBuildB <- build
  legacyBuildSeenB <- admission legacyBuildB
  legacyWaiter <- spawn (failedState legacyUnit (change "XM1_NATIVE_SWAP_MAX" "0" (change "XM1_NATIVE_MEMORY_MAX" "17G" native)))
  legacyBlocked <- waiting legacyWaiter
  legacyAlive <- case observedPid "probe-pid " legacySeen of
    Nothing -> pure False
    Just legacyOwner -> either (const False) (const True) <$> (try (signalProcess nullSignal legacyOwner) :: IO (Either IOException ()))
  secondLegacyAlive <- processAlive secondLegacySeen
  release legacy
  _ <- finish legacy
  legacyResumed <- admission legacyWaiter
  legacyNativeA <- spawn (change "XM1_NATIVE_SWAP_MAX" "0" (change "XM1_NATIVE_MEMORY_MAX" "3G" native))
  legacyNativeSeenA <- admission legacyNativeA
  legacyNativeB <- spawn (failedState secondLegacyUnit (change "XM1_NATIVE_SWAP_MAX" "0" (change "XM1_NATIVE_MEMORY_MAX" "4G" native)))
  secondLegacyBlocked <- waiting legacyNativeB
  secondLegacyStillAlive <- processAlive secondLegacySeen
  release secondLegacy
  _ <- finish secondLegacy
  legacyNativeSeenB <- admission legacyNativeB
  legacyOk <- expect "two legacy scopes retain separate 4 GiB reservations after helper death and release independently without killing either scope"
    (admitted legacySeen && admitted secondLegacySeen && reserved && secondReserved
      && all admitted [legacyBuildSeenA, legacyBuildSeenB, legacyNativeSeenA, legacyNativeSeenB]
      && legacyBlocked && secondLegacyBlocked && legacyAlive && secondLegacyAlive && secondLegacyStillAlive
      && reservationCode /= Nothing && reservationCode /= Just ExitSuccess
      && secondReservationCode /= Nothing && secondReservationCode /= Just ExitSuccess && admitted legacyResumed)
  mapM_ release [legacyBuildA, legacyBuildB, legacyNativeA, legacyNativeB, legacyWaiter]
  mapM_ finish [legacyBuildA, legacyBuildB, legacyNativeA, legacyNativeB, legacyWaiter]
  waitUnitEnded environment 50 legacyUnit
  reusedLegacy <- rawScope legacyUnit
  reusedLegacySeen <- admission reusedLegacy
  reusedLegacyInvocation <- invocationOf environment legacyUnit
  afterLegacyReuse <- spawn native
  afterLegacyReuseSeen <- admission afterLegacyReuse
  reusedLegacyAlive <- processAlive reusedLegacySeen
  reusedLegacyOk <- expect "expired legacy protection never stops a different invocation reusing its unit name"
    (admitted reusedLegacySeen && originalLegacyInvocation /= reusedLegacyInvocation && admitted afterLegacyReuseSeen && reusedLegacyAlive)
  unless reusedLegacyOk $ putStrLn ("legacy reuse observations: " ++
    show (originalLegacyInvocation, reusedLegacyInvocation, reusedLegacySeen, afterLegacyReuseSeen, reusedLegacyAlive))
  mapM_ release [reusedLegacy, afterLegacyReuse]
  mapM_ finish [reusedLegacy, afterLegacyReuse]
  (invalidLegacy, _, _) <- readCreateProcessWithExitCode (proc builder ["--reserve-legacy", "missing.scope"]) {env = Just environment} ""
  invalidLegacyOk <- expect "legacy reservation refuses a missing or unvalidated scope" (invalidLegacy /= ExitSuccess)
  (lowStorageCode, lowStorageOutput, _) <- readCreateProcessWithExitCode
    (proc source []) {env = Just (change "XM1_STORAGE_MIN_FREE" "999999999999999999999T" environment)} ""
  storageOk <- expect "insufficient disk reserve prevents admission"
    (lowStorageCode /= ExitSuccess && not ("probe-in-slot " `isInfixOf` lowStorageOutput))
  invalidCaps <- forM
    [ change "XM1_NATIVE_MEMORY_MAX" "33G" native
    , change "XM1_NATIVE_MEMORY_MAX" "0" native
    , change "XM1_NATIVE_SWAP_MAX" "33G" native
    , change "XM1_NATIVE_SWAP_MAX" "-1" native
    , change "XM1_EMULATOR_MEMORY_MAX" "33G" environment
    , change "XM1_EMULATOR_SWAP_MAX" "33G" environment
    ] $ \invalidEnvironment -> do
      (capCode, capOutput, _) <- readCreateProcessWithExitCode
        (proc source []) {env = Just invalidEnvironment} ""
      pure (capCode /= ExitSuccess && not ("probe-in-slot " `isInfixOf` capOutput))
  capOk <- expect "invalid RAM and swap caps are refused before admission" (and invalidCaps)
  let missingRuntime = root </> "no-session"
      sessionless = change "XDG_RUNTIME_DIR" missingRuntime
        (change "DBUS_SESSION_BUS_ADDRESS" ("unix:path=" ++ missingRuntime ++ "/bus") environment)
  (sessionCode, sessionOutput, sessionError) <- readCreateProcessWithExitCode
    (proc source []) {env = Just sessionless} ""
  sessionOk <- expect "a missing user manager cannot silently bypass memory enforcement" (sessionCode /= ExitSuccess)
  unless sessionOk $ putStrLn ("sessionless observations: " ++ show (sessionCode, sessionOutput, sessionError))
  let failedRunner = root </> "failed-runner"
  createDirectoryIfMissing True failedRunner
  writeFile (failedRunner </> "systemd-run") "#!/bin/sh\nexit 43\n"
  setFileMode (failedRunner </> "systemd-run") 0o700
  (startupCode, _, _) <- readCreateProcessWithExitCode
    (proc source []) {env = Just (change "PATH" (failedRunner ++ ":" ++ fromMaybe "" (lookup "PATH" environment)) environment)} ""
  afterStartup <- spawn native
  afterStartupSeen <- admission afterStartup
  release afterStartup
  _ <- finish afterStartup
  startupOk <- expect "failed scope startup propagates exit without leaking resource locks"
    (startupCode == ExitFailure 43 && admitted afterStartupSeen)
  heldMigration <- build
  heldMigrationSeen <- admission heldMigration
  (heldMigrationCode, _, _) <- readCreateProcessWithExitCode
    (proc builder ["--migrate-build-records"]) {env = Just environment} ""
  heldMigrationStatus <- case heldMigration of Probe _ _ process -> getProcessExitCode process
  release heldMigration
  _ <- finish heldMigration
  migrationLockOk <- expect "legacy build metadata migration refuses a held build lane without disturbing its owner"
    (admitted heldMigrationSeen && heldMigrationCode /= ExitSuccess && heldMigrationStatus == Nothing)
  let migrationUnit = "xm1-host-build-fixture-" ++ show pid ++ ".scope"
      oldRecord = home </> ".cache" </> "xm1-host-build" </> "scope"
  migrationScope <- rawScope migrationUnit
  migrationSeen <- admission migrationScope
  writeFile oldRecord (migrationUnit ++ "\n")
  (liveMigrationCode, _, _) <- readCreateProcessWithExitCode
    (proc builder ["--migrate-build-records"]) {env = Just environment} ""
  migrationAlive <- processAlive migrationSeen
  release migrationScope
  _ <- finish migrationScope
  waitUnitEnded environment 50 migrationUnit
  (completedMigrationCode, _, _) <- readCreateProcessWithExitCode
    (proc builder ["--migrate-build-records"]) {env = Just environment} ""
  (repeatedMigrationCode, _, _) <- readCreateProcessWithExitCode
    (proc builder ["--migrate-build-records"]) {env = Just environment} ""
  reusedMigration <- rawScope migrationUnit
  reusedMigrationSeen <- admission reusedMigration
  afterMigration <- build
  afterMigrationSeen <- admission afterMigration
  reusedMigrationAlive <- processAlive reusedMigrationSeen
  release afterMigration
  afterMigrationCode <- finish afterMigration
  release reusedMigration
  _ <- finish reusedMigration
  migrationOk <- expect "verified empty legacy metadata migrates idempotently and never signals a reused unit"
    (admitted migrationSeen && liveMigrationCode /= ExitSuccess && migrationAlive
      && completedMigrationCode == ExitSuccess && repeatedMigrationCode == ExitSuccess
      && admitted afterMigrationSeen && afterMigrationCode == Just ExitSuccess && reusedMigrationAlive)
  writeFile oldRecord "unknown-format\n"
  (unknownMigrationCode, _, _) <- readCreateProcessWithExitCode
    (proc builder ["--migrate-build-records"]) {env = Just environment} ""
  unknownMigrationOk <- expect "unknown legacy metadata is refused rather than cleared" (unknownMigrationCode /= ExitSuccess)
  let labelWorkspace = root </> "run-label"
      explicitWorkspace = root </> "run-label-explicit"
  forM_ [labelWorkspace, explicitWorkspace] $ \directory -> do
    createDirectoryIfMissing True directory
    createDirectoryIfMissing True (directory </> "private-data")
    (code, _, errors) <- readCreateProcessWithExitCode (proc "git" ["init", "-q", "-b", "main"]) {cwd = Just directory} ""
    unless (code == ExitSuccess) (ioError (userError ("git init: " ++ errors)))
  labelJob <- startIn source labelWorkspace environment
  _ <- admission labelJob
  release labelJob
  _ <- finish labelJob
  programLabel <- readRunLabel labelWorkspace
  programLabelOk <- expect "a slot run is labelled with the original program name" (programLabel == Just (takeFileName source))
  explicitJob <- startIn source explicitWorkspace (change "XM1_RUN_LABEL" "explicit-label" environment)
  _ <- admission explicitJob
  release explicitJob
  _ <- finish explicitJob
  explicitLabel <- readRunLabel explicitWorkspace
  explicitLabelOk <- expect "XM1_RUN_LABEL overrides the slot's program label" (explicitLabel == Just "explicit-label")
  pure (and [nativeCliOk, defaultFullOk, overrideCapsOk, fullOk, largeNativeOk, overlap, atomicOk, pinnedOk, exitsOk, buildOverlap, buildBudget, buildExit, smallOk, nativeLimit, nestedOk, grantOk, grantSecondOk, grantWrongSlotOk, grantMovedOk, grantNoPurposeOk, grantRegrantOk, grantOtherOk, grandchildOk, nestedBuildOk, exitOk, crashOk, survivorOk, reusedManagedOk, legacyOk, reusedLegacyOk, invalidLegacyOk, storageOk, capOk, sessionOk, startupOk, migrationLockOk, migrationOk, unknownMigrationOk, programLabelOk, explicitLabelOk])

change :: String -> String -> [(String, String)] -> [(String, String)]
change key value environment = (key, value) : filter ((/= key) . fst) environment

admitted :: Admission -> Bool
admitted (Admission _ slot) = slot /= Nothing

observedPid :: String -> Admission -> Maybe ProcessID
observedPid prefix (Admission seen _) = case mapMaybe (\line -> stripPrefix prefix line >>= readMaybe) seen of
  pid : _ -> Just pid
  [] -> Nothing

observedText :: String -> Admission -> Maybe String
observedText prefix (Admission seen _) = case mapMaybe (stripPrefix prefix) seen of
  value : _ -> Just value
  [] -> Nothing

processAlive :: Admission -> IO Bool
processAlive seen = case observedPid "probe-pid " seen of
  Nothing -> pure False
  Just owner -> either (const False) (const True) <$> (try (signalProcess nullSignal owner) :: IO (Either IOException ()))

awaitLegacyReady :: Handle -> IO Bool
awaitLegacyReady stream = fromMaybe False <$> timeout 10_000_000 go
  where
    go = do
      line <- try (hGetLine stream) :: IO (Either IOException String)
      case line of
        Left _ -> pure False
        Right text | "legacy reservation ready:" `isInfixOf` text -> pure True
        Right _ -> go

invocationOf :: [(String, String)] -> String -> IO ObservedInvocation
invocationOf environment unit = do
  (code, out, err) <- readCreateProcessWithExitCode
    (proc "systemctl" ["--user", "show", unit, "-p", "InvocationID", "--value"]) {env = Just environment} ""
  let identity = unwords (words out)
  if code == ExitSuccess && length identity == 32 && all isHexDigit identity
    then pure (ObservedInvocation identity)
    else ioError (userError ("cannot observe invocation of " ++ unit ++ ": " ++ show (code, out, err)))

waitUnitEnded :: [(String, String)] -> Int -> String -> IO ()
waitUnitEnded environment attempts unit = do
  (code, out, err) <- readCreateProcessWithExitCode
    (proc "systemctl" ["--user", "show", unit, "-p", "ActiveState", "--value"]) {env = Just environment} ""
  case unwords (words out) of
    state | state `elem` ["inactive", "failed"] -> pure ()
    state | state `elem` ["active", "activating", "deactivating"], attempts > 0 ->
      threadDelay 100_000 >> waitUnitEnded environment (attempts - 1) unit
    _ -> ioError (userError ("cannot observe completed scope " ++ unit ++ ": " ++ show (code, out, err)))

waitGone :: Int -> ProcessID -> IO Bool
waitGone attempts pid = do
  status <- try (signalProcess nullSignal pid) :: IO (Either IOException ())
  case status of
    Left _ -> pure True
    Right () | attempts > 0 -> threadDelay 100_000 >> waitGone (attempts - 1) pid
    _ -> pure False

admission :: Probe -> IO Admission
admission (Probe _ out _) = do
  seen <- newIORef []
  slot <- timeout 15_000_000 (go seen)
  Admission <$> (reverse <$> readIORef seen) <*> pure (fromMaybe Nothing slot)
  where
    go seen = do
      line <- try (hGetLine out) :: IO (Either IOException String)
      case line of
        Left _ -> pure Nothing
        Right text -> do
          modifyIORef seen (text :)
          maybe (go seen) (pure . Just) (stripPrefix "probe-in-slot " text)

waiting :: Probe -> IO Bool
waiting job@(Probe _ _ process) = do
  result <- timeout 1_500_000 (admission job)
  status <- getProcessExitCode process
  pure (case result of Nothing -> status == Nothing; Just _ -> False)

release :: Probe -> IO ()
release (Probe input _ _) = hClose input

finish :: Probe -> IO (Maybe ExitCode)
finish (Probe _ _ process) = timeout 15_000_000 (waitForProcess process)

startIn :: FilePath -> FilePath -> [(String, String)] -> IO Probe
startIn program directory environment = start (proc program []) {cwd = Just directory, env = Just environment}

readRunLabel :: FilePath -> IO (Maybe String)
readRunLabel workspace = do
  let runs = workspace </> "private-data" </> "scratch" </> "main" </> "runs"
  present <- doesDirectoryExist runs
  if not present
    then pure Nothing
    else do
      names <- listDirectory runs
      labels <- forM names $ \name -> do
        let record = runs </> name </> "run.json"
        exists <- doesFileExist record
        if not exists
          then pure Nothing
          else do
            bytes <- B.readFile record
            pure (case eitherDecodeStrict' bytes of
              Right (RunLabel label) -> Just label
              Left _ -> Nothing)
      pure (listToMaybe [label | Just label <- labels])

start :: CreateProcess -> IO Probe
start command = do
  started <- createProcess command {std_in = CreatePipe, std_out = CreatePipe}
  case started of
    (Just input, Just out, _, process) -> pure (Probe input out process)
    _ -> ioError (userError "probe pipes did not open")


probeEnvironment :: FilePath -> IO [(String, String)]
probeEnvironment home = do
  inherited <- getEnvironment
  uid <- getEffectiveUserID
  let runtime = "/run/user/" ++ show uid
      keep key value environment = if any ((== key) . fst) environment then environment else (key, value) : environment
      dropped = ["HOME", "XM1_EMULATOR_SLOT", "XM1_EMULATOR_MEMORY_MAX", "XM1_EMULATOR_SWAP_MAX", "XM1_NATIVE_MEMORY_MAX", "XM1_NATIVE_SWAP_MAX", "XM1_HOST_BUILD_MEMORY_MAX", "XM1_HOST_BUILD_SWAP_MAX", "XM1_NATIVE_SLOTS", "XM1_HOST_BUILD", "XM1_ADMISSION_SCOPE", "XM1_ADMISSION_OWNER", "XM1_ADMISSION_CLASS", "XM1_ADMISSION_INHERIT", "XM1_STORAGE_MIN_FREE", "XM1_PROBE_CLASS", "XM1_PROBE_LEGACY", "XM1_PROBE_NESTED", "XM1_PROBE_GRANT", "XM1_PROBE_SURVIVOR", "XM1_PROBE_EXIT", "XM1_PROBE_ALLOCATE_BYTES", "XM1_RUN_DIR", "XM1_RUN_WORK", "XM1_RUN_RESULT", "XM1_RUN_PROGRAM", "XM1_RUN_LABEL", "XM1_TASK_OUTPUT_MAX", "XM1_TASK_MEASURE_SECONDS"]
  pure (("HOME", home) : ("XM1_STORAGE_MIN_FREE", "1") : filter ((`notElem` dropped) . fst)
    (keep "XDG_RUNTIME_DIR" runtime (keep "DBUS_SESSION_BUS_ADDRESS" ("unix:path=" ++ runtime ++ "/bus") inherited)))
