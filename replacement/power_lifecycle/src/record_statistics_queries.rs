use core::ptr::{read_volatile, write_volatile};

#[repr(C)]
#[derive(Clone, Copy)]
pub struct GeometryQuery { pub kind: u32, pub first: u32, pub second: u32, pub coordinates: u32 }
#[repr(C)]
#[derive(Clone, Copy)]
pub struct CaptureWindow { pub first: u32, pub second: u32 }
#[repr(transparent)]
#[derive(Clone, Copy)]
struct Window([u32; 10]);
#[derive(Clone, Copy)]
struct Bank(u32);
#[derive(Clone, Copy)]
struct Mode(u32);

extern "C" {
    fn xm1_power_record_helper_eeda24(output: *mut u32, mode: u32, query: *const u32);
    fn xm1_power_record_helper_eedb98(output: *mut u32, mode: u32, query: *const u32);
    fn xm1_power_record_helper_eebd8c(output: *mut u32, input: *const u32);
    fn xm1_power_record_helper_eec168(mode: u32) -> u32;
    fn xm1_power_record_helper_eec300(mode: u32, relative: *mut u32, absolute: *mut u32);
    fn xm1_power_record_helper_eebe58(mode: u32) -> u32;
    fn xm1_power_record_helper_eec3c8(mode: u32, query: *const u32, window: *const u32, relative: *mut u32, absolute: *mut u32);
    fn xm1_power_record_helper_eebbbc(output: *mut u16, input: *const u32);
    fn xm1_power_record_helper_eebb98(output: *mut u16, input: *const u32);
    fn xm1_power_record_helper_eebc08(bank: u32) -> u32;
    fn xm1_power_record_helper_eec394(bank: u32, request: *const u16, output: *mut u8) -> u32;
    fn xm1_power_record_helper_eec1a0(output: u32, input: *const u8);
    fn xm1_power_record_helper_eebca4(output: u32, window: *const u32, relative: *const u32);
    fn xm1_power_record_helper_eebc30(output: u32, relative: *const u32);
    fn xm1_power_record_helper_eebd30(output: u32, window: *const u32, origin: *const u32);
}

unsafe fn active() -> bool { unsafe { read_volatile(0x4171_7764 as *const u32) != 0 } }
unsafe fn prepare(mode: Mode, query: *const GeometryQuery, extended: bool) -> Window {
    let mut window = Window([0; 10]);
    if extended { unsafe { xm1_power_record_helper_eedb98(window.0.as_mut_ptr(), mode.0, query.cast()); } }
    else { unsafe { xm1_power_record_helper_eeda24(window.0.as_mut_ptr(), mode.0, query.cast()); } }
    window
}
unsafe fn query_records(bank: Bank, mode: Mode, query: *const GeometryQuery, output: u32, flags: *mut u32, extended: bool) -> u32 {
    if !unsafe { active() } { return 0x81; }
    if !matches!(bank.0, 0..=2 | 5..=8) || query.is_null() || output == 0 { return 0x84; }
    let window = unsafe { prepare(mode, query, extended) };
    let mut absolute = [window.0[2], window.0[3], window.0[4], window.0[5]];
    let mut relative = [0; 4];
    unsafe { xm1_power_record_helper_eebd8c(relative.as_mut_ptr(), absolute.as_ptr()); }
    if unsafe { xm1_power_record_helper_eec168(mode.0) } == 1 {
        unsafe { xm1_power_record_helper_eec300(mode.0, relative.as_mut_ptr(), absolute.as_mut_ptr()); }
    }
    if unsafe { xm1_power_record_helper_eebe58(mode.0) } == 1 {
        unsafe { xm1_power_record_helper_eec3c8(mode.0, query.cast(), window.0.as_ptr(), relative.as_mut_ptr(), absolute.as_mut_ptr()); }
    }
    let mut request = [0u16; 12];
    let mut records = [0u8; 884];
    unsafe {
        xm1_power_record_helper_eebbbc(request.as_mut_ptr(), absolute.as_ptr());
        xm1_power_record_helper_eebb98(request.as_mut_ptr().add(4), relative.as_ptr().add(2));
    }
    let selected = unsafe { xm1_power_record_helper_eebc08(bank.0) };
    if unsafe { xm1_power_record_helper_eec394(selected, request.as_ptr(), records.as_mut_ptr()) } != 0 { return 0x81; }
    unsafe { xm1_power_record_helper_eec1a0(output, records.as_ptr()); }
    if unsafe { xm1_power_record_helper_eec168(mode.0) } == 1 {
        unsafe { xm1_power_record_helper_eebca4(output, window.0.as_ptr(), relative.as_ptr()); }
    } else { unsafe { xm1_power_record_helper_eebc30(output, relative.as_ptr()); } }
    unsafe { xm1_power_record_helper_eebd30(output, window.0.as_ptr(), relative.as_ptr().add(2)); }
    if !flags.is_null() {
        for (index, value) in window.0.iter().enumerate() { unsafe { write_volatile(flags.add(index), *value); } }
    }
    0
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_eedf48(bank: u32, mode: u32, query: *const GeometryQuery, output: u32, flags: u32) -> u32 {
    unsafe { query_records(Bank(bank), Mode(mode), query, output, flags as *mut u32, true) }
}
#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_eee214(bank: u32, mode: u32, query: *const GeometryQuery, output: u32, flags: u32) -> u32 {
    unsafe { query_records(Bank(bank), Mode(mode), query, output, flags as *mut u32, false) }
}
#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_eee0f4(query: *const GeometryQuery, dimensions: *const CaptureWindow, output: u32) -> u32 {
    if !unsafe { active() } { return 0x81; }
    if query.is_null() || dimensions.is_null() || output == 0 { return 0x84; }
    let copied = unsafe { read_volatile(query) };
    let window = unsafe { prepare(Mode(1), &copied, false) };
    let mut absolute = [window.0[2], window.0[3], window.0[4], window.0[5]];
    let mut relative = [0; 4];
    unsafe { xm1_power_record_helper_eebd8c(relative.as_mut_ptr(), absolute.as_ptr()); }
    if unsafe { xm1_power_record_helper_eec168(1) } == 1 {
        unsafe { xm1_power_record_helper_eec300(1, relative.as_mut_ptr(), absolute.as_mut_ptr()); }
    }
    let mut request = [0u16; 12];
    let mut records = [0u8; 884];
    unsafe {
        xm1_power_record_helper_eebbbc(request.as_mut_ptr(), absolute.as_ptr());
        xm1_power_record_helper_eebb98(request.as_mut_ptr().add(4), relative.as_ptr().add(2));
        xm1_power_record_helper_eebb98(request.as_mut_ptr().add(10), dimensions.cast());
    }
    let bank = unsafe { xm1_power_record_helper_eebc08(4) };
    if unsafe { xm1_power_record_helper_eec394(bank, request.as_ptr(), records.as_mut_ptr()) } != 0 { return 0x81; }
    unsafe {
        xm1_power_record_helper_eec1a0(output, records.as_ptr());
        xm1_power_record_helper_eebd30(output, window.0.as_ptr(), relative.as_ptr().add(2));
    }
    0
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_eedc84(query: *const GeometryQuery, output: u32) -> u32 {
    if !unsafe { active() } { return 0x81; }
    if query.is_null() || output == 0 { return 0x84; }
    let copied = unsafe { read_volatile(query) };
    let mut window = unsafe { prepare(Mode(1), &copied, true) };
    if window.0[8] != 0 && window.0[9] != 0 {
        if unsafe { xm1_power_record_helper_eec168(1) } == 1 {
            unsafe { xm1_power_record_helper_eec300(1, window.0.as_mut_ptr().add(6), window.0.as_mut_ptr().add(2)); }
        }
        let width = window.0[5].wrapping_mul(640) / window.0[8];
        let height = window.0[4].wrapping_mul(480) / window.0[9];
        window.0[0] = 480;
        window.0[1] = 640;
        window.0[2] = (640 - width.min(640)) >> 1;
        window.0[3] = (480 - height.min(480)) >> 1;
        window.0[4] = width.min(640);
        window.0[5] = height.min(480);
        window.0[6] = 0;
        window.0[7] = 0;
        window.0[8] = 480;
        window.0[9] = 640;
    }
    let absolute = [window.0[2], window.0[3], window.0[4], window.0[5]];
    let mut relative = [0; 4];
    unsafe { xm1_power_record_helper_eebd8c(relative.as_mut_ptr(), absolute.as_ptr()); }
    let mut origin = relative;
    origin[0] = 0;
    origin[1] = 0;
    let mut request = [0u16; 12];
    let mut records = [0u8; 884];
    unsafe {
        xm1_power_record_helper_eebbbc(request.as_mut_ptr(), absolute.as_ptr());
        xm1_power_record_helper_eebb98(request.as_mut_ptr().add(4), origin.as_ptr().add(2));
        xm1_power_record_helper_eebbbc(request.as_mut_ptr().add(6), origin.as_ptr());
    }
    let bank = unsafe { xm1_power_record_helper_eebc08(3) };
    if unsafe { xm1_power_record_helper_eec394(bank, request.as_ptr(), records.as_mut_ptr()) } != 0 { return 0x81; }
    unsafe {
        xm1_power_record_helper_eec1a0(output, records.as_ptr());
        xm1_power_record_helper_eebd30(output, window.0.as_ptr(), origin.as_ptr().add(2));
    }
    0
}
