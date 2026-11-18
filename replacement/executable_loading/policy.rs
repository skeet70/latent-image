#![no_std]

//! The schema-2 loading configuration a composition links. Exactly one
//! variant is linked; the loader library has no timeout or drain default.
//!
//! Timeout: the stock-wrapper callers are Root startup, whose caller policy is
//! `STARTUP_CLASS_HANG_BOUND`. Only the retention control links an explicit
//! qualification timeout (`XM1_LOADING_TEST_TIMEOUT_MS`) chosen so the false
//! drain leaf expires within a bounded run.
//! Drain: every variant is a qualification choice; no physical drain exists.

extern crate xm1_executable_loading;

use core::mem::MaybeUninit;
use xm1_executable_loading::native::LoadingConfiguration;
#[cfg(not(loading_qualification_retention))]
use xm1_executable_loading::timeout::STARTUP_CLASS_HANG_BOUND;

static POLICY: u32 = 0;

#[cfg(not(loading_qualification_retention))]
const LEGACY_TIMEOUT_MILLISECONDS: u32 = STARTUP_CLASS_HANG_BOUND.get();

#[cfg(loading_qualification_retention)]
const LEGACY_TIMEOUT_MILLISECONDS: u32 = {
    let bytes = env!("XM1_LOADING_TEST_TIMEOUT_MS").as_bytes();
    let mut value = 0u32;
    let mut index = 0;
    assert!(!bytes.is_empty(), "test timeout is required");
    while index < bytes.len() {
        assert!(bytes[index].is_ascii_digit(), "test timeout is decimal milliseconds");
        value = value * 10 + (bytes[index] - b'0') as u32;
        index += 1;
    }
    assert!(value != 0, "test timeout is nonzero");
    value
};

/// The linked stock-wrapper timeout, exported so artifact checks can bind it.
#[no_mangle]
pub static XM1_LOADING_LEGACY_TIMEOUT_MS: u32 = LEGACY_TIMEOUT_MILLISECONDS;

#[cfg(any(loading_qualification_external, loading_qualification_retention))]
unsafe extern "C" fn quiescent(_: *mut ()) -> u32 {
    #[cfg(loading_qualification_external)]
    { 1 }
    #[cfg(loading_qualification_retention)]
    { 0 }
}

static mut CONFIGURATION: MaybeUninit<LoadingConfiguration> = MaybeUninit::uninit();

/// The configuration startup obtains before constructing task60.
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
            version: 2,
            legacy_timeout_milliseconds: core::ptr::read_volatile(&XM1_LOADING_LEGACY_TIMEOUT_MS),
            drain_kind,
            quiescence_context: context,
            quiescence,
        });
        (*slot).as_ptr()
    }
}
