-- Runs the pinned RAM-push builder at the lowest and worst-case bases and
-- requires each manifest to pass the compiled reviewed contract. Run it with
-- `bazel run`; the builder comes from this binary's runfiles and the private
-- inputs from BUILD_WORKSPACE_DIRECTORY. The remaining cases run from a fresh
-- directory outside the workspace with a USB provider that fails every call.
-- Selects bank+D3 by default, or the payload named by its leading flag.
module Main where

import Control.Monad (forM, forM_, unless, when)
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import Data.Bits (shiftR, xor)
import Data.IORef (IORef, newIORef, readIORef, writeIORef)
import Data.Char (isDigit)
import Data.List (isInfixOf)
import Numeric (showHex)
import System.Directory (createDirectory, getTemporaryDirectory, makeAbsolute, removeDirectoryRecursive, setCurrentDirectory)
import System.Environment (getArgs)
import System.Exit (exitFailure)
import System.FilePath ((</>))
import System.Posix.Files (setFileMode)
import System.Posix.Process (getProcessID)
import Control.Exception (SomeException, displayException, try)
import FujiAF.RamPush
import FujiAF.SHA256 (sha256)
import FujiAF.Transport (UsbApi(..), realClock, writePrivateBytes)
import RamPushToolchain (RamPushToolchain(..))
import RamPushReviewedPins (reviewedContract)
import RamPushCcReviewedPins (reviewedCcContract)
import RamPushCcIconReviewedPins (reviewedCcIconContract)
import RamPushToggleReviewedPins (reviewedToggleContract)

-- | The payload under check, its compiled contract, the other payload and
-- its contract, and the label its acceptance lines carry.
data Target = Target Payload ReviewedContract Payload ReviewedContract String

targetFor :: [String] -> IO Target
targetFor arguments = case arguments of
  [] -> pure (Target bankD3Payload reviewedContract classicChromePayload reviewedCcContract "reviewed contract")
  ["--classic-chrome"] -> pure (Target classicChromePayload reviewedCcContract bankD3Payload reviewedContract "reviewed CC contract")
  ["--classic-chrome-icon"] -> pure (Target classicChromeIconPayload reviewedCcIconContract classicChromePayload reviewedCcContract "reviewed CC icon contract")
  ["--usb-toggle"] -> pure (Target usbTogglePayload reviewedToggleContract classicChromePayload reviewedCcContract "reviewed USB toggle contract")
  _ -> putStrLn "usage: ram-push-reviewed-build-check [--classic-chrome|--classic-chrome-icon|--usb-toggle]" >> exitFailure

failing :: String -> IO a
failing what = ioError (userError ("refusal case attempted USB "++what))

data UsbAccess = NotAttempted | Attempted deriving (Eq, Show)

-- | Fails every call but the cleanup exit, so a run that reaches USB ends refused at init.
noUsb :: IORef UsbAccess -> UsbApi
noUsb access = UsbApi (\_ -> writeIORef access Attempted >> failing "init") (\_ -> failing "enumeration") (\_ _ -> failing "open") (\_ _ -> failing "claim")
  (\_ _ -> failing "transfer") (\_ _ -> failing "release") (\_ -> failing "close") (\_ -> pure ())

privateDirectory :: FilePath -> IO FilePath
privateDirectory path = createDirectory path >> setFileMode path 0o700 >> pure path

expect :: String -> String -> Either String String -> IO ()
expect label wanted outcome = case outcome of
  Left reason | wanted `isInfixOf` reason -> putStrLn (label++": refused as expected: "++reason)
  other -> putStrLn (label++": expected a refusal containing "++show wanted++", got "++show other) >> exitFailure

pushOutcome :: RamPushOutcome -> Either String String
pushOutcome outcome = case outcome of
  RamPushRefused reason -> Left reason
  other -> Right (show other)

-- | Replaces the first occurrence of `needle` after the first occurrence of `anchor`.
replaceAfter :: B.ByteString -> B.ByteString -> B.ByteString -> B.ByteString -> Maybe B.ByteString
replaceAfter anchor needle replacement document = do
  let (before,rest) = B.breakSubstring anchor document
  unless (B.isPrefixOf anchor rest) Nothing
  let (middle,found) = B.breakSubstring needle rest
  unless (B.isPrefixOf needle found) Nothing
  pure (B.concat [before, middle, replacement, B.drop (B.length needle) found])

expectAccess :: IORef UsbAccess -> UsbAccess -> IO ()
expectAccess access expected = do
  actual <- readIORef access
  unless (actual == expected) $ do
    putStrLn ("expected USB access " ++ show expected ++ ", got " ++ show actual)
    exitFailure

requireJust :: String -> Maybe a -> IO a
requireJust what = maybe (putStrLn ("cannot construct "++what) >> exitFailure) pure

main :: IO ()
main = do
  Target payload contract otherPayload otherContract label <- getArgs >>= targetFor
  let (lowestBase, worstBase) = payloadReferenceBases payload
  builds <- forM [lowestBase, worstBase] $ \base -> do
    document <- withPinnedReviewedBuild contract payload (Address base) $ \built -> do
      let manifest = pinnedManifest built
      validateReviewedContract contract manifest
      unless (allocationBase manifest == Address base
          && allocationSize manifest == payloadAllocationSize payload
          && length (manifestPublications manifest) == payloadPublicationCount payload
          && manifestEntries manifest == payloadEntries payload (Address base)) $ do
        putStrLn ("pinned manifest does not describe base 0x" ++ showHex base "")
        exitFailure
      when (payloadKind payload == UsbToggle) $ do
        let publications = manifestPublications manifest
            setup = take 22 publications
            stub = B.pack [fromIntegral (unWordValue (publicationNew p) `shiftR` shift) | p <- setup, shift <- [0,8,16,24]]
        unless (allocationSize manifest == 0xd20
            && map publicationAddress publications == map Address ([0x00bf4454,0x00bf4458..0x00bf44a8] ++ [0x00ecf900])
            && map publicationPhase publications == replicate 22 Setup ++ [Activation]
            && sha256 stub == "35a7e9597f06bdc1dda250d991df4a468d2401e5ae3c24c22e37403318746d62"
            && publicationOld (last publications) == WordValue 0xebdf3ffd
            && publicationNew (last publications) == WordValue 0xebf492d3) $ do
          putStrLn "USB toggle fixed stub or activation contract differs"
          exitFailure
      pure (pinnedDocument built, pinnedRuntime built)
    putStrLn (label ++ " accepted at 0x" ++ showHex base "")
    when (payloadKind payload == UsbToggle) $
      putStrLn ("runtime SHA256 " ++ sha256 (snd document) ++ "; manifest SHA256 " ++ sha256 (fst document))
    pure document
  (otherBaseDocument, otherBaseRuntime) <- case builds of
    [_, other] -> pure other
    _ -> exitFailure
  -- Bases whose bank allocation would cover the image-5 mode-apply publication sites.
  forM_ (if payloadKind payload == RecipeBankD3 then [0x00c20000, 0x00c21c88] else []) $ \base -> do
    outcome <- try (withPinnedReviewedBuild contract payload (Address base) (pure . pinnedManifest))
    expect ("pinned build of an allocation at 0x" ++ showHex base "" ++ " over publication sites") "pinned RAM-push builder failed before USB"
      (case outcome of Left (failure :: SomeException) -> Left (displayException failure); Right _ -> Right "accepted")

  pid <- getProcessID
  temporary <- getTemporaryDirectory >>= makeAbsolute
  root <- privateDirectory (temporary </> ("ram-push-reviewed-build-check-"++show pid))
  setCurrentDirectory root
  let base = Address lowestBase
  PinnedBuild reviewed document runtime _ _ <- withPinnedReviewedBuild contract payload base pure

  substituted <- privateDirectory (root </> "substituted-tool")
  relinker <- runfilesRelinker payload
  let tools = relinkerToolchain relinker
      wrapper = substituted </> "clang"
  writePrivateBytes wrapper (C.pack ("#!"++toolBash tools++"\nexec "++toolClang tools++" \"$@\"\n"))
  setFileMode wrapper 0o700
  substitutedBuild <- try (withPinnedReviewedBuildUsing contract relinker { relinkerToolchain = tools { toolClang = wrapper } } base (pure . pinnedManifest))
  expect "pinned build with a substituted clang" "RAM_PUSH_CLANG is not the compiled toolchain path"
    (case substitutedBuild of Left (failure::SomeException) -> Left (displayException failure); Right _ -> Right "accepted")

  forM_ (relinkerRust relinker) $ \rust -> do
    changed <- privateDirectory (root </> "changed-rust")
    archive <- B.readFile (rustBankArchive rust)
    facts <- B.readFile (rustBankBuildFacts rust)
    -- Byte 24 is the first archive member's mtime field, which the linker does not read.
    unless (C.take 8 archive == C.pack "!<arch>\n" && C.length archive > 24 && isDigit (C.index archive 24)) $ do
      putStrLn "the bank's Rust archive has no ar member header"
      exitFailure
    let changedArchive = changed </> "bank.a"
        changedFacts = changed </> "build-facts.txt"
    writePrivateBytes changedArchive (B.take 24 archive <> C.singleton (if C.index archive 24 == '0' then '1' else '0') <> B.drop 25 archive)
    writePrivateBytes changedFacts (facts <> C.pack "changed\n")
    forM_ [ ("pinned build with one Rust archive byte changed", rust { rustBankArchive = changedArchive })
          , ("pinned build with changed Rust build facts", rust { rustBankBuildFacts = changedFacts }) ] $ \(caseLabel, inputs) -> do
      outcome <- try (withPinnedReviewedBuildUsing contract relinker { relinkerRust = Just inputs } base (pure . pinnedManifest))
      expect caseLabel "manifest source identity differs from the compiled reviewed contract"
        (case outcome of Left (failure::SomeException) -> Left (displayException failure); Right _ -> Right "accepted")

  allocDir <- privateDirectory (root </> "alloc")
  allocPath <- writeAllocStateFile allocDir
    (RamAllocState base (payloadAllocationSize payload) 0x4003 callbackTableAddress callbackStockWord bootstrapBytes (replicate 64 '0') "X-M1" 1)
  allocation <- readRetainedAllocation allocPath

  let pushCase name caseLabel wanted caseDocument caseRuntime caseAllocation caseContract = do
        directory <- privateDirectory (root </> name)
        writePrivateBytes (directory </> "ram-push-manifest.json") caseDocument
        writePrivateBytes (directory </> "runtime.bin") caseRuntime
        parsed <- try (readRamPushManifest (directory </> "ram-push-manifest.json"))
        case parsed of
          Left (failure::SomeException) -> expect caseLabel wanted (Left (displayException failure))
          Right supplied -> do
            pushDir <- privateDirectory (root </> (name++"-push"))
            access <- newIORef NotAttempted
            pushed <- runRamPush caseContract caseAllocation supplied realClock (noUsb access) pushDir
            expect caseLabel wanted (pushOutcome pushed)
            expectAccess access NotAttempted

  pushCase "linked" "ram-push with a byte-perturbed manifest" "supplied manifest differs byte-for-byte from the pinned builder output"
    (document <> C.pack "\n") runtime allocation contract

  let flippedRuntime = B.take 0x100 runtime <> B.singleton (B.index runtime 0x100 `xor` 1) <> B.drop 0x101 runtime
  flippedDocument <- requireJust "the changed-runtime manifest"
    (replaceAfter (C.pack "runtime_sha256") (C.pack (runtimeDigest reviewed)) (C.pack (sha256 flippedRuntime)) document)
  pushCase "changed-runtime" "ram-push with one runtime byte changed and its digest updated" "supplied manifest differs byte-for-byte from the pinned builder output"
    flippedDocument flippedRuntime allocation contract

  forM_ (zip [0 :: Int ..] (sourceIdentity reviewed)) $ \(index, (name, digest)) -> do
    let changedDigest = case digest of
          first : rest -> (if first == '0' then '1' else '0') : rest
          [] -> "0"
        caseName = "source-" ++ show index
    changedIdentity <- requireJust ("the changed source identity for " ++ name)
      (replaceAfter (C.pack ("\"" ++ name ++ "\"")) (C.pack digest) (C.pack changedDigest) document)
    pushCase caseName ("ram-push with substituted source " ++ name) "manifest source identity differs from the compiled reviewed contract"
      changedIdentity runtime allocation contract

  changedNew <- requireJust "the changed-new-word manifest" $ do
    let (before,rest) = B.breakSubstring (C.pack "\"new\"") document
        (middle,word) = B.breakSubstring (C.pack "0x") rest
    digit <- fst <$> C.uncons (C.drop 9 word)
    unless (C.length word >= 10) Nothing
    pure (B.concat [before, middle, C.take 9 word, C.singleton (if digit == '0' then '1' else '0'), C.drop 10 word])
  pushCase "changed-new-word" "ram-push with a changed publication new word" "supplied manifest differs byte-for-byte from the pinned builder output"
    changedNew runtime allocation contract

  changedOld <- requireJust "the changed-old-word manifest" $ do
    let (before,rest) = B.breakSubstring (C.pack "\"old\"") document
        (middle,word) = B.breakSubstring (C.pack "0x") rest
    digit <- fst <$> C.uncons (C.drop 9 word)
    unless (C.length word >= 10) Nothing
    pure (B.concat [before, middle, C.take 9 word, C.singleton (if digit == '0' then '1' else '0'), C.drop 10 word])
  pushCase "changed-old-word" "ram-push with a changed publication old word" "manifest publication contract differs from the compiled reviewed contract"
    changedOld runtime allocation contract

  pushCase "wrong-base" "ram-push of the other reference base's manifest" "manifest base differs from the retained allocation"
    otherBaseDocument otherBaseRuntime allocation contract

  wrongSizeDir <- privateDirectory (root </> "alloc-wrong-size")
  wrongSizePath <- writeAllocStateFile wrongSizeDir
    (RamAllocState base (payloadAllocationSize otherPayload) 0x4003 callbackTableAddress callbackStockWord bootstrapBytes (replicate 64 '0') "X-M1" 1)
  wrongSize <- readRetainedAllocation wrongSizePath
  -- A bank allocation at the Classic Chrome base would cover the bank's own publication sites, so its relink refuses first.
  let wrongSizeRefusal = if payloadKind otherPayload == RecipeBankD3
        then "pinned RAM-push builder failed before USB"
        else "manifest source identity differs from the compiled reviewed contract"
  pushCase "wrong-size" "ram-push into an allocation of the other payload's size" wrongSizeRefusal
    document runtime wrongSize contract

  pushCase "other-contract" "ram-push checked against the other payload's contract" "manifest source identity differs from the compiled reviewed contract"
    document runtime allocation otherContract

  relinkDir <- privateDirectory (root </> "relinked")
  relinkedPath <- runRamRelink contract allocation relinkDir
  relinked <- readRamPushManifest relinkedPath
  relinkPushDir <- privateDirectory (root </> "push-relinked")
  relinkAccess <- newIORef NotAttempted
  relinkedPush <- runRamPush contract allocation relinked realClock (noUsb relinkAccess) relinkPushDir
  expect "ram-push of a ram-relink manifest reaches USB" "refusal case attempted USB" (pushOutcome relinkedPush)
  expectAccess relinkAccess Attempted

  let altered = map (\c -> if c == '0' then '1' else '0') (take 1 (runtimeDigest reviewed)) ++ drop 1 (runtimeDigest reviewed)
      unloadCase name caseLabel wanted state = do
        stateDir <- privateDirectory (root </> name)
        statePath <- writeStateFile stateDir state
        readBack <- readRamPushState statePath
        unloadDir <- privateDirectory (root </> (name++"-unload"))
        access <- newIORef NotAttempted
        unloaded <- runRamUnload contract readBack realClock (noUsb access) unloadDir
        expect caseLabel wanted (case unloaded of RamUnloadRefused reason -> Left reason; other -> Right (show other))
        expectAccess access NotAttempted
      stateWith digest entries = RamPushState (manifestDigest reviewed) digest base (allocationSize reviewed) entries
        (manifestPublications reviewed) callbackStockWord bootstrapBytes 1
      shifted (RamPushEntries install rollback status) = RamPushEntries (Address (unAddress install + 4)) rollback status
  unloadCase "state" "ram-unload with an altered runtime_sha256" "installed state runtime hash differs from the pinned builder output"
    (stateWith altered (manifestEntries reviewed))
  unloadCase "state-entries" "ram-unload with a moved install entry" "installed state entry offsets differ from the compiled reviewed layout"
    (stateWith (runtimeDigest reviewed) (shifted (manifestEntries reviewed)))

  removeDirectoryRecursive root
