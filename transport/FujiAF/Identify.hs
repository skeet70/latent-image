-- Product identity over the C0 open, D0 tag-4 command, D1 reply and C4 close
-- sequence: eight service transfers, no configuration or memory access.
module FujiAF.Identify where

import Control.Exception
import Control.Monad (when)
import Data.IORef
import FujiAF.Transport
import qualified FujiAF.Broker as P

newtype ProductModel = ProductModel String deriving (Eq, Show)

identifyJournalCapBytes :: Int
identifyJournalCapBytes = 1024*1024

-- Eight service transfers, four acquisition stages and three cleanup stages,
-- each held to one transfer budget.
identifyAggregateDeadlineMs :: Int
identifyAggregateDeadlineMs = transferTimeoutMs * 15

runIdentify :: Clock -> UsbApi -> FilePath -> IO ProductModel
runIdentify clock api dir = runIdentifyCore clock api dir `catch` \(e::SomeException) -> do
  _ <- try (recordWithin identifyJournalCapBytes dir "session-uncertain" (take 4096 (displayException e))) :: IO (Either SomeException ())
  throwIO e

runIdentifyCore :: Clock -> UsbApi -> FilePath -> IO ProductModel
runIdentifyCore clock api dir = do
  s <- newSession clock api dir identifyJournalCapBytes identifyAggregateDeadlineMs
  held <- newHeld
  openedSession <- newIORef Nothing
  note s "prepared" "identify"
  mask $ \restore -> do
    result <- try $ restore $ do
      device <- acquireDevice s held
      _ <- tx s (P.controlTransfer (P.control 0xc0 8 0)) 12
      reply <- tx s (P.BulkIn (inputEndpoint (descriptors device)) 8) 8
      let sid = fromIntegral (reply!!0) + 256*fromIntegral (reply!!1)
          replyTail = foldr (\b a -> fromIntegral b + 256*a) (0::Integer) (drop 4 reply)
      writeIORef openedSession (Just (device,sid))
      when (replyTail /= 65536) (P.reject "session reply tail is not the constant 65536")
      _ <- tx s (P.controlTransfer (P.control 0xd0 32 sid)) 12
      _ <- tx s (P.BulkOut (outputEndpoint (descriptors device)) P.identityPacket) 32
      _ <- tx s (P.controlTransfer (P.control 0xd1 128 sid)) 12
      bytes <- tx s (P.BulkIn (inputEndpoint (descriptors device)) 128) 128
      ProductModel <$> either P.reject pure (P.decodeProduct bytes)
    -- All independent resource cleanup is attempted once. A failed protocol
    -- close does not skip release/close/exit. An expired deadline prevents
    -- further I/O.
    errors <- newIORef ([] :: [String])
    let attempt label action = action `catch` \(e::SomeException) -> modifyIORef' errors (++[label++": "++displayException e])
    opened <- readIORef openedSession
    attempt "protocol close" $ case opened of
      Nothing -> note s "cleanup-unavailable" "unknown SID; no invented C4"
      Just (device,sid) -> do
        _ <- tx s (P.controlTransfer (P.control 0xc4 2 sid)) 12
        _ <- tx s (P.BulkIn (inputEndpoint (descriptors device)) 2) 2
        note s "close-reply" "opaque bytes; stock restoration unobserved"
    releaseHeld held api (\label action -> attempt label (bounded s label action))
    failures <- readIORef errors
    case (result,failures) of
      (_, _:_) -> P.reject ("cleanup uncertain: "++show failures)
      (Left (e::SomeException),[]) -> throwIO e
      (Right model@(ProductModel text),[]) -> note s "identify-complete-stock-unverified" text >> pure model
