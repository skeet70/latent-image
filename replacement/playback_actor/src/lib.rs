//! Owned queue-4 playback machines of X-M1 1.50. Each machine's action functions replace the
//! stock ones in place in the plain image, and its action-table rows are republished to name
//! them; the stock dispatcher 0x011e8bf8 keeps selecting actions by event and state.
#![cfg_attr(not(test), no_std)]
#![deny(unsafe_code)]

pub mod machine0;
#[cfg(any(test, playback_native_machines))]
pub mod machine30;
#[cfg(any(test, playback_native_machines))]
pub mod machine7;
#[cfg(any(test, playback_native_machines))]
pub mod machine61;
pub mod request;
#[cfg(target_arch = "arm")]
#[allow(unsafe_code)]
mod stock;

#[cfg(test)]
mod tests;

/// A device notification packet handed to the retained notification dispatcher 0x72b680.
/// Stock builds it at the start of an 80-byte frame and leaves the rest unwritten.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DevicePacket {
    pub code: u32,
    pub value: u32,
}

impl DevicePacket {
    pub const fn new(code: u32, value: u32) -> Self {
        Self { code, value }
    }
}

#[cfg(all(target_arch = "arm", not(test)))]
#[panic_handler]
fn panic(_information: &core::panic::PanicInfo<'_>) -> ! {
    loop {
        core::hint::spin_loop();
    }
}
