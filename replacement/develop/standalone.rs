#![no_std]

#[path = "../capture/src/lib.rs"]
mod capture;
#[path = "../save/src/lib.rs"]
mod save;
#[path = "../media_permit/src/lib.rs"]
mod media_permit;
#[path = "../media_permit/src/stock.rs"]
mod media_permit_stock;
#[path = "src/lib.rs"]
mod develop;

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! { loop {} }
