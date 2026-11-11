use core::ptr::{read_volatile, write_volatile};

const SNAPSHOT: usize = 0x016b5f44;

unsafe extern "C" {
    fn xm1_power_record_helper_74e648(request: u32) -> u32;
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
    fn xm1_power_record_helper_772ff8(value: i32) -> u32;
    fn xm1_power_record_helper_74eb20(request: u32) -> u32;
    fn xm1_power_record_helper_7500bc(request: u32) -> u32;
    fn xm1_power_record_helper_7503ec(request: u32) -> u32;
    fn xm1_power_record_helper_74f1f0(request: u32) -> u32;
    fn xm1_power_record_helper_7509c4(request: u32) -> u32;
    fn xm1_power_record_helper_f5fc00(request: u32) -> u32;
    fn xm1_power_record_helper_74f124(request: u32) -> u32;
    fn xm1_power_record_helper_75060c(request: u32) -> u32;
    fn xm1_power_record_helper_74eba8(request: u32) -> u32;
    fn xm1_power_record_helper_74f564(request: u32) -> u32;
    fn xm1_power_record_helper_faaae8(request: u32);
    fn xm1_power_record_helper_ef74e4() -> u32;
    fn xm1_power_record_helper_75a178(request: u32) -> u32;
    fn xm1_power_record_helper_758c98(output: *mut u32, request: u32);
    fn xm1_power_record_helper_758cc4(request: u32, value: *const u32);
    fn xm1_power_record_helper_69f8fc(key: u32) -> u32;
    fn xm1_power_record_helper_750474(request: u32) -> u32;
    fn xm1_power_record_helper_f7e364(kind: u32, selection: u32, value: i32) -> u32;
    fn xm1_power_record_helper_75502c(request: u32) -> u32;
    fn xm1_power_record_helper_75516c(request: u32) -> u32;
    fn xm1_power_record_helper_feaadc(channel: u32, output: *mut u32);
    fn xm1_power_record_helper_75a4ac(request: u32, index: u32, output: *mut u32);
    fn xm1_power_record_helper_757e58(request: u32, value: u32);
    fn xm1_power_record_helper_757e38(request: u32) -> u32;
    fn xm1_power_record_helper_757df0(request: u32) -> u32;
    fn xm1_power_record_helper_757c60(request: u32) -> u32;
    fn xm1_power_record_helper_ef5310(value: u32) -> u32;
    fn xm1_power_record_helper_75738c(request: u32) -> u32;
    fn xm1_power_record_helper_75729c(request: u32) -> u32;
    fn xm1_power_record_helper_f36760(request: u32) -> u32;
    fn xm1_power_record_helper_74714c(first: u32, second: u32, base: u32) -> u32;
    fn xm1_power_record_helper_756ac4(request: u32) -> u32;
    fn xm1_power_record_helper_fab250(kind: u32, selection: u32, mode: u32, value: u32) -> u32;
    fn xm1_power_record_helper_fa7aa4(kind: u32, mode: u32, mapped: u32, detail: u32, profile: *mut u32);
    fn xm1_power_record_helper_75817c(request: u32) -> u32;
    fn xm1_power_record_helper_756e24(output: *mut u32, request: u32);
    fn xm1_power_record_helper_750034(request: u32) -> u32;
    fn xm1_power_record_helper_f5ecc0(kind: u32, index: u32, mode: u32, detail: u32) -> u32;
    fn xm1_power_record_helper_746c2c(value: u32) -> u32;
    fn xm1_power_record_helper_7580ec(request: u32) -> u32;
    fn xm1_power_record_helper_754d9c(request: u32) -> u32;
    fn xm1_power_record_helper_74fa2c(request: u32) -> u32;
    fn xm1_power_record_helper_75082c(request: u32) -> u32;
    fn xm1_power_record_helper_fa9588() -> u32;
    fn xm1_power_record_helper_f77c8c(parameters: *const u32, output: *mut u32);
    fn xm1_power_record_helper_7407b0();
    fn xm1_power_record_helper_f4f16c(request: u32, base: u32) -> u32;
    fn xm1_power_record_helper_7527f8(request: u32) -> u32;
    fn xm1_power_record_helper_752890(request: u32) -> u32;
    fn xm1_power_record_helper_74ec30(request: u32) -> u32;
    fn xm1_power_record_helper_fa7bb8(parameters: *const u32, output: *mut u32);
    fn xm1_power_record_helper_f367a4(request: u32, enabled: u32, profile: u32, cache: u32, area: u32, auxiliary: u32);
    fn xm1_power_record_helper_f1fc5c(auxiliary: u32, base: u32);
    fn xm1_power_record_helper_758250(request: u32, value: u32);
    fn xm1_power_record_helper_f17abc(destination: u32);
    fn xm1_power_record_helper_72872c(event: u32);
    fn xm1_power_record_helper_f4c620(callback: unsafe extern "C" fn(u32, u32), profile: *const u32);
    fn xm1_power_record_helper_ef9a94(event: u32, payload: *const u32, length: u32);
    fn xm1_power_record_helper_fa7edc(value: u32) -> u32;
    fn xm1_power_record_helper_ef7820(value: u32) -> u32;
    fn xm1_power_record_helper_758208(request: u32, value: u32);
    fn xm1_power_record_helper_772f28(value: u32) -> u32;
    fn xm1_power_record_helper_75741c(request: u32, output: *mut u32);
    fn xm1_power_record_helper_feab3c(profile: *const u32);
    fn xm1_power_record_helper_740e74(address: u32, length: u32);
}

unsafe fn word(address: usize) -> u32 { unsafe { read_volatile(address as *const u32) } }
unsafe fn half(address: usize) -> u16 { unsafe { read_volatile(address as *const u16) } }
unsafe fn byte(address: usize) -> u8 { unsafe { read_volatile(address as *const u8) } }
unsafe fn put(address: usize, value: u32) { unsafe { write_volatile(address as *mut u32, value) } }
unsafe fn put_half(address: usize, value: u32) { unsafe { write_volatile(address as *mut u16, value as u16) } }
unsafe fn put_byte(address: usize, value: u32) { unsafe { write_volatile(address as *mut u8, value as u8) } }

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f241d4(first: u32, second: u32) {
    unsafe {
        let payload = [first, second];
        if xm1_power_record_helper_69f8fc(0x24e) == 1 { xm1_power_record_helper_72872c(0x11102000); }
        xm1_power_record_helper_ef9a94(0x9c, payload.as_ptr(), 8);
    }
}

#[cfg(target_arch = "arm")]
core::arch::global_asm!(
    ".arm",
    ".global xm1_power_record_helper_f24bd0",
    ".type xm1_power_record_helper_f24bd0,%function",
    "xm1_power_record_helper_f24bd0:",
    "push {r4,r5,r6,r7,r8,r10,r11,lr}",
    "add r11,sp,#28",
    "sub sp,sp,#104",
    "mov r1,r11",
    "bl xm1_power_record_snapshot_frame",
    "sub sp,r11,#28",
    "pop {r4,r5,r6,r7,r8,r10,r11,pc}",
    options(raw),
);

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_snapshot_frame(request: u32, frame: usize) {
    unsafe {
        put(SNAPSHOT + 4, xm1_power_record_helper_74e648(request));
        put_half(SNAPSHOT + 8, xm1_power_record_helper_74e718(request));
        put(SNAPSHOT + 12, xm1_power_record_helper_74f1ac(request));
        put(SNAPSHOT + 16, xm1_power_record_helper_74f9a4(request));
        put(SNAPSHOT + 20, xm1_power_record_helper_74e7f0(request));
        put_byte(SNAPSHOT + 24, xm1_power_record_helper_74e878(request));
        put_half(SNAPSHOT + 26, xm1_power_record_helper_74e900(request));
        put(SNAPSHOT + 44, xm1_power_record_helper_74f674(request));
        put(SNAPSHOT + 48, xm1_power_record_helper_74e5c0(request));
        put_half(SNAPSHOT + 38, xm1_power_record_helper_74f5ec(request));
        put_half(SNAPSHOT + 42, xm1_power_record_helper_f5f9fc(request));
        put_half(SNAPSHOT + 66, xm1_power_record_helper_74f6fc(request));
        put(SNAPSHOT + 68, xm1_power_record_helper_74f784(request));
        put_half(SNAPSHOT + 72, xm1_power_record_helper_74f80c(request));
        put_half(SNAPSHOT + 74, xm1_power_record_helper_74f91c(request));
        let reference = xm1_power_record_helper_fa6bb4(request);
        put_half(SNAPSHOT + 36, reference);
        put(SNAPSHOT + 32, xm1_power_record_helper_772ff8(reference as i32));
        let source = xm1_power_record_helper_74eb20(request);
        put(SNAPSHOT + 100, xm1_power_record_helper_7500bc(request));
        put(SNAPSHOT + 136, xm1_power_record_helper_7503ec(request));
        put(SNAPSHOT + 180, xm1_power_record_helper_74f1f0(request));
        put(SNAPSHOT + 144, xm1_power_record_helper_7509c4(request));
        put_half(SNAPSHOT + 108, xm1_power_record_helper_f5fc00(request));
        put(SNAPSHOT + 116, xm1_power_record_helper_74f124(request));
        put(SNAPSHOT + 120, xm1_power_record_helper_75060c(request));
        put(SNAPSHOT + 124, xm1_power_record_helper_74eba8(request));
        put(SNAPSHOT + 128, xm1_power_record_helper_74f564(request));
        xm1_power_record_helper_faaae8(request);
        put_half(SNAPSHOT + 88, xm1_power_record_helper_ef74e4());
        put(SNAPSHOT + 212, xm1_power_record_helper_75a178(request));
        let mut active = [0; 2];
        xm1_power_record_helper_758c98(active.as_mut_ptr(), request);
        put(SNAPSHOT + 168, active[0]);
        if active[0] == 0 {
            active[1] = 0;
            xm1_power_record_helper_758cc4(request, active.as_ptr());
        }
        let available = xm1_power_record_helper_69f8fc(0x5256) & 3 != 0 && xm1_power_record_helper_750474(request) != 0;
        put(SNAPSHOT + 172, available as u32);
        let group = xm1_power_record_helper_f7e364(word(SNAPSHOT + 48), word(SNAPSHOT + 4), half(SNAPSHOT + 66) as i16 as i32);
        put(SNAPSHOT + 212, group);
        xm1_power_record_helper_75502c(request);
        xm1_power_record_helper_75516c(request);
        xm1_power_record_helper_feaadc(0, 0x016b65ec as *mut u32);
        let mut previous = [0; 2];
        xm1_power_record_helper_75a4ac(request, 3, previous.as_mut_ptr());
        xm1_power_record_helper_757e58(request, previous[0]);
        put_half(SNAPSHOT + 56, half(0x016b65d8) as u32);
        put(SNAPSHOT + 28, xm1_power_record_helper_757e38(request));
        put(SNAPSHOT + 52, word(0x016b6608));
        put(SNAPSHOT + 76, xm1_power_record_helper_757df0(request));
        put_half(SNAPSHOT + 84, xm1_power_record_helper_757c60(request));
        put_half(SNAPSHOT + 86, xm1_power_record_helper_ef5310(word(0x016b6604)));
        put_byte(SNAPSHOT + 221, xm1_power_record_helper_75738c(request));
        put_half(SNAPSHOT + 216, xm1_power_record_helper_75729c(request));
        put_half(SNAPSHOT + 218, xm1_power_record_helper_f36760(request));
        let mut selected = [0; 2];
        xm1_power_record_helper_75a4ac(request, 2, selected.as_mut_ptr());
        put(SNAPSHOT + 224, xm1_power_record_helper_74714c(selected[0], selected[1], previous[0]));
        let mode = xm1_power_record_helper_756ac4(request);
        put(SNAPSHOT + 104, mode);
        let mapped = xm1_power_record_helper_fab250(word(SNAPSHOT + 48), word(SNAPSHOT + 4), mode, source);
        put(SNAPSHOT + 96, mapped);
        xm1_power_record_helper_fa7aa4(word(SNAPSHOT + 48), mode, mapped, word(SNAPSHOT + 180), SNAPSHOT as *mut u32);
        put(SNAPSHOT + 164, xm1_power_record_helper_75817c(request));
        let mut geometry = [0; 5];
        xm1_power_record_helper_756e24(geometry.as_mut_ptr(), request);
        let geometry = geometry.as_ptr() as usize;
        put(SNAPSHOT + 152, half(geometry + 2) as u32);
        put(SNAPSHOT + 156, half(geometry) as u32);
        put(SNAPSHOT + 160, half(geometry + 4) as u32);
        let value = xm1_power_record_helper_f5fc00(request);
        put(SNAPSHOT + 80, if value <= 1 { 1 - value } else { 0 });
        put(SNAPSHOT + 92, xm1_power_record_helper_750034(request));
        put_half(SNAPSHOT + 132, xm1_power_record_helper_f5ecc0(word(SNAPSHOT + 4), half(SNAPSHOT + 8) as u32, word(SNAPSHOT + 12), word(SNAPSHOT + 16)));
        let base = xm1_power_record_helper_757e38(request);
        put_half(SNAPSHOT + 134, xm1_power_record_helper_746c2c(base));
        put(SNAPSHOT + 140, xm1_power_record_helper_7580ec(request));
        put(SNAPSHOT + 208, xm1_power_record_helper_754d9c(request));
        let special = xm1_power_record_helper_74fa2c(request) == 6 && xm1_power_record_helper_75082c(request) == 0;
        put(SNAPSHOT + 148, if special { xm1_power_record_helper_fa9588() } else { 0 });
        let parameters = frame - 116;
        for (offset, value) in [(0, word(SNAPSHOT + 48)), (4, mode), (8, word(SNAPSHOT + 140)), (12, word(SNAPSHOT + 4)), (16, word(SNAPSHOT + 168)), (20, word(SNAPSHOT + 212)), (24, word(SNAPSHOT + 208)), (28, 0), (32, 0)] { put(parameters + offset, value); }
        put_byte(parameters + 38, 1);
        put_byte(parameters + 39, 0);
        xm1_power_record_helper_f77c8c(parameters as *const u32, (SNAPSHOT + 176) as *mut u32);
        put(SNAPSHOT + 200, 0);
        put(SNAPSHOT, word(0x016b602c));
        if word(SNAPSHOT + 48) == 0 { xm1_power_record_helper_7407b0(); }
        put(0x016b6508, xm1_power_record_helper_f4f16c(request, word(SNAPSHOT + 28)));
        put(0x016b650c, xm1_power_record_helper_7527f8(request));
        put(0x016b6510, xm1_power_record_helper_752890(request));
        put(0x016b6514, xm1_power_record_helper_74ec30(request));
        put(0x016b6504, word(0x016b6604));
        xm1_power_record_helper_fa7bb8(0x016b6504 as *const u32, 0x016b6030 as *mut u32);
        put(SNAPSHOT + 184, 1);
        xm1_power_record_helper_f367a4(request, 1, SNAPSHOT as u32, 0x016b6030, 0x41717b80, 0x41717c28);
        put(SNAPSHOT + 188, 0x41717b80);
        let base = xm1_power_record_helper_757e38(request);
        xm1_power_record_helper_f1fc5c(0x41717c28, base);
        put_half(0x41717cb0, half(SNAPSHOT + 66) as u32);
        put(0x41717cb4, word(SNAPSHOT + 68));
        put_half(0x41717ca8, half(SNAPSHOT + 38) as u32);
        put_half(0x41717c38, xm1_power_record_helper_f36760(request));
        put(SNAPSHOT + 192, 0x41717c28);
        xm1_power_record_helper_758250(request, 0);
        xm1_power_record_helper_f17abc(0x41717d54);
        put(SNAPSHOT + 196, 0x41717d54);
        put_byte(SNAPSHOT + 220, 1);
        put(0x016b664c, word(SNAPSHOT + 68));
        put_half(0x016b6650, half(SNAPSHOT + 66) as u32);
        put_half(0x016b6652, half(SNAPSHOT + 38) as u32);
        if xm1_power_record_helper_69f8fc(0x24e) == 1 { xm1_power_record_helper_72872c(0x01102000); }
        xm1_power_record_helper_f4c620(xm1_power_record_helper_f241d4, SNAPSHOT as *const u32);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f2481c(request: u32) -> i32 {
    unsafe {
        let source = word(0x0169d384) as usize;
        if source == 0 { return 1; }
        xm1_power_record_helper_feaadc(0, 0x016b651c as *mut u32);
        let flags = byte(0x016b6578);
        let halfword = half(0x016b656a);
        let kind = xm1_power_record_helper_74e5c0(request);
        let reference = word(0x016b6534);
        let primary = word(source + 0xdc);
        let mode = xm1_power_record_helper_fa7edc(primary);
        let secondary = word(source + 0xe0);
        let context = word(source + 0xbc);
        let auxiliary = xm1_power_record_helper_ef7820(context);
        xm1_power_record_helper_75502c(request);
        xm1_power_record_helper_75516c(request);
        let parameters = [kind, primary, context, xm1_power_record_helper_74e648(request), secondary, word(source + 0xe8), xm1_power_record_helper_754d9c(request), 0, 0, 0x00010000];
        let mut result = 0;
        xm1_power_record_helper_f77c8c(parameters.as_ptr(), &mut result);
        xm1_power_record_helper_758208(request, result as u8 as u32);
        for offset in [0, 0x24] { put(0x016b6588 + offset, reference); }
        for offset in [4, 0x28] { put_half(0x016b6588 + offset, halfword as u32); }
        for offset in [6, 0x2a] { put_byte(0x016b6588 + offset, flags as u32); }
        put(0x016b6594, word(source + 0x58));
        put_half(0x016b6598, half(source + 0x62) as u32);
        let delta = xm1_power_record_helper_772f28(mode);
        put_half(0x016b659a, (half(source + 0x6a) as u32).wrapping_add(delta));
        put_half(0x016b659c, half(source + 0x72) as u32);
        put_half(0x016b65a4, half(source + 0x9e) as u32);
        put(0x016b6590, mode);
        put(0x016b65a8, auxiliary);
        put(0x016b65b8, word(source + 0x34));
        put_half(0x016b65bc, half(source + 0x3e) as u32);
        let delta = xm1_power_record_helper_772f28(mode);
        put_half(0x016b65be, (half(source + 0x46) as u32).wrapping_add(delta));
        put_half(0x016b65c0, half(source + 0x4e) as u32);
        put_half(0x016b65c8, half(source + 0x9e) as u32);
        put(0x016b65b4, mode);
        put(0x016b65cc, auxiliary);
        let mut focus = 0;
        xm1_power_record_helper_75741c(request, &mut focus);
        put(0x016b65d0, 0);
        xm1_power_record_helper_feab3c(0x016b6588 as *const u32);
        xm1_power_record_helper_740e74(source as u32, 0x114);
        1
    }
}

unsafe extern "C" {
    fn xm1_power_record_helper_feaacc(channel: u32);
    fn xm1_power_record_helper_103b064();
    fn xm1_power_record_helper_fa7954(request: u32) -> u32;
    fn xm1_power_record_helper_f5206c(output: *mut u32, request: u32);
    fn xm1_power_record_helper_746bf8(value: u32, base: u32) -> u32;
    fn xm1_power_record_helper_fa7a7c(kind: u32, mode: u32, zero: u32) -> u32;
    fn xm1_power_record_helper_fa7e20(value: i32, kind: u32, mode: u32) -> u32;
    fn xm1_power_record_helper_f4c584(callback: unsafe extern "C" fn(u32, u32), parameters: *const u32, workspace: *mut u32);
    fn xm1_power_record_helper_75a3c4(request: u32, value: i32);
    fn xm1_power_record_helper_7582c0(output: *mut u32, request: u32);
    fn xm1_power_record_helper_758300(request: u32, value: *const u32);
    fn xm1_power_record_helper_fa95b8(request: u32, reference: u32, enabled: u32, first: *mut u32, second: *mut u32);
    fn xm1_power_record_helper_fa8520(request: u32, enabled: u32);
    fn xm1_power_record_helper_fa91b0(reference: u32, enabled: u32);
    fn xm1_power_record_helper_f521d8(value: u32, mode: u32, enabled: u32);
    fn xm1_power_record_helper_faa7b0(request: u32, record: *const u32, output: *mut u32, value: *mut u16);
    fn xm1_power_record_helper_f1c8fc(request: u32, record: *const u32, first: *mut i16, second: *mut i16, output: *mut u32, a: *mut u16, b: *mut u16, c: *mut u16, d: *mut u16);
    fn xm1_power_record_helper_756d04(request: u32) -> u32;
    fn xm1_power_record_helper_756734(request: u32, output: *mut u32);
    fn xm1_power_record_helper_756754(request: u32, value: *const u32);
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f241a8(first: u32, second: u32) {
    unsafe {
        let payload = [first, second];
        xm1_power_record_helper_ef9a94(0x9b, payload.as_ptr(), 8);
    }
}

#[cfg(target_arch = "arm")]
core::arch::global_asm!(
    ".arm",
    ".global xm1_power_record_helper_f242ec",
    ".type xm1_power_record_helper_f242ec,%function",
    "xm1_power_record_helper_f242ec:",
    "push {r4,r5,r11,lr}",
    "add r11,sp,#12",
    "sub sp,sp,#1520",
    "sub sp,sp,#8",
    "mov r1,r11",
    "bl xm1_power_record_profile_frame",
    "sub sp,r11,#12",
    "pop {r4,r5,r11,pc}",
    options(raw),
);

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_profile_frame(request: u32, frame: usize) {
    unsafe {
        xm1_power_record_helper_feaacc(0);
        if xm1_power_record_helper_74f1f0(request) == 6 { xm1_power_record_helper_103b064(); }
        let source = frame - 108;
        xm1_power_record_helper_feaadc(0, source as *mut u32);
        let packet = frame - 276;
        put(packet, xm1_power_record_helper_74e648(request));
        put(packet + 8, xm1_power_record_helper_fa7954(request));
        put(packet + 32, xm1_power_record_helper_74f564(request));
        put_half(packet + 28, xm1_power_record_helper_74f5ec(request));
        let dimensions = xm1_power_record_helper_74ec30(request);
        put_half(packet + 0x6e, dimensions);
        put_half(packet + 0x70, dimensions >> 16);
        put(packet + 20, xm1_power_record_helper_74e5c0(request));
        let temporary = frame - 1540;
        xm1_power_record_helper_756e24(temporary as *mut u32, request);
        for offset in (0..20).step_by(4) { put(packet + 0x88 + offset, word(temporary + offset)); }
        put(packet + 0x9c, xm1_power_record_helper_74fa2c(request));
        xm1_power_record_helper_f5206c(temporary as *mut u32, request);
        put(packet + 0xa0, word(temporary));
        put(packet + 0xa4, word(temporary + 4));
        put(packet + 16, (word(source + 88) & 5 == 5) as u32);
        put(packet + 0x28, word(source + 28));
        put_half(packet + 0x2c, half(source + 32) as u32);
        let correction = xm1_power_record_helper_772f28(3);
        let reference = xm1_power_record_helper_772ff8(correction as i32);
        put(packet + 0x74, reference);
        let delta = xm1_power_record_helper_772f28(reference);
        put_half(packet + 0x2e, (half(source + 34) as u32).wrapping_sub(delta));
        put_half(packet + 0x86, half(source + 36) as u32);
        xm1_power_record_helper_faaae8(request);
        put_half(packet + 0x84, xm1_power_record_helper_ef74e4());
        let mapped = xm1_power_record_helper_746bf8(word(packet + 0x28), word(source + 40));
        let adjusted = (half(packet + 0x2c) as u32).wrapping_sub(half(packet + 0x2e) as u32).wrapping_sub(half(packet + 0x86) as u32).wrapping_sub(half(packet + 0x84) as u32).wrapping_add(mapped);
        put_half(packet + 0x24, adjusted);
        put(packet + 0x7c, 0);
        put(packet + 0x30, word(source + 24));
        put_half(packet + 0x34, half(source + 38) as u32);
        put_half(packet + 0x80, xm1_power_record_helper_757c60(request));
        for (destination, from) in [(0x38, 8), (0x3c, 12), (0x40, 0), (0x44, 4), (0x48, 16), (0x4c, 20)] { put(packet + destination, word(source + from)); }
        for (offset, value) in [(0x50, 100), (0x52, 100), (0x54, 256), (0x56, 100), (0x58, 100), (0x5a, 1)] { put_half(packet + offset, value); }
        put(packet + 0x5c, 0x5ffec1b4);
        put(packet + 0x60, 0x5ffed1b4);
        put(packet + 0x64, 0x1000);
        put(packet + 0x68, 16);
        let mode = xm1_power_record_helper_756ac4(request);
        put(packet + 24, mode);
        put_byte(packet + 0x6c, xm1_power_record_helper_fa7a7c(word(packet + 20), mode, 0));
        put(packet + 0x78, xm1_power_record_helper_fa7e20(half(packet + 0x2c) as i16 as i32, word(packet + 20), mode));
        put_half(packet + 0x82, xm1_power_record_helper_ef5310(word(packet + 0x30)));
        put(packet + 12, word(0x016b6028));
        let workspace = frame - 1512;
        put(workspace, 0);
        put(workspace + 4, 0);
        xm1_power_record_helper_f4c584(xm1_power_record_helper_f241a8, packet as *const u32, workspace as *mut u32);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f2457c(request: u32) {
    unsafe {
        put(0x016b6028, 0);
        let source = word(0x0169d384) as usize;
        if source == 0 { return; }
        xm1_power_record_helper_75a3c4(request, half(source) as i16 as i32);
        if xm1_power_record_helper_69f8fc(0x5257) & 3 != 0 && xm1_power_record_helper_750474(request) == 1 {
            let mut record = [0; 10];
            xm1_power_record_helper_7582c0(record.as_mut_ptr(), request);
            record[9] = 1;
            xm1_power_record_helper_758300(request, record.as_ptr());
            if xm1_power_record_helper_69f8fc(0x5257) & 2 != 0 {
                xm1_power_record_helper_fa95b8(request, source as u32, record[9], 0x016b657c as *mut u32, 0x016b6580 as *mut u32);
            }
            xm1_power_record_helper_fa8520(request, record[9]);
            if xm1_power_record_helper_69f8fc(0xa8) & 0x80 != 0 { xm1_power_record_helper_fa91b0(source as u32, record[9]); }
            if xm1_power_record_helper_69f8fc(0x5258) & 1 != 0 { xm1_power_record_helper_f521d8(record[5] as u16 as u32, record[4], record[9]); }
            xm1_power_record_helper_7582c0(record.as_mut_ptr(), request);
            let mut output = 0;
            let mut value = 0;
            xm1_power_record_helper_faa7b0(request, record.as_ptr(), &mut output, &mut value);
            let mut first = 0;
            let mut second = 0;
            let mut selection = 0;
            let mut a = 0;
            let mut b = 0;
            let mut c = 0;
            let mut d = 0;
            xm1_power_record_helper_f1c8fc(request, record.as_ptr(), &mut first, &mut second, &mut selection, &mut a, &mut b, &mut c, &mut d);
            let callback = xm1_power_record_helper_756d04(request);
            if callback != 0 {
                let publish: unsafe extern "C" fn(u32, u32, i32, i32, u32) = core::mem::transmute(callback as usize);
                publish(output, value as u32, first as i32, second as i32, selection);
            }
        }
        let mut record = [0; 33];
        xm1_power_record_helper_756734(request, record.as_mut_ptr());
        record[26] = 1;
        xm1_power_record_helper_756754(request, record.as_ptr());
        xm1_power_record_helper_740e74(source as u32, 0x50);
    }
}
