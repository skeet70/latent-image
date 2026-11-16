{-# LANGUAGE OverloadedStrings #-}
module Main (main) where

import Control.Exception (IOException, SomeException, displayException, try)
import Control.Monad (forM, forM_, unless, void)
import qualified Crypto.Hash.SHA256 as SHA
import Data.Aeson (Value (..), encode, object, (.=))
import qualified Data.Aeson.Key as K
import qualified Data.Aeson.KeyMap as KM
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as BL
import Data.Foldable (toList)
import Data.List (isPrefixOf)
import qualified Data.Text as T
import Data.Word (Word32)
import Emulation.BridgeClient
import qualified Emulation.CaptureStream as CS
import qualified Emulation.PlaybackOwned as PO
import EmulatorUsb (bridgeUsbApi)
import qualified FujiAF.Broker as P
import qualified FujiAF.Probe as Probe
import qualified FujiAF.Read as R
import FujiAF.Transport (usbTransfer)
import Numeric (showHex)
import System.Directory (canonicalizePath, createDirectory, doesPathExist, makeAbsolute)
import System.Environment (getArgs, getExecutablePath)
import System.Exit (ExitCode (..), exitWith)
import System.FilePath (splitDirectories, takeDirectory, takeFileName, (</>))
import System.IO (IOMode (..), SeekMode (..), hClose, hSeek, openBinaryFile, withBinaryFile)
import System.Process (CreateProcess (..), StdStream (..), callProcess, createProcess, proc, readProcessWithExitCode, waitForProcess)

newtype Digest = Digest T.Text deriving (Eq, Show)
data Options = Options FilePath Digest FilePath FilePath FilePath

digest :: B.ByteString -> Digest
digest = Digest . T.pack . concatMap byte . B.unpack . SHA.hash
  where byte value = let digits = showHex value "" in replicate (2 - length digits) '0' ++ digits

digestValue :: Digest -> Value
digestValue (Digest value) = String value

fileDigest :: FilePath -> IO Digest
fileDigest path = do
  (status, output, _) <- readProcessWithExitCode "sha256sum" ["--", path] ""
  require (status == ExitSuccess) ("sha256sum failed for " ++ path)
  case words output of
    value : _ | length value == 64 -> pure (Digest (T.pack value))
    _ -> harness "sha256sum returned no digest"

parseOptions :: [String] -> IO Options
parseOptions arguments = case arguments of
  ["--sdk",sdk,"--bridge-sha256",sha,"--installed",installed,"--owned-playback",package,"--output",output]
    | length sha == 64 && all (`elem` ("0123456789abcdef" :: String)) sha ->
      Options <$> canonicalizePath sdk <*> pure (Digest (T.pack sha)) <*> canonicalizePath installed <*> canonicalizePath package <*> makeAbsolute output
  _ -> harness "playback-mode-observe --sdk STORE --bridge-sha256 HEX --installed DIR --owned-playback PACKAGE --output NEW_PRIVATE_DIR"

json :: FilePath -> Value -> IO ()
json path value = BL.writeFile path (encode value <> "\n")

freshOutput :: FilePath -> IO ()
freshOutput path = do
  private <- canonicalizePath "private-data"
  parent <- canonicalizePath (takeDirectory path)
  exists <- doesPathExist path
  require (splitDirectories private `isPrefixOf` splitDirectories parent && not exists && takeFileName path `notElem` ["", ".", ".."]) "output must be new beneath private-data"
  createDirectory path

required :: Either String a -> IO a
required = either harness pure

checkGuest :: FilePath -> Bridge -> PO.Package -> IO [(Word32, B.ByteString)]
checkGuest path bridge package = do
  actual <- forM (PO.guestArtifacts package) $ \(address,expected) -> do
    bytes <- B.concat <$> forM [0,4096 .. B.length expected - 1] (\offset -> readMemory bridge (address + fromIntegral offset) (min 4096 (B.length expected - offset)))
    pure (address,bytes)
  json path (object ["artifacts" .= [object ["address" .= address,"expected_hex" .= P.hexBytes (B.unpack expected),"actual_hex" .= P.hexBytes (B.unpack bytes),"actual_sha256" .= digestValue (digest bytes)] | ((address,expected),(_,bytes)) <- zip (PO.guestArtifacts package) actual]])
  required (PO.checkGuestArtifacts package actual)
  bodies <- forM (PO.originalBodies package) (\(address,count) -> readMemory bridge address count)
  required (PO.checkOriginalBodies package bodies)
  pure actual

data Site = UsbEdge | UsbApplicationArm | ApplicationUsb | RequestUsbMode | UiTransition | Machine5Action5 | Packet142 | RetainedAction2 | RetainedAction3 deriving (Enum, Bounded)

site :: Site -> (Word32, T.Text)
site UsbEdge = (0xecf8b8,"usb-edge")
site UsbApplicationArm = (0xecf99c,"usb-application-arm")
site ApplicationUsb = (0xc82b00,"application-usb")
site RequestUsbMode = (0xd0afe0,"request-usb-mode")
site UiTransition = (0xc24994,"ui-transition")
site Machine5Action5 = (0xc256a8,"machine5-action5")
site Packet142 = (0xdb3d28,"packet-0142")
site RetainedAction2 = (0x11ea434,"retained-action2")
site RetainedAction3 = (0x11ea458,"retained-action3")

sites :: [(Word32,T.Text)]
sites = map site [minBound .. maxBound]

sourceIdentity :: FilePath -> Bridge -> IO ()
sourceIdentity output bridge = do
  let path = "private-data/analysis/x-m1-1.50/lz/id5-00bf1a00.bin"
      plainPath = "private-data/analysis/x-m1-1.50/lz/plain-01100000.bin"
  sourceSha <- fileDigest path
  plainSha <- fileDigest plainPath
  require (sourceSha == Digest "3cd501c07533938ae4d9347616986fd1649005a588c36b5abf05e36ef4fd892a") "stock image 5 identity differs"
  require (plainSha == Digest "ee211bdaad39005b4f72e3443a76ca194905381a967cab787d6a4ef14e2d889d") "stock plain image identity differs"
  comparisons <- withBinaryFile path ReadMode $ \image5 -> withBinaryFile plainPath ReadMode $ \plain ->
    forM sites $ \(address,label) -> do
      let (source,base,count) = if address >= 0x1100000 then (plain,0x1100000,36) else (image5,0xbf1a00,16)
      hSeek source AbsoluteSeek (fromIntegral (address - base))
      expected <- B.hGet source count
      actual <- readMemory bridge address count
      require (B.length expected == count && actual == expected) "mode route stock entry bytes differ"
      pure (object ["address" .= address,"label" .= label,"expected_hex" .= P.hexBytes (B.unpack expected),"actual_hex" .= P.hexBytes (B.unpack actual)])
  json (output </> "stock-entry-identities.json") (object ["revision" .= ("55436fc85c0560dd252b0410af6f2427fe198cd3" :: T.Text),"source" .= path,"sha256" .= digestValue sourceSha,"plain_sha256" .= digestValue plainSha,"comparisons" .= comparisons])

volatileGate :: Bridge -> FilePath -> IO ()
volatileGate bridge output = do
  let api = bridgeUsbApi (exchangeLine bridge)
      tx packet wanted = do
        (count,bytes) <- usbTransfer api packet 5000
        require (count == wanted) "configuration transfer count differs"
        pure bytes
      control op count sid = void (tx (P.controlTransfer (P.control op count sid)) 12)
  control 0xc0 8 0
  opened <- tx (P.BulkIn 0x81 8) 8
  sid <- case opened of
    low : high : rest | length rest == 6 -> pure (fromIntegral low + 256 * fromIntegral high)
    _ -> harness "short virtual protocol session reply"
  let readGate label = do
        control 0xd0 R.readRequestBytes sid
        void (tx (P.BulkOut 2 (R.readRequestFrame Probe.readbackWindow)) R.readRequestBytes)
        control 0xd1 R.readReplyBytes sid
        response <- tx (P.BulkIn 0x81 R.readReplyBytes) R.readReplyBytes
        bytes <- required (R.decodeReadReply Probe.readbackWindow response)
        require (length bytes == R.readWindowLength) "short gate readback"
        B.writeFile (output </> label) (B.pack bytes)
        pure bytes
  before <- readGate "configuration-before.bin"
  require (Probe.gateByteOf before == fromIntegral Probe.gateEnabled) "installed checkpoint does not have the debug gate enabled"
  control 0xd0 Probe.configWriteBytes sid
  void (tx (P.BulkOut 2 (Probe.configWriteFrame Probe.gateDisabled)) Probe.configWriteBytes)
  control 0xd1 Probe.configReplyBytes sid
  response <- tx (P.BulkIn 0x81 Probe.configReplyBytes) Probe.configReplyBytes
  required (Probe.decodeConfigWriteReply response)
  after <- readGate "configuration-after.bin"
  let expected = take Probe.gateIndex before ++ [fromIntegral Probe.gateDisabled] ++ drop (Probe.gateIndex + 1) before
  require (after == expected) "configuration readback differs beyond the volatile f7 byte"
  control 0xc4 2 sid
  void (tx (P.BulkIn 0x81 2) 2)
  json (output </> "volatile-configuration.json") (object ["index" .= Probe.gateIndex,"before" .= Probe.gateByteOf before,"after" .= Probe.gateByteOf after,"other_window_bytes_equal" .= True,"persistent_commit_requested" .= False])

traceHits :: Bridge -> IO Value
traceHits bridge = Object <$> call bridge (command "trace-hits" ["from" .= (0 :: Int)])

observe :: FilePath -> Bridge -> PO.Package -> IO Value
observe output bridge package = do
  sourceIdentity output bridge
  before <- checkGuest (output </> "installed-artifacts-before.json") bridge package
  void (call bridge (command "trace-sites" ["sites" .= object [K.fromString ("0x" ++ showHex address "") .= label | (address,label) <- sites]]))
  begun <- call bridge (PO.watchRequest package)
  required (PO.validateWatchBegin package (Object begun))
  json (output </> "watch-begin.json") (Object begun)
  Object <$> call bridge (command "ui-state" []) >>= json (output </> "ui-before.json")
  Object <$> call bridge (command "media-state" []) >>= json (output </> "media-before.json")
  volatileGate bridge output
  forM_ [(False,"detached"),(True,"attached")] $ \(connected,label) -> do
    reply <- call bridge (command "cable" ["connected" .= connected])
    require (KM.lookup "usb_connected" reply == Just (Bool connected)) "modeled cable readback differs"
    json (output </> (label ++ "-cable.json")) (Object reply)
    idle <- call bridge (command "idle" ["instructions" .= (40000000 :: Int)])
    json (output </> (label ++ "-idle.json")) (Object idle)
    Object <$> call bridge (command "ui-state" []) >>= json (output </> (label ++ "-ui.json"))
    traceHits bridge >>= json (output </> (label ++ "-trace.json"))
  enumeration <- call bridge (command "enumerate" ["budget" .= (90000000 :: Int)])
  json (output </> "enumeration.json") (Object enumeration)
  Object <$> call bridge (command "ui-state" []) >>= json (output </> "ui-final.json")
  Object <$> call bridge (command "media-state" []) >>= json (output </> "media-final.json")
  traces <- traceHits bridge
  json (output </> "trace-final.json") traces
  after <- checkGuest (output </> "installed-artifacts-after.json") bridge package
  state <- call bridge (command "watch-state" [])
  ended <- call bridge (command "watch-end" [])
  json (output </> "watch-state.json") (Object state)
  json (output </> "watch-end.json") (Object ended)
  witnessed <- required (PO.witness package (Object begun) (Object state) (Object ended))
  require (before == after) "installed runtime allocation or publication bytes changed during the mode interval"
  let hits = case traces of
        Object fields -> case KM.lookup "hits" fields of
          Just (Array rows) -> toList rows
          _ -> []
        _ -> []
      hit label = any (\row -> case row of Object fields -> textAt "label" fields == Just label; _ -> False) hits
      reached = PO.modeObserved witnessed && hit "machine5-action5" && (hit "retained-action2" || hit "retained-action3")
      runtime = case before of (address,bytes) : _ -> object ["start" .= address,"end_exclusive" .= (address + fromIntegral (B.length bytes)),"bytes" .= B.length bytes]; _ -> Null
  pure (object ["natural_mode_consumer_observed" .= reached,"owned_entry_witness" .= PO.witnessValue witnessed,"runtime_allocation" .= runtime,"runtime_and_publication_bytes_unchanged" .= (before == after),"integrated_jpeg_pair_acceptance" .= False,"whole_playback_acceptance" .= False])

main :: IO ()
main = do
  Options sdk expectedBridge installed package output <- getArgs >>= parseOptions
  freshOutput output
  selected <- required (PO.selection True (Just installed) (Just package)) >>= maybe (harness "installed selection missing") pure
  (_,owned) <- PO.loadPackage selected
  let bridgePath = sdk </> "bin/fujimod-xm1-ram-push"
      runDocument = sdk </> "share/fujimod-xm1/xm1-150.json"
      cardPath = "private-data/scratch/issue117-camera-fixture/DSCF3095-DCIM.img"
  actualBridge <- fileDigest bridgePath
  require (actualBridge == expectedBridge) "bridge identity differs"
  card <- canonicalizePath cardPath
  cardBefore <- fileDigest card
  require (cardBefore == Digest "e22a3208dac72cc7a3e2ceca982e222fc20a320fa29b2f3c79557fe133fcc9be") "card fixture identity differs"
  let copy = output </> "card.img"
  callProcess "cp" ["--reflink=auto","--",card,copy]
  firmware <- canonicalizePath "private-data/firmware/x-m1-1.50/FWUP0002.DAT"
  nand <- canonicalizePath "private-data/analysis/x-m1-1.50/stored-f7/emulation/inputs/f7-1.nand"
  rom <- canonicalizePath "private-data/analysis/x-m1-1.50/bootrom/bootrom-0x80000000-4000.bin"
  checkpoints <- canonicalizePath (takeDirectory installed)
  let arguments = ["--run",runDocument,"--boot-rom",rom,"--installed-dat",firmware,"--adjustment-nand",nand,"--output",output </> "bridge","--ram-push-checkpoints",checkpoints,"--sd-card-image",copy]
  executable <- getExecutablePath
  executableSha <- fileDigest executable
  runSha <- fileDigest runDocument
  json (output </> "invocation.json") (object ["sdk" .= sdk,"bridge" .= bridgePath,"bridge_sha256" .= digestValue actualBridge,"observer" .= executable,"observer_sha256" .= digestValue executableSha,"run_document_sha256" .= digestValue runSha,"arguments" .= arguments,"installed" .= installed,"package" .= package,"card_sha256" .= digestValue cardBefore])
  errors <- openBinaryFile (output </> "bridge.stderr") WriteMode
  requests <- openBinaryFile (output </> "bridge.stdin.jsonl") WriteMode
  replies <- openBinaryFile (output </> "bridge.stdout.jsonl") WriteMode
  (Just input, Just stdout, _, process) <- createProcess (proc bridgePath arguments)
    { std_in = CreatePipe,std_out = CreatePipe,std_err = UseHandle errors,cwd = Just output }
  reader <- CS.newBoundedReader stdout
  let bridge = Bridge input reader requests replies
  outcome <- try $ do
    options <- event bridge "start-options"
    json (output </> "start-options.json") (Object options)
    record <- B.readFile (installed </> "ram-push-install.json")
    start <- required (PO.validateInstalled selected owned record (Object options))
    send bridge start
    ready <- event bridge "ready"
    json (output </> "ready.json") (Object ready)
    required (PO.validateReady selected start (Object ready))
    observe output bridge owned
  finished <- try (call bridge (command "finish" [])) :: IO (Either SomeException Fields)
  either (json (output </> "finish-error.json") . String . T.pack . displayException) (json (output </> "finish.json") . Object) finished
  void (try (hClose input) :: IO (Either IOException ()))
  status <- waitForProcess process
  mapM_ hClose [errors,requests,replies,stdout]
  cardAfter <- fileDigest card
  copyAfter <- fileDigest copy
  let unchanged = cardBefore == cardAfter && copyAfter == cardBefore
      (details,problem) = case outcome of Left exception -> (Null,Just (displayException (exception :: SomeException))); Right value -> (value,Nothing)
      observed = case details of Object fields -> KM.lookup "natural_mode_consumer_observed" fields == Just (Bool True); _ -> False
      passed = observed && unchanged && status == ExitSuccess && either (const False) (const True) finished
  json (output </> "result.json") (object ["schema" .= (1 :: Int),"scope" .= ("installed volatile-f7 natural cable/application-mode consumer observation" :: T.Text),"qualified" .= passed,"details" .= details,"harness_error" .= problem,"bridge_exit" .= show status,"card_original_unchanged" .= (cardBefore == cardAfter),"card_copy_unchanged" .= (copyAfter == cardBefore),"card_original_sha256_after" .= digestValue cardAfter,"card_copy_sha256_after" .= digestValue copyAfter])
  unless passed (exitWith (ExitFailure 3))
