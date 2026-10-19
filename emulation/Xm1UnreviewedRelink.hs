-- | The emulator e2e's unreviewed RAM-push path: the runfiles relinker and
-- toolchain without the compiled source-identity and publication-contract
-- pins. Only //emulation links this module; //transport:camera does not.
module Xm1UnreviewedRelink
  ( withUnreviewedBuild
  , runUnreviewedRamPush
  , runUnreviewedRamUnload
  ) where

import FujiAF.RamPush
import FujiAF.Transport (Clock, UsbApi)

withUnreviewedBuild :: Payload -> AllowlistPolicy -> Address -> (PinnedBuild -> IO a) -> IO a
withUnreviewedBuild payload policy base action = do
  relinker <- runfilesRelinker payload
  withRelinkedBuildUsing relinker policy base action

runUnreviewedRamPush :: RetainedAllocation -> RamPushManifest -> Clock -> UsbApi -> FilePath -> IO RamPushOutcome
runUnreviewedRamPush retained = runRamPushWith xm1PeerCacheAvailability (retainedAllocation retained)

runUnreviewedRamUnload :: RamPushState -> Clock -> UsbApi -> FilePath -> IO RamUnloadOutcome
runUnreviewedRamUnload = runRamUnloadWith xm1PeerCacheAvailability
