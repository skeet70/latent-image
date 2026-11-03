-- | The emulator e2e's unreviewed RAM-push path: the runfiles relinker and
-- toolchain without the compiled source-identity and publication-contract
-- pins. Only //emulation links this module; //transport:camera does not.
module Xm1UnreviewedRelink
  ( withUnreviewedBuild
  , withUnreviewedBuildArchive
  , runUnreviewedRamPush
  , runUnreviewedRamUnload
  ) where

import FujiAF.RamPush
import FujiAF.Transport (Clock, UsbApi)

withUnreviewedBuild :: Payload -> AllowlistPolicy -> Address -> (PinnedBuild -> IO a) -> IO a
withUnreviewedBuild payload policy base action = do
  relinker <- runfilesRelinker payload
  withRelinkedBuildUsing relinker policy base action

-- | As 'withUnreviewedBuild' with the bank's Rust archive replaced; the
-- payload must link the bank.
withUnreviewedBuildArchive :: FilePath -> Payload -> AllowlistPolicy -> Address -> (PinnedBuild -> IO a) -> IO a
withUnreviewedBuildArchive archive payload policy base action = do
  relinker <- runfilesRelinker payload
  rust <- maybe (ioError (userError "the selected payload links no Rust bank archive")) pure (relinkerRust relinker)
  withRelinkedBuildUsing relinker { relinkerRust = Just rust { rustBankArchive = archive } } policy base action

runUnreviewedRamPush :: RamPushObserver -> RetainedAllocation -> RamPushManifest -> Clock -> UsbApi -> FilePath -> IO RamPushOutcome
runUnreviewedRamPush observer retained = runRamPushWith xm1PeerCacheAvailability observer (retainedAllocation retained)

runUnreviewedRamUnload :: RamPushState -> Clock -> UsbApi -> FilePath -> IO RamUnloadOutcome
runUnreviewedRamUnload = runRamUnloadWith xm1PeerCacheAvailability
