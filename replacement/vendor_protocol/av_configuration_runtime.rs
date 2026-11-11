//! X-M1 1.50 action-24 bank selection through the canonical audio executor.
//! Source pin: 55436fc85c0560dd252b0410af6f2427fe198cd3.

use crate::audio_zoom_actor::{AudioZoomActor, BankSelection, Completion, Error as CodecError, Request, RequestId, Tick, Timeout, WorkerFields};
use xm1_power_lifecycle::audio_release::executor::{ExecutorIo, ExecutorMemory};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CaptureMode { Default, Alternate }
impl CaptureMode {
    pub const fn from_selector(selector: u8) -> Self {
        if selector == 0 { Self::Default } else { Self::Alternate }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Progress { Waiting, Completed(Completion) }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error { Idle, Busy, Codec(CodecError) }

#[derive(Clone, Copy)]
enum State { Idle, Queued(CaptureMode, Timeout), Waiting(RequestId), Completed(Completion) }

pub struct AvConfigurationRuntime { state: State }
impl Default for AvConfigurationRuntime { fn default() -> Self { Self::new() } }
impl AvConfigurationRuntime {
    pub const fn new() -> Self { Self { state: State::Idle } }

    pub fn begin(
        &mut self, mode: CaptureMode, actor: &mut AudioZoomActor,
        memory: &mut impl ExecutorMemory, io: &mut impl ExecutorIo,
        worker: &mut impl WorkerFields, now: Tick, timeout: Timeout,
    ) -> Result<Progress, Error> {
        if matches!(self.state, State::Queued(..) | State::Waiting(_)) { return Err(Error::Busy); }
        self.state = State::Queued(mode, timeout);
        self.resume(actor, memory, io, worker, now)
    }

    pub fn resume(
        &mut self, actor: &mut AudioZoomActor, memory: &mut impl ExecutorMemory,
        io: &mut impl ExecutorIo, worker: &mut impl WorkerFields, now: Tick,
    ) -> Result<Progress, Error> {
        if let State::Queued(mode, timeout) = self.state {
            let bank = match mode { CaptureMode::Default => BankSelection::Primary, CaptureMode::Alternate => BankSelection::Alternate };
            match actor.submit(memory, io, worker, Request::SelectBank { bank }, now, timeout) {
                Ok(id) => self.state = State::Waiting(id),
                Err(CodecError::Busy) => return Ok(Progress::Waiting),
                Err(error) => {
                    self.state = State::Completed(Completion::Failed(error));
                    return Err(Error::Codec(error));
                }
            }
        }
        self.poll(actor)
    }

    /// The shared audio owner advances the executor before polling.
    pub fn poll(&mut self, actor: &AudioZoomActor) -> Result<Progress, Error> {
        match self.state {
            State::Idle => Err(Error::Idle),
            State::Queued(..) => Ok(Progress::Waiting),
            State::Completed(completion) => Ok(Progress::Completed(completion)),
            State::Waiting(id) => match actor.result(id).map_err(Error::Codec)? {
                None => Ok(Progress::Waiting),
                Some(completion) => {
                    self.state = State::Completed(completion);
                    Ok(Progress::Completed(completion))
                }
            },
        }
    }

    pub fn cancel(
        &mut self, actor: &mut AudioZoomActor, memory: &mut impl ExecutorMemory,
        io: &mut impl ExecutorIo,
    ) -> Result<Progress, Error> {
        match self.state {
            State::Queued(..) => self.state = State::Completed(Completion::Cancelled),
            State::Waiting(id) => actor.cancel(memory, io, id).map_err(Error::Codec)?,
            State::Idle | State::Completed(_) => {}
        }
        self.poll(actor)
    }
}

#[cfg(test)]
#[path = "av_configuration_runtime_tests.rs"]
mod tests;
