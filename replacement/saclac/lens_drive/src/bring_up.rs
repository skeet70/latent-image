#![forbid(unsafe_code)]

//! The bring-up child the root enters for LensBringUp and LensStop and their completions.

use crate::ports::{CommandBlocks, Configuration, Faults, LensControl, LensRequest, Replies, Reply, Status};
use crate::{parameter_word, Outcome};
use xm1_saclac_interface::lens::LensPayload;
use xm1_saclac_interface::{Completed, Completion, Parameters, RecordHandle};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Event<'a> {
    /// The LensBringUp command.
    BringUp,
    /// The LensStop command.
    Stop,
    Completed(Completed<'a>),
    Other,
}

pub trait Ports: LensControl + Replies + CommandBlocks + Configuration + Faults {}
impl<T: LensControl + Replies + CommandBlocks + Configuration + Faults> Ports for T {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum State { Idle, Starting, Stopping, Started }

pub struct BringUp {
    state: State,
}

impl Default for BringUp {
    fn default() -> Self { Self::new() }
}

impl BringUp {
    pub const fn new() -> Self { Self { state: State::Idle } }

    pub fn handle(&mut self, event: Event<'_>, record: RecordHandle, ports: &mut impl Ports) -> Outcome {
        match (self.state, event) {
            (_, Event::Other) => Outcome::Inactive,
            (_, Event::Completed(done)) if !handles(done.completion) => Outcome::Inactive,
            (State::Idle | State::Started, Event::BringUp) => {
                let parameters = ports.bring_up();
                // X-M1 1.50 0xf0348c answers Pending whether or not lens control posted it.
                let _ = ports.send(LensRequest::new(LensPayload::BringUp { parameters, record }));
                self.state = State::Starting;
                Outcome::Pending
            }
            (State::Starting, Event::Completed(done)) if matches!(done.completion, Completion::Lens31 | Completion::Lens32) => {
                report(done.parameters, ports);
                Outcome::Pending
            }
            (State::Starting, Event::Completed(done)) if done.completion == Completion::Lens2f => {
                report(done.parameters, ports);
                self.state = State::Started;
                Outcome::Complete
            }
            (State::Starting, Event::Completed(done)) if matches!(done.completion, Completion::Image3f | Completion::Image40) => Outcome::Pending,
            (State::Idle | State::Started, Event::Stop) => {
                self.state = State::Stopping;
                let parameters = ports.lens_stop();
                // X-M1 1.50 0xf0350c answers Pending whether or not lens control posted it.
                let _ = ports.send(LensRequest::new(LensPayload::Stop { parameters, record }));
                Outcome::Pending
            }
            (State::Stopping, Event::Completed(done)) if done.completion == Completion::Lens30 => {
                let status = Status::Lens(parameter_word(done.parameters, 0).0 as i32);
                ports.reply(Reply::LensStop { status, value: parameter_word(done.parameters, 1) });
                self.state = State::Idle;
                Outcome::Complete
            }
            _ => {
                ports.fault();
                Outcome::Fault
            }
        }
    }
}

const fn handles(completion: Completion) -> bool {
    matches!(
        completion,
        Completion::Lens2f | Completion::Lens30 | Completion::Lens31 | Completion::Lens32 | Completion::Image3f | Completion::Image40
    )
}

/// Applies lens control's resource index to the configuration lookup, then replies with the
/// completion's four words.
fn report(parameters: Parameters<'_>, ports: &mut impl Ports) {
    let index = ports.resource_index();
    ports.select_lookup(index);
    let status = Status::Lens(parameter_word(parameters, 0).0 as i32);
    let values = [1, 2, 3].map(|index| parameter_word(parameters, index));
    ports.reply(Reply::BringUp { status, values });
}
