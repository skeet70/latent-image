#![forbid(unsafe_code)]

//! The round's client replies: a reply clears its command's pending flag when its status ends
//! the command, and enters the client's callback when there is one.

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
