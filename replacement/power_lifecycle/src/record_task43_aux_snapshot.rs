use core::ptr::{read_volatile, write_volatile};
unsafe fn word(address: usize) -> u32 { unsafe { read_volatile(address as *const u32) } }
unsafe fn half(address: usize) -> u16 { unsafe { read_volatile(address as *const u16) } }
unsafe fn put(address: usize, value: u32) { unsafe { write_volatile(address as *mut u32, value) } }
unsafe fn put_half(address: usize, value: u16) { unsafe { write_volatile(address as *mut u16, value) } }
unsafe fn put_byte(address: usize, value: u8) { unsafe { write_volatile(address as *mut u8, value) } }
unsafe fn copy(destination: usize, source: usize, length: usize) { unsafe { core::ptr::copy_nonoverlapping(source as *const u8, destination as *mut u8, length); } }
unsafe extern "C" {
    fn xm1_power_record_helper_fa7634(request: u32) -> u32;
    fn xm1_power_record_helper_74e718(request: u32) -> u32;
    fn xm1_power_record_helper_fa7954(request: u32) -> u32;
    fn xm1_power_record_helper_74f564(request: u32) -> u32;
    fn xm1_power_record_helper_74f5ec(request: u32) -> u32;
    fn xm1_power_record_helper_74ec30(request: u32) -> u32;
    fn xm1_power_record_helper_74e5c0(request: u32) -> u32;
    fn xm1_power_record_helper_74e7f0(request: u32) -> u32;
    fn xm1_power_record_helper_74eb20(request: u32) -> u32;
    fn xm1_power_record_helper_756e24(output: *mut u32, request: u32);
    fn xm1_power_record_helper_74fa2c(request: u32) -> u32;
    fn xm1_power_record_helper_f5206c(output: *mut u32, request: u32);
    fn xm1_power_record_helper_fea7ac(index: u32, output: u32);
    fn xm1_power_record_helper_772f28(value: u32) -> u32;
    fn xm1_power_record_helper_faaae8(request: u32) -> u32;
    fn xm1_power_record_helper_ef74e4(value: u32) -> u32;
    fn xm1_power_record_helper_746bf8(value: u32, base: u32) -> u32;
    fn xm1_power_record_helper_757c60(request: u32) -> u32;
    fn xm1_power_record_helper_756ac4(request: u32) -> u32;
    fn xm1_power_record_helper_fa7a7c(mode: u32, kind: u32, alternate: u32) -> u32;
    fn xm1_power_record_helper_fa7e20(value: i32, mode: u32, kind: u32) -> u32;
    fn xm1_power_record_helper_f33164(kind: u32, first: i32, second: i32, third: i32, fourth: i32, fifth: i32, sixth: i32, adjustment: u32, mode: u32, limit: u32, flag: u32, alternate: u32, base: u32);
    fn xm1_power_record_helper_757e58(request: u32, base: u32);
    fn xm1_power_record_helper_757cb0(request: u32) -> u32;
    fn xm1_power_record_helper_7503ec(request: u32) -> u32;
    fn xm1_power_record_helper_7588e8(output: *mut u32, request: u32);
    fn xm1_power_record_helper_746834(value: u32) -> u32;
    fn xm1_power_record_helper_75890c(request: u32, input: u32);
    fn xm1_power_record_helper_756734(request: u32, output: *mut u32);
    fn xm1_power_record_helper_fab250(mode: u32, kind: u32, selection: u32, setting: u32) -> u32;
    fn xm1_power_record_helper_69f8fc(key: u32) -> u32;
    fn xm1_power_record_helper_f4f16c(request: u32, base: u32) -> u32;
    fn xm1_power_record_helper_7527f8(request: u32) -> u32;
    fn xm1_power_record_helper_752890(request: u32) -> u32;
    fn xm1_power_record_helper_fa7bb8(input: u32, output: u32);
    fn xm1_power_record_helper_72872c(event: u32);
    fn xm1_power_record_helper_f33568(first: u32, second: u32);
    fn xm1_power_record_helper_f4c584(callback: unsafe extern "C" fn(u32, u32), source: u32, cache: u32);
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f3579c() {
    const OUTPUT: usize = 0x016bfb88;
    const SNAPSHOT: usize = 0x016bec90;
    const CACHE: usize = 0x016bf370;
    unsafe {
        put(OUTPUT, xm1_power_record_helper_fa7634(word(0x016bfe48)));
        put_half(OUTPUT + 4, xm1_power_record_helper_74e718(word(0x016bfe48)) as u16);
        put(OUTPUT + 8, xm1_power_record_helper_fa7954(word(0x016bfe48)));
        put(OUTPUT + 0x20, xm1_power_record_helper_74f564(word(0x016bfe48)));
        put_half(OUTPUT + 0x1c, xm1_power_record_helper_74f5ec(word(0x016bfe48)) as u16);
        let dimensions = xm1_power_record_helper_74ec30(word(0x016bfe48));
        put_half(OUTPUT + 0x6e, dimensions as u16);
        put_half(OUTPUT + 0x70, (dimensions >> 16) as u16);
        put(OUTPUT + 0x14, xm1_power_record_helper_74e5c0(word(0x016bfe48)));
        xm1_power_record_helper_74e7f0(word(0x016bfe48));
        let setting = xm1_power_record_helper_74eb20(word(0x016bfe48));
        let mut geometry = [0u32; 5];
        xm1_power_record_helper_756e24(geometry.as_mut_ptr(), word(0x016bfe48));
        copy(OUTPUT + 0x88, geometry.as_ptr() as usize, 20);
        put(OUTPUT + 0x9c, xm1_power_record_helper_74fa2c(word(0x016bfe48)));
        xm1_power_record_helper_f5206c(geometry.as_mut_ptr(), word(0x016bfe48));
        copy(OUTPUT + 0xa0, geometry.as_ptr() as usize, 8);
        xm1_power_record_helper_fea7ac(0, SNAPSHOT as u32);
        put(OUTPUT + 0x10, ((word(SNAPSHOT) & 5) == 5) as u32);
        put(OUTPUT + 0x28, word(SNAPSHOT + 0xf8));
        put_half(OUTPUT + 0x2c, half(SNAPSHOT + 0xfc));
        put(OUTPUT + 0x74, word(SNAPSHOT + 0xf4));
        let exposure = half(SNAPSHOT + 0xfe);
        put_half(OUTPUT + 0x2e, exposure.wrapping_sub(xm1_power_record_helper_772f28(word(SNAPSHOT + 0xf4)) as u16));
        put_half(OUTPUT + 0x86, half(SNAPSHOT + 0x100));
        let mapped = xm1_power_record_helper_faaae8(word(0x016bfe48));
        put_half(OUTPUT + 0x84, xm1_power_record_helper_ef74e4(mapped) as u16);
        let base = word(SNAPSHOT + 0x110);
        let mapped = xm1_power_record_helper_746bf8(word(OUTPUT + 0x28), base);
        put_half(OUTPUT + 0x24, half(OUTPUT + 0x2c).wrapping_sub(half(OUTPUT + 0x84).wrapping_add(half(OUTPUT + 0x2e))).wrapping_sub(half(OUTPUT + 0x86)).wrapping_add(mapped as u16));
        put(OUTPUT + 0x7c, word(SNAPSHOT + 0x104));
        put(OUTPUT + 0x30, word(SNAPSHOT + 0xec));
        let limit = half(SNAPSHOT + 0xf0) as u32;
        let flag = read_volatile((SNAPSHOT + 0xf2) as *const u8) as u32;
        put_half(OUTPUT + 0x34, half(SNAPSHOT + 0x108));
        put_half(OUTPUT + 0x80, xm1_power_record_helper_757c60(word(0x016bfe48)) as u16);
        if word(OUTPUT + 0x7c) == 1 { put_half(OUTPUT + 0x24, half(OUTPUT + 0x24).wrapping_sub(100)); }
        for (destination, origin) in [(0x38, 0xe4), (0x3c, 0xe8), (0x40, 0xc), (0x44, 0x10), (0x48, 0x14), (0x4c, 0x18)] { put(OUTPUT + destination, word(SNAPSHOT + origin)); }
        for (offset, value) in [(0x50, 100), (0x52, 100), (0x54, 256), (0x56, 100), (0x58, 100), (0x5a, 1)] { put_half(OUTPUT + offset, value); }
        for (offset, value) in [(0x64, 0x1000), (0x68, 16), (0x5c, 0x5ffec1b4), (0x60, 0x5ffed1b4)] { put(OUTPUT + offset, value); }
        let selection = xm1_power_record_helper_756ac4(word(0x016bfe48));
        put(OUTPUT + 0x18, selection);
        put_byte(OUTPUT + 0x6c, xm1_power_record_helper_fa7a7c(word(OUTPUT + 0x14), selection, 0) as u8);
        put(OUTPUT + 0x78, xm1_power_record_helper_fa7e20(half(OUTPUT + 0x2c) as i16 as i32, word(OUTPUT + 0x14), selection));
        xm1_power_record_helper_f33164(word(OUTPUT + 0x28), half(OUTPUT + 0x2c) as i16 as i32, half(OUTPUT + 0x2e) as i16 as i32, half(OUTPUT + 0x34) as i16 as i32, half(OUTPUT + 0x84) as i16 as i32, half(OUTPUT + 0x86) as i16 as i32, half(OUTPUT + 0x80) as i16 as i32, word(OUTPUT + 0x74), word(OUTPUT + 0x30), limit, flag, word(OUTPUT + 0x7c), base);
        xm1_power_record_helper_757e58(word(0x016bfe48), base);
        put_half(OUTPUT + 0x82, xm1_power_record_helper_757cb0(word(0x016bfe48)) as u16);
        if xm1_power_record_helper_7503ec(word(0x016bfe48)) == 1 {
            let mut grid = [0; 19];
            xm1_power_record_helper_7588e8(grid.as_mut_ptr(), word(0x016bfe48));
            copy(0x016bec44, grid.as_ptr() as usize, 76);
            put(0x016bec7c, base);
            put(0x016bec80, xm1_power_record_helper_746834(word(OUTPUT + 0x28)));
            xm1_power_record_helper_75890c(word(0x016bfe48), 0x016bec44);
        }
        xm1_power_record_helper_756734(word(0x016bfe48), 0x016bfab4 as *mut u32);
        let active = word(0x016bfab8);
        put(OUTPUT + 12, if active <= 1 { 1 - active } else { 0 });
        put(CACHE, xm1_power_record_helper_fab250(word(OUTPUT + 0x14), word(OUTPUT), selection, setting));
        if word(CACHE) == 0 { put(CACHE + 4, 0); }
        else if xm1_power_record_helper_69f8fc(0x4a07) != 0 {
            copy(CACHE + 4, SNAPSHOT + 0x118, 0x414);
            copy(CACHE + 0x418, SNAPSHOT + 0x52c, 24);
            copy(CACHE + 0x430, SNAPSHOT + 0x544, 40);
            for index in 0..(word(SNAPSHOT + 0x118) & 0xffff) as usize {
                put(CACHE + 0x45c + index * 8, word(SNAPSHOT + 0x84 + index * 8));
                put(CACHE + 0x460 + index * 8, word(SNAPSHOT + 0x88 + index * 8));
            }
            put(CACHE + 0x4b0, 320);
            put(CACHE + 0x4ac, 0x5ffed1c4);
        } else {
            put(CACHE + 0x41c, xm1_power_record_helper_f4f16c(word(0x016bfe48), base));
            put(CACHE + 0x420, xm1_power_record_helper_7527f8(word(0x016bfe48)));
            put(CACHE + 0x424, xm1_power_record_helper_752890(word(0x016bfe48)));
            copy(CACHE + 0x428, OUTPUT + 0x6e, 4);
            put(CACHE + 0x418, word(OUTPUT + 0x30));
            xm1_power_record_helper_fa7bb8((CACHE + 0x418) as u32, CACHE as u32);
        }
        put_byte(CACHE + 0x458, xm1_power_record_helper_fa7a7c(word(OUTPUT + 0x14), selection, 1) as u8);
        if xm1_power_record_helper_69f8fc(0x24e) == 1 { xm1_power_record_helper_72872c(0x01101000); }
        xm1_power_record_helper_f4c584(xm1_power_record_helper_f33568, OUTPUT as u32, CACHE as u32);
    }
}
