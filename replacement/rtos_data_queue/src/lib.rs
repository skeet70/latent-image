//! Owned data-queue family of the X-M1 1.50 RTOS (#156); no guest memory outside the seam.
#![cfg_attr(not(test), no_std)]

#[path = "../../rtos_layout/src/lib.rs"]
pub mod layout;

pub mod admission;
pub mod create;
pub mod env;
pub mod id;
pub mod ops;
pub mod ring;
pub mod status;
pub mod timeout;

#[cfg(any(test, feature = "sim"))]
pub mod sim;

#[cfg(test)]
mod tests;

#[cfg(all(target_arch = "arm", not(test), not(feature = "stage")))]
#[panic_handler]
fn panic(_: &core::panic::PanicInfo<'_>) -> ! {
    loop {}
}

pub use admission::{admit, Admitted, Entry, Facts, Params, Records, Request, Service};
pub use create::{create_data_queue, delete_data_queue, DataQueueCreate, QueueAttributes};
pub use env::{
    release, Deadline, DispatchTarget, Env, HandlerContext, QueueOrder, SavedInterrupts,
    SavedMask, Selection, Tick, TimeoutCallback, TimerEntryAddress, WaitCompletion,
};
pub use id::{own_data_queue_slot, resolve_data_queue, DataQueueId, EncodedId, EntryDomain};
pub use layout::{DataQueueSlot, Domain, TaskSlot};
pub use ops::{
    force_send, receive, resume_receive, resume_send, send, send_poll, send_timeout, ReceiveBlocking,
    ReceivePending, ReceiveResume, SendBlocking, SendPending, SendResume,
};
pub use ring::{BufferedCount, Capacity, OutWord, Payload, Ring, RingBase, RingIndex};
pub use status::{ObjectStatus, WaitOutcome};
pub use timeout::Timeout;
