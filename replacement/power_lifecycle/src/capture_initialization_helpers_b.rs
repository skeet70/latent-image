use crate::mode_initialization::CaptureModeId;
use crate::capture_initialization_helpers_a::{self as a, PropertyKey, SettingValue};
use crate::capture_initialization_helpers_c as c;
use crate::capture_initialization_state::*;
use crate::capture_initialization_state_helpers as lists;
use crate::capture_initialization as cpu;
use crate::capture_initialization_conversions as conv;

#[path = "capture_initialization_defaults.rs"]
mod defaults;
pub(crate) use defaults::{f_6426bc, f_6427bc};

unsafe fn cached_mode() -> CaptureModeId { CaptureModeId::new(unsafe { read_word(INPUT) }) }
unsafe fn get(key: u32) -> u32 { unsafe { a::property(PropertyKey(key)).0 } }
unsafe fn projected(key: u32) -> u32 { unsafe { a::project(PropertyKey(key), a::property(PropertyKey(key)), 1, 3).0 } }
unsafe fn selected(mode: CaptureModeId, key: u32, value: u32) -> u32 {
    unsafe {
        if a::value_available(mode, PropertyKey(key), SettingValue(value)) { value }
        else { a::preferred_value(mode, PropertyKey(key)).0 }
    }
}
unsafe fn input(offset: usize) -> u32 { unsafe { read_word(INPUT + offset) } }
unsafe fn frame(offset: usize) -> u32 { unsafe { read_word(FRAME + offset) } }

pub(crate) unsafe fn f_6402b8(mode: CaptureModeId) {
    unsafe {
        let value = selected(mode, 0x16c, conv::f_646798());
        if value != 0x7fff_ffff { write_word(INPUT + 0x304, (value)); }
    }
}

pub(crate) unsafe fn f_640308(mode: CaptureModeId) {
    unsafe {
        let value = selected(mode, 0x48, conv::f_6467c4());
        if value != 0x7fff_ffff { write_word(INPUT + 0x2f0, (value)); }
    }
}

pub(crate) unsafe fn f_640358(mode: CaptureModeId) {
    unsafe {
        let value = selected(mode, 0x4b, conv::f_6467f0());
        if value != 0x7fff_ffff { write_word(INPUT + 0x2f4, (value)); }
    }
}

pub(crate) unsafe fn f_6403a8(mode: CaptureModeId) {
    unsafe {
        let value = selected(mode, 0x4e, projected(0x4e));
        if value != 0x7fff_ffff { write_word(INPUT + 0x380, (value)); }
    }
}

pub(crate) unsafe fn f_640414(mode: CaptureModeId) {
    unsafe {
        let value = selected(mode, 0xad, projected(0xad));
        if value != 0x7fff_ffff { write_word(INPUT + 0x378, (value)); }
    }
}

pub(crate) unsafe fn f_640480(mode: CaptureModeId) {
    unsafe {
        let value = selected(mode, 0x17a, projected(0x17a));
        if value != 0x7fff_ffff { write_word(INPUT + 0x390, (value)); }
    }
}

pub(crate) unsafe fn f_6404ec(mode: CaptureModeId) {
    unsafe {
        let value = selected(mode, 0x17b, projected(0x17b));
        if value != 0x7fff_ffff { write_word(INPUT + 0x394, (value)); }
    }
}

pub(crate) unsafe fn f_640920(mode: CaptureModeId) {
    unsafe {
        if input(0xf8) == 1 { return; }
        let value = selected(mode, 0xd3, projected(0xd3));
        if value != 0x7fff_ffff { write_word(INPUT + 0xa4, (value)); }
    }
}

pub(crate) unsafe fn f_6409a0(mode: CaptureModeId) {
    unsafe {
        let value = selected(mode, 0xe6, projected(0xe6));
        if value != 0x7fff_ffff { write_word(INPUT + 0x350, (value)); }
    }
}

pub(crate) unsafe fn f_640a0c(mode: CaptureModeId) {
    unsafe {
        let value = selected(mode, 0xf, projected(0xf));
        if value != 0x7fff_ffff { write_word(INPUT + 0x158, (value)); }
    }
}

pub(crate) unsafe fn f_640a78(mode: CaptureModeId) {
    unsafe {
        let value = selected(mode, 0x10, projected(0x10));
        if value != 0x7fff_ffff { write_word(INPUT + 0x168, (value)); }
    }
}

pub(crate) unsafe fn f_640ae4(mode: CaptureModeId) {
    unsafe {
        let value = selected(mode, 0x11, projected(0x11));
        if value != 0x7fff_ffff { write_word(INPUT + 0x16c, (value)); }
    }
}

pub(crate) unsafe fn f_640b50(mode: CaptureModeId) {
    unsafe {
        let value = selected(mode, 0x20d, projected(0x20d));
        if value != 0x7fff_ffff { write_word(INPUT + 0x38c, (value)); }
    }
}

pub(crate) unsafe fn f_640bbc(mode: CaptureModeId) {
    unsafe {
        let value = selected(mode, 0xd5, projected(0xd5));
        if value != 0x7fff_ffff { write_word(INPUT + 0x3b4, (value)); }
    }
}

pub(crate) unsafe fn f_640c28(mode: CaptureModeId) {
    unsafe {
        let value = selected(mode, 0x12, projected(0x12));
        if value != 0x7fff_ffff { write_word(INPUT + 0x170, (value)); }
    }
}

pub(crate) unsafe fn f_640c94(mode: CaptureModeId) {
    unsafe {
        let value = selected(mode, 0x2c, projected(0x2c));
        if value != 0x7fff_ffff { write_word(INPUT + 0x110, (value)); }
    }
}

pub(crate) unsafe fn f_640d00(mode: CaptureModeId) {
    unsafe {
        let value = selected(mode, 0x1dd, projected(0x1dd));
        let value = if input(0x4c) != 2 || input(0xfc) == 2 { 2 } else { value };
        if value != 0x7fff_ffff { write_word(INPUT + 0xb8, value); }
    }
}

unsafe fn signed_adjustment(mode: CaptureModeId, key: PropertyKey) -> u8 {
    unsafe {
        let value = (a::property(key).0.wrapping_sub(3) as u8 as i8) as u32;
        let value = if a::value_available(mode, key, SettingValue(value)) {
            value
        } else {
            a::preferred_value(mode, key).0.wrapping_sub(3)
        };
        if input(0x390) == 1 { 0 } else { value as u8 }
    }
}

pub(crate) unsafe fn f_640da0(mode: CaptureModeId) {
    unsafe { write_byte(INPUT + 0x339, signed_adjustment(mode, PropertyKey(0x1de))); }
}

pub(crate) unsafe fn f_640e0c(mode: CaptureModeId) {
    unsafe { write_byte(INPUT + 0x33a, signed_adjustment(mode, PropertyKey(0x1df))); }
}

pub(crate) unsafe fn f_640e78(reason: u32, mode: CaptureModeId) {
    unsafe {
        let key = match input(0xfc) { 2 => 0x1e3, 1 => 0x1e2, _ => 0x1d8 };
        let value = selected(mode, key, projected(key));
        if value != 0x7fff_ffff { write_word(INPUT + 0xc8, value); }
        if input(0xc8) == 5 {
            let blocked = input(0x34c) != 0 || input(0x4c) == 7 || input(0x10) != 0;
            if blocked || (input(0x100) == 0 && input(0xfc) == 1) {
                write_word(INPUT + 0xc8, 0);
            } else if read_byte(INPUT + 0x338) == 0 {
                if reason == 4 || (reason != 5 && frame(0xcfe0).wrapping_sub(3) < 2) {
                    write_word(INPUT + 0xc8, 9);
                }
            } else if input(0x4c) == 2 {
                write_byte(INPUT + 0x338, 0);
            } else if input(0x4c) == 0 {
                write_word(INPUT + 0xc8, 9);
            }
        }
        if conv::f_61d018() == 1 && input(0xc8) == 5
            && (conv::f_61d1f4() == 0xfe || conv::f_61d1f4() == 0xed
                || conv::f_61d1f4() == 0xee || conv::f_61d154() == 0xc)
            && input(0xfc) == 0
        {
            write_word(INPUT + 0xc8, 9);
        }
        if input(0xc8) == 1 {
            if input(0x4c) != 7 && input(0xc) != 7 { return; }
            write_word(INPUT + 0xc8, 0);
        }
        if !matches!(input(0xc8), 9 | 5) || input(0x334) != 1 { return; }
        write_word(INPUT + 0x334, 0);
        f_640480(cached_mode());
        f_640da0(cached_mode());
        f_640e0c(cached_mode());
    }
}

pub(crate) unsafe fn f_6411e0(mode: CaptureModeId) {
    unsafe {
        write_word(INPUT + 0x4c, a::f_63e8fc(mode).0);
        if input(0x4c).wrapping_sub(4) < 3 && frame(0xd2a4) == 1 {
            c::f_63be24(cached_mode());
            lists::f_63717c(0, 0);
            lists::f_637264();
        }
        if input(0x4c) == 2 {
            write_byte(INPUT + 0xb6, get(0x1db).wrapping_sub(3) as u8);
            write_byte(INPUT + 0xb7, get(0x1dc).wrapping_sub(3) as u8);
            if input(0xfc) == 2 { c::f_63b2c0(); }
            if a::available_count(mode, PropertyKey(0x1db)) < 2 {
                write_byte(INPUT + 0xb6, 0);
                write_byte(INPUT + 0xb7, 0);
            }
        }
        if (input(0x148) == 1 || lists::f_646550() == 1) && input(0x4c) == 2 {
            write_byte(INPUT + 0xb6, 0);
            write_byte(INPUT + 0xb7, 0);
        }
        a::f_63fb74(cached_mode());
        f_640da0(cached_mode());
        f_640e0c(cached_mode());
        f_640d00(mode);
    }
}

pub(crate) unsafe fn f_641334() {
    unsafe {
        c::f_63be24(cached_mode());
        if (input(0x4c) == 0 && !matches!(input(0), 0x18 | 1 | 0x1b | 0x1c))
            || input(0x4c) == 7
        {
            write_word(INPUT + 0x148, 0);
        } else if matches!(input(0x13c), 3..=6) || (input(8) == 1 && input(0) == 0x2c) {
            write_word(INPUT + 0x148, 1);
        }
        cpu::f_63d498();
        a::f_63e638(cached_mode());
        f_6411e0(cached_mode());
        lists::f_63d688(cached_mode());
        crate::mode_initialization_record::write_initial_setting_record();
    }
}

pub(crate) unsafe fn f_641428(_refresh: u32) { unsafe { f_641334(); } }

pub(crate) unsafe fn f_6424cc(refresh: u32) {
    unsafe {
        if refresh == 1 && read_byte(FRAME + 0xd375) != 1 { c::f_63c51c(1, 0, 0); }
        f_641438(conv::f_6376a0(cached_mode()), refresh);
    }
}

pub(crate) unsafe fn f_6425ec(mode: CaptureModeId) {
    unsafe {
        let value = selected(mode, 0x153, conv::f_643ec4(mode));
        if value != 0x7fff_ffff { write_word(INPUT + 0xa0, value); }
    }
}

pub(crate) unsafe fn f_643368() {
    unsafe {
        let value = if frame(0xbf88) == 0 && frame(0xbf94) == 1 {
            if frame(0xbf9c) != 1 || cpu::flag(0x423) == 1 { 1 } else { 0 }
        } else { 0 };
        write_word(INPUT + 0x3dc, value);
    }
}

pub(crate) unsafe fn f_6433f0(mode: CaptureModeId) {
    unsafe {
        let mut value = lists::f_6465a4();
        if frame(0xbf94) == 1 {
            if frame(0xbf9c) == 1 {
                if cpu::flag(0x423) == 1 {
                    if value == 0 { value = 1; }
                } else { value = 0; }
            } else if input(0) == 0xc { value = 0; }
        } else { value = 0; }
        value = selected(mode, 0xb, value);
        if value != 0x7fff_ffff { write_word(INPUT + 0x128, value); }
        if frame(0xbf94) != 1 || (frame(0xbf9c) == 1 && cpu::flag(0x423) == 0) {
            write_word(INPUT + 0x128, 0);
        }
        f_643368();
    }
}

pub(crate) unsafe fn f_6400b8(count: u32, rate: u32) -> u32 {
    unsafe {
        let mut row = 0x007a_0ab0;
        loop {
            let code = read_word(row);
            if code == 6 { return 6; }
            if read_word(row + 4) == count && read_word(row + 8) == rate { return code; }
            row += 12;
        }
    }
}

pub(crate) unsafe fn f_640118() -> u32 {
    unsafe {
        if !matches!(input(0xc), 7 | 1 | 3) || input(0) == 15 { return 0; }
        u32::from(f_6400b8(input(0x2fc), input(0x2f8)).wrapping_sub(2) <= 3)
    }
}

pub(crate) unsafe fn f_640188() -> u32 {
    unsafe {
        if input(0xf8) == 1 || input(0x64).wrapping_sub(1) < 2 || input(0xc) != 0
            || input(0x94) == 1 || matches!(input(0x84), 4 | 5)
            || cpu::f_63249c(0x18) == 1 || cpu::f_63249c(0x17) == 1
            || cpu::f_63249c(0x1b) == 1
            || a::available_count(cached_mode(), PropertyKey(0x1e6)) < 2
            || f_640118() != 0 || input(0xf0) == 1
        { return 0; }
        1u32.saturating_sub(read_byte(INPUT + 0x338) as u32)
    }
}

pub(crate) unsafe fn f_6405a8(mode: CaptureModeId) {
    unsafe {
        let value = conv::f_646a68(input(0xc));
        let value = match input(0xc) {
            1 | 3 => selected(mode, 0x214, value),
            7 => selected(mode, 0x215, value),
            _ => value,
        };
        if value != 0x7fff_ffff { write_word(INPUT + 0x2f8, value); }
    }
}

unsafe fn count_code(value: u32) -> u32 {
    unsafe {
        let mut row = 0x0079_ffdc;
        loop {
            let first = read_word(row);
            let second = read_word(row + 4);
            let code = read_word(row + 8);
            if first == 0x7fff_fffe && second == 0x7fff_fffe && code == 0x7fff_fffe {
                return code;
            }
            if first == value { return code; }
            row += 12;
        }
    }
}

#[derive(Clone, Copy)]
enum IntervalColumn { Value, Code }

unsafe fn interval_projection(value: u32, from: IntervalColumn, to: IntervalColumn) -> u32 {
    unsafe {
        let mut row = 0x007a_1234;
        loop {
            let first = read_word(row);
            let code = read_word(row + 4);
            if first == 0x7fff_fffe && code == 0x7fff_fffe { return code; }
            let candidate = match from { IntervalColumn::Value => first, IntervalColumn::Code => code };
            if candidate == value {
                return match to { IntervalColumn::Value => first, IntervalColumn::Code => code };
            }
            row += 8;
        }
    }
}

pub(crate) unsafe fn f_6407f8(mode: CaptureModeId) {
    unsafe {
        let value = conv::f_646b64();
        let value = match input(0xc) {
            1 | 3 => selected(mode, 0x217, value),
            7 => selected(mode, 0x219, value),
            _ => value,
        };
        if value != 0x7fff_ffff { write_word(INPUT + 0x2fc, value); }
        if input(0xc) != 7 { return; }
        if input(0x64).wrapping_sub(1) < 2 { write_word(INPUT + 0x2fc, 5); }
        let current = count_code(input(0x2fc));
        let preferred = count_code(conv::f_646b64());
        let before = interval_projection(conv::f_646c9c(), IntervalColumn::Value, IntervalColumn::Code);
        let scaled = if current == 0 || before >= preferred { 0 }
        else {
            let remaining = preferred.wrapping_sub(before).wrapping_sub(1);
            if remaining < before {
                current.wrapping_sub(1).wrapping_sub(current.wrapping_mul(remaining) / preferred)
            } else {
                before.wrapping_mul(current) / preferred
            }
        };
        write_word(INPUT + 0x300, interval_projection(scaled, IntervalColumn::Code, IntervalColumn::Value));
    }
}

pub(crate) unsafe fn f_641438(value: u32, refresh: u32) {
    unsafe {
        if input(0) == 0x1f {
            if value.wrapping_sub(1) < 2 { a::save(PropertyKey(0x165), SettingValue(value)); }
        } else if input(0) > 0x1e && input(0).wrapping_sub(0x21) < 5 && value.wrapping_sub(1) < 2 {
            a::save(PropertyKey(0x161), SettingValue(value));
        }
        lists::f_6373f4(value, cached_mode());
        if input(0) != 0x2c { a::f_63e430(cached_mode()); }
        lists::f_637090(u32::from(read_byte(FRAME + 0xd1f4)));
        lists::f_637064(0, 0);
        lists::f_63714c();
        if input(0) != 0x2c { c::f_63c068(cached_mode()); }
        lists::f_636534(u32::from(read_byte(FRAME + 0xd02c)));
        lists::f_636508(0, 0);
        lists::f_63665c();
        if input(0) != 0x2c { a::f_63ef60(cached_mode(), u32::from(refresh != 0x35)); }
        lists::f_6366e4(u32::from(read_byte(FRAME + 0xd074)));
        lists::f_63668c(0, 0);
        lists::f_63688c();
        if input(0) != 0x2c && input(0) != 0x1d && frame(0xda98) == 0 && input(0x64) != 1 {
            write_word(INPUT + 0x60, conv::f_6440b4());
        }
        if input(0) != 0x2c { a::f_63f28c(cached_mode()); }
        lists::f_636a00(u32::from(read_byte(FRAME + 0xd0f4)));
        lists::f_6369d4(0, 0);
        lists::f_636abc();
        a::f_63f0a0(cached_mode());
        c::f_63be24(cached_mode());
        lists::f_63717c(0, 0);
        lists::f_637264();
        a::f_63e638(cached_mode());
        lists::f_637294(0, 0);
        lists::f_63737c();
        a::f_63e4cc(cached_mode());
        a::f_63e7a4(cached_mode());
        lists::f_6366b8(0, 0);
        lists::f_63685c();
        a::f_63ff74(cached_mode());
        lists::f_636aec(0, 0);
        lists::f_636bd4();
        f_6411e0(cached_mode());
        lists::f_636c04(0, 0);
        lists::f_636cec();
        write_half(INPUT + 0x124, projected(0x1e5) as u16);
        f_6405a8(cached_mode());
        f_6407f8(cached_mode());
        a::f_63f8fc(cached_mode());
        a::f_63f968(cached_mode());
        a::f_63f330(cached_mode());
        a::f_63ee88(cached_mode());
        a::f_63eef4(cached_mode());
        f_640414(cached_mode());
        f_6403a8(cached_mode());
        f_640e78(0, cached_mode());
        f_6404ec(cached_mode());
        f_640920(cached_mode());
        a::f_63f504(cached_mode());
        let mut continuous = 6;
        if input(0x64).wrapping_sub(1) < 2 {
            if matches!(input(0xc), 5 | 6 | 8) { cpu::f_63d564(u32::from(refresh != 0x35)); }
            if matches!(input(0xc), 7 | 1 | 3) && input(0) != 0xf {
                continuous = f_6400b8(input(0x2fc), input(0x2f8));
                if matches!(continuous, 1..=5) {
                    if input(0x2fc) > 5 { write_word(INPUT + 0x2fc, 5); }
                    if input(0x2f8) > 5 { write_word(INPUT + 0x2f8, 5); }
                }
            }
        }
        if matches!(input(0xc), 7 | 1 | 3) && input(0) != 0xf {
            continuous = f_6400b8(input(0x2fc), input(0x2f8));
        }
        if input(0xc) == 5 { write_word(INPUT + 0x80, 0); }
        if input(0) == 0x2c {
            c::f_63c1dc(cached_mode());
            a::f_63f0a0(cached_mode());
        }
        f_6411e0(cached_mode());
        if input(0x4c) == 7 {
            if input(0x148) == 1 {
                write_word(INPUT + 0x148, 0);
                lists::f_63717c(0, 1);
                lists::f_637238(0xb, 1);
            }
            if input(0x10) != 0 { cpu::f_63d4f4(0); }
        }
        if input(0x4c) == 0 && !matches!(input(0), 0x18 | 1 | 0x1b | 0x1c) && input(0x148) == 1 {
            write_word(INPUT + 0x148, 0);
            lists::f_63717c(0, 1);
            lists::f_637238(0xb, 1);
        }
        if matches!(input(0x13c), 3..=6) { write_word(INPUT + 0x148, 1); }
        if input(0) == 0x2c {
            if input(8) == 1 { write_word(INPUT + 0x148, 1); }
            if input(0x54).wrapping_sub(7) < 3 && input(0x148) == 1 {
                write_word(INPUT + 0x148, 0);
                lists::f_63717c(0, 1);
                lists::f_637238(0, 1);
            }
        }
        lists::f_63d688(cached_mode());
        if input(0x64).wrapping_sub(1) < 2 {
            if input(0x7c) > 0x1a || input(0x7c) == 8 {
                write_word(INPUT + 0x7c, if input(0x7c) > 0x1a { 0x19 } else { 10 });
                lists::f_63668c(0, 1);
                lists::f_636804(0xc, 1);
            }
        }
        if input(0xc) == 4 {
            if input(0x7c) == 8 {
                write_word(INPUT + 0x7c, 10);
                lists::f_63668c(0, 1);
                lists::f_636804(7, 1);
            }
            if input(0x7c) > 0x1a {
                write_word(INPUT + 0x7c, 0x19);
                lists::f_63668c(0, 1);
                lists::f_636804(7, 1);
            }
        }
        if input(0xc) == 6 && matches!(input(0x7c), 8 | 10..=15) {
            write_word(INPUT + 0x7c, 0x10);
            lists::f_63668c(0, 1);
            lists::f_636804(7, 1);
            lists::f_636804(1, 1);
        }
        if input(0x7c) == 8 && continuous == 2 {
            write_word(INPUT + 0x7c, 10);
            lists::f_63668c(0, 1);
            lists::f_636804(7, 1);
            lists::f_636804(8, 1);
            lists::f_636804(9, 1);
        }
        if continuous.wrapping_sub(4) < 2 {
            if input(0x7c) == 8 || (input(0) == 0x22 && input(0x7c) == 1) {
                write_word(INPUT + 0x7c, if input(0x7c) == 8 { 10 } else { 2 });
                lists::f_63668c(0, 1);
                lists::f_636804(7, 1);
                lists::f_636804(8, 1);
                lists::f_636804(9, 1);
            }
        }
        if input(0xc) == 8 {
            if input(0x7c) == 8 {
                write_word(INPUT + 0x7c, 10);
                lists::f_63668c(0, 1);
                lists::f_636804(7, 1);
            }
            if input(0x7c) > 0x1a {
                write_word(INPUT + 0x7c, 0x19);
                lists::f_63668c(0, 1);
                lists::f_636804(7, 1);
            }
        }
        if continuous.wrapping_sub(2) < 2 && input(0x50) == 0 {
            enforce_size_family(1);
        }
        if continuous.wrapping_sub(4) < 2 && input(0x50) != 2 {
            enforce_size_family(2);
        }
        if matches!(continuous, 1..=5) && input(0x84).wrapping_sub(2) < 2 {
            write_word(INPUT + 0x84, 1);
            lists::f_6369d4(0, 1);
            lists::f_636a90(7, 1);
            lists::f_636a90(8, 1);
            lists::f_636a90(9, 1);
        }
        if input(0xc) == 8 && input(0x84).wrapping_sub(4) < 2 {
            write_word(INPUT + 0x84, 3);
            lists::f_6369d4(0, 1);
            lists::f_636a90(7, 1);
        }
        if input(0x7c) == 1 && input(0x84) == 3 {
            write_word(INPUT + 0x84, 2);
            lists::f_6369d4(0, 1);
            lists::f_636a90(1, 1);
        }
        if matches!(input(0x7c), 8 | 10..=12) && input(0x84).wrapping_sub(2) < 2 {
            write_word(INPUT + 0x84, 1);
            lists::f_6369d4(0, 1);
            lists::f_636a90(1, 1);
        }
        if input(0x7c).wrapping_sub(0xd) < 3 && input(0x84) == 3 {
            write_word(INPUT + 0x84, 2);
            lists::f_6369d4(0, 1);
            lists::f_636a90(1, 1);
        }
        if input(0x7c) > 0x1a && input(0x84).wrapping_sub(2) < 2 {
            write_word(INPUT + 0x84, 1);
            lists::f_6369d4(0, 1);
            lists::f_636a90(1, 1);
        }
        if input(0xc) == 6 { write_word(INPUT + 0x84, 0); }
        if f_640188() == 0 || projected(0x13) == 0 { cpu::f_63db24(1); }
        if input(0x64).wrapping_sub(1) < 2 || input(0xc) != 0 || input(0x94) == 1
            || matches!(input(0x84), 4 | 5) || cpu::f_63249c(0x18) == 1
            || cpu::f_63249c(0x17) == 1 || cpu::f_63249c(0x1b) == 1
        {
            write_word(INPUT + 0x34c, 0);
            write_word(INPUT + 0x340, 0);
            if a::available_count(cached_mode(), PropertyKey(0x16)) > 1 {
                let allowed = input(0x64).wrapping_sub(1) < 2
                    || (conv::f_644674(cached_mode()) == conv::f_6444a0(cached_mode())
                        && conv::f_6444a0(cached_mode()) >= 0x17)
                    || (conv::f_643f74(cached_mode()) == conv::f_643f30(cached_mode())
                        && (conv::f_643f30(cached_mode()) == 4 || conv::f_643f30(cached_mode()) == 5));
                if allowed {
                    let value = a::project(PropertyKey(0x16), SettingValue(input(0x34c)), 3, 1);
                    a::save(PropertyKey(0x16), value);
                }
            }
        }
        if (input(0x64).wrapping_sub(1) < 2 || input(0xc) != 0 || input(0x94) == 1
            || matches!(input(0x7c), 0x17..=0x19 | 0x1b)) && input(0x344) == 1
        {
            write_word(INPUT + 0x344, 0);
            if input(0xc) != 0 {
                lists::f_6366b8(0, 1);
                lists::f_636830(7, 1);
            }
            if matches!(input(0x7c), 0x17..=0x19 | 0x1b) {
                lists::f_6366b8(0, 1);
                lists::f_636830(1, 1);
            }
        }
        if input(0x64) == 1 || input(0xc) == 7 {
            write_word(INPUT + 0x378, 0);
            write_word(INPUT + 0x380, 0);
        }
        if input(0x390) != 2 { write_word(INPUT + 0x394, 0); }
        f_640e78(0, cached_mode());
        cpu::f_63d498();
        a::f_63e638(cached_mode());
        a::f_63f568(cached_mode());
        crate::mode_initialization_record::write_initial_setting_record();
        if (input(0x84) == 0 || matches!(input(0x7c), 0..=5)) && matches!(input(0), 0x1f | 0x21 | 0x20) {
            cpu::f_63d5fc();
        }
        if matches!(input(0x80), 4 | 5 | 7..=10) { write_word(INPUT + 0x88, 0); }
    }
}

unsafe fn enforce_size_family(family: u32) {
    unsafe {
        let code = a::project(PropertyKey(0xf9), SettingValue(input(0x60)), 3, 1).0;
        for index in 0..12 {
            let row = 0x007a_00cc + index * 16;
            if read_word(row + 8) == family && read_word(row + 12) == code {
                write_word(INPUT + 0x50, read_word(row));
                lists::f_636508(0, 1);
                lists::f_636630(7, 1);
                lists::f_636630(8, 1);
                lists::f_636630(9, 1);
                break;
            }
        }
    }
}
