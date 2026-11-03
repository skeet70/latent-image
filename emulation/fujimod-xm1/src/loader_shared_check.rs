use super::*;
#[path="loader_shared_controls.rs"]
mod controls;
#[path="loader_shared_policy.rs"]
mod policy;
use super::super as owner;
use owner::loader_startup_program::{Bindings,JoinedProgram};
use owner::shared_dispatch::shared::{AccessRow,ByteOrder,DataAttribution,FetchStamp,Capture as Dispatcher,Rows as DispatcherRows};
use owner::shared_dispatch::Source as DispatchSource;
use std::collections::BTreeSet;

#[derive(Clone,Copy,Deserialize,Serialize,PartialEq,Eq)]
#[serde(rename_all="kebab-case")]
enum Role {Retained,Joined}
#[derive(Deserialize,Serialize)]
struct RouteCapture {origin:Option<owner::EventId>,events:Vec<Event>,calls:Vec<owner::Call>,snapshots:Vec<owner::Snapshot>,pulses:Vec<owner::Pulse>,static_calls:Vec<owner::StaticCall>}
#[derive(Clone,Deserialize,Serialize)]
#[serde(tag="kind",rename_all="kebab-case")]
enum Evidence {Watch {file:FileBinding,teardown_error:Option<String>},ConsumerSegment {segment:usize}}
#[derive(Clone,Deserialize,Serialize)]
struct Epoch {stage:owner::WatchStage,start:owner::Count,end:owner::Count,start_counts:[u64;2],end_counts:[u64;2],evidence:Evidence}
#[derive(Deserialize)]
struct Endpoint {instructions:u64,counts:[u64;2],trace_index:usize}
#[derive(Deserialize,PartialEq)]
#[serde(rename_all="kebab-case")]
enum Termination {FirstFamiliesAndSecondaryRoot1}
#[derive(Deserialize,PartialEq)]
#[serde(rename_all="kebab-case")]
enum Outcome {Observed,NonObservation,Refused}
#[derive(Deserialize)]
#[serde(rename_all="kebab-case")]
enum FetchScope {Complete,Scoped}
#[derive(Deserialize)]
struct Context {core:Core,pc:Guest,counts:[u64;2]}
#[derive(Deserialize)]
struct Stop {core:Core,pc:Guest,instructions:u64}
#[derive(Deserialize)]
struct Segment {id:u64,start_counts:[u64;2],end_counts:[u64;2],start_contexts:[Context;2],end_contexts:[Context;2],
    fetch_scope:FetchScope,fetch_ranges:Vec<[u64;2]>,armed_sites:Vec<Guest>,watch:ReplayWatch,end_error:Option<String>,stops:Vec<Stop>}
#[derive(Deserialize)]
struct Window {schema:u32,role:Role,outcome:Outcome,error:Option<String>,machine_failure:Option<String>,instructions:u64,segments:Vec<Segment>,fetch_attempts:Vec<FetchAttempt>}
#[derive(Deserialize,PartialEq,Eq)]
#[serde(rename_all="kebab-case")]
enum FetchClassification {BoundaryAttempt}
#[derive(Deserialize)]
struct FetchAttempt {segment:u64,row:usize,classification:FetchClassification}
#[derive(Deserialize)]
struct ReplayRow {
    #[serde(flatten)]
    legacy:owner::Row,
    #[serde(default)]
    core_instructions:Option<u64>,
    #[serde(default)]
    latest_fetch:Option<FetchStamp>,
    #[serde(default)]
    before:Option<Vec<u8>>,
    #[serde(default)]
    byte_order:Option<ByteOrder>,
}
impl std::ops::Deref for ReplayRow {type Target=owner::Row;fn deref(&self)->&Self::Target {&self.legacy}}
#[derive(Deserialize)]
struct ReplayWatch {enabled:bool,violation:Option<String>,views:owner::WatchViews,classes:Vec<owner::Class>,records:Vec<ReplayRow>}
#[derive(Default)]
struct FetchReplay {settled_fetches:BTreeSet<(Core,Guest,u64,u64)>}
impl FetchReplay {
    fn admit(&mut self,segment:&Segment,declared:&[FetchAttempt],events:&[Event])->Result<BTreeSet<usize>,String>{
        let begin=segment.start_counts[0].checked_add(segment.start_counts[1]).ok_or("fetch start clock overflows")?;
        let settled=segment.end_counts[0].checked_add(segment.end_counts[1]).ok_or("fetch end clock overflows")?;
        require(!segment.fetch_ranges.is_empty()&&segment.fetch_ranges.len()<=256&&segment.fetch_ranges.iter().all(|range|range[0]<range[1]&&range[1]<=1u64<<32)
            &&segment.fetch_ranges.windows(2).all(|pair|pair[0][1]<pair[1][0]),"fetch replay scope differs")?;
        let mut boundary_rows=BTreeSet::new();let mut cores=BTreeSet::new();
        for stop in &segment.stops {
            let core=usize::from(stop.core);
            require(cores.insert(core)&&stop.instructions==settled&&stop.pc==segment.end_contexts[core].pc
                &&segment.armed_sites.binary_search(&stop.pc).is_ok()
                &&events.iter().any(|event|event.core==stop.core&&event.pc==stop.pc&&event.instructions.0==settled),
                "fetch attempt lacks its stopped source endpoint")?;
            let (index,row)=segment.watch.records.iter().enumerate().rfind(|(_,row)|row.source==Access::CpuFetch
                &&row.core==Some(stop.core)&&row.pc==Some(stop.pc)&&row.instructions.is_some_and(|count|count.0==settled))
                .ok_or("stopped fetch attempt absent")?;
            let own=row.core_instructions.ok_or("stopped fetch own-core count absent")?;
            require(own==segment.end_counts[core]&&row.address==stop.pc&&row.size==4
                &&!self.settled_fetches.contains(&(stop.core,stop.pc,own,settled)),"fetch attempt contradicts or reuses an earlier observation")?;
            require(boundary_rows.insert(index),"stops share fetch evidence")?;
        }
        let mut asserted=BTreeSet::new();
        let lower=declared.partition_point(|attempt|attempt.segment<segment.id);
        let upper=declared.partition_point(|attempt|attempt.segment<=segment.id);
        for attempt in &declared[lower..upper] {
            require(attempt.classification==FetchClassification::BoundaryAttempt&&asserted.insert(attempt.row),"fetch attempt classification repeated")?;
        }
        require(asserted==boundary_rows,"fetch attempt classifications do not match actual stops")?;
        require(begin<settled||!boundary_rows.is_empty(),"zero-instruction segment lacks a new attempted fetch")?;
        let mut progress=segment.start_counts;let mut last_clock=begin;let mut last_tick=None;
        let mut resumes=[false;2];
        for core in 0..2 {
            require(segment.start_counts[core]<=segment.end_counts[core],"fetch own-core endpoint decreased")?;
            let base=u64::from(segment.start_contexts[core].pc.0&!0x40000000);
            resumes[core]=segment.start_counts[core]<segment.end_counts[core]
                &&segment.fetch_ranges.iter().any(|range|range[0]<=base&&base+4<=range[1]);
        }
        for (index,row) in segment.watch.records.iter().enumerate().filter(|(_,row)|row.source==Access::CpuFetch) {
            let core=row.core.ok_or("fetch core absent")?;let cpu=usize::from(core);
            let clock=row.instructions.ok_or("fetch clock absent")?.0;
            let own=row.core_instructions.ok_or("fetch own-core count absent")?;
            let tick=row.tick.ok_or("fetch tick absent")?.0;let pc=row.pc.ok_or("fetch PC absent")?;
            let end=u64::from(row.physical.0).checked_add(row.size).ok_or("fetch extent overflows")?;
            let peer=clock.checked_sub(own).ok_or("fetch own-core count exceeds global clock")?;
            require(row.size!=0&&segment.fetch_ranges.iter().any(|range|range[0]<=u64::from(row.physical.0)&&end<=range[1])
                &&clock>=last_clock&&clock<=settled&&own<=segment.end_counts[cpu]
                &&peer>=progress[1-cpu]&&peer<=segment.end_counts[1-cpu]&&last_tick.is_none_or(|before|before<=tick),
                "fetch own-core clocks or scope contradict endpoints")?;
            last_tick=Some(tick);
            if boundary_rows.contains(&index){continue;}
            if resumes[cpu] {
                require(pc==segment.start_contexts[cpu].pc&&segment.start_counts[cpu].checked_add(1)==Some(own),
                    "fetch resumption does not advance actual own-core clock")?;resumes[cpu]=false;
            }
            require(clock>last_clock&&own>progress[cpu],"executed fetch does not advance clocks")?;
            if matches!(segment.fetch_scope,FetchScope::Complete) {
                require(progress[0].checked_add(progress[1]).and_then(|clock|clock.checked_add(1))==Some(clock)
                    &&progress[cpu].checked_add(1)==Some(own),"complete fetch replay does not advance exact scheduler")?;
            }
            progress[cpu]=own;progress[1-cpu]=peer;last_clock=clock;
        }
        require(!resumes.into_iter().any(|pending|pending),"executed in-scope resumption absent")?;
        if matches!(segment.fetch_scope,FetchScope::Complete){require(progress==segment.end_counts,"complete fetch replay does not reconcile actual per-core endpoints")?;}
        self.settled_fetches.retain(|(_,_,_,clock)|*clock==settled);
        for row in segment.watch.records.iter().filter(|row|row.source==Access::CpuFetch&&row.instructions.is_some_and(|clock|clock.0==settled)) {
            self.settled_fetches.insert((row.core.ok_or("fetch core absent")?,row.pc.ok_or("fetch PC absent")?,
                row.core_instructions.ok_or("fetch own-core count absent")?,row.instructions.ok_or("fetch clock absent")?.0));
        }
        Ok(boundary_rows)
    }
}
#[derive(Deserialize)]
struct Consumer {begin:Option<Endpoint>,end:Option<Endpoint>,termination:Option<Termination>,window:Window}
#[derive(Deserialize)]
struct Capture {
    schema:u32,role:Role,program:owner::ProgramBinding,artifacts:Bindings,identity:owner::InputIdentity,checkpoint:Digest,
    instructions:owner::Count,counts:Result<[u64;2],String>,menu:Result<Option<[u32;3]>,String>,error:Option<String>,machine_failure:Option<String>,
    route:RouteCapture,epochs:Vec<Epoch>,dispatcher:Dispatcher,consumer:Consumer,trace:FileBinding,timer_transactions:FileBinding,
}
impl Capture {
    fn route(&self)->RouteView<'_>{RouteView {origin:self.route.origin,events:&self.route.events,calls:&self.route.calls,snapshots:&self.route.snapshots,
        pulses:&self.route.pulses,instructions:self.instructions,menu:&self.menu,timer_transactions:&self.timer_transactions}}
}
struct Checked {capture:Capture,receipt:Digest,reset:loader_reset_check::Reset,timer:loader_timer_check::Admission,
    loaders:Vec<LoaderEffect>,publications:Vec<Publication>,dispatcher:owner::shared_dispatch::shared::Checked,policy:policy::Policy}
#[derive(Serialize)]
#[serde(rename_all="kebab-case")]
enum HostScope {SharedRouteAndDispatcher}
#[derive(Serialize)]
struct Verification {schema:u32,scope:HostScope,accepted:bool,error:Option<String>,receipt:Option<Digest>,role:Option<Role>,route_effects:Option<FileBinding>,dispatcher_effects:Option<FileBinding>,observation_policy:Option<FileBinding>}
fn scope(stage:owner::WatchStage)->Vec<([u64;2],Vec<Access>)>{
    watch_scope(owner::startup_joined::guard_classes(stage))
}
fn watch_scope(ranges:Vec<owner::WatchRange>)->Vec<([u64;2],Vec<Access>)>{
    let sources=[(fujimod_emulator::board::observation::WatchKinds::CPU_FETCH,Access::CpuFetch),
        (fujimod_emulator::board::observation::WatchKinds::CPU_READ,Access::CpuRead),
        (fujimod_emulator::board::observation::WatchKinds::CPU_WRITE,Access::CpuWrite),
        (fujimod_emulator::board::observation::WatchKinds::HOST_WRITE,Access::HostWrite),
        (fujimod_emulator::board::observation::WatchKinds::MODEL_BUS_WRITE,Access::ModelBusWrite)];
    ranges.into_iter().map(|range|([range.start,range.end],sources.iter().filter(|(kind,_)|range.kinds.contains(*kind)).map(|(_,source)|*source).collect())).collect()
}
fn covered(classes:&[owner::Class],span:[u64;2],kind:Access)->bool {
    let mut intervals:Vec<_>=classes.iter().filter(|class|class.kinds.contains(&kind)).map(|class|class.range).collect();
    intervals.sort_unstable();let mut cursor=span[0];
    for range in intervals {if range[0]>cursor {break;}if range[1]>cursor {cursor=range[1];}if cursor>=span[1]{return true;}}
    false
}
fn dispatch_source(source:Access)->DispatchSource {match source {Access::CpuFetch=>DispatchSource::CpuFetch,Access::CpuRead=>DispatchSource::CpuRead,
    Access::CpuWrite=>DispatchSource::CpuWrite,Access::HostRead=>DispatchSource::HostRead,Access::HostWrite=>DispatchSource::HostWrite,
    Access::ModelBusRead=>DispatchSource::ModelBusRead,Access::ModelBusWrite=>DispatchSource::ModelBusWrite}}
fn inspect_watch(raw:&ReplayWatch,epoch:&Epoch,report:RouteView<'_>,program:&Program,loaders:&[LoaderBounds<'_>;2],
    missing:&mut BTreeSet<(Core,Guest,u64)>,publications:&mut Vec<Publication>,timer_rows:&mut Vec<owner::Row>,dispatcher:&mut DispatcherRows,attempts:&BTreeSet<usize>)->Result<(),String>{
    require(raw.enabled&&raw.violation.is_none(),"shared classified watch refused or disabled")?;
    for (span,kinds) in scope(epoch.stage){for kind in kinds {require(covered(&raw.classes,span,kind),"shared route guard coverage absent")?;}}
    let bootstrap=epoch.stage==owner::WatchStage::Bootstrap;
    require(raw.views==if bootstrap {owner::WatchViews::Observe}else{owner::WatchViews::Fixed},"shared watch views differ")?;
    if bootstrap {for loader in loaders {require(raw.records.iter().filter(|row|loader_copy(row,loader.copy)).count()==1,"shared bootstrap loader copy census differs")?;}}
    let mut previous=epoch.start.0;let mut lower=None;
    let mut future=raw.records.iter().enumerate().filter(|(_,row)|matches!(row.source,Access::CpuRead|Access::CpuFetch|Access::CpuWrite)).peekable();
    for (index,row) in raw.records.iter().enumerate(){
        require(in_class(row,&raw.classes)?,"shared memory access exceeds declared classes")?;
        let end=u64::from(row.physical.0).checked_add(row.size).ok_or("shared memory extent overflow")?;
        let mut terminal_attempt=false;
        if matches!(row.source,Access::CpuRead|Access::CpuFetch|Access::CpuWrite){
            let at=row.instructions.ok_or("shared CPU access clock absent")?.0;
            let stopped=|clock:u64|report.events.iter().any(|event|event.instructions.0==clock&&Some(event.core)==row.core&&Some(event.pc)==row.pc&&event.pc==row.address);
            let boundary_attempt=row.source==Access::CpuFetch&&at==epoch.start.0&&stopped(at);
            // Points rearmed after the watch (any add_sites/remove_sites) record the rolled-back fetch of a stop one past its settled count
            // (tests/peer_wait_sdk.rs points_rearmed_after_a_fetch_watch_*); points armed before it record the settled count (heap_observe.rs:382-389).
            // The stop ending the epoch is the only such fetch without its executed retry; the next epoch records the execution.
            terminal_attempt=row.source==Access::CpuFetch&&Some(at)==epoch.end.0.checked_add(1)&&stopped(epoch.end.0);
            require(row.core.is_some()&&row.pc.is_some()&&(epoch.start.0<at||boundary_attempt)&&(at<=epoch.end.0||terminal_attempt)&&previous<=at,"shared CPU access attribution differs")?;
            previous=at;lower=Some(at);require(future.next().is_some_and(|(position,_)|position==index),"shared CPU chronology differs")?;
            require(!terminal_attempt||future.peek().is_none(),"shared terminal fetch attempt is not the last CPU access")?;
        }
        let upper=future.peek().and_then(|(_,row)|row.instructions).map(|count|count.0);
        if row.source==Access::CpuFetch {
            require(owner::DIAGNOSTICS.iter().all(|(low,high)|end<=u64::from(*low)||u64::from(*high)<=u64::from(row.physical.0)),"diagnostic instruction fetched in shared route")?;
        }
        let write=matches!(row.source,Access::CpuWrite|Access::HostWrite|Access::ModelBusWrite);
        if write&&PUBLICATIONS.iter().any(|span|u64::from(row.physical.0)<span[1]&&span[0]<end){
            publications.push(Publication {source:row.source,address:row.address,physical:row.physical,size:row.size,core:row.core,value:row.value,clock:row.instructions,
                writer:match row.pc {Some(pc)=>carrier_side(pc).map_or(Writer::Retained(pc),Writer::Carrier),None=>Writer::Untimed}});
        }
        if epoch.stage==owner::WatchStage::Startup&&write {require([[0x41ad4,0x41aec],[0x41b78,0x41b84]].iter().all(|span|end<=span[0]||span[1]<=u64::from(row.physical.0)),"shared timer source code overwritten")?;}
        for loader in loaders {
            if u64::from(row.physical.0)<loader.code[1]&&loader.code[0]<end {match row.source {
                Access::CpuFetch=>{
                    require(!bootstrap&&row.size==4&&row.address==row.physical&&row.pc==Some(row.address)&&row.core==Some(loader.core)
                        &&row.address.0%4==0&&program.execution_allowed(row.address),"shared carrier fetch is outside admitted source")?;
                    if !terminal_attempt {missing.remove(&(loader.core,row.address,row.instructions.ok_or("carrier fetch clock absent")?.0));}
                },Access::CpuWrite|Access::HostWrite|Access::ModelBusWrite=>require(bootstrap&&loader_copy(row,loader.copy)&&upper.is_some_and(|at|at<loader.entry.instructions.0),"shared carrier write lacks earlier bootstrap copy")?,_=>{},
            }}
            if u64::from(row.physical.0)<loader.frame[1]&&loader.frame[0]<end {
                if matches!(row.source,Access::HostWrite|Access::ModelBusWrite){require(upper.is_some_and(|at|at<loader.entry.instructions.0)||lower.is_some_and(|at|at>loader.transfer.instructions.0),"unattributed writer overlaps live loader frame")?;}
                if write&&row.instructions.is_some_and(|at|loader.entry.instructions.0<=at.0&&at.0<=loader.transfer.instructions.0){
                    require(row.source==Access::CpuWrite&&row.pc==Some(loader.entry.pc)&&row.core==Some(loader.core),"foreign writer reached live loader frame")?;
                }
            }
        }
        if loader_timer_check::relevant(row){timer_rows.push(row.legacy);}
        if terminal_attempt {continue;}
        let attribution=if row.source==Access::CpuRead&&row.pc==Some(Guest(0x657194)) {
            row.core_instructions.map(|own|DataAttribution {own,
                before:row.before.as_deref().and_then(|bytes|bytes.try_into().ok()),byte_order:row.byte_order,latest:row.latest_fetch.clone()})
        }else{None};
        let attempted=attempts.contains(&index) || (matches!(epoch.evidence,Evidence::Watch {..}) && row.source==Access::CpuFetch
            &&report.events.iter().any(|event|row.core==Some(event.core)&&row.pc==Some(event.pc)&&row.instructions==Some(event.instructions)));
        dispatcher.push(AccessRow {source:dispatch_source(row.source),address:row.address,physical:row.physical,size:row.size,
            core:row.core.map(usize::from),pc:row.pc,instructions:row.instructions.map(|count|count.0),tick:row.tick.map(|tick|tick.0),value:row.value,
            own:row.core_instructions,order:row.byte_order,attribution,attempt:attempted,interval:[epoch.start.0,epoch.end.0]})?;
    }
    Ok(())
}
fn endpoint(endpoint:&Endpoint,events:&[Event])->Result<(),String>{
    require(endpoint.counts[0].checked_add(endpoint.counts[1])==Some(endpoint.instructions)&&endpoint.trace_index!=0&&endpoint.trace_index<=events.len(),"consumer endpoint clocks or trace index differ")?;
    require(events[endpoint.trace_index-1].instructions.0==endpoint.instructions,"consumer endpoint lacks included stopped trace event")
}
fn verified(root:&Path,analysis:&Path)->Result<Checked,String>{
    let bytes=read(&root.join("startup-joined.json"))?;let receipt=Digest::of(&bytes);
    let capture:Capture=serde_json::from_slice(&bytes).map_err(|error|error.to_string())?;
    require(capture.schema==1&&capture.error.is_none()&&capture.machine_failure.is_none(),"shared observation did not complete")?;
    let actual_counts=capture.counts.as_ref().map_err(|_|"shared final core clocks absent")?;
    require(actual_counts[0].checked_add(actual_counts[1])==Some(capture.instructions.0)&&capture.instructions.0<=450_000_000,"shared final clocks exceed route cap")?;
    let mut catalog=Catalog::open(analysis)?;
    let (joined,_)=JoinedProgram::from_binding(capture.artifacts.clone(),root,&mut catalog)?;
    let (program,facts)=match capture.role {
        Role::Joined=>{let Program::Joined(proof)=&joined else{return Err("joined program proof absent".into())};
            let facts=proof.dispatcher_calls.clone();require(serde_json::to_vec(&capture.program).map_err(|error|error.to_string())?==serde_json::to_vec(&joined.binding()).map_err(|error|error.to_string())?,"joined program binding differs")?;(joined,facts)},
        Role::Retained=>{require(matches!(capture.program,owner::ProgramBinding::Retained),"retained program role differs")?;catalog=Catalog::open(analysis)?;(Program::Retained,Vec::new())},
    };
    let identity=&capture.identity;
    require(identity.update.as_str()==program.update()&&identity.run.as_str()==Digest::of(CANONICAL_RUN).as_str()
        &&identity.rom.as_str()=="4ecedfbfc1910bde488d36abe4380621f8845e6afcfc6cf54a49a33ec0abf9be","shared input program identity differs")?;
    require(identity.sources.len()==catalog.0.len(),"shared effective source catalog census differs")?;
    for image in &catalog.0 {require(identity.sources.iter().filter(|source|source.kind==image.binding.kind&&source.start==image.binding.start
        &&source.bytes.0==image.binding.bytes.0&&source.sha256.as_str()==image.binding.sha256.as_str()).count()==1,"shared effective source binding differs")?;}
    let document=fujimod_xm1::run::RunDocument::parse(CANONICAL_RUN)?;
    let update=read(&artifact(root,&identity.update_file)?)?;
    require(Digest::of(&update).as_str()==identity.update.as_str()&&identity.update_file.sha256.as_str()==identity.update.as_str(),"shared actual DAT binding differs")?;
    let media=fujimod_xm1::run::nand_base(&document,&fujimod_xm1::run::ValidatedUpdate(update))?;
    let reset_bytes=read(&root.join("reset.fujimod"))?;require(Digest::of(&reset_bytes).as_str()==capture.checkpoint.as_str(),"shared reset checkpoint identity differs")?;
    let reset=loader_reset_check::admit(&reset_bytes,&document,&media)?;
    let report=capture.route();route(report)?;
    let expected_calls=owner::startup_calls(&catalog)?;
    require(serde_json::to_vec(&expected_calls).map_err(|error|error.to_string())?==serde_json::to_vec(&capture.route.static_calls).map_err(|error|error.to_string())?,"shared source graph differs")?;
    let trace_bytes=read(&artifact(root,&capture.trace)?)?;require(Digest::of(&trace_bytes).as_str()==capture.trace.sha256.as_str(),"shared trace identity differs")?;
    let trace:Vec<Hit>=serde_json::from_slice(&trace_bytes).map_err(|error|error.to_string())?;
    require(trace.len()==report.events.len()&&trace.len()<131_072,"shared raw/event trace coverage differs")?;
    let mut previous=0;
    for (index,(hit,event)) in trace.iter().zip(report.events).enumerate(){
        require(event.id.0==index&&previous<=event.instructions.0&&event.instructions.0<=capture.instructions.0,"shared event chronology differs")?;previous=event.instructions.0;
        require(hit.address.0==event.pc.0&&hit.core==event.core&&hit.instructions==event.instructions&&hit.cpsr==event.registers.cpsr&&hit.sp.0==event.registers.sp.0&&hit.lr.0==event.registers.lr.0&&hit.registers.len()==13,"shared raw boundary differs")?;
        for number in 0..13u8 {require(hit.registers.get(&RegisterNumber(number)).is_some_and(|value|value.0==event.registers.r[usize::from(number)]),"shared raw register differs")?;}
        let (source,expected)=catalog.word(event.pc)?;words(&event.code)?;
        require(source==event.source&&event.code.mapping.guest==event.pc&&event.code.values==[expected]&&event.registers.cpsr&0x20==0&&program.execution_allowed(event.pc),"shared entered source differs")?;
        for sample in event.state.iter().chain(event.descriptor.iter()).chain(event.payload.iter()){words(sample)?;}if let Ok(frame)=&event.frame{words(frame)?;}
        require(!owner::diagnostics(event.pc)&&event.pc.0!=0x69d2d8&&(!matches!(event.pc.0,0x40aa8|0xd0aa8)||program.owned()),"shared route reached refused source")?;
    }
    let origin=event(report,report.origin.ok_or("shared loader origin absent")?)?;
    require(origin.pc==Guest(0xd09ac)&&origin.core==Core::Secondary&&origin.instructions.0<8_000_000
        &&report.events.iter().find(|event|carrier_side(event.pc).is_some()).is_some_and(|event|event.id.0==origin.id.0),"shared first-carrier origin differs")?;
    let loaders=[loader_bounds_until(report,Guest(0x40000),Guest(0x63450),Core::Primary,80_000_000)?,loader_bounds_until(report,Guest(0xd0000),Guest(0xf3410),Core::Secondary,80_000_000)?];
    let consumer=&capture.consumer;
    require(consumer.window.schema==1&&consumer.window.role==capture.role&&consumer.window.outcome==Outcome::Observed
        &&consumer.window.error.is_none()&&consumer.window.machine_failure.is_none()&&consumer.termination==Some(Termination::FirstFamiliesAndSecondaryRoot1),"shared consumer subobservation did not close")?;
    let begin=consumer.begin.as_ref().ok_or("shared consumer begin absent")?;let end=consumer.end.as_ref().ok_or("shared consumer end absent")?;
    endpoint(begin,report.events)?;endpoint(end,report.events)?;
    require(begin.instructions<end.instructions&&end.instructions<=80_000_000&&consumer.window.instructions==end.instructions
        &&begin.trace_index<=end.trace_index&&report.events[..begin.trace_index].iter().any(|event|event.core==Core::Primary
            &&event.pc==Guest(0x69cdf8)&&event.registers.r[0]==0&&event.instructions.0==begin.instructions),"shared consumer endpoint is not the outer0 window")?;
    require(!consumer.window.segments.is_empty()&&consumer.window.segments.len()<=32_768,"shared consumer segment census differs")?;
    let mut publications=Vec::new();let mut timer_rows=Vec::new();let mut dispatcher_rows=DispatcherRows::default();
    let mut missing=BTreeSet::new();for event in report.events.iter().filter(|event|carrier_side(event.pc).is_some()){
        require(origin.instructions<=event.instructions&&event.instructions.0<80_000_000,"shared loader continued beyond startup cap")?;
        missing.insert((event.core,event.pc,event.instructions.0.checked_add(1).ok_or("shared carrier fetch clock overflow")?));
    }
    require(!capture.epochs.is_empty()&&capture.epochs.len()<=32_864,"shared route epoch cap differs")?;
    let mut cursor=0;let mut core_cursor=[0,0];let mut segment_cursor=0;let mut records=0usize;let mut split=false;
    let mut replay=FetchReplay::default();
    require(consumer.window.fetch_attempts.len()<=2*consumer.window.segments.len()
        &&consumer.window.fetch_attempts.iter().all(|attempt|attempt.segment<(consumer.window.segments.len() as u64))
        &&consumer.window.fetch_attempts.windows(2).all(|pair|(pair[0].segment,pair[0].row)<(pair[1].segment,pair[1].row)),
        "shared attempted-fetch census exceeds segment endpoints")?;
    for epoch in &capture.epochs {
        require(epoch.start.0==cursor&&epoch.end.0>=cursor&&epoch.start_counts==core_cursor
            &&epoch.start_counts[0].checked_add(epoch.start_counts[1])==Some(epoch.start.0)&&epoch.end_counts[0].checked_add(epoch.end_counts[1])==Some(epoch.end.0)
            &&epoch.start_counts.iter().zip(epoch.end_counts).all(|(start,end)|*start<=end),"shared watch gap or endpoint clock differs")?;
        require(if cursor==0 {epoch.stage==owner::WatchStage::Bootstrap&&epoch.end==origin.instructions}
            else if cursor<8_000_000 {epoch.stage==owner::WatchStage::Startup&&epoch.end.0<=8_000_000}
            else {epoch.stage==owner::WatchStage::Tail},"shared epoch stage or8M split differs")?;
        if epoch.end.0==8_000_000{split=true;}
        match &epoch.evidence {
            Evidence::Watch {file,teardown_error}=>{
                require(teardown_error.is_none(),"shared route watch teardown failed")?;
                require(epoch.end.0>epoch.start.0,"zero-instruction route watch has no consumer stop evidence")?;
                require(epoch.end.0<=begin.instructions||epoch.start.0>=end.instructions,"separate route watch overlaps consumer subobservation")?;
                let bytes=read(&artifact(root,file)?)?;require(Digest::of(&bytes).as_str()==file.sha256.as_str(),"shared watch file binding differs")?;
                let raw:ReplayWatch=serde_json::from_slice(&bytes).map_err(|error|error.to_string())?;
                require(raw.records.len()<=3_000_000,"shared route watch record cap exceeded")?;
                inspect_watch(&raw,epoch,report,&program,&loaders,&mut missing,&mut publications,&mut timer_rows,&mut dispatcher_rows,&BTreeSet::new())?;
            },Evidence::ConsumerSegment {segment}=>{
                require(*segment==segment_cursor,"shared consumer segment reference is missing or repeated")?;
                let observed=consumer.window.segments.get(*segment).ok_or("shared consumer segment absent")?;
                require(observed.id==*segment as u64&&observed.start_counts==epoch.start_counts&&observed.end_counts==epoch.end_counts
                    &&observed.end_error.is_none()&&observed.watch.records.len()<=65_536,"shared consumer segment endpoints or cap differ")?;
                let attempts=replay.admit(observed,&consumer.window.fetch_attempts,report.events)?;
                for (core,(start,end)) in observed.start_contexts.iter().zip(&observed.end_contexts).enumerate(){
                    require(usize::from(start.core)==core&&usize::from(end.core)==core&&start.counts==epoch.start_counts&&end.counts==epoch.end_counts,"shared consumer context clocks differ")?;
                }
                require(epoch.start.0>=begin.instructions&&epoch.end.0<=end.instructions
                    &&epoch.end.0-epoch.start.0<=match observed.fetch_scope {FetchScope::Complete=>256,FetchScope::Scoped=>4096},"shared consumer cadence or subobservation lifetime differs")?;
                if *segment==0 {require(observed.start_counts==begin.counts&&observed.start_contexts[0].pc==Guest(0x69cdf8),"shared consumer first segment differs from begin")?;}
                if *segment+1==consumer.window.segments.len(){require(observed.end_counts==end.counts,"shared consumer last segment differs from end")?;}
                records=records.checked_add(observed.watch.records.len()).ok_or("shared consumer aggregate record overflow")?;
                require(records<=1_048_576,"shared consumer aggregate record cap exceeded")?;
                inspect_watch(&observed.watch,epoch,report,&program,&loaders,&mut missing,&mut publications,&mut timer_rows,&mut dispatcher_rows,&attempts)?;segment_cursor+=1;
            },
        }
        cursor=epoch.end.0;core_cursor=epoch.end_counts;
    }
    require(cursor==capture.instructions.0&&core_cursor==*actual_counts&&split&&segment_cursor==consumer.window.segments.len()&&missing.is_empty(),"shared route evidence is incomplete")?;
    let timer=loader_timer_check::admit_interval(root,report,&timer_rows,origin.instructions,owner::Count(8_000_000))?;
    let (_,_,effects)=semantic_calls(root,report,&catalog,&publications)?;
    let callers=report.calls.iter().filter(|call|call.kind==CallKind::Loader&&call.target==Guest(0x69cdf8)).map(|call|{
        let entered=event(report,call.entry.ok_or("dispatcher loader entry absent")?)?;Ok((usize::from(call.core),entered.registers.r[0],call.continuation.0))
    }).collect::<Result<Vec<_>,String>>()?;
    let image4=catalog.0.iter().find(|image|image.binding.kind==owner::SourceKind::Image4).ok_or("effective image4 absent")?;
    let code=image4.bytes.get(0x69cdf8-0x61c400..0x69cf84-0x61c400).ok_or("dispatcher code extent absent")?;
    let dispatcher=capture.dispatcher.verify(&dispatcher_rows,&trace_bytes,code,&callers,&facts,program.owned(),capture.instructions.0)?;
    let policy=policy::derive(&capture,&receipt,&catalog,&program,&expected_calls)?;
    Ok(Checked {capture,receipt,reset,timer,loaders:effects,publications,dispatcher,policy})
}
fn write_effects(output:&Path,prefix:&str,checked:&Checked)->Result<(FileBinding,FileBinding),String>{
    let route=serde_json::to_vec_pretty(&projected_effects(checked.capture.route(),&checked.loaders,&checked.publications,&checked.timer)?).map_err(|error|error.to_string())?;
    let dispatch=serde_json::to_vec_pretty(&checked.dispatcher).map_err(|error|error.to_string())?;
    Ok((persist_bound(output,&format!("{prefix}route-effects.json"),&route)?,persist_bound(output,&format!("{prefix}dispatcher-effects.json"),&dispatch)?))
}
fn write_policy(output:&Path,prefix:&str,checked:&Checked)->Result<FileBinding,String>{
    persist_bound(output,&format!("{prefix}observation-policy.json"),&checked.policy.encode()?)
}
pub(crate) fn check(root:&Path,analysis:&Path,output:&Path)->Result<(),String>{
    std::fs::create_dir(output).map_err(|error|error.to_string())?;
    let result=verified(root,analysis).and_then(|checked|{let (route,dispatcher)=write_effects(output,"",&checked)?;let policy=write_policy(output,"",&checked)?;
        Ok(Verification {schema:1,scope:HostScope::SharedRouteAndDispatcher,accepted:true,error:None,receipt:Some(checked.receipt),role:Some(checked.capture.role),route_effects:Some(route),dispatcher_effects:Some(dispatcher),observation_policy:Some(policy)})});
    let report=match result {Ok(report)=>report,Err(error)=>Verification {schema:1,scope:HostScope::SharedRouteAndDispatcher,accepted:false,error:Some(error),receipt:None,role:None,route_effects:None,dispatcher_effects:None,observation_policy:None}};
    owner::persist(&output.join("shared-verification.json"),&report)?;
    match report.error {Some(error)=>Err(error),None=>Ok(())}
}
/// Admits a capture directory and refuses its `observation-policy.json` unless it equals the fresh derivation.
fn supplied_policy(root:&Path,analysis:&Path)->Result<(),String>{
    let checked=verified(root,analysis)?;
    policy::admit(&read(&root.join("observation-policy.json"))?,&checked.policy)
}
#[derive(Serialize)]
struct Comparison {schema:u32,scope:HostScope,accepted:bool,error:Option<String>,retained:Option<Digest>,joined:Option<Digest>,retained_route_effects:Option<FileBinding>,joined_route_effects:Option<FileBinding>,retained_dispatcher_effects:Option<FileBinding>,joined_dispatcher_effects:Option<FileBinding>,retained_observation_policy:Option<FileBinding>,joined_observation_policy:Option<FileBinding>}
pub(crate) fn compare(retained:&Path,joined:&Path,analysis:&Path,output:&Path)->Result<(),String>{
    std::fs::create_dir(output).map_err(|error|error.to_string())?;
    let mut comparison=Comparison {schema:1,scope:HostScope::SharedRouteAndDispatcher,accepted:false,error:None,retained:None,joined:None,retained_route_effects:None,joined_route_effects:None,retained_dispatcher_effects:None,joined_dispatcher_effects:None,retained_observation_policy:None,joined_observation_policy:None};
    let result=(||->Result<(),String>{
        let left=verified(retained,analysis)?;comparison.retained=Some(left.receipt.clone());
        let right=verified(joined,analysis)?;comparison.joined=Some(right.receipt.clone());
        require(left.capture.role==Role::Retained&&right.capture.role==Role::Joined,"shared comparison roles differ")?;
        require(left.capture.identity.run.as_str()==right.capture.identity.run.as_str()&&left.capture.identity.rom.as_str()==right.capture.identity.rom.as_str()
            &&left.capture.identity.executable.as_str()==right.capture.identity.executable.as_str(),"shared comparison observer or reset identity differs")?;
        loader_reset_check::compare(&left.reset,&right.reset)?;
        let (lr,ld)=write_effects(output,"retained-",&left)?;let (rr,rd)=write_effects(output,"joined-",&right)?;
        let equal=lr.sha256.as_str()==rr.sha256.as_str()&&ld.sha256.as_str()==rd.sha256.as_str();
        comparison.retained_route_effects=Some(lr);comparison.joined_route_effects=Some(rr);comparison.retained_dispatcher_effects=Some(ld);comparison.joined_dispatcher_effects=Some(rd);
        require(equal,"shared route or dispatcher public effects differ")?;
        comparison.retained_observation_policy=Some(write_policy(output,"retained-",&left)?);
        comparison.joined_observation_policy=Some(write_policy(output,"joined-",&right)?);
        Ok(())
    })();
    comparison.accepted=result.is_ok();comparison.error=result.as_ref().err().cloned();
    owner::persist(&output.join("shared-comparison.json"),&comparison)?;result
}


pub(crate) fn refusal_controls(retained:&Path,joined:&Path,analysis:&Path,output:&Path)->Result<(),String>{
    controls::run(retained,joined,analysis,output)
}
