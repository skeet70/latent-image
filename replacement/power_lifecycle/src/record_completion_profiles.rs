#![cfg(target_arch = "arm")]

use core::mem::MaybeUninit;
use core::ptr::{copy_nonoverlapping, read_volatile, write_volatile};

extern "C" {
    fn xm1_power_record_helper_752450(output: u32, profile: u32);
    fn xm1_power_record_helper_7545c0(output: u32, profile: u32);
    fn xm1_power_record_helper_754ae4(output: u32, profile: u32);
    fn xm1_power_record_helper_752928(output: u32, profile: u32, index: u32);
    fn xm1_power_record_helper_758578(output: u32, profile: u32);
    fn xm1_power_record_helper_7582c0(output: u32, profile: u32);
    fn xm1_power_record_helper_75641c(profile: u32, destination: u32);
    fn xm1_power_record_helper_75587c(profile: u32) -> u32;
    fn xm1_power_record_helper_755914(profile: u32, index: u32) -> u32;
    fn xm1_power_record_helper_7559bc(profile: u32) -> u32;
    fn xm1_power_record_helper_755a5c(profile: u32, index: u32) -> u32;
    fn xm1_power_record_helper_755c5c(profile: u32) -> u32;
    fn xm1_power_record_helper_755cfc(profile: u32, index: u32) -> u32;
    fn xm1_power_record_helper_755b0c(profile: u32) -> u32;
    fn xm1_power_record_helper_755bac(profile: u32, index: u32) -> u32;
    fn xm1_power_record_helper_753a10(profile: u32, index: u32) -> u32;
    fn xm1_power_record_helper_74e648(profile: u32) -> u32;
    fn xm1_power_record_helper_74e718(profile: u32) -> u32;
    fn xm1_power_record_helper_7503ec(profile: u32) -> u32;
    fn xm1_power_record_helper_74f1ac(profile: u32) -> u32;
    fn xm1_power_record_helper_74f3cc(profile: u32) -> u32;
    fn xm1_power_record_helper_75ae78(destination: u32, source: u32);
    fn xm1_power_record_helper_75af54(destination: u32, source: u32);
    fn xm1_power_record_helper_75e6f0(bank: u32);
    fn xm1_power_record_helper_f2a200(a: u32, b: u32) -> u32;
}

unsafe fn word(address: u32) -> u32 { unsafe { read_volatile(address as *const u32) } }
unsafe fn put(address: u32, value: u32) { unsafe { write_volatile(address as *mut u32, value); } }
unsafe fn copy(destination: u32, source: u32, size: usize) {
    unsafe { copy_nonoverlapping(source as *const u8, destination as *mut u8, size); }
}
unsafe fn clear(channel: u32, status: u32) {
    if (status as i32) <= 0 || status == 20 || status == 21 {
        unsafe { write_volatile((0x0169_cf24 + channel) as *mut u8, 0); }
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_efa968(profile: u32) {
    let mut first = MaybeUninit::<[u32; 7]>::uninit();
    let mut second = MaybeUninit::<[u32; 7]>::uninit();
    unsafe {
        xm1_power_record_helper_752450(first.as_mut_ptr() as u32, profile);
        xm1_power_record_helper_7545c0(second.as_mut_ptr() as u32, profile);
        copy(word(0x0169_d0b4), first.as_ptr() as u32, 28);
        copy(word(0x0169_d0b8), second.as_ptr() as u32, 28);
    }
}

unsafe fn export_triplet(profile: u32, optional: bool) {
    let mut snapshot = MaybeUninit::<[u32; 3]>::uninit();
    unsafe {
        xm1_power_record_helper_754ae4(snapshot.as_mut_ptr() as u32, profile);
        let destination = word(0x0169_d0bc);
        if !optional || destination != 0 { copy(destination, snapshot.as_ptr() as u32, 12); }
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_efa9f0(profile: u32) {
    unsafe { export_triplet(profile, false); }
}
#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_efaacc(profile: u32) {
    unsafe { export_triplet(profile, true); }
}
#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_efaa34(profile: u32) {
    let mut snapshot = MaybeUninit::<[u32; 7]>::uninit();
    unsafe {
        xm1_power_record_helper_7545c0(snapshot.as_mut_ptr() as u32, profile);
        copy(word(0x0169_d0b8), snapshot.as_ptr() as u32, 28);
    }
}
#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_efaa80(profile: u32) {
    let mut snapshot = MaybeUninit::<[u32; 7]>::uninit();
    unsafe {
        xm1_power_record_helper_752450(snapshot.as_mut_ptr() as u32, profile);
        copy(word(0x0169_d0b4), snapshot.as_ptr() as u32, 28);
    }
}

#[no_mangle]
pub extern "C" fn xm1_power_record_helper_efa028(resource: u32, mode: u32, selection: u32, flags: u32) -> u32 {
    let excluded = matches!(resource, 5 | 11 | 12 | 13 | 18 | 21 | 25 | 34 | 37 | 40 | 48);
    let alternate = matches!(mode, 1 | 3 | 5) && !excluded && flags & 2 == 0;
    let value = match selection {
        0 => 6,
        3 => 4,
        _ if mode.wrapping_sub(2) <= 1 => 2,
        _ => 0,
    };
    if alternate && value != 6 { value + 1 } else { value }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_efab18(profile: u32, status: u32) {
    unsafe {
        let slot = 0x0169_d09c;
        clear(7, status);
        match status {
            0 => {
                xm1_power_record_helper_75e6f0(0);
                xm1_power_record_helper_75e6f0(1);
                xm1_power_record_helper_75e6f0(2);
            }
            7 => xm1_power_record_helper_75641c(profile, word(slot + 40) + 0x1470),
            9 | 10 | 11 => {
                if status == 9 {
                    let destination = word(slot + 40);
                    put(destination, xm1_power_record_helper_75587c(profile));
                    for index in 0..10 {
                        let source = xm1_power_record_helper_755914(profile, index);
                        copy(word(slot + 40) + 4 + index * 0x68, source, 0x68);
                    }
                    let destination = word(slot + 40);
                    put(destination + 0x828, xm1_power_record_helper_7559bc(profile));
                    for index in 0..10 {
                        let source = xm1_power_record_helper_755a5c(profile, index);
                        copy(word(slot + 40) + 0x82c + index * 0x68, source, 0x68);
                    }
                    let destination = word(slot + 40);
                    put(destination + 0x1050, xm1_power_record_helper_755c5c(profile));
                    for index in 0..10 {
                        let source = xm1_power_record_helper_755cfc(profile, index);
                        copy(word(slot + 40) + 0x1054 + index * 0x68, source, 0x68);
                    }
                }
                let mut snapshot = MaybeUninit::<[u32; 42]>::uninit();
                xm1_power_record_helper_752928(snapshot.as_mut_ptr() as u32, profile, 0);
                let state = word(slot + 56);
                let index = xm1_power_record_helper_f2a200(read_volatile(state as *const u8) as u32, read_volatile((state + 1) as *const u8) as u32);
                xm1_power_record_helper_75ae78(word(word(slot + 20) + index.wrapping_sub(1).wrapping_mul(4)), snapshot.as_ptr() as u32);
                let resource = xm1_power_record_helper_74e648(profile);
                let flags = xm1_power_record_helper_74e718(profile);
                let alternate = xm1_power_record_helper_7503ec(profile);
                let mode = xm1_power_record_helper_74f1ac(profile);
                let mut information = MaybeUninit::<[u32; 10]>::uninit();
                let information = information.as_mut_ptr() as u32;
                xm1_power_record_helper_7582c0(information, profile);
                let information_mode = word(information + 16);
                put(word(slot + 56) + 0x5c, information_mode);
                write_volatile((word(slot + 56) + 0x60) as *mut u16, read_volatile((information + 20) as *const u16));
                write_volatile((word(slot + 56) + 0x62) as *mut u16, read_volatile((information + 22) as *const u16));
                if alternate == 1 {
                    let selection = xm1_power_record_helper_753a10(profile, 0);
                    let destination = word(slot + 56);
                    put(destination + 0x50, xm1_power_record_helper_efa028(resource, mode, selection, flags));
                    let flags = if information_mode == 1 { flags | 1 } else { flags & 0xfffe };
                    put(word(slot + 56) + 0x48, resource);
                    write_volatile((word(slot + 56) + 0x4c) as *mut u16, flags as u16);
                } else {
                    put(word(slot + 56) + 0x50, mode);
                    put(word(slot + 56) + 0x48, resource);
                    write_volatile((word(slot + 56) + 0x4c) as *mut u16, flags as u16);
                }
                put(word(slot + 56) + 0x54, u32::from(word(snapshot.as_ptr() as u32 + 32) != 0));
                let destination = word(slot + 56);
                put(destination + 0x58, xm1_power_record_helper_74f3cc(profile));
                let destination = word(slot + 40);
                put(destination + 0xc3c, xm1_power_record_helper_755b0c(profile));
                if word(word(slot + 40) + 0xc3c) != 0 {
                    let source = xm1_power_record_helper_755bac(profile, 0);
                    copy(word(slot + 40) + 0xc40, source, 0x68);
                }
            }
            12 => {
                let mut snapshot = MaybeUninit::<[u32; 5]>::uninit();
                xm1_power_record_helper_758578(snapshot.as_mut_ptr() as u32, profile);
                let destination = word(word(slot + 48));
                if destination != 0 { xm1_power_record_helper_75af54(destination, snapshot.as_ptr() as u32); }
            }
            _ => {}
        }
        let callback = word(slot);
        if callback != 0 {
            let callback: unsafe extern "C" fn(u32) = core::mem::transmute(callback);
            callback(status);
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_efae88(profile: u32, status: u32) {
    unsafe {
        let slot = 0x0169_d0d8;
        clear(8, status);
        if status == 0 {
            let mut snapshot = MaybeUninit::<[u32; 5]>::uninit();
            xm1_power_record_helper_758578(snapshot.as_mut_ptr() as u32, profile);
            let destination = word(word(slot + 36));
            if destination != 0 { xm1_power_record_helper_75af54(destination, snapshot.as_ptr() as u32); }
        }
        let callback = word(slot);
        if callback != 0 {
            let callback: unsafe extern "C" fn(u32) = core::mem::transmute(callback);
            callback(0);
        }
    }
}
