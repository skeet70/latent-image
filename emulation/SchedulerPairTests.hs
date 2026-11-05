{-# LANGUAGE OverloadedStrings #-}
module Main (main) where

import Control.Monad (forM_, unless)
import qualified Data.Aeson as J
import qualified Data.ByteString as B
import qualified Data.Map.Strict as M
import qualified Data.ByteString.Lazy as L
import qualified Emulation.OwnFirstPairCheck as Q
import Emulation.SchedulerPairCheck
import Emulation.SchedulerPairModel
import Emulation.SchedulerPairPlan
import Emulation.Sha256 (sha256,sha256Hex)
import Replacement.RtosOwnFirst.Regions (Domain (..))
import Replacement.RtosScheduler.Regions (Helper (..))

assert :: String -> Bool -> IO ()
assert label value = unless value (error label)
plan :: Plan
plan = Plan Owned "dat" [Entry DomainA (HelperSemantic Notify) (Address 0x44018),Entry DomainB (HelperSemantic Notify) (Address 0xd3fd4)]
  [Guard DomainA (Address 0x44018) (Address 0x44044) "a",Guard DomainB (Address 0xd3fd4) (Address 0xd4004) "b"] [] []
planBytes :: B.ByteString
planBytes = "synthetic-plan"
call :: Call
call = Call DomainA (Address 0x44018) (HelperSemantic Notify) 0 0xd3 (Arguments [2,0,0,0]) M.empty M.empty 0 0xd3 [] [Written (Address 0xfffb40cc) 4 0x1000000] 1 30
readback :: Q.Readback
readback = Q.Readback 100 [range d lo hi digest | Guard d (Address lo) (Address hi) digest <- planGuards plan]
 where range d lo hi digest = Q.RangeReadback d lo hi digest [Q.ViewHash core a (Right digest) | core <- [0,1],a <- [lo,lo+0x40000000]]
observation :: Observation
observation = Observation Owned [sha256Hex (sha256 planBytes),Q.runDocumentSha256,"dat",Q.bootRomSha256,trajectorySha256,"observer"] 100 1 1 0 0 Nothing
  [Q.Arming DomainA (Q.Readback 10 (take 1 (Q.readbackRanges readback))),Q.Arming DomainB (Q.Readback 20 (drop 1 (Q.readbackRanges readback)))]
  [Q.Segment [DomainA] 10 20 0 Nothing [(0x44018,0x44044,writes)],Q.Segment [DomainA,DomainB] 20 100 0 Nothing [(0x44018,0x44044,writes),(0xd3fd4,0xd4004,writes)]]
  (Just readback) [call] [] [Q.Milestone phase Nothing menu (Just menu) True 100 | phase <- ["cold-start","shooting-menu","modal75-film","modal75-returned","modal75-film-repeated","modal75-returned-repeated","modal75-handback"]]
  Nothing Nothing (BankCounts 0 1) [PcWatch 1 (Address 0xd0b00) 1]
 where writes = ["cpu-write","host-write","model-bus-write"]; menu = Q.MenuTriple 1 5 2
refuses :: String -> Refusal -> Observation -> IO ()
refuses label wanted obs = assert label (case qualify planBytes plan obs of Left actual -> actual==wanted; Right _ -> False)
main :: IO ()
main = do
  assert "correct trace qualifies" (case qualify planBytes plan observation of Right _ -> True; Left _ -> False)
  assert "plan wire round trip" (J.eitherDecodeStrict (L.toStrict (J.encode (planValue plan))) == Right plan)
  assert "observer wire trace decodes and qualifies" (case J.eitherDecodeStrict (L.toStrict (J.encode wireObservation)) of
    Right decoded -> decoded == observation && case qualify planBytes plan decoded of Right _ -> True; Left _ -> False
    Left _ -> False)
  refuses "binding" Binding observation {obsInputs=[]}
  refuses "arming" Arming observation {obsArmings=take 1 (obsArmings observation)}
  refuses "readback" Readback observation {obsFinal=Nothing}
  refuses "watch" Watch observation {obsSegments=[]}
  refuses "fill fetch" FillFetch observation {obsFill=Just J.Null}
  refuses "incomplete" Incomplete observation {obsEntries=2}
  refuses "untracked" Untracked observation {obsUntracked=1}
  refuses "unarmed" Unarmed observation {obsUnarmed=1}
  refuses "wrong core" WrongCore observation {obsCalls=[call {callCore=1}]}
  refuses "outside plan" CallOutsidePlan observation {obsCalls=[call {callSemantic=HelperSemantic Insert}]}
  refuses "return value" ReturnValue observation {obsCalls=[call {callR0=1}]}
  refuses "CPSR" ProcessorState observation {obsCalls=[call {callReturnCpsr=0x53}]}
  refuses "memory result" MemoryResult observation {obsCalls=[call {callAfter=M.singleton (Address 0x1000) 1}]}
  refuses "written words" WrittenWords observation {obsCalls=[call {callWrites=[Written (Address 0x1000) 4 1]}]}
  refuses "silent notify" BankWrites observation {obsCalls=[call {callBank=[]}]}
  refuses "route" Route observation {obsMilestones=[]}
  refuses "observer error" ObserverError observation {obsError=Just "synthetic"}
  assert "model missing data" (stockCall DomainA Acquire (Arguments [0,0,0,0]) M.empty == Left (MissingWord (Address 0)))
  assert "B id 1 is remote" (checkCall call {callDomain=DomainB,callCore=1,callArguments=Arguments [1,0,0,0],callBank=[Written (Address 0xfffb30cc) 4 0x1000000]} == Right ())
  assert "B with A predicate refuses at id 1" (checkCall call {callDomain=DomainB,callCore=1,callArguments=Arguments [1,0,0,0],callR0=1,callBank=[]} == Left ReturnValue)
  forM_ [(DomainA,0),(DomainA,1),(DomainB,0),(DomainB,2)] $ \(domain,id') -> assert "local notify" (stockCall domain Notify (Arguments [id',0,0,0]) M.empty == Right (Expected 1 M.empty mempty []))
  assert "route comparison ignores call interleavings" (compareRoutes observation {obsRole=Stock} observation {obsCalls=[call {callFirst=80}]} == Right ())
  refuses "model invalid arguments" (Model InvalidArguments) observation {obsCalls=[call {callArguments=Arguments []}]}
  let emptyQueues = M.fromList ([(Address 0x1020,0x2000),(Address 0x1024,0x3000),(Address 0x102c,0x3004),(Address 0x1030,0x3008),
        (Address 0x3000,0),(Address 0x3004,0xdead),(Address 0x3008,2)] ++ [(Address (0x2000+p*8+offset),0x2000+p*8) | p <- [0..15],offset <- [0,4]])
      emptyRotate = call {callSemantic=HelperSemantic Rotate,callArguments=Arguments [0x1000,0,0,0],callBefore=emptyQueues,callAfter=emptyQueues,callBank=[]}
  assert "queue invariant" (checkCall emptyRotate == Left QueueInvariant)
  let unreadable = M.delete (Address 0x1024) emptyQueues
  assert "invariant data unreadable" (checkCall emptyRotate {callBefore=unreadable,callAfter=unreadable} == Left EntryUnreadable)
  let queues = M.union (M.fromList [(Address 0x3000,6),(Address 0x3004,0x4000),
        (Address 0x2008,0x4000),(Address 0x200c,0x4000),(Address 0x2010,0x4100),(Address 0x2014,0x4100),
        (Address 0x4000,0x2008),(Address 0x4004,0x2008),(Address 0x400c,0x101),
        (Address 0x4100,0x2010),(Address 0x4104,0x2010),(Address 0x410c,0x201)]) emptyQueues
      changedSelected = stockCall DomainA Requeue (Arguments [0x1000,0x4000,3,0]) queues
      newSelected = stockCall DomainA Requeue (Arguments [0x1000,0x4100,0,0]) queues
  assert "requeue selected branch returns boolean dispatch" (case changedSelected of
    Right expected -> expectedR0 expected == 1 && M.lookup (Address 0x3004) (expectedMemory expected) == Just 0x4100
    Left _ -> False)
  assert "requeue another-task branch returns raw dispatch" (case newSelected of
    Right expected -> expectedR0 expected == 2 && M.lookup (Address 0x3004) (expectedMemory expected) == Just 0x4100
    Left _ -> False)
  let silent = observation {obsCalls=[call {callBank=[]}]}
      wrong = observation {obsCalls=[call {callDomain=DomainB,callCore=1,callArguments=Arguments [1,0,0,0],callR0=1,callBank=[]}]}
  assert "silent control fails at first remote notify" (controlFailure SilentNotify silent == Right ())
  assert "wrong predicate fails at first B id 1 notify" (controlFailure WrongPredicate wrong == Right ())
  assert "later silent control is not the first remote notify" (controlFailure SilentNotify observation {obsCalls=[call,call {callFirst=50,callBank=[]}]} == Left BankWrites)
  let lockMemory = M.fromList [(Address 0x1000,0x2000),(Address 0x2000,1)]
      release = call {callSemantic=HelperSemantic Release,callArguments=Arguments [0x1000,0,0,0],callBefore=lockMemory,callAfter=lockMemory,
        callR0=0x1000,callWrites=[Written (Address 0x2000) 4 0],callBank=[]}
  assert "release permits a peer acquire before continuation" (checkCall release == Right ())
  assert "release still requires its zero write" (checkCall release {callWrites=[]} == Left WrittenWords)
  let outcome label helper args memory result cells = assert label (case stockCall DomainA helper (Arguments args) memory of
        Right expected -> expectedR0 expected==result && all (\(address,value) -> M.lookup (Address address) (expectedMemory expected)==Just value) cells
        Left _ -> False)
  outcome "acquire takes the lock" Acquire [0x1000,0,0,0] lockMemory 0 [(0x2000,1)]
  outcome "release writes zero" Release [0x1000,0,0,0] lockMemory 0x1000 [(0x2000,0)]
  outcome "find chooses the lowest priority head" Find [0x1000,0,0,0] queues 0x4000 []
  outcome "remove clears an empty priority and selects next" Remove [0x1000,0x4000,0,0] queues 2 [(0x2008,0x2008),(0x3000,4),(0x3004,0x4100)]
  let resetMemory = M.fromList [(Address 0x4008,0x5000),(Address 0x500c,5),(Address 0x400c,0x00ff0201),
        (Address 0x4010,9),(Address 0x4024,1),(Address 0x4028,1),(Address 0x402c,8)]
  outcome "reset restores definition priority and clears task requests" Reset [0x4000,0,0,0] resetMemory 0x4000
    [(0x400c,0x00f90500),(0x4010,0),(0x4024,0),(0x4028,0),(0x402c,0)]
  let prepareMemory = M.union (M.fromList [(Address 0x3004,0),(Address 0x107c,0x43728),(Address 0x4000,0),(Address 0x4004,0),
        (Address 0x4008,0x5000),(Address 0x400c,0x100),(Address 0x4018,0),(Address 0x401c,0),(Address 0x5010,0x100),(Address 0x5014,0x6000)]) emptyQueues
      equalPriority = M.union (M.fromList [(Address 0x4200,0),(Address 0x4204,0),(Address 0x420c,0x100)]) queues
      rotating = M.union (M.fromList [(Address 0x3000,2),(Address 0x200c,0x4100),(Address 0x2010,0x2010),(Address 0x2014,0x2010),
        (Address 0x4000,0x4100),(Address 0x4100,0x2008),(Address 0x4104,0x4000),(Address 0x410c,0x101)]) queues
  outcome "prepare publishes stack and trampoline then inserts" Prepare [0x1000,0x4000,0,0] prepareMemory 2 [(0x4018,0x6100),(0x401c,0x43728),(0x3004,0x4000),(0x3000,2)]
  outcome "insert does not preempt equal priority" Insert [0x1000,0x4200,0,0] equalPriority 0 [(0x3004,0x4000),(0x200c,0x4200),(0x4000,0x4200)]
  outcome "rotate selects the second task and appends the first" Rotate [0x1000,1,0,0] rotating 2 [(0x3004,0x4100),(0x2008,0x4100),(0x200c,0x4000),(0x4100,0x4000),(0x4000,0x2008)]
  assert "unmodeled semantic refuses" (checkCall call {callSemantic=DispatchSemantic Save} == Left (UnsupportedSemantic (DispatchSemantic Save)))
  let firstCatalog = Catalog 0xe7f000f0 [Entry DomainA (HelperSemantic Notify) (Address 0x42000),Entry DomainB (HelperSemantic Notify) (Address 0xd2000)]
        [Window DomainA (Address 0x42000) (Address 0x42010),Window DomainB (Address 0xd2000) (Address 0xd2010)] []
      secondCatalog = Catalog 0xe7f000f0 [Entry DomainA (DispatchSemantic Save) (Address 0x42100)]
        [Window DomainA (Address 0x42100) (Address 0x42110)] [Probe DomainA (Address 0x42104) SelectedRead (Register 1)]
      catalogPlan = derivePlan Owned B.empty [firstCatalog,secondCatalog] (M.fromList [(DomainA,B.replicate 0xdae0 0),(DomainB,B.replicate 0xdaa0 0)])
  assert "catalog list preserves actual entry addresses and semantic tags" (case catalogPlan of
    Right planned -> planEntries planned==catalogEntries firstCatalog++catalogEntries secondCatalog && planProbes planned==catalogProbes secondCatalog && length (planGuards planned)==3
    Left _ -> False)
  putStrLn "scheduler-pair host tests passed"

wireObservation :: J.Value
wireObservation = J.object
  [ "schema" J..= (2::Int), "role" J..= roleName (obsRole observation)
  , "inputs" J..= [J.object ["sha256" J..= digest] | digest <- obsInputs observation]
  , "instructions" J..= obsInstructions observation, "entries" J..= obsEntries observation, "returns" J..= obsReturns observation
  , "untracked" J..= J.object ["count" J..= (0::Int)], "unarmed_entries" J..= J.object ["count" J..= (0::Int)]
  , "fill_fetch" J..= J.Null
  , "armings" J..= [J.object ["domain" J..= domain d,"readback" J..= rb value] | Q.Arming d value <- obsArmings observation]
  , "segments" J..= [J.object ["domains" J..= map domain (Q.segmentDomains segment),"started" J..= Q.segmentStarted segment,"ended" J..= Q.segmentEnded segment,
      "state" J..= J.object ["records" J..= ([]::[J.Value]),"classes" J..= [J.object ["range" J..= [lo,hi],"kinds" J..= kinds] | (lo,hi,kinds) <- Q.segmentClasses segment]]]
      | segment <- obsSegments observation]
  , "final_readback" J..= rb readback, "classes" J..= [wireCall call], "pending" J..= ([]::[J.Value])
  , "milestones" J..= [J.object ["phase" J..= Q.milestonePhase milestone,"pulse" J..= J.Null,"expected" J..= menu (Q.milestoneExpected milestone),
      "observed" J..= fmap menu (Q.milestoneObserved milestone),"reached" J..= (100::Int),"finished" J..= Q.milestoneFinished milestone] | milestone <- obsMilestones observation]
  , "error" J..= J.Null,"machine_failure" J..= J.Null,"bank_counts" J..= [primaryBankWrites (obsBankCounts observation),secondaryBankWrites (obsBankCounts observation)]
  , "pc_watches" J..= [J.object ["core" J..= core c,"address" J..= hex a,"count" J..= n] | PcWatch c (Address a) n <- obsPcWatches observation]
  ]
 where
  domain DomainA = "a" :: String
  domain DomainB = "b"
  core 0 = "primary" :: String
  core _ = "secondary"
  menu (Q.MenuTriple a b c) = J.object ["menu" J..= a,"setting" J..= b,"cursor" J..= c]
  rb value = J.object ["instructions" J..= Q.readbackInstructions value,"ranges" J..=
    [J.object ["domain" J..= domain (Q.readbackDomain range),"start" J..= hex (Q.readbackStart range),"end" J..= hex (Q.readbackEnd range),
      "physical_sha256" J..= Q.readbackPhysical range,"views" J..= [J.object ["core" J..= Q.viewCore view,"address" J..= hex (Q.viewAddress view),
        "sha256" J..= either (const "") id (Q.viewDigest view)] | view <- Q.readbackViews range]] | range <- Q.readbackRanges value]]
  wireCall value = J.object ["entry" J..= J.object ["slot" J..= J.object ["domain" J..= domain (callDomain value),"entry" J..= addressOf (callAddress value),"semantic" J..= semanticValue (callSemantic value)],
      "core" J..= core (callCore value),"cpsr" J..= callCpsr value,"arguments" J..= arguments (callArguments value),"memory" J..= wordsOf (callBefore value)],
    "result" J..= J.object ["r0" J..= callR0 value,"cpsr" J..= callReturnCpsr value,"memory" J..= wordsOf (callAfter value),
      "writes" J..= map writeOf (callWrites value),"bank" J..= map writeOf (callBank value)],"count" J..= callCount value,"first" J..= callFirst value]
  arguments (Arguments values) = values
  wordsOf memory = [J.object ["address" J..= hex address,"value" J..= J.object ["view" J..= ("committed"::String),"value" J..= value]] | (Address address,value) <- M.toList memory]
  writeOf (Written (Address address) size value) = J.object ["address" J..= hex address,"size" J..= size,"value" J..= value]
  addressOf (Address address) = hex address
