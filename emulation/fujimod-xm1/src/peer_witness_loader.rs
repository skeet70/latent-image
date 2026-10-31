//! Schema-4 observation of the loader-B core-1 prefix, retained or owned, from
//! the paused record-2 handoff to pre-execution 0xd0aac under the scoped
//! memory watch, with classified watches from reset to the handoff.
use super::*;
use super::wait::{self, BoardState, CoreCounts, require};
use fujimod_catalog::configuration::RunEngine;
use fujimod_emulator::board::observation::scoped::{
    Confinement, ConfinementRule, CoreSet, CounterRule, CoverageRule, PhysicalWord, SampleBound, ScopeSpan,
    ScopedMemoryWatch, ScopedWatch, TcmBank, TcmRange, WordRange,
};
use fujimod_emulator::board::observation::{ViewPolicy, WatchKinds, WatchRange};
use fujimod_emulator::board::preservation::PhysicalRange;
use fujimod_emulator_cpu::Access;
use crate::witness::encode_hex;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

const BUDGET: u64 = 50_000_000;
const QUANTUM: u64 = 1_000;
const ARGUMENT: u32 = 0x69cdf8;
const RECORD1_ENTRY: u32 = 0x1800;
const CELL: u64 = 0xa00c4;
const CELL_GUEST: u64 = 0x400a_00c4;
const RECORD: (u64, u64) = (0x10000, 0x14c80);
const LOADER_B: (u64, u64) = (0xd0000, 0xddaa0);
const ZERO: (u64, u64) = (0xddaa0, 0xf3420);
const ZERO_GUEST: u64 = 0x4000_0000;
const ZERO_CLAIM_END: u64 = 0xf33ec;
const ENTRY_POOL: (u64, u64) = (0xd00c8, 0xd00fc);
const REPLACED: [(u64, u64); 3] = [(0xd0020, 0xd00c8), (0xd09ac, 0xd09d0), (0xd0a44, 0xd0a64)];
const LOADER_A_COMPLETION: (u64, u64) = (0x40a00, 0x40a10);
const LOADER_A_LITERAL: (u64, u64) = (0x40afc, 0x40b00);
const VENEER: (u64, u64) = (0x180, 0x19c);
const VECTOR_SLOTS: u64 = 0xffff_0024;
const VECTOR_BYTES: usize = 28;
const FRAME: u64 = 0x400f_3410;
const EXIT: u32 = 0xd0aac;
const HELD_GATE: u32 = 0x40a00;
const DELAY_ENTRY: u32 = 0x180;
const DELAY_RELEASE: u32 = 0x194;
const STORE: u32 = 0x40a08;
/// Stock 0xd0094 pushes four words and 0xd34f0 nine on the entry stack.
const ENTRY_STACK_BYTES: u64 = 52;
const HEAD_POLLS: usize = 4;
const HEAD_BOUND: u8 = 8;
const TAIL_BOUND: u8 = 2;
const MICROSTEPS: usize = 2048;
const MAX_STOPS: usize = 64;
const RETAINED_RECORD: &str = "2f1bf3f0e483b892d2890599679c1b515cfc1be8b8c27cbb06d4d913e428abb4";
const LOADER_B_DIGEST: &str = "8fb0c322a22443a3783cdab9071db17212a5629f6375646218ac1e043dc672ec";

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
enum Scenario { Natural, Held, Delayed, NonOne }

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct ScenarioWire { kind: Scenario }

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
enum ProgramKind { Retained, Owned }

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProgramWire {
    kind: ProgramKind,
    record_sha256: String,
    loader_sha256: String,
    handoff: Guest,
    entry: Guest,
    load: Guest,
    post_load: Guest,
    loaded_register: u8,
    poll_period: u64,
    image: Option<(Guest, Guest)>,
    refusal: Option<Guest>,
    forbid_fetch: Vec<(u64, u64)>,
    forbid_read: Vec<(u64, u64)>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct LoaderWire {
    schema: u32,
    budget: Instructions,
    scenario: ScenarioWire,
    program: ProgramWire,
}

pub(super) struct LoaderContract {
    scenario: Scenario,
    kind: ProgramKind,
    record: String,
    loader: String,
    handoff: u32,
    entry: u32,
    post_load: u32,
    loaded: u8,
    refusal: Option<u32>,
}
impl LoaderContract {
    pub(super) fn budget(&self) -> Instructions {
        Instructions(BUDGET)
    }
}

fn hex_digest(text: &str) -> Result<String, String> {
    require(text.len() == 64 && text.bytes().all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f')),
        "loader-prefix digest must be 64 lowercase hexadecimal characters")?;
    Ok(text.into())
}

impl LoaderWire {
    pub(super) fn validate(self) -> Result<LoaderContract, String> {
        require(self.schema == 4 && self.budget.0 == BUDGET, "loader-prefix witness requires schema 4 and budget 50000000")?;
        let program = self.program;
        require((1..=100_000).contains(&program.poll_period) && program.loaded_register <= 12,
            "loader-prefix poll period or loaded register is out of range")?;
        match program.kind {
            ProgramKind::Retained => require(program.record_sha256 == RETAINED_RECORD && program.loader_sha256 == LOADER_B_DIGEST
                && program.handoff.0 == 0x11850 && program.entry.0 == 0xd0020 && program.load.0 == 0xd0a44
                && program.post_load.0 == 0xd0a48 && program.loaded_register == 3 && program.image.is_none()
                && program.refusal.is_none() && program.forbid_fetch.is_empty() && program.forbid_read.is_empty(),
                "retained loader-prefix program differs from the stock sites")?,
            ProgramKind::Owned => {
                let (start, end) = program.image.map(|(start, end)| (start.0, end.0)).ok_or("owned loader prefix lacks its image")?;
                require(start < end && start % 4 == 0 && end % 4 == 0 && start >= 0x103e8 && end <= 0x11800,
                    "owned loader-prefix image lies outside record-2 padding")?;
                let sites: Vec<u32> = [program.handoff, program.entry, program.load, program.post_load].into_iter()
                    .chain(program.refusal).map(|site| site.0).collect();
                require(sites.iter().all(|site| site % 4 == 0 && (start..end).contains(site))
                    && sites.iter().collect::<BTreeSet<_>>().len() == sites.len(), "owned loader-prefix sites must be distinct aligned image addresses")?;
                let fetch: BTreeSet<(u64, u64)> = program.forbid_fetch.iter().copied().collect();
                require(program.loaded_register == 0 && fetch == REPLACED.iter().copied().collect()
                    && program.forbid_read == [ENTRY_POOL], "owned loader-prefix register or forbidden spans differ")?;
            }
        }
        Ok(LoaderContract {
            scenario: self.scenario.kind, kind: program.kind,
            record: hex_digest(&program.record_sha256)?, loader: hex_digest(&program.loader_sha256)?,
            handoff: program.handoff.0, entry: program.entry.0, post_load: program.post_load.0,
            loaded: program.loaded_register, refusal: program.refusal.map(|site| site.0),
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum Label {
    #[serde(rename = "record-1-entry")]
    Record1Entry,
    #[serde(rename = "record-2-handoff")]
    Record2Handoff,
    PrefixEntry,
    PostLoad,
    HeldGate,
    DelayEntry,
    DelayRelease,
    CompletionStore,
    PrefixExit,
    Refusal,
}
impl Label {
    fn name(self) -> &'static str {
        match self {
            Self::Record1Entry => "record-1-entry",
            Self::Record2Handoff => "record-2-handoff",
            Self::PrefixEntry => "prefix-entry",
            Self::PostLoad => "post-load",
            Self::HeldGate => "held-gate",
            Self::DelayEntry => "delay-entry",
            Self::DelayRelease => "delay-release",
            Self::CompletionStore => "completion-store",
            Self::PrefixExit => "prefix-exit",
            Self::Refusal => "refusal",
        }
    }
    fn core(self) -> WitnessCore {
        match self {
            Self::Record1Entry | Self::HeldGate | Self::DelayEntry | Self::DelayRelease | Self::CompletionStore => WitnessCore::Core0,
            _ => WitnessCore::Core1,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
struct Totals { count: u64, zero_count: u64, nonzero_count: u64 }

/// The scoped state fields read at stops.
#[derive(Deserialize)]
struct ScopedView {
    violation: Option<String>,
    records: Vec<Value>,
    counters: Vec<Totals>,
}

/// The classified state fields read at epoch ends.
#[derive(Deserialize)]
struct ClassifiedView {
    violation: Option<String>,
}

type TcmWords = [[Word; 2]; 2];

#[derive(Serialize)]
struct LoaderStop {
    label: Label,
    address: Guest,
    core: WitnessCore,
    instructions: Instructions,
    counts: CoreCounts,
    registers: [Word; 13],
    sp: Word,
    lr: Word,
    cpsr: Word,
    completion: Word,
    tcm: TcmWords,
    counter: Option<Totals>,
    watch_records: Option<usize>,
}

#[derive(Serialize)]
struct BoundMap { core: usize, address: Address, physical: Address, size: u64 }
#[derive(Serialize)]
struct Bound { guest: Address, size: u64, map: BoundMap }
impl Bound {
    fn physical(&self) -> u64 {
        self.map.physical.0 + self.guest.0 - self.map.address.0
    }
}
#[derive(Serialize)]
struct Views { r#virtual: Vec<u8>, physical: Vec<u8> }
#[derive(Serialize)]
struct Bindings {
    vector_slots: Bound,
    stack_window: Value,
    zero_range: Bound,
    veneer: Option<Views>,
}
#[derive(Serialize)]
struct Microstep { before: CoreCounts, after: CoreCounts, cell: Word }
#[derive(Serialize)]
struct WordViews { r#virtual: Word, physical: Word }
#[derive(Serialize)]
struct CommittedStore {
    instruction: WordViews,
    before_pc: Word,
    after_pc: Word,
    before_cpsr: Word,
    after_cpsr: Word,
    microsteps: Vec<Microstep>,
    readback: WordViews,
    counter: Totals,
    watch_records: usize,
}
#[derive(Serialize)]
struct ZeroReadback { range: (Address, Address), first_nonzero: Option<Address> }
#[derive(Serialize)]
struct Endpoint {
    frame: Views,
    vectors: Views,
    zero: ZeroReadback,
    record_sha256: String,
    loader_sha256: String,
}

#[derive(Serialize)]
struct Receipt<'a> {
    schema: u32,
    scenario: ScenarioWire,
    program: ProgramKind,
    result: WitnessResult,
    error: Option<&'a str>,
    machine_failure: Option<&'a str>,
    instructions: Instructions,
    cp14_identities: [Identity; 2],
    boot_watch: &'a Value,
    loaded_watch: &'a Value,
    scoped_watch: &'a Option<Value>,
    stops: &'a [LoaderStop],
    bindings: &'a Option<Bindings>,
    completion: &'a Option<CommittedStore>,
    endpoint: &'a Option<Endpoint>,
    final_primary: &'a Option<wait::Primary>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Epoch { Boot, Loaded, Scoped }

#[derive(Default)]
struct Phase {
    entry: bool,
    posts: usize,
    gate: bool,
    posts_after_gate: usize,
    last_post: Option<u64>,
    delay_entry: Option<usize>,
    released: bool,
    stored: bool,
    exit: bool,
}

fn tcm_words(machine: &Machine) -> Result<TcmWords, String> {
    Ok([
        [wait::tcm(machine, WitnessCore::Core0, wait::TcmBank::Atcm)?, wait::tcm(machine, WitnessCore::Core0, wait::TcmBank::Btcm)?],
        [wait::tcm(machine, WitnessCore::Core1, wait::TcmBank::Atcm)?, wait::tcm(machine, WitnessCore::Core1, wait::TcmBank::Btcm)?],
    ])
}

/// Cortex-R TCM region register: base [31:12], size code [6:2], enable [0].
fn tcm_window(register: Word) -> Option<(u64, u64)> {
    let code = (register.0 >> 2) & 0x1f;
    (register.0 & 1 != 0 && code != 0).then(|| {
        let base = u64::from(register.0 & 0xffff_f000);
        (base, base + (512u64 << code))
    })
}

fn digest_hex(bytes: &[u8]) -> String {
    encode_hex(&Sha256::digest(bytes))
}

struct Runner<'a> {
    contract: &'a LoaderContract,
    maps: Vec<factory::MemoryMap>,
    sites: BTreeMap<u32, Label>,
    epoch: Epoch,
    classified: Option<Box<MemoryWatch>>,
    scoped: Option<Box<ScopedMemoryWatch>>,
    boot_watch: Value,
    loaded_watch: Value,
    scoped_watch: Option<Value>,
    stops: Vec<LoaderStop>,
    bindings: Option<Bindings>,
    completion: Option<CommittedStore>,
    endpoint: Option<Endpoint>,
    final_primary: Option<wait::Primary>,
    phase: Phase,
}

impl<'a> Runner<'a> {
    fn new(contract: &'a LoaderContract) -> Self {
        Self {
            contract, maps: Vec::new(), sites: BTreeMap::new(), epoch: Epoch::Boot, classified: None, scoped: None,
            boot_watch: Value::Null, loaded_watch: Value::Null, scoped_watch: None, stops: Vec::new(), bindings: None,
            completion: None, endpoint: None, final_primary: None, phase: Phase::default(),
        }
    }

    fn arm(&mut self, machine: &mut Machine, sites: &[(u32, Label)]) -> Result<(), String> {
        machine.traces.arm(&machine.session, sites.iter().map(|&(address, label)| (address, label.name().into())).collect())?;
        machine.traces.stop_labels = sites.iter().map(|(_, label)| label.name().into()).collect();
        self.sites = sites.iter().copied().collect();
        Ok(())
    }

    fn add(&mut self, machine: &mut Machine, sites: &[(u32, Label)]) -> Result<(), String> {
        machine.traces.add_sites(&machine.session, sites.iter().map(|&(address, label)| (address, label.name().into())).collect())?;
        for &(address, label) in sites {
            machine.traces.stop_labels.insert(label.name().into());
            self.sites.insert(address, label);
        }
        Ok(())
    }

    fn remove(&mut self, machine: &mut Machine, address: u32) -> Result<(), String> {
        if self.sites.remove(&address).is_some() {
            machine.traces.remove_site(&machine.session, Guest(address))?;
        }
        Ok(())
    }

    fn classes(&self, epoch: Epoch) -> Vec<WatchRange> {
        let range = |(start, end): (u64, u64), kinds: WatchKinds| WatchRange { start, end, kinds };
        let delayed = self.contract.scenario == Scenario::Delayed;
        let mut classes = vec![range((CELL, CELL + 4), WatchKinds::WRITES)];
        if delayed && epoch != Epoch::Boot {
            classes.push(range(VENEER, WatchKinds::WRITES));
        }
        if epoch == Epoch::Scoped {
            classes.extend([LOADER_B, RECORD, LOADER_A_COMPLETION, LOADER_A_LITERAL].map(|span| range(span, WatchKinds::WRITES)));
            classes.push(range(ZERO, WatchKinds::HOST_WRITE.union(WatchKinds::MODEL_BUS_WRITE)));
        }
        classes
    }

    fn begin_classified(&mut self, machine: &mut Machine, epoch: Epoch) -> Result<(), String> {
        self.classified = Some(machine.session.board.memory_watch_begin_classified(self.classes(epoch), WATCH_LIMIT, self.maps.clone(), ViewPolicy::Observe)?);
        self.epoch = epoch;
        Ok(())
    }

    /// Ends the epoch's classified watch and returns its state.
    fn end_classified(&mut self, machine: &mut Machine) -> Result<Value, String> {
        let mut watch = self.classified.take().ok_or("loader-prefix classified watch absent")?;
        let state = watch.records.borrow().state();
        machine.session.board.memory_watch_end(&mut watch)?;
        let view: ClassifiedView = serde_json::from_value(state.clone()).map_err(|error| format!("loader-prefix classified watch: {error}"))?;
        if let Some(violation) = view.violation {
            return Err(format!("loader-prefix classified watch violation: {violation}"));
        }
        Ok(state)
    }

    fn scoped_view(&self) -> Result<Option<ScopedView>, String> {
        self.scoped.as_ref().map(|watch| {
            let view: ScopedView = serde_json::from_value(watch.state()).map_err(|error| format!("loader-prefix scoped watch: {error}"))?;
            match &view.violation {
                Some(violation) => Err(format!("loader-prefix scoped watch violation: {violation}")),
                None => Ok(view),
            }
        }).transpose()
    }

    /// The single readable static mapping of `core` overlapping the extent,
    /// which must contain it.
    fn static_map(&self, core: usize, guest: u64, length: u64) -> Result<&factory::MemoryMap, String> {
        let mut overlapping = self.maps.iter().filter(|map| map.core == core && map.permissions & 1 != 0
            && map.address < guest + length && guest < map.address + map.size as u64);
        let map = overlapping.next().ok_or_else(|| format!("loader-prefix address {guest:#x} has no core-{core} static mapping"))?;
        require(overlapping.next().is_none() && map.address <= guest && guest + length <= map.address + map.size as u64,
            "loader-prefix extent overlaps several static mappings or leaves its mapping")?;
        Ok(map)
    }

    fn physical_of(&self, core: usize, guest: u64, length: u64) -> Result<u64, String> {
        self.static_map(core, guest, length).map(|map| map.physical + guest - map.address)
    }

    fn bound(&self, guest: u64, length: u64) -> Result<Bound, String> {
        let map = self.static_map(1, guest, length)?;
        Ok(Bound { guest: Address(guest), size: length,
            map: BoundMap { core: 1, address: Address(map.address), physical: Address(map.physical), size: map.size as u64 } })
    }

    fn views(&self, machine: &Machine, core: usize, guest: u64, length: usize) -> Result<Views, String> {
        let physical = self.physical_of(core, guest, length as u64)?;
        Ok(Views { r#virtual: wait::bytes(machine, core, guest, length)?, physical: machine.read(physical, length)? })
    }

    fn snapshot(&self, machine: &Machine, label: Label, hit: &Hit) -> Result<LoaderStop, String> {
        let mut registers = [Word(0); 13];
        for (index, value) in registers.iter_mut().enumerate() {
            *value = register(machine, hit.core, arm::Register::R(index as u8))?;
        }
        let view = self.scoped_view()?;
        Ok(LoaderStop {
            label,
            address: Guest(hit_address(hit)?),
            core: WitnessCore::from_index(hit.core)?,
            instructions: Instructions(hit.instructions),
            counts: wait::counts(machine)?,
            registers,
            sp: register(machine, hit.core, arm::Register::R(13))?,
            lr: register(machine, hit.core, arm::Register::R(14))?,
            cpsr: Word(hit.cpsr),
            completion: word(machine, CELL)?,
            tcm: tcm_words(machine)?,
            counter: view.as_ref().map(|view| view.counters.first().copied().ok_or("loader-prefix counter absent")).transpose()?,
            watch_records: view.map(|view| view.records.len()),
        })
    }

    fn handoff_bindings(&self, machine: &Machine, stop: &LoaderStop) -> Result<(ScopeSpan, Bindings), String> {
        let observed: BoardState = serde_json::from_value(machine.session.board.state()).map_err(|error| error.to_string())?;
        require(observed.memory_decoders.choices == [1] && observed.memory_decoders.pending == [None] && observed.memory_decoders.failure.is_none(),
            "loader-prefix low-memory decoder is not committed to RAM at the handoff")?;
        for (core, guest, length) in [(1, LOADER_B.0, LOADER_B.1 - LOADER_B.0), (1, RECORD.0, RECORD.1 - RECORD.0),
            (0, LOADER_A_COMPLETION.0, 16), (0, LOADER_A_LITERAL.0, 4)] {
            require(self.physical_of(core, guest, length)? == guest, "loader-prefix code mapping is not identity")?;
        }
        let zero_range = self.bound(ZERO_GUEST + ZERO.0, ZERO.1 - ZERO.0)?;
        require(zero_range.physical() == ZERO.0, "loader-prefix zero range does not map to physical [0xddaa0,0xf3420)")?;
        let vector_slots = self.bound(VECTOR_SLOTS, VECTOR_BYTES as u64)?;
        let top = u64::from(stop.sp.0);
        let low = top.checked_sub(ENTRY_STACK_BYTES).ok_or("loader-prefix entry stack underflows")?;
        let banks = [(TcmBank::Atcm, stop.tcm[1][0]), (TcmBank::Btcm, stop.tcm[1][1])];
        let (bank, base) = banks.iter().find_map(|&(bank, register)| {
            tcm_window(register).filter(|&(base, end)| low >= base && top <= end).map(|(base, _)| (bank, base))
        }).ok_or("loader-prefix entry-stack window does not lie in one core-1 TCM bank")?;
        let window = ScopeSpan::Tcm(TcmRange::new(bank, (low - base) as u32, (top - base) as u32)?);
        let description = serde_json::json!({"tcm": {"bank": bank, "offsets": [low - base, top - base]}});
        let veneer = if self.contract.scenario == Scenario::Delayed {
            require(self.physical_of(0, VENEER.0, VENEER.1 - VENEER.0)? == VENEER.0, "loader-prefix veneer mapping is not identity")?;
            Some(self.views(machine, 0, VENEER.0, (VENEER.1 - VENEER.0) as usize)?)
        } else {
            None
        };
        Ok((window, Bindings {
            vector_slots,
            stack_window: description,
            zero_range,
            veneer,
        }))
    }

    fn scoped_rules(&self, window: ScopeSpan, slots: u64) -> Result<ScopedWatch, String> {
        let physical = |(start, end): (u64, u64)| PhysicalRange::new(start, end).map(ScopeSpan::Physical);
        let mut confinement = vec![
            ConfinementRule { cores: CoreSet::new(&[1])?, access: Access::Write,
                rule: Confinement::Allow(vec![physical(ZERO)?, physical((slots, slots + VECTOR_BYTES as u64))?, window]) },
            ConfinementRule { cores: CoreSet::new(&[0])?, access: Access::Write, rule: Confinement::Forbid(vec![physical(ZERO)?]) },
        ];
        if self.contract.kind == ProgramKind::Owned {
            confinement.push(ConfinementRule { cores: CoreSet::new(&[1])?, access: Access::Fetch,
                rule: Confinement::Forbid(REPLACED.iter().map(|&span| physical(span)).collect::<Result<_, _>>()?) });
            confinement.push(ConfinementRule { cores: CoreSet::new(&[1])?, access: Access::Read,
                rule: Confinement::Forbid(vec![physical(ENTRY_POOL)?]) });
        }
        Ok(ScopedWatch {
            classes: self.classes(Epoch::Scoped),
            limit: WATCH_LIMIT,
            confinement,
            counters: vec![CounterRule { cores: CoreSet::new(&[1])?, cell: PhysicalWord::new(CELL)?,
                head: SampleBound::new(HEAD_BOUND)?, tail: SampleBound::new(TAIL_BOUND)? }],
            coverage: vec![CoverageRule { cores: CoreSet::new(&[1])?, range: WordRange::new(ZERO.0, ZERO.1)? }],
        })
    }

    /// Steps until loader A's original store commits; core 0 must commit it in
    /// one entered instruction while core 1's count stays unchanged.
    fn commit(&self, machine: &mut Machine, stop_cpsr: Word) -> Result<CommittedStore, String> {
        let expected = if self.contract.scenario == Scenario::NonOne { (0xe583_30c4, 0x400a_0000) } else { (0xe583_50c4, 1) };
        let instruction = WordViews {
            r#virtual: Word(u32::from_le_bytes(wait::bytes(machine, 0, u64::from(STORE), 4)?.try_into().map_err(|_| "store word width")?)),
            physical: word(machine, self.physical_of(0, u64::from(STORE), 4)?)?,
        };
        require(instruction.r#virtual.0 == expected.0 && instruction.physical.0 == expected.0, "loader-A completion store word differs")?;
        let mut microsteps = Vec::new();
        let hits = machine.traces.hits.len();
        for _ in 0..MICROSTEPS {
            let before = wait::counts(machine)?;
            let before_pc = register(machine, 0, arm::Register::R(15))?;
            let before_cpsr = register(machine, 0, arm::Register::Cpsr)?;
            machine.run(1);
            machine.require_running()?;
            require(machine.traces.hits.len() == hits, "loader-prefix boundary occurred before the committed store")?;
            let after = wait::counts(machine)?;
            let cell = word(machine, CELL)?;
            microsteps.push(Microstep { before, after, cell });
            if cell.0 == 0 {
                continue;
            }
            let view = self.scoped_view()?.ok_or("loader-prefix scoped watch absent at commit")?;
            let mut readback = [0; 4];
            machine.session.board.read_memory(0, CELL_GUEST, &mut readback)?;
            let committed = CommittedStore {
                instruction,
                before_pc,
                after_pc: register(machine, 0, arm::Register::R(15))?,
                before_cpsr,
                after_cpsr: register(machine, 0, arm::Register::Cpsr)?,
                microsteps,
                readback: WordViews { r#virtual: Word(u32::from_le_bytes(readback)), physical: cell },
                counter: view.counters.first().copied().ok_or("loader-prefix counter absent")?,
                watch_records: view.records.len(),
            };
            require(before_pc.0 == STORE && committed.after_pc.0 == STORE + 4 && before_cpsr == stop_cpsr
                && after.0 == [before.0[0] + 1, before.0[1]] && cell.0 == expected.1 && committed.readback.r#virtual == cell,
                "loader-A completion store did not commit in one core-0 step with core 1 paused")?;
            return Ok(committed);
        }
        Err("loader-A completion store not committed within 2048 one-entry windows".into())
    }

    fn endpoint(&self, machine: &Machine) -> Result<Endpoint, String> {
        let zero = machine.read(ZERO.0, (ZERO_CLAIM_END - ZERO.0) as usize)?;
        let slots = self.bindings.as_ref().ok_or("loader-prefix bindings absent")?.vector_slots.physical();
        Ok(Endpoint {
            frame: self.views(machine, 1, FRAME, 16)?,
            vectors: Views { r#virtual: wait::bytes(machine, 1, VECTOR_SLOTS, VECTOR_BYTES)?, physical: machine.read(slots, VECTOR_BYTES)? },
            zero: ZeroReadback {
                range: (Address(ZERO.0), Address(ZERO_CLAIM_END)),
                first_nonzero: zero.iter().position(|&byte| byte != 0).map(|offset| Address(ZERO.0 + offset as u64)),
            },
            record_sha256: digest_hex(&machine.read(RECORD.0, (RECORD.1 - RECORD.0) as usize)?),
            loader_sha256: digest_hex(&machine.read(LOADER_B.0, (LOADER_B.1 - LOADER_B.0) as usize)?),
        })
    }

    fn end_scoped(&mut self, machine: &mut Machine) -> Result<(), String> {
        if let Some(mut watch) = self.scoped.take() {
            machine.session.board.memory_watch_end_scoped(&mut watch)?;
            self.scoped_watch = Some(watch.state());
        }
        Ok(())
    }

    /// True when the scenario's endpoint was accepted.
    fn stop(&mut self, machine: &mut Machine, hit: &Hit) -> Result<bool, String> {
        let address = hit_address(hit)?;
        let label = *self.sites.get(&address).ok_or("loader-prefix trace contains an unexpected address")?;
        require(hit.site == label.name(), "loader-prefix trace label differs from its armed site")?;
        let snapshot = self.snapshot(machine, label, hit)?;
        require(snapshot.core == label.core(), "loader-prefix stop has the wrong core")?;
        require(self.stops.len() < MAX_STOPS, "loader-prefix stop bound exceeded")?;
        self.stops.push(snapshot);
        let index = self.stops.len() - 1;
        let contract = self.contract;
        let scenario = contract.scenario;
        let mut accepted = false;
        match label {
            Label::Record1Entry => {
                require(self.epoch == Epoch::Boot, "record-1 entry out of phase")?;
                self.boot_watch = self.end_classified(machine)?;
                self.begin_classified(machine, Epoch::Loaded)?;
                self.remove(machine, RECORD1_ENTRY)?;
                self.add(machine, &[(contract.handoff, Label::Record2Handoff)])?;
            }
            Label::Record2Handoff => {
                require(self.epoch == Epoch::Loaded, "record-2 handoff out of phase")?;
                let stop = &self.stops[index];
                require(stop.registers[3].0 == contract.entry && stop.registers[0].0 == ARGUMENT && stop.completion.0 == 0
                    && stop.cpsr.0 & 0x0100_02ff == 0xd3, "record-2 handoff target, argument, cell or mode differs")?;
                let record = machine.read(RECORD.0, (RECORD.1 - RECORD.0) as usize)?;
                let loader = machine.read(LOADER_B.0, (LOADER_B.1 - LOADER_B.0) as usize)?;
                require(wait::bytes(machine, 1, RECORD.0, record.len())? == record && wait::bytes(machine, 1, LOADER_B.0, loader.len())? == loader
                    && digest_hex(&record) == contract.record && digest_hex(&loader) == contract.loader,
                    "record-2 or loader-B bytes or their core-1 views differ from the contract")?;
                let (window, bindings) = self.handoff_bindings(machine, &self.stops[index])?;
                let slots = bindings.vector_slots.physical();
                self.bindings = Some(bindings);
                self.loaded_watch = self.end_classified(machine)?;
                self.scoped = Some(machine.session.board.memory_watch_begin_scoped(self.scoped_rules(window, slots)?, self.maps.clone())?);
                self.epoch = Epoch::Scoped;
                self.remove(machine, contract.handoff)?;
                let mut add = vec![(contract.entry, Label::PrefixEntry), (contract.post_load, Label::PostLoad), (EXIT, Label::PrefixExit)];
                add.extend(contract.refusal.map(|site| (site, Label::Refusal)));
                add.extend(match scenario {
                    Scenario::Held => vec![(HELD_GATE, Label::HeldGate)],
                    Scenario::Delayed => vec![(DELAY_ENTRY, Label::DelayEntry), (DELAY_RELEASE, Label::DelayRelease), (STORE, Label::CompletionStore)],
                    Scenario::Natural | Scenario::NonOne => vec![(STORE, Label::CompletionStore)],
                });
                self.add(machine, &add)?;
            }
            Label::PrefixEntry => {
                let handoff = self.stops.iter().find(|stop| stop.label == Label::Record2Handoff).ok_or("prefix entry precedes the handoff")?;
                let stop = &self.stops[index];
                require(!self.phase.entry && stop.registers[0].0 == ARGUMENT && stop.sp == handoff.sp && stop.cpsr == handoff.cpsr
                    && stop.registers[6..12] == handoff.registers[6..12], "prefix entry differs from the handoff context")?;
                self.phase.entry = true;
                self.remove(machine, contract.entry)?;
            }
            Label::PostLoad => {
                require(self.phase.entry && !self.phase.exit, "post-load stop out of phase")?;
                let stop = &self.stops[index];
                let loaded = stop.registers[usize::from(contract.loaded)].0;
                let counter = stop.counter.ok_or("post-load stop lacks its counter")?;
                require(loaded == 0 || counter.nonzero_count == 1, "post-load register holds a nonzero value without a nonzero load")?;
                if let Some(previous) = self.phase.last_post {
                    require(counter.count == previous + 1, "consecutive post-load stops do not pair with exactly one completed load")?;
                }
                self.phase.last_post = Some(counter.count);
                if self.phase.gate {
                    self.phase.posts_after_gate += 1;
                    if self.phase.posts_after_gate == HEAD_POLLS {
                        let primary = wait::primary(machine)?;
                        require(primary.address.0 == HELD_GATE && counter.nonzero_count == 0, "held core 0 left its gate or a load saw completion")?;
                        self.final_primary = Some(primary);
                        accepted = true;
                    }
                } else {
                    self.phase.posts += 1;
                    if self.phase.posts == HEAD_POLLS {
                        self.remove(machine, contract.post_load)?;
                    }
                }
            }
            Label::HeldGate => {
                require(scenario == Scenario::Held && !self.phase.gate && self.stops[index].completion.0 == 0, "held gate out of phase")?;
                let mut gate = [0; 4];
                machine.session.board.read_memory(0, u64::from(HELD_GATE), &mut gate)?;
                require(u32::from_le_bytes(gate) == 0xeafffffe, "held gate is not a self-branch")?;
                self.phase.gate = true;
                self.phase.last_post = None;
                self.remove(machine, HELD_GATE)?;
                if !self.sites.contains_key(&contract.post_load) {
                    self.add(machine, &[(contract.post_load, Label::PostLoad)])?;
                }
            }
            Label::DelayEntry => {
                require(scenario == Scenario::Delayed && self.phase.delay_entry.is_none() && self.stops[index].lr.0 == HELD_GATE + 4,
                    "delay entry out of phase or with another return")?;
                self.phase.delay_entry = Some(index);
                self.remove(machine, DELAY_ENTRY)?;
            }
            Label::DelayRelease => {
                let entry = &self.stops[self.phase.delay_entry.ok_or("delay release precedes its entry")?];
                let stop = &self.stops[index];
                require(!self.phase.released && stop.cpsr.0 & 0xf800_0000 == entry.cpsr.0 & 0xf800_0000 && stop.sp == entry.sp
                    && stop.registers[11] == entry.registers[11], "delay release did not restore NZCVQ, SP or r11")?;
                self.phase.released = true;
                self.remove(machine, DELAY_RELEASE)?;
            }
            Label::CompletionStore => {
                require(!self.phase.stored && (scenario != Scenario::Delayed || self.phase.released), "completion store out of phase")?;
                let stop = &self.stops[index];
                require(stop.registers[3].0 == 0x400a_0000 && stop.registers[5].0 == 1 && stop.completion.0 == 0
                    && stop.counter.is_some_and(|counter| counter.nonzero_count == 0), "completion-store context differs")?;
                if let Some(entry) = self.phase.delay_entry {
                    let loads = |stop: &LoaderStop| stop.counter.map(|counter| counter.count);
                    require(loads(&self.stops[index]).zip(loads(&self.stops[entry])).is_some_and(|(store, entry)| store >= entry + 4),
                        "delayed control did not observe four zero completion loads during the delay")?;
                }
                let cpsr = self.stops[index].cpsr;
                self.remove(machine, STORE)?;
                self.remove(machine, contract.post_load)?;
                let committed = self.commit(machine, cpsr)?;
                self.completion = Some(committed);
                self.phase.stored = true;
            }
            Label::PrefixExit => {
                require(self.phase.stored && !self.phase.exit, "prefix exit precedes the committed store")?;
                self.endpoint = Some(self.endpoint(machine)?);
                self.phase.exit = true;
                accepted = true;
            }
            Label::Refusal => return Err("core-1 prefix refusal reached".into()),
        }
        let handoff_tcm = self.stops.iter().find(|stop| stop.label == Label::Record2Handoff).map(|stop| stop.tcm);
        if let Some(tcm) = handoff_tcm {
            require(self.stops[index].tcm == tcm || label == Label::Record2Handoff, "TCM registers changed after the handoff")?;
        }
        if accepted {
            self.end_scoped(machine)?;
        }
        Ok(accepted)
    }

    fn execute(&mut self, machine: &mut Machine) -> Result<(), String> {
        loop {
            require(machine.instructions() < BUDGET, "loader-prefix instruction budget exhausted")?;
            let before = machine.traces.hits.len();
            machine.run(BUDGET - machine.instructions());
            machine.require_running()?;
            let hits = machine.traces.hits[before..].to_vec();
            require(!hits.is_empty(), "loader-prefix boundary not reached within the budget")?;
            for hit in &hits {
                if self.stop(machine, hit)? {
                    return Ok(());
                }
            }
        }
    }

    fn finish(&mut self, machine: &mut Machine, mut outcome: Result<(), String>) -> Result<(), String> {
        if let Some(mut watch) = self.classified.take() {
            let state = watch.records.borrow().state();
            outcome = outcome.and(machine.session.board.memory_watch_end(&mut watch));
            match self.epoch {
                Epoch::Boot => self.boot_watch = state,
                _ => self.loaded_watch = state,
            }
        }
        if let Some(mut watch) = self.scoped.take() {
            outcome = outcome.and(machine.session.board.memory_watch_end_scoped(&mut watch));
            self.scoped_watch = Some(watch.state());
        }
        outcome
    }
}

pub(super) fn run(mut machine: Machine, contract: &LoaderContract, output: &Path) -> Result<(), String> {
    let mut identities = [Identity(0); 2];
    let mut runner = Runner::new(contract);
    let outcome = (|| {
        require(wait::counts(&machine)?.0 == [0, 0] && !machine.traces.armed() && machine.traces.hits.is_empty(),
            "loader-prefix witness must arm before either core executes")?;
        let (maps, configured) = bindings(&machine.document)?;
        identities = configured;
        require(u64::try_from(machine.document.execution.quantum) == Ok(QUANTUM) && matches!(machine.document.execution.engine, RunEngine::Unicorn)
            && machine.document.execution.instructions_per_tick.numerator == 24
            && machine.document.execution.instructions_per_tick.denominator == 1, "loader-prefix scheduling configuration differs")?;
        for (core, expected) in identities.iter().enumerate() {
            require(machine.session.board.cpu(core)?.rprofile_identity(None)? == expected.0, "loader-prefix active CP14 identity differs")?;
        }
        runner.maps = maps;
        runner.begin_classified(&mut machine, Epoch::Boot)?;
        runner.arm(&mut machine, &[(RECORD1_ENTRY, Label::Record1Entry)])?;
        runner.execute(&mut machine)
    })();
    let outcome = runner.finish(&mut machine, outcome);
    let receipt = Receipt {
        schema: 4, scenario: ScenarioWire { kind: contract.scenario }, program: contract.kind,
        result: if outcome.is_ok() { WitnessResult::Passed } else { WitnessResult::Failed },
        error: outcome.as_ref().err().map(String::as_str), machine_failure: machine.failure.as_deref(),
        instructions: Instructions(machine.instructions()), cp14_identities: identities,
        boot_watch: &runner.boot_watch, loaded_watch: &runner.loaded_watch, scoped_watch: &runner.scoped_watch,
        stops: &runner.stops, bindings: &runner.bindings, completion: &runner.completion, endpoint: &runner.endpoint,
        final_primary: &runner.final_primary,
    };
    let bytes = serde_json::to_vec_pretty(&receipt).map_err(|error| error.to_string())?;
    require(bytes.len() <= MAX_RECEIPT, "loader-prefix receipt exceeds 16 MiB")?;
    let path = output.join("peer-witness.json");
    std::fs::write(&path, bytes).map_err(|error| format!("{}: {error}", path.display()))?;
    println!("{}", path.display());
    outcome
}
