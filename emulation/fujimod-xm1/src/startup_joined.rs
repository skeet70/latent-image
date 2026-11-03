use super::*;
use super::loader_startup_program::{Bindings,JoinedProgram};
use super::shared_consumers::shared::{Event as ConsumerEvent,Input as ConsumerInput,Recorder,Sites as ConsumerSites};
use super::shared_dispatch::shared::Capture as Dispatcher;
use fujimod_emulator::board::observation::WatchKinds;
use std::io::Write;

const SHARED_USAGE:&str="loader-continuations --shared-role retained|joined --shared-artifact STAGED_DIR --run RUN --update DAT --boot-rom ROM --analysis ROOT --output NEW_DIR";
const SPLIT:u64=8_000_000;
const ROUTE_LIMIT:u64=450_000_000;
#[derive(Clone,Copy,Serialize)]
#[serde(rename_all="kebab-case")]
enum Role {Retained,Joined}
#[derive(Serialize)]
#[serde(tag="kind",rename_all="kebab-case")]
enum Evidence {Watch {file:FileBinding,teardown_error:Option<String>},ConsumerSegment {segment:usize}}
#[derive(Serialize)]
struct SharedEpoch {stage:WatchStage,start:Count,end:Count,start_counts:[u64;2],end_counts:[u64;2],evidence:Evidence}
#[derive(Serialize)]
struct Route<'a> {origin:Option<EventId>,events:&'a [Event],calls:&'a [Call],snapshots:&'a [Snapshot],pulses:&'a [Pulse],static_calls:&'a [StaticCall]}
#[derive(Serialize)]
struct Receipt<'a> {
    schema:u32,role:Role,program:ProgramBinding,artifacts:Bindings,identity:InputIdentity,checkpoint:Digest,
    instructions:Count,counts:Result<[u64;2],String>,menu:Result<Option<[u32;3]>,String>,error:Option<String>,machine_failure:Option<String>,
    route:Route<'a>,epochs:&'a [SharedEpoch],dispatcher:&'a Dispatcher,consumer:&'a Recorder,
    trace:FileBinding,timer_transactions:FileBinding,
}
fn require(ok:bool,message:&str)->Result<(),String>{if ok {Ok(())}else{Err(message.into())}}
fn counts(machine:&Machine)->Result<[u64;2],String>{machine.session.board.instruction_counts().try_into().map_err(|_|"shared route requires two core clocks".into())}
fn image(catalog:&Catalog,kind:SourceKind)->Result<&[u8],String>{catalog.0.iter().find(|image|image.binding.kind==kind).map(|image|image.bytes.as_slice()).ok_or_else(||"effective source absent".into())}
pub(super) fn guard_classes(stage:WatchStage)->Vec<WatchRange>{
    let mut result=classes(stage);
    result.extend([(0x633e8,0x63460),(0xf33c8,0xf3420)].into_iter().map(|(start,end)|
        WatchRange {start,end,kinds:WatchKinds::WRITES}));
    result.extend([(0x69cdf8,0x69cf84),(0x725a04,0x725b54)].into_iter().map(|(start,end)|
        WatchRange {start,end,kinds:WatchKinds::CPU_FETCH.union(WatchKinds::WRITES)}));
    result.extend([(0x61c008,0x61c00c),(0x7caf3c,0x7caf40)].into_iter().map(|(start,end)|
        WatchRange {start,end,kinds:WatchKinds::CPU_READ.union(WatchKinds::WRITES)}));
    result
}
struct Coordinator<'a> {
    route:Observation,dispatcher:Dispatcher,consumer:Recorder,epochs:Vec<SharedEpoch>,
    catalog:&'a Catalog,maps:Vec<MemoryMap>,output:&'a Path,permanent:BTreeMap<u32,String>,calls:Vec<StaticCall>,
    carrier_calls:BTreeSet<u32>,program:&'a Program,timer:TimerWatch,timer_active:bool,
    consumer_segments:usize,owners:StartupSites,
}
impl Coordinator<'_> {
    fn collect(&mut self,machine:&mut Machine,start:usize)->Result<(),String>{
        self.route.collect(machine,self.catalog,&self.maps,self.output,&self.permanent,&self.calls,self.program)?;
        self.dispatcher.collect(machine,&self.maps,start,&self.carrier_calls)?;
        machine.traces.stop_labels=machine.traces.sites.values().cloned().chain(["temporary-boundary".into()]).collect();
        Ok(())
    }
    fn timer_start(&mut self,machine:&mut Machine)->Result<(),String>{
        self.timer.start=Count(machine.instructions());
        machine.session.board.transaction_watch_begin(vec![(u64::from(TIMER_RANGE[0].0),u64::from(TIMER_RANGE[1].0))],TIMER_LIMIT)
            .map_err(|error|{self.timer.error=Some(error.clone());error})?;
        self.timer_active=true;Ok(())
    }
    fn timer_end(&mut self,machine:&mut Machine)->Result<(),String>{
        self.timer.end=Count(machine.instructions());self.timer_active=false;
        self.timer.batch=Some(machine.session.board.transaction_watch_end().map_err(|error|{self.timer.error=Some(error.clone());error})?);
        require(!self.timer.batch.as_ref().is_some_and(|batch|batch.omitted!=0),"shared timer transaction bound exhausted")
    }
    fn fixed_epoch(&mut self,machine:&mut Machine,end:u64,stage:WatchStage)->Result<(),String>{
        let start=Count(machine.instructions());let start_counts=counts(machine)?;
        let policy=if stage==WatchStage::Bootstrap {ViewPolicy::Observe}else{ViewPolicy::Fixed};
        let mut watch=machine.session.board.memory_watch_begin_classified(guard_classes(stage),3_000_000,self.maps.clone(),policy)?;
        let body=(||->Result<(),String>{
            while machine.instructions()<end {
                let before=machine.instructions();let trace_start=machine.traces.hits.len();
                machine.run(end-before);self.collect(machine,trace_start)?;machine.require_running()?;
                if stage==WatchStage::Bootstrap && self.route.first_loader.is_some(){break;}
                if !self.consumer.complete() && Recorder::is_outer(machine)? {break;}
                if !self.consumer.complete(){self.consumer.reject_early_family(machine,trace_start)?;}
                require(machine.instructions()>before || machine.traces.hits.len()>trace_start,"shared route made no instruction or stopped-boundary progress")?;
            }
            Ok(())
        })();
        let raw=serde_json::to_vec(&watch.records.borrow().state()).map_err(|error|error.to_string());
        let teardown=machine.session.board.memory_watch_end(&mut watch).err();
        let raw=raw?;let name=format!("watch-shared-{}.json",self.epochs.len());write(&self.output.join(&name),&raw)?;
        self.epochs.push(SharedEpoch {stage,start,end:Count(machine.instructions()),start_counts,end_counts:counts(machine)?,
            evidence:Evidence::Watch {file:binding(self.output,&name)?,teardown_error:teardown.clone()}});
        #[derive(Deserialize)]struct State {enabled:bool,violation:Option<String>}
        let state:State=serde_json::from_slice(&raw).map_err(|error|error.to_string())?;
        body?; if let Some(error)=teardown.or(state.violation){return Err(error);}
        require(state.enabled,"shared route memory watch disabled")?;
        self.dispatcher.check_writes(&raw)
    }
    fn advance(&mut self,machine:&mut Machine,count:u64)->Result<(),String>{
        let end=machine.instructions().checked_add(count).ok_or("shared route deadline overflow")?;
        require(end<=ROUTE_LIMIT,"shared route finite cap exceeded")?;
        while machine.instructions()<end {
            let before=machine.instructions();
            let endpoint=if before<SPLIT {end.min(SPLIT)}else{end};
            let stage=if before<SPLIT {WatchStage::Startup}else{WatchStage::Tail};
            if self.consumer.active(){
                let start=Count(before);let start_counts=counts(machine)?;
                let Self {route,dispatcher,consumer,catalog,maps,output,permanent,calls,carrier_calls,program,..}=self;
                let outcome=consumer.step(machine,guard_classes(stage),endpoint-before,output,|machine,event|{
                    match event {
                        ConsumerEvent::Stop(trace_start)=>{
                            route.collect(machine,catalog,maps,output,permanent,calls,program)?;
                            dispatcher.collect(machine,maps,trace_start,carrier_calls)?;
                            machine.traces.stop_labels=machine.traces.sites.values().cloned().chain(["temporary-boundary".into()]).collect();
                            Ok(())
                        },ConsumerEvent::Watch(raw)=>dispatcher.check_writes(raw),
                    }
                });
                let segments=consumer.segment_count();
                if segments>self.consumer_segments {
                    self.epochs.push(SharedEpoch {stage,start,end:Count(machine.instructions()),start_counts,end_counts:counts(machine)?,
                        evidence:Evidence::ConsumerSegment {segment:self.consumer_segments}});self.consumer_segments=segments;
                }
                outcome?;
                if self.consumer.complete(){self.consumer.close_sites(machine)?;}
            }else{
                self.fixed_epoch(machine,endpoint,stage)?;
                if !self.consumer.complete() && Recorder::is_outer(machine)? {
                    self.consumer.begin(machine,image(self.catalog,SourceKind::Image4)?,image(self.catalog,SourceKind::LoaderA)?,image(self.catalog,SourceKind::LoaderB)?)?;
                }else if !self.consumer.complete() && machine.instructions()>=SPLIT{return Err("outer phase0 entry deadline expired".into());}
            }
            if machine.instructions()==SPLIT && self.timer_active {self.timer_end(machine)?;}
            require(machine.instructions()>before || self.consumer.active(),"shared coordinator made no progress")?;
        }
        Ok(())
    }
    fn pulse(&mut self,machine:&mut Machine,key:Key)->Result<(),String>{
        machine.press(key.guest())?;let index=self.route.pulses.len();
        self.route.pulses.push(Pulse {key,pressed:Count(machine.instructions()),released:None,finished:None,outcome:None});
        self.advance(machine,8_000_000)?;machine.release_key(key.guest())?;
        let start=machine.instructions();self.route.pulses[index].released=Some(Count(start));
        let outcome=loop {
            if machine.document.firmware.released(&machine.session,key.guest())?{break ReleaseOutcome::Debounced;}
            let spent=machine.instructions()-start;if spent>=8_000_000{break ReleaseOutcome::Deadline;}
            self.advance(machine,(8_000_000-spent).min(250_000))?;
        };
        self.route.pulses[index].finished=Some(Count(machine.instructions()));self.route.pulses[index].outcome=Some(outcome);Ok(())
    }
    fn run(&mut self,machine:&mut Machine)->Result<(),String>{
        self.fixed_epoch(machine,SPLIT,WatchStage::Bootstrap)?;
        require(self.route.first_loader.is_some(),"first carrier entry deadline expired")?;
        self.owners.close(machine,Component::Bootstrap)?;
        self.timer_start(machine)?;
        self.advance(machine,SPLIT.checked_sub(machine.instructions()).ok_or("first entry exceeds startup cap")?)?;
        self.advance(machine,80_000_000u64.checked_sub(machine.instructions()).ok_or("startup exceeds menu deadline")?)?;
        require(self.consumer.complete(),"startup consumer subobservation did not close")?;
        require(machine.menu()?.is_some_and(|menu|(menu.menu_id,menu.setting_id,menu.cursor)==(69,52,12)),"natural initial cursor differs")?;
        for key in std::iter::repeat_n(Key::Up,12).chain([Key::Ok,Key::Ok]){self.pulse(machine,key)?;}
        self.advance(machine,40_000_000)?;self.pulse(machine,Key::Menu)?;self.advance(machine,90_000_000)?;
        self.dispatcher.finish()?;
        require(machine.menu()?.is_some_and(|menu|(menu.menu_id,menu.setting_id,menu.cursor)==(1,5,2)),"natural final menu differs")
    }
}
pub(super) fn drive(mut options:BTreeMap<OptionKey,PathBuf>)->Result<(),String>{
    let output=options.remove(&OptionKey::Output).ok_or(SHARED_USAGE)?;
    std::fs::create_dir(&output).map_err(|error|error.to_string())?;
    let result=(||->Result<(),String>{
        let role=match options.remove(&OptionKey::SharedRole).ok_or(SHARED_USAGE)?.to_str(){Some("retained")=>Role::Retained,Some("joined")=>Role::Joined,_=>return Err(SHARED_USAGE.into())};
        let mut take=|key|options.remove(&key).ok_or_else(||SHARED_USAGE.to_owned());
        let stage=take(OptionKey::SharedArtifact)?;let run_path=take(OptionKey::Run)?;let update_path=take(OptionKey::Update)?;
        let rom_path=take(OptionKey::Rom)?;let analysis=take(OptionKey::Analysis)?;require(options.is_empty(),SHARED_USAGE)?;
        let run_bytes=read(&run_path)?;let update=read(&update_path)?;let rom=read(&rom_path)?;
        let observer=read(&std::env::current_exe().map_err(|error|error.to_string())?)?;
        let mut catalog=Catalog::open(&analysis)?;
        let (joined,artifacts)=JoinedProgram::open(&stage,&output,&mut catalog)?;
        let (program,consumer_sites,dispatcher_facts)=match role {
            Role::Joined=>{
                let Program::Joined(proof)=&joined else{return Err("joined source proof absent".into())};
                let a=proof.sites(Side::LoaderA);let b=proof.sites(Side::LoaderB);
                let sites=ConsumerSites::owned([a.completion_store.0,a.completion_before_dsb.0,a.completion_after_dsb.0],
                    [b.peer_poll.0,b.peer_poll_postload.0,b.peer_poll_branch.0],b.load_destination_register,
                    [b.initializer_call.0,b.initializer_entry.0,b.initializer_return.0])?;
                let facts=proof.dispatcher_calls.clone();(joined,sites,facts)
            },Role::Retained=>{catalog=Catalog::open(&analysis)?;(Program::Retained,ConsumerSites::retained(),Vec::new())},
        };
        require(Digest::of(&run_bytes).as_str()=="efa32ff8d2a378ba9e098ac28015faf82856765c866d5db5b97cb1c38342deef"
            && Digest::of(&rom).as_str()=="4ecedfbfc1910bde488d36abe4380621f8845e6afcfc6cf54a49a33ec0abf9be"
            && Digest::of(&update).as_str()==program.update(),"shared reset inputs differ from admitted program")?;
        let identity=InputIdentity {run:Digest::of(&run_bytes),update:Digest::of(&update),update_file:FileBinding {path:"input-update.DAT".into(),sha256:Digest::of(&update)},
            rom:Digest::of(&rom),executable:Digest::of(&observer),sources:catalog.0.iter().map(|image|image.binding.clone()).collect()};
        std::fs::hard_link(&update_path,output.join("input-update.DAT")).map_err(|error|error.to_string())?;
        persist(&output.join("inputs.json"),&identity)?;
        fujimod_emulator_exr2_board::register_board_builder();fujimod_emulator_models::register();
        let document=RunDocument::parse(&run_bytes)?;let maps=run::memory_maps(&document)?;
        let validated=ValidatedUpdate(update);
        let mut session=run::open(&document,&validated,&rom)?;session.board.set_execution_policy(document.execution_policy.into())?;
        let mut machine=Machine::new(document,session)?;machine.disarm()?;
        let checkpoint=machine.session.checkpoint(None)?.export()?;write(&output.join("reset.fujimod"),&checkpoint)?;
        let restored=machine.session.import_snapshot(&checkpoint)?;machine.session.restore(&restored)?;machine.rearm()?;
        require(machine.instructions()==0 && counts(&machine)?==[0,0],"shared checkpoint is not fresh zero-instruction reset")?;
        let calls=startup_calls(&catalog)?;
        let carrier_calls=calls.iter().filter(|call|carrier(call.site).is_some()).map(|call|call.site.0).collect();
        let mut consumer=Recorder::new(&machine,ConsumerInput {stage:&stage,owned:matches!(role,Role::Joined),run:&run_bytes,update:&validated.0,rom:&rom,
            checkpoint:&checkpoint,observer:&observer,sites:consumer_sites,carrier_sizes:[image(&catalog,SourceKind::Image4)?.len(),image(&catalog,SourceKind::LoaderA)?.len(),image(&catalog,SourceKind::LoaderB)?.len()]})?;
        let permanent=sites(&calls,&program);
        let bootstrap=[Side::LoaderA,Side::LoaderB].into_iter().flat_map(|side|(side.base().0+0x9ac..side.base().0+0xb00).step_by(4))
            .map(|pc|(pc,"first-carrier-fetch".to_owned())).collect();
        let (owners,initial)=StartupSites::new([permanent.clone(),Dispatcher::sites(),consumer.initial_sites(),bootstrap]);
        consumer.attach_owners(owners.clone());
        let mut dispatcher=Dispatcher::new(matches!(role,Role::Joined),dispatcher_facts);dispatcher.attach_owners(owners.clone());
        let dispatcher_code=image(&catalog,SourceKind::Image4)?.get(0x69cdf8-0x61c400..0x69cf84-0x61c400).ok_or("dispatcher source extent absent")?;
        machine.traces.bind_code_words(NonZeroUsize::new(131_072).ok_or("zero shared trace cap")?,Guest(0x69cdf8),dispatcher_code.to_vec(),maps.clone())?;
        machine.traces.stop_labels=initial.values().cloned().chain(["temporary-boundary".into()]).collect();
        let mut capture=Coordinator {route:Observation {flexible_origin:true,shared_owners:Some(owners.clone()),..Observation::default()},dispatcher,consumer,
            epochs:Vec::new(),catalog:&catalog,maps,output:&output,permanent,calls,carrier_calls,program:&program,
            timer:TimerWatch {start:Count(0),end:Count(0),range:TIMER_RANGE,limit:TIMER_LIMIT,batch:None,error:None},timer_active:false,consumer_segments:0,owners};
        let outcome=machine.traces.arm(&machine.session,initial).and_then(|()|capture.run(&mut machine));
        if capture.timer_active {if let Err(error)=capture.timer_end(&mut machine){capture.timer.error=Some(error);}}
        capture.consumer.finalize(&machine,outcome.as_ref().err().cloned());
        persist(&output.join("timer-transactions.json"),&capture.timer)?;persist(&output.join("trace.json"),&machine.traces.hits)?;
        let receipt=Receipt {schema:1,role,program:program.binding(),artifacts,identity,checkpoint:Digest::of(&checkpoint),instructions:Count(machine.instructions()),counts:counts(&machine),
            menu:machine.menu().map(|menu|menu.map(|menu|[menu.menu_id,menu.setting_id,menu.cursor])),error:outcome.as_ref().err().cloned(),machine_failure:machine.failure.clone(),
            route:Route {origin:capture.route.first_loader,events:&capture.route.events,calls:&capture.route.calls,snapshots:&capture.route.snapshots,pulses:&capture.route.pulses,static_calls:&capture.calls},
            epochs:&capture.epochs,dispatcher:&capture.dispatcher,consumer:&capture.consumer,trace:binding(&output,"trace.json")?,timer_transactions:binding(&output,"timer-transactions.json")?};
        let file=std::fs::File::create_new(output.join("startup-joined.json")).map_err(|error|error.to_string())?;
        let mut writer=std::io::BufWriter::new(file);
        serde_json::to_writer(&mut writer,&receipt).map_err(|error|error.to_string())?;
        writer.flush().map_err(|error|error.to_string())?;
        outcome
    })();
    if let Err(error)=&result {#[derive(Serialize)]struct Failure<'a>{schema:u32,error:&'a str,primary_receipt_present:bool}
        persist(&output.join("observer-error.json"),&Failure {schema:1,error,primary_receipt_present:output.join("startup-joined.json").is_file()})?;}
    result
}

