#![forbid(unsafe_code)]

//! The live lens child: profile and setting requests, the lens drive and its cancellation,
//! position steps and selections while live processing runs.

use crate::ports::{
    CommandBlocks, Configuration, DriveDetail, DriveFourth, DriveReply, DriveResults, DriveWords, Faults, LensControl,
    LensRequest, ModeSource, Records, Replies, Reply, ResourceSelection, Status,
};
use crate::{parameter_word, Outcome};
use xm1_saclac_interface::lens::{DriveOwner, LensPayload, SelectionCode, Setting};
use xm1_saclac_interface::{Completed, Completion, ParameterValue, RecordHandle};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Profile { Primary, Secondary }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DriveOrigin {
    /// The LensDrive command.
    Command,
    /// The root's cancellation.
    Cancel,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Event<'a> {
    Profile(Profile),
    /// A lens setting the live router owes.
    Setting(Setting),
    /// Before the live controller suspends for a drive.
    PrepareDrive,
    Drive(DriveOrigin),
    /// The Cancel command.
    Cancel,
    /// Command10.
    Stop,
    /// Notification 0x1b's position step.
    Step,
    /// Command2c.
    Select,
    /// Command2d.
    Release,
    Completed(Completed<'a>),
    Other,
}

pub trait Ports: LensControl + Replies + CommandBlocks + Records + Configuration + DriveResults + Faults {}
impl<T: LensControl + Replies + CommandBlocks + Records + Configuration + DriveResults + Faults> Ports for T {}

/// Settings run one at a time in this order; a setting is owed only behind every active and
/// owed one.
const ORDER: [Setting; 3] = [Setting::Request04, Setting::Request06, Setting::Request05];

const fn rank(setting: Setting) -> usize {
    match setting {
        Setting::Request04 => 0,
        Setting::Request06 => 1,
        Setting::Request05 => 2,
    }
}

const fn ends(setting: Setting) -> Completion {
    match setting {
        Setting::Request04 => Completion::Lens38,
        Setting::Request06 => Completion::Lens39,
        Setting::Request05 => Completion::Lens36,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Settings {
    active: Setting,
    owed: [bool; 3],
}

impl Settings {
    fn last(self) -> usize {
        (0..3).rev().find(|&index| self.owed[index]).unwrap_or(rank(self.active))
    }

    fn next(self) -> Option<Setting> {
        ORDER.into_iter().find(|setting| self.owed[rank(*setting)])
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum State {
    Idle,
    Prepared,
    /// A profile's setting, ended by Lens36.
    Profile,
    Settings(Settings),
    Driving,
    Stepping,
    Selecting,
}

pub struct LiveLens {
    state: State,
}

impl Default for LiveLens {
    fn default() -> Self { Self::new() }
}

impl LiveLens {
    pub const fn new() -> Self { Self { state: State::Idle } }

    pub fn handle(&mut self, event: Event<'_>, record: RecordHandle, ports: &mut impl Ports) -> Outcome {
        match (self.state, event) {
            (_, Event::Other) => Outcome::Inactive,
            (_, Event::Completed(done)) if !handles(done.completion) => Outcome::Inactive,
            (State::Idle, Event::Profile(profile)) => {
                let source = match profile { Profile::Primary => ModeSource::Primary, Profile::Secondary => ModeSource::Secondary };
                self.start_setting(Setting::Request05, source, State::Profile, record, ports)
            }
            (State::Idle, Event::Setting(setting)) => {
                let next = State::Settings(Settings { active: setting, owed: [false; 3] });
                self.start_setting(setting, ModeSource::Autofocus, next, record, ports)
            }
            (State::Settings(mut settings), Event::Setting(setting)) if rank(setting) > settings.last() => {
                settings.owed[rank(setting)] = true;
                self.state = State::Settings(settings);
                Outcome::Pending
            }
            (State::Settings(settings), Event::Completed(done)) if done.completion == ends(settings.active) => {
                match settings.next() {
                    None => self.finish(),
                    Some(next) => {
                        let mut owed = settings.owed;
                        owed[rank(next)] = false;
                        self.start_setting(next, ModeSource::Autofocus, State::Settings(Settings { active: next, owed }), record, ports)
                    }
                }
            }
            (State::Profile, Event::Completed(done)) if done.completion == Completion::Lens36 => self.finish(),
            (State::Idle, Event::PrepareDrive) => {
                self.state = State::Prepared;
                Outcome::Pending
            }
            (State::Idle | State::Prepared, Event::Drive(origin)) => self.drive(origin, record, ports),
            (State::Idle, Event::Cancel) => {
                ports.reply(Reply::Cancel(Status::Unneeded));
                Outcome::Complete
            }
            (State::Prepared, Event::Cancel) => {
                let target = ports.cancel();
                ports.set_drive_third(target);
                ports.reply(Reply::Cancel(Status::Accepted));
                Outcome::Pending
            }
            (State::Driving, Event::Cancel) => {
                let payload = LensPayload::Finish { value: ports.cancel(), record };
                let sent = ports.send(LensRequest::new(payload));
                ports.reply(Reply::Cancel(Status::Accepted));
                pending_if(sent)
            }
            (State::Idle, Event::Stop) => {
                ports.reply(Reply::Stop(Status::Unneeded));
                Outcome::Complete
            }
            (State::Prepared, Event::Stop) => Outcome::Complete,
            (State::Driving, Event::Stop) => {
                let sent = ports.send(LensRequest::new(LensPayload::Halt));
                ports.reply(Reply::Stop(Status::Accepted));
                pending_if(sent)
            }
            (State::Driving, Event::Completed(done)) if matches!(done.completion, Completion::Lens34 | Completion::Lens37) => {
                let words = ports.drive();
                let reply = if corrects(record, ports) {
                    corrected_reply(Status::Done, words.with_fourth(DriveFourth::One), ports)
                } else {
                    recorded_reply(Status::Done, words, ports)
                };
                ports.reply(Reply::Drive(reply));
                self.finish()
            }
            (State::Idle, Event::Step) => {
                let step = ports.step();
                if ports.send(LensRequest::new(LensPayload::Step(step))).is_ok() {
                    self.state = State::Stepping;
                    Outcome::Pending
                } else {
                    let probes = ports.probes();
                    ports.reply(Reply::Step { status: Status::Refused, probes });
                    Outcome::Fault
                }
            }
            (State::Stepping, Event::Completed(done)) if done.completion == Completion::Lens35 => {
                let probes = ports.probes();
                ports.reply(Reply::Step { status: Status::Done, probes });
                self.finish()
            }
            (State::Idle, Event::Select) => {
                let value = ports.selection();
                let code = if ports.adjustment_configured() { SelectionCode::Configured } else { SelectionCode::Unconfigured };
                if ports.send(LensRequest::new(LensPayload::Select { value, code, record })).is_ok() {
                    self.state = State::Selecting;
                    ports.prepare_selection(record);
                    Outcome::Pending
                } else {
                    let probes = ports.probes();
                    ports.reply(Reply::Select { status: Status::Refused, probes, value: ParameterValue(0) });
                    Outcome::Fault
                }
            }
            (State::Selecting, Event::Completed(done)) if done.completion == Completion::Lens3d => {
                let probes = ports.probes();
                let value = parameter_word(done.parameters, 2);
                ports.reply(Reply::Select { status: Status::Done, probes, value });
                self.finish()
            }
            (State::Idle, Event::Release) => {
                ports.reply(Reply::Release(Status::Unneeded));
                Outcome::Complete
            }
            (State::Selecting, Event::Release) => {
                if ports.send(LensRequest::new(LensPayload::Release)).is_ok() {
                    ports.reply(Reply::Release(Status::Accepted));
                    Outcome::Pending
                } else {
                    ports.reply(Reply::Release(Status::Refused));
                    Outcome::Fault
                }
            }
            _ => {
                ports.fault();
                Outcome::Fault
            }
        }
    }

    fn finish(&mut self) -> Outcome {
        self.state = State::Idle;
        Outcome::Complete
    }

    /// A refused setting leaves the state as it was.
    fn start_setting(&mut self, setting: Setting, source: ModeSource, next: State, record: RecordHandle, ports: &mut impl Ports) -> Outcome {
        let slot = ports.profile_slot(record);
        let mode = ports.mode(source);
        if ports.send(LensRequest::new(LensPayload::Setting { setting, slot, mode })).is_ok() {
            self.state = next;
            Outcome::Pending
        } else {
            Outcome::Fault
        }
    }

    fn drive(&mut self, origin: DriveOrigin, record: RecordHandle, ports: &mut impl Ports) -> Outcome {
        let corrected = origin == DriveOrigin::Command && corrects(record, ports);
        let slot = ports.profile_slot(record);
        let words = ports.drive();
        let sent = if corrected { words.with_fourth(DriveFourth::One) } else { words };
        let payload = LensPayload::Drive { owner: DriveOwner::Live, parameters: sent.parameters(), slot };
        if ports.send(LensRequest::new(payload)).is_ok() {
            self.state = State::Driving;
            return Outcome::Pending;
        }
        let reply = if corrected { corrected_reply(Status::Refused, sent, ports) } else { recorded_reply(Status::Refused, words, ports) };
        ports.reply(Reply::Drive(reply));
        Outcome::Fault
    }
}

const fn handles(completion: Completion) -> bool {
    matches!(
        completion,
        Completion::Lens34 | Completion::Lens35 | Completion::Lens36 | Completion::Lens37 | Completion::Lens38
            | Completion::Lens39 | Completion::Lens3d
    )
}

const fn pending_if(sent: Result<(), crate::ports::Refused>) -> Outcome {
    if sent.is_ok() { Outcome::Pending } else { Outcome::Fault }
}

/// Whether a drive's fourth word is forced to 1, and its end reported with the command's first
/// word.
fn corrects(record: RecordHandle, ports: &mut impl Ports) -> bool {
    let policy = ports.drive_policy();
    let applies = policy.always
        || match ports.resource_selection(record) {
            ResourceSelection::One => policy.resource_one,
            ResourceSelection::Zero => policy.resource_zero,
            ResourceSelection::Other => false,
        };
    applies && ports.shared_drive_bit()
}

fn corrected_reply(status: Status, words: DriveWords, ports: &mut impl Ports) -> DriveReply {
    DriveReply { status, value: words.value, fourth: words.fourth, detail: DriveDetail::Lens(ports.probes()) }
}

fn recorded_reply(status: Status, words: DriveWords, ports: &mut impl Ports) -> DriveReply {
    DriveReply { status, value: ports.live_result(), fourth: words.fourth, detail: DriveDetail::Lens(ports.probes()) }
}
