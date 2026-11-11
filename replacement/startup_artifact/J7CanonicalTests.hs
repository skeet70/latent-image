{-# LANGUAGE OverloadedStrings #-}
-- | Canonical-input tests of the J7 stack bounds and caller and writer checks over
-- the J6 composition and the canonical images.
module Main where

import Control.Monad (forM_, unless)
import Data.Bits ((.&.), (.|.))
import Data.IORef (IORef, modifyIORef', newIORef, readIORef)
import Data.List (nub, sort, sortOn)
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
import qualified Replacement.StartupArtifact.Footprint.Producers as Producers
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
    encoder : rebuild : decoder : rest | length rest == 30 ->
      pure (encoder, rebuild, decoder, take 28 rest, rest !! 28, rest !! 29)
    _ -> H.failH "usage: startup-artifact-j7-canonical-tests ENCODER REBUILD DECODER (J6 IMAGE SYMBOLS){14} CANONICAL_DAT SCRATCH_DIR"
  resolved <- mapM canonicalizePath imagePaths
  [firstBin, firstSymbols, pairBin, pairSymbols, publisherBin, publisherSymbols, orchestrationABin, orchestrationASymbols
    , orchestrationBBin, orchestrationBSymbols, dispatcherBin, dispatcherSymbols, installerBin, installerSymbols
    , submissionBin, submissionSymbols, storageBin, storageSymbols, idleBin, idleSymbols, rootBin, rootSymbols
    , _appBin, _appSymbols, image5Bin, image5Symbols, plainBin, plainSymbols] <- pure resolved
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
  composedIndirect <- maybe (H.failH "no indirect targets on the composition") pure (indirectOf composed)
  cleanupResults <- report "cleanup" (concat [M.findWithDefault [] (SS.SlotOffset slot) (SS.rootSlots composedIndirect) | slot <- [0x84, 0x164, 0x4, 0x14]])
  demandUsed <- maybe (H.failH "no demand conditions") pure (stockDemandConditions composed)
  let conditionsOf results = Set.unions [SS.reportConditions walked | (_, Right walked) <- results]
      image4 = targetOf targets Image4
      outsideExtents producer results =
        let extents = Producers.closureExtents producer image4
         in sort (nub [ entered | (_, Right walked) <- results, entered <- SS.reportEntered walked, entered >= 0x61c400, entered < 0x89ec00
                      , not (any (\(low, high) -> low <= entered && entered < high) extents) ])
  putStrLn ("conditions: demand " ++ show (Set.toList demandUsed) ++ "; root " ++ show (Set.toList (conditionsOf rootResults))
    ++ "; image5 " ++ show (Set.toList (conditionsOf image5Results)) ++ "; plain " ++ show (Set.toList (conditionsOf plainResults))
    ++ "; cleanup " ++ show (Set.toList (conditionsOf cleanupResults)))
  putStrLn ("image4 entries outside the image5 footprint closure: " ++ show (map hex (outsideExtents Producers.Image5Load image5Results)))
  putStrLn ("image4 entries outside the plain footprint closure: " ++ show (map hex (outsideExtents Producers.PlainLoad plainResults)))

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
  ok "both kernels' context lock methods resolve"
    (map SS.lockMethods (SS.contextLocks indirect) == [SS.LockMethods [0x439d8] [0x43a14], SS.LockMethods [0xd398c] [0xd39c8]])

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
  forM_ ([SB.traceWord kernel | kernel <- SB.traceCallbackKernels] ++ [SB.errorCallbackWord, SB.fillFlag]) $ \word ->
    putStrLn ("stores reaching " ++ hex word ++ ": " ++ show (map hex (SB.storesReaching segments [] word)))
  putStrLn ("descriptor data ids: " ++ show [hex site | (site, SB.DataId) <- SB.descriptorIds segments []])
  forM_ SB.traceCallbackKernels $ \kernel -> do
    let word = SB.traceWord kernel; setter = SB.traceSetter kernel
    ok ("the only store to the trace callback word " ++ hex word ++ " is in its setter " ++ hex setter)
      (SB.storesReaching segments [] word == [setter + 0x10])
    ok ("no code forms the address of the trace setter " ++ hex setter) (null (SB.addressFormations segments [] (== setter)))
  ok "the loader-A setter is called only at 0x647390 and 0x6473cc, each with r0 = 0x6470fc; the loader-B setter is never called"
    (sort (SB.branchSites segments [] (== 0x41548)) == [(0x647390, 0x41548), (0x6473cc, 0x41548)]
      && null (SB.branchSites segments [] (== 0xd1548))
      && all (\site -> any (\(formed, value) -> formed < site && formed + 12 >= site && value == 0x6470fc)
                         (SB.addressFormations segments [] (== 0x6470fc))) [0x647390, 0x6473cc])

  -- Canonical wrapper callers: the 67 sites, two of them data-driven.
  ok "the canonical images call the wrapper at the 67 pinned sites"
    (sort (map fst (SB.branchSites segments [] (== SB.wrapperEntry))) == sort wrapperCallSites)
  let ids = SB.descriptorIds segments []
  putStrLn ("immediate descriptor ids: " ++ show (sort [value | (_, SB.ImmediateId value) <- ids]))
  ok "63 immediate ids in 1..120 (59 outside the dispatcher) and unfixed ids at 0x6b0010, 0x6b07f0, 0xe9231c and 0x11e00ec"
    (length [() | (_, SB.ImmediateId value) <- ids, value >= 1, value <= 120] == 63
      && sort [site | (site, SB.DataId) <- ids] == [0x6b0010, 0x6b07f0, 0xe9231c, 0x11e00ec])

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
  let patchedWords changes = foldl (\acc (address, value) -> either (const acc) id
        (maybe (Left CompositionUnchanged) (\change -> applyTargetEdits [change] acc) (edit (at Image4 address (address + 4)) (U.putWord32 value))))
        (compositionTargets j6) changes
      failsWith name changes = not (holds (JoinEvidence targets (patchedWords changes) parts (compositionPieces j6)) name)
      nop = 0xe1a00000
      reserve1024 = 0xe24ddb01
  ok "wrapper-caller-ids fails when the descriptor set-up before 0x69c3e0 is removed"
    (failsWith "wrapper-caller-ids" [(0x69c3e0 - 4 * back, nop) | back <- [1 .. 3]])
  ok "root-cleanup-skips-release fails with a call of release on the r0 = 0 path"
    (failsWith "root-cleanup-skips-release" [(0x7261a0, branchLink 0x7261a0 SB.releaseEntry)])
  ok "root-retained-depth fails when 0x6a9354 reserves 1024 bytes at entry"
    (failsWith "root-retained-depth" [(0x6a9354, reserve1024)])
  ok "producer-retained-depth fails when 0x66609c reserves 1024 bytes at entry"
    (failsWith "producer-retained-depth" [(0x66609c, reserve1024)])

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
  ok "each bound's premises are exactly the ones its text states"
    (demandUsed == Set.fromList SB.demandConditions && conditionsOf rootResults == Set.fromList SB.rootDepthConditions
      && conditionsOf image5Results == Set.fromList SB.image5DepthConditions && conditionsOf plainResults == Set.fromList SB.plainDepthConditions
      && Set.insert SS.SlotWordsFixed (conditionsOf cleanupResults) == Set.fromList SB.cleanupConditions)
  -- The footprint closures follow direct calls. StockStack also enters the trace callback 0x6470fc and
  -- the error callbacks 0x723330 and 0x72835c through resolved register branches, and 0x69f8fc, which
  -- the trace callback calls.
  ok "the image4 functions the producers' bounds enter outside their footprint closures are the register-branch targets and 0x69f8fc"
    (all (\(producer, results) -> outsideExtents producer results == [0x6470fc, 0x69f8fc, 0x723330, 0x72835c])
      [(Producers.Image5Load, image5Results), (Producers.PlainLoad, plainResults)])
  ok "J6 composes with every registered join check" (either (const False) (const True) (compose registeredJoins source j6Pieces))

  passed <- readIORef counter
  putStrLn ("startup-artifact-j7-canonical-tests: " ++ show passed ++ " checks passed")
