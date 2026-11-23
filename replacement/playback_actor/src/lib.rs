//! Owned queue-4 playback machines of X-M1 1.50. Each machine's action functions replace the
//! stock ones in place in the plain image, and its action-table rows are republished to name
//! them; the stock dispatcher 0x011e8bf8 keeps selecting actions by event and state.
#![cfg_attr(not(test), no_std)]
#![deny(unsafe_code)]

pub mod machine0;
pub mod machine1;
pub mod machine2;
pub mod machine3;
pub mod machine30;
pub mod machine34;
pub mod machine4;
pub mod machine44;
pub mod machine45;
pub mod machine46;
pub mod machine47;
pub mod machine5;
pub mod machine56;
pub mod machine57;
pub mod machine6;
pub mod machine61;
pub mod machine66;
pub mod machine7;
pub mod machine70;
pub mod machine8;
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
