{-# LANGUAGE OverloadedStrings #-}
module Main where

import Control.Monad (forM_, unless)
import qualified Data.Aeson as J
import Data.Aeson ((.:))
import Data.Aeson.Types (parseEither)
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import Data.Word (Word32)
import Fixtures
import qualified FujiAF.UpdateContainer as U
import Replacement.RootTask.Admission
import Replacement.RootTask.Artifact
import System.Environment (getArgs)

assert :: Bool -> String -> IO ()
assert premise message = unless premise (fail message)
right :: Show error => Either error value -> IO value
right = either (fail . show) pure
refuses :: Either error value -> Bool
refuses (Left _) = True
refuses _ = False

checkArtifact :: B.ByteString -> B.ByteString -> IO LinkedArtifact
checkArtifact bytes symbols = do
  artifact <- right (deriveArtifact bytes symbols)
  let proof = artifactProof artifact
      sites = callSites proof
  assert (artifactBytes artifact == bytes) "artifact changed admitted bytes"
  assert (scenarioCount proof == 256 && not (null (haltPcs proof)) && not (null (returnPcs proof)))
    "root proof lost a checked-result scenario or terminal outcome"
  assert (all (\pc -> pc `elem` executionPcs proof) (haltPcs proof ++ returnPcs proof ++ map callPc sites))
    "root proof contains unexecuted terminal or call metadata"
  assert (all (`elem` map callTarget sites) [0x7247ac,0x69ca24,0x69ca44,0x725424,0x72612c,0x6a59b8])
    "root proof lost a required retained effect"
  assert (scratchLow proof <= -8 && scratchLow proof >= -992) "root stack bound lost"
  assert (map fst (callPositions proof) == sites && all (not . null . snd) (callPositions proof))
    "call positions do not cover exactly the call sites"
  assert (sum [ length [() | EffectPosition 0 0 1 0 _ <- positions]
              | (site,positions) <- callPositions proof, callTarget site == 0x7247ac ] == 97)
    "the resource call sites do not serve 97 effects in the natural scenario"
  forM_ [B.empty,B.take 7 bytes,B.replicate 344 0] $ \invalid ->
    assert (refuses (deriveArtifact invalid symbols)) "invalid carrier admitted"
  assert (refuses (deriveArtifact bytes B.empty)) "missing symbols admitted"
  assert (refuses (deriveArtifact bytes (symbols <> symbols))) "duplicate symbols admitted"
  assert (refuses (deriveArtifact bytes (symbols <> "malformed\n"))) "malformed symbols admitted"
  forM_ ["__root_task_start","__root_task_end","xm1_root_task","root_cleanup","root_periodic_callback","root_periodic_context"] $ \name -> do
    let changed = C.unlines [case C.words line of
          [address,size,kind,symbol] | symbol == name -> C.unwords [address,size,if kind == "A" then "T" else "A",symbol]
          _ -> line | line <- C.lines symbols]
    assert (refuses (deriveArtifact bytes changed)) "wrong symbol section kind admitted"
  pure artifact

checkSource :: LinkedArtifact -> FilePath -> FilePath -> IO ()
checkSource artifact datPath imagePath = do
  dat <- B.readFile datPath
  image <- B.readFile imagePath
  source <- right (canonicalSource dat image)
  let staged = stageImage4 source artifact
      result = stagedBytes staged
      offset = 0x69d160-0x61c400
      linked = artifactBytes artifact
      padding = B.concat (replicate ((340-B.length linked) `div` 4) (U.putWord32 0xe320f000))
  assert (B.length result == B.length image && B.take offset result == B.take offset image
    && B.drop (offset+340) result == B.drop (offset+340) image) "root edit escaped its carrier"
  assert (B.take 340 (B.drop offset result) == linked <> padding) "root carrier does not contain admitted bytes"
  assert (B.take 16 (B.drop (0x69cbf0-0x61c400) result) == B.take 16 (B.drop (0x69cbf0-0x61c400) image))
    "root-only staging changed the shared idle body"
  (entry,extent,pcs,halts,returns,count) <- right (parseEither (J.withObject "root contract" $ \value ->
    (,,,,,) <$> value .: "entry" <*> value .: "replaced_extent" <*> value .: "execution_pcs"
      <*> value .: "halt_pcs" <*> value .: "return_pcs" <*> value .: "scenario_count") (contractValue staged))
  let proof = artifactProof artifact
  assert (entry == (0x69d160 :: Word32) && extent == ([0x69d160,0x69d2b4] :: [Word32])
    && pcs == executionPcs proof && halts == haltPcs proof && returns == returnPcs proof && count == scenarioCount proof)
    "staged metadata differs from admitted behavior"
  assert (refuses (canonicalSource (dat <> B.singleton 0) image)) "noncanonical DAT admitted"
  assert (refuses (canonicalSource dat (image <> B.singleton 0))) "noncanonical image4 admitted"

main :: IO ()
main = do
  forM_ [Compact,SavedFrame,TailCleanup,LiteralArguments,OrMarker] $ \layout -> do
    let bytes = fixture layout Intact
    artifact <- checkArtifact bytes (symbolsFor bytes)
    let kinds = map callKind (callSites (artifactProof artifact))
    assert (if layout == TailCleanup then TailCall `elem` kinds else all (== Call) kinds)
      "cleanup handback call kind differs from emitted transfer"
  forM_ [SkipResource,DuplicateResource,WrongConfig,CleanupSuccess,PcMoveWide,PcMoveTop,PcArithmetic
    ,PcOperand,PcStore,PcMultiStore,DriftHalt,DriftResource,HiddenCode,ExternalStore,BadReturn
    ,UnbalancedReturn,WrongOrder,BranchOnIgnored,LoopNeverEnds,LoadPcLiteral,Unsupported,OrUnknown,OrFlags,OrPc,OrStack,OrRegister] $ \damage ->
    assert (refuses (deriveRootProof (fixture Compact damage))) ("forbidden root behavior admitted: " ++ show damage)
  assert (refuses (canonicalSource B.empty B.empty)) "noncanonical source admitted"
  arguments <- getArgs
  case arguments of
    [imagePath,symbolsPath] -> do
      bytes <- B.readFile imagePath
      symbols <- B.readFile symbolsPath
      _ <- checkArtifact bytes symbols
      pure ()
    [imagePath,symbolsPath,datPath,sourceImagePath] -> do
      bytes <- B.readFile imagePath
      symbols <- B.readFile symbolsPath
      artifact <- checkArtifact bytes symbols
      checkSource artifact datPath sourceImagePath
    _ -> fail "usage: root-task-tests LINKED_BIN SYMBOLS [CANONICAL_DAT CANONICAL_IMAGE4]"
  putStrLn "root task finite host-admission tests passed; no runtime acceptance"
