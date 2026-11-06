//! The host side of the X-M1 ff80 service endpoint over FujiMod's USB port:
//! standard enumeration, twelve-byte control requests and bulk transfers on
//! OUT 2 and IN 0x81, with the RAM-push rendezvous hold of the retained
//! bridge (`tools/continue_rec_scanout.py`, `_FF80EndpointBridge`).

use crate::guest::{Press, RouteKey};
use crate::machine::{KeyPulse, Latch, Machine, RunOutcome};
use crate::trace::CaptureScope;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::time::Instant;

/// A wait polls after this many instructions and then at doubling intervals up to `WAIT_STEP`.
const FIRST_POLL: u64 = 1_000;
const WAIT_STEP: u64 = 200_000;
const WAIT_BUDGET: u64 = 30_000_000;
const ACTIVATION_BUDGET: u64 = 140_000_000;
/// The host waits at least 10 ms after SetAddress's status stage, as Linux's `hub_port_init` does
/// (`msleep(10)` after `hub_set_address`); USB 2.0 §9.2.6.3 gives the device 2 ms. Board ticks
/// run at the run document's 12 MHz device clock policy.
const SET_ADDRESS_RECOVERY_TICKS: u64 = 120_000;
const PACKET_BYTES: usize = 64;
const BULK_OUT: u8 = 2;
const BULK_IN: u8 = 0x81;
const BULK_IN_PORT: usize = 1;
/// The retained bridge's floor for its rendezvous UP hold: 455,645 instructions did not enter
/// stock focus-area selection and 1,778,831 did (emulated). The card stays write-protected as long.
const RENDEZVOUS_HOLD: u64 = 4_000_000;
pub const VENDOR_PRODUCT: [u8; 4] = [0xcb, 0x04, 0x80, 0xff];
const KEEPALIVE_PERIOD: u64 = 32;
const KEEPALIVE_PULSE: KeyPulse = KeyPulse::instructions(2_000_000, 2_000_000);
/// Controller 74's (modal 0x4a) state halfword, `0x01467588 + 2 * 74`.
const TOP_CONTROLLER_STATE: u64 = 0x0146_761c;
/// `[0x0096cc64, 0x0096ce26)`, the extent the stock BACK settings restore copies.
const LIVE_SETTINGS: u64 = 0x0096_cc64;
const LIVE_SETTINGS_BYTES: usize = 450;

#[derive(Serialize)]
pub struct LocalUiDrain {
    pub instructions: u64,
    pub ran: u64,
    pub ticks: u64,
    pub ui: crate::application_observation::UiWorkViews,
    pub render: LocalRenderState,
}

#[derive(Serialize)]
pub struct LocalRenderState {
    initialized: u8,
    idle: [u32; 7],
    keys_released: Vec<bool>,
    task_statuses: Vec<Option<u8>>,
    graphics: crate::application_observation::GraphicsState,
}

/// Controller 74's state: 0x04 on the live-view root; live-view overlays, menus and SET-UP
/// pages use other values (crawl-4, reports/recipe-bank-ui-ownership.md).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct TopControllerState(u16);
impl TopControllerState {
    const LIVE_VIEW_ROOT: Self = Self(0x04);
}
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

/// The inputs from the rendezvous lock to its release. The retained bridge's
/// `hold_rendezvous_inputs` holds UP, which opens focus-area selection at live view.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RendezvousInputs {
    DialPush,
    DialPushAndWriteProtect,
}

impl RendezvousInputs {
    fn names(self) -> Vec<&'static str> {
        match self {
            Self::DialPush => vec!["dial-push"],
            Self::DialPushAndWriteProtect => vec!["dial-push", "sd-write-protect"],
        }
    }
}

/// One rendezvous hold, from the locked entry to its release.
#[derive(Serialize)]
pub struct Rendezvous {
    latched_instructions: u64,
    controller_74_state: u16,
    inputs: Vec<&'static str>,
    dial_push_latched: Option<bool>,
    window_exits: Vec<WindowExit>,
    callback_instructions: Option<u64>,
    released_instructions: Option<u64>,
    menu_after: Option<Value>,
    #[serde(skip)]
    settings_at_latch: Box<[u8; LIVE_SETTINGS_BYTES]>,
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

    /// Whether a rendezvous is latched or not yet released.
    pub fn rendezvous_active(&self) -> bool {
        self.rendezvous_complete || self.rendezvous.last().is_some_and(|event| event.released_instructions.is_none())
    }

    pub fn begin_capture_observation(&mut self, scope: CaptureScope) -> Result<(), String> {
        self.machine.require_running()?;
        if self.rendezvous_active() {
            return Err("capture observation cannot replace an active rendezvous trace".into());
        }
        if self.hits_seen != self.machine.traces.hits.len() {
            return Err("capture observation requires consumed rendezvous trace hits".into());
        }
        self.machine.begin_capture_observation(scope)?;
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
                    let mut state = [0; 2];
                    self.machine.session.board.read_physical(TOP_CONTROLLER_STATE, &mut state)?;
                    let state = TopControllerState(u16::from_le_bytes(state));
                    if state != TopControllerState::LIVE_VIEW_ROOT {
                        return Err(format!(
                            "rendezvous refused: controller 74 state {:#04x} is not the live-view root {:#04x}; ram-push and ram-unload run only at the idle shooting screen with no menu open (transport/README.md)",
                            state.0, TopControllerState::LIVE_VIEW_ROOT.0));
                    }
                    let mut settings_at_latch = Box::new([0; LIVE_SETTINGS_BYTES]);
                    self.machine.session.board.read_physical(LIVE_SETTINGS, settings_at_latch.as_mut_slice())?;
                    if self.rendezvous_inputs == RendezvousInputs::DialPushAndWriteProtect {
                        self.machine.protect_card(true)?;
                    }
                    self.rendezvous.push(Rendezvous {
                        latched_instructions: hit.instructions,
                        controller_74_state: state.0,
                        inputs: self.rendezvous_inputs.names(),
                        dial_push_latched: None,
                        window_exits: Vec::new(),
                        callback_instructions: None,
                        released_instructions: None,
                        menu_after: None,
                        settings_at_latch,
                    });
                    // The keepalive pulse: at live view it leaves the screen and the MENU path unchanged (emulated).
                    let latched = self.machine.dial_push(KEEPALIVE_PULSE)? == Latch::Latched;
                    self.rendezvous.last_mut().ok_or("rendezvous activity is missing")?.dial_push_latched = Some(latched);
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

    /// Bounded by the no-wait instruction count.
    pub fn advance_ticks(&mut self, ticks: u64) -> Result<RunOutcome, String> {
        let until = self.machine.session.board.now().saturating_add(ticks);
        let end = self.machine.instructions().saturating_add(self.machine.hold_bound(ticks));
        loop {
            self.machine.require_running()?;
            let remaining = end.saturating_sub(self.machine.instructions());
            let outcome = self.machine.run_until(remaining, Some(until));
            self.machine.require_running()?;
            self.observe()?;
            if outcome == RunOutcome::DrainRequired {
                return Ok(outcome);
            }
            if self.machine.instructions() >= end || self.machine.session.board.now() >= until {
                return Ok(RunOutcome::Completed);
            }
        }
    }

    /// Observes current deferred UI and render work, not future timer sends.
    pub fn local_ui_drain(&mut self, ticks: u64) -> Result<LocalUiDrain, String> {
        let before = self.machine.instructions();
        let from = self.machine.session.board.now();
        let until = from.saturating_add(ticks);
        let end = before.saturating_add(self.machine.hold_bound(ticks));
        loop {
            self.machine.require_running()?;
            let ui = crate::application_observation::UiWorkViews::read(&self.machine.session)?;
            let facts = &self.machine.document.firmware.render;
            if ui.drained() && facts.probe(&self.machine)?.drained {
                let read = |address: crate::run::Address, bytes| {
                    let address = address.0 & !0x4000_0000;
                    self.machine.read(address, bytes).map(|raw| raw.iter().rev().fold(0u32, |value, byte| value << 8 | u32::from(*byte)))
                };
                let render = LocalRenderState {
                    initialized: read(facts.initialized, 1)? as u8,
                    idle: [
                        read(facts.ui_pending, 4)?, read(facts.ui_busy, 1)?, read(facts.display_suppression, 4)?,
                        read(facts.batch_busy, 4)?, read(facts.batch_callback, 4)?, read(facts.commands_dirty, 4)?,
                        read(facts.display_pending, 4)?,
                    ],
                    keys_released: self.machine.document.firmware.keys.iter()
                        .map(|binding| self.machine.document.firmware.released(&self.machine.session, binding.key))
                        .collect::<Result<_, _>>()?,
                    task_statuses: facts.task_statuses(&self.machine)?,
                    graphics: crate::application_observation::GraphicsState::of(
                        &self.machine.session.board.device_save(self.machine.session.node(&facts.graphics.0)?)?)?,
                };
                return Ok(LocalUiDrain {
                    instructions: self.machine.instructions(), ran: self.machine.instructions() - before,
                    ticks: self.machine.session.board.now() - from, ui, render,
                });
            }
            let remaining = end.saturating_sub(self.machine.instructions());
            if remaining == 0 || self.machine.session.board.now() >= until {
                return Err("local-ui-drained endpoint not observed within its tick bound".into());
            }
            let outcome = self.machine.run_until(remaining.min(250_000), Some(until));
            self.machine.require_running()?;
            self.observe()?;
            if outcome == RunOutcome::DrainRequired {
                return Err("local-ui-drained cannot advance with a capture page outstanding".into());
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
        if self.rendezvous_inputs == RendezvousInputs::DialPushAndWriteProtect {
            self.machine.protect_card(false)?;
        }
        let released = self.machine.instructions();
        let menu = self.machine.menu()?;
        let mut settings = [0; LIVE_SETTINGS_BYTES];
        self.machine.session.board.read_physical(LIVE_SETTINGS, &mut settings)?;
        let event = self.rendezvous.last_mut().ok_or("rendezvous activity is missing")?;
        event.released_instructions = Some(released);
        event.menu_after = Some(json!(menu.map(|m| json!({"menu_id": m.menu_id, "setting_id": m.setting_id, "cursor": m.cursor}))));
        self.rendezvous_complete = false;
        if let Some((offset, (now, latched))) = settings.iter().zip(event.settings_at_latch.iter()).enumerate().find(|(_, (now, latched))| now != latched) {
            return Err(format!("live setting byte {:#010x} changed from {latched:#04x} to {now:#04x} between the rendezvous latch and its release",
                LIVE_SETTINGS + offset as u64));
        }
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
    pub fn enumerate(&mut self) -> Result<(Vec<u8>, Vec<u8>), String> {
        let cable = self.machine.document.usb_cable.0.clone();
        self.machine.session.logic_input(&cable, true)?;
        let (device, configuration) = self.enumerate_attached(ACTIVATION_BUDGET)?;
        if device[8..12] != VENDOR_PRODUCT {
            return Err(format!("unexpected device descriptor {}", encode_hex(&device)));
        }
        Ok((device, configuration))
    }

    /// Standard enumeration of whatever device the guest presents, once the
    /// controller activates within `budget` instructions.
    pub fn enumerate_attached(&mut self, budget: u64) -> Result<(Vec<u8>, Vec<u8>), String> {
        enumerate_port(self, budget)
    }
}

trait EnumerationPort {
    fn await_activation(&mut self, budget: u64) -> Result<(), String>;
    fn reset(&mut self) -> Result<(), String>;
    fn control_in(&mut self, setup: [u8; 8], count: usize) -> Result<Vec<u8>, String>;
    fn control_status(&mut self, setup: [u8; 8]) -> Result<(), String>;
    fn advance(&mut self, instructions: u64) -> Result<RunOutcome, String>;
    /// The board clock in ticks.
    fn now(&self) -> u64;
    /// Instructions per board tick as (numerator, denominator).
    fn rate(&self) -> (u64, u64);
}

impl EnumerationPort for Ff80 {
    fn await_activation(&mut self, budget: u64) -> Result<(), String> {
        self.wait("USB controller activation", budget, |host| Ok(host.status()?.register(0x090) & 6 == 6))
    }
    fn reset(&mut self) -> Result<(), String> {
        self.machine.session.usb_reset(&self.usb())
    }
    fn control_in(&mut self, setup: [u8; 8], count: usize) -> Result<Vec<u8>, String> {
        Ff80::control_in(self, setup, count)
    }
    fn control_status(&mut self, setup: [u8; 8]) -> Result<(), String> {
        Ff80::control_status(self, setup)
    }
    fn advance(&mut self, instructions: u64) -> Result<RunOutcome, String> {
        Ff80::advance(self, instructions)
    }
    fn now(&self) -> u64 {
        self.machine.session.board.now()
    }
    fn rate(&self) -> (u64, u64) {
        let rate = &self.machine.document.execution.instructions_per_tick;
        (rate.numerator, rate.denominator)
    }
}

fn enumerate_port(port: &mut impl EnumerationPort, budget: u64) -> Result<(Vec<u8>, Vec<u8>), String> {
    port.await_activation(budget)?;
    port.reset()?;
    port.advance(1_000_000)?;
    let device = port.control_in([0x80, 6, 0, 1, 0, 0, 18, 0], 18)?;
    if device.len() != 18 {
        return Err(format!("unexpected device descriptor {}", encode_hex(&device)));
    }
    port.control_status([0, 5, 1, 0, 0, 0, 0, 0])?;
    advance_ticks(port, SET_ADDRESS_RECOVERY_TICKS)?;
    let head = port.control_in([0x80, 6, 0, 2, 0, 0, 9, 0], 9)?;
    let total = head.get(2..4).map(|bytes| usize::from(u16::from_le_bytes([bytes[0], bytes[1]]))).unwrap_or(0);
    if head.len() != 9 || !(9..=256).contains(&total) {
        return Err(format!("unexpected configuration descriptor head {}", encode_hex(&head)));
    }
    let length = (total as u16).to_le_bytes();
    let configuration = port.control_in([0x80, 6, 0, 2, 0, 0, length[0], length[1]], total)?;
    if configuration.len() != total {
        return Err("configuration descriptor is truncated".into());
    }
    port.control_status([0, 9, configuration[5], 0, 0, 0, 0, 0])?;
    port.advance(1_000_000)?;
    Ok((device, configuration))
}

fn advance_ticks(port: &mut impl EnumerationPort, ticks: u64) -> Result<(), String> {
    let (numerator, denominator) = port.rate();
    let end = port.now().saturating_add(ticks);
    loop {
        let now = port.now();
        if now >= end {
            return Ok(());
        }
        if port.advance(((end - now) * numerator).div_ceil(denominator))? == RunOutcome::DrainRequired {
            return Err("SetAddress recovery wait: a capture page is outstanding".into());
        }
    }
}

impl Ff80 {
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

    pub fn latched_key(&mut self, key: RouteKey, pulse: KeyPulse) -> Result<(), String> {
        self.machine.press_latched(Press::Key(key), pulse)?;
        self.observe()
    }

    pub fn settled_key(&mut self, key: Press, pulse: KeyPulse, budget: u64) -> Result<crate::render::Settle, String> {
        let settle = crate::render::settled_key(&mut self.machine, key, pulse, budget)?;
        self.observe()?;
        Ok(settle)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SET_ADDRESS: [u8; 8] = [0, 5, 1, 0, 0, 0, 0, 0];
    const CONFIGURATION: [u8; 18] = [9, 2, 18, 0, 1, 1, 0, 0xc0, 0, 9, 4, 0, 0, 0, 0xff, 0, 0, 0];

    /// A device answering descriptors at once, on a board clock of 5 instructions per 2 ticks.
    struct Bench {
        instructions: u64,
        drain: bool,
        setups: Vec<([u8; 8], u64)>,
    }
    impl Bench {
        fn new() -> Self { Self { instructions: 0, drain: false, setups: Vec::new() } }
        fn tick_of(&self, setup: [u8; 8]) -> u64 {
            self.setups.iter().find(|(seen, _)| seen[..4] == setup[..4]).map(|(_, tick)| *tick).expect("request sent")
        }
    }
    impl EnumerationPort for Bench {
        fn await_activation(&mut self, _: u64) -> Result<(), String> { Ok(()) }
        fn reset(&mut self) -> Result<(), String> { Ok(()) }
        fn control_in(&mut self, setup: [u8; 8], count: usize) -> Result<Vec<u8>, String> {
            self.setups.push((setup, self.now()));
            Ok(if setup[3] == 1 { vec![0x12; 18] } else { CONFIGURATION[..count].to_vec() })
        }
        fn control_status(&mut self, setup: [u8; 8]) -> Result<(), String> {
            self.setups.push((setup, self.now()));
            Ok(())
        }
        fn advance(&mut self, instructions: u64) -> Result<RunOutcome, String> {
            self.instructions += instructions;
            Ok(if self.drain { RunOutcome::DrainRequired } else { RunOutcome::Completed })
        }
        fn now(&self) -> u64 { self.instructions * 2 / 5 }
        fn rate(&self) -> (u64, u64) { (5, 2) }
    }

    #[test]
    fn configuration_request_follows_set_address_by_ten_milliseconds_of_board_time() {
        let mut bench = Bench::new();
        let (_, configuration) = enumerate_port(&mut bench, 1).unwrap();
        assert_eq!(configuration, CONFIGURATION);
        assert_eq!(bench.tick_of([0x80, 6, 0, 2, 0, 0, 0, 0]) - bench.tick_of(SET_ADDRESS), 120_000);
    }

    #[test]
    fn an_outstanding_capture_page_ends_enumeration_before_the_configuration_request() {
        let mut bench = Bench::new();
        bench.drain = true;
        assert!(enumerate_port(&mut bench, 1).is_err());
        assert!(bench.setups.iter().all(|(setup, _)| setup[..4] != [0x80, 6, 0, 2]));
    }
}
