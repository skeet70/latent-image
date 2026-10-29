//! Natural-ROM first-stage witness. It arms trace points before the first
//! instruction and records entry/helper/handoff registers and staged memory.
use crate::machine::Machine;
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

const MAX_STOPS: usize = 32;
const MAX_READS: usize = 64;
const MAX_READ: usize = 8 << 20;
const MAX_TOTAL_READ: usize = 32 << 20;
const MAX_BUDGET: u64 = 50_000_000;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WitnessContract {
    schema: u32,
    budget: u64,
    stops: Vec<Stop>,
    reads: Vec<Read>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Stop {
    label: String,
    address: u32,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Read {
    label: String,
    address: u64,
    length: usize,
}

impl WitnessContract {
    pub fn parse(bytes: &[u8]) -> Result<Self, String> {
        let contract: Self = serde_json::from_slice(bytes).map_err(|error| format!("witness contract: {error}"))?;
        if contract.schema != 1 {
            return Err("witness contract schema must be 1".into());
        }
        if contract.budget == 0 || contract.budget > MAX_BUDGET {
            return Err(format!("witness budget must be 1..{MAX_BUDGET}"));
        }
        if contract.stops.is_empty() || contract.stops.len() > MAX_STOPS {
            return Err(format!("witness stops must be 1..{MAX_STOPS}"));
        }
        let mut labels = BTreeSet::new();
        let mut addresses = BTreeSet::new();
        for stop in &contract.stops {
            if stop.label.is_empty() || !labels.insert(stop.label.clone()) {
                return Err("witness stop labels must be nonempty and unique".into());
            }
            if stop.address % 4 != 0 || !addresses.insert(stop.address) {
                return Err(format!("witness stop {:?} is not a unique word address", stop.label));
            }
        }
        if contract.reads.len() > MAX_READS {
            return Err(format!("witness reads must be at most {MAX_READS}"));
        }
        let mut read_labels = BTreeSet::new();
        let mut total = 0usize;
        for read in &contract.reads {
            if read.label.is_empty() || !read_labels.insert(read.label.clone()) {
                return Err("witness read labels must be nonempty and unique".into());
            }
            if read.length == 0 || read.length > MAX_READ {
                return Err(format!("witness read {:?} has an invalid length", read.label));
            }
            total = total
                .checked_add(read.length)
                .ok_or("witness read total overflow")?;
            if total > MAX_TOTAL_READ {
                return Err(format!("witness reads exceed {MAX_TOTAL_READ} bytes"));
            }
        }
        Ok(contract)
    }
}

pub fn run(mut machine: Machine, contract: WitnessContract, output: &Path) -> Result<(), String> {
    let outcome = execute(&mut machine, &contract);
    let labels: BTreeSet<&str> = contract.stops.iter().map(|stop| stop.label.as_str()).collect();
    let stops: Vec<_> = machine.traces.hits.iter().filter(|hit| labels.contains(hit.site.as_str())).collect();
    let (result, error, reads) = match &outcome {
        Ok(reads) => ("passed", Value::Null, reads.clone()),
        Err(error) => ("failed", json!(error), Vec::new()),
    };
    let receipt = json!({
        "schema": 1,
        "result": result,
        "error": error,
        "instructions": machine.instructions(),
        "machine_failure": machine.failure.clone(),
        "stops": stops,
        "reads": reads,
    });
    let path = output.join("first-stage-witness.json");
    std::fs::write(&path, serde_json::to_vec_pretty(&receipt).map_err(|error| error.to_string())?)
        .map_err(|error| format!("{}: {error}", path.display()))?;
    println!("{}", path.display());
    outcome.map(|_| ())
}

fn execute(machine: &mut Machine, contract: &WitnessContract) -> Result<Vec<Value>, String> {
    let sites: BTreeMap<u32, String> =
        contract.stops.iter().map(|stop| (stop.address, stop.label.clone())).collect();
    machine.traces.arm(&machine.session, sites)?;
    let expected: BTreeSet<String> = contract.stops.iter().map(|stop| stop.label.clone()).collect();
    let mut observed = BTreeSet::new();
    let start = machine.instructions();
    while observed.len() < expected.len() {
        let spent = machine.instructions().saturating_sub(start);
        if spent >= contract.budget {
            return Err(format!("witness budget {spent} exhausted before all stops"));
        }
        let before = machine.traces.hits.len();
        machine.traces.stop_labels = expected.difference(&observed).cloned().collect();
        machine.run(contract.budget - spent);
        if let Some(failure) = &machine.failure {
            return Err(format!("machine stopped: {failure}"));
        }
        let new_hits = &machine.traces.hits[before..];
        if new_hits.is_empty() {
            return Err("witness stop not reached within budget".into());
        }
        let before_count = observed.len();
        for hit in new_hits {
            if expected.contains(&hit.site) {
                observed.insert(hit.site.clone());
            }
        }
        if observed.len() == before_count {
            return Err("witness stop sequence did not advance".into());
        }
    }
    let mut reads = Vec::new();
    for read in &contract.reads {
        let bytes = machine.read(read.address, read.length)?;
        let digest = Sha256::digest(&bytes);
        let mut row = json!({
            "label": read.label,
            "address": format!("{:#x}", read.address),
            "length": read.length,
            "sha256": hex(digest.as_ref()),
        });
        if read.length <= 0x1000 {
            row["hex"] = json!(hex(&bytes));
        }
        reads.push(row);
    }
    Ok(reads)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
