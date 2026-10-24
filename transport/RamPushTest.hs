module Main where

import Control.Exception (SomeException, try)
import Control.Monad (forM_)
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import Data.IORef
import Data.List (isInfixOf)
import System.Directory
import System.FilePath
import System.Posix.Files (fileMode, getFileStatus, intersectFileModes, setFileMode)
import System.Posix.Temp (mkdtemp)
import FujiAF.RamPush
import FujiAF.SHA256 (sha256)
import FujiAF.Rehearsal (rehearseRamAlloc, rehearseRamPush, rehearseRamFree, rehearseRamUnload)
import FujiAF.Transport
import RamPushModel

contract :: ReviewedContract
contract = ReviewedContract
  [ ("build-recipe-bank",replicate 64 'c'), ("menu/ram_push_installer.c",replicate 64 'd'), ("ram-push-toolchain",replicate 64 'e') ]
  (replicate 64 '0')

makeDir :: FilePath -> IO ()
makeDir path = createDirectory path >> setFileMode path 0o700

runAlloc :: FilePath -> Model -> IO RamAllocOutcome
runAlloc directory model = runRamAllocWith PeerCacheBound reviewedAllocationSize (Clock (pure 0)) (modelApi model) directory

runPush :: FilePath -> Model -> RamAllocState -> IO RamPushOutcome
runPush directory model state = runRamPushWith PeerCacheBound state manifest (Clock (pure 0)) (modelApi model) directory

main :: IO ()
main = do
  -- Concurrent Bazel runs share /dev/shm and every sandbox reports pid 12.
  root <- mkdtemp "/dev/shm/fuji-af-ram-push-tests-XXXXXX"
  setFileMode root 0o700

  check "bootstraps are exactly 32 bytes" (all ((==32) . B.length) [bootstrapBytes,peerPrepareBytes,freeBootstrapBytes])
  check "allocator preserves callback output pointer" (B.take 4 (B.drop 4 bootstrapBytes) == B.pack [0x01,0x50,0xa0,0xe1])
  check "peer preparation calls the worker unconditionally" (B.index peerPrepareBytes 19 == 0xeb)
  check "CLI accepts split flow" (ramAllocArguments ["0x1d9f8"] == Just (reviewedAllocationSize,Nothing)
    && ramAllocArguments ["0x1d9d8"] == Nothing
    && ramPushArguments ["/a","/m"] == Just ("/a","/m",Nothing)
    && ramPushArguments ["/a","/m","/o"] == Just ("/a","/m",Just "/o")
    && ramFreeArguments ["/a"] == Just ("/a",Nothing))
  check "CLI rejects flags, non-reviewed sizes and excess arguments" (ramAllocArguments ["0"] == Nothing
    && ramAllocArguments ["0x1a99f"] == Nothing && ramAllocArguments ["0x1a9a1"] == Nothing
    && ramPushArguments ["/m"] == Nothing && ramPushArguments ["--raw","/m"] == Nothing
    && ramPushArguments ["/a","/m","/o","x"] == Nothing)

  requireReviewedBytes (C.pack "manifest") (C.pack "runtime") (C.pack "manifest") (C.pack "runtime")
  changedRuntime <- try (requireReviewedBytes (C.pack "manifest") (C.pack "runtime") (C.pack "manifest") (C.pack "changed"))
    :: IO (Either SomeException ())
  check "reviewed build rejects byte mismatch" (case changedRuntime of Left _ -> True; Right _ -> False)
  unreviewed <- try (validateReviewedContract contract manifest) :: IO (Either SomeException ())
  check "compiled contract rejects an unreviewed manifest before USB" (case unreviewed of Left _ -> True; Right _ -> False)

  let allocateRetained label model = do
        let directory = root </> (label++"-alloc")
        makeDir directory
        outcome <- runAlloc directory model
        path <- case outcome of
          RamAllocated statePath -> pure statePath
          other -> error (label++" allocation failed: "++show other)
        readRetainedAllocation path
      allocate label model = retainedAllocation <$> allocateRetained label model

  untrustedModel <- newModel Nothing
  let untrustedDir = root </> "untrusted-manifest"
  makeDir untrustedDir
  untrustedOutcome <- runRamPush contract (RetainedAllocation (untrustedDir </> "ram-alloc-state.json") allocation) manifest (Clock (pure 0)) (modelApi untrustedModel) untrustedDir
  check "production ram-push rejects an unreviewed manifest" (case untrustedOutcome of RamPushRefused _ -> True; _ -> False)
  readIORef (modelHostWrites untrustedModel) >>= check "unreviewed manifest is refused before USB writes" . null

  let withIdentity identity = manifest { sourceIdentity = identity }
      substituteTool name = [(entry, if entry == name then replicate 64 'f' else digest) | (entry,digest) <- reviewedSourceIdentity contract]
      refusalOf label candidate = do
        model <- newModel Nothing
        let directory = root </> ("tool-identity-"++label)
        makeDir directory
        usbBefore <- readIORef (modelUsbCalls model)
        outcome <- runRamPush contract (RetainedAllocation (directory </> "ram-alloc-state.json") allocation) candidate (Clock (pure 0)) (modelApi model) directory
        usbAfter <- readIORef (modelUsbCalls model)
        check "tool identity refusals make no USB call" (usbAfter == usbBefore)
        pure (case outcome of RamPushRefused reason -> reason; other -> error ("expected a refusal: "++show other))
  reviewedIdentityRefusal <- refusalOf "reviewed" (withIdentity (reviewedSourceIdentity contract))
  check "reviewed tool identities pass the source identity check" (not ("source identity" `isInfixOf` reviewedIdentityRefusal))
  forM_ ["build-recipe-bank","ram-push-toolchain"] $ \tool -> do
    substitutedRefusal <- refusalOf (tool++"-substituted") (withIdentity (substituteTool tool))
    check (tool++" with a substituted identity is refused") ("source identity" `isInfixOf` substitutedRefusal)
    staleRefusal <- refusalOf (tool++"-missing") (withIdentity (filter ((/= tool) . fst) (reviewedSourceIdentity contract)))
    check (tool++" missing from a stale builder's manifest is refused") ("source identity" `isInfixOf` staleRefusal)

  noPeer <- newModel Nothing
  let noPeerDir = root </> "no-peer"
  makeDir noPeerDir
  noPeerOutcome <- runRamPushWith PeerCacheUnbound allocation manifest (Clock (pure 0)) (modelApi noPeer) noPeerDir
  check "unbound peer refuses before USB and writes" (case noPeerOutcome of RamPushRefused _ -> True; _ -> False)
  readIORef (modelHostWrites noPeer) >>= check "unbound peer has no writes" . null
  parkModel <- newModel Nothing
  parkAllocation <- allocate "park-timeout" parkModel
  writeIORef (modelInstallStatus parkModel) 0xfffffff9
  let parkDir = root </> "park-timeout"
  makeDir parkDir
  parkOutcome <- runPush parkDir parkModel parkAllocation
  check "CPU1 park timeout refuses install" (case parkOutcome of RamPushRefused _ -> True; _ -> False)
  readIORef (modelInternalWrites parkModel) >>= check "CPU1 park timeout precedes every publication write" . null
  let spinReportOutcome label spins = do
        model <- newModel Nothing
        retainedSpin <- allocate label model
        writeIORef (modelSpinReport model) spins
        let directory = root </> label
        makeDir directory
        runPush directory model retainedSpin
  constantBound <- spinReportOutcome "constant-bound" (0x10010000,0x10010000,0)
  check "install reporting a constant in place of the spin bound is uncertain" (case constantBound of RamPushUncertain _ -> True; _ -> False)
  exhaustedAck <- spinReportOutcome "exhausted-ack" (0x80000,0x80000,17)
  check "install whose ACK wait reached the spin bound is uncertain" (case exhaustedAck of RamPushUncertain _ -> True; _ -> False)
  exhaustedPark <- spinReportOutcome "exhausted-park" (0x80000,3,0x80000)
  check "install whose PARKED wait reached the spin bound is uncertain" (case exhaustedPark of RamPushUncertain _ -> True; _ -> False)

  rehearseRamAlloc reviewedAllocationSize
  rehearseRamPush allocation manifest

  mismatchModel <- newModel Nothing
  let mismatchDir = root </> "allocation-mismatch"
      mismatch = allocation { allocBase = Address (base+32) }
  makeDir mismatchDir
  mismatchOutcome <- runPush mismatchDir mismatchModel mismatch
  check "allocation base mismatch refuses" (case mismatchOutcome of RamPushRefused _ -> True; _ -> False)
  readIORef (modelHostWrites mismatchModel) >>= check "allocation mismatch has no writes" . null

  let refusal label mutate = do
        model <- newModel Nothing
        retained <- allocate label model
        writesBefore <- length <$> readIORef (modelHostWrites model)
        _ <- mutate model
        let directory = root </> label
        makeDir directory
        outcome <- runPush directory model retained
        check (label++" refuses") (case outcome of RamPushRefused _ -> True; _ -> False)
        writesAfter <- length <$> readIORef (modelHostWrites model)
        check (label++" has no push RAM write") (writesAfter == writesBefore)
  refusal "table-pointer" $ \model -> writeWord (modelMemory model) (unAddress callbackTablePointerAddress) 0
  refusal "slot" $ \model -> writeWord (modelMemory model) (unAddress callbackSlotAddress) 0
  refusal "old-word" $ \model -> writeWord (modelMemory model) (unAddress (publicationAddress (publications!!47))) 0
  refusal "allocation-guard" $ \model -> writeBytes (modelMemory model) base [1]
  refusal "identity" $ \model -> modifyIORef' (modelIdentity model) (\bytes -> take 2 bytes ++ [0] ++ drop 3 bytes)
  refusal "f7" $ \model -> modifyIORef' (modelConfig model) (\bytes -> take (36+0xf7) bytes ++ [0] ++ drop (37+0xf7) bytes)

  freeModel <- newModel Nothing
  retainedForFree <- allocateRetained "free" freeModel
  rehearseRamFree (retainedAllocation retainedForFree)
  let freeDir = root </> "free"
  makeDir freeDir
  freed <- runRamFree retainedForFree (Clock (pure 0)) (modelApi freeModel) freeDir
  check "happy free" (freed == RamFreed)
  readIORef (modelFreed freeModel) >>= check "ram-free calls stock free"
  let marker = consumedMarkerPath retainedForFree
  check "consumed marker sits beside the allocation state" (takeDirectory marker == takeDirectory (retainedStatePath retainedForFree))
  markerMode <- fileMode <$> getFileStatus marker
  check "consumed marker is mode 0600" (markerMode `intersectFileModes` 0o777 == 0o600)
  markerBytes <- B.readFile marker
  check "consumed marker records the freed base and allocation hash"
    (C.pack "0x00a80000" `B.isInfixOf` markerBytes && C.pack (allocGuard (retainedAllocation retainedForFree)) `B.isInfixOf` markerBytes)
  writesAfterFree <- length <$> readIORef (modelHostWrites freeModel)
  let tagGuardDir = root </> "second-free-tag-guard"
  makeDir tagGuardDir
  tagGuarded <- runRamFreeWith PeerCacheBound (retainedAllocation retainedForFree) (Clock (pure 0)) (modelApi freeModel) tagGuardDir
  check "camera-side freed tag refuses a second free" (case tagGuarded of RamFreeRefused _ -> True; _ -> False)
  readIORef (modelHostWrites freeModel) >>= check "tag-guard refusal sends no write" . (== writesAfterFree) . length
  writeBytes (modelMemory freeModel) base (replicate 8 0)
  writeIORef (modelFreed freeModel) False
  restored <- B.pack <$> readBytes (modelMemory freeModel) base reviewedAllocationSize
  check "camera-side tag is gone and the allocation hash is back to the state's" (sha256 restored == allocGuard (retainedAllocation retainedForFree))
  usbBeforeConsumed <- readIORef (modelUsbCalls freeModel)
  let secondFreeDir = root </> "second-free"
  makeDir secondFreeDir
  secondFree <- runRamFree retainedForFree (Clock (pure 0)) (modelApi freeModel) secondFreeDir
  check "second ram-free from a consumed state is refused with the camera tag gone" (case secondFree of RamFreeRefused _ -> True; _ -> False)
  readIORef (modelFreed freeModel) >>= check "refused second free does not free" . not
  let freedPushDir = root </> "push-after-free"
  makeDir freedPushDir
  freedPush <- runRamPush contract retainedForFree manifest (Clock (pure 0)) (modelApi freeModel) freedPushDir
  check "ram-push from a consumed state is refused with the camera tag gone" (case freedPush of RamPushRefused _ -> True; _ -> False)
  readIORef (modelUsbCalls freeModel) >>= check "consumed-state refusals make no USB call" . (== usbBeforeConsumed)

  happyModel <- newModel Nothing
  retained <- allocate "happy" happyModel
  let happyDir = root </> "happy"
  makeDir happyDir
  happy <- runPush happyDir happyModel retained
  statePath <- case happy of
    RamPushInstalled path -> pure path
    other -> error ("happy push failed: "++show other)
  state <- readRamPushState statePath
  check "installed state retains allocation extent" (stateSize state == allocSize retained)

  requireReviewedState state manifest
  let entries = stateEntries state
      tamperedEntry = state { stateEntries = entries { rollbackEntry = Address (unAddress (rollbackEntry entries)+4) } }
      tamperPublication publication
        | publicationIndex publication == 0 =
            publication { publicationNew = WordValue (unWordValue (publicationNew publication)+1) }
        | otherwise = publication
      tamperedPublication = state { statePublications = map tamperPublication (statePublications state) }
      tamperedHash = state { stateRuntimeDigest = replicate 64 '0' }
      rejectsState candidate = do
        outcome <- try (requireReviewedState candidate manifest) :: IO (Either SomeException ())
        pure (case outcome of Left _ -> True; Right _ -> False)
  rejectsState tamperedEntry >>= check "unload authentication rejects tampered entry"
  rejectsState tamperedPublication >>= check "unload authentication rejects tampered publication"
  rejectsState tamperedHash >>= check "unload authentication rejects tampered runtime hash"
  forM_ publications $ \publication -> do
    actual <- u32 0 <$> readBytes (modelMemory happyModel) (unAddress (publicationAddress publication)) 4
    check "installed publication" (actual == unWordValue (publicationNew publication))
  hostWrites <- readIORef (modelHostWrites happyModel)
  let allowed (address,bytes) =
        (unAddress bootstrapAddress <= address && address + fromIntegral (length bytes) <= unAddress bootstrapAddress + 32)
        || (address == unAddress callbackSlotAddress && length bytes == 4)
        || (address == unAddress queryOutputAddress + 4 && length bytes == 4)
        || (address == unAddress queryOutputAddress && length bytes == 8)
        || (base <= address && address + fromIntegral (length bytes) <= base + fromIntegral (allocationSize manifest))
  check "host writes stay in runtime, bootstrap protocol scratch or slot" (all allowed hostWrites)
  internal <- readIORef (modelInternalWrites happyModel)
  check "installer writes exactly the manifest destinations" (map fst internal == map (unAddress . publicationAddress) publications)
  rehearseRamUnload state

  let staleFreeDir = root </> "stale-free"
  makeDir staleFreeDir
  staleFree <- runRamFreeWith PeerCacheBound retained (Clock (pure 0)) (modelApi happyModel) staleFreeDir
  check ("ram-free refuses an allocation modified by install: "++show staleFree) (case staleFree of RamFreeRefused _ -> True; _ -> False)
  readIORef (modelFreed happyModel) >>= check "stale ram-free does not free" . not

  refusalUnloadModel <- newModel Nothing
  forM_ publications $ \publication ->
    writeWord (modelMemory refusalUnloadModel) (unAddress (publicationAddress publication)) (unWordValue (publicationNew publication))
  writeWord (modelMemory refusalUnloadModel) (unAddress (publicationAddress (publications!!31))) 0
  let refusalUnloadDir = root </> "unload-preflight"
  makeDir refusalUnloadDir
  refusedUnload <- runRamUnloadWith PeerCacheBound state (Clock (pure 0)) (modelApi refusalUnloadModel) refusalUnloadDir
  check "unload active-word mismatch refuses" (case refusedUnload of RamUnloadRefused _ -> True; _ -> False)
  readIORef (modelHostWrites refusalUnloadModel) >>= check "unload refusal has no RAM write" . null
  readIORef (modelInternalWrites refusalUnloadModel) >>= check "unload refusal has no target write" . null

  let unloadDir = root </> "unload"
  makeDir unloadDir
  unloaded <- runRamUnloadWith PeerCacheBound state (Clock (pure 0)) (modelApi happyModel) unloadDir
  check ("happy unload: "++show unloaded) (unloaded == RamUnloaded)
  readIORef (modelFreed happyModel) >>= check "unload retains allocation" . not
  forM_ publications $ \publication -> do
    actual <- u32 0 <$> readBytes (modelMemory happyModel) (unAddress (publicationAddress publication)) 4
    check "rolled-back publication" (actual == unWordValue (publicationOld publication))
  allInternal <- readIORef (modelInternalWrites happyModel)
  check "rollback is reverse order" (map fst (drop publicationCount allInternal) == reverse (map (unAddress . publicationAddress) publications))

  let callbackFailure label failureOffsets expected = do
        model <- newModel Nothing
        retainedState <- allocateRetained label model
        ordinal <- readIORef (modelWriteReplies model)
        writeIORef (modelWriteFailures model) (map (ordinal+) failureOffsets)
        let directory = root </> label
        makeDir directory
        outcome <- runRamFree retainedState (Clock (pure 0)) (modelApi model) directory
        message <- case outcome of
          RamFreeUncertain reason -> pure reason
          other -> error (label++" expected uncertain outcome: "++show other)
        check (label++" reports callback-slot state") (expected `isInfixOf` message)
        current <- u32 0 <$> readBytes (modelMemory model) (unAddress callbackSlotAddress) 4
        check (label++" cleanup writes stock callback") (current == unWordValue callbackStockWord)
        doesFileExist (consumedMarkerPath retainedState) >>= check (label++" uncertain free writes the consumed marker")
  callbackFailure "callback-restored" [2] "stock callback slot restored"
  callbackFailure "callback-restore-uncertain" [2,3] "do not use camera; power it off now"

  failedModel <- newModel (Just 13)
  failedAllocation <- allocate "mid-sequence" failedModel
  let failedDir = root </> "mid-sequence"
  makeDir failedDir
  failed <- runPush failedDir failedModel failedAllocation
  check "mid-sequence loss is uncertain" (case failed of RamPushUncertain _ -> True; _ -> False)
  journal <- readFile (failedDir </> "journal.v1")
  check "mid-sequence state is recorded" ("allocation-active=True" `isInfixOf` journal)
  sent <- readIORef (modelHostWrites failedModel)
  check "lost write is not retried" (length sent == 14)

  -- The RAM caps count one native-outcome line per completed transfer.
  happyJournal <- lines <$> readFile (happyDir </> "journal.v1")
  let kindCount kind = length (filter (isInfixOf (show kind)) happyJournal)
  check "every completed transfer journals its native outcome"
    (kindCount "native-outcome" > 0 && kindCount "native-outcome" == kindCount "reply-before-deadline-check")

  removeDirectoryRecursive root
  putStrLn "ram-push tests: ok"
