-- Runs the pinned RAM-push builder at the lowest and worst-case bases and
-- requires each manifest to pass the compiled reviewed contract. Run it with
-- `bazel run`; the builder comes from this binary's runfiles and the private
-- inputs from BUILD_WORKSPACE_DIRECTORY. The remaining cases run from a fresh
-- directory outside the workspace with a USB provider that fails every call.
module Main where

import Control.Monad (forM_, unless)
import qualified Data.ByteString.Char8 as C
import Data.List (isInfixOf)
import Numeric (showHex)
import System.Directory (createDirectory, getTemporaryDirectory, makeAbsolute, removeDirectoryRecursive, setCurrentDirectory)
import System.Exit (exitFailure)
import System.FilePath ((</>))
import System.Posix.Files (setFileMode)
import System.Posix.Process (getProcessID)
import Control.Exception (SomeException, displayException, try)
import FujiAF.RamPush
import FujiAF.Transport (UsbApi(..), realClock, writePrivateBytes)
import RamPushToolchain (RamPushToolchain(..))
import RamPushReviewedPins (reviewedContract)

failing :: String -> IO a
failing what = ioError (userError ("refusal case attempted USB "++what))

-- | Fails every call but the cleanup exit, so a run that reaches USB ends refused at init.
noUsb :: UsbApi
noUsb = UsbApi (\_ -> failing "init") (\_ -> failing "enumeration") (\_ _ -> failing "open") (\_ _ -> failing "claim")
  (\_ _ -> failing "transfer") (\_ _ -> failing "release") (\_ -> failing "close") (\_ -> pure ())

privateDirectory :: FilePath -> IO FilePath
privateDirectory path = createDirectory path >> setFileMode path 0o700 >> pure path

expect :: String -> String -> Either String String -> IO ()
expect label wanted outcome = case outcome of
  Left reason | wanted `isInfixOf` reason -> putStrLn (label++": refused as expected: "++reason)
  other -> putStrLn (label++": expected a refusal containing "++show wanted++", got "++show other) >> exitFailure

main :: IO ()
main = do
  forM_ [0x00c20000, 0x00c21f88] $ \base -> do
    manifest <- pinnedReviewedManifest reviewedContract bankD3Payload (Address base)
    unless (allocationBase manifest == Address base && length (manifestPublications manifest) == publicationCount) $ do
      putStrLn ("pinned manifest does not describe base 0x" ++ showHex base "")
      exitFailure
    putStrLn ("reviewed contract accepted at 0x" ++ showHex base "")

  pid <- getProcessID
  temporary <- getTemporaryDirectory >>= makeAbsolute
  root <- privateDirectory (temporary </> ("ram-push-reviewed-build-check-"++show pid))
  setCurrentDirectory root
  let base = Address 0x00c20000
  PinnedBuild reviewed document runtime _ _ <- withPinnedReviewedBuild reviewedContract bankD3Payload base pure

  substituted <- privateDirectory (root </> "substituted-tool")
  relinker <- runfilesRelinker bankD3Payload
  let tools = relinkerToolchain relinker
      wrapper = substituted </> "clang"
  writePrivateBytes wrapper (C.pack ("#!"++toolBash tools++"\nexec "++toolClang tools++" \"$@\"\n"))
  setFileMode wrapper 0o700
  substitutedBuild <- try (withPinnedReviewedBuildUsing reviewedContract relinker { relinkerToolchain = tools { toolClang = wrapper } } base (pure . pinnedManifest))
  expect "pinned build with a substituted clang" "RAM_PUSH_CLANG is not the compiled toolchain path"
    (case substitutedBuild of Left (failure::SomeException) -> Left (displayException failure); Right _ -> Right "accepted")

  linked <- privateDirectory (root </> "linked")
  writePrivateBytes (linked </> "ram-push-manifest.json") (document <> C.pack "\n")
  writePrivateBytes (linked </> "runtime.bin") runtime
  allocDir <- privateDirectory (root </> "alloc")
  allocPath <- writeAllocStateFile allocDir
    (RamAllocState base reviewedAllocationSize 0x4003 callbackTableAddress callbackStockWord bootstrapBytes (replicate 64 '0') "X-M1" 1)
  allocation <- readRetainedAllocation allocPath
  supplied <- readRamPushManifest (linked </> "ram-push-manifest.json")
  pushDir <- privateDirectory (root </> "push")
  pushed <- runRamPush reviewedContract allocation supplied realClock noUsb pushDir
  expect "ram-push with a byte-perturbed c20000 manifest" "supplied manifest differs byte-for-byte from the pinned builder output"
    (case pushed of RamPushRefused reason -> Left reason; other -> Right (show other))

  relinkDir <- privateDirectory (root </> "relinked")
  relinkedPath <- runRamRelink reviewedContract allocation relinkDir
  relinked <- readRamPushManifest relinkedPath
  relinkPushDir <- privateDirectory (root </> "push-relinked")
  relinkedPush <- runRamPush reviewedContract allocation relinked realClock noUsb relinkPushDir
  expect "ram-push of a ram-relink manifest reaches USB" "refusal case attempted USB"
    (case relinkedPush of RamPushRefused reason -> Left reason; other -> Right (show other))

  stateDir <- privateDirectory (root </> "state")
  let altered = map (\c -> if c == '0' then '1' else '0') (take 1 (runtimeDigest reviewed)) ++ drop 1 (runtimeDigest reviewed)
  statePath <- writeStateFile stateDir
    (RamPushState (manifestDigest reviewed) altered base (allocationSize reviewed) (manifestEntries reviewed)
      (manifestPublications reviewed) callbackStockWord bootstrapBytes 1)
  state <- readRamPushState statePath
  unloadDir <- privateDirectory (root </> "unload")
  unloaded <- runRamUnload reviewedContract state realClock noUsb unloadDir
  expect "ram-unload with an altered runtime_sha256" "installed state runtime hash differs from the pinned builder output"
    (case unloaded of RamUnloadRefused reason -> Left reason; other -> Right (show other))

  removeDirectoryRecursive root
