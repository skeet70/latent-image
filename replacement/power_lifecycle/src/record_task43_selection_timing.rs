//! X-M1 1.50 selection timing and focus-drive handoff.
//! Source: x70view 55436fc85c0560dd252b0410af6f2427fe198cd3; id5 f17b40..f1a104.

#![allow(unsafe_op_in_unsafe_fn)]

use core::ptr::{read_volatile, write_volatile};
#[path = "record_task43_selection_timing_abi.rs"]
mod abi;
use abi::*;
#[path = "record_task43_selection_timing_records.rs"]
mod records;
#[path = "record_task43_selection_timing_publish.rs"]
mod publish;
#[cfg(test)]
#[path = "record_task43_selection_timing_tests.rs"]
mod tests;

const DRIVE: usize = 0x016a04d8;
const LIST: usize = 0x0169e580;
const PROFILE: usize = 0x016ad740;
const JOB: usize = 0x016b3ddc;
const TRACE: usize = 0x41717a54;
static mut FOCUS_POSITION_CACHE: i16 = 0x7fff;
fn cache_address() -> usize { core::ptr::addr_of_mut!(FOCUS_POSITION_CACHE) as usize }

#[derive(Clone, Copy)]
struct Item(i32);
#[derive(Clone, Copy)]
struct Slot(u8);
#[derive(Clone, Copy)]
struct Position(i16);
#[derive(Clone, Copy)]
struct Address(usize);

fn word(a: usize) -> i32 { unsafe { read_volatile(a as *const i32) } }
fn byte(a: usize) -> u8 { unsafe { read_volatile(a as *const u8) } }
fn half(a: usize) -> i16 { unsafe { read_volatile(a as *const i16) } }
fn put(a: usize, v: i32) { unsafe { write_volatile(a as *mut i32, v) } }
fn put_half(a: usize, v: i16) { unsafe { write_volatile(a as *mut i16, v) } }
fn item() -> Item { Item(word(DRIVE)) }
fn selected() -> Item { Item(word(PROFILE)) }
fn address(a: usize) -> usize { word(a) as u32 as usize }
fn position() -> Position { Position(half(DRIVE + 8)) }
unsafe fn request() -> Address { Address(ef9bc0(0) as u32 as usize) }
unsafe fn copy(dst: usize, src: usize, len: usize) {
    for offset in 0..len {
        write_volatile((dst + offset) as *mut u8, read_volatile((src + offset) as *const u8));
    }
}

#[repr(C)]
struct TimingRecord { words: [i32; 12] }
impl TimingRecord {
    fn new() -> Self { Self { words: [0; 12] } }
    fn address(&mut self) -> usize { self.words.as_mut_ptr() as usize }
    unsafe fn read(item: Item) -> Self {
        let mut record = Self::new();
        h756840(item.0, record.address() as i32);
        record
    }
    unsafe fn save(&mut self, item: Item) { h756878(item.0, self.address() as i32); }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f17b40() -> i32 {
    let r = request().0;
    let mode = word(r + 4) as u32;
    if mode > 7 { return 0; }
    let mask = 1u32 << mode;
    if mask & 0x84 == 0 {
        if mask & 0x61 == 0 { return 0; }
        if h7509c4(item().0) == 1 && h754d9c(item().0) == 3 && h74fa2c(item().0) == 0 { return 0; }
    }
    if matches!(h742064(), 0x68 | 0x63) || word(DRIVE + 0x10) == 0x80 { return 0; }
    i32::from(word(DRIVE + 0xc) < h747b50())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f17c14() { ef9a94(0xa1, 0, 0); }
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f17c30() { ef9a94(0x3a, 0, 0); }
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f17c4c() { ef9a94(0x35, 0, 0); }

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f17c68(raw_item: i32) {
    for offset in (0..0x28).step_by(4) { put(DRIVE + offset, 0); }
    let item = Item(raw_item);
    put(DRIVE, item.0);
    put_half(DRIVE + 8, h76da84() as i16);
    put(DRIVE + 0x10, 0x80);
    put(DRIVE + 0x14, 2);
    put(DRIVE + 4, 0x80);
    let enabled = f20300() == 1 || h10538a4() == 1;
    let mut record = TimingRecord::read(item);
    record.words[9] = i32::from(enabled);
    record.save(item);
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f17d0c() -> i32 {
    if f20300() != 1 && (f21bc0() == 1 || f21b80() == 1) { return 1; }
    i32::from(h76dac4() != 0)
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f17d54() {
    trace_begin(0x02040100);
    let timing = f201b0();
    h76e654(xm1_power_record_helper_f17c30, word(DRIVE + 4), 1, 1, 0, timing);
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f17da4() -> i32 { ((word(request().0 + 0x10) as u32 >> 3) & 1) as i32 }
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f17dbc() -> i32 { f153a0() }
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f17dcc() -> i32 { f15414() }
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f17ddc() -> i32 { f154b0() }
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f17dec() {
    let r = ef9c00() as u32 as usize;
    let pair = address(r + 0x38);
    let index = f2a200(byte(pair) as i32, byte(pair + 1) as i32).wrapping_sub(1) as u32 as usize;
    put(DRIVE + 4, word(address(address(r + 0x14) + index * 4) + 4));
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f17e2c() { put(DRIVE + 4, h752a40(item().0, 0)); }
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f17e50() { put(DRIVE + 4, h757b78(item().0)); }
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f17e70() { put(DRIVE + 4, h756fe4(item().0)); }
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f17e90() { trace_begin(0x12040000); put(DRIVE + 0x18, 1); }
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f17eb8() { trace_begin(0x12040100); put(DRIVE + 0x1c, 1); }
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f17ee0() { put(DRIVE + 0x20, 1); }
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f17f04() { xm1_power_record_helper_f17ee0(); }
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f17f14() -> i32 {
    i32::from(word(DRIVE + 0x18) == 0 || word(DRIVE + 0x1c) == 0 || word(DRIVE + 0x20) == 0)
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f17f6c() { efa3f8(item().0, 6); }
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f17f8c() {
    if xm1_power_record_helper_f17dbc() != 0 { xm1_power_record_helper_f17dec(); }
    else if xm1_power_record_helper_f17dcc() != 0 { xm1_power_record_helper_f17e2c(); }
    else if xm1_power_record_helper_f17ddc() != 0 { xm1_power_record_helper_f17e50(); }
    else { xm1_power_record_helper_f17e70(); }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f17fd8() {
    let r = request().0;
    if word(r + 4) == 0 && h74fa2c(item().0) == 0 {
        let mut record = TimingRecord::read(item());
        copy(DRIVE + 8, record.address(), 16);
        if configuration_byte(0x8bad) == 0 {
            put(DRIVE + 0xc, 0); put(DRIVE + 0x10, 0x80); put(DRIVE + 0x14, 2);
        }
        let delta = i32::from(position().0) - i32::from(half(cache_address()));
        let threshold = configuration_half(0x3e1770) * configuration_byte(0x8bb0);
        if delta.abs() > threshold { put_half(cache_address(), position().0); }
        put_half(DRIVE + 8, half(cache_address()));
    } else if word(r + 0x10) & 8 == 0 {
        let mut record = TimingRecord::read(item());
        copy(DRIVE + 8, record.address(), 16);
        put(DRIVE + 0x10, 0x80);
    } else if word(r + 0x10) & 4 == 0 {
        h752110(item().0, (DRIVE + 8) as i32);
    } else { f20140((DRIVE + 8) as i32); }
}

unsafe fn remap_position(p: Position) -> Position {
    Position(h74947c(i32::from(p.0), word(DRIVE + 0xc), word(DRIVE + 0x10), 0,
        i32::from(h75093c(item().0) == 1), 0, word(DRIVE + 0x14)) as i16)
}
unsafe fn shift_points(target: Option<i32>, offset: Position, format: i32) {
    let mut points = [0i32; 9];
    h758940(points.as_mut_ptr() as i32);
    let count = byte(points.as_ptr() as usize) as usize + 1;
    let mut slot = 0u8;
    loop {
        let a = points.as_mut_ptr() as usize + 4 + usize::from(slot) * 8;
        let p = Position(half(a));
        let p = if let Some(target) = target {
            Position(f47300(i32::from(p.0), word(DRIVE + 0xc), word(DRIVE + 0x10), target, 0, format,
                word(DRIVE + 0x14)) as i16)
        } else { Position(p.0.wrapping_add(offset.0)) };
        put_half(a, p.0);
        slot = slot.wrapping_add(1);
        if usize::from(slot) >= count { break; }
    }
    h758980(item().0, points.as_mut_ptr() as i32);
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f18148() {
    if xm1_power_record_helper_f17b40() != 0 {
        let target = h752a40(item().0, 0);
        let format = i32::from(h75093c(item().0) == 1);
        let p = f47300(i32::from(position().0), word(DRIVE + 0xc), word(DRIVE + 0x10), target, 0, format,
            word(DRIVE + 0x14));
        put_half(DRIVE + 8, p as i16);
        shift_points(Some(target), Position(0), format);
        put(DRIVE + 0x10, target);
        let mut record = TimingRecord::read(item());
        record.words[8] = 1;
        record.save(item());
    }
    let owner = address(address(request().0 + 0x2c));
    put(owner + 0x55c0c, 0);
    put_half(DRIVE + 0x24, 0);
    let mut settings = TimingRecord::new();
    h756940(item().0, settings.address() as i32);
    if settings.words[0] != 0 && settings.words[6] != 0 {
        let flags = h76f340();
        let delta = f4c930(word(DRIVE + 0xc), word(DRIVE + 0x10), i32::from(position().0),
            word(DRIVE + 0x14), flags, word(owner + 0x56278));
        put_half(DRIVE + 8, position().0.wrapping_add(delta as i16));
        shift_points(None, Position(delta as i16), 0);
        settings.words[6] = 0;
        h756970(item().0, settings.address() as i32);
        put(owner + 0x55c0c, delta);
        put_half(DRIVE + 0x24, delta as i16);
    }
    if !matches!(h742064(), 0x68 | 0x63) && configuration_byte(0xa8) & 2 == 0 && word(DRIVE + 0x14) != 2 && configuration_byte(0x3e0246) == 1 {
        let direction = if i32::from(position().0) > h76da84() { 0 }
            else if i32::from(position().0) < h76da84() { 1 } else { h76dad4() };
        let p = h747aac(i32::from(position().0), word(DRIVE + 0x14), direction);
        put_half(DRIVE + 8, p as i16);
        put(DRIVE + 0x14, direction);
    }
    let mut record = TimingRecord::read(item());
    let mut p = Position(position().0.wrapping_sub(half(DRIVE + 0x24)));
    if record.words[8] == 1 { p = remap_position(p); }
    put_half(record.address() + 0x10, p.0);
    record.words[5] = word(DRIVE + 0xc);
    record.words[6] = 0;
    record.words[7] = word(DRIVE + 0x14);
    record.save(item());
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f18520() {
    if word(request().0 + 4) == 1 { xm1_power_record_helper_f17c4c(); return; }
    trace_begin(0x02040000);
    let p = position();
    let current = h76da84();
    let timing = f20188();
    h76e1ac(xm1_power_record_helper_f17c4c, i32::from(p.0.wrapping_sub(current as i16)),
        word(DRIVE + 0x14), 1, 0, timing, 4);
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f1859c() {
    match word(request().0 + 4) {
        0 | 5 | 6 => {
            if h74e5c0(item().0) == 1 || (xm1_power_record_helper_f17ddc() != 0 && h74fa2c(item().0) != 0) { return; }
            let mut bounds = [-1i32; 3];
            let record = TimingRecord::read(item());
            let p = if h74fa2c(item().0) == 0 { Position(half(cache_address())) } else {
                let p = Position(position().0.wrapping_sub(half(DRIVE + 0x24)));
                if record.words[8] == 1 { remap_position(p) } else { p }
            };
            if h76da38() != 2 && h76d9a8() != 2 {
                if h746018() == 0 && h74fa2c(item().0) == 0 {
                    let iris = word(DRIVE + 4);
                    let flags = h76f340();
                    let direction = h76dad4();
                    let channel = h76d9c8();
                    let maximum = h76fc4c();
                    h749e98(bounds.as_mut_ptr() as i32, i32::from(p.0), channel, iris, flags, direction, maximum);
                } else {
                    let iris = word(DRIVE + 4);
                    let flags = h76f340();
                    let direction = h76dad4();
                    let mut channel = 0;
                    let mut aperture = 0i16;
                    h76fbac(&mut channel as *mut i32 as i32, &mut aperture as *mut i16 as i32);
                    let maximum = h76fc4c();
                    h749fac(bounds.as_mut_ptr() as i32, i32::from(p.0), channel, aperture as u16 as i32, iris, flags, direction, maximum);
                }
            }
            efb9f0(bounds.as_mut_ptr() as i32, 0);
        }
        2 => {
            if h744ae8() != 0 || h76da38() == 2 || h76d9a8() == 2 { return; }
            let record = TimingRecord::read(item());
            let format = i32::from(h75093c(item().0) == 1);
            let p = f47300(i32::from(position().0), word(DRIVE + 0xc), word(DRIVE + 0x10), record.words[2], format, 0,
                word(DRIVE + 0x14));
            put_half(cache_address(), p as i16);
            let zoom = word(DRIVE + 0xc);
            let flags = h76f340();
            h76fb00(p, zoom, 0, flags, word(DRIVE + 0x14));
        }
        _ => {}
    }
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f1886c() { for offset in [0x18, 0x1c, 0x20] { put(DRIVE + offset, 0); } }
#[unsafe(no_mangle)]
pub extern "C" fn xm1_power_record_helper_f18898() -> i32 { 0 }
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_ee8ba0(callback: unsafe extern "C" fn()) { callback(); }
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f188b0() { xm1_power_record_helper_ee8ba0(xm1_power_record_helper_f17c14); }

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f188c8() {
    let flags = word(request().0 + 0x10);
    f16160(if f21a3c() == 0 { (flags as u32 >> 3 & 1) as i32 } else { 0 });
    if f16664() == 0 { return; }
    let r = request().0;
    let active = word(r + 0x10) & 2 != 0;
    let target = h752a40(selected().0, 0);
    let aperture = if word(DRIVE + 0x10) == 0x80 { target } else { word(DRIVE + 0x10) };
    let channel = f160d0();
    let mapped = f4e620((DRIVE + 8) as i32, channel, aperture);
    if !active && word(r + 4) != 0 { return; }
    let mut slot = Slot(0);
    while u32::from(slot.0) < word(PROFILE + 0x5c) as u32 {
        h754148(selected().0, i32::from(slot.0), mapped);
        slot.0 = slot.0.wrapping_add(1);
    }
    if !active || word(r + 4) == 7 { return; }
    let mut p = position();
    if xm1_power_record_helper_f17b40() != 0 {
        let channel = f160d0();
        p = Position(f47300(i32::from(p.0), channel, aperture, target, 0, 0, word(DRIVE + 0x14)) as i16);
    }
    let selected = selected();
    let channel = f160d0();
    let result = f60efc(selected.0, channel, i32::from(p.0), target, word(DRIVE + 0x14), word(LIST + 0x1cc8), address(address(r + 0x2c)) as i32);
    put(PROFILE + 0x6414, result);
    let override_value = f616e8(selected.0, word(r + 4));
    if override_value != 4 { put(PROFILE + 0x6414, override_value); }
    if word(JOB + 0x126c) != 1 {
        put(TRACE + 4, if word(PROFILE + 0x6410) == 0 { 3 } else {
            match word(PROFILE + 0x6414) { 1 => 1, 2 => 2, _ => 0 }
        });
    }
    let mut slot = Slot(0);
    while u32::from(slot.0) < word(PROFILE + 0x5c) as u32 {
        h754098(selected.0, i32::from(slot.0), word(PROFILE + 0x6414));
        slot.0 = slot.0.wrapping_add(1);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f1a0a8() {
    xm1_power_record_helper_f188c8();
    publish::xm1_power_record_helper_f19634();
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f1a0bc() {
    put_half(DRIVE + 8, h76da84() as i16);
    put(DRIVE + 0xc, 0);
    put(DRIVE + 0x10, 0x80);
    put(DRIVE + 0x14, 2);
    put(DRIVE + 0x18, 1);
    put(DRIVE + 0x20, 1);
    xm1_power_record_helper_f17e50();
}
