{-# LANGUAGE OverloadedStrings #-}
module Main (main) where

import Control.Monad (unless)
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

decode :: J.FromJSON a => B.ByteString -> IO a
decode bytes = do
  document <- either H.failH pure (decodeStrictIntegers bytes)
  case J.fromJSON document of J.Success value -> pure value; J.Error problem -> H.failH problem

check :: ProbeCase -> ObservationDirectory -> IO ()
check selected (ObservationDirectory directory) = do
  ControlEnvelope receipt parsed <- H.readRegular 0x20000000 (directory </> "startup-tasks.json") >>= decode
  watch <- H.readRegular 0x2000000 (directory </> "memory-watch.json") >>= decode
  probe <- maybe (H.failH "release regression lacks a controlled probe") pure parsed
  unless (probeCase probe == selected && receiptScope receipt == OwnedProbeScope
    && case receiptProgram receipt of OwnedProgram _ _ _ -> True; _ -> False)
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

main :: IO ()
main = do
  arguments <- getArgs
  case arguments of
    [unlocked,peer,unlock] -> do
      check ReleaseUnlocked (ObservationDirectory unlocked)
      check ReleasePeerLock (ObservationDirectory peer)
      check ReleaseUnlockRefusal (ObservationDirectory unlock)
      putStrLn "three owned releases admit NativeWrite checkpoints without removing their replay snapshots"
    _ -> H.failH "usage: task-submission-control-tests OWNED_UNLOCKED_DIR OWNED_PEER_LOCK_DIR OWNED_UNLOCK_REFUSAL_DIR"
