module Main (main) where

import Control.Exception (IOException, try)
import Control.Monad (forM_, unless)
import qualified Xm1Viewer as Viewer
import Xm1RamPushE2E (ModalDiagnostic (..), ModalLifetimeGuard (..), ModalOwnedPackage (..), loadModalOwnedPackage)
import qualified Xm1RamPushE2E as RamE2E
import Emulation.Sha256 (parseSha256Hex)
import FujiAF.RamPush (payloadPublicationCount)
import Emulation.LiveSettings (LiveSettingChange (..), liveSettingChange, liveSettings)
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as BC
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
      session = either error id (RamE2E.keeperSessionId sessionText)
      transition = RamE2E.keeperTransition session
  check "keeper status retains the holding state"
    (transition RamE2E.KeeperHolding ("status " ++ sessionText) == Right (RamE2E.KeeperStatus, RamE2E.KeeperHolding))
  check "keeper readback retains the holding state"
    (transition RamE2E.KeeperHolding ("readback " ++ sessionText) == Right (RamE2E.KeeperReadback, RamE2E.KeeperHolding))
  check "authenticated stop ends the keeper"
    (transition RamE2E.KeeperHolding ("stop " ++ sessionText) == Right (RamE2E.KeeperStop, RamE2E.KeeperStopped))
  forM_ ["status", "stop " ++ replicate 64 'b', "install " ++ sessionText, "status " ++ sessionText ++ " extra", replicate 161 'a'] $ \line ->
    check "keeper rejects malformed, wrong-session or publication requests" (case transition RamE2E.KeeperHolding line of Left _ -> True; _ -> False)
  forM_ ["status ", "readback ", "stop "] $ \command ->
    check "stopped keeper refuses every command"
      (case transition RamE2E.KeeperStopped (command ++ sessionText) of Left _ -> True; _ -> False)
  let digest = either error id . parseSha256Hex
      dat = digest "cd2f3d7d187ab72151abdb325891bac5b5a3257db8640c6b0c0e836d29b37626"
      image4 = digest "40d7fe9afdcd067a4441df8a079cef1fb4ee9e18a8240d9abd492a6db545dbdf"
      contract = digest "1b839ebdb8f7448c211014a619bb004832b5cecc3eac673faf045f8226d841eb"
      other = digest (replicate 64 '0')
  check "keeper accepts the immutable admitted source triple"
    (RamE2E.checkPropertyAllocationSource dat image4 contract == Right ())
  forM_ [(other,image4,contract), (dat,other,contract), (dat,image4,other)] $ \(d,i,c) ->
    check "keeper rejects a changed DAT, image4 or composition identity"
      (case RamE2E.checkPropertyAllocationSource d i c of Left _ -> True; _ -> False)
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
