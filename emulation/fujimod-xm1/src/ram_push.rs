//! The RAM-push bridge of `emulation/EmulatorUsb.hs` on FujiMod's EXR II
//! runtime: the same JSON-lines events and commands as the retained
//! `continue_rec_scanout.py --ram-push-server` (with `--sd-card-image`) and
//! `cardless-ram-push-bridge` (without), with attach and install checkpoints
//! saved as FujiMod session snapshots.
use fujimod_xm1::adjustment::AdjustmentNand;
use fujimod_xm1::record_medium::RecordMedium;
use fujimod_xm1::bridge::{self, Inputs, SNAPSHOT, emit, load_card, menu_value, read, restore, sha256, write};
use fujimod_xm1::ff80::{Ff80, RENDEZVOUS_LOCKED, RendezvousInputs, Transfer, encode_hex};
use fujimod_xm1::guest::{DialTurn, ModeDialPosition, PhysicalShutterPosition, Press, RouteKey};
use fujimod_xm1::machine::{KeyPulse, Machine, ReleaseSegment, RunOutcome, key_pulse};
use fujimod_xm1::modal_trace::BeginRequest as ModalBeginRequest;
use fujimod_xm1::render::MAX_SETTLE;
use fujimod_xm1::run::{self, NodeName};
use fujimod_xm1::reset_observation::{AttachPhase, ResetObserver, Spec as ResetObservationSpec};
use fujimod_emulator::board::observation::{MemoryWatch, ViewPolicy, WatchKinds, WatchRange};
use fujimod_emulator::board::preservation::{
    PhysicalRange, Preservation, PreservationState, ProtectedRanges,
};
use fujimod_xm1::sd_host::{Backing, MediaImage, SectorAddress, SectorCount};
use fujimod_xm1::nand_read::{RowAddress, RowCount};
use fujimod_xm1::trace::{CAPTURE_OBSERVATION_BOUNDS, CaptureCursor, CaptureScope, CaptureSnapshot, InstructionCount, SiteRead};
use serde::{Deserialize, Serialize};
use fujimod_emulator_devices::Device;
use fujimod_emulator_exr2_isp_input::ImageInput;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    io::{BufRead, Write},
    path::{Path, PathBuf},
    process::ExitCode,
    time::Instant,
};

const USAGE: &str = "Usage: fujimod-xm1-ram-push --run RUN.json --boot-rom ROM --installed-dat DAT \
--adjustment-nand NAND [--record-nand NAND] --output DIR --ram-push-checkpoints DIR [--sd-card-image CARD] \
[--attach-hold-key KEY] [--fresh-boot [--reset-observation SPEC.json]]\n";

const ATTACH_RECORD: &str = "ram-push-attach.json";
const INSTALL_RECORD: &str = "ram-push-install.json";
const RESET_OBSERVATION_RECORD: &str = "reset-observation.json";
const CHECKPOINT_SCHEMA: u32 = 1;
const FIRST_START_SCREEN: u64 = 100_000_000;
const FIRST_START_PULSE: KeyPulse = KeyPulse::instructions(8_000_000, 8_000_000);
const LIVE_VIEW_SETTLE: u64 = 40_000_000;
const MODE_DIAL_SETTLE: u64 = 40_000_000;
const GPIO_HOLD: u64 = 8_000_000;
const GPIO_PULSE: KeyPulse = KeyPulse::instructions(GPIO_HOLD, 8_000_000);
/// `tools/probe_setup_menus.py` DIAL_PULSE.
const DIAL_PULSE: KeyPulse = KeyPulse::instructions(4_000_000, 1_000_000);
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
    record_nand: Option<PathBuf>,
    output: PathBuf,
    checkpoints: PathBuf,
    sd_card_image: Option<PathBuf>,
    reset_observation: Option<PathBuf>,
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
            "--record-nand",
            "--output",
            "--ram-push-checkpoints",
            "--sd-card-image",
            "--reset-observation",
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
    let record_nand = values.remove("--record-nand");
    let reset_observation = values.remove("--reset-observation");
    if reset_observation.is_some() && !fresh_boot {
        return Err(USAGE.into());
    }
    let mut take = |name: &str| values.remove(name).ok_or_else(|| USAGE.to_owned());
    Ok(Arguments {
        run: take("--run")?,
        boot_rom: take("--boot-rom")?,
        installed_dat: take("--installed-dat")?,
        adjustment_nand: take("--adjustment-nand")?,
        record_nand,
        output: take("--output")?,
        checkpoints: take("--ram-push-checkpoints")?,
        sd_card_image,
        reset_observation,
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

fn boot_key(arguments: &Arguments, inputs: &Inputs, adjustment: &[u8], records: Option<&[u8]>) -> Result<BootKey, String> {
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
    if let Some(records) = records {
        keyed["record_nand"] = json!(sha256(records));
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
            "record_nand": arguments.record_nand, "menu_sd_image": arguments.sd_card_image},
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
/// `ATTACH_HOLD_PULSE` before the cable rises and released after enumeration. An observer ends a
/// watch segment at each phase.
fn attach(machine: Machine, rendezvous: RendezvousInputs, hold: Option<RouteKey>, mut observer: Option<&mut ResetObserver>)
    -> Result<(Ff80, Attachment), String> {
    let mut bridge = Ff80::new(machine, rendezvous);
    let mut boundary = |bridge: &Ff80, phase| observer.as_mut().map_or(Ok(()), |observer| observer.boundary(&bridge.machine, phase));
    bridge.advance(FIRST_START_SCREEN.saturating_sub(bridge.machine.instructions()))?;
    boundary(&bridge, AttachPhase::FirstStart)?;
    if rendezvous == RendezvousInputs::MenuUpAndWriteProtect && !bridge.machine.card_present()? {
        return Err("the SD host does not read the inserted card as present".into());
    }
    for key in [RouteKey::Up; 12].into_iter().chain([RouteKey::Ok, RouteKey::Ok]) {
        bridge.key(key, FIRST_START_PULSE)?;
    }
    bridge.advance(LIVE_VIEW_SETTLE)?;
    boundary(&bridge, AttachPhase::LiveView)?;
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
    boundary(&bridge, AttachPhase::Attached)?;
    Ok((bridge, Attachment { pre_attach, device, configuration }))
}

fn hex_field(text: &str) -> Result<Vec<u8>, String> {
    fujimod_xm1::ff80::decode_hex(text)
}

#[derive(Clone, Copy, Default, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
enum WatchViews {
    #[default]
    Observe,
    Fixed,
}

#[derive(Deserialize)]
#[serde(tag = "command", rename_all = "kebab-case")]
enum Request {
    List,
    Transfer(Transfer),
    TraceSites { sites: BTreeMap<String, String>, #[serde(default)] reads: BTreeMap<String, Vec<[u64; 2]>> },
    TraceHits { from: usize, #[serde(default)] limit: Option<std::num::NonZeroUsize> },
    ModalObservationBegin(ModalBeginRequest),
    ModalObservationReport,
    ModalObservationEnd,
    CaptureObservationBegin { #[serde(default)] scope: CaptureScope },
    CaptureObservationHits { from: CaptureCursor },
    CaptureObservationAck { next: CaptureCursor },
    CaptureObservationResume,
    CaptureObservationState,
    Key { key: String, hold_instructions: Option<u64>, release_instructions: Option<u64>, settle_instructions: Option<u64> },
    ShutterSequence,
    ImageInputFingerprint { node: NodeName },
    CaptureObservationWindow,
    ReleaseContext { segment: Option<ReleaseSegment> },
    ModeDial { position: ModeDialPosition },
    ReadMemory { address: u64, count: usize },
    MediaRead { first: SectorAddress, count: SectorCount },
    NandRead { first: RowAddress, count: RowCount },
    WatchBegin {
        ranges: Vec<[u64; 2]>, limit: usize,
        #[serde(default)] views: WatchViews,
        #[serde(default)] write_ranges: Vec<[u64; 2]>,
        #[serde(default)] fetch_ranges: Vec<[u64; 2]>,
    },
    WatchState,
    WatchEnd,
    PreservationBegin { ranges: Vec<[u64; 2]> },
    PreservationState,
    MediaState,
    Capture { label: String },
    SaveInstallCheckpoint { base: String, manifest_sha256: String, runtime_sha256: String, host: Value },
    SnapshotSave { name: SnapshotName },
    SnapshotRestore { name: SnapshotName },
    SnapshotKey { name: SnapshotName, key: PressName },
    /// A `snapshot-key` press from the current guest.
    DisplayKey { key: PressName },
    /// Runs the guest for `instructions` with no input.
    Idle { instructions: GuestInstructions },
    SnapshotDrop { name: SnapshotName },
    Finish,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "kebab-case")]
enum RequestKind {
    List,
    Transfer,
    TraceSites,
    TraceHits,
    ModalObservationBegin,
    ModalObservationReport,
    ModalObservationEnd,
    CaptureObservationBegin,
    CaptureObservationHits,
    CaptureObservationAck,
    CaptureObservationResume,
    CaptureObservationState,
    Key,
    ShutterSequence,
    ImageInputFingerprint,
    CaptureObservationWindow,
    ReleaseContext,
    ModeDial,
    ReadMemory,
    MediaRead,
    NandRead,
    WatchBegin,
    WatchState,
    WatchEnd,
    PreservationBegin,
    PreservationState,
    MediaState,
    Capture,
    SaveInstallCheckpoint,
    SnapshotSave,
    SnapshotRestore,
    SnapshotKey,
    DisplayKey,
    Idle,
    SnapshotDrop,
    Finish,
}

impl Request {
    fn kind(&self) -> RequestKind {
        match self {
            Request::List => RequestKind::List,
            Request::Transfer(_) => RequestKind::Transfer,
            Request::TraceSites { .. } => RequestKind::TraceSites,
            Request::TraceHits { .. } => RequestKind::TraceHits,
            Request::ModalObservationBegin(_) => RequestKind::ModalObservationBegin,
            Request::ModalObservationReport => RequestKind::ModalObservationReport,
            Request::ModalObservationEnd => RequestKind::ModalObservationEnd,
            Request::CaptureObservationBegin { .. } => RequestKind::CaptureObservationBegin,
            Request::CaptureObservationHits { .. } => RequestKind::CaptureObservationHits,
            Request::CaptureObservationAck { .. } => RequestKind::CaptureObservationAck,
            Request::CaptureObservationResume => RequestKind::CaptureObservationResume,
            Request::CaptureObservationState => RequestKind::CaptureObservationState,
            Request::Key { .. } => RequestKind::Key,
            Request::ShutterSequence => RequestKind::ShutterSequence,
            Request::ImageInputFingerprint { .. } => RequestKind::ImageInputFingerprint,
            Request::CaptureObservationWindow => RequestKind::CaptureObservationWindow,
            Request::ReleaseContext { .. } => RequestKind::ReleaseContext,
            Request::ModeDial { .. } => RequestKind::ModeDial,
            Request::ReadMemory { .. } => RequestKind::ReadMemory,
            Request::MediaRead { .. } => RequestKind::MediaRead,
            Request::NandRead { .. } => RequestKind::NandRead,
            Request::WatchBegin { .. } => RequestKind::WatchBegin,
            Request::WatchState => RequestKind::WatchState,
            Request::WatchEnd => RequestKind::WatchEnd,
            Request::PreservationBegin { .. } => RequestKind::PreservationBegin,
            Request::PreservationState => RequestKind::PreservationState,
            Request::MediaState => RequestKind::MediaState,
            Request::Capture { .. } => RequestKind::Capture,
            Request::SaveInstallCheckpoint { .. } => RequestKind::SaveInstallCheckpoint,
            Request::SnapshotSave { .. } => RequestKind::SnapshotSave,
            Request::SnapshotRestore { .. } => RequestKind::SnapshotRestore,
            Request::SnapshotKey { .. } => RequestKind::SnapshotKey,
            Request::DisplayKey { .. } => RequestKind::DisplayKey,
            Request::Idle { .. } => RequestKind::Idle,
            Request::SnapshotDrop { .. } => RequestKind::SnapshotDrop,
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
    pending_capture: Option<PendingCapture>,
    capture_evidence_error: Option<String>,
    /// In-memory guest snapshots by client name.
    snapshots: BTreeMap<SnapshotName, Vec<u8>>,
}

/// About 120 MB per held snapshot of the X-M1 run document.
const SNAPSHOT_LIMIT: usize = 24;

const DISPLAY_STEP: u64 = 2_000_000;
const DISPLAY_WAIT: u64 = 60_000_000;

/// A client-chosen snapshot name: 1..=64 lowercase ASCII letters, digits or '-'.
#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(try_from = "String", into = "String")]
struct SnapshotName(String);

impl TryFrom<String> for SnapshotName {
    type Error = String;
    fn try_from(name: String) -> Result<Self, String> {
        if (1..=64).contains(&name.len()) && name.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-') {
            Ok(Self(name))
        } else {
            Err("snapshot name must be 1..64 lowercase ASCII letters, digits or '-'".into())
        }
    }
}

impl From<SnapshotName> for String {
    fn from(name: SnapshotName) -> Self {
        name.0
    }
}

/// A physical press as named on the wire, parsed once.
#[derive(Clone, Deserialize)]
#[serde(try_from = "String")]
struct PressName {
    name: String,
    press: Press,
}

impl TryFrom<String> for PressName {
    type Error = String;
    fn try_from(name: String) -> Result<Self, String> {
        let press = Press::parse(&name)?;
        Ok(Self { name, press })
    }
}

fn parse_address(text: &str) -> Result<u32, String> {
    text.strip_prefix("0x")
        .and_then(|digits| u32::from_str_radix(digits, 16).ok())
        .ok_or_else(|| format!("invalid trace site address {text}"))
}

/// The per-site reads of a `trace-sites` request: each named site is a requested one and each read
/// is `[address, count]` with 1..=`MAX_READ` bytes inside the 32-bit physical space.
fn parse_site_reads(sites: &BTreeMap<String, String>, reads: &BTreeMap<String, Vec<[u64; 2]>>)
    -> Result<BTreeMap<u32, Vec<SiteRead>>, String> {
    reads.iter().map(|(site, ranges)| {
        if !sites.contains_key(site) {
            return Err(format!("trace site read names an unrequested site {site}"));
        }
        let ranges = ranges.iter().map(|&[address, count]| {
            let count = usize::try_from(count).map_err(|_| "trace site read count exceeds the host".to_owned())?;
            if !(1..=MAX_READ).contains(&count) || address + count as u64 > 1 << 32 {
                return Err("a trace site read requires an address and 1..4096 bytes".to_owned());
            }
            Ok(SiteRead { address, count })
        }).collect::<Result<Vec<_>, String>>()?;
        Ok((parse_address(site)?, ranges))
    }).collect()
}

fn default_pulse(key: Press) -> KeyPulse {
    match key {
        Press::Key(key) => key_pulse(key),
        Press::Dial(DialTurn::Left | DialTurn::Right) => DIAL_PULSE,
        Press::Shutter(_) | Press::Play => GPIO_PULSE,
    }
}

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(transparent)]
struct GuestInstructions(u64);

const CAPTURE_OBSERVATION_WINDOW: GuestInstructions = GuestInstructions(MODE_DIAL_SETTLE);

#[derive(Deserialize, Serialize)]
#[serde(transparent)]
struct DeviceTicks(u64);

#[derive(Deserialize, Serialize)]
#[serde(transparent)]
struct PendingPhaseFlags(u32);

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(transparent)]
struct ImageInputRegister(u32);

struct ImageInputRegisters([ImageInputRegister; 256]);

impl Serialize for ImageInputRegisters {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.as_slice().serialize(serializer)
    }
}

#[derive(Serialize)]
#[serde(tag = "status", content = "value", rename_all = "kebab-case")]
enum InspectedField<T> {
    Present(T),
    Absent,
    Unsupported,
}

fn inspected_field<T: serde::de::DeserializeOwned>(state: &Value, name: &str) -> InspectedField<T> {
    match state.get(name) {
        None => InspectedField::Absent,
        Some(value) => match T::deserialize(value) {
            Ok(value) => InspectedField::Present(value),
            Err(_) => InspectedField::Unsupported,
        },
    }
}

fn inspected_registers(state: &Value) -> InspectedField<ImageInputRegisters> {
    let Some(value) = state.get("r") else {
        return InspectedField::Absent;
    };
    let Some(words) = value.as_array().filter(|words| words.len() == 256) else {
        return InspectedField::Unsupported;
    };
    let mut registers = [ImageInputRegister(0); 256];
    for (register, word) in registers.iter_mut().zip(words) {
        let Ok(value) = ImageInputRegister::deserialize(word) else {
            return InspectedField::Unsupported;
        };
        *register = value;
    }
    InspectedField::Present(ImageInputRegisters(registers))
}

#[derive(Serialize)]
struct ImageInputTimingState {
    phase_due: InspectedField<[Option<DeviceTicks>; 2]>,
    phase_pending: InspectedField<[PendingPhaseFlags; 2]>,
    phase_period: InspectedField<DeviceTicks>,
    sensor_due: InspectedField<Option<DeviceTicks>>,
    r: InspectedField<ImageInputRegisters>,
    capture_ack: InspectedField<bool>,
    capture_irq_sent: InspectedField<bool>,
}

#[derive(Serialize)]
#[serde(tag = "status", rename_all = "kebab-case")]
enum ImageInputTiming {
    Available { state: ImageInputTimingState },
    Absent,
    Unsupported { reason: String },
}

fn image_input_timing(machine: &Machine) -> ImageInputTiming {
    let Ok(node) = machine.session.node("image_input") else {
        return ImageInputTiming::Absent;
    };
    let fingerprint = match machine.session.board.inspect_device::<ImageInput, _>(
        node,
        |device| device.fingerprint(),
    ) {
        Ok(fingerprint) => fingerprint,
        Err(reason) => return ImageInputTiming::Unsupported { reason },
    };
    let Some(state) = fingerprint.get("state") else {
        return ImageInputTiming::Absent;
    };
    if !state.is_object() {
        return ImageInputTiming::Unsupported { reason: "image-input fingerprint state is not an object".into() };
    }
    ImageInputTiming::Available {
        state: ImageInputTimingState {
            phase_due: inspected_field(state, "phase_due"),
            phase_pending: inspected_field(state, "phase_pending"),
            phase_period: inspected_field(state, "phase_period"),
            sensor_due: inspected_field(state, "sensor_due"),
            r: inspected_registers(state),
            capture_ack: inspected_field(state, "capture_ack"),
            capture_irq_sent: inspected_field(state, "capture_irq_sent"),
        },
    }
}

#[derive(Serialize)]
struct CaptureObservationState {
    snapshot: Option<CaptureSnapshot>,
    timing: ImageInputTiming,
}

fn capture_observation_state(machine: &Machine) -> Result<CaptureObservationState, String> {
    Ok(CaptureObservationState {
        snapshot: machine.traces.capture_snapshot(&machine.session)?,
        timing: image_input_timing(machine),
    })
}

#[derive(Serialize)]
struct ShutterPhaseObservation {
    position: PhysicalShutterPosition,
    started: GuestInstructions,
    finished: GuestInstructions,
    #[serde(skip_serializing_if = "Option::is_none")]
    before: Option<CaptureObservationState>,
    #[serde(skip_serializing_if = "Option::is_none")]
    after: Option<CaptureObservationState>,
}

fn shutter_phase(
    bridge: &mut Ff80,
    position: PhysicalShutterPosition,
    duration: GuestInstructions,
) -> Result<ShutterPhaseObservation, String> {
    let started = GuestInstructions(bridge.machine.instructions());
    let observing = bridge.machine.traces.capture_observation_active();
    let before = observing.then(|| capture_observation_state(&bridge.machine)).transpose()?;
    bridge.machine.set_shutter_position(position)?;
    bridge.advance(duration.0)?;
    Ok(ShutterPhaseObservation {
        position,
        started,
        finished: GuestInstructions(bridge.machine.instructions()),
        before,
        after: observing.then(|| capture_observation_state(&bridge.machine)).transpose()?,
    })
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "kebab-case")]
enum CaptureOperation {
    ShutterSequence,
    Window,
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "kebab-case")]
enum CaptureProgressState {
    Completed,
    DrainRequired,
}

#[derive(Clone, Copy)]
enum ShutterPhaseIndex {
    HalfLead,
    Full,
    HalfRelease,
    Released,
}

impl Serialize for ShutterPhaseIndex {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_u8(match self {
            Self::HalfLead => 0, Self::Full => 1, Self::HalfRelease => 2, Self::Released => 3,
        })
    }
}

impl ShutterPhaseIndex {
    fn position(self) -> PhysicalShutterPosition {
        match self {
            Self::HalfLead | Self::HalfRelease => PhysicalShutterPosition::Half,
            Self::Full => PhysicalShutterPosition::Full,
            Self::Released => PhysicalShutterPosition::Released,
        }
    }

    fn duration(self) -> GuestInstructions {
        GuestInstructions(match self {
            Self::HalfLead | Self::Full => GPIO_HOLD,
            Self::HalfRelease | Self::Released => GPIO_PULSE.release / 2,
        })
    }

    fn next(self) -> Option<Self> {
        match self {
            Self::HalfLead => Some(Self::Full),
            Self::Full => Some(Self::HalfRelease),
            Self::HalfRelease => Some(Self::Released),
            Self::Released => None,
        }
    }
}

struct PendingShutterPhase {
    index: ShutterPhaseIndex,
    started: GuestInstructions,
    before: CaptureObservationState,
}

struct PendingCapture {
    operation: CaptureOperation,
    endpoint: GuestInstructions,
    phases: Vec<ShutterPhaseObservation>,
    current: Option<PendingShutterPhase>,
}

#[derive(Serialize)]
struct CaptureProgress<'a> {
    state: CaptureProgressState,
    operation: CaptureOperation,
    instructions: GuestInstructions,
    intended_endpoint: GuestInstructions,
    #[serde(skip_serializing_if = "Option::is_none")]
    current_phase: Option<ShutterPhaseIndex>,
    #[serde(skip_serializing_if = "Option::is_none")]
    position: Option<PhysicalShutterPosition>,
    #[serde(skip_serializing_if = "Option::is_none")]
    phases: Option<&'a [ShutterPhaseObservation]>,
}

impl Server {
    fn start_capture_operation(&mut self, operation: CaptureOperation) -> Result<Value, String> {
        self.bridge.machine.require_running()?;
        if !self.bridge.machine.traces.capture_observation_active() {
            return Err("capture progress requires an active observation".into());
        }
        if self.pending_capture.is_some() || self.bridge.machine.traces.capture_page_outstanding() {
            return Err("capture operation requires no pending continuation or published page".into());
        }
        let pending = match operation {
            CaptureOperation::Window => PendingCapture {
                operation,
                endpoint: GuestInstructions(self.bridge.machine.instructions().checked_add(CAPTURE_OBSERVATION_WINDOW.0)
                    .ok_or("capture instruction endpoint exceeds 64 bits")?),
                phases: Vec::new(), current: None,
            },
            CaptureOperation::ShutterSequence => self.start_capture_phase(ShutterPhaseIndex::HalfLead, Vec::with_capacity(4))?,
        };
        self.continue_capture(pending)
    }

    fn start_capture_phase(&mut self, index: ShutterPhaseIndex, phases: Vec<ShutterPhaseObservation>) -> Result<PendingCapture, String> {
        let started = GuestInstructions(self.bridge.machine.instructions());
        let endpoint = GuestInstructions(started.0.checked_add(index.duration().0)
            .ok_or("capture instruction endpoint exceeds 64 bits")?);
        let before = capture_observation_state(&self.bridge.machine)?;
        self.bridge.machine.set_shutter_position(index.position())?;
        Ok(PendingCapture {
            operation: CaptureOperation::ShutterSequence, endpoint, phases,
            current: Some(PendingShutterPhase { index, started, before }),
        })
    }

    fn continue_capture(&mut self, mut pending: PendingCapture) -> Result<Value, String> {
        loop {
            let remaining = pending.endpoint.0.saturating_sub(self.bridge.machine.instructions());
            let outcome = match self.bridge.advance(remaining) {
                Ok(outcome) => outcome,
                Err(error) => {
                    self.pending_capture = Some(pending);
                    return Err(error);
                }
            };
            let drained = outcome == RunOutcome::DrainRequired;
            if !drained {
                if let Some(current) = pending.current.take() {
                    pending.phases.push(ShutterPhaseObservation {
                        position: current.index.position(), started: current.started,
                        finished: GuestInstructions(self.bridge.machine.instructions()),
                        before: Some(current.before),
                        after: Some(capture_observation_state(&self.bridge.machine)?),
                    });
                    if let Some(next) = current.index.next() {
                        pending = self.start_capture_phase(next, pending.phases)?;
                        continue;
                    }
                }
            }
            let progress = CaptureProgress {
                state: if drained { CaptureProgressState::DrainRequired } else { CaptureProgressState::Completed },
                operation: pending.operation,
                instructions: GuestInstructions(self.bridge.machine.instructions()),
                intended_endpoint: pending.endpoint,
                current_phase: pending.current.as_ref().map(|phase| phase.index),
                position: pending.current.as_ref().map(|phase| phase.index.position()),
                phases: matches!(pending.operation, CaptureOperation::ShutterSequence).then_some(pending.phases.as_slice()),
            };
            let response = serde_json::to_value(progress).map_err(|error| format!("capture progress: {error}"))?;
            if drained {
                self.pending_capture = Some(pending);
            }
            return Ok(response);
        }
    }

    fn collect_capture_tail(&mut self) {
        let result = (|| {
            let machine = &mut self.bridge.machine;
            if machine.traces.capture_observation_active() && !machine.traces.capture_page_outstanding() {
                let instructions = InstructionCount::from_board(&machine.session)?;
                machine.traces.collect(&machine.session, instructions)?;
            }
            Ok::<(), String>(())
        })();
        if let Err(error) = result {
            self.capture_evidence_error.get_or_insert(error);
        }
    }

    fn arm(&mut self, requested: &BTreeMap<String, String>) -> Result<usize, String> {
        let parsed = requested.iter()
            .map(|(address, label)| Ok((parse_address(address)?, label.clone())))
            .collect::<Result<BTreeMap<u32, String>, String>>()?;
        self.arm_sites(&parsed)
    }

    fn arm_sites(&mut self, requested: &BTreeMap<u32, String>) -> Result<usize, String> {
        let mut sites: BTreeMap<u32, String> =
            FIXED_SITES.iter().map(|(address, label)| (*address, (*label).to_owned())).collect();
        sites.extend(requested.iter().map(|(address, label)| (*address, label.clone())));
        let count = sites.len();
        let machine = &mut self.bridge.machine;
        machine.traces.arm(&machine.session, sites)?;
        machine.traces.stop_labels = [RENDEZVOUS_LOCKED.to_owned()].into();
        if !requested.is_empty() {
            self.bridge.enable_keepalive();
        }
        Ok(count)
    }

    /// Returns the terminal stop the restored guest state replaced. After a stop the R4F guard
    /// restarts empty: its entered-block set holds blocks the refusal left undecoded.
    fn restore_snapshot(&mut self, name: &SnapshotName) -> Result<Option<String>, String> {
        let bytes = self.snapshots.get(name).ok_or_else(|| format!("snapshot {} is not held", name.0))?;
        let machine = &mut self.bridge.machine;
        let snapshot = machine.session.import_snapshot(bytes)?;
        machine.disarm()?;
        machine.session.restore(&snapshot)?;
        let cleared = machine.failure.take();
        if cleared.is_some() {
            machine.guard = Default::default();
        }
        machine.rearm()?;
        Ok(cleared)
    }

    /// Advances in DISPLAY_STEP slices until a scanout instance presents graphics and returns the
    /// instructions spent; a press can settle while no plane is presented.
    fn await_display_plane(&mut self) -> Result<u64, String> {
        let mut waited = 0;
        while !self.bridge.machine.presents_display_plane()? {
            if waited >= DISPLAY_WAIT {
                return Err(format!("no display plane after {waited} instructions"));
            }
            self.bridge.advance(DISPLAY_STEP)?;
            self.bridge.machine.require_running()?;
            waited += DISPLAY_STEP;
        }
        Ok(waited)
    }

    /// Presses `key`, then waits for a presented plane; a failed press is returned as its `failure`.
    fn display_press(&mut self, key: &PressName, snapshot: Option<&SnapshotName>) -> Result<Value, String> {
        let started = Instant::now();
        let before = self.bridge.machine.instructions();
        let pressed = self.bridge.settled_key(key.press, default_pulse(key.press), MAX_SETTLE)
            .and_then(|settle| self.bridge.machine.require_running().map(|()| settle))
            .and_then(|settle| self.await_display_plane().map(|waited| (settle, waited)));
        let instructions = self.bridge.machine.instructions() - before;
        let (settle, display_wait, menu, failure) = match pressed {
            Ok((settle, waited)) => (json!(settle), json!(waited), menu_value(self.bridge.machine.menu()?), Value::Null),
            Err(failure) => (Value::Null, Value::Null, Value::Null, json!(failure)),
        };
        self.key_log.push(json!({"key": key.name, "snapshot": snapshot, "seconds": started.elapsed().as_secs_f64(),
            "instructions": instructions, "settle": settle, "display_wait_instructions": display_wait, "failure": failure}));
        Ok(json!({"snapshot": snapshot, "key": key.name, "menu": menu, "settle": settle, "instructions": instructions,
            "display_wait_instructions": display_wait, "failure": failure}))
    }

    fn handle(&mut self, request: Request) -> Result<Option<Value>, String> {
        let tail_request = matches!(&request, Request::CaptureObservationHits { .. } | Request::CaptureObservationAck { .. }
            | Request::CaptureObservationState | Request::WatchState | Request::Finish
            | Request::ModalObservationReport | Request::ModalObservationEnd);
        let recovers = matches!(&request, Request::TraceHits { .. } | Request::SnapshotRestore { .. }
            | Request::SnapshotKey { .. } | Request::SnapshotDrop { .. });
        if self.bridge.machine.failure.is_some() && !tail_request && !recovers {
            return Err(self.bridge.machine.failure.as_ref().ok_or("terminal failure disappeared")?.clone());
        }
        let read_only = tail_request || matches!(&request, Request::List | Request::TraceHits { .. }
            | Request::ImageInputFingerprint { .. } | Request::ReadMemory { .. } | Request::MediaRead { .. } | Request::NandRead { .. }
            | Request::WatchState | Request::PreservationState | Request::MediaState);
        if !read_only {
            if let Some(failure) = self.bridge.machine.traces.modal_observation_report().failure {
                return Err(failure.to_owned());
            }
        }
        if (self.pending_capture.is_some() || self.bridge.machine.traces.capture_page_outstanding())
            && !read_only && !matches!(&request, Request::CaptureObservationResume) {
            return Err("capture continuation must be drained and resumed before mutation".into());
        }
        if self.bridge.machine.traces.capture_observation_active() && !read_only
            && !matches!(&request, Request::ShutterSequence | Request::CaptureObservationWindow
                | Request::CaptureObservationResume | Request::Capture { .. }) {
            return Err("active capture refuses this mutating request".into());
        }
        if self.bridge.machine.traces.modal_observation_active()
            && matches!(&request, Request::SaveInstallCheckpoint { .. }) {
            return Err("active modal observation refuses checkpoint replacement".into());
        }
        if matches!(&request, Request::SnapshotSave { .. } | Request::SnapshotRestore { .. } | Request::SnapshotKey { .. }) {
            let traces = &self.bridge.machine.traces;
            if self.watch.is_some() || self.preservation.is_some() || traces.modal_observation_active()
                || traces.capture_observation_active() || self.bridge.rendezvous_active() {
                return Err("snapshot save and restore require no armed watch, preservation, capture, modal or rendezvous observation".into());
            }
        }
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
            Request::TraceSites { sites, reads } => {
                let site_reads = parse_site_reads(&sites, &reads)?;
                let count = self.arm(&sites)?;
                self.bridge.machine.traces.site_reads = site_reads;
                json!({"sites": count})
            }
            Request::TraceHits { from, limit } => {
                let hits = &self.bridge.machine.traces.hits;
                if from > hits.len() {
                    return Err("trace-hits requires a hit index within the recorded hits".into());
                }
                let end = limit.map_or(hits.len(), |limit| hits.len().min(from.saturating_add(limit.get())));
                json!({"count": hits.len(), "hits": &hits[from..end]})
            }
            Request::ModalObservationBegin(request) => {
                let machine = &mut self.bridge.machine;
                let config = request.into_config(machine.session.board.mappings().to_vec())?;
                if config.owns_memory_watch() && (self.watch.is_some() || self.preservation.is_some()) {
                    return Err("installed modal observation requires exclusive memory-watch ownership".into());
                }
                machine.traces.begin_modal_observation(&machine.session, config)?;
                serde_json::to_value(machine.traces.modal_observation_report())
                    .map_err(|failure| format!("modal-observation-begin: {failure}"))?
            }
            Request::ModalObservationReport => {
                let machine = &mut self.bridge.machine;
                if machine.traces.modal_observation_active() {
                    let instructions = InstructionCount::from_board(&machine.session)?;
                    let _ = machine.traces.collect(&machine.session, instructions);
                }
                serde_json::to_value(machine.traces.modal_observation_report())
                    .map_err(|failure| format!("modal-observation-report: {failure}"))?
            }
            Request::ModalObservationEnd => {
                let machine = &mut self.bridge.machine;
                let instructions = InstructionCount::from_board(&machine.session)?;
                machine.traces.collect(&machine.session, instructions)?;
                machine.traces.end_modal_observation(&machine.session)?;
                serde_json::to_value(machine.traces.modal_observation_report())
                    .map_err(|failure| format!("modal-observation-end: {failure}"))?
            }
            Request::CaptureObservationBegin { scope } => {
                self.bridge.begin_capture_observation(scope)?;
                json!({"protocol": 2, "active": true, "scope": scope, "bounds": CAPTURE_OBSERVATION_BOUNDS})
            }
            Request::CaptureObservationHits { from } => {
                self.collect_capture_tail();
                serde_json::to_value(self.bridge.machine.traces.capture_observation_page(from).map_err(|error| error.to_string())?)
                    .map_err(|error| format!("capture-observation-hits: {error}"))?
            }
            Request::CaptureObservationAck { next } => {
                serde_json::to_value(self.bridge.machine.traces.acknowledge_capture_observation(next)
                    .map_err(|error| error.to_string())?).map_err(|error| error.to_string())?
            }
            Request::CaptureObservationResume => {
                self.bridge.machine.require_running()?;
                if !self.bridge.machine.traces.capture_drained() {
                    return Err("capture resume requires a drained boundary-complete snapshot".into());
                }
                let pending = self.pending_capture.take().ok_or("no capture continuation is pending")?;
                self.continue_capture(pending)?
            }
            Request::CaptureObservationState => {
                self.collect_capture_tail();
                serde_json::to_value(capture_observation_state(&self.bridge.machine)?)
                    .map_err(|error| format!("capture-observation-state: {error}"))?
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
                        KeyPulse::instructions(hold, release)
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
                let settle = match self.bridge.settled_key(route, pulse, budget) {
                    Ok(settle) => settle,
                    Err(failure) => {
                        self.key_log.push(json!({"key": key, "seconds": started.elapsed().as_secs_f64(),
                            "instructions": self.bridge.machine.instructions() - before, "failure": failure}));
                        return Err(failure);
                    }
                };
                self.bridge.machine.require_running()?;
                self.key_log.push(json!({"key": key, "seconds": started.elapsed().as_secs_f64(),
                    "instructions": self.bridge.machine.instructions() - before, "settle": settle}));
                json!({"menu": menu_value(self.bridge.machine.menu()?), "settle": settle})
            }
            Request::ShutterSequence => {
                if self.bridge.machine.traces.capture_observation_active() {
                    return Ok(Some(self.start_capture_operation(CaptureOperation::ShutterSequence)?));
                }
                let phases = [
                    shutter_phase(&mut self.bridge, PhysicalShutterPosition::Half, GuestInstructions(GPIO_HOLD))?,
                    shutter_phase(&mut self.bridge, PhysicalShutterPosition::Full, GuestInstructions(GPIO_HOLD))?,
                    shutter_phase(&mut self.bridge, PhysicalShutterPosition::Half, GuestInstructions(GPIO_PULSE.release / 2))?,
                    shutter_phase(&mut self.bridge, PhysicalShutterPosition::Released, GuestInstructions(GPIO_PULSE.release / 2))?,
                ];
                if !self.bridge.machine.traces.capture_observation_active() {
                    self.key_log.push(json!({"physical_shutter_sequence": phases}));
                }
                json!({"phases": phases})
            }
            Request::ImageInputFingerprint { node } => {
                let machine = &self.bridge.machine;
                let fingerprint = machine.session.board.inspect_device::<ImageInput, _>(
                    machine.session.node(&node.0)?,
                    |device| device.fingerprint(),
                )?;
                json!({"instructions": machine.instructions(), "fingerprint": fingerprint})
            }
            Request::CaptureObservationWindow => {
                if self.bridge.machine.traces.capture_observation_active() {
                    return Ok(Some(self.start_capture_operation(CaptureOperation::Window)?));
                }
                self.bridge.advance(CAPTURE_OBSERVATION_WINDOW.0)?;
                json!({"instructions": GuestInstructions(self.bridge.machine.instructions())})
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
            Request::NandRead { first, count } => {
                let read = self.bridge.machine.read_nand(first, count)?;
                json!({"first": read.first.0, "count": read.count.get(), "data": encode_hex(&read.data), "spare": encode_hex(&read.spare)})
            }
            Request::WatchBegin { ranges, limit, views, write_ranges, fetch_ranges } => {
                if self.bridge.machine.traces.modal_observation_active() {
                    return Err("active modal observation refuses memory-watch replacement".into());
                }
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
                match views {
                    WatchViews::Observe if !write_ranges.is_empty() || !fetch_ranges.is_empty() =>
                        return Err("classified write/fetch ranges require fixed views".into()),
                    WatchViews::Fixed => {
                        let mut classes: Vec<_> = write_ranges.iter().chain(&fetch_ranges).copied().collect();
                        if classes.is_empty() || classes.len() > 16
                            || classes.iter().any(|range| range[0] >= range[1] || range[1] > u32::MAX as u64) {
                            return Err("fixed watch requires nonempty bounded write/fetch classes".into());
                        }
                        classes.sort_unstable();
                        let mut union: Vec<[u64; 2]> = Vec::with_capacity(classes.len());
                        for range in classes {
                            match union.last_mut() {
                                Some(previous) if range[0] <= previous[1] => previous[1] = previous[1].max(range[1]),
                                _ => union.push(range),
                            }
                        }
                        if union != ranges {
                            return Err("fixed watch ranges differ from the write/fetch class union".into());
                        }
                    }
                    WatchViews::Observe => {}
                }
                let maps = run::memory_maps(&self.bridge.machine.document)?;
                let watch = match views {
                    WatchViews::Observe => self.bridge.machine.session.board.memory_watch_begin(
                        ranges.iter().map(|range| (range[0], range[1])).collect(), limit, maps)?,
                    WatchViews::Fixed => self.bridge.machine.session.board.memory_watch_begin_classified(
                        write_ranges.into_iter().map(|range| WatchRange { start: range[0], end: range[1], kinds: WatchKinds::WRITES })
                            .chain(fetch_ranges.into_iter().map(|range| WatchRange { start: range[0], end: range[1], kinds: WatchKinds::CPU_FETCH })).collect(),
                        limit, maps, ViewPolicy::Fixed)?,
                };
                let acknowledgement = match views {
                    WatchViews::Observe => json!({"armed": true, "ranges": ranges}),
                    WatchViews::Fixed => {
                        let state = watch.records.borrow().state();
                        json!({"armed": true, "ranges": ranges, "views": "fixed", "classes": state["classes"]})
                    }
                };
                self.watch = Some(watch);
                acknowledgement
            }
            Request::WatchState => match &self.watch {
                Some(watch) => watch.records.borrow().state(),
                None => match self.bridge.machine.traces.modal_watch_state() {
                    Some(state) => state?,
                    None => return Err("watch-state requires an armed memory watch".into()),
                },
            },
            Request::WatchEnd => {
                let mut watch = self.watch.take().ok_or("watch-end requires an armed memory watch")?;
                let state = watch.records.borrow().state();
                self.bridge.machine.session.board.memory_watch_end(&mut watch)?;
                state
            }
            Request::PreservationBegin { ranges } => {
                if self.bridge.machine.traces.modal_observation_active() {
                    return Err("active modal observation refuses preservation replacement".into());
                }
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
                let capture = self.bridge.machine.capture_frame()?;
                let directory = self.output.join("screenshots");
                std::fs::create_dir_all(&directory).map_err(|e| e.to_string())?;
                let path = directory.join(format!("{label}.png"));
                let capture = capture.write(&path)?;
                let shot = json!({"label": label, "path": capture.png.path, "frame_number": null,
                    "sha256": capture.png.sha256, "scanout_evidence": capture.scanout_evidence,
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
            Request::SnapshotSave { name } => {
                if self.snapshots.contains_key(&name) {
                    return Err(format!("snapshot {} is already held", name.0));
                }
                if self.snapshots.len() >= SNAPSHOT_LIMIT {
                    return Err(format!("at most {SNAPSHOT_LIMIT} snapshots are held"));
                }
                let machine = &mut self.bridge.machine;
                machine.disarm()?;
                let exported = machine.session.checkpoint(None).and_then(|snapshot| snapshot.export());
                machine.rearm()?;
                let bytes = exported?;
                let response = json!({"name": name, "bytes": bytes.len(), "instructions": machine.instructions(),
                    "menu": menu_value(machine.menu()?)});
                self.snapshots.insert(name, bytes);
                response
            }
            Request::SnapshotRestore { name } => {
                let cleared_failure = self.restore_snapshot(&name)?;
                let machine = &self.bridge.machine;
                json!({"snapshot": name, "instructions": machine.instructions(), "menu": menu_value(machine.menu()?),
                    "cleared_failure": cleared_failure})
            }
            Request::SnapshotKey { name, key } => {
                let cleared_failure = self.restore_snapshot(&name)?;
                let mut response = self.display_press(&key, Some(&name))?;
                response["cleared_failure"] = json!(cleared_failure);
                response
            }
            Request::DisplayKey { key } => self.display_press(&key, None)?,
            Request::Idle { instructions: GuestInstructions(instructions) } => {
                if !(1..=MAX_SETTLE).contains(&instructions) {
                    return Err(format!("idle instructions must be 1..{MAX_SETTLE}"));
                }
                let before = self.bridge.machine.instructions();
                self.bridge.advance(instructions)?;
                self.bridge.machine.require_running()?;
                let after = self.bridge.machine.instructions();
                json!({"instructions": GuestInstructions(after), "ran": GuestInstructions(after - before)})
            }
            Request::SnapshotDrop { name } => {
                self.snapshots.remove(&name).ok_or_else(|| format!("snapshot {} is not held", name.0))?;
                json!({"name": name, "held": self.snapshots.len()})
            }
            Request::Finish => {
                if self.bridge.machine.traces.modal_observation_active() {
                    let machine = &mut self.bridge.machine;
                    let instructions = InstructionCount::from_board(&machine.session)?;
                    machine.traces.collect(&machine.session, instructions)?;
                    machine.traces.end_modal_observation(&machine.session)?;
                }
                self.collect_capture_tail();
                return Ok(None);
            }
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
            "capture_evidence_error": self.capture_evidence_error,
            "key_log": self.key_log,
            "transfer_log": self.bridge.transfer_log,
            "transfer_profile": self.bridge.profile,
            "request_profile": self.requests,
            "rendezvous_activity": self.bridge.rendezvous,
            "keepalive_inputs": self.bridge.keepalive_inputs,
            "contexts": self.bridge.machine.traces.hits,
            "modal_observation": self.bridge.machine.traces.modal_observation_report(),
            "screenshots": self.screenshots,
            "memory_watch": self.watch.as_ref().map(|watch| watch.records.borrow().state()).unwrap_or(Value::Null),
            "preservation": preservation,
            "sd_host": self.bridge.machine.card_state().unwrap_or_else(|error| json!({"error": error})),
            "boundary": {"instructions": self.bridge.machine.instructions(), "failure": self.bridge.machine.failure},
        }))
    }

    fn serve(&mut self) -> Result<(), String> {
        self.serve_io(std::io::stdin().lock(), std::io::stdout().lock())
    }

    fn serve_io(&mut self, input: impl BufRead, mut output: impl Write) -> Result<(), String> {
        let mut emit = |value: &Value| -> Result<(), String> {
            serde_json::to_writer(&mut output, value).map_err(|error| error.to_string())?;
            output.write_all(b"\n").and_then(|()| output.flush()).map_err(|error| error.to_string())
        };
        emit(&json!({"ok": true, "event": "ready", "device": encode_hex(&self.attachment.device),
            "configuration": encode_hex(&self.attachment.configuration),
            "pre_attach": self.attachment.pre_attach, "checkpoint": self.checkpoint}))?;
        let mut error = None;
        let mut waiting = Instant::now();
        for line in input.lines() {
            let line = match line {
                Ok(line) => line,
                Err(io_error) => {
                    if error.is_some() {
                        self.capture_evidence_error.get_or_insert(io_error.to_string());
                    } else {
                        error = Some(io_error.to_string());
                    }
                    break;
                }
            };
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
            let (response, finished) = match result {
                Ok(Some(mut value)) => {
                    value.as_object_mut().ok_or("response must be an object")?.insert("ok".into(), json!(true));
                    (value, false)
                }
                Ok(None) => {
                    let response = match &error {
                        Some(message) => json!({"ok": false, "error": "BridgeError", "message": message, "finished": true}),
                        None => json!({"ok": true, "finished": true}),
                    };
                    (response, true)
                }
                Err(message) => {
                    let response = json!({"ok": false, "error": "BridgeError", "message": message});
                    if error.is_some() {
                        self.capture_evidence_error.get_or_insert(message);
                    } else if self.bridge.machine.failure.is_some()
                        || !(self.bridge.machine.traces.capture_observation_active()
                            || self.bridge.machine.traces.modal_observation_active()) {
                        error = Some(message);
                    }
                    let observing = self.bridge.machine.traces.capture_observation_active()
                        || self.bridge.machine.traces.modal_observation_active();
                    (response, !observing)
                }
            };
            if let Err(io_error) = emit(&response) {
                if error.is_some() {
                    self.capture_evidence_error.get_or_insert(io_error);
                } else {
                    error = Some(io_error);
                }
                break;
            }
            if finished { break; }
            waiting = Instant::now();
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
    let record_bytes = arguments.record_nand.as_deref().map(read).transpose()?;
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
        records: record_bytes.as_deref().map(RecordMedium::parse).transpose()?,
        card,
    };
    std::fs::create_dir_all(&arguments.output).map_err(|e| format!("{}: {e}", arguments.output.display()))?;
    let observation = arguments.reset_observation.as_deref().map(|path| ResetObservationSpec::parse(&read(path)?)).transpose()?;
    let key = boot_key(&arguments, &inputs, &adjustment_bytes, record_bytes.as_deref())?;
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
    let mut options = json!({"ok": true, "event": "start-options", "boot_key": key.key, "installed": listed});
    if let Some(records) = &inputs.records {
        options["record_nand"] = records.summary();
    }
    emit(&options)?;
    let mut line = String::new();
    std::io::stdin().lock().read_line(&mut line).map_err(|e| e.to_string())?;
    let start: Start = serde_json::from_str(&line).map_err(|e| format!("RAM-push start: {e}"))?;
    let attach_directory = root.join(&key.key.0);
    let restore_started = Instant::now();
    let (bridge, attachment, checkpoint) = match start.installed {
        Some(_) if observation.is_some() => return Err("a reset observation starts from no install checkpoint".into()),
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
            let mut machine = inputs.machine()?;
            let mut observer = observation.as_ref().map(|spec| ResetObserver::begin(&mut machine, spec)).transpose()?;
            let (mut bridge, attachment) = attach(machine, rendezvous, arguments.attach_hold, observer.as_mut())?;
            let boot_seconds = boot_started.elapsed().as_secs_f64();
            let start = if arguments.fresh_boot { StartKind::FreshBoot } else { StartKind::Cold };
            let checkpoint = match observer {
                Some(observer) => {
                    let path = arguments.output.join(RESET_OBSERVATION_RECORD);
                    let bytes = serde_json::to_vec(&observer.finish()?).map_err(|error| format!("reset observation record: {error}"))?;
                    write(&path, &bytes)?;
                    json!({"key": key.key, "inputs": key.inputs, "directory": attach_directory,
                        "key_seconds": key_seconds, "start": start, "boot_seconds": boot_seconds, "saved": false,
                        "reset_observation": {"path": path, "sha256": sha256(&bytes)}})
                }
                None => {
                    let save_started = Instant::now();
                    let record = json!({"schema": CHECKPOINT_SCHEMA, "key": key.key, "inputs": key.inputs,
                        "pre_attach": attachment.pre_attach, "device": encode_hex(&attachment.device),
                        "configuration": encode_hex(&attachment.configuration)});
                    save_checkpoint(&mut bridge, &root, &key.key.0, ATTACH_RECORD, &record)?;
                    json!({"key": key.key, "inputs": key.inputs, "directory": attach_directory,
                        "key_seconds": key_seconds, "start": start,
                        "boot_seconds": boot_seconds, "saved": true, "save_seconds": save_started.elapsed().as_secs_f64()})
                }
            };
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
        pending_capture: None,
        capture_evidence_error: None,
        snapshots: BTreeMap::new(),
    };
    server.arm_sites(observation.as_ref().map_or(&BTreeMap::new(), ResetObservationSpec::sites))?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use fujimod_xm1::run::{RunDocument, ValidatedUpdate};
    use std::sync::atomic::{AtomicU64, Ordering};

    #[test]
    fn nand_read_request_validates_row_count() {
        let request: Request = serde_json::from_str(r#"{"command":"nand-read","first":60160,"count":64}"#).unwrap();
        assert!(matches!(request, Request::NandRead { first: RowAddress(60160), count } if count.get() == 64));
        for count in [0, 65] {
            assert!(serde_json::from_str::<Request>(&format!(r#"{{"command":"nand-read","first":0,"count":{count}}}"#)).is_err());
        }
    }

    #[derive(Debug, Deserialize, Eq, PartialEq)]
    #[serde(rename_all = "kebab-case")]
    enum ProgressState {
        Completed,
        DrainRequired,
    }

    #[derive(Deserialize)]
    struct Phase {
        position: PhysicalShutterPosition,
        started: u64,
        finished: u64,
    }

    #[derive(Debug, Deserialize, Eq, PartialEq)]
    struct ObservedHit {
        sequence: CaptureCursor,
        instructions: u64,
    }

    #[derive(Deserialize)]
    struct Snapshot {
        instructions: u64,
    }

    #[derive(Deserialize)]
    struct Response {
        ok: bool,
        message: Option<String>,
        protocol: Option<u32>,
        state: Option<ProgressState>,
        armed: Option<bool>,
        enabled: Option<bool>,
        views: Option<WatchViews>,
        current_phase: Option<u8>,
        instructions: Option<u64>,
        intended_endpoint: Option<u64>,
        next: Option<CaptureCursor>,
        from: Option<CaptureCursor>,
        acknowledged: Option<CaptureCursor>,
        pending: Option<usize>,
        #[serde(default)]
        hits: Vec<ObservedHit>,
        snapshot: Option<Snapshot>,
        boundary_complete: Option<bool>,
        finished: Option<bool>,
        phases: Option<Vec<Phase>>,
        path: Option<PathBuf>,
        scanout_evidence: Option<ScanoutEvidenceReceipt>,
    }

    #[derive(Deserialize)]
    struct Boundary {
        instructions: u64,
        failure: Option<String>,
    }

    #[derive(Deserialize)]
    struct Screenshot {
        path: PathBuf,
        scanout_evidence: ScanoutEvidenceReceipt,
    }

    #[derive(Debug, Deserialize, Eq, PartialEq)]
    struct ArtifactReceipt {
        path: PathBuf,
        sha256: String,
        bytes: usize,
    }

    #[derive(Debug, Deserialize, Eq, PartialEq)]
    struct ScanoutEvidenceReceipt {
        metadata: ArtifactReceipt,
        source_bytes: ArtifactReceipt,
    }

    #[derive(Deserialize)]
    struct CaptureMetadataReceipt {
        schema: u32,
        node: String,
        instance: usize,
        instructions: u64,
        pixels: CapturedPixelsReceipt,
        png: ArtifactReceipt,
        source_bytes: ArtifactReceipt,
        source_planes: Vec<SourcePlaneReceipt>,
        native_scanout_state: NativeScanoutReceipt,
        native_frame_state: NativeFrameReceipt,
    }

    #[derive(Deserialize)]
    struct CapturedPixelsReceipt {
        width: usize,
        height: usize,
        rgb_bytes: usize,
        rgb_sha256: String,
    }

    #[derive(Debug, Deserialize, Eq, PartialEq)]
    #[serde(tag = "kind", rename_all = "kebab-case")]
    enum PlaneKindReceipt {
        Main,
        Graphics { layer: usize, channel: usize },
    }

    #[derive(Deserialize)]
    struct SourcePlaneReceipt {
        #[serde(flatten)]
        kind: PlaneKindReceipt,
        native_source_address: u64,
        offset: usize,
        bytes: usize,
    }

    #[derive(Deserialize)]
    struct NativeScanoutReceipt {
        instances: Vec<NativeInstanceReceipt>,
    }

    #[derive(Deserialize)]
    struct NativeInstanceReceipt {
        instance: usize,
        registers: BTreeMap<String, String>,
        written_offsets: Vec<String>,
    }

    #[derive(Deserialize)]
    struct NativeFrameReceipt {
        instance: usize,
        width: usize,
        height: usize,
        graphics: Vec<NativeGraphicsReceipt>,
        rectangles: Vec<NativeRectangleReceipt>,
    }

    #[derive(Deserialize)]
    struct NativeGraphicsReceipt {
        source: String,
        width: usize,
        height: usize,
        origin: [usize; 2],
        stride_bytes: usize,
        captured_bytes: usize,
        alpha_field: usize,
    }

    #[derive(Deserialize)]
    struct NativeRectangleReceipt {
        index: usize,
        luma: usize,
    }

    #[derive(Debug, Deserialize, Eq, PartialEq)]
    #[serde(transparent)]
    struct ModelTicks(u64);

    #[derive(Debug, Deserialize, Eq, PartialEq)]
    #[serde(transparent)]
    struct InstructionPhase(u64);

    #[derive(Debug, Deserialize, Eq, PartialEq)]
    struct StoppedTimeline {
        now: ModelTicks,
        phase: InstructionPhase,
    }

    #[derive(Debug, Deserialize, Eq, PartialEq)]
    struct ModelClock {
        timeline: StoppedTimeline,
    }

    #[derive(Deserialize)]
    struct Receipt {
        bridge_error: Option<String>,
        capture_evidence_error: Option<String>,
        boundary: Boundary,
        screenshots: Vec<Screenshot>,
    }

    fn server(program: &[u32]) -> Server {
        static SERIAL: AtomicU64 = AtomicU64::new(0);
        fujimod_emulator_exr2_board::register_board_builder();
        fujimod_emulator_models::register();
        let document = RunDocument::parse(include_bytes!("../data/xm1-150.json")).unwrap();
        let mut rom = vec![0; 16384];
        for (index, word) in program.iter().enumerate() {
            rom[index * 4..index * 4 + 4].copy_from_slice(&word.to_le_bytes());
        }
        let update = ValidatedUpdate(vec![0xff; document.update_flash_offset + 2048]);
        let mut session = run::open(&document, &update, &rom).unwrap();
        session.board.set_execution_policy(document.execution_policy.into()).unwrap();
        let machine = Machine::new(document, session).unwrap();
        let table = &machine.document.firmware.key_table;
        for (code, binding, interrupt) in [
            (7u32, &machine.document.firmware.shutter.half, 31u16),
            (8, &machine.document.firmware.shutter.full, 33),
        ] {
            let mut descriptor = [0; 28];
            descriptor[..4].copy_from_slice(&code.to_le_bytes());
            descriptor[9] = 4;
            descriptor[12..16].copy_from_slice(&u32::from(binding.pad.pin).to_le_bytes());
            descriptor[16..18].copy_from_slice(&1u16.to_le_bytes());
            descriptor[18..20].copy_from_slice(&interrupt.to_le_bytes());
            machine.session.board.write_physical(table.base.0 + binding.row.get() * table.row_bytes, &descriptor).unwrap();
        }
        let output = std::env::temp_dir().join(format!("capture-stream-{}-{}", std::process::id(), SERIAL.fetch_add(1, Ordering::Relaxed)));
        Server {
            bridge: Ff80::new(machine, RendezvousInputs::MenuUp),
            attachment: Attachment { pre_attach: Value::Null, device: Vec::new(), configuration: Vec::new() },
            checkpoint: Value::Null,
            key: BootKey { key: CheckpointKey("synthetic-capture".into()), inputs: Value::Null },
            root: output.clone(), output,
            screenshots: Vec::new(), watch: None, preservation: None,
            key_log: Vec::new(), requests: RequestProfile::default(),
            rendezvous: RendezvousInputs::MenuUp, pending_capture: None,
            capture_evidence_error: None, snapshots: BTreeMap::new(),
        }
    }

    fn exchange(server: &mut Server, input: &str) -> (Vec<Response>, Receipt) {
        exchange_bytes(server, input.as_bytes())
    }

    fn exchange_bytes(server: &mut Server, input: &[u8]) -> (Vec<Response>, Receipt) {
        let mut output = Vec::new();
        server.serve_io(input, &mut output).unwrap();
        let responses = output.split(|byte| *byte == b'\n').filter(|line| !line.is_empty())
            .map(|line| serde_json::from_slice(line).unwrap()).collect();
        let receipt = serde_json::from_slice(&std::fs::read(server.output.join("emulator-session.json")).unwrap()).unwrap();
        std::fs::remove_dir_all(&server.output).unwrap();
        (responses, receipt)
    }

    fn request(value: Value) -> Request {
        serde_json::from_value(value).unwrap()
    }

    #[test]
    fn snapshot_key_rewinds_to_the_saved_guest_and_clears_a_terminal_stop() {
        let mut server = server(&[0xe2800001, 0xeafffffd]);
        assert_eq!(server.bridge.machine.run(1000), RunOutcome::Completed);
        let saved = server.handle(request(json!({"command": "snapshot-save", "name": "n0"}))).unwrap().unwrap();
        let at_save = saved["instructions"].as_u64().unwrap();
        assert_eq!(server.bridge.machine.run(1000), RunOutcome::Completed);
        server.bridge.machine.failure = Some("synthetic terminal fault".into());
        assert!(server.handle(request(json!({"command": "snapshot-save", "name": "n1"}))).is_err());
        let pressed = server.handle(request(json!({"command": "snapshot-key", "name": "n0", "key": "up"}))).unwrap().unwrap();
        assert_eq!(pressed["cleared_failure"], "synthetic terminal fault");
        assert_eq!(server.bridge.machine.instructions(), at_save + pressed["instructions"].as_u64().unwrap());
        let again = server.handle(request(json!({"command": "snapshot-key", "name": "n0", "key": "up"}))).unwrap().unwrap();
        assert_eq!(again["instructions"], pressed["instructions"]);
        let restored = server.handle(request(json!({"command": "snapshot-restore", "name": "n0"}))).unwrap().unwrap();
        assert_eq!(restored["instructions"].as_u64(), Some(at_save));
        assert_eq!(server.bridge.machine.instructions(), at_save);
        server.handle(request(json!({"command": "snapshot-drop", "name": "n0"}))).unwrap();
        assert!(server.handle(request(json!({"command": "snapshot-key", "name": "n0", "key": "up"}))).is_err());
    }

    #[test]
    fn a_stopped_guest_answers_trace_hits_but_no_other_request_until_restored() {
        let mut server = server(&[0xeafffffe]);
        server.handle(request(json!({"command": "snapshot-save", "name": "n0"}))).unwrap();
        server.bridge.machine.failure = Some("synthetic terminal fault".into());
        assert!(server.handle(request(json!({"command": "trace-hits", "from": 0}))).is_ok());
        assert!(server.handle(request(json!({"command": "read-memory", "address": 0, "count": 4}))).is_err());
        let restored = server.handle(request(json!({"command": "snapshot-restore", "name": "n0"}))).unwrap().unwrap();
        assert_eq!(restored["cleared_failure"], "synthetic terminal fault");
        assert!(server.handle(request(json!({"command": "read-memory", "address": 0, "count": 4}))).is_ok());
    }

    #[test]
    fn snapshots_refuse_an_armed_watch() {
        let mut server = server(&[0xeafffffe]);
        server.handle(request(json!({"command": "watch-begin", "ranges": [[0x01467588u64, 0x01467840u64]], "limit": 16}))).unwrap();
        assert!(server.handle(request(json!({"command": "snapshot-save", "name": "n0"}))).is_err());
    }

    #[test]
    fn snapshot_requests_refuse_malformed_names_and_keys() {
        assert!(serde_json::from_value::<Request>(json!({"command": "snapshot-save", "name": "N0"})).is_err());
        assert!(serde_json::from_value::<Request>(json!({"command": "snapshot-save", "name": ""})).is_err());
        assert!(serde_json::from_value::<Request>(json!({"command": "snapshot-key", "name": "n0", "key": "shutter"})).is_err());
    }

    #[test]
    fn fixed_watch_protocol_accepts_a_write_only_class() {
        let mut server = server(&[0xeafffffe]);
        let begin = json!({
            "command": "watch-begin", "ranges": [[0x01467588u64, 0x01467840u64]],
            "limit": 16, "views": "fixed",
            "write_ranges": [[0x01467588u64, 0x01467840u64]], "fetch_ranges": []
        });
        let (responses, receipt) = exchange(&mut server,
            &format!("{begin}\n{{\"command\":\"watch-state\"}}\n{{\"command\":\"finish\"}}\n"));
        assert!(receipt.bridge_error.is_none());
        assert!(responses.iter().all(|response| response.ok));
        assert!(responses.iter().any(|response|
            response.armed == Some(true) && response.views == Some(WatchViews::Fixed)));
        assert!(responses.iter().any(|response|
            response.enabled == Some(true) && response.views == Some(WatchViews::Fixed)));
    }

    #[test]
    fn fixed_watch_protocol_refuses_absent_write_and_fetch_classes() {
        let mut server = server(&[0xeafffffe]);
        let begin = json!({
            "command": "watch-begin", "ranges": [[0x01467588u64, 0x01467840u64]],
            "limit": 16, "views": "fixed", "write_ranges": [], "fetch_ranges": []
        });
        let (responses, receipt) = exchange(&mut server, &format!("{begin}\n"));
        assert!(receipt.bridge_error.is_some());
        assert!(responses.last().is_some_and(|response| !response.ok));
        assert!(!responses.iter().any(|response| response.armed == Some(true)));
    }

    #[test]
    fn fixed_watch_protocol_records_guest_fetch_at_its_exact_pc() {
        let mut server = server(&[0xe3a00001, 0xeafffffe]);
        let physical = server.bridge.machine.document.boot_rom.address.0;
        assert_eq!(physical, 0x8000_0000);
        let mut reset_instruction = [0; 4];
        server.bridge.machine.session.board.read_memory(0, 0, &mut reset_instruction).unwrap();
        assert_eq!(reset_instruction, 0xe3a00001u32.to_le_bytes());
        let begin = serde_json::from_value(json!({
            "command": "watch-begin", "ranges": [[physical, physical + 4]], "limit": 16,
            "views": "fixed", "fetch_ranges": [[physical, physical + 4]]
        })).unwrap();
        let acknowledgement = server.handle(begin).unwrap().unwrap();
        assert_eq!(acknowledgement["views"], "fixed");
        assert_eq!(server.bridge.machine.run(1), RunOutcome::Completed);
        let state = server.handle(Request::WatchState).unwrap().unwrap();
        assert_eq!(state["enabled"], true);
        assert_eq!(state["views"], "fixed");
        assert!(state["violation"].is_null());
        assert!(state["records"].as_array().unwrap().iter().any(|record|
            record["source"] == "cpu-fetch" && record["address"] == 0 && record["pc"] == 0 && record["physical"] == physical));
        server.bridge.machine.failure = Some("synthetic terminal fault".into());
        let final_state = server.handle(Request::WatchState).unwrap().unwrap();
        assert_eq!(final_state, state);
    }

    const DRAIN_FIRST_PAUSE: &str = concat!(
        "{\"command\":\"trace-sites\",\"sites\":{\"0x00000004\":\"loop\"}}\n",
        "{\"command\":\"capture-observation-begin\"}\n",
        "{\"command\":\"shutter-sequence\"}\n",
        "{\"command\":\"capture-observation-resume\"}\n",
        "{\"command\":\"capture-observation-hits\",\"from\":0}\n",
        "{\"command\":\"capture-observation-ack\",\"next\":64}\n",
        "{\"command\":\"capture-observation-hits\",\"from\":64}\n",
        "{\"command\":\"capture-observation-ack\",\"next\":65}\n",
        "{\"command\":\"capture-observation-resume\"}\n",
        "{\"command\":\"capture-observation-hits\",\"from\":65}\n",
        "{\"command\":\"capture-observation-resume\"}\n",
    );

    #[test]
    fn shutter_resume_retains_contacts_phase_observations_and_the_fixed_24m_endpoint() {
        let mut server = server(&[0xe3a00000, 0xe2800001, 0xe3500041, 0x9afffffc, 0xeafffffe]);
        let (responses, receipt) = exchange(&mut server, &format!("{DRAIN_FIRST_PAUSE}{{\"command\":\"finish\"}}\n"));
        assert_eq!(responses.iter().find_map(|response| response.protocol), Some(2));
        let pause = responses.iter().find(|response| response.state == Some(ProgressState::DrainRequired)).unwrap();
        assert_eq!(pause.current_phase, Some(0));
        assert_eq!(pause.intended_endpoint, Some(8_000_000));
        assert!(pause.instructions.unwrap() < 8_000_000);
        assert_eq!(responses.iter().filter(|response| !response.ok).count(), 2);
        let completed = responses.iter().find(|response| response.state == Some(ProgressState::Completed)).unwrap();
        assert_eq!(completed.instructions, Some(24_000_000));
        assert_eq!(completed.intended_endpoint, completed.instructions);
        let phases = completed.phases.as_ref().unwrap();
        assert_eq!(phases.iter().map(|phase| (phase.started, phase.finished)).collect::<Vec<_>>(),
            [(0, 8_000_000), (8_000_000, 16_000_000), (16_000_000, 20_000_000), (20_000_000, 24_000_000)]);
        assert_eq!(phases.iter().map(|phase| phase.position).collect::<Vec<_>>(),
            [PhysicalShutterPosition::Half, PhysicalShutterPosition::Full, PhysicalShutterPosition::Half, PhysicalShutterPosition::Released]);
        assert!(responses.last().unwrap().ok);
        assert_eq!(responses.last().unwrap().finished, Some(true));
        assert!(receipt.bridge_error.is_none());
        assert!(receipt.boundary.failure.is_none());
    }

    #[test]
    fn window_resume_completes_at_the_fixed_40m_endpoint() {
        let mut server = server(&[0xe3a00000, 0xe2800001, 0xe3500041, 0x9afffffc, 0xeafffffe]);
        let script = DRAIN_FIRST_PAUSE.replace("\"shutter-sequence\"", "\"capture-observation-window\"");
        let (responses, receipt) = exchange(&mut server, &format!("{script}{{\"command\":\"finish\"}}\n"));
        let pause = responses.iter().find(|response| response.state == Some(ProgressState::DrainRequired)).unwrap();
        assert_eq!(pause.intended_endpoint, Some(40_000_000));
        assert!(pause.current_phase.is_none());
        let completed = responses.iter().find(|response| response.state == Some(ProgressState::Completed)).unwrap();
        assert_eq!(completed.instructions, Some(40_000_000));
        assert_eq!(completed.intended_endpoint, completed.instructions);
        assert!(completed.phases.is_none());
        assert_eq!(receipt.boundary.instructions, 40_000_000);
        assert!(receipt.bridge_error.is_none());
    }

    #[test]
    fn terminal_eof_and_invalid_evidence_input_preserve_the_original_strict_error() {
        for malformed_input in [false, true] {
            let mut server = server(&[0xe3a00000, 0xe2800001, 0xe3500041, 0x9afffffc, 0xe3a01006, 0xe3a02003, 0xe710f211]);
            let mut input = DRAIN_FIRST_PAUSE.as_bytes().to_vec();
            if malformed_input { input.extend_from_slice(b"\xff\n"); }
            let (responses, receipt) = exchange_bytes(&mut server, &input);
            assert!(receipt.bridge_error.is_some());
            assert!(responses.iter().any(|response| !response.ok && response.message == receipt.bridge_error));
            assert_eq!(receipt.capture_evidence_error.is_some(), malformed_input);
            assert!(responses.iter().all(|response| response.finished.is_none()));
        }
    }

    #[test]
    fn strict_failure_allows_tail_journaling_and_finish_without_overwriting_the_original_error() {
        let mut server = server(&[0xe3a00000, 0xe2800001, 0xe3500041, 0x9afffffc, 0xe3a01006, 0xe3a02003, 0xe710f211]);
        let input = format!("{DRAIN_FIRST_PAUSE}{}\n{}\n{}\n{}\n",
            r#"{"command":"capture-observation-hits","from":65}"#,
            r#"{"command":"capture-observation-ack","next":67}"#,
            r#"{"command":"capture-observation-ack","next":66}"#,
            r#"{"command":"capture-observation-hits","from":66}"#);
        let (responses, receipt) = exchange(&mut server, &format!("{input}{}\n{}\n",
            r#"{"command":"capture","label":"after-failure"}"#,
            r#"{"command":"finish"}"#));
        let terminal = responses.iter().find(|response| !response.ok && response.message.as_ref() == receipt.bridge_error.as_ref()).unwrap();
        assert!(receipt.boundary.failure.is_some());
        let tail = responses.iter().rev().find(|response| response.next.is_some()).unwrap();
        assert_eq!(tail.next, Some(CaptureCursor::new(66)));
        assert_eq!(tail.boundary_complete, Some(true));
        let finish = responses.last().unwrap();
        assert!(!finish.ok);
        assert_eq!(finish.finished, Some(true));
        assert_eq!(finish.message, terminal.message);
        let refused = &responses[responses.len() - 2];
        assert!(!refused.ok);
        assert_eq!(refused.message, receipt.boundary.failure);
        assert!(refused.path.is_none());
        assert!(receipt.screenshots.is_empty());
        assert!(receipt.capture_evidence_error.is_some());
        assert_eq!(receipt.boundary.instructions, server.bridge.machine.instructions());
        assert!(server.bridge.machine.instructions() < 8_000_000);
    }

    #[test]
    fn active_capture_writes_the_observed_png_without_guest_or_modeled_time_advancement() {
        let mut server = server(&[0xe5810000, 0xeafffffe]);
        let machine = &mut server.bridge.machine;
        for (offset, value) in [
            (0x00, 11 << 24),
            (0x10, 0x00010001),
            (0x84, 1),
            (0x90, 0x11),
            (0x94, 0),
            (0x98, 0),
            (0xa4, 0x00010001),
            (0xa8, 0x00010001),
            (0xb4, 0x2000),
            (0xb8, 0x3000),
            (0xc4, 0),
            (0xc8, 1),
            (0xe4, 0x8080),
            (0x108, 3),
            (0x11c, 0),
            (0x120, 0),
            (0x124, 0),
            (0x128, 0),
            (0x16c, 1),
            (0x174, 4),
            (0x178, 10),
            (0x17c, 20),
            (0x188, 0),
            (0x18c, 128),
            (0x190, 0),
        ] {
            machine.session.board.write_register(0, fujimod_emulator_cpu::Register::Pc, 0).unwrap();
            machine.session.board.write_register(0, fujimod_emulator_cpu::arm::CORE[0], value).unwrap();
            machine.session.board.write_register(0, fujimod_emulator_cpu::arm::CORE[1], 0xfff6b000 + offset).unwrap();
            machine.run(1);
            machine.require_running().unwrap();
        }
        machine.session.board.write_physical(0x4000_2000, &[255, 30, 60, 210].repeat(4)).unwrap();
        let lower_plane = [128, 40, 50, 60, 128, 70, 80, 90, 0, 0, 0, 0, 128, 10, 20, 30, 128, 50, 60, 70];
        machine.session.board.write_physical(0x4000_3000, &lower_plane).unwrap();
        let counts = machine.session.board.instruction_counts();
        let before: ModelClock = serde_json::from_value(machine.session.board.observation().unwrap()).unwrap();
        let mut output = Vec::new();
        let input = concat!(
            "{\"command\":\"capture-observation-begin\"}\n",
            "{\"command\":\"capture-observation-state\"}\n",
            "{\"command\":\"capture\",\"label\":\"final\"}\n",
            "{\"command\":\"capture-observation-state\"}\n",
            "{\"command\":\"finish\"}\n",
        );
        server.serve_io(input.as_bytes(), &mut output).unwrap();
        let responses: Vec<Response> = output.split(|byte| *byte == b'\n').filter(|line| !line.is_empty())
            .map(|line| serde_json::from_slice(line).unwrap()).collect();
        assert!(responses.iter().all(|response| response.ok));
        let captured = responses.iter().find(|response| response.path.is_some()).unwrap();
        let path = captured.path.as_ref().unwrap();
        let png = std::fs::read(path).unwrap();
        assert!(png.starts_with(b"\x89PNG\r\n\x1a\n"));
        assert_eq!(png.get(16..24), Some([0, 0, 0, 2, 0, 0, 0, 2].as_slice()));
        let pixels = fujimod_recording::Frame::from_png(&png).unwrap();
        assert_eq!(pixels.pixels(), &[20, 20, 20, 210, 60, 30, 210, 60, 30, 210, 60, 30]);
        let evidence = captured.scanout_evidence.as_ref().unwrap();
        let metadata_bytes = std::fs::read(&evidence.metadata.path).unwrap();
        assert_eq!(evidence.metadata.sha256, sha256(&metadata_bytes));
        assert_eq!(evidence.metadata.bytes, metadata_bytes.len());
        let metadata: CaptureMetadataReceipt = serde_json::from_slice(&metadata_bytes).unwrap();
        assert_eq!(metadata.schema, 1);
        assert_eq!(metadata.node, "scanout");
        assert_eq!(metadata.instance, 0);
        assert_eq!(metadata.instructions, server.bridge.machine.instructions());
        assert_eq!((metadata.pixels.width, metadata.pixels.height), (2, 2));
        assert_eq!(metadata.pixels.rgb_bytes, pixels.pixels().len());
        assert_eq!(metadata.pixels.rgb_sha256, sha256(pixels.pixels()));
        assert_eq!(&metadata.png.path, path);
        assert_eq!(metadata.png.sha256, sha256(&png));
        assert_eq!(metadata.png.bytes, png.len());
        assert_eq!(metadata.source_bytes, evidence.source_bytes);
        let raw = std::fs::read(&metadata.source_bytes.path).unwrap();
        assert_eq!(&raw[..20], &lower_plane);
        assert_eq!(&raw[20..], &[255, 30, 60, 210].repeat(4));
        assert_eq!(metadata.source_bytes.sha256, sha256(&raw));
        assert_eq!(metadata.source_bytes.bytes, raw.len());
        assert_eq!(metadata.source_planes.len(), 2);
        let lower = &metadata.source_planes[0];
        assert_eq!(lower.kind, PlaneKindReceipt::Graphics { layer: 1, channel: 1 });
        assert_eq!((lower.native_source_address, lower.offset, lower.bytes), (0x3000, 0, 20));
        let upper = &metadata.source_planes[1];
        assert_eq!(upper.kind, PlaneKindReceipt::Graphics { layer: 1, channel: 0 });
        assert_eq!((upper.native_source_address, upper.offset, upper.bytes), (0x2000, 20, 16));
        let native = &metadata.native_frame_state;
        assert_eq!((native.instance, native.width, native.height), (0, 2, 2));
        assert_eq!(native.graphics.len(), 2);
        let lower = &native.graphics[0];
        assert_eq!(lower.source, "0x3000");
        assert_eq!((lower.width, lower.height, lower.origin), (2, 2, [0, 0]));
        assert_eq!((lower.stride_bytes, lower.captured_bytes, lower.alpha_field), (12, 20, 128));
        let graphics = &native.graphics[1];
        assert_eq!(graphics.source, "0x2000");
        assert_eq!((graphics.width, graphics.height, graphics.origin), (2, 2, [0, 0]));
        assert_eq!((graphics.stride_bytes, graphics.captured_bytes, graphics.alpha_field), (8, 16, 128));
        assert_eq!(native.rectangles.iter().map(|rectangle| (rectangle.index, rectangle.luma)).collect::<Vec<_>>(), [(0, 10), (1, 20)]);
        let instance = metadata.native_scanout_state.instances.iter().find(|instance| instance.instance == metadata.instance).unwrap();
        assert_eq!(instance.registers.get("0xfff6b108").map(String::as_str), Some("0x3"));
        assert!(instance.written_offsets.iter().any(|offset| offset == "0x108"));
        let states: Vec<_> = responses.iter().filter_map(|response| response.snapshot.as_ref()).collect();
        assert_eq!(states.len(), 2);
        assert_eq!(states[0].instructions, states[1].instructions);
        assert_eq!(server.bridge.machine.session.board.instruction_counts(), counts);
        let after: ModelClock = serde_json::from_value(server.bridge.machine.session.board.observation().unwrap()).unwrap();
        assert_eq!(after, before);
        let receipt: Receipt = serde_json::from_slice(&std::fs::read(server.output.join("emulator-session.json")).unwrap()).unwrap();
        assert_eq!(receipt.screenshots.len(), 1);
        assert_eq!(&receipt.screenshots[0].path, path);
        assert_eq!(&receipt.screenshots[0].scanout_evidence, evidence);
        assert!(receipt.bridge_error.is_none());
        std::fs::remove_dir_all(&server.output).unwrap();
    }

    #[test]
    fn capture_without_source_evidence_refuses_a_screenshot_receipt() {
        let mut server = server(&[0xeafffffe]);
        let counts = server.bridge.machine.session.board.instruction_counts();
        let (responses, receipt) = exchange(&mut server, concat!(
            "{\"command\":\"capture\",\"label\":\"missing\"}\n",
            "{\"command\":\"finish\"}\n",
        ));
        let refusal = responses.iter().find(|response| !response.ok).unwrap();
        assert!(refusal.path.is_none());
        assert!(refusal.scanout_evidence.is_none());
        assert!(receipt.screenshots.is_empty());
        assert!(receipt.bridge_error.is_some());
        assert_eq!(server.bridge.machine.session.board.instruction_counts(), counts);
    }

    #[test]
    fn repeated_terminal_collection_failure_keeps_retained_pages_acknowledgements_and_state_available() {
        let mut server = server(&[0xe3a00000, 0xe2800001, 0xe3500041, 0x9afffffc, 0xeafffffe]);
        let prefix = DRAIN_FIRST_PAUSE.replacen(
            "\"0x00000004\":\"loop\"",
            &format!("\"0x00000004\":\"loop\",\"0x00000010\":\"{RENDEZVOUS_LOCKED}\""),
            1,
        );
        let input = format!("{prefix}{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n",
            r#"{"command":"capture-observation-state"}"#,
            r#"{"command":"capture-observation-hits","from":65}"#,
            r#"{"command":"capture-observation-hits","from":65}"#,
            r#"{"command":"capture-observation-ack","next":66}"#,
            r#"{"command":"capture-observation-state"}"#,
            r#"{"command":"capture-observation-hits","from":66}"#,
            r#"{"command":"capture-observation-hits","from":66}"#,
            r#"{"command":"capture-observation-ack","next":67}"#,
            r#"{"command":"finish"}"#);
        let (responses, receipt) = exchange(&mut server, &input);
        let retained: Vec<_> = responses.iter()
            .filter(|response| response.from == Some(CaptureCursor::new(65)) && !response.hits.is_empty()).collect();
        assert_eq!(retained.len(), 2);
        for response in &retained {
            assert!(response.ok);
            assert_eq!(response.next, Some(CaptureCursor::new(66)));
            assert_eq!(response.pending, Some(1));
            assert_eq!(response.boundary_complete, Some(false));
            assert_eq!(response.hits[0].sequence, CaptureCursor::new(65));
            assert!(response.hits[0].instructions <= receipt.boundary.instructions);
        }
        assert_eq!(retained[0].hits, retained[1].hits);
        let journaled: Vec<_> = [0, 64, 65].into_iter().flat_map(|from| {
            responses.iter().find(|response| response.from == Some(CaptureCursor::new(from)) && !response.hits.is_empty())
                .unwrap().hits.iter().map(|hit| hit.sequence)
        }).collect();
        assert_eq!(journaled, (0..66).map(CaptureCursor::new).collect::<Vec<_>>());
        let acknowledged = responses.iter().find(|response| response.from.is_none()
            && response.acknowledged == Some(CaptureCursor::new(66))).unwrap();
        assert!(acknowledged.ok);
        assert_eq!(acknowledged.pending, Some(0));
        let incomplete: Vec<_> = responses.iter().filter(|response| response.from == Some(CaptureCursor::new(66))).collect();
        assert_eq!(incomplete.len(), 2);
        for response in incomplete {
            assert!(response.ok);
            assert!(response.hits.is_empty());
            assert_eq!(response.pending, Some(0));
            assert_eq!(response.boundary_complete, Some(false));
        }
        let states: Vec<_> = responses.iter().filter(|response| response.snapshot.is_some()).collect();
        assert_eq!(states.len(), 2);
        for response in states {
            assert!(response.ok);
            assert_eq!(response.snapshot.as_ref().unwrap().instructions, receipt.boundary.instructions);
        }
        let terminal = &responses[prefix.lines().count()];
        assert!(!terminal.ok);
        assert_eq!(terminal.message, receipt.bridge_error);
        assert!(receipt.boundary.failure.is_some());
        assert_eq!(receipt.capture_evidence_error, receipt.boundary.failure);
        assert_ne!(receipt.bridge_error, receipt.capture_evidence_error);
        let finish = responses.last().unwrap();
        assert!(!finish.ok);
        assert_eq!(finish.finished, Some(true));
        assert_eq!(finish.message, receipt.bridge_error);
        assert!(receipt.boundary.instructions < 8_000_000);
    }

    #[test]
    fn watch_end_returns_the_final_state_and_releases_snapshots() {
        let mut server = server(&[0xeafffffe]);
        let begin = json!({
            "command": "watch-begin", "ranges": [[0x01467588u64, 0x01467840u64]],
            "limit": 16, "views": "fixed",
            "write_ranges": [[0x01467588u64, 0x01467840u64]], "fetch_ranges": []
        });
        server.handle(request(begin)).unwrap();
        assert!(server.handle(request(json!({"command": "snapshot-save", "name": "n0"}))).is_err());
        let state = server.handle(Request::WatchEnd).unwrap().unwrap();
        assert_eq!(state["enabled"], true);
        assert_eq!(state["views"], "fixed");
        assert!(server.handle(Request::WatchEnd).is_err());
        assert!(server.handle(Request::WatchState).is_err());
        assert!(server.handle(request(json!({"command": "snapshot-save", "name": "n0"}))).is_ok());
    }

    #[test]
    fn trace_site_reads_record_memory_at_entry() {
        let mut server = server(&[0xe2800001, 0xeafffffd]);
        server.handle(request(json!({"command": "trace-sites", "sites": {"0x00000004": "loop"},
            "reads": {"0x00000004": [[0, 4], [4, 4]]}}))).unwrap();
        assert_eq!(server.bridge.machine.run(100), RunOutcome::Completed);
        let hits = server.handle(request(json!({"command": "trace-hits", "from": 0}))).unwrap().unwrap();
        let hits = hits["hits"].as_array().unwrap();
        assert!(!hits.is_empty() && hits.iter().all(|hit| hit["memory"] == json!(["010080e2", "fdffffea"])));
        assert!(server.handle(request(json!({"command": "trace-sites", "sites": {"0x00000004": "loop"},
            "reads": {"0x00000008": [[0, 4]]}}))).is_err());
        assert!(server.handle(request(json!({"command": "trace-sites", "sites": {"0x00000004": "loop"},
            "reads": {"0x00000004": [[0, 0]]}}))).is_err());
    }

    #[test]
    fn trace_sites_beyond_one_engine_point_set_record_hits() {
        let mut server = server(&[0xe2800001, 0xeafffffd]);
        let sites: BTreeMap<String, String> = std::iter::once(("0x00000004".to_owned(), "loop".to_owned()))
            .chain((0..4999u32).map(|index| (format!("{:#010x}", 0x0200_0000 + 4 * index), "absent".to_owned())))
            .collect();
        server.handle(request(json!({"command": "trace-sites", "sites": sites}))).unwrap();
        assert_eq!(server.bridge.machine.run(100), RunOutcome::Completed);
        let hits = server.handle(request(json!({"command": "trace-hits", "from": 0}))).unwrap().unwrap();
        let hits = hits["hits"].as_array().unwrap();
        assert!(!hits.is_empty() && hits.iter().all(|hit| hit["site"] == "loop"));
    }

    #[test]
    fn trace_hits_pages_concatenate_to_the_whole_record() {
        let mut server = server(&[0xe2800001, 0xeafffffd]);
        server.handle(request(json!({"command": "trace-sites", "sites": {"0x00000004": "loop"}}))).unwrap();
        assert_eq!(server.bridge.machine.run(100), RunOutcome::Completed);
        let whole = server.handle(request(json!({"command": "trace-hits", "from": 0}))).unwrap().unwrap();
        let count = whole["count"].as_u64().unwrap() as usize;
        assert!(count > 3);
        let mut paged = Vec::new();
        while paged.len() < count {
            let page = server.handle(request(json!({"command": "trace-hits", "from": paged.len(), "limit": 3}))).unwrap().unwrap();
            assert_eq!(page["count"].as_u64(), Some(count as u64));
            let hits = page["hits"].as_array().unwrap();
            assert!(!hits.is_empty() && hits.len() <= 3);
            paged.extend(hits.iter().cloned());
        }
        assert_eq!(&paged, whole["hits"].as_array().unwrap());
        assert!(serde_json::from_value::<Request>(json!({"command": "trace-hits", "from": 0, "limit": 0})).is_err());
    }

    #[test]
    fn reset_observation_ends_one_write_segment_and_hit_range_per_attach_phase() {
        let mut server = server(&[0xe3a01609, 0xe2800001, 0xe5810000, 0xeafffffc]);
        let spec = ResetObservationSpec::parse(concat!(r#"{"schema":1,"sites":{"0x00000008":"store"},"hit_limit":100000,"#,
            r#""write_ranges":[[9437184,9437188]],"watch_limit":4096}"#).as_bytes()).unwrap();
        let mut observer = ResetObserver::begin(&mut server.bridge.machine, &spec).unwrap();
        for phase in [AttachPhase::FirstStart, AttachPhase::LiveView, AttachPhase::Attached] {
            assert_eq!(server.bridge.machine.run(60), RunOutcome::Completed);
            observer.boundary(&server.bridge.machine, phase).unwrap();
        }
        let record = serde_json::to_value(observer.finish().unwrap()).unwrap();
        assert_eq!(record["reset"]["instructions"], 0);
        let segments = record["segments"].as_array().unwrap();
        assert_eq!(segments.iter().map(|segment| segment["phase"].as_str().unwrap()).collect::<Vec<_>>(),
            ["first-start", "live-view", "attached"]);
        let mut previous = 0;
        for segment in segments {
            let hits = segment["hits_end"].as_u64().unwrap();
            assert!(hits > previous);
            previous = hits;
            let rows = segment["watch"]["records"].as_array().unwrap();
            assert!(!rows.is_empty() && rows.iter().all(|row| row["source"] == "cpu-write" && row["physical"] == 0x0090_0000));
        }
        assert_eq!(server.bridge.machine.traces.hits.len() as u64, previous);
    }

    #[test]
    fn a_record_nand_keys_the_attach_checkpoint_only_when_given() {
        use fujimod_xm1::nand_image::{CommittedImage, PAGES_PER_BLOCK, PAGE_BYTES, SPARE_BYTES};
        let pages = fujimod_xm1::adjustment::BLOCKS
            .flat_map(|block| block * PAGES_PER_BLOCK..(block + 1) * PAGES_PER_BLOCK)
            .map(|row| (row, (vec![0u8; PAGE_BYTES], vec![0xffu8; SPARE_BYTES])))
            .collect();
        let adjustment = CommittedImage::compose(&[0u8; 16], &pages).unwrap();
        let inputs = Inputs {
            document_bytes: b"run".to_vec(),
            update: b"dat".to_vec(),
            boot_rom: b"rom".to_vec(),
            adjustment: AdjustmentNand::parse(&adjustment).unwrap(),
            records: None,
            card: None,
        };
        let arguments = |record_nand: Option<&str>| Arguments {
            run: "run.json".into(),
            boot_rom: "rom".into(),
            installed_dat: "dat".into(),
            adjustment_nand: "adjustment.nand".into(),
            record_nand: record_nand.map(PathBuf::from),
            output: "out".into(),
            checkpoints: "checkpoints".into(),
            sd_card_image: None,
            reset_observation: None,
            attach_hold: None,
            fresh_boot: false,
        };
        let without = boot_key(&arguments(None), &inputs, &adjustment, None).unwrap();
        let first = boot_key(&arguments(Some("a.nand")), &inputs, &adjustment, Some(b"first")).unwrap();
        let moved = boot_key(&arguments(Some("b.nand")), &inputs, &adjustment, Some(b"first")).unwrap();
        let second = boot_key(&arguments(Some("a.nand")), &inputs, &adjustment, Some(b"second")).unwrap();
        assert!(without.inputs["inputs"].get("record_nand").is_none());
        assert_eq!(first.inputs["inputs"]["record_nand"], sha256(b"first"));
        assert_eq!(first.key, moved.key);
        assert_ne!(first.key, second.key);
        assert_ne!(first.key, without.key);
    }
}

