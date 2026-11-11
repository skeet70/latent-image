use crate::{modal_memory::FixedMemory, provenance::Guest};
use fujimod_emulator::{board::{factory::MemoryMap, observation::{WatchKinds, WatchRange}}, session::Session};
use fujimod_emulator_cpu::{Register, arm};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;
#[path = "af_memory.rs"]
pub mod memory;
pub use memory::{MemoryRefusal, ReadBank, ReadCore, ReadView, TcmValue, ReadPurpose, ReadProvenance};

struct MappingCatalog<'a>(&'a [MemoryMap]);
impl Serialize for MappingCatalog<'_> {
    fn serialize<S:serde::Serializer>(&self,serializer:S)->Result<S::Ok,S::Error> {
        use serde::ser::SerializeSeq;
        #[derive(Serialize)]
        struct Map { core:usize,address:u64,physical:u64,size:usize,permissions:u32 }
        let mut sequence=serializer.serialize_seq(Some(self.0.len()))?;
        for map in self.0 {
            sequence.serialize_element(&Map {core:map.core,address:map.address,physical:map.physical,
                size:map.size,permissions:map.permissions})?;
        }
        sequence.end()
    }
}

pub const ROW_LIMIT: usize = 65_536;
const CONTEXT_LIMIT: usize = 8192;
const OUTPUT_LIMIT: usize = 128 * 1024 * 1024;

#[derive(Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SiteKind { Consumer, ModeHelper, Primary, Current, Set, Callback, Clear, Request, WorkPost, QueuePost, Dequeue, Select, DispatchCallback, ReservePrimary, ReserveSecondary }
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RegisterRead { pub register: u8, pub offset: i32, pub bytes: u32 }
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Site { pub pc: u32, pub word: u32, pub kind: SiteKind, pub reads: Vec<[u32; 2]>, #[serde(default)] pub register_reads: Vec<RegisterRead> }
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Plan { pub sites: Vec<Site>, pub identity: Value }

const SNAPSHOT_RANGES: &[(u64,u64)] = &[
    (0x016f_ed28,0x016f_ee48),(0x016f_ec4c,0x016f_ec50),(0x016f_ec3c,0x016f_ec40),
    (0x0170_3080,0x0170_3088),(0x0170_319c,0x0170_31a0),(0x0170_31a4,0x0170_31a8),
    (0x0170_327c,0x0170_3280),(0x0170_306c,0x0170_3070),(0x0170_30d0,0x0170_30d4),
    (0x0170_3160,0x0170_3164)];
const QUEUE_RANGES: &[(u64,u64)] = &[(0x016f_e9bc,0x016f_ec34),(0x0170_30d4,0x0170_30ec),(0x0170_6a38,0x0170_6e44)];
pub fn ranges() -> Vec<WatchRange> {
    SNAPSHOT_RANGES.iter().chain(QUEUE_RANGES).map(|&(start,end)| WatchRange {start,end,kinds:WatchKinds::WRITES}).collect()
}

pub fn attribution_ranges(maps:&[MemoryMap])->Result<Vec<WatchRange>,String> {
    let mut spans=maps.iter().map(|map| {
        let end=map.physical.checked_add(map.size as u64).filter(|end|*end<=1<<32);
        if map.size==0 || map.core>=2 {return Err("AF attribution mapping is invalid".to_owned());}
        Ok((map.physical,end.ok_or("AF attribution mapping leaves32bits")?))
    }).collect::<Result<Vec<_>,String>>()?;
    spans.sort_unstable();
    let mut ranges:Vec<WatchRange>=Vec::new();
    for (start,end) in spans {
        if let Some(previous)=ranges.last_mut().filter(|previous|start<=previous.end) {
            previous.end=previous.end.max(end);
        } else {ranges.push(WatchRange{start,end,kinds:WatchKinds::CPU_FETCH});}
    }
    if ranges.is_empty() || ranges.len()>64 {return Err("AF attribution backing count exceeds bound".into());}
    Ok(ranges)
}
pub fn selects(row: &Value) -> Result<bool, String> {
    let physical = row.get("physical").and_then(Value::as_u64).ok_or("AF row lacks physical provenance")?;
    let width = row.get("size").and_then(Value::as_u64).filter(|n| *n > 0).ok_or("AF row lacks width")?;
    let end = physical.checked_add(width).filter(|end| *end <= 1 << 32).ok_or("AF row wraps")?;
    Ok(SNAPSHOT_RANGES.iter().chain(QUEUE_RANGES).any(|&(start,limit)| physical < limit && start < end))
}
#[derive(Serialize)]
pub struct Word { address: u32, value: u32 }
#[derive(Serialize)]
pub struct Boundary { counts: Vec<u64>, words: Vec<Word> }
#[derive(Serialize)]
struct Read { address: u32, provenance: ReadProvenance, bytes: Vec<u8> }
/// A context read row: the captured bytes, or a null record pointer at the site's declared span.
#[derive(Serialize)]
#[serde(untagged)]
enum ReadRow { Captured(Read), Null }
#[derive(Serialize)]
struct Context { sequence: usize, core: usize, pc: u32, word: u32, kind: SiteKind, entry:EntryEvidence, cpsr: u32, registers: [u32;13], sp:u32, lr:u32, counts:Vec<u64>, cursor:usize, sampled_task:TaskSample, reads:Vec<ReadRow> }

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "u32")]
pub struct ReadBytes(u32);
impl TryFrom<u32> for ReadBytes {
    type Error = String;
    fn try_from(value: u32) -> Result<Self, String> {
        if (1..=4096).contains(&value) { Ok(Self(value)) } else { Err("AF read leaves its bounded span".into()) }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "u8")]
pub struct ReadRegister(u8);
impl TryFrom<u8> for ReadRegister {
    type Error = String;
    fn try_from(value: u8) -> Result<Self, String> {
        if value <= 14 { Ok(Self(value)) } else { Err("AF read register exceeds LR".into()) }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "u16")]
pub struct ReadIndex(u16);
impl TryFrom<u16> for ReadIndex {
    type Error = String;
    fn try_from(value: u16) -> Result<Self, String> {
        if value < 128 { Ok(Self(value)) } else { Err("AF read index exceeds bound".into()) }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum ReadOrigin {
    Instruction,
    Absolute { index: ReadIndex },
    Register { index: ReadIndex, register: ReadRegister, base: Guest, offset: i32 },
}
pub(crate) fn serialize_mappings<S: serde::Serializer>(maps: &[MemoryMap], serializer: S) -> Result<S::Ok, S::Error> {
    MappingCatalog(maps).serialize(serializer)
}
#[derive(Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "kebab-case", deny_unknown_fields)]
pub enum RefusalMappings {
    Retained {
        #[serde(serialize_with = "serialize_mappings", deserialize_with = "crate::af_ownership_receipt::bounded_mappings")]
        mappings: Vec<MemoryMap>,
    },
    CapacityExceeded { entries: usize },
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "i64")]
pub struct ComputedReadAddress(i64);
impl TryFrom<i64> for ComputedReadAddress {
    type Error = String;
    fn try_from(value: i64) -> Result<Self, String> {
        if (-2_147_483_648..=6_442_450_942).contains(&value) { Ok(Self(value)) }
        else { Err("AF computed address exceeds register-plus-offset range".into()) }
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReadRefusal {
    pub core: memory::ReadCore,
    pub pc: Guest,
    pub counts: [u64; 2],
    pub origin: ReadOrigin,
    #[serde(deserialize_with = "crate::af_ownership_receipt::present")]
    pub address: Option<Guest>,
    pub computed_address: ComputedReadAddress,
    pub bytes: ReadBytes,
    pub purpose: ReadPurpose,
    pub execution: ExecutionContext,
    pub refusal: memory::MemoryRefusal,
    pub live_mappings: RefusalMappings,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "kebab-case", deny_unknown_fields)]
pub enum TaskSample {
    Sampled { kernel: ReadCore, id: u16, node: Guest, creation: Guest, descriptor: [u32; 10], context_depth: u32 },
    Refused { error: String, #[serde(deserialize_with = "crate::af_ownership_receipt::present")] context_depth: Option<u32> },
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionContext { pub entry: EntryEvidence, pub cpsr: u32, pub registers: [u32; 13], pub sp: u32, pub lr: u32, pub sampled_task: TaskSample }
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EntryTcm { pub address: u64, pub bytes: usize }
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EntryEvidence { pub opaque_id: OpaqueEntryId, pub address: u64, pub bytes: u32, pub progress: u64, pub pstate: u32, #[serde(deserialize_with = "crate::af_ownership_receipt::present")] pub physical: Option<u64>, pub tcm: [Option<EntryTcm>; 2] }
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(try_from = "String")]
pub struct OpaqueEntryId(String);
impl TryFrom<String> for OpaqueEntryId {
    type Error=String;
    fn try_from(value:String)->Result<Self,String> {
        if value.len()<=512 && value.starts_with("InstructionEntryId {") { Ok(Self(value)) } else { Err("AF opaque engine entry identity differs".into()) }
    }
}
fn sample_task(session: &Session, maps: &[MemoryMap], core: usize) -> TaskSample {
    match crate::modal_task::sample_validated_context(&FixedMemory { session, maps }, crate::modal_permit::CoreId(core)) {
        Ok(sample) => TaskSample::Sampled { kernel: match sample.identity.kernel { crate::modal_task::KernelId::Primary => ReadCore::Primary, crate::modal_task::KernelId::Secondary => ReadCore::Secondary }, id: sample.identity.id.get(), node: sample.identity.node.address(), creation: sample.identity.creation.address(), descriptor: sample.descriptor, context_depth: sample.context_depth },
        Err(error) => {
            let kernel=match core { 0=>crate::modal_task::KernelId::Primary,_=>crate::modal_task::KernelId::Secondary };
            let mut depth=[0;4];
            let context_depth=crate::modal_task::ValidatedTaskMemory::read(&FixedMemory{session,maps},kernel.context_cell(),&mut depth).ok().map(|()|u32::from_le_bytes(depth));
            TaskSample::Refused { error: format!("{error:?}"), context_depth }
        },
    }
}
struct ContextReader<'a> {
    session: &'a Session,
    maps: &'a [MemoryMap],
    core: memory::ReadCore,
    pc: Guest,
    refusal: &'a mut Option<ReadRefusal>,
    execution: Option<ExecutionContext>,
}
enum ReadFailureMessage { Memory, AddressWrap, ExtentWrap }
impl ReadFailureMessage {
    fn render(self) -> String {
        match self {
            Self::Memory => crate::modal_lifetime::LifetimeFailure::Memory.to_string(),
            Self::AddressWrap => "AF payload address wraps".into(),
            Self::ExtentWrap => "AF read leaves its bounded span".into(),
        }
    }
}
impl ContextReader<'_> {
    fn refuse(&mut self, origin: ReadOrigin, computed_address: ComputedReadAddress, bytes: ReadBytes,
        refusal: MemoryRefusal, message: ReadFailureMessage) -> Result<Read, String> {
        let maps = self.session.board.mappings();
        let live_mappings = if maps.len() <= 4096 {
            RefusalMappings::Retained { mappings: maps.to_vec() }
        } else { RefusalMappings::CapacityExceeded { entries: maps.len() } };
        let observed = counts(self.session)?;
        let address = u32::try_from(computed_address.0).ok().map(Guest);
        *self.refusal = Some(ReadRefusal {
            core: self.core, pc: self.pc, counts: [observed[0], observed[1]], origin,
            address, computed_address, bytes,
            purpose: if matches!(origin, ReadOrigin::Register { .. }) { ReadPurpose::ExecutingCore } else { ReadPurpose::SharedPhysical },
            execution: self.execution.take().ok_or("AF refusal execution context is absent")?,
            refusal, live_mappings,
        });
        Err(message.render())
    }

    fn read(&mut self, origin: ReadOrigin, computed: ComputedReadAddress, bytes: u32) -> Result<Read, String> {
        let bytes = ReadBytes::try_from(bytes)?;
        let bounds = || MemoryRefusal { error:memory::ReadError::Bounds,
            view:ReadView::Bounds, observed_tcm:[[None;2];2] };
        let Ok(address) = u32::try_from(computed.0) else {
            return self.refuse(origin,computed,bytes,bounds(),ReadFailureMessage::AddressWrap);
        };
        if u64::from(address) + u64::from(bytes.0) > 1 << 32 {
            return self.refuse(origin,computed,bytes,bounds(),ReadFailureMessage::ExtentWrap);
        }
        let mut output = vec![0; bytes.0 as usize];
        let result = if matches!(origin, ReadOrigin::Register { .. }) {
            memory::read_cpu_visible(&self.session.board, self.maps, self.core, Guest(address), &mut output)
        } else { memory::read_shared(self.session, self.maps, Guest(address), &mut output) };
        match result {
            Ok(provenance) => Ok(Read { address, provenance, bytes: output }),
            Err(refusal) => self.refuse(origin,computed,bytes,refusal,ReadFailureMessage::Memory),
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ObservationMode { Passive }
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RetentionStop { End, Refused, CapacityExceeded, ProvenanceUnavailable }
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "kebab-case", deny_unknown_fields)]
pub enum RetentionStatus { Disabled, Unarmed, Armed, Stopped { reason: RetentionStop } }
#[derive(Serialize)]
struct Limits { rows:usize, contexts:usize, pairs:usize, output_bytes:usize }
#[derive(Serialize)]
pub struct Receipt<'a> { schema:u32, mode:ObservationMode, retention:Option<RetentionStatus>, status:&'static str, identity:&'a Value, initial:&'a Option<Boundary>, final_boundary:&'a Option<Boundary>, rows:&'a [Value], contexts:&'a [Context], failure:&'a Option<String>, read_refusal:&'a Option<ReadRefusal>, limits:Limits, mappings:MappingCatalog<'a> }
impl Serialize for Projection {
    fn serialize<S: serde::Serializer>(&self, serializer:S)->Result<S::Ok,S::Error> {
        use serde::ser::SerializeMap;
        let receipt=self.receipt();
        let mut map=serializer.serialize_map(Some(15))?;
        map.serialize_entry("schema",&receipt.schema)?; map.serialize_entry("status",receipt.status)?;
        map.serialize_entry("mode",&receipt.mode)?; map.serialize_entry("retention",&receipt.retention)?;
        map.serialize_entry("identity",receipt.identity)?; map.serialize_entry("initial",receipt.initial)?;
        map.serialize_entry("final",receipt.final_boundary)?; map.serialize_entry("rows",receipt.rows)?;
        map.serialize_entry("contexts",receipt.contexts)?; map.serialize_entry("failure",receipt.failure)?;
        map.serialize_entry("read_refusal",receipt.read_refusal)?;
        map.serialize_entry("row_count",&self.rows.len())?;
        map.serialize_entry("context_count",&self.contexts.len())?;
        map.serialize_entry("limits",&receipt.limits)?;
        map.serialize_entry("mappings",&receipt.mappings)?; map.end()
    }
}
pub struct Projection { plan:Plan, initial:Option<Boundary>, final_boundary:Option<Boundary>, rows:Vec<Value>, contexts:Vec<Context>, failure:Option<String>, read_refusal:Option<ReadRefusal>, retention:Option<RetentionStatus>, bytes:usize, pairs:usize, maps:Arc<Vec<MemoryMap>> }
fn counts(session:&Session)->Result<Vec<u64>,String> {
    let counts:Vec<u64>=session.board.instruction_counts().into_iter().collect();
    if counts.len()!=2 { return Err("AF diagnostic requires both cores".into()); } Ok(counts)
}
fn read(session:&Session,maps:&[MemoryMap],address:u32,bytes:u32)->Result<Read,String> {
    if bytes==0 || bytes>4096 || address.checked_add(bytes).is_none() {return Err("AF read leaves its bounded span".into());}
    let mut output=vec![0;bytes as usize];
    let provenance=memory::read_shared(session,maps,Guest(address),&mut output).map_err(|failure|format!("AF shared read: {:?}",failure))?;
    Ok(Read{address,provenance,bytes:output})
}
fn boundary(session:&Session,maps:&[MemoryMap])->Result<Boundary,String> {
    let mut words=Vec::new();
    for &(start,end) in SNAPSHOT_RANGES {for address in (start..end).step_by(4) {
        let bytes=read(session,maps,address as u32,4)?.bytes;
        words.push(Word{address:address as u32,value:u32::from_le_bytes(bytes.try_into().map_err(|_|"AF boundary width")?)});
    }}
    words.sort_unstable_by_key(|word|word.address);
    Ok(Boundary{counts:counts(session)?,words})
}
impl Projection {
    pub fn new(plan:Plan,session:&Session,maps:Arc<Vec<MemoryMap>>)->Result<Self,String> {
        crate::af_ownership_receipt::validate_plan(&plan)?;
        crate::af_ownership_receipt::validate_mappings(&maps)?;
        for site in &plan.sites {
            let actual=read(session,&maps,site.pc,4)?;
            if actual.bytes!=site.word.to_le_bytes() {return Err("AF instruction differs from byte-bound plan".into());}
        }
        Ok(Self{plan,initial:None,final_boundary:None,rows:Vec::new(),contexts:Vec::new(),failure:None,read_refusal:None,retention:None,bytes:2*1024*1024,pairs:0,maps})
    }
    pub fn sites(&self)->impl Iterator<Item=u32>+'_ {self.plan.sites.iter().map(|site|site.pc)}
    pub fn contains(&self,pc:u32)->bool {self.sites().any(|site|site==pc)}
    pub fn arm(&mut self,session:&Session,maps:&[MemoryMap])->Result<(),String> {
        let result=(|| {
            if maps != self.maps.as_slice() {return Err("AF mapping catalogue changed before arm".to_owned());}
            if self.initial.is_some() || self.failure.is_some() {return Err("AF projection cannot be rearmed".to_owned());}
            self.initial=Some(boundary(session,maps)?);
            Ok(())
        })();
        if let Err(failure)=&result {self.fail(failure.clone());}
        result
    }
    pub fn active(&self)->bool {self.initial.is_some() && self.final_boundary.is_none() && self.failure.is_none() && self.retention==Some(RetentionStatus::Armed)}
    fn retain_bytes(&mut self,bytes:usize)->Result<(),String> {
        self.bytes=self.bytes.checked_add(bytes).ok_or("AF output byte count overflow")?;
        if self.bytes>OUTPUT_LIMIT {return Err("AF output capacity exhausted".into());} Ok(())
    }
    pub fn row(&mut self,row:&Value) {
        if let Err(failure)=self.retain_row(row) {self.fail(failure);}
    }
    fn retain_row(&mut self,row:&Value)->Result<(),String> {
        if !self.active(){return Ok(());}
        if self.rows.len()==ROW_LIMIT {return Err("AF row capacity exhausted".into());}
        let mut retained=row.clone();
        retained.as_object_mut().ok_or("AF row is not an object")?
            .insert("af_sequence".into(),Value::from(self.rows.len()));
        self.retain_bytes(serde_json::to_vec(&retained).map_err(|e|e.to_string())?.len())?;
        self.rows.push(retained); Ok(())
    }
    pub fn observe(&mut self,session:&Session,maps:&[MemoryMap],point:fujimod_emulator::board::observation::InstructionPointEvent) {
        if let Err(failure)=self.observe_inner(session,maps,point) {self.fail(failure);}
    }
    fn observe_inner(&mut self,session:&Session,maps:&[MemoryMap],point:fujimod_emulator::board::observation::InstructionPointEvent)->Result<(),String> {
        let core=point.core.0;
        let pc=u32::try_from(point.entry.address).map_err(|_|"AF entry address exceeds32bits")?;
        let entry=EntryEvidence { opaque_id:OpaqueEntryId::try_from(format!("{:?}",point.entry.id))?,address:point.entry.address,bytes:point.entry.bytes,progress:point.entry.progress,pstate:point.entry.pstate,physical:point.entry.physical,tcm:point.entry.tcm.map(|bank|bank.map(|bank|EntryTcm{address:bank.address,bytes:bank.bytes})) };
        if !self.active(){return Ok(());}
        if self.contexts.len()==CONTEXT_LIMIT {return Err("AF context capacity exhausted".into());}
        let site=self.plan.sites.iter().find(|site|site.pc==pc).ok_or("AF unknown instruction site")?;
        let register=|n|u32::try_from(session.board.read_register(core,Register::Arm32(arm::Register::R(n)))?).map_err(|_|"AF register exceeds32bits".to_owned());
        let mut registers=[0;13]; for (index,value) in registers.iter_mut().enumerate(){*value=register(index as u8)?;}
        let cpsr=u32::try_from(session.board.read_register(core,Register::Arm32(arm::Register::Cpsr))?).map_err(|_|"AF CPSR exceeds32bits")?;
        let sp=register(13)?;
        let lr=register(14)?;
        let sampled_task=sample_task(session,maps,core);
        let execution=ExecutionContext { entry, cpsr, registers, sp, lr, sampled_task };
        let mut reader=ContextReader {session,maps,core:ReadCore::from_index(core).ok_or("AF invalid core")?,pc:Guest(pc),refusal:&mut self.read_refusal,execution:Some(execution)};
        if cpsr & ((1<<5)|(1<<9)|(1<<24))!=0 {return Err("AF context is not little-endian A32".into());}
        if reader.read(ReadOrigin::Instruction,ComputedReadAddress(i64::from(pc)),4)?.bytes!=site.word.to_le_bytes(){return Err("AF source instruction changed".into());}
        let mut reads=Vec::with_capacity(site.reads.len()+site.register_reads.len());
        for (index,&[address,bytes]) in site.reads.iter().enumerate() {
            let index=ReadIndex(u16::try_from(index).map_err(|_|"AF read index exceeds bound")?);
            reads.push(ReadRow::Captured(reader.read(ReadOrigin::Absolute{index},ComputedReadAddress(i64::from(address)),bytes)?));
        }
        for (index,spec) in site.register_reads.iter().enumerate() {
            let base=match spec.register {13=>register(13)?,14=>register(14)?,n=>registers[usize::from(n)]};
            let address=ComputedReadAddress(i64::from(base)+i64::from(spec.offset));
            let index=ReadIndex(u16::try_from(index).map_err(|_|"AF read index exceeds bound")?);
            let origin=ReadOrigin::Register{index,register:ReadRegister::try_from(spec.register)?,base:Guest(base),offset:spec.offset};
            if address.0==0 { reads.push(ReadRow::Null); continue; }
            reads.push(ReadRow::Captured(reader.read(origin,address,spec.bytes)?));
        }
        if pc==0x0102_f1b0 {self.pairs+=1;if self.pairs>128{return Err("AF pair capacity exhausted".into());}}
        let execution=reader.execution.take().ok_or("AF execution context is absent")?;
        let context=Context{sequence:self.contexts.len(),core,pc,word:site.word,kind:site.kind,entry:execution.entry,cpsr,registers,sp,lr,counts:counts(session)?,cursor:self.rows.len(),sampled_task:execution.sampled_task,reads};
        self.retain_bytes(serde_json::to_vec(&context).map_err(|e|e.to_string())?.len())?;
        self.contexts.push(context); Ok(())
    }
    pub fn fail(&mut self,failure:String){self.failure.get_or_insert(failure);}
    pub fn sync_retention(&mut self,status:fujimod_emulator::board::observation::PassiveRetentionStatus) {
        use fujimod_emulator::board::observation::{PassiveRetentionStatus as Status,PassiveStopReason as Reason};
        let observed=match status {
            Status::Disabled=>RetentionStatus::Disabled,
            Status::Unarmed=>RetentionStatus::Unarmed,
            Status::Armed=>RetentionStatus::Armed,
            Status::Stopped(reason)=>RetentionStatus::Stopped { reason:match reason {
                Reason::End=>RetentionStop::End,Reason::Refused=>RetentionStop::Refused,
                Reason::CapacityExceeded=>RetentionStop::CapacityExceeded,
                Reason::ProvenanceUnavailable=>RetentionStop::ProvenanceUnavailable,
            }},
        };
        if self.retention.is_some_and(|previous|matches!(previous,RetentionStatus::Stopped{..}) && previous!=observed) {
            self.fail("AF passive retention changed after stopping".into());
            return;
        }
        self.retention=Some(observed);
        match observed {
            RetentionStatus::Disabled=>self.fail("AF passive retention is disabled".into()),
            RetentionStatus::Stopped { reason:RetentionStop::Refused }=>self.fail("AF passive retention refused".into()),
            RetentionStatus::Stopped { reason:RetentionStop::CapacityExceeded }=>self.fail("AF passive retention capacity exhausted".into()),
            RetentionStatus::Stopped { reason:RetentionStop::ProvenanceUnavailable }=>self.fail("AF passive provenance unavailable".into()),
            _=>{},
        }
    }
    pub fn end(&mut self,session:&Session,maps:&[MemoryMap])->Result<(),String> {
        if maps != self.maps.as_slice() {return Err("AF mapping catalogue changed before end".into());}
        if self.initial.is_none() || self.final_boundary.is_some(){return Err("AF projection is not active".into());}
        if self.retention!=Some(RetentionStatus::Stopped{reason:RetentionStop::End}) {
            self.fail("AF passive retention did not end completely".into());
        }
        self.final_boundary=Some(boundary(session,maps)?);
        if let Some(failure)=&self.failure { return Err(failure.clone()); }
        if self.rows.is_empty() || self.contexts.is_empty(){self.fail("AF capture lacks ownership evidence".into());return Err("AF capture lacks ownership evidence".into());}
        let bytes=serde_json::to_vec(self).map_err(|error|error.to_string())?;
        if bytes.len()>OUTPUT_LIMIT {return Err("AF output capacity exhausted".into());}
        crate::af_ownership_receipt::validate(&bytes, &self.plan.identity, &self.plan, &self.maps)
    }
    pub fn receipt(&self)->Receipt<'_>{Receipt{schema:4,mode:ObservationMode::Passive,retention:self.retention,status:if self.failure.is_some(){"blocked"}else if self.final_boundary.is_some(){"complete"}else{"armed"},identity:&self.plan.identity,initial:&self.initial,final_boundary:&self.final_boundary,rows:&self.rows,contexts:&self.contexts,failure:&self.failure,read_refusal:&self.read_refusal,limits:Limits{rows:ROW_LIMIT,contexts:CONTEXT_LIMIT,pairs:128,output_bytes:OUTPUT_LIMIT},mappings:MappingCatalog(&self.maps)}}
}

impl ReadRefusal {
    pub fn validate(&self, plan: &Plan) -> Result<(), String> {
        let site=plan.sites.iter().find(|site|site.pc==self.pc.0).ok_or("AF refusal site is unbound")?;
        crate::af_ownership_receipt::validate_task_sample(&self.execution.sampled_task)?;
        if let TaskSample::Sampled{kernel,..}=&self.execution.sampled_task {
            if *kernel!=self.core { return Err("AF refusal sampled kernel differs from executing core".into()); }
        }
        crate::af_ownership_receipt::validate_entry(&self.execution.entry,self.pc.0,self.execution.cpsr,self.counts[self.core.index()])?;
        let (computed,bytes,purpose)=match self.origin {
            ReadOrigin::Instruction => (i64::from(site.pc),4,ReadPurpose::SharedPhysical),
            ReadOrigin::Absolute { index } => {
                let &[address,bytes]=site.reads.get(usize::from(index.0)).ok_or("AF refusal absolute index differs")?;
                (i64::from(address),bytes,ReadPurpose::SharedPhysical)
            },
            ReadOrigin::Register { index, register, base, offset } => {
                let spec=site.register_reads.get(usize::from(index.0)).ok_or("AF refusal register index differs")?;
                let observed=match register.0 { 0..=12=>self.execution.registers[usize::from(register.0)],13=>self.execution.sp,14=>self.execution.lr,_=>return Err("AF refusal register differs".into()) };
                if spec.register!=register.0 || spec.offset!=offset || base.0!=observed { return Err("AF refusal register binding differs".into()); }
                (i64::from(base.0)+i64::from(offset),spec.bytes,ReadPurpose::ExecutingCore)
            },
        };
        if self.computed_address.0!=computed || self.bytes.0!=bytes || self.purpose!=purpose
            || self.address!=u32::try_from(computed).ok().map(Guest) { return Err("AF refusal extent binding differs".into()); }
        match &self.live_mappings {
            RefusalMappings::Retained { mappings }=>crate::af_ownership_receipt::validate_mappings(mappings)?,
            RefusalMappings::CapacityExceeded { entries } if *entries>4096=>{},
            _=>return Err("AF refusal mapping capacity differs".into()),
        }
        match self.refusal.view {
            ReadView::Bounds if self.refusal.error==memory::ReadError::Bounds
                && (self.address.is_none() || self.address.is_some_and(|address|u64::from(address.0)+u64::from(bytes)>1<<32))=>{},
            ReadView::Bounds=>return Err("AF refusal bounds differ".into()),
            ReadView::TcmRegister { core, bank } if self.refusal.error==memory::ReadError::Register
                && self.refusal.observed_tcm[core.index()][bank.index()].is_none()=>{},
            ReadView::TcmRegister { .. }=>return Err("AF refusal register observation differs".into()),
            ReadView::ExecutingCore { core } if core!=self.core && purpose==ReadPurpose::ExecutingCore=>return Err("AF refusal selected core differs".into()),
            _=>{},
        }
        let view_matches=match self.refusal.error {
            memory::ReadError::Bounds=>matches!(self.refusal.view,ReadView::Bounds),
            memory::ReadError::Catalogue=>matches!(self.refusal.view,ReadView::Catalogue),
            memory::ReadError::Register=>matches!(self.refusal.view,ReadView::TcmRegister{..}),
            memory::ReadError::SharedValidation|memory::ReadError::PhysicalMismatch=>matches!(self.refusal.view,ReadView::SharedPhysical),
            _=>matches!(self.refusal.view,ReadView::ExecutingCore{..}),
        };
        if !view_matches { return Err("AF refusal error/view binding differs".into()); }
        if !matches!(self.refusal.error,memory::ReadError::Bounds|memory::ReadError::Catalogue|memory::ReadError::Register)
            && self.refusal.observed_tcm.iter().flatten().any(Option::is_none) { return Err("AF refusal lacks live TCM observations".into()); }
        Ok(())
    }
}
