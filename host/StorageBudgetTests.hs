module Main (main) where

import Control.Exception (bracket)
import Control.Monad (forM_, unless, when)
import System.Directory (createDirectory, createDirectoryIfMissing, doesPathExist, getTemporaryDirectory, removePathForcibly)
import System.Environment (getArgs, getEnvironment, lookupEnv)
import System.Exit (ExitCode (..))
import System.FilePath ((</>))
import System.Posix.Files (createSymbolicLink)
import System.Posix.Process (getProcessID)
import System.Process (CreateProcess (..), proc, readCreateProcessWithExitCode)

assert :: Bool -> String -> IO ()
assert condition problem = unless condition (ioError (userError problem))

main :: IO ()
main = do
  [probe] <- getArgs
  temporary <- lookupEnv "TEST_TMPDIR" >>= maybe getTemporaryDirectory pure
  pid <- getProcessID
  let root = temporary </> ("storage-budget-" ++ show pid)
  bracket (createDirectory root >> pure root) removePathForcibly $ \directory -> do
    environment <- getEnvironment
    let run label reserve output = do
          let marker = directory </> label
              childEnvironment = ("XM1_STORAGE_MIN_FREE", reserve) : filter ((/= "XM1_STORAGE_MIN_FREE") . fst) environment
          (code, _, _) <- readCreateProcessWithExitCode (proc probe [output, marker]) {env = Just childEnvironment} ""
          effect <- doesPathExist marker
          pure (code, effect)
        freshOutput = directory </> "not-created" </> "nested"
    forM_ ["1", "1K", "1KiB", "1M", "1MiB"] $ \reserve -> do
      (code, effect) <- run ("accepted-" ++ reserve) reserve freshOutput
      assert (code == ExitSuccess && effect) ("available storage was refused for reserve " ++ reserve)
    created <- doesPathExist (directory </> "not-created")
    assert (not created) "the admission check created an output directory"
    (refused, effect) <- run "refused" "999999999999999999999T" freshOutput
    assert (refused /= ExitSuccess && not effect) "insufficient storage allowed the protected action"
    forM_ ["", "0", "-1", "1.5G", "1GB", "no-limit"] $ \reserve -> do
      (code, invalidEffect) <- run ("invalid-" ++ filter (`notElem` "/") reserve) reserve freshOutput
      assert (code /= ExitSuccess && not invalidEffect) ("invalid reserve allowed the protected action: " ++ show reserve)
    let link = directory </> "linked-output"
        target = directory </> "real-output"
    createDirectoryIfMissing True target
    createSymbolicLink target link
    (linked, linkedEffect) <- run "linked-admitted" "1" (link </> "new-child")
    assert (linked == ExitSuccess && linkedEffect) "an output below an existing directory symlink was refused"
    let dangling = directory </> "dangling-output"
    createSymbolicLink (directory </> "absent-target") dangling
    (danglingCode, danglingEffect) <- run "dangling-refused" "1" dangling
    assert (danglingCode /= ExitSuccess && not danglingEffect) "a dangling output symlink authorized the protected action"
    (probeFailure, probeEffect) <- readCreateProcessWithExitCode
      (proc probe [directory, directory </> "probe-error-effect"]) {env = Just [("PATH", "/nonexistent"), ("XM1_STORAGE_MIN_FREE", "1")]} "" >>= \(code, _, _) -> do
        observed <- doesPathExist (directory </> "probe-error-effect")
        pure (code, observed)
    when (probeFailure == ExitSuccess || probeEffect) (ioError (userError "a missing disk probe authorized the protected action"))
  putStrLn "storage budget: protected actions admitted or refused before output effects"
