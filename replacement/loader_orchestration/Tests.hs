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

refused :: Either a b -> Bool
refused (Left _) = True
refused (Right _) = False

check :: LoaderSide -> B.ByteString -> B.ByteString -> IO ()
check side bytes symbols = do
  artifact <- either (ioError . userError . show) pure (deriveArtifact side bytes symbols)
  assert "service frame does not preserve the source sixteen bytes" (frameLow (linkedProof artifact)==StackOffset (-16))
  changed <- either (ioError . userError) pure (mutations side artifact)
  forM_ changed $ \(fault,mutant) -> assert (show side++" admitted "++show fault) (refused (deriveArtifact side mutant symbols))
  assert "unbound symbols admitted" (refused (deriveArtifact side bytes B.empty))
  assert "unbound extra code admitted" (refused (deriveArtifact side (bytes<>B.replicate 4 0) symbols))
  let other = case side of LoaderA -> LoaderB; LoaderB -> LoaderA
  assert "wrong loader placement admitted" (refused (deriveArtifact other bytes symbols))

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
      assert "synthetic source admitted as canonical firmware" (refused (canonicalSource B.empty B.empty B.empty B.empty))
    _ -> ioError (userError "loader-orchestration-tests requires both emitted images and symbol files")
