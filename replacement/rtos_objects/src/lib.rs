//! Owned eventflag and mailbox policy of the X-M1 1.50 RTOS (#156, step F0); no guest memory.
#![cfg_attr(not(test), no_std)]

#[path = "../../rtos_layout/src/lib.rs"]
pub mod layout;

pub mod admission;
pub mod block;
pub mod create;
pub mod id;
pub mod locked;
pub mod matching;
pub mod message;
pub mod order;
pub mod status;
pub mod teardown;
pub mod timeout;
pub mod wake;

#[cfg(any(test, feature = "sim"))]
pub mod sim;

#[cfg(test)]
mod tests;

#[cfg(all(target_arch = "arm", not(test), not(feature = "stage")))]
#[panic_handler]
fn panic(_: &core::panic::PanicInfo<'_>) -> ! {
    loop {}
}

pub use admission::{
    admit, flag_wait_locked, poll_result, Admitted, Entry, Facts, Object, Params, Records, Request,
    Service,
};
pub use block::{
    receive, resume_flag, resume_receive, wait_flag, Block, Deadline, FlagBlocking, FlagWaitPending,
    FlagResume, IrqScope, ReceiveBlocking, ReceivePending, ReceiveResume, SavedMask, Tick,
    TimerEntryAddress, TimeoutCallback,
};
pub use create::{
    create_flag, create_mailbox, FlagAttributes, FlagCreate, Machine, MailboxAttributes, MailboxCreate,
    SavedInterrupts,
};
pub use id::{
    own_flag_slot, own_mailbox_slot, resolve_flag, resolve_flag_with, resolve_mailbox,
    resolve_mailbox_with, EncodedId, EntryDomain, FlagId, MailboxId,
};
pub use layout::{Domain, DomainId, FlagSlot, MailboxSlot, TaskSlot};
pub use locked::{clear_flag, poll_flag, poll_receive, ObjectLock, OutWord};
pub use matching::{check, KeepMask, MatchMode, Pattern, WantedMask};
pub use message::{MessagePriority, PriorityLimit};
pub use order::{precedes, QueueOrder};
pub use status::{ObjectStatus, WaitOutcome};
pub use teardown::{delete_flag, delete_mailbox, MessageDrain};
pub use timeout::Timeout;
pub use wake::{send, set_flag, set_flag_handler, DispatchTarget, Selection, Wake, WakeMode};
