use core::ptr::{read_volatile, write_volatile};

const SCAN: usize = 0x016b66ec;
const CONTROL: usize = 0x016b66d4;
const FRAME: usize = 0x016b665c;

unsafe extern "C" {
    fn xm1_power_record_helper_69f8fc(key: u32) -> u32;
    fn xm1_power_record_helper_69fec8(key: u32) -> u32;
    fn xm1_power_record_helper_f47330(request: u32) -> u32;
    fn xm1_power_record_helper_74ec30(request: u32) -> u32;
    fn xm1_power_record_helper_74e5c0(request: u32) -> u32;
    fn xm1_power_record_helper_74fa2c(request: u32) -> u32;
    fn xm1_power_record_helper_f5206c(output: *mut u32, request: u32);
    fn xm1_power_record_helper_74e648(request: u32) -> u32;
    fn xm1_power_record_helper_758c98(output: *mut u32, request: u32);
    fn xm1_power_record_helper_75702c(request: u32) -> u32;
    fn xm1_power_record_helper_75707c(request: u32) -> u32;
    fn xm1_power_record_helper_756fe4(request: u32) -> u32;
    fn xm1_power_record_helper_76d9c8() -> u32;
    fn xm1_power_record_helper_746bf8(value: u32, base: u32) -> u32;
    fn xm1_power_record_helper_772f28(index: u32) -> i32;
    fn xm1_power_record_helper_772ff8(value: i32) -> u32;
    fn xm1_power_record_helper_757254(request: u32) -> u32;
    fn xm1_power_record_helper_76da84() -> u32;
    fn xm1_power_record_helper_756940(request: u32, output: *mut u32);
    fn xm1_power_record_helper_f2580c(request: u32);
    fn xm1_power_record_helper_74677c() -> u32;
    fn xm1_power_record_helper_7469a0(value: u32) -> u32;
    fn xm1_power_record_helper_7257a8(output: *mut u32);
    fn xm1_power_record_helper_7567ec(request: u32, output: *mut u32);
    fn xm1_power_record_helper_756b54(request: u32) -> u32;
    fn xm1_power_record_caf_retention_log(state: u32, text: *const u8, selected: *const u8, first_sign: u32, first: u32, second_sign: u32, second: u32);
}

unsafe fn word(offset: usize) -> u32 { unsafe { read_volatile((SCAN + offset) as *const u32) } }
unsafe fn half(offset: usize) -> i16 { unsafe { read_volatile((SCAN + offset) as *const i16) } }
unsafe fn byte(offset: usize) -> u8 { unsafe { read_volatile((SCAN + offset) as *const u8) } }
unsafe fn put(offset: usize, value: u32) { unsafe { write_volatile((SCAN + offset) as *mut u32, value) } }
unsafe fn put_half(offset: usize, value: i32) { unsafe { write_volatile((SCAN + offset) as *mut i16, value as i16) } }
unsafe fn put_byte(offset: usize, value: u8) { unsafe { write_volatile((SCAN + offset) as *mut u8, value) } }
unsafe fn read(address: usize) -> u32 { unsafe { read_volatile(address as *const u32) } }
unsafe fn write(address: usize, value: u32) { unsafe { write_volatile(address as *mut u32, value) } }
unsafe fn clear(address: usize, length: usize) { unsafe { for offset in 0..length { write_volatile((address + offset) as *mut u8, 0); } } }

unsafe fn initialize_history() {
    unsafe {
        if xm1_power_record_helper_69f8fc(0x24c) & 2 == 0 { return; }
        write(0x016b66e8, 0);
        for index in 0..301 {
            if xm1_power_record_helper_69f8fc(0x24c) & 2 == 0 { continue; }
            let address = 0x5ffd3584 + index * 0x58;
            clear(address, 0x58);
            for offset in [2, 4, 6, 12, 14] { write_volatile((address + offset) as *mut u16, 0x7fff); }
            write(address + 8, 0x7fff);
            write(address + 20, 0x7fff);
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f26a60(request: u32) -> i32 {
    unsafe {
        if xm1_power_record_helper_f47330(request) == 0 { return 1; }
        clear(CONTROL, 20);
        clear(SCAN, 0xe20);
        clear(FRAME, 0x78);
        initialize_history();
        write(FRAME + 4, xm1_power_record_helper_74ec30(request));
        write(FRAME + 16, xm1_power_record_helper_74e5c0(request));
        write(FRAME + 20, xm1_power_record_helper_74fa2c(request));
        let mut ratio = [0; 2];
        xm1_power_record_helper_f5206c(ratio.as_mut_ptr(), request);
        write(FRAME + 24, ratio[0]);
        write(FRAME + 28, ratio[1]);
        write(FRAME + 76, xm1_power_record_helper_74e648(request));
        let mut active = [0; 2];
        xm1_power_record_helper_758c98(active.as_mut_ptr(), request);
        write(FRAME + 80, active[0]);
        let reference = xm1_power_record_helper_75702c(request);
        let previous = xm1_power_record_helper_75707c(request);
        let value = xm1_power_record_helper_756fe4(request);
        let base = xm1_power_record_helper_76d9c8();
        let mapped = xm1_power_record_helper_746bf8(value, base);
        let correction = xm1_power_record_helper_772f28(3);
        let bucket = xm1_power_record_helper_772ff8(correction);
        let delta = previous.wrapping_sub(xm1_power_record_helper_772f28(bucket) as u32) as u16;
        write_volatile((FRAME + 36) as *mut u16, delta);
        let adjusted = mapped.wrapping_add(reference).wrapping_sub(delta as u32) as u16;
        write_volatile((FRAME + 32) as *mut u16, adjusted);
        write_volatile((FRAME + 34) as *mut u16, adjusted);
        put(0x78, xm1_power_record_helper_757254(request));
        put(0x7c, value);
        put_half(0x80, reference as i32);
        put(0x88, bucket);
        put_half(0x82, previous as i32);
        put_half(0x84, delta as i32);
        put(0xe1c, word(0x78));
        let position = xm1_power_record_helper_76da84() as i32;
        for offset in [12, 0xd68, 0x2a, 0x2c, 0x30] { put_half(offset, position); }
        put_byte(0x29, 1);
        put_byte(0x28, 1);
        for offset in [0x3c, 0x40, 0x44, 0x48] { put(offset, 2); }
        put(0, 0);
        let mut saved = [0; 7];
        xm1_power_record_helper_756940(request, saved.as_mut_ptr());
        put(0x14, 0);
        xm1_power_record_helper_f2580c(request);
        let limit = read_volatile((saved.as_ptr() as usize + 8) as *const i16);
        put(0x14, (limit > half(0x1e)) as u32);
        let state = if saved[1] > 1 { 13 } else { 16 };
        put(8, state);
        state as i32
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f264e8(request: u32) {
    unsafe {
        if xm1_power_record_helper_69f8fc(0x24c) & 2 == 0 { return; }
        let index = read(0x016b66e8) as usize;
        let destination = 0x5ffd3584 + index * 0x58;
        write_volatile(destination as *mut u16, word(8) as u16);
        write_volatile((destination + 2) as *mut u16, read_volatile((FRAME + 32) as *const u16));
        let source = word(0x7c);
        let divisor = xm1_power_record_helper_74677c();
        let remainder = ((source as i32) % (divisor as i32)) as u32;
        write_volatile((destination + 4) as *mut u16, xm1_power_record_helper_7469a0(remainder) as u16);
        write_volatile((destination + 6) as *mut u16, half(0x80) as u16);
        write(destination + 8, half(0x82) as i32 as u32);
        write_volatile((destination + 12) as *mut u16, xm1_power_record_helper_76da84() as u16);
        write_volatile((destination + 14) as *mut u16, half(0xd86) as u16);
        write_volatile((destination + 16) as *mut u16, half(0xde0) as u16);
        write_volatile((destination + 18) as *mut u16, word(0xdd8) as u16);
        write(destination + 20, half(12) as i32 as u32);
        for (offset, source) in [(24, 0x54), (28, 0x58), (32, 0x4c), (36, 0x50)] { write(destination + offset, word(source)); }
        for (offset, source) in [(40, 0xe00), (42, 0xe08), (44, 0xe10), (46, 0xdf8)] { write_volatile((destination + offset) as *mut u16, half(source) as u16); }
        write(destination + 48, word(0x34).wrapping_add(1));
        for offset in [52, 56, 60] { write(destination + offset, 0); }
        xm1_power_record_helper_7257a8((destination + 84) as *mut u32);
        match word(8) {
            13 => { for (offset, source) in [(64, 0x98), (68, 0x90), (72, 0x97), (76, 0x96), (80, 0x99)] { write(destination + offset, byte(source) as u32); } }
            14 => {
                let mut history = [0; 3];
                xm1_power_record_helper_7567ec(request, history.as_mut_ptr());
                let record = history[2] as usize + (history[1] as u8 as usize) * 0x228;
                write(destination + 64, read_volatile((record + 0x204) as *const u16) as u32);
                write(destination + 68, read_volatile((record + 0x206) as *const u16) as u32);
                write(destination + 72, byte(0xd58) as u32);
                write(destination + 76, if half(0xee) == 0 { 0 } else if byte(0xee) == 0 { 1 } else if byte(0xef) == 0 { 2 } else { 3 });
                write(destination + 80, u32::MAX);
            }
            16 => {
                write(destination + 64, word(0xa8));
                write(destination + 68, byte(0xa0) as u32);
                write(destination + 72, byte(0xa1) as u32);
                write(destination + 76, u32::MAX);
                write(destination + 80, u32::MAX);
            }
            17 | 18 => { for offset in [64, 68, 72, 76, 80] { write(destination + offset, u32::MAX); } }
            _ => (),
        }
        let next = read(0x016b66e8).wrapping_add(1);
        write(0x016b66e8, if next > 300 { 0 } else { next });
    }
}

unsafe fn retention_status(enabled: bool) {
    unsafe {
        if xm1_power_record_helper_69f8fc(0x887d) == 0 { return; }
        let limit = xm1_power_record_helper_69f8fc(0x887e);
        if !enabled { return; }
        let state = word(8);
        let text = match state {
            13 => 0x00f28e48,
            14 => 0x00f28e50,
            18 => { put_byte(5, 0); 0x00f28e58 }
            _ => { if (byte(5) as u32) < limit { put_byte(5, byte(5).wrapping_add(1)); } 0x00f28e60 }
        };
        let first = read_volatile((CONTROL + 12) as *const i16) as i32;
        let second = half(12) as i32;
        if xm1_power_record_helper_69fec8(0xe8) & 0x80 != 0 {
            xm1_power_record_caf_retention_log(state, text as *const u8, (if (byte(5) as u32) < limit { 0x00eeb2c0usize } else { 0x00f53400usize }) as *const u8, if first < 0 { 45 } else { 32 }, first.unsigned_abs(), if second < 0 { 45 } else { 32 }, second.unsigned_abs());
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f26ff0(state: u32) {
    unsafe {
        if word(8) != state { put_byte(4, 0); }
        put(8, state);
        put(0xddc, word(0xdd8));
        put(0xdf4, word(0xdf0));
        retention_status(true);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f26fbc(request: u32) {
    unsafe {
        put(8, 17);
        xm1_power_record_helper_f264e8(request);
        if xm1_power_record_helper_69f8fc(0x24c) & 2 != 0 {
            let base = xm1_power_record_helper_756b54(request) as usize;
            write(base + 0xb6ce4, 0x53545246);
            write(base + 0xb6ce8, read(0x016b66e8));
            for offset in 0..0x6778 { write_volatile((base + 0xb6cec + offset) as *mut u8, read_volatile((0x5ffd3584 + offset) as *const u8)); }
            write(base + 0xbd464, 0x4c415246);
            if xm1_power_record_helper_69f8fc(0x24c) & 2 != 0 { clear(base + 0xb6cec + read(0x016b66e8) as usize * 0x58, 0x58); }
        }
        retention_status(false);
    }
}
