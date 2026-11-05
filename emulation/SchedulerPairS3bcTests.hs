module Main (main) where

import Control.Monad (forM_, unless)
import Data.Bits ((.&.), (.|.), shiftL)
import qualified Data.Map.Strict as M
import qualified Data.Set as S
import Data.Word (Word32)
import Emulation.SchedulerPairModel
import Emulation.SchedulerPairPlan (Address (..), DispatchPoint (Save), Service (..))
import Emulation.SchedulerPairService
import qualified Emulation.SchedulerPairS3b as B
import qualified Emulation.SchedulerPairS3c as C
import Replacement.RtosOwnFirst.Regions (Domain (..))

assert :: String -> Bool -> IO ()
assert label valid = unless valid (fail label)
put :: Word32 -> Word32 -> Memory -> Memory
put at value = M.insert (Address at) value
get :: Word32 -> Memory -> Maybe Word32
get at = M.lookup (Address at)
args :: Word32 -> Word32 -> Arguments
args a b = Arguments [a,b,0,0]
owner :: Domain -> Word32
owner DomainA = 1
owner DomainB = 2
taskAt, idleAt, definitionAt, headsAt, lockAt :: Word32 -> Word32
taskAt cls = 0x10000+cls*0x1000
idleAt cls = taskAt cls+0x38
definitionAt cls = 0x60000+cls*0x100
headsAt cls = 0x30000+cls*0x100
lockAt cls = 0x50000+cls*4
recordAt :: Domain -> Word32 -> Word32
recordAt domain cls = contextTable (domainWords domain)+(cls-1)*0x84

fixture :: Domain -> Memory
fixture domain = M.fromList (globals++concatMap record [1,2])
 where
  words' = domainWords domain
  own = owner domain
  globals = [(Address (contextWord words'),0),(Address (currentContext words'),recordAt domain own),
    (Address (runningCell words'),idleAt own),(Address (rescheduleCell words'),0),
    (Address (dispatchCell words'),1),(Address (selectedCell words'),idleAt own),(Address (sleepResults words'),0)]
  record cls =
    let r=recordAt domain cls; t=taskAt cls; idle=idleAt cls; h=headsAt cls; c=definitionAt cls
        bitmap=0x51000+cls*16; selected=bitmap+4; enabled=bitmap+8
        cells=[(r,if cls==own then 0 else cls),(r+8,lockAt cls),(r+0x20,h),(r+0x24,bitmap),(r+0x28,runningCell words'),
          (r+0x2c,selected),(r+0x30,enabled),(r+0x44,t),(r+0x48,120),(r+0x7c,if cls==1 then 0x43728 else 0xd3728),
          (lockAt cls,0),(bitmap,0x8000),(selected,idle),(enabled,1),(t+8,c),(t+12,4),(t+0x14,0x70000+cls*4),
          (idle,h+120),(idle+4,h+120),(idle+8,c),(idle+12,0xf01),(h+120,idle),(h+124,idle),
          (c+4,0x1234),(c+0x10,0x100),(c+0x14,0x80000),(c+0x1c,0x90000),(c+0x20,1),(c+0x24,1),
          (0x70000+cls*4,0)]
        zeros=[(at,0) | at <- [r,r+4..r+0x80]++[t,t+4..t+0x34]++[idle,idle+4..idle+0x34]++[c,c+4..c+0x24]]
        heads=[(h+p*8+offset,h+p*8) | p<-[0..15],offset<-[0,4]]
     in [(Address at,value) | (at,value)<-zeros++heads++cells]

withFlags :: Word32 -> Word32 -> Memory -> Memory
withFlags task flags memory = case get (task+12) memory of
  Just packed -> put (task+12) ((packed .&. 0xff00ffff) .|. (flags `shiftL` 16)) memory
  Nothing -> memory
withState :: Word32 -> Word32 -> Memory -> Memory
withState task state memory = case get (task+12) memory of
  Just packed -> put (task+12) ((packed .&. 0xffffff00) .|. state) memory
  Nothing -> memory
ready :: Domain -> Memory -> Memory
ready domain memory = foldr (uncurry put) memory
  [(t+12,1),(t,h),(t+4,h),(h,t),(h+4,t),(bitmap,0x8001),(bitmap+4,t)]
 where cls=owner domain; t=taskAt cls; h=headsAt cls; bitmap=0x51000+cls*16

expect :: String -> Either ModelFailure ServiceExpected -> ServiceCompletion -> Maybe Word32 -> [(Word32,Word32)] -> IO ServiceExpected
expect label outcome completion value memory = case outcome of
  Left failure -> fail (label++": "++show failure)
  Right predicted -> do
    assert (label++" completion") (serviceCompletion predicted==completion)
    assert (label++" result") (serviceR0 predicted==value)
    assert (label++" state") (all (\(at,v)->get at (expectedMemory (serviceExpected predicted))==Just v) memory)
    pure predicted

main :: IO ()
main = do
  forM_ [DomainA,DomainB] $ \domain -> do
    let memory=fixture domain; cls=owner domain; task=taskAt cls; idle=idleAt cls; words'=domainWords domain
        r=recordAt domain cls; h=headsAt cls; handle=cls*0x10000+1
        b service mem=B.serviceCall domain service 0x53 (args handle 0) mem
        c service argument mem=C.serviceCall domain service 0x53 (args handle argument) mem
        negative n=Just (fromIntegral (n::Int))
    _ <- expect "cancel activation consumes one request" (b CancelActivation (withFlags task 0xf9 memory)) ServiceReturn (Just 1) [(task+12,0xf80004)]
    _ <- expect "cancel wakeup preserves activation flag" (b CancelWakeup (withFlags task 3 memory)) ServiceReturn (Just 1) [(task+12,0x10004)]
    _ <- expect "activation prepares and dispatches" (b ActivateTask (withState task 0 memory)) (ServiceDispatch Save (fromIntegral (-7::Int))) (negative (-7)) [(task+12,1),(task+0x18,0x80100),(task+0x1c,if cls==1 then 0x43728 else 0xd3728),(lockAt cls,0)]
    _ <- expect "handler activation requests IRQ-exit reschedule" (b InterruptActivateTask (put (contextWord words') 1 (withState task 0 memory))) ServiceReturn (Just 0) [(rescheduleCell words',1),(task+12,1)]
    _ <- expect "sleep wake releases wait and dispatches" (b WakeTask (withState task 10 memory)) (ServiceDispatch Save (fromIntegral (-19::Int))) (negative (-19)) [(task+12,1),(task+0x2c,0)]
    _ <- expect "handler sleep wake requests reschedule" (b InterruptWakeTask (put (contextWord words') 1 (withState task 10 memory))) ServiceReturn (Just 0) [(rescheduleCell words',1),(task+12,1)]
    _ <- expect "activation overflow refuses without changing flags" (b ActivateTask (withFlags task 0xff memory)) ServiceReturn (negative (-43)) [(task+12,0xff0004)]
    _ <- expect "handler zero task refuses" (B.serviceCall domain InterruptWakeTask 0x53 (args 0 0) (put (contextWord words') 1 memory)) ServiceReturn (negative (-18)) []
    _ <- expect "priority writes caller output" (c GetTaskPriority 0x80000 (put 0x80000 99 memory)) ServiceReturn (Just 0) [(0x80000,1)]
    _ <- expect "sleep consumes queued wakeup" (C.serviceCall domain SleepTask 0x53 (args 0 0) (withFlags idle 2 memory)) ServiceReturn (Just 0) [(idle+12,0xf01)]
    let runningReady=put (runningCell words') task (ready domain memory)
    _ <- expect "sleep publishes wait result before switch" (C.serviceCall domain SleepTask 0x53 (args 0 0) runningReady) (ServiceDispatch Save (fromIntegral (-17::Int))) (negative (-17)) [(task+12,10),(task+0x14,sleepResults words'),(sleepResults words',0),(h,h)]
    _ <- expect "self recheck requests retry when ready bit cleared" (C.serviceCall domain SleepTask 0x53 (args 0 0) (put (runningCell words') task memory)) ServiceRetry Nothing [(lockAt cls,0)]
    _ <- expect "suspend removes ready task" (c SuspendTask 0 (ready domain memory)) (ServiceDispatch Save (fromIntegral (-22::Int))) (negative (-22)) [(task+12,4),(h,h)]
    forM_ [ResumeTask,ForceResumeTask] $ \service -> do
      _ <- expect "resume inserts suspended task" (c service 0 memory) (ServiceDispatch Save (fromIntegral (-23::Int))) (negative (-23)) [(task+12,1)]
      pure ()
    _ <- expect "raise queues pending pattern" (c RaiseTaskException 2 memory) ServiceReturn (Just 0) [(task+16,2)]
    _ <- expect "handler raise requests delivery at IRQ exit" (c InterruptRaiseTaskException 2 (put (contextWord words') 1 (withFlags task 4 runningReady))) ServiceReturn (Just 0) [(task+16,2),(rescheduleCell words',1)]
    _ <- expect "disable exception clears enable bit" (c DisableTaskException 0 (withFlags idle 7 memory)) ServiceReturn (Just 0) [(idle+12,0x30f01)]
    _ <- expect "enable exception preserves wakeup flags" (c EnableTaskException 0 (withFlags idle 3 memory)) ServiceReturn (Just 0) [(idle+12,0x70f01)]
    let delivery=withFlags idle 4 (put (idle+16) 3 memory)
    forM_ [DeliverTaskException,DeliverTaskExceptionEntry] $ \service -> do
      predicted <- expect "delivery releases lock before callback" (C.serviceCall domain service 0xd3 (args handle 0) (put (lockAt cls) (if service==DeliverTaskException then 1 else 0) delivery)) (ServiceCallback 0x90000 [3,0x1234]) Nothing [(idle+16,0),(idle+12,0xf01),(lockAt cls,0)]
      assert "callback has IRQ enabled" (serviceCpsr predicted .&. 0x80 == 0)
    _ <- expect "raise on running enabled task enters delivery" (c RaiseTaskException 2 (withFlags task 4 runningReady)) (ServiceCallback 0x90000 [2,0x1234]) Nothing [(task+16,0),(task+12,1)]
    _ <- expect "enable with pending enters delivery" (c EnableTaskException 0 (put (idle+16) 3 memory)) (ServiceCallback 0x90000 [3,0x1234]) Nothing [(idle+16,0)]
    _ <- expect "callback return restores enable under lock" (C.resumeService domain DeliverTaskException 0xd3 (args 0 0) memory) ServiceReturn Nothing [(idle+12,0x40f01),(lockAt cls,1)]
    _ <- expect "callback injected exception causes another callback" (C.resumeService domain DeliverTaskException 0xd3 (args 0 0) (put (idle+16) 9 memory)) (ServiceCallback 0x90000 [9,0x1234]) Nothing [(idle+16,0),(lockAt cls,0)]
    _ <- expect "resumed sleep reads actual wake result" (C.resumeService domain SleepTask 0xd3 (args 0 0) (put (idle+0x14) 0x80000 (put 0x80000 (fromIntegral (-50::Int)) memory))) ServiceReturn (negative (-50)) []
    forM_ [GetTaskPriority,SuspendTask,ResumeTask,ForceResumeTask,RaiseTaskException,InterruptRaiseTaskException,DisableTaskException,EnableTaskException] $ \service -> do
      _ <- expect "wrong execution context refuses" (c service 2 (put (contextWord words') (if service==InterruptRaiseTaskException then 0 else 1) memory)) ServiceReturn (negative (-25)) []
      pure ()
    forM_ [B.serviceCall domain ActivateTask,C.serviceCall domain GetTaskPriority] $ \model ->
      assert "unreadable entry state refuses" (case model 0x53 (args handle 0) M.empty of Left (MissingWord _) -> True; _ -> False)
    let remoteClass=if cls==1 then 2 else 1; otherTask=taskAt remoteClass
    remote <- expect "foreign activation rings target bank without local dispatch" (B.serviceCall domain ActivateTask 0x53 (args (remoteClass*0x10000+1) 0) (withState otherTask 0 memory)) ServiceReturn (Just 0) [(otherTask+12,1),(rescheduleCell words',0)]
    assert "foreign activation has required bank effect" (expectedBank (serviceExpected remote)==[(Address (if remoteClass==1 then 0xfffb30cc else 0xfffb40cc),0x1000000)])
    assert "activation records task publication writes" (S.member (Address (otherTask+0x18)) (expectedWrites (serviceExpected remote)))
    _ <- expect "suspended sleeping task remains suspended" (b WakeTask (withState task 14 memory)) ServiceReturn (Just 0) [(task+12,4)]
    _ <- expect "dormant priority query refuses" (c GetTaskPriority 0x80000 (withState task 0 memory)) ServiceReturn (negative (-41)) []
    _ <- expect "zero exception pattern refuses" (c RaiseTaskException 0 memory) ServiceReturn (negative (-17)) []
    _ <- expect "missing exception handler refuses" (c EnableTaskException 0 (put (definitionAt cls+0x1c) 0 memory)) ServiceReturn (negative (-41)) []
    _ <- expect "dispatch-disabled sleep refuses" (c SleepTask 0 (put (dispatchCell words') 0 memory)) ServiceReturn (negative (-25)) []
    forM_ [InterruptActivateTask,ActivateTask,CancelActivation,WakeTask,InterruptWakeTask,CancelWakeup] $ \service -> do
      refused <- expect "masked scheduler service refuses without effects" (B.serviceCall domain service 0xd3 (args handle 0) memory) ServiceReturn (negative (-25)) []
      assert "context refusal has no writes" (S.null (expectedWrites (serviceExpected refused)))
    assert "unmasked delivery entry violates its precondition" (case C.serviceCall domain DeliverTaskExceptionEntry 0x53 (args 0 0) memory of Left InvalidArguments -> True; _ -> False)
    assert "unlocked delivery loop violates its precondition" (case C.serviceCall domain DeliverTaskException 0xd3 (args 0 0) memory of Left InvalidArguments -> True; _ -> False)
    let timer=0xa0000; heap=0xa1000; countCell=0xa2000; baseCell=countCell+4
        timed=foldr (uncurry put) (withState task 10 memory)
          [(r+0x34,heap),(r+0x38,countCell),(r+0x3c,baseCell),(countCell,1),(baseCell,0),
           (timer,1),(heap,100),(heap+4,timer),(0x70000+cls*4,timer)]
    _ <- expect "wake removes active sleep timer" (b WakeTask timed) (ServiceDispatch Save (fromIntegral (-19::Int))) (negative (-19)) [(countCell,0),(0x70000+cls*4,0)]
    _ <- expect "wake restarts a terminate-pending queued activation" (b WakeTask (put (task+0x24) 1 (withFlags task 1 (withState task 10 memory)))) (ServiceDispatch Save (fromIntegral (-19::Int))) (negative (-19)) [(task+12,1),(task+0x30,1)]
    _ <- expect "wake applies deferred priority before insert" (b WakeTask (put (task+0x28) 1 (put (task+0x34) 5 (withState task 10 memory)))) (ServiceDispatch Save (fromIntegral (-19::Int))) (negative (-19)) [(task+12,0x501),(task+0x28,0)]
    let beforeBase=task-0x38
        zeroInterrupt=foldr (uncurry put) (put (contextWord words') 1 memory)
          [(beforeBase+8,definitionAt cls),(beforeBase+12,1),(beforeBase+16,0)]
    _ <- expect "interrupt exception zero id addresses record before task base" (C.serviceCall domain InterruptRaiseTaskException 0x53 (args (cls*0x10000) 8) zeroInterrupt) ServiceReturn (Just 0) [(beforeBase+16,8)]
    assert "unsupported tag refuses rather than activating" (case B.serviceCall domain SleepTask 0x53 (args handle 0) memory of Left InvalidArguments -> True; _ -> False)
    assert "record fixture is local" (get r memory==Just 0)
  putStrLn "scheduler S3b/S3c service model tests passed"
