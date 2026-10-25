//! Checks the run document's address provenance against the decoded X-M1
//! 1.50 images: every word or code binding must be formed where it says, and
//! no listed image may form an address the document marks unchecked.
use fujimod_xm1::provenance::{self, Guest, Provenance, RamFact, Target, Window};
use fujimod_xm1::sd_host;
use serde::Deserialize;
use serde_json::value::RawValue;
use std::{path::PathBuf, process::ExitCode};

const USAGE: &str = "Usage: fujimod-xm1-provenance --run RUN.json --analysis private-data/analysis/x-m1-1.50\n";

#[derive(Deserialize)]
struct KeyTable {
    base: u32,
}

#[derive(Deserialize)]
struct Firmware {
    current_menu: u32,
    calibration_pointer: u32,
    scanner_registered: u32,
    key_table: KeyTable,
}

#[derive(Deserialize)]
struct Document {
    devices: Box<RawValue>,
    sd: String,
    firmware: Firmware,
    provenance: Provenance,
}

fn drive() -> Result<bool, String> {
    let mut run = None;
    let mut analysis = None;
    let mut args = std::env::args_os().skip(1);
    while let Some(flag) = args.next() {
        let value = args.next().map(PathBuf::from).ok_or(USAGE)?;
        match flag.to_str() {
            Some("--run") => run = Some(value),
            Some("--analysis") => analysis = Some(value),
            _ => return Err(USAGE.into()),
        }
    }
    let (Some(run), Some(analysis)) = (run, analysis) else { return Err(USAGE.into()) };
    let analysis = match std::env::var_os("BUILD_WORKSPACE_DIRECTORY") {
        Some(workspace) if analysis.is_relative() => PathBuf::from(workspace).join(analysis),
        _ => analysis,
    };
    let bytes = std::fs::read(&run).map_err(|e| format!("{}: {e}", run.display()))?;
    let document: Document = serde_json::from_slice(&bytes).map_err(|e| format!("run document: {e}"))?;
    let windows = provenance::plan_windows(document.devices.get())?;
    let firmware = &document.firmware;
    let ram = [
        (RamFact::CurrentMenu, Guest(firmware.current_menu)),
        (RamFact::CalibrationPointer, Guest(firmware.calibration_pointer)),
        (RamFact::ScannerRegistered, Guest(firmware.scanner_registered)),
        (RamFact::KeyTable, Guest(firmware.key_table.base)),
    ];
    let registers: Vec<(&str, Guest)> = sd_host::REGISTERS.iter().map(|address| (document.sd.as_str(), Guest(*address))).collect();
    document.provenance.validate(&windows, &ram, &registers)?;
    let in_window = |value: Guest| windows.iter().any(|(_, window): &(String, Window)| value.0.wrapping_sub(window.base.0) < window.size);
    let wanted = |value: Guest| in_window(value) || ram.iter().any(|(_, address)| *address == value);
    let references = provenance::image_references(&document.provenance, &analysis, &wanted)?;
    let mut passed = true;
    let mut report = |target: Target, binding| match provenance::check(&target, binding, &references) {
        Ok(line) => println!("ok   {line}"),
        Err(line) => {
            passed = false;
            println!("FAIL {line}");
        }
    };
    for entry in &document.provenance.windows {
        report(Target::Window { node: &entry.node, window: Window { base: entry.base, size: entry.size } }, &entry.binding);
    }
    for entry in &document.provenance.ram {
        let address = ram.iter().find(|(fact, _)| *fact == entry.fact).map(|(_, address)| *address).ok_or("RAM fact")?;
        report(Target::Ram { fact: entry.fact, address }, &entry.binding);
    }
    for entry in &document.provenance.registers {
        report(Target::Register { node: &entry.node, address: entry.address }, &entry.binding);
    }
    Ok(passed)
}

fn main() -> ExitCode {
    match drive() {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(error) => {
            eprintln!("fujimod-xm1-provenance: {error}");
            ExitCode::FAILURE
        }
    }
}
