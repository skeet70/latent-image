use super::shared::{Address32, ByteOffset};
use core::ptr::{read_volatile, write_volatile};

#[derive(Clone, Copy)]
#[repr(transparent)]
pub(super) struct CalibrationKey(u32);
#[derive(Clone, Copy)]
#[repr(transparent)]
pub(super) struct DistortionStrength(u32);
#[derive(Clone, Copy)]
#[repr(transparent)]
pub(super) struct DistortionBand(u32);
#[derive(Clone, Copy)]
#[repr(transparent)]
pub(super) struct DistortionRegion(u32);
#[derive(Clone, Copy)]
#[repr(transparent)]
pub(super) struct CalibrationProfile(u32);
#[derive(Clone, Copy)]
#[repr(transparent)]
pub(super) struct FormatShift(u32);
impl FormatShift {
    fn left(self, value: u32) -> u32 { let shift = self.0 & 255; if shift < 32 { value << shift } else { 0 } }
    fn right(self, value: u32) -> u32 { let shift = self.0 & 255; if shift < 32 { value >> shift } else { 0 } }
}
#[derive(Clone, Copy)]
#[repr(C)]
pub(super) struct AxisPair { horizontal: u32, vertical: u32 }
#[derive(Clone, Copy)]
#[repr(C)]
pub(super) struct HalfPair { horizontal: u16, vertical: u16 }
#[repr(C)]
pub(super) struct ResizeConfiguration {
    mode: u32, format: FormatShift, scaling: AxisPair,
    source_address: Address32, source_stride: u32, source_dimensions: AxisPair, source_limits: AxisPair,
    destination_address: Address32, destination_stride: u32, destination_origin: AxisPair,
    destination_dimensions: AxisPair, destination_limits: AxisPair, correction: Address32, completion: u32,
}
#[repr(C)]
pub(super) struct DistortionResult {
    mode: u32, gains: AxisPair, coefficients: HalfPair, scales: HalfPair, source_limits: AxisPair, samples: [u32; 31],
}
#[repr(C)]
pub(super) struct CalibrationCoefficients {
    gains: HalfPair, reserved04: [u32; 2], coefficients: HalfPair, reserved10: u32, samples: [u32; 31],
}
#[repr(C)]
pub(super) struct RadialScratch {
    reserved00: [u32; 8], padding: AxisPair, limits: AxisPair, center: AxisPair,
    gains: AxisPair, scaling: AxisPair, reserved48: [u32; 4], coefficients: HalfPair, scales: HalfPair,
    source_limits: AxisPair, samples: [u32; 31],
}
#[derive(Clone, Copy)]
#[repr(C)]
pub(super) struct SignedBounds { maximum: i32, minimum: i32 }

unsafe extern "C" {
    fn xm1_buffers_configuration_byte(key: CalibrationKey) -> u32;
    fn xm1_pipeline_image_configuration_signed_half(key: CalibrationKey) -> i32;
    fn xm1_pipeline_image_distortion_band(stage: *mut u8) -> DistortionBand;
}
fn magnitude(value: u32) -> u32 { if value as i32 >= 0 { value } else { value.wrapping_neg() } }

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_configuration_u24(key: CalibrationKey) -> u32 {
    let first = unsafe { xm1_buffers_configuration_byte(key) };
    let second = unsafe { xm1_buffers_configuration_byte(CalibrationKey(key.0.wrapping_add(1))) };
    let third = unsafe { xm1_buffers_configuration_byte(CalibrationKey(key.0.wrapping_add(2))) };
    first | ((second | (third << 8)) << 8)
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_distortion_region(stage: *mut u8) -> DistortionRegion {
    let band = unsafe { xm1_pipeline_image_distortion_band(stage) }.0.wrapping_mul(4);
    let distance = unsafe { read_volatile(stage.add(0x2264) as *const i16) } as i32;
    for (index, base) in [0x3e3a10u32, 0x3e3a12].into_iter().enumerate() {
        let threshold = unsafe { xm1_pipeline_image_configuration_signed_half(CalibrationKey(base.wrapping_add(band))) } as u16 as i32;
        if distance <= threshold { return DistortionRegion(index as u32); }
    }
    DistortionRegion(2)
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_read_distortion_coefficients(strength: DistortionStrength, band: DistortionBand, region: DistortionRegion, profile: CalibrationProfile, output: *mut CalibrationCoefficients) {
    let (base, offset) = if strength.0 != 0 {
        (0x403600u32, strength.0.wrapping_sub(1).wrapping_mul(101))
    } else {
        (match profile.0 { 1 => 0x401200, 2 => 0x402400, _ => 0x400000 }, region.0.wrapping_mul(15).wrapping_add(band.0).wrapping_mul(101))
    };
    let base = base.wrapping_add(offset);
    for index in 0..31 { unsafe { write_volatile(core::ptr::addr_of_mut!((*output).samples).cast::<u32>().add(index), xm1_pipeline_image_configuration_u24(CalibrationKey(base.wrapping_add(index as u32 * 3)))) }; }
    unsafe {
        let gain_x = xm1_pipeline_image_configuration_signed_half(CalibrationKey(base.wrapping_add(93))) as u16;
        write_volatile(core::ptr::addr_of_mut!((*output).gains.horizontal), gain_x);
        let gain_y = xm1_pipeline_image_configuration_signed_half(CalibrationKey(base.wrapping_add(95))) as u16;
        write_volatile(core::ptr::addr_of_mut!((*output).gains.vertical), gain_y);
        let coefficient_x = xm1_pipeline_image_configuration_signed_half(CalibrationKey(base.wrapping_add(97))) as u16;
        write_volatile(core::ptr::addr_of_mut!((*output).coefficients.horizontal), coefficient_x);
        let coefficient_y = xm1_pipeline_image_configuration_signed_half(CalibrationKey(base.wrapping_add(99))) as u16;
        write_volatile(core::ptr::addr_of_mut!((*output).coefficients.vertical), coefficient_y);
    }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_fixed12_round(value: i32, mode: u32) -> i32 {
    let mut result = (magnitude(value as u32) as i32) >> 12;
    if value < 0 { result = result.wrapping_neg(); if mode == 0 && value & 4095 != 0 { result = result.wrapping_sub(1); } }
    else if mode == 1 && value > 0 && value & 4095 != 0 { result = result.wrapping_add(1); }
    result
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_find_signed_bounds(values: *const i32, output: *mut SignedBounds, count: u32) {
    unsafe { write_volatile(core::ptr::addr_of_mut!((*output).maximum), i32::MIN); write_volatile(core::ptr::addr_of_mut!((*output).minimum), i32::MAX); }
    for index in 0..count as usize {
        let value = unsafe { read_volatile(values.add(index)) };
        unsafe {
            write_volatile(core::ptr::addr_of_mut!((*output).maximum), read_volatile(core::ptr::addr_of!((*output).maximum)).max(value));
            let value = read_volatile(values.add(index));
            write_volatile(core::ptr::addr_of_mut!((*output).minimum), read_volatile(core::ptr::addr_of!((*output).minimum)).min(value));
        }
    }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_project_radial_coordinate(x: i32, y: i32, axis: u32, scratch: *const RadialScratch, samples: *const u32, format: FormatShift) -> i32 {
    let coefficient = if axis == 0 { unsafe { read_volatile(core::ptr::addr_of!((*scratch).coefficients.horizontal)) } } else { unsafe { read_volatile(core::ptr::addr_of!((*scratch).coefficients.vertical)) } } as u64;
    let gains = unsafe { read_volatile(core::ptr::addr_of!((*scratch).gains)) };
    let source = unsafe { read_volatile(core::ptr::addr_of!((*scratch).source_limits)) };
    let center = unsafe { read_volatile(core::ptr::addr_of!((*scratch).center)) };
    let scaling = unsafe { read_volatile(core::ptr::addr_of!((*scratch).scaling)) };
    let limits = unsafe { read_volatile(core::ptr::addr_of!((*scratch).limits)) };
    let scales = unsafe { read_volatile(core::ptr::addr_of!((*scratch).scales)) };
    let twice_x = (x as u32).wrapping_mul(2); let twice_y = (y as u32).wrapping_mul(2);
    let mut sx = format.left(scaling.horizontal.wrapping_mul(magnitude(twice_x.wrapping_sub(center.horizontal))) >> 1).min(0x0fffffff);
    let mut sy = format.left(scaling.vertical.wrapping_mul(magnitude(twice_y.wrapping_sub(center.vertical))) >> 1).min(0x0fffffff);
    if (twice_x as i32) < center.horizontal as i32 { sx = sx.wrapping_neg(); }
    if (twice_y as i32) < center.vertical as i32 { sy = sy.wrapping_neg(); }
    let projected_x = sx.wrapping_add(limits.horizontal.wrapping_mul(2048));
    let projected_y = sy.wrapping_add(limits.vertical.wrapping_mul(2048));
    let dx = magnitude(projected_x.wrapping_sub(source.horizontal.wrapping_mul(2048))).min(0x0fffffff);
    let dy = magnitude(projected_y.wrapping_sub(source.vertical.wrapping_mul(2048))).min(0x0fffffff);
    let rx = (scales.horizontal as u64 * dx as u64) >> 9; let ry = (scales.vertical as u64 * dy as u64) >> 9;
    let squared = ((rx.wrapping_mul(rx) as i64) >> 4).wrapping_add((ry.wrapping_mul(ry) as i64) >> 4) as u64;
    let parameter = (squared.wrapping_mul(coefficient) >> 19).min(0x13fff) as u32;
    let fraction = parameter & 8191; let inverse = 8192 - fraction; let square = inverse.wrapping_mul(inverse) >> 10;
    let first = inverse.wrapping_mul(square) >> 13;
    let second = square.wrapping_mul(fraction.wrapping_mul(3)) >> 13;
    let third = (fraction.wrapping_mul(fraction) >> 10).wrapping_mul(inverse.wrapping_mul(3)) >> 13;
    let fourth = 65536u32.wrapping_sub(third).wrapping_sub(second.wrapping_add(first));
    let segment = (parameter >> 13) as usize * 3;
    let term = |index: usize, weight: u32| -> u32 { ((unsafe { read_volatile(samples.add(segment + index)) } as u64 * weight as u64) >> 12) as u32 };
    let value = term(1, second).wrapping_add(term(2, third)).wrapping_add(term(0, first)).wrapping_add(term(3, fourth));
    let (gain, distance, source_center, projected) = if axis == 0 { (gains.horizontal, dx, source.horizontal, projected_x) } else { (gains.vertical, dy, source.vertical, projected_y) };
    let value = gain.wrapping_mul(value >> 4) >> 12;
    let value = ((value as u64 * distance as u64) >> 16).min(0x7ffffff) as u32;
    let center = source_center.wrapping_mul(2048);
    if projected as i32 > center as i32 { center.wrapping_add(value) as i32 } else { center.wrapping_sub(value) as i32 }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_validate_radial_grid(scratch: *const RadialScratch, format: FormatShift, grid: u32) -> i32 {
    if grid == 64 && format.0 == 2 { return 0; }
    let samples = 0x008fa998 as *mut u32;
    for index in 0..31 { unsafe { write_volatile(samples.add(index), read_volatile(core::ptr::addr_of!((*scratch).samples).cast::<u32>().add(index))) }; }
    let mut differences = [0i32; 10];
    for (index, difference) in differences.iter_mut().enumerate() { *difference = unsafe { read_volatile(samples.add(index * 3 + 3)).wrapping_sub(read_volatile(samples.add(index * 3))) } as i32; }
    let mut bounds = SignedBounds { maximum: i32::MIN, minimum: i32::MAX };
    unsafe { xm1_pipeline_image_find_signed_bounds(differences.as_ptr(), &mut bounds, 10) };
    let delta = bounds.maximum;
    for index in 0..24 { let value = unsafe { read_volatile(samples.add(24)) }.wrapping_sub(delta.wrapping_mul(24 - index as i32).wrapping_div(3) as u32); unsafe { write_volatile(samples.add(index), value) }; }
    for index in 25..31 { let value = unsafe { read_volatile(samples.add(24)) }.wrapping_add(delta.wrapping_mul(index as i32 - 24).wrapping_div(3) as u32); unsafe { write_volatile(samples.add(index), value) }; }
    let center = unsafe { read_volatile(core::ptr::addr_of!((*scratch).center)) };
    let padding = unsafe { read_volatile(core::ptr::addr_of!((*scratch).padding)) };
    for rectangle in 0..2 {
        let (xs, ys) = if rectangle == 0 { ([-1, grid as i32, -1, grid as i32], [-1, -1, 8, 8]) } else {
            let left = unsafe { xm1_pipeline_image_fixed12_round(center.horizontal.wrapping_mul(2048) as i32, 0) }.wrapping_sub((grid >> 1) as i32).wrapping_sub(1);
            let right = unsafe { xm1_pipeline_image_fixed12_round(center.horizontal.wrapping_mul(2048) as i32, 1) }.wrapping_add((grid >> 1) as i32).wrapping_add(1);
            let top = unsafe { xm1_pipeline_image_fixed12_round(center.vertical.wrapping_mul(2048) as i32, 0) }.wrapping_sub(5);
            let bottom = unsafe { xm1_pipeline_image_fixed12_round(center.vertical.wrapping_mul(2048) as i32, 1) }.wrapping_add(5);
            ([left, right, left, right], [top, top, bottom, bottom])
        };
        for axis in [1, 0] {
            let mut projected = [0i32; 4];
            for index in 0..4 { projected[index] = unsafe { xm1_pipeline_image_project_radial_coordinate(xs[index], ys[index], axis, scratch, samples, format) }; }
            for (index, value) in projected.iter_mut().enumerate() { let rounding = if axis == 1 { u32::from(index >= 2) } else { (index & 1) as u32 }; *value = unsafe { xm1_pipeline_image_fixed12_round(*value, rounding) }; }
            unsafe { xm1_pipeline_image_find_signed_bounds(projected.as_ptr(), &mut bounds, 4) };
            let extra = if axis == 1 { padding.vertical } else { padding.horizontal };
            let extent = (bounds.maximum.wrapping_sub(bounds.minimum) as u32).wrapping_add(extra.wrapping_mul(2));
            let limit = if axis == 1 { 32 } else if format.0 == 2 { 80 } else { 160 };
            if extent > format.left(limit) { return 0; }
        }
    }
    1
}

unsafe fn copy_axes(destination: *mut AxisPair, source: *const AxisPair) {
    unsafe {
        write_volatile(core::ptr::addr_of_mut!((*destination).horizontal), read_volatile(core::ptr::addr_of!((*source).horizontal)));
        write_volatile(core::ptr::addr_of_mut!((*destination).vertical), read_volatile(core::ptr::addr_of!((*source).vertical)));
    }
}

unsafe fn copy_halves(destination: *mut HalfPair, source: *const HalfPair) {
    unsafe {
        write_volatile(core::ptr::addr_of_mut!((*destination).horizontal), read_volatile(core::ptr::addr_of!((*source).horizontal)));
        write_volatile(core::ptr::addr_of_mut!((*destination).vertical), read_volatile(core::ptr::addr_of!((*source).vertical)));
    }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_choose_supplied_radial_grid(coefficients: *const HalfPair, gains: *const AxisPair, source_limits: *const AxisPair, center: *const AxisPair, limits: *const AxisPair, scaling: *const AxisPair, scales: *const HalfPair, samples: *const u32, padding: *const AxisPair, format: FormatShift) -> u32 {
    let scratch = 0x008faaf8 as *mut RadialScratch;
    unsafe {
        copy_halves(core::ptr::addr_of_mut!((*scratch).coefficients), coefficients);
        copy_axes(core::ptr::addr_of_mut!((*scratch).gains), gains);
        copy_axes(core::ptr::addr_of_mut!((*scratch).source_limits), source_limits);
        copy_axes(core::ptr::addr_of_mut!((*scratch).center), center);
        copy_axes(core::ptr::addr_of_mut!((*scratch).limits), limits);
        copy_axes(core::ptr::addr_of_mut!((*scratch).scaling), scaling);
        if (*scratch).scaling.horizontal == 4096 { write_volatile(core::ptr::addr_of_mut!((*scratch).scaling.horizontal), 4095); }
        if (*scratch).scaling.vertical == 4096 { write_volatile(core::ptr::addr_of_mut!((*scratch).scaling.vertical), 4095); }
        copy_halves(core::ptr::addr_of_mut!((*scratch).scales), scales);
        copy_axes(core::ptr::addr_of_mut!((*scratch).padding), padding);
        for index in 0..31 { write_volatile(core::ptr::addr_of_mut!((*scratch).samples).cast::<u32>().add(index), read_volatile(samples.add(index))); }
    }
    for grid in [64, 32, 16] { if unsafe { xm1_pipeline_image_validate_radial_grid(scratch, format, grid) } != 0 { return grid; } }
    0
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_build_distortion(stage: *mut u8, configuration: *const ResizeConfiguration, output: *mut DistortionResult) {
    let config = unsafe { &*configuration };
    if config.source_limits.horizontal == 0 && config.source_limits.vertical == 0 { loop {} }
    let extent = |limit: u32, size: u32| -> u32 { limit.wrapping_sub(size).wrapping_add(config.format.left(1)) & !1 };
    let source_limits = AxisPair {
        horizontal: extent(config.source_limits.horizontal, config.source_dimensions.horizontal).wrapping_add(config.format.left(config.format.right(config.source_dimensions.horizontal).wrapping_sub(1))),
        vertical: extent(config.source_limits.vertical, config.source_dimensions.vertical).wrapping_add(config.format.left(config.format.right(config.source_dimensions.vertical).wrapping_sub(1))),
    };
    let center = AxisPair { horizontal: config.destination_dimensions.horizontal.wrapping_sub(1), vertical: config.destination_dimensions.vertical.wrapping_sub(1) };
    let address = Address32(stage as u32);
    let word = |offset: u32| -> u32 { unsafe { address.offset(ByteOffset(offset)).word() } };
    let strength = if word(0x3240) == 1 && (1..=6).contains(&word(0x3244)) { 7 - word(0x3244) } else { 0 };
    let profile = match word(0x4b4) { 25 => 1, 1 => (word(0x710) >> 10) & 1, 26 => 2, _ => 0 };
    let band = unsafe { xm1_pipeline_image_distortion_band(stage) }; let region = unsafe { xm1_pipeline_image_distortion_region(stage) };
    let coefficients = 0x015705f0 as *mut CalibrationCoefficients;
    unsafe { xm1_pipeline_image_read_distortion_coefficients(DistortionStrength(strength), band, region, CalibrationProfile(profile), coefficients) };
    let samples = 0x0156892c as *mut u32;
    for index in 0..31 { unsafe { write_volatile(samples.add(index), read_volatile(core::ptr::addr_of!((*coefficients).samples).cast::<u32>().add(index))) }; }
    let gains = unsafe { read_volatile(core::ptr::addr_of!((*coefficients).gains)) };
    let gains = AxisPair { horizontal: gains.horizontal as u32, vertical: gains.vertical as u32 };
    let curve = unsafe { read_volatile(core::ptr::addr_of!((*coefficients).coefficients)) };
    let scales = HalfPair { horizontal: 1, vertical: if word(0x49c) & 0x2000 != 0 { 2 } else { 1 } };
    let padding = AxisPair { horizontal: 4, vertical: 4 };
    let grid = unsafe { xm1_pipeline_image_choose_supplied_radial_grid(&curve, &gains, &source_limits, &center, &source_limits, &config.scaling, &scales, samples, &padding, config.format) };
    unsafe {
        write_volatile(core::ptr::addr_of_mut!((*output).mode), match grid { 64 => 0, 32 => 1, _ => 2 });
        write_volatile(core::ptr::addr_of_mut!((*output).gains), gains);
        write_volatile(core::ptr::addr_of_mut!((*output).coefficients), curve);
        write_volatile(core::ptr::addr_of_mut!((*output).scales), scales);
        write_volatile(core::ptr::addr_of_mut!((*output).source_limits), source_limits);
        for index in 0..31 { write_volatile(core::ptr::addr_of_mut!((*output).samples).cast::<u32>().add(index), read_volatile(samples.add(index))); }
    }
}
