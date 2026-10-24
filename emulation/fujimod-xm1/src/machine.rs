//! A strict X-M1 session and its run document; runs record armed instruction
//! points and stop at the R4F guard's refusals.

use crate::guest::{MenuState, ModeDialPosition, ModeDialWindow, RouteKey, set_adc};
use crate::r4f::Guard;
use crate::run::RunDocument;
use crate::trace::TraceSites;
use fujimod_emulator::session::Session;

const RELEASE_POLL: u64 = 250_000;
/// Longest run between two guard scans.
const GUARD_WINDOW: u64 = 1_000_000;

/// Instruction budgets of one key press: held, then at most `release` until
/// the firmware has debounced the release.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KeyPulse {
    pub hold: u64,
    pub release: u64,
}

pub struct Machine {
    pub document: RunDocument,
    pub session: Session,
    /// The first strict stop; no input is admitted after it.
    pub failure: Option<String>,
    pub traces: TraceSites,
    pub guard: Guard,
}

pub fn instructions(session: &Session) -> u64 {
    session.board.instruction_counts().iter().sum()
}

impl Machine {
    /// A machine with the R4F guard armed.
    pub fn new(document: RunDocument, session: Session) -> Result<Self, String> {
        let mut guard = Guard::default();
        guard.arm(&session.board)?;
        Ok(Self { document, session, failure: None, traces: TraceSites::default(), guard })
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
        let (port, pin, pressed) = (key.port, key.pin, key.pressed_high);
        let gpio = self.document.gpio.0.clone();
        self.session.gpio(&gpio, port, pin, pressed)?;
        self.run(pulse.hold);
        self.session.gpio(&gpio, port, pin, !pressed)?;
        let start = self.instructions();
        while self.failure.is_none() && !self.document.firmware.dial_push_released(&self.session)? {
            let spent = self.instructions() - start;
            if spent >= pulse.release {
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
                break;
            }
            self.run((pulse.release - spent).min(RELEASE_POLL));
        }
        Ok(())
    }

    /// Holds the ADC key `key` pressed until `release_key`.
    pub fn press(&mut self, key: RouteKey) -> Result<(), String> {
        let (channel, sample) = self.document.firmware.pressed_sample(&self.session, key)?;
        set_adc(&mut self.session, &self.document.adc, channel, sample)
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
}
