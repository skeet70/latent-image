//! Owned eventflag and mailbox policy of the X-M1 1.50 RTOS (#156, step F0); no guest memory.
#![cfg_attr(not(test), no_std)]

#[path = "../../rtos_layout/src/lib.rs"]
pub mod layout;

pub mod admission;
pub mod id;
pub mod matching;
pub mod message;
pub mod order;
pub mod status;
pub mod timeout;

#[cfg(test)]
mod tests;

#[cfg(all(target_arch = "arm", not(test)))]
#[panic_handler]
fn panic(_: &core::panic::PanicInfo<'_>) -> ! {
    loop {}
}

pub use admission::{admit, flag_wait_locked, poll_result, Admitted, Entry, Facts, Object, Params, Records, Request, Service};
pub use id::{own_flag_slot, own_mailbox_slot, resolve_flag, resolve_mailbox, EncodedId, EntryDomain, FlagId, MailboxId};
pub use layout::{Domain, DomainId, FlagSlot, MailboxSlot, TaskSlot};
pub use matching::{check, KeepMask, MatchMode, Pattern};
pub use message::MessagePriority;
pub use order::{precedes, QueueOrder};
pub use status::{ObjectStatus, WaitOutcome};
pub use timeout::Timeout;
