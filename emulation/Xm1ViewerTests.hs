{-# LANGUAGE OverloadedStrings #-}
module Main (main) where

import Control.Exception (IOException, try)
import Control.Monad (forM_, unless)
import qualified Xm1Viewer as Viewer
import Xm1RamPushE2E (ModalDiagnostic (..), ModalLifetimeGuard (..), ModalOwnedPackage (..), loadModalOwnedPackage)
import qualified Xm1RamPushE2E as E2E
import FujiAF.RamPush (payloadPublicationCount)
import Emulation.LiveSettings (LiveSettingChange (..), liveSettingChange, liveSettings)
import Emulation.Sha256 (parseSha256Hex, sha256, sha256Hex)
import Data.Aeson (Value (..), object, toJSON, (.=))
import qualified Data.Aeson.KeyMap as KM
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as BC
import qualified Data.ByteString.Builder as BB
import qualified Data.ByteString.Lazy as BL
import Data.List (partition)
import Numeric (showHex)
import qualified Data.Text as T
import System.Directory (createDirectoryIfMissing, getTemporaryDirectory)
import System.Environment (lookupEnv)
import System.FilePath ((</>))

check :: String -> Bool -> IO ()
check label accepted = unless accepted (ioError (userError label))

acceptedOptions :: [String] -> IO Viewer.Options
acceptedOptions arguments = do
  command <- Viewer.parseCommand arguments
  case command of
    Viewer.Launch options -> pure options
    _ -> ioError (userError "expected launch configuration")

refuses :: String -> [String] -> IO ()
refuses label arguments = do
  result <- try (Viewer.parseCommand arguments) :: IO (Either IOException Viewer.Command)
  check label (either (const True) (const False) result)

main :: IO ()
main = do
  let keeper = ["--property-allocation-keeper", "--cardless", "--fujimod", "--fresh-boot"
        , "--allocation-image4", "image4.bin", "--allocation-contract", "composition.json"]
  held <- acceptedOptions keeper
  check "allocation keeper is separate from normal E2E"
    (Viewer.propertyAllocationKeeperOption held && not (Viewer.ramPushE2EOption held))
  forM_ ["--cardless", "--fujimod", "--fresh-boot"] $ \required ->
    refuses ("allocation keeper requires " ++ required) (filter (/= required) keeper)
  refuses "allocation keeper requires source sidecars" (take 4 keeper)
  refuses "source sidecars do not activate a keeper" (drop 4 keeper)
  forM_
    [ ["--ram-push-e2e"], ["--ff80-rehearsal"], ["--unreviewed-relink"]
    , ["--warm-checkpoint"], ["--flash-candidate"], ["--property-allocation-keeper"]
    , ["--trajectories", "route.json"], ["--segments", "all"], ["--sd-card-image", "card.img"]
    , ["--classic-chrome"], ["--experiment", "hello"], ["--attach-hold-q"]
    , ["--menu-observe", "observe.json"], ["--allocation-size", "253048"]
    ] $ \extra -> refuses ("allocation keeper rejects " ++ unwords extra) (keeper ++ extra)
  let sessionText = replicate 64 'a'
      session = either error id (E2E.keeperSessionId sessionText)
      transition = E2E.keeperTransition session
  check "keeper status retains the holding state"
    (transition E2E.KeeperHolding ("status " ++ sessionText) == Right (E2E.KeeperStatus, E2E.KeeperHolding))
  check "keeper readback retains the holding state"
    (transition E2E.KeeperHolding ("readback " ++ sessionText) == Right (E2E.KeeperReadback, E2E.KeeperHolding))
  check "authenticated stop ends the keeper"
    (transition E2E.KeeperHolding ("stop " ++ sessionText) == Right (E2E.KeeperStop, E2E.KeeperStopped))
  forM_ ["status", "stop " ++ replicate 64 'b', "install " ++ sessionText, "status " ++ sessionText ++ " extra", replicate 161 'a'] $ \line ->
    check "keeper rejects malformed, wrong-session or publication requests" (case transition E2E.KeeperHolding line of Left _ -> True; _ -> False)
  forM_ ["status ", "readback ", "stop "] $ \command ->
    check "stopped keeper refuses every command"
      (case transition E2E.KeeperStopped (command ++ sessionText) of Left _ -> True; _ -> False)
  let digest = either error id . parseSha256Hex
      dat = digest "cd2f3d7d187ab72151abdb325891bac5b5a3257db8640c6b0c0e836d29b37626"
      image4 = digest "40d7fe9afdcd067a4441df8a079cef1fb4ee9e18a8240d9abd492a6db545dbdf"
      contract = digest "1b839ebdb8f7448c211014a619bb004832b5cecc3eac673faf045f8226d841eb"
      other = digest (replicate 64 '0')
  check "keeper accepts the immutable admitted source triple"
    (E2E.checkPropertyAllocationSource dat image4 contract == Right ())
  forM_ [(other,image4,contract), (dat,other,contract), (dat,image4,other)] $ \(d,i,c) ->
    check "keeper rejects a changed DAT, image4 or composition identity"
      (case E2E.checkPropertyAllocationSource d i c of Left _ -> True; _ -> False)
  let stock = ["--ram-push-e2e", "--cardless", "--fujimod", "--unreviewed-relink"]
      guard = "--modal-lifetime-guard"
      scoped = "--ram-push-modal-scoped"
  defaults <- acceptedOptions []
  check "default viewer has no lifetime guard" (Viewer.modalLifetimeGuardOption defaults == NoModalLifetimeGuard)
  unguarded <- acceptedOptions stock
  check "default 223 invocation remains unguarded" (Viewer.modalLifetimeGuardOption unguarded == NoModalLifetimeGuard)
  control <- acceptedOptions (stock ++ [guard])
  check "stock control selects continuous lifetime guard" (Viewer.modalLifetimeGuardOption control == ContinuousModalLifetimeGuard)
  forM_ [stock ++ [scoped], scoped : stock] $ \arguments -> do
    candidate <- acceptedOptions arguments
    check "scoped 224 needs no explicit lifetime flag" (Viewer.modalLifetimeGuardOption candidate == NoModalLifetimeGuard)
    check "scoped 224 retains its payload selection" (Viewer.ramPayloadOption candidate == Viewer.BankModal75Choice)
  forM_ [stock ++ [scoped, guard], guard : scoped : stock] $ \arguments -> do
    candidate <- acceptedOptions arguments
    check "explicit guard agrees with scoped selection" (Viewer.modalLifetimeGuardOption candidate == ContinuousModalLifetimeGuard)
  _ <- acceptedOptions (stock ++ [guard, "--fresh-boot", "--trajectories", "selected-route.json"])
  let af = ["--af-ownership-plan", "af-plan.json"]
      route = ["--trajectories", "original-route.json"]
  afDiagnostic <- acceptedOptions (stock ++ [guard, "--fresh-boot"] ++ route ++ af)
  check "AF plan is forwarded without opening it during CLI parsing" (Viewer.afOwnershipPlanOption afDiagnostic == Just "af-plan.json")
  check "AF diagnostic stays opt-in" (Viewer.afOwnershipPlanOption defaults == Nothing)
  check "AF control defaults off" (not (Viewer.afOwnershipControlOption defaults))
  check "AF enabled arm does not select control" (not (Viewer.afOwnershipControlOption afDiagnostic))
  forM_ [stock ++ [guard, "--fresh-boot", "--af-ownership-control"] ++ route ++ af,
         "--af-ownership-control" : af ++ route ++ [guard, "--fresh-boot"] ++ stock] $ \arguments -> do
    afControl <- acceptedOptions arguments
    check "AF control preserves the exact plan" (Viewer.afOwnershipPlanOption afControl == Just "af-plan.json")
    check "AF control selects the disabled arm" (Viewer.afOwnershipControlOption afControl)
  refuses "AF enabled refuses checkpoint boot" (stock ++ [guard] ++ route ++ af)
  refuses "AF control requires plan" (stock ++ [guard, "--fresh-boot", "--af-ownership-control"] ++ route)
  refuses "AF control requires fresh boot" (stock ++ [guard, "--af-ownership-control"] ++ route ++ af)
  refuses "AF control requires explicit continuous guard" (stock ++ [scoped, "--fresh-boot", "--af-ownership-control"] ++ route ++ af)
  refuses "AF control flag refuses duplicates" (stock ++ [guard, "--fresh-boot", "--af-ownership-control", "--af-ownership-control"] ++ route ++ af)
  refuses "AF control is a closed flag" (stock ++ [guard, "--fresh-boot", "--af-ownership-control=true"] ++ route ++ af)
  forM_ [False, True] $ \disabled -> do
    let settings = E2E.E2ESettings
          { E2E.relinkMode = E2E.UnreviewedRelink E2E.EnforceAllowlist
          , E2E.bootMode = E2E.FreshBoot
          , E2E.checkpointRoot = "checkpoints"
          , E2E.segmentSelection = Nothing
          , E2E.pushMode = E2E.RamPush
          , E2E.e2ePayload = E2E.bankD3Payload
          , E2E.attachHold = E2E.NoAttachHold
          , E2E.flashUiConversion = E2E.StrictUiConversion
          , E2E.emulationCardMode = E2E.Cardless
          , E2E.fixedSceneFrame = Nothing
          , E2E.cardAttachKey = Nothing
          , E2E.emulationRuntime = E2E.FujiModRuntime (E2E.FujiModBridge "bridge" "run.json" "boot.bin")
          , E2E.firmwareSource = "firmware.dat"
          , E2E.experimentReads = []
          , E2E.modalDiagnostic = NoModalDiagnostic
          , E2E.modalLifetimeGuard = ContinuousModalLifetimeGuard
          , E2E.modalOwnedPackage = Nothing
          , E2E.prelinkedModalBuild = Nothing
          , E2E.afOwnershipPlanFile = Just "af-plan.json"
          , E2E.afOwnershipControl = disabled
          }
    (executable, arguments) <- E2E.sessionBridge settings "python" "installed.dat" "adjustment.bin" E2E.FreshBoot "receipt"
    check "both AF arms select the actual bridge" (executable == "bridge")
    check "both AF arms request a fresh reset proof" ("--fresh-boot" `elem` arguments && "--af-prestate-proof" `elem` arguments)
    (_, ordinaryArguments) <- E2E.sessionBridge (settings { E2E.afOwnershipPlanFile = Nothing, E2E.afOwnershipControl = False }) "python" "installed.dat" "adjustment.bin" E2E.FreshBoot "receipt"
    check "ordinary fresh boots do not request AF proof" ("--af-prestate-proof" `notElem` ordinaryArguments)
    checkpointAttempt <- try (E2E.sessionBridge settings "python" "installed.dat" "adjustment.bin" E2E.CheckpointedBoot "receipt")
      :: IO (Either IOException (FilePath, [String]))
    check "both AF bridges refuse checkpoint boot" (either (const True) (const False) checkpointAttempt)
  refuses "AF refuses absent continuous guard" (stock ++ ["--fresh-boot"] ++ route ++ af)
  refuses "AF refuses absent explicit trajectory" (stock ++ [guard, "--fresh-boot"] ++ af)
  refuses "AF refuses duplicate plan" (stock ++ [guard, "--fresh-boot"] ++ route ++ af ++ af)
  refuses "AF refuses no-install" (stock ++ [guard, "--fresh-boot", "--flash-candidate"] ++ route ++ af)
  refuses "AF refuses segmented lifetime" (stock ++ [guard, "--fresh-boot", "--segments", "down"] ++ route ++ af)
  refuses "AF refuses warm-only" (stock ++ [guard, "--fresh-boot", "--warm-checkpoint"] ++ route ++ af)
  let runDigest = sha256 "selected run document"
      installedDigest = sha256 "installed DAT"
      digest = sha256Hex runDigest
      resetIdentity = object
        [ "schema" .= (3 :: Int), "model" .= digest, "run_document" .= digest
        , "installed_dat" .= sha256Hex installedDigest, "boot_rom" .= digest
        , "adjustment_nand" .= digest, "record_nand" .= Null, "committed_nand" .= Null
        , "source_kind" .= String "initial-seeded", "card_sha256" .= Null
        , "procedure" .= sha256Hex (sha256 "initialized-adjusted-card-state-v2:af-hit-restored") ]
      rootComponent = object
        ["path" .= String "$", "kind" .= String "checkpoint", "checkpoint_kind" .= String "emulator-session-v2",
         "logical_bytes" .= (51 :: Int), "sha256" .= digest]
      headerComponent = object
        ["path" .= String "$/header", "kind" .= String "header", "checkpoint_kind" .= String "emulator-session-v2",
         "logical_bytes" .= (0 :: Int), "sha256" .= digest]
      componentEntries = [rootComponent, headerComponent]
      components = object
        [ "schema" .= (1 :: Int), "algorithm" .= String "sha256-hierarchical-v1", "root_sha256" .= digest
        , "chunk_bytes" .= (1048576 :: Int), "entries" .= bindArchive componentEntries ]
      fingerprint counts = object
        [ "schema" .= (1 :: Int), "status" .= String "complete", "machine_sha256" .= digest
        , "counts" .= (counts :: [Int]), "now" .= (0 :: Int), "logical_bytes" .= (84 + archiveBoardBytes)
        , "included_state" .= String "session-v2/board-v1-complete-machine"
        , "excluded_host_fields" .= map String ["board.entries", "board.hooks", "board.observations"]
        , "components" .= components, "exact_state" .= exactArchive counts ]
      resetProof counts = object
        [ "schema" .= (1 :: Int), "status" .= String "complete", "identity" .= resetIdentity
        , "key" .= digest, "snapshot_sha256" .= digest, "fingerprint" .= fingerprint counts
        , "live_before_reset" .= fingerprint [0, 0]
        , "reset_cache" .= object ["lookup" .= String "hit", "restore" .= String "restored"] ]
      replaceField name value (Object fields) = Object (KM.insert name value fields)
      replaceField _ _ value = value
      deleteField name (Object fields) = Object (KM.delete name fields)
      deleteField _ value = value
      binding = E2E.AfArchiveBinding runDigest runDigest
      validate proof = do
        E2E.validateAfResetProof runDigest installedDigest proof
        case proof of
          Object fields -> forM_ ["fingerprint", "live_before_reset"] $ \name ->
            maybe (Left "missing bound fingerprint") (E2E.validateAfArchiveBinding binding) (KM.lookup name fields)
          _ -> Left "missing reset proof"
      rejected label proof = check label (either (const True) (const False) (validate proof))
      primaryBlob = archiveBlob (0 :: Int) cpuArchiveBytes
      secondaryBlob = archiveBlob (1 :: Int) cpuArchiveBytes
      exclusiveBlob = archiveBlob (2 :: Int) exclusiveArchiveBytes
      blobRows = [primaryBlob, secondaryBlob, exclusiveBlob]
      exactArchive counts = object
        ["schema" .= (1 :: Int), "checkpoint_model_id" .= digest
        , "counts" .= (counts :: [Int]), "now" .= (0 :: Int)
        , "model_definition_sha256" .= sha256Hex (sha256 "null"), "executable_sha256" .= digest
        , "cpu" .= [primaryBlob, secondaryBlob], "exclusive" .= exclusiveBlob]
      entry path kind name bytes = object
        ["path" .= String path, "kind" .= String kind, "checkpoint_kind" .= String name,
         "logical_bytes" .= (bytes :: Int), "sha256" .= digest]
      rootEntry bytes = entry "$" "checkpoint" "emulator-session-v2" bytes
      rootHeader bytes = entry "$/header" "header" "emulator-session-v2" bytes
      chunk path bytes offset = replaceField "offset" (toJSON (offset :: Int))
        (entry path "chunk" "emulator-session-v2" bytes)
      node path bytes = entry path "checkpoint" "device-v1" bytes
      nodeHeader path bytes = entry path "header" "device-v1" bytes
      nestedEntries =
        [rootEntry 1049138, rootHeader 0,
         entry "$/blobs/0" "blob" "emulator-session-v2" 1048579,
         chunk "$/blobs/0/chunks/0" 1048576 0, chunk "$/blobs/0/chunks/1" 3 1048576,
         entry "$/blobs/1" "blob" "emulator-session-v2" 0]
        ++ concat
          [ let path = T.pack ("$/children/" ++ show number)
             in [node path (if number == 0 then 82 else 41), nodeHeader (path <> "/header") 0]
               ++ (if number == 0 then [node (path <> "/children/0") 41, nodeHeader (path <> "/children/0/header") 0] else [])
          | number <- [0 .. 10 :: Int] ]
      withEntries bytes entries = replaceField "fingerprint"
        (replaceField "logical_bytes" (toJSON (bytes + archiveBoardBytes))
          (replaceField "components" (replaceField "entries" (toJSON (bindArchive entries)) components) (fingerprint [0, 0])))
        (replaceField "live_before_reset" validFingerprint (resetProof [0, 0]))
      validFingerprint = fingerprint [0, 0]
      bindArchive entries =
        let shifted = map shiftChild entries
            (rootParts, childParts) = partition (not . isChild) shifted
         in map (\value -> if atPath "$" value then addBytes archiveBoardBytes value else value) rootParts
            ++ archiveBoardEntries ++ childParts
      isChild (Object fields) = case KM.lookup "path" fields of
        Just (String path) -> "$/children/" `T.isPrefixOf` path
        _ -> False
      isChild _ = False
      shiftChild value@(Object fields) = case KM.lookup "path" fields of
        Just (String path) | ["$", "children", number] <- take 3 (T.splitOn "/" path) ->
          case reads (T.unpack number) :: [(Int, String)] of
            [(index, "")] | T.pack (show index) == number -> replaceField "path" (String (T.intercalate "/" (["$", "children", T.pack (show (index + 1))] ++ drop 3 (T.splitOn "/" path)))) value
            _ -> value
        _ -> value
      shiftChild value = value
      addBytes extra value@(Object fields) = case KM.lookup "logical_bytes" fields of
        Just (Number count) -> replaceField "logical_bytes" (Number (count + fromIntegral extra)) value
        _ -> value
      addBytes _ value = value
      atPath path value = case value of
        Object fields -> KM.lookup "path" fields == Just (String path)
        _ -> False
      without path = filter (not . atPath path)
      change path name value = map (\item -> if atPath path item then replaceField name value item else item)
  check "complete zero-count restored reset proof is admitted" (validate (withEntries 84 componentEntries) == Right ())
  forM_ [[1, 0], [0, 1], [0], [-1, 0]] $ \counts ->
    rejected "reset proof requires exactly two zero counts" (resetProof counts)
  rejected "missing reset fingerprint refuses" (deleteField "fingerprint" (resetProof [0, 0]))
  rejected "incomplete reset proof refuses" (replaceField "status" (String "blocked") (resetProof [0, 0]))
  rejected "invalid reset snapshot digest refuses" (replaceField "snapshot_sha256" (String "invalid") (resetProof [0, 0]))
  rejected "invalid reset key refuses" (replaceField "key" (String "invalid") (resetProof [0, 0]))
  rejected "different model reset refuses" (replaceField "identity" (replaceField "run_document" (toJSON (sha256Hex installedDigest)) resetIdentity) (resetProof [0, 0]))
  rejected "different installed DAT reset refuses" (replaceField "identity" (replaceField "installed_dat" (toJSON digest) resetIdentity) (resetProof [0, 0]))
  rejected "incomplete reset fingerprint refuses" (replaceField "fingerprint" (replaceField "status" (String "blocked") (fingerprint [0, 0])) (resetProof [0, 0]))
  check "cache-hit restore proof is admitted" (validate (replaceField "reset_cache"
    (object ["lookup" .= String "hit", "restore" .= String "restored"]) (resetProof [0, 0])) == Right ())
  forM_ ["reset_cache", "live_before_reset"] $ \name ->
    rejected "missing reset branch/live state refuses" (deleteField name (resetProof [0, 0]))
  forM_ [("hit", "not-restored"), ("miss", "restored"), ("miss", "not-restored"), ("save", "restored"), ("unknown", "restored")] $ \(lookupResult, restoreResult) ->
    rejected "inconsistent reset branch refuses" (replaceField "reset_cache"
      (object ["lookup" .= String lookupResult, "restore" .= String restoreResult]) (resetProof [0, 0]))
  rejected "nonzero incoming live state refuses" (replaceField "live_before_reset" (fingerprint [1, 0]) (resetProof [0, 0]))
  let withComponents value = replaceField "fingerprint"
        (replaceField "components" value (fingerprint [0, 0])) (resetProof [0, 0])
  forM_ ["schema", "algorithm", "root_sha256", "chunk_bytes", "entries"] $ \name ->
    rejected "missing component proof refuses" (withComponents (deleteField name components))
  rejected "unknown component envelope fields refuse" (withComponents (replaceField "unknown" Null components))
  rejected "unknown component entry fields refuse" (withComponents (replaceField "entries"
    (toJSON (change "$" "unknown" Null (bindArchive componentEntries))) components))
  rejected "reordered components refuse" (withComponents (replaceField "entries" (toJSON (reverse (bindArchive componentEntries))) components))
  rejected "missing component header refuses" (withComponents (replaceField "entries" (toJSON (without "$/header" (bindArchive componentEntries))) components))
  rejected "component root framing mismatch refuses" (replaceField "fingerprint"
    (replaceField "logical_bytes" (toJSON (85 :: Int)) (fingerprint [0, 0])) (resetProof [0, 0]))
  rejected "component root identity mismatch refuses" (withComponents (replaceField "root_sha256"
    (toJSON (sha256Hex installedDigest)) components))
  let withArchive archive = replaceField "fingerprint" (replaceField "exact_state" archive (fingerprint [0,0])) (resetProof [0,0])
      withCpu cpu = withArchive (replaceField "cpu" (toJSON cpu) (exactArchive [0,0]))
      firstCpu name value = withCpu [replaceField name value primaryBlob, secondaryBlob]
  forM_ ["exact_state", "components"] $ \name ->
    rejected ("missing raw archive binding " ++ show name) (replaceField "fingerprint" (deleteField name (fingerprint [0,0])) (resetProof [0,0]))
  forM_ ["schema","checkpoint_model_id","counts","now","model_definition_sha256","executable_sha256","cpu","exclusive"] $ \name ->
    rejected ("missing exact archive " ++ show name) (withArchive (deleteField name (exactArchive [0,0])))
  forM_ [("counts",toJSON [1,0 :: Int]),("now",toJSON (1 :: Int)),("checkpoint_model_id",String "")
        ,("model_definition_sha256",String "invalid"),("executable_sha256",String "invalid"),("unknown",Null)] $ \(name,value) ->
    rejected ("invalid exact archive " ++ show name) (withArchive (replaceField name value (exactArchive [0,0])))
  forM_ ["checkpoint_model_id","model_definition_sha256","executable_sha256"] $ \name ->
    rejected ("different trusted archive identity " ++ show name)
      (withArchive (replaceField name (toJSON (sha256Hex installedDigest)) (exactArchive [0,0])))
  check "trusted immutable archive identity is admitted" (E2E.validateAfArchiveBinding binding (fingerprint [0,0]) == Right ())
  check "matching paired reset proofs are admitted"
    (E2E.validateAfResetPair runDigest installedDigest (resetProof [0,0]) (resetProof [0,0]) == Right ())
  forM_ ["key","snapshot_sha256"] $ \name ->
    check ("different paired reset " ++ show name)
      (either (const True) (const False) (E2E.validateAfResetPair runDigest installedDigest (resetProof [0,0])
        (replaceField name (toJSON (sha256Hex installedDigest)) (resetProof [0,0]))))
  forM_ ["path","format","length","sha256","component_sha256","bytes_hex"] $ \name ->
    rejected ("missing raw blob " ++ show name) (withCpu [deleteField name primaryBlob,secondaryBlob])
  forM_ [("path",String "$/children/0/blobs/1"),("format",String "unknown"),("length",toJSON (1 :: Int))
        ,("sha256",toJSON digest),("component_sha256",toJSON digest),("bytes_hex",String "00")
        ,("unknown",Null)] $ \(name,value) ->
    rejected ("mismatched raw blob " ++ show name) (firstCpu name value)
  rejected "reordered CPU blobs" (withCpu (reverse (take 2 blobRows)))
  rejected "removed CPU blob" (withCpu (take 1 blobRows))
  rejected "extra CPU blob" (withCpu (take 2 blobRows ++ take 1 blobRows))
  rejected "trailing raw bytes without new digest" (firstCpu "bytes_hex" (toJSON (archiveHex cpuArchiveBytes ++ "00")))
  rejected "component archive hash disagreement" (withComponents
    (replaceField "entries" (toJSON (change "$/children/0/blobs/0" "sha256" (toJSON digest) (bindArchive componentEntries))) components))
  forM_ ["schema","model","run_document","installed_dat","boot_rom","adjustment_nand","record_nand","committed_nand","source_kind","card_sha256","procedure"] $ \name ->
    rejected ("missing reset identity " ++ show name) (replaceField "identity" (deleteField name resetIdentity) (resetProof [0,0]))
  rejected "unknown reset source kind" (replaceField "identity" (replaceField "source_kind" (String "unknown") resetIdentity) (resetProof [0,0]))
  rejected "old reset procedure" (replaceField "identity" (replaceField "procedure" (toJSON digest) resetIdentity) (resetProof [0,0]))
  let committedIdentity = replaceField "source_kind" (String "full-committed")
        (replaceField "adjustment_nand" Null (replaceField "committed_nand" (toJSON digest) resetIdentity))
      withIdentity value = replaceField "identity" value (resetProof [0,0])
  check "full committed NAND reset identity is admitted" (validate (withIdentity committedIdentity) == Right ())
  rejected "seeded reset requires adjustment NAND" (withIdentity (replaceField "adjustment_nand" Null resetIdentity))
  rejected "seeded reset refuses committed NAND" (withIdentity (replaceField "committed_nand" (toJSON digest) resetIdentity))
  rejected "committed reset requires committed NAND" (withIdentity (replaceField "committed_nand" Null committedIdentity))
  forM_ ["adjustment_nand","record_nand"] $ \name ->
    rejected ("committed reset refuses seeded input " ++ show name) (withIdentity (replaceField name (toJSON digest) committedIdentity))
  check "nested multi-chunk component proof is admitted" (validate (withEntries 1049171 nestedEntries) == Right ())
  forM_ ["$/blobs/0", "$/blobs/0/chunks/0", "$/children/0/children/0/header"] $ \path ->
    rejected "missing component parent/chunk/header refuses" (withEntries 1049171 (without path nestedEntries))
  rejected "wrong chunk offset refuses" (withEntries 1049171 (change "$/blobs/0/chunks/1" "offset" (toJSON (1 :: Int)) nestedEntries))
  rejected "wrong final chunk length refuses" (withEntries 1049171 (change "$/blobs/0/chunks/1" "logical_bytes" (toJSON (4 :: Int)) nestedEntries))
  let extraChunk = take 5 nestedEntries ++ [chunk "$/blobs/0/chunks/2" 1 2097152] ++ drop 5 nestedEntries
  rejected "excess component chunk refuses" (withEntries 1049171 extraChunk)
  rejected "noncanonical component sibling index refuses" (withEntries 1049171
    (change "$/children/10" "path" (String "$/children/010") nestedEntries))
  let twoHeaders = [rootEntry 16777308, rootHeader 8388608,
        node "$/children/0" 8388649, nodeHeader "$/children/0/header" 8388608]
  check "aggregate header bound is inclusive" (validate (withEntries 16777341 twoHeaders) == Right ())
  rejected "single oversized checkpoint header refuses"
    (withEntries 16777301 [rootEntry 16777268, rootHeader 16777217])
  rejected "multiple checkpoint headers exceeding aggregate bound refuse"
    (withEntries 16777342 (change "$" "logical_bytes" (toJSON (16777309 :: Int))
      (change "$/header" "logical_bytes" (toJSON (8388609 :: Int)) twoHeaders)))
  diagnostic <- acceptedOptions
    ["--ram-push-e2e", "--cardless", "--fujimod", "--flash-candidate", "--modal-observation", "--trajectories", "retained-route.json"]
  check "retained no-install diagnostic stays separate"
    (Viewer.modalDiagnosticOption diagnostic == RetainedFilmModalDiagnostic && Viewer.modalLifetimeGuardOption diagnostic == NoModalLifetimeGuard)
  forM_ [guard, scoped] $ \mode -> do
    forM_
      [ ("requires RAM-push E2E", filter (/= "--ram-push-e2e") stock ++ [mode])
      , ("requires cardless", filter (/= "--cardless") stock ++ [mode])
      , ("requires FujiMod", filter (/= "--fujimod") stock ++ [mode])
      , ("requires unreviewed enforced allowlist", filter (/= "--unreviewed-relink") stock ++ [mode])
      , ("refuses allowlist override", stock ++ [mode, "--allowlist-override", "test control"])
      , ("refuses virtual card", stock ++ [mode, "--sd-card-image", "unopened-card.img"])
      , ("refuses no-install", stock ++ [mode, "--flash-candidate"])
      , ("refuses retained observation", stock ++ [mode, "--modal-observation"])
      , ("refuses warm-only", stock ++ [mode, "--warm-checkpoint"])
      , ("refuses segmented route", stock ++ [mode, "--segments", "selected"])
      , ("refuses rehearsal", stock ++ [mode, "--ff80-rehearsal"])
      , ("refuses duplicate mode", stock ++ [mode, mode])
      ] $ \(label, arguments) -> refuses (mode ++ " " ++ label) arguments
    forM_
      [ ["--classic-chrome"]
      , ["--classic-chrome-icon"]
      , ["--experiment", "hello"]
      ] $ \payload -> do
        refuses (mode ++ " refuses other payload after selection") (stock ++ [mode] ++ payload)
        refuses (mode ++ " refuses other payload before selection") (stock ++ payload ++ [mode])
  forM_ [[guard, "--ram-push-lifecycle"], ["--ram-push-lifecycle", guard], [scoped, "--ram-push-lifecycle", guard], [guard, "--ram-push-lifecycle", scoped]] $ \modes ->
    refuses "explicit lifetime guard refuses lifecycle payloads" (stock ++ modes)
  forM_ [[scoped, "--ram-push-lifecycle"], ["--ram-push-lifecycle", scoped]] $ \modes ->
    refuses "automatic scoped lifetime guard refuses lifecycle composition" (stock ++ modes)
  _ <- acceptedOptions (stock ++ ["--ram-push-lifecycle"])
  refuses "observation conflict refuses in reverse order" (stock ++ ["--modal-observation", guard])
  refuses "scoped observation conflict refuses in reverse order" (stock ++ ["--modal-observation", scoped])
  refuses "duplicate retained mode refuses" ["--modal-observation", "--modal-observation"]
  refuses "duplicate no-install mode refuses" ["--flash-candidate", "--flash-candidate"]
  refuses "lifetime guard is a closed flag" (stock ++ ["--modal-lifetime-guard=continuous"])
  help <- Viewer.parseCommand [guard, "--help"]
  check "help opens no inputs and requires no route" (case help of Viewer.Help -> True; _ -> False)
  let owned = ["--modal75-owned-archive", "bank.a", "--modal75-retired", "retired.json"]
      engine = "--menu-engine-owned"
  forM_ [stock ++ [scoped, engine] ++ owned, engine : owned ++ scoped : stock] $ \arguments -> do
    candidate <- acceptedOptions arguments
    check "the owned engine selects the scoped payload with the engine" (Viewer.menuEngineOwnedOption candidate && Viewer.ramPayloadOption candidate == Viewer.BankModal75Choice)
    check "default engine selection excludes render" (not (Viewer.renderOwnedOption candidate))
    check "default engine role retains 225 publications" (payloadPublicationCount (Viewer.selectedPayload candidate) == 225)
  render <- acceptedOptions (stock ++ [scoped, engine, "--render-owned"] ++ owned)
  check "render flag selects the composed render role" (Viewer.renderOwnedOption render && Viewer.menuEngineOwnedOption render)
  check "render role selects 228 publications" (payloadPublicationCount (Viewer.selectedPayload render) == 228)
  refuses "render requires engine" (stock ++ [scoped,"--render-owned"] ++ owned)
  refuses "render requires archive" (stock ++ [scoped,engine,"--render-owned"])
  refuses "render flag refuses duplicates" (stock ++ [scoped,engine,"--render-owned","--render-owned"] ++ owned)
  scopedOwned <- acceptedOptions (stock ++ [scoped] ++ owned)
  check "the owned modal role alone carries no engine" (not (Viewer.menuEngineOwnedOption scopedOwned))
  refuses "the owned engine requires the scoped selection" (stock ++ [guard, engine] ++ owned)
  refuses "the owned engine requires an owned archive" (stock ++ [scoped, engine])
  refuses "the owned engine appears once" (stock ++ [scoped, engine, engine] ++ owned)
  scratch <- maybe getTemporaryDirectory pure =<< lookupEnv "TEST_TMPDIR"
  let directory = scratch </> "modal75-owned-package"
      identity = concat (replicate 16 "a5") ++ concat (replicate 16 "3c")
      identityBytes = B.pack (replicate 16 0xa5 ++ replicate 16 0x3c)
      document = "{\"schema\":2,\"image5_sha256\":\"3cd501c07533938ae4d9347616986fd1649005a588c36b5abf05e36ef4fd892a\",\"table_identity\":\""
        ++ identity ++ "\",\"retired\":[{\"callback\":\"0x00c7a7e4\",\"start\":13084644,\"end\":13084680}]}"
      load archiveBytes = do
        B.writeFile (directory </> "bank.a") archiveBytes
        B.writeFile (directory </> "retired.json") (BC.pack document)
        try (loadModalOwnedPackage (directory </> "bank.a") (directory </> "retired.json")) :: IO (Either IOException ModalOwnedPackage)
  createDirectoryIfMissing True directory
  carried <- load (BC.pack "!<arch>\n" <> identityBytes <> BC.pack "rest")
  check "an archive carrying the retired document's table identity loads"
    (either (const False) ((== identity) . ownedTableIdentity) carried)
  missing <- load (BC.pack "!<arch>\n" <> B.reverse identityBytes)
  check "an archive without the retired document's table identity refuses" (either (const True) (const False) missing)
  let block = B.pack (take 450 (cycle [0 .. 255]))
      settings bytes = either (ioError . userError) pure (liveSettings bytes)
      flipped offsets = B.pack [if offset `elem` offsets then byte + 1 else byte | (offset, byte) <- zip [0 :: Int ..] (B.unpack block)]
  before <- settings block
  same <- settings block
  changed <- settings (flipped [0x118, 0x11a])
  last' <- settings (flipped [449])
  check "an unchanged live-settings block reports no change" (liveSettingChange before same == Nothing)
  check "the lowest changed live setting is reported at its guest address with both values"
    (liveSettingChange before changed == Just (LiveSettingChange 0x0096cd7c (B.index block 0x118) (B.index block 0x118 + 1)))
  check "the block's last byte is compared" (fmap changedSettingAddress (liveSettingChange before last') == Just 0x0096ce25)
  check "a live-settings read of another length refuses"
    (all (either (const True) (const False) . liveSettings) [B.take 449 block, block <> B.singleton 0])

archiveHex :: B.ByteString -> String
archiveHex = concatMap (\byte -> let digits = showHex byte "" in replicate (2 - length digits) '0' ++ digits) . B.unpack

archiveComponentDigest :: B.ByteString -> String
archiveComponentDigest raw = sha256Hex (sha256 (BL.toStrict (BB.toLazyByteString
  (field "af-machine-components-v1" <> field "blob" <> number (B.length raw) <> number (1 :: Int)
    <> number (B.length raw) <> BB.byteString digest))))
  where
    number = BB.word64LE . fromIntegral
    field bytes = number (B.length bytes) <> BB.byteString bytes
    digest = either error id (hexBytes (sha256Hex (sha256 raw)))

hexBytes :: String -> Either String B.ByteString
hexBytes [] = Right B.empty
hexBytes (a:b:rest) = case reads ("0x" ++ [a,b]) of
  [(byte, "")] -> B.cons byte <$> hexBytes rest
  _ -> Left "invalid independent fixture hex"
hexBytes _ = Left "odd independent fixture hex"

cpuArchiveBytes :: B.ByteString
cpuArchiveBytes = cborMap
  [("format", cborText "rprofile-r5f-v1"), ("context", cborBytes architecture)
  ,("identity_input", zero), ("clock", nil), ("clock_overshoot", zero), ("policy", cborText "exact")
  ,("pmu", cborMap [(name, zero) | name <- ["control","enabled","interrupts","overflow","frozen","start","observed"]])
  ,("tcm", array (replicate 2 (cborMap [("register",zero),("len",zero),("pages",cborBytes ("FJMPAGE1" <> B.replicate 16 0))])))
  ,("maps",array []),("mmio",array []),("reservation",cborBytes ("FJEXCL01" <> B.replicate 32 0))
  ,("ring",array (replicate 64 zero)),("budget",zero),("head",zero),("count",zero),("cap",zero)
  ,("synced",zero),("settled_head",zero),("thumb",false),("restarted",nil),("user_stop",false)
  ,("irq",false),("hint",nil),("exception",nil),("failure",nil)]
  where
    zero = B.singleton 0
    nil = B.singleton 0xf6
    false = B.singleton 0xf4
    array entries = cborHead 4 (length entries) <> B.concat entries
    zeros count = array (replicate count zero)
    architecture = cborMap [("schema",B.singleton 1),("architecture",cborMap
      [("cortex-r5f",cborMap [("identity",zeros 6),("cpsr",B.pack [0x18,0xd3]),("registers",zeros 16)
        ,("scalar",zeros 4),("vectors",array (replicate 16 (zeros 16)))
        ,("banks",array [cborMap [("registers",zeros 7),("spsr",if index `elem` [0,6 :: Int] then nil else zero)] | index <- [0..6]])
        ,("cp15",array (replicate 16 (zeros 2))),("mpu_selected",zero),("mpu",array (replicate 16 (zeros 3)))])])]

cborHead :: Int -> Int -> B.ByteString
cborHead major count
  | count < 24 = B.singleton (fromIntegral (major * 32 + count))
  | count < 256 = B.pack [fromIntegral (major * 32 + 24), fromIntegral count]
  | otherwise = BL.toStrict (BB.toLazyByteString (BB.word8 (fromIntegral (major * 32 + 25)) <> BB.word16BE (fromIntegral count)))

cborBytes :: B.ByteString -> B.ByteString
cborBytes bytes = cborHead 2 (B.length bytes) <> bytes

cborText :: String -> B.ByteString
cborText text = cborHead 3 (length text) <> BC.pack text

cborMap :: [(String, B.ByteString)] -> B.ByteString
cborMap entries = cborHead 5 (length entries) <> B.concat [cborText name <> value | (name,value) <- entries]

exclusiveArchiveBytes :: B.ByteString
exclusiveArchiveBytes = either error id (hexBytes "464a4d4f4e493031341200000000000000000000000000800200000000000000400000000000000000300000000000000400000000000000800000000000000000400000000000000800000000000000")

archiveBlob :: Int -> B.ByteString -> Value
archiveBlob index raw = object
  ["path" .= ("$/children/0/blobs/" ++ show index)
  , "format" .= (if index == 2 then "FJMONI01" else "rprofile-r5f-v1" :: String)
  , "length" .= B.length raw, "sha256" .= sha256Hex (sha256 raw)
  , "component_sha256" .= archiveComponentDigest raw, "bytes_hex" .= archiveHex raw]

archiveBoardBytes :: Int
archiveBoardBytes = 32 + length ("board-keyframe-v1" :: String) + sum [8 + B.length raw | raw <- [cpuArchiveBytes, cpuArchiveBytes, exclusiveArchiveBytes]]

archiveBoardEntries :: [Value]
archiveBoardEntries = [entry "$/children/0" "checkpoint" archiveBoardBytes digest, entry "$/children/0/header" "header" 0 digest]
  ++ concat [let path = "$/children/0/blobs/" ++ show index
              in [entry path "blob" (B.length raw) (archiveComponentDigest raw),
                  object ["path" .= (path ++ "/chunks/0"), "kind" .= String "chunk"
                    , "checkpoint_kind" .= String "board-keyframe-v1", "logical_bytes" .= B.length raw
                    , "sha256" .= sha256Hex (sha256 raw), "offset" .= (0 :: Int)]]
            | (index, raw) <- zip [0 :: Int ..] [cpuArchiveBytes, cpuArchiveBytes, exclusiveArchiveBytes]]
  where
    digest = sha256Hex (sha256 "independent board framing")
    entry path kind bytes hash = object ["path" .= (path :: String), "kind" .= (kind :: String)
      , "checkpoint_kind" .= String "board-keyframe-v1", "logical_bytes" .= (bytes :: Int), "sha256" .= (hash :: String)]
