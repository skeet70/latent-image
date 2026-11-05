module Replacement.Image5Init.Artifact
  ( ArtifactError(..), LinkedArtifact, deriveArtifact, artifactProof, stageImage5 ) where
import Control.Monad (unless)
import qualified Data.ByteString as B
import qualified FujiAF.FirstStage as F
import qualified FujiAF.UpdateContainer as U
import Replacement.Image5Init.Admission

data ArtifactError = ExtentRejected | SymbolRejected | SourceMismatch | CodeRejected AdmissionError deriving (Eq, Show)
data LinkedArtifact = LinkedArtifact B.ByteString FrameProof
require :: Bool -> ArtifactError -> Either ArtifactError ()
require b e = unless b (Left e)
deriveArtifact :: B.ByteString -> B.ByteString -> Either ArtifactError LinkedArtifact
deriveArtifact bytes symbols = do
  require (B.length bytes <= 324 && B.length bytes `mod` 4 == 0) ExtentRejected
  parsed <- either (const (Left SymbolRejected)) pure (F.parseSymbols symbols)
  let address name = [F.symbolAddress s | s <- parsed, F.symbolName s == name]
  require (address "xm1_image5_init" == [0xd985a4] && address "__image5_init_start" == [0xd9856c]
    && address "__image5_init_end" == [0xd9856c + fromIntegral (B.length bytes)]
    && case address "xm1_image5_hooks" of [a] -> a > 0xd985a4 && a < 0xd986b0; _ -> False) SymbolRejected
  proof <- either (Left . CodeRejected) pure (deriveFrameProof bytes)
  require (map callTarget (take 1 (drop 6 (proofCallSites proof))) == address "xm1_image5_hooks") SymbolRejected
  pure (LinkedArtifact bytes proof)
artifactProof :: LinkedArtifact -> FrameProof
artifactProof (LinkedArtifact _ proof) = proof
stageImage5 :: B.ByteString -> LinkedArtifact -> Either ArtifactError B.ByteString
stageImage5 original (LinkedArtifact bytes _) = do
  require (B.length original == 0x4f1e60 && U.word32At original 12 == 0xd985a4) SourceMismatch
  let offset = 0xd9856c - 0xbf1a00
      padding = B.concat (replicate ((324-B.length bytes) `div` 4) (U.putWord32 0xe320f000))
  pure (B.take offset original <> bytes <> padding <> B.drop (offset+324) original)
