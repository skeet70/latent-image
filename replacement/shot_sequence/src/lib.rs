#![no_std]
#![forbid(unsafe_code)]

pub mod boundary;
pub mod calculation;
pub mod calculation_work;
pub mod calculations_work;
pub mod capture;
pub mod capture_boundary;
pub mod capture_stock;
pub mod exposure;
pub mod exposure_work;
pub mod finish;
pub mod finish_work;
pub mod jobs;
pub mod lens_return;
pub mod outcome;
pub mod pipeline;
pub mod ports;
pub mod preparation;
pub mod sequence;
pub mod stock;
pub mod subjects;
pub mod summary;

pub use outcome::Outcome;

use capture::Capture;
use ports::{Ports, Signal};
use sequence::{HalfPress, Live, Steps};
use xm1_saclac_controllers::children::{ShotControl, ShotEvent, ShotTarget};
use xm1_saclac_interface::{CommandKind, Completion, RecordHandle};

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

    pub fn handle(&mut self, event: ShotEvent<'_>, ports: &mut impl Ports) -> Outcome {
        match event {
            ShotEvent::Control { control, record } => self.control(control, record, ports),
            ShotEvent::Command { target, command, record } => self.command(target, command.kind, record, ports),
            ShotEvent::Completed { target, completed, record } => {
                ports.receive(completed);
                self.completed(target, completed.completion, record, ports)
            }
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

    pub fn command(&mut self, target: ShotTarget, command: CommandKind, record: RecordHandle, ports: &mut impl Ports) -> Outcome {
        match target {
            ShotTarget::HalfPress => self.half_press.command(command, record, &mut self.steps, ports),
            ShotTarget::Capture => self.capture.command(command, record, ports),
            ShotTarget::Live => self.live.signal(Signal::Command(command), record, &mut self.steps, ports),
        }
    }

    pub fn completed(&mut self, target: ShotTarget, completion: Completion, record: RecordHandle, ports: &mut impl Ports) -> Outcome {
        let signal = Signal::Completion(completion);
        match target {
            ShotTarget::HalfPress => self.half_press.signal(signal, record, &mut self.steps, ports),
            ShotTarget::Capture => self.capture.completed(completion, record, ports),
            ShotTarget::Live => self.live.signal(signal, record, &mut self.steps, ports),
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
#[cfg(test)]
mod leaf_fakes;
#[cfg(test)]
mod preparation_tests;
#[cfg(test)]
mod subjects_tests;
#[cfg(test)]
mod summary_tests;
#[cfg(test)]
mod exposure_fakes;
#[cfg(test)]
mod exposure_work_tests;
#[cfg(test)]
mod capture_fakes;
#[cfg(test)]
mod lens_return_tests;
#[cfg(test)]
mod capture_stock_tests;
#[cfg(test)]
mod calculation_work_tests;
#[cfg(test)]
mod finish_work_tests;
#[cfg(test)]
mod calculations_work_tests;
