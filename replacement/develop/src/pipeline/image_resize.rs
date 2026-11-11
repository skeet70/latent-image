use super::image_planes::{current_task, ImagePlane};
use super::shared::{Address32, ByteOffset};
use core::ptr::{read_volatile, write_volatile};

#[repr(C)]
pub(super) struct ResizeConfiguration {
    mode: u32,
    shift: u32,
    horizontal_scale: u32,
    vertical_scale: u32,
    source: u32,
    source_stride: u32,
    source_width: u32,
    source_height: u32,
    source_last_x: u32,
    source_last_y: u32,
    destination: u32,
    destination_stride: u32,
    destination_x: u32,
    destination_y: u32,
    destination_width: u32,
    destination_height: u32,
    destination_last_x: u32,
    destination_last_y: u32,
    correction: u32,
    completion: u32,
}

impl ResizeConfiguration {
    const fn empty() -> Self {
        Self {
            mode: 0, shift: 0, horizontal_scale: 0, vertical_scale: 0,
            source: 0, source_stride: 0, source_width: 0, source_height: 0,
            source_last_x: 0, source_last_y: 0, destination: 0, destination_stride: 0,
            destination_x: 0, destination_y: 0, destination_width: 0, destination_height: 0,
            destination_last_x: 0, destination_last_y: 0, correction: 0, completion: 0,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ResizeMode {
    Normal,
    Aspect,
    Square,
    CustomScale,
}

impl ResizeMode {
    fn parse(mode: i32) -> Self {
        match mode {
            1 => Self::Aspect,
            2 => Self::Square,
            3 => Self::CustomScale,
            _ => Self::Normal,
        }
    }
}

fn left(value: u32, shift: u32) -> u32 {
    let shift = shift & 255;
    if shift < 32 { value << shift } else { 0 }
}

fn right(value: u32, shift: u32) -> u32 {
    let shift = shift & 255;
    if shift < 32 { value >> shift } else { 0 }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_select_resize_mode(configuration: *mut ResizeConfiguration) {
    let vertical = (unsafe { read_volatile(core::ptr::addr_of!((*configuration).source_height)) }.wrapping_mul(10)
        / unsafe { read_volatile(core::ptr::addr_of!((*configuration).destination_height)) }) & 0xffff;
    let horizontal = (unsafe { read_volatile(core::ptr::addr_of!((*configuration).source_width)) }.wrapping_mul(10)
        / unsafe { read_volatile(core::ptr::addr_of!((*configuration).destination_width)) }) & 0xffff;
    let minimum = vertical.min(horizontal);
    let mut index = 0;
    while index != 5 {
        if minimum < unsafe { resize_configuration_byte(0xc0a5 + index) } {
            break;
        }
        index += 1;
    }
    let mut shift = unsafe { resize_configuration_byte(0xc0aa + index) };
    if vertical < 20 || horizontal < 20 {
        shift = 0;
    } else if (vertical <= 39 || horizontal <= 39) && shift == 2 {
        shift = 1;
    }
    unsafe { write_volatile(core::ptr::addr_of_mut!((*configuration).shift), shift) };
}

unsafe fn copy_distortion(stage: Address32, output: Address32) {
    let input = || unsafe { Address32(stage.offset(ByteOffset(0x1884)).word()) };
    unsafe {
        output.offset(ByteOffset(4)).write_word(u32::from(input().offset(ByteOffset(0xc0)).half()));
        output.offset(ByteOffset(8)).write_word(u32::from(input().offset(ByteOffset(0xc2)).half()));
        output.offset(ByteOffset(12)).write_half(input().offset(ByteOffset(0xc4)).half());
        output.offset(ByteOffset(14)).write_half(input().offset(ByteOffset(0xc6)).half());
        output.offset(ByteOffset(16)).write_half(u16::from(input().offset(ByteOffset(0xc8)).byte()));
        output.offset(ByteOffset(18)).write_half(u16::from(input().offset(ByteOffset(0xc9)).byte()));
        output.offset(ByteOffset(20)).write_word(u32::from(input().offset(ByteOffset(0xca)).half()));
        output.offset(ByteOffset(24)).write_word(u32::from(input().offset(ByteOffset(0xcc)).half()));
        output.write_word(u32::from(input().offset(ByteOffset(0xbe)).byte()));
    }
    for index in 0..31 {
        let value = unsafe { input().offset(ByteOffset(0xd0 + index * 4)).word() };
        unsafe { output.offset(ByteOffset(28 + index * 4)).write_word(value) };
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_expand_distortion(stage: *mut u8, output: *mut u32) {
    unsafe { copy_distortion(Address32(stage as u32), Address32(output as u32)) };
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_distortion_band(stage: *mut u8) -> u32 {
    let stage = Address32(stage as u32);
    let threshold = i32::from(unsafe { stage.offset(ByteOffset(0x6c0)).byte() }) - 1;
    let mut key = 0x3e3a00;
    let mut index = 0;
    loop {
        if threshold <= unsafe { resize_configuration_byte(key) } as i32 {
            return index;
        }
        index += 1;
        key = unsafe { Address32(0x00e5_859c).offset(ByteOffset(index * 4)).word() };
        if key == 0 {
            return index;
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_distortion_enabled(stage: *mut u8) -> u32 {
    let frame = Address32(stage as u32);
    if unsafe { frame.offset(ByteOffset(0x4b4)).word() } == 0x1d {
        return 0;
    }
    if unsafe { frame.offset(ByteOffset(0x78)).word() } == 0x0b {
        let header = Address32(unsafe { frame.offset(ByteOffset(0x1884)).word() });
        if unsafe { header.offset(ByteOffset(0xc0)).half() } == 0 {
            return 0;
        }
        return u32::from(unsafe { header.offset(ByteOffset(0xc2)).half() } != 0);
    }
    let band = unsafe { xm1_pipeline_image_distortion_band(stage) };
    if unsafe { frame.offset(ByteOffset(0x3240)).word() } == 1 {
        if unsafe { resize_configuration_byte(0xc0) } & 0x80 == 0
            || unsafe { frame.offset(ByteOffset(0x3244)).word() } == 0 {
            return 0;
        }
    } else {
        if unsafe { frame.offset(ByteOffset(0x62c)).word() } == 1
            || unsafe { resize_configuration_byte(0xc0) } & 0x80 == 0 {
            return 0;
        }
        let mask = unsafe { resize_configuration_byte(0x3e3a4c + (band >> 3)) };
        if mask >> (band & 7) & 1 == 0 {
            return 0;
        }
    }
    if unsafe { resize_configuration_byte(0x4011c1) } == 0 {
        return 0;
    }
    let selection = unsafe { frame.offset(ByteOffset(0x272c)).word() };
    if selection <= 1 { 1 - selection } else { 0 }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_resize_complete() {
    let task = unsafe { Address32(0x0157_bcf4).word() };
    if task != 0 {
        unsafe { xm1_pipeline_image_send_notification(task, 0x10) };
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_resize_planes(
    stage: *mut u8,
    enabled: i32,
    mode: i32,
    source: *const ImagePlane,
    destination: *const ImagePlane,
    auxiliary: *const ImagePlane,
    configuration: *mut ResizeConfiguration,
    operation: i32,
) -> u32 {
    let frame = Address32(stage as u32);
    let mode = ResizeMode::parse(mode);
    let mut events = 0;
    unsafe { resize_log(0x00e6_6670 as *const u8) };
    let owned_configuration = configuration.is_null();
    let configuration = if owned_configuration {
        unsafe { xm1_pipeline_image_warp_initialize() };
        0x0157_58cc as *mut ResizeConfiguration
    } else {
        configuration
    };
    let input = if auxiliary.is_null() { source } else { auxiliary };
    let source_address = unsafe { read_volatile(core::ptr::addr_of!((*input).address)) };
    let source_stride = unsafe { read_volatile(core::ptr::addr_of!((*input).stride)) };
    let source_width = unsafe { read_volatile(core::ptr::addr_of!((*input).width)) } >> 1;
    let source_height = unsafe { read_volatile(core::ptr::addr_of!((*input).height)) };
    let source_flags = unsafe { read_volatile(core::ptr::addr_of!((*input).flags)) };
    unsafe { write_volatile(configuration, ResizeConfiguration::empty()) };
    let output_width = unsafe { read_volatile(core::ptr::addr_of!((*destination).width)) } >> 1;
    let output_height = unsafe { read_volatile(core::ptr::addr_of!((*destination).height)) };
    unsafe {
        write_volatile(core::ptr::addr_of_mut!((*configuration).destination), read_volatile(core::ptr::addr_of!((*destination).address)));
        write_volatile(core::ptr::addr_of_mut!((*configuration).destination_stride), read_volatile(core::ptr::addr_of!((*destination).stride)) >> 1);
        write_volatile(core::ptr::addr_of_mut!((*configuration).destination_width), output_width);
        write_volatile(core::ptr::addr_of_mut!((*configuration).destination_height), output_height);
        write_volatile(core::ptr::addr_of_mut!((*configuration).destination_last_x), output_width.wrapping_sub(1));
        write_volatile(core::ptr::addr_of_mut!((*configuration).destination_last_y), output_height.wrapping_sub(1));
    }
    let source_metadata = unsafe { read_volatile(core::ptr::addr_of!((*source).flags)) };
    let (mut crop_width, mut crop_height) = if source_metadata & 0x0400_0000 == 0
        || unsafe { frame.offset(ByteOffset(0x78)).word() } == 0x0b {
        let width = unsafe { read_volatile(core::ptr::addr_of!((*source).width)) } >> 1;
        let width = if width == unsafe { frame.offset(ByteOffset(0x42c)).word() } {
            unsafe { frame.offset(ByteOffset(0x430)).word() }
        } else {
            width
        };
        (width, unsafe { read_volatile(core::ptr::addr_of!((*source).height)) })
    } else {
        let offset = if operation == 2 { ByteOffset(0x460) } else { ByteOffset(0x490) };
        let width = unsafe { frame.offset(offset).word() };
        let height = unsafe { frame.offset(offset).offset(ByteOffset(4)).word() };
        (width, height)
    };
    if mode == ResizeMode::Aspect && operation == 0 {
        crop_height = match unsafe { frame.offset(ByteOffset(0x4f0)).word() } {
            2 => crop_width.wrapping_mul(9) >> 4,
            _ => crop_width.wrapping_mul(2) / 3,
        };
        if source_flags & 0x2000 != 0 {
            crop_height >>= 1;
        }
    } else if mode == ResizeMode::Square {
        crop_width = output_width.wrapping_mul(crop_height) / output_height;
        if source_flags & 0x2000 != 0 {
            crop_width = crop_width.wrapping_mul(2);
        }
    }
    let mut padded_width = if source_width < crop_width { crop_width.wrapping_add(15) & !15 } else { source_width };
    let mut padded_height = source_height.max(crop_height);
    unsafe {
        write_volatile(core::ptr::addr_of_mut!((*configuration).source_width), crop_width);
        write_volatile(core::ptr::addr_of_mut!((*configuration).source_height), crop_height);
        xm1_pipeline_image_select_resize_mode(configuration);
    }
    let shift = unsafe { read_volatile(core::ptr::addr_of!((*configuration).shift)) };
    let quantum = left(1, shift);
    if mode != ResizeMode::CustomScale {
        padded_width &= quantum.wrapping_neg();
        padded_height &= quantum.wrapping_neg();
    }
    let mask = left(!1, shift);
    crop_width = if padded_width & quantum == 0 { crop_width & mask } else { (crop_width.wrapping_sub(quantum) & mask).wrapping_add(quantum) };
    crop_height = if padded_height & quantum == 0 { crop_height & mask } else { (crop_height.wrapping_sub(quantum) & mask).wrapping_add(quantum) };
    let address = source_address
        .wrapping_sub(source_stride.wrapping_mul(padded_height.wrapping_sub(source_height) >> 1))
        .wrapping_sub(padded_width.wrapping_sub(source_width) & !1);
    unsafe {
        write_volatile(core::ptr::addr_of_mut!((*configuration).source), address);
        write_volatile(core::ptr::addr_of_mut!((*configuration).source_stride), source_stride >> 1);
        write_volatile(core::ptr::addr_of_mut!((*configuration).source_width), crop_width);
        write_volatile(core::ptr::addr_of_mut!((*configuration).source_height), crop_height);
        xm1_pipeline_image_select_resize_mode(configuration);
        write_volatile(core::ptr::addr_of_mut!((*configuration).source_width), padded_width);
        write_volatile(core::ptr::addr_of_mut!((*configuration).source_height), padded_height);
    }
    let shift = unsafe { read_volatile(core::ptr::addr_of!((*configuration).shift)) };
    unsafe {
        write_volatile(core::ptr::addr_of_mut!((*configuration).source_last_x), left(right(padded_width, shift).wrapping_sub(1), shift));
        write_volatile(core::ptr::addr_of_mut!((*configuration).source_last_y), left(right(padded_height, shift).wrapping_sub(1), shift));
    }
    let (horizontal_scale, vertical_scale) = if mode == ResizeMode::CustomScale {
        let aspect = unsafe { frame.offset(ByteOffset(0x4f0)).word() };
        let first = unsafe { frame.offset(if aspect == 3 { ByteOffset(0x43c) } else { ByteOffset(0x438) }).word() };
        let second = unsafe { frame.offset(ByteOffset(0x43c)).word() };
        let horizontal = unsafe { frame.offset(ByteOffset(0x490)).word() }.wrapping_mul(crop_width).wrapping_mul(0xa000)
            / output_width.wrapping_mul(first);
        let vertical = unsafe { frame.offset(ByteOffset(0x494)).word() }.wrapping_mul(crop_height).wrapping_mul(0xa000)
            / output_height.wrapping_mul(second);
        (horizontal.wrapping_add(5) / 10, vertical.wrapping_add(5) / 10)
    } else {
        (right(crop_width, shift).wrapping_sub(1).wrapping_mul(0x1000) / output_width.wrapping_sub(1),
         right(crop_height, shift).wrapping_sub(1).wrapping_mul(0x1000) / output_height.wrapping_sub(1))
    };
    unsafe {
        write_volatile(core::ptr::addr_of_mut!((*configuration).horizontal_scale), horizontal_scale);
        write_volatile(core::ptr::addr_of_mut!((*configuration).vertical_scale), vertical_scale);
    }
    let correction = Address32(0x0157_ba04);
    let special = unsafe { frame.offset(ByteOffset(0x78)).word() } == 0x0b;
    if special {
        unsafe { copy_distortion(frame, correction) };
        let present = unsafe { correction.offset(ByteOffset(4)).word() } != 0
            || unsafe { correction.offset(ByteOffset(8)).word() } != 0;
        unsafe { write_volatile(core::ptr::addr_of_mut!((*configuration).correction), if present && enabled != 0 { correction.0 } else { 0 }) };
    } else if enabled != 0 && unsafe { xm1_pipeline_image_distortion_enabled(stage) } != 0 {
        unsafe {
            xm1_pipeline_image_build_distortion(stage, configuration, correction.0 as *mut u32);
            write_volatile(core::ptr::addr_of_mut!((*configuration).correction), correction.0);
        }
    }
    unsafe { write_volatile(core::ptr::addr_of_mut!((*configuration).destination_height), output_height.wrapping_add(7) & !7) };
    let result = if owned_configuration {
        unsafe {
            write_volatile(core::ptr::addr_of_mut!((*configuration).mode), 0);
            write_volatile(core::ptr::addr_of_mut!((*configuration).completion), xm1_pipeline_image_resize_complete as *const () as u32);
            xm1_pipeline_image_warp_configure(configuration);
            xm1_pipeline_image_snapshot_warp(stage);
            resize_trace_begin(0x0602_3330);
            resize_log(0x00e6_667c as *const u8);
            Address32(0x0157_bcf4).write_word(current_task().0);
            xm1_pipeline_image_warp_start();
            xm1_pipeline_image_wait_all(0x10, &mut events, -1);
            Address32(0x0157_bcf4).write_word(0);
        }
        let completed = u32::from(events & 0x20 == 0);
        unsafe {
            xm1_pipeline_image_warp_cancel();
            resize_log(0x00e6_6684 as *const u8);
            resize_trace_end(0x0602_3330);
            xm1_pipeline_image_warp_release();
        }
        completed
    } else {
        unsafe { write_volatile(core::ptr::addr_of_mut!((*configuration).mode), 1) };
        if unsafe { resize_configuration_byte(0xa8) } & 0x80 != 0
            && unsafe { frame.offset(ByteOffset(0x220c)).word() } != 0 {
            unsafe {
                xm1_pipeline_image_warp_initialize();
                xm1_pipeline_image_warp_configure(configuration);
                xm1_pipeline_image_snapshot_warp(stage);
                xm1_pipeline_image_warp_cancel();
                xm1_pipeline_image_warp_release();
            }
        }
        1
    };
    unsafe { resize_log(0x00e6_668c as *const u8) };
    result
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_warp_read_registers(output: *mut u32) {
    let registers = Address32(0xfff6_5000);
    unsafe {
        write_volatile(output, registers.word());
        write_volatile(output.add(1), registers.offset(ByteOffset(4)).word());
    }
    for index in 0..19 {
        let value = unsafe { registers.offset(ByteOffset(0x10 + index * 4)).word() };
        unsafe { write_volatile(output.add(index as usize + 2), value) };
    }
    for index in 0..31 {
        let value = unsafe { registers.offset(ByteOffset(0x60 + index * 4)).word() };
        unsafe { write_volatile(output.add(index as usize + 21), value) };
    }
    for index in 0..3 {
        let value = unsafe { registers.offset(ByteOffset(0x140 + index * 4)).word() };
        unsafe { write_volatile(output.add(index as usize + 52), value) };
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_snapshot_warp(stage: *mut u8) {
    if unsafe { resize_configuration_byte(0xa8) } & 0x80 == 0 {
        return;
    }
    let slot = Address32(stage as u32).offset(ByteOffset(0x220c));
    let output = Address32(unsafe { slot.word() });
    if output.0 == 0 {
        return;
    }
    unsafe { output.offset(ByteOffset(0xc315c)).write_word(0x5354_5243) };
    let output = Address32(unsafe { slot.word() });
    unsafe { output.offset(ByteOffset(0xd7e50)).write_word(0x4c41_5243) };
    unsafe { xm1_pipeline_image_warp_read_registers(0x0157_591c as *mut u32) };
    let output = Address32(unsafe { slot.word() });
    let index = unsafe { output.offset(ByteOffset(0xd7e4c)).word() };
    if index > 9 {
        return;
    }
    let destination = output.offset(ByteOffset(0xd75b4 + index * 0xdc));
    for word in 0..55 {
        let value = unsafe { Address32(0x0157_591c).offset(ByteOffset(word * 4)).word() };
        unsafe { destination.offset(ByteOffset(word * 4)).write_word(value) };
    }
    let output = Address32(unsafe { slot.word() });
    unsafe { output.offset(ByteOffset(0xd7e4c)).write_word(index + 1) };
}

unsafe extern "C" {
    #[link_name = "xm1_buffers_configuration_byte"]
    fn resize_configuration_byte(key: u32) -> u32;
    fn resize_log(message: *const u8);
    #[link_name = "xm1_pipeline_image_trace_begin"]
    fn resize_trace_begin(operation: u32);
    #[link_name = "xm1_pipeline_image_trace_end"]
    fn resize_trace_end(operation: u32);
    fn xm1_pipeline_image_warp_initialize();
    fn xm1_pipeline_image_warp_configure(configuration: *const ResizeConfiguration);
    fn xm1_pipeline_image_warp_start();
    fn xm1_pipeline_image_warp_cancel();
    fn xm1_pipeline_image_warp_release();
    fn xm1_pipeline_image_build_distortion(stage: *mut u8, configuration: *const ResizeConfiguration, output: *mut u32);
    fn xm1_pipeline_image_wait_all(mask: u32, output: *mut u32, timeout: i32) -> i32;
    fn xm1_pipeline_image_send_notification(task: u32, value: u32) -> i32;
}
