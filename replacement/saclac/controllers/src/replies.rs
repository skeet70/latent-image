#![forbid(unsafe_code)]

use xm1_saclac_interface::{ClientCallback, CompletionStatus, Parameters, ReplyTarget};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Reply<'a> {
    pub target: ReplyTarget,
    pub callback: Option<ClientCallback>,
    pub status: CompletionStatus,
    pub values: Parameters<'a>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Effect<'a> {
    ClearPending(ReplyTarget),
    Deliver {
        target: ReplyTarget,
        callback: ClientCallback,
        status: CompletionStatus,
        values: Parameters<'a>,
    },
}

pub trait Effects {
    fn emit(&mut self, effect: Effect<'_>);
}

pub fn deliver(reply: Reply<'_>, effects: &mut impl Effects) {
    if reply.status.0 <= 0 || matches!(reply.status.0, 20 | 21) {
        effects.emit(Effect::ClearPending(reply.target));
    }
    if let Some(callback) = reply.callback {
        effects.emit(Effect::Deliver {
            target: reply.target,
            callback,
            status: reply.status,
            values: reply.values,
        });
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PreparationNotice {
    Suppressed,
    Requested(Option<ClientCallback>),
}

pub fn prepare(
    command_callback: Option<ClientCallback>,
    notice: PreparationNotice,
    status: CompletionStatus,
    values: Parameters<'_>,
    effects: &mut impl Effects,
) {
    let callback = if status == CompletionStatus(19) {
        match notice {
            PreparationNotice::Suppressed => None,
            PreparationNotice::Requested(callback) => callback,
        }
    } else {
        command_callback
    };
    deliver(Reply {
        target: ReplyTarget::Command(xm1_saclac_interface::CommandKind::Prepare),
        callback,
        status,
        values,
    }, effects);
}
