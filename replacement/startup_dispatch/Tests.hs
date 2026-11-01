module Main where

import Control.Monad (unless)
import qualified Data.ByteString as B
import Data.Either (isLeft)
import Data.List (nub)
import qualified FujiAF.UpdateContainer as U
import Replacement.StartupDispatch.Admission
import Replacement.StartupDispatch.Artifact
import System.Environment (getArgs)

assert :: Bool -> String -> IO ()
assert passed message = unless passed (fail message)

main :: IO ()
main = do
  arguments <- getArgs
  case arguments of
    [imagePath,symbolsPath] -> do
      image <- B.readFile imagePath
      symbols <- B.readFile symbolsPath
      _ <- either (fail . show) pure (deriveArtifact image symbols)
      proof <- either (fail . show) pure (deriveFrameProof image)
      let observed phase = nub [(events,spins) | PathProof actual events spins <- proofPaths proof, actual == phase]
      assert (observed Diagnostic == [([],False)] && observed Other == [([],False)]) "diagnostic/default return paths differ"
      assert (observed Root == [([Submit (Descriptor (TaskId 104) (NameAddress 0x781960) (Priority 1) (StackBytes 0x400) (EntryAddress 0x69d160) (Argument 0))],False)]) "root submission differs"
      assert (observed Idle0 == [([Submit (Descriptor (TaskId 102) (NameAddress 0x781968) (Priority 15) (StackBytes 0x400) (EntryAddress 0x69cbf0) (Argument 0))],False)]) "idle0 submission differs"
      assert (observed Idle1AndStandby == [([
        Submit (Descriptor (TaskId 103) (NameAddress 0x781970) (Priority 15) (StackBytes 0x400) (EntryAddress 0x69cbf0) (Argument 0)),
        Submit (Descriptor (TaskId 93) (NameAddress 0x781978) (Priority 13) (StackBytes 0x400) (EntryAddress 0x69cd58) (Argument 0))],False)]) "complete ordered phase4 submissions differ"
      let setupOutcomes =
            [ ([ModeRead,Install],False), ([ModeRead,Install],True)
            , ([ModeRead,ClearWords,Install],False), ([ModeRead,ClearWords,Install],True)
            ]
      assert (all (`elem` observed Setup) setupOutcomes && all (`elem` setupOutcomes) (observed Setup))
        "conditional setup clear or nonreturning status behavior differs"
      assert (scratchLow proof <= -48) "guest frame is not reserved"
      assert (case deriveArtifact (B.replicate 400 0) symbols of Left ExtentRejected -> True; _ -> False) "overflow carrier admitted"
      assert (case deriveArtifact image B.empty of Left SymbolRejected -> True; _ -> False) "missing symbols admitted"
      assert (case canonicalSource B.empty B.empty B.empty B.empty of Left SourceMismatch -> True; _ -> False) "synthetic source admitted as canonical"
      mapM_ (\bytes -> assert (case deriveFrameProof bytes of Left CarrierRejected -> True; _ -> False) "invalid carrier extent admitted")
        [B.empty,B.replicate 395 0,B.replicate 397 0,B.replicate 400 0]
      -- Synthetic A32 probes authored for this test.
      let unknownStore = B.concat (map U.putWord32 [0xe24dd030,0xe58d001c])
          unknownRead = B.concat (map U.putWord32 [0xe24dd030,0xe59d001c])
          impossibleReturn = B.concat (map U.putWord32 [0xe3a00000,0xe12fff1e])
      mapM_ (\bytes -> assert (isLeft (deriveFrameProof bytes)) "unsafe or behaviorally incomplete synthetic carrier admitted")
        [unknownStore,unknownRead,impossibleReturn,B.replicate 396 0]
    _ -> fail "usage: startup-dispatch-tests LINKED_BIN SYMBOLS"
