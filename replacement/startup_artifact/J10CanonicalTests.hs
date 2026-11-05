{-# LANGUAGE OverloadedStrings #-}
module Main where

import Control.Monad (unless)
import qualified Data.ByteString as B
import qualified Emulation.Image4Staging as Tools
import qualified FujiAF.Host as H
import qualified FujiAF.UpdateContainer as U
import Replacement.StartupArtifact.Adapters
import Replacement.StartupArtifact.Compose
import Replacement.StartupArtifact.Joins
import Replacement.StartupArtifact.Stage
import Replacement.StartupArtifact.Targets
import Replacement.StartupArtifact.Types
import System.Directory (canonicalizePath)
import System.Environment (getArgs)

main :: IO ()
main = do
  arguments <- getArgs
  (encoder,rebuild,decoder,ctorBin,ctorSymbols,submitBin,submitSymbols,datPath,scratchPath) <- case arguments of
    [a,b,c,d,e,f,g,h,i] -> pure (a,b,c,d,e,f,g,h,i)
    _ -> H.failH "usage: startup-artifact-j10-canonical-tests ENCODER REBUILD DECODER CTOR_BIN CTOR_SYMBOLS SUBMIT_BIN SUBMIT_SYMBOLS CANONICAL_DAT SCRATCH_DIR"
  tools <- Tools.Tools <$> canonicalizePath encoder <*> canonicalizePath rebuild <*> canonicalizePath decoder
  scratch <- Tools.validateScratchDirectory scratchPath
  source <- loadCanonical tools scratch datPath
  let linked bin symbols = LinkedImage <$> (ImageBytes <$> H.readRegular 0x10000 bin) <*> (SymbolBytes <$> H.readRegular 0x10000 symbols)
      derived = either (H.failH . show) pure
      check name condition = unless condition (H.failH ("failed: " ++ name))
  ctorInput <- linked ctorBin ctorSymbols
  constructorsPiece <- derived (deriveTaskConstructors source ctorInput)
  submission <- linked submitBin submitSymbols >>= derived . deriveTaskSubmission source
  composition <- derived (compose registeredJoins source [constructorsPiece,submission])
  check "constructor and task-submission compose with their registered join"
    (any (\(AppliedJoin join _) -> joinKind join == ConstructorTaskSubmission) (compositionJoins composition))
  check "omitting constructor-task-submission refuses the unjoined service entry"
    (case compose [join | join <- registeredJoins, joinKind join /= ConstructorTaskSubmission] source [constructorsPiece,submission] of
      Left (FootprintOverlap TaskConstructors TaskSubmission _) -> True
      _ -> False)
  let before = targetOf (canonicalTargets source) Image5
      after = targetOf (compositionTargets composition) Image5
      slice image start end = B.take (end-start) (B.drop (start-0xbf1a00) image)
  check "hook wrappers remain stock"
    (and [slice before low high == slice after low high | (low,high) <- [(0x107277c,0x1072794),(0x1066304,0x106631c),(0x1062114,0x106212c)]])
  check "syscont UI constructor seam remains stock" (slice before 0xec56a0 0xec56a4 == slice after 0xec56a0 0xec56a4)
  let LinkedImage (ImageBytes packed) symbols = ctorInput
  check "truncated packed constructor image refuses"
    (case deriveTaskConstructors source (LinkedImage (ImageBytes (B.take (B.length packed-1) packed)) symbols) of
      Left (AdapterRefused TaskConstructors _) -> True
      _ -> False)
  check "constructor piece owns image4 and image5"
    (all (\target -> any ((== target) . spanTarget) (pieceCarriers constructorsPiece)) [Image4,Image5])
  check "a constructor that loses its frame refuses"
    (case deriveTaskConstructors source (LinkedImage (ImageBytes (U.putWord32 0xe320f000 <> B.drop 4 packed)) symbols) of
      Left (AdapterRefused TaskConstructors _) -> True
      _ -> False)
  putStrLn "J10 canonical constructor composition checks passed"
