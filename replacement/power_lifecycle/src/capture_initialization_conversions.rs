use crate::capture_initialization_helpers_a::{property, project, save, PropertyKey, SettingValue};
use crate::capture_initialization_state::*;
use crate::mode_initialization::CaptureModeId;

unsafe extern "C" {
    fn xm1_property_get(id: u32, bank: u32, defaults: u32, selection: u32) -> u32;
}

unsafe fn projected_property(key: PropertyKey, projection: PropertyKey) -> u32 {
    unsafe { project(projection, property(key), 1, 3).0 }
}

unsafe fn mode_property(mode: CaptureModeId, table: usize, projection: PropertyKey, fallback: u32) -> u32 {
    let key = PropertyKey(unsafe { read_word(table + mode.raw() as usize * 4) });
    if key.0 == 0 { fallback } else { unsafe { projected_property(key, projection) } }
}

pub(crate) unsafe fn f_643c4c(mode: CaptureModeId) -> u32 {
    let mut key = PropertyKey(unsafe { read_word(0x0079_fe20 + mode.raw() as usize * 4) });
    if mode.raw() == 12 && unsafe { (read_word(INPUT) == 12 && read_word(INPUT + 0x90) == 1) || read_word(INPUT) == 18 } {
        key = PropertyKey(0x1f7);
    }
    if key.0 == 0 { 6 } else { unsafe { crate::capture_initialization::f_63d3bc(projected_property(key, PropertyKey(0x1ee))) } }
}

pub(crate) unsafe fn f_643d94(mode: CaptureModeId) -> u32 {
    let key = PropertyKey(unsafe { read_word(0x007a_0018 + mode.raw() as usize * 4) });
    if key.0 == 0 { 1 } else { unsafe { property(key).0 } }
}

pub(crate) unsafe fn f_643dc4(mode: CaptureModeId, value: u32) {
    let key = PropertyKey(unsafe { read_word(0x007a_0018 + mode.raw() as usize * 4) });
    if key.0 != 0 { unsafe { save(key, SettingValue(value)); } }
}

pub(crate) unsafe fn f_643dec(mode: CaptureModeId) -> u32 {
    unsafe { mode_property(mode, 0x007a_0eb8, PropertyKey(0x14b), 8) }
}
pub(crate) unsafe fn f_643e58(mode: CaptureModeId) -> u32 {
    unsafe { mode_property(mode, 0x007a_09fc, PropertyKey(0x14f), 2) }
}
pub(crate) unsafe fn f_643ec4(mode: CaptureModeId) -> u32 {
    unsafe { mode_property(mode, 0x007a_1314, PropertyKey(0x153), 3) }
}
pub(crate) unsafe fn f_643f30(mode: CaptureModeId) -> u32 {
    unsafe { mode_property(mode, 0x007a_1120, PropertyKey(0x104), 0) }
}
pub(crate) unsafe fn f_644004(use_default: u32) -> u32 {
    let value = if use_default == 1 {
        unsafe { xm1_property_get(0xf6, 0, 1, 0) }
    } else { unsafe { property(PropertyKey(0xf6)).0 } };
    unsafe { project(PropertyKey(0xf6), SettingValue(value), 1, 3).0 }
}
pub(crate) unsafe fn f_644054() -> u32 {
    unsafe { projected_property(PropertyKey(0xfa), PropertyKey(0xfa)) }
}
pub(crate) unsafe fn f_644080() -> u32 {
    u32::from(matches!(unsafe { property(PropertyKey(0xf2)).0 }, 1 | 3))
}
pub(crate) unsafe fn f_6440b4() -> u32 {
    match unsafe { read_word(INPUT) } {
        29 => 0,
        44 => {
            let pixel = unsafe { projected_property(PropertyKey(0xf2), PropertyKey(0xf2)) };
            let option = unsafe { f_644080() };
            for index in 0..8 {
                let row = 0x007a_11d4 + index * 12;
                if unsafe { read_word(row) == pixel && read_word(row + 4) == option } {
                    return unsafe { read_word(row + 8) };
                }
            }
            0
        }
        _ => unsafe { projected_property(PropertyKey(0xf9), PropertyKey(0xf9)) },
    }
}
pub(crate) unsafe fn f_6444a0(mode: CaptureModeId) -> u32 {
    let key = match mode.raw() {
        0 => 0x157, 2 => 0x15b, 3 => 0x15c, 4 => 0x15d,
        30 => 0x160, 31 => 0x15e, 32 => 0x15f, 33..=37 => 0x141,
        _ => return 0,
    };
    unsafe { projected_property(PropertyKey(key), PropertyKey(0x141)) }
}
pub(crate) unsafe fn f_644a24(white_balance: u32) -> u32 {
    let key = match white_balance {
        0 => 0x115, 1..=7 => 0x119 + white_balance * 2, 8 => 0x117, 10 => 0x119, _ => return 0,
    };
    unsafe { projected_property(PropertyKey(key), PropertyKey(0x113)) }
}
pub(crate) unsafe fn f_644bd4(white_balance: u32) -> u32 {
    let key = match white_balance {
        0 => 0x116, 1..=7 => 0x11a + white_balance * 2, 8 => 0x118, 10 => 0x11a, _ => return 0,
    };
    unsafe { projected_property(PropertyKey(key), PropertyKey(0x114)) }
}
pub(crate) unsafe fn f_6460c0() -> u32 {
    let key = if unsafe { read_word(INPUT) } == 44 { 0x137 } else { 0x129 };
    unsafe { projected_property(PropertyKey(key), PropertyKey(0x129)) }
}
pub(crate) unsafe fn f_6461cc() -> u32 {
    if unsafe { read_word(INPUT + 0x90) == 1 && read_word(INPUT) == 12 } { 0 }
    else if unsafe { read_word(INPUT) } == 18 { 22 } else { 13 }
}
pub(crate) unsafe fn f_63e1f4() -> u32 {
    let mode = unsafe { read_word(INPUT) };
    if matches!(mode, 34 | 36) {
        if unsafe { read_word(FRAME + 0xda64) } == 1 { 13 } else { unsafe { u32::from(read_byte(0x0094_32a0 + 0x155)) } }
    } else if unsafe { read_word(INPUT + 0x90) == 1 } && mode == 12 { 0 }
    else if mode == 18 { 22 } else { 13 }
}
pub(crate) unsafe fn f_63e2a0() -> u32 {
    let mode = unsafe { read_word(INPUT) };
    if matches!(mode, 34 | 36) {
        if unsafe { read_word(FRAME + 0xda64) } == 1 { 64 } else { unsafe { u32::from(read_byte(0x0094_32a0 + 0x154)) } }
    } else if unsafe { read_word(INPUT + 0x90) == 1 } && mode == 12 { 0 }
    else if mode == 18 { 31 } else { 64 }
}
pub(crate) unsafe fn f_6462d8() -> u32 {
    let maximum = unsafe { f_63e2a0() };
    let minimum = unsafe { f_63e1f4() };
    let mode = unsafe { read_word(INPUT) };
    let key = match mode {
        12 if unsafe { read_word(INPUT + 0x90) } == 1 => Some(PropertyKey(0x1d5)),
        18 => Some(PropertyKey(0x1d6)), 34 => Some(PropertyKey(0x1ce)), 36 => Some(PropertyKey(0x1cf)), _ => None,
    };
    let candidate = match key {
        Some(key) => {
            let value = unsafe { property(key).0 } & 0xff;
            if value == 0 {
                if matches!(unsafe { read_word(INPUT) }, 34 | 36) { return u32::MAX; }
                minimum
            } else { unsafe { f_6461cc() }.wrapping_add(value - 1) }
        }
        None => minimum,
    };
    if candidate == u32::MAX { candidate }
    else if (candidate as i32) < minimum as i32 { minimum }
    else if (candidate as i32) >= maximum as i32 { maximum } else { candidate }
}
pub(crate) unsafe fn f_6464b0() -> u32 {
    unsafe { property(PropertyKey(0x1c6)).0 }.wrapping_sub(127)
}
pub(crate) unsafe fn f_6464f4() -> u32 {
    unsafe { project(PropertyKey(0x13e), property(PropertyKey(0x13e)), 1, 0).0 }
}
pub(crate) unsafe fn f_646608() -> u32 {
    unsafe { projected_property(PropertyKey(12), PropertyKey(12)) }
}
pub(crate) unsafe fn f_64666c() -> u32 {
    unsafe { projected_property(PropertyKey(0x140), PropertyKey(0x140)) }
}
pub(crate) unsafe fn f_6466d0() -> u32 {
    unsafe { projected_property(PropertyKey(300), PropertyKey(300)) }
}
pub(crate) unsafe fn f_646734() -> u32 {
    unsafe { projected_property(PropertyKey(301), PropertyKey(301)) }
}
pub(crate) unsafe fn f_646924() -> u32 {
    let selection = u32::from(unsafe { read_word(0x0095_1480 + 0xe4) } != 3);
    unsafe { xm1_property_get(220, 0, 1, selection); }
    let value = unsafe { property(PropertyKey(220)).0 };
    if unsafe { read_word(0x0095_1f80 + 0x98) } != 0 { 0 } else { value }
}

pub(crate) unsafe fn property_bank(key: PropertyKey, bank: u32) -> SettingValue {
    let selection = u32::from(unsafe { read_word(0x0095_1480 + 0xe4) } != 3);
    SettingValue(unsafe { xm1_property_get(key.0, bank, 0, selection) })
}

pub(crate) unsafe fn default_property(key: PropertyKey) -> SettingValue {
    let selection = u32::from(unsafe { read_word(0x0095_1480 + 0xe4) } != 3);
    SettingValue(unsafe { xm1_property_get(key.0, 0, 1, selection) })
}

pub(crate) unsafe fn f_643f74(mode: CaptureModeId) -> u32 {
    let key = PropertyKey(unsafe { read_word(0x007a_1120 + mode.raw() as usize * 4) });
    if key.0 == 0 { 0 } else { unsafe { project(PropertyKey(0x104), property_bank(key, 1), 1, 3).0 } }
}

pub(crate) unsafe fn f_644674(mode: CaptureModeId) -> u32 {
    let key = match mode.raw() {
        0 => 0x157, 2 => 0x15b, 3 => 0x15c, 4 => 0x15d,
        30 => 0x160, 31 => 0x15e, 32 => 0x15f, 33..=37 => 0x141,
        _ => return 0,
    };
    unsafe { project(PropertyKey(0x141), property_bank(PropertyKey(key), 1), 1, 3).0 }
}

pub(crate) unsafe fn f_6461a0() -> u32 { unsafe { projected_property(PropertyKey(26), PropertyKey(26)) } }
pub(crate) unsafe fn f_646798() -> u32 { unsafe { projected_property(PropertyKey(364), PropertyKey(364)) } }
pub(crate) unsafe fn f_6467c4() -> u32 { unsafe { projected_property(PropertyKey(72), PropertyKey(72)) } }
pub(crate) unsafe fn f_6467f0() -> u32 { unsafe { projected_property(PropertyKey(75), PropertyKey(75)) } }
pub(crate) unsafe fn f_64681c() -> u32 { unsafe { projected_property(PropertyKey(471), PropertyKey(471)) } }
pub(crate) unsafe fn f_6469a0() -> u32 { unsafe { projected_property(PropertyKey(528), PropertyKey(528)) } }
pub(crate) unsafe fn f_646a04() -> u32 { unsafe { projected_property(PropertyKey(529), PropertyKey(529)) } }
pub(crate) unsafe fn f_646a68(input: u32) -> u32 {
    unsafe { default_property(PropertyKey(532)); }
    let value = match input {
        1 => unsafe { property(PropertyKey(532)) },
        3 => unsafe { property(PropertyKey(534)) },
        7 => unsafe { property(PropertyKey(533)) },
        _ => unsafe { default_property(PropertyKey(532)) },
    };
    unsafe { project(PropertyKey(532), value, 1, 3).0 }
}
pub(crate) unsafe fn f_646b64() -> u32 {
    let (key, projection) = match unsafe { read_word(INPUT + 0xc) } {
        1 => (535, 535), 3 => (536, 535), 7 => (537, 537), _ => return 0,
    };
    unsafe { projected_property(PropertyKey(key), PropertyKey(projection)) }
}
pub(crate) unsafe fn f_646c9c() -> u32 { unsafe { projected_property(PropertyKey(538), PropertyKey(538)) } }

pub(crate) unsafe fn f_6376a0(mode: CaptureModeId) -> u32 {
    let offset = match mode.raw() { 33 | 37 => 0xd378, 34 => 0xd384, 35 => 0xd380, 36 => 0xd37c, _ => return 0 };
    unsafe { read_word(FRAME + offset) }
}
pub(crate) unsafe fn f_64268c() -> u32 {
    let low = u32::from(unsafe { read_byte(0x0095_6770) });
    let high = u32::from(unsafe { read_byte(0x0095_6771) });
    (low | (high << 8)) / 10
}
pub(crate) unsafe fn f_61d018() -> u32 { unsafe { read_word(0x0095_71d0 + 0x42c) } }
pub(crate) unsafe fn f_61d154() -> u32 { unsafe { menu_snapshot().node_id } }
pub(crate) unsafe fn f_61d1f4() -> u32 { unsafe { menu_snapshot().property } }

struct MenuSnapshot { node_id: u32, property: u32 }

unsafe fn menu_snapshot() -> MenuSnapshot {
    const MENU: usize = 0x0095_71d0;
    unsafe {
        if read_word(MENU) == 0 { return MenuSnapshot { node_id: 134, property: 0 }; }
        menu_context();
        let node = read_word(MENU) as usize;
        let node_id = read_word(node);
        read_word(node + 4);
        let property = read_word(node + 8);
        let values = read_word(node + 0x20) as usize;
        if values != 0 {
            read_word(values + read_word(MENU + 8) as usize * 8);
            let previous = read_word(MENU + 0x15c);
            let values = read_word(node + 0x20) as usize;
            read_word(values + previous as usize * 8);
        }
        read_word(MENU + 8);
        read_word(MENU + 12);
        read_word(MENU + 16);
        let kind = read_word(node + 12);
        if kind == 0 { menu_divide_by_zero(); }
        read_word(MENU + 20);
        MenuSnapshot { node_id, property }
    }
}

unsafe fn menu_context() {
    const MENU: usize = 0x0095_71d0;
    unsafe {
        let node = read_word(MENU) as usize;
        if node == 0 { return; }
        let property = read_word(node + 8);
        if matches!(property, 5 | 0x220 | 0x247 | 0x245 | 0x47 | 0xdf | 0xe0 | 0x240 | 0x1bb | 0x1b9 | 0xd0 | 0x185 | 0x246) {
            read_word(node);
            return;
        }
        let selected = match property {
            0xed | 0xe5 | 0x141 | 7 | 0x166 | 0x27 => {
                let index = read_word(MENU + 8);
                let values = read_word(node + 0x20) as usize;
                Some(read_word(values + index as usize * 8))
            }
            _ => None,
        };
        let matched = matches!((property, selected), (0xed, Some(3)) | (0xe5, Some(0)) | (0x141, Some(27)) | (7, Some(1)) | (0x166, Some(0)) | (0x27, Some(0)))
            || matches!(property, 0x2d | 0x43 | 0xad | 0xe6 | 0x100 | 0x101 | 0x102 | 0xdc | 0xfc);
        if !matched {
            let parent = read_word(node + 0x1c) as usize;
            if parent == 0 {
                let values = read_word(node + 0x20) as usize;
                read_word(values + read_word(MENU + 8) as usize * 8 + 4);
            } else if read_word(parent) != read_word(node) && read_word(parent + 8) == 5 {
                let values = read_word(node + 0x20) as usize;
                read_word(values + read_word(MENU + 8) as usize * 8 + 4);
            }
        }
        read_word(node);
    }
}

fn menu_divide_by_zero() -> ! {
    #[cfg(target_arch = "arm")]
    unsafe { core::arch::asm!("svc 0x1010"); }
    #[cfg(not(target_arch = "arm"))]
    panic!("menu page divisor is zero");
    #[cfg(target_arch = "arm")]
    loop { core::hint::spin_loop(); }
}
