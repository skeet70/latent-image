{-# LANGUAGE OverloadedStrings #-}
module Main (main) where

import Control.Monad (forM_, unless)
import Data.Bits (shiftR)
import Data.Word (Word32)
import qualified Data.Aeson as J
import qualified Data.Aeson.KeyMap as K
import qualified Data.ByteString as B
import qualified Data.Map.Strict as M
import qualified Data.Set as S
import qualified Data.ByteString.Lazy as L
import qualified Emulation.OwnFirstPairCheck as Q
import Emulation.SchedulerPairCheck
import Emulation.SchedulerPairDecode (decodeCheckedObservation, decodeControlObservation)
import Emulation.SchedulerPairEvidence (qualifyEvidence, qualifyControlEvidence, checkControlEvidence)
import Emulation.SchedulerPairModel
import Emulation.SchedulerPairPlan
import qualified Emulation.SchedulerPairS2 as D
import qualified Emulation.SchedulerPairService as V
import Emulation.Sha256 (sha256,sha256Hex)
import Replacement.RtosOwnFirst.Regions (Domain (..))
import Replacement.RtosScheduler.Regions (Helper (..))

assert :: String -> Bool -> IO ()
assert label value = unless value (error label)
plan :: Plan
plan = Plan Owned "dat" [Entry DomainA (HelperSemantic Notify) (Address 0x44018),Entry DomainB (HelperSemantic Notify) (Address 0xd3fd4)]
  [Guard DomainA (Address 0x44018) (Address 0x44044) "a",Guard DomainB (Address 0xd3fd4) (Address 0xd4004) "b"] [] [] retainedArmingEntries [] []
planBytes :: B.ByteString
planBytes = "synthetic-plan"
call :: Call
call = Call DomainA (Address 0x44018) (HelperSemantic Notify) 0 0xd3 (Arguments [2,0,0,0]) M.empty M.empty 0 0xd3 [] [Written (Address 0xfffb40cc) 4 0x1000000] 1 30 (Just (CallTrace registers registers Returned [] Nothing)) 0x9000 0 Nothing [] [ownOrigin] S.empty
 where registers = D.Registers (replicate 16 0) 0xd3 0 0 0
ownOrigin :: WriteOrigin
ownOrigin = WriteOrigin 0 (Address 0x44018) (Address 0x8000) 0xd3 (Address 0x9000) 0 1
readback :: Q.Readback
readback = Q.Readback 100 [range d lo hi digest | Guard d (Address lo) (Address hi) digest <- planGuards plan]
 where range d lo hi digest = Q.RangeReadback d lo hi digest [Q.ViewHash core a (Right digest) | core <- [0,1],a <- [lo,lo+0x40000000]]
observation :: Observation
observation = Observation Owned [sha256Hex (sha256 planBytes),Q.runDocumentSha256,"dat",Q.bootRomSha256,trajectorySha256,"observer"] 100 1 1 0 0 Nothing
  [Q.Arming DomainA (Q.Readback 10 (take 1 (Q.readbackRanges readback))),Q.Arming DomainB (Q.Readback 20 (drop 1 (Q.readbackRanges readback)))]
  [Q.Segment [DomainA] 10 20 0 Nothing [(0x44018,0x44044,writes)],Q.Segment [DomainA,DomainB] 20 100 0 Nothing [(0x44018,0x44044,writes),(0xd3fd4,0xd4004,writes)]]
  (Just readback) [call] [] ([],[]) [Q.Milestone phase Nothing menu (Just menu) True 100 | phase <- ["cold-start","shooting-menu","modal75-film","modal75-returned","modal75-film-repeated","modal75-returned-repeated","modal75-handback"]]
  Nothing Nothing (BankCounts 0 1) [PcWatch 1 (Address 0xd0b00) 1] []
 where writes = ["cpu-write","host-write","model-bus-write"]; menu = Q.MenuTriple 1 5 2
refuses :: String -> Refusal -> Observation -> IO ()
refuses label wanted obs = assert label (case qualify planBytes plan obs of Left actual -> actual==wanted; Right _ -> False)
main :: IO ()
main = do
  assert "correct trace qualifies" (case qualify planBytes plan observation of Right _ -> True; Left _ -> False)
  assert "plan wire round trip" (J.eitherDecodeStrict (L.toStrict (J.encode (planValue plan))) == Right plan)
  let prefixArmings=either error id (armingEntries (Address 0x40020) (Address 0x1042c))
      prefixPlan=plan {planArmingEntries=prefixArmings}
      planFields=case planValue plan of J.Object fields->fields; _->error "fixture plan shape"
      domainEntry domain address=J.object ["domain" J..= (domain::String),"loader_entry" J..= (address::String)]
      primary=domainEntry "a" "0x40020"
      secondary=domainEntry "b" "0x1042c"
      withDomains value=J.Object (K.insert "domains" value planFields)
      decodePlanValue :: J.Value -> Either String Plan
      decodePlanValue value=J.eitherDecode (J.encode value)
      rejectsPlan value=case decodePlanValue value of Left _->True; Right _->False
  assert "relocated prefix arming survives the public plan wire round trip"
    (decodePlanValue (planValue prefixPlan)==Right prefixPlan)
  assert "domain row order does not change arming identity"
    (decodePlanValue (withDomains (J.toJSON [secondary,primary]))==Right prefixPlan)
  forM_
    [ J.Object (K.delete "domains" planFields),withDomains J.Null
    , withDomains (J.toJSON ([]::[J.Value])),withDomains (J.toJSON [primary])
    , withDomains (J.toJSON [primary,primary]),withDomains (J.toJSON [secondary,secondary])
    , withDomains (J.toJSON [primary,secondary,secondary])
    , withDomains (J.toJSON [primary,domainEntry "c" "0x1042c"])
    , withDomains (J.toJSON [primary,domainEntry "b" "0x0"])
    , withDomains (J.toJSON [primary,domainEntry "b" "0x1042d"])
    , withDomains (J.toJSON [primary,domainEntry "b" "0x40020"])
    ] $ \value->assert "malformed domain arming metadata refuses at the wire boundary" (rejectsPlan value)
  let encoded = J.encode wireObservation
      expectedEntry = J.object ["domain" J..= ("a"::String),"entry" J..= ("0x44018"::String)
        ,"semantic" J..= J.object ["kind" J..= ("helper"::String),"name" J..= ("notify"::String)],"calls" J..= (1::Int)]
      qualifiesReceipt bytes = case decodeCheckedObservation plan bytes of
        Right (decoded, evidence) -> case qualifyEvidence planBytes plan decoded evidence of
          Right (J.Object result) -> K.lookup "calls" result==Just (J.toJSON (1::Int))
            && K.lookup "per_entry" result==Just (J.toJSON [expectedEntry])
          _ -> False
        Left _ -> False
      rejectsReceipt bytes = case decodeCheckedObservation plan bytes of Left _ -> True; Right _ -> False
      wireFields = case wireObservation of J.Object fields -> fields; _ -> error "fixture is not an object"
      validClasses = case K.lookup "classes" wireFields of Just (J.Array values) -> values; _ -> error "missing fixture classes"
      lateInvalidClass = J.encode (J.Object (K.insert "classes" (J.Array (validClasses <> pure J.Null)) wireFields))
  assert "fragmented observer receipt reports its qualified notification"
    (qualifiesReceipt (L.fromChunks (map B.singleton (L.unpack encoded))))
  assert "malformed final receipt byte refuses" (rejectsReceipt (L.init encoded))
  assert "trailing receipt data refuses" (rejectsReceipt (encoded <> " true"))
  assert "missing class array refuses" (rejectsReceipt (J.encode (J.Object (K.delete "classes" wireFields))))
  assert "invalid class array element refuses" (rejectsReceipt lateInvalidClass)
  assert "invalid first schema cannot be replaced by a duplicate"
    (rejectsReceipt ("{\"schema\":2," <> L.drop 1 encoded))
  assert "invalid first classes cannot be replaced by a duplicate"
    (rejectsReceipt ("{\"classes\":[null]," <> L.drop 1 encoded))
  assert "later duplicate classes do not replace the first"
    (qualifiesReceipt (L.init encoded <> ",\"classes\":[null]}"))
  assert "ignored duplicate values still require valid JSON"
    (rejectsReceipt (L.init encoded <> ",\"classes\":[true,]}"))
  let classFields = case foldr (:) [] validClasses of [J.Object fields] -> fields; _ -> error "fixture class shape"
      resultFields = case K.lookup "result" classFields of Just (J.Object fields) -> fields; _ -> error "fixture result shape"
      badReturn = J.Object (K.insert "first" (J.toJSON (80::Int)) (K.insert "result"
        (J.Object (K.insert "r0" (J.toJSON (1::Int)) resultFields)) classFields))
      badBank = J.Object (K.insert "first" (J.toJSON (20::Int)) (K.insert "result"
        (J.Object (K.insert "bank" (J.Array mempty) resultFields)) classFields))
      goodClass = J.Object classFields
      twoFields = K.insert "entries" (J.toJSON (2::Int)) (K.insert "returns" (J.toJSON (2::Int)) wireFields)
      twoClasses :: [J.Value] -> J.Value
      twoClasses values = J.Object (K.insert "classes" (J.toJSON values) twoFields)
      streamedVerdict :: J.Value -> Either Refusal J.Value
      streamedVerdict value = case decodeCheckedObservation plan (J.encode value) of
        Left failure -> error failure
        Right (decoded, evidence) -> qualifyEvidence planBytes plan decoded evidence
  assert "streamed final class still receives its semantic check"
    (streamedVerdict (twoClasses [goodClass,badReturn])==Left ReturnValue)
  assert "streamed failures retain entered-order precedence"
    (streamedVerdict (twoClasses [badReturn,badBank])==Left BankWrites)
  let untrackedFields = K.insert "untracked" (J.object ["count" J..= (1::Int)]) twoFields
  assert "streamed class failures do not displace earlier metadata refusals"
    (streamedVerdict (J.Object (K.insert "classes" (J.toJSON [goodClass,badReturn]) untrackedFields))==Left Untracked)
  let entryFields = case K.lookup "entry" classFields of Just (J.Object fields) -> fields; _ -> error "fixture entry shape"
      silentFields = K.insert "result" (J.Object (K.insert "bank" (J.Array mempty) resultFields)) classFields
      silentTarget = J.Object silentFields
      lateSilentTarget = J.Object (K.insert "first" (J.toJSON (80::Int)) silentFields)
      spuriousLocal = J.Object (K.insert "first" (J.toJSON (20::Int)) (K.insert "entry"
        (J.Object (K.insert "arguments" (J.toJSON ([0,0,0,0]::[Int])) entryFields))
        (K.insert "result" (J.Object (K.insert "r0" (J.toJSON (1::Int)) resultFields)) classFields)))
      silentPlan = plan {planRole=SilentNotify}
      controlOutcome :: [J.Value] -> (Either Refusal J.Value, Either Refusal ())
      controlOutcome values =
        let fields = K.insert "role" (J.String "control-silent-notify") (K.insert "classes" (J.toJSON values)
              (K.insert "entries" (J.toJSON (length values)) (K.insert "returns" (J.toJSON (length values)) wireFields)))
        in case decodeControlObservation silentPlan (J.encode (J.Object fields)) of
          Left failure -> error failure
          Right (decoded, evidence) -> (qualifyControlEvidence planBytes silentPlan decoded evidence,
            checkControlEvidence SilentNotify decoded evidence)
  assert "streamed control keeps the missing-notification witness"
    (controlOutcome [silentTarget]==(Left BankWrites,Right ()))
  assert "streamed control refuses an unrelated earlier bank failure"
    (controlOutcome [spuriousLocal,silentTarget]==(Left BankWrites,Left BankWrites))
  assert "streamed control requires an actual notification target"
    (controlOutcome [spuriousLocal]==(Left BankWrites,Left Incomplete))
  assert "a later defect cannot replace a passing first control target"
    (controlOutcome [goodClass,lateSilentTarget]==(Left BankWrites,Left BankWrites))
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
  refuses "direct helper retains A equality" ProcessorState observation {obsCalls=[call {callReturnCpsr=0x1d3}]}
  let stackedCall=call {callTaskStack=Just (TaskStack (Address 0x7800) (Address 0x8000))}
  assert "calling task stack top is admitted" (checkCall stackedCall==Right ())
  assert "foreign stack cannot supply invocation bank effects" (checkCall stackedCall
    {callBankOrigins=[ownOrigin {writerSp=Address 0x7000}]}==Left MissingTrace)
  assert "foreign processor mode cannot supply invocation bank effects" (checkCall call
    {callBankOrigins=[ownOrigin {writerCpsr=0x17}]}==Left MissingTrace)
  let restoredRegisters pc sp cpsr = D.Registers (replicate 13 0++[sp,0,pc]) cpsr 0 0 0
      saved = Boundary SaveBoundary (Address 0x43640) 31 0 (restoredRegisters 0x43640 0x8000 0xd3) M.empty [] [] Nothing 0x9000 [] [] S.empty
      resumed = Boundary ResumeBoundary (Address 0x43668) 32 0 (restoredRegisters 0x43668 0x7fdc 0x1d3) M.empty [] [] Nothing 0x9000 [] [] S.empty
      restoredCall history = call {callReturnCpsr=0x1d3,callTrace=Just (CallTrace (restoredRegisters 0 0x8100 0xd3)
        (restoredRegisters 0 0x8100 0x1d3) Returned history Nothing)}
  assert "observed task restore permits only asynchronous A change" (checkCall (restoredCall [saved,resumed])==Right ())
  forM_ [0x80,0x40,0x20,1] $ \bit -> assert "I F T and mode remain checked after restore"
    (checkCall (restoredCall [saved,resumed]) {callReturnCpsr=0x1d3+bit}==Left ProcessorState)
  forM_ [[saved],[resumed],[saved,resumed {boundaryTask=0x9010}],[saved,resumed {boundaryCore=1}],
      [saved,resumed {boundaryRegisters=restoredRegisters 0x43668 0x7fd8 0x1d3}],
      [saved,resumed {boundaryRegisters=restoredRegisters 0x43668 0x7fdc 0x193}],
      [saved,resumed {boundaryRegisters=restoredRegisters 0x4366c 0x7fdc 0x1d3}]] $ \history ->
    assert "unproven restore retains A equality" (checkCall (restoredCall history)==Left ProcessorState)
  let foreignWrite=Written (Address 0x2000) 4 1
      foreignOrigin=ownOrigin {writerTask=Address 0xa000}
      interleaved=(restoredCall [saved,resumed]) {callBefore=M.singleton (Address 0x2000) 0,
        callAfter=M.singleton (Address 0x2000) 1,callWrites=[foreignWrite],callWriteOrigins=[foreignOrigin]}
  assert "foreign writes remain replayed concurrency inputs after a validated restore" (checkCall interleaved==Right ())
  assert "foreign effects require a same-core restore witness" (checkCall interleaved {callTrace=callTrace call,callReturnCpsr=0xd3}==Left MissingTrace)
  assert "foreign writes must reconcile with final state" (checkCall interleaved {callAfter=callBefore interleaved}==Left MemoryResult)
  assert "same-context extra writes still refuse" (checkCall interleaved {callWriteOrigins=[ownOrigin]}==Left WrittenWords)
  let irqOrigin=ownOrigin {writerContext=1,writerSp=Address 0x70001f80,writerPc=Address 0x49610}
  assert "IRQ writes retain their raw envelope after a witnessed restore" (checkCall interleaved {callWriteOrigins=[irqOrigin]}==Right ())
  assert "IRQ writers cannot pass as ordinary service effects" (checkCall interleaved {callTrace=callTrace call,callReturnCpsr=0xd3,callWriteOrigins=[irqOrigin]}==Left MissingTrace)
  refuses "memory result" MemoryResult observation {obsCalls=[call {callAfter=M.singleton (Address 0x1000) 1}]}
  refuses "written words" WrittenWords observation {obsCalls=[call {callWrites=[Written (Address 0x1000) 4 1]}]}
  refuses "silent notify" BankWrites observation {obsCalls=[call {callBank=[],callBankOrigins=[]}]}
  refuses "route" Route observation {obsMilestones=[]}
  refuses "observer error" ObserverError observation {obsError=Just "synthetic"}
  assert "model missing data" (stockCall DomainA Acquire (Arguments [0,0,0,0]) M.empty == Left (MissingWord (Address 0)))
  assert "B id 1 is remote" (checkCall call {callDomain=DomainB,callCore=1,callArguments=Arguments [1,0,0,0],callBank=[Written (Address 0xfffb30cc) 4 0x1000000],callBankOrigins=[ownOrigin {writerCore=1,writerPc=Address 0xd3fd4}]} == Right ())
  assert "B with A predicate refuses at id 1" (checkCall call {callDomain=DomainB,callCore=1,callArguments=Arguments [1,0,0,0],callR0=1,callBank=[],callBankOrigins=[]} == Left ReturnValue)
  forM_ [(DomainA,0),(DomainA,1),(DomainB,0),(DomainB,2)] $ \(domain,id') -> assert "local notify" (stockCall domain Notify (Arguments [id',0,0,0]) M.empty == Right (Expected 1 M.empty mempty []))
  assert "route comparison ignores call interleavings" (compareRoutes observation {obsRole=Stock} observation {obsCalls=[call {callFirst=80}]} == Right ())
  let separatelyObserved = observation { obsInputs = take 5 (obsInputs observation) ++ ["owned-observer"] }
      stockObserved = observation { obsRole = Stock }
  assert "route comparison permits separately verified observer identities"
    (compareRoutes stockObserved separatelyObserved == Right ())
  assert "route comparison rejects a different boot ROM"
    (compareRoutes stockObserved separatelyObserved { obsInputs = take 3 (obsInputs separatelyObserved) ++ ["other-rom"] ++ drop 4 (obsInputs separatelyObserved) } == Left Binding)
  assert "route comparison rejects a different trajectory"
    (compareRoutes stockObserved separatelyObserved { obsInputs = take 4 (obsInputs separatelyObserved) ++ ["other-trajectory"] ++ drop 5 (obsInputs separatelyObserved) } == Left Binding)
  assert "route comparison rejects truncated per-role bindings"
    (compareRoutes stockObserved separatelyObserved { obsInputs = take 5 (obsInputs separatelyObserved) } == Left Binding)
  refuses "model invalid arguments" (Model InvalidArguments) observation {obsCalls=[call {callArguments=Arguments []}]}
  let emptyQueues = M.fromList ([(Address 0x1020,0x2000),(Address 0x1024,0x3000),(Address 0x102c,0x3004),(Address 0x1030,0x3008),
        (Address 0x3000,0),(Address 0x3004,0xdead),(Address 0x3008,2)] ++ [(Address (0x2000+p*8+offset),0x2000+p*8) | p <- [0..15],offset <- [0,4]])
      emptyRotate = call {callSemantic=HelperSemantic Rotate,callArguments=Arguments [0x1000,0,0,0],callBefore=emptyQueues,callAfter=emptyQueues,callBank=[],callBankOrigins=[]}
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
  let silent = observation {obsCalls=[call {callBank=[],callBankOrigins=[]}]}
      wrong = observation {obsCalls=[call {callDomain=DomainB,callCore=1,callArguments=Arguments [1,0,0,0],callR0=1,callBank=[],callBankOrigins=[]}]}
  assert "silent control fails at first remote notify" (controlFailure SilentNotify silent == Right ())
  assert "wrong predicate fails at first B id 1 notify" (controlFailure WrongPredicate wrong == Right ())
  assert "later silent control is not the first remote notify" (controlFailure SilentNotify observation {obsCalls=[call,call {callFirst=50,callBank=[],callBankOrigins=[]}]} == Left BankWrites)
  let lockMemory = M.fromList [(Address 0x1000,0x2000),(Address 0x2000,1)]
      release = call {callSemantic=HelperSemantic Release,callArguments=Arguments [0x1000,0,0,0],callBefore=lockMemory,callAfter=lockMemory,
        callR0=0x1000,callWrites=[Written (Address 0x2000) 4 0,Written (Address 0x2000) 4 1],callBank=[],
        callWriteOrigins=[ownOrigin,ownOrigin {writerCore=1,writerTask=Address 0xa000}],callBankOrigins=[]}
  assert "release permits a peer acquire before continuation" (checkCall release == Right ())
  assert "release still requires its zero write" (checkCall release {callWrites=[Written (Address 0x2000) 4 1],
    callWriteOrigins=[ownOrigin {writerCore=1,writerTask=Address 0xa000}]} == Left WrittenWords)
  let serviceMemory = M.fromList [(Address address,value) | (address,value) <-
        [(0x400545a4,0),(0x400545c8,0x1000),(0x1008,0x2000),(0x2000,1),
         (0x40054760,0x3000),(0x300c,1),(0x40054764,1)]]
      serviceFinal=M.insert (Address 0x40054764) 0 (M.insert (Address 0x2000) 0 serviceMemory)
      serviceRegisters=D.Registers (replicate 16 0) 0x13 0 0 0
      acquired=Boundary AcquiredBoundary (Address 0x4000) 31 0 serviceRegisters serviceMemory [Written (Address 0x2000) 4 1] [] (Just 0) 0x9000 [ownOrigin] [] S.empty
      returned=Boundary ReturnBoundary (Address 0x5000) 40 0 serviceRegisters serviceFinal
        [Written (Address 0x40054764) 4 0,Written (Address 0x2000) 4 0] [] Nothing 0x9000 [ownOrigin,ownOrigin] [] S.empty
      disabled=call {callSemantic=ServiceSemantic DisableDispatch,callCpsr=0x13,callArguments=Arguments [0,0,0,0],
        callBefore=serviceMemory,callAfter=serviceFinal,callTrace=Just (CallTrace serviceRegisters serviceRegisters Returned [acquired,returned] Nothing)}
  assert "service judges state after contended acquisition" (checkCall disabled==Right ())
  let unreadableEvent kind = returned
        { boundaryKind=kind, boundaryInstructions=35
        , boundaryMemory=M.delete (Address 0x400545a4) serviceMemory
        , boundaryUnreadable=S.singleton (Address 0x400545a4)
        , boundaryWrites=[], boundaryWriteOrigins=[] }
      interposedService event = disabled
        { callTrace=Just (CallTrace serviceRegisters serviceRegisters Returned [acquired,event,returned] Nothing)
        , callUnreadable=S.singleton (Address 0x400545a4) }
  forM_ [SaveBoundary,NoSaveBoundary,RetryBoundary,CallbackBoundary,ReturnBoundary,ResumeBoundary,CallbackReturnBoundary,AcquiredBoundary] $ \kind ->
    assert ("unreadable service ownership cannot be skipped "++show kind)
      (checkCall (interposedService (unreadableEvent kind))==Left EntryUnreadable)
  let foreignIrq = (unreadableEvent ReturnBoundary)
        { boundaryRegisters=serviceRegisters { D.processorState=0x92 } }
  assert "known foreign IRQ service event does not require caller context"
    (checkCall (interposedService foreignIrq)==Right ())
  assert "direct service retains A equality" (checkCall disabled {callTrace=Just (CallTrace serviceRegisters serviceRegisters Returned
    [acquired,returned {boundaryRegisters=serviceRegisters {D.processorState=0x113}}] Nothing)}==Left ProcessorState)
  assert "service still requires dispatch disable write" (checkCall disabled {callTrace=Just (CallTrace serviceRegisters serviceRegisters Returned
    [acquired,returned {boundaryWrites=[Written (Address 0x2000) 4 0],boundaryWriteOrigins=[ownOrigin]}] Nothing)}==Left WrittenWords)
  let unreadableReturn=returned {boundaryWrites=Written (Address 0x301c) 4 0:boundaryWrites returned,
        boundaryWriteOrigins=ownOrigin:boundaryWriteOrigins returned}
      unreadableCall=disabled {callUnreadable=S.singleton (Address 0x301c),
        callTrace=Just (CallTrace serviceRegisters serviceRegisters Returned [acquired,unreadableReturn] Nothing)}
  assert "unreadable non-scratch saved continuation write refuses" (checkCall unreadableCall==Left WrittenWords)
  assert "irrelevant unwritten unreadable word passes" (checkCall disabled {callUnreadable=S.singleton (Address 0x301c)}==Right ())
  assert "required unreadable word refuses" (case checkCall disabled {callBefore=M.delete (Address 0x40054764) serviceMemory,
    callUnreadable=S.singleton (Address 0x40054764)} of Left _ -> True; Right _ -> False)
  let unreadableAcquisition=acquired {boundaryMemory=M.delete (Address 0x300c) serviceMemory,
        boundaryUnreadable=S.singleton (Address 0x300c)}
      acquisitionCall=disabled {callUnreadable=S.singleton (Address 0x300c),
        callTrace=Just (CallTrace serviceRegisters serviceRegisters Returned [unreadableAcquisition,returned] Nothing)}
  assert "unreadable acquisition word cannot fall back to entry" (case checkCall acquisitionCall of Left _ -> True; Right _ -> False)
  let scratchMemory=M.union (M.fromList [(Address 0x3008,0x6000),(Address 0x6010,0x1000),(Address 0x6014,0x7000),(Address 0x77f0,9)]) serviceMemory
      scratchRegisters=serviceRegisters {D.registers=replicate 13 0 ++ [0x7800,0,0]}
      scratchAcquired=acquired {boundaryMemory=scratchMemory,boundaryWrites=Written (Address 0x77f0) 4 7:boundaryWrites acquired,
        boundaryWriteOrigins=ownOrigin:boundaryWriteOrigins acquired}
      scratchReturned=returned {boundaryMemory=M.union serviceFinal scratchMemory}
      scratchCall=disabled {callBefore=scratchMemory,callAfter=boundaryMemory scratchReturned,
        callTrace=Just (CallTrace scratchRegisters scratchRegisters Returned [scratchAcquired,scratchReturned] Nothing)}
  assert "service ignores its own downward stack scratch" (checkCall scratchCall==Right ())
  assert "service refuses unrelated non-stack writes" (checkCall scratchCall {callTrace=Just (CallTrace scratchRegisters scratchRegisters Returned
    [scratchAcquired,returned {boundaryWrites=Written (Address 0x300c) 4 9:boundaryWrites returned,
      boundaryWriteOrigins=ownOrigin:boundaryWriteOrigins returned}] Nothing)}==Left WrittenWords)
  let concurrentFinal=M.insert (Address 0x300c) 5 serviceFinal
      concurrentReturn=returned {boundaryMemory=concurrentFinal,boundaryWrites=boundaryWrites returned++[Written (Address 0x300c) 4 5],
        boundaryWriteOrigins=boundaryWriteOrigins returned++[ownOrigin {writerCore=1,writerTask=Address 0xa000}]}
      concurrentCall=disabled {callAfter=concurrentFinal,callTrace=Just (CallTrace serviceRegisters serviceRegisters Returned [acquired,concurrentReturn] Nothing)}
  assert "shared task state reconciles all raw foreign writes" (checkCall concurrentCall==Right ())
  assert "missing raw concurrent mutation refuses" (checkCall concurrentCall {callTrace=Just (CallTrace serviceRegisters serviceRegisters Returned
    [acquired,returned {boundaryMemory=concurrentFinal}] Nothing)}==Left MemoryResult)
  assert "foreign mutation cannot substitute for missing owned service write" (checkCall concurrentCall {callTrace=Just (CallTrace serviceRegisters serviceRegisters Returned
    [acquired,concurrentReturn {boundaryWrites=[Written (Address 0x2000) 4 0,Written (Address 0x300c) 4 5],
      boundaryWriteOrigins=[ownOrigin,ownOrigin {writerCore=1,writerTask=Address 0xa000}]}] Nothing)}==Left WrittenWords)
  let nestedControlFixture domain =
        let words'=V.domainWords domain
            core=if domain==DomainA then 0 else 1
            remoteOwner=if domain==DomainA then 2 else 1
            record=V.contextTable words'+(remoteOwner-1)*0x84
            heads=[(0x30000+p*8+offset,0x30000+p*8) | p<-[0..15],offset<-[0,4]]
            memory=M.fromList [(Address at,value) | (at,value) <- heads++
              [(V.contextWord words',0),(V.currentContext words',0x1000),(V.runningCell words',0x3000),
               (record,remoteOwner),(record+8,0x20000),(record+0x20,0x30000),(record+0x24,0x40000),
               (record+0x2c,0x40004),(record+0x30,0x40008),(record+0x44,0x50000),(record+0x48,1),
               (0x20000,1),(0x40000,0x8000),(0x40004,0x51000),(0x40008,1),
               (0x50000,0),(0x50004,0),(0x50008,0x60000),(0x5000c,10),(0x50014,0x70000),
               (0x50024,0),(0x50028,0),(0x5002c,0),(0x60024,1),(0x70000,0),
               (0x51000,0x30078),(0x51004,0x30078),(0x5100c,0xf01),(0x30078,0x51000),(0x3007c,0x51000)]]
            effects=[Written (Address at) size value | (at,size,value) <-
              [(0x70000,4,0),(0x5002c,4,0),(0x5000c,1,1),(0x50004,4,0x30000),(0x50000,4,0x30000),
               (0x30000,4,0x50000),(0x30004,4,0x50000),(0x40000,4,0x8001),(0x40004,4,0x50000),(0x20000,4,0)]]
            final=M.union (M.fromList [(Address at,value) | (at,value) <-
              [(0x5000c,1),(0x50004,0x30000),(0x50000,0x30000),(0x30000,0x50000),
               (0x30004,0x50000),(0x40000,0x8001),(0x40004,0x50000),(0x20000,0)]]) memory
            origin=ownOrigin {writerCore=core,writerTask=Address 0x3000}
            entered=D.Registers ([remoteOwner*0x10000+1]++replicate 12 0++[0x8100,0x600,0x500]) 0x53 0 0 0
            finished=D.Registers (replicate 13 0++[0x8100,0x600,0x600]) 0x53 0 0 0
            locked=D.Registers (replicate 13 0++[0x8000,0x550,0x550]) 0xd3 0 0 0
            acquiredRemote=Boundary AcquiredBoundary (Address 0x550) 31 core locked memory
              [Written (Address 0x20000) 4 1] [] (Just 0) 0x3000 [origin] [] S.empty
            returnedRemote=Boundary ReturnBoundary (Address 0x600) 40 core finished final effects [] Nothing 0x3000 (map (const origin) effects) [] S.empty
            parent=call {callDomain=domain,callAddress=Address 0x500,callSemantic=ServiceSemantic WakeTask,callCore=core,
              callTask=0x3000,callCpsr=0x53,callReturnCpsr=0x53,callArguments=Arguments [remoteOwner*0x10000+1,0,0,0],
              callBefore=memory,callAfter=final,callBank=[],callBankOrigins=[],
              callTrace=Just (CallTrace entered finished Returned [acquiredRemote,returnedRemote] Nothing)}
            target=call {callDomain=domain,callAddress=Address (if domain==DomainA then 0x44018 else 0xd3fd4),
              callCore=core,callTask=0x3000,callFirst=35,callArguments=Arguments [remoteOwner,0,0,0],callBank=[],callBankOrigins=[]}
        in (parent,target)
      (remoteParent,remoteNotify)=nestedControlFixture DomainA
      controlled parent target=observation {obsRole=SilentNotify,obsCalls=[parent,target]}
      corruptedParent=remoteParent {callAfter=M.insert (Address 0x5000c) 7 (callAfter remoteParent)}
      unfinishedParent=remoteParent {callTrace=fmap (\trace -> trace {traceCompletion=Discarded}) (callTrace remoteParent)}
      (otherParent,otherNotify)=nestedControlFixture DomainB
      independent=call {callFirst=32,callArguments=Arguments [0,0,0,0],callBank=[],callBankOrigins=[]}
      bankedParent origin=remoteParent {callTrace=fmap (\trace -> trace {traceHistory=map
        (\boundary -> if boundaryKind boundary==ReturnBoundary then boundary
          {boundaryBank=[Written (Address 0xfffb30cc) 4 0x1000000],boundaryBankOrigins=[origin]} else boundary)
        (traceHistory trace)}) (callTrace remoteParent)}
      wrongNotify=otherNotify {callR0=1}
      dispatchHistory boundary
        | boundaryKind boundary/=ReturnBoundary = [boundary]
        | otherwise =
          [ boundary {boundaryKind=SaveBoundary,boundaryAddress=Address 0xd3640,boundaryInstructions=37,
              boundaryRegisters=D.Registers ([0xffffffed]++replicate 12 0++[0x8000,0x580,0xd3640]) 0xd3 0 0 0,
              boundaryValue=Just 0xffffffed}
          , Boundary ResumeBoundary (Address 0xd3668) 38 1
              (D.Registers (replicate 13 0++[0x7fdc,0x580,0xd3668]) 0xd3 0 0 0)
              (boundaryMemory boundary) [] [] Nothing 0x3000 [] [] S.empty
          , boundary {boundaryWrites=[],boundaryWriteOrigins=[]} ]
      wrongParent=otherParent {callTrace=fmap (\trace -> trace {traceHistory=concatMap dispatchHistory (traceHistory trace)}) (callTrace otherParent)}
      wrongControlled parent target=(controlled parent target) {obsRole=WrongPredicate}
      changeBoundary kind change parent=parent {callTrace=fmap (\trace -> trace
        {traceHistory=map (\boundary -> if boundaryKind boundary==kind then change boundary else boundary) (traceHistory trace)}) (callTrace parent)}
      changeFinal change parent=
        let changed=fmap (\trace ->
              let finished=change (traceAfter trace)
              in trace {traceAfter=finished,traceHistory=map (\boundary -> if boundaryKind boundary==ReturnBoundary
                then boundary {boundaryRegisters=finished} else boundary) (traceHistory trace)}) (callTrace parent)
        in parent {callTrace=changed,callReturnCpsr=maybe (callReturnCpsr parent) (D.processorState . traceAfter) changed}
      changeR4 state=case D.registers state of
        r0:r1:r2:r3:_:following -> state {D.registers=r0:r1:r2:r3:1:following}
        _ -> error "fixture registers must include r4"
      wrongFrame=changeBoundary ResumeBoundary (\boundary -> boundary
        {boundaryRegisters=D.Registers (replicate 13 0++[0x7fe0,0x580,0xd3668]) 0xd3 0 0 0}) wrongParent
      wrongReason=changeBoundary SaveBoundary (\boundary -> boundary {boundaryValue=Just 0xfffffff9,
        boundaryRegisters=(boundaryRegisters boundary) {D.registers=0xfffffff9:drop 1 (D.registers (boundaryRegisters boundary))}}) wrongParent
      unrelatedWrites=changeBoundary SaveBoundary (\boundary -> boundary
        {boundaryWrites=boundaryWrites boundary++[Written (Address (V.contextTable (V.domainWords DomainB))) 4 value | value <- [2,1]],
         boundaryWriteOrigins=boundaryWriteOrigins boundary++replicate 2 ownOrigin {writerCore=1,writerTask=Address 0x3000}}) wrongParent
  forM_ [(remoteParent,remoteNotify),(otherParent,otherNotify)] $ \(parent,target) -> do
    assert "remote wake fixture refuses only its missing bank write" (checkCall parent==Left BankWrites)
    assert "first missing notify also explains its enclosing service bank failure" (controlFailure SilentNotify (controlled parent target)==Right ())
    let contextAddress = Address (V.contextWord (V.domainWords (callDomain parent)))
        unknownParent = (changeBoundary ReturnBoundary (\boundary -> boundary
          { boundaryMemory=M.delete contextAddress (boundaryMemory boundary)
          , boundaryUnreadable=S.insert contextAddress (boundaryUnreadable boundary) }) parent)
          { callUnreadable=S.insert contextAddress (callUnreadable parent) }
    assert "unreadable caller return cannot establish an enclosing notify defect"
      (case controlFailure SilentNotify (controlled unknownParent target) of Left _ -> True; Right () -> False)
  assert "foreign bank effects remain separate from a missing invocation transaction" (controlFailure SilentNotify
    (controlled (bankedParent ownOrigin {writerCore=1,writerTask=Address 0xa000,writerPc=Address 0xd3fd4}) remoteNotify)==Right ())
  assert "unexpected owned bank effects are not missing-transaction propagation" (controlFailure SilentNotify
    (controlled (bankedParent ownOrigin {writerTask=Address 0x3000}) remoteNotify)==Left BankWrites)
  assert "notify after service return cannot explain an earlier bank failure" (controlFailure SilentNotify
    (controlled remoteParent remoteNotify {callFirst=41})==Left BankWrites)
  assert "enclosing bank failure cannot hide a corrupted snapshot" (controlFailure SilentNotify (controlled corruptedParent remoteNotify)==Left BankWrites)
  assert "enclosing bank failure cannot hide wrong completion" (controlFailure SilentNotify (controlled unfinishedParent remoteNotify)==Left BankWrites)
  assert "wrong predicate causes the modeled local save instead of the remote return" (checkCall wrongParent==Left ProcessorState
    && controlFailure WrongPredicate (wrongControlled wrongParent wrongNotify)==Right ())
  assert "wrong predicate does not excuse an omitted local save" (controlFailure WrongPredicate
    (wrongControlled otherParent wrongNotify)==Left ReturnValue)
  assert "wrong predicate does not excuse a returned IRQ mask" (controlFailure WrongPredicate
    (wrongControlled (changeFinal (\state -> state {D.processorState=0xd3}) wrongParent) wrongNotify)==Left ReturnValue)
  assert "wrong predicate does not excuse a changed caller register" (controlFailure WrongPredicate
    (wrongControlled (changeFinal changeR4 wrongParent) wrongNotify)==Left ReturnValue)
  assert "wrong predicate still requires the saved 36-byte frame" (controlFailure WrongPredicate
    (wrongControlled wrongFrame wrongNotify)==Left ReturnValue)
  assert "wrong predicate still requires the wake dispatch reason" (controlFailure WrongPredicate
    (wrongControlled wrongReason wrongNotify)==Left ReturnValue)
  assert "wrong predicate does not excuse unrelated writes" (controlFailure WrongPredicate
    (wrongControlled unrelatedWrites wrongNotify)==Left ReturnValue)
  assert "wrong predicate still requires the defective callee's preserved control state" (controlFailure WrongPredicate
    (wrongControlled wrongParent (changeFinal (\state -> state {D.processorState=0x53}) wrongNotify))==Left ReturnValue)
  assert "wrong predicate does not excuse a corrupted final snapshot" (controlFailure WrongPredicate
    (wrongControlled wrongParent {callAfter=M.insert (Address 0x5000c) 7 (callAfter wrongParent)} wrongNotify)==Left ReturnValue)
  assert "wrong predicate must produce the frozen mutant's return value" (controlFailure WrongPredicate
    (wrongControlled wrongParent wrongNotify {callR0=2})==Left ReturnValue)
  assert "wrong predicate does not excuse an independent earlier failure" (controlFailure WrongPredicate
    (wrongControlled wrongParent wrongNotify) {obsCalls=[wrongParent,independent,wrongNotify]}==Left ReturnValue)
  assert "enclosing service must miss the target notify transaction" (controlFailure SilentNotify
    (controlled otherParent otherNotify {callArguments=Arguments [3,0,0,0]})==Left BankWrites)
  assert "enclosing service must have the same actual task" (controlFailure SilentNotify
    (controlled remoteParent remoteNotify {callTask=0xa000})==Left BankWrites)
  assert "enclosing service must have the same kernel context" (controlFailure SilentNotify
    (controlled remoteParent remoteNotify {callKernelContext=1})==Left BankWrites)
  assert "independent earlier failures still refuse" (controlFailure SilentNotify
    (controlled remoteParent remoteNotify) {obsCalls=[remoteParent,independent,remoteNotify]}==Left BankWrites)
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
  assert "dispatch without boundary evidence refuses" (checkCall call {callSemantic=DispatchSemantic Save,callTrace=Nothing} == Left MissingTrace)
  let demandContext pc sp lr cpsr clock = DemandContext 0 pc sp lr cpsr ([0..12]++[sp,lr,pc]) clock 0x4005ffb4
      loadContext = demandContext 0x43758 0xa42200 0x453f0 0x53 10
      abortContext = demandContext 0xc 0x70000c00 0x1215484 0x1d7 11
      serviceContext = demandContext 0x45e54 0xa421b0 0x1234560 0x53 20
      demand = DemandPaging loadContext 0x1215480 7 abortContext 8 0x53 0x1215480 13
        (Just (DemandService serviceContext 0 (Address 0x4004db08) 0x45e54))
      mutations =
        [ \d -> d {demandLoad=(demandLoad d) {demandCore=1}}
        , \d -> d {demandAbort=(demandAbort d) {demandTask=1}}
        , \d -> d {demandLoad=(demandLoad d) {demandPc=0x43754}}
        , \d -> d {demandAbort=(demandAbort d) {demandPc=0x10}}
        , \d -> d {demandAbort=(demandAbort d) {demandCpsr=0x1d3}}
        , \d -> d {demandAbortCount=9}
        , \d -> d {demandSpsr=0xd3}
        , \d -> d {demandIfsr=5}
        , \d -> d {demandIfar=0x1215484}
        , \d -> d {demandAbort=(demandAbort d) {demandLr=0x1215488}}
        , \d -> d {demandAbort=(demandAbort d) {demandRegisters=replicate 16 0}}
        , \d -> d {demandService=Nothing}
        , \d -> d {demandService=Just (DemandService serviceContext 1 (Address 0x4004db08) 0x45e54)}
        , \d -> d {demandService=Just (DemandService serviceContext {demandTask=1} 0 (Address 0x4004db08) 0x45e54)}
        , \d -> d {demandService=Just (DemandService serviceContext {demandInstructions=10} 0 (Address 0x4004db08) 0x45e54)}
        , \d -> d {demandService=Just (DemandService serviceContext 0 (Address 0x400ddb08) 0x45e54)}
        , \d -> d {demandService=Just (DemandService serviceContext 0 (Address 0x4004db08) 0x45e58)}
        ]
  assert "permission-fault transfer with subsequent task service qualifies" (checkDemandPaging (Just 0x43758) demand==Right ())
  forM_ mutations $ \mutate -> assert "unproven permission-fault transfer refuses" (checkDemandPaging (Just 0x43758) (mutate demand)==Left DemandPagingState)
  assert "a demand load away from the plan's stop refuses" (checkDemandPaging (Just 0x43e04) demand==Left DemandPagingState)
  assert "a plan without a derived stop refuses every demand" (checkDemandPaging Nothing demand==Left DemandPagingState)
  refuses "unmatched demand-paging receipt" DemandPagingState observation {obsDemandPaging=[demand]}
  let witness proof =
        let loaded=demandLoad proof
            aborted=demandAbort proof
            raw=D.Registers (demandRegisters aborted) (demandCpsr aborted) 0 0 0
            attempted=D.Registers (take 15 (demandRegisters loaded)++[demandTarget proof]) (demandSpsr proof) 0 0 0
            boundary=Boundary PrefetchAbortBoundary (Address 0xc) (demandInstructions aborted) (demandCore aborted)
              raw M.empty [] [] (Just (demandTarget proof)) (demandTask aborted) [] [] S.empty
        in call {callSemantic=DispatchSemantic FirstEntry,callCore=demandCore loaded,callTask=demandTask loaded,
          callTrace=Just (CallTrace attempted attempted Dispatched [boundary] Nothing)}
      later=demand {demandLoad=loadContext {demandInstructions=30},demandLoadCount=17,
        demandAbort=abortContext {demandInstructions=31},demandAbortCount=18,
        demandService=Just (DemandService serviceContext {demandInstructions=40} 0 (Address 0x4004db08) 0x45e54)}
      paired=observation {obsCalls=[witness demand,witness later],obsDemandPaging=[demand,later]}
      demandPlan = plan {planStops=[Stop DomainA (Address 0x43758) FirstEntryStop,Stop DomainB (Address 0xd3758) FirstEntryStop]}
  assert "each distinct attempted transfer has its own complete proof" (checkDemandCalls demandPlan paired==Right ())
  assert "duplicate proof cannot cover another unproved abort" (checkDemandCalls demandPlan paired
    {obsDemandPaging=[demand,demand]}==Left DemandPagingState)
  assert "a legacy owned plan accepts the proof at the retained trampoline stop"
    (checkDemandCalls plan paired==Right () && demandStop plan 0==Just 0x43758 && demandStop plan 1==Just 0xd3758)
  assert "a plan that records a dropped stop refuses the proof"
    (checkDemandCalls plan {planDroppedStops=[DroppedStop DomainA "first-entry" "the owned symbol is not bound"]} paired==Left DemandPagingState)
  let firstCatalog = Catalog 0xe7f000f0 [Entry DomainA (HelperSemantic Notify) (Address 0x42000),Entry DomainB (HelperSemantic Notify) (Address 0xd2000)]
        [Window DomainA (Address 0x42000) (Address 0x42010),Window DomainB (Address 0xd2000) (Address 0xd2010)] []
      secondCatalog = Catalog 0xe7f000f0 [Entry DomainA (DispatchSemantic Save) (Address 0x42100)]
        [Window DomainA (Address 0x42100) (Address 0x42110)] [Probe DomainA (Address 0x42104) SelectedRead (Register 1)]
      catalogPlan = derivePlan Owned B.empty [firstCatalog,secondCatalog] (M.fromList [(DomainA,B.replicate 0xdae0 0),(DomainB,B.replicate 0xdaa0 0)])
  assert "catalog list preserves actual entry addresses and semantic tags" (case catalogPlan of
    Right planned -> planEntries planned==catalogEntries firstCatalog++catalogEntries secondCatalog && planProbes planned==catalogProbes secondCatalog && length (planGuards planned)==3 && planArmingEntries planned==retainedArmingEntries
    Left _ -> False)
  let boundaryEntries =
        [Entry d semantic (Address (base d+offset-(if d==DomainB && serviceEntry then 64 else 0)))
        | d <- [DomainA,DomainB]
        , (semantic,offset,serviceEntry) <- [(DispatchSemantic Select,0x36a4,False),(ServiceSemantic DisableDispatch,0xc9dc,True),
            (ServiceSemantic SleepTask,0x5cd4,True),(ServiceSemantic SuspendTask,0x6a00,True),(ServiceSemantic EnableTaskException,0x7428,True),
            (ServiceSemantic DeliverTaskException,0x41b4,True)]]
      boundaryCatalog = Catalog 0xe7f000f0 boundaryEntries
        [Window d (Address (base d+0x3000)) (Address (base d+0xcbd8)) | d <- [DomainA,DomainB]] []
      putWord :: Word32 -> B.ByteString
      putWord value = B.pack [fromIntegral (value `shiftR` shift) | shift <- [0,8,16,24]]
      loaderWith domain patches = let size = if domain==DomainA then 0xdae0 else 0xdaa0
                                      blank = B.replicate size 0
                                      patch bytes (address,values) = let offset = fromIntegral (address - base domain)
                                                                   in B.take offset bytes <> B.concat (map putWord values) <> B.drop (offset + 4*length values) bytes
                                  in foldl patch blank patches
      stockLoaders = M.fromList [(d, loaderWith d [((base d+0x4218-(if d==DomainB then 64 else 0)),[0xe12fff33])]) | d <- [DomainA,DomainB]]
      controlLoaders = M.fromList [(d, loaderWith d [((base d+0x4204-(if d==DomainB then 64 else 0)),[0xe12fff33])]) | d <- [DomainA,DomainB]]
      bodyA = 0x45000
      bodyB = 0xd5000
      firstA = 0x46000
      firstB = 0xd6000
      callbackBody :: [Word32]
      callbackBody = [0xe590201c,0xe5901004,0xe1a00004,0xe12fff32,0xe10f0000]
      firstBody :: [Word32]
      firstBody = [0xe5912008,0xe5920004,0xe592f008]
      ownedLoaders = M.fromList
        [(DomainA, loaderWith DomainA [(bodyA,callbackBody),(firstA,firstBody)])
        ,(DomainB, loaderWith DomainB [(bodyB,callbackBody),(firstB,firstBody)])]
      ownedAggregate = Aggregate
        (M.fromList [(DomainA, M.fromList [("s3c_exception_loop",Body bodyA (fromIntegral (4*length callbackBody))),("xm1_rtos_scheduler_first_entry",Body firstA (fromIntegral (4*length firstBody)))])
                    ,(DomainB, M.fromList [("s3c_exception_loop",Body bodyB (fromIntegral (4*length callbackBody))),("xm1_rtos_scheduler_first_entry",Body firstB (fromIntegral (4*length firstBody)))])])
        (M.fromList [((DomainA,0x441b4),"s3c_exception_loop"),((DomainA,0x43728),"xm1_rtos_scheduler_first_entry")
                    ,((DomainB,0xd4174),"s3c_exception_loop"),((DomainB,0xd3728),"xm1_rtos_scheduler_first_entry")])
      boundaryWithFirst = boundaryCatalog { catalogEntries = boundaryEntries ++
        [Entry d (DispatchSemantic FirstEntry) (Address (base d+0x3728)) | d <- [DomainA,DomainB]] }
      probeIn planned d pc kind register = Probe d (Address pc) kind (Register register) `elem` planProbes planned
  case derivePlan Stock B.empty [boundaryWithFirst] stockLoaders of
    Left failure -> error failure
    Right planned -> do
      forM_ [(DomainA,0x40000,0),(DomainB,0xd0000,64)] $ \(d,offset,delta) -> do
        assert "selected load probe follows the stock load" (probeIn planned d (offset+0x36b4) SelectedRead 1)
        assert "IRQ enabled loaded value is captured" (probeIn planned d (offset+0x376c) EnabledRead 0)
        assert "stock global lock probe reads committed memory" (probeIn planned d (offset+0x371c) GlobalLockRead 0)
        assert "stock IRQ selected/discarded probes retain ordering"
          (probeIn planned d (offset+0x377c) SelectedRead 0 && probeIn planned d (offset+0x3788) DiscardedRead 0)
        assert "save discard probe is absent in stock" (not (probeIn planned d (offset+0x3650) DiscardedRead 3))
        forM_ ([(0xca78,0xca4c),(0x5d9c,0x5d60),(0x6b60,0x6b10),(0x74c8,0x749c)] :: [(Word32,Word32)]) $ \(stock,_) ->
          assert "ready recheck retry is observed after unmask" (probeIn planned d (offset+stock-delta) Retry 0)
        let callback=offset+0x4218-delta
        assert "stock exception callback and return are paired" (probeIn planned d callback Callback 3 && probeIn planned d (callback+4) CallbackReturn 0)
      assert "stock select entry follows stock no-save prologue"
        (all (\entry -> entrySemantic entry/=DispatchSemantic Select || entryAddress entry==Address (base (entryDomain entry)+0x36ac)) (planEntries planned))
      assert "the stock plan arms no derived stop" (null (planStops planned) && null (planDroppedStops planned))
  case derivePlanWith ownedAggregate Owned B.empty [boundaryWithFirst] ownedLoaders of
    Left failure -> error failure
    Right planned -> do
      assert "the owned stop is the owned commit instruction"
        (planStops planned == [Stop DomainA (Address (firstA+8)) FirstEntryStop,Stop DomainB (Address (firstB+8)) FirstEntryStop])
      assert "the owned stop is not the stock trampoline"
        (all (\(Stop _ (Address address) _) -> address `notElem` [0x43758,0xd3758]) (planStops planned))
      forM_ [(DomainA,bodyA,0),(DomainB,bodyB,64)] $ \(d,body,delta) -> do
        assert "owned exception callback and return follow the body's handler call"
          (probeIn planned d (body+12) Callback 2 && probeIn planned d (body+16) CallbackReturn 0)
        assert "owned retry and dispatch probes are unchanged"
          (probeIn planned d (base d+0x36ac) SelectedRead 1 && probeIn planned d (base d+0xca4c-delta) Retry 0)
        let inside address = any (\(Guard gd (Address lo) (Address hi) _) -> gd==d && lo<=address && address<hi) (planGuards planned)
        assert "every owned stop and probe lies inside a guarded extent"
          (all (\(Stop sd (Address address) _) -> sd/=d || inside address) (planStops planned)
           && all (\probe -> probeDomain probe/=d || inside (let Address address=probeAddress probe in address)) (planProbes planned))
      assert "owned plan carries no dropped stop when both symbols are bound" (null (planDroppedStops planned))
  assert "an unbound owned first-entry symbol drops its stop and lists it"
    (case derivePlanWith emptyAggregate Owned B.empty [boundaryCatalog { catalogEntries = [Entry d (DispatchSemantic FirstEntry) (Address (base d+0x3728)) | d <- [DomainA,DomainB]] }] ownedLoaders of
      Left _ -> False
      Right planned -> null (planStops planned) && map droppedName (planDroppedStops planned) == ["first-entry","first-entry"])
  assert "callback probes refuse a body without the handler call"
    (case derivePlanWith ownedAggregate Owned B.empty [boundaryCatalog] stockLoaders of Left _ -> True; Right _ -> False)
  case derivePlan Restored B.empty [boundaryWithFirst] controlLoaders of
    Left failure -> error failure
    Right planned -> do
      assert "a control plan without an aggregate takes the retained trampoline stop"
        (planStops planned == [Stop DomainA (Address 0x43758) FirstEntryStop,Stop DomainB (Address 0xd3758) FirstEntryStop]
         && null (planDroppedStops planned))
      forM_ [(DomainA,0),(DomainB,64)] $ \(d,delta) ->
        assert "a control plan without an aggregate probes the s3c callback at its retained offset"
          (probeIn planned d (base d+0x4204-delta) Callback 3 && probeIn planned d (base d+0x4208-delta) CallbackReturn 0)
  assert "a stock plan carries no derived stops on the wire"
    (case planValue plan {planRole=Stock} of J.Object fields -> K.lookup "stops" fields == Nothing && K.lookup "dropped_stops" fields == Nothing; _ -> False)
  assert "an owned plan carries its derived stops on the wire"
    (case planValue plan of J.Object fields -> K.lookup "stops" fields /= Nothing && K.lookup "dropped_stops" fields /= Nothing; _ -> False)
  putStrLn "scheduler-pair host tests passed"

wireObservation :: J.Value
wireObservation = J.object
  [ "schema" J..= (3::Int), "role" J..= roleName (obsRole observation)
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
  , "demand_paging" J..= ([]::[J.Value])
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
      "core" J..= core (callCore value),"cpsr" J..= callCpsr value,"arguments" J..= arguments (callArguments value),"memory" J..= wordsOf (callBefore value),
      "registers" J..= replicate (16::Int) (0::Int),"cpacr" J..= (0::Int),"fpexc" J..= (0::Int),"tpidrurw" J..= (0::Int),
      "task" J..= J.object ["view" J..= ("committed"::String),"value" J..= callTask value],
      "kernel_context" J..= callKernelContext value,"task_stack" J..= J.Null],
    "result" J..= J.object ["r0" J..= callR0 value,"cpsr" J..= callReturnCpsr value,"memory" J..= wordsOf (callAfter value),
      "registers" J..= replicate (16::Int) (0::Int),"cpacr" J..= (0::Int),"fpexc" J..= (0::Int),"tpidrurw" J..= (0::Int),"completion" J..= ("return"::String),"history" J..= ([]::[J.Value]),
      "writes" J..= map writeOf (zip (callWrites value) (callWriteOrigins value)),"bank" J..= map writeOf (zip (callBank value) (callBankOrigins value))],"count" J..= callCount value,"first" J..= callFirst value]
  arguments (Arguments values) = values
  wordsOf memory = [J.object ["address" J..= hex address,"value" J..= J.object ["view" J..= ("committed"::String),"value" J..= value]] | (Address address,value) <- M.toList memory]
  writeOf (Written (Address address) size value,origin) = J.object ["address" J..= hex address,"size" J..= size,"value" J..= value,
    "origin" J..= J.object ["core" J..= core (writerCore origin),"pc" J..= addressOf (writerPc origin),"sp" J..= addressOf (writerSp origin),
      "task" J..= addressOf (writerTask origin),"cpsr" J..= writerCpsr origin,"kernel_context" J..= writerContext origin,"instructions" J..= writerInstructions origin]]
  addressOf (Address address) = hex address
