use super::shared::{Address32, ByteOffset};
use core::ptr::{read_volatile, write_volatile};

#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct OutputJob(pub u32);

#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct ImageWork(pub u32);

#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct OutputRequest(pub u32);

#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct ScatterList(pub u32);

#[derive(Clone, Copy)]
#[repr(C)]
pub struct OutputPlane {
    pub format: u32,
    pub address: u32,
    pub length: u32,
    pub stride: u32,
    pub width: u32,
    pub height: u32,
    pub flags: u32,
}

#[derive(Clone, Copy)]
#[repr(u32)]
enum OutputBank { Primary = 0, Preview = 1, Raw = 2 }

#[derive(Clone, Copy)]
#[repr(C)]
struct PlaneRequest {
    kind: u32,
    first: u8,
    second: u8,
    reserved: [u8; 2],
    paired: u32,
    reserved_words: [u32; 2],
    argument: u32,
    buffer: u32,
    enabled: u32,
}

unsafe fn word(base: u32, offset: u32) -> u32 {
    unsafe { Address32(base).offset(ByteOffset(offset)).word() }
}
unsafe fn store(base: u32, offset: u32, value: u32) {
    unsafe { Address32(base).offset(ByteOffset(offset)).write_word(value) }
}
unsafe fn image(request: OutputRequest) -> ImageWork {
    let address = unsafe { word(request.0, 8) };
    ImageWork(if address == 0 { 0x154d974 } else { address })
}
unsafe fn bank(request: OutputRequest) -> OutputBank {
    match unsafe { word(request.0, 4) } {
        1 => OutputBank::Preview,
        2..=4 => OutputBank::Raw,
        _ => OutputBank::Primary,
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_prepare_output_stage(job: OutputJob, request: OutputRequest) -> u32 {
    unsafe {
        if xm1_develop_capture_option_selection() != 0 { return 1; }
        let image = image(request);
        let bank = bank(request);
        let list = ScatterList(job.0.wrapping_add(0x27d0 + bank as u32 * 0x150));
        if word(request.0, 4) == 4 {
            let mut first = [0u32; 2];
            let mut second = [0u32; 2];
            first[0] = word(word(request.0, 12), 4);
            first[1] = word(word(request.0, 12), 8);
            second[0] = word(word(request.0, 16), 4);
            second[1] = word(word(request.0, 16), 8);
            let destinations = [first.as_mut_ptr(), second.as_mut_ptr()];
            let images = [ImageWork(word(job.0, 0x3234)), ImageWork(word(job.0, 0x3238))];
            output_jpeg_mpf_scatter(list, destinations.as_ptr(), images.as_ptr(), 2);
        } else if matches!(bank, OutputBank::Raw) {
            xm1_pipeline_image_build_packet_list(list, Address32(0x155ccc4));
        } else {
            xm1_pipeline_image_build_scatter_list(list, image);
        }
        if output_jpeg_configuration_byte(0xa8) & 0x80 != 0 {
            let buffer = word(job.0, 0x220c);
            if buffer != 0 { xm1_pipeline_image_append_output_buffer(list, Address32(buffer), ByteOffset(0xed800)); }
        }
        1
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_configure_output(_job: OutputJob, request: OutputRequest) -> u32 {
    unsafe {
        if xm1_develop_capture_option_selection() == 0 {
            store(0x155ccc4, 0xb608, image(request).0);
            if word(request.0, 4) == 2 { output_jpeg_build_raf(Address32(0x155ccc4)); }
        }
        1
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_configure_output_geometry(job: OutputJob, request: OutputRequest, template: Address32, primary: Address32, secondary: Address32) -> u32 {
    unsafe {
        if xm1_develop_capture_option_selection() != 0 { return 1; }
        let image = image(request);
        output_jpeg_initialize_identity();
        xm1_pipeline_image_initialize_raw_output(image, Address32(word(template.0, 4)), (word(request.0, 4) == 2) as u32);
        store(image.0, 0xf2e8, 0);
        let value = word(job.0, 0x167c);
        store(image.0, 0xf2ec, value);
        output_jpeg_initialize_metadata(Address32(image.0 + 0x42c), Address32(image.0 + 0x5d8), Address32(image.0 + 0x91c), Address32(image.0 + 0xf218), Address32(image.0 + 0xf2b8), 0, value);
        let kind = word(request.0, 4);
        store(job.0, 0x1678, (kind.wrapping_sub(2) < 2) as u32);
        output_jpeg_populate_metadata(job, Address32(image.0 + 0x42c), Address32(image.0 + 0xf2b8), Address32(image.0 + 0x5d8), Address32(image.0 + 0x91c), Address32(image.0 + 0xf218), (kind == 0 && word(job.0, 0x275c) == 1) as u32);
        if word(job.0, 0x78) == 11 { output_jpeg_special_metadata(job, Address32(image.0 + 0x91c)); }
        output_jpeg_capture_metadata(job, Address32(image.0 + 0x42c));
        if word(request.0, 4) == 2 {
            output_jpeg_prepare_raf(job, primary, secondary, image, Address32(0x155ccc4));
            output_jpeg_begin_raf(Address32(0x155ccc4));
        }
        output_jpeg_prepare_image(image);
        1
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_prepare_output_planes(job: OutputJob, source: *mut OutputPlane, second: *mut OutputPlane, mask: *mut OutputPlane, output: *mut OutputPlane, secondary: *mut OutputPlane, buffer: Address32, flag: u32, _unused_8: u32, _unused_9: u32, mode: u32) -> u32 {
    unsafe {
        let selection = xm1_develop_capture_option_selection();
        let first = word(job.0, 0x4a0) as u8;
        let second_value = word(job.0, 0x4a4) as u8;
        if selection == 2 { write_volatile(output, read_volatile(source)); return 1; }
        let request = 0x154ce88 as *mut PlaneRequest;
        write_volatile(request, PlaneRequest { kind: 0, first: 0, second: 0, reserved: [0; 2], paired: 0, reserved_words: [0; 2], argument: 0, buffer: 0, enabled: 0 });
        let margin = word(job.0, 0x498);
        if (margin as i32) > 0 {
            if !source.is_null() { xm1_pipeline_image_configure_borders(source, 0, 0, margin, margin as i32); }
            if !second.is_null() { xm1_pipeline_image_configure_borders(second, 0, 0, margin, margin as i32); }
        }
        let main = 0x154d860 as *mut OutputPlane;
        if selection == 1 {
            (*request).kind = 3;
            (*request).first = first;
            (*request).second = second_value;
            (*request).buffer = buffer.0;
            write_volatile(main, read_volatile(source));
            (*main).address = (*output).address;
            let result = xm1_pipeline_image_process_planes(job.0 as *mut u8, request.cast(), source, core::ptr::null(), core::ptr::null(), 0, main, 0);
            write_volatile(output, read_volatile(main));
            return result;
        }
        (*request).kind = match mode { 1 => 7, 2 => 8, _ => 6 };
        (*request).first = first;
        (*request).second = second_value;
        (*request).paired = (flag == 1) as u32;
        (*request).buffer = buffer.0;
        (*request).enabled = 1;
        write_volatile(main, read_volatile(output));
        (*main).flags |= (*source).flags & 0x2000;
        let capture = word(job.0, 0x4b4);
        if mode == 0 && capture != 0x1d { (*main).flags |= 0x4000000; }
        (*main).width = if mask.is_null() { word(job.0, 0x480).wrapping_mul(2) } else {
            let alignment = if word(job.0, 0x49c) & 0x10 != 0 { 64 } else { 32 };
            word(job.0, 0x42c).wrapping_sub(1).wrapping_add(alignment) / alignment * (alignment * 2)
        };
        (*main).height = word(job.0, 0x484);
        (*main).length = (*main).stride.wrapping_mul((*main).height);
        if capture == 0x19 || (capture == 1 && word(job.0, 0x710) & 0x400 != 0) {
            (*main).address = (*main).address.wrapping_add((*main).stride.wrapping_mul(9));
        }
        let target = if secondary.is_null() { core::ptr::null_mut() } else {
            let target = 0x154d8ac as *mut OutputPlane;
            write_volatile(target, read_volatile(secondary));
            (*target).flags |= (*main).flags & 0x2000;
            if mode == 0 && capture != 0x1d { (*target).flags |= 0x4000000; }
            (*target).width = ((*main).width >> 2).wrapping_add(7) & !0x80000007;
            (*target).height = (*main).height >> 2;
            (*target).stride = (*target).width;
            (*target).length = (*target).height.wrapping_mul((*target).width);
            target
        };
        let input = 0x154cc3c as *mut OutputPlane;
        write_volatile(input, read_volatile(source));
        if !mask.is_null() {
            let width = word(job.0, 0x42c);
            let width = if word(job.0, 0x49c) & 0x10 != 0 { (width.wrapping_add(63) & !63).wrapping_mul(3) >> 1 } else { (width.wrapping_add(31) & !31).wrapping_mul(4) >> 1 };
            (*mask).width = width;
            (*mask).height = word(job.0, 0x428);
            (*mask).length = (*mask).stride.wrapping_mul((*mask).height);
            (*input).width = width;
        }
        let result = xm1_pipeline_image_process_planes(job.0 as *mut u8, request.cast(), input, second, main, target as u32, mask, 0);
        if !mask.is_null() {
            (*mask).width = (*mask).stride;
            let divisor = if (*mask).flags & 0x10 != 0 { 3 } else { 4 };
            let margin = word(job.0, 0x498);
            let right = ((*mask).stride.wrapping_mul(2) / divisor).wrapping_sub(margin.wrapping_add(word(job.0, 0x424)));
            xm1_pipeline_image_configure_borders(mask, 0, 0, margin, right as i32);
            (*main).width = word(job.0, 0x42c).wrapping_mul(2);
        }
        write_volatile(output, read_volatile(main));
        if !secondary.is_null() { write_volatile(secondary, read_volatile(target)); }
        result
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_build_scatter_list(list: ScatterList, image: ImageWork) {
    unsafe {
        let mut count = 0;
        if word(image.0, 0xf2fc) == 1 {
            store(list.0, 8, word(image.0, 0xf310));
            let mut total = 0u32;
            for offset in [0xf324, 0xf328, 0xf32c, 0xf330, 0xf334] { total = total.wrapping_add(word(image.0, offset)); }
            store(list.0, 12, total);
        } else {
            for (address, size, include) in [(0, 8, true), (0x180, 0x188, true), (0x2d8, 0x2e0, word(image.0, 0x424) == 1)] {
                if include {
                    let data = word(image.0, address);
                    let size = word(image.0, size);
                    store(list.0, (count + 1) * 8, data);
                    store(list.0, (count + 1) * 8 + 4, size);
                    if data != 0 && size != 0 { count += 1; }
                }
            }
            store(list.0, (count + 1) * 8, word(image.0, 0x38));
            store(list.0, (count + 1) * 8 + 4, word(image.0, 0x40));
        }
        store(list.0, (count + 2) * 8, 0);
        store(list.0, (count + 2) * 8 + 4, 0);
        store(list.0, 0x88, 0);
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_build_packet_list(list: ScatterList, raw: Address32) {
    unsafe {
        let image = word(raw.0, 0xb608);
        let address = word(image, 0xf310);
        store(list.0, 8, address);
        let mut total = word(image, 8).wrapping_add(word(raw.0, 4)).wrapping_add(word(image, 0x188)).wrapping_add(word(image, 0x48));
        store(list.0, 12, total);
        total = total.wrapping_add(word(raw.0, 0x20));
        store(list.0, 12, total);
        if word(raw.0, 0x3c) != 0 { total = total.wrapping_add(word(raw.0, 0x3c)); store(list.0, 12, total); }
        total = total.wrapping_add(word(raw.0, 0x58));
        store(list.0, 12, total);
        store(list.0, 16, word(raw.0, 0x8c));
        store(list.0, 20, word(raw.0, 0x90));
        let count = if word(raw.0, 0xac) == 0 { 2 } else {
            store(list.0, 24, address.wrapping_add(total));
            store(list.0, 28, word(raw.0, 0x74));
            store(list.0, 32, word(raw.0, 0xa8));
            store(list.0, 36, word(raw.0, 0xac));
            4
        };
        store(list.0, (count + 1) * 8, 0);
        store(list.0, (count + 1) * 8 + 4, 0);
        store(list.0, 0x88, 0);
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_append_output_buffer(list: ScatterList, source: Address32, size: ByteOffset) -> i32 {
    unsafe {
        for slot in 1..=16 {
            if word(list.0, slot * 8) == 0 && word(list.0, slot * 8 + 4) == 0 {
                if slot == 16 { return -1; }
                store(list.0, slot * 8, source.0);
                store(list.0, slot * 8 + 4, size.0);
                store(list.0, (slot + 1) * 8, 0);
                store(list.0, (slot + 1) * 8 + 4, 0);
                store(list.0, 0x88, 0);
                return 0;
            }
        }
        -1
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_initialize_raw_output(image: ImageWork, buffer: Address32, encoding: u32) {
    unsafe {
        let aligned = buffer.0.wrapping_add(63) & !63;
        for (offset, value) in [(0xf310, aligned), (0xf324, 0), (0xf314, aligned), (0xf328, 0), (0xf318, 0), (0xf32c, 0), (0xf31c, 0), (0xf330, 0), (0xf320, 0), (0xf334, 0), (0xf2fc, 1), (0xf304, encoding)] { store(image.0, offset, value); }
    }
}

#[repr(C)]
pub struct MetadataIdentity {
    manufacturer: [u8; 16],
    model: [u8; 32],
    description: [u8; 64],
    firmware: [u8; 4],
    device: [u8; 4],
    serial: [u8; 12],
    copyright: [u8; 30],
}

unsafe fn configuration_text(destination: *mut u8, key: u32, count: usize) {
    unsafe {
        let source = word(0x96ca50, 0).wrapping_add(key) as *const u8;
        let mut ended = false;
        for index in 0..count {
            let value = if ended { 0 } else { read_volatile(source.add(index)) };
            ended |= value == 0;
            write_volatile(destination.add(index), value);
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_read_vendor_text(destination: *mut u8) -> u32 {
    unsafe { configuration_text(destination, 0x16f, 12); write_volatile(destination.add(11), 0); }
    0
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_read_product_text(destination: *mut u8) -> u32 {
    unsafe { configuration_text(destination, 0x15c, 19); write_volatile(destination.add(19), 0); }
    0
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_read_firmware_text(destination: *mut u8) -> u32 {
    unsafe {
        let configuration = word(0x96ca50, 0) as *const u8;
        let text = if read_volatile(configuration.add(0x206)) == 0xff {
            let major = read_volatile(configuration.add(0x204));
            let minor = read_volatile(configuration.add(0x205));
            [b'V', b'e', b'r', (major & 15) + b'0', b'.', (minor >> 4) + b'0', (minor & 15) + b'0', 0]
        } else { *b"Ver1.50\0" };
        for (index, value) in text.into_iter().enumerate() { write_volatile(destination.add(index), value); }
    }
    0
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_read_firmware_word(destination: *mut u8) -> u32 {
    unsafe {
        let source = word(0x96ca50, 0).wrapping_add(0x158) as *const u8;
        for index in 0..4 { write_volatile(destination.add(index), read_volatile(source.add(index))); }
    }
    0
}

unsafe fn identity_configuration(destination: *mut u8, start: u32, length: usize) -> u32 {
    unsafe {
        for index in 0..length {
            let value = output_jpeg_configuration_byte(start + index as u32) as u8;
            let alphanumeric = value.wrapping_sub(b'0') <= 9 || value.wrapping_sub(b'A') <= 25 || value.wrapping_sub(b'a') <= 25;
            write_volatile(destination.add(index), if alphanumeric { value } else { b' ' });
        }
    }
    0
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_read_device_code(destination: *mut u8) -> u32 {
    unsafe { identity_configuration(destination, 0x40, 4) }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_read_serial(destination: *mut u8) -> u32 {
    unsafe { identity_configuration(destination, 0x44, 12) }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_read_copyright(destination: *mut u8) -> u32 {
    unsafe { identity_configuration(destination, 0x11b, 30) }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_set_metadata_identity(identity: *const MetadataIdentity) {
    unsafe {
        let source = identity.cast::<u8>();
        let destination = 0x14e9d98 as *mut u8;
        for index in 0..0xa2 { write_volatile(destination.add(index), read_volatile(source.add(index))); }
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_initialize_metadata_identity() {
    unsafe {
        let ready = 0x14de9ac as *mut u8;
        if read_volatile(ready) == 1 { return; }
        write_volatile(ready, 1);
        let mut identity = MetadataIdentity { manufacturer: [0; 16], model: [0; 32], description: [0; 64], firmware: [0; 4], device: [0; 4], serial: [0; 12], copyright: [0; 30] };
        xm1_pipeline_image_read_vendor_text(identity.manufacturer.as_mut_ptr());
        xm1_pipeline_image_read_product_text(identity.model.as_mut_ptr());
        identity.description[..15].copy_from_slice(b"Digital Camera ");
        xm1_pipeline_image_read_product_text(identity.description.as_mut_ptr().add(15));
        let mut length = 0;
        while identity.description[15 + length] != 0 { length += 1; }
        identity.description[15 + length] = b' ';
        xm1_pipeline_image_read_firmware_text(identity.description.as_mut_ptr().add(16 + length));
        xm1_pipeline_image_read_firmware_word(identity.firmware.as_mut_ptr());
        xm1_pipeline_image_read_device_code(identity.device.as_mut_ptr());
        xm1_pipeline_image_read_serial(identity.serial.as_mut_ptr());
        xm1_pipeline_image_read_copyright(identity.copyright.as_mut_ptr());
        xm1_pipeline_image_set_metadata_identity(&identity);
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_capture_output_metadata(job: OutputJob, metadata: Address32) {
    unsafe {
        if output_jpeg_configuration_byte(0xa8) & 0x80 == 0 || word(job.0, 0x220c) == 0 { return; }
        store(word(job.0, 0x220c), 0xdbf3c, 0x53545053);
        let target = word(job.0, 0x220c);
        let value = output_jpeg_configuration_byte(0x9a82);
        Address32(target + 0xdbf40).write_half(value as u16);
        Address32(word(job.0, 0x220c) + 0xdbf42).write_half(word(metadata.0, 0x308) as u16);
        Address32(word(job.0, 0x220c) + 0xdbf44).write_half(Address32(metadata.0 + 0x568).half());
        let target = word(job.0, 0x220c);
        let key = Address32(job.0 + 0x6c0).byte() as u32 + 0x18fff;
        Address32(target + 0xdbf46).write_half(output_jpeg_configuration_byte(key) as u16);
        for index in 0..5 {
            let target = word(job.0, 0x220c);
            let high = output_jpeg_configuration_byte(0x9a85 + index * 2) as u16;
            let low = output_jpeg_configuration_byte(0x9a84 + index * 2) as u16;
            Address32(target + 0xdbf48 + index * 2).write_half(low | high << 8);
        }
        store(word(job.0, 0x220c), 0xdbf54, 0x4c415053);
    }
}

#[repr(C)]
pub struct MetadataClock { fields: [u8; 6] }

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_format_calendar(destination: *mut u8, clock: *const MetadataClock, order: u32) {
    unsafe {
        let fields = &(*clock).fields;
        let pair = |value: u8| [b'0' + value / 10, b'0' + value % 10];
        let year = pair(fields[0] % 100);
        let month = pair(fields[1]);
        let day = pair(fields[2]);
        let (first, second, third, separator) = match order {
            0 => (year, month, day, b'/'),
            1 => (month, day, year, b'.'),
            _ => (day, month, year, b'/'),
        };
        let hour = pair(fields[3]);
        let minute = pair(fields[4]);
        let second_time = pair(fields[5]);
        let text = [first[0], first[1], separator, second[0], second[1], separator, third[0], third[1], b' ', hour[0], hour[1], b':', minute[0], minute[1], b':', second_time[0], second_time[1], 0];
        for (index, value) in text.into_iter().enumerate() { write_volatile(destination.add(index), value); }
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_format_metadata_date(clock: *const MetadataClock, destination: *mut u8) {
    unsafe {
        let fields = &(*clock).fields;
        let pair = |value: u8| [b'0' + value / 10, b'0' + value % 10];
        let year = pair(fields[0] % 100);
        let month = pair(fields[1]);
        let day = pair(fields[2]);
        let hour = pair(fields[3]);
        let minute = pair(fields[4]);
        let second = pair(fields[5]);
        let text = [b'2', b'0', year[0], year[1], b':', month[0], month[1], b':', day[0], day[1], b' ', hour[0], hour[1], b':', minute[0], minute[1], b':', second[0], second[1], 0];
        for (index, value) in text.into_iter().enumerate() { write_volatile(destination.add(index), value); }
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_initialize_metadata_block(metadata: Address32) {
    unsafe {
        for (index, value) in b"0130".iter().enumerate() { Address32(metadata.0 + index as u32).write_byte(*value); }
        Address32(metadata.0 + 0x70).write_half(0);
        for (index, value) in b"NORMAL \0".iter().enumerate() { Address32(metadata.0 + 0x34 + index as u32).write_byte(*value); }
        store(metadata.0, 0x64, 0);
        store(metadata.0, 0x68, 10);
        Address32(metadata.0 + 0x3c).write_half(3);
        for offset in [0x9c, 0xbe, 0xd0, 0xdc] { Address32(metadata.0 + offset).write_half(0); }
        Address32(metadata.0 + 0xea).write_half(1);
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_initialize_secondary_metadata(metadata: Address32) {
    unsafe {
        for (offset, value) in [(0x6c, 0), (0x6e, 0), (0x3e, 0), (0x9e, 6), (0xa0, 1), (0x40, 0), (0xdc, 0), (0xde, 0), (0xe0, 0), (0xe2, 0), (0x72, 2), (0x74, 2), (0x76, 320), (0xc0, 0)] {
            Address32(metadata.0 + offset).write_half(value);
        }
        xm1_pipeline_image_initialize_metadata_block(metadata);
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_initialize_resolution(metadata: Address32) {
    unsafe {
        for (offset, value) in [(0, 72), (4, 1), (8, 72), (12, 1)] { store(metadata.0, offset, value); }
        for (offset, value) in [(0x28, 6), (0x2a, 1), (0x2e, 2), (0x2c, 2)] { Address32(metadata.0 + offset).write_half(value); }
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_initialize_exif_dates(metadata: Address32, clock: *const MetadataClock) {
    unsafe {
        for offset in [0x116, 0x12a] { xm1_pipeline_image_format_metadata_date(clock, (metadata.0 + offset) as *mut u8); }
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_initialize_exif_defaults(metadata: Address32, clock: *const MetadataClock) {
    unsafe {
        for (offset, value) in [(0x15c, 20), (0x160, 100), (0x14c, 500), (0x150, 100), (0x164, 2), (0x168, 100), (0x28c, 600), (0x290, 500), (8, 500), (12, 100), (0x144, 32), (0x148, 10), (0x154, 0), (0x158, 100), (0x16c, 50), (0x170, 60), (0x17c, 100), (0x180, 100), (0x294, 100), (0x298, 100), (0x29c, 100), (0x2a0, 100)] {
            store(metadata.0, offset, value);
        }
        for (offset, value) in [(0x10, 2), (0x174, 0), (0x178, 0), (0x28a, 1), (0x2a4, 3), (0x2a6, 2), (0x12, 500)] { Address32(metadata.0 + offset).write_half(value); }
        for (offset, value) in [(0x112, b'0'), (0x113, b'2'), (0x114, b'3'), (0x115, b'0'), (0x13e, 1), (0x13f, 2), (0x140, 3), (0x141, 0), (0x286, b'0'), (0x287, b'1'), (0x288, b'0'), (0x289, b'0'), (0x2a8, 3), (0x2aa, 1)] { Address32(metadata.0 + offset).write_byte(value); }
        store(metadata.0, 0, 0x30);
        store(metadata.0, 4, 0x20);
        for offset in [0x176, 0x2ae, 0x2b2, 0x2b0, 0x2b4, 0x2b6, 0x2ac] { Address32(metadata.0 + offset).write_half(0); }
        xm1_pipeline_image_initialize_exif_dates(metadata, clock);
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_initialize_thumbnail_metadata(metadata: Address32, clock: *const MetadataClock) {
    unsafe {
        for (offset, value) in [(0x70, 72), (0x74, 1), (0x78, 72), (0x7c, 1)] { store(metadata.0, offset, value); }
        for (offset, value) in [(0x84, 1), (0x86, 2), (0x88, 2)] { Address32(metadata.0 + offset).write_half(value); }
        for offset in 0..4 { Address32(metadata.0 + 0xb2 + offset).write_byte(b' '); }
        Address32(metadata.0 + 0xb6).write_byte(0);
        xm1_pipeline_image_format_metadata_date(clock, (metadata.0 + 0x9e) as *mut u8);
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_initialize_output_metadata(primary: Address32, secondary: Address32, exif: Address32, _maker: Address32, resolution: Address32) {
    unsafe {
        let mut clock = MetadataClock { fields: [0; 6] };
        xm1_pipeline_image_calendar_read(clock.fields.as_mut_ptr());
        xm1_pipeline_image_initialize_thumbnail_metadata(primary, &clock);
        xm1_pipeline_image_initialize_exif_defaults(secondary, &clock);
        xm1_pipeline_image_initialize_secondary_metadata(exif);
        xm1_pipeline_image_initialize_resolution(resolution);
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_select_raw_buffer(phase: u32, image: ImageWork) {
    unsafe {
        match phase {
            0 => store(image.0, 0xf308, word(image.0, 0xf310)),
            1 => store(image.0, 0, word(image.0, 0xf314)),
            2 => store(image.0, 0x248, word(image.0, 0xf318)),
            3 => store(image.0, 0x420, word(image.0, 0xf31c)),
            4 => store(image.0, 0x100, word(image.0, 0xf320).wrapping_sub(2)),
            _ => {}
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_advance_raw_output(phase: u32, image: ImageWork) {
    unsafe {
        match phase {
            0 => {
                let size = word(image.0, 0xf30c);
                store(image.0, 0xf324, size);
                store(image.0, 0xf314, word(image.0, 0xf310).wrapping_add(size));
            }
            1 => {
                let size = word(image.0, 8);
                store(image.0, 0xf328, size);
                store(image.0, 0xf318, word(image.0, 0xf314).wrapping_add(size));
            }
            2 => {
                let size = word(image.0, 0x188);
                store(image.0, 0xf32c, size);
                let destination = if word(image.0, 0xf304) == 0 { 0xf31c } else { 0xf320 };
                store(image.0, destination, word(image.0, 0xf318).wrapping_add(size));
            }
            3 => {
                let size = word(image.0, 0x2e0);
                store(image.0, 0xf330, size);
                store(image.0, 0xf320, word(image.0, 0xf31c).wrapping_add(size));
            }
            4 => store(image.0, 0xf334, word(image.0, 0x40)),
            _ => {}
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_fill_jpeg_padding(destination: Address32, size: ByteOffset, resume: u32) {
    unsafe {
        let (mut low, mut high, low_first) = if resume == 1 {
            let last = Address32(destination.0.wrapping_sub(1)).byte() as u32;
            let preceding = Address32(destination.0.wrapping_sub(2)).byte() as u32;
            if preceding < last { (preceding + 1, last + 1, true) } else { (last + 1, preceding + 1, false) }
        } else { (0, 128, true) };
        for index in 0..size.0 {
            if low > 127 { low = 0; }
            if high > 255 { high = 128; }
            let value = if (index & 1 == 0) == low_first { let value = low; low += 1; value } else { let value = high; high += 1; value };
            Address32(destination.0.wrapping_add(index)).write_byte(value as u8);
        }
    }
}

unsafe fn copy_bytes(destination: u32, source: u32, size: u32) {
    unsafe {
        for index in 0..size {
            Address32(destination.wrapping_add(index)).write_byte(Address32(source.wrapping_add(index)).byte());
        }
    }
}

unsafe fn fill_bytes(destination: u32, size: u32, value: u8) {
    unsafe { for index in 0..size { Address32(destination.wrapping_add(index)).write_byte(value); } }
}

unsafe fn be16(address: u32) -> u16 {
    unsafe { (Address32(address).byte() as u16) << 8 | Address32(address.wrapping_add(1)).byte() as u16 }
}

unsafe fn write_be16(address: u32, value: u16) {
    unsafe { Address32(address).write_byte((value >> 8) as u8); Address32(address.wrapping_add(1)).write_byte(value as u8); }
}

unsafe fn write_image_exif(image: ImageWork, destination: Address32) -> u32 {
    unsafe {
        xm1_pipeline_image_write_exif_header(destination, Address32(image.0 + 0x42c), Address32(image.0 + 0x5d8), Address32(image.0 + 0x91c), Address32(image.0 + 0xf218), Address32(image.0 + 0xf2b8), word(image.0, 0xf2e8), word(image.0, 0xf2ec))
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_relocate_jpeg(image: ImageWork, relocate: u32) {
    unsafe {
        store(image.0, 0x38, word(image.0, 0x100).wrapping_add(2));
        store(image.0, 0x40, word(image.0, 0x10c).wrapping_sub(2));
        let header = word(image.0, 0);
        let header_size = write_image_exif(image, Address32(header));
        store(image.0, 8, header_size);
        let size = word(image.0, 0x254);
        if relocate != 0 {
            let target = header.wrapping_add(0x40f).wrapping_add(header_size) & !15;
            let source = word(image.0, 0x244);
            if target <= source { copy_bytes(target, source, size); }
            else {
                for index in (0..size).rev() { Address32(target.wrapping_add(index)).write_byte(Address32(source.wrapping_add(index)).byte()); }
            }
            store(image.0, 0x248, target);
        }
        let data = word(image.0, 0x248);
        store(image.0, 0x180, data);
        let mut total = header_size.wrapping_add(size);
        let prefix = word(image.0, 0x18);
        let compact = word(image.0, 0xf2f0) != 0;
        let remainder = if compact { prefix.wrapping_sub(2).wrapping_add(total) & 15 } else { total.wrapping_add(prefix) & 0xffff };
        let output_size = if remainder != 0 {
            let padding = if compact { 16u32.wrapping_sub(remainder) } else { 0xffc0u32.wrapping_sub(remainder) };
            xm1_pipeline_image_fill_jpeg_padding(Address32(data.wrapping_add(size)), ByteOffset(padding), 0);
            total = total.wrapping_add(padding);
            size.wrapping_add(padding)
        } else { size };
        store(image.0, 0x188, output_size);
        xm1_pipeline_image_finish_exif(total, size & 0xffff);
        store(image.0, 0x428, word(image.0, 0x428) | 1);
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_align_jpeg(image: ImageWork) {
    unsafe {
        let remainder = word(image.0, 8).wrapping_add(word(image.0, 0x188)) & 63;
        let padding = if remainder == 0 { 0 } else { 64 - remainder };
        let header = word(image.0, 0);
        write_be16(header.wrapping_add(4), be16(header.wrapping_add(4)).wrapping_add(padding as u16));
        let size = word(image.0, 0x188);
        xm1_pipeline_image_fill_jpeg_padding(Address32(word(image.0, 0x180).wrapping_add(size)), ByteOffset(padding), 1);
        store(image.0, 0x188, word(image.0, 0x188).wrapping_add(padding));
        store(image.0, 0x428, word(image.0, 0x428) | 16);
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_build_raw_exif_header(image: ImageWork) {
    unsafe {
        let start = word(image.0, 0);
        let size = write_image_exif(image, Address32(start));
        let end = start.wrapping_add(size);
        let padding = (end.wrapping_add(15) & !15).wrapping_sub(end);
        store(image.0, 8, padding.wrapping_add(size));
        if padding != 0 { fill_bytes(end, padding, 0); }
        xm1_pipeline_image_patch_thumbnail_offset(word(image.0, 8).wrapping_sub(12));
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_prepare_jpeg_output(image: ImageWork) {
    unsafe {
        if word(image.0, 0xf2fc) == 1 {
            xm1_pipeline_image_select_raw_buffer(1, image);
            xm1_pipeline_image_build_raw_exif_header(image);
            xm1_pipeline_image_advance_raw_output(1, image);
        } else {
            xm1_pipeline_image_relocate_jpeg(image, 1);
            if word(image.0, 0x424) == 1 && word(image.0, 0x428) & 0x13 == 3 { xm1_pipeline_image_align_jpeg(image); }
        }
    }
}

#[derive(Clone, Copy)]
#[repr(C)]
pub struct MpfRecord { flags: u32, size: u32, offset: u32, first: u16, second: u16 }

#[repr(C)]
pub struct MpfIndex { signature: [u8; 4], count: u32, records: [MpfRecord; 2], header: Address32 }

#[derive(Clone, Copy)]
#[repr(C)]
pub struct MpfAttributes { signature: [u8; 4], values: [u32; 6], index: u32, header: Address32 }

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_build_mpf_scatter(list: ScatterList, destinations: *const *mut u32, images: *const ImageWork, count: u32) {
    unsafe {
        store(0x14ddb58, 0, 0);
        let count = count.min(2);
        if count == 0 { store(list.0, 8, 0); store(list.0, 12, 0); store(list.0, 0x88, 0); return; }
        let blank_record = MpfRecord { flags: 0, size: 0, offset: 0, first: 0, second: 0 };
        let mut index = MpfIndex { signature: [0; 4], count: 0, records: [blank_record; 2], header: Address32(0) };
        let blank_attribute = MpfAttributes { signature: [0; 4], values: [0; 6], index: 0, header: Address32(0) };
        let mut attributes = [blank_attribute; 2];
        let mut first_size = 0u32;
        let mut accumulated = 0u32;
        for item in 0..count {
            let image = read_volatile(images.add(item as usize));
            let slot = word(image.0, 0x1c);
            let destination = read_volatile(destinations.add(item as usize));
            let header_size = xm1_pipeline_image_write_mpf_header(item as u8, &mut index, attributes.as_mut_ptr().add(item as usize), destination);
            let remainder = word(image.0, 8).wrapping_add(word(image.0, 0x188)) & 63;
            let padding = if remainder == 0 { 0 } else { 64 - remainder };
            let header = word(image.0, 0).wrapping_add(4);
            write_be16(header, be16(header).wrapping_add(padding as u16));
            xm1_pipeline_image_fill_jpeg_padding(Address32(word(image.0, 0x180).wrapping_add(word(image.0, 0x188))), ByteOffset(padding), 0);
            store(image.0, 0x188, word(image.0, 0x188).wrapping_add(padding));
            let output = word(0x14ddb58, 0);
            let primary_header = word(image.0, 8);
            let primary_size = word(image.0, 0x188);
            let secondary_size = word(image.0, 0x2e0);
            for (entry, address, size) in [(1, word(image.0, 0), primary_header), (2, word(image.0, 0x180), primary_size), (3, word(image.0, 0x2d8), secondary_size), (4, read_volatile(destination), header_size)] {
                store(list.0, (output + entry) * 8, address);
                store(list.0, (output + entry) * 8 + 4, size);
            }
            let image_size = primary_header.wrapping_add(primary_size).wrapping_add(secondary_size);
            if slot == 0 { first_size = image_size.wrapping_add(8); }
            store(0x14ddb58, 0, output + 4);
            let padding = if slot < count - 1 {
                let tail_size = word(image.0, 0x40);
                let remainder = tail_size.wrapping_add(2) & 63;
                let padding = if remainder == 0 { 0 } else { 64 - remainder };
                fill_bytes(word(image.0, 0x38).wrapping_add(tail_size), padding, 0);
                padding
            } else { 0 };
            let tail_size = word(image.0, 0x40).wrapping_add(padding);
            store(list.0, (output + 5) * 8, word(image.0, 0x38));
            store(list.0, (output + 5) * 8 + 4, tail_size);
            store(0x14ddb58, 0, output + 5);
            let total = image_size.wrapping_add(header_size).wrapping_add(tail_size);
            let record = index.records.as_mut_ptr().add(slot as usize);
            (*record).size = total;
            (*record).first = 0;
            (*record).second = 0;
            (*record).offset = if slot == 0 { 0 } else { accumulated.wrapping_sub(first_size) };
            (*record).flags = if slot == 0 { 0x20020002 } else { 0x20002 };
            let attribute = attributes.as_mut_ptr().add(slot as usize);
            (*attribute).signature = *b"0100";
            for entry in 0..6 { (*attribute).values[entry] = word(image.0, 0x20 + entry as u32 * 4); }
            (*attribute).index = slot;
            accumulated = accumulated.wrapping_add(total);
        }
        let output = word(0x14ddb58, 0);
        store(list.0, (output + 1) * 8, 0);
        store(list.0, (output + 1) * 8 + 4, 0);
        store(list.0, 0x88, 0);
        index.signature = *b"0100";
        index.count = count;
        for item in 0..count {
            xm1_pipeline_image_write_mpf_header(item as u8, &mut index, attributes.as_mut_ptr().add(item as usize), read_volatile(destinations.add(item as usize)));
        }
    }
}

#[derive(Clone, Copy)]
enum TemplateKind { Bytes, Halves, Words, Utf16, End, Unknown }

impl TemplateKind {
    fn decode(value: u32) -> Self {
        match value { 0 => Self::Bytes, 1 => Self::Halves, 2 => Self::Words, 3 => Self::Utf16, u32::MAX => Self::End, _ => Self::Unknown }
    }
}

unsafe fn write_ordered(destination: u32, value: u32, width: u32, order: u32) {
    unsafe {
        for index in 0..width {
            let shift = if order == 1 { (width - index - 1) * 8 } else { index * 8 };
            Address32(destination.wrapping_add(index)).write_byte((value >> shift) as u8);
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_write_ascii_utf16(source: Address32, destination: Address32) -> u32 {
    unsafe {
        let mut count = 0;
        loop {
            let value = Address32(source.0.wrapping_add(count)).byte();
            Address32(destination.0.wrapping_add(count.wrapping_mul(2))).write_byte(value);
            Address32(destination.0.wrapping_add(count.wrapping_mul(2)).wrapping_add(1)).write_byte(0);
            count += 1;
            if value == 0 { return count * 2; }
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_write_template(destination: Address32, template: Address32) -> u32 {
    unsafe {
        if destination.0 == 0 || template.0 == 0 { return 0; }
        let mut record = template.0;
        let mut cursor = destination.0;
        loop {
            let kind = TemplateKind::decode(word(record, 0));
            let count = word(record, 4);
            let order = word(record, 8);
            let source = word(record, 12);
            match kind {
                TemplateKind::End => return cursor.wrapping_sub(destination.0),
                TemplateKind::Bytes => { copy_bytes(cursor, source, count); cursor = cursor.wrapping_add(count); }
                TemplateKind::Halves | TemplateKind::Words => {
                    let width = if matches!(kind, TemplateKind::Halves) { 2 } else { 4 };
                    for index in 0..count {
                        let source = source.wrapping_add(index.wrapping_mul(width));
                        if order == 1 || order == 2 {
                            let value = if width == 2 { Address32(source).half() as u32 } else { Address32(source).word() };
                            write_ordered(cursor, value, width, order);
                        } else { copy_bytes(cursor, source, width); }
                        cursor = cursor.wrapping_add(width);
                    }
                }
                TemplateKind::Utf16 => { cursor = cursor.wrapping_add(xm1_pipeline_image_write_ascii_utf16(Address32(source), Address32(cursor))); }
                TemplateKind::Unknown => {}
            }
            record = record.wrapping_add(16);
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_write_mpf_header(kind: u8, index: *mut MpfIndex, attributes: *mut MpfAttributes, destination: *mut u32) -> u32 {
    unsafe {
        Address32(0x14ddb50).write_half(0);
        let mut size = xm1_pipeline_image_write_template(Address32(read_volatile(destination)), Address32(0x11d4eb8));
        (*index).header = Address32(read_volatile(destination).wrapping_add(size));
        (*attributes).header = Address32(read_volatile(destination).wrapping_add(size));
        size = size.wrapping_add(xm1_pipeline_image_write_template(Address32(read_volatile(destination).wrapping_add(size)), Address32(0x11d4b10)));
        if kind == 0 { size = size.wrapping_add(xm1_pipeline_image_write_mpf_index(index, Address32(read_volatile(destination).wrapping_add(size)))); }
        size = size.wrapping_add(xm1_pipeline_image_write_mpf_attributes(attributes, Address32(read_volatile(destination).wrapping_add(size))));
        let padding = (64u32.wrapping_sub(size & 63)) & 63;
        fill_bytes(read_volatile(destination).wrapping_add(size), padding, 0);
        let total = size.wrapping_add(padding);
        Address32(0x14ddb50).write_half(total.wrapping_sub(2) as u16);
        xm1_pipeline_image_write_template(Address32(read_volatile(destination)), Address32(0x11d4eb8));
        total
    }
}

#[repr(transparent)]
pub struct TiffType(pub u16);

#[repr(C)]
pub struct TiffLayout { width: u16, indirect: u16 }

#[repr(C)]
pub struct TiffEntry { tag: u16, kind: u16, count: u32, value: u32 }

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_copy_mpf_records(source: Address32, _entry: Address32, destination: Address32, layout: *const TiffLayout, result: *const TiffEntry) {
    unsafe {
        let mut records = [0u8; 32];
        for (index, byte) in records.iter_mut().enumerate() { *byte = Address32(source.0 + 8 + index as u32).byte(); }
        let size = (*result).count.wrapping_mul((*layout).width as u32);
        for index in 0..size { Address32(destination.0.wrapping_add(index)).write_byte(records[index as usize]); }
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_tiff_type_size(kind: u32) -> u32 {
    unsafe { Address32(0x11c2cb8u32.wrapping_add(kind)).byte() as u32 }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_copy_tiff_value(destination: Address32, source: Address32, size: ByteOffset) -> u32 {
    unsafe { copy_bytes(destination.0, source.0, size.0); }
    size.0
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_allocate_tiff_value(size: ByteOffset, cursor: *mut u32) -> u32 {
    unsafe {
        let start = read_volatile(cursor);
        write_volatile(cursor, start.wrapping_add(size.0));
        if size.0 & 1 != 0 {
            Address32(start.wrapping_add(size.0)).write_byte(0);
            write_volatile(cursor, read_volatile(cursor).wrapping_add(1));
            size.0.wrapping_add(1)
        } else { size.0 }
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_copy_padded_tiff_value(kind: TiffType, size: ByteOffset, source: Address32, destination: Address32) -> u32 {
    unsafe {
        Address32(0x14e9ef0).write_half(kind.0);
        Address32(0x14e9ef2).write_half(size.0 as u16);
        store(0x14e9ea0, 0x54, source.0);
        store(0x14e9ea0, 0x58, destination.0);
        let (count, width) = match kind.0 {
            1 | 2 | 7 => (1, size.0),
            3 => (size.0 >> 1, 2),
            4 | 5 | 9 | 10 => (size.0 >> 2, 4),
            _ => return size.0,
        };
        for index in 0..count { copy_bytes(destination.0.wrapping_add(index.wrapping_mul(width)), source.0.wrapping_add(index.wrapping_mul(width)), width); }
        let end = destination.0.wrapping_add(count.wrapping_mul(width));
        if size.0 < 4 { fill_bytes(end, 4 - size.0, 0); 4 }
        else if size.0 & 1 != 0 { Address32(end).write_byte(0); size.0.wrapping_add(1) & 0xffff }
        else { size.0 }
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_write_tiff_value(rule: Address32, source: Address32, destination: Address32, count: u32, base: Address32, cursor: *mut u32) -> u32 {
    unsafe {
        let initial = read_volatile(cursor);
        copy_bytes(0x14e9ec4, rule.0, 32);
        store(0x14e9ea0, 0x44, count);
        store(0x14e9ea0, 0x48, base.0);
        store(0x14e9ea0, 0x4c, initial);
        store(0x14e9ea0, 0x54, source.0);
        store(0x14e9ea0, 0x58, destination.0);
        copy_bytes(destination.0, rule.0, 2);
        copy_bytes(destination.0 + 2, rule.0 + 4, 2);
        write_ordered(destination.0 + 4, count, 4, 2);
        let kind = TiffType(Address32(rule.0 + 4).half());
        let size = count.wrapping_mul(xm1_pipeline_image_tiff_type_size(kind.0 as u32)) & 0xffff;
        let entry_bytes = if size > 4 {
            write_ordered(destination.0 + 8, read_volatile(cursor).wrapping_sub(base.0), 4, 2);
            let target = read_volatile(cursor);
            let written = xm1_pipeline_image_copy_padded_tiff_value(kind, ByteOffset(size), source, Address32(target));
            write_volatile(cursor, target.wrapping_add(written));
            12
        } else { 8 + xm1_pipeline_image_copy_padded_tiff_value(kind, ByteOffset(size), source, Address32(destination.0 + 8)) };
        entry_bytes.wrapping_add(read_volatile(cursor).wrapping_sub(initial))
    }
}

unsafe fn mpf_directory_debug(kind: u32, source: u32, destination: u32, base: u32) {
    unsafe {
        store(0x14e9ea0, 0, kind);
        store(0x14e9ea0, 4, source);
        store(0x14e9ea0, 8, destination);
        store(0x14e9ea0, 0x48, base);
        for offset in [12, 16, 20, 24, 28, 32] { store(0x14e9ea0, offset, source); }
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_write_mpf_index(index: *mut MpfIndex, destination: Address32) -> u32 {
    unsafe {
        if Address32(0x14e9e3c).half() != 0 { return 0; }
        let source = index as u32;
        let base = (*index).header;
        mpf_directory_debug(7, source, destination.0, base.0);
        Address32(destination.0).write_half(3);
        let mut values = destination.0 + 42;
        let mut size = 2;
        for entry in 0..2 {
            let rule = 0x11d0e5c + entry * 32;
            size += xm1_pipeline_image_write_tiff_value(Address32(rule), Address32(source + word(rule, 28)), Address32(destination.0 + 2 + entry * 12), Address32(rule + 6).half() as u32, base, &mut values);
        }
        Address32(destination.0 + 26).write_half(0xb002);
        Address32(destination.0 + 28).write_half(7);
        write_ordered(destination.0 + 30, 32, 4, 2);
        write_ordered(destination.0 + 34, values.wrapping_sub(base.0), 4, 2);
        let layout = TiffLayout { width: 1, indirect: 1 };
        let entry = TiffEntry { tag: 0xb002, kind: 7, count: 32, value: values.wrapping_sub(base.0) };
        xm1_pipeline_image_copy_mpf_records(Address32(source), Address32(destination.0 + 26), Address32(values), &layout, &entry);
        size += xm1_pipeline_image_allocate_tiff_value(ByteOffset(32), &mut values);
        size += 12;
        store(0x14e9d50, 0x40, destination.0 + 38);
        size + 4
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_write_mpf_attributes(attributes: *mut MpfAttributes, destination: Address32) -> u32 {
    unsafe {
        let source = attributes as u32;
        let base = (*attributes).header;
        if (*attributes).index == 0 { write_ordered(word(0x14e9d50, 0x40), destination.0.wrapping_sub(base.0), 4, 2); }
        if Address32(0x14e9e3c).half() != 0 { return 0; }
        mpf_directory_debug(8, source, destination.0, base.0);
        let first = if (*attributes).index == 0 { 1 } else { 0 };
        let count = 5 - first;
        Address32(destination.0).write_half(count as u16);
        let mut values = destination.0 + count * 12 + 6;
        let mut size = 2;
        for entry in first..5 {
            let rule = 0x11d2324 + entry * 32;
            size += xm1_pipeline_image_write_tiff_value(Address32(rule), Address32(source + word(rule, 28)), Address32(destination.0 + 2 + (entry - first) * 12), Address32(rule + 6).half() as u32, base, &mut values);
        }
        write_ordered(destination.0 + 2 + count * 12, 0, 4, 2);
        size + 4
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_patch_app1_length(length: u16) {
    unsafe {
        let order = if Address32(0x14e9e3e).half() == 0 { 1 } else { 2 };
        write_ordered(word(0x14e9d50, 0x2c), length as u32, 2, order);
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_patch_thumbnail_length(length: u32) {
    unsafe { write_ordered(word(0x14e9d50, 0x20), length, 4, 2); }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_patch_thumbnail_offset(offset: u32) {
    unsafe { write_ordered(word(0x14e9d50, 0x24), offset, 4, 2); }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_finish_exif(size: u32, value: u32) {
    unsafe {
        xm1_pipeline_image_patch_app1_length(size.wrapping_sub(4) as u16);
        xm1_pipeline_image_patch_thumbnail_length(value);
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_write_raf_integer(destination: Address32, value: u32, size: ByteOffset, order: i32) -> u32 {
    unsafe {
        if size.0 == 2 || size.0 == 4 { write_ordered(destination.0, value, size.0, if order == 1 { 1 } else { 2 }); }
        else { destination.write_byte(value as u8); }
        size.0
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_write_raf_header(source: Address32, destination: Address32) -> u32 {
    unsafe {
        copy_bytes(destination.0, 0x14e9d98, 8);
        copy_bytes(destination.0 + 8, source.0 + 0x54, 8);
        copy_bytes(destination.0 + 0x10, source.0 + 0x5c, 4);
        copy_bytes(destination.0 + 0x14, source.0 + 0x60, 8);
        fill_bytes(destination.0 + 0x1c, 32, 0);
        let mut length = 0u32;
        while Address32(0x14e9da8 + length).byte() != 0 { length = length.wrapping_add(1); }
        copy_bytes(destination.0 + 0x1c, 0x14e9da8, length.wrapping_add(1).min(32));
        copy_bytes(destination.0 + 0x3c, source.0 + 0x68, 4);
        copy_bytes(destination.0 + 0x40, source.0, 20);
        for index in 0..16 { xm1_pipeline_image_write_raf_integer(Address32(destination.0 + 0x54 + index * 4), word(source.0, 0x14 + index * 4), ByteOffset(4), 1); }
        148
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_write_raf_tag_records(source: Address32, destination: Address32, tags: Address32, tag_count: u32) -> u32 {
    unsafe {
        xm1_pipeline_image_write_raf_integer(destination, tag_count, ByteOffset(4), 1);
        let mut cursor = destination.0 + 4;
        for index in 0..tag_count {
            let tag = Address32(tags.0 + index * 2).half();
            let rule_index = if tag == 0x100 { Some(0) } else { (1..10).find(|entry| Address32(0x11c89c4 + entry * 12).half() == tag) };
            let Some(rule_index) = rule_index else { continue; };
            let rule = 0x11c89c4 + rule_index * 12;
            let length = Address32(rule + 2).half() as u32;
            let width = Address32(rule + 4).half() as u32;
            let order = Address32(rule + 6).byte() as i8 as i32;
            let value = source.0.wrapping_add(word(rule, 8));
            xm1_pipeline_image_write_raf_integer(Address32(cursor), tag as u32, ByteOffset(2), 1);
            xm1_pipeline_image_write_raf_integer(Address32(cursor + 2), length, ByteOffset(2), 1);
            let payload = cursor + 4;
            let count = length / width;
            match width {
                2 | 4 => {
                    for element in 0..count {
                        let address = Address32(value.wrapping_add(element * width));
                        let sample = if width == 2 { address.half() as u32 } else { address.word() };
                        xm1_pipeline_image_write_raf_integer(Address32(payload + element * width), sample, ByteOffset(width), order);
                    }
                }
                1 if order == 2 => {
                    for element in 0..count {
                        let source = value.wrapping_add(element * 4);
                        let bytes = [Address32(source + 3).byte(), Address32(source + 2).byte(), Address32(source + 1).byte(), Address32(source).byte()];
                        for (offset, byte) in bytes.into_iter().enumerate() { Address32(payload + element * 4 + offset as u32).write_byte(byte); }
                    }
                }
                1 => copy_bytes(payload, value, count),
                _ => {}
            }
            cursor = cursor.wrapping_add(length + 4);
        }
        cursor.wrapping_sub(destination.0)
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_write_raf_directory(source: Address32, destination: Address32) {
    unsafe {
        let mut cursor = destination.0;
        for (offset, width, half) in [(0, 2, true), (2, 2, true), (4, 4, false), (8, 2, true), (12, 2, true), (16, 2, false), (20, 4, false), (24, 4, false), (28, 4, false), (32, 2, true)] {
            let address = Address32(source.0 + offset);
            let value = if half { address.half() as u32 } else { address.word() };
            xm1_pipeline_image_write_raf_integer(Address32(cursor), value, ByteOffset(width), 0);
            cursor += width;
        }
        let count = Address32(source.0 + 0x20).half() as u32;
        for index in 0..count {
            let record = source.0 + 0x24 + index * 16;
            for (offset, width, half) in [(0, 2, true), (4, 2, false), (8, 4, false), (12, 4, false)] {
                let address = Address32(record + offset);
                let value = if half { address.half() as u32 } else { address.word() };
                xm1_pipeline_image_write_raf_integer(Address32(cursor), value, ByteOffset(width), 0);
                cursor += width;
            }
        }
        xm1_pipeline_image_write_raf_integer(Address32(cursor), word(source.0, 0x124), ByteOffset(4), 0);
        cursor += 4;
        for index in 0..204 {
            xm1_pipeline_image_write_raf_integer(Address32(cursor), word(source.0, 0x128 + index * 4), ByteOffset(4), 0);
            cursor += 4;
        }
        xm1_pipeline_image_write_raf_integer(Address32(cursor), word(source.0, 0x458), ByteOffset(4), 0);
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_write_raf_tags(destination: Address32, source: Address32) -> u32 {
    unsafe { xm1_pipeline_image_write_raf_tag_records(source, destination, Address32(0x11db7e4), 10) }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_finish_raf_header(output: Address32, header: Address32, header_size: ByteOffset, prefix_size: ByteOffset, table_offset: ByteOffset, table_size: ByteOffset, directory_extent: ByteOffset, second_table_size: ByteOffset, secondary_extent: ByteOffset) {
    unsafe {
        store(header.0, 0x14, header_size.0);
        store(header.0, 0x18, prefix_size.0);
        let offset = table_offset.0.wrapping_add(header_size.0);
        store(header.0, 0x1c, offset);
        store(header.0, 0x20, table_size.0);
        let offset = table_size.0.wrapping_add(offset);
        store(header.0, 0x24, offset);
        store(header.0, 0x28, directory_extent.0.wrapping_add(2048));
        store(header.0, 0x2c, 0);
        if second_table_size.0 != 0 {
            store(header.0, 0x38, offset);
            let offset = offset.wrapping_add(second_table_size.0);
            store(header.0, 0x24, offset);
            store(header.0, 0x3c, second_table_size.0);
            store(header.0, 0x40, offset.wrapping_add(word(header.0, 0x30)));
            store(header.0, 0x44, secondary_extent.0.wrapping_add(2048));
            store(header.0, 0x48, 0);
        }
        let size = xm1_pipeline_image_write_raf_header(header, output);
        store(header.0, 0x14, size);
        xm1_pipeline_image_write_raf_header(header, output);
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_begin_raf_header(raw: Address32) {
    unsafe {
        xm1_pipeline_image_select_raw_buffer(0, ImageWork(word(raw.0, 0xb608)));
        let image = word(raw.0, 0xb608);
        let output = word(image, 0xf308);
        store(raw.0, 0, output);
        let size = xm1_pipeline_image_write_raf_header(Address32(raw.0 + 0xdc), Address32(output));
        store(image, 0xf30c, size);
        xm1_pipeline_image_advance_raw_output(0, ImageWork(word(raw.0, 0xb608)));
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_pack_raf(header: Address32, primary: Address32, secondary: Address32, directory: Address32, second_directory: Address32, output: Address32, header_size: ByteOffset, prefix_size: ByteOffset, padding_size: *mut u32, primary_extent: ByteOffset, secondary_extent: ByteOffset, primary_size: *mut u32, secondary_size: *mut u32, directory_size: *mut u32, second_directory_size: *mut u32) {
    unsafe {
        let mut cursor = output.0.wrapping_add(header_size.0).wrapping_add(prefix_size.0);
        if secondary_extent.0 == 0 {
            write_volatile(secondary_size, 0);
            write_volatile(second_directory_size, 0);
        }
        write_volatile(primary_size, xm1_pipeline_image_write_raf_tags(Address32(cursor), primary));
        if secondary_extent.0 != 0 { write_volatile(secondary_size, xm1_pipeline_image_write_raf_tags(Address32(cursor), secondary)); }
        let padding = 0u32.wrapping_sub(prefix_size.0.wrapping_add(header_size.0).wrapping_add(read_volatile(primary_size)).wrapping_add(read_volatile(secondary_size))) & 511;
        write_volatile(padding_size, padding);
        fill_bytes(cursor, padding, 0);
        cursor = cursor.wrapping_add(padding);
        let size = xm1_pipeline_image_write_raf_tags(Address32(cursor), primary);
        write_volatile(primary_size, size);
        cursor = cursor.wrapping_add(size);
        if secondary_extent.0 != 0 {
            let size = xm1_pipeline_image_write_raf_tags(Address32(cursor), secondary);
            write_volatile(secondary_size, size);
            cursor = cursor.wrapping_add(size);
        }
        fill_bytes(cursor, 2048, 0);
        xm1_pipeline_image_write_raf_directory(directory, Address32(cursor));
        write_volatile(directory_size, 2048);
        if secondary_extent.0 != 0 {
            cursor = cursor.wrapping_add(2048);
            fill_bytes(cursor, 2048, 0);
            xm1_pipeline_image_write_raf_directory(second_directory, Address32(cursor));
            write_volatile(second_directory_size, 2048);
        }
        xm1_pipeline_image_finish_raf_header(output, header, header_size, prefix_size, ByteOffset(padding.wrapping_add(prefix_size.0)), ByteOffset(read_volatile(primary_size)), primary_extent, ByteOffset(read_volatile(secondary_size)), secondary_extent);
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_build_raf_header(raw: Address32) {
    unsafe {
        xm1_pipeline_image_select_raw_buffer(0, ImageWork(word(raw.0, 0xb608)));
        let output = Address32(word(word(raw.0, 0xb608), 0xf308));
        store(raw.0, 0, output.0);
        let primary_extent = ByteOffset(word(raw.0, 0x90));
        let secondary_extent = ByteOffset(word(raw.0, 0xac));
        let header_size = xm1_pipeline_image_write_raf_header(Address32(raw.0 + 0xdc), output);
        let image = word(raw.0, 0xb608);
        let prefix = word(image, 0x40).wrapping_add(word(image, 8)).wrapping_add(word(image, 0x188));
        let mut padding = 0;
        let mut primary_size = 0;
        let mut secondary_size = 0;
        let mut directory_size = 0;
        let mut second_directory_size = 0;
        xm1_pipeline_image_pack_raf(Address32(raw.0 + 0xdc), Address32(raw.0 + 0x148), Address32(raw.0 + 0x574c), Address32(raw.0 + 0xad50), Address32(raw.0 + 0xb1ac), output, ByteOffset(header_size), ByteOffset(prefix), &mut padding, primary_extent, secondary_extent, &mut primary_size, &mut secondary_size, &mut directory_size, &mut second_directory_size);
        let image = word(raw.0, 0xb608);
        store(image, 0x48, word(image, 0x40).wrapping_add(padding));
        store(raw.0, 4, header_size);
        store(raw.0, 0x20, primary_size);
        store(raw.0, 0x3c, secondary_size);
        store(raw.0, 0x58, directory_size);
        store(raw.0, 0x74, second_directory_size);
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_public_version_bytes(destination: *mut u8) -> u32 {
    unsafe {
        let configuration = word(0x96ca50, 0);
        if Address32(configuration + 0x206).byte() == 0xff {
            write_volatile(destination, Address32(configuration + 0x204).byte());
            write_volatile(destination.add(1), Address32(word(0x96ca50, 0) + 0x205).byte());
        } else { write_volatile(destination, 1); write_volatile(destination.add(1), 0x50); }
        write_volatile(destination.add(2), 0);
        0
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_initialize_raf_metadata(header: Address32) {
    unsafe {
        copy_bytes(header.0 + 0x54, 0x11dbf04, 8);
        copy_bytes(header.0 + 0x5c, 0x11dbf10, 4);
        fill_bytes(header.0 + 0x60, 8, 0);
        let mut version = [0u8; 3];
        xm1_pipeline_image_public_version_bytes(version.as_mut_ptr());
        for index in 0..2 {
            Address32(header.0 + 0x68 + index * 2).write_byte((version[index as usize] >> 4) + b'0');
            Address32(header.0 + 0x69 + index * 2).write_byte((version[index as usize] & 15) + b'0');
        }
        fill_bytes(header.0, 20, 0);
        for offset in [0x14, 0x18, 0x1c, 0x20, 0x24, 0x2c, 0x30, 0x34, 0x38, 0x3c, 0x40, 0x44, 0x48, 0x4c, 0x50] { store(header.0, offset, 0); }
        store(header.0, 0x28, 0x600000);
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_prepare_raf_metadata(job: OutputJob, primary: Address32, secondary: Address32, image: ImageWork, raw: Address32) {
    unsafe {
        let header = Address32(raw.0 + 0xdc);
        xm1_pipeline_image_initialize_raf_metadata(header);
        store(raw.0, 0x8c, word(primary.0, 4));
        store(raw.0, 0x90, word(primary.0, 8));
        if secondary.0 != 0 && word(secondary.0, 4) != 0 {
            store(raw.0, 0xa8, word(secondary.0, 4));
            store(raw.0, 0xac, word(secondary.0, 8));
        } else { store(raw.0, 0xa8, 0); store(raw.0, 0xac, 0); }
        store(raw.0, 0xb608, image.0);
        store(raw.0, 0x10c, word(primary.0, 8).wrapping_add(2048));
        store(raw.0, 0x110, 0);
        let paired = secondary.0 != 0 && word(secondary.0, 4) != 0;
        store(raw.0, 0x128, if paired { word(secondary.0, 8).wrapping_add(2048) } else { 0 });
        store(raw.0, 0x12c, 0);
        xm1_pipeline_image_populate_raf_metadata(job, header, Address32(raw.0 + 0x148), Address32(if paired { raw.0 + 0x574c } else { 0 }), Address32(raw.0 + 0xad50), Address32(if paired { raw.0 + 0xb1ac } else { 0 }));
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_metadata_quality_selection(job: OutputJob) -> u32 {
    unsafe {
        if word(job.0, 0x4b4) == 0x2c { word(job.0, 0x510) }
        else { word(job.0, if word(job.0, 0x1678) != 0 { 0x514 } else { 0x50c }) }
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_copy_capture_date(job: OutputJob, metadata: Address32) {
    unsafe { copy_bytes(metadata.0 + 0x9e, job.0 + 0x1684, 20); }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_copy_exif_creation_date(job: OutputJob, metadata: Address32, primary: Address32) {
    unsafe {
        for index in 0..20 {
            let address = if word(job.0, 0x78) == 11 { job.0 + 0x197e + index } else { primary.0 + 0x9e + index };
            Address32(metadata.0 + 0x116 + index).write_byte(Address32(address).byte());
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_copy_exif_digitization_date(job: OutputJob, metadata: Address32, primary: Address32) {
    unsafe {
        for index in 0..20 {
            let address = if word(job.0, 0x78) == 11 { job.0 + 0x1992 + index } else { primary.0 + 0x9e + index };
            Address32(metadata.0 + 0x12a + index).write_byte(Address32(address).byte());
        }
    }
}

#[derive(Clone, Copy)]
struct ExposureCompensation(i32);

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_store_exposure_compensation(job: OutputJob, metadata: Address32) {
    unsafe {
        let delta = if word(job.0, 0x4a8) == 4 { match word(job.0, 0x4cc) { value @ 1..=3 => value, _ => 0 } } else { 0 };
        let base = word(job.0, 0x4c4);
        let value = match Address32(job.0 + 0x1680).half() {
            2 => base.wrapping_add(delta),
            3 => base.wrapping_sub(delta),
            _ => base,
        };
        let compensation = ExposureCompensation((value as i32).clamp(-9, 9));
        store(metadata.0, 0x164, word(0xeb027c, (compensation.0 + 9) as u32 * 4));
        store(metadata.0, 0x168, 100);
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_rotate_metadata_orientation(selection: u32, orientation: u32) -> u32 {
    match selection {
        2 => match orientation { 1 => 3, 2 => 4, 3 => 2, _ => 1 },
        3 => match orientation { 1 => 4, 2 => 1, 3 => 3, _ => 2 },
        4 => match orientation { 1 => 1, 2 => 2, 3 => 4, _ => 3 },
        _ => match orientation { 1 => 2, 2 => 3, 3 => 1, _ => 4 },
    }
}

unsafe fn copy_text(destination: u32, source: u32, length: u32) {
    unsafe {
        let mut terminated = false;
        for index in 0..length {
            let value = if terminated { 0 } else { Address32(source.wrapping_add(index)).byte() };
            terminated |= value == 0;
            Address32(destination.wrapping_add(index)).write_byte(value);
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_populate_location_metadata(job: OutputJob, metadata: Address32) {
    unsafe {
        copy_bytes(metadata.0, job.0 + 0x17e4, 4);
        copy_bytes(metadata.0 + 4, job.0 + 0x17e8, 2);
        for offset in (8..=0x1c).step_by(4) { store(metadata.0, offset, word(job.0, 0x17e4 + offset)); }
        copy_bytes(metadata.0 + 0x20, job.0 + 0x1804, 2);
        for offset in (0x24..=0x38).step_by(4) { store(metadata.0, offset, word(job.0, 0x17e4 + offset)); }
        Address32(metadata.0 + 0x3c).write_byte(Address32(job.0 + 0x1820).byte());
        for offset in (0x40..=0x5c).step_by(4) { store(metadata.0, offset, word(job.0, 0x17e4 + offset)); }
        for offset in [0x60, 0x63, 0x65] { Address32(metadata.0 + offset).write_byte(0); }
        store(metadata.0, 0x68, 0);
        store(metadata.0, 0x6c, 0);
        copy_bytes(metadata.0 + 0x70, job.0 + 0x1854, 2);
        store(metadata.0, 0x74, word(job.0, 0x1858));
        store(metadata.0, 0x78, word(job.0, 0x185c));
        copy_bytes(metadata.0 + 0x7c, job.0 + 0x1860, 2);
        store(metadata.0, 0x80, word(job.0, 0x1864));
        store(metadata.0, 0x84, word(job.0, 0x1868));
        copy_text(metadata.0 + 0x88, job.0 + 0x186c, 12);
        copy_bytes(metadata.0 + 0x94, job.0 + 0x1874, 11);
    }
}

#[repr(transparent)]
pub struct PixelExtent(pub u32);

#[repr(transparent)]
pub struct QualityProfile(pub u32);

#[repr(transparent)]
pub struct QualitySlot(pub i32);

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_find_quality_profile(width: PixelExtent, height: PixelExtent) -> QualityProfile {
    unsafe {
        for index in 0..40 {
            let row = 0x11dc3a0 + index * 6;
            let row_width = Address32(row).half() as i16 as i32 as u32;
            let row_height = Address32(row + 2).half() as i16 as i32 as u32;
            if (row_width == width.0 && row_height == height.0) || row_width == u32::MAX {
                return QualityProfile(Address32(row + 4).byte() as u32);
            }
        }
        QualityProfile(0)
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_initialize_quality_tables() {
    unsafe {
        if Address32(0x14de9ad).byte() == 1 { return; }
        fill_bytes(0x14df450, 0x640, 0);
        fill_bytes(0x14de9b0, 0x3c0, 0);
        fill_bytes(0x14df3b0, 0xa0, 0);
        fill_bytes(0x14ded70, 0x640, 0);
        Address32(0x14de9ad).write_byte(1);
        for index in 0..128 {
            let parameter = 0x11dc490 + index * 16;
            let kind = word(parameter, 0);
            let slot = word(parameter, 4);
            let quality = word(parameter, 8);
            let (scale, value) = if quality == 1 { (40, 64) } else {
                let key = word(parameter, 12).wrapping_mul(3);
                let scale = output_jpeg_configuration_byte(key.wrapping_add(0x106c2));
                let high = output_jpeg_configuration_byte(key.wrapping_add(0x106c0));
                let low = output_jpeg_configuration_byte(key.wrapping_add(0x106c1));
                (scale, (high.wrapping_mul(256).wrapping_add(low)) & 0xffff)
            };
            let entry = slot.wrapping_add(quality.wrapping_mul(2));
            let quality_address = Address32(0x14df3b0u32.wrapping_add(entry.wrapping_mul(2)));
            if quality_address.byte() == 0 {
                quality_address.write_byte(scale as u8);
                quality_address.offset(ByteOffset(1)).write_byte(10);
            }
            let limits = (if kind == 1 { 0x14ded70u32 } else { 0x14df450u32 }).wrapping_add(entry.wrapping_mul(20));
            if word(limits, 0) == 0 {
                for (offset, value) in [(0, value), (4, 0), (8, 0xa00), (12, 0), (16, 0)] { store(limits, offset, value); }
            }
            let geometry = 0x14de9b0u32.wrapping_add(entry.wrapping_mul(12));
            if word(geometry, 4) == 0 {
                store(geometry, 0, 0);
                store(geometry, 4, 0xa00);
                store(geometry, 8, 0);
            }
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_get_metadata_quality(_kind: u32, width: PixelExtent, height: PixelExtent, slot: QualitySlot, _reserved: u32, output: *mut Address32) -> u32 {
    unsafe {
        xm1_pipeline_image_initialize_quality_tables();
        let profile = xm1_pipeline_image_find_quality_profile(width, height);
        if slot.0 > 1 || profile.0 == 0 { loop { core::hint::spin_loop(); } }
        let entry = (slot.0 as u32).wrapping_add(profile.0.wrapping_mul(2));
        write_volatile(output, Address32(0x14df3b0u32.wrapping_add(entry.wrapping_mul(2))));
        0
    }
}

unsafe extern "C" {
    fn xm1_develop_capture_option_selection() -> u32;
    fn xm1_pipeline_image_configure_borders(plane: *mut OutputPlane, top: u32, bottom: i32, left: u32, right: i32);
    fn xm1_pipeline_image_process_planes(job: *mut u8, request: *const u32, source: *const OutputPlane, auxiliary: *const OutputPlane, plane: *const OutputPlane, option: u32, destination: *mut OutputPlane, flags: u32) -> u32;
    #[link_name = "xm1_buffers_configuration_byte"]
    fn output_jpeg_configuration_byte(key: u32) -> u32;
    fn output_jpeg_mpf_scatter(list: ScatterList, destinations: *const *mut u32, images: *const ImageWork, count: u32);
    fn output_jpeg_build_raf(raw: Address32);
    #[link_name = "xm1_pipeline_image_initialize_metadata_identity"]
    fn output_jpeg_initialize_identity();
    #[link_name = "xm1_pipeline_image_initialize_output_metadata"]
    fn output_jpeg_initialize_metadata(primary: Address32, secondary: Address32, exif: Address32, maker: Address32, auxiliary: Address32, mode: u32, value: u32);
    fn output_jpeg_populate_metadata(job: OutputJob, primary: Address32, auxiliary: Address32, secondary: Address32, exif: Address32, maker: Address32, paired: u32);
    fn output_jpeg_special_metadata(job: OutputJob, exif: Address32);
    #[link_name = "xm1_pipeline_image_capture_output_metadata"]
    fn output_jpeg_capture_metadata(job: OutputJob, primary: Address32);
    fn output_jpeg_prepare_raf(job: OutputJob, primary: Address32, secondary: Address32, image: ImageWork, raw: Address32);
    fn output_jpeg_begin_raf(raw: Address32);
    fn output_jpeg_prepare_image(image: ImageWork);
    fn xm1_pipeline_image_calendar_read(fields: *mut u8);
    fn xm1_pipeline_image_write_exif_header(destination: Address32, primary: Address32, secondary: Address32, exif: Address32, maker: Address32, auxiliary: Address32, mode: u32, value: u32) -> u32;
    fn xm1_pipeline_image_populate_raf_metadata(job: OutputJob, header: Address32, primary_tags: Address32, secondary_tags: Address32, primary_directory: Address32, secondary_directory: Address32);
}
