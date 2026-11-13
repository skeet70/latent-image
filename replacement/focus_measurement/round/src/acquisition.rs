#![forbid(unsafe_code)]

use crate::jobs::{InvalidTransition, Outcome};

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
    PublishFocus,
    CollectSecondary,
    PublishPrimary,
    PublishSecondary,
    Restore,
}

pub trait Ports {
    fn effect(&mut self, branch: Branch, effect: Effect);
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

    pub fn control(&mut self, control: Control, ports: &mut impl Ports) -> Result<Outcome, InvalidTransition> {
        match (self.stage, control) {
            (Stage::Disabled, Control::Initialize | Control::Enable) => {
                ports.effect(self.branch, Effect::Initialize);
                self.pass = Pass::First;
                if self.branch == Branch::S1 || control == Control::Enable {
                    self.stage = Stage::Ready;
                }
                if self.branch == Branch::Live && control == Control::Enable { ports.effect(self.branch, Effect::Enable); }
                Ok(Outcome::Complete)
            }
            (Stage::Disabled, Control::Stop) => Ok(Outcome::Complete),
            (Stage::Disabled, Control::Primary | Control::Secondary) => Ok(Outcome::NoWork),
            (Stage::Ready, Control::Stop) => {
                ports.effect(self.branch, Effect::Release);
                self.stage = Stage::Disabled;
                Ok(Outcome::Complete)
            }
            (Stage::Ready, Control::Primary) => {
                let pass = if self.branch == Branch::S1 { self.pass } else { Pass::First };
                ports.effect(self.branch, Effect::SubmitPrimary(pass));
                self.stage = Stage::Primary(pass);
                Ok(Outcome::Pending)
            }
            (Stage::Ready, Control::Secondary) if self.branch == Branch::Live => {
                ports.effect(self.branch, Effect::SubmitSecondary);
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

    pub fn completed(&mut self, completion: Completion, ports: &mut impl Ports) -> Outcome {
        match (self.stage, completion) {
            (Stage::Primary(pass), Completion::Primary) => {
                ports.effect(self.branch, Effect::CollectPrimary);
                if self.branch == Branch::S1 {
                    self.pass = pass.next();
                    if pass == Pass::Final {
                        ports.effect(self.branch, Effect::SubmitSecondary);
                        self.stage = Stage::Secondary;
                        return Outcome::Pending;
                    }
                } else {
                    ports.effect(self.branch, Effect::PublishPrimary);
                }
                self.stage = Stage::Ready;
                if self.branch == Branch::S1 { Outcome::Pending } else { Outcome::NoWork }
            }
            (Stage::Secondary, Completion::Secondary) => {
                if self.branch == Branch::S1 {
                    ports.effect(self.branch, Effect::Restore);
                    ports.effect(self.branch, Effect::PublishFocus);
                } else {
                    ports.effect(self.branch, Effect::CollectSecondary);
                }
                ports.effect(self.branch, Effect::PublishSecondary);
                self.stage = Stage::Ready;
                if self.branch == Branch::Live && ports.selection_changed() {
                    Outcome::SelectionUpdated
                } else if self.branch == Branch::S1 {
                    Outcome::Pending
                } else { Outcome::NoWork }
            }
            (Stage::StoppingPrimary, Completion::Primary) => {
                ports.effect(self.branch, Effect::CollectPrimary);
                if self.branch == Branch::Live { ports.effect(self.branch, Effect::PublishPrimary); }
                ports.effect(self.branch, Effect::Release);
                self.stage = Stage::Disabled;
                Outcome::Complete
            }
            (Stage::StoppingSecondary, Completion::Secondary) => {
                if self.branch == Branch::S1 {
                    ports.effect(self.branch, Effect::Restore);
                    ports.effect(self.branch, Effect::PublishFocus);
                } else {
                    ports.effect(self.branch, Effect::CollectSecondary);
                    ports.effect(self.branch, Effect::PublishSecondary);
                }
                ports.effect(self.branch, Effect::Release);
                self.stage = Stage::Disabled;
                Outcome::Complete
            }
            _ => Outcome::Unhandled,
        }
    }
}
