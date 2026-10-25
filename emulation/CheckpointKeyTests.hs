{-# LANGUAGE OverloadedStrings #-}
-- | Checkpoint keys over synthetic staged runtimes: which edits keep a key,
-- which change it, and that the runtime root does not enter it.
module Main (main) where

import Control.Monad (forM_, unless)
import Data.Aeson (Value (..), (.=))
import qualified Data.Aeson.Key as K
import Data.IORef (IORef, atomicModifyIORef', newIORef)
import Emulation.CheckpointKey
import System.Directory (createDirectoryIfMissing, getTemporaryDirectory, removeFile)
import System.Environment (lookupEnv)
import System.Exit (exitFailure)
import System.FilePath ((</>), takeDirectory)

-- | A staged runtime's files beneath the trees a launcher copies, plus entries
-- the launcher and the running bridges add beside them.
stagedFiles :: [(FilePath, String)]
stagedFiles =
  [ ("emulator/usb_devices.py", "class USBControl: pass\n")
  , ("emulator/boards/bodies/xm1-rec.json", "{\"body\": \"xm1\"}\n")
  , ("emulator/server.py", "def make_server(): pass\n")
  , ("emulator/web/boot.html", "<html></html>\n")
  , ("firmware/x-m1/1.50/emulator-rec.json", "{\"profile\": 1}\n")
  , ("inputs/selected.DAT", "dat")
  , ("inputs/x-m1-1.50/bootrom.bin", "rom")
  , ("native/python/x70_native/board.py", "class RustBoard: pass\n")
  , ("native/python/x70_native/_native.abi3.so", "engine")
  , ("native/src/lib.rs", "fn main() {}\n")
  , ("native/tests/test_usb_endpoint.py", "pass\n")
  , ("tools/continue_rec_scanout.py", "import tools.scanout_attach\n")
  , ("tools/firmware_registry.py", "pass\n")
  , ("tools/scanout_attach.py", "def attach(): pass\n")
  , ("tools/scanout_commands.py", "def transfer(): pass\n")
  , ("tools/start_setup_viewer.py", "def main(): pass\n")
  , ("viewer-validator", "launcher")
  , (".tools/firmware/xm1-150/manifest.json", "{}\n")
  ]

data Fixture = Fixture
  { fixtureRuntime :: FilePath
  , fixtureUnicorn :: FilePath
  , fixturePython :: FilePath
  }

newFixture :: FilePath -> IORef Int -> IO Fixture
newFixture base counter = do
  index <- atomicModifyIORef' counter (\n -> (n + 1, n))
  let directory = base </> ("case-" ++ show index)
      fixture = Fixture (directory </> "runtime") (directory </> "libunicorn.so.2") (directory </> "python3")
  forM_ stagedFiles $ \(path, contents) -> put (fixtureRuntime fixture </> path) contents
  put (fixtureUnicorn fixture) "unicorn"
  put (fixturePython fixture) "python"
  pure fixture

put :: FilePath -> String -> IO ()
put path contents = createDirectoryIfMissing True (takeDirectory path) >> writeFile path contents

routeInputs :: [(K.Key, Value)]
routeInputs = ["installed_dat" .= String "acb0fbbb", "adjustment_nand" .= String "baf47535"]

keyOf :: Fixture -> [(K.Key, Value)] -> IO String
keyOf fixture inputs = do
  guest <- guestState (fixtureRuntime fixture) (InterpreterExecutable (fixturePython fixture)) (UnicornLibrary (fixtureUnicorn fixture))
  pure (routeKeyHex (routeKey guest CardAttach inputs))

-- | Whether applying CHANGE to a fresh fixture leaves its key as before.
keeps :: FilePath -> IORef Int -> (Fixture -> IO ()) -> IO Bool
keeps base counter change = do
  fixture <- newFixture base counter
  before <- keyOf fixture routeInputs
  change fixture
  after <- keyOf fixture routeInputs
  pure (before == after)

-- | The compiled push driver digest a test tree stands in for.
compiledSources :: String
compiledSources = "a compiled push driver"

procedureOf :: Fixture -> String -> IO String
procedureOf fixture sources =
  procedureHex <$> hostProcedure (fixtureRuntime fixture) sources

-- | Whether applying CHANGE to a fresh fixture leaves its install identity as before.
keepsProcedure :: FilePath -> IORef Int -> (Fixture -> IO ()) -> IO Bool
keepsProcedure base counter change = do
  fixture <- newFixture base counter
  before <- procedureOf fixture compiledSources
  change fixture
  after <- procedureOf fixture compiledSources
  pure (before == after)

edit :: FilePath -> Fixture -> IO ()
edit path fixture = appendFile (fixtureRuntime fixture </> path) "# edited\n"

expect :: String -> Bool -> IO Bool
expect name ok = do
  putStrLn ((if ok then "ok   " else "FAIL ") ++ name)
  pure ok

main :: IO ()
main = do
  base <- lookupEnv "TEST_TMPDIR" >>= maybe ((</> "checkpoint-key-tests") <$> getTemporaryDirectory) pure
  counter <- newIORef 0
  let kept name change = keeps base counter change >>= expect ("keeps the key: " ++ name)
      changed name change = keeps base counter change >>= expect ("changes the key: " ++ name) . not
      keptProcedure name change = keepsProcedure base counter change >>= expect ("keeps the install identity: " ++ name)
      changedProcedure name change = keepsProcedure base counter change >>= expect ("changes the install identity: " ++ name) . not
  results <- sequence
    [ kept "a viewer launcher edit" (edit "tools/start_setup_viewer.py")
    , kept "a viewer HTTP server edit" (edit "emulator/server.py")
    , kept "a viewer page edit" (edit "emulator/web/boot.html")
    , kept "a Rust source edit, with the built engine unchanged" (edit "native/src/lib.rs")
    , kept "a native test edit" (edit "native/tests/test_usb_endpoint.py")
    , kept "a different launcher executable" (edit "viewer-validator")
    , kept "run outputs beside the trees" (\f -> put (fixtureRuntime f </> "viewer/boot-failure.json") "{}")
    , kept "unpacked firmware under .tools" (edit ".tools/firmware/xm1-150/manifest.json")
    , changed "a device model edit" (edit "emulator/usb_devices.py")
    , changed "a body JSON edit" (edit "emulator/boards/bodies/xm1-rec.json")
    , changed "a firmware profile edit" (edit "firmware/x-m1/1.50/emulator-rec.json")
    , changed "an engine Python edit" (edit "native/python/x70_native/board.py")
    , changed "a different native engine" (edit "native/python/x70_native/_native.abi3.so")
    , kept "a scanout launcher edit" (edit "tools/continue_rec_scanout.py")
    , kept "a bridge command-handling edit" (edit "tools/scanout_commands.py")
    , changed "an attach injection edit" (edit "tools/scanout_attach.py")
    , changed "a firmware registry edit" (edit "tools/firmware_registry.py")
    , changed "a different ROM" (edit "inputs/x-m1-1.50/bootrom.bin")
    , changed "a different selected DAT" (edit "inputs/selected.DAT")
    , changed "a new model module" (\f -> put (fixtureRuntime f </> "emulator/new_device.py") "pass\n")
    , changed "a removed model module" (\f -> removeFile (fixtureRuntime f </> "emulator/usb_devices.py"))
    , changed "a different Unicorn library" (\f -> appendFile (fixtureUnicorn f) "patched")
    , do
        fixture <- newFixture base counter
        before <- keyOf fixture routeInputs
        after <- keyOf fixture { fixturePython = fixtureUnicorn fixture } routeInputs
        expect "changes the key: a different interpreter" (before /= after)
    , do
        first <- newFixture base counter
        second <- newFixture base counter
        a <- keyOf first routeInputs
        b <- keyOf second { fixturePython = fixturePython first } routeInputs
        expect "keeps the key: the same staged bytes under another runtime root" (a == b)
    , do
        fixture <- newFixture base counter
        a <- keyOf fixture routeInputs
        b <- keyOf fixture ("attach_hold_key" .= String "q" : routeInputs)
        expect "changes the key: different route inputs" (a /= b)
    , do
        fixture <- newFixture base counter
        guest <- guestState (fixtureRuntime fixture) (InterpreterExecutable (fixturePython fixture)) (UnicornLibrary (fixtureUnicorn fixture))
        expect "changes the key: another checkpoint route"
          (routeKeyHex (routeKey guest CardAttach routeInputs) /= routeKeyHex (routeKey guest CardlessAttach routeInputs))
    , keptProcedure "a viewer launcher edit" (edit "tools/start_setup_viewer.py")
    , do
        first <- newFixture base counter
        second <- newFixture base counter
        a <- procedureOf first compiledSources
        b <- procedureOf second compiledSources
        expect "keeps the install identity: the same staged bytes under another runtime root" (a == b)
    , changedProcedure "a bridge command-handling edit" (edit "tools/scanout_commands.py")
    , do
        fixture <- newFixture base counter
        before <- procedureOf fixture compiledSources
        after <- procedureOf fixture "another compiled push driver"
        expect "changes the install identity: a different compiled push driver" (before /= after)
    ]
  unless (and results) exitFailure
