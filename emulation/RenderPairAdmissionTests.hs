{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}

-- | Admission behaviour: a complete matching receipt binds and reuses, and a changed, partial,
-- wrong-role or wrong-input receipt refuses. The review-packet import path is exercised against a
-- synthetic packet, and the owned configuration takes its depths and source bytes from the ELF.
module Main (main) where

import Control.Monad (forM_, unless)
import qualified Data.Aeson as J
import qualified Data.Aeson.Key as K
import qualified Data.Aeson.KeyMap as KM
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import qualified Data.ByteString.Lazy as BL
import Data.Bits (shiftR)
import Data.Either (isLeft)
import qualified Data.Text as T
import Data.Word (Word32)
import Emulation.RenderPairAdmission
import System.Directory
  ( createDirectoryIfMissing
  , createDirectoryLink
  , doesDirectoryExist
  , doesFileExist
  , doesPathExist
  , getCurrentDirectory
  , getTemporaryDirectory
  , listDirectory
  , removeDirectoryRecursive
  , removeFile
  , setCurrentDirectory
  )
import System.Exit (exitFailure)
import System.FilePath ((</>), takeDirectory, takeFileName)
import System.Posix.Process (getProcessID)

check :: String -> Bool -> IO Bool
check label good = do
  putStrLn ((if good then "ok   " else "FAIL ") ++ label)
  pure good

expectLeft :: String -> Either String a -> IO Bool
expectLeft label result = check label (isLeft result)

accepts :: String -> Either String a -> IO Bool
accepts label result = case result of
  Right _ -> check label True
  Left problem -> putStrLn ("     " ++ problem) >> check label False

word :: Word32 -> B.ByteString
word value = B.pack [fromIntegral (value `shiftR` shift) | shift <- [0, 8, 16, 24]]

half :: Word32 -> B.ByteString
half = B.take 2 . word

-- The RenderStack test fixture: a linked bank ELF whose four functions reserve the requested
-- bytes, so the derived entry depths are known.
elfFixture :: Word32 -> Word32 -> Word32 -> Word32 -> (B.ByteString, B.ByteString)
elfFixture treeReservation pickerReservation paneReservation bankReservation =
  (bytes, code)
 where
  code = B.concat [word (0xe24dd000 + reservation) <> word 0xe12fff1e
                  | reservation <- [treeReservation, pickerReservation, paneReservation, bankReservation]]
  names = C.pack "\0.text\0.shstrtab\0.strtab\0.symtab\0"
  strings = C.pack "\0rb_render_tree_entry\0rb_render_picker_entry\0rb_render_pane_entry\0rb_bank_draw_root\0"
  symbol name address = B.concat (map word [name, address, 8]) <> B.pack [0x12, 0] <> half 1
  symbols = B.replicate 16 0 <> symbol 1 0xc40000 <> symbol 22 (0xc40000 + 8)
    <> symbol 45 (0xc40000 + 16) <> symbol 66 (0xc40000 + 24)
  namesAt = 0x100 + B.length code
  stringsAt = namesAt + B.length names
  symbolsAt = stringsAt + B.length strings
  headersAt = symbolsAt + B.length symbols
  section :: Word32 -> Word32 -> Word32 -> Word32 -> Int -> Int -> Word32 -> Word32 -> B.ByteString
  section name kind flags address offset size link entrySize = B.concat
    (map word [name, kind, flags, address, fromIntegral offset, fromIntegral size, link, 0, 4, entrySize])
  sections = B.concat
    [ B.replicate 40 0
    , section 1 1 6 0xc40000 0x100 (B.length code) 0 0
    , section 7 3 0 0 namesAt (B.length names) 0 0
    , section 17 3 0 0 stringsAt (B.length strings) 0 0
    , section 25 2 0 0 symbolsAt (B.length symbols) 3 16
    ]
  header = B.pack [0x7f, 69, 76, 70, 1, 1, 1] <> B.replicate 9 0 <> half 2 <> half 40 <> word 1 <> word 0xc40000
    <> word 0 <> word (fromIntegral headersAt) <> word 0 <> half 52 <> half 32 <> half 0 <> half 40 <> half 5 <> half 2
  bytes = header <> B.replicate (0x100 - B.length header) 0 <> code <> names <> strings <> symbols <> sections

data Fixture = Fixture
  { fixtureRoot :: FilePath
  , fixtureReceipt :: FilePath
  , fixtureExpected :: Expected
  }

main :: IO ()
main = do
  temporary <- getTemporaryDirectory
  pid <- getProcessID
  let root = temporary </> ("render-pair-admission-tests-" ++ show pid)
  removeIfPresent root
  createDirectoryIfMissing True root
  results <- concat <$> mapM (runRoot root) [("stock", StockRole), ("owned", OwnedRole)]
  removeDirectoryRecursive root
  unless (and results) exitFailure
  putStrLn ("render pair admission tests passed: " ++ show (length results))

removeIfPresent :: FilePath -> IO ()
removeIfPresent path = do
  present <- doesPathExist path
  directory <- doesDirectoryExist path
  if not present then pure () else if directory then removeDirectoryRecursive path else removeFile path

runRoot :: FilePath -> (String, Role) -> IO [Bool]
runRoot root (label, role) = do
  let directory = root </> label
      receipt = directory </> "receipt"
  createDirectoryIfMissing True receipt
  fixture <- buildFixture directory receipt role
  original <- getCurrentDirectory
  setCurrentDirectory directory
  bound <- bindReceipt (fixtureExpected fixture) receipt GuestRun
  admitted <- accepts (label ++ " complete matching receipt binds") bound
  admissionFile <- doesFileExist (receipt </> "admission.json")
  admittedFile <- check (label ++ " bind writes its admission record") admissionFile
  reused <- verifyReceipt (fixtureExpected fixture) receipt
  reusedOk <- accepts (label ++ " admitted receipt reuses") reused
  let tamperedReceipt = fixtureRoot fixture </> "tampered-observation"
  copyTreeForTest receipt tamperedReceipt
  B.appendFile (tamperedReceipt </> "e2e-observation-0.json") "\n"
  tampered <- verifyReceipt (fixtureExpected fixture) tamperedReceipt
  tamperedOk <- expectLeft (label ++ " tampered observation document refuses reuse") tampered
  missingList <- observationListControl fixture receipt Nothing
  missingListOk <- expectLeft (label ++ " missing observation list refuses reuse") missingList
  emptyList <- observationListControl fixture receipt (Just [])
  emptyListOk <- expectLeft (label ++ " empty observation list refuses reuse") emptyList
  changed <- mutateAndVerify fixture receipt "e2e-result.json" (\bytes -> bytes <> "\n")
  changedOk <- expectLeft (label ++ " changed receipt file refuses reuse") changed
  missing <- partialReceipt fixture
  missingOk <- expectLeft (label ++ " partial receipt refuses") missing
  wrongTrajectory <- wrongTrajectoryDigest fixture receipt
  wrongTrajectoryOk <- expectLeft (label ++ " wrong trajectory refuses") wrongTrajectory
  wrongRole <- wrongRoleReceipt fixture receipt
  wrongRoleOk <- expectLeft (label ++ " wrong role shape refuses") wrongRole
  malformed <- badSourceIdentity fixture "{\"a\": 1}\n"
  malformedOk <- expectLeft (label ++ " malformed source identity refuses") malformed
  empty <- badSourceIdentity fixture "{}\n"
  emptyOk <- expectLeft (label ++ " empty source identity refuses") empty
  prelinkedBase <- if role == OwnedRole then verifyInputs ((fixtureExpected fixture) { expectedPrelinkedBase = Just 0xc40008 }) else pure (Right ())
  prelinkedBaseOk <- if role == OwnedRole then expectLeft (label ++ " wrong prelinked base refuses") prelinkedBase else pure True
  prelinkedSize <- if role == OwnedRole then verifyInputs ((fixtureExpected fixture) { expectedPrelinkedSize = Just 33 }) else pure (Right ())
  prelinkedSizeOk <- if role == OwnedRole then expectLeft (label ++ " wrong prelinked size refuses") prelinkedSize else pure True
  mutatedPrelinked <- badPrelinked fixture mutatePrelinkedManifest
  mutatedPrelinkedOk <- if role == OwnedRole
    then expectLeft (label ++ " mutated prelinked manifest refuses") mutatedPrelinked
    else pure True
  unboundSources <- if role == OwnedRole then badPrelinkedFile fixture "ram-push-manifest.json" mutateRenderSources else pure (Right ())
  unboundSourcesOk <- if role == OwnedRole
    then expectLeft (label ++ " prelinked manifest render source identity refuses") unboundSources
    else pure True
  alteredElf <- if role == OwnedRole then badPrelinkedFile fixture "kernel.elf" (\bytes -> bytes <> B.singleton 0) else pure (Right ())
  alteredElfOk <- if role == OwnedRole then expectLeft (label ++ " altered prelinked ELF refuses") alteredElf else pure True
  alteredRuntime <- if role == OwnedRole then badPrelinkedFile fixture "runtime.bin" (\bytes -> bytes <> B.singleton 0) else pure (Right ())
  alteredRuntimeOk <- if role == OwnedRole then expectLeft (label ++ " altered prelinked runtime refuses") alteredRuntime else pure True
  alteredInput <- if role == OwnedRole then badPrelinkedFile fixture "bank-allowlist-input.json" (\bytes -> bytes <> B.singleton 0) else pure (Right ())
  alteredInputOk <- if role == OwnedRole then expectLeft (label ++ " altered allowlist input refuses") alteredInput else pure True
  alteredReport <- if role == OwnedRole then badPrelinkedFile fixture "bank-allowlist-report.json" (\bytes -> bytes <> B.singleton 0) else pure (Right ())
  alteredReportOk <- if role == OwnedRole then expectLeft (label ++ " altered allowlist report refuses") alteredReport else pure True
  alteredProof <- if role == OwnedRole then badPrelinkedFile fixture "bank-allowlist-proof.json" mutateProofAnalysis else pure (Right ())
  alteredProofOk <- if role == OwnedRole then expectLeft (label ++ " altered allowlist proof refuses") alteredProof else pure True
  alteredChecker <- if role == OwnedRole then badPrelinkedFile fixture "bank-allowlist-proof.json" mutateProofChecker else pure (Right ())
  alteredCheckerOk <- if role == OwnedRole then expectLeft (label ++ " altered allowlist checker refuses") alteredChecker else pure True
  alteredArchive <- if role == OwnedRole then badArchive fixture else pure (Right ())
  alteredArchiveOk <- if role == OwnedRole then expectLeft (label ++ " altered owned archive refuses") alteredArchive else pure True
  alteredSource <- if role == OwnedRole then badSourceIdentityDigest fixture else pure (Right ())
  alteredSourceOk <- if role == OwnedRole then expectLeft (label ++ " altered source identity refuses") alteredSource else pure True
  unboundSidecar <- if role == OwnedRole then badSourceIdentityBytes fixture else pure (Right ())
  unboundSidecarOk <- if role == OwnedRole then expectLeft (label ++ " source sidecar bytes are not bound to the manifest") unboundSidecar else pure True
  missingChecker <- if role == OwnedRole then verifyInputs ((fixtureExpected fixture) { expectedChecker = Just (fixtureRoot fixture </> "missing-checker") }) else pure (Right ())
  missingCheckerOk <- if role == OwnedRole then expectLeft (label ++ " missing allowlist checker refuses") missingChecker else pure True
  failingReport <- if role == OwnedRole then badPrelinkedFailingReport fixture else pure (Right ())
  failingReportOk <- if role == OwnedRole then expectLeft (label ++ " failing EnforceAllowlist report refuses") failingReport else pure True
  imported <- importSynthetic root label fixture
  let mismatchedOutcome = if role == StockRole then "qualified-scoped-lifetime-diagnostic" else "candidate-qualified"
  refused <- mapM (\(control, name, key, replacement) -> do
    let path = receipt </> name
    bytes <- B.readFile path
    value <- either (ioError . userError) pure (J.eitherDecodeStrict' bytes)
    changedValue <- case value of
      J.Object fields -> pure (J.Object (KM.insert key replacement fields))
      _ -> ioError (userError "receipt fixture is not an object")
    BL.writeFile path (J.encode changedValue)
    result <- bindReceipt (fixtureExpected fixture) receipt GuestRun
    B.writeFile path bytes
    expectLeft (label ++ " " ++ control ++ " refuses") result)
    [ ("mismatched qualification", "e2e-result.json", "outcome", J.String mismatchedOutcome)
    , ("missing lifetime coverage", "e2e-modal-observation.json", "whole_installed_lifetime_coverage", J.Bool False)
    ]
  setCurrentDirectory original
  pure ([admitted, admittedFile, reusedOk, tamperedOk, missingListOk, emptyListOk, unboundSidecarOk, unboundSourcesOk, changedOk, missingOk, wrongTrajectoryOk, wrongRoleOk, malformedOk, emptyOk, prelinkedBaseOk, prelinkedSizeOk, mutatedPrelinkedOk
       , alteredElfOk, alteredRuntimeOk, alteredInputOk, alteredReportOk, alteredProofOk, alteredCheckerOk, alteredArchiveOk, alteredSourceOk, missingCheckerOk, failingReportOk, imported] ++ refused)

buildFixture :: FilePath -> FilePath -> Role -> IO Fixture
buildFixture directory receipt role = do
  let write name bytes = B.writeFile (directory </> name) bytes
      (elfBytes, code) = elfFixture 16 32 16 16
      retainedBytes = B.pack [1, 2, 3, 4]
      firmwareBytes = B.pack [5, 6, 7]
      nandBytes = B.pack [8, 9]
      bootRomBytes = B.pack [10, 11]
      runBytes = B.pack [12, 13]
      trajectoryBytes = B.pack [14, 15]
      configPath = directory </> "diagnostic.json"
      retainedPath = directory </> "retained.bin"
      textPath = directory </> "text.bin"
      firmwarePath = directory </> "firmware.bin"
      nandPath = directory </> "nand.bin"
      bootRomPath = directory </> "bootrom.bin"
      runPath = directory </> "run.json"
      trajectoryPath = directory </> "trajectory.json"
      pathsPath = directory </> "FujiModPaths.hs"
      sourceIdentityPath = directory </> "render-source-identity.json"
      sdk = "/nix/store/pfra9jndlp55cmxpjav11yskidbzg1pi-fujimod-xm1-0.1.0"
      archivePath = directory </> "bank.a"
      archiveBytes = B.pack [16, 17]
      checkerPath = directory </> "checker.bin"
      checkerBytes = B.pack [23, 24, 25]
  createDirectoryIfMissing True (receipt </> "relinked")
  write "retained.bin" retainedBytes
  write "text.bin" code
  write "firmware.bin" firmwareBytes
  write "nand.bin" nandBytes
  write "bootrom.bin" bootRomBytes
  write "run.json" runBytes
  write "trajectory.json" trajectoryBytes
  write "bank.a" archiveBytes
  write "checker.bin" checkerBytes
  writeFile pathsPath (unlines
    [ "module FujiModPaths (fujimodDriver, fujimodRamPushBridge, fujimodToggleBridge, fujimodRun) where"
    , "fujimodRamPushBridge = \"" ++ sdk ++ "/bin/fujimod-xm1-ram-push\""
    , "fujimodRun = \"" ++ sdk ++ "/share/fujimod-xm1/xm1-150.json\""
    ])
  let sources = [(relative, B.pack [fromIntegral index, 0]) | (index, relative) <- zip [1 :: Int ..] renderSourceIdentityPaths]
  forM_ sources $ \(relative, bytes) -> do
    let path = directory </> relative
    createDirectoryIfMissing True (takeDirectory path)
    B.writeFile path bytes
  BL.writeFile sourceIdentityPath (J.encode (J.object [K.fromString relative J..= digestBytes bytes | (relative, bytes) <- sources]) <> "\n")
  let config = J.object
        [ "version" J..= (1 :: Word32)
        , "role" J..= role
        , "sources" J..= [ J.object ["kind" J..= ("retained" :: T.Text), "start" J..= (4096 :: Word32), "sha256" J..= digestBytes retainedBytes, "path" J..= retainedPath]
                        , J.object ["kind" J..= ("owned" :: T.Text), "start" J..= (0xc40000 :: Word32), "sha256" J..= digestBytes code, "path" J..= textPath]
                        ]
        , "entries" J..= [ J.object ["kind" J..= ("tree" :: T.Text), "address" J..= (0x1164e1c :: Word32), "frame_depth" J..= (16 :: Word32)]
                         , J.object ["kind" J..= ("picker" :: T.Text), "address" J..= (0x1163f2c :: Word32), "frame_depth" J..= (32 :: Word32)]
                         , J.object ["kind" J..= ("pane" :: T.Text), "address" J..= (0x11662a0 :: Word32), "frame_depth" J..= (16 :: Word32)]
                         ]
        , "returns" J..= ([0x1167a60] :: [Word32])
        , "retained_services" J..= ([0x43560] :: [Word32])
        ]
  BL.writeFile configPath (J.encode config <> "\n")
  let session = J.object
        [ "checkpoint" J..= J.object ["directory" J..= (directory </> ("checkpoint-" ++ replicate 64 'a'))]
        , "inputs" J..= J.object
            [ "installed_dat" J..= digestBytes firmwareBytes
            , "adjustment_nand" J..= digestBytes nandBytes
            , "boot_rom" J..= digestBytes bootRomBytes
            , "run_document" J..= digestBytes runBytes
            , "model" J..= ("model" :: T.Text)
            , "procedure" J..= ("procedure" :: T.Text)
            ]
        ]
  let observationBytes = "{\"phase\":\"render-baseline\",\"trace_hits\":[]}\n" :: B.ByteString
      observationRef = J.object
        [ "phase" J..= ("render-baseline" :: T.Text), "path" J..= ("e2e-observation-0.json" :: FilePath)
        , "sha256" J..= digestBytes observationBytes, "bytes" J..= (fromIntegral (B.length observationBytes) :: Integer)
        , "trace_hits" J..= J.object
            [ "schema" J..= (1 :: Int), "path" J..= ("e2e-observation-0.json" :: FilePath)
            , "sha256" J..= digestBytes observationBytes, "count" J..= (0 :: Int) ]
        ]
      result = J.object
        [ "outcome" J..= (case role of StockRole -> "candidate-qualified"; OwnedRole -> "qualified-scoped-lifetime-diagnostic" :: T.Text)
        , "trajectory_failure" J..= (Nothing :: Maybe T.Text)
        , "forbidden_execution" J..= (Nothing :: Maybe T.Text)
        , "observations" J..= [observationRef]
        , "phases" J..= [J.object ["name" J..= ("segment render-baseline" :: T.Text)]]
        ]
  let verified = role == OwnedRole
      variant = if verified then "modal75,menu-engine,render" else "standard"
      publications = if verified then 228 else 223 :: Int
      entries = if verified then [J.object ["controller" J..= (75 :: Word32)]] else []
  let modal = J.object
        [ "task_identity_verified" J..= verified
        , "whole_installed_lifetime_coverage" J..= True
        , "request" J..= J.object
            [ "lifetime" J..= J.object
                [ "bank_variant" J..= (variant :: T.Text)
                , "publications" J..= replicate publications (1 :: Word32)
                ]
            ]
        , "permit_end" J..= J.object
            [ "task_entries" J..= entries
            , "lifetime" J..= J.object ["publication_count" J..= (publications :: Int)]
            ]
        ]
  let diagnostic = J.object
        [ "scope" J..= ("installed-before-unload" :: T.Text)
        , "diagnostic_only" J..= True
        , "whole_route_stack_minimum_proven" J..= False
        , "coverage" J..= ("continuous" :: T.Text)
        , "finished" J..= True
        , "failure" J..= (Nothing :: Maybe T.Text)
        , "active_invocations" J..= (0 :: Word32)
        , "incomplete_intervals" J..= False
        , "role" J..= role
        ]
  let request = case config of
        J.Object fields -> J.Object (KM.insert "command" (J.String "render-stack-diagnostic-begin") fields)
        _ -> config
  let manifest = J.object ["bytes" J..= (32 :: Word32)]
  BL.writeFile (receipt </> "e2e-result.json") (J.encode result <> "\n")
  BL.writeFile (receipt </> "emulator-session.json") (J.encode session <> "\n")
  BL.writeFile (receipt </> "e2e-modal-observation.json") (J.encode modal <> "\n")
  BL.writeFile (receipt </> "e2e-render-stack-diagnostic.json") (J.encode diagnostic <> "\n")
  BL.writeFile (receipt </> "e2e-render-stack-diagnostic-request.json") (J.encode request <> "\n")
  BL.writeFile (receipt </> "e2e-modal-watch-final.json") "{}\n"
  B.writeFile (receipt </> "e2e-observation-0.json") observationBytes
  BL.writeFile (receipt </> "relinked/kernel.elf") (BL.fromStrict elfBytes)
  BL.writeFile (receipt </> "relinked/ram-push-manifest.json") (J.encode manifest <> "\n")
  BL.writeFile (receipt </> "relinked/runtime.bin") (BL.fromStrict (B.pack [18, 19]))
  trajectorySha <- digestFile trajectoryPath
  configSha <- digestFile configPath
  archiveSha <- digestFile archivePath
  prelinkedPath <- if role == OwnedRole then do
    let prelinked = directory </> "prelinked"
        proofVariant = "modal75,menu-engine,render" :: T.Text
        prelinkedManifest sourceIdentityBytes = J.object
          [ "base" J..= ("0x00c40000" :: T.Text)
          , "size" J..= (32 :: Word32)
          , "bank_variant" J..= proofVariant
          , "runtime_sha256" J..= digestBytes (B.pack [18, 19])
          , "source_identity" J..= J.object
              [ "rust_object_replacement_bank" J..= archiveSha
              , "render_sources_sha256" J..= digestBytes sourceIdentityBytes ]
          ]
    sourceIdentityBytes <- B.readFile sourceIdentityPath
    let manifestBytes = BL.toStrict (J.encode (prelinkedManifest sourceIdentityBytes) <> "\n")
        inputBytes = B.pack [21, 22]
        report = J.object
          [ "result" J..= ("pass" :: T.Text), "functions" J..= (1 :: Int), "instructions" J..= (1 :: Int)
          , "proven_stores" J..= (0 :: Int), "proven_branches" J..= (0 :: Int)
          , "violations" J..= ([] :: [J.Value]), "declared" J..= ([] :: [J.Value])
          , "unused_declarations" J..= ([] :: [T.Text])
          ]
        reportBytes = BL.toStrict (J.encode report <> "\n")
        proof = J.object
          [ "schema" J..= (1 :: Word32), "analysis" J..= ("bank-allowlist-check-r1" :: T.Text)
          , "checker_sha256" J..= digestBytes checkerBytes, "bank_variant" J..= proofVariant, "policy" J..= ("enforce" :: T.Text)
          , "manifest_sha256" J..= digestBytes manifestBytes, "runtime_sha256" J..= digestBytes (B.pack [18, 19])
          , "kernel_elf_sha256" J..= digestBytes elfBytes, "archive_sha256" J..= archiveSha
          , "allowlist_input_sha256" J..= digestBytes inputBytes, "report_sha256" J..= digestBytes reportBytes
          ]
        proofBytes = BL.toStrict (J.encode proof <> "\n")
    createDirectoryIfMissing True prelinked
    B.writeFile (prelinked </> "ram-push-manifest.json") manifestBytes
    BL.writeFile (prelinked </> "runtime.bin") (BL.fromStrict (B.pack [18, 19]))
    BL.writeFile (prelinked </> "kernel.elf") (BL.fromStrict elfBytes)
    BL.writeFile (prelinked </> "modal-scope.json") "{}\n"
    BL.writeFile (prelinked </> "bank-allowlist-input.json") (BL.fromStrict inputBytes)
    B.writeFile (prelinked </> "bank-allowlist-report.json") reportBytes
    B.writeFile (prelinked </> "bank-allowlist-proof.json") proofBytes
    B.writeFile (receipt </> "relinked/ram-push-manifest.json") manifestBytes
    B.writeFile (receipt </> "relinked/bank-allowlist.json") reportBytes
    B.writeFile (receipt </> "relinked/bank-allowlist-proof.json") proofBytes
    pure (Just prelinked)
    else pure Nothing
  let expected = Expected
        { expectedName = T.pack (takeFileName receipt)
        , expectedRole = role
        , expectedSourceCommit = "2ef924c6"
        , expectedHeadCommit = "13d92acd"
        , expectedSdk = sdk
        , expectedFujiModPaths = pathsPath
        , expectedTrajectory = trajectoryPath
        , expectedTrajectorySha = trajectorySha
        , expectedFirmware = firmwarePath
        , expectedNand = nandPath
        , expectedBootRom = bootRomPath
        , expectedRunDocument = runPath
        , expectedConfig = configPath
        , expectedConfigSha = configSha
        , expectedArchive = if role == OwnedRole then Just archivePath else Nothing
        , expectedArchiveSha = if role == OwnedRole then Just archiveSha else Nothing
        , expectedSourceIdentity = if role == OwnedRole then Just sourceIdentityPath else Nothing
        , expectedChecker = if role == OwnedRole then Just checkerPath else Nothing
        , expectedPrelinked = prelinkedPath
        , expectedPrelinkedBase = if role == OwnedRole then Just 0xc40000 else Nothing
        , expectedPrelinkedSize = if role == OwnedRole then Just 32 else Nothing
        , expectedPins = Nothing
        }
  pure (Fixture directory receipt expected)

mutateAndVerify :: Fixture -> FilePath -> String -> (B.ByteString -> B.ByteString) -> IO (Either String ReceiptIdentity)
mutateAndVerify fixture receipt name change = do
  let path = receipt </> name
  bytes <- B.readFile path
  B.writeFile path (change bytes)
  verifyReceipt (fixtureExpected fixture) receipt

partialReceipt :: Fixture -> IO (Either String ReceiptIdentity)
partialReceipt fixture = do
  let copy = fixtureRoot fixture </> "partial"
  copyTreeForTest (fixtureReceipt fixture) copy
  B.writeFile (copy </> "e2e-observation-0.json") ""
  removeIfPresent (copy </> "e2e-observation-0.json")
  bindReceipt (fixtureExpected fixture) copy GuestRun

copyTreeForTest :: FilePath -> FilePath -> IO ()
copyTreeForTest source destination = do
  createDirectoryIfMissing True destination
  entries <- listDirectory source
  forM_ entries $ \name -> do
    let from = source </> name
        to = destination </> name
    directory <- doesDirectoryExist from
    if directory
      then copyTreeForTest from to
      else B.readFile from >>= B.writeFile to

wrongTrajectoryDigest :: Fixture -> FilePath -> IO (Either String ReceiptIdentity)
wrongTrajectoryDigest fixture receipt = do
  wrong <- pure (parseDigest (T.replicate 64 "0"))
  case wrong of
    Left problem -> ioError (userError problem)
    Right digest -> bindReceipt ((fixtureExpected fixture) { expectedTrajectorySha = digest }) receipt GuestRun

wrongRoleReceipt :: Fixture -> FilePath -> IO (Either String ReceiptIdentity)
wrongRoleReceipt fixture receipt = do
  let expected = fixtureExpected fixture
  bindReceipt (expected { expectedRole = otherRole (expectedRole expected) }) receipt GuestRun
 where
  otherRole StockRole = OwnedRole
  otherRole OwnedRole = StockRole

badSourceIdentity :: Fixture -> String -> IO (Either String ())
badSourceIdentity fixture contents = do
  let path = fixtureRoot fixture </> "bad-source-identity.json"
  writeFile path contents
  verifyInputs ((fixtureExpected fixture) { expectedSourceIdentity = Just path })

badPrelinked :: Fixture -> (FilePath -> IO ()) -> IO (Either String ())
badPrelinked fixture mutate = case fixtureExpected fixture of
  expected | Just directory <- expectedPrelinked expected -> do
    let copy = fixtureRoot fixture </> "bad-prelinked"
    copyTreeForTest directory copy
    mutate copy
    verifyInputs expected { expectedPrelinked = Just copy }
  _ -> pure (Right ())

badPrelinkedFile :: Fixture -> FilePath -> (B.ByteString -> B.ByteString) -> IO (Either String ())
badPrelinkedFile fixture relative change = badPrelinked fixture $ \directory -> do
  let path = directory </> relative
  bytes <- B.readFile path
  B.writeFile path (change bytes)

mutateProofAnalysis :: B.ByteString -> B.ByteString
mutateProofAnalysis bytes = replaceObjectField bytes "analysis" (J.String "bank-allowlist-check-r2")

mutateProofChecker :: B.ByteString -> B.ByteString
mutateProofChecker bytes = replaceObjectField bytes "checker_sha256" (J.String (T.replicate 64 "f"))

mutateReportFailure :: B.ByteString -> B.ByteString
mutateReportFailure bytes = replaceObjectField bytes "result" (J.String "fail")

replaceObjectField :: B.ByteString -> T.Text -> J.Value -> B.ByteString
replaceObjectField bytes name value = case J.eitherDecodeStrict' bytes of
  Right (J.Object fields) -> BL.toStrict (J.encode (J.Object (KM.insert (K.fromText name) value fields)) <> "\n")
  _ -> bytes

badPrelinkedFailingReport :: Fixture -> IO (Either String ())
badPrelinkedFailingReport fixture = badPrelinked fixture $ \directory -> do
  let reportPath = directory </> "bank-allowlist-report.json"
      proofPath = directory </> "bank-allowlist-proof.json"
  reportBytes <- B.readFile reportPath
  let failing = mutateReportFailure reportBytes
  B.writeFile reportPath failing
  proofBytes <- B.readFile proofPath
  B.writeFile proofPath (replaceObjectField proofBytes "report_sha256" (J.toJSON (digestBytes failing)))

badArchive :: Fixture -> IO (Either String ())
badArchive fixture = case expectedArchive (fixtureExpected fixture) of
  Just archive -> do
    let copy = fixtureRoot fixture </> "bad-archive.a"
    bytes <- B.readFile archive
    B.writeFile copy (bytes <> B.singleton 0)
    verifyInputs ((fixtureExpected fixture) { expectedArchive = Just copy })
  Nothing -> pure (Right ())

badSourceIdentityDigest :: Fixture -> IO (Either String ())
badSourceIdentityDigest fixture = case expectedSourceIdentity (fixtureExpected fixture) of
  Just _ -> do
    let copy = fixtureRoot fixture </> "bad-source-digest.json"
        wrong = BL.toStrict (J.encode (J.object [K.fromString (fixtureRoot fixture </> "text.bin") J..= digestBytes (B.pack [0, 0])]) <> "\n")
    B.writeFile copy wrong
    verifyInputs ((fixtureExpected fixture) { expectedSourceIdentity = Just copy })
  Nothing -> pure (Right ())

observationListControl :: Fixture -> FilePath -> Maybe [J.Value] -> IO (Either String ReceiptIdentity)
observationListControl fixture receipt change = do
  let copy = fixtureRoot fixture </> ("observation-list-" ++ maybe "missing" (const "empty") change)
  copyTreeForTest receipt copy
  removeIfPresent (copy </> "admission.json")
  let path = copy </> "e2e-result.json"
  bytes <- B.readFile path
  case J.eitherDecodeStrict' bytes of
    Right (J.Object fields) -> do
      let updated = case change of
            Nothing -> KM.delete "observations" fields
            Just rows -> KM.insert "observations" (J.toJSON rows) fields
      BL.writeFile path (J.encode (J.Object updated) <> "\n")
    _ -> pure ()
  bindReceipt (fixtureExpected fixture) copy GuestRun

badSourceIdentityBytes :: Fixture -> IO (Either String ())
badSourceIdentityBytes fixture = case expectedSourceIdentity (fixtureExpected fixture) of
  Nothing -> pure (Right ())
  Just path -> do
    let copy = fixtureRoot fixture </> "unbound-source-sidecar.json"
    bytes <- B.readFile path
    B.writeFile copy (bytes <> " ")
    verifyInputs ((fixtureExpected fixture) { expectedSourceIdentity = Just copy })

mutateRenderSources :: B.ByteString -> B.ByteString
mutateRenderSources bytes = case J.eitherDecodeStrict' bytes of
  Right (J.Object fields) -> case KM.lookup "source_identity" fields of
    Just (J.Object source) -> BL.toStrict (J.encode (J.Object (KM.insert "source_identity"
      (J.Object (KM.insert "render_sources_sha256" (J.String (T.replicate 64 "f")) source)) fields)) <> "\n")
    _ -> bytes
  _ -> bytes

mutatePrelinkedManifest :: FilePath -> IO ()
mutatePrelinkedManifest directory = do
  let path = directory </> "ram-push-manifest.json"
  bytes <- B.readFile path
  case J.eitherDecodeStrict' bytes of
    Right (J.Object fields) -> BL.writeFile path (J.encode (J.Object (KM.insert "runtime_sha256" (J.String (T.replicate 64 "0")) fields)) <> "\n")
    _ -> pure ()

importSynthetic :: FilePath -> String -> Fixture -> IO Bool
importSynthetic root label fixture = do
  let directory = root </> label </> "packet"
      source = directory </> "source"
      destination = directory </> "imported"
      packetPath = directory </> "review-packet.json"
      terminalPath = directory </> "terminal.json"
  createDirectoryIfMissing True source
  copyTreeForTest (fixtureReceipt fixture) source
  BL.writeFile terminalPath "{\"passed\":true}\n"
  resultSha <- digestFile (source </> "e2e-result.json")
  diagnosticSha <- digestFile (source </> "e2e-render-stack-diagnostic.json")
  sessionSha <- digestFile (source </> "emulator-session.json")
  terminalSha <- digestFile terminalPath
  let expected = fixtureExpected fixture
      packet = J.object
        [ "passed" J..= True
        , "source_commit" J..= ("2ef924c6" :: T.Text)
        , "sdk_package" J..= expectedSdk expected
        , "trajectory" J..= J.object ["path" J..= expectedTrajectory expected, "sha256" J..= expectedTrajectorySha expected]
        , "config" J..= J.object ["path" J..= expectedConfig expected, "sha256" J..= expectedConfigSha expected]
        , "terminal" J..= J.object ["path" J..= terminalPath, "sha256" J..= terminalSha]
        , "evidence" J..= J.object
            [ "result" J..= J.object ["path" J..= (source </> "e2e-result.json"), "sha256" J..= resultSha]
            , "diagnostic" J..= J.object ["path" J..= (source </> "e2e-render-stack-diagnostic.json"), "sha256" J..= diagnosticSha]
            , "session" J..= J.object ["path" J..= (source </> "emulator-session.json"), "sha256" J..= sessionSha]
            ]
        ]
  BL.writeFile packetPath (J.encode packet <> "\n")
  imported <- importReceipt expected packetPath source destination
  ok <- accepts (label ++ " review-packet import admits the preserved receipt") imported
  reused <- verifyReceipt expected destination
  reusedOk <- accepts (label ++ " imported receipt reuses") reused
  before <- digestFile (source </> "e2e-result.json")
  nested <- importReceipt expected packetPath source (source </> "nested-import")
  nestedOk <- expectLeft (label ++ " nested import destination refuses") nested
  let alias = directory </> "alias-source"
  createDirectoryLink source alias
  aliased <- importReceipt expected packetPath source (alias </> "imported")
  aliasedOk <- expectLeft (label ++ " aliased import destination refuses") aliased
  createDirectoryLink (fixtureRoot fixture) (source </> "escape")
  escaped <- importReceipt expected packetPath source (directory </> "escaped-import")
  escapedOk <- expectLeft (label ++ " symlinked source tree refuses") escaped
  after <- digestFile (source </> "e2e-result.json")
  untouched <- check (label ++ " refused imports leave the source untouched") (before == after)
  pure (ok && reusedOk && nestedOk && aliasedOk && escapedOk && untouched)
