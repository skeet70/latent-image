{-# LANGUAGE OverloadedStrings #-}
module Main (main) where

import Control.Monad (forM_, unless)
import qualified Data.Aeson as J
import qualified Data.ByteString as B
import Emulation.StartupTaskWire
import Emulation.TaskSubmissionControlWire
import qualified Emulation.TaskSubmissionControlWitness as C
import qualified FujiAF.Host as H
import FujiAF.JSON (decodeStrictIntegers)
import qualified Replacement.KernelInstall.Wire as W
import System.Environment (getArgs)
import System.FilePath ((</>))

newtype ObservationDirectory = ObservationDirectory FilePath

data Cohort = Standalone | Composed deriving (Eq, Show)

decode :: J.FromJSON a => B.ByteString -> IO a
decode bytes = do
  document <- either H.failH pure (decodeStrictIntegers bytes)
  case J.fromJSON document of J.Success value -> pure value; J.Error problem -> H.failH problem

check :: Cohort -> ProbeCase -> ObservationDirectory -> IO ()
check cohort selected (ObservationDirectory directory) = do
  ControlEnvelope receipt parsed <- H.readRegular 0x20000000 (directory </> "startup-tasks.json") >>= decode
  watch <- H.readRegular 0x2000000 (directory </> "memory-watch.json") >>= decode
  probe <- maybe (H.failH "release regression lacks a controlled probe") pure parsed
  let ownsExpectedProgram = case (cohort,receiptSchema receipt,receiptScope receipt,receiptProgram receipt) of
        (Standalone,TaskSubmissionProtocol,OwnedProbeScope,OwnedProgram _ _ _) -> True
        (Composed,JoinedTaskProtocol,JoinedProbeScope,JoinedProgram _) -> True
        _ -> False
  unless (probeCase probe == selected && ownsExpectedProgram)
    (H.failH "release regression fixture has the wrong case or ownership")
  entry <- maybe (H.failH "release regression lacks its entry") pure (probeReleaseEntryHit probe)
  returned <- maybe (H.failH "release regression lacks its return") pure (probeReleaseReturnHit probe)
  let interval = filter (\sample -> sampleHit sample >= entry && sampleHit sample <= returned) (receiptSamples receipt)
  unless (any ((==NativeWrite) . sampleBoundary) interval)
    (H.failH "release regression fixture lacks the discriminating NativeWrite checkpoint")
  case C.qualifyControl receipt (watch :: W.Watch) (Just probe) of
    Right (Just proof) | C.controlCase proof == selected -> pure ()
    Left problem -> H.failH ("instrumented release refused: " ++ show problem)
    _ -> H.failH "release regression did not produce its bounded control proof"
  forM_ [receipt {receiptScope=StockProbeScope},receipt {receiptScope=StockJoinedProbeScope}
    ,receipt {receiptSchema=LegacyStockProtocol}
    ,receipt {receiptSchema=if cohort == Standalone then JoinedTaskProtocol else TaskSubmissionProtocol}] $ \invalid ->
      unless (C.qualifyControl invalid watch (Just probe) == Left C.ControlScopeRejected)
        (H.failH "controlled receipt admitted mismatched protocol, ownership or scope")
  let returnedSamples = filter ((==returned) . sampleHit) (receiptSamples receipt)
      releasedLocks = [viewBytes view | sample <- returnedSamples,view <- sampleLocal sample,viewObject view == LockObject]
  unless (releasedLocks == [W.Bytes (B.replicate 4 0)])
    (H.failH "release did not clear the captured storage lock")
  whenPeerLock selected $ unless (any (\event -> W.eventSource event == W.CpuWrite
    && W.eventCore event == Just W.Primary && W.eventPhysical event == Just 0x9c280c
    && W.eventSize event == 4 && W.eventValue event == Just 0) (W.watchRecords watch))
      (H.failH "peer-owned lock release lacks its guest physical clear")
  if cohort == Composed then unless (any ((==W.CpuRead) . W.eventSource) (W.watchRecords watch))
    (H.failH "composed release lacks its captured read observations") else pure ()
  where
    whenPeerLock ReleasePeerLock action = action
    whenPeerLock _ _ = pure ()

main :: IO ()
main = do
  arguments <- getArgs
  case arguments of
    [unlocked,peer,unlock] -> checkReleases Standalone unlocked peer unlock
    [unlocked,peer,unlock,joinedUnlocked,joinedPeer,joinedUnlock] -> do
      checkReleases Standalone unlocked peer unlock
      checkReleases Composed joinedUnlocked joinedPeer joinedUnlock
    _ -> H.failH "usage: task-submission-control-tests OWNED_UNLOCKED_DIR OWNED_PEER_LOCK_DIR OWNED_UNLOCK_REFUSAL_DIR [JOINED_UNLOCKED_DIR JOINED_PEER_LOCK_DIR JOINED_UNLOCK_REFUSAL_DIR]"
  where
    checkReleases cohort unlocked peer unlock = do
      check cohort ReleaseUnlocked (ObservationDirectory unlocked)
      check cohort ReleasePeerLock (ObservationDirectory peer)
      check cohort ReleaseUnlockRefusal (ObservationDirectory unlock)
      putStrLn (show cohort ++ " releases retain physical-write replay, clear peer locks and reject mismatched protocol roles")
