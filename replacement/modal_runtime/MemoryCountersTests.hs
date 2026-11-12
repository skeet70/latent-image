{-# LANGUAGE GHC2021 #-}
module Main where

import Control.Monad (unless)
import Replacement.ModalRuntime.MemoryCounters (oomEvents, oomKills, peakBytes, readCounter)
import System.Directory (createDirectoryIfMissing, getTemporaryDirectory, removeDirectoryRecursive)
import System.Environment (lookupEnv)
import System.FilePath ((</>))
import System.Posix.IO (OpenFileFlags (creat, trunc), OpenMode (WriteOnly), closeFd, defaultFileFlags, fdWrite, openFd)
import System.Posix.Process (getProcessID)

main :: IO ()
main = do
  configured <- lookupEnv "TEST_TMPDIR"
  temporary <- maybe getTemporaryDirectory pure configured
  process <- getProcessID
  let root = temporary </> ("xm1-memory-counters-" ++ show process)
  createDirectoryIfMissing True root
  readCounterHoldsTheValueAtTheCall root
  peakHoldsTheValueAtTheCall root
  eventsHoldTheValueAtTheCall root
  removeDirectoryRecursive root

-- | A counter that rises between two reads reports the two values it held, not
-- the later value twice.
readCounterHoldsTheValueAtTheCall :: FilePath -> IO ()
readCounterHoldsTheValueAtTheCall root = do
  let limit = root </> "memory.max"
  replace limit "2147483648\n"
  before <- readCounter limit
  replace limit "1073741824\n"
  after <- readCounter limit
  unless (before == 2147483648 && after == 1073741824)
    (error ("memory.max read returned " ++ show (before, after) ++ " instead of (2147483648,1073741824)"))

peakHoldsTheValueAtTheCall :: FilePath -> IO ()
peakHoldsTheValueAtTheCall root = do
  let peak = root </> "memory.peak"
  replace peak "5\n"
  before <- peakBytes root
  replace peak "9\n"
  after <- peakBytes root
  unless (before == 5 && after == 9)
    (error ("memory.peak read returned " ++ show (before, after) ++ " instead of (5,9)"))

eventsHoldTheValueAtTheCall :: FilePath -> IO ()
eventsHoldTheValueAtTheCall root = do
  let events = root </> "memory.events"
  replace events "low 0\nhigh 0\nmax 0\noom 0\noom_kill 0\noom_group_kill 0\n"
  before <- counts
  replace events "low 0\nhigh 0\nmax 0\noom 2\noom_kill 1\noom_group_kill 7\n"
  after <- counts
  unless (before == (0, 0) && after == (1, 2))
    (error ("memory.events read returned " ++ show (before, after) ++ " instead of ((0,0),(1,2))"))
  where
    counts = (,) <$> oomKills root <*> oomEvents root

-- | Rewrites the file the way the kernel updates a counter: through the open
-- file, beside any reader's handle.
replace :: FilePath -> String -> IO ()
replace path text = do
  fd <- openFd path WriteOnly defaultFileFlags {trunc = True, creat = Just 0o600}
  _ <- fdWrite fd text
  closeFd fd
