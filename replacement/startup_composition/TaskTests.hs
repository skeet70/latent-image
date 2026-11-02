{-# LANGUAGE OverloadedStrings #-}
module Main (main) where

import Control.Monad (forM_, unless)
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import Data.Either (isLeft)
import Data.List (nub)
import qualified Emulation.CriticalPair as Critical
import qualified Fixtures
import qualified FujiAF.UpdateContainer as U
import Replacement.StartupComposition.TaskSubmission
import qualified Replacement.TaskStorage.Artifact as Storage
import qualified Replacement.TaskSubmission.Admission as A
import qualified Replacement.TaskSubmission.Artifact as Wrapper
import System.Environment (getArgs)

assert :: String -> Bool -> IO ()
assert label condition = unless condition (fail label)
accept :: Show failure => Either failure value -> IO value
accept = either (fail . show) pure
refuse :: String -> Either failure value -> IO ()
refuse label = assert label . isLeft

main :: IO ()
main = do
  args <- getArgs
  case args of
    [w,ws,c,cs,s,ss] -> nativeTests [w,ws,c,cs,s,ss] >> pure ()
    [w,ws,c,cs,s,ss,d,i,a,b] -> do
      artifact <- nativeTests [w,ws,c,cs,s,ss]
      stagingTests artifact [d,i,a,b]
    _ -> fail "usage: task-composition-tests WRAPPER WRAPPER_SYMBOLS CRITICAL CRITICAL_SYMBOLS STORAGE STORAGE_SYMBOLS [DAT IMAGE4 LOADER_A LOADER_B]"

nativeTests :: [FilePath] -> IO JoinedArtifact
nativeTests paths = do
  files <- mapM B.readFile paths
  (wrapper,critical,storage) <- case files of
    [w,ws,c,cs,s,ss] -> pure (NativeFiles w ws,NativeFiles c cs,NativeFiles s ss)
    _ -> fail "native fixture arity differs"
  artifact <- accept (deriveArtifact wrapper critical storage)
  let proof = joinedProof artifact
      wrapperProof = Wrapper.frameProof (wrapperArtifact artifact)
      owned target = target `elem` map A.FixedService [A.CriticalEnter,A.CriticalExit,A.Reservation]
      retained = [A.CalleeDemand target (A.StackOffset (-64)) | target <- nub (map A.callTarget (A.callBoundaries wrapperProof)),not (owned target)]
      context = A.StackContext 0x400f4000 (A.StackInterval (A.StackOffset (-8192)) (A.StackOffset 0)) []
  forM_ [A.Submission,A.Release] $ \entry -> do
    _ <- accept (joinStack artifact entry context retained)
    refuse "insufficient writable backing admitted" (joinStack artifact entry
      (context {A.writableStack = A.StackInterval (A.StackOffset (-8)) (A.StackOffset 0)}) retained)
    refuse "misaligned incoming stack admitted" (joinStack artifact entry (context {A.incomingStackAddress = 0x400f4004}) retained)
    refuse "overlapping live object admitted" (joinStack artifact entry
      (context {A.liveObjects = [A.StackInterval (A.StackOffset (-8192)) (A.StackOffset (-4))]}) retained)
    assert "owned entry bound is not below incoming SP" (entryLow proof entry < A.StackOffset 0)
  let nativeLow = A.scratchLow wrapperProof A.Submission
      composedLow = entryLow proof A.Submission
  assert "submission join does not include deeper owned demand" (composedLow < nativeLow)
  refuse "backing only the wrapper frame admits deeper owned calls" (joinStack artifact A.Submission
    (context {A.writableStack = A.StackInterval nativeLow (A.StackOffset 0)}) retained)
  refuse "live object below wrapper frame overlaps an owned callee" (joinStack artifact A.Submission
    (context {A.liveObjects = [A.StackInterval composedLow nativeLow]}) retained)
  refuse "missing retained demands admitted" (joinStack artifact A.Submission context [])
  refuse "owned demand override admitted" (joinStack artifact A.Submission context
    (A.CalleeDemand (A.FixedService A.CriticalEnter) (A.StackOffset 0):retained))
  assert "owned calls missing from joined proof" (not (null (joinedCalls proof)))
  forM_ [(wrapper {nativeBytes = B.empty},critical,storage),(wrapper,critical {nativeBytes = B.empty},storage)
        ,(wrapper,critical,storage {nativeBytes = B.empty}),(wrapper {nativeSymbols = B.empty},critical,storage)
        ,(wrapper,critical {nativeSymbols = B.empty},storage),(wrapper,critical,storage {nativeSymbols = B.empty})
        ,(critical,wrapper,storage),(wrapper,storage,critical)] $ \(w,c,s) ->
    refuse "missing or wrong component admitted" (deriveArtifact w c s)
  forM_ [Fixtures.MisalignedCall,Fixtures.BelowSpWrite,Fixtures.UninitializedParameter,Fixtures.WrongDependency,Fixtures.WrongDirectDependency,Fixtures.WrongIndirectDependency] $ \fault -> do
    let (bytes,symbols) = Fixtures.fixture fault
    refuse "wrong call alignment or service target admitted" (deriveArtifact (NativeFiles bytes symbols) critical storage)
  let (bytes,symbols) = Fixtures.fixture Fixtures.Control
  refuse "wrapper without reservation component call admitted" (deriveArtifact (NativeFiles bytes symbols) critical storage)
  let wrongSymbols = C.unlines [if "xm1_task_storage_resume" `C.isSuffixOf` line
        then C.pack "00725dac 00000000 A xm1_task_storage_resume" else line | line <- C.lines (nativeSymbols storage)]
  refuse "mismatched storage resume target admitted" (deriveArtifact wrapper critical (storage {nativeSymbols = wrongSymbols}))
  let Critical.Guest entry = Critical.witnessEntry (Critical.witnessLayout (criticalArtifact artifact))
      offset = fromIntegral (entry-0x10400)
      broken = nativeBytes critical
      wrongCritical = critical {nativeBytes = B.take offset broken <> U.putWord32 0xe320f000 <> B.drop (offset+4) broken}
  refuse "mismatched critical adapter admitted" (deriveArtifact wrapper wrongCritical storage)
  assert "critical counter stores missing" (not (null (Critical.criticalCounterStorePcs (Critical.criticalProof (criticalArtifact artifact)))))
  assert "storage guest stores missing" (not (null (Storage.guestStorePcs (storageArtifact artifact))))
  let original = B.pack [0..63]
      window = ImageWindow (A.CodeAddress 0x61c408) (A.CodeAddress 0x61c410) (B.replicate 8 99)
      adjacent = ImageWindow (A.CodeAddress 0x61c410) (A.CodeAddress 0x61c414) (B.replicate 4 88)
  merged <- accept (mergeImageWindows original [adjacent,window])
  assert "disjoint merge changes original gaps" (merged == B.take 8 original <> B.replicate 8 99 <> B.replicate 4 88 <> B.drop 20 original)
  refuse "overlapping component windows admitted" (mergeImageWindows original [window,window])
  refuse "wrong component byte extent admitted" (mergeImageWindows original [ImageWindow (A.CodeAddress 0x61c408) (A.CodeAddress 0x61c410) B.empty])
  refuse "unaligned component window admitted" (mergeImageWindows original [ImageWindow (A.CodeAddress 0x61c409) (A.CodeAddress 0x61c411) (B.replicate 8 0)])
  pure artifact

stagingTests :: JoinedArtifact -> [FilePath] -> IO ()
stagingTests artifact paths = do
  files <- mapM B.readFile paths
  case files of
    [dat,image,loaderA,loaderB] -> do
      let input = SourceFiles dat image loaderA loaderB
      source <- accept (canonicalSource input)
      staged <- accept (stageImage4 source artifact)
      criticalOriginal <- accept (Critical.deriveOriginal dat image)
      criticalStage <- accept (Critical.stageStartup criticalOriginal (criticalArtifact artifact))
      let actual = stagedSources staged
          joined = stagedBytes staged
          windows = [(0x7235d8,0x7235dc),(0x7236b0,0x7236b4),(0x725cc4,0x725da8),(0x725dd8,0x72612c)]
          restore bytes (start,end) = let offset = start-0x61c400; count = end-start
            in B.take offset bytes <> B.take count (B.drop offset image) <> B.drop (offset+count) bytes
      assert "joined stage changes decoded extent" (B.length joined == B.length image)
      assert "joined stage changes an original gap" (foldl restore joined windows == image)
      assert "loader A changes unexpectedly" (candidateLoaderA actual == loaderA)
      assert "loader B prefix and initializer edits lost" (candidateLoaderB actual /= loaderB)
      assert "record2 source does not cover complete slot" (B.length (candidateRecord2 actual) == 0x10000)
      assert "critical entry patch lost" (B.take 4 (B.drop (0x7235d8-0x61c400) joined) == B.take 4 (B.drop (0x7235d8-0x61c400) (Critical.patchedImage4 criticalStage)))
      forM_ [input {sourceDat = B.empty},input {sourceImage4 = B.empty},input {sourceLoaderA = B.empty},input {sourceLoaderB = B.empty}] $ \changed ->
        refuse "incorrect canonical dependency admitted" (canonicalSource changed)
    _ -> fail "source fixture arity differs"
