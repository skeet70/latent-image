use core::ptr::{read_volatile, write_volatile};
use crate::mode_initialization::{self as mode, FlagId, ProjectionColumn, SettingId};

const FRAME: usize = 0x944580;
const SNAPSHOT: usize = 0x93bea0;
const POLICY_RECORD_BYTES: usize = 76;
const POLICY_CAPACITY: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KeyBit(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BitmapOperation { Clear, Any, Set, Reset, Query }
impl BitmapOperation {
    pub const fn from_abi(raw: u32) -> Self {
        match raw { 64 => Self::Clear, 46 => Self::Any, 128 => Self::Set, 129 => Self::Reset, _ => Self::Query }
    }
}

pub fn bitmap(words: &mut [u64], key: KeyBit, operation: BitmapOperation) -> u64 {
    let index = key.0 / 63;
    if index as usize >= words.len() { return 0; }
    let bit = key.0.wrapping_add(index) & 63;
    let mask = 1u64 << bit;
    match operation {
        BitmapOperation::Clear => { words.fill(0); 0 }
        BitmapOperation::Any => words.iter().fold(0, |all, word| all | word),
        BitmapOperation::Set => { words[index as usize] |= mask; 0 }
        BitmapOperation::Reset => { words[index as usize] &= !mask; 0 }
        BitmapOperation::Query => words[index as usize] & mask,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PolicyMutation { Enable, Disable, Preserve }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PolicyRecord {
    pub id: FlagId,
    pub mutation: PolicyMutation,
    pub argument: u32,
    pub encoded: u32,
}
impl PolicyRecord {
    pub fn decode(encoded: u32, argument: u32) -> Self {
        let (id, modifier) = if encoded & !0xc0 < 0x600 { (encoded & !0xc0, encoded & 0xc0) } else { (encoded, 0) };
        let mutation = match modifier { 0 | 0x40 => PolicyMutation::Enable, 0x80 => PolicyMutation::Disable, _ => PolicyMutation::Preserve };
        Self { id: FlagId::new(id), mutation, argument, encoded }
    }
}

fn load(address: usize) -> u32 { unsafe { read_volatile(address as *const u32) } }
fn store(address: usize, value: u32) { unsafe { write_volatile(address as *mut u32, value) } }
fn half(address: usize) -> u16 { unsafe { read_volatile(address as *const u16) } }
fn store_half(address: usize, value: u16) { unsafe { write_volatile(address as *mut u16, value) } }
fn flag(id: u32) -> u32 { unsafe { mode::query_flag(FlagId::new(id)) } }

unsafe extern "C" {
    fn xm1_power_interrupt_lock() -> u32;
    fn xm1_power_interrupt_unlock(saved: u32);
    fn xm1_power_application_post(event: u32, argument: u32) -> i32;
}

/// The owned property service, snapshot and UI frame must be mapped.
pub unsafe fn refresh_system_snapshot() {
    use SettingId::*;
    for (id, offset) in [(Setting70, 0), (Setting72, 12), (Setting75, 16), (Setting8, 4)] {
        store(SNAPSHOT + offset, unsafe { mode::project_setting(id, mode::setting(id), ProjectionColumn::Second) });
    }
    store(SNAPSHOT + 8, 0);
    for (id, offset) in [(Setting212, 20), (Setting74, 24), (Setting206, 28), (Setting176, 32), (Setting30, 36)] {
        store(SNAPSHOT + offset, unsafe { mode::project_setting(id, mode::setting(id), ProjectionColumn::Second) });
    }
    store(SNAPSHOT + 40, 1);
    store(SNAPSHOT + 48, unsafe { mode::project_setting(Setting245, mode::setting(Setting245), ProjectionColumn::Second) });
    unsafe { write_volatile((FRAME + 0xbdfc) as *mut u8, 1) };
}

/// Both policy buffers and their count words must be mapped.
pub unsafe fn drain_policy_records() {
    let saved = unsafe { xm1_power_interrupt_lock() };
    for offset in (0..0x1304).step_by(4) { store(0x95b460 + offset, load(0x913520 + offset)); }
    store(0x913520 + 0x1300, 0);
    unsafe { xm1_power_interrupt_unlock(saved) };
}

/// The policy buffers, UI frame and owned transition producer must be available.
pub unsafe fn initialize_input_policy() -> i32 {
    unsafe { drain_policy_records() };
    let mut index = 0;
    while index < load(0x95b460 + POLICY_CAPACITY * POLICY_RECORD_BYTES) {
        let address = 0x95b460 + index as usize * POLICY_RECORD_BYTES;
        let record = PolicyRecord::decode(load(address), load(address + 4));
        let id = record.id.raw();
        if id.wrapping_sub(0x600) <= 3 {
            store(FRAME + (id as usize + 9958) * 4, 1);
        } else {
            match record.mutation {
                PolicyMutation::Enable => unsafe { mode::set_flag(record.id, true) },
                PolicyMutation::Disable => unsafe { mode::set_flag(record.id, false) },
                PolicyMutation::Preserve => (),
            }
        }
        if id == 17 { unsafe { xm1_power_application_post(record.encoded, record.argument); } }
        index += 1;
    }
    if flag(15) != 0 { store(FRAME + 0xbe28, 1); store(FRAME + 0xbe1c, 4); }
    else { store(FRAME + 0xbe28, 0); }
    for (id, policy) in [(0x601, 1), (0x602, 2), (0x603, 3)] {
        if flag(id) != 0 {
            if policy == 3 || load(FRAME + 0xbe1c) != 3 { store(FRAME + 0xbe1c, policy); }
            unsafe { xm1_power_application_post(id, 0); }
        }
    }
    if flag(56) != 0 { store(FRAME + 0xbf54, 1); }
    0
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyChannel { Channel0, Channel1, Channel2, Channel3 }
impl KeyChannel {
    pub const fn parse(raw: u32) -> Option<Self> {
        match raw { 0 => Some(Self::Channel0), 1 => Some(Self::Channel1), 2 => Some(Self::Channel2), 3 => Some(Self::Channel3), _ => None }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyLevel { Low, High, Unavailable }

/// The key configuration, retained pin handle and input registers must be mapped.
pub unsafe fn sample_key(channel: KeyChannel) -> KeyLevel {
    let channel = channel as usize;
    match load(0x917f6c + channel * 20) {
        5 => KeyLevel::Unavailable,
        6 => if load(0xfff89314) & 0x80 != 0 { KeyLevel::High } else { KeyLevel::Unavailable },
        1 => {
            if load(0x8297f0) == u32::MAX && load(0x8297f4) == u32::MAX {
                let saved = unsafe { xm1_power_interrupt_lock() };
                store(0xfff88028, load(0xfff88028) & !0x03000000);
                unsafe { xm1_power_interrupt_unlock(saved) };
                let mut handle = 0;
                if unsafe { xm1_power_resource_acquire(34, &mut handle) } == 0 {
                    store(0x8297f0, handle);
                    store(0x8297f4, 0);
                }
            }
            if load(0xfff52014) & 4 != 0 { KeyLevel::High } else { KeyLevel::Unavailable }
        }
        _ => KeyLevel::Low,
    }
}

unsafe extern "C" {
    fn xm1_storage_apply(id: u32, value: u32, bank: u32) -> u32;
    fn xm1_setter_owned(id: u32, value: u32, bank: u32, selection: u32) -> u32;
    fn xm1_power_resource_acquire(id: u32, handle: *mut u32) -> i32;
}

fn first_flag(table: usize, stride: usize, fallback: u32) -> u32 {
    let mut row = table;
    loop {
        let id = load(row);
        if id == 0 { return fallback; }
        if flag(id) == 1 { return id; }
        row += stride;
    }
}

pub fn category_number(id: FlagId) -> u32 {
    match id.raw() {
        0x304 => 1, 0x305 => 2, 0x317 => 3, 0x318 => 4, 0x319 => 5,
        0x31a => 6, 0x311 => 7, 0x31b => 8, 0x31c => 9, _ => 0,
    }
}

/// The category tables, UI frame, owned properties and key input registers must be mapped.
pub unsafe fn initialize_input_categories() {
    let raw_channel = unsafe { read_volatile(0x814732 as *const u8) };
    let key_status = match KeyChannel::parse(u32::from(raw_channel)) {
        Some(channel) => unsafe { sample_key(channel) },
        None => KeyLevel::Low,
    };
    if key_status == KeyLevel::Unavailable { store(0x918ab8 + 0x44, 3); }
    for offset in (0..112).step_by(4) { store(0x918b28 + offset, load(0x918ab8 + offset)); }
    for offset in (0..52).step_by(4) {
        let value = load(SNAPSHOT + offset);
        store(0x943190 + offset, value);
        store(0x9431c4 + offset, value);
    }
    let selection = load(0x956838);
    let setting_id = match selection {
        31 => Some(SettingId::Setting357),
        33..=37 => Some(SettingId::Setting353),
        _ => None,
    };
    let value = setting_id.map(|id| unsafe { mode::setting(id) }).unwrap_or(0);
    let offset = match selection { 33 | 37 => Some(0xd378), 36 => Some(0xd37c), 35 => Some(0xd380), 34 => Some(0xd384), _ => None };
    if let Some(offset) = offset { store(FRAME + offset, value); }
    for id in 0x302..=0x31d {
        if flag(id) != 0 { store(FRAME + 0xbea4, id); break; }
    }
    store(FRAME + 0xbeec, flag(51));
    store(FRAME + 0xbedc, first_flag(0x77496c, 4, load(0x774984)));
    store(FRAME + 0xbee0, first_flag(0x79fed4, 8, load(0x79fed4)));
    store(FRAME + 0xbee8, flag(0x425));
    store_half(FRAME + 0xbfb6, 0);
    let current = load(FRAME + 0xbb78);
    store(FRAME + 0xbb7c, current);
    if load(FRAME + 0xbe04) != 2 && unsafe { mode::setting(SettingId::Setting217) } != 1 {
        let enabled = match current {
            3 | 5 => half(0x96b414) != 0,
            4 | 6 => half(0x96b416) != 0,
            _ => false,
        };
        if enabled { store(FRAME + 0xbebc, 1); }
    }
    store(FRAME + 0xbe30, 2);
    unsafe { xm1_storage_apply(0x282, mode::project_setting_columns(SettingId::Setting642, 1, ProjectionColumn::Index, ProjectionColumn::First), 0); }
    let token = unsafe { mode::active_mode_token() };
    let category = category_number(FlagId::new(token.raw()));
    store(FRAME + 0xcfe8, load(FRAME + 0xcfe4));
    store(FRAME + 0xcfe4, category);
    unsafe {
        xm1_storage_apply(314, 0x1affffff, 0);
        xm1_storage_apply(19, 0x1affffff, 0);
        xm1_setter_owned(19, 0x1affffff, 0, 0);
        xm1_setter_owned(22, 0x1affffff, 0, 0);
    }
}

#[cfg(power_native)]
#[no_mangle]
pub unsafe extern "C" fn xm1_power_refresh_system_snapshot() { unsafe { refresh_system_snapshot() } }
#[cfg(power_native)]
#[no_mangle]
pub unsafe extern "C" fn xm1_power_initialize_input_policy() -> i32 { unsafe { initialize_input_policy() } }
#[cfg(power_native)]
#[no_mangle]
pub unsafe extern "C" fn xm1_power_initialize_input_categories() { unsafe { initialize_input_categories() } }

/// `words` must cover `count` writable words with exclusive access.
#[cfg(power_native)]
#[no_mangle]
pub unsafe extern "C" fn xm1_power_input_bitmap(words: *mut u64, count: u32, key: u32, operation: u32) -> u64 {
    if key / 63 >= count { return 0; }
    let words = unsafe { core::slice::from_raw_parts_mut(words, count as usize) };
    bitmap(words, KeyBit(key), BitmapOperation::from_abi(operation))
}

#[cfg(test)]
#[path = "input_initialization_tests.rs"]
mod tests;
