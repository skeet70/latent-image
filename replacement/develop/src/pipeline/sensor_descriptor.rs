use super::shared::{Address32, ByteOffset};
use super::sensor_requests::{byte, configuration_byte, copy, half, store, store_byte, store_half, word, ConfigurationKey, PipelineDescriptor, SettingsPacket};

#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct SettingsKind(pub u32);
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct DescriptorKind(pub u32);
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct ImportedLevel(pub u32);
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct LevelInput(pub i32);
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct LevelAdjustmentTenths(pub i32);
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct LevelIndex(pub u32);
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct LevelLookup(pub Address32);
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct TranslatedLevel(pub u32);

unsafe extern "C" {
    fn xm1_pipeline_image_scan_translate_small_kind(output: Address32, kind: u32);
}

fn checked_kind(value: u32) -> u32 { match value { 0 | 1 | 3 | 4 => value, _ => 2 } }
fn reversed_kind(value: u32) -> u32 { match value { 0 => 4, 1 => 3, 3 => 1, 4 => 0, _ => 2 } }
fn disabled_option(_value: u32) -> u32 { 0 }
fn option_kind(value: u32) -> u32 { match value { 1 => 5, 3 => 4, 4 | 7 | 8 | 9 | 10 => 2, 5 => 3, 6 => 1, 11 => 6, 12 => 7, _ => 0 } }

fn translated_format(kind: SettingsKind) -> DescriptorKind {
    DescriptorKind(match kind.0 {
        0x1f | 0x21 | 0x22 | 0x23 | 0x24 | 0x25 => 1, 2 => 2, 3 => 3, 4 => 4,
        5 | 0x4c => 5, 7 | 0x4d => 6, 0x32 | 0x33 | 0x56 | 0x57 => 7, 8 | 0x4e => 8,
        0x30 | 0x54 => 9, 0x31 | 0x55 => 10, 0xc | 0xd | 0x4f | 0x50 => 11, 0xb => 14,
        0xe => 16, 0x2e => 17, 0x2f => 18, 0xf => 19, 0x16 => 20, 0x3a => 21, 0x17 => 22,
        0x15 => 23, 0x14 => 24, 0x13 | 0x53 => 25, 0x38 | 0x5c => 26, 0x39 | 0x5d => 27,
        0x12 => 28, 0x10 | 0x51 => 29, 0x34 | 0x58 => 30, 0x35 | 0x59 => 31,
        0x11 | 0x52 => 32, 0x36 | 0x5a => 33, 0x37 | 0x5b => 34, 6 => 35, 0x3e => 36, 0x18 => 37,
        9 => 38, 10 => 39, 0x40 | 0x5e => 40, 0x41 | 0x5f => 41, 0x42 | 0x60 => 42,
        0x43 | 0x61 => 43, 0x44 | 0x62 => 44, 0x45 | 0x63 => 45, 0x46 | 0x64 => 46,
        0x47 | 0x65 => 47, 0x48 | 0x66 => 48, 0x1b => 49, 0x1c => 50, 0x49 | 0x67 => 51,
        0x2c => 52, 0x19 | 0x3f => 53, 0x1a => 54, 0x1d => 55, _ => 0,
    })
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_sensor_translate_format(output: Address32, kind: SettingsKind) { unsafe { store(output, 0, translated_format(kind).0) }; }

unsafe fn source_height(source: Address32) -> u32 {
    if source.0 == 0 { return 0; }
    let count = unsafe { word(source, 0) };
    if count == 0 || count & 255 == 0 { return 0; }
    let mut maximum = 0;
    for index in 0..(count & 255) { maximum = maximum.max(unsafe { half(source, index * 44 + 30) } as u32); }
    maximum
}
unsafe fn translate_table(value: u32) -> u32 {
    let table = Address32(0xd797a4);
    if value == 0 { return unsafe { half(table, 2) } as u32; }
    for index in 1..59 {
        if unsafe { half(table, index * 4) } as u32 == value { return unsafe { half(table, index * 4 + 2) } as u32; }
    }
    0
}
unsafe fn threshold_band(value: u8) -> u8 {
    let threshold = value.wrapping_sub(1) as u32;
    for index in 0..14 { if threshold <= unsafe { configuration_byte(ConfigurationKey(0x3e3d40 + index)) } { return index as u8; } }
    14
}
fn imported_mode(value: ImportedLevel) -> u32 { match value.0 { 0..=4 => 0, 8 => 2, 11..=25 => value.0 - 7, 27 => 20, 28 => 21, _ => 3 } }
fn option_selection(value: u32) -> u32 { match value { 2 => 0, 3 => 1, 4 => 4, 5 => 3, _ => 2 } }
fn extended_option(value: u32) -> u32 { match value { 1 => 1, 2 => 2, 12 => 3, 3 => 4, 4 => 5, 5 => 6, 13 => 7, 6 => 8, 9 => 9, 11 => 10, 7 => 11, 8 => 12, 10 => 13, _ => 0 } }

#[unsafe(no_mangle)]
pub extern "C" fn xm1_pipeline_sensor_profile_zero_first() -> u32 { 0 }
#[unsafe(no_mangle)]
pub extern "C" fn xm1_pipeline_sensor_profile_zero_second() -> u32 { 0 }

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_sensor_apply_settings(descriptor: PipelineDescriptor, packet: SettingsPacket, parameters: Address32) {
    let output = descriptor.0;
    let input = packet.0;
    unsafe {
        store(output, 0xec, word(input, 0x50));
        xm1_pipeline_sensor_translate_format(output.offset(ByteOffset(0xe4)), SettingsKind(word(input, 0x14)));
        store(output, 0xdc, u32::from(word(input, 0) != 0));
        store(output, 0xe0, u32::from(word(input, 4) != 0));
        store(output, 0x150, source_height(Address32(word(input, 0x48))));
        xm1_pipeline_sensor_import_primary_level(descriptor, ImportedLevel(word(input, 0x18)), LevelInput(half(input, 0x100) as i32));
        store(output, 0x130, half(input, 0x100) as u32);
        xm1_pipeline_sensor_import_strength(descriptor, ImportedLevel(word(input, 0x18)), LevelInput(word(input, 0x108) as u16 as i32));
        xm1_pipeline_sensor_import_level(descriptor, ImportedLevel(word(input, 0x18)), LevelInput(half(input, 0x100) as i32), SettingsKind(word(input, 0x14)), byte(input, 0x64));
        let _ = word(input, 0x20);
        store(output, 0xf8, 2);
        store(output, 0xfc, checked_kind(word(input, 0x30)));
        store(output, 0x100, checked_kind(word(input, 0x34)));
        store(output, 0x10c, option_selection(word(input, 0xc)));
        store(output, 0x108, disabled_option(word(input, 0x38)));
        store(output, 0x110, disabled_option(word(input, 0x24)));
        store(output, 0x114, reversed_kind(word(input, 0x10)));
        store_byte(output, 0x11b, byte(input, 0x114));
        store(output, 0x11c, word(input, 0xf8));
        store(output, 0x120, word(input, 0xfc));
        store_byte(output, 0x11a, byte(input, 0xf4));
        store_half(output, 0x118, half(input, 0x102));
        store(output, 0x144, 0);
        store(output, 0x124, translate_table(word(input, 0x10c)));
        store(output, 0x104, option_kind(word(input, 0x24)));
        xm1_pipeline_image_scan_translate_small_kind(output.offset(ByteOffset(0x154)), word(input, 0x110));
        store(output, 0x15c, u32::from(word(input, 0x54) != 0));
        store(output, 0x160, word(input, 0x58).min(3));
        store(output, 0x164, u32::from(word(input, 0x5c) != 0));
        store_half(output, 0x168, half(input, 0x60));
        store_half(output, 0x16a, half(input, 0x62));
        store_byte(output, 0x16c, threshold_band(byte(input, 0xf4)));
        store(output, 0x17c, u32::from(word(input, 0x68) != 0));
        store(output, 0x18c, u32::from(word(input, 0x6c) != 0));
        store_byte(output, 0x190, byte(input, 0x70));
        store(output, 0x194, checked_kind(word(input, 0x2c)));
        store(output, 0x128, u32::from(word(input, 0x28) == 1));
        store_byte(output, 0x198, byte(input, 0x104));
        let selected = if word(input, 0x5c) == 1 { 0x338 } else if word(input, 0x54) == 1 { 0x28 } else { 0x1b0 };
        store_half(output, 0x19a, half(input, 0x158 + selected));
        store(output, 0x19c, imported_mode(ImportedLevel(word(input, 0x18))));
        store_half(output, 0x1a0, half(input, 0x5f4));
        store_byte(output, 0x1a2, byte(input, 0x5f6));
        store_half(output, 0x1a6, 100);
        store_half(output, 0x1a4, half(input, 0x5f8));
        store(output, 0x1b8, word(input, 0x90));
        store(output, 0x1bc, u32::from(word(input, 0x5fc) != 0));
        store_half(output, 0x1c0, half(input, 0x94));
        store_half(output, 0x1c2, half(input, 0x96));
        store_byte(output, 0x1ac, byte(input, 0x71));
        store(output, 0x1a8, word(input, 0x74));
        store_byte(output, 0x1ad, byte(input, 0x600));
        store_byte(output, 0x1ae, byte(input, 0x78));
        store_byte(output, 0x1af, byte(input, 0x79));
        let profile = if byte(output, 0x1ae) == 1 { byte(output, 0x1af) as u32 } else { configuration_byte(ConfigurationKey(0x18ac0)) };
        let limit = configuration_byte(ConfigurationKey(0xbbf6));
        store(output, 0x1e0, u32::from((limit.wrapping_sub(10) as u8) < 6 && profile >= limit));
        store(output, 0xe8, extended_option(word(input, 0x3c)));
        store(output, 0x230, half(input, 0x154) as i16 as i32 as u32);
        let _ = word(parameters, 0x40);
        let _ = word(input, 0xf8);
        store(output, 0x170, xm1_pipeline_sensor_profile_zero_first());
        let _ = word(parameters, 0x40);
        store(output, 0x174, xm1_pipeline_sensor_profile_zero_second());
        let value = word(input, 0x7c);
        store(output, 0x1b0, if value <= 2 { value } else { 0 });
        let value = word(input, 0x80);
        store(output, 0x1b4, if value <= 1 { value } else { 0 });
        store_half(output, 0x1c8, 0);
        store_half(output, 0x1ca, 0);
        store_half(output, 0x1cc, 0);
        let value = word(input, 0xa4);
        store(output, 0x1d0, if value <= 1 { value } else { 0 });
        store(output, 0x228, 1);
        store(output, 0x22c, 0);
        let value = word(input, 0xa8);
        store(output, 0x1d4, if value <= 1 { value } else { 0 });
        let value = word(input, 0xac);
        store(output, 0x1d8, if value <= 2 { value } else { 0 });
        store(output, 0x1dc, 0);
        store(output, 0x1e4, word(input, 0x9c));
        store_half(output, 0x1e8, half(input, 0xa0));
        store_half(output, 0x1ea, half(input, 0xa2));
        copy(output.offset(ByteOffset(0x1ec)), input.offset(ByteOffset(0xb0)), 57);
        store(output, 0x238, word(input, 0xf0));
        store_half(output, 0x234, half(input, 0xea));
        store_half(output, 0x236, half(input, 0xec));
    }
}

fn translated_level(value: ImportedLevel) -> TranslatedLevel {
    TranslatedLevel(match value.0 { 0..=4 => 0, 6 | 7 => 1, 8 | 9 => 2, 13 => 6, 16 => 9, 19 => 12, 22 => 15, 25..=28 => value.0 - 7, _ => 3 })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_sensor_select_level(_kind: TranslatedLevel, value: LevelInput, lookup: LevelLookup) -> LevelIndex {
    let mut index = 0;
    let mut comparing = true;
    for threshold_index in 0..9 {
        let threshold = unsafe { super::sensor_requests::configuration_half(ConfigurationKey(0xa910 + threshold_index * 2)) };
        if comparing && value.0 >= threshold as i32 { index = threshold_index + 1; } else { comparing = false; }
    }
    LevelIndex(if lookup.0.0 == 0 { index } else { unsafe { word(lookup.0, index * 4) } })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_sensor_select_adjusted_level(kind: TranslatedLevel, value: LevelInput, adjustment: LevelAdjustmentTenths) -> LevelIndex {
    let level = unsafe { xm1_pipeline_sensor_select_level(kind, value, LevelLookup(Address32(0xd5e630))) };
    let adjusted = if adjustment.0 == 0 { level.0 } else { level.0.wrapping_add((adjustment.0 / 10 - 1) as u32) };
    LevelIndex(adjusted.min(9))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_sensor_compute_level(output: Address32, bucket: Address32, kind: TranslatedLevel, value: LevelInput, adjustment: LevelAdjustmentTenths) {
    let index = unsafe { xm1_pipeline_sensor_select_adjusted_level(kind, value, adjustment) };
    let bucket_value = if kind.0 == 0 || kind.0 == 22 {
        let baseline = unsafe { configuration_byte(ConfigurationKey(0xa922)) } as i32 * 100;
        if value.0 < baseline { 100 } else if value.0 >= baseline + 500 { 0 } else {
            let remainder = value.0 % 100;
            let third = if remainder <= 32 { 0 } else if remainder <= 66 { 1 } else { 2 };
            third + (value.0 - baseline) / 100 * 10
        }
    } else { 100 };
    unsafe { store(bucket, 0, bucket_value as u32); store(output, 0, index.0); }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_sensor_import_primary_level(descriptor: PipelineDescriptor, level: ImportedLevel, value: LevelInput) {
    unsafe { xm1_pipeline_sensor_compute_level(descriptor.0.offset(ByteOffset(0xf4)), descriptor.0.offset(ByteOffset(0x12c)), translated_level(level), value, LevelAdjustmentTenths(10)) };
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_sensor_import_strength(descriptor: PipelineDescriptor, level: ImportedLevel, value: LevelInput) {
    let selected = unsafe { xm1_pipeline_sensor_select_adjusted_level(TranslatedLevel(level.0), value, LevelAdjustmentTenths(10)) };
    unsafe { store(descriptor.0, 0x158, selected.0); }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_sensor_import_level_value(output: Address32, value: LevelInput, enabled: u8) {
    let key = match enabled { 1 => 0xa92d, 2 => 0xa92e, 3 => 0xa92f, _ => 0xa930 };
    let mut selected = (value.0 / 10).wrapping_sub(unsafe { configuration_byte(ConfigurationKey(key)) } as i32);
    if selected < unsafe { configuration_byte(ConfigurationKey(0xa931)) } as i32 { selected = unsafe { configuration_byte(ConfigurationKey(0xa931)) } as i32; }
    unsafe { store(output, 0, selected.wrapping_mul(10) as u32); }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_sensor_import_level(descriptor: PipelineDescriptor, level: ImportedLevel, value: LevelInput, kind: SettingsKind, enabled: u8) {
    if kind.0 != 0x3f && kind.0 != 0x19 { return; }
    unsafe {
        store_byte(descriptor.0, 0x178, enabled);
        xm1_pipeline_sensor_import_level_value(descriptor.0.offset(ByteOffset(0x188)), value, enabled);
        let imported = LevelInput(word(descriptor.0, 0x188) as i32);
        xm1_pipeline_sensor_compute_level(descriptor.0.offset(ByteOffset(0x180)), descriptor.0.offset(ByteOffset(0x184)), translated_level(level), imported, LevelAdjustmentTenths(10));
    }
}
