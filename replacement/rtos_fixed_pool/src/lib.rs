//! Owned fixed-size pool policy of the X-M1 1.50 RTOS (#156); no guest memory.
#![cfg_attr(not(test), no_std)]

#[path = "../../rtos_layout/src/lib.rs"]
pub mod layout;

pub mod admission;
pub mod create;
pub mod get;
pub mod id;
pub mod machine;
pub mod release;
pub mod status;
pub mod take;
pub mod teardown;
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

pub use admission::{admit, Admitted, Counts, Entry, Request, Service};
pub use id::{own_pool_slot, resolve_pool, EncodedId, EntryDomain, PoolId};
pub use create::{create_pool, PoolAttributes, PoolCreate};
pub use get::{get, poll, poll_unchecked, resume, OutWord, Pending, PoolBlocking, PoolResume};
pub use machine::{Block, DivMod, DispatchTarget, Enqueue, Irq, Locks, Machine, SaveIrq, SavedInterrupts, Selection, Wake};
pub use release::release;
pub use status::{PoolStatus, WaitOutcome};
pub use take::{take, BlockAddress};
pub use teardown::delete_pool;
pub use timeout::Timeout;

pub use layout::{
    Domain, DomainId, FixedPoolConfig, FixedPoolControl, FixedPoolSlot, FixedPoolWait, GuestAddress,
    KernelContext, Task, TaskSlot,
};
