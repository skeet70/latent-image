module Main (main) where

import Control.Monad (unless)
import qualified Data.ByteString as B
import Data.Word (Word32)
import Replacement.KernelInstall.Wire (Address (..), WordValue (..))
import Replacement.StartupConsumers.Source
import System.Exit (exitFailure)

main :: IO ()
main = do
  assert "full task id" (taskId 103 /= Nothing && taskId 0 == Nothing && taskId 121 == Nothing && taskId 0x10067 == Nothing)
  assert "canonical source refusal" (case deriveSource (SourceFiles B.empty B.empty B.empty B.empty) of Left CanonicalSourceRejected -> True; _ -> False)
  let empty = record 0 0 0
  assert "task address is low view" (reserve (input TaskAllocation 7 0x40a10200 empty)
    == allocated 0xa10200 0x40a10204 (record 0xa10200 7 1))
  assert "storage address keeps high view" (reserve (input StorageAllocation 7 0x409c4460 empty)
    == allocated 0x409c4460 0x409c4464 (record 0x409c4460 7 1))
  assert "task capacity equality at the arena boundary" (reserve (input TaskAllocation 4 0x40a5a1fc empty)
    == allocated 0xa5a1fc 0x40a5a200 (record 0xa5a1fc 4 1))
  assert "task capacity refusal above the arena boundary" (reserve (input TaskAllocation 5 0x40a5a1fc empty) == AllocationSizeRejected)
  assert "task capacity refuses the reserved arena" (reserve (input TaskAllocation 4 0x40a5a200 empty) == AllocationSizeRejected)
  assert "task capacity refuses a wrapping cursor" (reserve (input TaskAllocation 4 0xffffffff empty) == AllocationSizeRejected)
  assert "zero task request at the arena boundary keeps the cursor" (reserve (input TaskAllocation 0 0x40a5a200 empty)
    == allocated 0xa5a200 0x40a5a200 (record 0xa5a200 0 1))
  assert "retained task record above the reduced bound refuses" (reserve (input TaskAllocation 16 0x40a10400 (record 0xa5a300 16 0)) == AllocationSizeRejected)
  assert "retained zero record at the arena boundary is admitted" (reserve (input TaskAllocation 0 0x40a10400 (record 0xa5a200 0 0))
    == allocated 0xa5a200 0x40a10400 (record 0xa5a200 0 1))
  assert "retained nonzero record at the arena boundary refuses" (reserve (input TaskAllocation 16 0x40a10400 (record 0xa5a200 16 0)) == AllocationSizeRejected)
  assert "storage capacity refusal" (reserve (input StorageAllocation 5 0x40a0f45c empty) == AllocationSizeRejected)
  assert "reuse does not advance cursor" (reserve (input TaskAllocation 16 0x40a10400 (record 0xa10300 16 0))
    == allocated 0xa10300 0x40a10400 (record 0xa10300 16 1))
  assert "reuse size failure precedes busy" (reserve (input StorageAllocation 12 0x409c4500 (record 0x409c4460 16 1)) == AllocationSizeRejected)
  assert "reserved record refusal" (reserve (input StorageAllocation 16 0x409c4500 (record 0x409c4460 16 1)) == AllocationBusyRejected)
  assert "source unsigned addition wraps" (reserve (input StorageAllocation 0xffffffff 0x409c4461 empty)
    == allocated 0x409c4461 0x409c445d (record 0x409c4461 0xffffffff 1))
  assert "zero requested size keeps cursor" (reserve (input StorageAllocation 0 0x409c4460 empty)
    == allocated 0x409c4460 0x409c4460 (record 0x409c4460 0 1))
  where
    assert name condition = unless condition (putStrLn ("FAIL: " ++ name) >> exitFailure)

record :: Word32 -> Word32 -> Word32 -> Reservation
record base size busy = Reservation (Address base) (ByteCount size) (reservationState (WordValue busy))
input :: AllocationKind -> Word32 -> Word32 -> Reservation -> AllocationInput
input kind bytes cursor = AllocationInput kind (ByteCount bytes) (Address cursor)
allocated :: Word32 -> Word32 -> Reservation -> AllocationResult
allocated pointer cursor = Allocated (Address pointer) (Address cursor)
