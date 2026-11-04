//! Natural-pair observer for the own-first RTOS slots (#156). From a zero-instruction reset it runs
//! cold boot to the shooting menu and the packaged FILM SIMULATION route. Instruction points stop
//! both cores at every planned slot entry, each entered call's continuation, the +0x1618 tail
//! bodies, every fill word inside a guarded extent and both loader entries. A loader entry arms
//! that loader's immutable-byte guard: a readback of its guarded extents and a classified write
//! watch over them.

use fujimod_emulator::board::factory::MemoryMap;
use fujimod_emulator::board::observation::{MemoryWatch, ViewPolicy, WatchKinds, WatchRange};
use fujimod_emulator_cpu::{Register, arm};
use fujimod_xm1::bridge::{read, sha256};
use fujimod_xm1::guest::RouteKey;
use fujimod_xm1::machine::{Hold, KeyPulse, Machine, RunOutcome, key_pulse};
use fujimod_xm1::run::{self, RunDocument, ValidatedUpdate};
use serde::{Deserialize, Serialize, Serializer};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::{BufWriter, Write};
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

#[path = "memory_view.rs"]
mod memory_view;
use memory_view::{MemoryAddress, MemoryCore, TcmRegister, static_span, tcm_window};

const USAGE: &str = "own-first-pair --plan PLAN --run RUN --update DAT --boot-rom ROM --trajectory TRAJECTORY --output NEW_DIR";
const SCHEMA: u32 = 1;
const PLAN_SCHEMA: u32 = 1;
const ALIAS: u32 = 0x4000_0000;
const ENTRY_LABEL: &str = "own-first-entry";
const CONTINUATION_LABEL: &str = "own-first-continuation";
const TAIL_LABEL: &str = "own-first-tail";
const FILL_LABEL: &str = "own-first-fill";
const ARM_LABEL: &str = "own-first-arm";

/// The pinned engine refuses more than 4,096 instruction points per core.
const SITE_LIMIT: usize = 4096;
const PENDING_LIMIT: usize = 4096;
const CLASS_LIMIT: usize = 1 << 20;
const SAMPLE_LIMIT: usize = 64;
const INSTRUCTION_LIMIT: u64 = 1_500_000_000;
const WATCH_LIMIT: usize = 4096;

const INITIAL_MENU: u64 = 80_000_000;
const SETUP_SETTLE: u64 = 40_000_000;
const SHOOTING_SETTLE: u64 = 90_000_000;
const SETUP_HOLD: u64 = 8_000_000;
const SETUP_RELEASE: u64 = 8_000_000;
const RELEASE_POLL: u64 = 250_000;
const MENU_POLL: u64 = 250_000;
const STEP_STATE_CAP: u64 = 90_000_000;
const STEP_SETTLE: u64 = 40_000_000;

fn require(condition: bool, message: &str) -> Result<(), String> {
    if condition { Ok(()) } else { Err(message.into()) }
}

fn parse_hex(text: &str) -> Result<u32, String> {
    let digits = text.strip_prefix("0x").ok_or_else(|| format!("{text}: hexadecimal prefix missing"))?;
    u32::from_str_radix(digits, 16).map_err(|_| format!("{text}: invalid hexadecimal word"))
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd)]
#[serde(try_from = "String")]
struct Hex(u32);
impl TryFrom<String> for Hex {
    type Error = String;
    fn try_from(text: String) -> Result<Self, String> { parse_hex(&text).map(Self) }
}
impl Serialize for Hex {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&format!("{:#x}", self.0))
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "lowercase")]
enum Domain { A, B }

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
enum WordName { Context, Current, Dispatch, TimerBase, TimeOffset }

/// Which reads of the word at entry r0 a call records.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum Pointee { None, Entry, EntryAndReturn }

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum Role { Stock, Owned, ControlRestored, ControlWrongI }

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PlanDomain { domain: Domain, loader_entry: Hex, words: BTreeMap<WordName, Hex> }

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PlanEntry {
    domain: Domain,
    slot: Hex,
    entry: Hex,
    reads: Vec<WordName>,
    /// The byte at current-task + 0xe.
    task_flags: bool,
    pointee: Pointee,
    return_reads: Vec<WordName>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PlanTail { domain: Domain, body: Hex }

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct PlanRange { domain: Domain, start: Hex, end: Hex, sha256: String }

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Plan {
    schema: u32,
    role: Role,
    dat_sha256: String,
    domains: Vec<PlanDomain>,
    entries: Vec<PlanEntry>,
    tails: Vec<PlanTail>,
    guarded: Vec<PlanRange>,
    fills: Vec<Hex>,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
struct SlotId { domain: Domain, slot: Hex }

struct Slot { id: SlotId, reads: Vec<WordName>, task_flags: bool, pointee: Pointee, return_reads: Vec<WordName> }

/// Every planned site in both spellings.
struct Sites {
    entries: BTreeMap<u32, usize>,
    tails: BTreeMap<u32, Domain>,
    fills: BTreeSet<u32>,
    arms: BTreeMap<u32, Domain>,
}

impl Sites {
    fn labels(&self) -> BTreeMap<u32, String> {
        self.entries.keys().map(|&address| (address, ENTRY_LABEL))
            .chain(self.tails.keys().map(|&address| (address, TAIL_LABEL)))
            .chain(self.fills.iter().map(|&address| (address, FILL_LABEL)))
            .chain(self.arms.keys().map(|&address| (address, ARM_LABEL)))
            .map(|(address, label)| (address, label.to_owned())).collect()
    }
}

fn spellings(address: u32) -> [u32; 2] { [address, address | ALIAS] }

struct Validated {
    plan: Plan,
    slots: Vec<Slot>,
    sites: Sites,
    words: BTreeMap<Domain, BTreeMap<WordName, u32>>,
}

fn validate(plan: Plan) -> Result<Validated, String> {
    require(plan.schema == PLAN_SCHEMA, "plan schema differs")?;
    let mut words = BTreeMap::new();
    let mut arms = BTreeMap::new();
    for domain in &plan.domains {
        require(domain.words.len() == 5, "plan domain does not name its five data words")?;
        require(words.insert(domain.domain, domain.words.iter().map(|(&name, word)| (name, word.0)).collect()).is_none(),
            "plan repeats a domain")?;
        for address in spellings(domain.loader_entry.0) { arms.insert(address, domain.domain); }
    }
    require(words.len() == 2, "plan does not name both loaders")?;
    let mut slots = Vec::new();
    let mut entries = BTreeMap::new();
    for entry in &plan.entries {
        require(entry.entry.0 % 4 == 0 && entry.entry.0 < ALIAS, "plan entry is not an aligned low address")?;
        require(!entry.task_flags || entry.reads.contains(&WordName::Current), "task flags need the current-task read")?;
        for address in spellings(entry.entry.0) {
            require(entries.insert(address, slots.len()).is_none(), "two plan entries share an address")?;
        }
        slots.push(Slot { id: SlotId { domain: entry.domain, slot: entry.slot }, reads: entry.reads.clone(),
            task_flags: entry.task_flags, pointee: entry.pointee, return_reads: entry.return_reads.clone() });
    }
    require(slots.iter().map(|slot| slot.id).collect::<BTreeSet<_>>().len() == slots.len(), "plan repeats a slot")?;
    let tails: BTreeMap<u32, Domain> = plan.tails.iter()
        .flat_map(|tail| spellings(tail.body.0).map(|address| (address, tail.domain))).collect();
    require(tails.len() == 2 * plan.tails.len(), "plan repeats a tail body")?;
    require(plan.tails.iter().all(|tail| slots.iter().any(|slot| slot.id == SlotId { domain: tail.domain, slot: Hex(0x1618) })),
        "a tail body has no +0x1618 entry")?;
    let fills: BTreeSet<u32> = plan.fills.iter().flat_map(|fill| spellings(fill.0)).collect();
    require(fills.len() == 2 * plan.fills.len() && plan.fills.iter().all(|fill| fill.0 % 4 == 0 && fill.0 < ALIAS),
        "plan fill words are repeated or unaligned")?;
    require(!plan.guarded.is_empty() && plan.guarded.len() <= 256 && plan.guarded.iter().all(|range| {
        range.start.0 % 4 == 0 && range.end.0 % 4 == 0 && range.start.0 < range.end.0 && range.end.0 <= ALIAS
    }), "plan guarded ranges are malformed")?;
    let sites = Sites { entries, tails, fills, arms };
    let total = sites.entries.len() + sites.tails.len() + sites.fills.len() + sites.arms.len();
    require(sites.labels().len() == total, "two plan site kinds share an address")?;
    Ok(Validated { plan, slots, sites, words })
}

/// A word through the executing core's view.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(tag = "view", rename_all = "kebab-case")]
enum Read {
    Committed { value: u32 },
    /// The executing core's TCM overlays the word.
    TcmOverlay,
    ViewsDiffer { guest: u32, backing: u32 },
    /// No static map translates the word.
    Unresolved,
    Unaligned,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "lowercase")]
enum Core { Primary, Secondary }
impl Core {
    fn index(self) -> usize { match self { Self::Primary => 0, Self::Secondary => 1 } }
    fn from_index(index: usize) -> Result<Self, String> {
        match index { 0 => Ok(Self::Primary), 1 => Ok(Self::Secondary), _ => Err("the pair expects two cores".into()) }
    }
    fn own_kernel_current_task(self) -> u32 { match self { Self::Primary => 0x4005_4760, Self::Secondary => 0x400e_4720 } }
}

#[derive(Clone, Copy, Debug)]
struct Context { core: Core, pc: u32, lr: u32, sp: u32, cpsr: u32, r0: u32, instructions: u64, current: Read }
impl Context {
    fn mode(&self) -> u32 { self.cpsr & 0x1f }
}

struct Board<'a> { machine: &'a Machine, maps: &'a [MemoryMap] }
impl Board<'_> {
    fn register(&self, core: Core, register: arm::Register) -> Result<u32, String> {
        u32::try_from(self.machine.session.board.read_register(core.index(), Register::Arm32(register))?)
            .map_err(|_| "ARM register exceeds 32 bits".to_owned())
    }
    fn tcm(&self, core: Core) -> Result<[u32; 2], String> {
        let mut banks = [0; 2];
        for (bank, value) in banks.iter_mut().enumerate() {
            *value = u32::try_from(self.machine.session.board.cpu(core.index())?.coprocessor(arm::Coprocessor {
                cp: 15, opc1: 0, crn: 9, crm: 1, opc2: bank as u32, secure: 0, wide: false,
            }, None)?).map_err(|_| "TCM region register exceeds 32 bits".to_owned())?;
        }
        Ok(banks)
    }
    fn identity(&self, core: Core) -> Result<u32, String> {
        self.machine.session.board.cpu(core.index())?.rprofile_identity(None)
    }
    fn word(&self, core: Core, tcm: [u32; 2], address: u32) -> Result<Read, String> {
        if address % 4 != 0 { return Ok(Read::Unaligned); }
        let low = u64::from(address);
        let high = low + 4;
        for bank in tcm {
            if tcm_window(TcmRegister(bank))?.is_some_and(|(start, end)| start < high && low < end) {
                return Ok(Read::TcmOverlay);
            }
        }
        let Ok(span) = static_span(self.maps, MemoryCore(core.index()), MemoryAddress(low)..MemoryAddress(high)) else {
            return Ok(Read::Unresolved);
        };
        let (mut guest, mut backing) = ([0; 4], [0; 4]);
        self.machine.session.board.read_memory(core.index(), low, &mut guest)?;
        self.machine.session.board.read_physical(span.physical.0, &mut backing)?;
        let (guest, backing) = (u32::from_le_bytes(guest), u32::from_le_bytes(backing));
        Ok(if guest == backing { Read::Committed { value: guest } } else { Read::ViewsDiffer { guest, backing } })
    }
    fn context(&self, core: Core) -> Result<(Context, [u32; 2]), String> {
        let tcm = self.tcm(core)?;
        Ok((Context {
            core,
            pc: self.register(core, arm::Register::R(15))?,
            lr: self.register(core, arm::Register::R(14))?,
            sp: self.register(core, arm::Register::R(13))?,
            cpsr: self.register(core, arm::Register::Cpsr)?,
            r0: self.register(core, arm::Register::R(0))?,
            instructions: self.machine.instructions(),
            current: self.word(core, tcm, core.own_kernel_current_task())?,
        }, tcm))
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
struct EntryState {
    slot: SlotId,
    core: Core,
    cpsr: u32,
    r0: u32,
    lr: Hex,
    identity: u32,
    words: BTreeMap<WordName, Read>,
    #[serde(skip_serializing_if = "Option::is_none")]
    task_flags: Option<Read>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pointee: Option<Read>,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
struct ReturnState {
    r0: u32,
    cpsr: u32,
    words: BTreeMap<WordName, Read>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pointee: Option<Read>,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
struct CallClass { entry: EntryState, result: ReturnState, tail_hits: u32 }

#[derive(Serialize)]
struct ClassCount { count: u64, first: u64, last: u64 }

#[derive(Serialize)]
struct ClassRow<'a> { #[serde(flatten)] class: &'a CallClass, #[serde(flatten)] count: &'a ClassCount }

/// A call completes when its core next stops at its continuation with the entry's stack pointer
/// and processor mode; usr, svc and sys calls also need the entry's executing-core current task.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct ReturnKey { core: Core, pc: u32, sp: u32, task: Option<u32>, mode: u32 }
impl ReturnKey {
    fn of(context: &Context, pc: u32) -> Self {
        let task = match context.current {
            Read::Committed { value } if [0x10, 0x13, 0x1f].contains(&context.mode()) => Some(value),
            _ => None,
        };
        Self { core: context.core, pc, sp: context.sp, task, mode: context.mode() }
    }
}

#[derive(Clone, Debug, Serialize)]
struct OpenCall { entry: EntryState, entered: u64, tail_hits: u32 }

#[derive(Default)]
struct OpenCalls { pending: BTreeMap<ReturnKey, Vec<OpenCall>>, count: usize }
impl OpenCalls {
    fn open(&mut self, key: ReturnKey, call: OpenCall) -> Result<(), String> {
        require(self.count < PENDING_LIMIT, "open call bound exceeded")?;
        self.pending.entry(key).or_default().push(call);
        self.count += 1;
        Ok(())
    }
    fn close(&mut self, key: &ReturnKey) -> Vec<OpenCall> {
        let closed = self.pending.remove(key).unwrap_or_default();
        self.count -= closed.len();
        closed
    }
    /// The latest open +0x1618 call of `domain` under `key`.
    fn tail(&mut self, key: &ReturnKey, domain: Domain) -> Option<&mut OpenCall> {
        self.pending.get_mut(key)?.iter_mut().rev()
            .find(|call| call.entry.slot == SlotId { domain, slot: Hex(0x1618) })
    }
}

#[derive(Clone, Copy, Debug, Serialize)]
struct StopRecord { core: Core, address: Hex, sp: Hex, lr: Hex, instructions: u64 }
impl StopRecord {
    fn of(context: &Context) -> Self {
        Self { core: context.core, address: Hex(context.pc), sp: Hex(context.sp), lr: Hex(context.lr), instructions: context.instructions }
    }
}

#[derive(Serialize)]
struct ViewHash {
    core: usize,
    address: Hex,
    #[serde(skip_serializing_if = "Option::is_none")]
    sha256: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

/// One guarded extent, keyed by its physical range.
#[derive(Serialize)]
struct RangeReadback { domain: Domain, start: Hex, end: Hex, physical_sha256: String, views: Vec<ViewHash> }
impl RangeReadback {
    fn matches(&self, expected: &str) -> bool {
        self.physical_sha256 == expected && self.views.iter().all(|view| view.sha256.as_deref() == Some(expected))
    }
}

#[derive(Serialize)]
struct Readback { instructions: u64, ranges: Vec<RangeReadback> }

#[derive(Serialize)]
struct Arming { domain: Domain, stop: StopRecord, readback: Readback }

#[derive(Serialize)]
struct Segment { domains: Vec<Domain>, started: u64, ended: u64, state: serde_json::Value }

struct GuardWatch { watch: Box<MemoryWatch>, domains: Vec<Domain>, started: u64 }

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
enum Key { Up, Down, Ok, Menu, Back }
impl Key {
    fn guest(self) -> RouteKey {
        match self { Self::Up => RouteKey::Up, Self::Down => RouteKey::Down, Self::Ok | Self::Menu => RouteKey::Ok, Self::Back => RouteKey::Back }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct MenuTriple { menu: u32, setting: u32, cursor: u32 }

#[derive(Deserialize)]
struct Trajectory { before_unload: Vec<TrajectoryPhase> }
/// A `key_pulse` is refused; every key takes the bridge's default pulse.
#[derive(Deserialize)]
struct TrajectoryPhase { phase: String, key_pulse: Option<serde::de::IgnoredAny>, steps: Vec<(Key, u32, u32, u32)> }

#[derive(Serialize)]
struct Pulse { key: Key, pressed: u64, released: u64, finished: u64, debounced: bool }

#[derive(Serialize)]
struct Milestone { phase: String, pulse: Option<Pulse>, expected: MenuTriple, reached: Option<u64>, observed: Option<MenuTriple>, finished: u64 }

#[derive(Serialize)]
struct Binding { path: PathBuf, sha256: String }

#[derive(Serialize)]
struct Limits { sites: usize, pending: usize, classes: usize, samples: usize, instructions: u64, watch_rows: usize }

#[derive(Serialize)]
struct Reached { slot: SlotId, calls: u64, returns: u64, pending: u64, first: Option<u64>, last: Option<u64> }

#[derive(Serialize)]
struct Sample<T> { count: u64, sample: Vec<T> }
impl<T> Default for Sample<T> {
    fn default() -> Self { Self { count: 0, sample: Vec::new() } }
}
impl<T> Sample<T> {
    fn push(&mut self, item: T) {
        self.count += 1;
        if self.sample.len() < SAMPLE_LIMIT { self.sample.push(item); }
    }
}

struct Observer<'p> {
    validated: &'p Validated,
    maps: Vec<MemoryMap>,
    return_sites: BTreeSet<u32>,
    open: OpenCalls,
    classes: BTreeMap<CallClass, ClassCount>,
    entries: u64,
    returns: u64,
    unrelated_continuations: u64,
    misplaced_stops: u64,
    untracked: Sample<OpenCall>,
    unarmed_entries: Sample<StopRecord>,
    unmatched_tails: Sample<StopRecord>,
    tail_hits: u64,
    fill_fetch: Option<StopRecord>,
    armings: Vec<Arming>,
    watch: Option<GuardWatch>,
    segments: Vec<Segment>,
    phase: String,
    milestones: Vec<Milestone>,
}

impl Observer<'_> {
    fn slot(&self, index: usize) -> &Slot { &self.validated.slots[index] }

    fn words(&self, board: &Board, core: Core, tcm: [u32; 2], domain: Domain, names: &[WordName]) -> Result<BTreeMap<WordName, Read>, String> {
        let words = &self.validated.words[&domain];
        names.iter().map(|&name| Ok((name, board.word(core, tcm, words[&name])?))).collect()
    }

    fn readback(&self, machine: &Machine, domains: &[Domain]) -> Result<Readback, String> {
        let mut ranges = Vec::new();
        for range in self.validated.plan.guarded.iter().filter(|range| domains.contains(&range.domain)) {
            let count = (range.end.0 - range.start.0) as usize;
            let physical = machine.read(u64::from(range.start.0), count)?;
            let mut views = Vec::new();
            for core in 0..2 {
                for address in spellings(range.start.0) {
                    let mut bytes = vec![0; count];
                    let read = machine.session.board.read_memory(core, u64::from(address), &mut bytes);
                    views.push(match read {
                        Ok(()) => ViewHash { core, address: Hex(address), sha256: Some(sha256(&bytes)), error: None },
                        Err(error) => ViewHash { core, address: Hex(address), sha256: None, error: Some(error) },
                    });
                }
            }
            ranges.push(RangeReadback { domain: range.domain, start: range.start, end: range.end, physical_sha256: sha256(&physical), views });
        }
        Ok(Readback { instructions: machine.instructions(), ranges })
    }

    fn mismatch(&self, readback: &Readback) -> Option<Hex> {
        readback.ranges.iter().find(|observed| !self.validated.plan.guarded.iter()
            .any(|range| range.start == observed.start && range.end == observed.end && observed.matches(&range.sha256)))
            .map(|observed| observed.start)
    }

    fn end_watch(&mut self, machine: &Machine) -> Result<(), String> {
        let Some(mut guard) = self.watch.take() else { return Ok(()) };
        let state = guard.watch.records.borrow().state();
        let ended = machine.session.board.memory_watch_end(&mut guard.watch);
        self.segments.push(Segment { domains: guard.domains, started: guard.started, ended: machine.instructions(), state });
        ended
    }

    fn arm(&mut self, machine: &Machine, context: &Context, domain: Domain) -> Result<(), String> {
        if self.armings.iter().any(|arming| arming.domain == domain) { return Ok(()); }
        let readback = self.readback(machine, &[domain])?;
        let differs = self.mismatch(&readback);
        self.armings.push(Arming { domain, stop: StopRecord::of(context), readback });
        if let Some(start) = differs {
            return Err(format!("arming readback of the extent at {:#x} differs from the plan", start.0));
        }
        self.end_watch(machine)?;
        let domains: Vec<Domain> = self.armings.iter().map(|arming| arming.domain).collect();
        let classes = self.validated.plan.guarded.iter().filter(|range| domains.contains(&range.domain))
            .map(|range| WatchRange { start: u64::from(range.start.0), end: u64::from(range.end.0), kinds: WatchKinds::WRITES }).collect();
        let watch = machine.session.board.memory_watch_begin_classified(classes, WATCH_LIMIT, self.maps.clone(), ViewPolicy::Observe)?;
        self.watch = Some(GuardWatch { watch, domains, started: machine.instructions() });
        Ok(())
    }

    fn check_watch(&self) -> Result<(), String> {
        let Some(guard) = &self.watch else { return Ok(()) };
        let records = guard.watch.records.borrow();
        require(records.violation().is_none() && records.row_count() == 0, "an immutable-byte watch recorded a write or a violation")
    }

    fn enter(&mut self, machine: &mut Machine, context: &Context, tcm: [u32; 2], index: usize) -> Result<(), String> {
        let board = Board { machine, maps: &self.maps };
        let slot = self.slot(index);
        let domain = slot.id.domain;
        let words = self.words(&board, context.core, tcm, domain, &slot.reads)?;
        let task_flags = match (slot.task_flags, words.get(&WordName::Current)) {
            (true, Some(Read::Committed { value })) if *value != 0 => Some(match board.word(context.core, tcm, value.wrapping_add(0xc))? {
                Read::Committed { value } => Read::Committed { value: (value >> 16) & 0xff },
                other => other,
            }),
            _ => None,
        };
        let pointee = (slot.pointee != Pointee::None).then(|| board.word(context.core, tcm, context.r0)).transpose()?;
        let entry = EntryState {
            slot: slot.id, core: context.core, cpsr: context.cpsr, r0: context.r0, lr: Hex(context.lr),
            identity: board.identity(context.core)?, words, task_flags, pointee,
        };
        self.entries += 1;
        if !self.armings.iter().any(|arming| arming.domain == domain) { self.unarmed_entries.push(StopRecord::of(context)); }
        let call = OpenCall { entry, entered: context.instructions, tail_hits: 0 };
        let continuation = context.lr & !1;
        if continuation == 0 || (context.lr & 1 == 0 && continuation % 4 != 0) {
            self.untracked.push(call);
            return Ok(());
        }
        if !self.return_sites.contains(&continuation) {
            if !machine.traces.sites.contains_key(&continuation) {
                require(machine.traces.sites.len() < SITE_LIMIT, "continuation site bound exceeded")?;
                machine.traces.add_sites(&machine.session, [(continuation, CONTINUATION_LABEL.to_owned())].into_iter().collect())?;
            }
            self.return_sites.insert(continuation);
        }
        self.open.open(ReturnKey::of(context, continuation), call)
    }

    fn complete(&mut self, machine: &Machine, context: &Context, tcm: [u32; 2]) -> Result<(), String> {
        let closed = self.open.close(&ReturnKey::of(context, context.pc));
        if closed.is_empty() { self.unrelated_continuations += 1; }
        let board = Board { machine, maps: &self.maps };
        for call in closed {
            let slot = self.validated.slots.iter().find(|slot| slot.id == call.entry.slot).ok_or("closed call names no slot")?;
            let words = self.words(&board, context.core, tcm, slot.id.domain, &slot.return_reads)?;
            let pointee = (slot.pointee == Pointee::EntryAndReturn).then(|| board.word(context.core, tcm, call.entry.r0)).transpose()?;
            let class = CallClass { entry: call.entry, result: ReturnState { r0: context.r0, cpsr: context.cpsr, words, pointee }, tail_hits: call.tail_hits };
            self.returns += 1;
            if let Some(count) = self.classes.get_mut(&class) {
                count.count += 1;
                count.last = call.entered;
            } else {
                require(self.classes.len() < CLASS_LIMIT, "call class bound exceeded")?;
                self.classes.insert(class, ClassCount { count: 1, first: call.entered, last: call.entered });
            }
        }
        Ok(())
    }

    fn stop(&mut self, machine: &mut Machine, stops: &[(Core, u32)]) -> Result<(), String> {
        for &(core, point) in stops {
            let (context, tcm) = Board { machine, maps: &self.maps }.context(core)?;
            if context.pc != point {
                self.misplaced_stops += 1;
                continue;
            }
            if self.return_sites.contains(&context.pc) { self.complete(machine, &context, tcm)?; }
            let sites = &self.validated.sites;
            if sites.fills.contains(&context.pc) {
                self.fill_fetch = Some(StopRecord::of(&context));
                return Err(format!("fill word at {:#x} fetched", context.pc));
            }
            if let Some(&domain) = sites.arms.get(&context.pc) { self.arm(machine, &context, domain)?; }
            if let Some(&domain) = sites.tails.get(&context.pc) {
                match self.open.tail(&ReturnKey::of(&context, context.lr & !1), domain) {
                    Some(call) => { call.tail_hits += 1; self.tail_hits += 1; }
                    None => self.unmatched_tails.push(StopRecord::of(&context)),
                }
            }
            if let Some(&index) = sites.entries.get(&context.pc) { self.enter(machine, &context, tcm, index)?; }
        }
        Ok(())
    }

    fn advance(&mut self, machine: &mut Machine, budget: u64) -> Result<(), String> {
        self.advance_until(machine, budget, None)
    }

    /// Runs `budget` instructions, or until board tick `until` when given.
    fn advance_until(&mut self, machine: &mut Machine, budget: u64, until: Option<u64>) -> Result<(), String> {
        let end = machine.instructions().checked_add(budget).filter(|end| *end <= INSTRUCTION_LIMIT)
            .ok_or("pair instruction bound exceeded")?;
        let reached = |machine: &Machine| machine.instructions() >= end || until.is_some_and(|tick| machine.session.board.now() >= tick);
        while !reached(machine) {
            machine.traces.hits.clear();
            let before = machine.instructions();
            let outcome = machine.run_until(end - before, until);
            machine.require_running()?;
            require(matches!(outcome, RunOutcome::Stopped | RunOutcome::Completed), "unexpected pair run outcome")?;
            let mut stops = Vec::with_capacity(2);
            for hit in &machine.traces.hits {
                require(hit.instructions == machine.instructions(), "pair stop is no longer current")?;
                let core = Core::from_index(hit.core)?;
                require(!stops.iter().any(|&(other, _)| other == core), "one core stopped twice at one boundary")?;
                stops.push((core, parse_hex(&hit.address)?));
            }
            if stops.is_empty() {
                require(reached(machine), "run ended early without a pair stop")?;
            } else {
                self.stop(machine, &stops)?;
            }
            self.check_watch()?;
            require(machine.instructions() > before || !stops.is_empty(), "pair made no progress")?;
        }
        Ok(())
    }

    fn pulse(&mut self, machine: &mut Machine, key: Key, pulse: KeyPulse) -> Result<Pulse, String> {
        let pressed = machine.instructions();
        machine.press(key.guest())?;
        match pulse.hold {
            Hold::Instructions(hold) => self.advance(machine, hold)?,
            Hold::BoardTicks(ticks) => {
                let until = machine.session.board.now().saturating_add(ticks);
                self.advance_until(machine, machine.hold_bound(ticks), Some(until))?;
            }
        }
        machine.release_key(key.guest())?;
        let released = machine.instructions();
        let debounced = loop {
            if machine.document.firmware.released(&machine.session, key.guest())? { break true; }
            let spent = machine.instructions() - released;
            if spent >= pulse.release { break false; }
            self.advance(machine, (pulse.release - spent).min(RELEASE_POLL))?;
        };
        Ok(Pulse { key, pressed, released, finished: machine.instructions(), debounced })
    }

    fn menu(machine: &Machine) -> Result<Option<MenuTriple>, String> {
        Ok(machine.menu()?.map(|menu| MenuTriple { menu: menu.menu_id, setting: menu.setting_id, cursor: menu.cursor }))
    }

    /// Records a fixed-time endpoint.
    fn endpoint(&mut self, machine: &Machine, pulse: Option<Pulse>, expected: MenuTriple) -> Result<(), String> {
        let observed = Self::menu(machine)?;
        let reached = (observed == Some(expected)).then(|| machine.instructions());
        self.milestones.push(Milestone { phase: self.phase.clone(), pulse, expected, reached, observed, finished: machine.instructions() });
        require(reached.is_some(), "menu endpoint differs")
    }

    /// Runs until the menu state is `expected`, then a fixed settle.
    fn state(&mut self, machine: &mut Machine, pulse: Pulse, expected: MenuTriple) -> Result<(), String> {
        let start = machine.instructions();
        let reached = loop {
            if Self::menu(machine)? == Some(expected) { break Some(machine.instructions()); }
            let spent = machine.instructions() - start;
            if spent >= STEP_STATE_CAP { break None; }
            self.advance(machine, (STEP_STATE_CAP - spent).min(MENU_POLL))?;
        };
        if reached.is_some() { self.advance(machine, STEP_SETTLE)?; }
        let observed = Self::menu(machine)?;
        self.milestones.push(Milestone { phase: self.phase.clone(), pulse: Some(pulse), expected, reached, observed, finished: machine.instructions() });
        require(reached.is_some() && observed == Some(expected), "menu trajectory state differs")
    }

    fn scenario(&mut self, machine: &mut Machine, trajectory: &[TrajectoryPhase]) -> Result<(), String> {
        self.phase = "cold-start".into();
        self.advance(machine, INITIAL_MENU)?;
        self.endpoint(machine, None, MenuTriple { menu: 69, setting: 52, cursor: 12 })?;
        self.phase = "initial-setup".into();
        for key in std::iter::repeat_n(Key::Up, 12).chain([Key::Ok, Key::Ok]) {
            self.pulse(machine, key, KeyPulse::instructions(SETUP_HOLD, SETUP_RELEASE))?;
        }
        self.advance(machine, SETUP_SETTLE)?;
        self.phase = "shooting-menu".into();
        let pulse = self.pulse(machine, Key::Menu, KeyPulse::instructions(SETUP_HOLD, SETUP_RELEASE))?;
        self.advance(machine, SHOOTING_SETTLE)?;
        self.endpoint(machine, Some(pulse), MenuTriple { menu: 1, setting: 5, cursor: 2 })?;
        for phase in trajectory {
            self.phase = phase.phase.clone();
            for &(key, menu, setting, cursor) in &phase.steps {
                let pulse = self.pulse(machine, key, key_pulse(key.guest()))?;
                self.state(machine, pulse, MenuTriple { menu, setting, cursor })?;
            }
        }
        Ok(())
    }

    fn reached(&self) -> Vec<Reached> {
        self.validated.slots.iter().map(|slot| {
            let rows: Vec<_> = self.classes.iter().filter(|(class, _)| class.entry.slot == slot.id).map(|(_, count)| count).collect();
            let pending = self.open.pending.values().flatten().filter(|call| call.entry.slot == slot.id).count() as u64;
            let untracked = self.untracked.sample.iter().filter(|call| call.entry.slot == slot.id).count() as u64;
            let returns: u64 = rows.iter().map(|count| count.count).sum();
            Reached {
                slot: slot.id, calls: returns + pending + untracked, returns, pending,
                first: rows.iter().map(|count| count.first).min(), last: rows.iter().map(|count| count.last).max(),
            }
        }).collect()
    }
}

#[derive(Serialize)]
struct Report<'a> {
    schema: u32,
    role: Role,
    inputs: Vec<Binding>,
    reset_sha256: String,
    limits: Limits,
    instructions: u64,
    counts: Option<[u64; 2]>,
    entries: u64,
    returns: u64,
    unrelated_continuations: u64,
    misplaced_stops: u64,
    continuation_sites: usize,
    tail_hits: u64,
    untracked: &'a Sample<OpenCall>,
    unarmed_entries: &'a Sample<StopRecord>,
    unmatched_tails: &'a Sample<StopRecord>,
    fill_fetch: Option<StopRecord>,
    armings: &'a [Arming],
    segments: &'a [Segment],
    final_readback: Option<Readback>,
    milestones: &'a [Milestone],
    reached: Vec<Reached>,
    classes: Vec<ClassRow<'a>>,
    pending: Vec<&'a OpenCall>,
    error: Option<String>,
    machine_failure: Option<String>,
}

fn arguments() -> Result<BTreeMap<String, PathBuf>, String> {
    const OPTIONS: [&str; 6] = ["--plan", "--run", "--update", "--boot-rom", "--trajectory", "--output"];
    let mut arguments = std::env::args_os().skip(1);
    let mut paths = BTreeMap::new();
    while let Some(flag) = arguments.next() {
        let name = flag.to_str().filter(|name| OPTIONS.contains(name)).ok_or(USAGE)?.to_owned();
        require(paths.insert(name, PathBuf::from(arguments.next().ok_or(USAGE)?)).is_none(), USAGE)?;
    }
    require(paths.len() == OPTIONS.len(), USAGE)?;
    Ok(paths)
}

fn bound(path: &Path) -> Result<(Vec<u8>, Binding), String> {
    let bytes = read(path)?;
    let digest = sha256(&bytes);
    Ok((bytes, Binding { path: path.to_owned(), sha256: digest }))
}

fn drive() -> Result<(), String> {
    let paths = arguments()?;
    let output = paths["--output"].clone();
    let (plan_bytes, plan_binding) = bound(&paths["--plan"])?;
    let (run_bytes, run_binding) = bound(&paths["--run"])?;
    let (dat, dat_binding) = bound(&paths["--update"])?;
    let (rom, rom_binding) = bound(&paths["--boot-rom"])?;
    let (trajectory_bytes, trajectory_binding) = bound(&paths["--trajectory"])?;
    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    let (_, executable_binding) = bound(&executable)?;
    let plan: Plan = serde_json::from_slice(&plan_bytes).map_err(|error| format!("plan: {error}"))?;
    let validated = validate(plan)?;
    require(dat_binding.sha256 == validated.plan.dat_sha256, "DAT differs from the plan's DAT")?;
    let trajectory: Trajectory = serde_json::from_slice(&trajectory_bytes).map_err(|error| format!("trajectory: {error}"))?;
    let phases: Vec<&str> = trajectory.before_unload.iter().map(|phase| phase.phase.as_str()).collect();
    require(phases == ["package-menu-prefix", "modal75-shooting", "modal75-film", "modal75-returned", "modal75-film-repeated",
        "modal75-returned-repeated", "modal75-handback"], "trajectory phases differ from the #128 FILM SIMULATION route")?;
    require(trajectory.before_unload[1].steps.iter().map(|&(_, menu, setting, cursor)| [menu, setting, cursor]).eq([[1, 5, 2]]),
        "trajectory shooting-menu entry differs")?;
    require(trajectory.before_unload.iter().all(|phase| phase.key_pulse.is_none()), "trajectory pins a key pulse; pair keys take the default pulse")?;
    std::fs::create_dir(&output).map_err(|error| format!("{}: {error}", output.display()))?;

    fujimod_emulator_exr2_board::register_board_builder();
    fujimod_emulator_models::register();
    let document = RunDocument::parse(&run_bytes)?;
    let maps = run::memory_maps(&document)?;
    let mut session = run::open(&document, &ValidatedUpdate(dat), &rom)?;
    session.board.set_execution_policy(document.execution_policy.into())?;
    let mut machine = Machine::new(document, session)?;
    machine.disarm()?;
    let checkpoint = machine.session.checkpoint(None)?.export()?;
    let restored = machine.session.import_snapshot(&checkpoint)?;
    machine.session.restore(&restored)?;
    machine.rearm()?;
    require(machine.instructions() == 0, "reset checkpoint is not preexecution")?;

    let mut observer = Observer {
        validated: &validated,
        maps,
        return_sites: BTreeSet::new(),
        open: OpenCalls::default(),
        classes: BTreeMap::new(),
        entries: 0,
        returns: 0,
        unrelated_continuations: 0,
        misplaced_stops: 0,
        untracked: Sample::default(),
        unarmed_entries: Sample::default(),
        unmatched_tails: Sample::default(),
        tail_hits: 0,
        fill_fetch: None,
        armings: Vec::new(),
        watch: None,
        segments: Vec::new(),
        phase: String::new(),
        milestones: Vec::new(),
    };
    machine.traces.bound_full_registers(NonZeroUsize::new(2).ok_or("zero hit bound")?);
    machine.traces.stop_labels = [ENTRY_LABEL, CONTINUATION_LABEL, TAIL_LABEL, FILL_LABEL, ARM_LABEL]
        .into_iter().map(str::to_owned).collect();
    let observed = (|| -> Result<(), String> {
        machine.traces.arm(&machine.session, validated.sites.labels())?;
        observer.scenario(&mut machine, &trajectory.before_unload[2..])
    })();
    let domains: Vec<Domain> = observer.armings.iter().map(|arming| arming.domain).collect();
    let final_readback = observer.readback(&machine, &domains);
    let ended = observer.end_watch(&machine);
    let (final_readback, read_error) = match final_readback {
        Ok(readback) => (Some(readback), None),
        Err(error) => (None, Some(error)),
    };
    let error = observed.err().or(ended.err()).or(read_error);
    let report = Report {
        schema: SCHEMA,
        role: validated.plan.role,
        inputs: vec![plan_binding, run_binding, dat_binding, rom_binding, trajectory_binding, executable_binding],
        reset_sha256: sha256(&checkpoint),
        limits: Limits { sites: SITE_LIMIT, pending: PENDING_LIMIT, classes: CLASS_LIMIT, samples: SAMPLE_LIMIT, instructions: INSTRUCTION_LIMIT, watch_rows: WATCH_LIMIT },
        instructions: machine.instructions(),
        counts: machine.session.board.instruction_counts().try_into().ok(),
        entries: observer.entries,
        returns: observer.returns,
        unrelated_continuations: observer.unrelated_continuations,
        misplaced_stops: observer.misplaced_stops,
        continuation_sites: observer.return_sites.len(),
        tail_hits: observer.tail_hits,
        untracked: &observer.untracked,
        unarmed_entries: &observer.unarmed_entries,
        unmatched_tails: &observer.unmatched_tails,
        fill_fetch: observer.fill_fetch,
        armings: &observer.armings,
        segments: &observer.segments,
        final_readback,
        milestones: &observer.milestones,
        reached: observer.reached(),
        classes: observer.classes.iter().map(|(class, count)| ClassRow { class, count }).collect(),
        pending: observer.open.pending.values().flatten().collect(),
        error: error.clone(),
        machine_failure: machine.failure.clone(),
    };
    let file = File::create_new(output.join("observation.json")).map_err(|error| error.to_string())?;
    let mut writer = BufWriter::new(file);
    serde_json::to_writer(&mut writer, &report).map_err(|error| error.to_string())?;
    writer.write_all(b"\n").and_then(|()| writer.flush()).map_err(|error| error.to_string())?;
    match error { Some(error) => Err(error), None => Ok(()) }
}

fn main() -> ExitCode {
    match drive() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("own-first-pair: {error}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plan(fills: &str, tails: &str) -> Vec<u8> {
        let words = r#"{"context":"0x400545a4","current":"0x40054760","dispatch":"0x40054764","timer-base":"0x4005476c","time-offset":"0x40054774"}"#;
        format!(concat!(r#"{{"schema":1,"role":"owned","dat_sha256":"00","#,
            r#""domains":[{{"domain":"a","loader_entry":"0x40020","words":{w}}},{{"domain":"b","loader_entry":"0xd0020","words":{w}}}],"#,
            r#""entries":[{{"domain":"a","slot":"0x1618","entry":"0x412bc","reads":[],"task_flags":false,"pointee":"none","return_reads":[]}}],"#,
            r#""tails":{t},"guarded":[{{"domain":"a","start":"0x412bc","end":"0x412cc","sha256":"00"}}],"fills":{f}}}"#),
            w = words, t = tails, f = fills).into_bytes()
    }

    fn context(core: Core, sp: u32, lr: u32) -> Context {
        Context { core, pc: 0, lr, sp, cpsr: 0x13, r0: 0, instructions: 0, current: Read::Committed { value: 0x4005_ff0c } }
    }

    fn entry(domain: Domain, slot: u32) -> EntryState {
        EntryState { slot: SlotId { domain, slot: Hex(slot) }, core: Core::Primary, cpsr: 0x13, r0: 0, lr: Hex(0x725e00),
            identity: 0x4003, words: BTreeMap::new(), task_flags: None, pointee: None }
    }

    #[test]
    fn plan_arms_every_site_in_both_spellings() {
        let validated = validate(serde_json::from_slice(&plan(r#"["0x412c0"]"#, r#"[{"domain":"a","body":"0x4c71c"}]"#)).unwrap()).unwrap();
        let labels = validated.sites.labels();
        for address in [0x412bc, 0x4004_12bc, 0x412c0, 0x4004_12c0, 0x4c71c, 0x4004_c71c, 0x40020, 0x4004_0020, 0xd0020, 0x400d_0020] {
            assert!(labels.contains_key(&address), "{address:#x}");
        }
        assert_eq!(validated.words[&Domain::B][&WordName::TimeOffset], 0x4005_4774);
    }

    #[test]
    fn plan_refuses_a_fill_on_an_entry_and_a_tail_without_its_slot() {
        assert!(validate(serde_json::from_slice(&plan(r#"["0x412bc"]"#, "[]")).unwrap()).is_err());
        assert!(validate(serde_json::from_slice(&plan("[]", r#"[{"domain":"b","body":"0xdc6dc"}]"#)).unwrap()).is_err());
    }

    #[test]
    fn tail_hit_joins_the_open_identity_call_with_the_same_stack_and_continuation() {
        let mut calls = OpenCalls::default();
        let at_entry = context(Core::Primary, 0x1000, 0x725e00);
        calls.open(ReturnKey::of(&at_entry, 0x725e00), OpenCall { entry: entry(Domain::A, 0x1618), entered: 1, tail_hits: 0 }).unwrap();
        assert!(calls.tail(&ReturnKey::of(&context(Core::Primary, 0x0ff8, 0x725e00), 0x725e00), Domain::A).is_none());
        assert!(calls.tail(&ReturnKey::of(&context(Core::Secondary, 0x1000, 0x725e00), 0x725e00), Domain::A).is_none());
        assert!(calls.tail(&ReturnKey::of(&at_entry, 0x725e00), Domain::B).is_none());
        calls.tail(&ReturnKey::of(&at_entry, 0x725e00), Domain::A).unwrap().tail_hits += 1;
        assert_eq!(calls.close(&ReturnKey::of(&at_entry, 0x725e00))[0].tail_hits, 1);
    }

    #[test]
    fn a_tail_hit_never_joins_another_slot() {
        let mut calls = OpenCalls::default();
        let at_entry = context(Core::Primary, 0x1000, 0x725e00);
        calls.open(ReturnKey::of(&at_entry, 0x725e00), OpenCall { entry: entry(Domain::A, 0x164), entered: 1, tail_hits: 0 }).unwrap();
        assert!(calls.tail(&ReturnKey::of(&at_entry, 0x725e00), Domain::A).is_none());
    }
}
