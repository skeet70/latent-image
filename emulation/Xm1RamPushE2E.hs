{-# LANGUAGE OverloadedStrings #-}
module Xm1RamPushE2E (RelinkMode (..), AllowlistPolicy (..), OverrideReason (..), BootMode (..), PushMode (..), E2ESettings (..), runRamPushE2E) where

import Control.Exception (IOException, SomeException, bracket, catch, displayException, try)
import Control.Monad (forM, forM_, unless, when)
import Data.Aeson (FromJSON (..), Value (..), eitherDecodeFileStrict, encode, object, withObject, (.!=), (.:), (.:?), (.=))
import Data.Aeson.Types (parseEither)
import Data.IORef (IORef, modifyIORef', newIORef, readIORef, writeIORef)
import Data.List (nub, sort, stripPrefix)
import Data.Maybe (catMaybes)
import qualified Data.Text as T
import qualified Data.Text.Encoding as TE
import qualified Data.ByteString.Char8 as C
import GHC.Clock (getMonotonicTime)
import qualified Data.Aeson.Key as K
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as BL
import Data.Bits (shiftL, (.|.))
import Data.Int (Int32)
import Data.Word (Word32)
import Numeric (readHex, showFFloat, showHex)
import System.Directory (createDirectory, doesDirectoryExist, getCurrentDirectory, listDirectory, removeDirectory, setCurrentDirectory)
import System.FilePath ((</>))
import System.IO (BufferMode (LineBuffering), hSetBuffering, stdout)
import System.Posix.Files (setFileMode)
import EmulatorUsb
import FujiAF.RamPush
import FujiAF.Transport (ensurePrivateDirectory, realClock, writePrivateBytes)
import Xm1UnreviewedRelink (runUnreviewedRamPush, runUnreviewedRamUnload, withUnreviewedBuild)
import RamPushReviewedPins (reviewedContract)

-- | Unreviewed skips the compiled source-identity and publication-contract pins; its policy
-- decides whether a bank that fails the ELF allowlist is still pushed. Reviewed enforces it.
data RelinkMode = ReviewedRelink | UnreviewedRelink AllowlistPolicy deriving (Eq, Show)

-- | Fresh cold-boots and replaces the keyed attach checkpoint.
data BootMode = CheckpointedBoot | FreshBoot deriving (Eq, Show)

-- | FlashCandidate boots a DAT whose own loader publishes the bank and runs the segments
-- without ram-alloc, ram-push or ram-unload.
data PushMode = RamPush | FlashCandidate deriving (Eq, Show)

data E2ESettings = E2ESettings
  { relinkMode :: RelinkMode
  , bootMode :: BootMode
  , checkpointRoot :: FilePath
  , segmentSelection :: Maybe [T.Text]
  , pushMode :: PushMode
  }

relinkModeText :: RelinkMode -> T.Text
relinkModeText ReviewedRelink = "reviewed"
relinkModeText (UnreviewedRelink _) = "unreviewed"

data MenuState = MenuState Int Int Int deriving (Eq, Show)

require :: Bool -> String -> IO ()
require yes message = unless yes (ioError (userError message))

privateDirectory :: FilePath -> IO ()
privateDirectory path = createDirectory path >> setFileMode path 0o700

-- | Entry of the plain-image drawer that renders a menu message banner; r0 is the message id.
menuMessageDrawer :: Word32
menuMessageDrawer = 0x011392e0

menuMessageSite :: String
menuMessageSite = "menu-message"

-- | The D3 recipe hook's entry in the image-4 prefix (reports/recipe-patch-d3.md section 5.2).
d3HookEntry :: Word32
d3HookEntry = 0x0061c3ac

d3HookSite :: String
d3HookSite = "d3-hook"

hexBytes :: B.ByteString -> String
hexBytes = concatMap (\byte -> let digits = showHex byte "" in replicate (2 - length digits) '0' ++ digits) . B.unpack

-- | One trajectory segment: its phase name, the mode-dial detent set before its first key, the
-- expected menu state after each key, the message ids the menu message drawer is entered with during
-- the segment (none when absent), and an optional capture label taken after the last key. An
-- observing segment records each key's actual state instead of requiring the expected one; `reads`
-- are guest RAM windows recorded after the last key.
data Segment = Segment
  { segmentPhase :: T.Text
  , segmentModeDial :: Maybe ModeDial
  , segmentCapture :: Maybe String
  , segmentSteps :: [(String, MenuState)]
  , segmentMessages :: [Word32]
  , segmentObserve :: Bool
  , segmentReads :: [(String, ReadAddress, Int)]
  , segmentRequires :: [T.Text]
  }

-- | A read window's start: a fixed guest address, or a symbol of the relinked runtime.
data ReadAddress = FixedAddress Word32 | RuntimeSymbol String

instance FromJSON Segment where
  parseJSON = withObject "segment" $ \segment -> Segment
    <$> segment .: "phase"
    <*> (traverse modeDial =<< segment .:? "mode_dial")
    <*> segment .:? "capture"
    <*> (map step <$> segment .: "steps")
    <*> segment .:? "messages" .!= []
    <*> segment .:? "observe" .!= False
    <*> (mapM window =<< segment .:? "reads" .!= [])
    <*> segment .:? "requires" .!= []
    where
      step (key, menu, setting, cursor) = (key, MenuState menu setting cursor)
      modeDial position = case (position :: T.Text) of
        "M" -> pure ModeDialManual
        _ -> fail ("unsupported mode-dial position: " ++ T.unpack position)
      window (label, address, len) = case T.unpack address of
        text | Just symbol <- stripPrefix "runtime:" text, not (null symbol) -> pure (label, RuntimeSymbol symbol, len)
             | [(value, "")] <- readHex text -> pure (label, FixedAddress value, len)
             | otherwise -> fail ("read address must be hex without 0x or runtime:SYMBOL: " ++ text)

data Trajectories = Trajectories
  { beforeUnload :: [Segment]
  , afterUnload :: [Segment]
  }

instance FromJSON Trajectories where
  parseJSON = withObject "trajectories" $ \document -> do
    schema <- document .: "schema"
    unless (schema == (1 :: Int)) (fail "unsupported trajectory schema")
    Trajectories <$> document .: "before_unload" <*> document .: "after_unload"

menuState :: Value -> IO MenuState
menuState value = either (ioError . userError . ("invalid menu response: " ++)) pure $ parseEither parser value
  where
    parser = withObject "response" $ \response ->
      response .: "menu" >>= withObject "menu" (\current ->
        MenuState <$> current .: "menu_id" <*> current .: "setting_id" <*> current .: "cursor")
-- | A step key of four colon-separated fields (seek or cursor, a key, a target, a press limit) presses
-- the key until the setting id (seek) or the cursor (cursor) equals the target, at most limit times;
-- any other key is pressed once. Returns the key presses made and the final state.
pressStep :: Emulator -> String -> IO ([String], MenuState)
pressStep emulator key = case splitOn ':' key of
  [kind, seekKey, targetText, limit]
    | kind `elem` ["seek", "cursor"], [(target, "")] <- reads targetText, [(bound, "")] <- reads limit ->
    let field (MenuState _ setting cursor) = if kind == "seek" then setting else cursor
        go count pressed = do
          actual <- emulatorKey emulator seekKey >>= menuState
          if field actual == target || count + 1 >= (bound :: Int)
            then pure (reverse (seekKey : pressed), actual)
            else go (count + 1) (seekKey : pressed)
     in go 0 []
  _ -> (\actual -> ([key], actual)) <$> (emulatorKey emulator key >>= menuState)
  where
    splitOn c text = case break (== c) text of
      (front, []) -> [front]
      (front, _ : rest) -> front : splitOn c rest

press :: Emulator -> String -> MenuState -> IO ()
press emulator key expected = do
  (_, actual) <- pressStep emulator key
  require (actual == expected) ("menu trajectory after " ++ key ++ ": expected " ++ show expected ++ ", got " ++ show actual)

presses :: Emulator -> [(String, MenuState)] -> IO ()
presses emulator = mapM_ (uncurry (press emulator))

-- | Presses every step and records the key presses, expected and actual state and wall seconds
-- without requiring the state.
observe :: Emulator -> [(String, MenuState)] -> IO [Value]
observe emulator = mapM $ \(key, expected@(MenuState em es ec)) -> do
  started <- getMonotonicTime
  (pressed, MenuState am as ac) <- pressStep emulator key
  ended <- getMonotonicTime
  pure (object [ "key" .= key, "pressed" .= length pressed
               , "expected" .= [em, es, ec], "actual" .= [am, as, ac]
               , "matched" .= (MenuState am as ac == expected), "seconds" .= (ended - started) ])

hexAddress :: Word32 -> String
hexAddress value = "0x" ++ replicate (8 - length digits) '0' ++ digits where digits = showHex value ""


symbolAddress :: FilePath -> String -> IO Word32
symbolAddress path name = do
  rows <- lines <$> readFile path
  let addresses =
        [ value
        | row <- rows
        , let fields = words row
        , not (null fields)
        , last fields == name
        , (address : _) <- [fields]
        , (value, "") <- readHex address
        ]
  case addresses of
    [address] -> pure address
    _ -> ioError (userError ("linked symbol is not unique: " ++ name))

mailboxFields :: [String]
mailboxFields =
  [ "magic", "abi", "nonce", "operation", "status", "validated", "setup_stored"
  , "activation_stored", "restored", "service_core", "peer_nonce", "peer_core", "peer_status"
  , "peer_lines", "spin_bound", "cpu0_ack_spins"
  , "rendezvous_parked", "rendezvous_go", "rendezvous_completed", "cpu0_park_spins", "cpu1_spins"
  , "cpu0_cycles", "cpu0_instructions", "cpu1_cycles", "cpu1_instructions", "reserved" ]

-- | Little-endian word `index` of `bytes`.
leWord :: B.ByteString -> Int -> Word32
leWord bytes index = foldr (\byte acc -> acc `shiftL` 8 .|. fromIntegral byte) 0
  (B.unpack (B.take 4 (B.drop (4 * index) bytes)))

mailboxReport :: B.ByteString -> Value
mailboxReport bytes = object (zipWith field mailboxFields [0 ..])
  where
    field name index
      | name `elem` ["status", "peer_status"] = K.fromString name .= (fromIntegral (leWord bytes index) :: Int32)
      | otherwise = K.fromString name .= leWord bytes index

-- | The segments one emulator process runs, whether it unloads between them, and whether it may
-- start from a matching install checkpoint instead of allocating and pushing.
data Plan = Plan
  { planName :: T.Text
  , planBefore :: [Segment]
  , planAfter :: [Segment]
  , planUnload :: Bool
  , planFromInstall :: Bool
  }

phaseNames :: Trajectories -> [T.Text]
phaseNames trajectories = nub (map segmentPhase (beforeUnload trajectories ++ afterUnload trajectories))

-- | The named phase with every phase it transitively requires, in trajectory order; a phase after
-- unload brings the unload with it.
segmentPlan :: Trajectories -> T.Text -> IO Plan
segmentPlan trajectories name = do
  require (name `elem` phaseNames trajectories) ("unknown trajectory segment: " ++ T.unpack name)
  let entries = beforeUnload trajectories ++ afterUnload trajectories
      requiresOf phase = nub (concat [segmentRequires entry | entry <- entries, segmentPhase entry == phase])
      close [] seen = seen
      close (phase : rest) seen
        | phase `elem` seen = close rest seen
        | otherwise = close (requiresOf phase ++ rest) (phase : seen)
      chosen = close [name] []
      selected = filter ((`elem` chosen) . segmentPhase)
      after = selected (afterUnload trajectories)
  pure (Plan name (selected (beforeUnload trajectories)) after (not (null after)) True)

-- | A relinker build held in memory.
data Relinked = Relinked
  { relinkedDocument :: B.ByteString
  , relinkedRuntime :: B.ByteString
  , relinkedSyms :: B.ByteString
  , relinkedDisasm :: B.ByteString
  , relinkedManifest :: RamPushManifest
  , relinkedAllowlist :: AllowlistOutcome
  }

-- | What an install checkpoint carries for the host: the allocation and install state files, the
-- installer mailbox after install and the heap break around ram-alloc.
data InstallHost = InstallHost
  { hostAllocationState :: B.ByteString
  , hostPushState :: B.ByteString
  , hostInstallMailbox :: Value
  , hostBreakBefore :: String
  , hostBreakAfter :: String
  }

instance FromJSON InstallHost where
  parseJSON = withObject "install host" $ \host -> InstallHost
    <$> (TE.encodeUtf8 <$> host .: "allocation_state")
    <*> (TE.encodeUtf8 <$> host .: "push_state")
    <*> host .: "install_mailbox"
    <*> host .: "heap_break_before"
    <*> host .: "heap_break_after"

installHostValue :: InstallHost -> Value
installHostValue host = object
  [ "allocation_state" .= TE.decodeUtf8 (hostAllocationState host)
  , "push_state" .= TE.decodeUtf8 (hostPushState host)
  , "install_mailbox" .= hostInstallMailbox host
  , "heap_break_before" .= hostBreakBefore host
  , "heap_break_after" .= hostBreakAfter host ]

-- | Runs the whole trajectory in one process, or with `segmentSelection` each named segment (and
-- what it requires) in its own process from the install checkpoint, under RECEIPT/segment-NAME.
runRamPushE2E :: E2ESettings -> FilePath -> FilePath -> FilePath -> FilePath -> FilePath -> FilePath -> FilePath -> IO ()
runRamPushE2E settings python workspace trajectoryFile installedMedia adjustmentNand cardImage receipt = do
  hSetBuffering stdout LineBuffering
  trajectories <- eitherDecodeFileStrict trajectoryFile
    >>= either (ioError . userError . ("invalid RAM-push e2e trajectories: " ++)) pure
  forM_ [phase | entry <- beforeUnload trajectories ++ afterUnload trajectories, phase <- segmentRequires entry] $ \phase ->
    require (phase `elem` phaseNames trajectories) ("trajectory requires an unknown segment: " ++ T.unpack phase)
  exists <- doesDirectoryExist receipt
  when exists $ do
    entries <- listDirectory receipt
    require (null entries) "RAM-push receipt directory must be fresh or empty"
    removeDirectory receipt
  runtime <- getCurrentDirectory
  -- Host RAM-push operations resolve workspace-relative builder paths.
  setCurrentDirectory workspace
  let session = case pushMode settings of
        RamPush -> runSession settings python runtime installedMedia adjustmentNand cardImage
        FlashCandidate -> runFlashSession settings python runtime installedMedia adjustmentNand cardImage
      selection = case (segmentSelection settings, pushMode settings) of
        (Just names, _) -> Just names
        (Nothing, FlashCandidate) -> Just (phaseNames trajectories)
        (Nothing, RamPush) -> Nothing
  case selection of
    Nothing -> session (Plan "all" (beforeUnload trajectories) (afterUnload trajectories) True False) receipt
    Just names -> do
      plans <- mapM (segmentPlan trajectories) names
      when (pushMode settings == FlashCandidate) $ forM_ plans $ \plan ->
        require (not (planUnload plan)) ("segment " ++ T.unpack (planName plan) ++ " follows ram-unload, which a flash candidate does not run")
      privateDirectory receipt
      outcomes <- forM plans $ \plan -> do
        started <- getMonotonicTime
        outcome <- try (session plan (receipt </> ("segment-" ++ T.unpack (planName plan))))
        ended <- getMonotonicTime
        let failure = either (\(problem :: SomeException) -> Just (displayException problem)) (const Nothing) outcome
        putStrLn ("segment " ++ T.unpack (planName plan) ++ ": " ++ maybe "passed" ("failed: " ++) failure
          ++ " in " ++ seconds (ended - started))
        pure (object [ "segment" .= planName plan, "seconds" .= (ended - started)
                     , "result" .= String (maybe "passed" (const "failed") failure), "message" .= failure ], failure)
      writePrivateBytes (receipt </> "e2e-segments.json") (BL.toStrict (encode (map fst outcomes)) <> "\n")
      require (all (null . snd) outcomes) "one or more trajectory segments failed"

seconds :: Double -> String
seconds value = showFFloat (Just 1) value " s"

runSession :: E2ESettings -> FilePath -> FilePath -> FilePath -> FilePath -> FilePath -> Plan -> FilePath -> IO ()
runSession settings python runtime installedMedia adjustmentNand cardImage plan e2e = do
  phases <- newIORef []
  let timed :: T.Text -> IO a -> IO a
      timed label action = do
        started <- getMonotonicTime
        result <- action
        ended <- getMonotonicTime
        modifyIORef' phases (++ [object ["name" .= label, "seconds" .= (ended - started)]])
        putStrLn ("phase " ++ T.unpack label ++ ": " ++ seconds (ended - started))
        pure result
      arguments =
        [ "tools/continue_rec_scanout.py"
        , "--profile", "firmware/x-m1/1.50/emulator-rec.json"
        , "--installed-dat", installedMedia
        , "--adjustment-nand", adjustmentNand
        , "--menu-sd-image", cardImage
        , "--menus"
        , "--ram-push-server"
        , "--output", e2e
        , "--ram-push-checkpoints", checkpointRoot settings
        ] ++ ["--fresh-boot" | bootMode settings == FreshBoot]
      relinkAt base = case relinkMode settings of
        ReviewedRelink -> withPinnedReviewedBuild reviewedContract base
        UnreviewedRelink policy -> withUnreviewedBuild policy base
      pushWith = case relinkMode settings of
        ReviewedRelink -> runRamPush reviewedContract
        UnreviewedRelink _ -> runUnreviewedRamPush
      unloadWith = case relinkMode settings of
        ReviewedRelink -> runRamUnload reviewedContract
        UnreviewedRelink _ -> runUnreviewedRamUnload
      relinkFiles base = relinkAt base $ \pinned -> Relinked (pinnedDocument pinned) (pinnedRuntime pinned)
        <$> B.readFile (pinnedDirectory pinned </> "kernel.syms")
        <*> B.readFile (pinnedDirectory pinned </> "kernel.disasm")
        <*> pure (pinnedManifest pinned)
        <*> pure (pinnedAllowlist pinned)
      choose candidates
        | not (planFromInstall plan) || bootMode settings == FreshBoot = pure (Nothing, Nothing)
        | otherwise = do
            built <- mapM (\base -> (,) base <$> relinkFiles (Address base)) (nub (map installBase candidates))
            let matches =
                  [ (candidate, relinked)
                  | candidate <- candidates
                  , Just relinked <- [lookup (installBase candidate) built]
                  , manifestDigest (relinkedManifest relinked) == installManifestDigest candidate
                  , runtimeDigest (relinkedManifest relinked) == installRuntimeDigest candidate ]
            pure (case matches of
              (candidate, relinked) : _ -> (Just candidate, Just (candidate, relinked))
              [] -> (Nothing, Nothing))
  bracket (timed "emulator-start" (startEmulator python runtime arguments choose)) (\(emulator, _, _) -> stopEmulator emulator) $ \(emulator, start, installed) -> do
    putStrLn ("emulator start: " ++ show (bootStart start) ++ "; relink: " ++ T.unpack (relinkModeText (relinkMode settings)))
    let api = emulatorUsbApi emulator
        allocationDirectory = e2e </> "alloc"
        relinkDirectory = e2e </> "relinked"
        pushDirectory = e2e </> "push"
        unloadDirectory = e2e </> "unload"
        allocationBytes = reviewedAllocationSize
        heapBreak = (\bytes -> hexAddress (leWord bytes 0)) <$> emulatorReadMemory emulator 0x00829a08 4
        writeRelinked relinked = do
          privateDirectory relinkDirectory
          writePrivateBytes (relinkDirectory </> "runtime.bin") (relinkedRuntime relinked)
          writePrivateBytes (relinkDirectory </> "ram-push-manifest.json") (relinkedDocument relinked)
          writePrivateBytes (relinkDirectory </> "kernel.syms") (relinkedSyms relinked)
          writePrivateBytes (relinkDirectory </> "kernel.disasm") (relinkedDisasm relinked)
          let allowlist = allowlistReceipt (relinkedAllowlist relinked)
          writePrivateBytes (relinkDirectory </> "bank-allowlist.json") (BL.toStrict (encode allowlist) <> "\n")
          pure allowlist
    setFileMode e2e 0o700
    ensurePrivateDirectory e2e
    privateDirectory allocationDirectory
    restoredHost <- forM installed $ \(candidate, _) ->
      either (ioError . userError . ("invalid install checkpoint host record: " ++)) pure
        (parseEither parseJSON (installHost candidate))
    (retained, breakBefore, breakAfter, allowlist) <- case (installed, restoredHost) of
      (Just (_, relinked), Just host) -> do
        let path = allocationDirectory </> "ram-alloc-state.json"
        writePrivateBytes path (hostAllocationState host)
        retained <- readRetainedAllocation path
        allowlist <- writeRelinked relinked
        pure (retained, hostBreakBefore host, hostBreakAfter host, allowlist)
      _ -> do
        breakBefore <- heapBreak
        allocationPath <- timed "ram-alloc" (runRamAlloc allocationBytes realClock api allocationDirectory) >>= \outcome -> case outcome of
          RamAllocated path -> pure path
          other -> ioError (userError ("ram-alloc failed: " ++ show other))
        retained <- readRetainedAllocation allocationPath
        breakAfter <- heapBreak
        allowlist <- timed "relink" (relinkFiles (allocBase (retainedAllocation retained))) >>= writeRelinked
        pure (retained, breakBefore, breakAfter, allowlist)
    let allocation = retainedAllocation retained
        allocationPath = retainedStatePath retained
        Address base = allocBase allocation
    manifest <- readRamPushManifest (relinkDirectory </> "ram-push-manifest.json")
    rendezvous <- symbolAddress (relinkDirectory </> "kernel.syms") "ram_push_rendezvous_locked"
    cpu0Exit <- symbolAddress (relinkDirectory </> "kernel.syms") "ram_push_cpu0_window_exit"
    cpu1Exit <- symbolAddress (relinkDirectory </> "kernel.syms") "ram_push_cpu1_window_exit"
    mailbox <- symbolAddress (relinkDirectory </> "kernel.syms") "ram_push_mailbox"
    readSymbols <- mapM (\name -> (,) name <$> symbolAddress (relinkDirectory </> "kernel.syms") name)
      (nub [name | segment <- planBefore plan ++ planAfter plan
                 , (_, RuntimeSymbol name, _) <- segmentReads segment])
    let readAddress (FixedAddress value) = pure value
        readAddress (RuntimeSymbol name) =
          maybe (ioError (userError ("unresolved runtime symbol: " ++ name))) pure (lookup name readSymbols)
    let readMailbox = mailboxReport <$> emulatorReadMemory emulator mailbox 104
        stop phase message = do
          report <- readMailbox
          writePrivateBytes (e2e </> "e2e-failure.json") (BL.toStrict (encode (object
            [ "phase" .= String phase, "message" .= message, "mailbox" .= report
            , "relink" .= relinkModeText (relinkMode settings)
            , "heap_break_before" .= breakBefore, "heap_break_after" .= breakAfter ])) <> "\n")
          ioError (userError (show phase ++ " failed: " ++ message))
    let entries = manifestEntries manifest
    emulatorTraceSites emulator
      [ (unAddress (installEntry entries), "uploaded-installer")
      , (unAddress (rollbackEntry entries), "uploaded-rollback")
      , (unAddress (statusEntry entries), "uploaded-status")
      , (rendezvous, "rendezvous-locked")
      , (cpu0Exit, "cpu0-window-exit")
      , (cpu1Exit, "cpu1-window-exit")
      , (menuMessageDrawer, menuMessageSite)
      , (d3HookEntry, d3HookSite)
      ]
    privateDirectory pushDirectory
    (statePath, installMailbox) <- case restoredHost of
      Just host -> do
        let path = pushDirectory </> "ram-push-state.json"
        writePrivateBytes path (hostPushState host)
        pure (path, hostInstallMailbox host)
      Nothing -> do
        statePath <- timed "ram-push" (pushWith retained manifest realClock api pushDirectory) >>= \outcome -> case outcome of
          RamPushInstalled path -> pure path
          other -> stop "ram-push" (show other)
        installMailbox <- readMailbox
        host <- InstallHost <$> B.readFile allocationPath <*> B.readFile statePath
          <*> pure installMailbox <*> pure breakBefore <*> pure breakAfter
        saved <- timed "install-checkpoint" (emulatorSaveInstallCheckpoint emulator base
          (manifestDigest manifest) (runtimeDigest manifest) (installHostValue host))
        putStrLn ("install checkpoint: " ++ C.unpack (BL.toStrict (encode saved)))
        pure (statePath, installMailbox)
    state <- readRamPushState statePath
    runner <- newSegmentRunner emulator e2e (\label action -> timed label action) readAddress (\phase message -> stop phase message)
    let segments = runSegments runner
    capturedBefore <- segments (planBefore plan)
    unloadMailbox <- if planUnload plan
      then do
        privateDirectory unloadDirectory
        unload <- timed "ram-unload" (unloadWith state realClock api unloadDirectory)
        unless (unload == RamUnloaded) (stop "ram-unload" (show unload))
        Just <$> readMailbox
      else pure Nothing
    capturedAfter <- segments (planAfter plan)
    observed <- readIORef (runnerObservations runner)
    phaseTimes <- readIORef phases
    let result = object
          [ "schema" .= (1 :: Int)
          , "scope" .= String "emulated X-M1 RC0 ordinary-boot RAM push"
          , "plan" .= planName plan
          , "relink" .= object ["mode" .= relinkModeText (relinkMode settings),
                                "source_identity" .= object [K.fromString name .= digest | (name, digest) <- sourceIdentity manifest],
                                "allowlist" .= allowlist]
          , "emulator_start" .= startRecord start
          , "phases" .= (phaseTimes :: [Value])
          , "allocation" .= object ["base" .= hexAddress base, "bytes" .= allocationBytes,
                                     "cpu0" .= hexAddress (allocCpu0 allocation),
                                     "heap_break_before" .= breakBefore, "heap_break_after" .= breakAfter]
          , "runtime" .= object ["bytes" .= allocationSize manifest,
                                  "publications" .= length (manifestPublications manifest),
                                  "manifest" .= manifestPath manifest]
          , "states" .= object ["allocation" .= allocationPath, "installed" .= statePath]
          , "menu_receipts" .= catMaybes (capturedBefore ++ capturedAfter)
          , "trajectories" .= map (\name -> object ["name" .= name, "result" .= String "matched"])
              (nub (map segmentPhase (filter (not . segmentObserve) (planBefore plan ++ planAfter plan))))
          , "observations" .= (observed :: [Value])
          , "mailbox" .= object ["install" .= installMailbox, "rollback" .= unloadMailbox,
                                 "address" .= hexAddress mailbox]
          , "unload" .= (if planUnload plan
              then Just (String (T.pack (show publicationCount ++ " old words verified; allocation retained until power-off")))
              else Nothing)
          ]
    writePrivateBytes (e2e </> "e2e-result.json") (BL.toStrict (encode result) <> "\n")

-- | What running segments needs from its session; hits and observations accumulate across calls.
data SegmentRunner = SegmentRunner
  { runnerEmulator :: Emulator
  , runnerReceipt :: FilePath
  , runnerTimed :: T.Text -> IO (Maybe Value) -> IO (Maybe Value)
  , runnerReadAddress :: ReadAddress -> IO Word32
  , runnerStop :: T.Text -> String -> IO ()
  , runnerHits :: IORef Int
  , runnerObservations :: IORef [Value]
  }

newSegmentRunner :: Emulator -> FilePath -> (T.Text -> IO (Maybe Value) -> IO (Maybe Value))
  -> (ReadAddress -> IO Word32) -> (T.Text -> String -> IO ()) -> IO SegmentRunner
newSegmentRunner emulator receipt timedAction readAddress stopAction = do
  (recorded, _) <- emulatorTraceHits emulator 0
  SegmentRunner emulator receipt timedAction readAddress stopAction <$> newIORef recorded <*> newIORef []

-- | Runs each segment and returns its capture receipt.
runSegments :: SegmentRunner -> [Segment] -> IO [Maybe Value]
runSegments runner = mapM $ \segment -> runnerTimed runner ("segment " <> segmentPhase segment) $ do
  let emulator = runnerEmulator runner
      stop = runnerStop runner
  (before, _) <- readIORef (runnerHits runner) >>= emulatorTraceHits emulator
  forM_ (segmentModeDial segment) $ \position -> emulatorModeDial emulator position
  observed <- if segmentObserve segment
    then observe emulator (segmentSteps segment)
    else [] <$ (presses emulator (segmentSteps segment) `catch` \(failure :: IOException) ->
      stop (segmentPhase segment) (displayException failure))
  captured <- traverse (emulatorCapture emulator) (segmentCapture segment)
  windows <- mapM (\(label, location, count) -> do
      address <- runnerReadAddress runner location
      bytes <- emulatorReadMemory emulator address count
      pure (object ["label" .= label, "address" .= hexAddress address, "bytes" .= hexBytes bytes]))
    (segmentReads segment)
  (after, hits) <- emulatorTraceHits emulator before
  writeIORef (runnerHits runner) after
  let drawn = sort (nub [traceR0 hit | hit <- hits, traceSite hit == menuMessageSite])
      hookRows = [traceR0 hit | hit <- hits, traceSite hit == d3HookSite]
  when (segmentObserve segment || not (null (segmentReads segment)) || not (null hookRows)) $ do
    previous <- readIORef (runnerObservations runner)
    let current = object [ "phase" .= segmentPhase segment, "capture" .= segmentCapture segment
                         , "steps" .= observed, "reads" .= windows
                         , "d3_hook_r0" .= map hexAddress hookRows ]
    writeIORef (runnerObservations runner) (previous ++ [current])
    writePrivateBytes (runnerReceipt runner </> ("e2e-observation-" ++ show (length previous) ++ ".json"))
      (BL.toStrict (encode current) <> "\n")
  unless (segmentObserve segment || drawn == sort (nub (segmentMessages segment))) $
    stop (segmentPhase segment) ("menu messages drawn " ++ show (map hexAddress drawn)
      ++ ", expected " ++ show (map hexAddress (segmentMessages segment)))
  pure captured

-- | One flash-candidate process: boot or restore the attach checkpoint keyed by the candidate DAT,
-- then run the plan's segments; the candidate's loader has already published the bank.
runFlashSession :: E2ESettings -> FilePath -> FilePath -> FilePath -> FilePath -> FilePath -> Plan -> FilePath -> IO ()
runFlashSession settings python runtime installedMedia adjustmentNand cardImage plan e2e = do
  phases <- newIORef []
  let timed :: T.Text -> IO a -> IO a
      timed label action = do
        started <- getMonotonicTime
        result <- action
        ended <- getMonotonicTime
        modifyIORef' phases (++ [object ["name" .= label, "seconds" .= (ended - started)]])
        putStrLn ("phase " ++ T.unpack label ++ ": " ++ seconds (ended - started))
        pure result
      arguments =
        [ "tools/continue_rec_scanout.py"
        , "--profile", "firmware/x-m1/1.50/emulator-rec.json"
        , "--installed-dat", installedMedia
        , "--adjustment-nand", adjustmentNand
        , "--menu-sd-image", cardImage
        , "--menus"
        , "--ram-push-server"
        , "--output", e2e
        , "--ram-push-checkpoints", checkpointRoot settings
        ] ++ ["--fresh-boot" | bootMode settings == FreshBoot]
      noInstall _ = pure (Nothing, ())
  bracket (timed "emulator-start" (startEmulator python runtime arguments noInstall)) (\(emulator, _, _) -> stopEmulator emulator) $ \(emulator, start, ()) -> do
    putStrLn ("emulator start: " ++ show (bootStart start) ++ "; flash candidate")
    setFileMode e2e 0o700
    ensurePrivateDirectory e2e
    emulatorTraceSites emulator [(menuMessageDrawer, menuMessageSite), (d3HookEntry, d3HookSite)]
    let readAddress (FixedAddress value) = pure value
        readAddress (RuntimeSymbol name) =
          ioError (userError ("runtime symbol " ++ name ++ " belongs to a RAM-push build, not a flash candidate"))
        stop phase message = do
          writePrivateBytes (e2e </> "e2e-failure.json") (BL.toStrict (encode (object
            [ "phase" .= String phase, "message" .= message ])) <> "\n")
          ioError (userError (show phase ++ " failed: " ++ message))
    runner <- newSegmentRunner emulator e2e (\label action -> timed label action) readAddress stop
    captured <- runSegments runner (planBefore plan)
    observed <- readIORef (runnerObservations runner)
    phaseTimes <- readIORef phases
    writePrivateBytes (e2e </> "e2e-result.json") (BL.toStrict (encode (object
      [ "schema" .= (1 :: Int)
      , "scope" .= String "emulated X-M1 flash candidate UI segments"
      , "plan" .= planName plan
      , "emulator_start" .= startRecord start
      , "phases" .= (phaseTimes :: [Value])
      , "menu_receipts" .= catMaybes captured
      , "trajectories" .= map (\name -> object ["name" .= name, "result" .= String "matched"])
          (nub (map segmentPhase (filter (not . segmentObserve) (planBefore plan))))
      , "observations" .= (observed :: [Value])
      ])) <> "\n")
