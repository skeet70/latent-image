//! Resource initialization passes the native frame's unwritten window word and mode halfword through to the consumers.

use core::ptr::{read_volatile, write_volatile};

unsafe extern "C" {
    fn xm1_power_record_helper_fea428();
    fn xm1_power_record_helper_f4c3b8();
    fn xm1_power_record_helper_10546e0();
    fn xm1_power_record_helper_1039610(callback: u32);
    fn xm1_power_record_helper_76d950();
    fn xm1_power_record_helper_1050fa4();
    fn xm1_power_record_helper_1051fcc();
    fn xm1_power_record_helper_ed89b0();
    fn xm1_power_record_helper_756f10(resource: u32, mode: u32);
    fn xm1_power_record_helper_76d8f8();
    fn xm1_power_record_helper_ed87b0();
    fn xm1_power_record_helper_75e6c4();
    fn xm1_power_record_helper_fea418(window: u32);
    fn xm1_power_record_helper_f4c384();
    fn xm1_power_record_helper_10546d0();
    fn xm1_power_record_helper_1039580(window: u32, callback: u32);
    fn xm1_power_record_helper_1050f8c();
    fn xm1_power_record_helper_1051fb4();
}

fn word(address: usize) -> u32 { unsafe { read_volatile(address as *const u32) } }

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_f146d4() {
    unsafe {
        xm1_power_record_helper_fea428();
        xm1_power_record_helper_f4c3b8();
        xm1_power_record_helper_10546e0();
        xm1_power_record_helper_1039610(0);
        xm1_power_record_helper_76d950();
        xm1_power_record_helper_1050fa4();
        xm1_power_record_helper_1051fcc();
        xm1_power_record_helper_ed89b0();
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_task43_initialize_backed(resource: u32, mode: u32, first_window: *mut u32, second_window: *mut u32) {
    unsafe { xm1_power_record_helper_756f10(resource, mode) };
    if word(0x954dcc) != 2 { unsafe { xm1_power_record_helper_76d8f8() }; }
    unsafe {
        xm1_power_record_helper_ed87b0();
        xm1_power_record_helper_75e6c4();
    }
    let profile = word(0x169cff8) as usize;
    unsafe {
        write_volatile(first_window, word(profile + 24));
        write_volatile(first_window.add(1), word(profile + 32));
        write_volatile(first_window.add(2), word(profile + 28));
        write_volatile(first_window.add(3), word(profile + 36));
        xm1_power_record_helper_fea418(first_window as u32);
        xm1_power_record_helper_f4c384();
        xm1_power_record_helper_10546d0();
    }
    let profile = word(0x169cff8) as usize;
    unsafe {
        write_volatile(second_window, word(profile));
        write_volatile(second_window.add(1), word(profile + 16));
        write_volatile(second_window.add(2), 0);
        write_volatile(second_window.add(4), word(profile + 4));
        write_volatile(second_window.add(5), word(profile + 20));
        xm1_power_record_helper_1039580(second_window as u32, 0);
        xm1_power_record_helper_1050f8c();
        xm1_power_record_helper_1051fb4();
        super::common::xm1_power_record_helper_f147bc(resource, 0);
    }
}
