module Fixtures (Fault(..), Mutation(..), mutations) where

import qualified A32 as A
import Data.Bits ((.&.), (.|.), complement, xor)
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import Data.List (find)
import Data.Word (Word32)
import qualified FujiAF.UpdateContainer as U
import Replacement.LoaderOrchestration.Admission
import Replacement.LoaderOrchestration.Artifact
import Replacement.LoaderOrchestration.Interface
import Numeric (showHex)

data Fault = WrongPhase | WrongTarget | WrongReturnedCode | WrongSavedFrame | WrongFramePointer
  | UnallocatedStack | OutOfRangeEffect | MissingBarrier | WrongRoleDecision | ZeroEncodedLsr | ZeroEncodedRrx
  | ProgramCounterData | ProgramCounterLoad | ProgramCounterWide | ProgramCounterTop | CounterControlledExit deriving (Eq, Show)
data Mutation = Mutation Fault B.ByteString B.ByteString ArtifactError

mutations :: LoaderSide -> LinkedArtifact -> B.ByteString -> Either String [Mutation]
mutations side artifact symbols = do
  let bytes = linkedBytes artifact
      proof = linkedProof artifact
      CodeAddress start = carrierStart side
      at (CodeAddress pc) = U.word32At bytes (fromIntegral (pc-start))
      replaceIn input (CodeAddress pc) value = let offset=fromIntegral (pc-start) in B.take offset input<>U.putWord32 value<>B.drop (offset+4) input
      replace = replaceIn bytes
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
  loaded <- select "literal load absent" (\site@(CodeAddress pc) -> case A.decode pc (at site) of A.Load {} -> True; _ -> False)
  widened <- select "wide move absent" (\site@(CodeAddress pc) -> case A.decode pc (at site) of A.MoveWide {} -> True; _ -> False)
  topped <- select "top move absent" (\site@(CodeAddress pc) -> case A.decode pc (at site) of A.MoveTop {} -> True; _ -> False)
  polled <- select "completion poll absent" (\site@(CodeAddress pc) -> case A.decode pc (at site) of A.Load _ A.Word False (A.Reg 0) (A.Reg 6) (A.OffsetImmediate 0) A.OffsetOnly -> True; _ -> False)
  initialize <- maybe (Left "private initializer call absent") (Right . fst) (find ((==Retained PrivateInitialize) . snd) (callBoundaries proof))
  let CodeAddress savedStart = carrierStart side
      phaseAssignment = CodeAddress (firstPc-4)
      (identitySite@(CodeAddress identityPc),_) = identityCall
      ordinary fault mutant failure = Mutation fault mutant symbols (CodeRejected failure)
      pcDestination fault site = ordinary fault (replace site ((at site .&. complement 0xf000) .|. 0xf000)) (ControlFlowRejected site)
      jump from target = branchTo from target .&. 0xfeffffff
      roleSite = CodeAddress (identityPc+4)
      CodeAddress pollPc = polled
      thunk = CodeAddress (start+fromIntegral (B.length bytes))
      pollThunk = case thunk of CodeAddress pc -> CodeAddress (pc+12)
      extended = replaceIn (replace roleSite (jump roleSite thunk)) polled (jump polled pollThunk)
        <> B.concat (map U.putWord32
          [ at roleSite,0xe3a02000,jump (case thunk of CodeAddress pc -> CodeAddress (pc+8)) (CodeAddress (identityPc+8))
          , at polled,0xe2822001,0xe3520002
          , jump (case pollThunk of CodeAddress pc -> CodeAddress (pc+12)) initialize .&. 0x0fffffff
          , jump (case pollThunk of CodeAddress pc -> CodeAddress (pc+16)) (CodeAddress (pollPc+4)) ])
      symbol address name = C.pack (showHex address ""++" T "++name++"\n")
      extendedSymbols = B.concat
        [ symbol start "__loader_orchestration_start"
        , symbol (start+fromIntegral (B.length extended)) "__loader_orchestration_end"
        , symbol start "xm1_loader_orchestration_entry"
        , symbol (start+12) "xm1_loader_orchestration" ]
  pure
    [ ordinary WrongPhase (replace phaseAssignment 0xe3a00004) EffectRejected
    , ordinary WrongTarget (replace identitySite (branchTo identitySite wrongTarget)) (CallRejected wrongTarget)
    , ordinary WrongReturnedCode (replace lastDispatch 0xe3a00000) EffectRejected
    , ordinary WrongSavedFrame (replace (CodeAddress savedStart) 0xe92d4850) FrameRejected
    , ordinary WrongFramePointer (replace (CodeAddress (savedStart+4)) 0xe28db008) FrameRejected
    , ordinary UnallocatedStack (replace (CodeAddress (savedStart+4)) 0xe24dd004) FrameRejected
    , ordinary OutOfRangeEffect (replace ready 0x400b0000) MemoryRejected
    , ordinary MissingBarrier (replace barrier 0xe320f000) EffectRejected
    , ordinary WrongRoleDecision (replace decision (at decision `xor` 0x10000000)) EffectRejected
    , ordinary ZeroEncodedLsr (replace moved (at moved .|. 0x20)) OperandRejected
    , ordinary ZeroEncodedRrx (replace moved (at moved .|. 0x60)) OperandRejected
    , pcDestination ProgramCounterData moved
    , pcDestination ProgramCounterLoad loaded
    , pcDestination ProgramCounterWide widened
    , pcDestination ProgramCounterTop topped
    , Mutation CounterControlledExit extended extendedSymbols (CodeRejected PollingInvariantRejected)
    ]
