use core::ptr::{read_volatile, write_volatile};

#[derive(Clone, Copy)]
#[repr(transparent)]
struct Record(usize);
impl Record {
    unsafe fn word(self, offset: usize) -> u32 { unsafe { read_volatile((self.0 + offset) as *const u32) } }
    unsafe fn half(self, offset: usize) -> u32 { unsafe { read_volatile((self.0 + offset) as *const u16) as u32 } }
    unsafe fn byte(self, offset: usize) -> u32 { unsafe { read_volatile((self.0 + offset) as *const u8) as u32 } }
    unsafe fn put(self, offset: usize, value: u32) { unsafe { write_volatile((self.0 + offset) as *mut u32, value) } }
    unsafe fn put_half(self, offset: usize, value: u32) { unsafe { write_volatile((self.0 + offset) as *mut u16, value as u16) } }
    unsafe fn put_byte(self, offset: usize, value: u32) { unsafe { write_volatile((self.0 + offset) as *mut u8, value as u8) } }
}

unsafe extern "C" {
    fn xm1_power_record_helper_757e38(request: u32) -> u32;
    fn xm1_power_record_helper_fa7f40() -> u32;
    fn xm1_power_record_helper_f36760(request: u32) -> i32;
    fn xm1_power_record_helper_f625ec(value: u32) -> u32;
    fn xm1_power_record_helper_fa7954(request: u32) -> u32;
    fn xm1_power_record_helper_fa7984(request: u32) -> u32;
    fn xm1_power_record_helper_f624a8(output: *mut u32, value: u32, offset: i32, width: u32, height: u32, kind: u32, flag: u32, index: u32, enabled: u32);
    fn xm1_power_record_helper_74eb20(request: u32) -> u32;
    fn xm1_power_record_helper_74fa2c(request: u32) -> u32;
    fn xm1_power_record_helper_74fc64(request: u32) -> u32;
    fn xm1_power_record_helper_fa66d4(kind: u32, flag: u32, mode: u32, width: u32, choice: u32, enabled: u32, resource: u32, extra: u32) -> u32;
    fn xm1_power_record_helper_fa672c(kind: u32, flag: u32, mode: u32, width: u32, choice: u32, enabled: u32, resource: u32, extra: u32) -> u32;
    fn xm1_power_record_helper_f162e4(output: u32, request: u32, zero: u32, enabled: u32, extra: u32);
    fn xm1_power_record_helper_74ea10(request: u32) -> u32;
    fn xm1_power_record_helper_f4ddbc(width: u32, height: u32) -> u32;
    fn xm1_power_record_helper_f5f9fc(request: u32) -> u32;
    fn xm1_power_record_helper_750ac0(request: u32) -> u32;
    fn xm1_power_record_helper_750c00(request: u32) -> u32;
    fn xm1_power_record_helper_750c88(request: u32) -> u32;
    fn xm1_power_record_helper_754f8c(request: u32) -> u32;
    fn xm1_power_record_helper_75502c(request: u32) -> u32;
    fn xm1_power_record_helper_75569c(request: u32) -> u32;
    fn xm1_power_record_helper_75516c(request: u32) -> u32;
    fn xm1_power_record_helper_754d9c(request: u32) -> u32;
    fn xm1_power_record_helper_74fb54(request: u32) -> u32;
    fn xm1_power_record_helper_f51b94(request: u32, base: u32) -> u32;
    fn xm1_power_record_helper_7569a4(request: u32) -> u32;
    fn xm1_power_record_helper_758134(request: u32) -> u32;
    fn xm1_power_record_helper_faaae8(request: u32) -> u32;
    fn xm1_power_record_helper_75a178(request: u32) -> u32;
    fn xm1_power_record_helper_74081c() -> u32;
    fn xm1_power_record_helper_74f564(request: u32) -> u32;
    fn xm1_power_record_helper_f5206c(output: *mut u32, request: u32);
    fn xm1_power_record_helper_757c60(request: u32) -> u32;
    fn xm1_power_record_helper_772f28(value: u32) -> u32;
    fn xm1_power_record_helper_746c2c(value: u32) -> u32;
    fn xm1_power_record_helper_f5ecb0(mode: u32, width: u32, height: u32) -> u32;
    fn xm1_power_record_helper_f5ecc0(width: u32, height: u32, mode: u32, kind: u32) -> u32;
    fn xm1_power_record_helper_746bf8(value: u32, base: u32) -> u32;
    fn xm1_power_record_helper_772ff8(value: i32) -> u32;
    fn xm1_power_record_helper_69f8fc(key: u32) -> u32;
    fn xm1_power_record_helper_fa6934(width: u32, resource: u32, base: u32, height: u32, zero: u32, extra: u32) -> u32;
    fn xm1_power_record_helper_74f344(request: u32) -> u32;
    fn xm1_power_record_helper_740888(zero: u32, output: *mut u32, second: u32, third: u32);
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f367a4(request: u32, enabled: u32, source: u32, cache: u32, area: u32, auxiliary: u32) {
    let source = Record(source as usize);
    let cache = Record(cache as usize);
    let area = Record(area as usize);
    let output = Record(auxiliary as usize);
    let reference = Record(0x016bfc30);
    unsafe {
        output.put(0, 1);
        output.put(4, 3);
        output.put(8, xm1_power_record_helper_757e38(request));
        if enabled == 1 || xm1_power_record_helper_fa7f40() == 1 {
            output.put(12, Record(0x016ad738).word(0));
            output.put_half(16, xm1_power_record_helper_f36760(request) as u32);
        } else {
            output.put(12, 0x80);
            output.put_half(16, 0);
        }
        output.put(0x30, 0);
        output.put(0x14, xm1_power_record_helper_f625ec(source.word(0x14)));
        let value = source.word(0x5c);
        let offset = source.half(0x6c) as i16 as i32;
        let width = xm1_power_record_helper_fa7954(request);
        let height = xm1_power_record_helper_fa7984(request);
        xm1_power_record_helper_f624a8((output.0 + 0x1c) as *mut u32, value, offset, width, height, source.word(0x14), source.byte(0x18), source.half(0x1a), 1);
        let kind = xm1_power_record_helper_74eb20(request);
        let width = xm1_power_record_helper_fa7954(request);
        let choice = xm1_power_record_helper_74fa2c(request);
        let resource = xm1_power_record_helper_74fc64(request);
        output.put(0x34, xm1_power_record_helper_fa66d4(kind, cache.byte(4), source.word(0x30), width, choice, source.word(0x88), resource, cache.byte(0x2e)));
        xm1_power_record_helper_f162e4(reference.0 as u32, request, 0, 1, 0);
        output.put(0x40, reference.word(0));
        output.put_byte(0x3c, 3);
        output.put_byte(0x3d, 3);
        output.put_byte(0x3e, 0);
        output.put_byte(0x3f, 0);
        for offset in [0x44, 0x46, 0x48, 0x4a, 0x4c] { output.put_half(offset, 0); }
        output.put(0x50, source.word(0x30));
        output.put(0x54, xm1_power_record_helper_fa7954(request));
        output.put_half(0x58, xm1_power_record_helper_fa7984(request));
        output.put(0x5c, source.word(0x14));
        output.put_byte(0x60, source.byte(0x18));
        output.put_half(0x62, source.half(0x1a));
        output.put(0x64, xm1_power_record_helper_74ea10(request));
        output.put(0x68, xm1_power_record_helper_74eb20(request));
        output.put(0x6c, source.word(0x7c));
        output.put(0x70, source.word(0x74));
        output.put(0x74, source.word(0xb4));
        let mut mode = source.word(0x0c);
        if source.word(0x88) != 0 && xm1_power_record_helper_f4ddbc(output.word(0x54), source.half(8)) == 0 && mode <= 1 { mode = 6; }
        output.put(0x78, mode);
        output.put(0x7c, source.word(0x80));
        output.put_half(0x80, source.half(0x26));
        output.put_half(0x82, xm1_power_record_helper_f5f9fc(request));
        output.put(0x84, source.word(0x2c));
        output.put_half(0x88, source.half(0x42));
        output.put(0x8c, source.word(0x44));
        output.put_half(0x90, source.half(0x48));
        output.put_half(0x92, source.half(0x6c));
        output.put_half(0x94, source.half(0x4a));
        output.put(0x98, source.word(0x5c));
        output.put(0x9c, if source.word(0x88) != 0 && output.word(0x54) == 0x30 { 1 } else { source.word(0x10) });
        output.put(0xa0, xm1_power_record_helper_74fa2c(request));
        output.put(0xa4, xm1_power_record_helper_74fc64(request));
        for (destination, origin) in [(0xa8, 0x64), (0xac, 0x88), (0xb0, 0xac), (0xb4, 0x78), (0xb8, 0x90)] { output.put(destination, source.word(origin)); }
        output.put(0xbc, xm1_power_record_helper_750ac0(request));
        output.put(0xc0, xm1_power_record_helper_750c00(request));
        output.put(0xc4, xm1_power_record_helper_750c88(request));
        output.put(0xc8, xm1_power_record_helper_754f8c(request));
        output.put(0xcc, xm1_power_record_helper_75502c(request));
        output.put(0xd0, xm1_power_record_helper_75569c(request));
        output.put(0xd4, xm1_power_record_helper_75516c(request));
        output.put(0xd8, xm1_power_record_helper_754d9c(request));
        output.put(0xdc, xm1_power_record_helper_74fb54(request));
        output.put(0xe0, xm1_power_record_helper_f51b94(request, 0xffff));
        output.put(0xe4, xm1_power_record_helper_7569a4(request));
        output.put(0xe8, source.word(0x8c));
        output.put(0xec, xm1_power_record_helper_758134(request));
        output.put(0xf0, source.word(0x68));
        output.put(0xf4, xm1_power_record_helper_faaae8(request));
        output.put(0xf8, source.word(0xa8));
        output.put(0xfc, xm1_power_record_helper_75a178(request));
        output.put(0x100, xm1_power_record_helper_757e38(request));
        output.put(0x118, (output.0 + 0x1c) as u32);
        output.put(0x11c, xm1_power_record_helper_74081c());
        output.put(0x120, 4);
        output.put_half(0x124, 0);
        output.put(0x128, 0);
        let mode = xm1_power_record_helper_74f564(request);
        area.put(0, if mode == 1 || mode == 2 { mode } else { 0 });
        area.put(4, source.word(0x30));
        area.put(8, output.word(0x54));
        area.put_half(12, output.half(0x58));
        area.put(0x10, source.word(0x7c));
        area.put(0x14, output.word(8));
        area.put(0x18, source.word(0x44));
        area.put(0x1c, output.word(0x9c));
        area.put(0x20, source.word(0x64));
        area.put(0x24, source.word(0x14));
        area.put(0x28, source.word(0xb4));
        let mut pair = [0; 2];
        xm1_power_record_helper_f5206c(pair.as_mut_ptr(), request);
        area.put(0x68, pair[0]);
        area.put(0x6c, pair[1]);
        area.put(0x98, xm1_power_record_helper_74fc64(request));
        area.put(0x9c, xm1_power_record_helper_74fa2c(request));
        area.put(0xa0, (output.word(0x28) == 1) as u32);
        area.put_half(0xa4, source.half(0x48));
        for offset in [0x48, 0x4c, 0x50, 0x54, 0x88, 0x8c, 0x90, 0x94] { area.put(offset, 0); }
        let margin = xm1_power_record_helper_757c60(request);
        let adjustment = xm1_power_record_helper_772f28(source.word(0x4c));
        area.put_half(0x5a, output.half(0x1e).wrapping_sub(adjustment).wrapping_sub(margin));
        area.put_half(0x58, output.half(0x1e));
        area.put_half(0x5c, xm1_power_record_helper_746c2c(output.word(8)));
        area.put(0x60, xm1_power_record_helper_f5ecb0(source.word(0x30), output.word(0x54), area.half(12)));
        area.put_half(0x64, xm1_power_record_helper_f5ecc0(output.word(0x54), output.half(0x58), output.word(0x78), output.word(0x9c)));
        let mapped = xm1_power_record_helper_746bf8(source.word(0x34), output.word(8));
        area.put_half(0x66, area.half(0x5a).wrapping_add(1600).wrapping_add(mapped));
        area.put(0x70, (xm1_power_record_helper_772ff8(source.half(0x24) as i16 as i32) == 2) as u32);
        let kind = xm1_power_record_helper_74eb20(request);
        let width = xm1_power_record_helper_fa7954(request);
        let choice = xm1_power_record_helper_74fa2c(request);
        let resource = xm1_power_record_helper_74fc64(request);
        area.put(0x74, xm1_power_record_helper_fa672c(kind, cache.byte(4), source.word(0x30), width, choice, source.word(0x88), resource, cache.byte(0x2e)));
        area.put(0x78, output.word(0x34));
        area.put(0x7c, (cache.0 + 4) as u32);
        area.put(0x80, (cache.0 + 0x430) as u32);
        area.put(0x84, reference.word(0));
        area.put_byte(0x44, reference.byte(0x216));
        area.put(0x38, reference.word(8));
        area.put(0x3c, reference.word(12));
        area.put(0x40, reference.word(16));
        let rate = if output.word(0x34) != 0 && xm1_power_record_helper_69f8fc(0x657d) != 0 {
            let divisor = xm1_power_record_helper_fa6934(output.word(0x54), area.word(0x98), output.word(8), cache.half(0x2a), 0, 0);
            if divisor == 0 { 1 } else { 10000 / divisor }
        } else { 1 };
        area.put(0x34, rate);
        if xm1_power_record_helper_74f344(request) == 1 {
            xm1_power_record_helper_740888(0, (area.0 + 0x30) as *mut u32, 0, 0);
        } else { area.put(0x30, 0); }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f36e74(request: u32, source: u32, cache: u32, area: u32, auxiliary: u32) {
    unsafe { xm1_power_record_helper_f367a4(request, 0, source, cache, area, auxiliary); }
}
