use super::shared::{Address32, ByteOffset};
use super::sensor_backend::{mode_record, runtime, xm1_pipeline_sensor_fault, SensorConfiguration, SensorFeatures, SensorMode, SensorRequest};
use super::sensor_requests::{byte, clear, copy, store, word};

unsafe extern "C" {
    fn image_operation_divide_fault(value: u32) -> !;
    fn xm1_pipeline_sensor_program_mode(features: SensorFeatures, fields: *const OrientationParameters);
    fn xm1_pipeline_sensor_program_offsets(fields: *const OffsetParameters);
    fn xm1_pipeline_sensor_program_bounds(features: SensorFeatures, fields: *const BoundsParameters);
    fn xm1_pipeline_sensor_program_addresses(fields: Address32);
    fn xm1_pipeline_sensor_program_color(first: Address32, second: Address32, third: Address32);
    fn xm1_pipeline_sensor_program_extra(value: u32);
    fn xm1_pipeline_sensor_program_filters(first: Address32, second: Address32, third: Address32, fourth: Address32, fifth: Address32, option: u32);
    fn xm1_pipeline_sensor_program_padding(first: Address32, second: Address32, third: Address32);
}

pub(super) fn divide(numerator: u32, denominator: u32) -> u32 {
    if denominator == 0 { unsafe { image_operation_divide_fault(if numerator == 0 { 0 } else { u32::MAX }) } }
    numerator / denominator
}
pub(super) fn left(value: u32, shift: u32) -> u32 { if shift & 255 >= 32 { 0 } else { value << (shift & 255) } }
pub(super) fn right(value: u32, shift: u32) -> u32 { if shift & 255 >= 32 { 0 } else { value >> (shift & 255) } }

#[repr(C)]
struct OrientationParameters { first: u32, second: u32, pattern: u32 }
#[repr(C)]
struct OffsetParameters { reduced: u32, horizontal: u32, vertical: u32, profile_second: u32, profile_first: u32 }
#[repr(C)]
struct BoundsParameters { first: u32, second: u32, color_x: u32, color_y: u32, width: u32, height: u32 }
#[repr(C)]
struct PrimaryFilterParameters { exponent: u32, first: u32, second: u32, enabled: u32 }
#[repr(C)]
struct ColorFilterParameters { exponent: u32, mode: u32, first: u32, second: u32, enabled: u32, tail: u32 }

fn exponent(scale: u32) -> u32 { match scale { 256 => 4, 128 => 3, 64 => 2, 32 => 1, _ => 0 } }
fn address<T>(value: &T) -> Address32 { Address32((value as *const T) as u32) }
unsafe fn primary_filter(configuration: SensorConfiguration, enabled_offset: u32, channel_offset: u32, output: &mut core::mem::MaybeUninit<PrimaryFilterParameters>) -> Address32 {
    let enabled = unsafe { word(configuration.0, enabled_offset) };
    if enabled == 0 || unsafe { word(Address32(word(configuration.0, channel_offset)), 0) } == 0 { return Address32(0); }
    address(output.write(PrimaryFilterParameters { exponent: exponent(unsafe { word(configuration.0, enabled_offset + 4) }), first: 0, second: 0, enabled }))
}
unsafe fn color_filter(configuration: SensorConfiguration, record: Address32, enabled_offset: u32, channel_offset: u32, mode_offset: u32, output: &mut core::mem::MaybeUninit<ColorFilterParameters>) -> Address32 {
    let enabled = unsafe { word(configuration.0, enabled_offset) };
    if enabled == 0 || unsafe { word(Address32(word(configuration.0, channel_offset)), 0) } == 0 { return Address32(0); }
    address(output.write(ColorFilterParameters { exponent: exponent(unsafe { word(configuration.0, enabled_offset + 4) }), mode: unsafe { word(record, mode_offset) }, first: 0, second: 0, enabled, tail: 1 }))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_sensor_program_configuration(request: SensorRequest, configuration: SensorConfiguration) -> i32 {
    let mode = SensorMode(unsafe { word(request.0, 0) });
    let record = mode_record(mode);
    let extra = Address32(unsafe { word(request.0, 0xc8) });
    let features = SensorFeatures(unsafe { word(record, 4) });
    let mut orientation = OrientationParameters {
        first: unsafe { word(configuration.0, 0x54) }, second: unsafe { word(configuration.0, 0x50) },
        pattern: unsafe { byte(request.0, 0x84) as u32 | ((byte(request.0, 0x85) as u32) << 3) },
    };
    if features.0 == 0x80 { orientation.pattern = match orientation.pattern { 0 => 2, 1 => 3, 8 => 1, 9 => 0, _ => xm1_pipeline_sensor_fault() }; }
    unsafe { xm1_pipeline_sensor_program_mode(features, &orientation) };
    let profile = Address32(0xda4d14).offset(ByteOffset(unsafe { word(request.0, 0x80) }.wrapping_mul(12)));
    let offsets = OffsetParameters { reduced: u32::from(mode.0 == 26), horizontal: unsafe { word(request.0, 0x88) }, vertical: unsafe { word(request.0, 0x8c) }, profile_second: unsafe { word(profile, 8) }, profile_first: unsafe { word(profile, 4) } };
    unsafe { xm1_pipeline_sensor_program_offsets(&offsets) };
    let mut bounds = BoundsParameters {
        first: unsafe { word(configuration.0, 12) }, second: unsafe { word(configuration.0, 0x28) },
        color_x: unsafe { word(configuration.0, 0x44) }, color_y: unsafe { word(configuration.0, if matches!(features.0, 2 | 4) { 0x4c } else { 0x48 }) },
        width: divide(unsafe { word(request.0, 12) }.wrapping_mul(10), unsafe { word(configuration.0, 0) }), height: unsafe { word(request.0, 16) },
    };
    if unsafe { word(Address32(0x14cdd64), 0x18) } != 1 { bounds.width = bounds.width.wrapping_sub(1); bounds.height = bounds.height.wrapping_sub(1); }
    unsafe { xm1_pipeline_sensor_program_bounds(features, &bounds); xm1_pipeline_sensor_program_addresses(configuration.0.offset(ByteOffset(0x88))); }
    let mut windows = [Address32(0); 3];
    for (index, (channel, parameters)) in [(0x94,0xb8),(0x98,0xdc),(0x9c,0x100)].into_iter().enumerate() {
        if unsafe { word(configuration.0, channel) } != 0 { windows[index] = configuration.0.offset(ByteOffset(parameters)); }
    }
    unsafe { xm1_pipeline_sensor_program_color(windows[0], windows[1], windows[2]) };
    if extra.0 != 0 { unsafe { xm1_pipeline_sensor_program_extra(word(extra, 4)) }; }
    let mut first = core::mem::MaybeUninit::uninit();
    let mut second = core::mem::MaybeUninit::uninit();
    let mut third = core::mem::MaybeUninit::uninit();
    let mut fourth = core::mem::MaybeUninit::uninit();
    let mut fifth = core::mem::MaybeUninit::uninit();
    let first = unsafe { primary_filter(configuration, 0xa0, 0x88, &mut first) };
    let second = unsafe { primary_filter(configuration, 0xa8, 0x8c, &mut second) };
    let third = unsafe { color_filter(configuration, record, 0xb0, 0x94, 0x34, &mut third) };
    let fourth = unsafe { color_filter(configuration, record, 0xd4, 0x98, 0x40, &mut fourth) };
    let fifth = unsafe { color_filter(configuration, record, 0xf8, 0x9c, 0x4c, &mut fifth) };
    unsafe {
        xm1_pipeline_sensor_program_filters(first, second, third, fourth, fifth, word(record, 0x54));
        xm1_pipeline_sensor_program_padding(configuration.0.offset(ByteOffset(0x150)), configuration.0.offset(ByteOffset(0x15c)), configuration.0.offset(ByteOffset(0x168)));
    }
    0
}

unsafe fn channel(configuration: SensorConfiguration, index: u32, address: Address32, stride: u32, divisor_offset: u32) {
    let parameters = configuration.0.offset(ByteOffset(0x58 + index * 8));
    unsafe {
        store(parameters, 0, address.0);
        store(parameters, 4, divide(stride.wrapping_mul(10), word(configuration.0, divisor_offset)));
        store(configuration.0, 0x88 + index * 4, parameters.0);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_sensor_prepare_channels(request: SensorRequest, configuration: SensorConfiguration) {
    let record = mode_record(SensorMode(unsafe { word(request.0, 0) }));
    unsafe { clear(configuration.0.offset(ByteOffset(0x88)), 24) };
    if unsafe { word(record, 4) } & 0x40 != 0 {
        unsafe { channel(configuration, 2, Address32(word(request.0, 4)), word(request.0, 8), 0) };
    } else {
        let first = Address32(unsafe { word(request.0, 4) });
        if first.0 != 0 { unsafe { channel(configuration, 0, first, word(request.0, 8), 0) }; }
        let second = Address32(unsafe { word(request.0, 0x18) });
        if second.0 != 0 {
            let address = if unsafe { word(record, 4) } & 8 != 0 { second.0.wrapping_sub(unsafe { word(request.0, 0x1c) }.wrapping_mul(14)) } else { second.0 };
            unsafe { channel(configuration, 1, Address32(address), word(request.0, 0x1c), 0) };
        }
    }
    let third = Address32(unsafe { word(request.0, 0x2c) });
    if third.0 != 0 {
        let index = if unsafe { word(record, 4) } & 0x80 != 0 { 4 } else { 3 };
        unsafe { channel(configuration, index, third, word(request.0, 0x30), 4) };
    }
    let fifth = Address32(unsafe { word(request.0, 0x64) });
    if fifth.0 != 0 {
        let parameters = configuration.0.offset(ByteOffset(0x78));
        unsafe {
            store(parameters, 0, fifth.0);
            store(parameters, 4, word(request.0, 0x68).wrapping_mul(10) / 20);
            store(configuration.0, 0x98, parameters.0);
        }
    }
    let fourth = Address32(unsafe { word(request.0, 0x48) });
    if fourth.0 != 0 { unsafe { channel(configuration, 5, fourth, word(request.0, 0x4c), 8) }; }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_sensor_prepare_dimensions(request: SensorRequest, configuration: SensorConfiguration) {
    let mode = SensorMode(unsafe { word(request.0, 0) });
    let record = mode_record(mode);
    unsafe { clear(configuration.0.offset(ByteOffset(0x11c)), 52) };
    let mut height = unsafe { word(request.0, 0x38) };
    let mut alternate_height = 0;
    let mut width;
    if unsafe { word(record, 4) } & 6 != 0 {
        if height > 1 { width = divide(unsafe { word(request.0, 0x34) }.wrapping_mul(10), unsafe { word(configuration.0, 4) }); }
        else { width = divide(unsafe { word(request.0, 0x50) }.wrapping_mul(10), unsafe { word(configuration.0, 8) }); height = 0; }
        alternate_height = unsafe { word(request.0, 0x54) };
    } else if height == 1 {
        let shift = unsafe { word(request.0, 0x80) };
        width = left(unsafe { word(request.0, 0x6c) }.wrapping_mul(10) / 20, shift);
        height = left(unsafe { word(request.0, 0x70) }, shift);
    } else {
        width = divide(unsafe { word(request.0, 0x34) }.wrapping_mul(10), unsafe { word(configuration.0, 4) });
    }
    if mode.0 == 26 { width = width.wrapping_mul(4) / 3; height = height.wrapping_mul(4) / 3; alternate_height = alternate_height.wrapping_mul(4) / 3; }
    let divisor = unsafe { word(record, 8) };
    width = divide(width, divisor);
    let available_width = divide(unsafe { word(request.0, 12) }.wrapping_mul(10), unsafe { word(configuration.0, 0) });
    if width > available_width { xm1_pipeline_sensor_fault(); }
    height = divide(height, divisor);
    let available_height = unsafe { word(request.0, 16) };
    if height > available_height { xm1_pipeline_sensor_fault(); }
    alternate_height = divide(alternate_height, divisor);
    if alternate_height > available_height { xm1_pipeline_sensor_fault(); }
    unsafe {
        store(configuration.0, 0x124, available_width.wrapping_sub(width) >> 1);
        store(configuration.0, 0x128, if height == 0 { 0 } else { word(request.0, 16).wrapping_sub(height) >> 1 });
        store(configuration.0, 0x12c, if alternate_height == 0 { 0 } else { word(request.0, 16).wrapping_sub(alternate_height) >> 1 });
    }
    if unsafe { word(request.0, 0xc4) } != 0 {
        unsafe { store(configuration.0, 0x130, word(configuration.0, 0x130).wrapping_add(2)); store(configuration.0, 0x134, word(configuration.0, 0x134).wrapping_add(2)); }
    }
    if unsafe { word(request.0, 0xd0) } != 0 || unsafe { word(request.0, 0xd4) } != 0 {
        unsafe { store(configuration.0, 0x130, word(configuration.0, 0x130).wrapping_add(4)); store(configuration.0, 0x134, word(configuration.0, 0x134).wrapping_add(4)); }
    }
    if unsafe { word(request.0, 0xd8) } != 0 {
        unsafe { store(configuration.0, 0x130, word(configuration.0, 0x130).wrapping_add(1)); store(configuration.0, 0x134, word(configuration.0, 0x134).wrapping_add(1)); }
    }
    if unsafe { word(record, 4) } & 0x80 != 0 {
        unsafe { store(configuration.0, 0x138, word(configuration.0, 0x138).wrapping_add(17)); store(configuration.0, 0x13c, word(configuration.0, 0x13c).wrapping_add(17)); }
    } else if unsafe { word(record, 4) } & 0x159 != 0 {
        let extra = if unsafe { word(record, 0x60) } == 0 { 7 } else { 5 };
        unsafe { store(configuration.0, 0x138, word(configuration.0, 0x138).wrapping_add(extra)); store(configuration.0, 0x13c, word(configuration.0, 0x13c).wrapping_add(7)); }
    }
    for index in 0..13 {
        if unsafe { word(request.0, 0x90 + index * 4) } != 0 {
            for (target, table, component) in [(0x140,0xda4d50,0),(0x144,0xda4d50,4),(0x148,0xda4ff4,0),(0x14c,0xda4ff4,4)] {
                unsafe { store(configuration.0, target, word(configuration.0, target).wrapping_add(word(Address32(table), index * 52 + component))) };
            }
        }
    }
    let margin_x = unsafe { word(configuration.0, 0x138).wrapping_add(word(configuration.0, 0x130)).wrapping_add(word(configuration.0, 0x148).max(word(configuration.0, 0x140))) };
    unsafe { store(configuration.0, 0x11c, margin_x) };
    let margin_y = unsafe { word(configuration.0, 0x13c).wrapping_add(word(configuration.0, 0x134)).wrapping_add(word(configuration.0, 0x14c).max(word(configuration.0, 0x144))) };
    unsafe { store(configuration.0, 0x120, margin_y) };
    if unsafe { word(record, 4) } & 0x80 != 0 { unsafe { store(configuration.0, 0x11c, margin_x.wrapping_add(2) >> 1); store(configuration.0, 0x120, margin_y.wrapping_add(2) >> 1); } }
}

fn negative_alignment(center: u32, margin: u32, alignment: u32, signed: bool) -> u32 {
    if center == 0 { return if margin == 0 { 0u32.wrapping_sub(alignment) } else { 0u32.wrapping_sub(alignment.wrapping_mul(divide(margin.wrapping_sub(1).wrapping_add(alignment), alignment))) }; }
    if center >= margin { return 0; }
    let distance = if signed { (center.wrapping_sub(margin) as i32).unsigned_abs() } else { margin.wrapping_sub(center) };
    0u32.wrapping_sub(alignment.wrapping_mul(divide(distance.wrapping_sub(1).wrapping_add(alignment), alignment)))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_sensor_align_color(request: SensorRequest, configuration: SensorConfiguration) {
    let mode = SensorMode(unsafe { word(request.0, 0) });
    let record = mode_record(mode);
    let horizontal_alignment = match unsafe { word(configuration.0, 0) } { 15 => 96, 10 => 48, _ => 24 };
    let vertical_alignment = if unsafe { word(record, 0x60) } != 0 { 2 } else { 6 };
    let center_x = unsafe { word(configuration.0, 0x124) };
    let shift_x = negative_alignment(center_x, unsafe { word(configuration.0, 0x11c) }, horizontal_alignment, false);
    unsafe { store(configuration.0, 0x44, shift_x); store(configuration.0, 0x1c, if center_x == 0 { (shift_x as i32).unsigned_abs() } else { center_x.wrapping_sub(shift_x) }); }
    let center_y = unsafe { word(configuration.0, 0x128) };
    let shift_y = negative_alignment(center_y, unsafe { word(configuration.0, 0x120) }, vertical_alignment, true);
    unsafe { store(configuration.0, 0x48, shift_y); store(configuration.0, 0x20, if center_y == 0 { (shift_y as i32).unsigned_abs() } else { center_y.wrapping_sub(shift_y) }); }
    let alternate_y = unsafe { word(configuration.0, 0x12c) };
    let alternate_shift = negative_alignment(alternate_y, unsafe { word(configuration.0, 0x120) }, vertical_alignment, true);
    unsafe { store(configuration.0, 0x4c, alternate_shift) };
    if alternate_y == 0 {
        let shift = if unsafe { word(configuration.0, 0x128) } == 0 { unsafe { word(configuration.0, 0x4c) } } else { unsafe { word(configuration.0, 0x48) } };
        unsafe { store(configuration.0, 0x24, (shift as i32).unsigned_abs()) };
    } else {
        unsafe { store(configuration.0, 0x24, alternate_y.wrapping_sub(word(configuration.0, 0x4c))) };
        if unsafe { word(configuration.0, 0x128) } == 0 { xm1_pipeline_sensor_fault(); }
    }
    unsafe {
        store(configuration.0, 0x1c, word(configuration.0, 0x1c).wrapping_mul(word(record, 8)));
        store(configuration.0, 0x20, word(configuration.0, 0x20).wrapping_mul(word(record, 8)));
    }
    if mode.0 == 26 {
        unsafe { store(configuration.0, 0x1c, word(configuration.0, 0x1c).wrapping_mul(3) >> 2); store(configuration.0, 0x20, word(configuration.0, 0x20).wrapping_mul(3) >> 2); }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_sensor_find_alignment(value: u32, output: Address32) {
    if value & 255 == 0 { unsafe { store(output, 0, 256) }; }
    else {
        let mut alignment = 128;
        while alignment >= 16 {
            if value % alignment == 0 { unsafe { store(output, 0, alignment) }; break; }
            alignment >>= 1;
        }
    }
    if unsafe { word(output, 0) } == 0 { xm1_pipeline_sensor_fault(); }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_sensor_build_rows(request: SensorRequest, configuration: SensorConfiguration) {
    let mode = SensorMode(unsafe { word(request.0, 0) });
    let record = mode_record(mode);
    let state = unsafe { runtime(mode) };
    let base = match unsafe { word(configuration.0, 0) } { 15 => 64u32, 10 => 96u32, _ => 32u32 };
    let profile = unsafe { word(request.0, 0x80) };
    let alignment = if matches!(profile, 1..=4) { base << profile } else { base };
    let available = 512u32.wrapping_sub(unsafe { word(configuration.0, 0x1c).wrapping_add(word(configuration.0, 0x11c)) });
    if available < alignment { xm1_pipeline_sensor_fault(); }
    let extent = if unsafe { word(record, 4) } & 6 != 0 { divide(unsafe { word(request.0, 0x50) }.wrapping_mul(10), unsafe { word(configuration.0, 8) }) } else { divide(unsafe { word(request.0, 0x34) }.wrapping_mul(10), unsafe { word(configuration.0, 4) }) };
    let bounded = extent.min(available);
    let width = if mode.0 == 26 { divide(bounded.wrapping_mul(3) >> 2, base).wrapping_mul(base) } else { divide(bounded, alignment).wrapping_mul(alignment) };
    unsafe { store(configuration.0, 0x10, width) };
    if unsafe { word(state.0, 0x18) } == 2 {
        if width == 0 { unsafe { image_operation_divide_fault(if extent == 0 { 0 } else { u32::MAX }) } }
        let remainder = extent % width;
        unsafe { store(configuration.0, 0x14, remainder); store(configuration.0, 0x18, divide(extent, width).wrapping_add(u32::from(remainder != 0))); }
    } else { unsafe { store(configuration.0, 0x14, 0); store(configuration.0, 0x18, 1); } }
    unsafe { store(configuration.0, 12, divide(512, word(record, 8))) };
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_sensor_select_padding(request: SensorRequest, primary: Address32, secondary: Address32, extra: Address32) {
    let record = mode_record(SensorMode(unsafe { word(request.0, 0) }));
    let flags = unsafe { word(record, 4) };
    let shifted = unsafe { word(request.0, 0xa0) } != 0;
    unsafe {
        store(primary, 0, if flags & 1 == 0 { 0 } else if shifted { 0x5e } else { 0x5a });
        store(primary, 4, if flags & 2 == 0 { 0 } else if word(record, 0x18) == 1 { 6 } else { 7 });
        store(primary, 8, if flags & 4 == 0 { 0 } else { 14 });
        store(secondary, 0, if flags & 8 == 0 { 0 } else if shifted { 0x5e } else { 0x5a });
        store(secondary, 4, if flags & 0x10 == 0 { 0 } else if shifted { 0x5e } else { 0x5a });
        store(secondary, 8, if flags & 0x20 == 0 { 0 } else { 44 });
        store(extra, 0, if flags & 0x40 == 0 { 0 } else { 29 });
        store(extra, 4, if flags & 0x80 == 0 { 0 } else if shifted { 0x52 } else { 0x4e });
        store(extra, 8, if flags & 0x100 == 0 { 0 } else { 29 });
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_sensor_prepare_color(request: SensorRequest, configuration: SensorConfiguration) -> i32 {
    let mode = SensorMode(unsafe { word(request.0, 0) });
    let record = mode_record(mode);
    let layout = Address32(unsafe { word(record, 0) });
    unsafe { xm1_pipeline_sensor_align_color(request, configuration); xm1_pipeline_sensor_build_rows(request, configuration); }
    for (mode_offset, destination) in [(0x24,0xa0),(0x28,0xa8)] {
        unsafe { store(configuration.0, destination, word(record, mode_offset)) };
        if unsafe { word(record, mode_offset) } == 1 {
            let value = unsafe { word(configuration.0, 0).wrapping_mul(word(configuration.0, 12)) } / 10;
            unsafe { xm1_pipeline_sensor_find_alignment(value, configuration.0.offset(ByteOffset(destination + 4))) };
        }
    }
    unsafe { store(configuration.0, 0xb0, word(record, 0x30)) };
    if unsafe { word(record, 0x30) } == 1 {
        unsafe { store(configuration.0, 0xc0, word(configuration.0, 0x10)) };
        let value = unsafe { word(configuration.0, 4).wrapping_mul(word(configuration.0, 0x10)) } / 10;
        unsafe {
            xm1_pipeline_sensor_find_alignment(value, configuration.0.offset(ByteOffset(0xb4)));
            store(configuration.0, 0xcc, divide(word(request.0, 0x34).wrapping_mul(10), word(configuration.0, 4)).wrapping_add(word(request.0, 0x3c)));
            store(configuration.0, 0xd0, word(request.0, 0x40).wrapping_add(word(request.0, 0x38)));
            store(configuration.0, 0xc4, word(request.0, 0x3c));
            store(configuration.0, 0xc8, word(request.0, 0x40));
            store(configuration.0, 0xb8, word(configuration.0, 0x1c));
            store(configuration.0, 0xbc, word(configuration.0, 0x20));
        }
    }
    unsafe { store(configuration.0, 0xd4, word(record, 0x3c)) };
    if unsafe { word(record, 0x3c) } == 1 {
        let profile = unsafe { word(request.0, 0x80) };
        if unsafe { word(record, 0x50) } == 0 && profile != 0 { xm1_pipeline_sensor_fault(); }
        let channel = if profile != 0 { Some(request.0.offset(ByteOffset(0x64))) }
            else if unsafe { word(record, 0x30) } == 0 { Some(request.0.offset(ByteOffset(0x2c))) } else { None };
        if let Some(channel) = channel {
            let width = right(unsafe { word(configuration.0, 0x10) }, profile);
            unsafe {
                store(configuration.0, 0xe4, width);
                xm1_pipeline_sensor_find_alignment(width.wrapping_mul(20) / 10, configuration.0.offset(ByteOffset(0xd8)));
                store(configuration.0, 0xf0, word(channel, 16).wrapping_add(if word(layout, 0x18) == 2 { word(configuration.0, 0x10) } else { word(channel, 8).wrapping_mul(10) / 20 }));
                store(configuration.0, 0xf4, word(channel, 20).wrapping_add(word(channel, 12)));
                store(configuration.0, 0xe8, word(channel, 16));
                store(configuration.0, 0xec, word(channel, 20));
                store(configuration.0, 0xdc, right(word(configuration.0, 0x1c), word(request.0, 0x80)));
                store(configuration.0, 0xe0, right(word(configuration.0, 0x20), word(request.0, 0x80)));
            }
        }
    }
    unsafe { store(configuration.0, 0xf8, word(record, 0x48)) };
    if unsafe { word(record, 0x48) } == 1 {
        if unsafe { word(request.0, 0x48) } == 0 { xm1_pipeline_sensor_fault(); }
        unsafe { store(configuration.0, 0x108, word(configuration.0, 0x10)) };
        let value = unsafe { word(configuration.0, 8).wrapping_mul(word(configuration.0, 0x10)) } / 10;
        unsafe {
            xm1_pipeline_sensor_find_alignment(value, configuration.0.offset(ByteOffset(0xfc)));
            store(configuration.0, 0x114, divide(word(request.0, 0x50).wrapping_mul(10), word(configuration.0, 8)).wrapping_add(word(request.0, 0x58)));
            store(configuration.0, 0x118, word(request.0, 0x5c).wrapping_add(word(request.0, 0x54)));
            store(configuration.0, 0x10c, word(request.0, 0x58));
            store(configuration.0, 0x110, word(request.0, 0x5c));
            store(configuration.0, 0x100, word(configuration.0, 0x1c));
            store(configuration.0, 0x104, word(configuration.0, 0x24));
        }
    }
    let second_address = if unsafe { word(configuration.0, 0x9c) } != 0 { unsafe { word(configuration.0, 0x108) } }
        else if unsafe { word(configuration.0, 0x94) } != 0 { unsafe { word(configuration.0, 0xc0) } }
        else if mode.0 == 26 { unsafe { word(configuration.0, 0xe4) }.wrapping_mul(4) / 3 }
        else { unsafe { word(configuration.0, 0xe4) } };
    unsafe { store(configuration.0, 0x28, divide(second_address, word(record, 8))) };
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_sensor_build_configuration(request: SensorRequest, configuration: SensorConfiguration) -> i32 {
    let mode = SensorMode(unsafe { word(request.0, 0) });
    let record = mode_record(mode);
    unsafe { store(configuration.0, 0x50, 1); store(configuration.0, 0, 0); store(configuration.0, 0x54, 1); store(configuration.0, 4, 0); store(configuration.0, 8, 0); }
    if unsafe { word(record, 4) } & 32 != 0 {
        unsafe { store(configuration.0, 0, 20); store(configuration.0, 4, 20); store(configuration.0, 0x50, 0); store(configuration.0, 0x54, 0); }
    } else if unsafe { word(record, 4) } & 128 != 0 {
        unsafe { store(configuration.0, 0, 10); store(configuration.0, 4, 20); }
    } else {
        if unsafe { word(request.0, 0x14) } == 2 { unsafe { store(configuration.0, 0x50, 0); store(configuration.0, 0, 20); } }
        else { unsafe { store(configuration.0, 0, 15) }; }
        if unsafe { word(record, 4) } & 0x140 != 0 { unsafe { store(configuration.0, 0x54, 0); store(configuration.0, 4, 10); } }
        else { unsafe { store(configuration.0, 4, if word(record, 4) & 0x19 != 0 { 20 } else { 15 }) }; }
        if unsafe { word(record, 4) } & 6 != 0 {
            if unsafe { word(request.0, 0x60) } == 2 { unsafe { store(configuration.0, 0x54, 0); store(configuration.0, 8, 20); } }
            else { unsafe { store(configuration.0, 0x54, 1); store(configuration.0, 8, 15); } }
        }
    }
    unsafe {
        xm1_pipeline_sensor_prepare_channels(request, configuration);
        xm1_pipeline_sensor_prepare_dimensions(request, configuration);
        xm1_pipeline_sensor_select_padding(request, configuration.0.offset(ByteOffset(0x150)), configuration.0.offset(ByteOffset(0x15c)), configuration.0.offset(ByteOffset(0x168)));
        xm1_pipeline_sensor_prepare_color(request, configuration);
    }
    let (center_x, center_y);
    if unsafe { word(record, 4) } & 6 != 0 {
        if unsafe { word(request.0, 0x38) } > 1 {
            center_x = unsafe { (divide(word(request.0, 0x34).wrapping_mul(10), word(configuration.0, 4)) >> 1).wrapping_add(word(configuration.0, 0x124)) };
            center_y = unsafe { word(configuration.0, 0x128).wrapping_add(word(request.0, 0x38) >> 1) };
        } else {
            center_x = unsafe { (divide(word(request.0, 0x50).wrapping_mul(10), word(configuration.0, 8)) >> 1).wrapping_add(word(configuration.0, 0x124)) };
            center_y = unsafe { word(configuration.0, 0x12c).wrapping_add(word(request.0, 0x54) >> 1) };
        }
    } else if unsafe { word(request.0, 0x38) } == 1 {
        center_x = unsafe { word(configuration.0, 0x124).wrapping_add(left(word(request.0, 0x6c).wrapping_mul(10) / 20, word(request.0, 0x80)) >> 1) };
        center_y = unsafe { word(configuration.0, 0x128).wrapping_add(left(word(request.0, 0x70), word(request.0, 0x80)) >> 1) };
    } else if mode.0 == 26 {
        center_x = unsafe { word(configuration.0, 0x124).wrapping_add(divide(word(request.0, 0x34).wrapping_mul(10), word(configuration.0, 4)).wrapping_mul(2) / 3) };
        center_y = unsafe { word(configuration.0, 0x128).wrapping_add(word(request.0, 0x38).wrapping_mul(2) / 3) };
    } else {
        center_x = unsafe { (divide(word(request.0, 0x34).wrapping_mul(10), word(configuration.0, 4)) >> 1).wrapping_add(word(configuration.0, 0x124)) };
        center_y = unsafe { word(configuration.0, 0x128).wrapping_add(word(request.0, 0x38) >> 1) };
    }
    unsafe {
        store(configuration.0, 0x34, center_x);
        store(configuration.0, 0x38, center_y);
        store(configuration.0, 0x3c, center_x);
        store(configuration.0, 0x40, center_y);
        store(configuration.0, 0x2c, divide(center_x, word(record, 8)));
        store(configuration.0, 0x30, divide(center_y, word(record, 8)));
    }
    if unsafe { word(record, 4) } & 6 == 0 && unsafe { word(request.0, 0x38) } != 1 && mode.0 == 26 && unsafe { word(request.0, 0x98) } != 0 {
        let shift = unsafe { word(configuration.0, 0x44) } as i32;
        let adjusted = if shift < 0 { center_x.wrapping_add(shift.unsigned_abs().wrapping_mul(2)).wrapping_add(4096) } else { center_x.wrapping_add(4096).wrapping_sub((shift as u32).wrapping_mul(2)) };
        unsafe { store(configuration.0, 0x34, adjusted) };
    }
    0
}
