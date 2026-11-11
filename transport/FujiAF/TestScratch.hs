module FujiAF.TestScratch (scratchDirectory) where

import System.Directory (doesDirectoryExist, getTemporaryDirectory)
import System.FilePath ((</>))
import System.Posix.Temp (mkdtemp)

-- | A new directory named PREFIX plus a random suffix, on /dev/shm where the host has it, else
-- in the temporary directory. Transport sessions fsync every journal line: minutes on a
-- disk-backed Linux scratch directory, under a second on tmpfs. Concurrent Bazel runs share
-- /dev/shm and every sandbox reports pid 12.
scratchDirectory :: String -> IO FilePath
scratchDirectory prefix = do
  shm <- doesDirectoryExist "/dev/shm"
  base <- if shm then pure "/dev/shm" else getTemporaryDirectory
  mkdtemp (base </> prefix)
