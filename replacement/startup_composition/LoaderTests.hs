module Main (main) where

import qualified A32 as A
import Control.Monad (forM_, unless)
import qualified Data.ByteString as B
import Data.List (nub)
import qualified Data.Map.Strict as M
import LoaderFixtures
import qualified Replacement.LoaderOrchestration.Admission as LA
import qualified Replacement.LoaderOrchestration.Artifact as L
import qualified Replacement.LoaderOrchestration.Interface as LI
import qualified Replacement.KernelInstall.Artifact as I
import qualified Replacement.StartupComposition.Artifact as C
import Replacement.StartupComposition.LoaderStartup
import qualified Replacement.StartupDispatch.Admission as DA
import qualified Replacement.StartupDispatch.Artifact as D
import System.Environment (getArgs)

assert :: String -> Bool -> IO ()
assert label condition = unless condition (fail label)
accepted :: Either LoaderJoinError LoaderJoinedArtifact -> IO LoaderJoinedArtifact
accepted = either (fail . show) pure
refused :: String -> LoaderJoinError -> Either LoaderJoinError LoaderJoinedArtifact -> IO ()
refused label expected result = case result of
  Left actual -> assert (label++": "++show actual) (actual==expected)
  Right _ -> fail (label++" admitted")

main :: IO ()
main = do
  arguments <- getArgs
  case arguments of
    [aPath,aSymbols,bPath,bSymbols,dispatchPath,dispatchSymbols,installPath,installSymbols] -> do
      let native path symbols = C.NativeFiles <$> B.readFile path <*> B.readFile symbols
      a <- native aPath aSymbols
      b <- native bPath bSymbols
      dispatcher <- native dispatchPath dispatchSymbols
      installer <- native installPath installSymbols
      joined <- accepted (deriveArtifact a b dispatcher installer)
      let proof = joinedProof joined
          natural side = [boundary | boundary <- loaderBoundaries proof side
            ,boundaryScenario boundary==case side of
              LoaderA -> LI.Scenario LI.Primary LI.InitiallyPublished LI.SetupReturns
              LoaderB -> LI.Scenario LI.Peer LI.InitiallyPublished LI.SetupReturns]
      assert "owned dispatcher remains S-72" (stackOffsetValue (dispatcherLow proof)==(-72))
      assert "owned installer remains P-40" (stackOffsetValue (installerLow proof)==(-40))
      assert "joined bound is S-104, not standalone S-72" (stackOffsetValue (joinedLow proof)==(-104))
      assert "natural A phases are setup/root/idle0/diagnostic" (map boundaryPhase (natural LoaderA)==[LI.Setup,LI.Root,LI.Idle0,LI.Diagnostic])
      assert "natural B maps PeerTasks to Idle1AndStandby" (map boundaryDispatcherPhase (natural LoaderB)==[DA.Idle1AndStandby,DA.Diagnostic])
      assert "A natural owned low is L-120" (minimum (map (stackOffsetValue . boundaryOwnedLow) (natural LoaderA))==(-120))
      assert "B natural low is L-88 without installer" (minimum (map (stackOffsetValue . boundaryOwnedLow) (natural LoaderB))==(-88))
      forM_ [LoaderA,LoaderB] $ \side -> do
        let boundaries = loaderBoundaries proof side
            sites = loaderSites joined side
            LI.CodeAddress before = completionBeforeDsb sites
            LI.CodeAddress after = completionAfterDsb sites
            LI.CodeAddress poll = peerPoll sites
            LI.CodeAddress postload = peerPollPostload sites
            LI.CodeAddress call = initializerCall sites
            LI.CodeAddress returned = initializerReturn sites
        forM_ LI.scenarios $ \scenario@(LI.Scenario role completion outcome) -> do
          let expectedPhases = case (role,completion,outcome) of
                (LI.Primary,_,LI.SetupReturns) -> [LI.Setup,LI.Root,LI.Idle0,LI.Diagnostic]
                (LI.Primary,_,LI.SetupNonreturn) -> [LI.Setup]
                (LI.Peer,LI.NeverPublished,_) -> []
                (LI.Peer,_,_) -> [LI.PeerTasks,LI.Diagnostic]
              actual = [boundary | boundary <- boundaries,boundaryScenario boundary==scenario]
          assert "joined census covers every scenario without missing or duplicate phases" (map boundaryPhase actual==expectedPhases)
          assert "every scenario carries its phase-specific composed low"
            (all (\boundary -> stackOffsetValue (boundaryOwnedLow boundary)==
              if boundaryPhase boundary==LI.Setup then -120 else -88) actual)
        assert "loader preserves sixteen-byte saved frame at every dispatcher call"
          (all (\boundary -> stackOffsetValue (boundaryStack boundary)==(-16)
            && stackOffsetValue (boundaryFramePointer boundary)==(-4)
            && M.keys (boundaryLiveWords boundary)==map LA.StackOffset [-16,-12,-8,-4]) boundaries)
        assert "source-derived DSB postinstruction site is preserved" (after==before+4)
        assert "source-derived poll postload and initializer return are next instructions" (postload==poll+4&&returned==call+4)
        assert "source-derived initializer target matches admitted retained provider" (initializerEntry sites==LI.serviceAddress side LI.PrivateInitialize)
        assert "per-side execution census is nonempty" (not (null (LA.executionPcs (L.linkedProof (loaderArtifact joined side)))))
      (movedPublication,executedStore) <- either fail pure
        (relocatedPublication LoaderA a (completionStore (loaderSites joined LoaderA)))
      case L.deriveArtifact LoaderA (C.nativeBytes movedPublication) (C.nativeSymbols movedPublication) of
        Left failure -> fail ("relocated publication lost component behavior: "++show failure)
        Right _ -> pure ()
      moved <- accepted (deriveArtifact movedPublication b dispatcher installer)
      assert "executed publication wins over skipped conditional store before DSB"
        (completionStore (loaderSites moved LoaderA)==executedStore
          &&completionStore (loaderSites moved LoaderA)/=completionStore (loaderSites joined LoaderA))
      assert "relocated publication preserves the DSB boundary"
        (completionBeforeDsb (loaderSites moved LoaderA)==completionBeforeDsb (loaderSites joined LoaderA))
      forM_ [Baseline,AlignedExtraFrame] $ \layout -> do
        synthetic <- either fail pure (dispatcherFixture layout)
        composed <- accepted (deriveArtifact a b synthetic installer)
        let expected = case layout of Baseline -> -88; AlignedExtraFrame -> -104; MisalignedEntry -> -92
        assert "synthetic stack bound follows admitted callee depth" (stackOffsetValue (joinedLow (joinedProof composed))==expected)
        assert "synthetic and actual components remain independently admitted"
          (B.length (D.linkedBytes (C.dispatcherArtifact (dispatcherInstallerArtifact composed)))==B.length (C.nativeBytes synthetic))
      misaligned <- either fail pure (dispatcherFixture MisalignedEntry)
      refused "word-aligned standalone dispatcher cannot substitute installer ABI" (CompositionRejected C.InstallAlignmentRejected) (deriveArtifact a b misaligned installer)
      refused "loader A cannot substitute loader B" (LoaderRejected LoaderB L.SymbolRejected) (deriveArtifact a a dispatcher installer)
      refused "loader B cannot substitute loader A" (LoaderRejected LoaderA L.SymbolRejected) (deriveArtifact b b dispatcher installer)
      refused "dispatcher cannot substitute loader" (LoaderRejected LoaderA L.SymbolRejected) (deriveArtifact dispatcher b dispatcher installer)
      refused "installer cannot substitute dispatcher" (CompositionRejected (C.DispatcherRejected D.SymbolRejected)) (deriveArtifact a b installer installer)
      refused "dispatcher cannot substitute installer" (CompositionRejected (C.InstallerRejected I.SymbolRejected)) (deriveArtifact a b dispatcher dispatcher)
      refused "loader image omission" (LoaderRejected LoaderA L.SymbolRejected) (deriveArtifact (a {C.nativeBytes=B.empty}) b dispatcher installer)
      refused "loader symbol omission" (LoaderRejected LoaderB L.SymbolRejected) (deriveArtifact a (b {C.nativeSymbols=B.empty}) dispatcher installer)
      refused "dispatcher image omission" (CompositionRejected (C.DispatcherRejected D.ExtentRejected)) (deriveArtifact a b (dispatcher {C.nativeBytes=B.empty}) installer)
      refused "installer symbol omission" (CompositionRejected (C.InstallerRejected I.SymbolRejected)) (deriveArtifact a b dispatcher (installer {C.nativeSymbols=B.empty}))
      let aProof = L.linkedProof (loaderArtifact joined LoaderA)
          calls = nub [LA.boundaryInstruction boundary | boundary <- LA.dispatcherBoundaries aProof,LA.boundaryPhase boundary==LI.Setup]
      setup <- case calls of [pc] -> pure pc; _ -> fail "setup call is not unique"
      let LI.CodeAddress setupPc = setup
          wrongPhase = replaceLoaderWord LoaderA a (LI.CodeAddress (setupPc-4)) 0xe3a00004
          wrongLink = replaceLoaderWord LoaderA a setup 0xe12fff14
          wrongFrame = replaceLoaderWord LoaderA a (LI.carrierStart LoaderA) 0xe92d4850
          wrongFp = replaceLoaderWord LoaderA a (case LI.carrierStart LoaderA of LI.CodeAddress pc -> LI.CodeAddress (pc+4)) 0xe28db008
          retainedCopy = replaceLoaderWord LoaderA a setup (branchWord setup (LI.serviceAddress LoaderA LI.PrivateInitialize))
      refused "phase substitution" (LoaderRejected LoaderA (L.CodeRejected LA.EffectRejected)) (deriveArtifact wrongPhase b dispatcher installer)
      refused "unlinked dispatcher transfer loses continuation" (LoaderRejected LoaderA (L.CodeRejected (LA.UnsupportedInstruction setup))) (deriveArtifact wrongLink b dispatcher installer)
      refused "saved frame substitution" (LoaderRejected LoaderA (L.CodeRejected LA.FrameRejected)) (deriveArtifact wrongFrame b dispatcher installer)
      refused "frame-pointer substitution" (LoaderRejected LoaderA (L.CodeRejected LA.FrameRejected)) (deriveArtifact wrongFp b dispatcher installer)
      refused "retained service cannot substitute dispatcher" (LoaderRejected LoaderA (L.CodeRejected LA.EffectRejected)) (deriveArtifact retainedCopy b dispatcher installer)
      let publications = [site | (site,LA.Retained LI.PublishCallback) <- LA.callBoundaries aProof]
      publication <- case publications of [site] -> pure site; _ -> fail "callback publication is not unique"
      wrongCallback <- either fail pure (scalarCallback LoaderA a publication)
      refused "scalar callback is refused while publication call remains present" (LoaderRejected LoaderA (L.CodeRejected LA.EffectRejected)) (deriveArtifact wrongCallback b dispatcher installer)
      case canonicalSource (C.SourceFiles B.empty B.empty B.empty B.empty) of
        Left failure -> assert "canonical source cannot be omitted" (failure==CompositionRejected (C.SourceRejected D.SourceMismatch))
        Right _ -> fail "empty canonical source admitted"
      assert "typed poll destination is a general register" (case peerPollDestination (loaderSites joined LoaderA) of A.Reg register -> register>=0&&register<15)
      putStrLn "loader startup host admission controls complete"
    _ -> fail "usage: loader-startup-composition-tests A_BIN A_SYMBOLS B_BIN B_SYMBOLS DISPATCH_BIN DISPATCH_SYMBOLS INSTALL_BIN INSTALL_SYMBOLS"
