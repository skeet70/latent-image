//! X-M1 1.50 auxiliary selection tail. Source: x70view 55436fc85c0560dd252b0410af6f2427fe198cd3.

use core::ptr::{read_volatile, write_volatile};

#[repr(transparent)]
#[derive(Clone, Copy)]
struct ProfileHandle(u32);
#[repr(C)]
struct BaseRecord { words: [u32; 33] }
#[repr(C)]
struct SmallRecord { words: [u32; 8] }
#[repr(C)]
struct GeometryRecord { words: [u32; 5] }
#[repr(C)]
struct WideRecord { words: [u32; 20] }

unsafe extern "C" {
    fn xm1_power_record_helper_756734(handle: ProfileHandle, out: *mut BaseRecord);
    fn xm1_power_record_helper_756754(handle: ProfileHandle, record: *const BaseRecord);
    fn xm1_power_record_helper_756788(handle: ProfileHandle, out: *mut SmallRecord);
    fn xm1_power_record_helper_7567b8(handle: ProfileHandle, record: *const SmallRecord);
    fn xm1_power_record_helper_756e24(out: *mut GeometryRecord, handle: ProfileHandle);
    fn xm1_power_record_helper_756e5c(handle: ProfileHandle, record: *const GeometryRecord);
    fn xm1_power_record_helper_756f3c(out: *mut WideRecord, handle: ProfileHandle);
    fn xm1_power_record_helper_756f60(handle: ProfileHandle, record: *const WideRecord);
    fn xm1_power_record_helper_7569a4(handle: ProfileHandle) -> u32;
    fn xm1_power_record_helper_7554ac(handle: ProfileHandle) -> u32;
    fn xm1_power_record_helper_efaf24(change: i32, zero: u32);
    fn xm1_power_record_helper_efafbc(change: i32);
    fn xm1_power_record_helper_efb4b8(change: i32);
    fn xm1_power_record_helper_efb590(change: i32, flags: u32);
    fn xm1_power_record_helper_efb73c(change: i32);
    fn xm1_power_record_helper_efb784(change: i32);
    fn xm1_power_record_helper_efb814(change: i32);
    fn xm1_power_record_helper_efb85c(change: i32);
    fn xm1_power_record_helper_efb8a4(change: i32);
    fn xm1_power_record_helper_efb8ec(change: i32);
    fn xm1_power_record_helper_efba48(change: i32);
    fn xm1_power_record_helper_efbad8(change: i32);
    fn xm1_power_record_helper_faaddc(handle: ProfileHandle) -> u32;
    fn xm1_power_record_helper_756ae4(handle: ProfileHandle, value: u32);
    fn xm1_power_record_helper_ef9b50() -> u32;
    fn xm1_power_record_helper_ef9b6c() -> u32;
    fn xm1_power_record_helper_ef9c54() -> u32;
    fn xm1_power_record_helper_ef9ef4() -> u32;
    fn xm1_power_record_helper_75569c(handle: ProfileHandle) -> u32;
    fn xm1_power_record_helper_74eb20(handle: ProfileHandle) -> u32;
    fn xm1_power_record_helper_74e020(handle: ProfileHandle, mask: u64) -> u32;
    fn xm1_power_record_helper_7556ec(handle: ProfileHandle, value: u32);
    fn xm1_power_record_helper_faaea8(handle: ProfileHandle) -> u32;
    fn xm1_power_record_helper_fea438(callback: unsafe extern "C" fn(u32), value: u32, enabled: u32);
    fn xm1_power_record_helper_fea468(callback: unsafe extern "C" fn(u32), first: u32, second: u32, flags: u32);
    fn xm1_power_record_helper_756d4c(handle: ProfileHandle) -> u32;
    fn xm1_power_record_helper_fa7fd8() -> u32;
    fn xm1_power_record_helper_fea7cc(out: *mut u32);
    fn xm1_power_record_helper_756db4(handle: ProfileHandle, value: u32);
    fn xm1_power_record_helper_fea4d0(callback: unsafe extern "C" fn(u32), value: u32);
    fn xm1_power_record_helper_7503ec(handle: ProfileHandle) -> u32;
    fn xm1_power_record_helper_7588e8(out: *mut u32, handle: ProfileHandle);
    fn xm1_power_record_helper_75890c(handle: ProfileHandle, record: *const u32);
    fn xm1_power_record_helper_fea4fc(callback: unsafe extern "C" fn(u32));
    fn xm1_power_record_helper_74046c(group: u32);
    fn xm1_power_record_helper_740404(group: u32);
    fn xm1_power_record_helper_74fa2c(handle: ProfileHandle) -> u32;
    fn xm1_power_record_helper_754d9c(handle: ProfileHandle) -> u32;
    fn xm1_power_record_helper_ef9e68() -> u32;
    fn xm1_power_record_helper_7509c4(handle: ProfileHandle) -> u32;
    fn xm1_power_record_helper_f2f7c8(value: u32);
    fn xm1_power_record_helper_f2f7a0(value: u32);
    fn xm1_power_record_helper_f2f750(value: u32);
    fn xm1_power_record_helper_f2f728(value: u32);
}

unsafe fn word(offset: usize) -> u32 { unsafe { read_volatile((0x016b_e004 + offset) as *const u32) } }
unsafe fn put(offset: usize, value: u32) { unsafe { write_volatile((0x016b_e004 + offset) as *mut u32, value) } }
unsafe fn handle() -> ProfileHandle { ProfileHandle(unsafe { word(0x18) }) }
unsafe fn clear_base() {
    unsafe {
        let mut record = BaseRecord { words: [0; 33] };
        xm1_power_record_helper_756734(handle(), &mut record);
        record.words[1] = 0;
        record.words[24] = 0;
        xm1_power_record_helper_756754(handle(), &record);
    }
}
unsafe fn clear_small() {
    unsafe {
        let mut record = SmallRecord { words: [0; 8] };
        xm1_power_record_helper_756788(handle(), &mut record);
        record.words[0] = 0;
        xm1_power_record_helper_7567b8(handle(), &record);
    }
}
unsafe fn clear_geometry() {
    unsafe {
        let mut record = GeometryRecord { words: [0; 5] };
        xm1_power_record_helper_756e24(&mut record, handle());
        record.words[3] = 0;
        xm1_power_record_helper_756e5c(handle(), &record);
    }
}
unsafe fn clear_wide(second: bool) {
    unsafe {
        let mut record = WideRecord { words: [0; 20] };
        xm1_power_record_helper_756f3c(&mut record, handle());
        record.words[0] = 0;
        if second { record.words[1] = 0; }
        xm1_power_record_helper_756f60(handle(), &record);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f2f7f0(event: u32) {
    unsafe {
        let mut change = match event { 1 => -94, 2 => 21, _ => 0 };
        match word(0x14) {
            3 => { clear_base(); clear_small(); clear_geometry(); clear_wide(true); }
            9 => xm1_power_record_helper_efaf24(15, 0),
            10 => xm1_power_record_helper_efaf24(change, 0),
            11 => xm1_power_record_helper_efafbc(change),
            13 => clear_wide(false),
            23 => {
                if change == 21 { change = -96; }
                else if xm1_power_record_helper_7569a4(ProfileHandle(read_volatile(0x016b_e054 as *const u32))) == 0 {
                    clear_base(); clear_small(); clear_geometry(); clear_wide(false); clear_wide(true);
                }
                xm1_power_record_helper_efb4b8(change);
            }
            26 => xm1_power_record_helper_efb590(change, xm1_power_record_helper_7554ac(handle())),
            29 => xm1_power_record_helper_efb73c(change),
            30 => xm1_power_record_helper_efb784(change),
            32 => xm1_power_record_helper_efb814(change),
            33 => xm1_power_record_helper_efb85c(change),
            34 => xm1_power_record_helper_efb8a4(change),
            35 => xm1_power_record_helper_efb8ec(change),
            38 => xm1_power_record_helper_efba48(change),
            41 => xm1_power_record_helper_efbad8(change),
            _ => {}
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f2fb7c(bits: u32) { unsafe {
    let p = 0x016b_e020 as *mut u8;
    write_volatile(p, read_volatile(p) | bits as u8);
} }
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f2fba4() { unsafe { write_volatile(0x016b_e020 as *mut u8, 0) } }
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f2fbc8() { unsafe { xm1_power_record_helper_756ae4(handle(), xm1_power_record_helper_faaddc(handle())) } }
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f2fbf0(kind: u32) { unsafe {
    let selector = match kind { 9 | 42 | 170 => 1, 2 => u32::from(read_volatile((xm1_power_record_helper_ef9b50() + 16) as *const u32) == 0), _ => 0 };
    put(0, selector);
} }
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f2fc50() { unsafe { put(0, 0) } }
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f2fc74() -> u32 { unsafe { u32::from(read_volatile((xm1_power_record_helper_ef9c54() + 4) as *const u32) != 0) } }
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f2fc90(kind: u32) { unsafe {
    let flags = if kind == 3 { read_volatile((xm1_power_record_helper_ef9b6c() + 4) as *const u32) & 0x8000_0003 } else { 0 };
    put(12, flags);
} }
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f2fcd0() -> u32 { unsafe { u32::from(read_volatile((xm1_power_record_helper_ef9ef4() + 4) as *const u32) == 1) } }
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f2fcf0() -> u32 { unsafe {
    let first = xm1_power_record_helper_75569c(handle());
    let second = xm1_power_record_helper_74eb20(handle());
    if first == 1 || second == 1 { 4 } else { 1 }
} }
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f2fd34() -> u32 { unsafe { xm1_power_record_helper_74e020(handle(), 0x10_0000_0000) } }
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f2fd58(value: u32) { unsafe { xm1_power_record_helper_7556ec(handle(), value) } }
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f2fd78() { unsafe {
    let enabled = u32::from(word(0) != 0);
    let value = xm1_power_record_helper_faaea8(handle());
    xm1_power_record_helper_fea438(xm1_power_record_helper_f2f7c8, value, enabled);
} }
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f2fdb4() { unsafe { xm1_power_record_helper_fea468(xm1_power_record_helper_f2f7a0, 0, 0, word(12)) } }
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f2fde0() { unsafe {
    let value = xm1_power_record_helper_faaea8(handle());
    xm1_power_record_helper_fea438(xm1_power_record_helper_f2f7c8, value, 1);
} }
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f2fe10() { unsafe {
    let value = xm1_power_record_helper_756d4c(handle());
    xm1_power_record_helper_fea468(xm1_power_record_helper_f2f7a0, value, word(0x30), 3);
} }
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f2fe44() { unsafe {
    put(0x30, xm1_power_record_helper_fa7fd8());
    let kind = word(0x24);
    let flags = word(12);
    let mode = if kind == 1 && flags != 0 { if flags as i32 >= 0 { 1 } else { 6 } } else { match kind { 2..=5 => kind, _ => 0 } };
    let mut snapshot = [0u32; 2];
    xm1_power_record_helper_fea7cc(snapshot.as_mut_ptr());
    xm1_power_record_helper_756db4(handle(), u32::from(snapshot[0] == 1));
    xm1_power_record_helper_fea4d0(xm1_power_record_helper_f2f750, mode);
    put(0x24, 0);
} }
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f2ff10() { unsafe {
    if xm1_power_record_helper_7503ec(handle()) == 1 {
        let mut record = [0u32; 21];
        xm1_power_record_helper_7588e8(record.as_mut_ptr(), handle());
        record[17] = 4;
        xm1_power_record_helper_75890c(handle(), record.as_ptr());
    }
    xm1_power_record_helper_fea4fc(xm1_power_record_helper_f2f728);
} }
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f2ff80() { unsafe { xm1_power_record_helper_74046c(4) } }
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f2ff94() { unsafe { xm1_power_record_helper_740404(4) } }
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f2ffa8() -> u32 { unsafe {
    let first = xm1_power_record_helper_74fa2c(handle());
    let second = xm1_power_record_helper_754d9c(handle());
    let parameter = xm1_power_record_helper_ef9e68();
    if first != 0 { 1 } else if (second == 3) == (read_volatile((parameter + 4) as *const u32) == 3) { 1 } else { 4 }
} }
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f30014() -> u32 { unsafe {
    let mode = xm1_power_record_helper_754d9c(handle());
    let enabled = xm1_power_record_helper_7509c4(handle());
    u32::from(!(mode == 3 && enabled == 1))
} }
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f30050() -> u32 { unsafe {
    let matches = xm1_power_record_helper_74e020(handle(), 0x0200_0000);
    let unlocked = xm1_power_record_helper_f30014();
    u32::from(unlocked == 0 && matches != 0)
} }
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f30094() -> u32 { unsafe { xm1_power_record_helper_f30014() } }
