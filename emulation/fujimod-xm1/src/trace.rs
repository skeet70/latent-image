//! Labelled instruction points on every core and the entries recorded at them.

use crate::provenance::Guest;
use fujimod_emulator::board::observation::InstructionPoints;
use fujimod_emulator::session::Session;
use fujimod_emulator_cpu::{Register, arm};
use serde::{Deserialize, Serialize, Serializer, ser::SerializeSeq};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

/// One entry into an armed site, before its instruction executes.
#[derive(Clone, Debug, Serialize)]
pub struct Hit {
    pub site: String,
    pub address: String,
    pub core: usize,
    pub instructions: u64,
    pub registers: BTreeMap<&'static str, String>,
    pub sp: String,
    pub lr: String,
    pub cpsr: u32,
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
        + std::mem::size_of::<CaptureObservation>() + CAPTURE_OVERHEAD_BYTES,
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

    fn word(&self, address: u32) -> Option<u32> {
        self.spans[..self.count].iter().find_map(|span| {
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
        if self.word(0x015ad53c).is_none() {
            self.read(session, 0x015ad53c, 4);
        }
        self.read(session, 0x015ad430, 32);
        let Some(index) = self.word(0x015ad53c).filter(|&index| index <= 200) else {
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
        if let (Some(length), Some(payload)) = (self.word(physical + 4), self.word(physical + 8)) {
            self.read(session, payload, length as usize);
        }
        if self.word(physical).is_some_and(|word| word & 0xffff == 0x137e) {
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

    fn event(session: &Session, address: u32, registers: &[u32; 4]) -> Self {
        let mut ram = Self::default();
        match address {
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
}

const _: () = assert!(
    std::mem::size_of::<CaptureEvent>() * CAPTURE_DETAIL_LIMIT
        + std::mem::size_of::<SiteObservation>() * CAPTURE_SITE_LIMIT
        + std::mem::size_of::<CaptureObservation>()
        + CAPTURE_OVERHEAD_BYTES <= CAPTURE_MEMORY_BYTES
);

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
}

#[derive(Clone, Copy, Default)]
enum CoreSelection {
    #[default]
    All,
    Primary,
}

struct EnteredInstructions(u64);
struct ResumeMarker {
    pc: Guest,
    entered: EnteredInstructions,
}

#[derive(Default)]
pub struct TraceSites {
    pub sites: BTreeMap<u32, String>,
    /// Labels whose entry ends the current run.
    pub stop_labels: BTreeSet<String>,
    pub hits: Vec<Hit>,
    points: Vec<(usize, Box<InstructionPoints>)>,
    cores: CoreSelection,
    resumed: BTreeMap<usize, ResumeMarker>,
    capture_observation: Option<CaptureObservation>,
    pending_points: Vec<Option<u64>>,
}

fn register(session: &Session, core: usize, register: arm::Register) -> Result<u32, String> {
    let value = session.board.read_register(core, Register::Arm32(register))?;
    u32::try_from(value).map_err(|_| "ARM register exceeds 32 bits".to_owned())
}

impl TraceSites {
    pub fn capture_observation_active(&self) -> bool {
        self.capture_observation.is_some()
    }

    pub fn begin_capture_observation(&mut self) -> Result<(), String> {
        if self.capture_observation.is_some() {
            return Err("capture observation is already active".into());
        }
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
        if !self.capture_observation_active() { return Ok(None); }
        let mut ram = CaptureRam::default();
        ram.common(session);
        ram.recorder(session);
        Ok(Some(CaptureSnapshot {
            instructions: InstructionCount::from_board(session)?,
            ram_incomplete: ram.incomplete,
            ram,
        }))
    }

    pub fn armed(&self) -> bool {
        !self.points.is_empty()
    }

    /// Replaces the armed sites on every core.
    pub fn arm(&mut self, session: &Session, sites: BTreeMap<u32, String>) -> Result<(), String> {
        self.arm_selection(session, sites, CoreSelection::All)
    }

    pub fn arm_primary(&mut self, session: &Session, sites: BTreeMap<u32, String>) -> Result<(), String> {
        self.arm_selection(session, sites, CoreSelection::Primary)
    }

    fn arm_selection(
        &mut self,
        session: &Session,
        sites: BTreeMap<u32, String>,
        cores: CoreSelection,
    ) -> Result<(), String> {
        if self.capture_observation_active() {
            return Err("capture observation sites are frozen until the session ends".into());
        }
        self.disarm(session)?;
        self.sites = sites;
        self.cores = cores;
        self.resumed.clear();
        self.rearm(session)
    }

    pub fn disarm(&mut self, session: &Session) -> Result<(), String> {
        for (core, mut points) in self.points.drain(..) {
            session.board.instruction_points_end(core, &mut points)?;
        }
        self.pending_points.clear();
        Ok(())
    }

    pub fn remove_site(&mut self, session: &Session, address: Guest) -> Result<(), String> {
        if self.capture_observation_active() {
            return Err("capture observation sites are frozen until the session ends".into());
        }
        if !self.sites.contains_key(&address.0) {
            return Err("trace site is not armed".into());
        }
        self.disarm(session)?;
        let label = self.sites.remove(&address.0).ok_or("trace site disappeared")?;
        if !self.sites.values().any(|other| other == &label) {
            self.stop_labels.remove(&label);
        }
        self.rearm(session)?;
        self.restore_resume(session)
    }

    /// Arms more sites, keeping a core stopped at an armed site resumable.
    pub fn add_sites(&mut self, session: &Session, sites: BTreeMap<u32, String>) -> Result<(), String> {
        if self.capture_observation_active() {
            return Err("capture observation sites are frozen until the session ends".into());
        }
        if sites.keys().any(|address| self.sites.contains_key(address)) {
            return Err("trace site is already armed".into());
        }
        self.disarm(session)?;
        self.sites.extend(sites);
        self.rearm(session)?;
        self.restore_resume(session)
    }

    fn restore_resume(&self, session: &Session) -> Result<(), String> {
        let counts = session.board.instruction_counts();
        for (core, points) in &self.points {
            if let Some(marker) = self.resumed.get(core) {
                let current = session.board.read_register(*core, Register::Pc)?;
                if current == u64::from(marker.pc.0) && counts[*core] == marker.entered.0 {
                    points.resume(current, u64::from(marker.pc.0));
                }
            }
        }
        Ok(())
    }

    pub fn rearm(&mut self, session: &Session) -> Result<(), String> {
        if !self.points.is_empty() {
            return Err("trace sites are already armed".into());
        }
        if self.sites.is_empty() {
            return Ok(());
        }
        let addresses: Vec<u64> = self.sites.keys().map(|&address| u64::from(address)).collect();
        for core in 0..session.board.instruction_counts().len() {
            let selected = match self.cores {
                CoreSelection::All => true,
                CoreSelection::Primary => core == 0,
            };
            if !selected {
                continue;
            }
            let points = session.board.instruction_points_begin(core, addresses.clone())?;
            self.points.push((core, points));
            self.pending_points.push(None);
        }
        Ok(())
    }

    /// Pending hooks are retained until their details fit in the capture arena.
    pub fn collect(&mut self, session: &Session, instructions: InstructionCount) -> Result<CollectionOutcome, String> {
        if self.capture_page_outstanding() {
            return Ok(CollectionOutcome::DrainRequired);
        }
        if let Some(observation) = &mut self.capture_observation {
            observation.through_instructions = instructions;
            observation.boundary_complete = false;
        }
        let mut stop = false;
        for (index, (_, points)) in self.points.iter().enumerate() {
            if self.pending_points[index].is_none() {
                self.pending_points[index] = points.event();
            }
        }
        for (point_index, (core, points)) in self.points.iter().enumerate() {
            let Some(pc) = self.pending_points[point_index] else { continue };
            let address = u32::try_from(pc).map_err(|_| "trace site exceeds 32 bits")?;
            let site = self.sites.get(&address).ok_or("instruction point outside the armed sites")?;
            stop |= self.stop_labels.contains(site);
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
                let ram = CaptureRam::event(session, address, &registers);
                let event = CaptureEvent {
                    sequence, site_index: index, core: *core, instructions, registers,
                    sp: register(session, *core, arm::Register::R(13))?,
                    lr: register(session, *core, arm::Register::R(14))?,
                    cpsr: register(session, *core, arm::Register::Cpsr)?,
                    ram,
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
            let counts = session.board.instruction_counts();
            let instructions = counts.iter().sum();
            let site = site.clone();
            let registers = (0..4u8)
                .map(|index| {
                    let name = ["r0", "r1", "r2", "r3"][usize::from(index)];
                    register(session, *core, arm::Register::R(index)).map(|value| (name, format!("{value:#x}")))
                })
                .collect::<Result<_, String>>()?;
            self.hits.push(Hit {
                site,
                address: format!("{address:#x}"),
                core: *core,
                instructions,
                registers,
                sp: format!("{:#x}", register(session, *core, arm::Register::R(13))?),
                lr: format!("{:#x}", register(session, *core, arm::Register::R(14))?),
                cpsr: register(session, *core, arm::Register::Cpsr)?,
            });
            let current = session.board.read_register(*core, Register::Pc)?;
            points.resume(current, pc);
            self.pending_points[point_index] = None;
            if current == pc {
                self.resumed.insert(*core, ResumeMarker { pc: Guest(address), entered: EnteredInstructions(counts[*core]) });
            } else {
                self.resumed.remove(core);
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
    use super::{CaptureCursor, CaptureProtocolError, TraceSites};

    #[test]
    fn observation_rejects_inactive_cursor_invalid_cursor_and_duplicate_begin() {
        let mut traces = TraceSites::default();
        assert!(matches!(traces.capture_observation_page(CaptureCursor::new(0)), Err(CaptureProtocolError::Inactive)));
        traces.begin_capture_observation().unwrap();
        assert!(matches!(traces.capture_observation_page(CaptureCursor::new(1)), Err(CaptureProtocolError::WrongCursor)));
        assert!(traces.begin_capture_observation().is_err());
    }
}
