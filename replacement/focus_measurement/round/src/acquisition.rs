#![forbid(unsafe_code)]

//! The acquisition machine shared by the live round and the S1 child, and the work each of its
//! steps does.

pub mod exposure;
pub mod live;
pub mod ports;
pub mod s1;
pub mod selection;
pub mod statistics;

use crate::jobs::{InvalidTransition, Outcome};
use xm1_saclac_interface::{Completed, RecordHandle};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Branch { Live, S1 }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Control { Initialize, Enable, Stop, Primary, Secondary }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Completion { Primary, Secondary }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Pass { First, Second, Third, Final }
impl Pass {
    fn next(self) -> Self {
        match self { Self::First => Self::Second, Self::Second => Self::Third, Self::Third => Self::Final, Self::Final => Self::First }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Effect {
    Initialize,
    Enable,
    Release,
    SubmitPrimary(Pass),
    SubmitSecondary,
    CollectPrimary,
    /// The live acquisition's secondary collection and selection table.
    CollectSecondary,
    /// The S1 acquisition's restore of the shot's saved selection.
    Restore,
    /// The S1 acquisition's publication of the record's selection.
    PublishSelection,
    /// The S1 acquisition's focus table.
    PublishFocus,
}

/// What each machine step does.
pub trait Work {
    fn effect(&mut self, branch: Branch, effect: Effect);
    /// Whether the latest secondary collection changed the selection table.
    fn selection_changed(&mut self) -> bool;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Stage { Disabled, Ready, Primary(Pass), Secondary, StoppingPrimary, StoppingSecondary }

pub struct Acquisition {
    branch: Branch,
    stage: Stage,
    pass: Pass,
}

impl Acquisition {
    pub const fn new(branch: Branch) -> Self { Self { branch, stage: Stage::Disabled, pass: Pass::First } }

    pub fn control(&mut self, control: Control, work: &mut impl Work) -> Result<Outcome, InvalidTransition> {
        match (self.stage, control) {
            (Stage::Disabled, Control::Initialize | Control::Enable) => {
                work.effect(self.branch, Effect::Initialize);
                self.pass = Pass::First;
                if self.branch == Branch::S1 || control == Control::Enable {
                    self.stage = Stage::Ready;
                }
                if self.branch == Branch::Live && control == Control::Enable { work.effect(self.branch, Effect::Enable); }
                Ok(Outcome::Complete)
            }
            (Stage::Disabled, Control::Stop) => Ok(Outcome::Complete),
            (Stage::Disabled, Control::Primary | Control::Secondary) if self.branch == Branch::Live => Ok(Outcome::NoWork),
            (Stage::Disabled, Control::Primary) => Ok(Outcome::Ignored),
            (Stage::Ready, Control::Stop) => {
                work.effect(self.branch, Effect::Release);
                self.stage = Stage::Disabled;
                Ok(Outcome::Complete)
            }
            (Stage::Ready, Control::Primary) => {
                let pass = if self.branch == Branch::S1 { self.pass } else { Pass::First };
                work.effect(self.branch, Effect::SubmitPrimary(pass));
                self.stage = Stage::Primary(pass);
                Ok(Outcome::Pending)
            }
            (Stage::Ready, Control::Secondary) if self.branch == Branch::Live => {
                work.effect(self.branch, Effect::SubmitSecondary);
                self.stage = Stage::Secondary;
                Ok(Outcome::Pending)
            }
            (Stage::Primary(_), Control::Stop) => {
                self.stage = Stage::StoppingPrimary;
                Ok(Outcome::Pending)
            }
            (Stage::Secondary, Control::Stop) => {
                self.stage = Stage::StoppingSecondary;
                Ok(Outcome::Pending)
            }
            (Stage::Primary(_) | Stage::Secondary, Control::Primary) if self.branch == Branch::S1 => Ok(Outcome::Pending),
            _ => Err(InvalidTransition),
        }
    }

    pub fn completed(&mut self, completion: Completion, work: &mut impl Work) -> Outcome {
        match (self.stage, completion) {
            (Stage::Primary(pass), Completion::Primary) => {
                work.effect(self.branch, Effect::CollectPrimary);
                if self.branch == Branch::S1 {
                    self.pass = pass.next();
                    if pass == Pass::Final {
                        work.effect(self.branch, Effect::SubmitSecondary);
                        self.stage = Stage::Secondary;
                        return Outcome::Pending;
                    }
                }
                self.stage = Stage::Ready;
                if self.branch == Branch::S1 { Outcome::Pending } else { Outcome::NoWork }
            }
            (Stage::Secondary, Completion::Secondary) => {
                self.stage = Stage::Ready;
                if self.branch == Branch::S1 {
                    work.effect(self.branch, Effect::Restore);
                    work.effect(self.branch, Effect::PublishSelection);
                    work.effect(self.branch, Effect::PublishFocus);
                    return Outcome::Pending;
                }
                work.effect(self.branch, Effect::CollectSecondary);
                if work.selection_changed() { Outcome::SelectionUpdated } else { Outcome::NoWork }
            }
            (Stage::StoppingPrimary, Completion::Primary) => {
                work.effect(self.branch, Effect::CollectPrimary);
                work.effect(self.branch, Effect::Release);
                self.stage = Stage::Disabled;
                Outcome::Complete
            }
            (Stage::StoppingSecondary, Completion::Secondary) => {
                if self.branch == Branch::S1 {
                    work.effect(self.branch, Effect::Restore);
                    work.effect(self.branch, Effect::PublishSelection);
                } else {
                    work.effect(self.branch, Effect::CollectSecondary);
                }
                work.effect(self.branch, Effect::Release);
                self.stage = Stage::Disabled;
                Outcome::Complete
            }
            _ => Outcome::Ignored,
        }
    }
}

/// The live acquisition's steps for one record and, for a collection, the completion it follows.
pub(crate) struct LiveSteps<'a, 'c, P> {
    pub record: RecordHandle,
    pub completed: Option<Completed<'c>>,
    pub work: &'a mut live::LiveWork,
    pub ports: &'a mut P,
}

impl<P: ports::Ports> Work for LiveSteps<'_, '_, P> {
    fn effect(&mut self, _: Branch, effect: Effect) {
        let (record, ports) = (self.record, &mut *self.ports);
        match (effect, self.completed) {
            (Effect::Initialize, _) => self.work.initialize(record, ports),
            (Effect::Enable, _) => self.work.enable(record, ports),
            (Effect::Release, _) => self.work.release(record, ports),
            (Effect::SubmitPrimary(_), _) => self.work.submit_primary(record, ports),
            (Effect::SubmitSecondary, _) => self.work.submit_secondary(record, ports),
            (Effect::CollectPrimary, Some(completed)) => self.work.collect_primary(record, completed, ports),
            (Effect::CollectSecondary, Some(completed)) => { self.work.collect_secondary(record, completed, ports); }
            _ => {}
        }
    }
    fn selection_changed(&mut self) -> bool { self.work.changed() }
}

/// The S1 acquisition's steps for one record and, for a collection, the completion it follows.
pub(crate) struct S1Steps<'a, 'c, P> {
    pub record: RecordHandle,
    pub completed: Option<Completed<'c>>,
    pub work: &'a mut s1::S1Work,
    pub ports: &'a mut P,
}

impl<P: ports::Ports> Work for S1Steps<'_, '_, P> {
    fn effect(&mut self, _: Branch, effect: Effect) {
        let (record, ports) = (self.record, &mut *self.ports);
        match (effect, self.completed) {
            (Effect::Initialize, _) => self.work.initialize(record, ports),
            (Effect::Release, _) => self.work.stop(record, ports),
            (Effect::SubmitPrimary(_), _) => self.work.submit_primary(record, ports),
            (Effect::SubmitSecondary, _) => self.work.submit_secondary(record, ports),
            (Effect::CollectPrimary, Some(completed)) => self.work.collect_primary(record, completed, ports),
            (Effect::Restore, Some(completed)) => self.work.restore(record, completed, ports),
            (Effect::PublishSelection, Some(completed)) => self.work.publish_selection(record, completed, ports),
            (Effect::PublishFocus, Some(completed)) => self.work.publish_focus(record, completed, ports),
            _ => {}
        }
    }
    fn selection_changed(&mut self) -> bool { false }
}
