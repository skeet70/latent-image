//! The USB-toggle primitive server of `emulation/UsbToggleEmu.hs` on FujiMod's
//! EXR II runtime: the JSON-lines commands of the retained
//! `continue_rec_scanout.py --toggle-server`, with route checkpoints saved as
//! FujiMod session snapshots.
use fujimod_emulator::board::observation::MemoryWatch;
use fujimod_xm1::adjustment::AdjustmentNand;
use fujimod_xm1::bridge::{self, Inputs, SNAPSHOT, emit, menu_value, read, sha256, write};
use fujimod_xm1::ff80::{Ff80, Transfer, encode_hex};
use fujimod_xm1::guest::{RouteKey, set_adc};
use fujimod_xm1::machine::KeyPulse;
use fujimod_xm1::run::{self, RunDocument};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::BufRead,
    path::PathBuf,
    process::ExitCode,
    time::Instant,
};

const USAGE: &str = "Usage: fujimod-xm1-toggle --run RUN.json --boot-rom ROM --installed-dat DAT \
--adjustment-nand NAND --output DIR --toggle-checkpoints DIR --checkpoint-inputs JSON \
[--cable-at-reset] [--adc-at-reset CHANNEL=SAMPLE] [--access-probe-range LOW:HIGH]\n";

const RECORD: &str = "usb-toggle-boot.json";
const CHECKPOINT_SCHEMA: u32 = 1;
const MAX_RUN: u64 = 4_000_000_000;
const MAX_ENUMERATE: u64 = 1_000_000_000;
const MAX_PHASE: u64 = 100_000_000;
const MAX_READ: usize = 4096;
const ADC_CHANNELS: usize = 12;
const PROBE_LIMIT: usize = 100_000;
const DIRECTIONAL_PULSE: KeyPulse = KeyPulse { hold: 3_000_000, release: 12_000_000 };
const MENU_PULSE: KeyPulse = KeyPulse { hold: 4_000_000, release: 6_000_000 };

struct Arguments {
    run: PathBuf,
    boot_rom: PathBuf,
    installed_dat: PathBuf,
    adjustment_nand: PathBuf,
    output: PathBuf,
    checkpoints: PathBuf,
    route: Value,
    cable_at_reset: bool,
    adc_at_reset: Option<(usize, u16)>,
    probe_range: Option<(u64, u64)>,
}

fn number(text: &str) -> Option<u64> {
    match text.strip_prefix("0x") {
        Some(digits) => u64::from_str_radix(digits, 16).ok(),
        None => text.parse().ok(),
    }
}

fn arguments() -> Result<Arguments, String> {
    let mut values: BTreeMap<String, String> = BTreeMap::new();
    let mut cable_at_reset = false;
    let mut args = std::env::args().skip(1);
    while let Some(flag) = args.next() {
        if flag == "--cable-at-reset" {
            cable_at_reset = true;
            continue;
        }
        if ![
            "--run", "--boot-rom", "--installed-dat", "--adjustment-nand", "--output", "--toggle-checkpoints",
            "--checkpoint-inputs", "--adc-at-reset", "--access-probe-range",
        ]
        .contains(&flag.as_str())
        {
            return Err(USAGE.into());
        }
        let value = args.next().ok_or(USAGE)?;
        if values.insert(flag, value).is_some() {
            return Err(USAGE.into());
        }
    }
    let adc_at_reset = match values.remove("--adc-at-reset") {
        None => None,
        Some(text) => {
            let (channel, sample) = text.split_once('=').ok_or(USAGE)?;
            let channel = number(channel).and_then(|c| usize::try_from(c).ok()).filter(|&c| c < ADC_CHANNELS);
            let sample = number(sample).and_then(|s| u16::try_from(s).ok());
            Some((channel.ok_or(USAGE)?, sample.ok_or(USAGE)?))
        }
    };
    let probe_range = match values.remove("--access-probe-range") {
        None => None,
        Some(text) => {
            let (low, high) = text.split_once(':').ok_or(USAGE)?;
            let range = (number(low).ok_or(USAGE)?, number(high).ok_or(USAGE)?);
            if range.0 >= range.1 || range.1 > 1 << 32 {
                return Err(USAGE.into());
            }
            Some(range)
        }
    };
    let route = serde_json::from_str(&values.remove("--checkpoint-inputs").ok_or(USAGE)?)
        .map_err(|e| format!("--checkpoint-inputs: {e}"))?;
    let mut take = |name: &str| values.remove(name).map(PathBuf::from).ok_or_else(|| USAGE.to_owned());
    Ok(Arguments {
        run: take("--run")?,
        boot_rom: take("--boot-rom")?,
        installed_dat: take("--installed-dat")?,
        adjustment_nand: take("--adjustment-nand")?,
        output: take("--output")?,
        checkpoints: take("--toggle-checkpoints")?,
        route,
        cable_at_reset,
        adc_at_reset,
        probe_range,
    })
}

/// A route checkpoint's key: the SHA-256 of its identity document.
fn route_key(arguments: &Arguments, inputs: &Inputs, adjustment: &[u8]) -> Result<(CheckpointKey, Value), String> {
    let bridge = read(&std::env::current_exe().map_err(|e| e.to_string())?)?;
    let document = json!({
        "schema": CHECKPOINT_SCHEMA,
        "mode": "fujimod-usb-toggle-route-boot",
        "route": arguments.route,
        "inputs": {
            "installed_dat": sha256(&inputs.update),
            "adjustment_nand": sha256(adjustment),
            "run_document": sha256(&inputs.document_bytes),
            "boot_rom": sha256(&inputs.boot_rom),
        },
        "bridge": {"sha256": sha256(&bridge)},
    });
    Ok((CheckpointKey(sha256(document.to_string().as_bytes())), document))
}

/// The SHA-256 naming a route checkpoint directory.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(transparent)]
struct CheckpointKey(String);

impl std::fmt::Display for CheckpointKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "kebab-case")]
enum StartKind {
    Cold,
    Restored,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BootRecord {
    schema: u32,
    key: CheckpointKey,
    #[allow(dead_code)]
    inputs: Value,
    #[allow(dead_code)]
    instructions: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Start {
    #[allow(dead_code)]
    command: StartCommand,
    restore: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "kebab-case")]
enum StartCommand {
    Start,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Site {
    address: u32,
    label: String,
    r0: Option<u32>,
}

#[derive(Deserialize)]
#[serde(tag = "command", rename_all = "kebab-case")]
enum Request {
    State,
    Run { instructions: u64, #[serde(default)] stop_on: Vec<String> },
    Key { key: RouteKey, hold_instructions: Option<u64>, release_instructions: Option<u64> },
    Press { key: RouteKey },
    Release,
    Adc { channel: usize, sample: u16 },
    Cable { connected: bool },
    ReadMemory { address: u64, count: usize },
    TraceSites { sites: Vec<Site> },
    TraceHits { from: usize },
    Enumerate { budget: u64 },
    Transfer(Transfer),
    Capture { label: String },
    AccessProbe,
    SaveCheckpoint { replace: bool },
    Finish,
}

fn default_pulse(key: RouteKey) -> KeyPulse {
    match key {
        RouteKey::Up | RouteKey::Down | RouteKey::Left | RouteKey::Right => DIRECTIONAL_PULSE,
        RouteKey::Ok | RouteKey::Back | RouteKey::Q => MENU_PULSE,
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum AccessSource {
    CpuFetch,
    CpuAccess,
    Transfer,
}

impl AccessSource {
    fn parse(record: &Value) -> Result<Self, String> {
        match record["source"].as_str() {
            Some("cpu-fetch") => Ok(Self::CpuFetch),
            Some("cpu-read" | "cpu-write") => Ok(Self::CpuAccess),
            Some(_) if record["pc"].is_null() => Ok(Self::Transfer),
            other => Err(format!("unknown memory-watch source {other:?}")),
        }
    }
}

/// FujiMod's memory-watch state in the retained probe's convention. A CPU
/// record's count there is the instructions executed before the access;
/// FujiMod counts the accessing instruction as entered, one more. FujiMod can
/// also record one fetch twice at one count, which the retained probe
/// records once.
fn retained_convention(mut state: Value) -> Result<Value, String> {
    let records = state["records"].as_array_mut().ok_or("memory-watch records must be an array")?;
    let mut kept: Vec<Value> = Vec::with_capacity(records.len());
    for mut record in records.drain(..) {
        let source = AccessSource::parse(&record)?;
        if source != AccessSource::Transfer {
            let entered = record["instructions"].as_u64().ok_or("CPU access record lacks its count")?;
            record["instructions"] = json!(entered.checked_sub(1).ok_or("CPU access record at count 0")?);
        }
        let repeated = source == AccessSource::CpuFetch
            && kept.last().is_some_and(|last| {
                ["source", "pc", "address", "instructions"].iter().all(|key| last[*key] == record[*key])
            });
        if !repeated {
            kept.push(record);
        }
    }
    *records = kept;
    Ok(state)
}

fn hex_register(text: &str) -> Result<u64, String> {
    text.strip_prefix("0x")
        .and_then(|digits| u64::from_str_radix(digits, 16).ok())
        .ok_or_else(|| format!("trace register {text} is not hex"))
}

struct Server {
    bridge: Ff80,
    start: Value,
    key: CheckpointKey,
    inputs: Value,
    root: PathBuf,
    output: PathBuf,
    /// The r0 a site's entry must carry to be kept, by site label.
    filters: BTreeMap<String, u32>,
    hits: Vec<Value>,
    /// Machine trace hits already filtered into `hits`.
    converted: usize,
    held: Option<RouteKey>,
    probe: Option<Box<MemoryWatch>>,
    screenshots: Vec<Value>,
}

impl Server {
    fn usb_connected(&self) -> Result<bool, String> {
        let usb = &self.bridge.machine.document.usb.0;
        self.bridge.machine.session.usb_status(usb)?["connected"]
            .as_bool()
            .ok_or_else(|| "USB status lacks the cable level".into())
    }

    /// Filters new machine trace hits into `hits`; returns the first kept label in `stop_on`.
    fn collect(&mut self, stop_on: &BTreeSet<String>) -> Result<Option<String>, String> {
        let mut stopped = None;
        let raw = &self.bridge.machine.traces.hits;
        for hit in &raw[self.converted..] {
            let register = |name: &str| hit.registers.get(name).ok_or("trace hit lacks a register").map(|text| hex_register(text));
            let r0 = register("r0")??;
            if self.filters.get(&hit.site).is_some_and(|&wanted| u64::from(wanted) != r0) {
                continue;
            }
            if stopped.is_none() && stop_on.contains(&hit.site) {
                stopped = Some(hit.site.clone());
            }
            self.hits.push(json!({
                "label": hit.site, "address": hit.address, "core": hit.core, "instructions": hit.instructions,
                "registers": {"r0": r0, "r1": register("r1")??, "r2": register("r2")??, "r3": register("r3")??,
                    "sp": hex_register(&hit.sp)?, "lr": hex_register(&hit.lr)?},
            }));
        }
        self.converted = raw.len();
        Ok(stopped)
    }

    fn run(&mut self, count: u64, stop_on: BTreeSet<String>) -> Result<Option<String>, String> {
        let machine = &mut self.bridge.machine;
        let end = machine.instructions().saturating_add(count);
        machine.traces.stop_labels = stop_on.clone();
        let mut stopped = None;
        while stopped.is_none() && self.bridge.machine.instructions() < end {
            self.bridge.machine.require_running()?;
            let remaining = end - self.bridge.machine.instructions();
            self.bridge.machine.run(remaining);
            stopped = self.collect(&stop_on)?;
        }
        self.bridge.machine.traces.stop_labels.clear();
        self.bridge.machine.require_running()?;
        Ok(stopped)
    }

    fn handle(&mut self, request: Request) -> Result<Option<Value>, String> {
        Ok(Some(match request {
            Request::State => {
                let machine = &self.bridge.machine;
                let rate = machine.document.execution.instructions_per_tick;
                json!({
                    "instructions": machine.instructions(), "usb_connected": self.usb_connected()?,
                    "tick_ratio": format!("{}/{}", rate.numerator, rate.denominator),
                    "inputs_ready": machine.document.firmware.scanner_registered(&machine.session)?,
                    "asserted_keys": machine.document.firmware.asserted_keys(
                        &machine.session, &machine.session.adc_samples(&machine.document.adc.0)?)?,
                    "menu": menu_value(machine.menu()?),
                })
            }
            Request::Run { instructions, stop_on } => {
                if !(1..=MAX_RUN).contains(&instructions) {
                    return Err(format!("run takes 1..{MAX_RUN} instructions and a list of stop labels"));
                }
                let stopped = self.run(instructions, stop_on.into_iter().collect())?;
                json!({"stopped_on": stopped, "instructions": self.bridge.machine.instructions()})
            }
            Request::Key { key: route, hold_instructions, release_instructions } => {
                let pulse = match (hold_instructions, release_instructions) {
                    (None, None) => default_pulse(route),
                    (Some(hold), Some(release)) if (1..=MAX_PHASE).contains(&hold) && (1..=MAX_PHASE).contains(&release) => {
                        KeyPulse { hold, release }
                    }
                    _ => return Err(format!("hold and release must both be 1..{MAX_PHASE}")),
                };
                self.bridge.key(route, pulse)?;
                self.collect(&BTreeSet::new())?;
                self.bridge.machine.require_running()?;
                json!({"menu": menu_value(self.bridge.machine.menu()?)})
            }
            Request::Press { key: route } => {
                if self.held.is_some() {
                    return Err("a key is already held".into());
                }
                let sample = self.bridge.machine.press(route)?;
                self.held = Some(route);
                json!({"sample": sample})
            }
            Request::Release => {
                if let Some(key) = self.held.take() {
                    self.bridge.machine.release_key(key)?;
                }
                json!({"sample": self.bridge.machine.document.firmware.released_sample})
            }
            Request::Adc { channel, sample } => {
                if channel >= ADC_CHANNELS {
                    return Err(format!("adc takes a channel 0..{}", ADC_CHANNELS - 1));
                }
                let machine = &mut self.bridge.machine;
                set_adc(&mut machine.session, &machine.document.adc, channel, sample)?;
                json!({"samples": machine.session.adc_samples(&machine.document.adc.0)?})
            }
            Request::Cable { connected } => {
                let cable = self.bridge.machine.document.usb_cable.0.clone();
                self.bridge.machine.session.logic_input(&cable, connected)?;
                json!({"usb_connected": self.usb_connected()?, "instructions": self.bridge.machine.instructions()})
            }
            Request::ReadMemory { address, count } => {
                if !(1..=MAX_READ).contains(&count) || address + count as u64 > 1 << 32 {
                    return Err("read-memory requires an address and 1..4096 bytes".into());
                }
                json!({"bytes": encode_hex(&self.bridge.machine.read(address, count)?)})
            }
            Request::TraceSites { sites } => {
                let mut armed = BTreeMap::new();
                let mut filters = BTreeMap::new();
                for site in sites {
                    if let Some(r0) = site.r0 {
                        filters.insert(site.label.clone(), r0);
                    }
                    if armed.insert(site.address, site.label).is_some() {
                        return Err(format!("trace site {:#x} repeats", site.address));
                    }
                }
                self.collect(&BTreeSet::new())?;
                let count = armed.len();
                let machine = &mut self.bridge.machine;
                machine.traces.arm(&machine.session, armed)?;
                self.filters = filters;
                json!({"sites": count})
            }
            Request::TraceHits { from } => {
                self.collect(&BTreeSet::new())?;
                if from > self.hits.len() {
                    return Err("trace-hits requires a hit index within the recorded hits".into());
                }
                json!({"count": self.hits.len(), "hits": &self.hits[from..]})
            }
            Request::Enumerate { budget } => {
                if !(1..=MAX_ENUMERATE).contains(&budget) {
                    return Err(format!("enumerate takes an activation budget of 1..{MAX_ENUMERATE} instructions"));
                }
                let (device, configuration) = self.bridge.enumerate_attached(budget)?;
                self.collect(&BTreeSet::new())?;
                json!({"device": encode_hex(&device), "configuration": encode_hex(&configuration),
                    "instructions": self.bridge.machine.instructions()})
            }
            Request::Transfer(transfer) => {
                let (count, payload) = self.bridge.transfer(&transfer)?;
                self.collect(&BTreeSet::new())?;
                json!({"count": count, "bytes": encode_hex(&payload)})
            }
            Request::Capture { label } => {
                if label.is_empty() || !label.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-') {
                    return Err("screenshot label must be lowercase ASCII".into());
                }
                let png = self.bridge.machine.frame()?.ok_or("no display plane to capture")?;
                let directory = self.output.join("screenshots");
                std::fs::create_dir_all(&directory).map_err(|e| e.to_string())?;
                let path = directory.join(format!("{label}.png"));
                write(&path, &png)?;
                let shot = json!({"label": label, "path": path, "frame_number": null, "sha256": sha256(&png),
                    "menu": menu_value(self.bridge.machine.menu()?)});
                self.screenshots.push(shot.clone());
                shot
            }
            Request::AccessProbe => match &self.probe {
                Some(watch) => retained_convention(watch.records.borrow().state())?,
                None => json!({"enabled": false, "violation": null, "ranges": null, "records": []}),
            },
            Request::SaveCheckpoint { replace } => {
                if self.probe.is_some() {
                    return Err("an access-probe session saves no checkpoint".into());
                }
                let target = self.root.join(&self.key.0);
                if target.exists() && !replace {
                    return Err(format!("{} exists", target.display()));
                }
                let record = json!({"schema": CHECKPOINT_SCHEMA, "key": self.key, "inputs": self.inputs,
                    "instructions": self.bridge.machine.instructions()});
                let target = bridge::save_checkpoint(&mut self.bridge.machine, &self.root, &self.key.0, RECORD, &record)?;
                json!({"directory": std::fs::canonicalize(&target).map_err(|e| e.to_string())?,
                    "instructions": self.bridge.machine.instructions()})
            }
            Request::Finish => return Ok(None),
        }))
    }

    fn boundary(&self) -> Value {
        let machine = &self.bridge.machine;
        let usb = machine.session.usb_status(&machine.document.usb.0).unwrap_or_else(|error| json!({"unavailable": error}));
        json!({"instructions": machine.instructions(), "failure": machine.failure,
            "menu": machine.menu().map(menu_value).unwrap_or(Value::Null), "usb": usb})
    }

    fn serve(&mut self) -> Result<(), String> {
        let mut ready = self.start.clone();
        let fields = ready.as_object_mut().ok_or("start record must be an object")?;
        fields.insert("ok".into(), json!(true));
        fields.insert("event".into(), json!("ready"));
        fields.insert("instructions".into(), json!(self.bridge.machine.instructions()));
        fields.insert("usb_connected".into(), json!(self.usb_connected()?));
        emit(&ready)?;
        for line in std::io::stdin().lock().lines() {
            let line = line.map_err(|e| e.to_string())?;
            let result = serde_json::from_str::<Request>(&line)
                .map_err(|e| format!("invalid USB-toggle request: {e}"))
                .and_then(|request| self.handle(request));
            match result {
                Ok(Some(mut value)) => {
                    value.as_object_mut().ok_or("response must be an object")?.insert("ok".into(), json!(true));
                    emit(&value)?;
                }
                Ok(None) => {
                    emit(&json!({"ok": true, "finished": true}))?;
                    break;
                }
                Err(message) => {
                    emit(&json!({"ok": false, "error": "BridgeError", "message": message, "boundary": self.boundary()}))?;
                }
            }
        }
        std::fs::create_dir_all(&self.output).map_err(|e| e.to_string())?;
        let receipt = json!({
            "schema": 1,
            "scope": "emulated X-M1 USB-toggle primitive session on FujiMod's EXR II runtime",
            "start": self.start,
            "transfer_log": self.bridge.transfer_log,
            "screenshots": self.screenshots,
            "trace_hits": self.hits.len(),
            "boundary": self.boundary(),
        });
        write(&self.output.join("emulator-session.json"), format!("{receipt:#}\n").as_bytes())
    }
}

fn drive() -> Result<(), String> {
    fujimod_emulator_exr2_board::register_board_builder();
    fujimod_emulator_models::register();
    let arguments = arguments()?;
    let adjustment_bytes = read(&arguments.adjustment_nand)?;
    let inputs = Inputs {
        document_bytes: read(&arguments.run)?,
        update: read(&arguments.installed_dat)?,
        boot_rom: read(&arguments.boot_rom)?,
        adjustment: AdjustmentNand::parse(&adjustment_bytes)?,
    };
    std::fs::create_dir_all(&arguments.output).map_err(|e| format!("{}: {e}", arguments.output.display()))?;
    let (key, key_inputs) = route_key(&arguments, &inputs, &adjustment_bytes)?;
    let root = arguments.checkpoints.clone();
    let directory = root.join(&key.0);
    let saved = arguments.probe_range.is_none() && directory.join(SNAPSHOT).is_file() && directory.join(RECORD).is_file();
    emit(&json!({"ok": true, "event": "start-options", "key": key, "inputs": key_inputs,
        "checkpoint": saved.then(|| directory.clone())}))?;
    let mut line = String::new();
    std::io::stdin().lock().read_line(&mut line).map_err(|e| e.to_string())?;
    let start: Start = serde_json::from_str(&line)
        .map_err(|e| format!("USB-toggle start requires {{\"command\": \"start\", \"restore\": BOOLEAN}}: {e}"))?;
    let mut record = json!({"key": key, "inputs": key_inputs, "checkpoint_root": root,
        "access_probe_range": arguments.probe_range.map(|(low, high)| [low, high])});
    let mut probe = None;
    let machine = if start.restore {
        if !saved {
            return Err("USB-toggle start names no saved checkpoint".into());
        }
        let stored: BootRecord = serde_json::from_slice(&read(&directory.join(RECORD))?)
            .map_err(|e| format!("{}: {e}", directory.display()))?;
        if stored.schema != CHECKPOINT_SCHEMA || stored.key != key {
            return Err(format!("{} differs from key {key}", directory.display()));
        }
        let started = Instant::now();
        let machine = bridge::restore(&inputs, &directory)?;
        record["start"] = json!(StartKind::Restored);
        record["restore_seconds"] = json!(started.elapsed().as_secs_f64());
        machine
    } else {
        let mut machine = inputs.machine()?;
        if arguments.cable_at_reset {
            let cable = machine.document.usb_cable.0.clone();
            machine.session.logic_input(&cable, true)?;
        }
        if let Some((channel, sample)) = arguments.adc_at_reset {
            set_adc(&mut machine.session, &machine.document.adc, channel, sample)?;
        }
        if let Some(range) = arguments.probe_range {
            let document = RunDocument::parse(&inputs.document_bytes)?;
            probe = Some(machine.session.board.memory_watch_begin(vec![range], PROBE_LIMIT, run::memory_maps(&document)?)?);
        }
        record["start"] = json!(StartKind::Cold);
        machine
    };
    let mut server = Server {
        bridge: Ff80::new(machine),
        start: record,
        key,
        inputs: key_inputs,
        root,
        output: arguments.output,
        filters: BTreeMap::new(),
        hits: Vec::new(),
        converted: 0,
        held: None,
        probe,
        screenshots: Vec::new(),
    };
    server.serve()
}

fn main() -> ExitCode {
    match drive() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("fujimod-xm1-toggle: {error}");
            ExitCode::FAILURE
        }
    }
}
