//! Recording resource banking and completion state from the pinned X-M1 1.50 instructions.

use core::ptr::{read_volatile, write_volatile};

unsafe extern "C" {
    fn xm1_power_record_helper_756720();
    fn xm1_power_record_helper_75a63c(kind: u32, descriptor: u32) -> u32;
    fn xm1_power_record_helper_75a960(first: u32, second: u32) -> u32;
    fn xm1_power_record_helper_75a7c4(resource: u32, count: u32, kind: u32, source: u32) -> u32;
    fn xm1_power_record_helper_75a6cc(resource: u32);
    fn xm1_power_record_helper_74e074(resource: u32, header: u32);
    fn xm1_power_record_helper_74e0b8(resource: u32, value: u32);
    fn xm1_power_record_helper_74e108(resource: u32, value: u32);
    fn xm1_power_record_helper_74e154(resource: u32, value: u32);
    fn xm1_power_record_helper_759250(resource: u32, header: u32);
    fn xm1_power_record_helper_750b04(resource: u32, value: u32);
    fn xm1_power_record_helper_740a10();
    fn xm1_power_record_helper_7405dc();
    fn xm1_power_record_helper_74f344(resource: u32);
    fn xm1_power_record_helper_740cf8();
    fn xm1_power_record_helper_74e648(resource: u32);
    fn xm1_power_record_helper_740cc4();
    fn xm1_power_record_helper_74f894(resource: u32);
    fn xm1_power_record_helper_740b80();
    fn xm1_power_record_helper_74f784(resource: u32);
    fn xm1_power_record_helper_740bd4();
    fn xm1_power_record_helper_76d9c8();
    fn xm1_power_record_helper_740b18();
    fn xm1_power_record_helper_740a34();
    fn xm1_power_record_helper_740a58();
    fn xm1_power_record_helper_76d9a8() -> u32;
    fn xm1_power_record_helper_76da38() -> u32;
    fn xm1_power_record_helper_76dba4() -> u32;
    fn xm1_power_record_helper_76da48() -> u32;
    fn xm1_power_record_helper_efb094(status: u32, mode: u32, changed: u32, channels: u32);
    fn xm1_power_append_stream_text(task: u32, text: u32);
}

fn word(address: usize) -> u32 { unsafe { read_volatile(address as *const u32) } }
fn put(address: usize, value: u32) { unsafe { write_volatile(address as *mut u32, value) }; }

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_f14668() {
    if word(0x954dcc) != 2 { unsafe { xm1_power_record_helper_756720() }; }
    for slot in 0..9 { put(0x169cf00 + slot * 4, u32::MAX); }
    for slot in 0..3 { put(0x954dc0 + slot * 4, 0); }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_f146c0() {
    unsafe { super::xm1_power_record_task43_complete0(0) };
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_f14704() {
    unsafe { super::xm1_power_record_task43_complete1(0) };
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_f14718() { put(0x169d500, 1); }
#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_f1473c() { put(0x169d500, 0); }
#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_f14760() -> u32 { word(0x169d500) }
#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_f148fc() { put(0x169d504, 1); }
#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_f14920() { put(0x169d504, 0); }
#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_f14944() -> u32 { word(0x169d504) }

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_f147bc(resource: u32, event: u32) {
    unsafe {
        if event == 0 {
            xm1_power_record_helper_74e074(resource, 0x169d388);
            xm1_power_record_helper_74e0b8(resource, word(0x169d418));
            xm1_power_record_helper_74e108(resource, word(0x169d41c));
            xm1_power_record_helper_74e154(resource, word(0x169d420));
        } else if event == 13 {
            xm1_power_record_helper_759250(resource, 0x169d388);
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_f14964(resource: u32) {
    unsafe { xm1_power_record_helper_750b04(resource, u32::from(word(0x169d328) != 0)) };
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_f14990(resource: u32) {
    unsafe {
        xm1_power_record_helper_740a10();
        xm1_power_record_helper_7405dc();
        xm1_power_record_helper_74f344(resource);
        xm1_power_record_helper_740cf8();
        xm1_power_record_helper_74e648(resource);
        xm1_power_record_helper_740cc4();
        xm1_power_record_helper_74f894(resource);
        xm1_power_record_helper_740b80();
        xm1_power_record_helper_74f784(resource);
        xm1_power_record_helper_740bd4();
        xm1_power_record_helper_76d9c8();
        xm1_power_record_helper_740b18();
        xm1_power_record_helper_740a34();
        xm1_power_record_helper_740a58();
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_f149e8(bank: u32, kind: u32, event: u32) -> u32 {
    if bank > 3 { return u32::MAX; }
    let slot = 0x169cf00 + bank as usize * 4;
    let previous = word(slot);
    if kind == 3 && previous != u32::MAX {
        put(slot + 20, previous);
        put(slot, u32::MAX);
    }
    if word(slot) == u32::MAX {
        put(slot, unsafe { xm1_power_record_helper_75a63c(kind, 0x81aa6c) });
    }
    let source = unsafe { xm1_power_record_helper_75a960(1, 1) };
    let resource = unsafe { xm1_power_record_helper_75a7c4(word(slot), 63, 3, source) };
    unsafe { xm1_power_record_helper_f147bc(resource, event) };
    resource
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_f14aac(bank: u32) {
    let slot = 0x169cf00 + bank as usize * 4;
    let resource = word(slot);
    if resource == u32::MAX { return; }
    unsafe { xm1_power_record_helper_75a6cc(resource) };
    put(slot, u32::MAX);
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_task43_log_backed(application: *const [u8; 76], text: u32) {
    unsafe {
        crate::record_task43_application::xm1_power_record_helper_72b680(application as u32);
        xm1_power_append_stream_text(2, text);
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_f46c4c(status: u32) {
    let first = unsafe { xm1_power_record_helper_76d9a8() };
    let second = unsafe { xm1_power_record_helper_76da38() };
    let third = unsafe { xm1_power_record_helper_76dba4() };
    let mode = unsafe { xm1_power_record_helper_76da48() };
    let channels = u32::from(first == 2) * 2 | u32::from(second == 2) | u32::from(third == 2) * 4;
    let changed = u32::from(word(0x954dc0) == 1 || word(0x954dc4) == 1 || word(0x954dc8) == 1);
    put(0x954dc0, 0);
    put(0x954dc4, 0);
    put(0x954dc8, 0);
    unsafe { xm1_power_record_helper_efb094(status, mode, changed, channels) };
}
