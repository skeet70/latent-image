module Main (main) where

import qualified A32 as A
import Control.Monad (forM_, unless)
import qualified Data.ByteString as B
import qualified Data.Map.Strict as M
import qualified Emulation.Sha256 as SHA
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

checkServiceEntries :: LinkedArtifact -> IO ()
checkServiceEntries artifact = do
  let proof = linkedProof artifact
      entries = serviceEntryStates proof
      calls = [(site,service) | (site,Retained service) <- callBoundaries proof]
  assert "service entry states differ from the retained call census"
    (all (\entry -> lookup (entrySite entry) calls==Just (entryService entry)) entries
      && all (\(site,_) -> any ((==site) . entrySite) entries) calls)
  forM_ entries $ \entry -> do
    let CodeAddress site = entrySite entry
    assert "service entry lr is not the call return" (entryRegister entry A.lr==Word (site+4))
    assert "service entry is not at the sixteen-byte frame" (entryRegister entry A.sp==Stack (StackOffset (-16)))
  forM_ scenarios $ \scenario ->
    assert ("service entries differ from the effect order for "++show scenario)
      ([entryService entry | entry <- entries, entryScenario entry==scenario]
        ==[service | ServiceCall service _ <- concat [effects | PathProof selected effects _ <- proofPaths proof, selected==scenario]])

check :: LoaderSide -> B.ByteString -> B.ByteString -> IO ()
check side bytes symbols = do
  artifact <- either (ioError . userError . show) pure (deriveArtifact side bytes symbols)
  assert "service frame does not preserve the source sixteen bytes" (frameLow (linkedProof artifact)==StackOffset (-16))
  checkDispatcherBoundaries side artifact
  checkServiceEntries artifact
  changed <- either (ioError . userError) pure (mutations side artifact symbols)
  forM_ changed $ \(Mutation fault mutant mutantSymbols expected) -> expectRefusal (show side++" "++show fault) expected (deriveArtifact side mutant mutantSymbols)
  expectRefusal "unbound symbols" SymbolRejected (deriveArtifact side bytes B.empty)
  expectRefusal "unbound extra code" SymbolRejected (deriveArtifact side (bytes<>B.replicate 4 0) symbols)
  let other = case side of LoaderA -> LoaderB; LoaderB -> LoaderA
  expectRefusal "wrong loader placement" SymbolRejected (deriveArtifact other bytes symbols)

-- | The canonical DAT's extracted loaders and the staged candidate equal the
-- digests qualified by E-LOADER-ORCHESTRATION-HOST-ADMITTED-150.
checkCanonical :: B.ByteString -> B.ByteString -> B.ByteString -> B.ByteString -> B.ByteString -> B.ByteString -> IO ()
checkCanonical dat image4 a aSymbols b bSymbols = do
  let digest = SHA.sha256Hex . SHA.sha256
      admitted label = either (ioError . userError . ((label++": ")++) . show) pure
  source <- admitted "canonical source" (canonicalSource dat image4)
  assert "extracted loader A differs from loader-40000.bin"
    (digest (sourceLoader source LoaderA)=="21366ffff7cebc6176b8a8125bf3a9974835a51070a58ea325f7eed8aabd8882")
  assert "extracted loader B differs from loader-d0000.bin"
    (digest (sourceLoader source LoaderB)=="8fb0c322a22443a3783cdab9071db17212a5629f6375646218ac1e043dc672ec")
  linkedA <- admitted "loader A artifact" (deriveArtifact LoaderA a aSymbols)
  linkedB <- admitted "loader B artifact" (deriveArtifact LoaderB b bSymbols)
  candidate <- admitted "staged candidate" (stageCandidate source linkedA linkedB)
  assert "staged candidate differs from the qualified candidate.DAT"
    (digest (candidateBytes candidate)=="4ea1a92b8f8193d9352db00ea1c772349a1aa04a3965d186d671ad6fd27ad627")

main :: IO ()
main = do
  args <- getArgs
  let images aPath aSymbolsPath bPath bSymbolsPath = do
        a <- B.readFile aPath
        aSymbols <- B.readFile aSymbolsPath
        b <- B.readFile bPath
        bSymbols <- B.readFile bSymbolsPath
        check LoaderA a aSymbols
        check LoaderB b bSymbols
        expectRefusal "synthetic canonical source" SourceMismatch (canonicalSource B.empty B.empty)
        pure (a,aSymbols,b,bSymbols)
  case args of
    [aPath,aSymbolsPath,bPath,bSymbolsPath] -> () <$ images aPath aSymbolsPath bPath bSymbolsPath
    [aPath,aSymbolsPath,bPath,bSymbolsPath,datPath,image4Path] -> do
      (a,aSymbols,b,bSymbols) <- images aPath aSymbolsPath bPath bSymbolsPath
      dat <- B.readFile datPath
      image4 <- B.readFile image4Path
      checkCanonical dat image4 a aSymbols b bSymbols
    _ -> ioError (userError "loader-orchestration-tests requires both emitted images and symbol files, optionally followed by CANONICAL_DAT IMAGE4")
