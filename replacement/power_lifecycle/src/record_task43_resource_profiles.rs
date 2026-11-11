use core::ptr::{read_volatile, write_volatile};

const PRIMARY: usize = 0x016b65d8;
const PRESET: usize = 0x016b664c;

#[derive(Clone, Copy)]
#[repr(transparent)]
struct Request(u32);

unsafe extern "C" {
    fn xm1_power_record_helper_756fe4(request: u32) -> u32;
    fn xm1_power_record_helper_75702c(request: u32) -> u32;
    fn xm1_power_record_helper_75707c(request: u32) -> u32;
    fn xm1_power_record_helper_757254(request: u32) -> u32;
    fn xm1_power_record_helper_75729c(request: u32) -> u32;
    fn xm1_power_record_helper_746bf8(value: u32, base: u32) -> u32;
    fn xm1_power_record_helper_772f28(value: u32) -> u32;
    fn xm1_power_record_helper_ef5310(value: u32) -> u32;
    fn xm1_power_record_helper_75a3c4(request: u32, value: i32);
    fn xm1_power_record_helper_76d9c8() -> u32;
    fn xm1_power_record_helper_75a4ac(request: u32, index: u32, output: *mut u32);
    fn xm1_power_record_helper_75a464(request: u32, index: u32, value: *const u32);
    fn xm1_power_record_helper_74714c(first: u32, second: u32, base: u32) -> u32;
    fn xm1_power_record_helper_746864(value: u32, base: u32) -> u32;
    fn xm1_power_record_helper_74fa2c(request: u32) -> u32;
    fn xm1_power_record_helper_75741c(request: u32, output: *mut u32);
    fn xm1_power_record_helper_758c68(request: u32, output: *mut u32);
    fn xm1_power_record_helper_756840(request: u32, output: *mut u32);
    fn xm1_power_record_helper_756940(request: u32, output: *mut u32);
    fn xm1_power_record_helper_75093c(request: u32) -> u32;
    fn xm1_power_record_helper_76dad4() -> u32;
    fn xm1_power_record_helper_76da84() -> u32;
    fn xm1_power_record_helper_76e1ac(callback: unsafe extern "C" fn(), delta: i32, mode: u32, enabled: u32, zero: u32, kind: u32, count: u32);
    fn xm1_power_record_helper_f47330(request: u32) -> u32;
    fn xm1_power_record_helper_74947c(value: i32, limit: u32, reserved: u32, base: u32, zero: u32, single: u32, mode: u32) -> i32;
    fn xm1_power_record_helper_ef9a94(event: u32, payload: *const u32, length: u32);
    fn xm1_power_record_helper_74e648(request: u32) -> u32;
    fn xm1_power_record_helper_74f784(request: u32) -> u32;
    fn xm1_power_record_helper_752a98(request: u32, index: u32, value: u32);
    fn xm1_power_record_helper_75a4ec(request: u32, output: *mut u32);
    fn xm1_power_record_helper_75a52c(request: u32, value: *const u32);
    fn xm1_power_record_helper_740e74(address: u32, length: u32);
    fn xm1_power_record_helper_f363f8(request: u32, enabled: u32, value: *const u32);
}

unsafe fn word(address: usize) -> u32 { unsafe { read_volatile(address as *const u32) } }
unsafe fn half(address: usize) -> u16 { unsafe { read_volatile(address as *const u16) } }
unsafe fn put(address: usize, value: u32) { unsafe { write_volatile(address as *mut u32, value) } }
unsafe fn put_half(address: usize, value: u16) { unsafe { write_volatile(address as *mut u16, value) } }

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f238e0() -> i32 {
    unsafe { word(0x016b5b14) as i32 }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f2421c(request: u32) {
    let request = Request(request);
    unsafe {
        put_half(PRIMARY + 8, xm1_power_record_helper_75702c(request.0) as u16);
        put(PRIMARY + 4, xm1_power_record_helper_756fe4(request.0));
        put(PRIMARY + 16, xm1_power_record_helper_757254(request.0));
        put_half(PRIMARY + 10, xm1_power_record_helper_75707c(request.0) as u16);
        put_half(PRIMARY + 14, xm1_power_record_helper_75729c(request.0) as u16);
        let mapped = xm1_power_record_helper_746bf8(word(PRIMARY + 4), 0);
        let reference = half(PRIMARY + 8) as u32;
        let offset = xm1_power_record_helper_772f28(3);
        let previous = xm1_power_record_helper_75707c(request.0);
        let value = reference.wrapping_add(mapped).wrapping_add(offset).wrapping_sub(previous) as u16;
        put_half(PRIMARY, value);
        put_half(PRIMARY + 12, xm1_power_record_helper_ef5310(word(PRIMARY + 16)) as u16);
        put(0x016b657c, u32::MAX);
        put(0x016b6580, u32::MAX);
        xm1_power_record_helper_75a3c4(request.0, value as i16 as i32);
        put(0x016b6028, 1);
        put(0x016b602c, 1);
    }
}

unsafe fn adapt_focus(request: Request, value: i16, base: u32, limit: u32, output: &mut i16) -> bool {
    unsafe {
        *output = value;
        let mut status = [0; 7];
        xm1_power_record_helper_758c68(request.0, status.as_mut_ptr());
        if xm1_power_record_helper_f47330(request.0) == 1 && status[0] == 0 { return false; }
        let mut record = [0; 12];
        xm1_power_record_helper_756840(request.0, record.as_mut_ptr());
        if record[8] == 1 {
            let single = xm1_power_record_helper_75093c(request.0) == 1;
            let mode = xm1_power_record_helper_76dad4();
            *output = xm1_power_record_helper_74947c(*output as i32, limit, 0, base, 0, single as u32, mode) as i16;
        }
        let mut saved = [0; 7];
        xm1_power_record_helper_756940(request.0, saved.as_mut_ptr());
        true
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f24170() {
    unsafe { xm1_power_record_helper_ef9a94(0x37, core::ptr::null(), 0); }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f251e4(request: u32) -> i32 {
    let request = Request(request);
    unsafe {
        let base = xm1_power_record_helper_76d9c8();
        let mut selected = [0; 2];
        xm1_power_record_helper_75a4ac(request.0, 2, selected.as_mut_ptr());
        let next = xm1_power_record_helper_74714c(selected[0], selected[1], base);
        let mut previous = [0; 2];
        xm1_power_record_helper_75a4ac(request.0, 3, previous.as_mut_ptr());
        let old = previous[1];
        let first = xm1_power_record_helper_746864(selected[1], selected[0]);
        let second = xm1_power_record_helper_746864(previous[1], previous[0]);
        if first != second { write_volatile(0x016b6584 as *mut u8, 1); }
        previous[0] = base;
        if next == old {
            xm1_power_record_helper_75a464(request.0, 3, previous.as_ptr());
            return 1;
        }
        previous[1] = next;
        xm1_power_record_helper_75a464(request.0, 3, previous.as_ptr());
        let kind = xm1_power_record_helper_74fa2c(request.0);
        let mut focus = [0; 1];
        xm1_power_record_helper_75741c(request.0, focus.as_mut_ptr());
        let mut value = 0;
        if !adapt_focus(request, focus[0] as i16, next, base, &mut value) { return 1; }
        let current = xm1_power_record_helper_76da84();
        let mode = xm1_power_record_helper_76dad4();
        let delta = (value as u16).wrapping_sub(current as u16) as i16 as i32;
        xm1_power_record_helper_76e1ac(xm1_power_record_helper_f24170, delta, mode, 1, 0, if kind == 0 { 11 } else { 0 }, 4);
        2
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f25334(request: u32) {
    unsafe {
        if matches!(xm1_power_record_helper_74e648(request), 0x36 | 0x2e | 0x38 | 0x2f) {
            let value = xm1_power_record_helper_74f784(request);
            xm1_power_record_helper_752a98(request, 0, value);
        }
        write_volatile(0x016b6584 as *mut u8, 0);
    }
}

unsafe fn changed(request: Request, valid: u32, first: [u32; 2], second: [u32; 2]) -> bool {
    unsafe {
        if valid == 0 { return true; }
        let kind = xm1_power_record_helper_74e648(request.0);
        let current = xm1_power_record_helper_746864(first[0], first[1]);
        let previous = xm1_power_record_helper_746864(second[0], second[1]);
        if matches!(kind, 0x36 | 0x2e | 0x38 | 0x2f) && previous != current { return true; }
        if matches!(kind, 0xd | 0x2d | 0x2f | 0x37 | 0x38) && half(PRESET + 12) != half(PRESET + 4) { return true; }
        if !matches!(kind, 0x2f | 0x38) && half(PRESET + 14) != half(PRESET + 6) { return true; }
        let mut selected = [0; 2];
        xm1_power_record_helper_75a4ac(request.0, 2, selected.as_mut_ptr());
        let selected = xm1_power_record_helper_74714c(selected[0], selected[1], first[1]);
        xm1_power_record_helper_746864(selected, first[1]) == current && previous != current
    }
}

unsafe fn publish_focus(request: Request, full: bool, source: usize, output: &mut [u32; 11]) {
    unsafe {
        output[0] = word(source + 0xfc);
        output[1] = word(source + 0x88);
        if full {
            let destination = output.as_mut_ptr() as usize;
            for (to, from) in [(8, 0x8c), (10, 0xec), (12, 0x8e), (14, 0xee), (16, 0xf0), (18, 0xf2), (20, 0xf8), (22, 0x9c)] {
                put_half(destination + to, half(source + from));
            }
            output[6] = word(source + 0x94);
            output[7] = word(source + 0x90);
            output[8] = word(source + 4);
            output[9] = word(source + 0x98);
            write_volatile((destination + 40) as *mut u8, read_volatile((source + 0xe4) as *const u8));
            write_volatile((destination + 41) as *mut u8, read_volatile((source + 0x100) as *const u8));
        }
        xm1_power_record_helper_75a52c(request.0, output.as_ptr());
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f254bc(request: u32) {
    let request = Request(request);
    unsafe {
        let source = word(0x0169d384) as usize;
        if source == 0 { return; }
        xm1_power_record_helper_740e74(source as u32, 0x114);
        let mut output = [0; 11];
        xm1_power_record_helper_75a4ec(request.0, output.as_mut_ptr());
        let valid = read_volatile((output.as_ptr() as usize + 41) as *const u8) as u32;
        let full = changed(request, valid, [word(source + 0x88), word(source + 0xfc)], [output[1], output[0]]);
        publish_focus(request, full, source, &mut output);
        if full { xm1_power_record_helper_75a464(request.0, 4, output.as_ptr()); }
        put(PRESET + 8, word(PRESET));
        put(PRESET + 12, word(PRESET + 4));
        let selected = [word(source + 0xfc), word(source + 0x88)];
        xm1_power_record_helper_75a464(request.0, 1, selected.as_ptr());
        xm1_power_record_helper_f363f8(request.0, 1, output.as_ptr());
    }
}

unsafe extern "C" {
    fn xm1_power_record_helper_f1fbe8(context: u32);
    fn xm1_power_record_helper_f1d134(profile: u32);
    fn xm1_power_record_helper_f1d0f4(request: u32, value: u32);
    fn xm1_power_record_helper_f17af8(value: u32);
    fn xm1_power_record_helper_f1d5d4(a: u32, b: u32, c: u32, d: u32, e: u32, f: u32, g: u32, h: i32);
    fn xm1_power_record_helper_757b98(request: u32, value: u32);
    fn xm1_power_record_helper_757be4(request: u32, value: i32);
    fn xm1_power_record_helper_757c34(request: u32, value: i32);
    fn xm1_power_record_helper_757cd4(request: u32, value: i32);
    fn xm1_power_record_helper_757dc4(request: u32, value: i32);
    fn xm1_power_record_helper_75a414(request: u32, value: i32);
    fn xm1_power_record_helper_75a324(request: u32, value: i32);
    fn xm1_power_record_helper_75a374(request: u32, value: i32);
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f255ac(request: u32) {
    let request = Request(request);
    unsafe {
        put(0x016b602c, 0);
        let source = word(0x0169d384) as usize;
        if source == 0 { return; }
        let context = word(source + 0xc4);
        let mut output = [0; 11];
        xm1_power_record_helper_75a4ec(request.0, output.as_mut_ptr());
        if word(source + 0xd0) != 0 {
            xm1_power_record_helper_f1fbe8(context);
            let valid = read_volatile((output.as_ptr() as usize + 41) as *const u8) as u32;
            if changed(request, valid, [word(source + 0x88), word(source + 0xfc)], [output[1], output[0]]) {
                xm1_power_record_helper_f1d134(word(source + 0xc8));
                xm1_power_record_helper_f1d0f4(request.0, word(source + 0xfc));
                if read_volatile(0x016b6584 as *const u8) == 1 { xm1_power_record_helper_f17af8(1); }
            } else {
                xm1_power_record_helper_f1d5d4(
                    word(source + 0xfc), word(source + 0x108),
                    read_volatile((source + 0x10c) as *const u8) as u32,
                    read_volatile((source + 0x10d) as *const u8) as u32,
                    read_volatile((source + 0x110) as *const u8) as u32,
                    word(source + 0x104), word(source + 0x88), half(source + 0x10e) as i16 as i32,
                );
            }
            let profile = word(source + 0xc8) as usize;
            xm1_power_record_helper_757b98(request.0, word(profile + 8));
            xm1_power_record_helper_757be4(request.0, half(profile + 14) as i16 as i32);
            xm1_power_record_helper_757c34(request.0, half(profile + 62) as i16 as i32);
            xm1_power_record_helper_757cd4(request.0, half(profile + 20) as i16 as i32);
            xm1_power_record_helper_757dc4(request.0, half(profile + 64) as i16 as i32);
            xm1_power_record_helper_75a414(request.0, half(source + 158) as i16 as i32);
            xm1_power_record_helper_75a324(request.0, half(source + 132) as i16 as i32);
            xm1_power_record_helper_75a374(request.0, half(source + 134) as i16 as i32);
        }
        xm1_power_record_helper_740e74(source as u32, 0x114);
        if word(source + 0xd0) != 0 {
            xm1_power_record_helper_740e74(context, 124);
            xm1_power_record_helper_740e74(word(source + 0xc8), 0x63ec);
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f2476c(request: u32, full: u32, source: u32, output: *mut u32) {
    unsafe { publish_focus(Request(request), full == 1, source as usize, &mut *(output as *mut [u32; 11])); }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f2519c() {
    unsafe { put(PRESET + 8, word(PRESET)); put(PRESET + 12, word(PRESET + 4)); }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f253a0(request: u32, valid: u32, first: u32, base: u32, previous: u32, previous_base: u32) -> i32 {
    unsafe { changed(Request(request), valid, [first, base], [previous, previous_base]) as i32 }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f24b58(request: u32) {
    unsafe {
        let output = [0; 11];
        for offset in 0..16 { write_volatile((PRESET + offset) as *mut u8, 0); }
        xm1_power_record_helper_75a52c(request, output.as_ptr());
    }
}

unsafe extern "C" {
    fn xm1_power_record_helper_69f8fc(key: u32) -> u32;
    fn xm1_power_record_helper_750474(request: u32) -> u32;
    fn xm1_power_record_helper_7582c0(output: *mut u32, request: u32);
    fn xm1_power_record_helper_f1c8fc(request: u32, record: *const u32, first: *mut i16, second: *mut i16, output: *mut u32, a: *mut u16, b: *mut u16, c: *mut u16, d: *mut u16);
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f249f0(request: u32, first: *mut i16, a: *mut u16, b: *mut u16, c: *mut u16, d: *mut u16) {
    unsafe {
        if xm1_power_record_helper_69f8fc(0x5257) & 3 == 0 || xm1_power_record_helper_750474(request) != 1 { return; }
        let mut record = [0; 10];
        xm1_power_record_helper_7582c0(record.as_mut_ptr(), request);
        let mut second = 0;
        let mut output = 0;
        xm1_power_record_helper_f1c8fc(request, record.as_ptr(), first, &mut second, &mut output, a, b, c, d);
    }
}
