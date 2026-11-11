//! x70view 55436fc85c0560dd252b0410af6f2427fe198cd3, AF secondary profile helpers.
//! NONMATCHING layouts and argument flow are confirmed against image5 f2ef0c..f2f568.

use core::mem::MaybeUninit;
use core::ptr::{read_volatile, write_volatile};

#[derive(Clone, Copy)]
struct Address(u32);
impl Address {
    unsafe fn word(self, offset: u32) -> u32 { read_volatile(self.0.wrapping_add(offset) as *const u32) }
    unsafe fn byte(self, offset: u32) -> u8 { read_volatile(self.0.wrapping_add(offset) as *const u8) }
    unsafe fn half(self, offset: u32) -> u16 { read_volatile(self.0.wrapping_add(offset) as *const u16) }
    unsafe fn put(self, offset: u32, value: u32) { write_volatile(self.0.wrapping_add(offset) as *mut u32, value); }
    unsafe fn put_half(self, offset: u32, value: u16) { write_volatile(self.0.wrapping_add(offset) as *mut u16, value); }
}
const SECONDARY: Address = Address(0x016b_7a98);
const REFERENCES: Address = Address(0x016b_dfe4);
const LATCH: Address = Address(0x016b_e000);
const OWNER: Address = Address(0x0169_d09c);

extern "C" {
    fn xm1_power_record_helper_ef9a94(event: u32, payload: u32, size: u32);
    fn xm1_power_recording_capture_trace_begin(event: u32);
    fn xm1_power_record_helper_f1fc5c(destination: u32, context: u32);
    fn xm1_power_record_helper_f2a200(mode: u32, count: u32) -> u32;
    fn xm1_power_record_helper_75a564(profile: u32, value: u32);
    fn xm1_power_record_helper_75a5ac(profile: u32, count: u32);
    fn xm1_power_record_helper_75a5f4(profile: u32, source: u32);
    fn xm1_power_record_helper_752928(destination: u32, profile: u32, index: u32);
    fn xm1_power_record_helper_75ae78(destination: u32, source: u32);
    fn xm1_power_record_helper_74f784(profile: u32) -> u32;
    fn xm1_power_record_helper_74f6fc(profile: u32) -> u32;
    fn xm1_power_record_helper_74f5ec(profile: u32) -> u32;
    fn xm1_power_record_helper_76d9c8() -> u32;
    fn xm1_power_record_helper_75a4ac(profile: u32, selector: u32, destination: u32);
    fn xm1_power_record_helper_746c2c(value: u32) -> u32;
    fn xm1_power_record_helper_74714c(first: u32, second: u32, current: u32) -> u32;
    fn xm1_power_record_helper_746944(value: u32, mode: u32) -> u32;
    fn xm1_power_record_helper_f601fc(context: u32, buffer: u32, callback: unsafe extern "C" fn(u32));
    fn xm1_power_record_helper_76dbb4() -> u32;
    fn xm1_power_record_helper_75c874(profile: u32) -> u32;
    fn xm1_power_record_helper_76e654(callback: unsafe extern "C" fn(), value: u32, a: u32, b: u32, c: u32, d: u32) -> i32;
    fn xm1_power_record_helper_75093c(profile: u32) -> u32;
    fn xm1_power_record_helper_76da84() -> u32;
    fn xm1_power_record_helper_76dad4() -> u32;
    fn xm1_power_record_helper_74947c(a: u32, b: u32, c: u32, d: u32, e: u32, f: u32, g: u32) -> u32;
    fn xm1_power_record_helper_76e1ac(callback: unsafe extern "C" fn(), delta: i32, value: u32, a: u32, b: u32, c: u32, d: u32) -> i32;
    fn xm1_power_record_helper_74e7f0(profile: u32) -> u32;
    fn xm1_power_record_helper_752a98(profile: u32, index: u32, value: u32);
    fn xm1_power_record_helper_752b48(profile: u32, index: u32, value: i32);
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_f1d0d8(destination: u32) {
    core::ptr::copy(0x016a_0500 as *const u8, destination as *mut u8, 0x640c);
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_f1d680(source: u32) {
    core::ptr::copy(source as *const u8, 0x016a_d744 as *mut u8, 0x640c);
}

unsafe extern "C" fn secondary_reference_ready(source: u32) {
    xm1_power_record_helper_f1d680(source);
    xm1_power_record_helper_ef9a94(0x90, 0, 0);
}

unsafe extern "C" fn secondary_timing_ready() {
    xm1_power_record_helper_ef9a94(0x3a, 0, 0);
    xm1_power_recording_capture_trace_begin(0x1303_0100);
}

unsafe extern "C" fn secondary_delta_ready() {
    xm1_power_record_helper_ef9a94(0x35, 0, 0);
    xm1_power_recording_capture_trace_begin(0x1303_0000);
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_f2ef7c(request: u32) {
    SECONDARY.put(0, request);
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_f2ef9c() {
    let profile = SECONDARY.word(0);
    let owner = OWNER;
    xm1_power_record_helper_75a564(profile, Address(owner.word(0xc)).word(0x50));
    xm1_power_record_helper_75a5ac(profile, Address(owner.word(0x38)).byte(1) as u32);
    xm1_power_record_helper_75a5f4(profile, owner.word(0x14));
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_f2efe8() {
    xm1_power_record_helper_f1d0d8(SECONDARY.0 + 0x130);
    xm1_power_record_helper_f1d680(SECONDARY.0 + 0x130);
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_f2f054() {
    let profile = SECONDARY.word(0);
    let owner = OWNER;
    let selection = Address(owner.word(0x38));
    if xm1_power_record_helper_f2a200(selection.byte(0) as u32, selection.byte(1) as u32) != 1 { return; }
    let mut index = 0u8;
    if Address(owner.word(0x38)).byte(1) == 0 { return; }
    let mut buffer = [MaybeUninit::<u32>::uninit(); 42];
    loop {
        xm1_power_record_helper_752928(buffer.as_mut_ptr() as u32, profile, index as u32);
        let destination = Address(owner.word(0x14)).word(index as u32 * 4);
        xm1_power_record_helper_75ae78(destination, buffer.as_ptr() as u32);
        index = index.wrapping_add(1);
        if index >= Address(owner.word(0x38)).byte(1) { break; }
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_f2f0f4() {
    let profile = SECONDARY.word(0);
    let value90 = xm1_power_record_helper_74f784(profile);
    let value8c = xm1_power_record_helper_74f6fc(profile);
    let value84 = xm1_power_record_helper_74f5ec(profile);
    let current = xm1_power_record_helper_76d9c8();
    xm1_power_record_helper_f1fc5c(SECONDARY.0 + 4, current);
    let mut pair = [MaybeUninit::<u32>::uninit(); 2];
    xm1_power_record_helper_75a4ac(profile, 4, pair.as_mut_ptr() as u32);
    let first = pair[0].assume_init();
    let second = pair[1].assume_init();
    let delta = xm1_power_record_helper_746c2c(current).wrapping_sub(xm1_power_record_helper_746c2c(first)) as u16;
    let adjusted = xm1_power_record_helper_74714c(first, second, current);
    let reference_delta = xm1_power_record_helper_746944(second, 0).wrapping_sub(xm1_power_record_helper_746944(adjusted, 0));
    let delta = delta.wrapping_sub(reference_delta as u16);
    for offset in [0x3c, 0x108, 0x10a, 0x10c, 0x10e, 0x110, 0x112, 0x114, 0x116, 0x118] {
        SECONDARY.put_half(offset, SECONDARY.half(offset).wrapping_add(delta));
    }
    SECONDARY.put(0x90, value90);
    SECONDARY.put_half(0x8c, value8c as u16);
    SECONDARY.put_half(0x84, value84 as u16);
    xm1_power_record_helper_f601fc(SECONDARY.0 + 4, SECONDARY.0 + 0x130, secondary_reference_ready);
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_f2f278() -> i32 {
    let owner = OWNER;
    let current = xm1_power_record_helper_76dbb4();
    SECONDARY.put(0x653c, current);
    let saved = xm1_power_record_helper_75c874(Address(owner.word(0x14)).word(0));
    SECONDARY.put(0x6540, saved);
    i32::from(saved != SECONDARY.word(0x653c))
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_f2f2c0() {
    xm1_power_recording_capture_trace_begin(0x0303_0100);
    xm1_power_record_helper_76e654(secondary_timing_ready, SECONDARY.word(0x6540), 1, 1, 0, 4);
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_f2f314() {
    let enabled = u32::from(xm1_power_record_helper_75093c(SECONDARY.word(0)) == 1);
    let first = xm1_power_record_helper_76da84();
    let second = xm1_power_record_helper_76d9c8();
    let third = xm1_power_record_helper_76dbb4();
    let saved = SECONDARY.word(0x6540);
    let rate = xm1_power_record_helper_76dad4();
    let target = xm1_power_record_helper_74947c(first, second, third, saved, enabled, enabled, rate);
    SECONDARY.put_half(0x6544, target.wrapping_sub(first) as u16);
    SECONDARY.put(0x6548, xm1_power_record_helper_76dad4());
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_f2f3a0() {
    xm1_power_recording_capture_trace_begin(0x0303_0000);
    xm1_power_record_helper_76e1ac(secondary_delta_ready, SECONDARY.half(0x6544) as i16 as i32, SECONDARY.word(0x6548), 1, 0, 5, 4);
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_f2f4a0() -> i32 {
    let profile = SECONDARY.word(0);
    let owner = OWNER;
    let saved = LATCH.word(0);
    Address(0x016b_dffc).put(0, saved);
    if saved == 1 { return 1; }
    if xm1_power_record_helper_74e7f0(profile) != 13 { return 0; }
    if REFERENCES.word(0) == 0 { return 1; }
    if Address(owner.word(0x38)).byte(1) == 0 { return 0; }
    let mut index = 0u8;
    loop {
        xm1_power_record_helper_752a98(profile, index as u32, REFERENCES.word(4 + index as u32 * 4));
        xm1_power_record_helper_752b48(profile, index as u32, REFERENCES.half(16 + index as u32 * 2) as i16 as i32);
        index = index.wrapping_add(1);
        if index >= Address(owner.word(0x38)).byte(1) { break; }
    }
    0
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_f2f460() -> u32 {
    Address(0x016b_dffc).word(0)
}
