module Main where

import Control.Monad (forM_, unless)
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import Data.Word (Word32)
import Numeric (showHex)
import Replacement.PlainLoad.Artifact
import System.Environment (getArgs)

data Symbol = Entry | RustBody | End
newtype SymbolAddress = SymbolAddress Word32

assert :: Bool -> String -> IO ()
assert premise message = unless premise (fail message)

refuses :: ArtifactError -> Either ArtifactError a -> Bool
refuses expected (Left actual) = expected == actual
refuses _ (Right _) = False

main :: IO ()
main = do
  arguments <- getArgs
  case arguments of
    [imagePath,symbolsPath] -> do
      image <- B.readFile imagePath
      symbols <- B.readFile symbolsPath
      _ <- either (fail . show) pure (deriveArtifact image symbols)
      forM_ [B.empty,B.replicate 243 0,B.replicate 248 0] $ \bytes ->
        assert (refuses ExtentRejected (deriveArtifact bytes symbols)) "carrier extent accepted"
      assert (refuses SymbolRejected (deriveArtifact image B.empty)) "missing symbols accepted"
      assert (refuses SymbolRejected (deriveArtifact image (changeSymbol Entry (SymbolAddress 0x6a9848) symbols)))
        "interior entry accepted"
      assert (refuses SymbolRejected (deriveArtifact image (changeSymbol RustBody (SymbolAddress 0x6a9844) symbols)))
        "overlapping adapter and Rust entry accepted"
      let end = SymbolAddress (0x6a9844+fromIntegral (B.length image)+4)
      assert (refuses ExtentRejected (deriveArtifact image (changeSymbol End end symbols)))
        "symbol extent different from bytes accepted"
      assert (refuses SourceMismatch (canonicalSource B.empty B.empty)) "noncanonical source accepted"
      case deriveArtifact (B.replicate (B.length image) 0) symbols of
        Left (CodeRejected _) -> pure ()
        _ -> fail "invalid instruction body bypassed semantic admission"
      putStrLn "plain loading artifact: emitted image and typed source/symbol/extent/code refusals pass"
    _ -> fail "usage: plain-load-artifact-tests LINKED_BIN SYMBOLS"

changeSymbol :: Symbol -> SymbolAddress -> B.ByteString -> B.ByteString
changeSymbol wanted (SymbolAddress replacement) = C.unlines . map change . C.lines
  where
    symbolName = case wanted of
      Entry -> "xm1_plain_load_entry"
      RustBody -> "xm1_plain_load"
      End -> "__plain_load_end"
    change line = case C.words line of
      [_,size,kind,name] | name == C.pack symbolName ->
        C.unwords [C.pack (showHex replacement ""),size,kind,name]
      _ -> line
