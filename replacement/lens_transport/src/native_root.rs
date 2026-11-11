#![no_std]

extern crate xm1_lens_transport;

#[panic_handler]
fn panic(_: &core::panic::PanicInfo<'_>)->! { loop { core::hint::spin_loop(); } }
