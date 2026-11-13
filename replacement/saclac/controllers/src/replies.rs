#![forbid(unsafe_code)]

//! Client reply composition. A reply enters the callback its command's parameter block names
//! with the status and the reply's values, and ends the command when the status does.

use crate::layers::{LensControl, LensProbe};
use xm1_saclac_interface::{stock, ClientCallback, CommandKind, CompletionStatus};

/// What a reply hands the client beside its status.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Values<'a> {
    Words(&'a [u32]),
    /// A record the client reads through its address.
    Record(&'a [u8]),
}

/// The commands the controllers answer, each through its own reply entry. Command0a's
/// statistics release finishes through `Command09`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReplyCommand {
    Command02,
    Command03,
    Command09,
    Command0a,
    HalfRelease,
    Command11,
    Command13,
    Command17,
    Prepare,
    Command1d,
    Command1e,
    Command1f,
    Command20,
    Command21,
    Command22,
    Command23,
    Command24,
    Prepare26,
    Command29,
}

impl ReplyCommand {
    pub const fn kind(self) -> CommandKind {
        match self {
            Self::Command02 => CommandKind::Command02,
            Self::Command03 => CommandKind::Command03,
            Self::Command09 => CommandKind::Command09,
            Self::Command0a => CommandKind::Command0a,
            Self::HalfRelease => CommandKind::HalfRelease,
            Self::Command11 => CommandKind::Command11,
            Self::Command13 => CommandKind::Command13,
            Self::Command17 => CommandKind::Command17,
            Self::Prepare => CommandKind::Prepare,
            Self::Command1d => CommandKind::Command1d,
            Self::Command1e => CommandKind::Command1e,
            Self::Command1f => CommandKind::Command1f,
            Self::Command20 => CommandKind::Command20,
            Self::Command21 => CommandKind::Command21,
            Self::Command22 => CommandKind::Command22,
            Self::Command23 => CommandKind::Command23,
            Self::Command24 => CommandKind::Command24,
            Self::Prepare26 => CommandKind::Prepare26,
            Self::Command29 => CommandKind::Command29,
        }
    }

    /// The word of the command's own parameters its reply carries third (0xefa27c, 0xefa2f4).
    const fn echoed(self) -> Option<usize> {
        match self {
            Self::Command02 => Some(6),
            Self::Command03 => Some(5),
            _ => None,
        }
    }
}

/// The reply composition the controllers' modules answer their clients through.
pub trait Replies {
    fn reply(&mut self, command: ReplyCommand, status: CompletionStatus, values: Values<'_>);
}

/// The parameter blocks and pending flags the commands leave.
pub trait CommandBlocks {
    /// Word `index` of `command`'s parameters; word 0 names the client's callback, zero for none.
    fn word(&mut self, command: CommandKind, index: usize) -> u32;
    /// Clears `command`'s pending flag.
    fn end(&mut self, command: CommandKind);
}

pub trait Clients {
    fn enter(&mut self, callback: ClientCallback, status: CompletionStatus, values: Values<'_>);
}

pub const fn ends_command(status: CompletionStatus) -> bool { status.0 <= 0 || status.0 == 20 || status.0 == 21 }

/// The Prepare status that notices the preparation; it reaches the client only while bit 0 of
/// the reply's flags is set (0xefb5a0..0xefb5b0).
pub const PREPARATION_NOTICE: CompletionStatus = CompletionStatus(19);

const MOST_WORDS: usize = 5;

/// Composes the replies of entries 0xefa258..0xefbad8 the controllers call.
pub struct Composer<'a, B, L, C> {
    pub blocks: &'a mut B,
    pub lens: &'a mut L,
    pub clients: &'a mut C,
}

impl<B: CommandBlocks, L: LensControl, C: Clients> Composer<'_, B, L, C> {
    fn callback(&mut self, command: ReplyCommand, status: CompletionStatus, values: Values<'_>) -> Option<ClientCallback> {
        let noticed = |words: &[u32]| words.first().is_some_and(|flags| flags & 1 != 0);
        match (command, values) {
            (ReplyCommand::Prepare, Values::Words(words)) if status == PREPARATION_NOTICE && !noticed(words) => None,
            _ => stock::optional_callback(self.blocks.word(command.kind(), 0)),
        }
    }
}

impl<B: CommandBlocks, L: LensControl, C: Clients> Replies for Composer<'_, B, L, C> {
    fn reply(&mut self, command: ReplyCommand, status: CompletionStatus, values: Values<'_>) {
        let kind = command.kind();
        let callback = self.callback(command, status, values);
        let mut composed = [0; MOST_WORDS];
        let values = match values {
            Values::Words(words) => {
                let mut length = 0;
                let mut push = |word: u32| {
                    composed[length] = word;
                    length += 1;
                };
                let (head, tail) = words.split_at(words.len().min(2));
                head.iter().copied().for_each(&mut push);
                if let Some(index) = command.echoed() {
                    push(self.blocks.word(kind, index));
                }
                tail.iter().copied().for_each(&mut push);
                if command == ReplyCommand::Command11 {
                    let first = u32::from(self.lens.overridden(LensProbe::First));
                    let second = u32::from(self.lens.overridden(LensProbe::Second));
                    push(first << 1 | second);
                }
                Values::Words(&composed[..length])
            }
            Values::Record(record) => Values::Record(record),
        };
        if ends_command(status) {
            self.blocks.end(kind);
        }
        if let Some(callback) = callback {
            self.clients.enter(callback, status, values);
        }
    }
}
