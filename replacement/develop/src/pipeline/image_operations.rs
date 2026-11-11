use super::shared::{Address32, ByteOffset};
use core::ptr::{read_volatile, write_volatile};

#[derive(Clone, Copy)]
#[repr(transparent)]
pub(super) struct Pixels(pub(super) u32);

#[derive(Clone, Copy)]
#[repr(transparent)]
pub(super) struct RowBytes(pub(super) u32);

#[derive(Clone, Copy)]
#[repr(C)]
pub(super) struct ImagePlane {
    pub(super) mode: u32,
    pub(super) address: Address32,
    pub(super) size: u32,
    pub(super) stride: RowBytes,
    pub(super) width: Pixels,
    pub(super) height: Pixels,
    pub(super) flags: u32,
}

#[derive(Clone, Copy)]
#[repr(u32)]
pub(super) enum PlaneOperationKind {
    Select = 1,
    Merge = 2,
    Combine = 3,
    BlendFinish = 4,
    SelectThree = 5,
    LinearCombination = 7,
}

#[repr(C)]
pub(super) struct PlaneOperation {
    pub(super) kind: PlaneOperationKind,
    pub(super) argument: Address32,
    pub(super) table: Address32,
    pub(super) coefficients: *const i32,
    pub(super) kind8_parameters: Address32,
    pub(super) kind9_parameters: Address32,
    pub(super) parameters: Address32,
}

impl PlaneOperation {
    fn new(kind: PlaneOperationKind, coefficients: *const i32) -> Self {
        Self { kind, argument: Address32(0), table: Address32(0), coefficients,
            kind8_parameters: Address32(0), kind9_parameters: Address32(0), parameters: Address32(0) }
    }
}

unsafe extern "C" {
    fn xm1_develop_lookup_width(profile: u32, aspect: u32) -> u32;
    fn xm1_develop_lookup_height(profile: u32, aspect: u32) -> u32;
}

unsafe fn shaped(source: *const ImagePlane, destination: *const ImagePlane) -> ImagePlane {
    let mut work = unsafe { read_volatile(destination) };
    work.width = unsafe { read_volatile(core::ptr::addr_of!((*source).width)) };
    work.height = unsafe { read_volatile(core::ptr::addr_of!((*source).height)) };
    work.size = work.stride.0.wrapping_mul(work.height.0);
    work
}

#[derive(Clone, Copy)]
#[repr(transparent)]
struct ConfigurationKey(u32);

unsafe extern "C" {
    fn xm1_buffers_configuration_byte(key: ConfigurationKey) -> u32;
    fn xm1_buffers_configuration_half(key: ConfigurationKey) -> u16;
    fn xm1_pipeline_image_process_crop(job: u32, parameters: *const u32, source: *const ImagePlane, destination: *mut ImagePlane) -> u32;
}

fn halt() -> ! { loop { core::hint::spin_loop(); } }

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_fill_chroma(_job: u32, first: u32, second: u32, destination: *const ImagePlane) -> u32 {
    unsafe { xm1_pipeline_image_fill_word(destination, first | first.wrapping_shl(8) | second.wrapping_shl(16) | second.wrapping_shl(24)) }
}

#[derive(Clone, Copy)]
enum SplitPolicy { Four, Five, Thirteen }

unsafe fn split(job: u32, planes: *const *const ImagePlane, count: u32, policy: SplitPolicy) -> u32 {
    let expected = match policy { SplitPolicy::Five => 4, _ => 6 };
    if count != expected { halt(); }
    let mut work = unsafe { read_volatile(read_volatile(planes)) };
    for index in 0..count - 1 {
        let mut parameters = [0; 14];
        parameters[6] = 1;
        match policy {
            SplitPolicy::Five => {
                parameters[7..10].copy_from_slice(&[26, 26, 25]);
                parameters[10..14].copy_from_slice(&[38, 37, 36, 36]);
            }
            SplitPolicy::Four | SplitPolicy::Thirteen => {
                let first_key = match policy { SplitPolicy::Thirteen => 0x8cd1, _ => 0x8c08 };
                for channel in 0..7 {
                    parameters[7 + channel] = unsafe { xm1_buffers_configuration_byte(ConfigurationKey(first_key + channel as u32)) };
                    if channel == 2 && matches!(policy, SplitPolicy::Thirteen) && index == 0 {
                        parameters[4] = 1;
                        parameters[5] = unsafe { xm1_buffers_configuration_byte(ConfigurationKey(0x8cd0)) };
                    }
                }
            }
        }
        let mut output = unsafe { read_volatile(read_volatile(planes.add(index as usize + 1))) };
        let half_width = work.width.0 >> 1;
        output.width = Pixels(match policy {
            SplitPolicy::Five => (((half_width.wrapping_add(1) >> 1).wrapping_add(7)) >> 3).wrapping_mul(16),
            _ => (((half_width.wrapping_add(1) >> 1).wrapping_add(7)) / 8).wrapping_mul(16),
        });
        output.height = Pixels(work.height.0.wrapping_add(1) >> 1);
        output.size = output.stride.0.wrapping_mul(output.height.0);
        parameters[3] = output.width.0.wrapping_add(31) & !31;
        let right = output.width.0.wrapping_sub(half_width);
        let bottom = output.height.0.wrapping_mul(2).wrapping_sub(work.height.0);
        if ((right | bottom) as i32) < 0 { halt(); }
        unsafe { xm1_pipeline_image_pad_edges(work.address, work.stride, work.width, work.height, 0, right, 0, bottom) };
        work.width.0 = work.width.0.wrapping_add(right.wrapping_mul(2));
        work.height.0 = work.height.0.wrapping_add(bottom);
        unsafe { xm1_pipeline_image_process_crop(job, parameters.as_ptr(), &work, &mut output) };
        work = output;
    }
    1
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_split_four(job: u32, planes: *const *const ImagePlane, count: u32) -> u32 {
    unsafe { split(job, planes, count, SplitPolicy::Four) }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_split_five(job: u32, planes: *const *const ImagePlane, count: u32) -> u32 {
    unsafe { split(job, planes, count, SplitPolicy::Five) }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_split_thirteen(job: u32, planes: *const *const ImagePlane, count: u32) -> u32 {
    unsafe { split(job, planes, count, SplitPolicy::Thirteen) }
}

unsafe fn linear(job: u32, source: *const ImagePlane, second: *const ImagePlane, destination: *mut ImagePlane, coefficients: [i32; 4], reverse: bool) {
    let sources = if reverse { [second, source, core::ptr::null(), core::ptr::null(), core::ptr::null()] }
        else { [source, second, core::ptr::null(), core::ptr::null(), core::ptr::null()] };
    let operation = PlaneOperation::new(PlaneOperationKind::LinearCombination, coefficients.as_ptr());
    let mut work = unsafe { shaped(source, destination) };
    unsafe { xm1_pipeline_image_operate(job, &operation, sources.as_ptr(), 2, &mut work); write_volatile(destination, work); }
}

unsafe fn center(job: Address32, destination: *mut ImagePlane) {
    let profile = unsafe { job.offset(ByteOffset(0x644)).word() };
    let aspect = unsafe { job.offset(ByteOffset(0x4f0)).word() };
    let mut output = unsafe { read_volatile(destination) };
    let width = unsafe { xm1_develop_lookup_width(profile, aspect) };
    let height = unsafe { xm1_develop_lookup_height(profile, aspect) };
    output.address = Address32(output.address.0.wrapping_add(output.width.0.wrapping_sub(width.wrapping_mul(2)) >> 1)
        .wrapping_add(output.stride.0.wrapping_mul(output.height.0.wrapping_sub(height)) >> 1));
    output.width = Pixels(unsafe { xm1_develop_lookup_width(profile, aspect) }.wrapping_mul(2));
    output.height = Pixels(unsafe { xm1_develop_lookup_height(profile, aspect) });
    output.size = output.width.0.wrapping_mul(output.height.0);
    unsafe { write_volatile(destination, output) };
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_finish_primary(job: u32, source: *const ImagePlane, second: *const ImagePlane, destination: *mut ImagePlane) -> u32 {
    unsafe { linear(job, source, second, destination, [256, -256, 0, 0], false) }; 1
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_finish_secondary(job: u32, source: *const ImagePlane, second: *const ImagePlane, destination: *mut ImagePlane) -> u32 {
    unsafe { linear(job, source, second, destination, [256, -256, 0, 0], false) }; 1
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_blend_secondary(job: u32, source: *const ImagePlane, second: *const ImagePlane, destination: *mut ImagePlane) -> u32 {
    unsafe { linear(job, source, second, destination, [256, -256, 0, 0], true) }; 1
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_select_five(job: u32, source: *const ImagePlane, second: *const ImagePlane, destination: *mut ImagePlane) -> u32 {
    unsafe { linear(job, source, second, destination, [256, 256, 0, 0], false); center(Address32(job), destination) }; 1
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_blend_planes(job: u32, source: *const ImagePlane, second: *const ImagePlane, third: *const ImagePlane, destination: *mut ImagePlane) -> u32 {
    let sources = [source, second, third, core::ptr::null(), core::ptr::null()];
    let coefficients = [256, -256, 128, 0];
    let operation = PlaneOperation::new(PlaneOperationKind::LinearCombination, coefficients.as_ptr());
    let mut work = unsafe { shaped(source, destination) };
    unsafe { xm1_pipeline_image_operate(job, &operation, sources.as_ptr(), 3, &mut work); write_volatile(destination, work) }; 1
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_blend_finish(job: u32, source: *const ImagePlane, second: *const ImagePlane, third: *const ImagePlane, fourth: *const ImagePlane, destination: *mut ImagePlane) -> u32 {
    let sources = [third, second, source, fourth, core::ptr::null()];
    let operation = PlaneOperation::new(PlaneOperationKind::BlendFinish, core::ptr::null());
    let mut work = unsafe { shaped(source, destination) };
    unsafe { xm1_pipeline_image_operate(job, &operation, sources.as_ptr(), 3, &mut work); write_volatile(destination, work) }; 1
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_select_three(job: u32, source: *const ImagePlane, second: *const ImagePlane, third: *const ImagePlane, destination: *mut ImagePlane) -> u32 {
    let sources = [source, second, core::ptr::null(), third, core::ptr::null()];
    let operation = PlaneOperation::new(PlaneOperationKind::SelectThree, core::ptr::null());
    let mut work = unsafe { shaped(source, destination) };
    unsafe { xm1_pipeline_image_operate(job, &operation, sources.as_ptr(), 2, &mut work); write_volatile(destination, work); center(Address32(job), destination) }; 1
}

unsafe extern "C" { fn xm1_pipeline_image_cache_maintenance(); }

unsafe fn copy_pair(source: Address32, destination: Address32) {
    let first = unsafe { source.word() };
    let second = unsafe { source.offset(ByteOffset(4)).word() };
    unsafe { destination.write_word(first); destination.offset(ByteOffset(4)).write_word(second); }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_pad_edges(address: Address32, stride: RowBytes, width: Pixels, height: Pixels, left: u32, right: u32, top: u32, bottom: u32) {
    unsafe { xm1_pipeline_image_cache_maintenance() };
    let row_bytes = stride.0 & !7;
    let signed_width = width.0 as i32;
    let columns = if signed_width < 0 { signed_width.wrapping_add(7) } else { signed_width } >> 3;
    if columns >= 0 {
        for column in 0..=columns as u32 {
            let offset = column.wrapping_mul(8);
            let first = address.offset(ByteOffset(offset));
            for row in 0..top {
                let destination = address.offset(ByteOffset(offset.wrapping_sub(row_bytes.wrapping_mul(row.wrapping_add(1)))));
                unsafe { copy_pair(first, destination) };
            }
            let last = address.offset(ByteOffset(row_bytes.wrapping_mul(height.0.wrapping_sub(1)).wrapping_add(offset)));
            for row in 0..bottom {
                let destination = address.offset(ByteOffset(row_bytes.wrapping_mul(height.0.wrapping_add(row)).wrapping_add(offset)));
                unsafe { copy_pair(last, destination) };
            }
        }
    }
    let top_extent = if top == 0 { bottom } else { top };
    let bottom_extent = if bottom == 0 { top } else { bottom };
    let mut row = top_extent.wrapping_neg() as i32;
    let end = height.0.wrapping_add(bottom_extent) as i32;
    while row < end {
        let start = address.offset(ByteOffset(row_bytes.wrapping_mul(row as u32)));
        let last = start.offset(ByteOffset((width.0 >> 3).wrapping_sub(1).wrapping_mul(8)));
        let luma = unsafe { start.byte() } as u32;
        let left_value = luma | luma << 8 | (unsafe { start.offset(ByteOffset(2)).byte() } as u32) << 16 | (unsafe { start.offset(ByteOffset(3)).byte() } as u32) << 24;
        let luma = unsafe { last.offset(ByteOffset(5)).byte() } as u32;
        let right_value = luma | luma << 8 | (unsafe { last.offset(ByteOffset(6)).byte() } as u32) << 16 | (unsafe { last.offset(ByteOffset(7)).byte() } as u32) << 24;
        for block in 0..left >> 2 {
            let destination = start.offset(ByteOffset((block.wrapping_add(1).wrapping_mul(8)).wrapping_neg()));
            unsafe { destination.write_word(left_value); destination.offset(ByteOffset(4)).write_word(left_value); }
        }
        let word_start = address.offset(ByteOffset((stride.0 & !3).wrapping_mul(row as u32)));
        if left & 3 != 0 {
            unsafe { word_start.offset(ByteOffset(((left >> 2).wrapping_add(1).wrapping_mul(8)).wrapping_neg().wrapping_add(4))).write_word(left_value) };
        }
        for block in 0..right >> 2 {
            let destination = last.offset(ByteOffset(block.wrapping_add(1).wrapping_mul(8)));
            unsafe { destination.write_word(right_value); destination.offset(ByteOffset(4)).write_word(right_value); }
        }
        if right & 3 != 0 {
            unsafe { word_start.offset(ByteOffset((width.0 >> 2).wrapping_mul(4).wrapping_add((right >> 2).wrapping_mul(8)))).write_word(right_value) };
        }
        row = row.wrapping_add(1);
    }
    unsafe { xm1_pipeline_image_cache_maintenance() };
}

#[derive(Clone, Copy)]
#[repr(C)]
pub(super) struct ConversionParameters {
    mode: u32,
    block_size: u32,
    value: u32,
    alternate: u32,
    offset: u32,
    reserved: [u32; 6],
}

#[repr(C)]
struct ConversionCommand {
    enabled: u32,
    value: u32,
    alternate: u32,
    reserved: [u32; 2],
    block_shift: u32,
    mode: u32,
    zero: u32,
    offset: u32,
    unused: [u32; 6],
    primary: u32,
}

unsafe extern "C" {
    fn image_conversion_device_acquire();
    fn image_conversion_device_release();
    fn image_conversion_device_reset();
    fn image_conversion_device_primary(parameters: *const u32);
    fn image_conversion_device_secondary(parameters: *const u32);
    fn image_conversion_device_window_primary(command: *const ConversionCommand);
    fn image_conversion_device_window_secondary(command: *const ConversionCommand);
    fn image_conversion_device_enable(enabled: u32);
    fn image_conversion_device_begin(first: u32, second: u32);
    fn xm1_develop_enter_critical() -> u32;
    fn xm1_develop_leave_critical(token: u32);
    fn image_conversion_register_interrupt(interrupt: u32, callback: unsafe extern "C" fn(), handle: *mut u32) -> i32;
    fn image_conversion_release_interrupt(handle: u32) -> i32;
    fn image_conversion_prepare_clock(first: u32, second: u32) -> u32;
}

const CONVERSION_STATE: Address32 = Address32(0x014ce454);

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_conversion_initialize() -> i32 {
    unsafe { image_conversion_device_acquire() };
    for offset in (0..0x44).step_by(4) {
        unsafe { CONVERSION_STATE.offset(ByteOffset(offset)).write_word(0) };
    }
    unsafe { image_conversion_device_reset() }; 1
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_conversion_acquire() -> i32 {
    unsafe { image_conversion_device_acquire(); image_conversion_device_reset() }; 1
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_conversion_release() {
    unsafe { image_conversion_device_release() };
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_conversion_finish_primary(parameters: *const u32) {
    unsafe { image_conversion_device_primary(parameters) };
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_conversion_finish_secondary(parameters: *const u32) {
    unsafe { image_conversion_device_secondary(parameters) };
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_conversion_configure(parameters: *const ConversionParameters) -> i32 {
    let parameters = unsafe { read_volatile(parameters) };
    if parameters.mode != 3 { halt(); }
    let block_shift = match parameters.block_size { 16 => 4, 8 => 3, 4 => 2, 2 => 1, _ => 0 };
    let command = ConversionCommand {
        enabled: 1, value: parameters.value, alternate: u32::from(parameters.alternate != 0),
        reserved: [0; 2], block_shift, mode: parameters.mode, zero: 0, offset: parameters.offset,
        unused: [0; 6], primary: u32::from(parameters.alternate == 0),
    };
    if parameters.alternate == 0 {
        unsafe { image_conversion_device_window_primary(&command) };
    } else {
        unsafe { image_conversion_device_window_secondary(&command) };
    }
    unsafe { image_conversion_device_enable(1) }; 0
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_conversion_begin(first: u32, second: u32) {
    unsafe { image_conversion_prepare_clock(0, 0); image_conversion_device_begin(first, second) };
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_conversion_get_parameters(parameters: *mut Address32) {
    unsafe { write_volatile(parameters, CONVERSION_STATE.offset(ByteOffset(8))) };
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_conversion_complete() {
    unsafe { image_conversion_device_reset() };
    let token = unsafe { xm1_develop_enter_critical() };
    let callback = unsafe { CONVERSION_STATE.offset(ByteOffset(0x40)).word() };
    unsafe { CONVERSION_STATE.offset(ByteOffset(0x40)).write_word(0); xm1_develop_leave_critical(token) };
    if callback != 0 {
        let callback: unsafe extern "C" fn() = unsafe { core::mem::transmute(callback as usize) };
        unsafe { callback() };
    }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_conversion_wait() -> i32 {
    let token = unsafe { xm1_develop_enter_critical() };
    unsafe { CONVERSION_STATE.offset(ByteOffset(0x40)).write_word(0); xm1_develop_leave_critical(token); image_conversion_device_reset() };
    if unsafe { CONVERSION_STATE.offset(ByteOffset(4)).word() } == 0 { return 0; }
    let result = unsafe { image_conversion_release_interrupt(CONVERSION_STATE.word()) };
    if result == 0 { unsafe { CONVERSION_STATE.offset(ByteOffset(4)).write_word(0) }; }
    result
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_conversion_submit(first: *const ConversionParameters, second: *const ConversionParameters, first_mode: u32, second_mode: u32, completed: Option<unsafe extern "C" fn()>) -> i32 {
    if let Some(callback) = completed {
        unsafe { CONVERSION_STATE.offset(ByteOffset(0x40)).write_word(callback as usize as u32) };
        let status = unsafe { image_conversion_register_interrupt(0x3b, xm1_pipeline_image_conversion_complete, CONVERSION_STATE.0 as *mut u32) };
        if status != 0 { return status; }
        unsafe { CONVERSION_STATE.offset(ByteOffset(4)).write_word(1) };
    }
    let first = unsafe { read_volatile(first) };
    let status = unsafe { xm1_pipeline_image_conversion_configure(&first) };
    if status != 0 { return status; }
    let second = unsafe { read_volatile(second) };
    let status = unsafe { xm1_pipeline_image_conversion_configure(&second) };
    if status != 0 { return status; }
    unsafe { xm1_pipeline_image_conversion_begin(first_mode, second_mode) }; 0
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_conversion_prepare_mode(mode: u32) -> i32 {
    let source = CONVERSION_STATE.offset(ByteOffset(8));
    let column = if unsafe { source.offset(ByteOffset(0x2c)).word() } == 0 { 0 } else { 24 };
    let row = unsafe { source.offset(ByteOffset(4)).word() };
    let status = unsafe { image_conversion_register_interrupt(0x3b, xm1_pipeline_image_conversion_complete, CONVERSION_STATE.0 as *mut u32) };
    if status != 0 { return status; }
    unsafe { CONVERSION_STATE.offset(ByteOffset(4)).write_word(1) };
    if mode == 0 { halt(); }
    if mode.wrapping_sub(1) > 1 { return 0; }
    let ratio = Address32(0x00da6330).offset(ByteOffset(row.wrapping_mul(48).wrapping_add(column)));
    let denominator = unsafe { ratio.offset(ByteOffset(4)).word() };
    let block = unsigned_divide(denominator, denominator);
    let mut command = ConversionParameters {
        mode: 3, block_size: block, value: 1, alternate: 1,
        offset: block.wrapping_mul(unsafe { source.offset(ByteOffset(0x30)).word() }.wrapping_shl(3)), reserved: [0; 6],
    };
    let status = unsafe { xm1_pipeline_image_conversion_configure(&command) };
    if status != 0 { return status; }
    command.block_size = unsigned_divide(unsafe { ratio.word() }, denominator);
    command.alternate = 0;
    command.offset = 0x0ffffff8;
    unsafe { xm1_pipeline_image_conversion_configure(&command) }
}

unsafe extern "C" { fn image_operation_divide_fault(value: u32) -> !; }

fn unsigned_divide(numerator: u32, denominator: u32) -> u32 {
    if denominator == 0 { unsafe { image_operation_divide_fault(if numerator == 0 { 0 } else { u32::MAX }) }; }
    numerator / denominator
}

#[repr(C)]
pub(super) struct GeometryRequest {
    operation: u32,
    centered: [Address32; 4],
    sources: [Address32; 5],
    strides: [RowBytes; 5],
    heights: [Pixels; 5],
    source_width: Pixels,
    destination: Address32,
    destination_stride: RowBytes,
    destination_width: Pixels,
    destination_height: Pixels,
    reserved: u32,
    count: u32,
    completed: unsafe extern "C" fn(),
    argument: Address32,
    table: Address32,
    coefficients: Address32,
    kind8_parameters: Address32,
    kind9_parameters: Address32,
    parameters: Address32,
}

unsafe extern "C" {
    fn xm1_pipeline_image_geometry_initialize() -> i32;
    fn xm1_pipeline_image_geometry_configure(request: *const GeometryRequest) -> i32;
    fn xm1_pipeline_image_geometry_begin();
    fn xm1_pipeline_image_geometry_wait() -> i32;
    fn xm1_pipeline_image_geometry_release() -> i32;
    fn xm1_pipeline_image_geometry_mode(operation: u32) -> u32;
    fn xm1_pipeline_image_geometry_debug(job: u32);
    fn xm1_pipeline_image_trace(text: Address32);
    fn xm1_pipeline_image_trace_begin(operation: u32);
    fn xm1_pipeline_image_trace_end(operation: u32);
    fn xm1_pipeline_image_current_task() -> u32;
    fn xm1_pipeline_image_configure_task(task: u32, mode: u32) -> i32;
    fn xm1_pipeline_image_wait_all(mask: u32, result: *mut u32, timeout: i32) -> i32;
    fn xm1_pipeline_image_send_notification(task: u32, bits: u32) -> i32;
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_operation_complete() {
    let task = unsafe { Address32(0x0157bd08).word() };
    if task != 0 { unsafe { xm1_pipeline_image_send_notification(task, 0x10) }; }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_operate(job: u32, operation: *const PlaneOperation, sources: *const *const ImagePlane, count: u32, destination: *mut ImagePlane) -> u32 {
    unsafe { xm1_pipeline_image_trace(Address32(0x00e66708)); xm1_pipeline_image_geometry_initialize() };
    let request_address = Address32(0x01575844);
    for offset in (0..0x88).step_by(4) {
        unsafe { request_address.offset(ByteOffset(offset)).write_word(0) };
    }
    let operation = Address32(operation as u32);
    let kind = unsafe { operation.word() };
    unsafe { request_address.write_word(kind) };
    if !sources.is_null() {
        for index in 0..5 {
            let source = unsafe { read_volatile(sources.add(index)) };
            let enabled = !source.is_null() && (count > index as u32 || (index == 3 && unsafe { xm1_pipeline_image_geometry_mode(kind) } == 3));
            if !enabled { continue; }
            let source = unsafe { read_volatile(source) };
            let mode = unsafe { xm1_pipeline_image_geometry_mode(kind) };
            if matches!(mode, 1 | 3 | 4) {
                if index == 4 { halt(); }
                let centered = source.address.0.wrapping_add(source.width.0 >> 1).wrapping_add(source.stride.0.wrapping_mul(source.height.0 >> 1));
                unsafe { request_address.offset(ByteOffset(4 + index as u32 * 4)).write_word(centered) };
            } else {
                unsafe { request_address.offset(ByteOffset(0x14 + index as u32 * 4)).write_word(source.address.0) };
            }
            unsafe {
                request_address.offset(ByteOffset(0x28 + index as u32 * 4)).write_word(source.stride.0);
                request_address.offset(ByteOffset(0x3c + index as u32 * 4)).write_word(source.height.0);
            }
        }
    }
    let first = unsafe { read_volatile(read_volatile(sources)) };
    let destination = unsafe { read_volatile(destination) };
    unsafe {
        request_address.offset(ByteOffset(0x50)).write_word(first.width.0);
        request_address.offset(ByteOffset(0x54)).write_word(destination.address.0);
        request_address.offset(ByteOffset(0x58)).write_word(destination.stride.0);
        request_address.offset(ByteOffset(0x5c)).write_word(destination.width.0);
        request_address.offset(ByteOffset(0x60)).write_word(destination.height.0);
        request_address.offset(ByteOffset(0x68)).write_word(count);
    }
    for index in 0..6 {
        let word = unsafe { operation.offset(ByteOffset(4 + index * 4)).word() };
        unsafe { request_address.offset(ByteOffset(0x70 + index * 4)).write_word(word) };
    }
    unsafe {
        request_address.offset(ByteOffset(0x6c)).write_word(xm1_pipeline_image_operation_complete as usize as u32);
        xm1_pipeline_image_geometry_configure(request_address.0 as *const GeometryRequest);
        xm1_pipeline_image_geometry_debug(job);
        xm1_pipeline_image_trace_begin(0x06023370);
        xm1_pipeline_image_trace(Address32(0x00e66710));
        Address32(0x0157bd08).write_word(xm1_pipeline_image_current_task());
        xm1_pipeline_image_geometry_begin();
    }
    let mut result = 0;
    unsafe {
        xm1_pipeline_image_wait_all(0x10, &mut result, -1);
        Address32(0x0157bd08).write_word(0);
        xm1_pipeline_image_geometry_wait();
        xm1_pipeline_image_trace(Address32(0x00e66718));
        xm1_pipeline_image_trace_end(0x06023370);
        xm1_pipeline_image_geometry_release();
        xm1_pipeline_image_trace(Address32(0x00e66720));
    }
    u32::from(result & 0x20 == 0)
}

unsafe fn mask_dimensions(job: Address32) -> (u32, u32) {
    let profile = unsafe { job.offset(ByteOffset(0x644)).word() };
    let aspect = unsafe { job.offset(ByteOffset(0x4f0)).word() };
    match aspect {
        0..=2 => {
            let width = unsafe { xm1_develop_lookup_width(profile, aspect) } >> 2;
            (width, width.wrapping_mul(2) / 3)
        }
        3 => {
            let height = unsafe { xm1_develop_lookup_height(profile, 3) } >> 2;
            (((height.wrapping_mul(3) >> 1).wrapping_add(7)) & !7, height)
        }
        _ => halt(),
    }
}

fn radial_square(position: u32, center: u32) -> u64 {
    let delta = (position as i32 as i64).wrapping_sub(center as i64);
    let square = delta.wrapping_mul(delta);
    if square < 0 { 0 } else { square as u64 }
}

fn a32_shift_right(value: u64, shift: u32) -> u32 {
    let low = value as u32;
    let high = (value >> 32) as u32;
    fn lsr(value: u32, shift: u32) -> u32 { let shift = shift & 255; if shift < 32 { value >> shift } else { 0 } }
    fn lsl(value: u32, shift: u32) -> u32 { let shift = shift & 255; if shift < 32 { value << shift } else { 0 } }
    if (shift.wrapping_sub(32) as i32) >= 0 { lsr(high, shift.wrapping_sub(32)) }
    else { lsr(low, shift) | lsl(high, 32u32.wrapping_sub(shift)) }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_prepare_three(job: u32, plane: *mut ImagePlane) {
    let task = unsafe { xm1_pipeline_image_current_task() };
    unsafe { xm1_pipeline_image_cache_maintenance(); xm1_pipeline_image_configure_task(task, 12) };
    let job = Address32(job);
    let (width, height) = unsafe { mask_dimensions(job) };
    let aspect = unsafe { job.offset(ByteOffset(0x4f0)).word() };
    let (normal_width, normal_height) = match aspect {
        0 => (width, height),
        1 => (width, width.wrapping_mul(2) / 3),
        2 => (width, width.wrapping_mul(9) >> 4),
        3 => (height, height),
        _ => halt(),
    };
    let strength = unsafe { xm1_buffers_configuration_byte(ConfigurationKey(0x8c00)) } & 255;
    let denominator = normal_width.wrapping_mul(normal_width).wrapping_add(normal_height.wrapping_mul(normal_height));
    if denominator == 0 { unsafe { image_operation_divide_fault(if strength == 0 { 0 } else { u32::MAX }) }; }
    let factor = (strength as u64 * 10_000_000) / denominator as u64;
    let mut output = unsafe { read_volatile(plane) };
    let mut row = 0u32;
    loop {
        let y2 = radial_square(row, height >> 1);
        if width != u32::MAX {
            let mut column = 0u32;
            loop {
                let distance = radial_square(column, width >> 1).wrapping_add(y2);
                let value = distance.wrapping_mul(factor);
                let shift = unsafe { xm1_buffers_configuration_byte(ConfigurationKey(0x8c01)) };
                let value = a32_shift_right(value, shift).min(255);
                unsafe { output.address.offset(ByteOffset(output.stride.0.wrapping_mul(row).wrapping_add(column))).write_byte(value as u8) };
                column = column.wrapping_add(1);
                if column >= width.wrapping_add(1) { break; }
            }
        }
        row = row.wrapping_add(1);
        if row == height.wrapping_add(1) { break; }
    }
    output.width = Pixels(width);
    output.height = Pixels(height);
    output.size = output.stride.0.wrapping_mul(height);
    unsafe { write_volatile(plane, output); xm1_pipeline_image_configure_task(task, 6) };
    if unsafe { xm1_buffers_configuration_byte(ConfigurationKey(0xa8)) } & 0x80 != 0 {
        let debug = Address32(unsafe { job.offset(ByteOffset(0x220c)).word() });
        unsafe {
            debug.offset(ByteOffset(0xdd414)).write_word(normal_width);
            debug.offset(ByteOffset(0xdd418)).write_word(normal_height);
            debug.offset(ByteOffset(0xdd41c)).write_word(factor as u32);
        }
    }
    unsafe { xm1_pipeline_image_cache_maintenance() };
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_merge(job: u32, planes: *const *const ImagePlane, count: u32, destination: *mut ImagePlane) -> u32 {
    if count != 4 { halt(); }
    let first = unsafe { read_volatile(planes) };
    let sources = [first, unsafe { read_volatile(planes.add(1)) }, unsafe { read_volatile(planes.add(2)) }, unsafe { read_volatile(planes.add(3)) }, first];
    let table = Address32(0x01574780);
    for index in 0..256 {
        let entry = table.offset(ByteOffset(index * 12));
        unsafe { entry.offset(ByteOffset(4)).write_word(3); entry.write_word(3); entry.offset(ByteOffset(8)).write_word(0); }
    }
    let mut operation = PlaneOperation::new(PlaneOperationKind::Merge, core::ptr::null());
    operation.table = table;
    let mut work = unsafe { shaped(first, destination) };
    unsafe { xm1_pipeline_image_operate(job, &operation, sources.as_ptr(), 5, &mut work); write_volatile(destination, work) }; 1
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_difference(job: u32, source: *const ImagePlane, second: *const ImagePlane, destination: *mut ImagePlane, alternate: u32) -> u32 {
    let mut sources = [source, second, core::ptr::null(), core::ptr::null(), core::ptr::null()];
    let mut coefficients = [256, -256, 0, 0];
    let operation = PlaneOperation::new(PlaneOperationKind::LinearCombination, coefficients.as_ptr());
    let mut work = unsafe { shaped(source, destination) };
    unsafe { xm1_pipeline_image_operate(job, &operation, sources.as_ptr(), 2, &mut work) };
    sources[..4].fill(core::ptr::null());
    let key = if alternate == 0 { 0x8cbd } else { 0x8cc4 };
    let count = unsafe { xm1_buffers_configuration_byte(ConfigurationKey(key)) };
    for index in 0..count { unsafe { sources.as_mut_ptr().add(index as usize).write(&work) }; }
    coefficients[0] = 256;
    for index in 0..3 {
        coefficients[index + 1] = unsafe { xm1_buffers_configuration_half(ConfigurationKey(key + 1 + index as u32 * 2)) } as i32;
    }
    unsafe { xm1_pipeline_image_operate(job, &operation, sources.as_ptr(), count, &mut work); write_volatile(destination, work) }; 1
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_merge_finish(job: u32, source: *const ImagePlane, destination: *mut ImagePlane) -> u32 {
    let source = unsafe { read_volatile(source) };
    if source.flags & 0x100000 == 0 { halt(); }
    let mut input = source;
    let mut output = unsafe { read_volatile(destination) };
    let mut parameters = [0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 68, 56, 30, 8];
    unsafe { xm1_pipeline_image_pad_edges(input.address, input.stride, input.width, input.height, 0, 2, 0, 2) };
    input.width.0 = input.width.0.wrapping_add(4);
    input.height.0 = input.height.0.wrapping_add(2);
    let half_width = input.width.0 / 2;
    output.width = Pixels(((half_width.wrapping_add(1) >> 1).wrapping_add(15) / 16).wrapping_mul(16));
    output.height = Pixels(input.height.0.wrapping_add(1) >> 1);
    output.size = output.stride.0.wrapping_mul(output.height.0);
    parameters[3] = output.width.0.wrapping_add(31) & !31;
    let right = output.width.0.wrapping_mul(2).wrapping_sub(half_width);
    let bottom = output.height.0.wrapping_mul(2).wrapping_sub(input.height.0);
    if ((right | bottom) as i32) < 0 { halt(); }
    unsafe { xm1_pipeline_image_pad_edges(input.address, input.stride, input.width, input.height, 0, right, 0, bottom) };
    input.width.0 = input.width.0.wrapping_add(right.wrapping_mul(2));
    input.height.0 = input.height.0.wrapping_add(bottom);
    unsafe { xm1_pipeline_image_process_crop(job, parameters.as_ptr(), &input, &mut output) };
    output.width = Pixels(source.width.0 >> 2);
    output.height = Pixels(source.height.0 >> 1);
    unsafe { write_volatile(destination, output) }; 1
}


#[unsafe(no_mangle)]
pub(super) extern "C" fn xm1_pipeline_image_integer_root(value: i32) -> i32 {
    if value == 0 { return 0; }
    let mut root = 1i32;
    for _ in 0..51 {
        if root == 0 { return -1; }
        root = value.wrapping_div(root).wrapping_add(root).wrapping_div(2);
        if value >= root.wrapping_mul(root) && value < root.wrapping_add(1).wrapping_mul(root.wrapping_add(1)) { return root; }
    }
    root
}

unsafe fn configuration(key: u32) -> i32 {
    (unsafe { xm1_buffers_configuration_byte(ConfigurationKey(key)) }) as i32
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_prepare_four(job: u32, plane: *mut ImagePlane) {
    let task = unsafe { xm1_pipeline_image_current_task() };
    unsafe { xm1_pipeline_image_cache_maintenance(); xm1_pipeline_image_configure_task(task, 12) };
    let job = Address32(job);
    let field = |offset| unsafe { job.offset(ByteOffset(offset)).word() };
    let profile = field(0x644);
    let aspect = field(0x4f0);
    let width = unsafe { xm1_develop_lookup_width(profile, aspect) } as i32;
    let height = unsafe { xm1_develop_lookup_height(profile, aspect) } as i32;
    let (horizontal, vertical) = if field(0x534) == 0 {
        let horizontal_mode = unsafe { job.offset(ByteOffset(0x540)).byte() } as i8 as i32 as u32;
        let vertical_mode = unsafe { job.offset(ByteOffset(0x541)).byte() } as i8 as i32 as u32;
        ((unsafe { xm1_pipeline_image_processed_width(profile, aspect, horizontal_mode) } as i32).wrapping_sub(width / 2) / 4,
         (unsafe { xm1_pipeline_image_processed_height(profile, aspect, vertical_mode) } as i32).wrapping_sub(height / 2) / 4)
    } else if field(0x534) == 7 || field(0x74c) == 0 || field(0x5a0) != 1 || field(0x169c) == 0 || field(0x2734) != 1 || field(0x16b8) != 0 {
        let horizontal = unsafe { job.offset(ByteOffset(0x698)).byte() } as i8 as i32;
        let vertical = unsafe { job.offset(ByteOffset(0x699)).byte() } as i8 as i32;
        (width.wrapping_mul(horizontal).wrapping_mul(116) / 4000,
         height.wrapping_neg().wrapping_mul(vertical).wrapping_mul(116) / 4000)
    } else {
        for offset in 0..0xb8 {
            let value = unsafe { job.offset(ByteOffset(0x16a0 + offset)).byte() };
            unsafe { Address32(0x01575384).offset(ByteOffset(offset)).write_byte(value) };
        }
        let bounds = Address32(0x01575384);
        let coordinate = |a, b, extent: i32| {
            let sum = (unsafe { bounds.offset(ByteOffset(a)).word() } as i32).wrapping_add(unsafe { bounds.offset(ByteOffset(b)).word() } as i32);
            ((sum.wrapping_mul(50) / 10).wrapping_add(5) / 10).wrapping_sub(extent / 2) / 4
        };
        (coordinate(0, 4, width), coordinate(8, 12, height))
    };
    let (mask_width, mask_height) = unsafe { mask_dimensions(job) };
    let calibration = unsafe { xm1_buffers_configuration_half(ConfigurationKey(0x8c13)) } as i32;
    let scale = xm1_pipeline_image_integer_root(mask_height.wrapping_shl(4) as i32);
    let normalized = unsigned_divide(mask_height.wrapping_mul(10000).wrapping_add((calibration / 2) as u32), calibration as u32);
    let baseline = xm1_pipeline_image_integer_root(normalized as i32).wrapping_mul(255).wrapping_add(50) / 100;
    let first_slope = unsafe { configuration(0x8c0f) };
    let second_slope = unsafe { configuration(0x8c10) };
    let first_offset = unsafe { configuration(0x8c11) };
    let second_offset = unsafe { configuration(0x8c12) };
    let adjustment = match field(0x4e8) {
        0 => unsafe { configuration(0x8c19) },
        1 => unsafe { configuration(0x8c18) },
        2 => unsafe { configuration(0x8c17) },
        _ => halt(),
    };
    let maximum = adjustment.wrapping_add(baseline).clamp(0, 255);
    let slope_a = scale.wrapping_mul(first_slope).wrapping_add(2) / 4;
    let slope_b = scale.wrapping_mul(second_slope).wrapping_add(2) / 4;
    let offset_a = (slope_a.wrapping_mul(first_offset) / 100).wrapping_neg();
    let offset_b = (slope_b.wrapping_mul(second_offset) / 100).wrapping_neg();
    let orientation = field(0x1604);
    let (slope_a, slope_b, offset_a, offset_b) = match orientation {
        0..=2 => (slope_a, slope_b, offset_a, offset_b),
        3..=4 => (slope_b, slope_a, offset_b, offset_a),
        _ => halt(),
    };
    let transposed = matches!(orientation, 2 | 4);
    let pivot = if transposed { (horizontal as u32).wrapping_add(mask_width >> 1) } else { (vertical as u32).wrapping_add(mask_height >> 1) };
    let extent = if transposed { mask_width } else { mask_height };
    let mut output = unsafe { read_volatile(plane) };
    let mut numerator = (slope_a as u32).wrapping_mul(pivot);
    let mut position = 0u32;
    loop {
        let value = (unsigned_divide(numerator, extent).wrapping_add(offset_a as u32) as i32).clamp(0, maximum) as u8;
        unsafe { write_mask_line(&output, mask_width, mask_height, position, transposed, value) };
        numerator = numerator.wrapping_sub(slope_a as u32);
        position = position.wrapping_add(1);
        if position > pivot { break; }
    }
    numerator = slope_b as u32;
    while position < extent.wrapping_add(1) {
        let value = (unsigned_divide(numerator, extent).wrapping_add(offset_b as u32) as i32).clamp(0, maximum) as u8;
        unsafe { write_mask_line(&output, mask_width, mask_height, position, transposed, value) };
        numerator = numerator.wrapping_add(slope_b as u32);
        position = position.wrapping_add(1);
    }
    output.width = Pixels(mask_width);
    output.height = Pixels(mask_height);
    output.size = output.stride.0.wrapping_mul(mask_height);
    unsafe { write_volatile(plane, output); xm1_pipeline_image_configure_task(task, 6) };
    if unsafe { configuration(0xa8) } & 0x80 != 0 {
        let debug = Address32(field(0x220c));
        unsafe { debug.offset(ByteOffset(0xdd420)).write_word(mask_width); debug.offset(ByteOffset(0xdd424)).write_word(mask_height) };
    }
    unsafe { xm1_pipeline_image_cache_maintenance() };
}

unsafe fn write_mask_line(plane: &ImagePlane, width: u32, height: u32, position: u32, transposed: bool, value: u8) {
    let extent = if transposed { height } else { width };
    if extent == u32::MAX { return; }
    for index in 0..=extent {
        let (x, y) = if transposed { (position, index) } else { (index, position) };
        unsafe { plane.address.offset(ByteOffset(y.wrapping_mul(plane.stride.0).wrapping_add(x))).write_byte(value) };
    }
}

unsafe extern "C" {
    fn xm1_pipeline_image_configuration_signed_byte(key: ConfigurationKey) -> i32;
    fn xm1_pipeline_image_configuration_signed_half(key: ConfigurationKey) -> i32;
}

#[derive(Clone, Copy)]
#[repr(C)]
struct NoiseTriplet { first: u32, second: u32, weight: u32 }

unsafe fn triplet_debug(job: Address32, table: *const NoiseTriplet, bank: u32) {
    if unsafe { configuration(0xa8) } & 0x80 == 0 { return; }
    for index in 0..256u32 {
        let triplet = unsafe { read_volatile(table.add(index as usize)) };
        let slot = bank.wrapping_mul(0x300).wrapping_add(index);
        let debug = Address32(unsafe { job.offset(ByteOffset(0x220c)).word() });
        unsafe { debug.offset(ByteOffset(slot.wrapping_add(0x3750a).wrapping_mul(4))).write_word(triplet.second) };
        let debug = Address32(unsafe { job.offset(ByteOffset(0x220c)).word() });
        unsafe { debug.offset(ByteOffset(slot.wrapping_add(0x3760a).wrapping_mul(4))).write_word(triplet.first) };
        let debug = Address32(unsafe { job.offset(ByteOffset(0x220c)).word() });
        unsafe { debug.offset(ByteOffset(slot.wrapping_add(0x3770a).wrapping_mul(4))).write_word(triplet.weight) };
    }
}

unsafe fn blend_triplets(job: u32, table: *mut NoiseTriplet, magnitude: u32, alternate: u32) {
    let (first, second, weight) = if alternate != 0 {
        (2, 0, unsafe { configuration(0x8cda) })
    } else {
        let scale = u32::from(unsafe { xm1_buffers_configuration_half(ConfigurationKey(0x8cd8)) });
        let level = xm1_pipeline_image_integer_root(unsigned_divide(magnitude.wrapping_mul(0xfe01), scale) as i32).min(255);
        let upper = if level < 51 { 1 } else if level < 102 { 2 } else if level < 153 { 3 } else if level < 204 { 4 } else { 5 };
        let lower = upper - 1;
        let first = match upper - 2 { value if value < 0 => 0, value if value > 2 => 3, value => value };
        let second = if upper < 3 { 0 } else { lower - 2 };
        let intervals = Address32(0x00e63d28);
        let bottom = unsafe { intervals.offset(ByteOffset(lower as u32 * 4)).word() } as i32;
        let top = unsafe { intervals.offset(ByteOffset(upper as u32 * 4)).word() } as i32;
        let denominator = top.wrapping_sub(bottom);
        if denominator == 0 { unsafe { image_operation_divide_fault(level.wrapping_sub(bottom).wrapping_mul(256) as u32) }; }
        let weight = level.wrapping_sub(bottom).wrapping_mul(256).wrapping_div(denominator).clamp(0, 255);
        (first, second, weight)
    };
    for index in 0..256 {
        unsafe { write_volatile(table.add(index), NoiseTriplet { first: first as u32, second: second as u32, weight: weight as u32 }) };
    }
    unsafe { triplet_debug(Address32(job), table, alternate) };
}

unsafe fn noise_triplets(job: u32, table: *mut NoiseTriplet, bank: u32) {
    let key = |index: u32| ConfigurationKey(unsafe { Address32(0x00e63d40).offset(ByteOffset(index.wrapping_add(bank).wrapping_mul(4))).word() });
    for position in 0..256u32 {
        let level = |base| {
            let mut selected = 3;
            for index in 0..3 {
                if unsafe { xm1_buffers_configuration_byte(key(base + index * 2)) } >= position { selected = index; break; }
            }
            selected
        };
        let second = level(0);
        let first = level(6);
        let mut interval = 5;
        for index in 0..5 {
            if unsafe { xm1_buffers_configuration_byte(key(12 + index * 2)) } >= position { interval = index; break; }
        }
        let slope = unsafe { xm1_pipeline_image_configuration_signed_byte(key(22 + interval * 2)) };
        let intercept = unsafe { xm1_pipeline_image_configuration_signed_half(key(34 + interval * 4)) };
        let weight = (position as i32).wrapping_mul(slope).wrapping_add(intercept).clamp(0, 255) as u32;
        unsafe { write_volatile(table.add(position as usize), NoiseTriplet { first, second, weight }) };
    }
    unsafe { triplet_debug(Address32(job), table, bank) };
}

#[derive(Clone, Copy)]
enum CoefficientPolicy { Magnitude, Noise }

unsafe fn select_six(job: u32, planes: *const *const ImagePlane, count: u32, mask: *const ImagePlane, layout: *const ImagePlane, destination: *mut ImagePlane, policy: CoefficientPolicy) -> u32 {
    if count != 6 { halt(); }
    let plane = |index| unsafe { read_volatile(planes.add(index)) };
    let mut sources = [plane(2), plane(3), plane(4), plane(5), if matches!(policy, CoefficientPolicy::Magnitude) { plane(2) } else { mask }];
    let table = match policy { CoefficientPolicy::Magnitude => Address32(0x0157393c), CoefficientPolicy::Noise => Address32(0x01572d3c) };
    let mut work = unsafe { shaped(plane(2), layout) };
    let mut operation = PlaneOperation::new(PlaneOperationKind::Select, core::ptr::null());
    operation.table = table;
    let magnitude = unsafe { read_volatile(core::ptr::addr_of!((*plane(0)).width)) }.0 >> 1;
    match policy {
        CoefficientPolicy::Magnitude => unsafe { blend_triplets(job, table.0 as *mut NoiseTriplet, magnitude, 0) },
        CoefficientPolicy::Noise => unsafe { noise_triplets(job, table.0 as *mut NoiseTriplet, 0) },
    }
    unsafe { xm1_pipeline_image_operate(job, &operation, sources.as_ptr(), 5, &mut work) };
    sources = [plane(0), plane(1), &work, core::ptr::null(), if matches!(policy, CoefficientPolicy::Magnitude) { plane(2) } else { mask }];
    let mut output = unsafe { shaped(plane(0), destination) };
    operation.kind = PlaneOperationKind::Combine;
    match policy {
        CoefficientPolicy::Magnitude => unsafe { blend_triplets(job, table.0 as *mut NoiseTriplet, magnitude, 1) },
        CoefficientPolicy::Noise => unsafe { noise_triplets(job, table.0 as *mut NoiseTriplet, 1) },
    }
    unsafe { xm1_pipeline_image_operate(job, &operation, sources.as_ptr(), 5, &mut output); write_volatile(destination, output); center(Address32(job), destination) }; 1
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_select_thirteen(job: u32, planes: *const *const ImagePlane, count: u32, layout: *const ImagePlane, destination: *mut ImagePlane) -> u32 {
    unsafe { select_six(job, planes, count, core::ptr::null(), layout, destination, CoefficientPolicy::Magnitude) }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_select_four(job: u32, planes: *const *const ImagePlane, count: u32, mask: *const ImagePlane, layout: *const ImagePlane, destination: *mut ImagePlane) -> u32 {
    unsafe { select_six(job, planes, count, mask, layout, destination, CoefficientPolicy::Noise) }
}

#[derive(Clone, Copy)]
#[repr(transparent)]
struct DmaChannel(u32);

#[repr(C)]
struct DmaTransfer { control: u32, reserved: [u32; 2], source: Address32, destination: Address32, bytes: u32, tail: [u32; 5] }
#[repr(C)]
struct DmaCompletion { mode: u32, callback: unsafe extern "C" fn() }

unsafe extern "C" {
    fn image_dma_acquire(channel: u32, handle: *mut DmaChannel) -> i32;
    fn image_dma_release(handle: DmaChannel) -> i32;
    fn image_dma_configure(handle: DmaChannel, request: *const DmaTransfer) -> i32;
    fn image_dma_begin(handle: DmaChannel, completed: *const DmaCompletion) -> i32;
    fn xm1_pipeline_image_dma_wait(handle: i32) -> i32;
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_dma_complete() {
    let task = unsafe { Address32(0x0157bd0c).word() };
    if task != 0 { unsafe { xm1_pipeline_image_send_notification(task, 0x10) }; }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_fill_word(plane: *const ImagePlane, value: u32) -> u32 {
    unsafe { xm1_pipeline_image_trace(Address32(0x00e66728)) };
    let plane = unsafe { read_volatile(plane) };
    let mut bytes = plane.width.0;
    let mut rows = plane.height.0;
    if bytes == plane.stride.0 { bytes = bytes.wrapping_mul(rows); rows = 1; }
    let words = bytes >> 2;
    for index in 0..8 { unsafe { Address32(0x4157a0e0).offset(ByteOffset(index * 4)).write_word(value) }; }
    let request = Address32(0x0157b9d0);
    for offset in (0..0x2c).step_by(4) { unsafe { request.offset(ByteOffset(offset)).write_word(0) }; }
    unsafe {
        request.offset(ByteOffset(1)).write_byte(4);
        request.offset(ByteOffset(2)).write_byte(0x50);
        request.offset(ByteOffset(12)).write_word(0x0157a0e0);
        request.offset(ByteOffset(20)).write_word(words << 2);
        Address32(0x0157b9fc).write_word(4);
        Address32(0x0157ba00).write_word(xm1_pipeline_image_dma_complete as usize as u32);
        xm1_pipeline_image_trace_begin(0x060233a0);
        xm1_pipeline_image_trace(Address32(0x00e66734));
    }
    let mut channel = DmaChannel(0);
    unsafe { image_dma_acquire(5, &mut channel) };
    let mut success = 1;
    let mut destination = plane.address;
    let mut result = 0;
    for _ in 0..rows {
        let alignment = if destination.0 & 31 == 0 && words & 7 == 0 { 5 }
            else if destination.0 & 15 == 0 && words & 3 == 0 { 4 }
            else if destination.0 & 7 == 0 && words & 1 == 0 { 3 } else { 2 };
        unsafe {
            request.offset(ByteOffset(1)).write_byte(4 | alignment << 4);
            request.offset(ByteOffset(2)).write_byte(0x50 | alignment);
            request.offset(ByteOffset(16)).write_word(destination.0);
            image_dma_configure(channel, request.0 as *const DmaTransfer);
            Address32(0x0157bd0c).write_word(xm1_pipeline_image_current_task());
            image_dma_begin(channel, 0x0157b9fc as *const DmaCompletion);
            xm1_pipeline_image_wait_all(0x10, &mut result, -1);
            Address32(0x0157bd0c).write_word(0);
        }
        success = u32::from(result & 0x20 == 0);
        unsafe { xm1_pipeline_image_dma_wait(channel.0 as i32) };
        if success == 0 { break; }
        destination = destination.offset(ByteOffset(plane.stride.0));
    }
    unsafe {
        image_dma_release(channel);
        xm1_pipeline_image_trace_end(0x060233a0);
        xm1_pipeline_image_trace(Address32(0x00e6673c));
        xm1_pipeline_image_trace(Address32(0x00e66744));
    }
    success
}

unsafe extern "C" {
    fn xm1_pipeline_image_resize_planes(job: u32, enabled: i32, mode: i32, source: *const ImagePlane, destination: *mut ImagePlane, auxiliary: *const ImagePlane, configuration: *mut u32, operation: i32) -> u32;
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_next_noise() -> u32 {
    let x = unsafe { Address32(0x00e63e30).word() };
    let mixed = x ^ x.wrapping_shl(11);
    let y = unsafe { Address32(0x00e63d24).word() };
    unsafe { Address32(0x00e63e30).write_word(y) };
    let z = unsafe { Address32(0x00e63e28).word() };
    unsafe { Address32(0x00e63d24).write_word(z) };
    let w = unsafe { Address32(0x00e63e2c).word() };
    unsafe { Address32(0x00e63e28).write_word(w) };
    let next = w ^ (w >> 19) ^ mixed ^ (mixed >> 8);
    unsafe { Address32(0x00e63e2c).write_word(next) };
    next
}

unsafe fn add_noise(address: Address32, stride: RowBytes, width: Pixels, height: Pixels) {
    let level = unsafe { configuration(0x8ccb) };
    let bias = level.wrapping_shl(16) / 100;
    let mut row_address = address;
    for _ in 0..height.0 {
        let mut column = 0u32;
        while column < width.0 {
            for channel in 0..2 {
                let pixel = row_address.offset(ByteOffset(column.wrapping_add(channel)));
                let old = unsafe { pixel.byte() } as i32;
                let random = unsafe { xm1_pipeline_image_next_noise() };
                let fixed = bias.wrapping_add((random >> 16) as i32);
                let increment = if fixed < 0 { fixed.wrapping_add(0xffff) } else { fixed } >> 16;
                let value = old.wrapping_add(increment).min(255) as u8;
                unsafe { pixel.write_byte(value) };
            }
            column = column.wrapping_add(4);
        }
        row_address = row_address.offset(ByteOffset(column.wrapping_add(stride.0.wrapping_sub(width.0))));
    }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_prepare_finish(job: u32, source: *const ImagePlane, scratch: *const ImagePlane, destination: *mut ImagePlane) -> u32 {
    let mut input = unsafe { read_volatile(source) };
    let mut output = unsafe { read_volatile(destination) };
    let mut auxiliary = input;
    let scratch_template = unsafe { read_volatile(scratch) };
    let mut intermediate = scratch_template;
    intermediate.address = intermediate.address.offset(ByteOffset(intermediate.stride.0.wrapping_mul(intermediate.height.0.wrapping_sub(426) >> 1)));
    let width = unsafe { xm1_develop_lookup_width(4, 0) }.wrapping_mul(2);
    intermediate.width = Pixels(width);
    intermediate.height = Pixels(width / 3);
    intermediate.size = width.wrapping_mul(intermediate.height.0);
    let job_address = Address32(job);
    let aspect = unsafe { job_address.offset(ByteOffset(0x4f0)).word() };
    let profile = unsafe { job_address.offset(ByteOffset(0x644)).word() };
    let mode = if aspect == 3 {
        output.height = Pixels(unsafe { xm1_develop_lookup_height(profile, aspect) });
        output.width = Pixels((output.height.0.wrapping_mul(3) & !1).wrapping_add(31) & !31);
        2
    } else {
        output.width = Pixels(unsafe { xm1_develop_lookup_width(profile, aspect) }.wrapping_mul(2));
        output.height = Pixels(output.width.0 / 3);
        1
    };
    auxiliary.address = Address32(input.address.0.wrapping_sub(input.stride.0.wrapping_add(2).wrapping_mul(2)));
    auxiliary.width.0 = auxiliary.width.0.wrapping_add(8);
    auxiliary.height.0 = auxiliary.height.0.wrapping_add(4);
    auxiliary.size = auxiliary.stride.0.wrapping_mul(auxiliary.height.0);
    if auxiliary.width.0 > auxiliary.stride.0 { halt(); }
    unsafe { xm1_pipeline_image_resize_planes(job, 0, 3, &input, &mut intermediate, &auxiliary, core::ptr::null_mut(), 0) };
    input = intermediate;
    auxiliary = scratch_template;
    unsafe {
        add_noise(input.address, input.stride, input.width, input.height);
        xm1_pipeline_image_resize_planes(job, 0, mode, &input, &mut output, &auxiliary, core::ptr::null_mut(), 0);
        write_volatile(destination, output);
    }
    1
}

#[repr(C)]
struct NoisePalette {
    tables: [Address32; 4],
    width: u8,
    height: u8,
    first: u16,
    second: u16,
    enabled: [u8; 4],
}

#[repr(C)]
struct NoiseDimensions { enabled: u8, reserved: u8, width: u16, height: u16, third: u16, fourth: u16 }

#[repr(C)]
struct NoiseCoefficients { enabled: u8, reserved: u8, adjustment: i16, values: [u16; 9] }

#[repr(C)]
struct NoiseConfiguration { enabled: u8, bytes: [u8; 9], halves: [u16; 30] }

#[repr(C)]
struct NoiseFallback { enabled: u8, reserved: u8, offsets: [i16; 4] }

unsafe extern "C" {
    fn xm1_pipeline_image_prepare_noise_tables(job: u32, scan: Address32, options: Address32);
    fn xm1_pipeline_image_finalize_noise_tables(job: u32, scan: Address32, options: Address32);
    fn xm1_pipeline_image_write_noise_pixels(address: Address32, stride: RowBytes, palette: *const NoisePalette, offset: u32, dimensions: *const NoiseDimensions, coefficients: *const NoiseCoefficients, configuration: *const NoiseConfiguration, fallback: *const NoiseFallback);
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_prepare(job: u32, plane: *mut ImagePlane) {
    let mut output = unsafe { read_volatile(plane) };
    let address = output.address.offset(ByteOffset(output.stride.0.wrapping_mul(2).wrapping_add(4)));
    let task = unsafe { xm1_pipeline_image_current_task() };
    unsafe { xm1_pipeline_image_cache_maintenance(); xm1_pipeline_image_configure_task(task, 12) };
    let table = Address32(0x0157543c);
    let palette = NoisePalette {
        tables: [table, table.offset(ByteOffset(0x100)), table.offset(ByteOffset(0x200)), table.offset(ByteOffset(0x300))],
        width: unsafe { table.offset(ByteOffset(0x400)).byte() }, height: unsafe { table.offset(ByteOffset(0x401)).byte() },
        first: unsafe { table.offset(ByteOffset(0x402)).half() }, second: unsafe { table.offset(ByteOffset(0x404)).half() }, enabled: [1; 4],
    };
    let job_address = Address32(job);
    let offset = unsafe { job_address.offset(ByteOffset(0x1658)).word() }.wrapping_mul(10) & 255;
    let dimensions = NoiseDimensions {
        enabled: 1, reserved: 0,
        width: unsafe { job_address.offset(ByteOffset(0x2226)).half() },
        height: unsafe { job_address.offset(ByteOffset(0x2224)).half() },
        third: unsafe { job_address.offset(ByteOffset(0x2228)).half() },
        fourth: unsafe { job_address.offset(ByteOffset(0x2224)).half() },
    };
    let scan = Address32(0x01571adc);
    unsafe { xm1_pipeline_image_prepare_noise_tables(job, scan, Address32(0x0157453c)) };
    let mut coefficients = NoiseCoefficients {
        enabled: 1, reserved: 0, adjustment: unsafe { xm1_pipeline_image_configuration_signed_half(ConfigurationKey(0xae53)) } as i16, values: [0; 9],
    };
    for index in 0..9 { coefficients.values[index] = unsafe { scan.offset(ByteOffset(0xd4 + index as u32 * 2)).half() }; }
    let mut configuration = NoiseConfiguration { enabled: 1, bytes: [0; 9], halves: [0; 30] };
    for index in 0..9 { configuration.bytes[index] = unsafe { self::configuration(0x8c78 + index as u32) } as u8; }
    for index in 0..30 { configuration.halves[index] = unsafe { xm1_buffers_configuration_half(ConfigurationKey(0x8c81 + index as u32 * 2)) } as u16; }
    unsafe { xm1_pipeline_image_finalize_noise_tables(job, scan, Address32(0x0157453c)) };
    let mut fallback = NoiseFallback { enabled: 1, reserved: 0, offsets: [0; 4] };
    if unsafe { self::configuration(0x8cfc) } != 0 {
        for index in 0..4 { fallback.offsets[index] = unsafe { scan.offset(ByteOffset(0xf4a + index as u32 * 2)).half() } as i16; }
    }
    unsafe { xm1_pipeline_image_write_noise_pixels(address, output.stride, &palette, offset, &dimensions, &coefficients, &configuration, &fallback) };
    output.address = address;
    output.width = Pixels((palette.width as u32) << 1);
    output.height = Pixels(palette.height as u32);
    output.size = output.stride.0.wrapping_mul(output.height.0);
    unsafe {
        write_volatile(plane, output);
        xm1_pipeline_image_pad_edges(address, output.stride, output.width, output.height, 2, 2, 2, 2);
        noise_output_debug(job_address, &output, &palette, offset, &dimensions, &coefficients, &fallback);
        xm1_pipeline_image_configure_task(task, 6);
        xm1_pipeline_image_cache_maintenance();
    }
}

unsafe fn noise_output_debug(job: Address32, plane: &ImagePlane, palette: &NoisePalette, offset: u32, dimensions: &NoiseDimensions, coefficients: &NoiseCoefficients, fallback: &NoiseFallback) {
    if unsafe { configuration(0xa8) } & 0x80 == 0 { return; }
    let debug = || Address32(unsafe { job.offset(ByteOffset(0x220c)).word() });
    for index in 0..64u32 {
        for channel in 0..4usize {
            let value = unsafe { palette.tables[channel].offset(ByteOffset(index * 4)).word() };
            unsafe { debug().offset(ByteOffset((0x37b0a + channel as u32 * 64 + index) * 4)).write_word(value) };
        }
    }
    let header = [palette.width as u32, palette.height as u32, palette.first as u32, palette.second as u32, offset,
        dimensions.width as u32, dimensions.height as u32, dimensions.third as u32, dimensions.fourth as u32];
    for (index, value) in header.into_iter().enumerate() { unsafe { debug().offset(ByteOffset(0xdf028 + index as u32 * 4)).write_word(value) }; }
    for index in 0..9 { unsafe { debug().offset(ByteOffset(0xdf04c + index as u32 * 4)).write_word(coefficients.values[index] as i16 as i32 as u32) }; }
    for index in 0..4 { unsafe { debug().offset(ByteOffset(0xdf070 + index as u32 * 4)).write_word(fallback.offsets[index] as i32 as u32) }; }
    for y in 0..8u32 {
        for x in 0..16u32 {
            let value = unsafe { plane.address.offset(ByteOffset(plane.stride.0.wrapping_mul(y).wrapping_add(x))).byte() };
            unsafe { debug().offset(ByteOffset((0x37c20 + y * 16 + x) * 4)).write_word(value as u32) };
        }
    }
}


unsafe fn processed_dimensions(profile: u32, aspect: u32, horizontal: u32, vertical: u32) -> (u16, u16) {
    let width = unsafe { xm1_develop_lookup_width(profile, aspect) };
    let height = unsafe { xm1_develop_lookup_height(profile, aspect) };
    let source_width = if aspect == 3 { height.wrapping_mul(3) >> 1 } else { width };
    let source_height = if aspect == 2 { width.wrapping_mul(2) / 3 } else { height };
    let ratio = match aspect { 1 => (2, 3), 2 => (9, 16), 3 => (1, 1), _ => (3, 4) };
    let normalized = if matches!(aspect, 2 | 3) { (2, 3) } else { ratio };
    let fit = |(numerator, denominator): (u32, u32)| {
        let fitted_height = unsigned_divide(source_width.wrapping_mul(numerator), denominator).min(source_height);
        let fitted_width = unsigned_divide(fitted_height.wrapping_mul(denominator), numerator).min(source_width);
        ((source_width.wrapping_sub(fitted_width)) >> 1, (source_height.wrapping_sub(fitted_height)) >> 1, fitted_width, fitted_height)
    };
    let (left, top, fitted_width, fitted_height) = fit(normalized);
    let (original_left, original_top, _, _) = fit(ratio);
    let selected_x = horizontal.wrapping_add(4) as u8 as u32;
    let selected_y = 4u32.wrapping_sub(vertical) as u8 as u32;
    let center_x = left.wrapping_add(fitted_width.wrapping_mul(selected_x) >> 3);
    let center_y = top.wrapping_add(fitted_height.wrapping_mul(selected_y) >> 3);
    let half_width = (fitted_width >> 2) >> 1;
    let half_height = (fitted_height >> 2) >> 1;
    let center_x = (center_x.wrapping_sub(half_width) as i32).max(0) as u32 + half_width;
    let center_y = (center_y.wrapping_sub(half_height) as i32).max(0) as u32 + half_height;
    let extent = |center: u32, origin: u32, maximum: u32| {
        let difference = if center <= origin { 1 } else { (center.wrapping_sub(origin) as u16).max(1) as u32 };
        difference.min(maximum) as u16
    };
    (extent(center_x, original_left, width), extent(center_y, original_top, height))
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_processed_width(profile: u32, aspect: u32, mode: u32) -> u16 {
    unsafe { processed_dimensions(profile, aspect, mode, 0) }.0
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_processed_height(profile: u32, aspect: u32, mode: u32) -> u16 {
    unsafe { processed_dimensions(profile, aspect, 0, mode) }.1
}
