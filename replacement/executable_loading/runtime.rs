#![no_std]

//! Native-qualification harness only. The production composition links the
//! loading owner into the Root fixed bank; this root adds a bounded drive entry.

extern crate xm1_application_nand;
extern crate xm1_executable_loading;
extern crate xm1_image5_decoder;
extern crate xm1_loading_policy;

use core::panic::PanicInfo;
use xm1_executable_loading::client::CompletionWords;
use xm1_executable_loading::native;

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
