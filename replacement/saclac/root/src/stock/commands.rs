use xm1_saclac_interface::{stock::{command_code, parameters, PARAMETER_BLOCKS}, Command, CommandKind};
use xm1_saclac_shared::stock::Guest;
use crate::command_source::CommandSource;

impl CommandSource for Guest {
    fn with_command<T>(&self, kind: CommandKind, body: impl FnOnce(Command<'_>) -> T) -> T {
        let block = PARAMETER_BLOCKS[usize::from(command_code(kind))];
        // Queue43 keeps this block alive; the dispatcher serializes writes and the borrowed call.
        let bytes = unsafe { core::slice::from_raw_parts(block.address as *const u8, block.capacity) };
        body(Command { kind, parameters: parameters(bytes) })
    }
}
