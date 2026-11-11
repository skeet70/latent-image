use core::ptr::{read_volatile, write_volatile};
unsafe fn word(address: usize) -> u32 { unsafe { read_volatile(address as *const u32) } }
unsafe fn half(address: usize) -> u16 { unsafe { read_volatile(address as *const u16) } }
unsafe fn byte(address: usize) -> u8 { unsafe { read_volatile(address as *const u8) } }
unsafe fn put(address: usize, value: u32) { unsafe { write_volatile(address as *mut u32, value) } }
unsafe fn put_half(address: usize, value: u16) { unsafe { write_volatile(address as *mut u16, value) } }
unsafe extern "C" {
    fn xm1_power_record_helper_f336cc(source: u32);
    fn xm1_power_record_helper_756ac4(request: u32) -> u32;
    fn xm1_power_record_helper_757e38(request: u32) -> u32;
    fn xm1_power_record_helper_746c2c(value: u32) -> u32;
    fn xm1_power_record_helper_fa7edc(value: u32) -> u32;
    fn xm1_power_record_helper_772f28(value: u32) -> u32;
    fn xm1_power_record_helper_103af30(value: i32);
    fn xm1_power_record_helper_f4dd50(value: u32);
    fn xm1_power_record_helper_69f8fc(key: u32) -> u32;
    fn xm1_power_record_helper_69fec8(key: u32) -> u32;
    fn xm1_power_record_helper_750474(request: u32) -> u32;
    fn xm1_power_record_helper_7582c0(output: *mut u32, request: u32);
    fn xm1_power_record_helper_758300(request: u32, input: *const u32);
    fn xm1_power_record_helper_fa95b8(request: u32, source: u32, mode: u32, first: *mut u32, second: *mut u32);
    fn xm1_power_record_helper_fa89b8(request: u32);
    fn xm1_power_record_helper_fa8520(request: u32, mode: u32);
    fn xm1_power_record_helper_fa91b0(source: u32, mode: u32);
    fn xm1_power_record_helper_f521d8(first: u32, second: u32, third: u32);
    fn xm1_power_record_helper_f332d0(request: u32, record: u32, mode: u32) -> u32;
    fn xm1_power_record_helper_756734(request: u32, output: *mut u32);
    fn xm1_power_record_helper_756754(request: u32, input: *const u32);
    fn xm1_power_record_helper_76d9c8() -> u32;
    fn xm1_power_record_helper_740e74(address: u32, length: u32);
    fn xm1_power_record_helper_74fc64(request: u32) -> u32;
    fn xm1_power_record_helper_fa7ef4() -> u32;
    fn xm1_power_record_helper_f4f16c(request: u32, base: u32) -> u32;
    fn xm1_power_record_helper_74fab4(output: *mut u32, request: u32);
    fn xm1_power_record_helper_74fa2c(request: u32) -> u32;
    fn xm1_power_record_helper_7527f8(request: u32) -> u32;
    fn xm1_power_record_helper_752890(request: u32) -> u32;
    fn xm1_power_record_helper_fea5f0(input: *const u32);
    fn xm1_power_record_helper_75a4ec(request: u32, output: *mut u32);
    fn xm1_power_record_helper_f335b0(source: u32);
    fn xm1_power_record_helper_f1fbe8(source: u32);
    fn xm1_power_record_helper_f253a0(request: u32, enabled: u32, first: u32, base: u32, previous: u32, previous_base: u32) -> i32;
    fn xm1_power_record_helper_f1d134(source: u32);
    fn xm1_power_record_helper_75a4ac(request: u32, index: u32, output: *mut u32);
    fn xm1_power_record_helper_74714c(first: u32, second: u32, base: u32) -> u32;
    fn xm1_power_record_helper_f1d5d4(base: u32, value: u32, flag: u32, second_flag: u32, third_flag: u32, extra: u32, mapped: u32, offset: i32);
    fn xm1_power_record_helper_75a414(request: u32, value: i32);
    fn xm1_power_record_helper_75a324(request: u32, value: i32);
    fn xm1_power_record_helper_75a374(request: u32, value: i32);
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f3515c() {
    unsafe {
        let source = word(0x0169d384) as usize;
        if source == 0 { return; }
        xm1_power_record_helper_f336cc(source as u32);
        let mode = xm1_power_record_helper_756ac4(word(0x016bfe48));
        let base = xm1_power_record_helper_757e38(word(0x016bfe48));
        let mapped = xm1_power_record_helper_746c2c(base);
        let adjustment = xm1_power_record_helper_fa7edc(mode);
        let offset = xm1_power_record_helper_772f28(adjustment);
        xm1_power_record_helper_103af30((half(source) as u32).wrapping_sub(half(source + 0x38) as u32).wrapping_add(mapped).wrapping_sub(offset) as i16 as i32);
        if word(source + 0x44) == 1 {
            for offset in [0, 4, 8, 12, 16] { put(0x016bf978 + offset, word(source + 0x18 + offset)); }
            put_half(0x016bf98c, half(source + 0x2c));
            put_half(0x016bf98e, half(source + 0x30).wrapping_add(half(source + 0x2e)));
            for (destination, origin) in [(0x18, 0x32), (0x1a, 0x34), (0x1c, 0x36), (0x1e, 0x38)] { put_half(0x016bf978 + destination, half(source + origin)); }
            put(0x016bf998, word(source + 0x3c));
        }
        xm1_power_record_helper_f4dd50(word(source + 0x18));
        let request = word(0x016bfe48);
        if xm1_power_record_helper_69f8fc(0x5256) & 3 != 0 && xm1_power_record_helper_750474(request) == 1 && word(source + 0x44) == 1 {
            let mut exposure = [0u32; 10];
            xm1_power_record_helper_7582c0(exposure.as_mut_ptr(), request);
            exposure[9] = 0;
            xm1_power_record_helper_758300(request, exposure.as_ptr());
            if xm1_power_record_helper_69f8fc(0x5256) & 2 != 0 {
                xm1_power_record_helper_fa95b8(request, source as u32, exposure[9], 0x016bf970 as *mut u32, 0x016bf974 as *mut u32);
                xm1_power_record_helper_fa89b8(request);
            }
            xm1_power_record_helper_fa8520(request, exposure[9]);
            if xm1_power_record_helper_69f8fc(0xa8) & 0x80 != 0 { xm1_power_record_helper_fa91b0(source as u32, exposure[9]); }
            if xm1_power_record_helper_69f8fc(0x5258) & 1 != 0 { xm1_power_record_helper_f521d8(exposure[5] & 0xffff, exposure[4], exposure[9]); }
        }
        put(0x41717f6c, xm1_power_record_helper_f332d0(word(0x016bfe48), 0x016bf374, 0));
        let mut profile = [0u32; 33];
        xm1_power_record_helper_756734(word(0x016bfe48), profile.as_mut_ptr());
        profile[1] = 1;
        profile[26] = word(0x41717f6c);
        profile[10] = if word(0x016bf99c) != 0 { (word(0x41717f74) == xm1_power_record_helper_76d9c8()) as u32 } else { 0 };
        if xm1_power_record_helper_69fec8(0xe8) & 8 != 0 { profile[26] = 0; }
        xm1_power_record_helper_756754(word(0x016bfe48), profile.as_ptr());
        xm1_power_record_helper_740e74(source as u32, 0x50);
        let request = word(0x016bfe48);
        let base = xm1_power_record_helper_757e38(request);
        let resource = xm1_power_record_helper_74fc64(request);
        let mode = xm1_power_record_helper_fa7ef4();
        let current = xm1_power_record_helper_f4f16c(request, base);
        let fallback = xm1_power_record_helper_f4f16c(request, 0xffff);
        let mut pair = [0; 2];
        xm1_power_record_helper_74fab4(pair.as_mut_ptr(), request);
        let choice = xm1_power_record_helper_74fa2c(request);
        let values = [resource, mode, current, fallback, pair[0], pair[1], choice];
        if values.iter().enumerate().any(|(index, value)| *value != word(0x016be058 + index * 4)) {
            let first = xm1_power_record_helper_7527f8(word(0x016bfe48));
            let second = xm1_power_record_helper_752890(word(0x016bfe48));
            let packet = [resource, mode, current, first, second, fallback, first, second, pair[0], pair[1], choice];
            xm1_power_record_helper_fea5f0(packet.as_ptr());
        }
        for (index, value) in [resource, mode, current, mode, pair[0], pair[1], choice].iter().enumerate() { put(0x016be058 + index * 4, *value); }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f3557c() {
    unsafe {
        let source = word(0x0169d384) as usize;
        if source == 0 { return; }
        let mut saved = [0u32; 11];
        xm1_power_record_helper_75a4ec(word(0x016bfe48), saved.as_mut_ptr());
        xm1_power_record_helper_f335b0(source as u32);
        put(0x016bf99c, word(source + 0xcc));
        let reference = word(source + 0xc4) as usize;
        if word(source + 0xd0) != 0 || byte(0x016bfb84) == 1 {
            xm1_power_record_helper_f1fbe8(reference as u32);
            if byte(0x016bfb84) == 0 || xm1_power_record_helper_f253a0(word(0x016bfe48), byte(saved.as_ptr() as usize + 41) as u32, word(source + 0x88), word(source + 0xfc), saved[1], saved[0]) == 1 {
                xm1_power_record_helper_f1d134(word(source + 0xc8));
            } else {
                let mut pair = [0; 2];
                xm1_power_record_helper_75a4ac(word(0x016bfe48), 2, pair.as_mut_ptr());
                let mapped = xm1_power_record_helper_74714c(pair[0], pair[1], word(source + 0xfc));
                xm1_power_record_helper_f1d5d4(word(source + 0xfc), word(source + 0x108), byte(source + 0x10c) as u32, byte(source + 0x10d) as u32, byte(source + 0x110) as u32, word(source + 0x104), mapped, half(source + 0x10e) as i16 as i32);
            }
            xm1_power_record_helper_75a414(word(0x016bfe48), half(source + 0x9e) as i16 as i32);
            xm1_power_record_helper_75a324(word(0x016bfe48), half(source + 0x84) as i16 as i32);
            xm1_power_record_helper_75a374(word(0x016bfe48), half(source + 0x86) as i16 as i32);
        }
        let mut profile = [0u32; 33];
        xm1_power_record_helper_756734(word(0x016bfe48), profile.as_mut_ptr());
        profile[24] = 1;
        profile[27] = word(0x4171813c);
        profile[28] = word(0x41718074);
        put_half(profile.as_mut_ptr() as usize + 116, half(0x41718078));
        profile[30] = word(0x016bf374);
        put_half(profile.as_mut_ptr() as usize + 124, if word(source + 0xd0) != 0 { half(reference + 24).wrapping_sub(half(reference + 4)) } else { 0 });
        profile[32] = word(0x4171802c);
        xm1_power_record_helper_756754(word(0x016bfe48), profile.as_ptr());
        xm1_power_record_helper_740e74(source as u32, 0x114);
        if word(source + 0xd0) != 0 {
            xm1_power_record_helper_740e74(reference as u32, 0x7c);
            xm1_power_record_helper_740e74(word(source + 0xc8), 0x63ec);
        }
    }
}
