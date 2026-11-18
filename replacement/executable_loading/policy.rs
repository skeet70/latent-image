#![no_std]

//! Explicit qualification-only loading policy. A composition links exactly one
//! variant; the library itself has no duration, default or cleanup budget.

extern crate xm1_executable_loading;

use core::{mem::MaybeUninit, sync::atomic::{AtomicU32, Ordering}};
use xm1_executable_loading::native::{self, LoadingConfiguration};

static POLICY: u32 = 0;

/// Number of absolute deadlines this policy formed; one per request.
#[no_mangle]
pub static XM1_LOADING_DEADLINE_CALLS: AtomicU32 = AtomicU32::new(0);

fn policy_ticks() -> Option<u64> {
    let mut value = 0u64;
    let bytes = env!("XM1_LOADING_POLICY_TICKS").as_bytes();
    if bytes.is_empty() { return None; }
    for byte in bytes {
        if !byte.is_ascii_digit() { return None; }
        value = value.checked_mul(10)?.checked_add(u64::from(*byte - b'0'))?;
    }
    if value == 0 { None } else { Some(value) }
}

unsafe extern "C" fn deadline(context: *mut (), _: u32, result: *mut u64) -> i32 {
    XM1_LOADING_DEADLINE_CALLS.fetch_add(1, Ordering::Relaxed);
    if context != core::ptr::addr_of!(POLICY).cast_mut().cast() || result.is_null() { return -30; }
    let Some(ticks) = policy_ticks() else { return -30; };
    let Some(absolute) = native::current_count().checked_add(ticks) else { return -30; };
    if absolute == 0 { return -30; }
    unsafe { result.write(absolute); }
    0
}

#[cfg(any(loading_qualification_external, loading_qualification_retention))]
unsafe extern "C" fn quiescent(_: *mut ()) -> u32 {
    #[cfg(loading_qualification_external)]
    { 1 }
    #[cfg(loading_qualification_retention)]
    { 0 }
}

static mut CONFIGURATION: MaybeUninit<LoadingConfiguration> = MaybeUninit::uninit();

/// The schema-1 configuration startup obtains before constructing task60.
#[no_mangle]
pub unsafe extern "C" fn xm1_loading_configuration() -> *const LoadingConfiguration {
    let context = core::ptr::addr_of!(POLICY).cast_mut().cast();
    #[cfg(loading_qualification_fujimod)]
    let (drain_kind, quiescence) = (1, None);
    #[cfg(any(loading_qualification_external, loading_qualification_retention))]
    let (drain_kind, quiescence) = (2, Some(quiescent as unsafe extern "C" fn(*mut ()) -> u32));
    let slot = &raw mut CONFIGURATION;
    unsafe {
        (*slot).write(LoadingConfiguration {
            version: 1,
            deadline_context: context,
            deadline: Some(deadline),
            drain_kind,
            quiescence_context: context,
            quiescence,
        });
        (*slot).as_ptr()
    }
}
