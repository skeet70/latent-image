//! X-M1 1.50: x70view 55436fc85c0560dd252b0410af6f2427fe198cd3; id5 f3b058..f3be7c.

use core::ptr::{read_volatile, write_unaligned};
use crate::record_profile_accessors::ProfileWords;

unsafe extern "C" {
    fn xm1_power_record_helper_756e24(output: *mut ProfileWords<5>) -> *mut ProfileWords<5>;
    fn xm1_power_record_helper_756e90(output: *mut u32, resource: u32) -> *mut u32;
    fn xm1_power_record_helper_74fdfc(resource: u32) -> u32;
    fn xm1_power_record_helper_74fe84(resource: u32) -> u32;
    fn xm1_power_record_helper_74f1f0(resource: u32) -> u32;
    fn xm1_power_record_helper_757bc0(resource: u32) -> u32;
    fn xm1_power_record_helper_ef43b8(input: *const u32, first: u32, second: u32, output: *mut u32, kind: u32, flags: u32);
    fn xm1_power_record_helper_103a470(record: *const u32, value: u32, tail: u32);
    fn xm1_power_record_helper_fa7634(resource: u32) -> u32;
    fn xm1_power_record_helper_fa7a5c(resource: u32) -> u32;
    fn xm1_power_record_helper_faaea8(resource: u32) -> u32;
    fn xm1_power_record_helper_74e5c0(resource: u32) -> u32;
    fn xm1_power_record_helper_74e7a0(resource: u32) -> u32;
    fn xm1_power_record_helper_74ea10(resource: u32) -> u32;
    fn xm1_power_record_helper_74eba8(resource: u32) -> u32;
    fn xm1_power_record_helper_74ec30(resource: u32) -> u32;
    fn xm1_power_record_helper_74ece4(resource: u32) -> u32;
    fn xm1_power_record_helper_74ed6c(resource: u32) -> u32;
    fn xm1_power_record_helper_74ee7c(resource: u32) -> u32;
    fn xm1_power_record_helper_74ef04(resource: u32) -> u32;
    fn xm1_power_record_helper_74ef8c(resource: u32) -> u32;
    fn xm1_power_record_helper_74f014(resource: u32) -> u32;
    fn xm1_power_record_helper_74f124(resource: u32) -> u32;
    fn xm1_power_record_helper_750144(resource: u32) -> u32;
    fn xm1_power_record_helper_7501cc(resource: u32) -> u32;
    fn xm1_power_record_helper_7503ec(resource: u32) -> u32;
    fn xm1_power_record_helper_750474(resource: u32) -> u32;
    fn xm1_power_record_helper_750d98(resource: u32) -> u32;
    fn xm1_power_record_helper_752714(resource: u32) -> u32;
    fn xm1_power_record_helper_7527f8(resource: u32) -> u32;
    fn xm1_power_record_helper_752890(resource: u32) -> u32;
    fn xm1_power_record_helper_754d9c(resource: u32) -> u32;
    fn xm1_power_record_helper_754e3c(resource: u32) -> u32;
    fn xm1_power_record_helper_754edc(output: *mut u32, resource: u32);
    fn xm1_power_record_helper_754f8c(resource: u32) -> u32;
    fn xm1_power_record_helper_75502c(resource: u32) -> u32;
    fn xm1_power_record_helper_7550cc(resource: u32) -> u32;
    fn xm1_power_record_helper_75516c(resource: u32) -> u32;
    fn xm1_power_record_helper_75520c(output: *mut u32, resource: u32);
    fn xm1_power_record_helper_7552bc(output: *mut u32, resource: u32);
    fn xm1_power_record_helper_75536c(resource: u32) -> u32;
    fn xm1_power_record_helper_75569c(resource: u32) -> u32;
    fn xm1_power_record_helper_757c10(resource: u32) -> u32;
    fn xm1_power_record_helper_757df0(resource: u32) -> u32;
    fn xm1_power_record_helper_757da0(resource: u32) -> u32;
    fn xm1_power_record_helper_772f28() -> u32;
    fn xm1_power_record_helper_ee9ab0() -> u32;
    fn xm1_power_record_helper_759140(resource: u32) -> u32;
    fn xm1_power_record_helper_7590b8(resource: u32) -> u32;
    fn xm1_power_record_helper_f5206c(output: *mut u32, resource: u32);
    fn xm1_power_record_helper_dca798(output: *mut u32, mode: u32, dimensions: *const u32);
    fn xm1_power_record_helper_f3a794(status: u32);
    fn xm1_power_record_helper_103aff0(value: u32, byte: u32, mode: u32);
}

unsafe fn resource() -> u32 { unsafe { read_volatile(0x016c0718 as *const u32) } }
unsafe fn word(output: *mut u8, offset: usize, value: u32) { unsafe { write_unaligned(output.add(offset).cast::<u32>(), value); } }
unsafe fn half(output: *mut u8, offset: usize, value: u32) { unsafe { write_unaligned(output.add(offset).cast::<u16>(), value as u16); } }
unsafe fn byte(output: *mut u8, offset: usize, value: u32) { unsafe { output.add(offset).write(value as u8); } }

unsafe fn selection_snapshot() -> [u32; 5] {
    unsafe {
        let mut snapshot = [0u32; 5];
        xm1_power_record_helper_756e24(snapshot.as_mut_ptr().cast());
        let input = [snapshot[0], snapshot[1] & 0xffff];
        let first = xm1_power_record_helper_74fdfc(resource());
        let second = xm1_power_record_helper_74fe84(resource());
        let kind = xm1_power_record_helper_74f1f0(resource());
        xm1_power_record_helper_ef43b8(input.as_ptr(), first, second, snapshot.as_mut_ptr(), kind, 0);
        snapshot
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f3b058() {
    unsafe {
        let snapshot = selection_snapshot();
        let value = xm1_power_record_helper_757bc0(resource());
        xm1_power_record_helper_103a470(snapshot.as_ptr(), value, snapshot[4]);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f3b110() {
    unsafe {
        let mut record = [0u32; 4];
        xm1_power_record_helper_756e90(record.as_mut_ptr(), resource());
        let mode = xm1_power_record_helper_fa7634(resource());
        xm1_power_record_helper_103aff0(record[1], record[3] & 0xff, mode);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f3b568(output: *mut u8) {
    unsafe {
        word(output, 0, xm1_power_record_helper_752714(resource()));
        word(output, 4, xm1_power_record_helper_7527f8(resource()));
        word(output, 8, xm1_power_record_helper_752890(resource()));
        word(output, 12, 0x11de784a);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f3b5f0(output: *mut u8) {
    unsafe {
        word(output, 8, xm1_power_record_helper_74ec30(resource()));
        word(output, 12, xm1_power_record_helper_75516c(resource()));
        if xm1_power_record_helper_74e5c0(resource()) == 0 {
            word(output, 16, u32::from(xm1_power_record_helper_faaea8(resource()) == 2));
            word(output, 4, 640);
            word(output, 0, 240);
        } else {
            let mode = xm1_power_record_helper_74eba8(resource());
            word(output, 20, mode);
            word(output, 16, if mode.wrapping_sub(6) <= 1 { 9 } else { 8 });
            xm1_power_record_helper_dca798(output.cast(), mode, output.add(8).cast());
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f3b6c8(output: *mut u8) {
    unsafe {
        let mut record = [0u32; 4];
        xm1_power_record_helper_754edc(record.as_mut_ptr(), resource());
        core::ptr::copy_nonoverlapping(record.as_ptr().cast::<u8>(), output, 16);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f3b72c(output: *mut u8) {
    unsafe {
        word(output, 0, xm1_power_record_helper_754f8c(resource()));
        word(output, 4, xm1_power_record_helper_754d9c(resource()));
        word(output, 8, xm1_power_record_helper_754e3c(resource()));
        word(output, 12, xm1_power_record_helper_7550cc(resource()));
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f3b7a0(output: *mut u8) {
    unsafe {
        let snapshot = selection_snapshot();
        word(output, 36, snapshot[0]);
        half(output, 40, snapshot[1]);
        word(output, 32, snapshot[4]);
        word(output, 0, xm1_power_record_helper_74ece4(resource()));
        word(output, 4, xm1_power_record_helper_74ed6c(resource()));
        word(output, 8, xm1_power_record_helper_74ee7c(resource()));
        word(output, 12, xm1_power_record_helper_fa7a5c(resource()));
        let mode = xm1_power_record_helper_74e5c0(resource());
        word(output, 16, if mode == 0 { 1 } else { xm1_power_record_helper_74f124(resource()) });
        word(output, 20, xm1_power_record_helper_74ef04(resource()));
        word(output, 24, xm1_power_record_helper_74ef8c(resource()));
        word(output, 28, xm1_power_record_helper_74f014(resource()));
        half(output, 44, xm1_power_record_helper_757bc0(resource()));
        let first = xm1_power_record_helper_757c10(resource());
        xm1_power_record_helper_757df0(resource());
        half(output, 42, first.wrapping_add(xm1_power_record_helper_772f28()));
        let mut record = [0u32; 4];
        xm1_power_record_helper_756e90(record.as_mut_ptr(), resource());
        byte(output, 52, record[3]);
        word(output, 48, record[1]);
        byte(output, 53, xm1_power_record_helper_74f1f0(resource()));
        xm1_power_record_helper_757da0(resource());
        half(output, 46, xm1_power_record_helper_ee9ab0());
        byte(output, 54, xm1_power_record_helper_759140(resource()));
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f3b9d8(output: *mut u8) {
    unsafe {
        word(output, 0, xm1_power_record_helper_fa7634(resource()));
        word(output, 4, xm1_power_record_helper_74e5c0(resource()));
        byte(output, 29, xm1_power_record_helper_74ea10(resource()));
        word(output, 8, xm1_power_record_helper_75502c(resource()));
        word(output, 12, xm1_power_record_helper_750144(resource()));
        word(output, 16, xm1_power_record_helper_7501cc(resource()));
        byte(output, 30, xm1_power_record_helper_7503ec(resource()));
        byte(output, 31, xm1_power_record_helper_750474(resource()));
        byte(output, 32, xm1_power_record_helper_750d98(resource()));
        byte(output, 33, xm1_power_record_helper_7590b8(resource()));
        let dimensions = xm1_power_record_helper_74ec30(resource());
        let mut converted = [0u32; 2];
        xm1_power_record_helper_dca798(converted.as_mut_ptr(), 1, &dimensions);
        let mut ratio = [0u32; 2];
        xm1_power_record_helper_f5206c(ratio.as_mut_ptr(), resource());
        word(output, 40, (converted[0].wrapping_mul(116) / 1000).wrapping_mul(ratio[1].wrapping_sub(3)).wrapping_add(converted[0] >> 1));
        word(output, 36, (converted[1].wrapping_mul(116) / 1000).wrapping_mul(ratio[0].wrapping_sub(3)).wrapping_add(converted[1] >> 1));
        word(output, 44, converted[0]);
        word(output, 48, converted[1]);
        half(output, 52, xm1_power_record_helper_74e7a0(resource()));
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f3bb70(output: *mut u8) {
    unsafe {
        word(output, 0, xm1_power_record_helper_75569c(resource()));
        word(output, 4, 0x016c0728);
        word(output, 8, xm1_power_record_helper_f3a794 as *const () as usize as u32);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f3be7c(output: *mut u8) {
    unsafe {
        let mut record = [0u32; 4];
        xm1_power_record_helper_75520c(record.as_mut_ptr(), resource());
        core::ptr::copy_nonoverlapping(record.as_ptr().cast::<u8>(), output, 16);
        xm1_power_record_helper_7552bc(record.as_mut_ptr(), resource());
        core::ptr::copy_nonoverlapping(record.as_ptr().cast::<u8>(), output.add(16), 8);
        word(output, 24, xm1_power_record_helper_75536c(resource()));
    }
}
