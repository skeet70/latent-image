use core::ptr::{read_volatile, write_volatile};
unsafe fn word(address: usize) -> u32 { unsafe { read_volatile(address as *const u32) } }
unsafe fn half(address: usize) -> u16 { unsafe { read_volatile(address as *const u16) } }
unsafe fn signed_half(address: usize) -> i32 { unsafe { half(address) as i16 as i32 } }
unsafe fn byte(address: usize) -> u8 { unsafe { read_volatile(address as *const u8) } }
unsafe fn put(address: usize, value: u32) { unsafe { write_volatile(address as *mut u32, value) } }
unsafe fn put_half(address: usize, value: u16) { unsafe { write_volatile(address as *mut u16, value) } }
unsafe fn put_byte(address: usize, value: u8) { unsafe { write_volatile(address as *mut u8, value) } }
unsafe extern "C" {
    fn xm1_power_record_helper_74e5c0(request: u32) -> u32;
    fn xm1_power_record_helper_757e80(request: u32) -> u32;
    fn xm1_power_record_helper_fa7604(value: u32) -> u32;
    fn xm1_power_record_helper_757bc0(request: u32) -> u32;
    fn xm1_power_record_helper_757c10(request: u32) -> u32;
    fn xm1_power_record_helper_757da0(request: u32) -> u32;
    fn xm1_power_record_helper_757df0(request: u32) -> u32;
    fn xm1_power_record_helper_757d00(request: u32) -> u32;
    fn xm1_power_record_helper_757f60(request: u32) -> u32;
    fn xm1_power_record_helper_fa7edc(value: u32) -> u32;
    fn xm1_power_record_helper_ef7820(value: u32) -> u32;
    fn xm1_power_record_helper_75082c(request: u32) -> u32;
    fn xm1_power_record_helper_757e38(request: u32) -> u32;
    fn xm1_power_record_helper_74f1f0(request: u32) -> u32;
    fn xm1_power_record_helper_757ec8(request: u32) -> u32;
    fn xm1_power_record_helper_757f18(request: u32) -> u32;
    fn xm1_power_record_helper_fa7634(request: u32) -> u32;
    fn xm1_power_record_helper_754d9c(request: u32) -> u32;
    fn xm1_power_record_helper_758278(request: u32) -> u32;
    fn xm1_power_record_helper_74f784(request: u32) -> u32;
    fn xm1_power_record_helper_7581e8(request: u32) -> u32;
    fn xm1_power_record_helper_f77ef8(mode: u32, base: u32) -> u32;
    fn xm1_power_record_helper_757510(request: u32) -> u32;
    fn xm1_power_record_helper_f784f8(mode: u32, limit: u32, base: u32, current: u32) -> u32;
    fn xm1_power_record_helper_f77db4(mode: u32) -> u32;
    fn xm1_power_record_helper_f77c8c(input: *const u32, output: *mut u32);
    fn xm1_power_record_helper_758208(request: u32, mode: u32);
    fn xm1_power_record_helper_f7809c(mode: u32, base: u32, output: *mut u32, shape: *mut u32);
    fn xm1_power_record_helper_f7a0cc(mode: u32, kind: u32, adjustment: u32, first: *mut i16, second: *mut i16, third: *mut i16, offset: i32, choice: u32, extra: u32, flag: u32, alternate: u32, base: u32, extra2: u32, index: u32, table: *mut u32, output_half: *mut u16, output_byte: *mut u8) -> u32;
    fn xm1_power_record_helper_772f28(value: u32) -> u32;
    fn xm1_power_record_helper_757c60(request: u32) -> u32;
    fn xm1_power_record_helper_ef5310(value: u32) -> u32;
    fn xm1_power_record_helper_746bf8(value: u32, base: u32) -> u32;
    fn xm1_power_record_helper_fa7e8c(value: u32);
    fn xm1_power_record_helper_758068(request: u32, output: *mut u32);
    fn xm1_power_record_helper_758098(request: u32, input: *const u32);
    fn xm1_power_record_helper_fea5c0(input: u32);
    fn xm1_power_record_helper_740e74(address: u32, length: u32);
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f35d14() -> i32 {
    const OUTPUT: usize = 0x016be1a8;
    const SUMMARY: usize = 0x016bf954;
    unsafe {
        let source = word(0x0169d384) as usize;
        if source == 0 { return 0; }
        let kind = xm1_power_record_helper_74e5c0(word(0x016bfe48));
        let selection = word(source + 0xdc);
        let mut table = [xm1_power_record_helper_757e80(word(0x016bfe48))];
        let previous_kind = xm1_power_record_helper_fa7604(table[0]);
        let previous_first = xm1_power_record_helper_757bc0(word(0x016bfe48));
        let previous_second = xm1_power_record_helper_757c10(word(0x016bfe48));
        let previous_third = xm1_power_record_helper_757da0(word(0x016bfe48));
        let previous_adjustment = xm1_power_record_helper_757df0(word(0x016bfe48));
        let previous_limit = xm1_power_record_helper_757d00(word(0x016bfe48));
        let previous_extra = xm1_power_record_helper_757f60(word(0x016bfe48));
        let adjustment = xm1_power_record_helper_fa7edc(selection);
        let extra = word(source + 0xe0);
        let choice = word(source + 0xbc);
        let mapped = xm1_power_record_helper_ef7820(choice);
        let flag = xm1_power_record_helper_75082c(word(0x016bfe48));
        let base = xm1_power_record_helper_757e38(word(0x016bfe48));
        let index = xm1_power_record_helper_74f1f0(word(0x016bfe48));
        let previous_half = xm1_power_record_helper_757ec8(word(0x016bfe48));
        let previous_byte = xm1_power_record_helper_757f18(word(0x016bfe48));
        let alternate = word(source + 0xa4);
        let mut input = [0u32; 10];
        input[0] = kind;
        input[1] = selection;
        input[2] = choice;
        input[3] = xm1_power_record_helper_fa7634(word(0x016bfe48));
        input[4] = extra;
        input[5] = word(source + 0xe8);
        input[6] = xm1_power_record_helper_754d9c(word(0x016bfe48));
        input[7] = alternate;
        if word(source + 0xd8) == 0 { put(0x016bfe50, 0); }
        let disposition = xm1_power_record_helper_758278(word(0x016bfe48));
        let current = if disposition == 1 { xm1_power_record_helper_74f784(word(0x016bfe48)) } else if disposition == 2 { previous_kind } else { xm1_power_record_helper_f77ef8(xm1_power_record_helper_7581e8(word(0x016bfe48)), base) };
        let mode = xm1_power_record_helper_7581e8(word(0x016bfe48));
        let limit = xm1_power_record_helper_757510(word(0x016bfe48));
        let current_base = xm1_power_record_helper_757e38(word(0x016bfe48));
        put_half(input.as_mut_ptr() as usize + 36, xm1_power_record_helper_f784f8(mode, limit, current_base, current) as u16);
        let mode = xm1_power_record_helper_7581e8(word(0x016bfe48));
        let selection_kind = xm1_power_record_helper_f77db4(mode);
        input[8] = (alternate == 1 && selection_kind.wrapping_sub(1) < 2) as u32;
        put_byte(input.as_mut_ptr() as usize + 38, 0);
        put_byte(input.as_mut_ptr() as usize + 39, 2);
        let mut mode = 0;
        xm1_power_record_helper_f77c8c(input.as_ptr(), &mut mode);
        xm1_power_record_helper_758208(word(0x016bfe48), mode & 0xff);
        let mut offset = 0;
        let mut shape = 0;
        xm1_power_record_helper_f7809c(mode, base, &mut offset, &mut shape);
        let mut output_half = 0;
        let mut output_byte = 0;
        for channel in 0..4 {
            let row = OUTPUT + channel * 36;
            for (destination, source_kind, first, second, third, source_offset, additional) in [(row, 0x58, 0x5c, 0x64, 0x6c, 0x74, word(source + 0xb0)), (row + 0x90, 0x34, 0x38, 0x40, 0x48, 0x50, 0)] {
                let value = xm1_power_record_helper_f7a0cc(mode, word(source + source_kind), offset, (source + first + channel * 2) as *mut i16, (source + second + channel * 2) as *mut i16, (source + third + channel * 2) as *mut i16, signed_half(source + source_offset + channel * 2), choice, word(source + 0xa8), flag, alternate, base, additional, index, table.as_mut_ptr(), &mut output_half, &mut output_byte);
                put(destination, value);
                put_half(destination + 4, output_half);
                put_byte(destination + 6, output_byte);
                put(destination + 12, word(source + source_kind));
                put_half(destination + 16, half(source + first + channel * 2));
                let value = half(source + second + channel * 2);
                let correction = xm1_power_record_helper_772f28(adjustment);
                put_half(destination + 18, value.wrapping_add(correction as u16));
                put_half(destination + 20, half(source + third + channel * 2));
                put_half(destination + 28, half(source + 0x9e));
                put(destination + 8, adjustment);
                put(destination + 24, word(source + 0xa0));
                put(destination + 32, mapped);
            }
            if alternate != 0 {
                for offset in [0, 8, 12, 24, 32] { put(row + offset, word(row + 0x90 + offset)); }
                for offset in [4, 16, 18, 20, 28] { put_half(row + offset, half(row + 0x90 + offset)); }
                put_byte(row + 6, byte(row + 0x96));
            }
        }
        let changed = word(source + 0x58) != previous_kind || signed_half(source + 0x62) as u32 != previous_first || signed_half(source + 0x6a) as u32 != previous_second || previous_adjustment != adjustment || signed_half(source + 0x72) as u32 != previous_third || signed_half(source + 0x9e) as u32 != previous_limit || word(source + 0xa0) != previous_extra || word(OUTPUT + 0x6c) != table[0] || output_half as u32 != previous_half || output_byte as u32 != previous_byte;
        put(SUMMARY + 4, word(OUTPUT + 0x78));
        put_half(SUMMARY + 8, half(OUTPUT + 0x7c));
        let value = half(OUTPUT + 0x7e);
        put_half(SUMMARY + 10, value.wrapping_sub(xm1_power_record_helper_772f28(adjustment) as u16));
        put_half(SUMMARY + 12, half(OUTPUT + 0x80));
        put(SUMMARY + 0x14, word(OUTPUT + 0x6c));
        put_half(SUMMARY + 16, xm1_power_record_helper_757c60(word(0x016bfe48)) as u16);
        put_half(SUMMARY + 14, xm1_power_record_helper_ef5310(word(SUMMARY + 0x14)) as u16);
        let value = xm1_power_record_helper_746bf8(word(SUMMARY + 4), base);
        put_half(SUMMARY, half(SUMMARY + 8).wrapping_sub(half(SUMMARY + 12).wrapping_add(half(SUMMARY + 10))).wrapping_add(value as u16));
        put_half(SUMMARY + 24, half(OUTPUT + 0x70));
        put_byte(SUMMARY + 26, byte(OUTPUT + 0x72));
        xm1_power_record_helper_fa7e8c(word(SUMMARY + 0x14));
        if alternate != 0 {
            let mut profile = [0u32; 6];
            xm1_power_record_helper_758068(word(0x016bfe48), profile.as_mut_ptr());
            put_byte(profile.as_mut_ptr() as usize + 4, 1);
            profile[2] = word(OUTPUT + 0x108);
            put_half(profile.as_mut_ptr() as usize + 12, half(OUTPUT + 0xa0));
            profile[4] = word(OUTPUT + 0xfc);
            put_half(profile.as_mut_ptr() as usize + 20, half(OUTPUT + 0x100));
            put_byte(profile.as_mut_ptr() as usize + 22, byte(OUTPUT + 0x102));
            xm1_power_record_helper_758098(word(0x016bfe48), profile.as_ptr());
        }
        for offset in [0, 2, 4] { put_half(OUTPUT + 0x120 + offset, half(source + 0x18 + offset)); }
        if changed { xm1_power_record_helper_fea5c0(OUTPUT as u32); }
        xm1_power_record_helper_740e74(source as u32, 0x114);
        changed as i32
    }
}
