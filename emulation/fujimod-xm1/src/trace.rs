//! Labelled instruction points on every core and the entries recorded at them.

use crate::provenance::Guest;
use crate::modal_permit::CoreId;
use crate::memory_view::{MemoryAddress, MemoryCore, TcmRegister, static_span, tcm_window};
use crate::modal_trace::{Config as ModalConfig, ModalTrace, Report as ModalReport};
use fujimod_emulator::board::factory::{Graph, MemoryMap};
use fujimod_emulator::board::observation::{InstructionPointEvent, InstructionPoints};
use fujimod_emulator::session::Session;
use fujimod_emulator_cpu::{Register, arm};
use serde::{Deserialize, Serialize, Serializer, ser::SerializeSeq};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

#[path = "trace_save_lifecycle.rs"]
mod save_lifecycle;
pub use save_lifecycle::{CaptureScope, LifecycleProgram};
use save_lifecycle::LifecycleContext;

#[path = "trace_task_context.rs"]
mod task_context;
pub use task_context::{TaskCapture, TaskContext};
#[path = "trace_calls.rs"]
pub mod calls;

/// One entry into an armed site, before its instruction executes.
#[derive(Clone, Debug, Serialize)]
pub struct Hit {
    pub site: String,
    pub address: String,
    pub core: usize,
    pub instructions: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task: Option<TaskContext>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub render_object_provenance_sequence: Option<u64>,
    pub registers: BTreeMap<&'static str, String>,
    pub sp: String,
    pub lr: String,
    pub cpsr: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code_word: Option<CodeWord>,
    /// The site's physical reads at entry, in hex, in `TraceSites::site_reads` order.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub memory: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub stack_memory: Vec<StackRead>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub observation: Option<calls::Observation>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize)]
#[serde(untagged, deny_unknown_fields)]
pub enum SiteRead {
    Absolute([u64; 2]),
    Stack { sp: i32, bytes: usize },
}

#[derive(Clone, Debug, Serialize)]
pub struct StackRead {
    kind: StackReadKind,
    sp: i32,
    address: String,
    bytes: String,
}

#[derive(Clone, Debug, Serialize)]
enum StackReadKind {
    #[serde(rename = "sp")]
    Sp,
}

#[derive(Clone, Copy, Default, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RegisterCapture {
    #[default]
    Arguments,
    General,
}

pub fn parse_site_reads(
    sites: &BTreeMap<String, String>,
    reads: &BTreeMap<String, Vec<SiteRead>>,
) -> Result<BTreeMap<u32, Vec<SiteRead>>, String> {
    reads.iter().map(|(site, ranges)| {
        if !sites.contains_key(site) {
            return Err(format!("trace site read names an unrequested site {site}"));
        }
        let address = site.strip_prefix("0x")
            .and_then(|digits| u32::from_str_radix(digits, 16).ok())
            .ok_or_else(|| format!("invalid trace site address {site}"))?;
        let ranges = ranges.iter().map(|read| match *read {
            SiteRead::Absolute([address, count]) => {
                let count = usize::try_from(count).map_err(|_| "trace site read count exceeds the host")?;
                if !(1..=4096).contains(&count)
                    || address.checked_add(count as u64).is_none_or(|end| end > 1 << 32) {
                    return Err("a trace site read requires an address and 1..4096 bytes".to_owned());
                }
                Ok(SiteRead::Absolute([address, count as u64]))
            }
            SiteRead::Stack { sp, bytes } => {
                if !(1..=4096).contains(&bytes) {
                    return Err("a stack-relative trace read requires 1..4096 bytes".to_owned());
                }
                Ok(SiteRead::Stack { sp, bytes })
            }
        }).collect::<Result<Vec<_>, String>>()?;
        Ok((address, ranges))
    }).collect()
}

#[derive(Clone, Debug, Serialize)]
pub struct CodeWord {
    pub guest: u32,
    pub physical: u32,
    pub expected: u32,
    pub tcm: [[u32; 2]; 2],
}

struct CodeBinding { start: Guest, bytes: Vec<u8>, maps: Vec<MemoryMap> }

impl CodeBinding {
    fn sample(&self, session: &Session, core: usize, pc: u32) -> Result<Option<CodeWord>, String> {
        sample_bound_word(session, core, pc, self.start.0, &self.bytes, &self.maps)
    }
}

pub(crate) fn sample_bound_word(
    session: &Session, core: usize, pc: u32, start: u32, bytes: &[u8], maps: &[MemoryMap],
) -> Result<Option<CodeWord>, String> {
    let Some(offset) = pc.checked_sub(start).map(|offset| offset as usize)
        .filter(|offset| *offset < bytes.len()) else { return Ok(None) };
    if offset % 4 != 0 || session.board.read_register(core, Register::Pc)? != u64::from(pc) {
        return Err("code word is not at its preexecution instruction point".into());
    }
    let end = u64::from(start).checked_add(bytes.len() as u64)
        .filter(|end| *end <= 1 << 32).ok_or("code image exceeds32bits")?;
    let expected = bytes.get(offset..offset + 4).ok_or("code word leaves image")?;
    let expected = u32::from_le_bytes(expected.try_into().map_err(|_| "code word leaves image")?);
    let mut word = [0; 4];
    let tcm = read_checked_bytes(session, core, pc, &mut word, maps,
        ObservationBinding::IdentityCode(u64::from(start)..end))?;
    let observed = u32::from_le_bytes(word);
    if observed != expected { return Err("preexecution code word differs from bound image".into()); }
    Ok(Some(CodeWord { guest: observed, physical: observed, expected, tcm }))
}

pub(crate) fn read_identity_bytes<const N: usize>(
    session: &Session, core: usize, address: u32, bytes: &mut [u8; N], maps: &[MemoryMap],
) -> Result<[[u32; 2]; 2], String> {
    let end = u64::from(address).checked_add(N as u64)
        .filter(|end| *end <= 1 << 32).ok_or("observed memory exceeds32bits")?;
    read_checked_bytes(session, core, address, bytes, maps,
        ObservationBinding::IdentityCode(u64::from(address)..end))
}

pub(crate) fn read_mapped_bytes<const N: usize>(
    session: &Session, core: usize, address: u32, bytes: &mut [u8; N], maps: &[MemoryMap],
) -> Result<(), String> {
    read_checked_bytes(session, core, address, bytes, maps, ObservationBinding::MappedData).map(|_| ())
}

enum ObservationBinding {
    IdentityCode(std::ops::Range<u64>),
    MappedData,
}

fn read_checked_bytes<const N: usize>(
    session: &Session, core: usize, address: u32, bytes: &mut [u8; N], maps: &[MemoryMap],
    binding: ObservationBinding,
) -> Result<[[u32; 2]; 2], String> {
    let end = u64::from(address).checked_add(N as u64)
        .filter(|end| *end <= 1 << 32).ok_or("observed memory exceeds32bits")?;
    let protected = match &binding {
        ObservationBinding::IdentityCode(range) => range.clone(),
        ObservationBinding::MappedData => u64::from(address)..end,
    };
    let mut tcm = [[0; 2]; 2];
    for (index, banks) in tcm.iter_mut().enumerate() {
        if matches!(binding, ObservationBinding::MappedData) && index != core { continue; }
        let cpu = session.board.cpu(index)?;
        for (bank, value) in banks.iter_mut().enumerate() {
            *value = u32::try_from(cpu.coprocessor(arm::Coprocessor {
                cp: 15, opc1: 0, crn: 9, crm: 1, opc2: bank as u32, secure: 0, wide: false,
            }, None)?).map_err(|_| "observed TCM register exceeds32bits")?;
            if tcm_window(TcmRegister(*value))?.is_some_and(|(low, high)| {
                low < protected.end && protected.start < high
            }) {
                return Err("observed memory overlaps a current TCM window".into());
            }
        }
    }
    let span = static_span(maps, MemoryCore(core),
        MemoryAddress(u64::from(address))..MemoryAddress(end))?;
    if matches!(binding, ObservationBinding::IdentityCode(_)) && span.physical.0 != u64::from(address) {
        return Err("observed code is not identity-mapped".into());
    }
    let mut physical = [0; N];
    session.board.read_memory(core, u64::from(address), bytes)?;
    session.board.read_physical(span.physical.0, &mut physical)?;
    if bytes.as_slice() != physical.as_slice() {
        return Err("guest and physical observation bytes differ".into());
    }
    Ok(tcm)
}

const CAPTURE_SITE_LIMIT: usize = 256;
const CAPTURE_DETAIL_LIMIT: usize = 2048;
const CAPTURE_DETAILS_PER_SITE: u64 = 64;
const CAPTURE_RAM_BYTES: usize = 512;
const CAPTURE_SPAN_LIMIT: usize = 24;
const CAPTURE_PAGE_LIMIT: usize = 64;
const CAPTURE_MEMORY_BYTES: usize = 2 * 1024 * 1024;
const CAPTURE_OVERHEAD_BYTES: usize = 128 * 1024;

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct CaptureCursor(u64);

impl CaptureCursor {
    pub const fn new(value: u64) -> Self { Self(value) }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CollectionOutcome {
    Continue,
    Stop,
    DrainRequired,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CaptureProtocolError {
    Inactive,
    WrongCursor,
    NoPublishedPage,
    WrongAcknowledgement,
}

impl std::fmt::Display for CaptureProtocolError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Inactive => "capture observation is not active",
            Self::WrongCursor => "capture cursor must equal the acknowledged cursor",
            Self::NoPublishedPage => "no capture page is published",
            Self::WrongAcknowledgement => "acknowledgement must equal the published next cursor",
        })
    }
}

#[derive(Serialize)]
pub struct CaptureAcknowledgement {
    acknowledged: CaptureCursor,
    pending: usize,
}

#[derive(Clone, Copy, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct InstructionCount(u64);

impl InstructionCount {
    pub fn from_board(session: &Session) -> Result<Self, String> {
        session.board.instruction_counts().into_iter()
            .try_fold(0u64, |sum, count| sum.checked_add(count))
            .map(Self).ok_or_else(|| "aggregate instruction count exceeds 64 bits".into())
    }

    pub(crate) fn value(self) -> u64 { self.0 }
}

#[derive(Clone, Copy, Serialize)]
pub struct CaptureObservationBounds {
    sites: usize,
    label_bytes: usize,
    details_per_site: u64,
    details_total: usize,
    ram_bytes_per_event: usize,
    page_details: usize,
    retained_memory_bytes: usize,
    charged_storage_bytes: usize,
}

pub const CAPTURE_OBSERVATION_BOUNDS: CaptureObservationBounds = CaptureObservationBounds {
    sites: CAPTURE_SITE_LIMIT,
    label_bytes: 64,
    details_per_site: CAPTURE_DETAILS_PER_SITE,
    details_total: CAPTURE_DETAIL_LIMIT,
    ram_bytes_per_event: CAPTURE_RAM_BYTES,
    page_details: CAPTURE_PAGE_LIMIT,
    retained_memory_bytes: CAPTURE_MEMORY_BYTES,
    charged_storage_bytes: std::mem::size_of::<CaptureEvent>() * CAPTURE_DETAIL_LIMIT
        + std::mem::size_of::<SiteObservation>() * CAPTURE_SITE_LIMIT
        + std::mem::size_of::<CaptureObservation>() + CAPTURE_OVERHEAD_BYTES + CAPTURE_MAP_STORAGE_BYTES,
};

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "kebab-case")]
enum RamStatus {
    Complete,
    OutsideRam,
    ReadFailed,
    ByteLimit,
}

#[derive(Clone, Copy)]
struct CapturedSpan {
    address: u32,
    offset: u16,
    length: u16,
    requested: u16,
    status: RamStatus,
}

impl CapturedSpan {
    const EMPTY: Self = Self { address: 0, offset: 0, length: 0, requested: 0, status: RamStatus::Complete };
}

struct CaptureRam {
    bytes: [u8; CAPTURE_RAM_BYTES],
    spans: [CapturedSpan; CAPTURE_SPAN_LIMIT],
    used: usize,
    count: usize,
    incomplete: bool,
}

impl Default for CaptureRam {
    fn default() -> Self {
        Self { bytes: [0; CAPTURE_RAM_BYTES], spans: [CapturedSpan::EMPTY; CAPTURE_SPAN_LIMIT], used: 0, count: 0, incomplete: false }
    }
}

struct HexBytes<'a>(&'a [u8]);

impl Serialize for HexBytes<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut text = [0; CAPTURE_RAM_BYTES * 2];
        const HEX: &[u8; 16] = b"0123456789abcdef";
        for (byte, pair) in self.0.iter().zip(text.chunks_exact_mut(2)) {
            pair[0] = HEX[usize::from(byte >> 4)];
            pair[1] = HEX[usize::from(byte & 15)];
        }
        let text = std::str::from_utf8(&text[..self.0.len() * 2]).map_err(serde::ser::Error::custom)?;
        serializer.serialize_str(text)
    }
}

#[derive(Serialize)]
struct RamSpanView<'a> {
    address: u32,
    requested: u16,
    status: RamStatus,
    bytes: HexBytes<'a>,
}

impl Serialize for CaptureRam {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut sequence = serializer.serialize_seq(Some(self.count))?;
        for span in &self.spans[..self.count] {
            let start = usize::from(span.offset);
            sequence.serialize_element(&RamSpanView {
                address: span.address,
                requested: span.requested,
                status: span.status,
                bytes: HexBytes(&self.bytes[start..start + usize::from(span.length)]),
            })?;
        }
        sequence.end()
    }
}

#[derive(Clone, Copy)]
enum SecondFilterSite {
    Descriptor,
    SelectorCall,
    SelectorRead,
    SelectorStore,
    OperandCall,
    Low10,
    Low13,
    Store,
}

impl SecondFilterSite {
    fn at(address: Guest) -> Option<Self> {
        match address.0 {
            0x00da426c => Some(Self::Descriptor),
            0x00da4300 => Some(Self::SelectorCall),
            0x00689114 => Some(Self::SelectorRead),
            0x00689140 => Some(Self::SelectorStore),
            0x00da4308 => Some(Self::OperandCall),
            0x00689444 => Some(Self::Low10),
            0x00689468 => Some(Self::Low13),
            0x00689490 => Some(Self::Store),
            _ => None,
        }
    }
}

const CAPTURE_MAP_LIMIT: usize = 256;

struct DecoderWindow {
    core: MemoryCore,
    start: MemoryAddress,
    end: MemoryAddress,
}

/// Graph-owned RAM views for event-time pointer observations.
pub struct CaptureMemory {
    maps: Box<[MemoryMap]>,
    decoder_windows: Box<[DecoderWindow]>,
}

impl CaptureMemory {
    /// The graph must be the one used to construct the observed session.
    pub fn from_graph(graph: Graph) -> Result<Self, String> {
        graph.validate()?;
        if graph.mappings.len() > CAPTURE_MAP_LIMIT {
            return Err("capture memory graph exceeds its mapping bound".into());
        }
        let mut decoder_windows = Vec::new();
        // Graph::validate_decoders requires every choice's (core, address, size) set to equal the initial choice's set.
        for decoder in graph.decoders {
            let choice = decoder.choices.into_iter().nth(decoder.initial).ok_or("capture memory decoder choice absent")?;
            for map in choice.processors {
                if decoder_windows.len() == CAPTURE_MAP_LIMIT {
                    return Err("capture memory graph exceeds its decoder-window bound".into());
                }
                decoder_windows.push(DecoderWindow {
                    core: MemoryCore(map.core),
                    start: MemoryAddress(map.address),
                    end: MemoryAddress(map.address.checked_add(map.size as u64).ok_or("capture decoder window overflows")?),
                });
            }
        }
        Ok(Self { maps: graph.mappings.into_boxed_slice(), decoder_windows: decoder_windows.into_boxed_slice() })
    }

    fn physical(&self, core: MemoryCore, address: Guest, length: usize, tcm: &TcmViews) -> Option<RamAddress> {
        let physical = RamAddress::parse(address.0, length)?;
        let start = u64::from(address.0);
        let end = start.checked_add(length as u64)?;
        if self.decoder_windows.iter().any(|window| window.core.0 == core.0 && start < window.end.0 && window.start.0 < end)
            || tcm.windows.iter().flatten().any(|&(low, high)| start < high && low < end) {
            return None;
        }
        let span = static_span(&self.maps, core, MemoryAddress(start)..MemoryAddress(end)).ok()?;
        (span.physical.0 == u64::from(physical.0)).then_some(physical)
    }
}

const CAPTURE_MAP_STORAGE_BYTES: usize = std::mem::size_of::<CaptureMemory>()
    + CAPTURE_MAP_LIMIT * (std::mem::size_of::<MemoryMap>() + std::mem::size_of::<DecoderWindow>());

struct TcmViews {
    windows: [Option<(u64, u64)>; 2],
    registers: [u32; 2],
}

impl TcmViews {
    fn read(session: &Session, core: MemoryCore) -> Result<Self, String> {
        let cpu = session.board.cpu(core.0)?;
        let mut windows = [None; 2];
        let mut registers = [0; 2];
        for (bank, window) in windows.iter_mut().enumerate() {
            let register = cpu.coprocessor(arm::Coprocessor {
                cp: 15, opc1: 0, crn: 9, crm: 1, opc2: bank as u32, secure: 0, wide: false,
            }, None)?;
            let register = u32::try_from(register).map_err(|_| "capture TCM register exceeds 32 bits")?;
            registers[bank] = register;
            *window = tcm_window(TcmRegister(register))?;
        }
        Ok(Self { windows, registers })
    }
}

struct DiagnosticViews<'a> {
    memory: Option<&'a CaptureMemory>,
    core: MemoryCore,
    tcm: Option<TcmViews>,
}

#[derive(Clone, Copy)]
struct RamAddress(u32);

impl RamAddress {
    fn parse(address: u32, length: usize) -> Option<Self> {
        let physical = match address {
            0..0x20000000 => address,
            0x40000000..0x60000000 => address - 0x40000000,
            _ => return None,
        };
        (u64::from(physical) + length as u64 <= 0x20000000).then_some(Self(physical))
    }
}

impl CaptureRam {
    fn read(&mut self, session: &Session, address: u32, length: usize) {
        if length == 0 { return; }
        if self.spans[..self.count].iter().any(|span| {
            span.address == address && usize::from(span.length) == length && matches!(span.status, RamStatus::Complete)
        }) {
            return;
        }
        if self.count == CAPTURE_SPAN_LIMIT {
            self.incomplete = true;
            return;
        }
        let mut span = CapturedSpan {
            address,
            offset: self.used as u16,
            length: 0,
            requested: length.min(u16::MAX as usize) as u16,
            status: RamStatus::Complete,
        };
        if length > CAPTURE_RAM_BYTES - self.used {
            span.status = RamStatus::ByteLimit;
        } else if let Some(physical) = RamAddress::parse(address, length) {
            span.address = physical.0;
            let destination = &mut self.bytes[self.used..self.used + length];
            match session.board.read_physical(u64::from(physical.0), destination) {
                Ok(()) => {
                    span.length = length as u16;
                    self.used += length;
                }
                Err(_) => span.status = RamStatus::ReadFailed,
            }
        } else {
            span.status = RamStatus::OutsideRam;
        }
        self.incomplete |= !matches!(span.status, RamStatus::Complete);
        self.spans[self.count] = span;
        self.count += 1;
    }

    fn word(&self, address: Guest) -> Option<u32> {
        let address = RamAddress::parse(address.0, 4)?.0;
        self.spans[..self.count].iter().find_map(|span| {
            if !matches!(span.status, RamStatus::Complete) { return None; }
            let relative = address.checked_sub(span.address)? as usize;
            if relative.checked_add(4)? > usize::from(span.length) { return None; }
            let start = usize::from(span.offset) + relative;
            Some(u32::from_le_bytes(self.bytes[start..start + 4].try_into().ok()?))
        })
    }

    fn common(&mut self, session: &Session) {
        for (address, length) in [
            (0x016f82fc, 24), (0x016f83ac, 8), (0x016f83c4, 4),
            (0x016f83e0, 12), (0x016f8418, 4),
            (0x0169d468, 46), (0x0169d4b4, 46), (0x0169d508, 1),
            (0x016f08c0, 12), (0x016f0aa4, 16),
            (0x0095c884, 4), (0x0095c958, 64), (0x0089f6e0, 1),
            (0x015ad53c, 4),
        ] {
            self.read(session, address, length);
        }
    }

    fn recorder(&mut self, session: &Session) {
        if self.word(Guest(0x015ad53c)).is_none() {
            self.read(session, 0x015ad53c, 4);
        }
        self.read(session, 0x015ad430, 32);
        let Some(index) = self.word(Guest(0x015ad53c)).filter(|&index| index <= 200) else {
            self.incomplete = true;
            return;
        };
        let base = 0x015968f4 + index * 0x1d0;
        for offset in [0, 36, 64] {
            self.read(session, base + offset, 28);
        }
    }

    // Queue API descriptors contain a packed header, byte length and payload pointer.
    fn message(&mut self, session: &Session, address: u32) {
        let Some(physical) = RamAddress::parse(address, 12).map(|address| address.0) else {
            self.read(session, address, 12);
            return;
        };
        self.read(session, physical, 12);
        if let (Some(length), Some(payload)) = (self.word(Guest(physical + 4)), self.word(Guest(physical + 8))) {
            self.read(session, payload, length as usize);
        }
        if self.word(Guest(physical)).is_some_and(|word| word & 0xffff == 0x137e) {
            self.recorder(session);
        }
    }

    fn queue30(&mut self, session: &Session) {
        self.message(session, 0x016fea2c);
        self.read(session, 0x016feb3c, 12);
        self.read(session, 0x016fe9d4, 4);
        self.read(session, 0x016fec4c, 4);
        self.read(session, 0x01703194, 4);
        self.read(session, 0x0170389c, 4);
    }

    fn unavailable(&mut self, address: Guest, length: usize) {
        self.incomplete = true;
        if self.count == CAPTURE_SPAN_LIMIT { return; }
        self.spans[self.count] = CapturedSpan {
            address: address.0, offset: self.used as u16, length: 0,
            requested: length.min(u16::MAX as usize) as u16, status: RamStatus::ReadFailed,
        };
        self.count += 1;
    }

    fn view(&mut self, session: &Session, views: &DiagnosticViews<'_>, address: Guest, length: usize) {
        if RamAddress::parse(address.0, length).is_none() {
            self.read(session, address.0, length);
            return;
        }
        let physical = views.memory.zip(views.tcm.as_ref())
            .and_then(|(memory, tcm)| memory.physical(views.core, address, length, tcm));
        if let Some(physical) = physical {
            self.read(session, physical.0, length);
        } else {
            self.unavailable(address, length);
        }
    }

    fn follow(&mut self, session: &Session, views: &DiagnosticViews<'_>, pointer: Option<Guest>, length: usize) {
        if let Some(pointer) = pointer {
            self.view(session, views, pointer, length);
        } else {
            self.incomplete = true;
        }
    }

    fn pointer(&self, base: Guest, offset: u32) -> Option<Guest> {
        self.word(Guest(base.0.checked_add(offset)?)).map(Guest)
    }

    fn second_filter(&mut self, session: &Session, views: &DiagnosticViews<'_>, site: SecondFilterSite, registers: &[u32; 4], sp: Guest) {
        match site {
            SecondFilterSite::Descriptor => {
                let descriptor = Guest(registers[0]);
                self.view(session, views, descriptor, 240);
                self.follow(session, views, self.pointer(descriptor, 0xc8), 16);
                let row = self.word(descriptor)
                    .and_then(|index| index.checked_mul(100))
                    .and_then(|offset| 0x00da52c0u32.checked_add(offset))
                    .map(Guest);
                self.follow(session, views, row, 100);
            }
            SecondFilterSite::SelectorCall => {
                self.view(session, views, Guest(registers[3]), 16);
                self.view(session, views, sp, 24);
            }
            SecondFilterSite::SelectorRead | SecondFilterSite::SelectorStore => {
                self.view(session, views, sp, 24);
            }
            SecondFilterSite::OperandCall => {
                self.view(session, views, Guest(registers[0]), 16);
                self.view(session, views, sp, 24);
            }
            SecondFilterSite::Low10 | SecondFilterSite::Low13 | SecondFilterSite::Store => {
                self.view(session, views, sp, 24);
                self.follow(session, views, self.pointer(sp, 4), 16);
            }
        }
    }

    fn event(session: &Session, memory: Option<&CaptureMemory>, core: MemoryCore, address: Guest, registers: &[u32; 4], sp: Guest) -> Self {
        let mut ram = Self::default();
        if let Some(site) = SecondFilterSite::at(address) {
            let views = DiagnosticViews { memory, core, tcm: TcmViews::read(session, core).ok() };
            ram.second_filter(session, &views, site, registers, sp);
        }
        match address.0 {
            0x0072503c | 0x00ef9ae0 | 0x01039e18 | 0x00fe95bc |
            0x00e7fca0 | 0x00e6f0e0 | 0x00e6f260 | 0x00e6c490 => ram.message(session, registers[1]),
            0x00ef9a94 | 0x00fe9580 => ram.read(session, registers[1], registers[2] as usize),
            0x00f01c40 => ram.message(session, 0x0169d374),
            0x00fe8ac4 | 0x0100297c => ram.message(session, 0x016f0aa8),
            0x01028b4c | 0x01028bc4 | 0x01028bdc | 0x01028c00 | 0x0102957c |
            0x0102fd20 | 0x010300a4 | 0x01030284 | 0x01030310 => ram.queue30(session),
            0x0064e918 => ram.read(session, registers[0], 32),
            0x007411c4 => ram.read(session, registers[3], 4),
            0x00e8659c => {
                if registers[0] != 0 {
                    ram.message(session, registers[0]);
                }
                ram.recorder(session);
            }
            0x00e6c444 | 0x00e6efd4 | 0x00e6f100 | 0x00e7f9ec |
            0x00e869b8 | 0x00e869cc | 0x00e86a1c |
            0x00e86a30 | 0x00e755d0 | 0x00eb0ea0 => ram.recorder(session),
            _ => {}
        }
        ram.common(session);
        ram
    }
}

#[derive(Default, Serialize)]
struct SiteObservation {
    address: u32,
    total: u64,
    dropped: u64,
    captured: u64,
    pending: u64,
    first_instructions: Option<InstructionCount>,
    last_instructions: Option<InstructionCount>,
    first_dropped_instructions: Option<InstructionCount>,
    last_dropped_instructions: Option<InstructionCount>,
}

struct CaptureEvent {
    sequence: CaptureCursor,
    site_index: usize,
    core: usize,
    instructions: InstructionCount,
    registers: [u32; 4],
    sp: u32,
    lr: u32,
    cpsr: u32,
    ram: CaptureRam,
    lifecycle: Option<LifecycleContext>,
}

struct CaptureObservation {
    sites: Vec<SiteObservation>,
    events: VecDeque<CaptureEvent>,
    acknowledged: CaptureCursor,
    next_sequence: CaptureCursor,
    published: Option<CaptureCursor>,
    through_instructions: InstructionCount,
    boundary_complete: bool,
    drained_boundary_observed: bool,
    ram_incomplete: bool,
    scope: CaptureScope,
}

const _: () = assert!(CAPTURE_OBSERVATION_BOUNDS.charged_storage_bytes <= CAPTURE_MEMORY_BYTES);

#[derive(Serialize)]
struct CaptureEventView<'a> {
    sequence: CaptureCursor,
    site: &'a str,
    address: u32,
    core: usize,
    instructions: InstructionCount,
    registers: &'a [u32; 4],
    sp: u32,
    lr: u32,
    cpsr: u32,
    ram: &'a CaptureRam,
    ram_incomplete: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    lifecycle: Option<&'a LifecycleContext>,
}

struct CaptureEvents<'a> {
    observation: &'a CaptureObservation,
    labels: &'a BTreeMap<u32, String>,
    count: usize,
}

impl Serialize for CaptureEvents<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut sequence = serializer.serialize_seq(Some(self.count))?;
        for event in self.observation.events.iter().take(self.count) {
            let address = self.observation.sites[event.site_index].address;
            sequence.serialize_element(&CaptureEventView {
                sequence: event.sequence, site: &self.labels[&address], address, core: event.core,
                instructions: event.instructions, registers: &event.registers,
                sp: event.sp, lr: event.lr, cpsr: event.cpsr,
                ram: &event.ram, ram_incomplete: event.ram.incomplete,
                lifecycle: event.lifecycle.as_ref(),
            })?;
        }
        sequence.end()
    }
}

#[derive(Serialize)]
struct SiteObservationView<'a> {
    site: &'a str,
    #[serde(flatten)]
    counts: &'a SiteObservation,
}

struct CaptureSites<'a> {
    observation: &'a CaptureObservation,
    labels: &'a BTreeMap<u32, String>,
}

impl Serialize for CaptureSites<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut sequence = serializer.serialize_seq(Some(self.observation.sites.len()))?;
        for counts in &self.observation.sites {
            sequence.serialize_element(&SiteObservationView { site: &self.labels[&counts.address], counts })?;
        }
        sequence.end()
    }
}

#[derive(Serialize)]
pub struct CaptureObservationPage<'a> {
    from: CaptureCursor,
    next: CaptureCursor,
    acknowledged: CaptureCursor,
    pending: usize,
    captured: u64,
    through_instructions: InstructionCount,
    boundary_complete: bool,
    total: u64,
    dropped: u64,
    incomplete: bool,
    first_instructions: Option<InstructionCount>,
    last_instructions: Option<InstructionCount>,
    bounds: CaptureObservationBounds,
    sites: CaptureSites<'a>,
    hits: CaptureEvents<'a>,
}

#[derive(Serialize)]
pub struct CaptureSnapshot {
    instructions: InstructionCount,
    ram: CaptureRam,
    ram_incomplete: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    lifecycle: Option<LifecycleContext>,
    #[serde(skip_serializing_if = "Option::is_none")]
    core: Option<usize>,
}

#[derive(Clone, Copy, Default)]
enum CoreSelection {
    #[default]
    All,
    Primary,
}

impl CoreSelection {
    fn includes(self, core: usize) -> bool {
        matches!(self, Self::All) || core == 0
    }
}

#[derive(Clone, Copy, Default, Eq, PartialEq)]
enum PointSetMode {
    /// Per-address hooks, reinstalled on every site change.
    #[default]
    Fixed,
    /// One all-PC set per core whose membership follows site changes.
    Persistent(Arming),
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum Arming {
    Armed,
    Disarmed,
}

/// What `TraceSites::disarm` does to persistent point sets.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum DisarmedPoints {
    /// Removes the sets; `rearm` installs new ones.
    #[default]
    Release,
    /// Empties their membership and keeps them installed ahead of later watches.
    Retain,
}

enum PointScope {
    User,
    UserAndModal,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct EnteredInstructions(pub u64);

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(try_from = "TraceStackScopeFields")]
pub struct TraceStackScopeConfig {
    stack: [Guest; 2],
    owner: Guest,
    current: [Guest; 2],
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TraceStackScopeFields {
    stack: [Guest; 2],
    owner: Guest,
    current: [Guest; 2],
}

impl TryFrom<TraceStackScopeFields> for TraceStackScopeConfig {
    type Error = String;

    fn try_from(value: TraceStackScopeFields) -> Result<Self, Self::Error> {
        Self::new(value.stack, value.owner, value.current)
    }
}

impl TraceStackScopeConfig {
    pub fn new(stack: [Guest; 2], owner: Guest, current: [Guest; 2]) -> Result<Self, String> {
        if stack.into_iter().chain([owner]).chain(current).any(|value| value.0 % 4 != 0 || value.0 >= 0x20000000)
            || stack[0] >= stack[1] || owner.0 == 0 || current[0] == current[1] {
            return Err("stack scope requires aligned physical DRAM bounds, owner and distinct current-task cells".into());
        }
        Ok(Self { stack, owner, current })
    }

    pub const fn stack(&self) -> [Guest; 2] { self.stack }
    pub const fn owner(&self) -> Guest { self.owner }
    pub const fn current(&self) -> [Guest; 2] { self.current }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct ScopedHitCount(pub u64);

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TraceStackSkip {
    pub core: usize,
    pub site: Guest,
    pub instructions: EnteredInstructions,
    pub sp: Guest,
    pub current: Guest,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum TraceStackViolation {
    OwnerOutsideStack { hit: TraceStackSkip },
    SkipLimit { hit: TraceStackSkip },
    CounterOverflow { site: Guest, core: usize },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TraceStackSiteSummary {
    pub site: Guest,
    pub entered: EnteredInstructions,
    pub recorded: [ScopedHitCount; 2],
    pub outside: [ScopedHitCount; 2],
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TraceStackScopeSummary {
    pub config: TraceStackScopeConfig,
    pub sites: Vec<TraceStackSiteSummary>,
    pub violation: Option<TraceStackViolation>,
}

const STACK_SKIP_LIMIT: usize = 65536;

struct StackScope {
    summary: TraceStackScopeSummary,
    skips: Vec<TraceStackSkip>,
}

enum StackPoint {
    Unscoped,
    Inside { index: usize, next: ScopedHitCount },
    Outside,
}

fn stack_physical(value: Guest) -> Guest {
    match value.0 {
        0x40000000..0x60000000 => Guest(value.0 - 0x40000000),
        _ => value,
    }
}

impl StackScope {
    fn fail(&mut self, violation: TraceStackViolation) -> String {
        self.summary.violation = Some(violation);
        "trace stack scope refused".into()
    }

    fn observe(&mut self, session: &Session, core: usize, site: Guest, instructions: EnteredInstructions) -> Result<StackPoint, String> {
        let Ok(index) = self.summary.sites.binary_search_by_key(&site, |entry| entry.site) else {
            return Ok(StackPoint::Unscoped);
        };
        let cell = self.summary.config.current.get(core).ok_or("stack scope core exceeds two cores")?;
        let sp = Guest(register(session, core, arm::Register::R(13))?);
        let mut bytes = [0; 4];
        session.board.read_physical(u64::from(cell.0), &mut bytes)?;
        let current = stack_physical(Guest(u32::from_le_bytes(bytes)));
        let physical_sp = stack_physical(sp);
        let [start, end] = self.summary.config.stack;
        if start <= physical_sp && physical_sp <= end {
            let Some(next) = self.summary.sites[index].recorded[core].0.checked_add(1) else {
                return Err(self.fail(TraceStackViolation::CounterOverflow { site, core }));
            };
            return Ok(StackPoint::Inside { index, next: ScopedHitCount(next) });
        }
        let hit = TraceStackSkip { core, site, instructions, sp, current };
        if self.skips.len() == STACK_SKIP_LIMIT {
            return Err(self.fail(TraceStackViolation::SkipLimit { hit }));
        }
        let Some(next) = self.summary.sites[index].outside[core].0.checked_add(1) else {
            return Err(self.fail(TraceStackViolation::CounterOverflow { site, core }));
        };
        self.skips.push(hit);
        self.summary.sites[index].outside[core] = ScopedHitCount(next);
        if current == self.summary.config.owner {
            return Err(self.fail(TraceStackViolation::OwnerOutsideStack { hit }));
        }
        Ok(StackPoint::Outside)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ModalPointState { Unobserved, Observed }
#[derive(Clone, Copy)]
struct PendingPoint {
    pc: Guest,
    point: Option<InstructionPointEvent>,
}

struct ResumeMarker {
    pc: Guest,
    entered: EnteredInstructions,
    point: Option<InstructionPointEvent>,
    modal: ModalPointState,
}

#[derive(Default)]
pub struct TraceSites {
    pub sites: BTreeMap<u32, String>,
    /// Labels whose entry ends the current run.
    pub stop_labels: BTreeSet<String>,
    pub hits: Vec<Hit>,
    /// Physical ranges each site's hits read at entry, by site address.
    pub site_reads: BTreeMap<u32, Vec<SiteRead>>,
    pub calls: calls::Calls,
    hit_limit: Option<std::num::NonZeroUsize>,
    /// Hits record r0-r12 rather than r0-r3.
    full_registers: bool,
    code_binding: Option<CodeBinding>,
    /// Persistent sets are in core order; a set whose removal failed stays here.
    points: Vec<(usize, Box<InstructionPoints>)>,
    cores: CoreSelection,
    point_set_mode: PointSetMode,
    disarmed_points: DisarmedPoints,
    resumed: BTreeMap<usize, ResumeMarker>,
    capture_observation: Option<CaptureObservation>,
    pending_points: Vec<Option<PendingPoint>>,
    capture_memory: Option<CaptureMemory>,
    modal_observation: Option<ModalTrace>,
    stack_scope: Option<StackScope>,
    presentation: Option<crate::presentation::Tracker>,
    task_context: Option<task_context::Observer>,
    pub(crate) render_object_provenance_sequence: Option<u64>,
}

/// The engine's address bound for one instruction point set.
const FIXED_POINT_SET_LIMIT: usize = 4096;

fn register(session: &Session, core: usize, register: arm::Register) -> Result<u32, String> {
    let value = session.board.read_register(core, Register::Arm32(register))?;
    u32::try_from(value).map_err(|_| "ARM register exceeds 32 bits".to_owned())
}

impl TraceSites {
    pub(crate) fn begin_presentation_tracking(&mut self, session: &Session) -> Result<(), String> {
        if self.presentation.is_some() { return Err("presentation tracking already active".into()); }
        self.presentation = Some(crate::presentation::Tracker::default());
        self.rearm_points(session, PointScope::UserAndModal)
    }
    pub(crate) fn presentation_tracking_active(&self) -> bool { self.presentation.is_some() }

    pub(crate) fn invalidate_presentation(&mut self) {
        if let Some(tracker) = &mut self.presentation { tracker.invalidate(); }
    }

    pub(crate) fn select_presentation_target(&mut self) -> Result<Option<crate::presentation::FrameToken>, String> {
        self.presentation.as_mut().ok_or("presentation tracking is not active")?
            .select_target().map_err(str::to_owned)
    }

    pub(crate) fn presentation_acknowledgement(&self, token: crate::presentation::FrameToken) -> Result<Option<crate::presentation::Acknowledgement>, String> {
        self.presentation.as_ref().ok_or("presentation tracking is not active")?
            .acknowledgement(token).map_err(str::to_owned)
    }

    pub fn begin_stack_scope(&mut self, config: TraceStackScopeConfig) -> Result<(), String> {
        if self.stack_scope.is_some() || self.modal_observation_active() || self.capture_observation_active() {
            return Err("stack scope requires no existing stack, modal or capture observation".into());
        }
        self.stack_scope = Some(StackScope {
            summary: TraceStackScopeSummary { config, sites: Vec::new(), violation: None },
            skips: Vec::with_capacity(STACK_SKIP_LIMIT),
        });
        Ok(())
    }

    pub fn scope_stack_sites(&mut self, session: &Session, sites: &[Guest]) -> Result<(), String> {
        if self.modal_observation_active() || self.capture_observation_active() {
            return Err("stack scope conflicts with modal or capture observation".into());
        }
        if session.board.instruction_counts().len() != 2 {
            return Err("stack scope requires two executing core slots".into());
        }
        if sites.iter().any(|site| site.0 % 4 != 0 || RamAddress::parse(site.0, 4).is_none()) {
            return Err("stack scope sites must be aligned DRAM instruction addresses".into());
        }
        let entered = EnteredInstructions(InstructionCount::from_board(session)?.0);
        let scope = self.stack_scope.as_mut().ok_or("stack scope has not begun")?;
        if scope.summary.violation.is_some() {
            return Err("trace stack scope has already refused".into());
        }
        for &site in sites {
            if let Err(index) = scope.summary.sites.binary_search_by_key(&site, |entry| entry.site) {
                scope.summary.sites.insert(index, TraceStackSiteSummary {
                    site, entered, recorded: [ScopedHitCount(0); 2], outside: [ScopedHitCount(0); 2],
                });
            }
        }
        Ok(())
    }

    pub fn drain_stack_scope_skips(&mut self) -> impl Iterator<Item = TraceStackSkip> + '_ {
        self.stack_scope.iter_mut().flat_map(|scope| scope.skips.drain(..))
    }

    pub fn stack_scope_summary(&self) -> Option<&TraceStackScopeSummary> {
        self.stack_scope.as_ref().map(|scope| &scope.summary)
    }

    pub fn bound_full_registers(&mut self, limit: std::num::NonZeroUsize) {
        self.hit_limit = Some(limit);
        self.full_registers = true;
    }

    pub fn bound_hits(&mut self, limit: std::num::NonZeroUsize) {
        self.hit_limit = Some(limit);
    }

    pub fn bound_registers(&mut self, limit: std::num::NonZeroUsize, registers: RegisterCapture) {
        self.bound_hits(limit);
        self.full_registers = matches!(registers, RegisterCapture::General);
    }

    pub fn bind_code_words(&mut self, limit: std::num::NonZeroUsize, start: Guest, bytes: Vec<u8>, maps: Vec<MemoryMap>) -> Result<(), String> {
        if start.0 % 4 != 0 || bytes.is_empty() || bytes.len() % 4 != 0 || u64::from(start.0) + bytes.len() as u64 > 1 << 32 {
            return Err("code binding is not a bounded word-aligned image".into());
        }
        self.bound_full_registers(limit);
        self.code_binding = Some(CodeBinding { start, bytes, maps });
        Ok(())
    }

    pub fn with_memory(session: &Session, memory: CaptureMemory) -> Self {
        let mut traces = Self::default();
        traces.prepare_memory(session, memory);
        traces
    }

    pub(crate) fn needs_capture_memory(&self, scope: CaptureScope) -> bool {
        self.capture_memory.is_none() && self.capture_observation.is_none()
            && (scope.lifecycle() || self.sites.keys().any(|&address| SecondFilterSite::at(Guest(address)).is_some()))
    }

    pub(crate) fn prepare_memory(&mut self, session: &Session, memory: CaptureMemory) {
        self.capture_memory = (session.board.mappings() == memory.maps.as_ref()).then_some(memory);
    }

    pub fn modal_observation_active(&self) -> bool {
        self.modal_observation.as_ref().is_some_and(ModalTrace::active)
    }

    pub fn modal_observation_report(&self) -> ModalReport<'_> {
        self.modal_observation.as_ref().map(ModalTrace::report).unwrap_or_else(ModalReport::inactive)
    }

    /// The installed-lifetime guard's memory watch, which the modal observation owns exclusively.
    pub fn modal_watch_state(&self) -> Option<impl Serialize + '_> {
        self.modal_observation.as_ref().and_then(ModalTrace::watch_evidence)
    }

    pub fn begin_modal_observation(&mut self, session: &Session, config: ModalConfig) -> Result<(), String> {
        if self.stack_scope.is_some() {
            return Err("modal observation conflicts with stack scope".into());
        }
        if self.modal_observation_active() || self.capture_observation_active() {
            return Err("modal observation requires no active modal or capture observation".into());
        }
        let counts = session.board.instruction_counts();
        for (&core, marker) in &self.resumed {
            if config.refuses_prior_stop(marker.pc) && counts.get(core).copied() == Some(marker.entered.0)
                && session.board.read_register(core, Register::Pc)? == u64::from(marker.pc.0) {
                return Err("installed-lifetime observation requires a fresh protected instruction entry".into());
            }
        }
        let mut monitor = ModalTrace::new(config, session)?;
        let instructions = InstructionCount::from_board(session)?;
        let observed = self.resumed.iter_mut().try_for_each(|(&core, marker)| {
            if !monitor.guarded() && monitor.contains(marker.pc)
                && counts.get(core).copied() == Some(marker.entered.0)
                && session.board.read_register(core, Register::Pc)? == u64::from(marker.pc.0) {
                monitor.observe(session, core, marker.pc, instructions, marker.point)?;
                marker.modal = ModalPointState::Observed;
            }
            Ok::<(), String>(())
        });
        if let Err(failure) = &observed { monitor.fail(failure.clone()); }
        self.modal_observation = Some(monitor);
        if let Err(failure) = self.rearm_points(session, PointScope::UserAndModal) {
            if let Some(modal) = &mut self.modal_observation { modal.fail(failure.clone()); }
            return Err(failure);
        }
        observed
    }

    pub fn end_modal_observation(&mut self, session: &Session) -> Result<(), String> {
        self.modal_observation.as_mut().ok_or("modal observation has not begun")?.check_finish(session)?;
        let teardown = self.rearm_points(session, PointScope::User);
        let modal = self.modal_observation.as_mut().ok_or("modal observation disappeared")?;
        match teardown.and_then(|()| modal.finish_guard(session)) {
            Ok(()) => {
                modal.disable();
                Ok(())
            }
            Err(failure) => {
                modal.fail(failure.clone());
                Err(failure)
            }
        }
    }

    pub fn capture_observation_active(&self) -> bool {
        self.capture_observation.is_some()
    }

    pub fn begin_capture_observation(&mut self, scope: CaptureScope) -> Result<(), String> {
        if self.task_context.is_some() {
            return Err("capture arena does not record logical task context".into());
        }
        if self.stack_scope.is_some() {
            return Err("capture observation conflicts with stack scope".into());
        }
        if self.modal_observation_active() {
            return Err("capture observation refuses an active modal observation".into());
        }
        if self.capture_observation.is_some() {
            return Err("capture observation is already active".into());
        }
        scope.validate(&self.sites)?;
        if self.sites.len() > CAPTURE_SITE_LIMIT || self.points.len() > CAPTURE_SITE_LIMIT
            || self.sites.values().any(|label| label.len() > 64) {
            return Err("capture observation exceeds the site, core or label bound".into());
        }
        for label in self.sites.values_mut() {
            if label.capacity() > 64 {
                label.shrink_to_fit();
                if label.capacity() > 64 {
                    return Err("capture observation label capacity exceeds its retained bound".into());
                }
            }
        }
        self.points.shrink_to_fit();
        self.pending_points.shrink_to_fit();
        let mut sites = Vec::with_capacity(CAPTURE_SITE_LIMIT);
        sites.extend(self.sites.keys().map(|&address| SiteObservation { address, ..Default::default() }));
        self.hits = Vec::new();
        self.capture_observation = Some(CaptureObservation {
            sites,
            events: VecDeque::with_capacity(CAPTURE_DETAIL_LIMIT),
            acknowledged: CaptureCursor::default(),
            next_sequence: CaptureCursor::default(),
            published: None,
            through_instructions: InstructionCount(0),
            boundary_complete: false,
            drained_boundary_observed: false,
            ram_incomplete: false,
            scope,
        });
        Ok(())
    }

    pub fn capture_observation_page(&mut self, from: CaptureCursor) -> Result<CaptureObservationPage<'_>, CaptureProtocolError> {
        let observation = self.capture_observation.as_mut().ok_or(CaptureProtocolError::Inactive)?;
        if from != observation.acknowledged {
            return Err(CaptureProtocolError::WrongCursor);
        }
        let count = observation.events.len().min(CAPTURE_PAGE_LIMIT);
        let next = CaptureCursor(from.0 + count as u64);
        if count != 0 {
            observation.published.get_or_insert(next);
        } else if observation.boundary_complete {
            observation.drained_boundary_observed = true;
        }
        let dropped = observation.sites.iter().map(|site| site.dropped).sum();
        Ok(CaptureObservationPage {
            from, next, acknowledged: observation.acknowledged,
            pending: observation.events.len(),
            captured: observation.next_sequence.0,
            through_instructions: observation.through_instructions,
            boundary_complete: observation.boundary_complete,
            total: observation.sites.iter().map(|site| site.total).sum(),
            dropped,
            incomplete: dropped != 0 || observation.ram_incomplete,
            first_instructions: observation.sites.iter().filter_map(|site| site.first_instructions).min(),
            last_instructions: observation.sites.iter().filter_map(|site| site.last_instructions).max(),
            bounds: CAPTURE_OBSERVATION_BOUNDS,
            sites: CaptureSites { observation, labels: &self.sites },
            hits: CaptureEvents { observation, labels: &self.sites, count },
        })
    }

    pub fn acknowledge_capture_observation(&mut self, next: CaptureCursor) -> Result<CaptureAcknowledgement, CaptureProtocolError> {
        let observation = self.capture_observation.as_mut().ok_or(CaptureProtocolError::Inactive)?;
        let published = observation.published.ok_or(CaptureProtocolError::NoPublishedPage)?;
        if next != published {
            return Err(CaptureProtocolError::WrongAcknowledgement);
        }
        for _ in observation.acknowledged.0..next.0 {
            if let Some(event) = observation.events.pop_front() {
                observation.sites[event.site_index].pending -= 1;
            }
        }
        observation.acknowledged = next;
        observation.published = None;
        Ok(CaptureAcknowledgement { acknowledged: next, pending: observation.events.len() })
    }

    pub fn capture_page_outstanding(&self) -> bool {
        self.capture_observation.as_ref().is_some_and(|observation| observation.published.is_some())
    }

    pub fn capture_drained(&self) -> bool {
        self.capture_observation.as_ref().is_some_and(|observation| {
            observation.events.is_empty() && observation.boundary_complete && observation.drained_boundary_observed
        })
    }

    pub(crate) fn begin_advance(&mut self) {
        if let Some(observation) = &mut self.capture_observation {
            observation.boundary_complete = false;
            observation.drained_boundary_observed = false;
        }
    }

    pub fn capture_snapshot(&self, session: &Session) -> Result<Option<CaptureSnapshot>, String> {
        let Some(observation) = &self.capture_observation else { return Ok(None); };
        let (ram, lifecycle, core) = if observation.scope.lifecycle() {
            let (ram, context) = save_lifecycle::snapshot(session, self.capture_memory.as_ref())?;
            (ram, Some(context), Some(0))
        } else {
            let mut ram = CaptureRam::default();
            ram.common(session);
            ram.recorder(session);
            (ram, None, None)
        };
        Ok(Some(CaptureSnapshot {
            instructions: InstructionCount::from_board(session)?,
            ram_incomplete: ram.incomplete,
            ram,
            lifecycle,
            core,
        }))
    }

    pub fn armed(&self) -> bool {
        !self.points.is_empty() && self.point_set_mode != PointSetMode::Persistent(Arming::Disarmed)
    }

    /// Replaces the armed sites on every core.
    pub fn arm(&mut self, session: &Session, sites: BTreeMap<u32, String>) -> Result<(), String> {
        self.arm_selection(session, sites, CoreSelection::All, None)
    }

    pub fn arm_with_task_capture(&mut self, session: &Session, sites: BTreeMap<u32, String>, task: TaskCapture) -> Result<(), String> {
        self.arm_selection(session, sites, CoreSelection::All, Some(task))
    }

    pub(crate) fn before_advance(&self, session: &Session) -> Result<(), String> {
        self.task_context.as_ref().map_or(Ok(()), |observer| observer.before_advance(session))
    }

    pub fn arm_primary(&mut self, session: &Session, sites: BTreeMap<u32, String>) -> Result<(), String> {
        self.arm_selection(session, sites, CoreSelection::Primary, None)
    }

    /// Arms `sites` on every core through one persistent all-PC point set per core. Later site and modal changes,
    /// and `rearm` after a retained disarm, update only its membership; `disarm` follows `set_disarmed_points`.
    pub fn arm_mutable(&mut self, session: &Session, sites: BTreeMap<u32, String>) -> Result<(), String> {
        if self.stack_scope.is_some() {
            return Err("stack scope sites cannot be replaced".into());
        }
        if self.modal_observation_active() {
            return Err("modal observation sites are frozen until successful finish".into());
        }
        if self.capture_observation_active() {
            return Err("capture observation sites are frozen until the session ends".into());
        }
        if self.point_set_mode == PointSetMode::Fixed {
            self.end_points(session)?;
        }
        let memberships = self.memberships(session, sites.keys().copied(), CoreSelection::All, PointScope::UserAndModal);
        self.apply(session, memberships)?;
        self.discard_events();
        self.resumed.clear();
        self.sites = sites;
        self.cores = CoreSelection::All;
        self.point_set_mode = PointSetMode::Persistent(Arming::Armed);
        self.restore_resume(session)
    }

    /// Applies at the next persistent `disarm` or `reinstall`.
    pub fn set_disarmed_points(&mut self, policy: DisarmedPoints) {
        self.disarmed_points = policy;
    }

    pub fn replace_sites(&mut self, session: &Session, sites: BTreeMap<u32, String>) -> Result<(), String> {
        if self.modal_observation_active() {
            return Err("modal observation sites are frozen until successful finish".into());
        }
        if self.capture_observation_active() {
            return Err("capture observation sites are frozen until the session ends".into());
        }
        if self.point_set_mode != PointSetMode::Persistent(Arming::Armed) {
            return Err("mutable instruction points are not armed".into());
        }
        let memberships = self.memberships(session, sites.keys().copied(), self.cores, PointScope::UserAndModal);
        self.apply(session, memberships)?;
        self.sites = sites;
        self.restore_resume(session)
    }

    fn arm_selection(
        &mut self,
        session: &Session,
        sites: BTreeMap<u32, String>,
        cores: CoreSelection,
        task: Option<TaskCapture>,
    ) -> Result<(), String> {
        if self.stack_scope.is_some() {
            return Err("stack scope sites cannot be replaced".into());
        }
        if self.modal_observation_active() {
            return Err("modal observation sites are frozen until successful finish".into());
        }
        if self.capture_observation_active() {
            return Err("capture observation sites are frozen until the session ends".into());
        }
        let context = match task {
            None => None,
            Some(TaskCapture::Off) => Some(None),
            Some(TaskCapture::Logical) => Some(Some(task_context::Observer::new(session)?)),
        };
        self.end_points(session)?;
        if let Some(context) = context { self.task_context = context; }
        self.sites = sites;
        self.cores = cores;
        self.point_set_mode = PointSetMode::Fixed;
        self.resumed.clear();
        self.rearm(session)
    }

    /// Removes fixed points. Persistent sets are removed or emptied according to `set_disarmed_points`.
    pub fn disarm(&mut self, session: &Session) -> Result<(), String> {
        if self.modal_observation_active() {
            return Err("modal observation sites cannot be disarmed before successful finish".into());
        }
        self.invalidate_presentation();
        match (self.point_set_mode, self.disarmed_points) {
            (PointSetMode::Fixed, _) => self.end_points(session),
            (PointSetMode::Persistent(_), DisarmedPoints::Release) => {
                self.end_points(session)?;
                self.point_set_mode = PointSetMode::Persistent(Arming::Disarmed);
                Ok(())
            }
            (PointSetMode::Persistent(_), DisarmedPoints::Retain) => {
                if !self.points.is_empty() {
                    self.apply(session, vec![Vec::new(); session.board.instruction_counts().len()])?;
                    self.discard_events();
                }
                self.point_set_mode = PointSetMode::Persistent(Arming::Disarmed);
                Ok(())
            }
        }
    }

    pub fn rearm(&mut self, session: &Session) -> Result<(), String> {
        match self.point_set_mode {
            PointSetMode::Fixed => self.rearm_scope(session, PointScope::UserAndModal),
            PointSetMode::Persistent(Arming::Armed) if !self.points.is_empty() => Err("trace sites are already armed".into()),
            PointSetMode::Persistent(_) => self.arm_membership(session, PointScope::UserAndModal),
        }
    }

    /// Removes every installed point before a session checkpoint, keeping the arming for `reinstall`.
    pub fn teardown(&mut self, session: &Session) -> Result<(), String> {
        self.invalidate_presentation();
        match self.point_set_mode {
            PointSetMode::Fixed => self.disarm(session),
            PointSetMode::Persistent(_) => {
                if self.modal_observation_active() {
                    return Err("modal observation sites cannot be disarmed before successful finish".into());
                }
                self.end_points(session)
            }
        }
    }

    pub fn reinstall(&mut self, session: &Session) -> Result<(), String> {
        match (self.point_set_mode, self.disarmed_points) {
            (PointSetMode::Fixed, _) => self.rearm(session),
            (PointSetMode::Persistent(Arming::Armed), _) => self.arm_membership(session, PointScope::UserAndModal),
            (PointSetMode::Persistent(Arming::Disarmed), DisarmedPoints::Release) => Ok(()),
            (PointSetMode::Persistent(Arming::Disarmed), DisarmedPoints::Retain) => {
                self.apply(session, vec![Vec::new(); session.board.instruction_counts().len()])
            }
        }
    }

    /// Removes from the last core; a set whose removal fails stays owned.
    fn end_points(&mut self, session: &Session) -> Result<(), String> {
        if let Some(observer) = &mut self.task_context { observer.disarm()?; }
        while let Some((core, points)) = self.points.last_mut() {
            session.board.instruction_points_end(*core, points)?;
            self.points.pop();
            self.pending_points.pop();
        }
        Ok(())
    }

    pub fn remove_sites(&mut self, session: &Session, addresses: &[Guest]) -> Result<(), String> {
        if self.stack_scope.as_ref().is_some_and(|scope| addresses.iter()
            .any(|address| scope.summary.sites.binary_search_by_key(address, |entry| entry.site).is_ok())) {
            return Err("stack scope sites cannot be removed".into());
        }
        if self.modal_observation_active() {
            return Err("modal observation sites are frozen until successful finish".into());
        }
        if self.capture_observation_active() {
            return Err("capture observation sites are frozen until the session ends".into());
        }
        if addresses.is_empty() { return Ok(()); }
        if addresses.iter().any(|address| !self.sites.contains_key(&address.0)) {
            return Err("trace site is not armed".into());
        }
        let persistent = self.point_set_mode != PointSetMode::Fixed;
        if persistent {
            if self.point_set_mode == PointSetMode::Persistent(Arming::Disarmed) {
                return Err("mutable instruction points are not armed".into());
            }
            let remaining = self.sites.keys().copied().filter(|site| !addresses.iter().any(|removed| removed.0 == *site));
            let memberships = self.memberships(session, remaining, self.cores, PointScope::UserAndModal);
            self.apply(session, memberships)?;
        } else { self.end_points(session)?; }
        for address in addresses {
            if let Some(label) = self.sites.remove(&address.0) {
                if !self.sites.values().any(|other| other == &label) {
                    self.stop_labels.remove(&label);
                }
            }
        }
        if !persistent { self.rearm(session)?; }
        self.restore_resume(session)
    }

    /// Adds stopping sites while preserving a stopped core's resume marker.
    pub fn add_sites(&mut self, session: &Session, sites: BTreeMap<u32, String>) -> Result<(), String> {
        if self.modal_observation_active() {
            return Err("modal observation sites are frozen until successful finish".into());
        }
        if self.capture_observation_active() {
            return Err("capture observation sites are frozen until the session ends".into());
        }
        if sites.keys().any(|address| self.sites.contains_key(address)) {
            return Err("trace site is already armed".into());
        }
        let persistent = self.point_set_mode != PointSetMode::Fixed;
        if persistent {
            if self.point_set_mode == PointSetMode::Persistent(Arming::Disarmed) {
                return Err("mutable instruction points are not armed".into());
            }
            let memberships = self.memberships(session, self.sites.keys().chain(sites.keys()).copied(), self.cores, PointScope::UserAndModal);
            self.apply(session, memberships)?;
        } else { self.end_points(session)?; }
        for label in sites.values() {
            if !self.stop_labels.contains(label) {
                self.stop_labels.insert(label.clone());
            }
        }
        self.sites.extend(sites);
        if !persistent { self.rearm(session)?; }
        self.restore_resume(session)
    }

    fn restore_resume(&self, session: &Session) -> Result<(), String> {
        if self.point_set_mode != PointSetMode::Fixed {
            return self.restore_persistent_resume(session);
        }
        let counts = session.board.instruction_counts();
        for (core, points) in &self.points {
            if let Some(marker) = self.resumed.get(core) {
                let current = session.board.read_register(*core, Register::Pc)?;
                if current == u64::from(marker.pc.0) && counts[*core] == marker.entered.0 {
                    points.resume(current, u64::from(marker.pc.0));
                    if let Some(observer) = &self.task_context { observer.resume(CoreId(*core), Some(marker.pc)); }
                }
            }
        }
        Ok(())
    }

    /// Re-derives each core's once-only resume from its undelivered entry or its still-current marker.
    fn restore_persistent_resume(&self, session: &Session) -> Result<(), String> {
        let counts = session.board.instruction_counts();
        for ((core, points), pending) in self.points.iter().zip(&self.pending_points) {
            let current = session.board.read_register(*core, Register::Pc)?;
            let marker = self.resumed.get(core).filter(|marker| counts[*core] == marker.entered.0)
                .map(|marker| marker.pc);
            // `resume` keeps a skip only for a member observed at the current PC, so `!current` cancels it.
            let observed = pending.map(|pending| pending.pc).or(marker);
            points.resume(current, observed.map(|pc| u64::from(pc.0)).unwrap_or(!current));
            if let Some(observer) = &self.task_context { observer.resume(CoreId(*core), observed.filter(|pc| u64::from(pc.0) == current)); }
        }
        Ok(())
    }

    /// Re-arms the current sites under `scope`: fixed points are reinstalled, persistent sets change membership.
    fn rearm_points(&mut self, session: &Session, scope: PointScope) -> Result<(), String> {
        match self.point_set_mode {
            PointSetMode::Fixed => self.end_points(session)
                .and_then(|()| self.rearm_scope(session, scope))
                .and_then(|()| self.restore_resume(session)),
            PointSetMode::Persistent(_) => self.arm_membership(session, scope),
        }
    }

    fn rearm_scope(&mut self, session: &Session, scope: PointScope) -> Result<(), String> {
        if !self.points.is_empty() {
            return Err("trace sites are already armed".into());
        }
        let modal = self.modal_observation.as_ref()
            .filter(|modal| matches!(scope, PointScope::UserAndModal) && modal.active());
        let mut addresses: BTreeSet<u64> = self.sites.keys().map(|&address| u64::from(address)).collect();
        if let Some(modal) = modal {
            addresses.extend(modal.sites().map(|pc| u64::from(pc.0)));
        }
        if self.presentation.is_some() {
            addresses.extend(crate::presentation::Site::ALL.map(|site| u64::from(site.address())));
        }
        if addresses.is_empty() { return Ok(()); }
        let addresses: Vec<u64> = addresses.into_iter().collect();
        for core in 0..session.board.instruction_counts().len() {
            let selected = modal.is_some() || self.presentation.is_some() || match self.cores {
                CoreSelection::All => true,
                CoreSelection::Primary => core == 0,
            };
            if !selected {
                continue;
            }
            for chunk in addresses.chunks(FIXED_POINT_SET_LIMIT) {
                let points = session.board.instruction_points_begin(core, chunk.to_vec())?;
                self.points.push((core, points));
                self.pending_points.push(None);
            }
            if let Some(observer) = &mut self.task_context { observer.arm_core(CoreId(core), &addresses)?; }
        }
        Ok(())
    }

    /// Each core's persistent membership. A modal scope selects every core; otherwise unselected cores are empty.
    fn memberships(&self, session: &Session, user: impl Iterator<Item = u32>, cores: CoreSelection, scope: PointScope) -> Vec<Vec<u64>> {
        let modal = self.modal_observation.as_ref()
            .filter(|modal| matches!(scope, PointScope::UserAndModal) && modal.active());
        let mut addresses: BTreeSet<u64> = user.map(u64::from).collect();
        if let Some(modal) = modal {
            addresses.extend(modal.sites().map(|pc| u64::from(pc.0)));
        }
        if self.presentation.is_some() {
            addresses.extend(crate::presentation::Site::ALL.map(|site| u64::from(site.address())));
        }
        let addresses: Vec<u64> = addresses.into_iter().collect();
        (0..session.board.instruction_counts().len())
            .map(|core| if modal.is_some() || self.presentation.is_some() || cores.includes(core) { addresses.clone() } else { Vec::new() })
            .collect()
    }

    /// Arms the current sites under `scope` in the persistent sets, resuming each stopped core once.
    fn arm_membership(&mut self, session: &Session, scope: PointScope) -> Result<(), String> {
        let memberships = self.memberships(session, self.sites.keys().copied(), self.cores, scope);
        self.apply(session, memberships)?;
        self.point_set_mode = PointSetMode::Persistent(Arming::Armed);
        self.restore_resume(session)
    }

    /// Sets each persistent core's membership, installing the sets when none are installed.
    fn apply(&mut self, session: &Session, memberships: Vec<Vec<u64>>) -> Result<(), String> {
        if self.points.is_empty() {
            return self.install(session, memberships);
        }
        if self.points.len() != memberships.len() {
            return Err("trace instruction points were not fully removed".into());
        }
        self.take_events()?;
        for ((core, points), addresses) in self.points.iter().zip(memberships) {
            if let Some(observer) = &mut self.task_context { observer.arm_core(CoreId(*core), &addresses)?; }
            points.replace_addresses(addresses)?;
        }
        Ok(())
    }

    fn install(&mut self, session: &Session, memberships: Vec<Vec<u64>>) -> Result<(), String> {
        for (core, addresses) in memberships.into_iter().enumerate() {
            if let Some(observer) = &mut self.task_context { observer.arm_core(CoreId(core), &addresses)?; }
            match session.board.instruction_points_begin_mutable(core, addresses) {
                Ok(points) => {
                    self.points.push((core, points));
                    self.pending_points.push(None);
                }
                Err(failure) => {
                    self.end_points(session)?;
                    return Err(failure);
                }
            }
        }
        Ok(())
    }

    fn take_events(&mut self) -> Result<(), String> {
        for ((_, points), pending) in self.points.iter().zip(&mut self.pending_points) {
            if pending.is_none() {
                *pending = points.event().map(|pc| {
                    Ok::<_, String>(PendingPoint {
                        pc: Guest(u32::try_from(pc).map_err(|_| "trace site exceeds32bits")?),
                        point: points.event_context(),
                    })
                }).transpose()?;
            }
        }
        Ok(())
    }

    fn discard_events(&mut self) {
        for ((_, points), pending) in self.points.iter().zip(&mut self.pending_points) {
            points.event();
            *pending = None;
        }
        if let Some(observer) = &self.task_context { observer.discard_events(); }
    }

    /// Pending hooks are retained until their details fit in the capture arena.
    pub fn collect(&mut self, session: &Session, instructions: InstructionCount) -> Result<CollectionOutcome, String> {
        let result = self.collect_entries(session, instructions);
        if let Some(observer) = &self.task_context { observer.check()?; }
        result
    }

    fn collect_entries(&mut self, session: &Session, instructions: InstructionCount) -> Result<CollectionOutcome, String> {
        if self.stack_scope.as_ref().is_some_and(|scope| scope.summary.violation.is_some()) {
            return Err("trace stack scope has already refused".into());
        }
        if let Some(modal) = self.modal_observation.as_mut().filter(|modal| modal.active()) {
            modal.check_interval()?;
        }
        if self.capture_page_outstanding() {
            return Ok(CollectionOutcome::DrainRequired);
        }
        if let Some(observation) = &mut self.capture_observation {
            observation.through_instructions = instructions;
            observation.boundary_complete = false;
        }
        let mut stop = false;
        let mut new_returns = [None; 2];
        let mut call_returns = Vec::new();
        let mut observed_modal = [None; 2];
        let counts = session.board.instruction_counts();
        self.take_events()?;
        for (point_index, (core, points)) in self.points.iter().enumerate() {
            let Some(pending) = self.pending_points[point_index] else { continue };
            let address = pending.pc.0;
            let pc = u64::from(address);
            if let Some(tracker) = &mut self.presentation {
                if let Some(site) = crate::presentation::Site::at(address) {
                    let mut word = [0; 4];
                    session.board.read_physical(u64::from(address), &mut word)?;
                    let cpsr = register(session, *core, arm::Register::Cpsr)?;
                    if u32::from_le_bytes(word) != site.word() || cpsr & 0x0100_0220 != 0 {
                        return Err("presentation instruction differs from X-M1 1.50 A32 source".into());
                    }
                    let frame = match site {
                        crate::presentation::Site::Enter => register(session, *core, arm::Register::R(13))?
                            .checked_sub(4).ok_or("presentation entry stack underflow")?,
                        _ => register(session, *core, arm::Register::R(11))?,
                    };
                    let result = match site {
                        crate::presentation::Site::AfterWait => register(session, *core, arm::Register::R(0))?,
                        crate::presentation::Site::Finish => register(session, *core, arm::Register::R(4))?,
                        crate::presentation::Site::Enter => 0,
                    };
                    tracker.observe(site, crate::presentation::Core::from_index(*core)?,
                        crate::presentation::FramePointer(frame), crate::presentation::CoreInstructions(counts[*core]), result);
                }
            }
            let task = self.task_context.as_ref().map(|observer| observer.take(CoreId(*core), pending.point)).transpose()?.flatten();
            if let Some(modal) = self.modal_observation.as_mut().filter(|modal| modal.active()) {
                if modal.contains(Guest(address)) {
                    new_returns[*core] = modal.observe(session, *core, Guest(address), instructions, pending.point)?;
                    observed_modal[*core] = Some(Guest(address));
                }
            }
            let selected = matches!(self.cores, CoreSelection::All) || *core == 0;
            let Some(site) = self.sites.get(&address).filter(|_| selected) else {
                let current = session.board.read_register(*core, Register::Pc)?;
                points.resume(current, pc);
                if let Some(observer) = &self.task_context { observer.resume(CoreId(*core), (current == pc).then_some(pending.pc)); }
                self.pending_points[point_index] = None;
                if current == pc {
                    self.resumed.insert(*core, ResumeMarker {
                        pc: Guest(address), entered: EnteredInstructions(counts[*core]), point: pending.point,
                        modal: if observed_modal[*core] == Some(Guest(address)) {
                            ModalPointState::Observed
                        } else { ModalPointState::Unobserved },
                    });
                } else {
                    self.resumed.remove(core);
                }
                continue;
            };
            if let Some(observation) = &mut self.capture_observation {
                if self.stop_labels.contains(site) {
                    return Err("capture observation encountered a rendezvous stop".into());
                }
                let instructions = observation.through_instructions;
                let index = observation.sites.binary_search_by_key(&address, |site| site.address)
                    .map_err(|_| "capture observation site is missing")?;
                if observation.sites[index].pending == CAPTURE_DETAILS_PER_SITE
                    || observation.events.len() == CAPTURE_DETAIL_LIMIT {
                    return Ok(CollectionOutcome::DrainRequired);
                }
                let sequence = observation.next_sequence;
                let next_sequence = sequence.0.checked_add(1).ok_or("capture event sequence exceeds 64 bits")?;
                let registers = [
                    register(session, *core, arm::Register::R(0))?,
                    register(session, *core, arm::Register::R(1))?,
                    register(session, *core, arm::Register::R(2))?,
                    register(session, *core, arm::Register::R(3))?,
                ];
                let sp = register(session, *core, arm::Register::R(13))?;
                let (ram, lifecycle) = if observation.scope.lifecycle() {
                    let (ram, context) = save_lifecycle::event(observation.scope, session, self.capture_memory.as_ref(),
                        MemoryCore(*core), Guest(address), &registers)?;
                    (ram, Some(context))
                } else {
                    (CaptureRam::event(session, self.capture_memory.as_ref(), MemoryCore(*core), Guest(address), &registers, Guest(sp)), None)
                };
                let event = CaptureEvent {
                    sequence, site_index: index, core: *core, instructions, registers,
                    sp,
                    lr: register(session, *core, arm::Register::R(14))?,
                    cpsr: register(session, *core, arm::Register::Cpsr)?,
                    ram,
                    lifecycle,
                };
                let current = session.board.read_register(*core, Register::Pc)?;
                observation.ram_incomplete |= event.ram.incomplete;
                observation.events.push_back(event);
                observation.next_sequence = CaptureCursor(next_sequence);
                let counts = &mut observation.sites[index];
                counts.total += 1;
                counts.captured += 1;
                counts.pending += 1;
                counts.first_instructions.get_or_insert(instructions);
                counts.last_instructions = Some(instructions);
                observation.drained_boundary_observed = false;
                self.pending_points[point_index] = None;
                points.resume(current, pc);
                continue;
            }
            let scoped = self.stack_scope.as_mut().map(|scope| {
                scope.observe(session, *core, Guest(address), EnteredInstructions(instructions.0))
            }).transpose()?.unwrap_or(StackPoint::Unscoped);
            if !matches!(scoped, StackPoint::Outside) {
                stop |= self.stop_labels.contains(site);
                if self.hit_limit.is_some_and(|limit| self.hits.len() >= limit.get()) {
                    return Err("trace hit limit reached".into());
                }
                let instructions = counts.iter().sum();
                let site = site.clone();
                let registers = (0..if self.full_registers { 13u8 } else { 4u8 })
                    .map(|index| {
                        let name = ["r0", "r1", "r2", "r3", "r4", "r5", "r6", "r7", "r8", "r9", "r10", "r11", "r12"][usize::from(index)];
                        register(session, *core, arm::Register::R(index)).map(|value| (name, format!("{value:#x}")))
                    })
                    .collect::<Result<_, String>>()?;
                let (observation, call_return) = self.calls.observe(session, *core, address)?;
                call_returns.extend(call_return);
                let sp = register(session, *core, arm::Register::R(13))?;
                let mut memory = Vec::new();
                let mut stack_memory = Vec::new();
                for read in self.site_reads.get(&address).into_iter().flatten() {
                    let (resolved, count) = match *read {
                        SiteRead::Absolute([address, count]) => (address, count as usize),
                        SiteRead::Stack { sp: offset, bytes } => {
                            let address = i64::from(sp) + i64::from(offset);
                            if address < 0 || address + bytes as i64 > 1i64 << 32 {
                                return Err("stack-relative trace read exceeds the guest address space".into());
                            }
                            (address as u64, bytes)
                        }
                    };
                    let mut bytes = vec![0; count];
                    session.board.read_physical(resolved, &mut bytes)?;
                    let bytes = bytes.iter().map(|byte| format!("{byte:02x}")).collect::<String>();
                    match *read {
                        SiteRead::Absolute(_) => memory.push(bytes),
                        SiteRead::Stack { sp, .. } => stack_memory.push(StackRead {
                            kind: StackReadKind::Sp, sp, address: format!("{resolved:#x}"), bytes,
                        }),
                    }
                }
                self.hits.push(Hit {
                    site,
                    address: format!("{address:#x}"),
                    core: *core,
                    instructions,
                    task,
                    render_object_provenance_sequence: self.render_object_provenance_sequence,
                    registers,
                    sp: format!("{sp:#x}"),
                    lr: format!("{:#x}", register(session, *core, arm::Register::R(14))?),
                    cpsr: register(session, *core, arm::Register::Cpsr)?,
                    code_word: self.code_binding.as_ref().map(|binding| binding.sample(session, *core, address)).transpose()?.flatten(),
                    memory,
                    stack_memory,
                    observation,
                });
                if let StackPoint::Inside { index, next } = scoped {
                    if let Some(scope) = &mut self.stack_scope {
                        scope.summary.sites[index].recorded[*core] = next;
                    }
                }
            }
            let current = session.board.read_register(*core, Register::Pc)?;
            points.resume(current, pc);
            if let Some(observer) = &self.task_context { observer.resume(CoreId(*core), (current == pc).then_some(pending.pc)); }
            self.pending_points[point_index] = None;
            if current == pc {
                self.resumed.insert(*core, ResumeMarker {
                    pc: Guest(address), entered: EnteredInstructions(counts[*core]), point: pending.point,
                    modal: if observed_modal[*core] == Some(Guest(address)) {
                        ModalPointState::Observed
                    } else { ModalPointState::Unobserved },
                });
            } else {
                self.resumed.remove(core);
            }
        }
        let mut calls_changed = false;
        for address in call_returns {
            if let std::collections::btree_map::Entry::Vacant(entry) = self.sites.entry(address) {
                entry.insert("call-return".into());
                calls_changed = true;
            }
        }
        let sites_before = self.sites.len();
        self.sites.retain(|address, label| label != "call-return" || self.calls.pending_return(*address));
        calls_changed |= self.sites.len() != sites_before;
        if calls_changed || new_returns.iter().any(Option::is_some) {
            if let Some(modal) = &mut self.modal_observation {
                for (&core, marker) in &mut self.resumed {
                    if new_returns.contains(&Some(marker.pc)) && marker.modal == ModalPointState::Unobserved
                        && counts[core] == marker.entered.0
                        && session.board.read_register(core, Register::Pc)? == u64::from(marker.pc.0) {
                        modal.observe(session, core, marker.pc, instructions, marker.point)?;
                        marker.modal = ModalPointState::Observed;
                    }
                }
            }
            let rearmed = self.rearm_points(session, PointScope::UserAndModal);
            if let Err(failure) = rearmed {
                if let Some(modal) = &mut self.modal_observation { modal.fail(failure.clone()); }
                return Err(failure);
            }
        }
        if let Some(observation) = &mut self.capture_observation {
            observation.boundary_complete = true;
        }
        Ok(if stop { CollectionOutcome::Stop } else { CollectionOutcome::Continue })
    }
}

#[cfg(test)]
mod tests {
    use super::{CaptureCursor, CaptureProtocolError, CaptureScope, TraceSites};

    #[test]
    fn observation_rejects_inactive_cursor_invalid_cursor_and_duplicate_begin() {
        let mut traces = TraceSites::default();
        assert!(matches!(traces.capture_observation_page(CaptureCursor::new(0)), Err(CaptureProtocolError::Inactive)));
        traces.begin_capture_observation(CaptureScope::Capture).unwrap();
        assert!(matches!(traces.capture_observation_page(CaptureCursor::new(1)), Err(CaptureProtocolError::WrongCursor)));
        assert!(traces.begin_capture_observation(CaptureScope::Capture).is_err());
    }
}
