{-# LANGUAGE GHC2021 #-}
module Main (main) where

import Control.Concurrent (newEmptyMVar, putMVar, takeMVar)
import Control.Exception (AsyncException (..), IOException, bracket_, throwIO, try)
import Control.Monad (unless)
import Data.IORef (atomicModifyIORef', newIORef, readIORef, writeIORef)
import Emulation.ParallelCases (mapNativeBounded, readNativeJobs)
import System.Timeout (timeout)

ensure :: Bool -> String -> IO ()
ensure condition message = unless condition (fail message)

main :: IO ()
main = do
  jobs <- readNativeJobs (Just "2")
  release <- newEmptyMVar
  completed <- newIORef ([] :: [Int])
  let record value = atomicModifyIORef' completed (\values -> (values ++ [value], ()))
  ordered <- mapNativeBounded jobs (\value -> do
    if value == 0 then takeMVar release else pure ()
    record value
    if value == 1 then putMVar release () else pure ()
    pure value) [0,1]
  completionOrder <- readIORef completed
  ensure (completionOrder == [1,0]) "fixture did not complete out of order"
  ensure (ordered == [0,1]) "results are not in input order"
  active <- newIORef (0 :: Int, 0 :: Int)
  let enter = atomicModifyIORef' active (\(count,peak) -> let next=count+1 in ((next,max peak next),()))
      leave = atomicModifyIORef' active (\(count,peak) -> ((count-1,peak),()))
  values <- mapNativeBounded jobs (\value -> bracket_ enter leave (do
    if value == 0 then takeMVar release else pure ()
    if value == 1 then putMVar release () else pure ()
    pure value)) ([0..15] :: [Int])
  (remaining,peak) <- readIORef active
  ensure (values == [0..15] && remaining == 0 && peak == 2) "worker concurrency is not bounded at two"
  preserved <- newIORef ([] :: [Int])
  partial <- try (mapNativeBounded jobs (\value -> do
    if value == 1 then fail "case failed" else atomicModifyIORef' preserved (\seen -> (value:seen,()))
    pure value) [0,1,2]) :: IO (Either IOException [Int])
  case partial of
    Left _ -> pure ()
    Right _ -> fail "case failure was discarded"
  effects <- readIORef preserved
  ensure (0 `elem` effects && 2 `elem` effects && length effects == 2) "case failure discarded other completed effects"
  invalid <- try (readNativeJobs (Just "9"))
  case invalid of
    Left (_ :: IOException) -> pure ()
    Right _ -> fail "unbounded native jobs accepted"
  single <- readNativeJobs (Just "1")
  let selfAsync exception = do
        outcome <- timeout 2000000 (try (mapNativeBounded single (\() -> throwIO exception) [(),()]) :: IO (Either AsyncException [()]))
        ensure (outcome == Just (Left exception)) "self-async case failure did not propagate promptly"
  selfAsync ThreadKilled
  selfAsync UserInterrupt
  started <- newEmptyMVar
  blocked <- newEmptyMVar
  cleaned <- newIORef False
  extra <- newIORef False
  cancelled <- timeout 2000000 (try (mapNativeBounded jobs (\value -> case value of
    0 -> takeMVar started >> throwIO ThreadKilled
    1 -> bracket_ (putMVar started ()) (writeIORef cleaned True) (takeMVar blocked >> pure value)
    _ -> writeIORef extra True >> pure value) ([0,1,2] :: [Int])) :: IO (Either AsyncException [Int]))
  ensure (cancelled == Just (Left ThreadKilled)) "async worker failure did not cancel the batch"
  didClean <- readIORef cleaned
  didStartExtra <- readIORef extra
  ensure (didClean && not didStartExtra) "cancelled workers were not joined or queued work started"
