//! The RAM-push bridge of `emulation/EmulatorUsb.hs` on FujiMod's EXR II
//! runtime: the same JSON-lines events and commands as the retained
//! `continue_rec_scanout.py --ram-push-server` (with `--sd-card-image`) and
//! `cardless-ram-push-bridge` (without), with attach and install checkpoints
//! saved as FujiMod session snapshots.
use fujimod_xm1::adjustment::AdjustmentNand;
use fujimod_xm1::bridge::{self, Inputs, SNAPSHOT, emit, load_card, menu_value, read, restore, sha256, write};
use fujimod_xm1::ff80::{Ff80, RENDEZVOUS_LOCKED, RendezvousInputs, Transfer, encode_hex};
use fujimod_xm1::guest::{DialTurn, ModeDialPosition, Press, RouteKey};
use fujimod_xm1::machine::{KeyPulse, Machine, MENU_PULSE, ReleaseSegment};
use fujimod_xm1::render::MAX_SETTLE;
use fujimod_xm1::run;
use fujimod_emulator::board::observation::MemoryWatch;
use fujimod_emulator::board::preservation::{
    PhysicalRange, Preservation, PreservationState, ProtectedRanges,
};
use fujimod_xm1::sd_host::{Backing, MediaImage, SectorAddress, SectorCount};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    io::BufRead,
    path::{Path, PathBuf},
    process::ExitCode,
    time::Instant,
};

const USAGE: &str = "Usage: fujimod-xm1-ram-push --run RUN.json --boot-rom ROM --installed-dat DAT \
--adjustment-nand NAND --output DIR --ram-push-checkpoints DIR [--sd-card-image CARD] [--attach-hold-key KEY] \
[--fresh-boot]\n";

const ATTACH_RECORD: &str = "ram-push-attach.json";
const INSTALL_RECORD: &str = "ram-push-install.json";
const CHECKPOINT_SCHEMA: u32 = 1;
const FIRST_START_SCREEN: u64 = 100_000_000;
const FIRST_START_PULSE: KeyPulse = KeyPulse { hold: 8_000_000, release: 8_000_000 };
const LIVE_VIEW_SETTLE: u64 = 40_000_000;
const MODE_DIAL_SETTLE: u64 = 40_000_000;
const DIRECTIONAL_PULSE: KeyPulse = KeyPulse { hold: 3_000_000, release: 12_000_000 };
const GPIO_PULSE: KeyPulse = KeyPulse { hold: 8_000_000, release: 8_000_000 };
/// `tools/probe_setup_menus.py` DIAL_PULSE.
const DIAL_PULSE: KeyPulse = KeyPulse { hold: 4_000_000, release: 1_000_000 };
const MAX_PHASE: u64 = 15_000_000;
/// `tools/continue_rec_scanout.py` ATTACH_HOLD_PULSE: the held key's lead before the cable rises
/// and its settle after release.
const ATTACH_HOLD_PULSE: u64 = 8_000_000;
const MAX_READ: usize = 4096;

/// Sites the retained bridge arms for every session.
const FIXED_SITES: [(u32, &str); 9] = [
    (0x01469b20, "request-bootstrap"),
    (0x01469b38, "request-bootstrap-return"),
    (0x00d806e8, "query-callback-result"),
    (0x00d8073c, "query-response-ready"),
    (0x006b0aa8, "cpu1-sync-wrapper"),
    (0x006b0c3c, "cpu1-range-wrapper"),
    (0x0061c490, "cpu1-cache-control"),
    (0x006c6b5c, "cpu1-worker"),
    (0x006b0704, "cpu1-worker-completion"),
];

struct Arguments {
    run: PathBuf,
    boot_rom: PathBuf,
    installed_dat: PathBuf,
    adjustment_nand: PathBuf,
    output: PathBuf,
    checkpoints: PathBuf,
    sd_card_image: Option<PathBuf>,
    attach_hold: Option<RouteKey>,
    fresh_boot: bool,
}

fn arguments() -> Result<Arguments, String> {
    let mut values: BTreeMap<String, PathBuf> = BTreeMap::new();
    let mut fresh_boot = false;
    let mut attach_hold = None;
    let mut args = std::env::args_os().skip(1);
    while let Some(flag) = args.next() {
        let flag = flag.to_str().ok_or(USAGE)?.to_owned();
        if flag == "--fresh-boot" {
            fresh_boot = true;
            continue;
        }
        if flag == "--attach-hold-key" {
            let name = args.next().ok_or(USAGE)?;
            if attach_hold.replace(RouteKey::parse(name.to_str().ok_or(USAGE)?)?).is_some() {
                return Err(USAGE.into());
            }
            continue;
        }
        if ![
            "--run",
            "--boot-rom",
            "--installed-dat",
            "--adjustment-nand",
            "--output",
            "--ram-push-checkpoints",
            "--sd-card-image",
        ]
        .contains(&flag.as_str())
        {
            return Err(USAGE.into());
        }
        let value = args.next().map(PathBuf::from).ok_or(USAGE)?;
        if values.insert(flag, value).is_some() {
            return Err(USAGE.into());
        }
    }
    let sd_card_image = values.remove("--sd-card-image");
    let mut take = |name: &str| values.remove(name).ok_or_else(|| USAGE.to_owned());
    Ok(Arguments {
        run: take("--run")?,
        boot_rom: take("--boot-rom")?,
        installed_dat: take("--installed-dat")?,
        adjustment_nand: take("--adjustment-nand")?,
        output: take("--output")?,
        checkpoints: take("--ram-push-checkpoints")?,
        sd_card_image,
        attach_hold,
        fresh_boot,
    })
}

/// The attach checkpoint's key and the identity document it hashes.
struct BootKey {
    key: CheckpointKey,
    inputs: Value,
}

/// The SHA-256 naming an attach checkpoint and prefixing its install checkpoints.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(transparent)]
struct CheckpointKey(String);

impl std::fmt::Display for CheckpointKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// How the bridge reached its first command, as `EmulatorUsb.BootStart` reads it.
#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "kebab-case")]
enum StartKind {
    Cold,
    FreshBoot,
    Restored,
    Installed,
}

fn boot_key(arguments: &Arguments, inputs: &Inputs, adjustment: &[u8]) -> Result<BootKey, String> {
    let bridge = read(&std::env::current_exe().map_err(|e| e.to_string())?)?;
    let card: Option<&MediaImage> = inputs.card.as_ref().map(|card| &card.image);
    let mut keyed = json!({
        "installed_dat": sha256(&inputs.update),
        "adjustment_nand": sha256(adjustment),
        "run_document": sha256(&inputs.document_bytes),
        "boot_rom": sha256(&inputs.boot_rom),
        "card_present": card.is_some(),
    });
    if let Some(card) = card {
        keyed["menu_sd_image"] = json!(card.sha256);
    }
    if let Some(key) = arguments.attach_hold {
        keyed["attach_hold_key"] = json!(key);
    }
    let inputs = json!({
        "schema": CHECKPOINT_SCHEMA,
        "mode": if card.is_some() { "fujimod-card-volatile-ram-bank" } else { "fujimod-cardless-volatile-ram-bank" },
        "inputs": keyed,
        "bridge": {"sha256": sha256(&bridge)},
        "paths": {"installed_dat": arguments.installed_dat, "adjustment_nand": arguments.adjustment_nand,
            "menu_sd_image": arguments.sd_card_image},
    });
    let mut identity = inputs.clone();
    identity.as_object_mut().ok_or("key document")?.remove("paths");
    Ok(BootKey { key: CheckpointKey(sha256(identity.to_string().as_bytes())), inputs })
}

/// The host record of an install checkpoint, kept whole for the e2e.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct InstallRecord {
    schema: u32,
    key: CheckpointKey,
    pre_attach: Value,
    device: String,
    configuration: String,
    base: String,
    manifest_sha256: String,
    runtime_sha256: String,
    host: Value,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AttachRecord {
    schema: u32,
    key: CheckpointKey,
    #[allow(dead_code)]
    inputs: Value,
    pre_attach: Value,
    device: String,
    configuration: String,
}

fn install_name(key: &CheckpointKey, manifest: &str, runtime: &str) -> String {
    format!("{key}-installed-{}", sha256(format!("{manifest}:{runtime}").as_bytes()))
}

fn install_checkpoints(root: &Path, key: &CheckpointKey) -> Result<Vec<(PathBuf, InstallRecord)>, String> {
    let Ok(entries) = std::fs::read_dir(root) else {
        return Ok(Vec::new());
    };
    let prefix = format!("{key}-installed-");
    let mut found = Vec::new();
    for entry in entries {
        let path = entry.map_err(|e| e.to_string())?.path();
        let named = path.file_name().and_then(|name| name.to_str()).is_some_and(|name| name.starts_with(&prefix));
        if named && path.join(INSTALL_RECORD).is_file() && path.join(SNAPSHOT).is_file() {
            let record: InstallRecord =
                serde_json::from_slice(&read(&path.join(INSTALL_RECORD))?).map_err(|e| format!("{}: {e}", path.display()))?;
            if record.schema != CHECKPOINT_SCHEMA || record.key != *key {
                return Err(format!("{} differs from key {key}", path.display()));
            }
            found.push((std::fs::canonicalize(&path).map_err(|e| e.to_string())?, record));
        }
    }
    found.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(found)
}

/// Releases a held rendezvous first.
fn save_checkpoint(host: &mut Ff80, root: &Path, name: &str, record_file: &str, record: &Value) -> Result<PathBuf, String> {
    host.release_rendezvous()?;
    bridge::save_checkpoint(&mut host.machine, root, name, record_file, record)
}

struct Attachment {
    pre_attach: Value,
    device: Vec<u8>,
    configuration: Vec<u8>,
}

/// Boots to live view past the first-start screens and enumerates ff80. A held key is pressed
/// `ATTACH_HOLD_PULSE` before the cable rises and released after enumeration.
fn attach(machine: Machine, rendezvous: RendezvousInputs, hold: Option<RouteKey>) -> Result<(Ff80, Attachment), String> {
    let mut bridge = Ff80::new(machine, rendezvous);
    bridge.advance(FIRST_START_SCREEN.saturating_sub(bridge.machine.instructions()))?;
    if rendezvous == RendezvousInputs::MenuUpAndWriteProtect && !bridge.machine.card_present()? {
        return Err("the SD host does not read the inserted card as present".into());
    }
    for key in [RouteKey::Up; 12].into_iter().chain([RouteKey::Ok, RouteKey::Ok]) {
        bridge.key(key, FIRST_START_PULSE)?;
    }
    bridge.advance(LIVE_VIEW_SETTLE)?;
    let pressed = bridge.machine.instructions();
    let mut pre_attach = json!({"instructions": pressed});
    if let Some(key) = hold {
        bridge.machine.press(key)?;
        bridge.advance(ATTACH_HOLD_PULSE)?;
    }
    let attached = bridge.machine.instructions();
    let (device, configuration) = bridge.enumerate()?;
    if let Some(key) = hold {
        bridge.machine.release_key(key)?;
        let released = bridge.machine.instructions();
        bridge.advance(ATTACH_HOLD_PULSE)?;
        pre_attach["attach_hold"] = json!({"key": key, "pressed_instructions": pressed,
            "attach_instructions": attached, "released_instructions": released,
            "menu_after_release": menu_value(bridge.machine.menu()?)});
    }
    Ok((bridge, Attachment { pre_attach, device, configuration }))
}

fn hex_field(text: &str) -> Result<Vec<u8>, String> {
    fujimod_xm1::ff80::decode_hex(text)
}

#[derive(Deserialize)]
#[serde(tag = "command", rename_all = "kebab-case")]
enum Request {
    List,
    Transfer(Transfer),
    TraceSites { sites: BTreeMap<String, String> },
    TraceHits { from: usize },
    Key { key: String, hold_instructions: Option<u64>, release_instructions: Option<u64>, settle_instructions: Option<u64> },
    ReleaseContext { segment: Option<ReleaseSegment> },
    ModeDial { position: ModeDialPosition },
    ReadMemory { address: u64, count: usize },
    MediaRead { first: SectorAddress, count: SectorCount },
    WatchBegin { ranges: Vec<[u64; 2]>, limit: usize },
    WatchState,
    PreservationBegin { ranges: Vec<[u64; 2]> },
    PreservationState,
    MediaState,
    Capture { label: String },
    SaveInstallCheckpoint { base: String, manifest_sha256: String, runtime_sha256: String, host: Value },
    Finish,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "kebab-case")]
enum RequestKind {
    List,
    Transfer,
    TraceSites,
    TraceHits,
    Key,
    ReleaseContext,
    ModeDial,
    ReadMemory,
    MediaRead,
    WatchBegin,
    WatchState,
    PreservationBegin,
    PreservationState,
    MediaState,
    Capture,
    SaveInstallCheckpoint,
    Finish,
}

impl Request {
    fn kind(&self) -> RequestKind {
        match self {
            Request::List => RequestKind::List,
            Request::Transfer(_) => RequestKind::Transfer,
            Request::TraceSites { .. } => RequestKind::TraceSites,
            Request::TraceHits { .. } => RequestKind::TraceHits,
            Request::Key { .. } => RequestKind::Key,
            Request::ReleaseContext { .. } => RequestKind::ReleaseContext,
            Request::ModeDial { .. } => RequestKind::ModeDial,
            Request::ReadMemory { .. } => RequestKind::ReadMemory,
            Request::MediaRead { .. } => RequestKind::MediaRead,
            Request::WatchBegin { .. } => RequestKind::WatchBegin,
            Request::WatchState => RequestKind::WatchState,
            Request::PreservationBegin { .. } => RequestKind::PreservationBegin,
            Request::PreservationState => RequestKind::PreservationState,
            Request::MediaState => RequestKind::MediaState,
            Request::Capture { .. } => RequestKind::Capture,
            Request::SaveInstallCheckpoint { .. } => RequestKind::SaveInstallCheckpoint,
            Request::Finish => RequestKind::Finish,
        }
    }
}

/// Host seconds per request kind, and seconds spent waiting for the host's next request.
#[derive(Default, Serialize)]
struct RequestProfile {
    kinds: BTreeMap<RequestKind, KindProfile>,
    host_wait_seconds: f64,
}

#[derive(Default, Serialize)]
struct KindProfile {
    requests: u64,
    seconds: f64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Start {
    #[allow(dead_code)]
    command: StartCommand,
    installed: Option<PathBuf>,
}

#[derive(Deserialize)]
#[serde(rename_all = "kebab-case")]
enum StartCommand {
    Start,
}

struct Server {
    bridge: Ff80,
    attachment: Attachment,
    checkpoint: Value,
    key: BootKey,
    root: PathBuf,
    output: PathBuf,
    screenshots: Vec<Value>,
    watch: Option<Box<MemoryWatch>>,
    preservation: Option<Box<Preservation>>,
    key_log: Vec<Value>,
    requests: RequestProfile,
    rendezvous: RendezvousInputs,
}

fn parse_address(text: &str) -> Result<u32, String> {
    text.strip_prefix("0x")
        .and_then(|digits| u32::from_str_radix(digits, 16).ok())
        .ok_or_else(|| format!("invalid trace site address {text}"))
}

fn default_pulse(key: Press) -> KeyPulse {
    match key {
        Press::Key(RouteKey::Up | RouteKey::Down | RouteKey::Left | RouteKey::Right) => DIRECTIONAL_PULSE,
        Press::Key(RouteKey::Ok | RouteKey::Back | RouteKey::Q) => MENU_PULSE,
        Press::Dial(DialTurn::Left | DialTurn::Right) => DIAL_PULSE,
        Press::Shutter(_) | Press::Play => GPIO_PULSE,
    }
}

impl Server {
    fn arm(&mut self, requested: &BTreeMap<String, String>) -> Result<usize, String> {
        let mut sites: BTreeMap<u32, String> =
            FIXED_SITES.iter().map(|(address, label)| (*address, (*label).to_owned())).collect();
        for (address, label) in requested {
            sites.insert(parse_address(address)?, label.clone());
        }
        let count = sites.len();
        let machine = &mut self.bridge.machine;
        machine.traces.stop_labels = [RENDEZVOUS_LOCKED.to_owned()].into();
        machine.traces.arm(&machine.session, sites)?;
        if !requested.is_empty() {
            self.bridge.enable_keepalive();
        }
        Ok(count)
    }

    fn handle(&mut self, request: Request) -> Result<Option<Value>, String> {
        Ok(Some(match request {
            Request::List => {
                let c = &self.attachment.configuration;
                let byte = |index: usize| c.get(index).copied().ok_or("configuration descriptor is short");
                json!({"candidate": {"path": "emulator:xm1", "vid": 0x04cb, "pid": 0xff80,
                    "configuration": byte(5)?, "interface": 0, "alternate": 0, "class": byte(14)?,
                    "subclass": byte(15)?, "protocol": byte(16)?, "input": 0x81, "output": 2,
                    "input_packet": 64, "output_packet": 64}})
            }
            Request::Transfer(transfer) => {
                let (count, payload) = self.bridge.transfer(&transfer)?;
                json!({"count": count, "bytes": encode_hex(&payload)})
            }
            Request::TraceSites { sites } => json!({"sites": self.arm(&sites)?}),
            Request::TraceHits { from } => {
                let hits = &self.bridge.machine.traces.hits;
                if from > hits.len() {
                    return Err("trace-hits requires a hit index within the recorded hits".into());
                }
                json!({"count": hits.len(), "hits": &hits[from..]})
            }
            Request::ReleaseContext { segment } => {
                self.bridge.machine.set_release_context(segment);
                json!({})
            }
            Request::Key { key, hold_instructions, release_instructions, settle_instructions } => {
                let route = Press::parse(&key)?;
                let pulse = match (hold_instructions, release_instructions) {
                    (None, None) => default_pulse(route),
                    (Some(hold), Some(release)) if (1..=MAX_PHASE).contains(&hold) && (1..=MAX_PHASE).contains(&release) => {
                        KeyPulse { hold, release }
                    }
                    _ => return Err(format!("hold and release must both be 1..{MAX_PHASE}")),
                };
                let budget = match settle_instructions {
                    None => MAX_SETTLE,
                    Some(budget) if (1..=MAX_SETTLE).contains(&budget) => budget,
                    Some(_) => return Err(format!("settle_instructions must be 1..{MAX_SETTLE}")),
                };
                let started = Instant::now();
                let before = self.bridge.machine.instructions();
                let settle = self.bridge.settled_key(route, pulse, budget)?;
                self.bridge.machine.require_running()?;
                self.key_log.push(json!({"key": key, "seconds": started.elapsed().as_secs_f64(),
                    "instructions": self.bridge.machine.instructions() - before, "settle": settle}));
                json!({"menu": menu_value(self.bridge.machine.menu()?), "settle": settle})
            }
            Request::ModeDial { position } => {
                let window = self.bridge.machine.set_mode_dial(position)?;
                self.bridge.advance(MODE_DIAL_SETTLE)?;
                json!({"position": position, "sample": window.midpoint(), "low": window.low, "high": window.high,
                    "menu": menu_value(self.bridge.machine.menu()?)})
            }
            Request::ReadMemory { address, count } => {
                if !(1..=MAX_READ).contains(&count) || address + count as u64 > 1 << 32 {
                    return Err("read-memory requires an address and 1..4096 bytes".into());
                }
                json!({"bytes": encode_hex(&self.bridge.machine.read(address, count)?)})
            }
            Request::MediaRead { first, count } => {
                let read = self.bridge.machine.read_media(first, count)?;
                json!({"first": read.first.0, "count": read.count.get(), "bytes": encode_hex(&read.bytes)})
            }
            Request::WatchBegin { ranges, limit } => {
                if self.watch.is_some() {
                    return Err("a memory watch is already armed".into());
                }
                if self.preservation.is_some() {
                    return Err("a preservation observation conflicts with memory watch".into());
                }
                if ranges.is_empty() || ranges.len() > 8
                    || !(1..=1 << 20).contains(&limit)
                    || ranges.iter().any(|range| range[0] >= range[1] || range[1] > u32::MAX as u64)
                {
                    return Err("watch-begin requires bounded 32-bit ranges and a positive record limit".into());
                }
                let maps = run::memory_maps(&self.bridge.machine.document)?;
                let watch = self.bridge.machine.session.board.memory_watch_begin(
                    ranges.iter().map(|range| (range[0], range[1])).collect(), limit, maps)?;
                self.watch = Some(watch);
                json!({"armed": true, "ranges": ranges})
            }
            Request::WatchState => match &self.watch {
                Some(watch) => watch.records.borrow().state(),
                None => return Err("watch-state requires an armed memory watch".into()),
            },
            Request::PreservationBegin { ranges } => {
                if self.preservation.is_some() {
                    return Err("a preservation observation is already armed".into());
                }
                if self.watch.is_some() {
                    return Err("a memory watch conflicts with preservation observation".into());
                }
                if ranges.is_empty()
                    || ranges.len() > 8
                    || ranges
                        .iter()
                        .any(|range| range[0] >= range[1] || range[1] > u32::MAX as u64)
                {
                    return Err("preservation-begin requires 1..8 bounded 32-bit ranges".into());
                }
                let ranges = ranges
                    .iter()
                    .map(|range| PhysicalRange::new(range[0], range[1]))
                    .collect::<Result<Vec<_>, _>>()?;
                let ranges = ProtectedRanges::new(ranges)
                    .map_err(|error| format!("preservation-begin: {error}"))?;
                let acknowledgement = json!({"armed": true, "ranges": &ranges});
                let preservation = self
                    .bridge
                    .machine
                    .session
                    .board
                    .preservation_begin(ranges)?;
                self.preservation = Some(preservation);
                acknowledgement
            }
            Request::PreservationState => match &self.preservation {
                Some(preservation) => serde_json::to_value(preservation.state())
                    .map_err(|error| format!("preservation-state: {error}"))?,
                None => serde_json::to_value(PreservationState::disabled())
                    .map_err(|error| format!("preservation-state: {error}"))?,
            },
            Request::MediaState => self.bridge.machine.card_state()?,
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
            Request::SaveInstallCheckpoint { base, manifest_sha256, runtime_sha256, host } => {
                let name = install_name(&self.key.key, &manifest_sha256, &runtime_sha256);
                let target = self.root.join(&name);
                if target.is_dir() {
                    json!({"directory": target, "saved": false, "existing": true})
                } else {
                    let started = Instant::now();
                    let record = json!({"schema": CHECKPOINT_SCHEMA, "key": self.key.key,
                        "pre_attach": self.attachment.pre_attach, "device": encode_hex(&self.attachment.device),
                        "configuration": encode_hex(&self.attachment.configuration), "base": base,
                        "manifest_sha256": manifest_sha256, "runtime_sha256": runtime_sha256, "host": host});
                    let target = save_checkpoint(&mut self.bridge, &self.root, &name, INSTALL_RECORD, &record)?;
                    json!({"directory": std::fs::canonicalize(&target).map_err(|e| e.to_string())?, "saved": true,
                        "existing": false, "seconds": started.elapsed().as_secs_f64()})
                }
            }
            Request::Finish => return Ok(None),
        }))
    }

    fn receipt(&self, error: Option<&str>) -> Result<Value, String> {
        let card = match self.rendezvous {
            RendezvousInputs::MenuUpAndWriteProtect => "card-backed",
            RendezvousInputs::MenuUp => "cardless",
        };
        let preservation = match &self.preservation {
            Some(preservation) => serde_json::to_value(preservation.state())
                .map_err(|error| format!("preservation receipt: {error}"))?,
            None => Value::Null,
        };
        Ok(json!({
            "schema": 1,
            "scope": format!("emulated X-M1 ordinary-boot {card} RAM push on FujiMod's EXR II runtime"),
            "inputs": self.key.inputs,
            "pre_attach": self.attachment.pre_attach,
            "device_descriptor": encode_hex(&self.attachment.device),
            "configuration_descriptor": encode_hex(&self.attachment.configuration),
            "checkpoint": self.checkpoint,
            "bridge_error": error,
            "key_log": self.key_log,
            "transfer_log": self.bridge.transfer_log,
            "transfer_profile": self.bridge.profile,
            "request_profile": self.requests,
            "rendezvous_activity": self.bridge.rendezvous,
            "keepalive_inputs": self.bridge.keepalive_inputs,
            "contexts": self.bridge.machine.traces.hits,
            "screenshots": self.screenshots,
            "memory_watch": self.watch.as_ref().map(|watch| watch.records.borrow().state()).unwrap_or(Value::Null),
            "preservation": preservation,
            "sd_host": self.bridge.machine.card_state().unwrap_or_else(|error| json!({"error": error})),
            "boundary": {"instructions": self.bridge.machine.instructions(), "failure": self.bridge.machine.failure},
        }))
    }

    fn serve(&mut self) -> Result<(), String> {
        emit(&json!({"ok": true, "event": "ready", "device": encode_hex(&self.attachment.device),
            "configuration": encode_hex(&self.attachment.configuration),
            "pre_attach": self.attachment.pre_attach, "checkpoint": self.checkpoint}))?;
        let mut error = None;
        let mut waiting = Instant::now();
        for line in std::io::stdin().lock().lines() {
            let line = line.map_err(|e| e.to_string())?;
            self.requests.host_wait_seconds += waiting.elapsed().as_secs_f64();
            let result = serde_json::from_str::<Request>(&line)
                .map_err(|e| format!("invalid RAM-push bridge request: {e}"))
                .and_then(|request| {
                    let started = Instant::now();
                    let kind = request.kind();
                    let result = self.handle(request);
                    let profile = self.requests.kinds.entry(kind).or_default();
                    profile.requests += 1;
                    profile.seconds += started.elapsed().as_secs_f64();
                    result
                });
            match result {
                Ok(Some(mut value)) => {
                    value.as_object_mut().ok_or("response must be an object")?.insert("ok".into(), json!(true));
                    emit(&value)?;
                    waiting = Instant::now();
                }
                Ok(None) => {
                    emit(&json!({"ok": true, "finished": true}))?;
                    break;
                }
                Err(message) => {
                    emit(&json!({"ok": false, "error": "BridgeError", "message": message}))?;
                    error = Some(message);
                    break;
                }
            }
        }
        std::fs::create_dir_all(&self.output).map_err(|e| e.to_string())?;
        let receipt = self.receipt(error.as_deref())?;
        write(&self.output.join("emulator-session.json"), format!("{receipt:#}\n").as_bytes())
    }
}

fn drive() -> Result<(), String> {
    fujimod_emulator_exr2_board::register_board_builder();
    fujimod_emulator_models::register();
    let started = Instant::now();
    let arguments = arguments()?;
    let adjustment_bytes = read(&arguments.adjustment_nand)?;
    let document_bytes = read(&arguments.run)?;
    let card = arguments
        .sd_card_image
        .as_deref()
        .map(|path| load_card(&document_bytes, path, Backing::Sparse))
        .transpose()?;
    let rendezvous = if card.is_some() { RendezvousInputs::MenuUpAndWriteProtect } else { RendezvousInputs::MenuUp };
    let inputs = Inputs {
        document_bytes,
        update: read(&arguments.installed_dat)?,
        boot_rom: read(&arguments.boot_rom)?,
        adjustment: AdjustmentNand::parse(&adjustment_bytes)?,
        card,
    };
    std::fs::create_dir_all(&arguments.output).map_err(|e| format!("{}: {e}", arguments.output.display()))?;
    let key = boot_key(&arguments, &inputs, &adjustment_bytes)?;
    let key_seconds = started.elapsed().as_secs_f64();
    let root = arguments.checkpoints.clone();
    let installed = if arguments.fresh_boot { Vec::new() } else { install_checkpoints(&root, &key.key)? };
    let listed: Vec<Value> = installed
        .iter()
        .map(|(directory, record)| {
            json!({"directory": directory, "base": record.base, "manifest_sha256": record.manifest_sha256,
                "runtime_sha256": record.runtime_sha256, "host": record.host})
        })
        .collect();
    emit(&json!({"ok": true, "event": "start-options", "boot_key": key.key, "installed": listed}))?;
    let mut line = String::new();
    std::io::stdin().lock().read_line(&mut line).map_err(|e| e.to_string())?;
    let start: Start = serde_json::from_str(&line).map_err(|e| format!("RAM-push start: {e}"))?;
    let attach_directory = root.join(&key.key.0);
    let restore_started = Instant::now();
    let (bridge, attachment, checkpoint) = match start.installed {
        Some(directory) => {
            let (directory, record) = installed
                .into_iter()
                .find(|(listed, _)| *listed == directory)
                .ok_or("RAM-push start names no listed install checkpoint")?;
            let machine = restore(&inputs, &directory)?;
            let attachment = Attachment {
                pre_attach: record.pre_attach,
                device: hex_field(&record.device)?,
                configuration: hex_field(&record.configuration)?,
            };
            let checkpoint = json!({"key": key.key, "inputs": key.inputs, "directory": attach_directory,
                "key_seconds": key_seconds, "start": StartKind::Installed, "installed": directory,
                "restore_seconds": restore_started.elapsed().as_secs_f64()});
            (Ff80::new(machine, rendezvous), attachment, checkpoint)
        }
        None if attach_directory.join(SNAPSHOT).is_file() && !arguments.fresh_boot => {
            let record: AttachRecord = serde_json::from_slice(&read(&attach_directory.join(ATTACH_RECORD))?)
                .map_err(|e| format!("{}: {e}", attach_directory.display()))?;
            if record.schema != CHECKPOINT_SCHEMA || record.key != key.key {
                return Err(format!("{} differs from key {}", attach_directory.display(), key.key));
            }
            let machine = restore(&inputs, &attach_directory)?;
            let attachment = Attachment {
                pre_attach: record.pre_attach,
                device: hex_field(&record.device)?,
                configuration: hex_field(&record.configuration)?,
            };
            let checkpoint = json!({"key": key.key, "inputs": key.inputs, "directory": attach_directory,
                "key_seconds": key_seconds, "start": StartKind::Restored,
                "restore_seconds": restore_started.elapsed().as_secs_f64()});
            (Ff80::new(machine, rendezvous), attachment, checkpoint)
        }
        None => {
            let boot_started = Instant::now();
            let (mut bridge, attachment) = attach(inputs.machine()?, rendezvous, arguments.attach_hold)?;
            let boot_seconds = boot_started.elapsed().as_secs_f64();
            let save_started = Instant::now();
            let record = json!({"schema": CHECKPOINT_SCHEMA, "key": key.key, "inputs": key.inputs,
                "pre_attach": attachment.pre_attach, "device": encode_hex(&attachment.device),
                "configuration": encode_hex(&attachment.configuration)});
            save_checkpoint(&mut bridge, &root, &key.key.0, ATTACH_RECORD, &record)?;
            let start = if arguments.fresh_boot { StartKind::FreshBoot } else { StartKind::Cold };
            let checkpoint = json!({"key": key.key, "inputs": key.inputs, "directory": attach_directory,
                "key_seconds": key_seconds, "start": start,
                "boot_seconds": boot_seconds, "saved": true, "save_seconds": save_started.elapsed().as_secs_f64()});
            (bridge, attachment, checkpoint)
        }
    };
    let mut server = Server {
        bridge,
        attachment,
        checkpoint,
        key,
        root,
        output: arguments.output,
        screenshots: Vec::new(),
        watch: None,
        preservation: None,
        key_log: Vec::new(),
        requests: RequestProfile::default(),
        rendezvous,
    };
    server.arm(&BTreeMap::new())?;
    server.serve()
}

fn main() -> ExitCode {
    match drive() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("fujimod-xm1-ram-push: {error}");
            ExitCode::FAILURE
        }
    }
}
