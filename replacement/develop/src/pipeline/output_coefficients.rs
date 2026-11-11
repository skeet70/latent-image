use core::mem::MaybeUninit;
use core::ptr::write_volatile;
use super::shared::{Address32, ByteOffset};

#[derive(Clone, Copy)]
#[repr(transparent)]
pub(super) struct CoefficientDescriptor(pub(super) Address32);
#[derive(Clone, Copy)]
#[repr(transparent)]
pub(super) struct CoefficientBlock(pub(super) Address32);
#[derive(Clone, Copy)]
#[repr(transparent)]
pub(super) struct ConfigurationKey(pub(super) u32);
#[derive(Clone, Copy)]
#[repr(transparent)]
pub(super) struct LookupId(pub(super) u32);
#[derive(Clone, Copy)]
#[repr(transparent)]
pub(super) struct MatrixValue(pub(super) i64);
#[derive(Clone, Copy)]
#[repr(transparent)]
pub(super) struct MatrixIndex(pub(super) u32);
#[derive(Clone, Copy)]
#[repr(transparent)]
pub(super) struct LookupColumn(pub(super) u32);
#[derive(Clone, Copy)]
#[repr(transparent)]
pub(super) struct BlendWeight(pub(super) u32);
#[repr(u32)]
pub(super) enum ThresholdPriority {
    Default = 0, ReadoutSecond = 1, SecondaryFirst = 2, ReadoutFirst = 3,
    SecondarySecond = 4, PrimaryFirst = 5, PrimarySecond = 6,
}
struct CoefficientProfile { header: [u8; 9], curve: [u16; 28] }
#[derive(Clone, Copy)]
#[repr(u32)]
enum Field {
    Source = 0x28, Kind = 0xe4, Primary = 0xe8, Secondary = 0xec,
    ExposureBand = 0xf4, Detail = 0xf8, GridX = 0xfc, GridY = 0x100,
    MatrixKind = 0x104, MatrixBank = 0x108, MatrixAlternative = 0x110,
    MatrixGain = 0x118, PairFirst = 0x11c, PairSecond = 0x120, Special = 0x124,
    Exposure = 0x130, Weight = 0x13c, Profile = 0x140, Selection = 0x154,
    NoiseLevel = 0x158, PairIndex = 0x15c, Readout = 0x160, Alternate = 0x164,
    ThresholdFirst = 0x168, ThresholdSecond = 0x16a, ThresholdIndex = 0x16c,
    Priority = 0x170, SecondaryPriority = 0x174, SpecialOffset = 0x17c,
    NoiseOverride = 0x18c, GainKind = 0x194, Blend = 0x198, Selected = 0x199,
    SensorGain = 0x1a0, SensorTag = 0x1a2, SensorScale = 0x1a4,
    ExportScale = 0x1a6, MatrixScale = 0x1a8, MatrixEnabled = 0x1ac,
    MatrixCustom = 0x1ad, NoisePreset = 0x1bc, FirstLookup = 0x1c4,
    SecondLookup = 0x1c5, LookupWeight = 0x1c6, CaptureMode = 0x1d0,
    CaptureActive = 0x1d4, NoiseAlternative = 0x22c, ThresholdValue = 0x230,
}
impl CoefficientDescriptor {
    unsafe fn word(self, field: Field) -> u32 { unsafe { self.0.offset(ByteOffset(field as u32)).word() } }
    unsafe fn half(self, field: Field) -> u16 { unsafe { self.0.offset(ByteOffset(field as u32)).half() } }
    unsafe fn byte(self, field: Field) -> u8 { unsafe { self.0.offset(ByteOffset(field as u32)).byte() } }
    unsafe fn set_word(self, field: Field, value: u32) { unsafe { self.0.offset(ByteOffset(field as u32)).write_word(value) }; }
    unsafe fn set_byte(self, field: Field, value: u8) { unsafe { self.0.offset(ByteOffset(field as u32)).write_byte(value) }; }
}
impl CoefficientBlock {
    unsafe fn byte(self, offset: ByteOffset, value: u8) { unsafe { self.0.offset(offset).write_byte(value) }; }
    unsafe fn half(self, offset: ByteOffset, value: u16) { unsafe { self.0.offset(offset).write_half(value) }; }
}
unsafe extern "C" {
    fn xm1_buffers_configuration_byte(key: ConfigurationKey) -> u32;
    fn xm1_buffers_configuration_half(key: ConfigurationKey) -> u16;
    fn xm1_pipeline_image_configuration_signed_half(key: ConfigurationKey) -> i32;
    fn xm1_pipeline_image_configuration_value_within_limit(value: u32, limit: u32) -> u32;
    fn xm1_pipeline_image_configuration_mode_value(descriptor: CoefficientDescriptor, limit: u32) -> u32;
    fn xm1_pipeline_image_configuration_stage_mode(descriptor: CoefficientDescriptor, output: *mut u8);
    fn xm1_pipeline_image_configuration_kind_mode(descriptor: CoefficientDescriptor, output: *mut u8);
    fn xm1_pipeline_image_encode_configuration_mode(descriptor: CoefficientDescriptor, output: *mut u8);
    fn xm1_pipeline_image_configuration_threshold_band(index: u32, value: u32) -> u32;
    fn xm1_pipeline_sensor_selection_index(descriptor: CoefficientDescriptor, output: *mut u8);
    fn xm1_pipeline_sensor_configuration_key_bytes(destination: Address32, keys: Address32, count: i32);
    fn xm1_pipeline_sensor_configuration_key_halves(destination: Address32, keys: Address32, count: i32);
    fn __aeabi_uidiv(numerator: u32, denominator: u32) -> u32;
}
unsafe fn byte(key: u32) -> u32 { unsafe { xm1_buffers_configuration_byte(ConfigurationKey(key)) } }
unsafe fn half(key: u32) -> u16 { unsafe { xm1_buffers_configuration_half(ConfigurationKey(key)) } }
unsafe fn keyed_byte(table: u32, index: u32) -> u32 {
    let key = unsafe { Address32(table).offset(ByteOffset(index.wrapping_mul(2))).half() };
    unsafe { byte(key as u32) }
}
unsafe fn decoded(descriptor: CoefficientDescriptor, decoder: unsafe extern "C" fn(CoefficientDescriptor, *mut u8)) -> u8 {
    let mut result = MaybeUninit::<u8>::uninit();
    unsafe { decoder(descriptor, result.as_mut_ptr()); result.assume_init() }
}
unsafe fn key_bytes<const N: usize>(table: Address32) -> [u8; N] {
    let mut result = MaybeUninit::<[u8; N]>::uninit();
    unsafe { xm1_pipeline_sensor_configuration_key_bytes(Address32(result.as_mut_ptr() as u32), table, N as i32); result.assume_init() }
}
unsafe fn key_halves<const N: usize>(table: Address32) -> [u16; N] {
    let mut result = MaybeUninit::<[u16; N]>::uninit();
    unsafe { xm1_pipeline_sensor_configuration_key_halves(Address32(result.as_mut_ptr() as u32), table, N as i32); result.assume_init() }
}
unsafe fn store_bytes<const N: usize>(output: CoefficientBlock, offset: ByteOffset, values: &[u8; N]) {
    for (index, value) in values.iter().enumerate() { unsafe { output.byte(ByteOffset(offset.0.wrapping_add(index as u32)), *value) }; }
}
unsafe fn store_halves<const N: usize>(output: CoefficientBlock, offset: ByteOffset, values: &[u16; N]) {
    for (index, value) in values.iter().enumerate() { unsafe { output.half(ByteOffset(offset.0.wrapping_add(index as u32 * 2)), *value) }; }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_lookup_coefficient_profile(id: LookupId, header: Address32, curve: Address32) -> Address32 {
    let mut index = 0u32;
    let mut result = Address32(id.0);
    while index < unsafe { Address32(0x824bc4).word() } {
        let row = Address32(0x824bc8).offset(ByteOffset(index.wrapping_mul(72)));
        if unsafe { row.word() } == id.0 {
            if header.0 != 0 {
                for lane in 0..9 { result = header.offset(ByteOffset(lane)); unsafe { result.write_byte(row.offset(ByteOffset(4 + lane)).byte()) }; }
            }
            if curve.0 != 0 {
                for lane in 0..28 { result = curve.offset(ByteOffset(lane * 2)); unsafe { result.write_half(row.offset(ByteOffset(14 + lane * 2)).half()) }; }
            }
            break;
        }
        index = index.wrapping_add(1);
    }
    if index == unsafe { Address32(0x824bc4).word() } {
        if header.0 != 0 { for lane in 0..9 { unsafe { header.offset(ByteOffset(lane)).write_byte(Address32(0x824bcc + lane).byte()) }; } }
        if curve.0 != 0 {
            for lane in 0..28 { result = Address32((lane + 4) * 2); unsafe { curve.offset(ByteOffset(lane * 2)).write_half(Address32(0x824bd6 + lane * 2).half()) }; }
        }
    }
    result
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_lookup_export_curve(id: LookupId, output: Address32) {
    if output.0 == 0 { return; }
    let mut index = 0u32;
    while index < unsafe { Address32(0x825630).word() } {
        let row = Address32(0x825634).offset(ByteOffset(index.wrapping_mul(48)));
        if unsafe { row.word() } == id.0 {
            for lane in 0..22 { unsafe { output.offset(ByteOffset(lane * 2)).write_half(row.offset(ByteOffset(4 + lane * 2)).half()) }; }
            break;
        }
        index = index.wrapping_add(1);
    }
    if index == unsafe { Address32(0x825630).word() } {
        for lane in 0..22 { unsafe { output.offset(ByteOffset(lane * 2)).write_half(Address32(0x825638 + lane * 2).half()) }; }
    }
}
unsafe fn lookup(id: u32) -> CoefficientProfile {
    let mut header = MaybeUninit::<[u8; 9]>::uninit();
    let mut curve = MaybeUninit::<[u16; 28]>::uninit();
    unsafe {
        xm1_pipeline_image_lookup_coefficient_profile(LookupId(id), Address32(header.as_mut_ptr() as u32), Address32(curve.as_mut_ptr() as u32));
        CoefficientProfile { header: header.assume_init(), curve: curve.assume_init() }
    }
}
unsafe fn export_curve(id: u32, output: Address32) { unsafe { xm1_pipeline_image_lookup_export_curve(LookupId(id), output) }; }
unsafe fn lookup_curve(id: u32) -> [u16; 28] {
    let mut curve = MaybeUninit::<[u16; 28]>::uninit();
    unsafe {
        xm1_pipeline_image_lookup_coefficient_profile(LookupId(id), Address32(0), Address32(curve.as_mut_ptr() as u32));
        curve.assume_init()
    }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_read_matrix_coefficients(output: Address32, index: MatrixIndex) {
    let values: [MatrixValue; 9] = core::array::from_fn(|lane| {
        let key = unsafe { Address32(0xd67f88).offset(ByteOffset(index.0.wrapping_mul(18).wrapping_add(lane as u32 * 2))).half() };
        MatrixValue(unsafe { half(key as u32) } as i64)
    });
    for (lane, value) in values.iter().enumerate() {
        for (byte, value) in value.0.to_le_bytes().iter().enumerate() {
            unsafe { output.offset(ByteOffset(lane as u32 * 8 + byte as u32)).write_byte(*value) };
        }
    }
}
#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_build_matrix_coefficients(output: CoefficientBlock, descriptor: CoefficientDescriptor) {
    let kind = unsafe { descriptor.word(Field::MatrixKind) };
    let bank = unsafe { descriptor.word(Field::MatrixBank) };
    let key = if kind == 2 && unsafe { descriptor.word(Field::MatrixAlternative) } != 0 {
        unsafe { Address32(0xd680f8).offset(ByteOffset((descriptor.word(Field::MatrixAlternative).wrapping_add(bank.wrapping_mul(4))).wrapping_mul(2))).half() }
    } else {
        let table = if unsafe { descriptor.word(Field::Kind) } == 0x17 { 0xd67e64 } else { 0xd6891c };
        unsafe { Address32(table).offset(ByteOffset(kind.wrapping_add(bank.wrapping_mul(8)).wrapping_mul(2))).half() }
    };
    let selection = unsafe { byte(key as u32) };
    let enabled = unsafe { xm1_pipeline_image_configuration_value_within_limit(selection, 20) } != 0;
    let mut values = if enabled {
        let mut raw = MaybeUninit::<[MatrixValue; 9]>::uninit();
        unsafe { xm1_pipeline_image_read_matrix_coefficients(Address32(raw.as_mut_ptr() as u32), MatrixIndex(selection.wrapping_sub(1) as u8 as u32)); raw.assume_init() }
    } else { [MatrixValue(1024),MatrixValue(0),MatrixValue(0),MatrixValue(0),MatrixValue(1024),MatrixValue(0),MatrixValue(0),MatrixValue(0),MatrixValue(1024)] };
    if enabled {
        if unsafe { descriptor.word(Field::Kind) } == 0x18 { values[0] = MatrixValue(1024); values[1] = MatrixValue(0); values[2] = MatrixValue(0); }
        for value in &mut values { if value.0 & !0xfff != 0 { value.0 |= !0xfff; } }
        if unsafe { descriptor.word(Field::Profile) } == 7 && unsafe { descriptor.byte(Field::MatrixEnabled) } != 0 {
            let percent = match unsafe { descriptor.word(Field::MatrixScale) } { 0 => 800,1 => 632,2 => 503,3 => 400,4 => 316,5 => 251,6 => 200,7 => 158,8 => 126,10..=12 => (unsafe { descriptor.byte(Field::MatrixCustom) }) as i64,_ => 100 };
            for value in &mut values { value.0 = value.0 * percent / 100; }
        }
    }
    unsafe { output.byte(ByteOffset(0), u8::from(enabled)) };
    for (lane, value) in values.iter().enumerate() { unsafe { output.half(ByteOffset(2 + lane as u32 * 2), value.0 as u16) }; }
}
#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_store_matrix_coefficients(input: CoefficientBlock, output: CoefficientBlock) {
    unsafe { output.byte(ByteOffset(0x2c), input.0.byte()) };
    for group in 0..3 {
        let second = unsafe { input.0.offset(ByteOffset(4 + group * 6)).half() };
        let first = unsafe { input.0.offset(ByteOffset(2 + group * 6)).half() };
        unsafe { output.half(ByteOffset(0x2e + group * 6), first); output.half(ByteOffset(0x30 + group * 6), second); }
        let third = unsafe { input.0.offset(ByteOffset(6 + group * 6)).half() };
        unsafe { output.half(ByteOffset(0x32 + group * 6), third) };
    }
}
#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_expand_matrix_coefficients(descriptor: CoefficientDescriptor, output: CoefficientBlock) {
    let mut packet = MaybeUninit::<[u16; 10]>::uninit();
    let packet = CoefficientBlock(Address32(packet.as_mut_ptr() as u32));
    unsafe { xm1_pipeline_image_build_matrix_coefficients(packet, descriptor); xm1_pipeline_image_store_matrix_coefficients(packet, output); }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_select_parameter_coefficient(row: LookupId, column: LookupColumn, output: *mut u8) {
    let value = if unsafe { xm1_pipeline_image_configuration_value_within_limit(row.0, 60) } != 0 { unsafe { keyed_byte(0xd685f4, row.0.wrapping_sub(1).wrapping_mul(6).wrapping_add(column.0)) } } else { 0 };
    unsafe { write_volatile(output, value as u8) };
}
#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_select_export_coefficient(row: LookupId, column: LookupColumn, output: *mut u8) {
    let value = if unsafe { xm1_pipeline_image_configuration_value_within_limit(row.0, 80) } != 0 { unsafe { keyed_byte(0xd6a920, row.0.wrapping_sub(1).wrapping_mul(6).wrapping_add(column.0)) } } else { 0 };
    unsafe { write_volatile(output, value as u8) };
}
unsafe fn parameter(row: u32, column: u8) -> u8 {
    let mut output = MaybeUninit::uninit();
    unsafe { xm1_pipeline_image_select_parameter_coefficient(LookupId(row), LookupColumn(column as u32), output.as_mut_ptr()); output.assume_init() }
}
unsafe fn export_id(row: u32, column: u8) -> u8 {
    let mut output = MaybeUninit::uninit();
    unsafe { xm1_pipeline_image_select_export_coefficient(LookupId(row), LookupColumn(column as u32), output.as_mut_ptr()); output.assume_init() }
}
#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_build_blended_lookup(descriptor: CoefficientDescriptor, header: Address32, curve: Address32, extra: Address32) {
    let override_key = if unsafe { byte(0xb5) } == 1 { Some(0xcb75) } else if unsafe { byte(0xcb74) } == 1 { Some(0xcb76) } else { None };
    if let Some(key) = override_key {
        unsafe { xm1_pipeline_image_lookup_coefficient_profile(LookupId(byte(key)), header, curve); if extra.0 != 0 { extra.write_half(0); } }
        return;
    }
    let stage = unsafe { decoded(descriptor, xm1_pipeline_image_configuration_stage_mode) };
    let kind = if stage == 2 && unsafe { descriptor.word(Field::Special) } == 7 { 30 } else { unsafe { decoded(descriptor, xm1_pipeline_image_configuration_kind_mode) } };
    let (first, second) = if kind == 0 {
        let index = unsafe { descriptor.word(Field::MatrixKind) }.wrapping_mul(5).wrapping_add(stage as u32 * 40).wrapping_add(unsafe { descriptor.word(Field::Detail) });
        (unsafe { keyed_byte(0xd684a0, index) }, unsafe { keyed_byte(0xd683b0, index) })
    } else { (unsafe { keyed_byte(0xd68296, stage as u32 * 43 + kind as u32) },unsafe { keyed_byte(0xd67e82, stage as u32 * 43 + kind as u32) }) };
    let column = unsafe { decoded(descriptor, xm1_pipeline_sensor_selection_index) };
    let first = unsafe { parameter(first, column) };
    let second = unsafe { parameter(second, column) };
    let weight = unsafe { keyed_byte(0xd685e0, descriptor.word(Field::Weight)) };
    let CoefficientProfile { header: head, curve: first_curve } = unsafe { lookup(first as u32) };
    let second_curve = unsafe { lookup_curve(second as u32) };
    for (lane, value) in head.iter().enumerate() { unsafe { header.offset(ByteOffset(lane as u32)).write_byte(*value) }; }
    for lane in 0..28 {
        let value = weight.wrapping_mul(first_curve[lane] as u32).wrapping_add(128u32.wrapping_sub(weight).wrapping_mul(second_curve[lane] as u32)).wrapping_add(64) >> 7;
        unsafe { curve.offset(ByteOffset(lane as u32 * 2)).write_half(value.min(0x3fff) as u16) };
    }
    if extra.0 != 0 {
        let value = if stage == 0 { (unsafe { descriptor.half(Field::MatrixGain) }) as u32 * unsafe { keyed_byte(0xd68108, descriptor.word(Field::MatrixKind).wrapping_mul(5).wrapping_add(descriptor.word(Field::Selection))) } >> 6 } else { 0 };
        unsafe { extra.write_half(value as u16) };
    }
    unsafe { descriptor.set_byte(Field::FirstLookup, first); descriptor.set_byte(Field::SecondLookup, second); descriptor.set_byte(Field::LookupWeight, weight as u8); }
}
#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_expand_blended_lookup(descriptor: CoefficientDescriptor, output: CoefficientBlock) {
    let mut header = MaybeUninit::<[u8; 9]>::uninit(); let mut curve = MaybeUninit::<[u16; 28]>::uninit(); let mut extra = MaybeUninit::<u16>::uninit();
    unsafe { xm1_pipeline_image_build_blended_lookup(descriptor, Address32(header.as_mut_ptr() as u32), Address32(curve.as_mut_ptr() as u32), Address32(extra.as_mut_ptr() as u32));
        store_bytes(output, ByteOffset(0x40), &header.assume_init()); store_halves(output, ByteOffset(0x4a), &curve.assume_init()); output.half(ByteOffset(0x82), 0xffff); output.half(ByteOffset(0x84), extra.assume_init()); }
}
#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_expand_paired_lookup(descriptor: CoefficientDescriptor, output: CoefficientBlock) {
    let readout = unsafe { descriptor.word(Field::Readout) };
    let (first_key, second_key) = if unsafe { descriptor.word(Field::CaptureMode) } == 1 && unsafe { descriptor.word(Field::CaptureActive) } != 0 { (0xc64d,0xc653) }
    else if readout == 3 && unsafe { descriptor.word(Field::PairFirst) == descriptor.word(Field::PairSecond) } { (0xc64c,0xc652) }
    else { (unsafe { Address32(0xd680f0).offset(ByteOffset(readout.wrapping_mul(2))).half() } as u32, unsafe { Address32(0xd68914).offset(ByteOffset(readout.wrapping_mul(2))).half() } as u32) };
    let column = unsafe { decoded(descriptor, xm1_pipeline_sensor_selection_index) };
    let first = unsafe { byte(first_key) }; let second = unsafe { byte(second_key) };
    let first = unsafe { parameter(first,column) }; let second = unsafe { parameter(second,column) };
    let a = unsafe { lookup_curve(first as u32) }; let b = unsafe { lookup_curve(second as u32) };
    unsafe { store_halves(output, ByteOffset(0x86), &a); store_halves(output, ByteOffset(0xbe), &b); }
}
#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_expand_sensor_layout(descriptor: CoefficientDescriptor, output: CoefficientBlock) {
    let bytes: [u8; 16] = unsafe { key_bytes(Address32(0xd68158).offset(ByteOffset(descriptor.word(Field::ExposureBand).wrapping_mul(32)))) };
    let halves: [u16; 3] = unsafe { key_halves(Address32(0xd685a4).offset(ByteOffset(descriptor.word(Field::ExposureBand).wrapping_mul(6)))) };
    for lane in 0..13 { unsafe { output.byte(ByteOffset(0x12 + lane), bytes[lane as usize]) }; }
    unsafe { store_halves(output, ByteOffset(0x20), &halves); output.byte(ByteOffset(0x26),bytes[15]); output.byte(ByteOffset(0x27),bytes[14]); output.byte(ByteOffset(0x28),bytes[13]); }
}
#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_expand_signed_channels(descriptor: CoefficientDescriptor, output: CoefficientBlock) {
    let common: [u8; 10] = unsafe { key_bytes(Address32(0xd68590)) };
    let selected: [u8; 4] = unsafe { key_bytes(Address32(0xd688c4).offset(ByteOffset(descriptor.word(Field::ExposureBand).wrapping_mul(8)))) };
    unsafe { store_bytes(output, ByteOffset(0), &common); output.half(ByteOffset(10), selected[0] as i8 as i16 as u16); output.half(ByteOffset(12), selected[1] as i8 as i16 as u16); output.half(ByteOffset(14),selected[2] as u16); output.byte(ByteOffset(16),selected[3]); }
}
#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_expand_auxiliary_channels(descriptor: CoefficientDescriptor, output: CoefficientBlock) {
    let values: [u8; 6] = unsafe { key_bytes(Address32(0xd689a4).offset(ByteOffset(descriptor.word(Field::ExposureBand).wrapping_mul(12)))) };
    unsafe { output.byte(ByteOffset(0), values[0]); output.byte(ByteOffset(1), values[1]); output.byte(ByteOffset(2), values[2]); output.half(ByteOffset(4), values[3] as u16); output.half(ByteOffset(6), values[4] as u16); output.byte(ByteOffset(8),values[5]); }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_expand_noise_controls(descriptor: CoefficientDescriptor, output: CoefficientBlock) {
    match unsafe { byte(0xa94e) } { 1 => unsafe { descriptor.set_word(Field::NoisePreset,0) },2 => unsafe { descriptor.set_word(Field::NoisePreset,1) },_ => {} }
    let level_key = unsafe { Address32(0xd694b8).offset(ByteOffset(descriptor.word(Field::NoiseLevel).wrapping_mul(2))).half() };
    let level = unsafe { half(level_key as u32) } as u32;
    let (gain, ratio) = if unsafe { descriptor.word(Field::NoiseAlternative) } == 1 {
        let tag = unsafe { descriptor.byte(Field::SensorTag) };
        let mut product = 0u32;
        for index in 0..7 { let row = Address32(0xd69640 + index * 2); if unsafe { row.byte() } == tag { product = unsafe { row.offset(ByteOffset(1)).byte() } as u32 * unsafe { descriptor.half(Field::SensorGain) } as u32; } }
        let gain = (product as u64 * 1953 / 10000) as u32;
        let divisor = (level * 1953 / 10000).max(1);
        (gain, unsafe { __aeabi_uidiv(gain.wrapping_mul(100),divisor) })
    } else {
        let gain = unsafe { descriptor.half(Field::SensorScale) } as u32;
        let rounded = (level + 5) / 10 * 10;
        let divisor = if rounded <= 4089 { (unsafe { Address32(0xd6916c + rounded / 10 * 2).half() }) as u32 } else { 0x12cc6 };
        (gain, unsafe { __aeabi_uidiv(gain * 100,divisor) })
    };
    let preset = unsafe { descriptor.word(Field::NoisePreset) };
    let controls: [u16; 3] = unsafe { key_halves(Address32(0xd696e8).offset(ByteOffset(preset.wrapping_mul(6)))) };
    let second = (gain as u64 * controls[1] as u64 / 100).min(65535) as u16;
    let third = (gain as u64 * controls[2] as u64 / 100).min(65535) as u16;
    let mut band = 0u32;
    if ratio >= 50 { band = 1; while band < 8 { if ratio < unsafe { Address32(0xd69768 + band).byte() } as u32 { break; } band += 1; } }
    let first = unsafe { keyed_byte(0xd69770, descriptor.word(Field::ExposureBand)) };
    let second_gain = unsafe { keyed_byte(0xd696d4,band) };
    let strength = ((first * second_gain) >> 6) as u16;
    let values: [u16; 8] = unsafe { key_halves(Address32(0xd694cc).offset(ByteOffset(descriptor.word(Field::NoiseLevel).wrapping_add(descriptor.word(Field::NoisePreset).wrapping_mul(10)).wrapping_mul(16)))) };
    let flag = unsafe { byte(0xa94d) };
    let limit = unsafe { half(0xa947) };
    unsafe { output.byte(ByteOffset(4),flag as u8); output.byte(ByteOffset(5),strength.min(255) as u8); output.half(ByteOffset(6),limit); output.half(ByteOffset(8),controls[0].min(8191)); output.half(ByteOffset(10),second); output.half(ByteOffset(12),third); }
    let first_limit = unsafe { half(0xa94b) }; let second_limit = unsafe { half(0xa949) };
    unsafe { output.half(ByteOffset(22),first_limit); output.half(ByteOffset(24),second_limit); }
    for pair in 0..4 { let raw=values[pair*2+1]; let signed=if raw & 0xe000 != 0 {raw|0xe000} else {raw}; unsafe { output.half(ByteOffset(26+pair as u32*4),values[pair*2]); output.half(ByteOffset(28+pair as u32*4),signed); } }
    unsafe { output.half(ByteOffset(42),256) };
}
#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_expand_noise_coefficients(descriptor: CoefficientDescriptor, output: CoefficientBlock) {
    unsafe { xm1_pipeline_image_expand_noise_controls(descriptor,output) };
    let (first, middle, third) = if unsafe { descriptor.word(Field::NoiseOverride) } == 1 { (256,256,256) } else {
        let mut base = unsafe { keyed_byte(0xd6911c, descriptor.word(Field::ExposureBand).wrapping_add(descriptor.word(Field::Readout).wrapping_mul(10))) };
        if unsafe { descriptor.word(Field::ExposureBand) } != 9 {
            let next=unsafe { keyed_byte(0xd6911c, descriptor.word(Field::ExposureBand).wrapping_add(descriptor.word(Field::Readout).wrapping_mul(10)).wrapping_add(1)) };
            let amount=unsafe { byte(0xa99c) };
            if base < next { let remainder=(unsafe { descriptor.word(Field::Exposure) } as i32 % 100) as u32; base=base.wrapping_add(next.wrapping_sub(base).wrapping_mul(remainder).wrapping_mul(amount)/6400) as u16 as u32; }
        }
        if unsafe { descriptor.word(Field::Kind) } == 0x35 && unsafe { descriptor.word(Field::SpecialOffset) } == 0 { base=base.wrapping_sub(unsafe { byte(0xa99b) }) as u16 as u32; }
        let mode=unsafe { decoded(descriptor,xm1_pipeline_image_encode_configuration_mode) };
        let exposure=unsafe { xm1_pipeline_image_configuration_mode_value(descriptor,11) };
        let gain=unsafe { byte(0xa99d) };
        let band=unsafe { keyed_byte(0xd694a0,exposure) };
        let weight=unsafe { keyed_byte(0xd69620,descriptor.word(Field::Weight)) };
        let mode=unsafe { keyed_byte(0xd69660,mode as u32) };
        let selection=unsafe { keyed_byte(0xd69634,descriptor.word(Field::Selection)) };
        let kind=unsafe { descriptor.word(Field::GainKind) };
        let kind_gain=if kind<5 && (1u32<<kind)&0x1b !=0 { unsafe { keyed_byte(0xd696f4,descriptor.word(Field::ExposureBand).wrapping_add(kind.wrapping_mul(10))) } } else {64};
        let matrix=unsafe { descriptor.word(Field::MatrixKind) };
        let matrix_gain=if matrix.wrapping_sub(2)<6 { unsafe { keyed_byte(0xd69758,matrix) } } else {64};
        let alternate=if unsafe { descriptor.word(Field::Alternate) } ==1 { unsafe { byte(0xa99e) } } else {64};
        let first_product=base as u64*gain as u8 as u64*selection as u8 as u64*band as u8 as u64*kind_gain as u8 as u64*matrix_gain as u8 as u64;
        let middle=((first_product>>24) as u32 as u64*weight as u8 as u64*mode as u8 as u64*alternate as u8 as u64>>24) as u32;
        let mut first=unsafe { keyed_byte(0xd69650,descriptor.word(Field::Readout).wrapping_mul(2)) };
        let mut third=unsafe { keyed_byte(0xd69650,descriptor.word(Field::Readout).wrapping_mul(2).wrapping_add(1)) };
        if unsafe { descriptor.word(Field::Kind) }==0x35 && unsafe { descriptor.word(Field::SpecialOffset) }==0 { first=first.wrapping_add(unsafe { byte(0xa999) }) as u16 as u32; third=third.wrapping_add(unsafe { byte(0xa99a) }) as u16 as u32; }
        (first.wrapping_add(middle),middle,third.wrapping_add(middle))
    };
    unsafe { output.half(ByteOffset(14),first.min(511) as u16); output.half(ByteOffset(16),middle.min(511) as u16); }
    let flag=unsafe { keyed_byte(0xd696c0,descriptor.word(Field::ExposureBand)) };
    unsafe { output.half(ByteOffset(18),third.min(511) as u16); output.byte(ByteOffset(20),flag as u8); }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_threshold_priority(descriptor: CoefficientDescriptor) -> ThresholdPriority {
    match unsafe { descriptor.word(Field::Priority) } { 1=>return ThresholdPriority::PrimaryFirst,2=>return ThresholdPriority::PrimarySecond,_=>{} }
    match unsafe { descriptor.word(Field::SecondaryPriority) } { 1=>return ThresholdPriority::SecondaryFirst,2=>return ThresholdPriority::SecondarySecond,_=>{} }
    match unsafe { descriptor.word(Field::Readout) } { 1=>ThresholdPriority::ReadoutFirst,2=>ThresholdPriority::ReadoutSecond,_=>ThresholdPriority::Default }
}
#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_expand_threshold_curves(descriptor: CoefficientDescriptor, output: CoefficientBlock) {
    let band=unsafe { xm1_pipeline_image_configuration_threshold_band(descriptor.byte(Field::ThresholdIndex) as u32,descriptor.word(Field::ThresholdValue)) };
    let base=band.wrapping_mul(15).wrapping_add(unsafe { descriptor.byte(Field::ThresholdIndex) } as u32).wrapping_mul(112).wrapping_add(0x408a00);
    let mut first=MaybeUninit::<[u16;28]>::uninit(); let mut second=MaybeUninit::<[u16;28]>::uninit();
    for lane in 0..28 {
        let a=unsafe { xm1_pipeline_image_configuration_signed_half(ConfigurationKey(base.wrapping_add(lane as u32*2))) } as u16;
        let b=unsafe { xm1_pipeline_image_configuration_signed_half(ConfigurationKey(base.wrapping_add(56+lane as u32*2))) } as u16;
        unsafe { first.as_mut_ptr().cast::<u16>().add(lane).write(a); second.as_mut_ptr().cast::<u16>().add(lane).write(b); }
    }
    let priority=unsafe { xm1_pipeline_image_threshold_priority(descriptor) } as u32;
    let table=if unsafe { descriptor.word(Field::Alternate) }==1 {0xd69b68} else {0xd69bbc};
    let row=Address32(table).offset(ByteOffset(priority*12));
    let first_gain=unsafe { byte(row.word()) }; let second_gain=unsafe { byte(row.offset(ByteOffset(4)).word()) }; let third_gain=unsafe { half(row.offset(ByteOffset(8)).word()) };
    let first_flag=unsafe { byte(0xc0a4) }; let second_flag=unsafe { byte(0xc0a4) };
    unsafe { store_halves(output,ByteOffset(14),&first.assume_init()); store_halves(output,ByteOffset(70),&second.assume_init()); output.half(ByteOffset(12),first_gain as u16); output.half(ByteOffset(10),second_gain as u16); output.half(ByteOffset(8),third_gain); output.byte(ByteOffset(1),first_flag as u8); output.byte(ByteOffset(2),second_flag as u8); output.half(ByteOffset(6),descriptor.half(Field::ThresholdFirst)); output.half(ByteOffset(4),descriptor.half(Field::ThresholdSecond)); output.half(ByteOffset(126),if descriptor.word(Field::Source)==0 {29} else {17}); }
}
#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_expand_selected_lookup(descriptor: CoefficientDescriptor, output: CoefficientBlock) {
    let id=if unsafe { descriptor.word(Field::Readout)==3 && descriptor.word(Field::PairFirst)==descriptor.word(Field::PairSecond) } { unsafe { byte(0xc63f) } }
    else { unsafe { keyed_byte(0xd6a83c,descriptor.word(Field::PairIndex).wrapping_mul(5).wrapping_add(descriptor.word(Field::Selection))) } };
    let CoefficientProfile { header, curve }=unsafe { lookup(id) };
    unsafe { store_bytes(output,ByteOffset(0x34),&header); store_halves(output,ByteOffset(0x3e),&curve); }
}
#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_expand_lookup_controls(descriptor: CoefficientDescriptor, output: CoefficientBlock) {
    let stage=unsafe { decoded(descriptor,xm1_pipeline_image_configuration_stage_mode) };
    let kind=if stage==2 && unsafe { descriptor.word(Field::Special) }==7 {30} else {unsafe { decoded(descriptor,xm1_pipeline_image_configuration_kind_mode) }};
    let key=if kind==0 {unsafe { Address32(0xd6a5c4).offset(ByteOffset(descriptor.word(Field::MatrixKind).wrapping_add(stage as u32*8).wrapping_mul(2))).half() }} else {unsafe { Address32(0xd6a5f2).offset(ByteOffset((stage as u32*43+kind as u32)*2)).half() }};
    let selection=unsafe { byte(key as u32) };
    let mode=if unsafe { xm1_pipeline_image_configuration_value_within_limit(selection,16) }!=0 { let column=unsafe { decoded(descriptor,xm1_pipeline_sensor_selection_index) }; (unsafe { keyed_byte(0xd6a850,selection.wrapping_sub(1).wrapping_mul(6).wrapping_add(column as u32)) }) as u8 } else {0};
    let third=unsafe { byte(0xc65b) };
    unsafe { output.byte(ByteOffset(2),third as u8); }
    let fourth=unsafe { byte(0xc658) }; let fifth=unsafe { byte(0xc659) }; let sixth=unsafe { byte(0xc65a) };
    unsafe { output.byte(ByteOffset(3),fourth as u8); output.byte(ByteOffset(4),fifth as u8); output.byte(ByteOffset(5),sixth as u8); output.byte(ByteOffset(6),mode); }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_blend_export_curves(output: Address32, first: LookupId, second: LookupId, weight: BlendWeight) {
    unsafe { export_curve(first.0,Address32(0x1467ab4)); export_curve(second.0,Address32(0x1467a88)); }
    for lane in 0..22 { let a=unsafe { Address32(0x1467ab4+lane*2).half() } as u32; let b=unsafe { Address32(0x1467a88+lane*2).half() } as u32; let result=a.wrapping_add(b.wrapping_sub(a).wrapping_mul(weight.0).wrapping_add(64)>>7); unsafe { output.offset(ByteOffset(lane*2)).write_half(result as u16) }; }
}
#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_build_selected_export(descriptor: CoefficientDescriptor, output: Address32) {
    let stage=unsafe { decoded(descriptor,xm1_pipeline_image_configuration_stage_mode) };
    let kind=unsafe { decoded(descriptor,xm1_pipeline_image_configuration_kind_mode) };
    let column=unsafe { decoded(descriptor,xm1_pipeline_sensor_selection_index) };
    unsafe { descriptor.set_byte(Field::Selected,kind) };
    let first=unsafe { keyed_byte(0xd6a736,stage as u32*43+kind as u32) }; let second=unsafe { keyed_byte(0xd6a4be,stage as u32*43+kind as u32) };
    let first=unsafe { export_id(first,column) }; let second=unsafe { export_id(second,column) };
    if stage==1 || (stage!=0 && unsafe { descriptor.word(Field::Profile) }==7) {unsafe { export_curve(first as u32,output) };}
    else {unsafe { xm1_pipeline_image_blend_export_curves(output,LookupId(first as u32),LookupId(second as u32),BlendWeight(descriptor.byte(Field::Blend) as u32)) };}
}
#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_build_grid_export(descriptor: CoefficientDescriptor, output: Address32) {
    let stage=unsafe { decoded(descriptor,xm1_pipeline_image_configuration_stage_mode) };
    let first=if stage==2 && unsafe { descriptor.word(Field::Special) }==7 { unsafe { keyed_byte(0xd6a6f8,descriptor.word(Field::MatrixKind)) } }
    else {unsafe { keyed_byte(0xd6a708,descriptor.word(Field::MatrixKind).wrapping_add(stage as u32*8)) }};
    let id=unsafe { keyed_byte(0xd6acec,descriptor.word(Field::GridX).wrapping_mul(5).wrapping_add(first.wrapping_sub(1).wrapping_mul(25)).wrapping_add(descriptor.word(Field::GridY))) };
    let column=unsafe { decoded(descriptor,xm1_pipeline_sensor_selection_index) };
    unsafe { export_curve(export_id(id,column) as u32,output) };
}
#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_build_export_coefficients(descriptor: CoefficientDescriptor, output: Address32) {
    if unsafe { byte(0xb5) }==1 {unsafe { export_curve(byte(0xc656),output); descriptor.set_byte(Field::Selected,255); }}
    else if unsafe { byte(0xcb74) }==1 {unsafe { export_curve(byte(0xc657),output); descriptor.set_byte(Field::Selected,255); }}
    else {
        let stage=unsafe { decoded(descriptor,xm1_pipeline_image_configuration_stage_mode) };
        let kind=unsafe { descriptor.word(Field::Kind) };
        let use_grid_branch=if stage>1 {true} else { matches!(kind,1..=4|0x10|0x13|0x36|0x37) || ((kind==0 || kind==0x35)&&unsafe { descriptor.word(Field::Secondary) }==0) };
        let direct=use_grid_branch && unsafe { descriptor.word(if stage>1 {Field::Secondary} else {Field::Primary}) }==0;
        if direct {unsafe { xm1_pipeline_image_build_grid_export(descriptor,output) };} else {unsafe { xm1_pipeline_image_build_selected_export(descriptor,output) };}
    }
    if unsafe { descriptor.half(Field::ExportScale) }<100 {unsafe { descriptor.0.offset(ByteOffset(Field::ExportScale as u32)).write_half(100) };}
    for lane in 0..22 { let value=unsafe { output.offset(ByteOffset(lane*2)).half() } as u32; let scale=unsafe { descriptor.half(Field::ExportScale) } as u32; let value=value.wrapping_mul(scale).wrapping_add(50)/100; unsafe { output.offset(ByteOffset(lane*2)).write_half(value.min(8192) as u16) }; }
}
#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_expand_export_coefficients(descriptor: CoefficientDescriptor, output: CoefficientBlock) {
    let mut values=MaybeUninit::<[u16;22]>::uninit();
    unsafe { xm1_pipeline_image_build_export_coefficients(descriptor,Address32(values.as_mut_ptr() as u32)); store_halves(output,ByteOffset(8),&values.assume_init()); }
}
