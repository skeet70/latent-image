#![cfg(target_arch = "arm")]

use crate::recording_engine25_native::{byte, half, missing, store, word};

unsafe extern "C" {
    fn xm1_power_interrupt_lock() -> u32;
    fn xm1_power_interrupt_unlock(saved: u32);
    fn xm1_power_task_delay(ticks: u32) -> i32;
    fn xm1_power_dcache_clean_invalidate_all();
}
fn descriptor(index: u32) -> u32 {
    if index <= 19 { 0x00e48370 + index * 16 }
    else { 0x00e4a948 + (word(0x015480d4).wrapping_mul(73).wrapping_sub(20).wrapping_add(index)).wrapping_mul(16) }
}
fn region_end(region: u32, start: u32) -> u32 {
    let rows = word(region + 0x14);
    let stride = word(region + 0xc);
    start.wrapping_add(if rows != 0 && stride != 0 { rows.wrapping_mul(stride) } else { word(region + 8) }).wrapping_sub(1)
}
fn region_available(region: u32) -> bool {
    let raw_start = word(region + 4);
    let start = raw_start | 0x40000000;
    let end = region_end(region, raw_start);
    let saved = unsafe { xm1_power_interrupt_lock() };
    let mut available = true;
    for index in 0..128 {
        let entry = 0x0154ad2c + index * 44;
        if half(entry) == 0 { break; }
        if word(entry + 16) != word(region) { continue; }
        let other_start = word(entry + 20) | 0x40000000;
        let other_end = region_end(entry + 16, other_start);
        if if start < other_start { end >= other_start } else { other_end >= start } { available = false; break; }
    }
    unsafe { xm1_power_interrupt_unlock(saved); }
    available
}
fn find_region(operation: u32) -> u32 {
    let requested = descriptor(operation);
    let mut canonical = 0;
    while canonical <= 92 && word(descriptor(canonical)) != word(requested) { canonical += 1; }
    let saved = unsafe { xm1_power_interrupt_lock() };
    for offset in 0..26 {
        unsafe { core::ptr::write_volatile((0x015480b8 + offset) as *mut u8, byte(0x015480dc + canonical * 26 + offset)); }
    }
    unsafe { xm1_power_interrupt_unlock(saved); }
    if word(requested) == 0 { return 0; }
    let count = half(descriptor(canonical) + 4) as i16;
    for index in 0..count {
        let index = index as u32;
        if byte(0x015480b8 + index / 8) & (0x80 >> (index % 8)) != 0 { continue; }
        let region = word(requested + 12) + index * 28;
        if region_available(region) { return region; }
    }
    0
}
fn current_capture() -> Option<u32> {
    let index = word(0x0157be10);
    if index == u32::MAX { None }
    else if index <= 200 { Some(word(0x0157dd3c).wrapping_add(index.wrapping_mul(0x326c))) }
    else { missing() }
}
fn prepare() {
    const BUFFER: u32 = 0x01545560;
    store(BUFFER + 4, 0x00355200);
    store(BUFFER + 12, 0x0024d820);
    store(BUFFER + 20, 0x0004b000);
    store(BUFFER + 28, 0x002ddc00);
    let mut region = find_region(36);
    if region == 0 {
        if let Some(capture) = current_capture() {
            let eligible = word(capture + 0x5a8) != 1
                && (word(capture + 0x4a8) != 0 || !matches!(word(capture + 0x550), 0x97 | 0x98))
                && word(capture + 0x4ac) != 1 && word(capture + 0x614) == 0 && word(capture + 0x61c) == 0;
            if eligible { region = find_region(45); }
        }
    }
    if region == 0 {
        for (offset, value) in [(0, 0x4829d600), (8, 0x1f9bdfc0), (16, 0x485f2800), (24, 0x4863d800)] { store(BUFFER + offset, value); }
    } else {
        let first = word(region + 4) | 0x40000000;
        store(BUFFER, first);
        let second = (first & !0x40000000).wrapping_add(word(BUFFER + 4));
        store(BUFFER + 8, second);
        let third = (second | 0x40000000).wrapping_add(word(BUFFER + 12));
        store(BUFFER + 16, third);
        store(BUFFER + 24, (third | 0x40000000).wrapping_add(word(BUFFER + 20)));
        let required = word(BUFFER + 4).wrapping_add(word(BUFFER + 12)).wrapping_add(word(BUFFER + 20));
        if word(region + 8) < required { loop { core::hint::spin_loop(); } }
    }
}
fn select_primary(buffer: u32, _size: u32) -> bool {
    if buffer == 0 { return false; }
    let mode = word(0x016fec4c);
    if !matches!(mode, 1 | 2) || word(0x017037bc) != 0 { return false; }
    store(0x017037d0, buffer);
    // 0x75e6f0 returns without effects; 0x75e6d8 returns one.
    true
}
fn select_secondary(pair: &[u32; 2]) -> bool {
    if (word(0x017032b0), word(0x017032b4)) != (pair[0], pair[1]) {
        let active = word(0x017032a0);
        if active != 0 && (!matches!(word(0x016fec4c), 1 | 2) || !(6..=9).contains(&active)) { return false; }
        store(0x017032b0, pair[0]);
        store(0x017032b4, pair[1]);
    }
    if pair[0] != 0 { unsafe { xm1_power_dcache_clean_invalidate_all(); } }
    true
}
pub(crate) fn acquire_output_buffers() {
    prepare();
    while !select_primary(word(0x01545560), word(0x01545564)) { unsafe { xm1_power_task_delay(15); } }
    let pair = [word(0x01545568), word(0x0154556c)];
    while !select_secondary(&pair) { unsafe { xm1_power_task_delay(15); } }
    for offset in 0..4 { store(0x0152a7ec + offset * 4, word(0x01545570 + offset * 4)); }
}
