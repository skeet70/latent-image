//! Core-qualified record-2 observation on the natural-reset route.
use crate::machine::Machine;
use crate::provenance::Guest;
use crate::run::{self, RunDocument};
use crate::trace::Hit;
use crate::witness::{WitnessCore, WitnessResult, hit_address};
use fujimod_emulator::board::{factory, observation::MemoryWatch};
use fujimod_emulator_cpu::{Model as CpuModel, Register, arm};
use fujimod_emulator_exr2_plan::{Model, Plan};
use serde::{Deserialize, Deserializer, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

const MAX_BUDGET: u64 = 50_000_000;
const WATCH_LIMIT: usize = 16_384;
const MAX_RECEIPT: usize = 16 << 20;
const WATCH_RANGES: [(u64, u64); 3] = [(0x14c80, 0x14c88), (0x20000, 0x20008), (0xa00c4, 0xa00c8)];

#[path = "peer_witness_wait.rs"]
mod wait;

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(transparent)]
struct Instructions(u64);

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(transparent)]
struct Identity(u32);

#[derive(Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(transparent)]
struct Word(u32);

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(transparent)]
struct Address(u64);

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NaturalContract {
    schema: u32,
    budget: Instructions,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum ContractWire {
    Natural(NaturalContract),
    Control(wait::ControlContract),
}

pub struct PeerContract {
    budget: Instructions,
    control: Option<wait::Control>,
}

impl PeerContract {
    pub fn parse(bytes: &[u8]) -> Result<Self, String> {
        let contract: ContractWire = serde_json::from_slice(bytes).map_err(|error| format!("peer contract: {error}"))?;
        match contract {
            ContractWire::Natural(contract) => {
                if contract.schema != 1 || !(1..=MAX_BUDGET).contains(&contract.budget.0) {
                    return Err("peer contract requires schema 1 and budget 1..50000000".into());
                }
                Ok(Self { budget: contract.budget, control: None })
            }
            ContractWire::Control(contract) => {
                contract.validate()?;
                Ok(Self { budget: contract.budget, control: Some(contract.control) })
            }
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum PeerStop {
    #[serde(rename = "record-2-entry")]
    Record2Entry,
    #[serde(rename = "record-2-rendezvous-tail")]
    Record2RendezvousTail,
    #[serde(rename = "record-2-handoff")]
    Record2Handoff,
    LoaderBEntry,
}

impl PeerStop {
    const ALL: [Self; 4] = [Self::Record2Entry, Self::Record2RendezvousTail, Self::Record2Handoff, Self::LoaderBEntry];

    fn address(self) -> u32 {
        match self {
            Self::Record2Entry => 0x11800,
            Self::Record2RendezvousTail => 0x11f40,
            Self::Record2Handoff => 0x11850,
            Self::LoaderBEntry => 0xd0020,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Record2Entry => "record-2-entry",
            Self::Record2RendezvousTail => "record-2-rendezvous-tail",
            Self::Record2Handoff => "record-2-handoff",
            Self::LoaderBEntry => "loader-b-entry",
        }
    }
}

#[derive(Serialize)]
struct PeerState {
    cell0: Word,
    cell1: Word,
    completion: Word,
    target: Word,
    argument: Word,
}

#[derive(Serialize)]
struct PeerHit<L = PeerStop> {
    label: L,
    address: Guest,
    core: WitnessCore,
    instructions: Instructions,
    registers: [Word; 13],
    sp: Word,
    lr: Word,
    cpsr: Word,
    state: PeerState,
    frame: Option<[u8; 24]>,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
enum CpuAccess {
    CpuWrite,
    CpuRead,
    CpuFetch,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
enum ByteOrder {
    Little,
    Big,
}

fn core<'de, D: Deserializer<'de>>(deserializer: D) -> Result<WitnessCore, D::Error> {
    WitnessCore::from_index(usize::deserialize(deserializer)?).map_err(serde::de::Error::custom)
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct CpuAttempt {
    source: CpuAccess,
    address: Address,
    physical: Address,
    size: u32,
    value: Option<u64>,
    before: Option<Vec<u8>>,
    byte_order: ByteOrder,
    #[serde(deserialize_with = "core")]
    core: WitnessCore,
    pc: Guest,
    instructions: Instructions,
    tick: u64,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
enum TransferAccess {
    HostRead,
    HostWrite,
    ModelBusRead,
    ModelBusWrite,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ObservedTransfer {
    source: TransferAccess,
    address: Address,
    physical: Address,
    size: u64,
    core: (),
    pc: (),
    instructions: (),
    tick: (),
}

#[derive(Deserialize, Serialize)]
#[serde(untagged)]
enum MemoryEvent {
    Cpu(CpuAttempt),
    Transfer(ObservedTransfer),
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct WatchState {
    enabled: bool,
    ranges: Vec<(Address, Address)>,
    records: Vec<MemoryEvent>,
    violation: Option<String>,
}

#[derive(Serialize)]
struct PeerReceipt<'a> {
    schema: u32,
    result: WitnessResult,
    error: Option<&'a str>,
    machine_failure: Option<&'a str>,
    instructions: Instructions,
    cp14_identities: [Identity; 2],
    stops: &'a [PeerHit],
    memory_watch: WatchState,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReleaseOptions {
    released: bool,
}

fn mapping(maps: &[factory::MemoryMap], core: usize, address: u64, physical: u64, length: u64) -> bool {
    maps.iter().any(|map| {
        map.core == core && map.address <= address && address + length <= map.address + map.size as u64
            && map.physical + address - map.address == physical && map.permissions & 7 == 7
    })
}

fn bindings(document: &RunDocument) -> Result<(Vec<factory::MemoryMap>, [Identity; 2]), String> {
    let (_, graph) = run::graph(document)?;
    if graph.processors.len() != 2 {
        return Err("peer witness requires exactly two scheduled contexts".into());
    }
    let mut identities = [Identity(0); 2];
    for (core, processor) in graph.processors.iter().enumerate() {
        let expected_pc = if core == 0 { 0 } else { 0x3fff0000 };
        let expected_identity = if core == 0 { 0x4003 } else { 0x5003 };
        let profile = processor.rprofile.ok_or("peer context lacks R-profile options")?;
        if processor.model != CpuModel::CortexR5F || processor.released != (core == 0)
            || profile.cp14_identity != expected_identity
            || !processor.registers.iter().any(|&(register, value)| register == Register::Pc && value == expected_pc)
        {
            return Err("peer context reset/CP14 bindings differ from the source contract".into());
        }
        identities[core] = Identity(profile.cp14_identity);
        for &(lo, hi) in &WATCH_RANGES {
            for address in [lo, 0x40000000 + lo] {
                if !mapping(&graph.mappings, core, address, lo, hi - lo) {
                    return Err(format!("peer handshake mapping differs on core {core} at {address:#x}"));
                }
            }
        }
        for stop in PeerStop::ALL {
            let address = u64::from(stop.address());
            if !mapping(&graph.mappings, core, address, address, 4) {
                return Err("peer instruction binding lacks its identity RAM mapping".into());
            }
        }
    }
    for (core, address) in [(0, 0x7fff0000), (1, 0x3fff0000)] {
        if !mapping(&graph.mappings, core, address, 0x3fff0000, 0x3e8) {
            return Err("peer staged vector alias differs from the source contract".into());
        }
    }
    let plan = Plan::parse(document.devices.get().as_bytes())?;
    let mut releases = plan.nodes.iter().filter_map(|node| match &node.model {
        Model::CoreRelease { options, core, request } => Some((options, core, request)),
        _ => None,
    });
    let (options, release_core, request) = releases.next().ok_or("peer witness requires a declared release device")?;
    if releases.next().is_some() {
        return Err("peer witness requires one declared release device".into());
    }
    let options: ReleaseOptions = serde_json::from_value(options.clone()).map_err(|error| error.to_string())?;
    if *release_core != 1 || options.released || request.as_str() != "release" {
        return Err("peer release binding differs from the source contract".into());
    }
    let mut controllers = plan.nodes.iter().filter_map(|node| match &node.model {
        Model::InterruptController { core: 1, base: 0xfffb4000, software, .. } => Some(software),
        _ => None,
    });
    let software = controllers.next().ok_or("peer software-interrupt controller is absent")?;
    if controllers.next().is_some() || software.len() != 1 || software[0].line != 120 || software[0].pulse != "release" {
        return Err("peer software-interrupt release route differs from the source contract".into());
    }
    if !matches!(document.execution_policy, run::Policy::Exact) {
        return Err("peer observation requires exact instruction boundaries".into());
    }
    Ok((graph.mappings, identities))
}

fn register(machine: &Machine, core: usize, register: arm::Register) -> Result<Word, String> {
    u32::try_from(machine.session.board.read_register(core, Register::Arm32(register))?)
        .map(Word).map_err(|_| "peer register exceeds 32 bits".into())
}

fn word(machine: &Machine, physical: u64) -> Result<Word, String> {
    let mut bytes = [0; 4];
    machine.session.board.read_physical(physical, &mut bytes)?;
    Ok(Word(u32::from_le_bytes(bytes)))
}

fn snapshot(machine: &Machine, label: PeerStop, hit: &Hit) -> Result<PeerHit, String> {
    snapshot_at(machine, label, hit.core == 1 && label == PeerStop::Record2RendezvousTail, hit)
}

fn state(machine: &Machine) -> Result<PeerState, String> {
    Ok(PeerState {
        cell0: word(machine, 0x20000)?, cell1: word(machine, 0x20004)?,
        completion: word(machine, 0xa00c4)?, target: word(machine, 0x14c80)?, argument: word(machine, 0x14c84)?,
    })
}

fn snapshot_at<L>(machine: &Machine, label: L, saved_frame: bool, hit: &Hit) -> Result<PeerHit<L>, String> {
    let mut registers = [Word(0); 13];
    for (index, value) in registers.iter_mut().enumerate() {
        *value = register(machine, hit.core, arm::Register::R(index as u8))?;
    }
    let frame = if saved_frame {
        let mut bytes = [0; 24];
        let address = registers[11].0.checked_sub(20).ok_or("peer saved frame address underflow")?;
        machine.session.board.read_memory(hit.core, u64::from(address), &mut bytes)?;
        Some(bytes)
    } else {
        None
    };
    Ok(PeerHit {
        label,
        address: Guest(hit_address(hit)?),
        core: WitnessCore::from_index(hit.core)?,
        instructions: Instructions(hit.instructions),
        registers,
        sp: register(machine, hit.core, arm::Register::R(13))?,
        lr: register(machine, hit.core, arm::Register::R(14))?,
        cpsr: Word(hit.cpsr),
        state: state(machine)?,
        frame,
    })
}

fn watch_state(watch: &MemoryWatch) -> Result<WatchState, String> {
    serde_json::from_value(watch.records.borrow().state()).map_err(|error| format!("peer memory watch: {error}"))
}

fn execute(machine: &mut Machine, contract: &PeerContract, stops: &mut Vec<PeerHit>) -> Result<(), String> {
    let sites: BTreeMap<u32, String> = PeerStop::ALL.into_iter().map(|stop| (stop.address(), stop.name().into())).collect();
    machine.traces.arm(&machine.session, sites)?;
    machine.traces.stop_labels = PeerStop::ALL.into_iter().map(|stop| stop.name().into()).collect();
    for expected in PeerStop::ALL {
        let before = machine.traces.hits.len();
        let remaining = contract.budget.0.saturating_sub(machine.instructions());
        if remaining == 0 {
            return Err("peer instruction budget exhausted".into());
        }
        machine.run(remaining);
        for hit in &machine.traces.hits[before..] {
            let label = PeerStop::ALL.into_iter().find(|stop| hit_address(hit).ok() == Some(stop.address()))
                .ok_or("peer trace contains an unexpected address")?;
            stops.push(snapshot(machine, label, hit)?);
            if hit.core != 1 || label != expected || hit.site != label.name() {
                return Err("peer stop has the wrong core or handoff sequence".into());
            }
        }
        if let Some(failure) = &machine.failure {
            return Err(format!("peer machine stopped: {failure}"));
        }
        if machine.traces.hits.len() != before + 1 {
            return Err("peer boundary not reached exactly once within budget".into());
        }
    }
    Ok(())
}

pub fn run(mut machine: Machine, contract: PeerContract, output: &Path) -> Result<(), String> {
    if let Some(control) = contract.control {
        return wait::run(machine, control, output);
    }
    if machine.instructions() != 0 || machine.traces.armed() || !machine.traces.hits.is_empty() {
        return Err("peer witness must arm before either core executes".into());
    }
    let (maps, identities) = bindings(&machine.document)?;
    let mut watch = machine.session.board.memory_watch_begin(WATCH_RANGES.to_vec(), WATCH_LIMIT, maps)?;
    let mut stops = Vec::with_capacity(PeerStop::ALL.len());
    let outcome = execute(&mut machine, &contract, &mut stops);
    let memory_watch = watch_state(&watch)?;
    machine.session.board.memory_watch_end(&mut watch)?;
    let outcome = outcome.and_then(|()| match &memory_watch.violation {
        Some(violation) => Err(violation.clone()),
        None => Ok(()),
    });
    let receipt = PeerReceipt {
        schema: 1,
        result: if outcome.is_ok() { WitnessResult::Passed } else { WitnessResult::Failed },
        error: outcome.as_ref().err().map(String::as_str),
        machine_failure: machine.failure.as_deref(),
        instructions: Instructions(machine.instructions()),
        cp14_identities: identities,
        stops: &stops,
        memory_watch,
    };
    let bytes = serde_json::to_vec_pretty(&receipt).map_err(|error| error.to_string())?;
    if bytes.len() > MAX_RECEIPT {
        return Err("peer receipt exceeds 16 MiB".into());
    }
    let path = output.join("peer-witness.json");
    std::fs::write(&path, bytes).map_err(|error| format!("{}: {error}", path.display()))?;
    println!("{}", path.display());
    outcome
}
