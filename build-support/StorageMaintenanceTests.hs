{-# LANGUAGE ForeignFunctionInterface #-}
{-# LANGUAGE OverloadedStrings #-}
{-# LANGUAGE ScopedTypeVariables #-}
module Main (main) where

import Control.Concurrent (threadDelay)
import Control.Exception (bracket)
import Control.Monad (forM_, unless)
import Data.Aeson (FromJSON (parseJSON), eitherDecodeStrict', withObject, (.:))
import qualified Data.ByteString as B
import qualified Data.Text as T
import qualified Data.Text.Encoding as TE
import Foreign.C.Types (CInt (..), CLong (..), CSize (..))
import Foreign.Ptr (Ptr, intPtrToPtr, nullPtr)
import System.Directory (canonicalizePath, createDirectory, createDirectoryIfMissing, doesPathExist, getTemporaryDirectory, removeDirectoryRecursive, removeFile)
import System.Environment (getArgs, getEnvironment, getExecutablePath)
import System.Exit (ExitCode (..))
import System.FilePath ((</>), takeDirectory, takeFileName)
import System.IO (hClose, hFlush, openTempFile, stdout)
import System.Posix.Files (createLink, createSymbolicLink, setFileMode)
import System.Posix.IO (OpenFileFlags (cloexec), OpenMode (ReadOnly, ReadWrite), closeFd, defaultFileFlags, openFd)
import System.Posix.Process (executeFile)
import System.Posix.Types (Fd (..))
import System.Process (CreateProcess (..), StdStream (CreatePipe), createProcess, proc, readProcessWithExitCode, terminateProcess, waitForProcess)

foreign import ccall unsafe "flock" c_flock :: CInt -> CInt -> IO CInt
foreign import ccall unsafe "mmap" c_mmap :: Ptr () -> CSize -> CInt -> CInt -> CInt -> CLong -> IO (Ptr ())

data AuditSummary = AuditSummary Bool [Integer]
instance FromJSON AuditSummary where
  parseJSON = withObject "audit" $ \o -> do
    verified <- o .: "protectedFilesVerified"
    changes <- o .: "actualAvailableDelta"
    pure (AuditSummary verified (map snd (changes :: [(FilePath, Integer)])))

data InventorySummary = InventorySummary [Integer] [[String]]
instance FromJSON InventorySummary where
  parseJSON = withObject "inventory" $ \o -> do
    available <- o .: "availableBytes"
    users <- o .: "activeUsers"
    pure (InventorySummary (map snd (available :: [(FilePath, Integer)])) (map snd (users :: [(FilePath, [String])])))

data Counts = Counts Integer Integer Integer Integer
instance FromJSON Counts where
  parseJSON = withObject "root counts" $ \o -> Counts <$> o .: "recognizedFileCount" <*> o .: "recognizedBytes" <*> o .: "retainedFileCount" <*> o .: "retainedBytes"
newtype CountSummary = CountSummary [Counts]
instance FromJSON CountSummary where
  parseJSON = withObject "inventory counts" $ \o -> CountSummary <$> o .: "rootSummaries"

check :: Bool -> String -> IO ()
check ok message = unless ok (ioError (userError message))

put :: FilePath -> B.ByteString -> IO ()
put p bytes = createDirectoryIfMissing True (takeDirectory p) >> B.writeFile p bytes

hashSuffix :: String
hashSuffix = "0123456789abcdef"

library, fingerprint, lock :: FilePath
library = "debug/deps/libfixture-" ++ hashSuffix ++ ".rlib"
fingerprint = "debug/.fingerprint/fixture-" ++ hashSuffix ++ "/lib-fixture"
lock = "debug/.cargo-lock"

compilerFiles :: [FilePath]
compilerFiles = [library, "debug/deps/libfixture-" ++ hashSuffix ++ ".rmeta", "debug/deps/libfixture-" ++ hashSuffix ++ ".so"] ++ ["debug/incremental/fixture-123/s-456/" ++ name | name <- ["dep-graph.bin", "query-cache.bin", "work-products.bin"]] ++ ["debug/build/builder-" ++ hashSuffix ++ "/build-script-build"]

protectedFiles :: [FilePath]
protectedFiles = ["main.rs", "case.json", "results/report.html", "image.DAT", "imports/raw.bin", "debug/fixture", "debug/examples/fixture"] ++ ["debug/deps/" ++ name | name <- ["source.rs", "report.json", "report.html", "database.db", "database.sqlite", "firmware.DAT", "custom.bin", "import.rlib", "libunclaimed-0123456789abcdef.rlib", "libfixture-0123456789abcdef.d"]]

fixture :: FilePath -> IO ()
fixture root = do
  createDirectory root
  put (root </> "CACHEDIR.TAG") "Signature: 8a477f597d28d172789f06886806bc55\n# synthetic Cargo fixture\n"
  put (root </> lock) ""
  put (root </> fingerprint) "0123456789abcdef"
  put (root </> (fingerprint ++ ".json")) "{\"rustc\":1,\"features\":\"[]\",\"target\":2,\"profile\":3,\"path\":4,\"deps\":[]}"
  let buildFingerprint = "debug/.fingerprint/builder-" ++ hashSuffix ++ "/build-script-build-script-build"
  put (root </> buildFingerprint) "0123456789abcdef"
  put (root </> (buildFingerprint ++ ".json")) "{\"rustc\":1,\"features\":\"[]\",\"target\":2,\"profile\":3,\"path\":4,\"deps\":[]}"
  forM_ compilerFiles $ \file -> put (root </> file) (if ".rlib" `suffix` file then "!<arch>\ncompiler fixture" else if ".so" `suffix` file || takeFileName file == "build-script-build" then B.pack [127,69,76,70,1,2,3,4] else "rust compiler fixture")
  forM_ protectedFiles $ \file -> put (root </> file) "synthetic protected consumer data\n"
 where suffix ending value = reverse ending `prefix` reverse value
       prefix beginning value = take (length beginning) value == beginning

snapshot :: FilePath -> [FilePath] -> IO [B.ByteString]
snapshot root = mapM (B.readFile . (root </>))

run :: FilePath -> Bool -> [String] -> IO B.ByteString
run executable success arguments = do
  (code, out, err) <- readProcessWithExitCode executable arguments ""
  check ((code == ExitSuccess) == success) ("unexpected command result: " ++ show arguments ++ " " ++ show code ++ "\n" ++ err)
  pure (TE.encodeUtf8 (T.pack out))

makePlan :: FilePath -> FilePath -> FilePath -> IO ()
makePlan executable root plan = run executable True ["plan", "--cargo-target", root, "--output", plan] >> pure ()

assertUnchanged :: FilePath -> [FilePath] -> [B.ByteString] -> IO ()
assertUnchanged root files expected = snapshot root files >>= \actual -> check (actual == expected) "consumer data changed"

withChild :: CreateProcess -> IO a -> IO a
withChild command action = bracket start stop (const action)
 where
  start = do
    (_, Just output, _, child) <- createProcess command { std_out = CreatePipe }
    ready <- B.hGet output 5
    hClose output
    check (ready == "ready") "child did not reach its active state"
    pure child
  stop child = terminateProcess child >> waitForProcess child >> pure ()

main :: IO ()
main = do
  arguments <- getArgs
  case arguments of
    [tool] -> do
      self <- getExecutablePath
      environment <- getEnvironment
      let fixtureEnvironment = ("LC_ALL", "C.UTF-8") : filter ((/= "LC_ALL") . fst) environment
      -- The isolated process creates every managed root.
      executeFile "unshare" True
        ["--user", "--map-root-user", "--pid", "--fork", "--mount-proc", self, "--isolated-fixtures", tool] (Just fixtureEnvironment)
    ["--isolated-fixtures", tool] -> runFixtures tool
    ["--mapped-fixture", file] -> do
      mapped <- bracket (openFd file ReadOnly defaultFileFlags) closeFd $ \(Fd fd) ->
        c_mmap nullPtr 4096 1 2 fd 0
      check (mapped /= intPtrToPtr (-1)) "cannot map the synthetic fixture"
      putStr "ready"
      hFlush stdout
      threadDelay 60_000_000
    _ -> ioError (userError "storage-maintenance-tests TOOL")

runFixtures :: FilePath -> IO ()
runFixtures argument = do
  executable <- canonicalizePath argument
  base <- getTemporaryDirectory
  bracket (do (name, handle) <- openTempFile base "storage-maintenance-tests"; hClose handle; removeFile name; createDirectory name; canonicalizePath name) removeDirectoryRecursive $ \workspace -> do
    let fresh name action = do
          let root = workspace </> name
              plan = workspace </> (name ++ ".plan.json")
          fixture root
          action root plan
    fresh "preview" $ \root plan -> do
      let files = compilerFiles ++ protectedFiles ++ [fingerprint]
      expected <- snapshot root files
      inventory <- run executable True ["inventory", "--cargo-target", root]
      case eitherDecodeStrict' inventory of
        Right (CountSummary [Counts selected selectedBytes kept keptBytes]) -> check (selected == fromIntegral (length compilerFiles + 2) && selectedBytes > 0 && kept == fromIntegral (length protectedFiles + 4) && keptBytes > 0) "inventory file counts or byte totals differ"
        Right _ -> ioError (userError "inventory omitted root summary")
        Left err -> ioError (userError err)
      makePlan executable root plan
      assertUnchanged root files expected
    fresh "metadata-only" $ \root _ -> do
      let unreadable = root </> "debug/deps/database.db"
      bracket (setFileMode unreadable 0) (const (setFileMode unreadable 0o600)) $ \_ -> do
        _ <- run executable True ["inventory", "--cargo-target", root]
        pure ()
    fresh "apply" $ \root plan -> do
      expected <- snapshot root protectedFiles
      makePlan executable root plan
      _ <- run executable True ["apply", "--plan", plan]
      forM_ (fingerprint : compilerFiles) $ \p -> doesPathExist (root </> p) >>= \exists -> check (not exists) ("compiler output retained: " ++ p)
      assertUnchanged root protectedFiles expected
      audit <- B.readFile (plan ++ ".audit.json")
      case eitherDecodeStrict' audit of
        Right (AuditSummary verified deltas) -> check (verified && length deltas == 1) "missing measured audit or protected verification"
        Left err -> ioError (userError err)
      inventory <- run executable True ["inventory", "--cargo-target", root]
      case eitherDecodeStrict' inventory of
        Right (CountSummary [Counts selected selectedBytes _ _]) -> check (selected == 0 && selectedBytes == 0) "post-maintenance inventory selected retained data"
        Right _ -> ioError (userError "post-maintenance inventory omitted root summary")
        Left err -> ioError (userError err)
    fresh "nested-source-state" $ \root plan -> do
      let protectedRoot = root </> "source/.git"
          protected = compilerFiles ++ protectedFiles ++ [fingerprint]
      createDirectory (root </> "source")
      fixture protectedRoot
      expected <- snapshot protectedRoot protected
      makePlan executable root plan
      _ <- run executable True ["apply", "--plan", plan]
      assertUnchanged protectedRoot protected expected
      doesPathExist (root </> library) >>= \exists -> check (not exists) "ordinary compiler output was not removed beside a preserved Git tree"
    fresh "stale" $ \root plan -> do
      makePlan executable root plan
      put (root </> "debug/deps/new-private-data.bin") "new consumer data"
      expected <- snapshot root (compilerFiles ++ protectedFiles)
      _ <- run executable False ["apply", "--plan", plan]
      assertUnchanged root (compilerFiles ++ protectedFiles) expected
      B.readFile (root </> "debug/deps/new-private-data.bin") >>= \bytes -> check (bytes == "new consumer data") "new protected data removed"
    fresh "changed" $ \root plan -> do
      makePlan executable root plan
      put (root </> library) "!<arch>\nchanged compiler artifact"
      expected <- snapshot root (compilerFiles ++ protectedFiles)
      _ <- run executable False ["apply", "--plan", plan]
      assertUnchanged root (compilerFiles ++ protectedFiles) expected
    fresh "wrong-header" $ \root plan -> do
      let file = "debug/deps/libfixture-" ++ hashSuffix ++ ".rmeta"
      put (root </> file) "firmware data with a compiler-shaped name"
      makePlan executable root plan
      _ <- run executable True ["apply", "--plan", plan]
      B.readFile (root </> file) >>= \bytes -> check (bytes == "firmware data with a compiler-shaped name") "non-compiler content removed"
    fresh "retained-unit" $ \root plan -> do
      forM_ compilerFiles $ \file -> put (root </> file) "application content, not a compiler artifact"
      let files = compilerFiles ++ protectedFiles ++ [fingerprint, "debug/.fingerprint/builder-" ++ hashSuffix ++ "/build-script-build-script-build"]
      expected <- snapshot root files
      makePlan executable root plan
      _ <- run executable True ["apply", "--plan", plan]
      assertUnchanged root files expected
    fresh "hardlinked-unit" $ \root plan -> do
      let external = workspace </> "external-hardlinks"
      createDirectory external
      forM_ (zip [0 :: Int ..] compilerFiles) $ \(index, file) -> createLink (root </> file) (external </> show index)
      expected <- snapshot root (compilerFiles ++ [fingerprint])
      makePlan executable root plan
      _ <- run executable True ["apply", "--plan", plan]
      assertUnchanged root (compilerFiles ++ [fingerprint]) expected
    fresh "tampered" $ \root plan -> do
      makePlan executable root plan
      bytes <- B.readFile plan
      let (before, after) = B.breakSubstring "\"Retain\"" bytes
      check (not (B.null after)) "plan lacks preservation metadata"
      B.writeFile plan (before <> "\"CompilerArtifact\"" <> B.drop 8 after)
      expected <- snapshot root (compilerFiles ++ protectedFiles)
      _ <- run executable False ["apply", "--plan", plan]
      assertUnchanged root (compilerFiles ++ protectedFiles) expected
    fresh "symlink" $ \root plan -> do
      makePlan executable root plan
      let outside = workspace </> "outside.bin"
      put outside "external consumer data"
      createSymbolicLink outside (root </> "debug/deps/escape.bin")
      _ <- run executable False ["apply", "--plan", plan]
      _ <- run executable False ["inventory", "--cargo-target", root]
      B.readFile outside >>= \bytes -> check (bytes == "external consumer data") "symlink escaped root"
      let alias = workspace </> "alias"
      createSymbolicLink root alias
      _ <- run executable False ["inventory", "--cargo-target", alias]
      pure ()
    forM_ ["private-data", ".git"] $ \protected -> do
      let parent = workspace </> protected
      createDirectory parent
      fixture (parent </> "target")
      _ <- run executable False ["inventory", "--cargo-target", parent </> "target"]
      pure ()
    fresh "active" $ \root plan -> do
      makePlan executable root plan
      expected <- snapshot root (compilerFiles ++ protectedFiles)
      withChild ((proc "sh" ["-c", "printf ready; exec sleep 60"]) { cwd = Just root }) $ do
        inventory <- run executable True ["inventory", "--cargo-target", root]
        case eitherDecodeStrict' inventory of
          Right (InventorySummary available users) -> check (length available == 1 && any (not . null) users) "inventory omitted active user or measured space"
          Left err -> ioError (userError err)
        _ <- run executable False ["apply", "--plan", plan]
        assertUnchanged root (compilerFiles ++ protectedFiles) expected
    fresh "mapped-consumer" $ \root plan -> do
      makePlan executable root plan
      expected <- snapshot root (compilerFiles ++ protectedFiles)
      self <- getExecutablePath
      withChild ((proc self ["--mapped-fixture", root </> library]) {cwd = Just workspace}) $ do
        _ <- run executable False ["apply", "--plan", plan]
        assertUnchanged root (compilerFiles ++ protectedFiles) expected
    fresh "relative-env-λ" $ \root plan -> do
      makePlan executable root plan
      environment <- getEnvironment
      let command = (proc "sh" ["-c", "printf ready; exec sleep 60"]) { cwd = Just workspace, env = Just (("CARGO_TARGET_DIR", takeFileName root) : filter ((/= "CARGO_TARGET_DIR") . fst) environment) }
      withChild command $ do
        _ <- run executable False ["apply", "--plan", plan]
        doesPathExist (root </> library) >>= \exists -> check exists "relative environment target was not protected"
    forM_ [".cargo-lock", ".cargo-build-lock", ".cargo-artifact-lock"] $ \lockName ->
      fresh ("locked-" ++ lockName) $ \root plan -> do
        let held = root </> "debug" </> lockName
        put held ""
        makePlan executable root plan
        expected <- snapshot root (compilerFiles ++ protectedFiles)
        bracket (openFd held ReadWrite defaultFileFlags {cloexec = True}) closeFd $ \(Fd fd) -> do
          result <- c_flock fd 6
          check (result == 0) "cannot acquire fixture Cargo lock"
          -- The lock holder is outside this probe's PID namespace.
          _ <- run "unshare" False
            ["--pid", "--fork", "--mount-proc", executable, "apply", "--plan", plan]
          assertUnchanged root (compilerFiles ++ protectedFiles) expected
    fresh "multi-a" $ \root plan -> do
      let other = workspace </> "multi-b"
      fixture other
      _ <- run executable True ["plan", "--cargo-target", root, "--cargo-target", other, "--output", plan]
      put (other </> "reports/new.json") "preserve"
      expected <- snapshot root (compilerFiles ++ protectedFiles)
      _ <- run executable False ["apply", "--plan", plan]
      assertUnchanged root (compilerFiles ++ protectedFiles) expected
  putStrLn "storage maintenance behavioral tests passed"
