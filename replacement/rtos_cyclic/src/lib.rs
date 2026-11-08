//! Allocation-free CYCLIC host core: the stock creation and control records, the
//! scheduler-lock lifecycle and the expiry callback. Timer heap membership comes from
//! the owned TIMER core; expiry requires its detached pop.
#![cfg_attr(not(test), no_std)]

#[path = "../../rtos_layout/src/lib.rs"]
pub mod layout;

pub mod id;
pub mod machine;
pub mod ops;
pub mod status;
pub mod types;

#[cfg(feature = "stage")]
#[path = "../stage/abi.rs"]
pub mod stage;

#[cfg(test)]
mod tests;

pub use id::{target_context, target_domain, own_cyclic_slot, EncodedId};
pub use layout::{CyclicConfig, CyclicControl, CyclicSlot, Domain, GuestAddress, KernelContext};
pub use machine::Machine;
pub use ops::{configure, create, delete, expire, initialize, schedule, start, stop};
pub use status::CyclicStatus;
pub use types::{
    CyclicArgument, CyclicAttributes, CyclicCreate, CyclicHandler, CyclicPeriod, CyclicPhase,
    Expiry, PeriodWord, SavedInterrupts,
};

#[cfg(all(target_arch = "arm", not(test), not(feature = "stage")))]
#[panic_handler]
fn panic(_: &core::panic::PanicInfo<'_>) -> ! {
    loop {}
}
