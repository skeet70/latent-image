{-# LANGUAGE OverloadedStrings, ScopedTypeVariables #-}
module AF.Simulation where
import FujiAF.Host
import Data.Aeson (Value(..))
import FujiAF.JSON (encodeCanonical)
import Control.Concurrent
import Control.Exception
import Control.Monad
import Data.List (sortOn)
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as BC
import Data.IORef
import GHC.Clock
import System.Exit
import System.IO
import System.Process
import System.Timeout
import System.Posix.Signals
import System.Posix.Types

u32,u64 :: Integer
u32 = 4294967295
u64 = 18446744073709551615
base :: [(String,Value)]
base = [("schema_version",n 1),("origin",s "synthetic"),("clock_id",s "synthetic-ms"),("position_units",s "synthetic-lens-unit")]
validateConfig :: Value -> IO ()
validateConfig fixture = do
 let c=get fixture "config"; m=get fixture "model"; q k=num(get c k); span'=q "position_max"-q "position_min"
 ensure (1<=span' && span'<=1000000 && 1<=q "fine_step" && q "fine_step"<=q "coarse_step" && q "coarse_step"<=span' && q "min_contrast_span"/=0) "unsupported bounded coarse/fine configuration"
 forM_ [num(get m "initial_position"),num(get (get m "curve") "peak_position")] $ \p -> ensure (q "position_min"<=p && p<=q "position_max") "synthetic model position outside declared bounds"
 forM_ (list(get fixture "faults")) $ \f -> ensure (num(get f "at_sequence")+num(get f "repeat_count")-1<=u32) "fault sequence exceeds uint32"
configurationFields :: Value -> [String]
configurationFields c = "AF1" : map (show . flagNum . get c) names ++ ["0"]
 where names=words "position_min position_max coarse_step fine_step min_peak_score min_contrast_span confirmation_drop settle_ms max_age_ms max_moves max_measurements max_steps max_duration_ms require_measured_position require_completion exposure_epoch"
flagNum :: Value -> Integer
flagNum (Bool yes)=if yes then 1 else 0
flagNum x=num x
observationFields :: Value -> [String]
observationFields o = "OBS1" : map show [q "sequence_id",q "command_id_at_acquisition",q "commanded_position_at_acquisition",q "acquired_at",q "delivered_at",known "reported_position",q "reported_position",known "reported_completion",flagNum(get o "reported_completion"),flagNum(get o "valid"),known "sharpness",q "sharpness",q "exposure_epoch",flagNum(get o "cancelled"),if get o "mode"==s "single-af" then 1 else 0]
 where q k=num(get o k); known k=if get o k==Null then 0 else 1

data Bridge = Bridge Handle Handle Handle ProcessHandle (IORef B.ByteString) Integer (Maybe CPid) ThreadId (IORef B.ByteString)
openBridge :: FilePath -> IO Bridge
openBridge binary = do
 (Just hin,Just hout,Just herr,ph)<-createProcess(proc binary []){std_in=CreatePipe,std_out=CreatePipe,std_err=CreatePipe,create_group=True}
 hSetBuffering hin NoBuffering
 buf<-newIORef B.empty
 deadline<- (+30000000000) . toInteger <$> getMonotonicTimeNSec
 pid<-getPid ph
 detail<-newIORef B.empty
 let drain=do
      chunk<-B.hGetSome herr 1024
      unless(B.null chunk)$do
       old<-readIORef detail
       writeIORef detail(B.take 1024(old<>chunk))
       when(B.length old+B.length chunk>1024)$forM_ pid(\p->signalProcessGroup sigKILL p `catch` \(_::IOException)->pure())
       drain
 tid<-forkIO(drain `catch` \(_::IOException)->pure())
 pure(Bridge hin hout herr ph buf deadline pid tid detail)
exchange :: Bridge -> [String] -> IO Value
exchange (Bridge hin hout _ _ buffer deadline _ _ _) values = do
 now<-toInteger <$> getMonotonicTimeNSec
 let input=BC.pack(unwords values++"\n")
 ensure (B.length input<1024 && now<deadline) "finite C bridge input/time budget exceeded"
 response<-timeout (fromInteger(min 3000000 ((deadline-now) `div` 1000))) $ do
  B.hPut hin input
  let collect=do
       old<-readIORef buffer
       case BC.elemIndex '\n' old of
        Just i -> writeIORef buffer (B.drop(i+1) old) >> pure(B.take i old)
        Nothing -> do
         chunk<-B.hGetSome hout 1024
         ensure (not(B.null chunk)) "C bridge rejected input or exited without an action"
         let new=old<>chunk
         ensure (B.length new<=1024) "C bridge output byte budget exceeded"
         writeIORef buffer new; collect
  collect
 line<-maybe(failH "C bridge response deadline") pure response
 let parts=words(BC.unpack line)
 ensure (length parts==12 && take 1 parts==["ACT1"]) "unexpected explicit C action framing"
 numbers<-forM(drop 3 parts) $ \x -> case reads x of [(v,"")]->pure(v::Integer); _->failH "invalid C integer"
 (command,trigger,sequence',targetKnown,target,measures,moves,steps,terminal)<-case numbers of
  [a,c,d,e,f,g,h,i,j]->pure(a,c,d,e,f,g,h,i,j)
  _->failH "unexpected C action integer count"
 ensure (all (`elem` [0,1]) [trigger,targetKnown,terminal] && 0<=command && command<=u32 && 0<=sequence' && sequence'<=u32) "C action flag/counter overflow"
 pure$obj(base++[("record_type",s "action"),("kind",s(parts!!1)),("reason",s(parts!!2)),("command_id",s(show command)),("triggering_sequence_id",if trigger==1 then s(show sequence') else Null),("target_position",if targetKnown==1 then n target else Null),("delta_position",Null),("expected_next_event",s(if terminal==1 then "terminal" else "fresh_settled_sample")),("measurements",n measures),("moves",n moves),("steps",n steps),("terminal",b(terminal==1))])
abortBridge :: Bridge -> IO ()
abortBridge (Bridge hin hout herr ph _ _ pid tid _) = do
 forM_ pid(\p->signalProcessGroup sigKILL p `catch` \(_::IOException)->pure())
 terminateProcess ph `catch` \(_::IOException)->pure()
 void(timeout 1000000(waitForProcess ph))
 killThread tid
 mapM_ (\h->hClose h `catch` \(_::IOException)->pure()) [hin,hout,herr]
closeBridge :: Bridge -> IO ()
closeBridge bridge@(Bridge hin _ _ ph _ _ _ _ detail) = do
 hClose hin
 status<-timeout 1000000(waitForProcess ph)
 case status of
  Nothing->abortBridge bridge >> failH "C bridge failed to stop after finite input"
  Just ExitSuccess -> abortBridge bridge
  _ -> do bytes<-readIORef detail; abortBridge bridge; failH("C bridge failure: "++BC.unpack bytes)

data Lens = Lens {start::Integer,target::Integer,issued::Integer,duration::Integer,command::Integer,previousCommand::Integer,previousTarget::Integer,noise::Integer}
actual :: Value -> Lens -> Integer -> (Integer,Bool)
actual model lens now
 | get model "stuck_lens"==b True=(start lens,False)
 | elapsed>=duration lens=(target lens,True)
 | otherwise=(start lens + (if target lens>=start lens then distance else -distance),False)
 where elapsed=max 0(now-issued lens); distance=abs(target lens-start lens)*elapsed `div` duration lens
move :: Value -> Lens -> Value -> Integer -> Lens
move m l a now = l{start=p,target=t,issued=now,duration=num(get m "move_base_ms")+abs(t-p)*num(get m "travel_ms_per_unit"),command=num(get a "command_id"),previousCommand=command l,previousTarget=target l}
 where (p,_)=actual m l now; t=num(get a "target_position")
score :: Value -> Lens -> Integer -> (Integer,Lens)
score m l p = (min u32(max 0(value+perturb)),l{noise=seed})
 where curve=get m "curve"; peak=num(get curve "peak_score"); value=if get curve "kind"==s "flat" then peak else max 0(peak-num(get curve "slope")*(p-num(get curve "peak_position"))^(2::Int)); seed=(1664525*noise l+1013904223) `mod` (u32+1); amplitude=num(get m "noise_amplitude"); perturb=seed `mod` (2*amplitude+1)-amplitude
simulate :: FilePath -> Value -> IO (Value,[Value])
simulate binary fixture = do
 validateConfig fixture
 bracketOnError (openBridge binary) abortBridge $ \bridge -> do
  let c=get fixture "config"; m=get fixture "model"; q k=num(get c k); mq k=num(get m k); pos=mq "initial_position"
  trace<-newIORef([]::[Value]); size<-newIORef(0::Int)
  let record v=do
       old<-readIORef trace; count<-readIORef size
       let total=count+B.length(encodeCanonical v)
       ensure (total<=8*1024*1024 && length old<8192) "finite synthetic trace budget exceeded"
       modifyIORef' trace (v:); writeIORef size total
      active seq' f=num(get f "at_sequence")<=seq' && seq'<num(get f "at_sequence")+num(get f "repeat_count")
      applyFault l now o f = case str(get f "kind") of
       "previous_command"->merge o [("command_id_at_acquisition",s(show(previousCommand l))),("commanded_position_at_acquisition",n(previousTarget l))]
       "stale"->set "delivered_at" (s(show(now+mq "delivery_delay_ms"+q "max_age_ms"+1))) o
       "invalid"->merge o [("valid",b False),("invalid_reason",s "synthetic_invalid"),("sharpness",Null)]
       "cancel"->set "cancelled" (b True) o
       "unsupported_mode"->set "mode" (s "continuous-af") o
       "exposure_change"->set "exposure_epoch" (n(if q "exposure_epoch"==0 then 1 else 0)) o
       "position_mismatch"->set "reported_position" (n(if target l<q "position_max" then target l+1 else target l-1)) o
       "future_acquisition"->set "acquired_at" (s(show(num(get o "delivered_at")+1))) o
       "duplicate_sequence"->set "sequence_id" (s(show(max 0(num(get o "sequence_id")-1)))) o
       "clock_regression"->set "delivered_at" (s "0") o
       _->o
      loop l action next seq' events now queued
       | get action "terminal"==b True = pure(l,action,events,now)
       | otherwise=do
        ensure(events<num(get fixture "event_limit")) "synthetic event limit before controller termination"
        if null queued || next<=firstTime queued then do
         let now'=next; seq''=seq'+1; (p,done)=actual m l now'; (value,l')=score m l p
         ensure(seq''<=u32 && now'<=u64) "synthetic sequence/clock overflow"
         let observation=obj(base++[("record_type",s "observation"),("sequence_id",s(show seq'')),("command_id_at_acquisition",s(show(command l))),("commanded_position_at_acquisition",n(target l)),("acquired_at",s(show now')),("delivered_at",s(show(now'+mq "delivery_delay_ms"))),("reported_position",if get m "position_available"==b True then n p else Null),("reported_completion",if get m "completion_available"==b True then b done else Null),("valid",b True),("invalid_reason",Null),("sharpness",n value),("exposure_epoch",get c "exposure_epoch"),("cancelled",b False),("mode",s "single-af")])
             faults=filter(active seq'')(list(get fixture "faults"))
             changed=foldl(applyFault l now') observation faults
             delivered=now'+mq "delivery_delay_ms"+if any(\f->get f "kind"==s "stale") faults then q "max_age_ms"+1 else 0
         loop l' action (next+mq "frame_period_ms") seq'' (events+1) now' (sortOn(\(t,i,_)->(t,i))((delivered,seq'',changed):queued))
        else do
         (now',o,rest)<-case queued of
          (t,_,v):remaining->pure(t,v,remaining)
          []->failH "empty synthetic queue"
         record o; a<-exchange bridge(observationFields o); record a
         ensure(all(\k->num(get a k)<=q("max_"++k)) ["measurements","moves","steps"]) "kernel exceeded declared finite budget"
         l'<-if get a "kind"==s "move_to" then do
           let t=num(get a "target_position")
           ensure(q "position_min"<=t && t<=q "position_max") "kernel action outside synthetic bounds"
           pure(move m l a now')
          else pure l
         loop l' a next seq' (events+1) now' rest
      firstTime ((t,_,_):_)=t
      firstTime []=0
  initial<-exchange bridge(configurationFields c); record initial
  let l=Lens pos pos 0 0 0 0 pos (mq "noise_seed")
  (final,action,events,now)<-loop (move m l initial 0) initial (mq "frame_period_ms") 0 0 0 []
  closeBridge bridge
  documents<-reverse <$> readIORef trace
  let (p,done)=actual m final now
  pure(obj[("schema_version",n 1),("origin",s "synthetic"),("controller",s "A1-coarse-fine-v1"),("fixture_id",get fixture "fixture_id"),("units",s "arbitrary synthetic lens units/ms/contrast"),("terminal_action",action),("synthetic_actual_position",n p),("commanded_position",n(target final)),("synthetic_move_completed",b done),("synthetic_elapsed_ms",s(show now)),("events",n events),("target_runtime_budget",Null),("stock_comparison",Null),("camera_state_change",s "none")],documents)
