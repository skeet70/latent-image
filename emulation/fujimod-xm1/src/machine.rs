//! A strict X-M1 session and its run document; runs record armed instruction
//! points and stop at the R4F guard's refusals.

use crate::guest::{DialTurn, MenuState, ModeDialPosition, ModeDialWindow, Press, RouteKey, set_adc};
use crate::r4f::Guard;
use crate::run::RunDocument;
use crate::sd_host::{Contacts, RetainedCard, SdHost};
use crate::trace::TraceSites;
use fujimod_emulator::session::Session;
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::PathBuf;

const RELEASE_POLL: u64 = 250_000;
const PHASE_POLL: u64 = 20_000;
/// Longest run between two guard scans.
const GUARD_WINDOW: u64 = 1_000_000;

/// Fixed hold and debounced-release cap in guest instructions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KeyPulse {
    pub hold: u64,
    pub release: u64,
}

pub const MENU_PULSE: KeyPulse = KeyPulse { hold: 4_000_000, release: 6_000_000 };

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
    DialPush,
    DialLeft,
    DialRight,
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
            key_release_observations: Vec::new(),
            release_context: ReleaseContext {
                route: std::env::var("XM1_KEY_RELEASE_ROUTE").ok().map(ReleaseRoute),
                segment: None,
            },
            release_log: std::env::var_os("XM1_KEY_RELEASE_LOG").map(PathBuf::from),
        })
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
        self.traces.disarm(&self.session)?;
        self.guard.disarm(&self.session.board)
    }

    pub fn rearm(&mut self) -> Result<(), String> {
        self.guard.arm(&self.session.board)?;
        self.traces.rearm(&self.session)
    }

    pub fn instructions(&self) -> u64 {
        instructions(&self.session)
    }

    /// Records every armed site entered, and returns early after a stop label's entry.
    pub fn run(&mut self, count: u64) {
        if self.failure.is_some() || count == 0 {
            return;
        }
        let end = self.instructions().saturating_add(count);
        while self.failure.is_none() && self.instructions() < end {
            let window = end.min(self.instructions().saturating_add(GUARD_WINDOW));
            let result = self.session.board.run_window(window, self.document.execution.quantum, None, None);
            let refused = self.guard.scan(&self.session.board);
            if let Err(error) = refused.and(result) {
                self.failure = Some(error);
                break;
            }
            match self.traces.collect(&self.session) {
                Ok(true) => break,
                Ok(false) => {}
                Err(error) => self.failure = Some(error),
            }
        }
    }

    pub fn dial_push(&mut self, pulse: KeyPulse) -> Result<(), String> {
        self.require_running()?;
        let key = self.document.firmware.dial_push(&self.session)?;
        let (port, pin, pressed, row) = (key.port, key.pin, key.pressed_high, KeyRow(key.row));
        let gpio = self.document.gpio.0.clone();
        self.session.gpio(&gpio, port, pin, pressed)?;
        self.run(pulse.hold);
        self.session.gpio(&gpio, port, pin, !pressed)?;
        let start = self.instructions();
        while self.failure.is_none() && !self.document.firmware.dial_push_released(&self.session)? {
            let spent = self.instructions() - start;
            if spent >= pulse.release {
                self.observe_incomplete_release(ReleaseKey::Gpio(GpioReleaseKey::DialPush), Some(row), pulse.release, start);
                break;
            }
            self.run((pulse.release - spent).min(RELEASE_POLL));
        }
        self.require_running()
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

    pub fn press_once(&mut self, press: Press, pulse: KeyPulse) -> Result<(), String> {
        match press {
            Press::Key(key) => self.key(key, pulse),
            Press::Dial(turn) => self.turn_dial(turn, pulse),
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
        let phase_budget = pulse.hold / 4;
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

    pub fn key(&mut self, key: RouteKey, pulse: KeyPulse) -> Result<(), String> {
        if self.failure.is_some() {
            return Err("Restore the checkpoint before another input after a strict stop".into());
        }
        let (channel, sample) = self.document.firmware.pressed_sample(&self.session, key)?;
        set_adc(&mut self.session, &self.document.adc, channel, sample)?;
        self.run(pulse.hold);
        let released = self.document.firmware.released_sample;
        set_adc(&mut self.session, &self.document.adc, channel, released)?;
        let start = self.instructions();
        while self.failure.is_none() && !self.document.firmware.released(&self.session, key)? {
            let spent = self.instructions() - start;
            if spent >= pulse.release {
                let row = KeyRow(self.document.firmware.binding(key)?.row);
                self.observe_incomplete_release(ReleaseKey::Adc(key), Some(row), pulse.release, start);
                break;
            }
            self.run((pulse.release - spent).min(RELEASE_POLL));
        }
        Ok(())
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
        let name = &self.document.scanout.0;
        let state = self.session.scanout_state(name)?;
        let presented = state["instances"].as_array().into_iter().flatten().find(|instance| {
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
        let Some(instance) = presented.and_then(|instance| instance["instance"].as_u64()) else {
            return Ok(None);
        };
        let instance = usize::try_from(instance).map_err(|_| "scanout instance")?;
        let Some(frame) = self.session.scanout(name, instance, true, false)? else {
            return Ok(None);
        };
        let rgb = frame.rgb.ok_or("scanout did not return RGB")?;
        let width = u32::try_from(frame.width).map_err(|_| "frame width")?;
        let height = u32::try_from(frame.height).map_err(|_| "frame height")?;
        Ok(Some(fujimod_recording::Frame::rgb(width, height, rgb)?.png()?.to_vec()))
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
}
