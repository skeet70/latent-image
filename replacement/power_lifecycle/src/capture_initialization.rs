use crate::capture_initialization_conversions as conversions;
use crate::capture_initialization_helpers_a as a;
use crate::capture_initialization_helpers_b as b;
use crate::capture_initialization_helpers_c as c;
use crate::capture_initialization_state::*;
use crate::capture_initialization_state_helpers as state;
use crate::mode_initialization::{CaptureModeId, CaptureSubtype, FlagId};

/// The capture, view, property and snapshot windows and owned device services must be available.
pub(crate) unsafe fn initialize_capture(mode: CaptureModeId, subtype: CaptureSubtype) {
    unsafe {
        write_word(FRAME + 0xda64, 1);
        write_word(FRAME + 0xcfe8, read_word(FRAME + 0xcfe4));
        write_word(FRAME + 0xcfe4, subtype.raw());
        refresh_capture(mode);
        write_word(FRAME + 0xda64, 0);
    }
}

pub(crate) unsafe fn refresh_capture(mode: CaptureModeId) {
    unsafe {
        write_word(FRAME + 0xcfd0, 0);
        write_word(FRAME + 0xcfd4, 0);
        write_word(FRAME + 0xcfd8, 1);
        write_word(FRAME + 0xcfdc, 128);
        initialize_transition_snapshot();
        initialize_view_flags();
        write_word(FRAME + 0xd024, 0);
        apply_capture_settings(mode, 1);
        write_word(FRAME + 0xcfc0, read_word(INPUT));
        let menu = if read_word(FRAME + 0xbe20).wrapping_sub(4) <= 1 { 165 } else { 164 };
        f_63b480(menu);
        initialize_groups(read_word(INPUT + 0x174) as usize);
        fill_bytes(0x409b_0b10, 0xff, 0x44c0);
        copy_cached_settings(0x0095_a6b4, INPUT);
        reset_capture_state();
        copy_bytes(0x0089_ec00, 0x0096_cc64, 0x1c2);
        copy_bytes(0x0095_1c64, 0x0095_15ac, 860);
        copy_bytes(0x0096_b58c, 0x0095_71d0, 1072);
    }
}

pub(crate) unsafe fn apply_capture_settings(mode: CaptureModeId, refresh: u32) {
    unsafe {
        write_word(INPUT, mode.raw());
        b::f_6427bc();
        c::f_63ba0c(refresh);
        c::f_63ba5c(mode);
        conversions::f_646924();
        c::f_63c054();
        write_word(FRAME + 0xcff8, 0);
        if !(read_word(INPUT) == 12 && read_word(INPUT + 0x90) == 1) && read_word(INPUT) != 18 {
            write_word(FRAME + 0xcfec, 0);
        }
        if read_word(FRAME + 0xcfe4) != read_word(FRAME + 0xcfe8)
            || read_word(FRAME + 0xcfc0) != mode.raw() || read_word(FRAME + 0xcf24) == 1 {
            f_63d5fc();
        }
        c::f_63bc84();
        if read_word(FRAME + 0xbf88) != 0 {
            c::f_63bd20();
            c::f_63bd34();
            c::f_63bd78();
            c::f_63be0c();
        }
        a::f_63e4cc(mode);
        a::f_63f6e4(0, mode);
        a::f_63f6f8();
        a::f_63fdd0(mode);
        write_word(FRAME + 0xcfc4, read_word(FRAME + 0xcfc0));
        write_word(FRAME + 0xcfc0, mode.raw());
        write_word(INPUT + 0x14c, u32::from(read_word(INPUT + 0x13c) == 0));
        a::f_63e430(mode);
        b::f_6402b8(mode);
        b::f_640308(mode);
        b::f_640358(mode);
        b::f_6403a8(mode);
        b::f_640414(mode);
        b::f_640480(mode);
        b::f_6404ec(mode);
        a::f_63e5cc(mode);
        a::f_63e638(mode);
        a::f_63e708(mode);
        a::f_63e774();
        a::f_63e7a4(mode);
        a::f_63e810(mode, read_word(INPUT + 0x2c));
        b::f_6411e0(mode);
        a::f_63ea30(mode);
        a::f_63ebcc(mode);
        a::f_63eb60(mode);
        a::f_63ed44(mode);
        a::f_63edb0(mode);
        a::f_63ec6c(mode);
        a::f_63ecd8(mode);
        a::f_63ee1c(mode);
        a::f_63f0a0(mode);
        a::f_63f148(mode);
        a::f_63f1b4(mode);
        a::f_63f220(mode);
        a::f_63ee88(mode);
        a::f_63eef4(mode);
        a::f_63ef60(mode, 1);
        a::f_63f28c(mode);
        a::f_63f330(mode);
        a::f_63f39c(mode);
        a::f_63f47c(mode);
        a::f_63f504(mode);
        a::f_63f568(mode);
        a::f_63f694(mode);
        a::f_63f808(mode);
        a::f_63f890(mode);
        a::f_63f8fc(mode);
        a::f_63f968(mode);
        a::f_63f9d4(mode);
        a::f_63fa60(mode);
        a::f_63fb10(mode);
        a::f_63fb60(mode);
        a::f_63fcc4(mode);
        a::f_63fd30(mode);
        a::f_63fd80(mode);
        b::f_6433f0(mode);
        a::f_63fb74(mode);
        a::f_63ff08(mode);
        a::f_63ff74(mode);
        a::f_63ffe0(mode);
        a::f_64004c(mode);
        c::f_63c068(mode);
        c::f_63c108(mode);
        a::f_63ec1c(CaptureModeId::new(44));
        a::f_63ea80(mode);
        a::f_63ead0(mode);
        c::f_63c170(mode);
        c::f_63c1dc(mode);
        b::f_640920(mode);
        state::f_63d688(mode);
        b::f_641428(refresh);
        b::f_640e78(0, mode);
        a::f_63f720(mode);
        a::f_63f7e0(mode.raw());
        b::f_6424cc(refresh);
        b::f_6409a0(mode);
        b::f_640a0c(mode);
        b::f_640a78(mode);
        b::f_640ae4(mode);
        b::f_640c28(mode);
        b::f_640c94(mode);
        b::f_640b50(mode);
        b::f_640bbc(mode);
        b::f_6425ec(mode);
        write_byte(FRAME + 0xda44, 0);
        write_byte(FRAME + 0xda46, 0);
        write_word(FRAME + 0xe3d8, 0);
    }
}

pub(crate) unsafe fn initialize_view_flags() {
    const VIEW: usize = 0x0094_32a0;
    unsafe {
        write_word(VIEW + 0x164, 0x0093_c0ec);
        write_word(VIEW + 0x1d4, 0);
        write_byte(VIEW + 0x154, 64);
        write_byte(VIEW + 0x155, 13);
        for offset in (8..=0xc0).step_by(4) { write_word(VIEW + offset, 9999); }
        let value = conversions::f_646924();
        write_word(VIEW + 0x1d0, if value == 2 { 4 } else { 2 });
        write_word(VIEW + 0x948, 0x409b_0b10);
    }
}

pub(crate) unsafe fn initialize_transition_snapshot() {
    const TRANSITION: usize = 0x0095_b3a0;
    const CONFIGURATION: usize = 0x0096_b400;
    unsafe {
        let previous = read_word(FRAME + 0xbb70);
        let token = match previous { 3 => 231, 4 => 232, 5 | 6 => 233, 7 => 237, 12 => 253, 13 | 14 => 230, _ => 229 };
        write_word(TRANSITION, token);
        if read_word(FRAME + 0xbebc) == 1 && read_word(FRAME + 0xbb70).wrapping_sub(1) <= 1 {
            write_half(TRANSITION + 8, read_half(CONFIGURATION + 20));
            write_word(TRANSITION + 4, 2);
        } else {
            write_half(TRANSITION + 8, 0);
            write_word(TRANSITION + 4, 1);
        }
        write_word(TRANSITION + 12, u32::from(read_word(FRAME + 0xbed0) == 1));
        write_word(TRANSITION + 20, u32::from(read_word(FRAME + 0xbfa4) == 1 && flag(0x426) == 1));
        let value = read_half(FRAME + 0xb354);
        write_word(TRANSITION + 16, u32::from(value));
        let invert = read_word(FRAME + 0xbb70) == 5 && read_word(FRAME + 0xbb74) == 3;
        write_half(FRAME + 0xbfb8, if invert { !read_half(FRAME + 0xb354) } else { read_half(FRAME + 0xb354) });
        write_word(FRAME + 0xbfbc, u32::from(invert));
        write_byte(TRANSITION + 0x18, 1);
        write_word(TRANSITION + 0x1c, 0);
        write_byte(TRANSITION + 0x20, 5);
        write_byte(TRANSITION + 0x21, 3);
        write_byte(TRANSITION + 0x22, read_byte(CONFIGURATION + 0x3a));
        write_byte(TRANSITION + 0x23, 23);
        write_byte(TRANSITION + 0x24, 0);
        write_byte(TRANSITION + 0x25, read_byte(CONFIGURATION + 0x3a));
        write_byte(TRANSITION + 0x26, read_byte(CONFIGURATION + 0x39));
        write_word(TRANSITION + 0x28, 30);
        let first = u32::from(read_byte(CONFIGURATION + 0x66));
        write_word(TRANSITION + 0x2c, first * 100);
        let second = u32::from(read_byte(CONFIGURATION + 0x67));
        write_word(TRANSITION + 0x30, second * 100);
        let third = u32::from(read_byte(CONFIGURATION + 0x68));
        write_word(TRANSITION + 0x34, third * 100);
        for (destination, source) in [(0x38, 0x6c), (0x3c, 0x6d), (0x40, 0x6e), (0x44, 0x6f), (0x48, 0x70), (0x4c, 0x71), (0x50, 0x72), (0x54, 0x73), (0x58, 0x74), (0x5c, 0x75)] {
            write_word(TRANSITION + destination, u32::from(read_byte(CONFIGURATION + source)) * 100);
        }
        write_word(TRANSITION + 0x60, second * 100);
        write_word(TRANSITION + 0x64, third.wrapping_sub(second).wrapping_mul(100));
        if a::property(a::PropertyKey(632)).0 == 0 {
            write_word(TRANSITION + 0x68, 0);
            return;
        }
        write_word(TRANSITION + 0x68, 1);
        write_word(TRANSITION + 0x78, a::property(a::PropertyKey(632)).0);
        write_byte(TRANSITION + 0x7c, 17);
        write_half(TRANSITION + 0x80, a::property(a::PropertyKey(635)).0 as u16);
        write_half(TRANSITION + 0x82, a::property(a::PropertyKey(636)).0 as u16);
        write_word(TRANSITION + 0x88, u32::from(a::property(a::PropertyKey(8)).0 != 1));
    }
}

pub(crate) unsafe fn initialize_groups(destination: usize) {
    unsafe {
        for group in 0..16 {
            let row = destination + group * 120;
            write_word(row + 8, 0);
            write_half(row + 12, 0);
            write_word(row + 0x7c, 0);
            write_half(row + 0x60, 0);
            write_word(row + 0x64, 0);
            for index in 0..10 {
                write_word(row + 0x10 + index * 4, u32::MAX);
                write_word(row + 0x38 + index * 4, 0);
                write_half(row + 0x68 + index * 2, 0);
            }
        }
        write_byte(destination, 0);
        write_byte(destination + 1, 0);
        write_word(destination + 4, read_word(0x0094_32a0 + 0x948));
    }
}

pub(crate) unsafe fn copy_cached_settings(destination: usize, source: usize) {
    unsafe {
        let destination_groups = read_word(destination + 0x174);
        let source_groups = read_word(source + 0x174);
        copy_bytes(destination, source, 1000);
        write_word(destination + 0x174, destination_groups);
        write_word(source + 0x174, source_groups);
        copy_bytes(read_word(destination + 0x174) as usize, source_groups as usize, 0x788);
    }
}

pub(crate) unsafe fn reset_capture_state() {
    unsafe {
        write_byte(FRAME + 0xcf00, 0);
        for offset in [0xcf04, 0xcf08, 0xcf18, 0xcf1c, 0xcf84, 0xcf88, 0xcf8c, 0xcf90, 0xcf94] { write_word(FRAME + offset, 0); }
        write_word(FRAME + 0xcf98, 1);
        write_word(FRAME + 0xcf9c, 0);
        write_word(FRAME + 0xcfa0, 129);
        for offset in [0xcfa8, 0xcfac, 0xcfc8, 0xcfcc, 0xcfe0, 0xcffc, 0xd004, 0xd00c, 0xd010, 0xda4c, 0xda6c, 0xcf38, 0xcf34, 0xd014, 0xda58, 0xda5c, 0xda60, 0xcf24, 0xcf28, 0xcf2c] { write_word(FRAME + offset, 0); }
        write_byte(FRAME + 0xdaa0, 1);
        write_word(FRAME + 0xcfa4, 129);
        for offset in [0xe410, 0xda80, 0xfd18, 0xe3e0, 0x10784, 0xff80, 0x107ec, 0xff8c] { write_word(FRAME + offset, 0); }
        write_byte(FRAME + 0xff94, 0);
        write_byte(FRAME + 0xff95, 0);
        for offset in [0xcf30, 0xcf44, 0xcf48, 0xcf4c, 0xcf50, 0xcf58, 0xcf5c, 0xcf54, 0xcf60, 0xcf78, 0xe3ec] { write_word(FRAME + offset, 0); }
        fill_bytes(0x0095_0544, 0, 0xf3c);
        c::f_63c51c(1, 0, 0);
    }
}

pub(crate) unsafe fn flag(raw: u32) -> u32 { unsafe { crate::mode_initialization::query_flag(FlagId::new(raw)) } }

pub(crate) unsafe fn f_63b480(value: u32) {
    unsafe {
        let active = value == 165 || f_63249c(27) == 1;
        write_word(INPUT + 0xbc, if active { 165 } else { 164 });
        write_word(INPUT + 0xe0, u32::from(active));
    }
}

pub(crate) unsafe fn f_63249c(id: u32) -> u32 {
    let quotient = id / 63;
    let bit = (id + quotient) & 63;
    u32::from(quotient < 2 && unsafe { read_word(0x0095_0118 + quotient as usize * 8 + (bit / 32) as usize * 4) } & (1 << (bit % 32)) != 0)
}

pub(crate) unsafe fn f_63d3bc(value: u32) -> u32 {
    unsafe {
        let selector = read_word(INPUT + 0x80);
        let offset = if matches!(selector, 4 | 5 | 7 | 8 | 9 | 10) || read_word(INPUT + 0x148) == 0 { 0 }
        else if (read_word(INPUT + 0x64) == 1 || matches!(read_word(INPUT), 31 | 32)) && read_word(INPUT + 0x148) == 1 { 11 }
        else { read_word(INPUT + 0x154).wrapping_mul(11) };
        read_word(0x007a_12bc + value.wrapping_add(offset) as usize * 4)
    }
}

pub(crate) unsafe fn f_63d498() {
    unsafe {
        if read_word(INPUT + 0x148) == 1 || (read_word(INPUT) == 15 && read_word(INPUT + 0x20) == 1) { write_word(INPUT + 0xf8, 0); }
    }
}
pub(crate) unsafe fn f_63d4f4(save: u32) {
    unsafe {
        write_word(INPUT + 0x10, 0);
        if save == 1 {
            let value = a::project(a::PropertyKey(473), a::SettingValue(read_word(INPUT + 0x10)), 3, 1);
            a::save(a::PropertyKey(473), value);
        }
    }
}
pub(crate) unsafe fn f_63d564(save: u32) {
    unsafe {
        write_word(INPUT + 0xc, 0);
        if save != 1 { return; }
        let backup = a::project(a::PropertyKey(29), conversions::property_bank(a::PropertyKey(29), 1), 1, 3);
        let current = a::project(a::PropertyKey(29), a::property(a::PropertyKey(29)), 1, 3);
        if backup == current {
            let value = a::project(a::PropertyKey(433), a::SettingValue(read_word(INPUT + 0xc)), 3, 1);
            a::save(a::PropertyKey(433), value);
        }
    }
}
pub(crate) unsafe fn f_63d5fc() {
    unsafe {
        write_byte(INPUT + 0x98, 0);
        let value = (read_byte(INPUT + 0x98) as i8 as i32).wrapping_add(127) as u32;
        a::save(a::PropertyKey(454), a::SettingValue(value));
    }
}
pub(crate) unsafe fn f_63d614(value: u32) {
    unsafe {
        if value == 0 && read_word(INPUT + 0x14) != 6 && matches!(read_word(INPUT), 31 | 32 | 33) {
            f_63d5fc();
            write_byte(INPUT + 0x480, read_byte(INPUT + 0x98));
        }
        write_word(INPUT + 0x20, value);
        write_word(INPUT + 0x408, read_word(INPUT + 0x20));
    }
}
pub(crate) unsafe fn f_63db24(save: u32) {
    unsafe {
        write_half(INPUT + 0x124, 0);
        if save == 1 {
            let value = a::project(a::PropertyKey(485), a::SettingValue(u32::from(read_half(INPUT + 0x124))), 3, 1);
            a::save(a::PropertyKey(485), value);
        }
    }
}

#[cfg(test)]
#[path = "capture_initialization_tests.rs"]
mod tests;

pub(crate) unsafe fn f_6326b8() -> u32 {
    u32::from(matches!(unsafe { read_word(FRAME + 0xbb6c) }, 4 | 6))
}

pub(crate) unsafe fn f_632f84(output: *mut u32) -> u32 {
    let status = unsafe { state::f_72b774(output) };
    unsafe {
        if status == 0 { write_word(0x0089_edd0, core::ptr::read(output)); }
        else { core::ptr::write(output, read_word(0x0089_edd0)); }
    }
    status
}

pub(crate) unsafe fn f_63a428() -> u32 {
    if unsafe { read_word(FRAME + 0xbfa4) == 1 && flag(0x426) == 1 } { 2 } else { 1 }
}

unsafe fn view_mode_available() -> bool {
    unsafe {
        if read_word(INPUT + 0x100) == 1 || read_word(INPUT + 0x10) != 0 || read_byte(INPUT + 0x338) != 0 { return false; }
        let mode = CaptureModeId::new(read_word(INPUT));
        if !a::value_available(mode, a::PropertyKey(492), a::SettingValue(2)) { return false; }
        if read_word(INPUT + 0x334) == 1 || matches!(read_word(FRAME + 0xcfe0), 4 | 7)
            || read_word(INPUT + 0x34c) != 0 || read_word(FRAME + 0xff88) == 1
            || read_word(INPUT + 0x4c) == 7 || read_word(INPUT + 0x398) == 1
            || read_word(FRAME + 0xbe20).wrapping_sub(4) <= 1 { return false; }
        if view_blocked() { return false; }
        if read_word(FRAME + 0xcf4c) == 1 && read_word(FRAME + 0xcf5c) != 1 { return false; }
        if conversions::f_61d018() == 1 {
            conversions::f_61d154() == 10 || conversions::f_61d154() == 21
        } else { read_word(FRAME + 0xe410) != 1 }
    }
}

unsafe fn category_bit(raw: u32) -> bool {
    let offset = match raw & 0xf000 { 0x1000 => 0xb308, 0x2000 => 0xb310, _ => return false };
    let bit = (raw >> 6) & 63;
    unsafe { read_word(FRAME + offset + (bit / 32) as usize * 4) & (1 << (bit % 32)) != 0 }
}

unsafe fn view_blocked() -> bool {
    unsafe {
        if read_word(FRAME + 0xff70) != 0 || category_bit(0x1240) { return true; }
        let mask = (1u64 << ((0x2380 >> 6) & 63)) | (1u64 << ((0x20c0 >> 6) & 63));
        let bits = u64::from(read_word(FRAME + 0xb310)) | (u64::from(read_word(FRAME + 0xb314)) << 32);
        if bits & !mask != 0 { return true; }
        if read_word(FRAME + 0xcf44) == 1 { return false; }
        category_bit(0x2380) || category_bit(0x20c0)
    }
}

pub(crate) unsafe fn f_63af44(request: u32) -> u32 {
    unsafe {
        let configured = a::property(a::PropertyKey(492)).0;
        let mut value = read_word(INPUT + 0xfc);
        'selection: {
            'reconcile: {
                match request {
                    1 => {
                        if a::property(a::PropertyKey(492)).0 == 0 {
                            if flag(51) != 1 { value = 0; break 'selection; }
                            if a::property(a::PropertyKey(481)).0 != 1 { value = 1; break 'selection; }
                        } else {
                            if a::property(a::PropertyKey(492)).0 == 1 { value = 0; break 'selection; }
                            if a::property(a::PropertyKey(481)).0 != 1 { value = 1; break 'selection; }
                        }
                    }
                    2 => { if value != 1 { value = 1; break 'selection; } }
                    4 => {
                        if value != 2 { break 'selection; }
                        if read_word(INPUT + 0x4c) == 0 { value = 1; break 'selection; }
                    }
                    6 => {
                        if value == 1 {
                            if a::property(a::PropertyKey(481)).0 != 1 { break 'selection; }
                        } else if value != 2 { break 'selection; }
                    }
                    7 => { value = 0; break 'selection; }
                    _ => {
                        if configured == 1 {
                            if a::property(a::PropertyKey(481)).0 != 1 { value = 1; break 'selection; }
                        } else if configured == 2 {
                            if flag(51) != 1 { value = 0; break 'selection; }
                            if a::property(a::PropertyKey(481)).0 != 1 { value = 1; break 'selection; }
                        } else {
                            value = 0;
                            break 'reconcile;
                        }
                    }
                }
                if read_word(FRAME + 0xbe20).wrapping_sub(4) <= 1 {
                    value = if configured == 2 || read_word(INPUT + 0x10) == 1 { 1 }
                        else { u32::from(!a::value_available(CaptureModeId::new(read_word(INPUT)), a::PropertyKey(492), a::SettingValue(2))) };
                } else if !view_mode_available() { value = 1; }
                else if read_word(FRAME + 0xbe20) == 7 {
                    value = if configured == 1 || read_word(INPUT + 0x10) == 1 { 1 }
                        else { u32::from(!a::value_available(CaptureModeId::new(read_word(INPUT)), a::PropertyKey(492), a::SettingValue(2))) };
                } else { value = 2; }
            }
        }
        if value <= 1 { value = 0; }
        let mode = CaptureModeId::new(read_word(INPUT));
        if !a::value_available(mode, a::PropertyKey(492), a::SettingValue(value)) {
            value = a::preferred_value(CaptureModeId::new(read_word(INPUT)), a::PropertyKey(492)).0;
        }
        if value == 0x7fff_ffff {
            value = a::project(a::PropertyKey(492), a::property(a::PropertyKey(492)), 1, 3).0;
        }
        value
    }
}
