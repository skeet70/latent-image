#![no_std]

#[path = "src/lib.rs"]
mod media_permit;

#[path = "src/stock.rs"]
mod media_permit_stock;

#[no_mangle]
#[link_section = ".text.rb_media_permit_case"]
pub extern "C" fn rb_media_permit_case(mode: u32, arg0: u32, _arg1: u32) -> i32 {
    match mode {
        0 => media_permit_stock::resolve_key(arg0),
        1 => media_permit_stock::apply_mirror() as i32,
        _ => 2,
    }
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {}
}
