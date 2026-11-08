{-# LANGUAGE OverloadedStrings #-}

-- | The emulator e2e's unreviewed RAM-push path: the runfiles relinker and
-- toolchain without the compiled source-identity and publication-contract
-- pins. Only //emulation links this module; //transport:camera does not.
module Xm1UnreviewedRelink
  ( withUnreviewedBuild
  , withUnreviewedBuildArchive
  , withPrelinkedBuild
  , runUnreviewedRamPush
  , runUnreviewedRamUnload
  ) where

import BankAllowlist (Report, allowlistAnalysisIdentity, violations)
import BankAllowlistComposition (bankAllowlistSourcePaths, composeBankAllowlist)
import Control.Monad (forM, unless)
import qualified Data.Aeson as J
import qualified Data.ByteString as B
import Data.Char (isHexDigit)
import Emulation.Sha256 (Sha256, parseSha256Hex, sha256Hex)
import FujiAF.PlanDocument (privateRegular)
import FujiAF.RamPush
import FujiAF.SHA256 (sha256)
import FujiAF.Transport (Clock, UsbApi)
import RamPushLayout (bankVariantTag)
import System.Directory (getCurrentDirectory)
import System.FilePath ((</>))

withUnreviewedBuild :: Payload -> AllowlistPolicy -> Address -> (PinnedBuild -> IO a) -> IO a
withUnreviewedBuild payload policy base action = do
  relinker <- runfilesRelinker payload
  withRelinkedBuildUsing relinker policy base action

-- | As 'withUnreviewedBuild' with the bank's Rust archive replaced; the
-- payload must link the bank.
withUnreviewedBuildArchive :: FilePath -> Payload -> AllowlistPolicy -> Address -> (PinnedBuild -> IO a) -> IO a
withUnreviewedBuildArchive archive payload policy base action = do
  relinker <- runfilesRelinker payload
  rust <- maybe (ioError (userError "the selected payload links no Rust bank archive")) pure (relinkerRust relinker)
  withRelinkedBuildUsing relinker { relinkerRust = Just rust { rustBankArchive = archive } } policy base action

data PrelinkedProof = PrelinkedProof
  { proofAnalysis :: String
  , proofChecker :: String
  , proofVariant :: String
  , proofPolicy :: String
  , proofManifest :: String
  , proofRuntime :: String
  , proofElf :: String
  , proofArchive :: String
  , proofInput :: String
  , proofReport :: String
  }

instance J.FromJSON PrelinkedProof where
  parseJSON = J.withObject "prelinked allowlist proof" $ \fields ->
    PrelinkedProof <$> fields J..: "analysis" <*> fields J..: "checker_sha256" <*> fields J..: "bank_variant"
      <*> fields J..: "policy" <*> fields J..: "manifest_sha256" <*> fields J..: "runtime_sha256"
      <*> fields J..: "kernel_elf_sha256" <*> fields J..: "archive_sha256" <*> fields J..: "allowlist_input_sha256"
      <*> fields J..: "report_sha256"

withPrelinkedBuild :: FilePath -> Sha256 -> Payload -> AllowlistPolicy -> Address -> (PinnedBuild -> IO a) -> IO a
withPrelinkedBuild directory archiveDigest payload policy base action = do
  let manifestPath = directory </> "ram-push-manifest.json"
  document <- privateRegular "prelinked RAM-push manifest" manifestPath (512 * 1024)
  runtime <- privateRegular "prelinked RAM-push runtime" (directory </> "runtime.bin") (512 * 1024)
  reviewed <- parseRamPushManifest manifestPath document runtime
  unless (allocationBase reviewed == base)
    (ioError (userError "prelinked manifest base differs from the allocation base"))
  unless (allocationSize reviewed == payloadAllocationSize payload)
    (ioError (userError "prelinked manifest size differs from the payload"))
  unless (manifestBankVariant reviewed == payloadBankVariant payload)
    (ioError (userError "prelinked manifest bank variant differs from the payload"))
  storedArchive <- maybe
    (ioError (userError "prelinked manifest archive digest is missing"))
    (either (ioError . userError) pure . parseSha256Hex)
    (lookup "rust_object_replacement_bank" (sourceIdentity reviewed))
  unless (storedArchive == archiveDigest)
    (ioError (userError "prelinked manifest archive differs from the selected archive"))
  variant <- maybe (ioError (userError "prelinked manifest has no bank variant")) pure (payloadBankVariant payload)
  unless (policy == EnforceAllowlist) (ioError (userError "prelinked proof requires EnforceAllowlist"))
  proofBytes <- B.readFile (directory </> "bank-allowlist-proof.json")
  proof <- either (ioError . userError . ("prelinked allowlist proof: " ++)) pure (J.eitherDecodeStrict' proofBytes)
  unless (proofAnalysis proof == allowlistAnalysisIdentity)
    (ioError (userError "prelinked allowlist proof analysis differs"))
  unless (proofChecker proof /= "" && length (proofChecker proof) == 64 && all isHexDigit (proofChecker proof))
    (ioError (userError "prelinked allowlist proof checker identity is invalid"))
  unless (proofPolicy proof == "enforce") (ioError (userError "prelinked allowlist proof policy differs"))
  unless (proofVariant proof == bankVariantTag variant) (ioError (userError "prelinked allowlist proof bank variant differs"))
  elf <- privateRegular "prelinked kernel.elf" (directory </> "kernel.elf") (4 * 1024 * 1024)
  input <- privateRegular "prelinked bank allowlist input" (directory </> "bank-allowlist-input.json") (1024 * 1024)
  reportBytes <- privateRegular "prelinked bank allowlist report" (directory </> "bank-allowlist-report.json") (1024 * 1024)
  unless (proofManifest proof == sha256 document) (ioError (userError "prelinked proof manifest identity differs"))
  unless (proofRuntime proof == sha256 runtime) (ioError (userError "prelinked proof runtime identity differs"))
  unless (proofElf proof == sha256 elf) (ioError (userError "prelinked proof kernel ELF identity differs"))
  unless (proofArchive proof == sha256Hex archiveDigest) (ioError (userError "prelinked proof archive identity differs"))
  unless (proofInput proof == sha256 input) (ioError (userError "prelinked proof allowlist input identity differs"))
  unless (proofReport proof == sha256 reportBytes) (ioError (userError "prelinked proof report identity differs"))
  root <- getCurrentDirectory
  sources <- forM (bankAllowlistSourcePaths variant) $ \path -> do
    bytes <- B.readFile (root </> path)
    unless (B.length bytes <= 1024 * 1024) (ioError (userError "bank allowlist source exceeds its size bound"))
    unless (lookup path (sourceIdentity reviewed) == Just (sha256 bytes))
      (ioError (userError "bank allowlist differs from the manifest's source identity"))
    pure (path, bytes)
  expected <- either (ioError . userError . ("bank allowlist composition failed: " ++)) pure (composeBankAllowlist variant sources)
  unless (input == expected) (ioError (userError "bank allowlist output differs from the selected source composition"))
  unless (lookup composedAllowlistIdentity (sourceIdentity reviewed) == Just (sha256 input))
    (ioError (userError "composed bank allowlist differs from the manifest's source identity"))
  report <- either (ioError . userError . ("prelinked allowlist report: " ++)) pure (J.eitherDecodeStrict' reportBytes :: Either String Report)
  unless (null (violations report)) (ioError (userError "prelinked allowlist report has violations under EnforceAllowlist"))
  action (PinnedBuild reviewed document runtime directory (AllowlistPassed report))

runUnreviewedRamPush :: RamPushObserver -> RetainedAllocation -> RamPushManifest -> Clock -> UsbApi -> FilePath -> IO RamPushOutcome
runUnreviewedRamPush observer retained = runRamPushWith xm1PeerCacheAvailability observer (retainedAllocation retained)

runUnreviewedRamUnload :: RamPushState -> Clock -> UsbApi -> FilePath -> IO RamUnloadOutcome
runUnreviewedRamUnload = runRamUnloadWith xm1PeerCacheAvailability
