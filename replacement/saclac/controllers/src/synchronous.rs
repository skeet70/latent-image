#![forbid(unsafe_code)]

use xm1_saclac_interface::{ClientCallback, CompletionStatus};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Completion {
    Progress,
    Finished,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Effect {
    Reply { callback: ClientCallback, status: CompletionStatus },
    Occupied,
    Released,
}

pub trait Effects {
    fn emit(&mut self, effect: Effect);
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Admission {
    Accepted,
    Busy,
}

#[derive(Default)]
pub struct Clients {
    pending: Option<ClientCallback>,
}

impl Clients {
    pub const fn new() -> Self {
        Self { pending: None }
    }

    pub fn reserve(&mut self, callback: ClientCallback, effects: &mut impl Effects) -> Admission {
        if self.pending.is_some() {
            effects.emit(Effect::Reply { callback, status: CompletionStatus(-95) });
            return Admission::Busy;
        }
        self.pending = Some(callback);
        effects.emit(Effect::Occupied);
        Admission::Accepted
    }

    pub fn complete(&mut self, completion: Completion, effects: &mut impl Effects) {
        if completion == Completion::Finished {
            if let Some(callback) = self.pending {
                effects.emit(Effect::Reply { callback, status: CompletionStatus(0) });
                self.pending = None;
                effects.emit(Effect::Released);
            }
        }
    }
}
