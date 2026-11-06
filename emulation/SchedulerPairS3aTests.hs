module Main (main) where

import Control.Monad (forM_, unless)
import qualified Data.Map.Strict as M
import qualified Data.Set as S
import Data.Word (Word32)
import Emulation.SchedulerPairModel (Memory, Arguments (..), Expected (..), ModelFailure (..))
import Emulation.SchedulerPairPlan (Address (..), Service (..), DispatchPoint (..))
import Emulation.SchedulerPairS3a
import Emulation.SchedulerPairService
import Replacement.RtosOwnFirst.Regions (Domain (..))

assert :: String -> Bool -> IO ()
assert label result = unless result (error label)

members :: Domain -> (Word32,Word32,Word32,Word32,Word32)
members DomainA = (0x2000,0x3000,0x4000,0x4100,0x5000)
members DomainB = (0x2200,0x3004,0x4400,0x4500,0x5200)

recordOf :: Domain -> Domain -> Word32
recordOf executing target = contextTable (domainWords executing)+(if target==DomainA then 0 else 0x84)

lockOf :: Domain -> Word32
lockOf DomainA = 0x7000
lockOf DomainB = 0x7004

fixture :: Domain -> Memory
fixture executing = M.fromList (concatMap cells [DomainA,DomainB])
  where
    cells target =
      let state=domainWords target
          (heads,bitmap,first,second,definition)=members target
          head'=heads+24
          record=recordOf executing target
          identity=if target==executing then 0 else if target==DomainA then 1 else 2
      in [(Address (contextWord state),0),(Address (currentContext state),recordOf target target),
          (Address (runningCell state),first),(Address (selectedCell state),first),(Address (dispatchCell state),2),
          (Address (rescheduleCell state),0x80000000),(Address (lockOf target),0),
          (Address record,identity),(Address (record+8),lockOf target),
          (Address (record+0x20),heads),(Address (record+0x24),bitmap),(Address (record+0x28),runningCell state),
          (Address (record+0x2c),selectedCell state),(Address (record+0x30),dispatchCell state),
          (Address (record+0x7c),if target==DomainA then 0x43728 else 0xd3728),
          (Address bitmap,8),(Address head',first),(Address (head'+4),second),
          (Address first,second),(Address (first+4),head'),(Address (first+8),definition),(Address (first+0xc),0x301),
          (Address (first+0x10),9),(Address (first+0x18),0),(Address (first+0x1c),0),(Address (first+0x24),0x91),
          (Address (first+0x28),0x92),(Address (first+0x2c),0x93),(Address (first+0x30),0),
          (Address second,head'),(Address (second+4),first),(Address (second+0xc),0x301),
          (Address (definition+0xc),3),(Address (definition+0x10),0x6000),(Address (definition+0x14),0x100)]
          ++ [(Address (heads+p*8+offset),heads+p*8) | p<-[0..15],p/=3,offset<-[0,4]]

set :: Word32 -> Word32 -> Memory -> Memory
set at value = M.insert (Address at) value

args :: Word32 -> Word32 -> Arguments
args first second = Arguments [first,second,0,0]

expectService :: String -> Domain -> Service -> Word32 -> Arguments -> Memory -> ServiceCompletion -> Word32 -> Word32 -> [(Word32,Word32)] -> [Word32] -> [(Address,Word32)] -> IO ()
expectService label domain service cpsr arguments before completion result processor changed written banks =
  case serviceCall domain service cpsr arguments before of
    Left failure -> error (label++": "++show failure)
    Right actual -> do
      assert (label++" completion") (serviceCompletion actual==completion)
      assert (label++" result") (serviceR0 actual==Just result)
      assert (label++" processor") (serviceCpsr actual==processor)
      let expected=serviceExpected actual
      assert (label++" memory") (expectedMemory expected==M.union (M.fromList [(Address at,value) | (at,value)<-changed]) before)
      assert (label++" writes") (expectedWrites expected==S.fromList (map Address written))
      assert (label++" bank") (expectedBank expected==banks)

negative :: Int -> Word32
negative = fromIntegral

main :: IO ()
main = do
  forM_ [DomainA,DomainB] $ \domain -> do
    let state=domainWords domain
        before=fixture domain
        (heads,bitmap,first,second,_)=members domain
        head'=heads+24
        lock=lockOf domain
        handler=set (contextWord state) 1 before
        rotateChanges=[(head',second),(head'+4,first),(first,head'),(first+4,second),(second,first),(second+4,head'),(selectedCell state,second)]
        rotateWrites=lock:map fst rotateChanges
        returning service arguments initial changed written = expectService (show domain++" "++show service) domain service 0x53 arguments initial ServiceReturn 0 0x53 changed written []
        dispatching service arguments initial completion result changed written = expectService (show domain++" "++show service) domain service 0x53 arguments initial completion (negative result) 0xd3 changed written []
    returning DisableDispatch (args 0 0) before [(dispatchCell state,0),(lock,0)] [dispatchCell state,lock]
    returning EnableDispatch (args 0 0) before [(dispatchCell state,1),(lock,0)] [dispatchCell state,lock]
    dispatching EnableDispatch (args 0 0) (set (selectedCell state) second before) (ServiceDispatch Save (negative (-92))) (-92) [(dispatchCell state,1),(lock,0)] [dispatchCell state,lock]
    expectService "discarded enable unmasks before no-save" domain EnableDispatch 0x53 (args 0 0) (set (first+0x30) 2 before)
      (ServiceDispatch NoSave (negative (-92))) (negative (-92)) 0x53 [(dispatchCell state,1),(lock,0)] [dispatchCell state,lock] []
    forM_ [0,4] $ \priority -> dispatching RotateReady (args priority 0) before (ServiceDispatch Save (negative (-85))) (-85) rotateChanges rotateWrites
    returning RotateReady (args 4 0) (set (dispatchCell state) 0 before) rotateChanges rotateWrites
    returning InterruptRotateReady (args 4 0) handler (rotateChanges++[(rescheduleCell state,1)]) (rescheduleCell state:rotateWrites)
    forM_ [RotateReady,InterruptRotateReady,CoreRotateReady,InterruptCoreRotateReady] $ \service -> do
      let interrupt=service `elem` [InterruptRotateReady,InterruptCoreRotateReady]
          initial=if interrupt then handler else before
          priority argument=if service `elem` [CoreRotateReady,InterruptCoreRotateReady] then args 0 argument else args argument 0
      returning service (priority 1) initial [] [lock]
      returning service (priority 4) (set first head' (set (head'+4) first initial)) [] [lock]
    returning RotateReady (args 4 0) (set (selectedCell state) second before)
      (filter ((/=selectedCell state) . fst) rotateChanges) (filter (/=selectedCell state) rotateWrites)
    expectService "SYS F/A control state is preserved" domain DisableDispatch 0x15f (args 0 0) before ServiceReturn 0 0x15f
      [(dispatchCell state,0),(lock,0)] [dispatchCell state,lock] []
    forM_ [CoreRotateReady,InterruptCoreRotateReady] $ \service -> do
      let initial=if service==CoreRotateReady then before else handler
      if service==CoreRotateReady then dispatching service (args 0 4) initial (ServiceDispatch Save (negative (-4))) (-4) rotateChanges rotateWrites
        else returning service (args 0 4) initial (rotateChanges++[(rescheduleCell state,1)]) (rescheduleCell state:rotateWrites)
      let target=if domain==DomainA then DomainB else DomainA
          selector=if target==DomainA then 1 else 2
          targetState=domainWords target
          (remoteHeads,_,remoteFirst,remoteSecond,_)=members target
          remoteHead=remoteHeads+24
          remoteLock=lockOf target
          changes=[(remoteHead,remoteSecond),(remoteHead+4,remoteFirst),(remoteFirst,remoteHead),(remoteFirst+4,remoteSecond),(remoteSecond,remoteFirst),(remoteSecond+4,remoteHead),(selectedCell targetState,remoteSecond)]
          selfStore=[(rescheduleCell state,0x80000000) | service==InterruptCoreRotateReady]
          bank=Address (if target==DomainA then 0xfffb30cc else 0xfffb40cc)
      expectService "remote rotation keeps local reschedule and rings target bank" domain service 0x53 (args selector 4) initial ServiceReturn 0 0x53
        (changes++selfStore) (remoteLock:map fst (changes++selfStore)) [(bank,0x1000000)]
    let resetChanges=[(head',second),(second+4,head'),(selectedCell state,second),(first+0xc,0x300),(first+0x10,0),(first+0x24,0),(first+0x28,0),(first+0x2c,0)]
        resetWrites=lock:map fst resetChanges
    dispatching ExitTask (args 0 0) (set (first+0xc) 0x60301 before) (ServiceDispatch NoSave (negative (-10))) (-10) resetChanges resetWrites
    dispatching ExitTask (args 0 0) (set (contextWord state) 1 (set (first+0xc) 0x60302 before))
      (ServiceDispatch NoSave (negative (-10))) (-10) resetChanges resetWrites
    let queuedChanges=rotateChanges++[(first+0xc,0x301),(first+0x10,0),(first+0x24,0),(first+0x28,0),(first+0x2c,0),(first+0x18,0x6100),(first+0x1c,if domain==DomainA then 0x43728 else 0xd3728)]
    dispatching ExitTask (args 0 0) (set (first+0xc) 0x70301 before) (ServiceDispatch NoSave (negative (-10))) (-10) queuedChanges (lock:bitmap:map fst queuedChanges)
    dispatching ExitTask (args 0 0) (set (first+0x30) 1 (set (dispatchCell state) 0 before)) (ServiceDispatch NoSave (negative (-10))) (-10)
      [(dispatchCell state,1),(lock,0)] [dispatchCell state,lock]
    forM_ [DisableDispatch,EnableDispatch,RotateReady,InterruptRotateReady,InterruptCoreRotateReady] $ \service -> do
      let initial=if service `elem` [InterruptRotateReady,InterruptCoreRotateReady] then before else handler
      expectService "wrong execution context refuses without effects" domain service 0x53 (args 4 4) initial ServiceReturn (negative (-25)) 0x53 [] [] []
    forM_ [DisableDispatch,EnableDispatch,RotateReady,InterruptRotateReady,InterruptCoreRotateReady] $ \service -> do
      let initial=if service `elem` [InterruptRotateReady,InterruptCoreRotateReady] then handler else before
      expectService "masked entry refuses without effects" domain service 0xd3 (args 4 4) initial ServiceReturn (negative (-25)) 0xd3 [] [] []
    forM_ [RotateReady,InterruptRotateReady,InterruptCoreRotateReady] $ \service -> do
      let initial=if service==RotateReady then before else handler
          arguments=if service==InterruptCoreRotateReady then args 0 17 else args 17 0
      expectService "invalid priority refuses without effects" domain service 0x53 arguments initial ServiceReturn (negative (-17)) 0x53 [] [] []
    forM_ [(args 3 4,before),(args 0 0,before),(args 0 4,handler)] $ \(arguments,initial) ->
      returning CoreRotateReady arguments initial [] []
    assert "interrupt core selector has a caller precondition" (serviceCall domain InterruptCoreRotateReady 0x53 (args 3 4) handler==Left InvalidArguments)
    assert "missing service state refuses" (serviceCall domain DisableDispatch 0x53 (args 0 0) M.empty==Left (MissingWord (Address (contextWord state))))
    assert "invalid argument vector refuses" (serviceCall domain ExitTask 0x53 (Arguments []) before==Left InvalidArguments)
    case serviceCall domain DisableDispatch 0x53 (args 0 0) (set (first+0xc) 0x302 before) of
      Right result -> assert "nonready task retries after unlocking" (serviceCompletion result==ServiceRetry && serviceR0 result==Nothing && serviceCpsr result==0x53 && M.lookup (Address lock) (expectedMemory (serviceExpected result))==Just 0)
      Left failure -> error (show failure)
    case serviceCall domain DisableDispatch 0x53 (args 0 0) (set lock 1 before) of
      Right result -> assert "held lock remains held while acquisition retries" (serviceCompletion result==ServiceRetry && serviceCpsr result==0xd3 && expectedWrites (serviceExpected result)==S.empty)
      Left failure -> error (show failure)

  forM_ [DomainA,DomainB] $ \domain -> do
    forM_ [EnableDispatch,RotateReady,CoreRotateReady] $ \service ->
      case resumeService domain service 0x1df (args 0 0) M.empty of
        Right result -> assert "resumed services return zero and unmask without rereading task state"
          (serviceCompletion result==ServiceReturn && serviceR0 result==Just 0 && serviceCpsr result==0x15f &&
           serviceExpected result==Expected 0 M.empty S.empty [])
        Left failure -> error (show failure)
    forM_ [DisableDispatch,ExitTask,InterruptRotateReady,InterruptCoreRotateReady] $ \service ->
      assert "services without save dispatch reject resume"
        (resumeService domain service 0xd3 (args 0 0) M.empty==Left InvalidArguments)
  putStrLn "S3a natural service model tests passed"
