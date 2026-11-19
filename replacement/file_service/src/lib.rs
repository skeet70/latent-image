#![cfg_attr(not(test), no_std)]
#![deny(unsafe_code)]

pub mod handles;
#[allow(unsafe_code)]
pub mod stock;

#[cfg(test)]
mod tests;

#[cfg(all(not(test), file_service_image))]
#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {}
}
