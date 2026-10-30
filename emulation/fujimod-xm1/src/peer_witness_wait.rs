use super::*;
use fujimod_catalog::configuration::RunEngine;
use sha2::{Digest, Sha256};

const BUDGET: u64 = 10_000_000;
const PREFIX: u64 = 4_000_000;
const HELD: u64 = 4_000_000;
const DELAYED: u64 = 5_100_000;
const RELEASE: u64 = 20_000;
const MICROSTEPS: usize = 2048;
const ROWS: usize = 12_288;
const WAIT_READS: usize = 128;
const ARGUMENT: u32 = 0x69cdf8;
const VENEER: [u32; 26] = [
    0xe92d500f, 0xe10f0000, 0xe3021000, 0xe3443002, 0xe300c22b,
    0xe5932004, 0xe3520000, 0x0a00000a, 0xe152000c, 0x1a000005,
    0xe3a02064, 0xe320f000, 0xe2522001, 0x1afffffc, 0xe2511001,
    0x1afffff4, 0xe128f000, 0xe8bd500f, 0xeafffffe, 0xe3011000,
    0xe2511001, 0x1afffffd, 0xe128f000, 0xe8bd500f, 0xe3443002,
    0xea0007a3,
];

#[derive(Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub(super) enum Control { HeldSentinel, DelayedRelease }

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ControlContract {
    schema: u32,
    pub(super) control: Control,
    pub(super) budget: Instructions,
}
impl ControlContract {
    pub(super) fn validate(&self) -> Result<(), String> {
        require(self.schema == 2 && self.budget.0 == BUDGET, "wait control requires schema 2 and budget 10000000")
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum Stop {
    #[serde(rename = "record-2-entry")]
    Entry,
    #[serde(rename = "record-2-rendezvous-tail")]
    Tail,
    PeerWaitLoaded,
    PrimaryGate,
    PeerAcknowledged,
    PrimaryClear,
    ControlFailure,
    #[serde(rename = "record-2-handoff")]
    Handoff,
    LoaderBEntry,
}
impl Stop {
    const ALL: [Self; 9] = [Self::Entry, Self::Tail, Self::PeerWaitLoaded, Self::PrimaryGate,
        Self::PeerAcknowledged, Self::PrimaryClear, Self::ControlFailure, Self::Handoff, Self::LoaderBEntry];
    fn address(self) -> u32 {
        match self {
            Self::Entry => 0x11800, Self::Tail => 0x11f40, Self::PeerWaitLoaded => 0x11f78,
            Self::PrimaryGate => 0x1ff4, Self::PeerAcknowledged => 0x14c, Self::PrimaryClear => 0x1ff8,
            Self::ControlFailure => 0x148, Self::Handoff => 0x11850, Self::LoaderBEntry => 0xd0020,
        }
    }
    fn name(self) -> &'static str {
        match self {
            Self::Entry => "record-2-entry", Self::Tail => "record-2-rendezvous-tail",
            Self::PeerWaitLoaded => "peer-wait-loaded", Self::PrimaryGate => "primary-gate",
            Self::PeerAcknowledged => "peer-acknowledged", Self::PrimaryClear => "primary-clear",
            Self::ControlFailure => "control-failure", Self::Handoff => "record-2-handoff",
            Self::LoaderBEntry => "loader-b-entry",
        }
    }
    fn core(self) -> WitnessCore {
        match self {
            Self::PrimaryGate | Self::PeerAcknowledged | Self::PrimaryClear | Self::ControlFailure => WitnessCore::Core0,
            _ => WitnessCore::Core1,
        }
    }
    fn enabled(self, control: Control) -> bool {
        control == Control::DelayedRelease || !matches!(self, Self::PeerAcknowledged | Self::ControlFailure)
    }
}

#[derive(Clone, Copy, Serialize)]
#[serde(transparent)]
struct WatchCursor(usize);
#[derive(Serialize)]
struct Boundary {
    #[serde(flatten)]
    hit: PeerHit<Stop>,
    watch_records: WatchCursor,
}
#[derive(Clone, Copy, Serialize)]
#[serde(transparent)]
struct CoreCounts([u64; 2]);
#[derive(Serialize)]
struct Microstep { before: CoreCounts, after: CoreCounts, cell0: Word }
#[derive(Serialize)]
struct ClearReadback {
    instructions: Instructions,
    primary_address: Guest,
    primary_cpsr: Word,
    primary_instruction: [u8; 4],
    state: PeerState,
    watch_records: WatchCursor,
    microsteps: Vec<Microstep>,
}
#[derive(Serialize)]
struct Primary {
    address: Guest,
    registers: [Word; 13],
    sp: Word,
    lr: Word,
    cpsr: Word,
    instructions: Instructions,
}
#[derive(Serialize)]
struct CodeBinding { address: Address, r#virtual: Vec<u8>, physical: Vec<u8> }
#[derive(Serialize)]
struct LowRam { address: Address, physical: Address, size: usize, permissions: u32 }
#[derive(Serialize)]
struct GateBindings {
    core: WitnessCore,
    instructions: Instructions,
    decoder_choices: Vec<usize>,
    low_ram: LowRam,
    atcm: Word,
    btcm: Word,
    stack_address: Address,
    stack: Vec<u8>,
    code: Vec<CodeBinding>,
}
#[derive(Serialize)]
#[serde(untagged)]
enum WatchEvidence {
    Observed(WatchState),
    Rejected(Box<serde_json::value::RawValue>),
}
#[derive(Serialize)]
struct Receipt<'a> {
    schema: u32,
    control: Control,
    result: WitnessResult,
    error: Option<&'a str>,
    machine_failure: Option<&'a str>,
    instructions: Instructions,
    cp14_identities: [Identity; 2],
    stops: &'a [Boundary],
    bindings: &'a Option<GateBindings>,
    final_primary: &'a Option<Primary>,
    clear_readback: &'a Option<ClearReadback>,
    memory_watch: WatchEvidence,
}
#[derive(Default)]
struct Observations {
    stops: Vec<Boundary>,
    bindings: Option<GateBindings>,
    final_primary: Option<Primary>,
    clear_readback: Option<ClearReadback>,
}
#[derive(Default)]
struct Phase {
    entry: bool,
    tail: bool,
    waits: usize,
    gate: Option<usize>,
    ack: bool,
    clear: bool,
    handoff: bool,
}

fn require(condition: bool, problem: &str) -> Result<(), String> {
    if condition { Ok(()) } else { Err(problem.into()) }
}
fn pending(state: &PeerState) -> bool {
    state.cell0.0 == 0x22b && state.cell1.0 == 0 && state.completion.0 == 0
        && state.target.0 == 0 && state.argument.0 == 0
}
fn primary(machine: &Machine) -> Result<Primary, String> {
    let mut registers = [Word(0); 13];
    for (index, value) in registers.iter_mut().enumerate() {
        *value = register(machine, 0, arm::Register::R(index as u8))?;
    }
    Ok(Primary {
        address: Guest(register(machine, 0, arm::Register::R(15))?.0), registers,
        sp: register(machine, 0, arm::Register::R(13))?, lr: register(machine, 0, arm::Register::R(14))?,
        cpsr: register(machine, 0, arm::Register::Cpsr)?, instructions: Instructions(machine.instructions()),
    })
}
fn same_context(observed: &Primary, gate: &PeerHit<Stop>, clear: bool) -> bool {
    observed.registers.iter().enumerate().all(|(index, word)| {
        word.0 == if clear && index == 3 { 0x40020000 } else { gate.registers[index].0 }
    }) && observed.sp == gate.sp && observed.lr == gate.lr && observed.cpsr == gate.cpsr
}
fn counts(machine: &Machine) -> Result<CoreCounts, String> {
    machine.session.board.instruction_counts().try_into().map(CoreCounts).map_err(|_| "control requires two instruction counters".into())
}
fn bytes(machine: &Machine, core: usize, address: u64, length: usize) -> Result<Vec<u8>, String> {
    let mut result = vec![0; length];
    machine.session.board.read_memory(core, address, &mut result)?;
    Ok(result)
}
enum OriginalImage { Record2, LoaderB }
fn verify_image(machine: &Machine, image: OriginalImage) -> Result<(), String> {
    let (address, length, expected): (u64, usize, [u8; 32]) = match image {
        OriginalImage::Record2 => (0x10000, 0x4c80, [
            0x2f,0x1b,0xf3,0xf0,0xe4,0x83,0xb8,0x92,0xd2,0x89,0x05,0x99,0x67,0x9c,0x1b,0x51,
            0x5c,0xfc,0x1b,0xe8,0xb8,0xc2,0x7c,0xbb,0x06,0xd4,0xd9,0x13,0xe4,0x28,0xab,0xb4]),
        OriginalImage::LoaderB => (0xd0000, 0xdaa0, [
            0x8f,0xb0,0xc3,0x22,0xa2,0x24,0x43,0xa3,0x78,0x3c,0xda,0xb9,0x07,0x1d,0xb1,0x72,
            0x12,0xa5,0x62,0x9f,0x63,0x75,0x64,0x62,0x18,0xac,0x1e,0x04,0x3d,0xc6,0x72,0xec]),
    };
    for bank in 0..2 {
        let register = tcm(machine, WitnessCore::Core1, bank)?.0;
        let base = u64::from(register & 0xfffff000);
        let size = 512u64 << ((register >> 2) & 31);
        require(register & 1 == 0 || base >= address + length as u64 || base + size <= address,
            "control original image intersects private TCM")?;
    }
    let mut physical = vec![0; length];
    machine.session.board.read_physical(address, &mut physical)?;
    let virtual_bytes = bytes(machine, 1, address, length)?;
    require(virtual_bytes == physical && Sha256::digest(&physical)[..] == expected[..],
        "control original guest image digest or active alias differs")
}
fn tcm(machine: &Machine, core: WitnessCore, bank: u8) -> Result<Word, String> {
    let core = match core { WitnessCore::Core0 => 0, WitnessCore::Core1 => 1 };
    u32::try_from(machine.session.board.cpu(core)?.coprocessor(arm::Coprocessor {
        cp: 15, opc1: 0, crn: 9, crm: 1, opc2: bank, secure: 0, wide: false,
    }, None)?).map(Word).map_err(|_| "control TCM register exceeds a word".into())
}
#[derive(Deserialize)]
struct DecoderState { choices: Vec<usize>, pending: Vec<Option<usize>>, failure: Option<String> }
#[derive(Deserialize)]
struct BoardState { memory_decoders: DecoderState }

fn gate_bindings(machine: &Machine, control: Control) -> Result<GateBindings, String> {
    let (_, graph) = run::graph(&machine.document)?;
    let observed: BoardState = serde_json::from_value(machine.session.board.state()).map_err(|e| e.to_string())?;
    let decoder = observed.memory_decoders;
    require(decoder.choices == [1] && decoder.pending == [None] && decoder.failure.is_none()
        && graph.decoders.len() == 1, "control low-memory decoder is not committed to RAM")?;
    let selected = graph.decoders[0].choices.get(1).ok_or("control RAM decoder choice absent")?;
    let low = selected.processors.iter().find(|map| map.core == 0 && map.address == 0)
        .ok_or("control active low-memory mapping absent")?;
    require(low.physical == 0 && low.size == 0x4000 && low.permissions == 7,
        "control active low-memory mapping differs")?;
    let atcm = tcm(machine, WitnessCore::Core0, 1)?;
    let btcm = tcm(machine, WitnessCore::Core0, 0)?;
    require(atcm.0 == 0x70000011 && btcm.0 == 0, "control private TCM stack binding differs")?;
    let padding = if control == Control::HeldSentinel { vec![0xff; 104] }
        else { VENEER.iter().flat_map(|word| word.to_le_bytes()).collect() };
    let gate = if control == Control::HeldSentinel { 0xeafffffe } else { 0xeafff841 };
    let predecessors: [u32; 8] = [0xe3003000, 0xe344300a, 0xe3a02000, 0xe58320c4,
        0xe3003000, gate, 0xe5832000, 0xf57ff04f];
    let original = predecessors.iter().flat_map(|word| word.to_le_bytes()).collect();
    let mut code = Vec::with_capacity(2);
    for (address, expected) in [(0x100, padding), (0x1fe0, original)] {
        let virtual_bytes = bytes(machine, 0, address, expected.len())?;
        let mut physical = vec![0; expected.len()];
        machine.session.board.read_physical(address, &mut physical)?;
        require(virtual_bytes == expected && physical == expected, "control active code/predecessor bytes differ")?;
        code.push(CodeBinding { address: Address(address), r#virtual: virtual_bytes, physical });
    }
    Ok(GateBindings {
        core: WitnessCore::Core0, instructions: Instructions(machine.instructions()),
        decoder_choices: decoder.choices,
        low_ram: LowRam { address: Address(low.address), physical: Address(low.physical), size: low.size, permissions: low.permissions },
        atcm, btcm, stack_address: Address(0x700007d8), stack: bytes(machine, 0, 0x700007d8, 40)?, code,
    })
}

fn cpu_matches(row: &CpuAttempt, core: WitnessCore, pc: u32, physical: u64, value: u64, access: CpuAccess) -> bool {
    std::mem::discriminant(&row.source) == std::mem::discriminant(&access)
        && row.core == core && row.pc.0 == pc && row.physical.0 == physical && row.value == Some(value)
        && row.size == 4 && matches!(row.byte_order, ByteOrder::Little)
        && (row.address.0 == physical || row.address.0 == physical + 0x40000000)
        && row.before.as_ref().is_some_and(|bytes| bytes.len() == 4)
}
fn writer<'a>(watch: &'a WatchState, core: WitnessCore, pc: u32, physical: u64, value: u64) -> Result<(usize, &'a CpuAttempt), String> {
    let mut found = watch.records.iter().enumerate().filter_map(|(index, event)| match event {
        MemoryEvent::Cpu(row) if cpu_matches(row, core, pc, physical, value, CpuAccess::CpuWrite) => Some((index, row)),
        _ => None,
    });
    let row = found.next().ok_or("control required CPU write attempt absent")?;
    require(found.next().is_none(), "control CPU write attempt repeated")?;
    Ok(row)
}
fn check_watch(watch: &WatchState) -> Result<(), String> {
    require(watch.enabled && watch.violation.is_none() && watch.records.len() <= ROWS
        && watch.ranges.len() == WATCH_RANGES.len()
        && watch.ranges.iter().zip(WATCH_RANGES).all(|((lo, hi), expected)| (lo.0, hi.0) == expected),
        "control memory watch disabled, violated or over successful bound")?;
    let sentinel_end = watch.records.iter().position(|event| matches!(event, MemoryEvent::Cpu(row)
        if cpu_matches(row, WitnessCore::Core0, 0x1e68, 0x20004, 0x22b, CpuAccess::CpuWrite)));
    let clear_index = watch.records.iter().position(|event| matches!(event, MemoryEvent::Cpu(row)
        if cpu_matches(row, WitnessCore::Core0, 0x1ff8, 0x20000, 0, CpuAccess::CpuWrite)));
    let mut wait_reads = 0;
    let mut polls = 0;
    let mut previous = (0, 0);
    for (index, event) in watch.records.iter().enumerate() {
        let (physical, size) = match event {
            MemoryEvent::Cpu(row) => (row.physical.0, u64::from(row.size)),
            MemoryEvent::Transfer(row) => (row.physical.0, row.size),
        };
        require(size > 0 && physical.checked_add(size).is_some_and(|end|
            WATCH_RANGES.iter().any(|&(lo, hi)| physical < hi && lo < end)), "control event lies outside watched ranges")?;
        match event {
            MemoryEvent::Transfer(row) => {
                require(!matches!(row.source, TransferAccess::HostWrite), "control watch contains a host write")?;
                require(!matches!(row.source, TransferAccess::ModelBusWrite)
                    || sentinel_end.is_none_or(|end| index < end), "control watch contains a post-sentinel model write")?;
            }
            MemoryEvent::Cpu(row) => {
                require(row.instructions.0 >= previous.0 && row.tick >= previous.1,
                    "control CPU attempt chronology differs")?;
                previous = (row.instructions.0, row.tick);
                require(row.size == 4 && matches!(row.byte_order, ByteOrder::Little)
                    && (row.address.0 == row.physical.0 || row.address.0 == row.physical.0 + 0x40000000),
                    "control CPU attempt width/order/alias differs")?;
                let before: [u8; 4] = row.before.as_ref().and_then(|bytes| bytes.as_slice().try_into().ok())
                    .ok_or("control CPU attempt lacks four before bytes")?;
                match row.source {
                    CpuAccess::CpuRead => {
                        require(row.value == Some(u64::from(u32::from_le_bytes(before))), "control CPU read value differs from actual before bytes")?;
                        let allowed = match (row.core, row.pc.0, row.physical.0) {
                            (WitnessCore::Core1, 0x11f74, 0x20000) => {
                                wait_reads += 1;
                                let expected = if clear_index.is_some_and(|clear| index > clear) { 0 } else { 0x22b };
                                row.address.0 == 0x40020000 && row.value == Some(expected)
                            }
                            (WitnessCore::Core0, 0x114, 0x20004) => {
                                polls += 1;
                                row.address.0 == 0x40020004 && matches!(row.value, Some(0 | 0x22b))
                            }
                            (WitnessCore::Core1, 0x11844, 0x14c80) => row.address.0 == 0x14c80 && row.value == Some(0xd0020),
                            (WitnessCore::Core1, 0x1184c, 0x14c84) => row.address.0 == 0x14c84 && row.value == Some(u64::from(ARGUMENT)),
                            (WitnessCore::Core0, 0x2020, 0x20004) => clear_index.is_some_and(|clear| index > clear)
                                && row.address.0 == 0x40020004 && row.value == Some(0),
                            _ => false,
                        };
                        require(allowed, "control watch contains an unexplained CPU read")?;
                    }
                    CpuAccess::CpuWrite => {
                        let allowed = match (row.core, row.pc.0, row.physical.0, row.value) {
                            (WitnessCore::Core0, 0x1e64, 0x20000, Some(0x22b))
                            | (WitnessCore::Core0, 0x1e68, 0x20004, Some(0x22b))
                            | (WitnessCore::Core0, 0x1fec, 0xa00c4, Some(0))
                            | (WitnessCore::Core0, 0x1ff8, 0x20000, Some(0))
                            | (WitnessCore::Core1, 0x11818, 0x14c80 | 0x14c84, Some(0))
                            | (WitnessCore::Core1, 0x11f4c, 0x20004, Some(0))
                            | (WitnessCore::Core1, 0x11f98, 0x14c80, Some(0xd0020)) => true,
                            (WitnessCore::Core1, 0x11fb0, 0x14c84, Some(value)) => value == u64::from(ARGUMENT),
                            _ => false,
                        };
                        require(allowed, "control watch contains an unexplained CPU write attempt")?;
                        let expected_before: u32 = match row.pc.0 {
                            0x11818 => 0xffffffff,
                            0x1e64 | 0x1e68 | 0x1ff8 | 0x11f4c => 0x22b,
                            _ => 0,
                        };
                        require(before == expected_before.to_le_bytes(), "control CPU write preimage differs")?;
                    }
                    CpuAccess::CpuFetch => return Err("control watch contains a data-range instruction fetch".into()),
                }
            }
        }
    }
    require(wait_reads <= WAIT_READS && polls <= 8192, "control equality/poll read bound exceeded")
}

fn loaded(watch: &WatchState, previous: WatchCursor, current: &Boundary) -> Result<(), String> {
    let interval = watch.records.get(previous.0..current.watch_records.0).ok_or("control equality cursor interval differs")?;
    let mut reads = interval.iter().filter_map(|event| match event {
        MemoryEvent::Cpu(row) if cpu_matches(row, WitnessCore::Core1, 0x11f74, 0x20000, 0x22b, CpuAccess::CpuRead) => Some(row),
        _ => None,
    });
    let read = reads.next().ok_or("control equality stop lacks an original completed sentinel load")?;
    require(reads.next().is_none() && read.address.0 == 0x40020000
        && read.before.as_deref() == Some(&[0x2b, 2, 0, 0][..]) && read.instructions.0 < current.hit.instructions.0,
        "control equality load attribution or interval differs")?;
    require(pending(&current.hit.state) && current.hit.registers[1].0 == 0x40020000
        && current.hit.registers[2].0 == 0x22b && current.hit.registers[3].0 == 0x22b,
        "control equality stop register/state readback differs")
}
fn tail(hit: &PeerHit<Stop>) -> Result<(), String> {
    let frame = hit.frame.as_ref().ok_or("control rendezvous frame absent")?;
    require(hit.registers[11].0.checked_sub(hit.sp.0) == Some(60) && hit.registers[6].0 == 1
        && u32::from_le_bytes(frame[20..24].try_into().map_err(|_| "control frame width")?) == 0x1183c,
        "control rendezvous saved frame differs")
}
fn handoff(hit: &PeerHit<Stop>, tail: &PeerHit<Stop>) -> Result<(), String> {
    let frame = tail.frame.as_ref().ok_or("control rendezvous frame absent")?;
    require(hit.cpsr.0 & 0x010002ff == 0xd3 && hit.registers[0].0 == ARGUMENT && hit.registers[3].0 == 0xd0020
        && hit.state.cell0.0 == 0 && hit.state.cell1.0 == 0 && hit.state.target.0 == 0xd0020
        && hit.state.argument.0 == ARGUMENT && tail.sp.0.checked_add(64) == Some(hit.sp.0), "control handoff context differs")?;
    for (index, register) in [4, 5, 6, 7, 11].into_iter().enumerate() {
        require(hit.registers[register].0 == u32::from_le_bytes(frame[4 * index..4 * index + 4].try_into()
            .map_err(|_| "control frame width")?), "control handoff callee-saved context differs")?;
    }
    Ok(())
}
fn clear(machine: &mut Machine, watch: &MemoryWatch, preclear: &Boundary, readback: &mut Option<ClearReadback>) -> Result<(), String> {
    let mut original = [0; 4];
    machine.session.board.read_memory(0, 0x1ff8, &mut original)?;
    let mut physical = [0; 4];
    machine.session.board.read_physical(0x1ff8, &mut physical)?;
    require(original == 0xe5832000u32.to_le_bytes() && physical == original, "control original clear instruction differs")?;
    let mut microsteps = Vec::with_capacity(MICROSTEPS);
    for _ in 0..MICROSTEPS {
        let before = counts(machine)?;
        let before_pc = register(machine, 0, arm::Register::R(15))?;
        let before_cpsr = register(machine, 0, arm::Register::Cpsr)?;
        let hits = machine.traces.hits.len();
        machine.run(1);
        let cell0 = word(machine, 0x20000)?;
        let after = counts(machine)?;
        microsteps.push(Microstep { before, after, cell0 });
        let delta = [after.0[0].checked_sub(before.0[0]), after.0[1].checked_sub(before.0[1])];
        require(matches!(delta, [Some(1), Some(0)] | [Some(0), Some(1)]), "control microstep did not advance exactly one aggregate entered instruction")?;
        machine.require_running()?;
        require(machine.traces.hits.len() == hits, "control boundary occurred before committed clear readback")?;
        if cell0.0 == 0x22b { continue; }
        let current = primary(machine)?;
        let state = state(machine)?;
        let cleared = state.cell0.0 == 0 && state.cell1.0 == 0 && state.completion.0 == 0
            && state.target.0 == 0 && state.argument.0 == 0;
        let observed = watch_state(watch)?;
        let cursor = WatchCursor(observed.records.len());
        *readback = Some(ClearReadback {
            instructions: Instructions(machine.instructions()), primary_address: current.address, primary_cpsr: current.cpsr,
            primary_instruction: original, state, watch_records: cursor, microsteps,
        });
        check_watch(&observed)?;
        let (index, attempt) = writer(&observed, WitnessCore::Core0, 0x1ff8, 0x20000, 0)?;
        require(delta == [Some(1), Some(0)] && before_pc.0 == 0x1ff8 && current.address.0 == 0x1ffc
            && before_cpsr == preclear.hit.cpsr && current.cpsr == before_cpsr && cell0.0 == 0 && cleared
            && index >= preclear.watch_records.0 && index < cursor.0 && attempt.instructions.0 == machine.instructions()
            && attempt.before.as_deref() == Some(&[0x2b, 2, 0, 0][..]), "control attempted clear lacks original instruction completion and immediate zero readback")?;
        return Ok(());
    }
    Err("control original clear not committed within 2048 one-entry windows".into())
}

fn execute(machine: &mut Machine, control: Control, watch: &MemoryWatch, observed: &mut Observations) -> Result<(), String> {
    let sites = Stop::ALL.into_iter().filter(|stop| stop.enabled(control)).map(|stop| (stop.address(), stop.name().into())).collect();
    machine.traces.arm(&machine.session, sites)?;
    machine.traces.stop_labels = Stop::ALL.into_iter().filter(|stop| stop.enabled(control)).map(|stop| stop.name().into()).collect();
    let mut phase = Phase::default();
    let mut last_wait = WatchCursor(0);
    loop {
        let phase_end = if let Some(clear) = &observed.clear_readback { clear.instructions.0.saturating_add(RELEASE) }
            else if let Some(index) = phase.gate { observed.stops[index].hit.instructions.0.saturating_add(
                if control == Control::HeldSentinel { HELD } else { DELAYED }) } else { PREFIX };
        let end = BUDGET.min(phase_end);
        require(machine.instructions() < end, "control phase or aggregate instruction budget exhausted")?;
        let before = machine.traces.hits.len();
        machine.run(end - machine.instructions());
        let hit_count = machine.traces.hits.len() - before;
        let mut boundary_watch = None;
        for hit in &machine.traces.hits[before..] {
            let label = Stop::ALL.into_iter().find(|stop| hit_address(hit).ok() == Some(stop.address()))
                .ok_or("control trace contains an unexpected address")?;
            let snapshot = snapshot_at(machine, label, hit.core == 1 && label == Stop::Tail, hit)?;
            let watched = watch_state(watch)?;
            let cursor = WatchCursor(watched.records.len());
            observed.stops.push(Boundary { hit: snapshot, watch_records: cursor });
            boundary_watch = Some(watched);
        }
        machine.require_running()?;
        let boundary_watch = match boundary_watch { Some(state) => state, None => watch_state(watch)? };
        check_watch(&boundary_watch)?;
        require(hit_count == 1, "control boundary not reached exactly once within phase budget")?;
        let index = observed.stops.len() - 1;
        let boundary = &observed.stops[index];
        let hit = &boundary.hit;
        let label = hit.label;
        require(hit.core == label.core() && machine.traces.hits[before].site == label.name(), "control stop has wrong core/site")?;
        match label {
            Stop::Entry => {
                require(!phase.entry && !phase.tail && phase.waits == 0 && !phase.ack && !phase.clear, "control entry repeated or late")?;
                verify_image(machine, OriginalImage::Record2)?;
                phase.entry = true;
            }
            Stop::Tail => {
                require(phase.entry && !phase.tail && phase.waits == 0 && !phase.clear, "control rendezvous tail out of phase")?;
                tail(hit)?;
                phase.tail = true;
                last_wait = boundary.watch_records;
            }
            Stop::PrimaryGate => {
                require(phase.gate.is_none() && !phase.clear && hit.registers[2].0 == 0 && hit.registers[3].0 == 0
                    && hit.sp.0 == 0x700007f0 && hit.cpsr.0 & 0x010002ff == 0xd3
                    && hit.state.cell0.0 == 0x22b && matches!(hit.state.cell1.0, 0 | 0x22b)
                    && hit.state.completion.0 == 0 && hit.state.target.0 == 0 && hit.state.argument.0 == 0,
                    "control primary gate context differs")?;
                observed.bindings = Some(gate_bindings(machine, control)?);
                phase.gate = Some(index);
                machine.traces.remove_site(&machine.session, Guest(label.address()))?;
            }
            Stop::PeerWaitLoaded => {
                require(phase.tail && phase.waits < 4 && !phase.clear, "control equality stop out of phase")?;
                loaded(&boundary_watch, last_wait, boundary)?;
                last_wait = boundary.watch_records;
                phase.waits += 1;
                if phase.waits == 4 {
                    if control == Control::HeldSentinel {
                        let final_primary = primary(machine)?;
                        observed.final_primary = Some(final_primary);
                        let gate = &observed.stops[phase.gate.ok_or("held control lacks primary gate")?].hit;
                        let final_primary = observed.final_primary.as_ref().ok_or("held primary snapshot absent")?;
                        require(final_primary.address.0 == 0x1ff4 && same_context(final_primary, gate, false), "held primary context changed")?;
                        return acceptance(control, observed, &boundary_watch);
                    }
                    machine.traces.remove_site(&machine.session, Guest(label.address()))?;
                }
            }
            Stop::PeerAcknowledged => {
                require(control == Control::DelayedRelease && phase.gate.is_some() && phase.tail && !phase.ack && !phase.clear
                    && pending(&hit.state) && hit.registers[2].0 == 0, "control peer acknowledgement out of phase")?;
                phase.ack = true;
            }
            Stop::PrimaryClear => {
                require(control == Control::DelayedRelease && phase.ack && phase.waits == 4 && !phase.clear && pending(&hit.state),
                    "control original clear occurred before required wait observations")?;
                let gate = &observed.stops[phase.gate.ok_or("control primary gate absent")?].hit;
                require(same_context(&primary(machine)?, gate, true), "control veneer did not restore primary context")?;
                machine.traces.remove_site(&machine.session, Guest(label.address()))?;
                clear(machine, watch, boundary, &mut observed.clear_readback)?;
                phase.clear = true;
            }
            Stop::Handoff => {
                require(phase.clear && !phase.handoff, "control handoff before committed clear or repeated")?;
                let tail_stop = observed.stops.iter().find(|stop| stop.hit.label == Stop::Tail).ok_or("control tail absent")?;
                handoff(hit, &tail_stop.hit)?;
                verify_image(machine, OriginalImage::LoaderB)?;
                phase.handoff = true;
            }
            Stop::LoaderBEntry => {
                require(phase.handoff, "control loader entry before handoff")?;
                let tail_stop = observed.stops.iter().find(|stop| stop.hit.label == Stop::Tail).ok_or("control tail absent")?;
                handoff(hit, &tail_stop.hit)?;
                return acceptance(control, observed, &boundary_watch);
            }
            Stop::ControlFailure => return Err("guest delayed control entered its failure loop".into()),
        }
    }
}

fn acceptance(control: Control, observed: &Observations, watch: &WatchState) -> Result<(), String> {
    check_watch(watch)?;
    require(observed.bindings.is_some(), "control gate binding evidence absent")?;
    let entry = observed.stops.iter().find(|stop| stop.hit.label == Stop::Entry).ok_or("control peer entry absent")?;
    let tail = observed.stops.iter().find(|stop| stop.hit.label == Stop::Tail).ok_or("control peer tail absent")?;
    let first_wait = observed.stops.iter().find(|stop| stop.hit.label == Stop::PeerWaitLoaded).ok_or("control peer wait absent")?;
    let (s0, sentinel0) = writer(watch, WitnessCore::Core0, 0x1e64, 0x20000, 0x22b)?;
    let (s1, sentinel1) = writer(watch, WitnessCore::Core0, 0x1e68, 0x20004, 0x22b)?;
    let (peer_index, peer) = writer(watch, WitnessCore::Core1, 0x11f4c, 0x20004, 0)?;
    writer(watch, WitnessCore::Core0, 0x1fec, 0xa00c4, 0)?;
    writer(watch, WitnessCore::Core1, 0x11818, 0x14c80, 0)?;
    writer(watch, WitnessCore::Core1, 0x11818, 0x14c84, 0)?;
    require(s0 < s1 && sentinel0.instructions.0 < sentinel1.instructions.0
        && sentinel1.instructions.0 < entry.hit.instructions.0 && entry.hit.instructions.0 < tail.hit.instructions.0
        && peer_index >= tail.watch_records.0 && peer_index < first_wait.watch_records.0
        && peer.address.0 == 0x40020004 && peer.before.as_deref() == Some(&[0x2b, 2, 0, 0][..]),
        "control original sentinel/release/peer-clear attribution differs")?;
    if control == Control::HeldSentinel {
        require(!watch.records.iter().any(|event| matches!(event, MemoryEvent::Cpu(row)
            if row.core == WitnessCore::Core0 && row.pc.0 == 0x114)), "held control contains a delayed-veneer poll")?;
        require(!watch.records.iter().any(|event| matches!(event, MemoryEvent::Cpu(row)
            if matches!(row.source, CpuAccess::CpuWrite) && row.pc.0 == 0x1ff8)), "held control contains a primary clear attempt")?;
    } else {
        let clear = observed.clear_readback.as_ref().ok_or("control committed clear evidence absent")?;
        let ack = observed.stops.iter().find(|stop| stop.hit.label == Stop::PeerAcknowledged)
            .ok_or("control peer acknowledgement absent")?;
        let polls = watch.records.get(peer_index + 1..ack.watch_records.0).ok_or("control acknowledgement cursor interval differs")?;
        require(polls.iter().any(|event| matches!(event, MemoryEvent::Cpu(row)
            if cpu_matches(row, WitnessCore::Core0, 0x114, 0x20004, 0, CpuAccess::CpuRead))),
            "control acknowledgement lacks a completed zero poll after peer clear")?;
        let (_, primary) = writer(watch, WitnessCore::Core0, 0x1ff8, 0x20000, 0)?;
        let (target_index, target) = writer(watch, WitnessCore::Core1, 0x11f98, 0x14c80, 0xd0020)?;
        let (_, argument) = writer(watch, WitnessCore::Core1, 0x11fb0, 0x14c84, u64::from(ARGUMENT))?;
        require(primary.instructions.0 == clear.instructions.0 && target_index >= clear.watch_records.0
            && target.instructions.0 > clear.instructions.0 && argument.instructions.0 > target.instructions.0
            && target.before.as_deref() == Some(&[0; 4][..]) && argument.before.as_deref() == Some(&[0; 4][..]),
            "control publication precedes committed clear or has wrong preimage")?;
    }
    Ok(())
}

pub(super) fn run(mut machine: Machine, control: Control, output: &Path) -> Result<(), String> {
    let mut observed = Observations::default();
    let mut identities = [Identity(0); 2];
    let mut watch = None;
    let outcome = (|| {
        require(counts(&machine)?.0 == [0, 0] && !machine.traces.armed() && machine.traces.hits.is_empty(),
            "control witness must arm before either core executes")?;
        let (maps, configured) = bindings(&machine.document)?;
        identities = configured;
        require(machine.document.execution.quantum == 1000 && matches!(machine.document.execution.engine, RunEngine::Unicorn)
            && machine.document.execution.instructions_per_tick.numerator == 24
            && machine.document.execution.instructions_per_tick.denominator == 1, "control scheduling configuration differs")?;
        for (core, expected) in identities.iter().enumerate() {
            require(machine.session.board.cpu(core)?.rprofile_identity(None)? == expected.0, "control active CP14 identity differs")?;
        }
        watch = Some(machine.session.board.memory_watch_begin(WATCH_RANGES.to_vec(), WATCH_LIMIT, maps)?);
        execute(&mut machine, control, watch.as_deref().ok_or("control watch absent")?, &mut observed)
    })();
    let mut outcome = outcome;
    let memory_watch = match watch.as_deref() {
        Some(watch) => match watch_state(watch) {
            Ok(state) => WatchEvidence::Observed(state),
            Err(error) => {
                outcome = outcome.and(Err(error));
                WatchEvidence::Rejected(serde_json::value::to_raw_value(&watch.records.borrow().state())
                    .map_err(|error| error.to_string())?)
            }
        },
        None => WatchEvidence::Observed(WatchState { enabled: false, ranges: Vec::new(), records: Vec::new(), violation: None }),
    };
    if let Some(watch) = watch.as_deref_mut() {
        outcome = outcome.and(machine.session.board.memory_watch_end(watch));
    }
    outcome = outcome.and_then(|()| match &memory_watch {
        WatchEvidence::Observed(state) => check_watch(state),
        WatchEvidence::Rejected(_) => Err("control memory watch protocol rejected".into()),
    });
    let receipt = Receipt {
        schema: 2, control, result: if outcome.is_ok() { WitnessResult::Passed } else { WitnessResult::Failed },
        error: outcome.as_ref().err().map(String::as_str), machine_failure: machine.failure.as_deref(),
        instructions: Instructions(machine.instructions()), cp14_identities: identities,
        stops: &observed.stops, bindings: &observed.bindings, final_primary: &observed.final_primary,
        clear_readback: &observed.clear_readback, memory_watch,
    };
    let bytes = serde_json::to_vec_pretty(&receipt).map_err(|error| error.to_string())?;
    require(bytes.len() <= MAX_RECEIPT, "control receipt exceeds 16 MiB")?;
    let path = output.join("peer-witness.json");
    std::fs::write(&path, bytes).map_err(|error| format!("{}: {error}", path.display()))?;
    println!("{}", path.display());
    outcome
}
