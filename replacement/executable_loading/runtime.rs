#![no_std]

extern crate xm1_application_nand;
extern crate xm1_executable_loading;
extern crate xm1_image5_decoder;

use core::{mem::MaybeUninit, panic::PanicInfo, sync::atomic::{AtomicU32, Ordering}};
use xm1_executable_loading::client::CompletionWords;
use xm1_executable_loading::native::{self, LoadingConfiguration};

static POLICY: u32 = 0;
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

unsafe extern "C" fn quiescent(_: *mut ()) -> u32 {
    #[cfg(any(loading_qualification_fujimod, loading_qualification_external))]
    { 1 }
    #[cfg(loading_qualification_retention)]
    { 0 }
}

#[cfg(loading_qualification_fujimod)]
const DRAIN_KIND: u32 = 1;
#[cfg(any(loading_qualification_external, loading_qualification_retention))]
const DRAIN_KIND: u32 = 2;

static mut CONFIGURATION: MaybeUninit<LoadingConfiguration> = MaybeUninit::uninit();

/// The opt-in qualification artifact supplies this schema-1 policy. Production
/// composition must supply its own caller policy; there is no library default.
#[no_mangle]
pub unsafe extern "C" fn xm1_loading_configuration() -> *const LoadingConfiguration {
    let slot = &raw mut CONFIGURATION;
    unsafe {
        (*slot).write(LoadingConfiguration {
            version: 1,
            deadline_context: core::ptr::addr_of!(POLICY).cast_mut().cast(),
            deadline: Some(deadline),
            drain_kind: DRAIN_KIND,
            quiescence_context: core::ptr::addr_of!(POLICY).cast_mut().cast(),
            quiescence: Some(quiescent),
        });
        (*slot).as_ptr()
    }
}

/// Bounded host entry: configure the real owner, post through its Sender, then
/// execute one Receiver iteration over the RTOS fixture contract.
#[no_mangle]
pub unsafe extern "C" fn xm1_loading_qualification_drive(
    kind: u32,
    callback: u32,
    argument: u32,
    result: *mut CompletionWords,
) -> i32 {
    let status = unsafe { native::xm1_loading_startup(0) };
    if status != 0 { return status; }
    let status = unsafe { native::xm1_loading_legacy_request(kind, callback, argument) };
    if status != 0 { return status; }
    let status = unsafe { native::xm1_loading_receive_once() };
    if status != 0 { return status; }
    unsafe { native::xm1_loading_observe_completion(kind, result) }
}

#[panic_handler]
fn panic(_: &PanicInfo<'_>) -> ! {
    loop { core::hint::spin_loop(); }
}
