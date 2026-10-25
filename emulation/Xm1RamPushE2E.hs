{-# LANGUAGE OverloadedStrings #-}
module Xm1RamPushE2E (RelinkMode (..), AllowlistPolicy (..), OverrideReason (..), BootMode (..), PushMode (..), AttachHold (..), UiConversionProfile (..), CardMode (..), EmulationRuntime (..), FujiModBridge (..), E2ESettings (..), Payload, bankD3Payload, classicChromePayload, classicChromeIconPayload, runRamPushE2E, warmAttachCheckpoint) where

import Control.Applicative ((<|>))
import Control.Exception (IOException, SomeException, bracket, catch, displayException, finally, try)
import Control.Monad (forM, forM_, unless, when)
import Data.Aeson (FromJSON (..), Value (..), encode, object, withObject, (.:), (.=))
import Data.Aeson.Types (parseEither)
import Data.IORef (IORef, modifyIORef', newIORef, readIORef, writeIORef)
import Data.List (nub, sort)
import Data.Maybe (catMaybes, isJust)
import qualified Data.Text as T
import qualified Data.Text.Encoding as TE
import qualified Data.ByteString.Char8 as C
import GHC.Clock (getMonotonicTime)
import qualified Data.Aeson.Key as K
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as BL
import Data.Bits (shiftL, shiftR, (.|.))
import Data.Int (Int32)
import Data.Word (Word32)
import Numeric (showFFloat, showHex)
import System.Directory (createDirectory, doesDirectoryExist, doesFileExist, getCurrentDirectory, listDirectory, removeDirectory, removeDirectoryRecursive, setCurrentDirectory)
import System.Environment (getExecutablePath, lookupEnv)
import System.FilePath (takeDirectory, takeFileName, (</>))
import System.IO (BufferMode (LineBuffering), hSetBuffering, stdout)
import System.Posix.Files (setFileMode)
import System.Posix.Temp (mkdtemp)
import Emulation.CheckpointKey (CheckpointRoute (..), Interpreter (..), RouteKey (..), UnicornLibrary (..), guestState, routeKey, routeKeyHex)
import Emulation.Sha256 (sha256File, sha256Hex)
import Emulation.Symbols (SymbolTable, parseSymbolTable, resolveSymbol)
import Emulation.UiTrajectory
import Emulation.ViewerRuntime (FixedSceneFrame (..), fixedSceneFrameArguments)
import EmulatorUsb
import FujiAF.RamPush
import FujiAF.Transport (ensurePrivateDirectory, realClock, writePrivateBytes)
import Xm1UnreviewedRelink (runUnreviewedRamPush, runUnreviewedRamUnload, withUnreviewedBuild)
import RamPushReviewedPins (reviewedContract)
import RamPushCcReviewedPins (reviewedCcContract)
import RamPushCcIconReviewedPins (reviewedCcIconContract)

-- | Unreviewed skips the compiled source-identity and publication-contract pins; its policy
-- decides whether a bank that fails the ELF allowlist is still pushed. Reviewed enforces it.
data RelinkMode = ReviewedRelink | UnreviewedRelink AllowlistPolicy deriving (Eq, Show)

-- | Fresh cold-boots and replaces the keyed attach checkpoint.
data BootMode = CheckpointedBoot | FreshBoot deriving (Eq, Show)

-- | FlashCandidate boots a DAT whose own loader publishes the bank and runs the segments
-- without ram-alloc, ram-push or ram-unload.
data PushMode = RamPush | FlashCandidate deriving (Eq, Show)

-- | A key held across the attach edge of every boot the e2e starts.
data AttachHold = NoAttachHold | AttachHoldQ deriving (Eq, Show)

-- | The UI conversion contract a session boots with; DeclaredUiCpuFallback is the
-- viewer's opt-in stock CPU conversion, which mode-dial positions with a mode guide need.
data UiConversionProfile = StrictUiConversion | DeclaredUiCpuFallback deriving (Eq, Show)

-- | CardBacked boots with the named virtual card image; Cardless boots with card-detect absent
-- through the cardless RAM-push bridge.
data CardMode = CardBacked FilePath | Cardless deriving (Eq, Show)

-- | The FujiMod RAM-push bridge (`emulation/fujimod-xm1`), its run document and the boot ROM.
data FujiModBridge = FujiModBridge
  { fujimodBridge :: FilePath
  , fujimodRunDocument :: FilePath
  , fujimodBootRom :: FilePath
  } deriving (Eq, Show)

-- | The engine behind the bridge protocol: the retained Python/native closure or jitl's FujiMod.
data EmulationRuntime = RetainedRuntime | FujiModRuntime FujiModBridge deriving (Eq, Show)

data E2ESettings = E2ESettings
  { relinkMode :: RelinkMode
  , bootMode :: BootMode
  , checkpointRoot :: FilePath
  , segmentSelection :: Maybe [T.Text]
  , pushMode :: PushMode
  , e2ePayload :: Payload
  , attachHold :: AttachHold
  , flashUiConversion :: UiConversionProfile
  , emulationCardMode :: CardMode
  , fixedSceneFrame :: Maybe FixedSceneFrame
    -- | The card attach checkpoint key, which 'runRamPushE2E' sets.
  , cardAttachKey :: Maybe RouteKey
  , emulationRuntime :: EmulationRuntime
  -- | The DAT the user supplied, which a flash candidate's symbol table sits beside.
  , firmwareSource :: FilePath
  }

-- | The card attach checkpoint key: the staged guest state and the session's inputs.
cardAttachRouteKey :: E2ESettings -> FilePath -> FilePath -> FilePath -> FilePath -> FilePath -> IO RouteKey
cardAttachRouteKey settings python runtime installedMedia adjustmentNand cardImage = do
  unicorn <- lookupEnv "X70_UNICORN_LIBRARY" >>= maybe (ioError (userError "X70_UNICORN_LIBRARY is missing")) pure
  guest <- guestState runtime (InterpreterExecutable python) (UnicornLibrary unicorn)
  installed <- sha256File (runtime </> installedMedia)
  adjustment <- sha256File adjustmentNand
  card <- sha256File cardImage
  pure (routeKey guest CardAttach (
    [ "profile" .= String "firmware/x-m1/1.50/emulator-rec.json"
    , "installed_dat" .= sha256Hex installed
    , "adjustment_nand" .= sha256Hex adjustment
    , "menu_sd_image" .= sha256Hex card ]
    ++ [ "attach_hold_key" .= String "q" | attachHold settings == AttachHoldQ ]
    ++ [ "ui_conversion_profile" .= String "declared-ui-cpu-fallback" | flashUiConversion settings == DeclaredUiCpuFallback ]
    ++ [ "fixed_scene_frame" .= frame | Just (FixedSceneFrame frame) <- [fixedSceneFrame settings] ]))

attachHoldArguments :: AttachHold -> [String]
attachHoldArguments NoAttachHold = []
attachHoldArguments AttachHoldQ = ["--attach-hold-key", "q"]

attachHoldText :: AttachHold -> T.Text
attachHoldText NoAttachHold = "none"
attachHoldText AttachHoldQ = "q"

cardRecord :: CardMode -> Value
cardRecord mode = object
  [ "present" .= (mode /= Cardless)
  , "image" .= (case mode of CardBacked image -> String (T.pack image); Cardless -> Null) ]

-- | The bridge executable and its arguments for one boot of the session's inputs writing to `output`.
sessionBridge :: E2ESettings -> FilePath -> FilePath -> FilePath -> BootMode -> FilePath -> IO (FilePath, [String])
sessionBridge settings python installedMedia adjustmentNand boot output = case (emulationRuntime settings, emulationCardMode settings) of
  (FujiModRuntime bridge, card) -> do
    require (card /= Cardless || attachHold settings == NoAttachHold) "the cardless RAM-push bridge holds no key across the attach edge"
    require (null (fixedSceneFrame settings)) "the FujiMod RAM-push bridge takes no --fixed-scene-frame"
    pure (fujimodBridge bridge,
      [ "--run", fujimodRunDocument bridge
      , "--boot-rom", fujimodBootRom bridge
      , "--installed-dat", installedMedia
      , "--adjustment-nand", adjustmentNand
      , "--output", output
      , "--ram-push-checkpoints", checkpointRoot settings
      ] ++ freshArguments ++ cardArguments card ++ attachHoldArguments (attachHold settings))
  (RetainedRuntime, CardBacked cardImage) -> do
    key <- maybe (ioError (userError "the card attach key is unset")) pure (cardAttachKey settings)
    pure (python,
      [ "tools/continue_rec_scanout.py"
      , "--profile", "firmware/x-m1/1.50/emulator-rec.json"
      , "--installed-dat", installedMedia
      , "--adjustment-nand", adjustmentNand
      , "--menu-sd-image", cardImage
      , "--menus"
      , "--ram-push-server"
      , "--output", output
      , "--ram-push-checkpoints", checkpointRoot settings
      , "--checkpoint-key", routeKeyHex key
      , "--checkpoint-inputs", C.unpack (BL.toStrict (encode (routeKeyDocument key)))
      ] ++ freshArguments ++ profileArguments ++ attachHoldArguments (attachHold settings) ++ frameArguments)
  (RetainedRuntime, Cardless) -> do
    require (attachHold settings == NoAttachHold) "the cardless RAM-push bridge holds no key across the attach edge"
    bridge <- runfilesCardlessBridge
    pure (bridge,
      [ "--profile", "firmware/x-m1/1.50/emulator-rec.json"
      , "--installed-dat", installedMedia
      , "--adjustment-nand", adjustmentNand
      , "--output", output
      , "--ram-push-checkpoints", checkpointRoot settings
      ] ++ freshArguments ++ profileArguments ++ frameArguments)
  where
    freshArguments = ["--fresh-boot" | boot == FreshBoot]
    profileArguments = case flashUiConversion settings of
      StrictUiConversion -> []
      DeclaredUiCpuFallback -> ["--ui-conversion-profile", "declared-ui-cpu-fallback"]
    frameArguments = fixedSceneFrameArguments (fixedSceneFrame settings)
    cardArguments (CardBacked image) = ["--sd-card-image", image]
    cardArguments Cardless = []

payloadText :: Payload -> T.Text
payloadText p = case payloadKind p of
  RecipeBankD3 -> "recipe-bank-d3"
  ClassicChrome -> "classic-chrome"
  ClassicChromeIcon -> "classic-chrome-icon"
  AfRing -> "af-ring"

payloadContract :: Payload -> ReviewedContract
payloadContract p = case payloadKind p of
  RecipeBankD3 -> reviewedContract
  ClassicChrome -> reviewedCcContract
  ClassicChromeIcon -> reviewedCcIconContract
  AfRing -> error "AF ring has no live reviewed contract; use //emulation:xm1-af-ring-e2e"

-- | Whether the payload carries the Classic Chrome publications.
carriesClassicChrome :: Payload -> Bool
carriesClassicChrome p = payloadKind p `elem` [ClassicChrome, ClassicChromeIcon]

-- | The Classic Chrome entry words and hook sites (reports/classic-chrome-patch.md
-- section 8.2.7 step 2), armed from the attach to the end of install.
classicChromeFetchSites :: [(Word32, String)]
classicChromeFetchSites =
  [ (0x00738fe8, "cc-entry-capture-converter"), (0x00736a74, "cc-entry-settings-converter")
  , (0x00d7851c, "cc-entry-marker"), (0x00eab2e0, "cc-entry-filmmode"), (0x00eaac88, "cc-entry-saturation")
  , (0x00d461ec, "cc-entry-raw-commit"), (0x00d5ada4, "cc-entry-raw-highlight"), (0x012548a4, "cc-entry-raw-request")
  , (0x00d67284, "cc-hook-colour"), (0x00d60f60, "cc-hook-chroma-1"), (0x00d64ef4, "cc-hook-chroma-2")
  , (0x00d677ac, "cc-hook-tone-1"), (0x00d677bc, "cc-hook-tone-2") ]

relinkModeText :: RelinkMode -> T.Text
relinkModeText ReviewedRelink = "reviewed"
relinkModeText (UnreviewedRelink _) = "unreviewed"

require :: Bool -> String -> IO ()
require yes message = unless yes (ioError (userError message))

privateDirectory :: FilePath -> IO ()
privateDirectory path = createDirectory path >> setFileMode path 0o700

-- | Runs a transport session in a private directory on tmpfs (XDG_RUNTIME_DIR, else /dev/shm),
-- then copies its files into DIRECTORY, which must exist, whether or not the session succeeded.
-- The transport fsyncs every journal line, which costs tens of milliseconds a transfer on disk.
stagedSession :: FilePath -> (FilePath -> IO a) -> IO a
stagedSession directory session = do
  root <- maybe "/dev/shm" id <$> lookupEnv "XDG_RUNTIME_DIR"
  bracket (mkdtemp (root </> "xm1-e2e-session-")) removeDirectoryRecursive $ \staging -> do
    let work = staging </> "session"
    privateDirectory work
    session work `finally` (listDirectory work >>= mapM_ (\name -> B.readFile (work </> name) >>= writePrivateBytes (directory </> name)))

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

menuState :: Value -> IO MenuState
menuState value = either (ioError . userError . ("invalid menu response: " ++)) pure $ parseEither parser value
  where
    parser = withObject "response" $ \response ->
      response .: "menu" >>= withObject "menu" (\current ->
        MenuState <$> current .: "menu_id" <*> current .: "setting_id" <*> current .: "cursor")
-- | A step key of four colon-separated fields (seek or cursor, a key, a target, a press limit) presses
-- the key until the setting id (seek) or the cursor (cursor) equals the target, at most limit times;
-- any other key is pressed once. Returns the key presses made and the final state.
pressStep :: Emulator -> KeyTiming -> String -> IO ([String], MenuState)
pressStep emulator timing key = case splitOn ':' key of
  [kind, seekKey, targetText, limit]
    | kind `elem` ["seek", "cursor"], [(target, "")] <- reads targetText, [(bound, "")] <- reads limit ->
    let field (MenuState _ setting cursor) = if kind == "seek" then setting else cursor
        go count pressed = do
          actual <- emulatorKey emulator timing seekKey >>= menuState
          if field actual == target || count + 1 >= (bound :: Int)
            then pure (reverse (seekKey : pressed), actual)
            else go (count + 1) (seekKey : pressed)
     in go 0 []
  _ -> (\actual -> ([key], actual)) <$> (emulatorKey emulator timing key >>= menuState)
  where
    splitOn c text = case break (== c) text of
      (front, []) -> [front]
      (front, _ : rest) -> front : splitOn c rest

-- | A step's own pulse, else the segment's; its own settle cap.
stepKeyTiming :: Maybe KeyPulse -> Step -> KeyTiming
stepKeyTiming segmentPulse step = KeyTiming (keyTimingPulse timing <|> segmentPulse) (keyTimingSettle timing)
  where timing = stepTiming step

press :: Emulator -> Maybe KeyPulse -> Step -> IO ()
press emulator pulse step = do
  (_, actual) <- pressStep emulator (stepKeyTiming pulse step) (stepKey step)
  require (actual == stepExpected step)
    ("menu trajectory after " ++ stepKey step ++ ": expected " ++ show (stepExpected step) ++ ", got " ++ show actual)

presses :: Emulator -> Maybe KeyPulse -> [Step] -> IO ()
presses emulator pulse = mapM_ (press emulator pulse)

-- | Presses every step and records the key presses, expected and actual state and wall seconds
-- without requiring the state.
observe :: Emulator -> Maybe KeyPulse -> [Step] -> IO [Value]
observe emulator pulse = mapM $ \step -> do
  let MenuState em es ec = stepExpected step
  started <- getMonotonicTime
  (pressed, MenuState am as ac) <- pressStep emulator (stepKeyTiming pulse step) (stepKey step)
  ended <- getMonotonicTime
  pure (object [ "key" .= stepKey step, "pressed" .= length pressed
               , "expected" .= [em, es, ec], "actual" .= [am, as, ac]
               , "matched" .= (MenuState am as ac == stepExpected step), "seconds" .= (ended - started) ])

hexAddress :: Word32 -> String
hexAddress value = "0x" ++ replicate (8 - length digits) '0' ++ digits where digits = showHex value ""


symbolAddress :: FilePath -> String -> IO Word32
symbolAddress path name = do
  table <- parseSymbolTable <$> B.readFile path
  either (ioError . userError) pure (resolveSymbol table name)

-- | The bank symbol table a flash candidate's package carries beside its linked kernel.
flashCandidateSymbols :: FilePath -> IO SymbolTable
flashCandidateSymbols firmware = do
  let path = takeDirectory firmware </> "kernel" </> "kernel.syms"
  present <- doesFileExist path
  unless present (ioError (userError ("the flash candidate has no symbol table: " ++ path)))
  parseSymbolTable <$> B.readFile path

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
  , planTraceSites :: [TraceSite]
  }


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
  pure (Plan name (selected (beforeUnload trajectories)) after (not (null after)) True (trajectoryTraceSites trajectories))

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

-- | GIVEN with its card attach key, timed and printed; the cardless and FujiMod bridges key their own.
keyedSettings :: E2ESettings -> FilePath -> FilePath -> FilePath -> FilePath -> IO E2ESettings
keyedSettings given python runtime installedMedia adjustmentNand = case (emulationRuntime given, emulationCardMode given) of
  (FujiModRuntime _, _) -> pure given
  (RetainedRuntime, Cardless) -> pure given
  (RetainedRuntime, CardBacked cardImage) -> do
    started <- getMonotonicTime
    key <- cardAttachRouteKey given python runtime installedMedia adjustmentNand cardImage
    ended <- getMonotonicTime
    putStrLn ("checkpoint key: " ++ routeKeyHex key ++ " in " ++ seconds (ended - started))
    pure given { cardAttachKey = Just key }

-- | Boots or restores the session's attach checkpoint, saving it on a cold boot, then stops.
warmAttachCheckpoint :: E2ESettings -> FilePath -> FilePath -> FilePath -> FilePath -> IO ()
warmAttachCheckpoint given python installedMedia adjustmentNand output = do
  hSetBuffering stdout LineBuffering
  runtime <- getCurrentDirectory
  settings <- keyedSettings given python runtime installedMedia adjustmentNand
  (executable, arguments) <- sessionBridge settings python installedMedia adjustmentNand (bootMode settings) output
  started <- getMonotonicTime
  bracket (startEmulator executable runtime arguments (\_ -> pure (Nothing, ()))) (\(emulator, _, _) -> stopEmulator emulator) $
    \(_, start, ()) -> do
      ended <- getMonotonicTime
      putStrLn ("attach checkpoint: " ++ show (bootStart start) ++ " in " ++ seconds (ended - started))
      C.putStrLn (BL.toStrict (encode (startRecord start)))

-- | Runs the whole trajectory in one process, or with `segmentSelection` each named segment (and
-- what it requires) in its own process from the install checkpoint, under RECEIPT/segment-NAME.
runRamPushE2E :: E2ESettings -> FilePath -> FilePath -> FilePath -> FilePath -> FilePath -> FilePath -> IO ()
runRamPushE2E given python workspace trajectoryFile installedMedia adjustmentNand receipt = do
  hSetBuffering stdout LineBuffering
  trajectories <- B.readFile trajectoryFile
    >>= either (ioError . userError . ("invalid RAM-push e2e trajectories: " ++)) pure . decodeTrajectories
  let settleCaps = [ () | segment <- beforeUnload trajectories ++ afterUnload trajectories
                        , step <- segmentSteps segment, isJust (keyTimingSettle (stepTiming step)) ]
  case emulationRuntime given of
    RetainedRuntime | not (null settleCaps) ->
      ioError (userError "settle_instructions is FujiMod press timing; the retained bridge takes no settle cap")
    _ -> pure ()
  exists <- doesDirectoryExist receipt
  when exists $ do
    entries <- listDirectory receipt
    require (null entries) "RAM-push receipt directory must be fresh or empty"
    removeDirectory receipt
  runtime <- getCurrentDirectory
  settings <- keyedSettings given python runtime installedMedia adjustmentNand
  -- Host RAM-push operations resolve workspace-relative builder paths.
  setCurrentDirectory workspace
  let session = case pushMode settings of
        RamPush -> runSession settings python runtime installedMedia adjustmentNand
        FlashCandidate -> runFlashSession settings python runtime installedMedia adjustmentNand
      selection = case (segmentSelection settings, pushMode settings) of
        (Just names, _) -> Just names
        (Nothing, FlashCandidate) -> Just (phaseNames trajectories)
        (Nothing, RamPush) -> Nothing
  case selection of
    Nothing -> session (Plan "all" (beforeUnload trajectories) (afterUnload trajectories) True False (trajectoryTraceSites trajectories)) receipt
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

runSession :: E2ESettings -> FilePath -> FilePath -> FilePath -> FilePath -> Plan -> FilePath -> IO ()
runSession settings python runtime installedMedia adjustmentNand plan e2e = do
  phases <- newIORef []
  let timed :: T.Text -> IO a -> IO a
      timed label action = do
        started <- getMonotonicTime
        result <- action
        ended <- getMonotonicTime
        modifyIORef' phases (++ [object ["name" .= label, "seconds" .= (ended - started)]])
        putStrLn ("phase " ++ T.unpack label ++ ": " ++ seconds (ended - started))
        pure result
      selected = e2ePayload settings
      relinkAt base = case relinkMode settings of
        ReviewedRelink -> withPinnedReviewedBuild (payloadContract selected) selected base
        UnreviewedRelink policy -> withUnreviewedBuild selected policy base
      pushWith = case relinkMode settings of
        ReviewedRelink -> runRamPush (payloadContract selected)
        UnreviewedRelink _ -> runUnreviewedRamPush
      unloadWith = case relinkMode settings of
        ReviewedRelink -> runRamUnload (payloadContract selected)
        UnreviewedRelink _ -> runUnreviewedRamUnload
      fetchSites = if carriesClassicChrome selected then classicChromeFetchSites else []
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
  (executable, arguments) <- sessionBridge settings python installedMedia adjustmentNand (bootMode settings) e2e
  published <- bracket (timed "emulator-start" (startEmulator executable runtime arguments choose)) (\(emulator, _, _) -> stopEmulator emulator) $ \(emulator, start, installed) -> do
    putStrLn ("emulator start: " ++ show (bootStart start) ++ "; relink: " ++ T.unpack (relinkModeText (relinkMode settings))
      ++ "; payload: " ++ T.unpack (payloadText selected))
    let api = emulatorUsbApi emulator
        allocationDirectory = e2e </> "alloc"
        relinkDirectory = e2e </> "relinked"
        pushDirectory = e2e </> "push"
        unloadDirectory = e2e </> "unload"
        allocationBytes = payloadAllocationSize selected
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
    (retained, breakBefore, breakAfter, allowlist, fetchStart) <- case (installed, restoredHost) of
      (Just (_, relinked), Just host) -> do
        let path = allocationDirectory </> "ram-alloc-state.json"
        writePrivateBytes path (hostAllocationState host)
        retained <- readRetainedAllocation path
        allowlist <- writeRelinked relinked
        pure (retained, hostBreakBefore host, hostBreakAfter host, allowlist, Nothing)
      _ -> do
        fetchStart <- if null fetchSites then pure Nothing else do
          emulatorTraceSites emulator fetchSites
          Just . fst <$> emulatorTraceHits emulator 0
        breakBefore <- heapBreak
        allocationPath <- timed "ram-alloc" (stagedSession allocationDirectory (runRamAlloc allocationBytes realClock api)) >>= \outcome -> case outcome of
          RamAllocated path -> pure (allocationDirectory </> takeFileName path)
          other -> ioError (userError ("ram-alloc failed: " ++ show other))
        retained <- readRetainedAllocation allocationPath
        breakAfter <- heapBreak
        allowlist <- timed "relink" (relinkFiles (allocBase (retainedAllocation retained))) >>= writeRelinked
        pure (retained, breakBefore, breakAfter, allowlist, fetchStart)
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
                 , (_, RuntimeSymbol name _, _) <- segmentReads segment])
    let readAddress (FixedAddress value) = pure value
        readAddress (RuntimeSymbol name offset) =
          maybe (ioError (userError ("unresolved runtime symbol: " ++ name))) (pure . (+ offset)) (lookup name readSymbols)
    let readMailbox = mailboxReport <$> emulatorReadMemory emulator mailbox 104
        stop phase message = do
          report <- readMailbox
          writePrivateBytes (e2e </> "e2e-failure.json") (BL.toStrict (encode (object
            [ "phase" .= String phase, "message" .= message, "mailbox" .= report
            , "relink" .= relinkModeText (relinkMode settings)
            , "heap_break_before" .= breakBefore, "heap_break_after" .= breakAfter ])) <> "\n")
          ioError (userError (show phase ++ " failed: " ++ message))
    let entries = manifestEntries manifest
    emulatorTraceSites emulator $
      [ (unAddress (installEntry entries), "uploaded-installer")
      , (unAddress (rollbackEntry entries), "uploaded-rollback")
      , (unAddress (statusEntry entries), "uploaded-status")
      , (rendezvous, "rendezvous-locked")
      , (cpu0Exit, "cpu0-window-exit")
      , (cpu1Exit, "cpu1-window-exit")
      , (menuMessageDrawer, menuMessageSite)
      ] ++ [(d3HookEntry, d3HookSite) | payloadKind selected == RecipeBankD3] ++ fetchSites ++ declaredSites plan
    privateDirectory pushDirectory
    (statePath, installMailbox) <- case restoredHost of
      Just host -> do
        let path = pushDirectory </> "ram-push-state.json"
        writePrivateBytes path (hostPushState host)
        pure (path, hostInstallMailbox host)
      Nothing -> do
        statePath <- timed "ram-push" (stagedSession pushDirectory (pushWith retained manifest realClock api)) >>= \outcome -> case outcome of
          RamPushInstalled path -> pure (pushDirectory </> takeFileName path)
          other -> stop "ram-push" (show other)
        installMailbox <- readMailbox
        forM_ fetchStart $ \from -> do
          (_, hits) <- emulatorTraceHits emulator from
          let fetched = [hit | hit <- hits, traceSite hit `elem` map snd fetchSites]
          writePrivateBytes (e2e </> "e2e-fetch-watch.json") (BL.toStrict (encode (object
            [ "scope" .= String "Classic Chrome entry words and hook sites, attach checkpoint to the end of ram-push"
            , "sites" .= object [K.fromString label .= hexAddress address | (address, label) <- fetchSites]
            , "hits" .= [object ["site" .= traceSite hit, "r0" .= hexAddress (traceR0 hit)] | hit <- fetched]
            , "fetched" .= not (null fetched) ])) <> "\n")
          putStrLn ("fetch watch: " ++ show (length fetched) ++ " entries into Classic Chrome sites from attach to install")
        host <- InstallHost <$> B.readFile allocationPath <*> B.readFile statePath
          <*> pure installMailbox <*> pure breakBefore <*> pure breakAfter
        saved <- timed "install-checkpoint" (emulatorSaveInstallCheckpoint emulator base
          (manifestDigest manifest) (runtimeDigest manifest) (installHostValue host))
        putStrLn ("install checkpoint: " ++ C.unpack (BL.toStrict (encode saved)))
        pure (statePath, installMailbox)
    state <- readRamPushState statePath
    runner <- newSegmentRunner emulator e2e (\label action -> timed label action) readAddress (\phase message -> stop phase message) plan
    let segments = runSegments runner
    capturedBefore <- segments (planBefore plan)
    unloadMailbox <- if planUnload plan
      then do
        privateDirectory unloadDirectory
        unload <- timed "ram-unload" (stagedSession unloadDirectory (unloadWith state realClock api))
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
          , "payload" .= payloadText selected
          , "relink" .= object ["mode" .= relinkModeText (relinkMode settings),
                                "source_identity" .= object [K.fromString name .= digest | (name, digest) <- sourceIdentity manifest],
                                "allowlist" .= allowlist]
          , "emulator_start" .= startRecord start
          , "attach_hold" .= attachHoldText (attachHold settings)
          , "phases" .= (phaseTimes :: [Value])
          , "allocation" .= object ["base" .= hexAddress base, "bytes" .= allocationBytes,
                                     "cpu0" .= hexAddress (allocCpu0 allocation),
                                     "heap_break_before" .= breakBefore, "heap_break_after" .= breakAfter]
          , "runtime" .= object ["bytes" .= allocationSize manifest,
                                  "publications" .= length (manifestPublications manifest),
                                  "manifest" .= manifestPath manifest]
          , "states" .= object ["allocation" .= allocationPath, "installed" .= statePath]
          , "card" .= cardRecord (emulationCardMode settings)
          , "menu_receipts" .= catMaybes (capturedBefore ++ capturedAfter)
          , "trajectories" .= map (\name -> object ["name" .= name, "result" .= String "matched"])
              (nub (map segmentPhase (filter (not . segmentObserve) (planBefore plan ++ planAfter plan))))
          , "observations" .= (observed :: [Value])
          , "mailbox" .= object ["install" .= installMailbox, "rollback" .= unloadMailbox,
                                 "address" .= hexAddress mailbox]
          , "unload" .= (if planUnload plan
              then Just (String (T.pack (show (length (manifestPublications manifest)) ++ " old words verified; allocation retained until power-off")))
              else Nothing)
          ]
    writePrivateBytes (e2e </> "e2e-result.json") (BL.toStrict (encode result) <> "\n")
    pure (manifestPublications manifest)
  when (planUnload plan && carriesClassicChrome selected) $
    timed "cold-restart-read" (coldRestartRead settings python runtime installedMedia adjustmentNand published e2e)

-- | A cold boot of the same inputs, then every publication word read against its old word.
coldRestartRead :: E2ESettings -> FilePath -> FilePath -> FilePath -> FilePath -> [Publication] -> FilePath -> IO ()
coldRestartRead settings python runtime installedMedia adjustmentNand publications e2e = do
  let directory = e2e </> "cold-restart"
  (executable, arguments) <- sessionBridge settings python installedMedia adjustmentNand FreshBoot directory
  bracket (startEmulator executable runtime arguments (\_ -> pure (Nothing, ()))) (\(emulator, _, _) -> stopEmulator emulator) $ \(emulator, start, ()) -> do
    words' <- forM publications $ \publication -> do
      bytes <- emulatorReadMemory emulator (unAddress (publicationAddress publication)) 4
      let current = leWord bytes 0
      pure (publication, current)
    let mismatched = [p | (p, current) <- words', current /= unWordValue (publicationOld p)]
    writePrivateBytes (directory </> "e2e-cold-restart.json") (BL.toStrict (encode (object
      [ "scope" .= String "fresh boot of the same DAT, NAND and card after ram-unload; every publication word read"
      , "card" .= cardRecord (emulationCardMode settings)
      , "emulator_start" .= startRecord start
      , "words" .= length words'
      , "mismatched" .= [hexAddress (unAddress (publicationAddress p)) | p <- mismatched]
      , "result" .= String (if null mismatched then "all old" else "mismatch") ])) <> "\n")
    putStrLn ("cold restart: " ++ show (length words') ++ " publication words read, " ++ show (length mismatched) ++ " differ from old")
    require (null mismatched) "cold restart read found publication words that are not old"

-- | What running segments needs from its session; hits and observations accumulate across calls.
data SegmentRunner = SegmentRunner
  { runnerEmulator :: Emulator
  , runnerReceipt :: FilePath
  , runnerTimed :: T.Text -> IO (Maybe Value) -> IO (Maybe Value)
  , runnerReadAddress :: ReadAddress -> IO Word32
  , runnerStop :: T.Text -> String -> IO ()
  , runnerTraceLabels :: [String]
  , runnerHits :: IORef Int
  , runnerObservations :: IORef [Value]
  }

newSegmentRunner :: Emulator -> FilePath -> (T.Text -> IO (Maybe Value) -> IO (Maybe Value))
  -> (ReadAddress -> IO Word32) -> (T.Text -> String -> IO ()) -> Plan -> IO SegmentRunner
newSegmentRunner emulator receipt timedAction readAddress stopAction plan = do
  (recorded, _) <- emulatorTraceHits emulator 0
  SegmentRunner emulator receipt timedAction readAddress stopAction (map traceSiteLabel (planTraceSites plan))
    <$> newIORef recorded <*> newIORef []

-- | The plan's trajectory-declared sites in the bridge's form.
declaredSites :: Plan -> [(Word32, String)]
declaredSites plan = [(traceSiteAddress site, traceSiteLabel site) | site <- planTraceSites plan]

traceHitValue :: TraceHit -> Value
traceHitValue hit = object
  [ "site" .= traceSite hit, "core" .= traceCore hit, "instructions" .= traceInstructions hit
  , "r0" .= hexAddress (traceR0 hit), "r1" .= hexAddress (traceR1 hit)
  , "r2" .= hexAddress (traceR2 hit), "r3" .= hexAddress (traceR3 hit) ]

-- | Runs each segment and returns its capture receipt.
runSegments :: SegmentRunner -> [Segment] -> IO [Maybe Value]
runSegments runner = mapM $ \segment -> runnerTimed runner ("segment " <> segmentPhase segment) $ do
  let emulator = runnerEmulator runner
      stop = runnerStop runner
  context <- either (ioError . userError) pure (releaseSegment (segmentPhase segment))
  emulatorReleaseContext emulator (Just context)
  (before, _) <- readIORef (runnerHits runner) >>= emulatorTraceHits emulator
  forM_ (segmentModeDial segment) $ \position -> emulatorModeDial emulator position
  observed <- if segmentObserve segment
    then observe emulator (segmentKeyPulse segment) (segmentSteps segment)
    else [] <$ (presses emulator (segmentKeyPulse segment) (segmentSteps segment) `catch` \(failure :: IOException) ->
      stop (segmentPhase segment) (displayException failure))
  captured <- traverse (emulatorCapture emulator) (segmentCapture segment)
  windows <- mapM (\(label, location, count) -> do
      address <- runnerReadAddress runner location
      bytes <- emulatorReadMemory emulator address count
      pure (object ["label" .= label, "address" .= hexAddress address, "bytes" .= hexBytes bytes]))
    (segmentReads segment)
  film <- traverse (\expected -> do
      -- Ordinary image byte 0x8a stores the film code in bits 4-7.
      bytes <- emulatorReadMemory emulator 0x0096ccee 1
      require (B.length bytes == 1) "film identity read must contain one byte"
      pure (expected, fromIntegral (B.head bytes `shiftR` 4) :: Word32))
    (segmentFilmRow segment)
  (after, hits) <- emulatorTraceHits emulator before
  writeIORef (runnerHits runner) after
  let drawn = sort (nub [traceR0 hit | hit <- hits, traceSite hit == menuMessageSite])
      hookRows = [traceR0 hit | hit <- hits, traceSite hit == d3HookSite]
      ccHits = [traceSite hit | hit <- hits, traceSite hit `elem` map snd classicChromeFetchSites]
      declaredHits = [hit | hit <- hits, traceSite hit `elem` runnerTraceLabels runner]
  when (segmentObserve segment || not (null (segmentReads segment)) || not (null hookRows) || not (null ccHits)
        || not (null declaredHits) || not (null film)) $ do
    previous <- readIORef (runnerObservations runner)
    let current = object ([ "phase" .= segmentPhase segment, "capture" .= segmentCapture segment
                          , "steps" .= observed, "reads" .= windows
                          , "d3_hook_r0" .= map hexAddress hookRows
                          , "classic_chrome_site_entries" .= ccHits
                          , "menu_messages" .= map hexAddress drawn
                          , "trace_hits" .= map traceHitValue declaredHits ]
                          ++ [ "film_identity" .= object
                               ["expected_row" .= fromEnum expected, "expected_stored_code" .= filmStoredCode expected,
                                "stored_code" .= actual]
                             | (expected, actual) <- maybe [] pure film ])
    writeIORef (runnerObservations runner) (previous ++ [current])
    writePrivateBytes (runnerReceipt runner </> ("e2e-observation-" ++ show (length previous) ++ ".json"))
      (BL.toStrict (encode current) <> "\n")
  forM_ film $ \(expected, actual) ->
    unless (actual == filmStoredCode expected) $
      stop (segmentPhase segment) ("film row " ++ show (fromEnum expected)
        ++ " requires stored code " ++ show (filmStoredCode expected) ++ ", got " ++ show actual)
  unless (segmentObserve segment || drawn == sort (nub (segmentMessages segment))) $
    stop (segmentPhase segment) ("menu messages drawn " ++ show (map hexAddress drawn)
      ++ ", expected " ++ show (map hexAddress (segmentMessages segment)))
  emulatorReleaseContext emulator Nothing
  pure captured

-- | The cardless bridge from this binary's runfiles; //emulation:xm1-viewer lists it as data.
runfilesCardlessBridge :: IO FilePath
runfilesCardlessBridge = do
  executable <- getExecutablePath
  let path = (executable ++ ".runfiles") </> "_main" </> "emulation" </> "cardless-ram-push-bridge"
  present <- doesFileExist path
  require present ("cardless RAM-push bridge is missing from this binary's runfiles: " ++ path)
  pure path

-- | One flash-candidate process: boot or restore the attach checkpoint keyed by the candidate DAT,
-- then run the plan's segments; the candidate's loader has already published the bank.
runFlashSession :: E2ESettings -> FilePath -> FilePath -> FilePath -> FilePath -> Plan -> FilePath -> IO ()
runFlashSession settings python runtime installedMedia adjustmentNand plan e2e = do
  require (emulationCardMode settings /= Cardless) "--cardless belongs to the RAM push, not --flash-candidate"
  phases <- newIORef []
  let timed :: T.Text -> IO a -> IO a
      timed label action = do
        started <- getMonotonicTime
        result <- action
        ended <- getMonotonicTime
        modifyIORef' phases (++ [object ["name" .= label, "seconds" .= (ended - started)]])
        putStrLn ("phase " ++ T.unpack label ++ ": " ++ seconds (ended - started))
        pure result
      noInstall _ = pure (Nothing, ())
  (executable, arguments) <- sessionBridge settings python installedMedia adjustmentNand (bootMode settings) e2e
  bracket (timed "emulator-start" (startEmulator executable runtime arguments noInstall)) (\(emulator, _, _) -> stopEmulator emulator) $ \(emulator, start, ()) -> do
    putStrLn ("emulator start: " ++ show (bootStart start) ++ "; flash candidate")
    setFileMode e2e 0o700
    ensurePrivateDirectory e2e
    emulatorTraceSites emulator ([(menuMessageDrawer, menuMessageSite), (d3HookEntry, d3HookSite)] ++ declaredSites plan)
    let readAddress (FixedAddress value) = pure value
        readAddress (RuntimeSymbol name offset) = do
          symbols <- flashCandidateSymbols (firmwareSource settings)
          either (ioError . userError) (pure . (+ offset)) (resolveSymbol symbols name)
        stop phase message = do
          writePrivateBytes (e2e </> "e2e-failure.json") (BL.toStrict (encode (object
            [ "phase" .= String phase, "message" .= message ])) <> "\n")
          ioError (userError (show phase ++ " failed: " ++ message))
    runner <- newSegmentRunner emulator e2e (\label action -> timed label action) readAddress stop plan
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
