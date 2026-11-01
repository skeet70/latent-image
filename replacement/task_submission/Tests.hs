module Main where

import Control.Monad (forM_, unless)
import qualified Data.ByteString as B
import Data.Either (isLeft)
import Data.List (nub)
import Fixtures
import qualified FujiAF.Host as H
import Replacement.TaskSubmission.Admission
import Replacement.TaskSubmission.Artifact
import System.Environment (getArgs)

assert :: Bool -> String -> IO ()
assert premise message = unless premise (fail message)
admit :: (B.ByteString,B.ByteString) -> IO LinkedArtifact
admit (bytes,symbols) = either (fail . show) pure (deriveArtifact bytes symbols)

main :: IO ()
main = do
  arguments <- getArgs
  case arguments of
    [nativePath,symbolPath] -> nativeTests nativePath symbolPath
    [nativePath,symbolPath,datPath,imagePath,loaderAPath,loaderBPath] -> do
      nativeTests nativePath symbolPath
      stagingTests [datPath,imagePath,loaderAPath,loaderBPath]
    _ -> fail "usage: task-submission-tests LINKED_BIN SYMBOLS [SOURCE_DAT IMAGE4 LOADER_A LOADER_B]"

nativeTests :: FilePath -> FilePath -> IO ()
nativeTests nativePath symbolPath = do
  native <- H.readRegular 852 nativePath
  symbols <- H.readRegular 0x10000 symbolPath
  _ <- admit (native,symbols)
  forM_ [Control,EquivalentMove,UnexecutedPadding,AlternateTargetRegister] $ \control -> do
    _ <- admit (fixture control)
    pure ()
  forM_ [GuestWrite,UnknownOffsetGuestWrite] $ \control -> do
    checked <- admit (fixture control)
    assert (globalWrites (frameProof checked) == [CodeAddress (0x725dd8+212)]) "nonstack store lacks an operand checkpoint"
  forM_ [MisalignedCall .. maxBound] $ \fault -> do
    let (bytes,names) = fixture fault
    assert (isLeft (deriveArtifact bytes names)) ("malformed artifact admitted: " ++ show fault)
  assert (isLeft (deriveArtifact native B.empty)) "missing symbols admitted"
  assert (isLeft (canonicalSource B.empty B.empty B.empty B.empty)) "unbound source admitted"
  artifact <- admit (fixture Control)
  let proof = frameProof artifact
      boundaries = filter ((== Submission) . callEntry) (callBoundaries proof)
      targets = nub (map callTarget boundaries)
      demands = [CalleeDemand target (StackOffset (-16)) | target <- targets]
      context = StackContext 0x41001000 (StackInterval (StackOffset (-256)) (StackOffset 0)) []
      join = joinStack proof Submission
  alternate <- admit (fixture AlternateTargetRegister)
  let callsFor target = filter ((== target) . callTarget) . callBoundaries . frameProof
      auxiliary = callsFor AuxiliaryService artifact
      creation = callsFor CreationService artifact
  assert (not (null auxiliary) && not (null creation)) "parameterized calls missing"
  assert (map callTargetRegister auxiliary /= map callTargetRegister (callsFor AuxiliaryService alternate)) "actual indirect register was not derived"
  assert (any ((== Nothing) . callTargetRegister) boundaries && any ((/= Nothing) . callTargetRegister) boundaries) "direct/indirect fixed call forms missing"
  forM_ [(auxiliary,2),(creation,6)] $ \(parameterCalls,count) ->
    forM_ parameterCalls $ \boundary -> do
      assert (callR0 boundary == ConstantArgument 1) "encoded argument provenance differs"
      assert (length (callParameters boundary) == count) "parameter initialization footprint differs"
      case callR1 boundary of
        StackArgument offset -> assert (map parameterOffset (take 1 (callParameters boundary)) == [offset]) "parameter pointer provenance differs"
        _ -> fail "parameter pointer lost stack provenance"
  forM_ boundaries $ \boundary -> do
    let CodeAddress instruction = callInstruction boundary
        CodeAddress continuation = callContinuation boundary
    assert (continuation == instruction+4) "call continuation differs from the A32 ABI"
  assert (all (\boundary -> let StackOffset at = callStack boundary in at `mod` 8 == 0) boundaries) "call SP alignment differs"
  joined <- either (fail . show) pure (join context demands)
  let StackOffset nativeLow = scratchLow proof Submission
      StackOffset combinedLow = joinedLow joined
  assert (combinedLow < nativeLow) "nested demand was not added at the actual call SP"
  assert (isLeft (join context [])) "missing nested demand admitted"
  assert (isLeft (join context {incomingStackAddress = 0x41001004} demands)) "misaligned entry admitted"
  assert (isLeft (join context {incomingStackAddress = 8} demands)) "stack arithmetic underflow admitted"
  let nativeOnly = context {writableStack = StackInterval (StackOffset nativeLow) (StackOffset 0)}
  assert (isLeft (join nativeOnly demands)) "individually bounded frames admitted without combined backing"
  let conflicting = context {liveObjects = [StackInterval (StackOffset combinedLow) (StackOffset nativeLow)]}
  assert (isLeft (join conflicting demands)) "combined scratch overlaps live object"
  assert (isLeft (join nativeOnly {incomingStackAddress = 0x41001004} demands)) "combined scratch/call-SP failure admitted"
  assert (isLeft (join context [CalleeDemand target (StackOffset (-15)) | target <- targets])) "unaligned callee demand admitted"
  putStrLn "task submission: public admission and stack composition tests complete"

stagingTests :: [FilePath] -> IO ()
stagingTests paths = do
  files <- sequence [H.readRegular cap path | (cap,path) <- zip [64*1024*1024,0x300000,0x20000,0x20000] paths]
  case files of
    [dat,image,loaderA,loaderB] -> do
      source <- either (fail . show) pure (canonicalSource dat image loaderA loaderB)
      artifact <- admit (fixture Control)
      let staged = stagedBytes (stageImage4 source artifact)
          offset = 0x725dd8-0x61c400
      assert (B.length staged == B.length image) "staging changed decoded image extent"
      assert (B.take offset staged == B.take offset image) "staging changed image prefix"
      assert (B.take 852 (B.drop offset staged) == linkedBytes artifact) "staging did not replace all 852 owned bytes"
      assert (B.drop (offset+852) staged == B.drop (offset+852) image) "staging changed image suffix"
      forM_ ([0..3] :: [Int]) $ \changed -> do
        let damage index bytes = if index == changed then B.snoc bytes 0 else bytes
            altered = zipWith damage [0..3] [dat,image,loaderA,loaderB]
        case altered of
          [d,i,a,b] -> assert (isLeft (canonicalSource d i a b)) "altered canonical dependency admitted"
          _ -> fail "source fixture arity differs"
      putStrLn "task submission: canonical source and complete image4 staging tests complete"
    _ -> fail ("expected four canonical inputs: " ++ show canonicalPaths)
