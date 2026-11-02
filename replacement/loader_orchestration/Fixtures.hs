module Fixtures (Fault(..), mutations) where

import qualified A32 as A
import Data.Bits ((.&.), (.|.), xor)
import qualified Data.ByteString as B
import Data.List (find)
import Data.Word (Word32)
import qualified FujiAF.UpdateContainer as U
import Replacement.LoaderOrchestration.Admission
import Replacement.LoaderOrchestration.Artifact
import Replacement.LoaderOrchestration.Interface

data Fault = WrongPhase | WrongTarget | WrongReturnedCode | WrongSavedFrame | WrongFramePointer
  | UnallocatedStack | OutOfRangeEffect | MissingBarrier | WrongRoleDecision | ZeroEncodedLsr | ZeroEncodedRrx deriving (Eq, Show)

mutations :: LoaderSide -> LinkedArtifact -> Either String [(Fault,B.ByteString)]
mutations side artifact = do
  let bytes = linkedBytes artifact
      proof = linkedProof artifact
      CodeAddress start = carrierStart side
      at (CodeAddress pc) = U.word32At bytes (fromIntegral (pc-start))
      replace (CodeAddress pc) value = let offset=fromIntegral (pc-start) in B.take offset bytes<>U.putWord32 value<>B.drop (offset+4) bytes
      points = executionPcs proof
      select label predicate = maybe (Left label) Right (find predicate points)
      dispatchers = [site | (site,Dispatcher) <- callBoundaries proof]
      first [] = Left "dispatcher boundary absent"
      first (site:_) = Right site
      lastPoint [] = Left "dispatcher boundary absent"
      lastPoint values = Right (last values)
  CodeAddress firstPc <- first dispatchers
  lastDispatch <- lastPoint dispatchers
  case A.decode (firstPc-4) (at (CodeAddress (firstPc-4))) of
    A.DataProcessing _ A.MOV False (A.Reg 0) _ (A.Immediate 0) -> pure ()
    _ -> Left "setup phase assignment absent"
  identityCall <- maybe (Left "identity call absent") Right (find ((==Retained Identity) . snd) (callBoundaries proof))
  let branchTo :: CodeAddress -> CodeAddress -> Word32
      branchTo (CodeAddress pc) (CodeAddress target) =
        0xeb000000 .|. (fromInteger ((toInteger target-toInteger pc-8) `div` 4) .&. 0xffffff)
      wrongTarget = CodeAddress (loaderBase side+0x2700)
  ready <- maybe (Left "ready publication literal absent") (Right . CodeAddress . (start+) . fromIntegral)
    (find (\offset -> U.word32At bytes offset==readyAddress side) [0,4..B.length bytes-4])
  barrier <- select "source store barrier absent" ((==0xf57ff04f) . at)
  decision <- select "role decision branch absent" (\site@(CodeAddress pc) -> case A.decode pc (at site) of A.Branch (A.Cond condition) False _ -> condition==0||condition==1; _ -> False)
  moved <- select "dispatcher capability assignment absent" (\site@(CodeAddress pc) -> case A.decode pc (at site) of A.DataProcessing _ A.MOV False _ _ (A.ShiftedImmediate (A.Reg 0) A.LSL 0) -> True; _ -> False)
  let CodeAddress savedStart = carrierStart side
      phaseAssignment = CodeAddress (firstPc-4)
      (identitySite,_) = identityCall
  pure
    [ (WrongPhase,replace phaseAssignment 0xe3a00004)
    , (WrongTarget,replace identitySite (branchTo identitySite wrongTarget))
    , (WrongReturnedCode,replace lastDispatch 0xe3a00000)
    , (WrongSavedFrame,replace (CodeAddress savedStart) 0xe92d4850)
    , (WrongFramePointer,replace (CodeAddress (savedStart+4)) 0xe28db008)
    , (UnallocatedStack,replace (CodeAddress (savedStart+4)) 0xe24dd004)
    , (OutOfRangeEffect,replace ready 0x400b0000)
    , (MissingBarrier,replace barrier 0xe320f000)
    , (WrongRoleDecision,replace decision (at decision `xor` 0x10000000))
    , (ZeroEncodedLsr,replace moved (at moved .|. 0x20))
    , (ZeroEncodedRrx,replace moved (at moved .|. 0x60))
    ]
