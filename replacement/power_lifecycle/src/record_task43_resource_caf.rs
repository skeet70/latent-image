use core::ptr::{read_volatile, write_volatile};

const SCAN: usize = 0x016b66ec;
const CONTROL: usize = 0x016b66d4;
const FRAME: usize = 0x016b665c;

unsafe extern "C" {
    fn xm1_power_record_helper_69f8fc(key: u32) -> u32;
    fn xm1_power_record_helper_69fec8(key: u32) -> u32;
    fn xm1_power_record_helper_76da84() -> u32;
    fn xm1_power_record_helper_746054() -> u32;
    fn xm1_power_record_helper_fa767c(request: u32, first: *mut i16, second: *mut i16, lower: *mut i16, upper: *mut i16, reference: *mut i16, mode: u32, available: u32);
    fn xm1_power_record_helper_76f360() -> u32;
    fn xm1_power_record_helper_feaaec(callback: unsafe extern "C" fn(u32, i32, u32), parameters: *const u32);
    fn xm1_power_record_helper_f4c400(callback: unsafe extern "C" fn(u32, u32), parameters: *const u32);
    fn xm1_power_record_helper_ef9a94(event: u32, payload: *const u32, length: u32);
    fn xm1_power_record_helper_f264e8(request: u32);
    fn xm1_power_record_helper_756b54(request: u32) -> u32;
    fn xm1_power_record_helper_75a960(kind: u32, enabled: u32) -> u32;
    fn xm1_power_record_helper_75a7c4(request: u32, index: u32, mode: u32, source: u32) -> u32;
    fn xm1_power_record_helper_76dec8(callback: unsafe extern "C" fn(u32, u32, u32, u32), first: u32, second: u32, third: u32, mode: u32, request: u32) -> i32;
    fn xm1_power_record_helper_76d9a8() -> u32;
    fn xm1_power_record_helper_76da38() -> u32;
    fn xm1_power_record_helper_76dba4() -> u32;
    fn xm1_power_record_helper_efb104(status: u32, route: u32, changed: u32, modes: u32);
    fn xm1_power_record_helper_76df7c(zero: u32, value: u32, request: u32) -> i32;
    fn xm1_power_record_helper_efb194(value: u32);
    fn xm1_power_record_helper_efb1dc(value: u32);
    fn xm1_power_record_helper_76e008() -> i32;
}

unsafe fn word(offset: usize) -> u32 { unsafe { read_volatile((SCAN + offset) as *const u32) } }
unsafe fn half(offset: usize) -> i16 { unsafe { read_volatile((SCAN + offset) as *const i16) } }
unsafe fn byte(offset: usize) -> u8 { unsafe { read_volatile((SCAN + offset) as *const u8) } }
unsafe fn put(offset: usize, value: u32) { unsafe { write_volatile((SCAN + offset) as *mut u32, value) } }
unsafe fn put_half(offset: usize, value: i32) { unsafe { write_volatile((SCAN + offset) as *mut i16, value as i16) } }
unsafe fn put_byte(offset: usize, value: u8) { unsafe { write_volatile((SCAN + offset) as *mut u8, value) } }
unsafe fn read(address: usize) -> u32 { unsafe { read_volatile(address as *const u32) } }
unsafe fn write(address: usize, value: u32) { unsafe { write_volatile(address as *mut u32, value) } }

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f25784() {
    unsafe {
        let value = xm1_power_record_helper_76da84() as i32;
        put_half(12, value);
        put_byte(4, 0);
        for offset in [0xd68, 0x2a, 0x2c, 0x30] { put_half(offset, value); }
        put_byte(0x29, 1);
        put_byte(0x28, 1);
        for offset in [0x3c, 0x40, 0x44, 0x48] { put(offset, 2); }
        put(8, 13);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f257e4() -> i32 { unsafe { (read(CONTROL) == 0) as i32 } }

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f2580c(request: u32) {
    unsafe {
        let mode = word(0x14);
        let available = xm1_power_record_helper_746054();
        xm1_power_record_helper_fa767c(request, (SCAN + 0x20) as *mut i16, (SCAN + 0x22) as *mut i16, (SCAN + 0x1c) as *mut i16, (SCAN + 0x1e) as *mut i16, (SCAN + 0x24) as *mut i16, mode, available);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f25860() {
    unsafe {
        let alternate = word(0xdd8) == 0 && xm1_power_record_helper_69f8fc(if word(8) == 13 { 0x8805 } else { 0x8804 }) == 1;
        put(0xdc8, word(if alternate { 0x50 } else { 0x4c }));
        put(0xdcc, word(if alternate { 0x58 } else { 0x54 }));
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f258e0() {
    unsafe {
        if half(0xd68) < half(0x1c) {
            put_half(0xd68, half(0x1c) as i32);
            put(0xda8, 2);
        } else if half(0xd68) > half(0x1e) {
            put_half(0xd68, half(0x1e) as i32);
            put(0xda8, 2);
        }
        let target = half(0xd68);
        let current = half(0x2a);
        let direction = if target > current { 1 } else { -1 };
        put_byte(0x28, direction as u8);
        put_half(0x26, (target as i32 - current as i32) * direction);
        put(0x38, word(0xda8));
        put_half(12, target as i32);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f259b8() {
    unsafe {
        let current = half(0x2a) as i32;
        let lower = half(0x1c) as i32;
        let upper = half(0x1e) as i32;
        let distance = if current < lower {
            put_byte(0x28, 1);
            (lower - current) as u16
        } else if current > upper {
            put_byte(0x28, 255);
            (current - upper) as u16
        } else {
            match byte(0x28) as i8 {
                -1 if current - (half(0x26) as i32) < lower => put_half(0x26, current - lower),
                1 if current + (half(0x26) as i32) > upper => put_half(0x26, upper - current),
                _ => (),
            }
            return;
        };
        let distance = if distance as u32 > xm1_power_record_helper_69f8fc(0x8838) { xm1_power_record_helper_69f8fc(0x8838) as u16 } else { distance };
        if half(0x26) < distance as i16 { put_half(0x26, distance as i16 as i32); }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f25afc() -> i32 { unsafe { (read(CONTROL) == 0 && read(CONTROL + 4) == 0) as i32 } }

#[unsafe(no_mangle)]
pub extern "C" fn xm1_power_record_helper_f25b3c() -> i32 { 1 }

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f25b54() {
    unsafe {
        write(CONTROL + 4, 0);
        write(CONTROL + 8, read(0x0169d380));
        let value = read_volatile(0x0169d384 as *const i16);
        write_volatile((CONTROL + 12) as *mut i16, value);
        let mode = read(0x0169d388);
        write(CONTROL + 16, mode);
        put_half(0x30, value as i32);
        put(0x48, mode);
        put_half(0x2a, value as i32);
        put(0x3c, mode);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f25758(first: u32, second: u32) {
    unsafe {
        let payload = [first, second];
        xm1_power_record_helper_ef9a94(0x9d, payload.as_ptr(), 8);
    }
}

#[cfg(target_arch = "arm")]
core::arch::global_asm!(
    ".arm",
    ".global xm1_power_record_helper_f25728",
    ".type xm1_power_record_helper_f25728,%function",
    "xm1_power_record_helper_f25728:",
    "push {r11,lr}",
    "add r11,sp,#4",
    "sub sp,sp,#16",
    "str r0,[r11,#-16]",
    "strh r1,[r11,#-12]",
    "str r2,[r11,#-8]",
    "sub r0,r11,#16",
    "bl xm1_power_record_caf_step_completion",
    "sub sp,r11,#4",
    "pop {r11,pc}",
    options(raw),
);

unsafe extern "C" {
    fn xm1_power_record_helper_f25728(first: u32, second: i32, third: u32);
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_caf_step_completion(payload: *const u32) {
    unsafe { xm1_power_record_helper_ef9a94(0x5a, payload, 12); }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f25bb0() {
    unsafe {
        write(CONTROL, 1);
        xm1_power_record_helper_f4c400(xm1_power_record_helper_f25758, FRAME as *const u32);
    }
}

#[cfg(target_arch = "arm")]
core::arch::global_asm!(
    ".arm",
    ".global xm1_power_record_helper_f25be0",
    ".type xm1_power_record_helper_f25be0,%function",
    "xm1_power_record_helper_f25be0:",
    "push {r11,lr}",
    "add r11,sp,#4",
    "sub sp,sp,#24",
    "sub r1,r11,#24",
    "bl xm1_power_record_caf_step_frame",
    "sub sp,r11,#4",
    "pop {r11,pc}",
    options(raw),
);

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_caf_step_frame(enabled: u32, parameters: usize) {
    unsafe {
        let displacement = if enabled == 1 { (half(0x26) as i32).wrapping_mul(byte(0x28) as i8 as i32) as u16 as u32 } else { 0 };
        let mode = if enabled == 1 { word(0x38) } else { 2 };
        write_volatile(parameters as *mut u16, displacement as u16);
        write(parameters + 4, word(0x10));
        write(parameters + 8, enabled);
        write(parameters + 12, mode);
        write(parameters + 16, xm1_power_record_helper_76f360());
        if xm1_power_record_helper_69f8fc(0x3e0246) == 1 { write(parameters + 12, 2); }
        if read(CONTROL + 4) == 0 {
            write(CONTROL + 4, 1);
            xm1_power_record_helper_feaaec(xm1_power_record_helper_f25728, parameters as *const u32);
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f267b0(request: u32) -> i32 {
    unsafe {
        if read(CONTROL + 8) != 0 { return 0; }
        xm1_power_record_helper_f264e8(request);
        put_byte(4, 0);
        put(8, 16);
        1
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f267f8(request: u32) {
    unsafe {
        write(CONTROL, 0);
        let source = read(0x0169d384) as usize;
        for (destination, offset) in [(0x5c, 0x10), (0x60, 0x14), (0x54, 8), (0x58, 12), (0x4c, 0), (0x50, 4), (0x64, 0x60)] { put(destination, read(source + offset)); }
        put_half(0x68, read(source + 0x28) as i32);
        put_half(0x6a, read(source + 0x2c) as i32);
        put(0x74, read(source + 0x64));
        if word(0) == 0 {
            put(0, 1);
            if word(8) == 16 {
                xm1_power_record_helper_f25860();
                put(0xd64, word(0xdc8));
            }
        }
        put(0x18, read(read(0x0169d384) as usize + 0x68));
        put_half(0x2e, half(0x2c) as i32);
        put_half(0x2c, half(0x2a) as i32);
        put_byte(0x29, byte(0x28));
        put(0x44, word(0x40));
        put(0x40, word(0x3c));
        put_half(0x2a, half(0x30) as i32);
        put(0x3c, word(0x48));
        if word(0xde8) == 1 {
            put_half(0xd86, half(0x24) as i32);
            put_half(0xde0, xm1_power_record_helper_69f8fc(0xa4e0) as i32);
        }
        let count = byte(4).wrapping_add(1);
        put_byte(4, if count == 255 { 254 } else { count });
        xm1_power_record_helper_f264e8(request);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f28e94(status: u32, route: u32, first: u32, second: u32) {
    unsafe {
        write(0x016b750c, route);
        let payload = [status, route, first, second];
        xm1_power_record_helper_ef9a94(if status == 18 { 0x33 } else { 0x34 }, payload.as_ptr(), 16);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f28edc(request: u32, use_mode: u32) -> i32 {
    unsafe {
        let source = xm1_power_record_helper_75a960(8, 1);
        let request = xm1_power_record_helper_75a7c4(request, 63, 3, source);
        xm1_power_record_helper_76dec8(xm1_power_record_helper_f28e94, read(0x0169d1cc), read(0x0169d1d0), read(0x0169d1d4), if use_mode == 0 { 1 } else { read(0x0169d1d8) }, request)
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f28f4c(status: i32) {
    unsafe {
        let mut modes = if xm1_power_record_helper_76d9a8() == 2 { 2 } else { 0 };
        if xm1_power_record_helper_76da38() == 2 { modes |= 1; }
        if xm1_power_record_helper_76dba4() == 2 { modes |= 4; }
        xm1_power_record_helper_efb104(status as u32, read(0x016b750c), (read(0x0169d1d8) != 1) as u32, modes);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f28fac(request: u32) -> i32 { unsafe { xm1_power_record_helper_76df7c(0, read(0x0169d1e0), request) } }

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f28fd0(value: u32) { unsafe { xm1_power_record_helper_efb194(value); } }

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f28fe0() -> i32 { unsafe { xm1_power_record_helper_76e008() } }

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f28ff0(value: u32) { unsafe { xm1_power_record_helper_efb1dc(value); } }
