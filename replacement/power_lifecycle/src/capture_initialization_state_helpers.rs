//! Capture candidate snapshots and flash-state projection for X-M1 1.50.
//! Source: x70view 55436fc85c0560dd252b0410af6f2427fe198cd3.

use crate::capture_initialization_state::{read_word, read_byte, write_word, write_byte, FRAME, INPUT};
use crate::capture_initialization_helpers_a::{property, project, preferred_value, value_available, PropertyKey, SettingValue};
use crate::mode_initialization::{CaptureModeId, FlagId, query_flag};

macro_rules! fields {
    (byte: $($name:ident => $offset:expr),* $(,)?) => {$ (
        pub(crate) unsafe fn $name(value: u32) {
            unsafe { write_byte(FRAME + $offset, value as u8); }
        }
    )*};
    (word: $($name:ident => $offset:expr),* $(,)?) => {$ (
        pub(crate) unsafe fn $name(value: u32) {
            unsafe { write_word(FRAME + $offset, value); }
        }
    )*};
}

fields!(byte:
    f_636534 => 0xd02d, f_636558 => 0xd02e, f_63660c => 0xd02f,
    f_6366e4 => 0xd075, f_636708 => 0xd235, f_63672c => 0xd076,
    f_636750 => 0xd236, f_6367bc => 0xd077, f_6367e0 => 0xd237,
    f_6368e8 => 0xd0b5, f_63690c => 0xd0b6, f_636954 => 0xd0b7,
    f_636a00 => 0xd0f5, f_636a24 => 0xd0f6, f_636a6c => 0xd0f7,
    f_636b18 => 0xd2b5, f_636b3c => 0xd2b6, f_636b84 => 0xd2b7,
    f_636c30 => 0xd2f5, f_636c54 => 0xd2f6, f_636c9c => 0xd2f7,
    f_636d48 => 0xd175, f_636d6c => 0xd176, f_636db4 => 0xd177,
    f_636e60 => 0xd135, f_636e84 => 0xd136, f_636ecc => 0xd137,
    f_636f78 => 0xd1b5, f_636f9c => 0xd1b6, f_636fe4 => 0xd1b7,
    f_637090 => 0xd1f5, f_6370b4 => 0xd1f6, f_6370fc => 0xd1f7,
    f_6371a8 => 0xd275, f_6371cc => 0xd276, f_637214 => 0xd277,
    f_6372c0 => 0xd335, f_6372e4 => 0xd336, f_63732c => 0xd337,
    f_6373ac => 0xd374, f_6373d0 => 0xd375,
);
fields!(word:
    f_63657c => 0xd068, f_6365a0 => 0xda40, f_6365c4 => 0xd06c,
    f_6365e8 => 0xd070, f_636774 => 0xd0b0, f_636798 => 0xd270,
    f_636930 => 0xd0f0, f_636a48 => 0xd130, f_636b60 => 0xd2f0,
    f_636c78 => 0xd330, f_636d90 => 0xd1b0, f_636ea8 => 0xd170,
    f_636fc0 => 0xd1f0, f_6370d8 => 0xd230, f_6371f0 => 0xd2b0,
    f_637308 => 0xd370,
);

macro_rules! snapshots {
    ($($name:ident => $offset:expr),* $(,)?) => {$ (
        pub(crate) unsafe fn $name(index: u32, value: u32) {
            let address = (FRAME as u32).wrapping_add($offset).wrapping_add(index.wrapping_mul(0x35c));
            unsafe { write_byte(address as usize, value as u8); }
        }
    )*};
}
snapshots!(
    f_636508 => 0xd02c, f_63668c => 0xd074, f_6366b8 => 0xd234,
    f_6368bc => 0xd0b4, f_6369d4 => 0xd0f4, f_636aec => 0xd2b4,
    f_636c04 => 0xd2f4, f_636d1c => 0xd174, f_636e34 => 0xd134,
    f_636f4c => 0xd1b4, f_637064 => 0xd1f4, f_63717c => 0xd274,
    f_637294 => 0xd334,
);

macro_rules! candidates {
    ($($set:ident, $clear:ident => $offset:expr),* $(,)?) => {$ (
        pub(crate) unsafe fn $set(index: u32, value: u32) {
            let address = (FRAME as u32).wrapping_add($offset).wrapping_add(index.wrapping_mul(4));
            unsafe { write_word(address as usize, value); }
        }
        pub(crate) unsafe fn $clear() {
            for index in 0..14 { unsafe { write_word(FRAME + $offset + index * 4, 0); } }
        }
    )*};
}
candidates!(
    f_636630, f_63665c => 0xd030,
    f_636804, f_63688c => 0xd078,
    f_636830, f_63685c => 0xd238,
    f_636978, f_6369a4 => 0xd0b8,
    f_636a90, f_636abc => 0xd0f8,
    f_636ba8, f_636bd4 => 0xd2b8,
    f_636cc0, f_636cec => 0xd2f8,
    f_636dd8, f_636e04 => 0xd178,
    f_636ef0, f_636f1c => 0xd138,
    f_637008, f_637034 => 0xd1b8,
    f_637120, f_63714c => 0xd1f8,
    f_637238, f_637264 => 0xd278,
    f_637350, f_63737c => 0xd338,
);

pub(crate) unsafe fn f_6373f4(value: u32, mode: CaptureModeId) {
    let offset = match mode.raw() {
        0x21 | 0x25 => 0xd378,
        0x24 => 0xd37c,
        0x23 => 0xd380,
        0x22 => 0xd384,
        _ => return,
    };
    unsafe { write_word(FRAME + offset, value); }
}

pub(crate) unsafe fn f_646550() -> u32 {
    unsafe { u32::from(property(PropertyKey(0x1e8)).0 == 1) }
}

pub(crate) unsafe fn f_6465a4() -> u32 {
    unsafe { project(PropertyKey(0xb), property(PropertyKey(0xb)), 1, 3).0 }
}

fn ordinary_flash_mode(mode: CaptureModeId) -> bool {
    matches!(mode.raw(), 0..=3 | 5 | 0xb..=0xd | 0x10 | 0x11 | 0x13 | 0x16..=0x18 | 0x1f..=0x25)
}

fn flash_probe_mode(mode: CaptureModeId) -> bool {
    ordinary_flash_mode(mode) || matches!(mode.raw(), 0xf | 0x14 | 0x1b | 0x1c)
}

pub(crate) unsafe fn f_63d688(mode: CaptureModeId) {
    use crate::capture_initialization::{f_63d3bc, f_63d5fc, f_63d614};
    use crate::capture_initialization_conversions::{f_643c4c, f_6464f4};
    unsafe {
        let mut info = [0; 2];
        let status = f_72b858(&mut info);
        let current = CaptureModeId::new(read_word(INPUT));
        if flash_probe_mode(current) && query_flag(FlagId::new(0x35)) == 1
            && status == 0 && info[1] != 0 && info[1] != 0xf
        {
            let mut extended = [0; 11];
            let _ = f_72b868(&mut extended);
            let selection = if extended[0] == 0 {
                2
            } else if property(PropertyKey(0x20d)).0 == 1 || read_word(INPUT + 0xc) != 0 || read_word(INPUT + 0x10) == 2 {
                0
            } else {
                u32::from(ordinary_flash_mode(CaptureModeId::new(read_word(INPUT))))
            };
            write_word(INPUT + 0x18, selection);
            write_word(FRAME + 0xda94, 1);
        } else {
            write_word(INPUT + 0x18, 0);
            write_word(FRAME + 0xda94, 0);
        }
        write_word(INPUT + 0x1c, 0);
        let mut selection;
        if (read_word(INPUT + 0xc) == 0 && property(PropertyKey(0x20d)).0 == 0 && read_word(INPUT + 0x10) != 2) || read_word(INPUT) == 0xf {
            selection = f_643c4c(mode);
            if !value_available(mode, PropertyKey(0x1ee), SettingValue(selection)) {
                selection = preferred_value(mode, PropertyKey(0x1ee)).0;
            }
            write_word(INPUT + 0x14, selection);
            if selection == 10 { write_word(INPUT + 0x1c, 2); }
            if ordinary_flash_mode(CaptureModeId::new(read_word(INPUT))) && f_6464f4() == 1 && read_word(INPUT + 0x18) == 0 {
                selection = 7;
            }
        } else {
            selection = 6;
        }
        let observed = f_63d3bc(selection);
        write_word(INPUT + 0x14, observed);
        if !(read_word(INPUT + 0x20) == 1 && read_word(INPUT + 0x14) != 7 && read_word(INPUT + 0x18) == 0)
            && read_word(INPUT + 0x14) != 6
            && matches!(read_word(INPUT), 0x1f | 0x20 | 0x21)
        {
            f_63d5fc();
        }
        f_63d614(u32::from(query_flag(FlagId::new(0x1c)) != 1));
    }
}

unsafe extern "C" {
    fn xm1_vendor_device_serial_read(command: u32, output: *mut u8, length: u32) -> i32;
}

unsafe fn configured_flash_mode(packet: u8) -> u8 {
    let option = unsafe { read_byte(read_word(0x0096_ca50) as usize + 0x1367) };
    if option & 0x80 == 0 {
        packet & 15
    } else {
        match option & 15 { value @ (1 | 2) => value, _ => 0 }
    }
}

pub(crate) unsafe fn f_72b858(output: &mut [u8; 2]) -> u32 {
    unsafe {
        let mut packet = [0; 2];
        let result = xm1_vendor_device_serial_read(0x23, packet.as_mut_ptr(), packet.len() as u32);
        output[0] = (packet[1] >> 4) & 1;
        output[1] = configured_flash_mode(packet[1]);
        write_byte(0x0089_f7a8, output[1]);
        result as u32
    }
}

pub(crate) unsafe fn f_72b868(output: &mut [u8; 11]) -> u32 {
    unsafe {
        let mut packet = [0; 9];
        let result = xm1_vendor_device_serial_read(0x20, packet.as_mut_ptr(), packet.len() as u32);
        output[0] = packet[1] >> 6;
        output[1] = (packet[1] >> 5) & 1;
        output[2] = (packet[1] >> 4) & 1;
        output[3] = (packet[2] >> 4) & 1;
        output[4] = configured_flash_mode(packet[2]);
        output[5] = packet[3];
        output[6] = packet[4];
        output[7] = packet[5] >> 7;
        output[8] = packet[5] & 127;
        output[9] = packet[6] & 3;
        output[10] = packet[7] & 1;
        result as u32
    }
}

pub(crate) unsafe fn f_72b774(output: *mut u32) -> u32 {
    let source = unsafe { read_word(0x0093_a778) };
    let (code, result) = match source {
        0 => (0xd4, 0),
        3 => (0xd2, 0),
        4 => (0xd3, 0),
        5 => (0xd5, 0),
        6 => (0xd6, 0),
        9 => (0xd8, 0),
        10 => (0xd9, 0),
        _ => (0x81, (-63_i32) as u32),
    };
    unsafe { core::ptr::write_volatile(output, code); }
    result
}
