use core::ptr::{read_volatile, write_volatile};

#[repr(C)]
struct HeaderTriplet { words: [u16; 3] }
#[repr(C)]
struct ProfileWords5 { words: [u32; 5] }

extern "C" {
    fn __aeabi_uidiv(numerator: u32, denominator: u32) -> u32;
    fn xm1_power_configuration_byte(argument0: u32) -> u32;
    fn xm1_power_recording_capture_trace_begin(argument0: u32);
    fn xm1_power_record_helper_740e74(argument0: u32, argument1: u32);
    fn xm1_power_record_helper_746c2c(argument0: u32) -> u32;
    fn xm1_power_record_helper_74e5c0(argument0: u32) -> u32;
    fn xm1_power_record_helper_74e7a0(argument0: u32) -> u32;
    fn xm1_power_record_helper_74eba8(argument0: u32) -> u32;
    fn xm1_power_record_helper_74ed6c(argument0: u32) -> u32;
    fn xm1_power_record_helper_74f1ac(argument0: u32) -> u32;
    fn xm1_power_record_helper_74f9a4(argument0: u32) -> u32;
    fn xm1_power_record_helper_74fcec(argument0: u32) -> u32;
    fn xm1_power_record_helper_74ff0c(argument0: *mut HeaderTriplet, argument1: u32) -> *mut HeaderTriplet;
    fn xm1_power_record_helper_74ffac(argument0: u32) -> u32;
    fn xm1_power_record_helper_7503ec(argument0: u32) -> u32;
    fn xm1_power_record_helper_756a34(argument0: u32) -> u32;
    fn xm1_power_record_helper_756ac4(argument0: u32) -> u32;
    fn xm1_power_record_helper_756b0c(argument0: u32) -> u32;
    fn xm1_power_record_helper_756b2c(argument0: u32, argument1: u32);
    fn xm1_power_record_helper_756b54(argument0: u32) -> u32;
    fn xm1_power_record_helper_756e24(argument0: *mut ProfileWords5) -> *mut ProfileWords5;
    fn xm1_power_record_helper_756e5c(argument0: u32, argument1: *const ProfileWords5);
    fn xm1_power_record_helper_7577e0() -> u32;
    fn xm1_power_record_helper_757828(argument0: u32) -> u32;
    fn xm1_power_record_helper_75784c(argument0: u32, argument1: u32);
    fn xm1_power_record_helper_757bc0() -> u32;
    fn xm1_power_record_helper_757c10(argument0: u32) -> u32;
    fn xm1_power_record_helper_757c60(argument0: u32) -> u32;
    fn xm1_power_record_helper_757df0(argument0: u32) -> u32;
    fn xm1_power_record_helper_757e38(argument0: u32) -> u32;
    fn xm1_power_record_helper_7580ec(argument0: u32) -> u32;
    fn xm1_power_record_helper_76d9c8() -> u32;
    fn xm1_power_record_helper_772f28(argument0: u32) -> u32;
    fn __aeabi_uldivmod(numerator: u64, denominator: u64) -> u64;
    fn xm1_power_record_helper_ef3434(argument0: u32, argument1: u32, argument2: u32);
    fn xm1_power_record_helper_ef9a94(argument0: u32, argument1: u32, argument2: u32);
    fn xm1_power_record_helper_f4c490(argument0: u32, argument1: u32) -> u32;
    fn xm1_power_record_helper_f4d5bc(argument0: u32, argument1: u32);
    fn xm1_power_record_helper_f4f838(argument0: u32, argument1: u32, argument2: u32, argument3: u32);
    fn xm1_power_record_helper_f5eba0(argument0: *mut u32, argument1: *mut u32, argument2: *mut u32, argument3: *mut u32, argument4: u32, argument5: u32);
    fn xm1_power_record_helper_fa7928(argument0: u32) -> u32;
    fn xm1_power_record_helper_fa7e20(argument0: i32, argument1: u32, argument2: u32) -> u32;
    fn xm1_power_record_helper_fa8018() -> u32;
    fn xm1_power_record_helper_fa8078(argument0: u32, argument1: u32, argument2: u32) -> u32;
    fn xm1_power_record_helper_fa80f4(argument0: u32);
    fn xm1_power_record_helper_fea7ac(argument0: u32, argument1: *mut u8);
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_f37cbc() {
    enum Flow { B0, B1, B2, B3, B4 }
    let mut frame = core::mem::MaybeUninit::<[u32; 30]>::uninit();
    let mut sp = (frame.as_mut_ptr() as usize as u32).wrapping_add(120);
    let mut r0: u32 = 0;
    let mut r1: u32 = 0;
    let mut r2: u32 = 0;
    let mut r3: u32 = 0;
    let mut r4: u32 = 0;
    let mut r5: u32 = 0;
    let mut r11: u32 = 0;
    let mut r12: u32 = 0;
    let mut equal = false;
    let mut lower_or_same = false;
    let mut lower = false;
    let mut next = Flow::B0;
    unsafe {
        loop {
            match next {
                Flow::B0 => {
                    sp = sp.wrapping_sub(16);
                    r11 = sp.wrapping_add(12u32);
                    sp = sp.wrapping_sub(104u32);
                    r3 = 0xfe54u32;
                    r3 = (r3 & 0xffff) | (0x16bu32 << 16);
                    r0 = read_volatile((r3.wrapping_add(0x220u32)) as *const u32) as u32;
                    r0 = xm1_power_record_helper_756a34(r0);
                    equal = r0 == 2u32; lower_or_same = r0 <= 2u32; lower = r0 < 2u32;
                    if equal { next = Flow::B2; } else { next = Flow::B1; }
                    continue;
                }
                Flow::B1 => {
                    r0 = 5u32;
                    r1 = 0u32;
                    r2 = r11.wrapping_sub(32u32);
                    xm1_power_record_helper_ef3434(r0, r1, r2);
                    r12 = read_volatile((r11.wrapping_add((-32i32 as u32))) as *const u16) as u32;
                    r3 = 0xfe54u32;
                    r3 = (r3 & 0xffff) | (0x16bu32 << 16);
                    r2 = 100u32;
                    r1 = r2.wrapping_mul(r12);
                    write_volatile((r3.wrapping_add(0xcu32)) as *mut u32, r1 as u32);
                    r0 = read_volatile((r11.wrapping_add((-30i32 as u32))) as *const u16) as u32;
                    r1 = r2.wrapping_mul(r0);
                    write_volatile((r3.wrapping_add(0x10u32)) as *mut u32, r1 as u32);
                    r1 = read_volatile((r11.wrapping_add((-28i32 as u32))) as *const u16) as u32;
                    r2 = r2.wrapping_mul(r1);
                    write_volatile((r3.wrapping_add(0x14u32)) as *mut u32, r2 as u32);
                    r2 = 536u32;
                    write_volatile((r3.wrapping_add(r2)) as *mut u16, r12 as u16);
                    r2 = r2.wrapping_add(2u32);
                    write_volatile((r3.wrapping_add(r2)) as *mut u16, r0 as u16);
                    r2 = 540u32;
                    write_volatile((r3.wrapping_add(r2)) as *mut u16, r1 as u16);
                    r2 = 1u32;
                    write_volatile((r3.wrapping_add(0x4u32)) as *mut u32, r2 as u32);
                    r2 = 0u32;
                    write_volatile((r3.wrapping_add(0x1u32)) as *mut u8, r2 as u8);
                    next = Flow::B2;
                }
                Flow::B2 => {
                    r4 = 0xfe54u32;
                    r4 = (r4 & 0xffff) | (0x16bu32 << 16);
                    r0 = read_volatile((r4.wrapping_add(0x220u32)) as *const u32) as u32;
                    r0 = xm1_power_record_helper_fa7928(r0);
                    write_volatile((r11.wrapping_add((-0x5ci32 as u32))) as *mut u32, r0 as u32);
                    r0 = read_volatile((r4.wrapping_add(0x220u32)) as *const u32) as u32;
                    r0 = xm1_power_record_helper_74fcec(r0);
                    write_volatile((r11.wrapping_add((-0x58i32 as u32))) as *mut u32, r0 as u32);
                    r0 = read_volatile((r4.wrapping_add(0x220u32)) as *const u32) as u32;
                    r0 = xm1_power_record_helper_74ffac(r0);
                    write_volatile((r11.wrapping_add((-0x54i32 as u32))) as *mut u32, r0 as u32);
                    r0 = read_volatile((r4.wrapping_add(0x220u32)) as *const u32) as u32;
                    r0 = xm1_power_record_helper_74f9a4(r0);
                    write_volatile((r11.wrapping_add((-0x50i32 as u32))) as *mut u32, r0 as u32);
                    r0 = read_volatile((r4.wrapping_add(0x220u32)) as *const u32) as u32;
                    r0 = xm1_power_record_helper_74f1ac(r0);
                    write_volatile((r11.wrapping_add((-0x4ci32 as u32))) as *mut u32, r0 as u32);
                    r0 = read_volatile((r4.wrapping_add(0x220u32)) as *const u32) as u32;
                    r0 = xm1_power_record_helper_74ed6c(r0);
                    write_volatile((r11.wrapping_add((-0x48i32 as u32))) as *mut u32, r0 as u32);
                    r0 = r11.wrapping_sub(116u32);
                    r1 = read_volatile((r4.wrapping_add(0x220u32)) as *const u32) as u32;
                    r0 = xm1_power_record_helper_74ff0c(r0 as *mut HeaderTriplet, r1) as usize as u32;
                    r0 = r11.wrapping_sub(68u32);
                    r1 = r11.wrapping_sub(116u32);
                    r2 = 6u32;
                    core::ptr::copy_nonoverlapping(r1 as *const u8, r0 as *mut u8, r2 as usize);
                    r5 = 0u32;
                    write_volatile((r11.wrapping_add((-0x3ci32 as u32))) as *mut u32, r5 as u32);
                    r0 = xm1_power_record_helper_76d9c8();
                    write_volatile((r11.wrapping_add((-0x38i32 as u32))) as *mut u32, r0 as u32);
                    write_volatile((r11.wrapping_add((-52i32 as u32))) as *mut u16, r5 as u16);
                    write_volatile((r11.wrapping_add((-50i32 as u32))) as *mut u16, r5 as u16);
                    write_volatile((r11.wrapping_add((-48i32 as u32))) as *mut u16, r5 as u16);
                    r2 = read_volatile((r4.wrapping_add(0x10u32)) as *const u32) as u32;
                    r3 = 0x851fu32;
                    r3 = (r3 & 0xffff) | (0x51ebu32 << 16);
                    { let product = (r3 as u64).wrapping_mul(r2 as u64); r1 = product as u32; r2 = (product >> 32) as u32; }
                    r2 = r2.wrapping_shr(5u32);
                    write_volatile((r11.wrapping_add((-44i32 as u32))) as *mut u16, r2 as u16);
                    r2 = read_volatile((r4.wrapping_add(0xcu32)) as *const u32) as u32;
                    { let product = (r3 as u64).wrapping_mul(r2 as u64); r1 = product as u32; r2 = (product >> 32) as u32; }
                    r2 = r2.wrapping_shr(5u32);
                    write_volatile((r11.wrapping_add((-46i32 as u32))) as *mut u16, r2 as u16);
                    r2 = read_volatile((r4.wrapping_add(0x14u32)) as *const u32) as u32;
                    { let product = (r3 as u64).wrapping_mul(r2 as u64); r1 = product as u32; r3 = (product >> 32) as u32; }
                    r3 = r3.wrapping_shr(5u32);
                    write_volatile((r11.wrapping_add((-42i32 as u32))) as *mut u16, r3 as u16);
                    write_volatile((r11.wrapping_add((-0x28i32 as u32))) as *mut u32, r5 as u32);
                    r5 = 1u32;
                    write_volatile((r11.wrapping_add((-0x24i32 as u32))) as *mut u32, r5 as u32);
                    r0 = r11.wrapping_sub(92u32);
                    r1 = r11.wrapping_sub(104u32);
                    xm1_power_record_helper_f4d5bc(r0, r1);
                    r3 = read_volatile((r11.wrapping_add((-102i32 as u32))) as *const u16) as u32;
                    write_volatile((r11.wrapping_add((-30i32 as u32))) as *mut u16, r3 as u16);
                    r3 = read_volatile((r11.wrapping_add((-104i32 as u32))) as *const u16) as u32;
                    write_volatile((r11.wrapping_add((-32i32 as u32))) as *mut u16, r3 as u16);
                    r3 = read_volatile((r11.wrapping_add((-100i32 as u32))) as *const u16) as u32;
                    write_volatile((r11.wrapping_add((-28i32 as u32))) as *mut u16, r3 as u16);
                    r3 = 0x21au32;
                    r3 = read_volatile((r4.wrapping_add(r3)) as *const u16) as u32;
                    write_volatile((r11.wrapping_add((-24i32 as u32))) as *mut u16, r3 as u16);
                    r3 = 536u32;
                    r3 = read_volatile((r4.wrapping_add(r3)) as *const u16) as u32;
                    write_volatile((r11.wrapping_add((-26i32 as u32))) as *mut u16, r3 as u16);
                    r3 = 540u32;
                    r3 = read_volatile((r4.wrapping_add(r3)) as *const u16) as u32;
                    write_volatile((r11.wrapping_add((-22i32 as u32))) as *mut u16, r3 as u16);
                    write_volatile((r11.wrapping_add((-0x14i32 as u32))) as *mut u32, r5 as u32);
                    r3 = read_volatile((r11.wrapping_add((-0x60i32 as u32))) as *const u32) as u32;
                    write_volatile((r11.wrapping_add((-0x10i32 as u32))) as *mut u32, r3 as u32);
                    r0 = read_volatile((r4.wrapping_add(0x220u32)) as *const u32) as u32;
                    r1 = r11.wrapping_sub(32u32);
                    xm1_power_record_helper_756e5c(r0, r1 as *const ProfileWords5);
                    r0 = xm1_power_record_helper_fa8018();
                    equal = r0 == r5; lower_or_same = r0 <= r5; lower = r0 < r5;
                    if !equal { next = Flow::B4; } else { next = Flow::B3; }
                    continue;
                }
                Flow::B3 => {
                    r0 = read_volatile((r4.wrapping_add(0x220u32)) as *const u32) as u32;
                    xm1_power_record_helper_fa80f4(r0);
                    next = Flow::B4;
                }
                Flow::B4 => {
                    sp = r11.wrapping_sub(12u32);
                    return;
                }
            }
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_f37e90() {
    enum Flow { B0, B1, B2, B3 }
    let mut lr: u32 = 0;
    let mut r0: u32 = 0;
    let mut r1: u32 = 0;
    let mut r2: u32 = 0;
    let mut r3: u32 = 0;
    let mut r4: u32 = 0;
    let mut r12: u32 = 0;
    let mut equal = false;
    let mut lower_or_same = false;
    let mut lower = false;
    let mut next = Flow::B0;
    unsafe {
        loop {
            match next {
                Flow::B0 => {
                    r4 = 0xfe54u32;
                    r4 = (r4 & 0xffff) | (0x16bu32 << 16);
                    r3 = 0u32;
                    write_volatile((r4) as *mut u8, r3 as u8);
                    write_volatile((r4.wrapping_add(0x18u32)) as *mut u8, r3 as u8);
                    r0 = 0x82b0u32;
                    r0 = xm1_power_configuration_byte(r0);
                    equal = r0 == 64u32; lower_or_same = r0 <= 64u32; lower = r0 < 64u32;
                    if !lower { r0 = 64u32; }
                    r12 = r0 & 0xff;
                    write_volatile((r4.wrapping_add(0x19u32)) as *mut u8, r12 as u8);
                    r2 = read_volatile((r4.wrapping_add(0xcu32)) as *const u32) as u32;
                    r3 = 0x851fu32;
                    r3 = (r3 & 0xffff) | (0x51ebu32 << 16);
                    { let product = (r3 as u64).wrapping_mul(r2 as u64); r0 = product as u32; r2 = (product >> 32) as u32; }
                    r2 = (r2 >> 0x5u32) & 65535u32;
                    r1 = read_volatile((r4.wrapping_add(0x10u32)) as *const u32) as u32;
                    { let product = (r3 as u64).wrapping_mul(r1 as u64); lr = product as u32; r1 = (product >> 32) as u32; }
                    r1 = (r1 >> 0x5u32) & 65535u32;
                    r0 = read_volatile((r4.wrapping_add(0x14u32)) as *const u32) as u32;
                    { let product = (r3 as u64).wrapping_mul(r0 as u64); lr = product as u32; r0 = (product >> 32) as u32; }
                    r0 = (r0 >> 0x5u32) & 65535u32;
                    equal = r12 == 0u32; lower_or_same = r12 <= 0u32; lower = r12 < 0u32;
                    if equal { next = Flow::B3; } else { next = Flow::B1; }
                    continue;
                }
                Flow::B1 => {
                    r3 = r4.wrapping_add(26u32);
                    r12 = r12.wrapping_sub(1u32);
                    r12 = r12 & 0xffff;
                    lr = r3.wrapping_add(2u32);
                    r12 = lr.wrapping_add(r12.wrapping_shl(1u32));
                    next = Flow::B2;
                }
                Flow::B2 => {
                    write_volatile((r3) as *mut u16, r2 as u16);r3 = r3.wrapping_add(2u32);
                    write_volatile((r3.wrapping_add(126u32)) as *mut u16, r1 as u16);
                    write_volatile((r3.wrapping_add(254u32)) as *mut u16, r0 as u16);
                    equal = r3 == r12; lower_or_same = r3 <= r12; lower = r3 < r12;
                    if !equal { next = Flow::B2; } else { next = Flow::B3; }
                    continue;
                }
                Flow::B3 => {
                    r4 = 0xfe54u32;
                    r4 = (r4 & 0xffff) | (0x16bu32 << 16);
                    r0 = read_volatile((r4.wrapping_add(0x220u32)) as *const u32) as u32;
                    r0 = xm1_power_record_helper_756b0c(r0);
                    r1 = r0 | 4u32;
                    r0 = read_volatile((r4.wrapping_add(0x220u32)) as *const u32) as u32;
                    xm1_power_record_helper_756b2c(r0, r1);
                    return;
                }
            }
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_f37f40() {
    enum Flow { B0, B1, B2, B3, B4, B5 }
    let mut frame = core::mem::MaybeUninit::<[u32; 14]>::uninit();
    let mut sp = (frame.as_mut_ptr() as usize as u32).wrapping_add(56);
    let mut lr: u32 = 0;
    let mut r0: u32 = 0;
    let mut r1: u32 = 0;
    let mut r2: u32 = 0;
    let mut r3: u32 = 0;
    let mut r11: u32 = 0;
    let mut r12: u32 = 0;
    let mut equal = false;
    let mut lower_or_same = false;
    let mut lower = false;
    let mut next = Flow::B0;
    unsafe {
        loop {
            match next {
                Flow::B0 => {
                    sp = sp.wrapping_sub(8);
                    r11 = sp.wrapping_add(4u32);
                    sp = sp.wrapping_sub(48u32);
                    r0 = xm1_power_record_helper_f378f0();
                    r3 = 0xfe54u32;
                    r3 = (r3 & 0xffff) | (0x16bu32 << 16);
                    r0 = r11.wrapping_sub(52u32);
                    r1 = read_volatile((r3.wrapping_add(0x220u32)) as *const u32) as u32;
                    r0 = xm1_power_record_helper_756e24(r0 as *mut ProfileWords5) as usize as u32;
                    r12 = r11.wrapping_sub(24u32);
                    lr = r11.wrapping_sub(52u32);
                    { let transfer = lr; r0 = read_volatile(transfer.wrapping_add(0) as *const u32); r1 = read_volatile(transfer.wrapping_add(4) as *const u32); r2 = read_volatile(transfer.wrapping_add(8) as *const u32); r3 = read_volatile(transfer.wrapping_add(12) as *const u32); lr = transfer.wrapping_add(16); }
                    { let transfer = r12; write_volatile(transfer.wrapping_add(0) as *mut u32, r0); write_volatile(transfer.wrapping_add(4) as *mut u32, r1); write_volatile(transfer.wrapping_add(8) as *mut u32, r2); write_volatile(transfer.wrapping_add(12) as *mut u32, r3); r12 = transfer.wrapping_add(16); }
                    r3 = read_volatile((lr) as *const u32) as u32;
                    write_volatile((r12) as *mut u32, r3 as u32);
                    r0 = 0x82c1u32;
                    r0 = xm1_power_configuration_byte(r0);
                    equal = r0 == 0u32; lower_or_same = r0 <= 0u32; lower = r0 < 0u32;
                    if equal { next = Flow::B3; } else { next = Flow::B1; }
                    continue;
                }
                Flow::B1 => {
                    r3 = read_volatile((r11.wrapping_add((-0x8i32 as u32))) as *const u32) as u32;
                    equal = r3 == 0u32; lower_or_same = r3 <= 0u32; lower = r3 < 0u32;
                    if !equal { next = Flow::B3; } else { next = Flow::B2; }
                    continue;
                }
                Flow::B2 => {
                    r12 = read_volatile((r11.wrapping_add((-16i32 as u32))) as *const u16) as u32;
                    write_volatile((r11.wrapping_add((-22i32 as u32))) as *mut u16, r12 as u16);
                    r0 = read_volatile((r11.wrapping_add((-18i32 as u32))) as *const u16) as u32;
                    write_volatile((r11.wrapping_add((-24i32 as u32))) as *mut u16, r0 as u16);
                    r1 = read_volatile((r11.wrapping_add((-14i32 as u32))) as *const u16) as u32;
                    write_volatile((r11.wrapping_add((-20i32 as u32))) as *mut u16, r1 as u16);
                    r3 = 0xfe54u32;
                    r3 = (r3 & 0xffff) | (0x16bu32 << 16);
                    r2 = 100u32;
                    r12 = r2.wrapping_mul(r12);
                    write_volatile((r3.wrapping_add(0x10u32)) as *mut u32, r12 as u32);
                    r0 = r2.wrapping_mul(r0);
                    write_volatile((r3.wrapping_add(0xcu32)) as *mut u32, r0 as u32);
                    r2 = r2.wrapping_mul(r1);
                    write_volatile((r3.wrapping_add(0x14u32)) as *mut u32, r2 as u32);
                    r0 = read_volatile((r3.wrapping_add(0x220u32)) as *const u32) as u32;
                    r1 = r11.wrapping_sub(24u32);
                    xm1_power_record_helper_756e5c(r0, r1 as *const ProfileWords5);
                    next = Flow::B3;
                }
                Flow::B3 => {
                    r3 = 0xfe54u32;
                    r3 = (r3 & 0xffff) | (0x16bu32 << 16);
                    r0 = read_volatile((r3.wrapping_add(0x220u32)) as *const u32) as u32;
                    r0 = xm1_power_record_helper_756b54(r0);
                    equal = r0 == 0u32; lower_or_same = r0 <= 0u32; lower = r0 < 0u32;
                    if equal { next = Flow::B5; } else { next = Flow::B4; }
                    continue;
                }
                Flow::B4 => {
                    r3 = 0xfe54u32;
                    r3 = (r3 & 0xffff) | (0x16bu32 << 16);
                    r2 = read_volatile((r3.wrapping_add(0x18u32)) as *const u8) as u32;
                    r0 = r0.wrapping_add(638976u32);
                    r0 = r0.wrapping_add(3792u32);
                    r1 = r3.wrapping_add(r2.wrapping_shl(1u32));
                    r12 = read_volatile((r1.wrapping_add(26u32)) as *const u16) as u32;
                    r1 = 0x1710u32;
                    write_volatile((r0.wrapping_add(r1)) as *mut u32, r12 as u32);
                    r1 = r3.wrapping_add(r2.wrapping_shl(1u32));
                    r12 = read_volatile((r1.wrapping_add(154u32)) as *const u16) as u32;
                    r1 = 0x1714u32;
                    write_volatile((r0.wrapping_add(r1)) as *mut u32, r12 as u32);
                    r2 = r2.wrapping_add(140u32);
                    r3 = r3.wrapping_add(r2.wrapping_shl(1u32));
                    r2 = read_volatile((r3.wrapping_add(2u32)) as *const u16) as u32;
                    r3 = 0x1718u32;
                    write_volatile((r0.wrapping_add(r3)) as *mut u32, r2 as u32);
                    next = Flow::B5;
                }
                Flow::B5 => {
                    sp = r11.wrapping_sub(4u32);
                    return;
                }
            }
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_f38048() {
    enum Flow { B0, B1, B2, B3, B4 }
    let mut r0: u32 = 0;
    let mut r1: u32 = 0;
    let mut r2: u32 = 0;
    let mut r3: u32 = 0;
    let mut r4: u32 = 0;
    let mut equal = false;
    let mut lower_or_same = false;
    let mut lower = false;
    let mut next = Flow::B0;
    unsafe {
        loop {
            match next {
                Flow::B0 => {
                    r3 = 0xfe54u32;
                    r3 = (r3 & 0xffff) | (0x16bu32 << 16);
                    r2 = read_volatile((r3) as *const u8) as u32;
                    r2 = r2.wrapping_add(1u32);
                    r2 = r2 & 0xff;
                    write_volatile((r3) as *mut u8, r2 as u8);
                    equal = r2 == 3u32; lower_or_same = r2 <= 3u32; lower = r2 < 3u32;
                    if lower_or_same { next = Flow::B2; } else { next = Flow::B1; }
                    continue;
                }
                Flow::B1 => {
                    r3 = 0xfe54u32;
                    r3 = (r3 & 0xffff) | (0x16bu32 << 16);
                    r2 = 1u32;
                    write_volatile((r3) as *mut u8, r2 as u8);
                    next = Flow::B3;
                    continue;
                }
                Flow::B2 => {
                    equal = r2 == 1u32; lower_or_same = r2 <= 1u32; lower = r2 < 1u32;
                    if !equal { r3 = 0xfe54u32; }
                    if !equal { r3 = (r3 & 0xffff) | (0x16bu32 << 16); }
                    if !equal { write_volatile((r3.wrapping_add(0x1a0u32)) as *mut u8, r2 as u8); }
                    if !equal { next = Flow::B4; } else { next = Flow::B3; }
                    continue;
                }
                Flow::B3 => {
                    r4 = 23855088u32;
                    r0 = r4;
                    r1 = 0u32;
                    r2 = 124u32;
                    core::ptr::write_bytes(r0 as *mut u8, r1 as u8, r2 as usize);
                    r0 = r4;
                    xm1_power_record_helper_f37ab8(r0);
                    r3 = 0xfe54u32;
                    r3 = (r3 & 0xffff) | (0x16bu32 << 16);
                    r2 = read_volatile((r3.wrapping_add(0x19cu32)) as *const u32) as u32;
                    write_volatile((r3.wrapping_add(0x8u32)) as *mut u32, r2 as u32);
                    equal = r2 == 0u32; lower_or_same = r2 <= 0u32; lower = r2 < 0u32;
                    if equal { r3 = 0xfe54u32; }
                    if equal { r3 = (r3 & 0xffff) | (0x16bu32 << 16); }
                    if equal { write_volatile((r3) as *mut u8, r2 as u8); }
                    if equal { return; }
                    next = Flow::B4;
                }
                Flow::B4 => {
                    r3 = 0xfe54u32;
                    r3 = (r3 & 0xffff) | (0x16bu32 << 16);
                    r3 = read_volatile((r3) as *const u8) as u32;
                    equal = r3 == 1u32; lower_or_same = r3 <= 1u32; lower = r3 < 1u32;
                    if !equal { return; }
                    r4 = 0xfe54u32;
                    r4 = (r4 & 0xffff) | (0x16bu32 << 16);
                    r0 = read_volatile((r4.wrapping_add(0x220u32)) as *const u32) as u32;
                    r0 = xm1_power_record_helper_756b0c(r0);
                    r1 = r0 & !4u32;
                    r0 = read_volatile((r4.wrapping_add(0x220u32)) as *const u32) as u32;
                    xm1_power_record_helper_756b2c(r0, r1);
                    return;
                }
            }
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_f38110() -> u32 {
    enum Flow { B0, B1, B2, B3, B4, B5, B6, B7, B8, B9, B10, B11, B12, B13, B14, B15, B16, B17, B18, B19, B20, B21, B22, B23, B24, B25, B26 }
    let mut frame = core::mem::MaybeUninit::<[u32; 12]>::uninit();
    let mut sp = (frame.as_mut_ptr() as usize as u32).wrapping_add(48);
    let mut lr: u32 = 0;
    let mut r0: u32 = 0;
    let mut r1: u32 = 0;
    let mut r2: u32 = 0;
    let mut r3: u32 = 0;
    let mut r4: u32 = 0;
    let mut r5: u32 = 0;
    let mut r6: u32 = 0;
    let mut r7: u32 = 0;
    let mut r8: u32 = 0;
    let mut r9: u32 = 0;
    let mut r10: u32 = 0;
    let mut r11: u32 = 0;
    let mut r12: u32 = 0;
    let mut equal = false;
    let mut lower_or_same = false;
    let mut lower = false;
    let mut next = Flow::B0;
    unsafe {
        loop {
            match next {
                Flow::B0 => {
                    sp = sp.wrapping_sub(36);
                    r11 = sp.wrapping_add(32u32);
                    sp = sp.wrapping_sub(12u32);
                    r3 = 0xd370u32;
                    r3 = (r3 & 0xffff) | (0x169u32 << 16);
                    r5 = read_volatile((r3.wrapping_add(0x14u32)) as *const u32) as u32;
                    equal = r5 == 0u32; lower_or_same = r5 <= 0u32; lower = r5 < 0u32;
                    if equal { r0 = 0u32; }
                    if equal { next = Flow::B26; } else { next = Flow::B1; }
                    continue;
                }
                Flow::B1 => {
                    r3 = 0xfe54u32;
                    r3 = (r3 & 0xffff) | (0x16bu32 << 16);
                    r3 = read_volatile((r3) as *const u8) as u32;
                    equal = r3 == 3u32; lower_or_same = r3 <= 3u32; lower = r3 < 3u32;
                    if !equal { next = Flow::B23; } else { next = Flow::B2; }
                    continue;
                }
                Flow::B2 => {
                    r4 = 0xfe54u32;
                    r4 = (r4 & 0xffff) | (0x16bu32 << 16);
                    r0 = read_volatile((r4.wrapping_add(0x220u32)) as *const u32) as u32;
                    r0 = xm1_power_record_helper_756b0c(r0);
                    r1 = r0 | 4u32;
                    r0 = read_volatile((r4.wrapping_add(0x220u32)) as *const u32) as u32;
                    xm1_power_record_helper_756b2c(r0, r1);
                    r3 = read_volatile((r4) as *const u8) as u32;
                    equal = r3 == 3u32; lower_or_same = r3 <= 3u32; lower = r3 < 3u32;
                    if !equal { next = Flow::B23; } else { next = Flow::B3; }
                    continue;
                }
                Flow::B3 => {
                    r3 = r4;
                    r2 = read_volatile((r4.wrapping_add(0x1u32)) as *const u8) as u32;
                    r2 = r2.wrapping_add(1u32);
                    r2 = r2 & 0xff;
                    write_volatile((r4.wrapping_add(0x1u32)) as *mut u8, r2 as u8);
                    equal = r2 == 255u32; lower_or_same = r2 <= 255u32; lower = r2 < 255u32;
                    if equal { r3 = 0xfe54u32; }
                    if equal { r3 = r4; }
                    if equal { r2 = !1u32; }
                    if equal { write_volatile((r3.wrapping_add(0x1u32)) as *mut u8, r2 as u8); }
                    r3 = 0xfe54u32;
                    r3 = (r3 & 0xffff) | (0x16bu32 << 16);
                    r3 = read_volatile((r3.wrapping_add(0x8u32)) as *const u32) as u32;
                    equal = r3 == 0u32; lower_or_same = r3 <= 0u32; lower = r3 < 0u32;
                    if equal { next = Flow::B6; } else { next = Flow::B4; }
                    continue;
                }
                Flow::B4 => {
                    r3 = read_volatile((r5.wrapping_add(0x8u32)) as *const u32) as u32;
                    equal = r3 == 0u32; lower_or_same = r3 <= 0u32; lower = r3 < 0u32;
                    if !equal { next = Flow::B6; } else { next = Flow::B5; }
                    continue;
                }
                Flow::B5 => {
                    r3 = 0xfe54u32;
                    r3 = (r3 & 0xffff) | (0x16bu32 << 16);
                    r2 = read_volatile((r3.wrapping_add(0x18u32)) as *const u8) as u32;
                    r2 = r2.wrapping_add(1u32);
                    r2 = r2 & 0xff;
                    write_volatile((r3.wrapping_add(0x18u32)) as *mut u8, r2 as u8);
                    r3 = read_volatile((r3.wrapping_add(0x19u32)) as *const u8) as u32;
                    equal = r3 == r2; lower_or_same = r3 <= r2; lower = r3 < r2;
                    if lower_or_same { r3 = 0xfe54u32; }
                    if lower_or_same { r3 = (r3 & 0xffff) | (0x16bu32 << 16); }
                    if lower_or_same { r2 = 0u32; }
                    if lower_or_same { write_volatile((r3.wrapping_add(0x18u32)) as *mut u8, r2 as u8); }
                    r3 = 0xfe54u32;
                    r3 = (r3 & 0xffff) | (0x16bu32 << 16);
                    r2 = read_volatile((r3.wrapping_add(0x18u32)) as *const u8) as u32;
                    r0 = read_volatile((r5.wrapping_add(2u32)) as *const u16) as u32;
                    r1 = r3.wrapping_add(r2.wrapping_shl(1u32));
                    write_volatile((r1.wrapping_add(26u32)) as *mut u16, r0 as u16);
                    r0 = read_volatile((r5.wrapping_add(4u32)) as *const u16) as u32;
                    write_volatile((r1.wrapping_add(154u32)) as *mut u16, r0 as u16);
                    r1 = read_volatile((r5.wrapping_add(6u32)) as *const u16) as u32;
                    r2 = r2.wrapping_add(140u32);
                    r3 = r3.wrapping_add(r2.wrapping_shl(1u32));
                    write_volatile((r3.wrapping_add(2u32)) as *mut u16, r1 as u16);
                    next = Flow::B6;
                }
                Flow::B6 => {
                    r3 = 0xfe54u32;
                    r3 = (r3 & 0xffff) | (0x16bu32 << 16);
                    r1 = read_volatile((r3.wrapping_add(0x19u32)) as *const u8) as u32;
                    equal = r1 == 0u32; lower_or_same = r1 <= 0u32; lower = r1 < 0u32;
                    if equal { r4 = 0u32; }
                    if equal { r6 = r4; }
                    if equal { r7 = r4; }
                    if equal { next = Flow::B9; } else { next = Flow::B7; }
                    continue;
                }
                Flow::B7 => {
                    r3 = r3.wrapping_add(26u32);
                    r1 = r1.wrapping_sub(1u32);
                    r1 = r1 & 0xff;
                    r2 = r3.wrapping_add(2u32);
                    r1 = r2.wrapping_add(r1.wrapping_shl(1u32));
                    r4 = 0u32;
                    r6 = r4;
                    r7 = r4;
                    next = Flow::B8;
                }
                Flow::B8 => {
                    r2 = read_volatile((r3) as *const u16) as u32;r3 = r3.wrapping_add(2u32);
                    r7 = r7.wrapping_add(r2);
                    r2 = read_volatile((r3.wrapping_add(126u32)) as *const u16) as u32;
                    r6 = r6.wrapping_add(r2);
                    r2 = read_volatile((r3.wrapping_add(254u32)) as *const u16) as u32;
                    r4 = r4.wrapping_add(r2);
                    equal = r3 == r1; lower_or_same = r3 <= r1; lower = r3 < r1;
                    if !equal { next = Flow::B8; } else { next = Flow::B9; }
                    continue;
                }
                Flow::B9 => {
                    r0 = 0x82b1u32;
                    r0 = xm1_power_configuration_byte(r0);
                    r8 = 0xfe54u32;
                    r8 = (r8 & 0xffff) | (0x16bu32 << 16);
                    r3 = read_volatile((r8.wrapping_add(0x18u32)) as *const u8) as u32;
                    r2 = r8.wrapping_add(r3.wrapping_shl(1u32));
                    r2 = read_volatile((r2.wrapping_add(26u32)) as *const u16) as u32;
                    r1 = r8.wrapping_add(r3.wrapping_shl(1u32));
                    r10 = read_volatile((r1.wrapping_add(154u32)) as *const u16) as u32;
                    r10 = r10.wrapping_mul(r0).wrapping_add(r6);
                    r3 = r3.wrapping_add(140u32);
                    r3 = r8.wrapping_add(r3.wrapping_shl(1u32));
                    r3 = read_volatile((r3.wrapping_add(2u32)) as *const u16) as u32;
                    r3 = r3.wrapping_mul(r0).wrapping_add(r4);
                    write_volatile((r11.wrapping_add((-0x28i32 as u32))) as *mut u32, r3 as u32);
                    r4 = read_volatile((r8.wrapping_add(0x19u32)) as *const u8) as u32;
                    r4 = r4.wrapping_add(r0);
                    r0 = r2.wrapping_mul(r0).wrapping_add(r7);
                    r1 = r4;
                    r0 = __aeabi_uidiv(r0, r1);
                    r6 = r0;
                    r0 = r10;
                    r1 = r4;
                    r0 = __aeabi_uidiv(r0, r1);
                    r9 = r0;
                    r0 = read_volatile((r11.wrapping_add((-0x28i32 as u32))) as *const u32) as u32;
                    r1 = r4;
                    r0 = __aeabi_uidiv(r0, r1);
                    r10 = r0;
                    r4 = read_volatile((r8.wrapping_add(0xcu32)) as *const u32) as u32;
                    r1 = read_volatile((r8.wrapping_add(0x10u32)) as *const u32) as u32;
                    r1 = r1.wrapping_mul(r6);
                    r7 = 0u32;
                    r0 = r1.wrapping_shl(7u32);
                    r1 = r1.wrapping_shr(25u32);
                    r2 = r4.wrapping_mul(r9);
                    r3 = r7;
                    { let quotient = __aeabi_uldivmod((r0 as u64) | ((r1 as u64) << 32), (r2 as u64) | ((r3 as u64) << 32)); r0 = quotient as u32; r1 = (quotient >> 32) as u32; }
                    write_volatile((r11.wrapping_add((-0x28i32 as u32))) as *mut u32, r0 as u32);
                    r1 = read_volatile((r8.wrapping_add(0x14u32)) as *const u32) as u32;
                    r1 = r1.wrapping_mul(r6);
                    r0 = r1.wrapping_shl(7u32);
                    r1 = r1.wrapping_shr(25u32);
                    r2 = r4.wrapping_mul(r10);
                    r3 = r7;
                    { let quotient = __aeabi_uldivmod((r0 as u64) | ((r1 as u64) << 32), (r2 as u64) | ((r3 as u64) << 32)); r0 = quotient as u32; r1 = (quotient >> 32) as u32; }
                    r4 = r0;
                    r3 = read_volatile((r8.wrapping_add(0x4u32)) as *const u32) as u32;
                    equal = r3 == r7; lower_or_same = r3 <= r7; lower = r3 < r7;
                    if !equal { next = Flow::B16; } else { next = Flow::B10; }
                    continue;
                }
                Flow::B10 => {
                    r0 = 0x82b6u32;
                    r0 = xm1_power_configuration_byte(r0);
                    r7 = read_volatile((r11.wrapping_add((-0x28i32 as u32))) as *const u32) as u32;
                    r0 = r0.wrapping_add(r7);
                    equal = r0 == 127u32; lower_or_same = r0 <= 127u32; lower = r0 < 127u32;
                    if lower_or_same { next = Flow::B14; } else { next = Flow::B11; }
                    continue;
                }
                Flow::B11 => {
                    r0 = 0x82b6u32;
                    r0 = xm1_power_configuration_byte(r0);
                    r0 = r0.wrapping_add(128u32);
                    equal = r7 == r0; lower_or_same = r7 <= r0; lower = r7 < r0;
                    if !lower_or_same { next = Flow::B14; } else { next = Flow::B12; }
                    continue;
                }
                Flow::B12 => {
                    r0 = 0x82b6u32;
                    r0 = xm1_power_configuration_byte(r0);
                    r0 = r0.wrapping_add(r4);
                    equal = r0 == 127u32; lower_or_same = r0 <= 127u32; lower = r0 < 127u32;
                    if lower_or_same { next = Flow::B14; } else { next = Flow::B13; }
                    continue;
                }
                Flow::B13 => {
                    r0 = 0x82b6u32;
                    r0 = xm1_power_configuration_byte(r0);
                    r0 = r0.wrapping_add(128u32);
                    equal = r4 == r0; lower_or_same = r4 <= r0; lower = r4 < r0;
                    if lower_or_same { next = Flow::B15; } else { next = Flow::B14; }
                    continue;
                }
                Flow::B14 => {
                    r0 = xm1_power_record_helper_f3789c();
                    r3 = 0xfe54u32;
                    r3 = (r3 & 0xffff) | (0x16bu32 << 16);
                    r3 = read_volatile((r3.wrapping_add(0x2u32)) as *const u8) as u32;
                    equal = r0 == r3; lower_or_same = r0 <= r3; lower = r0 < r3;
                    if !lower_or_same { r2 = 0xfe54u32; }
                    if !lower_or_same { r2 = (r2 & 0xffff) | (0x16bu32 << 16); }
                    if !lower_or_same { r3 = r3.wrapping_add(1u32); }
                    if !lower_or_same { write_volatile((r2.wrapping_add(0x2u32)) as *mut u8, r3 as u8); }
                    if lower_or_same { r3 = 0xfe54u32; }
                    if lower_or_same { r3 = (r3 & 0xffff) | (0x16bu32 << 16); }
                    if lower_or_same { r2 = 1u32; }
                    if lower_or_same { write_volatile((r3.wrapping_add(0x4u32)) as *mut u32, r2 as u32); }
                    next = Flow::B22;
                    continue;
                }
                Flow::B15 => {
                    r3 = 0xfe54u32;
                    r3 = (r3 & 0xffff) | (0x16bu32 << 16);
                    r2 = 0u32;
                    write_volatile((r3.wrapping_add(0x2u32)) as *mut u8, r2 as u8);
                    next = Flow::B22;
                    continue;
                }
                Flow::B16 => {
                    r0 = 0x82b5u32;
                    r0 = xm1_power_configuration_byte(r0);
                    r7 = read_volatile((r11.wrapping_add((-0x28i32 as u32))) as *const u32) as u32;
                    r0 = r0.wrapping_add(r7);
                    equal = r0 == 127u32; lower_or_same = r0 <= 127u32; lower = r0 < 127u32;
                    if lower_or_same { next = Flow::B21; } else { next = Flow::B17; }
                    continue;
                }
                Flow::B17 => {
                    r0 = 0x82b5u32;
                    r0 = xm1_power_configuration_byte(r0);
                    r0 = r0.wrapping_add(128u32);
                    equal = r7 == r0; lower_or_same = r7 <= r0; lower = r7 < r0;
                    if !lower_or_same { next = Flow::B21; } else { next = Flow::B18; }
                    continue;
                }
                Flow::B18 => {
                    r0 = 0x82b5u32;
                    r0 = xm1_power_configuration_byte(r0);
                    r0 = r0.wrapping_add(r4);
                    equal = r0 == 127u32; lower_or_same = r0 <= 127u32; lower = r0 < 127u32;
                    if lower_or_same { next = Flow::B21; } else { next = Flow::B19; }
                    continue;
                }
                Flow::B19 => {
                    r0 = 0x82b5u32;
                    r0 = xm1_power_configuration_byte(r0);
                    r0 = r0.wrapping_add(128u32);
                    equal = r4 == r0; lower_or_same = r4 <= r0; lower = r4 < r0;
                    if !lower_or_same { next = Flow::B21; } else { next = Flow::B20; }
                    continue;
                }
                Flow::B20 => {
                    r3 = 23854676u32;
                    r4 = read_volatile((r3.wrapping_add(0x1u32)) as *const u8) as u32;
                    r0 = 0x82b4u32;
                    r0 = xm1_power_configuration_byte(r0);
                    equal = r4 == r0; lower_or_same = r4 <= r0; lower = r4 < r0;
                    if !lower { r3 = 23854676u32; }
                    if !lower { r2 = 0u32; }
                    if !lower { write_volatile((r3.wrapping_add(0x2u32)) as *mut u8, r2 as u8); }
                    if !lower { write_volatile((r3.wrapping_add(0x4u32)) as *mut u32, r2 as u32); }
                    next = Flow::B22;
                    continue;
                }
                Flow::B21 => {
                    r3 = 0xfe54u32;
                    r3 = (r3 & 0xffff) | (0x16bu32 << 16);
                    r4 = read_volatile((r3.wrapping_add(0x1u32)) as *const u8) as u32;
                    r0 = 0x82b4u32;
                    r0 = xm1_power_configuration_byte(r0);
                    equal = r4 == r0; lower_or_same = r4 <= r0; lower = r4 < r0;
                    if lower { r0 = 0x82b3u32; }
                    if !lower { r0 = 0x82b2u32; }
                    r0 = xm1_power_configuration_byte(r0);
                    r3 = 0xfe54u32;
                    r3 = (r3 & 0xffff) | (0x16bu32 << 16);
                    r12 = read_volatile((r3.wrapping_add(0xcu32)) as *const u32) as u32;
                    r1 = 100u32;
                    lr = r1.wrapping_mul(r6);
                    lr = lr.wrapping_sub(r12);
                    lr = r0.wrapping_mul(lr);
                    r2 = 0x851fu32;
                    r2 = (r2 & 0xffff) | (0x51ebu32 << 16);
                    { let product = (r2 as i32 as i64).wrapping_mul(lr as i32 as i64) as u64; r7 = product as u32; r4 = (product >> 32) as u32; }
                    lr = (lr as i32).wrapping_shr(31u32) as u32;
                    lr = ((r4 as i32).wrapping_shr(5u32) as u32).wrapping_sub(lr);
                    r12 = lr.wrapping_add(r12);
                    write_volatile((r3.wrapping_add(0xcu32)) as *mut u32, r12 as u32);
                    r12 = read_volatile((r3.wrapping_add(0x10u32)) as *const u32) as u32;
                    lr = r1.wrapping_mul(r9);
                    lr = lr.wrapping_sub(r12);
                    lr = lr.wrapping_mul(r0);
                    { let product = (r2 as i32 as i64).wrapping_mul(lr as i32 as i64) as u64; r7 = product as u32; r4 = (product >> 32) as u32; }
                    lr = (lr as i32).wrapping_shr(31u32) as u32;
                    lr = ((r4 as i32).wrapping_shr(5u32) as u32).wrapping_sub(lr);
                    r12 = lr.wrapping_add(r12);
                    write_volatile((r3.wrapping_add(0x10u32)) as *mut u32, r12 as u32);
                    r12 = read_volatile((r3.wrapping_add(0x14u32)) as *const u32) as u32;
                    r1 = r1.wrapping_mul(r10);
                    r1 = r1.wrapping_sub(r12);
                    r0 = r1.wrapping_mul(r0);
                    { let product = (r2 as i32 as i64).wrapping_mul(r0 as i32 as i64) as u64; r1 = product as u32; r2 = (product >> 32) as u32; }
                    r0 = (r0 as i32).wrapping_shr(31u32) as u32;
                    r2 = ((r2 as i32).wrapping_shr(5u32) as u32).wrapping_sub(r0);
                    r12 = r2.wrapping_add(r12);
                    write_volatile((r3.wrapping_add(0x14u32)) as *mut u32, r12 as u32);
                    next = Flow::B22;
                }
                Flow::B22 => {
                    r3 = 0xfe54u32;
                    r3 = (r3 & 0xffff) | (0x16bu32 << 16);
                    r2 = 536u32;
                    write_volatile((r3.wrapping_add(r2)) as *mut u16, r6 as u16);
                    r2 = r2.wrapping_add(2u32);
                    write_volatile((r3.wrapping_add(r2)) as *mut u16, r9 as u16);
                    r2 = 540u32;
                    write_volatile((r3.wrapping_add(r2)) as *mut u16, r10 as u16);
                    next = Flow::B23;
                }
                Flow::B23 => {
                    r4 = 0xfe54u32;
                    r4 = (r4 & 0xffff) | (0x16bu32 << 16);
                    r0 = read_volatile((r4.wrapping_add(0x220u32)) as *const u32) as u32;
                    r1 = read_volatile((r5) as *const i16) as i32 as u32;
                    xm1_power_record_helper_75784c(r0, r1);
                    r0 = xm1_power_record_helper_f378f0();
                    r6 = r0;
                    r0 = read_volatile((r4.wrapping_add(0x220u32)) as *const u32) as u32;
                    r0 = xm1_power_record_helper_7503ec(r0);
                    equal = r0 == 1u32; lower_or_same = r0 <= 1u32; lower = r0 < 1u32;
                    if !equal { next = Flow::B25; } else { next = Flow::B24; }
                    continue;
                }
                Flow::B24 => {
                    r0 = read_volatile((r4.wrapping_add(0x220u32)) as *const u32) as u32;
                    r1 = 0u32;
                    r2 = r1;
                    r3 = r1;
                    xm1_power_record_helper_f4f838(r0, r1, r2, r3);
                    next = Flow::B25;
                }
                Flow::B25 => {
                    r0 = r5;
                    r1 = 12u32;
                    xm1_power_record_helper_740e74(r0, r1);
                    r0 = r6;
                    next = Flow::B26;
                }
                Flow::B26 => {
                    sp = r11.wrapping_sub(32u32);
                    return r0;
                }
            }
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_f3857c() {
    enum Flow { B0, B1, B2 }
    let mut r0: u32 = 0;
    let mut r1: u32 = 0;
    let mut equal = false;
    let mut lower_or_same = false;
    let mut lower = false;
    let mut next = Flow::B0;
    unsafe {
        loop {
            match next {
                Flow::B0 => {
                    r0 = 0x24eu32;
                    r0 = xm1_power_configuration_byte(r0);
                    equal = r0 == 1u32; lower_or_same = r0 <= 1u32; lower = r0 < 1u32;
                    if !equal { next = Flow::B2; } else { next = Flow::B1; }
                    continue;
                }
                Flow::B1 => {
                    r0 = 16384u32;
                    r0 = (r0 & 0xffff) | (0x110u32 << 16);
                    xm1_power_recording_capture_trace_begin(r0);
                    next = Flow::B2;
                }
                Flow::B2 => {
                    r0 = 0x7854u32;
                    r0 = xm1_power_record_helper_f37854 as usize as u32;
                    r1 = 23855088u32;
                    r0 = xm1_power_record_helper_f4c490(r0, r1);
                    return;
                }
            }
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_f3789c() -> u32 {
    let mut frame = core::mem::MaybeUninit::<[u32; 374]>::uninit();
    let mut sp = (frame.as_mut_ptr() as usize as u32).wrapping_add(1496);
    let mut r0: u32 = 0;
    let mut r1: u32 = 0;
    let mut r2: u32 = 0;
    let mut r4: u32 = 0;
    let mut r5: u32 = 0;
    let mut r11: u32 = 0;
    unsafe {
        sp = sp.wrapping_sub(16);
        r11 = sp.wrapping_add(12u32);
        sp = sp.wrapping_sub(1472u32);
        sp = sp.wrapping_sub(8u32);
        r4 = 0xfe54u32;
        r4 = (r4 & 0xffff) | (0x16bu32 << 16);
        r0 = read_volatile((r4.wrapping_add(0x220u32)) as *const u32) as u32;
        r0 = xm1_power_record_helper_74e5c0(r0);
        r5 = r0;
        r0 = read_volatile((r4.wrapping_add(0x220u32)) as *const u32) as u32;
        r0 = xm1_power_record_helper_74eba8(r0);
        r4 = r0;
        r0 = 2u32;
        r1 = r11.wrapping_sub(1488u32);
        xm1_power_record_helper_fea7ac(r0, r1 as *mut u8);
        r0 = r5;
        r1 = r4;
        r2 = read_volatile((r11.wrapping_add((-0x4e4i32 as u32))) as *const u32) as u32;
        r0 = xm1_power_record_helper_fa8078(r0, r1, r2);
        sp = r11.wrapping_sub(12u32);
        return r0;
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_f378f0() -> u32 {
    enum Flow { B0, B1, B2 }
    let mut frame = core::mem::MaybeUninit::<[u32; 38]>::uninit();
    let mut sp = (frame.as_mut_ptr() as usize as u32).wrapping_add(152);
    let mut lr: u32 = 0;
    let mut r0: u32 = 0;
    let mut r1: u32 = 0;
    let mut r2: u32 = 0;
    let mut r3: u32 = 0;
    let mut r4: u32 = 0;
    let mut r5: u32 = 0;
    let mut r6: u32 = 0;
    let mut r11: u32 = 0;
    let mut r12: u32 = 0;
    let mut equal = false;
    let mut lower_or_same = false;
    let mut lower = false;
    let mut next = Flow::B0;
    unsafe {
        loop {
            match next {
                Flow::B0 => {
                    sp = sp.wrapping_sub(20);
                    r11 = sp.wrapping_add(16u32);
                    sp = sp.wrapping_sub(132u32);
                    r5 = 0xfe54u32;
                    r5 = (r5 & 0xffff) | (0x16bu32 << 16);
                    r0 = read_volatile((r5.wrapping_add(0x220u32)) as *const u32) as u32;
                    r0 = xm1_power_record_helper_fa7928(r0);
                    write_volatile((r11.wrapping_add((-0x64i32 as u32))) as *mut u32, r0 as u32);
                    r0 = read_volatile((r5.wrapping_add(0x220u32)) as *const u32) as u32;
                    r0 = xm1_power_record_helper_74fcec(r0);
                    write_volatile((r11.wrapping_add((-0x60i32 as u32))) as *mut u32, r0 as u32);
                    r0 = read_volatile((r5.wrapping_add(0x220u32)) as *const u32) as u32;
                    r0 = xm1_power_record_helper_74ffac(r0);
                    write_volatile((r11.wrapping_add((-0x5ci32 as u32))) as *mut u32, r0 as u32);
                    r0 = read_volatile((r5.wrapping_add(0x220u32)) as *const u32) as u32;
                    r0 = xm1_power_record_helper_74f9a4(r0);
                    write_volatile((r11.wrapping_add((-0x58i32 as u32))) as *mut u32, r0 as u32);
                    r0 = read_volatile((r5.wrapping_add(0x220u32)) as *const u32) as u32;
                    r0 = xm1_power_record_helper_74f1ac(r0);
                    write_volatile((r11.wrapping_add((-0x54i32 as u32))) as *mut u32, r0 as u32);
                    r0 = read_volatile((r5.wrapping_add(0x220u32)) as *const u32) as u32;
                    r0 = xm1_power_record_helper_74ed6c(r0);
                    write_volatile((r11.wrapping_add((-0x50i32 as u32))) as *mut u32, r0 as u32);
                    r0 = r11.wrapping_sub(124u32);
                    r1 = read_volatile((r5.wrapping_add(0x220u32)) as *const u32) as u32;
                    r0 = xm1_power_record_helper_74ff0c(r0 as *mut HeaderTriplet, r1) as usize as u32;
                    r0 = r11.wrapping_sub(76u32);
                    r1 = r11.wrapping_sub(124u32);
                    r2 = 6u32;
                    core::ptr::copy_nonoverlapping(r1 as *const u8, r0 as *mut u8, r2 as usize);
                    r6 = 0u32;
                    write_volatile((r11.wrapping_add((-0x44i32 as u32))) as *mut u32, r6 as u32);
                    r0 = xm1_power_record_helper_76d9c8();
                    write_volatile((r11.wrapping_add((-0x40i32 as u32))) as *mut u32, r0 as u32);
                    r0 = read_volatile((r5.wrapping_add(0x220u32)) as *const u32) as u32;
                    r0 = xm1_power_record_helper_757828(r0);
                    r4 = r0;
                    r0 = read_volatile((r11.wrapping_add((-0x40i32 as u32))) as *const u32) as u32;
                    r0 = xm1_power_record_helper_746c2c(r0);
                    r0 = r0.wrapping_add(r4);
                    write_volatile((r11.wrapping_add((-60i32 as u32))) as *mut u16, r0 as u16);
                    r0 = read_volatile((r5.wrapping_add(0x220u32)) as *const u32) as u32;
                    r0 = xm1_power_record_helper_757c10(r0);
                    r4 = r0;
                    r0 = read_volatile((r5.wrapping_add(0x220u32)) as *const u32) as u32;
                    r0 = xm1_power_record_helper_757df0(r0);
                    r0 = xm1_power_record_helper_772f28(r0);
                    r0 = r0.wrapping_add(r4);
                    write_volatile((r11.wrapping_add((-58i32 as u32))) as *mut u16, r0 as u16);
                    r0 = read_volatile((r5.wrapping_add(0x220u32)) as *const u32) as u32;
                    r0 = xm1_power_record_helper_757bc0();
                    write_volatile((r11.wrapping_add((-56i32 as u32))) as *mut u16, r0 as u16);
                    r2 = read_volatile((r5.wrapping_add(0x10u32)) as *const u32) as u32;
                    r3 = 0x851fu32;
                    r3 = (r3 & 0xffff) | (0x51ebu32 << 16);
                    { let product = (r3 as u64).wrapping_mul(r2 as u64); r1 = product as u32; r2 = (product >> 32) as u32; }
                    r2 = r2.wrapping_shr(5u32);
                    write_volatile((r11.wrapping_add((-52i32 as u32))) as *mut u16, r2 as u16);
                    r2 = read_volatile((r5.wrapping_add(0xcu32)) as *const u32) as u32;
                    { let product = (r3 as u64).wrapping_mul(r2 as u64); r1 = product as u32; r2 = (product >> 32) as u32; }
                    r2 = r2.wrapping_shr(5u32);
                    write_volatile((r11.wrapping_add((-54i32 as u32))) as *mut u16, r2 as u16);
                    r2 = read_volatile((r5.wrapping_add(0x14u32)) as *const u32) as u32;
                    { let product = (r3 as u64).wrapping_mul(r2 as u64); r1 = product as u32; r3 = (product >> 32) as u32; }
                    r3 = r3.wrapping_shr(5u32);
                    write_volatile((r11.wrapping_add((-50i32 as u32))) as *mut u16, r3 as u16);
                    write_volatile((r11.wrapping_add((-0x30i32 as u32))) as *mut u32, r6 as u32);
                    r3 = 1u32;
                    write_volatile((r11.wrapping_add((-0x2ci32 as u32))) as *mut u32, r3 as u32);
                    r0 = r11.wrapping_sub(100u32);
                    r1 = r11.wrapping_sub(112u32);
                    xm1_power_record_helper_f4d5bc(r0, r1);
                    r0 = r11.wrapping_sub(148u32);
                    r1 = read_volatile((r5.wrapping_add(0x220u32)) as *const u32) as u32;
                    r0 = xm1_power_record_helper_756e24(r0 as *mut ProfileWords5) as usize as u32;
                    r12 = r11.wrapping_sub(40u32);
                    lr = r11.wrapping_sub(148u32);
                    { let transfer = lr; r0 = read_volatile(transfer.wrapping_add(0) as *const u32); r1 = read_volatile(transfer.wrapping_add(4) as *const u32); r2 = read_volatile(transfer.wrapping_add(8) as *const u32); r3 = read_volatile(transfer.wrapping_add(12) as *const u32); lr = transfer.wrapping_add(16); }
                    { let transfer = r12; write_volatile(transfer.wrapping_add(0) as *mut u32, r0); write_volatile(transfer.wrapping_add(4) as *mut u32, r1); write_volatile(transfer.wrapping_add(8) as *mut u32, r2); write_volatile(transfer.wrapping_add(12) as *mut u32, r3); r12 = transfer.wrapping_add(16); }
                    r3 = read_volatile((r11.wrapping_add((-0x70i32 as u32))) as *const u32) as u32;
                    equal = r0 == r3; lower_or_same = r0 <= r3; lower = r0 < r3;
                    if !equal { r4 = 1u32; }
                    if !equal { next = Flow::B2; } else { next = Flow::B1; }
                    continue;
                }
                Flow::B1 => {
                    r4 = read_volatile((r11.wrapping_add((-36i32 as u32))) as *const u16) as u32;
                    r3 = read_volatile((r11.wrapping_add((-108i32 as u32))) as *const u16) as u32;
                    r4 = r4.wrapping_sub(r3); equal = r4 == 0;
                    if !equal { r4 = 1u32; }
                    next = Flow::B2;
                }
                Flow::B2 => {
                    r3 = read_volatile((r11.wrapping_add((-110i32 as u32))) as *const u16) as u32;
                    write_volatile((r11.wrapping_add((-38i32 as u32))) as *mut u16, r3 as u16);
                    r3 = read_volatile((r11.wrapping_add((-112i32 as u32))) as *const u16) as u32;
                    write_volatile((r11.wrapping_add((-40i32 as u32))) as *mut u16, r3 as u16);
                    r3 = read_volatile((r11.wrapping_add((-108i32 as u32))) as *const u16) as u32;
                    write_volatile((r11.wrapping_add((-36i32 as u32))) as *mut u16, r3 as u16);
                    r3 = 0xfe54u32;
                    r3 = (r3 & 0xffff) | (0x16bu32 << 16);
                    r2 = 0x21au32;
                    r2 = read_volatile((r3.wrapping_add(r2)) as *const u16) as u32;
                    write_volatile((r11.wrapping_add((-32i32 as u32))) as *mut u16, r2 as u16);
                    r2 = 536u32;
                    r2 = read_volatile((r3.wrapping_add(r2)) as *const u16) as u32;
                    write_volatile((r11.wrapping_add((-34i32 as u32))) as *mut u16, r2 as u16);
                    r2 = 540u32;
                    r2 = read_volatile((r3.wrapping_add(r2)) as *const u16) as u32;
                    write_volatile((r11.wrapping_add((-30i32 as u32))) as *mut u16, r2 as u16);
                    r2 = 1u32;
                    write_volatile((r11.wrapping_add((-0x1ci32 as u32))) as *mut u32, r2 as u32);
                    r2 = read_volatile((r11.wrapping_add((-0x68i32 as u32))) as *const u32) as u32;
                    write_volatile((r11.wrapping_add((-0x18i32 as u32))) as *mut u32, r2 as u32);
                    r0 = read_volatile((r3.wrapping_add(0x220u32)) as *const u32) as u32;
                    r1 = r11.wrapping_sub(40u32);
                    xm1_power_record_helper_756e5c(r0, r1 as *const ProfileWords5);
                    r0 = r4;
                    sp = r11.wrapping_sub(16u32);
                    return r0;
                }
            }
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_f37ab8(argument0: u32) {
    enum Flow { B0, B1, B2 }
    let mut frame = core::mem::MaybeUninit::<[u32; 380]>::uninit();
    let mut sp = (frame.as_mut_ptr() as usize as u32).wrapping_add(1520);
    let mut r0: u32 = argument0;
    let mut r1: u32 = 0;
    let mut r2: u32 = 0;
    let mut r3: u32 = 0;
    let mut r4: u32 = 0;
    let mut r5: u32 = 0;
    let mut r6: u32 = 0;
    let mut r11: u32 = 0;
    let mut equal = false;
    let mut lower_or_same = false;
    let mut lower = false;
    let mut next = Flow::B0;
    unsafe {
        loop {
            match next {
                Flow::B0 => {
                    sp = sp.wrapping_sub(20);
                    r11 = sp.wrapping_add(16u32);
                    sp = sp.wrapping_sub(1488u32);
                    sp = sp.wrapping_sub(12u32);
                    r5 = r0.wrapping_sub(0u32); equal = r5 == 0;
                    if equal { next = Flow::B2; } else { next = Flow::B1; }
                    continue;
                }
                Flow::B1 => {
                    r6 = 0xfe54u32;
                    r6 = (r6 & 0xffff) | (0x16bu32 << 16);
                    r0 = read_volatile((r6.wrapping_add(0x220u32)) as *const u32) as u32;
                    r0 = xm1_power_record_helper_74e5c0(r0);
                    write_volatile((r5.wrapping_add(0x8u32)) as *mut u32, r0 as u32);
                    r0 = read_volatile((r6.wrapping_add(0x220u32)) as *const u32) as u32;
                    r0 = xm1_power_record_helper_fa7928(r0);
                    write_volatile((r5.wrapping_add(0xcu32)) as *mut u32, r0 as u32);
                    r0 = read_volatile((r6.wrapping_add(0x220u32)) as *const u32) as u32;
                    r0 = xm1_power_record_helper_74f1ac(r0);
                    write_volatile((r5.wrapping_add(0x10u32)) as *mut u32, r0 as u32);
                    r0 = read_volatile((r6.wrapping_add(0x220u32)) as *const u32) as u32;
                    r0 = xm1_power_record_helper_74f9a4(r0);
                    write_volatile((r5.wrapping_add(0x14u32)) as *mut u32, r0 as u32);
                    r0 = read_volatile((r6.wrapping_add(0x220u32)) as *const u32) as u32;
                    r0 = xm1_power_record_helper_74fcec(r0);
                    write_volatile((r5.wrapping_add(0x18u32)) as *mut u32, r0 as u32);
                    r0 = read_volatile((r6.wrapping_add(0x220u32)) as *const u32) as u32;
                    r0 = xm1_power_record_helper_74e7a0(r0);
                    write_volatile((r5.wrapping_add(120u32)) as *mut u16, r0 as u16);
                    r0 = 2u32;
                    r2 = r11.wrapping_sub(1488u32);
                    r2 = r2.wrapping_sub(4u32);
                    r1 = r2.wrapping_sub(4u32);
                    xm1_power_record_helper_fea7ac(r0, r1 as *mut u8);
                    r3 = 0xfb38u32;
                    r3 = (r3 & 0xffff) | (0xffffu32 << 16);
                    r2 = r11.wrapping_sub(20u32);
                    r3 = read_volatile((r2.wrapping_add(r3)) as *const u16) as u32;
                    write_volatile((r5.wrapping_add(34u32)) as *mut u16, r3 as u16);
                    r3 = read_volatile((r11.wrapping_add((-0x4e0i32 as u32))) as *const u32) as u32;
                    write_volatile((r5.wrapping_add(0x24u32)) as *mut u32, r3 as u32);
                    r0 = read_volatile((r11.wrapping_add((-0x4e4i32 as u32))) as *const u32) as u32;
                    write_volatile((r5.wrapping_add(0x1cu32)) as *mut u32, r0 as u32);
                    r3 = 0xfb3au32;
                    r3 = (r3 & 0xffff) | (0xffffu32 << 16);
                    r4 = read_volatile((r2.wrapping_add(r3)) as *const u16) as u32;
                    r0 = xm1_power_record_helper_772f28(r0);
                    r0 = r4.wrapping_sub(r0);
                    write_volatile((r5.wrapping_add(40u32)) as *mut u16, r0 as u16);
                    r3 = 0xfb3cu32;
                    r3 = (r3 & 0xffff) | (0xffffu32 << 16);
                    r2 = r11.wrapping_sub(20u32);
                    r3 = read_volatile((r2.wrapping_add(r3)) as *const u16) as u32;
                    write_volatile((r5.wrapping_add(42u32)) as *mut u16, r3 as u16);
                    r3 = read_volatile((r11.wrapping_add((-0x4eci32 as u32))) as *const u32) as u32;
                    write_volatile((r5.wrapping_add(0x2cu32)) as *mut u32, r3 as u32);
                    r3 = read_volatile((r11.wrapping_add((-0x4c8i32 as u32))) as *const u32) as u32;
                    write_volatile((r5.wrapping_add(0x30u32)) as *mut u32, r3 as u32);
                    r3 = read_volatile((r11.wrapping_add((-0x5cci32 as u32))) as *const u32) as u32;
                    write_volatile((r5.wrapping_add(0x34u32)) as *mut u32, r3 as u32);
                    r2 = read_volatile((r11.wrapping_add((-0x5c8i32 as u32))) as *const u32) as u32;
                    write_volatile((r5.wrapping_add(0x38u32)) as *mut u32, r2 as u32);
                    r1 = read_volatile((r11.wrapping_add((-0x4f4i32 as u32))) as *const u32) as u32;
                    write_volatile((r5.wrapping_add(0x3cu32)) as *mut u32, r1 as u32);
                    r1 = read_volatile((r11.wrapping_add((-0x4f0i32 as u32))) as *const u32) as u32;
                    write_volatile((r5.wrapping_add(0x40u32)) as *mut u32, r1 as u32);
                    r1 = read_volatile((r11.wrapping_add((-0x5c4i32 as u32))) as *const u32) as u32;
                    write_volatile((r5.wrapping_add(0x4cu32)) as *mut u32, r1 as u32);
                    r1 = read_volatile((r11.wrapping_add((-0x5c0i32 as u32))) as *const u32) as u32;
                    write_volatile((r5.wrapping_add(0x50u32)) as *mut u32, r1 as u32);
                    r1 = read_volatile((r11.wrapping_add((-0x4d4i32 as u32))) as *const u32) as u32;
                    write_volatile((r5.wrapping_add(0x68u32)) as *mut u32, r1 as u32);
                    r1 = 0xd304u32;
                    r1 = (r1 & 0xffff) | (0x5ffeu32 << 16);
                    write_volatile((r5.wrapping_add(0x58u32)) as *mut u32, r1 as u32);
                    r1 = 0xe304u32;
                    r1 = (r1 & 0xffff) | (0x5ffeu32 << 16);
                    write_volatile((r5.wrapping_add(0x5cu32)) as *mut u32, r1 as u32);
                    r1 = read_volatile((r11.wrapping_add((-0x5d8i32 as u32))) as *const u32) as u32;
                    r1 = r1 & 5u32;
                    equal = r1 == 5u32; lower_or_same = r1 <= 5u32; lower = r1 < 5u32;
                    if !equal { r1 = 0u32; }
                    if equal { r1 = 1u32; }
                    write_volatile((r5) as *mut u32, r1 as u32);
                    r1 = 100u32;
                    write_volatile((r5.wrapping_add(68u32)) as *mut u16, r1 as u16);
                    write_volatile((r5.wrapping_add(70u32)) as *mut u16, r1 as u16);
                    r1 = 256u32;
                    write_volatile((r5.wrapping_add(72u32)) as *mut u16, r1 as u16);
                    write_volatile((sp) as *mut u32, r2 as u32);
                    write_volatile((sp.wrapping_add(0x4u32)) as *mut u32, r3 as u32);
                    r0 = r5.wrapping_add(84u32);
                    r3 = r11.wrapping_sub(1488u32);
                    r3 = r3.wrapping_sub(4u32);
                    r1 = r3.wrapping_sub(8u32);
                    r2 = r3.wrapping_sub(12u32);
                    r3 = r11.wrapping_sub(1504u32);
                    r3 = r3.wrapping_sub(4u32);
                    xm1_power_record_helper_f5eba0(r0 as *mut u32, r1 as *mut u32, r2 as *mut u32, r3 as *mut u32, read_volatile(sp.wrapping_add(0) as *const u32), read_volatile(sp.wrapping_add(4) as *const u32));
                    r3 = read_volatile((r5.wrapping_add(0x54u32)) as *const u32) as u32;
                    r3 = r3.wrapping_shl(2u32);
                    write_volatile((r5.wrapping_add(0x54u32)) as *mut u32, r3 as u32);
                    r0 = read_volatile((r6.wrapping_add(0x220u32)) as *const u32) as u32;
                    r0 = xm1_power_record_helper_756ac4(r0);
                    r4 = r0;
                    r0 = read_volatile((r6.wrapping_add(0x220u32)) as *const u32) as u32;
                    r0 = xm1_power_record_helper_7577e0();
                    write_volatile((r5.wrapping_add(0x74u32)) as *mut u32, r0 as u32);
                    r0 = read_volatile((r6.wrapping_add(0x220u32)) as *const u32) as u32;
                    r0 = xm1_power_record_helper_757e38(r0);
                    r0 = xm1_power_record_helper_746c2c(r0);
                    write_volatile((r5.wrapping_add(112u32)) as *mut u16, r0 as u16);
                    r0 = read_volatile((r6.wrapping_add(0x220u32)) as *const u32) as u32;
                    r0 = xm1_power_record_helper_757c60(r0);
                    write_volatile((r5.wrapping_add(114u32)) as *mut u16, r0 as u16);
                    r0 = read_volatile((r5.wrapping_add(34u32)) as *const i16) as i32 as u32;
                    r1 = read_volatile((r5.wrapping_add(0x8u32)) as *const u32) as u32;
                    r2 = r4;
                    r0 = xm1_power_record_helper_fa7e20(r0 as i32, r1, r2);
                    write_volatile((r5.wrapping_add(0x64u32)) as *mut u32, r0 as u32);
                    r3 = read_volatile((r6) as *const u8) as u32;
                    write_volatile((r5.wrapping_add(0x4u32)) as *mut u8, r3 as u8);
                    r3 = 0u32;
                    write_volatile((r5.wrapping_add(0x6cu32)) as *mut u32, r3 as u32);
                    r0 = read_volatile((r6.wrapping_add(0x220u32)) as *const u32) as u32;
                    r0 = xm1_power_record_helper_7580ec(r0);
                    r0 = r0.wrapping_sub(3u32);
                    equal = r0 == 1u32; lower_or_same = r0 <= 1u32; lower = r0 < 1u32;
                    if lower_or_same { r3 = 1u32; }
                    if lower_or_same { write_volatile((r5.wrapping_add(0x6cu32)) as *mut u32, r3 as u32); }
                    next = Flow::B2;
                }
                Flow::B2 => {
                    sp = r11.wrapping_sub(16u32);
                    return;
                }
            }
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_f37854(argument0: u32, argument1: u32) {
    enum Flow { B0, B1, B2 }
    let mut frame = core::mem::MaybeUninit::<[u32; 4]>::uninit();
    let mut sp = (frame.as_mut_ptr() as usize as u32).wrapping_add(16);
    let mut r0: u32 = argument0;
    let mut r1: u32 = argument1;
    let mut r2: u32 = 0;
    let mut r11: u32 = 0;
    let mut equal = false;
    let mut lower_or_same = false;
    let mut lower = false;
    let mut next = Flow::B0;
    unsafe {
        loop {
            match next {
                Flow::B0 => {
                    sp = sp.wrapping_sub(8);
                    r11 = sp.wrapping_add(4u32);
                    sp = sp.wrapping_sub(8u32);
                    write_volatile((r11.wrapping_add((-0xci32 as u32))) as *mut u32, r0 as u32);
                    write_volatile((r11.wrapping_add((-0x8i32 as u32))) as *mut u32, r1 as u32);
                    r0 = 0x24eu32;
                    r0 = xm1_power_configuration_byte(r0);
                    equal = r0 == 1u32; lower_or_same = r0 <= 1u32; lower = r0 < 1u32;
                    if !equal { next = Flow::B2; } else { next = Flow::B1; }
                    continue;
                }
                Flow::B1 => {
                    r0 = 16384u32;
                    r0 = (r0 & 0xffff) | (0x1110u32 << 16);
                    xm1_power_recording_capture_trace_begin(r0);
                    next = Flow::B2;
                }
                Flow::B2 => {
                    r0 = 138u32;
                    r1 = r11.wrapping_sub(12u32);
                    r2 = 8u32;
                    xm1_power_record_helper_ef9a94(r0, r1, r2);
                    sp = r11.wrapping_sub(4u32);
                    return;
                }
            }
        }
    }
}
