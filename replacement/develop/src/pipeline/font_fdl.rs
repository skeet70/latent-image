use super::callbacks::{self, CallbackWord};
use core::ptr::{read_volatile, write_volatile};
use super::shared::Address32;

#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq)]
struct Address(u32);
impl Address {
    fn at(self, offset: u32) -> Self { Self(self.0.wrapping_add(offset)) }
    unsafe fn word(self) -> u32 { unsafe { read_volatile(self.0 as *const u32) } }
    unsafe fn half(self) -> u16 { unsafe { read_volatile(self.0 as *const u16) } }
    unsafe fn byte(self) -> u8 { unsafe { read_volatile(self.0 as *const u8) } }
    unsafe fn put(self, value: u32) { unsafe { write_volatile(self.0 as *mut u32, value) }; }
    unsafe fn put_half(self, value: u16) { unsafe { write_volatile(self.0 as *mut u16, value) }; }
    unsafe fn put_byte(self, value: u8) { unsafe { write_volatile(self.0 as *mut u8, value) }; }
    unsafe fn pointer(self) -> Self { Self(unsafe { self.word() }) }
}
#[repr(transparent)]
#[derive(Clone, Copy)]
struct FontObject(Address);
impl FontObject {
    unsafe fn field(self, offset: u32) -> u32 { unsafe { self.0.at(offset).word() } }
    unsafe fn pointer(self, offset: u32) -> Address { Address(unsafe { self.field(offset) }) }
    unsafe fn engine(self) -> Address { unsafe { self.pointer(0) } }
    unsafe fn context(self) -> Address { unsafe { self.pointer(0x88) } }
    unsafe fn request(self) -> Address { unsafe { self.pointer(0x80) } }
}
#[repr(transparent)]
#[derive(Clone, Copy)]
struct TextLength(u32);
#[repr(transparent)]
#[derive(Clone, Copy)]
struct PreparationFlags(u32);
impl PreparationFlags {
    fn clear(self) -> bool { self.0 & 4 != 0 }
    fn render(self) -> bool { self.0 & 2 != 0 }
    fn refresh(self) -> bool { self.0 & 1 != 0 }
}
#[repr(C)]
struct GlyphBitmap { width: u32, height: u32, stride: u32, format: u32, pixel_format: u32, fill: u32, foreground: u32, background: u32, length: u32, coverage: u32 }

unsafe fn fill(address: Address, byte: u8, count: u32) {
    let prefix = 4 - (address.0 & 3);
    for offset in 0..prefix { unsafe { address.at(offset).put_byte(byte) }; }
    let pattern = u32::from(byte) * 0x01010101;
    let mut offset = prefix;
    while offset.wrapping_add(4) <= count {
        unsafe { address.at(offset).put(pattern) };
        offset = offset.wrapping_add(4);
    }
    while offset < count {
        unsafe { address.at(offset).put_byte(byte) };
        offset = offset.wrapping_add(1);
    }
}
unsafe fn copy(source: Address, destination: Address, bytes: u32) {
    if source.0 == 0 || destination.0 == 0 || bytes == 0 { return; }
    let aligned = (source.0 | destination.0) & 3 == 0;
    let separated = destination.0 > source.0.wrapping_add(4) || source.0 > destination.0.wrapping_add(4);
    let width = if aligned && (bytes & 3 == 0 || separated) { 4 } else if (source.0 | destination.0 | bytes) & 1 == 0 { 2 } else { 1 };
    let mut offset = 0;
    while bytes - offset >= width {
        match width {
            4 => unsafe { destination.at(offset).put(source.at(offset).word()) },
            2 => unsafe { destination.at(offset).put_half(source.at(offset).half()) },
            _ => unsafe { destination.at(offset).put_byte(source.at(offset).byte()) },
        }
        offset += width;
    }
    let tail_width = if (source.0 | destination.0 | bytes) & 1 == 0 { 2 } else { 1 };
    while offset < bytes {
        if tail_width == 2 { unsafe { destination.at(offset).put_half(source.at(offset).half()) }; }
        else { unsafe { destination.at(offset).put_byte(source.at(offset).byte()) }; }
        offset += tail_width;
    }
}
unsafe fn map_engine_error(result: i32) -> i32 {
    let index = (result as u32).wrapping_add(13);
    if index < 13 { unsafe { Address(0x11859dc + 0x5c).at(index * 4).word() as i32 } } else { 0 }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_fill_memory(destination: u32, byte: u32, count: u32) -> u32 {
    unsafe { fill(Address(destination), byte as u8, count) };
    destination
}
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_copy_memory(destination: u32, source: u32, count: u32) {
    unsafe { copy(Address(source), Address(destination), count) };
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_clear_context(engine: u32, context: u32) -> i32 {
    let context = Address(context);
    unsafe { context.put(0); context.at(0xc20).put_half(0) };
    if engine == 0 { return -1; }
    let engine = Address(engine);
    let mode = unsafe { engine.at(0xff4).word() };
    if mode == 1 {
        let buffer = unsafe { context.at(0xc30).pointer() };
        if buffer.0 != 0 { unsafe { fill(buffer, 0, context.at(0xc2c).word()) }; }
        unsafe { fill(context.at(0xc34), 0, 200); context.at(0xc2c).put(0); context.at(0xc24).put(0); context.at(0xc28).put(0) };
    }
    if unsafe { engine.at(0xff4).word() }.wrapping_sub(2) <= 5 {
        for offset in [0xd18, 0xcfc, 0xd00, 0xd04, 0xd08, 0xd0c, 0xd14] { unsafe { context.at(offset).put(0) }; }
    }
    0
}

unsafe fn export_layout(object: FontObject, outline: bool) {
    let context = unsafe { object.context() };
    let layout = unsafe { object.pointer(if outline { 0x116c } else { 0x1168 }) };
    unsafe {
        layout.at(12).put_half(context.at(0xc20).half());
        layout.put(context.word());
        layout.at(4).put(context.at(4).0);
    }
    if outline {
        unsafe { layout.at(16).put(context.at(0xc24).word()); layout.at(20).put(context.at(0xc28).word()); layout.at(24).put(0); layout.at(28).put(0); layout.at(32).put(0); fill(object.0.at(0x478), 0, 0xc80) };
    } else {
        for (destination, source) in [(16,0xd00),(20,0xd04),(24,0xd08),(28,0xcfc),(32,0xd0c)] { unsafe { layout.at(destination).put(context.at(source).word()) }; }
        unsafe { layout.at(36).put(0) };
    }
    let records = object.0.at(if outline { 0x478 } else { 0x90 });
    let mut previous = [0u32; 3];
    let count = unsafe { layout.word() } as i32;
    let mut index = 0u32;
    while (index as i32) < count && index < 50 {
        let source = context.at(index * 60 + 0x68);
        let current = unsafe { source.at(4).half() } as u32;
        let packed = unsafe { source.at(6).half() } as u32;
        previous[0] = (previous[0] & 0xffff0000) | current;
        if packed != 0 {
            previous[0] = current | current.wrapping_shl(16);
            previous[1] = (unsafe { source.half() } as u32).wrapping_shl(16) | if packed <= 256 { 1 } else { packed >> 8 };
            previous[2] = (previous[2] & 0xffff0000) | if packed <= 256 { packed } else { packed & 255 };
        }
        let destination = records.at(index * if outline { 64 } else { 20 });
        for (field, value) in previous.iter().enumerate() { unsafe { destination.at(field as u32 * 4).put(*value) }; }
        let remaining = if outline { 13 } else { 2 };
        for field in 0..remaining { unsafe { destination.at(12 + field * 4).put(source.at(8 + field * 4).word()) }; }
        index += 1;
    }
    unsafe { layout.at(8).put(records.0) };
}
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_export_bitmap(object: u32) -> i32 { unsafe { export_layout(FontObject(Address(object)), false) }; 0 }
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_export_outline(object: u32) -> i32 { unsafe { export_layout(FontObject(Address(object)), true) }; 0 }

unsafe fn prepare_request(object: FontObject, text: Address, length: TextLength, clear: bool) -> Address {
    let request = unsafe { object.request() };
    if clear { unsafe { fill(request, 0, object.field(0x84)) }; }
    unsafe {
        request.at(12).put(length.0);
        request.at(8).put_half(object.0.at(0x5c).half());
        request.at(16).put(text.0);
        request.put(0);
        request.at(4).put(0);
    }
    request
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_prepare_outline(object: u32, text: *const u16, length: u32, flags: u32, output: *mut u32) -> i32 {
    let object = FontObject(Address(object));
    if object.0.0 == 0 { return -2; }
    if unsafe { object.field(12) } != 1 { return -12; }
    let flags = PreparationFlags(flags);
    if flags.clear() {
        let layout = unsafe { object.pointer(0x116c) };
        unsafe { layout.put(0) };
        let text = unsafe { layout.at(4).pointer() };
        if text.0 != 0 { unsafe { fill(text, 0, 100) }; }
        let layout = unsafe { object.pointer(0x116c) };
        let records = unsafe { layout.at(8).pointer() };
        if records.0 != 0 { unsafe { fill(records, 0, 0xc80) }; }
        let layout = unsafe { object.pointer(0x116c) };
        unsafe { layout.at(12).put_half(0); layout.at(16).put(0); layout.at(20).put(0) };
        let bytes = unsafe { layout.at(32).pointer() };
        if bytes.0 != 0 { unsafe { fill(bytes, 0, layout.at(28).word()) }; }
        let layout = unsafe { object.pointer(0x116c) };
        unsafe { layout.at(32).put(0); layout.at(28).put(0); xm1_pipeline_image_font_clear_context(object.engine().0, object.context().0); write_volatile(output, 0) };
        return 0;
    }
    let mut layout = Address(unsafe { read_volatile(output) });
    if flags.refresh() || !flags.render() || layout.0 == 0 {
        if text.is_null() || length as i32 <= 0 { return -9; }
        let request = unsafe { prepare_request(object, Address(text as u32), TextLength(length), true) };
        let context = unsafe { object.context() };
        unsafe { fill(context, 0, object.field(0x8c)) };
        let status = unsafe { xm1_pipeline_image_font_shape(object.engine().0, request.0, context.0, 1) };
        let status = unsafe { map_engine_error(status) };
        if status != 0 { return status; }
        unsafe { fill(object.pointer(0x1170), 0, object.field(0x1174)); object.0.at(0x116c).put(object.field(0x1170)); export_layout(object, true) };
        layout = unsafe { object.pointer(0x116c) };
        unsafe { write_volatile(output, layout.0) };
        if !flags.render() { return 0; }
        if layout.0 == 0 { return -13; }
    }
    if unsafe { layout.word() } as i32 > 50 { unsafe { layout.put(50) }; }
    let context = unsafe { object.context() };
    let request = unsafe { object.request() };
    unsafe { fill(object.pointer(0x1178), 0, object.field(0x117c)); context.at(0xc30).put(object.field(0x1178)); context.at(0xc2c).put(object.field(0x117c)) };
    let status = unsafe { xm1_pipeline_image_font_shape(object.engine().0, request.0, context.0, 2) };
    let old_capacity = unsafe { object.field(0x117c) } as i32;
    let actual = unsafe { context.at(0xc2c).word() };
    let buffer = unsafe { context.at(0xc30).word() };
    let layout = Address(unsafe { read_volatile(output) });
    unsafe { layout.at(28).put(actual); layout.at(32).put(buffer); layout.at(24).put(context.at(0xc34).0) };
    if actual as i32 > old_capacity { unsafe { object.0.at(0x117c).put(actual); object.0.at(0x1178).put(context.at(0xc30).word()) }; }
    unsafe { map_engine_error(status) }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_prepare_bitmap(object: u32, text: *const u16, length: u32, flags: u32, output: *mut u32, buffer: u32, stride: u32, rows: u32, x: u32, y: u32) -> i32 {
    let object = FontObject(Address(object));
    if object.0.0 == 0 { return -2; }
    if unsafe { object.field(12) } == 1 { return -12; }
    let flags = PreparationFlags(flags);
    if flags.clear() {
        let layout = unsafe { object.pointer(0x1168) };
        for offset in [32,0,28,16,20,24,36] { unsafe { layout.at(offset).put(0) }; }
        unsafe { layout.at(12).put_half(0) };
        let context = unsafe { object.context() };
        if buffer != 0 { unsafe { context.at(0xd10).put(0) }; }
        unsafe { xm1_pipeline_image_font_clear_context(object.engine().0, context.0); write_volatile(output, 0) };
        return 0;
    }
    let mut layout = Address(unsafe { read_volatile(output) });
    if flags.refresh() || !flags.render() || layout.0 == 0 {
        if text.is_null() || length as i32 <= 0 { return -9; }
        let request = unsafe { prepare_request(object, Address(text as u32), TextLength(length), false) };
        let status = unsafe { xm1_pipeline_image_font_shape(object.engine().0, request.0, object.context().0, 1) };
        let status = unsafe { map_engine_error(status) };
        if status != 0 { return status; }
        unsafe { object.0.at(0x1168).put(object.field(0x1170)); export_layout(object, false) };
        layout = unsafe { object.pointer(0x1168) };
        unsafe { write_volatile(output, layout.0) };
        if !flags.render() { return 0; }
        if layout.0 == 0 { return -13; }
    }
    if unsafe { layout.word() } as i32 > 50 { unsafe { layout.put(50) }; }
    let request;
    let context;
    if buffer == 0 {
        let required = unsafe { layout.at(32).word() };
        if (unsafe { object.field(0x117c) } as i32) < required as i32 {
            let rounded = required.wrapping_add(1023) as i32;
            let rounded = if rounded < 0 { rounded.wrapping_add(1023) } else { rounded } as u32 & !1023;
            let allocation = unsafe { xm1_pipeline_image_font_resize(object.field(0x1178), rounded) };
            if allocation == 0 { return -10; }
            unsafe { object.0.at(0x1178).put(allocation); object.0.at(0x117c).put(Address(read_volatile(output)).at(32).word()) };
        }
        let layout = Address(unsafe { read_volatile(output) });
        context = unsafe { object.context() };
        request = unsafe { object.request() };
        unsafe { fill(object.pointer(0x1178), 0, layout.at(32).word()); Address(read_volatile(output)).at(36).put(object.field(0x1178)); context.at(0xd10).put(object.field(0x1178)); context.at(0xd18).put(0); context.at(0xd14).put(0) };
    } else {
        request = unsafe { object.request() };
        context = unsafe { object.context() };
        if (x as i32) < 0 || (y as i32) < 0 || (stride.wrapping_mul(rows) as i32) < (unsafe { layout.at(32).word() } as i32) || (rows as i32) < (y.wrapping_add(unsafe { layout.at(20).word() }) as i32) { return -10; }
        let stride = if stride as i32 <= 0 { unsafe { layout.at(24).word() } } else { stride };
        unsafe { context.at(0xd10).put(buffer); context.at(0xd18).put(y); context.at(0xd08).put(stride); context.at(0xd04).put(rows); context.at(0xd14).put(x) };
    }
    let status = unsafe { xm1_pipeline_image_font_shape(object.engine().0, request.0, context.0, 2) };
    let status = unsafe { map_engine_error(status) };
    if unsafe { object.field(0x2c) } as i32 > 0 { unsafe { fill_padding_rows(object, context, PaddingSide::Leading) }; }
    if unsafe { object.field(0x38) } as i32 > 0 { unsafe { fill_padding_rows(object, context, PaddingSide::Trailing) }; }
    status
}

#[derive(Clone, Copy)]
enum PaddingSide { Leading, Trailing }
unsafe fn fill_padding_rows(object: FontObject, context: Address, side: PaddingSide) {
    let buffer = unsafe { context.at(0xd10).pointer() };
    if buffer.0 == 0 { return; }
    let last = context.at(unsafe { context.word() }.wrapping_sub(1).wrapping_mul(60));
    let start = unsafe { last.at(0x70).word() };
    let mut width = start;
    if unsafe { last.at(0x78).word() } == start { width = width.wrapping_add(unsafe { last.at(0x80).word() }); }
    let (margin, count, offset) = match side { PaddingSide::Leading => (0x30,0x2c,0x28), PaddingSide::Trailing => (0x3c,0x38,0x34) };
    width = width.wrapping_add((unsafe { object.field(margin) } as i32).min(unsafe { object.field(0x24) } as i32) as u32);
    width = ((width as i32).min(unsafe { context.at(0xd00).word() } as i32)) as u32;
    let count = unsafe { object.field(count) } as i32;
    let format = unsafe { context.at(0xcfc).word() };
    if count <= 0 || !matches!(format, 1 | 8) { return; }
    let x = unsafe { context.at(0xd14).word() };
    let y = unsafe { context.at(0xd04).word() }.wrapping_sub(unsafe { object.field(offset) }).wrapping_sub(count as u32).wrapping_add(unsafe { context.at(0xd18).word() });
    let stride = unsafe { context.at(0xd08).word() };
    let (column, bytes, value) = if format == 1 {
        let remainder = x & 7;
        (((x as i32) >> 3) as u32, (((7u32.wrapping_sub(remainder).wrapping_add(width) as i32) >> 3).wrapping_add((remainder != 0) as i32)) as u32, 255)
    } else {
        let value = if unsafe { object.field(0x40) } == 1 && unsafe { object.field(0x64) } == 0 { 15 } else { 255 };
        (x, width, value)
    };
    let mut address = buffer.at(stride.wrapping_mul(y).wrapping_add(column));
    for _ in 0..count {
        if bytes as i32 > 0 {
            if format == 8 { unsafe { fill(address, value, bytes) }; } else { for offset in 0..bytes { unsafe { address.at(offset).put_byte(255) }; } }
        }
        address = address.at(unsafe { context.at(0xd08).word() });
    }
}
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_fill_leading_rows(object: u32, context: u32) { unsafe { fill_padding_rows(FontObject(Address(object)), Address(context), PaddingSide::Leading) }; }
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_fill_trailing_rows(object: u32, context: u32) { unsafe { fill_padding_rows(FontObject(Address(object)), Address(context), PaddingSide::Trailing) }; }


#[repr(u32)]
#[derive(Clone, Copy)]
enum FontProperty {
    Scale = 0, Width = 1, Height = 2, AutoWeight = 3, AutoSpacing = 4, AutoTrailing = 5, AutoLeading = 6,
    Weight = 20, EdgeWidth = 21, Spacing = 22, TrailingPosition = 23, Top = 24, TrailingSpacing = 25,
    LeadingPosition = 26, Bottom = 27, LeadingSpacing = 28, PixelFormat = 40, Rotation = 41,
    Direction = 42, OutlineMode = 43, Fill = 45, Alignment = 46, Expansion = 47,
}
unsafe fn set(object: FontObject, property: FontProperty, value: u32) -> i32 {
    unsafe { xm1_pipeline_image_font_set_option(object.0.0, property as u32, value) }
}
unsafe fn engine_expansion(object: FontObject, value: u32) {
    unsafe { xm1_pipeline_image_font_backend_option(object.engine().0, 10, value) };
}
fn rounded_blocks(value: u32) -> u32 {
    ((value.wrapping_add(1023) as i32) / 1024) as u32
}
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_get_option(object: u32, key: u32) -> i32 {
    let object = FontObject(Address(object));
    if object.0.0 == 0 { return -2; }
    let mut metrics = [0u32; 8];
    let status = unsafe { xm1_pipeline_image_font_engine_metrics(object.engine().0, metrics.as_mut_ptr()) };
    if status < 0 { return unsafe { map_engine_error(status) }; }
    if key.wrapping_sub(100) < 8 { return metrics[(key - 100) as usize] as i32; }
    if key == 200 { return rounded_blocks(unsafe { object.pointer(0x11ac).at(8).word() }).wrapping_shl(10) as i32; }
    if key == 201 {
        return rounded_blocks(unsafe { object.field(4) }).wrapping_add(rounded_blocks(unsafe { object.field(0x117c) })).wrapping_add(rounded_blocks(unsafe { object.engine().word() })).wrapping_add(if unsafe { object.field(0x1190) } == 0 { 0 } else { 69 }).wrapping_shl(10) as i32;
    }
    let mode = unsafe { object.field(12) };
    if mode == 2 && !matches!(key, 42 | 46 | 47) { return -4; }
    let outline = mode == 1;
    let offset = match key {
        0 => 0x10, 1 => 0x14, 2 => 0x18, 3 => 0x70, 4 => 0x74, 5 => 0x78, 6 => 0x7c,
        20 => 0x1c, 21 => 0x20, 22 => 0x24, 23 => 0x28, 24 => 0x2c, 25 => 0x30, 26 => 0x34, 27 => 0x38, 28 => 0x3c,
        40 if !outline => 0x40, 41 => 0x58, 42 => 0x5c, 43 if outline => 0x60,
        45 if !outline => 0x64, 46 if !outline => 0x68, 47 => 0x6c,
        _ => return -4,
    };
    unsafe { object.field(offset) as i32 }
}

#[derive(Clone, Copy)]
struct DrawPlacement { mode: u32, x: u32, y: u32, first: u32, second: u32, foreground: Address, background: Address, edge: Address, details: Address, state: Address }
unsafe fn prepared_bitmap(object: FontObject, layout: Address, foreground: Address, background: Address) -> GlyphBitmap {
    GlyphBitmap {
        width: unsafe { layout.at(16).word() }, height: unsafe { layout.at(20).word() },
        stride: unsafe { layout.at(24).word() }, format: unsafe { layout.at(28).word() },
        pixel_format: unsafe { object.field(0x40) }, fill: unsafe { object.field(0x64) },
        foreground: unsafe { foreground.word() }, background: unsafe { background.word() },
        length: unsafe { layout.at(32).word() }, coverage: unsafe { layout.at(36).word() },
    }
}
unsafe fn draw_prepared(object: FontObject, layout: Address, placement: DrawPlacement, shadow: Option<u32>) -> i32 {
    let (foreground, background, x, y) = match shadow {
        Some(offset) => (placement.edge, placement.foreground, placement.x.wrapping_add(offset), placement.y.wrapping_add(offset)),
        None => (placement.foreground, placement.background, placement.x, placement.y),
    };
    let bitmap = unsafe { prepared_bitmap(object, layout, foreground, background) };
    unsafe { xm1_pipeline_image_blend_glyph(placement.mode, x, y, placement.first, placement.second, &bitmap, placement.state.0) }
}
unsafe fn draw_object(object: FontObject, text: *const u16, length: TextLength, placement: DrawPlacement) -> i32 {
    if object.0.0 == 0 { return -2; }
    if unsafe { object.field(12) } == 1 { return -12; }
    let distance = unsafe { object.field(0x20) };
    let shadow = unsafe { object.field(0x1c) } == 1 && distance as i32 > 0 && unsafe { object.field(12) } != 2 && placement.edge.0 != 0;
    let (old_format, old_top, old_bottom, old_expansion) = if shadow {
        let format = unsafe { object.field(0x40) };
        let top = unsafe { xm1_pipeline_image_font_get_option(object.0.0, FontProperty::Top as u32) } as u32;
        let bottom = unsafe { xm1_pipeline_image_font_get_option(object.0.0, FontProperty::Bottom as u32) } as u32;
        let expansion = unsafe { xm1_pipeline_image_font_get_option(object.0.0, FontProperty::Expansion as u32) } as u32;
        unsafe { set(object, FontProperty::Bottom, 0); set(object, FontProperty::Weight, 2); object.0.at(0x11a8).put(1) };
        (format, top, bottom, expansion)
    } else { (0, 0, 0, 0) };
    let old_weight = shadow as u32;
    let mut layout = 0u32;
    let mut result = unsafe { xm1_pipeline_image_font_prepare_bitmap(object.0.0, text, length.0, 1, &mut layout, 0, 0, 0, 0, 0) };
    if result >= 0 {
        if placement.details.0 != 0 {
            let layout = Address(layout);
            unsafe {
                placement.details.put(layout.at(16).word()); placement.details.at(4).put(layout.at(20).word());
                placement.details.at(8).put(layout.word()); copy(layout.at(4).pointer(), placement.details.at(12), layout.word().wrapping_shl(1));
            }
        }
        result = unsafe { xm1_pipeline_image_font_prepare_bitmap(object.0.0, text, length.0, 2, &mut layout, 0, 0, 0, 0, 0) };
        if result >= 0 { result = unsafe { draw_prepared(object, Address(layout), placement, None) }; }
        if result >= 0 {
            unsafe { xm1_pipeline_image_font_prepare_bitmap(object.0.0, text, length.0, 4, &mut layout, 0, 0, 0, 0, 0) };
            if !shadow { return 0; }
            unsafe { object.0.at(0x11a8).put(0); set(object, FontProperty::Top, 0); set(object, FontProperty::Bottom, old_bottom); set(object, FontProperty::Weight, 0); set(object, FontProperty::Expansion, old_expansion.wrapping_add(distance.wrapping_shl(1))) };
            result = unsafe { xm1_pipeline_image_font_prepare_bitmap(object.0.0, text, length.0, 1, &mut layout, 0, 0, 0, 0, 0) };
            if result >= 0 { result = unsafe { xm1_pipeline_image_font_prepare_bitmap(object.0.0, text, length.0, 2, &mut layout, 0, 0, 0, 0, 0) }; }
            if result >= 0 { result = unsafe { draw_prepared(object, Address(layout), placement, Some(distance)) }; }
            if result >= 0 {
                unsafe { set(object, FontProperty::Top, old_top); set(object, FontProperty::Weight, old_weight); set(object, FontProperty::Expansion, old_expansion) };
                return 0;
            }
        }
    }
    unsafe { object.0.at(0x11a8).put(0) };
    if shadow {
        unsafe { set(object, FontProperty::Top, old_top) };
        result = unsafe { set(object, FontProperty::Weight, old_weight) };
        unsafe { engine_expansion(object, old_expansion) };
    }
    unsafe { set(object, FontProperty::PixelFormat, old_format); set(object, FontProperty::Weight, old_weight); engine_expansion(object, old_expansion) };
    result
}
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_draw(object: u32, text: *const u16, length: u32, mode: u32, x: u32, y: u32, first: u32, second: u32, foreground: u32, background: u32, edge: u32, details: u32, state: u32) -> i32 {
    unsafe { draw_object(FontObject(Address(object)), text, TextLength(length), DrawPlacement { mode, x, y, first, second, foreground: Address(foreground), background: Address(background), edge: Address(edge), details: Address(details), state: Address(state) }) }
}

unsafe extern "C" {
    fn xm1_pipeline_image_font_set_option(object: u32, key: u32, value: u32) -> i32;
    fn xm1_pipeline_image_font_backend_option(engine: u32, key: u32, value: u32);
    fn xm1_pipeline_image_blend_glyph(mode: u32, x: u32, y: u32, first: u32, second: u32, bitmap: *const GlyphBitmap, state: u32) -> i32;
}

#[repr(transparent)]
#[derive(Clone, Copy)]
struct FontHeap(Address);
unsafe fn initialize_heap(heap: FontHeap, bytes: u32, fresh: bool, minimum: u32) -> Address {
    if heap.0.0 == 0 || bytes <= 64 { return Address(0); }
    if !fresh {
        for offset in 0..8 {
            if unsafe { heap.0.at(offset).byte() } != unsafe { Address(0x1186b9c).at(offset).byte() } { return Address(0); }
        }
        return heap.0;
    }
    unsafe {
        fill(heap.0, 0, 56); copy(Address(0x1186b9c), heap.0, 8);
        heap.0.at(8).put(56); heap.0.at(12).put(if minimum > 8 { minimum.wrapping_add(7) & !7 } else { 8 });
        heap.0.at(32).put(bytes); heap.0.at(64).put(heap.0.at(8).word()); heap.0.at(56).put(bytes - 64); heap.0.at(16).put(bytes);
    }
    heap.0
}
unsafe fn heap_allocate(heap: FontHeap, requested: u32, zero: bool) -> Address {
    let minimum = unsafe { heap.0.at(12).word() };
    let amount = if requested >= minimum { requested.wrapping_add(7) & !7 } else { minimum };
    let mut first = unsafe { heap.0.at(8).word() };
    let mut start = heap.0.at(first);
    let mut attempt = 1u32;
    loop {
        let mut previous = start;
        loop {
            let offset = unsafe { previous.at(8).word() };
            if offset == 0 || offset > unsafe { heap.0.at(16).word() } { return Address(0); }
            let block = heap.0.at(offset);
            if block.0 == 0 { return block; }
            let available = unsafe { block.word() };
            if available >= amount {
                let allocation;
                if unsafe { heap.0.at(12).word() }.wrapping_add(8).wrapping_add(amount) < available {
                    let rest = block.at(amount.wrapping_add(8));
                    unsafe {
                        rest.put(available.wrapping_sub(amount).wrapping_sub(8));
                        if previous != block { rest.at(8).put(block.at(8).word()); } else { previous = rest; }
                        block.put(amount); previous.at(8).put(rest.0.wrapping_sub(heap.0.0));
                        if block == heap.0.at(heap.0.at(8).word()) { heap.0.at(8).put(rest.0.wrapping_sub(heap.0.0)); }
                    }
                    allocation = amount;
                } else if offset != first {
                    unsafe { previous.at(8).put(block.at(8).word()) };
                    allocation = available;
                } else { break; }
                unsafe {
                    heap.0.at(20).put(heap.0.at(20).word().wrapping_add(8).wrapping_add(allocation));
                    heap.0.at(28).put(heap.0.at(28).word().wrapping_add(requested));
                }
                let payload = block.at(8);
                if zero { unsafe { fill(payload, 0, amount) }; }
                return payload;
            }
            previous = block;
            if offset == first { break; }
        }
        if attempt != 0 {
            let callback = unsafe { heap.0.at(36).word() };
            let depth = unsafe { heap.0.at(44).word() } as i32;
            if callback == 0 || depth > 2 { return Address(0); }
            unsafe { heap.0.at(44).put(depth.wrapping_add(1) as u32) };
            let result = callbacks::call_reclaim(callbacks::registry(), CallbackWord(callback as usize), heap.0.0, unsafe { heap.0.at(40).word() }, attempt).unwrap_or(0);
            unsafe { heap.0.at(44).put(heap.0.at(44).word().wrapping_sub(1)) };
            if result == 0 { return Address(0); }
            first = unsafe { heap.0.at(8).word() };
            start = heap.0.at(first);
        }
        attempt = attempt.wrapping_add(1);
    }
}
unsafe fn heap_release(heap: FontHeap, payload: Address) -> i32 {
    if payload.0 == 0 { return 0; }
    if heap.0.0 == 0 || payload == heap.0 || payload.0.wrapping_sub(heap.0.0) > unsafe { heap.0.at(16).word() } { return -1; }
    let block = payload.at(u32::MAX - 7);
    let size = unsafe { block.word() };
    let offset = block.0.wrapping_sub(heap.0.0);
    let head = unsafe { heap.0.at(8).word() };
    if size > unsafe { heap.0.at(32).word() } || offset == 0 || offset > unsafe { heap.0.at(16).word() } { return -1; }
    unsafe { heap.0.at(24).put(heap.0.at(24).word().wrapping_add(size).wrapping_add(8)) };
    let mut previous = heap.0.at(head);
    if previous.0 < block.0 {
        if block == previous.at(unsafe { previous.word() }.wrapping_add(8)) { unsafe { previous.put(size.wrapping_add(previous.word()).wrapping_add(8)) }; }
        else { unsafe { block.at(8).put(previous.at(8).word()); previous.at(8).put(offset); heap.0.at(8).put(offset) }; }
        return 0;
    }
    let next_offset;
    loop {
        let next = unsafe { previous.at(8).word() };
        if offset <= next { next_offset = next; break; }
        if next <= 7 { return -1; }
        previous = heap.0.at(next);
    }
    let next = heap.0.at(next_offset);
    let joins_previous = block == previous.at(unsafe { previous.word() }.wrapping_add(8));
    let joins_next = next == block.at(size.wrapping_add(8));
    if !joins_previous {
        if !joins_next { unsafe { block.at(8).put(next_offset) }; }
        else {
            if next.0.wrapping_sub(heap.0.0) == head { unsafe { heap.0.at(8).put(offset) }; }
            let link = unsafe { next.at(8).word() };
            unsafe { block.at(8).put(if link == next.0.wrapping_sub(heap.0.0) { offset } else { link }); block.put(block.word().wrapping_add(next.word()).wrapping_add(8)) };
        }
        unsafe { previous.at(8).put(offset) };
        return 0;
    }
    let mut previous_size = unsafe { previous.word() };
    if joins_next {
        if next.0.wrapping_sub(heap.0.0) == head { unsafe { heap.0.at(8).put(previous.0.wrapping_sub(heap.0.0)) }; }
        previous_size = previous_size.wrapping_add(unsafe { next.word() }).wrapping_add(8);
        unsafe { previous.at(8).put(next.at(8).word()); previous.put(previous_size) };
    }
    unsafe { previous.put(block.word().wrapping_add(previous_size).wrapping_add(8)) };
    0
}
unsafe fn heap_resize(heap: FontHeap, payload: Address, bytes: u32) -> Address {
    if payload.0 == 0 { return unsafe { heap_allocate(heap, bytes, false) }; }
    if bytes == 0 { unsafe { heap_release(heap, payload) }; return Address(0); }
    let old_size = unsafe { payload.at(u32::MAX - 7).word() };
    if bytes <= old_size && old_size.wrapping_sub(bytes.wrapping_add(7) & !7) <= unsafe { heap.0.at(12).word() } { return payload; }
    let next = unsafe { heap_allocate(heap, bytes, false) };
    if next.0 != 0 { unsafe { copy(payload, next, bytes); heap_release(heap, payload) }; }
    next
}
unsafe fn font_heap() -> FontHeap {
    let slot = Address(0x1667040);
    let mut heap = unsafe { slot.pointer() };
    if heap.0 == 0 {
        heap = unsafe { initialize_heap(FontHeap(Address(0x2063800)), 0xae000, true, 0x400) };
        unsafe { slot.put(heap.0) };
    }
    FontHeap(heap)
}
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_allocate(bytes: u32) -> u32 {
    let fresh = unsafe { Address(0x1667040).word() } == 0;
    let result = unsafe { heap_allocate(font_heap(), bytes, false) };
    if fresh { unsafe { Address(0x1666ff8).put(result.0) }; }
    result.0
}
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_resize(allocation: u32, bytes: u32) -> u32 { unsafe { heap_resize(font_heap(), Address(allocation), bytes) }.0 }
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_release(allocation: u32) {
    let heap = FontHeap(unsafe { Address(0x1667040).pointer() });
    if heap.0.0 != 0 && allocation != unsafe { Address(0x1666ff8).word() } && allocation > heap.0.0 && allocation < heap.0.0.wrapping_add(0xae000) { unsafe { heap_release(heap, Address(allocation)) }; }
}

#[repr(transparent)]
#[derive(Clone, Copy)]
struct ScriptClass(u32);
#[repr(transparent)]
#[derive(Clone, Copy)]
struct Language(u32);
#[repr(C)]
#[derive(Clone, Copy)]
struct PositionedRun {
    range: u32, script: ScriptClass, language: Language, advance: i32, height: i32,
    x: i32, y: i32, terminal_x: i32, terminal_y: i32,
}
unsafe extern "C" {
    fn xm1_pipeline_image_font_detect_mode(text: *const u16, count: i32, script: *mut i32, language: *mut i32) -> u32;
    fn xm1_pipeline_image_font_detect_script_run(text: *const u16, count: u16, start: u16, end: *mut u16, direction: u32, script: *mut i32, language: *mut i32, run_direction: *mut u16) -> i32;
    fn xm1_pipeline_image_font_select_row(engine: u32, script: u32, language: u32, row: *mut u16) -> i32;
    fn xm1_pipeline_image_font_dispatch_script(engine: u32, script: u32, language: u32, flags: u32, run: u32) -> i32;
}
unsafe fn script_direction(script: ScriptClass) -> u16 {
    let index = script.0.wrapping_sub(2);
    if index <= 4 { unsafe { Address(0x118f970).at(index * 2).half() } } else { 0x200e }
}
unsafe fn render_positioned_runs(engine: Address, mut result: i32) -> i32 {
    unsafe { engine.at(0x524).put_half(0); engine.at(0x526).put_half(0) };
    let mut index = 0;
    while index < u32::from(unsafe { engine.at(0xea4).half() }) {
        let mut run = unsafe { read_volatile(engine.at(0xea8 + index * 36).0 as *const PositionedRun) };
        result |= unsafe { xm1_pipeline_image_font_dispatch_script(engine.0, run.script.0, run.language.0, 0, &mut run as *mut PositionedRun as u32) };
        index += 1;
    }
    result
}
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_shape(engine: u32, request: u32, context: u32, flags: u32) -> i32 {
    let engine = Address(engine);
    let request = Address(request);
    if engine.0 == 0 { return -1; }
    if unsafe { request.word() } > 7 { return -4; }
    if unsafe { request.at(4).word() } > 10 { return -5; }
    let count = unsafe { request.at(12).word() } as i32;
    if count <= 0 || unsafe { request.at(16).word() } == 0 { return -7; }
    if count > 50 { return -13; }
    unsafe { engine.at(0x53c).put(context); engine.at(16).put(request.0) };
    let mut result = 0;
    if flags & 3 != 2 {
        if unsafe { engine.at(0x1070).word() } != 0 && unsafe { engine.at(0xff4).word() }.wrapping_sub(3) <= 2 {
            loop {
                let status = unsafe { xm1_pipeline_image_font_initialize_raster_layout(engine.at(0x68).word(), engine.at(0x6c).word(), engine.at(0x1030).word(), engine.at(0x1034).word(), engine.0) };
                if status != -6 { if status != 0 { return status; } break; }
                let status = unsafe { xm1_pipeline_image_font_grow_workspace(engine.0) };
                if status != 0 { return status; }
                unsafe { engine.put(engine.at(8).word().wrapping_add(engine.at(0x1030).word()).wrapping_add(256)) };
            }
        }
        let explicit_direction = unsafe { request.at(8).half() };
        let direction = if explicit_direction != 0 { u32::from(explicit_direction) }
        else if unsafe { request.word() } != 0 && unsafe { request.at(4).word() } != 0 { u32::from(unsafe { script_direction(ScriptClass(request.word())) }) }
        else {
            let detected = unsafe { xm1_pipeline_image_font_detect_mode(request.at(16).word() as *const u16, count, core::ptr::null_mut(), core::ptr::null_mut()) };
            if detected == 0 { 0x200e } else { detected }
        };
        unsafe {
            engine.at(0xff0).put(direction);
            for offset in [0x5a4, 0x60a, 0x524, 0x526, 0xea4] { engine.at(offset).put_half(0); }
            for offset in [0x534, 0x52c, 0x530] { engine.at(offset).put(0); }
        }
        let mut cursor = 0u16;
        while i32::from(cursor) < unsafe { request.at(12).word() } as i32 {
            let script = unsafe { request.word() };
            let language = unsafe { request.at(4).word() };
            if script == 0 || language == 0 {
                unsafe { xm1_pipeline_image_font_detect_script_run(request.at(16).word() as *const u16, request.at(12).word() as u16, cursor, engine.at(0x526).0 as *mut u16, engine.at(0xff0).word(), engine.at(0x52c).0 as *mut i32, engine.at(0x530).0 as *mut i32, engine.at(0x528).0 as *mut u16) };
            } else {
                unsafe { engine.at(0x52c).put(script); engine.at(0x526).put_half(request.at(12).word() as u16); engine.at(0x530).put(language); engine.at(0x528).put_half(script_direction(ScriptClass(script))); }
            }
            let end = unsafe { engine.at(0x526).half() };
            let start = unsafe { engine.at(0x524).half() };
            let length = i32::from(end) - i32::from(start);
            if length <= 0 { break; }
            let selected = unsafe { xm1_pipeline_image_font_select_row(engine.0, engine.at(0x52c).word(), engine.at(0x530).word(), engine.at(0x514).0 as *mut u16) };
            unsafe { engine.at(0x538).put(1); engine.at(0x518).put(u32::from(selected != -1)) };
            if selected == -1 { result |= 2; }
            result |= unsafe { xm1_pipeline_image_font_dispatch_script(engine.0, engine.at(0x52c).word(), engine.at(0x530).word(), 0, 0) };
            cursor = unsafe { engine.at(0x524).half() }.wrapping_add(length as u16);
            unsafe { engine.at(0xea4).put_half(engine.at(0xea4).half().wrapping_add(1)); engine.at(0x524).put_half(cursor); engine.at(0x534).put(0); }
        }
        let status = unsafe { xm1_pipeline_image_font_finalize_runs(engine.0) };
        if status != 0 { return status; }
        if flags & 2 == 0 { return result; }
    }
    unsafe { engine.at(0x538).put(2) };
    if unsafe { engine.at(0xff4).word() } == 1 {
        let context = unsafe { engine.at(0x53c).pointer() };
        unsafe {
            fill(context.at(0xc30).pointer(), 0, context.at(0xc2c).word());
            fill(engine.at(0x53c).pointer().at(0xc34), 0, 200);
        }
        let context = unsafe { engine.at(0x53c).pointer() };
        unsafe { engine.at(0xfe4).put(context.at(0xc2c).word()); context.at(0xc2c).put(0) };
        result = unsafe { render_positioned_runs(engine, result) };
    }
    if unsafe { engine.at(0xff4).word() }.wrapping_sub(3) <= 2 { result = unsafe { render_positioned_runs(engine, result) }; }
    if unsafe { engine.at(0xff4).word() } == 2 { result = unsafe { render_positioned_runs(engine, result) }; }
    if unsafe { engine.at(0xff4).word() }.wrapping_sub(6) <= 1 { result = unsafe { render_positioned_runs(engine, result) }; }
    result
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_grow_workspace(engine: u32) -> i32 {
    let engine = Address(engine);
    if engine.0 == 0 { return -1; }
    let backend = unsafe { engine.at(0xff4).word() };
    let weight = unsafe { engine.at(0x105c).word() };
    let increment = match backend {
        3 if weight.wrapping_sub(1) <= 1 => if weight == 2 { 0x10000 } else { 0x2000 },
        4 => if weight == 2 { 0x10000 } else { 0x2000 },
        5 => 0x10000,
        6 | 7 => 0,
        _ => 0x400,
    };
    let size = unsafe { engine.at(0x1030).word() }.wrapping_add(increment);
    let old = unsafe { engine.at(0x1034).word() };
    if old != 0 { unsafe { xm1_pipeline_image_font_release(old) }; }
    let rounded = size.wrapping_add(1023) as i32;
    let rounded = if rounded < 0 { rounded.wrapping_add(1023) } else { rounded };
    let allocation = unsafe { xm1_pipeline_image_font_allocate(((rounded >> 10) << 10) as u32) };
    if allocation == 0 { return -6; }
    unsafe { engine.at(0x1030).put(size); engine.at(0x1034).put(allocation) };
    0
}
#[repr(transparent)]
#[derive(Clone, Copy)]
struct RasterLayout(Address);
#[derive(Clone, Copy)]
enum MetricAxis { Width, Height }
unsafe fn raster_edge(layout: RasterLayout) -> i32 {
    let first = unsafe { layout.0.at(24).word() } as i32;
    let second = unsafe { layout.0.at(32).word() } as i32;
    let other = if second > 0 { (unsafe { layout.0.at(36).word() }) as i32 } else { 0 };
    if first > 0 || second > 0 { first.max(other) } else { 0 }
}
unsafe fn raster_extent(layout: RasterLayout, axis: MetricAxis) -> i32 {
    let mode = unsafe { layout.0.at(16).word() };
    let weight = unsafe { layout.0.at(20).word() } as i32;
    let base = unsafe { layout.0.at(match axis { MetricAxis::Width => 8, MetricAxis::Height => 12 }).word() } as i32;
    let mut extent = match (axis, mode) {
        (_, 1 | 2) => base.wrapping_add(weight.wrapping_mul(2)),
        (MetricAxis::Height, 4 | 5) | (MetricAxis::Width, 3 | 5) => base.wrapping_add(weight),
        (MetricAxis::Height, 0 | 3) | (MetricAxis::Width, 0 | 4) => base,
        _ => 0,
    };
    let vertical = unsafe { layout.0.at(4).word() } != 0;
    if matches!(axis, MetricAxis::Height) == vertical { extent = extent.wrapping_add(unsafe { raster_edge(layout) }); }
    extent
}
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_raster_height(layout: u32) -> i32 { unsafe { raster_extent(RasterLayout(Address(layout)), MetricAxis::Height) } }
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_raster_width(layout: u32) -> i32 { unsafe { raster_extent(RasterLayout(Address(layout)), MetricAxis::Width) } }
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_raster_right_overhang(layout: u32) -> i32 { unsafe { raster_edge(RasterLayout(Address(layout))) } }

#[derive(Clone, Copy)]
pub(crate) enum BackendMetric { Height, Width, SecondaryExtent, RightOverhang, TopOverhang }
unsafe extern "C" {
    fn xm1_pipeline_image_font_position_forward(engine: u32, run: u32) -> i32;
    fn xm1_pipeline_image_font_position_arabic(engine: u32, run: u32) -> i32;
    fn xm1_pipeline_image_font_position_thai(engine: u32, run: u32) -> i32;
    fn xm1_pipeline_image_font_position_hebrew(engine: u32, run: u32) -> i32;
}
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_finalize_runs(engine: u32) -> i32 {
    let engine = Address(engine);
    let context = unsafe { engine.at(0x53c).pointer() };
    unsafe {
        context.put(u32::from(engine.at(0x60a).half()));
        context.at(0xc20).put_half(engine.at(0xff0).word() as u16);
        engine.at(0xfec).put(0);
    }
    let mut width = 0i32;
    for index in 0..u32::from(unsafe { engine.at(0xea4).half() }) {
        width = width.wrapping_add(unsafe { engine.at(0xeb4 + index * 36).word() } as i32);
    }
    let mut extent = width;
    if width > 0 {
        let last = unsafe { engine.at(0x60a).half() };
        let glyph = unsafe { engine.at((u32::from(last) + 0x2d1) * 2 + 2).half() };
        extent = unsafe { engine_metric(engine, BackendMetric::RightOverhang, glyph) }.wrapping_add(width);
        let bias = unsafe { engine.at(0x1068).word() } as i32;
        if bias < 0 { extent = extent.wrapping_sub(bias); }
    }
    let height = unsafe { engine_metric(engine, BackendMetric::Height, 0) };
    let mut index = 0;
    let mut x = 0i32;
    while index < u32::from(unsafe { engine.at(0xea4).half() }) {
        let address = engine.at(0xea8 + index * 36);
        let mut run = unsafe { read_volatile(address.0 as *const PositionedRun) };
        let context = unsafe { engine.at(0x53c).pointer() };
        let reversed = unsafe { context.at(0xc20).half() } == 0x200f;
        if index == 0 { x = if reversed { extent } else { 0 }; }
        else {
            let previous = unsafe { address.at(u32::MAX - 23).word() } as i32;
            x = if reversed { x.wrapping_sub(previous) } else { x.wrapping_add(previous) };
        }
        run.x = x; run.y = 0;
        match (run.script.0, run.language.0) {
            (2, 5) => { unsafe { xm1_pipeline_image_font_position_arabic(engine.0, &mut run as *mut PositionedRun as u32) }; }
            (3, 6) => { unsafe { xm1_pipeline_image_font_position_thai(engine.0, &mut run as *mut PositionedRun as u32) }; }
            (6, 9) => { unsafe { xm1_pipeline_image_font_position_hebrew(engine.0, &mut run as *mut PositionedRun as u32) }; }
            _ => { unsafe { xm1_pipeline_image_font_position_forward(engine.0, &mut run as *mut PositionedRun as u32) }; }
        }
        unsafe { write_volatile(address.0 as *mut PositionedRun, run) };
        index += 1;
    }
    let context = unsafe { engine.at(0x53c).pointer() };
    if unsafe { engine.at(0xff4).word() } == 1 {
        unsafe { context.at(0xc28).put(height as u32); context.at(0xc24).put(width as u32) };
    } else {
        let format = unsafe { engine.at(0x106c).word() };
        unsafe { context.at(0xcfc).put(if format == 0x100 { 1 } else { 8 }); context.at(0xd00).put(extent as u32); context.at(0xd04).put(height as u32) };
        let mut alignment = unsafe { engine.at(0x1078).word() } as i32;
        if alignment == 0 { alignment = 4; unsafe { engine.at(0x1078).put(4) }; }
        let format = unsafe { engine.at(0x106c).word() };
        let stride = if format & 0x100 != 0 {
            let bits = alignment.wrapping_shl(3);
            alignment.wrapping_mul(unsafe { signed_quotient(extent.wrapping_sub(1).wrapping_add(bits), bits) })
        } else if format & 0x200 != 0 {
            alignment.wrapping_mul(unsafe { signed_quotient(extent.wrapping_sub(1).wrapping_add(alignment), alignment) })
        } else { 0 };
        unsafe { context.at(0xd08).put(stride as u32); context.at(0xd0c).put(stride.wrapping_mul(height) as u32) };
    }
    0
}

#[derive(Clone, Copy)]
enum FontBackend { Outline, Builtin, Raster, Icon }
unsafe fn engine_backend(engine: Address) -> FontBackend {
    match unsafe { engine.at(0xff4).word() } {
        1 => FontBackend::Outline, 2 => FontBackend::Builtin,
        3..=5 => FontBackend::Raster, 6 | 7 => FontBackend::Icon,
        _ => unsafe { core::hint::unreachable_unchecked() },
    }
}
unsafe extern "C" {
    fn xm1_pipeline_image_font_builtin_height(state: u32) -> i32;
    fn xm1_pipeline_image_font_builtin_width(state: u32) -> i32;
    fn xm1_pipeline_image_font_builtin_secondary_extent(state: u32) -> i32;
    fn xm1_pipeline_image_font_builtin_right_overhang(state: u32, glyph: u16) -> i32;
    fn xm1_pipeline_image_font_builtin_top_overhang(state: u32) -> i32;
    fn xm1_pipeline_image_font_builtin_glyph_width(state: u32, glyph: u16) -> i32;
    fn xm1_pipeline_image_font_icon_height(state: u32) -> i32;
    fn xm1_pipeline_image_font_icon_width(state: u32) -> i32;
    fn xm1_pipeline_image_font_icon_secondary_extent(state: u32) -> i32;
    fn xm1_pipeline_image_font_icon_right_overhang(state: u32, glyph: u16) -> i32;
    fn xm1_pipeline_image_font_icon_top_overhang(state: u32) -> i32;
    fn xm1_pipeline_image_font_icon_glyph_width(state: u32, glyph: u16) -> i32;
    fn xm1_pipeline_image_font_draw_text_bitmap(state: u32, glyph: u16, width: i32, height: i32, x: i32, y: i32, stride: i32, pixels: u32, extra: u32, format: u32) -> i32;
    fn xm1_pipeline_image_font_draw_icon_bitmap(state: u32, glyph: u16, width: i32, height: i32, x: i32, y: i32, stride: i32, pixels: u32, extra: u32, format: u32) -> i32;
    fn xm1_pipeline_image_font_builtin_properties(state: u32, output: u32) -> i32;
    fn xm1_pipeline_image_font_icon_weights() -> i32;
    fn xm1_pipeline_image_font_builtin_stroke_padding(state: u32, first: *mut i32, second: *mut i32, third: *mut i32, fourth: *mut i32) -> i32;
    fn xm1_pipeline_image_font_icon_stroke_padding(state: u32, first: *mut i32, second: *mut i32, third: *mut i32, fourth: *mut i32) -> i32;
}
unsafe fn engine_metric(engine: Address, metric: BackendMetric, glyph: u16) -> i32 {
    let state = unsafe { engine.at(0x68).pointer() };
    match (unsafe { engine_backend(engine) }, metric) {
        (FontBackend::Outline, BackendMetric::Height) => unsafe { xm1_pipeline_image_font_outline_height(state.0) },
        (FontBackend::Outline, BackendMetric::Width) => unsafe { xm1_pipeline_image_font_outline_width(state.0) },
        (FontBackend::Outline, BackendMetric::SecondaryExtent) => unsafe { xm1_pipeline_image_font_outline_secondary_extent(state.0) },
        (FontBackend::Outline, BackendMetric::RightOverhang) => unsafe { xm1_pipeline_image_font_outline_right_overhang(state.0, glyph) },
        (FontBackend::Outline, BackendMetric::TopOverhang) => unsafe { xm1_pipeline_image_font_outline_top_overhang(state.0) },
        (FontBackend::Builtin, BackendMetric::Height) => unsafe { xm1_pipeline_image_font_builtin_height(state.0) },
        (FontBackend::Builtin, BackendMetric::Width) => unsafe { xm1_pipeline_image_font_builtin_width(state.0) },
        (FontBackend::Builtin, BackendMetric::SecondaryExtent) => unsafe { xm1_pipeline_image_font_builtin_secondary_extent(state.0) },
        (FontBackend::Builtin, BackendMetric::RightOverhang) => unsafe { xm1_pipeline_image_font_builtin_right_overhang(state.0, glyph) },
        (FontBackend::Builtin, BackendMetric::TopOverhang) => unsafe { xm1_pipeline_image_font_builtin_top_overhang(state.0) },
        (FontBackend::Raster, BackendMetric::Height) => unsafe { raster_extent(RasterLayout(state), MetricAxis::Height) },
        (FontBackend::Raster, BackendMetric::Width) => unsafe { raster_extent(RasterLayout(state), MetricAxis::Width) },
        (FontBackend::Raster, BackendMetric::SecondaryExtent | BackendMetric::RightOverhang) => unsafe { raster_edge(RasterLayout(state)) },
        (FontBackend::Raster, BackendMetric::TopOverhang) => unsafe { xm1_pipeline_image_font_raster_top_overhang(state.0) },
        (FontBackend::Icon, BackendMetric::Height) => unsafe { xm1_pipeline_image_font_icon_height(state.0) },
        (FontBackend::Icon, BackendMetric::Width) => unsafe { xm1_pipeline_image_font_icon_width(state.0) },
        (FontBackend::Icon, BackendMetric::SecondaryExtent) => unsafe { xm1_pipeline_image_font_icon_secondary_extent(state.0) },
        (FontBackend::Icon, BackendMetric::RightOverhang) => unsafe { xm1_pipeline_image_font_icon_right_overhang(state.0, glyph) },
        (FontBackend::Icon, BackendMetric::TopOverhang) => unsafe { xm1_pipeline_image_font_icon_top_overhang(state.0) },
    }
}

unsafe fn prepare_engine_metrics(engine: Address) -> i32 {
    if engine.0 == 0 { return -1; }
    if unsafe { engine.at(0x1070).word() } != 0 && unsafe { engine.at(0xff4).word() }.wrapping_sub(3) <= 2 {
        loop {
            let status = unsafe { xm1_pipeline_image_font_initialize_raster_layout(engine.at(0x68).word(), engine.at(0x6c).word(), engine.at(0x1030).word(), engine.at(0x1034).word(), engine.0) };
            if status != -6 { if status != 0 { return status; } break; }
            let status = unsafe { xm1_pipeline_image_font_grow_workspace(engine.0) };
            if status != 0 { return status; }
            unsafe { engine.put(engine.at(8).word().wrapping_add(256).wrapping_add(engine.at(0x1030).word())) };
        }
    }
    if unsafe { engine.at(0x68).word() } == 0 { -6 } else { 0 }
}
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_engine_metrics(engine: u32, output: *mut u32) -> i32 {
    let engine = Address(engine);
    let status = unsafe { prepare_engine_metrics(engine) };
    if status != 0 { return status; }
    let height = unsafe { engine_metric(engine, BackendMetric::Height, 0) };
    unsafe { write_volatile(output, height as u32) };
    let edge = unsafe { engine.at(0x1044).word() };
    let scale = unsafe { engine.at(0x1040).word() } as i32;
    let mut ascent = scale.wrapping_mul(860).wrapping_add(999) / 1000;
    if edge == 1 || edge == 2 { ascent = ascent.wrapping_add(unsafe { engine.at(0x1048).word() } as i32); }
    unsafe {
        write_volatile(output.add(1), ascent as u32); write_volatile(output.add(2), height.wrapping_sub(ascent) as u32);
        write_volatile(output.add(3), 0); write_volatile(output.add(4), 0);
        write_volatile(output.add(5), engine_metric(engine, BackendMetric::Width, 0) as u32);
    }
    let secondary = unsafe { engine_metric(engine, BackendMetric::SecondaryExtent, 0) };
    unsafe { write_volatile(output.add(7), 4); write_volatile(output.add(6), secondary as u32) };
    0
}

#[repr(transparent)]
#[derive(Clone, Copy)]
struct FontData(Address);
impl FontData {
    unsafe fn half(self, offset: u32) -> u16 {
        let high = unsafe { self.0.at(offset.wrapping_add(1)).byte() };
        u16::from(unsafe { self.0.at(offset).byte() }) | (u16::from(high) << 8)
    }
    unsafe fn word(self, offset: u32) -> u32 {
        let low = unsafe { self.0.at(offset).byte() };
        let second = unsafe { self.0.at(offset.wrapping_add(1)).byte() };
        let third = unsafe { self.0.at(offset.wrapping_add(2)).byte() };
        let high = unsafe { self.0.at(offset.wrapping_add(3)).byte() };
        u32::from(low) | (u32::from(second) << 8) | (u32::from(third) << 16) | (u32::from(high) << 24)
    }
}
#[repr(transparent)]
#[derive(Clone, Copy)]
pub struct FontFormatCode(u32);
#[repr(transparent)]
#[derive(Clone, Copy)]
pub struct GlyphCount(u32);
#[repr(transparent)]
#[derive(Clone, Copy)]
pub struct FontDirectoryFormat(u32);
#[repr(C)]
pub struct FontBinaryInfo { format: FontFormatCode, glyph_count: GlyphCount, directory_format: FontDirectoryFormat }
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_binary_info(data: u32, info: *mut FontBinaryInfo) -> i32 {
    let data = FontData(Address(data));
    let count = u32::from(unsafe { data.half(0x1c) });
    unsafe { write_volatile(core::ptr::addr_of_mut!((*info).glyph_count), GlyphCount(count)) };
    if count == 0 {
        let offset = unsafe { data.word(0x2e) };
        unsafe { write_volatile(core::ptr::addr_of_mut!((*info).glyph_count), GlyphCount(data.word(offset))) };
    }
    unsafe { write_volatile(core::ptr::addr_of_mut!((*info).format), FontFormatCode(u32::from(data.half(0x36)))) };
    let low = unsafe { data.0.at(0x38).byte() };
    let high = unsafe { data.0.at(0x39).byte() };
    unsafe { write_volatile(core::ptr::addr_of_mut!((*info).directory_format), FontDirectoryFormat(u32::from(low) | (u32::from(high) << 8))) };
    1
}
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_initialize_raster_layout(state: u32, data: u32, bytes: u32, workspace: u32, engine: u32) -> i32 {
    let layout = RasterLayout(Address(state));
    let engine = Address(engine);
    unsafe { layout.0.put(engine.0) };
    let mut info = core::mem::MaybeUninit::<FontBinaryInfo>::uninit();
    if unsafe { xm1_pipeline_image_font_binary_info(data, info.as_mut_ptr()) } == 0 { return -3; }
    if unsafe { info.assume_init() }.format.0 != 4 { return -4; }
    let depth = unsafe { engine.at(0x105c).word() };
    let format = if depth == 1 { 1 } else if depth == 2 { 2 } else { 0 };
    let width = unsafe { engine.at(0x103c).word() } as i32;
    unsafe {
        layout.0.at(4).put(0); layout.0.at(8).put(width as u32);
        let height = engine.at(0x1040).word(); layout.0.at(40).put(format); layout.0.at(12).put(height);
        layout.0.at(16).put(engine.at(0x1044).word()); layout.0.at(20).put(engine.at(0x1048).word());
        layout.0.at(24).put((engine.at(0x104c).word() as i32).min(engine.at(0x103c).word() as i32) as u32);
        layout.0.at(28).put(engine.at(0x1050).word()); layout.0.at(32).put(engine.at(0x1054).word()); layout.0.at(36).put(engine.at(0x1058).word());
    }
    let limit = 105i32.wrapping_sub(width).wrapping_sub(unsafe { raster_edge(layout) });
    unsafe {
        layout.0.at(64).put(data); layout.0.at(68).put(workspace); layout.0.at(72).put(bytes);
        if limit < layout.0.at(36).word() as i32 { layout.0.at(36).put(limit as u32); }
    }
    let format = unsafe { layout.0.at(40).word() };
    unsafe { layout.0.at(0x3d).put_byte(engine.at(0x1074).word() as u8) };
    if format & 1 != 0 {
        let palette = unsafe { engine.at(0x1060).pointer() };
        if palette.0 != 0 && unsafe { palette.byte() } == 17 {
            unsafe { layout.0.at(0x3d).put_byte(2); copy(engine.at(0x1060).pointer(), layout.0.at(44), u32::from(engine.at(0x1060).pointer().byte()) + 1) };
        } else {
            unsafe { layout.0.at(44).put_byte(16) };
            let source = Address(0x1196bb0).at(if unsafe { engine.at(0x1074).word() } == 1 { 0 } else { 96 });
            for offset in 0..16 { unsafe { layout.0.at(45 + offset).put_byte(source.at(offset).byte()) }; }
        }
    }
    match unsafe { xm1_pipeline_image_font_raster_configure(layout.0.at(4).0) } { -4 => -6, 0 => 0, _ => -12 }
}

#[repr(transparent)]
#[derive(Clone, Copy)]
struct RasterProgramContext(Address);
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq)]
struct FontPropertyId(u32);
unsafe fn raster_find_property(context: RasterProgramContext, property: FontPropertyId) -> i32 {
    let offset = unsafe { context.0.at(0x83d4).word() } as i32;
    if offset <= 0 { return 0; }
    let count = unsafe { context.0.at(0x83d8).half() };
    if count == 0 { return 0; }
    let data = FontData(unsafe { context.0.at(0x48).pointer() });
    let mut cursor = (offset as u32).wrapping_add(2);
    let mut index = 0u16;
    loop {
        let identifier = FontPropertyId(u32::from(unsafe { data.half(cursor) }));
        let length = unsafe { data.half(cursor.wrapping_add(2)) };
        if identifier == property { return cursor as i32; }
        index = index.wrapping_add(1);
        cursor = cursor.wrapping_add(u32::from(length));
        if count <= index { return 0; }
    }
}
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_raster_find_property(context: u32, property: u32) -> i32 { unsafe { raster_find_property(RasterProgramContext(Address(context)), FontPropertyId(property)) } }
#[derive(Clone, Copy)]
enum RasterDepth { Mono, Gray4, Gray8 }
impl RasterDepth {
    fn parse(value: u32) -> Option<Self> { match value { 0 => Some(Self::Mono), 1 => Some(Self::Gray4), 2 => Some(Self::Gray8), _ => None } }
    fn code(self) -> u32 { match self { Self::Mono => 0, Self::Gray4 => 1, Self::Gray8 => 2 } }
    fn workspace_size(self, height: i32) -> i32 { 0x83eci32.wrapping_add(height.wrapping_mul(match self { Self::Mono => 0, Self::Gray4 => 104, Self::Gray8 => 224 })) }
}
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_raster_configure(request: u32) -> i32 {
    let request = Address(request);
    let width = unsafe { request.at(4).word() } as i32;
    let context = RasterProgramContext(unsafe { request.at(0x40).pointer() });
    if width > 400 { return -1; }
    let height = unsafe { request.at(8).word() } as i32;
    if height > 400 { return -1; }
    let style = unsafe { request.at(12).word() };
    if style > 5 { return -2; }
    if style != 0 && unsafe { request.at(16).word() } > 10 { return -3; }
    let padding = unsafe { request.at(20).word() } as i32;
    if padding < 0 || width < padding { return -8; }
    let fallback_count = unsafe { request.at(28).word() } as i32;
    if fallback_count < 0 { return -9; }
    if fallback_count > 0 {
        let offset = unsafe { request.at(24).word() } as i32;
        if offset < 0 || offset >= height || offset.wrapping_add(fallback_count) > height || (unsafe { request.at(32).word() } as i32) < 0 { return -9; }
    }
    let Some(depth) = RasterDepth::parse(unsafe { request.at(36).word() }) else { return -5; };
    if matches!(depth, RasterDepth::Gray4) && unsafe { request.at(40).byte() } != 16 { return -6; }
    if unsafe { request.at(60).word() } == 0 { return -7; }
    if depth.workspace_size(height) > unsafe { request.at(68).word() } as i32 { return -4; }
    unsafe { copy(request, context.0, 72) };
    let depth = unsafe { context.0.at(36).word() };
    let scale = unsafe { Address(0x11a2ad4).at(depth * 4).word() };
    let style = unsafe { context.0.at(12).word() };
    let stroke = if style != 0 { unsafe { context.0.at(16).word() }.wrapping_mul(scale) } else { 0 };
    unsafe {
        context.0.at(0x110).put(context.0.at(8).word().wrapping_mul(scale));
        context.0.at(0x13c).put(depth);
        context.0.at(0x10c).put(context.0.at(4).word().wrapping_mul(scale));
        context.0.at(0x118).put(stroke); context.0.at(0x154).put(0); context.0.at(0x158).put(0);
        context.0.at(0x11c).put(context.0.at(20).word().wrapping_mul(scale));
    }
    if depth == RasterDepth::Gray4.code() {
        for offset in 0..17 { unsafe { context.0.at(0x140 + offset).put_byte(request.at(40 + offset).byte()) }; }
    }
    let data = FontData(unsafe { context.0.at(60).pointer() });
    unsafe { context.0.at(0x48).put(data.0.0) };
    for offset in (8..24).step_by(2) { unsafe { context.0.at(0xc4 + offset - 8).put_half(data.half(offset)) }; }
    unsafe {
        context.0.at(0x4c).put(u32::from(data.half(0x18))); context.0.at(0x50).put(u32::from(data.half(0x1a))); context.0.at(0x54).put(u32::from(data.half(0x1c)));
        for (destination, source) in [(0x58, 0x1e), (0x5c, 0x22), (0x60, 0x26), (0x64, 0x2a), (0x68, 0x2e), (0x6c, 0x32)] { context.0.at(destination).put(data.word(source)); }
        context.0.at(0x70).put(u32::from(data.half(0x36)));
        let directory_format = data.half(0x38);
        context.0.at(0x83d4).put(0); context.0.at(0x74).put(u32::from(directory_format)); context.0.at(0x83d8).put_half(0);
        context.0.at(0x83d4).put(data.word(0x3a));
    }
    let properties = unsafe { context.0.at(0x83d4).word() } as i32;
    if properties > 0 {
        unsafe { context.0.at(0x83d8).put_half(data.half(properties as u32)) };
        let adjustment = unsafe { raster_find_property(context, FontPropertyId(10)) };
        unsafe { context.0.at(0x83dc).put(adjustment as u32) };
        if adjustment <= 0 { unsafe { fill(context.0.at(0x83e0), 0, 12) }; }
        else {
            let data = FontData(unsafe { context.0.at(0x48).pointer() });
            for index in 0..4 { unsafe { context.0.at(0x83e0 + index * 2).put_half(data.half((adjustment as u32).wrapping_add(6 + index * 2))) }; }
            unsafe { context.0.at(0x83e8).put((adjustment as u32).wrapping_add(14)) };
        }
    }
    let directory = unsafe { context.0.at(0x68).word() };
    let directory_format = unsafe { context.0.at(0x74).word() };
    if directory_format != 0x101 {
        let count = unsafe { context.0.at(0x54).word() };
        unsafe { context.0.at(0x7c).put_half(u16::MAX) };
        if count != 0 { unsafe { context.0.at(0x78).put(directory) }; return 0; }
        let data = FontData(unsafe { context.0.at(0x48).pointer() });
        let count = unsafe { data.word(directory) };
        unsafe { context.0.at(0x78).put(directory.wrapping_add(4)); context.0.at(0x54).put(count) };
        return 0;
    }
    let data = FontData(unsafe { context.0.at(0x48).pointer() });
    unsafe {
        context.0.at(0x54).put(data.word(directory));
        context.0.at(0x78).put(context.0.at(0x68).word().wrapping_add(data.word(directory.wrapping_add(4))));
        context.0.at(0x7c).put_half(data.half(directory.wrapping_add(8))); context.0.at(0x7e).put_half(data.half(directory.wrapping_add(10)));
        context.0.at(0x84).put(u32::MAX); context.0.at(0x80).put(directory.wrapping_add(12));
    }
    0
}

#[repr(transparent)]
#[derive(Clone, Copy)]
pub(crate) struct GlyphCode(pub(crate) u32);
#[repr(transparent)]
#[derive(Clone, Copy)]
struct GlyphRecordFlags(u8);
#[derive(Clone, Copy)]
enum GlyphBackend { Outline, Raster }
enum GlyphDirectoryEncoding { Compact, Dense, Paged }
impl GlyphDirectoryEncoding {
    fn parse(value: u32) -> Option<Self> { match value { 0 => Some(Self::Compact), 0x100 => Some(Self::Dense), 0x101 => Some(Self::Paged), _ => None } }
}
#[derive(Clone, Copy)]
struct GlyphDirectory { context: Address, backend: GlyphBackend }
impl GlyphDirectory {
    fn field(self, offset: u32) -> Address { self.context.at(offset + match self.backend { GlyphBackend::Outline => 0, GlyphBackend::Raster => 0x40 }) }
    fn orientation(self) -> Address { self.context.at(match self.backend { GlyphBackend::Outline => 4, GlyphBackend::Raster => 0 }) }
}
struct GlyphAdvanceOutputs { horizontal_x: *mut i16, horizontal_y: *mut i16, vertical_x: *mut i16, vertical_y: *mut i16 }
fn arm_lsl(value: u32, count: u32) -> u32 { let count = count & 255; if count < 32 { value << count } else { 0 } }
fn arm_asr(value: i32, count: u32) -> i32 { let count = count & 255; if count < 32 { value >> count } else { value >> 31 } }
unsafe fn font_u24(data: FontData, offset: u32) -> u32 {
    u32::from(unsafe { data.0.at(offset).byte() }) | (u32::from(unsafe { data.0.at(offset.wrapping_add(1)).byte() }) << 8) | (u32::from(unsafe { data.0.at(offset.wrapping_add(2)).byte() }) << 16)
}
unsafe fn plane_glyph_index(directory: GlyphDirectory, data: FontData, code: GlyphCode) -> Option<u32> {
    let plane = (code.0 as i32 >> 16) as u32;
    let low = code.0 & 0xffff;
    if plane != unsafe { directory.field(0x44).word() } {
        let entry = unsafe { directory.field(0x40).word() }.wrapping_add(plane.wrapping_mul(8));
        let mode = unsafe { directory.field(0x3c).half() };
        unsafe { directory.field(0x44).put(plane) };
        let first = unsafe { data.word(entry) };
        let base = unsafe { data.word(entry.wrapping_add(4)) };
        unsafe { directory.field(0x48).put(base) };
        let mapping = first.wrapping_add(unsafe { directory.field(0x28).word() });
        if mode == 0 {
            let count = unsafe { data.half(mapping) };
            unsafe { directory.field(0x5c).put(mapping.wrapping_add(2)); directory.field(0x58).put_half(count) };
        } else if mode == 1 {
            let shift = unsafe { data.half(mapping) };
            unsafe { directory.field(0x4c).put_half(shift) };
            let pages = unsafe { data.half(mapping.wrapping_add(2)) };
            unsafe { directory.field(0x54).put(mapping.wrapping_add(4)); directory.field(0x50).put_half(pages); directory.field(0x4e).put_half(arm_lsl(1, u32::from(shift)).wrapping_sub(1) as u16) };
        }
    }
    if unsafe { directory.field(0x3c).half() } == 0 {
        let mut start = unsafe { directory.field(0x5c).word() };
        let mut count = u32::from(unsafe { directory.field(0x58).half() });
        loop {
            if count == 0 { return None; }
            let middle = count >> 1;
            let record = start.wrapping_add(middle.wrapping_mul(6));
            let first = u32::from(unsafe { data.half(record) });
            let last = u32::from(unsafe { data.half(record.wrapping_add(2)) });
            if first <= low && low <= last {
                return Some(low.wrapping_add(unsafe { directory.field(0x48).word() }).wrapping_sub(first).wrapping_add(u32::from(unsafe { data.half(record.wrapping_add(4)) })));
            }
            if low < first { count = middle; }
            else { count = count.wrapping_sub(middle).wrapping_sub(1); start = record.wrapping_add(6); }
        }
    }
    let shift = u32::from(unsafe { directory.field(0x4c).half() });
    let slot = unsafe { directory.field(0x54).word() }.wrapping_add((arm_asr(low as i32, shift) as u32).wrapping_mul(2));
    let index = unsafe { data.half(slot) };
    if index == u16::MAX { return None; }
    Some((low & u32::from(unsafe { directory.field(0x4e).half() })).wrapping_add(unsafe { directory.field(0x48).word() }).wrapping_add(u32::from(index)))
}
unsafe fn load_glyph(directory: GlyphDirectory, code: GlyphCode, output: GlyphAdvanceOutputs) -> bool {
    let count = unsafe { directory.field(0x14).word() } as i32;
    let orientation = unsafe { directory.orientation().word() };
    if code.0 as i32 >= count { return false; }
    let Some(format) = GlyphDirectoryEncoding::parse(unsafe { directory.field(0x34).word() }) else { return false; };
    let data = FontData(unsafe { directory.field(8).pointer() });
    let records = unsafe { directory.field(0x38).word() };
    let (begin, end, mut flags, prefixed) = match format {
        GlyphDirectoryEncoding::Compact => {
            let entry = code.0.wrapping_mul(3).wrapping_add(records);
            (unsafe { font_u24(data, entry) }, unsafe { font_u24(data, entry.wrapping_add(3)) }, GlyphRecordFlags(0), false)
        }
        GlyphDirectoryEncoding::Dense => {
            let entry = records.wrapping_add(code.0.wrapping_mul(4));
            let packed = unsafe { data.word(entry) };
            let end = match directory.backend { GlyphBackend::Outline => unsafe { font_u24(data, entry.wrapping_add(4)) }, GlyphBackend::Raster => (unsafe { data.word(entry.wrapping_add(4)) }) & 0xffffff };
            (packed & 0xffffff, end, GlyphRecordFlags((packed >> 24) as u8), false)
        }
        GlyphDirectoryEncoding::Paged => {
            let Some(index) = (unsafe { plane_glyph_index(directory, data, code) }) else { return false; };
            let entry = index.wrapping_mul(3).wrapping_add(unsafe { directory.field(0x38).word() });
            (unsafe { font_u24(data, entry) }, unsafe { font_u24(data, entry.wrapping_add(3)) }, GlyphRecordFlags(0), true)
        }
    };
    let mut remaining = end.wrapping_sub(begin);
    if remaining == 0 { return false; }
    let mut cursor = begin.wrapping_add(unsafe { directory.field(0x2c).word() });
    unsafe { write_volatile(output.vertical_y, 0); write_volatile(output.vertical_x, 0); write_volatile(output.horizontal_y, 0); write_volatile(output.horizontal_x, 0) };
    if prefixed {
        remaining = remaining.wrapping_sub(1);
        flags = GlyphRecordFlags(unsafe { data.0.at(cursor).byte() });
        if remaining == 0 { return false; }
        cursor = cursor.wrapping_add(1);
    }
    let vertical = orientation != 0 && (code.0 as i32) > 255;
    if flags.0 & 1 != 0 {
        if !vertical { unsafe { write_volatile(output.horizontal_x, data.half(cursor) as i16); write_volatile(output.horizontal_y, data.half(cursor.wrapping_add(2)) as i16) }; }
        remaining = remaining.wrapping_sub(4); cursor = cursor.wrapping_add(4);
    }
    if flags.0 & 2 != 0 {
        if vertical { unsafe { write_volatile(output.vertical_x, data.half(cursor) as i16); write_volatile(output.vertical_y, data.half(cursor.wrapping_add(2)) as i16) }; }
        remaining = remaining.wrapping_sub(4); cursor = cursor.wrapping_add(4);
    }
    if flags.0 != 0 {
        let primary_length = u32::from(unsafe { data.half(cursor) });
        cursor = cursor.wrapping_add(2);
        if vertical && flags.0 & 4 != 0 && (remaining.wrapping_sub(primary_length) as i32) > 0 {
            let length = u32::from(unsafe { data.half(cursor.wrapping_add(primary_length)) });
            if length != 0 { cursor = cursor.wrapping_add(2).wrapping_add(primary_length); remaining = length; }
            else { remaining = primary_length; }
        } else { remaining = primary_length; }
    }
    match directory.backend {
        GlyphBackend::Outline => unsafe { directory.field(0x64).put(cursor); directory.field(0x60).put(remaining) },
        GlyphBackend::Raster => unsafe {
            directory.field(0x60).put(remaining);
            directory.context.at(0x129).put_byte(u8::from(orientation != 0 && (code.0 as i32) <= 255));
            directory.field(0x64).put(cursor); directory.context.at(0x128).put_byte(orientation as u8);
        },
    }
    true
}
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_program_load_glyph(context: u32, code: u32, horizontal_x: *mut i16, horizontal_y: *mut i16, vertical_x: *mut i16, vertical_y: *mut i16) -> i32 {
    i32::from(unsafe { load_glyph(GlyphDirectory { context: Address(context), backend: GlyphBackend::Outline }, GlyphCode(code), GlyphAdvanceOutputs { horizontal_x, horizontal_y, vertical_x, vertical_y }) })
}
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_raster_load_glyph(context: u32, code: u32, horizontal_x: *mut i16, horizontal_y: *mut i16, vertical_x: *mut i16, vertical_y: *mut i16) -> i32 {
    i32::from(unsafe { load_glyph(GlyphDirectory { context: Address(context), backend: GlyphBackend::Raster }, GlyphCode(code), GlyphAdvanceOutputs { horizontal_x, horizontal_y, vertical_x, vertical_y }) })
}

#[repr(transparent)]
#[derive(Clone, Copy)]
struct OutlineProgram(Address);
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_program_initialize(data: u32, bytes: u32, workspace: u32) -> u32 {
    if bytes < 0x814 || workspace == 0 { return 0; }
    let data = FontData(Address(data));
    let program = OutlineProgram(Address(workspace));
    unsafe { program.0.at(8).put(data.0.0) };
    for offset in (8..24).step_by(2) { unsafe { program.0.at(0x80 + offset - 8).put_half(data.half(offset)) }; }
    unsafe {
        program.0.at(0xc).put(u32::from(data.half(0x18))); program.0.at(0x10).put(u32::from(data.half(0x1a))); program.0.at(0x14).put(u32::from(data.half(0x1c)));
        for (destination, source) in [(0x18, 0x1e), (0x1c, 0x22), (0x20, 0x26), (0x24, 0x2a), (0x28, 0x2e), (0x2c, 0x32)] { program.0.at(destination).put(data.word(source)); }
        program.0.at(0x30).put(u32::from(data.half(0x36))); program.0.at(0x34).put(u32::from(data.half(0x38)));
    }
    let directory = unsafe { program.0.at(0x28).word() };
    if unsafe { program.0.at(0x34).word() } == 0x101 {
        unsafe {
            program.0.at(0x14).put(data.word(directory));
            program.0.at(0x38).put(directory.wrapping_add(data.word(directory.wrapping_add(4))));
            program.0.at(0x3c).put_half(data.half(directory.wrapping_add(8)));
            let pages = data.half(directory.wrapping_add(10));
            program.0.at(0x40).put(directory.wrapping_add(12)); program.0.at(0x3e).put_half(pages); program.0.at(0x44).put(u32::MAX);
        }
    } else {
        let count = unsafe { program.0.at(0x14).word() };
        if count != 0 { unsafe { program.0.at(0x38).put(directory) }; }
        unsafe { program.0.at(0x3c).put_half(u16::MAX) };
        if count == 0 {
            let count = unsafe { data.word(directory) };
            unsafe { program.0.at(0x38).put(directory.wrapping_add(4)); program.0.at(0x14).put(count) };
        }
    }
    workspace
}
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_program_read_byte(program: u32) -> u8 {
    let program = OutlineProgram(Address(program));
    let data = unsafe { program.0.at(8).pointer() };
    let cursor = unsafe { program.0.at(0xb4).word() };
    let value = unsafe { data.at(cursor).byte() };
    unsafe { program.0.at(0xb4).put(cursor.wrapping_add(1)); program.0.at(0xb0).put(program.0.at(0xb0).word().wrapping_sub(1)) };
    value
}
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_program_read_short(program: u32) -> i16 {
    let program = OutlineProgram(Address(program));
    let cursor = unsafe { program.0.at(0xb4).word() };
    let data = unsafe { program.0.at(8).pointer() };
    let low = unsafe { data.at(cursor).byte() };
    let high = unsafe { data.at(cursor.wrapping_add(1)).byte() };
    unsafe { program.0.at(0xb4).put(cursor.wrapping_add(2)); program.0.at(0xb0).put(program.0.at(0xb0).word().wrapping_sub(2)) };
    (u16::from(low) | (u16::from(high) << 8)) as i16
}

unsafe extern "C" { fn xm1_font_division_fault(saturated_numerator: i32) -> !; }
pub(crate) unsafe fn signed_quotient(numerator: i32, denominator: i32) -> i32 {
    if denominator == 0 { unsafe { xm1_font_division_fault(if numerator > 0 { i32::MAX } else if numerator < 0 { i32::MIN } else { 0 }) }; }
    numerator.wrapping_div(denominator)
}
#[derive(Clone, Copy)]
struct RasterRequestSnapshot { width: u32, height: u32, style: u32, padding: u32 }
impl RasterRequestSnapshot {
    unsafe fn read(request: Address) -> Self { unsafe { Self { width: request.at(4).word(), height: request.at(8).word(), style: request.at(12).word(), padding: request.at(20).word() } } }
    unsafe fn restore(self, request: Address) {
        unsafe { request.at(4).put(self.width); request.at(8).put(self.height); request.at(12).put(self.style); request.at(20).put(self.padding); xm1_pipeline_image_font_raster_configure(request.0) };
    }
}
#[repr(transparent)]
#[derive(Clone, Copy)]
struct GlyphAdjustmentIndex(u32);
unsafe fn glyph_adjustment_index(context: RasterProgramContext, code: GlyphCode, format: u32) -> Option<GlyphAdjustmentIndex> {
    if format != 4 || (unsafe { context.0.at(0x83d4).word() } as i32) <= 0 || unsafe { context.0.at(0x83d8).half() } == 0 { return None; }
    let first = u32::from(unsafe { context.0.at(0x83e0).half() });
    if (code.0 as i32) < first as i32 || (code.0 as i32) > i32::from(unsafe { context.0.at(0x83e2).half() }) { return None; }
    Some(GlyphAdjustmentIndex(code.0.wrapping_sub(first)))
}
unsafe fn rescale_raster_request(request: Address, context: RasterProgramContext, index: GlyphAdjustmentIndex, height: i32) -> i32 {
    let data = FontData(unsafe { context.0.at(0x48).pointer() });
    let offset = unsafe { context.0.at(0x83e8).word() }.wrapping_add(index.0.wrapping_mul(2));
    let per_mille = i32::from(unsafe { data.half(offset) });
    let size = unsafe { signed_quotient(height.wrapping_mul(1000).wrapping_sub(1).wrapping_add(per_mille), per_mille) };
    unsafe { request.at(4).put(size as u32); request.at(8).put(size as u32); xm1_pipeline_image_font_raster_configure(request.0) }
}
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_raster_adjust_size(request: u32, glyph: u32, adjusted: *mut i32, context: u32) -> i32 {
    let request = Address(request);
    let context = RasterProgramContext(Address(context));
    let saved = unsafe { RasterRequestSnapshot::read(request) };
    let format = unsafe { context.0.at(0x70).word() };
    unsafe { write_volatile(adjusted, 0) };
    let Some(index) = (unsafe { glyph_adjustment_index(context, GlyphCode(glyph), format) }) else { return 0; };
    unsafe { write_volatile(adjusted, 1) };
    let status = unsafe { rescale_raster_request(request, context, index, request.at(8).word() as i32) };
    if status != 0 { unsafe { saved.restore(request) }; }
    status
}
#[repr(C)]
struct GlyphAdvances { horizontal_x: i16, horizontal_y: i16, vertical_x: i16, vertical_y: i16 }
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_raster_glyph_advance(request: u32, glyph: u32) -> i32 {
    let request = Address(request);
    let context = RasterProgramContext(unsafe { request.at(64).pointer() });
    let saved = unsafe { RasterRequestSnapshot::read(request) };
    let adjustment = unsafe { glyph_adjustment_index(context, GlyphCode(glyph), context.0.at(0x70).word()) };
    if let Some(index) = adjustment {
        let status = unsafe { rescale_raster_request(request, context, index, saved.height as i32) };
        if status != 0 { unsafe { saved.restore(request) }; return 0; }
    }
    let mut advances = core::mem::MaybeUninit::<GlyphAdvances>::uninit();
    let output = advances.as_mut_ptr();
    let present = unsafe { load_glyph(GlyphDirectory { context: context.0, backend: GlyphBackend::Raster }, GlyphCode(glyph), GlyphAdvanceOutputs {
        horizontal_x: core::ptr::addr_of_mut!((*output).horizontal_x), horizontal_y: core::ptr::addr_of_mut!((*output).horizontal_y),
        vertical_x: core::ptr::addr_of_mut!((*output).vertical_x), vertical_y: core::ptr::addr_of_mut!((*output).vertical_y),
    }) };
    let result = if !present { 0 } else {
        let advances = unsafe { advances.assume_init() };
        let vertical = unsafe { request.word() } != 0;
        let (dimension, advance) = if !vertical { (unsafe { request.at(4).word() }, advances.horizontal_y) }
        else { (unsafe { request.at(8).word() }, if unsafe { context.0.at(0x129).byte() } != 0 { advances.horizontal_y } else { advances.vertical_y }) };
        let mut width = if advance == 0 { dimension as u16 } else { ((dimension as i32).wrapping_mul(i32::from(advance)).wrapping_add(999) / 1000) as u16 };
        let style = unsafe { request.at(12).word() };
        let stroke = unsafe { request.at(16).word() };
        if style == 1 || style == 2 { width = width.wrapping_add(stroke.wrapping_mul(2) as u16); }
        else if (!vertical && (style == 3 || style == 5)) || (vertical && (style == 4 || style == 5)) { width = width.wrapping_add(stroke as u16); }
        i32::from(width as i16)
    };
    if adjustment.is_some() { unsafe { saved.restore(request) }; }
    result
}

#[repr(transparent)]
#[derive(Clone, Copy)]
pub(crate) struct FontEngine(pub(crate) u32);
#[repr(transparent)]
#[derive(Clone, Copy)]
pub(crate) struct FontBitmapFormat(pub(crate) u32);
pub(crate) struct GlyphDraw {
    pub(crate) glyph: GlyphCode, pub(crate) width: i32, pub(crate) height: i32,
    pub(crate) x: i32, pub(crate) y: i32, pub(crate) stride: i32,
    pub(crate) pixels: Address32, pub(crate) format: FontBitmapFormat,
}
impl FontEngine {
    pub(crate) unsafe fn metric(self, metric: BackendMetric, glyph: GlyphCode) -> i32 { unsafe { engine_metric(Address(self.0), metric, glyph.0 as u16) } }
    pub(crate) unsafe fn glyph_advance(self, glyph: GlyphCode) -> i32 {
        let engine = Address(self.0);
        let state = unsafe { engine.at(0x68).word() };
        match unsafe { engine_backend(engine) } {
            FontBackend::Outline => unsafe { xm1_pipeline_image_font_handle_glyph_width(state, glyph.0 as u16) },
            FontBackend::Builtin => unsafe { xm1_pipeline_image_font_builtin_glyph_width(state, glyph.0 as u16) },
            FontBackend::Raster => unsafe { xm1_pipeline_image_font_chained_glyph_width(state, glyph.0 as u16) },
            FontBackend::Icon => unsafe { xm1_pipeline_image_font_icon_glyph_width(state, glyph.0 as u16) },
        }
    }
    pub(crate) unsafe fn append_outline(self, glyph: GlyphCode, length: *mut u32, buffer: *mut u32, words: *mut u32) -> i32 {
        unsafe { self.append_outline_clipped(glyph, Address32(0), length, buffer, words) }
    }
    pub(crate) unsafe fn append_outline_clipped(self, glyph: GlyphCode, clip: Address32, length: *mut u32, buffer: *mut u32, words: *mut u32) -> i32 {
        unsafe { xm1_pipeline_image_font_handle_append_bitmap(Address(self.0).at(0x68).word(), glyph.0 as u16, clip.0, length, buffer, words) }
    }
    pub(crate) unsafe fn draw(self, request: GlyphDraw) -> i32 {
        unsafe { self.draw_clipped(request, Address32(0)) }
    }
    pub(crate) unsafe fn draw_clipped(self, request: GlyphDraw, clip: Address32) -> i32 {
        let engine = Address(self.0);
        let state = unsafe { engine.at(0x68).word() };
        match unsafe { engine_backend(engine) } {
            FontBackend::Outline => unsafe { core::hint::unreachable_unchecked() },
            FontBackend::Builtin => unsafe { xm1_pipeline_image_font_draw_text_bitmap(state, request.glyph.0 as u16, request.width, request.height, request.x, request.y, request.stride, request.pixels.0, clip.0, request.format.0) },
            FontBackend::Raster => unsafe { xm1_pipeline_image_font_draw_raster_bitmap(state, request.glyph.0 as u16, request.width, request.height, request.x, request.y, request.stride, request.pixels.0, clip.0, request.format.0) },
            FontBackend::Icon => unsafe { xm1_pipeline_image_font_draw_icon_bitmap(state, request.glyph.0 as u16, request.width, request.height, request.x, request.y, request.stride, request.pixels.0, clip.0, request.format.0) },
        }
    }
    pub(crate) unsafe fn thai_metrics(self, destination: Address32) {
        let engine = Address(self.0);
        let state = unsafe { engine.at(0x68).word() };
        match unsafe { engine_backend(engine) } {
            FontBackend::Outline => { unsafe { xm1_pipeline_image_font_handle_weights(state, destination.0 as *mut i16) }; }
            FontBackend::Builtin => { unsafe { xm1_pipeline_image_font_builtin_properties(state, destination.0) }; }
            FontBackend::Raster => { unsafe { xm1_pipeline_image_font_raster_weights(state, destination.0 as *mut i16) }; }
            FontBackend::Icon => { unsafe { xm1_pipeline_image_font_icon_weights() }; }
        }
    }
    pub(crate) unsafe fn stroke_padding(self, left: Address32, top: Address32, right: Address32, bottom: Address32) {
        let engine = Address(self.0);
        let state = unsafe { engine.at(0x68).word() };
        match unsafe { engine_backend(engine) } {
            FontBackend::Outline => { unsafe { xm1_pipeline_image_font_outline_stroke_padding(state, left.0 as *mut i32, top.0 as *mut i32, right.0 as *mut i32, bottom.0 as *mut i32) }; }
            FontBackend::Builtin => { unsafe { xm1_pipeline_image_font_builtin_stroke_padding(state, left.0 as *mut i32, top.0 as *mut i32, right.0 as *mut i32, bottom.0 as *mut i32) }; }
            FontBackend::Raster => { unsafe { xm1_pipeline_image_font_raster_stroke_padding(state, left.0 as *mut i32, top.0 as *mut i32, right.0 as *mut i32, bottom.0 as *mut i32) }; }
            FontBackend::Icon => { unsafe { xm1_pipeline_image_font_icon_stroke_padding(state, left.0 as *mut i32, top.0 as *mut i32, right.0 as *mut i32, bottom.0 as *mut i32) }; }
        }
    }
}
unsafe fn cached_raster_height_edge(layout: RasterLayout, primary: i32, fallback_count: i32) -> i32 {
    if primary <= 0 && fallback_count <= 0 { return 0; }
    let fallback = if fallback_count > 0 { (unsafe { layout.0.at(36).word() }) as i32 } else { 0 };
    if unsafe { layout.0.at(4).word() } != 0 { primary.max(fallback) } else { 0 }
}
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_raster_top_overhang(state: u32) -> i32 {
    let layout = RasterLayout(Address(state));
    let style = unsafe { layout.0.at(16).word() };
    if style == 0 { return 0; }
    let stroke = unsafe { layout.0.at(20).word() } as i32;
    let height = unsafe { layout.0.at(12).word() } as i32;
    let styled = match style { 1 | 2 => height.wrapping_add(stroke.wrapping_mul(2)), 3 => height, 4 | 5 => height.wrapping_add(stroke), _ => 0 };
    let primary = unsafe { layout.0.at(24).word() } as i32;
    let fallback_count = unsafe { layout.0.at(32).word() } as i32;
    let styled = styled.wrapping_add(unsafe { cached_raster_height_edge(layout, primary, fallback_count) });
    unsafe { layout.0.at(16).put(0) };
    let plain = height.wrapping_add(unsafe { cached_raster_height_edge(layout, primary, fallback_count) });
    unsafe { layout.0.at(16).put(style) };
    styled.wrapping_sub(plain)
}

unsafe fn zero_optional(pointer: *mut i32) { if !pointer.is_null() { unsafe { write_volatile(pointer, 0) }; } }
unsafe fn stroke_optional(layout: RasterLayout, pointer: *mut i32) { if !pointer.is_null() { unsafe { write_volatile(pointer, layout.0.at(20).word() as i32) }; } }
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_outline_stroke_padding(_state: u32, first: *mut i32, second: *mut i32, third: *mut i32, fourth: *mut i32) -> i32 {
    unsafe { zero_optional(first); zero_optional(second); zero_optional(third); zero_optional(fourth) };
    0
}
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_raster_stroke_padding(state: u32, first: *mut i32, second: *mut i32, third: *mut i32, fourth: *mut i32) -> i32 {
    unsafe { zero_optional(first); zero_optional(second); zero_optional(third); zero_optional(fourth) };
    let layout = RasterLayout(Address(state));
    match unsafe { layout.0.at(16).word() } {
        1 | 2 => unsafe { stroke_optional(layout, first); stroke_optional(layout, second); stroke_optional(layout, third); stroke_optional(layout, fourth) },
        3 => unsafe { stroke_optional(layout, fourth) },
        4 => unsafe { stroke_optional(layout, second) },
        5 => unsafe { stroke_optional(layout, fourth); stroke_optional(layout, second) },
        _ => {}
    }
    0
}
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_raster_secondary_extent(state: u32) -> i32 { unsafe { raster_edge(RasterLayout(Address(state))) } }
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_binary_read(data: u32, offset: u32, count: i32) -> u32 {
    let start = Address(data).at(offset);
    let mut value = u32::from(unsafe { start.byte() });
    if count > 1 {
        for index in 1..count as u32 { value = value.wrapping_add(arm_lsl(u32::from(unsafe { start.at(index).byte() }), index.wrapping_mul(8))); }
    }
    value
}

#[repr(C)]
pub struct OutlineGlyphRequest { glyph: u32, orientation: i32, bitmap: u32, capacity: i32, words: i32 }
unsafe fn initialize_outline_request(request: *mut OutlineGlyphRequest, glyph: GlyphCode, orientation: i32, bitmap: Address, capacity: i32) {
    unsafe {
        write_volatile(core::ptr::addr_of_mut!((*request).glyph), glyph.0);
        write_volatile(core::ptr::addr_of_mut!((*request).orientation), orientation);
        write_volatile(core::ptr::addr_of_mut!((*request).bitmap), bitmap.0);
        write_volatile(core::ptr::addr_of_mut!((*request).capacity), capacity);
    }
}
unsafe extern "C" { fn xm1_pipeline_image_font_render_outline_program(program: u32, transform: u32, stream: u32) -> i32; }
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_program_glyph_width(program: u32, glyph: u32, orientation: i32) -> i32 {
    let program = OutlineProgram(Address(program));
    if program.0.0 == 0 { return -1; }
    unsafe { program.0.at(4).put(u32::from(orientation != 0 && (glyph as i32) > 255)) };
    let mut advances = core::mem::MaybeUninit::<GlyphAdvances>::uninit();
    let output = advances.as_mut_ptr();
    let present = unsafe { load_glyph(GlyphDirectory { context: program.0, backend: GlyphBackend::Outline }, GlyphCode(glyph), GlyphAdvanceOutputs {
        horizontal_x: core::ptr::addr_of_mut!((*output).horizontal_x), horizontal_y: core::ptr::addr_of_mut!((*output).horizontal_y),
        vertical_x: core::ptr::addr_of_mut!((*output).vertical_x), vertical_y: core::ptr::addr_of_mut!((*output).vertical_y),
    }) };
    if !present { return -2; }
    let advances = unsafe { advances.assume_init() };
    let value = if unsafe { program.0.at(4).word() } != 0 { advances.vertical_y } else { advances.horizontal_y };
    if value == 0 { 2048 } else { i32::from(((i32::from(value).wrapping_shl(11).wrapping_add(999)) / 1000) as i16) }
}
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_program_render(program: u32, request: *mut OutlineGlyphRequest) -> i32 {
    let program = OutlineProgram(Address(program));
    if program.0.0 == 0 { return -1; }
    unsafe { program.0.at(0x78).put(program.0.0); program.0.at(0x7c).put(0); program.0.at(4).put(0) };
    let orientation = unsafe { read_volatile(core::ptr::addr_of!((*request).orientation)) };
    let glyph = unsafe { read_volatile(core::ptr::addr_of!((*request).glyph)) };
    if orientation != 0 { unsafe { program.0.at(if glyph as i32 <= 255 { 0x7c } else { 4 }).put(1) }; }
    unsafe {
        program.0.at(0xcc).put(read_volatile(core::ptr::addr_of!((*request).capacity)) as u32);
        program.0.at(0xc8).put(read_volatile(core::ptr::addr_of!((*request).bitmap))); program.0.at(0xd0).put(0);
    }
    let mut advances = core::mem::MaybeUninit::<GlyphAdvances>::uninit();
    let output = advances.as_mut_ptr();
    let present = unsafe { load_glyph(GlyphDirectory { context: program.0, backend: GlyphBackend::Outline }, GlyphCode(glyph), GlyphAdvanceOutputs {
        horizontal_x: core::ptr::addr_of_mut!((*output).horizontal_x), horizontal_y: core::ptr::addr_of_mut!((*output).horizontal_y),
        vertical_x: core::ptr::addr_of_mut!((*output).vertical_x), vertical_y: core::ptr::addr_of_mut!((*output).vertical_y),
    }) };
    if !present { return -2; }
    let advances = unsafe { advances.assume_init() };
    let advance_x = (i32::from(advances.horizontal_x).wrapping_shl(11) / 1000) as i16;
    let advance_y = (i32::from(advances.vertical_x).wrapping_shl(11) / 1000) as i16;
    unsafe { program.0.at(0xb8).put(i32::from(advance_x) as u32); program.0.at(0xbc).put(i32::from(advance_y) as u32); program.0.at(0xd4).put(0); program.0.at(0xd8).put(0) };
    if (unsafe { program.0.at(0x60).word() } as i32) <= 0 { return -2; }
    let status = unsafe { xm1_pipeline_image_font_render_outline_program(program.0.0, 0, 0) };
    if status != 0 { return status; }
    let words = unsafe { program.0.at(0xd0).word() } as i32;
    let capacity = unsafe { read_volatile(core::ptr::addr_of!((*request).capacity)) };
    unsafe { write_volatile(core::ptr::addr_of_mut!((*request).words), words) };
    if words <= capacity { 0 } else { -4 }
}
#[repr(transparent)]
#[derive(Clone, Copy)]
struct OutlineHandle(Address);
impl OutlineHandle {
    unsafe fn engine(self) -> Address { unsafe { self.0.pointer() } }
    unsafe fn program(self) -> u32 { unsafe { self.0.at(4).word() } }
    unsafe fn initialize(self, data: Address, engine: Address) {
        let program = unsafe { xm1_pipeline_image_font_program_initialize(data.0, engine.at(0x1030).word(), engine.at(0x1034).word()) };
        unsafe { self.0.at(4).put(program) };
    }
}
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_handle_query_width(state: u32, glyph: u32, orientation: i32) -> u32 {
    let handle = OutlineHandle(Address(state));
    let engine = unsafe { handle.engine() };
    let fallback = unsafe { engine.at(0x1c).pointer() };
    let mut width = unsafe { xm1_pipeline_image_font_program_glyph_width(handle.program(), glyph, orientation) } as u32;
    if width.wrapping_add(2) < 2 {
        if (unsafe { engine.at(32).word() } as i32) <= 0 { return 0; }
        unsafe { handle.initialize(fallback.at(100).pointer(), engine) };
        width = unsafe { xm1_pipeline_image_font_program_glyph_width(handle.program(), glyph, orientation) } as u32;
        if width.wrapping_add(2) < 2 { width = 0; }
        unsafe { handle.initialize(handle.0.at(8).pointer(), engine) };
    }
    width
}
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_handle_render(state: u32, width: *mut i32, request: *mut OutlineGlyphRequest) -> i32 {
    let handle = OutlineHandle(Address(state));
    let engine = unsafe { handle.engine() };
    let fallback = unsafe { engine.at(0x1c).pointer() };
    let glyph = unsafe { read_volatile(core::ptr::addr_of!((*request).glyph)) };
    let orientation = unsafe { read_volatile(core::ptr::addr_of!((*request).orientation)) };
    let value = unsafe { xm1_pipeline_image_font_program_glyph_width(handle.program(), glyph, orientation) };
    unsafe { write_volatile(width, value) };
    if (value as u32).wrapping_add(2) < 2 {
        unsafe { write_volatile(width, 0) };
        if (unsafe { engine.at(32).word() } as i32) > 0 {
            unsafe { handle.initialize(fallback.at(100).pointer(), engine) };
            let value = unsafe { xm1_pipeline_image_font_program_glyph_width(handle.program(), glyph, orientation) };
            unsafe { write_volatile(width, value) };
            let status = unsafe { xm1_pipeline_image_font_program_render(handle.program(), request) };
            unsafe { handle.initialize(handle.0.at(8).pointer(), engine) };
            return status;
        }
    }
    unsafe { xm1_pipeline_image_font_program_render(handle.program(), request) }
}
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_handle_glyph_width(state: u32, glyph: u16) -> i32 {
    let handle = OutlineHandle(Address(state));
    let engine = unsafe { handle.engine() };
    let bias = unsafe { engine.at(0x1068).word() } as i32;
    let units = unsafe { engine.at(0x103c).word() } as i32;
    let width = unsafe { xm1_pipeline_image_font_handle_query_width(state, u32::from(glyph), 0) };
    if width == 0 { return 0; }
    (width as i32).wrapping_add(unsafe { signed_quotient(bias.wrapping_shl(11), units) })
}
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_handle_append_bitmap(state: u32, glyph: u16, _clip: u32, length: *mut u32, buffer: *mut u32, words: *mut u32) -> i32 {
    let handle = OutlineHandle(Address(state));
    let engine = unsafe { handle.engine() };
    let scratch = unsafe { engine.at(24).pointer().at(0x70).pointer() };
    if (unsafe { scratch.at(12).word() } as i32) < 8192 { return -10; }
    let pixels = unsafe { scratch.at(16).pointer() };
    unsafe { fill(pixels, 0, 8192) };
    let mut request = core::mem::MaybeUninit::<OutlineGlyphRequest>::uninit();
    let request = request.as_mut_ptr();
    unsafe { initialize_outline_request(request, GlyphCode(u32::from(glyph)), handle.0.at(12).word() as i32, pixels, 4096) };
    let mut advance = 0;
    let status = unsafe { xm1_pipeline_image_font_handle_render(state, &mut advance, request) };
    if status != 0 {
        if status == -2 { unsafe { write_volatile(words, 0) }; return 0; }
        return status;
    }
    let count = unsafe { read_volatile(core::ptr::addr_of!((*request).words)) } as u32;
    let old_length = unsafe { read_volatile(length) };
    let required = old_length.wrapping_add(count.wrapping_mul(2));
    let mut copied_length = old_length;
    if (required as i32) > unsafe { engine.at(0xfe4).word() } as i32 {
        let rounded = required.wrapping_add(1023) as i32;
        let rounded = if rounded < 0 { rounded.wrapping_add(1023) } else { rounded };
        let allocation = unsafe { xm1_pipeline_image_font_resize(read_volatile(buffer), ((rounded >> 10) << 10) as u32) };
        if allocation == 0 { return -10; }
        unsafe { engine.at(0xfe4).put(required); copied_length = read_volatile(length); write_volatile(buffer, allocation) };
    }
    unsafe { copy(pixels, Address(read_volatile(buffer)).at(copied_length), count.wrapping_mul(2)); write_volatile(length, required); write_volatile(words, count) };
    0
}
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_handle_bitmap(state: u32, glyph: u16, size: u32, bitmap: u32, words: *mut u32) -> i32 {
    unsafe { fill(Address(bitmap), 0, size) };
    let mut request = core::mem::MaybeUninit::<OutlineGlyphRequest>::uninit();
    let request = request.as_mut_ptr();
    unsafe { initialize_outline_request(request, GlyphCode(u32::from(glyph)), Address(state).at(12).word() as i32, Address(bitmap), (size >> 1) as i32) };
    let mut advance = 0;
    let status = unsafe { xm1_pipeline_image_font_handle_render(state, &mut advance, request) };
    if status == 0 { unsafe { write_volatile(words, read_volatile(core::ptr::addr_of!((*request).words)) as u32) }; }
    else if status == -2 { unsafe { write_volatile(words, 0) }; return 0; }
    status
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RasterProperty { Scale, Adjusted, Width, Height, Weights, Adjustments, Other }
impl RasterProperty {
    fn parse(value: u32) -> Self { match value { 1 => Self::Scale, 2 => Self::Adjusted, 4 => Self::Width, 5 => Self::Height, 6 => Self::Weights, 10 => Self::Adjustments, _ => Self::Other } }
}
unsafe fn find_data_property(data: FontData, property: FontPropertyId) -> u32 {
    let offset = unsafe { data.word(0x3a) };
    if (offset as i32) <= 0 { return 0; }
    let count = unsafe { data.half(offset) };
    let mut cursor = offset.wrapping_add(2);
    for _ in 0..count {
        let identifier = FontPropertyId(u32::from(unsafe { data.half(cursor) }));
        let length = unsafe { data.half(cursor.wrapping_add(2)) };
        if identifier == property { return cursor; }
        cursor = cursor.wrapping_add(u32::from(length));
    }
    0
}
unsafe fn raster_property(request: Address, glyph: GlyphCode, property: RasterProperty) -> u32 {
    let data = FontData(unsafe { request.at(60).pointer() });
    let mut adjusted = false;
    let mut coefficient = 1000;
    if unsafe { data.half(0x36) } == 4 {
        let entry = unsafe { find_data_property(data, FontPropertyId(10)) };
        if (entry as i32) > 0 {
            let start = u32::from(unsafe { data.half(entry.wrapping_add(6)) });
            let end = u32::from(unsafe { data.half(entry.wrapping_add(8)) });
            if (glyph.0 as i32) >= start as i32 && (glyph.0 as i32) <= end as i32 {
                adjusted = true;
                coefficient = u32::from(unsafe { data.half(entry.wrapping_add(14).wrapping_add(glyph.0.wrapping_sub(start).wrapping_mul(2))) });
            }
        }
    }
    let height = unsafe { request.at(8).word() };
    let extent = unsafe { signed_quotient(height.wrapping_mul(1000).wrapping_sub(1).wrapping_add(coefficient) as i32, coefficient as i32) } as u32;
    match property {
        RasterProperty::Scale => coefficient, RasterProperty::Adjusted => u32::from(adjusted),
        RasterProperty::Width => if adjusted { extent } else { unsafe { request.at(4).word() } },
        RasterProperty::Height => if adjusted { extent } else { height },
        RasterProperty::Weights => unsafe { find_data_property(data, FontPropertyId(6)) },
        RasterProperty::Adjustments => unsafe { find_data_property(data, FontPropertyId(10)) },
        RasterProperty::Other => 0,
    }
}
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_raster_property(request: u32, glyph: u32, property: u32) -> u32 {
    unsafe { raster_property(Address(request), GlyphCode(glyph), RasterProperty::parse(property)) }
}
unsafe fn query_raster_fallback(layout: RasterLayout, request: Address, glyph: GlyphCode, property: RasterProperty) -> u32 {
    let engine = unsafe { layout.0.pointer() };
    let fallback = unsafe { engine.at(28).pointer() };
    let primary = unsafe { request.at(60).word() };
    let mut value = unsafe { raster_property(request, glyph, property) };
    let missing = match property {
        RasterProperty::Scale => value == 0 || value == 1000,
        RasterProperty::Adjusted | RasterProperty::Weights | RasterProperty::Adjustments => value == 0,
        RasterProperty::Width => value == unsafe { request.at(4).word() },
        RasterProperty::Height => value == unsafe { request.at(8).word() },
        RasterProperty::Other => false,
    };
    if missing && (unsafe { engine.at(32).word() } as i32) > 0 {
        unsafe { request.at(60).put(fallback.at(100).word()) };
        value = unsafe { raster_property(request, glyph, property) };
        unsafe { request.at(60).put(primary) };
    }
    value
}
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_query_fallback(state: u32, request: u32, glyph: u32, property: u32) -> u32 {
    unsafe { query_raster_fallback(RasterLayout(Address(state)), Address(request), GlyphCode(glyph), RasterProperty::parse(property)) }
}
#[derive(Clone, Copy)]
enum WeightScale { Outline, Raster(RasterLayout) }
unsafe fn write_weights(data: FontData, offset: u32, scale: WeightScale, output: *mut i16) {
    let first = u32::from(unsafe { data.half(offset.wrapping_add(6)) });
    let second = u32::from(unsafe { data.half(offset.wrapping_add(8)) });
    let third = u32::from(unsafe { data.half(offset.wrapping_add(10)) });
    let fourth = u32::from(unsafe { data.half(offset.wrapping_add(12)) });
    let total = i32::from(first.wrapping_add(second).wrapping_add(third).wrapping_add(fourth) as u16);
    let fifth = u32::from(unsafe { data.half(offset.wrapping_add(14)) });
    let height = match scale { WeightScale::Outline => 2048, WeightScale::Raster(layout) => (unsafe { layout.0.at(12).word() }) as i32 };
    let first = unsafe { signed_quotient(height.wrapping_mul(first as i32), total) } as u16;
    unsafe { write_volatile(output, first as i16) };
    let second = unsafe { signed_quotient(height.wrapping_mul(second as i32), total) } as u16;
    unsafe { write_volatile(output.add(1), second as i16) };
    let third = unsafe { signed_quotient(height.wrapping_mul(third as i32), total) } as u16;
    let width = match scale { WeightScale::Outline => 2048, WeightScale::Raster(layout) => (unsafe { layout.0.at(8).word() }) as i32 };
    let remainder = height.wrapping_sub(i32::from(first)).wrapping_sub(i32::from(second)).wrapping_sub(i32::from(third)) as i16;
    match scale {
        WeightScale::Outline => unsafe { write_volatile(output.add(2), third as i16); write_volatile(output.add(3), remainder) },
        WeightScale::Raster(_) => unsafe { write_volatile(output.add(3), remainder); write_volatile(output.add(2), third as i16) },
    }
    let fifth = unsafe { signed_quotient(width.wrapping_mul(fifth as i32), total) } as i16;
    unsafe { write_volatile(output.add(4), fifth) };
}
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_handle_weights(state: u32, output: *mut i16) -> i32 {
    let data = FontData(unsafe { Address(state).at(8).pointer() });
    let offset = unsafe { data.word(0x3a) };
    if offset == 0 { return 0; }
    let count = unsafe { data.half(offset) };
    let mut cursor = offset.wrapping_add(2);
    for _ in 0..count {
        let identifier = FontPropertyId(u32::from(unsafe { data.half(cursor) }));
        let length = unsafe { data.word(cursor.wrapping_add(2)) };
        if identifier == FontPropertyId(6) {
            unsafe { write_weights(data, cursor, WeightScale::Outline, output) };
            cursor = cursor.wrapping_add(16);
        } else { cursor = cursor.wrapping_add(length); }
    }
    0
}
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_raster_weights(state: u32, output: *mut i16) -> i32 {
    let layout = RasterLayout(Address(state));
    let engine = unsafe { layout.0.pointer() };
    let fallback = unsafe { engine.at(28).pointer() };
    let mut data = unsafe { layout.0.at(64).pointer() };
    let mut offset = unsafe { raster_property(layout.0.at(4), GlyphCode(0), RasterProperty::Weights) };
    if offset == 0 && (unsafe { engine.at(32).word() } as i32) > 0 {
        unsafe { layout.0.at(64).put(fallback.at(100).word()) };
        offset = unsafe { raster_property(layout.0.at(4), GlyphCode(0), RasterProperty::Weights) };
        unsafe { layout.0.at(64).put(data.0) };
        if offset == 0 { return 0; }
        data = unsafe { fallback.at(100).pointer() };
    }
    if data.0 != 0 && (offset as i32) > 0 { unsafe { write_weights(FontData(data), offset, WeightScale::Raster(layout), output) }; }
    0
}
unsafe fn fallback_raster_advance(layout: RasterLayout, request: Address, glyph: GlyphCode) -> i32 {
    let engine = unsafe { layout.0.pointer() };
    let fallback = unsafe { engine.at(28).pointer() };
    let primary = unsafe { request.at(60).word() };
    let mut advance = unsafe { xm1_pipeline_image_font_raster_glyph_advance(request.0, glyph.0) };
    if advance == 0 && (unsafe { engine.at(32).word() } as i32) > 0 {
        unsafe { request.at(60).put(fallback.at(100).word()); xm1_pipeline_image_font_raster_configure(request.0) };
        advance = unsafe { xm1_pipeline_image_font_raster_glyph_advance(request.0, glyph.0) };
        unsafe { request.at(60).put(primary); xm1_pipeline_image_font_raster_configure(request.0) };
    }
    advance
}
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_layout_glyph_width(state: u32, request: u32, glyph: u32) -> i32 {
    unsafe { fallback_raster_advance(RasterLayout(Address(state)), Address(request), GlyphCode(glyph)) }
}
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_chained_glyph_width(state: u32, glyph: u16) -> i32 {
    let layout = RasterLayout(Address(state));
    let engine = unsafe { layout.0.pointer() };
    let advance = unsafe { fallback_raster_advance(layout, layout.0.at(4), GlyphCode(u32::from(glyph))) };
    if advance != 0 { return advance.wrapping_add(unsafe { engine.at(0x1068).word() } as i32); }
    if (unsafe { engine.at(40).word() } as i32) <= 0 { return 0; }
    let style = unsafe { layout.0.at(16).word() };
    let stroke = unsafe { layout.0.at(20).word() } as i32;
    let padding = match style { 1 | 2 => stroke.wrapping_mul(2), 3 | 5 => stroke, _ => 0 };
    let advance = unsafe { xm1_pipeline_image_font_icon_glyph_width(engine.at(100).word(), glyph) };
    if advance > 0 { advance.wrapping_add(padding) }
    else if advance == 0 { 0 }
    else { advance.wrapping_add(unsafe { engine.at(0x1068).word() } as i32) }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_handle_init(state: u32, data: u32, bytes: u32, workspace: u32, engine: u32) -> i32 {
    let handle = OutlineHandle(Address(state));
    unsafe { handle.0.put(engine); handle.0.at(12).put(0); handle.0.at(4).put(0); handle.0.at(8).put(data) };
    let mut info = core::mem::MaybeUninit::<FontBinaryInfo>::uninit();
    if unsafe { xm1_pipeline_image_font_binary_info(data, info.as_mut_ptr()) } == 0 { return -3; }
    if unsafe { info.assume_init() }.format.0 != 4 { return -4; }
    let program = unsafe { xm1_pipeline_image_font_program_initialize(data, bytes, workspace) };
    unsafe { handle.0.at(4).put(program) };
    if program == 0 { -6 } else { 0 }
}
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_handle_dimensions(state: u32, glyph: u16, horizontal: *mut i32, vertical: *mut u32) -> u32 {
    let handle = OutlineHandle(Address(state));
    let engine = unsafe { handle.engine() };
    let bias = unsafe { engine.at(0x1068).word() } as i32;
    let units = unsafe { engine.at(0x103c).word() } as i32;
    let width = unsafe { xm1_pipeline_image_font_handle_query_width(state, u32::from(glyph), handle.0.at(12).word() as i32) };
    if unsafe { handle.0.at(12).word() } == 0 {
        let bias = unsafe { signed_quotient(bias.wrapping_shl(11), units) };
        unsafe { write_volatile(horizontal, (width as i32).wrapping_add(bias)); write_volatile(vertical, 0) };
    } else { unsafe { write_volatile(horizontal, 0); write_volatile(vertical, width) }; }
    width
}
#[derive(Clone, Copy)]
struct GlyphCache(Address);
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct GlyphCacheKey(u32);
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct GlyphCacheVariant(u32);
impl GlyphCacheKey {
    fn new(glyph: GlyphCode, width: i16, height: i16) -> Self { Self((glyph.0 & 0xffff).wrapping_add(((u32::from(height as u8)).wrapping_add(u32::from(width as u8) << 8)) << 16)) }
}
impl GlyphCacheVariant {
    fn new(format: u16, kind: u16) -> Self { Self(u32::from(format) | (u32::from(kind) << 16)) }
}
impl GlyphCache {
    unsafe fn configuration(self) -> Address { unsafe { self.0.at(24).pointer() } }
    unsafe fn entries(self) -> Address { unsafe { self.0.at(28).pointer() } }
    unsafe fn descriptor(self, index: i32) -> Address {
        let slot = unsafe { self.entries().at((index as u32).wrapping_mul(8).wrapping_add(6)).half() } as i16;
        unsafe { self.0.at(40).pointer().at((i32::from(slot) as u32).wrapping_mul(16)) }
    }
}
unsafe fn signed_remainder(value: i32, divisor: i32) -> i32 {
    if divisor == 0 { return unsafe { signed_quotient(value, divisor) }; }
    value.wrapping_rem(divisor)
}
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_cache_initialize(state: u32) {
    let cache = GlyphCache(Address(state));
    if unsafe { cache.0.at(4).word() } != 0 { return; }
    unsafe { cache.0.at(4).put(1); cache.0.at(8).put(0); cache.0.at(12).put(0) };
    let count = unsafe { cache.configuration().at(8).word() } as i32;
    let entries = unsafe { cache.entries() };
    for index in 0..count { unsafe { entries.at((index as u32).wrapping_mul(8).wrapping_add(6)).put_half(index as u16) }; }
}
unsafe fn cache_search(cache: GlyphCache, key: GlyphCacheKey, variant: GlyphCacheVariant, table: Address, count_address: Address, eviction: i32, capacity: i32) -> i32 {
    let scratch = unsafe { cache.0.at(32).pointer() };
    let count = unsafe { count_address.word() } as i32;
    let mut high = count.wrapping_sub(1);
    let mut low = 0;
    let mut insertion = 0;
    while high >= 0 && low <= high {
        insertion = high.wrapping_add(low) >> 1;
        let entry = table.at((insertion as u32).wrapping_mul(8));
        let existing_key = GlyphCacheKey(unsafe { entry.word() });
        let descriptor = unsafe { cache.descriptor(insertion) };
        let existing_variant = GlyphCacheVariant(u32::from(unsafe { descriptor.at(8).half() }) | (u32::from(unsafe { descriptor.at(10).half() }) << 16));
        if existing_key == key && existing_variant == variant { return insertion; }
        if existing_key > key || (existing_key == key && existing_variant > variant) {
            high = insertion.wrapping_sub(1);
            if insertion == 0 { break; }
        } else { insertion = insertion.wrapping_add(1); low = insertion; }
    }
    if eviction < 0 { return -1; }
    let slot;
    let entry;
    if count < capacity {
        entry = table.at((insertion as u32).wrapping_mul(8));
        slot = count as u16;
        unsafe { copy(entry, scratch, (count.wrapping_sub(insertion) as u32).wrapping_mul(8)) };
        let count = unsafe { count_address.word() } as i32;
        unsafe { copy(scratch, table.at((insertion.wrapping_add(1) as u32).wrapping_mul(8)), (count.wrapping_sub(insertion) as u32).wrapping_mul(8)) };
        unsafe { count_address.put(count_address.word().wrapping_add(1)) };
    } else {
        let evicted = table.at((eviction as u32).wrapping_mul(8));
        slot = unsafe { evicted.at(6).half() };
        if insertion < eviction {
            entry = table.at((insertion as u32).wrapping_mul(8));
            let prefix = (eviction.wrapping_sub(insertion) as u32).wrapping_mul(8);
            unsafe { copy(entry, scratch, prefix) };
            let count = unsafe { count_address.word() } as i32;
            unsafe { copy(table.at((eviction.wrapping_add(1) as u32).wrapping_mul(8)), scratch.at(prefix), (count.wrapping_sub(1).wrapping_sub(eviction) as u32).wrapping_mul(8)) };
            let count = unsafe { count_address.word() } as i32;
            unsafe { copy(scratch, table.at((insertion.wrapping_add(1) as u32).wrapping_mul(8)), (count.wrapping_sub(1).wrapping_sub(insertion) as u32).wrapping_mul(8)) };
        } else {
            if insertion > eviction {
                insertion = insertion.wrapping_sub(1);
                unsafe { copy(table.at((eviction.wrapping_add(1) as u32).wrapping_mul(8)), evicted, (insertion.wrapping_sub(eviction) as u32).wrapping_mul(8)) };
            }
            entry = table.at((insertion as u32).wrapping_mul(8));
        }
    }
    unsafe { entry.at(6).put_half(slot); entry.put(key.0) };
    insertion
}
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_cache_find_or_insert(state: u32, key: u32, variant: u32, table: u32, count: *mut i32, eviction: i32, capacity: i32) -> i32 {
    unsafe { cache_search(GlyphCache(Address(state)), GlyphCacheKey(key), GlyphCacheVariant(variant), Address(table), Address(count as u32), eviction, capacity) }
}
unsafe fn touch_cached_glyph(cache: GlyphCache, key: GlyphCacheKey, variant: GlyphCacheVariant) -> i32 {
    if unsafe { cache.0.at(12).word() } == 0 { return 0; }
    let capacity = unsafe { cache.configuration().at(8).word() } as i32;
    let entries = unsafe { cache.entries() };
    let index = unsafe { cache_search(cache, key, variant, entries, cache.0.at(12), -1, capacity) };
    if index < 0 { return -1; }
    let entry = entries.at((index as u32).wrapping_mul(8));
    let previous = unsafe { entry.at(4).half() };
    let position = i32::from(previous as i16);
    if position == unsafe { cache.0.at(8).word() } as i32 { return 1; }
    let next = unsafe { signed_remainder(position.wrapping_add(1), capacity) };
    let ring = unsafe { cache.0.at(36).pointer() };
    let neighbor = ring.at((next as u32).wrapping_mul(8));
    let other_variant = GlyphCacheVariant(unsafe { neighbor.at(4).word() });
    let other_key = GlyphCacheKey(unsafe { neighbor.word() });
    let other_index = unsafe { cache_search(cache, other_key, other_variant, entries, cache.0.at(12), -1, capacity) };
    if other_index < 0 { return -1; }
    let ring = unsafe { cache.0.at(36).pointer() };
    let entries = unsafe { cache.entries() };
    let current = ring.at((position as u32).wrapping_mul(8));
    let neighbor = ring.at((next as u32).wrapping_mul(8));
    unsafe {
        current.put(other_key.0); neighbor.put(key.0); current.at(4).put(other_variant.0);
        entries.at((index as u32).wrapping_mul(8).wrapping_add(4)).put_half(next as u16);
        neighbor.at(4).put(variant.0); entries.at((other_index as u32).wrapping_mul(8).wrapping_add(4)).put_half(previous);
    }
    1
}
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_cache_touch(state: u32, glyph: u16, width: i16, height: i16, format: u16, kind: u16) -> i32 {
    unsafe { touch_cached_glyph(GlyphCache(Address(state)), GlyphCacheKey::new(GlyphCode(u32::from(glyph)), width, height), GlyphCacheVariant::new(format, kind)) }
}
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_cache_find(state: u32, glyph: u16, width: i16, height: i16, format: u16, kind: u16) -> u32 {
    let cache = GlyphCache(Address(state));
    let limit = unsafe { cache.configuration().at(12).word() } as i32;
    if i32::from(width) > limit || i32::from(height) > limit || unsafe { cache.0.at(12).word() } == 0 { return 0; }
    let key = GlyphCacheKey::new(GlyphCode(u32::from(glyph)), width, height);
    let variant = GlyphCacheVariant::new(format, kind);
    let index = unsafe { cache_search(cache, key, variant, cache.entries(), cache.0.at(12), -1, cache.configuration().at(8).word() as i32) };
    if index < 0 { return 0; }
    unsafe { touch_cached_glyph(cache, key, variant); cache.descriptor(index).0 }
}
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_cache_insert(state: u32, glyph: u16, width: i16, height: i16, descriptor: u32) -> i32 {
    let cache = GlyphCache(Address(state));
    let descriptor = Address(descriptor);
    let configuration = unsafe { cache.configuration() };
    let stride = unsafe { descriptor.half() };
    let rows = unsafe { descriptor.at(2).half() };
    let limit = unsafe { configuration.at(12).word() } as i32;
    if i32::from(width) > limit || i32::from(height) > limit { return 0; }
    let key = GlyphCacheKey::new(GlyphCode(u32::from(glyph)), width, height);
    let variant = GlyphCacheVariant(u32::from(unsafe { descriptor.at(8).half() }) | (u32::from(unsafe { descriptor.at(10).half() }) << 16));
    let cursor = unsafe { signed_remainder((cache.0.at(8).word() as i32).wrapping_add(1), configuration.at(8).word() as i32) };
    unsafe { cache.0.at(8).put(cursor as u32) };
    let count = unsafe { cache.0.at(12).word() } as i32;
    let capacity = unsafe { configuration.at(8).word() } as i32;
    let index = if count < capacity {
        if count <= 0 {
            unsafe { cache.entries().put(key.0); cache.0.at(12).put(cache.0.at(12).word().wrapping_add(1)) };
            0
        } else { unsafe { cache_search(cache, key, variant, cache.entries(), cache.0.at(12), cursor, capacity) } }
    } else {
        let previous = unsafe { cache.0.at(36).pointer().at((cursor as u32).wrapping_mul(8)) };
        let old_key = GlyphCacheKey(unsafe { previous.word() });
        let old_variant = GlyphCacheVariant(unsafe { previous.at(4).word() });
        let evicted = unsafe { cache_search(cache, old_key, old_variant, cache.entries(), cache.0.at(12), -1, capacity) };
        unsafe { cache_search(cache, key, variant, cache.entries(), cache.0.at(12), evicted, cache.configuration().at(8).word() as i32) }
    };
    let entry = unsafe { cache.entries() }.at((index as u32).wrapping_mul(8));
    let cursor = unsafe { cache.0.at(8).word() };
    let slot = i32::from(unsafe { entry.at(6).half() } as i16) as u32;
    let ring = unsafe { cache.0.at(36).pointer() };
    let target = unsafe { cache.0.at(40).pointer() }.at(slot.wrapping_mul(16));
    unsafe { ring.at(cursor.wrapping_mul(8)).put(key.0) };
    let cursor = unsafe { cache.0.at(8).word() };
    unsafe { entry.at(4).put_half(cursor as u16) };
    let previous_pixels = unsafe { target.at(12).word() };
    unsafe { ring.at(cursor.wrapping_mul(8).wrapping_add(4)).put(variant.0); copy(descriptor, target, 16) };
    let descriptors = unsafe { cache.0.at(40).pointer() };
    if previous_pixels == 0 {
        let limit = unsafe { cache.configuration().at(12).word() };
        let pixels = unsafe { cache.0.at(20).word() };
        let bytes = unsafe { cache.0.at(16).word() };
        unsafe { descriptors.at(slot.wrapping_mul(16).wrapping_add(12)).put(pixels) };
        let extent = limit.wrapping_mul(limit);
        let pixels = unsafe { cache.0.at(20).word() };
        unsafe { cache.0.at(16).put(bytes.wrapping_sub(extent)); cache.0.at(20).put(pixels.wrapping_add(extent)) };
    } else { unsafe { descriptors.at(slot.wrapping_mul(16).wrapping_add(12)).put(previous_pixels) }; }
    let target = descriptors.at(slot.wrapping_mul(16));
    unsafe { copy(descriptor.at(12).pointer(), target.at(12).pointer(), u32::from(stride).wrapping_mul(u32::from(rows))) };
    1
}

#[derive(Clone, Copy)]
enum RasterStyle { Plain, Stroke, Outline, Right, Bottom, RightBottom, Other }
impl RasterStyle { fn parse(value: u32) -> Self { match value { 0 => Self::Plain, 1 => Self::Stroke, 2 => Self::Outline, 3 => Self::Right, 4 => Self::Bottom, 5 => Self::RightBottom, _ => Self::Other } } }
#[derive(Default)]
struct RasterGeometry { stride: i32, height: i32, width: i32, left: i32, top: i32, expansion: i32 }
unsafe fn raster_geometry(request: Address, geometry: &mut RasterGeometry) {
    let stroke = unsafe { request.at(16).word() } as i32;
    let width = unsafe { request.at(4).word() } as i32;
    let height = unsafe { request.at(8).word() } as i32;
    let (mut width, mut height, padding) = match RasterStyle::parse(unsafe { request.at(12).word() }) {
        RasterStyle::Plain => (width, height, 0),
        RasterStyle::Stroke | RasterStyle::Outline => (width.wrapping_add(stroke.wrapping_mul(2)), height.wrapping_add(stroke.wrapping_mul(2)), stroke),
        RasterStyle::Right => (width.wrapping_add(stroke), height, 0),
        RasterStyle::Bottom => (width, height.wrapping_add(stroke), 0),
        RasterStyle::RightBottom => (width.wrapping_add(stroke), height.wrapping_add(stroke), 0),
        RasterStyle::Other => (0, 0, 0),
    };
    let primary = unsafe { request.at(20).word() } as i32;
    let fallback_count = unsafe { request.at(28).word() } as i32;
    let expansion = if primary <= 0 && fallback_count <= 0 { 0 } else {
        let secondary = if fallback_count > 0 { (unsafe { request.at(32).word() }) as i32 } else { 0 };
        let expansion = secondary.max(primary);
        if unsafe { request.word() } == 0 { width = width.wrapping_add(expansion); } else { height = height.wrapping_add(expansion); }
        expansion
    };
    match RasterDepth::parse(unsafe { request.at(36).word() }) {
        Some(RasterDepth::Mono) => { let rounded = width.wrapping_add(7); geometry.stride = (if rounded < 0 { rounded.wrapping_add(7) } else { rounded }) >> 3; }
        Some(RasterDepth::Gray4 | RasterDepth::Gray8) => geometry.stride = width,
        None => {}
    }
    geometry.width = width; geometry.height = height; geometry.left = padding; geometry.top = padding; geometry.expansion = expansion;
}
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_layout_dimensions(request: u32, stride: *mut i32, height: *mut i32, width: *mut i32, left: *mut i32, top: *mut i32, expansion: *mut i32) {
    let request = Address(request);
    let mut geometry = RasterGeometry::default();
    unsafe { raster_geometry(request, &mut geometry) };
    if unsafe { request.at(36).word() } <= 2 { unsafe { write_volatile(stride, geometry.stride) }; }
    unsafe { write_volatile(height, geometry.height) };
    for (pointer, value) in [(width, geometry.width), (left, geometry.left), (top, geometry.top), (expansion, geometry.expansion)] {
        if !pointer.is_null() { unsafe { write_volatile(pointer, value) }; }
    }
}
#[repr(C)]
pub struct RasterGlyphRequest {
    glyph: u32, x: i32, y: i32, width: i32, height: i32, stride: i32, pixels: u32, workspace_bytes: u32, coverage_pointer: u32,
}
unsafe extern "C" { fn xm1_pipeline_image_font_raster_render_bitmap(request: u32, glyph: u32) -> i32; }
unsafe fn render_raster_cached(layout: RasterLayout, advance: *mut i32, request: Address, glyph_request: *mut RasterGlyphRequest) -> i32 {
    let engine = unsafe { layout.0.pointer() };
    let configuration = unsafe { engine.at(24).pointer() };
    let fallback = unsafe { engine.at(28).pointer() };
    let scratch = unsafe { configuration.at(0x70).pointer() };
    let primary = unsafe { request.at(60).word() };
    let glyph = GlyphCode(unsafe { read_volatile(core::ptr::addr_of!((*glyph_request).glyph)) });
    let first_advance = unsafe { xm1_pipeline_image_font_raster_glyph_advance(request.0, glyph.0) };
    unsafe { write_volatile(advance, first_advance) };
    let mut used_fallback = false;
    let status;
    if first_advance == 0 && (unsafe { engine.at(32).word() } as i32) > 0 {
        unsafe { request.at(60).put(fallback.at(100).word()); xm1_pipeline_image_font_raster_configure(request.0) };
        let fallback_advance = unsafe { xm1_pipeline_image_font_raster_glyph_advance(request.0, read_volatile(core::ptr::addr_of!((*glyph_request).glyph))) };
        unsafe { write_volatile(advance, fallback_advance) };
        status = if fallback_advance > 0 {
            used_fallback = true;
            unsafe { xm1_pipeline_image_font_raster_render_bitmap(request.0, glyph_request as u32) }
        } else { first_advance };
        unsafe { request.at(60).put(primary); xm1_pipeline_image_font_raster_configure(request.0) };
    } else { status = unsafe { xm1_pipeline_image_font_raster_render_bitmap(request.0, glyph_request as u32) }; }
    let cache_configuration = unsafe { scratch.at(68).pointer() };
    if cache_configuration.0 == 0 { return status; }
    let limit = unsafe { cache_configuration.at(12).word() } as i32;
    if (unsafe { request.at(4).word() } as i32) > limit || (unsafe { request.at(8).word() } as i32) > limit { return status; }
    let current_engine = unsafe { layout.0.pointer() };
    let current_fallback = unsafe { current_engine.at(28).pointer() };
    let glyph = GlyphCode(unsafe { read_volatile(core::ptr::addr_of!((*glyph_request).glyph)) });
    let current_data = unsafe { request.at(60).word() };
    if unsafe { raster_property(request, glyph, RasterProperty::Adjusted) } != 0 { return status; }
    if (unsafe { current_engine.at(32).word() } as i32) > 0 {
        unsafe { request.at(60).put(current_fallback.at(100).word()) };
        let adjusted = unsafe { raster_property(request, glyph, RasterProperty::Adjusted) };
        unsafe { request.at(60).put(current_data) };
        if adjusted != 0 { return status; }
    }
    if status != 0 || unsafe { read_volatile(advance) } <= 0 || unsafe { request.at(20).word() } != 0 || unsafe { request.at(12).word() } != 0 || unsafe { request.at(28).word() } != 0 || unsafe { request.at(36).word() } == 0 { return status; }
    let font = if used_fallback { fallback } else { configuration };
    let mut tag = unsafe { font.half() }.wrapping_shl(8);
    if unsafe { request.at(36).word() } as u16 == 1 && unsafe { engine.at(0x1074).word() } as u16 == 0 { tag |= 1; }
    let width = unsafe { request.at(4).word() };
    let height = unsafe { request.at(8).word() };
    let mut descriptor = [0_u32; 4];
    descriptor[0] = u32::from(width as u16) | (u32::from(height as u16) << 16);
    descriptor[1] = u32::from(unsafe { read_volatile(core::ptr::addr_of!((*glyph_request).x)) } as u8)
        | (u32::from(unsafe { read_volatile(core::ptr::addr_of!((*glyph_request).y)) } as u8) << 8)
        | (u32::from(unsafe { read_volatile(core::ptr::addr_of!((*glyph_request).width)) } as u8) << 16)
        | (u32::from(unsafe { read_volatile(core::ptr::addr_of!((*glyph_request).height)) } as u8) << 24);
    descriptor[2] = u32::from(tag); descriptor[3] = unsafe { read_volatile(core::ptr::addr_of!((*glyph_request).pixels)) };
    unsafe { xm1_pipeline_image_font_cache_insert(scratch.at(44).0, glyph.0 as u16, width as i16, height as i16, descriptor.as_ptr() as u32) };
    status
}
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_render_with_fallback_cache(state: u32, advance: *mut i32, request: u32, glyph_request: *mut RasterGlyphRequest) -> i32 {
    unsafe { render_raster_cached(RasterLayout(Address(state)), advance, Address(request), glyph_request) }
}
unsafe fn find_raster_cached_glyph(layout: RasterLayout, scratch: Address, engine: Address, glyph: GlyphCode) -> Address {
    let depth = unsafe { layout.0.at(40).word() };
    let adjustment = u16::from(depth as u16 == 1 && unsafe { engine.at(0x1074).word() } as u16 == 0);
    let configuration = unsafe { engine.at(24).pointer() };
    let width = unsafe { layout.0.at(8).word() } as i16;
    let height = unsafe { layout.0.at(12).word() } as i16;
    let tag = (unsafe { configuration.word() }.wrapping_shl(8) as u16) | adjustment;
    let first = unsafe { xm1_pipeline_image_font_cache_find(scratch.at(44).0, glyph.0 as u16, width, height, tag, 0) };
    if first != 0 || (unsafe { engine.at(32).word() } as i32) <= 0 { return Address(first); }
    let fallback = unsafe { engine.at(28).pointer() };
    let tag = (unsafe { fallback.word() }.wrapping_shl(8) as u16) | adjustment;
    Address(unsafe { xm1_pipeline_image_font_cache_find(scratch.at(44).0, glyph.0 as u16, layout.0.at(8).half() as i16, layout.0.at(12).half() as i16, tag, 0) })
}
#[derive(Clone, Copy)]
enum RasterRowTransfer { Copy, Or }
unsafe fn transfer_raster_row(source: Address, destination: Address, bytes: u32, mode: RasterRowTransfer) {
    if source.0 == 0 || destination.0 == 0 { return; }
    let aligned_word = (source.0 | destination.0 | bytes) & 3 == 0;
    let aligned_half = (source.0 | destination.0 | bytes) & 1 == 0;
    let separated = source.0 > destination.0.wrapping_add(4) || destination.0 > source.0.wrapping_add(4);
    let width = if aligned_word || (aligned_half && bytes / 2 > 1 && (source.0 | destination.0) & 3 == 0 && separated) { 4 } else if aligned_half { 2 } else { 1 };
    let mut offset = 0;
    while bytes.wrapping_sub(offset) >= width {
        let source = source.at(offset); let destination = destination.at(offset);
        match (width, mode) {
            (4, RasterRowTransfer::Copy) => unsafe { destination.put(source.word()) },
            (4, RasterRowTransfer::Or) => {
                let value = if aligned_word { unsafe { destination.word() | source.word() } } else { unsafe { source.word() | destination.word() } };
                unsafe { destination.put(value) };
            }
            (2, RasterRowTransfer::Copy) => unsafe { destination.put_half(source.half()) },
            (2, RasterRowTransfer::Or) => unsafe { destination.put_half(source.half() | destination.half()) },
            (_, RasterRowTransfer::Copy) => unsafe { destination.put_byte(source.byte()) },
            (_, RasterRowTransfer::Or) => unsafe { destination.put_byte(source.byte() | destination.byte()) },
        }
        offset = offset.wrapping_add(width);
    }
    while offset < bytes {
        let source = source.at(offset); let destination = destination.at(offset);
        match mode { RasterRowTransfer::Copy => unsafe { destination.put_half(source.half()) }, RasterRowTransfer::Or => unsafe { destination.put_half(source.half() | destination.half()) } }
        offset = offset.wrapping_add(2);
    }
}
// After the first bit, the preceding destination byte controls the next bit.
unsafe fn blit_mono_bytes(source: Address, geometry: &RasterGeometry, target: &GlyphDraw, x: i32, y: i32, advance: i32) {
    let width = geometry.expansion.wrapping_add(advance);
    let mut row = 0_i32;
    while row < geometry.height && y.wrapping_add(row) < target.height {
        let mut byte = 0_i32;
        while byte < geometry.stride {
            let source = source.at(row.wrapping_mul(geometry.stride).wrapping_add(byte) as u32);
            let column = byte.wrapping_mul(8);
            let mut decision = unsafe { source.byte() };
            if x.wrapping_add(column) < target.stride && column < width {
                for bit in 0..8 {
                    let column = column.wrapping_add(bit);
                    if x.wrapping_add(column) >= target.stride || column >= width { break; }
                    let destination = Address(target.pixels.0).at(y.wrapping_add(row).wrapping_mul(target.stride).wrapping_add(x).wrapping_add(column) as u32);
                    let old = unsafe { destination.byte() };
                    unsafe { destination.put_byte(if decision & (0x80 >> bit) != 0 { 255 } else { old }) };
                    decision = old;
                }
            }
            byte = byte.wrapping_add(1);
        }
        row = row.wrapping_add(1);
    }
}
unsafe fn blit_mono_packed(source: Address, geometry: &RasterGeometry, target: &GlyphDraw, x: i32, y: i32, advance: i32) {
    let stride = target.stride.wrapping_mul(8);
    let width = geometry.expansion.wrapping_add(advance);
    let mut row = 0_i32;
    while row < geometry.height && y.wrapping_add(row) < target.height {
        let mut column = 0_i32;
        while column < width && x.wrapping_add(column) < stride {
            let source_bit = row.wrapping_mul(geometry.stride).wrapping_mul(8).wrapping_add(column);
            let value = unsafe { source.at((source_bit >> 3) as u32).byte() };
            let target_bit = y.wrapping_add(row).wrapping_mul(stride).wrapping_add(x).wrapping_add(column);
            let destination = Address(target.pixels.0).at((target_bit >> 3) as u32);
            let old = unsafe { destination.byte() };
            let mask = 1_u8 << (!(x.wrapping_add(column)) & 7);
            unsafe { destination.put_byte(if value & (1_u8 << (!column & 7)) != 0 { old | mask } else { old }) };
            column = column.wrapping_add(1);
        }
        row = row.wrapping_add(1);
    }
}
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_draw_raster_bitmap(state: u32, glyph: u16, width: i32, height: i32, x: i32, y: i32, stride: i32, pixels: u32, clip: u32, format: u32) -> i32 {
    let layout = RasterLayout(Address(state)); let request = layout.0.at(4);
    let engine = unsafe { layout.0.pointer() };
    let object = unsafe { engine.at(20).pointer() };
    let scratch = unsafe { engine.at(24).pointer().at(0x70).pointer() };
    let glyph = GlyphCode(u32::from(glyph));
    let mut workspace_bytes = unsafe { engine.at(0x1030).word() };
    let mut geometry = RasterGeometry::default();
    let adjusted = unsafe { query_raster_fallback(layout, request, glyph, RasterProperty::Adjusted) };
    let cache_configuration = unsafe { scratch.at(68).pointer() };
    let can_cache = cache_configuration.0 != 0
        && (unsafe { layout.0.at(8).word() } as i32) <= unsafe { cache_configuration.at(12).word() } as i32
        && (unsafe { layout.0.at(12).word() } as i32) <= unsafe { cache_configuration.at(12).word() } as i32 && adjusted == 0;
    let cache = if can_cache && unsafe { layout.0.at(24).word() } == 0 && unsafe { layout.0.at(16).word() } == 0 && unsafe { layout.0.at(32).word() } == 0 && unsafe { layout.0.at(40).word() } != 0 {
        unsafe { find_raster_cached_glyph(layout, scratch, engine, glyph) }
    } else { Address(0) };
    let mut glyph_request = RasterGlyphRequest { glyph: glyph.0, x: 0, y: 0, width: 0, height: 0, stride: 0, pixels: 0, workspace_bytes: 0, coverage_pointer: 0 };
    let mut advance = 0_i32;
    let bitmap;
    if cache.0 != 0 {
        geometry.height = i32::from(unsafe { cache.at(2).half() }); geometry.stride = i32::from(unsafe { cache.half() });
        glyph_request.x = i32::from(unsafe { cache.at(4).byte() }); glyph_request.y = i32::from(unsafe { cache.at(5).byte() });
        glyph_request.width = i32::from(unsafe { cache.at(6).byte() }); glyph_request.height = i32::from(unsafe { cache.at(7).byte() });
        bitmap = unsafe { cache.at(12).pointer() };
        advance = unsafe { fallback_raster_advance(layout, request, glyph) };
    } else {
        unsafe { raster_geometry(request, &mut geometry) };
        if adjusted != 0 {
            let old_width = unsafe { layout.0.at(8).word() }; let old_height = unsafe { layout.0.at(12).word() };
            let scaled_width = unsafe { query_raster_fallback(layout, request, glyph, RasterProperty::Width) };
            let scaled_height = unsafe { query_raster_fallback(layout, request, glyph, RasterProperty::Height) };
            unsafe { layout.0.at(8).put(scaled_width); layout.0.at(12).put(scaled_height) };
            let mut scaled = RasterGeometry { stride: geometry.stride, ..RasterGeometry::default() };
            unsafe { raster_geometry(request, &mut scaled); layout.0.at(8).put(old_width); layout.0.at(12).put(old_height) };
            geometry.stride = scaled.stride; geometry.width = scaled.width;
        }
        let bytes = geometry.stride.wrapping_mul(geometry.height);
        if (unsafe { scratch.at(12).word() } as i32) < bytes { return 0; }
        bitmap = unsafe { scratch.at(16).pointer() };
        unsafe { fill(bitmap, 0, bytes as u32) };
        glyph_request.x = geometry.left; glyph_request.y = geometry.top; glyph_request.stride = geometry.stride; glyph_request.pixels = bitmap.0;
        loop {
            let status = unsafe { render_raster_cached(layout, &mut advance, request, &mut glyph_request) };
            if status != -4 { if status < 0 { return 0; } break; }
            workspace_bytes = workspace_bytes.wrapping_add(1024);
            let old_workspace = unsafe { engine.at(0x1034).word() };
            if old_workspace != 0 { unsafe { xm1_pipeline_image_font_release(old_workspace) }; }
            let rounded = workspace_bytes.wrapping_add(1023) as i32;
            let rounded = if rounded < 0 { rounded.wrapping_add(1023) } else { rounded };
            let workspace = unsafe { xm1_pipeline_image_font_allocate(((rounded >> 10) << 10) as u32) };
            if workspace == 0 { return 0; }
            unsafe {
                engine.at(0x1034).put(workspace); layout.0.at(68).put(workspace); engine.put(engine.word().wrapping_add(1024));
                layout.0.at(72).put(workspace_bytes); engine.at(0x1030).put(workspace_bytes);
            }
            if unsafe { xm1_pipeline_image_font_raster_configure(request.0) } != -4 { break; }
        }
    }
    if advance == 0 {
        if (unsafe { engine.at(40).word() } as i32) > 0 && unsafe { object.at(0x11a8).word() } == 0 {
            return unsafe { xm1_pipeline_image_font_draw_icon_bitmap(engine.at(100).word(), glyph.0 as u16, width, height, x, y, stride, pixels, clip, format) };
        }
        return 0;
    }
    let span = unsafe { engine.at(0x53c).pointer() };
    let y = y.wrapping_add(unsafe { span.at(0xd18).word() } as i32);
    let x = x.wrapping_add(unsafe { span.at(0xd14).word() } as i32);
    let target = GlyphDraw { glyph, width, height, x, y, stride, pixels: Address32(pixels), format: FontBitmapFormat(format) };
    if format & 0x200 != 0 {
        if clip != 0 && unsafe { Address(clip).at(24).word() } == 1 {
            geometry.height = unsafe { Address(clip).at(12).word() } as i32;
            if geometry.height < glyph_request.y.wrapping_add(glyph_request.height) { glyph_request.height = geometry.height.wrapping_sub(glyph_request.y); }
        }
        match RasterDepth::parse(unsafe { layout.0.at(40).word() }) {
            Some(RasterDepth::Gray4 | RasterDepth::Gray8) => {
                let available_height = height.wrapping_sub(y).min(geometry.height);
                let available_width = stride.wrapping_sub(x).min(geometry.stride);
                let rows = if available_height < glyph_request.height.wrapping_add(glyph_request.y) { available_height.wrapping_sub(glyph_request.y) } else { glyph_request.height };
                let columns = if available_width < glyph_request.width.wrapping_add(glyph_request.x) { available_width.wrapping_sub(glyph_request.x) } else { glyph_request.width };
                if rows <= 0 || columns <= 0 { return 0; }
                let mode = if unsafe { engine.at(0x104c).word() } != 0 || (unsafe { engine.at(0x1068).word() } as i32) < 0 || (glyph.0 as u16).wrapping_sub(0xe00) < 0x80 { RasterRowTransfer::Or } else { RasterRowTransfer::Copy };
                for row in 0..rows {
                    let source = bitmap.at(geometry.stride.wrapping_mul(glyph_request.y.wrapping_add(row)).wrapping_add(glyph_request.x) as u32);
                    let destination = Address(pixels).at(stride.wrapping_mul(y.wrapping_add(glyph_request.y).wrapping_add(row)).wrapping_add(x).wrapping_add(glyph_request.x) as u32);
                    unsafe { transfer_raster_row(source, destination, columns as u32, mode) };
                }
            }
            Some(RasterDepth::Mono) => unsafe { blit_mono_bytes(bitmap, &geometry, &target, x, y, advance) },
            None => {}
        }
    } else if format & 0x100 != 0 {
        if clip != 0 {
            let clip = Address(clip);
            let mut row = unsafe { clip.at(12).word() } as i32;
            let offset = unsafe { clip.at(8).word() };
            while row < geometry.height {
                unsafe { fill(bitmap.at(row.wrapping_mul(geometry.stride) as u32).at(offset), 0, geometry.stride as u32) };
                row = row.wrapping_add(1);
            }
        }
        unsafe { blit_mono_packed(bitmap, &geometry, &target, x, y, advance) };
    }
    advance.wrapping_add(geometry.expansion).wrapping_add(unsafe { engine.at(0x1068).word() } as i32)
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_outline_height(_state: u32) -> i32 { 2048 }
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_outline_width(_state: u32) -> i32 { 2048 }
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_outline_secondary_extent(_state: u32) -> i32 { 0 }
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_outline_right_overhang(_state: u32, _glyph: u16) -> i32 { 0 }
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_outline_top_overhang(_state: u32) -> i32 { 0 }
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_outline_storage_size() -> i32 { 0x814 }
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_program_destroy(_program: u32) -> i32 { 0 }
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_handle_destroy(state: u32) -> i32 {
    let status = unsafe { xm1_pipeline_image_font_program_destroy(Address(state).at(4).word()) };
    if status == 0 { 0 } else { -7 }
}

unsafe fn initialize_engine_storage(engine: Address) {
    unsafe {
        engine.at(8).put(0x107c);
        engine.at(0x64).put(engine.at(0x107c).0);
        engine.at(0x68).put(engine.at(0x1088).0);
        engine.at(12).put(engine.0);
    }
}
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_create_engine(output: *mut u32) -> i32 {
    if unsafe { read_volatile(output) } != 0 { return 0x1188; }
    let engine = Address(unsafe { xm1_pipeline_image_font_allocate(0x1400) });
    if engine.0 == 0 { return -5; }
    unsafe { fill(engine, 0, 0x1188); write_volatile(output, engine.0); initialize_engine_storage(engine) };
    0x1188
}
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_raster_storage_size(width: i32, height: i32, mode: i32) -> i32 {
    let width = width.wrapping_add(5).wrapping_shl(2).min(400);
    let height = height.wrapping_add(20).min(400);
    if mode == 0 { return 0x83ec; }
    let triple = width.wrapping_add(width.wrapping_shl(1));
    if mode == 1 { height.wrapping_add(triple.wrapping_shl(2)).wrapping_shl(3).wrapping_add(0xa3ec) }
    else { height.wrapping_add(triple.wrapping_shl(1)).wrapping_shl(5).wrapping_add(0x123ec) }
}
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_raster_destroy() -> i32 { 0 }
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_raster_dimensions() -> i32 { 0 }

#[derive(Clone, Copy)]
struct FontName(Address);
#[derive(Clone, Copy)]
struct PrefixBytes(u32);
unsafe fn name_length(name: FontName) -> u32 {
    if name.0.0 == 0 { return 0; }
    let mut length = 0u32;
    while unsafe { name.0.at(length).byte() } != 0 { length = length.wrapping_add(1); }
    length
}
unsafe fn compare_prefix(left: FontName, right: FontName, count: PrefixBytes) -> i32 {
    let left_length = unsafe { name_length(left) };
    let right_length = unsafe { name_length(right) };
    let difference = left.0.0.wrapping_sub(right.0.0) as i32;
    if left_length == 0 || right_length == 0 || count.0 > left_length || count.0 > right_length { return difference; }
    for offset in 0..count.0 {
        if unsafe { left.0.at(offset).byte() } != unsafe { right.0.at(offset).byte() } { return difference; }
    }
    0
}
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_compare_prefix(left: u32, right: u32, count: u32) -> i32 {
    unsafe { compare_prefix(FontName(Address(left)), FontName(Address(right)), PrefixBytes(count)) }
}
#[derive(Clone, Copy)]
enum EmbeddedFont { Arabic, Big5, Gb, Jis, Ksc, Latin, Thai, Icon }
impl EmbeddedFont {
    fn slot(self) -> u32 { match self { Self::Arabic => 0, Self::Big5 => 1, Self::Gb => 2, Self::Jis => 3, Self::Ksc => 4, Self::Latin => 5, Self::Thai => 6, Self::Icon => 7 } }
    fn signature(self) -> (FontName, PrefixBytes) {
        let (address, length) = match self {
            Self::Arabic => (0x1180564,15), Self::Big5 => (0x1180574,13),
            Self::Gb => (0x1180584,11), Self::Jis => (0x1180590,12),
            Self::Ksc => (0x11805a0,12), Self::Latin => (0x11805b0,14),
            Self::Thai => (0x11805c0,13), Self::Icon => (0x11805d0,13),
        };
        (FontName(Address(address)), PrefixBytes(length))
    }
}
#[derive(Clone, Copy)]
struct FontReference(Address);
unsafe fn open_reference(reference: FontReference) -> i32 {
    let references = unsafe { reference.0.at(0x6c).word() };
    if references != 0 {
        unsafe { reference.0.at(0x6c).put(references.wrapping_add(1)) };
        return 0;
    }
    let name = FontName(reference.0.at(16));
    for font in [EmbeddedFont::Arabic,EmbeddedFont::Big5,EmbeddedFont::Gb,EmbeddedFont::Jis,EmbeddedFont::Ksc,EmbeddedFont::Latin,EmbeddedFont::Thai,EmbeddedFont::Icon] {
        let (signature, length) = font.signature();
        if unsafe { compare_prefix(name, signature, length) } == 0 {
            let entry = Address(0x11804a4).at(font.slot() * 24);
            unsafe { reference.0.at(0x6c).put(1) };
            let data = unsafe { entry.at(8).word() };
            let size = unsafe { entry.at(12).word() };
            unsafe { reference.0.at(0x64).put(data); reference.0.at(0x68).put(size) };
            return 1;
        }
    }
    0
}
unsafe fn close_reference(reference: FontReference) {
    let count = unsafe { reference.0.at(0x6c).word() }.wrapping_sub(1);
    unsafe { reference.0.at(0x6c).put(count) };
    if count == 0 { unsafe { reference.0.at(0x64).put(0); reference.0.at(0x68).put(0) }; }
}
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_open_reference(reference: u32, _timeout: i32, _first: u32, _second: u32) -> i32 {
    unsafe { open_reference(FontReference(Address(reference))) }
}
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_close_reference(reference: u32, _timeout: i32) {
    unsafe { close_reference(FontReference(Address(reference))) };
}
#[derive(Clone, Copy)]
struct FontReferenceIndex(u32);
unsafe fn reference_at(manager: Address, index: FontReferenceIndex) -> FontReference {
    FontReference(unsafe { manager.pointer() }.at(index.0.wrapping_mul(0x74)))
}
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_reference_at_index(manager: u32, index: u32) -> u32 {
    unsafe { reference_at(Address(manager), FontReferenceIndex(index)).0.0 }
}
#[derive(Clone, Copy)]
enum FontRenderingMode { Outline, Builtin, Mono, Gray4, Gray8, IconMono, IconByte }
impl FontRenderingMode {
    fn parse(value: u32) -> Option<Self> {
        match value { 1 => Some(Self::Outline), 2 => Some(Self::Builtin), 3 => Some(Self::Mono), 4 => Some(Self::Gray4), 5 => Some(Self::Gray8), 6 => Some(Self::IconMono), 7 => Some(Self::IconByte), _ => None }
    }
    fn wire(self) -> u32 { match self { Self::Outline => 1, Self::Builtin => 2, Self::Mono => 3, Self::Gray4 => 4, Self::Gray8 => 5, Self::IconMono => 6, Self::IconByte => 7 } }
}
#[derive(Clone, Copy)]
enum FontFileClass { Fdl, Bdf, Other }
impl FontFileClass { fn parse(value: u32) -> Self { match value { 0 => Self::Fdl, 1 => Self::Bdf, _ => Self::Other } } }
#[derive(Clone, Copy)]
enum RenderingRequest { Outline, Mono, Gray4, Gray8, Unsupported }
impl RenderingRequest { fn parse(value: u32) -> Self { match value { 1 => Self::Outline, 2 => Self::Mono, 8 => Self::Gray4, 4 => Self::Gray8, _ => Self::Unsupported } } }
fn select_rendering_mode(class: FontFileClass, request: RenderingRequest) -> Option<FontRenderingMode> {
    match (class, request) {
        (FontFileClass::Fdl,RenderingRequest::Outline) => Some(FontRenderingMode::Outline),
        (FontFileClass::Bdf,RenderingRequest::Mono) => Some(FontRenderingMode::Builtin),
        (FontFileClass::Fdl,RenderingRequest::Mono) => Some(FontRenderingMode::Mono),
        (FontFileClass::Fdl,RenderingRequest::Gray4) => Some(FontRenderingMode::Gray4),
        (FontFileClass::Fdl,RenderingRequest::Gray8) => Some(FontRenderingMode::Gray8),
        _ => None,
    }
}
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_object_mode(class: u32, request: u32) -> u32 {
    select_rendering_mode(FontFileClass::parse(class), RenderingRequest::parse(request)).map_or(8, FontRenderingMode::wire)
}
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_read_directory(engine: u32) -> i32 {
    let engine = Address(engine);
    let signature = Address(0x118f9bc);
    let first = unsafe { signature.word() };
    let second = unsafe { signature.at(4).word() };
    let _ = unsafe { signature.at(8).word() };
    unsafe { engine.at(0x74).put(0) };
    let data = unsafe { engine.at(0x70).pointer() };
    for offset in 0..8 {
        let expected = if offset < 4 { first } else { second };
        let byte = unsafe { data.at(offset).byte() };
        unsafe { engine.at(0x4f8 + offset).put_byte(byte); engine.at(0x74).put(offset + 1) };
        if byte != (expected >> ((offset & 3) * 8)) as u8 { return -2; }
    }
    let version = u16::from(unsafe { data.at(8).byte() }) | u16::from(unsafe { data.at(9).byte() }) << 8;
    unsafe { engine.at(0x74).put(10); engine.at(0x500).put_half(version) };
    let count = u16::from(unsafe { data.at(10).byte() }) | u16::from(unsafe { data.at(11).byte() }) << 8;
    unsafe { engine.at(0x74).put(12); engine.at(0x502).put_half(count) };
    for index in 0..u32::from(count) {
        let cursor = 12 + index * 4;
        let third = u32::from(unsafe { data.at(cursor + 2).byte() });
        let second = u32::from(unsafe { data.at(cursor + 1).byte() });
        let first = u32::from(unsafe { data.at(cursor).byte() });
        let fourth = u32::from(unsafe { data.at(cursor + 3).byte() });
        let offset = third << 16 | second << 8 | first | fourth << 24;
        unsafe { engine.at(0x74).put(cursor + 4); engine.at(0x78 + index * 4).put(offset) };
    }
    unsafe { engine.at(0x504).put(engine.at(0x78).0) };
    0
}

#[derive(Clone, Copy)]
enum MethodTable { Primary, Fallback }
#[derive(Clone, Copy)]
enum MethodSlot { Init, Append, Draw, Weights, GlyphWidth, Height, Width, Secondary, Right, Top, Padding, Dimensions, Destroy, Bitmap }
impl MethodSlot {
    fn offset(self, table: MethodTable) -> u32 {
        let primary = match self {
            Self::Init => 0xff8, Self::Append => 0xffc, Self::Draw => 0x1000, Self::Weights => 0x1004,
            Self::GlyphWidth => 0x1008, Self::Height => 0x100c, Self::Width => 0x1010, Self::Secondary => 0x1014,
            Self::Right => 0x1018, Self::Top => 0x101c, Self::Padding => 0x1020, Self::Dimensions => 0x1024,
            Self::Destroy => 0x1028, Self::Bitmap => 0x102c,
        };
        match table { MethodTable::Primary => primary, MethodTable::Fallback => primary - 0xfcc }
    }
}
#[derive(Clone, Copy)]
struct FontMethod(*const ());
unsafe fn install_method(engine: Address, table: MethodTable, slot: MethodSlot, method: FontMethod) {
    unsafe { engine.at(slot.offset(table)).put(method.0 as usize as u32) };
}
unsafe extern "C" {
    fn xm1_pipeline_image_font_builtin_storage_size() -> i32;
    fn xm1_pipeline_image_font_builtin_destroy() -> i32;
    fn xm1_pipeline_image_font_builtin_dimensions() -> i32;
    fn xm1_pipeline_image_font_builtin_init(state: u32, data: u32, size: i32, storage: u32, owner: u32) -> i32;
    fn xm1_pipeline_image_font_icon_storage_size(mode: u32, first: i32, second: i32, third: i32) -> i32;
    fn xm1_pipeline_image_font_icon_destroy() -> i32;
    fn xm1_pipeline_image_font_icon_dimensions() -> i32;
    fn xm1_pipeline_image_font_icon_init(state: u32, data: u32, size: i32, storage: u32, owner: u32) -> i32;
}
unsafe fn install_primary_methods(engine: Address, mode: FontRenderingMode) {
    macro_rules! set {
        ($slot:ident, $function:ident) => { unsafe { install_method(engine, MethodTable::Primary, MethodSlot::$slot, FontMethod($function as *const ())) } };
    }
    match mode {
        FontRenderingMode::Outline => {
            set!(Destroy, xm1_pipeline_image_font_handle_destroy);
            set!(GlyphWidth, xm1_pipeline_image_font_handle_glyph_width);
            set!(Dimensions, xm1_pipeline_image_font_handle_dimensions);
            set!(Secondary, xm1_pipeline_image_font_outline_secondary_extent);
            set!(Height, xm1_pipeline_image_font_outline_height);
            set!(Width, xm1_pipeline_image_font_outline_width);
            set!(Right, xm1_pipeline_image_font_outline_right_overhang);
            set!(Top, xm1_pipeline_image_font_outline_top_overhang);
            set!(Padding, xm1_pipeline_image_font_outline_stroke_padding);
            set!(Weights, xm1_pipeline_image_font_handle_weights);
            set!(Bitmap, xm1_pipeline_image_font_handle_bitmap);
            set!(Init, xm1_pipeline_image_font_handle_init);
            set!(Append, xm1_pipeline_image_font_handle_append_bitmap);
        }
        FontRenderingMode::Builtin => {
            set!(Destroy, xm1_pipeline_image_font_builtin_destroy);
            set!(GlyphWidth, xm1_pipeline_image_font_builtin_glyph_width);
            set!(Dimensions, xm1_pipeline_image_font_builtin_dimensions);
            set!(Secondary, xm1_pipeline_image_font_builtin_secondary_extent);
            set!(Height, xm1_pipeline_image_font_builtin_height);
            set!(Width, xm1_pipeline_image_font_builtin_width);
            set!(Right, xm1_pipeline_image_font_builtin_right_overhang);
            set!(Top, xm1_pipeline_image_font_builtin_top_overhang);
            set!(Padding, xm1_pipeline_image_font_builtin_stroke_padding);
            set!(Draw, xm1_pipeline_image_font_draw_text_bitmap);
            set!(Weights, xm1_pipeline_image_font_builtin_properties);
            set!(Init, xm1_pipeline_image_font_builtin_init);
        }
        FontRenderingMode::Mono | FontRenderingMode::Gray4 | FontRenderingMode::Gray8 => {
            set!(Destroy, xm1_pipeline_image_font_raster_destroy);
            set!(GlyphWidth, xm1_pipeline_image_font_chained_glyph_width);
            set!(Dimensions, xm1_pipeline_image_font_raster_dimensions);
            set!(Secondary, xm1_pipeline_image_font_raster_secondary_extent);
            set!(Height, xm1_pipeline_image_font_raster_height);
            set!(Width, xm1_pipeline_image_font_raster_width);
            set!(Right, xm1_pipeline_image_font_raster_right_overhang);
            set!(Top, xm1_pipeline_image_font_raster_top_overhang);
            set!(Padding, xm1_pipeline_image_font_raster_stroke_padding);
            set!(Draw, xm1_pipeline_image_font_draw_raster_bitmap);
            set!(Weights, xm1_pipeline_image_font_raster_weights);
            set!(Init, xm1_pipeline_image_font_initialize_raster_layout);
        }
        FontRenderingMode::IconMono | FontRenderingMode::IconByte => {
            set!(Destroy, xm1_pipeline_image_font_icon_destroy);
            set!(GlyphWidth, xm1_pipeline_image_font_icon_glyph_width);
            set!(Dimensions, xm1_pipeline_image_font_icon_dimensions);
            set!(Secondary, xm1_pipeline_image_font_icon_secondary_extent);
            set!(Height, xm1_pipeline_image_font_icon_height);
            set!(Width, xm1_pipeline_image_font_icon_width);
            set!(Right, xm1_pipeline_image_font_icon_right_overhang);
            set!(Top, xm1_pipeline_image_font_icon_top_overhang);
            set!(Padding, xm1_pipeline_image_font_icon_stroke_padding);
            set!(Draw, xm1_pipeline_image_font_draw_icon_bitmap);
            set!(Weights, xm1_pipeline_image_font_icon_weights);
            set!(Init, xm1_pipeline_image_font_icon_init);
        }
    }
}
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_install_methods(engine: u32) {
    let engine = Address(engine);
    if (unsafe { engine.at(0x28).word() } as i32) <= 0 { return; }
    if unsafe { engine.at(0x24).word() } == 0 { return; }
    macro_rules! set {
        ($slot:ident, $function:ident) => { unsafe { install_method(engine, MethodTable::Fallback, MethodSlot::$slot, FontMethod($function as *const ())) } };
    }
    set!(Weights, xm1_pipeline_image_font_icon_weights);
    set!(Init, xm1_pipeline_image_font_icon_init);
    set!(Destroy, xm1_pipeline_image_font_icon_destroy);
    set!(GlyphWidth, xm1_pipeline_image_font_icon_glyph_width);
    set!(Dimensions, xm1_pipeline_image_font_icon_dimensions);
    set!(Secondary, xm1_pipeline_image_font_icon_secondary_extent);
    set!(Height, xm1_pipeline_image_font_icon_height);
    set!(Width, xm1_pipeline_image_font_icon_width);
    set!(Right, xm1_pipeline_image_font_icon_right_overhang);
    set!(Top, xm1_pipeline_image_font_icon_top_overhang);
    set!(Padding, xm1_pipeline_image_font_icon_stroke_padding);
    set!(Draw, xm1_pipeline_image_font_draw_icon_bitmap);
}
#[derive(Clone, Copy)]
struct FontWorkspaceBytes(i32);
unsafe fn initialize_backend(engine: Address, mode: FontRenderingMode, data: FontData, bytes: FontWorkspaceBytes, storage: Address) -> i32 {
    let state = unsafe { engine.at(0x68).word() };
    match mode {
        FontRenderingMode::Outline => unsafe { xm1_pipeline_image_font_handle_init(state, data.0.0, bytes.0 as u32, storage.0, engine.0) },
        FontRenderingMode::Builtin => unsafe { xm1_pipeline_image_font_builtin_init(state, data.0.0, bytes.0, storage.0, engine.0) },
        FontRenderingMode::Mono | FontRenderingMode::Gray4 | FontRenderingMode::Gray8 => unsafe { xm1_pipeline_image_font_initialize_raster_layout(state, data.0.0, bytes.0 as u32, storage.0, engine.0) },
        FontRenderingMode::IconMono | FontRenderingMode::IconByte => unsafe { xm1_pipeline_image_font_icon_init(state, data.0.0, bytes.0, storage.0, engine.0) },
    }
}
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_initialize_engine_backends(script_data: u32, reference: u32, rendering_mode: u32, size: i32, output: *mut u32, fallback: u32, fallback_count: u32, icon: u32, icon_count: u32) -> i32 {
    let data = unsafe { Address(reference).at(0x64).pointer() };
    if data.0 == 0 { return -3; }
    let mut engine = Address(unsafe { read_volatile(output) });
    if engine.0 == 0 {
        engine = Address(unsafe { xm1_pipeline_image_font_allocate(0x1400) });
        if engine.0 == 0 { return -5; }
        unsafe { fill(engine, 0, 0x1188); initialize_engine_storage(engine) };
    }
    unsafe { engine.at(0xff4).put(rendering_mode) };
    let mode = match FontRenderingMode::parse(rendering_mode) {
        Some(mode) => mode,
        None => {
            let storage = unsafe { engine.at(0x1034).word() };
            if storage != 0 { unsafe { xm1_pipeline_image_font_release(storage) }; }
            unsafe { xm1_pipeline_image_font_release(engine.0) };
            return -8;
        }
    };
    unsafe { install_primary_methods(engine, mode); engine.at(0x1040).put(size as u32); engine.at(0x103c).put(size as u32) };
    match mode {
        FontRenderingMode::Mono => unsafe { engine.at(0x105c).put(0); engine.at(0x106c).put(0x100) },
        FontRenderingMode::Gray4 => unsafe { engine.at(0x105c).put(1); engine.at(0x106c).put(0x200) },
        FontRenderingMode::Gray8 => unsafe { engine.at(0x105c).put(2); engine.at(0x106c).put(0x200) },
        FontRenderingMode::Builtin | FontRenderingMode::IconMono => unsafe { engine.at(0x106c).put(0x100) },
        FontRenderingMode::IconByte => unsafe { engine.at(0x106c).put(0x200) },
        FontRenderingMode::Outline => (),
    }
    let mut needed = match mode {
        FontRenderingMode::Outline => unsafe { xm1_pipeline_image_font_outline_storage_size() },
        FontRenderingMode::Builtin => unsafe { xm1_pipeline_image_font_builtin_storage_size() },
        FontRenderingMode::Mono | FontRenderingMode::Gray4 | FontRenderingMode::Gray8 => unsafe { xm1_pipeline_image_font_raster_storage_size(size, size, 0) },
        FontRenderingMode::IconMono | FontRenderingMode::IconByte => unsafe { xm1_pipeline_image_font_icon_storage_size(0x106c, mode.wire() as i32 - 1, engine.at(0x105c).word() as i32, 0) },
    };
    if needed < 0 {
        let storage = unsafe { engine.at(0x1034).word() };
        if storage != 0 { unsafe { xm1_pipeline_image_font_release(storage) }; }
        unsafe { xm1_pipeline_image_font_release(engine.0) };
        return -8;
    }
    loop {
        let (bytes, storage) = if needed == 0 {
            (unsafe { engine.at(0x1030).word() } as i32, unsafe { engine.at(0x1034).pointer() })
        } else {
            let rounded = needed.wrapping_add(0x3ff).wrapping_shr(10).wrapping_shl(10);
            let storage = Address(unsafe { xm1_pipeline_image_font_allocate(rounded as u32) });
            unsafe { engine.at(0x1034).put(storage.0) };
            if storage.0 == 0 { unsafe { xm1_pipeline_image_font_release(engine.0) }; return -6; }
            unsafe { engine.at(0x1030).put(needed as u32) };
            (needed, storage)
        };
        let mut status = unsafe { initialize_backend(engine, mode, FontData(data), FontWorkspaceBytes(bytes), storage) };
        if status == -6 {
            needed = needed.wrapping_add(0x400);
            let storage = unsafe { engine.at(0x1034).word() };
            if storage != 0 {
                unsafe {
                    xm1_pipeline_image_font_release(storage);
                    engine.at(0x1034).put(0); engine.at(0x1030).put(0);
                    engine.put(engine.at(8).word().wrapping_add(0x100));
                }
            }
            continue;
        }
        if status == 0 {
            unsafe {
                engine.at(0x70).put(script_data);
                engine.at(0x18).put(reference);
                engine.at(0x6c).put(data.0);
                engine.put(engine.at(8).word().wrapping_add(engine.at(0x1030).word()).wrapping_add(0x100));
                engine.at(0x1c).put(fallback); engine.at(0x20).put(fallback_count);
                engine.at(0x28).put(icon_count); engine.at(4).put(script_data); engine.at(0x24).put(icon);
                xm1_pipeline_image_font_install_methods(engine.0);
            }
            if icon != 0 && (unsafe { engine.at(0x28).word() } as i32) > 0 {
                unsafe { xm1_pipeline_image_font_icon_init(engine.at(0x64).word(), Address(icon).at(0x64).word(), 0, 0, engine.0) };
            }
            status = if unsafe { engine.at(0x70).word() } == 0 { 1 } else {
                let directory = unsafe { xm1_pipeline_image_font_read_directory(engine.0) };
                if directory == -2 { 1 } else { directory }
            };
        }
        if status as u32 > 1 {
            let storage = unsafe { engine.at(0x1034).word() };
            if storage != 0 { unsafe { xm1_pipeline_image_font_release(storage) }; }
            unsafe { xm1_pipeline_image_font_release(engine.0) };
            return status;
        }
        unsafe { write_volatile(output, engine.0) };
        return status;
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_reset_options(object: u32) {
    let object = FontObject(Address(object));
    if unsafe { object.field(12) } == 2 {
        unsafe { set(object, FontProperty::Direction, 0x200e); set(object, FontProperty::Alignment, 4) };
        return;
    }
    unsafe { set(object, FontProperty::Width, 27); set(object, FontProperty::Height, 27) };
    let index = unsafe { object.field(12) }.wrapping_sub(3);
    let format = if index < 3 { unsafe { Address(0x11859dc + 0x44).at(index * 4).word() } } else { u32::MAX };
    unsafe { set(object, FontProperty::PixelFormat, format) };
    for property in [
        FontProperty::Fill, FontProperty::Weight, FontProperty::EdgeWidth, FontProperty::Scale, FontProperty::Spacing,
        FontProperty::Top, FontProperty::TrailingPosition, FontProperty::TrailingSpacing, FontProperty::Bottom,
        FontProperty::LeadingPosition, FontProperty::LeadingSpacing, FontProperty::Rotation,
    ] { unsafe { set(object, property, 0) }; }
    unsafe {
        set(object, FontProperty::Direction, 0x200e);
        set(object, FontProperty::OutlineMode, 2);
        set(object, FontProperty::Alignment, 4);
        set(object, FontProperty::AutoWeight, 0);
        set(object, FontProperty::AutoSpacing, 0);
        set(object, FontProperty::AutoTrailing, 0);
        set(object, FontProperty::AutoLeading, 0);
    }
}
unsafe fn release_font_object(object: FontObject) {
    let storage = unsafe { object.pointer(0x1178) };
    if storage.0 != 0 {
        unsafe { xm1_pipeline_image_font_release(storage.0); object.0.at(0x1178).put(0); object.0.at(0x117c).put(0) };
    }
    unsafe { xm1_pipeline_image_font_release(object.0.0) };
}
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_object_create(reference: u32, request: u32, fallback: u32, icon: u32) -> u32 {
    let reference = FontReference(Address(reference));
    if reference.0.0 == 0 { return 0; }
    let manager = unsafe { reference.0.at(0x70).pointer() };
    if manager.0 == 0 || unsafe { reference.0.at(4).word() } == 3 { return 0; }
    let primary = unsafe { reference_at(manager, FontReferenceIndex(reference.0.word())) };
    let icon = if icon == 0 { FontReference(Address(0)) } else {
        let icon = FontReference(Address(icon));
        let manager = unsafe { primary.0.at(0x70).pointer() };
        if manager.0 != unsafe { icon.0.at(0x70).word() } || request == 1 || unsafe { icon.0.at(4).word() } != 3 { return 0; }
        unsafe { reference_at(manager, FontReferenceIndex(icon.0.word())) }
    };
    let fallback = if fallback == 0 { FontReference(Address(0)) } else {
        let fallback = FontReference(Address(fallback));
        let manager = unsafe { primary.0.at(0x70).pointer() };
        let class = unsafe { primary.0.at(4).word() };
        if manager.0 != unsafe { fallback.0.at(0x70).word() } || class != unsafe { fallback.0.at(4).word() } || class != 0 { return 0; }
        unsafe { reference_at(manager, FontReferenceIndex(fallback.0.word())) }
    };
    let Some(mode) = select_rendering_mode(FontFileClass::parse(unsafe { primary.0.at(4).word() }), RenderingRequest::parse(request)) else { return 0; };
    let index = mode.wire().wrapping_sub(1);
    let (extra, object_bytes, allocation_bytes) = if index < 5 {
        let extra = unsafe { Address(0x11859dc).at(index * 4).word() };
        let rounded = extra.wrapping_add(0x22df);
        let rounded = if (rounded as i32) < 0 { extra.wrapping_add(0x26de) } else { rounded };
        (extra, extra.wrapping_add(0x1ee0), rounded & 0xfffffc00)
    } else { (0, 0x1ee0, 0x2000) };
    let object = FontObject(Address(unsafe { xm1_pipeline_image_font_allocate(allocation_bytes) }));
    if object.0.0 == 0 { return 0; }
    let workspace_bytes = if index < 5 { unsafe { Address(0x11859dc + 0x14).at(index * 4).word() } } else { 0 };
    let storage = if (workspace_bytes as i32) > 0 {
        let storage = Address(unsafe { xm1_pipeline_image_font_allocate(workspace_bytes.wrapping_add(0x3ff) & 0xfffffc00) });
        unsafe { object.0.at(0x1178).put(storage.0) };
        storage
    } else { unsafe { object.pointer(0x1178) } };
    if storage.0 == 0 { unsafe { xm1_pipeline_image_font_release(object.0.0) }; return 0; }
    unsafe { object.0.at(0x117c).put(workspace_bytes) };
    if unsafe { open_reference(primary) } != 1 { unsafe { release_font_object(object) }; return 0; }
    let icon_opened = if icon.0.0 == 0 { false } else {
        if unsafe { open_reference(icon) } != 1 {
            unsafe { close_reference(primary); release_font_object(object) };
            return 0;
        }
        unsafe { object.0.at(0x11a0).put(icon.0.0); object.0.at(0x11a4).put(1) };
        true
    };
    let fallback_opened = if fallback.0.0 == 0 { false } else {
        if unsafe { open_reference(fallback) } != 1 {
            unsafe {
                close_reference(primary);
                if icon_opened && icon.0.at(0x64).word() != 0 { close_reference(icon); }
                release_font_object(object);
            }
            return 0;
        }
        unsafe { object.0.at(0x1198).put(fallback.0.0); object.0.at(0x119c).put(1) };
        true
    };
    let header = object.0.at(0x1ee0);
    unsafe {
        object.0.at(0x88).put(object.0.at(0x11c4).0);
        object.0.at(0x1170).put(header.0);
        object.0.at(0x11ac).put(primary.0.at(0x70).word());
        object.0.at(0x80).put(object.0.at(0x11b0).0);
        object.0.at(0x84).put(20);
        object.0.at(0x1174).put(extra);
        object.0.at(4).put(object_bytes);
        object.0.at(0x8c).put(0xd1c);
        object.0.at(if matches!(mode, FontRenderingMode::Outline) { 0x116c } else { 0x1168 }).put(header.0);
        object.0.at(8).put(primary.0.0);
        object.0.at(12).put(mode.wire());
    }
    let mut engine = 0u32;
    let status = unsafe {
        xm1_pipeline_image_font_initialize_engine_backends(0x118360c, primary.0.0, mode.wire(), 100, &mut engine,
            object.field(0x1198), object.field(0x119c), object.field(0x11a0), object.field(0x11a4))
    };
    if status >= 0 && engine != 0 {
        unsafe { Address(engine).at(20).put(object.0.0); object.0.put(engine); xm1_pipeline_image_font_reset_options(object.0.0) };
        return object.0.0;
    }
    unsafe {
        close_reference(primary);
        if fallback_opened && fallback.0.at(0x64).word() != 0 { close_reference(fallback); }
        if icon_opened && icon.0.at(0x64).word() != 0 { close_reference(icon); }
        release_font_object(object);
    }
    0
}
