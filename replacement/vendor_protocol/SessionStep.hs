{-# LANGUAGE GHC2021 #-}
-- | A child run that is bounded in both directions.
--
-- 'FujiAF.Host.runBounded' bounds the wait with 'System.Timeout.timeout' and drains the child's
-- streams from forked threads, and neither works without the threaded RTS: the timeout cannot
-- interrupt 'waitForProcess', which blocks in a safe @waitpid@ call, and no other thread runs
-- while that call holds the only capability. A child whose report exceeds one pipe capacity then
-- blocks in @write@ and holds its caller for as long as it lives. This step keeps the drain and
-- adds a watchdog that kills the child's process group at the deadline, so the bound holds for a
-- child that never exits as well.
module SessionStep (runBoundedGroup) where

import Control.Concurrent (forkIO, killThread, newEmptyMVar, putMVar, takeMVar, threadDelay)
import Control.Exception (IOException, catch, finally, mask, onException)
import Control.Monad (unless, void, when)
import qualified Data.ByteString as B
import Data.IORef (IORef, newIORef, readIORef, writeIORef)
import System.Exit (ExitCode)
import System.IO (BufferMode (NoBuffering), Handle, hClose, hSetBuffering)
import System.Posix.Signals (sigKILL, signalProcessGroup)
import System.Process (CreateProcess (create_group, std_err, std_in, std_out), StdStream (CreatePipe),
                       createProcess, getPid, proc, waitForProcess)
import System.Timeout (timeout)

-- | Runs @executable@ with @input@ on its standard input and returns whether the bound or the
-- output cap ended it, its exit status, and the retained prefix of its two streams.
--
-- The child owns a process group, so a descendant that outlives it cannot keep the pipes open past
-- the step: the group is killed once the child has been reaped.
runBoundedGroup :: Int -> Int -> FilePath -> [String] -> B.ByteString
                -> IO (Bool, ExitCode, B.ByteString, B.ByteString)
runBoundedGroup micros cap executable arguments input = mask $ \restore -> do
  created <- createProcess (proc executable arguments)
    { std_in = CreatePipe, std_out = CreatePipe, std_err = CreatePipe, create_group = True }
  (hin, hout, herr, ph) <- case created of
    (Just hin, Just hout, Just herr, ph) -> pure (hin, hout, herr, ph)
    _ -> ioError (userError "the bounded step needs all three standard streams")
  hSetBuffering hin NoBuffering
  pid <- getPid ph
  out <- newIORef B.empty
  err <- newIORef B.empty
  over <- newIORef False
  let kill = case pid of
        Nothing -> pure ()
        Just group -> void (signalProcessGroup sigKILL group) `catch` \(_ :: IOException) -> pure ()
      reader :: Handle -> IORef B.ByteString -> IO ()
      reader handle ref = loop `finally` (hClose handle `catch` \(_ :: IOException) -> pure ())
       where
        loop = do
          chunk <- B.hGetSome handle 4096
          unless (B.null chunk) $ do
            current <- readIORef ref
            let combined = current <> chunk
            writeIORef ref (B.take cap combined)
            when (B.length combined > cap) (writeIORef over True >> kill)
            loop
  od <- newEmptyMVar
  ed <- newEmptyMVar
  ot <- forkIO (reader hout out `finally` putMVar od ())
  et <- forkIO (reader herr err `finally` putMVar ed ())
  wt <- forkIO ((B.hPut hin input `catch` \(_ :: IOException) -> pure ())
                `finally` (hClose hin `catch` \(_ :: IOException) -> pure ()))
  expired <- newIORef False
  wd <- forkIO (threadDelay micros >> writeIORef expired True >> kill)
  code <- restore (waitForProcess ph) `onException` kill
  kill
  void (timeout 1000000 (takeMVar od >> takeMVar ed))
  mapM_ killThread [ot, et, wt, wd]
  timed <- readIORef expired
  excess <- readIORef over
  retained <- readIORef out
  errors <- readIORef err
  pure (timed || excess, code, retained, errors)
