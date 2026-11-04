{-# LANGUAGE OverloadedStrings #-}
-- | Canonical-input tests of the J7 stack bounds and caller and writer checks over
-- the J6 composition and the canonical images.
module Main where

import Control.Monad (forM_, unless)
import Data.Bits ((.&.), (.|.))
import Data.IORef (IORef, modifyIORef', newIORef, readIORef)
import Data.List (sort, sortOn)
import qualified Data.Map.Strict as M
import qualified Data.Set as Set
import Data.Word (Word32)
import qualified Emulation.Image4Staging as Tools
import qualified FujiAF.Host as H
import qualified FujiAF.UpdateContainer as U
import Numeric (showHex)
import qualified Replacement.Image5Load.Admission as I5A
import qualified Replacement.PlainLoad.Admission as PLA
import Replacement.StartupArtifact.Adapters
import Replacement.StartupArtifact.Compose
import Replacement.StartupArtifact.Joins
import qualified Replacement.StartupArtifact.StackBounds as SB
import Replacement.StartupArtifact.Stage (loadCanonical)
import Replacement.StartupArtifact.Targets
import Replacement.StartupArtifact.Types
import qualified Replacement.TaskSubmission.Admission as WA
import qualified StockStack as SS
import System.Directory (canonicalizePath)
import System.Environment (getArgs)

check :: IORef Int -> String -> Bool -> IO ()
check counter name ok = do
  unless ok (H.failH ("failed: " ++ name))
  modifyIORef' counter (+ 1)

at :: Target -> Word32 -> Word32 -> Span
at target start end = Span target (Guest start) (Guest end)

-- | An A32 BL from a site to a target.
branchLink :: Word32 -> Word32 -> Word32
branchLink site target = 0xeb000000 .|. (((target - site - 8) `div` 4) .&. 0xffffff)

hex :: Word32 -> String
hex value = "0x" ++ showHex value ""

depthOf :: Either SS.StackRefusal SS.StackReport -> Maybe Word32
depthOf result = case result of
  Right report -> let SS.StackDepth bytes = SS.reportDepth report in Just bytes
  Left _ -> Nothing

main :: IO ()
main = do
  arguments <- getArgs
  (encoder, rebuild, decoder, imagePaths, datPath, scratchPath) <- case arguments of
    encoder : rebuild : decoder : rest | length rest == 28 ->
      pure (encoder, rebuild, decoder, take 26 rest, rest !! 26, rest !! 27)
    _ -> H.failH "usage: startup-artifact-j7-canonical-tests ENCODER REBUILD DECODER (J6 IMAGE SYMBOLS){13} CANONICAL_DAT SCRATCH_DIR"
  resolved <- mapM canonicalizePath imagePaths
  [firstBin, firstSymbols, pairBin, pairSymbols, publisherBin, publisherSymbols, orchestrationABin, orchestrationASymbols
    , orchestrationBBin, orchestrationBSymbols, dispatcherBin, dispatcherSymbols, installerBin, installerSymbols
    , submissionBin, submissionSymbols, storageBin, storageSymbols, idleBin, idleSymbols, rootBin, rootSymbols
    , image5Bin, image5Symbols, plainBin, plainSymbols] <- pure resolved
  tools <- Tools.Tools <$> canonicalizePath encoder <*> canonicalizePath rebuild <*> canonicalizePath decoder
  counter <- newIORef 0
  let ok = check counter
  scratch <- Tools.validateScratchDirectory scratchPath
  source <- loadCanonical tools scratch datPath
  let targets = canonicalTargets source
      parts = U.parsedParts (canonicalParsed source)
      linked bin symbols = LinkedImage <$> (ImageBytes <$> H.readRegular 0x10000 bin) <*> (SymbolBytes <$> H.readRegular 0x10000 symbols)
      derived = either (H.failH . show) pure
      single kind bin symbols = linked bin symbols >>= derived . derivePieceOf source . SingleImage kind
  first <- single FirstStage firstBin firstSymbols
  pair <- single Record2Link pairBin pairSymbols
  publisher <- single PrimaryPublisher publisherBin publisherSymbols
  startupImages <- LoaderStartupImages <$> linked orchestrationABin orchestrationASymbols <*> linked orchestrationBBin orchestrationBSymbols
    <*> linked dispatcherBin dispatcherSymbols <*> linked installerBin installerSymbols
  startup <- derived (derivePieceOf source startupImages)
  submission <- single TaskSubmission submissionBin submissionSymbols
  storage <- single TaskStorage storageBin storageSymbols
  idle <- single IdleTask idleBin idleSymbols
  root <- single RootTask rootBin rootSymbols
  image5 <- single Image5Producer image5Bin image5Symbols
  plain <- single PlainProducer plainBin plainSymbols
  let j6Pieces = [first, pair, publisher, startup, submission, storage, idle, root, image5, plain]
  j6 <- either (H.failH . show) pure (compose [join {joinChecks = []} | join <- registeredJoins] source j6Pieces)
  let composed = JoinEvidence targets (compositionTargets j6) parts (compositionPieces j6)
  supplied <- maybe (H.failH "no supplied depths") pure (suppliedOf composed)
  putStrLn ("supplied: " ++ show [(hex entry, bytes) | (entry, SS.StackDepth bytes) <- M.toList supplied])
  targetsOfRoot <- maybe (H.failH "no root targets") pure (rootTargets composed)
  let report name entries = do
        results <- maybe (H.failH ("no depths for " ++ name)) pure (depthsIn composed supplied entries)
        forM_ results $ \(entry, result) -> putStrLn (name ++ " " ++ hex entry ++ ": " ++ either show (show . SS.reportDepth) result)
        pure results
  rootResults <- report "root" targetsOfRoot
  image5Results <- report "image5" I5A.retainedTargets
  plainResults <- report "plain" PLA.retainedTargets

  -- Canonical: stock bytes everywhere, the publisher's facts give table A.
  let stock = JoinEvidence targets targets parts [publisher {pieceCarriers = []}]
      code = stockCodeOfEvidence stock
  indirect <- maybe (H.failH "no indirect targets on canonical images") pure (indirectOf stock)

  -- irq-entry-frame.
  forM_ [(0x40000, "loader A"), (0xd0000, "loader B")] $ \(base, name) -> do
    ok (name ++ ": the IRQ entry stores 32 bytes on the interrupted stack") (SB.irqEntryFrame code base == Right 32)
    ok (name ++ ": an IRQ that preempts a task peaks 88 bytes below the interrupted SP")
      (SB.irqPreemptionPeak code indirect base == Right SB.irqPreemptionBytes)

  -- The switch model's addresses hold the instructions it names.
  let SS.SwitchModel ends resumes exceptions = SB.switchModel
      wordAt address = SS.codeWord code address
      setsSp word = word .&. 0x0c10f000 == 0x0410d000 || word .&. 0x0fe0f000 == 0x03a0d000
      storesSp word = word .&. 0x0c10f000 == 0x0400d000
  ok "every switch end loads SP from memory or moves an immediate into it" (all (maybe False setsSp . wordAt) (Set.toList ends))
  ok "every resume edge is keyed by a store of SP" (all (maybe False storesSp . wordAt) (M.keys resumes))
  ok "every resume target and exception path is stock code" (all (\address -> wordAt address /= Nothing) (M.elems resumes ++ Set.toList exceptions))

  -- Context lock methods recovered from the kernel initializers.
  ok "both kernels' context lock methods resolve" (map snd (SS.contextLocks indirect) == [([0x439d8], [0x43a14]), ([0xd398c], [0xd39c8])])

  -- error-callback-depth and wrapper-callee-demands on canonical bytes.
  let canonicalDepth = maybe Nothing (lookup 0x72835c) (depthsIn stock M.empty [0x72835c])
  putStrLn ("canonical D(0x72835c): " ++ show (canonicalDepth >>= depthOf))
  ok "error callback 0x72835c needs 272 bytes" ((canonicalDepth >>= depthOf) == Just 272)
  demand <- maybe (H.failH "no canonical stock demands") pure (stockDemand stock)
  let fixedTargets = map WA.FixedService [WA.Fill, WA.Cache, WA.Log, WA.ErrorCallback, WA.TryLock, WA.Unlock]
      slotTargets = map snd WA.rootServiceOffsets
      derivedOf target = let WA.CalleeDemand _ (WA.StackOffset offset) = demand target in negate offset
      tableOf target = let WA.CalleeDemand _ (WA.StackOffset offset) = WA.stockCalleeDemand target in negate offset
  forM_ (fixedTargets ++ slotTargets) $ \target ->
    putStrLn ("demand " ++ show target ++ ": derived " ++ show (derivedOf target) ++ ", table " ++ show (tableOf target))
  ok "derived demands: fill 12, cache 36, log 160, error dispatch 280, locks 0, slot services 8, 4, 4, 16, 16, 84"
    (map derivedOf (fixedTargets ++ slotTargets) == [12, 36, 160, 280, 0, 0, 8, 4, 4, 16, 16, 84])
  ok "the derived demands differ from the hand table only at error dispatch (12) and activation (40)"
    ([(target, tableOf target) | target <- fixedTargets ++ slotTargets, derivedOf target /= tableOf target]
      == [(WA.FixedService WA.ErrorCallback, 12), (WA.ActivationService, 40)])

  -- trace-callback-writers on canonical bytes.
  let segments = composedSegments stock
  ok "the only store to the trace callback word is 0x41558 in its setter"
    (SB.storesReaching segments [] SB.traceCallbackWord == [0x41558])
  ok "the setter 0x41548 is called only at 0x647390 and 0x6473cc, each with r0 = 0x6470fc"
    (sort (SB.branchSites segments [] (== SB.traceCallbackSetter)) == [(0x647390, 0x41548), (0x6473cc, 0x41548)]
      && all (\site -> any (\(formed, value) -> formed < site && formed + 12 >= site && value == 0x6470fc)
                         (SB.addressFormations segments [] (== 0x6470fc))) [0x647390, 0x6473cc])
  ok "no code forms the setter's address" (null (SB.addressFormations segments [] (== SB.traceCallbackSetter)))

  -- Canonical wrapper callers: the 67 sites, two of them data-driven.
  ok "the canonical images call the wrapper at the 67 pinned sites"
    (sort (map fst (SB.branchSites segments [] (== SB.wrapperEntry))) == sort wrapperCallSites)
  let ids = SB.descriptorIds segments []
  putStrLn ("immediate descriptor ids: " ++ show (sort [value | (_, SB.ImmediateId value) <- ids]))
  ok "65 immediate ids in 1..120 (61 outside the dispatcher) and data ids at 0x6b07f0 and 0xe9231c"
    (length [() | (_, SB.ImmediateId value) <- ids, value >= 1, value <= 120] == 65
      && sort [site | (site, SB.DataId) <- ids] == [0x6b07f0, 0xe9231c])

  -- Composed J6: every J7 check holds, and each fails on a retargeted stock call.
  let named name = [predicate | join <- registeredJoins, HostCheck checkName predicate <- joinChecks join, checkName == name]
      holds evidence name = not (null (named name)) && all ($ evidence) (named name)
      j7Checks = [ "wrapper-callers-closed", "storage-prelude-sole-caller", "wrapper-caller-ids", "error-callback-writers"
                 , "root-cleanup-skips-release", "root-retained-depth", "producer-retained-depth", "fill-flag-writers"
                 , "submission-joined-stack-in-boot-window", "root-stack-request-covers-frame", "idle-stack-request-covers-frame" ]
  forM_ j7Checks $ \name -> ok (show name ++ " holds on the J6 composition") (holds composed name)
  forM_ [ ("wrapper-callers-closed", 0x725ddc), ("storage-prelude-sole-caller", 0x725c6c)
        , ("error-callback-writers", SB.errorCallbackSetter), ("fill-flag-writers", SB.fillDoneCallback) ] $ \(name, target) -> do
    let site = 0x69c3e0
        patched = either (const (compositionTargets j6)) id
          (maybe (Left CompositionUnchanged) (\change -> applyTargetEdits [change] (compositionTargets j6))
            (edit (at Image4 site (site + 4)) (U.putWord32 (branchLink site target))))
    ok (show name ++ " fails with the stock call at 0x69c3e0 retargeted to " ++ hex target)
      (not (holds (JoinEvidence targets patched parts (compositionPieces j6)) name))

  -- Refused retained entries are the pinned lists; the deepest bounded entries give the margins.
  let refused results = sortOn fst [(entry, refusal) | (entry, Left refusal) <- results]
      deepest results = maximum [(bytes, entry) | (entry, Right walked) <- results, let SS.StackDepth bytes = SS.reportDepth walked]
  ok "root's refused targets are the pinned list" (refused rootResults == sortOn fst rootUnbounded)
  ok "image5's refused services are the pinned list" (refused image5Results == sortOn fst image5Unbounded)
  ok "plain's refused services are the pinned list" (refused plainResults == sortOn fst plainUnbounded)
  ok "root's deepest bounded target is 0x6a9354 at 752 bytes: 16 + 752 + 88 = 856 of 1024" (deepest rootResults == (752, 0x6a9354))
  ok "image5's deepest bounded service is 0x66609c at 536 bytes: 48 + 536 + 88 = 672 of 3040" (deepest image5Results == (536, 0x66609c))
  ok "plain's deepest bounded service is 0x724c00 at 392 bytes: 40 + 392 + 88 = 520 of 3040" (deepest plainResults == (392, 0x724c00))
  ok "owned entries carry their proved depths: critical pair 128, wrapper 456, release 40, idle 8, root 16"
    (M.toList supplied == [(0x69cbf0, SS.StackDepth 8), (0x69d160, SS.StackDepth 16), (0x7235d8, SS.StackDepth 128), (0x7236b0, SS.StackDepth 128), (0x725dd8, SS.StackDepth 456), (0x726098, SS.StackDepth 40)])
  ok "J6 composes with every registered join check" (either (const False) (const True) (compose registeredJoins source j6Pieces))

  passed <- readIORef counter
  putStrLn ("startup-artifact-j7-canonical-tests: " ++ show passed ++ " checks passed")
