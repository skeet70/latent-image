use core::ptr::{read_volatile, write_volatile};

const SCAN: usize = 0x016b66ec;
const FRAME: usize = 0x016b665c;
const CONTROL: usize = 0x016b66d4;

unsafe extern "C" {
    fn xm1_power_record_helper_feaacc(channel: u32);
    fn xm1_power_record_helper_feaadc(channel: u32, output: *mut u32);
    fn xm1_power_record_helper_faaae8(request: u32);
    fn xm1_power_record_helper_ef74e4() -> u32;
    fn xm1_power_record_helper_757254(request: u32) -> u32;
    fn xm1_power_record_helper_75a3f0(request: u32) -> i32;
    fn xm1_power_record_helper_69f8fc(key: u32) -> u32;
    fn xm1_power_record_helper_69fec8(key: u32) -> u32;
    fn xm1_power_record_helper_74eb20(request: u32) -> u32;
    fn xm1_power_record_helper_750474(request: u32) -> u32;
    fn xm1_power_record_helper_7567ec(request: u32, output: *mut u32);
    fn xm1_power_record_helper_76f360() -> u32;
    fn xm1_power_record_helper_76dac4() -> u32;
    fn xm1_power_record_helper_faab3c(request: u32, kind: u32, value: u32, measurement: u32, context: u32, clock: u32, output: *mut u32) -> i32;
    fn xm1_power_record_helper_fa7614(request: u32, value: u32) -> u32;
    fn xm1_power_record_helper_f8e9e4(parameters: *const u32) -> i32;
}

unsafe fn word(offset: usize) -> u32 { unsafe { read_volatile((SCAN + offset) as *const u32) } }
unsafe fn half(offset: usize) -> i16 { unsafe { read_volatile((SCAN + offset) as *const i16) } }
unsafe fn byte(offset: usize) -> u8 { unsafe { read_volatile((SCAN + offset) as *const u8) } }
unsafe fn put(offset: usize, value: u32) { unsafe { write_volatile((SCAN + offset) as *mut u32, value) } }
unsafe fn put_half(offset: usize, value: i32) { unsafe { write_volatile((SCAN + offset) as *mut i16, value as i16) } }
unsafe fn put_byte(offset: usize, value: u8) { unsafe { write_volatile((SCAN + offset) as *mut u8, value) } }
unsafe fn read(address: usize) -> u32 { unsafe { read_volatile(address as *const u32) } }
unsafe fn write(address: usize, value: u32) { unsafe { write_volatile(address as *mut u32, value) } }

#[cfg(target_arch = "arm")]
core::arch::global_asm!(
    ".arm",
    ".global xm1_power_record_helper_f2711c",
    ".type xm1_power_record_helper_f2711c,%function",
    "xm1_power_record_helper_f2711c:",
    "push {r4,r5,r6,r11,lr}",
    "add r11,sp,#16",
    "sub sp,sp,#100",
    "sub r1,r11,#116",
    "bl xm1_power_record_caf_frame",
    "sub sp,r11,#16",
    "pop {r4,r5,r6,r11,pc}",
    options(raw),
);

unsafe fn threshold_met(request: u32) -> bool {
    unsafe {
        if xm1_power_record_helper_69f8fc(0x8803) & 1 == 0 { return false; }
        let mut history = [0; 3];
        xm1_power_record_helper_7567ec(request, history.as_mut_ptr());
        if history[0] == 0 { return false; }
        let record = history[2] as usize + (history[1] as u8 as usize) * 0x228;
        if read(record) == 0 { return false; }
        let threshold = xm1_power_record_helper_69fec8(0x944f);
        read_volatile((record + 0x204) as *const u16) as u32 >= threshold || read_volatile((record + 0x206) as *const u16) as u32 >= threshold
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_caf_frame(request: u32, source: usize) {
    unsafe {
        xm1_power_record_helper_feaacc(1);
        xm1_power_record_helper_feaadc(1, source as *mut u32);
        write(FRAME + 0x44, read(source + 68));
        write(FRAME + 0x28, read(source + 52));
        write(FRAME + 0x2c, read(source + 56));
        write(FRAME + 0x38, 0x5ffebe8c);
        write(FRAME + 0x3c, 0x5ffec08c);
        write(FRAME + 0x40, 0x5ffee518);
        let offset = if read(source + 68) == 1 { 60 } else { 44 };
        write(FRAME + 0x30, read(source + offset));
        write(FRAME + 0x34, read(source + offset + 4));
        write(FRAME + 0x5c, read(source));
        write(FRAME + 0x60, read(source + 4));
        write(FRAME + 0x64, 0x1000);
        write(FRAME + 0x58, 0x5ffec1b4);
        xm1_power_record_helper_faaae8(request);
        write_volatile((FRAME + 0x6e) as *mut u16, xm1_power_record_helper_ef74e4() as u16);
        write(FRAME + 0x68, xm1_power_record_helper_757254(request));
        write(FRAME + 0x70, 0);
        write_volatile((FRAME + 0x74) as *mut u16, read_volatile((FRAME + 0x22) as *const u16));
        write(FRAME + 8, 1);
        let value = xm1_power_record_helper_75a3f0(request);
        write_volatile((FRAME + 0x20) as *mut i16, value as i16);
        put_half(0x6e, value);
        write(FRAME + 12, 0);
        let previous = read_volatile((FRAME + 0x22) as *const i16) as i32;
        if value < previous {
            let configured = xm1_power_record_helper_69f8fc(0x8889).wrapping_mul(5);
            let signed = ((configured << 17) as i32) >> 17;
            if previous - (read_volatile((FRAME + 0x20) as *const i16) as i32) > signed * 2 { write(FRAME + 12, 1); }
        }
        put(0xdd0, xm1_power_record_helper_74eb20(request));
        put(0xdd4, read(source + 72));
        put_half(0xde0, read_volatile((source + 76) as *const u16) as i32);
        put(0xdd8, read(FRAME + 0x44));
        put(0x34, read(source + 40));
        put(0xde8, xm1_power_record_helper_750474(request));
        put(0xdec, read(source + 80));
        put(0xdf0, read(source + 84));
        if word(8) == 14 {
            if byte(4) == 0 && word(0xdd8) == 0 {
                if read(FRAME + 0x48) == 0 { write(FRAME + 0x48, threshold_met(request) as u32); }
            } else if word(0xdd8) == 1 { write(FRAME + 0x48, 0); }
        } else { write(FRAME + 0x48, 0); }
        let phase = read(FRAME + 0x48);
        write(FRAME + 0x54, if word(8) == 14 { if phase == 0 { 1 } else { 2 } } else { 0 });
        put_byte(0xd58, phase as u8);
    }
}

unsafe fn reset_extrema() {
    unsafe {
        for offset in [0xdfc, 0xe04, 0xe0c, 0xe14] { put(offset, u32::MAX); put_half(offset + 4, 0x7fff); }
    }
}

unsafe fn begin_iteration() {
    unsafe {
        if byte(4) == 1 {
            for offset in [0xa0, 0xa1, 0xa2, 0xa3, 0x90, 0x91, 0x95, 0x97, 0x98, 0x93, 0x92, 0x94, 5] { put_byte(offset, 0); }
            for offset in [0xa4, 0xa8, 0xb0, 0xb4] { put(offset, 0); }
            put_half(0xb8, 0);
            put_half(0xba, 0);
            put(0xbc, 2);
            write(CONTROL + 8, 0);
            put(0xac, word(0xdc8));
            reset_extrema();
        }
        put_byte(0x99, 0);
        put(0xa4, word(0xdc8));
        put_half(0xb8, half(0x2e) as i32);
        put(0xbc, word(0x44));
    }
}

unsafe fn choose_direction() {
    unsafe {
        if half(0x2a) <= half(0x1c) { put_byte(0x28, 1); }
        else if half(0x2a) >= half(0x1e) { put_byte(0x28, 255); }
    }
}

unsafe fn end_iteration() {
    unsafe { put(0xb4, word(0xb0)); put(0xa8, word(0xa4)); put_half(0xba, half(0xb8) as i32); }
}

unsafe fn restart_scan(mask: u32) -> i32 {
    unsafe {
        put_byte(0xf1, 0);
        put_byte(0xf0, 0);
        put_half(0xec, half(0x2a) as i32);
        put_byte(4, 0);
        put_byte(0x28, if xm1_power_record_helper_69f8fc(0x880e) & mask == 0 { 255 } else { 1 });
        put_half(0x26, 0);
        14
    }
}

unsafe fn search_counters() -> i32 {
    unsafe {
        if byte(0x91) as u32 >= xm1_power_record_helper_69f8fc(0x8811) {
            let state = restart_scan(2);
            put_byte(0x99, 1);
            return state;
        }
        if (byte(0x90) as u32) < xm1_power_record_helper_69f8fc(0x8812) {
            put_byte(0x28, (byte(0x29) as i8).wrapping_neg() as u8);
            put_byte(0x99, 4);
            return 13;
        }
        let state = restart_scan(2);
        if xm1_power_record_helper_69f8fc(0x8896) == 1 && word(0xd98) == 1 {
            put_byte(0x28, 1);
            put_byte(0x99, 2);
        } else { put_byte(0x28, 255); put_byte(0x99, 3); }
        state
    }
}

unsafe fn update_direction_counts() {
    unsafe {
        match byte(0x28) as i8 {
            1 => {
                if word(0xa4) < word(0xa8) { put_byte(0x93, 0); put_byte(0x92, byte(0x92).wrapping_add(1)); }
                else { put_byte(0x93, if word(0xa4) == 0 { 0 } else { byte(0x93).wrapping_add(1) }); put_byte(0x92, 0); }
            }
            -1 => {
                if word(0xa4) < word(0xa8) { put_byte(0x93, byte(0x93).wrapping_add(1)); put_byte(0x92, 0); }
                else { put_byte(0x92, if word(0xa4) == 0 { 0 } else { byte(0x92).wrapping_add(1) }); put_byte(0x93, 0); }
            }
            _ => { put_byte(0x93, 0); put_byte(0x92, 0); }
        }
    }
}

unsafe fn reverse_from_counts() {
    unsafe { if byte(0x93) != 0 { put_byte(0x28, 1); } else if byte(0x92) != 0 { put_byte(0x28, 255); } }
}

unsafe fn reset_search_counters() {
    unsafe { for offset in [0x93, 0x92, 4, 0x90, 0x94] { put_byte(offset, 0); } }
}

unsafe fn relative_search() -> i32 {
    unsafe {
        put_byte(0x99, 4);
        if byte(0x90) as u32 >= xm1_power_record_helper_69f8fc(0x8812) {
            reverse_from_counts();
            let state = if (byte(0x94) as u32) < xm1_power_record_helper_69f8fc(0x881e) && xm1_power_record_helper_69f8fc(0x8819) != 0 {
                put(0xd64, 0);
                16
            } else {
                put_byte(0xf1, 0); put_byte(0xf0, 0); put_half(0xec, half(0x2a) as i32); put_half(0x26, 0);
                put_byte(0x99, 1); put_byte(0x95, 0); put_byte(0x91, 0);
                14
            };
            reset_search_counters();
            return state;
        }
        if (byte(0x93) as u32) < xm1_power_record_helper_69f8fc(0x8811) && (byte(0x92) as u32) < xm1_power_record_helper_69f8fc(0x8811) {
            put_byte(0x28, if half(0x9e) < half(0x9c) { 1 } else { 255 });
            if xm1_power_record_helper_69f8fc(0x8819) == 1 {
                if byte(0x93) as u32 >= xm1_power_record_helper_69f8fc(0x8810) { put_byte(0x28, 1); }
                else if byte(0x92) as u32 >= xm1_power_record_helper_69f8fc(0x8810) { put_byte(0x28, 255); }
            }
            let first = (word(0xb0) as i32).unsigned_abs();
            if first < xm1_power_record_helper_69fec8(0x8885).wrapping_mul(10) {
                let second = (word(0xb4) as i32).unsigned_abs();
                if second < xm1_power_record_helper_69fec8(0x8885).wrapping_mul(10) { put_byte(0x94, byte(0x94).wrapping_add(1)); return 13; }
            }
            put_byte(0x94, 0);
            return 13;
        }
        reverse_from_counts();
        let first = (word(0xb0) as i32).unsigned_abs();
        if first >= xm1_power_record_helper_69fec8(0x8885).wrapping_mul(10) {
            let second = (word(0xb4) as i32).unsigned_abs();
            if second >= xm1_power_record_helper_69fec8(0x8885).wrapping_mul(10) && xm1_power_record_helper_69f8fc(0x8819) == 1 { reset_search_counters(); put(0xd64, 0); return 16; }
        }
        put_byte(0xf1, 0); put_byte(0xf0, 0); put_half(0xec, half(0x2a) as i32); put_half(0x26, 0);
        put_byte(0x99, 1); put_byte(0x95, 0); put_byte(0x91, 0);
        reset_search_counters();
        14
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f2751c(request: u32) -> i32 {
    unsafe {
        put(0x38, 2);
        put_byte(0xd5f, 0);
        begin_iteration();
        let clock = xm1_power_record_helper_76f360();
        let step = xm1_power_record_helper_faab3c(request, 5, word(0x7c), word(0xdc8), word(0xe1c), clock, (SCAN + 0x10) as *mut u32) as i16;
        put_half(0x26, step as i32);
        if xm1_power_record_helper_69f8fc(0x87f3) as i32 <= step as i32 { put_half(0x26, xm1_power_record_helper_69f8fc(0x87f3) as i32); }
        let measurement = word(0xd6c);
        put_half(0x9c, half(0x9e) as i32);
        put_half(0x9e, half(0x30) as i32);
        let state = if read(CONTROL + 8) != 0 {
            put_byte(0x28, byte(0x29));
            13
        } else {
            if byte(4) == 1 {
                if (half(0x1e) as i32) - (half(0x26) as i32) < half(0x2a) as i32 { put_byte(0x28, 255); }
                else if (half(0x2a) as i32) < (half(0x26) as i32) + (half(0x1c) as i32) { put_byte(0x28, 1); }
            }
            if word(0x18) != 0 { put_half(0x26, 0); 13 }
            else if byte(0xd5f) == 0 && measurement > xm1_power_record_helper_69f8fc(0x8842).wrapping_mul(10) && xm1_power_record_helper_69f8fc(0x8802) == 1 { put_byte(4, 0); 16 }
            else if xm1_power_record_helper_69f8fc(0x8818) == 1 {
                if byte(0x90) == 0 { put_byte(0x93, 0); put_byte(0x92, 0); put_byte(0x90, 1); }
                else { let count = byte(0x90); put_byte(0x90, if count == 254 { 255 } else { count.wrapping_add(1) }); }
                if half(0x9c) == half(0x9e) { 13 }
                else { put(0xb0, word(0xa4).wrapping_sub(word(0xa8))); update_direction_counts(); let next = relative_search(); put(0xac, word(0xa4)); next }
            } else if (word(0x74) == 1 || byte(0x98) == 1) && xm1_power_record_helper_76dac4() == 1 {
                put_byte(0x98, 1);
                if byte(0x96) == 0 && xm1_power_record_helper_69f8fc(0x880f) == 0 { restart_scan(8) }
                else {
                    put_byte(0x90, byte(0x90).wrapping_add(1));
                    if word(0x74) == 1 {
                        if (byte(0x97) as u32) >= xm1_power_record_helper_69f8fc(0x8816) { restart_scan(4) }
                        else {
                            put_byte(0x97, 0);
                            if word(0xa4) < xm1_power_record_helper_69f8fc(0x8814).wrapping_mul(10) { restart_scan(1) }
                            else {
                                if word(0xa4) < word(0xa8) { put_byte(0x95, 0); }
                                else { put_byte(0x91, if byte(0x95) == 0 { byte(0x91).wrapping_add(1) } else { 0 }); put_byte(0x95, 1); }
                                search_counters()
                            }
                        }
                    } else {
                        let count = byte(0x97).wrapping_add(1);
                        put_byte(0x97, if count == 255 { 254 } else { count });
                        if (byte(0x97) as u32) < xm1_power_record_helper_69f8fc(0x8816) { search_counters() }
                        else { put_half(0x26, 0); 13 }
                    }
                }
            } else { put_half(0x26, 0); 13 }
        };
        choose_direction();
        end_iteration();
        state
    }
}

unsafe fn track_direction() {
    unsafe {
        if byte(4) < 4 || half(0xb8) == half(0xba) { put_byte(0x28, byte(0x29)); return; }
        let special = (xm1_power_record_helper_69f8fc(0x8849) > 2 || xm1_power_record_helper_69f8fc(0x8849) == 1)
            && word(0xdd8) == 1 && word(0xa4) < xm1_power_record_helper_69fec8(0x884c).wrapping_mul(100);
        let previous = word(0xa8);
        let current = word(0xa4);
        let percent = xm1_power_record_helper_69f8fc(if special { 0x884b } else { 0x884a });
        let rising = previous <= current.wrapping_add(previous.wrapping_mul(percent) / 100);
        if rising {
            if !special || word(0xa8) <= word(0xa4) {
                put_byte(0xa3, 1);
                put_byte(0xa0, byte(0xa0).wrapping_add(1));
            }
            put_byte(0x28, if half(0xba) < half(0xb8) { 1 } else { 255 });
        } else {
            if byte(0xa3) != 0 { put_byte(0xa3, 0); put_byte(0xa1, byte(0xa1).wrapping_add(1)); }
            put_byte(0xa0, 0);
            put_byte(0x28, if half(0xba) < half(0xb8) { 255 } else { 1 });
        }
    }
}

unsafe fn record_peak(value: u32, position: i16) {
    unsafe { put(0xe14, value); put_half(0xe18, position as i32); }
}

unsafe fn rotate_peak(value: u32, position: i16) {
    unsafe {
        put(0xdfc, word(0xe14));
        put_half(0xe00, half(0xe18) as i32);
        put(0xe04, value);
        put_half(0xe08, position as i32);
        put(0xe0c, u32::MAX);
        put_half(0xe10, 0x7fff);
    }
}

unsafe fn complete_peak(value: u32, position: i16) -> bool {
    unsafe {
        if word(0xe0c) != u32::MAX || word(0xe04) == u32::MAX { return false; }
        put(0xe0c, value);
        put_half(0xe10, position as i32);
        if xm1_power_record_helper_69f8fc(0x87ff) == 0 || word(0xdfc) == u32::MAX || word(0xe0c) == u32::MAX { return false; }
        let middle = word(0xe04) as i32;
        let first = word(0xdfc) as i32;
        let last = word(0xe0c) as i32;
        if middle < first { if last <= middle { return false; } }
        else if middle <= first || middle <= last { return false; }
        let first = half(0xe00);
        let middle = half(0xe08);
        let last = half(0xe10);
        if first == 0x7fff || middle == 0x7fff || last == 0x7fff { return false; }
        if middle <= first { middle < first && last < middle } else { middle < last }
    }
}

#[cfg(target_arch = "arm")]
core::arch::global_asm!(
    ".arm",
    ".global xm1_power_record_helper_f27c2c",
    ".type xm1_power_record_helper_f27c2c,%function",
    "xm1_power_record_helper_f27c2c:",
    "push {r4,r11,lr}",
    "add r11,sp,#8",
    "sub sp,sp,#52",
    "sub r0,r11,#56",
    "bl xm1_power_record_caf_interpolation_frame",
    "sub sp,r11,#8",
    "pop {r4,r11,pc}",
    options(raw),
);

unsafe extern "C" {
    fn xm1_power_record_helper_f27c2c() -> i32;
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_caf_interpolation_frame(parameters: usize) -> i32 {
    unsafe {
        write(parameters + 8, word(0xdfc));
        write(parameters + 12, word(0xe04));
        write(parameters + 16, word(0xe0c));
        write(parameters + 28, half(0xe00) as i32 as u32);
        write(parameters + 32, half(0xe08) as i32 as u32);
        write(parameters + 36, half(0xe10) as i32 as u32);
        let position = xm1_power_record_helper_f8e9e4(parameters as *const u32);
        put_half(0xdf8, position);
        position
    }
}

unsafe fn interpolate_peak() -> i16 {
    unsafe { xm1_power_record_helper_f27c2c() as i16 }
}

unsafe fn evaluate_peak(request: u32, transitions: u32) -> i32 {
    unsafe {
        let position = half(0xb8);
        let value = word(0xa4);
        if byte(4) < 3 { return 2; }
        if word(0xd8c) == 1 { put(0xd64, value); put(0xd8c, 0); return 2; }
        if word(0xd64) < value {
            put(0xd64, value);
            put_half(0xd68, position as i32);
            put(0xda8, word(0xbc));
            put(0xd90, (word(0xdd8) != 0) as u32);
            if half(0xb8) != half(0xba) { rotate_peak(value, position); record_peak(word(0xa4), half(0xb8)); }
        } else if half(0xb8) != half(0xba) {
            if complete_peak(value, position) { put_half(0xd68, interpolate_peak() as i32); }
            if half(0xb8) != half(0xba) { record_peak(word(0xa4), half(0xb8)); }
        }
        let value = word(0xa4);
        let minimum = xm1_power_record_helper_69f8fc(0x8842).wrapping_mul(10);
        let maximum = word(0xd64);
        let current = word(0xa4);
        let stable = if value < minimum { false } else {
            let scale = xm1_power_record_helper_fa7614(request, xm1_power_record_helper_69f8fc(0x8843));
            let margin = maximum.wrapping_mul(scale) / 100;
            let allowance = xm1_power_record_helper_69f8fc(0x8847).wrapping_mul(10);
            let next_max = word(0xd64);
            let next_value = word(0xa4);
            if maximum > current.wrapping_add(margin).wrapping_add(allowance) { false } else {
                let scale = xm1_power_record_helper_fa7614(request, xm1_power_record_helper_69f8fc(0x8843));
                let margin = next_max.wrapping_mul(scale) / 100;
                let allowance = xm1_power_record_helper_69f8fc(0x8847).wrapping_mul(10);
                next_value <= next_max.wrapping_add(margin).wrapping_add(allowance)
            }
        };
        let mut state = if stable { put_byte(0xa2, 0); 2 } else {
            put_byte(0xa2, byte(0xa2).wrapping_add(1));
            if (byte(0xa2) as u32) < xm1_power_record_helper_69f8fc(0x8848) { 2 }
            else { put_byte(4, 0); put_half(0x26, 0); put_byte(0x96, 0); 13 }
        };
        let count = byte(0xa0) as u32;
        let limit = xm1_power_record_helper_69f8fc(0x883f);
        let previous = word(0xa8);
        let current = word(0xa4);
        if count < limit {
            if transitions <= byte(0xa1) as u32 {
                let margin = xm1_power_record_helper_69fec8(0x8887).wrapping_mul(10);
                state = if current.wrapping_add(margin) < previous || xm1_power_record_helper_69f8fc(0x8819) == 0 { 18 } else { put_byte(0x96, 0); 13 };
                put_byte(4, 0);
                put_half(0x26, 0);
            }
        } else {
            put_byte(0xf1, 0); put_byte(0xf0, 0); put_half(0xec, half(0x2a) as i32); put_byte(4, 0); put_half(0x26, 0);
            state = 14;
        }
        state
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f27fcc(request: u32) -> i32 {
    unsafe {
        put(0x38, 2);
        if word(0x18) != 0 { put_byte(4, 0); put_byte(0x96, 0); return 13; }
        if word(0xddc) != word(0xdd8) { put_byte(4, 0); return 16; }
        begin_iteration();
        let clock = xm1_power_record_helper_76f360();
        let step = xm1_power_record_helper_faab3c(request, 7, word(0x7c), word(0xdc8), word(0xe1c), clock, (SCAN + 0x10) as *mut u32);
        put_half(0x26, step);
        track_direction();
        choose_direction();
        let transitions = xm1_power_record_helper_69f8fc(0x883d);
        let state = evaluate_peak(request, transitions);
        end_iteration();
        if state == 2 { 16 } else { state }
    }
}

unsafe extern "C" {
    fn xm1_power_record_helper_ee9928(value: u32, output: *mut u32);
    fn xm1_power_record_helper_fa9320(value: u32, maximum: u32, minimum: u32, threshold: u32) -> u32;
    fn xm1_power_record_helper_fa936c(history: u32, index: u32, cursor: *mut u32, threshold: u32, phase: u32) -> u32;
    fn xm1_power_record_helper_fa9488(history: u32, index: u32) -> u32;
    fn xm1_power_record_helper_fa7ce0(request: u32, frame: u32, threshold: u32, lower: i32, upper: i32) -> i32;
    fn xm1_power_record_helper_04d978(dividend: u32, divisor: u32) -> u32;
}

unsafe fn stop_history() { unsafe { put_half(0x26, 0); put_byte(4, 0); } }

unsafe fn initialize_search_history() {
    unsafe {
        put_byte(0xd5f, 0);
        put_byte(0xef, 0);
        put_byte(0xee, 0);
        let selected = if xm1_power_record_helper_69f8fc(0x8804) == 1 { (word(0xdd8) != 0) as u32 } else { 1 };
        put(0xcf8, selected);
        put(0xd00, 0);
        put(0xd04, 0);
        put(0xd08, u32::MAX);
        put(0xd0c, u32::MAX);
        put_half(0x70, half(0x6e) as i32);
        reset_extrema();
        if word(0xdd8) == 1 { put_byte(0xcfc, 2); }
        else if xm1_power_record_helper_69f8fc(0x8821) == 1 { put_byte(0xcfc, byte(0xcfc) | 3); }
        else { put_byte(0xcfc, if word(0xcf8) == 0 { 1 } else { 2 }); }
        put_half(0xcf4, 0);
        put(0xd10, 0);
        put(0xd14, 0);
        put_half(0xd20, 0);
        put(0xd24, 2);
    }
}

unsafe fn history_confident(request: u32) -> bool {
    unsafe {
        if xm1_power_record_helper_69f8fc(0x8803) & 2 == 0 { return false; }
        let mut history = [0; 3];
        xm1_power_record_helper_7567ec(request, history.as_mut_ptr());
        if history[0] == 0 { return false; }
        let needed = xm1_power_record_helper_69f8fc(0x944c);
        if (byte(4) as u32) < needed { return false; }
        let threshold = xm1_power_record_helper_69fec8(0x944f);
        let mut index = history[1] as u8 as u16;
        let end = index.wrapping_sub(5);
        let mut consecutive = 0u8;
        loop {
            let wrapped = if index & 0x8000 != 0 { index.wrapping_add(5) } else { index };
            let offset = (wrapped as i16 as i32).wrapping_mul(0x228);
            let record = history[2].wrapping_add(offset as u32) as usize;
            if read(record) != 0 {
                let first = read_volatile((record + 0x204) as *const u16) as u32;
                let second = read_volatile((record + 0x206) as *const u16) as u32;
                if first < threshold && second < threshold { consecutive = 0; }
                else { consecutive = consecutive.wrapping_add(1); if needed <= consecutive as u32 { return true; } }
            }
            index = index.wrapping_sub(1);
            if index == end { return false; }
        }
    }
}

unsafe fn update_history_extrema() {
    unsafe {
        if word(0xdd8) == 0 && word(0xd00) < word(0x50) { put(0xd00, word(0x50)); }
        if word(0xd04) < word(0x4c) { put(0xd04, word(0x4c)); }
        if word(0xdd8) == 0 && word(0x50) < word(0xd08) { put(0xd08, word(0x50)); }
        if word(0x4c) < word(0xd0c) { put(0xd0c, word(0x4c)); }
    }
}

unsafe fn limit_step() {
    unsafe {
        let distance = if byte(0x28) as i8 == 1 { (half(0x1e) as u16).wrapping_sub(half(0x2a) as u16) } else { (half(0x2a) as u16).wrapping_sub(half(0x1c) as u16) };
        let step = half(0x26) as i32;
        if (distance as i32) < step * 2 {
            let half_distance = ((distance as u32 + 1) >> 1) as i16;
            if step > half_distance as i32 { put_half(0x26, half_distance as i32); }
        }
    }
}

unsafe fn configured_threshold(keys: [u32; 4]) -> u32 {
    unsafe {
        let mut converted = [0; 2];
        xm1_power_record_helper_ee9928(word(0x4c), converted.as_mut_ptr());
        let first = xm1_power_record_helper_69f8fc(keys[0]);
        let second = xm1_power_record_helper_69f8fc(keys[1]);
        let lower = xm1_power_record_helper_69f8fc(keys[2]).wrapping_mul(100);
        let upper = xm1_power_record_helper_69f8fc(keys[3]).wrapping_mul(100);
        let sample = word(0x8f0 + (half(0xcf4) as u16 as usize) * 4);
        if sample >= 10000 { return second; }
        let value = converted[0];
        if value < lower { return first; }
        if value > upper { return second; }
        let numerator = first.wrapping_sub(second).wrapping_mul(value.wrapping_sub(lower));
        first.wrapping_sub(xm1_power_record_helper_04d978(numerator, upper.wrapping_sub(lower)))
    }
}

unsafe fn update_falling_flags(mask: u8) {
    unsafe {
        let threshold = configured_threshold([0x8829, 0x8822, 0x882b, 0x882c]);
        put(0xd1c, threshold);
        if mask & 1 != 0 && byte(0xee) & 1 == 0 && xm1_power_record_helper_fa9320(word(0x50), word(0xd00), word(0xd08), word(0xd1c)) != 0 { put_byte(0xee, byte(0xee) | 1); }
        if mask & 2 != 0 && byte(0xee) & 2 == 0 && xm1_power_record_helper_fa9320(word(0x4c), word(0xd04), word(0xd0c), word(0xd1c)) != 0 { put_byte(0xee, byte(0xee) | 2); }
    }
}

unsafe fn update_rising_flags(mask: u8) {
    unsafe {
        let threshold = configured_threshold([0x882a, 0x8826, 0x882d, 0x882e]);
        put(0xd18, threshold);
        if mask & 1 != 0 && byte(0xef) & 1 == 0 {
            let index = byte(0xcf4).wrapping_sub(1) as u32;
            if xm1_power_record_helper_fa936c((SCAN + 0xf4) as u32, index, (SCAN + 0xd10) as *mut u32, word(0xd18), read(FRAME + 0x48)) != 0 { put_byte(0xef, byte(0xef) | 1); }
        }
        if mask & 2 != 0 && byte(0xef) & 2 == 0 {
            let index = byte(0xcf4).wrapping_sub(1) as u32;
            if xm1_power_record_helper_fa936c((SCAN + 0x4f4) as u32, index, (SCAN + 0xd14) as *mut u32, word(0xd18), read(FRAME + 0x48)) != 0 { put_byte(0xef, byte(0xef) | 2); }
        }
        if byte(0xef) == 0 {
            let sample = word(0x4f0 + half(0xcf4) as u16 as usize * 4);
            if sample < xm1_power_record_helper_69f8fc(0x882f).wrapping_mul(10) {
                let index = byte(0xcf4).wrapping_sub(1) as u32;
                if xm1_power_record_helper_fa9488((SCAN + 0x8f4) as u32, index) != 0 { put_byte(0xef, byte(0xef) | 2); }
            }
        }
    }
}

unsafe fn position_changed() -> bool {
    unsafe {
        let previous = half(0x70) as i32;
        let current = half(0x6e) as i32;
        if current > previous.wrapping_add(xm1_power_record_helper_69f8fc(0x883a).wrapping_mul(10) as i32) { return true; }
        let current = half(0x6e) as i32;
        current.wrapping_add(xm1_power_record_helper_69f8fc(0x883a).wrapping_mul(10) as i32) < half(0x70) as i32
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f287e8(request: u32) -> i32 {
    unsafe {
        put(0x38, 2);
        if word(0x18) != 0 { stop_history(); put_byte(0x96, 0); return 13; }
        if word(0xddc) != word(0xdd8) { put_byte(0x28, byte(0x29)); stop_history(); return 14; }
        if byte(4) == 1 { initialize_search_history(); }
        if read(FRAME + 0x48) == 0 && word(0xdd8) == 0 && history_confident(request) {
            write(FRAME + 0x48, 1);
            put_byte(0x28, byte(0x29));
            stop_history();
            return 14;
        }
        if byte(4) > 2 {
            let index = (half(0xcf4) as u16).min(255);
            put_half(0xcf4, index as i32);
            let offset = index as usize * 4;
            put(0xf4 + offset, word(0x50));
            put(0x4f4 + offset, word(0x4c));
            put(0x8f4 + offset, word(0x64));
            put_half(0xcf4, index.wrapping_add(1) as i32);
            let value = word(0xdc8);
            if word(0xd00 + word(0xcf8) as usize * 4) < value {
                put_half(0xd20, half(0x2e) as i32);
                put(0xd24, word(0x44));
                put(0xd90, (word(0xdd8) != 0) as u32);
                rotate_peak(value, half(0x2e));
            } else if complete_peak(value, half(0x2e)) { put_half(0xd20, interpolate_peak() as i32); }
            record_peak(word(0xdc8), half(0x2e));
            update_history_extrema();
        }
        let clock = xm1_power_record_helper_76f360();
        let step = xm1_power_record_helper_faab3c(request, 6, word(0x7c), word(0xdc8), word(0xe1c), clock, (SCAN + 0x10) as *mut u32);
        put_half(0x26, step);
        if byte(4) < 4 { put_byte(0x28, byte(0x29)); limit_step(); return 14; }
        if xm1_power_record_helper_69f8fc(0x8839) == 1 && position_changed() {
            stop_history();
            put_byte(0x96, 0);
            put_byte(0x28, byte(0x29).wrapping_neg());
            limit_step();
            return 13;
        }
        if byte(0xee) != byte(0xcfc) { update_falling_flags(byte(0xcfc)); }
        if byte(0xef) != byte(0xcfc) { update_rising_flags(byte(0xcfc)); }
        let mask = byte(0xcfc);
        let state = if mask & byte(0xef) != 0 {
            let state = if mask & byte(0xee) == 0 { 14 }
            else if xm1_power_record_helper_69f8fc(0x8839) == 1 || !position_changed() {
                put(0xd64, word(0xd00 + word(0xcf8) as usize * 4));
                put_half(0xd68, half(0xd20) as i32);
                put(0xda8, word(0xd24));
                18
            } else { put_byte(0x96, 0); 13 };
            put_byte(0x28, byte(0x29).wrapping_neg());
            stop_history();
            state
        } else {
            put_byte(0x28, byte(0x29));
            if half(0x2a) <= half(0x1c) { put_byte(0x28, 1); put_half(0x26, xm1_power_record_helper_69f8fc(0x8837) as i32); }
            else if half(0x2a) >= half(0x1e) { put_byte(0x28, 255); put_half(0x26, xm1_power_record_helper_69f8fc(0x8837) as i32); }
            if half(0x2e) <= half(0x1c) { put_byte(0xf0, 1); }
            else {
                if half(0x2e) >= half(0x1e) { put_byte(0xf1, 1); }
                if byte(0xf0) == 0 { limit_step(); return 14; }
            }
            if byte(0xf1) == 0 { 14 } else {
                let selected = word(0xcf8) as usize * 4;
                let maximum = word(0xd00 + selected);
                put(0xd64, maximum);
                put_half(0x70, half(0x6e) as i32);
                let other = word(0xd08 + selected);
                let margin = xm1_power_record_helper_69f8fc(0x8807).wrapping_mul(10);
                if maximum < other.wrapping_add(margin) {
                    if word(0xdd8) == 0 || word(0xde8) == 1 {
                        put_byte(0xd5f, 2);
                        put_half(0xd68, half(0x24) as i32);
                        put(0xda8, 1);
                    } else {
                        let value = xm1_power_record_helper_fa7ce0(request, word(0x34), half(0xde0) as u16 as u32, half(0x1c) as i32, half(0x1e) as i32);
                        put_half(0xd86, value);
                        put_byte(0xd5f, 2);
                        put_half(0xd68, value);
                        put(0xda8, 1);
                        put_half(0xd88, value);
                    }
                } else {
                    put_byte(0xd5f, 1);
                    put_half(0xd68, half(0xd20) as i32);
                    put(0xda8, word(0xd24));
                }
                stop_history();
                18
            }
        };
        limit_step();
        state
    }
}
