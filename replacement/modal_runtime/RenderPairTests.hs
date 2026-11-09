{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}
module Main (main) where
import Control.Monad (foldM, forM_, unless)
import qualified Data.Aeson as J
import qualified Data.Aeson.KeyMap as KM
import qualified Data.Aeson.Types as J
import Data.Bits (shiftR)
import qualified Data.ByteString as B
import qualified Data.Map.Strict as M
import qualified Data.Text as T
import Data.Word (Word32)
import qualified RenderStack as S
import Replacement.ModalRuntime.MenuEnginePair
import Replacement.ModalRuntime.MenuEngineTree
import Replacement.ModalRuntime.RenderPair
import Replacement.ModalRuntime.RenderObjectProvenance
import System.Environment (getArgs)

check :: String -> Bool -> IO ()
check name good = unless good (fail name)
passes :: Either String a -> Bool
passes (Right _) = True
passes _ = False
le :: Word32 -> B.ByteString
le n = B.pack [fromIntegral (n `shiftR` (i*8)) | i <- [0..3]]
put :: Int -> Word32 -> B.ByteString -> B.ByteString
put offset n bytes = B.take offset bytes <> le n <> B.drop (offset+4) bytes
config :: RenderConfig
config = RenderConfig [1] Nothing 0xa4ce00 DrawsRequired
config18 :: RenderConfig
config18 = RenderConfig [0..18] Nothing 0xa4ce00 DrawsRequired
node :: TableNode
node = TableNode (Address 0x776000) 0x25 0 5 1 0 0 0 Nothing 0 (Just [TableRow 0 LeafChild]) Nothing
document :: TablesDocument
document = TablesDocument "" "" [] [node]
record :: B.ByteString
record = foldr (uncurry put) (B.replicate 40 0) [(8,0x1109130),(12,8),(16,4096),(20,0xa4ce00),(32,33)]
entry :: DrawFunction -> Hit
entry function = Hit (OtherSite (if function == TreeDraw then "render-tree-entry" else "render-picker-entry")) 0 (0x25,0,0,0)
  (Just (if function == TreeDraw then 0x1167a60 else 0x1168ce4)) (Just 0xa4dc00)
  [B.replicate 28 0,put 0 0x776000 (B.replicate 0x430 0), B.replicate 24 0,B.replicate 4 0,B.replicate 152 0,le 0x4006060c,le 0x4005c3ac,record] (Just (ProvenanceSequence 0)) Nothing
hit :: T.Text -> Hit
hit label = Hit (OtherSite label) 0 (0,0,0,0) Nothing (Just 0xa4dc00) [] (Just (ProvenanceSequence 0)) Nothing
returned :: Hit
returned = (hit "render-tree-return") {hitMemory = map (`B.replicate` 0) [68,24,64,152,4096,704,4096,4096,4096,4096,4096,4096,2560]}
emptyBatch :: ProvenanceBatch
emptyBatch = either error id (J.parseEither parseProvenanceBatch (J.object
  ["version" J..= (1 :: Int),"start" J..= boundary,"end" J..= boundary
  ,"sequence_start" J..= (0 :: Int),"sequence_end" J..= (0 :: Int)
  ,"overflow" J..= False,"coverage" J..= ("continuous" :: String),"events" J..= ([] :: [J.Value])]))
 where boundary = J.object ["instructions" J..= (0 :: Int),"tick" J..= (0 :: Int)]
observation :: [Hit] -> Observation
observation hits = Observation "segment" EngineResident [("render-task-node",le 0x4005c3ac),("render-task-record",record),("render-task-stack",B.replicate 4096 0),("render-osd-front",B.singleton 0)] hits (Just emptyBatch)
readerFor :: Observation -> ObservationReader
readerFor (Observation phase residency memoryReads hits provenance) =
  ObservationReader phase residency memoryReads provenance (\step initial -> pure (foldM step initial hits))
owned :: [Hit]
owned = [entry TreeDraw,hit "render-tree-adapter",returned]
run :: [Hit] -> Either String [Draw]
run hits = concat <$> renderDraws config document document OwnedRole [observation hits]

provenanceCases :: IO ()
provenanceCases = do
  let clock n = J.object ["instructions" J..= (n :: Int),"tick" J..= n]
      event n fields = J.object (["sequence" J..= (n :: Int),"instructions" J..= (n+1),"tick" J..= (n+1),"core" J..= (0 :: Int)] ++ fields)
      allocate n base ordinal extent = event n
        ["kind" J..= ("allocate" :: String),"constructor" J..= ("image_d4" :: String)
        ,"allocation_base" J..= (base :: String),"extent" J..= (extent :: Int),"ordinal" J..= (ordinal :: Int)]
      bind n base = event n ["kind" J..= ("bind" :: String),"object_base" J..= ("0x015edf38" :: String),"allocation_base" J..= (base :: String)]
      free n base = event n ["kind" J..= ("free" :: String),"allocation_base" J..= (base :: String)]
      batch :: Int -> Int -> Int -> Int -> [J.Value] -> J.Value
      batch first last_ start end events = J.object
        ["version" J..= (1 :: Int),"start" J..= clock start,"end" J..= clock end
        ,"sequence_start" J..= (first :: Int),"sequence_end" J..= (last_ :: Int)
        ,"overflow" J..= False,"coverage" J..= ("continuous" :: String),"events" J..= events]
      withBatch value provenanceSequence pointer = do
        parsed <- J.parseEither parseProvenanceBatch value
        let memory = hitMemory returned
            result = returned {hitMemory=take 3 memory ++ [put 8 0x15edf38 (memory !! 3)] ++ take 2 (drop 4 memory) ++ [put 4 pointer (memory !! 6)] ++ drop 7 memory}
            hits = [entry TreeDraw,hit "render-tree-adapter",result]
        pure ((observation [h {hitProvenanceSequence=Just (ProvenanceSequence provenanceSequence)} | h <- hits]) {observationProvenance=Just parsed})
      draw value provenanceSequence pointer = do
        segment <- withBatch value provenanceSequence pointer
        concat <$> renderDraws config document document OwnedRole [segment]
      stockBatch = batch 0 2 0 10 [allocate 0 "0x00100000" 1 212,bind 1 "0x00100000"]
      ownedBatch = batch 0 2 0 10 [allocate 0 "0x00200000" 1 212,bind 1 "0x00200000"]
      compareRoles stockDraws ownedDraws = do a <- stockDraws; b <- ownedDraws; compareDraws config a b
      left = draw stockBatch 2 0x100000
      right = draw ownedBatch 2 0x200000
  check "constructor-proven handles compare across allocation addresses" (passes (compareRoles left right))
  forM_ [0,8,12,208] $ \offset -> do
    let mutated = do
          values <- right
          pure [value {drawMemory=take 6 (drawMemory value) ++ [put offset 1 (drawMemory value !! 6)] ++ drop 7 (drawMemory value)} | value <- values]
    check "provenance does not exempt other object fields" (not (passes (compareRoles left mutated)))
  let unknown pointer = run [entry TreeDraw,hit "render-tree-adapter",returned {hitMemory=take 3 (hitMemory returned) ++ [put 8 0x15edf38 (hitMemory returned !! 3)] ++ take 2 (drop 4 (hitMemory returned)) ++ [put 4 pointer (hitMemory returned !! 6)] ++ drop 7 (hitMemory returned)}]
  check "unknown different handles remain raw" (not (passes (compareRoles (unknown 0x100000) (unknown 0x200000))))
  check "unknown equal handles remain raw-equal" (passes (compareRoles (unknown 0x100000) (unknown 0x100000)))
  let wrongObject = batch 0 3 0 10
        [allocate 0 "0x00210000" 1 212,allocate 1 "0x00200000" 2 212,bind 2 "0x00200000"]
  check "different constructor allocation identity fails" (not (passes (compareRoles left (draw wrongObject 3 0x200000))))
  forM_ ["0x00200004","0x002000d4"] $ \address -> do
    let invalid = batch 0 2 0 10 [allocate 0 "0x00200000" 1 212,bind 1 address]
    check "interior and out-of-range handles cannot bind" (not (passes (draw invalid 2 0x200000)))
  let stale = batch 0 3 0 10 [allocate 0 "0x00200000" 1 212,bind 1 "0x00200000",free 2 "0x00200000"]
      reused = batch 0 4 0 10 [allocate 0 "0x00200000" 1 212,bind 1 "0x00200000",free 2 "0x00200000",allocate 3 "0x00200000" 2 212]
      rebound = batch 0 5 0 10 [allocate 0 "0x00200000" 1 212,bind 1 "0x00200000",free 2 "0x00200000",allocate 3 "0x00200000" 2 212,bind 4 "0x00200000"]
  check "freed handle is rejected even when raw pointer is unchanged" (not (passes (draw stale 3 0x200000)))
  check "same-address reuse does not revive the old object binding" (not (passes (draw reused 4 0x200000)))
  check "same-address reuse with a new binding is live" (passes (draw rebound 5 0x200000))
  check "reused allocation has a new logical identity" (not (passes (compareRoles right (draw rebound 5 0x200000))))
  let missing = batch 0 2 0 10 [allocate 0 "0x00200000" 1 212]
      badExtent = batch 0 2 0 10 [allocate 0 "0x00200000" 1 208,bind 1 "0x00200000"]
      wrongSequence = batch 0 2 0 10 [allocate 1 "0x00200000" 1 212,bind 2 "0x00200000"]
  check "missing provenance events fail" (not (passes (draw missing 2 0x200000)))
  check "constructor extent is source-bound" (not (passes (draw badExtent 2 0x200000)))
  check "event sequence gaps fail" (not (passes (draw wrongSequence 2 0x200000)))
  check "draw cannot exceed the captured sequence" (not (passes (draw ownedBatch 3 0x200000)))
  let uncovered hits = (observation hits) {observationProvenance=Nothing}
      roleDraws = renderDraws config document document OwnedRole
  check "all missing provenance refuses even raw-equal draws" (not (passes (roleDraws [uncovered owned])))
  check "leading missing provenance refuses later covered draws" (not (passes (roleDraws [uncovered [],observation owned])))
  check "missing empty interval refuses otherwise covered draws" (not (passes (roleDraws [observation [],uncovered [],observation owned])))
  check "covered empty intervals are retained" (passes (roleDraws [observation [],observation owned]))
  check "covered draw requires every hit sequence" (not (passes (roleDraws [observation [(entry TreeDraw) {hitProvenanceSequence=Nothing},hit "render-tree-adapter",returned]])))
  let across second = do
        first <- J.parseEither parseProvenanceBatch ownedBatch
        final <- withBatch second 2 0x200000
        renderDraws config document document OwnedRole [(observation []) {observationProvenance=Just first},final]
  check "live bindings survive continuously covered inter-segment gaps" (passes (across (batch 2 2 10 20 [])))
  check "unobserved inter-segment gap refuses logical equality" (not (passes (across (batch 2 2 11 20 []))))
  let between rebind = do
        first <- J.parseEither parseProvenanceBatch ownedBatch
        let events = [eventAt 11 (free 2 "0x00200000"),eventAt 12 (allocate 3 "0x00200000" 2 212)]
              ++ [eventAt 13 (bind 4 "0x00200000") | rebind]
            boundary = if rebind then 5 else 4
        final <- withBatch (batch 2 boundary 10 20 events) (fromIntegral boundary) 0x200000
        renderDraws config document document OwnedRole [(observation []) {observationProvenance=Just first},final]
      eventAt n (J.Object fields) = J.Object (KM.insert "instructions" (J.toJSON (n :: Int)) (KM.insert "tick" (J.toJSON n) fields))
      eventAt _ _ = error "test provenance event must be an object"
  check "inter-segment free and same-address reuse invalidate the binding" (not (passes (between False)))
  check "inter-segment reuse with rebinding restores a live identity" (passes (between True))
  forM_ [("overflow",J.Bool True),("coverage",J.String "refused"),("version",J.toJSON (2 :: Int))] $ \(key,value) ->
    case ownedBatch of
      J.Object fields -> check "refused provenance cannot establish equality" (not (passes (draw (J.Object (KM.insert key value fields)) 2 0x200000)))
      _ -> fail "invalid test batch"
  let oversized = batch 0 65537 0 70000 [free n "0x00300000" | n <- [0..65536]]
  check "oversized provenance batch refuses comparison" (not (passes (draw oversized 65537 0x200000)))
  let forwarded = do
        first <- J.parseEither parseProvenanceBatch ownedBatch
        second <- J.parseEither parseProvenanceBatch (batch 2 2 10 20 [])
        merged <- combineBatches [first,second]
        case merged of
          Just value -> draw (J.toJSON value) 2 0x200000
          Nothing -> Left "missing merged provenance"
  check "forwarded batches retain activation events" (passes forwarded)
  let secondaryEvent = case allocate 0 "0x00200000" 1 212 of
        J.Object fields -> J.Object (KM.insert "core" (J.toJSON (1 :: Int)) fields)
        _ -> error "test allocation must be an object"
      roundtrip = do
        parsed <- J.parseEither parseProvenanceBatch (batch 0 2 0 10 [secondaryEvent,bind 1 "0x00200000"])
        encoded <- J.parseEither parseProvenanceBatch (J.toJSON parsed)
        pure (parsed == encoded)
  check "provenance forwarding preserves core and allocation facts" (roundtrip == Right True)

main :: IO ()
main = do
  args <- getArgs
  fixturePath <- case args of [path] -> pure path; _ -> fail "expected declaration receipt"
  declaration <- B.readFile fixturePath >>= either fail pure . J.eitherDecodeStrict'
  check "declared reads match bank set" (passes (J.parseEither (validateRenderDeclarations config OwnedRole) declaration))
  check "different bank declaration fails" (not (passes (J.parseEither (validateRenderDeclarations (config {renderBanks=[3]}) OwnedRole) declaration)))
  check "missing declarations fail" (not (passes (J.parseEither (validateRenderDeclarations config OwnedRole) (J.object []))))
  forM_ [TreeDraw,PickerDraw] $ \function -> do
    check "live stock class owned" (deriveDrawClass document document function (entry function) == Right (OwnedDraw function))
    let e = entry function
        saved = e {hitMemory = put 20 7 (B.replicate 28 0):drop 1 (hitMemory e)}
    check "saved report class refused" (deriveDrawClass document document function saved == Right (StockDraw function))
    check "other caller refused" (deriveDrawClass document document function (e {hitLr = Just 0x1169308}) == Right (StockDraw function))
    check "unknown record refused" (deriveDrawClass document document function (e {hitRegisters = (0x7d,0,0,0)}) == Right (StockDraw function))
  let fn = (entry PickerDraw) {hitLr = Just 0x116c588}
  check "Fn picker live stock class owned" (deriveDrawClass document document PickerDraw fn == Right (OwnedDraw PickerDraw))
  forM_ [2,3,7,9,13] $ \mode -> do
    let excluded = fn {hitMemory = put 20 mode (B.replicate 28 0):drop 1 (hitMemory fn)}
    check "Fn picker saved and skipped modes refused" (deriveDrawClass document document PickerDraw excluded == Right (StockDraw PickerDraw))
  let package = document {documentNodes = [node {nodeAddress = Address 0x123000}]}
  check "package pointer collision refused" (deriveDrawClass document package TreeDraw ((entry TreeDraw) {hitMemory = B.replicate 28 0:put 0 0x123000 (B.replicate 0x430 0):drop 2 (hitMemory (entry TreeDraw))}) == Right (StockDraw TreeDraw))
  let paneNode = node {nodeId=1}
      paneDocument = document {documentNodes=[paneNode]}
      pane = (entry TreeDraw) {hitSite=OtherSite "render-pane-entry",hitRegisters=(1,0,0,0),hitLr=Just 0x1158184}
      paneClass = deriveDrawClass paneDocument paneDocument PaneDraw
      paneReturned = returned {hitSite=OtherSite "render-pane-return",hitMemory=take 1 (hitMemory returned) ++ [B.replicate 40 0] ++ drop 2 (hitMemory returned)}
      paneRun hits = renderDraws config paneDocument paneDocument OwnedRole [observation hits]
  check "pane live root class owned" (paneClass pane == Right (OwnedDraw PaneDraw))
  forM_ [0,1,3,4,6,8,9,10,12,14,255] $ \mode ->
    check "pane covered live modes owned" (paneClass (pane {hitRegisters=(1,mode,0,0)}) == Right (OwnedDraw PaneDraw))
  forM_ [2,5,7,11,13,15,256,0xffffffff] $ \mode ->
    check "pane saved and uncovered modes refused" (paneClass (pane {hitRegisters=(1,mode,0,0)}) == Right (StockDraw PaneDraw))
  forM_ [0,1,2,3,9] $ \kind -> do
    let typed = paneDocument {documentNodes=[paneNode {nodeType=kind}]}
    check "pane covered snapshot types owned" (deriveDrawClass typed typed PaneDraw pane == Right (OwnedDraw PaneDraw))
  let uncovered = paneDocument {documentNodes=[paneNode {nodeType=4}]}
  check "pane uncovered snapshot type refused" (deriveDrawClass uncovered uncovered PaneDraw pane == Right (StockDraw PaneDraw))
  check "pane other caller refused" (paneClass (pane {hitLr=Just 0x11586e8}) == Right (StockDraw PaneDraw))
  check "pane non-root refused" (paneClass (pane {hitRegisters=(0x25,0,0,0)}) == Right (StockDraw PaneDraw))
  check "pane owned adapter completes at root wrapper return" (passes (paneRun [pane,hit "render-pane-adapter",paneReturned]))
  check "pane resident owned class cannot enter stock body" (not (passes (paneRun [pane,hit "render-pane-stock",paneReturned])))
  check "pane return requires adapter" (not (passes (paneRun [pane,paneReturned])))
  let leaves = map hit ["render-pane-grid","render-pane-label-grid","render-pane-aligned","render-pane-cell-box","render-pane-bounds"]
  check "pane leaf arguments are not draw boundaries" (passes (paneRun ([pane,hit "render-pane-adapter"] ++ leaves ++ [paneReturned])))
  check "pane leaf arguments before an entry are ignored" (passes (paneRun (leaves ++ [pane,hit "render-pane-adapter",paneReturned])))
  check "pane draws provide coverage" (passes (checkRenderCoverage config [observation [pane]] [observation [pane]]))
  check "owned adapter draw accepted" (passes (run owned))
  check "stock body residency fails" (not (passes (run [entry TreeDraw,hit "render-tree-stock",returned])))
  check "missing adapter fails" (not (passes (run [entry TreeDraw,returned])))
  check "unloaded stock body accepted" (passes (renderDraws config document document OwnedRole [((observation [entry TreeDraw,hit "render-tree-stock",returned]) {observationResidency = EngineUnloaded})]))
  check "unterminated draw fails" (not (passes (run [entry TreeDraw,hit "render-tree-adapter"])))
  let altered = returned {hitMemory = take 3 (hitMemory returned) ++ [put 0 1 (hitMemory returned !! 3)] ++ drop 4 (hitMemory returned)}
  check "unread bank head change fails" (not (passes (run [entry TreeDraw,hit "render-tree-adapter",altered])))
  draws <- either fail pure (run owned)
  check "equal per draw state passes" (passes (compareDraws config draws draws))
  forM_ [0..5] $ \index -> do
    let change bytes = B.cons 1 (B.drop 1 bytes)
        memory = hitMemory returned
        mutation = Draw (OwnedDraw TreeDraw) (take index memory ++ [change (memory !! index)] ++ drop (index+1) memory) M.empty
    check ("prefix state byte difference fails " ++ show index) (not (passes (compareDraws config draws [mutation])))
  let bank15Base = 0x15e7538 + 15*27136
      heads18 = put 120 bank15Base (put 124 bank15Base (hitMemory returned !! 3))
      bank15 liveKey freeKey liveState freeState liveGeometry =
        foldr (\(offset,value) bytes -> put offset value bytes) (B.replicate 4096 0)
          [(0,0),(16,liveState),(52,liveGeometry),(110,liveKey),(212,0),(228,freeState),(322,freeKey)]
      memory18 liveKey freeKey liveState freeState liveGeometry =
        let bankChunks = concat
              [ if bank == 15
                  then bank15 liveKey freeKey liveState freeState liveGeometry : replicate 5 (B.replicate 4096 0) ++ [B.replicate 2560 0]
                  else replicate 6 (B.replicate 4096 0) ++ [B.replicate 2560 0]
              | bank <- [0 :: Int .. 18] ]
        in take 3 (hitMemory returned) ++ [heads18] ++ take 2 (drop 4 (hitMemory returned)) ++ bankChunks
      draw18 liveKey freeKey liveState freeState liveGeometry =
        Draw (OwnedDraw TreeDraw) (memory18 liveKey freeKey liveState freeState liveGeometry) M.empty
      live18 = draw18 0x30 0x30 0 2 0
      reader18 liveKey freeKey liveState freeState liveGeometry =
        [entry TreeDraw,hit "render-tree-adapter",returned {hitMemory=memory18 liveKey freeKey liveState freeState liveGeometry}]
      compareReader18 draw = compareDrawsReader config18 document document [draw]
  check "live bank15 key difference fails" (not (passes (compareDraws config18 [live18] [draw18 0x31 0x30 0 2 0])))
  check "live bank15 geometry difference fails" (not (passes (compareDraws config18 [live18] [draw18 0x30 0x30 0 2 1])))
  check "live bank15 allocation state difference fails" (not (passes (compareDraws config18 [live18] [draw18 0x30 0x30 1 2 0])))
  check "off-list allocation state change fails" (not (passes (compareDraws config18 [live18] [draw18 0x30 0x30 0 0 0])))
  check "off-list stale key difference passes" (passes (compareDraws config18 [live18] [draw18 0x30 0x31 0 2 0]))
  readerKey <- compareReader18 live18 [readerFor (observation (reader18 0x31 0x30 0 2 0))]
  check "reader live bank15 key difference fails" (not (passes readerKey))
  readerGeometry <- compareReader18 live18 [readerFor (observation (reader18 0x30 0x30 0 2 1))]
  check "reader live bank15 geometry difference fails" (not (passes readerGeometry))
  readerState <- compareReader18 live18 [readerFor (observation (reader18 0x30 0x30 1 2 0))]
  check "reader live bank15 allocation state difference fails" (not (passes readerState))
  readerFreeState <- compareReader18 live18 [readerFor (observation (reader18 0x30 0x30 0 0 0))]
  check "reader off-list allocation state change fails" (not (passes readerFreeState))
  readerFreeKey <- compareReader18 live18 [readerFor (observation (reader18 0x30 0x31 0 2 0))]
  check "reader off-list stale key difference passes" (passes readerFreeKey)
  check "unpaired draws fail" (not (passes (compareDraws config draws [])))
  let resource = 0x12345678
      clockConfig = config {renderClock = Just (ResourceId resource)}
      clockPrefix = take 3 (hitMemory returned) ++ [put 8 0x15edf38 (hitMemory returned !! 3)] ++ take 2 (drop 4 (hitMemory returned))
      clockBank = put 8 resource (B.replicate 27136 0)
      clockA = Draw (OwnedDraw TreeDraw) (clockPrefix ++ [clockBank]) M.empty
      clockB = Draw (OwnedDraw TreeDraw) (clockPrefix ++ [put 12 1 clockBank]) M.empty
  check "clock resource exempts object" (passes (compareDraws clockConfig [clockA] [clockB]))
  check "clock unset refuses object difference" (not (passes (compareDraws config [clockA] [clockB])))
  check "clock-only duplicate draw accepted" (passes (compareDraws clockConfig [clockA,clockB] [clockB]))
  let bounds = S.RenderStackReport (S.RenderEntryReport 128 3416 1024) (S.RenderEntryReport 128 3720 1024) (S.RenderEntryReport 128 3984 1024)
      baseline = (observation []) {observationPhase = "render-baseline"}
      end = observation owned
  check "task stack admission passes" (passes (checkRenderStackObservations config bounds document document [baseline,end]))
  check "missing stack baseline fails" (not (passes (checkRenderStackObservations config bounds document document [end])))
  let high = end {observationReads = [(name,if name == "render-task-stack" then put 0xef0 1 bytes else bytes) | (name,bytes) <- observationReads end]}
  check "changed word above floor is reported" (checkRenderStackObservations config bounds document document [baseline,high] == Right (StackReport (Just 0xa4d780) (Just 0xa4dcf0)))
  let belowEstimate = end {observationReads = [(name,if name == "render-task-stack" then put 0x700 1 bytes else bytes) | (name,bytes) <- observationReads end]}
  check "whole-route changed-word mark below borrowed estimate remains a lower bound"
    (checkRenderStackObservations config bounds document document [baseline,belowEstimate] == Right (StackReport (Just 0xa4d780) (Just 0xa4d500)))
  check "low entry SP fails" (not (passes (checkRenderStackObservations config bounds document document [baseline,observation [(entry TreeDraw) {hitSp = Just 0xa4ce10}]])))
  let changed = end {observationReads = [(name,if name == "render-task-stack" then B.cons 1 (B.replicate 4095 0) else bytes) | (name,bytes) <- observationReads end]}
  check "snapshot change inside existing bottom guard fails" (not (passes (checkRenderStackObservations config bounds document document [baseline,changed])))
  let moved = end {observationReads = [(name,if name == "render-task-node" then le 0x40050000 else bytes) | (name,bytes) <- observationReads end]}
  check "moved descriptor fails" (not (passes (checkRenderStackObservations config bounds document document [baseline,moved])))
  check "render coverage refuses two empty roles" (not (passes (checkRenderCoverage config [observation []] [observation []])))
  check "render coverage requires an entry in each role" (not (passes (checkRenderCoverage config [observation [entry TreeDraw]] [observation []])))
  check "render coverage accepts entries in later armed segments" (passes (checkRenderCoverage config [observation [],observation [entry TreeDraw]] [observation [entry PickerDraw]]))
  let drawFree = config {renderCoverage = DeclaredDrawFree}
  check "explicit draw-free pair accepts empty roles" (passes (checkRenderCoverage drawFree [observation []] [observation []]))
  check "explicit draw-free pair refuses an observed draw" (not (passes (checkRenderCoverage drawFree [observation [entry TreeDraw]] [observation []])))
  let handoff buffer = (hit "render-osd-buffer") {hitRegisters = (0,buffer,0,0),hitLr = Just 0x6d7b3c}
      increment n = (hit "render-osd-increment") {hitRegisters = (0,0,n,0)}
      osd = observation [handoff 0x4228c800,increment 1,handoff 0x423de000,increment 2]
  check "OSD two buffers and wrap pass" (passes (osdInputs [osd]))
  check "OSD inputs carry both buffers and increments" (osdInputs [osd] == Right [("segment",J.object ["front_byte" J..= (0 :: Int),"buffers" J..= ([0x423de000,0x4228c800] :: [Word32]),"increments" J..= (2 :: Int)])])
  check "OSD zero-increment window starts at its own end byte" (passes (osdInputs [osd,(observation []) {observationReads = [("render-osd-front",B.singleton 1)]}]))
  let afterOddGap = observation [handoff 0x423de000,increment 2]
      endingOne hits = (observation hits) {observationReads = [("render-osd-front",B.singleton 1)]}
  check "OSD odd untraced gap between windows passes" (osdInputs [osd,afterOddGap] == Right
    [ ("segment",J.object ["front_byte" J..= (0 :: Int),"buffers" J..= ([0x423de000,0x4228c800] :: [Word32]),"increments" J..= (2 :: Int)])
    , ("segment",J.object ["front_byte" J..= (1 :: Int),"buffers" J..= ([0x423de000,0x4228c800] :: [Word32]),"increments" J..= (1 :: Int)]) ])
  check "OSD nonalternating increments within a window fail" (not (passes (osdInputs [endingOne [handoff 0x4228c800,increment 1,handoff 0x4228c800,increment 1]])))
  check "OSD end byte differing from last increment fails" (not (passes (osdInputs [osd {observationReads = [("render-osd-front",B.singleton 1)]}])))
  check "OSD duplicate handoff before increment fails" (not (passes (osdInputs [endingOne [handoff 0x4228c800,handoff 0x4228c800,increment 1]])))
  check "OSD increment without buffer fails" (not (passes (osdInputs [observation [increment 1]])))
  check "OSD buffer replacement fails" (not (passes (osdInputs [osd,endingOne [handoff 0x4228d000,increment 1]])))
  let beforeDraw = (observation []) {observationPhase = "before"}
      afterDraw = osd {observationPhase = "after"}
  earlyFront <- osdInputsReader (map readerFor [beforeDraw,afterDraw])
  check "earlier captures use the subsequently observed stable buffer pair" (earlyFront == Right
    [ ("before",J.object ["front_byte" J..= (0 :: Int),"buffers" J..= ([0x423de000,0x4228c800] :: [Word32]),"increments" J..= (0 :: Int)])
    , ("after",J.object ["front_byte" J..= (0 :: Int),"buffers" J..= ([0x423de000,0x4228c800] :: [Word32]),"increments" J..= (2 :: Int)]) ])
  bufferDrift <- osdInputsReader (map readerFor [beforeDraw,osd,endingOne [handoff 0x4228d000,increment 1]])
  check "later buffer replacement refuses streaming front evidence" (not (passes bufferDrift))
  provenanceCases
  putStrLn "render pair synthetic receipts pass"
