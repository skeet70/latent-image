#![cfg(target_arch = "arm")]

use crate::callback_registry::registry as callbacks;
use crate::callback_slots;
use core::ptr::{read_volatile, write_volatile};

extern "C" {
    fn xm1_power_record_helper_74a214(profile: u32) -> u32;
    fn xm1_power_record_helper_74a740(kind: u32, index: u32) -> u32;
    fn xm1_power_record_helper_76d9a8() -> u32;
    fn xm1_power_record_helper_76da38() -> u32;
    fn xm1_power_record_helper_76dba4() -> u32;
}

unsafe fn word(address: u32) -> u32 { unsafe { read_volatile(address as *const u32) } }
unsafe fn clear(channel: u32, status: u32) {
    if (status as i32) <= 0 || status == 20 || status == 21 {
        unsafe { write_volatile((0x0169_cf24 + channel) as *mut u8, 0); }
    }
}

macro_rules! scalar_callback {
    ($name:ident, $channel:expr, $slot:expr) => {
        #[no_mangle]
        pub unsafe extern "C" fn $name(status: u32) {
            let callback = unsafe { word($slot) };
            unsafe { clear($channel, status); }
            let _ = callback_slots::call_argument(callbacks(), callback, status);
        }
    };
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_efaf24(status: u32, value: *const u8) {
    let callback = unsafe { word(0x0169_d100) };
    unsafe { clear(9, status); }
    if callback != 0 {
        let callback: unsafe extern "C" fn(u32, *const u8) = unsafe { core::mem::transmute(callback) };
        unsafe { callback(status, value); }
    }
}
scalar_callback!(xm1_power_record_helper_efaf74, 10, 0x0169_d110);
scalar_callback!(xm1_power_record_helper_efafbc, 11, 0x0169_d118);
scalar_callback!(xm1_power_record_helper_efb004, 12, 0x0169_d11c);
scalar_callback!(xm1_power_record_helper_efb04c, 40, 0x0169_d318);
scalar_callback!(xm1_power_record_helper_efb194, 15, 0x0169_d1dc);
scalar_callback!(xm1_power_record_helper_efb1dc, 16, 0x0169_d1e4);
scalar_callback!(xm1_power_record_helper_efb298, 18, 0x0169_d200);
scalar_callback!(xm1_power_record_helper_efb2e0, 19, 0x0169_d208);
scalar_callback!(xm1_power_record_helper_efb3a8, 21, 0x0169_d220);
scalar_callback!(xm1_power_record_helper_efb3f0, 22, 0x0169_d228);
scalar_callback!(xm1_power_record_helper_efb4b8, 23, 0x0169_d22c);
scalar_callback!(xm1_power_record_helper_efb500, 24, 0x0169_d254);
scalar_callback!(xm1_power_record_helper_efb548, 25, 0x0169_d274);
scalar_callback!(xm1_power_record_helper_efb6ac, 45, 0x0169_d368);
scalar_callback!(xm1_power_record_helper_efb6f4, 28, 0x0169_d294);
scalar_callback!(xm1_power_record_helper_efb73c, 29, 0x0169_d29c);
scalar_callback!(xm1_power_record_helper_efb784, 30, 0x0169_d2a8);
scalar_callback!(xm1_power_record_helper_efb7cc, 31, 0x0169_d2b0);

unsafe fn context_profile(profile: u32, saved: u32) -> u32 {
    if unsafe { xm1_power_record_helper_74a214(profile) } == 0 {
        unsafe { xm1_power_record_helper_74a740(0x80, 0) }
    } else { saved }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_efb094(status: u32, profile: u32, flags: u32, mask: u32) {
    let callback = unsafe { word(0x0169_d128) };
    let saved = unsafe { word(0x0169_d1c4) };
    unsafe { clear(13, status); }
    if callback != 0 {
        let callback: unsafe extern "C" fn(u32, u32, u32, u32, u32) = unsafe { core::mem::transmute(callback) };
        unsafe { callback(status, profile, saved, flags, mask); }
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_efb104(status: u32, profile: u32, flags: u32, mask: u32) {
    let callback = unsafe { word(0x0169_d1c8) };
    let saved = unsafe { word(0x0169_d1d0) };
    unsafe { clear(14, status); }
    if callback != 0 {
        let context = unsafe { context_profile(profile, saved) };
        let callback: unsafe extern "C" fn(u32, u32, u32, u32, u32) = unsafe { core::mem::transmute(callback) };
        unsafe { callback(status, profile, context, flags, mask); }
    }
}

macro_rules! profile_callback {
    ($name:ident, $channel:expr, $slot:expr, $context:expr) => {
        #[no_mangle]
        pub unsafe extern "C" fn $name(status: u32, profile: u32, value: u32) {
            let callback = unsafe { word($slot) };
            let saved = unsafe { word($context) };
            unsafe { clear($channel, status); }
            if callback != 0 {
                let context = unsafe { context_profile(profile, saved) };
                let callback: unsafe extern "C" fn(u32, u32, u32, u32) = unsafe { core::mem::transmute(callback) };
                unsafe { callback(status, profile, context, value); }
            }
        }
    };
}
profile_callback!(xm1_power_record_helper_efb328, 20, 0x0169_d20c, 0x0169_d21c);
profile_callback!(xm1_power_record_helper_efb438, 43, 0x0169_d350, 0x0169_d35c);

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_efb224(status: u32, value: u32) {
    let callback = unsafe { word(0x0169_d1ec) };
    let mut flags = if unsafe { xm1_power_record_helper_76d9a8() } == 2 { 2 } else { 0 };
    if unsafe { xm1_power_record_helper_76da38() } == 2 { flags |= 1; }
    unsafe { xm1_power_record_helper_76dba4(); clear(17, status); }
    if callback != 0 {
        let callback: unsafe extern "C" fn(u32, u32, u32) = unsafe { core::mem::transmute(callback) };
        unsafe { callback(status, value, flags); }
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_efb590(status: u32, flags: u32) {
    let callback = if status == 19 {
        if flags & 1 != 0 { unsafe { word(0x0169_d278) } } else { 0 }
    } else { unsafe { word(0x0169_d278) } };
    unsafe { clear(26, status); }
    let _ = callback_slots::call_pair(callbacks(), callback, status, flags);
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_efb604(status: u32, value: u32) {
    let callback = unsafe { word(0x0169_d28c) };
    unsafe { clear(27, status); }
    let _ = callback_slots::call_pair(callbacks(), callback, status, value);
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_efb654(status: u32, a: u32, b: u32) {
    let callback = unsafe { word(0x0169_d360) };
    unsafe { clear(44, status); }
    if callback != 0 {
        let callback: unsafe extern "C" fn(u32, u32, u32) = unsafe { core::mem::transmute(callback) };
        unsafe { callback(status, a, b); }
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_callback2(callback: u32, a: u32, b: u32) {
    let _ = callback_slots::call_pair(callbacks(), callback, a, b);
}

scalar_callback!(xm1_power_record_helper_efba90, 39, 0x0169_d310);

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_efb984(status: u32, triplet: u32, extra: u32) {
    let callback = unsafe { word(0x0169_d2fc) };
    unsafe { clear(37, status); }
    if callback != 0 {
        let packet = unsafe { [extra, word(triplet), word(triplet + 4), word(triplet + 8)] };
        let callback: unsafe extern "C" fn(u32, *const u32) = unsafe { core::mem::transmute(callback) };
        unsafe { callback(status, packet.as_ptr()); }
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_efb9f0(triplet: u32, extra: u32) {
    if unsafe { word(0x0095_4ddc) } != 0 {
        let callback = unsafe { word(0x0095_4dd8) };
        if callback != 0 {
            let packet = unsafe { [extra, word(triplet), word(triplet + 4), word(triplet + 8)] };
            let callback: unsafe extern "C" fn(*const u32) = unsafe { core::mem::transmute(callback) };
            unsafe { callback(packet.as_ptr()); }
        }
    }
}

scalar_callback!(xm1_power_record_helper_efb814, 32, 0x0169_d2c4);
scalar_callback!(xm1_power_record_helper_efb85c, 33, 0x0169_d2cc);
scalar_callback!(xm1_power_record_helper_efb8a4, 34, 0x0169_d2d4);
scalar_callback!(xm1_power_record_helper_efb8ec, 35, 0x0169_d2dc);

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_efb934(status: u32, value: u32) {
    let callback = unsafe { word(0x0169_d2e8) };
    unsafe { clear(36, status); }
    let _ = callback_slots::call_pair(callbacks(), callback, status, value);
}
