module Main (main) where

import Control.Monad (forM_, unless)
import qualified Data.ByteString as B
import Fixtures
import Replacement.LoaderOrchestration.Admission
import Replacement.LoaderOrchestration.Artifact
import Replacement.LoaderOrchestration.Interface
import System.Environment (getArgs)

assert :: String -> Bool -> IO ()
assert label premise = unless premise (ioError (userError label))

expectRefusal :: String -> ArtifactError -> Either ArtifactError a -> IO ()
expectRefusal label expected actual = assert (label++": expected "++show expected++", got "++either show (const "admitted") actual) (case actual of Left failure -> failure==expected; Right _ -> False)

check :: LoaderSide -> B.ByteString -> B.ByteString -> IO ()
check side bytes symbols = do
  artifact <- either (ioError . userError . show) pure (deriveArtifact side bytes symbols)
  assert "service frame does not preserve the source sixteen bytes" (frameLow (linkedProof artifact)==StackOffset (-16))
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
