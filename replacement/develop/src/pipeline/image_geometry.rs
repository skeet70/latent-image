use super::callbacks::{self, CallbackWord};
use core::ptr::{read_volatile, write_volatile};
use super::shared::{Address32, ByteOffset};

#[derive(Clone, Copy)]
#[repr(C)]
pub struct ImagePlane {
    pub mode: u32,
    pub address: Address32,
    pub size: u32,
    pub stride: u32,
    pub width: u32,
    pub height: u32,
    pub flags: u32,
}

#[repr(transparent)]
#[derive(Clone, Copy)]
struct ConfigurationKey(u32);

#[repr(C)]
struct CropRequest {
    byte_input: u32,
    byte_output: u32,
    transform: u32,
    stride: u32,
    enabled: u32,
    threshold: u32,
    alternate: u32,
    red: u32,
    green: u32,
    blue: u32,
    controls: [u32; 4],
}

impl CropRequest {
    const fn zero() -> Self {
        Self { byte_input: 0, byte_output: 0, transform: 0, stride: 0,
            enabled: 0, threshold: 0, alternate: 0, red: 0, green: 0, blue: 0,
            controls: [0; 4] }
    }
}

unsafe extern "C" {
    fn xm1_buffers_configuration_byte(key: ConfigurationKey) -> u32;
    fn xm1_buffers_configuration_half(key: ConfigurationKey) -> u16;
    fn xm1_pipeline_image_current_task() -> u32;
    fn xm1_pipeline_image_configure_task(task: u32, mode: u32) -> i32;
    fn xm1_pipeline_geometry_cache_whole(first: u32, second: u32);
    fn xm1_pipeline_image_process_crop(stage: u32, request: *const CropRequest, input: *const ImagePlane, output: *mut ImagePlane) -> u32;
    fn xm1_pipeline_image_process_planes(stage: u32, request: *const u32, input: *mut ImagePlane, unused: u32, output: *mut ImagePlane, auxiliary: u32, zero: u32, flags: u32) -> u32;
    fn xm1_pipeline_image_pad_edges(address: u32, stride: u32, width: u32, height: u32, left: u32, right: u32, top: u32, bottom: u32);
    fn xm1_pipeline_image_publish_debug_file(stage: u32, kind: u32, plane: *const ImagePlane, number: u32, suffix: u32);
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_image_cache_maintenance() {
    unsafe { xm1_pipeline_geometry_cache_whole(0, 0) };
}

unsafe fn cfg(key: u32) -> u32 {
    unsafe { xm1_buffers_configuration_byte(ConfigurationKey(key)) as u32 }
}

fn rejected() -> ! {
    loop { core::hint::spin_loop(); }
}

fn halve(input: &mut ImagePlane, output: &mut ImagePlane, request: &mut CropRequest) {
    input.width &= !31;
    input.height &= !1;
    output.width = input.width >> 1;
    output.height = input.height >> 1;
    request.stride = output.width.wrapping_add(31) & !31;
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_image_prepare_border(stage: u32, source: *const ImagePlane, scratch: *const ImagePlane, destination: *mut ImagePlane) -> u32 {
    let mut input = unsafe { read_volatile(source) };
    let mut output = unsafe { read_volatile(scratch) };
    let mut request = CropRequest::zero();
    halve(&mut input, &mut output, &mut request);
    for index in 0..4 { request.controls[index] = unsafe { cfg(0x49b0 + index as u32) }; }
    unsafe { xm1_pipeline_image_process_crop(stage, &request, &input, &mut output) };
    input = output;
    output = unsafe { read_volatile(destination) };
    halve(&mut input, &mut output, &mut request);
    unsafe { xm1_pipeline_image_process_crop(stage, &request, &input, &mut output) };
    unsafe { write_volatile(destination, output) };
    unsafe { xm1_pipeline_image_publish_debug_file(stage, 0x25, destination, 0, 0) };
    1
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_image_transform_planes(stage: u32, planes: *const *const ImagePlane, intermediate: *const ImagePlane, count: u32) -> u32 {
    if count != 4 { rejected(); }
    let first = unsafe { read_volatile(planes) };
    let mut output = unsafe { read_volatile(read_volatile(planes.add(1))) };
    let mut request = CropRequest::zero();
    for index in 0..3 {
        let mut input = if index == 0 { unsafe { read_volatile(first) } } else { output };
        output = unsafe { read_volatile(if index == 2 { intermediate } else { read_volatile(planes.add(index + 1)) }) };
        halve(&mut input, &mut output, &mut request);
        request.alternate = unsafe { cfg(0x49c4) == 1 } as u32;
        request.red = unsafe { cfg(0x49c5) };
        request.green = unsafe { cfg(0x49c6) };
        request.blue = unsafe { cfg(0x49c7) };
        for control in 0..4 { request.controls[control] = unsafe { cfg(0x49c8 + control as u32) }; }
        unsafe { xm1_pipeline_image_process_crop(stage, &request, &input, &mut output) };
    }
    let mut input = output;
    output = unsafe { read_volatile(read_volatile(planes.add(3))) };
    input.width &= !15;
    input.height &= !1;
    output.width = input.width;
    output.height = input.height;
    request.stride = output.width.wrapping_add(31) & !31;
    request.transform = 1;
    unsafe { xm1_pipeline_image_process_crop(stage, &request, &input, &mut output) };
    let mut debug = unsafe { read_volatile(read_volatile(planes.add(1))) };
    debug.height = unsafe { read_volatile(first) }.height.wrapping_add(1) >> 1;
    debug.size = debug.stride.wrapping_mul(debug.height);
    unsafe { xm1_pipeline_image_publish_debug_file(stage, 0x27, first, 0, 0) };
    unsafe { xm1_pipeline_image_publish_debug_file(stage, 0x28, &debug, 0, 0) };
    1
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_image_reduction_ready(stage: u32, _argument: u32, planes: *const *const ImagePlane, count: u32) -> u32 {
    if stage == 0 { return 0; }
    let frame = Address32(stage);
    let kind = unsafe { frame.offset(ByteOffset(0x4b4)).word() };
    let key = match kind {
        25 => 0x472f,
        1 if unsafe { frame.offset(ByteOffset(0x710)).word() } & 0x400 != 0 => 0x472f,
        26 => 0x493f,
        29 => 0x9fc5,
        _ => return 0,
    };
    if count.wrapping_sub(1) < unsafe { cfg(key) } { return 0; }
    for index in 0..count {
        let plane = unsafe { read_volatile(planes.add(index as usize)) };
        if plane.is_null() || unsafe { read_volatile(plane) }.address.0 == 0 { return 0; }
    }
    (unsafe { read_volatile(read_volatile(planes)) }.flags & 16 == 0) as u32
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_image_reduce_planes(stage: u32, planes: *const *const ImagePlane, count: u32, argument: u32, alternate: u32) -> u32 {
    if unsafe { xm1_pipeline_image_reduction_ready(stage, argument, planes, count) } == 0 { rejected(); }
    let kind = unsafe { Address32(stage).offset(ByteOffset(0x4b4)).word() };
    let bank = match kind { 25 | 1 => 0, 26 => 2, 29 => 4, _ => rejected() } + (alternate != 0) as u32;
    if count == 1 { return 1; }
    let keys = Address32(0xe5fb70 + bank * 36);
    let mut output = unsafe { read_volatile(read_volatile(planes)) };
    let mut request = CropRequest::zero();
    for index in 0..count.wrapping_sub(1) {
        if index == 0 {
            let key = unsafe { keys.word() };
            request.byte_input = 0;
            request.enabled = (unsafe { cfg(key) } >> 3) & 1;
            request.threshold = unsafe { xm1_buffers_configuration_half(ConfigurationKey(keys.offset(ByteOffset(4)).word())) } as u32;
            request.alternate = (unsafe { cfg(key) } >> 1) & 1;
            request.red = unsafe { cfg(keys.offset(ByteOffset(8)).word()) }.wrapping_mul(4);
            request.green = unsafe { cfg(keys.offset(ByteOffset(12)).word()) }.wrapping_mul(4);
            request.blue = unsafe { cfg(keys.offset(ByteOffset(16)).word()) }.wrapping_mul(4);
        } else {
            request.byte_input = 1;
            request.enabled = 0;
            request.threshold = 0;
            request.alternate = 0;
            request.red = 0;
            request.green = 0;
            request.blue = 0;
        }
        request.byte_output = 1;
        request.transform = 0;
        for control in 0..4 { request.controls[control] = unsafe { cfg(keys.offset(ByteOffset(20 + control as u32 * 4)).word()) }; }
        let mut input = output;
        output = unsafe { read_volatile(read_volatile(planes.add(index as usize + 1))) };
        let divisor = if request.byte_input == 0 { 2 } else { 1 };
        let logical_width = input.width / divisor;
        output.width = ((logical_width.wrapping_add(1) >> 1).wrapping_sub(1).wrapping_add(16) / 16).wrapping_mul(16);
        output.height = input.height.wrapping_add(1) >> 1;
        output.size = output.stride.wrapping_mul(output.height);
        request.stride = output.width.wrapping_add(31) & !31;
        let right = output.width.wrapping_mul(2).wrapping_sub(logical_width);
        let bottom = output.height.wrapping_mul(2).wrapping_sub(input.height);
        if ((right | bottom) as i32) < 0 { rejected(); }
        if request.byte_input == 0 {
            unsafe { xm1_pipeline_image_pad_edges(input.address.0, input.stride, input.width, input.height, 0, right, 0, bottom) };
        } else {
            unsafe { xm1_pipeline_image_pad_bytes(input.address.0, input.stride, input.width, input.height, 0, right, 0, bottom) };
        }
        input.width = input.width.wrapping_add(divisor.wrapping_mul(right));
        input.height = input.height.wrapping_add(bottom);
        unsafe { xm1_pipeline_image_process_crop(stage, &request, &input, &mut output) };
    }
    1
}

unsafe fn transfer_row(destination: Address32, source: Address32, width: u32, prefix: u32) {
    let mut index = 0;
    while index < prefix {
        unsafe { destination.offset(ByteOffset(index)).write_byte(source.offset(ByteOffset(index)).byte()) };
        index += 1;
    }
    while index < width & !3 {
        let value = unsafe { core::ptr::read_unaligned(source.offset(ByteOffset(index)).0 as *const u32) };
        unsafe { core::ptr::write_unaligned(destination.offset(ByteOffset(index)).0 as *mut u32, value) };
        index += 4;
    }
    while index < width {
        unsafe { destination.offset(ByteOffset(index)).write_byte(source.offset(ByteOffset(index)).byte()) };
        index += 1;
    }
}

unsafe fn repeat_byte(destination: Address32, value: u8, count: u32, prefix: u32) {
    let mut index = 0;
    while index < prefix { unsafe { destination.offset(ByteOffset(index)).write_byte(value) }; index += 1; }
    while index < count & !3 {
        unsafe { core::ptr::write_unaligned(destination.offset(ByteOffset(index)).0 as *mut u32, u32::from(value) * 0x01010101) };
        index += 4;
    }
    while index < count { unsafe { destination.offset(ByteOffset(index)).write_byte(value) }; index += 1; }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_image_pad_bytes(address: u32, stride: u32, width: u32, height: u32, left: u32, right: u32, top: u32, bottom: u32) {
    let task = unsafe { xm1_pipeline_image_current_task() };
    unsafe { xm1_pipeline_image_configure_task(task, 12) };
    unsafe { xm1_pipeline_image_cache_maintenance() };
    let first = Address32(address.wrapping_sub(stride.wrapping_mul(top)));
    let prefix = (first.0 & 3).min(width);
    for row in 0..top { unsafe { transfer_row(first.offset(ByteOffset(stride.wrapping_mul(row))), Address32(address), width, prefix) }; }
    let last = Address32(address.wrapping_add(stride.wrapping_mul(height.wrapping_sub(1))));
    let below = last.offset(ByteOffset(stride));
    let prefix = (below.0 & 3).min(width);
    for row in 0..bottom { unsafe { transfer_row(below.offset(ByteOffset(stride.wrapping_mul(row))), last, width, prefix) }; }
    let rows = top.wrapping_add(height).wrapping_add(bottom);
    let left_start = Address32(first.0.wrapping_sub(left));
    let prefix = (left_start.0 & 3).min(left);
    for row in 0..rows {
        let destination = left_start.offset(ByteOffset(stride.wrapping_mul(row)));
        let value = unsafe { destination.offset(ByteOffset(left)).byte() };
        unsafe { repeat_byte(destination, value, left, prefix) };
    }
    let right_start = first.offset(ByteOffset(width));
    let prefix = (right_start.0 & 3).min(right);
    for row in 0..rows {
        let destination = right_start.offset(ByteOffset(stride.wrapping_mul(row)));
        let value = unsafe { Address32(destination.0.wrapping_sub(1)).byte() };
        unsafe { repeat_byte(destination, value, right, prefix) };
    }
    unsafe { xm1_pipeline_image_cache_maintenance() };
    unsafe { xm1_pipeline_image_configure_task(task, 6) };
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_image_fill_row_borders(address: u32, stride: u32, width: u32, height: i32) {
    unsafe { xm1_pipeline_image_cache_maintenance() };
    if height > 0 {
        for row in 0..height as u32 {
            let start = Address32(address.wrapping_add((stride & !1).wrapping_mul(row)));
            unsafe { start.offset(ByteOffset(2)).write_half(0x8080) };
            unsafe { start.offset(ByteOffset((width & !1).wrapping_sub(2))).write_half(0x8080) };
        }
    }
    unsafe { xm1_pipeline_image_cache_maintenance() };
}

unsafe fn border_operation(stage: u32, source: *mut ImagePlane, destination: *const ImagePlane, auxiliary: u32, alternate: Option<bool>) -> u32 {
    if stage == 0 || source.is_null() || destination.is_null() { rejected(); }
    let request = if alternate.is_none() { Address32(0x15716b4) } else { Address32(0x1571284) };
    for word in 0..8 { unsafe { request.offset(ByteOffset(word * 4)).write_word(0) }; }
    let kind = match alternate { None => 10, Some(false) => 11, Some(true) => 12 };
    unsafe { request.write_word(kind) };
    unsafe { request.offset(ByteOffset(28)).write_word(1) };
    let mut output = unsafe { read_volatile(destination) };
    let input = unsafe { read_volatile(source) };
    if alternate.is_none() { output.address.0 = output.address.0.wrapping_add(output.stride.wrapping_mul(9)); }
    output.width = input.width;
    output.height = input.height;
    output.size = output.stride.wrapping_mul(output.height);
    if alternate.is_some() { output.flags |= 0x4000000; }
    unsafe { xm1_pipeline_image_pad_edges(input.address.0, input.stride, input.width, input.height, 8, 8, 0, 0) };
    unsafe { (*source).address.0 = (*source).address.0.wrapping_sub(16) };
    unsafe { (*source).width = (*source).width.wrapping_add(32) };
    unsafe { xm1_pipeline_image_fill_row_borders((*source).address.0, (*source).stride, (*source).width, (*source).height as i32) };
    let status = unsafe { xm1_pipeline_image_process_planes(stage, request.0 as *const u32, source, 0, &mut output, auxiliary, 0, 0) };
    if alternate.is_none() {
        unsafe { (*source).address.0 = (*source).address.0.wrapping_add(16) };
        unsafe { (*source).width = (*source).width.wrapping_sub(32) };
        unsafe { xm1_pipeline_image_publish_debug_file(stage, 0x20, &output, 0, 0) };
    }
    unsafe { write_volatile(source, output) };
    status
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_image_reduce_border(stage: u32, source: *mut ImagePlane, destination: *const ImagePlane) -> u32 {
    unsafe { border_operation(stage, source, destination, 0, None) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_image_transform_border(stage: u32, source: *mut ImagePlane, destination: *const ImagePlane, auxiliary: u32, alternate: i32) -> u32 {
    unsafe { border_operation(stage, source, destination, auxiliary, Some(alternate != 0)) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_image_buffer_views(stage: u32, source: *const ImagePlane, output: *mut ImagePlane, phase: *mut u32, enabled: *mut u16) {
    let frame = Address32(stage);
    let mut plane = unsafe { read_volatile(source) };
    let width = unsafe { frame.offset(ByteOffset(0x480)).word() };
    let height = unsafe { frame.offset(ByteOffset(0x484)).word() };
    let divisor = if plane.flags & 16 != 0 { 3 } else { 4 };
    let packed = width.wrapping_mul(divisor);
    let logical_width = plane.width.wrapping_mul(2) / divisor;
    let source_height = plane.height;
    plane.width = packed >> 1;
    plane.height = height;
    plane.address.0 = plane.address.0.wrapping_add(plane.stride.wrapping_mul(source_height >> 1))
        .wrapping_add(unsafe { read_volatile(source) }.width >> 1)
        .wrapping_sub(plane.stride.wrapping_mul(height >> 1)).wrapping_sub(packed >> 2);
    plane.size = height.wrapping_mul(packed >> 1);
    unsafe { write_volatile(output, plane) };
    unsafe { write_volatile(phase, frame.offset(ByteOffset(0x4a0)).word().wrapping_add(logical_width.wrapping_sub(width) >> 1) % 6) };
    unsafe { write_volatile(phase.add(1), frame.offset(ByteOffset(0x4a4)).word().wrapping_add(source_height.wrapping_sub(height) >> 1) % 6) };
    unsafe { write_volatile(enabled, 1) };
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_image_read_packed12(image: *const ImagePlane, x: i32, y: i32) -> u32 {
    let plane = unsafe { read_volatile(image) };
    let group = (x / 8).wrapping_mul(12) as u32;
    let row = plane.address.offset(ByteOffset((plane.stride & !3).wrapping_mul(y as u32).wrapping_add(group)));
    match x % 8 {
        0 => (unsafe { row.word() }) & 0xfff,
        1 => (unsafe { row.word() } >> 12) & 0xfff,
        2 => (unsafe { row.word() } >> 24) | ((unsafe { row.offset(ByteOffset(4)).word() } & 15) << 8),
        3 => (unsafe { row.offset(ByteOffset(4)).word() } >> 4) & 0xfff,
        4 => (unsafe { row.offset(ByteOffset(4)).word() } >> 16) & 0xfff,
        5 => (unsafe { row.offset(ByteOffset(4)).word() } >> 28) | ((unsafe { row.offset(ByteOffset(8)).byte() } as u32) << 4),
        6 => (unsafe { row.offset(ByteOffset(8)).word() } >> 8) & 0xfff,
        _ => (unsafe { row.offset(ByteOffset(8)).word() }) >> 20,
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_image_write_packed12(image: *const ImagePlane, x: i32, y: i32, value: u32) {
    let plane = unsafe { read_volatile(image) };
    let group = (x / 8).wrapping_mul(12) as u32;
    let row = plane.address.offset(ByteOffset((plane.stride & !3).wrapping_mul(y as u32).wrapping_add(group)));
    let (offset, mask, bits) = match x % 8 {
        0 => (0, 0xfffff000, value & 0xfff),
        1 => (0, 0xff000fff, (value & 0xfff) << 12),
        2 => (0, 0x00ffffff, value << 24),
        3 => (4, 0xffff000f, (value & 0xfff) << 4),
        4 => (4, 0xf000ffff, (value & 0xfff) << 16),
        5 => (4, 0x0fffffff, value << 28),
        6 => (8, 0xfff000ff, (value & 0xfff) << 8),
        _ => (8, 0x000fffff, value << 20),
    };
    let word = row.offset(ByteOffset(offset));
    unsafe { word.write_word((word.word() & mask) | bits) };
    if x % 8 == 2 {
        let next = row.offset(ByteOffset(4));
        unsafe { next.write_word((next.word() & 0xfffffff0) | ((value & 0xfff) >> 8)) };
    } else if x % 8 == 5 {
        let next = row.offset(ByteOffset(8));
        unsafe { next.write_word((next.word() & 0xffffff00) | ((value & 0xfff) >> 4)) };
    }
}

fn square(value: u32) -> u32 { value.wrapping_mul(value) }

unsafe fn half_configuration(key: u32) -> u32 {
    unsafe { xm1_buffers_configuration_half(ConfigurationKey(key)) as u32 }
}

unsafe extern "C" {
    fn xm1_pipeline_geometry_divide_fault() -> !;
}

unsafe fn xm1_pipeline_geometry_signed_divide(numerator: i32, denominator: i32) -> i32 {
    if denominator == 0 { unsafe { xm1_pipeline_geometry_divide_fault() } }
    numerator.wrapping_div(denominator)
}

unsafe fn unsigned_divide(numerator: u32, denominator: u32) -> u32 {
    if denominator == 0 { unsafe { xm1_pipeline_geometry_divide_fault() } }
    numerator / denominator
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_image_configure_planes(stage: u32, image: *const ImagePlane, alternate: u32) -> u32 {
    let frame = Address32(stage);
    let plane = unsafe { read_volatile(image) };
    let task = unsafe { xm1_pipeline_image_current_task() };
    let parameters = frame.offset(ByteOffset(if alternate == 0 { 0x2268 } else { 0x23f0 }));
    if unsafe { parameters.offset(ByteOffset(0x2c)).word() } == 0 { return 1; }
    let strength = [
        unsafe { parameters.offset(ByteOffset(0x30)).word() } >> 2,
        unsafe { parameters.offset(ByteOffset(0x34)).word() } >> 2,
        unsafe { parameters.offset(ByteOffset(0x38)).word() } >> 2,
    ];
    let selection = if plane.flags & 0x2800 == 0 { (plane.flags >> 12) & 1 } else { 2 };
    let table = Address32(0xe58764);
    let (origin_x, origin_y, extent_x, extent_y, scale) = if selection != 2 {
        (unsafe { half_configuration(table.offset(ByteOffset(selection * 4)).word()) },
         unsafe { half_configuration(table.offset(ByteOffset((selection + 3) * 4)).word()) },
         unsafe { half_configuration(table.offset(ByteOffset((selection + 6) * 4)).word()) },
         unsafe { half_configuration(table.offset(ByteOffset((selection + 9) * 4)).word()) }, 1u32)
    } else {
        (unsafe { half_configuration(0x2eca) }, unsafe { half_configuration(0x2ecc) },
         unsafe { half_configuration(0x2ece) }, unsafe { half_configuration(0x2ed0) }, 2u32)
    };
    let mut centers_x = [0u32; 3];
    let mut centers_y = [0u32; 3];
    let mut starts_x = [0u32; 3];
    let mut starts_y = [0u32; 3];
    let mut baseline = [0i32; 3];
    let mut common_numerator = 0;
    let radius_square = square((extent_y.wrapping_mul(scale) as i32 / 2) as u32);
    for channel in 0..3 {
        let keys = table.offset(ByteOffset(48 + selection * 84 + channel as u32 * 28));
        centers_x[channel] = unsafe { half_configuration(keys.word()) };
        centers_y[channel] = unsafe { half_configuration(keys.offset(ByteOffset(4)).word()) };
        starts_x[channel] = unsafe { half_configuration(keys.offset(ByteOffset(8)).word()) };
        starts_y[channel] = unsafe { half_configuration(keys.offset(ByteOffset(12)).word()) };
        let floor = unsafe { cfg(keys.offset(ByteOffset(24)).word()) }.wrapping_mul(4);
        let dx = square(starts_x[channel].wrapping_sub(centers_x[channel]));
        let dy = square(starts_y[channel].wrapping_sub(centers_y[channel]).wrapping_mul(scale));
        let outer = square(centers_x[channel].wrapping_sub(starts_x[channel]).wrapping_add(extent_x));
        let denominator = ((radius_square.wrapping_add(dx)) as i32).min(outer as i32);
        baseline[channel] = unsafe { xm1_pipeline_geometry_signed_divide(dx.wrapping_add(dy).wrapping_mul(strength[channel]) as i32, denominator) }.wrapping_add(floor as i32);
        if channel == 0 { common_numerator = dx; }
    }
    let _ = (origin_x, origin_y);
    let half_extent = (extent_y as i32 / 2) as u32;
    let first_y = starts_y[0].wrapping_sub(half_extent).wrapping_add(1);
    let last_y = starts_y[0].wrapping_add(half_extent);
    let first_x = starts_x[0].wrapping_sub(extent_x).wrapping_add(1);
    let last_x = starts_x[0];
    let row_offset = unsafe { frame.offset(ByteOffset(0x498)).word() };
    let packed = unsafe { frame.offset(ByteOffset(0x49c)).word() } & 16 != 0;
    unsafe { xm1_pipeline_image_configure_task(task, 12) };
    let mut y = first_y;
    while y <= last_y {
        let dy = square(y.wrapping_sub(centers_y[0]).wrapping_mul(scale));
        let mut x = first_x;
        while x <= last_x {
            let denominator = square(x.wrapping_sub(centers_x[0])).wrapping_add(dy);
            let row = row_offset.wrapping_add(x);
            for channel in 0..3 {
                let correction = unsafe { xm1_pipeline_geometry_signed_divide(common_numerator.wrapping_mul(strength[channel]) as i32, denominator as i32) }.wrapping_sub(baseline[channel]);
                if correction > 0 {
                    let column = y.wrapping_add(starts_y[channel].wrapping_sub(starts_y[0]));
                    if packed {
                        let old = unsafe { xm1_pipeline_image_read_packed12(image, row as i32, column as i32) };
                        let value = old.wrapping_sub(correction as u32);
                        let value = if (value as i32) < 0 { 0 } else { value };
                        unsafe { xm1_pipeline_image_write_packed12(image, row as i32, column as i32, value) };
                    } else {
                        let address = plane.address.offset(ByteOffset(row.wrapping_mul(plane.stride & !1).wrapping_add(column.wrapping_mul(2))));
                        let value = (unsafe { address.half() } as u32).wrapping_sub((correction as u32).wrapping_mul(4));
                        unsafe { address.write_half(if (value as i32) < 0 { 0 } else { value as u16 }) };
                    }
                }
            }
            x = x.wrapping_add(1);
        }
        y = y.wrapping_add(1);
    }
    unsafe { xm1_pipeline_image_configure_task(task, 6) };
    1
}

unsafe fn numeric_level(value: u32) -> u32 {
    let table = Address32(0x816e20);
    let mut index = 1u32;
    while index < 20 {
        let row = table.offset(ByteOffset((index - 1) * 16));
        let limit = unsafe { half_configuration(row.offset(ByteOffset(16)).word()) }
            .wrapping_add(unsafe { row.offset(ByteOffset(20)).word() });
        if value.wrapping_add(50) < limit { break; }
        index += 1;
    }
    let code = unsafe { table.offset(ByteOffset(index * 16 - 8)).word() };
    let mut slot = 0;
    if code != 7 {
        slot = 1;
        while slot < 20 {
            if unsafe { table.offset(ByteOffset((slot - 1) * 16 + 24)).word() } == code { break; }
            slot += 1;
        }
        if slot == 20 { slot = 2; }
    }
    unsafe { table.offset(ByteOffset(slot * 16 + 12)).word() }
}

unsafe fn geometry_delta(frame: Address32, alternate: u32, parameters: Address32) -> i32 {
    let flags = unsafe { frame.offset(ByteOffset(0x49c)).word() };
    let selection = if flags & 0x2000 != 0 { 2 } else { (flags >> 12) & 1 };
    let key = unsafe { Address32(0xe58614 + ((alternate == 1) as u32 * 3 + selection) * 4).word() };
    let scale = unsafe { half_configuration(key) };
    let mut first = 0u32;
    let mut second = 0u32;
    for index in 0..4 {
        first += unsafe { parameters.offset(ByteOffset(12 + index * 2)).half() } as u32;
        second += unsafe { parameters.offset(ByteOffset(20 + index * 2)).half() } as u32;
    }
    unsafe { parameters.offset(ByteOffset(0x180)).write_word(first >> 2) };
    unsafe { parameters.offset(ByteOffset(0x184)).write_word(second >> 2) };
    if unsafe { parameters.offset(ByteOffset(4)).word() } == 0 { return 0; }
    let denominator = unsafe { parameters.offset(ByteOffset(36)).word() }
        .wrapping_sub(unsafe { parameters.offset(ByteOffset(32)).word() }) as i32;
    if denominator == 0 { return 0; }
    let numerator = (second.wrapping_sub(first) as i32 as i64).wrapping_mul(scale as i64).wrapping_shl(10);
    let delta = (numerator.wrapping_div(denominator as i64) as i32) / 40;
    if delta < -8192 { 8192 } else if delta >= 8192 { 8191 } else { delta }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_image_geometry_delta(stage: Address32, secondary: u32, parameters: Address32, output: *mut i32) {
    let delta = unsafe { geometry_delta(stage, secondary, parameters) };
    unsafe { write_volatile(output, delta) };
}

fn clamp_geometry(value: u32) -> u32 {
    let signed = value as i32;
    if signed < -1023 { 0 } else if signed >= 0x800000 { 8191 } else { ((signed / 1024) as u32) & 0xffff }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_image_geometry_levels(stage: u32, alternate: u32, secondary: u32, output: *mut u32) {
    let frame = Address32(stage);
    let mode = unsafe { frame.offset(ByteOffset(0x640)).word() };
    let parameters = frame.offset(ByteOffset(if secondary == 1 { 0x2578 } else if alternate == 0 { 0x2268 } else { 0x23f0 }));
    let mut levels = [0u32; 4];
    if secondary == 1 && mode == 1 {
        for (index, offset) in [0x2586, 0x2584, 0x2588, 0x258a].into_iter().enumerate() {
            levels[index] = ((unsafe { frame.offset(ByteOffset(offset)).half() } as u32)
                + (unsafe { frame.offset(ByteOffset(offset + 8)).half() } as u32) + 1) >> 1;
        }
    } else if mode == 2 {
        levels.fill(unsafe { parameters.offset(ByteOffset(0x44)).word() });
    } else if secondary != 1 && mode == 1 && unsafe { parameters.offset(ByteOffset(8)).word() } != 0 {
        let delta = unsafe { geometry_delta(frame, secondary, parameters) } as u32;
        let distance = unsafe { parameters.offset(ByteOffset(0x20)).word() }
            .wrapping_sub(unsafe { parameters.offset(ByteOffset(0x1c)).word() });
        for (index, offset) in [14, 12, 16, 18].into_iter().enumerate() {
            levels[index] = clamp_geometry((unsafe { parameters.offset(ByteOffset(offset)).half() } as u32)
                .wrapping_mul(1024).wrapping_sub(delta.wrapping_mul(distance)));
        }
    } else {
        let mut level = unsafe { numeric_level(frame.offset(ByteOffset(0x732)).half() as u32) };
        if level != 0 { level -= 1; }
        let key = unsafe { Address32(0xe585f0 + level.wrapping_mul(4)).word() };
        if unsafe { cfg(key) } == 1 {
            for (index, value) in levels.iter_mut().enumerate() {
                let offset = level.wrapping_mul(16).wrapping_add(index as u32 * 4);
                let first_key = unsafe { Address32(0xe58b20).offset(ByteOffset(offset)).word() };
                let first = unsafe { half_configuration(first_key) };
                let second_key = unsafe { Address32(0xe58a90).offset(ByteOffset(offset)).word() };
                *value = first.wrapping_add(unsafe { half_configuration(second_key) });
            }
        } else {
            for (index, offset) in [0x176, 0x174, 0x178, 0x17a].into_iter().enumerate() {
                levels[index] = unsafe { parameters.offset(ByteOffset(offset)).half() } as u32;
            }
        }
    }
    for index in 0..8 {
        unsafe { write_volatile(output.add(index), levels[index & 3]) };
    }
    if secondary == 0 {
        let destination = frame.offset(ByteOffset(0x160c_u32.wrapping_add(alternate.wrapping_mul(32))));
        for index in 0..8 { unsafe { destination.offset(ByteOffset(index * 4)).write_word(levels[index as usize & 3]) }; }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_image_configure_plane_flags(stage: u32, source: *const ImagePlane, destination: *const ImagePlane) -> u32 {
    let task = unsafe { xm1_pipeline_image_current_task() };
    let mut view = core::mem::MaybeUninit::<ImagePlane>::uninit();
    let mut phase = [0u32; 2];
    let mut enabled = 0u16;
    unsafe { xm1_pipeline_image_buffer_views(stage, source, view.as_mut_ptr(), phase.as_mut_ptr(), &mut enabled) };
    let view = unsafe { view.assume_init() };
    let frame = Address32(stage);
    let difference = unsafe { frame.offset(ByteOffset(0x42c)).word() }
        .wrapping_sub(unsafe { frame.offset(ByteOffset(0x438)).word() }) as i32;
    let _margin = (difference / 2) as u16;
    let mut levels = [0u32; 8];
    unsafe { xm1_pipeline_image_geometry_levels(stage, 0, 0, levels.as_mut_ptr()) };
    let parameters = Address32(0x1569610);
    for word in 0..75 { unsafe { parameters.offset(ByteOffset(word * 4)).write_word(0) }; }
    unsafe { xm1_pipeline_image_configure_task(task, 12) };
    let period = unsafe { half_configuration(0xa798) };
    let (divisor, multiplier) = if view.flags & 16 != 0 { (3, 2) } else { (2, 1) };
    let width = unsafe { unsigned_divide(view.width.wrapping_mul(multiplier), divisor) };
    let count = unsafe { unsigned_divide(width.wrapping_mul(100).wrapping_add(period).wrapping_sub(1), period) } as u16;
    let control = unsafe { cfg(0xb2) };
    let mean = levels[..4].iter().map(|value| *value as u16 as u32).fold(0u32, u32::wrapping_add) >> 2;
    let buffer = unsafe { read_volatile(destination) }.address;
    for index in 0..count as u32 {
        unsafe { buffer.offset(ByteOffset(index * 2)).write_half(if control & 4 == 0 { mean as u16 } else { 0 }) };
    }
    unsafe { frame.offset(ByteOffset(0x19b6)).write_half(period as u16) };
    unsafe { xm1_pipeline_image_configure_task(task, 6) };
    1
}

unsafe fn clear_groups(plane: &ImagePlane, row: u32, first: u32, end: u32) {
    for group in first..end {
        let address = plane.address.offset(ByteOffset(row.wrapping_mul(plane.stride & !7).wrapping_add(group.wrapping_mul(8))));
        unsafe { address.write_word(0) };
        unsafe { address.offset(ByteOffset(4)).write_word(0) };
    }
}

unsafe fn clear_row(plane: &ImagePlane, row: u32) {
    let groups = plane.width >> 3;
    unsafe { clear_groups(plane, row, 0, groups) };
    for byte in groups.wrapping_mul(8)..plane.width {
        unsafe { plane.address.offset(ByteOffset(row.wrapping_mul(plane.stride).wrapping_add(byte))).write_byte(0) };
    }
}

/// # Safety
/// `image` must point to a live, aligned `ImagePlane` whose address, stride, width and height cover the cleared rows.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_image_configure_borders(image: *const ImagePlane, top: u32, bottom: i32, left: u32, right: i32) {
    let plane = unsafe { read_volatile(image) };
    let task = unsafe { xm1_pipeline_image_current_task() };
    unsafe { xm1_pipeline_image_cache_maintenance() };
    unsafe { xm1_pipeline_image_configure_task(task, 12) };
    for row in 0..top { unsafe { clear_row(&plane, row) }; }
    let end = plane.height.wrapping_sub(bottom as u32);
    for row in end..plane.height { unsafe { clear_row(&plane, row) }; }
    if plane.flags & 0x10000 == 0 && plane.flags & 16 != 0 {
        let left_scaled = left.wrapping_mul(15);
        let left_groups = left_scaled / 120;
        for row in top..end {
            let address = plane.address.offset(ByteOffset(row.wrapping_mul(plane.stride & !3)));
            for group in 0..left_groups {
                let start = address.offset(ByteOffset(group.wrapping_mul(12)));
                unsafe { start.write_word(0) };
                unsafe { start.offset(ByteOffset(4)).write_word(0) };
                unsafe { start.offset(ByteOffset(8)).write_word(0) };
            }
            if left_scaled % 120 != 0 {
                for pixel in left_groups.wrapping_mul(120) / 15..left {
                    unsafe { xm1_pipeline_image_write_packed12(image, pixel as i32, row as i32, 0) };
                }
            }
            let right_scaled = plane.width.wrapping_mul(10).wrapping_sub((right as u32).wrapping_mul(15));
            let mut pixel = right_scaled.wrapping_mul(2) / 30;
            if right_scaled % 120 != 0 && pixel.wrapping_mul(15) % 120 != 0 {
                let mut next = pixel.wrapping_add(1).wrapping_mul(15);
                loop {
                    unsafe { xm1_pipeline_image_write_packed12(image, pixel as i32, row as i32, 0) };
                    pixel = pixel.wrapping_add(1);
                    let remainder = next % 120;
                    next = next.wrapping_add(15);
                    if remainder == 0 { break; }
                }
            }
            let first_group = pixel.wrapping_mul(15) / 120;
            for group in first_group..plane.width / 12 {
                let start = plane.address.offset(ByteOffset((plane.stride >> 2).wrapping_mul(row).wrapping_add(group.wrapping_mul(3)).wrapping_mul(4)));
                unsafe { start.write_word(0) };
                unsafe { start.offset(ByteOffset(4)).write_word(0) };
                unsafe { start.offset(ByteOffset(8)).write_word(0) };
            }
        }
    } else {
        let factor = if plane.flags & 0x10000 == 0 { 20 } else { 10 };
        let left_scaled = left.wrapping_mul(factor);
        let left_groups = left_scaled / 80;
        let right_bytes = (right as u32).wrapping_mul(factor) / 10;
        for row in top..end {
            unsafe { clear_groups(&plane, row, 0, left_groups) };
            for byte in left_groups.wrapping_mul(8)..left_scaled / 10 {
                unsafe { plane.address.offset(ByteOffset(row.wrapping_mul(plane.stride).wrapping_add(byte))).write_byte(0) };
            }
            let mut byte = plane.width.wrapping_sub(right_bytes);
            while byte & 7 != 0 {
                unsafe { plane.address.offset(ByteOffset(row.wrapping_mul(plane.stride).wrapping_add(byte))).write_byte(0) };
                byte = byte.wrapping_add(1);
            }
            let first = byte >> 3;
            let groups = plane.width >> 3;
            unsafe { clear_groups(&plane, row, first, groups) };
            let last = if first < groups { groups } else { first };
            for byte in last.wrapping_mul(8)..plane.width {
                unsafe { plane.address.offset(ByteOffset(row.wrapping_mul(plane.stride).wrapping_add(byte))).write_byte(0) };
            }
        }
    }
    unsafe { xm1_pipeline_image_configure_task(task, 6) };
    unsafe { xm1_pipeline_image_cache_maintenance() };
}

#[repr(C)]
pub struct CropPacket {
    source_mode: u32,
    target_mode: u32,
    transform: u32,
    input_address: Address32,
    input_stride: u32,
    input_width: u32,
    input_height: u32,
    output_address: Address32,
    output_stride: u32,
    requested_stride: u32,
    output_width: u32,
    output_height: u32,
    option: u32,
    limit: u32,
    filter_enabled: u32,
    weights: [u32; 3],
    controls: [u32; 4],
    completed: Address32,
}

#[repr(C)]
pub struct CropHardware {
    source_mode: u32,
    target_mode: u32,
    transform: u32,
    input_address: Address32,
    input_stride: u32,
    input_width: u32,
    input_height: u32,
    output_address: Address32,
    output_stride: u32,
    output_width: u32,
    output_height: u32,
    input_step: u32,
    output_step: u32,
    reserved_34: u32,
    reserved_38: u32,
    mode: u32,
    enabled: u32,
    border: u32,
    grouping: u32,
    option: u32,
    limit: u32,
    filter_enabled: u32,
    weights: [u32; 3],
    controls: [u32; 4],
}

#[derive(Clone, Copy)]
#[repr(transparent)]
struct CropEngineAddress(Address32);

impl CropEngineAddress {
    const STATE: Self = Self(Address32(0x014ce498));
    fn field(self, offset: u32) -> Address32 { self.0.offset(ByteOffset(offset)) }
}

#[derive(Clone, Copy)]
#[repr(u32)]
enum CropRegister {
    Control = 0xfff43008,
    InputAddress = 0xfff43010,
    InputStride = 0xfff43014,
    InputDimensions = 0xfff43018,
    OutputAddress = 0xfff43020,
    OutputStride = 0xfff43024,
    OutputDimensions = 0xfff43028,
    Limit = 0xfff43030,
    Weights = 0xfff43034,
    Controls = 0xfff43038,
    Steps = 0xfff4303c,
    Options = 0xfff43080,
    Pending = 0xfff43084,
    Acknowledge = 0xfff43088,
    Enable = 0xfff4308c,
    Begin = 0xfff43004,
}

impl CropRegister {
    unsafe fn read(self) -> u32 { unsafe { Address32(self as u32).word() } }
    unsafe fn write(self, value: u32) { unsafe { Address32(self as u32).write_word(value) } }
}

unsafe extern "C" {
    fn xm1_pipeline_image_wait_reserved(handle: u32, timeout: u32) -> i32;
    fn xm1_pipeline_image_signal_reserved(handle: u32) -> i32;
    fn xm1_pipeline_image_route_request(selector: u32, slot: u32, level: u32) -> i32;
    fn xm1_pipeline_image_route_acquire(index: u32, handle: Address32) -> i32;
    fn xm1_pipeline_image_route_release(handle: u32) -> i32;
    fn xm1_pipeline_image_interrupt_acquire(source: u32, callback: u32, handle: Address32) -> i32;
    fn xm1_pipeline_image_interrupt_release(handle: u32) -> i32;
    fn xm1_develop_enter_critical() -> u32;
    fn xm1_develop_leave_critical(token: u32);
    fn xm1_pipeline_geometry_crop_reset_wait();
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_image_crop_device_acquire() {
    let state = Address32(0x0089f748);
    if unsafe { xm1_pipeline_image_wait_reserved(0x2b, u32::MAX) } == 0 {
        unsafe { state.write_word(0) };
        unsafe { state.offset(ByteOffset(4)).write_word(0) };
        unsafe { xm1_pipeline_image_route_request(0x410, 0x13, 8) };
        if unsafe { xm1_pipeline_image_route_acquire(0x29, state) } == 0 {
            unsafe { state.offset(ByteOffset(4)).write_word(1) };
        }
    }
    unsafe { CropRegister::Acknowledge.write(u32::MAX) };
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_image_crop_device_release() {
    unsafe { CropRegister::Enable.write(u32::MAX) };
    let state = Address32(0x0089f748);
    let handle = unsafe { state.word() };
    if handle != 0 {
        unsafe { xm1_pipeline_image_route_release(handle) };
        unsafe { state.write_word(0) };
    }
    unsafe { xm1_pipeline_image_route_request(0x410, 0x13, 0) };
    unsafe { xm1_pipeline_image_signal_reserved(0x2b) };
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_image_crop_device_begin() {
    unsafe { CropRegister::Enable.write(1) };
    unsafe { CropRegister::Begin.write(1) };
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_image_crop_initialize() -> i32 {
    unsafe { xm1_pipeline_image_crop_device_acquire() };
    for word in 0..54 { unsafe { CropEngineAddress::STATE.field(word * 4).write_word(0) }; }
    unsafe { xm1_pipeline_geometry_crop_reset_wait() };
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_image_crop_release() -> i32 {
    unsafe { xm1_pipeline_image_crop_device_release() };
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_image_crop_begin() {
    unsafe { xm1_pipeline_image_cache_maintenance() };
    unsafe { xm1_pipeline_image_crop_device_begin() };
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_image_crop_wait() -> i32 {
    unsafe { xm1_pipeline_geometry_crop_reset_wait() };
    if unsafe { CropEngineAddress::STATE.field(4).word() } != 0 {
        let result = unsafe { xm1_pipeline_image_interrupt_release(CropEngineAddress::STATE.field(0).word()) };
        if result != 0 { return result; }
        unsafe { CropEngineAddress::STATE.field(4).write_word(0) };
    }
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_image_crop_interrupt() {
    let token = unsafe { xm1_develop_enter_critical() };
    unsafe { CropRegister::Acknowledge.write(CropRegister::Pending.read()) };
    let completed = unsafe { CropEngineAddress::STATE.field(96).word() };
    unsafe { CropEngineAddress::STATE.field(96).write_word(0) };
    unsafe { xm1_develop_leave_critical(token) };
    if completed != 0 {
        let _ = callbacks::call_no_arguments(callbacks::registry(), CallbackWord(completed));
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_image_crop_publish(packet: *const CropPacket) {
    for index in 0..23 {
        let value = unsafe { read_volatile((packet as *const u32).add(index)) };
        unsafe { CropEngineAddress::STATE.field(8 + index as u32 * 4).write_word(value) };
    }
    let result = unsafe { xm1_pipeline_image_interrupt_acquire(0x3c, xm1_pipeline_image_crop_interrupt as *const () as u32, CropEngineAddress::STATE.field(0)) };
    if result == 0 {
        unsafe { CropEngineAddress::STATE.field(4).write_word(1) };
        unsafe { xm1_pipeline_image_crop_configure(packet) };
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_image_crop_configure(packet: *const CropPacket) -> i32 {
    let p = unsafe { &*packet };
    let hardware = CropEngineAddress::STATE.field(100);
    unsafe { hardware.write_word(p.source_mode) };
    unsafe { hardware.offset(ByteOffset(4)).write_word(p.target_mode) };
    unsafe { hardware.offset(ByteOffset(8)).write_word(p.transform) };
    if p.input_address.0 & 15 != 0 { rejected(); }
    unsafe { hardware.offset(ByteOffset(12)).write_word(p.input_address.0) };
    if p.input_stride & 15 != 0 { rejected(); }
    unsafe { hardware.offset(ByteOffset(16)).write_word(p.input_stride) };
    if p.input_width & 15 != 0 { rejected(); }
    let input_width = if p.source_mode == 0 { p.input_width >> 1 } else { p.input_width };
    unsafe { hardware.offset(ByteOffset(20)).write_word(input_width) };
    if p.input_height & 1 != 0 { rejected(); }
    unsafe { hardware.offset(ByteOffset(24)).write_word(p.input_height) };
    if p.output_address.0 & 15 != 0 { rejected(); }
    unsafe { hardware.offset(ByteOffset(28)).write_word(p.output_address.0) };
    if p.output_stride & 15 != 0 { rejected(); }
    unsafe { hardware.offset(ByteOffset(32)).write_word(p.output_stride) };
    if p.output_width & 15 != 0 { rejected(); }
    let output_width = if p.target_mode == 0 { p.output_width >> 1 } else { p.output_width };
    unsafe { hardware.offset(ByteOffset(36)).write_word(output_width) };
    unsafe { hardware.offset(ByteOffset(40)).write_word(p.output_height) };
    if p.requested_stride & 15 != 0 || p.requested_stride > p.output_stride || p.requested_stride < p.output_width { rejected(); }
    let reduction = if p.transform == 0 { 2u32 } else { 1u32 };
    if input_width != output_width.wrapping_mul(reduction) || p.input_height != p.output_height.wrapping_mul(reduction) { rejected(); }
    let mut tile = if p.target_mode == 0 { 32 } else { 16 };
    let mut grouping = 2;
    while (p.output_width >> 4) > tile * ((p.requested_stride >> 4) / tile) {
        (tile, grouping) = match tile {
            32 => (24, 2), 24 => (16, 2), 16 => (8, 2), 8 => (12, 1),
            12 => (4, 1), 4 => (10, 0), 10 => (6, 0), 6 => (2, 0),
            _ => rejected(),
        };
    }
    let output_step = tile << if p.target_mode == 0 { 3 } else { 4 };
    unsafe { hardware.offset(ByteOffset(44)).write_word(output_step.wrapping_mul(reduction)) };
    unsafe { hardware.offset(ByteOffset(48)).write_word(output_step) };
    unsafe { hardware.offset(ByteOffset(52)).write_word(0) };
    unsafe { hardware.offset(ByteOffset(56)).write_word(0) };
    unsafe { hardware.offset(ByteOffset(60)).write_word(2) };
    unsafe { hardware.offset(ByteOffset(64)).write_word((p.transform != 0) as u32) };
    unsafe { hardware.offset(ByteOffset(68)).write_word(if p.transform == 0 { 0 } else { 60 }) };
    unsafe { hardware.offset(ByteOffset(72)).write_word(grouping) };
    for index in 0..10 {
        let value = unsafe { read_volatile((packet as *const u32).add(12 + index)) };
        unsafe { hardware.offset(ByteOffset(76 + index as u32 * 4)).write_word(value) };
    }
    unsafe { xm1_pipeline_image_crop_program(hardware.0 as *const CropHardware) };
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_image_crop_program(configuration: *const CropHardware) {
    let p = unsafe { &*configuration };
    unsafe { CropRegister::Control.write((p.source_mode & 3) | ((p.target_mode & 1) << 4) | ((p.option & 1) << 8) | ((p.filter_enabled & 1) << 12) | ((p.transform & 1) << 16)) };
    if p.input_address.0 & 15 != 0 { rejected(); }
    unsafe { CropRegister::InputAddress.write(p.input_address.0) };
    if p.input_stride > 0x7fff || p.input_stride & 15 != 0 { rejected(); }
    unsafe { CropRegister::InputStride.write(p.input_stride & 0x7fff) };
    if p.input_width > 0x1fff || p.input_height > 0x1fff || p.input_width & 7 != 0 { rejected(); }
    unsafe { CropRegister::InputDimensions.write((p.input_width & 0x1fff) | ((p.input_height & 0x1fff) << 16)) };
    if p.output_address.0 & 15 != 0 { rejected(); }
    unsafe { CropRegister::OutputAddress.write(p.output_address.0) };
    if p.output_stride > 0x7fff || p.output_stride & 15 != 0 { rejected(); }
    unsafe { CropRegister::OutputStride.write(p.output_stride & 0x7fff) };
    if p.output_width > 0x1fff || p.output_height > 0x1fff || p.output_width & 7 != 0 { rejected(); }
    unsafe { CropRegister::OutputDimensions.write((p.output_width & 0x1fff) | ((p.output_height & 0x1fff) << 16)) };
    if p.limit > 0x3ff { rejected(); }
    unsafe { CropRegister::Limit.write(p.limit & 0x3ff) };
    if p.weights.iter().any(|value| *value > 0x7f) { rejected(); }
    if p.filter_enabled == 1 && p.weights[0].wrapping_add(p.weights[2].wrapping_add(p.weights[1]).wrapping_mul(2)) != 128 { rejected(); }
    unsafe { CropRegister::Weights.write((p.weights[0] & 0x7f) | ((p.weights[1] & 0x7f) << 8) | ((p.weights[2] & 0x7f) << 16)) };
    if p.controls.iter().any(|value| *value > 0xff) { rejected(); }
    if p.controls[0].wrapping_add(p.controls[3].wrapping_add(p.controls[2]).wrapping_add(p.controls[1]).wrapping_mul(2)) != 256 { rejected(); }
    unsafe { CropRegister::Controls.write(p.controls[0] | (p.controls[1] << 8) | (p.controls[2] << 16) | (p.controls[3] << 24)) };
    if p.input_step > 512 || p.output_step > 512 || (p.input_step | p.output_step) & 7 != 0 { rejected(); }
    unsafe { CropRegister::Steps.write((p.input_step & 0x3ff) | ((p.output_step & 0x3ff) << 16)) };
    if p.reserved_38 > 255 || p.border > 255 { rejected(); }
    unsafe { CropRegister::Options.write((p.reserved_38 & 255) | ((p.reserved_34 & 1) << 8) | ((p.mode & 3) << 12) | ((p.border & 255) << 16) | ((p.enabled & 1) << 24) | ((p.grouping & 3) << 28)) };
}
