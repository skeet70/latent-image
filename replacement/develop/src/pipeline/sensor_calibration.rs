use super::shared::{Address32, ByteOffset};
use core::ptr::{read_volatile, write_volatile};

#[repr(transparent)]
#[derive(Clone, Copy)]
struct ConfigurationKey(u32);
#[repr(transparent)]
#[derive(Clone, Copy)]
struct SensorMode(u32);
#[repr(transparent)]
#[derive(Clone, Copy)]
struct SensorChannel(u32);
#[repr(transparent)]
#[derive(Clone, Copy)]
struct SensorTemperature(i32);
#[repr(transparent)]
#[derive(Clone, Copy)]
struct CorrectionStep(i8);
#[repr(transparent)]
#[derive(Clone, Copy)]
struct CalibrationFlags(u8);
#[repr(transparent)]
#[derive(Clone, Copy)]
struct BankAddress(u32);
#[derive(Clone, Copy)]
struct CalibrationInput { mode: SensorMode, channel: SensorChannel, temperature: SensorTemperature }
#[derive(Clone, Copy)]
struct CalibrationProfile(u32);
impl CalibrationProfile {
    fn parse(value: u32) -> Option<Self> { if value <= 12 && value != 4 { Some(Self(value)) } else { None } }
    fn channels(self) -> ChannelLayout { match self.0 { 0 | 1 => ChannelLayout::Permuted, 2 | 3 => ChannelLayout::Full, _ => ChannelLayout::Neutral } }
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum CacheState { Unavailable, Available }
impl CacheState { fn from_flag(value: u8) -> Self { if value == 1 { Self::Available } else { Self::Unavailable } } }
#[derive(Clone, Copy, PartialEq, Eq)]
enum ExposureMode { Configured, Zero, Specified }
#[derive(Clone, Copy, PartialEq, Eq)]
enum ChannelLayout { Neutral, Permuted, Alternate, Full }
impl ChannelLayout {
    fn from_code(value: u32) -> Self { match value { 1 => Self::Permuted, 2 => Self::Alternate, 3 => Self::Full, _ => Self::Neutral } }
    fn code(self) -> u32 { match self { Self::Neutral => 0, Self::Permuted => 1, Self::Alternate => 2, Self::Full => 3 } }
    fn counts(self) -> (u32, u32) { if self == Self::Full { (18, 12) } else { (9, 6) } }
}
#[derive(Clone, Copy)]
struct Selection { mode: u8, channel: u8, temperature: u8 }
#[repr(C)]
pub struct CalibrationRecord {
    primary_enabled: u8,
    secondary_enabled: u8,
    primary_offset: u16,
    primary_width: u16,
    secondary_offset: u16,
    secondary_width: u16,
    horizontal_mode: u8,
    vertical_mode: u8,
    primary_height: u16,
    secondary_height: u16,
    secondary_stride: u16,
    border_mode: u8,
    reserved: u8,
    primary_curve: [u16; 28],
    full_banks: [BankAddress; 14],
    reserved_words: [u32; 10],
    compact_banks: [BankAddress; 6],
}
const _: [(); 196] = [(); core::mem::size_of::<CalibrationRecord>()];
impl CalibrationRecord {
    const fn empty() -> Self {
        Self { primary_enabled: 0, secondary_enabled: 0, primary_offset: 0, primary_width: 0, secondary_offset: 0, secondary_width: 0, horizontal_mode: 0, vertical_mode: 0, primary_height: 0, secondary_height: 0, secondary_stride: 0, border_mode: 0, reserved: 0, primary_curve: [0; 28], full_banks: [BankAddress(0); 14], reserved_words: [0; 10], compact_banks: [BankAddress(0); 6] }
    }
}
#[derive(Clone, Copy)]
struct SensorFrame(Address32);
impl SensorFrame {
    fn field(self, offset: u32) -> Address32 { self.0.offset(ByteOffset(offset)) }
    unsafe fn word(self, offset: u32) -> u32 { unsafe { self.field(offset).word() } }
    unsafe fn byte(self, offset: u32) -> u8 { unsafe { self.field(offset).byte() } }
    unsafe fn input(self) -> CalibrationInput {
        CalibrationInput { mode: SensorMode(unsafe { self.byte(0x6c0) }.wrapping_sub(1) as u32), channel: SensorChannel(unsafe { self.byte(0x660) } as u32), temperature: SensorTemperature(unsafe { self.field(0x2264).half() } as i16 as i32) }
    }
}
#[derive(Clone, Copy)]
struct CalibrationView(Address32);
impl CalibrationView {
    fn field(self, offset: u32) -> Address32 { self.0.offset(ByteOffset(offset)) }
    unsafe fn byte(self, offset: u32) -> u8 { unsafe { self.field(offset).byte() } }
    unsafe fn put_byte(self, offset: u32, value: u8) { unsafe { self.field(offset).write_byte(value) }; }
    unsafe fn put_half(self, offset: u32, value: u16) { unsafe { self.field(offset).write_half(value) }; }
}
#[repr(C)]
struct ChannelCorrections { primary: [i8; 18], secondary: [i8; 12] }
const GRID: Address32 = Address32(0x014d49f0);
const FIRST_EXPOSURE: Address32 = Address32(0x014cf070);
const SECOND_EXPOSURE: Address32 = Address32(0x014ceff0);
const CACHE: Address32 = Address32(0x01568318);

unsafe fn byte(key: ConfigurationKey) -> u8 { unsafe { configuration_byte(key.0) as u8 } }
unsafe fn half(key: ConfigurationKey) -> u16 { unsafe { configuration_half(key.0) } }
unsafe fn signed_byte(key: ConfigurationKey) -> i32 { unsafe { configuration_signed_byte(key.0) } }
unsafe fn signed_half(key: ConfigurationKey) -> i32 { unsafe { configuration_signed_half(key.0) } }
fn lsl(value: u32, count: u32) -> u32 { let count = count & 255; if count >= 32 { 0 } else { value << count } }
fn asr(value: i32, count: u32) -> i32 { let count = count & 255; if count >= 32 { value >> 31 } else { value >> count } }
fn bounded_coefficient(value: i32) -> u16 { value.clamp(0, 1023) as u16 }
unsafe fn clear(address: Address32, bytes: u32) {
    let mut offset = 0;
    while offset < bytes && address.0.wrapping_add(offset) & 3 != 0 {
        unsafe { address.offset(ByteOffset(offset)).write_byte(0) };
        offset += 1;
    }
    while bytes - offset >= 4 {
        unsafe { address.offset(ByteOffset(offset)).write_word(0) };
        offset += 4;
    }
    while offset < bytes {
        unsafe { address.offset(ByteOffset(offset)).write_byte(0) };
        offset += 1;
    }
}
unsafe fn copy_bytes(destination: Address32, source: Address32, bytes: u32) {
    let mut offset = 0;
    if (destination.0 ^ source.0) & 3 == 0 {
        while offset < bytes && destination.0.wrapping_add(offset) & 3 != 0 {
            let value = unsafe { source.offset(ByteOffset(offset)).byte() };
            unsafe { destination.offset(ByteOffset(offset)).write_byte(value) };
            offset += 1;
        }
        while bytes - offset >= 4 {
            let value = unsafe { source.offset(ByteOffset(offset)).word() };
            unsafe { destination.offset(ByteOffset(offset)).write_word(value) };
            offset += 4;
        }
    }
    while offset < bytes {
        let value = unsafe { source.offset(ByteOffset(offset)).byte() };
        unsafe { destination.offset(ByteOffset(offset)).write_byte(value) };
        offset += 1;
    }
}
unsafe fn rom_word(base: *const u8, offset: u32) -> u32 { unsafe { Address32((base as u32).wrapping_add(offset)).word() } }
unsafe fn rom_byte(base: *const u8, offset: u32) -> u8 { unsafe { Address32((base as u32).wrapping_add(offset)).byte() } }
unsafe fn optional_half(key: ConfigurationKey) -> u16 { if key.0 == u32::MAX { 0 } else { unsafe { half(key) } } }
unsafe fn threshold_bin(input: u32, key: ConfigurationKey, count: u32) -> u8 {
    for index in 0..count { if input <= unsafe { byte(ConfigurationKey(key.0.wrapping_add(index))) } as u32 { return index as u8; } }
    count as u8
}
unsafe fn temperature_bin(mode: u8, input: SensorTemperature) -> u8 {
    let key = ConfigurationKey(0x3e3120 + mode as u32 * 4);
    if input.0 <= unsafe { half(key) } as i32 { 0 } else if input.0 <= unsafe { half(ConfigurationKey(key.0 + 2)) } as i32 { 1 } else { 2 }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_sensor_select_calibration(mode: u32, channel: u32, temperature: i32, mode_out: *mut u8, channel_out: *mut u8, temperature_out: *mut u8) {
    unsafe { write_volatile(mode_out, threshold_bin(mode, ConfigurationKey(0x3e3100), 14)) };
    unsafe { write_volatile(channel_out, threshold_bin(channel, ConfigurationKey(0x3e310e), 5)) };
    let selected = unsafe { read_volatile(mode_out) };
    unsafe { write_volatile(temperature_out, temperature_bin(selected, SensorTemperature(temperature))) };
}
unsafe fn select(input: CalibrationInput) -> Selection {
    let mut selected = Selection { mode: 0, channel: 0, temperature: 0 };
    unsafe { xm1_pipeline_sensor_select_calibration(input.mode.0, input.channel.0, input.temperature.0, &mut selected.mode, &mut selected.channel, &mut selected.temperature) };
    selected
}
#[no_mangle]
pub extern "C" fn xm1_pipeline_sensor_correction_key(step: i32) -> u32 {
    match step { -5 => 0x4653, -4 => 0x461b, -3 => 0x45e3, -2 => 0x45ab, -1 => 0x4573, 1 => 0x453b, 2 => 0x4503, 3 => 0x44cb, 4 => 0x4493, 5 => 0x445b, _ => 0 }
}
unsafe fn midpoint_bin(input: i32, base: ConfigurationKey) -> u8 {
    for index in 0..14 {
        let first = unsafe { byte(ConfigurationKey(base.0 + index)) } as i32;
        let second = unsafe { byte(ConfigurationKey(base.0 + index + 1)) } as i32;
        if input.wrapping_mul(10) < ((first + second) * 5) as i16 as i32 { return index as u8; }
    }
    14
}
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_sensor_select_interpolated_calibration(mode: i32, channel: i32, temperature: i32, mode_out: *mut u8, channel_out: *mut u8, temperature_out: *mut u8) {
    unsafe { write_volatile(mode_out, midpoint_bin(mode, ConfigurationKey(0x3e4300))) };
    unsafe { write_volatile(channel_out, midpoint_bin(channel, ConfigurationKey(0x3e430f))) };
    let selected = unsafe { read_volatile(mode_out) };
    let base = 0x3e431e + selected as u32 * 6;
    let mut previous = 0i16;
    for index in 0..3 {
        let current = unsafe { signed_half(ConfigurationKey(base + index * 2)) };
        if temperature < current {
            let value = if index == 0 { 0 } else { temperature.wrapping_sub(previous as i32).wrapping_mul(10) / (current as i16 as i32 - previous as i32) + (index as i32 - 1) * 10 };
            unsafe { write_volatile(temperature_out, value as u8) };
            return;
        }
        previous = current as i16;
    }
    unsafe { write_volatile(temperature_out, 20) };
}
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_sensor_exposure_is_zero(exposure: *const i8) -> i32 {
    if exposure.is_null() { return 1; }
    for index in 0..4 { if unsafe { read_volatile(exposure.add(index)) } != 0 { return 0; } }
    1
}
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_sensor_sample_calibration(alternate: u32, mode: u32, channel: u32, temperature: i32, component: u8) -> i32 {
    let (base, minimum, range) = if alternate == 0 { (0x3e5770u32, -250i32, 550i32) } else { (0x3e6c90u32, -400i32, 400i32) };
    let key = base.wrapping_add((temperature as u32).wrapping_mul(0x708)).wrapping_add(mode.wrapping_mul(8)).wrapping_add(channel.wrapping_mul(0x78)).wrapping_add((component as u32).wrapping_sub(1));
    (minimum + range * unsafe { byte(ConfigurationKey(key)) } as i32 / 255) as i16 as i32
}
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_sensor_interpolate_calibration(alternate: u32, mode: u32, channel: u32, temperature: u32, component: u8) -> i32 {
    let lower = if temperature < 10 { 0 } else { 1 };
    let base = unsafe { xm1_pipeline_sensor_sample_calibration(alternate, mode, channel, lower, component) };
    let upper = unsafe { xm1_pipeline_sensor_sample_calibration(alternate, mode, channel, lower + 1, component) };
    let repeated = unsafe { xm1_pipeline_sensor_sample_calibration(alternate, mode, channel, lower, component) };
    let position = temperature.wrapping_sub(lower as u32 * 10);
    base.wrapping_add(upper.wrapping_sub(repeated).wrapping_mul(position as i32) / 10) as i16 as i32
}
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_sensor_calibration_axes(mode: u32, channel: u32, temperature: u32, mean_out: *mut i16, difference_out: *mut i16) {
    if unsafe { byte(ConfigurationKey(0x3e0009)) } != 0 {
        unsafe { clear(Address32(mean_out as u32), 16); clear(Address32(difference_out as u32), 16) };
        return;
    }
    for index in 0..8 {
        let first = unsafe { xm1_pipeline_sensor_interpolate_calibration(0, mode, channel, temperature, index as u8 + 1) } as i16;
        let second = unsafe { xm1_pipeline_sensor_interpolate_calibration(1, mode, channel, temperature, index as u8 + 1) } as i16;
        let sum = first.wrapping_add(second) as i32;
        unsafe { write_volatile(mean_out.add(index), ((sum + if sum < 0 { -10 } else { 10 }) / 20) as i16) };
        let difference = first.wrapping_sub(second) as i32;
        unsafe { write_volatile(difference_out.add(index), ((difference + if difference < 0 { -5 } else { 5 }) / 10) as i16) };
    }
}
unsafe fn signed_midpoint_bin(value: i16, key: ConfigurationKey, count: u32) -> u8 {
    for index in 0..count {
        let first = unsafe { signed_byte(ConfigurationKey(key.0 + index)) };
        let second = unsafe { signed_byte(ConfigurationKey(key.0 + index + 1)) };
        if (value as i32) < (first + second) / 2 { return index as u8; }
    }
    count as u8
}
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_sensor_calibration_regions(means: *const i16, differences: *const i16, regions: *mut u8, alternate: u32) {
    if unsafe { byte(ConfigurationKey(0x3e0009)) } != 0 { unsafe { clear(Address32(regions as u32), 8) }; return; }
    let (table, mean_key, difference_key) = if alternate == 0 { (0x1c800, 0x3246, 0x3264) } else { (0x1dac0, 0x3278, 0x3296) };
    for index in 0..8 {
        let mean = unsafe { signed_midpoint_bin(read_volatile(means.add(index)), ConfigurationKey(mean_key), 29) };
        let difference = unsafe { signed_midpoint_bin(read_volatile(differences.add(index)), ConfigurationKey(difference_key), 19) };
        let value = unsafe { byte(ConfigurationKey(table + (index as u32 * 30 + mean as u32) * 20 + difference as u32)) };
        unsafe { write_volatile(regions.add(index), value) };
    }
}
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_sensor_read_axis_corrections(base: u32, cell: u32, region: u32, output: *mut i8) {
    let key = base.wrapping_add(cell).wrapping_add(region.wrapping_mul(128));
    unsafe { write_volatile(output, signed_byte(ConfigurationKey(key)) as i8); write_volatile(output.add(1), 0); write_volatile(output.add(2), signed_byte(ConfigurationKey(key.wrapping_add(64))) as i8) };
}
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_sensor_read_channel_corrections(base: u32, cell: u32, region: u32, output: *mut i8) {
    let key = base.wrapping_add(cell).wrapping_add(region.wrapping_mul(0x780));
    for index in 0..30 { unsafe { write_volatile(output.add(index), signed_byte(ConfigurationKey(key.wrapping_add(index as u32 * 64))) as i8) }; }
}
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_sensor_extrapolate_cell(source: u32, adjacent: u32, destination: u32, primary_count: u32, secondary_count: u8, table: u32) {
    if primary_count != 0 {
        let count = primary_count.wrapping_sub(1) as u8 as u32 + 1;
        for index in 0..count {
            let first = unsafe { Address32(table.wrapping_add(source.wrapping_mul(60)).wrapping_add(index * 2)).half() };
            let second = unsafe { Address32(table.wrapping_add(adjacent.wrapping_mul(60)).wrapping_add(index * 2)).half() };
            unsafe { Address32(table.wrapping_add(destination.wrapping_mul(60)).wrapping_add(index * 2)).write_half(first.wrapping_mul(2).wrapping_sub(second)) };
        }
    }
    for index in 0..secondary_count as u32 {
        let first = unsafe { Address32(table.wrapping_add(source.wrapping_mul(60)).wrapping_add(36 + index * 2)).half() };
        let second = unsafe { Address32(table.wrapping_add(adjacent.wrapping_mul(60)).wrapping_add(36 + index * 2)).half() };
        unsafe { Address32(table.wrapping_add(destination.wrapping_mul(60)).wrapping_add(36 + index * 2)).write_half(first.wrapping_mul(2).wrapping_sub(second)) };
    }
}
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_sensor_copy_calibration_cell(source: u32, destination: u32, primary_bytes: u32, secondary_bytes: u32, table: u32) {
    let source = Address32(table.wrapping_add(source.wrapping_mul(60)));
    let destination = Address32(table.wrapping_add(destination.wrapping_mul(60)));
    unsafe { copy_bytes(destination, source, primary_bytes); copy_bytes(destination.offset(ByteOffset(36)), source.offset(ByteOffset(36)), secondary_bytes) };
}
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_sensor_extend_calibration_border(mode: u32, layout: u32, table: u32) {
    if mode > 1 { return; }
    let (primary, secondary) = ChannelLayout::from_code(layout).counts();
    let transfer = |source: u32, adjacent: u32, destination: u32| unsafe {
        if mode == 1 { xm1_pipeline_sensor_copy_calibration_cell(source, destination, primary * 2, secondary * 2, table); }
        else { xm1_pipeline_sensor_extrapolate_cell(source, adjacent, destination, primary, secondary as u8, table); }
    };
    transfer(0x44, 0x55, 0x33);
    for cell in 0x44..0x4c { transfer(cell, cell + 16, cell - 16); }
    transfer(0x4b, 0x5a, 0x3c);
    for row in 4..12 { let cell = row * 16 + 4; transfer(cell, cell + 1, cell - 1); }
    for row in 4..12 { let cell = row * 16 + 11; transfer(cell, cell - 1, cell + 1); }
    transfer(0xb4, 0xa5, 0xc3);
    for cell in 0xb4..0xbc { transfer(cell, cell - 16, cell + 16); }
    transfer(0xbb, 0xaa, 0xcc);
}
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_sensor_calibration_cell(cell: u32) -> u8 { unsafe { rom_byte(&raw const sensor_calibration_cell_map, cell) } }
unsafe fn grid_put(cell: u32, component: u32, value: u16) {
    let mapped = unsafe { xm1_pipeline_sensor_calibration_cell(cell) } as u32;
    unsafe { GRID.offset(ByteOffset(mapped * 60 + component * 2)).write_half(value) };
}
unsafe fn component(layout: ChannelLayout, index: u32, secondary: bool) -> u32 {
    match (layout, secondary) {
        (ChannelLayout::Permuted, false) => (unsafe { byte(ConfigurationKey(0x13433 + index)) }) as u32,
        (ChannelLayout::Alternate, false) => (unsafe { byte(ConfigurationKey(0x13442 + index)) }) as u32,
        (ChannelLayout::Permuted, true) => (unsafe { byte(ConfigurationKey(0x1343c + index)) }) as u32,
        (ChannelLayout::Alternate, true) => (unsafe { byte(ConfigurationKey(0x1344b + index)) }) as u32,
        _ => index,
    }
}
unsafe fn accumulate_exposures(first: *const i8, second: *const i8) {
    unsafe { clear(FIRST_EXPOSURE, 128); clear(SECOND_EXPOSURE, 128) };
    for exposure in 0..4 {
        let step = if exposure & 1 == 0 { 1 } else { 255 };
        for strip in 0..8 {
            let mut cell = unsafe { rom_byte(&raw const sensor_calibration_exposure_starts, exposure as u32 * 8 + strip) } as u32;
            for position in strip * 8..strip * 8 + 8 {
                let offset = ByteOffset(cell * 2);
                let current = unsafe { FIRST_EXPOSURE.offset(offset).half() };
                let weight = unsafe { signed_byte(ConfigurationKey(0x3340 + position)) };
                let value = current.wrapping_add(weight.wrapping_mul(unsafe { read_volatile(first.add(exposure)) } as i32) as u16);
                unsafe { FIRST_EXPOSURE.offset(offset).write_half(value) };
                let current = unsafe { SECOND_EXPOSURE.offset(offset).half() };
                let weight = unsafe { signed_byte(ConfigurationKey(0x3380 + position)) };
                let value = current.wrapping_add(weight.wrapping_mul(unsafe { read_volatile(second.add(exposure)) } as i32) as u16);
                unsafe { SECOND_EXPOSURE.offset(offset).write_half(value) };
                cell = (cell + step) & 63;
            }
        }
    }
}
unsafe fn specified_grid(layout: ChannelLayout, first: *const i8, second: *const i8) {
    let exponent = unsafe { byte(ConfigurationKey(0x3004)) } as u32;
    let unit = lsl(1, exponent) as i32;
    unsafe { accumulate_exposures(first, second) };
    let (primary, secondary) = layout.counts();
    for cell in 0..64 {
        let axes = [((unsafe { FIRST_EXPOSURE.offset(ByteOffset(cell * 2)).half() } as i16 as i32) / 9) as i8, 0, ((unsafe { SECOND_EXPOSURE.offset(ByteOffset(cell * 2)).half() } as i16 as i32) / 9) as i8];
        for index in 0..primary {
            let value = if layout == ChannelLayout::Neutral { 256 } else {
                let selected = unsafe { component(layout, index, false) };
                let axis = unsafe { rom_byte(&raw const sensor_calibration_primary_components, selected) } as usize;
                bounded_coefficient(asr(unit.wrapping_add(axes[axis] as i32).wrapping_shl(8).wrapping_add(unit >> 1), exponent))
            };
            unsafe { grid_put(cell, index, value) };
        }
        for index in 0..secondary {
            let value = if layout != ChannelLayout::Neutral { 256 } else {
                let axis = unsafe { rom_byte(&raw const sensor_calibration_secondary_components, index) } as usize;
                bounded_coefficient(asr(unit.wrapping_add(axes[axis] as i32).wrapping_shl(8).wrapping_add(unit >> 1), exponent))
            };
            unsafe { grid_put(cell, 18 + index, value) };
        }
    }
}
unsafe fn configured_grid(layout: ChannelLayout, first_regions: &[u8; 8], second_regions: &[u8; 8]) {
    let first_exponent = unsafe { byte(ConfigurationKey(0x3004)) } as u32;
    let second_exponent = unsafe { byte(ConfigurationKey(0x3005)) } as u32;
    let first_unit = lsl(1, first_exponent) as i32;
    let second_unit = lsl(1, second_exponent) as i32;
    let (primary, secondary) = layout.counts();
    for cell in 0..64 {
        let region = unsafe { byte(ConfigurationKey(0x31d80 + cell)) }.min(8).wrapping_sub(1) as usize;
        let mut axes = core::mem::MaybeUninit::<[i8; 3]>::uninit();
        unsafe { xm1_pipeline_sensor_read_axis_corrections(0x1ed80, cell, first_regions[region] as u32, axes.as_mut_ptr().cast::<i8>()) };
        let axes = unsafe { axes.assume_init() };
        let corrections = if layout == ChannelLayout::Neutral { None } else {
            let mut corrections = core::mem::MaybeUninit::<ChannelCorrections>::uninit();
            unsafe { xm1_pipeline_sensor_read_channel_corrections(0x22d80, cell, second_regions[region] as u32, corrections.as_mut_ptr().cast::<i8>()) };
            Some(unsafe { corrections.assume_init() })
        };
        for index in 0..primary {
            let value = if let Some(corrections) = &corrections {
                let selected = unsafe { component(layout, index, false) } as usize;
                let axis = unsafe { rom_byte(&raw const sensor_calibration_primary_components, selected as u32) } as usize;
                let numerator = second_unit.wrapping_add(corrections.primary[selected] as i32).wrapping_mul(first_unit.wrapping_add(axes[axis] as i32).wrapping_shl(8)).wrapping_add(first_unit.wrapping_add(second_unit) >> 1);
                bounded_coefficient(asr(numerator, first_exponent + second_exponent))
            } else { 256 };
            unsafe { grid_put(cell, index, value) };
        }
        for index in 0..secondary {
            let value = if let Some(corrections) = &corrections {
                let selected = unsafe { component(layout, index, true) } as usize;
                bounded_coefficient(asr(second_unit.wrapping_add(corrections.secondary[selected] as i32).wrapping_shl(8).wrapping_add(second_unit >> 1), second_exponent))
            } else {
                let axis = unsafe { rom_byte(&raw const sensor_calibration_secondary_components, index) } as usize;
                bounded_coefficient(asr(first_unit.wrapping_add(axes[axis] as i32).wrapping_shl(8).wrapping_add(first_unit >> 1), first_exponent))
            };
            unsafe { grid_put(cell, 18 + index, value) };
        }
    }
}
unsafe fn pack_triplet(destination: Address32, source: Address32) {
    let first = unsafe { source.half() } as u32 & 1023;
    let second = unsafe { source.offset(ByteOffset(2)).half() } as u32 & 1023;
    let third = unsafe { source.offset(ByteOffset(4)).half() } as u32 & 1023;
    let previous = unsafe { destination.word() };
    unsafe { destination.write_word((previous & 0xc0000000) | first | (second << 10) | (third << 20)) };
}
unsafe fn pack_banks(layout: ChannelLayout, output: CalibrationView) {
    let full = layout == ChannelLayout::Full;
    let base = if full { Address32(0x014d11f0) } else { Address32(0x014cf9f0) };
    let bank_count = if full { 14 } else { 5 };
    for row in 3..13 {
        for column in 3..13 {
            let cell = row * 16 + column;
            for bank in 0..bank_count {
                let component = if full { if bank < 10 { bank * 3 } else { 18 + (bank - 10) * 3 } } else if bank < 3 { bank * 3 } else { 18 + (bank - 3) * 3 };
                unsafe { pack_triplet(base.offset(ByteOffset(bank * 1024 + cell * 4)), GRID.offset(ByteOffset(cell * 60 + component * 2))) };
            }
        }
    }
    let (pointer_offset, count) = if full { (0x4c, 14) } else { (0xac, 6) };
    for bank in 0..count { unsafe { output.field(pointer_offset + bank * 4).write_word(base.0 + bank * 1024) }; }
}
unsafe fn generate(profile: Option<CalibrationProfile>, input: CalibrationInput, cache: CacheState, first: *const i8, second: *const i8, correction: CorrectionStep, requested: CalibrationFlags, output: CalibrationView) {
    let flags = CalibrationFlags(unsafe { byte(ConfigurationKey(0xc0)) } & requested.0);
    let configured = unsafe { byte(ConfigurationKey(0x3e0009)) };
    let mut primary_selection = Selection { mode: 0, channel: 0, temperature: 0 };
    let mut secondary_selection = Selection { mode: 0, channel: 0, temperature: 0 };
    let mut cached_primary = false;
    unsafe { output.put_byte(0, 0) };
    if flags.0 & 16 != 0 {
        if configured == 1 {
            if correction.0 != 0 && cache == CacheState::Available { unsafe { output.put_byte(0, 1) }; cached_primary = true; }
        } else {
            primary_selection = unsafe { select(input) };
            let mask = unsafe { half(ConfigurationKey(0x3e315c + primary_selection.channel as u32 * 2)) };
            if (mask >> primary_selection.mode) & 1 != 0 { unsafe { output.put_byte(0, 1) }; }
        }
    }
    unsafe { output.put_byte(1, 0) };
    let mut exposure_mode = ExposureMode::Configured;
    if flags.0 & 2 != 0 {
        if configured == 1 {
            if cache == CacheState::Available {
                unsafe { output.put_byte(1, 1) };
                exposure_mode = if unsafe { xm1_pipeline_sensor_exposure_is_zero(first) } == 0 || unsafe { xm1_pipeline_sensor_exposure_is_zero(second) } == 0 { ExposureMode::Specified } else { ExposureMode::Zero };
            }
        } else {
            unsafe { xm1_pipeline_sensor_select_interpolated_calibration(input.mode.0 as i32, input.channel.0 as i32, input.temperature.0, &mut secondary_selection.mode, &mut secondary_selection.channel, &mut secondary_selection.temperature); output.put_byte(1, 1) };
        }
    }
    if unsafe { output.field(0).half() } == 0 { return; }
    let Some(profile) = profile else { unsafe { output.put_byte(0, 0); output.put_byte(1, 0) }; return; };
    let primary_base = profile.0 * 20;
    let secondary_base = profile.0 * 24;
    let mut first_regions = [0u8; 8];
    let mut second_regions = [0u8; 8];
    if unsafe { output.byte(1) } == 1 {
        for (key_offset, destination) in [(0, 6), (4, 8)] { unsafe { output.put_half(destination, optional_half(ConfigurationKey(rom_word(&raw const sensor_calibration_secondary_keys, secondary_base + key_offset)))) }; }
        for (key_offset, destination) in [(16, 14), (20, 16)] { unsafe { output.put_half(destination, half(ConfigurationKey(rom_word(&raw const sensor_calibration_secondary_keys, secondary_base + key_offset)))) }; }
        if exposure_mode == ExposureMode::Configured {
            let mut means = [0i16; 8];
            let mut differences = [0i16; 8];
            unsafe { xm1_pipeline_sensor_calibration_axes(secondary_selection.mode as u32, secondary_selection.channel as u32, secondary_selection.temperature as u32, means.as_mut_ptr(), differences.as_mut_ptr()); xm1_pipeline_sensor_calibration_regions(means.as_ptr(), differences.as_ptr(), first_regions.as_mut_ptr(), 0); xm1_pipeline_sensor_calibration_regions(means.as_ptr(), differences.as_ptr(), second_regions.as_mut_ptr(), 1) };
        }
        let key = unsafe { rom_word(&raw const sensor_calibration_border_keys, profile.0 * 4) };
        unsafe { output.put_byte(18, byte(ConfigurationKey(key))) };
    }
    if unsafe { output.byte(0) } == 1 {
        for (key_offset, destination) in [(0, 2), (4, 4)] { unsafe { output.put_half(destination, optional_half(ConfigurationKey(rom_word(&raw const sensor_calibration_primary_keys, primary_base + key_offset)))) }; }
        let key = unsafe { rom_word(&raw const sensor_calibration_primary_keys, primary_base + 16) };
        unsafe { output.put_half(12, half(ConfigurationKey(key))) };
    }
    let (table, offset) = if unsafe { output.byte(1) } == 1 { (&raw const sensor_calibration_secondary_keys, secondary_base) } else { (&raw const sensor_calibration_primary_keys, primary_base) };
    for (key_offset, destination) in [(8, 10), (12, 11)] { let key = unsafe { rom_word(table, offset + key_offset) }; unsafe { output.put_byte(destination, byte(ConfigurationKey(key))) }; }
    if unsafe { output.byte(0) } == 1 {
        let base = if cached_primary { xm1_pipeline_sensor_correction_key(correction.0 as i32) } else {
            let bit = 1u32 << profile.0;
            let bank = if bit & 0x1fe3 != 0 { u32::from(unsafe { byte(ConfigurationKey(0x3e3168)) }) } else { primary_selection.temperature as u32 };
            0x404e00 + bank * 0x13b0 + primary_selection.channel as u32 * 0x348 + primary_selection.mode as u32 * 56
        };
        for index in 0..28 { let value = unsafe { half(ConfigurationKey(base + index * 2)) }; unsafe { output.put_half(20 + index * 2, value) }; }
    }
    if unsafe { output.byte(1) } == 1 {
        let layout = profile.channels();
        if exposure_mode == ExposureMode::Specified { unsafe { specified_grid(layout, first, second) }; }
        else { unsafe { configured_grid(layout, &first_regions, &second_regions) }; }
        unsafe { xm1_pipeline_sensor_extend_calibration_border(output.byte(18) as u32, layout.code(), GRID.0); pack_banks(layout, output) };
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_sensor_generate_calibration(profile: u32, mode: u32, channel: u32, temperature: i32, cached: u8, first: *const i8, second: *const i8, correction: i8, flags: u8, output: *mut CalibrationRecord) {
    unsafe { generate(CalibrationProfile::parse(profile), CalibrationInput { mode: SensorMode(mode), channel: SensorChannel(channel), temperature: SensorTemperature(temperature) }, CacheState::from_flag(cached), first, second, CorrectionStep(correction), CalibrationFlags(flags), CalibrationView(Address32(output as u32))) };
}
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_sensor_cached_primary_geometry(stage: u32, output: u32) -> u8 {
    let frame = SensorFrame(Address32(stage));
    unsafe { Address32(output).offset(ByteOffset(132)).write_word(0) };
    for (destination, source, wide) in [(0, 2, true), (4, 4, true), (8, 6, false), (12, 7, false), (16, 8, true)] {
        let source = Address32(unsafe { frame.word(0x1884) }).offset(ByteOffset(source));
        let value = if wide { u32::from(unsafe { source.half() }) } else { u32::from(unsafe { source.byte() }) };
        unsafe { Address32(output).offset(ByteOffset(destination)).write_word(value) };
    }
    for index in 0..28 {
        let value = unsafe { Address32(frame.word(0x1884)).offset(ByteOffset(10 + index * 2)).half() } as u32;
        unsafe { Address32(output).offset(ByteOffset(20 + index * 4)).write_word(value) };
    }
    unsafe { Address32(frame.word(0x1884)).byte() }
}
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_sensor_primary_geometry(stage: u32, output: u32, _operation: u32, _selection: u32, _secondary: i32) -> u32 {
    let frame = SensorFrame(Address32(stage));
    if unsafe { frame.word(0x78) } == 11 { return u32::from(unsafe { xm1_pipeline_sensor_cached_primary_geometry(stage, output) } != 0); }
    if unsafe { frame.word(0x2728) } != 0 { unsafe { clear(Address32(output), 136) }; return 0; }
    let cache = if unsafe { frame.word(0x3240) } == 1 { CacheState::Available } else { CacheState::Unavailable };
    let correction = if cache == CacheState::Available { (unsafe { frame.byte(0x3250) }) as i8 } else { 0 };
    let mut record = CalibrationRecord::empty();
    unsafe { generate(CalibrationProfile::parse(2), frame.input(), cache, core::ptr::null(), core::ptr::null(), CorrectionStep(correction), CalibrationFlags(16), CalibrationView(Address32(&mut record as *mut CalibrationRecord as u32))) };
    if record.primary_enabled != 1 { unsafe { clear(Address32(output), 136) }; return 0; }
    let fields = [record.primary_offset as u32, record.primary_width as u32, record.horizontal_mode as u32, record.vertical_mode as u32, record.primary_height as u32];
    for (index, value) in fields.into_iter().enumerate() { unsafe { Address32(output).offset(ByteOffset(index as u32 * 4)).write_word(value) }; }
    for (index, value) in record.primary_curve.iter().copied().enumerate() { unsafe { Address32(output).offset(ByteOffset(20 + index as u32 * 4)).write_word(value as u32) }; }
    unsafe { Address32(output).write_word(frame.word(0x498).wrapping_add(record.primary_offset as u32)) };
    1
}
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_sensor_calibration_unchanged(enabled: u32, kind: u32, mode: u32, channel: u32) -> u32 {
    if enabled == 0 { unsafe { CACHE.write_word(1) }; return 0; }
    let matches = unsafe { CACHE.word() } == 0 && unsafe { CACHE.offset(ByteOffset(8)).word() } == mode && unsafe { CACHE.offset(ByteOffset(12)).word() } == channel && unsafe { CACHE.offset(ByteOffset(4)).word() } == kind;
    unsafe { CACHE.write_word(0); CACHE.offset(ByteOffset(8)).write_word(mode); CACHE.offset(ByteOffset(12)).write_word(channel); CACHE.offset(ByteOffset(4)).write_word(kind) };
    u32::from(matches)
}
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_sensor_secondary_geometry(stage: u32, output: u32, _operation: u32, _selection: u32, _secondary: i32) -> u32 {
    let frame = SensorFrame(Address32(stage));
    if unsafe { frame.word(0x2728) } != 0 { unsafe { clear(Address32(output), 0x5424); xm1_pipeline_sensor_calibration_unchanged(0, 0, 0, 0) }; return 0; }
    let cache = if unsafe { frame.word(0x3240) } == 1 { CacheState::Available } else { CacheState::Unavailable };
    let mut first = [0i8; 4];
    let mut second = [0i8; 4];
    if cache == CacheState::Available {
        for (index, offset) in [0, 2, 6, 4].into_iter().enumerate() { first[index] = unsafe { frame.byte(0x3248 + offset) } as i8; second[index] = unsafe { frame.byte(0x3249 + offset) } as i8; }
    }
    let mut record = CalibrationRecord::empty();
    let (first, second) = if cache == CacheState::Available { (first.as_ptr(), second.as_ptr()) } else { (core::ptr::null(), core::ptr::null()) };
    unsafe { generate(CalibrationProfile::parse(2), frame.input(), cache, first, second, CorrectionStep(0), CalibrationFlags(2), CalibrationView(Address32(&mut record as *mut CalibrationRecord as u32))) };
    let result = if record.secondary_enabled == 1 {
        let fields = [1, record.border_mode as u32, record.secondary_offset as u32, record.secondary_width as u32, record.secondary_height as u32, record.secondary_stride as u32, record.horizontal_mode as u32, record.vertical_mode as u32];
        for (index, value) in fields.into_iter().enumerate() { unsafe { Address32(output).offset(ByteOffset(index as u32 * 4)).write_word(value) }; }
        for bank in 0..6 {
            for cell in 0..256 {
                let value = unsafe { Address32(record.full_banks[bank].0).offset(ByteOffset(cell * 4)).word() };
                for component in 0..3 { unsafe { Address32(output).offset(ByteOffset(32 + bank as u32 * 1536 + component * 512 + cell * 2)).write_half(((value >> (component * 10)) & 1023) as u16) }; }
            }
        }
        for bank in 0..8 {
            for cell in 0..256 {
                let value = unsafe { Address32(record.full_banks[bank + 6].0).offset(ByteOffset(cell * 4)).word() };
                for (shift, offset) in [(0, 0x4420), (10, 0x3420), (20, 0x2420)] { unsafe { Address32(output).offset(ByteOffset(offset + bank as u32 * 512 + cell * 2)).write_half(((value >> shift) & 1023) as u16) }; }
            }
        }
        unsafe { Address32(output).offset(ByteOffset(8)).write_word(frame.word(0x498).wrapping_add(record.secondary_offset as u32)) };
        1
    } else { unsafe { clear(Address32(output), 0x5424) }; 0 };
    let unchanged = unsafe { xm1_pipeline_sensor_calibration_unchanged(1, 2, (frame.byte(0x6c0) as u32).wrapping_sub(1), frame.word(0x660)) };
    unsafe { Address32(output).offset(ByteOffset(0x5420)).write_word(unchanged) };
    result
}

unsafe extern "C" {
    #[link_name = "xm1_buffers_configuration_byte"]
    fn configuration_byte(key: u32) -> u32;
    #[link_name = "xm1_buffers_configuration_half"]
    fn configuration_half(key: u32) -> u16;
    #[link_name = "xm1_pipeline_image_configuration_signed_byte"]
    fn configuration_signed_byte(key: u32) -> i32;
    #[link_name = "xm1_pipeline_image_configuration_signed_half"]
    fn configuration_signed_half(key: u32) -> i32;
    static sensor_calibration_primary_keys: u8;
    static sensor_calibration_secondary_keys: u8;
    static sensor_calibration_border_keys: u8;
    static sensor_calibration_exposure_starts: u8;
    static sensor_calibration_primary_components: u8;
    static sensor_calibration_secondary_components: u8;
    static sensor_calibration_cell_map: u8;
}
