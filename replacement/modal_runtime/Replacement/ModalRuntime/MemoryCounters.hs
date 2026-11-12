-- | The cgroup counters that bracket one viewer run. Each read returns the
-- value the file holds at the call; a lazy read returns the value at first use,
-- which for a counter is a later one.
module Replacement.ModalRuntime.MemoryCounters
  ( readCounter
  , peakBytes
  , oomKills
  , oomEvents
  , memoryEvent
  ) where

import Data.Char (isSpace)
import System.FilePath ((</>))
import System.IO (readFile')

readCounter :: FilePath -> IO Integer
readCounter path = read . trim <$> readFile' path

peakBytes :: FilePath -> IO Integer
peakBytes directory = readCounter (directory </> "memory.peak")

oomKills, oomEvents :: FilePath -> IO Integer
oomKills = memoryEvent "oom_kill"
oomEvents = memoryEvent "oom"

memoryEvent :: String -> FilePath -> IO Integer
memoryEvent name directory = do
  text <- readFile' (directory </> "memory.events")
  pure $ sum [read value | line <- lines text, [key, value] <- [words line], key == name]

trim :: String -> String
trim = reverse . dropWhile isSpace . reverse
