use core::ptr::{read_volatile, write_volatile};

#[derive(Clone, Copy)]
#[repr(transparent)]
struct Request(u32);

unsafe extern "C" {
    fn xm1_power_record_helper_756734(request: u32, output: *mut u32);
    fn xm1_power_record_helper_756754(request: u32, input: *const u32);
    fn xm1_power_record_helper_756840(request: u32, output: *mut u32);
    fn xm1_power_record_helper_756b9c(request: u32) -> u32;
    fn xm1_power_record_helper_752ca8(request: u32, index: u32, value: i32);
    fn xm1_power_record_helper_752d58(request: u32, index: u32, value: i32);
    fn xm1_power_record_helper_752f68(request: u32, index: u32, value: i32);
    fn xm1_power_record_helper_753178(request: u32, index: u32, value: i32);
    fn xm1_power_record_helper_752a98(request: u32, index: u32, value: u32);
    fn xm1_power_record_helper_752bf8(request: u32, index: u32, value: i32);
    fn xm1_power_record_helper_7536f8(request: u32, index: u32, value: u32);
    fn xm1_power_record_helper_7537a8(request: u32, index: u32, value: i32);
    fn xm1_power_record_helper_753858(request: u32, index: u32, value: u32);
    fn xm1_power_record_helper_753908(request: u32, index: u32, value: u32);
    fn xm1_power_record_helper_f249f0(request: u32, first: *const i16, second: *const i16, third: *const i16, fourth: *const i16, fifth: *const i16);
    fn xm1_power_record_helper_fa6bb4() -> u32;
    fn xm1_power_record_helper_f5f9fc(request: u32) -> u32;
    fn xm1_power_record_helper_fa7634(request: u32) -> u32;
    fn xm1_power_record_helper_757e38(request: u32) -> u32;
    fn xm1_power_record_helper_74f6fc(request: u32) -> u32;
    fn xm1_power_record_helper_74f784(request: u32) -> u32;
    fn xm1_power_record_helper_74f5ec(request: u32) -> u32;
    fn xm1_power_record_helper_f5f368(kind: u32, base: u32, difference: i32, minimum: u32, maximum: u32, limit: i32) -> i32;
    fn xm1_power_record_helper_fa7f40() -> u32;
    fn xm1_power_record_helper_75a464(request: u32, index: u32, input: *const u32);
    fn xm1_power_record_helper_75a4ec(request: u32, output: *mut u32);
    fn xm1_power_record_helper_740e74(address: u32, length: u32);
    fn xm1_power_record_helper_f253a0(request: u32, enabled: u32, mode: u32, value: u32, second: u32, first: u32) -> u32;
    fn xm1_power_record_helper_f2476c(request: u32, enabled: u32, source: u32, output: *mut u32);
    fn xm1_power_record_helper_f2519c();
    fn xm1_power_record_helper_f24b58(request: u32);
}

unsafe fn word(address: usize) -> u32 { unsafe { read_volatile(address as *const u32) } }
unsafe fn signed_half(address: usize) -> i32 { unsafe { read_volatile(address as *const i16) as i32 } }

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f363b8() -> i32 {
    unsafe { word(0x016bfe4c) as i32 }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f363d8(request: u32) {
    unsafe { write_volatile(0x016bfe48 as *mut u32, request) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f363f8(request: u32, enabled: u32, record: *const u32) {
    let request = Request(request);
    let address = record as usize;
    unsafe {
        let first_flag = word(address + 24) & 1;
        let bits = word(address + 28);
        let mut mode = if bits & 2 != 0 { 2 } else { bits & 1 };
        if word(address + 32) & 3 != 0 { mode = 3; }
        let second_flag = word(address + 36) & 1;
        let mut profile = [0u32; 33];
        let difference;
        let disposition;
        if enabled == 1 {
            difference = read_volatile((address + 20) as *const u16) as i32;
            xm1_power_record_helper_756734(request.0, profile.as_mut_ptr());
            write_volatile((profile.as_mut_ptr() as *mut u8).add(102), read_volatile((address + 40) as *const u8));
            xm1_power_record_helper_756754(request.0, profile.as_ptr());
            xm1_power_record_helper_752ca8(request.0, 0, signed_half(address + 10));
            xm1_power_record_helper_752d58(request.0, 0, signed_half(address + 14));
            xm1_power_record_helper_752f68(request.0, 0, signed_half(address + 16));
            xm1_power_record_helper_753178(request.0, 0, signed_half(address + 18));
            xm1_power_record_helper_752a98(request.0, 0, word(address + 4));
            xm1_power_record_helper_752bf8(request.0, 0, signed_half(address + 8));
            xm1_power_record_helper_7536f8(request.0, 0, mode);
            xm1_power_record_helper_7537a8(request.0, 0, signed_half(address + 20));
            xm1_power_record_helper_753858(request.0, 0, first_flag);
            xm1_power_record_helper_753908(request.0, 0, second_flag);
            xm1_power_record_helper_f249f0(request.0, (address + 8) as *const i16, (address + 10) as *const i16, (address + 14) as *const i16, (address + 16) as *const i16, (address + 18) as *const i16);
            disposition = 1;
        } else {
            let initial = (read_volatile((address + 22) as *const u16) as u32).wrapping_add(xm1_power_record_helper_fa6bb4()) as u16;
            let previous = xm1_power_record_helper_f5f9fc(request.0);
            let kind = xm1_power_record_helper_fa7634(request.0);
            let base = xm1_power_record_helper_757e38(request.0);
            let minimum = xm1_power_record_helper_74f6fc(request.0);
            let maximum = xm1_power_record_helper_74f784(request.0);
            difference = xm1_power_record_helper_f5f368(kind, base, (initial as u32).wrapping_sub(previous) as i16 as i32, minimum, maximum, signed_half(address + 12));
            xm1_power_record_helper_756734(request.0, profile.as_mut_ptr());
            write_volatile((profile.as_mut_ptr() as *mut u8).add(102), read_volatile((address + 40) as *const u8));
            xm1_power_record_helper_756754(request.0, profile.as_ptr());
            disposition = xm1_power_record_helper_fa7f40();
            if disposition == 1 {
                let pair = [word(address), word(address + 4)];
                xm1_power_record_helper_75a464(request.0, 1, pair.as_ptr());
            }
        }
        let callback = xm1_power_record_helper_756b9c(request.0);
        if callback != 0 {
            let callback: unsafe extern "C" fn(u32, i32, u32, i32, u32, u32, u32, u32, i32, i32, i32, i32, u32) = core::mem::transmute(callback as usize);
            callback(mode, signed_half(address + 8), word(address + 4), difference as i16 as i32, first_flag, second_flag, 0, disposition, signed_half(address + 10), signed_half(address + 14), signed_half(address + 16), signed_half(address + 18), word(address));
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f36658() {
    unsafe {
        let source = word(0x0169d384);
        if source == 0 { return; }
        xm1_power_record_helper_740e74(source, 0x114);
        let mut record = [0u32; 11];
        xm1_power_record_helper_75a4ec(word(0x016bfe48), record.as_mut_ptr());
        if xm1_power_record_helper_fa7f40() == 1 {
            let enabled = read_volatile((record.as_ptr() as *const u8).add(41)) as u32;
            if xm1_power_record_helper_f253a0(word(0x016bfe48), enabled, word(source as usize + 0x88), word(source as usize + 0xfc), record[1], record[0]) == 1 {
                xm1_power_record_helper_f2476c(word(0x016bfe48), 1, source, record.as_mut_ptr());
                let pair = [record[0], record[1]];
                xm1_power_record_helper_75a464(word(0x016bfe48), 4, pair.as_ptr());
            }
            xm1_power_record_helper_f2519c();
        } else {
            xm1_power_record_helper_f24b58(word(0x016bfe48));
            xm1_power_record_helper_f2476c(word(0x016bfe48), 1, source, record.as_mut_ptr());
        }
        xm1_power_record_helper_f363f8(word(0x016bfe48), 0, record.as_ptr());
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f36760(request: u32) -> i32 {
    unsafe {
        let mut record = [0u32; 12];
        xm1_power_record_helper_756840(request, record.as_mut_ptr());
        let current = xm1_power_record_helper_74f5ec(request);
        let offset = read_volatile((record.as_ptr() as *const u8).add(40) as *const u16) as u32;
        (read_volatile(0x016b3d38 as *const u16) as u32).wrapping_sub(current.wrapping_sub(offset)) as i16 as i32
    }
}
