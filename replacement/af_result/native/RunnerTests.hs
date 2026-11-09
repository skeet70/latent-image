module Main (main) where

import Control.Monad (forM_, unless)
import qualified Data.ByteString as B
import qualified Data.Map.Strict as M
import Data.Word (Word32)
import Emulation.NativeObservation
import Replacement.AfResult.PoolSuite (compareObservations)

main :: IO ()
main = forM_ tests $ \(label,passed) -> unless passed (fail label)

tests :: [(String,Bool)]
tests =
  [ ("equal caller-visible state", null (compareObservations True reference reference))
  , ("defined reservation result differs", rejects reference {nativeRegisters = M.insert R0 0xffff registers})
  , ("void return scratch register is not observable", null (compareObservations False reference reference {nativeRegisters = M.insert R0 0xffff registers}))
  , ("equal failed statuses cannot qualify", not (null (compareObservations True failed failed)))
  , ("status defect", rejects failed)
  , ("pool mutation defect", rejects reference {nativeMemory = memory (B.pack [1,0,0,0]) (B.replicate 4 0)})
  , ("selection defect", rejects reference {nativeMemory = memory (B.replicate 4 0) (B.pack [1,0,0,0])})
  , ("caller frame defect", rejects reference {nativeRegisters = M.insert Sp 0x3007fc registers})
  , ("preserved register defect", rejects reference {nativeRegisters = M.insert R6 0 registers})
  , ("alternate bank defect", rejects reference {nativeMemory = [(key,if key == RegionId "late-state" then B.pack [1,0,0,0] else bytes) | (key,bytes) <- nativeMemory reference]})
  , ("caller stack guard defect", rejects reference {nativeMemory = [(key,if key == RegionId "stack" then B.cons 0 (B.tail bytes) else bytes) | (key,bytes) <- nativeMemory reference]})
  , ("equal corrupted caller frame cannot qualify", let bad = reference {nativeRegisters = M.insert Sp 0x3007fc registers} in not (null (compareObservations True bad bad)))
  ]
  where
    rejects = not . null . compareObservations True reference
    failed = reference {nativeStatus = MemoryGuard}

registers :: M.Map Register Word32
registers = M.fromList ((R0,0) : (Sp,0x300800) : [(r,0xa5a50000 + fromIntegral (fromEnum r)) | r <- [R4 .. R11]])

memory :: B.ByteString -> B.ByteString -> [(RegionId,B.ByteString)]
memory state output = [(RegionId "state",state),(RegionId "late-state",B.replicate 4 0),(RegionId "output",output),(RegionId "receipts",B.replicate 16 0),(RegionId "stack",B.replicate 0x1000 0xa5)]

reference :: NativeObservation
reference = NativeObservation Returned registers (memory (B.replicate 4 0) (B.replicate 4 0)) (Complete (ProvenanceTotal 0) []) (InstructionCount 10) (EngineError 0) Nothing
