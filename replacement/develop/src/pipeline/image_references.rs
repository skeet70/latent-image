use super::callbacks;
use super::shared::{Address32, ByteOffset};
use core::ptr::{null, read_volatile, write_volatile};

#[derive(Clone, Copy)]
#[repr(transparent)]
pub(super) struct PlaneFlags(u32);
#[derive(Clone, Copy)]
#[repr(transparent)]
pub(super) struct PixelWidth(u32);
#[derive(Clone, Copy)]
#[repr(transparent)]
pub(super) struct RowCount(i32);
#[derive(Clone, Copy)]
#[repr(transparent)]
pub(super) struct RowStride(u32);
#[derive(Clone, Copy)]
#[repr(transparent)]
pub(super) struct ByteCount(u32);

#[derive(Clone, Copy)]
#[repr(C)]
pub(super) struct ReferencePlane {
    mode: u32,
    address: Address32,
    size: ByteCount,
    stride: RowStride,
    width: PixelWidth,
    height: RowCount,
    flags: PlaneFlags,
}

#[derive(Clone, Copy)]
#[repr(u32)]
enum ReferenceOperation { Filter = 0, Lookup = 3, SelectedBlend = 6, Blend = 7, NoiseMask = 8, NoiseFilter = 9, NoiseCombine = 10 }

#[repr(C)]
struct Operation {
    kind: ReferenceOperation,
    argument: *const u32,
    table: *const LookupEntry,
    coefficients: *const u32,
    mask: *const u32,
    filter: *const u32,
    combine: *const u32,
}

impl Operation {
    fn new(kind: ReferenceOperation) -> Self {
        Self { kind, argument: null(), table: null(), coefficients: null(), mask: null(), filter: null(), combine: null() }
    }
}

#[repr(C)]
struct LookupEntry { first: u32, second: u32, third: u32 }
#[repr(C)]
pub(super) struct LookupSource { reserved: u32, table: Address32 }

unsafe extern "C" {
    fn xm1_buffers_configuration_byte(key: ConfigurationKey) -> u32;
    fn xm1_buffers_configuration_half(key: ConfigurationKey) -> u16;
    fn xm1_pipeline_image_operate(job: Address32, operation: *const Operation, sources: *const *const ReferencePlane, count: u32, destination: *mut ReferencePlane) -> u32;
    fn xm1_pipeline_image_publish_debug_file(job: Address32, kind: u32, plane: *const ReferencePlane, first: u32, suffix: *const u8);
    fn __aeabi_uidiv(numerator: u32, denominator: u32) -> u32;
    fn __aeabi_ldivmod(numerator: i64, denominator: i64) -> i64;
    fn __aeabi_idiv(numerator: i32, denominator: i32) -> i32;
    fn xm1_pipeline_image_pad_edges(address: Address32, stride: RowStride, width: PixelWidth, height: RowCount, left: u32, right: u32, top: u32, bottom: u32);
    fn xm1_pipeline_image_affine_coefficients(job: Address32, matrix: Address32, output: *mut u32) -> i32;
    fn xm1_pipeline_image_select_affine_block(job: Address32, matrix: Address32, output: *mut u32) -> u8;
    fn xm1_pipeline_image_affine_copy_safe(job: Address32, matrix: Address32, source: *const ReferencePlane, destination: *const ReferencePlane) -> i32;
    fn xm1_pipeline_image_current_task() -> u32;
    fn xm1_pipeline_image_configure_task(task: u32, mode: u32) -> i32;
    fn xm1_pipeline_image_estimate_transform(previous: Address32, current: Address32, first: u32, second: u32, matrix: Address32, reference: u32, config: *const AffineConfiguration) -> i32;
    fn xm1_pipeline_image_cache_maintenance();
    fn xm1_pipeline_image_find_motion(previous: *const MotionRegion, reference: *const MotionRegion, offsets: *mut i32, vectors: *mut i32, variant: u32) -> u32;
    fn xm1_pipeline_image_refine_motion(current: *const MotionRegion, reference: *const MotionRegion, offsets: *mut i32, vectors: *mut i32) -> u32;
    fn xm1_pipeline_image_transform_mapped_word(index: u32, selection: u32) -> u16;
    fn xm1_pipeline_image_scaled_channel_curve(selection: u32) -> u32;
    fn xm1_pipeline_image_conversion_initialize() -> i32;
    fn xm1_pipeline_image_conversion_get_parameters(output: *mut Address32);
    fn xm1_pipeline_image_conversion_finish_primary(config: *const u32);
    fn xm1_pipeline_image_conversion_finish_secondary(config: *const u32);
    fn xm1_pipeline_image_conversion_prepare_mode(mode: u32) -> i32;
    fn xm1_pipeline_image_conversion_begin(mode: u32, packing: u32);
    fn xm1_pipeline_image_conversion_wait() -> i32;
    fn xm1_pipeline_image_conversion_release();
    fn xm1_pipeline_image_wait_all(mask: u32, completion: *mut u32, timeout: u32) -> i32;
    fn xm1_pipeline_image_trace_begin(operation: u32);
    fn xm1_pipeline_image_trace_end(operation: u32);
    fn xm1_pipeline_image_send_notification(task: u32, event: u32) -> i32;
}

#[derive(Clone, Copy)]
#[repr(transparent)]
pub(super) struct ConfigurationKey(pub(super) u32);


#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_blend_weights(weights: *mut u32, selected: u32, count: u32) {
    let primary = unsafe { __aeabi_uidiv(count.wrapping_add(255), count) };
    let secondary = unsafe { __aeabi_uidiv(256u32.wrapping_sub(primary), count.wrapping_sub(1)) };
    for index in 0..4 {
        let value = if count > index as u32 && count > 1 {
            if selected == index as u32 + 1 { primary } else { secondary }
        } else { 0 };
        unsafe { write_volatile(weights.add(index), value) };
    }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_blend_reference(job: Address32, planes: *const *const ReferencePlane, count: u32, destination: *const ReferencePlane) -> u32 {
    let first = unsafe { read_volatile(planes) };
    let sources = [first, unsafe { read_volatile(planes.add(1)) }, unsafe { read_volatile(planes.add(2)) }, null(), null()];
    let mut output = unsafe { read_volatile(destination) };
    output.width = unsafe { (*first).width };
    output.height = unsafe { (*first).height };
    output.size = ByteCount(output.stride.0.wrapping_mul(output.height.0 as u32));
    let mut weights = [0; 4];
    unsafe { xm1_pipeline_image_blend_weights(weights.as_mut_ptr(), 1, count) };
    let mut operation = Operation::new(ReferenceOperation::SelectedBlend);
    operation.coefficients = weights.as_ptr();
    unsafe {
        xm1_pipeline_image_operate(job, &operation, sources.as_ptr(), count, &mut output);
        xm1_pipeline_image_publish_debug_file(job, 0x1f, &output, 0, null());
    }
    1
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_blend_overlapping(job: Address32, planes: *const *const ReferencePlane, count: u32, destination: *mut ReferencePlane) -> u32 {
    let mut output = unsafe { read_volatile(destination) };
    let original = output;
    let first = unsafe { read_volatile(planes) };
    output.width = unsafe { (*first).width };
    output.height = unsafe { (*first).height };
    let mut sources = [null(); 5];
    for (index, slot) in sources.iter_mut().enumerate() {
        if (index as u32) < count {
            let plane = unsafe { read_volatile(planes.add(index)) };
            *slot = plane;
            let input = unsafe { read_volatile(plane) };
            let start = original.address.0;
            let input_start = input.address.0;
            let overlaps = if start >= input_start && start <= input_start.wrapping_add(input.stride.0.wrapping_mul(input.height.0 as u32)) {
                true
            } else {
                input_start <= start.wrapping_add(original.stride.0.wrapping_mul(original.height.0 as u32)) && !(start > input_start)
            };
            if overlaps { output.stride = input.stride; }
        }
    }
    output.size = ByteCount(output.stride.0.wrapping_mul(output.height.0 as u32));
    let mut weights = [0; 4];
    unsafe { xm1_pipeline_image_blend_weights(weights.as_mut_ptr(), 0, count) };
    let mut operation = Operation::new(ReferenceOperation::Blend);
    operation.coefficients = weights.as_ptr();
    unsafe {
        xm1_pipeline_image_operate(job, &operation, sources.as_ptr(), count, &mut output);
        write_volatile(destination, output);
    }
    1
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_lut_reference(job: Address32, planes: *const *const ReferencePlane, _unused: u32, mask: *const ReferencePlane, source: *const LookupSource, destination: *mut ReferencePlane) -> u32 {
    let first = unsafe { read_volatile(planes) };
    let sources = [first, unsafe { read_volatile(planes.add(1)) }, unsafe { read_volatile(planes.add(2)) }, unsafe { read_volatile(planes.add(3)) }, mask];
    let mut output = unsafe { read_volatile(destination) };
    output.width = unsafe { (*first).width };
    output.height = unsafe { (*first).height };
    output.size = ByteCount(output.stride.0.wrapping_mul(output.height.0 as u32));
    let table = 0x01570684 as *mut LookupEntry;
    for index in 0..256u32 {
        let data = unsafe { read_volatile(source) }.table;
        let second = unsafe { data.offset(ByteOffset(256 + index)).byte() } as u32;
        unsafe { write_volatile(core::ptr::addr_of_mut!((*table.add(index as usize)).second), second) };
        let data = unsafe { read_volatile(source) }.table;
        let first = unsafe { data.offset(ByteOffset(index)).byte() } as u32;
        unsafe { write_volatile(core::ptr::addr_of_mut!((*table.add(index as usize)).first), first) };
        let data = unsafe { read_volatile(source) }.table;
        let third = unsafe { data.offset(ByteOffset(512 + index * 2)).half() } as u32;
        unsafe { write_volatile(core::ptr::addr_of_mut!((*table.add(index as usize)).third), third) };
    }
    let mut operation = Operation::new(ReferenceOperation::Lookup);
    operation.table = table;
    unsafe {
        xm1_pipeline_image_operate(job, &operation, sources.as_ptr(), 5, &mut output);
        write_volatile(destination, output);
        (*destination).flags.0 |= (*first).flags.0;
        xm1_pipeline_image_publish_debug_file(job, 0x29, &output, 0, null());
    }
    1
}

unsafe fn calibration(key: ConfigurationKey) -> u32 {
    unsafe { xm1_buffers_configuration_byte(key) }
}

unsafe fn classification(key: ConfigurationKey, maximum: u32) -> u32 {
    for candidate in 0..maximum {
        if unsafe { calibration(key) } == candidate { return candidate; }
    }
    maximum
}

unsafe fn table_key(base: Address32, row: u32, level: u32) -> ConfigurationKey {
    ConfigurationKey(unsafe { base.offset(ByteOffset((row * 5 + level) * 4)).word() })
}

unsafe fn noise_level(job: Address32) -> u32 {
    for level in 0..4 {
        let sensitivity = unsafe { job.offset(ByteOffset(0x2244)).half() } as u32;
        let key = ConfigurationKey(unsafe { Address32(0xe5fb60 + level * 4).word() });
        if sensitivity <= unsafe { calibration(key) } * 10 { return level; }
    }
    4
}

fn expand_noise(plane: &mut ReferencePlane) {
    plane.address.0 = plane.address.0.wrapping_sub(16).wrapping_sub(plane.stride.0.wrapping_mul(8));
    plane.width.0 = plane.width.0.wrapping_add(32);
    plane.height.0 = plane.height.0.wrapping_add(16);
    plane.size.0 = plane.stride.0.wrapping_mul(plane.height.0 as u32);
}

fn invalid_request() -> ! {
    loop { core::hint::spin_loop(); }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_noise_reference(job: Address32, source: *const ReferencePlane, reference: *const ReferencePlane, mask: *const ReferencePlane, destination: *mut ReferencePlane) -> u32 {
    if source.is_null() || reference.is_null() || mask.is_null() || destination.is_null() { invalid_request(); }
    let level = unsafe { noise_level(job) };
    let mut reference_plane = unsafe { read_volatile(reference) };
    expand_noise(&mut reference_plane);
    let mut source_plane = unsafe { read_volatile(source) };
    expand_noise(&mut source_plane);
    let sources = [&reference_plane as *const _, &source_plane as *const _, mask, null(), null()];
    let mut output = unsafe { read_volatile(destination) };
    output.address.0 = output.address.0.wrapping_add(output.stride.0.wrapping_mul(9));
    output.width = unsafe { (*source).width };
    output.height = unsafe { (*source).height };
    output.size.0 = output.stride.0.wrapping_mul(output.height.0 as u32);
    let keys = Address32(0xe5fedc);
    let mut parameters = [0; 20];
    parameters[0] = unsafe { classification(table_key(keys, 2, level), 2) };
    parameters[1] = unsafe { classification(table_key(keys, 0, level), 2) };
    parameters[2] = unsafe { classification(table_key(keys, 1, level), 3) };
    for row in 3..6 {
        parameters[(row + 1) as usize] = unsafe { calibration(table_key(keys, row, level)) };
    }
    parameters[18] = unsafe { xm1_pipeline_image_configuration_signed_half(table_key(keys, 6, level)) } as u32;
    parameters[19] = unsafe { xm1_pipeline_image_configuration_signed_half(table_key(keys, 7, level)) } as u32;
    let mut operation = Operation::new(ReferenceOperation::NoiseCombine);
    operation.combine = parameters.as_ptr();
    unsafe {
        xm1_pipeline_image_operate(job, &operation, sources.as_ptr(), 3, &mut output);
        write_volatile(destination, output);
        xm1_pipeline_image_publish_debug_file(job, 0x21, destination, 0, null());
    }
    1
}

unsafe fn filter_taps(count: u32, selected: usize) -> [u32; 6] {
    let (enable, blend, high, low) = if count == 2 {
        if selected == 0 { (0x49c0, 0x49be, 0x49b5, 0x49b4) } else { (0x49c0, 0x49be, 0x49b7, 0x49b6) }
    } else {
        match selected {
            0 => (0x49c1, 0x49bf, 0x49b9, 0x49b8),
            1 => (0x49c1, 0x49bf, 0x49bb, 0x49ba),
            _ => (0x49c1, 0x49bf, 0x49bd, 0x49bc),
        }
    };
    unsafe {
        [
            u32::from(calibration(ConfigurationKey(enable)) != 0),
            u32::from(calibration(ConfigurationKey(blend)) != 0),
            calibration(ConfigurationKey(high)) & 7,
            (calibration(ConfigurationKey(high)) >> 4) & 7,
            calibration(ConfigurationKey(low)) & 7,
            (calibration(ConfigurationKey(low)) >> 4) & 7,
        ]
    }
}

unsafe fn filter_debug(job: Address32, parameters: &[u32; 6], count: u32) {
    if unsafe { calibration(ConfigurationKey(0xa8)) } & 0x80 == 0 { return; }
    let debug = Address32(unsafe { job.offset(ByteOffset(0x220c)).word() });
    unsafe {
        debug.offset(ByteOffset(0xd8b70)).write_word(0x53544d55);
        debug.offset(ByteOffset(0xdb980)).write_word(0x4c414d55);
        debug.offset(ByteOffset(0xdb960)).write_word(count);
        for (index, parameter) in [parameters[4], parameters[5], parameters[2], parameters[3], parameters[1], parameters[0]].iter().enumerate() {
            debug.offset(ByteOffset(0xdb964 + index as u32 * 4)).write_word(*parameter);
        }
        debug.offset(ByteOffset(0xdb97c)).write_word(job.offset(ByteOffset(0x5c0)).word().wrapping_add(1));
    }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_reference_filter(job: Address32, planes: *const *const ReferencePlane, count: u32, _unused: u32, destination: *mut ReferencePlane) -> u32 {
    let first = unsafe { read_volatile(planes) };
    let sources = [first, unsafe { read_volatile(planes.add(1)) }, unsafe { read_volatile(planes.add(2)) }, null(), null()];
    let mut output = unsafe { read_volatile(destination) };
    let width = unsafe { (*first).width.0 } >> 1;
    output.width.0 = width.wrapping_add(7) & !7;
    output.height = unsafe { (*first).height };
    output.size.0 = output.stride.0.wrapping_mul(output.height.0 as u32);
    let margin = output.width.0.wrapping_sub(width) >> 1;
    output.address.0 = output.address.0.wrapping_sub(16).wrapping_sub(output.stride.0.wrapping_mul(9));
    let mut values = [0i32; 5];
    for index in 0..count as usize {
        let slot = unsafe { job.offset(ByteOffset(0x3080 + index as u32 * 4)).word() };
        values[index] = unsafe { job.offset(ByteOffset(0x6c8 + slot.wrapping_mul(4))).word() } as i32;
    }
    for index in 0..count as usize {
        for inner in index..count as usize {
            if values[index] < values[inner] { values.swap(index, inner); }
        }
    }
    let target = unsafe { job.offset(ByteOffset(0x6cc)).word() } as i32;
    let selected = values[..count as usize].iter().position(|value| *value == target).unwrap_or(count as usize);
    let parameters = unsafe { filter_taps(count, selected) };
    let mut operation = Operation::new(ReferenceOperation::Filter);
    operation.argument = parameters.as_ptr();
    unsafe {
        filter_debug(job, &parameters, count);
        xm1_pipeline_image_operate(job, &operation, sources.as_ptr(), count, &mut output);
    }
    output.width.0 = output.width.0.wrapping_sub(margin.wrapping_mul(2));
    output.address.0 = output.address.0.wrapping_add(output.stride.0.wrapping_mul(9)).wrapping_add(16).wrapping_add(margin);
    unsafe {
        write_volatile(destination, output);
        xm1_pipeline_image_publish_debug_file(job, 0x26, destination, 0, null());
    }
    1
}

#[derive(Clone, Copy)]
#[repr(u32)]
enum NoisePass { First = 1, Second = 2, Third = 3 }

impl NoisePass {
    fn parse(value: u32) -> Self {
        match value { 1 => Self::First, 2 => Self::Second, 3 => Self::Third, _ => invalid_request() }
    }
}

unsafe fn parameter_word(base: Address32, index: u32, value: u32) {
    unsafe { base.offset(ByteOffset(index * 4)).write_word(value) };
}

unsafe fn noise_debug(job: Address32, level: u32, pass: NoisePass) {
    if unsafe { calibration(ConfigurationKey(0xa8)) } & 0x80 == 0 { return; }
    let debug = Address32(unsafe { job.offset(ByteOffset(0x220c)).word() });
    unsafe {
        debug.offset(ByteOffset(0xd8b70)).write_word(0x53544d55);
        debug.offset(ByteOffset(0xdb980)).write_word(0x4c414d55);
        debug.offset(ByteOffset(0xdacf0)).write_word(job.offset(ByteOffset(0x2244)).half() as u32 / 10);
        debug.offset(ByteOffset(0xdacf4)).write_word(level + 1);
        debug.offset(ByteOffset(0xdacf8)).write_word(pass as u32 + 1);
    }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_noise_pass(job: Address32, source: *const ReferencePlane, reference: *const ReferencePlane, mask: *const ReferencePlane, filtered: *const ReferencePlane, destination: *mut ReferencePlane, pass: u32) -> u32 {
    if source.is_null() || reference.is_null() || mask.is_null() || filtered.is_null() || destination.is_null() { invalid_request(); }
    let pass = NoisePass::parse(pass);
    let source_plane = unsafe { read_volatile(source) };
    let reference_plane = unsafe { read_volatile(reference) };
    let horizontal = match pass { NoisePass::First => 6, _ => 8 };
    unsafe {
        xm1_pipeline_image_pad_edges(source_plane.address, source_plane.stride, source_plane.width, source_plane.height, horizontal, horizontal, 8, 8);
        xm1_pipeline_image_pad_edges(reference_plane.address, reference_plane.stride, reference_plane.width, reference_plane.height, 8, 8, 8, 8);
    }
    let mut source_plane = source_plane;
    expand_noise(&mut source_plane);
    let mut reference_plane = reference_plane;
    expand_noise(&mut reference_plane);
    let level = unsafe { noise_level(job) };
    let keys = Address32(0xe5fc48);
    let mask_parameters = Address32(0x01571690);
    let filter_parameters = Address32(0x015712f4);
    let combine_parameters = Address32(0x015712a4);
    unsafe {
        parameter_word(mask_parameters, 0, classification(ConfigurationKey(0x484e), 4));
        parameter_word(mask_parameters, 1, classification(table_key(keys, 0, level), 3));
        parameter_word(mask_parameters, 2, classification(table_key(keys, 1, level), 2));
        parameter_word(mask_parameters, 3, classification(table_key(keys, 2, level), 2));
        for (index, key) in [(4, 0x484f), (5, 0x4850), (6, 0x4850)] {
            parameter_word(mask_parameters, index, calibration(ConfigurationKey(key)));
        }
    }
    let sources = [&source_plane as *const _, &reference_plane as *const _, null(), null(), null()];
    let mut output = unsafe { read_volatile(mask) };
    let mut operation = Operation::new(ReferenceOperation::NoiseMask);
    operation.mask = mask_parameters.0 as *const u32;
    unsafe {
        xm1_pipeline_image_operate(job, &operation, sources.as_ptr(), 2, &mut output);
        xm1_pipeline_image_publish_debug_file(job, 0x1c, &output, 0, null());
        for index in 0..4 { parameter_word(filter_parameters, index, mask_parameters.offset(ByteOffset(index * 4)).word()); }
        parameter_word(filter_parameters, 4, classification(table_key(keys, 3, level), 2));
        for index in 4..7 { parameter_word(filter_parameters, index + 1, mask_parameters.offset(ByteOffset(index * 4)).word()); }
        for row in 4..20 { parameter_word(filter_parameters, row + 4, calibration(table_key(keys, row, level))); }
        parameter_word(filter_parameters, 24, xm1_buffers_configuration_half(table_key(keys, 20, level)) as u32);
        for index in 0..4 { parameter_word(filter_parameters, 25 + index, calibration(ConfigurationKey(0x4851 + index))); }
        for index in 0..8 { parameter_word(filter_parameters, 29 + index, xm1_pipeline_image_configuration_signed_byte(ConfigurationKey(0x4855 + index)) as u32); }
    }
    let sources = [&source_plane as *const _, &reference_plane as *const _, mask, null(), null()];
    output = unsafe { read_volatile(filtered) };
    operation = Operation::new(ReferenceOperation::NoiseFilter);
    operation.filter = filter_parameters.0 as *const u32;
    unsafe {
        xm1_pipeline_image_operate(job, &operation, sources.as_ptr(), 3, &mut output);
        xm1_pipeline_image_publish_debug_file(job, 0x1d, &output, 0, null());
        parameter_word(combine_parameters, 0, classification(table_key(keys, 23, level), 2));
        parameter_word(combine_parameters, 1, classification(table_key(keys, 21, level), 2));
        parameter_word(combine_parameters, 2, classification(table_key(keys, 22, level), 3));
        parameter_word(combine_parameters, 3, u32::from(calibration(ConfigurationKey(0x485d)) != 0));
        for index in 0..3 { parameter_word(combine_parameters, index + 4, calibration(table_key(keys, index + 24, level))); }
        parameter_word(combine_parameters, 15, calibration(ConfigurationKey(0x485e)));
        parameter_word(combine_parameters, 16, calibration(ConfigurationKey(0x485f)));
        parameter_word(combine_parameters, 17, xm1_pipeline_image_configuration_signed_byte(ConfigurationKey(0x4860)) as u32);
        let row = 27 + (pass as u32 - 1) * 2;
        parameter_word(combine_parameters, 18, xm1_pipeline_image_configuration_signed_half(table_key(keys, row, level)) as u32);
        parameter_word(combine_parameters, 19, xm1_pipeline_image_configuration_signed_half(table_key(keys, row + 1, level)) as u32);
    }
    let mut matrix = [0; 8];
    unsafe {
        xm1_pipeline_image_affine_corner_matrix(job, source, job.offset(ByteOffset(0x2fcc)), matrix.as_mut_ptr());
        for (index, entry) in [0, 4, 1, 5, 3, 7, 2, 6].iter().enumerate() {
            parameter_word(combine_parameters, 7 + index as u32, matrix[*entry]);
        }
    }
    output = unsafe { read_volatile(destination) };
    output.address.0 = output.address.0.wrapping_add(output.stride.0.wrapping_mul(9));
    output.width = unsafe { (*source).width };
    output.height = unsafe { (*source).height };
    output.size.0 = output.stride.0.wrapping_mul(output.height.0 as u32);
    let sources = [&source_plane as *const _, &reference_plane as *const _, filtered, null(), null()];
    operation = Operation::new(ReferenceOperation::NoiseCombine);
    operation.combine = combine_parameters.0 as *const u32;
    unsafe {
        xm1_pipeline_image_operate(job, &operation, sources.as_ptr(), 3, &mut output);
        write_volatile(destination, output);
        xm1_pipeline_image_publish_debug_file(job, 0x1e, destination, 0, null());
        noise_debug(job, level, pass);
    }
    1
}

unsafe fn reference_index(job: Address32) -> u32 {
    let current = unsafe { job.offset(ByteOffset(0x2fe4)).word() };
    let reference = unsafe { job.offset(ByteOffset(0x2fe8)).word() };
    if current < reference { current.wrapping_sub(1) } else if current > reference { current.wrapping_sub(2) } else { 0 }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_validate_affine(job: Address32, source: *const ReferencePlane, destination: *const ReferencePlane, _transform: u32) -> u32 {
    let matrix = job.offset(ByteOffset(0x2fcc));
    let mut bits = u32::from(unsafe { xm1_pipeline_image_affine_coefficients(job, matrix, core::ptr::null_mut()) } == 1);
    if unsafe { xm1_pipeline_image_select_affine_block(job, matrix, core::ptr::null_mut()) } == 1 { bits += 2; }
    let source = unsafe { read_volatile(source) };
    let mut output = unsafe { read_volatile(destination) };
    output.address.0 = output.address.0.wrapping_add(output.stride.0.wrapping_mul(9));
    output.width = source.width;
    output.height = source.height;
    output.size.0 = (source.height.0 as u32).wrapping_mul(source.stride.0);
    if unsafe { xm1_pipeline_image_affine_copy_safe(job, matrix, &source, &output) } == 1 { bits += 4; }
    unsafe {
        let index = reference_index(job);
        job.offset(ByteOffset(0x2fc0 + index.wrapping_mul(4))).write_word(bits);
        let index = reference_index(job);
        let debug = Address32(job.offset(ByteOffset(0x220c)).word());
        debug.offset(ByteOffset(0xdace4 + index.wrapping_mul(4))).write_word(bits);
        let valid = u32::from(job.offset(ByteOffset(0x2fbc)).word() == 1 && bits == 7);
        job.offset(ByteOffset(0x2fbc)).write_word(valid);
        valid
    }
}

#[repr(C)]
pub(super) struct AffineConfiguration {
    mode: u8,
    level: u8,
    limit: u16,
    weights: [u8; 4],
    enabled: u32,
    threshold: u8,
    reserved_d: u8,
    limit_0: u16,
    threshold_1: u8,
    reserved_11: u8,
    limit_1: u16,
    threshold_2: u8,
    reserved_15: [u8; 3],
    enabled_1: u32,
    ranges: [u8; 3],
    enabled_2: u8,
    steps: [u8; 4],
    enabled_3: u32,
    counts: [u8; 5],
    reserved_2d: [u8; 3],
}

unsafe fn configuration_le(key: ConfigurationKey, bytes: u32) -> u32 {
    let mut result = 0;
    for index in (0..bytes).rev() {
        result |= unsafe { calibration(ConfigurationKey(key.0 + index)) } << (index * 8);
    }
    result
}

unsafe fn affine_configuration(base: u32) -> AffineConfiguration {
    let mut config = AffineConfiguration {
        mode: unsafe { calibration(ConfigurationKey(base + 0x4700)) } as u8,
        level: unsafe { calibration(ConfigurationKey(base + 0x4701)) } as u8,
        limit: unsafe { configuration_le(ConfigurationKey(base + 0x4741), 2) } as u16,
        weights: [0; 4], enabled: 0, threshold: 0, reserved_d: 0, limit_0: 0,
        threshold_1: 0, reserved_11: 0, limit_1: 0, threshold_2: 0, reserved_15: [0; 3],
        enabled_1: 0, ranges: [0; 3], enabled_2: 0, steps: [0; 4], enabled_3: 0,
        counts: [0; 5], reserved_2d: [0; 3],
    };
    unsafe {
        for index in 0..4 { config.weights[index] = calibration(ConfigurationKey(base + 0x4743 + index as u32)) as u8; }
        config.enabled = calibration(ConfigurationKey(base + 0x4740)) & 1;
        config.threshold = calibration(ConfigurationKey(base + 0x4747)) as u8;
        config.limit_0 = configuration_le(ConfigurationKey(base + 0x4748), 2) as u16;
        config.threshold_1 = calibration(ConfigurationKey(base + 0x474a)) as u8;
        config.limit_1 = configuration_le(ConfigurationKey(base + 0x474b), 2) as u16;
        config.threshold_2 = calibration(ConfigurationKey(base + 0x474d)) as u8;
        config.enabled_1 = (calibration(ConfigurationKey(base + 0x4740)) >> 1) & 1;
        for index in 0..3 { config.ranges[index] = calibration(ConfigurationKey(base + 0x474e + index as u32)) as u8; }
        config.enabled_2 = ((calibration(ConfigurationKey(base + 0x4740)) >> 2) & 1) as u8;
        for index in 0..4 { config.steps[index] = calibration(ConfigurationKey(base + 0x4751 + index as u32)) as u8; }
        config.enabled_3 = (calibration(ConfigurationKey(base + 0x4740)) >> 3) & 1;
        for index in 0..5 { config.counts[index] = calibration(ConfigurationKey(base + 0x4755 + index as u32)) as u8; }
    }
    config
}

unsafe fn affine_debug(job: Address32, config: &AffineConfiguration, corners: &[u32; 2], direct: u32, coefficients: &[u32; 6], estimated: i32, valid: i32) {
    if unsafe { calibration(ConfigurationKey(0xa8)) } & 0x80 == 0 { return; }
    let debug = Address32(unsafe { job.offset(ByteOffset(0x220c)).word() });
    let enabled = config.enabled_3 * 8 + u32::from(config.enabled_2 != 0) * 4 + config.enabled_1 * 2 + config.enabled;
    let fields = [
        (0xdac00, enabled), (0xdac04, config.limit as u32),
        (0xdac08, config.weights[0] as u32), (0xdac0c, config.weights[1] as u32),
        (0xdac10, config.weights[2] as u32), (0xdac14, config.weights[3] as u32),
        (0xdac18, config.threshold as u32), (0xdac1c, config.limit_0 as u32),
        (0xdac20, config.threshold_1 as u32), (0xdac24, config.limit_1 as u32),
        (0xdac28, config.threshold_2 as u32), (0xdac2c, config.ranges[0] as u32),
        (0xdac30, config.ranges[1] as u32), (0xdac34, config.ranges[2] as u32),
        (0xdac38, config.steps[0] as u32), (0xdac3c, config.steps[1] as u32),
        (0xdac40, config.steps[2] as u32), (0xdac5c, config.steps[3] as u32),
        (0xdac48, config.counts[0] as u32), (0xdac4c, config.counts[1] as u32),
        (0xdac50, config.counts[2] as u32), (0xdac54, config.counts[3] as u32),
        (0xdac58, config.counts[4] as u32),
    ];
    unsafe {
        debug.offset(ByteOffset(0xd8b70)).write_word(0x53544d55);
        debug.offset(ByteOffset(0xdb980)).write_word(0x4c414d55);
        for (offset, value) in fields { debug.offset(ByteOffset(offset)).write_word(value); }
        debug.offset(ByteOffset(0xdacb4)).write_word(corners[0]);
        debug.offset(ByteOffset(0xdacb8)).write_word(corners[1]);
        debug.offset(ByteOffset(0xdacbc)).write_word(direct);
        for (index, value) in coefficients.iter().enumerate() { debug.offset(ByteOffset(0xdacc0 + index as u32 * 4)).write_word(*value); }
        let index = reference_index(job);
        for coefficient in 0..6 {
            let value = job.offset(ByteOffset(0x2fcc + coefficient * 4)).word();
            debug.offset(ByteOffset(0xdac60 + index.wrapping_mul(24) + coefficient * 4)).write_word(value);
        }
        debug.offset(ByteOffset(0xdaca8 + index.wrapping_mul(4))).write_word(u32::from(estimated == 1));
        debug.offset(ByteOffset(0xdacd8 + index.wrapping_mul(4))).write_word(u32::from(valid == 1));
    }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_estimate_affine(job: Address32, image: *const ReferencePlane, previous: Address32, current: Address32, reference: u32, _unused: u32) -> u32 {
    let kind = unsafe { job.offset(ByteOffset(0x4b4)).word() };
    let task = unsafe { xm1_pipeline_image_current_task() };
    unsafe { xm1_pipeline_image_configure_task(task, 12) };
    if unsafe { (*image).flags.0 } & 0x100000 == 0 { invalid_request(); }
    let width = unsafe { (*image).width.0 };
    let height = unsafe { (*image).height.0 };
    let bank = if kind == 0x1a { 0x210 } else { 0 };
    let mut coefficients = [0u32; 6];
    unsafe {
        for index in 0..4 {
            let value = configuration_le(ConfigurationKey(bank + 0x4769 + index as u32 * 2), 2);
            coefficients[index] = if value > 0x1fff { value.wrapping_sub(0x4000) } else { value };
            coefficients[index] = coefficients[index].wrapping_mul(16);
        }
        for index in 4..6 {
            let value = configuration_le(ConfigurationKey(bank + 0x4771 + (index as u32 - 4) * 4), 4);
            coefficients[index] = if value >= 0x2000000 { value.wrapping_sub(0x4000000) } else { value };
            coefficients[index] = coefficients[index].wrapping_mul(16);
        }
    }
    let config = unsafe { affine_configuration(bank) };
    let direct = u32::from(unsafe { calibration(ConfigurationKey(bank + 0x4768)) } != 0);
    let matrix = job.offset(ByteOffset(0x2fcc));
    let estimated = if direct != 1 {
        unsafe { xm1_pipeline_image_estimate_transform(previous, current, 0, 0, matrix, reference, &config) }
    } else {
        for (index, value) in coefficients.iter().enumerate() { unsafe { matrix.offset(ByteOffset(index as u32 * 4)).write_word(*value) }; }
        1
    };
    let corners = unsafe { [configuration_le(ConfigurationKey(bank + 0x4760), 2), configuration_le(ConfigurationKey(bank + 0x4762), 2)] };
    let valid = if estimated != 1 { 0 } else { unsafe { xm1_pipeline_image_affine_corners_valid(width >> 1, height, matrix, reference, corners.as_ptr()) } };
    unsafe {
        affine_debug(job, &config, &corners, direct, &coefficients, estimated, valid);
        xm1_pipeline_image_configure_task(task, 6);
        job.offset(ByteOffset(0x2fbc)).write_word(u32::from(estimated == 1 && valid == 1));
    }
    1
}

#[derive(Clone, Copy)]
#[repr(C)]
pub(super) struct MotionRegion {
    address: Address32,
    stride: RowStride,
    width: PixelWidth,
    height: RowCount,
}

impl MotionRegion {
    unsafe fn load(plane: *const ReferencePlane) -> Self {
        Self {
            address: Address32(unsafe { (*plane).address.0 } & !0x40000000),
            stride: unsafe { (*plane).stride },
            width: unsafe { (*plane).width },
            height: unsafe { (*plane).height },
        }
    }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_initialize_motion(width: i32, height: i32) {
    let coarse = Address32(0x014e2a34);
    let full = Address32(0x014e29dc);
    for (state, width, height) in [(coarse, width / 4, height / 4), (full, width, height)] {
        let inner_width = width.wrapping_mul(3) / 10;
        let inner_height = height.wrapping_mul(6) / 10;
        let values = [width, height, width.wrapping_sub(inner_width) / 2, height.wrapping_sub(inner_height) / 2, inner_width, inner_height, width.wrapping_sub(inner_width).wrapping_add(1), height.wrapping_sub(inner_height).wrapping_add(1)];
        for (index, value) in values.iter().enumerate() { unsafe { state.offset(ByteOffset(index as u32 * 4)).write_word(*value as u32) }; }
        let key = ConfigurationKey(if state == coarse { 0x9f05 } else { 0x9f06 });
        let limit = unsafe { calibration(key) };
        unsafe { state.offset(ByteOffset(0x3c)).write_word(limit) };
    }
    let initial_offset = unsafe { calibration(ConfigurationKey(0x9f04)) }.wrapping_mul(4);
    unsafe {
        Address32(0x014e2a28).write_word(initial_offset);
        Address32(0x014e2a2c).write_word(0);
        for offset in (0..0x890).step_by(4) { Address32(0x014e2148 + offset).write_word(0); }
        Address32(0x014e2148).write_word(4);
        Address32(0x014e29d8).write_word(0);
    }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_set_motion_offsets(horizontal: i32, vertical: i32) {
    unsafe {
        Address32(0x014e2a28).write_word(horizontal as u32);
        Address32(0x014e2a2c).write_word(vertical as u32);
    }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_motion_owned(_job: Address32, current: *const ReferencePlane, previous: *const ReferencePlane, current_reference: *const ReferencePlane, previous_reference: *const ReferencePlane, apply_size: u32, count: *const i32, offset: *const i32, mode: u32, offset_x: *mut i32, offset_y: *mut i32, vector_0: *mut i32, vector_1: *mut i32, vector_2: *mut i32, vector_3: *mut i32, inherited_vector_2: u32) -> u32 {
    let task = unsafe { xm1_pipeline_image_current_task() };
    unsafe { xm1_pipeline_image_configure_task(task, 13) };
    let current = unsafe { MotionRegion::load(current) };
    let previous = unsafe { MotionRegion::load(previous) };
    let current_reference = unsafe { MotionRegion::load(current_reference) };
    let previous_reference = unsafe { MotionRegion::load(previous_reference) };
    let limit = unsafe { calibration(ConfigurationKey(0x9f7b)) };
    if apply_size == 1 { unsafe { xm1_pipeline_image_initialize_motion((current.width.0 >> 1) as i32, current.height.0) }; }
    unsafe { xm1_pipeline_image_cache_maintenance() };
    if mode == 3 || mode <= 1 {
        if mode == 3 || unsafe { read_volatile(count) } < (limit * 4) as i32 {
            if !count.is_null() && !offset.is_null() {
                unsafe { xm1_pipeline_image_set_motion_offsets(read_volatile(count), read_volatile(offset)) };
            }
        }
    }
    let mut offsets = core::mem::MaybeUninit::<[i32; 2]>::uninit();
    let mut vectors = core::mem::MaybeUninit::<[i32; 4]>::uninit();
    let offsets_pointer = offsets.as_mut_ptr().cast::<i32>();
    let vectors_pointer = vectors.as_mut_ptr().cast::<i32>();
    unsafe { write_volatile(vectors_pointer.add(2), inherited_vector_2 as i32) };
    if mode != 1 {
        unsafe { xm1_pipeline_image_find_motion(&previous, &previous_reference, offsets_pointer, vectors_pointer, u32::from(mode == 3)) };
    }
    let result = unsafe { xm1_pipeline_image_refine_motion(&current, &current_reference, offsets_pointer, vectors_pointer) };
    unsafe {
        xm1_pipeline_image_cache_maintenance();
        let offsets = offsets.assume_init();
        let vectors = vectors.assume_init();
        write_volatile(offset_x, offsets[0]);
        write_volatile(offset_y, offsets[1]);
        for (pointer, value) in [vector_0, vector_1, vector_2, vector_3].into_iter().zip(vectors) { write_volatile(pointer, value); }
        xm1_pipeline_image_configure_task(task, 6);
    }
    result
}

#[repr(C)]
pub(super) struct CurveRequest {
    count: u32,
    selected: u32,
    limits: [u32; 3],
    mapped: i32,
    channel: u32,
    scaled: i32,
    values: [i32; 5],
    mode: u32,
    threshold: u32,
    ranges: [u32; 4],
}

unsafe fn curve_debug(job: Address32, request: &CurveRequest, table: Address32) {
    if unsafe { calibration(ConfigurationKey(0xa8)) } & 0x80 == 0 { return; }
    let debug = Address32(unsafe { job.offset(ByteOffset(0x220c)).word() });
    unsafe {
        debug.offset(ByteOffset(0xd8b70)).write_word(0x53544d55);
        debug.offset(ByteOffset(0xdb980)).write_word(0x4c414d55);
        for (offset, value) in [
            (0xdacfc, request.count), (0xdad00, request.limits[0]), (0xdad04, request.limits[1]),
            (0xdad08, request.limits[2]), (0xdad0c, request.scaled as u32),
        ] { debug.offset(ByteOffset(offset)).write_word(value); }
        for index in 0..4 {
            let value = calibration(ConfigurationKey(0x4996 + index));
            debug.offset(ByteOffset(0xdad10 + index * 4)).write_word(value);
        }
        for index in 0..6 {
            let value = configuration_le(ConfigurationKey(0x499a + index * 2), 2);
            debug.offset(ByteOffset(0xdad20 + index * 4)).write_word(value);
        }
        debug.offset(ByteOffset(0xdad38)).write_word(request.mapped as u32);
        debug.offset(ByteOffset(0xdad3c)).write_word(request.channel);
        for index in 0..5 { debug.offset(ByteOffset(0xdad40 + index * 4)).write_word(request.values[index as usize] as u32); }
        for index in 0..256 {
            debug.offset(ByteOffset(0xdad54 + index * 4)).write_word(table.offset(ByteOffset(index)).byte() as u32);
            debug.offset(ByteOffset(0xdb154 + index * 4)).write_word(table.offset(ByteOffset(256 + index)).byte() as u32);
            debug.offset(ByteOffset(0xdb554 + index * 4)).write_word(table.offset(ByteOffset(512 + index * 2)).half() as u32);
        }
        for index in 0..3 {
            let value = configuration_le(ConfigurationKey(0x49a6 + index * 2), 2);
            debug.offset(ByteOffset(0xdb954 + index * 4)).write_word(value);
        }
    }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_reference_curve(job: Address32, count: u32, reference: u32, destination: *const ReferencePlane) -> u32 {
    if unsafe { (*destination).size.0 } < 1024 { return 0; }
    let mut request = CurveRequest { count, selected: reference.wrapping_sub(1), limits: [0; 3], mapped: 0, channel: 0, scaled: 0, values: [0; 5], mode: 0, threshold: 0, ranges: [0; 4] };
    unsafe {
        for index in 0..3 { request.limits[index] = configuration_le(ConfigurationKey(0x4990 + index as u32 * 2), 2); }
        let index = xm1_pipeline_image_conversion_index(job.offset(ByteOffset(0x65c)).byte() as u32);
        let selection = xm1_pipeline_image_conversion_selection(job.offset(ByteOffset(0x6c0)).byte() as u32);
        request.mapped = (xm1_pipeline_image_transform_mapped_word(index, selection) as i32).wrapping_mul(32) / 100;
        let selection = xm1_pipeline_image_conversion_selection(job.offset(ByteOffset(0x6c0)).byte() as u32);
        request.channel = xm1_pipeline_image_channel_packed_value(selection).wrapping_mul(8) / 100;
        let selection = xm1_pipeline_image_conversion_selection(job.offset(ByteOffset(0x6c0)).byte() as u32);
        request.scaled = (xm1_pipeline_image_scaled_channel_curve(selection) as i32).wrapping_mul(16) / 100;
    }
    if count as i32 > 0 {
        let mut minimum = 0;
        for index in 0..count as usize {
            let slot = unsafe { job.offset(ByteOffset(0x3080 + index as u32 * 4)).word() };
            let value = unsafe { job.offset(ByteOffset(0x6c8 + slot.wrapping_mul(4))).word() } as i32;
            request.values[index] = value;
            if value <= minimum { minimum = value; }
        }
        let adjustment = minimum.wrapping_abs();
        for value in &mut request.values[..count as usize] { *value = value.wrapping_add(adjustment).min(2047); }
        let target = unsafe { job.offset(ByteOffset(0x6cc + request.selected.wrapping_mul(4))).word() } as i32;
        for index in 0..count as usize {
            for inner in index..count as usize {
                if request.values[index] < request.values[inner] { request.values.swap(index, inner); }
            }
        }
        if let Some(selected) = request.values[..count as usize].iter().position(|value| *value == target) { request.selected = selected as u32; }
    }
    unsafe {
        request.threshold = calibration(ConfigurationKey(0x4999));
        let mode = job.offset(ByteOffset(0x5c0)).word();
        let column = match mode { 2 => 2, 1 => 1, _ => 0 };
        request.mode = calibration(ConfigurationKey(0x4996 + column));
        let adjustment = (count.wrapping_mul(6).wrapping_sub(12) as i32).max(0) as u32;
        request.ranges[0] = configuration_le(ConfigurationKey(0x499a + column * 2 + adjustment), 2);
        for index in 0..3 { request.ranges[index + 1] = configuration_le(ConfigurationKey(0x49a6 + index as u32 * 2), 2); }
        xm1_pipeline_image_build_curve(&mut request, (*destination).address);
        curve_debug(job, &request, (*destination).address);
    }
    1
}

unsafe fn primary_capture(job: Address32) -> bool {
    let kind = unsafe { job.offset(ByteOffset(0x4b4)).word() };
    kind == 0x19 || (kind == 1 && unsafe { job.offset(ByteOffset(0x710)).word() } & 0x400 != 0)
}

unsafe fn clear_words(address: Address32, bytes: u32) {
    for offset in (0..bytes).step_by(4) { unsafe { address.offset(ByteOffset(offset)).write_word(0) }; }
}

unsafe fn conversion_completion(command: Address32, mode: u32) -> u32 {
    unsafe {
        Address32(0x015716d4).write_word(xm1_pipeline_image_current_task());
        xm1_pipeline_image_conversion_begin(mode, command.offset(ByteOffset(4)).word());
    }
    let mut completion = 0;
    unsafe {
        xm1_pipeline_image_wait_all(0x10, &mut completion, u32::MAX);
        Address32(0x015716d4).write_word(0);
    }
    u32::from(completion & 0x20 == 0)
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_motion_primary(job: Address32, planes: *const *const ReferencePlane, output: Address32, request: u32) -> u32 {
    if request == 0 {
        let kind = if unsafe { primary_capture(job) } { 0x19 } else { 0x22 };
        unsafe { xm1_pipeline_image_publish_debug_file(job, kind, read_volatile(planes), 0, null()) };
    }
    let config = Address32(0x01571534);
    unsafe { clear_words(config, 0x15c) };
    let reduction = unsafe { xm1_pipeline_image_select_reduction(job, request) };
    let image = unsafe { read_volatile(planes.add(reduction as usize)) };
    if unsafe { xm1_pipeline_image_primary_motion_allowed(job, image, output) } == 0 { return 0; }
    unsafe { xm1_pipeline_image_conversion_initialize() };
    let mut commands = [Address32(0); 2];
    unsafe { xm1_pipeline_image_conversion_get_parameters(commands.as_mut_ptr()) };
    let command = commands[0];
    unsafe {
        command.write_word(1);
        xm1_pipeline_image_describe_motion_rows(image, command.offset(ByteOffset(4)), reduction);
        xm1_pipeline_image_primary_motion_options(job, request, command.offset(ByteOffset(4)).word(), config);
        command.offset(ByteOffset(0x30)).write_word(config.offset(ByteOffset(0x58)).word());
    }
    let shifted = output.offset(ByteOffset(unsafe { config.offset(ByteOffset(0x58)).word() }.wrapping_mul(4)));
    unsafe {
        xm1_pipeline_image_primary_motion_output(shifted, command.offset(ByteOffset(4)), config);
        xm1_pipeline_image_conversion_finish_primary(config.0 as *const u32);
        let _ = callbacks::registry().ensure(xm1_pipeline_image_motion_conversion_complete as unsafe extern "C" fn());
        command.offset(ByteOffset(0x38)).write_word(xm1_pipeline_image_motion_conversion_complete as *const () as u32);
        xm1_pipeline_image_conversion_prepare_mode(1);
        xm1_pipeline_image_trace_begin(0x06023350);
        motion_register_debug(job, 1, request);
    }
    let result = unsafe { conversion_completion(command, 1) };
    unsafe {
        xm1_pipeline_image_copy_motion_words(shifted, output, config.offset(ByteOffset(0x58)).word(), reduction, request);
        motion_result_debug(job, 1, output, Address32(0), request);
        xm1_pipeline_image_conversion_wait();
        xm1_pipeline_image_trace_end(0x06023350);
        xm1_pipeline_image_conversion_release();
    }
    result
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_motion_secondary(job: Address32, inputs: *const *const ReferencePlane, mut input_count: u32, input: Address32, outputs: *const *const ReferencePlane, mut output_count: u32, output: Address32, request: u32) -> u32 {
    let config = Address32(0x01571388);
    if request == 0 {
        let mut plane = unsafe { read_volatile(read_volatile(inputs.add(1))) };
        let first = unsafe { read_volatile(inputs) };
        plane.height.0 = unsafe { (*first).height.0 }.wrapping_add(1).cast_unsigned().wrapping_shr(1) as i32;
        plane.size.0 = plane.stride.0.wrapping_mul(plane.height.0 as u32);
        let kind = if unsafe { primary_capture(job) } { 0x19 } else { 0x22 };
        unsafe {
            xm1_pipeline_image_publish_debug_file(job, kind, read_volatile(outputs), 0, null());
            xm1_pipeline_image_publish_debug_file(job, kind + 1, &plane, 0, null());
            clear_words(config, 0x1ac);
        }
        let key = ConfigurationKey(if unsafe { primary_capture(job) } { 0x471c } else { 0x492c });
        input_count = unsafe { calibration(key) }.wrapping_add(1);
        output_count = input_count;
    } else {
        unsafe { clear_words(config, 0x1ac) };
    }
    let reduction = unsafe { xm1_pipeline_image_select_reduction(job, request) };
    let inputs_selected = unsafe { inputs.add(reduction as usize) };
    let outputs_selected = unsafe { outputs.add(reduction as usize) };
    if unsafe { xm1_pipeline_image_secondary_motion_allowed(job, request, inputs_selected, input_count, input, outputs_selected, output_count, output) } == 0 { return 0; }
    unsafe { xm1_pipeline_image_conversion_initialize() };
    let mut commands = [Address32(0); 2];
    unsafe { xm1_pipeline_image_conversion_get_parameters(commands.as_mut_ptr()) };
    let command = commands[0];
    unsafe {
        command.write_word(2);
        xm1_pipeline_image_describe_motion_rows(read_volatile(inputs_selected), command.offset(ByteOffset(4)), reduction);
        xm1_pipeline_image_describe_motion_rows(read_volatile(outputs_selected), command.offset(ByteOffset(24)), reduction);
        let kind = job.offset(ByteOffset(0x4b4)).word();
        xm1_pipeline_image_secondary_motion_options(if kind == 1 { 0x19 } else { kind }, request, command.offset(ByteOffset(4)).word(), config);
        command.offset(ByteOffset(0x30)).write_word(config.offset(ByteOffset(0xa8)).word());
        xm1_pipeline_image_secondary_motion_configuration(inputs_selected, input_count, input.offset(ByteOffset(config.offset(ByteOffset(0xa8)).word().wrapping_mul(4))), command.offset(ByteOffset(4)), outputs_selected, output_count, output, command.offset(ByteOffset(24)), config);
        xm1_pipeline_image_conversion_finish_secondary(config.0 as *const u32);
        let _ = callbacks::registry().ensure(xm1_pipeline_image_motion_conversion_complete as unsafe extern "C" fn());
        command.offset(ByteOffset(0x38)).write_word(xm1_pipeline_image_motion_conversion_complete as *const () as u32);
        xm1_pipeline_image_conversion_prepare_mode(2);
        xm1_pipeline_image_trace_begin(0x06023350);
        motion_register_debug(job, 2, request);
    }
    let result = unsafe { conversion_completion(command, 2) };
    unsafe {
        xm1_pipeline_image_copy_motion_words(output, output, config.offset(ByteOffset(0xa8)).word(), reduction, request);
        motion_result_debug(job, 2, Address32(0), output, request);
        xm1_pipeline_image_conversion_wait();
        xm1_pipeline_image_trace_end(0x06023350);
        xm1_pipeline_image_conversion_release();
    }
    result
}

#[derive(Clone, Copy)]
#[repr(u32)]
enum MotionRegister { Control = 0x70, Geometry = 0x74, Windows = 0x78, PrimaryGrid = 0x60, PrimaryOrigin = 0x64, PrimaryStep = 0x68, PrimaryLimit = 0x6c, PrimaryCount = 0x84, SecondaryFlags = 0x88, SecondaryCount = 0x8c, SecondaryOrigin = 0x90, SecondaryStep = 0x94, SecondaryLimit = 0x98 }

unsafe fn motion_register(register: MotionRegister) -> u32 {
    unsafe { Address32(0xfff42000 + register as u32).word() }
}

unsafe fn motion_register_debug(job: Address32, mode: u32, request: u32) {
    if unsafe { calibration(ConfigurationKey(0xa8)) } & 0x80 == 0 { return; }
    let debug = Address32(unsafe { job.offset(ByteOffset(0x220c)).word() });
    unsafe {
        debug.offset(ByteOffset(0xd8b70)).write_word(0x53544d55);
        debug.offset(ByteOffset(0xdb980)).write_word(0x4c414d55);
        let kind = job.offset(ByteOffset(0x4b4)).word();
        let key = if primary_capture(job) { Some(0x472f) } else { match kind { 0x1a => Some(0x493f), 0x1d => Some(0x9fc5), _ => None } };
        let count = match key { Some(key) => calibration(ConfigurationKey(key)), None => 0 };
        debug.offset(ByteOffset(0xd8b74)).write_word(count);
        debug.offset(ByteOffset(0xd8b78)).write_word(xm1_pipeline_image_select_reduction(job, request));
        let control = motion_register(MotionRegister::Control);
        let flags = u32::from(control & 1 != 0) * 8 + u32::from(control & 0x10 != 0) * 4 + u32::from(control & 0x100 != 0) * 2 + u32::from(control & 0x1000 != 0);
        debug.offset(ByteOffset(0xd8b9c)).write_word(flags);
        let geometry = motion_register(MotionRegister::Geometry);
        debug.offset(ByteOffset(0xd8ba4)).write_word(geometry & 0xfff);
        debug.offset(ByteOffset(0xd8ba0)).write_word((geometry >> 16) & 0xfff);
        let windows = motion_register(MotionRegister::Windows);
        debug.offset(ByteOffset(0xd8bac)).write_word(windows & 0x1f);
        debug.offset(ByteOffset(0xd8bb0)).write_word((windows >> 8) & 15);
        debug.offset(ByteOffset(0xd8bb4)).write_word((windows >> 16) & 7);
        debug.offset(ByteOffset(0xd8ba8)).write_word((windows >> 28) & 3);
        if mode == 1 {
            let primary = motion_register(MotionRegister::PrimaryGrid);
            debug.offset(ByteOffset(0xd8b80)).write_word((primary & 0x3f) + 1);
            debug.offset(ByteOffset(0xd8b7c)).write_word(((primary >> 16) & 0x3f) + 1);
            let origin = motion_register(MotionRegister::PrimaryOrigin);
            debug.offset(ByteOffset(0xd8b88)).write_word(origin & 0x1fff);
            debug.offset(ByteOffset(0xd8b84)).write_word((origin >> 16) & 0x1fff);
            let step = motion_register(MotionRegister::PrimaryStep);
            debug.offset(ByteOffset(0xd8b90)).write_word((step & 0xff) + 1);
            debug.offset(ByteOffset(0xd8b8c)).write_word(((step >> 16) & 0xff) + 1);
            let limit = motion_register(MotionRegister::PrimaryLimit);
            debug.offset(ByteOffset(0xd8b98)).write_word((limit & 0xfff) + 1);
            debug.offset(ByteOffset(0xd8b94)).write_word(((limit >> 16) & 0xfff) + 1);
            let count = motion_register(MotionRegister::PrimaryCount);
            debug.offset(ByteOffset(0xd8bbc)).write_word(count & 0x3f);
            debug.offset(ByteOffset(0xd8bb8)).write_word((count >> 16) & 0x3ff);
        } else if mode == 2 {
            let secondary = motion_register(MotionRegister::SecondaryFlags);
            debug.offset(ByteOffset(0xd8bdc)).write_word(secondary & 15);
            for (offset, shift, mask) in [(0xd8bd8, 4, 15), (0xd8bd4, 8, 15), (0xd8bd0, 12, 15), (0xd8bcc, 16, 15), (0xd8bc8, 20, 15), (0xd8bc4, 24, 15), (0xd8bc0, 28, 7)] { debug.offset(ByteOffset(offset)).write_word((secondary >> shift) & mask); }
            debug.offset(ByteOffset(0xd8be0)).write_word(motion_register(MotionRegister::SecondaryCount) & 0x3f);
            let origin = motion_register(MotionRegister::SecondaryOrigin);
            debug.offset(ByteOffset(0xd8be8)).write_word(origin & 0x1ff);
            debug.offset(ByteOffset(0xd8be4)).write_word((origin >> 16) & 0xff);
            let step = motion_register(MotionRegister::SecondaryStep);
            debug.offset(ByteOffset(0xd8bf4)).write_word(step & 15);
            debug.offset(ByteOffset(0xd8bf0)).write_word((step >> 16) & 0xff);
            debug.offset(ByteOffset(0xd8bec)).write_word(step >> 24);
            let limit = motion_register(MotionRegister::SecondaryLimit);
            debug.offset(ByteOffset(0xd8bfc)).write_word(limit & 0x3fff);
            debug.offset(ByteOffset(0xd8bf8)).write_word((limit >> 16) & 0x1ff);
        }
    }
}

unsafe fn copy_debug_bytes(destination: Address32, source: Address32, count: u32) {
    for offset in 0..count { unsafe { destination.offset(ByteOffset(offset)).write_byte(source.offset(ByteOffset(offset)).byte()) }; }
}

unsafe fn motion_result_debug(job: Address32, mode: u32, first: Address32, second: Address32, inhibited: u32) {
    if unsafe { calibration(ConfigurationKey(0xa8)) } & 0x80 == 0 { return; }
    let debug = Address32(unsafe { job.offset(ByteOffset(0x220c)).word() });
    unsafe {
        debug.offset(ByteOffset(0xd8b70)).write_word(0x53544d55);
        debug.offset(ByteOffset(0xdb980)).write_word(0x4c414d55);
    }
    if inhibited != 0 { return; }
    if mode == 1 {
        let count = unsafe { debug.offset(ByteOffset(0xd8b7c)).word() }.wrapping_mul(unsafe { debug.offset(ByteOffset(0xd8b80)).word() }).wrapping_mul(4);
        unsafe { copy_debug_bytes(debug.offset(ByteOffset(0xd8c00)), first, count) };
        return;
    }
    if mode != 2 { return; }
    let reference = unsafe { job.offset(ByteOffset(0x2fe8)).word() };
    let current = unsafe { job.offset(ByteOffset(0x2fe4)).word() };
    if reference == current { return; }
    let mut index = if current <= reference { current.wrapping_sub(1) } else { current };
    if reference <= current {
        if index <= reference { return; }
        index = index.wrapping_sub(2);
    }
    if index > 2 { return; }
    let count = unsafe { debug.offset(ByteOffset(0xd8b7c)).word() }.wrapping_mul(unsafe { debug.offset(ByteOffset(0xd8b80)).word() }).wrapping_mul(4);
    unsafe { copy_debug_bytes(debug.offset(ByteOffset(0xd9400 + index * 0x800)), second, count) };
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_select_reduction(job: Address32, request: u32) -> u32 {
    if request != 0 { return 0; }
    let key = ConfigurationKey(if unsafe { primary_capture(job) } { 0x4730 } else { 0x4940 });
    unsafe { calibration(key) }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_primary_motion_allowed(job: Address32, image: *const ReferencePlane, output: Address32) -> u32 {
    if job.0 == 0 { return 0; }
    let kind = unsafe { job.offset(ByteOffset(0x4b4)).word() };
    let allowed = unsafe { primary_capture(job) } || kind == 0x1a || kind == 0x1d;
    if !allowed || image.is_null() { return 0; }
    u32::from(unsafe { (*image).address.0 } != 0 && output.0 != 0 && unsafe { (*image).flags.0 } & 0x10 == 0)
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_secondary_motion_allowed(job: Address32, _request: u32, inputs: *const *const ReferencePlane, input_count: u32, input: Address32, outputs: *const *const ReferencePlane, output_count: u32, output: Address32) -> u32 {
    if job.0 == 0 { return 0; }
    let kind = unsafe { job.offset(ByteOffset(0x4b4)).word() };
    let key = if unsafe { primary_capture(job) } { 0x471c } else { match kind { 0x1a => 0x492c, 0x1d => 0x9fc5, _ => return 0 } };
    let minimum = unsafe { calibration(ConfigurationKey(key)) };
    if input_count.wrapping_sub(1) < minimum || output_count.wrapping_sub(1) < minimum || input_count != output_count || input.0 == 0 || output.0 == 0 { return 0; }
    for index in 0..minimum as usize {
        let first = unsafe { read_volatile(inputs.add(index)) };
        if first.is_null() || unsafe { (*first).address.0 } == 0 { return 0; }
        let second = unsafe { read_volatile(outputs.add(index)) };
        if second.is_null() || unsafe { (*second).address.0 } == 0 { return 0; }
    }
    let first = unsafe { read_volatile(inputs) };
    let second = unsafe { read_volatile(outputs) };
    u32::from(unsafe { (*first).flags.0 } & 0x10 == 0 && unsafe { (*second).flags.0 } & 0x10 == 0)
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_describe_motion_rows(image: *const ReferencePlane, rows: Address32, _reduction: u32) {
    let flags = unsafe { (*image).flags.0 };
    let (mode, divisor) = if flags & 0x100000 != 0 { (2, 2) } else if flags & 0x20000 != 0 { (0, 2) } else { (1, 1) };
    unsafe {
        rows.write_word(mode);
        rows.offset(ByteOffset(4)).write_word((*image).address.0);
        rows.offset(ByteOffset(8)).write_word((*image).width.0 / divisor);
        rows.offset(ByteOffset(12)).write_word((*image).height.0 as u32);
        rows.offset(ByteOffset(16)).write_word((*image).stride.0);
    }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_primary_motion_output(output: Address32, rows: Address32, config: Address32) {
    let divisor = if unsafe { rows.word() } == 2 { 2 } else { 1 };
    unsafe {
        config.write_word(rows.offset(ByteOffset(16)).word() / divisor);
        config.offset(ByteOffset(4)).write_word(rows.offset(ByteOffset(12)).word());
        config.offset(ByteOffset(8)).write_word(rows.offset(ByteOffset(4)).word());
        config.offset(ByteOffset(12)).write_word(output.0);
    }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_secondary_motion_configuration(inputs: *const *const ReferencePlane, input_count: u32, input: Address32, rows: Address32, outputs: *const *const ReferencePlane, output_count: u32, output: Address32, output_rows: Address32, config: Address32) {
    let divisor = if unsafe { rows.word() } == 2 { 2 } else { 1 };
    unsafe {
        config.write_word(rows.offset(ByteOffset(16)).word() / divisor);
        config.offset(ByteOffset(4)).write_word(rows.offset(ByteOffset(12)).word());
        config.offset(ByteOffset(8)).write_word(rows.offset(ByteOffset(4)).word());
        for index in 1..input_count as usize {
            config.offset(ByteOffset(8 + index as u32 * 4)).write_word((*read_volatile(inputs.add(index))).address.0);
        }
        config.offset(ByteOffset(36)).write_word(output_rows.offset(ByteOffset(4)).word());
        for index in 1..output_count as usize {
            config.offset(ByteOffset(36 + index as u32 * 4)).write_word((*read_volatile(outputs.add(index))).address.0);
        }
        config.offset(ByteOffset(64)).write_word(input.0);
        config.offset(ByteOffset(68)).write_word(output.0);
    }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_copy_motion_words(source: Address32, output: Address32, count: u32, reduction: u32, unchanged: u32) {
    for index in 0..count {
        let value = unsafe { source.offset(ByteOffset(index.wrapping_mul(8))).word() };
        unsafe { output.offset(ByteOffset(index.wrapping_mul(4))).write_word(value) };
    }
    if unchanged != 0 { return; }
    for index in 0..count {
        let word = output.offset(ByteOffset(index.wrapping_mul(4)));
        let value = unsafe { word.word() };
        let shifted = if value == u32::MAX { u32::MAX } else {
            let high = (value >> 16).checked_shl(reduction & 255).unwrap_or(0);
            let low = (value & 0xffff).checked_shl(reduction & 255).unwrap_or(0);
            (high << 16) | low
        };
        unsafe { word.write_word(shifted) };
    }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_motion_conversion_complete() {
    if unsafe { Address32(0x015716d4).word() } != 0 {
        unsafe { xm1_pipeline_image_send_notification(Address32(0x015716d4).word(), 0x10) };
    }
}

#[derive(Clone, Copy)]
enum CaptureKind { Still, Alternate, Continuous }

impl CaptureKind {
    fn parse(value: u32) -> Option<Self> {
        match value { 0x19 => Some(Self::Still), 0x1a => Some(Self::Alternate), 0x1d => Some(Self::Continuous), _ => None }
    }
    fn flags_key(self) -> ConfigurationKey {
        ConfigurationKey(match self { Self::Still => 0x470c, Self::Alternate => 0x491c, Self::Continuous => 0x9fad })
    }
}

unsafe fn reduction_count(kind: CaptureKind, mode: u32, packing: u32) -> u32 {
    let flags = unsafe { calibration(kind.flags_key()) };
    let division = match kind {
        CaptureKind::Still => unsafe { calibration(ConfigurationKey(0x4711)) },
        CaptureKind::Alternate => unsafe { calibration(ConfigurationKey(0x4921)) },
        CaptureKind::Continuous => 0,
    };
    if mode == 0 { return 8; }
    if mode != 1 && mode != 2 { return 0; }
    let result = u32::from(packing != 2) + if flags & 2 != 0 { 2 } else { 0 };
    if mode == 1 && flags & 1 != 0 {
        match division { 0 => (result + 1) >> 1, 1 => (result + 3) >> 2, 2 => (result + 7) >> 3, _ => result }
    } else { result }
}

unsafe fn option_flags(config: Address32, offset: u32, value: u32) {
    for bit in (0..4).rev() { unsafe { config.offset(ByteOffset(offset + bit)).write_byte(((value >> bit) & 1) as u8) }; }
}

unsafe fn store_configuration(config: Address32, offset: ByteOffset, key: ConfigurationKey, bytes: u32, increment: u32) {
    let low = unsafe { calibration(key) };
    if bytes == 1 {
        unsafe { config.offset(offset).write_word(low.wrapping_add(increment)) };
    } else {
        unsafe { config.offset(offset).write_word(low) };
        let high = unsafe { calibration(ConfigurationKey(key.0 + 1)) };
        unsafe { config.offset(offset).write_word((low | high << 8).wrapping_add(increment)) };
    }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_primary_motion_options(job: Address32, _request: u32, packing: u32, config: Address32) {
    let raw_kind = unsafe { job.offset(ByteOffset(0x4b4)).word() };
    let Some(kind) = CaptureKind::parse(if raw_kind == 1 { 0x19 } else { raw_kind }) else { return; };
    let flags = unsafe { calibration(kind.flags_key()) };
    let selection = unsafe { reduction_count(kind, 1, packing) };
    unsafe {
        config.offset(ByteOffset(0x30)).write_word(selection);
        option_flags(config, 0x34, flags);
    }
    match kind {
        CaptureKind::Still | CaptureKind::Alternate => {
            let base = match kind { CaptureKind::Still => 0x4700, _ => 0x4910 };
            unsafe {
                for (offset, key, bytes) in [(0x38, base + 13, 2), (0x3c, base + 15, 2), (0x40, base + 17, 1), (0x44, base + 20, 1), (0x48, base + 19, 1), (0x4c, base + 18, 1), (0x10, base, 1), (0x14, base + 1, 1), (0x18, base + 2, 2), (0x1c, base + 4, 2), (0x20, base + 6, 1), (0x24, base + 7, 1), (0x28, base + 8, 2), (0x2c, base + 10, 2), (0x50, base + 25, 2), (0x54, base + 27, 1)] {
                    if offset == 0x40 && matches!(kind, CaptureKind::Alternate) {
                        config.offset(ByteOffset(offset)).write_word(0);
                    } else {
                        store_configuration(config, ByteOffset(offset), ConfigurationKey(key), bytes, 0);
                    }
                }
            }
        }
        CaptureKind::Continuous => unsafe {
            config.offset(ByteOffset(0x38)).write_word(64);
            config.offset(ByteOffset(0x3c)).write_word(64);
            config.offset(ByteOffset(0x40)).write_word(0);
            for (offset, key, bytes, increment) in [(0x44, 0x9fb5, 1, 0), (0x48, 0x9fb4, 1, 0), (0x4c, 0x9fb3, 1, 0), (0x10, 0x9fb6, 1, 1), (0x14, 0x9fb7, 1, 1), (0x18, 0x9fb8, 2, 0), (0x1c, 0x9fba, 2, 0), (0x20, 0x9fbc, 1, 1), (0x24, 0x9fbd, 1, 1), (0x28, 0x9fbe, 2, 1), (0x2c, 0x9fc0, 2, 1), (0x50, 0x9fc2, 2, 0), (0x54, 0x9fc4, 1, 0)] {
                store_configuration(config, ByteOffset(offset), ConfigurationKey(key), bytes, increment);
            }
        },
    }
    unsafe {
        let cells = config.offset(ByteOffset(0x10)).word().wrapping_mul(config.offset(ByteOffset(0x14)).word());
        config.offset(ByteOffset(0x58)).write_word(cells);
        config.offset(ByteOffset(0x5c)).write_byte(0);
    }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_secondary_motion_options(raw_kind: u32, _request: u32, packing: u32, config: Address32) {
    let Some(kind) = CaptureKind::parse(raw_kind) else { return; };
    let flags = unsafe { calibration(kind.flags_key()) };
    let selection = unsafe { reduction_count(kind, 2, packing) };
    unsafe {
        config.offset(ByteOffset(0x48)).write_word(selection);
        option_flags(config, 0x4c, flags);
    }
    match kind {
        CaptureKind::Still | CaptureKind::Alternate => {
            let base = match kind { CaptureKind::Still => 0x4700, _ => 0x4910 };
            unsafe {
                for (index, key, bytes) in [(0, base + 13, 2), (1, base + 15, 2)] {
                    store_configuration(config, ByteOffset(0x50 + index * 4), ConfigurationKey(key), bytes, 0);
                }
                config.offset(ByteOffset(0x58)).write_word(0);
                for (index, key) in [(3, base + 20), (4, base + 19), (5, base + 18)] {
                    let value = calibration(ConfigurationKey(key));
                    config.offset(ByteOffset(0x50 + index * 4)).write_word(value);
                }
                for index in 6..16 {
                    let value = calibration(ConfigurationKey(base + 28 + index - 6));
                    config.offset(ByteOffset(0x50 + index * 4)).write_word(value);
                }
                for (index, key, bytes) in [(16, base + 38, 2), (17, base + 40, 1), (18, base + 41, 1), (19, base + 42, 1), (20, base + 43, 2), (21, base + 45, 2)] {
                    store_configuration(config, ByteOffset(0x50 + index * 4), ConfigurationKey(key), bytes, 0);
                }
                let first = calibration(ConfigurationKey(base));
                let second = calibration(ConfigurationKey(if matches!(kind, CaptureKind::Still) { base + 1 } else { base }));
                config.offset(ByteOffset(0xa8)).write_word(first.wrapping_mul(second));
            }
        }
        CaptureKind::Continuous => unsafe {
            config.offset(ByteOffset(0x50)).write_word(64);
            config.offset(ByteOffset(0x54)).write_word(64);
            config.offset(ByteOffset(0x58)).write_word(0);
            for (index, key) in [(3, 0x9fb5), (4, 0x9fb4), (5, 0x9fb3), (6, 0x9fc5), (7, 0x9fcb), (8, 0x9fcb), (9, 0x9fcb), (10, 0x9fcb), (11, 0x9fcb), (12, 0x9fcb), (13, 0x9fcc)] {
                let value = calibration(ConfigurationKey(key));
                config.offset(ByteOffset(0x50 + index * 4)).write_word(value);
            }
            config.offset(ByteOffset(0x88)).write_word(32);
            config.offset(ByteOffset(0x8c)).write_word(0);
            config.offset(ByteOffset(0x90)).write_word(192);
            for (index, key, bytes) in [(17, 0x9fd1, 1), (18, 0x9fd2, 1), (19, 0x9fd3, 1), (20, 0x9fd4, 2), (21, 0x9fd6, 2)] {
                store_configuration(config, ByteOffset(0x50 + index * 4), ConfigurationKey(key), bytes, 0);
            }
            let first = calibration(ConfigurationKey(0x9fb6));
            let second = calibration(ConfigurationKey(0x9fb6)).wrapping_add(1);
            config.offset(ByteOffset(0xa8)).write_word(first.wrapping_mul(second).wrapping_add(second));
        },
    }
    unsafe { config.offset(ByteOffset(0xac)).write_byte(0) };
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_affine_corner_matrix(_job: Address32, plane: *const ReferencePlane, matrix: Address32, output: *mut u32) {
    let mut coefficients = [0i64; 6];
    for (index, value) in coefficients.iter_mut().enumerate() {
        *value = unsafe { matrix.offset(ByteOffset(index as u32 * 4)).word() } as i32 as i64;
    }
    let [a, b, c, d, e, f] = coefficients;
    let width = unsafe { (*plane).width.0 }.wrapping_shr(1).wrapping_sub(1) as i64;
    let height = unsafe { (*plane).height.0 }.cast_unsigned().wrapping_sub(1) as i64;
    let determinant = a.wrapping_mul(d).wrapping_sub(b.wrapping_mul(c)) / 65536;
    let mut x = [0i64; 4];
    let mut y = [0i64; 4];
    for (index, (corner_x, corner_y)) in [(0, 0), (width << 16, 0), (width << 16, height << 16), (0, height << 16)].into_iter().enumerate() {
        x[index] = unsafe { __aeabi_ldivmod(d.wrapping_mul(corner_x.wrapping_sub(e)).wrapping_add(b.wrapping_mul(f.wrapping_sub(corner_y))), determinant) };
        y[index] = unsafe { __aeabi_ldivmod(c.wrapping_mul(e.wrapping_sub(corner_x)).wrapping_add(a.wrapping_mul(corner_y.wrapping_sub(f))), determinant) };
    }
    let first_slope = unsafe { __aeabi_ldivmod(y[1].wrapping_sub(y[0]).wrapping_shl(16), x[1].wrapping_sub(x[0])) };
    let mut borders = [0i32; 4];
    for border in &mut borders { *border = unsafe { xm1_pipeline_image_configuration_signed_byte(ConfigurationKey(0x4861)) }; }
    let first = first_slope / 8;
    let values = if !(-8191..8192).contains(&first) {
        [0, 0, 0, 0, 0, height as u32, width as u32, 0]
    } else {
        let second_slope = unsafe { __aeabi_ldivmod(x[2].wrapping_sub(x[1]).wrapping_shl(16), y[2].wrapping_sub(y[1])) };
        let second = second_slope / 8;
        if !(-8191..8192).contains(&second) {
            [0, 0, 0, 0, 0, height as u32, width as u32, 0]
        } else {
            let first_intercept = (y[0].wrapping_sub(first_slope.wrapping_mul(x[0]) / 65536) / 65536) as i32;
            let opposite_intercept = (y[2].wrapping_sub(first_slope.wrapping_mul(x[2]) / 65536) / 65536) as i32;
            let second_intercept = (x[1].wrapping_sub(second_slope.wrapping_mul(y[1]) / 65536) / 65536) as i32;
            let opposite_second = (x[3].wrapping_sub(second_slope.wrapping_mul(y[3]) / 65536) / 65536) as i32;
            [first as u32, first as u32, second as u32, second as u32,
                first_intercept.wrapping_add(borders[0]) as u32,
                opposite_intercept.wrapping_sub(borders[1]) as u32,
                second_intercept.wrapping_sub(borders[2]) as u32,
                opposite_second.wrapping_add(borders[3]) as u32]
        }
    };
    for (index, value) in values.iter().enumerate() { unsafe { write_volatile(output.add(index), *value) }; }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_reduction_count(raw_kind: u32, _request: u32, mode: u32, packing: u32) -> u32 {
    match CaptureKind::parse(raw_kind) {
        Some(kind) => unsafe { reduction_count(kind, mode, packing) },
        None => match mode { 0 => 8, 1 | 2 => u32::from(packing != 2), _ => 0 },
    }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_affine_corners_valid(width: u32, height: i32, matrix: Address32, _reference: u32, corners: *const u32) -> i32 {
    let margin_x = unsafe { read_volatile(corners) } as i32;
    let margin_y = unsafe { read_volatile(corners.add(1)) } as i32;
    let right = (width as i32).wrapping_sub(margin_x).wrapping_sub(1);
    let bottom = height.wrapping_sub(margin_y).wrapping_sub(1);
    let mut coefficients = [0i32; 6];
    for (index, value) in coefficients.iter_mut().enumerate() { *value = unsafe { matrix.offset(ByteOffset(index as u32 * 4)).word() } as i32; }
    for (x, y) in [(margin_x, margin_y), (right, margin_y), (right, bottom), (margin_x, bottom)] {
        let horizontal = (x.wrapping_mul(coefficients[0]) as i64).wrapping_add(y.wrapping_mul(coefficients[1]) as i64).wrapping_add(coefficients[4] as i64).wrapping_add(32768) / 65536;
        let horizontal = horizontal as i32;
        if horizontal < 5 || horizontal >= (width as i32).wrapping_sub(4) { return 0; }
        let vertical = (x.wrapping_mul(coefficients[2]) as i64).wrapping_add(y.wrapping_mul(coefficients[3]) as i64).wrapping_add(coefficients[5] as i64).wrapping_add(32768) / 65536;
        let vertical = vertical as i32;
        if vertical < 5 || vertical >= height.wrapping_sub(4) { return 0; }
    }
    1
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_sample_curve(value: u32, request: *const CurveRequest) -> u32 {
    let request = unsafe { &*request };
    let x = value as i32;
    let (segment, step) = match request.count {
        2 => (0, 8i32),
        3 => (if value & 128 == 0 { 0 } else { 1 }, 16),
        4 => (if value < 85 { 0 } else if value < 170 { 1 } else { 2 }, 24),
        _ => (if value < 64 { 0 } else if value & 128 == 0 { 1 } else if value < 191 { 2 } else { 3 }, 32),
    };
    let distance = x.wrapping_mul(step).wrapping_sub(segment as i32 * 2040);
    let numerator = request.values[segment].wrapping_mul(2040i32.wrapping_sub(distance))
        .wrapping_add(request.values[segment + 1].wrapping_mul(distance)).wrapping_add(127);
    let interpolated = numerator / 255;
    let scaled = interpolated.wrapping_mul(request.scaled).wrapping_mul(8).wrapping_add(500) / 1000;
    let reference = request.scaled.wrapping_mul(request.values[request.selected as usize]).wrapping_mul(64).wrapping_add(500) / 1000;
    let difference = if reference < scaled { scaled.wrapping_sub(reference) } else { reference.wrapping_sub(scaled) };
    let weight = difference.wrapping_mul(125).wrapping_add(64) / 128;
    let normalized = unsafe { __aeabi_idiv(scaled.wrapping_mul(8), request.channel as i32) };
    let contrast = unsafe { __aeabi_idiv((request.limits[1] as i32).wrapping_shl(10), request.limits[2] as i32) };
    let denominator = contrast.wrapping_mul(normalized).wrapping_add(0x100000);
    let response = unsafe { __aeabi_idiv(denominator, normalized.wrapping_add(1024)) };
    let square = response.wrapping_mul(response).wrapping_add(512) / 1024;
    let corrected = unsafe { __aeabi_idiv(2048i32.wrapping_sub(square).wrapping_shl(19), denominator) };
    let weighted = unsafe { __aeabi_idiv(corrected.wrapping_mul(weight).wrapping_mul(32), request.mapped) };
    let divided = unsafe { __aeabi_idiv(weighted, request.limits[0] as i32) } as u32;
    request.mode.wrapping_mul(divided.min(0x1ffffff)).wrapping_add(16) >> 5
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_curve_interval(value: i32, lower: *mut u32, upper: *mut u32, weight: *mut i32, left: i32, middle: i32, right: i32) {
    let (first, second, numerator, denominator) = if value <= left {
        (0, 1, left.wrapping_sub(value).wrapping_mul(256), left)
    } else if value <= middle {
        (1, 2, middle.wrapping_sub(value).wrapping_mul(256), middle.wrapping_sub(left))
    } else if value <= right {
        (2, 3, right.wrapping_sub(value).wrapping_mul(256), right.wrapping_sub(middle))
    } else {
        unsafe { write_volatile(weight, 0); write_volatile(lower, 2); write_volatile(upper, 3); }
        return;
    };
    let blend = unsafe { __aeabi_idiv(numerator, denominator) }.clamp(0, 256);
    unsafe { write_volatile(weight, blend); write_volatile(lower, first); write_volatile(upper, second); }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_build_curve(request: *mut CurveRequest, table: Address32) -> i32 {
    let count = unsafe { (*request).count };
    if count.wrapping_sub(2) > 3 || (count as i32) < unsafe { (*request).selected }.wrapping_add(1) as i32 { return 0; }
    match unsafe { (*request).threshold } {
        1 => {
            unsafe { (*request).mode = 32; }
            let first = unsafe { xm1_pipeline_image_sample_curve(0, request) };
            let last = unsafe { xm1_pipeline_image_sample_curve(255, request) };
            let maximum = first.max(last);
            let numerator = unsafe { (*request).ranges[0] }.wrapping_mul(32).wrapping_add(maximum >> 1);
            unsafe { (*request).mode = __aeabi_uidiv(numerator, maximum); }
        }
        0 => {}
        _ => return 0,
    }
    for sample in 0..256 {
        let value = unsafe { xm1_pipeline_image_sample_curve(sample, request) };
        let mut lower = 0;
        let mut upper = 0;
        let mut weight = 0;
        unsafe {
            xm1_pipeline_image_curve_interval(value as i32, &mut lower, &mut upper, &mut weight, (*request).ranges[1] as i32, (*request).ranges[2] as i32, (*request).ranges[3] as i32);
            table.offset(ByteOffset(sample)).write_byte(lower as u8);
            table.offset(ByteOffset(256 + sample)).write_byte(if weight > 255 { lower } else { upper } as u8);
            table.offset(ByteOffset(512 + sample * 2)).write_half(weight.min(255) as u16);
        }
    }
    1
}

#[unsafe(no_mangle)]
pub(super) extern "C" fn xm1_pipeline_image_conversion_selection(value: u32) -> u32 {
    match value { 0x80 | 0xff => value, 0 => 0xa0, _ => value.wrapping_sub(1) }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_configuration_signed_byte(key: ConfigurationKey) -> i32 {
    unsafe { xm1_buffers_configuration_byte(key) as i8 as i32 }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_configuration_signed_half(key: ConfigurationKey) -> i32 {
    unsafe { xm1_buffers_configuration_half(key) as i16 as i32 }
}

#[unsafe(no_mangle)]
pub(super) extern "C" fn xm1_pipeline_image_conversion_index(value: u32) -> u32 {
    if value == 0 { 0 } else { value.wrapping_sub(1) }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_channel_count() -> u32 {
    let mut calibration = unsafe { super::profile_calibration::physical_calibration() };
    super::profile_calibration::calibration_value(calibration.channel_count()).get()
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_channel_packed_value(channel: u32) -> u32 {
    if (channel as i32) < 0 { return 0xffff; }
    if channel >= unsafe { xm1_pipeline_image_channel_count() } { return 0xffff; }
    let key = 0x3e2007u32.wrapping_add(channel.wrapping_mul(3));
    let low = unsafe { xm1_buffers_configuration_byte(ConfigurationKey(key)) };
    let middle = unsafe { xm1_buffers_configuration_byte(ConfigurationKey(key.wrapping_add(1))) };
    let high = unsafe { xm1_buffers_configuration_byte(ConfigurationKey(key.wrapping_add(2))) };
    low | (middle << 8) | (high << 16)
}
