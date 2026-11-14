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
                self.0.enter(entry, &arguments[..words.len() + 1]);
            }
            Values::Record(record) => { self.0.enter(entry, &[status.0 as usize, record.as_ptr() as usize]); }
        }
    }
}

impl<M: Machine + Clone, R: Memory, E: StockEntries, P> Replies for Services<M, R, E, P> {
    fn reply(&mut self, command: ReplyCommand, status: CompletionStatus, values: Values<'_>) {
        let mut blocks = Blocks(self.machine.clone());
        let mut clients = Clients(self.machine.clone());
        Composer { blocks: &mut blocks, lens: self, clients: &mut clients }.reply(command, status, values);
    }
}
impl<M: Machine + Clone, R: Memory, E: StockEntries, P> CommandParameters for Services<M, R, E, P> {
    fn word(&mut self, command: CommandKind, index: usize) -> u32 { parameter_word(&self.machine, command, index) }
}
