#![forbid(unsafe_code)]

use xm1_saclac_controllers::{layers::CommandParameters, replies::{self, CommandBlocks, Composer, Replies, ReplyCommand, Values}};
use xm1_saclac_interface::{stock::{callback_word, reply_target_code, PARAMETER_BLOCKS}, ClientCallback, CommandKind, CompletionStatus, ReplyTarget};
use xm1_saclac_profile_records::{Memory, StockEntries};
use xm1_saclac_shared::{commands::CommandMemory, stock::{Machine, Stock}};
use crate::services::Services;

pub(crate) fn parameter_word(machine: &impl Machine, command: CommandKind, index: usize) -> u32 {
    let code = reply_target_code(ReplyTarget::Command(command));
    let block = PARAMETER_BLOCKS[usize::from(code)];
    assert!(index < block.capacity / 4, "command parameter word outside its block");
    machine.read_u32(block.address + 4 * index as u32)
}

pub(crate) fn scalar_reply<M: Machine + Clone>(machine: M, command: CommandKind, status: CompletionStatus) {
    command_reply(machine, command, status, &[]);
}

pub(crate) fn command_reply<M: Machine + Clone>(machine: M, command: CommandKind, status: CompletionStatus, words: &[u32]) {
    let callback = xm1_saclac_interface::stock::optional_callback(parameter_word(&machine, command, 0));
    if replies::ends_command(status) {
        Stock::new(machine.clone()).set_pending(ReplyTarget::Command(command), false);
    }
    if let Some(callback) = callback {
        replies::Clients::enter(&mut Clients(machine), callback, status, Values::Words(words));
    }
}

/// 0xefb984: the reply record the client reads through r1, `leading` then `words`, copied to an
/// aligned record the client copies whole.
pub(crate) fn record_reply<M: Machine + Clone>(machine: M, command: CommandKind, status: CompletionStatus, leading: u32, words: &[u32; 3]) {
    let callback = xm1_saclac_interface::stock::optional_callback(parameter_word(&machine, command, 0));
    if replies::ends_command(status) {
        Stock::new(machine.clone()).set_pending(ReplyTarget::Command(command), false);
    }
    if let Some(callback) = callback {
        let mut record = crate::stock::acquisition::Aligned([0; 16]);
        for (slot, word) in record.0.chunks_exact_mut(4).zip([leading, words[0], words[1], words[2]]) {
            slot.copy_from_slice(&word.to_le_bytes());
        }
        replies::Clients::enter(&mut Clients(machine), callback, status, Values::Record(&record.0));
    }
}

pub(crate) fn autofocus_reply<M: Machine + Clone>(machine: M, status: CompletionStatus) {
    xm1_saclac_shared::commands::Commands::new(Stock::new(machine.clone())).reply_autofocus_lens(
        status, &mut Stock::new(machine.clone()), &mut Stock::new(machine.clone()), &mut Stock::new(machine));
}

struct Blocks<M>(M);
impl<M: Machine + Clone> CommandBlocks for Blocks<M> {
    fn word(&mut self, command: CommandKind, index: usize) -> u32 { parameter_word(&self.0, command, index) }
    fn end(&mut self, command: CommandKind) { Stock::new(self.0.clone()).set_pending(ReplyTarget::Command(command), false); }
}

struct Clients<M>(M);
impl<M: Machine> replies::Clients for Clients<M> {
    fn enter(&mut self, callback: ClientCallback, status: CompletionStatus, values: Values<'_>) {
        let entry = callback_word(callback);
        match values {
            Values::Words(words) => {
                let mut arguments = [0; 6];
                arguments[0] = status.0 as usize;
                for (argument, word) in arguments[1..].iter_mut().zip(words) { *argument = *word as usize; }
                assert!(words.len() < arguments.len(), "controller reply exceeds its ABI");
                self.0.enter_client(entry, &arguments[..words.len() + 1], None);
            }
            Values::Record(record) => {
                let shape = Some(xm1_saclac_shared::stock::ClientRecord { index: 1, bytes: record.len() });
                self.0.enter_client(entry, &[status.0 as usize, record.as_ptr() as usize], shape);
            }
        }
    }
}

pub(crate) fn reply_with<M: Machine + Clone, L: xm1_saclac_controllers::layers::LensControl>(machine: M, lens: &mut L, command: ReplyCommand, status: CompletionStatus, values: Values<'_>) {
    let mut blocks = Blocks(machine.clone());
    let mut clients = Clients(machine);
    Composer { blocks: &mut blocks, lens, clients: &mut clients }.reply(command, status, values);
}

impl<M: Machine + Clone, R: Memory, E: StockEntries, P> Replies for Services<M, R, E, P> {
    fn reply(&mut self, command: ReplyCommand, status: CompletionStatus, values: Values<'_>) {
        reply_with(self.machine.clone(), self, command, status, values);
    }
}
impl<M: Machine + Clone, R: Memory, E: StockEntries, P> CommandParameters for Services<M, R, E, P> {
    fn word(&mut self, command: CommandKind, index: usize) -> u32 { parameter_word(&self.machine, command, index) }
}
