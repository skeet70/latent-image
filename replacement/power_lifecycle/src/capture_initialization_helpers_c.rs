//! Capture aperture selection and setting-change tracking.
//! Source: x70view 55436fc85c0560dd252b0410af6f2427fe198cd3; X-M1 1.50 bytes 63ba0c..63d060.

use core::ptr::{read_volatile, write_volatile};
use crate::mode_initialization::{CaptureModeId, FlagId, query_flag};
use crate::capture_initialization_helpers_a::{PropertyKey, SettingValue, property, project, value_available, preferred_value};

const INPUT: usize = 0x956838;
const FRAME: usize = 0x944580;
fn word(address: usize) -> u32 { unsafe { read_volatile(address as *const u32) } }
fn byte(address: usize) -> u8 { unsafe { read_volatile(address as *const u8) } }
fn half(address: usize) -> u16 { unsafe { read_volatile(address as *const u16) } }
fn put(address: usize, value: u32) { unsafe { write_volatile(address as *mut u32, value) } }
fn put_byte(address: usize, value: u8) { unsafe { write_volatile(address as *mut u8, value) } }
unsafe fn get(key: u32) -> u32 { unsafe { property(PropertyKey(key)).0 } }
unsafe fn mapped(key: u32, value: u32) -> u32 { unsafe { project(PropertyKey(key), SettingValue(value), 1, 3).0 } }
unsafe fn selected(mode: CaptureModeId, key: u32, value: u32) -> Option<u32> {
    let value = unsafe { if value_available(mode, PropertyKey(key), SettingValue(value)) { value } else { preferred_value(mode, PropertyKey(key)).0 } };
    (value != 0x7fff_ffff).then_some(value)
}
unsafe fn first_flag(table: usize, stride: usize) -> u32 {
    let first = word(table);
    let mut row = table;
    loop {
        let token = word(row);
        if token == 0 { return first; }
        if unsafe { query_flag(FlagId::new(token)) } == 1 { return token; }
        row += stride;
    }
}

pub(crate) unsafe fn f_63a244() -> u32 {
    if word(FRAME + 0xbf54) == 0 { return 1; }
    match word(FRAME + 0xbf8c) { 1 => 2, 2 => 3, _ => 1 }
}
pub(crate) unsafe fn f_63a4f4() -> u32 { unsafe { first_flag(0x79fed4, 8) } }
pub(crate) unsafe fn f_63a6fc() -> u32 { unsafe { first_flag(0x77495c, 4) } }
pub(crate) unsafe fn f_63ba0c(refresh: u32) {
    if refresh == 1 { put(INPUT + 0x354, u32::from(unsafe { f_63a6fc() } == 0x205)); }
}
pub(crate) fn f_63ba38(mode: CaptureModeId) -> bool { mode.raw().wrapping_sub(0x23) <= 1 }
pub(crate) unsafe fn f_646458(mode: CaptureModeId) -> u32 { unsafe { get(if mode.raw() == 0x23 { 0x1cc } else { 0x1cb }) as u16 as u32 } }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ApertureValue(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ApertureIndex(pub u8);

pub(crate) fn nearest_aperture(value: ApertureValue, values: &[u16]) -> ApertureIndex {
    let mut best = 0;
    let mut distance = u32::MAX;
    for (index, candidate) in values.iter().enumerate() {
        let next = u32::from(*candidate).abs_diff(value.0);
        if next < distance { best = index; distance = next; }
    }
    ApertureIndex(best.wrapping_add(1) as u8)
}
pub(crate) unsafe fn f_72c854(value: ApertureValue) -> ApertureIndex {
    let count = byte(0x95cae8) as usize;
    nearest_aperture(value, unsafe { core::slice::from_raw_parts(0x93a814 as *const u16, count) })
}
pub(crate) const fn f_72ccac(value: i32) -> u32 { value as u16 as u32 }
pub(crate) unsafe fn f_72ccbc(value: u16) -> ApertureValue {
    let configuration = word(0x96ca50) as usize;
    let value = if byte(configuration + 0x3e0009) == 1 { 0 } else { u32::from(value) };
    let count = byte(configuration + 0x3e0f82);
    let count = if count.wrapping_sub(1) > 30 { 1 } else { u32::from(count) };
    let index = if count > value { value } else if value > 30 && value < count + 31 { value - 31 } else { 0 };
    let entry = byte(configuration + 0x3f4600 + index as usize);
    if entry == 0xff { ApertureValue(100) } else { ApertureValue(u32::from(half(configuration + ((0x1f1298 + u32::from(entry)) * 2) as usize))) }
}
pub(crate) unsafe fn f_63b8bc(mode: CaptureModeId) -> ApertureIndex {
    let token = unsafe { f_63a4f4() };
    if unsafe { f_63a244() } != 2 { return ApertureIndex(0); }
    let mut row = 0x79fed4;
    loop {
        let current = word(row);
        if current == 0 { return ApertureIndex(0); }
        if current == token {
            let value = unsafe { f_72ccbc(half(row + 4)) };
            let value = if token == 0x400 { ApertureValue(unsafe { f_646458(mode) }) } else { value };
            return unsafe { f_72c854(value) };
        }
        row += 8;
    }
}
pub(crate) unsafe fn f_63ba5c(mode: CaptureModeId) {
    let (base, adjustment) = if !f_63ba38(mode) { (1, 0) } else {
        match unsafe { f_63a244() } {
            2 => (unsafe { f_63b8bc(mode) }.0, unsafe { get(0x1d0) }.wrapping_sub(7) as u8),
            1 | 3 => (unsafe { f_72c854(ApertureValue(f_646458(mode))) }.0, 0),
            _ => (1, 0),
        }
    };
    let adjusted = base.wrapping_add(adjustment);
    for offset in [0xa9, 0xaa, 0xad, 0xae] { put_byte(INPUT + offset, adjusted); }
    for offset in [0xab, 0xac] { put_byte(INPUT + offset, base); }
}
pub(crate) unsafe fn f_63bc84() {
    let value = match unsafe { get(0x55) } { 1 => 0x18, 2 => 0x1c, 3 => 0x23, 4 => unsafe { get(0x5f) }, 5 => unsafe { get(0x60) }, _ => 0x15 };
    put(INPUT + 0x358, value);
}
pub(crate) unsafe fn f_63bd20() {
    let profile = unsafe { get(0x55) };
    let key = 0x62 + if profile <= 5 { profile } else { 0 };
    put(INPUT + 0x3a0, unsafe { mapped(0x61, get(key)) });
}
#[derive(Clone, Copy)]
enum ColorAxis { First, Second }
unsafe fn update_color(slot: u32, axis: ColorAxis) {
    let profile = unsafe { get(0x55) };
    let profile = if profile <= 5 { profile } else { 0 };
    let axis = match axis { ColorAxis::First => 0, ColorAxis::Second => 1 };
    let key = 0x6c + slot * 2 + profile * 8 + axis;
    let value = unsafe { mapped(0x6a, get(key)) };
    put_byte(INPUT + 0x3a4 + slot as usize * 2 + axis as usize, value as u8);
}
pub(crate) unsafe fn f_63bd34() { for slot in 0..4 { unsafe { update_color(slot, ColorAxis::First) }; } }
pub(crate) unsafe fn f_63bd78() { for slot in 0..4 { unsafe { update_color(slot, ColorAxis::Second) }; } }
pub(crate) unsafe fn f_63be0c() {
    let profile = unsafe { get(0x55) };
    let key = 0x9d + if profile <= 5 { profile } else { 0 };
    put_byte(INPUT + 0x3ac, unsafe { mapped(0x9c, get(key)) } as u8);
}
pub(crate) unsafe fn f_63c054() { put(INPUT + 0x3b0, 0); }
pub(crate) unsafe fn f_63c068(mode: CaptureModeId) {
    let mut value = unsafe { mapped(0xf6, get(0xf6)) };
    if word(INPUT + 0x64) == 1 {
        for (source, destination) in [(0xcfd0, 0x50), (0xcfd4, 0x58), (0xcfd8, 0x60)] { put(INPUT + destination, word(FRAME + source)); }
        value = word(INPUT + 0x50);
    }
    if let Some(value) = unsafe { selected(mode, 0xf6, value) } { put(INPUT + 0x50, value); }
}
pub(crate) unsafe fn f_63c108(mode: CaptureModeId) { if let Some(value) = unsafe { selected(mode, 0xf2, mapped(0xf2, get(0xf2))) } { put(INPUT + 0x54, value); } }
pub(crate) unsafe fn f_63c170(mode: CaptureModeId) { if let Some(value) = unsafe { selected(mode, 0xf3, mapped(0xf3, get(0xf3))) } { put(INPUT + 0x58, value); } }
pub(crate) unsafe fn f_63c1dc(mode: CaptureModeId) {
    let value = unsafe { mapped(0xf4, get(0xf4)) };
    let value = if mode.raw() == 0x2c { if word(INPUT + 0x54).wrapping_sub(7) <= 2 { 0 } else { word(INPUT + 8) } } else { value };
    if let Some(value) = unsafe { selected(mode, 0xf4, value) } { put(INPUT + 0x5c, value); }
}

pub(crate) unsafe fn f_63bc24() {
    put(INPUT + 0x358, 0);
    for slot in 0..4 { put_byte(INPUT + 0x3a4 + slot * 2, 0); put_byte(INPUT + 0x3a5 + slot * 2, 0); }
    put(INPUT + 0x3a0, 0);
    put_byte(INPUT + 0x3ac, 0);
}
pub(crate) const fn f_63c274() -> u32 { 0 }
pub(crate) unsafe fn f_63be24(mode: CaptureModeId) {
    let value = unsafe { u32::from(get(0x1e8) == 1) };
    if let Some(value) = unsafe { selected(mode, 0x1e8, value) } { put(INPUT + 0x148, value); }
    if word(INPUT) == 0xc && word(INPUT + 0x90) == 1 { put(INPUT + 0x148, 0); }
}

#[derive(Clone, Copy)]
enum TrackedSetting { Output, Recording, Detail, Exposure, Focus, Drive, Stabilization, Metering, Mode }
impl TrackedSetting {
    const ALL: [Self; 9] = [Self::Output, Self::Recording, Self::Detail, Self::Exposure, Self::Focus, Self::Drive, Self::Stabilization, Self::Metering, Self::Mode];
    const fn flag(self) -> usize { match self { Self::Output => 0xd02c, Self::Recording => 0xd074, Self::Detail => 0xd234, Self::Exposure => 0xd2b4, Self::Focus => 0xd2f4, Self::Drive => 0xd0f4, Self::Stabilization => 0xd274, Self::Metering => 0xd334, Self::Mode => 0xd1f4 } }
    const fn input(self) -> usize { match self { Self::Output => 0x50, Self::Recording => 0x7c, Self::Detail => 0x344, Self::Exposure => 0x30c, Self::Focus => 0x4c, Self::Drive => 0x84, Self::Stabilization => 0x148, Self::Metering => 0x28, Self::Mode => 0xc } }
    fn snapshot(self) { put(FRAME + self.flag() + 0x3c, word(INPUT + self.input())); }
    fn clear(self) {
        put_byte(FRAME + self.flag(), 0);
        for index in 0..14 { put(FRAME + self.flag() + 4 + index * 4, 0); }
    }
    fn latch(self) { put_byte(FRAME + self.flag() + 2, byte(FRAME + self.flag())); }
    fn changed(self) -> bool { word(INPUT + self.input()) != word(FRAME + self.flag() + 0x3c) }
    fn enabled(self) -> bool { byte(FRAME + self.flag()) == 1 || byte(FRAME + self.flag() + 2) == 1 }
    fn mark(self) { put_byte(FRAME + 0xd374, 1); put_byte(FRAME + self.flag() + 3, 1); }
}

unsafe fn tracker_default(setting: TrackedSetting, mode: CaptureModeId) -> bool {
    use crate::capture_initialization_conversions as conversions;
    let expected = unsafe { match setting {
        TrackedSetting::Output => {
            return word(INPUT + 0x50) == conversions::f_644004(0)
                && word(INPUT + 0x58) == conversions::f_644054()
                && word(INPUT + 0x60) == conversions::f_6440b4();
        }
        TrackedSetting::Recording => conversions::f_6444a0(mode),
        TrackedSetting::Detail => mapped(0x17, get(0x17)),
        TrackedSetting::Exposure => mapped(0x111, get(0x111)),
        TrackedSetting::Focus => conversions::f_6460c0(),
        TrackedSetting::Drive => conversions::f_643f30(mode),
        TrackedSetting::Stabilization => u32::from(get(0x1e8) == 1),
        TrackedSetting::Metering => return crate::capture_initialization_helpers_a::available_count(mode, PropertyKey(0x1c2)) == 1 || word(INPUT + 0x148) == 0,
        TrackedSetting::Mode => mapped(0x1b1, get(0x1b1)),
    } };
    word(INPUT + setting.input()) == expected
}

pub(crate) unsafe fn f_63c51c(operation: u32, source: u32, key: u32) {
    match operation {
        1 => {
            put_byte(FRAME + 0xd374, 0);
            put_byte(FRAME + 0xd375, 0);
            let mode = CaptureModeId::new(word(INPUT));
            for setting in TrackedSetting::ALL {
                put_byte(FRAME + setting.flag() + 3, 0);
                setting.snapshot();
                if matches!(setting, TrackedSetting::Output) {
                    put(FRAME + 0xd06c, word(INPUT + 0x58));
                    put(FRAME + 0xd070, word(INPUT + 0x60));
                }
                if unsafe { tracker_default(setting, mode) } { setting.clear(); }
                setting.latch();
                put_byte(FRAME + setting.flag() + 1, 0);
            }
            for offset in 0..860 {
                put_byte(FRAME + 0xd02c + 860 + offset, byte(FRAME + 0xd02c + offset));
            }
        }
        0x34 => {
            let setting = match key {
                0xf6 | 0xf8 => TrackedSetting::Output, 0x141 => TrackedSetting::Recording,
                0x17 => TrackedSetting::Detail, 0x111 => TrackedSetting::Exposure,
                0x129 => TrackedSetting::Focus, 0x104 => TrackedSetting::Drive,
                0x1e8 | 0x1e9 => TrackedSetting::Stabilization, 0x1c2 => TrackedSetting::Metering,
                0x1b1 => TrackedSetting::Mode, _ => return,
            };
            if !matches!(setting, TrackedSetting::Mode) { setting.clear(); }
            setting.latch();
            setting.snapshot();
            if matches!(setting, TrackedSetting::Output) {
                put(FRAME + 0xd06c, word(INPUT + 0x58));
                put(FRAME + 0xd070, word(INPUT + 0x60));
            }
        }
        0x1d => {
            let output = TrackedSetting::Output;
            let class_offset = match word(INPUT) { 0x21 | 0x25 => Some(0xd378), 0x24 => Some(0xd37c), 0x23 => Some(0xd380), 0x22 => Some(0xd384), _ => None };
            let class = class_offset.map_or(0, |offset| word(FRAME + offset));
            if !output.changed() {
                put_byte(FRAME + 0xd374, 0);
                put_byte(FRAME + output.flag() + 3, 0);
                put_byte(FRAME + 0xd376, 0);
            } else if (byte(FRAME + output.flag() + 2) == 1 && class != 1)
                || (byte(FRAME + output.flag() + 2) != 1 && byte(FRAME + output.flag()) == 1) {
                output.mark();
                put_byte(FRAME + 0xd376, u8::from(source == 0x1b && word(FRAME + 0xcfc4) == 1));
            }
            if source != 0x1b {
                let recording = TrackedSetting::Recording;
                let current = byte(FRAME + recording.flag()) == 1;
                let latched = byte(FRAME + recording.flag() + 2) == 1;
                if recording.changed() && ((latched && !current && class != 2) || (!latched && current) || (latched && current && class != 2)) { recording.mark(); }
                let drive = TrackedSetting::Drive;
                if drive.enabled() && drive.changed() { drive.mark(); }
            }
            for setting in [TrackedSetting::Detail, TrackedSetting::Exposure, TrackedSetting::Focus, TrackedSetting::Stabilization, TrackedSetting::Metering, TrackedSetting::Mode] {
                if setting.enabled() && setting.changed() { setting.mark(); }
            }
        }
        _ => {}
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ShutterValue(pub i32);
pub(crate) fn shutter_code(value: ShutterValue) -> i32 {
    const VALUES: [i32; 52] = [-3000, -2500, -2000, -1500, -1300, -1000, -800, -650, -500, -400, -300, -250, -200, -150, -130, -100, 130, 160, 200, 250, 300, 400, 500, 600, 800, 1000, 1300, 1500, 2000, 2500, 3000, 4000, 5000, 6000, 8000, 10000, 12500, 16000, 20000, 25000, 32000, 40000, 50000, 64000, 80000, 100000, 120000, 160000, 200000, 250000, 300000, 400000];
    if value.0 == -1 { return -1; }
    if value.0 == 100 { return 28; }
    VALUES.iter().position(|entry| *entry == value.0).map_or(0, |index| index as i32 + 13)
}
pub(crate) unsafe fn f_63b96c() -> u32 {
    let token = unsafe { first_flag(0x7a0e40, 8) };
    let mut row = 0x7a0e40;
    loop {
        let current = word(row);
        if current == 0 { return 0; }
        if current == token {
            return if token == 0x32a { unsafe { get(0x1d2) } } else { shutter_code(ShutterValue(word(row + 4) as i32)) as u32 };
        }
        row += 8;
    }
}
pub(crate) unsafe fn f_63b2c0() {
    if word(INPUT + 0xfc) != 2 {
        put(INPUT + 0xb8, unsafe { mapped(0x1dd, get(0x1dd)) });
        return;
    }
    put(INPUT + 0xb8, 2);
    if word(INPUT + 0x4c) != 2 || word(FRAME + 0xfff0) == 1 { return; }
    let red = byte(INPUT + 0xb6);
    let blue = byte(INPUT + 0xb7);
    let (red, blue) = match half(INPUT + 0xb6) {
        0x0303 => (red.wrapping_sub(1), blue.wrapping_sub(1)),
        0xfd03 => (red.wrapping_sub(1), blue.wrapping_add(1)),
        0x03fd => (red.wrapping_add(1), blue.wrapping_sub(1)),
        0xfdfd => (red.wrapping_add(1), blue.wrapping_add(1)),
        _ if red as i8 == 3 => (red.wrapping_sub(1), blue),
        _ if red as i8 == -3 => (red.wrapping_add(1), blue),
        _ if blue as i8 == 3 => (red, blue.wrapping_sub(1)),
        _ if blue as i8 == -3 => (red, blue.wrapping_add(1)),
        _ => (red, blue),
    };
    put_byte(INPUT + 0xb6, red);
    put_byte(INPUT + 0xb7, blue);
    unsafe {
        crate::capture_initialization_helpers_a::save(PropertyKey(0x1db), SettingValue((i32::from(red as i8) + 3) as u32));
        crate::capture_initialization_helpers_a::save(PropertyKey(0x1dc), SettingValue((i32::from(blue as i8) + 3) as u32));
    }
}

pub(crate) unsafe fn f_63c2bc(value: u32) -> bool {
    word(FRAME + 0xbfc4) == value || (0..9).any(|index| word(FRAME + 0xbfe4 + index * 0x20) == value)
}
pub(crate) unsafe fn f_63c324(operation: u32) {
    if operation < 2 {
        let replacement = match (word(INPUT + 0x50), word(INPUT + 0x7c)) { (0, value) if value > 0x16 => Some(0x16), (1, 0x1b) => Some(0x19), _ => None };
        if let Some(value) = replacement {
            put(INPUT + 0x7c, value);
            put_byte(FRAME + 0xd074, 1);
            put(FRAME + 0xd078, 1);
        }
    }
    if operation != 0 && operation != 2 { return; }
    let recording = word(INPUT + 0x7c);
    let output = word(INPUT + 0x50);
    let class = if (0x17..=0x19).contains(&recording) && output == 0 { 1 } else if recording == 0x1b && output != 2 { 2 } else { return; };
    let value = unsafe { project(PropertyKey(0xf9), SettingValue(word(INPUT + 0x60)), 3, 1).0 };
    for index in 0..12 {
        let row = 0x7a00cc + index * 16;
        if word(row + 8) == class && word(row + 12) == value {
            put(INPUT + 0x50, word(row));
            put_byte(FRAME + 0xd02c, 1);
            put(FRAME + 0xd034, 1);
            return;
        }
    }
}

pub(crate) const fn f_63bb20() -> u32 { 0 }
pub(crate) const fn f_63bb84() -> u32 { 0 }
pub(crate) fn f_63bb9c(mode: CaptureModeId) -> bool { f_63ba38(mode) }
pub(crate) unsafe fn f_63bb38(mode: CaptureModeId) -> bool {
    (unsafe { crate::capture_initialization_helpers_a::available_count(mode, PropertyKey(0x1cd)) > 1 })
        || (mode.raw() == 0xc && word(INPUT + 0x90) == 1)
}
pub(crate) unsafe fn f_63bbc0() -> u32 { unsafe { first_flag(0x7749a4, 4) } }

#[cfg(test)]
#[path = "capture_initialization_helpers_c_tests.rs"]
mod tests;
