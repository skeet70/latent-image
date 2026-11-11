use super::shared::{Address32, ByteOffset};
use core::mem::MaybeUninit;
use core::ptr::{read_volatile, write_volatile};

#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct Utf16Unit(pub u16);

#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct PixelExtent(pub u16);

#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct ProgramAddress(pub u32);

#[derive(Clone, Copy)]
#[repr(C)]
pub struct StampRequest {
    pub address: ProgramAddress,
    pub enabled: u32,
    pub width: PixelExtent,
    pub height: PixelExtent,
    pub first: PixelExtent,
    pub second: PixelExtent,
}

#[derive(Clone, Copy)]
#[repr(C)]
pub struct StampResult {
    pub status: u32,
    pub first: u32,
    pub second: u32,
}

#[derive(Clone, Copy)]
#[repr(C)]
pub struct TextRequest {
    pub first_percent: u32,
    pub second_percent: u32,
    pub metadata: u32,
    pub first_text: *mut Utf16Unit,
    pub second_text: *mut Utf16Unit,
    pub address: ProgramAddress,
    pub completion: u32,
}

#[derive(Clone, Copy)]
enum DateFormat { YearMonthDay, MonthDayYear, DayMonthYear }
impl DateFormat {
    fn parse(value: u32) -> Self {
        match value { 0 => Self::YearMonthDay, 1 => Self::MonthDayYear, _ => Self::DayMonthYear }
    }
}

#[derive(Clone, Copy)]
enum TextLimit { Unlimited, Maximum(u8) }
impl TextLimit {
    fn parse(value: u32) -> Self {
        if value == 255 { Self::Unlimited } else { Self::Maximum(value as u8) }
    }
}

fn invalid() -> ! { loop { core::hint::spin_loop(); } }
unsafe fn word(base: u32, offset: u32) -> u32 {
    unsafe { Address32(base).offset(ByteOffset(offset)).word() }
}
unsafe fn store(base: u32, offset: u32, value: u32) {
    unsafe { Address32(base).offset(ByteOffset(offset)).write_word(value) };
}
unsafe fn copy_words(source: u32, destination: u32, count: u32) {
    for index in 0..count { unsafe { store(destination, index * 4, word(source, index * 4)) }; }
}

unsafe extern "C" {
    fn xm1_buffers_configuration_byte(key: u32) -> u32;
    fn xm1_pipeline_image_current_task() -> u32;
    fn xm1_pipeline_image_configure_task(task: u32, priority: u32) -> i32;
    fn xm1_pipeline_image_wait_all(mask: u32, events: *mut u32, timeout: u32) -> i32;
    fn xm1_pipeline_image_signal_current_zero() -> i32;
    fn xm1_pipeline_image_cache_maintenance();
    fn xm1_property_get(key: u32, bank: u32, defaults: u32, slot: u32) -> u32;
    fn xm1_pipeline_image_fixed10_sine_degrees(angle: u32) -> i32;
    fn xm1_pipeline_image_fixed10_cosine_degrees(angle: u32) -> i32;
    fn xm1_pipeline_packed_cache_read(address: Address32, length: u32) -> u32;
    fn xm1_pipeline_packed_cache_write(address: Address32, length: u32) -> u32;
    fn xm1_pipeline_image_utf16_load(address: u32) -> u32;
    fn xm1_pipeline_image_utf16_store(address: u32, value: u32);
    fn xm1_pipeline_image_send_notification(task: u32, value: u32) -> i32;
}

unsafe fn truncate(text: *mut Utf16Unit, limit: TextLimit) {
    let TextLimit::Maximum(maximum) = limit else { return; };
    let mut count = 0u8;
    loop {
        if unsafe { xm1_pipeline_image_utf16_load((text as u32).wrapping_add(u32::from(count) * 2)) } == 0 { break; }
        count = count.wrapping_add(1);
        if count == maximum.wrapping_add(1) { break; }
    }
    if count > maximum {
        for index in 29..32 { unsafe { xm1_pipeline_image_utf16_store((text as u32).wrapping_add(index * 2), 46) }; }
        unsafe { xm1_pipeline_image_utf16_store((text as u32).wrapping_add(64), 0) };
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_prepare_text(
    input: *const StampRequest, first: *mut Utf16Unit, second: *mut Utf16Unit,
    orientation: u32, output: *mut StampResult,
) -> u32 {
    let limit = TextLimit::parse(unsafe { xm1_buffers_configuration_byte(0x2db) });
    unsafe { copy_words(input as u32, 0x0154_cc80, 4) };
    let request = unsafe { read_volatile(input) };
    if first.is_null() || request.address.0 == 0 || request.width.0 == 0 || request.height.0 == 0 { invalid(); }
    unsafe { truncate(first, limit); truncate(second, limit); }
    let percent = if orientation == 1 || orientation == 3 {
        (u32::from(request.height.0) * 28 + 500) / 1000
    } else { (u32::from(request.width.0) * 21 + 500) / 1000 }.min(100);
    let render = TextRequest {
        first_percent: percent, second_percent: percent, metadata: request.enabled,
        first_text: first, second_text: second, address: request.address,
        completion: xm1_pipeline_image_date_notify as *const () as u32,
    };
    unsafe { write_volatile(0x0154_d8e4 as *mut TextRequest, render) };
    unsafe { xm1_pipeline_image_queue_text(0x0154_d8e4 as *const TextRequest) };
    let task = unsafe { xm1_pipeline_image_current_task() };
    unsafe { store(0x0155_ccac, 0x14, task) };
    let mut events = 0;
    unsafe { xm1_pipeline_image_wait_all(0x10, &mut events, u32::MAX) };
    unsafe { store(0x0155_ccac, 0x14, 0); xm1_pipeline_image_signal_current_zero(); }
    if unsafe { word(0x0154_d964, 0) } == 0 { invalid(); }
    unsafe { write_volatile(output, StampResult { status: 1, first: word(0x0154_d964, 4), second: word(0x0154_d964, 8) }) };
    1
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_queue_text(request: *const TextRequest) {
    unsafe { copy_words(request as u32, 0x015e_64bc, 7) };
    while unsafe { Address32(0x015e_470d).byte() } == 0 { unsafe { xm1_pipeline_image_date_delay(5) }; }
    unsafe { xm1_pipeline_image_date_post(0x9000, 0, 0, 0) };
}

unsafe fn date_text(frame: u32, mode: u32, format: DateFormat) -> [Utf16Unit; 18] {
    let mut text = [Utf16Unit(0); 18];
    let source = |index: u32| unsafe { Address32(frame.wrapping_add(0x1684 + index)).byte() as i8 as i16 as u16 };
    for index in 0..10 { text[index] = Utf16Unit(source(index as u32)); }
    text[10] = Utf16Unit(source(10));
    for index in 11..17 { text[index] = Utf16Unit(source(index as u32 - 1)); }
    for index in [5, 8, 12] { if text[index].0 == 48 { text[index] = Utf16Unit(32); } }
    match format {
        DateFormat::YearMonthDay => { text[4] = Utf16Unit(46); text[7] = Utf16Unit(46); }
        DateFormat::MonthDayYear | DateFormat::DayMonthYear => {
            let (first, second, separator) = match format {
                DateFormat::MonthDayYear => (5, 8, 47), _ => (8, 5, 46),
            };
            let positions = [first, first + 1, 0, second, second + 1, 0, 0, 1, 2, 3];
            for index in 0..10 {
                text[index] = Utf16Unit(if index == 2 || index == 5 { separator } else { source(positions[index]) });
            }
            for index in [0, 3] { if text[index].0 == 48 { text[index] = Utf16Unit(32); } }
        }
    }
    if mode == 1 { text[10..17].fill(Utf16Unit(0)); }
    text
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_transform_output(frame: u32, plane: u32, alternate: u32, program: ProgramAddress) -> u32 {
    unsafe { xm1_pipeline_image_cache_maintenance() };
    let mut orientation = unsafe { word(frame, 0x1604) };
    if orientation == 0 { orientation = 1; }
    let enabled = unsafe { word(frame, 0x614) };
    let aspect = unsafe { word(frame, 0x4f0) };
    if enabled | unsafe { word(frame, 0x61c) } == 0 { return 1; }
    let (width, height) = if aspect <= 2 || alternate == 1 {
        let width = (unsafe { word(plane, 0x10) } >> 1) & 0xffff;
        (width as u16, (width * 2 / 3) as u16)
    } else if aspect == 3 {
        let height = unsafe { Address32(plane.wrapping_add(0x14)).half() };
        ((u32::from(height) * 3 / 2) as u16, height)
    } else { invalid(); };
    if enabled != 0 {
        let mut request = StampRequest { address: program, enabled: 1, width: PixelExtent(width), height: PixelExtent(height), first: PixelExtent(0), second: PixelExtent(0) };
        let mut text = unsafe { date_text(frame, enabled, DateFormat::parse(word(frame, 0x618))) };
        unsafe { write_volatile(0x0154_d89c as *mut StampRequest, request) };
        let mut result = StampResult { status: 0, first: 0, second: 0 };
        if unsafe { xm1_pipeline_image_prepare_text(&request, text.as_mut_ptr(), core::ptr::null_mut(), orientation, &mut result) } == 0 { invalid(); }
        request = unsafe { read_volatile(0x0154_d89c as *const StampRequest) };
        request.first = PixelExtent(result.first as u16);
        request.second = PixelExtent(result.second as u16);
        let applied = unsafe { xm1_pipeline_image_apply_stamp(plane, orientation, alternate, aspect, &request) };
        if applied == 0 { invalid(); }
        unsafe { xm1_pipeline_image_cache_maintenance() };
        applied
    } else {
        unsafe { xm1_pipeline_image_cache_maintenance() };
        1
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_apply_stamp(plane: u32, orientation: u32, alternate: u32, aspect: u32, request: *const StampRequest) -> u32 {
    let task = unsafe { xm1_pipeline_image_current_task() };
    unsafe { copy_words(request as u32, 0x0154_cee8, 4) };
    let factor = unsafe { xm1_buffers_configuration_byte(0x2dc) };
    let input = unsafe { read_volatile(request) };
    let width = unsafe { word(plane, 0x10) } >> 1;
    let height = unsafe { word(plane, 0x14) };
    let mut x = 0u32;
    let mut y = 0u32;
    if alternate == 1 {
        match aspect {
            1 => y = height.wrapping_sub(width.wrapping_mul(2) / 3) >> 1,
            2 => y = height.wrapping_sub(width.wrapping_mul(9) >> 4) >> 1,
            3 => x = width.wrapping_sub(height) >> 1,
            _ => (),
        }
    }
    let horizontal = orientation == 1 || orientation == 3;
    let correction = if horizontal { (u32::from(input.height.0) * 28 + 500) / 1000 } else { (u32::from(input.width.0) * 21 + 500) / 1000 };
    let correction = correction.wrapping_mul(factor).wrapping_add(50) / 100;
    let fw = u32::from(input.first.0);
    let fh = u32::from(input.second.0);
    let (other_x, other_y);
    if horizontal {
        x = x.wrapping_add(width.wrapping_mul(9).wrapping_add(50) / 100);
        y = y.wrapping_add(height.wrapping_mul(72).wrapping_add(500) / 1000);
        if orientation == 1 {
            x = width.wrapping_sub(x);
            other_x = x.wrapping_sub(fw);
            y = height.wrapping_sub(y);
            if input.enabled != 1 { y = y.wrapping_sub(correction); }
            other_y = y.wrapping_sub(fh);
        } else {
            other_x = x.wrapping_add(fw);
            if input.enabled != 1 { y = y.wrapping_add(correction); }
            other_y = y.wrapping_add(fh);
        }
    } else {
        x = x.wrapping_add(width.wrapping_mul(72).wrapping_add(500) / 1000);
        y = y.wrapping_add(height.wrapping_mul(9).wrapping_add(50) / 100);
        if orientation == 2 {
            x = width.wrapping_sub(x);
            if input.enabled != 1 { x = x.wrapping_sub(correction); }
            other_x = x.wrapping_sub(fh);
            other_y = y.wrapping_add(fw);
        } else {
            if input.enabled != 1 { x = x.wrapping_add(correction); }
            other_x = x.wrapping_add(fh);
            y = height.wrapping_sub(y);
            other_y = y.wrapping_sub(fw);
        }
    }
    let stride = (unsafe { word(plane, 0xc) } >> 1) as u16;
    let packed = [input.address.0, unsafe { word(plane, 4) }, u32::from(stride) | (fw << 16), fh | (fw << 16), orientation.wrapping_mul(90).wrapping_sub(90), x.min(other_x), x.max(other_x), y.min(other_y), y.max(other_y)];
    for (index, value) in packed.into_iter().enumerate() { unsafe { store(0x0154_cd60, index as u32 * 4, value) }; }
    unsafe { xm1_pipeline_image_configure_task(task, 12) };
    unsafe { xm1_pipeline_image_stamp_rectangle(packed[0], packed[1], packed[2], packed[3], packed[4], packed[5], packed[6], packed[7], packed[8]) };
    unsafe { xm1_pipeline_image_configure_task(task, 6) };
    1
}

#[derive(Clone, Copy)]
#[repr(C)]
pub struct PackedStamp {
    pub source: u32, pub destination: u32,
    pub source_width: u32, pub source_height: u32,
    pub width: u32, pub height: u32,
    pub source_stride: u32, pub destination_stride: u32,
    pub source_top: u32, pub reserved_24: u32, pub source_left: u32,
    pub reserved_2c: u32, pub scale: u32, pub rotation: u32,
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_stamp_rectangle(source: u32, destination: u32, dimensions: u32, heights: u32, rotation: u32, left: u32, right: u32, top: u32, bottom: u32) -> u32 {
    let rotation = rotation % 360;
    let (width, height) = if rotation == 90 || rotation == 270 { (bottom.wrapping_sub(top), right.wrapping_sub(left)) } else { (right.wrapping_sub(left), bottom.wrapping_sub(top)) };
    let stride = dimensions & 0xffff;
    let request = PackedStamp {
        source, destination: destination.wrapping_add(top.wrapping_mul(stride).wrapping_mul(2)).wrapping_add(left.wrapping_mul(2)) & !3,
        source_width: dimensions >> 16, source_height: heights & 0xffff,
        width, height, source_stride: heights >> 16, destination_stride: stride,
        source_top: 0, reserved_24: 0, source_left: 0, reserved_2c: 0,
        scale: 128, rotation: 360 - rotation,
    };
    unsafe { xm1_pipeline_image_stamp_packed(&request) }
}

#[derive(Clone, Copy)]
struct ColorSample { alpha: u32, luma: u32, blue: u32, red: u32 }

unsafe fn sample_color(address: Address32) -> ColorSample {
    let alpha = u32::from(unsafe { address.byte() });
    let blue = i32::from(unsafe { address.offset(ByteOffset(1)).byte() });
    let green = i32::from(unsafe { address.offset(ByteOffset(2)).byte() });
    let red = i32::from(unsafe { address.offset(ByteOffset(3)).byte() });
    ColorSample {
        alpha,
        luma: ((blue * 11400 + green * 58700 + red * 29900) / 100000) as u32 & 255,
        blue: ((blue * 50000 - red * 16874 - green * 33126) / 100000 - 128) as u32 & 255,
        red: ((red * 50000 - blue * 8131 - green * 41869) / 100000 - 128) as u32 & 255,
    }
}

unsafe fn lookup_coordinate(table: Address32, coordinate: i32) -> u32 {
    (unsafe { table.offset(ByteOffset(((coordinate >> 10) as u32).wrapping_mul(4))).word() }) >> 10
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_stamp_packed(input: *const PackedStamp) -> u32 {
    let request = unsafe { read_volatile(input) };
    let sine = unsafe { xm1_pipeline_image_fixed10_sine_degrees(request.rotation) };
    let cosine = unsafe { xm1_pipeline_image_fixed10_cosine_degrees(request.rotation) };
    let width = request.width.wrapping_sub(1) as i32;
    let height = request.height.wrapping_sub(1) as i32;
    let corners = [(0i32, 0i32), (0, height), (width, height), (width, 0)];
    let (mut left, mut right, mut top, mut bottom) = (0i32, 0i32, 0i32, 0i32);
    for (x, y) in corners {
        let rotated_x = x.wrapping_mul(cosine).wrapping_sub(y.wrapping_mul(sine)) >> 10;
        let rotated_y = x.wrapping_mul(sine).wrapping_add(y.wrapping_mul(cosine)) >> 10;
        left = left.min(rotated_x);
        right = right.max(rotated_x);
        top = top.min(rotated_y);
        bottom = bottom.max(rotated_y);
    }
    let source = Address32(request.source.wrapping_add(
        request.source_top.wrapping_mul(request.source_stride).wrapping_add(request.source_left).wrapping_mul(4),
    ) & !0x4000_0000);
    let destination = Address32(request.destination & !0x4000_0000);
    unsafe { xm1_pipeline_packed_cache_read(source, request.source_height.wrapping_mul(request.source_stride).wrapping_mul(4)) };
    let x_table = Address32(0x0095_e1f8);
    let y_table = Address32(0x0096_21f8);
    for (table, extent, source_extent) in [
        (x_table, request.width, request.source_width),
        (y_table, request.height, request.source_height),
    ] {
        let bound = extent.wrapping_shl(10);
        let mut position = 0u32;
        let mut numerator = 0u32;
        let mut index = 0u32;
        while position < bound {
            unsafe { table.offset(ByteOffset(index.wrapping_mul(4))).write_word(numerator / extent) };
            numerator = numerator.wrapping_add(source_extent.wrapping_shl(10));
            position = position.wrapping_add(1024);
            index = index.wrapping_add(1);
        }
    }
    let pairs = (right.wrapping_sub(left).wrapping_add(1) as u32) >> 1;
    let rows = bottom.wrapping_sub(top).wrapping_add(1) as u32;
    let mut row_x = left.wrapping_mul(cosine).wrapping_add(top.wrapping_mul(sine));
    let mut row_y = left.wrapping_neg().wrapping_mul(sine).wrapping_add(top.wrapping_mul(cosine));
    let mut target_row = destination;
    for _ in 0..rows {
        let mut x = row_x;
        let mut y = row_y;
        let mut target = target_row;
        for _ in 0..pairs {
            let first_x = unsafe { lookup_coordinate(x_table, x) };
            let first_y = unsafe { lookup_coordinate(y_table, y) };
            let second_x = unsafe { lookup_coordinate(x_table, x.wrapping_add(cosine)) };
            let second_y = unsafe { lookup_coordinate(y_table, y.wrapping_sub(sine)) };
            let source_offset = |sx: u32, sy: u32| ByteOffset(sy.wrapping_mul(request.source_stride).wrapping_add(sx).wrapping_mul(4));
            let first = unsafe { sample_color(source.offset(source_offset(first_x, first_y))) };
            let second = if first_x == second_x && first_y == second_y {
                first
            } else { unsafe { sample_color(source.offset(source_offset(second_x, second_y))) } };
            let alpha = (first.alpha + second.alpha) >> 1;
            let packed = first.luma | (second.luma << 8) | (((first.blue + second.blue) >> 1) << 16) | (((first.red + second.red) >> 1) << 24);
            if alpha == 255 {
                unsafe { target.write_word(packed) };
            } else if alpha != 0 {
                for (index, coverage) in [first.alpha, second.alpha, alpha, alpha].into_iter().enumerate() {
                    let byte = target.offset(ByteOffset(index as u32));
                    let old = u32::from(unsafe { byte.byte() });
                    let value = (packed >> (index * 8)) & 255;
                    unsafe { byte.write_byte(((old * (255 - coverage) + value * coverage) >> 8) as u8) };
                }
            }
            target = target.offset(ByteOffset(4));
            x = x.wrapping_add(cosine.wrapping_mul(2));
            y = y.wrapping_sub(sine.wrapping_mul(2));
        }
        row_x = row_x.wrapping_add(sine);
        row_y = row_y.wrapping_add(cosine);
        target_row = target_row.offset(ByteOffset(request.destination_stride.wrapping_mul(2)));
    }
    unsafe { xm1_pipeline_packed_cache_write(destination, request.height.wrapping_mul(request.destination_stride).wrapping_mul(4)) }
}

#[derive(Clone, Copy)]
enum GlyphEncoding { Stamp, Wide }

#[derive(Clone, Copy)]
#[repr(C)]
pub struct TextMetrics { pub width: u32, pub height: u32, pub advance: u32 }

#[repr(C)]
pub struct RasterBuffer { pub capacity: u32, pub address: u32, pub x: u32, pub y: u32 }


unsafe fn encode_glyphs(input: *const Utf16Unit, destination: *mut Utf16Unit, encoding: GlyphEncoding) {
    let mut index = 0usize;
    loop {
        let unit = unsafe { read_volatile(input.add(index)).0 };
        match encoding {
            GlyphEncoding::Stamp => {
                for row in 0..15 {
                    let source = Address32(0x0117_93a4 + row * 4);
                    if i32::from(unsafe { source.half() }) == i32::from(unit as i16) {
                        let glyph = unsafe { source.offset(ByteOffset(2)).half() };
                        unsafe { write_volatile(destination.add(index), Utf16Unit(glyph)) };
                        break;
                    }
                }
            }
            GlyphEncoding::Wide => unsafe { write_volatile(destination.add(index), Utf16Unit(if unit < 256 { unit.wrapping_add(0xe100) } else { unit })) },
        }
        if unit == 0 { break; }
        index += 1;
    }
}

unsafe fn render_text(request: TextRequest, metrics: &mut TextMetrics) -> u32 {
    let encoding = if request.metadata != 0 { GlyphEncoding::Stamp } else { GlyphEncoding::Wide };
    let bank = unsafe { Address32(0x015e_67a4).byte() };
    let text = (0x015e_67a6u32 + u32::from(bank) * 66) as *mut Utf16Unit;
    unsafe { encode_glyphs(request.first_text, text, encoding) };
    for offset in (0..0x34).step_by(4) { unsafe { store(0x015e_6708, offset, 0) }; }
    let advance = (((request.second_percent & 0xffff) * 5 + 50) / 100).max(1);
    unsafe {
        store(0x015e_6708, 0, request.first_percent & 0xffff);
        store(0x015e_6708, 4, request.second_percent & 0xffff);
        store(0x015e_6708, 8, 1);
        store(0x015e_6708, 0xc, advance);
        store(0x015e_6708, 0x10, word(0x0111_3bcc, 0));
        store(0x015e_6708, 0x14, 0);
        store(0x015e_6708, 0x18, 0x9900_0000);
    }
    if unsafe { xm1_pipeline_image_font_configure(0x015e_6708 as *const u32) } == 0 { return 0; }
    unsafe {
        store(0x015e_673c, 0, 0);
        store(0x015e_673c, 4, 0);
        store(0x015e_673c, 8, 0);
    }
    if unsafe { xm1_pipeline_image_font_metrics(text, 0x015e_673c as *mut TextMetrics) } == 0 { return 0; }
    metrics.width = u32::from(unsafe { Address32(0x015e_673c).half() });
    metrics.height = u32::from(unsafe { Address32(0x015e_6740).half() });
    if request.address.0 == 0 { return 0; }
    let raster = RasterBuffer { capacity: metrics.width.wrapping_mul(metrics.height).wrapping_mul(4), address: request.address.0, x: 0, y: 0 };
    if unsafe { xm1_pipeline_image_font_rasterize(text, &raster, 0) } == 0 { return 0; }
    unsafe { Address32(0x015e_67a4).write_byte(bank.wrapping_add(1)) };
    request.address.0
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_measure_text(request: *const TextRequest, output: *mut StampResult) {
    if request.is_null() || output.is_null() { return; }
    let request = unsafe { read_volatile(request) };
    if request.first_text.is_null() || request.address.0 == 0 { return; }
    let mut metrics = TextMetrics { width: 0, height: 0, advance: 0 };
    let rendered = unsafe { render_text(request, &mut metrics) };
    unsafe { write_volatile(output, if rendered == 0 || metrics.width == 0 || metrics.height == 0 {
        StampResult { status: 0, first: 0, second: 0 }
    } else { StampResult { status: 1, first: metrics.width, second: metrics.height } }) };
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_publish_text() {
    let mut output = StampResult { status: 0, first: 0, second: 0 };
    let request = 0x015e_64bc as *mut TextRequest;
    let mut value = unsafe { read_volatile(request) };
    if !(value.first_text.is_null() && value.second_text.is_null()) && value.address.0 != 0 && value.first_percent != 0 && value.second_percent != 0 {
        if value.metadata == 0 {
            if unsafe { xm1_property_get(0x34, 0, 0, 0) } == 0 && !value.second_text.is_null() && unsafe { read_volatile(value.second_text).0 } != 0 {
                value.first_text = value.second_text;
                unsafe { write_volatile(core::ptr::addr_of_mut!((*request).first_text), value.first_text) };
            }
            unsafe { xm1_pipeline_image_measure_text(request, &mut output) };
        } else if !value.first_text.is_null() {
            unsafe { xm1_pipeline_image_measure_text(request, &mut output) };
        }
    }
    let completion = unsafe { read_volatile(core::ptr::addr_of!((*request).completion)) };
    if completion != 0 {
        let callback: unsafe extern "C" fn(u32, u32, u32) = unsafe { core::mem::transmute(completion) };
        unsafe { callback(output.status, output.first, output.second) };
    }
}


#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_date_notify(first: u32, second: u32, third: u32) {
    unsafe {
        store(0x0154_d964, 0, first);
        store(0x0154_d964, 4, second);
        store(0x0154_d964, 8, third);
        xm1_pipeline_image_send_notification(word(0x0155_ccac, 0x14), 0x10);
    }
}

#[repr(C)]
pub struct QueueMessage {
    pub code: u16, pub sender: u8, pub destination: u8,
    pub length: u32, pub payload: u32,
}

unsafe extern "C" {
    fn xm1_pipeline_image_send_message(channel: u32, message: *const QueueMessage) -> i32;
    fn xm1_pipeline_image_storage_dispatch_error(status: i32);
    fn xm1_pipeline_image_date_dispatch_trace(operation: u32, object: u32, status: i32);
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_date_delay(ticks: u32) -> i32 {
    let table = unsafe { super::image_planes::interface(super::image_planes::current_interface()) };
    if ticks == 0 {
        let endpoint: unsafe extern "C" fn(u32) = unsafe { core::mem::transmute(table.offset(ByteOffset(0x11c)).word()) };
        unsafe { endpoint(0) };
        return 0;
    }
    let endpoint: unsafe extern "C" fn(u32) -> i32 = unsafe { core::mem::transmute(table.offset(ByteOffset(0x4c)).word()) };
    let status = unsafe { endpoint(ticks) };
    if status != 0 {
        unsafe {
            xm1_pipeline_image_date_dispatch_trace(9, ticks, status);
            Address32(0x40a0_f82c).write_word(status as u32);
            xm1_pipeline_image_storage_dispatch_error(status);
        }
    }
    status
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_date_send(packet: u32) {
    let message = QueueMessage { code: 0x3100, sender: unsafe { xm1_pipeline_image_current_task() } as u8, destination: 0x21, length: 12, payload: packet };
    for _ in 0..10 {
        let status = unsafe { xm1_pipeline_image_send_message(0x21, &message) };
        if status != -18 && status != -62 { break; }
        unsafe { xm1_pipeline_image_date_delay(100) };
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_date_post(event: u32, length: u32, data: u32, sequence: u32) {
    let prior = unsafe { Address32(0x014e_32b8).half() };
    if sequence == 1 {
        if u32::from(prior) == event && unsafe { Address32(0x014e_32bc).byte() } != unsafe { Address32(0x014e_3338).byte() } { return; }
        unsafe { Address32(0x014e_32b8).write_half(event as u16) };
    } else { unsafe { Address32(0x014e_32b8).write_half(0) }; }
    let mut packet = [0u32; 3];
    packet[0] = event & 0xffff;
    packet[1] = length;
    packet[2] = 0;
    if length == 0 || data != 0 {
        let destination = core::ptr::addr_of_mut!(packet[2]) as *mut u8;
        for offset in 0..length {
            let value = unsafe { Address32(data.wrapping_add(offset)).byte() };
            unsafe { write_volatile(destination.wrapping_add(offset as usize), value) };
        }
    }
    let selected = match sequence {
        1 => {
            let value = unsafe { Address32(0x014e_32bc).byte() };
            let next = if value == 255 { 1 } else { value + 1 };
            unsafe { Address32(0x014e_32bc).write_byte(next) };
            u32::from(next)
        }
        2 => u32::from(unsafe { Address32(0x014e_32bc).byte() }),
        _ => 0,
    };
    packet[0] |= selected << 16;
    unsafe { Address32(0x014e_32ba).write_half(event as u16) };
    unsafe { xm1_pipeline_image_date_send(packet.as_ptr() as u32) };
}

#[derive(Clone, Copy)]
#[repr(transparent)]
struct FontHandle(u32);

#[derive(Clone, Copy)]
#[repr(u32)]
enum FontProperty {
    Width = 1, Height = 2, Background = 3, Italic = 4, Underline = 5,
    EdgeMode = 20, EdgeWidth = 21, UnderlinePosition = 23,
    UnderlineWidth = 24, PixelFormat = 40, Direction = 42, Format = 47,
}


unsafe fn set_font_property(handle: FontHandle, key: FontProperty, value: u32) -> i32 {
    unsafe { xm1_pipeline_image_font_set_option(handle.0, key as u32, value) }
}

unsafe fn font_configuration_error(message: u32) -> u32 {
    unsafe { xm1_pipeline_image_font_error(message) };
    0
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_configure(configuration: *const u32) -> u32 {
    let handle = FontHandle(unsafe { word(0x0166_5a38, 4) });
    if handle.0 == 0 { return unsafe { font_configuration_error(0x0117_f0f4) }; }
    let base = configuration as u32;
    let width = unsafe { word(base, 0) };
    let height = unsafe { word(base, 4) };
    if width == 0 || width > 100 { return unsafe { font_configuration_error(0x0117_f114) }; }
    if height == 0 || height > 100 { return unsafe { font_configuration_error(0x0117_f134) }; }
    let mode = unsafe { word(base, 8) };
    let advance = unsafe { word(base, 12) };
    if (mode == 0 && advance != 0) || (mode == 1 && advance == 0) || advance > 5 {
        return unsafe { font_configuration_error(0x0117_f154) };
    }
    let underline = unsafe { word(base, 36) };
    let underline_width = unsafe { word(base, 44) };
    if (underline == 0 && underline_width != 0) || (underline == 1 && underline_width == 0) {
        return unsafe { font_configuration_error(0x0117_f17c) };
    }
    let underline_position = unsafe { word(base, 40) };
    if height < advance.wrapping_add(underline_width).wrapping_add(underline_position) {
        return unsafe { font_configuration_error(0x0117_f1a0) };
    }
    let italic = unsafe { word(base, 32) };
    if italic == 1 && (advance.wrapping_add(width) > 48 || advance.wrapping_add(height) > 48) {
        return unsafe { font_configuration_error(0x0117_f1c8) };
    }
    let mut status = unsafe { set_font_property(handle, FontProperty::Width, width) };
    for (property, value) in [
        (FontProperty::Height, height),
        (FontProperty::Italic, italic),
        (FontProperty::Underline, underline),
        (FontProperty::UnderlinePosition, underline_position),
        (FontProperty::UnderlineWidth, underline_width),
        (FontProperty::Format, unsafe { word(base, 48) }),
        (FontProperty::PixelFormat, 1),
        (FontProperty::Direction, if unsafe { word(0x0166_5a38, 0xf0) } == 0 { 0x200f } else { 0x200e }),
    ] {
        status = status.wrapping_add(unsafe { set_font_property(handle, property, value) });
    }
    if mode == 0 {
        for (property, value) in [
            (FontProperty::EdgeMode, 0), (FontProperty::EdgeWidth, 0),
            (FontProperty::Background, unsafe { word(base, 28) }),
        ] { status = status.wrapping_add(unsafe { set_font_property(handle, property, value) }); }
        unsafe {
            store(0x0166_5a38, 0xf4, word(base, 16));
            store(0x0166_5a38, 0xf8, 0);
            store(0x0166_5a38, 0xfc, word(base, 20));
        }
    } else {
        for (property, value) in [
            (FontProperty::Background, 0), (FontProperty::EdgeMode, 1),
            (FontProperty::EdgeWidth, advance),
        ] { status = status.wrapping_add(unsafe { set_font_property(handle, property, value) }); }
        unsafe {
            store(0x0166_5a38, 0xf4, word(base, 24));
            store(0x0166_5a38, 0xf8, word(base, 16));
            store(0x0166_5a38, 0xfc, word(base, 20));
        }
    }
    if status != 0 { unsafe { font_configuration_error(0x0117_f1ec) } } else { 1 }
}

#[derive(Clone, Copy)]
#[repr(u32)]
enum BackendOption {
    Scale = 0, Width = 1, Height = 2, EdgeMode = 3, EdgeWidth = 4,
    Spacing = 5, Tracking = 6, Slant = 7, Weight = 8, PixelFormat = 9,
    Format = 10, Fill = 11, RasterMode = 12,
}

unsafe fn backend_option(handle: u32, option: BackendOption, value: u32) {
    if matches!(option, BackendOption::Format) {
        unsafe { store(handle, 0x1068, value) };
        return;
    }
    if (value as i32) < 0 { return; }
    let mode = unsafe { word(handle, 0xff4) };
    if mode == 2 || mode == 6 || mode == 7 {
        unsafe { store(handle, 0x106c, if mode == 7 { 512 } else { 256 }) };
        if matches!(option, BackendOption::RasterMode) {
            unsafe { store(handle, 0x1078, if value == 0 { 4 } else { value }) };
        }
    }
    if !(3..=5).contains(&mode) { return; }
    let (offset, value) = match option {
        BackendOption::Scale => (0x1038, value),
        BackendOption::Width => (0x103c, value.min(100)),
        BackendOption::Height => (0x1040, value.min(100)),
        BackendOption::EdgeMode => (0x1044, if value <= 5 { value } else { 0 }),
        BackendOption::EdgeWidth => (0x1048, value.min(5)),
        BackendOption::Spacing => (0x104c, value),
        BackendOption::Tracking => (0x1050, value),
        BackendOption::Slant => (0x1054, value),
        BackendOption::Weight => (0x1058, value),
        BackendOption::PixelFormat => {
            let value = if value <= 2 { value } else { 1 };
            unsafe {
                store(handle, 0x106c, if value == 0 { 256 } else { 512 });
                store(handle, 0xff4, value + 3);
            }
            (0x105c, value)
        }
        BackendOption::Fill => (0x1074, if value <= 1 { value } else { 0 }),
        BackendOption::RasterMode => {
            unsafe { store(handle, 0x1078, if value == 0 { 4 } else { value }) };
            return;
        }
        BackendOption::Format => return,
    };
    let old = unsafe { word(handle, offset) };
    unsafe { store(handle, offset, value) };
    if old != value { unsafe { store(handle, 0x1070, 1) }; }
}

unsafe fn font_limit(object: u32) -> i32 {
    let reference = unsafe { word(object, 8) };
    let workspace = unsafe { word(reference, 0x70) };
    (unsafe { word(workspace, 12) }) as i32
}

unsafe fn font_auto_weight(object: u32, enabled: u32) {
    let (mode, width) = if enabled == 0 { (0, 0) } else {
        let sum = unsafe { word(object, 0x14) }.wrapping_add(unsafe { word(object, 0x18) }) as i32;
        let adjusted = sum.wrapping_add((sum as u32 >> 31) as i32);
        let average = adjusted >> 1;
        if average <= 24 { (3, (average / 12) as u32) }
        else if average <= 32 { (5, 1) }
        else { (2, (adjusted >> 6) as u32) }
    };
    unsafe {
        store(object, 0x1c, mode);
        store(object, 0x20, width);
        backend_option(word(object, 0), BackendOption::EdgeMode, mode);
        backend_option(word(object, 0), BackendOption::EdgeWidth, width);
    }
}

unsafe fn font_spacing(object: u32, enabled: u32) {
    let height = (unsafe { word(object, 0x18) }) as i32;
    let spacing = (if enabled == 0 { 0 } else { height / 2 }) as u32;
    unsafe { store(object, 0x24, spacing) };
    let area = height.wrapping_mul(spacing.wrapping_add(unsafe { word(object, 0x14) }) as i32);
    if area > unsafe { font_limit(object) } { return; }
    unsafe { backend_option(word(object, 0), BackendOption::Spacing, spacing); store(object, 0x30, spacing); }
}

unsafe fn font_padding(object: u32, enabled: u32, leading: bool) {
    let height = (unsafe { word(object, 0x18) }) as i32;
    let extent = (if enabled == 0 { 0 } else if height > 15 { height >> 4 } else { 1 }) as u32;
    if leading {
        unsafe {
            store(object, 0x38, extent);
            store(object, 0x34, (if enabled == 0 { 0 } else { height.wrapping_mul(2) / 5 }) as u32);
            store(object, 0x3c, word(object, 0x24));
        }
    } else {
        unsafe { store(object, 0x2c, extent); store(object, 0x28, 0); store(object, 0x30, word(object, 0x24)); }
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_set_option(object: u32, key: u32, value: u32) -> i32 {
    if object == 0 { return -2; }
    let mode = unsafe { word(object, 12) };
    if mode == 2 && !matches!(key, 42 | 46 | 47) { return -4; }
    let outline = mode == 1;
    let handle = unsafe { word(object, 0) };
    match key {
        0 if value == 0 => unsafe { store(object, 16, 0); backend_option(handle, BackendOption::Scale, 0); },
        1 | 2 if value <= 100 => {
            let other = unsafe { word(object, if key == 1 { 24 } else { 20 }) };
            let area = if key == 1 { other.wrapping_mul(value.wrapping_add(48)) } else { other.wrapping_add(48).wrapping_mul(value) };
            if (area as i32) > unsafe { font_limit(object) } { return -15; }
            unsafe {
                store(object, if key == 1 { 20 } else { 24 }, value);
                backend_option(handle, if key == 1 { BackendOption::Width } else { BackendOption::Height }, value);
            }
            if !outline {
                unsafe { font_auto_weight(object, word(object, 0x70)) };
                if key == 2 {
                    unsafe {
                        font_spacing(object, word(object, 0x74));
                        font_padding(object, word(object, 0x78), false);
                        font_padding(object, word(object, 0x7c), true);
                    }
                }
            }
        }
        3..=6 if !outline => {
            let enabled = u32::from(value != 0);
            unsafe { store(object, 0x70 + (key - 3) * 4, enabled) };
            match key {
                3 => unsafe { font_auto_weight(object, enabled) },
                4 => unsafe { font_spacing(object, enabled) },
                5 => unsafe { font_padding(object, enabled, false) },
                _ => unsafe { font_padding(object, enabled, true) },
            }
        }
        20 if value <= 5 && (!outline || !(3..=5).contains(&value)) => unsafe {
            store(object, 28, value); backend_option(handle, BackendOption::EdgeMode, value);
        },
        21 if value <= 5 => unsafe { store(object, 32, value); backend_option(handle, BackendOption::EdgeWidth, value); },
        22 if !outline && value <= 48 && (value as i32) <= (unsafe { word(object, 20) }) as i32 => {
            let area = unsafe { word(object, 24) }.wrapping_mul(value.wrapping_add(unsafe { word(object, 20) }));
            if (area as i32) > unsafe { font_limit(object) } { return -15; }
            unsafe { store(object, 36, value); backend_option(handle, BackendOption::Spacing, value); }
        }
        23 | 24 | 26 | 27 if (value as i32) >= 0 => {
            let (destination, opposite) = match key { 23 => (40, 44), 24 => (44, 40), 26 => (52, 56), _ => (56, 52) };
            let available = unsafe { word(object, 24) }.wrapping_sub(unsafe { word(object, opposite) }) as i32;
            if (value as i32) >= available { return -4; }
            unsafe { store(object, destination, value) };
        }
        25 | 28 if !outline && (value as i32) >= 0 && (value as i32) <= (unsafe { word(object, 36) }) as i32 => unsafe {
            store(object, if key == 25 { 48 } else { 60 }, value);
        },
        40 if !outline && value <= 2 && (mode == 5 || (mode == 4 && value <= 1) || (mode == 3 && value == 0)) => unsafe {
            store(object, 64, value);
            backend_option(handle, BackendOption::PixelFormat, word(0x0118_59dc, 0x50 + value * 4));
        },
        41 if value <= 3600 => unsafe { store(object, 88, value) },
        42 if value == 0 || value.wrapping_sub(0x200e) <= 1 => unsafe { store(object, 92, value) },
        43 if outline && value.wrapping_sub(1) <= 1 => unsafe { store(object, 96, value) },
        45 if !outline && value == 0 => unsafe { store(object, 100, 0); backend_option(handle, BackendOption::Fill, 0); },
        46 if !outline => unsafe {
            store(object, 104, if (value as i32) < 0 { 4 } else { value });
            backend_option(handle, BackendOption::RasterMode, value);
        },
        47 => unsafe { store(object, 108, value); backend_option(handle, BackendOption::Format, value); },
        _ => return -4,
    }
    0
}

unsafe extern "C" {
    fn xm1_pipeline_image_font_draw(handle: u32, text: *const Utf16Unit, count: u32, mode: u32, x: u32, y: u32, first: u32, second: u32, foreground: u32, background: u32, edge: u32, scratch: u32, state: u32) -> i32;
}

unsafe fn utf16_length(text: *const Utf16Unit) -> u32 {
    let mut count = 0u32;
    while unsafe { xm1_pipeline_image_utf16_load((text as u32).wrapping_add(count.wrapping_mul(2))) } != 0 {
        count = count.wrapping_add(1);
    }
    count
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_metrics(text: *const Utf16Unit, metrics: *mut TextMetrics) -> u32 {
    if text.is_null() { return unsafe { font_configuration_error(0x0117_f0a4) }; }
    if unsafe { read_volatile(text).0 } == 0 { return 1; }
    let handle = unsafe { word(0x0166_5a38, 4) };
    if handle == 0 { return unsafe { font_configuration_error(0x0117_f0b8) }; }
    let count = unsafe { utf16_length(text) };
    let result = unsafe { xm1_pipeline_image_font_query(handle, text, count, 0, 0, 0x0166_5b38) };
    if result < 0 { return unsafe { font_configuration_error(0x0117_f0d8) }; }
    unsafe {
        write_volatile(metrics, TextMetrics {
            width: word(0x0166_5b38, 0x10d4),
            height: word(0x0166_5b38, 0x10d8),
            advance: word(0x0166_5b38, 0x10e4),
        });
    }
    1
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_rasterize(text: *const Utf16Unit, buffer: *const RasterBuffer, mode: u32) -> u32 {
    let handle = unsafe { word(0x0166_5a38, 4) };
    if handle == 0 { return unsafe { font_configuration_error(0x0117_efd0) }; }
    if text.is_null() { return unsafe { font_configuration_error(0x0117_eff0) }; }
    if unsafe { read_volatile(text).0 } == 0 { return 1; }
    let buffer = unsafe { read_volatile(buffer) };
    if buffer.address == 0 { return unsafe { font_configuration_error(0x0117_f004) }; }
    let count = unsafe { utf16_length(text) };
    unsafe { xm1_pipeline_image_font_query(handle, text, count, 0, 0, 0x0166_5b38) };
    let width = unsafe { word(0x0166_5b38, 0x10d4) }.wrapping_add(buffer.x);
    let height = unsafe { word(0x0166_5b38, 0x10d8) }.wrapping_add(buffer.y);
    if buffer.capacity < width.wrapping_mul(height).wrapping_mul(4) {
        return unsafe { font_configuration_error(0x0117_f01c) };
    }
    unsafe {
        store(0x0166_59c8, 0, buffer.address);
        store(0x0166_59c8, 4, 0x0166_5b38);
        store(0x0166_59c8, 8, 0);
        store(0x0166_59c8, 12, buffer.x);
        store(0x0166_59c8, 16, buffer.y);
    }
    let result = unsafe {
        xm1_pipeline_image_font_draw(handle, text, count, mode, 0, 0, 0, 0, 0x0166_5b2c, 0x0166_5b34, 0x0166_5b30, 0x0166_5938, 0x0166_59c8)
    };
    if result < 0 { unsafe { font_configuration_error(0x0117_f038) } } else { 1 }
}

unsafe extern "C" {
    fn xm1_pipeline_image_font_prepare_outline(object: u32, text: *const Utf16Unit, count: u32, flags: u32, output: *mut u32) -> i32;
    fn xm1_pipeline_image_font_prepare_bitmap(object: u32, text: *const Utf16Unit, count: u32, flags: u32, output: *mut u32, buffer: u32, stride: u32, rows: u32, x: u32, y: u32) -> i32;
    fn xm1_pipeline_image_font_copy_memory(destination: u32, source: u32, count: u32);
}


#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_query(object: u32, text: *const Utf16Unit, count: u32, rectangle: u32, stride: u32, details: u32) -> i32 {
    if object == 0 { return -2; }
    if text.is_null() || (count as i32) <= 0 { return -9; }
    let outline = unsafe { word(object, 12) } == 1;
    let mut layout = 0;
    let result = if outline {
        unsafe { xm1_pipeline_image_font_prepare_outline(object, text, count, 1, &mut layout) }
    } else { unsafe { xm1_pipeline_image_font_prepare_bitmap(object, text, count, 1, &mut layout, 0, 0, 0, 0, 0) } };
    if result < 0 { return result; }
    let (width, height, pitch);
    if outline {
        let source_width = (unsafe { word(layout, 16) }) as i32;
        let source_height = (unsafe { word(layout, 20) }) as i32;
        width = (if source_width == 0 { 0 } else {
            ((unsafe { word(object, 20) }) as i32).wrapping_mul(source_width).wrapping_add(2047) / 2048
        }) as u32;
        height = (if source_height == 0 { 0 } else {
            ((unsafe { word(object, 24) }) as i32).wrapping_mul(source_height).wrapping_add(2047) / 2048
        }) as u32;
        pitch = 0;
    } else {
        pitch = unsafe { word(layout, 24) };
        width = unsafe { word(layout, 16) };
        height = unsafe { word(layout, 20) };
    }
    if rectangle != 0 {
        unsafe { store(rectangle, 0, 0); store(rectangle, 4, 0); store(rectangle, 8, width); store(rectangle, 12, height); }
    }
    if stride != 0 { unsafe { Address32(stride).write_word(pitch) }; }
    if details != 0 {
        let entries = unsafe { word(layout, 0) };
        unsafe {
            store(details, 0, entries);
            xm1_pipeline_image_font_copy_memory(details.wrapping_add(4), word(layout, 4), entries.wrapping_mul(2));
            Address32(details.wrapping_add(0x68)).write_half(Address32(layout.wrapping_add(12)).half());
            xm1_pipeline_image_font_copy_memory(details.wrapping_add(if outline { 0x6c } else { 0xcec }), word(layout, 8), entries.wrapping_mul(if outline { 64 } else { 20 }));
            store(details, 0x10d4, width);
            store(details, 0x10d8, height);
        }
        if !outline {
            unsafe { store(details, 0x10dc, pitch); store(details, 0x10e0, word(layout, 28)); store(details, 0x10e4, word(layout, 32)); }
        }
    }
    if outline {
        let result = unsafe { xm1_pipeline_image_font_prepare_outline(object, text, count, 4, &mut layout) };
        if result < 0 { return result; }
    }
    0
}

#[derive(Clone, Copy)]
#[repr(C)]
pub struct GlyphBitmap {
    pub width: u32, pub height: u32, pub stride: u32, pub format: u32,
    pub pixel_format: u32, pub fill: u32, pub foreground: u32, pub background: u32,
    pub length: u32, pub coverage: u32,
}

#[derive(Clone, Copy)]
#[repr(C)]
pub struct GlyphRasterState {
    pub destination: u32, pub metrics: u32, pub passes: u32, pub x: u32, pub y: u32,
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_blend_glyph(_mode: u32, x: u32, y: u32, _first: u32, _second: u32, bitmap: *const GlyphBitmap, state: *mut GlyphRasterState) -> i32 {
    let bitmap = unsafe { read_volatile(bitmap) };
    let old_state = unsafe { read_volatile(state) };
    let passes = old_state.passes.wrapping_add(1);
    unsafe { write_volatile(core::ptr::addr_of_mut!((*state).passes), passes) };
    let left = x.wrapping_add(old_state.x);
    let top = y.wrapping_add(old_state.y);
    let pitch = unsafe { word(old_state.metrics, 0x10d4) };
    let remainder = (bitmap.width as i32) % 4;
    let padding = (if remainder > 0 { 4 - remainder } else { remainder }) as u32;
    let palette = Address32(0x0166_6ffc);
    if unsafe { word(0x0166_703c, 0) } != bitmap.background || unsafe { word(0x0166_7044, 0) } != bitmap.foreground {
        for coverage in 0..16u32 {
            let mut color = 0u32;
            for channel in 0..4u32 {
                let background = ((bitmap.background >> (channel * 8)) & 255) as i32;
                let foreground = ((bitmap.foreground >> (channel * 8)) & 255) as i32;
                let value = if coverage == 15 { foreground } else {
                    background + ((foreground - background) * coverage as i32 >> 4)
                };
                color |= ((value as u32) & 255) << ((3 - channel) * 8);
            }
            unsafe { palette.offset(ByteOffset(coverage * 4)).write_word(color) };
        }
        unsafe { store(0x0166_703c, 0, bitmap.background); store(0x0166_7044, 0, bitmap.foreground); }
    }
    let columns = pitch.wrapping_sub(left) as i32;
    let mut destination = Address32(old_state.destination.wrapping_add(pitch.wrapping_mul(top).wrapping_mul(4)));
    let mut source = Address32(bitmap.coverage);
    for _ in 0..(bitmap.height as i32).max(0) {
        destination = destination.offset(ByteOffset(left.wrapping_mul(4)));
        if columns > 0 {
            for column in 0..columns as u32 {
                let coverage = unsafe { source.offset(ByteOffset(column)).byte() };
                if passes == 1 || coverage != 0 {
                    let pixel = unsafe { palette.offset(ByteOffset(u32::from(coverage) * 4)).word() };
                    unsafe { destination.offset(ByteOffset(column * 4)).write_word(pixel) };
                }
            }
            source = source.offset(ByteOffset(columns as u32));
            destination = destination.offset(ByteOffset((columns as u32).wrapping_mul(4)));
        }
        source = source.offset(ByteOffset(padding.wrapping_add(left)));
    }
    0
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_backend_option(handle: u32, key: u32, value: u32) {
    let option = match key {
        0 => BackendOption::Scale, 1 => BackendOption::Width,
        2 => BackendOption::Height, 3 => BackendOption::EdgeMode,
        4 => BackendOption::EdgeWidth, 5 => BackendOption::Spacing,
        6 => BackendOption::Tracking, 7 => BackendOption::Slant,
        8 => BackendOption::Weight, 9 => BackendOption::PixelFormat,
        10 => BackendOption::Format, 11 => BackendOption::Fill,
        12 => BackendOption::RasterMode,
        _ => {
            if (value as i32) >= 0 {
                match unsafe { word(handle, 0xff4) } {
                    2 | 6 => unsafe { store(handle, 0x106c, 256) },
                    7 => unsafe { store(handle, 0x106c, 512) },
                    _ => (),
                }
            }
            return;
        }
    };
    unsafe { backend_option(handle, option, value) };
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_auto_weight(object: u32, enabled: u32) {
    unsafe { font_auto_weight(object, enabled) };
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_spacing(object: u32, enabled: u32) {
    unsafe { font_spacing(object, enabled) };
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_trailing_padding(object: u32, enabled: u32) {
    unsafe { font_padding(object, enabled, false) };
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_leading_padding(object: u32, enabled: u32) {
    unsafe { font_padding(object, enabled, true) };
}

#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
pub struct NotificationEvent(pub u32);

#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct NotificationValue(pub u32);

#[derive(Clone, Copy)]
#[repr(C)]
pub struct DeviceNotificationHeader {
    pub event: NotificationEvent,
    pub value: NotificationValue,
}

#[repr(transparent)]
pub struct NotificationOpaqueTail([MaybeUninit<u8>; 68]);

#[repr(C)]
pub struct DeviceNotificationPacket {
    pub header: DeviceNotificationHeader,
    pub payload: NotificationOpaqueTail,
}

#[derive(Clone, Copy)]
#[repr(u8)]
enum NotificationCategory {
    Primary = 1,
    LoadTest = 7,
}

#[derive(Clone, Copy)]
enum NotificationRoute {
    History,
    Queue,
}

impl NotificationRoute {
    fn parse(address: Address32) -> Self {
        if address.0 == 0x0072_b318
            || address.0 == xm1_pipeline_image_font_notification_history as *const () as u32
        {
            Self::History
        } else if address.0 == 0x0072_b210
            || address.0 == xm1_pipeline_image_font_notification_router as *const () as u32
        {
            Self::Queue
        } else {
            invalid()
        }
    }
}

unsafe extern "C" {
    fn xm1_develop_append_stream_text(category: u32, text: u32);
    fn xm1_develop_enter_critical() -> u32;
    fn xm1_develop_leave_critical(token: u32);
    fn xm1_develop_loadtest_current() -> u32;
    fn xm1_pipeline_image_font_notification_trace(message: u32);
    fn xm1_pipeline_image_ui_font_notification(snapshot: u32, value: NotificationValue);
}

unsafe fn copy_notification_storage(destination: u32, source: u32, length: usize) {
    unsafe {
        core::ptr::copy_nonoverlapping(
            source as *const MaybeUninit<u8>,
            destination as *mut MaybeUninit<u8>,
            length,
        );
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_error(message: u32) {
    let mut terminated = false;
    for index in 0..29 {
        let byte = if terminated { 0 } else {
            unsafe { Address32(message.wrapping_add(index)).byte() }
        };
        terminated |= byte == 0;
        unsafe { Address32(0x0166_59dc + index).write_byte(byte) };
    }
    unsafe { xm1_pipeline_image_ui_font_notification(0x0166_59dc, NotificationValue(0)) };
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_ui_font_notify_current_task(text: u32) {
    let task = unsafe { xm1_pipeline_image_current_task() } as u8;
    unsafe { xm1_develop_append_stream_text(u32::from(task), text) };
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_ui_font_notification_from_entry(
    snapshot: u32,
    value: NotificationValue,
    entry_tail: *const NotificationOpaqueTail,
) {
    if snapshot == 0 {
        unsafe { Address32(0x0095_482c).write_byte(0) };
        for offset in (0x20..=0x54).step_by(4) {
            unsafe { store(0x0095_482c, offset, 0) };
        }
    } else {
        unsafe {
            xm1_pipeline_image_ui_font_notify_current_task(snapshot);
            copy_notification_storage(0x0095_482c, snapshot, 0x5c);
        }
    }
    let task = unsafe { xm1_pipeline_image_current_task() } as u8;
    unsafe { Address32(0x0095_4884).write_byte(task) };
    let mut packet = MaybeUninit::<DeviceNotificationPacket>::uninit();
    let pointer = packet.as_mut_ptr();
    unsafe {
        core::ptr::addr_of_mut!((*pointer).header).write(DeviceNotificationHeader {
            event: NotificationEvent(0x91b),
            value,
        });
        copy_notification_storage(
            core::ptr::addr_of_mut!((*pointer).payload) as u32,
            entry_tail as u32,
            68,
        );
        xm1_pipeline_image_dispatch_font_notification(pointer);
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_init_font_notification_source() {
    if unsafe { Address32(0x0095_4884).byte() } != 0 { return; }
    let task = unsafe { xm1_pipeline_image_current_task() } as u8;
    unsafe { Address32(0x0095_4884).write_byte(task) };
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_dispatch_font_notification(packet: *const DeviceNotificationPacket) -> i32 {
    match NotificationRoute::parse(Address32(unsafe { word(0x0082_9a0c, 0) })) {
        NotificationRoute::History => unsafe { xm1_pipeline_image_font_notification_history(packet) },
        NotificationRoute::Queue => unsafe { xm1_pipeline_image_font_notification_router(packet) },
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_notification_history(packet: *const DeviceNotificationPacket) -> i32 {
    let saved = unsafe { xm1_develop_enter_critical() };
    let count = unsafe { word(0x0091_34c0, 0) };
    let status = if count < 64 {
        let header = unsafe { read_volatile(core::ptr::addr_of!((*packet).header)) };
        let destination = 0x0091_21c0u32.wrapping_add(count * 76);
        unsafe {
            store(destination, 0, header.event.0);
            store(destination, 4, header.value.0);
            store(0x0091_34c0, 0, count + 1);
        }
        0
    } else {
        let task = unsafe { xm1_pipeline_image_current_task() } as u8;
        unsafe { xm1_develop_append_stream_text(u32::from(task), 0x0078_2054) };
        -63
    };
    unsafe { xm1_develop_leave_critical(saved) };
    status
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_notification_router(input: *const DeviceNotificationPacket) -> i32 {
    let task = unsafe { xm1_pipeline_image_current_task() } as u8;
    let loadtest = unsafe { xm1_develop_loadtest_current() };
    let header = unsafe { read_volatile(core::ptr::addr_of!((*input).header)) };
    let category = if loadtest == 1 && header.event.0 & 0x900 != 0 {
        NotificationCategory::LoadTest
    } else { NotificationCategory::Primary };
    let mut packet = MaybeUninit::<DeviceNotificationPacket>::uninit();
    let pointer = packet.as_mut_ptr();
    unsafe { core::ptr::addr_of_mut!((*pointer).header).write(header) };
    if header.event == NotificationEvent(0x82e) && matches!(category, NotificationCategory::Primary) {
        let count = unsafe { word(0x0091_21bc, 0) };
        if count != unsafe { word(0x0091_34c4, 0) } {
            unsafe { xm1_pipeline_image_font_notification_trace(0x0078_2048) };
            return 0;
        }
        unsafe { store(0x0091_21bc, 0, count.wrapping_add(1)) };
    }
    unsafe {
        core::ptr::copy_nonoverlapping(
            core::ptr::addr_of!((*input).payload).cast::<MaybeUninit<u8>>(),
            core::ptr::addr_of_mut!((*pointer).payload).cast::<MaybeUninit<u8>>(),
            68,
        );
    }
    let message = QueueMessage {
        code: 0,
        sender: task,
        destination: category as u8,
        length: 76,
        payload: pointer as u32,
    };
    let status = unsafe { xm1_pipeline_image_send_message(category as u32, &message) };
    if unsafe { read_volatile(core::ptr::addr_of!((*input).header.event)) } == NotificationEvent(0x91b) {
        unsafe { xm1_pipeline_image_init_font_notification_source() };
    }
    status
}
