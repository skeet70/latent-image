#![forbid(unsafe_code)]

//! Queue-43 command parameters after their event, the commands' pending flags and the client
//! replies more than one module sends.

use xm1_saclac_interface::stock::{reply_target_code, PARAMETER_BLOCKS};
use xm1_saclac_interface::{ClientCallback, ClientNotification, CommandKind, CompletionStatus, ReplyTarget};

use crate::lens::{LensControl, LensNotices, LensProbe, Notice, ProbeState};
use crate::settings::{Channel, Channels};

/// A queue-43 message whose stored parameters modules read after its event.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum StoredCommand {
    Command02,
    Command03,
    LensBringUp,
    LensStop,
    HalfPress,
    Autofocus,
    LensDrive,
    Cancel,
    Command10,
    Command17,
    Step,
    Command2c,
    Command2d,
}

impl StoredCommand {
    pub const fn target(self) -> ReplyTarget {
        ReplyTarget::Command(match self {
            Self::Command02 => CommandKind::Command02,
            Self::Command03 => CommandKind::Command03,
            Self::LensBringUp => CommandKind::LensBringUp,
            Self::LensStop => CommandKind::LensStop,
            Self::HalfPress => CommandKind::HalfPress,
            Self::Autofocus => CommandKind::Autofocus,
            Self::LensDrive => CommandKind::LensDrive,
            Self::Cancel => CommandKind::Cancel,
            Self::Command10 => CommandKind::Command10,
            Self::Command17 => CommandKind::Command17,
            Self::Command2c => CommandKind::Command2c,
            Self::Command2d => CommandKind::Command2d,
            Self::Step => return ReplyTarget::Notification(ClientNotification::Notification1b),
        })
    }

    /// The message code whose parameter block holds the words.
    pub fn code(self) -> u8 { reply_target_code(self.target()) }

    /// Words in the message's parameter block.
    pub fn words(self) -> usize { PARAMETER_BLOCKS[usize::from(self.code())].capacity / 4 }
}

/// The parameter blocks and pending flags of queue-43 commands. `index` is below the command's
/// `words`.
pub trait CommandMemory {
    fn word(&self, command: StoredCommand, index: usize) -> u32;
    fn set_word(&mut self, command: StoredCommand, index: usize, value: u32);
    /// Sets the pending byte of the message whose code `target` has.
    fn set_pending(&mut self, target: ReplyTarget, pending: bool);
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClientArguments {
    Status(CompletionStatus),
    Reply { status: CompletionStatus, words: ReplyWords },
}

impl ClientArguments {
    pub const fn status(self) -> CompletionStatus {
        match self {
            Self::Status(status) | Self::Reply { status, .. } => status,
        }
    }
}

/// The one to four words a client reply passes after its status.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReplyWords {
    words: [u32; 4],
    count: usize,
}

impl ReplyWords {
    /// Panics unless `words` holds one to four words.
    pub fn new(words: &[u32]) -> Self {
        assert!((1..=4).contains(&words.len()), "a client reply passes one to four words after its status");
        let mut stored = [0; 4];
        stored[..words.len()].copy_from_slice(words);
        Self { words: stored, count: words.len() }
    }

    pub fn words(&self) -> &[u32] { &self.words[..self.count] }
}

pub trait Clients {
    fn enter(&mut self, callback: ClientCallback, arguments: ClientArguments);
}

/// Whether a reply with `status` ends its command and clears its pending flag.
pub const fn ends_command(status: CompletionStatus) -> bool { status.0 <= 0 || status.0 == 20 || status.0 == 21 }

/// The client's own word reaches the reply only when `channel` is a configured channel.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LensDriveReply {
    pub status: CompletionStatus,
    pub channel: Channel,
    pub words: [u32; 2],
}

/// The word a lens-drive reply carries in place of the client's when its channel is not
/// configured.
pub const UNCONFIGURED_CHANNEL: u32 = 0x80 << 16;

pub struct Commands<M> {
    memory: M,
}

impl<M: CommandMemory> Commands<M> {
    pub const fn new(memory: M) -> Self { Self { memory } }

    pub fn memory(&self) -> &M { &self.memory }

    pub fn word(&self, command: StoredCommand, index: usize) -> Option<u32> {
        (index < command.words()).then(|| self.memory.word(command, index))
    }

    /// Stores `value` when `index` is inside the block; `false` when it is not.
    pub fn set_word(&mut self, command: StoredCommand, index: usize, value: u32) -> bool {
        let inside = index < command.words();
        if inside {
            self.memory.set_word(command, index, value);
        }
        inside
    }

    pub fn set_pending(&mut self, command: CommandKind, pending: bool) { self.memory.set_pending(ReplyTarget::Command(command), pending) }

    fn callback(&self, command: StoredCommand) -> Option<ClientCallback> {
        xm1_saclac_interface::stock::optional_callback(self.memory.word(command, 0))
    }

    fn finish(&mut self, command: StoredCommand, status: CompletionStatus) {
        if ends_command(status) {
            self.memory.set_pending(command.target(), false);
        }
    }

    /// Replies to `command`'s client with `arguments` as the reply entries 0xefa348, 0xefa3a8,
    /// 0xefb194, 0xefb1dc, 0xefb604, 0xefb654 and 0xefb6ac do.
    pub fn reply(&mut self, command: StoredCommand, arguments: ClientArguments, clients: &mut impl Clients) {
        let callback = self.callback(command);
        self.finish(command, arguments.status());
        if let Some(callback) = callback {
            clients.enter(callback, arguments);
        }
    }

    /// Replies to the lens-drive command (0xefb104).
    pub fn reply_lens_drive(&mut self, reply: LensDriveReply, channels: &impl Channels, clients: &mut impl Clients) {
        let callback = self.callback(StoredCommand::LensDrive);
        let client = self.memory.word(StoredCommand::LensDrive, 2);
        self.finish(StoredCommand::LensDrive, reply.status);
        if let Some(callback) = callback {
            let third = if channels.is_valid(reply.channel) { client } else { UNCONFIGURED_CHANNEL };
            let [fourth, fifth] = reply.words;
            clients.enter(callback, ClientArguments::Reply { status: reply.status, words: ReplyWords::new(&[reply.channel.0 as u32, third, fourth, fifth]) });
        }
    }

    /// Replies to command 0x17 with its status (0xefb4b8).
    pub fn reply_command17(&mut self, status: CompletionStatus, clients: &mut impl Clients) {
        let callback = self.callback(StoredCommand::Command17);
        self.finish(StoredCommand::Command17, status);
        if let Some(callback) = callback {
            clients.enter(callback, ClientArguments::Status(status));
        }
    }

    /// Replies to the autofocus command with the lens state and consumes the lens control's three
    /// notices (0xf46c4c through 0xefb094).
    pub fn reply_autofocus_lens(
        &mut self,
        status: CompletionStatus,
        lens: &mut impl LensControl,
        notices: &mut impl LensNotices,
        clients: &mut impl Clients,
    ) {
        let overridden = |lens: &mut _, probe| LensControl::probe(lens, probe) == ProbeState::Overridden;
        let probes = u32::from(overridden(lens, LensProbe::First)) << 1
            | u32::from(overridden(lens, LensProbe::Second))
            | u32::from(overridden(lens, LensProbe::Third)) << 2;
        let value = lens.value();
        let noticed = [Notice::First, Notice::Second, Notice::Third].into_iter().any(|notice| notices.notice(notice) == 1);
        notices.clear_notices();

        let callback = self.callback(StoredCommand::Autofocus);
        let client = self.memory.word(StoredCommand::Autofocus, 39);
        self.finish(StoredCommand::Autofocus, status);
        if let Some(callback) = callback {
            clients.enter(callback, ClientArguments::Reply { status, words: ReplyWords::new(&[value, client, u32::from(noticed), probes]) });
        }
    }
}
