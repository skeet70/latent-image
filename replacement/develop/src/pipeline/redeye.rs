use super::image_planes::{ImagePlane, xm1_pipeline_image_current_task, xm1_pipeline_image_wait_all, xm1_pipeline_image_send_notification};
use super::shared::{Address32, ByteOffset};
use core::ptr::{read_volatile, write_volatile};

#[derive(Clone, Copy)]
#[repr(transparent)]
struct Frame(Address32);
impl Frame {
    unsafe fn word(self, offset: u32) -> u32 { unsafe { self.0.offset(ByteOffset(offset)).word() } }
    unsafe fn store(self, offset: u32, value: u32) { unsafe { self.0.offset(ByteOffset(offset)).write_word(value) }; }
}

#[derive(Clone, Copy)]
#[repr(C)]
pub struct RedEyeRectangle {
    left: u32,
    top: u32,
    width: u32,
    height: u32,
    code: u16,
    reserved: u16,
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_unpack_rectangle(plane: *const ImagePlane, rectangle: *mut u32) {
    let plane = unsafe { read_volatile(plane) };
    for (index, value) in [plane.address, plane.width >> 1, plane.height, plane.stride.wrapping_sub(plane.width) >> 1].into_iter().enumerate() {
        unsafe { write_volatile(rectangle.add(index), value) };
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_rectangle_to_bounds(rectangle: *const RedEyeRectangle, bounds: *mut u32) {
    let rectangle = unsafe { read_volatile(rectangle) };
    let values = [rectangle.left, rectangle.left.wrapping_add(rectangle.width).wrapping_sub(1), rectangle.top, rectangle.top.wrapping_add(rectangle.height).wrapping_sub(1), 0, 1, 1];
    for (index, value) in values.into_iter().enumerate() { unsafe { write_volatile(bounds.add(index), value) }; }
    unsafe { write_volatile(bounds.add(7).cast::<u16>(), rectangle.code) };
}

unsafe fn copy_bytes(destination: Address32, source: Address32, length: u32) {
    for offset in 0..length {
        unsafe { destination.offset(ByteOffset(offset)).write_byte(source.offset(ByteOffset(offset)).byte()) };
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_complete_primary_copy(status: i32, values: *const u32, source: *const u8, _flags: u32) {
    unsafe { copy_bytes(Address32(0x0154_cc58), Address32(values as u32), 8) };
    unsafe { copy_bytes(Address32(0x0154_c780), Address32(source as u32), 0x464) };
    let task = unsafe { Address32(0x0155_ccb0).word() };
    if task != 0 { unsafe { xm1_pipeline_image_send_notification(task, if status == 1 { 0x10 } else { 0x20 }) }; }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_complete_secondary_copy(status: i32, source: *const u8, values: *const u32) {
    unsafe { copy_bytes(Address32(0x0154_cf44), Address32(source as u32), 0x1bc) };
    unsafe { copy_bytes(Address32(0x0154_c748), Address32(values as u32), 12) };
    let task = unsafe { Address32(0x0155_ccac).word() };
    if task != 0 { unsafe { xm1_pipeline_image_send_notification(task, if status == 1 { 0x10 } else { 0x20 }) }; }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_finish_copy(frame: *mut u8) -> u32 {
    let frame = Frame(Address32(frame as u32));
    unsafe { Address32(0x0155_ccb0).write_word(xm1_pipeline_image_current_task()) };
    unsafe { xm1_pipeline_image_submit_redeye_value(xm1_pipeline_image_complete_primary_copy as *const () as u32) };
    let mut events = core::mem::MaybeUninit::<u32>::uninit();
    unsafe { xm1_pipeline_image_wait_all(0x30, events.as_mut_ptr(), -1) };
    unsafe { Address32(0x0155_ccb0).write_word(0) };
    if unsafe { events.assume_init() } & 0x20 != 0 { return 0; }
    unsafe { frame.store(0x750, 1) };
    1
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_start_work(frame: *mut u8, source: *const ImagePlane, target: *const ImagePlane, _third: u32, _fourth: u32, _fifth: u32, _sixth: u32, command: *const u32) -> u32 {
    let frame = Frame(Address32(frame as u32));
    let aspect = unsafe { frame.word(0x4f0) };
    let (horizontal, vertical) = if aspect == 0 { (0, 0) } else {
        let profile = unsafe { frame.word(0x644) };
        let width = unsafe { xm1_develop_lookup_width(profile, aspect) };
        let height = unsafe { xm1_develop_lookup_height(profile, aspect) };
        if aspect == 3 { (((height.wrapping_shl(2) / 3).wrapping_sub(width) >> 1).wrapping_add(1) & !1, 0) }
        else { (0, ((width.wrapping_mul(3) >> 2).wrapping_sub(height)) >> 1) }
    };
    let source = unsafe { read_volatile(source) };
    let target = unsafe { read_volatile(target) };
    let source_width = horizontal.wrapping_mul(2).wrapping_add(source.width >> 1);
    let descriptors = [
        (Address32(0x0154_cd18), [source.address.wrapping_sub(horizontal.wrapping_mul(2)).wrapping_sub(source.stride.wrapping_mul(vertical)), source_width, source.height.wrapping_add(vertical.wrapping_mul(2)), (source.stride >> 1).wrapping_sub(source_width)]),
        (Address32(0x0154_c754), [target.address, target.width >> 1, target.height, target.stride.wrapping_sub(target.width) >> 1]),
    ];
    for (address, words) in descriptors { for (index, word) in words.into_iter().enumerate() { unsafe { address.offset(ByteOffset(index as u32 * 4)).write_word(word) }; } }
    unsafe { Address32(0x0155_ccac).write_word(xm1_pipeline_image_current_task()) };
    unsafe { xm1_pipeline_image_submit_redeye_request(0x0154_cd18 as *const u32, 0x0154_c3a4 as *const u8, 0x0154_c754 as *const u32, read_volatile(command.add(1)), xm1_pipeline_image_complete_secondary_copy as *const () as u32) };
    let mut events = core::mem::MaybeUninit::<u32>::uninit();
    unsafe { xm1_pipeline_image_wait_all(0x30, events.as_mut_ptr(), -1) };
    unsafe { Address32(0x0155_ccac).write_word(0) };
    if unsafe { events.assume_init() } & 0x20 != 0 { return 0; }
    unsafe { frame.store(0x2758, 1); frame.store(0x275c, 0); }
    let count = unsafe { Address32(0x0154_cf44).byte() };
    for index in 0..u32::from(count) {
        let result = Address32(0x0154_cf44 + index * 0x2c);
        let sides = unsafe { result.offset(ByteOffset(4)).word() };
        if sides == 0 { continue; }
        unsafe { frame.store(0x275c, 1) };
        for (selected, rectangle_offset, frame_offset) in [(sides == 1 || sides == 3, 8, 0x784), (sides == 2 || sides == 3, 0x1c, 0x7a4)] {
            if !selected { continue; }
            let output = frame.0.offset(ByteOffset(index * 0xb8 + frame_offset));
            unsafe { xm1_pipeline_image_rectangle_to_bounds(result.offset(ByteOffset(rectangle_offset)).0 as *const RedEyeRectangle, output.0 as *mut u32) };
            if aspect != 0 {
                for (offset, subtraction) in [(0, horizontal), (4, horizontal), (8, vertical), (12, vertical)] {
                    let field = output.offset(ByteOffset(offset));
                    unsafe { field.write_word(field.word().wrapping_sub(subtraction)) };
                }
            }
        }
    }
    1
}
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_prepare_geometry(frame: *mut u8, source: *const ImagePlane, cropped: *mut ImagePlane, first: *const ImagePlane, second: *const ImagePlane, third: *const ImagePlane, fourth: *const ImagePlane) -> u32 {
    let frame = Frame(Address32(frame as u32));
    unsafe { redeye_append_text(xm1_pipeline_image_current_task() as u8, 0x00e5_439c as *const u8) };
    let outputs = [first, second, third, fourth];
    let mut cursors = outputs.map(|plane| unsafe { read_volatile(plane).address });
    for offset in 0..0x39c { unsafe { Address32(0x0154_c3a4 + offset).write_byte(0) }; }
    let mut ticks = [0; 2];
    unsafe { xm1_pipeline_image_get_ticks(ticks.as_mut_ptr()); redeye_trace_begin(0x0602_0700); }
    let mut input = unsafe { read_volatile(cropped) };
    input.address = input.address.wrapping_add(input.stride.wrapping_sub(0x800) >> 1).wrapping_add(input.stride.wrapping_mul(input.height.wrapping_sub(0x300) >> 1));
    input.width = 0x800;
    input.height = 0x300;
    input.length = 0x180000;
    unsafe { write_volatile(cropped, input) };
    let mut resized = input;
    unsafe { xm1_develop_geometry(source, &mut resized, core::ptr::null_mut(), frame.word(0x644), frame.word(0x4f0), frame.word(0x4b4), 0, 0) };
    let mut status = unsafe { xm1_pipeline_image_resize_through_crop(frame.0.0 as *mut u8, source, &mut resized, core::ptr::null_mut(), 0) };
    if unsafe { frame.word((xm1_pipeline_image_current_task() + 5) * 4) } == 0 || status == 0 { return 0; }
    let aspect = unsafe { frame.word(0x4f0) };
    let profile = unsafe { frame.word(0x644) };
    let (extent, scale) = if aspect == 3 { (input.height, unsafe { xm1_develop_lookup_height(profile, aspect) }) } else { (input.width >> 1, unsafe { xm1_develop_lookup_width(profile, aspect) }) };
    unsafe { copy_bytes(Address32(0x0154_d120), frame.0.offset(ByteOffset(0x74c)), 0x740) };
    unsafe { xm1_pipeline_image_normalize_faces(frame.0.0 as *mut u8, 0x0154_d120 as *mut u8) };
    let count = unsafe { Address32(0x0154_d120).byte() };
    unsafe { Address32(0x0154_c3a4).write_byte(count) };
    for index in 0..u32::from(count) {
        let original = Frame(Address32(0x0154_d120 + index * 0xb8));
        let prepared = Frame(Address32(0x0154_c3a4 + index * 0x5c));
        let left = unsafe { original.word(0x0c) };
        let mut x = left.wrapping_mul(extent) / scale;
        let mut y = unsafe { original.word(0x14) }.wrapping_mul(extent) / scale;
        let mut size = unsafe { original.word(0x10) }.wrapping_sub(left).wrapping_add(1).wrapping_mul(extent) / scale;
        match (x & 1, size & 1) {
            (1, 0) => { x = x.wrapping_add(1); y = y.wrapping_add(1); size = size.wrapping_sub(2); }
            (0, 1) => size = size.wrapping_sub(1),
            (1, 1) => { x = x.wrapping_add(1); y = y.wrapping_add(1); size = size.wrapping_sub(1); }
            _ => {}
        }
        unsafe {
            prepared.store(4, x); prepared.store(8, y); prepared.store(12, size); prepared.store(16, size);
            prepared.0.offset(ByteOffset(20)).write_half(original.0.offset(ByteOffset(40)).half());
            let kind = original.word(28);
            let orientation = original.word(32);
            prepared.store(24, if kind <= 2 { kind } else { 0 });
            prepared.store(28, if orientation <= 11 { orientation } else { 0 });
        }
    }
    for index in 0..u32::from(count).min(10) {
        let prepared = Frame(Address32(0x0154_c3a4 + index * 0x5c));
        match aspect {
            1 => unsafe { prepared.store(8, prepared.word(8).wrapping_add(input.height.wrapping_sub((input.width & !1) / 3) >> 1)) },
            2 => unsafe { prepared.store(8, prepared.word(8).wrapping_add(input.height.wrapping_sub((input.width >> 1).wrapping_mul(9) >> 4) >> 1)) },
            3 => unsafe { prepared.store(4, prepared.word(4).wrapping_add((input.width >> 1).wrapping_sub(input.height) >> 1)) },
            _ => {}
        }
        let mut sample = input;
        sample.address = input.address.wrapping_add(unsafe { prepared.word(8) }.wrapping_mul(input.stride)).wrapping_add(unsafe { prepared.word(4) }.wrapping_mul(2));
        sample.width = unsafe { prepared.word(12) }.wrapping_mul(2);
        sample.height = unsafe { prepared.word(16) };
        sample.length = input.stride.wrapping_mul(sample.height);
        let mut canonical = unsafe { read_volatile(first) };
        for output_index in 0..4 {
            let mut destination = unsafe { read_volatile(outputs[output_index]) };
            destination.address = cursors[output_index];
            cursors[output_index] = cursors[output_index].wrapping_add(destination.height.wrapping_mul(destination.stride));
            let source = if output_index == 0 {
                destination.address = destination.address.wrapping_add(destination.stride.wrapping_mul(8)).wrapping_add(16);
                destination.width = 0x180;
                destination.height = 0xc0;
                destination.length = destination.stride.wrapping_mul(0xc0);
                canonical = destination;
                &sample
            } else { &canonical };
            unsafe { xm1_pipeline_image_unpack_rectangle(&destination, prepared.0.offset(ByteOffset(32 + output_index as u32 * 16)).0 as *mut u32) };
            status = unsafe { xm1_pipeline_image_resize_software(source, &mut destination) } as u32;
            if unsafe { frame.word((xm1_pipeline_image_current_task() + 5) * 4) } == 0 || status == 0 { return 0; }
        }
    }
    unsafe { redeye_trace_end(0x0602_0700); Address32(0x0154_c740).write_word(xm1_pipeline_image_elapsed(ticks[0])); }
    status
}


unsafe extern "C" {
    fn xm1_develop_lookup_width(profile: u32, aspect: u32) -> u32;
    fn xm1_develop_lookup_height(profile: u32, aspect: u32) -> u32;
    fn xm1_develop_geometry(source: *const ImagePlane, plane: *mut ImagePlane, borders: *mut u32, profile: u32, aspect: u32, capture: u32, mode: u32, anchor: u32);
    fn xm1_pipeline_image_resize_through_crop(frame: *mut u8, source: *const ImagePlane, destination: *mut ImagePlane, auxiliary: *mut ImagePlane, enabled: i32) -> u32;
    fn xm1_pipeline_image_resize_software(source: *const ImagePlane, destination: *mut ImagePlane) -> i32;
    #[link_name = "xm1_develop_append_stream_text"]
    fn redeye_append_text(task: u8, text: *const u8);
    #[link_name = "xm1_pipeline_image_trace_begin"]
    fn redeye_trace_begin(operation: u32);
    #[link_name = "xm1_pipeline_image_trace_end"]
    fn redeye_trace_end(operation: u32);
    #[link_name = "xm1_buffers_configuration_byte"]
    fn redeye_configuration_byte(key: u32) -> u32;
    #[link_name = "xm1_develop_fit_aspect"]
    fn redeye_aspect_dimensions(width: u32, height: u32, aspect: u32, output: *mut u32);
}

fn aspect_scale(value: u32, aspect: u32, half: bool) -> u32 {
    let value = value as i32;
    match (aspect, half) {
        (3, false) => value as u32,
        (3, true) => (value / 2) as u32,
        (2, false) => (value.wrapping_mul(9) / 16) as u32,
        (2, true) => (value.wrapping_mul(9) / 32) as u32,
        (_, false) => (value.wrapping_mul(2) / 3) as u32,
        (_, true) => (value.wrapping_mul(2) / 6) as u32,
    }
}

unsafe fn face_delta(frame: Frame, aspect: u32) -> u32 {
    let first = unsafe { frame.0.offset(ByteOffset(0x6c2)).half() } as u32;
    let second = unsafe { frame.0.offset(ByteOffset(0x6c4)).half() } as u32;
    let step = unsafe { redeye_configuration_byte(0x254) };
    let step = if aspect == 3 { (step as i32).wrapping_mul(2) / 3 } else { step as i32 } as u32;
    step.wrapping_mul(second.wrapping_sub(first)).wrapping_add(unsafe { frame.word(0x460) }.wrapping_sub(unsafe { frame.word(0x458) }))
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_normalize_faces(frame: *mut u8, faces: *mut u8) {
    let frame = Frame(Address32(frame as u32));
    let faces = Frame(Address32(faces as u32));
    let aspect = unsafe { frame.word(0x4f0) };
    let profile = unsafe { frame.word(0x644) };
    let mut scale_x = unsafe { xm1_develop_lookup_width(profile, aspect) };
    let mut scale_y = unsafe { xm1_develop_lookup_height(profile, aspect) };
    let special_capture = unsafe { frame.word(0x78) } == 0xb;
    let selected = unsafe { frame.word(0x27ac) } == 1 && unsafe { frame.word(0x600) } == 1;
    let (denominator_x, denominator_y, bias_x, bias_y, offset_x, offset_y);
    if special_capture {
        denominator_x = unsafe { frame.word(0x490) };
        denominator_y = unsafe { frame.word(0x494) };
        bias_x = denominator_x.wrapping_sub(unsafe { frame.word(0x460) }).wrapping_sub(unsafe { face_delta(frame, aspect) }) >> 1;
        bias_y = denominator_y.wrapping_sub(unsafe { frame.word(0x464) }).wrapping_sub(aspect_scale(unsafe { face_delta(frame, aspect) }, aspect, false)) >> 1;
        if unsafe { frame.word(0x70c) } == 1 {
            offset_x = (unsafe { face_delta(frame, aspect) } as i32 / 2) as u32;
            offset_y = aspect_scale(unsafe { face_delta(frame, aspect) }, aspect, true);
        } else { offset_x = 0; offset_y = 0; }
    } else if selected {
        let mut dimensions = [0; 6];
        unsafe { redeye_aspect_dimensions(720, 480, aspect, dimensions.as_mut_ptr()) };
        denominator_x = dimensions[0]; denominator_y = dimensions[1];
        bias_x = dimensions[4]; bias_y = dimensions[2];
        offset_x = 0; offset_y = 0;
        scale_x = unsafe { frame.word(0x1770) };
        scale_y = unsafe { frame.word(0x176c) };
    } else {
        denominator_x = unsafe { frame.word(0x460) }.wrapping_sub(unsafe { face_delta(frame, aspect) });
        denominator_y = unsafe { frame.word(0x464) }.wrapping_sub(aspect_scale(unsafe { face_delta(frame, aspect) }, aspect, false));
        bias_x = 0; bias_y = 0;
        if unsafe { frame.word(0x70c) } == 1 {
            offset_x = (unsafe { face_delta(frame, aspect) } as i32 / 2) as u32;
            offset_y = aspect_scale(unsafe { face_delta(frame, aspect) }, aspect, true);
        } else { offset_x = 0; offset_y = 0; }
    }
    for index in 0..unsafe { faces.word(0) } {
        let face = Frame(faces.0.offset(ByteOffset(index.wrapping_mul(0xb8))));
        for (field, bias, offset, scale, denominator) in [(12, bias_x, offset_x, scale_x, denominator_x), (16, bias_x, offset_x, scale_x, denominator_x), (20, bias_y, offset_y, scale_y, denominator_y), (24, bias_y, offset_y, scale_y, denominator_y)] {
            let mut value = unsafe { face.word(field) };
            if special_capture { value = value.wrapping_add(bias); }
            if selected { value = value.wrapping_sub(bias); }
            unsafe { face.store(field, scale.wrapping_mul(value.wrapping_add(offset)) / denominator) };
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_get_ticks(output: *mut u32) {
    let interface = unsafe { super::image_planes::current_interface() };
    let table = unsafe { super::image_planes::interface(interface) };
    let clock = Address32(unsafe { table.offset(ByteOffset(0x174)).word() });
    unsafe { write_volatile(output, clock.word()) };
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_elapsed(start: u32) -> u32 {
    let mut now = 0;
    unsafe { xm1_pipeline_image_get_ticks(&mut now) };
    now.wrapping_sub(start).wrapping_mul(10)
}

#[repr(C)]
pub struct RedEyeMessage {
    event: u16,
    sender: u8,
    destination: u8,
    size: u32,
    payload: *const u32,
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_submit_redeye_request(first: *const u32, faces: *const u8, third: *const u32, command: u32, callback: u32) -> i32 {
    unsafe { Address32(0x014e_212c).write_byte(0) };
    unsafe {
        copy_bytes(Address32(0x014d_fa90), Address32(first as u32), 16);
        copy_bytes(Address32(0x014d_faa0), Address32(faces as u32), 0x39c);
        copy_bytes(Address32(0x014d_fe3c), Address32(third as u32), 16);
    }
    let payload = [command, callback];
    let message = RedEyeMessage { event: 0x4800, sender: unsafe { xm1_pipeline_image_current_task() } as u8, destination: 0x38, size: 8, payload: payload.as_ptr() };
    if unsafe { redeye_send_message(0x38, &message) } == 0 { 0 } else { -255 }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_submit_redeye_value(callback: u32) -> i32 {
    unsafe { Address32(0x014e_212c).write_byte(0) };
    let message = RedEyeMessage { event: 0x4801, sender: unsafe { xm1_pipeline_image_current_task() } as u8, destination: 0x38, size: 4, payload: &callback };
    if unsafe { redeye_send_message(0x38, &message) } == 0 { 0 } else { -255 }
}

unsafe extern "C" {
    #[link_name = "xm1_pipeline_image_send_message"]
    fn redeye_send_message(queue: u32, message: *const RedEyeMessage) -> i32;
}
