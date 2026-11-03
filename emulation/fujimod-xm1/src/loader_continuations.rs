use fujimod_emulator::board::{bus::TransactionBatch, factory::MemoryMap, observation::{ViewPolicy, WatchKinds, WatchRange}};
use fujimod_emulator_cpu::{Register, arm};
use fujimod_xm1::{bridge::{read, write}, guest::RouteKey, machine::Machine, provenance::Guest, run::{self, RunDocument, ValidatedUpdate}};
use serde::{Deserialize, Serialize};
use std::{collections::{BTreeMap, BTreeSet}, num::NonZeroUsize, path::{Path, PathBuf}, process::ExitCode};

mod continuation_view;
mod loader_continuations_check;
mod application_pump;
mod loader_orchestration_owned;
mod plain_program;
mod plain_wire;
mod plain_runtime;
mod plain_check;
use loader_orchestration_owned::{Program, ProgramBinding, Side};
use continuation_view::{ByteCount, Digest, EventId, FileBinding, Resource, Snapshot, Words};

const USAGE: &str = "loader-continuations --run RUN --update DAT --boot-rom ROM --analysis ROOT [--owned-artifact STAGED_DIR | --plain-witness retained|owned [--plain-artifact DIR]] --output NEW_DIR; --verify DIR | --verify-plain DIR --analysis ROOT; --compare-retained DIR --compare-owned DIR | --compare-plain-retained DIR --compare-plain-owned DIR --analysis ROOT --output NEW_DIR; --plain-controls DIR --analysis ROOT --output NEW_DIR; --source-calls NEW_DIR --analysis ROOT; --application-pump retained|owned --original-update ORIGINAL_DAT --update ACTUAL_DAT --pump-code BIN --pump-symbols SYMBOLS --pump-admission CONTRACT --run RUN --boot-rom ROM --analysis ROOT --output NEW_APP154_DIR; --verify-application-pump DIR --analysis ROOT; --compare-application-pump RETAINED --owned-pump OWNED --analysis ROOT --output NEW_APP154_DIR; --controls-application-pump OWNED --analysis ROOT --output NEW_APP154_DIR";
const TRACE_LIMIT: usize = 16384;
const DIAGNOSTICS: [(u32, u32); 7] = [(0x40cac,0x40d50),(0xd0cac,0xd0d50),(0x40d70,0x40d98),(0xd0d70,0xd0d98),(0x69cc6c,0x69cd58),(0xed468c,0xed485c),(0xed4a8c,0xed4a9c)];

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "kebab-case")]
enum SourceKind { BootA, BootB, LoaderA, LoaderB, Image4, Image5, Plain }
struct SourceSpec { kind: SourceKind, path: &'static str, start: Guest, sha256: &'static str }
const SOURCES: [SourceSpec;7] = [
    SourceSpec {kind:SourceKind::BootA,path:"decode/boot-40048.bin",start:Guest(0),sha256:"c2da900dcd3f193b05b5ae968dfcb87723571462eafd84c0fed8467f66f7dff3"},
    SourceSpec {kind:SourceKind::BootB,path:"decode/boot-50048.bin",start:Guest(0x10000),sha256:"2f1bf3f0e483b892d2890599679c1b515cfc1be8b8c27cbb06d4d913e428abb4"},
    SourceSpec {kind:SourceKind::LoaderA,path:"decode/loader-40000.bin",start:Guest(0x40000),sha256:"21366ffff7cebc6176b8a8125bf3a9974835a51070a58ea325f7eed8aabd8882"},
    SourceSpec {kind:SourceKind::LoaderB,path:"decode/loader-d0000.bin",start:Guest(0xd0000),sha256:"8fb0c322a22443a3783cdab9071db17212a5629f6375646218ac1e043dc672ec"},
    SourceSpec {kind:SourceKind::Image4,path:"lz/id4-0061c400.bin",start:Guest(0x61c400),sha256:"f2e2acab892dafeed85171f6870a411f0a2d5eae10921b86a24630d401dcff16"},
    SourceSpec {kind:SourceKind::Image5,path:"lz/id5-00bf1a00.bin",start:Guest(0xbf1a00),sha256:"3cd501c07533938ae4d9347616986fd1649005a588c36b5abf05e36ef4fd892a"},
    SourceSpec {kind:SourceKind::Plain,path:"lz/plain-01100000.bin",start:Guest(0x1100000),sha256:"ee211bdaad39005b4f72e3443a76ca194905381a967cab787d6a4ef14e2d889d"},
];
#[derive(Clone, Debug, Deserialize, Serialize)]
struct SourceBinding { kind: SourceKind, start: Guest, bytes: ByteCount, sha256: Digest }
struct SourceImage { binding: SourceBinding, bytes: Vec<u8> }
struct Catalog(Vec<SourceImage>);
impl Catalog {
    fn open(root:&Path)->Result<Self,String>{
        let mut images=Vec::with_capacity(SOURCES.len());
        for spec in SOURCES {let bytes=read(&root.join(spec.path))?;let digest=Digest::of(&bytes);if digest.as_str()!=spec.sha256{return Err(format!("canonical {:?} source differs",spec.kind));}images.push(SourceImage {binding:SourceBinding {kind:spec.kind,start:spec.start,bytes:ByteCount(u32::try_from(bytes.len()).map_err(|_|"source extent exceeds word")?),sha256:digest},bytes});}
        Ok(Self(images))
    }
    fn word(&self,address:Guest)->Result<(SourceKind,u32),String>{
        let physical=address.0&!0x40000000;
        for image in &self.0 {if let Some(offset)=physical.checked_sub(image.binding.start.0){if let Some(bytes)=image.bytes.get(offset as usize..offset as usize+4){return Ok((image.binding.kind,u32::from_le_bytes(bytes.try_into().map_err(|_|"source word extent")?)));}}}
        Err(format!("instruction {:#x} is outside the canonical source catalog",address.0))
    }
    fn expected(&self,resource:Resource)->Result<Option<&[u8]>,String>{
        if matches!(resource,Resource::Image5Bss){return Ok(None);}
        let kind=match resource {Resource::LoaderA=>SourceKind::LoaderA,Resource::LoaderB=>SourceKind::LoaderB,Resource::Image5=>SourceKind::Image5,_=>SourceKind::Plain};
        let image=self.0.iter().find(|image|image.binding.kind==kind).ok_or("snapshot source absent")?;
        let (start,end)=resource.extent();let low=start.checked_sub(image.binding.start.0).ok_or("snapshot starts before source")? as usize;
        Ok(Some(image.bytes.get(low..low+(end-start) as usize).ok_or("snapshot exceeds source")?))
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(transparent)]
struct Count(u64);
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(transparent)]
struct Tick(u64);
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(try_from="usize",into="usize")]
enum Core { Primary, Secondary }
impl TryFrom<usize> for Core {type Error=String;fn try_from(value:usize)->Result<Self,String>{match value{0=>Ok(Self::Primary),1=>Ok(Self::Secondary),_=>Err("core is outside the two-core model".into())}}}
impl From<Core> for usize {fn from(value:Core)->Self{match value{Core::Primary=>0,Core::Secondary=>1}}}
#[derive(Clone, Debug, Deserialize, Serialize)]
struct Registers { r:[u32;13], sp:Guest, lr:Guest, cpsr:u32 }
#[derive(Clone, Debug, Deserialize, Serialize)]
struct Event { id:EventId, instructions:Count, core:Core, pc:Guest, source:SourceKind, code:Words, registers:Registers, frame:Result<Words,String>, state:Vec<Words>, descriptor:Option<Words>, payload:Option<Words> }
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all="kebab-case")]
enum CallKind { Loader, Image5Wait, Image5Entry, Image5Service, Image5Constructor, PlainProducer, PlainService, PlainHook, Deferred, Task }
#[derive(Clone, Debug, Deserialize, Serialize)]
struct Call { kind:CallKind, before:Option<EventId>, entry:Option<EventId>, returned:Option<EventId>, core:Core, target:Guest, continuation:Guest, sp:Guest, descriptor:Option<Guest> }
#[derive(Clone, Debug, Deserialize, Serialize)]
struct StaticCall { site:Guest, target:CallTarget, continuation:Guest }
#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(try_from="u8",into="u8")]
struct Gpr(u8);
impl TryFrom<u8> for Gpr {type Error=String;fn try_from(value:u8)->Result<Self,String>{if value<13{Ok(Self(value))}else{Err("call operand is outside the captured general registers".into())}}}
impl From<Gpr> for u8 {fn from(value:Gpr)->Self{value.0}}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all="kebab-case")]
enum CallTarget { Direct(Guest), Register(Gpr) }
#[derive(Clone, Debug, Deserialize, Serialize)]
struct InputIdentity { run:Digest, update:Digest, update_file:FileBinding, rom:Digest, executable:Digest, sources:Vec<SourceBinding> }
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all="kebab-case")]
enum Access { CpuFetch, CpuRead, CpuWrite, HostRead, HostWrite, ModelBusRead, ModelBusWrite }
#[derive(Clone, Copy, Deserialize, Serialize)]
struct Row {source:Access,address:Guest,physical:Guest,size:u64,core:Option<Core>,pc:Option<Guest>,instructions:Option<Count>,value:Option<u64>,tick:Option<Tick>}
#[derive(Deserialize, Serialize)]
struct Class {range:[u64;2],kinds:Vec<Access>}
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all="kebab-case")]
enum WatchViews { Observe, Fixed }
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all="kebab-case")]
enum WatchStage { Bootstrap, Startup, Tail }
impl WatchStage {
    fn early(self)->bool {self!=Self::Tail}
    fn views(self)->WatchViews {if self==Self::Bootstrap {WatchViews::Observe} else {WatchViews::Fixed}}
    fn file(self)->&'static str {match self {Self::Bootstrap=>"watch-bootstrap.json",Self::Startup=>"watch-prefix.json",Self::Tail=>"watch-tail.json"}}
}
#[derive(Deserialize)]
struct RawWatch {enabled:bool,violation:Option<serde::de::IgnoredAny>,views:WatchViews,classes:Vec<Class>,records:Vec<Row>}
#[derive(Deserialize, Serialize)]
struct Epoch {stage:WatchStage,start:Count,end:Count,enabled:bool,refused:bool,views:WatchViews,teardown_error:Option<String>,classes:Vec<Class>,record_count:usize,file:FileBinding}
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all="kebab-case")]
enum Key { Up, Ok, Menu }
impl Key {fn guest(self)->RouteKey{match self{Self::Up=>RouteKey::Up,Self::Ok|Self::Menu=>RouteKey::Ok}}}
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all="kebab-case")]
enum ReleaseOutcome { Debounced, Deadline }
#[derive(Deserialize, Serialize)]
struct Pulse {key:Key,pressed:Count,released:Option<Count>,finished:Option<Count>,outcome:Option<ReleaseOutcome>}
#[derive(Deserialize, Serialize)]
struct Report {schema:u32,program:ProgramBinding,#[serde(default,skip_serializing_if="Option::is_none")] plain:Option<plain_wire::Evidence>,origin:Option<EventId>,identity:InputIdentity,checkpoint:Digest,instructions:Count,menu:Result<Option<[u32;3]>,String>,error:Option<String>,machine_failure:Option<String>,events:Vec<Event>,calls:Vec<Call>,snapshots:Vec<Snapshot>,pulses:Vec<Pulse>,epochs:Vec<Epoch>,trace:FileBinding,timer_transactions:FileBinding,static_calls:Vec<StaticCall>}
const TIMER_RANGE:[Guest;2]=[Guest(0xfff91000),Guest(0xfff91008)];
const TIMER_LIMIT:usize=3_000_000;
#[derive(Serialize)]
struct TimerWatch {start:Count,end:Count,range:[Guest;2],limit:usize,batch:Option<TransactionBatch>,error:Option<String>}

#[derive(Default)]
struct Observation {events:Vec<Event>,calls:Vec<Call>,snapshots:Vec<Snapshot>,pulses:Vec<Pulse>,cursor:usize,plain_views:BTreeSet<u32>,image5_seen:bool,loader_sp:[Option<Guest>;2],first_loader:Option<EventId>,plain:Option<plain_runtime::Runtime>}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase { Setup, Root, DiagnosticReturn, Idle0, Idle1AndStandby }
impl Phase {
    fn parse(value:u32)->Result<Self,String>{match value{0=>Ok(Self::Setup),1=>Ok(Self::Root),2=>Ok(Self::DiagnosticReturn),3=>Ok(Self::Idle0),4=>Ok(Self::Idle1AndStandby),_=>Err("phase is outside the loader producer set".into())}}
}
fn dispatch_phase(pc:Guest)->Option<Phase>{
    carrier(pc)?;
    match pc.0&0xffff{0x09fc=>Some(Phase::Setup),0x0a34=>Some(Phase::Root),0x0a3c=>Some(Phase::Idle0),0x0a68=>Some(Phase::DiagnosticReturn),0x0aec=>Some(Phase::Idle1AndStandby),_=>None}
}
fn carrier(pc:Guest)->Option<Side>{[Side::LoaderA,Side::LoaderB].into_iter().find(|side|(side.base().0+0x9ac..side.base().0+0xb00).contains(&(pc.0&!0x40000000)))}
fn decode_call(site:Guest,word:u32)->Result<Option<StaticCall>,String>{
    if word&0xff000000==0xeb000000 {let displacement=(((word&0xffffff)<<8) as i32>>6) as u32;Ok(Some(StaticCall {site,target:CallTarget::Direct(Guest(site.0.wrapping_add(8).wrapping_add(displacement))),continuation:Guest(site.0+4)}))}
    else if word&0xfffffff0==0xe12fff30 {Ok(Some(StaticCall {site,target:CallTarget::Register(Gpr::try_from((word&15) as u8)?),continuation:Guest(site.0+4)}))}else{Ok(None)}
}
fn startup_calls(catalog:&Catalog)->Result<Vec<StaticCall>,String>{
    let mut calls=Vec::new();for (start,end) in [(0x409ac,0x40af4),(0xd09ac,0xd0af4),(0xd985a4,0xd986ac),(0x6a9844,0x6a9934),(0x73b554,0x73b558)]{for pc in (start..end).step_by(4){if let Some(call)=decode_call(Guest(pc),catalog.word(Guest(pc))?.1)?{calls.push(call);}}}Ok(calls)
}
fn diagnostics(pc:Guest)->bool{let physical=pc.0&!0x40000000;DIAGNOSTICS.iter().any(|&(start,end)|(start..end).contains(&physical))}
fn base_sites(calls:&[StaticCall])->BTreeMap<u32,String>{
    let mut points=BTreeMap::new();for call in calls{points.insert(call.site.0,"source-call".into());points.insert(call.continuation.0,"source-return".into());}
    for base in [0x40000,0xd0000]{for offset in [0x9ac,0x9cc,0x9d0,0xa08,0xa0c,0xa8c,0xa90,0xaa8,0xaac,0xab0]{points.insert(base+offset,"loader-boundary".into());}}
    for &(start,end) in &DIAGNOSTICS{for pc in (start..end).step_by(4){points.insert(pc,"diagnostic-refusal".into());}}
    for pc in [0x43688,0xd3688,0x436e4,0xd36e4,0x436e8,0xd36e8,0x43728,0xd3728,0x43758,0xd3758,0x69cbf0,0x69cd58,0x69d160,0x69d2b4,0x69d2e0,0x69d2e4,0x69cf84,0x69cf94,0x69cf98,0xd985a4,0xd985c0,0xd985c4,0xd985e0,0xd985e4,0x6a96c0,0x6a9734,0x6a9840,0x6a9844,0x6a9934,0x6ae360,0x6ae3f4,0x6ae500,0x73b3a0,0x73b4ac,0x73b554,0x73b924,0x73b950,0x73b9c4,0x73bc54,0x73bd24,0x73be5c,0x73be60,0x73be70,0x73be74,0x73beac,0x73beb0,0x73cdc0,0x73e048,0x725dd8,0x1109130,0x11e6e14,0x125cdfc,0x1273f60,0x1279828,0x127aabc,0x127bdf4,0x127cbb0,0x127da94,0x127f658,0x12812c4,0x128becc,0x13e36fc,0x13e9c0c,0x13eb548]{points.insert(pc,"continuation".into());}
    for pc in [0xd9858c,0xd98590,0xd986ac,0xd986c8,0xd986cc,0xd986dc,0xd986e0,0x107277c,0x1066304,0x1062114,0x1071ca4,0x106ceec,0x1064224,0x6a97bc]{points.insert(pc,"image5-continuation".into());}
    for pc in [0x6a8bec,0x6a8c00,0x6a8c04]{points.insert(pc,"bss-completion".into());}
    points.insert(0x69d2d8,"source-spin-refusal".into());
    points
}
fn sites(calls:&[StaticCall],program:&Program)->BTreeMap<u32,String>{
    let mut points=base_sites(calls);
    for pc in [0x41ad4,0x41ad8,0x41adc,0x41ae0,0x41ae4,0x41ae8,0x41b78,0x41b7c,0x41b80]{points.insert(pc,"elapsed-source".into());}
    for pc in program.refusal_sites(){points.insert(pc.0,"unsupported-carrier-fetch".into());}
    points
}
const LOADER_SPANS:[(u64,u64);20]=[(0x61c000,0x61c220),(0x40000,0x40020),(0xd0000,0xd0020),(0xa00c4,0xa00c8),(0x5927c,0x59280),(0xe923c,0xe9240),(0x4f10c,0x4f118),(0xdf0cc,0xdf0d8),(0x89f794,0x89f798),(0x96bd20,0x96bd28),(0x89f8ac,0x89f8b0),(0x944440,0x944464),(0x914834,0x914838),(0x972e88,0x972e94),(0x14c100c,0x14c1010),(0x14c1018,0x14c101c),(0x14c113c,0x14c1140),(0x9662d4,0x9662d8),(0x89f784,0x89f788),(0x89f78c,0x89f790)];
const EARLY_SPANS:[(u64,u64);8]=[(0x545a4,0x545a8),(0x54760,0x5476c),(0xe4564,0xe4568),(0xe4720,0xe472c),(0x63450,0x63460),(0xf3410,0xf3420),(0x63460,0x63498),(0xf3420,0xf3458)];
const RING_HEADER:(u64,u64)=(0xa0800,0xa0838);
fn loader_classes(spans:Vec<(u64,u64)>,elapsed:&[(u64,u64)],carrier:WatchKinds)->Vec<WatchRange>{
    let fetched=WatchKinds::CPU_FETCH.union(WatchKinds::WRITES);
    let mut result:Vec<_>=spans.into_iter().map(|(start,end)|WatchRange {start,end,kinds:WatchKinds::WRITES}).collect();
    result.extend(DIAGNOSTICS.map(|(start,end)|WatchRange {start:u64::from(start),end:u64::from(end),kinds:fetched}));
    result.extend(elapsed.iter().map(|&(start,end)|WatchRange {start,end,kinds:fetched}));
    result.extend([(0x409ac,0x40b00),(0xd09ac,0xd0b00)].map(|(start,end)|WatchRange {start,end,kinds:carrier}));result
}
fn classes(stage:WatchStage)->Vec<WatchRange>{
    let mut spans=LOADER_SPANS.to_vec();spans.push(RING_HEADER);
    if stage.early(){spans.extend(EARLY_SPANS);spans.push((0xa0800,0xc0900));}
    let elapsed:&[(u64,u64)]=if stage==WatchStage::Startup{&[(0x41ad4,0x41aec),(0x41b78,0x41b84)]}else{&[]};
    loader_classes(spans,elapsed,if stage==WatchStage::Bootstrap{WatchKinds::WRITES}else{WatchKinds::CPU_FETCH.union(WatchKinds::WRITES)})
}
fn registers(machine:&Machine,core:Core)->Result<Registers,String>{
    let index=usize::from(core);let mut r=[0;13];for (number,value) in r.iter_mut().enumerate(){*value=u32::try_from(machine.session.board.read_register(index,Register::Arm32(arm::Register::R(number as u8)))?).map_err(|_|"register exceeds word")?;}
    let get=|register|u32::try_from(machine.session.board.read_register(index,Register::Arm32(register))?).map_err(|_|"register exceeds word".to_owned());
    Ok(Registers {r,sp:Guest(get(arm::Register::R(13))?),lr:Guest(get(arm::Register::R(14))?),cpsr:get(arm::Register::Cpsr)?})
}
fn persist<T:Serialize>(path:&Path,value:&T)->Result<(),String>{write(path,&serde_json::to_vec_pretty(value).map_err(|error|error.to_string())?)}
fn binding(output:&Path,name:&str)->Result<FileBinding,String>{Ok(FileBinding {path:PathBuf::from(name),sha256:Digest::of(&read(&output.join(name))?)})}

impl Observation {
    fn capture(&mut self,machine:&Machine,catalog:&Catalog,maps:&[MemoryMap],core:Core,pc:Guest)->Result<EventId,String>{
        if machine.session.board.read_register(usize::from(core),Register::Pc)?!=u64::from(pc.0){return Err("event is not at the live stopped PC".into());}
        let reader=continuation_view::Reader::new(&machine.session,maps)?;let registers=registers(machine,core)?;let (source,expected)=catalog.word(pc)?;let code=reader.words(pc,1)?;
        if matches!(pc.0,0x409ac|0xd09ac){self.loader_sp[usize::from(core)]=Some(Guest(registers.sp.0.checked_sub(16).ok_or("startup frame underflow")?));}
        if code.values!=[expected]{return Err("entered instruction differs from canonical source".into());}
        let frame=registers.sp.0.checked_sub(64).ok_or_else(||"stack projection underflows".to_owned()).and_then(|low|reader.words(Guest(low),20));
        let id=EventId(self.events.len());let mut state=Vec::new();
        let ring=match pc.0{0x41ad4|0x41ad8|0x41adc|0x41ae0|0x41ae4|0x41ae8=>Some(registers.r[3]),0x41b78|0x41b7c=>Some(registers.r[8]),0x41b80 if self.events.iter().rev().find(|event|event.core==core).is_some_and(|event|event.pc.0==0x41b7c)=>Some(registers.r[8]),_=>None};
        if let Some(base)=ring{let address=base.checked_add(0x40).ok_or("elapsed ring word address overflows")?;let physical=address&!0x40000000;if address%4!=0||!(0xa0840..0xc0900).contains(&physical){return Err("elapsed ring word is outside the shared ring".into());}state.push(reader.words(Guest(address),1)?);}
        for (address,words) in [(0xa00c4,1),(0x61c000,8),(0x61c1a4,24),(0x61c400,4),(0x40000,8),(0xd0000,8),(0x5927c,1),(0xe923c,1),(0x4f10c,3),(0xdf0cc,3),(0x63460,14),(0xf3420,14),(0xa0800,14),(0x96bd20,2),(0x89f784,6),(0x89f8ac,1),(0x944440,9),(0x914834,1),(0x972e88,3),(0x8e1900,8),(0x9c2f54,2)]{state.push(reader.words(Guest(address),words)?);}
        if machine.instructions()<=8_000_000{if let Some(sp)=self.loader_sp[usize::from(core)]{state.push(reader.words(sp,4)?);}}
        if pc.0==0x69d2e0{let descriptor=reader.words(Guest(registers.r[0]),2)?;let header=Guest(descriptor.values[1]);state.push(descriptor);state.push(reader.words(header,4)?);}
        let descriptor=if pc.0==0x725dd8{Some(reader.words(Guest(registers.r[0]),7)?)}else if matches!(pc.0,0x73be5c|0x73be60|0x73be70|0x73be74){Some(reader.words(Guest(registers.r[4]),6)?)}else{None};
        let payload=if pc.0==0x73b9c4{Some(reader.words(Guest(registers.r[2]),1)?)}else{None};
        self.events.push(Event {id,instructions:Count(machine.instructions()),core,pc,source,code,registers,frame,state,descriptor,payload});Ok(id)
    }
    fn target(&mut self,machine:&mut Machine,catalog:&Catalog,target:Guest)->Result<(),String>{
        if target.0%4!=0{return Err("computed target is outside the admitted A32 entry set".into());}catalog.word(target)?;
        if diagnostics(target){return Err("computed transfer would enter a diagnostic body".into());}
        if !machine.traces.sites.contains_key(&target.0){machine.traces.add_sites(&machine.session,BTreeMap::from([(target.0,"temporary-boundary".into())]))?;}
        Ok(())
    }
    fn collect(&mut self,machine:&mut Machine,catalog:&Catalog,maps:&[MemoryMap],output:&Path,permanent:&BTreeMap<u32,String>,static_calls:&[StaticCall],program:&Program)->Result<(),String>{
        if machine.traces.hits.len()>=TRACE_LIMIT{return Err("continuation trace bound exhausted".into());}
        while self.cursor<machine.traces.hits.len(){
            let hit=&machine.traces.hits[self.cursor];let core=Core::try_from(hit.core)?;let pc=Guest(u32::from_str_radix(hit.address.strip_prefix("0x").ok_or("trace PC prefix absent")?,16).map_err(|error|error.to_string())?);
            if hit.instructions!=machine.instructions(){return Err("stopped trace event is no longer live".into());}
            if !program.execution_allowed(pc){return Err("carrier fetch lies outside the admitted executable code".into());}
            let first_carrier=self.first_loader.is_none()&&carrier(pc).is_some();
            if first_carrier&&(pc!=Guest(0xd09ac)||core!=Core::Secondary||machine.instructions()!=5_861_929){return Err("first carrier fetch differs from the canonical reset caller".into());}
            let id=self.capture(machine,catalog,maps,core,pc)?;self.cursor+=1;
            if let Some(plain)=&mut self.plain {plain.capture(machine,maps,output,&mut self.events[id.0],catalog)?;}
            if first_carrier{self.first_loader=Some(id);}
            if diagnostics(pc){return Err("natural route reached a diagnostic body before execution".into());}
            if pc.0==0x69d2d8||(!program.owned()&&matches!(pc.0,0x40aa8|0xd0aa8)){return Err("selected route reached an unexpected returned-context or header-lookup spin".into());}
            let event=&self.events[id.0];let regs=event.registers.clone();
            for call in &mut self.calls {
                if call.core!=core{continue;}
                if call.entry.is_none() && call.target==pc && call.sp==regs.sp && call.continuation==regs.lr{call.entry=Some(id);}
                if call.entry.is_some() && call.returned.is_none() && call.continuation==pc && call.sp==regs.sp{call.returned=Some(id);}
            }
            let boundary_consumed=self.calls.iter().any(|call|call.returned.is_some_and(|returned|returned.0==id.0)||call.entry.is_some_and(|entry|entry.0==id.0));
            let boundary_needed=self.calls.iter().any(|call|(call.entry.is_none()&&call.target==pc)||(call.entry.is_some()&&call.returned.is_none()&&call.continuation==pc));
            if boundary_consumed && !boundary_needed && !permanent.contains_key(&pc.0){machine.traces.remove_sites(&machine.session,&[pc])?;}
            if let Some(address)=self.calls.iter().find(|call|call.kind==CallKind::Task&&call.returned.is_some_and(|returned|returned.0==id.0)).and_then(|call|call.descriptor){self.events[id.0].descriptor=Some(continuation_view::words(&machine.session,maps,address,7)?);}
            let spec=if let Some(spec)=static_calls.iter().find(|call|call.site==pc){Some(spec.clone())}else if matches!(pc.0,0xd9858c|0x69d2e0|0x73be5c|0x73beac){decode_call(pc,catalog.word(pc)?.1)?}else{None};
            if let Some(spec)=spec {
                let target=match spec.target{CallTarget::Direct(target)=>target,CallTarget::Register(register)=>Guest(regs.r[usize::from(register.0)])};
                if carrier(pc).is_some()&&matches!(spec.target,CallTarget::Register(_)){Phase::parse(regs.r[0])?;if target!=Guest(0x69cdf8)||regs.r[4]!=0x69cdf8{return Err("loader phase dispatcher capability differs".into());}}
                let kind=match pc.0{0xd985c0=>CallKind::Image5Wait,0x69d2e0=>CallKind::Image5Entry,0xd9858c=>CallKind::Image5Constructor,0x73b554=>CallKind::PlainProducer,0x73be5c=>CallKind::PlainHook,0x73beac=>CallKind::Deferred,pc if (0xd985a4..0xd986ac).contains(&pc)=>CallKind::Image5Service,pc if (0x6a9844..0x6a9934).contains(&pc)=>CallKind::PlainService,_=>CallKind::Loader};
                self.target(machine,catalog,target)?;self.target(machine,catalog,spec.continuation)?;
                self.calls.push(Call {kind,before:Some(id),entry:None,returned:None,core,target,continuation:spec.continuation,sp:regs.sp,descriptor:None});
            }
            if pc.0==0x725dd8 {
                if self.calls.iter().any(|call|call.kind==CallKind::Task&&call.core==core&&call.sp==regs.sp&&call.returned.is_none()){return Err("task-call return identity overlaps an unfinished call".into());}
                self.target(machine,catalog,regs.lr)?;
                self.calls.push(Call {kind:CallKind::Task,before:None,entry:Some(id),returned:None,core,target:pc,continuation:regs.lr,sp:regs.sp,descriptor:Some(Guest(regs.r[0]))});
            }
            if matches!(pc.0,0x436e8|0xd36e8){
                let context=continuation_view::words(&machine.session,maps,Guest(regs.r[1].checked_add(0x18).ok_or("saved context address overflow")?),2)?;
                if regs.sp.0!=context.values[0]{return Err("selected stack differs from the loaded saved context".into());}
                let target=Guest(context.values[1]);self.events[id.0].state.push(context);self.target(machine,catalog,target)?;
                let addresses=if pc.0==0x436e8{[Guest(0x43688),Guest(0x436e4),Guest(0x436e8)]}else{[Guest(0xd3688),Guest(0xd36e4),Guest(0xd36e8)]};machine.traces.remove_sites(&machine.session,&addresses)?;
            }
            if matches!(pc.0,0x43758|0xd3758){let task=continuation_view::words(&machine.session,maps,Guest(regs.r[2]),3)?;let link=continuation_view::words(&machine.session,maps,Guest(regs.r[1].checked_add(8).ok_or("task creation-record link overflows")?),1)?;if link.values[0]!=regs.r[2]{return Err("current task creation-record link differs".into());}self.target(machine,catalog,Guest(task.values[2]))?;self.events[id.0].state.extend([task,link]);machine.traces.remove_sites(&machine.session,&[pc])?;}
            let resources:Vec<Resource>=match pc.0 {
                0x409ac=>vec![Resource::LoaderA],0xd09ac if first_carrier=>vec![Resource::LoaderB,Resource::LoaderA],0xd09ac=>vec![Resource::LoaderB],
                0x6a8c04=>vec![Resource::Image5Bss],
                0x6a9734=>vec![Resource::Image5Bss],
                0x6a9840=>vec![Resource::Image5,Resource::Image5Bss],0x6a9934=>vec![Resource::Plain],
                0xd985a4 if !self.image5_seen=>{self.image5_seen=true;vec![Resource::Image5,Resource::Image5Bss,Resource::Plain]},
                0x73be5c|0x73be70 if self.plain_views.insert(regs.r[7])=>vec![match regs.r[7]{0=>Resource::Plain0,2=>Resource::Plain2,3=>Resource::Plain3,_=>return Err("plain extent ID differs from the canonical three-entry table".into())}],_=>Vec::new(),
            };
            for resource in resources{self.snapshots.push(continuation_view::snapshot(&machine.session,maps,output,id,resource,catalog.expected(resource)?)?);}
            if self.plain.as_ref().is_some_and(|plain|plain.program.owned()&&pc==plain.program.return_pc()){self.snapshots.push(continuation_view::snapshot(&machine.session,maps,output,id,Resource::Plain,catalog.expected(Resource::Plain)?)?);}
        }
        Ok(())
    }
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Until { Budget, FirstCarrier }
fn advance(machine:&mut Machine,observation:&mut Observation,catalog:&Catalog,maps:&[MemoryMap],output:&Path,permanent:&BTreeMap<u32,String>,calls:&[StaticCall],count:u64,program:&Program,until:Until)->Result<(),String>{
    let end=machine.instructions().checked_add(count).ok_or("instruction endpoint overflow")?;
    while machine.instructions()<end {
        let before=machine.instructions();machine.run(end-before);observation.collect(machine,catalog,maps,output,permanent,calls,program)?;machine.require_running()?;
        if until==Until::FirstCarrier&&observation.first_loader.is_some(){return Ok(());}
        if machine.instructions()==before{return Err("observer made no entered-instruction progress".into());}
    }
    Ok(())
}
fn pulse(machine:&mut Machine,observation:&mut Observation,catalog:&Catalog,maps:&[MemoryMap],output:&Path,permanent:&BTreeMap<u32,String>,calls:&[StaticCall],key:Key,program:&Program)->Result<(),String>{
    machine.press(key.guest())?;let index=observation.pulses.len();observation.pulses.push(Pulse {key,pressed:Count(machine.instructions()),released:None,finished:None,outcome:None});
    advance(machine,observation,catalog,maps,output,permanent,calls,8_000_000,program,Until::Budget)?;machine.release_key(key.guest())?;let start=machine.instructions();observation.pulses[index].released=Some(Count(start));
    let outcome=loop{if machine.document.firmware.released(&machine.session,key.guest())?{break ReleaseOutcome::Debounced;}let spent=machine.instructions()-start;if spent>=8_000_000{break ReleaseOutcome::Deadline;}advance(machine,observation,catalog,maps,output,permanent,calls,(8_000_000-spent).min(250_000),program,Until::Budget)?;};
    observation.pulses[index].finished=Some(Count(machine.instructions()));observation.pulses[index].outcome=Some(outcome);Ok(())
}
fn epoch(machine:&mut Machine,observation:&mut Observation,catalog:&Catalog,maps:&[MemoryMap],output:&Path,permanent:&BTreeMap<u32,String>,calls:&[StaticCall],stage:WatchStage,program:&Program)->Result<(Epoch,Result<(),String>),String>{
    let start=Count(machine.instructions());let views=match stage.views(){WatchViews::Observe=>ViewPolicy::Observe,WatchViews::Fixed=>ViewPolicy::Fixed};
    let mut ranges=classes(stage);
    let mut watch=if let Some(plain)=&observation.plain {ranges.extend(plain.classes());machine.session.board.memory_watch_begin_with_fetch_attribution(ranges,plain.attribution(catalog,machine)?,3_000_000,maps.to_vec(),views)?} else {machine.session.board.memory_watch_begin_classified(ranges,3_000_000,maps.to_vec(),views)?};
    let result=(||->Result<(),String>{
        if stage==WatchStage::Bootstrap {
            advance(machine,observation,catalog,maps,output,permanent,calls,8_000_000,program,Until::FirstCarrier)?;
            return if observation.first_loader.is_some(){Ok(())}else{Err("first carrier entry deadline expired".into())};
        }
        if stage==WatchStage::Startup {
            let addresses:Vec<_>=machine.traces.sites.iter().filter(|(_,label)|label.as_str()=="first-carrier-fetch").map(|(&address,_)|Guest(address)).collect();
            if !addresses.is_empty(){machine.traces.remove_sites(&machine.session,&addresses)?;}
            return advance(machine,observation,catalog,maps,output,permanent,calls,8_000_000u64.checked_sub(machine.instructions()).ok_or("first entry exceeds startup deadline")?,program,Until::Budget);
        }
        advance(machine,observation,catalog,maps,output,permanent,calls,80_000_000u64.checked_sub(machine.instructions()).ok_or("prefix exceeds initial menu deadline")?,program,Until::Budget)?;
        if !machine.menu()?.is_some_and(|menu|(menu.menu_id,menu.setting_id,menu.cursor)==(69,52,12)){return Err("natural initial cursor differs".into());}
        for key in std::iter::repeat_n(Key::Up,12).chain([Key::Ok,Key::Ok]){pulse(machine,observation,catalog,maps,output,permanent,calls,key,program)?;}
        advance(machine,observation,catalog,maps,output,permanent,calls,40_000_000,program,Until::Budget)?;
        pulse(machine,observation,catalog,maps,output,permanent,calls,Key::Menu,program)?;
        advance(machine,observation,catalog,maps,output,permanent,calls,90_000_000,program,Until::Budget)
    })();
    let raw=watch.records.borrow().state();let name=stage.file();persist(&output.join(name),&raw)?;
    let teardown_error=machine.session.board.memory_watch_end(&mut watch).err();let state:RawWatch=serde_json::from_value(raw).map_err(|error|error.to_string())?;
    Ok((Epoch {stage,start,end:Count(machine.instructions()),enabled:state.enabled,refused:state.violation.is_some(),views:state.views,teardown_error,classes:state.classes,record_count:state.records.len(),file:binding(output,name)?},result))
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum OptionKey { Run,Update,Rom,Analysis,Output,Verify,SourceCalls,OwnedArtifact,CompareRetained,CompareOwned,PlainWitness,PlainArtifact,VerifyPlain,ComparePlainRetained,ComparePlainOwned,PlainControls,ApplicationPump,VerifyApplicationPump,CompareApplicationPump,OwnedPump,ControlsApplicationPump,OriginalUpdate,PumpCode,PumpSymbols,PumpAdmission }
impl OptionKey {fn parse(value:&std::ffi::OsStr)->Result<Self,String>{match value.to_str(){Some("--run")=>Ok(Self::Run),Some("--update")=>Ok(Self::Update),Some("--boot-rom")=>Ok(Self::Rom),Some("--analysis")=>Ok(Self::Analysis),Some("--output")=>Ok(Self::Output),Some("--verify")=>Ok(Self::Verify),Some("--source-calls")=>Ok(Self::SourceCalls),Some("--owned-artifact")=>Ok(Self::OwnedArtifact),Some("--compare-retained")=>Ok(Self::CompareRetained),Some("--compare-owned")=>Ok(Self::CompareOwned),Some("--plain-witness")=>Ok(Self::PlainWitness),Some("--plain-artifact")=>Ok(Self::PlainArtifact),Some("--verify-plain")=>Ok(Self::VerifyPlain),Some("--compare-plain-retained")=>Ok(Self::ComparePlainRetained),Some("--compare-plain-owned")=>Ok(Self::ComparePlainOwned),Some("--plain-controls")=>Ok(Self::PlainControls),Some("--application-pump")=>Ok(Self::ApplicationPump),Some("--verify-application-pump")=>Ok(Self::VerifyApplicationPump),Some("--compare-application-pump")=>Ok(Self::CompareApplicationPump),Some("--owned-pump")=>Ok(Self::OwnedPump),Some("--controls-application-pump")=>Ok(Self::ControlsApplicationPump),Some("--original-update")=>Ok(Self::OriginalUpdate),Some("--pump-code")=>Ok(Self::PumpCode),Some("--pump-symbols")=>Ok(Self::PumpSymbols),Some("--pump-admission")=>Ok(Self::PumpAdmission),_=>Err(USAGE.into())}}}
fn drive()->Result<(),String>{
    let mut arguments=std::env::args_os().skip(1);let mut options=BTreeMap::new();
    while let Some(flag)=arguments.next(){if options.insert(OptionKey::parse(&flag)?,PathBuf::from(arguments.next().ok_or(USAGE)?)).is_some(){return Err(USAGE.into());}}
    let analysis=options.remove(&OptionKey::Analysis).ok_or(USAGE)?;
    if let Some(output)=options.remove(&OptionKey::VerifyApplicationPump){if !options.is_empty(){return Err(USAGE.into());}return application_pump::verify(&output,&analysis);}
    if let Some(retained)=options.remove(&OptionKey::CompareApplicationPump){
        let owned=options.remove(&OptionKey::OwnedPump).ok_or(USAGE)?;
        let output=options.remove(&OptionKey::Output).ok_or(USAGE)?;
        if !options.is_empty(){return Err(USAGE.into());}
        return application_pump::compare(&retained,&owned,&analysis,&output);
    }
    if let Some(source)=options.remove(&OptionKey::ControlsApplicationPump){
        let output=options.remove(&OptionKey::Output).ok_or(USAGE)?;
        if !options.is_empty(){return Err(USAGE.into());}
        return application_pump::controls(&source,&analysis,&output);
    }
    if let Some(mode)=options.remove(&OptionKey::ApplicationPump){
        let mode=application_pump::Mode::parse(&mode)?;
        let mut take=|key|options.remove(&key).ok_or_else(||USAGE.to_owned());
        let request=application_pump::Request {mode,analysis,run:take(OptionKey::Run)?,update:take(OptionKey::Update)?,original_update:take(OptionKey::OriginalUpdate)?,boot_rom:take(OptionKey::Rom)?,output:take(OptionKey::Output)?,code:take(OptionKey::PumpCode)?,symbols:take(OptionKey::PumpSymbols)?,admission:take(OptionKey::PumpAdmission)?};
        if !options.is_empty(){return Err(USAGE.into());}
        return application_pump::run(request);
    }
    if let Some(output)=options.remove(&OptionKey::SourceCalls){
        if !options.is_empty(){return Err(USAGE.into());}
        let catalog=Catalog::open(&analysis)?;std::fs::create_dir(&output).map_err(|error|error.to_string())?;
        #[derive(Serialize)]struct Model {schema:u32,sources:Vec<SourceBinding>,calls:Vec<StaticCall>}
        return persist(&output.join("source-calls.json"),&Model {schema:1,sources:catalog.0.iter().map(|image|image.binding.clone()).collect(),calls:startup_calls(&catalog)?});
    }
    if let Some(output)=options.remove(&OptionKey::Verify){if !options.is_empty(){return Err(USAGE.into());}let verified=loader_continuations_check::check(&output,&analysis)?;println!("{}",serde_json::to_string(&verified).map_err(|error|error.to_string())?);return Ok(());}
    if let Some(retained)=options.remove(&OptionKey::CompareRetained){let owned=options.remove(&OptionKey::CompareOwned).ok_or(USAGE)?;let output=options.remove(&OptionKey::Output).ok_or(USAGE)?;if !options.is_empty(){return Err(USAGE.into());}return loader_continuations_check::compare(&retained,&owned,&analysis,&output);}
    if let Some(root)=options.remove(&OptionKey::VerifyPlain){if !options.is_empty(){return Err(USAGE.into());}let receipt=loader_continuations_check::check_plain(&root,&analysis)?;println!("{}",serde_json::to_string(&receipt).map_err(|error|error.to_string())?);return Ok(());}
    if let Some(retained)=options.remove(&OptionKey::ComparePlainRetained){let owned=options.remove(&OptionKey::ComparePlainOwned).ok_or(USAGE)?;let output=options.remove(&OptionKey::Output).ok_or(USAGE)?;if !options.is_empty(){return Err(USAGE.into());}return loader_continuations_check::compare_plain(&retained,&owned,&analysis,&output);}
    if let Some(root)=options.remove(&OptionKey::PlainControls){let output=options.remove(&OptionKey::Output).ok_or(USAGE)?;if !options.is_empty(){return Err(USAGE.into());}return loader_continuations_check::controls_plain(&root,&analysis,&output);}
    let owned_artifact=options.remove(&OptionKey::OwnedArtifact);
    let plain_role=options.remove(&OptionKey::PlainWitness).map(|value|match value.to_str(){Some("retained")=>Ok(plain_wire::Role::Retained),Some("owned")=>Ok(plain_wire::Role::Owned),_=>Err("plain witness role must be retained or owned".to_owned())}).transpose()?;
    let plain_artifact=options.remove(&OptionKey::PlainArtifact);
    if (plain_role.is_some()&&owned_artifact.is_some())||(plain_role.is_none()&&plain_artifact.is_some()){return Err("plain witness is separate from owned loader composition".into());}
    let mut take=|key|options.remove(&key).ok_or_else(||USAGE.to_owned());let run_path=take(OptionKey::Run)?;let update_path=take(OptionKey::Update)?;let rom_path=take(OptionKey::Rom)?;let output=take(OptionKey::Output)?;
    if !options.is_empty(){return Err(USAGE.into());}std::fs::create_dir(&output).map_err(|error|error.to_string())?;
    let result=(||->Result<(),String>{
        let run_bytes=read(&run_path)?;let update=read(&update_path)?;let rom=read(&rom_path)?;let mut catalog=Catalog::open(&analysis)?;
        let program=match &owned_artifact {Some(stage)=>Program::from_staged(stage,&output,&mut catalog)?,None=>Program::Retained};
        let mut calls=startup_calls(&catalog)?;
        let plain=plain_role.map(|role|{
            let payload=plain_runtime::bind_payload(&analysis,&output,&catalog)?;
            plain_program::Program::from_staged(role,plain_artifact.as_deref(),&output,&mut catalog).map(|program|plain_runtime::Runtime::new(program,payload))
        }).transpose()?;
        if let Some(plain)=&plain {calls.retain(|call|!(0x6a9844..0x6a9938).contains(&call.site.0));calls.extend(plain.program.call_sites());}
        let update_digest=Digest::of(&update);
        let identity=InputIdentity {run:Digest::of(&run_bytes),update:update_digest.clone(),update_file:FileBinding {path:"input-update.DAT".into(),sha256:update_digest},rom:Digest::of(&rom),executable:Digest::of(&read(&std::env::current_exe().map_err(|error|error.to_string())?)?),sources:catalog.0.iter().map(|image|image.binding.clone()).collect()};
        if identity.run.as_str()!="efa32ff8d2a378ba9e098ac28015faf82856765c866d5db5b97cb1c38342deef"||identity.update.as_str()!=plain.as_ref().map_or_else(||program.update(),|plain|plain.program.update())||identity.rom.as_str()!="4ecedfbfc1910bde488d36abe4380621f8845e6afcfc6cf54a49a33ec0abf9be"{return Err("observer inputs differ from the admitted reset route and program".into());}
        std::fs::hard_link(&update_path,output.join(&identity.update_file.path)).map_err(|error|format!("cannot retain the validated DAT input: {error}"))?;
        persist(&output.join("inputs.json"),&identity)?;persist(&output.join("source-graph.json"),&calls)?;
        fujimod_emulator_exr2_board::register_board_builder();fujimod_emulator_models::register();let document=RunDocument::parse(&run_bytes)?;let maps=run::memory_maps(&document)?;
        let mut session=run::open(&document,&ValidatedUpdate(update),&rom)?;session.board.set_execution_policy(document.execution_policy.into())?;let mut machine=Machine::new(document,session)?;
        machine.disarm()?;let checkpoint=machine.session.checkpoint(None)?.export()?;write(&output.join("reset.fujimod"),&checkpoint)?;let restored=machine.session.import_snapshot(&checkpoint)?;machine.session.restore(&restored)?;machine.rearm()?;
        if machine.instructions()!=0{return Err("saved reset checkpoint is not preexecution".into());}
        let mut permanent=sites(&calls,&program);
        if let Some(plain)=&plain {for pc in plain.sites(){permanent.insert(pc.0,"plain-witness".into());}}
        let mut initial=permanent.clone();
        for side in [Side::LoaderA,Side::LoaderB]{for pc in (side.base().0+0x9ac..side.base().0+0xb00).step_by(4){initial.entry(pc).or_insert_with(||"first-carrier-fetch".into());}}
        machine.traces.bound_full_registers(NonZeroUsize::new(TRACE_LIMIT).ok_or("zero trace bound")?);machine.traces.stop_labels=initial.values().cloned().chain(["temporary-boundary".into()]).collect();
        let mut observation=Observation {plain,..Observation::default()};let mut epochs=Vec::new();
        let mut timer=TimerWatch {start:Count(0),end:Count(0),range:TIMER_RANGE,limit:TIMER_LIMIT,batch:None,error:None};let mut timer_active=false;
        let outcome=(||->Result<(),String>{
            machine.traces.arm(&machine.session,initial)?;
            for stage in [WatchStage::Bootstrap,WatchStage::Startup,WatchStage::Tail]{
                if stage==WatchStage::Startup{
                    timer.start=Count(machine.instructions());
                    machine.session.board.transaction_watch_begin(vec![(u64::from(TIMER_RANGE[0].0),u64::from(TIMER_RANGE[1].0))],TIMER_LIMIT).map_err(|error|{timer.error=Some(error.clone());error})?;timer_active=true;
                }
                let (observed,result)=epoch(&mut machine,&mut observation,&catalog,&maps,&output,&permanent,&calls,stage,&program)?;epochs.push(observed);
                if stage==WatchStage::Startup{
                    timer.end=Count(machine.instructions());timer_active=false;
                    timer.batch=Some(machine.session.board.transaction_watch_end().map_err(|error|{timer.error=Some(error.clone());error})?);
                    if timer.batch.as_ref().is_some_and(|batch|batch.omitted!=0){let error="timer transaction bound exhausted".to_owned();timer.error=Some(error.clone());return Err(error);}
                }
                result?;
            }
            Ok(())
        })();
        if timer_active{timer.end=Count(machine.instructions());match machine.session.board.transaction_watch_end(){Ok(batch)=>timer.batch=Some(batch),Err(error)=>timer.error=Some(error)}}
        persist(&output.join("timer-transactions.json"),&timer)?;
        persist(&output.join("trace.json"),&machine.traces.hits)?;
        let report=Report {schema:3,program:program.binding(),plain:observation.plain.map(|plain|plain.evidence),origin:observation.first_loader,identity,checkpoint:Digest::of(&checkpoint),instructions:Count(machine.instructions()),menu:machine.menu().map(|menu|menu.map(|menu|[menu.menu_id,menu.setting_id,menu.cursor])),error:outcome.err(),machine_failure:machine.failure.clone(),events:observation.events,calls:observation.calls,snapshots:observation.snapshots,pulses:observation.pulses,epochs,trace:binding(&output,"trace.json")?,timer_transactions:binding(&output,"timer-transactions.json")?,static_calls:calls};
        persist(&output.join("continuations.json"),&report)?;
        if report.error.is_some()||report.machine_failure.is_some(){return Err("continuation observation did not complete".into());}
        drop(catalog);let verified=loader_continuations_check::check(&output,&analysis)?;persist(&output.join("verification.json"),&verified)
    })();
    if let Err(error)=&result{#[derive(Serialize)]struct Failure<'a>{error:&'a str,primary_receipt_present:bool}persist(&output.join("observer-error.json"),&Failure {error,primary_receipt_present:output.join("continuations.json").is_file()})?;}
    result
}
fn main()->ExitCode{match drive(){Ok(())=>ExitCode::SUCCESS,Err(error)=>{eprintln!("{error}");ExitCode::FAILURE}}}
