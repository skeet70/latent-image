#![no_std]

#[path = "src/lib.rs"]
mod media_permit;

#[path = "src/stock.rs"]
mod media_permit_stock;

#[no_mangle]
pub extern "C" fn rb_media_permit_case(mode: u32, arg0: u32, arg1: u32) -> i32 {
    match mode {
        0 => media_permit_stock::resolve_key(arg0),
        1 => media_permit_stock::apply_mirror() as i32,
        2 => media_permit_stock::send_words(arg0, arg1) as i32,
        _ => 3,
    }
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {}
}
