use core::ptr::{read_volatile, write_volatile};
unsafe fn word(address: usize) -> u32 { unsafe { read_volatile(address as *const u32) } }
unsafe fn half(address: usize) -> u16 { unsafe { read_volatile(address as *const u16) } }
unsafe fn signed_half(address: usize) -> i32 { unsafe { half(address) as i16 as i32 } }
unsafe fn byte(address: usize) -> u8 { unsafe { read_volatile(address as *const u8) } }
unsafe fn put(address: usize, value: u32) { unsafe { write_volatile(address as *mut u32, value) } }
unsafe fn put_half(address: usize, value: u16) { unsafe { write_volatile(address as *mut u16, value) } }
unsafe fn put_byte(address: usize, value: u8) { unsafe { write_volatile(address as *mut u8, value) } }
unsafe extern "C" {
    fn xm1_power_record_helper_fa7634(request: u32) -> u32;
    fn xm1_power_record_helper_74e718(request: u32) -> u32;
    fn xm1_power_record_helper_74f1ac(request: u32) -> u32;
    fn xm1_power_record_helper_74f9a4(request: u32) -> u32;
    fn xm1_power_record_helper_74e7f0(request: u32) -> u32;
    fn xm1_power_record_helper_74e878(request: u32) -> u32;
    fn xm1_power_record_helper_74e900(request: u32) -> u32;
    fn xm1_power_record_helper_74f674(request: u32) -> u32;
    fn xm1_power_record_helper_74e5c0(request: u32) -> u32;
    fn xm1_power_record_helper_74f5ec(request: u32) -> u32;
    fn xm1_power_record_helper_f5f9fc(request: u32) -> u32;
    fn xm1_power_record_helper_74f6fc(request: u32) -> u32;
    fn xm1_power_record_helper_74f784(request: u32) -> u32;
    fn xm1_power_record_helper_74f80c(request: u32) -> u32;
    fn xm1_power_record_helper_74f91c(request: u32) -> u32;
    fn xm1_power_record_helper_fa6bb4(request: u32) -> u32;
    fn xm1_power_record_helper_772ff8(value: u32) -> u32;
    fn xm1_power_record_helper_74eb20(request: u32) -> u32;
    fn xm1_power_record_helper_7500bc(request: u32) -> u32;
    fn xm1_power_record_helper_7503ec(request: u32) -> u32;
    fn xm1_power_record_helper_74f1f0(request: u32) -> u32;
    fn xm1_power_record_helper_7509c4(request: u32) -> u32;
    fn xm1_power_record_helper_f5fc00(request: u32) -> u32;
    fn xm1_power_record_helper_74ed6c(request: u32) -> u32;
    fn xm1_power_record_helper_74f124(request: u32) -> u32;
    fn xm1_power_record_helper_75060c(request: u32) -> u32;
    fn xm1_power_record_helper_74eba8(request: u32) -> u32;
    fn xm1_power_record_helper_74f564(request: u32) -> u32;
    fn xm1_power_record_helper_faaae8(request: u32) -> u32;
    fn xm1_power_record_helper_ef74e4(value: u32) -> u32;
    fn xm1_power_record_helper_758c98(output: *mut u32, request: u32);
    fn xm1_power_record_helper_758cc4(request: u32, input: *const u32);
    fn xm1_power_record_helper_69f8fc(key: u32) -> u32;
    fn xm1_power_record_helper_750474(request: u32) -> u32;
    fn xm1_power_record_helper_fea7ac(index: u32, output: u32);
    fn xm1_power_record_helper_772f28(value: u32) -> u32;
    fn xm1_power_record_helper_757c60(request: u32) -> u32;
    fn xm1_power_record_helper_757e38(request: u32) -> u32;
    fn xm1_power_record_helper_f33164(kind: u32, first: i32, second: i32, third: i32, fourth: i32, fifth: i32, sixth: i32, adjustment: u32, mode: u32, limit: u32, flag: u32, alternate: u32, base: u32);
    fn xm1_power_record_helper_76d9c8() -> u32;
    fn xm1_power_record_helper_757e58(request: u32, base: u32);
    fn xm1_power_record_helper_757b78(request: u32) -> u32;
    fn xm1_power_record_helper_757df0(request: u32) -> u32;
    fn xm1_power_record_helper_756ac4(request: u32) -> u32;
    fn xm1_power_record_helper_fab250(mode: u32, kind: u32, selection: u32, setting: u32) -> u32;
    fn xm1_power_record_helper_fa7aa4(mode: u32, selection: u32, cache: u32, index: u32, output: u32);
    fn xm1_power_record_helper_75817c(request: u32) -> u32;
    fn xm1_power_record_helper_756e24(output: *mut u32, request: u32);
    fn xm1_power_record_helper_750034(request: u32) -> u32;
    fn xm1_power_record_helper_fa7b84(alternate: u32, offset: i32) -> u32;
    fn xm1_power_record_helper_f5ecc0(kind: u32, width: u32, first: u32, second: u32) -> u32;
    fn xm1_power_record_helper_746c2c(value: u32) -> u32;
    fn xm1_power_record_helper_7580ec(request: u32) -> u32;
    fn xm1_power_record_helper_754d9c(request: u32) -> u32;
    fn xm1_power_record_helper_74fa2c(request: u32) -> u32;
    fn xm1_power_record_helper_75082c(request: u32) -> u32;
    fn xm1_power_record_helper_fa9588() -> u32;
    fn xm1_power_record_helper_7581e8(request: u32) -> u32;
    fn xm1_power_record_helper_f77c8c(input: *const u32, output: *mut u32);
    fn xm1_power_record_helper_f77dfc(mode: u32) -> u32;
    fn xm1_power_record_helper_758298(request: u32, mode: u32);
    fn xm1_power_record_helper_758278(request: u32) -> u32;
    fn xm1_power_record_helper_f77ef8(mode: u32, base: u32) -> u32;
    fn xm1_power_record_helper_757510(request: u32) -> u32;
    fn xm1_power_record_helper_f784f8(mode: u32, limit: u32, base: u32, current: u32) -> u32;
    fn xm1_power_record_helper_f77db4(mode: u32) -> u32;
    fn xm1_power_record_helper_f77e68(mode: u32) -> u32;
    fn xm1_power_record_helper_756734(request: u32, output: *mut u32);
    fn xm1_power_record_helper_7407b0();
    fn xm1_power_record_helper_f332d0(request: u32, record: u32, mode: u32) -> u32;
    fn xm1_power_record_helper_fa7f40() -> u32;
    fn xm1_power_record_helper_f36e74(request: u32, source: u32, cache: u32, area: u32, auxiliary: u32);
    fn xm1_power_record_helper_f1fc5c(record: u32, base: u32);
    fn xm1_power_record_helper_f36760(request: u32) -> i32;
    fn xm1_power_record_helper_f17abc(output: u32);
    fn xm1_power_record_helper_758250(request: u32, value: u32);
    fn xm1_power_record_helper_f24ba8(value: u32, first: i32, second: i32);
    fn xm1_power_record_helper_72872c(event: u32);
    fn xm1_power_record_helper_f33520(first: u32, second: u32);
    fn xm1_power_record_helper_f4c620(callback: unsafe extern "C" fn(u32, u32), source: u32);
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f36eac() {
    const OUTPUT: usize = 0x016bf9d0;
    const SNAPSHOT: usize = 0x016be3e8;
    const EXPOSURE: usize = 0x016be2d0;
    unsafe {
        put(OUTPUT + 4, xm1_power_record_helper_fa7634(word(0x016bfe48)));
        put_half(OUTPUT + 8, xm1_power_record_helper_74e718(word(0x016bfe48)) as u16);
        put(OUTPUT + 12, xm1_power_record_helper_74f1ac(word(0x016bfe48)));
        put(OUTPUT + 16, xm1_power_record_helper_74f9a4(word(0x016bfe48)));
        put(OUTPUT + 20, xm1_power_record_helper_74e7f0(word(0x016bfe48)));
        put_byte(OUTPUT + 24, xm1_power_record_helper_74e878(word(0x016bfe48)) as u8);
        put_half(OUTPUT + 26, xm1_power_record_helper_74e900(word(0x016bfe48)) as u16);
        put(OUTPUT + 0x2c, xm1_power_record_helper_74f674(word(0x016bfe48)));
        put(OUTPUT + 0x30, xm1_power_record_helper_74e5c0(word(0x016bfe48)));
        put_half(OUTPUT + 0x26, xm1_power_record_helper_74f5ec(word(0x016bfe48)) as u16);
        put_half(OUTPUT + 0x2a, xm1_power_record_helper_f5f9fc(word(0x016bfe48)) as u16);
        put_half(OUTPUT + 0x42, xm1_power_record_helper_74f6fc(word(0x016bfe48)) as u16);
        put(OUTPUT + 0x44, xm1_power_record_helper_74f784(word(0x016bfe48)));
        put_half(OUTPUT + 0x48, xm1_power_record_helper_74f80c(word(0x016bfe48)) as u16);
        put_half(OUTPUT + 0x4a, xm1_power_record_helper_74f91c(word(0x016bfe48)) as u16);
        let mode = xm1_power_record_helper_fa6bb4(word(0x016bfe48));
        put_half(OUTPUT + 0x24, mode as u16);
        put(OUTPUT + 0x20, xm1_power_record_helper_772ff8(mode));
        let setting = xm1_power_record_helper_74eb20(word(0x016bfe48));
        put(OUTPUT + 0x64, xm1_power_record_helper_7500bc(word(0x016bfe48)));
        put(OUTPUT + 0x88, xm1_power_record_helper_7503ec(word(0x016bfe48)));
        put(OUTPUT + 0xb4, xm1_power_record_helper_74f1f0(word(0x016bfe48)));
        put(OUTPUT + 0x90, xm1_power_record_helper_7509c4(word(0x016bfe48)));
        put_half(OUTPUT + 0x6c, xm1_power_record_helper_f5fc00(word(0x016bfe48)) as u16);
        put(OUTPUT + 0x70, xm1_power_record_helper_74ed6c(word(0x016bfe48)));
        put(OUTPUT + 0x74, xm1_power_record_helper_74f124(word(0x016bfe48)));
        put(OUTPUT + 0x78, xm1_power_record_helper_75060c(word(0x016bfe48)));
        put(OUTPUT + 0x7c, xm1_power_record_helper_74eba8(word(0x016bfe48)));
        put(OUTPUT + 0x80, xm1_power_record_helper_74f564(word(0x016bfe48)));
        let mode = xm1_power_record_helper_faaae8(word(0x016bfe48));
        put_half(OUTPUT + 0x58, xm1_power_record_helper_ef74e4(mode) as u16);
        put(OUTPUT + 0xd4, 0);
        let mut pair = [0; 2];
        xm1_power_record_helper_758c98(pair.as_mut_ptr(), word(0x016bfe48));
        put(OUTPUT + 0xa8, pair[0]);
        if pair[0] == 0 {
            pair[1] = 0;
            xm1_power_record_helper_758cc4(word(0x016bfe48), pair.as_ptr());
        }
        put(OUTPUT + 0xac, (xm1_power_record_helper_69f8fc(0x5256) & 3 != 0 && xm1_power_record_helper_750474(word(0x016bfe48)) != 0) as u32);
        xm1_power_record_helper_fea7ac(0, SNAPSHOT as u32);
        put(EXPOSURE + 0x28, word(SNAPSHOT + 0xf8));
        put_half(EXPOSURE + 0x2c, half(SNAPSHOT + 0xfc));
        put(EXPOSURE + 0x74, word(SNAPSHOT + 0xf4));
        let original = half(SNAPSHOT + 0xfe);
        put_half(EXPOSURE + 0x2e, original.wrapping_sub(xm1_power_record_helper_772f28(word(SNAPSHOT + 0xf4)) as u16));
        put_half(EXPOSURE + 0x86, half(SNAPSHOT + 0x100));
        put(EXPOSURE + 0x7c, word(SNAPSHOT + 0x104));
        put(EXPOSURE + 0x30, word(SNAPSHOT + 0xec));
        let limit = half(SNAPSHOT + 0xf0) as u32;
        let flag = byte(SNAPSHOT + 0xf2) as u32;
        let offset = signed_half(SNAPSHOT + 0x108);
        let margin = xm1_power_record_helper_757c60(word(0x016bfe48));
        put_half(EXPOSURE + 0x80, margin as u16);
        let base = xm1_power_record_helper_757e38(word(0x016bfe48));
        xm1_power_record_helper_f33164(word(EXPOSURE + 0x28), signed_half(EXPOSURE + 0x2c), signed_half(EXPOSURE + 0x2e), offset, signed_half(OUTPUT + 0x58), signed_half(EXPOSURE + 0x86), margin as i32, word(EXPOSURE + 0x74), word(EXPOSURE + 0x30), limit, flag, word(EXPOSURE + 0x7c), base);
        let request = word(0x016bfe48);
        xm1_power_record_helper_757e58(request, xm1_power_record_helper_76d9c8());
        put_half(OUTPUT + 0x38, half(0x016bf954));
        put(OUTPUT + 0x1c, xm1_power_record_helper_757e38(word(0x016bfe48)));
        put(OUTPUT + 0x34, xm1_power_record_helper_757b78(word(0x016bfe48)));
        put(OUTPUT + 0x4c, xm1_power_record_helper_757df0(word(0x016bfe48)));
        put_half(OUTPUT + 0x54, half(0x016bf964));
        put_half(OUTPUT + 0x56, half(0x016bf962));
        let selection = xm1_power_record_helper_756ac4(word(0x016bfe48));
        put(OUTPUT + 0x68, selection);
        let cache = xm1_power_record_helper_fab250(word(OUTPUT + 0x30), word(OUTPUT + 4), selection, setting);
        put(OUTPUT + 0x60, cache);
        xm1_power_record_helper_fa7aa4(word(OUTPUT + 0x30), word(OUTPUT + 0x68), cache, word(OUTPUT + 0xb4), OUTPUT as u32);
        let dimensions = xm1_power_record_helper_75817c(word(0x016bfe48));
        put_half(OUTPUT + 0xa4, dimensions as u16);
        put_half(OUTPUT + 0xa6, (dimensions >> 16) as u16);
        let mut geometry = [0u32; 5];
        xm1_power_record_helper_756e24(geometry.as_mut_ptr(), word(0x016bfe48));
        put(OUTPUT + 0x98, half(geometry.as_ptr() as usize + 2) as u32);
        put(OUTPUT + 0x9c, half(geometry.as_ptr() as usize) as u32);
        put(OUTPUT + 0xa0, half(geometry.as_ptr() as usize + 4) as u32);
        let active = xm1_power_record_helper_f5fc00(word(0x016bfe48));
        put(OUTPUT + 0x50, if active <= 1 { 1 - active } else { 0 });
        put(OUTPUT + 0x5c, xm1_power_record_helper_750034(word(0x016bfe48)));
        put_byte(OUTPUT + 0x5a, xm1_power_record_helper_fa7b84(word(SNAPSHOT + 0x104), signed_half(OUTPUT + 0x26)) as u8);
        put_half(OUTPUT + 0x84, xm1_power_record_helper_f5ecc0(word(OUTPUT + 4), half(OUTPUT + 8) as u32, word(OUTPUT + 12), word(OUTPUT + 16)) as u16);
        let base = xm1_power_record_helper_757e38(word(0x016bfe48));
        put_half(OUTPUT + 0x86, xm1_power_record_helper_746c2c(base) as u16);
        put(OUTPUT + 0x8c, xm1_power_record_helper_7580ec(word(0x016bfe48)));
        put(OUTPUT + 0xd0, xm1_power_record_helper_754d9c(word(0x016bfe48)));
        let alternate = if xm1_power_record_helper_74fa2c(word(0x016bfe48)) == 6 && xm1_power_record_helper_75082c(word(0x016bfe48)) == 0 { xm1_power_record_helper_fa9588() } else { 0 };
        put(OUTPUT + 0x94, alternate);
        let mut input = [word(OUTPUT + 0x30), selection, word(OUTPUT + 0x8c), word(OUTPUT + 4), word(OUTPUT + 0xa8), word(OUTPUT + 0xd4), word(OUTPUT + 0xd0), alternate, 0, 0];
        put_byte(input.as_mut_ptr() as usize + 38, 0);
        put_byte(input.as_mut_ptr() as usize + 39, 2);
        let disposition = if word(0x016bfe50) == 0 {
            let mode = xm1_power_record_helper_7581e8(word(0x016bfe48));
            let mut selected = 0;
            xm1_power_record_helper_f77c8c(input.as_ptr(), &mut selected);
            let first = xm1_power_record_helper_f77dfc(mode) == 1;
            let second = xm1_power_record_helper_f77dfc(selected) == 1;
            if first != second {
                xm1_power_record_helper_758298(word(0x016bfe48), 2);
                put(0x016bfe50, 1);
                2
            } else {
                let disposition = first as u32;
                xm1_power_record_helper_758298(word(0x016bfe48), disposition);
                disposition
            }
        } else { xm1_power_record_helper_758278(word(0x016bfe48)) };
        put(OUTPUT + 0xcc, word(0x016bfe50));
        let current = if disposition == 1 { word(OUTPUT + 0x44) } else if disposition == 2 { word(OUTPUT + 0x34) } else { xm1_power_record_helper_f77ef8(xm1_power_record_helper_7581e8(word(0x016bfe48)), word(OUTPUT + 0x1c)) };
        let mode = xm1_power_record_helper_7581e8(word(0x016bfe48));
        let limit = xm1_power_record_helper_757510(word(0x016bfe48));
        let base = xm1_power_record_helper_757e38(word(0x016bfe48));
        put_half(input.as_mut_ptr() as usize + 36, xm1_power_record_helper_f784f8(mode, limit, base, current) as u16);
        let mode = xm1_power_record_helper_7581e8(word(0x016bfe48));
        let kind = xm1_power_record_helper_f77db4(mode);
        input[8] = (alternate == 1 && kind.wrapping_sub(1) < 2) as u32;
        xm1_power_record_helper_f77c8c(input.as_ptr(), (OUTPUT + 0xb0) as *mut u32);
        let mode = xm1_power_record_helper_7581e8(word(0x016bfe48));
        let selected = byte(OUTPUT + 0xb0) as u32;
        let changed = (xm1_power_record_helper_f77e68(mode) == 4 && xm1_power_record_helper_f77e68(selected) == 3) || (xm1_power_record_helper_f77e68(mode) == 3 && xm1_power_record_helper_f77e68(selected) == 4);
        put(OUTPUT + 0xc8, changed as u32);
        xm1_power_record_helper_756734(word(0x016bfe48), 0x016bf254 as *mut u32);
        let active = word(0x016bf2b4);
        put(OUTPUT, if active <= 1 { 1 - active } else { 0 });
        if word(OUTPUT + 0x30) == 0 { xm1_power_record_helper_7407b0(); }
        put(OUTPUT + 0xb8, xm1_power_record_helper_f332d0(word(0x016bfe48), 0x016bf374, 1));
        let retained = xm1_power_record_helper_fa7f40() == 1;
        put_byte(OUTPUT + 0xdc, retained as u8);
        put_byte(0x016bfb84, retained as u8);
        put(OUTPUT + 0xc4, 0);
        if word(OUTPUT + 0xb8) != 0 || retained {
            xm1_power_record_helper_f36e74(word(0x016bfe48), OUTPUT as u32, 0x016bf370, 0x41717f78, 0x41718020);
            put(OUTPUT + 0xbc, 0x41717f78);
            put(OUTPUT + 0xc0, 0x41718020);
            if byte(OUTPUT + 0xdc) == 1 {
                let base = xm1_power_record_helper_757e38(word(0x016bfe48));
                xm1_power_record_helper_f1fc5c(0x41718020, base);
                put_half(0x417180a8, half(OUTPUT + 0x42));
                put(0x417180ac, word(OUTPUT + 0x44));
                put_half(0x417180a0, half(OUTPUT + 0x26));
                put_half(0x41718030, xm1_power_record_helper_f36760(word(0x016bfe48)) as u16);
                put(OUTPUT + 0xc0, 0x41718020);
                xm1_power_record_helper_f17abc(0x4171814c);
                put(OUTPUT + 0xc4, 0x4171814c);
            }
            if word(OUTPUT + 0xb8) != 0 {
                put(0x41717f70, 1);
                xm1_power_record_helper_758250(word(0x016be01c), 0);
            }
        } else if word(OUTPUT + 0xd4) == 1 {
            xm1_power_record_helper_f36e74(word(0x016bfe48), OUTPUT as u32, 0x016bf370, 0x41717f78, 0x41718020);
            put(OUTPUT + 0xbc, 0x41717f78);
            put(OUTPUT + 0xc0, 0x41718020);
        }
        put(0x41717f74, xm1_power_record_helper_76d9c8());
        if byte(OUTPUT + 0xdc) == 1 { xm1_power_record_helper_f24ba8(word(OUTPUT + 0x44), signed_half(OUTPUT + 0x42), signed_half(OUTPUT + 0x26)); }
        if xm1_power_record_helper_69f8fc(0x24e) == 1 { xm1_power_record_helper_72872c(0x01102000); }
        xm1_power_record_helper_f4c620(xm1_power_record_helper_f33520, OUTPUT as u32);
    }
}
