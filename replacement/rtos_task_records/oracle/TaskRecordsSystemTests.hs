module Main (main) where

import Control.Monad (forM_, unless)
import qualified Data.Map.Strict as M
import qualified Data.Set as S
import Data.Word (Word32)
import qualified Emulation.SchedulerPairCheck as C
import Emulation.SchedulerPairModel
import Emulation.SchedulerPairPlan
import qualified Emulation.SchedulerPairS2 as D
import qualified Emulation.SchedulerPairTaskRecords as T
import Replacement.RtosTaskRecords.SystemCheck
import Replacement.RtosOwnFirst.Regions (Domain (..), domains)

require :: Bool -> String -> IO ()
require holds label=unless holds (fail label)

sp,request,definition,task,current,callerDefinition :: Word32
sp=0x80001000
request=sp+32
definition=0x4005beac+6*0x28
task=0x4005ff0c+6*0x38
current=0x4005ff0c+0x38
callerDefinition=0x4005beac+0x28

before :: Memory
before=M.fromList ([(Address address,0) | domain<-domains,let (definitions,tasks,ids,_)=T.recordAddresses domain,(start,bytes)<-[(definitions,120*0x28),(tasks,120*0x38),(ids,120*4)],address<-[start,start+4..start+bytes-4]]
  ++[(Address address,0x87654321) | address<-[sp-48,sp-44..sp+20]]
  ++[(Address (request+offset),value) | (offset,value)<-zip [0,4..20] [0x11,0x22,0x40001000,16,0x101,0x80100000]]
  ++[(Address (task+8),definition),(Address (task+12),0xabffee00),(Address (task+24),0x80201000),(Address (task+28),0x43668),(Address (task+52),9)
    ,(Address (current+8),callerDefinition),(Address (callerDefinition+16),0x2000),(Address (callerDefinition+20),0x80000000),(Address 0x400c0804,0x21)])

stores :: [C.Written]
stores=[C.Written (Address (definition+offset)) 4 value | (offset,value)<-zip [0,4..32] [0x11,0x22,0x40001000,15,0x104,0x80100000,0,0,7]]
  ++[C.Written (Address (0x400622d4+6*4)) 4 7,C.Written (Address (task+14)) 1 0xf8,C.Written (Address (task+32)) 4 1]
  ++[C.Written (Address (task+offset)) 4 0 | offset<-[36,40,44,48]]
  ++[C.Written (Address (task+13)) 1 15,C.Written (Address (task+12)) 1 0,C.Written (Address (task+16)) 4 0,C.Written (Address (definition+36)) 4 1]

after :: Memory
after=foldr (uncurry M.insert) before ([(Address (definition+offset),value) | (offset,value)<-zip [0,4..36] [0x11,0x22,0x40001000,15,0x104,0x80100000,0,0,7,1]]
  ++[(Address (0x400622d4+6*4),7),(Address (task+12),0xabf80f00),(Address (task+16),0),(Address (task+32),1)]
  ++[(Address (task+offset),0) | offset<-[36,40,44,48]])

registers :: D.Registers
registers=D.Registers ([0x10007,request,0,0]++[4..12]++[sp,0x725fac,0x4d094]) 0x93 2 0xf00000 0
returnedRegisters :: D.Registers
returnedRegisters=registers {D.registers=0:take 14 (drop 1 (D.registers registers))++[0x725fac]}
origin :: C.WriteOrigin
origin=C.WriteOrigin 0 (Address 0x46204) (Address (sp-40)) 0xd3 (Address current) 0 10
boundary :: C.BoundaryKind -> Memory -> [C.Written] -> C.Boundary
boundary kind memory writes=C.Boundary kind (Address (if kind==C.ReturnBoundary then 0x725fac else 0x4c984)) (if kind==C.ReturnBoundary then 30 else 20) 0 (if kind==C.ReturnBoundary then returnedRegisters else registers) memory writes [] Nothing current (replicate (length writes) origin) [] S.empty

fixture :: C.Call
fixture=C.Call {C.callDomain=DomainA,C.callAddress=Address 0x4d094,C.callSemantic=TaskRecordSemantic CreateTaskRecord,C.callCore=0,C.callCpsr=0x93,C.callArguments=Arguments [0x10007,request,0,0],C.callBefore=before,C.callAfter=after,C.callR0=0,C.callReturnCpsr=0x93,C.callWrites=stores,C.callBank=[],C.callCount=1,C.callFirst=1
  ,C.callTrace=Just (C.CallTrace registers returnedRegisters C.Returned [boundary C.PublicationBarrierBoundary before (init stores),boundary C.ReturnBoundary after [last stores]] Nothing)
  ,C.callTask=current,C.callKernelContext=0,C.callTaskStack=Just (C.TaskStack (Address 0x80000000) (Address 0x80002000)),C.callWriteOrigins=replicate (length stores) origin,C.callBankOrigins=[],C.callUnreadable=S.empty}

clearFixture :: C.Call
clearFixture =
  let initial=M.insert (Address 0x400c0804) 0
        (M.insert (Address (callerDefinition+36)) 1 (M.insert (Address (current+12)) 1
          (foldr M.delete before [Address (request+offset) | offset<-[0,4..20]])))
      final=M.insert (Address (callerDefinition+36)) 0 initial
      arguments=[0x10002,0,0,0]
      entered=registers {D.registers=arguments++[4..12]++[sp,0x7261bc,0x4d1a4]}
      returned=entered {D.registers=0:take 14 (drop 1 (D.registers entered))++[0x7261bc]}
      writes=[C.Written (Address (callerDefinition+36)) 4 0]
      completed=C.Boundary C.ReturnBoundary (Address 0x7261bc) 30 0 returned final writes [] Nothing current [origin] [] S.empty
  in fixture {C.callAddress=Address 0x4d1a4,C.callSemantic=TaskRecordSemantic DeleteTaskRecord
    ,C.callArguments=Arguments arguments,C.callBefore=initial,C.callAfter=final,C.callWrites=writes
    ,C.callWriteOrigins=[origin],C.callTrace=Just (C.CallTrace entered returned C.Returned [completed] Nothing)}

resetFixture :: C.Call
resetFixture =
  let initial=C.callBefore clearFixture
      defaults=M.fromList [(Address (definition+offset),value) | (offset,value)<-zip [0,4..36] [0,6,0,15,4,0x40062c50,0,0,7,0]]
      writes=[C.Written address bytes (M.findWithDefault value address defaults) | C.Written address bytes value<-stores]
      final=foldr (uncurry M.insert) initial (M.toList defaults
        ++[(Address (0x400622d4+6*4),7),(Address (task+12),0xabf80f00),(Address (task+16),0),(Address (task+32),1)]
        ++[(Address (task+offset),0) | offset<-[36,40,44,48]])
      arguments=[0x10007,1,0,0]
      entered=registers {D.registers=arguments++[4..12]++[sp,0x726234,0x4d1a4]}
      returned=entered {D.registers=0:take 14 (drop 1 (D.registers entered))++[0x726234]}
      observed kind effects=C.Boundary kind (Address (if kind==C.ReturnBoundary then 0x726234 else 0x4c984))
        (if kind==C.ReturnBoundary then 30 else 20) 0 (if kind==C.ReturnBoundary then returned else entered)
        final effects [] Nothing current (replicate (length effects) origin) [] S.empty
  in clearFixture {C.callArguments=Arguments arguments,C.callBefore=initial,C.callAfter=final,C.callWrites=writes
    ,C.callWriteOrigins=replicate (length writes) origin
    ,C.callTrace=Just (C.CallTrace entered returned C.Returned
      [observed C.PublicationBarrierBoundary (init writes),observed C.ReturnBoundary [last writes]] Nothing)}

resetRefusalFixture :: C.Call
resetRefusalFixture =
  let initial=M.adjust (\word->word+1) (Address (task+12)) (C.callBefore clearFixture)
      arguments=[0x10007,1,0,0]
      entered=registers {D.registers=arguments++[4..12]++[sp,0x726234,0x4d1a4]}
      returned=entered {D.registers=0xffffffd7:take 14 (drop 1 (D.registers entered))++[0x726234]}
      completed=C.Boundary C.ReturnBoundary (Address 0x726234) 30 0 returned initial [] [] Nothing current [] [] S.empty
  in clearFixture {C.callArguments=Arguments arguments,C.callBefore=initial,C.callAfter=initial,C.callR0=0xffffffd7,C.callWrites=[]
    ,C.callWriteOrigins=[],C.callTrace=Just (C.CallTrace entered returned C.Returned [completed] Nothing)}

bootstrapFixture :: Address -> C.Call
bootstrapFixture (Address bootSp) =
  let bootRequest=bootSp+8
      oldWords=[Address address | address<-[sp-48,sp-44..sp+20]++[request,request+4..request+20]]
      incoming=M.fromList ([(Address address,0x87654321) | address<-[bootSp-48,bootSp-44..bootSp+20]]
        ++[(Address (bootRequest+offset),value) | (offset,value)<-zip [0,4..20] [0x11,0x22,0x40001000,16,0x101,0x80100000]])
      remap memory=M.union incoming (foldr M.delete memory oldWords)
      initial=remap before
      final=remap after
      arguments=[0x10007,bootRequest,0,0]
      entered=registers {D.registers=arguments++[4..12]++[bootSp,0x725fac,0x4d094]}
      returned=entered {D.registers=0:take 14 (drop 1 (D.registers entered))++[0x725fac]}
      writer=origin {C.writerSp=Address (bootSp-40),C.writerTask=Address 0,C.writerContext=1}
      observed kind memory writes=C.Boundary kind (Address (if kind==C.ReturnBoundary then 0x725fac else 0x4c984))
        (if kind==C.ReturnBoundary then 30 else 20) 0 (if kind==C.ReturnBoundary then returned else entered)
        memory writes [] Nothing 0 (replicate (length writes) writer) [] S.empty
  in fixture {C.callArguments=Arguments arguments,C.callBefore=initial,C.callAfter=final
    ,C.callTask=0,C.callKernelContext=1,C.callTaskStack=Nothing,C.callWriteOrigins=replicate (length stores) writer
    ,C.callTrace=Just (C.CallTrace entered returned C.Returned
      [observed C.PublicationBarrierBoundary initial (init stores),observed C.ReturnBoundary final [last stores]] Nothing)}

main :: IO ()
main=do
  case T.recordCall True DomainA CreateTaskRecord (C.callArguments fixture) before of
    Right result->require (expectedR0 result==0 && expectedMemory result==after) "source-grounded creation outputs"
    Left error'->fail (show error')
  require (C.checkCallFor Owned fixture==Right ()) "owned create consumer"
  let changeTrace f call=call {C.callTrace=f <$> C.callTrace call}
      changedRegister call=changeTrace (\trace->trace {C.traceAfter=(C.traceAfter trace) {D.registers=take 4 (D.registers registers)++[99]++drop 5 (D.registers registers)}}) call
      noBarrier=changeTrace (\trace->trace {C.traceHistory=filter ((/=C.PublicationBarrierBoundary) . C.boundaryKind) (C.traceHistory trace)}) fixture
      unknownWrite=fixture {C.callWrites=stores++[C.Written (Address 0x81234560) 4 1],C.callWriteOrigins=C.callWriteOrigins fixture++[origin]}
      withOwner value call =
        let changed=M.insert (Address 0x400c0804) value
        in call {C.callBefore=changed (C.callBefore call),C.callAfter=changed (C.callAfter call)
          ,C.callTrace=(\trace->trace {C.traceHistory=map (\value'->value' {C.boundaryMemory=changed (C.boundaryMemory value')}) (C.traceHistory trace)}) <$> C.callTrace call}
      onSecondary call=call {C.callCore=1,C.callWriteOrigins=map secondaryOrigin (C.callWriteOrigins call)
        ,C.callTrace=(\trace->trace {C.traceHistory=map (\value->value {C.boundaryCore=1,C.boundaryWriteOrigins=map secondaryOrigin (C.boundaryWriteOrigins value)}) (C.traceHistory trace)}) <$> C.callTrace call}
      secondaryOrigin value=value {C.writerCore=1}
      cases=[("status",fixture {C.callR0=1}),("callee register",changedRegister fixture),("saved resume",fixture {C.callAfter=M.insert (Address (task+28)) 0xdeadbeef after}),("publication",noBarrier),("outside effect",unknownWrite),("missing opposite table",fixture {C.callBefore=M.delete (Address 0x400ebe6c) before}),("missing stack contract",fixture {C.callTaskStack=Nothing}),("IRQ stack premise",fixture {C.callKernelContext=1}),("unheld wrapper lock",fixture {C.callBefore=M.insert (Address 0x400c0804) 0 before})]
  forM_ cases $ \(label,call)->require (C.checkCallFor Owned call/=Right ()) ("accepted corrupt "++label)
  forM_ [0,1,0x11,0x12,0x22,0x1021,0x80000021] $ \owner ->
    require (C.checkCallFor Owned (withOwner owner fixture)==Left C.MemoryResult) "foreign or invalid task-wrapper owner refuses"
  let otherCore=onSecondary (withOwner 0x22 fixture)
  require (C.checkCallFor Owned otherCore==Right ()) "domain A may execute on core1"
  require (C.checkCallFor Owned (withOwner 0x21 otherCore)==Left C.MemoryResult) "primary wrapper owner cannot authorize the other core"
  require (C.checkCallFor Owned clearFixture==Right ()) "current-task clear does not require a global wrapper owner"
  require (C.checkCallFor Stock clearFixture==Right ()) "stock current-task clear follows its caller contract"
  require (C.checkCallFor Owned (onSecondary clearFixture)==Left C.WrongCore) "delete uses the executing core domain"
  require (C.checkCallFor Owned resetRefusalFixture==Right ()) "reset refusal does not require a global wrapper owner"
  require (C.checkCallFor Stock resetFixture==Right ()) "stock reset permits an unpublished dormant definition"
  require (C.checkCallFor Owned resetFixture==Right ()) "owned reset retains the clear-then-reset caller contract"
  require (C.checkCallFor Owned resetFixture {C.callR0=0xffffffd7}/=Right ()) "unpublished reset cannot invent NotCreated"
  require (C.checkCallFor Owned resetFixture {C.callAfter=M.insert (Address (task+28)) 0xdeadbeef (C.callAfter resetFixture)}/=Right ()) "unpublished reset preserves the saved resume identity"
  let primaryBoot=bootstrapFixture (Address 0x400633e8)
      secondaryBoot=onSecondary (withOwner 0x22 (bootstrapFixture (Address 0x400f33a8)))
      irqRegisters state=state {D.processorState=0x92}
      irqOrigin value=value {C.writerCpsr=0xd2}
      irqBoot=primaryBoot {C.callCpsr=0x92,C.callReturnCpsr=0x92,C.callWriteOrigins=map irqOrigin (C.callWriteOrigins primaryBoot)
        ,C.callTrace=(\trace->trace {C.traceBefore=irqRegisters (C.traceBefore trace),C.traceAfter=irqRegisters (C.traceAfter trace)
          ,C.traceHistory=map (\value->value {C.boundaryRegisters=irqRegisters (C.boundaryRegisters value),C.boundaryWriteOrigins=map irqOrigin (C.boundaryWriteOrigins value)}) (C.traceHistory trace)}) <$> C.callTrace primaryBoot}
  require (C.checkCallFor Owned primaryBoot==Right ()) "primary source bootstrap allocation"
  require (C.checkCallFor Owned secondaryBoot==Right ()) "bootstrap backing follows the physical core rather than target domain"
  require (C.checkCallFor Owned (bootstrapFixture (Address (0x40062c54+52)))==Right ()) "exact bootstrap allocation lower boundary"
  require (C.checkCallFor Owned (bootstrapFixture (Address (0x40062c54+44)))==Left C.MissingTrace) "bootstrap frame below allocation refuses"
  require (C.checkCallFor Owned (bootstrapFixture (Address (0x40063460-16)))==Left C.MissingTrace) "bootstrap request above allocation refuses"
  require (C.checkCallFor Owned (primaryBoot {C.callKernelContext=2})==Left C.MissingTrace) "nested bootstrap IRQ is not a boot caller"
  require (C.checkCallFor Owned (primaryBoot {C.callKernelContext=0})==Left C.MissingTrace) "an absent bootstrap marker refuses"
  require (C.checkCallFor Owned (primaryBoot {C.callTaskStack=Just (C.TaskStack (Address 0x40062c54) (Address 0x40063460))})==Left C.MissingTrace) "boot backing is not a task allocation"
  require (C.checkCallFor Owned irqBoot==Left C.MissingTrace) "IRQ mode cannot borrow bootstrap backing"
  let busy=M.insert (Address (definition+36)) 1 before
  require (case T.recordCall True DomainA CreateTaskRecord (C.callArguments fixture) busy of Right result->expectedR0 result==0xffffffd7 && expectedMemory result==busy; _->False) "duplicate is effect free"
  let live=M.insert (Address (task+12)) 1 before
  require (case T.recordCall True DomainA CreateTaskRecord (C.callArguments fixture) live of Right result->expectedR0 result==0xffffffd7 && expectedMemory result==live; _->False) "live-record correction"
  require (case T.recordCall False DomainA CreateTaskRecord (C.callArguments fixture) live of Right result->expectedR0 result==0; _->False) "stock live-record counterexample"
  require (coverage []==Left (MissingLifecycle DomainA Created)) "unobserved lifecycle refuses"
  let lifecycle domain=[fixture {C.callDomain=domain,C.callFirst=clock,C.callSemantic=TaskRecordSemantic point,C.callArguments=Arguments [7,if point==CreateTaskRecord then request else mode,0,0]} | (clock,point,mode)<-[(1,CreateTaskRecord,0),(2,DeleteTaskRecord,1),(3,CreateTaskRecord,0),(4,DeleteTaskRecord,0)]]
  require (coverage (lifecycle DomainA++lifecycle DomainB)==Right ()) "both-domain complete lifecycle"
  require (coverage (lifecycle DomainA)==Left (MissingLifecycle DomainB Created)) "one-domain coverage refuses"
  require (coverage (map (\call->call {C.callCount=2}) (lifecycle DomainA))==Left RepeatedInvocation) "aggregated lifecycle order refuses"
  putStrLn "task-record system source, effect, context, publication and lifecycle controls pass"
