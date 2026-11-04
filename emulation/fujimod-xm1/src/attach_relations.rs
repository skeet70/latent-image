//! Attach-state source relations: the kernel timer replayed from its recorded accesses, boot-log timestamps paired
//! with the counter reads they store, and the LZ method-4 stream derived from its pinned compressed input.
use crate::attach_state::{
    AttachRuleRefusal, Allowance, AllowanceKind, BoardLeaf, BoardPointer, FileRecord, Hashing, SiteLabel, Space, Span, bound_file,
    hex, io,
};
use crate::composed_catalog::{Citation, Core, Pc, Role};
use crate::composed_receipt::Word;
use flate2::{Compression, read::GzDecoder, write::GzEncoder};
use fujimod_emulator::board::bus::Transaction;
use fujimod_emulator_devices::compression::method4::Stream;
use fujimod_emulator_devices::{Cx, Device, Host, Record};
use fujimod_emulator_exr2_system::timer::{self, Options, SystemTimer, Timers, Wiring};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
    io::{BufReader, BufWriter, Read, Write},
    path::Path,
};

pub const TRANSACTIONS_FILE: &str = "relation-transactions.bin.gz";
/// Rows of one transaction-watch drain.
pub const DRAIN_LIMIT: usize = 262_144;
pub const TRANSACTION_LIMIT: u64 = 16_777_216;
pub const LOG_WRITE_LIMIT: usize = 4096;
pub const SITE_LIMIT: usize = 32;
pub const LOG_SPAN_LIMIT: u64 = 4096;
pub const LZ_SOURCE_LIMIT: u64 = 64 << 20;
const TRANSACTION_FILE_LIMIT: u64 = 256 << 20;
const MAGIC: &[u8] = b"xm1-relation-tx\x01";
const ROW_BYTES: usize = 23;
const HOST_CORE: u8 = 0xff;
const TIMER_KIND: &str = "exr2-system-timers";
const LZ_KIND: &str = "exr2-lz";
const TIMER_WINDOW: u64 = 0x40;
const DRAM_END: u64 = 0x2000_0000;
const ALIAS_BASE: u64 = 0x4000_0000;
const ALIAS_END: u64 = 0x6000_0000;
const HISTORY: usize = 2048;
const PENDING: usize = 128;
/// Timer counter registers: the main counter, both polled channels and the application timer.
const COUNTERS: [u32; 4] = [0x00, 0x10, 0x20, 0x30];
const LZ_RESET: u32 = 0x00;
const LZ_TRIGGER: u32 = 0x04;
const LZ_SOURCE: u32 = 0x40;
const LZ_COUNT: u32 = 0x44;
const LZ_DESTINATION: u32 = 0x48;
const LZ_OUTPUT: u32 = 0x4c;
const LZ_START_JOB: u32 = 2;
const LZ_DEFAULT_COUNT: u64 = 0x80000;
/// Bytes of a DAT record header before its stream.
const RECORD_HEADER: u32 = 0x14;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TimerRelation {
    pub citation: Citation,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LzRelation {
    pub citation: Citation,
    #[serde(default)]
    pub submitted: Option<SubmittedWord>,
}

/// Physical, 4-aligned.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct WordAddress(pub u64);

/// A record's total length word, header included.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct RecordLength(pub u32);

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RoleLengths {
    pub retained: RecordLength,
    pub composed: RecordLength,
}

impl RoleLengths {
    pub fn of(&self, role: Role) -> RecordLength {
        match role {
            Role::Retained => self.retained,
            Role::Composed => self.composed,
        }
    }
}

/// The record-1 word counting the record-4 stream bytes submitted to the LZ device, with each role's record-4
/// length from its DAT.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SubmittedWord {
    pub address: WordAddress,
    pub citation: Citation,
    pub record_length: RoleLengths,
}

impl SubmittedWord {
    /// `None` when the word does not lie inside DRAM.
    pub fn span(&self) -> Option<Span> {
        let start = self.address.0;
        start.checked_add(4).filter(|end| *end <= DRAM_END).map(|end| Span { start, end })
    }
}

/// A counter read and the timestamp store that follows it, on one core in one role.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LogSite {
    pub label: SiteLabel,
    pub role: Role,
    pub core: Core,
    pub read_pc: Pc,
    pub store_pc: Pc,
    pub citation: Citation,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LogRelation {
    /// Physical.
    pub span: Span,
    pub sites: Vec<LogSite>,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Relations {
    pub timer: Option<TimerRelation>,
    pub log: Option<LogRelation>,
    pub lz: Option<LzRelation>,
}

impl Relations {
    pub fn any(&self) -> bool { self.timer.is_some() || self.log.is_some() || self.lz.is_some() }

    pub(crate) fn validate(&self, allowances: &[Allowance]) -> Result<(), AttachRuleRefusal> {
        if let Some(log) = &self.log {
            let span = log.span;
            if span.start % 4 != 0 || span.end % 4 != 0 || span.len() > LOG_SPAN_LIMIT || span.end > DRAM_END {
                return Err(AttachRuleRefusal::LogSpan);
            }
            if log.sites.len() > SITE_LIMIT {
                return Err(AttachRuleRefusal::Bound);
            }
            let mut labels = BTreeSet::new();
            let mut keys = BTreeSet::new();
            for site in &log.sites {
                if site.store_pc.0 != site.read_pc.0.wrapping_add(4) || !labels.insert(&site.label)
                    || !keys.insert((site.role, site.core, site.read_pc)) {
                    return Err(AttachRuleRefusal::LogSite(site.label.clone()));
                }
            }
            if let Some(allowance) = allowances.iter().find(|allowance| allowance.space == Space::Physical && allowance.span.intersects(span)) {
                return Err(AttachRuleRefusal::LogSpanOverlap(allowance.label.clone()));
            }
        }
        if let Some(submitted) = self.lz.as_ref().and_then(|lz| lz.submitted.as_ref()) {
            let Some(span) = submitted.span() else { return Err(AttachRuleRefusal::LzSubmittedWord) };
            let short = [submitted.record_length.retained, submitted.record_length.composed].iter().any(|length| length.0 < RECORD_HEADER);
            let overlaps = allowances.iter().any(|allowance| allowance.space == Space::Physical && allowance.span.intersects(span))
                || self.log.as_ref().is_some_and(|log| log.span.intersects(span));
            if span.start % 4 != 0 || short || overlaps {
                return Err(AttachRuleRefusal::LzSubmittedWord);
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(transparent)]
pub struct TimerState(pub SystemTimer);

impl Eq for TimerState {}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TransactionFile {
    pub file: FileRecord,
    pub rows: u64,
}

/// A CPU write into the log span before attach.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LogWrite {
    pub core: Core,
    pub pc: Pc,
    pub physical: u64,
    pub size: u8,
    pub value: Word,
    pub tick: u64,
}

/// Recorded from reset to the attach stop.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AttachSources {
    pub timer_reset: TimerState,
    pub transactions: TransactionFile,
    pub log_writes: Vec<LogWrite>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Access {
    Read,
    Write,
}

/// One bus access to the timer or LZ window.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TransactionRow {
    pub tick: u64,
    /// `None` for a host access.
    pub core: Option<Core>,
    pub pc: Pc,
    pub address: u32,
    pub size: u8,
    pub access: Access,
    pub accepted: bool,
    pub value: u32,
}

impl TransactionRow {
    pub fn from_transaction(row: &Transaction) -> Result<Self, String> {
        let core = if row.cpu == usize::MAX { None } else { Some(Core::from_index(row.cpu)?) };
        let access = match row.operation.as_ref() {
            "read" => Access::Read,
            "write" => Access::Write,
            _ => return Err("transaction operation is neither read nor write".into()),
        };
        Ok(Self {
            tick: row.tick,
            core,
            pc: Pc(u32::try_from(row.pc).map_err(|_| "transaction PC exceeds 32 bits")?),
            address: u32::try_from(row.address).map_err(|_| "transaction address exceeds 32 bits")?,
            size: row.size,
            access,
            accepted: row.accepted,
            value: u32::try_from(row.value).map_err(|_| "transaction value exceeds 32 bits")?,
        })
    }

    fn encode(&self) -> [u8; ROW_BYTES] {
        let mut bytes = [0u8; ROW_BYTES];
        bytes[0..8].copy_from_slice(&self.tick.to_le_bytes());
        bytes[8] = self.core.map_or(HOST_CORE, |core| core.index() as u8);
        bytes[9..13].copy_from_slice(&self.pc.0.to_le_bytes());
        bytes[13..17].copy_from_slice(&self.address.to_le_bytes());
        bytes[17] = self.size;
        bytes[18] = u8::from(self.access == Access::Write) | if self.accepted { 0 } else { 0x80 };
        bytes[19..23].copy_from_slice(&self.value.to_le_bytes());
        bytes
    }

    fn decode(bytes: &[u8; ROW_BYTES]) -> Result<Self, String> {
        let word = |at: usize| u32::from_le_bytes(bytes[at..at + 4].try_into().expect("four bytes"));
        let core = match bytes[8] {
            HOST_CORE => None,
            index => Some(Core::from_index(usize::from(index))?),
        };
        if bytes[18] & !0x81 != 0 {
            return Err("transaction kind has unknown bits".into());
        }
        Ok(Self {
            tick: u64::from_le_bytes(bytes[0..8].try_into().expect("eight bytes")),
            core,
            pc: Pc(word(9)),
            address: word(13),
            size: bytes[17],
            access: if bytes[18] & 1 != 0 { Access::Write } else { Access::Read },
            accepted: bytes[18] & 0x80 == 0,
            value: word(19),
        })
    }
}

/// Appends transaction-watch drains to `relation-transactions.bin.gz`.
pub struct TransactionWriter {
    encoder: GzEncoder<Hashing<BufWriter<File>>>,
    rows: u64,
}

impl TransactionWriter {
    pub fn create(directory: &Path) -> Result<Self, String> {
        let path = directory.join(TRANSACTIONS_FILE);
        let file = File::create_new(&path).map_err(|error| format!("{}: {error}", path.display()))?;
        let mut encoder = GzEncoder::new(Hashing { inner: BufWriter::new(file), hasher: sha2::Digest::new(), bytes: 0 }, Compression::fast());
        encoder.write_all(MAGIC).map_err(io)?;
        Ok(Self { encoder, rows: 0 })
    }

    pub fn append(&mut self, rows: &[Transaction]) -> Result<(), String> {
        for row in rows {
            self.push(&TransactionRow::from_transaction(row)?)?;
        }
        Ok(())
    }

    pub fn push(&mut self, row: &TransactionRow) -> Result<(), String> {
        if self.rows >= TRANSACTION_LIMIT {
            return Err("relation transaction bound exhausted".into());
        }
        self.encoder.write_all(&row.encode()).map_err(io)?;
        self.rows += 1;
        Ok(())
    }

    pub fn finish(mut self) -> Result<TransactionFile, String> {
        self.encoder.write_all(&self.rows.to_le_bytes()).map_err(io)?;
        let mut hashing = self.encoder.finish().map_err(io)?;
        hashing.flush().map_err(io)?;
        if hashing.bytes > TRANSACTION_FILE_LIMIT {
            return Err("relation transaction file exceeds its bound".into());
        }
        Ok(TransactionFile {
            file: FileRecord { path: TRANSACTIONS_FILE.into(), sha256: hex(hashing.hasher)?, compressed_bytes: hashing.bytes },
            rows: self.rows,
        })
    }
}

/// Visits every row of a bound transaction file in order; ticks never decrease.
pub fn read_transactions(directory: &Path, file: &TransactionFile, mut visit: impl FnMut(u64, TransactionRow)) -> Result<(), String> {
    let path = bound_file(directory, &file.file, TRANSACTION_FILE_LIMIT)?;
    let mut decoder = GzDecoder::new(BufReader::new(File::open(&path).map_err(|error| format!("{}: {error}", path.display()))?));
    let mut magic = [0u8; MAGIC.len()];
    decoder.read_exact(&mut magic).map_err(io)?;
    if magic != MAGIC {
        return Err("relation transaction file has the wrong magic".into());
    }
    let mut previous = 0u64;
    let mut buffer = [0u8; ROW_BYTES];
    for index in 0..file.rows {
        decoder.read_exact(&mut buffer).map_err(io)?;
        let row = TransactionRow::decode(&buffer)?;
        if row.tick < previous {
            return Err("relation transaction ticks decrease".into());
        }
        previous = row.tick;
        visit(index, row);
    }
    let mut count = [0u8; 8];
    decoder.read_exact(&mut count).map_err(io)?;
    if u64::from_le_bytes(count) != file.rows || decoder.read(&mut [0u8; 1]).map_err(io)? != 0 {
        return Err("relation transaction file row count differs from its receipt".into());
    }
    Ok(())
}

/// The timer device's state and the timer and LZ windows, from `Board::state()` at reset.
pub fn reset_sources(board: &Value) -> Result<(TimerState, Vec<(u64, u64)>), String> {
    let timer = device(board, TIMER_KIND).map_err(|_| "board lacks one kernel timer device")?;
    let lz = device(board, LZ_KIND).map_err(|_| "board lacks one LZ device")?;
    let state: SystemTimer = serde_json::from_value(board["bus"]["devices"][timer]["state"].clone())
        .map_err(|error| format!("timer state: {error}"))?;
    let window = |index: usize| -> Result<(u64, u64), String> {
        let windows = board["bus"]["devices"][index]["windows"].as_array().ok_or("device lacks windows")?;
        let [only] = windows.as_slice() else { return Err("device has more than one window".into()) };
        let base = only["base"].as_u64().ok_or("window base")?;
        let size = only["size"].as_u64().ok_or("window size")?;
        Ok((base, base.checked_add(size).ok_or("window overflows")?))
    };
    let timer_window = window(timer)?;
    if timer_window != (timer::BASE, timer::BASE + TIMER_WINDOW) {
        return Err("kernel timer window differs from FujiMod's".into());
    }
    Ok((TimerState(state), vec![timer_window, window(lz)?]))
}

/// The CPU writes of a pre-attach watch that land in `span`; any other writer refuses.
pub fn log_writes(records: &Value, span: Span) -> Result<Vec<LogWrite>, String> {
    let rows = records.as_array().ok_or("watch records are not an array")?;
    let mut writes = Vec::new();
    for row in rows {
        let physical = row["physical"].as_u64().ok_or("watch row lacks a physical address")?;
        let size = row["size"].as_u64().ok_or("watch row lacks a size")?;
        if !(physical < span.end && span.start < physical.saturating_add(size)) {
            continue;
        }
        if row["source"] != "cpu-write" {
            return Err("a non-CPU writer reached the log span".into());
        }
        if writes.len() == LOG_WRITE_LIMIT {
            return Err("log write bound exhausted".into());
        }
        let number = |key: &str| row[key].as_u64().ok_or_else(|| format!("log write lacks {key}"));
        writes.push(LogWrite {
            core: Core::from_index(number("core")? as usize)?,
            pc: Pc(u32::try_from(number("pc")?).map_err(|_| "log write PC exceeds 32 bits")?),
            physical,
            size: u8::try_from(size).map_err(|_| "log write size")?,
            value: Word(u32::try_from(number("value")?).map_err(|_| "log write value exceeds 32 bits")?),
            tick: number("tick")?,
        });
    }
    Ok(writes)
}

/// One element of a core's timer access sequence; counter reads outside log sites are not elements.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Token {
    Write { offset: u32, value: Word },
    Read { offset: u32, value: Word },
    /// A log-site read, as the slot its store wrote, the value word stored after it and the number of main-timer
    /// register writes (`+0x00`-`+0x0c`) from any core before it.
    LogEntry { slot: u64, value: Word, main_writes: u64 },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum RelationFailure {
    Device { device: DeviceKind },
    Transactions { reason: String },
    HostAccess { row: u64 },
    RefusedAccess { row: u64 },
    TimerResetDiffers,
    TimerReplayRead { row: u64, recorded: Word, model: Word },
    TimerReplayRefused { row: u64, reason: String },
    TimerSettle { model: u64, captured: u64 },
    TimerState { leaf: BoardLeaf },
    TimerSequence { core: Core, index: usize, retained: Option<Token>, composed: Option<Token> },
    LogWriteShape { index: usize },
    LogSiteRead { site: SiteLabel, row: u64 },
    LogUnpaired { site: SiteLabel, reads: u64, stores: u64 },
    LogOrder { site: SiteLabel, k: u64 },
    LogValue { site: SiteLabel, k: u64, read: Word, stored: Word },
    LogEntryValue { site: SiteLabel, k: u64 },
    LogForeignSite { core: Core, pc: Pc },
    LogCaptured { address: u64 },
    LogTimestampWords,
    LzSource { job: usize },
    /// The captured pending job does not parse as an LZ or MOVRSZ job.
    LzPendingJob,
    LzContiguity { job: usize },
    LzSourceUnpinned { start: u64, end: u64 },
    LzDecode { reason: String },
    LzStream { leaf: BoardLeaf },
    /// The submitted word differs from the started jobs' total or from the record length less its header.
    LzSubmitted { captured: Word, started: u64, stream: u64 },
    /// Every stream byte was started but the word lacks the last job: the record-1 drain step (boot-40048 0x3dc0)
    /// submitted it without storing the word.
    LzSubmittedDrain { captured: Word, started: u64, last: u64 },
    /// A descriptor register the scan derives differs from the captured one; `captured` is absent when the board
    /// lacks a numeric word there.
    LzRegister { leaf: BoardLeaf, derived: u64, captured: Option<u64> },
    RelationOverlap { pointer: BoardPointer },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum DeviceKind {
    KernelTimer,
    Lz,
}

fn device(board: &Value, kind: &str) -> Result<usize, ()> {
    let devices = board["bus"]["devices"].as_array().ok_or(())?;
    let mut found = devices.iter().enumerate().filter(|(_, device)| device["kind"] == kind).map(|(index, _)| index);
    match (found.next(), found.next()) {
        (Some(index), None) => Ok(index),
        _ => Err(()),
    }
}

fn physical(address: u64) -> Option<u64> {
    if (ALIAS_BASE..ALIAS_END).contains(&address) {
        Some(address - ALIAS_BASE)
    } else if address < DRAM_END {
        Some(address)
    } else {
        None
    }
}

/// A host for replaying the timer outside a board: it reports `now` and ignores interrupt effects.
#[derive(Default)]
struct Clock {
    now: u64,
}

impl Host for Clock {
    fn now(&self) -> u64 { self.now }
    fn schedule(&mut self, _: usize, _: u64, _: u64) {}
    fn set_irq(&mut self, _: u32, _: bool) {}
    fn raise_irq(&mut self, _: usize, _: u32) -> Result<(), String> { Ok(()) }
    fn ram_read(&self, _: u64, _: &mut [u8]) -> Result<(), String> { Err("the timer read RAM".into()) }
    fn ram_write(&mut self, _: u64, _: &[u8]) -> Result<(), String> { Err("the timer wrote RAM".into()) }
    fn record(&mut self, _: Record) {}
}

struct Replay {
    model: Timers,
    clock: Clock,
    device: usize,
}

impl Replay {
    fn new(reset: &SystemTimer, device: usize) -> Result<Self, String> {
        let controllers = reset.targets.iter()
            .map(|&core| (usize::from(core), fujimod_emulator_devices::exr2::irq::Inputs::default())).collect();
        let mut model = Timers::new(Options { main_targets: reset.targets.clone() }, Wiring { controllers, start_tick: reset.last_tick })?;
        let saved = serde_json::to_value(reset).map_err(|error| error.to_string())?;
        model.restore(&saved)?;
        if model.save() != saved {
            return Err("timer reset state does not round-trip".into());
        }
        Ok(Self { model, clock: Clock::default(), device })
    }

    /// The model's value for a read; `None` for a write.
    fn access(&mut self, row: &TransactionRow, core: Core) -> Result<Option<u32>, String> {
        self.clock.now = row.tick;
        let mut cx = Cx::new(&mut self.clock, core.index(), u64::from(row.pc.0), self.device);
        let value = match row.access {
            Access::Read => Some(self.model.read(&mut cx, u64::from(row.address), row.size)),
            Access::Write => {
                self.model.write(&mut cx, u64::from(row.address), row.size, u64::from(row.value));
                None
            }
        };
        if cx.is_unmodeled() {
            return Err("unmodeled timer access".into());
        }
        if let Some(reason) = cx.take_refusal() {
            return Err(reason);
        }
        value.map(|value| u32::try_from(value).map_err(|_| "timer read exceeds 32 bits".to_owned())).transpose()
    }

    fn last_tick(&self) -> u64 { self.model.save()["last_tick"].as_u64().unwrap_or(0) }

    /// Settles at `tick`, the captured `last_tick`, when a modelled deadline lies at or before it.
    fn settle(&mut self, tick: u64, attach: u64) -> Result<(), RelationFailure> {
        let model = self.last_tick();
        if tick == model {
            return Ok(());
        }
        let due = self.model.next_deadline(model).is_some_and(|deadline| deadline <= tick);
        if tick < model || tick > attach || !due {
            return Err(RelationFailure::TimerSettle { model, captured: tick });
        }
        self.clock.now = tick;
        let mut cx = Cx::new(&mut self.clock, usize::MAX, 0, self.device);
        self.model.advance(&mut cx);
        if cx.take_refusal().is_some() {
            return Err(RelationFailure::TimerSettle { model, captured: tick });
        }
        Ok(())
    }
}

pub(crate) struct RoleInput<'a> {
    pub role: Role,
    pub sources: &'a AttachSources,
    pub directory: &'a Path,
    pub board: &'a Value,
    pub allowances: &'a [Allowance],
}

/// An LZ derivation waiting for its role's snapshot bytes `[start, end)`.
#[derive(Clone, Debug)]
pub(crate) struct LzPlan {
    pub device: usize,
    pub start: u64,
    pub end: u64,
    /// A stream reset was written after reset of the board.
    pub reset: bool,
    pub captured: Value,
    /// Total count of the jobs started since the last reset, a pending one included.
    pub started: u64,
    /// Count of the last started job; 0 without one.
    pub last: u64,
    pub registers: RegisterPlan,
}

/// What the scan fixes of the count, destination and output-count registers.
#[derive(Clone, Debug)]
pub(crate) struct RegisterPlan {
    /// The board's `regs` array.
    pub captured: Value,
    /// The count register at the last start, unless a later write replaced it.
    pub count: Option<Word>,
    /// The destination at the first start since the last stream reset; absent if a destination write followed the
    /// last start.
    pub first_destination: Option<Word>,
    pub output: OutputRule,
    /// Span bytes fed before the last started job: the whole span when that job is pending.
    pub before_last: u64,
}

/// The output-count register: 0 after a start that has not completed or after a stream reset, else the last fed
/// job's output (lz.rs:280, 629, 686).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum OutputRule {
    Zero,
    LastFed,
    /// No job start and no stream reset was recorded.
    Underived,
}

/// The derived descriptor registers, absent where the scan does not fix one.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct LzRegisters {
    pub count: Option<Word>,
    pub destination: Option<Word>,
    pub output: Option<Word>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct TimestampWord {
    pub address: u64,
    pub value: Word,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct LzSummary {
    pub start: u64,
    pub end: u64,
    pub literals: u64,
    pub matches: u64,
    pub submitted: Option<Word>,
    pub registers: LzRegisters,
}

/// One role's relation results before the memory stream.
pub(crate) struct RoleRelations {
    pub role: Role,
    pub rows: u64,
    pub timer_device: Option<usize>,
    pub timer: Option<Result<(), RelationFailure>>,
    pub sequence: [Vec<Token>; 2],
    pub log: Option<Result<Vec<TimestampWord>, RelationFailure>>,
    pub lz: Option<Result<LzPlan, RelationFailure>>,
}

fn first<T>(slot: &mut Option<Result<T, RelationFailure>>, failure: RelationFailure) {
    if !matches!(slot, Some(Err(_))) {
        *slot = Some(Err(failure));
    }
}

fn failed<T>(slot: &Option<Result<T, RelationFailure>>) -> bool { matches!(slot, Some(Err(_))) }

struct SitePairing<'a> {
    site: &'a LogSite,
    stores: Vec<usize>,
    reads: u64,
    previous_store_tick: u64,
}

/// Evaluates every relation `relations` names for one role; nothing here reads the memory snapshot.
pub(crate) fn evaluate(relations: &Relations, input: &RoleInput) -> RoleRelations {
    let role = input.role;
    let mut out = RoleRelations {
        role, rows: input.sources.transactions.rows, timer_device: None,
        timer: relations.timer.as_ref().map(|_| Ok(())), sequence: [Vec::new(), Vec::new()],
        log: relations.log.as_ref().map(|_| Ok(Vec::new())), lz: None,
    };
    let timer_device = device(input.board, TIMER_KIND).ok();
    out.timer_device = timer_device;
    let mut replay = None;
    if relations.timer.is_some() {
        match timer_device {
            None => first(&mut out.timer, RelationFailure::Device { device: DeviceKind::KernelTimer }),
            Some(index) => match Replay::new(&input.sources.timer_reset.0, index) {
                Ok(model) => replay = Some(model),
                Err(reason) => first(&mut out.timer, RelationFailure::TimerReplayRefused { row: 0, reason }),
            },
        }
    }
    let lz_device = relations.lz.as_ref().map(|_| device(input.board, LZ_KIND));
    let lz_base = lz_device.and_then(|index| index.ok())
        .and_then(|index| input.board["bus"]["devices"][index]["windows"][0]["base"].as_u64());
    let writes = &input.sources.log_writes;
    let mut pairings: Vec<SitePairing> = Vec::new();
    let mut declared_reads: BTreeSet<u32> = BTreeSet::new();
    if let Some(log) = &relations.log {
        for (index, write) in writes.iter().enumerate() {
            if write.size != 4 || write.physical % 4 != 0 || !log.span.contains(Span { start: write.physical, end: write.physical + 4 }) {
                first(&mut out.log, RelationFailure::LogWriteShape { index });
            }
        }
        declared_reads = log.sites.iter().map(|site| site.read_pc.0).collect();
        for site in log.sites.iter().filter(|site| site.role == role) {
            let stores = writes.iter().enumerate()
                .filter(|(_, write)| write.core == site.core && write.pc == site.store_pc).map(|(index, _)| index).collect();
            pairings.push(SitePairing { site, stores, reads: 0, previous_store_tick: 0 });
        }
    }
    let mut lz = LzScan::default();
    let mut main_writes = 0u64;
    let timer_range = timer::BASE..timer::BASE + TIMER_WINDOW;
    let scan = read_transactions(input.directory, &input.sources.transactions, |index, row| {
        let address = u64::from(row.address);
        let in_timer = timer_range.contains(&address);
        let in_lz = lz_base.is_some_and(|base| (base..base + 0x100).contains(&address));
        let Some(core) = row.core else {
            if in_timer && relations.timer.is_some() { first(&mut out.timer, RelationFailure::HostAccess { row: index }); }
            if in_lz { first(&mut out.lz, RelationFailure::HostAccess { row: index }); }
            return;
        };
        if !row.accepted {
            if in_timer && relations.timer.is_some() { first(&mut out.timer, RelationFailure::RefusedAccess { row: index }); }
            if in_lz { first(&mut out.lz, RelationFailure::RefusedAccess { row: index }); }
            return;
        }
        if in_timer {
            let offset = row.address - timer::BASE as u32;
            if let Some(model) = replay.as_mut().filter(|_| !failed(&out.timer)) {
                match model.access(&row, core) {
                    Ok(Some(value)) if value != row.value =>
                        first(&mut out.timer, RelationFailure::TimerReplayRead { row: index, recorded: Word(row.value), model: Word(value) }),
                    Ok(_) => {}
                    Err(reason) => first(&mut out.timer, RelationFailure::TimerReplayRefused { row: index, reason }),
                }
            }
            let mut entry = None;
            if row.access == Access::Read {
                if let Some(pairing) = pairings.iter_mut().find(|pairing| pairing.site.core == core && pairing.site.read_pc == row.pc) {
                    let label = pairing.site.label.clone();
                    let k = pairing.reads;
                    pairing.reads += 1;
                    if offset != 0 || row.size != 4 {
                        first(&mut out.log, RelationFailure::LogSiteRead { site: label, row: index });
                    } else if let Some(&store) = pairing.stores.get(k as usize) {
                        let write = writes[store];
                        if write.tick < row.tick || row.tick < pairing.previous_store_tick {
                            first(&mut out.log, RelationFailure::LogOrder { site: label.clone(), k });
                        }
                        pairing.previous_store_tick = write.tick;
                        if write.value.0 != row.value {
                            first(&mut out.log, RelationFailure::LogValue { site: label.clone(), k, read: Word(row.value), stored: write.value });
                        }
                        match writes[store + 1..].iter().find(|next| next.core == core && next.physical == write.physical + 4) {
                            Some(next) => entry = Some(Token::LogEntry { slot: write.physical, value: next.value, main_writes }),
                            None => first(&mut out.log, RelationFailure::LogEntryValue { site: label, k }),
                        }
                    }
                } else if offset == 0 && declared_reads.contains(&row.pc.0) {
                    first(&mut out.log, RelationFailure::LogForeignSite { core, pc: row.pc });
                }
            }
            if relations.timer.is_some() {
                let token = match row.access {
                    Access::Write => Some(Token::Write { offset, value: Word(row.value) }),
                    Access::Read if COUNTERS.contains(&offset) => entry,
                    Access::Read => Some(Token::Read { offset, value: Word(row.value) }),
                };
                out.sequence[core.index()].extend(token);
            }
            if row.access == Access::Write && offset < 0x10 {
                main_writes += 1;
            }
        }
        if in_lz && row.access == Access::Write {
            lz.write(row.address - lz_base.unwrap_or(0) as u32, row.value);
        }
    });
    if let Err(reason) = scan {
        let failure = RelationFailure::Transactions { reason };
        if out.timer.is_some() { first(&mut out.timer, failure.clone()); }
        if out.log.is_some() { first(&mut out.log, failure.clone()); }
        out.lz = Some(Err(failure));
        return out;
    }
    if let (Some(model), Some(index)) = (replay.as_mut(), timer_device) {
        if !failed(&out.timer) {
            let captured = &input.board["bus"]["devices"][index]["state"];
            let attach = input.board["timeline"]["now"].as_u64().unwrap_or(0);
            match captured["last_tick"].as_u64() {
                None => first(&mut out.timer, RelationFailure::TimerState { leaf: BoardLeaf(format!("/bus/devices/{index}/state/last_tick")) }),
                Some(tick) => if let Err(failure) = model.settle(tick, attach) { first(&mut out.timer, failure) },
            }
            if !failed(&out.timer) {
                let mut leaves = Vec::new();
                crate::attach_state::differing_leaves(&format!("/bus/devices/{index}/state"), Some(&model.model.save()), Some(captured), &mut leaves);
                if let Some(leaf) = leaves.into_iter().next() {
                    first(&mut out.timer, RelationFailure::TimerState { leaf: BoardLeaf(leaf) });
                }
            }
        }
    }
    if relations.log.is_some() && !failed(&out.log) {
        for pairing in &pairings {
            if pairing.reads != pairing.stores.len() as u64 {
                first(&mut out.log, RelationFailure::LogUnpaired {
                    site: pairing.site.label.clone(), reads: pairing.reads, stores: pairing.stores.len() as u64,
                });
            }
        }
        let site_stores: BTreeSet<usize> = pairings.iter().flat_map(|pairing| pairing.stores.iter().copied()).collect();
        let mut last: BTreeMap<u64, (bool, Word)> = BTreeMap::new();
        for (index, write) in writes.iter().enumerate() {
            last.insert(write.physical, (site_stores.contains(&index), write.value));
        }
        if !failed(&out.log) {
            out.log = Some(Ok(last.into_iter().filter(|(_, (site, _))| *site)
                .map(|(address, (_, value))| TimestampWord { address, value }).collect()));
        }
    }
    if let Some(found) = lz_device {
        if failed(&out.lz) {
            return out;
        }
        out.lz = Some(match found {
            Err(()) => Err(RelationFailure::Device { device: DeviceKind::Lz }),
            Ok(index) => lz_plan(index, input, lz),
        });
    }
    out
}

/// One role's LZ descriptor writes and stream events.
#[derive(Default)]
struct LzScan {
    source: u64,
    count: Option<Word>,
    destination: Option<Word>,
    /// Jobs started since the last stream reset: physical-bus source and count, 0 read as `LZ_DEFAULT_COUNT`.
    jobs: Vec<(u64, u64)>,
    /// The count and destination registers at each start in `jobs`.
    starts: Vec<(Option<Word>, Option<Word>)>,
    reset: bool,
    /// The last stream event was a reset.
    reset_last: bool,
    /// A count or destination write followed the last start.
    rewritten: (bool, bool),
}

impl LzScan {
    fn write(&mut self, offset: u32, value: u32) {
        match (offset, value) {
            (LZ_SOURCE, value) => self.source = u64::from(value),
            (LZ_COUNT, value) => {
                self.count = Some(Word(value));
                self.rewritten.0 = true;
            }
            (LZ_DESTINATION, value) => {
                self.destination = Some(Word(value));
                self.rewritten.1 = true;
            }
            (LZ_TRIGGER, LZ_START_JOB) => {
                let count = match self.count { Some(Word(0)) | None => LZ_DEFAULT_COUNT, Some(Word(count)) => u64::from(count) };
                self.jobs.push((self.source, count));
                self.starts.push((self.count, self.destination));
                self.reset_last = false;
                self.rewritten = (false, false);
            }
            (LZ_RESET, 0) => {
                self.jobs.clear();
                self.starts.clear();
                self.reset = true;
                self.reset_last = true;
            }
            _ => {}
        }
    }
}

/// The LZ device's job kinds, as FujiMod's `lz.rs` saves them.
#[derive(Deserialize)]
enum JobKind {
    Lz,
    Resize,
}

#[derive(Deserialize)]
struct PendingJob {
    job: JobKind,
}

fn lz_plan(index: usize, input: &RoleInput, scan: LzScan) -> Result<LzPlan, RelationFailure> {
    let saved = &input.board["bus"]["devices"][index]["state"]["state"];
    let pending: Option<PendingJob> = serde_json::from_value(saved["pending"].clone()).map_err(|_| RelationFailure::LzPendingJob)?;
    let LzScan { mut jobs, starts, reset, reset_last, rewritten, .. } = scan;
    let started = jobs.iter().map(|(_, count)| count).sum();
    let last = jobs.last().map_or(0, |(_, count)| *count);
    let pending_lz = matches!(pending, Some(PendingJob { job: JobKind::Lz }));
    if pending_lz {
        jobs.pop();
    }
    let (start, end) = job_span(&jobs)?;
    if end - start > LZ_SOURCE_LIMIT {
        return Err(RelationFailure::LzSourceUnpinned { start, end });
    }
    if end > start {
        let span = Span { start, end };
        let pinned = input.allowances.iter()
            .any(|allowance| allowance.space == Space::Physical && matches!(allowance.kind, AllowanceKind::Copy { .. }) && allowance.span.contains(span));
        if !pinned {
            return Err(RelationFailure::LzSourceUnpinned { start, end });
        }
    }
    let output = if pending_lz || reset_last {
        OutputRule::Zero
    } else if starts.is_empty() {
        OutputRule::Underived
    } else {
        OutputRule::LastFed
    };
    let registers = RegisterPlan {
        captured: saved["regs"].clone(),
        count: if rewritten.0 { None } else { starts.last().and_then(|(count, _)| *count) },
        first_destination: if rewritten.1 || starts.is_empty() { None } else { starts.first().and_then(|(_, destination)| *destination) },
        output,
        before_last: if pending_lz { end - start } else { (end - start).saturating_sub(last) },
    };
    Ok(LzPlan { device: index, start, end, reset, captured: saved["stream"].clone(), started, last, registers })
}

/// Checks one role's captured submitted word against its started jobs and its record-4 length.
pub(crate) fn check_submitted(plan: &LzPlan, captured: u32, length: RecordLength) -> Result<(), RelationFailure> {
    let stream = u64::from(length.0 - RECORD_HEADER);
    let word = u64::from(captured);
    if word == plan.started && word == stream {
        return Ok(());
    }
    if plan.started == stream && plan.last > 0 && word + plan.last == plan.started {
        return Err(RelationFailure::LzSubmittedDrain { captured: Word(captured), started: plan.started, last: plan.last });
    }
    Err(RelationFailure::LzSubmitted { captured: Word(captured), started: plan.started, stream })
}

/// The physical source `[start, end)` the fed jobs consumed, each starting where the previous one ended.
fn job_span(jobs: &[(u64, u64)]) -> Result<(u64, u64), RelationFailure> {
    let mut start = None;
    let mut end = 0u64;
    for (job, (source, count)) in jobs.iter().enumerate() {
        let begin = physical(*source).ok_or(RelationFailure::LzSource { job })?;
        let finish = begin.checked_add(*count).filter(|finish| *finish <= DRAM_END).ok_or(RelationFailure::LzSource { job })?;
        match start {
            None => start = Some(begin),
            Some(_) if begin != end => return Err(RelationFailure::LzContiguity { job }),
            Some(_) => {}
        }
        end = finish;
    }
    Ok((start.unwrap_or(0), end))
}

fn stream_leaf(device: usize, field: &str) -> BoardLeaf { BoardLeaf(format!("/bus/devices/{device}/state/state/stream/{field}")) }

fn register_leaf(device: usize, offset: u32) -> BoardLeaf { BoardLeaf(format!("/bus/devices/{device}/state/state/regs/{}", offset / 4)) }

/// Checks the registers `plan` derives against the captured `regs`; returns the derived values and their leaves.
fn derive_registers(plan: &LzPlan, before_last: u64, produced: u64) -> Result<(LzRegisters, Vec<String>), RelationFailure> {
    let registers = &plan.registers;
    let output = match registers.output {
        OutputRule::Zero => Some(0),
        OutputRule::LastFed => Some(produced - before_last),
        OutputRule::Underived => None,
    };
    let derived = [
        (LZ_COUNT, registers.count.map(|count| u64::from(count.0))),
        (LZ_DESTINATION, registers.first_destination.map(|first| u64::from(first.0) + before_last)),
        (LZ_OUTPUT, output),
    ];
    let mut words = [None; 3];
    let mut leaves = Vec::new();
    for (slot, (offset, value)) in derived.into_iter().enumerate() {
        let Some(value) = value else { continue };
        let leaf = register_leaf(plan.device, offset);
        let captured = registers.captured[(offset / 4) as usize].as_u64();
        match u32::try_from(value) {
            Ok(word) if captured == Some(value) => words[slot] = Some(Word(word)),
            _ => return Err(RelationFailure::LzRegister { leaf, derived: value, captured }),
        }
        leaves.push(leaf.0);
    }
    let [count, destination, output] = words;
    Ok((LzRegisters { count, destination, output }, leaves))
}

/// Feeds `bytes` to a fresh method-4 stream and checks it and the derived descriptor registers against the capture;
/// returns the summary and the captured leaves the derivation fixes.
pub(crate) fn derive_lz(plan: &LzPlan, bytes: &[u8]) -> Result<(LzSummary, Vec<String>), RelationFailure> {
    let mut stream = Stream::default();
    let mut out = [0u8; PENDING];
    let mut pending_written = 0usize;
    let mut before_last = 0u64;
    for (index, byte) in bytes.iter().enumerate() {
        if index as u64 == plan.registers.before_last {
            before_last = stream.produced();
        }
        pending_written = pending_written.max(stream.pending_bytes() + 1);
        stream.feed(std::slice::from_ref(byte), &mut out).map_err(|reason| RelationFailure::LzDecode { reason })?;
    }
    if plan.registers.before_last == bytes.len() as u64 {
        before_last = stream.produced();
    }
    let derived = serde_json::to_value(&stream).map_err(|error| RelationFailure::LzDecode { reason: error.to_string() })?;
    let captured = &plan.captured;
    for field in ["produced", "literals", "matches", "pending_len", "failed"] {
        if derived[field] != captured[field] {
            return Err(RelationFailure::LzStream { leaf: stream_leaf(plan.device, field) });
        }
    }
    let produced = derived["produced"].as_u64().unwrap_or(0) as usize;
    let history = if plan.reset { produced.min(HISTORY) } else { HISTORY };
    let pending = if plan.reset { pending_written.min(PENDING) } else { PENDING };
    for (field, written) in [("history", history), ("pending", pending)] {
        for position in 0..written {
            if derived[field][position] != captured[field][position] {
                return Err(RelationFailure::LzStream { leaf: stream_leaf(plan.device, &format!("{field}/{position}")) });
            }
        }
    }
    let (registers, register_leaves) = derive_registers(plan, before_last, stream.produced())?;
    let leaves = ["literals", "matches", "pending_len"].into_iter().map(|field| stream_leaf(plan.device, field).0)
        .chain((0..pending).map(|position| stream_leaf(plan.device, &format!("pending/{position}")).0)).chain(register_leaves).collect();
    let number = |field: &str| derived[field].as_u64().unwrap_or(0);
    Ok((LzSummary { start: plan.start, end: plan.end, literals: number("literals"), matches: number("matches"), submitted: None, registers }, leaves))
}

/// Timer leaves the replay fixes.
pub(crate) fn timer_leaves(device: usize) -> Vec<String> {
    let base = format!("/bus/devices/{device}/state");
    ["count", "phase", "last_tick", "polled/0/count", "polled/0/phase", "polled/1/count", "polled/1/phase", "application/count", "application/phase"]
        .into_iter().map(|field| format!("{base}/{field}")).collect()
}

/// LZ leaves any LZ relation could fix, for the board-entry overlap check.
pub(crate) fn lz_leaf_prefixes(device: usize) -> Vec<String> {
    ["literals", "matches", "pending_len", "pending"].into_iter().map(|field| stream_leaf(device, field).0)
        .chain([LZ_COUNT, LZ_DESTINATION, LZ_OUTPUT].into_iter().map(|offset| register_leaf(device, offset).0)).collect()
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct RoleRelationSummary {
    pub role: Role,
    pub transactions: u64,
    pub timestamps: Vec<TimestampWord>,
    pub lz: Option<LzSummary>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Literal `abc`, an overlapping match repeating it twice, then literal `xy`.
    const COMPRESSED: [u8; 9] = [3, b'a', b'b', b'c', 0x80, 0x36, 2, b'x', b'y'];

    fn captured(bytes: &[u8], reset: bool) -> Value {
        let mut stream = Stream::default();
        let mut out = [0u8; PENDING];
        if reset {
            stream.feed(&[5, 9, 9, 9, 9, 9], &mut out).unwrap();
            stream.reset();
        }
        for byte in bytes {
            stream.feed(std::slice::from_ref(byte), &mut out).unwrap();
        }
        serde_json::to_value(&stream).unwrap()
    }

    fn plan(captured: Value, reset: bool) -> LzPlan {
        let count = COMPRESSED.len() as u64;
        let registers = RegisterPlan { captured: Value::Null, count: None, first_destination: None, output: OutputRule::Underived, before_last: 0 };
        LzPlan { device: 29, start: 0x1000, end: 0x1000 + count, reset, captured, started: count, last: count, registers }
    }

    #[test]
    fn the_stream_derived_from_its_input_matches_the_capture() {
        let (summary, leaves) = derive_lz(&plan(captured(&COMPRESSED, false), false), &COMPRESSED).unwrap();
        assert_eq!((summary.literals, summary.matches), (2, 1));
        assert!(leaves.contains(&"/bus/devices/29/state/state/stream/literals".to_owned()));
        assert_eq!(leaves.iter().filter(|leaf| leaf.contains("/pending/")).count(), PENDING);
    }

    #[test]
    fn a_changed_tally_or_input_byte_refuses() {
        let mut changed = captured(&COMPRESSED, false);
        changed["literals"] = json!(3);
        assert!(matches!(derive_lz(&plan(changed, false), &COMPRESSED), Err(RelationFailure::LzStream { .. })));
        let mut input = COMPRESSED;
        input[7] = b'z';
        assert!(matches!(derive_lz(&plan(captured(&COMPRESSED, false), false), &input), Err(RelationFailure::LzStream { .. })));
    }

    #[test]
    fn after_a_reset_only_positions_written_since_are_derived() {
        let stale = captured(&COMPRESSED, true);
        let (_, leaves) = derive_lz(&plan(stale.clone(), true), &COMPRESSED).unwrap();
        assert_eq!(leaves.iter().filter(|leaf| leaf.contains("/pending/")).count(), 4);
        assert!(matches!(derive_lz(&plan(stale, false), &COMPRESSED), Err(RelationFailure::LzStream { .. })));
    }

    #[test]
    fn jobs_must_be_contiguous_and_in_dram() {
        assert_eq!(job_span(&[(0x4200_0000, 0x100), (0x4200_0100, 0x80)]).unwrap(), (0x200_0000, 0x200_0180));
        assert_eq!(job_span(&[(0x4200_0000, 0x100), (0x4200_0200, 0x80)]), Err(RelationFailure::LzContiguity { job: 1 }));
        assert_eq!(job_span(&[(0x9000_0000, 0x100)]), Err(RelationFailure::LzSource { job: 0 }));
    }

    #[test]
    fn transaction_files_round_trip_and_refuse_tampering() {
        let directory = std::env::temp_dir().join(format!("relation-transactions-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let row = |tick, access| TransactionRow {
            tick, core: Some(Core::Secondary), pc: Pc(0x40f4), address: 0xfff9_1000, size: 4, access, accepted: true, value: 7,
        };
        let rows = [row(5, Access::Read), row(9, Access::Write), TransactionRow { core: None, accepted: false, ..row(9, Access::Read) }];
        let mut writer = TransactionWriter::create(&directory).unwrap();
        for item in &rows {
            writer.push(item).unwrap();
        }
        let file = writer.finish().unwrap();
        let mut read = Vec::new();
        read_transactions(&directory, &file, |_, item| read.push(item)).unwrap();
        assert_eq!(read, rows);
        let more = TransactionFile { rows: 4, ..file.clone() };
        assert!(read_transactions(&directory, &more, |_, _| {}).is_err());
        let path = directory.join(TRANSACTIONS_FILE);
        let mut bytes = std::fs::read(&path).unwrap();
        let last = bytes.len() - 1;
        bytes[last] ^= 1;
        std::fs::write(&path, bytes).unwrap();
        assert!(read_transactions(&directory, &file, |_, _| {}).is_err());
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn decreasing_ticks_refuse() {
        let directory = std::env::temp_dir().join(format!("relation-ticks-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let row = |tick| TransactionRow {
            tick, core: Some(Core::Primary), pc: Pc(0), address: 0xfff9_1000, size: 4, access: Access::Read, accepted: true, value: 0,
        };
        let mut writer = TransactionWriter::create(&directory).unwrap();
        writer.push(&row(9)).unwrap();
        writer.push(&row(8)).unwrap();
        let file = writer.finish().unwrap();
        assert!(read_transactions(&directory, &file, |_, _| {}).is_err());
        std::fs::remove_dir_all(directory).unwrap();
    }
}
