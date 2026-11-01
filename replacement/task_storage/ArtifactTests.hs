module Main where

import Control.Monad (forM, unless)
import Data.Bits ((.&.), (.|.), shiftR)
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import Data.Either (isLeft)
import Data.List (nub, sort)
import Data.Word (Word32)
import Numeric (showHex)
import Replacement.TaskStorage.Artifact
  ( deriveArtifact, artifactAdmission, Admission (..), Scratch (..), ByteCount (..)
  , Access (..), Location (..), DataRoot (..), Premise (..)
  )
import qualified FujiAF.UpdateContainer as U

-- Synthetic A32 inputs specify public admission boundaries, not firmware execution.
newtype Program = Program [Word32]
data Expected = Admit ByteCount [DataRoot] | Refuse
data Scenario = Scenario String Expected Program

main :: IO ()
main = do
  outcomes <- forM scenarios $ \(Scenario name expected program) -> do
    let (image, symbols) = artifact program
        actual = deriveArtifact image symbols
        passed = case expected of
          Admit bytes roots -> case actual of
            Left _ -> False
            Right admitted ->
              let proof = artifactAdmission admitted
                  observedRoots = sort (nub [root | Access _ _ (GuestWord root _) <- admissionAccesses proof])
              in scratchBytes (admissionScratch proof) == bytes && observedRoots == sort roots
                && all (`elem` admissionPremises proof)
                  [GuestWritesDisjointImageAndAllocatedFrame,OwnFrameStableStorage]
          Refuse -> isLeft actual
    putStrLn (name ++ ": " ++ if passed then "passed" else "FAILED")
    pure passed
  unless (and outcomes) (fail "task-storage public artifact admission regressions failed")

scenarios :: [Scenario]
scenarios =
  [ Scenario "leaf return preserves caller with no frame" (Admit (ByteCount 0) []) (Program [0xe3a00000,0xe12fff1e])
  , Scenario "callee spill and block restore preserve caller" (Admit (ByteCount 8) []) (Program
      [0xe92d4010,0xe1a04000,0xe3a00000,0xe8bd8010])
  , Scenario "single-transfer frame allocation and return preserve caller" (Admit (ByteCount 4) []) (Program
      [0xe52de004,0xe49df004])
  , Scenario "frame alias load writeback remains within allocated frame" (Admit (ByteCount 8) []) (Program
      [0xe92d4010,0xe1a0300d,0xe4934004,0xe493e004,0xe1a0d003,0xe12fff1e])
  , Scenario "output word access requires no new guest alignment check" (Admit (ByteCount 0) [Output]) (Program
      [0xe5821000,0xe5920000,0xe12fff1e])
  , Scenario "generic incoming index reaches only the indexed requested-size word" (Admit (ByteCount 0) [IndexedState]) (Program
      [0xe30fcc44,0xe344c0a0,0xe0800080,0xe08c3100,0xe5831004,0xe12fff1e])
  , Scenario "cursor value can be copied to output without becoming a pointer" (Admit (ByteCount 0) [Cursor,Output]) (Program
      [0xe30e3a98,0xe3443089,0xe5930000,0xe5820000,0xe12fff1e])
  , Scenario "callee register clobber cannot return" Refuse (Program [0xe1a04000,0xe12fff1e])
  , Scenario "callee spill overwrite cannot restore caller identity" Refuse (Program
      [0xe92d4010,0xe58d0000,0xe8bd8010])
  , Scenario "destroyed incoming return cannot be used as control" Refuse (Program
      [0xe3a0e000,0xe12fff1e])
  , Scenario "overwritten saved return cannot be loaded into PC" Refuse (Program
      [0xe92d4000,0xe58d1000,0xe8bd8000])
  , Scenario "stack alias arithmetic cannot hide a return overwrite" Refuse (Program
      [0xe92d4000,0xe1a0300d,0xe2233000,0xe5831000,0xe8bd8000])
  , Scenario "deallocated frame words cannot regain saved return provenance" Refuse (Program
      [0xe92d4000,0xe28dd004,0xe24dd004,0xe59df000])
  , Scenario "frame load cannot reach caller storage" Refuse (Program
      [0xe92d4000,0xe59df004])
  , Scenario "frame store cannot reach unallocated storage" Refuse (Program
      [0xe50d1004,0xe12fff1e])
  , Scenario "unknown guest-loaded word cannot manufacture a store capability" Refuse (Program
      [0xe5923000,0xe5831000,0xe12fff1e])
  , Scenario "unknown scalar argument cannot manufacture a load capability" Refuse (Program
      [0xe5910000,0xe12fff1e])
  , Scenario "unrelated absolute address cannot authorize a store" Refuse (Program
      [0xe3033000,0xe34430a0,0xe5831000,0xe12fff1e])
  , Scenario "indexed state requires the source twelve-byte stride" Refuse (Program
      [0xe30fcc44,0xe344c0a0,0xe08c3180,0xe5831000,0xe12fff1e])
  , Scenario "indexed state access cannot reach the next record" Refuse (Program
      [0xe30fcc44,0xe344c0a0,0xe0800080,0xe08c3100,0xe583100c,0xe12fff1e])
  , Scenario "cursor access cannot reach an adjacent word" Refuse (Program
      [0xe30e3a98,0xe3443089,0xe5831004,0xe12fff1e])
  , Scenario "output access cannot reach a second word" Refuse (Program
      [0xe5821004,0xe12fff1e])
  , Scenario "own-frame address cannot escape through output" Refuse (Program
      [0xe582d000,0xe12fff1e])
  , Scenario "caller-register load writeback cannot retain its identity" Refuse (Program
      [0xe4990004,0xe12fff1e])
  , Scenario "single-store writeback cannot overlap its source" Refuse (Program
      [0xe4822004,0xe12fff1e])
  , Scenario "single-load writeback cannot overlap its destination" Refuse (Program
      [0xe4922004,0xe12fff1e])
  , Scenario "block-load writeback cannot overlap a destination" Refuse (Program
      [0xe92d4010,0xe1a0400d,0xe8b44010,0xe12fff1e])
  , Scenario "block transfer cannot include SP in its register list" Refuse (Program
      [0xe92d6000,0xe8bd8000])
  , Scenario "empty block transfer remains outside admission" Refuse (Program
      [0xe92d0000,0xe12fff1e])
  , Scenario "PC cannot be a register memory index" Refuse (Program
      [0xe7821faf,0xe12fff1e])
  , Scenario "guest data cannot be loaded as a return target" Refuse (Program
      [0xe592f000])
  , Scenario "status-restoring MOV PC cannot be an ordinary return" Refuse (Program
      [0xe1b0f00e])
  , Scenario "user-register block load cannot be an ordinary return" Refuse (Program
      [0xe92d4000,0xe8fd8000])
  , Scenario "unprivileged transfer remains outside admission" Refuse (Program
      [0xe4b20004,0xe12fff1e])
  , Scenario "unproved conditional callee clobber cannot be skipped" Refuse (Program
      [0x13a04000,0xe12fff1e])
  , Scenario "unbounded control cycle cannot establish a returning frame" Refuse (Program
      [0xeafffffe])
  ]

artifact :: Program -> (B.ByteString, B.ByteString)
artifact (Program body) =
  let start = 0x725cc4
      rust = start + 24
      end = rust + fromIntegral (length body * 4)
      call = branch 0xeb000000 (start + 12) rust
      resume = branch 0xea000000 (start + 20) 0x725da8
      adapter = [0xe1a00004,0xe1a01007,0xe1a02008,call,0xe1a04000,resume]
      symbols =
        [ (start,0,"T","__task_storage_start"), (end,0,"T","__task_storage_end")
        , (start,24,"T","xm1_task_storage_entry")
        , (rust,fromIntegral (length body * 4),"T","xm1_task_storage_transition")
        , (0x725da8,0,"A","xm1_task_storage_resume")
        , (0x725dd4,0,"A","xm1_task_storage_pool_base_literal")
        ]
  in (B.concat (map U.putWord32 (adapter ++ body)), C.pack (unlines (map symbolLine symbols)))

branch :: Word32 -> Word32 -> Word32 -> Word32
branch opcode site target = opcode .|. (((target - site - 8) `shiftR` 2) .&. 0xffffff)

symbolLine :: (Word32, Word32, String, String) -> String
symbolLine (address, bytes, kind, name) = unwords [showHex address "",showHex bytes "",kind,name]
