use super::{Access, CallKind, Catalog, Core, Event, Guest, Report, SourceKind};
use super::continuation_view::{Digest, EventId, RangeSnapshot, Words};
use super::loader_continuations_check::{artifact, at, event, guarded, in_class, mapping, words};
use super::plain_program::Program;
use super::plain_wire::{Evidence, Limit, Role, SampleKind};
use fujimod_xm1::bridge::read;
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use std::{collections::BTreeSet, fs::File, io::Read, path::Path};

const START:u32=0x1100000;
const END:u32=0x1460000;
const BLOCK:u32=0x20000;
const CALLBACK:u32=0x73b924;
const EXTENTS:[(u32,u32,u32,u32,u32);3]=[(0,START,0x11ac5a0,0x61c1c0,0x44),(2,0x11c0000,0x11dcca0,0x61c1d8,0x46),(3,0x11e0000,0x144a5e0,0x61c1f0,0x47)];
const HOOKS:&[u32]=&[0x110f9d8,0x11e00f8,0x125a260,0x125f16c,0x1274628,0x1279d38,0x127ae64,0x127c06c,0x127ce18,0x127dcc4,0x127fa28,0x1281560,0x128eb8c,0x13e4d98,0x13ed5c0];
const TASKS:[(u32,u32,u32,u32);14]=[(33,0x1109130,9,0x1000),(26,0x11e6e14,9,0xc00),(31,0x125cdfc,9,0xc00),(32,0x1273f60,6,0xc00),(63,0x1279828,7,0xc00),(74,0x127aabc,6,0xc00),(75,0x127bdf4,6,0xc00),(76,0x127cbb0,6,0xc00),(77,0x127da94,6,0xc00),(78,0x127f658,6,0xc00),(79,0x12812c4,6,0xc00),(70,0x128becc,9,0xc00),(87,0x13e36fc,6,0xc00),(86,0x13e9c0c,5,0xc00)];
const SUBMISSIONS:[u32;14]=[0x110f9cc,0x11e00ec,0x125a254,0x125f160,0x127461c,0x1279d2c,0x127ae58,0x127c060,0x127ce0c,0x127dcb8,0x127fa1c,0x1281554,0x13e4d80,0x13ed5a0];

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all="kebab-case")]
pub(super) enum MissingEvidence { PlainWitness, CallerBefore, ProducerEntry, CallerReturn, RawRead, RawTransferWrites, Progress, FreshBlock, InitialView, ProducerReturnView, ExtentView, Publication, TaskEntry, Image5Entry, Frame, CodeGuard, InputReads, WriterAttribution }
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all="kebab-case")]
pub(super) enum AbiBoundary { SourceIdentity, Destination, ProducerEntry, CallbackTarget, SourceOperation, CommittedData, ProducerFrame, SavedStatus, Publication, WriteConfinement, Coverage, Mapping }
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag="refusal",rename_all="kebab-case")]
pub(super) enum Refusal {
    Missing { evidence:MissingEvidence },
    WrongImageIdentity,
    WrongDestination,
    WrongEntry,
    WrongCallback,
    PrematurePublication { extent:u32 },
    OutOfRangeWrite { address:u32 },
    CommittedDataDiffers { address:u32 },
    BadFrame,
    BadStatus,
    SourceOperationDiffers,
    CoverageDiffers,
    SyntheticProgressNotCommitted,
    ReadFailureNotCommitted { status:u32 },
    Abi { boundary:AbiBoundary },
    Artifact { boundary:AbiBoundary, detail:String },
}
type Result<T,E=Refusal>=std::result::Result<T,E>;
fn require(ok:bool,error:Refusal)->Result<()> {if ok{Ok(())}else{Err(error)}}
fn missing(evidence:MissingEvidence)->Refusal{Refusal::Missing {evidence}}
fn source_error(detail:String)->Refusal{Refusal::Artifact {boundary:AbiBoundary::SourceIdentity,detail}}
fn projection_error(detail:String)->Refusal{Refusal::Artifact {boundary:AbiBoundary::CommittedData,detail}}
fn boundary(report:&Report,id:EventId)->Result<&Event>{event(report,id).map_err(|_|Refusal::CoverageDiffers)}
fn scalar(observed:&Event,address:u32)->Result<u32>{at(observed,address).map_err(|_|missing(MissingEvidence::InputReads))}
fn word(view:&Words,address:u32)->Option<u32>{let offset=address.checked_sub(view.mapping.guest.0)?;if offset%4!=0{return None;}view.values.get((offset/4) as usize).copied()}
fn overlaps(low:u64,high:u64,start:u64,end:u64)->bool{low<end&&start<high}
fn extent(id:u32)->Result<(u32,u32,u32,u32,u32)>{EXTENTS.into_iter().find(|item|item.0==id).ok_or(Refusal::SourceOperationDiffers)}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(transparent)]
struct ContentDigest(String);
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
struct ViewEffect { kind:ViewBoundary, start:Guest, bytes:u32, digest:ContentDigest }
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag="kind",content="identity",rename_all="kebab-case")]
enum ViewBoundary { FreshBlock(u32), FirstExtent(u32) }
#[derive(Clone,Copy,Debug,Deserialize,Serialize,PartialEq,Eq)]
#[serde(transparent)]
struct TransferBytes(u32);
#[derive(Clone,Copy,Debug,Deserialize,Serialize,PartialEq,Eq)]
#[serde(transparent)]
struct BlockOffset(u32);
#[derive(Clone,Copy,Debug,Deserialize,Serialize,PartialEq,Eq)]
#[serde(transparent)]
struct BlockCount(u32);
#[derive(Clone,Copy,Debug,Deserialize,Serialize,PartialEq,Eq)]
#[serde(transparent)]
struct Status(u32);
#[derive(Clone,Copy,Debug,Deserialize,Serialize,PartialEq,Eq)]
#[serde(transparent)]
struct TaskId(u32);
#[derive(Clone,Copy,Debug,Deserialize,Serialize,PartialEq,Eq)]
#[serde(transparent)]
struct Priority(u32);
#[derive(Clone,Copy,Debug,Deserialize,Serialize,PartialEq,Eq)]
#[serde(rename_all="kebab-case")]
enum TaskAttribute { Sentinel, Ordinary }
#[derive(Clone,Copy,Debug,Deserialize,Serialize,PartialEq,Eq)]
#[serde(rename_all="kebab-case")]
enum SkipPolicy { SkipBadBlocks }
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag="operation",rename_all="kebab-case")]
enum Operation { Segment6 {pointer:Guest}, Extents {pointer:Guest}, ReadMode, RawRead {offset:BlockOffset,blocks:BlockCount,destination:Guest,callback:Guest,skip:SkipPolicy,status:Status}, FinalProgress {bytes:TransferBytes} }
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
struct TaskEffect { id:TaskId, core:Core, entry:Guest, priority:Priority, stack:TransferBytes, argument:u32, attribute:TaskAttribute, status:Status }
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
struct ExtentEffect { id:u32, hooks:Vec<Guest>, done:bool, release_argument:u32, deferred:Vec<Guest> }
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
struct Effects { operations:Vec<Operation>, progress:Vec<TransferBytes>, status:Status, extents:Vec<ExtentEffect>, tasks:Vec<TaskEffect>, image5:Image5Effect, views:Vec<ViewEffect> }
#[derive(Clone,Debug,Deserialize,Serialize,PartialEq,Eq)]
struct Image5Effect {core:Core,entry:Guest,descriptor:Guest}
#[derive(Clone, Debug, Deserialize, Serialize)]
pub(super) struct Admission { schema:u32, role:Role, scope:Scope, limits:Vec<Limit>, unqualified:[UnqualifiedBoundary;10], effects:Effects }
#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all="kebab-case")]
enum Scope { NaturalReadSuccessObservedBoundaries }
#[derive(Clone,Copy,Debug,Deserialize,Serialize)]
#[serde(rename_all="kebab-case")]
enum UnqualifiedBoundary { NonzeroMode, MissingSegmentNonreturn, ReadFailureErrorSidePublication, NonreturningHook, RetainedResourceOwnership, RetainedNestedCalleeStackDiscipline, PhysicalCacheCoherence, TransientRemapExclusion, CopiedBodyAndComputedCallerClosure, PostConsumerAndWholeMachineLifetime }

fn checked_view(root:&Path,view:&RangeSnapshot,catalog:&Catalog,start:u32,end:u32,source_equal:bool)->Result<ContentDigest>{
    range_mapping(view,start,end)?;
    require(view.mapping.guest==Guest(start)&&view.mapping.bytes.0==end-start,Refusal::WrongDestination)?;
    let source=catalog.0.iter().find(|image|image.binding.kind==SourceKind::Plain).ok_or(Refusal::WrongImageIdentity)?;
    let low=start.checked_sub(START).ok_or(Refusal::WrongDestination)? as usize;
    let expected=source.bytes.get(low..low+(end-start) as usize).ok_or(Refusal::WrongDestination)?;
    let mut digests=Vec::with_capacity(5);let mut difference=None;
    for (index,binding) in view.files.iter().enumerate(){
        let path=artifact(root,binding).map_err(projection_error)?;
        let mut file=File::open(path).map_err(|error|projection_error(error.to_string()))?;
        require(file.metadata().map_err(|error|projection_error(error.to_string()))?.len()==u64::from(end-start),Refusal::CoverageDiffers)?;
        let mut hasher=Sha256::new();let mut offset=0usize;let mut bytes=[0;4096];
        loop {let count=file.read(&mut bytes).map_err(|error|projection_error(error.to_string()))?;if count==0{break;}let high=offset.checked_add(count).ok_or(Refusal::CoverageDiffers)?;require(high<=expected.len(),Refusal::CoverageDiffers)?;hasher.update(&bytes[..count]);if index==4&&difference.is_none(){difference=bytes[..count].iter().zip(&expected[offset..high]).position(|(observed,expected)|observed!=expected).map(|inside|Guest(start+(offset+inside) as u32));}offset=high;}
        require(offset==expected.len(),Refusal::CoverageDiffers)?;
        let digest=format!("{:x}",hasher.finalize());require(digest==binding.sha256.as_str(),Refusal::WrongImageIdentity)?;digests.push(digest);
    }
    require(view.guest_equal==[true;4]&&digests.iter().all(|digest|digest==&digests[4]),Refusal::Abi {boundary:AbiBoundary::Mapping})?;
    require(view.expected_equal==difference.is_none()&&view.first_difference==difference,Refusal::CoverageDiffers)?;
    if source_equal{if let Some(address)=difference{return Err(Refusal::CommittedDataDiffers {address:address.0});}}
    Ok(ContentDigest(digests.pop().ok_or(Refusal::CoverageDiffers)?))
}
fn view_effect(root:&Path,view:&RangeSnapshot,catalog:&Catalog,start:u32,end:u32,equal:bool,kind:ViewBoundary)->Result<ViewEffect>{Ok(ViewEffect {kind,start:Guest(start),bytes:end-start,digest:checked_view(root,view,catalog,start,end,equal)?})}
fn range_mapping(view:&RangeSnapshot,start:u32,end:u32)->Result<()> {
    mapping(&view.mapping).map_err(projection_error)?;mapping(&view.alias_mapping).map_err(projection_error)?;
    require(view.mapping.guest==Guest(start)&&view.alias_mapping.guest==Guest(start|0x40000000)&&view.mapping.physical==Guest(start)&&view.alias_mapping.physical==Guest(start)&&view.mapping.bytes.0==end-start&&view.alias_mapping.bytes.0==end-start&&view.mapping.tcm==view.alias_mapping.tcm&&std::mem::discriminant(&view.mapping.decoder)==std::mem::discriminant(&view.alias_mapping.decoder),Refusal::Abi {boundary:AbiBoundary::Mapping})
}

fn identity(report:&Report,catalog:&Catalog,program:&Program)->Result<()> {
    require(report.identity.update.as_str()==program.update(),Refusal::WrongImageIdentity)?;
    for (kind,digest) in [(SourceKind::Plain,"ee211bdaad39005b4f72e3443a76ca194905381a967cab787d6a4ef14e2d889d"),(SourceKind::Image5,"3cd501c07533938ae4d9347616986fd1649005a588c36b5abf05e36ef4fd892a")]{
        let image=catalog.0.iter().find(|image|image.binding.kind==kind).ok_or(Refusal::WrongImageIdentity)?;
        require(Digest::of(&image.bytes).as_str()==digest&&report.identity.sources.iter().filter(|source|source.kind==kind&&source.sha256.as_str()==digest&&source.start==image.binding.start&&source.bytes.0==image.binding.bytes.0).count()==1,Refusal::WrongImageIdentity)?;
    }
    Ok(())
}
fn limits(evidence:&Evidence)->Result<()> {
    let required=[Limit::PausedMappingSamplesNotTransientRemapExclusion,Limit::FrameSamplesNotContinuousStackConfinement,Limit::FiniteDestinationHaloNotWholeMachineWriteConfinement,Limit::RetainedServiceAndCallerLifetimeOnly];
    require(evidence.limits.len()==required.len()&&required.iter().all(|limit|evidence.limits.iter().filter(|observed|*observed==limit).count()==1),Refusal::CoverageDiffers)
}

pub(super) fn admit(root:&Path,report:&Report,catalog:&Catalog,program:&Program)->Result<Admission>{
    let evidence=report.plain.as_ref().ok_or(missing(MissingEvidence::PlainWitness))?;
    identity(report,catalog,program)?;limits(evidence)?;
    let payload=read(&artifact(root,&evidence.payload).map_err(source_error)?).map_err(source_error)?;
    require(evidence.payload.sha256.as_str()=="8c6f806ef4554128e5be887a76e438a91b7d6cb030c940fc1c8fd6d32d5926c0"&&Digest::of(&payload).as_str()==evidence.payload.sha256.as_str()&&payload.get(0x5a0048..0x900048)==catalog.0.iter().find(|image|image.binding.kind==SourceKind::Plain).map(|image|image.bytes.as_slice()),Refusal::WrongImageIdentity)?;
    let before=boundary(report,evidence.caller_before.ok_or(missing(MissingEvidence::CallerBefore))?)?;
    let entry=boundary(report,evidence.entry.ok_or(missing(MissingEvidence::ProducerEntry))?)?;
    let returned=boundary(report,evidence.caller_return.ok_or(missing(MissingEvidence::CallerReturn))?)?;
    require(before.pc==Guest(0x73b554)&&entry.pc==Guest(0x6a9844)&&returned.pc==Guest(0x73b558)&&before.core==entry.core&&entry.core==returned.core&&before.instructions<entry.instructions&&entry.instructions<returned.instructions,Refusal::WrongEntry)?;
    require(entry.registers.lr==Guest(0x73b558)&&entry.registers.sp==before.registers.sp&&returned.registers.sp==entry.registers.sp&&entry.registers.sp==Guest(0xa1c1e0)&&returned.registers.r[4..12]==entry.registers.r[4..12]&&returned.registers.lr==Guest(0x6a992c)&&returned.registers.cpsr&0xff==entry.registers.cpsr&0xff,Refusal::BadFrame)?;
    require(report.calls.iter().filter(|call|call.kind==CallKind::PlainProducer&&call.before.is_some_and(|id|id.0==before.id.0)&&call.entry.is_some_and(|id|id.0==entry.id.0)&&call.returned.is_some_and(|id|id.0==returned.id.0)).count()==1,Refusal::CoverageDiffers)?;
    for (address,value) in [(0x61c1a4,0x4061c1b0),(0x61c1a8,START),(0x61c1ac,END-START),(0x61c1f8,u32::MAX)] {require(scalar(entry,address)?==value,if address==0x61c1a8{Refusal::WrongDestination}else{Refusal::SourceOperationDiffers})?;}
    for (id,start,end,done,resource) in EXTENTS {let base=done-16;for (address,value) in [(base,id),(base+4,start),(base+8,end-start),(base+12,0),(done,0),(base+20,resource)]{require(scalar(entry,address)?==value,Refusal::SourceOperationDiffers)?;}}
    let operations=operations(report,entry,returned,evidence)?;
    let status=returned.registers.r[0];require(operations.iter().any(|operation|matches!(operation,Operation::RawRead {status:observed,..} if observed.0==status)),Refusal::BadStatus)?;
    let mut views=progress(root,report,catalog,evidence,entry,returned)?;
    frames(report,evidence,entry,returned,program)?;
    guards(root,report,catalog,evidence,entry,returned,program)?;
    samples(root,report,catalog,evidence,entry,returned,&mut views)?;
    let (extents,tasks)=publications(report,evidence)?;
    let image5=image5_effect(report)?;
    views.sort_by_key(|view|match view.kind{ViewBoundary::FreshBlock(id)=>(0,id),ViewBoundary::FirstExtent(id)=>(1,id)});
    Ok(Admission {schema:1,role:evidence.binding.role,scope:Scope::NaturalReadSuccessObservedBoundaries,limits:evidence.limits.clone(),unqualified:[UnqualifiedBoundary::NonzeroMode,UnqualifiedBoundary::MissingSegmentNonreturn,UnqualifiedBoundary::ReadFailureErrorSidePublication,UnqualifiedBoundary::NonreturningHook,UnqualifiedBoundary::RetainedResourceOwnership,UnqualifiedBoundary::RetainedNestedCalleeStackDiscipline,UnqualifiedBoundary::PhysicalCacheCoherence,UnqualifiedBoundary::TransientRemapExclusion,UnqualifiedBoundary::CopiedBodyAndComputedCallerClosure,UnqualifiedBoundary::PostConsumerAndWholeMachineLifetime],effects:Effects {operations,progress:evidence.progress.iter().map(|progress|TransferBytes(progress.cumulative.0)).collect(),status:Status(status),extents,tasks,image5,views}})
}

fn operations(report:&Report,entry:&Event,returned:&Event,evidence:&Evidence)->Result<Vec<Operation>> {
    let calls=report.calls.iter().filter(|call|call.kind==CallKind::PlainService&&call.core==entry.core&&call.before.is_some_and(|id|entry.id.0<id.0&&id.0<returned.id.0)).collect::<Vec<_>>();
    let expected=[0x657114,0x6571a0,0x657184,0x6a8f1c,CALLBACK];
    require(calls.len()==expected.len(),Refusal::SourceOperationDiffers)?;
    let mut effects=Vec::with_capacity(expected.len());
    for (call,target) in calls.into_iter().zip(expected) {
        require(call.target==Guest(target),if target==CALLBACK{Refusal::WrongCallback}else{Refusal::SourceOperationDiffers})?;
        let entered=boundary(report,call.entry.ok_or(missing(MissingEvidence::RawRead))?)?;
        let finished=boundary(report,call.returned.ok_or(missing(MissingEvidence::RawRead))?)?;
        let operation=match target {
            0x657114=>{require(entered.registers.r[0]==6,Refusal::SourceOperationDiffers)?;require(finished.registers.r[0]!=0,Refusal::Abi {boundary:AbiBoundary::SourceOperation})?;Operation::Segment6 {pointer:Guest(finished.registers.r[0])}},
            0x6571a0=>{require(finished.registers.r[0]&!0x40000000==0x61c1a4,Refusal::WrongDestination)?;Operation::Extents {pointer:Guest(finished.registers.r[0])}},
            0x657184=>{require(finished.registers.r[0]==0,Refusal::SyntheticProgressNotCommitted)?;Operation::ReadMode},
            0x6a8f1c=>{
                require(entered.registers.r[2]==START,Refusal::WrongDestination)?;
                require(entered.registers.r[3]==CALLBACK,Refusal::WrongCallback)?;
                require(entered.registers.r[0]==45&&entered.registers.r[1]==27,Refusal::SourceOperationDiffers)?;
                let frame=entered.frame.as_ref().map_err(|_|missing(MissingEvidence::Frame))?;words(frame).map_err(projection_error)?;
                require(word(frame,entered.registers.sp.0)==Some(1),Refusal::Abi {boundary:AbiBoundary::SourceOperation})?;
                require(evidence.raw_calls.len()==1&&evidence.raw_calls[0].0==entered.id.0,Refusal::CoverageDiffers)?;
                require(finished.registers.r[0]==0,Refusal::ReadFailureNotCommitted {status:finished.registers.r[0]})?;
                Operation::RawRead {offset:BlockOffset(45),blocks:BlockCount(27),destination:Guest(START),callback:Guest(CALLBACK),skip:SkipPolicy::SkipBadBlocks,status:Status(finished.registers.r[0])}
            },
            CALLBACK=>{require(entered.registers.r[0]==END-START,Refusal::SourceOperationDiffers)?;Operation::FinalProgress {bytes:TransferBytes(END-START)}},
            _=>return Err(Refusal::SourceOperationDiffers),
        };
        effects.push(operation);
    }
    Ok(effects)
}

fn progress(root:&Path,report:&Report,catalog:&Catalog,evidence:&Evidence,entry:&Event,returned:&Event)->Result<Vec<ViewEffect>> {
    let raw=report.calls.iter().find(|call|call.target==Guest(0x6a8f1c)&&call.entry.is_some_and(|id|evidence.raw_calls.iter().any(|raw|raw.0==id.0))).ok_or(missing(MissingEvidence::RawRead))?;
    let raw_entry=boundary(report,raw.entry.ok_or(missing(MissingEvidence::RawRead))?)?;
    let raw_return=boundary(report,raw.returned.ok_or(missing(MissingEvidence::RawRead))?)?;
    let observed=report.events.iter().filter(|observed|observed.pc==Guest(CALLBACK)&&observed.core==entry.core&&entry.id.0<observed.id.0&&observed.id.0<returned.id.0).collect::<Vec<_>>();
    require(evidence.progress.len()==observed.len()&&observed.len()==28,missing(MissingEvidence::Progress))?;
    let mut views=Vec::with_capacity(27);
    for (index,(progress,observed)) in evidence.progress.iter().zip(observed).enumerate() {
        require(progress.event.0==observed.id.0&&progress.cumulative.0==observed.registers.r[0],Refusal::CoverageDiffers)?;
        let cumulative=BLOCK*((index as u32+1).min(27));
        require(progress.cumulative.0==cumulative,Refusal::SourceOperationDiffers)?;
        if index<27 {
            require(raw_entry.id.0<observed.id.0&&observed.id.0<raw_return.id.0&&observed.registers.lr==Guest(0x6f1a1c),Refusal::WrongCallback)?;
            let fresh=progress.fresh.as_ref().ok_or(missing(MissingEvidence::FreshBlock))?;
            require(fresh.event.0==observed.id.0,Refusal::CoverageDiffers)?;
            views.push(view_effect(root,fresh,catalog,START+BLOCK*index as u32,START+cumulative,true,ViewBoundary::FreshBlock(index as u32))?);
        } else {
            require(progress.fresh.is_none()&&raw_return.id.0<observed.id.0,Refusal::CoverageDiffers)?;
            require(report.calls.iter().any(|call|call.kind==CallKind::PlainService&&call.target==Guest(CALLBACK)&&call.entry.is_some_and(|id|id.0==observed.id.0)&&call.continuation==observed.registers.lr),Refusal::WrongCallback)?;
        }
    }
    Ok(views)
}

fn frames(report:&Report,evidence:&Evidence,entry:&Event,returned:&Event,program:&Program)->Result<()> {
    let sp=entry.registers.sp.0;
    let expected=saved_slots(entry,program);
    let interval=report.events.iter().filter(|event|entry.id.0<=event.id.0&&event.id.0<=returned.id.0).collect::<Vec<_>>();
    require(evidence.frames.len()==interval.len(),missing(MissingEvidence::Frame))?;
    for (frame,observed) in evidence.frames.iter().zip(interval) {
        require(frame.event.0==observed.id.0,Refusal::CoverageDiffers)?;words(&frame.words).map_err(projection_error)?;
        require(frame.words.mapping.guest==Guest(sp-40)&&frame.words.mapping.bytes.0==40,Refusal::BadFrame)?;
        let caller=observed.state.iter().find(|view|view.mapping.guest==Guest(sp)&&view.values.len()==8).ok_or(missing(MissingEvidence::Frame))?;
        let original=entry.state.iter().find(|view|view.mapping.guest==Guest(sp)&&view.values.len()==8).ok_or(missing(MissingEvidence::Frame))?;
        require(caller.values==original.values,Refusal::BadFrame)?;
        if (0x6a9844..0x6a9938).contains(&observed.pc.0){require(sp-if program.owned(){40}else{32}<=observed.registers.sp.0&&observed.registers.sp.0<=sp&&observed.registers.sp.0%8==0,Refusal::BadFrame)?;}
        if observed.id.0==entry.id.0||program.owned()&&observed.pc==Guest(0x6a9848){continue;}
        let rust_started=!program.owned()||report.events.iter().any(|event|entry.id.0<event.id.0&&event.id.0<observed.id.0&&event.pc==Guest(0x6a9854)&&event.core==entry.core);
        for &(address,value) in &expected {if address<sp-8&&!rust_started{continue;}require(word(&frame.words,address)==Some(value),Refusal::BadFrame)?;}
    }
    let code=entry.state.iter().find(|view|view.mapping.guest==Guest(0x6a9844)&&view.values.len()==61).ok_or(missing(MissingEvidence::CodeGuard))?;
    words(code).map_err(projection_error)?;
    Ok(())
}

#[derive(Deserialize,Serialize)]
pub(super) struct AttributedRow {
    #[serde(flatten)]
    pub(super) row:super::Row,
    pub(super) latest_fetch:Option<Fetch>,
    pub(super) core_instructions:Option<u64>,
    pub(super) before:Option<AccessBytes>,
    pub(super) byte_order:Option<ByteOrder>,
}
#[derive(Deserialize,Serialize)]
pub(super) struct Fetch {pub(super) source:Access,pub(super) address:Guest,pub(super) physical:Guest,pub(super) size:u64,pub(super) core:Core,pub(super) pc:Guest,pub(super) instructions:super::Count,pub(super) tick:super::Tick,pub(super) core_instructions:u64,pub(super) byte_order:ByteOrder,pub(super) value:Option<u64>,pub(super) before:Option<AccessBytes>}
#[derive(Deserialize,Serialize,PartialEq,Eq)]
#[serde(rename_all="lowercase")]
pub(super) enum ByteOrder {Little,Big}
#[derive(Deserialize,Serialize)]
pub(super) struct Attribution {pub(super) sources:Vec<Access>,pub(super) ranges:Vec<[u64;2]>}
#[derive(Deserialize,Serialize)]
pub(super) struct Watch {pub(super) enabled:bool,pub(super) violation:Option<String>,pub(super) views:super::WatchViews,pub(super) classes:Vec<super::Class>,pub(super) records:Vec<AttributedRow>,pub(super) fetch_attribution:Option<Attribution>}
#[derive(Deserialize,Serialize)]
#[serde(transparent)]
pub(super) struct AccessBytes(pub(super) Vec<u8>);

fn guards(root:&Path,report:&Report,catalog:&Catalog,evidence:&Evidence,entry:&Event,returned:&Event,program:&Program)->Result<()> {
    let lease=evidence.stack.as_ref().ok_or(missing(MissingEvidence::Frame))?;
    require(lease.event.0==entry.id.0,Refusal::BadFrame)?;
    for view in [&lease.current,&lease.task,&lease.creation]{words(view).map_err(projection_error)?;}
    require(lease.current.mapping.guest==Guest(0x40054760)&&lease.current.values.len()==1&&lease.task.values.len()==3&&lease.creation.values.len()==10,Refusal::BadFrame)?;
    let node=*lease.current.values.first().ok_or(Refusal::BadFrame)?;
    require(lease.task.mapping.guest==Guest(node)&&node.checked_sub(0x4005ff0c).is_some_and(|offset|offset%56==0&&offset/56+1==60)&&lease.creation.mapping.guest==Guest(lease.task.values[2])&&lease.creation.values[0]==2&&lease.creation.values[8]==60&&lease.creation.values[2]==0x73b3a0&&lease.creation.values[1]==0x91482c&&entry.core==Core::Primary,Refusal::BadFrame)?;
    let code_view=evidence.code.as_ref().ok_or(missing(MissingEvidence::CodeGuard))?;
    untyped_view(root,code_view,entry.id,0x6a9844,0x6a9938)?;
    let code=read(&artifact(root,&code_view.files[4]).map_err(source_error)?).map_err(source_error)?;
    for (index,bytes) in code.chunks_exact(4).enumerate(){require(catalog.word(Guest(0x6a9844+4*index as u32)).map_err(source_error)?.1==u32::from_le_bytes(bytes.try_into().map_err(|_|Refusal::CoverageDiffers)?),Refusal::WrongImageIdentity)?;}
    let stack_base=lease.creation.values[5]&!0x40000000;let stack_size=lease.creation.values[4];
    let stack_end=stack_base.checked_add(stack_size).ok_or(Refusal::BadFrame)?;
    require(stack_base==0xa1b600&&stack_size==0xc00&&stack_end==0xa1c200&&entry.registers.sp.0.checked_sub(40).is_some_and(|low|stack_base<=low)&&entry.registers.sp.0<=stack_end,Refusal::BadFrame)?;
    untyped_view(root,&lease.view,entry.id,stack_base,stack_end)?;
    for observed in report.events.iter().filter(|event|event.core==entry.core&&entry.id.0<=event.id.0&&event.id.0<=returned.id.0&&((0x6a9844..0x6a9938).contains(&event.pc.0)||evidence.progress.iter().any(|progress|progress.event.0==event.id.0)||report.calls.iter().any(|call|call.kind==CallKind::PlainService&&(call.entry.is_some_and(|id|id.0==event.id.0)||call.returned.is_some_and(|id|id.0==event.id.0))))) {
        require(stack_base<=observed.registers.sp.0&&observed.registers.sp.0<=stack_end&&observed.registers.sp.0%8==0,Refusal::BadFrame)?;
    }
    let code=entry.state.iter().find(|view|view.mapping.guest==Guest(0x6a9844)&&view.values.len()==61).ok_or(missing(MissingEvidence::CodeGuard))?;
    for (index,&value) in code.values.iter().enumerate(){require(catalog.word(Guest(0x6a9844+4*index as u32)).map_err(source_error)?.1==value,Refusal::WrongImageIdentity)?;}
    let segment_call=report.calls.iter().find(|call|call.kind==CallKind::PlainService&&call.target==Guest(0x657114)&&call.entry.is_some_and(|id|entry.id.0<id.0&&id.0<returned.id.0)).ok_or(missing(MissingEvidence::InputReads))?;
    let segment_return=boundary(report,segment_call.returned.ok_or(missing(MissingEvidence::InputReads))?)?;
    let segment=segment_return.registers.r[0];require(segment!=0&&segment%4==0,Refusal::SourceOperationDiffers)?;
    let header=segment_return.state.iter().find(|view|view.mapping.guest==Guest(segment)&&view.values.len()==8).ok_or(missing(MissingEvidence::InputReads))?;
    words(header).map_err(projection_error)?;
    require(header.values[0]==6&&header.values[5]==0x5a0000&&header.values[6]==END-START,Refusal::SourceOperationDiffers)?;
    let raw=report.calls.iter().find(|call|call.target==Guest(0x6a8f1c)&&call.entry.is_some_and(|id|evidence.raw_calls.iter().any(|raw|id.0==raw.0))).ok_or(missing(MissingEvidence::RawRead))?;
    let raw_entry=boundary(report,raw.entry.ok_or(missing(MissingEvidence::RawRead))?)?;let raw_return=boundary(report,raw.returned.ok_or(missing(MissingEvidence::RawRead))?)?;
    let mode_call=report.calls.iter().find(|call|call.kind==CallKind::PlainService&&call.target==Guest(0x657184)&&call.entry.is_some_and(|id|entry.id.0<id.0&&id.0<returned.id.0)).ok_or(missing(MissingEvidence::InputReads))?;
    let mode_entry=boundary(report,mode_call.entry.ok_or(missing(MissingEvidence::InputReads))?)?;let mode_return=boundary(report,mode_call.returned.ok_or(missing(MissingEvidence::InputReads))?)?;
    let mut reads=[false;5];let mut fetches=BTreeSet::new();let mut stores=BTreeSet::new();
    let mut copied:[Vec<[u64;2]>;27]=std::array::from_fn(|_|Vec::new());let saved=saved_slots(entry,program);let mut saved_writes=BTreeSet::new();
    let expected_attribution=attribution_ranges(catalog)?;
    for epoch in &report.epochs {
        let bytes=read(&artifact(root,&epoch.file).map_err(projection_error)?).map_err(projection_error)?;
        require(Digest::of(&bytes).as_str()==epoch.file.sha256.as_str(),Refusal::CoverageDiffers)?;
        let watch:Watch=serde_json::from_slice(&bytes).map_err(|error|projection_error(error.to_string()))?;
        require(watch.enabled&&watch.violation.is_none()&&watch.records.len()==epoch.record_count,Refusal::CoverageDiffers)?;
        require(guarded(&watch.classes,[0x6a9844,0x6a9938],true)&&guarded(&watch.classes,[0x10fff00,0x1460100],false)&&guarded(&watch.classes,[0xa1b500,0xa1c300],false),missing(MissingEvidence::CodeGuard))?;
        require([[0x61c000,0x61c220],[0x61c400,0x61c600]].iter().all(|span|guarded(&watch.classes,*span,false)&&watch.classes.iter().any(|class|class.range==*span&&class.kinds.contains(&Access::CpuRead))),missing(MissingEvidence::InputReads))?;
        let attribution=watch.fetch_attribution.as_ref().ok_or(missing(MissingEvidence::WriterAttribution))?;
        require(attribution.sources.as_slice()==[Access::CpuRead,Access::CpuWrite]&&attribution.ranges==expected_attribution,Refusal::CoverageDiffers)?;
        let mut future=watch.records.iter().filter(|record|matches!(record.row.source,Access::CpuRead|Access::CpuWrite|Access::CpuFetch)).peekable();let mut lower=None;
        for record in &watch.records {
            let row=&record.row;require(in_class(row,&watch.classes).map_err(projection_error)?,Refusal::CoverageDiffers)?;
            let cpu=matches!(row.source,Access::CpuRead|Access::CpuWrite|Access::CpuFetch);if cpu {lower=row.instructions.map(|clock|clock.0);future.next();}
            let upper=future.peek().and_then(|record|record.row.instructions).map(|clock|clock.0);
            let low=u64::from(row.physical.0);let high=low.checked_add(row.size).ok_or(Refusal::CoverageDiffers)?;
            let during=row.instructions.is_some_and(|clock|entry.instructions.0<clock.0&&clock.0<=returned.instructions.0);
            let may_live=row.instructions.map_or_else(||!(upper.is_some_and(|clock|clock<=entry.instructions.0)||lower.is_some_and(|clock|returned.instructions.0<clock)),|clock|entry.instructions.0<clock.0&&clock.0<=returned.instructions.0);
            if matches!(row.source,Access::CpuRead|Access::CpuWrite) {
                let fetch=record.latest_fetch.as_ref().ok_or(missing(MissingEvidence::WriterAttribution))?;
                require(fetch.source==Access::CpuFetch&&fetch.value.is_none()&&fetch.before.is_none()&&fetch.size==4&&fetch.address==fetch.pc&&fetch.physical.0==fetch.address.0&!0x40000000&&Some(fetch.core)==row.core&&Some(fetch.pc)==row.pc&&Some(fetch.instructions)==row.instructions&&Some(fetch.tick)==row.tick&&Some(fetch.core_instructions)==record.core_instructions&&fetch.byte_order==ByteOrder::Little&&record.byte_order==Some(ByteOrder::Little),Refusal::CoverageDiffers)?;
                let before=record.before.as_ref().ok_or(Refusal::CoverageDiffers)?;require(before.0.len() as u64==row.size,Refusal::CoverageDiffers)?;
                if row.source==Access::CpuRead&&row.size<=8 {let value=before.0.iter().rev().fold(0u64,|value,byte|(value<<8)|u64::from(*byte));require(row.value==Some(value),Refusal::CoverageDiffers)?;}
            }
            if row.source==Access::CpuFetch&&overlaps(low,high,0x6a9844,0x6a9938) {
                require(during&&row.core==Some(entry.core)&&row.address==row.physical&&row.pc==Some(row.address)&&row.size==4&&program.execution_allowed(row.address),Refusal::WrongEntry)?;
                fetches.insert((row.address.0,row.instructions.ok_or(Refusal::CoverageDiffers)?.0));
            }
            if matches!(row.source,Access::CpuWrite|Access::HostWrite|Access::ModelBusWrite) {
                if overlaps(low,high,0x6a9844,0x6a9938) {require(row.instructions.is_some_and(|clock|clock.0<=entry.instructions.0)||upper.is_some_and(|clock|clock<=entry.instructions.0),Refusal::OutOfRangeWrite {address:row.physical.0})?;}
                if may_live&&overlaps(low,high,u64::from(entry.registers.sp.0-if program.owned(){32}else{24}),u64::from(entry.registers.sp.0)) {
                    let push=row.source==Access::CpuWrite&&row.core==Some(entry.core)&&row.pc.is_some_and(|pc|pc==Guest(0x6a9844)||program.owned()&&pc==Guest(0x6a9854));
                    require(push,Refusal::BadFrame)?;
                    let stopped=report.events.iter().find(|event|Some(event.pc)==row.pc&&event.core==entry.core&&entry.id.0<=event.id.0&&event.id.0<=returned.id.0).ok_or(Refusal::BadFrame)?;
                    require(row.instructions.is_some_and(|clock|clock.0==stopped.instructions.0+1),Refusal::BadFrame)?;
                    for &(address,value) in &saved {if low<=u64::from(address)&&u64::from(address)+4<=high {if row.size==4{require(row.value==Some(u64::from(value)),Refusal::BadFrame)?;}saved_writes.insert(address);}}
                }
                if may_live&&overlaps(low,high,u64::from(entry.registers.sp.0),u64::from(stack_end)) {return Err(Refusal::BadFrame);}
                if during&&row.pc.is_some_and(|pc|(0x6a9844..0x6a9938).contains(&(pc.0&!0x40000000))) {
                    let min=entry.registers.sp.0-if program.owned(){40}else{32};
                    require(u64::from(min)<=low&&high<=u64::from(entry.registers.sp.0),Refusal::OutOfRangeWrite {address:row.physical.0})?;
                }
                if may_live&&matches!(row.source,Access::HostWrite|Access::ModelBusWrite)&&(overlaps(low,high,0x10fff00,u64::from(START))||overlaps(low,high,u64::from(END),0x1460100)) {return Err(Refusal::OutOfRangeWrite {address:row.physical.0});}
                if may_live&&overlaps(low,high,u64::from(START),u64::from(END))&&row.source==Access::HostWrite{return Err(Refusal::Abi {boundary:AbiBoundary::WriteConfinement});}
                if row.source==Access::ModelBusWrite&&overlaps(low,high,u64::from(START),u64::from(END))&&lower.is_some_and(|clock|raw_entry.instructions.0<=clock)&&upper.is_some_and(|clock|clock<=raw_return.instructions.0) {
                    require(u64::from(START)<=low&&high<=u64::from(END)&&row.address.0&!0x40000000==row.physical.0&&row.core.is_none()&&row.pc.is_none()&&row.instructions.is_none()&&row.value.is_none(),Refusal::Abi {boundary:AbiBoundary::WriteConfinement})?;
                    for (index,coverage) in copied.iter_mut().enumerate(){let block_start=u64::from(START)+u64::from(BLOCK)*index as u64;let block_end=block_start+u64::from(BLOCK);let progress=boundary(report,evidence.progress[index].event)?;if overlaps(low,high,block_start,block_end)&&upper.is_some_and(|clock|clock<=progress.instructions.0+1){coverage.push([low.max(block_start),high.min(block_end)]);}}
                }
                if may_live&&(overlaps(low,high,u64::from(segment&!0x40000000)+20,u64::from(segment&!0x40000000)+28)||overlaps(low,high,0x61c1a4,0x61c1b0)){return Err(Refusal::Abi {boundary:AbiBoundary::SourceOperation});}
                for (id,_,_,done,_) in EXTENTS {if overlaps(low,high,u64::from(done),u64::from(done+4))&&row.value==Some(1) {
                    let stopped=report.events.iter().find(|event|event.pc==Guest(0x73be70)&&event.registers.r[7]==id).ok_or(missing(MissingEvidence::Publication))?;
                    require(row.source==Access::CpuWrite&&row.pc==Some(Guest(0x73be70))&&row.size==4&&row.physical==Guest(done)&&row.core==Some(stopped.core)&&row.instructions.is_some_and(|clock|clock.0==stopped.instructions.0+1),Refusal::PrematurePublication {extent:id})?;stores.insert(done);
                }}
            }
            if during&&row.source==Access::CpuRead&&row.core==Some(entry.core)&&row.pc==Some(Guest(0x657194))&&row.physical==Guest(0x61c008)&&row.size==4&&row.value==Some(0)&&row.instructions.is_some_and(|clock|mode_entry.instructions.0<clock.0&&clock.0<=mode_return.instructions.0){reads[4]=true;}
            if during&&row.source==Access::CpuRead&&row.core==Some(entry.core)&&row.pc.is_some_and(|pc|(0x6a9844..0x6a9938).contains(&pc.0)) {
                let count=row.instructions.ok_or(Refusal::CoverageDiffers)?.0;
                for (index,address,value,after,before) in [(0,segment+20,0x5a0000,segment_return.instructions.0,raw_entry.instructions.0),(1,segment+24,END-START,segment_return.instructions.0,raw_entry.instructions.0),(2,0x61c1a8,START,segment_return.instructions.0,raw_entry.instructions.0),(3,segment+24,END-START,raw_return.instructions.0,returned.instructions.0)] {
                    if row.physical==Guest(address&!0x40000000)&&row.size==4&&row.value==Some(u64::from(value))&&after<count&&count<=before {reads[index]=true;}
                }
            }
        }
    }
    require(reads==[true;5],missing(MissingEvidence::InputReads))?;
    for observed in report.events.iter().filter(|event|(0x6a9844..0x6a9938).contains(&(event.pc.0&!0x40000000))) {require(fetches.contains(&(observed.pc.0,observed.instructions.0+1)),missing(MissingEvidence::CodeGuard))?;}
    require(EXTENTS.iter().all(|extent|stores.contains(&extent.3)),missing(MissingEvidence::Publication))?;
    require(saved.iter().all(|(address,_)|saved_writes.contains(address)),missing(MissingEvidence::Frame))?;
    for (index,coverage) in copied.iter_mut().enumerate(){coverage.sort_unstable();let start=u64::from(START)+u64::from(BLOCK)*index as u64;let mut committed=start;for span in coverage {require(span[0]<=committed,missing(MissingEvidence::RawTransferWrites))?;committed=committed.max(span[1]);}require(committed==start+u64::from(BLOCK),missing(MissingEvidence::RawTransferWrites))?;}
    Ok(())
}
fn extent_by_done(done:u32)->u32{EXTENTS.iter().find(|extent|extent.3==done).map_or(u32::MAX,|extent|extent.0)}

fn untyped_view(root:&Path,view:&RangeSnapshot,event:EventId,start:u32,end:u32)->Result<()> {
    range_mapping(view,start,end)?;
    require(view.event.0==event.0&&view.guest_equal==[true;4],Refusal::BadFrame)?;
    let mut digests=Vec::with_capacity(5);
    for binding in &view.files {let bytes=read(&artifact(root,binding).map_err(projection_error)?).map_err(projection_error)?;require(bytes.len()==(end-start) as usize&&Digest::of(&bytes).as_str()==binding.sha256.as_str(),Refusal::BadFrame)?;digests.push(Digest::of(&bytes));}
    require(digests.iter().all(|digest|digest.as_str()==digests[0].as_str()),Refusal::BadFrame)
}

fn samples(root:&Path,report:&Report,catalog:&Catalog,evidence:&Evidence,entry:&Event,returned:&Event,views:&mut Vec<ViewEffect>)->Result<()> {
    let mut required=Vec::new();
    required.push((entry.id.0,SampleKind::Initial,START,END,None,false));
    required.push((returned.id.0,SampleKind::ProducerReturn,START,END,None,false));
    let image5=report.events.iter().find(|event|event.pc==Guest(0xd985a4)).ok_or(missing(MissingEvidence::Image5Entry))?;
    required.push((image5.id.0,SampleKind::Image5Entry,START,END,None,false));
    let mut first=BTreeSet::new();
    for observed in &report.events {
        if matches!(observed.pc.0,0x73be5c|0x73be74)||observed.pc==Guest(0x73be70)&&observed.registers.r[7]==2 {
            let (id,start,end,_,_)=extent(observed.registers.r[7])?;
            let hook=observed.pc==Guest(0x73be5c);
            let kind=if hook{SampleKind::ExtentHook}else if observed.pc==Guest(0x73be70){SampleKind::ExtentReady}else{SampleKind::ExtentPublished};
            let initial=first.insert(id);
            required.push((observed.id.0,kind,start,end,if initial{Some(ViewBoundary::FirstExtent(id))}else{None},initial));
        }
    }
    require(first.len()==3,missing(MissingEvidence::ExtentView))?;
    for (_,pc,_,_) in TASKS {
        let observed=report.events.iter().find(|event|event.pc==Guest(pc)).ok_or(missing(MissingEvidence::TaskEntry))?;
        let (_,start,end,_,_)=extent(if pc<0x11c0000{0}else{3})?;
        required.push((observed.id.0,SampleKind::TaskEntry,start,end,None,false));
    }
    require(required.len()==evidence.samples.len(),Refusal::CoverageDiffers)?;
    for (id,kind,start,end,effect,equal) in required {
        let matching=evidence.samples.iter().filter(|sample|sample.event.0==id&&sample.kind==kind).collect::<Vec<_>>();
        require(matching.len()==1,missing(MissingEvidence::ExtentView))?;
        let sample=matching[0];require(sample.view.event.0==id,Refusal::CoverageDiffers)?;
        let digest=checked_view(root,&sample.view,catalog,start,end,equal)?;
        if let Some(kind)=effect{views.push(ViewEffect {kind,start:Guest(start),bytes:end-start,digest});}
    }
    Ok(())
}

fn publications(report:&Report,evidence:&Evidence)->Result<(Vec<ExtentEffect>,Vec<TaskEffect>)> {
    let hooks=report.calls.iter().filter(|call|call.kind==CallKind::PlainHook).collect::<Vec<_>>();
    require(hooks.iter().map(|call|call.target.0).eq(HOOKS.iter().copied()),Refusal::SourceOperationDiffers)?;
    let mut effects=Vec::with_capacity(3);
    for (id,start,end,done,resource) in EXTENTS {
        let sufficient=evidence.progress.iter().find(|progress|START+progress.cumulative.0>=end&&progress.fresh.is_some()).ok_or(missing(MissingEvidence::FreshBlock))?;
        let committed=boundary(report,sufficient.event)?;
        let stored=report.events.iter().filter(|event|event.pc==Guest(0x73be70)&&event.registers.r[7]==id).collect::<Vec<_>>();
        let published=report.events.iter().filter(|event|event.pc==Guest(0x73be74)&&event.registers.r[7]==id).collect::<Vec<_>>();
        let released=report.events.iter().filter(|event|event.pc==Guest(0x73be78)&&event.registers.r[7]==id).collect::<Vec<_>>();
        require(stored.len()==1&&published.len()==1&&released.len()==1,missing(MissingEvidence::Publication))?;
        let (stored,published,released)=(stored[0],published[0],released[0]);
        require(committed.id.0<stored.id.0&&stored.id.0<published.id.0&&published.id.0<released.id.0&&released.registers.r[0]==resource&&stored.registers.r[9]>=end&&stored.registers.r[9]<=END,Refusal::PrematurePublication {extent:id})?;
        require(scalar(stored,done)?==0&&scalar(published,done)?==1,Refusal::PrematurePublication {extent:id})?;
        for observed in [stored,published] {
            let descriptor=observed.descriptor.as_ref().ok_or(missing(MissingEvidence::Publication))?;
            require(descriptor.values.len()==6&&(descriptor.mapping.guest.0&!0x40000000)==done-16&&descriptor.values[0]==id&&descriptor.values[1]==start&&descriptor.values[2]==end-start&&descriptor.values[5]==resource&&descriptor.values[4]==u32::from(observed.pc==Guest(0x73be74)),Refusal::SourceOperationDiffers)?;
        }
        let extent_hooks=hooks.iter().filter(|call|call.before.and_then(|id|report.events.get(id.0)).is_some_and(|event|event.registers.r[7]==id)).collect::<Vec<_>>();
        for call in &extent_hooks {
            let before=boundary(report,call.before.ok_or(missing(MissingEvidence::Publication))?)?;
            let finished=boundary(report,call.returned.ok_or(missing(MissingEvidence::Publication))?)?;
            require(committed.id.0<before.id.0&&finished.id.0<stored.id.0&&before.registers.r[9]>=end&&scalar(before,0x914834)? >=end-START,Refusal::PrematurePublication {extent:id})?;
            require(before.registers.r[10]&!0x40000000>=start&&before.registers.r[10]&!0x40000000<end&&scalar(before,done)?==0,Refusal::SourceOperationDiffers)?;
        }
        let mut deferred=Vec::new();
        for call in report.calls.iter().filter(|call|call.kind==CallKind::Deferred) {
            let before=boundary(report,call.before.ok_or(missing(MissingEvidence::Publication))?)?;
            if before.registers.r[7]!=id{continue;}
            require(released.id.0<before.id.0&&scalar(before,done)?==1,Refusal::PrematurePublication {extent:id})?;
            let address=before.registers.r[5].checked_add(before.registers.r[4]).ok_or(Refusal::CoverageDiffers)?;
            let slot=before.state.iter().find(|view|view.mapping.guest==Guest(address)&&view.values.len()==2).ok_or(missing(MissingEvidence::Publication))?;
            require(slot.values==[4,0],Refusal::SourceOperationDiffers)?;deferred.push(call.target);
        }
        effects.push(ExtentEffect {id,hooks:extent_hooks.iter().map(|call|call.target).collect(),done:true,release_argument:resource,deferred});
    }
    let mut tasks=Vec::with_capacity(TASKS.len());
    for (index,(id,pc,priority,stack)) in TASKS.into_iter().enumerate() {
        let submissions=report.calls.iter().filter(|call|call.kind==CallKind::Task&&call.entry.and_then(|id|report.events.get(id.0)).and_then(|entry|entry.descriptor.as_ref()).is_some_and(|descriptor|descriptor.values.first()==Some(&id))).collect::<Vec<_>>();
        require(submissions.len()==1,missing(MissingEvidence::TaskEntry))?;
        let call=submissions[0];
        let entered=boundary(report,call.entry.ok_or(missing(MissingEvidence::TaskEntry))?)?;let returned=boundary(report,call.returned.ok_or(missing(MissingEvidence::TaskEntry))?)?;
        let descriptor=entered.descriptor.as_ref().ok_or(missing(MissingEvidence::TaskEntry))?;
        require(descriptor.values.len()==7&&descriptor.values[2]==priority&&descriptor.values[3]==stack&&descriptor.values[4]==pc&&descriptor.values[5]==0&&returned.registers.r[0]==0,Refusal::SourceOperationDiffers)?;
        let consumer=report.events.iter().find(|event|event.pc==Guest(pc)).ok_or(missing(MissingEvidence::TaskEntry))?;
        require(entered.id.0<consumer.id.0&&consumer.registers.r[0]==0,Refusal::PrematurePublication {extent:if pc<0x11c0000{0}else{3}})?;
        let hook=HOOKS[if index>=12{index+1}else{index}];
        require(call.continuation==Guest(SUBMISSIONS[index]+4)&&hooks.iter().any(|observed|observed.target==Guest(hook)&&observed.entry.is_some_and(|entry|entry.0<entered.id.0)&&observed.returned.is_some_and(|returned|entered.id.0<returned.0)),Refusal::SourceOperationDiffers)?;
        task_context(consumer,TaskId(id),Priority(priority),TransferBytes(stack),call.core)?;
        tasks.push(TaskEffect {id:TaskId(id),core:consumer.core,entry:Guest(pc),priority:Priority(priority),stack:TransferBytes(stack),argument:0,attribute:if descriptor.values[6]==0xa5f0_a5f0{TaskAttribute::Sentinel}else{TaskAttribute::Ordinary},status:Status(0)});
    }
    Ok((effects,tasks))
}

pub(super) fn compare(retained:&Admission,owned:&Admission)->Result<()> {
    require(retained.role==Role::Retained&&owned.role==Role::Owned,Refusal::Abi {boundary:AbiBoundary::SourceIdentity})?;
    require(retained.effects==owned.effects,Refusal::SourceOperationDiffers)
}

#[derive(Clone,Copy,Debug,Serialize)]
#[serde(rename_all="kebab-case")]
enum Control { Unchanged, WrongImage, WrongDestination, WrongEntry, WrongCallback, PrematurePublication, OutOfRangeWrite, MissingFreshBlock, MissingTransferWrites, BadFrame, BadStatus, ReadFailureWithFinalNotification, SyntheticProgress, MissingCodeGuard, WrongAliasBytes }
impl Control {
    fn name(self)->&'static str {match self {Self::Unchanged=>"unchanged",Self::WrongImage=>"wrong-image",Self::WrongDestination=>"wrong-destination",Self::WrongEntry=>"wrong-entry",Self::WrongCallback=>"wrong-callback",Self::PrematurePublication=>"premature-publication",Self::OutOfRangeWrite=>"out-of-range-write",Self::MissingFreshBlock=>"missing-fresh-block",Self::MissingTransferWrites=>"missing-transfer-writes",Self::BadFrame=>"bad-frame",Self::BadStatus=>"bad-status",Self::ReadFailureWithFinalNotification=>"read-failure-with-final-notification",Self::SyntheticProgress=>"synthetic-progress",Self::MissingCodeGuard=>"missing-code-guard",Self::WrongAliasBytes=>"wrong-alias-bytes"}}
    fn expected(self)->Option<Refusal> {match self {
        Self::Unchanged=>None,Self::WrongImage=>Some(Refusal::WrongImageIdentity),Self::WrongDestination=>Some(Refusal::WrongDestination),Self::WrongEntry=>Some(Refusal::WrongEntry),Self::WrongCallback=>Some(Refusal::WrongCallback),
        Self::PrematurePublication=>Some(Refusal::PrematurePublication {extent:0}),Self::OutOfRangeWrite=>Some(Refusal::OutOfRangeWrite {address:END}),Self::MissingFreshBlock=>Some(missing(MissingEvidence::FreshBlock)),
        Self::MissingTransferWrites=>Some(missing(MissingEvidence::RawTransferWrites)),
        Self::BadFrame=>Some(Refusal::BadFrame),Self::BadStatus=>Some(Refusal::BadStatus),Self::ReadFailureWithFinalNotification=>Some(Refusal::ReadFailureNotCommitted {status:1}),Self::SyntheticProgress=>Some(Refusal::SyntheticProgressNotCommitted),
        Self::MissingCodeGuard=>Some(missing(MissingEvidence::CodeGuard)),Self::WrongAliasBytes=>Some(Refusal::Abi {boundary:AbiBoundary::Mapping}),
    }}
}
#[derive(Serialize)]
struct ControlResult {control:Control,receipt:super::continuation_view::FileBinding,expected:Option<Refusal>,observed:Option<Refusal>,full_consumer_refused:bool,accepted:bool}
#[derive(Serialize)]
struct Controls {schema:u32,baseline:super::continuation_view::FileBinding,accepted:bool,cases:Vec<ControlResult>}

pub(super) fn controls<F>(root:&Path,analysis:&Path,output:&Path,mut full_consumer:F)->std::result::Result<(),String>
where F:FnMut(&Path,&mut Report)->std::result::Result<bool,String> {
    if output.try_exists().map_err(|error|error.to_string())? {return Err("plain controls output already exists".into());}
    let receipt=read(&root.join("continuations.json"))?;
    std::fs::create_dir(output).map_err(|error|error.to_string())?;
    let baseline=super::continuation_view::FileBinding {path:"baseline.json".into(),sha256:Digest::of(&receipt)};
    fujimod_xm1::bridge::write(&output.join(&baseline.path),&receipt)?;
    let mut results=Controls {schema:1,baseline,accepted:false,cases:Vec::new()};
    for control in [Control::Unchanged,Control::WrongImage,Control::WrongDestination,Control::WrongEntry,Control::WrongCallback,Control::PrematurePublication,Control::OutOfRangeWrite,Control::MissingFreshBlock,Control::MissingTransferWrites,Control::BadFrame,Control::BadStatus,Control::ReadFailureWithFinalNotification,Control::SyntheticProgress,Control::MissingCodeGuard,Control::WrongAliasBytes] {
        let case=output.join(control.name());std::fs::create_dir(&case).map_err(|error|error.to_string())?;
        for file in std::fs::read_dir(root).map_err(|error|error.to_string())? {
            let file=file.map_err(|error|error.to_string())?;
            if file.file_name()=="continuations.json"||!file.file_type().map_err(|error|error.to_string())?.is_file(){continue;}
            std::fs::hard_link(file.path(),case.join(file.file_name())).map_err(|error|error.to_string())?;
        }
        let mut report:Report=serde_json::from_slice(&receipt).map_err(|error|error.to_string())?;
        mutate(control,&case,&mut report)?;
        let refused=full_consumer(&case,&mut report)?;
        let serialized=read(&case.join("continuations.json"))?;
        let report:Report=serde_json::from_slice(&serialized).map_err(|error|error.to_string())?;
        let mut catalog=Catalog::open(analysis)?;
        let binding=report.plain.as_ref().ok_or("control plain witness is missing")?.binding.clone();
        let program=Program::from_binding(binding,&case,&mut catalog)?;
        let observed=admit(&case,&report,&catalog,&program).err();
        let expected=control.expected();let accepted=observed==expected&&refused==expected.is_some();
        results.cases.push(ControlResult {control,receipt:super::continuation_view::FileBinding {path:std::path::PathBuf::from(control.name()).join("continuations.json"),sha256:Digest::of(&serialized)},expected,observed,full_consumer_refused:refused,accepted});
        fujimod_xm1::bridge::write(&output.join("plain-controls.json"),&serde_json::to_vec_pretty(&results).map_err(|error|error.to_string())?)?;
        if !accepted{return Err(format!("plain control {control:?} did not reach its exact refusal"));}
    }
    results.accepted=true;
    fujimod_xm1::bridge::write(&output.join("plain-controls.json"),&serde_json::to_vec_pretty(&results).map_err(|error|error.to_string())?)
}

fn mutate(control:Control,root:&Path,report:&mut Report)->std::result::Result<(),String> {
    let evidence=report.plain.as_ref().ok_or("plain evidence absent")?;
    let entry=evidence.entry.ok_or("plain entry absent")?.0;let returned=evidence.caller_return.ok_or("plain caller return absent")?.0;
    match control {
        Control::Unchanged=>{},
        Control::WrongImage=>{
            let plain=report.identity.sources.iter_mut().find(|source|source.kind==SourceKind::Plain).ok_or("plain source identity absent")?;
            plain.sha256=Digest::of(b"wrong plain image control");
        },
        Control::WrongEntry=>{
            let original=report.events[entry].state.iter().find(|view|view.mapping.guest==Guest(0x6a9844)&&view.values.len()==61).ok_or("whole carrier entry view absent")?.values[1];
            report.events[entry].pc=Guest(0x6a9848);report.events[entry].code.mapping.guest=Guest(0x6a9848);report.events[entry].code.mapping.physical=Guest(0x6a9848);report.events[entry].code.values[0]=original;
            let call=report.calls.iter_mut().find(|call|call.kind==CallKind::PlainProducer&&call.entry.is_some_and(|id|id.0==entry)).ok_or("plain source call absent")?;call.target=Guest(0x6a9848);
        },
        Control::WrongDestination|Control::WrongCallback=>{
            let call=report.calls.iter().find(|call|call.target==Guest(0x6a8f1c)&&call.entry.is_some_and(|id|entry<id.0&&id.0<returned)).ok_or("raw read call absent")?;
            let before=call.before.ok_or("raw source call absent")?.0;let entered=call.entry.ok_or("raw entry absent")?.0;
            let (register,value)=if matches!(control,Control::WrongDestination){(2,START+4)}else{(3,CALLBACK+4)};
            report.events[before].registers.r[register]=value;report.events[entered].registers.r[register]=value;
        },
        Control::MissingFreshBlock=>{report.plain.as_mut().ok_or("plain evidence absent")?.progress.first_mut().ok_or("progress absent")?.fresh=None;},
        Control::MissingCodeGuard=>{report.plain.as_mut().ok_or("plain evidence absent")?.code=None;},
        Control::BadFrame=>{report.events[returned].registers.r[4]^=1;},
        Control::BadStatus=>{report.events[returned].registers.r[0]=1;},
        Control::ReadFailureWithFinalNotification=>{
            let finished=report.calls.iter().find(|call|call.target==Guest(0x6a8f1c)&&call.entry.is_some_and(|id|entry<id.0&&id.0<returned)).and_then(|call|call.returned).ok_or("raw read return absent")?.0;
            report.events[finished].registers.r[0]=1;report.events[returned].registers.r[0]=1;
            let final_progress=report.plain.as_ref().and_then(|plain|plain.progress.last()).ok_or("final progress absent")?;
            if final_progress.cumulative.0!=END-START||final_progress.fresh.is_some(){return Err("error-side final notification control lost source effect".into());}
        },
        Control::SyntheticProgress=>{
            let finished=report.calls.iter().find(|call|call.target==Guest(0x657184)&&call.entry.is_some_and(|id|entry<id.0&&id.0<returned)).and_then(|call|call.returned).ok_or("mode query return absent")?.0;
            report.events[finished].registers.r[0]=1;
        },
        Control::WrongAliasBytes=>{
            let fresh=report.plain.as_mut().and_then(|plain|plain.progress.first_mut()).and_then(|progress|progress.fresh.as_mut()).ok_or("fresh block absent")?;
            let mut bytes=read(&artifact(root,&fresh.files[2])?)?;let first=bytes.first_mut().ok_or("alias bytes empty")?;*first^=1;
            let name="control-wrong-alias.bin";fujimod_xm1::bridge::write(&root.join(name),&bytes)?;
            fresh.files[2]=super::continuation_view::FileBinding {path:name.into(),sha256:Digest::of(&bytes)};fresh.guest_equal[2]=false;
        },
        Control::MissingTransferWrites=>{
            let mut changed=false;
            for (index,epoch) in report.epochs.iter_mut().enumerate(){
                let mut watch:Watch=serde_json::from_slice(&read(&artifact(root,&epoch.file)?)?).map_err(|error|error.to_string())?;
                let count=watch.records.len();watch.records.retain(|record|record.row.source!=Access::ModelBusWrite||!overlaps(u64::from(record.row.physical.0),u64::from(record.row.physical.0)+record.row.size,u64::from(START),u64::from(END)));
                if count!=watch.records.len(){let bytes=serde_json::to_vec_pretty(&watch).map_err(|error|error.to_string())?;let name=format!("control-watch-{index}.json");fujimod_xm1::bridge::write(&root.join(&name),&bytes)?;epoch.file=super::continuation_view::FileBinding {path:name.into(),sha256:Digest::of(&bytes)};epoch.record_count=watch.records.len();changed=true;}
            }
            if !changed{return Err("raw plain transfer writes absent from serialized evidence".into());}
        },
        Control::PrematurePublication|Control::OutOfRangeWrite=>{
            let mut changed=false;
            for (index,epoch) in report.epochs.iter_mut().enumerate() {
                let bytes=read(&artifact(root,&epoch.file)?)?;
                let mut watch:Watch=serde_json::from_slice(&bytes).map_err(|error|error.to_string())?;
                if let Some(record)=watch.records.iter_mut().find(|record|record.row.source==Access::CpuWrite&&record.row.pc.is_some_and(|pc|pc.0==0x6a9898||pc.0==0x6a9908)&&record.row.instructions.is_some_and(|clock|report.events[entry].instructions.0<clock.0&&clock.0<report.events[returned].instructions.0)) {
                    let address=if matches!(control,Control::OutOfRangeWrite){END}else{0x61c1c0};
                    record.row.address=Guest(address);record.row.physical=Guest(address);record.row.value=Some(1);record.row.size=4;
                    if matches!(control,Control::PrematurePublication) {
                        record.row.pc=Some(Guest(0x73be70));let fetch=record.latest_fetch.as_mut().ok_or("writer fetch absent")?;
                        fetch.pc=Guest(0x73be70);fetch.address=fetch.pc;fetch.physical=fetch.pc;
                    }
                    let bytes=serde_json::to_vec_pretty(&watch).map_err(|error|error.to_string())?;let name=format!("control-watch-{index}.json");
                    fujimod_xm1::bridge::write(&root.join(&name),&bytes)?;epoch.file=super::continuation_view::FileBinding {path:name.into(),sha256:Digest::of(&bytes)};changed=true;break;
                }
            }
            if !changed{return Err("source fifth-argument store absent from serialized watch".into());}
        },
    }
    Ok(())
}

#[derive(Deserialize)]
struct CanonicalRun {boot_rom:BootRom}
#[derive(Deserialize)]
struct BootRom {address:Guest,bytes:super::continuation_view::ByteCount}
fn attribution_ranges(catalog:&Catalog)->Result<Vec<[u64;2]>> {
    let run:CanonicalRun=serde_json::from_slice(include_bytes!("../data/xm1-150.json")).map_err(|error|source_error(error.to_string()))?;
    let mut ranges=catalog.0.iter().map(|image|[u64::from(image.binding.start.0),u64::from(image.binding.start.0)+image.bytes.len() as u64]).collect::<Vec<_>>();
    ranges.push([u64::from(run.boot_rom.address.0),u64::from(run.boot_rom.address.0)+u64::from(run.boot_rom.bytes.0)]);
    ranges.sort_unstable();let mut normalized:Vec<[u64;2]>=Vec::with_capacity(ranges.len());
    for range in ranges {require(range[0]<range[1]&&range[1]<=1u64<<32,Refusal::CoverageDiffers)?;if let Some(previous)=normalized.last_mut(){if range[0]<=previous[1]{previous[1]=previous[1].max(range[1]);continue;}}normalized.push(range);}
    Ok(normalized)
}

fn task_context(consumer:&Event,id:TaskId,priority:Priority,stack:TransferBytes,core:Core)->Result<()> {
    let (current_address,node_base,creation_base)=match core{Core::Primary=>(0x40054760,0x4005ff0c,0x4005beac),Core::Secondary=>(0x400e4720,0x400efecc,0x400ebe6c)};
    require(consumer.core==core&&consumer.registers.cpsr&0x010002bf==0x13,Refusal::Abi {boundary:AbiBoundary::ProducerEntry})?;
    let current=consumer.state.iter().find(|view|view.mapping.guest==Guest(current_address)&&view.values.len()==3).ok_or(missing(MissingEvidence::TaskEntry))?;
    let node=node_base+56*(id.0-1);let creation=creation_base+40*(id.0-1);
    require(current.values[0]==node&&consumer.registers.r[1]==node&&consumer.registers.r[2]==creation,Refusal::SourceOperationDiffers)?;
    let task=consumer.state.iter().find(|view|view.mapping.guest==Guest(node)&&view.values.len()==3).ok_or(missing(MissingEvidence::TaskEntry))?;
    let record=consumer.state.iter().find(|view|view.mapping.guest==Guest(creation)&&view.values.len()==10).ok_or(missing(MissingEvidence::TaskEntry))?;
    let kernel_priority=priority.0.checked_sub(1).ok_or(Refusal::SourceOperationDiffers)?;
    require(task.values[2]==creation&&record.values[0]==2&&record.values[1]==0&&record.values[2]==consumer.pc.0&&record.values[3]==kernel_priority&&record.values[4]==stack.0&&record.values[8]==id.0,Refusal::SourceOperationDiffers)?;
    let base=record.values[5]&!0x40000000;require(base%8==0&&base.checked_add(stack.0)==Some(consumer.registers.sp.0)&&consumer.registers.sp.0%8==0,Refusal::BadFrame)
}
fn image5_effect(report:&Report)->Result<Image5Effect> {
    let call=report.calls.iter().find(|call|call.kind==CallKind::Image5Entry).ok_or(missing(MissingEvidence::Image5Entry))?;
    let before=boundary(report,call.before.ok_or(missing(MissingEvidence::Image5Entry))?)?;let entered=boundary(report,call.entry.ok_or(missing(MissingEvidence::Image5Entry))?)?;
    let descriptor=before.state.iter().find(|view|view.mapping.guest==Guest(before.registers.r[0])&&view.values.len()==2).ok_or(missing(MissingEvidence::Image5Entry))?;
    let header=before.state.iter().find(|view|view.mapping.guest==Guest(descriptor.values[1])&&view.values.len()==4).ok_or(missing(MissingEvidence::Image5Entry))?;
    require(descriptor.values[0]==5&&header.values[3]==0xd985a4&&entered.pc==Guest(0xd985a4)&&entered.registers.r[0]==before.registers.r[0],Refusal::WrongEntry)?;
    Ok(Image5Effect {core:entered.core,entry:entered.pc,descriptor:Guest(entered.registers.r[0])})
}

fn saved_slots(entry:&Event,program:&Program)->Vec<(u32,u32)> {
    let sp=entry.registers.sp.0;
    if program.owned(){vec![(sp-32,entry.registers.r[4]),(sp-28,entry.registers.r[5]),(sp-24,entry.registers.r[6]),(sp-20,entry.registers.r[10]),(sp-16,entry.registers.r[11]),(sp-12,0x6a984c),(sp-8,entry.registers.r[11]),(sp-4,entry.registers.lr.0)]}else{vec![(sp-24,entry.registers.r[4]),(sp-20,entry.registers.r[5]),(sp-16,entry.registers.r[6]),(sp-12,entry.registers.r[7]),(sp-8,entry.registers.r[11]),(sp-4,entry.registers.lr.0)]}
}
