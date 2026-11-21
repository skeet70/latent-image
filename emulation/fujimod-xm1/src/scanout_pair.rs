//! Stock/owned display scanout pair on the natural no-card menu route: every entry into a retired stock body,
//! its bus traffic on the display banks, the callbacks it drains and the record words it changes, with a
//! continuous fetch/write guard over the retired bodies and the owned fixed range.

mod scanout_pair_laws;
mod scanout_pair_route;

use fujimod_emulator::board::factory::MemoryMap;
use fujimod_emulator::board::observation::{MemoryWatch, ViewPolicy, WatchKinds, WatchRange};
use fujimod_emulator_cpu::{Register, arm};
use fujimod_xm1::bridge::{read, sha256, write};
use fujimod_xm1::guest::RouteKey;
use fujimod_xm1::machine::{Machine, RunOutcome};
use fujimod_xm1::run::{self, RunDocument, ValidatedUpdate};
use scanout_pair_laws::*;
use scanout_pair_route::{Binding, Presenting, bind_frame};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::process::ExitCode;

const USAGE: &str = "scanout-pair stock CONTRACT DAT ANALYSIS RUN ROM OUTPUT\n\
    scanout-pair owned CONTRACT DAT ANALYSIS RUN ROM STOCK_DIR OUTPUT\n\
    scanout-pair check STOCK_DIR OWNED_DIR REPORT_JSON\n\
    scanout-pair qualify STOCK_DIR OWNED_DIR REGISTER_ORDER_DIR SKIPPED_DRAIN_DIR REPORT_JSON";
const ALIAS: u32 = 0x4000_0000;
const IMAGE4: u32 = 0x61c400;
const LABEL: &str = "scanout";
const DISPLAY_BANKS: (u64, u64) = (0xfff69000, 0xfff6f000);
const BUS_LIMIT: usize = 1 << 20;
const WATCH_LIMIT: usize = 1 << 20;
const HIT_LIMIT: usize = 1 << 16;
const KEY_HOLD: u64 = 8_000_000;
const FRAME_WAIT: u64 = 1_000_000;
/// Receipts whose checkpoints carry the wait steps they took before their frame.
const RECEIPT_SCHEMA: u32 = 3;

fn require(condition: bool, reason: &str) -> Result<(), String> { if condition { Ok(()) } else { Err(reason.into()) } }

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum Role { Stock, Owned }

#[derive(Deserialize)]
struct Range { low: u32, high: u32 }
#[derive(Deserialize)]
struct Named { name: String, address: u32 }
#[derive(Deserialize)]
struct Published { address: u32, old: u32, new: u32 }
#[derive(Deserialize)]
struct Contract { schema: u32, subsystem: String, carriers: Vec<Range>, entries: Vec<Named>, publications: Vec<Published>, body: Range }

struct Inputs {
    role: Role, contract_sha256: String, run: Vec<u8>, rom: Vec<u8>, dat: ValidatedUpdate,
    /// Image 4 as the role runs it: canonical for stock, canonical with the entry branches for owned.
    image4: Vec<u8>, entries: Vec<u32>, body: (u32, u32),
    /// The owned role's route: the board tick each stock route point ended at, the stock checkpoints' wait steps
    /// and the stock receipt's digest.
    schedule: Option<Schedule>,
}

struct Schedule { ticks: Vec<u64>, waits: Vec<u32>, digest: String }

fn load(arguments: &[String]) -> Result<Inputs, String> {
    let role = if arguments[0] == "stock" { Role::Stock } else { Role::Owned };
    let contract_bytes = read(Path::new(&arguments[1]))?;
    let contract: Contract = serde_json::from_slice(&contract_bytes).map_err(|error| error.to_string())?;
    require(contract.schema == 2 && contract.subsystem == "display_scanout", "contract is not a display scanout fixed-bank contract")?;
    require(contract.carriers.iter().map(|range| (range.low, range.high)).eq(CARRIERS), "contract carriers differ from the retired bodies")?;
    let names = ["isr_one", "isr_zero", "scheduler_register", "capture", "item_configuration", "range_packet", "range_enabled",
        "group_options", "commit", "commit_zero", "commit_one", "blank"];
    require(contract.entries.len() == Entry::ALL.len() && contract.entries.iter().zip(Entry::ALL).zip(names)
        .all(|((named, entry), name)| named.name == format!("xm1_scanout_{name}") && named.address == entry.address()), "contract entries differ")?;
    let mut image4 = read(&Path::new(&arguments[3]).join("lz/id4-0061c400.bin"))?;
    require(sha256(&image4) == "f2e2acab892dafeed85171f6870a411f0a2d5eae10921b86a24630d401dcff16", "canonical image 4 differs")?;
    require(contract.publications.iter().map(|row| row.address).eq(Entry::ALL.map(Entry::address)), "contract publications differ from the entries")?;
    for row in &contract.publications {
        let offset = (row.address - IMAGE4) as usize;
        require(image4[offset..offset + 4] == row.old.to_le_bytes(), "publication preimage differs from canonical image 4")?;
        if role == Role::Owned { image4[offset..offset + 4].copy_from_slice(&row.new.to_le_bytes()); }
    }
    let run = read(Path::new(&arguments[4]))?;
    let rom = read(Path::new(&arguments[5]))?;
    require(sha256(&run) == "efa32ff8d2a378ba9e098ac28015faf82856765c866d5db5b97cb1c38342deef"
        && sha256(&rom) == "4ecedfbfc1910bde488d36abe4380621f8845e6afcfc6cf54a49a33ec0abf9be", "the pair requires the canonical no-card run and boot ROM")?;
    let schedule = match role {
        Role::Stock => None,
        Role::Owned => {
            let bytes = read(&Path::new(&arguments[6]).join("receipt.json"))?;
            let stock: Receipt = serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
            require(stock.role == Role::Stock && stock.complete && !stock.points.is_empty(), "the owned route needs a complete stock receipt")?;
            Some(Schedule {
                ticks: stock.points.iter().map(|point| point.tick).collect(),
                waits: stock.checkpoints.iter().map(|checkpoint| checkpoint.waits).collect(),
                digest: sha256(&bytes),
            })
        }
    };
    Ok(Inputs {
        role, contract_sha256: sha256(&contract_bytes), run, rom, dat: ValidatedUpdate(read(Path::new(&arguments[2]))?), image4,
        entries: contract.publications.iter().map(|row| row.address).collect(), body: (contract.body.low, contract.body.high), schedule,
    })
}

fn register(machine: &Machine, core: usize, number: u8) -> Result<u32, String> {
    u32::try_from(machine.session.board.read_register(core, Register::Arm32(arm::Register::R(number)))?).map_err(|error| error.to_string())
}

fn words(machine: &Machine, address: u32, count: u32) -> Result<Vec<u32>, String> {
    let bytes = machine.read(u64::from(address), count as usize * 4)?;
    Ok(bytes.chunks_exact(4).map(|word| u32::from_le_bytes([word[0], word[1], word[2], word[3]])).collect())
}

/// Words as `core` sees them, which includes its tightly coupled stack memory.
fn core_words(machine: &Machine, core: usize, address: u32, count: usize) -> Result<Vec<u32>, String> {
    let mut bytes = vec![0; count * 4];
    machine.session.board.read_memory(core, u64::from(address), &mut bytes)?;
    Ok(bytes.chunks_exact(4).map(|word| u32::from_le_bytes([word[0], word[1], word[2], word[3]])).collect())
}

fn region_words(machine: &Machine) -> Result<Vec<(u32, u32)>, String> {
    let mut result = Vec::new();
    for (start, length) in REGIONS {
        for (index, value) in words(machine, start, length / 4)?.into_iter().enumerate() { result.push((start + 4 * index as u32, value)); }
    }
    Ok(result)
}

/// A commit's first then second six slots, or the scheduler's one-shot then persistent callback words.
fn drain_words(machine: &Machine, call: &Call) -> Result<Vec<u32>, String> {
    match (call.entry, call.engine) {
        (_, Some(engine)) => words(machine, runtime(engine.min(1)) + 0x24, 12),
        (Entry::IsrOne | Entry::IsrZero, None) => scheduler_words(machine),
        _ => Ok(Vec::new()),
    }
}

fn scheduler_words(machine: &Machine) -> Result<Vec<u32>, String> {
    let mut result = Vec::new();
    for base in [SCHEDULER + 0x54, SCHEDULER + 0x14] { for slot in 0..4 { result.push(machine.word(u64::from(base + slot * 16 + 8))?); } }
    Ok(result)
}

fn front_state(machine: &Machine, engine: u32) -> Result<FrontState, String> {
    let front = machine.read(u64::from(FRONT), 1)?[0];
    let image = machine.word(u64::from(STAGE_IMAGES + 4 * u32::from(front.min(1))))?;
    let image_address = machine.word(u64::from((image & !ALIAS) + 0x1c))?;
    Ok(FrontState { front, image_address, shadow: machine.word(u64::from(saved(engine.min(1)) + 0xb0))? })
}

fn menu(machine: &Machine) -> Result<Option<[u32; 3]>, String> {
    machine.menu().map(|value| value.map(|value| [value.menu_id, value.setting_id, value.cursor]))
}

#[derive(Deserialize)]
#[serde(rename_all = "kebab-case")]
enum Access { CpuFetch, CpuWrite, CpuRead, HostRead, HostWrite, ModelBusRead, ModelBusWrite }
#[derive(Deserialize)]
struct AccessRecord { source: Access, physical: u64, size: u64, core: Option<usize> }
#[derive(Deserialize)]
struct AccessBatch { enabled: bool, violation: Option<String>, records: Vec<AccessRecord> }

/// The guard's fetch/write classes, and CPU reads and writes of the compared record words.
fn guard_ranges(body: (u32, u32)) -> Vec<WatchRange> {
    CARRIERS.iter().map(|(low, high)| WatchRange { start: u64::from(*low), end: u64::from(*high), kinds: WatchKinds::CPU_FETCH.union(WatchKinds::WRITES) })
        .chain([WatchRange { start: u64::from(body.0), end: u64::from(body.1), kinds: WatchKinds::CPU_FETCH }])
        .chain(REGIONS.iter().map(|(start, length)| WatchRange {
            start: u64::from(*start), end: u64::from(start + length), kinds: WatchKinds::CPU_READ.union(WatchKinds::CPU_WRITE),
        }))
        .collect()
}

/// `fetches` counts instruction fetches per address in the retired bodies and the owned range.
#[derive(Default)]
struct Guard { carrier_fetches: u64, body_fetches: u64, installation_writes: u64, first_fetch: bool, fetches: BTreeMap<u32, u64> }

impl Guard {
    /// Retired bodies change only through the image-4 load before their first fetch. In the owned role the only
    /// retired words fetched are the published entry branches.
    fn batch(&mut self, batch: &AccessBatch, role: Role, entries: &[u32], body: (u32, u32)) -> Result<(), String> {
        require(batch.enabled && batch.violation.is_none() && batch.records.len() < WATCH_LIMIT, "guard watch overflowed or refused")?;
        for row in &batch.records {
            let low = u32::try_from(row.physical).map_err(|error| error.to_string())?;
            let high = low.checked_add(u32::try_from(row.size).map_err(|error| error.to_string())?).ok_or("guard row overflows")?;
            if matches!(row.source, Access::CpuFetch) && body.0 <= low && high <= body.1 {
                self.body_fetches += 1;
                *self.fetches.entry(low).or_insert(0) += 1;
                continue;
            }
            if !CARRIERS.iter().any(|(start, end)| low < *end && *start < high) { continue; }
            match row.source {
                Access::CpuFetch => {
                    require(role == Role::Stock || (row.size == 4 && entries.contains(&low)), "owned role fetched a retired stock body word")?;
                    self.first_fetch = true;
                    self.carrier_fetches += 1;
                    *self.fetches.entry(low).or_insert(0) += 1;
                }
                Access::ModelBusWrite if !self.first_fetch => self.installation_writes += 1,
                Access::CpuWrite | Access::HostWrite | Access::ModelBusWrite => return Err("retired body modified outside the image-4 load".into()),
                Access::CpuRead | Access::HostRead | Access::ModelBusRead => {}
            }
        }
        Ok(())
    }
}

#[derive(Deserialize)]
struct Transaction { cpu: usize, address: u64, operation: String, value: u64 }
#[derive(Deserialize)]
struct TransactionBatch { transactions: Vec<Transaction>, omitted: u64 }

enum Open {
    /// `scope` is the call that collects bus rows and callbacks while this frame is the innermost frame.
    Frame { call: usize, scope: usize, lr: u32, sp: u32, before: Vec<(u32, u32)>, sampled: bool },
    Callback { call: usize, callback: usize, lr: u32, sp: u32 },
}
impl Open {
    fn returns_at(&self, pc: u32, stack: u32) -> bool {
        match self { Self::Frame { lr, sp, .. } | Self::Callback { lr, sp, .. } => *lr == pc && *sp == stack }
    }
}

struct Observer {
    body: (u32, u32),
    calls: Vec<Call>,
    open: [Vec<Open>; 2],
    unattributed: Vec<Bus>,
    guard: Guard,
    armed: BTreeSet<u32>,
}

impl Observer {
    fn innermost(&self, core: usize) -> Option<(usize, usize)> {
        self.open[core].iter().rev().find_map(|open| match open { Open::Frame { call, scope, .. } => Some((*call, *scope)), Open::Callback { .. } => None })
    }

    fn bus(&mut self, batch: TransactionBatch) -> Result<(), String> {
        require(batch.omitted == 0, "display bus watch overflowed")?;
        for transaction in batch.transactions {
            let row = Bus { address: u32::try_from(transaction.address).map_err(|error| error.to_string())?, write: transaction.operation == "write",
                value: u32::try_from(transaction.value).map_err(|error| error.to_string())? };
            match self.innermost(transaction.cpu.min(1)) {
                Some((_, scope)) => self.calls[scope].bus.push(row),
                None if row.write => self.unattributed.push(row),
                None => {}
            }
        }
        Ok(())
    }

    /// Each CPU access to a compared record word belongs to every call open on its core in the innermost call's
    /// context: a call spans its callees and callbacks, not an interrupt that preempts it.
    fn accesses(&mut self, batch: &AccessBatch) -> Result<(), String> {
        for row in &batch.records {
            let write = match row.source { Access::CpuRead => false, Access::CpuWrite => true, _ => continue };
            let low = u32::try_from(row.physical).map_err(|error| error.to_string())? & !ALIAS;
            let high = low.checked_add(u32::try_from(row.size).map_err(|error| error.to_string())?).ok_or("access row overflows")?;
            if !in_regions(low & !3) && !in_regions((high - 1) & !3) { continue; }
            let core = row.core.ok_or("CPU access row carries no core")?.min(1);
            let Some((innermost, _)) = self.innermost(core) else { continue };
            let context = self.calls[innermost].context();
            let calls = &mut self.calls;
            for open in self.open[core].iter().rev() {
                let Open::Frame { call, .. } = open else { continue };
                if calls[*call].context() != context { break; }
                let words = if write { &mut calls[*call].writes } else { &mut calls[*call].reads };
                let mut word = low & !3;
                while word < high {
                    if in_regions(word) { words.insert(word); }
                    word += 4;
                }
            }
        }
        Ok(())
    }

    /// A commit's register writes end at the first callback into, or return from, its scope.
    fn sample(&mut self, machine: &Machine, core: usize) -> Result<(), String> {
        let Some((_, scope)) = self.innermost(core) else { return Ok(()) };
        let Some(engine) = self.calls[scope].engine else { return Ok(()) };
        let Some(Open::Frame { sampled, .. }) = self.open[core].iter_mut().find(|open| matches!(open, Open::Frame { call, .. } if *call == scope)) else { return Ok(()) };
        if *sampled { return Ok(()); }
        *sampled = true;
        self.calls[scope].saved = words(machine, saved(engine.min(1)), 0x1a4 / 4)?;
        self.calls[scope].before_drain = self.calls[scope].bus.len();
        Ok(())
    }

    fn finish(&mut self, machine: &Machine, core: usize, open: Open) -> Result<(), String> {
        match open {
            Open::Frame { call, before, .. } => {
                let after = region_words(machine)?;
                let record = &mut self.calls[call];
                record.changes = before.iter().zip(&after).filter(|(old, new)| old.1 != new.1).map(|(old, new)| [old.0, old.1, new.1]).collect();
                record.result = Some(register(machine, core, 0)?);
                record.returned = true;
                record.instructions[1] = machine.instructions();
                if let (Entry::SchedulerRegister, [_, output]) = (record.entry, record.arguments.as_slice()) {
                    record.output = Some(core_words(machine, core, *output, 1)?[0]);
                }
                if let (Entry::ItemConfiguration, Some([previous, _]), [engine, group, item, _]) = (record.entry, record.item, record.arguments.as_slice()) {
                    record.item = Some([previous, machine.word(u64::from(saved(*engine) + 0x64 + group * 0x4c + item * 16))?]);
                }
            }
            Open::Callback { call, callback, .. } => {
                let after = drain_words(machine, &self.calls[call])?;
                self.calls[call].callbacks[callback].after = Some(after);
            }
        }
        Ok(())
    }

    /// Processes one stop at `raw` on `core`; returns the code words to watch next.
    fn hit(&mut self, machine: &Machine, core: usize, raw: u32) -> Result<Vec<u32>, String> {
        let pc = raw & !ALIAS;
        let (sp, lr) = (register(machine, core, 13)?, register(machine, core, 14)?);
        let returning = self.open[core].last().is_some_and(|open| open.returns_at(raw, sp));
        let entry = Entry::from_address(pc);
        let top = match self.open[core].last() { Some(Open::Frame { call, scope, .. }) => Some((*call, *scope)), _ => None };
        let mut sites = Vec::new();
        let queued = |call: &Call| call.slots.contains(&raw) || call.callbacks.iter().any(|callback| callback.after.as_ref().is_some_and(|after| after.contains(&raw)));
        let callback = !returning && top.is_some_and(|(_, scope)| self.calls[scope].entry.drains() && queued(&self.calls[scope]))
            && Caller::of(lr & !ALIAS, self.body) == Caller::Unit;
        if returning || callback { self.sample(machine, core)?; }
        if returning {
            while self.open[core].last().is_some_and(|open| open.returns_at(raw, sp)) {
                let open = self.open[core].pop().ok_or("open call vanished")?;
                self.finish(machine, core, open)?;
            }
            return Ok(sites);
        }
        if callback && let Some((_, scope)) = top {
            let live = drain_words(machine, &self.calls[scope])?;
            self.calls[scope].callbacks.push(Callback { target: raw, live, after: None });
            let callback = self.calls[scope].callbacks.len() - 1;
            self.open[core].push(Open::Callback { call: scope, callback, lr, sp });
            sites.push(lr);
        }
        if let Some(entry) = entry {
            let arguments = (0..entry.arity()).map(|number| register(machine, core, number as u8)).collect::<Result<Vec<_>, _>>()?;
            let caller = Caller::of(lr & !ALIAS, self.body);
            let engine = entry.committed_engine(&arguments);
            let index = self.calls.len();
            let top = match self.open[core].last() { Some(Open::Frame { call, scope, .. }) => Some((*call, *scope)), _ => None };
            let (head, scope) = match (engine, top) {
                (Some(_), Some((below, scope))) if self.calls[below].engine.is_some() => (false, scope),
                (Some(_), _) => (true, index),
                (None, Some((_, scope))) if caller == Caller::Unit => (false, scope),
                (None, _) => (false, index),
            };
            let item = match (entry, arguments.as_slice()) {
                (Entry::ItemConfiguration, [engine, group, item, _]) if *engine <= 1 && *group <= 1 && *item <= 3 =>
                    Some([machine.word(u64::from(saved(*engine) + 0x64 + group * 0x4c + item * 16))?, 0]),
                _ => None,
            };
            let mut call = Call {
                entry, caller, head, engine, arguments, result: None,
                selection: [machine.word(u64::from(SELECTION + 0x10))?, machine.word(u64::from(SELECTION + 0x28))?], slots: Vec::new(),
                pending: match engine { Some(engine) if head => machine.word(u64::from(runtime(engine.min(1)) + 0x0c))?, _ => 0 },
                front: match engine { Some(engine) if head => Some(front_state(machine, engine)?), _ => None },
                saved: Vec::new(), before_drain: 0, bus: Vec::new(), callbacks: Vec::new(), changes: Vec::new(), item, returned: false,
                request: None, output: None, stack: sp, entry_words: Vec::new(), reads: BTreeSet::new(), writes: BTreeSet::new(),
                instructions: [machine.instructions(), 0],
            };
            call.slots = drain_words(machine, &call)?;
            if let (Entry::SchedulerRegister, [request, _]) = (entry, call.arguments.as_slice()) {
                let words = core_words(machine, core, *request, 4)?;
                call.request = Some([words[0], words[1], words[2], words[3]]);
            }
            let before = region_words(machine)?;
            call.entry_words = before.iter().map(|(_, value)| *value).collect();
            sites.extend(call.slots.iter().copied().filter(|word| *word != 0));
            self.calls.push(call);
            self.open[core].push(Open::Frame { call: index, scope, lr, sp, before, sampled: false });
            sites.push(lr);
        }
        Ok(sites)
    }
}

#[derive(Clone, Deserialize, Eq, PartialEq, Serialize)]
struct Checkpoint { label: String, waits: u32, menu: Option<[u32; 3]>, frame_sha256: Option<String>, front: FrontState }

/// Where one route step ended.
#[derive(Clone, Copy, Deserialize, Serialize)]
struct RoutePoint { tick: u64, instructions: u64 }

#[derive(Deserialize, Serialize)]
struct Receipt {
    schema: u32, role: Role, complete: bool, error: Option<String>, machine_failure: Option<String>,
    contract_sha256: String, update_sha256: String, run_sha256: String, rom_sha256: String, observer_sha256: String,
    /// The stock receipt whose route ticks an owned role replayed.
    schedule_sha256: Option<String>,
    instructions: u64, points: Vec<RoutePoint>, checkpoints: Vec<Checkpoint>,
    guard_carrier_fetches: u64, guard_body_fetches: u64, guard_installation_writes: u64, guard_carriers_match: bool,
    calls: usize, compared_calls: usize, open_calls: usize, entry_counts: BTreeMap<String, u64>,
    calls_sha256: String, unattributed_writes: usize, unattributed_sha256: String,
}

struct Run<'a> {
    inputs: &'a Inputs,
    machine: Machine,
    observer: Observer,
    watch: Option<Box<MemoryWatch>>,
    maps: Vec<MemoryMap>,
    points: Vec<RoutePoint>,
    checkpoints: Vec<Checkpoint>,
}

impl Run<'_> {
    /// Attributes the bus and record-access rows and admits the guard rows since the previous stop.
    fn drain(&mut self, restart: bool) -> Result<(), String> {
        let board = &self.machine.session.board;
        let batch = board.transaction_watch_end()?;
        let batch: TransactionBatch = serde_json::from_value(serde_json::to_value(&batch).map_err(|error| error.to_string())?).map_err(|error| error.to_string())?;
        self.observer.bus(batch)?;
        let mut active = self.watch.take().ok_or("guard watch interval absent")?;
        let bytes = serde_json::to_vec(&active.records.borrow().state()).map_err(|error| error.to_string())?;
        board.memory_watch_end(&mut active)?;
        let records: AccessBatch = serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
        self.observer.guard.batch(&records, self.inputs.role, &self.inputs.entries, self.inputs.body)?;
        self.observer.accesses(&records)?;
        if restart { self.begin()?; }
        Ok(())
    }

    fn begin(&mut self) -> Result<(), String> {
        let board = &self.machine.session.board;
        board.transaction_watch_begin(vec![DISPLAY_BANKS], BUS_LIMIT)?;
        self.watch = Some(board.memory_watch_begin_classified(guard_ranges(self.inputs.body), WATCH_LIMIT, self.maps.clone(), ViewPolicy::Observe)?);
        Ok(())
    }

    /// One route step: `budget` instructions in the stock role; in an owned role, until the board tick the stock
    /// role's step ended at, so both roles reach each route point at the same guest time.
    fn advance(&mut self, budget: u64) -> Result<(), String> {
        let (target, until) = match &self.inputs.schedule {
            None => (self.machine.instructions().checked_add(budget).ok_or("route budget overflow")?, None),
            Some(Schedule { ticks, .. }) => {
                let tick = *ticks.get(self.points.len()).ok_or("the stock route has fewer points")?;
                let bound = self.machine.hold_bound(tick.saturating_sub(self.machine.session.board.now()) + 1);
                (self.machine.instructions().checked_add(bound).ok_or("route budget overflow")?, Some(tick))
            }
        };
        let reached = |machine: &Machine| machine.instructions() >= target || until.is_some_and(|tick| machine.session.board.now() >= tick);
        while !reached(&self.machine) {
            let outcome = self.machine.run_until(target - self.machine.instructions(), until);
            self.machine.require_running()?;
            if outcome == RunOutcome::Completed { continue; }
            require(outcome == RunOutcome::Stopped, "unexpected scanout route run outcome")?;
            let hits: Vec<_> = self.machine.traces.hits.drain(..).collect();
            require(hits.len() == 1 && hits[0].core < 2, "scanout stop carries other than one hit")?;
            let core = hits[0].core;
            self.drain(true)?;
            let machine = &self.machine;
            let pc = register(machine, core, 15)?;
            let mut sites = self.observer.hit(machine, core, pc)?;
            for engine in 0..2 { sites.extend(words(machine, runtime(engine) + 0x24, 12)?); }
            sites.extend(scheduler_words(machine)?);
            let armed = &mut self.observer.armed;
            let new: BTreeMap<u32, String> = sites.into_iter().filter(|site| *site != 0 && (*site & !ALIAS) < 0x0200_0000)
                .flat_map(|site| [site, site ^ ALIAS]).filter(|site| armed.insert(*site)).map(|site| (site, LABEL.into())).collect();
            if !new.is_empty() { self.machine.traces.add_sites(&self.machine.session, new)?; }
        }
        match until {
            None => require(self.machine.instructions() == target, "route instruction endpoint differs")?,
            Some(tick) => require(self.machine.session.board.now() >= tick, "route tick endpoint not reached within its instruction bound")?,
        }
        self.points.push(RoutePoint { tick: self.machine.session.board.now(), instructions: self.machine.instructions() });
        Ok(())
    }

    fn key(&mut self, key: RouteKey) -> Result<(), String> {
        self.machine.press(key)?;
        self.advance(KEY_HOLD)?;
        self.machine.release_key(key)?;
        self.advance(KEY_HOLD)
    }

    fn checkpoint(&mut self, label: String, binding: Binding) -> Result<(), String> {
        let replay = match &self.inputs.schedule {
            None => None,
            Some(Schedule { waits, .. }) => Some(*waits.get(self.checkpoints.len()).ok_or("the stock route has fewer checkpoints")?),
        };
        let (waits, frame_sha256) = bind_frame(self, binding, replay)?;
        let machine = &self.machine;
        let engine = selected_engine([machine.word(u64::from(SELECTION + 0x10))?, machine.word(u64::from(SELECTION + 0x28))?]);
        let checkpoint = Checkpoint { label, waits, menu: menu(machine)?, frame_sha256, front: front_state(machine, engine)? };
        self.checkpoints.push(checkpoint);
        Ok(())
    }

    /// The natural no-card route of the general-heap pair, then four rows down and back up the shooting menu.
    fn route(&mut self) -> Result<(), String> {
        self.advance(80_000_000)?;
        self.checkpoint("setup".into(), Binding::Frame)?;
        for (number, key) in std::iter::repeat_n(RouteKey::Up, 12).chain([RouteKey::Ok, RouteKey::Ok]).enumerate() {
            self.key(key)?;
            self.checkpoint(format!("setup-{number}"), Binding::Frame)?;
        }
        self.advance(40_000_000)?;
        self.key(RouteKey::Ok)?;
        self.advance(90_000_000)?;
        self.checkpoint("shooting-menu".into(), Binding::Frame)?;
        for (number, key) in std::iter::repeat_n(RouteKey::Down, 4).chain(std::iter::repeat_n(RouteKey::Up, 4)).enumerate() {
            self.key(key)?;
            self.checkpoint(format!("shooting-{number}"), Binding::Frame)?;
        }
        self.advance(40_000_000)?;
        self.checkpoint("final".into(), Binding::FrameBetweenCalls)
    }
}

impl Presenting for Run<'_> {
    fn presented(&mut self) -> Result<Option<String>, String> { Ok(self.machine.frame()?.map(|bytes| sha256(&bytes))) }
    fn open_calls(&self) -> usize { self.observer.open.iter().map(Vec::len).sum() }
    fn wait(&mut self) -> Result<(), String> { self.advance(FRAME_WAIT) }
}

fn execute(inputs: &Inputs, output: &Path) -> Result<(), String> {
    std::fs::create_dir(output).map_err(|error| error.to_string())?;
    let document = RunDocument::parse(&inputs.run)?;
    let maps = run::memory_maps(&document)?;
    let mut session = run::open(&document, &inputs.dat, &inputs.rom)?;
    session.board.set_execution_policy(document.execution_policy.into())?;
    let mut machine = Machine::new(document, session)?;
    machine.disarm()?;
    let checkpoint = machine.session.checkpoint(None)?.export()?;
    let reset = machine.session.import_snapshot(&checkpoint)?;
    machine.session.restore(&reset)?;
    machine.rearm()?;
    require(machine.instructions() == 0, "scanout pair checkpoint is not reset")?;
    let armed: BTreeSet<u32> = Entry::ALL.into_iter().flat_map(|entry| [entry.address(), entry.address() | ALIAS]).collect();
    machine.traces.bound_full_registers(std::num::NonZeroUsize::new(HIT_LIMIT).ok_or("invalid hit limit")?);
    machine.traces.arm(&machine.session, armed.iter().map(|address| (*address, LABEL.into())).collect())?;
    machine.traces.stop_labels = [LABEL.into()].into_iter().collect();
    let observer = Observer { body: inputs.body, calls: Vec::new(), open: [Vec::new(), Vec::new()], unattributed: Vec::new(), guard: Guard::default(), armed };
    let mut run = Run { inputs, machine, observer, watch: None, maps, points: Vec::new(), checkpoints: Vec::new() };
    run.begin()?;
    let result = run.route();
    let drained = run.drain(false);
    let Run { machine, observer, points, checkpoints, .. } = run;
    let mut carriers_match = true;
    for (low, high) in CARRIERS {
        let offset = (low - IMAGE4) as usize;
        carriers_match &= machine.read(u64::from(low), (high - low) as usize)? == inputs.image4[offset..offset + (high - low) as usize];
    }
    let guard = &observer.guard;
    let executed = match inputs.role { Role::Stock => guard.carrier_fetches > 0 && guard.body_fetches == 0, Role::Owned => guard.carrier_fetches > 0 && guard.body_fetches > 0 };
    let open_calls = observer.open.iter().map(Vec::len).sum();
    let calls_bytes = serde_json::to_vec(&observer.calls).map_err(|error| error.to_string())?;
    let unattributed_bytes = serde_json::to_vec(&observer.unattributed).map_err(|error| error.to_string())?;
    write(&output.join("calls.json"), &calls_bytes)?;
    write(&output.join("unattributed.json"), &unattributed_bytes)?;
    write(&output.join("fetches.json"), &serde_json::to_vec(&observer.guard.fetches).map_err(|error| error.to_string())?)?;
    let mut entry_counts = BTreeMap::new();
    for call in &observer.calls { *entry_counts.entry(serde_json::to_value(call.entry).map_err(|error| error.to_string())?.as_str().unwrap_or("").to_owned()).or_insert(0) += 1; }
    let error = result.err().or(drained.err())
        .or_else(|| (!carriers_match).then(|| "retired body bytes differ from the role's image 4".into()))
        .or_else(|| (!executed).then(|| "the role did not execute the code its image publishes".into()));
    let receipt = Receipt {
        schema: RECEIPT_SCHEMA, role: inputs.role, complete: error.is_none() && machine.failure.is_none(),
        error, machine_failure: machine.failure.clone(), contract_sha256: inputs.contract_sha256.clone(), update_sha256: sha256(&inputs.dat.0),
        run_sha256: sha256(&inputs.run), rom_sha256: sha256(&inputs.rom),
        observer_sha256: sha256(&read(&std::env::current_exe().map_err(|error| error.to_string())?)?),
        schedule_sha256: inputs.schedule.as_ref().map(|schedule| schedule.digest.clone()),
        instructions: machine.instructions(), points, checkpoints, guard_carrier_fetches: guard.carrier_fetches, guard_body_fetches: guard.body_fetches,
        guard_installation_writes: guard.installation_writes, guard_carriers_match: carriers_match,
        calls: observer.calls.len(), compared_calls: observer.calls.iter().filter(|call| call.compared()).count(), open_calls,
        entry_counts, calls_sha256: sha256(&calls_bytes), unattributed_writes: observer.unattributed.len(), unattributed_sha256: sha256(&unattributed_bytes),
    };
    write(&output.join("receipt.json"), &serde_json::to_vec_pretty(&receipt).map_err(|error| error.to_string())?)?;
    require(receipt.complete, "scanout role refused; receipt retained")
}

fn check(stock_dir: &Path, owned_dir: &Path) -> Result<serde_json::Value, String> {
    let receipt = |directory: &Path| -> Result<Receipt, String> { serde_json::from_slice(&read(&directory.join("receipt.json"))?).map_err(|error| error.to_string()) };
    let calls = |directory: &Path| -> Result<Vec<Call>, String> { serde_json::from_slice(&read(&directory.join("calls.json"))?).map_err(|error| error.to_string()) };
    let unattributed = |directory: &Path| -> Result<Vec<Bus>, String> { serde_json::from_slice(&read(&directory.join("unattributed.json"))?).map_err(|error| error.to_string()) };
    let (stock, owned) = (receipt(stock_dir)?, receipt(owned_dir)?);
    let (stock_calls, owned_calls) = (calls(stock_dir)?, calls(owned_dir)?);
    let mut reasons = Vec::new();
    if stock.role != Role::Stock || owned.role != Role::Owned { reasons.push("roles are not stock and owned".to_owned()); }
    if stock.schema != RECEIPT_SCHEMA || owned.schema != RECEIPT_SCHEMA { reasons.push("a receipt predates entry-word and read sampling".to_owned()); }
    for (name, role) in [("stock", &stock), ("owned", &owned)] {
        if !role.complete { reasons.push(format!("{name} role is incomplete: {:?}", role.error)); }
    }
    let stock_receipt_sha256 = sha256(&read(&stock_dir.join("receipt.json"))?);
    let replayed = owned.schedule_sha256.as_deref() == Some(stock_receipt_sha256.as_str()) && owned.points.len() == stock.points.len()
        && owned.points.iter().zip(&stock.points).all(|(owned, stock)| owned.tick >= stock.tick);
    if (stock.run_sha256.as_str(), stock.rom_sha256.as_str()) != (owned.run_sha256.as_str(), owned.rom_sha256.as_str()) || !replayed {
        reasons.push("roles ran different inputs, or the owned role did not replay this stock role's route ticks".into());
    }
    let (stock_laws, owned_laws) = (laws(&stock_calls), laws(&owned_calls));
    if !stock_laws.is_empty() { reasons.push(format!("stock role breaks {} scanout laws", stock_laws.len())); }
    if !owned_laws.is_empty() { reasons.push(format!("owned role breaks {} scanout laws", owned_laws.len())); }
    let menu_at = |label: &str| stock.checkpoints.iter().find(|checkpoint| checkpoint.label == label).and_then(|checkpoint| checkpoint.menu);
    if menu_at("setup") != Some([69, 52, 12]) || menu_at("shooting-menu") != Some([1, 5, 2]) {
        reasons.push("stock role did not reach the setup and shooting-menu endpoints".into());
    }
    if stock.checkpoints.iter().any(|checkpoint| checkpoint.frame_sha256.is_none()) { reasons.push("a stock checkpoint has no rendered frame".into()); }
    let checkpoints_equal = stock.checkpoints == owned.checkpoints;
    if !checkpoints_equal { reasons.push("menus, rendered frames or front-buffer state differ at a route checkpoint".into()); }
    let unattributed_equal = unattributed(stock_dir)? == unattributed(owned_dir)?;
    if !unattributed_equal { reasons.push("retained display-bank writes differ".into()); }
    let difference = first_difference(&stock_calls, &owned_calls);
    if difference.is_some() { reasons.push("compared calls differ".into()); }
    let heads = |calls: &[Call]| calls.iter().filter(|call| call.head && call.returned).count();
    let pending = |calls: &[Call]| calls.iter().filter(|call| call.head && call.returned && call.pending == 1).count();
    let drained = |calls: &[Call]| calls.iter().filter(|call| call.head).map(|call| call.callbacks.len()).sum::<usize>();
    let summary = |role: &Receipt, calls: &[Call], findings: &[Finding]| serde_json::json!({
        "complete": role.complete, "update_sha256": role.update_sha256, "contract_sha256": role.contract_sha256, "calls_sha256": role.calls_sha256,
        "compared_calls": role.compared_calls, "interrupt_calls": in_context(calls, Context::Interrupt).len(), "task_calls": in_context(calls, Context::Task).len(),
        "commits": heads(calls), "pending_commits": pending(calls), "drained_callbacks": drained(calls),
        "instructions": role.instructions, "route_end_tick": role.points.last().map(|point| point.tick),
        "carrier_fetches": role.guard_carrier_fetches, "body_fetches": role.guard_body_fetches, "entry_counts": role.entry_counts,
        "instruction_spans": instruction_spans(calls), "laws": findings,
    });
    Ok(serde_json::json!({
        "schema": 2, "verdict": if reasons.is_empty() { "equal" } else { "refused" }, "reasons": reasons,
        "stock": summary(&stock, &stock_calls, &stock_laws), "owned": summary(&owned, &owned_calls, &owned_laws),
        "checkpoints": stock.checkpoints.len(), "checkpoints_equal": checkpoints_equal, "unattributed_writes_equal": unattributed_equal,
        "stack_excess_bytes": stack_excess(&stock_calls, &owned_calls), "difference": difference,
        "cross_context_order_difference": cross_context_order(&stock_calls, &owned_calls),
    }))
}

fn qualify(arguments: &[String]) -> Result<(), String> {
    let stock = Path::new(&arguments[1]);
    let positive = check(stock, Path::new(&arguments[2]))?;
    let control = |directory: &str, law: &str| -> Result<serde_json::Value, String> {
        let report = check(stock, Path::new(directory))?;
        let returned = report["owned"]["complete"] == true;
        let named = report["owned"]["laws"].as_array().is_some_and(|laws| laws.iter().any(|finding| finding["law"] == law));
        Ok(serde_json::json!({ "expected_law": law, "returned": returned, "detected": report["verdict"] == "refused" && named, "report": report }))
    };
    let controls = serde_json::json!({ "register-order": control(&arguments[3], "register-sequence")?, "skipped-drain": control(&arguments[4], "drain")? });
    let qualified = positive["verdict"] == "equal"
        && ["register-order", "skipped-drain"].iter().all(|name| controls[name]["returned"] == true && controls[name]["detected"] == true);
    let report = serde_json::json!({ "schema": 2, "qualified": qualified, "positive": positive, "controls": controls });
    write(Path::new(&arguments[5]), &serde_json::to_vec_pretty(&report).map_err(|error| error.to_string())?)?;
    require(qualified, "scanout cutover is not qualified")
}

fn drive() -> Result<(), String> {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    match arguments.first().map(String::as_str) {
        Some("stock") if arguments.len() == 7 => {
            let inputs = load(&arguments)?;
            fujimod_emulator_exr2_board::register_board_builder();
            fujimod_emulator_models::register();
            execute(&inputs, Path::new(&arguments[6]))
        }
        Some("owned") if arguments.len() == 8 => {
            let inputs = load(&arguments)?;
            fujimod_emulator_exr2_board::register_board_builder();
            fujimod_emulator_models::register();
            execute(&inputs, Path::new(&arguments[7]))
        }
        Some("check") if arguments.len() == 4 => {
            let report = check(Path::new(&arguments[1]), Path::new(&arguments[2]))?;
            write(Path::new(&arguments[3]), &serde_json::to_vec_pretty(&report).map_err(|error| error.to_string())?)?;
            require(report["verdict"] == "equal", "stock and owned scanout roles differ")
        }
        Some("qualify") if arguments.len() == 6 => qualify(&arguments),
        _ => Err(USAGE.into()),
    }
}

fn main() -> ExitCode {
    match drive() { Ok(()) => ExitCode::SUCCESS, Err(error) => { eprintln!("{error}"); ExitCode::FAILURE } }
}
