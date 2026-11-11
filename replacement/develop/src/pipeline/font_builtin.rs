use core::ptr::{read_volatile, write_volatile};
use crate::develop::font_core::bitmap::{self, BdfHeader, BdfVersion, BitmapError, BitmapGeometry, BitmapOutput, ByteSource, ClearRectangle, CoverageInk, DrawMode, GlyphCode, GlyphIndex, GlyphRecord, Keyword, Line, LineAction, LineSource, Origin, PixelOutput, TextOffset, ThaiField, ThaiProperty, Token, TokenSink};

#[repr(transparent)]
#[derive(Clone, Copy)]
struct Address(u32);
impl Address {
    fn at(self, offset: u32) -> Self { Self(self.0.wrapping_add(offset)) }
    unsafe fn byte(self) -> u8 { unsafe { read_volatile(self.0 as *const u8) } }
    unsafe fn half(self) -> u16 { unsafe { read_volatile(self.0 as *const u16) } }
    unsafe fn signed_half(self) -> i32 { (unsafe { self.half() }) as i16 as i32 }
    unsafe fn word(self) -> u32 { unsafe { read_volatile(self.0 as *const u32) } }
    unsafe fn pointer(self) -> Self { Self(unsafe { self.word() }) }
    unsafe fn put_byte(self, value: u8) { unsafe { write_volatile(self.0 as *mut u8, value) }; }
    unsafe fn put_half(self, value: u16) { unsafe { write_volatile(self.0 as *mut u16, value) }; }
    unsafe fn put(self, value: u32) { unsafe { write_volatile(self.0 as *mut u32, value) }; }
    unsafe fn bytes<'a>(self, length: usize) -> &'a [u8] { if length == 0 { &[] } else { unsafe { core::slice::from_raw_parts(self.0 as *const u8,length) } } }
}

#[derive(Clone, Copy)]
struct Engine(Address);
impl Engine {
    unsafe fn bias(self) -> i32 { (unsafe { self.0.at(0x1068).word() }) as i32 }
    unsafe fn origin(self) -> Origin {
        let span = unsafe { self.0.at(0x53c).pointer() };
        Origin { x: unsafe { span.at(0xd14).word() } as i32, y: unsafe { span.at(0xd18).word() } as i32 }
    }
}
#[derive(Clone, Copy)]
struct BdfState(Address);
impl BdfState {
    unsafe fn engine(self) -> Engine { Engine(unsafe { self.0.pointer() }) }
    unsafe fn resource(self) -> Address { unsafe { self.0.at(0x38).pointer() } }
    unsafe fn line_buffer(self) -> Address { unsafe { self.0.at(0x3c).pointer() } }
    unsafe fn token_buffer(self) -> Address { unsafe { self.0.at(0x40).pointer() } }
    unsafe fn glyph_offset(self, code: GlyphCode) -> Option<TextOffset> {
        let table = unsafe { self.0.at(0x30).pointer() };
        let page = unsafe { table.at((code.0 as u32 >> 8) * 4).word() };
        if page == 0 { return None; }
        let offset = unsafe { table.at((page + (code.0 as u32 & 255)) * 4).word() };
        if offset == 0 { None } else { Some(TextOffset(offset as usize)) }
    }
}
#[derive(Clone, Copy)]
struct IconState(Address);
impl IconState {
    unsafe fn engine(self) -> Engine { Engine(unsafe { self.0.pointer() }) }
    unsafe fn glyph(self, code: GlyphCode) -> Option<Address> {
        let resource = unsafe { self.0.at(8).pointer() };
        bitmap::icon_glyph_offset(&RawBytes(resource),code).ok().flatten().map(|offset| resource.at(offset as u32))
    }
}

unsafe extern "C" { fn xm1_pipeline_image_font_fill_memory(destination: u32, byte: u32, count: u32) -> u32; }
unsafe fn fill(destination: Address, count: u32) { unsafe { xm1_pipeline_image_font_fill_memory(destination.0,0,count) }; }

struct RawBytes(Address);
impl ByteSource for RawBytes {
    fn byte(&self, offset: usize) -> Result<u8,BitmapError> { Ok(unsafe { self.0.at(offset as u32).byte() }) }
}
struct RawPixels(Address);
impl BitmapOutput for RawPixels {
    fn put(&mut self, offset: usize, byte: u8) -> Result<(),BitmapError> { unsafe { self.0.at(offset as u32).put_byte(byte) }; Ok(()) }
}
impl PixelOutput for RawPixels {
    fn get(&self, offset: usize) -> Result<u8,BitmapError> { Ok(unsafe { self.0.at(offset as u32).byte() }) }
}
struct RawTokens<'a> { scratch: Address, name: Option<&'a mut [u8; 14]> }
impl<'a> RawTokens<'a> { fn new(scratch: Address) -> Self { Self { scratch, name: None } } }
impl TokenSink for RawTokens<'_> {
    fn token(&mut self, token: Token<'_>) {
        for (index,byte) in token.bytes.iter().enumerate() { unsafe { self.scratch.at(index as u32).put_byte(*byte) }; }
        unsafe { self.scratch.at(token.bytes.len() as u32).put_byte(0) };
    }
    fn clear_name(&mut self, length: usize) {
        if let Some(name) = self.name.as_deref_mut() {
            for (index,byte) in name.iter_mut().take(length).enumerate() { *byte = unsafe { self.scratch.at(index as u32).byte() }; }
        }
        unsafe { fill(self.scratch,length as u32) };
    }
}

unsafe fn text_line(resource: Address, offset: TextOffset, next: *mut u32, length_out: *mut u32, destination: Address, copy: bool) -> i32 {
    if copy && destination.0 == 0 { return -1; }
    let Ok((length,after)) = bitmap::line_extent(&RawBytes(resource),offset) else { return -1; };
    if copy {
        for index in 0..length { unsafe { destination.at(index as u32).put_byte(resource.at(offset.0.wrapping_add(index) as u32).byte()) }; }
        unsafe { destination.at(length as u32).put_byte(0) };
    }
    unsafe { write_volatile(next,after.0 as u32); write_volatile(length_out,if copy { length as u32 } else { 0 }) };
    0
}
struct RawLines { resource: Address, scratch: Address, last_next: TextOffset }
impl RawLines {
    unsafe fn new(state: BdfState, resource: Address, offset: TextOffset) -> Self { Self { resource, scratch: unsafe { state.line_buffer() }, last_next: offset } }
}
impl LineSource for RawLines {
    fn line(&mut self, offset: TextOffset) -> Result<Line<'_>,BitmapError> {
        let mut next = offset.0 as u32;
        let mut length = 0;
        if unsafe { text_line(self.resource,offset,&mut next,&mut length,self.scratch,true) } < 0 { self.last_next = TextOffset(0); return Err(BitmapError::MissingLineScratch); }
        self.last_next = TextOffset(next as usize);
        Ok(Line { bytes: unsafe { self.scratch.bytes(length as usize) }, next: self.last_next })
    }
    fn skip_line(&mut self, offset: TextOffset) -> Result<TextOffset,BitmapError> { bitmap::line_extent(&RawBytes(self.resource),offset).map(|(_,next)| next) }
}
struct RawIndex { table: Address, capacity: u32, pages: u32 }
impl GlyphIndex for RawIndex {
    fn insert(&mut self, code: GlyphCode, offset: TextOffset) -> bool {
        let high = (code.0 as u32 >> 8) * 4;
        let mut page = unsafe { self.table.at(high).word() };
        if page == 0 {
            let ordinal = self.pages + 1;
            self.pages = ordinal & 255;
            page = (ordinal << 8) & 65535;
            unsafe { self.table.at(high).put(page) };
        } else { page &= 65535; }
        let entry = (page + (code.0 as u32 & 255)) & 65535;
        if (self.capacity >> 2) <= entry { return false; }
        unsafe { self.table.at(entry * 4).put(offset.0 as u32) };
        true
    }
    fn finish(&self) -> Result<(),BitmapError> {
        if self.capacity < (self.pages + 1) * 1024 { Err(BitmapError::IndexCapacity) } else { Ok(()) }
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct GlyphMetrics {
    name: [u8; 14], code: u16, scalable: [i16; 2], advance: [i16; 2],
    scalable_vertical: [i16; 2], advance_vertical: [i16; 2], vector: [i16; 2],
    width: i16, height: i16, x: i16, y: i16, reserved: u32, bitmap: u32,
}
impl GlyphMetrics {
    fn record(&self) -> GlyphRecord {
        GlyphRecord { code: GlyphCode(self.code), scalable: self.scalable, advance: self.advance, scalable_vertical: self.scalable_vertical, advance_vertical: self.advance_vertical, vector: self.vector, bounds: [self.width,self.height,self.x,self.y], bitmap: if self.bitmap == 0 { None } else { Some(TextOffset(self.bitmap as usize)) } }
    }
    fn update(&mut self, record: &GlyphRecord) {
        self.code = record.code.0; self.scalable = record.scalable; self.advance = record.advance;
        self.scalable_vertical = record.scalable_vertical; self.advance_vertical = record.advance_vertical; self.vector = record.vector;
        [self.width,self.height,self.x,self.y] = record.bounds;
        self.bitmap = record.bitmap.map_or(0, |offset| offset.0 as u32);
    }
}
unsafe fn halves<const N: usize>(address: Address) -> [i16; N] { core::array::from_fn(|index| (unsafe { address.at(index as u32 * 2).signed_half() }) as i16) }
unsafe fn put_halves(address: Address, values: &[i16]) { for (index,value) in values.iter().enumerate() { unsafe { address.at(index as u32 * 2).put_half(*value as u16) }; } }
unsafe fn read_header(address: Address) -> BdfHeader {
    BdfHeader {
        version: match unsafe { address.byte() } { 0x21 => Some(BdfVersion::V21), 0x22 => Some(BdfVersion::V22), _ => None },
        size: unsafe { halves(address.at(2)) }, bounds: unsafe { halves(address.at(8)) }, metrics_set: unsafe { address.at(16).signed_half() } as i16,
        scalable: unsafe { halves(address.at(18)) }, advance: unsafe { halves(address.at(22)) }, scalable_vertical: unsafe { halves(address.at(26)) },
        advance_vertical: unsafe { halves(address.at(30)) }, vector: unsafe { halves(address.at(34)) }, properties: unsafe { address.at(38).signed_half() } as i16, characters: unsafe { address.at(40).word() } as i32,
    }
}
unsafe fn write_header(address: Address, header: &BdfHeader) {
    if let Some(version) = header.version { unsafe { address.put_byte(match version { BdfVersion::V21 => 0x21, BdfVersion::V22 => 0x22 }) }; }
    unsafe { put_halves(address.at(2),&header.size); put_halves(address.at(8),&header.bounds); address.at(16).put_half(header.metrics_set as u16);
        put_halves(address.at(18),&header.scalable); put_halves(address.at(22),&header.advance); put_halves(address.at(26),&header.scalable_vertical);
        put_halves(address.at(30),&header.advance_vertical); put_halves(address.at(34),&header.vector); address.at(38).put_half(header.properties as u16); address.at(40).put(header.characters as u32); }
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum ParseMode { Header, Glyph }
fn keyword_number(kind: Keyword) -> u32 {
    match kind {
        Keyword::StartFont => 0, Keyword::Comment => 1, Keyword::ContentVersion => 2, Keyword::Font => 3, Keyword::Size => 4,
        Keyword::FontBox => 5, Keyword::MetricsSet => 6, Keyword::Scalable => 7, Keyword::Advance => 8,
        Keyword::ScalableVertical => 9, Keyword::AdvanceVertical => 10, Keyword::Vector => 11,
        Keyword::StartProperties => 12, Keyword::EndProperties => 13, Keyword::Chars => 14, Keyword::EndFont => 15,
        Keyword::StartChar => 16, Keyword::Encoding => 17, Keyword::Box => 18, Keyword::Bitmap => 19, Keyword::Data => 20, Keyword::EndChar => 21,
    }
}
unsafe fn apply_line(action: LineAction<'_>, header: Address, glyph: Address, mode: ParseMode, tokens: &mut RawTokens<'_>) {
    let metric = if mode == ParseMode::Header { header.at(18) } else { glyph.at(16) };
    match action {
        LineAction::None => {},
        LineAction::Version(version) => unsafe { header.put_byte(match version { BdfVersion::V21 => 0x21, BdfVersion::V22 => 0x22 }) },
        LineAction::Size(value) => unsafe { put_halves(header.at(2),&value) }, LineAction::FontBox(value) => unsafe { put_halves(header.at(8),&value) },
        LineAction::MetricsSet(value) => unsafe { header.at(16).put_half(value as u16) },
        LineAction::Scalable(value) => unsafe { put_halves(metric,&value) }, LineAction::Advance(value) => unsafe { put_halves(metric.at(4),&value) },
        LineAction::ScalableVertical(value) => unsafe { put_halves(metric.at(8),&value) }, LineAction::AdvanceVertical(value) => unsafe { put_halves(metric.at(12),&value) },
        LineAction::Vector(value) => unsafe { put_halves(metric.at(16),&value) }, LineAction::Properties(value) => unsafe { header.at(38).put_half(value as u16) },
        LineAction::Chars(value) => unsafe { header.at(40).put(value as u32) }, LineAction::Encoding(value) => unsafe { glyph.at(14).put_half(value.0) },
        LineAction::Box(value) => unsafe { put_halves(glyph.at(36),&value) },
        LineAction::Name(name) => {
            for (index,byte) in name.iter().take(14).enumerate() { unsafe { glyph.at(index as u32).put_byte(*byte) }; }
            tokens.clear_name(name.len());
        },
    }
}
unsafe fn parse_line(state: BdfState, line: Address, length: i32, header: Address, glyph: Address, mode: ParseMode, kind: &mut Keyword) -> i32 {
    if (mode == ParseMode::Header && header.0 == 0) || (mode == ParseMode::Glyph && glyph.0 == 0) { return -1; }
    if length < 1 { return 0; }
    let mut tokens = RawTokens::new(unsafe { state.token_buffer() });
    let Some(parsed) = bitmap::parse_line(unsafe { line.bytes(length as usize) },&mut tokens) else { return 0; };
    *kind = parsed.keyword;
    unsafe { apply_line(parsed.action,header,glyph,mode,&mut tokens) };
    match parsed.keyword { Keyword::Chars => i32::from(mode == ParseMode::Header), Keyword::EndChar => i32::from(mode == ParseMode::Glyph), Keyword::EndFont => 2, _ => 0 }
}
unsafe fn scan_header(state: BdfState, resource: Address, next: &mut TextOffset) -> i32 {
    let mut source = unsafe { RawLines::new(state,resource,TextOffset(0)) };
    let mut tokens = RawTokens::new(unsafe { state.token_buffer() });
    let mut header = unsafe { read_header(state.0.at(4)) };
    let result = bitmap::scan_header(&mut source,&mut header,&mut tokens);
    *next = source.last_next;
    unsafe { write_header(state.0.at(4),&header) };
    match result { Ok(_) => 0, Err(BitmapError::MissingLineScratch) => -1, Err(_) => -2 }
}
unsafe fn scan_glyph(state: BdfState, resource: Address, offset: TextOffset, metrics: &mut GlyphMetrics, next: &mut TextOffset) -> i32 {
    let mut source = unsafe { RawLines::new(state,resource,offset) };
    let mut record = metrics.record();
    let mut tokens = RawTokens { scratch: unsafe { state.token_buffer() }, name: Some(&mut metrics.name) };
    let result = bitmap::scan_glyph(&mut source,offset,&mut record,&mut tokens);
    metrics.update(&record);
    *next = source.last_next;
    match result { Ok(_) => 0, Err(BitmapError::MissingLineScratch) => -1, Err(BitmapError::MissingGlyph) => -3, Err(_) => -2 }
}
unsafe fn metric_glyph(state: BdfState, code: GlyphCode) -> Option<GlyphRecord> {
    let offset = unsafe { state.glyph_offset(code) }?;
    let mut source = unsafe { RawLines::new(state,state.resource(),offset) };
    let mut tokens = RawTokens::new(unsafe { state.token_buffer() });
    let mut glyph = GlyphRecord::EMPTY;
    bitmap::scan_glyph(&mut source,offset,&mut glyph,&mut tokens).ok().map(|_| glyph)
}
unsafe fn index_glyphs(state: BdfState, resource: Address, offset: TextOffset, table: Address, capacity: u32) -> i32 {
    let mut source = unsafe { RawLines::new(state,resource,offset) };
    let mut tokens = RawTokens::new(unsafe { state.token_buffer() });
    let mut index = RawIndex { table, capacity, pages: 0 };
    match bitmap::index_glyphs(&mut source,offset,&mut index,&mut tokens) { Ok(count) => count as i32, Err(_) => -6 }
}
unsafe fn write_thai(output: Address, name: Address, property: ThaiProperty) {
    let (index,length) = match property.field { ThaiField::Top => (0,8), ThaiField::Above => (1,10), ThaiField::Base => (2,9), ThaiField::Below => (3,10), ThaiField::Shift => (4,10) };
    if length != 8 && (length..12).any(|offset| unsafe { name.at(offset).byte() } != 0) { return; }
    unsafe { output.at(index * 2).put_half(property.value as u16) };
}
unsafe fn decode_bitmap(state: BdfState, glyph: &GlyphRecord, destination: Address, stride: i32, y: i32) -> i32 {
    let mut source = unsafe { RawLines::new(state,state.resource(),TextOffset(0)) };
    match bitmap::decode_bitmap(&mut source,glyph,&mut RawPixels(destination),stride,y) { Ok(()) => 0, Err(BitmapError::MissingEndChar) => -2, Err(_) => -1 }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_builtin_height(state: u32) -> i32 { unsafe { Address(state).at(14).signed_half() } }
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_builtin_width(state: u32) -> i32 { unsafe { Address(state).at(12).signed_half() } }
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_builtin_secondary_extent(state: u32) -> i32 {
    let state = Address(state);
    let mut header = BdfHeader::EMPTY;
    header.bounds[0] = unsafe { state.at(12).signed_half() } as i16;
    header.bounds[2] = unsafe { state.at(16).signed_half() } as i16;
    header.advance[0] = unsafe { state.at(26).signed_half() } as i16;
    bitmap::secondary_extent(&header)
}
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_builtin_right_overhang(state: u32, code: u16) -> i32 { unsafe { metric_glyph(BdfState(Address(state)),GlyphCode(code)) }.map_or(0, |glyph| bitmap::right_overhang(&glyph)) }
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_icon_height(state: u32) -> i32 { (unsafe { Address(state).at(8).pointer().at(9).byte() }) as i32 }
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_icon_width(state: u32) -> i32 { (unsafe { Address(state).at(8).pointer().at(8).byte() }) as i32 }
#[no_mangle]
pub extern "C" fn xm1_pipeline_image_font_icon_secondary_extent(_state: u32) -> i32 { 0 }
#[no_mangle]
pub extern "C" fn xm1_pipeline_image_font_icon_right_overhang(_state: u32, _code: u16) -> i32 { 0 }
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_icon_init(state: u32, data: u32, _size: i32, _storage: u32, owner: u32) -> i32 { unsafe { Address(state).put(owner); Address(state).at(8).put(data) }; 0 }
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_icon_glyph_width(state: u32, code: u16) -> i32 {
    let state = IconState(Address(state));
    let Some(glyph) = (unsafe { state.glyph(GlyphCode(code)) }) else { return 0; };
    let width = unsafe { glyph.byte() } as i32;
    if width == 0 { 0 } else { width.wrapping_add(unsafe { state.engine().bias() }) }
}
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_builtin_glyph_width(state: u32, code: u16) -> i32 {
    let state = BdfState(Address(state));
    if unsafe { state.glyph_offset(GlyphCode(code)) }.is_none() { return 0; }
    if let Some(glyph) = unsafe { metric_glyph(state,GlyphCode(code)) } {
        let width = glyph.advance[0] as i32;
        return if width == 0 { 0 } else { width.wrapping_add(unsafe { state.engine().bias() }) };
    }
    let engine = unsafe { state.engine() };
    if unsafe { engine.0.at(0x28).word() } as i32 > 0 { return unsafe { xm1_pipeline_image_font_icon_glyph_width(engine.0.at(100).word(),code) }; }
    let width = if code == 32 { (unsafe { state.0.at(12).signed_half() }) / 2 } else { 0 };
    if width == 0 { 0 } else { width.wrapping_add(unsafe { engine.bias() }) }
}
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_builtin_parse_line(state: u32, line: u32, length: i32, header: u32, glyph: u32, mode: u32, keyword_out: *mut u32) -> i32 {
    let mode = match mode { 0 => ParseMode::Header, 1 => ParseMode::Glyph, _ => return -1 };
    let mut kind = Keyword::StartFont;
    let result = unsafe { parse_line(BdfState(Address(state)),Address(line),length,Address(header),Address(glyph),mode,&mut kind) };
    if length > 0 && result != -1 { unsafe { write_volatile(keyword_out,keyword_number(kind)) }; }
    result
}
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_builtin_init(state: u32, data: u32, size: i32, storage: u32, owner: u32) -> i32 {
    if size < 2048 { return -6; }
    let state = BdfState(Address(state));
    unsafe { state.0.put(owner); state.0.at(60).put(storage); state.0.at(64).put(storage.wrapping_add(512)) };
    let mut next = TextOffset(0);
    if unsafe { scan_header(state,Address(data),&mut next) } < 0 { return -3; }
    let table = Address(storage.wrapping_add(768));
    let capacity = (size as u32).wrapping_sub(768);
    unsafe { state.0.at(48).put(table.0); state.0.at(52).put(capacity); state.0.at(56).put(data) };
    let count = unsafe { index_glyphs(state,Address(data),next,table,capacity) };
    if count < 0 { count } else if count as u32 == unsafe { state.0.at(44).word() } { 0 } else { -3 }
}
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_builtin_scan_header(state: u32, data: u32, next: *mut u32) -> i32 {
    let mut offset = TextOffset(0);
    let result = unsafe { scan_header(BdfState(Address(state)),Address(data),&mut offset) };
    unsafe { write_volatile(next,offset.0 as u32) }; result
}
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_builtin_scan_glyph(state: u32, data: u32, offset: u32, metrics: *mut GlyphMetrics, next: *mut u32) -> i32 {
    let mut after = TextOffset(offset as usize);
    let result = unsafe { scan_glyph(BdfState(Address(state)),Address(data),TextOffset(offset as usize),&mut *metrics,&mut after) };
    unsafe { write_volatile(next,after.0 as u32) }; result
}
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_builtin_index_glyphs(state: u32, data: u32, offset: u32, table: u32, capacity: u32) -> i32 { unsafe { index_glyphs(BdfState(Address(state)),Address(data),TextOffset(offset as usize),Address(table),capacity) } }
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_builtin_thai_metric(output: u32, line: u32, length: u32, name: u32, name_length: u32, value_offset: u32) {
    if !matches!(name_length,8 | 9 | 10) { return; }
    if let Some(property) = bitmap::thai_property(unsafe { Address(line).bytes(length as usize) },unsafe { Address(name).bytes(name_length as usize) },value_offset as usize) { unsafe { write_thai(Address(output),Address(name),property) }; }
}
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_builtin_properties(state: u32, output: u32) -> i32 {
    let state = BdfState(Address(state));
    let mut source = unsafe { RawLines::new(state,state.resource(),TextOffset(0)) };
    let mut tokens = RawTokens::new(unsafe { state.token_buffer() });
    match bitmap::properties(&mut source,&mut tokens, |property| unsafe { write_thai(Address(output),state.token_buffer(),property) }) { Ok(()) => 0, Err(BitmapError::MissingLineScratch) => -1, Err(_) => -2 }
}
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_builtin_parse_text_lines(state: u32, output: u32, data: u32, offset: u32, count: i32, next: *mut u32, consumed: *mut u32) {
    let state = BdfState(Address(state));
    let mut source = unsafe { RawLines::new(state,Address(data),TextOffset(offset as usize)) };
    let mut tokens = RawTokens::new(unsafe { state.token_buffer() });
    match bitmap::property_lines(&mut source,&mut tokens,TextOffset(offset as usize),count, |property| unsafe { write_thai(Address(output),state.token_buffer(),property) }) {
        Ok(progress) => { if let Some(after) = progress.next { unsafe { write_volatile(next,after.0 as u32) }; } unsafe { write_volatile(consumed,progress.consumed as u32) }; },
        Err(_) => unsafe { write_volatile(consumed,0) },
    }
}
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_builtin_decode_bitmap(state: u32, metrics: *const GlyphMetrics, destination: u32, stride: i32, y: i32) -> i32 { unsafe { decode_bitmap(BdfState(Address(state)),&(*metrics).record(),Address(destination),stride,y) } }
#[no_mangle]
pub extern "C" fn xm1_pipeline_image_font_builtin_top_overhang(_state: u32) -> i32 { 0 }
#[no_mangle]
pub extern "C" fn xm1_pipeline_image_font_icon_top_overhang(_state: u32) -> i32 { 0 }
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_builtin_stroke_padding(_state: u32, first: *mut i32, second: *mut i32, third: *mut i32, fourth: *mut i32) -> i32 { for output in [first,second,third,fourth] { if !output.is_null() { unsafe { write_volatile(output,0) }; } } 0 }
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_icon_stroke_padding(_state: u32, first: *mut i32, second: *mut i32, third: *mut i32, fourth: *mut i32) -> i32 { for output in [first,second,third,fourth] { unsafe { write_volatile(output,0) }; } 0 }

#[derive(Clone, Copy)]
struct DrawFlags(u32);
impl DrawFlags {
    fn packed(self) -> bool { self.0 & 0x100 != 0 }
    unsafe fn icon_mode(self, engine: Engine) -> DrawMode {
        if self.packed() { DrawMode::Packed }
        else if self.0 & 0x200 != 0 { DrawMode::Coverage(if unsafe { engine.0.at(0x105c).word() } == 1 && unsafe { engine.0.at(0x1074).word() } == 0 { CoverageInk::Gray4 } else { CoverageInk::Gray8 }) }
        else { DrawMode::Suppressed }
    }
}
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_draw_icon_bitmap(state: u32, code: u16, width: i32, height: i32, x: i32, y: i32, stride: i32, pixels: u32, extra: u32, format: u32) -> i32 {
    let state = IconState(Address(state));
    let Some(glyph) = (unsafe { state.glyph(GlyphCode(code)) }) else { return 0; };
    let source_width = unsafe { glyph.byte() } as i32;
    let mut rows = unsafe { glyph.at(1).byte() } as i32;
    if extra != 0 && unsafe { Address(extra).at(24).word() } == 1 { rows = unsafe { Address(extra).at(12).word() } as i32; }
    let engine = unsafe { state.engine() };
    let translation = unsafe { engine.origin() };
    let origin = Origin { x: x.wrapping_add(translation.x), y: y.wrapping_add(translation.y) };
    if bitmap::blend_icon(&RawBytes(glyph.at(2)),source_width,rows,BitmapGeometry { width, height, stride },origin,unsafe { DrawFlags(format).icon_mode(engine) },&mut RawPixels(Address(pixels))).is_err() { return 0; }
    source_width.wrapping_add(unsafe { engine.bias() })
}
unsafe fn draw_text(state: BdfState, code: GlyphCode, target: BitmapGeometry, pixels: Address, origin: Origin, extra: Address, flags: DrawFlags) -> i32 {
    let engine = unsafe { state.engine() };
    let workspace = unsafe { engine.0.at(24).pointer().at(0x70).pointer() };
    if unsafe { state.glyph_offset(code) }.is_none() { return 0; }
    let Some(glyph) = (unsafe { metric_glyph(state,code) }) else {
        if unsafe { engine.0.at(32).word() } as i32 > 0 { return unsafe { xm1_pipeline_image_font_draw_icon_bitmap(engine.0.at(100).word(),code.0,target.width,target.height,origin.x,origin.y,target.stride,pixels.0,extra.0,flags.0) }; }
        return 0;
    };
    let baseline = unsafe { state.0.at(14).signed_half().wrapping_add(state.0.at(18).signed_half()) };
    let translation = unsafe { engine.origin() };
    let glyph_origin = Origin { x: origin.x.wrapping_add(translation.x).wrapping_add(glyph.bounds[2] as i32), y: origin.y.wrapping_add(translation.y).wrapping_add(baseline).wrapping_sub(glyph.bounds[3] as i32).wrapping_sub(glyph.bounds[1] as i32) };
    if unsafe { engine.0.at(16).pointer().at(12).word() } == 1 && glyph_origin.x == 0 {
        if unsafe { decode_bitmap(state,&glyph,pixels,target.stride,glyph_origin.y) } < 0 { return 0; }
        return glyph.advance[0] as i32;
    }
    let sum = glyph.bounds[0] as i32 + 7;
    let source_stride = (if sum < 0 { sum + 7 } else { sum }) >> 3;
    let required = baseline.wrapping_mul(source_stride);
    if (unsafe { workspace.at(12).word() } as i32) < required { return 0; }
    let source = unsafe { workspace.at(16).pointer() };
    unsafe { fill(source,required as u32) };
    if unsafe { decode_bitmap(state,&glyph,source,source_stride,0) } < 0 { return 0; }
    if flags.packed() {
        let mut output = RawPixels(pixels);
        if bitmap::blend_bdf(&RawBytes(source),source_stride,&glyph,target,glyph_origin,&mut output).is_err() { return 0; }
        if extra.0 != 0 {
            let origin = Origin { x: (unsafe { extra.word() } as i32).wrapping_add(translation.x), y: (unsafe { extra.at(4).word() } as i32).wrapping_add(translation.y) };
            unsafe { extra.put(origin.x as u32); extra.at(4).put(origin.y as u32) };
            let rectangle = ClearRectangle { origin, width: unsafe { extra.at(16).word() } as i32, height: unsafe { extra.at(20).word() } as i32, clear: unsafe { extra.at(24).word() } == 1 };
            if bitmap::clear_rectangle(rectangle,target,&mut output).is_err() { return 0; }
        }
    }
    glyph.advance[0] as i32
}
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_draw_text_bitmap(state: u32, code: u16, width: i32, height: i32, x: i32, y: i32, stride: i32, pixels: u32, extra: u32, format: u32) -> i32 { unsafe { draw_text(BdfState(Address(state)),GlyphCode(code),BitmapGeometry { width, height, stride },Address(pixels),Origin { x, y },Address(extra),DrawFlags(format)) } }
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_builtin_read_text_line(data: u32, offset: u32, next: *mut u32, length: *mut u32, output: u32, copy: u32) -> i32 { unsafe { text_line(Address(data),TextOffset(offset as usize),next,length,Address(output),copy != 0) } }
#[no_mangle]
pub extern "C" fn xm1_pipeline_image_font_builtin_storage_size() -> i32 { 0xb00 }
#[no_mangle]
pub extern "C" fn xm1_pipeline_image_font_builtin_destroy() -> i32 { 0 }
#[no_mangle]
pub extern "C" fn xm1_pipeline_image_font_builtin_dimensions() -> i32 { 0 }
#[no_mangle]
pub extern "C" fn xm1_pipeline_image_font_icon_storage_size(_mode: u32, _first: i32, _second: i32, _third: i32) -> i32 { 0 }
#[no_mangle]
pub extern "C" fn xm1_pipeline_image_font_icon_destroy() -> i32 { 0 }
#[no_mangle]
pub extern "C" fn xm1_pipeline_image_font_icon_dimensions() -> i32 { 0 }
#[no_mangle]
pub extern "C" fn xm1_pipeline_image_font_icon_weights() -> i32 { -1 }
