//! X-M1 1.50: x70view 55436fc85c0560dd252b0410af6f2427fe198cd3; id5 f3af14..f3b568.

use core::ptr::{read_volatile, write_volatile};

unsafe extern "C" {
    fn xm1_power_record_helper_1039dd4(stop: u32, pending: u32, callback: unsafe extern "C" fn(u32));
    fn xm1_power_record_helper_103a1a8(alternate: u32, callback: unsafe extern "C" fn(u32));
    fn xm1_power_record_helper_f3a9f0(status: u32);
    fn xm1_power_record_helper_f3a978(status: u32);
    fn xm1_power_record_helper_7580ec(resource: u32) -> u32;
    fn xm1_power_record_helper_6a7de0(value: u32);
    fn xm1_power_record_helper_756b54(resource: u32) -> u32;
    fn xm1_power_record_helper_f48f1c(resource: u32);
    fn xm1_power_record_helper_f48e74(resource: u32);
    fn xm1_power_record_helper_f525ec(resource: u32);
    fn xm1_power_record_helper_f48f54(resource: u32);
    fn xm1_power_record_helper_f4929c(resource: u32);
    fn xm1_power_record_helper_f60ab8(resource: u32);
    fn xm1_power_record_helper_69f8fc(key: u32) -> u32;
    fn xm1_power_record_helper_752714(resource: u32) -> u32;
    fn xm1_power_record_helper_7554ac(resource: u32) -> u32;
    fn xm1_power_record_helper_efaf24(status: u32, descriptor: *const u8);
    fn xm1_power_record_helper_efb224(status: u32, value: u32);
    fn xm1_power_record_helper_efb500(status: u32);
    fn xm1_power_record_helper_efb548(status: u32);
    fn xm1_power_record_helper_efb6f4(status: u32);
    fn xm1_power_record_helper_efb73c(status: u32);
    fn xm1_power_record_helper_efb784(status: u32);
    fn xm1_power_record_helper_efb7cc(status: u32);
    fn xm1_power_record_helper_efb814(status: u32);
    fn xm1_power_record_helper_efb85c(status: u32);
    fn xm1_power_record_helper_efb8a4(status: u32);
    fn xm1_power_record_helper_efb298(command: u32);
    fn xm1_power_record_helper_efb2e0(command: u32);
    fn xm1_power_record_helper_efaf74(command: u32);
    fn xm1_power_record_helper_efb590(command: u32, value: u32);
    fn xm1_power_record_helper_103a09c(value: u32);
    fn xm1_power_record_helper_103a0f0();
    fn xm1_power_record_helper_103a200(callback: unsafe extern "C" fn(u32));
    fn xm1_power_record_helper_f3a8d4(status: u32);
    fn xm1_power_record_helper_7589c0(resource: u32);
    fn xm1_power_record_helper_edb554();
    fn xm1_power_record_helper_74ec30(resource: u32) -> u32;
    fn xm1_power_record_helper_74eba8(resource: u32) -> u32;
    fn xm1_power_record_helper_dca798(output: *mut u32, mode: u32, dimensions: *const u32);
}

unsafe fn resource() -> u32 { unsafe { read_volatile(0x016c0718 as *const u32) } }

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f3af14(value: u32) {
    unsafe {
        write_volatile(0x016c0720 as *mut u32, 0);
        let pending = read_volatile(0x016c0724 as *const u32);
        xm1_power_record_helper_1039dd4(u32::from(value != 1), u32::from(pending != 0), xm1_power_record_helper_f3a9f0);
        match xm1_power_record_helper_7580ec(resource()) {
            3 => xm1_power_record_helper_6a7de0(0),
            4 => xm1_power_record_helper_6a7de0(1),
            _ => {}
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f3af90(value: u32) {
    unsafe {
        write_volatile(0x016c0720 as *mut u32, 0);
        xm1_power_record_helper_103a1a8(u32::from(value == 2), xm1_power_record_helper_f3a978);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f3afc4() {
    unsafe {
        let saved = xm1_power_record_helper_756b54(resource());
        if saved != 0 {
            xm1_power_record_helper_f48f1c(saved);
            xm1_power_record_helper_f48e74(saved);
            xm1_power_record_helper_f525ec(saved);
            xm1_power_record_helper_f48f54(saved);
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f3b008() {
    unsafe { let saved = xm1_power_record_helper_756b54(resource()); if saved != 0 { xm1_power_record_helper_f4929c(saved); } }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f3b030() {
    unsafe { let saved = xm1_power_record_helper_756b54(resource()); if saved != 0 { xm1_power_record_helper_f60ab8(saved); } }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f3b0fc() {}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f3b150() {
    unsafe { xm1_power_record_helper_103a09c(read_volatile(0x0169d204 as *const u32)); }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f3b168() { unsafe { xm1_power_record_helper_103a0f0(); } }

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f3b178() { unsafe { xm1_power_record_helper_103a200(xm1_power_record_helper_f3a8d4); } }

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f3b190(value: u32) {
    unsafe {
        let status = if value == 0 { 0 } else { (-94i32) as u32 };
        match read_volatile(0x016c0714 as *const u32) {
            10 => xm1_power_record_helper_efaf24(status, core::ptr::null()),
            17 => xm1_power_record_helper_efb224(status, xm1_power_record_helper_752714(resource())),
            24 => xm1_power_record_helper_efb500(status),
            25 => xm1_power_record_helper_efb548(status),
            28 => xm1_power_record_helper_efb6f4(status),
            29 => xm1_power_record_helper_efb73c(status),
            30 => xm1_power_record_helper_efb784(status),
            31 => xm1_power_record_helper_efb7cc(status),
            32 => xm1_power_record_helper_efb814(status),
            33 => xm1_power_record_helper_efb85c(status),
            34 => xm1_power_record_helper_efb8a4(status),
            177 => {
                if xm1_power_record_helper_69f8fc(0x65a8) & 1 != 0 && xm1_power_record_helper_69f8fc(0xa8) & 0x80 != 0 { xm1_power_record_helper_f3b030(); }
                if xm1_power_record_helper_69f8fc(0x24c) != 0 && xm1_power_record_helper_69f8fc(0xa8) & 0x80 != 0 { xm1_power_record_helper_f3afc4(); }
                xm1_power_record_helper_f3b008();
            }
            _ => {}
        }
    }
}

unsafe fn dispatch(event: u32, command: u32) {
    unsafe {
        match event {
            18 => xm1_power_record_helper_efb298(command),
            19 => xm1_power_record_helper_efb2e0(command),
            10 | 177 => xm1_power_record_helper_efaf74(command),
            _ => {}
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f3b318(event: u32) { unsafe { dispatch(event, 20); } }
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f3b370(event: u32) { unsafe { dispatch(event, 21); } }
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f3b3c8() { unsafe { xm1_power_record_helper_efb224(0, xm1_power_record_helper_752714(resource())); } }
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f3b3f4() { unsafe { xm1_power_record_helper_efb224(18, xm1_power_record_helper_752714(resource())); } }
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f3b420() { unsafe { xm1_power_record_helper_efb590(19, xm1_power_record_helper_7554ac(resource())); } }
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f3b448() { unsafe { xm1_power_record_helper_efaf24(16, core::ptr::null()); } }

#[repr(C, align(4))]
struct SavedSelection { words: [u32; 6], row: [u8; 40], callback: [u8; 0x414] }

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f3b460() {
    unsafe {
        let mut descriptor = SavedSelection { words: [0; 6], row: [0; 40], callback: [0; 0x414] };
        for index in 0..3 { descriptor.words[index] = read_volatile((0x0169d384usize + index * 4) as *const u32); }
        descriptor.words[5] = read_volatile(0x0169d390 as *const u32);
        core::ptr::copy_nonoverlapping(0x0169d394 as *const u8, descriptor.row.as_mut_ptr(), 40);
        let enabled = read_volatile(0x0169d3bc as *const u32);
        core::ptr::copy_nonoverlapping(0x016c0728 as *const u8, descriptor.callback.as_mut_ptr(), 0x414);
        if enabled == 1 { xm1_power_record_helper_7589c0(resource()); xm1_power_record_helper_edb554(); }
        let dimensions = xm1_power_record_helper_74ec30(resource());
        let mut converted = [0u32; 2];
        xm1_power_record_helper_dca798(converted.as_mut_ptr(), xm1_power_record_helper_74eba8(resource()), &dimensions);
        descriptor.words[3] = converted[1].wrapping_mul(2);
        descriptor.words[4] = converted[0];
        xm1_power_record_helper_efaf24(17, &descriptor as *const SavedSelection as *const u8);
    }
}
