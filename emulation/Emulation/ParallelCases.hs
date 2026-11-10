{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE TypeApplications #-}
-- Case suites run independent native runs. This keeps the worker count bounded
-- and results in spec order.
module Emulation.ParallelCases (NativeJobs, nativeJobsCount, readNativeJobs, mapNativeBounded, defaultJobs, mapBounded, readJobs) where

import Control.Concurrent (forkIOWithUnmask, killThread, modifyMVar, modifyMVar_, newEmptyMVar, newMVar, putMVar, takeMVar)
import Control.Concurrent.Chan (newChan, readChan, writeChan)
import Control.Exception (SomeAsyncException, SomeException, bracket, catch, finally, fromException, throwIO, tryJust)
import Control.Monad (forM, replicateM, replicateM_)
import GHC.Conc (setNumCapabilities)
import Text.Read (readMaybe)

-- The per-agent worker budget: 2 concurrent case workers.
defaultJobs :: Int
defaultJobs = 2

readJobs :: Maybe String -> IO Int
readJobs raw = case raw of
  Nothing -> pure defaultJobs
  Just text -> case readMaybe text of
    Just n | n > 0 -> pure n
    _ -> fail "--jobs: positive integer required"

newtype NativeJobs = NativeJobs {nativeJobsCount :: Int}
  deriving (Eq, Show)

readNativeJobs :: Maybe String -> IO NativeJobs
readNativeJobs Nothing = pure (NativeJobs 4)
readNativeJobs (Just text) = case readMaybe text of
  Just count | count >= 1 && count <= 8 -> pure (NativeJobs count)
  _ -> fail "--jobs: integer from 1 through 8 required"

mapNativeBounded :: NativeJobs -> (a -> IO b) -> [a] -> IO [b]
mapNativeBounded jobs = mapBounded (nativeJobsCount jobs)

mapBounded :: Int -> (a -> IO b) -> [a] -> IO [b]
mapBounded jobs action items = do
  setNumCapabilities (max 1 jobs)
  slots <- forM items $ \_ -> newEmptyMVar
  queue <- newMVar (zip items slots)
  events <- newChan
  let worker = do
        next <- modifyMVar queue $ \pending -> case pending of
          [] -> pure ([], Nothing)
          entry : rest -> pure (rest, Just entry)
        case next of
          Nothing -> pure ()
          Just (item, slot) -> do
            result <- tryJust (\exception -> case fromException exception :: Maybe SomeAsyncException of
              Just _ -> Nothing
              Nothing -> Just exception) (action item)
            putMVar slot result
            writeChan events (Right ())
            worker
      start = do
        done <- newEmptyMVar
        thread <- forkIOWithUnmask $ \restore ->
          (restore worker `catch` \(exception :: SomeException) -> writeChan events (Left exception))
            `finally` putMVar done ()
        pure (thread, done)
      stop workers = do
        modifyMVar_ queue (const (pure []))
        mapM_ (killThread . fst) workers
        mapM_ (takeMVar . snd) workers
  bracket (replicateM (min (max 1 jobs) (length slots)) start) stop $ \_ -> do
    replicateM_ (length slots) (readChan events >>= either throwIO pure)
    outcomes <- mapM takeMVar slots
    either throwIO pure (sequence outcomes)
