use core::ptr::{read_volatile, write_volatile};

const FRAME: usize = 0x0094_4580;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum Mode {
    None = 0, Mode1 = 1, Mode2 = 2, Mode3 = 3, Mode4 = 4,
    Mode5 = 5, Mode6 = 6, Mode7 = 7, Mode8 = 8, Mode9 = 9,
    Mode10 = 10, Mode11 = 11, Mode12 = 12, PowerOff = 13, Mode14 = 14,
}
impl Mode {
    pub const fn parse(raw: u32) -> Option<Self> {
        Some(match raw {
            0 => Self::None, 1 => Self::Mode1, 2 => Self::Mode2, 3 => Self::Mode3,
            4 => Self::Mode4, 5 => Self::Mode5, 6 => Self::Mode6, 7 => Self::Mode7,
            8 => Self::Mode8, 9 => Self::Mode9, 10 => Self::Mode10, 11 => Self::Mode11,
            12 => Self::Mode12, 13 => Self::PowerOff, 14 => Self::Mode14, _ => return None,
        })
    }
    pub const fn raw(self) -> u32 { self as u32 }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum ModeRequest {
    Initial = 1, Alternate = 2, Capture = 3, Request4 = 4, Request5 = 5,
    Request6 = 6, Request7 = 7, Playback = 8, Request9 = 9, Request10 = 10,
    Request11 = 11, Request12 = 12, Request13 = 13, PowerOff = 14,
    Request15 = 15, Request16 = 16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PowerOffConditions {
    pub delay_nonzero: bool,
    pub flag23: bool,
    pub flag24: bool,
    pub flag27: bool,
}

pub fn requested_mode(pending: Mode, request: ModeRequest, power: PowerOffConditions) -> Mode {
    use Mode::*;
    match request {
        ModeRequest::Initial if pending == None => Mode3,
        ModeRequest::Alternate if matches!(pending, None | Mode3) => Mode4,
        ModeRequest::Capture if pending == Mode3 => Mode5,
        ModeRequest::Capture if pending == Mode4 => Mode6,
        ModeRequest::Playback if matches!(pending, None | Mode3 | Mode4) => Mode7,
        ModeRequest::Request10 if matches!(pending, None | Mode3 | Mode4 | Mode7) => Mode8,
        ModeRequest::Request12 if matches!(pending, None | Mode3 | Mode4 | Mode7 | Mode8) => Mode11,
        ModeRequest::Request13 if matches!(pending, None | Mode3 | Mode4 | Mode7 | Mode8) => Mode2,
        ModeRequest::PowerOff => if !power.delay_nonzero || power.flag23 || power.flag24 || power.flag27 { PowerOff } else { Mode12 },
        ModeRequest::Request16 => Mode14,
        _ => pending,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FlagId(u32);
impl FlagId {
    pub const fn new(raw: u32) -> Self { Self(raw) }
    pub const fn raw(self) -> u32 { self.0 }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ModeToken(u32);
impl ModeToken {
    pub const fn raw(self) -> u32 { self.0 }
}

fn load(address: usize) -> u32 { unsafe { read_volatile(address as *const u32) } }
fn store(address: usize, value: u32) { unsafe { write_volatile(address as *mut u32, value) } }
fn byte(address: usize) -> u8 { unsafe { read_volatile(address as *const u8) } }

pub fn compact_flag_index(id: FlagId) -> u32 {
    const RANGES: [(u32, u32); 7] = [(0, 0), (1, 58), (257, 266), (512, 543), (768, 826), (1024, 1067), (1280, 1291)];
    let mut preceding = 0;
    for (start, end) in RANGES {
        if id.0 >= start && id.0 < end { return preceding + id.0 + 1 - start; }
        preceding += end - start;
    }
    0
}

/// The fixed UI frame and its flag words must be mapped.
pub unsafe fn query_flag(id: FlagId) -> u32 {
    if id.0.wrapping_sub(1536) <= 3 {
        return load(FRAME + ((id.0 + 9920 + 38) as usize) * 4);
    }
    if id.0 >= 1536 { return 0; }
    let index = compact_flag_index(id);
    u32::from(load(FRAME + ((index / 32 + 11470) as usize) * 4) & (1 << (index % 32)) != 0)
}

pub fn delay_ticks(value: u8, fallback: bool) -> u32 {
    let ticks = ((value as f64) * f64::from_bits(0x4016_6666_6666_6666)) as u32;
    if ticks == 0 && fallback { 10 } else { ticks }
}

#[derive(Clone, Copy)]
#[repr(u32)]
pub enum SettingId { Setting1 = 1, Setting2 = 2, Setting8 = 8, Setting30 = 30, Setting41 = 41, Setting48 = 48, Setting49 = 49, Setting52 = 52, Setting70 = 70, Setting72 = 72, Setting74 = 74, Setting75 = 75, Setting80 = 80, Setting175 = 175, Setting176 = 176, Setting206 = 206, Setting207 = 207, Setting212 = 212, Setting213 = 213, Setting217 = 217, Setting218 = 218, Setting225 = 225, Setting226 = 226, Setting229 = 229, Setting231 = 231, Setting232 = 232, Setting233 = 233, Setting234 = 234, Setting235 = 235, Setting245 = 245, Setting353 = 353, Setting357 = 357, Setting492 = 492, Setting493 = 493, Setting610 = 610, Setting611 = 611, Setting612 = 612, Setting613 = 613, Setting614 = 614, Setting642 = 642, Setting645 = 645 }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum ProjectionColumn { Index = 0, First = 1, Second = 2, Third = 3 }

unsafe extern "C" {
    fn xm1_getter_owned(id: u32, bank: u32, defaults: u32, selection: u32) -> u32;
}

/// The owned property service and fixed property tables must be available.
pub unsafe fn setting(id: SettingId) -> u32 {
    let selection = u32::from(load(0x0095_1480 + 0xe4) != 3);
    unsafe { xm1_getter_owned(id as u32, 0, 0, selection) }
}

/// The property projection table and error cell must be mapped.
pub unsafe fn project_setting(id: SettingId, value: u32, output_column: ProjectionColumn) -> u32 {
    unsafe { project_setting_columns(id, value, ProjectionColumn::First, output_column) }
}

pub unsafe fn project_setting_columns(id: SettingId, value: u32, input_column: ProjectionColumn, output_column: ProjectionColumn) -> u32 {
    use crate::capture_initialization_helpers_a::{project, PropertyKey, SettingValue};
    unsafe { project(PropertyKey(id as u32), SettingValue(value), input_column as u32, output_column as u32).0 }
}

/// The owned property service, mode frame and runtime settings must be mapped.
pub unsafe fn power_off_delay() -> u32 {
    let first = unsafe { project_setting(SettingId::Setting213, setting(SettingId::Setting213), ProjectionColumn::Index) };
    let second = unsafe { project_setting(SettingId::Setting80, setting(SettingId::Setting80), ProjectionColumn::Index) };
    let offset = match (first == 0, second == 1) { (true, true) => 0xf7, (true, false) => 0xf9, (false, true) => 0xf8, (false, false) => 0xfa };
    delay_ticks(byte(0x0096_b400 + offset), load(0x0095_0380 + 0x28) == 1)
}

fn transient_flag(id: u32) -> bool {
    let quotient = id / 63;
    let bit = (id + quotient) & 63;
    quotient < 2 && load(0x0095_0118 + quotient as usize * 8 + (bit / 32) as usize * 4) & (1 << (bit % 32)) != 0
}

/// The fixed UI frame, owned properties and transient flags must be mapped.
pub unsafe fn request_mode14() -> Mode {
    store(FRAME + 0xbb80, ModeRequest::PowerOff as u32);
    let mode = if unsafe { power_off_delay() } == 0 || transient_flag(23) || transient_flag(24) || transient_flag(27) { Mode::PowerOff } else { Mode::Mode12 };
    store(FRAME + 0xbb78, mode.raw());
    mode
}

/// The fixed flag backing must be mapped.
pub unsafe fn active_mode_token() -> ModeToken {
    const ORDER: [u32; 24] = [0x302, 0x304, 0x305, 0x307, 0x308, 0x309, 0x30a, 0x30b, 0x30d, 0x30c, 0x30e, 0x30f, 0x310, 0x311, 0x312, 0x317, 0x318, 0x319, 0x31a, 0x306, 0x31b, 0x31c, 0x31d, 0x314];
    ModeToken(ORDER.into_iter().find(|raw| unsafe { query_flag(FlagId::new(*raw)) } == 1).unwrap_or(ORDER[0]))
}

/// The fixed UI frame must be mapped.
pub unsafe fn store_mode_token(token: ModeToken) { store(FRAME + 0xbea4, token.raw()); }

/// The fixed flag backing must be mapped.
pub unsafe fn set_flag(id: FlagId, enabled: bool) {
    let index = compact_flag_index(id);
    let address = FRAME + ((index / 32 + 11470) as usize) * 4;
    let mask = 1 << (index % 32);
    let value = load(address);
    store(address, if enabled { value | mask } else { value & !mask });
}

unsafe extern "C" {
    fn xm1_power_backup_write_field(index: u32, value: u32, mode: u32) -> i32;
}

/// The power controller MMIO and configuration table must be mapped.
pub unsafe fn refresh_power_source() {
    if load(0x0089_f81c) != 0 { return; }
    store(0x0089_f81c, 1);
    store(0x0089_f818, 4);
    let pins = load(0x400a_00c0);
    store(0x0089_f814, pins);
    store(0x0089_f824, pins & 0xf3);
    store(0x0089_f820, pins & 3);
    unsafe { xm1_power_backup_write_field(7, 0, 1); }
    let state = load(0x0089_f824);
    if state & 0x80 != 0 {
        store(0x0089_f818, load(0x007c_b798));
    } else {
        for index in 1..6 {
            let row = 0x007c_b794 + index * 8;
            if state & u32::from(byte(row)) == u32::from(byte(row + 1)) {
                store(0x0089_f818, load(row + 4));
                break;
            }
        }
    }
    if load(0x0089_f814) & 4 == 0
        && matches!(load(0x0089_f818), 2 | 3)
        && load(0x400a_0000) & 0x20 == 0 {
        store(0x0089_f818, 4);
    }
}

/// The power source refresh requirements apply.
pub unsafe fn system_source_mode() -> u32 {
    unsafe { refresh_power_source(); }
    u32::from(load(0x0089_f818) == 1)
}

/// The frontend admits this call only when pending mode is not 12, 13 or 14.
pub unsafe fn request_power_off_mode() -> Mode {
    store(FRAME + 0xbb80, ModeRequest::PowerOff as u32);
    let delay = unsafe { power_off_delay() };
    let current = load(FRAME + 0xbb6c);
    let immediate = delay == 0 || matches!(current, 11 | 8)
        || (current == 12 && unsafe { system_source_mode() } != 2)
        || transient_flag(23) || transient_flag(24) || transient_flag(27);
    let mode = if immediate {
        Mode::PowerOff
    } else if load(FRAME + 0xbf40) == 1 {
        store(FRAME + 0xbf40, 0);
        Mode::PowerOff
    } else {
        Mode::Mode12
    };
    store(FRAME + 0xbb78, mode.raw());
    mode
}

fn clear_bytes(address: usize, length: usize) {
    for offset in 0..length { unsafe { write_volatile((address + offset) as *mut u8, 0); } }
}

fn copy_bytes(destination: usize, source: usize, length: usize) {
    for offset in 0..length {
        let value = byte(source + offset);
        unsafe { write_volatile((destination + offset) as *mut u8, value); }
    }
}

fn update_transient(backing: usize, id: u32, enabled: bool) {
    let quotient = id / 63;
    if quotient >= 2 { return; }
    let bit = (id + quotient) & 63;
    let address = backing + quotient as usize * 8 + (bit / 32) as usize * 4;
    let mask = 1 << (bit % 32);
    let value = load(address);
    store(address, if enabled { value | mask } else { value & !mask });
}

/// The mode frame, three transient bitmaps and mode-name table must be mapped.
pub unsafe fn commit_mode() {
    let previous = load(FRAME + 0xbb70);
    store(FRAME + 0xbb74, previous);
    let old = load(FRAME + 0xbb6c);
    store(FRAME + 0xbb70, old);
    let current = load(FRAME + 0xbb78);
    store(FRAME + 0xbb6c, current);
    store(FRAME + 0xbb78, 0);
    unsafe { write_volatile((FRAME + 0xbb90) as *mut u8, 0); }
    store(FRAME + 0xbb94, 0);
    clear_bytes(0x0095_0118, 16);
    clear_bytes(0x0095_0128, 16);
    clear_bytes(0x0095_0138, 16);
    store(FRAME + 0xbbc8, 0);
    store(FRAME + 0xd024, 0);
    store(FRAME + 0xcfec, 0);
    if !matches!(old, 1 | 2) { clear_bytes(FRAME + 0x5858, 0x5808); }
    clear_bytes(0x0095_03e8, 60);
    let category = load(FRAME + 0xbe20);
    if (old == 3 && category == 5) || (old == 4 && category == 6)
        || (matches!(old, 5 | 6) && matches!(category, 1 | 2 | 3 | 7)) {
        store(FRAME + 0xbe20, 0);
    }
    let primary = matches!(old, 3 | 5 | 7 | 12);
    if current == 4 {
        let token = if primary { Some(224) } else if matches!(old, 1 | 2) { Some(225) }
            else if old == 6 { Some(if load(FRAME + 0xe464) == 0x804 && load(FRAME + 0xe490) != 1 { 21 } else { 224 }) }
            else { None };
        if let Some(token) = token { store(FRAME + 0xe448, token); }
    }
    if matches!(current, 4 | 6) && primary {
        unsafe { write_volatile((FRAME + 0xe488) as *mut u8, 0); }
    }
    if old != 6 || current != 4 { store(FRAME + 0xe484, 0); }
    let name = match current {
        3 => Some(0x007a_147c), 4 => Some(0x007a_169d),
        5 | 6 => Some(0x007a_18be), 7 => Some(0x007a_1adf),
        8 => Some(0x007a_1d00), _ => None,
    };
    if let Some(name) = name { copy_bytes(0x0095_0150, name, 0x221); }
    if current == 3 {
        if load(FRAME + 0xbdf4) != 0 { update_transient(0x0095_0118, 27, true); }
        if load(FRAME + 0xbdf8) != 0 { update_transient(0x0095_0128, 27, true); }
    }
}

/// The mode frame and owned property service must be mapped.
pub unsafe fn submit_mode_request(request: ModeRequest) -> Result<Mode, u32> {
    store(FRAME + 0xbb80, request as u32);
    if request == ModeRequest::PowerOff { return Ok(unsafe { request_mode14() }); }
    let raw = load(FRAME + 0xbb78);
    let Some(pending) = Mode::parse(raw) else { return Err(raw); };
    let next = requested_mode(pending, request, PowerOffConditions { delay_nonzero: false, flag23: false, flag24: false, flag27: false });
    if next != pending { store(FRAME + 0xbb78, next.raw()); }
    Ok(next)
}

/// The runtime settings, mode frame and power controller backing must be mapped.
pub unsafe fn initialize_mode_policy(preserve: bool) -> Result<Mode, u32> {
    unsafe { submit_mode_request(ModeRequest::Initial)?; }
    if preserve { return Mode::parse(load(FRAME + 0xbb78)).ok_or(load(FRAME + 0xbb78)); }
    let flag = |id| unsafe { query_flag(FlagId::new(id)) };
    if flag(6) == 0 {
        if unsafe { system_source_mode() } == 2 {
            unsafe { submit_mode_request(ModeRequest::Request13)?; }
        } else {
            unsafe { submit_mode_request(ModeRequest::PowerOff)?; }
            store(FRAME + 0xbf10, 1);
        }
    }
    if flag(1536) != 0 { unsafe { submit_mode_request(ModeRequest::PowerOff)?; } }
    if flag(13) != 0 { unsafe { submit_mode_request(ModeRequest::Initial)?; } store(FRAME + 0xbe30, 1); }
    if flag(14) != 0 { unsafe { submit_mode_request(ModeRequest::Alternate)?; } store(FRAME + 0xbe30, 0); }
    if flag(16) != 0 { unsafe { submit_mode_request(ModeRequest::Playback)?; } }
    if flag(0x339) != 0 { unsafe { submit_mode_request(ModeRequest::Request12)?; } }
    if unsafe { setting(SettingId::Setting1) } == 0 { store(FRAME + 0xbe20, 1); }
    else if unsafe { setting(SettingId::Setting2) } == 0 { store(FRAME + 0xbe20, 2); }
    let half = unsafe { read_volatile((0x0096_b400 + 24) as *const u16) };
    if (half == 0xff80 && flag(16) == 1) || (flag(2) == 1 && flag(16) == 1 && flag(8) == 1) {
        return unsafe { submit_mode_request(ModeRequest::Request12) };
    }
    if flag(528) == 1 && load(FRAME + 0xbb78) != 11 {
        return unsafe { submit_mode_request(ModeRequest::PowerOff) };
    }
    if byte(0x0096_b400 + 0x29) & 2 != 0 && flag(9) == 1 {
        return unsafe { submit_mode_request(ModeRequest::Request10) };
    }
    if matches!(load(FRAME + 0xbe20), 1 | 2 | 3) {
        return unsafe { submit_mode_request(ModeRequest::Capture) };
    }
    let raw = load(FRAME + 0xbb78);
    Mode::parse(raw).ok_or(raw)
}

unsafe extern "C" {
    fn xm1_storage_initialize();
}

fn store_byte(address: usize, value: u8) { unsafe { write_volatile(address as *mut u8, value); } }
fn store_half(address: usize, value: u16) { unsafe { write_volatile(address as *mut u16, value); } }

/// The owned settings loader and fixed UI, menu and category windows must be mapped.
pub unsafe fn initialize_frame_policy() {
    unsafe { xm1_storage_initialize(); }
    clear_bytes(FRAME, 0x107f8);
    clear_bytes(FRAME, 80);
    clear_bytes(FRAME + 0x50, 0x5808);
    clear_bytes(FRAME + 0x5858, 0x5808);
    store(FRAME + 0xb060, 0);
    clear_bytes(FRAME + 0xb064, 80);
    clear_bytes(FRAME + 0xbb1c, 80);
    clear_bytes(FRAME + 0xb0b4, 20);
    for offset in [0xb0cc, 0xb0d0, 0xb0d4, 0xb0d8, 0xb0dc] { store(FRAME + offset, 0); }
    for (table, bytes, count, tag) in [
        (FRAME + 0xb158, FRAME + 0xb278, 28, 0x1000),
        (0x0094_f710, FRAME + 0xb294, 22, 0x2000),
        (0x0094_f73c, FRAME + 0xb2aa, 47, 0x4000),
        (0x0094_f79a, FRAME + 0xb2d9, 16, 0x8000),
        (0x0094_f7ba, FRAME + 0xb2e9, 9, 0x9000),
        (0x0094_f7cc, FRAME + 0xb2f2, 22, 0xa000),
    ] {
        for index in 0..count {
            store_half(table + index * 2, (tag | (index << 6)) as u16);
            store_byte(bytes + index, 0);
        }
    }
    clear_bytes(0x0094_f888, 48);
    for offset in (0xb338..=0xb350).step_by(4) { store(FRAME + offset, 0); }
    for offset in (0xb3a8..=0xb4d4).step_by(4) { store(FRAME + offset, 0); }
    store(FRAME + 0xbb6c, 1);
    for offset in [0xbb70, 0xbb74, 0xbb78, 0xbb80] { store(FRAME + offset, 0); }
    store_byte(FRAME + 0xbb90, 0);
    for offset in [0xbeec, 0xbf0c, 0xbf10, 0xbef4] { store(FRAME + offset, 0); }
    store(FRAME + 0xbb94, 0);
    clear_bytes(0x0095_0118, 16);
    clear_bytes(0x0095_0128, 16);
    clear_bytes(0x0095_0138, 16);
    store(FRAME + 0xbbc8, 0);
    store(FRAME + 0xbdf4, 0);
    store(FRAME + 0xbdf8, 0);
    for offset in [0xbdfc, 0xbdfd] { store_byte(FRAME + offset, 0); }
    for offset in [0xbe04, 0xbe10, 0xbe14, 0xbe0c, 0xbe18, 0xbe1c, 0xbe20] { store(FRAME + offset, 0); }
    store_byte(FRAME + 0xbe24, 0);
    store(FRAME + 0xbe28, 0);
    store_byte(FRAME + 0xbe34, 0);
    store(FRAME + 0xbe30, 2);
    store(FRAME + 0xbe38, 0);
    for offset in [0xbe36, 0xbe35, 0x10060, 0xfff8] { store_byte(FRAME + offset, 0); }
    for offset in [0xbe3c, 0xbe40, 0xbe44, 0xbe48, 0xbe4c, 0xbe58, 0xbe50, 0xbe54, 0xbe5c] { store(FRAME + offset, 0); }
    store_byte(FRAME + 0xbe60, 0);
    store(FRAME + 0x107f0, 0);
    store(FRAME + 0xcfa4, 129);
    store(FRAME + 0xbef0, 21);
    store(FRAME + 0xbf34, 0);
    store(FRAME + 0xd018, 0);
    store_byte(FRAME + 0xd028, 0);
    for offset in [0xe3f8, 0xda94, 0xe3d8, 0xe40c, 0xe3ec, 0xda68] { store(FRAME + offset, 0); }
    store_byte(FRAME + 0xe488, 1);
    store(FRAME + 0xe48c, 0);
    store(FRAME + 0xe490, 0);
    copy_bytes(0x0097_433c, 0x0095_b19c, 172);
    clear_bytes(FRAME + 0xbe68, 60);
    store(FRAME + 0xbeb8, 0);
    store(FRAME + 0xf268, 0);
    unsafe { initialize_menu_registry(); }
    store(FRAME + 0xfd20, 0);
    let mut packed = unsafe { setting(SettingId::Setting614) };
    for id in [SettingId::Setting613, SettingId::Setting612, SettingId::Setting611, SettingId::Setting610] { packed = unsafe { setting(id) } | (packed << 1); }
    store(FRAME + 0xe134, packed);
    store_byte(FRAME + 0xe130, 0);
    store(FRAME + 0xe4d0, 0);
    store(FRAME + 0xfd00, 0);
    store_byte(FRAME + 0xfd10, 0);
    store(FRAME + 0xff70, 0);
    clear_bytes(FRAME + 0xfd2c, 32);
    clear_bytes(FRAME + 0xffb8, 4);
    clear_bytes(FRAME + 0xffbc, 2);
    store_byte(FRAME + 0xffbe, 0);
    clear_bytes(FRAME + 0xffc0, 4);
    clear_bytes(FRAME + 0xffc4, 2);
    clear_bytes(FRAME + 0xffc6, 4);
    clear_bytes(FRAME + 0xffc4, 2);
    store_byte(FRAME + 0xffcc, 0);
    store(FRAME + 0xffd0, 0);
    for offset in [0xffd4, 0xffd5, 0xffd6] { store_byte(FRAME + offset, 0); }
    store(FRAME + 0xffe0, 0);
    store_byte(FRAME + 0xffdc, 0);
    for offset in [0xffe4, 0xffe8, 0xfd18, 0xff88, 0xff78, 0xff7c, 0xff80, 0xffa4, 0xfd70, 0xfd1c] { store(FRAME + offset, 0); }
    store(0x0093_c5c4 + 0x214, 0x0093_c0ec);
    store(0x0093_c5c4 + 0xfec, 0x0093_c0ec);
    store(FRAME + 0xe3e8, 0x0095_6900);
    for offset in [0xbeac, 0xe4a0, 0xe4b0, 0xe4b4, 0xe4bc, 0xe4c0, 0xe4c4] { store(FRAME + offset, 0); }
    store(FRAME + 0xcfe8, load(FRAME + 0xcfe4));
    store(FRAME + 0xcfe4, 0);
    store(FRAME + 0xda54, 0);
    store(FRAME + 0xcff0, 0);
    store(FRAME + 0xcff4, 1);
    store(FRAME + 0xda90, 38);
    for offset in [0xbebc, 0xbed0, 0xbed4] { store(FRAME + offset, 0); }
    store_byte(FRAME + 0xbed8, 0);
    store(FRAME + 0xbf2c, 0);
    store_half(FRAME + 0xbf38, 0);
    store(FRAME + 0xbf54, 0);
    store(FRAME + 0xbeb4, 0);
}

unsafe fn initialize_menu_registry() {
    const MENU: usize = 0x0095_71d0;
    clear_bytes(MENU, 1072);
    for index in 0..134 {
        let node = load(0x0081_babc + index * 4) as usize;
        if node != 0 {
            let values = load(node + 0x20) as usize;
            if values != 0 { store(MENU + index * 4 + 0x16c, load(values)); }
        }
    }
    let selection = unsafe { setting(SettingId::Setting645) };
    if load(MENU + 0x42c) == 1 {
        let node = load(0x0081_babc + 0x1e8) as usize;
        let values = load(node + 0x20) as usize;
        store(MENU + 0x354, load(values + selection as usize * 8));
    }
    copy_bytes(0x0096_b58c, MENU, 1072);
}

/// The token property projections and mode override cell must be mapped.
pub unsafe fn capture_mode_id(token: ModeToken) -> u32 {
    let value = match token.raw() {
        0x308 => 8, 0x309 => 11, 0x30a => 12, 0x30b => 14, 0x30d => 15,
        0x30e => 33, 0x30f => 34, 0x310 => 35, 0x312 => 36,
        0x306 => 1, 0x31d => 29, 0x314 => 44, 0x307 => 5,
        raw => {
            let id = match raw {
                0x304 => Some(SettingId::Setting225), 0x305 => Some(SettingId::Setting226),
                0x317 => Some(SettingId::Setting233), 0x318 => Some(SettingId::Setting234),
                0x319 => Some(SettingId::Setting235),
                0x31a if byte(0x0096_b400 + 0x76) == 0 => Some(SettingId::Setting235),
                0x311 => Some(SettingId::Setting232), 0x31b => Some(SettingId::Setting231),
                0x31c => Some(SettingId::Setting229), _ => None,
            };
            match id { Some(id) => unsafe { project_setting(id, setting(id), ProjectionColumn::Third) }, None => 0 }
        }
    };
    if load(0x0095_1f80 + 0x98) != 0 { 43 } else { value }
}


#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DisplayMode(u32);
impl DisplayMode { pub const fn new(raw: u32) -> Self { Self(raw) } }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DisplayProjection { pub primary: u32, pub secondary: u32 }

pub fn display_projection(mode: DisplayMode) -> DisplayProjection {
    let primary = match mode.0 {
        0 => 0, 2..=5 => mode.0, 6..=10 => mode.0 + 3, 11..=13 => mode.0 - 5,
        14..=22 => mode.0, 23 => 25, 24 => 26, 25 => 24, 26 => 23,
        27..=34 => mode.0, _ => 1,
    };
    let secondary = match mode.0 { 0 => 3, 5 => 2, 6 => 1, 12 => 4, 13 => 6, 22 | 26 | 30 => 0, _ => 5 };
    DisplayProjection { primary, secondary }
}

fn initialize_display_projection(output: usize) {
    let mode = DisplayMode(unsafe { setting(SettingId::Setting52) });
    let projected = display_projection(mode);
    store(output + 0x94, projected.primary);
    store(0x0095_66ac, projected.primary);
    store(output + 0x98, projected.secondary);
    store(0x0095_53c4, projected.secondary);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InitializationError { Configuration(i32), InputPolicy(i32), InvalidPending(u32) }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CaptureModeId(u32);
impl CaptureModeId {
    pub(crate) const fn new(raw: u32) -> Self { Self(raw) }
    pub const fn raw(self) -> u32 { self.0 }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CaptureSubtype(u32);
impl CaptureSubtype {
    pub(crate) const fn new(raw: u32) -> Self { Self(raw) }
    pub const fn raw(self) -> u32 { self.0 }
}

fn publish_initial_output(output: usize) {
    let current = load(FRAME + 0xbb6c);
    let category = match current { 3 => 1, 4 => 2, 5 => 3, 6 => 4, 7 => 5, 8 => 6, 11 => 7, 2 => 8, _ => 0 };
    store(output, category);
    let flag = |id| unsafe { query_flag(FlagId::new(id)) };
    let source_mode = matches!(load(FRAME + 0xbb6c), 4 | 6);
    let setting70 = if flag(26) == 1 && source_mode {
        match cached_setting70_token(load(0x0093_a760 + 0x18)) {
            Some(token) => { store(0x0089_edd0, token); token }
            None => load(0x0089_edd0),
        }
    } else {
        unsafe { project_setting(SettingId::Setting70, setting(SettingId::Setting70), ProjectionColumn::Third) }
    };
    store(output + 0xc, setting70);
    let (token, input) = if flag(25) == 1 { (128, 1) } else if flag(26) == 1 && source_mode { (128, 2) } else { (129, 0) };
    store(output + 0x10, token);
    store(FRAME + 0xbe04, input);
    store(output + 4, u32::from(load(FRAME + 0xbebc) == 1));
    store(output + 0xe0, 0);
    let selected = unsafe { setting(if matches!(current, 3 | 5) { SettingId::Setting492 } else { SettingId::Setting493 }) };
    let choice = selected == 1 || (selected == 2 && flag(51) == 1);
    store(output + 0x14, if choice { 203 } else { 202 });
    store(output + 0x18, unsafe { setting(if choice { SettingId::Setting49 } else { SettingId::Setting48 }) });
    store(output + 0x14, 202);
    store(output + 0x18, unsafe { setting(SettingId::Setting48) });
    store(output + 0x20, unsafe { project_setting(SettingId::Setting41, setting(SettingId::Setting41), ProjectionColumn::Second) });
    store(output + 0x38, unsafe { project_setting(SettingId::Setting8, setting(SettingId::Setting8), ProjectionColumn::Second) });
    store(output + 0x3c, unsafe { setting(SettingId::Setting207) });
    store(output + 0x90, 129);
    for (offset, address) in [
        (0x40, 0x0091_8ab8), (0x44, 0x0093_c5c4), (0x48, 0x0095_b3a0),
        (0x4c, 0x0095_6838), (0x50, 0x0091_845c), (0x54, 0x0094_32a0),
        (0x8c, 0x0093_e174), (0x64, 0x0096_e0ac), (0x70, FRAME),
        (0x74, 0x0096_b400), (0x78, 0x0095_b098), (0x7c, 0x0093_bea0),
        (0x80, 0x0095_d460), (0x88, 0x0095_4d78),
    ] { store(output + offset, address as u32); }
    initialize_display_projection(output);
    if current == 3 {
        for (destination, source) in [
            (0x9c, 0), (0xa4, 0x10), (0xac, 0x128), (0xb0, 0x12c),
            (0xb4, 0x60), (0xb8, 0x7c), (0xbc, 0x118), (0xc4, 0x3b0),
            (0xd4, 0x148), (0xa0, 0x90), (0xd8, 0x134), (0xa8, 0x4c),
            (0xdc, 0x348), (0xe4, 0x314), (0xc8, 0xd4), (0xe8, 0xfc), (0xec, 0x3b8),
        ] { store(output + destination, load(0x0095_6838 + source)); }
    }
    store(output + 0x1c, unsafe { project_setting(SettingId::Setting30, setting(SettingId::Setting30), ProjectionColumn::Second) });
}

fn cached_setting70_token(selector: u32) -> Option<u32> {
    match selector { 0 => Some(212), 3 => Some(210), 4 => Some(211), 5 => Some(213), 6 => Some(214), 9 => Some(216), 10 => Some(217), _ => None }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InitializationPolicy { Reevaluate, PreservePending }

/// A nonnull output covers 240 writable bytes. The subsystem windows and owned initialization services must be mapped.
pub unsafe fn initialize_mode(output: *mut u32, policy: InitializationPolicy) -> Result<(), InitializationError> {
    for (address, value) in [
        (0x0095_6838 + 0x174, 0x409a_dde0),
        (0x0095_6838 + 0x55c, 0x409a_e568),
        (0x0095_a6b4 + 0x174, 0x409a_ecf0),
        (0x0094_32a0 + 0x944, 0x409a_f478),
        (0x0093_a954 + 0x174, 0x409a_fc00),
        (0x0091_845c + 0x174, 0x409b_0388),
    ] { store(address, value); }
    let configuration = unsafe { crate::mode_initialization_projector::initialize_runtime_configuration() };
    if configuration != 0 { return Err(InitializationError::Configuration(configuration)); }
    store(0x0089_f7c0, 2);
    unsafe {
        initialize_frame_policy();
        crate::input_initialization::refresh_system_snapshot();
    }
    let input = unsafe { crate::input_initialization::initialize_input_policy() };
    if input != 0 { return Err(InitializationError::InputPolicy(input)); }
    unsafe { initialize_mode_policy(policy == InitializationPolicy::PreservePending) }.map_err(InitializationError::InvalidPending)?;
    unsafe {
        crate::input_initialization::initialize_input_categories();
        commit_mode();
    }
    if load(FRAME + 0xbb6c) == 3 {
        let token = unsafe { active_mode_token() };
        let mode = unsafe { capture_mode_id(token) };
        let subtype = crate::input_initialization::category_number(FlagId::new(token.raw()));
        unsafe { crate::capture_initialization::initialize_capture(CaptureModeId::new(mode), CaptureSubtype::new(subtype)); }
    }
    unsafe { crate::mode_initialization_record::write_initial_setting_record(); }
    if !output.is_null() { publish_initial_output(output as usize); }
    Ok(())
}

#[cfg(test)]
#[path = "mode_initialization_tests.rs"]
mod tests;
