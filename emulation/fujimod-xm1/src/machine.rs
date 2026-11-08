//! A strict X-M1 session and its run document; runs record armed instruction
//! points and stop at the R4F guard's refusals.

use crate::frame_capture::CapturedFrame;
use crate::guest::{DialTurn, GpioKey, MenuState, ModeDialPosition, ModeDialWindow, PhysicalShutterPosition, Press, RouteKey, ShutterStage, set_adc};
use crate::r4f::Guard;
use crate::run::RunDocument;
use crate::sd_host::{Contacts, MediaRead, RetainedCard, SectorAddress, SectorCount, SdHost};
use crate::trace::{CaptureMemory, CaptureScope, CollectionOutcome, InstructionCount, TraceSites};
use fujimod_emulator::session::Session;
use fujimod_emulator_devices::exr2::scanout_frame::Frame as ScanoutFrame;
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::PathBuf;

const RELEASE_POLL: u64 = 250_000;
const PHASE_POLL: u64 = 20_000;
/// Longest run between two guard scans.
const GUARD_WINDOW: u64 = 1_000_000;


#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RunOutcome {
    Completed,
    Stopped,
    DrainRequired,
    Failed,
}
/// How long a press holds its input.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hold {
    Instructions(u64),
    /// Board ticks of guest time.
    BoardTicks(u64),
}

/// A hold, and a debounced-release cap in guest instructions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KeyPulse {
    pub hold: Hold,
    pub release: u64,
}

impl KeyPulse {
    pub const fn instructions(hold: u64, release: u64) -> Self {
        Self { hold: Hold::Instructions(hold), release }
    }
}

/// Board ticks between two X-M1 key-scanner samples (period argument 25, 25 ms at 12 MHz).
pub const SCAN_INTERVAL_TICKS: u64 = 300_000;

/// Two scan intervals, the longest an ADC key row waits for its second matching sample, plus
/// 120,000 ticks (10 ms); 2.4 of the 40 scans before rows 14..20 repeat.
pub const SCANNED_HOLD: Hold = Hold::BoardTicks(2 * SCAN_INTERVAL_TICKS + 120_000);

pub const MENU_PULSE: KeyPulse = KeyPulse { hold: SCANNED_HOLD, release: 6_000_000 };
pub const DIRECTIONAL_PULSE: KeyPulse = KeyPulse { hold: SCANNED_HOLD, release: 12_000_000 };

/// The default pulse of an ADC key.
pub fn key_pulse(key: RouteKey) -> KeyPulse {
    match key {
        RouteKey::Up | RouteKey::Down | RouteKey::Left | RouteKey::Right => DIRECTIONAL_PULSE,
        RouteKey::Ok | RouteKey::Back | RouteKey::Q => MENU_PULSE,
    }
}

/// A press's key-table row at the end of its hold.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Latch {
    Latched,
    /// The row read released at `hold_end` guest instructions.
    Unlatched { hold_end: u64 },
    /// The sub-command dial has no key-table row.
    Untracked,
}

#[derive(Clone, Debug, Serialize)]
#[serde(transparent)]
struct ReleaseRoute(String);

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(transparent)]
pub struct ReleaseSegment(String);

#[derive(Default, Debug, Serialize)]
struct ReleaseContext {
    route: Option<ReleaseRoute>,
    segment: Option<ReleaseSegment>,
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(transparent)]
struct GuestInstructions(u64);

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(transparent)]
struct KeyRow(u64);

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "kebab-case")]
enum GpioReleaseKey {
    Play,
    Fn,
    DialPush,
    DialLeft,
    DialRight,
    HalfPress,
    FullPress,
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(untagged)]
enum ReleaseKey {
    Adc(RouteKey),
    Gpio(GpioReleaseKey),
}

#[derive(Debug, Serialize)]
pub struct KeyReleaseObservation {
    route: Option<ReleaseRoute>,
    segment: Option<ReleaseSegment>,
    key: ReleaseKey,
    row: Option<KeyRow>,
    cap: GuestInstructions,
    instructions_used: GuestInstructions,
    instructions: GuestInstructions,
}

pub struct Machine {
    pub document: RunDocument,
    pub session: Session,
    /// The first strict stop; no input is admitted after it.
    pub failure: Option<String>,
    pub traces: TraceSites,
    pub render_provenance: Option<crate::render_provenance::Collector>,
    pub guard: Guard,
    pub key_release_observations: Vec<KeyReleaseObservation>,
    release_context: ReleaseContext,
    release_log: Option<PathBuf>,
}

pub fn instructions(session: &Session) -> u64 {
    session.board.instruction_counts().iter().sum()
}

impl Machine {
    /// A machine with the R4F guard armed.
    pub fn new(document: RunDocument, session: Session) -> Result<Self, String> {
        let mut guard = Guard::default();
        guard.arm(&session.board)?;
        Ok(Self {
            document, session, failure: None, traces: TraceSites::default(), guard,
            render_provenance: None,
            key_release_observations: Vec::new(),
            release_context: ReleaseContext {
                route: std::env::var("XM1_KEY_RELEASE_ROUTE").ok().map(ReleaseRoute),
                segment: None,
            },
            release_log: std::env::var_os("XM1_KEY_RELEASE_LOG").map(PathBuf::from),
        })
    }

    pub fn begin_render_provenance(&mut self) -> Result<(), String> {
        if self.render_provenance.is_some() { return Err("render provenance already active".into()); }
        let (_, graph) = crate::run::graph(&self.document)?;
        self.render_provenance = Some(crate::render_provenance::Collector::begin(&self.session, graph.mappings)?);
        self.traces.begin_presentation_tracking(&self.session)?;
        self.traces.render_object_provenance_sequence = Some(0);
        Ok(())
    }

    fn collect_render_provenance(&mut self) -> Result<(), String> {
        if let Some(collector) = &mut self.render_provenance {
            collector.collect(&self.session)?;
            self.traces.render_object_provenance_sequence = Some(collector.sequence());
        }
        Ok(())
    }

    pub(crate) fn begin_capture_observation(&mut self, scope: CaptureScope) -> Result<(), String> {
        if self.traces.needs_capture_memory(scope) {
            let (_, graph) = crate::run::graph(&self.document)?;
            self.traces.prepare_memory(&self.session, CaptureMemory::from_graph(graph)?);
        }
        self.traces.begin_capture_observation(scope)
    }

    pub fn set_release_context(&mut self, segment: Option<ReleaseSegment>) {
        self.release_context.segment = segment;
    }

    fn observe_incomplete_release(&mut self, key: ReleaseKey, row: Option<KeyRow>, cap: u64, start: u64) {
        let instructions = self.instructions();
        let observation = KeyReleaseObservation {
            route: self.release_context.route.clone(),
            segment: self.release_context.segment.clone(),
            key,
            row,
            cap: GuestInstructions(cap),
            instructions_used: GuestInstructions(instructions - start),
            instructions: GuestInstructions(instructions),
        };
        if let Some(path) = &self.release_log {
            let result = (|| -> Result<(), String> {
                let mut line = serde_json::to_vec(&observation).map_err(|error| error.to_string())?;
                line.push(b'\n');
                std::fs::OpenOptions::new().create(true).append(true).open(path)
                    .and_then(|mut file| file.write_all(&line)).map_err(|error| error.to_string())
            })();
            if let Err(error) = result {
                eprintln!("{}: key-release observation: {error}", path.display());
            }
        }
        self.key_release_observations.push(observation);
    }

    /// Removes host instrumentation before a session checkpoint.
    pub fn disarm(&mut self) -> Result<(), String> {
        if let Some(collector) = &mut self.render_provenance { collector.disarm(&self.session)?; }
        self.traces.teardown(&self.session)?;
        self.guard.disarm(&self.session.board)
    }

    pub fn rearm(&mut self) -> Result<(), String> {
        self.guard.arm(&self.session.board)?;
        if let Some(collector) = &mut self.render_provenance { collector.rearm(&self.session)?; }
        self.traces.reinstall(&self.session)
    }

    pub fn instructions(&self) -> u64 {
        instructions(&self.session)
    }

    /// Records every armed site entered, and returns early after a stop label's entry.
    pub fn run(&mut self, count: u64) -> RunOutcome {
        self.run_until(count, None)
    }

    /// Runs at most `count` instructions, ending at board tick `until` when given.
    pub fn run_until(&mut self, count: u64, until: Option<u64>) -> RunOutcome {
        if self.failure.is_some() {
            return RunOutcome::Failed;
        }
        if self.traces.capture_page_outstanding() {
            return RunOutcome::DrainRequired;
        }
        let mut current = match InstructionCount::from_board(&self.session) {
            Ok(current) => current,
            Err(error) => {
                self.failure = Some(error);
                return RunOutcome::Failed;
            }
        };
        let end = current.value().saturating_add(count);
        if self.traces.capture_observation_active() || self.traces.modal_observation_active() || self.traces.presentation_tracking_active() {
            match self.traces.collect(&self.session, current) {
                Ok(CollectionOutcome::DrainRequired) => return RunOutcome::DrainRequired,
                Ok(CollectionOutcome::Stop) => return RunOutcome::Stopped,
                Ok(CollectionOutcome::Continue) => {}
                Err(error) => {
                    self.failure = Some(error);
                    return RunOutcome::Failed;
                }
            }
        }
        loop {
            if current.value() >= end || until.is_some_and(|tick| self.session.board.now() >= tick) {
                return RunOutcome::Completed;
            }
            let window = end.min(current.value().saturating_add(GUARD_WINDOW));
            if let Some(collector) = &self.render_provenance {
                if let Err(error) = collector.begin_advance(&self.session) {
                    self.failure = Some(error);
                    return RunOutcome::Failed;
                }
            }
            self.traces.begin_advance();
            let result = self.session.board.run_window(window, self.document.execution.quantum, None, until);
            let refused = self.guard.scan(&self.session.board);
            if let Err(error) = refused.and(result) {
                self.failure = Some(error);
                return RunOutcome::Failed;
            }
            current = match InstructionCount::from_board(&self.session) {
                Ok(current) => current,
                Err(error) => {
                    self.failure = Some(error);
                    return RunOutcome::Failed;
                }
            };
            if let Err(error) = self.collect_render_provenance() {
                self.failure = Some(error);
                return RunOutcome::Failed;
            }
            match self.traces.collect(&self.session, current) {
                Ok(CollectionOutcome::Stop) => return RunOutcome::Stopped,
                Ok(CollectionOutcome::DrainRequired) => return RunOutcome::DrainRequired,
                Ok(CollectionOutcome::Continue) => {}
                Err(error) => {
                    self.failure = Some(error);
                    return RunOutcome::Failed;
                }
            }
        }
    }

    /// The instructions `ticks` board ticks take with no core waiting.
    pub fn hold_bound(&self, ticks: u64) -> u64 {
        let rate = &self.document.execution.instructions_per_tick;
        (ticks * rate.numerator).div_ceil(rate.denominator)
    }

    fn hold(&mut self, hold: Hold) {
        match hold {
            Hold::Instructions(count) => {
                self.run(count);
            }
            Hold::BoardTicks(ticks) => {
                let until = self.session.board.now().saturating_add(ticks);
                self.run_until(self.hold_bound(ticks), Some(until));
            }
        }
    }

    pub fn dial_push(&mut self, pulse: KeyPulse) -> Result<Latch, String> {
        self.require_running()?;
        let key = self.document.firmware.dial_push(&self.session)?;
        self.gpio_press(key, GpioReleaseKey::DialPush, pulse)
    }

    pub fn play(&mut self, pulse: KeyPulse) -> Result<Latch, String> {
        self.require_running()?;
        let key = self.document.firmware.play(&self.session)?;
        self.gpio_press(key, GpioReleaseKey::Play, pulse)
    }

    pub fn fn_key(&mut self, pulse: KeyPulse) -> Result<Latch, String> {
        self.require_running()?;
        let key = self.document.firmware.fn_key(&self.session)?;
        self.gpio_press(key, GpioReleaseKey::Fn, pulse)
    }

    pub fn shutter(&mut self, stage: ShutterStage, pulse: KeyPulse) -> Result<Latch, String> {
        self.require_running()?;
        let key = self.document.firmware.shutter(&self.session, stage)?;
        let release = match stage {
            ShutterStage::Half => GpioReleaseKey::HalfPress,
            ShutterStage::Full => GpioReleaseKey::FullPress,
        };
        self.gpio_press(key, release, pulse)
    }

    pub fn set_shutter_position(&mut self, position: PhysicalShutterPosition) -> Result<(), String> {
        self.require_running()?;
        let half = self.document.firmware.shutter(&self.session, ShutterStage::Half)?;
        let full = self.document.firmware.shutter(&self.session, ShutterStage::Full)?;
        let gpio = &self.document.gpio.0;
        let mut drive = |key: GpioKey, pressed| {
            self.session.gpio(gpio, key.port, key.pin, if pressed { key.pressed_high } else { !key.pressed_high })
        };
        match position {
            PhysicalShutterPosition::Released => {
                drive(full, false)?;
                drive(half, false)
            }
            PhysicalShutterPosition::Half => {
                drive(full, false)?;
                drive(half, true)
            }
            PhysicalShutterPosition::Full => {
                drive(half, true)?;
                drive(full, true)
            }
        }
    }

    /// Releases the pad after the hold and waits for the row's debounced release byte.
    fn gpio_press(&mut self, key: GpioKey, release: GpioReleaseKey, pulse: KeyPulse) -> Result<Latch, String> {
        let released = self.document.firmware.key_table.base.0
            + key.row * self.document.firmware.key_table.row_bytes
            + self.document.firmware.key_table.released_offset;
        self.session.gpio(&self.document.gpio.0, key.port, key.pin, key.pressed_high)?;
        self.hold(pulse.hold);
        self.session.gpio(&self.document.gpio.0, key.port, key.pin, !key.pressed_high)?;
        let start = self.instructions();
        let mut sampled = [0];
        self.session.board.read_physical(released, &mut sampled)?;
        let latch = if sampled[0] == 0 { Latch::Unlatched { hold_end: start } } else { Latch::Latched };
        while self.failure.is_none() {
            self.session.board.read_physical(released, &mut sampled)?;
            if sampled[0] == 0 {
                break;
            }
            let spent = self.instructions() - start;
            if spent >= pulse.release {
                self.observe_incomplete_release(ReleaseKey::Gpio(release), Some(KeyRow(key.row)), pulse.release, start);
                break;
            }
            self.run((pulse.release - spent).min(RELEASE_POLL));
        }
        self.require_running()?;
        Ok(latch)
    }

    pub fn require_running(&self) -> Result<(), String> {
        match &self.failure {
            Some(failure) => Err(format!("the machine stopped: {failure}")),
            None => Ok(()),
        }
    }

    pub fn menu(&self) -> Result<Option<MenuState>, String> {
        self.document.firmware.menu(&self.session)
    }

    pub fn press_once(&mut self, press: Press, pulse: KeyPulse) -> Result<Latch, String> {
        match press {
            Press::Key(key) => self.key(key, pulse),
            Press::Dial(turn) => self.turn_dial(turn, pulse).map(|()| Latch::Untracked),
            Press::Shutter(stage) => self.shutter(stage, pulse),
            Press::Play => self.play(pulse),
            Press::Fn => self.fn_key(pulse),
        }
    }

    /// Presses once and fails when the key-table row read released at the end of the hold.
    pub fn press_latched(&mut self, press: Press, pulse: KeyPulse) -> Result<(), String> {
        let latch = self.press_once(press, pulse)?;
        self.require_running()?;
        match latch {
            Latch::Unlatched { hold_end } => Err(format!(
                "{press:?} press was not latched: its key row read released when the hold ended, at instruction {hold_end}")),
            Latch::Latched | Latch::Untracked => Ok(()),
        }
    }

    /// Drives the dial's four phases, each within a quarter of `pulse.hold` and refused unless
    /// the scanner samples it, then waits up to `pulse.release` for the released phase.
    pub fn turn_dial(&mut self, turn: DialTurn, pulse: KeyPulse) -> Result<(), String> {
        if self.failure.is_some() {
            return Err("Restore the checkpoint before another input after a strict stop".into());
        }
        self.document.firmware.subcommand_ready(&self.session)?;
        const RELEASED: u8 = 0b11;
        if self.document.firmware.subcommand_sampled(&self.session)? != RELEASED {
            return Err("the sub-command scanner has not sampled released pads".into());
        }
        let (port, pins) = (self.document.firmware.subcommand.port, self.document.firmware.subcommand.pins);
        let gpio = self.document.gpio.0.clone();
        let Hold::Instructions(hold) = pulse.hold else {
            return Err("the sub-command dial takes an instruction hold".into());
        };
        let phase_budget = hold / 4;
        for (pin8, pin9) in turn.phases() {
            self.session.gpio(&gpio, port, pins[0], pin8)?;
            self.session.gpio(&gpio, port, pins[1], pin9)?;
            let phase = u8::from(pin8) << 1 | u8::from(pin9);
            if !self.run_until_sampled(phase, phase_budget)? {
                return Err(format!("the sub-command scanner did not sample phase {phase:02b}"));
            }
        }
        let start = self.instructions();
        if !self.run_until_sampled(RELEASED, pulse.release)? {
            let key = match turn {
                DialTurn::Left => GpioReleaseKey::DialLeft,
                DialTurn::Right => GpioReleaseKey::DialRight,
            };
            self.observe_incomplete_release(ReleaseKey::Gpio(key), None, pulse.release, start);
        }
        self.require_running()
    }

    fn run_until_sampled(&mut self, phase: u8, budget: u64) -> Result<bool, String> {
        let start = self.instructions();
        loop {
            self.require_running()?;
            if self.document.firmware.subcommand_sampled(&self.session)? == phase {
                return Ok(true);
            }
            let spent = self.instructions() - start;
            if spent >= budget {
                return Ok(false);
            }
            self.run((budget - spent).min(PHASE_POLL));
        }
    }

    pub fn key(&mut self, key: RouteKey, pulse: KeyPulse) -> Result<Latch, String> {
        if self.failure.is_some() {
            return Err("Restore the checkpoint before another input after a strict stop".into());
        }
        let (channel, sample) = self.document.firmware.pressed_sample(&self.session, key)?;
        set_adc(&mut self.session, &self.document.adc, channel, sample)?;
        self.hold(pulse.hold);
        let released = self.document.firmware.released_sample;
        set_adc(&mut self.session, &self.document.adc, channel, released)?;
        let start = self.instructions();
        let latch = if self.document.firmware.released(&self.session, key)? { Latch::Unlatched { hold_end: start } } else { Latch::Latched };
        while self.failure.is_none() && !self.document.firmware.released(&self.session, key)? {
            let spent = self.instructions() - start;
            if spent >= pulse.release {
                let row = KeyRow(self.document.firmware.binding(key)?.row);
                self.observe_incomplete_release(ReleaseKey::Adc(key), Some(row), pulse.release, start);
                break;
            }
            self.run((pulse.release - spent).min(RELEASE_POLL));
        }
        Ok(latch)
    }

    /// Holds the ADC key `key` pressed until `release_key`; returns the held sample.
    pub fn press(&mut self, key: RouteKey) -> Result<u16, String> {
        let (channel, sample) = self.document.firmware.pressed_sample(&self.session, key)?;
        set_adc(&mut self.session, &self.document.adc, channel, sample)?;
        Ok(sample)
    }

    pub fn release_key(&mut self, key: RouteKey) -> Result<(), String> {
        let channel = self.document.firmware.binding(key)?.channel;
        let released = self.document.firmware.released_sample;
        set_adc(&mut self.session, &self.document.adc, channel, released)
    }

    /// Holds the mode-dial ADC channel at the position's calibrated midpoint.
    pub fn set_mode_dial(&mut self, position: ModeDialPosition) -> Result<ModeDialWindow, String> {
        if self.failure.is_some() {
            return Err("Restore the checkpoint before another input after a strict stop".into());
        }
        let window = self.document.firmware.mode_dial_window(&self.session, position)?;
        let channel = self.document.firmware.mode_dial.channel;
        set_adc(&mut self.session, &self.document.adc, channel, window.midpoint())?;
        Ok(window)
    }

    /// PNG of the first scanout output presenting configured graphics.
    pub fn frame(&self) -> Result<Option<Vec<u8>>, String> {
        let Some((_, _, frame)) = self.observed_frame()? else {
            return Ok(None);
        };
        let rgb = frame.rgb.ok_or("scanout did not return RGB")?;
        let width = u32::try_from(frame.width).map_err(|_| "frame width")?;
        let height = u32::try_from(frame.height).map_err(|_| "frame height")?;
        Ok(Some(fujimod_recording::Frame::rgb(width, height, rgb)?.png()?.to_vec()))
    }

    pub fn capture_frame(&self) -> Result<CapturedFrame, String> {
        let (state, instance, frame) = self.observed_frame()?.ok_or("no display plane to capture")?;
        CapturedFrame::new(self.document.scanout.clone(), instance, InstructionCount::from_board(&self.session)?, state, frame)
    }

    pub fn presents_display_plane(&self) -> Result<bool, String> {
        Ok(self.observed_frame()?.is_some())
    }

    pub fn select_presentation_target(&mut self) -> Result<Option<crate::presentation::FrameToken>, String> {
        self.traces.select_presentation_target()
    }

    pub fn presentation_acknowledgement(&self, token: crate::presentation::FrameToken) -> Result<Option<crate::presentation::Acknowledgement>, String> {
        self.traces.presentation_acknowledgement(token)
    }

    fn observed_frame(&self) -> Result<Option<(serde_json::Value, usize, ScanoutFrame)>, String> {
        let name = &self.document.scanout.0;
        let state = self.session.scanout_state(name)?;
        let instances = state["instances"].as_array().into_iter().flatten();
        let graphics = instances.clone().find(|instance| {
            instance["graphics"]
                .as_array()
                .into_iter()
                .flatten()
                .flat_map(|layer| layer["channels"].as_array().into_iter().flatten())
                .any(|channel| {
                    channel["enabled"] == true
                        && channel["configured"] == true
                        && channel["width"].as_u64().is_some_and(|n| n > 1)
                        && channel["height"].as_u64().is_some_and(|n| n > 1)
                })
        });
        if let Some(metadata) = graphics {
            if let Some(instance) = metadata["instance"].as_u64() {
                let instance = usize::try_from(instance).map_err(|_| "scanout instance")?;
                if let Some(frame) = self.session.scanout(name, instance, true, false)? {
                    return Ok(Some((state, instance, frame)));
                }
            }
        }
        for metadata in instances {
            let instance = metadata["instance"].as_u64().ok_or_else(|| format!("scanout instance missing; native_scanout_state={state}"))?;
            let instance = usize::try_from(instance).map_err(|_| "scanout instance")?;
            if let Some(frame) = self.session.scanout(name, instance, true, false)? {
                return Ok(Some((state, instance, frame)));
            }
        }
        Ok(None)
    }

    pub fn read(&self, address: u64, count: usize) -> Result<Vec<u8>, String> {
        let mut bytes = vec![0; count];
        self.session.board.read_physical(address, &mut bytes)?;
        Ok(bytes)
    }

    pub fn word(&self, address: u64) -> Result<u32, String> {
        let bytes = self.read(address, 4)?;
        Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    fn card_contacts(&self) -> Result<Contacts, String> {
        let node = self.session.node(&self.document.sd.0)?;
        self.session.board.inspect_device::<SdHost, _>(node, |host| Ok(host.contacts()))
    }

    /// Inserts `card` into the unexecuted SD host and requests card detect low; the GPIO
    /// applies the request once the board runs.
    pub fn insert_card(&mut self, card: RetainedCard) -> Result<(), String> {
        let node = self.session.node(&self.document.sd.0)?;
        self.session.board.edit_device::<SdHost, _>(node, |host| host.insert(card))?;
        let Contacts { gpio, detect, .. } = self.card_contacts()?;
        self.session.gpio(&gpio.0, detect.port, detect.pin, false)
    }

    pub fn card_present(&self) -> Result<bool, String> {
        let node = self.session.node(&self.document.sd.0)?;
        self.session.board.inspect_device::<SdHost, _>(node, |host| host.present())
    }

    /// Drives the SD host's write-protect pad.
    pub fn protect_card(&mut self, protected: bool) -> Result<(), String> {
        let Contacts { gpio, protect, .. } = self.card_contacts()?;
        self.session.gpio(&gpio.0, protect.port, protect.pin, protected)
    }

    /// The SD host's diagnostic state: presence, card state, counters and recent commands.
    pub fn card_state(&self) -> Result<serde_json::Value, String> {
        self.session.board.device_save(self.session.node(&self.document.sd.0)?)
    }

    pub fn read_media(&self, first: SectorAddress, count: SectorCount) -> Result<MediaRead, String> {
        let node = self.session.node(&self.document.sd.0)?;
        self.session.board.inspect_device::<SdHost, _>(node, |host| host.read_sectors(first, count))
    }

    pub fn read_nand(&self, first: crate::nand_read::RowAddress, count: crate::nand_read::RowCount) -> Result<crate::nand_read::NandRead, String> {
        use fujimod_emulator_devices::capability::NandStore;
        self.session.board.inspect_capability::<dyn NandStore, _>(
            self.session.node(&self.document.nand.0)?,
            |controller| crate::nand_read::read(controller.medium(), first, count),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::run::ValidatedUpdate;

    const RUN: &[u8] = include_bytes!("../data/xm1-150.json");
    const ROM_BYTES: usize = 16384;
    const SPIN: u32 = 0xeaff_fffe;

    fn machine() -> Machine {
        machine_with_code(&[SPIN])
    }

    fn machine_with_code(code: &[u32]) -> Machine {
        fujimod_emulator_exr2_board::register_board_builder();
        fujimod_emulator_models::register();
        let document = RunDocument::parse(RUN).unwrap();
        let mut rom = vec![0u8; ROM_BYTES];
        for (bytes, instruction) in rom.chunks_exact_mut(4).zip(code) {
            bytes.copy_from_slice(&instruction.to_le_bytes());
        }
        let update = ValidatedUpdate(vec![0xff; document.update_flash_offset + 2048]);
        let mut session = crate::run::open(&document, &update, &rom).unwrap();
        session.board.set_execution_policy(document.execution_policy.into()).unwrap();
        Machine::new(document, session).unwrap()
    }

    #[test]
    fn render_provenance_refuses_unrelated_code_at_source_address() {
        use fujimod_emulator_cpu::Register;
        let mut machine = machine();
        machine.begin_render_provenance().unwrap();
        machine.session.board.write_physical(0x6dcc44, &SPIN.to_le_bytes()).unwrap();
        machine.session.board.write_register(0, Register::Pc, 0x6dcc44).unwrap();
        assert_eq!(machine.run(100), RunOutcome::Failed);
        assert_eq!(render_batch(&mut machine)["coverage"], "refused");
    }

    fn render_event(machine: &mut Machine, site: u32, base: u32, object: u32) {
        use fujimod_emulator_cpu::{Register, arm};
        let branch = 0xea00_0000 | ((0u32.wrapping_sub(site + 12) >> 2) & 0x00ff_ffff);
        let source: u32 = match site {
            0x6dcc44 | 0x6dce5c | 0x6dcf20 => 0xe2505000,
            0x117d804 | 0x117d760 => 0xe5840004,
            0x89cdc4 => 0xe92d4008,
            _ => panic!("unknown fixture site"),
        };
        machine.session.board.write_physical(u64::from(site), &source.to_le_bytes()).unwrap();
        machine.session.board.write_physical(u64::from(site + 4), &branch.to_le_bytes()).unwrap();
        for (register, value) in [(Register::Pc, site), (Register::Arm32(arm::Register::R(0)), base),
            (Register::Arm32(arm::Register::R(4)), object), (Register::Arm32(arm::Register::R(13)), 0x00a00000)] {
            machine.session.board.write_register(0, register, u64::from(value)).unwrap();
        }
        assert_eq!(machine.run(100), RunOutcome::Completed);
    }

    fn render_batch(machine: &mut Machine) -> serde_json::Value {
        serde_json::to_value(machine.render_provenance.as_mut().unwrap().drain(&machine.session).unwrap()).unwrap()
    }

    #[test]
    fn render_provenance_covers_idle_gaps_and_preserves_free_reuse_identity() {
        let mut machine = machine();
        machine.begin_render_provenance().unwrap();
        render_event(&mut machine, 0x6dcc44, 0x01000000, 0);
        render_event(&mut machine, 0x117d804, 0x01000000, 0x02000000);
        let first = render_batch(&mut machine);
        assert_eq!(first["sequence_start"], 0);
        assert_eq!(first["sequence_end"], 2);
        assert_eq!(first["events"][0]["ordinal"], 1);
        assert_eq!(machine.run(100), RunOutcome::Completed);
        render_event(&mut machine, 0x89cdc4, 0x01000000, 0);
        render_event(&mut machine, 0x6dcc44, 0x01000000, 0);
        let second = render_batch(&mut machine);
        assert_eq!(second["start"], first["end"]);
        assert_eq!(second["sequence_start"], 2);
        assert_eq!(second["sequence_end"], 4);
        assert_eq!(second["coverage"], "continuous");
        assert_eq!(second["events"][0]["kind"], "free");
        assert_eq!(second["events"][1]["ordinal"], 2);
        assert_eq!(second["events"][1]["allocation_base"], "0x01000000");
        let empty = render_batch(&mut machine);
        assert_eq!(empty["sequence_start"], 4);
        assert_eq!(empty["sequence_end"], 4);
        assert_eq!(empty["events"], serde_json::json!([]));
    }

    #[test]
    fn render_trace_hits_carry_the_exact_provenance_boundary() {
        let mut machine = machine();
        machine.begin_render_provenance().unwrap();
        machine.traces.arm(&machine.session, [(0x117d804, "bind".into())].into()).unwrap();
        render_event(&mut machine, 0x6dcc44, 0x01000000, 0);
        render_event(&mut machine, 0x117d804, 0x01000000, 0x02000000);
        let batch = render_batch(&mut machine);
        let hit = serde_json::to_value(&machine.traces.hits[0]).unwrap();
        assert_eq!(hit["render_object_provenance_sequence"], 2);
        assert_eq!(hit["instructions"], batch["events"][1]["instructions"]);
    }

    fn presentation_event(machine: &mut Machine, site: crate::presentation::Site, result: u32) -> RunOutcome {
        use fujimod_emulator_cpu::{Register, arm};
        let pc = site.address();
        let branch = 0xea00_0000 | ((0u32.wrapping_sub(pc + 12) >> 2) & 0x00ff_ffff);
        machine.session.board.write_physical(u64::from(pc), &site.word().to_le_bytes()).unwrap();
        machine.session.board.write_physical(u64::from(pc + 4), &branch.to_le_bytes()).unwrap();
        let fp = if site == crate::presentation::Site::Enter { 0x12345678 } else { 0x009ffffc };
        for (register, value) in [
            (Register::Pc, pc), (Register::Arm32(arm::Register::R(13)), 0x00a00000),
            (Register::Arm32(arm::Register::R(11)), fp),
            (Register::Arm32(arm::Register::R(0)), result), (Register::Arm32(arm::Register::R(4)), result),
        ] {
            machine.session.board.write_register(0, register, u64::from(value)).unwrap();
        }
        machine.run(100)
    }

    #[test]
    fn presentation_completion_preserves_overlapping_user_capture_across_a_page_retry() {
        use crate::presentation::Site;
        use crate::trace::{CaptureCursor, CaptureScope};
        let mut machine = machine();
        machine.begin_render_provenance().unwrap();
        machine.traces.arm(&machine.session, Site::ALL.map(|site| (site.address(), format!("user-{:x}", site.address()))).into()).unwrap();
        machine.begin_capture_observation(CaptureScope::Capture).unwrap();
        for _ in 0..64 {
            for site in Site::ALL {
                assert_eq!(presentation_event(&mut machine, site, 0), RunOutcome::Completed);
            }
        }
        assert_eq!(presentation_event(&mut machine, Site::Enter, 0), RunOutcome::DrainRequired);
        let target = machine.select_presentation_target().unwrap().unwrap();
        assert!(machine.presentation_acknowledgement(target).unwrap().is_none());
        let page = serde_json::to_value(machine.traces.capture_observation_page(CaptureCursor::new(0)).unwrap()).unwrap();
        let mut hits = page["hits"].as_array().unwrap().clone();
        let mut cursor = page["next"].as_u64().unwrap();
        machine.traces.acknowledge_capture_observation(CaptureCursor::new(cursor)).unwrap();
        assert_eq!(machine.run(100), RunOutcome::Completed);
        for site in [Site::AfterWait, Site::Finish] {
            assert_eq!(presentation_event(&mut machine, site, 0), RunOutcome::Completed);
        }
        loop {
            let page = serde_json::to_value(machine.traces.capture_observation_page(CaptureCursor::new(cursor)).unwrap()).unwrap();
            let entries = page["hits"].as_array().unwrap();
            if entries.is_empty() { break; }
            hits.extend(entries.iter().cloned());
            cursor = page["next"].as_u64().unwrap();
            machine.traces.acknowledge_capture_observation(CaptureCursor::new(cursor)).unwrap();
        }
        assert_eq!(hits.len(), 195);
        for frame in hits.chunks_exact(3) {
            assert_eq!(frame.iter().map(|hit| hit["site"].as_str().unwrap()).collect::<Vec<_>>(),
                ["user-6d7920", "user-6d7da4", "user-6d7df4"]);
        }
        let proof = serde_json::to_value(machine.presentation_acknowledgement(target).unwrap().unwrap()).unwrap();
        assert_eq!(proof["selected_frame"], 65);
        assert_eq!(proof["completed_frame"], 65);
        machine.disarm().unwrap();
        machine.rearm().unwrap();
        assert!(machine.select_presentation_target().unwrap().is_none());
    }

    fn write_row(machine: &Machine, row: u64, record: &[u8; 28]) {
        let table = &machine.document.firmware.key_table;
        machine.session.board.write_physical(table.base.0 + row * table.row_bytes, record).unwrap();
    }

    fn play_record(released_byte: u8) -> [u8; 28] {
        let mut record = [0u8; 28];
        record[0..4].copy_from_slice(&11u32.to_le_bytes());
        record[9] = 2;
        record[12..16].copy_from_slice(&(2u32 << 16 | 9).to_le_bytes());
        record[21] = released_byte;
        record
    }

    fn full_press_record(released_byte: u8) -> [u8; 28] {
        let mut record = [0u8; 28];
        record[0..4].copy_from_slice(&8u32.to_le_bytes());
        record[9] = 4;
        record[12..16].copy_from_slice(&3u32.to_le_bytes());
        record[16..18].copy_from_slice(&1u16.to_le_bytes());
        record[18..20].copy_from_slice(&33u16.to_le_bytes());
        record[21] = released_byte;
        record
    }

    fn fn_record(released_byte: u8) -> [u8; 28] {
        let mut record = [0u8; 28];
        record[0..4].copy_from_slice(&0x28u32.to_le_bytes());
        record[9] = 2;
        record[12..16].copy_from_slice(&11u32.to_le_bytes());
        record[16] = 1;
        record[20] = 2;
        record[21] = released_byte;
        record
    }

    #[test]
    fn fn_drives_gpio0_11_low_during_hold_and_high_on_release() {
        let mut machine = machine_with_code(&[
            0xe59f_000c, 0xe59f_200c, 0xe590_1000, 0xe582_1000, 0xeaff_fffc,
            0xfff8_9014, 0x00a0_0000,
        ]);
        write_row(&machine, 13, &fn_record(0));
        let mut word = [0u8; 4];
        machine.run(100);
        machine.require_running().unwrap();
        machine.session.board.read_physical(0x00a0_0000, &mut word).unwrap();
        assert_ne!(u32::from_le_bytes(word) & (1 << 11), 0);
        let start = machine.session.board.now();
        let latch = machine.press_once(Press::Fn, KeyPulse { hold: Hold::BoardTicks(10_000), release: 0 }).unwrap();
        assert!((10_000..10_100).contains(&(machine.session.board.now() - start)));
        machine.session.board.read_physical(0x00a0_0000, &mut word).unwrap();
        assert_eq!(u32::from_le_bytes(word) & (1 << 11), 0);
        machine.run(100);
        machine.require_running().unwrap();
        machine.session.board.read_physical(0x00a0_0000, &mut word).unwrap();
        assert_ne!(u32::from_le_bytes(word) & (1 << 11), 0);
        assert!(matches!(latch, Latch::Unlatched { .. }));
    }

    #[test]
    fn fn_requires_a_latched_row_and_observes_incomplete_release() {
        let mut machine = machine();
        write_row(&machine, 13, &fn_record(0));
        assert!(machine.press_latched(Press::Fn, KeyPulse::instructions(0, 3)).is_err());
        write_row(&machine, 13, &fn_record(1));
        machine.press_latched(Press::Fn, KeyPulse::instructions(0, 3)).unwrap();
        let observed = serde_json::to_value(&machine.key_release_observations).unwrap();
        assert_eq!(observed[0]["key"], "fn");
        assert_eq!(observed[0]["row"], 13);
        assert_eq!(observed[0]["cap"], 3);
    }

    #[test]
    fn full_press_reports_its_row_and_bounded_release_cap() {
        let mut machine = machine();
        let row = machine.document.firmware.shutter.full.row.get();
        write_row(&machine, row, &full_press_record(1));
        machine.shutter(ShutterStage::Full, KeyPulse::instructions(0, 3)).unwrap();
        let observed = serde_json::to_value(&machine.key_release_observations).unwrap();
        assert_eq!(observed.as_array().unwrap().len(), 1);
        assert_eq!(observed[0]["key"].as_str(), Some("full-press"));
        assert_eq!(observed[0]["row"], row);
        assert_eq!(observed[0]["cap"], 3u64);
        assert_eq!(observed[0]["instructions_used"], 3u64);
    }

    #[test]
    fn a_row_outside_its_declared_binding_is_refused() {
        let mut machine = machine();
        let row = machine.document.firmware.play.row.get();
        let mut record = play_record(0);
        record[9] = 3;
        write_row(&machine, row, &record);
        assert!(machine.play(KeyPulse::instructions(0, 0)).is_err());
    }

    /// Registers the scanner and loads disjoint calibrated intervals, each row reading
    /// `released_byte` at `+0x15`.
    fn adc_keys(machine: &Machine, released_byte: u8) {
        const CALIBRATION: u64 = 0x00a0_0000;
        let firmware = &machine.document.firmware;
        let board = &machine.session.board;
        board.write_physical(firmware.scanner_registered.0, &1u32.to_le_bytes()).unwrap();
        board.write_physical(firmware.calibration_pointer.0, &(CALIBRATION as u32).to_le_bytes()).unwrap();
        for (index, binding) in firmware.keys.iter().enumerate() {
            let low = 100 * index as u16 + 10;
            board.write_physical(CALIBRATION + u64::from(binding.low_tag), &low.to_le_bytes()).unwrap();
            board.write_physical(CALIBRATION + u64::from(binding.high_tag), &(low + 50).to_le_bytes()).unwrap();
            let mut record = [0u8; 28];
            record[0..4].copy_from_slice(&binding.code.to_le_bytes());
            record[9] = 1;
            record[12..16].copy_from_slice(&(binding.channel as u32).to_le_bytes());
            record[16..18].copy_from_slice(&binding.low_tag.to_le_bytes());
            record[18..20].copy_from_slice(&binding.high_tag.to_le_bytes());
            record[21] = released_byte;
            write_row(machine, binding.row, &record);
        }
    }

    #[test]
    fn a_press_whose_row_never_latches_fails_at_that_press() {
        let mut machine = machine();
        adc_keys(&machine, 0);
        assert!(machine.press_latched(Press::Key(RouteKey::Ok), KeyPulse::instructions(0, 3)).is_err());
    }

    #[test]
    fn a_press_whose_row_latches_passes() {
        let mut machine = machine();
        adc_keys(&machine, 1);
        machine.press_latched(Press::Key(RouteKey::Ok), KeyPulse::instructions(0, 3)).unwrap();
    }

    #[test]
    fn a_gpio_press_whose_row_never_latches_fails_at_that_press() {
        let mut machine = machine();
        let row = machine.document.firmware.shutter.full.row.get();
        write_row(&machine, row, &full_press_record(0));
        assert!(machine.press_latched(Press::Shutter(ShutterStage::Full), KeyPulse::instructions(0, 3)).is_err());
    }

    #[test]
    fn a_board_tick_hold_holds_for_that_guest_time() {
        let mut machine = machine();
        let row = machine.document.firmware.shutter.full.row.get();
        write_row(&machine, row, &full_press_record(0));
        let ticks = machine.session.board.now();
        machine.press_once(Press::Shutter(ShutterStage::Full), KeyPulse { hold: Hold::BoardTicks(10_000), release: 1 }).unwrap();
        let held = machine.session.board.now() - ticks;
        assert!((10_000..10_100).contains(&held), "held {held} ticks");
    }
}
