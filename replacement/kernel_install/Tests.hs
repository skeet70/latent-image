module Main where

import Control.Monad (forM_, unless)
import qualified Data.ByteString as B
import Fixtures
import Replacement.KernelInstall.Artifact
import System.Environment (getArgs)

assert :: Bool -> String -> IO ()
assert passed message = unless passed (fail message)

refuses :: ArtifactError -> Either ArtifactError a -> Bool
refuses expected (Left actual) = actual == expected
refuses _ (Right _) = False

main :: IO ()
main = do
  arguments <- getArgs
  case arguments of
    [imagePath,symbolsPath] -> do
      image <- B.readFile imagePath
      symbols <- B.readFile symbolsPath
      _ <- either (fail . show) pure (deriveArtifact image symbols)
      forM_ [Control,EquivalentImmediate,UnreachableNop] $ \control -> do
        let (bytes,names) = fixture control
        _ <- either (fail . ((show control ++ ": ") ++) . show) pure (deriveArtifact bytes names)
        pure ()
      forM_ [WrongHeader .. maxBound] $ \fault -> do
        let (bytes,names) = fixture fault
        expected <- maybe (fail ("missing refusal contract: " ++ show fault)) pure (expectedRefusal fault)
        assert (refuses (CodeRejected expected) (deriveArtifact bytes names)) ("refusal reason differs: " ++ show fault)
      forM_ [B.empty,B.replicate 335 0,B.replicate 340 0] $ \bytes ->
        assert (refuses ExtentRejected (deriveArtifact bytes symbols)) "invalid carrier extent refusal differs"
      assert (refuses SymbolRejected (deriveArtifact image B.empty)) "missing symbols refusal differs"
      assert (refuses SourceMismatch (canonicalSource B.empty B.empty B.empty B.empty)) "noncanonical source refusal differs"
      putStrLn "kernel installation: emitted image, three positive fixtures and 25 typed refusals pass"
    _ -> fail "usage: kernel-install-tests LINKED_BIN SYMBOLS"
