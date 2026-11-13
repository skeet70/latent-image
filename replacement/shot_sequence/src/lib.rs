#![no_std]
#![forbid(unsafe_code)]

pub mod calculation;
pub mod capture;
pub mod exposure;
pub mod finish;
pub mod jobs;
pub mod outcome;
pub mod pipeline;
pub mod ports;
pub mod sequence;
pub mod stock;

pub use outcome::Outcome;

use capture::Capture;
use ports::{Ports, Signal};
use sequence::{HalfPress, Live, Steps};
use xm1_saclac_controllers::children::{ChildEvent, ShotControl};
use xm1_saclac_interface::{CommandKind, Completion, RecordHandle};

/// The entry a parent routes a command or completion through. Controls select their own entry.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Target { HalfPress, Capture, Live }

/// S1 preparation, the live preparation sequence and the capture pipeline behind queue 43.
#[derive(Default)]
pub struct Shot {
    steps: Steps,
    half_press: HalfPress,
    live: Live,
    capture: Capture,
}

impl Shot {
    pub fn new() -> Self { Self::default() }

    pub fn handle(&mut self, target: Target, event: ChildEvent<'_, ShotControl>, ports: &mut impl Ports) -> Outcome {
        match event {
            ChildEvent::Control { control, record, .. } => self.control(control, record, ports),
            ChildEvent::Command { command, record } => self.command(target, command.kind, record, ports),
            ChildEvent::Completed { completed, record } => self.completed(target, completed.completion, record, ports),
        }
    }

    pub fn control(&mut self, control: ShotControl, record: RecordHandle, ports: &mut impl Ports) -> Outcome {
        match control {
            ShotControl::StartS1 => self.half_press.start(record, &mut self.steps, ports),
            ShotControl::AdvanceS1 => self.half_press.advance(record, &mut self.steps, ports),
            ShotControl::InitializeCapture => self.capture.start(record, ports),
            ShotControl::StartLiveSequence => self.live.start(record, &mut self.steps, ports),
        }
    }

    pub fn command(&mut self, target: Target, command: CommandKind, record: RecordHandle, ports: &mut impl Ports) -> Outcome {
        match target {
            Target::HalfPress => self.half_press.command(command, record, &mut self.steps, ports),
            Target::Capture => self.capture.command(command, record, ports),
            Target::Live => self.live.signal(Signal::Command(command), record, &mut self.steps, ports),
        }
    }

    pub fn completed(&mut self, target: Target, completion: Completion, record: RecordHandle, ports: &mut impl Ports) -> Outcome {
        let signal = Signal::Completion(completion);
        match target {
            Target::HalfPress => self.half_press.signal(signal, record, &mut self.steps, ports),
            Target::Capture => self.capture.completed(completion, record, ports),
            Target::Live => self.live.signal(signal, record, &mut self.steps, ports),
        }
    }
}

#[cfg(test)]
extern crate std;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod capture_tests;
#[cfg(test)]
mod stock_tests;
