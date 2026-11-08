//! Owned semaphore services of the X-M1 1.50 RTOS (#156); no guest memory.
#![cfg_attr(not(test), no_std)]

#[path = "../../rtos_layout/src/lib.rs"]
pub mod layout;

pub mod admission;
pub mod block;
pub mod count;
pub mod create;
pub mod id;
pub mod locks;
pub mod order;
pub mod signal;
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

pub use admission::{admit, require_created, Entry, Request, Service};
pub use block::{
    block, poll, resume, timed_wait, wait, Block, Enqueue, SemaphoreBlocking, SemaphorePending,
    SemaphoreResume, TimerEntryAddress,
};
pub use count::{Count, Maximum, SemaphoreAttributes};
pub use create::{create_semaphore, Machine, SavedInterrupts, SemaphoreCreate};
pub use id::{resolve, EncodedId, EntryDomain, SemaphoreId};
pub use locks::{IrqScope, Locks};
pub use order::QueueOrder;
pub use signal::signal;
pub use status::{SemaphoreStatus, WaitOutcome};
pub use teardown::delete_semaphore;
pub use timeout::Timeout;
pub use wake::{
    release_with, DispatchTarget, Primitives, Release, Selection, WaitCompletion, Wake, WakeMode,
};
