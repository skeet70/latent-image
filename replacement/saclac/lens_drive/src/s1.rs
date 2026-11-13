#![forbid(unsafe_code)]

//! The S1 lens child: the drive the S1 router starts on the root's cancellation, and its
//! cancellation, stop and reports.

use crate::ports::{CommandBlocks, DriveDetail, DriveReply, DriveResults, Faults, LensControl, LensRequest, Records, Replies, Reply, Status};
use crate::{parameter_word, Outcome};
use xm1_saclac_interface::lens::{DriveOwner, LensPayload};
use xm1_saclac_interface::{Completed, Completion, RecordHandle};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Event<'a> {
    /// The root's cancellation drives the lens.
    CancelDrive,
    /// The Cancel command.
    Cancel,
    /// Command10.
    Stop,
    Completed(Completed<'a>),
    Other,
}

pub trait Ports: LensControl + Replies + CommandBlocks + Records + DriveResults + Faults {}
impl<T: LensControl + Replies + CommandBlocks + Records + DriveResults + Faults> Ports for T {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum State { Idle, Driving }

pub struct S1Lens {
    state: State,
}

impl Default for S1Lens {
    fn default() -> Self { Self::new() }
}

impl S1Lens {
    pub const fn new() -> Self { Self { state: State::Idle } }

    pub fn handle(&mut self, event: Event<'_>, record: RecordHandle, ports: &mut impl Ports) -> Outcome {
        match (self.state, event) {
            (_, Event::Other) => Outcome::Inactive,
            (_, Event::Completed(done)) if !matches!(done.completion, Completion::Lens33 | Completion::Lens34) => Outcome::Inactive,
            (State::Idle, Event::CancelDrive) => {
                let slot = ports.profile_slot(record);
                let parameters = ports.drive().parameters();
                let payload = LensPayload::Drive { owner: DriveOwner::S1, parameters, slot };
                if ports.send(LensRequest::new(payload)).is_ok() {
                    self.state = State::Driving;
                    Outcome::Pending
                } else {
                    let reply = reported(Status::Refused, ports);
                    ports.reply(Reply::Drive(reply));
                    Outcome::Fault
                }
            }
            (State::Idle, Event::Cancel) => {
                ports.reply(Reply::Cancel(Status::Accepted));
                Outcome::Pending
            }
            (State::Driving, Event::Cancel) => {
                let payload = LensPayload::Finish { value: ports.cancel(), record };
                let sent = ports.send(LensRequest::new(payload));
                ports.reply(Reply::Cancel(Status::Accepted));
                if sent.is_ok() { Outcome::Pending } else { Outcome::Fault }
            }
            (State::Idle, Event::Stop) => {
                ports.reply(Reply::Stop(Status::Accepted));
                let reply = reported(Status::Done, ports);
                ports.reply(Reply::Drive(reply));
                Outcome::Complete
            }
            (State::Driving, Event::Stop) => {
                let sent = ports.send(LensRequest::new(LensPayload::Halt));
                ports.reply(Reply::Stop(Status::Accepted));
                if sent.is_ok() { Outcome::Pending } else { Outcome::Fault }
            }
            (State::Driving, Event::Completed(done)) if done.completion == Completion::Lens34 => {
                let reply = reported(Status::Done, ports);
                ports.reply(Reply::Drive(reply));
                self.state = State::Idle;
                Outcome::Complete
            }
            (State::Driving, Event::Completed(done)) => {
                let words = ports.drive();
                ports.reply(Reply::Drive(DriveReply {
                    status: Status::Progress,
                    value: parameter_word(done.parameters, 1),
                    fourth: words.fourth,
                    detail: DriveDetail::Reported(parameter_word(done.parameters, 3)),
                }));
                Outcome::Pending
            }
            _ => {
                ports.fault();
                Outcome::Fault
            }
        }
    }
}

fn reported(status: Status, ports: &mut impl Ports) -> DriveReply {
    DriveReply {
        status,
        value: ports.s1_report(),
        fourth: ports.drive().fourth,
        detail: DriveDetail::Lens(ports.probes()),
    }
}
