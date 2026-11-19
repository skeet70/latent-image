#![cfg_attr(not(test), no_std)]
#![deny(unsafe_code)]

pub mod completion;
pub mod handles;
pub mod requests;
pub mod routing;
#[allow(unsafe_code)]
pub mod stock;
#[allow(unsafe_code)]
pub mod stock_requests;

#[cfg(test)]
mod tests;
#[cfg(test)]
mod routing_tests;

#[cfg(all(not(test), file_service_image))]
#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {}
}
