#![no_std]
use core::{arch::asm, panic::PanicInfo, ptr::read_volatile};

extern "C" {
    fn service_72872c(argument: u32) -> u32;
    fn service_6c62a8() -> u32;
    fn service_73aca4(argument: u32) -> u32;
    fn service_ed4acc() -> u32;
    fn service_ed4afc() -> u32;
    fn service_69cf84(argument: u32) -> u32;
    fn service_d986b0() -> u32;
    fn service_d80d44() -> u32;
    fn service_1056c24(argument: u32) -> u32;
    fn service_db01fc(argument: u32) -> u32;
    fn service_ec5350() -> u32;
    fn service_dca604() -> u32;
    fn service_db63e0() -> u32;
    fn service_ed10b4() -> u32;
    fn service_ef8c90(argument: u32) -> u32;
    fn service_ed2b00() -> u32;
    fn service_1055d10() -> u32;
    fn service_6b7df8() -> u32;
    fn service_69f000() -> u32;
}


#[no_mangle]
#[link_section = ".text.image5_init"]
pub unsafe extern "C" fn xm1_image5_init() -> u32 {
    'initialize: {
        service_72872c(0x1010100);
        service_6c62a8();
        service_73aca4(7);
        if service_ed4acc() != 0 { break 'initialize; }
        service_ed4afc();
        service_69cf84(0xed4a8c);
        xm1_image5_hooks();
        service_d986b0();
        if service_d80d44() != 0 { break 'initialize; }
        if service_1056c24(0) != 0 { break 'initialize; }
        service_db01fc(1);
        if service_ec5350() != 0 { break 'initialize; }
        if service_dca604() != 0 { break 'initialize; }
        if service_db63e0() != 0 { break 'initialize; }
        if service_ed10b4() != 0 { break 'initialize; }
        service_ef8c90(0);
        if service_ed2b00() != 0 { break 'initialize; }
        if service_1055d10() != 0 { break 'initialize; }
        if service_6b7df8() != 0 { break 'initialize; }
        if service_69f000() != 0 { break 'initialize; }
        let mut result = service_72872c(0x11010100);
        asm!("", inout("r0") result, options(nomem, nostack, preserves_flags));
        return result;
    }
    loop {}
}

#[no_mangle]
#[inline(never)]
pub unsafe extern "C" fn xm1_image5_hooks() {
    let mut cursor = 0xbf1a2c as *const u32;
    loop {
        asm!("/* {0} */", inout(reg) cursor, options(nomem, nostack, preserves_flags));
        let entry = read_volatile(cursor);
        cursor = cursor.sub(1);
        asm!("/* {0} */", inout(reg) cursor, options(nomem, nostack, preserves_flags));
        if entry == u32::MAX { return; }
        let hook: unsafe extern "C" fn() = core::mem::transmute(entry);
        hook();
    }
}

#[panic_handler]
fn panic(_: &PanicInfo) -> ! { loop {} }
