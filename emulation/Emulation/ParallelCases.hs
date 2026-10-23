{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE TypeApplications #-}
-- Case suites run independent native runs. This keeps the worker count bounded
-- and results in spec order.
module Emulation.ParallelCases (defaultJobs, mapBounded, readJobs) where

import Control.Concurrent (forkIO, newEmptyMVar, putMVar, takeMVar)
import Control.Concurrent.QSem (newQSem, signalQSem, waitQSem)
import Control.Exception (SomeException, bracket_, throwIO, try)
import Control.Monad (forM)
import GHC.Conc (setNumCapabilities)
import Text.Read (readMaybe)

defaultJobs :: Int
defaultJobs = 8

readJobs :: Maybe String -> IO Int
readJobs raw = case raw of
  Nothing -> pure defaultJobs
  Just text -> case readMaybe text of
    Just n | n > 0 -> pure n
    _ -> fail "--jobs: positive integer required"

mapBounded :: Int -> (a -> IO b) -> [a] -> IO [b]
mapBounded jobs action items = do
  -- Case work splits between a native run and parent-side case preparation,
  -- report parsing and record keeping; one capability serializes the latter.
  setNumCapabilities (max 1 jobs)
  gate <- newQSem (max 1 jobs)
  slots <- forM items $ \item -> do
    slot <- newEmptyMVar
    _ <- forkIO (try @SomeException (bracket_ (waitQSem gate) (signalQSem gate) (action item)) >>= putMVar slot)
    pure slot
  outcomes <- mapM takeMVar slots
  either throwIO pure (sequence outcomes)
