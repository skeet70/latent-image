use super::shared::{Address32, ByteOffset};
use core::mem::MaybeUninit;
use core::ptr::{addr_of, addr_of_mut, read_volatile, write_volatile};
use crate::develop::calibration_core as calibration;

#[derive(Clone, Copy)]
#[repr(transparent)]
pub(super) struct ConfigurationKey(pub(super) u32);
#[derive(Clone, Copy)]
#[repr(transparent)]
pub(super) struct ProfileGain(pub(super) u16);
#[derive(Clone, Copy)]
#[repr(transparent)]
pub(super) struct DeltaIndex(pub(super) i32);
#[derive(Clone, Copy)]
#[repr(transparent)]
pub(super) struct SplitIndex(pub(super) i32);
#[derive(Clone, Copy)]
#[repr(transparent)]
pub(super) struct ProfileSelector(pub(super) i32);
#[derive(Clone, Copy)]
#[repr(transparent)]
pub(super) struct FactorMode(pub(super) i32);
#[derive(Clone, Copy)]
#[repr(transparent)]
pub(super) struct ProfileClass(pub(super) u32);
#[derive(Clone, Copy)]
#[repr(transparent)]
pub(super) struct ProfileRate(pub(super) u32);
#[derive(Clone, Copy)]
#[repr(C)]
pub(super) struct CalibrationTriplet { pub(super) values: [ProfileGain; 3] }
#[repr(C)]
pub(super) struct FactorRequest {
    pub(super) first_delta: DeltaIndex,
    pub(super) second_delta: DeltaIndex,
    pub(super) split: SplitIndex,
    pub(super) selector: ProfileSelector,
    pub(super) values: CalibrationTriplet,
    pub(super) reserved: u16,
    pub(super) mode: FactorMode,
}
#[repr(C)]
pub(super) struct FactorSet { pub(super) groups: [CalibrationTriplet; 4] }
#[derive(Clone, Copy)]
#[repr(C)]
pub(super) struct ClampFlags { pub(super) upper: u32, pub(super) lower: u32 }

unsafe extern "C" {
    fn xm1_buffers_configuration_byte(key: ConfigurationKey) -> u32;
    fn xm1_buffers_configuration_half(key: ConfigurationKey) -> u16;
    fn __aeabi_idiv(numerator: i32, denominator: i32) -> i32;
    fn xm1_profile_load_optional_rate_byte(pointer: *const u8) -> u32;
}

unsafe fn setting_half(key: u32) -> u16 {
    unsafe { xm1_buffers_configuration_half(ConfigurationKey(key)) }
}

pub(super) fn calibration_value<T>(result: Result<T, calibration::configuration::ConfigurationError<core::convert::Infallible>>) -> T {
    match result { Ok(value) => value, Err(_) => halt() }
}

pub(super) unsafe fn physical_calibration() -> calibration::Calibration<'static> {
    let configuration = calibration_value(unsafe { super::buffers::configuration_view() });
    calibration::Calibration::new(configuration)
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_configuration_base() -> Address32 {
    Address32(unsafe { Address32(0x96ca50).word() })
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_configuration_pointer(key: ConfigurationKey) -> Address32 {
    unsafe { xm1_pipeline_image_configuration_base().offset(ByteOffset(key.0)) }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_profile_split_count() -> u32 {
    calibration_value(unsafe { physical_calibration() }.split_count()).get()
}

fn delta_key(index: DeltaIndex, first: bool) -> Option<ConfigurationKey> {
    let key = match index.0 {
        0..=8 => (if first { 0x7ed7 } else { 0x7efb }) - index.0 as u32 * 2,
        10..=18 => (if first { 0x7eb5 } else { 0x7ed9 }) + (index.0 as u32 - 10) * 2,
        _ => return None,
    };
    Some(ConfigurationKey(key))
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_profile_delta_factors(first: DeltaIndex, second: DeltaIndex, output: *mut CalibrationTriplet) {
    unsafe { write_volatile(addr_of_mut!((*output).values[2]), ProfileGain(1024)) };
    for (index, selection) in [first, second].iter().enumerate() {
        let gain = match delta_key(*selection, index == 0) {
            Some(key) => unsafe { xm1_buffers_configuration_half(key) },
            None => 1024,
        };
        unsafe { write_volatile(addr_of_mut!((*output).values[index]), ProfileGain(gain)) };
    }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_profile_split_factors(selection: SplitIndex, output: *mut CalibrationTriplet) {
    let index = (selection.0 as u8 as u32).min(unsafe { xm1_pipeline_image_profile_split_count() } - 1);
    let table = Address32(0xef5048).offset(ByteOffset(index * 4));
    let first = unsafe { setting_half(table.half() as u32) };
    unsafe { write_volatile(addr_of_mut!((*output).values[0]), ProfileGain(first)) };
    let second = unsafe { setting_half(table.offset(ByteOffset(2)).half() as u32) };
    unsafe { write_volatile(addr_of_mut!((*output).values[1]), ProfileGain(second)) };
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_profile_selected_factors(selection: ProfileSelector, output: *mut CalibrationTriplet) {
    let key = match selection.0 { 1 => Some(0x80d1), 2 => Some(0x80d5), 3 => Some(0x80e9), 4 => Some(0x80d9), 5 => Some(0x80dd), 6 => Some(0x80e1), 7..=12 => Some(0x80e5), 14 => Some(0x80ed), _ => None };
    if let Some(key) = key {
        let first = unsafe { setting_half(key) };
        unsafe { write_volatile(addr_of_mut!((*output).values[0]), ProfileGain(first)) };
        let second = unsafe { setting_half(key + 2) };
        unsafe { write_volatile(addr_of_mut!((*output).values[1]), ProfileGain(second)) };
    } else {
        unsafe {
            write_volatile(addr_of_mut!((*output).values[0]), ProfileGain(1024));
            write_volatile(addr_of_mut!((*output).values[1]), ProfileGain(1024));
        }
    }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_build_profile_factors(input: *const FactorRequest, output: *mut FactorSet) {
    let words = output.cast::<u16>();
    for offset in [11, 0, 1, 3, 4, 6, 7] { unsafe { write_volatile(words.add(offset), 1024) }; }
    let mode = unsafe { read_volatile(addr_of!((*input).mode)) };
    if matches!(mode.0, 0 | 2) {
        unsafe {
            xm1_pipeline_image_profile_split_factors(read_volatile(addr_of!((*input).split)), addr_of_mut!((*output).groups[1]));
            xm1_pipeline_image_profile_selected_factors(read_volatile(addr_of!((*input).selector)), addr_of_mut!((*output).groups[2]));
            xm1_pipeline_image_profile_delta_factors(read_volatile(addr_of!((*input).first_delta)), read_volatile(addr_of!((*input).second_delta)), addr_of_mut!((*output).groups[0]));
        }
    }
    for lane in 0..2 {
        let selected = unsafe { read_volatile(words.add(6 + lane)) } as u64;
        let split = unsafe { read_volatile(words.add(3 + lane)) } as u64;
        let delta = unsafe { read_volatile(words.add(lane)) } as u64;
        unsafe { write_volatile(words.add(9 + lane), (selected * split * delta >> 20) as u16) };
    }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_calibrate_profile(input: *const FactorRequest, output: *mut CalibrationTriplet, metadata: Address32) {
    let mut factors = MaybeUninit::<FactorSet>::uninit();
    unsafe { xm1_pipeline_image_build_profile_factors(input, factors.as_mut_ptr()) };
    let words = factors.as_ptr().cast::<u16>();
    for lane in 0..3 {
        let value = unsafe { read_volatile(addr_of!((*input).values.values[lane])) }.0 as u32;
        let result = if lane == 0 { value } else {
            let factor = unsafe { read_volatile(words.add(8 + lane)) } as u32;
            value.wrapping_mul(factor).wrapping_add(512) >> 10
        };
        unsafe { write_volatile(addr_of_mut!((*output).values[lane]), ProfileGain(result as u16)) };
    }
    if metadata.0 != 0 {
        for (slot, index) in [3, 4, 6, 7, 0, 1].iter().enumerate() {
            let value = unsafe { read_volatile(words.add(*index)) } as u32;
            unsafe { metadata.offset(ByteOffset(0x9e688 + slot as u32 * 4)).write_word(value) };
        }
        unsafe { metadata.offset(ByteOffset(0x9e6a0)).write_word(1024) };
        for lane in 0..3 {
            let value = unsafe { read_volatile(addr_of!((*output).values[lane])) }.0;
            unsafe { metadata.offset(ByteOffset(0x9e6a4 + lane as u32 * 4)).write_word(value as u32) };
        }
    }
}

fn clamp_bit(class: ProfileClass, lane: u32) -> u32 {
    1u32.checked_shl(((class.0 as u8 as u32) * 3 + lane) & 255).unwrap_or(0)
}

unsafe fn rescale(values: *mut CalibrationTriplet, lane: usize, target: u32, previous: ProfileGain) {
    let previous = previous.0 as u32;
    for other in 0..3 {
        if other == lane { continue; }
        let value = unsafe { read_volatile(addr_of!((*values).values[other])) }.0 as u32;
        let product = value.wrapping_mul(target);
        let result = if previous == 0 { product } else { (unsafe { __aeabi_idiv(product as i32, previous as i32) }) as u32 };
        unsafe { write_volatile(addr_of_mut!((*values).values[other]), ProfileGain(result as u16)) };
    }
    unsafe { write_volatile(addr_of_mut!((*values).values[lane]), ProfileGain(target as u16)) };
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_clamp_profile(class: ProfileClass, values: *mut CalibrationTriplet, flags: *mut ClampFlags, rate: ProfileRate) {
    let mut upper = 0;
    let mut lower = 0;
    if class.0 <= 1 {
        let maximum = unsafe { [32767, setting_half(0x7930) as u32, setting_half(0x7934) as u32] };
        let minimum = if class.0 == 0 {
            unsafe { [setting_half(0x792a) as u32, setting_half(0x7928) as u32, setting_half(0x792c) as u32] }
        } else {
            unsafe { [setting_half(0x7924) as u32, setting_half(0x7922) as u32, setting_half(0x7926) as u32] }
        };
        let scale = unsafe { setting_half(if class.0 == 0 { 0x7938 } else { 0x7936 }) } as u32;
        let ceiling = ((rate.0.wrapping_mul(scale) as i32) / 10) as u16 as u32;
        for lane in 0..3 {
            let previous = unsafe { read_volatile(addr_of!((*values).values[lane])) };
            if previous.0 as u32 >= minimum[lane] { continue; }
            unsafe { rescale(values, lane, minimum[lane], previous) };
            lower |= clamp_bit(class, lane as u32);
        }
        if lower != 0 {
            let previous = unsafe { read_volatile(addr_of!((*values).values[0])) };
            if previous.0 as u32 > ceiling {
                unsafe { rescale(values, 0, ceiling, previous) };
                lower |= clamp_bit(class, 3);
            }
            for lane in 1..3 {
                if (unsafe { read_volatile(addr_of!((*values).values[lane])) }.0 as u32) < minimum[lane] {
                    unsafe { write_volatile(addr_of_mut!((*values).values[lane]), ProfileGain(minimum[lane] as u16)) };
                    lower |= clamp_bit(class, lane as u32 + 3);
                }
            }
        }
        for lane in 0..3 {
            if unsafe { read_volatile(addr_of!((*values).values[lane])) }.0 as u32 > maximum[lane] {
                unsafe { write_volatile(addr_of_mut!((*values).values[lane]), ProfileGain(maximum[lane] as u16)) };
                upper |= clamp_bit(class, lane as u32);
            }
        }
    } else {
        let (maximum, minimum) = if class.0 == 2 {
            let second_maximum = unsafe { setting_half(0x7cba) };
            let third_maximum = unsafe { setting_half(0x7cbe) };
            let second_minimum = unsafe { setting_half(0x7cb4) };
            let third_minimum = unsafe { setting_half(0x7cb8) };
            let _ = unsafe { setting_half(0x7938) };
            ([32767, second_maximum, third_maximum], [256, second_minimum, third_minimum])
        } else { ([32767; 3], [256; 3]) };
        for lane in 0..3 {
            let value = unsafe { read_volatile(addr_of!((*values).values[lane])) }.0;
            if value > maximum[lane] {
                unsafe { write_volatile(addr_of_mut!((*values).values[lane]), ProfileGain(maximum[lane])) };
                upper |= clamp_bit(class, lane as u32);
            } else if value < minimum[lane] {
                unsafe { write_volatile(addr_of_mut!((*values).values[lane]), ProfileGain(minimum[lane])) };
                lower |= clamp_bit(class, lane as u32);
            }
        }
    }
    if !flags.is_null() { unsafe { write_volatile(flags, ClampFlags { upper, lower }) }; }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_clamp_geometry(_unused: u32, input: *const CalibrationTriplet, rate: ProfileRate, output: *mut CalibrationTriplet, effective: *mut u8, exposure: i16, metadata: Address32) {
    let threshold = unsafe { xm1_buffers_configuration_byte(ConfigurationKey(0x792e)) } as i32 * 100;
    let mut values = unsafe { read_volatile(input) };
    let rate = ProfileRate(rate.0.max(10));
    let mut flags = ClampFlags { upper: 0, lower: 0 };
    unsafe { xm1_pipeline_image_clamp_profile(ProfileClass(u32::from((exposure as i32) < threshold)), &mut values, &mut flags, rate) };
    if !effective.is_null() { unsafe { write_volatile(effective, rate.0 as u8) }; }
    unsafe { write_volatile(output, values) };
    if metadata.0 != 0 {
        unsafe {
            metadata.offset(ByteOffset(0x9e6b0)).write_word(rate.0);
            let applied = xm1_profile_load_optional_rate_byte(effective);
            metadata.offset(ByteOffset(0x9e6b4)).write_word(applied);
            metadata.offset(ByteOffset(0x9e6b8)).write_word(flags.upper);
            metadata.offset(ByteOffset(0x9e6bc)).write_word(flags.lower);
        }
    }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_clamp_profile_output(input: *mut CalibrationTriplet, output: *mut CalibrationTriplet, metadata: Address32) {
    let mut flags = ClampFlags { upper: 0, lower: 0 };
    unsafe { xm1_pipeline_image_clamp_profile(ProfileClass(3), input, &mut flags, ProfileRate(10)) };
    unsafe { write_volatile(output, read_volatile(input)) };
    if metadata.0 != 0 {
        unsafe {
            metadata.offset(ByteOffset(0xa0428)).write_word(flags.upper);
            metadata.offset(ByteOffset(0xa042c)).write_word(flags.lower);
            for lane in 0..3 { metadata.offset(ByteOffset(0xa0430 + lane as u32 * 4)).write_word(read_volatile(addr_of!((*output).values[lane])).0 as u32); }
        }
    }
}

#[derive(Clone, Copy)]
#[repr(transparent)]
pub(super) struct ExposureHundredths(pub(super) i32);
#[derive(Clone, Copy)]
#[repr(transparent)]
pub(super) struct Percent(pub(super) u32);

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_profile_history_linearize(exposure: ExposureHundredths) -> u32 {
    let quotient = exposure.0 / 100;
    let index = exposure.0.wrapping_sub(quotient.wrapping_mul(100)).wrapping_add(100) as i16;
    let value = unsafe { Address32(0xee9dc8).offset(ByteOffset(index as i32 as u32)).byte() } as u32;
    let quotient = quotient as i16;
    if quotient < 0 {
        let shift = quotient.wrapping_neg() as u32 & 255;
        65536u32.checked_shr(shift).unwrap_or(0).wrapping_mul(value) >> 16
    } else {
        1u32.checked_shl(quotient as u32 & 255).unwrap_or(0).wrapping_mul(value)
    }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_profile_scale_percent(values: *mut CalibrationTriplet, percent: Percent) {
    for lane in 0..3 {
        let value = unsafe { read_volatile(addr_of!((*values).values[lane])) }.0 as u64;
        unsafe { write_volatile(addr_of_mut!((*values).values[lane]), ProfileGain((value * percent.0 as u64 / 100) as u16)) };
    }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_apply_negative_profile_gain(exposure: ExposureHundredths, values: *mut CalibrationTriplet, output: *mut u8) {
    unsafe { write_volatile(output, 100) };
    if exposure.0 >= 0 { return; }
    let mut minimum = 0u16;
    for (lane, key) in [0xce81, 0xce7f, 0xce83].iter().enumerate() {
        let empty = unsafe { read_volatile(addr_of!((*values).values[lane])) }.0 == 0;
        let scaled = unsafe { setting_half(*key) } as u32 * 100;
        let ratio = if empty { scaled } else {
            let previous = unsafe { read_volatile(addr_of!((*values).values[lane])) }.0 as i32;
            (unsafe { __aeabi_idiv(scaled as i32, previous) }) as u32
        };
        minimum = minimum.max(ratio as u16);
    }
    let minimum = minimum as u8 as u32;
    let requested = unsafe { xm1_pipeline_image_profile_history_linearize(ExposureHundredths(exposure.0 as i16 as i32)) } as u8 as u32;
    let percent = if requested < minimum {
        unsafe { write_volatile(output, (requested * 100 / minimum) as u8) };
        minimum
    } else { requested };
    unsafe { xm1_pipeline_image_profile_scale_percent(values, Percent(percent)) };
}

#[derive(Clone, Copy)]
#[repr(transparent)]
pub(super) struct CurveIndex(pub(super) u32);
#[derive(Clone, Copy)]
#[repr(transparent)]
pub(super) struct ConfigurationChannel(pub(super) i32);
#[derive(Clone, Copy)]
#[repr(transparent)]
pub(super) struct ChannelSample(pub(super) i16);

#[unsafe(no_mangle)]
pub(super) extern "C" fn xm1_pipeline_image_round_tenths(value: i32) -> i32 {
    calibration::round_tenths(calibration::Tenths(value)).0
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_classify_curve_index(index: CurveIndex) -> calibration::CurveClass {
    calibration_value(unsafe { physical_calibration() }.classify_curve_index(calibration::CurveIndex(index.0)))
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_collapse_curve_index(index: CurveIndex) -> u32 {
    calibration_value(unsafe { physical_calibration() }.collapse_curve_index(calibration::CurveIndex(index.0))).0
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_configuration_mapped_word(index: CurveIndex, channel: ConfigurationChannel) -> i32 {
    calibration_value(unsafe { physical_calibration() }.mapped_word(calibration::CurveIndex(index.0), calibration::ChannelIndex(channel.0))).0 as i32
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_transform_mapped_word(index: CurveIndex, channel: ConfigurationChannel) -> u16 {
    calibration_value(unsafe { physical_calibration() }.transform_mapped_word(calibration::CurveIndex(index.0), calibration::ChannelIndex(channel.0))).0
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_channel_curve_origin(channel: ConfigurationChannel) -> u32 {
    calibration_value(unsafe { physical_calibration() }.channel_curve_origin(calibration::ChannelIndex(channel.0))).0 as u32
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_channel_bias(channel: ConfigurationChannel, selector: u32) -> i32 {
    let bank = if selector == 254 { calibration::BiasBank::Alternate } else { calibration::BiasBank::Primary };
    calibration_value(unsafe { physical_calibration() }.channel_bias(calibration::ChannelIndex(channel.0), bank)).0 as i32
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_channel_biased_word(channel: ConfigurationChannel, selector: u32) -> i32 {
    let bank = if selector == 254 { calibration::BiasBank::Alternate } else { calibration::BiasBank::Primary };
    calibration_value(unsafe { physical_calibration() }.channel_biased_word(calibration::ChannelIndex(channel.0), bank)).0 as i32
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_interpolate_channel_curve(channel: ConfigurationChannel, sample: ChannelSample) -> u32 {
    calibration_value(unsafe { physical_calibration() }.interpolate_channel_curve(calibration::ChannelIndex(channel.0), calibration::ChannelSample(sample.0))).0 as u32
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_channel_curve_value(channel: ConfigurationChannel) -> u32 {
    calibration_value(unsafe { physical_calibration() }.channel_curve_value(calibration::ChannelIndex(channel.0))).0 as u32
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_scaled_channel_curve(channel: ConfigurationChannel) -> u32 {
    let channel = calibration::ChannelIndex(channel.0);
    if let Some(value) = channel.rejected_value() { return value.0 as u32; }
    calibration_value(unsafe { physical_calibration() }.scaled_channel_curve(channel)).0 as u32
}

#[derive(Clone, Copy)]
#[repr(transparent)]
pub(super) struct EncodedProfileLevel(pub(super) i32);

#[unsafe(no_mangle)]
pub(super) extern "C" fn xm1_pipeline_image_decode_profile_level(level: EncodedProfileLevel) -> i32 {
    match level.0 { 2..=5 => level.0 - 1, _ => 0 }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_profile_converted_decade(level: EncodedProfileLevel) -> i32 {
    let index = xm1_pipeline_image_decode_profile_level(level);
    let value = unsafe { xm1_buffers_configuration_byte(ConfigurationKey(0x4a24 + index as u32)) };
    (value as i32 * 10) as i16 as i32
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_profile_apply_converted_value(values: *mut CalibrationTriplet, exposure: ExposureHundredths) {
    let percent = unsafe { xm1_pipeline_image_profile_history_linearize(exposure) };
    unsafe { xm1_pipeline_image_profile_scale_percent(values, Percent(percent)) };
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_profile_apply_positive(exposure: ExposureHundredths, values: *mut CalibrationTriplet) {
    if exposure.0 > 0 { unsafe { xm1_pipeline_image_profile_apply_converted_value(values, exposure) }; }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_profile_apply_difference(first: EncodedProfileLevel, second: EncodedProfileLevel, values: *mut CalibrationTriplet) {
    let first = unsafe { xm1_pipeline_image_profile_converted_decade(first) };
    let second = unsafe { xm1_pipeline_image_profile_converted_decade(second) };
    unsafe { xm1_pipeline_image_profile_apply_converted_value(values, ExposureHundredths(first.wrapping_sub(second) as i16 as i32)) };
}

#[derive(Clone, Copy)]
#[repr(transparent)]
pub(super) struct StreamFormat(pub(super) u32);
#[derive(Clone, Copy)]
#[repr(transparent)]
pub(super) struct ByteCount(pub(super) u32);
#[derive(Clone, Copy)]
#[repr(transparent)]
pub(super) struct CalibrationEntries(pub(super) u32);
#[derive(Clone, Copy)]
#[repr(transparent)]
pub(super) struct PairControl(pub(super) u32);
#[derive(Clone, Copy)]
#[repr(transparent)]
pub(super) struct StreamRequestKind(pub(super) u32);
#[derive(Clone, Copy)]
#[repr(transparent)]
pub(super) struct InterruptHandle(pub(super) u32);
#[derive(Clone, Copy)]
#[repr(transparent)]
pub(super) struct TaskId(pub(super) u32);
#[derive(Clone, Copy)]
#[repr(transparent)]
pub(super) struct TaskFlags(pub(super) u32);
#[derive(Clone, Copy)]
#[repr(transparent)]
pub(super) struct StreamInterruptMask(pub(super) u32);
#[derive(Clone, Copy)]
#[repr(C)]
pub(super) struct StreamParameters {
    pub(super) format: StreamFormat,
    pub(super) source: Address32,
    pub(super) width: ByteCount,
    pub(super) destination: Address32,
    pub(super) length: CalibrationEntries,
    pub(super) coefficients: Address32,
    pub(super) alternate: PairControl,
}
#[repr(C)]
pub(super) struct CalibrationOverlay {
    pub(super) mode: StreamRequestKind,
    pub(super) packing: StreamFormat,
    pub(super) address: Address32,
    pub(super) stride: ByteCount,
    pub(super) start: Address32,
    pub(super) extent: CalibrationEntries,
    pub(super) flags: PairControl,
    pub(super) parameters: Address32,
}
#[repr(C)]
pub(super) struct StreamFinish {
    pub(super) requested: StreamRequestKind,
    pub(super) completed: Option<unsafe extern "C" fn()>,
}
#[repr(C)]
pub(super) struct StreamSpan { pub(super) address: Address32, pub(super) length: CalibrationEntries }
#[repr(u32)]
enum StreamPhase { Prepared = 1, Configured = 2, Running = 3, Completed = 4, Stopped = 5, Released = 6 }
#[repr(C)]
struct StreamState {
    phase: u32,
    handle: InterruptHandle,
    parameters: StreamParameters,
    completed: Option<unsafe extern "C" fn()>,
    requested: StreamRequestKind,
    remaining: CalibrationEntries,
    next_address: Address32,
}
#[derive(Clone, Copy)]
#[repr(u32)]
pub(super) enum StreamError { Range = 1, Alignment = 2, Null = 3, NotEnabled = 4, Pending = 5 }
#[derive(Clone, Copy)]
#[repr(u32)]
enum PfpRegister {
    Control = 0xfff1a000,
    Source = 0xfff1a004,
    Width = 0xfff1a008,
    Destination = 0xfff1a00c,
    Extent = 0xfff1a010,
    Pair = 0xfff1a014,
    Status = 0xfff1a018,
    Acknowledge = 0xfff1a01c,
    InterruptMask = 0xfff1a020,
    Coefficients = 0xfff1a030,
}

impl PfpRegister {
    unsafe fn read(self) -> u32 { unsafe { Address32(self as u32).word() } }
    unsafe fn write(self, value: u32) { unsafe { Address32(self as u32).write_word(value) }; }
}

unsafe extern "C" {
    fn xm1_profile_load_physical_word(address: Address32) -> u32;
    fn xm1_profile_cache_all();
    fn xm1_develop_trace_event(label: Address32);
    fn xm1_pipeline_image_wait_reserved(handle: u32, timeout: u32) -> i32;
    fn xm1_pipeline_image_signal_reserved(handle: u32) -> i32;
    fn xm1_pipeline_image_route_request(selector: u32, slot: u32, level: u32) -> i32;
    fn xm1_pipeline_image_route_acquire(slot: u32, output: Address32) -> i32;
    fn xm1_pipeline_image_route_release(handle: u32) -> i32;
    fn xm1_pipeline_image_interrupt_acquire(source: u32, callback: unsafe extern "C" fn(), output: Address32) -> i32;
    fn xm1_pipeline_image_interrupt_release(handle: InterruptHandle) -> i32;
    fn xm1_pipeline_image_current_task() -> TaskId;
    fn xm1_pipeline_image_send_notification(task: TaskId, flags: TaskFlags) -> i32;
    fn xm1_pipeline_image_wait_all(mask: TaskFlags, events: *mut u32, timeout: u32) -> i32;
    fn xm1_pipeline_image_trace_begin(operation: u32);
    fn xm1_pipeline_image_trace_end(operation: u32);
}

const PFP_DRIVER: Address32 = Address32(0x89f75c);
const PFP_STATE: *mut StreamState = 0x14ce6f8 as *mut StreamState;
const PFP_PACKET: *mut StreamParameters = 0x157a098 as *mut StreamParameters;
const PFP_FINISH: *mut StreamFinish = 0x157a0d0 as *mut StreamFinish;
const PFP_TASK: *mut TaskId = 0x157bce4 as *mut TaskId;

fn halt() -> ! { loop { core::hint::spin_loop(); } }

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_pfp_fault(error: StreamError) -> ! {
    unsafe { PFP_DRIVER.offset(ByteOffset(12)).write_word(error as u32) };
    halt()
}

unsafe fn require_pfp_enabled() {
    if unsafe { PFP_DRIVER.word() } != 1 { unsafe { xm1_pipeline_image_pfp_fault(StreamError::NotEnabled) }; }
}

unsafe fn require_pfp_idle() {
    unsafe { require_pfp_enabled() };
    if unsafe { PFP_DRIVER.offset(ByteOffset(4)).word() } != 0 { unsafe { xm1_pipeline_image_pfp_fault(StreamError::Pending) }; }
}

unsafe fn physical_word<T>(pointer: *const T, offset: u32) -> u32 {
    unsafe { xm1_profile_load_physical_word(Address32((pointer as u32).wrapping_add(offset))) }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_pfp_initialize_control() {
    unsafe { xm1_pipeline_image_wait_reserved(49, u32::MAX) };
    if unsafe { PFP_DRIVER.word() } != 0 { unsafe { xm1_pipeline_image_pfp_fault(StreamError::NotEnabled) }; }
    for index in 0..4 { unsafe { PFP_DRIVER.offset(ByteOffset(index * 4)).write_word(0) }; }
    unsafe {
        xm1_pipeline_image_route_request(0x212, 13, 8);
        if xm1_pipeline_image_route_acquire(44, PFP_DRIVER.offset(ByteOffset(8))) != 0 { halt(); }
        PFP_DRIVER.write_word(1);
    }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_pfp_release_control() {
    if unsafe { PFP_DRIVER.word() } == 1 {
        if unsafe { PFP_DRIVER.offset(ByteOffset(4)).word() } != 0 { unsafe { xm1_pipeline_image_pfp_fault(StreamError::Pending) }; }
        unsafe {
            xm1_pipeline_image_route_release(PFP_DRIVER.offset(ByteOffset(8)).word());
            PFP_DRIVER.offset(ByteOffset(8)).write_word(0);
            xm1_pipeline_image_route_request(0x212, 13, 0);
            PFP_DRIVER.write_word(0);
            xm1_pipeline_image_signal_reserved(49);
        }
    }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_pfp_wait_ready() {
    unsafe { require_pfp_idle(); PfpRegister::Control.write(0); }
    while unsafe { PfpRegister::Control.read() } & 0x10000 == 0 {}
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_pfp_begin(request: *const StreamRequestKind) {
    unsafe { require_pfp_idle() };
    if request.is_null() { unsafe { xm1_pipeline_image_pfp_fault(StreamError::Null) }; }
    if unsafe { read_volatile(request) }.0 == 0 { unsafe { xm1_pipeline_image_pfp_fault(StreamError::Null) }; }
    unsafe { xm1_profile_cache_all() };
    let previous = unsafe { PfpRegister::Control.read() };
    let requested = unsafe { read_volatile(request) }.0;
    unsafe {
        PfpRegister::Control.write((previous & !7) | (requested & 7) | 0x10000);
        PFP_DRIVER.offset(ByteOffset(4)).write_word(1);
    }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_pfp_stop_control() {
    unsafe { require_pfp_enabled(); PFP_DRIVER.offset(ByteOffset(4)).write_word(0); }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_pfp_set_source(parameters: *const StreamParameters) -> *const StreamParameters {
    unsafe { require_pfp_idle() };
    let source = unsafe { physical_word(parameters, 4) } & !0x40000000;
    if parameters.is_null() || source == 0 { unsafe { xm1_pipeline_image_pfp_fault(StreamError::Null) }; }
    if unsafe { read_volatile(addr_of!((*parameters).width)) }.0 == 0 { unsafe { xm1_pipeline_image_pfp_fault(StreamError::Null) }; }
    if source > 0x3ffffff8 { unsafe { xm1_pipeline_image_pfp_fault(StreamError::Range) }; }
    if unsafe { read_volatile(addr_of!((*parameters).width)) }.0 > 0x1fff { unsafe { xm1_pipeline_image_pfp_fault(StreamError::Range) }; }
    let alignment = match unsafe { read_volatile(addr_of!((*parameters).format)) }.0 { 0 => 15, 1 => 3, _ => u32::MAX };
    if unsafe { read_volatile(addr_of!((*parameters).width)) }.0 & alignment != 0 { unsafe { xm1_pipeline_image_pfp_fault(StreamError::Alignment) }; }
    let format = unsafe { read_volatile(addr_of!((*parameters).format)) }.0;
    unsafe {
        PfpRegister::Control.write(((format & 1) << 8) | 0x10000);
        PfpRegister::Source.write(source & 0x3fffffff);
        PfpRegister::Width.write(read_volatile(addr_of!((*parameters).width)).0 & 0x1fff);
    }
    parameters
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_pfp_set_span(span: *const StreamSpan) {
    unsafe { require_pfp_idle() };
    let address = unsafe { physical_word(span, 0) } & !0x40000000;
    if span.is_null() || address == 0 { unsafe { xm1_pipeline_image_pfp_fault(StreamError::Null) }; }
    if address > 0x3ffffff8 { unsafe { xm1_pipeline_image_pfp_fault(StreamError::Range) }; }
    if unsafe { read_volatile(addr_of!((*span).length)) }.0 > 0x3ffff { unsafe { xm1_pipeline_image_pfp_fault(StreamError::Range) }; }
    if address & 7 != 0 { unsafe { xm1_pipeline_image_pfp_fault(StreamError::Alignment) }; }
    unsafe {
        PfpRegister::Destination.write(address & 0x3fffffff);
        PfpRegister::Extent.write(read_volatile(addr_of!((*span).length)).0 & 0x3ffff);
    }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_pfp_set_coefficients(table: Address32) {
    unsafe { require_pfp_idle() };
    if table.0 == 0 { unsafe { xm1_pipeline_image_pfp_fault(StreamError::Null) }; }
    for index in 0..128 {
        let entry = table.offset(ByteOffset(index * 4));
        let first = unsafe { entry.byte() } as u32 & 63;
        let second = (unsafe { entry.half() } as u32 >> 7) & 63;
        let third = (unsafe { entry.word() } >> 14) & 63;
        let fourth = (unsafe { entry.offset(ByteOffset(2)).half() } as u32 >> 5) & 63;
        if (first + second + third + fourth) & !32 != 0 { unsafe { xm1_pipeline_image_pfp_fault(StreamError::Alignment) }; }
        let first = unsafe { entry.byte() } as u32 & 63;
        let second = unsafe { entry.half() } as u32 & 0x1f80;
        let third = unsafe { entry.word() } & 0xfc000;
        let fourth = ((unsafe { entry.offset(ByteOffset(2)).half() } as u32 >> 5) & 63) << 21;
        let classification = (unsafe { entry.offset(ByteOffset(3)).byte() } as u32 >> 4) << 28;
        unsafe { Address32(PfpRegister::Coefficients as u32).offset(ByteOffset(index * 4)).write_word(first | second | third | fourth | classification) };
    }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_pfp_set_pair(parameters: *const PairControl) {
    unsafe { require_pfp_idle() };
    if parameters.is_null() { unsafe { xm1_pipeline_image_pfp_fault(StreamError::Null) }; }
    let first = unsafe { read_volatile(parameters) }.0 & 0x3ff;
    let enabled = unsafe { read_volatile(parameters) }.0 & 0x1000;
    let second = unsafe { read_volatile(parameters) }.0 & 0x3ff0000;
    let alternate = unsafe { read_volatile(parameters) }.0 & 0x10000000;
    unsafe { PfpRegister::Pair.write(first | enabled | second | alternate) };
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_pfp_set_interrupt_mask(parameters: *const StreamInterruptMask) {
    unsafe { require_pfp_idle() };
    if parameters.is_null() { unsafe { xm1_pipeline_image_pfp_fault(StreamError::Null) }; }
    let mut value = 0;
    for bit in 0..5 { value |= unsafe { read_volatile(parameters) }.0 & (1 << bit); }
    unsafe { PfpRegister::InterruptMask.write(value) };
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_pfp_read_status(output: *mut u32) {
    unsafe { require_pfp_enabled() };
    if output.is_null() { unsafe { xm1_pipeline_image_pfp_fault(StreamError::Null) }; }
    unsafe { write_volatile(output, PfpRegister::Status.read()) };
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_pfp_acknowledge() {
    unsafe { require_pfp_enabled(); PfpRegister::Acknowledge.write(31); }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_calibration_stream_initialize() -> i32 {
    unsafe { xm1_pipeline_image_pfp_initialize_control() };
    for index in 0..13 { unsafe { Address32(PFP_STATE as u32).offset(ByteOffset(index * 4)).write_word(0) }; }
    unsafe {
        xm1_pipeline_image_pfp_set_interrupt_mask(&StreamInterruptMask(0));
        xm1_pipeline_image_pfp_wait_ready();
        xm1_pipeline_image_pfp_acknowledge();
        write_volatile(addr_of_mut!((*PFP_STATE).phase), StreamPhase::Prepared as u32);
    }
    0
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_calibration_stream_release() -> i32 {
    unsafe { xm1_pipeline_image_pfp_release_control(); write_volatile(addr_of_mut!((*PFP_STATE).phase), StreamPhase::Released as u32); }
    0
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_calibration_stream_write_parameters(parameters: *const StreamParameters) -> i32 {
    for index in 0..7 {
        let value = unsafe { read_volatile(parameters.cast::<u32>().add(index)) };
        unsafe { write_volatile(addr_of_mut!((*PFP_STATE).parameters).cast::<u32>().add(index), value) };
    }
    unsafe {
        xm1_pipeline_image_pfp_set_source(parameters);
        xm1_pipeline_image_pfp_set_coefficients(read_volatile(addr_of!((*parameters).coefficients)));
    }
    let address = unsafe { read_volatile(addr_of!((*parameters).destination)) };
    let length = unsafe { read_volatile(addr_of!((*parameters).length)) }.0;
    let length = if length >= 0x40000 {
        unsafe {
            write_volatile(addr_of_mut!((*PFP_STATE).remaining), CalibrationEntries(length - 0x3ffe0));
            write_volatile(addr_of_mut!((*PFP_STATE).next_address), address.offset(ByteOffset(0x10ff78)));
        }
        0x3ffe0
    } else {
        unsafe {
            write_volatile(addr_of_mut!((*PFP_STATE).remaining), CalibrationEntries(0));
            write_volatile(addr_of_mut!((*PFP_STATE).next_address), Address32(0));
        }
        length
    };
    unsafe {
        xm1_pipeline_image_pfp_set_span(&StreamSpan { address, length: CalibrationEntries(length) });
        xm1_pipeline_image_pfp_set_pair(addr_of!((*parameters).alternate));
        write_volatile(addr_of_mut!((*PFP_STATE).phase), StreamPhase::Configured as u32);
    }
    0
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_calibration_stream_finish_request(request: *const StreamFinish) -> i32 {
    unsafe { xm1_profile_cache_all() };
    let completed = unsafe { read_volatile(addr_of!((*request).completed)) };
    unsafe { write_volatile(addr_of_mut!((*PFP_STATE).completed), completed) };
    if unsafe { read_volatile(addr_of!((*PFP_STATE).parameters.length)) }.0 == 0 {
        unsafe {
            write_volatile(addr_of_mut!((*PFP_STATE).requested), read_volatile(addr_of!((*request).requested)));
            write_volatile(addr_of_mut!((*PFP_STATE).phase), StreamPhase::Running as u32);
        }
        if let Some(completed) = unsafe { read_volatile(addr_of!((*PFP_STATE).completed)) } { unsafe { completed() }; }
    } else {
        let handle = Address32(unsafe { addr_of_mut!((*PFP_STATE).handle) } as u32);
        if unsafe { xm1_pipeline_image_interrupt_acquire(0x65, xm1_pipeline_image_calibration_stream_complete, handle) } != 0 { halt(); }
        unsafe {
            xm1_pipeline_image_pfp_set_interrupt_mask(&StreamInterruptMask(31));
            let requested = read_volatile(addr_of!((*request).requested));
            xm1_pipeline_image_pfp_begin(&requested);
            write_volatile(addr_of_mut!((*PFP_STATE).requested), read_volatile(addr_of!((*request).requested)));
            write_volatile(addr_of_mut!((*PFP_STATE).phase), StreamPhase::Running as u32);
        }
    }
    0
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_calibration_stream_stop() -> i32 {
    let mut status = 0;
    if unsafe { read_volatile(addr_of!((*PFP_STATE).parameters.length)) }.0 != 0 {
        unsafe { xm1_pipeline_image_pfp_stop_control(); xm1_pipeline_image_pfp_set_interrupt_mask(&StreamInterruptMask(0)); }
        let handle = unsafe { read_volatile(addr_of!((*PFP_STATE).handle)) };
        if handle.0 != 0 {
            status = unsafe { xm1_pipeline_image_interrupt_release(handle) };
            unsafe { write_volatile(addr_of_mut!((*PFP_STATE).handle), InterruptHandle(0)) };
        }
    }
    unsafe {
        write_volatile(addr_of_mut!((*PFP_STATE).completed), None);
        xm1_pipeline_image_pfp_wait_ready();
        xm1_pipeline_image_pfp_acknowledge();
        write_volatile(addr_of_mut!((*PFP_STATE).phase), StreamPhase::Stopped as u32);
    }
    status
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_calibration_stream_complete() {
    let mut status = 0;
    unsafe { xm1_pipeline_image_pfp_read_status(&mut status); xm1_pipeline_image_pfp_acknowledge(); }
    if status & 0x1e != 0 { halt(); }
    if unsafe { read_volatile(addr_of!((*PFP_STATE).remaining)) }.0 == 0 {
        if let Some(completed) = unsafe { read_volatile(addr_of!((*PFP_STATE).completed)) } { unsafe { completed() }; }
        unsafe { write_volatile(addr_of_mut!((*PFP_STATE).phase), StreamPhase::Completed as u32) };
    } else {
        unsafe {
            xm1_pipeline_image_pfp_stop_control();
            write_volatile(addr_of_mut!((*PFP_STATE).parameters.destination), read_volatile(addr_of!((*PFP_STATE).next_address)));
            write_volatile(addr_of_mut!((*PFP_STATE).parameters.length), read_volatile(addr_of!((*PFP_STATE).remaining)));
            xm1_pipeline_image_calibration_stream_write_parameters(addr_of!((*PFP_STATE).parameters));
            let requested = read_volatile(addr_of!((*PFP_STATE).requested));
            xm1_pipeline_image_pfp_begin(&requested);
        }
    }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_calibration_overlay_complete() {
    if unsafe { read_volatile(PFP_TASK) }.0 != 0 {
        let task = unsafe { read_volatile(PFP_TASK) };
        unsafe { xm1_pipeline_image_send_notification(task, TaskFlags(0x10)) };
    }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_process_calibration_overlay(_job: Address32, request: *const CalibrationOverlay) -> i32 {
    let mut events = 0;
    unsafe {
        xm1_develop_trace_event(Address32(0xe665e0));
        xm1_pipeline_image_calibration_stream_initialize();
    }
    for index in 0..7 { unsafe { write_volatile(PFP_PACKET.cast::<u32>().add(index), 0) }; }
    for index in 0..2 { unsafe { write_volatile(PFP_FINISH.cast::<u32>().add(index), 0) }; }
    unsafe {
        write_volatile(addr_of_mut!((*PFP_PACKET).source), read_volatile(addr_of!((*request).address)));
        write_volatile(addr_of_mut!((*PFP_PACKET).format), read_volatile(addr_of!((*request).packing)));
        write_volatile(addr_of_mut!((*PFP_PACKET).width), read_volatile(addr_of!((*request).stride)));
        write_volatile(addr_of_mut!((*PFP_PACKET).destination), read_volatile(addr_of!((*request).start)));
        write_volatile(addr_of_mut!((*PFP_PACKET).length), read_volatile(addr_of!((*request).extent)));
        write_volatile(addr_of_mut!((*PFP_PACKET).coefficients), read_volatile(addr_of!((*request).parameters)));
        write_volatile(addr_of_mut!((*PFP_PACKET).alternate), read_volatile(addr_of!((*request).flags)));
        xm1_pipeline_image_calibration_stream_write_parameters(PFP_PACKET);
        write_volatile(addr_of_mut!((*PFP_FINISH).requested), read_volatile(addr_of!((*request).mode)));
        write_volatile(addr_of_mut!((*PFP_FINISH).completed), Some(xm1_pipeline_image_calibration_overlay_complete));
        xm1_pipeline_image_trace_begin(0x6023300);
        xm1_develop_trace_event(Address32(0xe665e8));
        write_volatile(PFP_TASK, xm1_pipeline_image_current_task());
        xm1_pipeline_image_calibration_stream_finish_request(PFP_FINISH);
        xm1_pipeline_image_wait_all(TaskFlags(0x30), &mut events, u32::MAX);
        write_volatile(PFP_TASK, TaskId(0));
        let result = i32::from(events & 0x20 == 0);
        xm1_pipeline_image_calibration_stream_stop();
        xm1_develop_trace_event(Address32(0xe665f0));
        xm1_pipeline_image_trace_end(0x6023300);
        xm1_pipeline_image_calibration_stream_release();
        xm1_develop_trace_event(Address32(0xe665f8));
        result
    }
}
