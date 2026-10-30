//! Schema-3 core-1 record-2 startup observation of the retained program or an
//! owned candidate, with classified memory watches per epoch.
use super::*;
use super::wait::{self, BoardState, ClearReadback, Control, CoreCounts, GateBindings, Primary, TcmBank, WatchCursor, require};
use fujimod_catalog::configuration::RunEngine;
use fujimod_emulator::board::observation::{ViewPolicy, WatchKinds, WatchRange};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

const NATURAL_BUDGET: u64 = 20_000_000;
const PAIRS: usize = 128;
const HELD_PAIRS: usize = 4;
const ROWS: usize = 12_288;
const VENEER_POLLS: usize = 8_192;
const QUANTUM: u64 = 1_000;
const HELD_PC: u32 = 0x3fff_0000;
const GATE: u32 = 0x1ff4;
const ACKNOWLEDGED: u32 = 0x14c;
const CLEAR: u32 = 0x1ff8;
const FAILURE: u32 = 0x148;
const LOADER_ENTRY: u32 = 0xd0020;
const SVC_TOP: u32 = 0x7000_0800;
const ATCM_INIT: u32 = 0x7000_0011;
const SENTINEL: u32 = 0x22b;
const ARGUMENT_SOURCE: u64 = 0x61c40c;
const SEGMENT_ROOT: u64 = 0x61c000;
const SEGMENT_RECORD: u32 = 36;
const SEGMENT_LIMIT: u32 = 64;
const RECORD: (u64, u64) = (0x10000, 0x14c80);
const LOADER_B: (u64, u64) = (0xd0000, 0xddaa0);
const GLOBALS: (u64, u64) = (0x14c60, 0x14ca0);
const COMPANION: (u64, u64) = (0xa0000, 0xa0548);
const IMMUTABLE: [(u64, u64); 2] = [(0x10000, 0x14c60), (0x14c6c, 0x14c80)];
const MUTABLE: [(u64, u64); 4] = [(0x14c60, 0x14c6c), (0x14c88, 0x14ca0), (0xa0000, 0xa00c4), (0xa00c8, 0xa0548)];
const FORBID_FETCH: [(u32, u32); 7] = [
    (0x11800, 0x11854), (0x119e4, 0x11abc), (0x11cc0, 0x11fe4), (0x12960, 0x12974),
    (0x12abc, 0x12ad0), (0x13490, 0x13574), (0x1386c, 0x138ac),
];
const FORBID_READ: [(u32, u32); 3] = [(0x11ac4, 0x11acc), (0x11ad4, 0x11ae8), (0x11fe4, 0x11fec)];
const RETAINED_RECORD: [u8; 32] = [
    0x2f, 0x1b, 0xf3, 0xf0, 0xe4, 0x83, 0xb8, 0x92, 0xd2, 0x89, 0x05, 0x99, 0x67, 0x9c, 0x1b, 0x51,
    0x5c, 0xfc, 0x1b, 0xe8, 0xb8, 0xc2, 0x7c, 0xbb, 0x06, 0xd4, 0xd9, 0x13, 0xe4, 0x28, 0xab, 0xb4,
];
/// Core-1 guest, physical and length of each mapping the startup relies on.
const REQUIRED_MAPS: [(u64, u64, u64); 5] = [
    (0x10000, 0x10000, 0x4ca0), (0x4002_0000, 0x20000, 8), (0xd0000, 0xd0000, 0xdaa0),
    (0x61c000, 0x61c000, 4), (0x61c40c, 0x61c40c, 4),
];

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
enum Endpoint { Handoff, LoaderEntry }

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
enum Scenario {
    Natural { endpoint: Endpoint },
    HeldSentinel {},
    DelayedRelease { endpoint: Endpoint },
}
impl Scenario {
    fn budget(self) -> u64 {
        match self { Self::Natural { .. } => NATURAL_BUDGET, _ => wait::BUDGET }
    }
    fn control(self) -> Option<Control> {
        match self {
            Self::Natural { .. } => None,
            Self::HeldSentinel {} => Some(Control::HeldSentinel),
            Self::DelayedRelease { .. } => Some(Control::DelayedRelease),
        }
    }
    fn endpoint(self) -> Option<Endpoint> {
        match self {
            Self::Natural { endpoint } | Self::DelayedRelease { endpoint } => Some(endpoint),
            Self::HeldSentinel {} => None,
        }
    }
}

#[derive(Clone, Copy, Deserialize)]
#[serde(deny_unknown_fields)]
struct Sites {
    entry: Guest,
    main: Guest,
    peer_load: Guest,
    peer_loaded: Guest,
    clear_store: Guest,
    target_store: Guest,
    argument_store: Guest,
    target_load: Guest,
    argument_load: Guest,
    handoff: Guest,
}
impl Sites {
    fn all(&self) -> [Guest; 10] {
        [self.entry, self.main, self.peer_load, self.peer_loaded, self.clear_store, self.target_store,
            self.argument_store, self.target_load, self.argument_load, self.handoff]
    }
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
enum ProgramWire {
    Retained {},
    Owned {
        record_sha256: String,
        image: (Guest, Guest),
        sites: Sites,
        loaded_register: u8,
        poll_period: u64,
        forbid_fetch: Vec<(Guest, Guest)>,
        forbid_read: Vec<(Guest, Guest)>,
    },
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct StartupWire {
    schema: u32,
    budget: Instructions,
    scenario: Scenario,
    program: ProgramWire,
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum ProgramKind { Retained, Owned }

struct Program {
    kind: ProgramKind,
    record: [u8; 32],
    startup: (u32, u32),
    sites: Sites,
    loaded: u8,
    period: u64,
    forbid_fetch: Vec<(u32, u32)>,
    forbid_read: Vec<(u32, u32)>,
}

pub(super) struct StartupContract {
    scenario: Scenario,
    program: Program,
}
impl StartupContract {
    pub(super) fn budget(&self) -> Instructions {
        Instructions(self.scenario.budget())
    }
}

fn retained() -> Program {
    Program {
        kind: ProgramKind::Retained,
        record: RETAINED_RECORD,
        startup: (0x11800, 0x11fec),
        sites: Sites {
            entry: Guest(0x11800), main: Guest(0x11808), peer_load: Guest(0x11f74), peer_loaded: Guest(0x11f78),
            clear_store: Guest(0x11f4c), target_store: Guest(0x11f98), argument_store: Guest(0x11fb0),
            target_load: Guest(0x11844), argument_load: Guest(0x1184c), handoff: Guest(0x11850),
        },
        loaded: 3,
        period: 304,
        forbid_fetch: Vec::new(),
        forbid_read: Vec::new(),
    }
}

fn digest(text: &str) -> Result<[u8; 32], String> {
    require(text.len() == 64 && text.bytes().all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f')),
        "startup record digest must be 64 lowercase hexadecimal characters")?;
    let mut bytes = [0; 32];
    for (index, value) in bytes.iter_mut().enumerate() {
        *value = u8::from_str_radix(&text[2 * index..2 * index + 2], 16).map_err(|error| error.to_string())?;
    }
    Ok(bytes)
}

fn exact_set(observed: &[(Guest, Guest)], expected: &[(u32, u32)]) -> Result<Vec<(u32, u32)>, String> {
    let observed: BTreeSet<(u32, u32)> = observed.iter().map(|(lo, hi)| (lo.0, hi.0)).collect();
    let expected: BTreeSet<(u32, u32)> = expected.iter().copied().collect();
    require(observed == expected, "startup forbidden intervals differ from the source-bound set")?;
    Ok(observed.into_iter().collect())
}

impl StartupWire {
    pub(super) fn validate(self) -> Result<StartupContract, String> {
        require(self.schema == 3 && self.budget.0 == self.scenario.budget(), "startup witness requires schema 3 and its scenario budget")?;
        let program = match self.program {
            ProgramWire::Retained {} => retained(),
            ProgramWire::Owned { record_sha256, image, sites, loaded_register, poll_period, forbid_fetch, forbid_read } => {
                let (start, end) = (image.0 .0, image.1 .0);
                require(start < end && start % 4 == 0 && end % 4 == 0 && start >= 0x103e8 && end <= 0x11800,
                    "startup owned image lies outside record-2 padding")?;
                let all = sites.all();
                require(all.iter().all(|site| site.0 % 4 == 0 && (start..end).contains(&site.0))
                    && all.iter().map(|site| site.0).collect::<BTreeSet<_>>().len() == all.len(),
                    "startup owned sites must be distinct aligned image addresses")?;
                require(loaded_register <= 12 && (1..=100_000).contains(&poll_period), "startup owned register or poll period is out of range")?;
                Program {
                    kind: ProgramKind::Owned,
                    record: digest(&record_sha256)?,
                    startup: (start, end),
                    sites,
                    loaded: loaded_register,
                    period: poll_period,
                    forbid_fetch: exact_set(&forbid_fetch, &FORBID_FETCH)?,
                    forbid_read: exact_set(&forbid_read, &FORBID_READ)?,
                }
            }
        };
        Ok(StartupContract { scenario: self.scenario, program })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum Label {
    HeldRelease,
    StartupEntry,
    StartupMain,
    PeerLoaded,
    PrimaryGate,
    PeerAcknowledged,
    PrimaryClear,
    ControlFailure,
    StartupHandoff,
    LoaderBEntry,
    ForbiddenFetch,
}
impl Label {
    fn name(self) -> &'static str {
        match self {
            Self::HeldRelease => "held-release",
            Self::StartupEntry => "startup-entry",
            Self::StartupMain => "startup-main",
            Self::PeerLoaded => "peer-loaded",
            Self::PrimaryGate => "primary-gate",
            Self::PeerAcknowledged => "peer-acknowledged",
            Self::PrimaryClear => "primary-clear",
            Self::ControlFailure => "control-failure",
            Self::StartupHandoff => "startup-handoff",
            Self::LoaderBEntry => "loader-b-entry",
            Self::ForbiddenFetch => "forbidden-fetch",
        }
    }
    fn core(self) -> Option<WitnessCore> {
        match self {
            Self::ForbiddenFetch => None,
            Self::PrimaryGate | Self::PeerAcknowledged | Self::PrimaryClear | Self::ControlFailure => Some(WitnessCore::Core0),
            _ => Some(WitnessCore::Core1),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum Epoch { Reset, Released, Published }

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ClassState {
    range: (Address, Address),
    kinds: Vec<String>,
}
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
enum Views { Observe, Fixed }
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ClassifiedWatch {
    enabled: bool,
    ranges: Vec<(Address, Address)>,
    records: Vec<MemoryEvent>,
    violation: Option<String>,
    classes: Vec<ClassState>,
    views: Views,
}
#[derive(Serialize)]
#[serde(untagged)]
enum WatchEvidence {
    Observed(ClassifiedWatch),
    Rejected(Box<serde_json::value::RawValue>),
}
#[derive(Serialize)]
struct EpochRecord {
    epoch: Epoch,
    instructions: Instructions,
    memory_watch: WatchEvidence,
}

#[derive(Serialize)]
struct ProgramState {
    globals: String,
    companion: String,
}
type TcmWords = [[Word; 2]; 2];
#[derive(Serialize)]
struct StartupStop {
    label: Label,
    address: Guest,
    core: WitnessCore,
    instructions: Instructions,
    counts: CoreCounts,
    registers: [Word; 13],
    sp: Word,
    lr: Word,
    cpsr: Word,
    state: PeerState,
    tcm: TcmWords,
    epoch: Epoch,
    watch_records: usize,
    program_state: Option<ProgramState>,
}
#[derive(Serialize)]
struct BoundMap {
    core: usize,
    address: Address,
    physical: Address,
    size: u64,
}
#[derive(Serialize)]
struct Bindings {
    decoder_choices: Vec<usize>,
    maps: Vec<BoundMap>,
    tcm: TcmWords,
}
#[derive(Clone, Serialize)]
struct Segment {
    root: Word,
    index: u32,
    record: Word,
    header: Word,
    entry: Word,
    argument: Word,
    loader_sha256: String,
    loader_entry: Word,
}
#[derive(Serialize)]
struct RecordCheck {
    sha256: String,
    site_words: Vec<(Guest, Word)>,
}
#[derive(Serialize)]
struct Receipt<'a> {
    schema: u32,
    scenario: Scenario,
    program: ProgramKind,
    result: WitnessResult,
    error: Option<&'a str>,
    machine_failure: Option<&'a str>,
    instructions: Instructions,
    cp14_identities: [Identity; 2],
    epochs: &'a [EpochRecord],
    stops: &'a [StartupStop],
    bindings: &'a Option<Bindings>,
    gate: &'a Option<GateBindings>,
    clear_readback: &'a Option<ClearReadback>,
    final_primary: &'a Option<Primary>,
    segment: &'a Option<Segment>,
    record: &'a Option<RecordCheck>,
}

#[derive(Default)]
struct Phase {
    release: bool,
    entry: bool,
    main: Option<usize>,
    pairs: usize,
    waiting: usize,
    last_pair: usize,
    last_waiting_core1: Option<u64>,
    exit: bool,
    gate: Option<usize>,
    ack: Option<usize>,
    pairs_at_ack: usize,
    clear: Option<Instructions>,
    handoff: bool,
    /// The peer-loaded site was disarmed during the clear microsteps; loads
    /// completed then have no paired stop.
    rejoin: bool,
    release_tcm: Option<TcmWords>,
    main_tcm: Option<TcmWords>,
    published: Vec<(u64, u64)>,
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn word_at(bytes: &[u8], address: u32) -> Result<Word, String> {
    let offset = usize::try_from(address.checked_sub(RECORD.0 as u32).ok_or("startup site lies below record 2")?)
        .map_err(|error| error.to_string())?;
    let slice = bytes.get(offset..offset + 4).ok_or("startup site lies outside record 2")?;
    Ok(Word(u32::from_le_bytes(slice.try_into().map_err(|_| "startup site word width")?)))
}

fn bl_target(word: Word, at: u32) -> Option<u32> {
    (word.0 & 0xff00_0000 == 0xeb00_0000)
        .then(|| at.wrapping_add(8).wrapping_add((((word.0 << 8) as i32) >> 6) as u32))
}

fn load_register(word: Word) -> Option<(u32, u32)> {
    (word.0 & 0xfff0_0fff == 0xe590_0000).then_some(((word.0 >> 12) & 15, (word.0 >> 16) & 15))
}

fn is_store(word: Word) -> bool {
    word.0 & 0xfe50_0000 == 0xe400_0000
}

fn tcm_words(machine: &Machine) -> Result<TcmWords, String> {
    Ok([
        [wait::tcm(machine, WitnessCore::Core0, TcmBank::Atcm)?, wait::tcm(machine, WitnessCore::Core0, TcmBank::Btcm)?],
        [wait::tcm(machine, WitnessCore::Core1, TcmBank::Atcm)?, wait::tcm(machine, WitnessCore::Core1, TcmBank::Btcm)?],
    ])
}

fn overlaps(row: (u64, u64), range: (u64, u64)) -> bool {
    row.0 < range.1 && range.0 < row.1
}

fn row_extent(event: &MemoryEvent) -> (u64, u64) {
    match event {
        MemoryEvent::Cpu(row) => (row.physical.0, row.physical.0.saturating_add(u64::from(row.size))),
        MemoryEvent::Transfer(row) => (row.physical.0, row.physical.0.saturating_add(row.size)),
    }
}

fn cpu_row(event: &MemoryEvent, core: WitnessCore, pc: u32, physical: u64, access: CpuAccess) -> Option<&CpuAttempt> {
    match event {
        MemoryEvent::Cpu(row) if row.core == core && row.pc.0 == pc && row.physical.0 == physical
            && std::mem::discriminant(&row.source) == std::mem::discriminant(&access) => Some(row),
        _ => None,
    }
}

struct Runner<'a> {
    contract: &'a StartupContract,
    maps: Vec<factory::MemoryMap>,
    sites: BTreeMap<u32, Label>,
    epochs: Vec<EpochRecord>,
    epoch: Epoch,
    epoch_start: Instructions,
    watch: Option<Box<MemoryWatch>>,
    stops: Vec<StartupStop>,
    bindings: Option<Bindings>,
    gate: Option<GateBindings>,
    clear_readback: Option<ClearReadback>,
    final_primary: Option<Primary>,
    segment: Option<Segment>,
    record: Option<RecordCheck>,
    phase: Phase,
}

impl<'a> Runner<'a> {
    fn new(contract: &'a StartupContract) -> Self {
        Self {
            contract, maps: Vec::new(), sites: BTreeMap::new(), epochs: Vec::new(), epoch: Epoch::Reset,
            epoch_start: Instructions(0), watch: None, stops: Vec::new(), bindings: None, gate: None,
            clear_readback: None, final_primary: None, segment: None, record: None, phase: Phase::default(),
        }
    }

    fn program(&self) -> &'a Program {
        &self.contract.program
    }

    fn classes(&self, epoch: Epoch) -> Vec<WatchRange> {
        let range = |(start, end): (u64, u64), kinds: WatchKinds| WatchRange { start, end, kinds };
        let mut classes: Vec<WatchRange> = WATCH_RANGES.iter().map(|&span| range(span, WatchKinds::ALL)).collect();
        if epoch == Epoch::Reset {
            return classes;
        }
        let program = self.program();
        classes.extend(IMMUTABLE.iter().map(|&span| range(span, WatchKinds::WRITES)));
        let retired = WatchKinds::CPU_READ.union(WatchKinds::CPU_FETCH).union(WatchKinds::MODEL_BUS_READ).union(WatchKinds::WRITES);
        classes.extend(program.forbid_fetch.iter().chain(&program.forbid_read)
            .map(|&(lo, hi)| range((u64::from(lo), u64::from(hi)), retired)));
        classes.extend(MUTABLE.iter().map(|&span| range(span, WatchKinds::HOST_WRITE.union(WatchKinds::MODEL_BUS_WRITE))));
        if epoch == Epoch::Published {
            classes.extend(self.phase.published.iter().map(|&span| range(span, WatchKinds::WRITES)));
        }
        classes
    }

    fn watch_now(&self) -> Result<ClassifiedWatch, String> {
        let watch = self.watch.as_ref().ok_or("startup memory watch absent")?;
        serde_json::from_value(watch.records.borrow().state()).map_err(|error| format!("startup memory watch: {error}"))
    }

    fn begin_epoch(&mut self, machine: &mut Machine, epoch: Epoch) -> Result<(), String> {
        if let Some(mut watch) = self.watch.take() {
            let state = serde_json::from_value(watch.records.borrow().state())
                .map_err(|error| format!("startup memory watch: {error}"))?;
            machine.session.board.memory_watch_end(&mut watch)?;
            self.epochs.push(EpochRecord { epoch: self.epoch, instructions: self.epoch_start, memory_watch: WatchEvidence::Observed(state) });
        }
        let views = if epoch == Epoch::Reset { ViewPolicy::Observe } else { ViewPolicy::Fixed };
        self.watch = Some(machine.session.board.memory_watch_begin_classified(self.classes(epoch), WATCH_LIMIT, self.maps.clone(), views)?);
        self.epoch = epoch;
        self.epoch_start = Instructions(machine.instructions());
        Ok(())
    }

    fn finish(&mut self, machine: &mut Machine, mut outcome: Result<(), String>) -> Result<(), String> {
        if let Some(mut watch) = self.watch.take() {
            let state = watch.records.borrow().state();
            let evidence = match serde_json::from_value::<ClassifiedWatch>(state.clone()) {
                Ok(observed) => WatchEvidence::Observed(observed),
                Err(error) => {
                    outcome = outcome.and(Err(format!("startup memory watch: {error}")));
                    WatchEvidence::Rejected(serde_json::value::to_raw_value(&state).map_err(|error| error.to_string())?)
                }
            };
            outcome = outcome.and(machine.session.board.memory_watch_end(&mut watch));
            self.epochs.push(EpochRecord { epoch: self.epoch, instructions: self.epoch_start, memory_watch: evidence });
        }
        outcome
    }

    fn arm(&mut self, machine: &mut Machine, sites: BTreeMap<u32, Label>) -> Result<(), String> {
        machine.traces.arm(&machine.session, sites.iter().map(|(&address, label)| (address, label.name().into())).collect())?;
        machine.traces.stop_labels = sites.values().map(|label| label.name().into()).collect();
        self.sites = sites;
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
        machine.traces.remove_site(&machine.session, Guest(address))?;
        self.sites.remove(&address);
        Ok(())
    }

    fn snapshot(&self, machine: &Machine, label: Label, hit: &Hit) -> Result<StartupStop, String> {
        let mut registers = [Word(0); 13];
        for (index, value) in registers.iter_mut().enumerate() {
            *value = register(machine, hit.core, arm::Register::R(index as u8))?;
        }
        let state = state(machine)?;
        let program_state = if matches!(label, Label::StartupHandoff | Label::LoaderBEntry) {
            Some(ProgramState {
                globals: hex(&machine.read(GLOBALS.0, (GLOBALS.1 - GLOBALS.0) as usize)?),
                companion: hex(&machine.read(COMPANION.0, (COMPANION.1 - COMPANION.0) as usize)?),
            })
        } else {
            None
        };
        Ok(StartupStop {
            label,
            address: Guest(hit_address(hit)?),
            core: WitnessCore::from_index(hit.core)?,
            instructions: Instructions(hit.instructions),
            counts: wait::counts(machine)?,
            registers,
            sp: register(machine, hit.core, arm::Register::R(13))?,
            lr: register(machine, hit.core, arm::Register::R(14))?,
            cpsr: Word(hit.cpsr),
            state,
            tcm: tcm_words(machine)?,
            epoch: self.epoch,
            watch_records: self.watch_now()?.records.len(),
            program_state,
        })
    }

    fn phase_end(&self) -> u64 {
        let budget = self.contract.scenario.budget();
        let phase_end = match self.contract.scenario.control() {
            None => budget,
            Some(control) => match (self.phase.clear, self.phase.gate) {
                (Some(clear), _) => clear.0.saturating_add(wait::RELEASE),
                (None, Some(gate)) => self.stops[gate].instructions.0.saturating_add(
                    if control == Control::HeldSentinel { wait::HELD } else { wait::DELAYED }),
                (None, None) => wait::PREFIX,
            },
        };
        budget.min(phase_end)
    }

    fn guarded(&self) -> Vec<(u64, u64)> {
        self.classes(self.epoch).iter().map(|range| (range.start, range.end)).collect()
    }

    fn tcm_disjoint(&self, tcm: &TcmWords) -> Result<(), String> {
        let guarded = self.guarded();
        for (core, banks) in tcm.iter().enumerate() {
            for bank in banks.iter().filter(|bank| bank.0 & 1 != 0) {
                let base = u64::from(bank.0 & 0xffff_f000);
                let window = (base, base + (512u64 << ((bank.0 >> 2) & 31)));
                for map in self.maps.iter().filter(|map| map.core == core) {
                    for &(lo, hi) in &guarded {
                        let start = lo.max(map.physical);
                        let end = hi.min(map.physical + map.size as u64);
                        if start < end {
                            let view = (map.address + start - map.physical, map.address + end - map.physical);
                            require(!overlaps(view, window), "startup TCM window overlays a guarded memory view")?;
                        }
                    }
                }
            }
        }
        Ok(())
    }

    fn tcm_stable(&self, tcm: &TcmWords) -> Result<(), String> {
        self.tcm_disjoint(tcm)?;
        match (self.phase.main_tcm, self.phase.release_tcm) {
            (Some(main), _) => require(*tcm == main, "startup TCM registers changed after the post-initialization stop"),
            (None, Some(release)) => require(*tcm == release, "startup TCM registers changed before the post-initialization stop"),
            (None, None) => Ok(()),
        }
    }

    fn physical_of(&self, guest: u32, length: u64) -> Result<u64, String> {
        let guest = u64::from(guest);
        self.maps.iter().find(|map| map.core == 1 && map.address <= guest
            && guest + length <= map.address + map.size as u64 && map.permissions & 1 != 0)
            .map(|map| map.physical + guest - map.address)
            .ok_or_else(|| format!("startup address {guest:#x} has no core-1 static mapping"))
    }

    fn rows(&self) -> Result<Vec<(Epoch, MemoryEvent)>, String> {
        let mut rows = Vec::new();
        for record in &self.epochs {
            if let WatchEvidence::Observed(state) = &record.memory_watch {
                let replay: ClassifiedWatch = serde_json::from_value(serde_json::to_value(state).map_err(|error| error.to_string())?)
                    .map_err(|error| error.to_string())?;
                rows.extend(replay.records.into_iter().map(|row| (record.epoch, row)));
            } else {
                return Err("startup epoch watch evidence was rejected".into());
            }
        }
        rows.extend(self.watch_now()?.records.into_iter().map(|row| (self.epoch, row)));
        Ok(rows)
    }

    fn epoch_offset(&self, epoch: Epoch) -> Result<usize, String> {
        let mut offset = 0;
        for record in &self.epochs {
            if record.epoch == epoch {
                return Ok(offset);
            }
            match &record.memory_watch {
                WatchEvidence::Observed(state) => offset += state.records.len(),
                WatchEvidence::Rejected(_) => return Err("startup epoch watch evidence was rejected".into()),
            }
        }
        require(self.epoch == epoch, "startup stop epoch is unknown")?;
        Ok(offset)
    }

    fn global(&self, stop: &StartupStop) -> Result<usize, String> {
        Ok(self.epoch_offset(stop.epoch)? + stop.watch_records)
    }

    fn describe(&self, extent: (u64, u64)) -> &'static str {
        let program = self.program();
        let wide = |(lo, hi): (u32, u32)| (u64::from(lo), u64::from(hi));
        if program.forbid_fetch.iter().any(|&span| overlaps(extent, wide(span))) {
            "startup watch observed an access to retired stock code"
        } else if program.forbid_read.iter().any(|&span| overlaps(extent, wide(span))) {
            "startup watch observed an access to a retired stock literal pool"
        } else if IMMUTABLE.iter().any(|&span| overlaps(extent, span)) {
            "startup watch observed a write to immutable record-2 bytes"
        } else if MUTABLE.iter().any(|&span| overlaps(extent, span)) {
            "startup watch observed a host or model-bus write to declared guest-mutable state"
        } else if self.phase.published.iter().any(|&span| overlaps(extent, span)) {
            "startup watch observed a write to published loader, segment or argument bytes"
        } else {
            "startup watch observed a row outside every watched range"
        }
    }

    fn check_rows(&self) -> Result<(), String> {
        let current = self.watch_now()?;
        if let Some(violation) = &current.violation {
            return Err(format!("startup memory watch violation: {violation}"));
        }
        require(current.enabled, "startup memory watch disabled")?;
        let rows = self.rows()?;
        require(rows.len() <= ROWS, "startup watch exceeded its successful row bound")?;
        let program = self.program();
        let sites = &program.sites;
        let held = self.contract.scenario == (Scenario::HeldSentinel {});
        let delayed = matches!(self.contract.scenario, Scenario::DelayedRelease { .. });
        let position = |test: &dyn Fn(&MemoryEvent) -> bool| rows.iter().position(|(_, row)| test(row));
        let sentinel_end = position(&|row| cpu_row(row, WitnessCore::Core0, 0x1e68, 0x20004, CpuAccess::CpuWrite).is_some());
        let primary_clear = position(&|row| cpu_row(row, WitnessCore::Core0, CLEAR, 0x20000, CpuAccess::CpuWrite).is_some());
        let peer_clear = position(&|row| cpu_row(row, WitnessCore::Core1, sites.clear_store.0, 0x20004, CpuAccess::CpuWrite).is_some());
        let segment_entry = self.segment.as_ref().map(|segment| u64::from(segment.entry.0));
        let argument = self.segment.as_ref().map(|segment| u64::from(segment.argument.0));
        let (mut pairs, mut polls, mut previous) = (0, 0, (0, 0));
        for (index, (epoch, row)) in rows.iter().enumerate() {
            let extent = row_extent(row);
            let protocol = WATCH_RANGES.iter().any(|&span| overlaps(extent, span));
            if !protocol {
                return Err(self.describe(extent).into());
            }
            require(WATCH_RANGES.iter().any(|&(lo, hi)| extent.0 >= lo && extent.1 <= hi), "startup row straddles a protocol range")?;
            match row {
                MemoryEvent::Transfer(transfer) => match transfer.source {
                    TransferAccess::HostWrite => return Err("startup watch contains a host write".into()),
                    TransferAccess::ModelBusWrite => require(*epoch == Epoch::Reset && sentinel_end.is_none_or(|end| index < end),
                        "startup watch contains a model-bus write after the primary sentinel")?,
                    TransferAccess::HostRead | TransferAccess::ModelBusRead => {}
                },
                MemoryEvent::Cpu(cpu) => {
                    require(cpu.instructions.0 >= previous.0 && cpu.tick >= previous.1, "startup CPU attempt chronology differs")?;
                    previous = (cpu.instructions.0, cpu.tick);
                    require(cpu.size == 4 && matches!(cpu.byte_order, ByteOrder::Little)
                        && (cpu.address.0 == cpu.physical.0 || cpu.address.0 == cpu.physical.0 + 0x4000_0000),
                        "startup CPU attempt width, byte order or alias differs")?;
                    let before: [u8; 4] = cpu.before.as_ref().and_then(|bytes| bytes.as_slice().try_into().ok())
                        .ok_or("startup CPU attempt lacks four before bytes")?;
                    let before = u64::from(u32::from_le_bytes(before));
                    let value = cpu.value;
                    let in_startup = (program.startup.0..program.startup.1).contains(&cpu.pc.0);
                    let allowed = match cpu.source {
                        CpuAccess::CpuFetch => return Err("startup watch contains a protocol-range instruction fetch".into()),
                        CpuAccess::CpuRead => {
                            require(value == Some(before), "startup CPU read value differs from its before bytes")?;
                            match (cpu.core, cpu.pc.0, cpu.physical.0) {
                                (WitnessCore::Core1, pc, 0x20000) if pc == sites.peer_load.0 => {
                                    pairs += 1;
                                    let cleared = primary_clear.is_some_and(|clear| index > clear);
                                    cpu.address.0 == 0x4002_0000 && value == Some(if cleared { 0 } else { u64::from(SENTINEL) })
                                }
                                (WitnessCore::Core0, 0x114, 0x20004) => {
                                    polls += 1;
                                    delayed && cpu.address.0 == 0x4002_0004 && matches!(value, Some(0 | 0x22b))
                                }
                                (WitnessCore::Core0, 0x2020, 0x20004) => {
                                    let cleared = peer_clear.is_some_and(|clear| index > clear);
                                    cpu.address.0 == 0x4002_0004 && value == Some(if cleared { 0 } else { u64::from(SENTINEL) })
                                }
                                (WitnessCore::Core1, pc, 0x14c80) if pc == sites.target_load.0 => segment_entry.is_some() && value == segment_entry,
                                (WitnessCore::Core1, pc, 0x14c84) if pc == sites.argument_load.0 => argument.is_some() && value == argument,
                                _ => false,
                            }
                        }
                        CpuAccess::CpuWrite => match (cpu.core, cpu.pc.0, cpu.physical.0, value) {
                            (WitnessCore::Core0, 0x1e64, 0x20000, Some(0x22b)) | (WitnessCore::Core0, 0x1e68, 0x20004, Some(0x22b))
                            | (WitnessCore::Core0, CLEAR, 0x20000, Some(0)) => before == u64::from(SENTINEL),
                            (WitnessCore::Core0, 0x1fec, 0xa00c4, Some(0)) => before == 0,
                            (WitnessCore::Core1, pc, 0x20004, Some(0)) if pc == sites.clear_store.0 => before == u64::from(SENTINEL),
                            (WitnessCore::Core1, pc, 0x14c80, Some(target)) if pc == sites.target_store.0 =>
                                before == 0 && segment_entry == Some(target),
                            (WitnessCore::Core1, pc, 0x14c84, Some(stored)) if pc == sites.argument_store.0 =>
                                before == 0 && argument == Some(stored),
                            (WitnessCore::Core1, _, 0x14c80 | 0x14c84, Some(0)) => in_startup && before == 0xffff_ffff,
                            _ => false,
                        },
                    };
                    require(allowed, "startup watch contains an unexplained CPU attempt")?;
                }
            }
        }
        require(pairs <= PAIRS && polls <= VENEER_POLLS && (!held || polls == 0), "startup peer-read or veneer-poll bound exceeded")
    }

    fn unique<'r>(&self, rows: &'r [(Epoch, MemoryEvent)], core: WitnessCore, pc: u32, physical: u64)
        -> Result<(usize, &'r CpuAttempt), String> {
        let mut found = rows.iter().enumerate()
            .filter_map(|(index, (_, row))| cpu_row(row, core, pc, physical, CpuAccess::CpuWrite).map(|cpu| (index, cpu)));
        let first = found.next().ok_or_else(|| format!("startup required write attempt at {pc:#x} to {physical:#x} absent"))?;
        require(found.next().is_none(), "startup required write attempt repeated")?;
        Ok(first)
    }

    fn accept(&self) -> Result<(), String> {
        self.check_rows()?;
        let rows = self.rows()?;
        let program = self.program();
        let sites = &program.sites;
        let stop = |label: Label| self.stops.iter().find(|stop| stop.label == label)
            .ok_or_else(|| format!("startup {} stop absent", label.name()));
        let entry = stop(Label::StartupEntry)?;
        let (s0, sentinel0) = self.unique(&rows, WitnessCore::Core0, 0x1e64, 0x20000)?;
        let (s1, sentinel1) = self.unique(&rows, WitnessCore::Core0, 0x1e68, 0x20004)?;
        let (peer_index, _) = self.unique(&rows, WitnessCore::Core1, sites.clear_store.0, 0x20004)?;
        self.unique(&rows, WitnessCore::Core0, 0x1fec, 0xa00c4)?;
        let zeroing = |physical: u64| -> Result<usize, String> {
            let found: Vec<usize> = rows.iter().enumerate().filter_map(|(index, (_, row))| match row {
                MemoryEvent::Cpu(cpu) if cpu.core == WitnessCore::Core1 && cpu.physical.0 == physical
                    && matches!(cpu.source, CpuAccess::CpuWrite) && cpu.value == Some(0)
                    && (program.startup.0..program.startup.1).contains(&cpu.pc.0) => Some(index),
                _ => None,
            }).collect();
            require(found.len() == 1, "startup publication globals are not zeroed exactly once")?;
            Ok(found[0])
        };
        let zero_target = zeroing(0x14c80)?;
        let zero_argument = zeroing(0x14c84)?;
        let first_read = rows.iter().position(|(_, row)| cpu_row(row, WitnessCore::Core1, sites.peer_load.0, 0x20000, CpuAccess::CpuRead).is_some())
            .ok_or("startup peer-cell load absent")?;
        require(s0 < s1 && sentinel0.instructions.0 < sentinel1.instructions.0 && sentinel1.instructions.0 < entry.instructions.0
            && zero_target < peer_index && zero_argument < peer_index && peer_index < first_read,
            "startup sentinel, zeroing, peer-clear and first-load order differs")?;
        let publications = rows.iter().filter(|(_, row)| matches!(row, MemoryEvent::Cpu(cpu)
            if matches!(cpu.source, CpuAccess::CpuWrite) && cpu.core == WitnessCore::Core1
                && (cpu.pc.0 == sites.target_store.0 || cpu.pc.0 == sites.argument_store.0))).count();
        if self.contract.scenario == (Scenario::HeldSentinel {}) {
            require(!rows.iter().any(|(_, row)| cpu_row(row, WitnessCore::Core0, CLEAR, 0x20000, CpuAccess::CpuWrite).is_some())
                && publications == 0 && !self.phase.exit, "held control contains a clear, a publication or a released wait")?;
            return Ok(());
        }
        let (clear_index, _) = self.unique(&rows, WitnessCore::Core0, CLEAR, 0x20000)?;
        let (target_index, target) = self.unique(&rows, WitnessCore::Core1, sites.target_store.0, 0x14c80)?;
        let (argument_index, argument) = self.unique(&rows, WitnessCore::Core1, sites.argument_store.0, 0x14c84)?;
        let exit_read = rows.iter().position(|(_, row)| cpu_row(row, WitnessCore::Core1, sites.peer_load.0, 0x20000, CpuAccess::CpuRead)
            .is_some_and(|cpu| cpu.value == Some(0))).ok_or("startup released peer-cell load absent")?;
        require(clear_index < exit_read && exit_read < target_index && target_index < argument_index
            && target.instructions.0 < argument.instructions.0, "startup publication order differs")?;
        if let Some(ack) = self.phase.ack {
            let ack = self.global(&self.stops[ack])?;
            require(rows.get(peer_index + 1..ack).is_some_and(|interval| interval.iter().any(|(_, row)|
                cpu_row(row, WitnessCore::Core0, 0x114, 0x20004, CpuAccess::CpuRead).is_some_and(|cpu| cpu.value == Some(0)))),
                "delayed control acknowledgement lacks a completed zero poll after the peer clear")?;
        }
        Ok(())
    }

    fn verify_record(&mut self, machine: &Machine) -> Result<(), String> {
        let length = (RECORD.1 - RECORD.0) as usize;
        let guest = wait::bytes(machine, 1, RECORD.0, length)?;
        let physical = machine.read(RECORD.0, length)?;
        let observed = Sha256::digest(&physical);
        let program = self.program();
        let sites = program.sites;
        let mut checked = Vec::new();
        let mut site = |address: u32| -> Result<Word, String> {
            let word = word_at(&physical, address)?;
            checked.push((Guest(address), word));
            Ok(word)
        };
        let entry = sites.entry.0;
        let first = site(entry)?;
        let second = site(entry + 4)?;
        let third = if program.kind == ProgramKind::Owned { Some(site(entry + 8)?) } else { None };
        let load = site(sites.peer_load.0)?;
        let call = if program.kind == ProgramKind::Owned { Some(site(sites.peer_loaded.0 - 4)?) } else { None };
        let stores = [site(sites.clear_store.0)?, site(sites.target_store.0)?, site(sites.argument_store.0)?];
        let loads = [site(sites.target_load.0)?, site(sites.argument_load.0)?];
        let handoff = site(sites.handoff.0)?;
        self.record = Some(RecordCheck { sha256: hex(&observed), site_words: checked });
        require(guest == physical && observed[..] == program.record[..],
            "startup record-2 bytes or the active core-1 view differ from the contract digest")?;
        require(bl_target(first, entry) == Some(0x11854) && bl_target(second, entry + 4) == Some(0x118b8)
            && third.is_none_or(|word| bl_target(word, entry + 8) == Some(sites.main.0)),
            "startup entry does not call the retained state and stack services before main")?;
        require(load_register(load).is_some_and(|(rt, _)| rt == u32::from(program.loaded)), "startup peer load site is not a word load into its register")?;
        require(match call {
            None => sites.peer_loaded.0 == sites.peer_load.0 + 4,
            Some(word) => bl_target(word, sites.peer_loaded.0 - 4)
                .is_some_and(|target| target <= sites.peer_load.0 && sites.peer_load.0 < target + 32),
        }, "startup peer-loaded site does not follow the peer load")?;
        require(stores.iter().all(|&word| is_store(word)), "startup store site is not a word store")?;
        require(loads.iter().all(|&word| load_register(word).is_some_and(|(rt, rn)| rt == rn)), "startup handoff load site differs")?;
        require(handoff.0 == 0xe1a0_f003, "startup handoff site is not mov pc,r3")
    }

    fn publish(&mut self, machine: &mut Machine) -> Result<(), String> {
        let mut argument_view = [0; 4];
        machine.session.board.read_memory(1, ARGUMENT_SOURCE, &mut argument_view)?;
        let argument = word(machine, ARGUMENT_SOURCE)?;
        require(u32::from_le_bytes(argument_view) == argument.0, "startup image-4 argument view differs")?;
        let root = word(machine, SEGMENT_ROOT)?;
        let mut found = None;
        for index in 0..SEGMENT_LIMIT {
            let record = root.0.wrapping_add(index * SEGMENT_RECORD);
            let id = word(machine, self.physical_of(record, 8)?)?;
            require(id.0 != u32::MAX, "startup segment id 1 is absent")?;
            if id.0 == 1 {
                found = Some((index, record));
                break;
            }
        }
        let (index, record) = found.ok_or("startup segment id 1 lies beyond the bounded scan")?;
        let record_physical = self.physical_of(record, 8)?;
        let header = word(machine, record_physical + 4)?;
        let entry = word(machine, self.physical_of(header.0, 16)? + 0xc)?;
        wait::verify_image(machine, wait::OriginalImage::LoaderB)?;
        let loader = machine.read(LOADER_B.0, (LOADER_B.1 - LOADER_B.0) as usize)?;
        let loader_entry = word(machine, LOADER_B.0 + 0xc)?;
        self.segment = Some(Segment {
            root, index, record: Word(record), header, entry, argument,
            loader_sha256: hex(&Sha256::digest(&loader)), loader_entry,
        });
        require(entry.0 == LOADER_ENTRY && loader_entry.0 == LOADER_ENTRY, "startup segment id 1 entry differs from the loader-B header entry")?;
        let root_physical = self.physical_of(root.0, 4)?;
        self.phase.published = vec![
            LOADER_B,
            (ARGUMENT_SOURCE, ARGUMENT_SOURCE + 4),
            (SEGMENT_ROOT, SEGMENT_ROOT + 4),
            (root_physical, record_physical + u64::from(SEGMENT_RECORD)),
        ];
        Ok(())
    }

    fn handoff(&self, stop: &StartupStop) -> Result<(), String> {
        let main = &self.stops[self.phase.main.ok_or("startup handoff precedes the post-initialization stop")?];
        let segment = self.segment.as_ref().ok_or("startup handoff precedes publication")?;
        require(stop.cpsr.0 & 0x0100_02ff == 0xd3, "startup handoff is not masked A32 little-endian SVC")?;
        require(stop.registers[3] == segment.entry && stop.state.target == segment.entry,
            "startup handoff target differs from the loader-B segment entry")?;
        require(stop.registers[0] == segment.argument && stop.state.argument == segment.argument,
            "startup handoff argument differs from image-4 word +0xc")?;
        require(stop.sp == main.sp, "startup handoff stack differs from the post-initialization stack")?;
        require(stop.registers[6..12] == main.registers[6..12], "startup handoff r6-r11 differ from the post-initialization values")?;
        require(stop.state.cell0.0 == 0 && stop.state.cell1.0 == 0, "startup handoff peer cells are not clear")
    }

    /// Returns whether the scenario's endpoint was accepted.
    fn stop(&mut self, machine: &mut Machine, hit: &Hit) -> Result<bool, String> {
        let address = hit_address(hit)?;
        let label = *self.sites.get(&address).ok_or("startup trace contains an unexpected address")?;
        require(hit.site == label.name(), "startup trace label differs from its armed site")?;
        let snapshot = self.snapshot(machine, label, hit)?;
        let core = snapshot.core;
        let tcm = snapshot.tcm;
        self.stops.push(snapshot);
        let index = self.stops.len() - 1;
        if label == Label::ForbiddenFetch {
            return Err(format!("startup forbidden fetch of retired stock code at {address:#x} on core {}", hit.core));
        }
        require(label.core() == Some(core), "startup stop has the wrong core")?;
        let program = self.program();
        let sites = program.sites;
        let scenario = self.contract.scenario;
        let mut accepted = false;
        match label {
            Label::HeldRelease => {
                require(!self.phase.release, "startup release repeated")?;
                let observed: BoardState = serde_json::from_value(machine.session.board.state()).map_err(|error| error.to_string())?;
                let decoder = observed.memory_decoders;
                require(decoder.choices == [1] && decoder.pending == [None] && decoder.failure.is_none(),
                    "startup low-memory decoder is not committed to RAM at core-1 release")?;
                let mut maps = Vec::new();
                for (guest, physical, size) in REQUIRED_MAPS {
                    require(mapping(&self.maps, 1, guest, physical, size), "startup required core-1 mapping absent")?;
                    maps.push(BoundMap { core: 1, address: Address(guest), physical: Address(physical), size });
                }
                self.bindings = Some(Bindings { decoder_choices: decoder.choices, maps, tcm });
                self.phase.release_tcm = Some(tcm);
                self.phase.release = true;
                self.remove(machine, HELD_PC)?;
                self.begin_epoch(machine, Epoch::Released)?;
                self.tcm_disjoint(&tcm)?;
                let mut add = vec![(sites.entry.0, Label::StartupEntry)];
                add.extend(program.forbid_fetch.iter().map(|&(start, _)| (start, Label::ForbiddenFetch)));
                self.add(machine, &add)?;
            }
            Label::StartupEntry => {
                require(self.phase.release && !self.phase.entry, "startup entry out of phase")?;
                self.tcm_stable(&tcm)?;
                self.verify_record(machine)?;
                self.phase.entry = true;
                self.remove(machine, sites.entry.0)?;
                self.add(machine, &[(sites.main.0, Label::StartupMain)])?;
            }
            Label::StartupMain => {
                require(self.phase.entry && self.phase.main.is_none(), "startup post-initialization stop out of phase")?;
                let stop = &self.stops[index];
                let release = self.phase.release_tcm.ok_or("startup release TCM binding absent")?;
                require(stop.cpsr.0 & 0x0100_02ff == 0xd3 && stop.sp.0 == SVC_TOP,
                    "startup post-initialization state is not masked SVC on the requested stack")?;
                require(tcm[1][0].0 == ATCM_INIT && tcm[1][1] == release[1][1] && tcm[0] == release[0],
                    "startup TCM change differs from the source-bound ATCM initialization")?;
                self.tcm_disjoint(&tcm)?;
                self.phase.main_tcm = Some(tcm);
                self.phase.last_pair = stop.watch_records;
                self.phase.main = Some(index);
                self.remove(machine, sites.main.0)?;
                self.add(machine, &[(sites.peer_loaded.0, Label::PeerLoaded), (sites.handoff.0, Label::StartupHandoff)])?;
            }
            Label::PeerLoaded => {
                require(self.phase.main.is_some() && !self.phase.exit, "startup peer pair out of phase")?;
                self.tcm_stable(&tcm)?;
                let watch = self.watch_now()?;
                let stop = &self.stops[index];
                let interval = watch.records.get(self.phase.last_pair..stop.watch_records).ok_or("startup pair cursor interval differs")?;
                let reads: Vec<&CpuAttempt> = interval.iter()
                    .filter_map(|row| cpu_row(row, WitnessCore::Core1, sites.peer_load.0, 0x20000, CpuAccess::CpuRead)).collect();
                let skipped = if self.phase.rejoin { reads.len().saturating_sub(1) } else { 0 };
                require(reads.len() == skipped + 1, "startup pair lacks exactly one completed peer-cell load")?;
                require(reads[..skipped].iter().all(|read| read.value == Some(u64::from(SENTINEL))),
                    "startup load completed during the clear microsteps did not read the sentinel")?;
                if self.phase.rejoin {
                    self.phase.last_waiting_core1 = None;
                    self.phase.rejoin = false;
                }
                let read = reads[skipped];
                let before = read.before.as_ref().and_then(|bytes| <[u8; 4]>::try_from(bytes.as_slice()).ok())
                    .ok_or("startup pair load lacks four before bytes")?;
                let value = u32::from_le_bytes(before);
                require(read.address.0 == 0x4002_0000 && read.size == 4 && read.value == Some(u64::from(value))
                    && read.instructions.0 <= stop.instructions.0, "startup pair load attribution differs")?;
                require(stop.registers[usize::from(program.loaded)].0 == value, "startup pair register differs from the loaded peer-cell value")?;
                let core1 = stop.counts.0[1];
                self.phase.pairs += 1 + skipped;
                self.phase.last_pair = stop.watch_records;
                require(self.phase.pairs <= PAIRS, "startup peer pair bound exceeded")?;
                if value == SENTINEL {
                    if let Some(previous) = self.phase.last_waiting_core1 {
                        require(core1.checked_sub(previous) == Some(program.period), "startup waiting pairs differ from the poll period")?;
                    }
                    self.phase.last_waiting_core1 = Some(core1);
                    self.phase.waiting += 1;
                    if scenario == (Scenario::HeldSentinel {}) && self.phase.waiting == HELD_PAIRS {
                        let gate = &self.stops[self.phase.gate.ok_or("held control lacks the primary gate")?];
                        let final_primary = wait::primary(machine)?;
                        let same = final_primary.address.0 == GATE
                            && wait::same_context(&final_primary, &gate.registers, gate.sp, gate.lr, gate.cpsr, false);
                        self.final_primary = Some(final_primary);
                        require(same, "held primary context changed")?;
                        accepted = true;
                    }
                } else {
                    require(value == 0, "startup peer cell holds neither the sentinel nor zero")?;
                    require(scenario != (Scenario::HeldSentinel {}), "held control observed a released peer cell")?;
                    require(self.contract.scenario.control().is_none() || self.phase.clear.is_some(),
                        "delayed control observed release before the committed primary clear")?;
                    self.phase.exit = true;
                    self.publish(machine)?;
                    self.begin_epoch(machine, Epoch::Published)?;
                    self.remove(machine, sites.peer_loaded.0)?;
                    if scenario.endpoint() == Some(Endpoint::LoaderEntry) {
                        self.add(machine, &[(LOADER_ENTRY, Label::LoaderBEntry)])?;
                    }
                }
            }
            Label::PrimaryGate => {
                self.tcm_stable(&tcm)?;
                let stop = &self.stops[index];
                require(scenario.control().is_some() && self.phase.gate.is_none() && self.phase.clear.is_none()
                    && stop.registers[2].0 == 0 && stop.registers[3].0 == 0 && stop.sp.0 == 0x7000_07f0
                    && stop.cpsr.0 & 0x0100_02ff == 0xd3 && stop.state.cell0.0 == SENTINEL
                    && matches!(stop.state.cell1.0, 0 | SENTINEL) && stop.state.completion.0 == 0
                    && stop.state.target.0 == 0 && stop.state.argument.0 == 0, "startup primary gate context differs")?;
                let control = scenario.control().ok_or("startup gate outside a control scenario")?;
                self.gate = Some(wait::gate_bindings(machine, control)?);
                self.phase.gate = Some(index);
                self.remove(machine, GATE)?;
                if control == Control::DelayedRelease {
                    self.add(machine, &[(ACKNOWLEDGED, Label::PeerAcknowledged), (CLEAR, Label::PrimaryClear), (FAILURE, Label::ControlFailure)])?;
                }
            }
            Label::PeerAcknowledged => {
                self.tcm_stable(&tcm)?;
                let stop = &self.stops[index];
                require(matches!(scenario, Scenario::DelayedRelease { .. }) && self.phase.gate.is_some() && self.phase.main.is_some()
                    && self.phase.ack.is_none() && self.phase.clear.is_none() && wait::pending(&stop.state) && stop.registers[2].0 == 0,
                    "startup peer acknowledgement out of phase")?;
                self.phase.ack = Some(index);
                self.phase.pairs_at_ack = self.phase.pairs;
                self.remove(machine, ACKNOWLEDGED)?;
            }
            Label::PrimaryClear => {
                let stop = &self.stops[index];
                require(self.phase.ack.is_some() && self.phase.waiting >= HELD_PAIRS && self.phase.clear.is_none() && wait::pending(&stop.state),
                    "startup original clear occurred before the required wait observations")?;
                let gate = &self.stops[self.phase.gate.ok_or("startup primary gate absent")?];
                require(wait::same_context(&wait::primary(machine)?, &gate.registers, gate.sp, gate.lr, gate.cpsr, true),
                    "startup veneer did not restore the primary context")?;
                let preclear_cpsr = stop.cpsr;
                let preclear_cursor = self.global(stop)?;
                self.remove(machine, CLEAR)?;
                self.remove(machine, sites.peer_loaded.0)?;
                let committed = wait::commit_clear(machine)?;
                let cleared = committed.cleared();
                let wait::CommittedClear { single_core0_step, before_pc, before_cpsr, current, cell0, state, original, microsteps } = committed;
                let cursor = self.watch_now()?.records.len();
                self.clear_readback = Some(ClearReadback {
                    instructions: Instructions(machine.instructions()), primary_address: current.address, primary_cpsr: current.cpsr,
                    primary_instruction: original, state, watch_records: WatchCursor(cursor), microsteps,
                });
                self.check_rows()?;
                let rows = self.rows()?;
                let (row_index, attempt) = self.unique(&rows, WitnessCore::Core0, CLEAR, 0x20000)?;
                require(single_core0_step && before_pc.0 == CLEAR && current.address.0 == CLEAR + 4
                    && before_cpsr == preclear_cpsr && current.cpsr == before_cpsr && cell0.0 == 0 && cleared
                    && row_index >= preclear_cursor && row_index < self.epoch_offset(self.epoch)? + cursor
                    && attempt.instructions.0 == machine.instructions() && attempt.before.as_deref() == Some(&[0x2b, 2, 0, 0][..]),
                    "startup attempted clear lacks original instruction completion and immediate zero readback")?;
                let ack = &self.stops[self.phase.ack.ok_or("startup acknowledgement absent")?];
                let now = wait::counts(machine)?;
                let core0 = now.0[0].saturating_sub(ack.counts.0[0]);
                let core1 = now.0[1].saturating_sub(ack.counts.0[1]);
                require(core1 <= QUANTUM * (core0.div_ceil(QUANTUM) + 1), "startup core-1 progress from acknowledgement to clear exceeds the scheduler bound")?;
                require((self.phase.pairs - self.phase.pairs_at_ack) as u64 <= core1 / program.period + 1,
                    "startup pairs from acknowledgement to clear exceed the poll-period bound")?;
                self.phase.clear = Some(Instructions(machine.instructions()));
                self.add(machine, &[(sites.peer_loaded.0, Label::PeerLoaded)])?;
                self.phase.rejoin = true;
            }
            Label::ControlFailure => return Err("guest delayed control entered its failure loop".into()),
            Label::StartupHandoff => {
                require(scenario != (Scenario::HeldSentinel {}) && self.phase.exit && !self.phase.handoff, "startup handoff out of phase")?;
                self.tcm_stable(&tcm)?;
                self.handoff(&self.stops[index])?;
                self.phase.handoff = true;
                self.remove(machine, sites.handoff.0)?;
                accepted = scenario.endpoint() == Some(Endpoint::Handoff);
            }
            Label::LoaderBEntry => {
                require(self.phase.handoff && scenario.endpoint() == Some(Endpoint::LoaderEntry) && address == LOADER_ENTRY,
                    "startup loader-B entry out of phase")?;
                self.tcm_stable(&tcm)?;
                self.handoff(&self.stops[index])?;
                wait::verify_image(machine, wait::OriginalImage::LoaderB)?;
                accepted = true;
            }
            Label::ForbiddenFetch => unreachable!(),
        }
        self.check_rows()?;
        if accepted {
            self.accept()?;
        }
        Ok(accepted)
    }

    fn execute(&mut self, machine: &mut Machine) -> Result<(), String> {
        loop {
            let end = self.phase_end();
            require(machine.instructions() < end, "startup phase or aggregate instruction budget exhausted")?;
            let before = machine.traces.hits.len();
            machine.run(end - machine.instructions());
            machine.require_running()?;
            let hits = machine.traces.hits[before..].to_vec();
            require(!hits.is_empty(), "startup boundary not reached within the phase budget")?;
            require(hits.len() == 1 || hits.iter().all(|hit| hit_address(hit).ok() != Some(CLEAR)),
                "startup primary clear coincided with another stop")?;
            for hit in &hits {
                if self.stop(machine, hit)? {
                    return Ok(());
                }
            }
        }
    }
}

pub(super) fn run(mut machine: Machine, contract: &StartupContract, output: &Path) -> Result<(), String> {
    let mut identities = [Identity(0); 2];
    let mut runner = Runner::new(contract);
    let outcome = (|| {
        require(wait::counts(&machine)?.0 == [0, 0] && !machine.traces.armed() && machine.traces.hits.is_empty(),
            "startup witness must arm before either core executes")?;
        let (maps, configured) = bindings(&machine.document)?;
        identities = configured;
        require(machine.document.execution.quantum == QUANTUM && matches!(machine.document.execution.engine, RunEngine::Unicorn)
            && machine.document.execution.instructions_per_tick.numerator == 24
            && machine.document.execution.instructions_per_tick.denominator == 1, "startup scheduling configuration differs")?;
        for (core, expected) in identities.iter().enumerate() {
            require(machine.session.board.cpu(core)?.rprofile_identity(None)? == expected.0, "startup active CP14 identity differs")?;
        }
        runner.maps = maps;
        runner.begin_epoch(&mut machine, Epoch::Reset)?;
        let mut initial = BTreeMap::from([(HELD_PC, Label::HeldRelease)]);
        if contract.scenario.control().is_some() {
            initial.insert(GATE, Label::PrimaryGate);
        }
        runner.arm(&mut machine, initial)?;
        runner.execute(&mut machine)
    })();
    let outcome = runner.finish(&mut machine, outcome);
    let receipt = Receipt {
        schema: 3, scenario: contract.scenario, program: contract.program.kind,
        result: if outcome.is_ok() { WitnessResult::Passed } else { WitnessResult::Failed },
        error: outcome.as_ref().err().map(String::as_str), machine_failure: machine.failure.as_deref(),
        instructions: Instructions(machine.instructions()), cp14_identities: identities,
        epochs: &runner.epochs, stops: &runner.stops, bindings: &runner.bindings, gate: &runner.gate,
        clear_readback: &runner.clear_readback, final_primary: &runner.final_primary, segment: &runner.segment,
        record: &runner.record,
    };
    let bytes = serde_json::to_vec_pretty(&receipt).map_err(|error| error.to_string())?;
    require(bytes.len() <= MAX_RECEIPT, "startup receipt exceeds 16 MiB")?;
    let path = output.join("peer-witness.json");
    std::fs::write(&path, bytes).map_err(|error| format!("{}: {error}", path.display()))?;
    println!("{}", path.display());
    outcome
}
