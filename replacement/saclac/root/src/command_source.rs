#![forbid(unsafe_code)]

use xm1_saclac_interface::{Command, CommandKind};
use xm1_saclac_shared::stock::Machine;

/// Borrows the same staged command that scalar CommandMemory reads. The command cannot escape.
pub trait CommandSource: Machine {
    fn with_command<T>(&self, kind: CommandKind, body: impl FnOnce(Command<'_>) -> T) -> T;
}
