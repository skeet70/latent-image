-- | Haskell child fixture for thermal-guard process tests. It starts a
-- descendant in a different process group and records both PIDs.
module Main (main) where

import Control.Concurrent (threadDelay)
import Data.Char (isDigit)
import System.Directory (doesFileExist)
import System.Environment (getArgs, getExecutablePath)
import System.Exit (ExitCode (..), exitWith)
import System.Posix.Process (getProcessID)
import System.Process
  ( CreateProcess (..)
  , StdStream (Inherit)
  , createProcess
  , getPid
  , proc
  )
import Text.Read (readMaybe)

main :: IO ()
main = do
  arguments <- getArgs
  case arguments of
    ["exit", value] -> case readMaybe value of
      Just code -> exitWith (if code == 0 then ExitSuccess else ExitFailure code)
      Nothing -> exitWith (ExitFailure 2)
    ["descendant", pidFile] -> descendant pidFile
    ["orphan", pidFile] -> orphan pidFile
    ["limits", path] -> limits path
    ["affinity", pidFile, releaseFile] -> affinity pidFile releaseFile
    ["sleep"] -> threadDelay 30_000_000
    _ -> exitWith (ExitFailure 2)

limits :: FilePath -> IO ()
limits path = do
  cgroup <- readFile "/proc/self/cgroup"
  case [drop 3 line | line <- lines cgroup, take 3 line == "0::"] of
    (relative : _) -> do
      let directory = "/sys/fs/cgroup" ++ relative
      memory <- readFile (directory ++ "/memory.max")
      swap <- readFile (directory ++ "/memory.swap.max")
      writeFile path (memory ++ swap)
    _ -> exitWith (ExitFailure 2)

affinity :: FilePath -> FilePath -> IO ()
affinity pidFile releaseFile = do
  _ <- spawnDescendant pidFile
  waitForRelease (200 :: Int)
  where
    waitForRelease 0 = exitWith (ExitFailure 2)
    waitForRelease attempts = do
      released <- doesFileExist releaseFile
      if released then exitWith (ExitFailure 7)
        else threadDelay 50_000 >> waitForRelease (attempts - 1)

descendant :: FilePath -> IO ()
descendant pidFile = do
  _ <- spawnDescendant pidFile
  threadDelay 30_000_000

orphan :: FilePath -> IO ()
orphan pidFile = do
  _ <- spawnDescendant pidFile
  pure ()

spawnDescendant :: FilePath -> IO (FilePath)
spawnDescendant pidFile = do
  executable <- getExecutablePath
  parent <- getProcessID
  (_, _, _, child) <- createProcess (proc executable ["sleep"])
    { create_group = True
    , std_in = Inherit
    , std_out = Inherit
    , std_err = Inherit
    }
  childPid <- getPid child
  case childPid of
    Nothing -> exitWith (ExitFailure 2)
    Just pid -> do
      writeFile pidFile (show parent ++ "\n" ++ showPid pid ++ "\n")
      pure pidFile
  where
    showPid = filter isDigit . show
