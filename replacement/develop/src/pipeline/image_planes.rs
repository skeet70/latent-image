use super::callbacks::{self, CallbackWord};
use super::shared::{Address32, ByteOffset};
use core::ptr::{read_volatile, write_volatile};

#[derive(Clone, Copy)]
#[repr(C)]
pub struct ImagePlane {
    pub format: u32,
    pub address: u32,
    pub length: u32,
    pub stride: u32,
    pub width: u32,
    pub height: u32,
    pub flags: u32,
}

#[derive(Clone, Copy)]
#[repr(transparent)]
pub(super) struct TaskId(pub(super) u32);

#[derive(Clone, Copy)]
#[repr(transparent)]
pub(super) struct TaskPriority(pub(super) u32);

#[derive(Clone, Copy)]
enum FillPattern {
    Neutral,
    White,
    Grey,
    Red,
    Green,
    Blue,
    Custom(u32),
}

impl FillPattern {
    fn parse(pattern: i32, custom: u32) -> Self {
        match pattern {
            1 => Self::White,
            2 => Self::Grey,
            3 => Self::Red,
            4 => Self::Green,
            5 => Self::Blue,
            6 => Self::Custom(custom),
            _ => Self::Neutral,
        }
    }

    fn packed(self) -> u32 {
        match self {
            Self::Neutral => 0x8080_0000,
            Self::White => 0x8080_fefe,
            Self::Grey => 0x8080_8080,
            Self::Red => 0xff55_4c4c,
            Self::Green => 0x162c_9595,
            Self::Blue => 0x6bff_1d1d,
            Self::Custom(value) => value,
        }
    }
}

#[derive(Clone, Copy)]
#[repr(transparent)]
pub(super) struct InterfaceIndex(pub(super) u32);

pub(super) unsafe fn interface(index: InterfaceIndex) -> Address32 {
    Address32(unsafe { Address32(0x0096_bd20).offset(ByteOffset(index.0.wrapping_mul(4))).word() })
}

pub(super) unsafe fn current_interface() -> InterfaceIndex {
    let table = unsafe { interface(InterfaceIndex(0)) };
    InterfaceIndex(unsafe { callbacks::kernel_slot::<unsafe extern "C" fn() -> u32>(table.0, 0x1618) }())
}

unsafe fn interface_task(index: InterfaceIndex) -> TaskId {
    let table = unsafe { interface(index) };
    TaskId(unsafe { callbacks::kernel_slot::<unsafe extern "C" fn() -> u32>(table.0, 0x16c) }())
}

pub(super) unsafe fn current_task() -> TaskId {
    unsafe { interface_task(current_interface()) }
}

unsafe fn encoded_task(task: TaskId) -> u32 {
    let low = task.0 & 0xffff;
    let namespace = unsafe { Address32(0x007c_ad9c).offset(ByteOffset(low.wrapping_sub(1).wrapping_mul(4))).word() };
    low | namespace.wrapping_shl(16)
}

#[derive(Clone, Copy)]
#[repr(u32)]
enum DispatchEvent {
    Priority = 5,
    Wait = 0x11,
    Notify = 0x13,
    SignalCurrent = 0x14,
    WaitReserved = 0x18,
    SignalReserved = 0x19,
}

#[derive(Clone, Copy)]
#[repr(transparent)]
struct DispatchObjectId(u32);

unsafe fn record_dispatch_error(event: DispatchEvent, object: DispatchObjectId, result: i32) {
    unsafe { image_planes_thread_event(event as u32, object.0, result) };
    unsafe { Address32(0x40a0_f82c).write_word(result as u32) };
    let entry = unsafe { Address32(0x4089_ea8c).word() };
    let _ = callbacks::call_interface_signed_argument(callbacks::registry(), CallbackWord(entry as usize), result);
}

pub(super) unsafe fn configure_priority(task: TaskId, priority: TaskPriority) -> i32 {
    let index = unsafe { current_interface() };
    let task = if task.0 == 0 {
        unsafe { interface_task(index) }
    } else {
        task
    };
    let table = unsafe { interface(index) };
    let result = unsafe { callbacks::kernel_slot::<unsafe extern "C" fn(u32, u32) -> i32>(table.0, 0x1c)(encoded_task(task), priority.0) };
    if result != 0 {
        unsafe { record_dispatch_error(DispatchEvent::Priority, DispatchObjectId(task.0), result) };
    }
    result
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_current_task() -> u32 {
    unsafe { current_task() }.0
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_configure_task(task: u32, priority: u32) -> i32 {
    unsafe { configure_priority(TaskId(task), TaskPriority(priority)) }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_wait_all(mask: u32, output: *mut u32, timeout: i32) -> i32 {
    let index = unsafe { current_interface() };
    let table = unsafe { interface(index) };
    let first = unsafe { interface_task(index) };
    let namespace = unsafe { encoded_task(first) } & 0xffff_0000;
    let second = unsafe { interface_task(index) };
    let result = unsafe { callbacks::kernel_slot::<unsafe extern "C" fn(u32, u32, u32, *mut u32, i32) -> i32>(table.0, 0x9c)(namespace | (second.0 & 0xffff), mask, 1, output, timeout) };
    if result != 0 && result != -50 {
        let task = unsafe { interface_task(index) };
        unsafe { record_dispatch_error(DispatchEvent::Wait, DispatchObjectId(task.0), result) };
    }
    result
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_send_notification(task: u32, value: u32) -> i32 {
    let index = unsafe { current_interface() };
    let table = unsafe { interface(index) };
    let mode = unsafe { callbacks::kernel_slot::<unsafe extern "C" fn() -> u32>(table.0, 0x148) }();
    let table = unsafe { interface(index) };
    let offset = if mode == 1 { 0x8c } else { 0x88 };
    let task = TaskId(task);
    let result = unsafe { callbacks::kernel_slot::<unsafe extern "C" fn(u32, u32) -> i32>(table.0, offset)(encoded_task(task), value) };
    if result != 0 {
        unsafe { record_dispatch_error(DispatchEvent::Notify, DispatchObjectId(task.0), result) };
    }
    result
}

#[derive(Clone, Copy)]
#[repr(transparent)]
struct ReservedHandle(u32);

unsafe fn encoded_reserved(handle: ReservedHandle) -> u32 {
    let low = handle.0 & 0xffff;
    let namespace = if (handle.0 as i32) < 170 {
        unsafe { Address32(0x0093_e6f4).offset(ByteOffset(low.wrapping_sub(1).wrapping_mul(4))).word() }
    } else {
        1
    };
    low | namespace.wrapping_shl(16)
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_wait_reserved(handle: u32, timeout: u32) -> i32 {
    let index = unsafe { current_interface() };
    let table = unsafe { interface(index) };
    let result = unsafe { callbacks::kernel_slot::<unsafe extern "C" fn(u32, u32) -> i32>(table.0, 0x7c)(encoded_reserved(ReservedHandle(handle)), timeout) };
    if result != 0 && result != -50 {
        unsafe { record_dispatch_error(DispatchEvent::WaitReserved, DispatchObjectId(handle), result) };
    }
    result
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_signal_reserved(handle: u32) -> i32 {
    let index = unsafe { current_interface() };
    let table = unsafe { interface(index) };
    let mode = unsafe { callbacks::kernel_slot::<unsafe extern "C" fn() -> u32>(table.0, 0x148) }();
    let table = unsafe { interface(index) };
    let offset = if mode == 1 { 0x70 } else { 0x6c };
    let result = unsafe { callbacks::kernel_slot::<unsafe extern "C" fn(u32) -> i32>(table.0, offset)(encoded_reserved(ReservedHandle(handle))) };
    if result != 0 {
        unsafe { record_dispatch_error(DispatchEvent::SignalReserved, DispatchObjectId(handle), result) };
    }
    result
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_signal_current_zero() -> i32 {
    let index = unsafe { current_interface() };
    let table = unsafe { interface(index) };
    let first = unsafe { interface_task(index) };
    let namespace = unsafe { encoded_task(first) } & 0xffff_0000;
    let second = unsafe { interface_task(index) };
    let result = unsafe { callbacks::kernel_slot::<unsafe extern "C" fn(u32, u32) -> i32>(table.0, 0x90)(namespace | (second.0 & 0xffff), 0) };
    if result != 0 {
        let task = unsafe { interface_task(index) };
        unsafe { record_dispatch_error(DispatchEvent::SignalCurrent, DispatchObjectId(task.0), result) };
    }
    result
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_fill_pattern(
    cancel: *const u8,
    plane: *const ImagePlane,
    pattern: i32,
    custom: u32,
    task: u32,
    restore_priority: u32,
) -> i32 {
    let width = unsafe { read_volatile(core::ptr::addr_of!((*plane).width)) };
    let stride = unsafe { read_volatile(core::ptr::addr_of!((*plane).stride)) };
    let mut address = Address32(unsafe { read_volatile(core::ptr::addr_of!((*plane).address)) });
    let value = FillPattern::parse(pattern, custom).packed();
    let gap = stride.wrapping_sub(width);
    let words = width >> 2;
    unsafe { configure_priority(TaskId(task), TaskPriority(12)) };
    let mut line = 0;
    let mut cancelled = 0;
    while line < unsafe { read_volatile(core::ptr::addr_of!((*plane).height)) } {
        if unsafe { read_volatile(cancel) } == 1 {
            cancelled = 1;
            break;
        }
        for _ in 0..words {
            unsafe { address.write_word(value) };
            address = address.offset(ByteOffset(4));
        }
        address = address.offset(ByteOffset(gap));
        line += 1;
    }
    unsafe { configure_priority(TaskId(task), TaskPriority(restore_priority)) };
    cancelled
}

#[no_mangle]
pub extern "C" fn xm1_pipeline_image_stage_ready() -> i32 {
    1
}

#[no_mangle]
pub extern "C" fn xm1_pipeline_image_stage_complete() -> i32 {
    1
}

#[no_mangle]
pub extern "C" fn xm1_pipeline_image_stage_inactive() -> i32 {
    0
}

#[no_mangle]
pub extern "C" fn xm1_pipeline_image_output_disabled(_stage: u32, _request: u32) -> i32 {
    0
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_store_triplets(
    stage: *mut u8,
    values: *const u16,
    secondary: i32,
) -> i32 {
    let first = unsafe { read_volatile(values) };
    let second = unsafe { read_volatile(values.add(1)) };
    let third = unsafe { read_volatile(values.add(2)) };
    if secondary == 0 {
        for offset in [0x2224, 0x2230] {
            let target = unsafe { stage.add(offset).cast::<u16>() };
            unsafe {
                write_volatile(target, first);
                write_volatile(target.add(1), second);
                write_volatile(target.add(2), third);
            }
        }
    } else {
        let target = unsafe { stage.add(0x71c).cast::<u16>() };
        unsafe {
            write_volatile(target, first);
            write_volatile(target.add(1), second);
            write_volatile(target.add(2), third);
        }
    }
    1
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_copy_rows(
    _stage: u32,
    source: *const ImagePlane,
    destination: *const ImagePlane,
    _flags: u32,
) -> i32 {
    let mut source_address = Address32(unsafe { read_volatile(core::ptr::addr_of!((*source).address)) });
    let source_stride = unsafe { read_volatile(core::ptr::addr_of!((*source).stride)) };
    let width = unsafe { read_volatile(core::ptr::addr_of!((*source).width)) };
    let height = unsafe { read_volatile(core::ptr::addr_of!((*source).height)) };
    let mut destination_address = Address32(unsafe { read_volatile(core::ptr::addr_of!((*destination).address)) });
    let destination_stride = unsafe { read_volatile(core::ptr::addr_of!((*destination).stride)) };
    for _ in 0..height {
        for byte in 0..width {
            let value = unsafe { source_address.offset(ByteOffset(byte)).byte() };
            unsafe { destination_address.offset(ByteOffset(byte)).write_byte(value) };
        }
        source_address = source_address.offset(ByteOffset(source_stride));
        destination_address = destination_address.offset(ByteOffset(destination_stride));
    }
    1
}

#[derive(Clone, Copy)]
#[repr(transparent)]
struct DebugFileKind(i32);

#[derive(Clone, Copy)]
#[repr(transparent)]
struct ConfigurationKey(u32);

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_publish_debug_file(
    _stage: u32,
    kind: i32,
    image: *const ImagePlane,
    number: u32,
    suffix: *const u8,
) {
    let kind = DebugFileKind(kind);
    let row = Address32(0x00e5_40b0).offset(ByteOffset((kind.0 as u32).wrapping_mul(12)));
    let bit = unsafe { row.offset(ByteOffset(4)).byte() };
    let key = ConfigurationKey(unsafe { row.word() });
    let options = unsafe { image_planes_configuration_byte(key.0) };
    let mask = if bit < 32 { 1u32 << bit } else { 0 };
    if bit != 255 && (options & mask) as u8 != 0 {
        unsafe { image_planes_trace_begin(0x0601_0700) };
        let address = unsafe { read_volatile(core::ptr::addr_of!((*image).address)) };
        let height = unsafe { read_volatile(core::ptr::addr_of!((*image).height)) };
        let size = if height == 0 {
            unsafe { read_volatile(core::ptr::addr_of!((*image).length)) }
        } else {
            unsafe { read_volatile(core::ptr::addr_of!((*image).stride)) }.wrapping_mul(height)
        };
        unsafe { xm1_pipeline_image_write_debug_file(kind.0, address, size, number, suffix) };
        unsafe { image_planes_trace_end(0x0601_0700) };
    }
}

#[derive(Clone, Copy)]
#[repr(u32)]
enum PlaneRequestKind {
    Auxiliary = 0,
    Special = 1,
    OutputCopy = 2,
    OutputTarget = 3,
    OutputMask = 4,
    OutputStage = 5,
}

#[repr(C)]
struct PlaneRequest {
    kind: PlaneRequestKind,
    first: u8,
    second: u8,
    reserved_6: u16,
    reserved_8: u32,
    reserved_c: u32,
    reserved_10: u32,
    selection: i32,
    completion: u32,
    enabled: u32,
}

impl PlaneRequest {
    fn new(kind: PlaneRequestKind) -> Self {
        Self {
            kind, first: 0, second: 0, reserved_6: 0, reserved_8: 0,
            reserved_c: 0, reserved_10: 0, selection: 0, completion: 0, enabled: 0,
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_prepare_output_request(
    stage: *mut u8,
    source: *const ImagePlane,
    auxiliary: *const ImagePlane,
    destination: *mut ImagePlane,
    template: *const ImagePlane,
    selection: i32,
) -> u32 {
    let stage_address = Address32(stage as u32);
    let first = unsafe { stage_address.offset(ByteOffset(0x4a0)).word() };
    let second = unsafe { stage_address.offset(ByteOffset(0x4a4)).word() };
    if unsafe { xm1_develop_capture_option_selection() } == 2 {
        return 1;
    }
    if stage.is_null() || source.is_null() || auxiliary.is_null() || destination.is_null() || template.is_null() {
        loop { core::hint::spin_loop(); }
    }
    let request = 0x0154_d940 as *mut PlaneRequest;
    unsafe { write_volatile(request, PlaneRequest::new(PlaneRequestKind::OutputCopy)) };
    let image = 0x0154_cbe4 as *mut ImagePlane;
    unsafe { write_volatile(image, read_volatile(template)) };
    let stride = unsafe { stage_address.offset(ByteOffset(0x42c)).word() }.wrapping_shl(1);
    unsafe {
        write_volatile(core::ptr::addr_of_mut!((*image).stride), stride);
        write_volatile(core::ptr::addr_of_mut!((*image).width), stride);
        write_volatile(core::ptr::addr_of_mut!((*image).height), 1);
        write_volatile(core::ptr::addr_of_mut!((*request).first), first as u8);
        write_volatile(core::ptr::addr_of_mut!((*request).second), second as u8);
        write_volatile(core::ptr::addr_of_mut!((*request).selection), selection);
        write_volatile(core::ptr::addr_of_mut!((*request).enabled), 1);
        xm1_pipeline_image_process_planes(stage, request.cast(), source, auxiliary, image, 0, destination, 0)
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_prepare_auxiliary_plane(
    stage: *mut u8,
    source: *mut ImagePlane,
    destination: *mut ImagePlane,
    divisor: i32,
) -> u32 {
    if unsafe { image_planes_configuration_byte(0xec) } == 1 {
        let flags = unsafe { read_volatile(core::ptr::addr_of!((*source).flags)) };
        unsafe { write_volatile(core::ptr::addr_of_mut!((*source).flags), flags | 0x10) };
    }
    if unsafe { read_volatile(core::ptr::addr_of!((*source).address)) } != 0x437c_ee00 {
        return 1;
    }
    let request = 0x0154_cc00 as *mut PlaneRequest;
    unsafe { write_volatile(request, PlaneRequest::new(PlaneRequestKind::Auxiliary)) };
    unsafe { write_volatile(core::ptr::addr_of_mut!((*request).reserved_10), (0xa00i32 / divisor) as u32) };
    unsafe { xm1_pipeline_image_process_planes(stage, request.cast(), source, core::ptr::null(), core::ptr::null(), 0, destination, 0) }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_prepare_special_plane(
    stage: *mut u8,
    source: *const ImagePlane,
    auxiliary: *const ImagePlane,
    destination: *const ImagePlane,
    divisor: u16,
) -> u32 {
    let request = 0x0154_d100 as *mut PlaneRequest;
    unsafe { write_volatile(request, PlaneRequest::new(PlaneRequestKind::Special)) };
    unsafe { write_volatile(core::ptr::addr_of_mut!((*request).reserved_c), 0x280u32 / u32::from(divisor)) };
    let original_source = source;
    let source = unsafe { read_volatile(source) };
    let image = 0x0154_ce6c as *mut ImagePlane;
    let (width, stride) = if source.flags & 0x10 != 0 {
        (source.width.wrapping_shl(2) / 3, source.stride.wrapping_shl(2) / 3)
    } else {
        (source.width, source.stride)
    };
    unsafe {
        write_volatile(image, ImagePlane {
            format: source.format, address: 0x5fa2_7b60, length: stride, stride,
            width, height: 1, flags: 0x0010_0000,
        });
    }
    let target = 0x0154_cd44 as *mut ImagePlane;
    let target_address = unsafe { read_volatile(core::ptr::addr_of!((*destination).address)) };
    unsafe {
        write_volatile(target, ImagePlane { address: target_address, ..source });
        xm1_pipeline_image_process_planes(stage, request.cast(), original_source, auxiliary, image, 0, target, 0)
    }
}

#[repr(C)]
pub(super) struct SoftImageRequest {
    source: u32,
    destination: u32,
    source_width: u32,
    source_height: u32,
    destination_width: u32,
    destination_height: u32,
    source_stride: u32,
    destination_stride: u32,
    reserved: [u32; 4],
    scale: u32,
    rotation: u32,
}

#[derive(Clone, Copy)]
enum QuarterTurn {
    None,
    Clockwise,
    Half,
    Counterclockwise,
}

impl QuarterTurn {
    fn parse(rotation: i32) -> Self {
        match rotation {
            1 => Self::Clockwise,
            2 => Self::Half,
            3 => Self::Counterclockwise,
            _ => Self::None,
        }
    }

    fn degrees(self) -> u32 {
        match self {
            Self::None => 0,
            Self::Clockwise => 90,
            Self::Half => 180,
            Self::Counterclockwise => 270,
        }
    }
}

unsafe fn soft_request(
    source: *const ImagePlane,
    destination: *const ImagePlane,
    rotation: QuarterTurn,
) -> SoftImageRequest {
    SoftImageRequest {
        source: unsafe { read_volatile(core::ptr::addr_of!((*source).address)) },
        destination: unsafe { read_volatile(core::ptr::addr_of!((*destination).address)) },
        source_width: unsafe { read_volatile(core::ptr::addr_of!((*source).width)) } >> 1,
        source_height: unsafe { read_volatile(core::ptr::addr_of!((*source).height)) },
        destination_width: unsafe { read_volatile(core::ptr::addr_of!((*destination).width)) } >> 1,
        destination_height: unsafe { read_volatile(core::ptr::addr_of!((*destination).height)) },
        source_stride: unsafe { read_volatile(core::ptr::addr_of!((*source).stride)) } >> 1,
        destination_stride: unsafe { read_volatile(core::ptr::addr_of!((*destination).stride)) } >> 1,
        reserved: [0; 4],
        scale: 128,
        rotation: rotation.degrees(),
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_resize_software(
    source: *const ImagePlane,
    destination: *const ImagePlane,
) -> i32 {
    let task = unsafe { current_task() };
    let request = 0x0154_cea8 as *mut SoftImageRequest;
    unsafe { write_volatile(request, soft_request(source, destination, QuarterTurn::None)) };
    unsafe { image_planes_task_log(current_task().0 & 255, 0x00e5_4348 as *const u8) };
    unsafe { configure_priority(task, TaskPriority(12)) };
    unsafe { xm1_pipeline_image_scale_packed_buffer(request) };
    unsafe { configure_priority(task, TaskPriority(6)) };
    unsafe { image_planes_task_log(current_task().0 & 255, 0x00e5_435c as *const u8) };
    1
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_rotate_software(
    _stage: u32,
    source: *const ImagePlane,
    destination: *const ImagePlane,
    rotation: i32,
) -> i32 {
    let request = 0x0154_d900 as *mut SoftImageRequest;
    unsafe { write_volatile(request, soft_request(source, destination, QuarterTurn::parse(rotation))) };
    unsafe { image_planes_task_log(current_task().0 & 255, 0x00e5_4370 as *const u8) };
    unsafe { xm1_pipeline_image_rotate_packed_buffer(request) };
    unsafe { image_planes_task_log(current_task().0 & 255, 0x00e5_4388 as *const u8) };
    1
}

unsafe fn special_resize_profile(stage: Address32) -> bool {
    if unsafe { stage.offset(ByteOffset(0x4b4)).word() } != 0x20 {
        return false;
    }
    matches!(unsafe { stage.offset(ByteOffset(0x530)).word() }, 3 | 4 | 5 | 13)
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_resize_capture(
    stage: *mut u8,
    source: *const ImagePlane,
    destination: *mut ImagePlane,
    enabled: i32,
    _first: u32,
    _second: u32,
    _third: u32,
    operation: i32,
) -> i32 {
    let frame = Address32(stage as u32);
    let mut input = unsafe { read_volatile(source) };
    if operation != 0 {
        if operation == 4 {
            unsafe {
                write_volatile(core::ptr::addr_of_mut!((*destination).width), input.width);
                write_volatile(core::ptr::addr_of_mut!((*destination).height), input.height);
            }
            if input.flags & 0x0400_0000 != 0 {
                input.flags &= !0x0400_0000;
                let flags = unsafe { read_volatile(core::ptr::addr_of!((*destination).flags)) };
                unsafe { write_volatile(core::ptr::addr_of_mut!((*destination).flags), flags | 0x0400_0000) };
            }
        }
        unsafe { xm1_pipeline_image_resize_planes(stage, enabled, 0, &input, destination, &input, core::ptr::null_mut(), operation) };
    } else {
        let profile = unsafe { frame.offset(ByteOffset(0x644)).word() };
        let aspect = unsafe { frame.offset(ByteOffset(0x4f0)).word() };
        if unsafe { special_resize_profile(frame) } {
            if aspect == 3 {
                let height = unsafe { image_planes_lookup_height(profile, aspect) };
                unsafe {
                    write_volatile(core::ptr::addr_of_mut!((*destination).height), height);
                    write_volatile(core::ptr::addr_of_mut!((*destination).width), (height.wrapping_mul(3) & !1).wrapping_add(31) & !31);
                }
            } else {
                let width = unsafe { image_planes_lookup_width(profile, aspect) }.wrapping_shl(1);
                unsafe {
                    write_volatile(core::ptr::addr_of_mut!((*destination).width), width);
                    write_volatile(core::ptr::addr_of_mut!((*destination).height), width / 3);
                }
            }
        } else {
            let width = unsafe { image_planes_lookup_width(profile, aspect) }.wrapping_shl(1);
            unsafe { write_volatile(core::ptr::addr_of_mut!((*destination).width), width) };
            let height = if aspect == 3 {
                unsafe { image_planes_lookup_height(profile, aspect) }
            } else {
                width / 3
            };
            unsafe { write_volatile(core::ptr::addr_of_mut!((*destination).height), height) };
            if aspect == 3 {
                unsafe { write_volatile(core::ptr::addr_of_mut!((*destination).length), width.wrapping_mul(height)) };
            }
            unsafe { write_volatile(core::ptr::addr_of_mut!((*destination).stride), width) };
        }
        unsafe { xm1_pipeline_image_resize_planes(stage, enabled, if aspect == 3 { 2 } else { 0 }, &input, destination, &input, core::ptr::null_mut(), 0) };
        if !unsafe { special_resize_profile(frame) } && unsafe { frame.offset(ByteOffset(0x4f0)).word() } != 3 {
            let address = unsafe { read_volatile(core::ptr::addr_of!((*destination).address)) };
            let stride = unsafe { read_volatile(core::ptr::addr_of!((*destination).stride)) };
            let height = unsafe { read_volatile(core::ptr::addr_of!((*destination).height)) };
            let profile = unsafe { frame.offset(ByteOffset(0x644)).word() };
            let aspect = unsafe { frame.offset(ByteOffset(0x4f0)).word() };
            let target_height = unsafe { image_planes_lookup_height(profile, aspect) };
            unsafe { write_volatile(core::ptr::addr_of_mut!((*destination).address), address.wrapping_add(stride.wrapping_mul(height.wrapping_sub(target_height)) >> 1)) };
            let width = unsafe { image_planes_lookup_width(profile, aspect) }.wrapping_shl(1);
            unsafe { write_volatile(core::ptr::addr_of_mut!((*destination).width), width) };
            let height = unsafe { image_planes_lookup_height(profile, aspect) };
            unsafe {
                write_volatile(core::ptr::addr_of_mut!((*destination).height), height);
                write_volatile(core::ptr::addr_of_mut!((*destination).length), width.wrapping_mul(height));
            }
        }
    }
    let flags = if enabled == 0 {
        (unsafe { read_volatile(core::ptr::addr_of!((*source).flags)) }) & !0x0400_0000
    } else {
        (unsafe { read_volatile(core::ptr::addr_of!((*destination).flags)) }) | 0x0200_0000
    };
    unsafe { write_volatile(core::ptr::addr_of_mut!((*destination).flags), flags) };
    1
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_resize_through_crop(
    stage: *mut u8,
    source: *const ImagePlane,
    destination: *mut ImagePlane,
    auxiliary: *mut ImagePlane,
    enabled: i32,
) -> u32 {
    let frame = Address32(stage as u32);
    let special = unsafe { frame.offset(ByteOffset(0x4b4)).word() } == 0x1d;
    let (width, height) = if special {
        (0x500, 0x3c0)
    } else {
        (unsafe { image_planes_lookup_width(4, 0) }, unsafe { image_planes_lookup_height(4, 0) })
    };
    let source_width = unsafe { read_volatile(core::ptr::addr_of!((*source).width)) } >> 1;
    let source_height = unsafe { read_volatile(core::ptr::addr_of!((*source).height)) };
    let destination_width = unsafe { read_volatile(core::ptr::addr_of!((*destination).width)) } >> 1;
    let destination_height = unsafe { read_volatile(core::ptr::addr_of!((*destination).height)) };
    let intermediate = 0x0154_cd28 as *mut ImagePlane;
    let reduced = width < source_width && height < source_height && destination_width < width
        && !auxiliary.is_null() && destination_height < height;
    if reduced {
        let mut plane = unsafe { read_volatile(auxiliary) };
        let mut plane_width = width.wrapping_mul(2);
        let mut plane_height = height;
        plane.address = plane.address
            .wrapping_add(plane.stride.wrapping_mul(plane.height.wrapping_sub(height) >> 1))
            .wrapping_add(plane.stride.wrapping_sub(plane_width) >> 1);
        if special {
            let index = unsafe { frame.offset(ByteOffset(0x5d0)).word() }.wrapping_mul(5)
                .wrapping_add(unsafe { frame.offset(ByteOffset(0x3214)).word() });
            plane_height = unsafe { Address32(0x00e5_4018).offset(ByteOffset(index.wrapping_mul(4))).word() };
            plane.address = plane.address.wrapping_add(plane.stride.wrapping_mul(height.wrapping_sub(plane_height) >> 1));
        } else {
            match unsafe { frame.offset(ByteOffset(0x4f0)).word() } {
                1 => {
                    plane_height = 0x1aa;
                    plane.address = plane.address.wrapping_add(plane.stride.wrapping_mul(height.wrapping_sub(plane_height) >> 1));
                }
                2 => {
                    plane_height = 0x168;
                    plane.address = plane.address.wrapping_add(plane.stride.wrapping_mul(height.wrapping_sub(plane_height) >> 1));
                }
                3 => {
                    plane_width = height.wrapping_mul(2);
                    plane.address = plane.address.wrapping_add(plane.stride.wrapping_sub(plane_width) >> 1);
                }
                _ => {}
            }
        }
        plane.width = plane_width;
        plane.height = plane_height;
        plane.length = plane_width.wrapping_mul(plane_height);
        unsafe {
            write_volatile(intermediate, plane);
            xm1_pipeline_image_resize_planes(stage, enabled, 0, source, intermediate, source, core::ptr::null_mut(), 2);
            write_volatile(auxiliary, read_volatile(intermediate));
        }
    } else {
        unsafe { write_volatile(intermediate, read_volatile(source)) };
    }
    let result = unsafe { xm1_pipeline_image_resize_planes(stage, if reduced || enabled == 0 { 0 } else { 1 }, 0, intermediate, destination, intermediate, core::ptr::null_mut(), 2) };
    let flags = if enabled == 0 {
        (unsafe { read_volatile(core::ptr::addr_of!((*source).flags)) }) & !0x0400_0000
    } else {
        (unsafe { read_volatile(core::ptr::addr_of!((*destination).flags)) }) | 0x0200_0000
    };
    unsafe { write_volatile(core::ptr::addr_of_mut!((*destination).flags), flags) };
    result
}

unsafe fn copy_plane_dimensions(destination: *mut ImagePlane, source: *const ImagePlane) {
    unsafe {
        write_volatile(core::ptr::addr_of_mut!((*destination).flags), read_volatile(core::ptr::addr_of!((*source).flags)));
        write_volatile(core::ptr::addr_of_mut!((*destination).width), read_volatile(core::ptr::addr_of!((*source).width)));
        write_volatile(core::ptr::addr_of_mut!((*destination).height), read_volatile(core::ptr::addr_of!((*source).height)));
        write_volatile(core::ptr::addr_of_mut!((*destination).stride), read_volatile(core::ptr::addr_of!((*source).stride)));
        write_volatile(core::ptr::addr_of_mut!((*destination).length), read_volatile(core::ptr::addr_of!((*source).length)));
    }
}

unsafe fn clear_output_margins(stage: Address32, image: *mut ImagePlane) {
    let stride = unsafe { read_volatile(core::ptr::addr_of!((*image).stride)) };
    unsafe { write_volatile(core::ptr::addr_of_mut!((*image).width), stride) };
    let flags = unsafe { read_volatile(core::ptr::addr_of!((*image).flags)) };
    let margin = unsafe { stage.offset(ByteOffset(0x498)).word() };
    let image_width = unsafe { stage.offset(ByteOffset(0x424)).word() };
    let right = (stride.wrapping_mul(2) / if flags & 0x10 != 0 { 3 } else { 4 })
        .wrapping_sub(margin.wrapping_add(image_width));
    unsafe { xm1_pipeline_image_configure_borders(image, 0, 0, margin, right as i32) };
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_configure_output_policy(
    stage: *mut u8,
    output: *mut ImagePlane,
    layout: *const ImagePlane,
    buffer: u32,
    argument: i32,
) -> u32 {
    let frame = Address32(stage as u32);
    let first = unsafe { frame.offset(ByteOffset(0x4a0)).word() };
    let second = unsafe { frame.offset(ByteOffset(0x4a4)).word() };
    if unsafe { xm1_develop_capture_option_selection() } != 0 {
        return 1;
    }
    if stage.is_null() || output.is_null() || layout.is_null() || buffer == 0 {
        loop { core::hint::spin_loop(); }
    }
    let request = 0x0154_cc60 as *mut PlaneRequest;
    let image = 0x0154_d8c8 as *mut ImagePlane;
    unsafe {
        write_volatile(request, PlaneRequest::new(PlaneRequestKind::OutputStage));
        write_volatile(image, read_volatile(layout));
        copy_plane_dimensions(image, output);
        write_volatile(core::ptr::addr_of_mut!((*request).first), first as u8);
        write_volatile(core::ptr::addr_of_mut!((*request).second), second as u8);
        write_volatile(core::ptr::addr_of_mut!((*request).selection), argument);
        write_volatile(core::ptr::addr_of_mut!((*request).completion), buffer);
        write_volatile(core::ptr::addr_of_mut!((*request).enabled), 1);
    }
    let result = unsafe { xm1_pipeline_image_process_planes(stage, request.cast(), output, core::ptr::null(), core::ptr::null(), 0, image, 0) };
    unsafe {
        clear_output_margins(frame, image);
        write_volatile(output, read_volatile(image));
    }
    result
}

#[derive(Clone, Copy)]
enum OutputPlaneMode {
    Mask,
    Target,
}

impl OutputPlaneMode {
    fn parse(mode: u32) -> Self {
        match mode {
            0 => Self::Mask,
            2 => Self::Target,
            _ => loop { core::hint::spin_loop(); },
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_prepare_output_mask(
    stage: *mut u8,
    output: *mut ImagePlane,
    layout: *const ImagePlane,
    mask: *mut ImagePlane,
    buffer: u32,
    argument: i32,
) -> u32 {
    let frame = Address32(stage as u32);
    let first = unsafe { frame.offset(ByteOffset(0x4a0)).word() };
    let second = unsafe { frame.offset(ByteOffset(0x4a4)).word() };
    if unsafe { xm1_develop_capture_option_selection() } != 0 {
        return 1;
    }
    if stage.is_null() || output.is_null() || layout.is_null() || buffer == 0 {
        loop { core::hint::spin_loop(); }
    }
    let request = 0x0154_cef8 as *mut PlaneRequest;
    unsafe { write_volatile(request, PlaneRequest::new(PlaneRequestKind::Auxiliary)) };
    let margin = unsafe { frame.offset(ByteOffset(0x498)).word() } as i32;
    if margin > 0 {
        unsafe { xm1_pipeline_image_configure_borders(output, 0, 0, margin as u32, margin) };
    }
    let mode = OutputPlaneMode::parse(unsafe { frame.offset(ByteOffset(0x63c)).word() });
    let image = 0x0154_d87c as *mut ImagePlane;
    unsafe { write_volatile(image, read_volatile(layout)) };
    let (source, target) = match mode {
        OutputPlaneMode::Mask => {
            if !mask.is_null() {
                unsafe { copy_plane_dimensions(mask, output) };
            }
            unsafe { write_volatile(core::ptr::addr_of_mut!((*request).kind), PlaneRequestKind::OutputMask) };
            (image as *const ImagePlane, mask)
        }
        OutputPlaneMode::Target => {
            let width = unsafe { read_volatile(core::ptr::addr_of!((*output).width)) };
            let height = unsafe { read_volatile(core::ptr::addr_of!((*output).height)) };
            let stride = unsafe { read_volatile(core::ptr::addr_of!((*image).stride)) };
            unsafe {
                write_volatile(core::ptr::addr_of_mut!((*image).width), width);
                write_volatile(core::ptr::addr_of_mut!((*image).height), height);
                write_volatile(core::ptr::addr_of_mut!((*image).length), height.wrapping_mul(stride));
                write_volatile(core::ptr::addr_of_mut!((*request).kind), PlaneRequestKind::OutputTarget);
            }
            (core::ptr::null(), image)
        }
    };
    unsafe {
        write_volatile(core::ptr::addr_of_mut!((*request).first), first as u8);
        write_volatile(core::ptr::addr_of_mut!((*request).second), second as u8);
        write_volatile(core::ptr::addr_of_mut!((*request).selection), argument);
        write_volatile(core::ptr::addr_of_mut!((*request).completion), buffer);
        write_volatile(core::ptr::addr_of_mut!((*request).enabled), 1);
    }
    let result = unsafe { xm1_pipeline_image_process_planes(stage, request.cast(), output, core::ptr::null(), source, 0, target, 0) };
    if !mask.is_null() {
        unsafe { clear_output_margins(frame, mask) };
    }
    unsafe { write_volatile(output, read_volatile(image)) };
    result
}

unsafe extern "C" {
    fn image_planes_thread_event(event: u32, task: u32, status: i32);
    #[link_name = "xm1_buffers_configuration_byte"]
    fn image_planes_configuration_byte(key: u32) -> u32;
    #[link_name = "xm1_pipeline_image_trace_begin"]
    fn image_planes_trace_begin(operation: u32);
    #[link_name = "xm1_pipeline_image_trace_end"]
    fn image_planes_trace_end(operation: u32);
    fn xm1_develop_capture_option_selection() -> i32;
    fn xm1_pipeline_image_process_planes(stage: *mut u8, request: *const u32, source: *const ImagePlane, auxiliary: *const ImagePlane, plane: *const ImagePlane, option: u32, destination: *mut ImagePlane, flags: u32) -> u32;
    fn xm1_pipeline_image_write_debug_file(kind: i32, address: u32, size: u32, number: u32, suffix: *const u8);
    #[link_name = "xm1_develop_append_stream_text"]
    fn image_planes_task_log(task: u32, message: *const u8);
    fn xm1_pipeline_image_scale_packed_buffer(request: *const SoftImageRequest);
    fn xm1_pipeline_image_rotate_packed_buffer(request: *const SoftImageRequest);
    #[link_name = "xm1_develop_lookup_width"]
    fn image_planes_lookup_width(profile: u32, aspect: u32) -> u32;
    #[link_name = "xm1_develop_lookup_height"]
    fn image_planes_lookup_height(profile: u32, aspect: u32) -> u32;
    fn xm1_pipeline_image_resize_planes(stage: *mut u8, enabled: i32, mode: i32, source: *const ImagePlane, destination: *mut ImagePlane, auxiliary: *const ImagePlane, configuration: *mut u32, operation: i32) -> u32;
    fn xm1_pipeline_image_configure_borders(image: *const ImagePlane, top: u32, bottom: i32, left: u32, right: i32);
}
