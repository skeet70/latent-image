//! The host side of the X-M1 ff80 service endpoint over FujiMod's USB port:
//! standard enumeration, twelve-byte control requests and bulk transfers on
//! OUT 2 and IN 0x81, with the RAM-push rendezvous hold of the retained
//! bridge (`tools/continue_rec_scanout.py`, `_FF80EndpointBridge`).

use crate::guest::{Press, RouteKey};
use crate::machine::{KeyPulse, Machine, RunOutcome};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::time::Instant;

/// A wait polls after this many instructions and then at doubling intervals up to `WAIT_STEP`.
const FIRST_POLL: u64 = 1_000;
const WAIT_STEP: u64 = 200_000;
const WAIT_BUDGET: u64 = 30_000_000;
const ACTIVATION_BUDGET: u64 = 140_000_000;
/// Instructions standing for USB 2.0's 2 ms SetAddress recovery interval.
pub const SET_ADDRESS_RECOVERY: u64 = 1_000_000;
const PACKET_BYTES: usize = 64;
const BULK_OUT: u8 = 2;
const BULK_IN: u8 = 0x81;
const BULK_IN_PORT: usize = 1;
/// An UP hold of 455,645 instructions did not enter stock focus-area selection
/// on the retained engine and 1,778,831 did (emulated).
const RENDEZVOUS_HOLD: u64 = 4_000_000;
pub const VENDOR_PRODUCT: [u8; 4] = [0xcb, 0x04, 0x80, 0xff];
const KEEPALIVE_PERIOD: u64 = 32;
const KEEPALIVE_PULSE: KeyPulse = KeyPulse { hold: 2_000_000, release: 2_000_000 };
/// Trace labels the e2e arms on its runtime's rendezvous symbols.
#[derive(Clone, Copy, PartialEq, Eq)]
enum RendezvousSite {
    Locked,
    Cpu0WindowExit,
    Cpu1WindowExit,
    QueryCallbackResult,
}

impl RendezvousSite {
    fn parse(label: &str) -> Option<Self> {
        Some(match label {
            "rendezvous-locked" => Self::Locked,
            "cpu0-window-exit" => Self::Cpu0WindowExit,
            "cpu1-window-exit" => Self::Cpu1WindowExit,
            "query-callback-result" => Self::QueryCallbackResult,
            _ => return None,
        })
    }
}

pub const RENDEZVOUS_LOCKED: &str = "rendezvous-locked";

#[derive(Deserialize)]
struct BusStatus {
    registers: Vec<u32>,
    rx_length: [usize; 2],
    rx_position: [usize; 2],
    setup_length: usize,
    setup_position: usize,
    tx_packets: Vec<usize>,
    active_dma: bool,
}

impl BusStatus {
    fn register(&self, offset: usize) -> u32 {
        self.registers.get(offset / 4).copied().unwrap_or(0)
    }
}

#[derive(Deserialize, PartialEq, Eq)]
#[serde(rename_all = "PascalCase")]
enum Direction {
    In,
    Out,
}

#[derive(Deserialize)]
struct ActiveTransfer {
    direction: Direction,
    exact: usize,
}

#[derive(Deserialize)]
struct EndpointDma {
    active: Option<ActiveTransfer>,
}

#[derive(Deserialize)]
struct ControllerSave {
    endpoints: Option<EndpointDma>,
}

/// The inputs held from the rendezvous lock to its release (the retained
/// bridge's `hold_rendezvous_inputs`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RendezvousInputs {
    MenuUp,
    MenuUpAndWriteProtect,
}

impl RendezvousInputs {
    fn names(self) -> Vec<&'static str> {
        match self {
            Self::MenuUp => vec!["menu-up"],
            Self::MenuUpAndWriteProtect => vec!["menu-up", "sd-write-protect"],
        }
    }
}

/// One rendezvous hold, from the locked entry to its release.
#[derive(Serialize)]
pub struct Rendezvous {
    latched_instructions: u64,
    inputs: Vec<&'static str>,
    window_exits: Vec<WindowExit>,
    callback_instructions: Option<u64>,
    released_instructions: Option<u64>,
    menu_after: Option<Value>,
}

#[derive(Serialize)]
struct WindowExit {
    cpu1: bool,
    core: usize,
    instructions: u64,
}

/// A transfer request of the bridge protocol.
#[derive(Deserialize)]
#[serde(tag = "transfer", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Transfer {
    Control { bytes: String },
    BulkOut { endpoint: u8, bytes: String },
    BulkIn { endpoint: u8, count: usize },
}

/// Where transfer time goes: host-side bus queries and guest instructions
/// spent waiting for the endpoint.
#[derive(Default, Serialize)]
pub struct TransferProfile {
    pub transfers: u64,
    pub transfer_seconds: f64,
    pub transfer_instructions: u64,
    pub polls: u64,
    pub wait_instructions: u64,
    pub status_calls: u64,
    pub status_seconds: f64,
    pub dma_calls: u64,
    pub dma_seconds: f64,
}

pub struct Ff80 {
    pub machine: Machine,
    pub rendezvous: Vec<Rendezvous>,
    rendezvous_inputs: RendezvousInputs,
    rendezvous_complete: bool,
    hits_seen: usize,
    pub transfer_log: Vec<Value>,
    /// Presses dial-push before every 32nd transfer; stock 1.50 live view
    /// resets auto power-off on it with no screen or MENU change (emulated).
    pub keepalive: bool,
    transfers: u64,
    pub keepalive_inputs: Vec<Value>,
    pub profile: TransferProfile,
}

pub fn decode_hex(text: &str) -> Result<Vec<u8>, String> {
    if text.len() % 2 != 0 {
        return Err("hex payload has odd length".into());
    }
    (0..text.len() / 2)
        .map(|index| u8::from_str_radix(&text[2 * index..2 * index + 2], 16).map_err(|_| "payload is not hex".into()))
        .collect()
}

pub fn encode_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

impl Ff80 {
    pub fn new(machine: Machine, rendezvous_inputs: RendezvousInputs) -> Self {
        let hits_seen = machine.traces.hits.len();
        Self {
            machine,
            rendezvous: Vec::new(),
            rendezvous_inputs,
            rendezvous_complete: false,
            hits_seen,
            transfer_log: Vec::new(),
            keepalive: false,
            transfers: 0,
            keepalive_inputs: Vec::new(),
            profile: TransferProfile::default(),
        }
    }

    /// Starts counting transfers towards the next keepalive press.
    pub fn enable_keepalive(&mut self) {
        self.keepalive = true;
        self.transfers = 0;
    }

    fn usb(&self) -> String {
        self.machine.document.usb.0.clone()
    }

    fn status(&mut self) -> Result<BusStatus, String> {
        let started = Instant::now();
        let status = self.machine.session.usb_status(&self.usb())?;
        let status = serde_json::from_value(status["endpoint"].clone()).map_err(|e| format!("USB bus status: {e}"));
        self.profile.status_calls += 1;
        self.profile.status_seconds += started.elapsed().as_secs_f64();
        status
    }

    /// The endpoint's active DMA transfer, from the controller's full diagnostic state.
    fn dma(&mut self) -> Result<EndpointDma, String> {
        let started = Instant::now();
        let node = self.machine.session.node(&self.usb())?;
        let save: ControllerSave = serde_json::from_value(self.machine.session.board.device_save(node)?)
            .map_err(|e| format!("USB controller state: {e}"))?;
        self.profile.dma_calls += 1;
        self.profile.dma_seconds += started.elapsed().as_secs_f64();
        save.endpoints.ok_or_else(|| "USB endpoints are not connected".into())
    }

    pub fn begin_capture_observation(&mut self) -> Result<(), String> {
        self.machine.require_running()?;
        if self.rendezvous_complete || self.rendezvous.last().is_some_and(|event| event.released_instructions.is_none()) {
            return Err("capture observation cannot replace an active rendezvous trace".into());
        }
        if self.hits_seen != self.machine.traces.hits.len() {
            return Err("capture observation requires consumed rendezvous trace hits".into());
        }
        self.machine.begin_capture_observation()?;
        self.hits_seen = 0;
        Ok(())
    }

    /// Applies the rendezvous protocol to entries recorded since the last call.
    fn observe(&mut self) -> Result<(), String> {
        while self.hits_seen < self.machine.traces.hits.len() {
            let hit = self.machine.traces.hits[self.hits_seen].clone();
            self.hits_seen += 1;
            let open = self.rendezvous.last().is_some_and(|event| event.released_instructions.is_none());
            match (RendezvousSite::parse(&hit.site), open) {
                (Some(RendezvousSite::Locked), false) => {
                    let masked = (0..self.machine.session.board.instruction_counts().len())
                        .map(|core| {
                            self.machine
                                .session
                                .board
                                .read_register(core, fujimod_emulator_cpu::Register::Arm32(fujimod_emulator_cpu::arm::Register::Cpsr))
                                .map(|cpsr| cpsr & 0x80 != 0)
                        })
                        .collect::<Result<Vec<bool>, String>>()?;
                    if hit.core != 0 || masked.iter().any(|masked| !masked) {
                        return Err(format!("rendezvous trace lacks two masked cores: core {} masks {masked:?}", hit.core));
                    }
                    self.machine.press(RouteKey::Up)?;
                    if self.rendezvous_inputs == RendezvousInputs::MenuUpAndWriteProtect {
                        self.machine.protect_card(true)?;
                    }
                    self.rendezvous.push(Rendezvous {
                        latched_instructions: hit.instructions,
                        inputs: self.rendezvous_inputs.names(),
                        window_exits: Vec::new(),
                        callback_instructions: None,
                        released_instructions: None,
                        menu_after: None,
                    });
                }
                (Some(exit @ (RendezvousSite::Cpu0WindowExit | RendezvousSite::Cpu1WindowExit)), true) => {
                    let event = self.rendezvous.last_mut().ok_or("rendezvous activity is missing")?;
                    if event.callback_instructions.is_none() {
                        event.window_exits.push(WindowExit {
                            cpu1: exit == RendezvousSite::Cpu1WindowExit,
                            core: hit.core,
                            instructions: hit.instructions,
                        });
                    }
                }
                (Some(RendezvousSite::QueryCallbackResult), true) => {
                    let event = self.rendezvous.last_mut().ok_or("rendezvous activity is missing")?;
                    if event.callback_instructions.is_none() {
                        let cpu1 = event.window_exits.iter().filter(|exit| exit.cpu1).count();
                        if cpu1 != 1 || event.window_exits.len() != 2 {
                            return Err("incomplete rendezvous window trace".into());
                        }
                        event.callback_instructions = Some(hit.instructions);
                        self.rendezvous_complete = true;
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }

    pub fn advance(&mut self, count: u64) -> Result<RunOutcome, String> {
        if count == 0 && !self.machine.traces.capture_observation_active() {
            self.machine.require_running()?;
            return Ok(RunOutcome::Completed);
        }
        let end = self.machine.instructions().saturating_add(count);
        loop {
            self.machine.require_running()?;
            let remaining = end.saturating_sub(self.machine.instructions());
            let outcome = self.machine.run(remaining);
            self.machine.require_running()?;
            self.observe()?;
            if outcome == RunOutcome::DrainRequired {
                return Ok(outcome);
            }
            if self.machine.instructions() >= end {
                return Ok(RunOutcome::Completed);
            }
        }
    }

    /// Runs until `ready` holds.
    fn wait(&mut self, label: &str, budget: u64, mut ready: impl FnMut(&mut Self) -> Result<bool, String>) -> Result<(), String> {
        let start = self.machine.instructions();
        let mut step = FIRST_POLL;
        loop {
            self.machine.require_running().map_err(|e| format!("{label}: {e}"))?;
            self.profile.polls += 1;
            if ready(self)? {
                self.profile.wait_instructions += self.machine.instructions() - start;
                return Ok(());
            }
            let spent = self.machine.instructions() - start;
            if spent >= budget {
                return Err(format!("{label}: instruction budget expired"));
            }
            self.advance((budget - spent).min(step)).map_err(|e| format!("{label}: {e}"))?;
            step = (step * 2).min(WAIT_STEP);
        }
    }

    pub fn release_rendezvous(&mut self) -> Result<(), String> {
        if !self.rendezvous_complete {
            return Ok(());
        }
        let latched = self.rendezvous.last().ok_or("rendezvous activity is missing")?.latched_instructions;
        let held = self.machine.instructions() - latched;
        if held < RENDEZVOUS_HOLD {
            self.advance(RENDEZVOUS_HOLD - held)?;
        }
        self.machine.release_key(RouteKey::Up)?;
        if self.rendezvous_inputs == RendezvousInputs::MenuUpAndWriteProtect {
            self.machine.protect_card(false)?;
        }
        let released = self.machine.instructions();
        let menu = self.machine.menu()?;
        let event = self.rendezvous.last_mut().ok_or("rendezvous activity is missing")?;
        event.released_instructions = Some(released);
        event.menu_after = Some(json!(menu.map(|m| json!({"menu_id": m.menu_id, "setting_id": m.setting_id, "cursor": m.cursor}))));
        self.rendezvous_complete = false;
        Ok(())
    }

    fn control_setup(&mut self, setup: [u8; 8]) -> Result<(), String> {
        self.machine.session.usb_setup(&self.usb(), &setup)
    }

    fn control_in(&mut self, setup: [u8; 8], count: usize) -> Result<Vec<u8>, String> {
        self.control_setup(setup)?;
        let label = format!("control IN data for setup {}", encode_hex(&setup));
        let mut payload = Vec::new();
        while payload.len() < count {
            self.wait(&label, WAIT_BUDGET, |host| Ok(host.status()?.tx_packets[0] > 0))?;
            let packet = self.machine.session.usb_in(&self.usb(), 0x80, count - payload.len())?;
            let short = packet.len() < PACKET_BYTES;
            payload.extend(packet);
            if short {
                break;
            }
        }
        self.machine.session.usb_out(&self.usb(), 0, &[])?;
        Ok(payload)
    }

    fn control_status(&mut self, setup: [u8; 8]) -> Result<(), String> {
        self.control_setup(setup)?;
        self.wait("control status IN", WAIT_BUDGET, |host| Ok(host.status()?.tx_packets[0] > 0))?;
        if !self.machine.session.usb_in(&self.usb(), 0x80, 64)?.is_empty() {
            return Err("control status packet was not empty".into());
        }
        Ok(())
    }

    fn control_out(&mut self, payload: &[u8]) -> Result<(), String> {
        let length = u16::try_from(payload.len()).map_err(|_| "control payload exceeds 16 bits")?.to_le_bytes();
        self.control_setup([0x43, 0, 0, 0, 0, 0, length[0], length[1]])?;
        self.wait("control OUT setup consumption", WAIT_BUDGET, |host| {
            let status = host.status()?;
            Ok(status.setup_position == status.setup_length)
        })?;
        self.machine.session.usb_out(&self.usb(), 0, payload)?;
        self.wait("control OUT status", WAIT_BUDGET, |host| Ok(host.status()?.tx_packets[0] > 0))?;
        if !self.machine.session.usb_in(&self.usb(), 0x80, 64)?.is_empty() {
            return Err("control OUT status packet was not empty".into());
        }
        Ok(())
    }

    fn bulk_out(&mut self, endpoint: u8, payload: &[u8]) -> Result<(), String> {
        if endpoint != BULK_OUT {
            return Err("RAM-push bridge admits only bulk OUT endpoint 2".into());
        }
        if self.status()?.active_dma {
            self.wait("bulk OUT DMA readiness", WAIT_BUDGET, |host| {
                Ok(match host.dma()?.active {
                    None => true,
                    Some(active) => active.direction == Direction::Out && active.exact == payload.len(),
                })
            })?;
        }
        for chunk in payload.chunks(PACKET_BYTES) {
            self.machine.session.usb_out(&self.usb(), BULK_OUT, chunk)?;
            self.wait("bulk OUT packet consumption", WAIT_BUDGET, |host| {
                let status = host.status()?;
                Ok(status.rx_position[1] == status.rx_length[1])
            })?;
        }
        self.wait("bulk OUT DMA completion", WAIT_BUDGET, |host| Ok(!host.status()?.active_dma))
    }

    fn bulk_in(&mut self, endpoint: u8, count: usize) -> Result<Vec<u8>, String> {
        if endpoint != BULK_IN {
            return Err("RAM-push bridge admits only bulk IN endpoint 0x81".into());
        }
        let mut payload = Vec::new();
        while payload.len() < count {
            let usb = self.usb();
            self.wait("bulk IN reply", WAIT_BUDGET, |host| {
                let status = host.status()?;
                if status.tx_packets[BULK_IN_PORT] > 0 {
                    return Ok(true);
                }
                if status.register(0x030) & 4 == 0 {
                    host.machine.session.usb_in_token(&usb, BULK_IN)?;
                }
                Ok(false)
            })?;
            let packet = self.machine.session.usb_in(&self.usb(), BULK_IN, count - payload.len())?;
            let short = packet.len() < PACKET_BYTES;
            payload.extend(packet);
            if short {
                break;
            }
        }
        Ok(payload)
    }

    /// Raises the cable level first; returns the ff80 device and configuration descriptors.
    /// The RAM-push routes' accepted trajectories were recorded with no SetAddress recovery wait.
    pub fn enumerate(&mut self) -> Result<(Vec<u8>, Vec<u8>), String> {
        let cable = self.machine.document.usb_cable.0.clone();
        self.machine.session.logic_input(&cable, true)?;
        let (device, configuration) = self.enumerate_attached(ACTIVATION_BUDGET, 0)?;
        if device[8..12] != VENDOR_PRODUCT {
            return Err(format!("unexpected device descriptor {}", encode_hex(&device)));
        }
        Ok((device, configuration))
    }

    /// Standard enumeration of whatever device the guest presents, once the
    /// controller activates within `budget` instructions, waiting `set_address_recovery`
    /// instructions after SetAddress's status stage.
    pub fn enumerate_attached(&mut self, budget: u64, set_address_recovery: u64) -> Result<(Vec<u8>, Vec<u8>), String> {
        self.wait("USB controller activation", budget, |host| Ok(host.status()?.register(0x090) & 6 == 6))?;
        self.machine.session.usb_reset(&self.usb())?;
        self.advance(1_000_000)?;
        let device = self.control_in([0x80, 6, 0, 1, 0, 0, 18, 0], 18)?;
        if device.len() != 18 {
            return Err(format!("unexpected device descriptor {}", encode_hex(&device)));
        }
        self.control_status([0, 5, 1, 0, 0, 0, 0, 0])?;
        self.advance(set_address_recovery)?;
        let head = self.control_in([0x80, 6, 0, 2, 0, 0, 9, 0], 9)?;
        let total = head.get(2..4).map(|bytes| usize::from(u16::from_le_bytes([bytes[0], bytes[1]]))).unwrap_or(0);
        if head.len() != 9 || !(9..=256).contains(&total) {
            return Err(format!("unexpected configuration descriptor head {}", encode_hex(&head)));
        }
        let length = (total as u16).to_le_bytes();
        let configuration = self.control_in([0x80, 6, 0, 2, 0, 0, length[0], length[1]], total)?;
        if configuration.len() != total {
            return Err("configuration descriptor is truncated".into());
        }
        self.control_status([0, 9, configuration[5], 0, 0, 0, 0, 0])?;
        self.advance(1_000_000)?;
        Ok((device, configuration))
    }

    pub fn transfer(&mut self, request: &Transfer) -> Result<(usize, Vec<u8>), String> {
        let started = self.machine.instructions();
        let clock = Instant::now();
        self.release_rendezvous()?;
        self.transfers += 1;
        if self.keepalive && self.transfers % KEEPALIVE_PERIOD == 0 {
            let before = self.machine.instructions();
            self.machine.dial_push(KEEPALIVE_PULSE)?;
            self.observe()?;
            self.keepalive_inputs.push(json!({"key": "dial-push", "before_instructions": before,
                "after_instructions": self.machine.instructions()}));
        }
        let (kind, frame, result) = match request {
            Transfer::Control { bytes } => {
                let payload = decode_hex(bytes)?;
                if payload.len() != 12 {
                    return Err("control transfer must carry the internal twelve-byte request".into());
                }
                self.control_out(&payload)?;
                ("control", payload.clone(), (payload.len(), Vec::new()))
            }
            Transfer::BulkOut { endpoint, bytes } => {
                let payload = decode_hex(bytes)?;
                self.bulk_out(*endpoint, &payload)?;
                ("bulk-out", payload.clone(), (payload.len(), Vec::new()))
            }
            Transfer::BulkIn { endpoint, count } => {
                let payload = self.bulk_in(*endpoint, *count)?;
                ("bulk-in", Vec::new(), (payload.len(), payload))
            }
        };
        let opcode = (frame.len() >= 4).then(|| format!("{:08x}", u32::from_le_bytes([frame[0], frame[1], frame[2], frame[3]])));
        let function = (frame.len() >= 20).then(|| u32::from_le_bytes([frame[16], frame[17], frame[18], frame[19]]));
        self.transfer_log.push(json!({
            "transfer": kind, "bytes": result.0, "opcode": opcode, "function": function,
            "instructions": self.machine.instructions() - started,
            "host_seconds": clock.elapsed().as_secs_f64(),
        }));
        self.profile.transfers += 1;
        self.profile.transfer_seconds += clock.elapsed().as_secs_f64();
        self.profile.transfer_instructions += self.machine.instructions() - started;
        Ok(result)
    }

    pub fn key(&mut self, key: RouteKey, pulse: KeyPulse) -> Result<(), String> {
        self.machine.key(key, pulse)?;
        self.observe()
    }

    pub fn settled_key(&mut self, key: Press, pulse: KeyPulse, budget: u64) -> Result<crate::render::Settle, String> {
        let settle = crate::render::settled_key(&mut self.machine, key, pulse, budget)?;
        self.observe()?;
        Ok(settle)
    }
}
