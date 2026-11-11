//! Offline capture completeness validation, not queue causality or AF acceptance.

use serde::{Deserialize, Deserializer, de::{self, SeqAccess, Visitor}};
use serde_json::Value;
use std::{collections::BTreeMap, fmt, marker::PhantomData};
use fujimod_emulator::board::factory::MemoryMap;
use crate::memory_view::{MemoryAddress, MemoryCore, static_span};

pub(crate) fn validate_mappings(maps: &[MemoryMap]) -> Result<(), String> {
    require(!maps.is_empty() && maps.len() <= 4096, "AF mapping catalogue bound differs")?;
    for map in maps {
        require(map.core < 2 && map.size > 0 && map.permissions & !7 == 0
            && map.address.checked_add(map.size as u64).is_some_and(|end| end <= 1<<32)
            && map.physical.checked_add(map.size as u64).is_some_and(|end| end <= 1<<32),
            "AF mapping catalogue geometry differs")?;
    }
    Ok(())
}

const MAX_BYTES: usize = 134_217_728;
const MAX_ROWS: usize = 65_536;
const MAX_CONTEXTS: usize = 8_192;
const MAX_PAIRS: usize = 128;
const MAX_READ_BYTES: usize = 4_096;

#[derive(Clone, Copy, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(transparent)]
struct Address(u32);
#[derive(Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(transparent)]
struct Word(u32);
#[derive(Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(try_from = "u8")]
enum Core { Primary, Secondary }
impl TryFrom<u8> for Core {
    type Error = &'static str;
    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value { 0 => Ok(Self::Primary), 1 => Ok(Self::Secondary), _ => Err("invalid core") }
    }
}
impl Core {
    fn index(self) -> usize { match self { Self::Primary => 0, Self::Secondary => 1 } }
}

struct Bounded<T, const N: usize>(Vec<T>);
impl<'de, T: Deserialize<'de>, const N: usize> Deserialize<'de> for Bounded<T, N> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Items<T, const N: usize>(PhantomData<T>);
        impl<'de, T: Deserialize<'de>, const N: usize> Visitor<'de> for Items<T, N> {
            type Value = Bounded<T, N>;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { write!(f, "an array of at most {N} items") }
            fn visit_seq<A: SeqAccess<'de>>(self, mut sequence: A) -> Result<Self::Value, A::Error> {
                let mut values = Vec::with_capacity(sequence.size_hint().unwrap_or(0).min(N));
                while let Some(value) = sequence.next_element()? {
                    if values.len() == N { return Err(de::Error::custom("array limit exceeded")); }
                    values.push(value);
                }
                Ok(Bounded(values))
            }
        }
        deserializer.deserialize_seq(Items::<T, N>(PhantomData))
    }
}

pub(crate) fn bounded_mappings<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<MemoryMap>, D::Error> {
    Bounded::<MemoryMap, 4096>::deserialize(deserializer).map(|maps| maps.0)
}

#[derive(Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum Status { Armed, Complete, Blocked }
#[derive(Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum ByteOrder { Little, Big }
#[derive(Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
enum Source { CpuFetch, CpuRead, CpuWrite, HostRead, HostWrite, ModelBusRead, ModelBusWrite }
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Limits { rows: usize, contexts: usize, pairs: usize, output_bytes: usize }
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SnapshotWord { address: Address, value: Word }
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Boundary { counts: [u64; 2], words: Bounded<SnapshotWord, 82> }
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Read { address: Address, provenance: crate::af_ownership::ReadProvenance, bytes: Bounded<u8, MAX_READ_BYTES> }
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Context {
    sequence: usize,
    entry: crate::af_ownership::EntryEvidence,
    core: Core, pc: Address, word: Word, kind: crate::af_ownership::SiteKind, cpsr: u32, registers: [u32; 13], sp: Address,
    lr: Address, sampled_task: crate::af_ownership::TaskSample, counts: [u64; 2], cursor: usize, reads: Bounded<Read, MAX_PAIRS>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Fetch {
    source: Source, address: Address, physical: Address, size: u32,
    core: Core, pc: Address, instructions: u64, tick: u64,
    #[serde(deserialize_with = "present")]
    value: Option<u64>,
    #[serde(deserialize_with = "present")]
    before: Option<Bounded<u8, 8>>,
    byte_order: ByteOrder,
    core_instructions: u64,
}

fn mapped_fetch(fetch: &Fetch, maps: &[MemoryMap]) -> bool {
    let start = u64::from(fetch.pc.0);
    static_span(maps,MemoryCore(fetch.core.index()),MemoryAddress(start)..MemoryAddress(start+4))
        .is_ok_and(|span| span.physical.0 == u64::from(fetch.physical.0)
            && maps.iter().filter(|map| map.core == fetch.core.index()
                && map.address < start+4 && map.address+map.size as u64 > start)
                .all(|map| map.permissions & 4 != 0))
}

pub(crate) fn validate_attributed_cpu_write(row: &Value, maps: &[MemoryMap]) -> Result<(), &'static str> {
    let fetch = Fetch::deserialize(row.get("latest_fetch").ok_or("CPU write fetch absent")?)
        .map_err(|_| "CPU write fetch shape differs")?;
    let core = row.get("core").and_then(Value::as_u64).ok_or("CPU write core absent")?;
    let pc = row.get("pc").and_then(Value::as_u64).ok_or("CPU write PC absent")?;
    let instructions = row.get("instructions").and_then(Value::as_u64).ok_or("CPU write clock absent")?;
    let tick = row.get("tick").and_then(Value::as_u64).ok_or("CPU write tick absent")?;
    let core_instructions = row.get("core_instructions").and_then(Value::as_u64).ok_or("CPU write core clock absent")?;
    if fetch.source != Source::CpuFetch || fetch.core.index() as u64 != core
        || pc % 4 != 0 || u64::from(fetch.pc.0) != pc || fetch.address != fetch.pc
        || !mapped_fetch(&fetch,maps) || fetch.size != 4
        || fetch.byte_order != ByteOrder::Little || fetch.instructions != instructions
        || fetch.tick != tick || fetch.core_instructions != core_instructions
        || core_instructions > instructions || fetch.value.is_some() || fetch.before.is_some() {
        return Err("CPU write fetch identity differs");
    }
    Ok(())
}

pub(crate) fn validates_projection_metadata(row: &Value, maps: &[MemoryMap]) -> bool {
    let has_metadata = row.get("latest_fetch").is_some() || row.get("core_instructions").is_some();
    match Source::deserialize(&row["source"]) {
        Ok(Source::CpuWrite) if has_metadata => validate_attributed_cpu_write(row,maps).is_ok(),
        Ok(Source::CpuFetch) if has_metadata => Fetch::deserialize(row).is_ok_and(|fetch|
            fetch.source == Source::CpuFetch && fetch.address == fetch.pc && fetch.pc.0 % 4 == 0
                && mapped_fetch(&fetch,maps) && fetch.size == 4
                && fetch.byte_order == ByteOrder::Little && fetch.core_instructions <= fetch.instructions
                && fetch.value.is_none() && fetch.before.is_none()),
        Ok(_) => !has_metadata,
        Err(_) => false,
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Row {
    af_sequence: usize,
    source: Source, address: Address, physical: Address, size: u32,
    #[serde(deserialize_with = "present")]
    core: Option<Core>,
    #[serde(deserialize_with = "present")]
    pc: Option<Address>,
    #[serde(deserialize_with = "present")]
    instructions: Option<u64>,
    #[serde(deserialize_with = "present")]
    tick: Option<u64>,
    before: Option<Bounded<u8, 8>>, value: Option<u64>, byte_order: Option<ByteOrder>,
    core_instructions: Option<u64>, latest_fetch: Option<Fetch>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Receipt {
    row_count: usize, context_count: usize,
    schema: u32, status: Status, identity: Value, initial: Boundary,
    mode: crate::af_ownership::ObservationMode,
    #[serde(deserialize_with = "present")]
    retention: Option<crate::af_ownership::RetentionStatus>,
    #[serde(deserialize_with = "present")]
    r#final: Option<Boundary>, rows: Bounded<Row, MAX_ROWS>,
    contexts: Bounded<Context, MAX_CONTEXTS>,
    #[serde(deserialize_with = "present")]
    failure: Option<String>, limits: Limits,
    #[serde(deserialize_with = "present")]
    read_refusal: Option<crate::af_ownership::ReadRefusal>,
    mappings: Bounded<MemoryMap, 4096>,
}

pub(crate) fn present<'de, D: Deserializer<'de>, T: Deserialize<'de>>(deserializer: D) -> Result<Option<T>, D::Error> {
    Option::<T>::deserialize(deserializer)
}

fn require(condition: bool, message: &str) -> Result<(), String> {
    if condition { Ok(()) } else { Err(message.into()) }
}
fn total(counts: [u64; 2]) -> Result<u64, String> {
    counts[0].checked_add(counts[1]).ok_or_else(|| "instruction sum overflows".into())
}
fn between(counts: [u64; 2], begin: [u64; 2], end: [u64; 2]) -> bool {
    (0..2).all(|core| begin[core] <= counts[core] && counts[core] <= end[core])
}
fn snapshot_addresses() -> impl Iterator<Item = Address> {
    [0x016f_ec3c, 0x016f_ec4c].into_iter()
        .chain((0x016f_ed28..0x016f_ee48).step_by(4))
        .chain([0x0170_306c, 0x0170_3080, 0x0170_3084, 0x0170_30d0, 0x0170_3160,
            0x0170_319c, 0x0170_31a4, 0x0170_327c]).map(Address)
}
fn boundary(boundary: &Boundary) -> Result<(), String> {
    total(boundary.counts)?;
    require(boundary.words.0.len() == 82, "incomplete AF boundary snapshot")?;
    for (word, address) in boundary.words.0.iter().zip(snapshot_addresses()) {
        require(word.address == address, "AF boundary addresses differ or are reordered")?;
    }
    Ok(())
}

pub fn validate_plan(expected_plan: &crate::af_ownership::Plan) -> Result<(), String> {
    let identity = expected_plan.identity.as_object().ok_or("expected identity must be an object")?;
    require(identity.len() == 5 && ["sdk", "viewer", "bridge", "model", "package"].iter()
        .all(|key| identity.get(*key).and_then(Value::as_str).is_some_and(|value| !value.is_empty())),
        "expected immutable identity is incomplete")?;
    require(!expected_plan.sites.is_empty() && expected_plan.sites.len() <= MAX_PAIRS, "site binding count is outside limits")?;
    let mut previous_pc = None;
    for site in &expected_plan.sites {
        require(site.pc % 4 == 0 && site.pc & 0x4000_0000 == 0 && site.pc.checked_add(4).is_some()
            && previous_pc.is_none_or(|pc| pc < site.pc), "sites are not ordered aligned A32 addresses")?;
        require(site.reads.len() + site.register_reads.len() <= 16, "site read count exceeds bound")?;
        for &[address, bytes] in &site.reads {
            require(bytes > 0 && bytes <= MAX_READ_BYTES as u32 && address.checked_add(bytes).is_some(), "site read extent differs")?;
        }
        for read in &site.register_reads {
            require(read.register <= 14 && read.bytes > 0 && read.bytes <= MAX_READ_BYTES as u32, "register read extent differs")?;
        }
        previous_pc = Some(site.pc);
    }
    use crate::af_ownership::SiteKind;
    for (pc, kind) in [(0x0103_fd88, SiteKind::Consumer), (0x0103_e988, SiteKind::ModeHelper),
        (0x0103_1664, SiteKind::Primary), (0x0104_01f0, SiteKind::Current), (0x0104_0250, SiteKind::Set),
        (0x0102_dec8, SiteKind::Callback), (0x0104_0288, SiteKind::Clear), (0x0103_a254, SiteKind::Request),
        (0x0072_503c, SiteKind::QueuePost), (0x0072_5318, SiteKind::Dequeue), (0x0102_8b54, SiteKind::Select),
        (0x0102_8bc4, SiteKind::DispatchCallback), (0x0105_44e4, SiteKind::WorkPost), (0x0105_47b8, SiteKind::Callback),
        (0x0102_f1b0, SiteKind::ReservePrimary), (0x0102_f1c8, SiteKind::ReserveSecondary), (0x0102_f1d8, SiteKind::ReserveSecondary)] {
        require(expected_plan.sites.iter().any(|site| site.pc == pc && site.kind == kind),
            "mandatory AF observation site is absent or has a different kind")?;
    }
    Ok(())
}

/// Success does not establish observer noninterference, queue causality, or AF acceptance.
/// The expected catalogue comes from the independently bound SDK/model, not the receipt.
pub fn validate(bytes: &[u8], expected_identity: &Value, expected_plan: &crate::af_ownership::Plan,
    expected_mappings: &[MemoryMap]) -> Result<(), String> {
    require(bytes.len() <= MAX_BYTES, "receipt output bound exceeded")?;
    validate_plan(expected_plan)?;
    validate_mappings(expected_mappings)?;
    require(expected_plan.identity == *expected_identity, "plan identity differs")?;
    let sites: BTreeMap<Address, &crate::af_ownership::Site> = expected_plan.sites.iter()
        .map(|site| (Address(site.pc), site)).collect();
    let receipt: Receipt = serde_json::from_slice(bytes).map_err(|error| format!("invalid AF receipt: {error}"))?;
    if let Some(refusal)=&receipt.read_refusal { refusal.validate(expected_plan)?; }
    require(receipt.schema == 4 && receipt.status == Status::Complete && receipt.failure.is_none()
        && receipt.read_refusal.is_none(), "AF capture is not complete")?;
    require(receipt.mode == crate::af_ownership::ObservationMode::Passive
        && receipt.retention == Some(crate::af_ownership::RetentionStatus::Stopped { reason:crate::af_ownership::RetentionStop::End }),
        "AF passive retention is incomplete")?;
    require(receipt.identity == *expected_identity, "AF identity differs")?;
    validate_mappings(&receipt.mappings.0)?;
    require(receipt.mappings.0.as_slice() == expected_mappings, "AF mapping catalogue differs from expected SDK/model")?;
    require(!receipt.rows.0.is_empty() && !receipt.contexts.0.is_empty(), "AF capture lacks ownership evidence")?;
    require(receipt.row_count == receipt.rows.0.len() && receipt.context_count == receipt.contexts.0.len(),
        "AF declared event counts differ")?;
    let limits = receipt.limits;
    require(limits.rows == MAX_ROWS && limits.contexts == MAX_CONTEXTS && limits.pairs == MAX_PAIRS
        && limits.output_bytes == MAX_BYTES, "AF limits differ")?;
    let end = receipt.r#final.as_ref().ok_or("AF final boundary is absent")?;
    boundary(&receipt.initial)?;
    boundary(end)?;
    require(between(receipt.initial.counts, receipt.initial.counts, end.counts), "AF boundary counts decrease")?;
    let begin_total = total(receipt.initial.counts)?;
    let end_total = total(end.counts)?;
    let mut previous = (begin_total, 0);
    let mut memory: BTreeMap<Address, [u8; 4]> = receipt.initial.words.0.iter()
        .map(|word| (word.address, word.value.0.to_le_bytes())).collect();
    let mut core_clocks = receipt.initial.counts;
    for (sequence, row) in receipt.rows.0.iter().enumerate() {
        require(row.af_sequence == sequence, "AF row sequence is missing or reordered")?;
        let instructions = row.instructions.ok_or("AF row lacks merged instruction ordering evidence")?;
        let tick = row.tick.ok_or("AF row lacks merged tick ordering evidence")?;
        require(instructions >= begin_total && instructions <= end_total
            && instructions >= previous.0 && tick >= previous.1, "AF rows are outside boundaries or reordered")?;
        previous = (instructions, tick);
        require(row.byte_order == Some(ByteOrder::Little) && matches!(row.size, 1 | 2 | 4 | 8)
            && row.address.0.checked_add(row.size).is_some() && row.physical.0.checked_add(row.size).is_some(), "AF row extent or byte order differs")?;
        require(row.physical.0 == row.address.0 & !0x4000_0000, "AF row physical provenance differs")?;
        let extent = u64::from(row.physical.0)..u64::from(row.physical.0) + u64::from(row.size);
        require(snapshot_addresses().any(|address| extent.start < u64::from(address.0) + 4 && u64::from(address.0) < extent.end)
            || [(0x016f_e9bc, 0x016f_ec34), (0x0170_30d4, 0x0170_30ec), (0x0170_6a38, 0x0170_6e44)]
                .into_iter().any(|(start, end)| extent.start < end && start < extent.end), "AF row is outside observed regions")?;
        require(matches!(row.source, Source::CpuWrite | Source::HostWrite | Source::ModelBusWrite), "unexpected source in AF write projection")?;
        require(row.before.as_ref().is_some_and(|before| before.0.len() == row.size as usize)
            && row.value.is_some_and(|value| row.size == 8 || value < (1u64 << (row.size * 8))), "AF row data is incomplete")?;
        if matches!(row.source, Source::CpuRead | Source::CpuWrite) {
            let core = row.core.ok_or("CPU row core absent")?;
            let pc = row.pc.ok_or("CPU row PC absent")?;
            let fetch = row.latest_fetch.as_ref().ok_or("CPU row instruction provenance absent")?;
            require(pc.0 % 4 == 0 && fetch.source == Source::CpuFetch && fetch.core == core
                && fetch.pc == pc && fetch.address == pc && mapped_fetch(fetch,&receipt.mappings.0) && fetch.size == 4
                && fetch.byte_order == ByteOrder::Little && fetch.instructions == instructions
                && fetch.tick == tick && Some(fetch.core_instructions) == row.core_instructions
                && fetch.core_instructions >= receipt.initial.counts[core.index()]
                && fetch.core_instructions <= end.counts[core.index()]
                && fetch.value.is_none() && fetch.before.is_none(), "CPU row instruction provenance differs")?;
            let other = instructions.checked_sub(fetch.core_instructions).ok_or("CPU merged clock underflows")?;
            let mut counts = [other; 2];
            counts[core.index()] = fetch.core_instructions;
            require(between(counts, core_clocks, end.counts), "CPU instruction provenance is reordered or outside boundaries")?;
            core_clocks = counts;
        } else {
            require(row.latest_fetch.is_none() && row.core_instructions.is_none(), "non-CPU row claims instruction provenance")?;
        }
        let before = &row.before.as_ref().ok_or("AF row preimage absent")?.0;
        let value = row.value.ok_or("AF row value absent")?.to_le_bytes();
        for offset in 0..row.size as usize {
            let address = row.physical.0 + offset as u32;
            if let Some(word) = memory.get_mut(&Address(address & !3)) {
                let index = (address % 4) as usize;
                require(word[index] == before[offset], "AF row preimage differs from capture history")?;
                word[index] = value[offset];
            }
        }
    }
    for word in &end.words.0 {
        require(memory.get(&word.address) == Some(&word.value.0.to_le_bytes()), "AF final snapshot differs from captured writes")?;
    }
    let mut previous_counts = receipt.initial.counts;
    let mut previous_cursor = 0;
    let mut pairs = 0;
    for (sequence, context) in receipt.contexts.0.iter().enumerate() {
        require(context.sequence == sequence, "AF context sequence is missing or reordered")?;
        validate_entry(&context.entry,context.pc.0,context.cpsr,context.counts[context.core.index()])?;
        require(between(context.counts, previous_counts, end.counts), "AF contexts are outside boundaries or reordered")?;
        require(context.cursor >= previous_cursor && context.cursor <= receipt.rows.0.len(), "AF context cursor is reordered or outside rows")?;
        let site = sites.get(&context.pc).ok_or("AF context site is unbound")?;
        require(context.pc.0 % 4 == 0 && context.cpsr & ((1 << 5) | (1 << 9) | (1 << 24)) == 0
            && site.word == context.word.0 && site.kind == context.kind, "AF context is not its bound little-endian A32 instruction")?;
        if context.pc.0 == 0x0102_f1b0 {
            pairs += 1;
            require(pairs <= MAX_PAIRS, "AF pair capacity exceeded")?;
        }
        let at = total(context.counts)?;
        require(context.cursor == 0 || receipt.rows.0[context.cursor - 1].instructions.is_some_and(|clock| clock <= at), "AF context cursor precedes earlier rows")?;
        require(context.cursor == receipt.rows.0.len() || receipt.rows.0[context.cursor].instructions.is_some_and(|clock| clock >= at), "AF context cursor follows later rows")?;
        require(context.reads.0.len() == site.reads.len() + site.register_reads.len(), "AF read mask differs")?;
        validate_task_sample(&context.sampled_task)?;
        if let crate::af_ownership::TaskSample::Sampled{kernel,..}=&context.sampled_task {
            require(kernel.index()==context.core.index(),"AF sampled kernel differs from executing core")?;
        }
        for (index, read) in context.reads.0.iter().enumerate() {
            require(context.reads.0.first().is_none_or(|first|first.provenance.observed_tcm()==read.provenance.observed_tcm()),"AF stopped context TCM observations disagree")?;
            validate_entry_tcm(&context.entry,read.provenance.observed_tcm()[context.core.index()])?;
            crate::af_ownership::memory::validate_provenance(read.provenance,
                if index < site.reads.len() { crate::af_ownership::ReadPurpose::SharedPhysical } else { crate::af_ownership::ReadPurpose::ExecutingCore },
                crate::af_ownership::ReadCore::from_index(context.core.index()).unwrap(), crate::provenance::Guest(read.address.0), read.bytes.0.len() as u32, expected_mappings)?;
        }
        for (read, &[address, bytes]) in context.reads.0.iter().zip(&site.reads) {
            require(read.address.0 == address && read.bytes.0.len() == bytes as usize, "AF fixed read mask differs")?;
        }
        for (read, spec) in context.reads.0.iter().skip(site.reads.len()).zip(&site.register_reads) {
            let base = match spec.register { 0..=12 => context.registers[usize::from(spec.register)],
                13 => context.sp.0, 14 => context.lr.0, _ => return Err("invalid read register".into()) };
            let address = base.checked_add_signed(spec.offset).ok_or("AF register read address overflows")?;
            require(read.address.0 == address && read.bytes.0.len() == spec.bytes as usize
                && u64::from(address)+u64::from(spec.bytes)<=1<<32, "AF register read mask differs")?;
        }
        let _ = context.core;
        previous_counts = context.counts;
        previous_cursor = context.cursor;
    }
    Ok(())
}

pub(crate) fn validate_task_sample(sample: &crate::af_ownership::TaskSample) -> Result<(), String> {
    use crate::af_ownership::TaskSample;
    match sample {
        TaskSample::Refused { error, .. } => require(!error.is_empty() && error.len() <= 4096, "AF task refusal differs"),
        TaskSample::Sampled { kernel, id, node, creation, descriptor, .. } => {
            let (nodes, creations) = match kernel { crate::af_ownership::ReadCore::Primary => (0x4005_ff0cu32, 0x4005_beacu32), crate::af_ownership::ReadCore::Secondary => (0x400e_fecc, 0x400e_be6c) };
            require((1..=120).contains(id) && descriptor[8]==u32::from(*id) && descriptor[9]==1 && node.0 == nodes + (u32::from(*id) - 1) * 56
                && creation.0 == creations + (u32::from(*id) - 1) * 40, "AF sampled task identity differs")?;
            crate::modal_task::PhysicalExtent::new(crate::provenance::Guest(descriptor[5]), descriptor[4]).map_err(|_| "AF sampled stack extent differs".to_owned())?;
            Ok(())
        },
    }
}

pub(crate) fn validate_entry(entry: &crate::af_ownership::EntryEvidence, pc: u32, cpsr: u32, progress: u64) -> Result<(), String> {
    require(entry.address==u64::from(pc) && entry.bytes==4 && entry.progress==progress && entry.pstate==cpsr
        && entry.physical==Some(u64::from(pc)) && entry.tcm.iter().flatten().all(|bank|
            bank.bytes>0 && bank.address.checked_add(bank.bytes as u64).is_some_and(|end|
                end<=1<<32 && !(bank.address<u64::from(pc)+4 && u64::from(pc)<end))),
        "AF instruction entry evidence differs")
}

fn validate_entry_tcm(entry: &crate::af_ownership::EntryEvidence, registers: [crate::af_ownership::TcmValue; 2]) -> Result<(), String> {
    for (observed, retained) in registers.into_iter().zip(&entry.tcm) {
        let register=u32::try_from(observed.0).map_err(|_|"AF payload TCM register exceeds32bits")?;
        let window=crate::memory_view::tcm_window(crate::memory_view::TcmRegister(register))?;
        let retained=retained.as_ref().map(|bank| {
            bank.address.checked_add(bank.bytes as u64).map(|end|(bank.address,end))
                .ok_or("AF entry TCM window overflows")
        }).transpose()?;
        require(window==retained,"AF payload TCM windows differ from instruction entry")?;
    }
    Ok(())
}
