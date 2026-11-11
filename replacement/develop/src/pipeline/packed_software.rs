use super::shared::{Address32, ByteOffset};

#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct PixelCount(u32);

#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct Opacity(u32);

#[repr(C)]
pub struct PackedRequest {
    source: Address32,
    destination: Address32,
    source_width: PixelCount,
    source_height: PixelCount,
    destination_width: PixelCount,
    destination_height: PixelCount,
    source_stride: PixelCount,
    destination_stride: PixelCount,
    crop_row: PixelCount,
    reserved_24: u32,
    crop_column: PixelCount,
    reserved_2c: u32,
    opacity: Opacity,
    rotation_degrees: i32,
}

unsafe extern "C" {
    fn xm1_pipeline_packed_cache_read(address: Address32, bytes: u32) -> u32;
    fn xm1_pipeline_packed_cache_write(address: Address32, bytes: u32) -> u32;
}

const HORIZONTAL_MAP: Address32 = Address32(0x0095e1f8);
const VERTICAL_MAP: Address32 = Address32(0x009621f8);

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_image_fixed10_sine_degrees(angle: u32) -> i32 {
    unsafe { Address32(0x007d02ec).offset(ByteOffset((angle % 360) * 4)).word() as i32 }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_image_fixed10_cosine_degrees(angle: u32) -> i32 {
    unsafe { Address32(0x007cfd4c).offset(ByteOffset((angle % 360) * 4)).word() as i32 }
}

impl PackedRequest {
    fn source_start(&self) -> Address32 {
        Address32(self.source.0.wrapping_add(self.crop_row.0.wrapping_mul(self.source_stride.0).wrapping_add(self.crop_column.0).wrapping_mul(2)) & 0xbfff_ffff)
    }

    fn destination_start(&self) -> Address32 {
        Address32(self.destination.0 & 0xbfff_ffff)
    }

    unsafe fn begin(&self) {
        unsafe { xm1_pipeline_packed_cache_read(self.source_start(), self.source_height.0.wrapping_mul(self.source_stride.0).wrapping_mul(2)) };
    }

    unsafe fn finish(&self) -> u32 {
        unsafe { xm1_pipeline_packed_cache_write(self.destination_start(), self.destination_height.0.wrapping_mul(self.destination_stride.0).wrapping_mul(2)) }
    }
}

fn luma(pair: u32, column: u32) -> u32 {
    (pair >> ((column & 1) * 8)) & 255
}

fn pair(first: u32, first_column: u32, second: u32, second_column: u32) -> u32 {
    luma(first, first_column) | (luma(second, second_column) << 8)
        | ((((first >> 16) & 255) + ((second >> 16) & 255)) >> 1) << 16
        | (((first >> 24) + (second >> 24)) >> 1) << 24
}

unsafe fn store(destination: Address32, pixels: u32, opacity: Opacity) {
    if opacity.0 >= 128 {
        unsafe { destination.write_word(pixels) };
    } else {
        for channel in 0..4 {
            let address = destination.offset(ByteOffset(channel));
            let old = unsafe { address.byte() } as u32;
            let new = (pixels >> (channel * 8)) & 255;
            unsafe { address.write_byte(((old * (128 - opacity.0) + new * opacity.0) >> 7) as u8) };
        }
    }
}

unsafe fn coordinate_map(base: Address32, source: PixelCount, destination: PixelCount) {
    let limit = destination.0.wrapping_shl(10);
    let mut position = 0u32;
    let mut numerator = 0u32;
    let mut address = base;
    while position < limit {
        unsafe { address.write_word(numerator / destination.0) };
        numerator = numerator.wrapping_add(source.0.wrapping_shl(10));
        position = position.wrapping_add(1024);
        address = address.offset(ByteOffset(4));
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_image_scale_packed_buffer(request: *const PackedRequest) -> u32 {
    let request = unsafe { &*request };
    unsafe { request.begin() };
    if request.opacity.0 != 0 {
        unsafe {
            coordinate_map(HORIZONTAL_MAP, request.source_width, request.destination_width);
            coordinate_map(VERTICAL_MAP, request.source_height, request.destination_height);
        }
        for row in 0..request.destination_height.0 {
            let source_row = (unsafe { VERTICAL_MAP.offset(ByteOffset(row.wrapping_mul(4))).word() } >> 10).min(request.source_height.0);
            let source = request.source_start().offset(ByteOffset((request.source_stride.0 >> 1).wrapping_mul(source_row).wrapping_mul(4)));
            let destination = request.destination_start().offset(ByteOffset((request.destination_stride.0 >> 1).wrapping_mul(row).wrapping_mul(4)));
            for column in 0..(request.destination_width.0 >> 1) {
                let map = HORIZONTAL_MAP.offset(ByteOffset(column.wrapping_mul(8)));
                let first_column = (unsafe { map.word() } >> 10).min(request.source_width.0);
                let second_raw = unsafe { map.offset(ByteOffset(4)).word() } >> 10;
                let first = unsafe { source.offset(ByteOffset((first_column >> 1).wrapping_mul(4))).word() };
                let pixels = if second_raw == first_column {
                    pair(first, first_column, first, first_column)
                } else {
                    let second_column = second_raw.min(request.source_width.0);
                    let second = unsafe { source.offset(ByteOffset((second_column >> 1).wrapping_mul(4))).word() };
                    pair(first, first_column, second, second_column)
                };
                unsafe { store(destination.offset(ByteOffset(column.wrapping_mul(4))), pixels, request.opacity) };
            }
        }
    }
    unsafe { request.finish() }
}

unsafe fn rotated_sample(request: &PackedRequest, x: i32, y: i32, destination: Address32) -> u32 {
    if y < 0 || y > request.source_height.0 as i32 {
        unsafe { destination.word() }
    } else {
        let width = request.source_width.0 as i32;
        let clamped_x = if x < 0 { 0 } else if x > width { width } else { x };
        let offset = (request.source_stride.0 as i32 >> 1).wrapping_mul(y).wrapping_add(clamped_x >> 1).wrapping_mul(4);
        let pixels = unsafe { request.source_start().offset(ByteOffset(offset as u32)).word() };
        if x < 0 || x > width { unsafe { destination.word() } } else { pixels }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_image_rotate_packed_buffer(request: *const PackedRequest) -> u32 {
    let request = unsafe { &*request };
    unsafe { request.begin() };
    let sine = unsafe { xm1_pipeline_image_fixed10_sine_degrees(request.rotation_degrees as u32) };
    let cosine = unsafe { xm1_pipeline_image_fixed10_cosine_degrees(request.rotation_degrees as u32) };
    let width = request.source_width.0.wrapping_sub(1) as i32;
    let height = request.source_height.0.wrapping_sub(1) as i32;
    let (mut min_x, mut max_x, mut min_y, mut max_y) = (0i32, 0i32, 0i32, 0i32);
    for (x, y) in [(width, 0), (width, height), (0, height)] {
        let rotated_x = x.wrapping_mul(cosine).wrapping_sub(y.wrapping_mul(sine)) >> 10;
        let rotated_y = x.wrapping_mul(sine).wrapping_add(y.wrapping_mul(cosine)) >> 10;
        min_x = min_x.min(rotated_x);
        max_x = max_x.max(rotated_x);
        min_y = min_y.min(rotated_y);
        max_y = max_y.max(rotated_y);
    }
    let pairs = (max_x.wrapping_sub(min_x).wrapping_add(1) as u32) >> 1;
    let rows = max_y.wrapping_sub(min_y).wrapping_add(1) as u32;
    let mut row_x = min_x.wrapping_mul(cosine).wrapping_add(min_y.wrapping_mul(sine));
    let mut row_y = min_x.wrapping_neg().wrapping_mul(sine).wrapping_add(min_y.wrapping_mul(cosine));
    for row in 0..rows {
        let mut x = row_x;
        let mut y = row_y;
        let destination = request.destination_start().offset(ByteOffset((request.destination_stride.0 >> 1).wrapping_mul(row).wrapping_mul(4)));
        for column in 0..pairs {
            let output = destination.offset(ByteOffset(column.wrapping_mul(4)));
            let first_x = x >> 10;
            let first_y = y >> 10;
            let second_x = x.wrapping_add(cosine) >> 10;
            let second_y = y.wrapping_sub(sine) >> 10;
            let first = unsafe { rotated_sample(request, first_x, first_y, output) };
            let second = if first_x == second_x && first_y == second_y { first } else { unsafe { rotated_sample(request, second_x, second_y, output) } };
            unsafe { store(output, pair(first, first_x as u32, second, second_x as u32), request.opacity) };
            x = x.wrapping_add(cosine.wrapping_mul(2));
            y = y.wrapping_sub(sine.wrapping_mul(2));
        }
        row_x = row_x.wrapping_add(sine);
        row_y = row_y.wrapping_add(cosine);
    }
    unsafe { request.finish() }
}
