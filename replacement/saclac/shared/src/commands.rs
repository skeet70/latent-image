#![forbid(unsafe_code)]

//! Queue-43 command parameters after their event, the commands' pending flags and the client
//! replies more than one module sends.

use xm1_saclac_interface::stock::{command_code, PARAMETER_BLOCKS};
use xm1_saclac_interface::{ClientCallback, CommandKind, CompletionStatus};

use crate::lens::{LensControl, LensNotices, LensProbe, Notice, ProbeState};
use crate::settings::{Channel, Channels};

/// A command whose stored parameters modules read after its event.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum StoredCommand { Command02, Command03, HalfPress, Autofocus, LensDrive, Command17 }

impl StoredCommand {
    pub const fn kind(self) -> CommandKind {
        match self {
            Self::Command02 => CommandKind::Command02,
            Self::Command03 => CommandKind::Command03,
            Self::HalfPress => CommandKind::HalfPress,
            Self::Autofocus => CommandKind::Autofocus,
            Self::LensDrive => CommandKind::LensDrive,
            Self::Command17 => CommandKind::Command17,
        }
    }

    /// Words in the command's parameter block.
    pub fn words(self) -> usize {
        PARAMETER_BLOCKS[usize::from(command_code(self.kind()))].capacity / 4
    }
}

/// The parameter blocks and pending flags of queue-43 commands. `index` is below the command's
/// `words`.
pub trait CommandMemory {
    fn word(&self, command: StoredCommand, index: usize) -> u32;
    fn set_pending(&mut self, command: CommandKind, pending: bool);
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClientArguments {
    Status(CompletionStatus),
    Reply { status: CompletionStatus, words: [u32; 4] },
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

    pub fn set_pending(&mut self, command: CommandKind, pending: bool) { self.memory.set_pending(command, pending) }

    fn callback(&self, command: StoredCommand) -> Option<ClientCallback> {
        xm1_saclac_interface::stock::optional_callback(self.memory.word(command, 0))
    }

    fn finish(&mut self, command: StoredCommand, status: CompletionStatus) {
        if ends_command(status) {
            self.memory.set_pending(command.kind(), false);
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
            clients.enter(callback, ClientArguments::Reply { status: reply.status, words: [reply.channel.0 as u32, third, fourth, fifth] });
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
            clients.enter(callback, ClientArguments::Reply { status, words: [value, client, u32::from(noticed), probes] });
        }
    }
}
