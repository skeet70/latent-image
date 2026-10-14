{-# LANGUAGE OverloadedStrings,ScopedTypeVariables #-}
module Main where
import FujiAF.Host
import AF.Simulation
import Control.Exception
import Control.Monad
import Data.Aeson(Value(..))
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as BC
import FujiAF.JSON(decodeStrict)
import System.Environment
import System.Directory
import System.FilePath
import System.Exit
import System.Posix.Files
import System.IO

check :: Bool -> String -> IO ()
check=ensure
reject :: IO a -> IO ()
reject action=do r<-try(void action); case r of Left(_::IOException)->pure();Right _->failH "unsupported input accepted"
main :: IO ()
main=do
 [binary,checker,cli]<-getArgs
 baseFixture<-canonicalizePath "af/fixtures/peak-delayed.json" >>= readRegular(1024*1024) >>= jsonRead
 let altered changes=set "config" (merge(get baseFixture "config")changes)baseFixture
     model fixture changes=set "model" (merge(get fixture "model")changes)fixture
     fault :: String -> Integer -> Integer -> [(String,Value)] -> Value
     fault kind at repeat' config=set "faults" (arr[obj[("at_sequence",s(show at)),("repeat_count",n repeat'),("kind",s kind)]])(altered config)
     run name fixture kind reason held=do
      schemaCheck checker "schemas/af-simulation.schema.json"[fixture]
      (result,trace)<-simulate binary fixture
      schemaCheck checker "schemas/af-observation-action.schema.json"trace
      let terminal=get result "terminal_action"; c=get fixture "config"
      check(get terminal "kind"==s kind && get terminal "reason"==s reason)(name++": terminal differs")
      let pairs []=[]; pairs(o:a:rest)=(o,a):pairs rest; pairs _=[]
          verify _ _ []=pure()
          verify pending prev ((o,a):rest)=do
           check(get a "triggering_sequence_id"==get o "sequence_id")(name++": action trigger")
           when(num(get a "measurements")>prev)$do
            check(num(get a "measurements")==prev+1)(name++": accepted count")
            check(get o "valid"==b True && get o "sharpness"/=Null)(name++": invalid score accepted")
            check(get o "command_id_at_acquisition"==get pending "command_id" && get o "commanded_position_at_acquisition"==get pending "target_position")(name++": wrong acquisition command accepted")
            check(num(get o "delivered_at")-num(get o "acquired_at")<=num(get c "max_age_ms"))(name++": stale accepted")
            check(get o "reported_completion"/=b False)(name++": unfinished move accepted")
            check(get o "reported_position"==Null || get o "reported_position"==get pending "target_position")(name++": wrong measured position accepted")
           when(get a "kind"==s "move_to")$check(num(get c "position_min")<=num(get a "target_position") && num(get a "target_position")<=num(get c "position_max"))(name++": move bounds")
           verify(if get a "kind"==s "move_to" then a else pending)(num(get a "measurements"))rest
      case trace of
       pending:rest->verify pending 0(pairs rest)
       []->failH "missing initial action"
      forM_ held$ \hold->check(any(\a->get a "reason"==s hold)trace)(name++": controlled hold missing")
      when(kind=="finish")$do
       let expected=get fixture "expected"
       check(abs(num(get result "synthetic_actual_position")-num(get expected "position"))<=num(get expected "tolerance"))(name++": final position")
       check(get result "synthetic_move_completed"==b True)(name++": final completion")
      check(get result "target_runtime_budget"==Null && get result "stock_comparison"==Null)(name++": scope")
      putStrLn(name++": "++kind++"/"++reason)
      pure(result,trace)
 (_,delayed)<-run "delayed-peak" baseFixture "finish" "confirmed" (Just "acquisition_command_mismatch")
 check(any(\o->get o "record_type"==s "observation" && get o "reported_position"/=get o "commanded_position_at_acquisition")delayed)"commanded and measured collapsed"
 forM_["flat","unknown-telemetry"]$ \filename->do
  fixture<-canonicalizePath("af/fixtures/"++filename++".json") >>= readRegular(1024*1024) >>= jsonRead
  (_,trace)<-run filename fixture (str(get(get fixture "expected")"kind"))(str(get(get fixture "expected")"reason")) Nothing
  when(filename=="unknown-telemetry")$check(all(\o->get o "record_type"/=s "observation" || get o "reported_position"==Null && get o "reported_completion"==Null)trace)"invented telemetry"
 let noisy=set "expected" (set "tolerance" (n 5)(get baseFixture "expected"))(model baseFixture[("noise_amplitude",n 10)])
 void$run "deterministic-noise" noisy "finish" "confirmed" Nothing
 forM_[("stale",32,"stale_sample"),("invalid",32,"invalid_observation"),("previous_command",8,"acquisition_command_mismatch")]$ \(kind,steps,hold)->void$run kind(fault kind 1 1024[("max_steps",n steps)])"fail""observation_limit"(Just hold)
 void$run "duplicate-sequence"(fault "duplicate_sequence" 2 1[])"finish""confirmed"(Just "duplicate_sequence")
 void$run "early-acquisition"(altered[("settle_ms",n 30)])"finish""confirmed"(Just "early_sample")
 forM_[("cancel","cancelled"),("unsupported_mode","unsupported_mode"),("exposure_change","exposure_changed")]$ \(kind,reason)->void$run kind(fault kind 1 1[])"hand_back_to_stock"reason Nothing
 void$run "future-clock"(fault "future_acquisition" 1 1[])"fail""clock_error"Nothing
 void$run "regressing-clock"(fault "clock_regression" 2 1[])"fail""clock_error"Nothing
 forM_[("max_moves",2,"move_limit"),("max_measurements",2,"measurement_limit"),("max_steps",2,"observation_limit"),("max_duration_ms",10,"time_limit")]$ \(field,value,reason)->void$run field(altered[(field,n value)])"fail"reason Nothing
 void$run "unfinished-lens"(model(altered[("max_steps",n 16)])[("stuck_lens",b True)])"fail""observation_limit"(Just "wait_completion")
 void$run "unknown-required-position"(model(altered[("max_steps",n 32)])[("position_available",b False)])"fail""observation_limit"(Just "wait_position")
 void$run "position-mismatch"(fault "position_mismatch" 1 1024[("max_steps",n 32)])"fail""observation_limit"(Just "wait_position")
 forM_[("schema_version",n 2),("origin",s "hardware"),("stock_speedup",n 2),("position_units",s "X-M1-motor-ticks")]$ \(k,v)->reject(schemaCheck checker "schemas/af-simulation.schema.json"[set k v baseFixture])
 observation<-case filter(\d->get d "record_type"==s "observation")delayed of
  first:_->pure first
  []->failH "missing delayed observation"
 forM_[("sharpness",n(u32+1)),("reported_completion",n 1),("firmware_address",s "0x1000")]$ \(k,v)->reject(schemaCheck checker "schemas/af-observation-action.schema.json"[set k v observation])
 forM_[[("position_min",n(-2147483648)),("position_max",n 2147483647)],[("fine_step",n 26)],[("min_contrast_span",n 0)]]$ \changes->reject(validateConfig(altered changes))
 let line=unwords(configurationFields(get baseFixture "config"))++"\n"
     overflow=unwords(configurationFields(get(altered[("coarse_step",n(u32+1))])"config"))++"\n"
 forM_["AF2 0\n",init line++" extra\n",replicate 1200 'A'++"\n",overflow,line++"OBS1 4294967296 1 0 0 1 1 0 1 1 1 1 100 0 0 1\n"]$ \input->do
  (_,code,_,_)<-runBounded 3000000 8192 binary[](BC.pack input)
  check(code/=ExitSuccess)"C bridge accepted malformed input"
 withTemporary "fuji-af-input-tests"$ \root->do
  physicalFixture<-canonicalizePath "af/fixtures/peak-delayed.json"
  let workspaceRoot=takeDirectory(takeDirectory(takeDirectory physicalFixture))
  binaryAbs<-canonicalizePath binary; checkerAbs<-canonicalizePath checker; cliAbs<-canonicalizePath cli
  old<-lookupEnv "BUILD_WORKSPACE_DIRECTORY"
  bracket_(setEnv "BUILD_WORKSPACE_DIRECTORY" workspaceRoot)(maybe(unsetEnv "BUILD_WORKSPACE_DIRECTORY")(setEnv "BUILD_WORKSPACE_DIRECTORY")old)$do
   (timed,code,out,_)<-withCurrentDirectory root(runBounded 20000000 (8*1024*1024) cliAbs[binaryAbs,checkerAbs,"af/fixtures/peak-delayed.json"]B.empty)
   response<-jsonRead out
   check(not timed && code==ExitSuccess && get(get response "terminal_action")"kind"==s "finish")"CLI workspace-relative input failed"
  let regular=root </> "input.json"; fifo=root </> "fifo"; link=root </> "symlink"; large=root </> "large"
  B.writeFile regular "{}"; createNamedPipe fifo 0o600; createSymbolicLink regular link
  withBinaryFile large WriteMode$ \h->hSetFileSize h(1024*1024+1)
  mapM_(reject.readRegular(1024*1024))[fifo,link,root,large,"/dev/null"]
  reject(readRegularWith(removeFile regular>>createNamedPipe regular 0o600)True(1024*1024)regular)
 check(either(const True)(const False)(decodeStrict "{\"x\":1,\"x\":2}"))"duplicate JSON accepted"
 putStrLn "PASS: 21 closed-loop scenarios plus strict contracts, C framing/overflow, finite input guards"
