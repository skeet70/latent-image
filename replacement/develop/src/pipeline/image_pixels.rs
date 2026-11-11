use super::image_planes::{current_task, configure_priority, ImagePlane, TaskPriority};
use super::shared::{Address32, ByteOffset};
use core::ptr::{read_volatile, write_volatile};

#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct Fixed16(pub i32);

#[derive(Clone, Copy)]
#[repr(C)]
pub struct AffineCoefficients {
    pub xx: Fixed16,
    pub xy: Fixed16,
    pub reserved0: [i32; 2],
    pub x_bias: Fixed16,
    pub yx: Fixed16,
    pub yy: Fixed16,
    pub reserved1: [i32; 2],
    pub y_bias: Fixed16,
    pub origin_x: i32,
    pub origin_y: i32,
    pub offset_x: i32,
    pub offset_y: i32,
}

#[repr(C)]
struct ScaleConfiguration { block: u32, coefficients: *const AffineCoefficients }

#[repr(C)]
struct PlaneOperation {
    kind: u32,
    argument: u32,
    table: u32,
    coefficients: *const i32,
    kind8: u32,
    kind9: u32,
    parameters: u32,
}

unsafe extern "C" {
    fn image_pixels_cache();
    fn image_pixels_arithmetic_fault(value: u32) -> !;
    fn xm1_buffers_configuration_byte(key: u32) -> u32;
    fn xm1_buffers_configuration_half(key: u32) -> u16;
    fn xm1_pipeline_image_scale_configuration(job: u32, configuration: *const ScaleConfiguration, source: *const ImagePlane, destination: *mut ImagePlane) -> i32;
    fn xm1_pipeline_image_copy_configuration(job: u32, rotation: *const u32, source: *const ImagePlane, destination: *mut ImagePlane) -> i32;
    fn xm1_pipeline_image_operate(job: u32, operation: *const PlaneOperation, sources: *const *const ImagePlane, count: u32, destination: *mut ImagePlane) -> u32;
    fn xm1_pipeline_image_pad_edges(address: u32, stride: u32, width: u32, height: u32, left: u32, right: u32, top: u32, bottom: u32);
    fn xm1_pipeline_image_publish_debug_file(job: u32, kind: u32, plane: *const ImagePlane, argument: u32, flags: u32);
}

fn fatal() -> ! { loop { core::hint::spin_loop(); } }
fn abs_bits(value: i32) -> u32 { value.wrapping_abs() as u32 }
fn trunc16(value: i64) -> i32 { (value / 65536) as i32 }
fn divide_signed(numerator: i64, denominator: i64) -> i64 {
    if denominator == 0 { return if numerator < 0 { i64::MIN } else if numerator > 0 { i64::MAX } else { 0 }; }
    numerator.wrapping_div(denominator)
}
fn divide_unsigned(numerator: u32, denominator: u32) -> u32 {
    if denominator == 0 { unsafe { image_pixels_arithmetic_fault(if numerator == 0 { 0 } else { u32::MAX }) }; }
    numerator / denominator
}
unsafe fn motion_state(job: u32) -> Address32 {
    Address32(unsafe { Address32(job).offset(ByteOffset(0x3228)).word() })
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_blend_pixels(_job: u32, first: *const ImagePlane, second: *const ImagePlane, vertical: i32, destination_selector: u32) {
    let task = unsafe { current_task() };
    unsafe { configure_priority(task, TaskPriority(12)) };
    let distance = abs_bits(vertical);
    let a = unsafe { read_volatile(first) };
    let b = unsafe { read_volatile(second) };
    let mut first_address = a.address.wrapping_add(if vertical < 0 { 0 } else { a.stride.wrapping_mul(distance) });
    let mut second_address = b.address.wrapping_add(if vertical < 0 { b.stride.wrapping_mul(distance) } else { 0 });
    let (mut output, stride) = if destination_selector == 0 { (first_address, a.stride) } else { (second_address, b.stride) };
    unsafe { image_pixels_cache() };
    if b.width > 1 {
        let mut column = 0u32;
        loop {
            if b.height != distance {
                let mut row = 0u32;
                loop {
                    let weight = divide_unsigned(column.wrapping_shl(7), unsafe { read_volatile(core::ptr::addr_of!((*first).width)) } >> 1);
                    let av = unsafe { Address32(first_address.wrapping_add(a.stride.wrapping_mul(row))).word() };
                    let bv = unsafe { Address32(second_address.wrapping_add(b.stride.wrapping_mul(row))).word() };
                    for lane in 0..4 {
                        let left = ((av >> (lane * 8)) & 255) as i32;
                        let right = ((bv >> (lane * 8)) & 255) as i32;
                        let sum = left.wrapping_mul(128i32.wrapping_sub(weight as i32)).wrapping_add(right.wrapping_mul(weight as i32));
                        let rounded = sum.wrapping_add(64);
                        let rounded = if rounded < 0 { sum.wrapping_add(191) } else { rounded };
                        let value = (rounded >> 7).clamp(0, 255) as u8;
                        unsafe { Address32(output.wrapping_add(stride.wrapping_mul(row)).wrapping_add(lane)).write_byte(value) };
                    }
                    row = row.wrapping_add(1);
                    if row >= unsafe { read_volatile(core::ptr::addr_of!((*second).height)) }.wrapping_sub(distance) { break; }
                }
            }
            column = column.wrapping_add(2);
            first_address = first_address.wrapping_add(4);
            second_address = second_address.wrapping_add(4);
            output = output.wrapping_add(4);
            if column >= unsafe { read_volatile(core::ptr::addr_of!((*second).width)) } >> 1 { break; }
        }
    }
    unsafe { image_pixels_cache(); configure_priority(task, TaskPriority(6)); }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_affine_coefficients(_job: u32, input: *const i32, output: *mut AffineCoefficients) -> i32 {
    let a = unsafe { read_volatile(input) } as i64;
    let b = unsafe { read_volatile(input.add(1)) } as i64;
    let c = unsafe { read_volatile(input.add(2)) } as i64;
    let d = unsafe { read_volatile(input.add(3)) } as i64;
    let tx = unsafe { read_volatile(input.add(4)) } as i64;
    let ty = unsafe { read_volatile(input.add(5)) } as i64;
    let xx = divide_signed(a.wrapping_mul(d).wrapping_sub(b.wrapping_mul(c)), d);
    let xy = divide_signed(b << 16, d);
    if xx.unsigned_abs() >= 0x20000 || xy.unsigned_abs() >= 0x20000 { return 0; }
    let bias = divide_signed(tx.wrapping_mul(d).wrapping_sub(ty.wrapping_mul(b)), d);
    if bias.unsigned_abs() >= 0x20000000 || c.unsigned_abs() >= 0x20000 || d.unsigned_abs() >= 0x20000 || ty.unsigned_abs() >= 0x20000000 { return 0; }
    if !output.is_null() {
        unsafe { write_volatile(output, AffineCoefficients { xx: Fixed16(xx as i32), xy: Fixed16(xy as i32), reserved0: [0; 2], x_bias: Fixed16(bias as i32), yx: Fixed16(c as i32), yy: Fixed16(d as i32), reserved1: [0; 2], y_bias: Fixed16(ty as i32), origin_x: 0, origin_y: 0, offset_x: 0, offset_y: 0 }) };
    }
    1
}

impl AffineCoefficients {
    fn map(self, x: i32, y: i32) -> (i32, i32) {
        let dx = (x as i64).wrapping_sub(self.origin_x as i64);
        let dy = (y as i64).wrapping_sub(self.origin_y as i64);
        let mapped_y = self.offset_y.wrapping_add(trunc16((self.y_bias.0 as i64).wrapping_add((self.yx.0 as i64).wrapping_mul(dx)).wrapping_add((self.yy.0 as i64).wrapping_mul(dy))));
        let mapped_x = self.offset_x.wrapping_add(trunc16((self.x_bias.0 as i64).wrapping_add((self.xx.0 as i64).wrapping_mul(dx)).wrapping_add((self.xy.0 as i64).wrapping_mul((mapped_y as i64).wrapping_sub(self.origin_y as i64)))));
        (mapped_x, mapped_y)
    }
}

unsafe fn coefficients(job: u32) -> AffineCoefficients {
    let mut result = core::mem::MaybeUninit::uninit();
    if unsafe { xm1_pipeline_image_affine_coefficients(job, job.wrapping_add(0x2fcc) as *const i32, result.as_mut_ptr()) } == 0 { fatal(); }
    unsafe { result.assume_init() }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_select_affine_block(job: u32, _input: *const i32, output: *mut u32) -> u8 {
    let affine = unsafe { coefficients(job) };
    for block in [64u32, 32, 16] {
        let a = affine.map(7, block.wrapping_sub(1) as i32);
        let b = affine.map(7, 0);
        let height = a.0.max(b.0).wrapping_sub(a.0.min(b.0)).wrapping_add(20);
        let width = a.1.max(b.1).wrapping_sub(a.1.min(b.1)).wrapping_add(12);
        if height <= 160 && width <= 32 {
            if !output.is_null() { unsafe { write_volatile(output, block) }; }
            return 1;
        }
    }
    if !output.is_null() { unsafe { write_volatile(output, 0) }; }
    0
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_affine_copy_safe(job: u32, _input: *const i32, source: *const ImagePlane, destination: *const ImagePlane) -> i32 {
    let source = unsafe { read_volatile(source) };
    let destination = unsafe { read_volatile(destination) };
    if source.stride != destination.stride || source.width != destination.width || source.height != destination.height { fatal(); }
    let affine = unsafe { coefficients(job) };
    let pitch = source.stride >> 1;
    let mut threshold = (source.width.wrapping_shr(1).wrapping_sub(1).wrapping_add(pitch.wrapping_mul(7))).wrapping_mul(2);
    for row in 0..source.height.wrapping_add(7).wrapping_shr(3) {
        let top = row.wrapping_mul(8) as i32;
        let right = source.width.wrapping_shr(1).wrapping_sub(1) as i32;
        let p0 = affine.map(top, 0);
        let p1 = affine.map(top, right);
        let offset = |p: (i32, i32)| (p.0.wrapping_sub(4) as u32).wrapping_mul(pitch).wrapping_add((p.1.max(0) as u32).min(pitch)).wrapping_mul(2);
        let earliest = offset(p0).min(offset(p1));
        if source.address.wrapping_sub(destination.address).wrapping_add(earliest) <= threshold { return 0; }
        threshold = threshold.wrapping_add(pitch.wrapping_mul(16));
    }
    1
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_prepare_affine(job: u32, source: *const ImagePlane, destination: *mut ImagePlane) -> i32 {
    let source = unsafe { read_volatile(source) };
    let mut output = unsafe { read_volatile(destination) };
    output.address = output.address.wrapping_add(output.stride.wrapping_mul(9));
    output.width = source.width;
    output.height = source.height;
    output.length = source.height.wrapping_mul(source.stride);
    let affine = unsafe { coefficients(job) };
    let mut configuration = ScaleConfiguration { block: 0, coefficients: &affine };
    if unsafe { xm1_pipeline_image_select_affine_block(job, job.wrapping_add(0x2fcc) as *const i32, &mut configuration.block) } == 0 { fatal(); }
    if unsafe { xm1_pipeline_image_affine_copy_safe(job, job.wrapping_add(0x2fcc) as *const i32, &source, &output) } == 0 { fatal(); }
    unsafe { xm1_pipeline_image_pad_edges(source.address, source.stride, source.width, source.height, 0, 2, 0, 0); xm1_pipeline_image_scale_configuration(job, &configuration, &source, &mut output); write_volatile(destination, output); }
    let mode = unsafe { Address32(job).offset(ByteOffset(0x4b4)).word() };
    let kind = if mode == 0x19 || (mode == 1 && unsafe { Address32(job).offset(ByteOffset(0x710)).word() } & 0x400 != 0) { 0x1b } else { 0x24 };
    unsafe { xm1_pipeline_image_publish_debug_file(job, kind, destination, 0, 0) };
    1
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_shear_plane(job: u32, source: *mut ImagePlane, destination: *const ImagePlane, slope: *mut i32) -> i32 {
    let selector = unsafe { xm1_buffers_configuration_byte(0x9fdf) };
    let block = if selector == 0 { 64 } else if unsafe { xm1_buffers_configuration_byte(0x9fdf) } == 1 { 32 } else { 16 };
    let state = unsafe { motion_state(job) };
    let vertical = unsafe { state.offset(ByteOffset(0x100)).word() } as i32;
    let denominator = unsafe { state.offset(ByteOffset(0xfc)).word() }.wrapping_add(unsafe { state.offset(ByteOffset(0x48)).word() } >> 1);
    let unsigned = divide_unsigned(abs_bits(vertical).wrapping_shl(16), denominator);
    unsafe { write_volatile(slope, unsigned as i32) };
    let negative = (unsafe { motion_state(job).offset(ByteOffset(0x100)).word() } as i32) < 0;
    if negative { unsafe { write_volatile(slope, (unsigned as i32).wrapping_neg()) }; }
    let slope = unsafe { read_volatile(slope) };
    let original = unsafe { read_volatile(source) };
    let product = (original.width >> 1).wrapping_mul(abs_bits(slope)) as i32;
    let rounded = product.wrapping_add(0x8000);
    let margin = (if rounded < 0 { rounded.wrapping_add(0xffff) } else { rounded } >> 16).wrapping_add(2) as u32;
    let affine = AffineCoefficients { xx: Fixed16(65536), xy: Fixed16(0), reserved0: [0; 2], x_bias: Fixed16(0), yx: Fixed16(slope), yy: Fixed16(65536), reserved1: [0; 2], y_bias: Fixed16(0), origin_x: 0, origin_y: margin as i32, offset_x: 0, offset_y: 0 };
    let configuration = ScaleConfiguration { block, coefficients: &affine };
    let mut output = unsafe { read_volatile(destination) };
    let height = original.height.wrapping_sub(margin.wrapping_mul(2));
    output.width = original.width;
    output.length = original.width.wrapping_mul(height);
    output.address = output.address.wrapping_add(output.stride.wrapping_mul(margin));
    output.height = height & !7;
    unsafe { xm1_pipeline_image_scale_configuration(job, &configuration, &original, &mut output) };
    let mut replacement = unsafe { read_volatile(destination) };
    replacement.width = original.width;
    unsafe { write_volatile(source, replacement) };
    1
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_rotate_plane(job: u32, source: *const ImagePlane, destination: *mut ImagePlane, rotation: u32) -> i32 {
    let original = unsafe { read_volatile(destination) };
    let mut output = original;
    let mode = match rotation { 1..=3 => rotation, _ => 0 };
    unsafe { Address32(0x0157_0680).write_word(mode) };
    let (width, height) = if rotation == 1 || rotation == 3 {
        let input = unsafe { read_volatile(source) };
        output.width = input.width;
        output.height = input.height;
        (input.height.wrapping_mul(2), input.width >> 1)
    } else { (original.width, original.height) };
    let result = unsafe { xm1_pipeline_image_copy_configuration(job, 0x0157_0680 as *const u32, source, &mut output) };
    output = original;
    output.width = width;
    output.height = height;
    unsafe { write_volatile(destination, output) };
    result
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_copy_plane(job: u32, source: *mut ImagePlane, destination: *const ImagePlane) -> i32 {
    let input = unsafe { read_volatile(source) };
    let mut output = unsafe { read_volatile(destination) };
    output.width = input.width;
    output.height = input.height;
    output.length = input.height.wrapping_mul(output.stride);
    let coefficients = [256, 0, 0, 0];
    let operation = PlaneOperation { kind: 7, argument: 0, table: 0, coefficients: coefficients.as_ptr(), kind8: 0, kind9: 0, parameters: 0 };
    let sources = [source as *const ImagePlane, source as *const ImagePlane, core::ptr::null(), core::ptr::null(), core::ptr::null()];
    unsafe { xm1_pipeline_image_operate(job, &operation, sources.as_ptr(), 2, &mut output); write_volatile(source, output); }
    1
}

#[repr(C)]
pub struct MotionRegion { pub address: u32, pub stride: u32, pub width: u32, pub height: u32 }
#[repr(C)]
pub struct MeshRequest { pub count: u32, pub band: u32, pub x: i32, pub y: i32, pub bounds: [i32; 4], pub first: u32, pub second: u32, pub valid: u32, pub points: u32, pub mapped: u32, pub triangles: u32 }
#[repr(C)]
pub struct MotionCandidateRequest { pub count: u32, pub band: u32, pub valid: u32, pub first: u32, pub second: u32, pub x: i32, pub y: i32 }

struct MeshOrigin { x: i32, y: i32 }
struct MeshPointCount(i32);
struct MeshTriangleCount(i32);

enum MeshOutcome {
    InsufficientPoints { retained: MeshPointCount },
    Completed { origin: MeshOrigin, retained_with_corners: MeshPointCount, triangles: MeshTriangleCount },
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_reduce_pair(job: u32, source: *const ImagePlane, destination: *mut ImagePlane, workspace: *const ImagePlane, auxiliary: *const ImagePlane) -> i32 {
    let task = unsafe { current_task() };
    unsafe { configure_priority(task, TaskPriority(13)) };
    let threshold = unsafe { xm1_buffers_configuration_byte(0x9fa5) } as i32;
    let state = unsafe { motion_state(job) };
    let input = unsafe { read_volatile(source) };
    let output = unsafe { read_volatile(destination) };
    let input = MotionRegion { address: input.address & !0x40000000, stride: input.stride, width: unsafe { state.offset(ByteOffset(0x104)).word() }.wrapping_mul(2), height: input.height };
    let mut output = MotionRegion { address: output.address, stride: output.stride, width: output.width, height: output.height };
    let auxiliary = MotionRegion { address: unsafe { read_volatile(core::ptr::addr_of!((*auxiliary).address)) } & !0x40000000, stride: ((unsafe { (xm1_buffers_configuration_half(0x9f12) as u32) } >> 1 & 0xffff) + 240).wrapping_mul(4), width: ((unsafe { (xm1_buffers_configuration_half(0x9f12) as u32) } >> 1 & 0xffff) + 240).wrapping_mul(4), height: unsafe { (xm1_buffers_configuration_half(0x9f12) as u32) } };
    let mut request = MeshRequest { count: unsafe { state.offset(ByteOffset(0x118)).word() }, band: unsafe { state.offset(ByteOffset(0x11c)).word() }, x: unsafe { state.offset(ByteOffset(0xfc)).word() } as i32, y: unsafe { state.offset(ByteOffset(0x100)).word() } as i32, bounds: [0; 4], first: state.0.wrapping_add(0x134), second: state.0.wrapping_add(0x8134), valid: state.0.wrapping_add(0x10134), points: state.0.wrapping_add(0x11134), mapped: state.0.wrapping_add(0x1192c), triangles: state.0.wrapping_add(0x12124) };
    unsafe { image_pixels_cache() };
    let outcome = unsafe { warp_mesh(&input, &mut output, read_volatile(core::ptr::addr_of!((*workspace).address)) & !0x40000000, &auxiliary, &mut request) };
    unsafe { write_volatile(core::ptr::addr_of_mut!((*destination).width), output.width); write_volatile(core::ptr::addr_of_mut!((*destination).height), output.height); }
    let accepted = match outcome {
        MeshOutcome::InsufficientPoints { retained } => {
            unsafe {
                motion_state(job).offset(ByteOffset(0x128)).write_word(retained.0 as u32);
                motion_state(job).offset(ByteOffset(0x12c)).write_word(0);
            }
            false
        }
        MeshOutcome::Completed { origin, retained_with_corners, triangles } => {
            unsafe {
                motion_state(job).offset(ByteOffset(0x120)).write_word(origin.x as u32);
                motion_state(job).offset(ByteOffset(0x124)).write_word(origin.y as u32);
                motion_state(job).offset(ByteOffset(0x128)).write_word(retained_with_corners.0 as u32);
                motion_state(job).offset(ByteOffset(0x12c)).write_word(triangles.0 as u32);
            }
            threshold <= retained_with_corners.0
        }
    };
    unsafe { image_pixels_cache(); configure_priority(task, TaskPriority(6)); }
    accepted as i32
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_reduce_pair_alternate(job: u32, first: *const u32, second: *const u32) -> i32 {
    let rows = unsafe { xm1_buffers_configuration_byte(0x9fb6) };
    let columns = unsafe { xm1_buffers_configuration_byte(0x9fb7) }.wrapping_add(1);
    let candidates = rows.wrapping_mul(columns).wrapping_add(columns);
    let high = unsafe { xm1_buffers_configuration_byte(0x9fe5) };
    let limit = (unsafe { xm1_buffers_configuration_byte(0x9fe4) } | high.wrapping_shl(8)).min(candidates);
    let state = unsafe { motion_state(job) };
    let mut request = MotionCandidateRequest { count: 0, band: 0, valid: state.0.wrapping_add(0x10134), first: state.0.wrapping_add(0x134), second: state.0.wrapping_add(0x8134), x: unsafe { state.offset(ByteOffset(0xfc)).word() } as i32, y: unsafe { state.offset(ByteOffset(0x100)).word() } as i32 };
    for index in 0..candidates {
        let a = unsafe { read_volatile(first.add(index as usize)) };
        let b = unsafe { read_volatile(second.add(index as usize)) };
        let invalid = a >> 16 == 65535 || b >> 16 == 65535;
        if invalid || b >> 16 <= unsafe { motion_state(job).offset(ByteOffset(0x108)).word() }.wrapping_mul(8) {
            let packed = if invalid { [65535; 4] } else { [(a >> 16).wrapping_add(4) >> 3, (a & 65535).wrapping_add(4) >> 3, (b >> 16).wrapping_add(4) >> 3, (b & 65535).wrapping_add(4) >> 3] };
            for lane in 0..2 { unsafe { Address32(request.first).offset(ByteOffset(request.count.wrapping_mul(8) + lane * 4)).write_word(packed[lane as usize]); Address32(request.second).offset(ByteOffset(request.count.wrapping_mul(8) + lane * 4)).write_word(packed[2 + lane as usize]); } }
            unsafe { Address32(request.valid).offset(ByteOffset(request.count)).write_byte((!invalid) as u8) };
            request.count = request.count.wrapping_add(1);
        }
        if request.count >= limit { break; }
    }
    unsafe { motion_state(job).offset(ByteOffset(0x118)).write_word(request.count) };
    let result = unsafe { xm1_pipeline_image_filter_motion_candidates(&mut request) };
    let threshold = unsafe { xm1_buffers_configuration_byte(0x9fa5) } as i32;
    if result < threshold { return 0; }
    unsafe { xm1_pipeline_image_select_motion_band(&mut request); motion_state(job).offset(ByteOffset(0x11c)).write_word(request.band); motion_state(job).offset(ByteOffset(0xfc)).write_word(request.x as u32); motion_state(job).offset(ByteOffset(0x100)).write_word(request.y as u32); }
    1
}

unsafe fn motion_point(address: u32, index: u32) -> (i32, i32) {
    let address = Address32(address).offset(ByteOffset(index.wrapping_mul(8)));
    (unsafe { address.word() } as i32, unsafe { address.offset(ByteOffset(4)).word() } as i32)
}

unsafe fn motion_valid(request: &MotionCandidateRequest, index: u32) -> bool {
    (unsafe { Address32(request.valid).offset(ByteOffset(index)).byte() }) != 0
}

unsafe fn reject_motion(request: &MotionCandidateRequest, index: u32) {
    unsafe { Address32(request.valid).offset(ByteOffset(index)).write_byte(0) };
}

fn signed_word_quotient(numerator: i32, denominator: i32) -> i32 {
    if denominator == 0 {
        let value = if numerator < 0 { i32::MIN } else if numerator > 0 { i32::MAX } else { 0 };
        unsafe { image_pixels_arithmetic_fault(value as u32) };
    }
    numerator.wrapping_div(denominator)
}

#[no_mangle]
pub extern "C" fn xm1_pipeline_image_motion_sqrt(value: i32, fraction_bits: u32) -> i32 {
    if value <= 0 { return 0; }
    let shift = fraction_bits.wrapping_mul(2) & 255;
    let scaled = if shift >= 32 { 0 } else { value.wrapping_shl(shift) };
    let mut root = 1i32;
    if scaled > 1 {
        let mut bound = scaled;
        loop {
            root = root.wrapping_mul(2);
            bound >>= 1;
            if root >= bound { break; }
        }
    }
    loop {
        let next = signed_word_quotient(scaled, root).wrapping_add(root) >> 1;
        if root <= next { return root; }
        root = next;
    }
}

unsafe fn motion_delta(request: &MotionCandidateRequest, index: u32) -> (i32, i32) {
    let a = unsafe { motion_point(request.first, index) };
    let b = unsafe { motion_point(request.second, index) };
    (a.0.wrapping_sub(b.0), a.1.wrapping_sub(b.1))
}

fn motion_distance_squared(delta: (i32, i32)) -> i32 {
    delta.0.wrapping_mul(delta.0).wrapping_add(delta.1.wrapping_mul(delta.1))
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_filter_motion_candidates(request: *mut MotionCandidateRequest) -> i32 {
    let request = unsafe { &mut *request };
    let count = request.count as i32;
    if count <= 0 { return 0; }
    for index in 0..count as u32 {
        if !unsafe { motion_valid(request, index) } { continue; }
        let a = unsafe { motion_point(request.first, index) };
        let b = unsafe { motion_point(request.second, index) };
        for later in index + 1..count as u32 {
            if unsafe { motion_point(request.first, later) } == a || unsafe { motion_point(request.second, later) } == b {
                unsafe { reject_motion(request, later) };
            }
        }
    }
    let mut slopes = 0i32;
    let mut sum = 0i32;
    let mut sum_squares = 0i64;
    let mut vertical = false;
    for index in 0..count as u32 {
        if !unsafe { motion_valid(request, index) } { continue; }
        let delta = unsafe { motion_delta(request, index) };
        if delta.0 == 0 { vertical = true; continue; }
        slopes = slopes.wrapping_add(1);
        let slope = signed_word_quotient(delta.1.wrapping_shl(8), delta.0);
        sum = sum.wrapping_add(slope);
        sum_squares = sum_squares.wrapping_add(slope.wrapping_mul(slope) as i64);
    }
    if slopes == 0 && !vertical { return 0; }
    if slopes != 0 {
        let mean = signed_word_quotient(sum, slopes);
        let variance = (divide_signed(sum_squares, slopes as i64) as i32).wrapping_sub(mean.wrapping_mul(mean));
        let deviation = xm1_pipeline_image_motion_sqrt(variance, 0);
        let factor = unsafe { xm1_buffers_configuration_byte(0x9fa6) } as i32;
        let tolerance = deviation.wrapping_mul(factor) / 64;
        for index in 0..count as u32 {
            if !unsafe { motion_valid(request, index) } { continue; }
            let delta = unsafe { motion_delta(request, index) };
            if delta.0 != 0 {
                let slope = signed_word_quotient(delta.1.wrapping_shl(8), delta.0);
                if slope > mean.wrapping_add(tolerance) || slope < mean.wrapping_sub(tolerance) {
                    unsafe { reject_motion(request, index) };
                }
            }
        }
    }
    let mut retained = 0i32;
    let mut lengths = 0i32;
    let mut squared_lengths = 0i64;
    for index in 0..count as u32 {
        if !unsafe { motion_valid(request, index) } { continue; }
        let squared = motion_distance_squared(unsafe { motion_delta(request, index) });
        retained = retained.wrapping_add(1);
        squared_lengths = squared_lengths.wrapping_add(squared as i64);
        lengths = lengths.wrapping_add(xm1_pipeline_image_motion_sqrt(squared, 0));
    }
    if retained == 0 { return 0; }
    let mean = signed_word_quotient(lengths, retained);
    let variance = (divide_signed(squared_lengths, retained as i64) as i32).wrapping_sub(mean.wrapping_mul(mean));
    let deviation = xm1_pipeline_image_motion_sqrt(variance, 0);
    let factor = unsafe { xm1_buffers_configuration_byte(0x9fa6) } as i32;
    let tolerance = deviation.wrapping_mul(factor) / 64;
    retained = 0;
    for index in 0..count as u32 {
        if !unsafe { motion_valid(request, index) } { continue; }
        let distance = xm1_pipeline_image_motion_sqrt(motion_distance_squared(unsafe { motion_delta(request, index) }), 0);
        if distance > mean.wrapping_add(tolerance) || distance < mean.wrapping_sub(tolerance) {
            unsafe { reject_motion(request, index) };
        } else {
            retained = retained.wrapping_add(1);
        }
    }
    retained
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_select_motion_band(request: *mut MotionCandidateRequest) {
    let request = unsafe { &mut *request };
    let extent = unsafe { (xm1_buffers_configuration_half(0x9f12) as u32) } as i32;
    let width = unsafe { xm1_buffers_configuration_byte(0x9faa) } as i32;
    let mut bound = extent.wrapping_sub(width);
    let mut cursor = 0i32;
    let mut selected = 0i32;
    let mut population = 0i32;
    while cursor < bound {
        let mut matches = 0i32;
        for index in 0..(request.count as i32).max(0) as u32 {
            if !unsafe { motion_valid(request, index) } { continue; }
            let point = unsafe { motion_point(request.first, index) };
            if point.0 < cursor { continue; }
            let width = unsafe { xm1_buffers_configuration_byte(0x9faa) } as i32;
            if point.0 < cursor.wrapping_add(width) { matches = matches.wrapping_add(1); }
        }
        if matches > population { population = matches; selected = cursor; }
        let step = (unsafe { xm1_buffers_configuration_byte(0x9faa) } >> 2) & 255;
        cursor = cursor.wrapping_add(step as i32);
        let extent = unsafe { (xm1_buffers_configuration_half(0x9f12) as u32) } as i32;
        let width = unsafe { xm1_buffers_configuration_byte(0x9faa) } as i32;
        bound = extent.wrapping_sub(width);
    }
    let x = request.x;
    let y = request.y;
    let positive_y = y.max(0);
    let negative_y = if y <= 0 { y.wrapping_neg() } else { 0 };
    selected = selected.wrapping_add(x);
    request.band = 0;
    request.y = 0;
    request.x = 0;
    for index in 0..(request.count as i32).max(0) as u32 {
        if !unsafe { motion_valid(request, index) } { continue; }
        let a = unsafe { motion_point(request.first, index) };
        let b = unsafe { motion_point(request.second, index) };
        let shifted_x = x.wrapping_add(a.0);
        let shifted_y = a.1.wrapping_add(positive_y);
        let shifted_other_y = b.1.wrapping_add(negative_y);
        let first = Address32(request.first).offset(ByteOffset(index.wrapping_mul(8)));
        let second = Address32(request.second).offset(ByteOffset(index.wrapping_mul(8)));
        unsafe { first.write_word(shifted_x as u32); first.offset(ByteOffset(4)).write_word(shifted_y as u32); second.offset(ByteOffset(4)).write_word(shifted_other_y as u32); }
        if shifted_x < selected { continue; }
        let width = unsafe { xm1_buffers_configuration_byte(0x9faa) } as i32;
        if shifted_x >= selected.wrapping_add(width) { continue; }
        request.x = request.x.wrapping_add(shifted_x.wrapping_sub(b.0));
        request.y = request.y.wrapping_add(shifted_y.wrapping_sub(shifted_other_y));
        request.band = request.band.wrapping_add(b.0 as u32);
    }
    let population = if population == 0 { 1 } else { population };
    request.y = signed_word_quotient(request.y, population);
    let mean = signed_word_quotient(request.x, population);
    let rounded = mean.wrapping_add(4);
    request.x = (if rounded < 0 { mean.wrapping_add(11) } else { rounded }) & !7;
    let mean = signed_word_quotient(request.band as i32, population).wrapping_add(request.x);
    let rounded = mean.wrapping_add(4);
    request.band = ((if rounded < 0 { mean.wrapping_add(11) } else { rounded }) & !7) as u32;
}

#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct MeshPoint { pub x: i32, pub y: i32 }

impl MeshPoint {
    unsafe fn load(address: u32, index: u32) -> Self {
        let (x, y) = unsafe { motion_point(address, index) };
        Self { x, y }
    }

    unsafe fn store(self, address: u32, index: u32) {
        let address = Address32(address).offset(ByteOffset(index.wrapping_mul(8)));
        unsafe { address.write_word(self.x as u32); address.offset(ByteOffset(4)).write_word(self.y as u32); }
    }
}

#[derive(Clone, Copy)]
#[repr(C)]
pub struct MeshAffineRecord {
    pub triangle: u32,
    pub xx: i32,
    pub xy: i32,
    pub bias_x: i32,
    pub yx: i32,
    pub yy: i32,
    pub bias_y: i32,
}

impl MeshAffineRecord {
    fn identity(triangle: u32) -> Self {
        Self { triangle, xx: 65536, xy: 0, bias_x: 0, yx: 0, yy: 65536, bias_y: 0 }
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_fit_mesh_triangle(ax: i32, ay: i32, bx: i32, by: i32, cx: i32, cy: i32, sx: i32, sy: i32, tx: i32, ty: i32, ux: i32, uy: i32, result: *mut MeshAffineRecord) -> i32 {
    let abx = bx.wrapping_sub(ax);
    let aby = by.wrapping_sub(ay);
    let acx = cx.wrapping_sub(ax);
    let acy = cy.wrapping_sub(ay);
    let mut determinant = acy.wrapping_mul(abx).wrapping_sub(acx.wrapping_mul(aby));
    if determinant.wrapping_add(31) as u32 <= 62 { return 0; }
    let mut numerators = [
        tx.wrapping_sub(sx).wrapping_mul(acy).wrapping_sub(aby.wrapping_mul(ux.wrapping_sub(sx))),
        acx.wrapping_neg().wrapping_mul(tx.wrapping_sub(sx)).wrapping_add(ux.wrapping_sub(sx).wrapping_mul(abx)),
        ty.wrapping_sub(sy).wrapping_mul(acy).wrapping_sub(aby.wrapping_mul(uy.wrapping_sub(sy))),
        acx.wrapping_neg().wrapping_mul(ty.wrapping_sub(sy)).wrapping_add(uy.wrapping_sub(sy).wrapping_mul(abx)),
    ];
    if determinant < 0 {
        determinant = determinant.wrapping_neg();
        for numerator in &mut numerators { *numerator = numerator.wrapping_neg(); }
    }
    let limit = determinant.wrapping_mul(16);
    let half = (determinant >> 1) as i64;
    let mut coefficients = [0i32; 4];
    for (coefficient, numerator) in coefficients.iter_mut().zip(numerators) {
        if numerator.wrapping_abs() > limit { return 0; }
        let rounded = (numerator as i64).wrapping_mul(65536).wrapping_add(if numerator < 0 { -half } else { half });
        *coefficient = divide_signed(rounded, determinant as i64) as i32;
    }
    let [xx, xy, yx, yy] = coefficients;
    let bias_x = (sx.wrapping_shl(16) as i64).wrapping_sub((xx as i64).wrapping_mul(ax as i64)).wrapping_sub((xy as i64).wrapping_mul(ay as i64));
    if bias_x.unsigned_abs() > 0x40000000 { return 0; }
    let bias_y = (sy.wrapping_shl(16) as i64).wrapping_sub((yx as i64).wrapping_mul(ax as i64)).wrapping_sub((yy as i64).wrapping_mul(ay as i64));
    if bias_y.unsigned_abs() > 0x40000000 { return 0; }
    unsafe {
        write_volatile(core::ptr::addr_of_mut!((*result).xx), xx);
        write_volatile(core::ptr::addr_of_mut!((*result).xy), xy);
        write_volatile(core::ptr::addr_of_mut!((*result).bias_x), bias_x as i32);
        write_volatile(core::ptr::addr_of_mut!((*result).yx), yx);
        write_volatile(core::ptr::addr_of_mut!((*result).yy), yy);
        write_volatile(core::ptr::addr_of_mut!((*result).bias_y), bias_y as i32);
    }
    1
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_mesh_circle(ax: i32, ay: i32, bx: i32, by: i32, cx: i32, cy: i32, center: *mut MeshPoint, radius: *mut i32, radius_squared: *mut i32) -> i32 {
    if (ax == bx && ax == cx) || (ay == by && ay == cy) { return 0; }
    let origin_x = ax.min(bx).min(cx);
    let origin_y = ay.min(by).min(cy);
    let a = MeshPoint { x: ax.wrapping_sub(origin_x), y: ay.wrapping_sub(origin_y) };
    let b = MeshPoint { x: bx.wrapping_sub(origin_x), y: by.wrapping_sub(origin_y) };
    let c = MeshPoint { x: cx.wrapping_sub(origin_x), y: cy.wrapping_sub(origin_y) };
    let (p, q, r, s) = if a.y == b.y { (b, c, c, a) } else if b.y == c.y { (a, b, a, c) } else { (a, b, b, c) };
    let slope = |first: MeshPoint, second: MeshPoint| signed_word_quotient(first.x.wrapping_mul(2).wrapping_sub(second.x.wrapping_mul(2)).wrapping_mul(16), second.y.wrapping_mul(2).wrapping_sub(first.y.wrapping_mul(2)));
    let first_slope = slope(p, q);
    let second_slope = slope(r, s);
    if first_slope == second_slope { return 0; }
    let intercept = |first: MeshPoint, second: MeshPoint, slope: i32| {
        let sum_y = first.y.wrapping_mul(2).wrapping_add(second.y.wrapping_mul(2)) >> 1;
        let sum_x = first.x.wrapping_mul(2).wrapping_add(second.x.wrapping_mul(2)) >> 1;
        sum_y.wrapping_mul(16).wrapping_sub(slope.wrapping_mul(sum_x)) >> 1
    };
    let first_intercept = intercept(p, q, first_slope);
    let second_intercept = intercept(r, s, second_slope);
    let denominator = first_slope.wrapping_sub(second_slope);
    let x = signed_word_quotient(second_intercept.wrapping_sub(first_intercept).wrapping_mul(16), denominator).wrapping_add(8) >> 4;
    let y = signed_word_quotient(first_slope.wrapping_mul(second_intercept).wrapping_sub(second_slope.wrapping_mul(first_intercept)), denominator).wrapping_add(8) >> 4;
    let squared = motion_distance_squared((a.x.wrapping_sub(x), a.y.wrapping_sub(y)));
    unsafe { write_volatile(center, MeshPoint { x, y }); write_volatile(radius_squared, squared); }
    let length = xm1_pipeline_image_motion_sqrt(squared, 0);
    unsafe { write_volatile(radius, length); write_volatile(center, MeshPoint { x: x.wrapping_add(origin_x), y: y.wrapping_add(origin_y) }); }
    1
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_mesh_transform_point(result: *mut MeshPoint, records: *const MeshAffineRecord, triangle: u32, x: i32, y: i32, count: i32) {
    if triangle != 0 {
        let limit = if count < 0 { 1 } else { count };
        for index in 0..limit {
            let record = unsafe { read_volatile(records.add(index as usize)) };
            if count >= 0 && record.triangle != triangle { continue; }
            let mapped_x = (record.xx as i64).wrapping_mul(x as i64).wrapping_add((record.xy as i64).wrapping_mul(y as i64)).wrapping_add(record.bias_x as i64);
            let mapped_y = (record.yx as i64).wrapping_mul(x as i64).wrapping_add((record.yy as i64).wrapping_mul(y as i64)).wrapping_add(record.bias_y as i64);
            unsafe { write_volatile(result, MeshPoint { x: (mapped_x >> 16) as i32, y: (mapped_y >> 16) as i32 }) };
            return;
        }
    }
    unsafe { write_volatile(result, MeshPoint { x, y }) };
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_build_mesh_affines(points: u32, mapped: u32, triangles: u32, workspace: *mut MeshAffineRecord, count: i32) {
    for index in 0..count.max(0) as u32 {
        let triangle = Address32(triangles).offset(ByteOffset(index.wrapping_mul(16)));
        let id = unsafe { triangle.word() };
        let ia = unsafe { triangle.offset(ByteOffset(4)).word() };
        let ib = unsafe { triangle.offset(ByteOffset(8)).word() };
        let ic = unsafe { triangle.offset(ByteOffset(12)).word() };
        let a = unsafe { MeshPoint::load(mapped, ia) };
        let b = unsafe { MeshPoint::load(mapped, ib) };
        let c = unsafe { MeshPoint::load(mapped, ic) };
        let sa = unsafe { MeshPoint::load(points, ia) };
        let sb = unsafe { MeshPoint::load(points, ib) };
        let sc = unsafe { MeshPoint::load(points, ic) };
        let output = unsafe { workspace.add(index as usize) };
        unsafe { write_volatile(output, MeshAffineRecord::identity(id)); xm1_pipeline_image_fit_mesh_triangle(a.x, a.y, b.x, b.y, c.x, c.y, sa.x, sa.y, sb.x, sb.y, sc.x, sc.y, output); }
    }
}

fn mesh_edge(a: MeshPoint, b: MeshPoint, p: MeshPoint) -> i32 {
    b.x.wrapping_sub(a.x).wrapping_mul(p.y.wrapping_sub(a.y)).wrapping_sub(b.y.wrapping_sub(a.y).wrapping_mul(p.x.wrapping_sub(a.x)))
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_draw_mesh_labels(points: u32, triangles: u32, labels: u32, left: i32, top: i32, width: i32, _height: i32, count: i32) {
    let pitch = ((unsafe { (xm1_buffers_configuration_half(0x9f12) as u32) } >> 1 & 65535).wrapping_add(240)).wrapping_mul(4);
    for index in 0..count.max(0) as u32 {
        let triangle = Address32(triangles).offset(ByteOffset(index.wrapping_mul(16)));
        let id = unsafe { triangle.word() };
        let a = unsafe { MeshPoint::load(points, triangle.offset(ByteOffset(4)).word()) };
        let b = unsafe { MeshPoint::load(points, triangle.offset(ByteOffset(8)).word()) };
        let c = unsafe { MeshPoint::load(points, triangle.offset(ByteOffset(12)).word()) };
        let area = (mesh_edge(a, b, c) / 2).wrapping_abs();
        if area == 0 { continue; }
        let minimum = a.x.min(b.x).min(c.x).wrapping_sub(1);
        let start = minimum.wrapping_add(minimum % 2);
        let last = a.x.max(b.x).max(c.x);
        let mut y = a.y.min(b.y).min(c.y);
        let end_y = a.y.max(b.y).max(c.y);
        while y <= end_y {
            let mut x = start;
            let mut started = false;
            while x <= last && x.wrapping_add(1) < left.wrapping_add(width) {
                let address = Address32(labels).offset(ByteOffset((y.wrapping_sub(top) as u32).wrapping_mul(pitch).wrapping_add((x.wrapping_sub(left) as u32).wrapping_mul(4))));
                if unsafe { address.word() } == 0 {
                    let p = MeshPoint { x, y };
                    let first = (mesh_edge(a, b, p) / 2).wrapping_abs();
                    let second = (mesh_edge(a, c, p) / 2).wrapping_abs();
                    let third = (mesh_edge(b, c, p) / 2).wrapping_abs();
                    let sum = first.wrapping_add(second).wrapping_add(third);
                    if first <= area && second <= area && third <= area && (sum == area || sum.wrapping_add(1) == area) {
                        unsafe { address.write_word(id); address.offset(ByteOffset(4)).write_word(id); }
                        started = true;
                    } else if started { break; }
                }
                x = x.wrapping_add(2);
            }
            y = y.wrapping_add(1);
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_mesh_warp(source: *const MotionRegion, destination: *mut MotionRegion, workspace: u32, auxiliary: *const MotionRegion, request: *mut MeshRequest) -> i32 {
    match unsafe { warp_mesh(source, destination, workspace, auxiliary, request) } {
        MeshOutcome::InsufficientPoints { retained } => retained.0,
        MeshOutcome::Completed { retained_with_corners, .. } => retained_with_corners.0,
    }
}

unsafe fn warp_mesh(source: *const MotionRegion, destination: *mut MotionRegion, workspace: u32, auxiliary: *const MotionRegion, request: *mut MeshRequest) -> MeshOutcome {
    let configured_height = unsafe { (xm1_buffers_configuration_half(0x9f12) as u32) };
    unsafe { (xm1_buffers_configuration_half(0x9f12) as u32) };
    let auxiliary = unsafe { &*auxiliary };
    let request = unsafe { &mut *request };
    request.bounds[2] = 0;
    request.bounds[3] = 0;
    for offset in 0..auxiliary.width.wrapping_mul(auxiliary.height) { unsafe { Address32(auxiliary.address).offset(ByteOffset(offset)).write_byte(0) }; }
    let band = request.band;
    let span = unsafe { (xm1_buffers_configuration_half(0x9fa7) as u32) };
    let span_right = unsafe { (xm1_buffers_configuration_half(0x9fa7) as u32) };
    let source = unsafe { &*source };
    let candidate_right = band.wrapping_add((span_right >> 1) & 65535);
    let right = if candidate_right >= source.width >> 1 { (source.width >> 1).wrapping_sub(1) } else { candidate_right };
    let bottom = source.height.wrapping_sub(1) as i32;
    let margin = unsafe { xm1_buffers_configuration_byte(0x9fa9) };
    let output_height = unsafe { (xm1_buffers_configuration_half(0x9f12) as u32) };
    for index in 0..(request.count as i32).max(0) as u32 {
        if unsafe { Address32(request.valid).offset(ByteOffset(index)).byte() } == 0 { continue; }
        let first = unsafe { MeshPoint::load(request.first, index) };
        let second = unsafe { MeshPoint::load(request.second, index) };
        let low = (request.band as i32).wrapping_sub(((unsafe { xm1_buffers_configuration_byte(0x9faa) } >> 1) & 255) as i32);
        if first.x < low { continue; }
        let high = (request.band as i32).wrapping_add(((unsafe { xm1_buffers_configuration_byte(0x9faa) } >> 1) & 255) as i32);
        if first.x > high || first.y < 0 || first.y > bottom { continue; }
        let retained = request.bounds[2] as u32;
        unsafe { first.store(request.points, retained); MeshPoint { x: second.x.wrapping_add(request.x), y: second.y.wrapping_add(request.y) }.store(request.mapped, retained); }
        request.bounds[2] = request.bounds[2].wrapping_add(1);
        let maximum = unsafe { xm1_buffers_configuration_byte(0x9fab) } as i32;
        if (retained as i32).wrapping_add(5) >= maximum { break; }
    }
    let threshold = unsafe { xm1_buffers_configuration_byte(0x9fa5) } as i32;
    if request.bounds[2] < threshold { return MeshOutcome::InsufficientPoints { retained: MeshPointCount(request.bounds[2]) }; }
    let start = (band as i32).wrapping_sub(((span >> 1) & 65535) as i32);
    let left = start.wrapping_add(7).wrapping_sub(start.wrapping_sub(1) % 8);
    let retained = request.bounds[2] as u32;
    for (offset, point) in [MeshPoint { x: left, y: 0 }, MeshPoint { x: left, y: bottom }, MeshPoint { x: right as i32, y: 0 }, MeshPoint { x: right as i32, y: bottom }].into_iter().enumerate() {
        unsafe { point.store(request.points, retained.wrapping_add(offset as u32)); point.store(request.mapped, retained.wrapping_add(offset as u32)); }
    }
    request.bounds[2] = request.bounds[2].wrapping_add(4);
    let count = request.bounds[2].max(0) as u32;
    for a in 0..count {
        for b in a + 1..count {
            for c in b + 1..count {
                let first = unsafe { MeshPoint::load(request.points, a) };
                let second = unsafe { MeshPoint::load(request.points, b) };
                let third = unsafe { MeshPoint::load(request.points, c) };
                let mut center = MeshPoint { x: 0, y: 0 };
                let mut radius = 0;
                let mut squared = 0;
                if unsafe { xm1_pipeline_image_mesh_circle(first.x, first.y, second.x, second.y, third.x, third.y, &mut center, &mut radius, &mut squared) } == 0 || radius == 0 { continue; }
                let mut inside = false;
                for other in 0..count {
                    if other == a || other == b || other == c { continue; }
                    let point = unsafe { MeshPoint::load(request.points, other) };
                    let distance = motion_distance_squared((point.x.wrapping_sub(center.x), point.y.wrapping_sub(center.y)));
                    if distance <= squared && xm1_pipeline_image_motion_sqrt(distance, 0).wrapping_add(3) < radius { inside = true; break; }
                }
                if inside { continue; }
                let triangle = Address32(request.triangles).offset(ByteOffset((request.bounds[3] as u32).wrapping_mul(16)));
                unsafe { triangle.write_word((center.x as u32).wrapping_shl(16).wrapping_add(center.y as u32)); triangle.offset(ByteOffset(4)).write_word(a); triangle.offset(ByteOffset(8)).write_word(b); triangle.offset(ByteOffset(12)).write_word(c); }
                request.bounds[3] = request.bounds[3].wrapping_add(1);
            }
        }
    }
    let width = band.wrapping_sub(left as u32).wrapping_add(margin);
    unsafe { xm1_pipeline_image_build_mesh_affines(request.points, request.mapped, request.triangles, workspace as *mut MeshAffineRecord, request.bounds[3]); xm1_pipeline_image_draw_mesh_labels(request.mapped, request.triangles, auxiliary.address, left, 0, width as i32, output_height as i32, request.bounds[3]); }
    let label_pitch = ((configured_height >> 1) & 65535).wrapping_add(240).wrapping_mul(4);
    let destination = unsafe { &mut *destination };
    let mut y = 0i32;
    while y <= bottom {
        let mut x = left;
        let mut offset = 0u32;
        while x <= right as i32 && x < left.wrapping_add(width as i32) {
            let label = unsafe { Address32(auxiliary.address).offset(ByteOffset((y as u32).wrapping_mul(label_pitch).wrapping_add(offset.wrapping_mul(2)))).word() };
            let mut point = MeshPoint { x, y };
            unsafe { xm1_pipeline_image_mesh_transform_point(&mut point, workspace as *const MeshAffineRecord, label, x, y, request.bounds[3]) };
            let sx = (point.x.max(0) as u32).min((source.width >> 1).wrapping_sub(1)) as i32;
            let sx = sx.wrapping_sub(1);
            let sx = sx.wrapping_add(sx % 2);
            let sy = (point.y.max(0) as u32).min(source.height.wrapping_sub(1));
            let value = unsafe { Address32(source.address).offset(ByteOffset((sx as u32).wrapping_mul(2).wrapping_add(source.stride.wrapping_mul(sy)))).word() };
            let address = Address32(destination.address).offset(ByteOffset(destination.stride.wrapping_mul(y as u32).wrapping_add(offset)));
            for lane in 0..4 { unsafe { address.offset(ByteOffset(lane)).write_byte((value >> (lane * 8)) as u8) }; }
            offset = offset.wrapping_add(4);
            x = x.wrapping_add(2);
        }
        y = y.wrapping_add(1);
    }
    destination.width = width.wrapping_mul(2);
    destination.height = output_height;
    request.bounds[0] = left;
    request.bounds[1] = 0;
    MeshOutcome::Completed { origin: MeshOrigin { x: left, y: 0 }, retained_with_corners: MeshPointCount(request.bounds[2]), triangles: MeshTriangleCount(request.bounds[3]) }
}
