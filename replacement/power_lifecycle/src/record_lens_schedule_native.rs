//! Task 50 scheduling ABI for recording consumers.
#![cfg(target_arch = "arm")]

#[derive(Clone, Copy)]
#[repr(u32)]
enum Request {
    Initialize = 0x4202,
    Reset = 0x4203,
    Zoom = 0x4204,
    Focus = 0x4205,
    Iris = 0x4206,
    Combined = 0x4207,
    Finish = 0x4208,
    Stop = 0x4209,
    FocusDelta = 0x420b,
    ZoomParameters = 0x4211,
    IrisParameters = 0x4213,
    Query = 0x4217,
    Configure = 0x4214,
    IrisSpecial = 0x421d,
    Adjust = 0x421e,
    Select = 0x4220,
}

extern "C" {
    // The task 50 queue copies the payload before returning.
    fn xm1_power_task50_post(opcode: u32, payload: *const u32, words: u32) -> u32;
    fn xm1_power_record_helper_76d9a8() -> u32;
    fn xm1_power_record_helper_742054() -> u32;
    fn xm1_power_configuration_byte(offset: u32) -> u32;
    fn xm1_power_record_helper_76d9c8() -> u32;
    fn xm1_power_record_helper_747b50() -> u32;
    fn xm1_power_record_helper_768b20(first: *mut u32, second: *mut u16);
}

#[no_mangle]
pub extern "C" fn xm1_power_record_helper_742064() -> u32 {
    if unsafe { xm1_power_record_helper_742054() } == 0 { 0xff }
    else { unsafe { xm1_power_configuration_byte(0x80) } }
}

fn post<const N: usize>(request: Request, payload: [u32; N]) -> u32 {
    if unsafe { xm1_power_task50_post(request as u32, payload.as_ptr(), N as u32) } == 0 { 0 } else { 0x84 }
}

macro_rules! entry {
    ($name:ident, $request:ident, $($argument:ident),+) => {
        #[no_mangle]
        pub extern "C" fn $name($($argument: u32),+) -> u32 {
            post(Request::$request, [$($argument),+])
        }
    };
}

entry!(xm1_power_record_helper_76dbe4, Initialize, callback, first, second, third, fourth, fifth, sixth);
entry!(xm1_power_record_helper_76dc90, Reset, callback, first, second, third);
entry!(xm1_power_record_helper_76dd24, Zoom, callback, first, second);
entry!(xm1_power_record_helper_76ddb0, Focus, callback, first, second);
entry!(xm1_power_record_helper_76de3c, Iris, callback, first, second);
entry!(xm1_power_record_helper_76df7c, Finish, callback, first, second);
entry!(xm1_power_record_helper_76ffec, ZoomParameters, callback, first, second, third);
entry!(xm1_power_record_helper_76f394, Select, selection, value, enabled);
entry!(xm1_power_record_helper_76f5dc, Configure, callback, enabled);
entry!(xm1_power_record_helper_76ff38, Adjust, value);

#[no_mangle]
pub extern "C" fn xm1_power_record_helper_76e008() -> u32 { post(Request::Stop, []) }

#[no_mangle]
pub extern "C" fn xm1_power_record_helper_76dec8(callback: u32, first: u32, second: u32, third: u32, fourth: u32, fifth: u32) -> u32 {
    if unsafe { xm1_power_record_helper_76d9a8() } == 1 { return 0x81; }
    post(Request::Combined, [callback, first, second, third, fourth, fifth])
}

#[no_mangle]
pub extern "C" fn xm1_power_record_helper_76f660(callback: u32, argument: u8) {
    let _ = post(Request::Query, [callback, u32::from(argument)]);
}

#[no_mangle]
pub extern "C" fn xm1_power_record_helper_76e1ac(callback: u32, delta: i32, direction: u32, first: u32, second: u32, third: u32, fourth: u32) -> u32 {
    let mode = match direction { 0 => 0, 1 => 1, 2 => 2, _ => return 0x81 };
    post(Request::FocusDelta, [callback, u32::from(delta as u16), mode, first, second, third, fourth])
}

#[no_mangle]
pub extern "C" fn xm1_power_record_helper_76e654(callback: u32, value: u32, first: u32, second: u32, third: u32, fourth: u32) -> u32 {
    let special = unsafe { xm1_power_record_helper_742054() } != 0
        && xm1_power_record_helper_742064() == 0x70 && fourth == 10;
    post(if special { Request::IrisSpecial } else { Request::IrisParameters }, [callback, value, first, second, third, fourth])
}

/// # Safety
/// Both output pointers address writable, naturally aligned values.
#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_76fbac(first: *mut u32, second: *mut u16) {
    let value = unsafe { xm1_power_record_helper_76d9c8() };
    unsafe {
        core::ptr::write(first, value);
        core::ptr::write(second, 0);
    }
    let count = unsafe { xm1_power_record_helper_747b50() };
    if (value as i32) < 0 || (count.wrapping_sub(1) as i32) < value as i32 {
        unsafe { core::ptr::write(first, 0); }
    } else {
        unsafe { xm1_power_record_helper_768b20(first, second); }
    }
}
