//! The composed-startup application phase: task1 samples, queue1 receives, pump calls, readiness with a stable
//! first-ready, the route's settled inputs with visible captures, the per-role checks `--compare` runs over them and
//! the controls each check must refuse.
use crate::composed_catalog::{physical_address, CodeLabel, CodeWord, Core, Domain, Instructions, Pc, QueueLabel, SiteLabel, Sha256};
use crate::composed_receipt::{ByteRange, HexBytes, Word, WriteKind};
use crate::guest::RouteKey;
use crate::provenance::Guest;
use crate::render::{Graphics, Probe};
use crate::trace::{Hit, TraceStackScopeSummary};
use fujimod_emulator::session::Session;
use serde::{Deserialize, Serialize};
use std::io::{BufRead, Read, Write};
use std::path::Path;

mod capture;
mod checks;
mod controls;
mod planes;
mod readiness;
mod scenarios;

pub use scenarios::Scenario;

pub use capture::{hit_observers, Observers, Recorder};
pub use checks::{
    capture_bound, check_evidence, check_pair, check_role, code_identity, initializer, mapping, pump_sequences, queue_provenance,
    readiness, render_completion, route, stack_context, task_context, ApplicationCheck, ApplicationRefusal, PairField, RoleEvidence,
    RoleSummary,
};
pub use controls::{run_controls, Control, ControlReport, ControlRow, Expected};

const ALIAS: u32 = 0x4000_0000;
const DRAM_END: u64 = 0x2000_0000;
/// The most words one view reads.
pub const VIEW_WORDS: usize = 80;
/// Decoded bytes of the evidence side file.
pub const EVIDENCE_LIMIT: u64 = 4 << 30;
/// Bytes of any other side file.
pub const SIDE_FILE_LIMIT: u64 = 256 << 20;
pub const EVIDENCE: &str = "application-evidence.json.gz";

/// Words at `address` read through the physical bus, and through the low and `0x40000000` spellings on each core.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RawView {
    pub address: Guest,
    pub physical: HexBytes,
    /// `[core][low, alias]`.
    pub spellings: [[HexBytes; 2]; 2],
}

impl RawView {
    pub fn read(session: &Session, address: Guest, words: usize) -> Result<Self, String> {
        if address.0 % 4 != 0 || words == 0 || words > VIEW_WORDS {
            return Err(format!("view at {address} is not 1..={VIEW_WORDS} aligned words"));
        }
        let low = physical_address(address.0);
        let length = words * 4;
        if u64::from(low) + length as u64 > DRAM_END {
            return Err(format!("view at {address} is outside DRAM"));
        }
        let mut physical = vec![0; length];
        session.board.read_physical(u64::from(low), &mut physical)?;
        let spelled = |core: usize, spelling: u32| -> Result<HexBytes, String> {
            let mut bytes = vec![0; length];
            session.board.read_memory(core, u64::from(spelling), &mut bytes)?;
            Ok(HexBytes(bytes))
        };
        let spellings = [[spelled(0, low)?, spelled(0, low | ALIAS)?], [spelled(1, low)?, spelled(1, low | ALIAS)?]];
        Ok(Self { address, physical: HexBytes(physical), spellings })
    }

    /// Every spelling read the physical bytes, which are whole words.
    pub fn agrees(&self) -> bool {
        !self.physical.0.is_empty() && self.physical.0.len() % 4 == 0 && self.spellings.iter().flatten().all(|bytes| bytes == &self.physical)
    }

    /// The physical word at `address` in either spelling, if the view holds it.
    pub fn word(&self, address: u32) -> Option<u32> {
        let offset = physical_address(address).checked_sub(physical_address(self.address.0))? as usize;
        if offset % 4 != 0 {
            return None;
        }
        let bytes = self.physical.0.get(offset..offset + 4)?;
        Some(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    pub fn words(&self) -> impl Iterator<Item = u32> + '_ {
        self.physical.0.chunks_exact(4).map(|bytes| u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }
}

/// The first view holding the word at `address`.
pub fn view_word(views: &[RawView], address: u32) -> Option<u32> { views.iter().find_map(|view| view.word(address)) }

pub fn view_byte(views: &[RawView], address: u32) -> Option<u8> {
    view_word(views, address & !3).map(|word| (word >> (8 * (address & 3))) as u8)
}

/// One byte-wide firmware fact and the aligned view holding it.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ByteView {
    pub address: Guest,
    pub view: RawView,
}

impl ByteView {
    pub fn read(session: &Session, address: Guest) -> Result<Self, String> {
        Ok(Self { address, view: RawView::read(session, Guest(address.0 & !3), 1)? })
    }
    pub fn value(&self) -> Option<u8> { view_byte(std::slice::from_ref(&self.view), self.address.0) }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct ControllerState(pub u16);

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ControllerStateView {
    pub address: Guest,
    pub view: RawView,
}

impl ControllerStateView {
    pub fn read(session: &Session, address: Guest) -> Result<Self, String> {
        if address.0 % 2 != 0 {
            return Err("a controller state address is not halfword-aligned".into());
        }
        Ok(Self { address, view: RawView::read(session, Guest(address.0 & !3), 1)? })
    }

    pub fn value(&self) -> Option<ControllerState> {
        (self.address.0 % 2 == 0).then(|| self.view.word(self.address.0 & !3)
            .map(|word| ControllerState((word >> (8 * (self.address.0 & 3))) as u16))).flatten()
    }
}

const CONTROLLER74: u32 = 0x146761c;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Registers {
    pub r: [Word; 13],
    pub sp: Word,
    pub lr: Word,
    pub cpsr: Word,
}

fn hex_word(text: &str) -> Result<u32, String> {
    u32::from_str_radix(text.strip_prefix("0x").ok_or("trace word is not hexadecimal")?, 16).map_err(|error| error.to_string())
}

impl Registers {
    pub fn of(hit: &Hit) -> Result<Self, String> {
        const NAMES: [&str; 13] = ["r0", "r1", "r2", "r3", "r4", "r5", "r6", "r7", "r8", "r9", "r10", "r11", "r12"];
        let mut r = [Word(0); 13];
        for (slot, name) in r.iter_mut().zip(NAMES) {
            *slot = Word(hex_word(hit.registers.get(name).ok_or("hit lacks full registers")?)?);
        }
        Ok(Self { r, sp: Word(hex_word(&hit.sp)?), lr: Word(hex_word(&hit.lr)?), cpsr: Word(hit.cpsr) })
    }
}

/// A core's CP14 kernel identity.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct CoreIdentity(pub u32);

impl CoreIdentity {
    pub fn domain(self) -> Option<Domain> { Domain::of_identity(self.0) }
}

/// Position in the evidence's sample list.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct SampleIndex(pub u64);

/// Position in the evidence's readiness list.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct ReadinessIndex(pub u64);

/// Position in the receipt's route inputs.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct InputIndex(pub u64);

/// The armed site a sample stopped at; a dynamic service target is keyed by the sample's PC.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", content = "label", rename_all = "kebab-case")]
pub enum SiteRef {
    Application(SiteLabel),
    Queue(QueueLabel),
    Target,
}

/// One stop at an application, queue or dynamic target site.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Sample {
    pub index: SampleIndex,
    pub site: SiteRef,
    /// As fetched.
    pub pc: Pc,
    pub core: Core,
    pub stop: Instructions,
    /// The first instruction count after the stopped fetch.
    pub resume: Instructions,
    /// Per core, summing to `stop`.
    pub counts: [Instructions; 2],
    pub identities: [CoreIdentity; 2],
    pub registers: Registers,
    pub code: RawView,
    /// Both domains' task layout cells; at task1-attributed sites only.
    pub task: Vec<RawView>,
    pub stack: Vec<RawView>,
    /// Controller, state, queue roots and their tables; at task1's entry and reset and at queue and dynamic-target sites.
    pub state: Vec<RawView>,
    pub packet: Vec<RawView>,
    pub input: Option<InputIndex>,
}

impl Sample {
    pub fn views(&self) -> impl Iterator<Item = &RawView> {
        std::iter::once(&self.code).chain(&self.task).chain(&self.stack).chain(&self.state).chain(&self.packet)
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ReadinessKind {
    Periodic,
    /// Taken at a candidate's first-ready plus the settle interval.
    SettleEnd,
}

/// The render facts `RenderFacts::probe` reads, as views.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RenderViews {
    pub initialized: ByteView,
    pub ui_busy: ByteView,
    /// UI pending, display suppression, batch busy and callback, commands dirty and display pending.
    pub idle: Vec<RawView>,
    /// Each key's released byte.
    pub keys: Vec<ByteView>,
}

/// The task tables `RenderFacts::probe` walks for task statuses.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TaskTableViews {
    /// `RenderFacts::task_entries`: the main UI (task1), graphic, VGLIB and UILIB task entries.
    pub entries: Vec<Pc>,
    /// Each table record: the table pointer and its row count.
    pub records: Vec<RawView>,
    /// The tables' 0x38-byte rows.
    pub rows: Vec<RawView>,
    /// The creation records the rows name, where readable.
    pub descriptors: Vec<RawView>,
}

pub(crate) const TASK_ROW_BYTES: u32 = 0x38;
pub(crate) const TASK_ROW_LIMIT: u32 = 128;

impl TaskTableViews {
    /// Statuses in `entries` order as the probe derives them; all `None` when a table's rows were not viewed.
    pub fn statuses(&self) -> Vec<Option<u8>> {
        let mut statuses = vec![None; self.entries.len()];
        for record in &self.records {
            let (Some(table), Some(count)) = (record.word(record.address.0), record.word(record.address.0.wrapping_add(4))) else {
                return vec![None; self.entries.len()];
            };
            if !(1..=TASK_ROW_LIMIT).contains(&count) {
                continue;
            }
            for index in 0..count {
                let row = table.wrapping_add(index * TASK_ROW_BYTES);
                let (Some(descriptor), Some(status)) = (view_word(&self.rows, row.wrapping_add(8)), view_word(&self.rows, row.wrapping_add(12))) else {
                    return vec![None; self.entries.len()];
                };
                let entry = view_word(&self.descriptors, descriptor.wrapping_add(8));
                let active = view_word(&self.descriptors, descriptor.wrapping_add(36)).is_some_and(|word| word != 0);
                if let Some(slot) = entry.filter(|_| active).and_then(|entry| self.entries.iter().position(|candidate| candidate.0 == entry)) {
                    statuses[slot] = Some((status & 0xff) as u8);
                }
            }
        }
        statuses
    }

    pub fn views(&self) -> impl Iterator<Item = &RawView> { self.records.iter().chain(&self.rows).chain(&self.descriptors) }
}

const SECOND_QUEUE: u32 = 0x949dd0;
const SECOND_HOLD: u32 = 0x949dd4;
const MAIN_QUEUE: u32 = 0x94f5d8;
const MAIN_HOLD: u32 = 0x94f5dc;
pub(crate) const PRIORITY_COUNT: u32 = 0x950098;
const SELECTION_CHANGED: u32 = 0x950148;
const PENDING_WORDS: [u32; 2] = [0x951424, 0x951528];
/// Xm1MenuState: node pointer at +0, cursor at +8.
const MENU_STATE: u32 = 0x9571d0;
/// Xm1MenuState +0x424.
pub(crate) const MENU_LEVEL: u32 = 0x9575f4;

/// The Xm1UiPolicy cells task1's loop tests before its receive at `0xc2314c` (`0xc22fa0`-`0xc23148`), and Xm1MenuState.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct UiWorkViews {
    /// Head and tail u16s of the second (`0x949dd0`) and main (`0x94f5d8`) deferred queues, each with its hold byte.
    pub queues: [RawView; 2],
    /// Priority-queue count `0x950098`, selection-changed `0x950148` and pending words `0x951424`, `0x951528`.
    pub words: Vec<RawView>,
    pub menu: RawView,
    pub depth: RawView,
    /// Three words at the node pointer, when it is nonzero and readable.
    pub node: Option<RawView>,
}

/// The menu at a readiness sample.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct MenuIdentity {
    pub node: u32,
    pub id: Option<u32>,
    pub setting: Option<u32>,
    pub cursor: u32,
    pub level: u32,
}

impl UiWorkViews {
    pub fn read(session: &Session) -> Result<Self, String> {
        let menu = RawView::read(session, Guest(MENU_STATE), 3)?;
        let pointer = menu.word(MENU_STATE).unwrap_or(0);
        Ok(Self {
            queues: [RawView::read(session, Guest(SECOND_QUEUE), 2)?, RawView::read(session, Guest(MAIN_QUEUE), 2)?],
            words: [PRIORITY_COUNT, SELECTION_CHANGED, PENDING_WORDS[0], PENDING_WORDS[1]].into_iter()
                .map(|address| RawView::read(session, Guest(address), 1)).collect::<Result<_, _>>()?,
            menu,
            depth: RawView::read(session, Guest(MENU_LEVEL), 1)?,
            node: (pointer != 0).then(|| RawView::read(session, Guest(pointer & !3), 3).ok()).flatten(),
        })
    }

    pub fn views(&self) -> impl Iterator<Item = &RawView> {
        self.queues.iter().chain([&self.menu, &self.depth]).chain(&self.words).chain(&self.node)
    }

    /// Conjunct 5: the second queue's hold clear, the priority queue empty, the selection-changed and pending words
    /// not 1, and each deferred queue held or with head equal to tail.
    pub fn drained(&self) -> bool {
        let word = |address| view_word(&self.words, address);
        let half = |address: u32| view_word(&self.queues, address & !3).map(|value| (value >> (8 * (address & 3))) as u16);
        let idle = |queue: u32, hold: Option<u8>| hold == Some(1) || half(queue).is_some() && half(queue) == half(queue + 2);
        let second_hold = view_byte(&self.queues, SECOND_HOLD);
        second_hold == Some(0)
            && word(PRIORITY_COUNT) == Some(0)
            && [SELECTION_CHANGED, PENDING_WORDS[0], PENDING_WORDS[1]].into_iter().all(|address| word(address).is_some_and(|value| value != 1))
            && idle(MAIN_QUEUE, view_byte(&self.queues, MAIN_HOLD))
            && idle(SECOND_QUEUE, second_hold)
    }

    pub fn menu(&self) -> Option<MenuIdentity> {
        let node = self.menu.word(MENU_STATE)?;
        let at = |offset: u32| self.node.as_ref().filter(|view| physical_address(view.address.0) == physical_address(node & !3)).and_then(|view| view.word(node + offset));
        Some(MenuIdentity { node, id: at(0), setting: at(8), cursor: self.menu.word(MENU_STATE + 8)?, level: self.depth.word(MENU_LEVEL)? })
    }
}

/// The vector GPU engine's state from its device save.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GraphicsState {
    pub busy: bool,
    pub command_pending: bool,
    /// Register `0x34`.
    pub control: u32,
    pub jobs_started: u64,
    pub jobs_completed: u64,
    pub bytes_completed: u64,
    pub acknowledgments: u64,
}

impl GraphicsState {
    pub fn of(save: &serde_json::Value) -> Result<Self, String> {
        #[derive(Deserialize)]
        struct Engine {
            regs: Vec<(u32, u32)>,
            busy: bool,
            command_pending: serde_json::Value,
            jobs_started: u64,
            jobs_completed: u64,
            bytes_completed: u64,
            acknowledgments: u64,
        }
        #[derive(Deserialize)]
        struct Save {
            engine: Engine,
        }
        let save: Save = serde_json::from_value(save.clone()).map_err(|error| format!("graphics state: {error}"))?;
        let engine = save.engine;
        Ok(Self {
            busy: engine.busy,
            command_pending: !engine.command_pending.is_null(),
            control: engine.regs.iter().find(|(offset, _)| *offset == 0x34).map_or(0, |(_, value)| *value),
            jobs_started: engine.jobs_started,
            jobs_completed: engine.jobs_completed,
            bytes_completed: engine.bytes_completed,
            acknowledgments: engine.acknowledgments,
        })
    }

    /// As `RenderFacts::probe` derives it.
    pub fn graphics(&self) -> Graphics {
        Graphics {
            idle: !self.busy && !self.command_pending && self.control == 0 && self.jobs_started == self.jobs_completed
                && self.acknowledgments >= self.jobs_completed,
            started: self.jobs_started,
            completed: self.jobs_completed,
            bytes_completed: self.bytes_completed,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReadinessSample {
    pub index: ReadinessIndex,
    /// Samples recorded before this one.
    pub preceding: u64,
    pub instructions: Instructions,
    /// The `0xc2314c` sample that opened the joined receive.
    pub receive: SampleIndex,
    pub kind: ReadinessKind,
    pub probe: Probe,
    pub tasks: TaskTableViews,
    pub ui: UiWorkViews,
    pub render: RenderViews,
    pub graphics: GraphicsState,
}

/// A classified-watch row overlapping a contract `code` range.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ApplicationWrite {
    /// Position in the watch's rows.
    pub row: u64,
    pub source: WriteKind,
    pub physical: u64,
    pub size: u64,
    pub core: Option<Core>,
    pub pc: Option<u32>,
}

#[derive(Deserialize)]
#[serde(rename_all = "kebab-case")]
enum Record {
    Sample(Sample),
    Readiness(ReadinessSample),
    Write(ApplicationWrite),
}

/// Serializes as `Record`.
#[derive(Serialize)]
#[serde(rename_all = "kebab-case")]
enum RecordRef<'a> {
    Sample(&'a Sample),
    Readiness(&'a ReadinessSample),
    Write(&'a ApplicationWrite),
}

/// The content of the evidence side file.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Evidence {
    pub samples: Vec<Sample>,
    pub readiness: Vec<ReadinessSample>,
    pub writes: Vec<ApplicationWrite>,
}

/// A single filename in the receipt's directory.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(try_from = "String", into = "String")]
pub struct FileName(String);

impl TryFrom<String> for FileName {
    type Error = String;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        if value.is_empty() || value == "." || value == ".." || value.contains('/') {
            Err("side file is not one filename".into())
        } else {
            Ok(Self(value))
        }
    }
}

impl From<FileName> for String {
    fn from(value: FileName) -> Self { value.0 }
}

impl FileName {
    pub fn as_str(&self) -> &str { &self.0 }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SideFile {
    pub path: FileName,
    pub sha256: Sha256,
    pub bytes: u64,
}

impl SideFile {
    /// Binds a file already in `directory`.
    pub fn bind(directory: &Path, name: &str) -> Result<Self, String> {
        let path = FileName::try_from(name.to_owned())?;
        let bytes = std::fs::read(directory.join(name)).map_err(|error| format!("{name}: {error}"))?;
        Ok(Self { path, sha256: Sha256::of(&bytes), bytes: bytes.len() as u64 })
    }

    /// Writes `bytes` as `name`, replacing any file or link of that name.
    pub fn replace(directory: &Path, name: &str, bytes: &[u8]) -> Result<Self, String> {
        let path = directory.join(name);
        if path.symlink_metadata().is_ok() {
            std::fs::remove_file(&path).map_err(|error| format!("{name}: {error}"))?;
        }
        crate::bridge::write(&path, bytes)?;
        Self::bind(directory, name)
    }

    /// The bytes, refused unless their length and digest are the binding's.
    pub fn read(&self, directory: &Path) -> Result<Vec<u8>, String> {
        let name = self.path.as_str();
        let file = std::fs::File::open(directory.join(name)).map_err(|error| format!("{name}: {error}"))?;
        let mut bytes = Vec::new();
        if self.bytes > SIDE_FILE_LIMIT {
            return Err(format!("{name} exceeds the side-file bound"));
        }
        file.take(SIDE_FILE_LIMIT + 1).read_to_end(&mut bytes).map_err(|error| format!("{name}: {error}"))?;
        if bytes.len() as u64 != self.bytes || Sha256::of(&bytes) != self.sha256 {
            return Err(format!("{name} differs from its binding"));
        }
        Ok(bytes)
    }
}

struct Hashing<W> {
    inner: W,
    hash: sha2::Sha256,
    bytes: u64,
}

impl<W: Write> Write for Hashing<W> {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        let written = self.inner.write(buffer)?;
        sha2::Digest::update(&mut self.hash, &buffer[..written]);
        self.bytes += written as u64;
        Ok(written)
    }
    fn flush(&mut self) -> std::io::Result<()> { self.inner.flush() }
}

/// Streams evidence records into the gzip side file.
pub struct EvidenceWriter {
    encoder: flate2::write::GzEncoder<Hashing<std::fs::File>>,
    line: Vec<u8>,
    name: String,
}

impl EvidenceWriter {
    pub fn create(directory: &Path, name: &str) -> Result<Self, String> {
        let file = std::fs::OpenOptions::new().write(true).create_new(true).open(directory.join(name))
            .map_err(|error| format!("{name}: {error}"))?;
        let hashing = Hashing { inner: file, hash: <sha2::Sha256 as sha2::Digest>::new(), bytes: 0 };
        Ok(Self { encoder: flate2::write::GzEncoder::new(hashing, flate2::Compression::default()), line: Vec::with_capacity(1 << 16), name: name.to_owned() })
    }

    fn append(&mut self, record: &RecordRef) -> Result<(), String> {
        self.line.clear();
        serde_json::to_writer(&mut self.line, record).map_err(|error| error.to_string())?;
        self.line.push(b'\n');
        self.encoder.write_all(&self.line).map_err(|error| format!("{}: {error}", self.name))
    }

    pub fn sample(&mut self, sample: &Sample) -> Result<(), String> { self.append(&RecordRef::Sample(sample)) }
    pub fn readiness(&mut self, sample: &ReadinessSample) -> Result<(), String> { self.append(&RecordRef::Readiness(sample)) }
    pub fn write(&mut self, write: &ApplicationWrite) -> Result<(), String> { self.append(&RecordRef::Write(write)) }

    pub fn finish(self) -> Result<SideFile, String> {
        let mut hashing = self.encoder.finish().map_err(|error| format!("{}: {error}", self.name))?;
        hashing.flush().map_err(|error| format!("{}: {error}", self.name))?;
        let digest: String = sha2::Digest::finalize(hashing.hash).iter().map(|byte| format!("{byte:02x}")).collect();
        Ok(SideFile { path: FileName::try_from(self.name)?, sha256: Sha256::try_from(digest)?, bytes: hashing.bytes })
    }
}

/// Rewrites the evidence side file in `directory`, replacing any file or link of that name.
pub fn write_evidence(directory: &Path, name: &str, evidence: &Evidence) -> Result<SideFile, String> {
    let path = directory.join(name);
    if path.symlink_metadata().is_ok() {
        std::fs::remove_file(&path).map_err(|error| format!("{name}: {error}"))?;
    }
    let mut writer = EvidenceWriter::create(directory, name)?;
    evidence.samples.iter().try_for_each(|sample| writer.sample(sample))?;
    evidence.readiness.iter().try_for_each(|sample| writer.readiness(sample))?;
    evidence.writes.iter().try_for_each(|write| writer.write(write))?;
    writer.finish()
}

pub fn read_evidence(directory: &Path, binding: &SideFile) -> Result<Evidence, String> {
    let name = binding.path.as_str();
    let compressed = {
        let file = std::fs::File::open(directory.join(name)).map_err(|error| format!("{name}: {error}"))?;
        let mut bytes = Vec::new();
        file.take(binding.bytes + 1).read_to_end(&mut bytes).map_err(|error| format!("{name}: {error}"))?;
        bytes
    };
    if compressed.len() as u64 != binding.bytes || Sha256::of(&compressed) != binding.sha256 {
        return Err(format!("{name} differs from its binding"));
    }
    let mut reader = std::io::BufReader::new(flate2::read::GzDecoder::new(compressed.as_slice()).take(EVIDENCE_LIMIT + 1));
    let mut evidence = Evidence::default();
    let mut line = Vec::new();
    let mut decoded = 0u64;
    loop {
        line.clear();
        let read = reader.read_until(b'\n', &mut line).map_err(|error| format!("{name}: {error}"))?;
        if read == 0 {
            break;
        }
        decoded += read as u64;
        if decoded > EVIDENCE_LIMIT {
            return Err(format!("{name} exceeds its decoded bound"));
        }
        match serde_json::from_slice(&line).map_err(|error| format!("{name}: {error}"))? {
            Record::Sample(sample) => evidence.samples.push(sample),
            Record::Readiness(sample) => evidence.readiness.push(sample),
            Record::Write(write) => evidence.writes.push(write),
        }
    }
    Ok(evidence)
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum InputPurpose {
    FirstStart,
    Ordinary,
    BackHandback,
    AfterBack,
    LiveView,
    FilmOpen,
    FilmReentry,
    EditorOpen,
    EditorCancel,
    EditorConfirm,
    UnhandledProbe,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RouteInput {
    pub key: RouteKey,
    pub purpose: InputPurpose,
    pub pressed: Instructions,
    pub released: Instructions,
    pub debounced: Instructions,
    pub settled: Option<Instructions>,
    pub before: Option<Probe>,
    pub after: Option<Probe>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Menu {
    pub menu: u32,
    pub setting: u32,
    pub cursor: u32,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct VisibleCapture {
    /// `None` for the drained frame after the route.
    pub input: Option<InputIndex>,
    pub instructions: Instructions,
    pub probe: Probe,
    pub menu: Option<Menu>,
    /// The controller record at `0x944580`.
    pub controller: RawView,
    pub controller74: ControllerStateView,
    pub png: SideFile,
    pub planes: SideFile,
    pub scanout: SideFile,
    pub graphics: SideFile,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Task1 {
    pub core: Core,
    /// The first entry with a committed context, where the stack scope began.
    pub entry: SampleIndex,
    pub domain: Domain,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FirstReady {
    pub readiness: ReadinessIndex,
    pub receive: SampleIndex,
    pub instructions: Instructions,
}

/// The first image5 producer publication hit and the watch rows recorded before it.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Publication {
    pub sample: SampleIndex,
    pub row: u64,
}

/// Digests of one contract `code` range read at publication and after the route.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CodeObservation {
    pub label: CodeLabel,
    pub publication: Option<Sha256>,
    pub end: Sha256,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ScopeObservation {
    pub summary: TraceStackScopeSummary,
    /// Every site added to the scope, in both spellings.
    pub sites: Vec<Guest>,
    /// Skipped hits drained over the run.
    pub skips: u64,
}

/// A queue service target or the packet copy's callee, armed at its first call.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DynamicTarget {
    pub pc: Pc,
    /// The role's image word at `pc` when it was armed.
    pub word: CodeWord,
    /// The calling sample.
    pub sample: SampleIndex,
}

/// The receipt's application section; samples, readiness samples and code writes are in `evidence`.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ApplicationReceipt {
    pub scenario: Scenario,
    pub evidence: SideFile,
    pub samples: u64,
    pub readiness: u64,
    pub writes: u64,
    pub task1: Option<Task1>,
    pub publication: Option<Publication>,
    pub code: Vec<CodeObservation>,
    pub scope: Option<ScopeObservation>,
    pub targets: Vec<DynamicTarget>,
    pub first_ready: Option<FirstReady>,
    /// Write classes the watch recorded.
    pub watch_classes: Vec<ByteRange>,
    pub inputs: Vec<RouteInput>,
    pub visible: Vec<VisibleCapture>,
}
