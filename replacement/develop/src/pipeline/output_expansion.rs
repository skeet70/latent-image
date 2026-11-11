use super::shared::{Address32, ByteOffset};

#[derive(Clone, Copy)]
#[repr(transparent)]
pub(super) struct OutputDescriptorAddress(pub(super) Address32);
#[derive(Clone, Copy)]
#[repr(transparent)]
pub(super) struct ExpandedOutputAddress(pub(super) Address32);
#[derive(Clone, Copy)]
#[repr(transparent)]
pub(super) struct ConfigurationKind(pub(super) u32);
#[derive(Clone, Copy)]
#[repr(transparent)]
pub(super) struct ConfigurationSelection(pub(super) u32);
#[derive(Clone, Copy)]
#[repr(transparent)]
pub(super) struct ConfigurationLimit(pub(super) u32);
#[derive(Clone, Copy)]
#[repr(transparent)]
pub(super) struct ThresholdFamily(pub(super) u32);
#[derive(Clone, Copy)]
#[repr(transparent)]
pub(super) struct ThresholdValue(pub(super) i32);
#[derive(Clone, Copy)]
#[repr(transparent)]
struct ConfigurationKey(u32);
#[derive(Clone, Copy)]
#[repr(transparent)]
struct CachedConfigurationSlot(u32);
#[derive(Clone, Copy)]
#[repr(transparent)]
struct ConfigurationKeyAddress(Address32);

unsafe extern "C" {
    fn xm1_buffers_configuration_byte(key: u32) -> u32;
    fn xm1_buffers_configuration_half(key: u32) -> u16;
    fn xm1_pipeline_sensor_configuration_key_bytes(destination: u32, keys: u32, count: u32);
    fn xm1_pipeline_sensor_configuration_key_halves(destination: u32, keys: u32, count: u32);
}

unsafe fn byte(base: Address32, offset: u32) -> u8 {
    unsafe { base.offset(ByteOffset(offset)).byte() }
}
unsafe fn half(base: Address32, offset: u32) -> u16 {
    unsafe { base.offset(ByteOffset(offset)).half() }
}
unsafe fn word(base: Address32, offset: u32) -> u32 {
    unsafe { base.offset(ByteOffset(offset)).word() }
}
unsafe fn store_byte(base: Address32, offset: u32, value: u8) {
    unsafe { base.offset(ByteOffset(offset)).write_byte(value); }
}
unsafe fn store_half(base: Address32, offset: u32, value: u16) {
    unsafe { base.offset(ByteOffset(offset)).write_half(value); }
}
unsafe fn store_word(base: Address32, offset: u32, value: u32) {
    unsafe { base.offset(ByteOffset(offset)).write_word(value); }
}
unsafe fn configuration_byte(key: ConfigurationKey) -> u8 {
    unsafe { xm1_buffers_configuration_byte(key.0) as u8 }
}
unsafe fn configuration_half(key: ConfigurationKey) -> u16 {
    unsafe { xm1_buffers_configuration_half(key.0) }
}

#[unsafe(no_mangle)]
pub(super) extern "C" fn xm1_pipeline_image_configuration_value_within_limit(value: ConfigurationSelection, limit: ConfigurationLimit) -> u32 {
    u32::from(value.0 != 0 && value.0 <= limit.0)
}

#[unsafe(no_mangle)]
pub(super) extern "C" fn xm1_pipeline_image_select_type_mode_value_copy(kind: ConfigurationKind, mode: ConfigurationSelection, value: u32, alternate: u32) -> u32 {
    if kind.0 == 0x35 && mode.0 == 1 { alternate } else { value }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_encode_configuration_mode(descriptor: OutputDescriptorAddress, output: Address32) {
    let descriptor = descriptor.0;
    let code = match unsafe { word(descriptor, 0xe4) } {
        5 => 1, 6 => 2, 0x25 => 3, 8 => 4, 9 => 5, 10 => 6, 14 => 7,
        11 | 12 => 8, 7 => 9, 13 => 10, 0x1d => 11, 0x1e => 12,
        0x1f => 13, 0x20 => 14, 0x21 => 15, 0x22 => 16, 0x1c => 17,
        0x19 => 18, 0x1a => 19, 0x1b => 20, 0x18 => 21, 0x17 => 22,
        0x14 => 23, 0x16 => 24, 0x28 => 25, 0x29 => 26, 0x2a => 27,
        0x2b => 28, 0x2c => 29, 0x2d => 30, 0x2e => 31, 0x2f => 32,
        0x30 => 33,
        _ => {
            unsafe { store_byte(output, 0, 0); }
            let secondary = unsafe { word(descriptor, 0xe8) };
            if (1..=13).contains(&secondary) {
                unsafe { store_byte(output, 0, secondary as u8 + 33); }
            }
            return;
        }
    };
    unsafe { store_byte(output, 0, code); }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_configuration_mode_value(descriptor: OutputDescriptorAddress, limit: ConfigurationLimit) -> u32 {
    let descriptor = descriptor.0;
    if unsafe { configuration_byte(ConfigurationKey(0xa923)) } == 0 {
        let value = unsafe { byte(descriptor, 0x11b) } as u32;
        return if limit.0 <= 11 { ((value + 2) / 3).min(10) } else { value.min(30) };
    }
    let value = unsafe { half(descriptor, 0x1ea) } as i16 as i32;
    let band = match value {
        ..=46 => 0u32, 47..=149 => 1, 150..=246 => 2, 247..=349 => 3,
        350..=446 => 4, 447..=549 => 5, 550..=641 => 6, 642..=749 => 7,
        750..=841 => 8, 842..=949 => 9, _ => 10,
    };
    if limit.0 <= 11 { band.min(10) } else if band == 0 { 0 } else { ((band * 3 - 2) as u8 as u32).min(30) }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_configuration_stage_mode(descriptor: OutputDescriptorAddress, output: Address32) {
    let descriptor = descriptor.0;
    let mode = unsafe { word(descriptor, 0x140) };
    let code = match mode {
        5 | 6 => 2,
        3 | 4 => if unsafe { word(descriptor, 0xdc) } == 1 { 2 } else { 1 },
        1 | 2 => 1,
        _ => if unsafe { word(descriptor, 0x1d0) } != 1 || unsafe { word(descriptor, 0x1d4) } == 0 { 0 }
            else if unsafe { word(descriptor, 0xdc) } == 1 { 2 } else { 1 },
    };
    unsafe { store_byte(output, 0, code); }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_configuration_kind_mode(descriptor: OutputDescriptorAddress, output: Address32) {
    let descriptor = descriptor.0;
    let code = match unsafe { word(descriptor, 0xe4) } {
        5 => 2, 6 => 3, 7 => 26, 8 => 4, 9 => 5, 10 => 6,
        11 | 12 => 25, 14 => 28, 0x14 => 32, 0x15 => 33, 0x16 => 34,
        0x17 => 31, 0x18 => 30, 0x19 => 13, 0x1a => 14, 0x1b => 15,
        0x1c => 29, 0x1d => 7, 0x1e => 8, 0x1f => 9, 0x20 => 10,
        0x21 => 11, 0x22 => 12, 0x25 => 27, 0x28 => 16, 0x29 => 17,
        0x2a => 18, 0x2b => 19, 0x2c => 20, 0x2d => 21, 0x2e => 22,
        0x2f => 23, 0x30 => 24, 0x31 | 0x32 => 35, 0x33 => 1,
        _ => match unsafe { word(descriptor, 0xe8) } {
            secondary @ 1..=7 => secondary as u8 + 35,
            8..=13 => 43,
            _ => u8::from(unsafe { word(descriptor, 0xec) } != 0),
        },
    };
    unsafe { store_byte(output, 0, code); }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_configuration_threshold_band(family: ThresholdFamily, value: ThresholdValue) -> u32 {
    let keys = Address32(0x00829ad8).offset(ByteOffset(family.0.wrapping_mul(16)));
    let first = unsafe { configuration_half(ConfigurationKey(word(keys, 0))) } as i32;
    let second = unsafe { configuration_half(ConfigurationKey(word(keys, 4))) } as i32;
    let third = unsafe { configuration_half(ConfigurationKey(word(keys, 8))) } as i32;
    let fourth = unsafe { configuration_half(ConfigurationKey(word(keys, 12))) } as i32;
    if first >= value.0 { 0 } else if value.0 <= second { 1 }
    else if value.0 <= third { 2 } else if value.0 <= fourth { 3 } else { 4 }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_configuration_threshold_bucket(value: ConfigurationSelection) -> u32 {
    let mut bucket = 0u32;
    while bucket < 9 {
        let key = ConfigurationKey(unsafe { word(Address32(0x00829a38), bucket * 4) });
        if unsafe { configuration_byte(key) } as u32 >= value.0 { break; }
        bucket += 1;
    }
    bucket
}

unsafe fn cached_byte(slot: CachedConfigurationSlot, key: ConfigurationKeyAddress) -> u8 {
    let cache = Address32(0x0095ada0).offset(ByteOffset(slot.0 * 8));
    if unsafe { word(cache, 4) } == 0 {
        let value = unsafe { configuration_byte(ConfigurationKey(word(key.0, 0))) };
        unsafe { store_byte(cache, 0, value); store_word(cache, 4, 1); }
    }
    unsafe { byte(cache, 0) }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_configuration_cached_profile(value: ConfigurationSelection) -> u8 {
    if value.0 > 30 { return 64; }
    let key = ConfigurationKeyAddress(Address32(0x00829a5c).offset(ByteOffset(value.0 * 4)));
    unsafe { cached_byte(CachedConfigurationSlot(value.0), key) }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_configuration_cached_exposure(value: ConfigurationSelection) -> u8 {
    let bucket = unsafe { xm1_pipeline_image_configuration_threshold_bucket(value) };
    let key = ConfigurationKeyAddress(Address32(0x00829a10).offset(ByteOffset(bucket * 4)));
    unsafe { cached_byte(CachedConfigurationSlot(bucket + 31), key) }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_configuration_exposure_record(selection: ConfigurationSelection, exposure: ConfigurationSelection, output: Address32) {
    if output.0 == 0 { return; }
    let bucket = unsafe { xm1_pipeline_image_configuration_threshold_bucket(exposure) };
    let keys = Address32(0x008179a0).offset(ByteOffset(selection.0.wrapping_sub(1).wrapping_mul(440).wrapping_add(bucket * 44)));
    for index in 0..11 {
        let key = ConfigurationKey(unsafe { word(keys, index * 4) });
        unsafe { store_byte(output, index, configuration_byte(key)); }
    }
}

#[derive(Clone, Copy)]
#[repr(transparent)]
struct ConfigurationKeyTable(Address32);
struct CoefficientBytes<const N: usize>([u8; N]);
struct CoefficientHalves<const N: usize>([u16; N]);

#[derive(Clone, Copy)]
enum CoefficientPacking {
    NineBits,
    TenBits,
    TwelveBits,
}
impl CoefficientPacking {
    fn expand(self, value: u16) -> u16 {
        let mask = match self {
            Self::NineBits => 0xfe00,
            Self::TenBits => 0xfc00,
            Self::TwelveBits => 0xf000,
        };
        if value & mask == 0 { value } else { value | mask }
    }
}

unsafe extern "C" {
    fn xm1_pipeline_sensor_select_type_mode_value(kind: u32, mode: u32, value: u32, alternate: u32) -> u32;
}

unsafe fn table_byte(table: ConfigurationKeyTable, index: ConfigurationSelection) -> u8 {
    let key = ConfigurationKey(unsafe { half(table.0, index.0.wrapping_mul(2)) } as u32);
    unsafe { configuration_byte(key) }
}

unsafe fn coefficient_bytes<const N: usize>(keys: ConfigurationKeyTable) -> CoefficientBytes<N> {
    let mut values = core::mem::MaybeUninit::<[u8; N]>::uninit();
    unsafe {
        xm1_pipeline_sensor_configuration_key_bytes(values.as_mut_ptr() as u32, keys.0.0, N as u32);
        CoefficientBytes(values.assume_init())
    }
}

unsafe fn coefficient_halves<const N: usize>(keys: ConfigurationKeyTable) -> CoefficientHalves<N> {
    let mut values = core::mem::MaybeUninit::<[u16; N]>::uninit();
    unsafe {
        xm1_pipeline_sensor_configuration_key_halves(values.as_mut_ptr() as u32, keys.0.0, N as u32);
        CoefficientHalves(values.assume_init())
    }
}

unsafe fn selected_primary(descriptor: OutputDescriptorAddress) -> ConfigurationSelection {
    let descriptor = descriptor.0;
    ConfigurationSelection(unsafe {
        xm1_pipeline_sensor_select_type_mode_value(word(descriptor, 0xe4), word(descriptor, 0x1b0), word(descriptor, 0xf4), word(descriptor, 0x180))
    })
}

unsafe fn selected_secondary(descriptor: OutputDescriptorAddress) -> ConfigurationSelection {
    let descriptor = descriptor.0;
    ConfigurationSelection(unsafe {
        xm1_pipeline_image_select_type_mode_value_copy(
            ConfigurationKind(word(descriptor, 0xe4)), ConfigurationSelection(word(descriptor, 0x1b0)),
            word(descriptor, 0x12c), word(descriptor, 0x184),
        )
    })
}

fn combined_selection(mode: ConfigurationSelection, value: ConfigurationSelection) -> ConfigurationSelection {
    ConfigurationSelection(mode.0.wrapping_mul(10).wrapping_add(value.0))
}

unsafe fn profile_override(descriptor: OutputDescriptorAddress) -> Option<ConfigurationSelection> {
    let descriptor = descriptor.0;
    if unsafe { word(descriptor, 0x1d0) } != 1 || unsafe { word(descriptor, 0x1d4) } == 0 { return None; }
    let selection = unsafe { word(descriptor, 0x1d8) };
    if selection == 0 || unsafe { word(descriptor, 0x144) } != 0 { None } else { Some(ConfigurationSelection(selection)) }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_read_detail_profile(output: Address32, selection: ConfigurationSelection) {
    let keys = Address32(0x00d5f27c).offset(ByteOffset(selection.0.wrapping_sub(1).wrapping_mul(20)));
    unsafe { xm1_pipeline_sensor_configuration_key_bytes(output.0, keys.0, 10); }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_read_chroma_profile(output: Address32, selection: ConfigurationSelection) {
    let keys = Address32(0x00d5f6fc).offset(ByteOffset(selection.0.wrapping_sub(1).wrapping_mul(16)));
    unsafe { xm1_pipeline_sensor_configuration_key_bytes(output.0, keys.0, 8); }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_expand_filter_matrix(descriptor: OutputDescriptorAddress, output: ExpandedOutputAddress) {
    let primary = unsafe { selected_primary(descriptor) };
    let key = if unsafe { word(descriptor.0, 0x164) } == 1 {
        ConfigurationKey(0xbbf3)
    } else if let Some(selection) = unsafe { profile_override(descriptor) } {
        ConfigurationKey(unsafe { half(Address32(0x00d5f678), selection.0.wrapping_mul(2)) } as u32)
    } else {
        let index = combined_selection(ConfigurationSelection(unsafe { word(descriptor.0, 0x160) }), primary);
        ConfigurationKey(unsafe { half(Address32(0x00d5f9d4), index.0.wrapping_mul(2)) } as u32)
    };
    let selection = unsafe { configuration_byte(key) } as u32;
    let keys = ConfigurationKeyTable(Address32(0x00d5f334).offset(ByteOffset(selection.wrapping_sub(1).wrapping_mul(20))));
    let values = unsafe { coefficient_halves::<10>(keys) };
    for (index, value) in values.0.into_iter().enumerate() {
        unsafe { store_half(output.0, 2 + index as u32 * 2, CoefficientPacking::NineBits.expand(value)); }
    }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_expand_filter_gain(descriptor: OutputDescriptorAddress, output: ExpandedOutputAddress) {
    let primary = unsafe { selected_primary(descriptor) };
    let secondary = unsafe { selected_secondary(descriptor) };
    let mut mode_code = 0u8;
    unsafe { xm1_pipeline_image_encode_configuration_mode(descriptor, Address32((&mut mode_code as *mut u8) as u32)); }
    let exposure = unsafe { xm1_pipeline_image_configuration_mode_value(descriptor, ConfigurationLimit(11)) };
    let mode = ConfigurationSelection(unsafe { word(descriptor.0, 0x160) });
    let primary_gain = unsafe { table_byte(ConfigurationKeyTable(Address32(0x00d5f468)), combined_selection(mode, primary)) } as u64;
    let stage_gain = unsafe { table_byte(ConfigurationKeyTable(Address32(0x00d5f2e0)), ConfigurationSelection(word(descriptor.0, 0xf0))) } as u64;
    let option_gain = unsafe { table_byte(ConfigurationKeyTable(Address32(0x00d5f26c)), ConfigurationSelection(word(descriptor.0, 0x114))) } as u64;
    let exposure_gain = unsafe { table_byte(ConfigurationKeyTable(Address32(0x00d5f6e4)), ConfigurationSelection(exposure)) } as u64;
    let secondary_gain = if secondary.0 == 100 { 128 } else {
        let index = secondary.0.wrapping_sub((secondary.0 / 10).wrapping_mul(7));
        unsafe { table_byte(ConfigurationKeyTable(Address32(0x00d5f874)), ConfigurationSelection(index)) }
    } as u64;
    let threshold_gain = unsafe { table_byte(ConfigurationKeyTable(Address32(0x00d5faa0)), ConfigurationSelection(word(descriptor.0, 0x13c))) } as u64;
    let mode_gain = unsafe { table_byte(ConfigurationKeyTable(Address32(0x00d5fa24)), ConfigurationSelection(mode_code as u32)) } as u64;
    let type_gain = if unsafe { word(descriptor.0, 0xe4) } == 0x35 {
        unsafe { table_byte(ConfigurationKeyTable(Address32(0x00d5f6d0)), primary) }
    } else { 64 } as u32;
    let special_gain = if unsafe { word(descriptor.0, 0x164) } == 1 {
        unsafe { configuration_byte(ConfigurationKey(0xbcaa)) }
    } else { 64 } as u32;
    let adjusted_gain = if unsafe { word(descriptor.0, 0x1d0) } == 1
        && unsafe { word(descriptor.0, 0x1d4) } != 0 && unsafe { word(descriptor.0, 0x144) } == 0 {
        let index = unsafe { word(descriptor.0, 0x128).wrapping_add(word(descriptor.0, 0x1d8).wrapping_mul(2)) };
        unsafe { table_byte(ConfigurationKeyTable(Address32(0x00d5fa84)), ConfigurationSelection(index)) }
    } else { 64 } as u32;
    let auxiliary_gain = if unsafe { word(descriptor.0, 0x1b4) } == 1 {
        unsafe { configuration_byte(ConfigurationKey(0xbcb1)) }
    } else { 64 } as u32;
    let (profile_gain, sensitivity_gain) = if unsafe { word(descriptor.0, 0x1e4) } != 0 {
        (64u64, 64u64)
    } else if unsafe { byte(descriptor.0, 0x1ae) } == 1 {
        (unsafe { byte(descriptor.0, 0x1ec) } as u64, unsafe { byte(descriptor.0, 0x1ed) } as u64)
    } else {
        (unsafe { xm1_pipeline_image_configuration_cached_profile(ConfigurationSelection(byte(descriptor.0, 0x11b) as u32)) } as u64,
         unsafe { xm1_pipeline_image_configuration_cached_exposure(ConfigurationSelection(byte(descriptor.0, 0x11a) as u32)) } as u64)
    };
    let gains = primary_gain * stage_gain * option_gain * exposure_gain * secondary_gain * threshold_gain * mode_gain;
    let scaled = ((gains >> 34) as u32 & 0x003fffc0) as u64;
    let structural = type_gain.wrapping_mul(special_gain).wrapping_mul(adjusted_gain).wrapping_mul(auxiliary_gain) >> 18;
    let result = (scaled * structural as u64 * profile_gain * sensitivity_gain >> 24) as u32;
    unsafe { store_byte(output.0, 0x25, result.min(255) as u8); }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_expand_filter_channels(descriptor: OutputDescriptorAddress, output: ExpandedOutputAddress) {
    let gain = unsafe { table_byte(ConfigurationKeyTable(Address32(0x00d5fa98)), ConfigurationSelection(word(descriptor.0, 0x160))) };
    let mut channels = [0u8; 11];
    if unsafe { word(descriptor.0, 0xe8) } == 4 {
        channels = [0, 0, 0, 0, 16, 0, 16, 0, 16, 0, 16];
        unsafe { store_byte(output.0, 1, 0); }
    } else {
        let selection = ConfigurationSelection(unsafe { table_byte(ConfigurationKeyTable(Address32(0x00d5f278)), ConfigurationSelection(word(descriptor.0, 0x1e0))) } as u32);
        if xm1_pipeline_image_configuration_value_within_limit(selection, ConfigurationLimit(3)) == 0
            || unsafe { word(descriptor.0, 0x1e4) } != 0 {
            channels = [0, 0, 0, 0, 16, 0, 16, 0, 16, 0, 16];
            unsafe { store_byte(output.0, 1, 0); }
        } else if unsafe { byte(descriptor.0, 0x1ae) } == 1 {
            let mut source = selection.0.wrapping_sub(1).wrapping_mul(11) as u8;
            for channel in &mut channels {
                *channel = unsafe { byte(descriptor.0, 0x1ee + source as u32) };
                source = source.wrapping_add(1);
            }
        } else {
            unsafe {
                xm1_pipeline_image_configuration_exposure_record(
                    selection, ConfigurationSelection(byte(descriptor.0, 0x11a) as u32),
                    Address32(channels.as_mut_ptr() as u32),
                );
            }
        }
    }
    let selection = unsafe { word(descriptor.0, 0x1e0) };
    let second = unsafe { configuration_byte(ConfigurationKey(half(Address32(0x00d5fa90), selection.wrapping_mul(4).wrapping_add(2)) as u32)) };
    let first = unsafe { configuration_byte(ConfigurationKey(half(Address32(0x00d5fa90), selection.wrapping_mul(4)) as u32)) };
    let second_limit = unsafe { half(descriptor.0, 0x1c2) };
    let first_limit = unsafe { half(descriptor.0, 0x1c0) };
    unsafe {
        store_half(output.0, 0x26, second_limit);
        store_half(output.0, 0x28, first_limit);
        store_byte(output.0, 0x2a, gain);
    }
    for (index, channel) in channels.into_iter().enumerate() {
        unsafe { store_byte(output.0, 0x2b + index as u32, channel); }
    }
    unsafe { store_byte(output.0, 0x36, first); store_byte(output.0, 0x37, second); }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_expand_detail_channels(descriptor: OutputDescriptorAddress, output: ExpandedOutputAddress) {
    let primary = unsafe { selected_primary(descriptor) };
    let mode = ConfigurationSelection(unsafe { word(descriptor.0, 0x160) });
    let selection = ConfigurationSelection(unsafe { table_byte(ConfigurationKeyTable(Address32(0x00d5f75c)), combined_selection(mode, primary)) } as u32);
    let profile = if xm1_pipeline_image_configuration_value_within_limit(selection, ConfigurationLimit(5)) == 0 {
        CoefficientBytes([31, 0, 0, 0, 0, 0, 0, 0, 0, 0])
    } else {
        unsafe { coefficient_bytes::<10>(ConfigurationKeyTable(Address32(0x00d5f27c).offset(ByteOffset(selection.0.wrapping_sub(1).wrapping_mul(20))))) }
    };
    let first = unsafe { table_byte(ConfigurationKeyTable(Address32(0x00d5f4b8)), combined_selection(ConfigurationSelection(word(descriptor.0, 0x160)), primary)) };
    let second = unsafe { table_byte(ConfigurationKeyTable(Address32(0x00d5f824)), combined_selection(ConfigurationSelection(word(descriptor.0, 0x160)), primary)) };
    for (index, value) in profile.0.into_iter().enumerate() {
        unsafe { store_byte(output.0, 0x38 + index as u32, value); }
    }
    unsafe { store_byte(output.0, 0x42, first); store_byte(output.0, 0x43, second); }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_expand_filter_offsets(descriptor: OutputDescriptorAddress, output: ExpandedOutputAddress) {
    let mut offsets = [0u8; 4];
    if unsafe { word(descriptor.0, 0x104) } != 2 || unsafe { configuration_byte(ConfigurationKey(0xc043)) } == 0 {
        let primary = unsafe { selected_primary(descriptor) };
        let secondary = unsafe { selected_secondary(descriptor) };
        let keys = if unsafe { word(descriptor.0, 0x164) } == 1 {
            Address32(0x00d5f21c).offset(ByteOffset(primary.0.wrapping_mul(8)))
        } else {
            let table = if unsafe { word(descriptor.0, 0x154) } == 0 { 0x00d5f894 } else { 0x00d5f538 };
            Address32(table).offset(ByteOffset(combined_selection(ConfigurationSelection(unsafe { word(descriptor.0, 0x160) }), primary).0.wrapping_mul(8)))
        };
        let base = unsafe { coefficient_bytes::<4>(ConfigurationKeyTable(keys)) };
        let delta = if secondary.0 == 100 { CoefficientBytes([0u8; 4]) } else {
            let index = secondary.0.wrapping_sub((secondary.0 / 10).wrapping_mul(7));
            unsafe { coefficient_bytes::<4>(ConfigurationKeyTable(Address32(0x00d5f7ac).offset(ByteOffset(index.wrapping_mul(8))))) }
        };
        for index in 0..4 {
            offsets[index] = (base.0[index] as i32 + delta.0[index] as i8 as i32).clamp(0, 63) as u8;
        }
    }
    for (index, value) in offsets.into_iter().enumerate() {
        unsafe { store_byte(output.0, 0x19 + index as u32, value); }
    }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_expand_filter_triplet(descriptor: OutputDescriptorAddress, output: ExpandedOutputAddress) {
    let table = if unsafe { word(descriptor.0, 0x154) } == 0 { 0x00d5f520 } else { 0x00d5f508 };
    let keys = Address32(table).offset(ByteOffset(unsafe { word(descriptor.0, 0x160) }.wrapping_mul(6)));
    let values = unsafe { coefficient_bytes::<3>(ConfigurationKeyTable(keys)) };
    for (index, value) in values.0.into_iter().enumerate() {
        unsafe { store_byte(output.0, 0x16 + index as u32, value); }
    }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_expand_filter_chroma(descriptor: OutputDescriptorAddress, output: ExpandedOutputAddress) {
    let primary = unsafe { selected_primary(descriptor) };
    let index = if let Some(selection) = unsafe { profile_override(descriptor) } {
        unsafe { table_byte(ConfigurationKeyTable(Address32(0x00d5f460)), selection) }
    } else {
        unsafe { table_byte(ConfigurationKeyTable(Address32(0x00d5f680)), combined_selection(ConfigurationSelection(word(descriptor.0, 0x160)), primary)) }
    };
    let selection = ConfigurationSelection(index as u32);
    let values = if xm1_pipeline_image_configuration_value_within_limit(selection, ConfigurationLimit(6)) == 0 {
        CoefficientBytes([16, 32, 128, 128, 128, 0, 0, 0])
    } else {
        unsafe { coefficient_bytes::<8>(ConfigurationKeyTable(Address32(0x00d5f6fc).offset(ByteOffset(selection.0.wrapping_sub(1).wrapping_mul(16))))) }
    };
    for (offset, index) in [0usize, 1, 2, 5, 3, 6, 4, 7].into_iter().enumerate() {
        unsafe { store_byte(output.0, 0x1d + offset as u32, values.0[index]); }
    }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_select_output_profile(descriptor: OutputDescriptorAddress, mode: ConfigurationSelection) -> u32 {
    let selector = match unsafe { word(descriptor.0, 0x124) } {
        1 => 9, 2 => 10, 3 => 11, 4 => 12, 5 => 13, 6 => 14, 7 => 1,
        8 => 15, 9 => 16, 10 => 17, 11 => 7, 12 => 8, 13 => 6,
        14 | 18 => 18, 15 | 19 => 19, 16 | 20 => 20,
        _ => match unsafe { word(descriptor.0, 0xe4) } {
            8..=10 => 3, 0x13 => 2, 0x18 => 1, 0x20..=0x22 => 4,
            0x2b..=0x2d => 5, _ => 0,
        },
    };
    let index = mode.0.wrapping_mul(21).wrapping_add(selector);
    let key = ConfigurationKey(unsafe { word(Address32(0x00d6020c), index.wrapping_mul(4)) });
    let selection = unsafe { configuration_byte(key) } as u32;
    if (1..=6).contains(&selection) { selection } else { 255 }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_select_output_profile_exposure(descriptor: OutputDescriptorAddress, profile: ConfigurationSelection) -> u32 {
    let exposure = unsafe { xm1_pipeline_image_configuration_mode_value(descriptor, ConfigurationLimit(11)) };
    let index = profile.0.wrapping_sub(1).wrapping_mul(11).wrapping_add(exposure);
    let key = ConfigurationKey(unsafe { word(Address32(0x00d6035c), index.wrapping_mul(4)) });
    let selection = unsafe { configuration_byte(key) } as u32;
    if (1..=7).contains(&selection) { selection } else { 255 }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_expand_output_profile(descriptor: OutputDescriptorAddress, output: ExpandedOutputAddress) {
    let mode = unsafe { byte(descriptor.0, 0x160) };
    let mode = ConfigurationSelection(if mode > 3 { 0 } else { mode as u32 });
    let profile = unsafe { xm1_pipeline_image_select_output_profile(descriptor, mode) };
    let selection = if profile == 255 { 255 } else {
        unsafe { xm1_pipeline_image_select_output_profile_exposure(descriptor, ConfigurationSelection(profile)) }
    };
    let header = Address32(0x00d60544).offset(ByteOffset(mode.0 * 20));
    for index in 0..5 {
        let value = unsafe { configuration_byte(ConfigurationKey(word(header, index * 4))) };
        unsafe { store_byte(output.0, index + 1, value); }
    }
    if selection == 255 {
        for index in 6..14 { unsafe { store_byte(output.0, index, 15); } }
    } else {
        let profile = Address32(0x00d60464).offset(ByteOffset(((selection.wrapping_sub(1) as u8) as u32) * 32));
        for index in 0..8 {
            let value = unsafe { configuration_byte(ConfigurationKey(word(profile, index * 4))) };
            unsafe { store_byte(output.0, index + 6, value); }
        }
    }
}

#[derive(Clone, Copy)]
enum CalibrationAxis {
    First,
    Second,
    Third,
}
#[derive(Clone, Copy)]
struct CalibrationAxisIndex(u16);
#[derive(Clone, Copy)]
struct CalibrationAxisValue(u16);
struct CalibrationCoordinate {
    count: u16,
    index: CalibrationAxisIndex,
    value: CalibrationAxisValue,
}

unsafe fn calibration_coordinate(descriptor: OutputDescriptorAddress, axis: CalibrationAxis) -> CalibrationCoordinate {
    let (count_offset, thresholds) = match axis {
        CalibrationAxis::First => (4, Address32(0x007c2df4)),
        CalibrationAxis::Second => (2, Address32(0x007c2de8)),
        CalibrationAxis::Third => (6, Address32(0x007c2df8)),
    };
    let count = unsafe { half(Address32(0x007c2de0), count_offset) };
    if count == 1 {
        return CalibrationCoordinate { count, index: CalibrationAxisIndex(0), value: CalibrationAxisValue(0) };
    }
    let value = match axis {
        CalibrationAxis::First => (unsafe { byte(descriptor.0, 0x11a) }) as u16,
        CalibrationAxis::Second => unsafe { half(descriptor.0, 0x1ea) },
        CalibrationAxis::Third => ((unsafe { word(descriptor.0, 0x238) }.wrapping_add(128) >> 8) & 0xffff) as u16,
    };
    let last = count.wrapping_sub(1);
    let mut index = 0u16;
    let mut bucket = 0u16;
    if last != 0 && value >= unsafe { half(thresholds, 0) } {
        loop {
            index = index.wrapping_add(1);
            if index >= last { break; }
            if value < unsafe { half(thresholds, index as u32 * 2) } {
                bucket = index;
                break;
            }
        }
    }
    if index as u32 == (count as u32).wrapping_sub(1) { bucket = last; }
    CalibrationCoordinate { count, index: CalibrationAxisIndex(bucket), value: CalibrationAxisValue(value) }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_expand_calibration_grid(descriptor: OutputDescriptorAddress, output: ExpandedOutputAddress) {
    let width = (unsafe { half(descriptor.0, 0x234) } as u32 + 4) >> 3;
    let width = if (width.wrapping_sub(8) as u16) > 1016 { 8 } else { width };
    unsafe { store_half(output.0, 2, width as u16); }
    let height = (unsafe { half(descriptor.0, 0x236) } as u32 + 3) / 6;
    let height = if (height.wrapping_sub(8) as u16) > 1016 { 8 } else { height };
    unsafe {
        store_half(output.0, 4, height as u16);
        store_half(output.0, 6, ((0x40000 + (width >> 1)) / width) as u16);
        store_half(output.0, 8, ((0x40000 + (height >> 1)) / height) as u16);
        store_half(output.0, 10, 0);
        store_half(output.0, 12, 0);
    }
    let first = unsafe { calibration_coordinate(descriptor, CalibrationAxis::First) };
    let second = unsafe { calibration_coordinate(descriptor, CalibrationAxis::Second) };
    let third = unsafe { calibration_coordinate(descriptor, CalibrationAxis::Third) };
    let index = (second.count as u32).wrapping_mul(first.index.0 as u32).wrapping_add(second.index.0 as u32)
        .wrapping_mul(third.count as u32).wrapping_add(third.index.0 as u32) as u16;
    let row = Address32(0x007c2dfc).offset(ByteOffset((index as u32).wrapping_mul(3240)));
    unsafe {
        store_word(output.0, 0xcec, row.0);
        store_half(descriptor.0, 0x23c, index);
        store_half(descriptor.0, 0x23e, first.value.0);
        store_half(descriptor.0, 0x240, second.value.0);
        store_half(descriptor.0, 0x242, third.value.0);
    }
}

unsafe fn grid_control_band(descriptor: OutputDescriptorAddress, first_key: ConfigurationKey, second_key: ConfigurationKey) -> ConfigurationSelection {
    let first = unsafe { configuration_byte(first_key) } as u32;
    let second = unsafe { configuration_byte(second_key) } as u32;
    if first >= second { return ConfigurationSelection(0); }
    let value = unsafe { word(descriptor.0, 0xf4) };
    ConfigurationSelection(if value <= first { 0 } else if value < second { 1 } else { 2 })
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_expand_calibration_controls(descriptor: OutputDescriptorAddress, output: ExpandedOutputAddress) {
    let first = unsafe { grid_control_band(descriptor, ConfigurationKey(0xaf2a), ConfigurationKey(0xaf2b)) };
    let second = unsafe { grid_control_band(descriptor, ConfigurationKey(0xaf82), ConfigurationKey(0xaf83)) };
    let values = unsafe { coefficient_bytes::<4>(ConfigurationKeyTable(Address32(0x00d60c28).offset(ByteOffset(first.0 * 8)))) };
    let coefficient = unsafe { configuration_half(ConfigurationKey(half(Address32(0x00d60c48), second.0 * 2) as u32)) };
    for (index, value) in values.0.into_iter().enumerate() {
        unsafe { store_byte(output.0, 0xe + index as u32, value); }
    }
    let limit = unsafe { configuration_half(ConfigurationKey(0xaf80)) };
    unsafe { store_half(output.0, 0x14, limit); }
    let first = unsafe { table_byte(ConfigurationKeyTable(Address32(0x00d60c40)), second) };
    let second = unsafe { table_byte(ConfigurationKeyTable(Address32(0x00d60c20)), second) };
    unsafe {
        store_byte(output.0, 0x16, first);
        store_byte(output.0, 0x17, second);
        store_half(output.0, 0x18, CoefficientPacking::NineBits.expand(coefficient));
    }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_expand_calibration_gain(descriptor: OutputDescriptorAddress, output: ExpandedOutputAddress) -> u32 {
    let index = combined_selection(ConfigurationSelection(unsafe { word(descriptor.0, 0x160) }), ConfigurationSelection(unsafe { word(descriptor.0, 0xf4) }));
    let primary_gain = unsafe { table_byte(ConfigurationKeyTable(Address32(0x00d60bb0)), index) } as u64;
    let exposure = unsafe { xm1_pipeline_image_configuration_mode_value(descriptor, ConfigurationLimit(11)) };
    let scale = unsafe { word(descriptor.0, 0x238) };
    let bucket = if unsafe { half(Address32(0x007c2de6), 0) } == 1 { 0 } else {
        let value = ((scale.wrapping_add(128) >> 8) & 0xffff) as u16;
        if value < unsafe { half(Address32(0x007c2df8), 0) } { 0 }
        else if value < unsafe { half(Address32(0x007c2df8), 2) } { 1 } else { 2 }
    };
    let option_gain = unsafe { table_byte(ConfigurationKeyTable(Address32(0x00d60c50)), ConfigurationSelection(word(descriptor.0, 0x114))) } as u64;
    let exposure_gain = unsafe { table_byte(ConfigurationKeyTable(Address32(0x00d60c00)), ConfigurationSelection(exposure)) } as u64;
    let threshold_gain = unsafe { table_byte(ConfigurationKeyTable(Address32(0x00d60b98)), ConfigurationSelection(word(descriptor.0, 0x13c))) } as u64;
    let scale_gain = unsafe { table_byte(ConfigurationKeyTable(Address32(0x00d60c18)), ConfigurationSelection(bucket)) } as u64;
    let auxiliary_gain = if unsafe { word(descriptor.0, 0x1b4) } == 1 {
        unsafe { configuration_byte(ConfigurationKey(0xaf7d)) }
    } else { 64 } as u64;
    let type_gain = if unsafe { word(descriptor.0, 0xe4) } == 0x35 {
        unsafe { table_byte(ConfigurationKeyTable(Address32(0x00d60bac)), ConfigurationSelection(word(descriptor.0, 0x17c))) }
    } else { 64 } as u64;
    let product = (primary_gain << 6) * option_gain * exposure_gain * threshold_gain * scale_gain * auxiliary_gain * type_gain;
    unsafe { store_byte(output.0, 0x12, ((product >> 42) as u32).min(255) as u8); }
    product as u32
}

#[derive(Clone, Copy)]
#[repr(transparent)]
pub(super) struct TenLevelTableAddress(pub(super) Address32);

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_configuration_level(value: ConfigurationSelection, table: TenLevelTableAddress) -> u16 {
    let thresholds = [
        unsafe { configuration_byte(ConfigurationKey(0xa924)) },
        unsafe { configuration_byte(ConfigurationKey(0xa925)) },
        unsafe { configuration_byte(ConfigurationKey(0xa926)) },
        unsafe { configuration_byte(ConfigurationKey(0xa927)) },
        unsafe { configuration_byte(ConfigurationKey(0xa928)) },
        unsafe { configuration_byte(ConfigurationKey(0xa929)) },
        unsafe { configuration_byte(ConfigurationKey(0xa92a)) },
        unsafe { configuration_byte(ConfigurationKey(0xa92b)) },
        unsafe { configuration_byte(ConfigurationKey(0xa92c)) },
    ];
    let index = thresholds.iter().position(|threshold| *threshold as u32 >= value.0).unwrap_or(9);
    unsafe { half(table.0, index as u32 * 2) }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_reset_output_descriptor(descriptor: OutputDescriptorAddress, output: ExpandedOutputAddress) {
    unsafe { core::ptr::write_bytes(output.0.0 as *mut u8, 0, 0x1260); }
    let level = unsafe {
        xm1_pipeline_image_configuration_level(
            ConfigurationSelection(byte(descriptor.0, 0x11a) as u32),
            TenLevelTableAddress(Address32(0x00d62348)),
        )
    };
    unsafe { store_word(descriptor.0, 0x13c, level as u32); }
    if unsafe { word(descriptor.0, 0x140) } != 7 || unsafe { word(descriptor.0, 0x160) } != 3 { return; }
    let mode = unsafe { word(descriptor.0, 0x154) };
    if mode == 0 || unsafe { word(descriptor.0, 0x11c) } == unsafe { word(descriptor.0, 0x120) } { return; }
    let mode = match mode { 3 => 1, 4 => 2, _ => 0 };
    unsafe { store_word(descriptor.0, 0x154, mode); }
}

struct WidePairCoefficients([i64; 4]);
struct PairCoefficients {
    active: u8,
    values: WidePairCoefficients,
}
#[derive(Clone, Copy)]
enum PairScaling {
    Base,
    Output,
}

unsafe fn pair_profile(descriptor: OutputDescriptorAddress) -> ConfigurationSelection {
    let table = if unsafe { word(descriptor.0, 0xe4) } == 0x17 { 0x00d61fbc } else { 0x00d621a0 };
    let index = unsafe { word(descriptor.0, 0x104).wrapping_add(word(descriptor.0, 0x108).wrapping_mul(8)) };
    ConfigurationSelection(unsafe { table_byte(ConfigurationKeyTable(Address32(table)), ConfigurationSelection(index)) } as u32)
}

unsafe fn pair_coefficients(selection: ConfigurationSelection) -> WidePairCoefficients {
    let keys = Address32(0x00d61e14).offset(ByteOffset(selection.0.wrapping_mul(8)));
    let first = unsafe { configuration_half(ConfigurationKey(half(keys, 0) as u32)) } as i64;
    let second = unsafe { configuration_half(ConfigurationKey(half(keys, 4) as u32)) } as i64;
    let third = unsafe { configuration_half(ConfigurationKey(half(keys, 6) as u32)) } as i64;
    let fourth = unsafe { configuration_half(ConfigurationKey(half(keys, 2) as u32)) } as i64;
    WidePairCoefficients([first, second, third, fourth])
}

unsafe fn pair_option_gain(descriptor: OutputDescriptorAddress) -> u8 {
    unsafe { table_byte(ConfigurationKeyTable(Address32(0x00d62194)), ConfigurationSelection(word(descriptor.0, 0x10c))) }
}
unsafe fn pair_primary_gain(descriptor: OutputDescriptorAddress) -> u8 {
    let index = combined_selection(ConfigurationSelection(unsafe { word(descriptor.0, 0x160) }), ConfigurationSelection(unsafe { word(descriptor.0, 0xf4) }));
    unsafe { table_byte(ConfigurationKeyTable(Address32(0x00d61f34)), index) }
}
unsafe fn pair_selection_gain(descriptor: OutputDescriptorAddress) -> u8 {
    let table = if unsafe { word(descriptor.0, 0xe4) } == 0x17 { 0x00d61c60 } else { 0x00d61e04 };
    let selection = unsafe { table_byte(ConfigurationKeyTable(Address32(table)), ConfigurationSelection(word(descriptor.0, 0x104))) } as u32;
    if !(1..=5).contains(&selection) { return 128; }
    let index = selection.wrapping_sub(1).wrapping_mul(5).wrapping_add(unsafe { word(descriptor.0, 0x154) });
    unsafe { table_byte(ConfigurationKeyTable(Address32(0x00d620b0)), ConfigurationSelection(index)) }
}
unsafe fn pair_level_gain(descriptor: OutputDescriptorAddress) -> u8 {
    unsafe { table_byte(ConfigurationKeyTable(Address32(0x00d61e64)), ConfigurationSelection(word(descriptor.0, 0x13c))) }
}
unsafe fn pair_mode_gain(descriptor: OutputDescriptorAddress) -> u8 {
    let mut mode = 0u8;
    unsafe { xm1_pipeline_image_encode_configuration_mode(descriptor, Address32((&mut mode as *mut u8) as u32)); }
    unsafe { table_byte(ConfigurationKeyTable(Address32(0x00d62134)), ConfigurationSelection(mode as u32)) }
}
unsafe fn pair_output_gain(descriptor: OutputDescriptorAddress) -> u8 {
    let table = match unsafe { word(descriptor.0, 0x140) } {
        3 | 4 => 0x00d66d0c,
        5 | 6 => return 128,
        _ => 0x00d6691c,
    };
    unsafe { table_byte(ConfigurationKeyTable(Address32(table)), ConfigurationSelection(word(descriptor.0, 0x128))) }
}
unsafe fn pair_stage_gains(descriptor: OutputDescriptorAddress) -> (u8, u8) {
    let first = match unsafe { word(descriptor.0, 0x140) } {
        3 | 4 if unsafe { word(descriptor.0, 0xdc) } == 1 => unsafe { configuration_byte(ConfigurationKey(0xc257)) },
        5 => {
            let key = if unsafe { word(descriptor.0, 0xec) } == 0 { 0xc257 } else {
                match unsafe { word(descriptor.0, 0xe4) } {
                    5 => 0xc258, 6 => 0xc259, 7 => 0xc25c, 8 => 0xc25a, 11 | 12 => 0xc25b, _ => 0xc257,
                }
            };
            unsafe { configuration_byte(ConfigurationKey(key)) }
        }
        _ => 128,
    };
    let second = if unsafe { word(descriptor.0, 0x140) } == 5 {
        unsafe { table_byte(ConfigurationKeyTable(Address32(0x00d66540)), ConfigurationSelection(word(descriptor.0, 0x148))) }
    } else { 128 };
    (first, second)
}
unsafe fn pair_kind_gain(descriptor: OutputDescriptorAddress) -> u8 {
    if unsafe { word(descriptor.0, 0x140) } == 6 {
        unsafe { configuration_byte(ConfigurationKey(0xc260)) }
    } else { 128 }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_select_pair_profile(descriptor: OutputDescriptorAddress, output: Address32) {
    let value = unsafe { pair_profile(descriptor) };
    unsafe { store_byte(output, 0, value.0 as u8); }
}
#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_read_pair_coefficients(output: Address32, selection: ConfigurationSelection) {
    let values = unsafe { pair_coefficients(selection) };
    for (index, value) in values.0.into_iter().enumerate() {
        unsafe { store_word(output, index as u32 * 8, value as u32); store_word(output, index as u32 * 8 + 4, 0); }
    }
}
#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_pair_option_gain(output: Address32, descriptor: OutputDescriptorAddress) {
    let value = unsafe { pair_option_gain(descriptor) };
    unsafe { store_byte(output, 0, value); }
}
#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_pair_primary_gain(output: Address32, descriptor: OutputDescriptorAddress) {
    let value = unsafe { pair_primary_gain(descriptor) };
    unsafe { store_byte(output, 0, value); }
}
#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_pair_selection_gain(output: Address32, descriptor: OutputDescriptorAddress) {
    let value = unsafe { pair_selection_gain(descriptor) };
    unsafe { store_byte(output, 0, value); }
}
#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_pair_level_gain(output: Address32, descriptor: OutputDescriptorAddress) {
    let value = unsafe { pair_level_gain(descriptor) };
    unsafe { store_byte(output, 0, value); }
}
#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_pair_mode_gain(output: Address32, descriptor: OutputDescriptorAddress) {
    let value = unsafe { pair_mode_gain(descriptor) };
    unsafe { store_byte(output, 0, value); }
}
#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_pair_output_gain(output: Address32, descriptor: OutputDescriptorAddress) {
    let value = unsafe { pair_output_gain(descriptor) };
    unsafe { store_byte(output, 0, value); }
}
#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_pair_stage_gains(first: Address32, second: Address32, descriptor: OutputDescriptorAddress) {
    let values = unsafe { pair_stage_gains(descriptor) };
    unsafe { store_byte(first, 0, values.0); store_byte(second, 0, values.1); }
}
#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_pair_kind_gain(output: Address32, descriptor: OutputDescriptorAddress) {
    let value = unsafe { pair_kind_gain(descriptor) };
    unsafe { store_byte(output, 0, value); }
}

unsafe fn scaled_pair_coefficients(descriptor: OutputDescriptorAddress, scaling: PairScaling) -> PairCoefficients {
    if unsafe { word(descriptor.0, 0x104) }.wrapping_sub(2) < 2 {
        return PairCoefficients { active: 1, values: WidePairCoefficients([0; 4]) };
    }
    let selection = unsafe { pair_profile(descriptor) };
    if !(1..=10).contains(&selection.0) {
        return PairCoefficients { active: 0, values: WidePairCoefficients([1024, 0, 0, 1024]) };
    }
    let mut values = unsafe { pair_coefficients(ConfigurationSelection(selection.0 - 1)) };
    for value in &mut values.0 {
        if *value & !0xfff != 0 { *value |= !0xfff; }
    }
    let option = unsafe { pair_option_gain(descriptor) } as i64;
    let primary = unsafe { pair_primary_gain(descriptor) } as i64;
    let selected = unsafe { pair_selection_gain(descriptor) } as i64;
    let level = unsafe { pair_level_gain(descriptor) } as i64;
    let mode = unsafe { pair_mode_gain(descriptor) } as i64;
    let base_gain = option * primary * selected * level * mode;
    let additional_gain = match scaling {
        PairScaling::Base => 1i64,
        PairScaling::Output => {
            let output = unsafe { pair_output_gain(descriptor) } as i64;
            let stages = unsafe { pair_stage_gains(descriptor) };
            let kind = unsafe { pair_kind_gain(descriptor) } as i64;
            output * stages.0 as i64 * stages.1 as i64 * kind
        }
    };
    for value in &mut values.0 {
        let product = value.wrapping_mul(base_gain);
        let scaled = match scaling {
            PairScaling::Base => product >> 34,
            PairScaling::Output => (product >> 28).wrapping_mul(additional_gain) >> 34,
        };
        *value = scaled.clamp(-4096, 4095);
    }
    PairCoefficients { active: 1, values }
}

unsafe fn store_pair_coefficients(output: Address32, pair: PairCoefficients) {
    unsafe {
        store_byte(output, 0, pair.active);
        store_half(output, 2, pair.values.0[0] as u16);
        store_half(output, 6, pair.values.0[1] as u16);
        store_half(output, 8, pair.values.0[2] as u16);
        store_half(output, 4, pair.values.0[3] as u16);
    }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_build_pair_coefficients(output: Address32, descriptor: OutputDescriptorAddress) {
    let pair = unsafe { scaled_pair_coefficients(descriptor, PairScaling::Base) };
    unsafe { store_pair_coefficients(output, pair); }
}
#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_build_output_pair_coefficients(output: Address32, descriptor: OutputDescriptorAddress) {
    let pair = unsafe { scaled_pair_coefficients(descriptor, PairScaling::Output) };
    unsafe { store_pair_coefficients(output, pair); }
}
#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_expand_pair_coefficients(descriptor: OutputDescriptorAddress, output: ExpandedOutputAddress) {
    let pair = unsafe { scaled_pair_coefficients(descriptor, PairScaling::Base) };
    unsafe {
        store_byte(output.0, 2, pair.active);
        store_half(output.0, 0x26, pair.values.0[0] as u16);
        store_half(output.0, 0x28, pair.values.0[1] as u16);
        store_half(output.0, 0x2a, pair.values.0[3] as u16);
        store_half(output.0, 0x2c, pair.values.0[2] as u16);
    }
}
#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_store_output_pair_coefficients(pair: Address32, output: ExpandedOutputAddress) {
    let active = unsafe { byte(pair, 0) };
    unsafe { store_byte(output.0, 8, active); }
    let first = unsafe { half(pair, 2) };
    let second = unsafe { half(pair, 6) };
    unsafe { store_half(output.0, 0xdc, first); store_half(output.0, 0xde, second); }
    let third = unsafe { half(pair, 4) };
    let fourth = unsafe { half(pair, 8) };
    unsafe { store_half(output.0, 0xe0, third); store_half(output.0, 0xe2, fourth); }
}
#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_expand_output_pair_coefficients(descriptor: OutputDescriptorAddress, output: ExpandedOutputAddress) {
    let pair = unsafe { scaled_pair_coefficients(descriptor, PairScaling::Output) };
    unsafe {
        store_byte(output.0, 8, pair.active);
        store_half(output.0, 0xdc, pair.values.0[0] as u16);
        store_half(output.0, 0xde, pair.values.0[1] as u16);
        store_half(output.0, 0xe0, pair.values.0[3] as u16);
        store_half(output.0, 0xe2, pair.values.0[2] as u16);
    }
}

struct DualCoefficientBlock {
    active: u8,
    first: CoefficientHalves<5>,
    second: CoefficientHalves<5>,
    gains: CoefficientBytes<2>,
}
struct ChannelPairs {
    header: CoefficientBytes<4>,
    values: CoefficientHalves<10>,
}
struct HeaderChannels {
    header: CoefficientBytes<4>,
    channels: CoefficientBytes<5>,
    weights: CoefficientHalves<5>,
}

unsafe fn dual_selections(descriptor: OutputDescriptorAddress) -> (u8, u8) {
    let mode = unsafe { word(descriptor.0, 0x108) };
    let secondary = unsafe { word(descriptor.0, 0xe8) };
    let index = secondary.wrapping_add(mode.wrapping_mul(14));
    let value = unsafe { table_byte(ConfigurationKeyTable(Address32(0x00d61f84)), ConfigurationSelection(index)) };
    (value & 15, value >> 4)
}

unsafe fn dual_coefficients(descriptor: OutputDescriptorAddress) -> DualCoefficientBlock {
    if unsafe { word(descriptor.0, 0xe8) } == 0 {
        return DualCoefficientBlock {
            active: 1,
            first: CoefficientHalves([0, 0, 512, 136, 0x1f88]),
            second: CoefficientHalves([0; 5]),
            gains: CoefficientBytes([16, 16]),
        };
    }
    let (first, second) = unsafe { dual_selections(descriptor) };
    let (first_values, gains) = if first == 0 {
        (CoefficientHalves([0; 5]), CoefficientBytes([16, 16]))
    } else {
        let values = unsafe { coefficient_halves::<5>(ConfigurationKeyTable(Address32(0x00d621d8).offset(ByteOffset((first as u32 - 1) * 10)))) };
        let gains = unsafe { coefficient_bytes::<2>(ConfigurationKeyTable(Address32(0x00d61c1c).offset(ByteOffset((first as u32 - 1) * 4)))) };
        (values, gains)
    };
    let second_values = if second == 0 { CoefficientHalves([0; 5]) } else {
        unsafe { coefficient_halves::<5>(ConfigurationKeyTable(Address32(0x00d621d8).offset(ByteOffset((second as u32 - 1) * 10)))) }
    };
    DualCoefficientBlock { active: u8::from(first != 0 || second != 0), first: first_values, second: second_values, gains }
}

fn dual_coefficient(value: u16, index: usize) -> u16 {
    match index {
        0 | 1 => CoefficientPacking::NineBits.expand(value),
        2 => value,
        _ => CoefficientPacking::TwelveBits.expand(value),
    }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_select_dual_coefficients(descriptor: OutputDescriptorAddress, first: Address32, second: Address32) {
    let values = unsafe { dual_selections(descriptor) };
    unsafe { store_byte(first, 0, values.0); store_byte(second, 0, values.1); }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_expand_dual_coefficients(descriptor: OutputDescriptorAddress, output: ExpandedOutputAddress) {
    let coefficients = unsafe { dual_coefficients(descriptor) };
    unsafe { store_byte(output.0, 3, coefficients.active); }
    for (index, value) in coefficients.first.0.into_iter().enumerate() {
        unsafe { store_half(output.0, 0x46 + index as u32 * 2, dual_coefficient(value, index)); }
    }
    for (index, value) in coefficients.second.0.into_iter().enumerate() {
        unsafe { store_half(output.0, 0x50 + index as u32 * 2, dual_coefficient(value, index)); }
    }
    unsafe { store_byte(output.0, 0x5a, coefficients.gains.0[0]); store_byte(output.0, 0x5b, coefficients.gains.0[1]); }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_expand_channel_matrix(descriptor: OutputDescriptorAddress, output: ExpandedOutputAddress) {
    let index = combined_selection(ConfigurationSelection(unsafe { word(descriptor.0, 0x160) }), ConfigurationSelection(unsafe { word(descriptor.0, 0xf4) }));
    let selection = unsafe { table_byte(ConfigurationKeyTable(Address32(0x00d61db4)), index) } as u32;
    let values = if !(1..=6).contains(&selection) {
        unsafe { store_byte(output.0, 4, 0); }
        CoefficientHalves([1023, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0])
    } else {
        unsafe { coefficient_halves::<15>(ConfigurationKeyTable(Address32(0x00d61e80).offset(ByteOffset((selection - 1) * 30)))) }
    };
    for (index, value) in values.0.into_iter().enumerate() {
        unsafe { store_half(output.0, 8 + index as u32 * 2, CoefficientPacking::TenBits.expand(value)); }
    }
}

unsafe fn channel_pairs(descriptor: OutputDescriptorAddress) -> ChannelPairs {
    let mode = unsafe { word(descriptor.0, 0x104) };
    if mode == 2 { return ChannelPairs { header: CoefficientBytes([0; 4]), values: CoefficientHalves([0; 10]) }; }
    let index = mode.wrapping_add(unsafe { word(descriptor.0, 0x108) }.wrapping_mul(8));
    let selection = unsafe { table_byte(ConfigurationKeyTable(Address32(0x00d62040)), ConfigurationSelection(index)) } as u32;
    if !(1..=8).contains(&selection) {
        return ChannelPairs { header: CoefficientBytes([0; 4]), values: CoefficientHalves([0, 0, 0, 0, 0, 254, 254, 254, 254, 254]) };
    }
    let header = unsafe { coefficient_bytes::<4>(ConfigurationKeyTable(Address32(0x00d61d74).offset(ByteOffset((selection - 1) * 8)))) };
    let values = unsafe { coefficient_halves::<10>(ConfigurationKeyTable(Address32(0x00d61cd4).offset(ByteOffset((selection - 1) * 20)))) };
    ChannelPairs { header, values }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_expand_channel_pairs(descriptor: OutputDescriptorAddress, output: ExpandedOutputAddress) {
    let channels = unsafe { channel_pairs(descriptor) };
    for (index, value) in channels.header.0.into_iter().enumerate() {
        unsafe { store_byte(output.0, 0x5c + index as u32, value); }
    }
    for index in 0..5 {
        unsafe {
            store_half(output.0, 0x60 + index as u32 * 4, CoefficientPacking::TenBits.expand(channels.values.0[index]));
            store_half(output.0, 0x62 + index as u32 * 4, channels.values.0[index + 5]);
        }
    }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_read_coefficient_header(output: Address32) {
    unsafe { xm1_pipeline_sensor_configuration_key_bytes(output.0, 0x00d621d0, 4); }
}
#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_read_coefficient_channels(output: Address32, primary: ConfigurationSelection) {
    let keys = Address32(0x00d61c70).offset(ByteOffset(primary.0.wrapping_mul(10)));
    unsafe { xm1_pipeline_sensor_configuration_key_bytes(output.0, keys.0, 5); }
}

unsafe fn header_channels(descriptor: OutputDescriptorAddress, weights: ConfigurationKeyTable) -> HeaderChannels {
    let header = unsafe { coefficient_bytes::<4>(ConfigurationKeyTable(Address32(0x00d621d0))) };
    let primary = unsafe { word(descriptor.0, 0xf4) };
    let channels = unsafe { coefficient_bytes::<5>(ConfigurationKeyTable(Address32(0x00d61c70).offset(ByteOffset(primary.wrapping_mul(10))))) };
    let primary = unsafe { word(descriptor.0, 0xf4) };
    let weights = unsafe { coefficient_halves::<5>(ConfigurationKeyTable(weights.0.offset(ByteOffset(primary.wrapping_mul(10))))) };
    HeaderChannels { header, channels, weights }
}

unsafe fn store_header_channels(output: ExpandedOutputAddress, offset: ByteOffset, channels: HeaderChannels) {
    for (index, value) in channels.header.0.into_iter().enumerate() {
        unsafe { store_byte(output.0, offset.0 + index as u32, value); }
    }
    for index in 0..5 {
        unsafe {
            store_byte(output.0, offset.0 + 4 + index as u32 * 4, channels.channels.0[index]);
            store_half(output.0, offset.0 + 6 + index as u32 * 4, channels.weights.0[index]);
        }
    }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_expand_header_channels(descriptor: OutputDescriptorAddress, output: ExpandedOutputAddress) {
    let channels = unsafe { header_channels(descriptor, ConfigurationKeyTable(Address32(0x00d61fdc))) };
    unsafe { store_header_channels(output, ByteOffset(0x2e), channels); }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_expand_channel_gains(descriptor: OutputDescriptorAddress, output: ExpandedOutputAddress) {
    let index = combined_selection(ConfigurationSelection(unsafe { word(descriptor.0, 0x160) }), ConfigurationSelection(unsafe { word(descriptor.0, 0xf4) }));
    let first = unsafe { table_byte(ConfigurationKeyTable(Address32(0x00d62060)), index) };
    let index = combined_selection(ConfigurationSelection(unsafe { word(descriptor.0, 0x160) }), ConfigurationSelection(unsafe { word(descriptor.0, 0xf4) }));
    let second = unsafe { table_byte(ConfigurationKeyTable(Address32(0x00d62270)), index) };
    let index = combined_selection(ConfigurationSelection(unsafe { word(descriptor.0, 0x160) }), ConfigurationSelection(unsafe { word(descriptor.0, 0xf4) }));
    let third = unsafe { table_byte(ConfigurationKeyTable(Address32(0x00d620e4)), index) };
    unsafe { store_byte(output.0, 0x74, first); store_byte(output.0, 0x75, second); store_byte(output.0, 0x76, third); }
}

#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub(super) enum GainMixMode { Direct = 0, Interpolated = 1, Constant = 2 }

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_configuration_feature_enabled(descriptor: OutputDescriptorAddress) -> u32 {
    let (key, mask) = match unsafe { word(descriptor.0, 0xe4) } {
        1 => (0xb432, 2), 49 | 50 => (0xb432, 0x20), 5 => (0xb432, 0x40),
        6 => (0xb432, 0x80), 8 => (0xb433, 2), 9 => (0xb433, 4),
        10 => (0xb433, 8), 14 => (0xb433, 0x10), 11 | 12 => (0xb433, 0x20),
        7 => (0xb433, 0x40), 13 => (0xb433, 0x80), 29 => (0xb434, 1),
        30 => (0xb434, 2), 31 => (0xb434, 4), 32 => (0xb434, 8),
        33 => (0xb434, 0x10), 34 => (0xb434, 0x20), 28 => (0xb434, 0x40),
        25 => (0xb434, 0x80), 26 => (0xb435, 1), 27 => (0xb435, 2),
        24 => (0xb435, 4), 23 => (0xb435, 8), 20 => (0xb435, 0x10),
        21 => (0xb435, 0x20), 22 => (0xb435, 0x40), 40 => (0xb435, 0x80),
        41 => (0xb436, 1), 42 => (0xb436, 2), 43 => (0xb436, 4),
        44 => (0xb436, 8), 45 => (0xb436, 0x10), 46 => (0xb436, 0x20),
        47 => (0xb436, 0x40), 48 => (0xb436, 0x80), _ => (0xb432, 1),
    };
    u32::from(unsafe { configuration_byte(ConfigurationKey(key)) } & mask != 0)
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_gain_mix_mode(descriptor: OutputDescriptorAddress) -> GainMixMode {
    if unsafe { word(descriptor.0, 0xe4) } == 0x25 { return GainMixMode::Constant; }
    if unsafe { xm1_pipeline_image_configuration_feature_enabled(descriptor) } == 1
        && unsafe { word(descriptor.0, 0x150) } != 0 {
        GainMixMode::Interpolated
    } else { GainMixMode::Direct }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_expand_channel_scaling(descriptor: OutputDescriptorAddress, output: ExpandedOutputAddress) {
    let primary = unsafe { selected_primary(descriptor) };
    let mut mode = 0u8;
    unsafe { xm1_pipeline_image_encode_configuration_mode(descriptor, Address32(&raw mut mode as u32)); }
    let exposure = unsafe { xm1_pipeline_image_configuration_mode_value(descriptor, ConfigurationLimit(11)) };
    let selection = combined_selection(ConfigurationSelection(unsafe { word(descriptor.0, 0x160) }), primary);
    let coefficients = unsafe { coefficient_bytes::<4>(ConfigurationKeyTable(Address32(0x00d63658).offset(ByteOffset(selection.0.wrapping_mul(8))))) };
    let adjustment = unsafe { table_byte(ConfigurationKeyTable(Address32(0x00d63974)), ConfigurationSelection(word(descriptor.0, 0x194))) };
    let exposure = unsafe { table_byte(ConfigurationKeyTable(Address32(0x00d63980)), ConfigurationSelection(exposure)) };
    let level = unsafe { table_byte(ConfigurationKeyTable(Address32(0x00d63210)), ConfigurationSelection(word(descriptor.0, 0x13c))) };
    let mode = unsafe { table_byte(ConfigurationKeyTable(Address32(0x00d63224)), ConfigurationSelection(mode as u32)) };
    let scale = unsafe { table_byte(ConfigurationKeyTable(Address32(0x00d6396c)), ConfigurationSelection(word(descriptor.0, 0x160))) } as u32;
    let gain = adjustment as u64 * exposure as u64 * level as u64 * mode as u64;
    for (index, coefficient) in coefficients.0.into_iter().enumerate() {
        let value = ((gain * coefficient as u64) >> 16) as u32;
        let scaled = value.wrapping_mul(scale) >> 4;
        unsafe {
            store_byte(output.0, 0x1a + index as u32 * 2, value.min(255) as u8);
            store_byte(output.0, 0x1b + index as u32 * 2, scaled.min(255) as u8);
        }
    }
}

fn expand_packed_byte(value: u8, high_mask: u8) -> u8 {
    if value & high_mask == 0 { value } else { value | high_mask }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_expand_channel_limits(descriptor: OutputDescriptorAddress, output: ExpandedOutputAddress) {
    let primary = unsafe { selected_primary(descriptor) };
    let selection = combined_selection(ConfigurationSelection(unsafe { word(descriptor.0, 0x160) }), primary);
    let values = unsafe { coefficient_bytes::<8>(ConfigurationKeyTable(Address32(0x00d633d8).offset(ByteOffset(selection.0.wrapping_mul(16))))) };
    unsafe { store_byte(output.0, 0x22, values.0[0]); store_byte(output.0, 0x23, values.0[1]); }
    for index in 0..3 {
        unsafe {
            store_byte(output.0, 0x24 + index as u32 * 2, values.0[3 + index * 2]);
            store_byte(output.0, 0x25 + index as u32 * 2, expand_packed_byte(values.0[2 + index * 2], 0xc0));
        }
    }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_expand_mix_limits(descriptor: OutputDescriptorAddress, output: ExpandedOutputAddress) {
    let primary = unsafe { selected_primary(descriptor) };
    let mode = unsafe { xm1_pipeline_image_gain_mix_mode(descriptor) };
    let key = if mode == GainMixMode::Constant { ConfigurationKey(0xb671) } else {
        let table = if mode == GainMixMode::Interpolated { 0x00d637bc } else { 0x00d63814 };
        let index = combined_selection(ConfigurationSelection(unsafe { word(descriptor.0, 0x160) }), primary);
        ConfigurationKey(unsafe { half(Address32(table), index.0.wrapping_mul(2)) } as u32)
    };
    let selection = unsafe { configuration_byte(key) } as u32;
    let values = if (1..=15).contains(&selection) {
        unsafe { coefficient_bytes::<8>(ConfigurationKeyTable(Address32(0x00d6386c).offset(ByteOffset((selection - 1) * 16)))) }
    } else { CoefficientBytes([0x40, 0x80, 0, 4, 0, 4, 0, 4]) };
    unsafe { store_byte(output.0, 0xb, values.0[0]); store_byte(output.0, 0xc, values.0[1]); }
    for index in 0..3 {
        unsafe {
            store_half(output.0, 0xe + index as u32 * 4, values.0[3 + index * 2] as u16);
            store_byte(output.0, 0x10 + index as u32 * 4, expand_packed_byte(values.0[2 + index * 2], 0xe0));
        }
    }
}

#[derive(Clone, Copy)]
struct ChannelGainPair { base: u32, scaled: u32 }

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_expand_mixed_channel_gains(descriptor: OutputDescriptorAddress, output: ExpandedOutputAddress) {
    let mode = unsafe { xm1_pipeline_image_gain_mix_mode(descriptor) };
    let result = if mode == GainMixMode::Constant {
        core::array::from_fn::<_, 4, _>(|_| {
            let value = unsafe { configuration_byte(ConfigurationKey(0xb577)) } as u32;
            ChannelGainPair { base: value, scaled: value }
        })
    } else {
        let primary = unsafe { selected_primary(descriptor) };
        let exposure = unsafe { xm1_pipeline_image_configuration_mode_value(descriptor, ConfigurationLimit(11)) };
        let exposure = unsafe { table_byte(ConfigurationKeyTable(Address32(0x00d637a4)), ConfigurationSelection(exposure)) };
        let level = unsafe { table_byte(ConfigurationKeyTable(Address32(0x00d631fc)), ConfigurationSelection(word(descriptor.0, 0x13c))) };
        let adjustment = unsafe { table_byte(ConfigurationKeyTable(Address32(0x00d63798)), ConfigurationSelection(word(descriptor.0, 0x194))) };
        let special = if unsafe { word(descriptor.0, 0x160) } == 3 {
            unsafe { table_byte(ConfigurationKeyTable(Address32(0x00d63284)), ConfigurationSelection(word(descriptor.0, 0x154))) }
        } else { 32 };
        let (kind, offset) = if unsafe { word(descriptor.0, 0xe4) } == 0x35 {
            let kind = unsafe { table_byte(ConfigurationKeyTable(Address32(0x00d63864)), ConfigurationSelection(word(descriptor.0, 0x1b0))) };
            let offset = unsafe { table_byte(ConfigurationKeyTable(Address32(0x00d6380c)), ConfigurationSelection(word(descriptor.0, 0x1b0))) } as i8 as i32;
            (kind, offset)
        } else { (32, 0) };
        let weights = unsafe { coefficient_bytes::<4>(ConfigurationKeyTable(Address32(0x00d63290))) };
        let selection = combined_selection(ConfigurationSelection(unsafe { word(descriptor.0, 0x160) }), primary);
        let first = unsafe { coefficient_bytes::<4>(ConfigurationKeyTable(Address32(0x00d63298).offset(ByteOffset(selection.0.wrapping_mul(8))))) };
        let first_scale = unsafe { table_byte(ConfigurationKeyTable(Address32(0x00d6395c)), ConfigurationSelection(word(descriptor.0, 0x160))) } as u64;
        let gain = exposure as u64 * level as u64 * adjustment as u64 * special as u64 * kind as u64;
        let first: [ChannelGainPair; 4] = core::array::from_fn(|index| {
            let coefficient = (first.0[index] as i32 + offset).clamp(0, 31) as u64;
            let base = ((gain * coefficient) >> 25) as u32;
            let scaled = ((base as u64 * first_scale * weights.0[index] as u64) >> 10) as u32;
            ChannelGainPair { base: base.min(31), scaled: scaled.min(31) }
        });
        let selection = combined_selection(ConfigurationSelection(unsafe { word(descriptor.0, 0x160) }), primary);
        let second = unsafe { coefficient_bytes::<4>(ConfigurationKeyTable(Address32(0x00d6305c).offset(ByteOffset(selection.0.wrapping_mul(8))))) };
        let second_scale = unsafe { table_byte(ConfigurationKeyTable(Address32(0x00d63964)), ConfigurationSelection(word(descriptor.0, 0x160))) } as u32;
        let second: [ChannelGainPair; 4] = core::array::from_fn(|index| {
            let coefficient = (second.0[index] as i32 + offset).clamp(0, 31) as u64;
            let base = ((gain * coefficient) >> 25) as u32;
            let scaled = base.wrapping_mul(second_scale).wrapping_mul(weights.0[index] as u32) >> 10;
            ChannelGainPair { base: base.min(31), scaled: scaled.min(31) }
        });
        let thresholds = unsafe { coefficient_bytes::<2>(ConfigurationKeyTable(Address32(0x00d631b0).offset(ByteOffset(word(descriptor.0, 0x160).wrapping_mul(4))))) };
        if mode == GainMixMode::Direct { first } else {
            let value = unsafe { word(descriptor.0, 0x150) };
            let low = thresholds.0[0] as u32;
            let high = thresholds.0[1] as u32;
            if value <= low { first } else if value > high { second } else {
                let distance = value - low;
                let span = high - low;
                core::array::from_fn(|index| {
                    let interpolate = |lower: u32, upper: u32| {
                        upper.wrapping_add(distance.wrapping_mul(upper.wrapping_sub(lower)) / span).min(31)
                    };
                    ChannelGainPair { base: interpolate(first[index].base, second[index].base), scaled: interpolate(first[index].scaled, second[index].scaled) }
                })
            }
        }
    };
    for (index, value) in result.into_iter().enumerate() {
        unsafe { store_byte(output.0, 3 + index as u32 * 2, value.base as u8); store_byte(output.0, 4 + index as u32 * 2, value.scaled as u8); }
    }
}

#[derive(Clone, Copy)]
#[repr(transparent)]
pub(super) struct TemplateGroups(pub(super) u32);

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_expand_template_selectors(descriptor: OutputDescriptorAddress, groups: TemplateGroups, output: ExpandedOutputAddress) {
    let primary = unsafe { selected_primary(descriptor) };
    for (mask, table, offset) in [(1, 0x00d63e00, 4), (2, 0x00d63e50, 0xd)] {
        if groups.0 & mask != 0 {
            let selection = combined_selection(ConfigurationSelection(unsafe { word(descriptor.0, 0x160) }), primary);
            let value = unsafe { table_byte(ConfigurationKeyTable(Address32(table)), selection) };
            unsafe { store_byte(output.0, offset, value); }
        }
    }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_expand_template_gains(descriptor: OutputDescriptorAddress, groups: TemplateGroups, output: ExpandedOutputAddress) {
    let primary = unsafe { selected_primary(descriptor) };
    let tables = [0x00d63f90, 0x00d63ef0, 0x00d63db0, 0x00d64030, 0x00d63fe0, 0x00d63f40, 0x00d63ea0, 0x00d64080];
    let values: [u8; 8] = core::array::from_fn(|index| {
        let selection = combined_selection(ConfigurationSelection(unsafe { word(descriptor.0, 0x160) }), primary);
        unsafe { table_byte(ConfigurationKeyTable(Address32(tables[index])), selection) }
    });
    let option = unsafe { word(descriptor.0, 0x104) };
    let (first_mode, second_mode) = if option <= 7 && (1u32 << option) & 0xf1 != 0 {
        let first = unsafe { table_byte(ConfigurationKeyTable(Address32(0x00d63d74)), ConfigurationSelection(option)) };
        let second = unsafe { table_byte(ConfigurationKeyTable(Address32(0x00d63d84)), ConfigurationSelection(word(descriptor.0, 0x104))) };
        (first as u32, second as u32)
    } else { (64, 64) };
    let (first_kind, second_kind) = if unsafe { word(descriptor.0, 0xe4) } == 0x35 {
        let first = unsafe { table_byte(ConfigurationKeyTable(Address32(0x00d63d6c)), ConfigurationSelection(word(descriptor.0, 0x1b0))) };
        let second = unsafe { table_byte(ConfigurationKeyTable(Address32(0x00d63d94)), ConfigurationSelection(word(descriptor.0, 0x1b0))) };
        (first as u32, second as u32)
    } else { (64, 64) };
    for (mask, offset, index, mode, kind) in [(1, 5, 0, first_mode, first_kind), (2, 0xe, 4, second_mode, second_kind)] {
        if groups.0 & mask != 0 {
            let first = (values[index + 1] as u32).wrapping_mul(32).wrapping_mul(mode).wrapping_mul(kind) >> 17;
            let second = (values[index + 2] as u32).wrapping_mul(32).wrapping_mul(mode).wrapping_mul(kind) >> 17;
            unsafe {
                store_byte(output.0, offset, values[index]);
                store_byte(output.0, offset + 1, first.min(63) as u8);
                store_byte(output.0, offset + 2, second.min(63) as u8);
                store_byte(output.0, offset + 3, values[index + 3]);
            }
        }
    }
}

unsafe extern "C" {
    fn xm1_pipeline_image_expand_noise_controls(descriptor: OutputDescriptorAddress, block: Address32);
    fn xm1_pipeline_image_expand_lookup_controls(descriptor: OutputDescriptorAddress, block: Address32);
    fn xm1_pipeline_image_build_blended_lookup(descriptor: OutputDescriptorAddress, header: Address32, curve: Address32, extra: Address32);
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_expand_template_noise(descriptor: OutputDescriptorAddress, output: ExpandedOutputAddress) {
    let mut controls = core::mem::MaybeUninit::<[u16; 22]>::uninit();
    let controls = Address32(controls.as_mut_ptr() as u32);
    unsafe { xm1_pipeline_image_expand_noise_controls(descriptor, controls); }
    let mode = unsafe { word(descriptor.0, 0x140) };
    let table = match mode { 3 | 4 => 0x00d66850, 5 | 6 => 0x00d66548, _ => 0x00d66cf8 };
    let mut gain = unsafe { table_byte(ConfigurationKeyTable(Address32(table)), ConfigurationSelection(word(descriptor.0, 0xf4))) } as u32;
    let primary = unsafe { word(descriptor.0, 0xf4) };
    if primary != 9 {
        let mode = unsafe { word(descriptor.0, 0x140) };
        let table = match mode { 3 | 4 => 0x00d66850, 5 | 6 => 0x00d66548, _ => 0x00d66cf8 };
        let next = unsafe { table_byte(ConfigurationKeyTable(Address32(table)), ConfigurationSelection(primary.wrapping_add(1))) } as u32;
        let scale = unsafe { configuration_byte(ConfigurationKey(0xc0ee)) } as u32;
        if gain < next {
            let fraction = (unsafe { word(descriptor.0, 0x130) } as i32 % 100) as u32;
            gain = gain.wrapping_add((next - gain).wrapping_mul(fraction).wrapping_mul(scale) / 6400) as u16 as u32;
        }
    }
    let scale = unsafe { configuration_byte(ConfigurationKey(0xc0ef)) };
    let option = unsafe { word(descriptor.0, 0x104) };
    let option_gain = if option.wrapping_sub(2) <= 5 {
        unsafe { table_byte(ConfigurationKeyTable(Address32(0x00d66704)), ConfigurationSelection(option)) }
    } else { 64 };
    let mode_gain = if unsafe { word(descriptor.0, 0x140) }.wrapping_sub(7) <= 1 {
        unsafe { configuration_byte(ConfigurationKey(0xc0f6)) }
    } else { 64 };
    let gain = ((gain as u64 * scale as u64 * option_gain as u64 * mode_gain as u64) >> 18) as u32;
    let table = match unsafe { word(descriptor.0, 0x140) } {
        3 | 4 => 0x00d66700, 5 | 6 => 0x00d66ea8, _ => 0x00d66af0,
    };
    let first_key = ConfigurationKey(unsafe { half(Address32(table), 0) } as u32);
    let second_key = ConfigurationKey(unsafe { half(Address32(table), 2) } as u32);
    let first = unsafe { configuration_byte(first_key) } as u32;
    let second = unsafe { configuration_byte(second_key) } as u32;
    let first = first.wrapping_add(gain).min(511) as u16;
    let middle = gain.min(511) as u16;
    let second = second.wrapping_add(gain).min(511) as u16;
    let selector = unsafe { table_byte(ConfigurationKeyTable(Address32(0x00d66920)), ConfigurationSelection(word(descriptor.0, 0xf4))) };
    let control_gain = unsafe { byte(controls, 5) } as u32;
    let mode = unsafe { word(descriptor.0, 0x140) };
    let primary = unsafe { word(descriptor.0, 0xf4) };
    let low = unsafe { configuration_byte(ConfigurationKey(0xc0f7)) } as u32;
    let high = unsafe { configuration_byte(ConfigurationKey(0xc0f8)) } as u32;
    let band = if low >= high || primary <= low { 0 } else if primary < high { 1 } else { 2 };
    let table = match mode { 5 | 6 => 0x00d66c08, 7 | 8 => 0x00d66ea0, _ => 0x00d666f0 };
    let scale = unsafe { table_byte(ConfigurationKeyTable(Address32(table)), ConfigurationSelection(band)) } as u32;
    unsafe {
        store_byte(output.0, 0x15, byte(controls, 4));
        store_byte(output.0, 0x16, (control_gain * scale >> 6) as u8);
        store_half(output.0, 0x18, half(controls, 8));
        store_half(output.0, 0x1a, half(controls, 10));
        store_half(output.0, 0x1c, half(controls, 12));
        store_half(output.0, 0x1e, 0);
        store_half(output.0, 0x20, first);
        store_half(output.0, 0x22, middle);
        store_half(output.0, 0x24, second);
        store_byte(output.0, 0x26, selector);
    }
    for index in 0..5 {
        let first = unsafe { half(controls, 24 + index * 4) };
        let second = unsafe { half(controls, 22 + index * 4) };
        unsafe { store_half(output.0, 0x28 + index * 4, first); store_half(output.0, 0x2a + index * 4, second); }
    }
    unsafe { store_half(output.0, 0x3c, half(controls, 42)); }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_expand_template_lookup_controls(descriptor: OutputDescriptorAddress, output: ExpandedOutputAddress) {
    let mut packet = core::mem::MaybeUninit::<[u8; 7]>::uninit();
    let packet = Address32(packet.as_mut_ptr() as u32);
    unsafe { xm1_pipeline_image_expand_lookup_controls(descriptor, packet); }
    let values = unsafe { [byte(packet, 3), byte(packet, 4), byte(packet, 5), byte(packet, 6)] };
    for (index, value) in values.into_iter().enumerate() {
        unsafe { store_byte(output.0, 0x4f + index as u32, value); }
    }
}

unsafe fn store_half_pairs(source: Address32, output: Address32, offset: ByteOffset, pairs: u32) {
    for index in 0..pairs {
        let first = unsafe { half(source, index * 4) };
        let second = unsafe { half(source, index * 4 + 2) };
        unsafe { store_half(output, offset.0 + index * 4, first); store_half(output, offset.0 + index * 4 + 2, second); }
    }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_store_template_lookup(header: Address32, curve: Address32, extra: Address32, output: ExpandedOutputAddress) {
    if header.0 != 0 {
        let prefix: [u8; 8] = core::array::from_fn(|index| unsafe { byte(header, index as u32) });
        for (index, value) in prefix.into_iter().enumerate() { unsafe { store_byte(output.0, 0x92 + index as u32, value); } }
        unsafe { store_byte(output.0, 0x9a, byte(header, 8)); }
    }
    if curve.0 != 0 { unsafe { store_half_pairs(curve, output.0, ByteOffset(0x9c), 14); } }
    if extra.0 != 0 { unsafe { store_half(output.0, 0xd4, half(extra, 0)); store_half(output.0, 0xd6, 0x3fff); } }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_forward_template_lookup(descriptor: OutputDescriptorAddress, header: Address32, curve: Address32, extra: Address32) {
    unsafe { xm1_pipeline_image_build_blended_lookup(descriptor, header, curve, extra); }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_expand_template_lookup(descriptor: OutputDescriptorAddress, output: ExpandedOutputAddress) {
    let mut header = core::mem::MaybeUninit::<[u8; 9]>::uninit();
    let mut curve = core::mem::MaybeUninit::<[u16; 28]>::uninit();
    let mut extra = core::mem::MaybeUninit::<u16>::uninit();
    let header = Address32(header.as_mut_ptr() as u32);
    let curve = Address32(curve.as_mut_ptr() as u32);
    let extra = Address32(extra.as_mut_ptr() as u32);
    unsafe {
        xm1_pipeline_image_forward_template_lookup(descriptor, header, curve, extra);
        xm1_pipeline_image_store_template_lookup(header, curve, extra, output);
    }
}

unsafe extern "C" {
    fn xm1_pipeline_image_build_export_coefficients(descriptor: OutputDescriptorAddress, curve: Address32);
    fn xm1_pipeline_image_build_matrix_coefficients(packet: Address32, descriptor: OutputDescriptorAddress);
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_forward_export_coefficients(descriptor: OutputDescriptorAddress, curve: Address32) {
    unsafe { xm1_pipeline_image_build_export_coefficients(descriptor, curve); }
}
#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_store_export_coefficients(curve: Address32, output: ExpandedOutputAddress) {
    unsafe { store_half_pairs(curve, output.0, ByteOffset(0x54), 11); }
}
#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_expand_template_export_coefficients(descriptor: OutputDescriptorAddress, output: ExpandedOutputAddress) {
    let mut curve = core::mem::MaybeUninit::<[u16; 22]>::uninit();
    let curve = Address32(curve.as_mut_ptr() as u32);
    unsafe { xm1_pipeline_image_forward_export_coefficients(descriptor, curve); xm1_pipeline_image_store_export_coefficients(curve, output); }
}
#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_forward_matrix_coefficients(packet: Address32, descriptor: OutputDescriptorAddress) {
    unsafe { xm1_pipeline_image_build_matrix_coefficients(packet, descriptor); }
}
#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_store_template_matrix_coefficients(packet: Address32, output: ExpandedOutputAddress) {
    unsafe { store_byte(output.0, 4, byte(packet, 0)); }
    for group in 0..3 {
        let second = unsafe { half(packet, 4 + group * 6) };
        let first = unsafe { half(packet, 2 + group * 6) };
        unsafe { store_half(output.0, 0x80 + group * 6, first); store_half(output.0, 0x82 + group * 6, second); }
        unsafe { store_half(output.0, 0x84 + group * 6, half(packet, 6 + group * 6)); }
    }
}
#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_expand_template_matrix_coefficients(descriptor: OutputDescriptorAddress, output: ExpandedOutputAddress) {
    let mut packet = core::mem::MaybeUninit::<[u16; 10]>::uninit();
    let packet = Address32(packet.as_mut_ptr() as u32);
    unsafe { xm1_pipeline_image_forward_matrix_coefficients(packet, descriptor); xm1_pipeline_image_store_template_matrix_coefficients(packet, output); }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_expand_template_header_channels(descriptor: OutputDescriptorAddress, output: ExpandedOutputAddress) {
    let header = unsafe { coefficient_bytes::<4>(ConfigurationKeyTable(Address32(0x00d621d0))) };
    let primary = unsafe { word(descriptor.0, 0xf4) };
    let channels = unsafe { coefficient_bytes::<5>(ConfigurationKeyTable(Address32(0x00d61c70).offset(ByteOffset(primary.wrapping_mul(10))))) };
    let table = match unsafe { word(descriptor.0, 0x140) } { 5 => 0x00d667c0, 6 => 0x00d6658c, _ => 0x00d669b8 };
    let primary = unsafe { word(descriptor.0, 0xf4) };
    let weights = unsafe { coefficient_halves::<5>(ConfigurationKeyTable(Address32(table).offset(ByteOffset(primary.wrapping_mul(10))))) };
    unsafe { store_header_channels(output, ByteOffset(0xe4), HeaderChannels { header, channels, weights }); }
}
#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_expand_template_dual_coefficients(descriptor: OutputDescriptorAddress, output: ExpandedOutputAddress) {
    let channels = unsafe { dual_coefficients(descriptor) };
    unsafe { store_byte(output.0, 10, channels.active); }
    for (group, values) in [channels.first, channels.second].into_iter().enumerate() {
        for (index, value) in values.0.into_iter().enumerate() {
            unsafe { store_half(output.0, 0xfc + group as u32 * 10 + index as u32 * 2, dual_coefficient(value, index)); }
        }
    }
    unsafe { store_byte(output.0, 0x110, channels.gains.0[0]); store_byte(output.0, 0x111, channels.gains.0[1]); }
}
#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_expand_template_channel_pairs(descriptor: OutputDescriptorAddress, output: ExpandedOutputAddress) {
    let channels = unsafe { channel_pairs(descriptor) };
    for (index, value) in channels.header.0.into_iter().enumerate() { unsafe { store_byte(output.0, 0x112 + index as u32, value); } }
    for index in 0..5 {
        unsafe {
            store_half(output.0, 0x116 + index as u32 * 4, CoefficientPacking::TenBits.expand(channels.values.0[index]));
            store_half(output.0, 0x118 + index as u32 * 4, channels.values.0[index + 5]);
        }
    }
}

unsafe fn store_coefficient_bytes<const N: usize>(output: ExpandedOutputAddress, offset: ByteOffset, values: CoefficientBytes<N>) {
    for (index, value) in values.0.into_iter().enumerate() { unsafe { store_byte(output.0, offset.0 + index as u32, value); } }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_expand_template_triplet(descriptor: OutputDescriptorAddress, output: ExpandedOutputAddress) {
    let mode = unsafe { word(descriptor.0, 0x140) };
    let (table, index) = match mode {
        5 => (0x00d66824, ConfigurationSelection(unsafe { word(descriptor.0, 0xf4) })),
        6 => (0x00d66c24, ConfigurationSelection(unsafe { word(descriptor.0, 0xf4) })),
        _ => (0x00d66a74, combined_selection(ConfigurationSelection(unsafe { word(descriptor.0, 0x128) }), ConfigurationSelection(unsafe { word(descriptor.0, 0xf4) }))),
    };
    let selection = unsafe { table_byte(ConfigurationKeyTable(Address32(table)), index) } as u32;
    let values = if (1..=4).contains(&selection) {
        unsafe { coefficient_bytes::<3>(ConfigurationKeyTable(Address32(0x00d667a8).offset(ByteOffset((selection - 1) * 6)))) }
    } else {
        unsafe { store_byte(output.0, 0xd, 0); }
        CoefficientBytes([14, 1, 0])
    };
    unsafe { store_coefficient_bytes(output, ByteOffset(0xd8), values); }
}
#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_expand_template_header(descriptor: OutputDescriptorAddress, output: ExpandedOutputAddress) {
    let table = match unsafe { word(descriptor.0, 0x140) } { 5 => 0x00d66d28, 6 => 0x00d66af8, _ => 0x00d6694c };
    let first = unsafe { coefficient_bytes::<3>(ConfigurationKeyTable(Address32(table))) };
    let second = unsafe { coefficient_bytes::<4>(ConfigurationKeyTable(Address32(0x00d66b54))) };
    unsafe {
        store_coefficient_bytes(output, ByteOffset(0x44), first);
        store_coefficient_bytes(output, ByteOffset(0x47), CoefficientBytes([second.0[2], second.0[1], second.0[0], second.0[3]]));
    }
}
#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_expand_template_controls(descriptor: OutputDescriptorAddress, output: ExpandedOutputAddress) {
    let table = match unsafe { word(descriptor.0, 0x140) } { 5 => 0x00d66a6c, 6 => 0x00d668b4, _ => 0x00d66604 };
    let values = unsafe { coefficient_bytes::<4>(ConfigurationKeyTable(Address32(table))) };
    unsafe { store_coefficient_bytes(output, ByteOffset(0x4b), values); }
}
#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_expand_template_channel_scaling(descriptor: OutputDescriptorAddress, output: ExpandedOutputAddress) {
    let table = match unsafe { word(descriptor.0, 0x140) } { 1..=4 => 0x00d668cc, 5 => 0x00d66bb8, 6 => 0x00d66b68, _ => 0x00d66968 };
    let primary = unsafe { word(descriptor.0, 0xf4) };
    let values = unsafe { coefficient_bytes::<4>(ConfigurationKeyTable(Address32(table).offset(ByteOffset(primary.wrapping_mul(8))))) };
    let first_gain = if unsafe { word(descriptor.0, 0x140) } == 3 { u32::from(unsafe { configuration_byte(ConfigurationKey(0xc305)) }) } else { 64 };
    let second_gain = if unsafe { word(descriptor.0, 0x140) } == 4 { u32::from(unsafe { configuration_byte(ConfigurationKey(0xc306)) }) } else { 64 };
    let first = (values.0[1] as u32 * first_gain * second_gain >> 12).min(63) as u8;
    let second = (values.0[2] as u32 * first_gain * second_gain >> 12).min(63) as u8;
    unsafe { store_coefficient_bytes(output, ByteOffset(0x12a), CoefficientBytes([values.0[0], first, second, values.0[3]])); }
}

unsafe fn template_override(descriptor: OutputDescriptorAddress, require_ordinary_source: bool) -> Option<ConfigurationSelection> {
    if unsafe { word(descriptor.0, 0x1d0) } != 1 { return None; }
    let selection = unsafe { word(descriptor.0, 0x1d8) };
    if selection == 0 { return None; }
    if require_ordinary_source && unsafe { word(descriptor.0, 0xe0) } != 0 { return None; }
    if unsafe { word(descriptor.0, 0x144) } != 0 { return None; }
    Some(ConfigurationSelection(selection))
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_expand_template_channel_matrix(descriptor: OutputDescriptorAddress, output: ExpandedOutputAddress) {
    let key = match unsafe { word(descriptor.0, 0x140) } {
        1 | 2 => {
            let extra = unsafe { template_override(descriptor, false) };
            let index = unsafe { word(descriptor.0, 0x128) }.wrapping_add(extra.map_or(0, |selection| selection.0.wrapping_mul(2)));
            ConfigurationKey(unsafe { half(Address32(0x00d66b5c), index.wrapping_mul(2)) } as u32)
        }
        3 | 4 => ConfigurationKey(unsafe { half(Address32(0x00d666e8), word(descriptor.0, 0x128).wrapping_mul(2)) } as u32),
        5 => {
            let extra = unsafe { template_override(descriptor, true) };
            let index = unsafe { word(descriptor.0, 0x148) }.wrapping_add(extra.map_or(0, |selection| selection.0.wrapping_mul(4)));
            ConfigurationKey(unsafe { half(Address32(0x00d66d48), index.wrapping_mul(2)) } as u32)
        }
        6 => ConfigurationKey(unsafe { half(Address32(0x00d66714), word(descriptor.0, 0x1dc).wrapping_mul(2)) } as u32),
        _ => ConfigurationKey(0xc3bc),
    };
    let selection = unsafe { configuration_byte(key) } as u32;
    let values = if (1..=6).contains(&selection) {
        unsafe { coefficient_halves::<12>(ConfigurationKeyTable(Address32(0x00d66d60).offset(ByteOffset((selection - 1) * 24)))) }
    } else { CoefficientHalves([511, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]) };
    for (index, value) in values.0.into_iter().enumerate() { unsafe { store_half(output.0, 0x12e + index as u32 * 2, CoefficientPacking::NineBits.expand(value)); } }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_expand_template_chroma_gain(descriptor: OutputDescriptorAddress, output: ExpandedOutputAddress) {
    let mode = unsafe { word(descriptor.0, 0x140) };
    let (table, index) = match mode {
        1..=4 => (0x00d66e70, combined_selection(ConfigurationSelection(unsafe { word(descriptor.0, 0x128) }), ConfigurationSelection(unsafe { word(descriptor.0, 0xf4) }))),
        5 => (0x00d66864, combined_selection(ConfigurationSelection(unsafe { word(descriptor.0, 0x148) }), ConfigurationSelection(unsafe { word(descriptor.0, 0xf4) }))),
        6 => (0x00d66df0, combined_selection(ConfigurationSelection(unsafe { word(descriptor.0, 0x1dc) }), ConfigurationSelection(unsafe { word(descriptor.0, 0xf4) }))),
        _ => (0x00d66838, ConfigurationSelection(unsafe { word(descriptor.0, 0xf4) })),
    };
    let gain = unsafe { table_byte(ConfigurationKeyTable(Address32(table)), index) } as u32;
    let extra = unsafe { template_override(descriptor, true) };
    let override_gain = if let Some(extra) = extra {
        let mode = unsafe { word(descriptor.0, 0x140) };
        if mode == 6 { 32 } else {
            let table = if mode == 5 { 0x00d66784 } else { 0x00d66d10 };
            let index = unsafe { word(descriptor.0, 0x128) }.wrapping_add(extra.0.wrapping_mul(2));
            let index = unsafe { word(descriptor.0, 0x1d4) }.wrapping_add(index.wrapping_mul(2));
            u32::from(unsafe { table_byte(ConfigurationKeyTable(Address32(table)), ConfigurationSelection(index)) })
        }
    } else { 32 };
    let selected = if unsafe { byte(output.0, 0x12) } == 0 { 32 } else { u32::from(unsafe { configuration_byte(ConfigurationKey(0xc56f)) }) };
    let value = selected.wrapping_mul(gain).wrapping_mul(override_gain) >> 10;
    unsafe { store_byte(output.0, 0x155, value.min(255) as u8); }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_expand_template_chroma_controls(descriptor: OutputDescriptorAddress, output: ExpandedOutputAddress) {
    let table = if unsafe { word(descriptor.0, 0x140) }.wrapping_sub(5) <= 1 { 0x00d666f8 } else { 0x00d66cdc };
    let values = unsafe { coefficient_bytes::<3>(ConfigurationKeyTable(Address32(table))) };
    unsafe { store_coefficient_bytes(output, ByteOffset(0x146), values); }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_expand_template_chroma_offsets(descriptor: OutputDescriptorAddress, output: ExpandedOutputAddress) {
    let (table, index) = match unsafe { word(descriptor.0, 0x140) } {
        5 => (0x00d663f4, combined_selection(ConfigurationSelection(unsafe { word(descriptor.0, 0x148) }), ConfigurationSelection(unsafe { word(descriptor.0, 0xf4) }))),
        6 => (0x00d66734, ConfigurationSelection(unsafe { word(descriptor.0, 0xf4) })),
        _ => (0x00d66620, combined_selection(ConfigurationSelection(unsafe { word(descriptor.0, 0x128) }), ConfigurationSelection(unsafe { word(descriptor.0, 0xf4) }))),
    };
    let values = unsafe { coefficient_bytes::<4>(ConfigurationKeyTable(Address32(table).offset(ByteOffset(index.0.wrapping_mul(8))))) };
    unsafe { store_coefficient_bytes(output, ByteOffset(0x149), values); }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_expand_template_chroma(descriptor: OutputDescriptorAddress, output: ExpandedOutputAddress) {
    let key = if let Some(extra) = unsafe { template_override(descriptor, true) } {
        ConfigurationKey(unsafe { half(Address32(0x00d66e68), extra.0.wrapping_mul(2)) } as u32)
    } else {
        let (table, index) = match unsafe { word(descriptor.0, 0x140) } {
            5 => (0x00d66c54, combined_selection(ConfigurationSelection(unsafe { word(descriptor.0, 0x148) }), ConfigurationSelection(unsafe { word(descriptor.0, 0xf4) }))),
            6 => (0x00d66954, ConfigurationSelection(unsafe { word(descriptor.0, 0xf4) })),
            _ => (0x00d66a30, combined_selection(ConfigurationSelection(unsafe { word(descriptor.0, 0x128) }), ConfigurationSelection(unsafe { word(descriptor.0, 0xf4) }))),
        };
        ConfigurationKey(unsafe { half(Address32(table), index.0.wrapping_mul(2)) } as u32)
    };
    let selection = unsafe { configuration_byte(key) } as u32;
    let values = if (1..=6).contains(&selection) {
        unsafe { coefficient_bytes::<8>(ConfigurationKeyTable(Address32(0x00d5f6fc).offset(ByteOffset((selection - 1) * 16)))) }
    } else { CoefficientBytes([16, 32, 128, 128, 128, 0, 0, 0]) };
    let order = [0, 1, 2, 5, 3, 6, 4, 7];
    for (index, source) in order.into_iter().enumerate() { unsafe { store_byte(output.0, 0x14d + index as u32, values.0[source]); } }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_expand_template_detail(descriptor: OutputDescriptorAddress, output: ExpandedOutputAddress) {
    let mode = unsafe { word(descriptor.0, 0x140) };
    let primary = unsafe { word(descriptor.0, 0xf4) };
    let (tables, first_index) = match mode {
        1..=4 => ([0x00d66ce4, 0x00d66a58, 0x00d66e54], primary),
        5 => ([0x00d66b00, 0x00d66720, 0x00d6660c], primary.wrapping_add(unsafe { word(descriptor.0, 0x148) }.wrapping_mul(10))),
        6 => ([0x00d66578, 0x00d66cb8, 0x00d665f0], primary),
        _ => ([0x00d66a1c, 0x00d66934, 0x00d66c10], primary),
    };
    let keys = unsafe {
        [ConfigurationKey(half(Address32(tables[0]), first_index.wrapping_mul(2)) as u32),
         ConfigurationKey(half(Address32(tables[1]), primary.wrapping_mul(2)) as u32),
         ConfigurationKey(half(Address32(tables[2]), primary.wrapping_mul(2)) as u32)]
    };
    let selection = unsafe { configuration_byte(keys[0]) } as u32;
    let values = if (1..=5).contains(&selection) {
        unsafe { coefficient_bytes::<10>(ConfigurationKeyTable(Address32(0x00d5f27c).offset(ByteOffset((selection - 1) * 20)))) }
    } else { CoefficientBytes([31, 0, 0, 0, 0, 0, 0, 0, 0, 0]) };
    let first = unsafe { configuration_byte(keys[1]) };
    let second = unsafe { configuration_byte(keys[2]) };
    for index in 0..6 { unsafe { store_byte(output.0, 0x156 + index as u32, values.0[index]); } }
    unsafe { store_byte(output.0, 0x15c, first); store_byte(output.0, 0x15d, second); }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_expand_template_detail_limits(descriptor: OutputDescriptorAddress, output: ExpandedOutputAddress) {
    let (tables, offset) = match unsafe { word(descriptor.0, 0x140) } {
        1 | 2 => ([0x00d6679c, 0x00d66cd8, 0x00d668bc], 0x128),
        3 | 4 => ([0x00d666ec, 0x00d6684c, 0x00d66948], 0x128),
        5 => ([0x00d66c38, 0x00d66e98, 0x00d66ca4], 0x148),
        6 => ([0x00d668c0, 0x00d66cac, 0x00d66534], 0x1dc),
        _ => ([0, 0, 0], 0),
    };
    let keys = if offset == 0 {
        [ConfigurationKey(0xc60b), ConfigurationKey(0xc618), ConfigurationKey(0xc625)]
    } else {
        let index = unsafe { word(descriptor.0, offset) }.wrapping_mul(2);
        core::array::from_fn(|index_of_table| ConfigurationKey(unsafe { half(Address32(tables[index_of_table]), index) } as u32))
    };
    let first = unsafe { configuration_byte(keys[0]) };
    let second = unsafe { configuration_byte(keys[1]) };
    let third = unsafe { configuration_byte(keys[2]) };
    unsafe {
        store_byte(output.0, 0x15e, first.min(128));
        store_byte(output.0, 0x15f, second.min(128));
        store_byte(output.0, 0x160, third);
    }
}

#[unsafe(no_mangle)]
pub(super) extern "C" fn xm1_pipeline_image_finish_template(_descriptor: OutputDescriptorAddress, _output: ExpandedOutputAddress) {}

unsafe extern "C" {
    fn xm1_pipeline_image_expand_noise_coefficients(descriptor: OutputDescriptorAddress, block: ExpandedOutputAddress);
    fn xm1_pipeline_image_expand_selected_lookup(descriptor: OutputDescriptorAddress, block: ExpandedOutputAddress);
    fn xm1_pipeline_image_expand_export_coefficients(descriptor: OutputDescriptorAddress, block: ExpandedOutputAddress);
    fn xm1_pipeline_image_expand_matrix_coefficients(descriptor: OutputDescriptorAddress, block: ExpandedOutputAddress);
    fn xm1_pipeline_image_expand_blended_lookup(descriptor: OutputDescriptorAddress, block: ExpandedOutputAddress);
    fn xm1_pipeline_image_expand_paired_lookup(descriptor: OutputDescriptorAddress, block: ExpandedOutputAddress);
    fn xm1_pipeline_image_expand_sensor_layout(descriptor: OutputDescriptorAddress, block: ExpandedOutputAddress);
    fn xm1_pipeline_image_expand_signed_channels(descriptor: OutputDescriptorAddress, block: ExpandedOutputAddress);
    fn xm1_pipeline_image_expand_auxiliary_channels(descriptor: OutputDescriptorAddress, block: ExpandedOutputAddress);
    fn xm1_pipeline_image_expand_threshold_curves(descriptor: OutputDescriptorAddress, block: ExpandedOutputAddress);
}

#[derive(Clone, Copy)]
#[repr(u32)]
enum ExpandedComponent {
    Noise = 0x20000, SavedNoise = 0x40000, Header = 0x80000,
    LookupControls = 0x100000, Matrix = 0x200000, Lookup = 0x400000,
    Triplet = 0x800000, Paired = 0x1000000, HeaderChannels = 0x2000000,
    Dual = 0x4000000, ChannelPairs = 0x8000000, ChannelScaling = 0x10000000,
    Chroma = 0x20000000, DetailLimits = 0x40000000,
}
unsafe fn mark_expanded(descriptor: OutputDescriptorAddress, component: ExpandedComponent) {
    let mask = unsafe { word(descriptor.0, 0x14c) } | component as u32;
    unsafe { store_word(descriptor.0, 0x14c, mask); }
}
fn output_block(output: ExpandedOutputAddress, offset: ByteOffset) -> ExpandedOutputAddress {
    ExpandedOutputAddress(output.0.offset(offset))
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_expand_output_descriptor(descriptor: OutputDescriptorAddress, output: ExpandedOutputAddress) {
    for (source, destination) in [(8, 4), (4, 5), (0, 6)] {
        unsafe { store_byte(output.0, destination, word(descriptor.0, source) as u8); }
    }
    if unsafe { word(descriptor.0, 8) } == 1 { unsafe { xm1_pipeline_image_expand_noise_coefficients(descriptor, output_block(output, ByteOffset(4))); } }
    let block = output_block(output, ByteOffset(0x30));
    unsafe { store_byte(output.0, 0x30, word(descriptor.0, 0x10) as u8); store_byte(output.0, 0x31, word(descriptor.0, 0xc) as u8); }
    if unsafe { word(descriptor.0, 0x10) } == 1 { unsafe { xm1_pipeline_image_expand_selected_lookup(descriptor, block); } }
    if unsafe { word(descriptor.0, 0x10) } == 2 { unsafe { xm1_pipeline_image_expand_selected_lookup(descriptor, block); } }
    unsafe { xm1_pipeline_image_expand_lookup_controls(descriptor, block.0); xm1_pipeline_image_expand_export_coefficients(descriptor, block); }
    let block = output_block(output, ByteOffset(0xa6));
    for (source, destination) in [(0x14, 0xd2), (0x18, 0xd1), (0x1c, 0xd0), (0x20, 0xcf)] {
        unsafe { store_byte(output.0, destination, word(descriptor.0, source) as u8); }
    }
    if unsafe { word(descriptor.0, 0x14) } == 1 { unsafe { xm1_pipeline_image_expand_matrix_coefficients(descriptor, block); } }
    if unsafe { word(descriptor.0, 0x18) } == 1 { unsafe { xm1_pipeline_image_expand_blended_lookup(descriptor, block); } }
    if unsafe { word(descriptor.0, 0x20) } == 1 { unsafe { xm1_pipeline_image_expand_paired_lookup(descriptor, block); } }
    if unsafe { word(descriptor.0, 0xd8) } & 2 != 0 {
        unsafe { xm1_pipeline_image_expand_sensor_layout(descriptor, block); xm1_pipeline_image_expand_signed_channels(descriptor, block); }
    }
    if unsafe { word(descriptor.0, 0xd8) } & 4 != 0 { unsafe { xm1_pipeline_image_expand_auxiliary_channels(descriptor, output_block(output, ByteOffset(0x19c))); } }
    unsafe { store_byte(output.0, 0x1a6, word(descriptor.0, 0x24) as u8); }
    if unsafe { word(descriptor.0, 0x24) } == 1 { unsafe { xm1_pipeline_image_expand_threshold_curves(descriptor, output_block(output, ByteOffset(0x1a6))); } }
    unsafe { store_byte(output.0, 0x226, word(descriptor.0, 0x2c) as u8); }
    if unsafe { word(descriptor.0, 0x2c) } == 1 { unsafe { xm1_pipeline_image_expand_output_profile(descriptor, output_block(output, ByteOffset(0x226))); } }
    unsafe { store_byte(output.0, 0x234, word(descriptor.0, 0x30) as u8); }
    if unsafe { word(descriptor.0, 0x30) } == 1 {
        let block = output_block(output, ByteOffset(0x234));
        unsafe { xm1_pipeline_image_expand_calibration_controls(descriptor, block); xm1_pipeline_image_expand_calibration_gain(descriptor, block); xm1_pipeline_image_expand_calibration_grid(descriptor, block); }
    }
    let block = output_block(output, ByteOffset(0xf24));
    for index in 0..8 { unsafe { store_byte(block.0, index, word(descriptor.0, 0x50 - index * 4) as u8); } }
    if unsafe { word(descriptor.0, 0x48) } == 1 { unsafe { xm1_pipeline_image_expand_pair_coefficients(descriptor, block); } }
    if unsafe { word(descriptor.0, 0x44) } == 1 { unsafe { xm1_pipeline_image_expand_dual_coefficients(descriptor, block); } }
    if unsafe { word(descriptor.0, 0x40) } == 1 { unsafe { xm1_pipeline_image_expand_channel_matrix(descriptor, block); } }
    if unsafe { word(descriptor.0, 0x3c) } == 1 { unsafe { xm1_pipeline_image_expand_channel_pairs(descriptor, block); } }
    if unsafe { word(descriptor.0, 0x38) } == 1 { unsafe { xm1_pipeline_image_expand_header_channels(descriptor, block); } }
    if unsafe { word(descriptor.0, 0x34) } != 0 { unsafe { xm1_pipeline_image_expand_channel_gains(descriptor, block); } }
    let block = output_block(output, ByteOffset(0xf9c));
    for index in 0..3 { unsafe { store_byte(block.0, index, word(descriptor.0, 0x5c - index * 4) as u8); } }
    if unsafe { word(descriptor.0, 0x5c) } == 1 { unsafe { xm1_pipeline_image_expand_mixed_channel_gains(descriptor, block); xm1_pipeline_image_expand_mix_limits(descriptor, block); } }
    if unsafe { word(descriptor.0, 0x54) } == 1 { unsafe { xm1_pipeline_image_expand_channel_scaling(descriptor, block); xm1_pipeline_image_expand_channel_limits(descriptor, block); } }
    let block = output_block(output, ByteOffset(0xfc6));
    for (source, destination) in [(0x60, 0), (0x64, 1), (0x68, 2), (0x6c, 3), (0x70, 9), (0x74, 10), (0x78, 11), (0x7c, 12)] {
        unsafe { store_byte(block.0, destination, word(descriptor.0, source) as u8); }
    }
    if unsafe { word(descriptor.0, 0x60) } == 1 { unsafe { xm1_pipeline_image_expand_template_gains(descriptor, TemplateGroups(1), block); } }
    if unsafe { word(descriptor.0, 0x70) } == 1 { unsafe { xm1_pipeline_image_expand_template_gains(descriptor, TemplateGroups(2), block); } }
    if unsafe { word(descriptor.0, 0x68) } == 1 { unsafe { xm1_pipeline_image_expand_template_selectors(descriptor, TemplateGroups(1), block); } }
    if unsafe { word(descriptor.0, 0x78) } == 1 { unsafe { xm1_pipeline_image_expand_template_selectors(descriptor, TemplateGroups(2), block); } }
    let block = output_block(output, ByteOffset(0xfd8));
    unsafe { store_byte(block.0, 0, word(descriptor.0, 0x84) as u8); store_byte(block.0, 1, word(descriptor.0, 0x80) as u8); }
    if unsafe { word(descriptor.0, 0x84) } != 0 {
        unsafe {
            xm1_pipeline_image_expand_filter_matrix(descriptor, block);
            xm1_pipeline_image_expand_filter_gain(descriptor, block);
            xm1_pipeline_image_expand_filter_triplet(descriptor, block);
            xm1_pipeline_image_expand_filter_offsets(descriptor, block);
            xm1_pipeline_image_expand_filter_chroma(descriptor, block);
        }
    }
    if unsafe { word(descriptor.0, 0x80) } == 1 { unsafe { xm1_pipeline_image_expand_filter_channels(descriptor, block); } }
    if unsafe { word(descriptor.0, 0x84) } == 2 { unsafe { xm1_pipeline_image_expand_detail_channels(descriptor, block); } }
    let block = output_block(output, ByteOffset(0x101c));
    for index in 0..20 { unsafe { store_byte(block.0, index + 1, word(descriptor.0, 0x88 + index * 4) as u8); } }
    if unsafe { word(descriptor.0, 0x88) } == 1 { unsafe { xm1_pipeline_image_expand_template_noise(descriptor, block); mark_expanded(descriptor, ExpandedComponent::Noise); } }
    if unsafe { word(descriptor.0, 0x8c) } == 1 {
        for index in 0..3 { unsafe { store_half(output.0, 0x105a + index * 2, half(descriptor.0, 0x1c8 + index * 2)); } }
        unsafe { mark_expanded(descriptor, ExpandedComponent::SavedNoise); }
    }
    unsafe { xm1_pipeline_image_expand_template_header(descriptor, block); xm1_pipeline_image_expand_template_controls(descriptor, block); mark_expanded(descriptor, ExpandedComponent::Header); }
    if unsafe { word(descriptor.0, 0x90) } == 1 { unsafe { xm1_pipeline_image_expand_template_lookup_controls(descriptor, block); xm1_pipeline_image_expand_template_export_coefficients(descriptor, block); mark_expanded(descriptor, ExpandedComponent::LookupControls); } }
    if unsafe { word(descriptor.0, 0x94) } == 1 { unsafe { xm1_pipeline_image_expand_template_matrix_coefficients(descriptor, block); mark_expanded(descriptor, ExpandedComponent::Matrix); } }
    if unsafe { word(descriptor.0, 0x98) } == 1 { unsafe { xm1_pipeline_image_expand_template_lookup(descriptor, block); mark_expanded(descriptor, ExpandedComponent::Lookup); } }
    if unsafe { word(descriptor.0, 0xa0) } == 1 { unsafe { xm1_pipeline_image_expand_template_triplet(descriptor, block); mark_expanded(descriptor, ExpandedComponent::Triplet); } }
    if unsafe { word(descriptor.0, 0xa4) } == 1 { unsafe { xm1_pipeline_image_expand_output_pair_coefficients(descriptor, block); mark_expanded(descriptor, ExpandedComponent::Paired); } }
    if unsafe { word(descriptor.0, 0xa8) } == 1 { unsafe { xm1_pipeline_image_expand_template_header_channels(descriptor, block); mark_expanded(descriptor, ExpandedComponent::HeaderChannels); } }
    if unsafe { word(descriptor.0, 0xac) } == 1 { unsafe { xm1_pipeline_image_expand_template_dual_coefficients(descriptor, block); mark_expanded(descriptor, ExpandedComponent::Dual); } }
    if unsafe { word(descriptor.0, 0xb0) } == 1 { unsafe { xm1_pipeline_image_expand_template_channel_pairs(descriptor, block); mark_expanded(descriptor, ExpandedComponent::ChannelPairs); } }
    if unsafe { word(descriptor.0, 0xc0) } == 1 { unsafe { xm1_pipeline_image_expand_template_channel_scaling(descriptor, block); mark_expanded(descriptor, ExpandedComponent::ChannelScaling); } }
    if unsafe { word(descriptor.0, 0xc4) } != 0 {
        unsafe {
            xm1_pipeline_image_expand_template_channel_matrix(descriptor, block);
            xm1_pipeline_image_expand_template_chroma_gain(descriptor, block);
            xm1_pipeline_image_expand_template_chroma_controls(descriptor, block);
            xm1_pipeline_image_expand_template_chroma_offsets(descriptor, block);
            xm1_pipeline_image_expand_template_chroma(descriptor, block);
            mark_expanded(descriptor, ExpandedComponent::Chroma);
        }
        if unsafe { word(descriptor.0, 0xc4) } == 2 { unsafe { xm1_pipeline_image_expand_template_detail(descriptor, block); mark_expanded(descriptor, ExpandedComponent::Chroma); } }
    }
    if unsafe { word(descriptor.0, 0xc8) } == 1 { unsafe { xm1_pipeline_image_expand_template_detail_limits(descriptor, block); mark_expanded(descriptor, ExpandedComponent::DetailLimits); } }
    if unsafe { word(descriptor.0, 0xcc) } != 0 { unsafe { xm1_pipeline_image_finish_template(descriptor, block); mark_expanded(descriptor, ExpandedComponent::DetailLimits); } }
    unsafe { store_word(output.0, 0, word(descriptor.0, 0xd8)); }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_sensor_submit_descriptor(output: ExpandedOutputAddress, descriptor: OutputDescriptorAddress) {
    unsafe { xm1_pipeline_image_reset_output_descriptor(descriptor, output); xm1_pipeline_image_expand_output_descriptor(descriptor, output); }
}
