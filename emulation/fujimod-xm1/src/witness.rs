//! Natural-ROM first-stage witness. It arms trace points before the first
//! instruction and records entry/helper/handoff registers and staged memory.
use crate::machine::Machine;
use crate::provenance::Guest;
use crate::trace::Hit;
use serde::{Deserialize, Serialize, Serializer};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

const MAX_STOPS: usize = 32;
const MAX_READS: usize = 64;
const MAX_READ: usize = 8 << 20;
const MAX_TOTAL_READ: usize = 32 << 20;
const MAX_BUDGET: u64 = 50_000_000;

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(transparent)]
pub struct Budget(u64);

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(transparent)]
pub struct ReadLength(u32);

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum StopLabel {
    #[serde(rename = "owned-first-stage-entry")]
    OwnedFirstStageEntry,
    #[serde(rename = "retained-storage-init")]
    RetainedStorageInit,
    #[serde(rename = "retained-trailer-gate")]
    RetainedTrailerGate,
    #[serde(rename = "retained-stage-images")]
    RetainedStageImages,
    #[serde(rename = "owned-handoff-vector")]
    OwnedHandoffVector,
}

impl StopLabel {
    fn as_str(self) -> &'static str {
        match self {
            StopLabel::OwnedFirstStageEntry => "owned-first-stage-entry",
            StopLabel::RetainedStorageInit => "retained-storage-init",
            StopLabel::RetainedTrailerGate => "retained-trailer-gate",
            StopLabel::RetainedStageImages => "retained-stage-images",
            StopLabel::OwnedHandoffVector => "owned-handoff-vector",
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum ReadLabel {
    #[serde(rename = "owned-image")]
    OwnedImage,
    #[serde(rename = "handoff-words")]
    HandoffWords,
    #[serde(rename = "skip-list")]
    SkipList,
    #[serde(rename = "loader-a")]
    LoaderA,
    #[serde(rename = "loader-b")]
    LoaderB,
    #[serde(rename = "segment-table")]
    SegmentTable,
    #[serde(rename = "image-4")]
    Image4,
    #[serde(rename = "image-4-bss")]
    Image4Bss,
    #[serde(rename = "image-4-arg")]
    Image4Arg,
    #[serde(rename = "image-4-publication")]
    Image4Publication,
    #[serde(rename = "peer-cells")]
    PeerCells,
    #[serde(rename = "retained-event-word")]
    RetainedEventWord,
}

#[derive(Clone, Copy, Debug, Serialize)]
pub struct Registers {
    r0: u32,
    r1: u32,
    r2: u32,
    r3: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WitnessCore {
    Core0,
    Core1,
}

impl WitnessCore {
    pub(crate) fn from_index(index: usize) -> Result<Self, String> {
        match index {
            0 => Ok(WitnessCore::Core0),
            1 => Ok(WitnessCore::Core1),
            _ => Err("witness stop is not on core 0 or 1".into()),
        }
    }
}

impl Serialize for WitnessCore {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_u8(match self {
            WitnessCore::Core0 => 0,
            WitnessCore::Core1 => 1,
        })
    }
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(transparent)]
pub struct Cpsr(u32);

#[derive(Clone, Debug, Serialize)]
#[serde(transparent)]
pub struct Sha256Hex(String);

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum WitnessResult {
    Passed,
    Failed,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WitnessContract {
    schema: u32,
    budget: Budget,
    stops: Vec<Stop>,
    reads: Vec<Read>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Stop {
    label: StopLabel,
    address: Guest,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Read {
    label: ReadLabel,
    address: Guest,
    length: ReadLength,
}

#[derive(Serialize)]
pub struct WitnessRead {
    label: ReadLabel,
    address: Guest,
    length: ReadLength,
    sha256: Sha256Hex,
    hex: Option<String>,
}

#[derive(Serialize)]
pub struct WitnessStop {
    label: StopLabel,
    address: Guest,
    core: WitnessCore,
    instructions: u64,
    registers: Registers,
    sp: u32,
    lr: u32,
    cpsr: Cpsr,
}

#[derive(Serialize)]
pub struct WitnessReceipt<'a> {
    schema: u32,
    result: WitnessResult,
    error: Option<&'a str>,
    instructions: u64,
    machine_failure: Option<&'a str>,
    stops: Vec<WitnessStop>,
    reads: &'a [WitnessRead],
}

impl WitnessContract {
    pub fn parse(bytes: &[u8]) -> Result<Self, String> {
        let contract: Self = serde_json::from_slice(bytes).map_err(|error| format!("witness contract: {error}"))?;
        if contract.schema != 1 {
            return Err("witness contract schema must be 1".into());
        }
        let budget = contract.budget.0;
        if budget == 0 || budget > MAX_BUDGET {
            return Err(format!("witness budget must be 1..{MAX_BUDGET}"));
        }
        if contract.stops.is_empty() || contract.stops.len() > MAX_STOPS {
            return Err(format!("witness stops must be 1..{MAX_STOPS}"));
        }
        let mut labels = BTreeSet::new();
        let mut addresses = BTreeSet::new();
        for stop in &contract.stops {
            if !labels.insert(stop.label) {
                return Err("witness stop labels must be unique".into());
            }
            if stop.address.0 % 4 != 0 || !addresses.insert(stop.address) {
                return Err("witness stop addresses must be unique and word aligned".into());
            }
        }
        if contract.reads.len() > MAX_READS {
            return Err(format!("witness reads must be at most {MAX_READS}"));
        }
        let mut read_labels = BTreeSet::new();
        let mut total = 0usize;
        for read in &contract.reads {
            if !read_labels.insert(read.label) {
                return Err("witness read labels must be unique".into());
            }
            let length = read.length.0 as usize;
            if length == 0 || length > MAX_READ {
                return Err(format!("witness read {:?} has an invalid length", read.label));
            }
            total = total.checked_add(length).ok_or("witness read total overflow")?;
            if total > MAX_TOTAL_READ {
                return Err(format!("witness reads exceed {MAX_TOTAL_READ} bytes"));
            }
        }
        Ok(contract)
    }
}

pub fn run(mut machine: Machine, contract: WitnessContract, output: &Path) -> Result<(), String> {
    let outcome = execute(&mut machine, &contract);
    let (result, error, reads): (WitnessResult, Option<&str>, &[WitnessRead]) = match &outcome {
        Ok(reads) => (WitnessResult::Passed, None, reads.as_slice()),
        Err(error) => (WitnessResult::Failed, Some(error.as_str()), &[]),
    };
    let labels: BTreeMap<Guest, StopLabel> = contract.stops.iter().map(|stop| (stop.address, stop.label)).collect();
    let mut stops = Vec::new();
    for hit in &machine.traces.hits {
        if let Some(label) = labels.get(&Guest(hit_address(hit)?)) {
            stops.push(stop_from_hit(*label, hit)?);
        }
    }
    let receipt = WitnessReceipt {
        schema: 1,
        result,
        error,
        instructions: machine.instructions(),
        machine_failure: machine.failure.as_deref(),
        stops,
        reads,
    };
    let path = output.join("first-stage-witness.json");
    std::fs::write(&path, serde_json::to_vec_pretty(&receipt).map_err(|error| error.to_string())?)
        .map_err(|error| format!("{}: {error}", path.display()))?;
    println!("{}", path.display());
    drop(receipt);
    outcome.map(|_| ())
}

fn execute(machine: &mut Machine, contract: &WitnessContract) -> Result<Vec<WitnessRead>, String> {
    let sites: BTreeMap<u32, String> =
        contract.stops.iter().map(|stop| (stop.address.0, stop.label.as_str().to_owned())).collect();
    machine.traces.arm_primary(&machine.session, sites)?;
    let expected: BTreeSet<StopLabel> = contract.stops.iter().map(|stop| stop.label).collect();
    let mut observed = BTreeSet::new();
    let start = machine.instructions();
    while observed.len() < expected.len() {
        let spent = machine.instructions().saturating_sub(start);
        if spent >= contract.budget.0 {
            return Err(format!("witness budget {spent} exhausted before all stops"));
        }
        let before = machine.traces.hits.len();
        machine.traces.stop_labels = expected.difference(&observed).map(|label| label.as_str().to_owned()).collect();
        machine.run(contract.budget.0 - spent);
        if let Some(failure) = &machine.failure {
            return Err(format!("machine stopped: {failure}"));
        }
        let new_hits = &machine.traces.hits[before..];
        if new_hits.is_empty() {
            return Err("witness stop not reached within budget".into());
        }
        let before_count = observed.len();
        for hit in new_hits {
            let address = Guest(hit_address(hit)?);
            if let Some(stop) = contract.stops.iter().find(|stop| stop.address == address) {
                observed.insert(stop.label);
            }
        }
        if observed.len() == before_count {
            return Err("witness stop sequence did not advance".into());
        }
    }
    let mut reads = Vec::new();
    for read in &contract.reads {
        let length = read.length.0 as usize;
        let bytes = machine.read(u64::from(read.address.0), length)?;
        reads.push(WitnessRead {
            label: read.label,
            address: read.address,
            length: read.length,
            sha256: Sha256Hex(encode_hex(Sha256::digest(&bytes).as_ref())),
            hex: if length <= 0x1000 { Some(encode_hex(&bytes)) } else { None },
        });
    }
    Ok(reads)
}

pub(crate) fn hit_address(hit: &Hit) -> Result<u32, String> {
    parse_hex(&hit.address)
}

pub(crate) fn parse_hex(text: &str) -> Result<u32, String> {
    u32::from_str_radix(text.strip_prefix("0x").ok_or_else(|| format!("address is not hex: {text}"))?, 16)
        .map_err(|_| format!("address is not a 32-bit hex value: {text}"))
}

fn stop_from_hit(label: StopLabel, hit: &Hit) -> Result<WitnessStop, String> {
    if hit.site.as_str() != label.as_str() {
        return Err("trace hit label differs from the armed stop".into());
    }
    let register = |name: &str| -> Result<u32, String> {
        let value = hit.registers.get(name).ok_or("trace hit lacks a register")?;
        parse_hex(value)
    };
    Ok(WitnessStop {
        label,
        address: Guest(hit_address(hit)?),
        core: WitnessCore::from_index(hit.core)?,
        instructions: hit.instructions,
        registers: Registers {
            r0: register("r0")?,
            r1: register("r1")?,
            r2: register("r2")?,
            r3: register("r3")?,
        },
        sp: parse_hex(&hit.sp)?,
        lr: parse_hex(&hit.lr)?,
        cpsr: Cpsr(hit.cpsr),
    })
}

fn encode_hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let byte = *byte;
        text.push(DIGITS[usize::from(byte >> 4)] as char);
        text.push(DIGITS[usize::from(byte & 0x0f)] as char);
    }
    text
}
