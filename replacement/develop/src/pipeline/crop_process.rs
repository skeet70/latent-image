use super::shared::{Address32, ByteOffset};
use core::ptr::{read_volatile, write_volatile};

#[derive(Clone, Copy)]
#[repr(transparent)]
pub(super) struct RowBytes(pub(super) u32);
#[derive(Clone, Copy)]
#[repr(transparent)]
pub(super) struct Pixels(pub(super) u32);
#[derive(Clone, Copy)]
#[repr(transparent)]
pub(super) struct Lines(pub(super) u32);
#[derive(Clone, Copy)]
#[repr(transparent)]
pub(super) struct BufferBytes(pub(super) u32);
#[derive(Clone, Copy)]
#[repr(transparent)]
struct ConfigurationKey(u32);
#[derive(Clone, Copy)]
#[repr(C)]
pub(super) struct ImagePlane {
    mode: u32,
    address: Address32,
    size: BufferBytes,
    stride: RowBytes,
    width: RowBytes,
    height: Lines,
    flags: u32,
}
impl ImagePlane {
    const EMPTY: Self = Self { mode: 0, address: Address32(0), size: BufferBytes(0), stride: RowBytes(0), width: RowBytes(0), height: Lines(0), flags: 0 };
    unsafe fn optional(pointer: *const Self) -> Self {
        if pointer.is_null() { Self::EMPTY } else { unsafe { read_volatile(pointer) } }
    }
}
#[repr(C)]
pub(super) struct CropParameters {
    source_mode: u32, target_mode: u32, transform: u32, stride: RowBytes,
    enabled: u32, threshold: u32, alternate: u32, red: u32, green: u32, blue: u32,
    controls: [u32; 4],
}
#[repr(C)]
pub(super) struct CropPacket {
    source_mode: u32, target_mode: u32, transform: u32,
    input: Address32, input_stride: RowBytes, input_width: RowBytes, input_height: Lines,
    output: Address32, output_stride: RowBytes, stride: RowBytes, output_width: RowBytes, output_height: Lines,
    enabled: u32, threshold: u32, alternate: u32, red: u32, green: u32, blue: u32,
    controls: [u32; 4], completion: Option<unsafe extern "C" fn()>,
}
#[repr(C)]
pub(super) struct ScaleRequest { configuration: u32, words: *const [u32; 14] }
#[derive(Clone, Copy)]
#[repr(C)]
pub(super) struct ScalePacket {
    mode: u32, input: Address32, input_stride: Pixels, input_width: Pixels, input_height: Lines,
    output: Address32, output_stride: Pixels, output_width: Pixels, output_height: Lines,
    reserved: [u32; 2], configuration: u32, words: *const [u32; 14], trailing: u32,
    completion: Option<unsafe extern "C" fn()>,
}
#[repr(C)]
pub(super) struct CopyPacket {
    format: u32, width: Pixels, height: Lines, source: Address32, destination: Address32,
    completion: Option<unsafe extern "C" fn()>,
}
unsafe extern "C" {
    fn xm1_buffers_configuration_byte(key: ConfigurationKey) -> u32;
    fn xm1_pipeline_image_trace(text: Address32);
    fn xm1_pipeline_image_trace_begin(operation: u32);
    fn xm1_pipeline_image_trace_end(operation: u32);
    fn xm1_pipeline_image_current_task() -> u32;
    fn xm1_pipeline_image_wait_all(mask: u32, result: *mut u32, timeout: i32) -> i32;
    fn xm1_pipeline_image_send_notification(task: u32, bits: u32) -> i32;
    fn xm1_pipeline_image_geometry_finish() -> u32;
    fn xm1_pipeline_image_crop_initialize() -> i32;
    fn xm1_pipeline_image_crop_publish(packet: *const CropPacket);
    fn xm1_pipeline_image_crop_begin();
    fn xm1_pipeline_image_crop_wait() -> i32;
    fn xm1_pipeline_image_crop_release() -> i32;
}
fn halt() -> ! { loop { core::hint::spin_loop(); } }
unsafe fn notify(slot: Address32) {
    let task = unsafe { slot.word() };
    if task != 0 { unsafe { xm1_pipeline_image_send_notification(slot.word(), 0x10) }; }
}
#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_crop_complete() { unsafe { notify(Address32(0x0157bcfc)) }; }
#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_scale_complete() { unsafe { notify(Address32(0x0157bd00)) }; }
#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_copy_complete() { unsafe { notify(Address32(0x0157bd04)) }; }

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_process_crop(stage: u32, parameters: *const CropParameters, input: *const ImagePlane, output: *mut ImagePlane) -> u32 {
    unsafe { xm1_pipeline_image_trace(Address32(0xe66698)) };
    let p = unsafe { &*parameters };
    if p.stride.0 & 31 != 0 { halt(); }
    unsafe { xm1_pipeline_image_crop_initialize() };
    let i = unsafe { read_volatile(input) };
    let o = unsafe { read_volatile(output) };
    let packet = CropPacket {
        source_mode: p.source_mode, target_mode: p.target_mode, transform: p.transform,
        input: i.address, input_stride: i.stride, input_width: i.width, input_height: i.height,
        output: o.address, output_stride: o.stride, stride: p.stride, output_width: o.width, output_height: o.height,
        enabled: p.enabled, threshold: p.threshold, alternate: p.alternate, red: p.red, green: p.green, blue: p.blue,
        controls: p.controls, completion: Some(xm1_pipeline_image_crop_complete),
    };
    unsafe {
        write_volatile(0x0157a100 as *mut CropPacket, packet);
        xm1_pipeline_image_crop_publish(0x0157a100 as *const CropPacket);
        xm1_pipeline_image_crop_debug(stage);
        xm1_pipeline_image_trace_begin(0x06023360);
        xm1_pipeline_image_trace(Address32(0xe666a4));
        Address32(0x0157bcfc).write_word(xm1_pipeline_image_current_task());
        xm1_pipeline_image_crop_begin();
    }
    let mut events = 0;
    unsafe {
        xm1_pipeline_image_wait_all(0x10, &mut events, -1);
        Address32(0x0157bcfc).write_word(0);
        xm1_pipeline_image_crop_wait();
        xm1_pipeline_image_trace(Address32(0xe666ac));
        xm1_pipeline_image_trace_end(0x06023360);
        xm1_pipeline_image_crop_release();
        xm1_pipeline_image_trace(Address32(0xe666b4));
    }
    u32::from(events & 0x20 == 0)
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_scale_configuration(_stage: u32, request: *const ScaleRequest, input: *const ImagePlane, output: *mut ImagePlane) -> u32 {
    unsafe { xm1_pipeline_image_trace(Address32(0xe666c0)); xm1_pipeline_image_scale_initialize() };
    let r = unsafe { &*request };
    let configuration = unsafe { read_volatile(r.words) };
    let i = unsafe { read_volatile(input) };
    let o = unsafe { read_volatile(output) };
    let packet = ScalePacket {
        mode: 1, input: i.address, input_stride: Pixels(i.stride.0 >> 1), input_width: Pixels(i.width.0 >> 1), input_height: i.height,
        output: o.address, output_stride: Pixels(o.stride.0 >> 1), output_width: Pixels(o.width.0 >> 1), output_height: o.height,
        reserved: [0; 2], configuration: r.configuration, words: &configuration, trailing: 0,
        completion: Some(xm1_pipeline_image_scale_complete),
    };
    unsafe {
        write_volatile(0x01575ae8 as *mut ScalePacket, packet);
        xm1_pipeline_image_scale_publish(0x01575ae8 as *const ScalePacket);
        xm1_pipeline_image_trace_begin(0x06023340);
        xm1_pipeline_image_trace(Address32(0xe666cc));
        Address32(0x0157bd00).write_word(xm1_pipeline_image_current_task());
        xm1_pipeline_image_scale_start();
    }
    let mut events = 0;
    unsafe {
        xm1_pipeline_image_wait_all(0x10, &mut events, -1);
        Address32(0x0157bd00).write_word(0);
        xm1_pipeline_image_scale_cancel();
        xm1_pipeline_image_trace(Address32(0xe666d4));
        xm1_pipeline_image_trace_end(0x06023340);
        xm1_pipeline_image_scale_release();
        xm1_pipeline_image_trace(Address32(0xe666dc));
    }
    u32::from(events & 0x20 == 0)
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_copy_configuration(_stage: u32, format: *const u32, input: *const ImagePlane, output: *mut ImagePlane) -> u32 {
    unsafe { xm1_pipeline_image_trace(Address32(0xe666e8)); xm1_pipeline_image_copy_initialize() };
    for offset in (0..24).step_by(4) { unsafe { Address32(0x01575b24 + offset).write_word(0) }; }
    if input.is_null() || output.is_null() { halt(); }
    let i = unsafe { read_volatile(input) };
    let o = unsafe { read_volatile(output) };
    if (i.address.0 | o.address.0 | (i.width.0 >> 1) | i.height.0) & 7 != 0 { halt(); }
    let packet = CopyPacket { format: unsafe { read_volatile(format) }, width: Pixels(i.width.0 >> 1), height: i.height, source: i.address, destination: o.address, completion: Some(xm1_pipeline_image_copy_complete) };
    unsafe {
        write_volatile(0x01575b24 as *mut CopyPacket, packet);
        xm1_pipeline_image_copy_publish(0x01575b24 as *const CopyPacket);
        xm1_pipeline_image_trace_begin(0x06023380);
        xm1_pipeline_image_trace(Address32(0xe666f0));
        Address32(0x0157bd04).write_word(xm1_pipeline_image_current_task());
        xm1_pipeline_image_copy_start();
    }
    let mut events = 0;
    unsafe {
        xm1_pipeline_image_wait_all(0x10, &mut events, -1);
        Address32(0x0157bd04).write_word(0);
        xm1_pipeline_image_copy_release();
        xm1_pipeline_image_trace_end(0x06023380);
        xm1_pipeline_image_trace(Address32(0xe666f8));
        xm1_pipeline_image_trace(Address32(0xe66700));
    }
    u32::from(events & 0x20 == 0)
}

unsafe fn debug_buffer(stage: u32) -> Address32 { Address32(unsafe { Address32(stage).offset(ByteOffset(0x220c)).word() }) }
#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_geometry_debug(stage: u32) {
    if unsafe { xm1_buffers_configuration_byte(ConfigurationKey(0xa8)) } & 0x80 == 0 { return; }
    let buffer = unsafe { debug_buffer(stage) };
    let index = unsafe { buffer.offset(ByteOffset(0xdbf34)).word() }.min(12);
    unsafe { buffer.offset(ByteOffset(0xdbb10)).write_word(0x53544443) };
    let timestamp_buffer = unsafe { debug_buffer(stage) };
    let timestamp = unsafe { xm1_pipeline_image_geometry_finish() };
    unsafe { timestamp_buffer.offset(ByteOffset(0xdbb14 + index * 88)).write_word(timestamp) };
    for ordinal in 0..21 {
        let destination = unsafe { debug_buffer(stage) }.offset(ByteOffset(0xdbb18 + index * 88 + ordinal * 4));
        let register = if ordinal < 2 { 0xfff48060 + ordinal * 4 } else { 0xfff48070 + (ordinal - 2) * 4 };
        unsafe { destination.write_word(Address32(register).word()) };
    }
    unsafe { debug_buffer(stage).offset(ByteOffset(0xdbf38)).write_word(0x4c414443) };
    let buffer = unsafe { debug_buffer(stage) };
    let counter = buffer.offset(ByteOffset(0xdbf34));
    unsafe { counter.write_word(counter.word().wrapping_add(1)) };
}
#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_crop_debug(stage: u32) {
    if unsafe { xm1_buffers_configuration_byte(ConfigurationKey(0xa8)) } & 0x80 == 0 { return; }
    let index = unsafe { debug_buffer(stage).offset(ByteOffset(0xdbb08)).word() }.min(24);
    unsafe { debug_buffer(stage).offset(ByteOffset(0xdb984)).write_word(0x53545052) };
    for (ordinal, register) in [0xfff43008, 0xfff43030, 0xfff43034, 0xfff43038].into_iter().enumerate() {
        let destination = unsafe { debug_buffer(stage) }.offset(ByteOffset(0xdb988 + index * 16 + ordinal as u32 * 4));
        unsafe { destination.write_word(Address32(register).word()) };
    }
    unsafe { debug_buffer(stage).offset(ByteOffset(0xdbb0c)).write_word(0x4c415052) };
    let buffer = unsafe { debug_buffer(stage) };
    let counter = buffer.offset(ByteOffset(0xdbb08));
    unsafe { counter.write_word(counter.word().wrapping_add(1)) };
}

#[repr(C)]
pub(super) struct ProcessingRequest {
    operation: u32, selection: u32, parameter: Address32, coefficients: Address32,
    color: Address32, option: u32, auxiliary: Address32, scale_option: u32,
}
#[derive(Clone, Copy)]
#[repr(u32)]
enum ProcessingOperation {
    Color = 0, Coefficients = 1, Raw = 2, Develop = 3, Reduced = 5,
    Auxiliary = 6, Bypass = 7, Alternate = 8, Mode9 = 9, Mode10 = 10, Mode11 = 11, Mode12 = 12,
}
impl ProcessingOperation {
    fn parse(raw: u32) -> Self {
        match raw {
            0 => Self::Color, 1 => Self::Coefficients, 2 => Self::Raw, 3 => Self::Develop,
            5 => Self::Reduced, 6 => Self::Auxiliary, 7 => Self::Bypass, 8 => Self::Alternate,
            9 => Self::Mode9, 10 => Self::Mode10, 11 => Self::Mode11, 12 => Self::Mode12,
            _ => halt(),
        }
    }
    fn configuration(self, auxiliary: bool) -> u32 {
        match self {
            Self::Color | Self::Develop => 8, Self::Coefficients => 17, Self::Raw => 13,
            Self::Reduced => 6, Self::Auxiliary => if auxiliary { 2 } else { 0 },
            Self::Bypass | Self::Alternate => 0,
            Self::Mode9 | Self::Mode10 | Self::Mode11 | Self::Mode12 => 21,
        }
    }
}
unsafe extern "C" {
    fn xm1_pipeline_image_prepare_processing(stage: u32, request: *const ProcessingRequest, source: *mut ImagePlane, flags: *mut u32, operation: u32, option: u32, argument: i32, output: Address32, output_pointer: Address32, original: *const ImagePlane);
    fn xm1_pipeline_image_setup_processing(stage: u32, work_a: Address32, work_b: Address32, saved: Address32, operation: u32, scale: u32, option: u32, first_flags: u32, third_flags: u32, request_option: u32, output: Address32);
    fn xm1_pipeline_image_saved_request_submit(request: Address32);
    fn xm1_pipeline_image_sensor_initialize(mode: u32) -> i32;
    fn xm1_pipeline_image_sensor_start(mode: u32);
    fn xm1_pipeline_image_sensor_wait(mode: u32) -> i32;
    fn xm1_pipeline_image_sensor_release(mode: u32) -> i32;
    fn xm1_pipeline_image_sensor_configure(packet: Address32);
    fn xm1_pipeline_image_sensor_read_registers(mode: u32, output: Address32);
    fn xm1_develop_lookup_width(profile: u32, aspect: u32) -> u32;
}
fn packing(flags: u32, honor_byte: bool) -> u32 {
    if honor_byte && flags & 0x10000 != 0 { 0 } else if flags & 0x10 != 0 { 1 } else { 2 }
}
#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_process_planes(stage: u32, request: *const ProcessingRequest, first: *const ImagePlane, second: *const ImagePlane, third: *const ImagePlane, fourth: *const ImagePlane, fifth: *const ImagePlane, argument: i32) -> u32 {
    unsafe { xm1_pipeline_image_trace(Address32(0xe66648)) };
    let packet = Address32(0x015759f8);
    for offset in (0..0xf0).step_by(4) { unsafe { packet.offset(ByteOffset(offset)).write_word(0) }; }
    let mut source = unsafe { ImagePlane::optional(first) };
    let b = unsafe { ImagePlane::optional(second) };
    let c = unsafe { ImagePlane::optional(third) };
    let d = unsafe { ImagePlane::optional(fourth) };
    let e = unsafe { ImagePlane::optional(fifth) };
    let r = unsafe { &*request };
    let operation = ProcessingOperation::parse(r.operation);
    let mode = operation.configuration(e.address.0 != 0);
    let put = |offset, value| unsafe { packet.offset(ByteOffset(offset)).write_word(value) };
    put(0, mode);
    for (offset, plane) in [(4, source), (0x18, b), (0x2c, c), (0x48, e), (0x64, d)] {
        put(offset, plane.address.0);
        put(offset + 4, plane.stride.0);
        put(offset + 8, plane.width.0);
        put(offset + 12, plane.height.0);
    }
    put(0x14, packing(source.flags, true));
    put(0x28, packing(b.flags, true));
    put(0x44, packing(c.flags, true));
    put(0x60, packing(e.flags, false));
    put(0x7c, 2);
    let ratio = if d.height.0 == 0 { 0 } else {
        let denominator = d.height.0;
        match c.height.0 / denominator { 2 => 1, 4 => 2, 8 => 3, 16 => 4, _ => halt() }
    };
    put(0x80, ratio);
    put(0x8c, unsafe { xm1_buffers_configuration_byte(ConfigurationKey(0xc0a4)) });
    match operation {
        ProcessingOperation::Color => {
            for (index, value) in [0x100, 0x100, 0x100, r.color.0].into_iter().enumerate() {
                unsafe { Address32(0x0157b9c0 + index as u32 * 4).write_word(value) };
            }
            put(0xe8, 0x0157b9c0);
        }
        ProcessingOperation::Coefficients => {
            for index in 0..7 { unsafe { Address32(0x0157a0b4 + index * 4).write_word(if index == 5 { r.coefficients.0 } else { 0 }) }; }
            put(0xc4, 0x0157a0b4);
        }
        _ => {
            let mut flags = match operation {
                ProcessingOperation::Raw => 0x200,
                ProcessingOperation::Bypass => 0x448,
                ProcessingOperation::Develop | ProcessingOperation::Auxiliary | ProcessingOperation::Alternate | ProcessingOperation::Reduced => {
                    let unused = unsafe { xm1_pipeline_image_saved_request_unused(stage) } & 1;
                    let accepted = unsafe { xm1_pipeline_image_saved_request_accept(stage) } & 1;
                    (if matches!(operation, ProcessingOperation::Reduced) { 0x881 } else { 0xcc9 }) | (unused << 5) | (accepted << 1)
                }
                _ => 0,
            };
            unsafe { xm1_pipeline_image_prepare_processing(stage, request, &mut source, &mut flags, r.operation, r.option, argument, packet.offset(ByteOffset(0x90)), packet.offset(ByteOffset(0xc4)), first) };
        }
    }
    unsafe { packet.offset(ByteOffset(0x84)).write_half(r.selection as u16) };
    put(0x88, r.parameter.0);
    put(0xec, xm1_pipeline_image_processing_complete as usize as u32);
    unsafe { xm1_pipeline_image_sensor_initialize(packet.word()) };
    let scale = if r.operation < 2 { 0x1000 } else {
        let frame = Address32(stage);
        let profile = unsafe { frame.offset(ByteOffset(0x644)).word() };
        let aspect = unsafe { frame.offset(ByteOffset(0x4f0)).word() };
        let width = unsafe { xm1_develop_lookup_width(profile, aspect) };
        let denominator = width.wrapping_mul(2) / 3;
        divide_unsigned((unsafe { frame.offset(ByteOffset(0x494)).word() }).wrapping_shl(12), denominator)
    };
    unsafe { xm1_pipeline_image_setup_processing(stage, Address32(0x0157a15c), Address32(0x0157a760), Address32(0x0157ba9c), r.operation, scale, r.scale_option, source.flags, c.flags, r.option, packet.offset(ByteOffset(0x90))) };
    if r.operation < 13 && (1u32 << r.operation) & 0x18c0 != 0 {
        unsafe { Address32(stage).offset(ByteOffset(0x27b0)).write_word(packet.offset(ByteOffset(0xb0)).word()) };
    }
    if r.operation.wrapping_sub(6) < 3 {
        unsafe { Address32(stage).offset(ByteOffset(0x27b4)).write_word(packet.offset(ByteOffset(0xa0)).word()) };
    }
    unsafe {
        xm1_pipeline_image_sensor_configure(packet);
        xm1_pipeline_image_processing_debug(packet.word(), stage);
        xm1_pipeline_image_trace_begin(0x06023320);
        xm1_pipeline_image_trace(Address32(0xe66654));
        Address32(0x0157bcf0).write_word(xm1_pipeline_image_current_task());
        xm1_pipeline_image_sensor_start(packet.word());
    }
    let mut events = 0;
    unsafe {
        xm1_pipeline_image_wait_all(0x30, &mut events, -1);
        Address32(0x0157bcf0).write_word(0);
        xm1_pipeline_image_sensor_wait(packet.word());
        xm1_pipeline_image_trace_end(0x06023320);
        xm1_pipeline_image_trace(Address32(0xe6665c));
        xm1_pipeline_image_sensor_release(packet.word());
        xm1_pipeline_image_saved_request_submit(Address32(0x0157ba9c));
        xm1_pipeline_image_trace(Address32(0xe66664));
    }
    u32::from(events & 0x20 == 0)
}
#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_processing_complete() { unsafe { notify(Address32(0x0157bcf0)) }; }
#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_processing_debug(mode: u32, stage: u32) {
    if unsafe { xm1_buffers_configuration_byte(ConfigurationKey(0xa8)) } & 0x80 == 0 { return; }
    let buffer = unsafe { debug_buffer(stage) };
    if buffer.0 == 0 { return; }
    unsafe {
        buffer.offset(ByteOffset(0xc315c)).write_word(0x53545243);
        debug_buffer(stage).offset(ByteOffset(0xd7e50)).write_word(0x4c415243);
        xm1_pipeline_image_sensor_read_registers(mode, Address32(0x01575b3c));
    }
    let buffer = unsafe { debug_buffer(stage) };
    let index = unsafe { buffer.offset(ByteOffset(0xd75b0)).word() };
    if index > 3 { return; }
    let destination = buffer.offset(ByteOffset(0xc5f60 + index * 0x4594));
    for offset in (0..0x455c).step_by(4) {
        unsafe { destination.offset(ByteOffset(offset)).write_word(Address32(0x01575b3c + offset).word()) };
    }
    unsafe { debug_buffer(stage).offset(ByteOffset(0xd75b0)).write_word(index + 1) };
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_saved_request_accept(_stage: u32) -> u32 { 1 }
#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_saved_request_unused(_stage: u32) -> u32 { 0 }

#[derive(Clone, Copy)]
#[repr(transparent)]
struct InterruptLine(u32);
#[derive(Clone, Copy)]
#[repr(transparent)]
struct ResourceHandle(u32);
#[repr(C)]
pub(super) struct CopyHardwareConfiguration {
    format: u32, shift: u32, enabled: u32, length: u32, width: Pixels, height: Lines,
    source: Address32, destination: Address32,
}
unsafe extern "C" {
    fn xm1_pipeline_image_route_request(selector: u32, slot: u32, level: u32) -> i32;
    fn xm1_pipeline_image_route_acquire(index: u32, handle: Address32) -> i32;
    fn xm1_pipeline_image_route_release(handle: ResourceHandle) -> i32;
    fn xm1_pipeline_image_interrupt_acquire(source: InterruptLine, callback: unsafe extern "C" fn(), handle: Address32) -> i32;
    fn xm1_pipeline_image_interrupt_release(handle: ResourceHandle) -> i32;
    fn xm1_pipeline_image_wait_reserved(handle: u32, timeout: u32) -> i32;
    fn xm1_pipeline_image_signal_reserved(handle: u32) -> i32;
    fn xm1_pipeline_image_copy_cache_flush(value: u32, option: u32) -> u32;
    fn xm1_develop_enter_critical() -> u32;
    fn xm1_develop_leave_critical(token: u32);
    fn xm1_pipeline_image_copy_reset();
}
const COPY_RUNTIME: Address32 = Address32(0x014ce6c8);
const COPY_RESOURCE: Address32 = Address32(0x00955964);
unsafe fn copy_runtime(offset: u32) -> u32 { unsafe { COPY_RUNTIME.offset(ByteOffset(offset)).word() } }
unsafe fn copy_store(offset: u32, value: u32) { unsafe { COPY_RUNTIME.offset(ByteOffset(offset)).write_word(value) }; }

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_copy_acquire() {
    let result = unsafe { xm1_pipeline_image_wait_reserved(6, u32::MAX) };
    unsafe { COPY_RESOURCE.offset(ByteOffset(8)).write_word(result as u32) };
    for offset in [0, 4, 8] { unsafe { COPY_RESOURCE.offset(ByteOffset(offset)).write_word(0) }; }
    unsafe { xm1_pipeline_image_route_request(0x304, 35, 5) };
    if unsafe { xm1_pipeline_image_route_acquire(23, COPY_RESOURCE) } == 0 {
        unsafe { COPY_RESOURCE.offset(ByteOffset(4)).write_word(1) };
    }
}
#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_copy_device_release() {
    if unsafe { COPY_RESOURCE.offset(ByteOffset(4)).word() } != 0 {
        unsafe {
            xm1_pipeline_image_route_release(ResourceHandle(COPY_RESOURCE.word()));
            COPY_RESOURCE.offset(ByteOffset(4)).write_word(0);
        }
    }
    unsafe { xm1_pipeline_image_route_request(0x304, 35, 0); xm1_pipeline_image_signal_reserved(6) };
}
#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_copy_initialize() -> i32 {
    unsafe { xm1_pipeline_image_copy_acquire() };
    for offset in (0..48).step_by(4) { unsafe { copy_store(offset, 0) }; }
    unsafe { xm1_pipeline_image_copy_reset(); Address32(0xfff6f020).write_word(0x10030003) };
    0
}
#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_copy_release() -> i32 {
    unsafe { xm1_pipeline_image_copy_device_release() };
    0
}
#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_copy_start() {
    unsafe {
        Address32(0xfff6f024).write_word(0x10000000);
        xm1_pipeline_image_copy_cache_flush(0, 0);
        Address32(0xfff6f004).write_word(1);
    }
}
#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_copy_program(configuration: *const CopyHardwareConfiguration) -> i32 {
    let c = unsafe { &*configuration };
    if c.format >= 6 { return -30; }
    unsafe {
        Address32(0xfff6f008).write_word((c.format & 7) | ((c.shift & 3) << 4));
        Address32(0xfff6f00c).write_word(((c.enabled & 7) << 28) | ((c.length & 0x3ff) << 16));
    }
    if c.width.0 > 8192 || c.height.0 > 8192 || (c.width.0 | c.height.0) & 7 != 0 { return -30; }
    unsafe { Address32(0xfff6f010).write_word(((c.width.0.wrapping_sub(1) & 0x1fff) << 16) | (c.height.0.wrapping_sub(1) & 0x1fff)) };
    if c.source.0 & 7 != 0 { return -30; }
    unsafe { Address32(0xfff6f014).write_word(c.source.0 & 0x1fffffff) };
    if c.destination.0 & 7 != 0 { return -30; }
    unsafe { Address32(0xfff6f018).write_word(c.destination.0 & 0x1fffffff) };
    0
}
#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_copy_publish(packet: *const CopyPacket) -> i32 {
    for index in 0..6 {
        let value = unsafe { read_volatile((packet as *const u32).add(index)) };
        unsafe { copy_store(8 + index as u32 * 4, value) };
    }
    let width = unsafe { copy_runtime(12) };
    let height = unsafe { copy_runtime(16) };
    let combined = width | height;
    let shift = if combined & 31 == 0 { 2 } else if combined & 15 == 0 { 1 } else if combined & 7 == 0 { 0 } else {
        unsafe { copy_store(36, 0); copy_store(40, 0) };
        return -30;
    };
    unsafe { copy_store(32, shift); copy_store(36, 0); copy_store(40, 0) };
    let configuration = CopyHardwareConfiguration {
        format: unsafe { copy_runtime(8) }, shift, enabled: 0, length: 0,
        width: Pixels(width), height: Lines(height), source: Address32(unsafe { copy_runtime(20) }), destination: Address32(unsafe { copy_runtime(24) }),
    };
    let result = unsafe { xm1_pipeline_image_copy_program(&configuration) };
    if result != 0 { return result; }
    unsafe { copy_store(4, 1) };
    let result = unsafe { xm1_pipeline_image_interrupt_acquire(InterruptLine(0x24), xm1_pipeline_image_copy_interrupt, COPY_RUNTIME) };
    if result != 0 { unsafe { copy_store(4, 0) }; }
    result
}
unsafe fn copy_drop_interrupt() {
    if unsafe { copy_runtime(4) } != 0 {
        if unsafe { xm1_pipeline_image_interrupt_release(ResourceHandle(copy_runtime(0))) } != 0 { halt(); }
        unsafe { copy_store(4, 0) };
    }
}
unsafe fn completion_at(slot: Address32) -> Option<unsafe extern "C" fn()> {
    unsafe { read_volatile(slot.0 as *const Option<unsafe extern "C" fn()>) }
}
unsafe fn invoke_completion(callback: Option<unsafe extern "C" fn()>) {
    if let Some(callback) = callback { unsafe { callback() }; }
}
#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_copy_interrupt() {
    let token = unsafe { xm1_develop_enter_critical() };
    let status = unsafe { Address32(0xfff6f01c).word() };
    unsafe {
        copy_store(44, status);
        Address32(0xfff6f020).write_word(status);
        xm1_pipeline_image_copy_reset();
        Address32(0xfff6f024).write_word(0);
        copy_drop_interrupt();
    }
    let callback = unsafe { completion_at(COPY_RUNTIME.offset(ByteOffset(28))) };
    unsafe { copy_store(28, 0); xm1_develop_leave_critical(token); invoke_completion(callback) };
}
#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_copy_cancel() {
    let token = unsafe { xm1_develop_enter_critical() };
    let callback = unsafe { completion_at(COPY_RUNTIME.offset(ByteOffset(28))) };
    unsafe { copy_store(28, 0) };
    if callback.is_some() {
        unsafe { xm1_pipeline_image_copy_reset(); Address32(0xfff6f020).write_word(0x10030003) };
    }
    unsafe { copy_drop_interrupt(); xm1_develop_leave_critical(token); invoke_completion(callback) };
}

const SCALE_RUNTIME: Address32 = Address32(0x00943250);
const SCALE_RESOURCE: Address32 = Address32(0x009412c8);
#[repr(C)]
pub(super) struct ScaleCoefficients {
    xx: i32, xy: i32, x_fraction: i32, x_shift: u32, x_bias: i32,
    yx: i32, yy: i32, y_fraction: i32, y_shift: u32, y_bias: i32,
    origin_x: Pixels, origin_y: Pixels, offset_x: Pixels, offset_y: Pixels,
}
#[derive(Clone, Copy)]
enum CoefficientEncoding { Horizontal, Vertical, Fraction, Bias }
impl CoefficientEncoding {
    fn encode(self, value: i32) -> u32 {
        let raw = value as u32;
        let (prefix, maximum, mask) = match self {
            Self::Horizontal => (0xfffe0000, 0x1ffff, 0x3ffff),
            Self::Vertical => (0xfffc0000, 0x1ffff, 0x3ffff),
            Self::Fraction => (0xfffffe00, 0x1ff, 0x3ff),
            Self::Bias => (0xc0000000, 0x1fffffff, 0x3fffffff),
        };
        if raw & prefix != prefix && raw > maximum { halt(); }
        raw & mask
    }
}
#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_scale_acquire() {
    for offset in [0, 4, 8] { unsafe { SCALE_RESOURCE.offset(ByteOffset(offset)).write_word(0) }; }
    let result = unsafe { xm1_pipeline_image_wait_reserved(5, u32::MAX) };
    unsafe {
        SCALE_RESOURCE.offset(ByteOffset(8)).write_word(result as u32);
        xm1_pipeline_image_route_request(0x405, 16, 8);
    }
    if unsafe { xm1_pipeline_image_route_acquire(21, SCALE_RESOURCE) } == 0 {
        unsafe { SCALE_RESOURCE.offset(ByteOffset(4)).write_word(1) };
    }
}
#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_scale_device_release() {
    if unsafe { SCALE_RESOURCE.offset(ByteOffset(4)).word() } != 0 {
        unsafe {
            xm1_pipeline_image_route_release(ResourceHandle(SCALE_RESOURCE.word()));
            SCALE_RESOURCE.offset(ByteOffset(4)).write_word(0);
        }
    }
    unsafe { xm1_pipeline_image_route_request(0x405, 16, 0); xm1_pipeline_image_signal_reserved(5) };
}
#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_scale_initialize() -> i32 {
    unsafe { xm1_pipeline_image_scale_acquire() };
    for offset in (0..80).step_by(4) { unsafe { SCALE_RUNTIME.offset(ByteOffset(offset)).write_word(0) }; }
    unsafe { Address32(0xfff47000).write_word(0) };
    0
}
#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_scale_release() -> i32 {
    unsafe { xm1_pipeline_image_scale_device_release() };
    0
}
#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_scale_cancel() -> i32 {
    let token = unsafe { xm1_develop_enter_critical() };
    unsafe {
        SCALE_RUNTIME.offset(ByteOffset(56)).write_word(0);
        xm1_develop_leave_critical(token);
        Address32(0xfff47000).write_word(0);
    }
    if unsafe { SCALE_RUNTIME.offset(ByteOffset(76)).word() } == 0 { return 0; }
    let handle = ResourceHandle(unsafe { SCALE_RUNTIME.offset(ByteOffset(72)).word() });
    let result = unsafe { xm1_pipeline_image_interrupt_release(handle) };
    if result == 0 { unsafe { SCALE_RUNTIME.offset(ByteOffset(76)).write_word(0) }; }
    result
}
#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_scale_start() {
    unsafe {
        xm1_pipeline_image_copy_cache_flush(0, 0);
        Address32(0xfff47004).write_word(SCALE_RUNTIME.word() & 3);
    }
}
#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_scale_select_burst(packet: *const ScalePacket, output: *mut [u32; 3]) -> i32 {
    let p = unsafe { &*packet };
    let mut size = p.configuration;
    if size == 0 {
        size = if p.trailing == 0 { 64 } else { 32 };
        let ratio = divide_unsigned(p.input_width.0.wrapping_shl(12), p.output_width.0);
        while ratio.wrapping_mul(size).wrapping_add(4095) > 0xa0fff {
            size >>= 1;
            if size < 16 { halt(); }
        }
    }
    let mode = match size { 64 => 0, 32 => 1, 16 => 2, _ => halt() };
    unsafe { write_volatile(output, [mode, 0, 0]) };
    0
}
#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_scale_coefficients(coefficients: *const ScaleCoefficients) {
    let c = unsafe { &*coefficients };
    unsafe {
        Address32(0xfff47040).write_word(CoefficientEncoding::Horizontal.encode(c.xx));
        Address32(0xfff47044).write_word(CoefficientEncoding::Horizontal.encode(c.xy));
    }
    let fraction = CoefficientEncoding::Fraction.encode(c.x_fraction);
    if c.x_shift > 15 { halt(); }
    unsafe {
        Address32(0xfff47048).write_word(fraction | (c.x_shift << 16));
        Address32(0xfff4704c).write_word(CoefficientEncoding::Bias.encode(c.x_bias));
        Address32(0xfff47050).write_word(CoefficientEncoding::Vertical.encode(c.yx));
        Address32(0xfff47054).write_word(CoefficientEncoding::Vertical.encode(c.yy));
    }
    let fraction = CoefficientEncoding::Fraction.encode(c.y_fraction);
    if c.y_shift > 15 { halt(); }
    unsafe {
        Address32(0xfff47058).write_word(fraction | (c.y_shift << 16));
        Address32(0xfff4705c).write_word(CoefficientEncoding::Bias.encode(c.y_bias));
    }
    if c.origin_x.0 > 8191 || c.origin_y.0 > 8191 { halt(); }
    unsafe { Address32(0xfff47060).write_word(c.origin_x.0 | (c.origin_y.0 << 16)) };
    if c.offset_x.0 > 8191 || c.offset_y.0 > 8191 { halt(); }
    unsafe { Address32(0xfff47064).write_word(c.offset_x.0 | (c.offset_y.0 << 16)) };
}
#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_scale_program(packet: *const ScalePacket, burst: *const [u32; 3]) -> i32 {
    let p = unsafe { &*packet };
    let burst = unsafe { read_volatile(burst) };
    if burst[0] > 2 { halt(); }
    unsafe { Address32(0xfff47008).write_word(burst[0]) };
    if p.input.0 & 15 != 0 { halt(); }
    unsafe { Address32(0xfff47010).write_word(p.input.0 & 0x3fffffff) };
    if p.input_stride.0 > 32767 || p.input_stride.0 & 7 != 0 { halt(); }
    unsafe { Address32(0xfff47014).write_word(p.input_stride.0) };
    if p.input_width.0 > 8191 || p.input_height.0 > 8191 || p.input_width.0 & 7 != 0 { halt(); }
    unsafe { Address32(0xfff47018).write_word(p.input_width.0 | (p.input_height.0 << 16)) };
    if p.output.0 & 15 != 0 { halt(); }
    unsafe { Address32(0xfff47020).write_word(p.output.0 & 0x3fffffff) };
    if p.output_stride.0 > 32767 || p.output_stride.0 & 7 != 0 { halt(); }
    unsafe { Address32(0xfff47024).write_word(p.output_stride.0) };
    if p.output_width.0 > 8191 || p.output_height.0 > 8191 || p.output_width.0 & 7 != 0 { halt(); }
    unsafe { Address32(0xfff47028).write_word(p.output_width.0 | (p.output_height.0 << 16)) };
    if p.reserved[0] > 8191 || p.reserved[1] > 8191 || p.reserved[0] & 7 != 0 { halt(); }
    unsafe { Address32(0xfff4702c).write_word(p.reserved[0] | (p.reserved[1] << 16)) };
    if !p.words.is_null() { unsafe { xm1_pipeline_image_scale_coefficients(p.words as *const ScaleCoefficients) }; }
    if burst[1] > 255 || burst[2] > 255 { halt(); }
    let horizontal = burst[1] | if burst[1] == 0 { 0 } else { 0x100 };
    let vertical = burst[2] | if burst[2] == 0 { 0 } else { 0x100 };
    unsafe {
        Address32(0xfff47070).write_word(horizontal | (vertical << 16));
        Address32(0xfff4707c).write_word(0x1301);
    }
    let (first, last) = if p.trailing == 0 { (0, 255) } else {
        let range = Address32(p.trailing);
        unsafe { (range.word(), range.offset(ByteOffset(4)).word()) }
    };
    if last > 255 || first > 3 { halt(); }
    unsafe { Address32(0xfff47080).write_word(last | (first << 16)) };
    0
}
#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_scale_publish(packet: *const ScalePacket) -> i32 {
    unsafe {
        write_volatile(SCALE_RUNTIME.0 as *mut ScalePacket, read_volatile(packet));
        xm1_pipeline_image_scale_select_burst(packet, SCALE_RUNTIME.offset(ByteOffset(60)).0 as *mut [u32; 3]);
    }
    let result = unsafe { xm1_pipeline_image_interrupt_acquire(InterruptLine(0x3a), xm1_pipeline_image_scale_interrupt, SCALE_RUNTIME.offset(ByteOffset(72))) };
    if result != 0 { return result; }
    unsafe {
        SCALE_RUNTIME.offset(ByteOffset(76)).write_word(1);
        xm1_pipeline_image_scale_program(packet, SCALE_RUNTIME.offset(ByteOffset(60)).0 as *const [u32; 3])
    }
}
#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_scale_interrupt() {
    let token = unsafe { xm1_develop_enter_critical() };
    let status = unsafe { Address32(0xfff47074).word() };
    unsafe { Address32(0xfff47078).write_word(status); xm1_develop_leave_critical(token) };
    if status & 1 == 0 {
        if status & 0x1300 != 0 { halt(); }
        return;
    }
    unsafe { Address32(0xfff4707c).write_word(0); Address32(0xfff47000).write_word(0) };
    let token = unsafe { xm1_develop_enter_critical() };
    let callback = unsafe { completion_at(SCALE_RUNTIME.offset(ByteOffset(56))) };
    unsafe {
        SCALE_RUNTIME.offset(ByteOffset(56)).write_word(0);
        xm1_develop_leave_critical(token);
        invoke_completion(callback);
    }
}

unsafe extern "C" { fn image_operation_divide_fault(value: u32) -> !; }
fn divide_unsigned(numerator: u32, denominator: u32) -> u32 {
    if denominator == 0 {
        unsafe { image_operation_divide_fault(if numerator == 0 { 0 } else { u32::MAX }) };
    }
    numerator / denominator
}
