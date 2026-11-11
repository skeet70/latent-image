use crate::mode_initialization::CaptureModeId;
use core::ptr::{read_volatile, write_volatile};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PropertyKey(pub u32);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SettingValue(pub u32);
const UNAVAILABLE: SettingValue = SettingValue(0x7fff_ffff);
const END: u32 = 0x7fff_fffe;
const INPUT: usize = 0x956838;
const FRAME: usize = 0x944580;
fn word(address: usize) -> u32 { unsafe { read_volatile(address as *const u32) } }
fn put(address: usize, value: u32) { unsafe { write_volatile(address as *mut u32, value) } }
fn half(address: usize, value: u16) { unsafe { write_volatile(address as *mut u16, value) } }
fn byte(address: usize, value: u8) { unsafe { write_volatile(address as *mut u8, value) } }

fn column(mode: CaptureModeId) -> Option<usize> {
    for index in 0..45 {
        let candidate = word(0x792fac + index * 4);
        if candidate == 45 { return None; }
        if candidate == mode.raw() { return Some(index * 4); }
    }
    None
}
fn registry(key: PropertyKey) -> Option<(usize, u32)> {
    let mut row = 0x79944c;
    loop {
        let records = word(row + 8) as usize;
        if records == 0 { return None; }
        if word(row) == key.0 { return Some((records, word(row + 4))); }
        row += 12;
    }
}

pub(crate) unsafe fn value_available(mode: CaptureModeId, key: PropertyKey, value: SettingValue) -> bool {
    let Some(col) = column(mode) else { return false; };
    let Some((mut row, kind)) = registry(key) else { return false; };
    if kind == 1 {
        if value.0 == word(row + 0x5c) { return word(row + 0x60 + col) == 2; }
        return (word(row) as i32) <= (value.0 as i32)
            && (value.0 as i32) <= (word(row + 0xb8) as i32)
            && word(row + 4 + col).wrapping_sub(1) <= 1
            && word(row + 0xbc + col).wrapping_sub(1) <= 1;
    }
    loop {
        let candidate = word(row);
        if candidate == END { return false; }
        if candidate == value.0 { return word(row + 4 + col).wrapping_sub(1) <= 1; }
        row += 92;
    }
}

pub(crate) unsafe fn preferred_value(mode: CaptureModeId, key: PropertyKey) -> SettingValue {
    let Some(col) = column(mode) else { return UNAVAILABLE; };
    let Some((mut row, kind)) = registry(key) else { return UNAVAILABLE; };
    if kind == 1 {
        return if word(row + 0x60 + col) == 2 { SettingValue(word(row + 0x5c)) } else { UNAVAILABLE };
    }
    loop {
        let candidate = word(row);
        if candidate == END { return UNAVAILABLE; }
        if word(row + 4 + col) == 2 { return SettingValue(candidate); }
        row += 92;
    }
}

pub(crate) unsafe fn available_count(mode: CaptureModeId, key: PropertyKey) -> u32 {
    let Some(col) = column(mode) else { return 0; };
    let Some((mut row, _)) = registry(key) else { return 0; };
    let mut count = 0;
    while word(row) != END {
        if word(row + 4 + col).wrapping_sub(1) <= 1 { count += 1; }
        if count == 2 { break; }
        row += 92;
    }
    count
}

unsafe extern "C" {
    fn xm1_property_get(id: u32, bank: u32, defaults: u32, selection: u32) -> u32;
    fn xm1_property_set(id: u32, value: u32, bank: u32, selection: u32) -> u32;
}
pub(crate) unsafe fn property(key: PropertyKey) -> SettingValue {
    SettingValue(unsafe { xm1_property_get(key.0, 0, 0, u32::from(word(0x951564) != 3)) })
}
pub(crate) unsafe fn save(key: PropertyKey, value: SettingValue) {
    unsafe { xm1_property_set(key.0, value.0, 0, u32::from(word(0x951564) != 3)); }
}
pub(crate) unsafe fn project(key: PropertyKey, value: SettingValue, from: u32, to: u32) -> SettingValue {
    if key.0 > 0x286 { put(0x89edc8, 1); return SettingValue(0); }
    let mut row = word(0x81b09c + key.0 as usize * 4) as usize;
    let mut index = 0;
    if row != 0 {
        loop {
            let fields = [word(row), word(row + 4), word(row + 8)];
            if fields == [0xffff; 3] { break; }
            let input = match from { 0 => Some(index), 1..=3 => Some(fields[(from - 1) as usize]), _ => None };
            if input == Some(value.0) {
                put(0x89edc8, 0);
                let output = match to { 0 => index, 1..=3 => fields[(to - 1) as usize], _ => 0 };
                if output == END { put(0x89edc8, 1); return SettingValue(0); }
                return SettingValue(output);
            }
            row += 12;
            index += 1;
        }
    }
    put(0x89edc8, 1);
    SettingValue(0)
}
unsafe fn selected(mode: CaptureModeId, key: PropertyKey, value: SettingValue) -> SettingValue {
    if unsafe { value_available(mode, key, value) } { value } else { unsafe { preferred_value(mode, key) } }
}
unsafe fn converted(key: PropertyKey) -> SettingValue { unsafe { project(key, property(key), 1, 3) } }
unsafe fn apply(mode: CaptureModeId, key: PropertyKey, target: usize) -> SettingValue {
    let value = unsafe { selected(mode, key, converted(key)) };
    if value != UNAVAILABLE { put(target, value.0); }
    value
}

use crate::capture_initialization_conversions as conversions;

macro_rules! profile_helper {
    ($($name:ident, $key:expr, $offset:expr);* $(;)?) => {$(
        pub(crate) unsafe fn $name(mode: CaptureModeId) {
            unsafe { apply(mode, PropertyKey($key), INPUT + $offset); }
        }
    )*};
}
profile_helper! {
    f_63e5cc, 0x138, 0x24;
    f_63e708, 0xed, 0x2c;
    f_63e7a4, 0x17, 0x344;
    f_63eb60, 0x1d, 0x64;
    f_63ec6c, 0x10f, 0x68;
    f_63ecd8, 0x139, 0x6c;
    f_63ed44, 0x20, 0x314;
    f_63edb0, 0xf0, 0x310;
    f_63ee88, 0x10d, 0x74;
    f_63eef4, 0x10c, 0x78;
    f_63f148, 0x100, 0x35c;
    f_63f1b4, 0x101, 0x360;
    f_63f220, 0x102, 0x364;
    f_63f330, 0x10e, 0x88;
    f_63f39c, 0x1c5, 0x8c;
    f_63fcc4, 0xef, 0x164;
    f_63ff08, 0x110, 0x308;
    f_63ff74, 0x111, 0x30c;
    f_63ffe0, 0x28, 0x318;
    f_64004c, 0x24, 0x31c;
}

macro_rules! candidate_helper {
    ($($name:ident, $key:expr, $getter:ident, $offset:expr);* $(;)?) => {$(
        pub(crate) unsafe fn $name(mode: CaptureModeId) {
            let candidate = SettingValue(unsafe { conversions::$getter() });
            let value = unsafe { selected(mode, PropertyKey($key), candidate) };
            if value != UNAVAILABLE { put(INPUT + $offset, value.0); }
        }
    )*};
}
candidate_helper! {
    f_63ea30, 0xf9, f_6440b4, 0x60;
    f_63ea80, 0xf9, f_6440b4, 0x32c;
    f_63ebcc, 0xfa, f_644054, 0x58;
    f_63ec1c, 0xfa, f_644080, 0x5c;
    f_63fb10, 0x140, f_64666c, 0x154;
    f_63fd30, 0x12c, f_6466d0, 0x2e0;
    f_63fd80, 0x12d, f_646734, 0x2e4;
}

pub(crate) unsafe fn f_63e430(mode: CaptureModeId) {
    unsafe { apply(mode, PropertyKey(0x1b1), INPUT + 0xc); }
    if word(INPUT) == 0xc && word(INPUT + 0x90) == 1 {
        unsafe { crate::capture_initialization::f_63d564(0); }
    }
}
pub(crate) unsafe fn f_63e4cc(mode: CaptureModeId) {
    unsafe { apply(mode, PropertyKey(0x1d9), INPUT + 0x10); }
    if word(FRAME + 0xbfa0) == 1 { put(INPUT + 0x10, 0); return; }
    if mode.raw() == 0xf && word(INPUT + 0x10) == 2 {
        put(INPUT + 0x10, 1);
        let key = PropertyKey(0x1d9);
        unsafe { save(key, project(key, SettingValue(word(INPUT + 0x10)), 3, 1)); }
    }
}
pub(crate) unsafe fn f_63e5ac(mode: CaptureModeId) -> u32 {
    u32::from(unsafe { available_count(mode, PropertyKey(0x1d9)) } > 1)
}
pub(crate) unsafe fn f_63e638(mode: CaptureModeId) {
    let key = PropertyKey(0x1c2);
    let value = if unsafe { available_count(mode, key) } == 1 {
        unsafe { preferred_value(mode, key) }
    } else {
        if word(INPUT + 0x148) == 1 {
            put(INPUT + 0x28, 0);
            byte(FRAME + 0xd334, 1);
            put(FRAME + 8 + (0x34cc + 0xd) * 4, 1);
            return;
        }
        unsafe { selected(mode, key, converted(key)) }
    };
    if value != UNAVAILABLE { put(INPUT + 0x28, value.0); }
}
pub(crate) unsafe fn f_63e774() {
    put(INPUT + 0x30, unsafe { converted(PropertyKey(0xee)) }.0);
}
pub(crate) unsafe fn f_63e810(mode: CaptureModeId, white_balance: u32) {
    let red = unsafe { selected(mode, PropertyKey(0x112), SettingValue(conversions::f_644a24(white_balance))) };
    if red != UNAVAILABLE {
        for slot in [0, 1, 2] { put(INPUT + 0x34 + slot * 8, red.0); }
    }
    let blue = unsafe { selected(mode, PropertyKey(0x112), SettingValue(conversions::f_644bd4(white_balance))) };
    if blue != UNAVAILABLE {
        for slot in [0, 1, 2] { put(INPUT + 0x38 + slot * 8, blue.0); }
    }
}
pub(crate) unsafe fn f_63ead0(mode: CaptureModeId) {
    let value = unsafe { apply(mode, PropertyKey(0xf2), INPUT + 0x328) };
    put(INPUT + 0x328, if word(INPUT + 0x320) == 1 { value.0 } else { word(INPUT + 0x50) });
}
pub(crate) unsafe fn f_63ee1c(mode: CaptureModeId) {
    unsafe { apply(mode, PropertyKey(0x183), INPUT + 0x70); }
}
pub(crate) unsafe fn f_63f0a0(mode: CaptureModeId) {
    let key = PropertyKey(0xfe);
    let candidate = if mode.raw() == 0x2c { SettingValue(word(INPUT + 0x80)) } else { unsafe { converted(key) } };
    let mode = if word(INPUT) == 0x2c && word(INPUT + 8) == 1 { CaptureModeId::new(1) } else { mode };
    let value = unsafe { selected(mode, key, candidate) };
    if value != UNAVAILABLE { put(INPUT + 0x80, value.0); }
}
pub(crate) unsafe fn f_63f47c(mode: CaptureModeId) {
    unsafe { apply(mode, PropertyKey(0x13d), INPUT + 0x94); }
    if word(INPUT + 0x94) == 1 { put(INPUT + 0xc0, 0x97); }
}
pub(crate) unsafe fn f_63f504(mode: CaptureModeId) {
    let key = PropertyKey(0x1c6);
    let candidate = SettingValue(unsafe { conversions::f_6464b0() });
    let value = if unsafe { available_count(mode, key) } <= 1 || word(FRAME + 0xbf54) == 0 {
        unsafe { preferred_value(mode, key) }
    } else { candidate };
    if value != UNAVAILABLE { byte(INPUT + 0x98, value.0 as u8); }
}
pub(crate) unsafe fn f_63f694(mode: CaptureModeId) {
    let value = unsafe { selected(mode, PropertyKey(0x1cd), SettingValue(conversions::f_6462d8())) };
    if value != UNAVAILABLE { byte(INPUT + 0xa8, value.0 as u8); }
}
pub(crate) unsafe fn f_63f6f8() {
    put(INPUT + 0x3b8, u32::from(unsafe { property(PropertyKey(0x1ec)) }.0 == 2));
}
pub(crate) unsafe fn f_63f720(mode: CaptureModeId) {
    let value = unsafe { apply(mode, PropertyKey(0x1e4), INPUT + 0xe8) };
    if word(INPUT + 0xc8) != 8 { put(INPUT + 0xe8, 0); }
    else if value.0 == 1 { put(INPUT + 0xe8, 1); }
    put(FRAME + 0xcf14, 0);
    if word(INPUT + 0xe8) == 1 { put(FRAME + 0xcf14, 1); }
}
pub(crate) unsafe fn f_63f7e0(_value: u32) {
    put(INPUT + 0xec, if word(INPUT + 0xe8) == 1 { 0x41 } else { 0 });
}
pub(crate) unsafe fn f_63f808(mode: CaptureModeId) {
    unsafe { apply(mode, PropertyKey(0x2b), INPUT + 0x10c); }
    if unsafe { property(PropertyKey(0x20d)) }.0 == 1 { put(INPUT + 0x10c, 0); }
}
pub(crate) unsafe fn f_63f890(mode: CaptureModeId) {
    let value = unsafe { selected(mode, PropertyKey(0x1a), SettingValue(conversions::f_6461a0())) };
    if value != UNAVAILABLE { put(INPUT + 0x114, value.0); }
    if unsafe { property(PropertyKey(0x20d)) }.0 == 1 { put(INPUT + 0x114, 0); }
}
pub(crate) unsafe fn f_63f8fc(mode: CaptureModeId) {
    let candidate = SettingValue(u32::from(unsafe { converted(PropertyKey(0x16)) }.0 != 0));
    let value = unsafe { selected(mode, PropertyKey(0x13), candidate) };
    if value != UNAVAILABLE { put(INPUT + 0x340, value.0); }
}
pub(crate) unsafe fn f_63f9d4(mode: CaptureModeId) {
    unsafe { apply(mode, PropertyKey(0x1c), INPUT + 0x120); }
    if word(INPUT + 0x64).wrapping_sub(1) <= 1 { put(INPUT + 0x120, 0); }
}
pub(crate) unsafe fn f_63fa60(mode: CaptureModeId) {
    let key = PropertyKey(0x1e5);
    let value = unsafe { selected(mode, key, converted(key)) };
    if value == UNAVAILABLE { return; }
    half(INPUT + 0x124, value.0 as u16);
    if unsafe { available_count(mode, key) } == 1 {
        let current = SettingValue(unsafe { read_volatile((INPUT + 0x124) as *const u16) } as u32);
        unsafe { save(key, project(key, current, 3, 1)); }
    }
}
pub(crate) unsafe fn f_63fb60(_mode: CaptureModeId) { put(INPUT + 0x15c, 0); }
pub(crate) unsafe fn f_63fcb0(_mode: CaptureModeId) { put(INPUT + 0x130, 0); }

pub(crate) unsafe fn f_63e8fc(mode: CaptureModeId) -> SettingValue {
    let candidate = match unsafe { crate::capture_initialization::f_63a428() } {
        0 => SettingValue(6),
        1 => SettingValue(unsafe { conversions::f_6460c0() }),
        _ => SettingValue(0),
    };
    let mut value = unsafe { selected(mode, PropertyKey(0x129), candidate) };
    if value == UNAVAILABLE { value = candidate; }
    if word(INPUT) == 0x2c {
        if word(INPUT + 8) == 1 {
            value = SettingValue(6);
            if word(INPUT + 0x148) == 1 { value = SettingValue(6); }
        } else if value.0 == 0 {
            if word(INPUT + 0x54).wrapping_sub(7) <= 2 { value = SettingValue(2); }
        } else if word(INPUT + 0x148) == 1 { value = SettingValue(6); }
    }
    if matches!(word(INPUT + 0x13c), 3..=6) { value = SettingValue(3); }
    if word(FRAME + 0x10778) == 1 {
        let key = PropertyKey(0x129);
        let default = SettingValue(unsafe { xm1_property_get(key.0, 0, 1, 0) });
        return unsafe { project(key, default, 1, 3) };
    }
    value
}

pub(crate) unsafe fn f_63ef60(mode: CaptureModeId, _unused: u32) {
    let mut first = unsafe { selected(mode, PropertyKey(0x141), SettingValue(conversions::f_6444a0(mode))) };
    let second = unsafe { selected(mode, PropertyKey(0x14b), SettingValue(conversions::f_643dec(mode))) };
    let mut use_alternate = true;
    if unsafe { available_count(mode, PropertyKey(0x141)) } <= 1 {
        if first.0 == 0 || unsafe { available_count(mode, PropertyKey(0x14f)) } <= 1 {
            use_alternate = false;
        } else {
            unsafe { conversions::f_643dc4(mode, 0); }
        }
    }
    if use_alternate && unsafe { conversions::f_643d94(mode) } == 0 {
        first = unsafe { selected(mode, PropertyKey(0x14f), SettingValue(conversions::f_643e58(mode))) };
    }
    if word(INPUT).wrapping_sub(0xc) <= 1 && word(INPUT + 0x90) == 1 {
        first = SettingValue(8);
    } else if first == UNAVAILABLE { return; }
    put(INPUT + 0x7c, first.0);
    put(INPUT + 0x33c, second.0);
}

pub(crate) unsafe fn f_63f28c(mode: CaptureModeId) {
    let key = PropertyKey(0x104);
    let mut value = SettingValue(unsafe { conversions::f_643f30(mode) });
    let option = word(INPUT + 0x350);
    if word(INPUT) == 0x20 && option <= 12 && (0x1024u32 & (1 << option)) != 0 && value.0.wrapping_sub(2) <= 1 {
        value = unsafe { preferred_value(mode, key) };
    }
    value = unsafe { selected(mode, key, value) };
    if value != UNAVAILABLE { put(INPUT + 0x84, value.0); }
}
pub(crate) unsafe fn f_63f408(mode: CaptureModeId) -> u32 {
    let result = u32::from(unsafe { available_count(mode, PropertyKey(0x1c5)) } > 1);
    if (word(INPUT) == 0xc && word(INPUT + 0x90) == 1) || word(INPUT) == 0x12 { 1 } else { result }
}
pub(crate) unsafe fn f_63f45c(mode: CaptureModeId) -> u32 {
    u32::from(unsafe { available_count(mode, PropertyKey(0x1c5)) } > 1)
}
pub(crate) unsafe fn f_63f568(mode: CaptureModeId) {
    let mut value = unsafe { selected(mode, PropertyKey(0x1cd), SettingValue(conversions::f_6462d8())) };
    if mode.raw() == 0x24 && word(INPUT + 0xc) != 0 && value.0 == u32::MAX { value = SettingValue(0xd); }
    let current_mode = word(INPUT);
    if current_mode == 0x2c { value = SettingValue(0); }
    else if current_mode == 0xc || current_mode == 0x12 {
        if current_mode == 0x12 || word(INPUT + 0x90) == 1 {
            put(INPUT + 0x9c, unsafe { conversions::f_6462d8() });
        }
        return;
    }
    if (current_mode == 0x24 || word(INPUT + 4) == 3) && word(INPUT + 0xc) == 0
        && unsafe { conversions::f_6462d8() } == u32::MAX {
        put(INPUT + 0x9c, u32::MAX);
        return;
    }
    if value != UNAVAILABLE { put(INPUT + 0x9c, value.0); }
}
pub(crate) unsafe fn f_63f6e4(request: u32, _mode: CaptureModeId) {
    put(INPUT + 0xfc, unsafe { crate::capture_initialization::f_63af44(request) });
}
pub(crate) unsafe fn f_63f968(mode: CaptureModeId) {
    unsafe { apply(mode, PropertyKey(0x16), INPUT + 0x34c); }
}
fn flag(raw: u32) -> u32 {
    unsafe { crate::mode_initialization::query_flag(crate::mode_initialization::FlagId::new(raw)) }
}
pub(crate) unsafe fn f_63fb74(mode: CaptureModeId) {
    let candidate = unsafe { conversions::f_646608() };
    let mut value = match word(INPUT) {
        13 => 1,
        12 if word(FRAME + 0xbf94) != 1 => 0,
        12 if word(FRAME + 0xbf9c) != 1 => 1,
        12 => u32::from(flag(0x423) != 0),
        _ => candidate,
    };
    if word(INPUT + 0x4c) == 7 || word(INPUT + 0x7c) > 5 { value = 0; }
    let value = unsafe { selected(mode, PropertyKey(0xc), SettingValue(value)) };
    if value != UNAVAILABLE { put(INPUT + 0x12c, value.0); }
    if word(FRAME + 0xbf94) == 1 {
        if word(FRAME + 0xbf9c) != 1 { return; }
        if flag(0x423) != 0 { return; }
    }
    if word(INPUT) != 1 { put(INPUT + 0x12c, 0); }
}
pub(crate) unsafe fn f_63fdd0(mode: CaptureModeId) {
    let key = PropertyKey(0x13a);
    unsafe { apply(mode, key, INPUT + 0x13c); }
    if word(FRAME + 0xcfe4) != word(FRAME + 0xcfe8)
        || word(FRAME + 0xcfc0) != mode.raw() || word(FRAME + 0xcf24) == 1 {
        if matches!(word(INPUT + 0x13c), 3..=6) {
            put(INPUT + 0x13c, 0);
            unsafe { save(key, project(key, SettingValue(word(INPUT + 0x13c)), 3, 1)); }
        }
    }
    if word(INPUT) == 0x2c { put(INPUT + 0x14c, 1); }
    put(FRAME + 0xcfb4, 0);
}
