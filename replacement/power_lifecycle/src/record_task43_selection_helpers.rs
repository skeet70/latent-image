use core::ptr::{read_volatile, write_volatile};

#[repr(C)]
struct HeaderTriplet { words: [u16; 3] }
#[repr(C)]
struct ProfileWords5 { words: [u32; 5] }
#[repr(C)]
struct ProfileWords2 { words: [u32; 2] }

extern "C" {
    fn xm1_power_configuration_byte(argument0: u32) -> u32;
    fn xm1_power_record_helper_74f564(argument0: u32) -> u32;
    fn xm1_power_record_helper_7524fc(argument0: u32, argument1: u32);
    fn xm1_power_record_helper_75259c(argument0: u32, argument1: *const HeaderTriplet);
    fn xm1_power_record_helper_75263c(argument0: u32, argument1: *const HeaderTriplet);
    fn xm1_power_record_helper_7526cc(argument0: u32, argument1: u32);
    fn xm1_power_record_helper_753c20(argument0: u32, argument1: u32) -> u32;
    fn xm1_power_record_helper_75466c(argument0: u32, argument1: u32);
    fn xm1_power_record_helper_75470c(argument0: u32, argument1: *const HeaderTriplet);
    fn xm1_power_record_helper_7547ac(argument0: u32, argument1: *const HeaderTriplet);
    fn xm1_power_record_helper_75484c(argument0: u32, argument1: *const HeaderTriplet);
    fn xm1_power_record_helper_7548dc(argument0: u32, argument1: u32);
    fn xm1_power_record_helper_75496c(argument0: u32, argument1: u32);
    fn xm1_power_record_helper_7549fc(argument0: u32, argument1: u32);
    fn xm1_power_record_helper_754a9c(argument0: u32, argument1: *const ProfileWords2);
    fn xm1_power_record_helper_754b88(argument0: u32, argument1: u32);
    fn xm1_power_record_helper_754c20(argument0: u32, argument1: u32);
    fn xm1_power_record_helper_754cb8(argument0: u32, argument1: u32);
    fn xm1_power_record_helper_754d50(argument0: u32, argument1: u32);
    fn xm1_power_record_helper_756e24(argument0: *mut ProfileWords5) -> *mut ProfileWords5;
    fn xm1_power_record_helper_ef9a94(argument0: u32, argument1: u32, argument2: u32);
    fn xm1_power_record_helper_efa968(argument0: u32);
    fn xm1_power_record_helper_efa9f0(argument0: u32);
    fn xm1_power_record_helper_efaa34(argument0: u32);
    fn xm1_power_record_helper_efaa80(argument0: u32);
    fn xm1_power_record_helper_efaacc(argument0: u32);
    fn xm1_power_record_helper_f2c210() -> u32;
    fn xm1_power_record_helper_f2c234() -> u32;
    fn xm1_power_record_helper_f2c258(argument0: u32) -> u32;
    fn xm1_power_record_helper_f2c2bc() -> u32;
    fn xm1_power_record_helper_f2c2e0() -> u32;
    fn xm1_power_record_helper_f2c304() -> u32;
    fn xm1_power_record_helper_f2c348() -> u32;
    fn xm1_power_record_helper_f2c37c() -> u32;
    fn xm1_power_record_helper_f2c3a0() -> u32;
    fn xm1_power_record_helper_f2c3c0(argument0: u32);
    fn xm1_power_record_helper_f2c878() -> u32;
    fn xm1_power_record_helper_f4d350(argument0: u32, argument1: u32);
    fn xm1_power_record_helper_f4d448(argument0: u32, argument1: u32);
    fn xm1_power_record_helper_f51bcc(argument0: u32, argument1: u32, argument2: u32);
    fn xm1_power_record_helper_f52120(argument0: u32, argument1: u32);
    fn xm1_power_record_helper_feacac(argument0: u32, argument1: u32, argument2: u32, argument3: u32, argument4: u32, argument5: u32) -> u32;
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_f2e754(argument0: u32) {
    let mut r0: u32 = argument0;
    let mut r3: u32 = 0;
    unsafe {
        r3 = 0x79a8u32;
        r3 = (r3 & 0xffff) | (0x16bu32 << 16);
        write_volatile((r3) as *mut u32, r0 as u32);
        return;
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_f2e774() {
    let mut r0: u32 = 0;
    let mut r2: u32 = 0;
    let mut r3: u32 = 0;
    let mut r4: u32 = 0;
    let mut equal = false;
    let mut lower_or_same = false;
    let mut lower = false;
    unsafe {
        r0 = 23711900u32;
        r4 = r0;
        r2 = read_volatile((r0.wrapping_add(0x4u32)) as *const u32) as u32;
        r3 = 0x79a8u32;
        r3 = (r3 & 0xffff) | (0x16bu32 << 16);
        write_volatile((r3.wrapping_add(0x4u32)) as *mut u32, r2 as u32);
        r0 = 168u32;
        r0 = xm1_power_configuration_byte(r0);
        equal = (r0 & 128u32) == 0;
        r3 = 0x79a8u32;
        r3 = (r3 & 0xffff) | (0x16bu32 << 16);
        if !equal { r2 = read_volatile((r4.wrapping_add(0xcu32)) as *const u32) as u32; }
        if !equal { r2 = read_volatile((r2.wrapping_add(0x50u32)) as *const u32) as u32; }
        if equal { r2 = 0u32; }
        write_volatile((r3.wrapping_add(0x8u32)) as *mut u32, r2 as u32);
        return;
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_f2e7bc() -> u32 {
    enum Flow { B0, B1, B2, B3, B4, B5, B6, B7 }
    let mut frame = core::mem::MaybeUninit::<[u32; 26]>::uninit();
    let mut sp = (frame.as_mut_ptr() as usize as u32).wrapping_add(104);
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
                    sp = sp.wrapping_sub(68u32);
                    r0 = 23711900u32;
                    r4 = r0;
                    r2 = 0x79a8u32;
                    r2 = (r2 & 0xffff) | (0x16bu32 << 16);
                    r3 = 0xd370u32;
                    r3 = (r3 & 0xffff) | (0x169u32 << 16);
                    r1 = read_volatile((r3.wrapping_add(0x14u32)) as *const u8) as u32;
                    write_volatile((r2.wrapping_add(0xcu32)) as *mut u8, r1 as u8);
                    r2 = read_volatile((r3.wrapping_add(0x2cu32)) as *const u32) as u32;
                    write_volatile((r11.wrapping_add((-0x34i32 as u32))) as *mut u32, r2 as u32);
                    r1 = read_volatile((r3.wrapping_add(0x30u32)) as *const u32) as u32;
                    write_volatile((r11.wrapping_add((-0x30i32 as u32))) as *mut u32, r1 as u32);
                    r1 = read_volatile((r0.wrapping_add(0x38u32)) as *const u32) as u32;
                    write_volatile((r1.wrapping_add(0x18u32)) as *mut u32, r2 as u32);
                    r2 = read_volatile((r0.wrapping_add(0x38u32)) as *const u32) as u32;
                    r3 = read_volatile((r3.wrapping_add(0x30u32)) as *const u32) as u32;
                    write_volatile((r2.wrapping_add(0x1cu32)) as *mut u32, r3 as u32);
                    r3 = read_volatile((r11.wrapping_add((-0x34i32 as u32))) as *const u32) as u32;
                    equal = r3 == 3u32; lower_or_same = r3 <= 3u32; lower = r3 < 3u32;
                    if equal { r3 = 1u32; }
                    if equal { write_volatile((r11.wrapping_add((-0x38i32 as u32))) as *mut u32, r3 as u32); }
                    if equal { r3 = 0xd370u32; }
                    if equal { r3 = (r3 & 0xffff) | (0x169u32 << 16); }
                    if equal { r3 = read_volatile((r3.wrapping_add(0x15u32)) as *const u8) as u32; }
                    if !equal { r3 = 0u32; }
                    if !equal { write_volatile((r11.wrapping_add((-0x38i32 as u32))) as *mut u32, r3 as u32); }
                    write_volatile((r11.wrapping_add((-0x50i32 as u32))) as *mut u8, r3 as u8);
                    r3 = 0x79a8u32;
                    r3 = (r3 & 0xffff) | (0x16bu32 << 16);
                    r2 = read_volatile((r3.wrapping_add(0x4u32)) as *const u32) as u32;
                    r3 = 0x11eu32;
                    r3 = r2 & r3;
                    equal = r3 == 0u32; lower_or_same = r3 <= 0u32; lower = r3 < 0u32;
                    if equal { next = Flow::B6; } else { next = Flow::B1; }
                    continue;
                }
                Flow::B1 => {
                    r2 = 0x79a8u32;
                    r2 = (r2 & 0xffff) | (0x16bu32 << 16);
                    r0 = read_volatile((r2.wrapping_add(0xcu32)) as *const u8) as u32;
                    r3 = read_volatile((r4.wrapping_add(0xcu32)) as *const u32) as u32;
                    r3 = read_volatile((r3.wrapping_add(0x48u32)) as *const u32) as u32;
                    r1 = read_volatile((r2.wrapping_add(0x8u32)) as *const u32) as u32;
                    write_volatile((sp) as *mut u32, r1 as u32);
                    r1 = r11.wrapping_sub(80u32);
                    write_volatile((sp.wrapping_add(0x4u32)) as *mut u32, r1 as u32);
                    r1 = 0x6120u32;
                    r1 = (r1 & 0xffff) | (0x5ffbu32 << 16);
                    r2 = r2.wrapping_add(16u32);
                    r0 = xm1_power_record_helper_feacac(r0, r1, r2, r3, read_volatile(sp.wrapping_add(0) as *const u32), read_volatile(sp.wrapping_add(4) as *const u32));
                    write_volatile((r11.wrapping_add((-0x58i32 as u32))) as *mut u32, r0 as u32);
                    r9 = read_volatile((r4.wrapping_add(0x38u32)) as *const u32) as u32;
                    r4 = 0x7140u32;
                    r4 = (r4 & 0xffff) | (0x5ffbu32 << 16);
                    r0 = r4;
                    r1 = 0u32;
                    r2 = 1024u32;
                    core::ptr::write_bytes(r0 as *mut u8, r1 as u8, r2 as usize);
                    r5 = 0x7240u32;
                    r5 = (r5 & 0xffff) | (0x5ffbu32 << 16);
                    r12 = 0x7340u32;
                    r12 = (r12 & 0xffff) | (0x5ffbu32 << 16);
                    r0 = 0x7440u32;
                    r0 = (r0 & 0xffff) | (0x5ffbu32 << 16);
                    r8 = 0u32;
                    r10 = r11.wrapping_sub(80u32);
                    r1 = 0x6fffu32;
                    r1 = (r1 & 0xffff) | (0x5ffbu32 << 16);
                    r2 = 0xf121u32;
                    r2 = (r2 & 0xffff) | (0xffffu32 << 16);
                    next = Flow::B2;
                }
                Flow::B2 => {
                    r6 = r8 & 7u32;
                    r3 = r8.wrapping_shr(3u32);
                    r3 = r3.wrapping_shl(5u32);
                    r3 = r3.wrapping_add(r6.wrapping_shl(1u32));
                    r3 = r3 & 0xff;
                    write_volatile((r11.wrapping_add((-0x54i32 as u32))) as *mut u8, r3 as u8);
                    r6 = r3.wrapping_add(1u32);
                    write_volatile((r11.wrapping_add((-0x53i32 as u32))) as *mut u8, r6 as u8);
                    r6 = r3.wrapping_add(16u32);
                    write_volatile((r11.wrapping_add((-0x52i32 as u32))) as *mut u8, r6 as u8);
                    r3 = r3.wrapping_add(17u32);
                    write_volatile((r11.wrapping_add((-0x51i32 as u32))) as *mut u8, r3 as u8);
                    r3 = r11.wrapping_sub(84u32);
                    next = Flow::B3;
                }
                Flow::B3 => {
                    r6 = read_volatile((r3) as *const u8) as u32;
                    r6 = r1.wrapping_add(r6.wrapping_shl(2u32));
                    r7 = read_volatile((r4) as *const u32) as u32;
                    r6 = read_volatile((r6.wrapping_add(r2)) as *const u32) as u32;
                    r6 = r7.wrapping_add(r6);
                    write_volatile((r4) as *mut u32, r6 as u32);
                    r6 = read_volatile((r3) as *const u8) as u32;r3 = r3.wrapping_add(1u32);
                    r6 = r6.wrapping_add(256u32);
                    r6 = r1.wrapping_add(r6.wrapping_shl(2u32));
                    r7 = read_volatile((r5) as *const u32) as u32;
                    r6 = read_volatile((r6.wrapping_add(r2)) as *const u32) as u32;
                    r6 = r7.wrapping_add(r6);
                    write_volatile((r5) as *mut u32, r6 as u32);
                    r6 = read_volatile((r3.wrapping_add((-0x1i32 as u32))) as *const u8) as u32;
                    r6 = r6.wrapping_add(512u32);
                    r6 = r1.wrapping_add(r6.wrapping_shl(2u32));
                    r7 = read_volatile((r12) as *const u32) as u32;
                    r6 = read_volatile((r6.wrapping_add(r2)) as *const u32) as u32;
                    r6 = r7.wrapping_add(r6);
                    write_volatile((r12) as *mut u32, r6 as u32);
                    r6 = read_volatile((r3.wrapping_add((-0x1i32 as u32))) as *const u8) as u32;
                    r6 = r6.wrapping_add(768u32);
                    r6 = r1.wrapping_add(r6.wrapping_shl(2u32));
                    r7 = read_volatile((r0) as *const u32) as u32;
                    r6 = read_volatile((r6.wrapping_add(r2)) as *const u32) as u32;
                    r6 = r7.wrapping_add(r6);
                    write_volatile((r0) as *mut u32, r6 as u32);
                    equal = r3 == r10; lower_or_same = r3 <= r10; lower = r3 < r10;
                    if !equal { next = Flow::B3; } else { next = Flow::B4; }
                    continue;
                }
                Flow::B4 => {
                    r3 = read_volatile((r4) as *const u32) as u32;
                    r3 = r3.wrapping_shr(2u32);
                    write_volatile((r4) as *mut u32, r3 as u32);r4 = r4.wrapping_add(4u32);
                    r3 = read_volatile((r5) as *const u32) as u32;
                    r3 = r3.wrapping_shr(2u32);
                    write_volatile((r5) as *mut u32, r3 as u32);r5 = r5.wrapping_add(4u32);
                    r3 = read_volatile((r12) as *const u32) as u32;
                    r3 = r3.wrapping_shr(2u32);
                    write_volatile((r12) as *mut u32, r3 as u32);r12 = r12.wrapping_add(4u32);
                    r3 = read_volatile((r0) as *const u32) as u32;
                    r3 = r3.wrapping_shr(2u32);
                    write_volatile((r0) as *mut u32, r3 as u32);r0 = r0.wrapping_add(4u32);
                    r8 = r8.wrapping_add(1u32);
                    equal = r8 == 64u32; lower_or_same = r8 <= 64u32; lower = r8 < 64u32;
                    if !equal { next = Flow::B2; } else { next = Flow::B5; }
                    continue;
                }
                Flow::B5 => {
                    r3 = 0x7140u32;
                    r3 = (r3 & 0xffff) | (0x5ffbu32 << 16);
                    write_volatile((r9.wrapping_add(0x28u32)) as *mut u32, r3 as u32);
                    r3 = 0x7240u32;
                    r3 = (r3 & 0xffff) | (0x5ffbu32 << 16);
                    write_volatile((r9.wrapping_add(0x2cu32)) as *mut u32, r3 as u32);
                    r3 = 0x7340u32;
                    r3 = (r3 & 0xffff) | (0x5ffbu32 << 16);
                    write_volatile((r9.wrapping_add(0x30u32)) as *mut u32, r3 as u32);
                    r3 = 0x7440u32;
                    r3 = (r3 & 0xffff) | (0x5ffbu32 << 16);
                    write_volatile((r9.wrapping_add(0x34u32)) as *mut u32, r3 as u32);
                    r3 = 8u32;
                    write_volatile((r9.wrapping_add(0x38u32)) as *mut u8, r3 as u8);
                    write_volatile((r9.wrapping_add(0x39u32)) as *mut u8, r3 as u8);
                    r3 = 0x79a8u32;
                    r3 = (r3 & 0xffff) | (0x16bu32 << 16);
                    r2 = read_volatile((r3.wrapping_add(0x14u32)) as *const u32) as u32;
                    write_volatile((r9.wrapping_add(58u32)) as *mut u16, r2 as u16);
                    r3 = read_volatile((r3.wrapping_add(0x10u32)) as *const u32) as u32;
                    write_volatile((r9.wrapping_add(60u32)) as *mut u16, r3 as u16);
                    next = Flow::B7;
                    continue;
                }
                Flow::B6 => {
                    r2 = 0x79a8u32;
                    r2 = (r2 & 0xffff) | (0x16bu32 << 16);
                    r0 = read_volatile((r2.wrapping_add(0xcu32)) as *const u8) as u32;
                    r3 = read_volatile((r4.wrapping_add(0xcu32)) as *const u32) as u32;
                    r3 = read_volatile((r3.wrapping_add(0x48u32)) as *const u32) as u32;
                    r1 = 0u32;
                    write_volatile((sp) as *mut u32, r1 as u32);
                    r12 = r11.wrapping_sub(80u32);
                    write_volatile((sp.wrapping_add(0x4u32)) as *mut u32, r12 as u32);
                    r2 = r2.wrapping_add(16u32);
                    r0 = xm1_power_record_helper_feacac(r0, r1, r2, r3, read_volatile(sp.wrapping_add(0) as *const u32), read_volatile(sp.wrapping_add(4) as *const u32));
                    write_volatile((r11.wrapping_add((-0x58i32 as u32))) as *mut u32, r0 as u32);
                    next = Flow::B7;
                }
                Flow::B7 => {
                    r0 = read_volatile((r11.wrapping_add((-0x58i32 as u32))) as *const u32) as u32;
                    sp = r11.wrapping_sub(32u32);
                    return r0;
                }
            }
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_f2ea4c() {
    let mut r0: u32 = 0;
    unsafe {
        return;
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_f2ea60() -> u32 {
    let mut r0: u32 = 0;
    let mut r3: u32 = 0;
    let mut equal = false;
    let mut lower_or_same = false;
    let mut lower = false;
    unsafe {
        r3 = 0x79a8u32;
        r3 = (r3 & 0xffff) | (0x16bu32 << 16);
        r3 = read_volatile((r3.wrapping_add(0x4u32)) as *const u32) as u32;
        equal = (r3 & 10u32) == 0;
        if equal { r0 = 0u32; }
        if !equal { r0 = 1u32; }
        return r0;
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_f2ea8c() {
    enum Flow { B0, B1, B2, B3 }
    let mut frame = core::mem::MaybeUninit::<[u32; 36]>::uninit();
    let mut sp = (frame.as_mut_ptr() as usize as u32).wrapping_add(144);
    let mut lr: u32 = 0;
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
                    sp = sp.wrapping_sub(128u32);
                    r4 = 0x79a8u32;
                    r4 = (r4 & 0xffff) | (0x16bu32 << 16);
                    r3 = r4;
                    r2 = read_volatile((r3) as *const u32) as u32;r3 = r3.wrapping_add(16u32);
                    write_volatile((r11.wrapping_add((-0x5ci32 as u32))) as *mut u32, r2 as u32);
                    write_volatile((r11.wrapping_add((-0x58i32 as u32))) as *mut u32, r3 as u32);
                    r0 = xm1_power_record_helper_f2c210();
                    write_volatile((r11.wrapping_add((-84i32 as u32))) as *mut u16, r0 as u16);
                    r0 = xm1_power_record_helper_f2c234();
                    write_volatile((r11.wrapping_add((-82i32 as u32))) as *mut u16, r0 as u16);
                    r0 = read_volatile((r4.wrapping_add(0x98u32)) as *const u32) as u32;
                    r0 = xm1_power_record_helper_f2c258(r0);
                    write_volatile((r11.wrapping_add((-80i32 as u32))) as *mut u16, r0 as u16);
                    r0 = xm1_power_record_helper_f2c2bc();
                    write_volatile((r11.wrapping_add((-78i32 as u32))) as *mut u16, r0 as u16);
                    r0 = xm1_power_record_helper_f2c304();
                    write_volatile((r11.wrapping_add((-76i32 as u32))) as *mut u16, r0 as u16);
                    r0 = xm1_power_record_helper_f2c2e0();
                    write_volatile((r11.wrapping_add((-56i32 as u32))) as *mut u16, r0 as u16);
                    r0 = xm1_power_record_helper_f2c37c();
                    write_volatile((r11.wrapping_add((-54i32 as u32))) as *mut u16, r0 as u16);
                    r0 = xm1_power_record_helper_f2c3a0();
                    write_volatile((r11.wrapping_add((-0x14i32 as u32))) as *mut u8, r0 as u8);
                    r0 = r11.wrapping_sub(140u32);
                    xm1_power_record_helper_f2c3c0(r0);
                    r12 = r11.wrapping_sub(44u32);
                    lr = r11.wrapping_sub(140u32);
                    { let transfer = lr; r0 = read_volatile(transfer.wrapping_add(0) as *const u32); r1 = read_volatile(transfer.wrapping_add(4) as *const u32); r2 = read_volatile(transfer.wrapping_add(8) as *const u32); r3 = read_volatile(transfer.wrapping_add(12) as *const u32); lr = transfer.wrapping_add(16); }
                    { let transfer = r12; write_volatile(transfer.wrapping_add(0) as *mut u32, r0); write_volatile(transfer.wrapping_add(4) as *mut u32, r1); write_volatile(transfer.wrapping_add(8) as *mut u32, r2); write_volatile(transfer.wrapping_add(12) as *mut u32, r3); r12 = transfer.wrapping_add(16); }
                    { let transfer = lr; r0 = read_volatile(transfer.wrapping_add(0) as *const u32); r1 = read_volatile(transfer.wrapping_add(4) as *const u32);  }
                    { let transfer = r12; write_volatile(transfer.wrapping_add(0) as *mut u32, r0); write_volatile(transfer.wrapping_add(4) as *mut u32, r1);  }
                    r0 = xm1_power_record_helper_f2c348();
                    write_volatile((r11.wrapping_add((-16i32 as u32))) as *mut u16, r0 as u16);
                    r0 = read_volatile((r11.wrapping_add((-0x5ci32 as u32))) as *const u32) as u32;
                    r1 = 0u32;
                    r0 = xm1_power_record_helper_753c20(r0, r1);
                    equal = r0 == 2u32; lower_or_same = r0 <= 2u32; lower = r0 < 2u32;
                    if !equal { next = Flow::B2; } else { next = Flow::B1; }
                    continue;
                }
                Flow::B1 => {
                    r0 = xm1_power_record_helper_f2c878();
                    equal = r0 == 0u32; lower_or_same = r0 <= 0u32; lower = r0 < 0u32;
                    if !equal { r3 = 1u32; }
                    if !equal { next = Flow::B3; } else { next = Flow::B2; }
                    continue;
                }
                Flow::B2 => {
                    r3 = 0u32;
                    next = Flow::B3;
                }
                Flow::B3 => {
                    write_volatile((r11.wrapping_add((-0x34i32 as u32))) as *mut u32, r3 as u32);
                    r0 = read_volatile((r11.wrapping_add((-0x5ci32 as u32))) as *const u32) as u32;
                    r0 = xm1_power_record_helper_74f564(r0);
                    write_volatile((r11.wrapping_add((-0x30i32 as u32))) as *mut u32, r0 as u32);
                    r4 = 0x79a8u32;
                    r4 = (r4 & 0xffff) | (0x16bu32 << 16);
                    r0 = r11.wrapping_sub(112u32);
                    r1 = read_volatile((r4) as *const u32) as u32;
                    r0 = xm1_power_record_helper_756e24(r0 as *mut ProfileWords5) as usize as u32;
                    r5 = 6u32;
                    r0 = r11.wrapping_sub(74u32);
                    r1 = r11.wrapping_sub(112u32);
                    r2 = r5;
                    core::ptr::copy_nonoverlapping(r1 as *const u8, r0 as *mut u8, r2 as usize);
                    r0 = r11.wrapping_sub(68u32);
                    r1 = r11.wrapping_sub(106u32);
                    r2 = r5;
                    core::ptr::copy_nonoverlapping(r1 as *const u8, r0 as *mut u8, r2 as usize);
                    r3 = read_volatile((r4.wrapping_add(0x8u32)) as *const u32) as u32;
                    write_volatile((r11.wrapping_add((-0x3ci32 as u32))) as *mut u32, r3 as u32);
                    r0 = 0xe710u32;
                    r0 = xm1_power_record_helper_f2e710 as usize as u32;
                    r1 = 0u32;
                    r2 = r11.wrapping_sub(92u32);
                    xm1_power_record_helper_f51bcc(r0, r1, r2);
                    sp = r11.wrapping_sub(12u32);
                    return;
                }
            }
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_f2ebb0() {
    let mut r0: u32 = 0;
    unsafe {
        return;
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_f2ebc4() {
    let mut r0: u32 = 0;
    let mut r1: u32 = 0;
    let mut r2: u32 = 0;
    let mut r3: u32 = 0;
    let mut r4: u32 = 0;
    let mut r5: u32 = 0;
    let mut r6: u32 = 0;
    let mut r12: u32 = 0;
    unsafe {
        r6 = 0x79a8u32;
        r6 = (r6 & 0xffff) | (0x16bu32 << 16);
        r4 = 0xd370u32;
        r4 = (r4 & 0xffff) | (0x169u32 << 16);
        r12 = r6.wrapping_add(168u32);
        r5 = r4.wrapping_add(16u32);
        { let transfer = r5; r0 = read_volatile(transfer.wrapping_add(0) as *const u32); r1 = read_volatile(transfer.wrapping_add(4) as *const u32); r2 = read_volatile(transfer.wrapping_add(8) as *const u32); r3 = read_volatile(transfer.wrapping_add(12) as *const u32); r5 = transfer.wrapping_add(16); }
        { let transfer = r12; write_volatile(transfer.wrapping_add(0) as *mut u32, r0); write_volatile(transfer.wrapping_add(4) as *mut u32, r1); write_volatile(transfer.wrapping_add(8) as *mut u32, r2); write_volatile(transfer.wrapping_add(12) as *mut u32, r3); r12 = transfer.wrapping_add(16); }
        { let transfer = r5; r0 = read_volatile(transfer.wrapping_add(0) as *const u32); r1 = read_volatile(transfer.wrapping_add(4) as *const u32); r2 = read_volatile(transfer.wrapping_add(8) as *const u32); r3 = read_volatile(transfer.wrapping_add(12) as *const u32); r5 = transfer.wrapping_add(16); }
        { let transfer = r12; write_volatile(transfer.wrapping_add(0) as *mut u32, r0); write_volatile(transfer.wrapping_add(4) as *mut u32, r1); write_volatile(transfer.wrapping_add(8) as *mut u32, r2); write_volatile(transfer.wrapping_add(12) as *mut u32, r3); r12 = transfer.wrapping_add(16); }
        { let transfer = r5; r0 = read_volatile(transfer.wrapping_add(0) as *const u32); r1 = read_volatile(transfer.wrapping_add(4) as *const u32); r2 = read_volatile(transfer.wrapping_add(8) as *const u32); r3 = read_volatile(transfer.wrapping_add(12) as *const u32);  }
        { let transfer = r12; write_volatile(transfer.wrapping_add(0) as *mut u32, r0); write_volatile(transfer.wrapping_add(4) as *mut u32, r1); write_volatile(transfer.wrapping_add(8) as *mut u32, r2); write_volatile(transfer.wrapping_add(12) as *mut u32, r3);  }
        r0 = read_volatile((r6) as *const u32) as u32;
        r1 = read_volatile((r4.wrapping_add(0x10u32)) as *const u32) as u32;
        xm1_power_record_helper_7524fc(r0, r1);
        r0 = read_volatile((r6) as *const u32) as u32;
        r1 = r4.wrapping_add(20u32);
        xm1_power_record_helper_75259c(r0, r1 as *const HeaderTriplet);
        r0 = read_volatile((r6) as *const u32) as u32;
        r1 = read_volatile((r4.wrapping_add(0x1cu32)) as *const u32) as u32;
        xm1_power_record_helper_75466c(r0, r1);
        r0 = read_volatile((r6) as *const u32) as u32;
        r1 = r5.wrapping_sub(16u32);
        xm1_power_record_helper_75470c(r0, r1 as *const HeaderTriplet);
        r0 = read_volatile((r6) as *const u32) as u32;
        r1 = r4.wrapping_add(38u32);
        xm1_power_record_helper_7547ac(r0, r1 as *const HeaderTriplet);
        r0 = read_volatile((r6) as *const u32) as u32;
        r1 = r4.wrapping_add(44u32);
        xm1_power_record_helper_75484c(r0, r1 as *const HeaderTriplet);
        r0 = read_volatile((r6) as *const u32) as u32;
        r1 = r4.wrapping_add(56u32);
        xm1_power_record_helper_754a9c(r0, r1 as *const ProfileWords2);
        r0 = read_volatile((r6) as *const u32) as u32;
        r1 = read_volatile((r4.wrapping_add(50u32)) as *const u16) as u32;
        xm1_power_record_helper_7548dc(r0, r1);
        r0 = read_volatile((r6) as *const u32) as u32;
        r1 = read_volatile((r4.wrapping_add(52u32)) as *const i16) as i32 as u32;
        xm1_power_record_helper_75496c(r0, r1);
        r0 = read_volatile((r6) as *const u32) as u32;
        r1 = read_volatile((r4.wrapping_add(54u32)) as *const i16) as i32 as u32;
        xm1_power_record_helper_7549fc(r0, r1);
        r0 = read_volatile((r6) as *const u32) as u32;
        xm1_power_record_helper_efa968(r0);
        r0 = read_volatile((r6) as *const u32) as u32;
        xm1_power_record_helper_efaa34(r0);
        r3 = read_volatile((r6.wrapping_add(0x4u32)) as *const u32) as u32;
        r3 = r3 & !10u32;
        write_volatile((r6.wrapping_add(0x4u32)) as *mut u32, r3 as u32);
        return;
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_f2ec94() -> u32 {
    let mut r0: u32 = 0;
    let mut r3: u32 = 0;
    unsafe {
        r3 = 0x79a8u32;
        r3 = (r3 & 0xffff) | (0x16bu32 << 16);
        r0 = read_volatile((r3.wrapping_add(0x4u32)) as *const u32) as u32;
        r0 = (r0 >> 0x2u32) & 1u32;
        return r0;
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_f2ecb8() {
    let mut frame = core::mem::MaybeUninit::<[u32; 6]>::uninit();
    let mut sp = (frame.as_mut_ptr() as usize as u32).wrapping_add(24);
    let mut r0: u32 = 0;
    let mut r1: u32 = 0;
    let mut r2: u32 = 0;
    let mut r3: u32 = 0;
    let mut r11: u32 = 0;
    unsafe {
        sp = sp.wrapping_sub(8);
        r11 = sp.wrapping_add(4u32);
        sp = sp.wrapping_sub(16u32);
        r3 = 0x79a8u32;
        r3 = (r3 & 0xffff) | (0x16bu32 << 16);
        r2 = r3;
        r1 = read_volatile((r2) as *const u32) as u32;r2 = r2.wrapping_add(16u32);
        write_volatile((r11.wrapping_add((-0x10i32 as u32))) as *mut u32, r1 as u32);
        write_volatile((r11.wrapping_add((-0xci32 as u32))) as *mut u32, r2 as u32);
        r3 = read_volatile((r3.wrapping_add(0x8u32)) as *const u32) as u32;
        write_volatile((r11.wrapping_add((-0x8i32 as u32))) as *mut u32, r3 as u32);
        r0 = 0xe6e4u32;
        r0 = xm1_power_record_helper_f2e6e4 as usize as u32;
        r1 = r11.wrapping_sub(16u32);
        xm1_power_record_helper_f4d350(r0, r1);
        sp = r11.wrapping_sub(4u32);
        return;
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_f2ecfc() {
    let mut r0: u32 = 0;
    unsafe {
        return;
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_f2ed10() {
    let mut r0: u32 = 0;
    let mut r1: u32 = 0;
    let mut r3: u32 = 0;
    let mut r4: u32 = 0;
    unsafe {
        r4 = 0x79a8u32;
        r4 = (r4 & 0xffff) | (0x16bu32 << 16);
        r3 = 0xd370u32;
        r3 = (r3 & 0xffff) | (0x169u32 << 16);
        r1 = read_volatile((r3.wrapping_add(0x10u32)) as *const u32) as u32;
        write_volatile((r4.wrapping_add(0xd8u32)) as *mut u32, r1 as u32);
        r0 = read_volatile((r4) as *const u32) as u32;
        xm1_power_record_helper_754b88(r0, r1);
        r0 = read_volatile((r4) as *const u32) as u32;
        xm1_power_record_helper_efa9f0(r0);
        r3 = read_volatile((r4.wrapping_add(0x4u32)) as *const u32) as u32;
        r3 = r3 & !4u32;
        write_volatile((r4.wrapping_add(0x4u32)) as *mut u32, r3 as u32);
        return;
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_f2ed50() -> u32 {
    let mut r0: u32 = 0;
    let mut r3: u32 = 0;
    unsafe {
        r3 = 0x79a8u32;
        r3 = (r3 & 0xffff) | (0x16bu32 << 16);
        r0 = read_volatile((r3.wrapping_add(0x4u32)) as *const u32) as u32;
        r0 = (r0 >> 0x4u32) & 1u32;
        return r0;
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_f2ed74() {
    let mut frame = core::mem::MaybeUninit::<[u32; 4]>::uninit();
    let mut sp = (frame.as_mut_ptr() as usize as u32).wrapping_add(16);
    let mut r0: u32 = 0;
    let mut r1: u32 = 0;
    let mut r2: u32 = 0;
    let mut r3: u32 = 0;
    let mut r11: u32 = 0;
    unsafe {
        sp = sp.wrapping_sub(8);
        r11 = sp.wrapping_add(4u32);
        sp = sp.wrapping_sub(8u32);
        r3 = 0x79a8u32;
        r3 = (r3 & 0xffff) | (0x16bu32 << 16);
        r2 = read_volatile((r3) as *const u32) as u32;r3 = r3.wrapping_add(16u32);
        write_volatile((r11.wrapping_add((-0xci32 as u32))) as *mut u32, r2 as u32);
        write_volatile((r11.wrapping_add((-0x8i32 as u32))) as *mut u32, r3 as u32);
        r0 = 0xe6b4u32;
        r0 = xm1_power_record_helper_f2e6b4 as usize as u32;
        r1 = r11.wrapping_sub(12u32);
        xm1_power_record_helper_f4d448(r0, r1);
        sp = r11.wrapping_sub(4u32);
        return;
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_f2edac() {
    let mut r0: u32 = 0;
    unsafe {
        return;
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_f2edc0() {
    let mut r0: u32 = 0;
    let mut r1: u32 = 0;
    let mut r2: u32 = 0;
    let mut r3: u32 = 0;
    let mut r4: u32 = 0;
    let mut r5: u32 = 0;
    let mut r12: u32 = 0;
    unsafe {
        r5 = 0x79a8u32;
        r5 = (r5 & 0xffff) | (0x16bu32 << 16);
        r4 = 0xd370u32;
        r4 = (r4 & 0xffff) | (0x169u32 << 16);
        r12 = r5.wrapping_add(220u32);
        r3 = r4.wrapping_add(16u32);
        { let transfer = r3; r0 = read_volatile(transfer.wrapping_add(0) as *const u32); r1 = read_volatile(transfer.wrapping_add(4) as *const u32); r2 = read_volatile(transfer.wrapping_add(8) as *const u32);  }
        { let transfer = r12; write_volatile(transfer.wrapping_add(0) as *mut u32, r0); write_volatile(transfer.wrapping_add(4) as *mut u32, r1); write_volatile(transfer.wrapping_add(8) as *mut u32, r2);  }
        r0 = read_volatile((r5) as *const u32) as u32;
        r1 = r3;
        xm1_power_record_helper_75263c(r0, r1 as *const HeaderTriplet);
        r0 = read_volatile((r5) as *const u32) as u32;
        r1 = read_volatile((r4.wrapping_add(0x18u32)) as *const u32) as u32;
        xm1_power_record_helper_7526cc(r0, r1);
        r0 = read_volatile((r5) as *const u32) as u32;
        xm1_power_record_helper_efaa80(r0);
        r3 = read_volatile((r5.wrapping_add(0x4u32)) as *const u32) as u32;
        r3 = r3 & !16u32;
        write_volatile((r5.wrapping_add(0x4u32)) as *mut u32, r3 as u32);
        return;
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_f2ee18() -> u32 {
    enum Flow { B0, B1, B2 }
    let mut r0: u32 = 0;
    let mut r3: u32 = 0;
    let mut equal = false;
    let mut lower_or_same = false;
    let mut lower = false;
    let mut next = Flow::B0;
    unsafe {
        loop {
            match next {
                Flow::B0 => {
                    r3 = 0x79a8u32;
                    r3 = (r3 & 0xffff) | (0x16bu32 << 16);
                    r3 = read_volatile((r3.wrapping_add(0x4u32)) as *const u32) as u32;
                    equal = (r3 & 256u32) == 0;
                    if equal { next = Flow::B2; } else { next = Flow::B1; }
                    continue;
                }
                Flow::B1 => {
                    r0 = 23711900u32;
                    r0 = read_volatile((r0.wrapping_add(0x20u32)) as *const u32) as u32;
                    r0 = r0.wrapping_add(0u32); equal = r0 == 0;
                    if !equal { r0 = 1u32; }
                    return r0;
                }
                Flow::B2 => {
                    r0 = 0u32;
                    return r0;
                }
            }
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_f2ee50() {
    let mut frame = core::mem::MaybeUninit::<[u32; 6]>::uninit();
    let mut sp = (frame.as_mut_ptr() as usize as u32).wrapping_add(24);
    let mut r0: u32 = 0;
    let mut r1: u32 = 0;
    let mut r2: u32 = 0;
    let mut r3: u32 = 0;
    let mut r11: u32 = 0;
    unsafe {
        sp = sp.wrapping_sub(8);
        r11 = sp.wrapping_add(4u32);
        sp = sp.wrapping_sub(16u32);
        r3 = 0x79a8u32;
        r3 = (r3 & 0xffff) | (0x16bu32 << 16);
        r2 = r3;
        r1 = read_volatile((r2) as *const u32) as u32;r2 = r2.wrapping_add(16u32);
        write_volatile((r11.wrapping_add((-0x10i32 as u32))) as *mut u32, r1 as u32);
        write_volatile((r11.wrapping_add((-0xci32 as u32))) as *mut u32, r2 as u32);
        r3 = read_volatile((r3.wrapping_add(0x8u32)) as *const u32) as u32;
        write_volatile((r11.wrapping_add((-0x8i32 as u32))) as *mut u32, r3 as u32);
        r0 = 0xe67cu32;
        r0 = xm1_power_record_helper_f2e67c as usize as u32;
        r1 = r11.wrapping_sub(16u32);
        xm1_power_record_helper_f52120(r0, r1);
        sp = r11.wrapping_sub(4u32);
        return;
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_f2ee94() {
    let mut r0: u32 = 0;
    unsafe {
        return;
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_f2eea8() {
    let mut r0: u32 = 0;
    let mut r1: u32 = 0;
    let mut r3: u32 = 0;
    let mut r4: u32 = 0;
    let mut r5: u32 = 0;
    unsafe {
        r5 = 0x79a8u32;
        r5 = (r5 & 0xffff) | (0x16bu32 << 16);
        r4 = 0xd370u32;
        r4 = (r4 & 0xffff) | (0x169u32 << 16);
        r3 = r4.wrapping_add(16u32);
        { let transfer = r3; r0 = read_volatile(transfer.wrapping_add(0) as *const u32); r1 = read_volatile(transfer.wrapping_add(4) as *const u32);  }
        write_volatile((r5.wrapping_add(0xe8u32)) as *mut u32, r0 as u32);
        write_volatile((r5.wrapping_add(236u32)) as *mut u16, r1 as u16);
        r0 = read_volatile((r5) as *const u32) as u32;
        r1 = read_volatile((r4.wrapping_add(0x10u32)) as *const u8) as u32;
        xm1_power_record_helper_754c20(r0, r1);
        r0 = read_volatile((r5) as *const u32) as u32;
        r1 = read_volatile((r4.wrapping_add(18u32)) as *const u16) as u32;
        xm1_power_record_helper_754cb8(r0, r1);
        r0 = read_volatile((r5) as *const u32) as u32;
        r1 = read_volatile((r4.wrapping_add(20u32)) as *const u16) as u32;
        xm1_power_record_helper_754d50(r0, r1);
        r0 = read_volatile((r5) as *const u32) as u32;
        xm1_power_record_helper_efaacc(r0);
        r3 = read_volatile((r5.wrapping_add(0x4u32)) as *const u32) as u32;
        r3 = r3 & !256u32;
        write_volatile((r5.wrapping_add(0x4u32)) as *mut u32, r3 as u32);
        return;
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_f2e6b4(argument0: u32) {
    let mut frame = core::mem::MaybeUninit::<[u32; 6]>::uninit();
    let mut sp = (frame.as_mut_ptr() as usize as u32).wrapping_add(24);
    let mut r0: u32 = argument0;
    let mut r1: u32 = 0;
    let mut r2: u32 = 0;
    let mut r3: u32 = 0;
    let mut r11: u32 = 0;
    unsafe {
        sp = sp.wrapping_sub(8);
        r11 = sp.wrapping_add(4u32);
        sp = sp.wrapping_sub(16u32);
        r3 = r11.wrapping_sub(16u32);
        { let transfer = r0; r0 = read_volatile(transfer.wrapping_add(0) as *const u32); r1 = read_volatile(transfer.wrapping_add(4) as *const u32); r2 = read_volatile(transfer.wrapping_add(8) as *const u32);  }
        { let transfer = r3; write_volatile(transfer.wrapping_add(0) as *mut u32, r0); write_volatile(transfer.wrapping_add(4) as *mut u32, r1); write_volatile(transfer.wrapping_add(8) as *mut u32, r2);  }
        r0 = 153u32;
        r1 = r3;
        r2 = 12u32;
        xm1_power_record_helper_ef9a94(r0, r1, r2);
        sp = r11.wrapping_sub(4u32);
        return;
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_f2e6e4(argument0: u32) {
    let mut frame = core::mem::MaybeUninit::<[u32; 4]>::uninit();
    let mut sp = (frame.as_mut_ptr() as usize as u32).wrapping_add(16);
    let mut r0: u32 = argument0;
    let mut r1: u32 = 0;
    let mut r2: u32 = 0;
    let mut r3: u32 = 0;
    let mut r11: u32 = 0;
    unsafe {
        sp = sp.wrapping_sub(8);
        r11 = sp.wrapping_add(4u32);
        sp = sp.wrapping_sub(8u32);
        r3 = read_volatile((r0) as *const u32) as u32;
        r1 = r11.wrapping_sub(4u32);
        write_volatile((r1.wrapping_add((-0x4i32 as u32))) as *mut u32, r3 as u32);r1 = r1.wrapping_add((-0x4i32 as u32));
        r0 = 152u32;
        r2 = 4u32;
        xm1_power_record_helper_ef9a94(r0, r1, r2);
        sp = r11.wrapping_sub(4u32);
        return;
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_f2e710(argument0: u32) {
    let mut frame = core::mem::MaybeUninit::<[u32; 14]>::uninit();
    let mut sp = (frame.as_mut_ptr() as usize as u32).wrapping_add(56);
    let mut lr: u32 = 0;
    let mut r0: u32 = argument0;
    let mut r1: u32 = 0;
    let mut r2: u32 = 0;
    let mut r3: u32 = 0;
    let mut r11: u32 = 0;
    let mut r12: u32 = 0;
    unsafe {
        sp = sp.wrapping_sub(8);
        r11 = sp.wrapping_add(4u32);
        sp = sp.wrapping_sub(48u32);
        r12 = r11.wrapping_sub(52u32);
        lr = r0;
        { let transfer = lr; r0 = read_volatile(transfer.wrapping_add(0) as *const u32); r1 = read_volatile(transfer.wrapping_add(4) as *const u32); r2 = read_volatile(transfer.wrapping_add(8) as *const u32); r3 = read_volatile(transfer.wrapping_add(12) as *const u32); lr = transfer.wrapping_add(16); }
        { let transfer = r12; write_volatile(transfer.wrapping_add(0) as *mut u32, r0); write_volatile(transfer.wrapping_add(4) as *mut u32, r1); write_volatile(transfer.wrapping_add(8) as *mut u32, r2); write_volatile(transfer.wrapping_add(12) as *mut u32, r3); r12 = transfer.wrapping_add(16); }
        { let transfer = lr; r0 = read_volatile(transfer.wrapping_add(0) as *const u32); r1 = read_volatile(transfer.wrapping_add(4) as *const u32); r2 = read_volatile(transfer.wrapping_add(8) as *const u32); r3 = read_volatile(transfer.wrapping_add(12) as *const u32); lr = transfer.wrapping_add(16); }
        { let transfer = r12; write_volatile(transfer.wrapping_add(0) as *mut u32, r0); write_volatile(transfer.wrapping_add(4) as *mut u32, r1); write_volatile(transfer.wrapping_add(8) as *mut u32, r2); write_volatile(transfer.wrapping_add(12) as *mut u32, r3); r12 = transfer.wrapping_add(16); }
        { let transfer = lr; r0 = read_volatile(transfer.wrapping_add(0) as *const u32); r1 = read_volatile(transfer.wrapping_add(4) as *const u32); r2 = read_volatile(transfer.wrapping_add(8) as *const u32); r3 = read_volatile(transfer.wrapping_add(12) as *const u32);  }
        { let transfer = r12; write_volatile(transfer.wrapping_add(0) as *mut u32, r0); write_volatile(transfer.wrapping_add(4) as *mut u32, r1); write_volatile(transfer.wrapping_add(8) as *mut u32, r2); write_volatile(transfer.wrapping_add(12) as *mut u32, r3);  }
        r0 = 151u32;
        r1 = r11.wrapping_sub(52u32);
        r2 = 48u32;
        xm1_power_record_helper_ef9a94(r0, r1, r2);
        sp = r11.wrapping_sub(4u32);
        return;
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_f2e67c(argument0: u32) {
    let mut frame = core::mem::MaybeUninit::<[u32; 6]>::uninit();
    let mut sp = (frame.as_mut_ptr() as usize as u32).wrapping_add(24);
    let mut r0: u32 = argument0;
    let mut r1: u32 = 0;
    let mut r2: u32 = 0;
    let mut r4: u32 = 0;
    let mut r11: u32 = 0;
    unsafe {
        sp = sp.wrapping_sub(12);
        r11 = sp.wrapping_add(8u32);
        sp = sp.wrapping_sub(12u32);
        r1 = r0;
        r4 = 6u32;
        r0 = r11.wrapping_sub(20u32);
        r2 = r4;
        core::ptr::copy_nonoverlapping(r1 as *const u8, r0 as *mut u8, r2 as usize);
        r0 = 154u32;
        r1 = r11.wrapping_sub(20u32);
        r2 = r4;
        xm1_power_record_helper_ef9a94(r0, r1, r2);
        sp = r11.wrapping_sub(8u32);
        return;
    }
}
