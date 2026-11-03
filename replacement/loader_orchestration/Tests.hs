module Main (main) where

import qualified A32 as A
import Control.Monad (forM_, unless)
import qualified Data.ByteString as B
import qualified Data.Map.Strict as M
import qualified FujiAF.UpdateContainer as U
import Fixtures
import Replacement.LoaderOrchestration.Admission
import Replacement.LoaderOrchestration.Artifact
import Replacement.LoaderOrchestration.Interface
import System.Environment (getArgs)

assert :: String -> Bool -> IO ()
assert label premise = unless premise (ioError (userError label))

expectRefusal :: String -> ArtifactError -> Either ArtifactError a -> IO ()
expectRefusal label expected actual = assert (label++": expected "++show expected++", got "++either show (const "admitted") actual) (case actual of Left failure -> failure==expected; Right _ -> False)

expectedPhases :: Scenario -> [Phase]
expectedPhases scenario = case scenario of
  Scenario Primary _ SetupReturns -> [Setup,Root,Idle0,Diagnostic]
  Scenario Primary _ SetupNonreturn -> [Setup]
  Scenario Peer NeverPublished _ -> []
  Scenario Peer _ _ -> [PeerTasks,Diagnostic]

checkDispatcherBoundaries :: LoaderSide -> LinkedArtifact -> IO ()
checkDispatcherBoundaries side artifact = do
  let proof = linkedProof artifact
      entries = dispatcherBoundaries proof
      CodeAddress start = carrierStart side
      saved = M.fromList
        [ (StackOffset (-16),Incoming (A.Reg 4)),(StackOffset (-12),Incoming (A.Reg 5))
        , (StackOffset (-8),Incoming (A.Reg 11)),(StackOffset (-4),IncomingLink)
        ]
  forM_ scenarios $ \scenario ->
    assert ("dispatcher phases differ for "++show scenario)
      (map boundaryPhase (filter ((==scenario) . boundaryScenario) entries)==expectedPhases scenario)
  forM_ entries $ \entry -> do
    let CodeAddress instruction = boundaryInstruction entry
        CodeAddress continuation = boundaryContinuation entry
        actual = A.decode instruction (U.word32At (linkedBytes artifact) (fromIntegral (instruction-start)))
    assert "dispatcher edge has the wrong loader side" (boundarySide entry==side)
    assert "dispatcher edge lacks the source SP/FP"
      (boundaryStack entry==StackOffset (-16)&&boundaryFramePointer entry==StackOffset (-4))
    assert "dispatcher edge does not preserve the caller's saved words" (boundaryLiveWords entry==saved)
    assert "dispatcher continuation is not the emitted A32 call return" (continuation==instruction+4)
    assert "dispatcher boundary is not an emitted indirect call" (case actual of A.BranchExchange _ True _ -> True; _ -> False)
    assert "dispatcher edge differs from the existing call census" (lookup (boundaryInstruction entry) (callBoundaries proof)==Just Dispatcher)

check :: LoaderSide -> B.ByteString -> B.ByteString -> IO ()
check side bytes symbols = do
  artifact <- either (ioError . userError . show) pure (deriveArtifact side bytes symbols)
  assert "service frame does not preserve the source sixteen bytes" (frameLow (linkedProof artifact)==StackOffset (-16))
  checkDispatcherBoundaries side artifact
  changed <- either (ioError . userError) pure (mutations side artifact symbols)
  forM_ changed $ \(Mutation fault mutant mutantSymbols expected) -> expectRefusal (show side++" "++show fault) expected (deriveArtifact side mutant mutantSymbols)
  expectRefusal "unbound symbols" SymbolRejected (deriveArtifact side bytes B.empty)
  expectRefusal "unbound extra code" SymbolRejected (deriveArtifact side (bytes<>B.replicate 4 0) symbols)
  let other = case side of LoaderA -> LoaderB; LoaderB -> LoaderA
  expectRefusal "wrong loader placement" SymbolRejected (deriveArtifact other bytes symbols)

main :: IO ()
main = do
  args <- getArgs
  case args of
    [aPath,aSymbolsPath,bPath,bSymbolsPath] -> do
      a <- B.readFile aPath
      aSymbols <- B.readFile aSymbolsPath
      b <- B.readFile bPath
      bSymbols <- B.readFile bSymbolsPath
      check LoaderA a aSymbols
      check LoaderB b bSymbols
      expectRefusal "synthetic canonical source" SourceMismatch (canonicalSource B.empty B.empty B.empty B.empty)
    _ -> ioError (userError "loader-orchestration-tests requires both emitted images and symbol files")
