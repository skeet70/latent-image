use crate::heap_accesses::Digest;
use crate::heap_replay_state::{Bytes, Layout, Tape};
use crate::heap_state::{Address, AllocationId, Counter};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const IMAGE_BASE: u32 = 0x01f00000;
pub const IMAGE_LIMIT: u32 = 0x01f40000;
pub const STACK_START: u32 = 0x01f80000;
pub const STACK_END: u32 = 0x01f90000;
pub const STATS_ADDRESS: u32 = 0x01fa0000;
pub const RETURN_ADDRESS: u32 = 0x01ff0000;
pub const POOL_BASE: u32 = 0x00a71a00;
pub const REENT: u32 = 0x0089c978;
// malloc_av 1032 bytes, sbrk_base/trim_threshold 8 bytes and BSS bookkeeping 52 bytes.
pub const STOCK_EXTERNAL_METADATA: Bytes = Bytes(1092);
pub const CALL_BUDGET: u64 = 2_000_000;
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Backend { Stock, Rlsf, Talc, Buddy }
impl Backend {
    pub const ALL: [Self; 4] = [Self::Stock, Self::Rlsf, Self::Talc, Self::Buddy];
    pub fn wire(self) -> Option<u32> { match self { Self::Stock => None, Self::Rlsf => Some(1), Self::Talc => Some(2), Self::Buddy => Some(3) } }
    pub fn name(self) -> &'static str { match self { Self::Stock => "stock", Self::Rlsf => "rlsf", Self::Talc => "talc", Self::Buddy => "buddy" } }
}
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Capacity { Original, Recipe }
impl Capacity {
    pub const ALL: [Self; 2] = [Self::Original, Self::Recipe];
    pub fn bytes(self) -> Bytes { Bytes(match self { Self::Original => 1_572_832, Self::Recipe => 1_507_296 }) }
    pub fn name(self) -> &'static str { match self { Self::Original => "original", Self::Recipe => "recipe" } }
    pub fn bound_word(self) -> u32 { match self { Self::Original => 0xe34030bf, Self::Recipe => 0xe34030be } }
}
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct CallId(pub usize);
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct StepIndex(pub usize);
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct Instructions(pub u64);
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct Word(pub u32);
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct RegisterBits(pub u64);
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Span { pub start: Address, pub end: Address }
impl Span {
    pub fn contains(self, address: Address, bytes: Bytes) -> bool { address.0 >= self.start.0 && address.0.checked_add(bytes.0).is_some_and(|end| end <= self.end.0) }
    pub fn bytes(self) -> Bytes { Bytes(self.end.0 - self.start.0) }
}
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Symbol { Init, Allocate, Free, Footprint, Statistics, Zero, Copy, Status, Panic, TextStart, TextEnd, ReadOnlyEnd, ImageStart, ImageEnd, BssStart, BssEnd }
impl Symbol {
    pub fn parse(name: &str) -> Option<Self> {
        Some(match name { "heap_replay_init" => Self::Init, "heap_replay_alloc" => Self::Allocate, "heap_replay_free" => Self::Free,
            "heap_replay_footprint" => Self::Footprint, "heap_replay_stats" => Self::Statistics, "heap_replay_zero" => Self::Zero,
            "heap_replay_copy" => Self::Copy, "heap_replay_status" => Self::Status, "heap_replay_panic" => Self::Panic,
            "__text_start" => Self::TextStart, "__text_end" => Self::TextEnd, "__read_only_end" => Self::ReadOnlyEnd, "__image_start" => Self::ImageStart,
            "__image_end" => Self::ImageEnd, "__bss_start" => Self::BssStart, "__bss_end" => Self::BssEnd, _ => return None })
    }
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Bindings { pub run: Digest, pub update: Digest, pub boot_rom: Digest, pub seed: Digest, pub payload: Digest, pub symbols: Digest, pub events: Digest, pub summary: Digest, pub admission: Digest, pub replay_producer: Digest }
pub struct CanonicalImage { pub base: Address, pub bytes: Vec<u8> }
pub struct Inputs {
    pub run: Vec<u8>, pub update: Vec<u8>, pub boot_rom: Vec<u8>, pub seed: Vec<u8>, pub payload: Vec<u8>,
    pub symbols: BTreeMap<Symbol, Address>, pub images: Vec<CanonicalImage>, pub bindings: Bindings,
}
impl Inputs {
    pub fn symbol(&self, symbol: Symbol) -> Result<Address, Failure> { self.symbols.get(&symbol).copied().ok_or(Failure::Symbols) }
    pub fn canonical(&self, span: Span) -> Result<Vec<u8>, Failure> {
        let image = self.images.iter().find(|image| span.start.0 >= image.base.0 && u64::from(span.end.0) <= u64::from(image.base.0) + image.bytes.len() as u64).ok_or(Failure::CodeExtent)?;
        Ok(image.bytes[(span.start.0 - image.base.0) as usize..(span.end.0 - image.base.0) as usize].to_vec())
    }
    pub fn payload_text(&self) -> Result<Span, Failure> { Ok(Span { start: self.symbol(Symbol::TextStart)?, end: self.symbol(Symbol::TextEnd)? }) }
    pub fn payload_read_only(&self) -> Result<Span, Failure> { Ok(Span { start: self.symbol(Symbol::TextStart)?, end: self.symbol(Symbol::ReadOnlyEnd)? }) }
}
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CallRole { Initialize, Allocate, Free, Resize, Calloc, PayloadZero, PayloadCopy, Footprint, Statistics }
impl CallRole { pub fn timed(self) -> bool { matches!(self, Self::Allocate | Self::Free | Self::Resize | Self::Calloc | Self::PayloadZero | Self::PayloadCopy) } }
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum TargetStatus { Success, Invalid, OutOfMemory, UnsupportedMetric }
impl TargetStatus {
    pub fn parse(word: Word) -> Result<Self, Failure> { match word.0 { 0 => Ok(Self::Success), 1 => Ok(Self::Invalid), 2 => Ok(Self::OutOfMemory), 3 => Ok(Self::UnsupportedMetric), _ => Err(Failure::TargetStatus { observed: word }) } }
}
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CpuFrame { pub registers: [u32; 16], pub d8_d15: [RegisterBits; 8], pub cpsr: Word, pub counts: [Instructions; 2], pub root: Address, pub task_record: Address, pub kernel_context: Word, pub identity: Word, pub owner: Word, pub depth: Word, pub heap_break: Address }
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum HarnessEvent { Return { pc: Address }, Breakpoint { pc: Address }, Failure { message: String } }
#[derive(Clone, Debug, Default)]
pub struct EmptyEvents;
impl<'de> Deserialize<'de> for EmptyEvents {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Visitor;
        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = EmptyEvents;
            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { formatter.write_str("an empty event array") }
            fn visit_seq<A: serde::de::SeqAccess<'de>>(self, mut sequence: A) -> Result<Self::Value, A::Error> {
                if sequence.next_element::<serde::de::IgnoredAny>()?.is_some() { return Err(serde::de::Error::custom("unexpected fault or IRQ event")); }
                Ok(EmptyEvents)
            }
        }
        deserializer.deserialize_seq(Visitor)
    }
}
impl Serialize for EmptyEvents {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(std::iter::empty::<()>())
    }
}
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct FetchGuard { pub enabled: bool, pub checked_fetches: Counter, pub accepted_fetches: Counter, pub vector_fetches: Counter, pub range_fetches: Vec<Counter>, pub halted: bool, pub ranges: Option<Vec<[u64; 2]>>, pub vector_ranges: Vec<[u64; 2]>, pub vector_range_fetches: Vec<Counter> }
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Harness { pub event: Option<HarnessEvent>, pub faults: EmptyEvents, pub irq_events: EmptyEvents, pub counts: Vec<Counter>, pub trace_total: Counter, pub guard: FetchGuard }
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Call { pub identity: CallId, pub role: CallRole, pub function: Address, pub arguments: [Word; 4], pub before: CpuFrame, pub after: CpuFrame, pub status: Option<TargetStatus>, pub harness: Harness, pub count_ranges: Vec<Span> }
impl Call { pub fn elapsed(&self) -> Result<Instructions, Failure> { Ok(Instructions(self.after.counts[0].0.checked_sub(self.before.counts[0].0).ok_or(Failure::Clock)?)) } }
#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
pub struct ChunkHeader { pub address: Address, pub word: Word }
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Footprint { Stock { pointer: Address, header: Word }, Candidate { call: CallId } }
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Statistics { Stock { heap_break: Address, top: Address, chunks: Vec<ChunkHeader>, external_metadata_bytes: Option<Bytes> }, Candidate { call: CallId, output: Address, words: [Word; 5] } }
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Stats { pub allocated_block_bytes: Bytes, pub free_block_bytes: Bytes, pub largest_free_block_bytes: Bytes, pub external_metadata_bytes: Option<Bytes>, pub pool_managed_bytes: Bytes }
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum DataRole { BeforeFree, BeforeResize, PreservedResize, ZeroedCalloc, FinalLive }
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(transparent)]
pub struct PayloadBytes(pub Vec<u8>);
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Data { pub role: DataRole, pub identity: AllocationId, pub pointer: Address, pub bytes: PayloadBytes }
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct StepRecord { pub index: StepIndex, pub sequence: Counter, pub calls: Vec<CallId>, pub result: Address, pub footprint: Option<Footprint>, pub statistics: Vec<Statistics>, pub data: Vec<Data> }
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CodeBinding { pub span: Span, pub expected: Digest, pub before: Digest, pub after: Option<Digest> }
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum AccessKind { CpuWrite, HostWrite, ModelBusWrite }
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct WatchClass { pub range: [u64; 2], pub kinds: Vec<AccessKind> }
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Views { Observe }
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CodeWatch { pub enabled: bool, pub violation: Option<String>, pub records: EmptyEvents, pub classes: Vec<WatchClass>, pub ranges: Vec<[u64; 2]>, pub views: Views }
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Case { pub schema: u32, pub backend: Backend, pub capacity: Capacity, pub bindings: Bindings, pub complete: bool, pub error: Option<Failure>, pub code: Vec<CodeBinding>, pub code_watch: Option<CodeWatch>, pub initial_statistics: Option<Statistics>, pub calls: Vec<Call>, pub steps: Vec<StepRecord>, pub final_data: Vec<Data>, pub final_statistics: Option<Statistics> }
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Failure { InputIdentity, Symbols, CodeExtent, CodeWord { address: Address, expected: Word, actual: Word },
    Clock, Budget, Context, Aapcs, Return, TargetStatus { observed: Word }, BackendFailure, Data, Overlap, Divergence,
    Statistics, Footprint, CodeWrite, Incomplete, Tape, Evidence, Io { detail: String }, Execution { detail: String } }
impl std::fmt::Display for Failure { fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { write!(formatter, "{self:?}") } }
impl std::error::Error for Failure {}
impl From<std::io::Error> for Failure { fn from(error: std::io::Error) -> Self { Self::Io { detail: error.to_string() } } }
impl From<serde_json::Error> for Failure { fn from(_: serde_json::Error) -> Self { Self::Evidence } }
pub fn pattern(identity: AllocationId, offset: usize) -> u8 { (identity.0.wrapping_mul(73) ^ (offset as u64).wrapping_mul(151)) as u8 }
pub fn expected_layouts(tape: &Tape) -> &BTreeMap<AllocationId, Layout> { tape.final_live() }
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Distribution { pub samples: Counter, pub minimum: Instructions, pub median: Instructions, pub p95: Instructions, pub p99: Instructions, pub maximum: Instructions }
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct OperationDistribution { pub operation: crate::heap_state::Operation, pub entry_instructions: Distribution, pub allocator_instructions: Distribution, pub payload_instructions: Distribution }
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum PhysicalPeakScope { CompleteMallocFreeTrace, BoundaryAndCandidateTransient }
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CaseSummary { pub backend: Backend, pub capacity: Capacity, pub steps: Counter, pub peak_allocated_block_bytes: Bytes, pub physical_peak_scope: PhysicalPeakScope, pub peak_requested_bytes: Bytes, pub final_requested_bytes: Bytes, pub final_allocated_block_bytes: Bytes, pub final_free_block_bytes: Bytes, pub smallest_largest_free_block_bytes: Bytes, pub external_metadata_bytes: Option<Bytes>, pub payload_text_bytes: Bytes, pub operations: Vec<OperationDistribution> }

pub fn code_spans(inputs: &Inputs, tape: &Tape) -> Result<Vec<Span>, Failure> {
    let loader = inputs.images.iter().find(|image| image.base == Address(0x40000)).ok_or(Failure::CodeExtent)?;
    let loader_end = loader.base.0.checked_add(u32::try_from(loader.bytes.len()).map_err(|_| Failure::CodeExtent)?).ok_or(Failure::CodeExtent)?;
    let mut spans = vec![
        Span { start: loader.base, end: Address(loader_end) },
        Span { start: Address(0x72b110), end: Address(0x72b210) },
        Span { start: Address(0x723cb8), end: Address(0x723ce8) },
        Span { start: Address(0x724b68), end: Address(0x724cbc) },
        Span { start: Address(0x89cda4), end: Address(0x89d4f8) },
        Span { start: Address(0x89d908), end: Address(0x89e7c4) },
        inputs.payload_read_only()?,
    ];
    if tape.steps().iter().any(|step| matches!(&step.request, crate::heap_replay_state::Request::Calloc { .. })) {
        spans.push(Span { start: Address(0xbf1c1c), end: Address(0xbf1cd4) });
    }
    spans.sort_by_key(|span| span.start);
    let mut merged: Vec<Span> = Vec::with_capacity(spans.len());
    for span in spans {
        if span.start.0 >= span.end.0 || span.start.0 % 4 != 0 || span.end.0 % 4 != 0 { return Err(Failure::CodeExtent); }
        if let Some(previous) = merged.last_mut() {
            if span.start.0 <= previous.end.0 { previous.end.0 = previous.end.0.max(span.end.0); continue; }
        }
        merged.push(span);
    }
    Ok(merged)
}
pub fn expected_code(inputs: &Inputs, span: Span, capacity: Capacity) -> Result<Vec<u8>, Failure> {
    let read_only = inputs.payload_read_only()?;
    let mut bytes = if read_only.contains(span.start, span.bytes()) {
        let start = span.start.0.checked_sub(IMAGE_BASE).ok_or(Failure::CodeExtent)? as usize;
        let end = start.checked_add(span.bytes().0 as usize).ok_or(Failure::CodeExtent)?;
        inputs.payload.get(start..end).ok_or(Failure::CodeExtent)?.to_vec()
    } else { inputs.canonical(span)? };
    if span.contains(Address(0x72b130), Bytes(4)) {
        let offset = (0x72b130 - span.start.0) as usize;
        if bytes[offset..offset + 4] != 0xe34030bfu32.to_le_bytes() { return Err(Failure::CodeExtent); }
        bytes[offset..offset + 4].copy_from_slice(&capacity.bound_word().to_le_bytes());
    }
    Ok(bytes)
}
pub fn merged_watch_ranges(spans: &[Span]) -> Vec<[u64; 2]> {
    spans.iter().map(|span| [u64::from(span.start.0), u64::from(span.end.0)]).collect()
}
