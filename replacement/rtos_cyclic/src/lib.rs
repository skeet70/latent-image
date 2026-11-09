//! Allocation-free CYCLIC host core: the stock creation and control records, the
//! scheduler-lock lifecycle and the expiry callback. Timer heap membership comes from
//! the owned TIMER core; expiry requires its detached pop.
#![cfg_attr(not(test), no_std)]

#[path = "../../rtos_layout/src/lib.rs"]
pub mod layout;

pub mod app;
pub mod id;
pub mod machine;
pub mod ops;
pub mod status;
pub mod types;

#[cfg(feature = "stage")]
#[path = "../stage/abi.rs"]
pub mod stage;

#[cfg(feature = "stage-app")]
#[path = "../stage/app_abi.rs"]
pub mod stage_app;

#[cfg(test)]
mod app_tests;

#[cfg(test)]
mod tests;

pub use app::{
    register_callback, unregister_callback, CallbackHandler, CallbackMachine, CallbackMode,
    CallbackRequest, CallbackSlot, CallbackSlots, CallbackStatus, CallbackToken, CriticalToken,
    InterfaceId, TokenOutput, DISPATCH_EVENT, ERROR_OPERATION, TOKEN_ATTEMPTS,
};
pub use id::{target_context, target_domain, own_cyclic_slot, EncodedId};
pub use layout::{CyclicConfig, CyclicControl, CyclicSlot, Domain, GuestAddress, KernelContext};
pub use machine::Machine;
pub use ops::{
    configure, create, create_from_request, create_with_request, delete, expire, initialize,
    schedule, start, stop,
};
pub use status::CyclicStatus;
pub use types::{
    CyclicArgument, CyclicAttributes, CyclicCreate, CyclicHandler, CyclicPeriod, CyclicPhase,
    Expiry, PeriodWord, SavedInterrupts,
};

#[cfg(all(
    target_arch = "arm",
    not(test),
    not(feature = "stage"),
    not(feature = "stage-app")
))]
#[panic_handler]
fn panic(_: &core::panic::PanicInfo<'_>) -> ! {
    loop {}
}
