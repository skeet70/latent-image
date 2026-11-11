use super::{ByteStride, CoverageEncoding, FontConfig, FontError, FontSize, GlyphId, GlyphMask, GlyphMetrics, PixelExtent, PixelOffset, WritingMode};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BitmapError { Truncated, InvalidResource, InvalidDimensions, WorkspaceTooSmall, MissingGlyph, MissingBitmap, MissingEndChar, MissingLineScratch, IndexCapacity }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TextOffset(pub usize);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GlyphCode(pub u16);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BdfVersion { V21, V22 }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Keyword {
    StartFont, Comment, ContentVersion, Font, Size,
    FontBox, MetricsSet, Scalable, Advance, ScalableVertical, AdvanceVertical, Vector,
    StartProperties, EndProperties, Chars, EndFont, StartChar, Encoding, Box, Bitmap, Data, EndChar,
}

#[derive(Clone, Copy)]
pub struct Token<'a> { pub bytes: &'a [u8], pub next: usize }

pub fn token(line: &[u8], offset: usize) -> Token<'_> {
    let offset = offset.min(line.len());
    let length = line[offset..].iter().position(|byte| *byte == b' ').unwrap_or(line.len() - offset);
    let end = offset + length;
    Token { bytes: &line[offset..end], next: end + usize::from(end < line.len()) }
}

pub fn decimal(bytes: &[u8]) -> i32 {
    let negative = bytes.first() == Some(&b'-');
    let number = bytes[usize::from(negative)..].iter().fold(0i32, |number, byte| number.wrapping_mul(10).wrapping_add(*byte as i32 - 48));
    if negative { number.wrapping_neg() } else { number }
}

pub fn keyword(bytes: &[u8]) -> Keyword {
    if bytes.len() < 3 || bytes[0] < b'A' { return Keyword::Data; }
    if bytes.len() == 3 { return Keyword::Box; }
    if bytes.len() == 9 { return if bytes[5] == b'C' { Keyword::StartChar } else { Keyword::StartFont }; }
    let prefix = u32::from_be_bytes([bytes[0],bytes[1],bytes[2],bytes[3]]);
    match (bytes.len(),prefix) {
        (4,0x464f4e54) => Keyword::Font, (4,0x53495a45) => Keyword::Size,
        (5,0x43484152) => Keyword::Chars, (6,0x4249544d) => Keyword::Bitmap,
        (6,0x44574944) => Keyword::Advance, (6,0x53574944) => Keyword::Scalable,
        (7,0x434f4d4d) => Keyword::Comment, (7,0x44574944) => Keyword::AdvanceVertical,
        (7,0x454e4443) => Keyword::EndChar, (7,0x454e4446) => Keyword::EndFont,
        (7,0x53574944) => Keyword::ScalableVertical, (7,0x56564543) => Keyword::Vector,
        (8,0x454e434f) => Keyword::Encoding, (10,0x4d455452) => Keyword::MetricsSet,
        (13,0x454e4450) => Keyword::EndProperties, (14,0x434f4e54) => Keyword::ContentVersion,
        (15,0x464f4e54) => Keyword::FontBox, (15,0x53544152) => Keyword::StartProperties,
        _ => Keyword::Data,
    }
}
pub trait TokenSink {
    fn token(&mut self, token: Token<'_>);
    fn clear_name(&mut self, _length: usize) {}
}
impl<F> TokenSink for F where F: for<'a> FnMut(Token<'a>) { fn token(&mut self, token: Token<'_>) { self(token); } }

#[derive(Clone, Copy)]
pub enum LineAction<'a> {
    None, Version(BdfVersion), Size([i16; 3]), FontBox([i16; 4]), MetricsSet(i16),
    Scalable([i16; 2]), Advance([i16; 2]), ScalableVertical([i16; 2]),
    AdvanceVertical([i16; 2]), Vector([i16; 2]), Properties(i16), Chars(i32),
    Name(&'a [u8]), Encoding(GlyphCode), Box([i16; 4]),
}

pub struct ParsedLine<'a> { pub keyword: Keyword, pub action: LineAction<'a> }

pub fn parse_line<'a>(line: &'a [u8], sink: &mut impl TokenSink) -> Option<ParsedLine<'a>> {
    if line.is_empty() { return None; }
    let first = token(line,0);
    sink.token(first);
    let kind = keyword(first.bytes);
    let mut offset = first.next;
    let mut next = || {
        let value = token(line,offset);
        offset = value.next;
        sink.token(value);
        value.bytes
    };
    let action = match kind {
        Keyword::StartFont => match next() { b"2.1" => LineAction::Version(BdfVersion::V21), b"2.2" => LineAction::Version(BdfVersion::V22), _ => LineAction::None },
        Keyword::StartChar => LineAction::Name(next()),
        Keyword::Chars => LineAction::Chars(decimal(next())),
        Keyword::Size => LineAction::Size([decimal(next()) as i16,decimal(next()) as i16,decimal(next()) as i16]),
        Keyword::FontBox => LineAction::FontBox([decimal(next()) as i16,decimal(next()) as i16,decimal(next()) as i16,decimal(next()) as i16]),
        Keyword::MetricsSet => LineAction::MetricsSet(decimal(next()) as i16),
        Keyword::Scalable => LineAction::Scalable([decimal(next()) as i16,decimal(next()) as i16]),
        Keyword::Advance => LineAction::Advance([decimal(next()) as i16,decimal(next()) as i16]),
        Keyword::ScalableVertical => LineAction::ScalableVertical([decimal(next()) as i16,decimal(next()) as i16]),
        Keyword::AdvanceVertical => LineAction::AdvanceVertical([decimal(next()) as i16,decimal(next()) as i16]),
        Keyword::Vector => LineAction::Vector([decimal(next()) as i16,decimal(next()) as i16]),
        Keyword::StartProperties => LineAction::Properties(decimal(next()) as i16),
        Keyword::Encoding => LineAction::Encoding(GlyphCode(decimal(next()) as u16)),
        Keyword::Box => LineAction::Box([decimal(next()) as i16,decimal(next()) as i16,decimal(next()) as i16,decimal(next()) as i16]),
        _ => LineAction::None,
    };
    Some(ParsedLine { keyword: kind, action })
}

#[derive(Clone, Copy)]
pub struct BdfHeader {
    pub version: Option<BdfVersion>, pub size: [i16; 3], pub bounds: [i16; 4], pub metrics_set: i16,
    pub scalable: [i16; 2], pub advance: [i16; 2], pub scalable_vertical: [i16; 2],
    pub advance_vertical: [i16; 2], pub vector: [i16; 2], pub properties: i16, pub characters: i32,
}
impl BdfHeader {
    pub const EMPTY: Self = Self { version: None, size: [0; 3], bounds: [0; 4], metrics_set: 0, scalable: [0; 2], advance: [0; 2], scalable_vertical: [0; 2], advance_vertical: [0; 2], vector: [0; 2], properties: 0, characters: 0 };
    pub fn apply(&mut self, action: LineAction<'_>) {
        match action {
            LineAction::Version(value) => self.version = Some(value), LineAction::Size(value) => self.size = value,
            LineAction::FontBox(value) => self.bounds = value, LineAction::MetricsSet(value) => self.metrics_set = value,
            LineAction::Scalable(value) => self.scalable = value, LineAction::Advance(value) => self.advance = value,
            LineAction::ScalableVertical(value) => self.scalable_vertical = value, LineAction::AdvanceVertical(value) => self.advance_vertical = value,
            LineAction::Vector(value) => self.vector = value, LineAction::Properties(value) => self.properties = value,
            LineAction::Chars(value) => self.characters = value, _ => {},
        }
    }
}

#[derive(Clone, Copy)]
pub struct GlyphRecord {
    pub code: GlyphCode, pub scalable: [i16; 2], pub advance: [i16; 2],
    pub scalable_vertical: [i16; 2], pub advance_vertical: [i16; 2], pub vector: [i16; 2],
    pub bounds: [i16; 4], pub bitmap: Option<TextOffset>,
}
impl GlyphRecord {
    pub const EMPTY: Self = Self { code: GlyphCode(0), scalable: [0; 2], advance: [0; 2], scalable_vertical: [0; 2], advance_vertical: [0; 2], vector: [0; 2], bounds: [0; 4], bitmap: None };
    pub fn apply(&mut self, action: LineAction<'_>) {
        match action {
            LineAction::Encoding(value) => self.code = value, LineAction::Scalable(value) => self.scalable = value,
            LineAction::Advance(value) => self.advance = value, LineAction::ScalableVertical(value) => self.scalable_vertical = value,
            LineAction::AdvanceVertical(value) => self.advance_vertical = value, LineAction::Vector(value) => self.vector = value,
            LineAction::Box(value) => self.bounds = value, _ => {},
        }
    }
}

pub struct Line<'a> { pub bytes: &'a [u8], pub next: TextOffset }
pub trait LineSource {
    fn line(&mut self, offset: TextOffset) -> Result<Line<'_>,BitmapError>;
    fn skip_line(&mut self, offset: TextOffset) -> Result<TextOffset,BitmapError> { self.line(offset).map(|line| line.next) }
}

pub struct ResourceLines<'a> { bytes: &'a [u8] }
impl<'a> ResourceLines<'a> { pub fn new(bytes: &'a [u8]) -> Self { Self { bytes } } }
impl LineSource for ResourceLines<'_> {
    fn line(&mut self, offset: TextOffset) -> Result<Line<'_>,BitmapError> {
        let (length,next) = line_extent(self.bytes,offset)?;
        if self.bytes.get(next.0 - 1) != Some(&b'\n') { return Err(BitmapError::InvalidResource); }
        Ok(Line { bytes: &self.bytes[offset.0..offset.0 + length], next })
    }
}

pub fn line_extent(source: &(impl ByteSource + ?Sized), offset: TextOffset) -> Result<(usize,TextOffset),BitmapError> {
    let mut length = 0usize;
    while source.byte(offset.0.wrapping_add(length))? != b'\r' { length += 1; }
    Ok((length,TextOffset(offset.0.wrapping_add(length).wrapping_add(2))))
}

pub fn scan_header(source: &mut impl LineSource, header: &mut BdfHeader, sink: &mut impl TokenSink) -> Result<TextOffset,BitmapError> {
    let mut offset = TextOffset(0);
    let mut kind = Keyword::StartFont;
    loop {
        let line = source.line(offset)?;
        let next = line.next;
        if let Some(parsed) = parse_line(line.bytes,sink) { header.apply(parsed.action); kind = parsed.keyword; }
        offset = next;
        if kind == Keyword::Chars { return Ok(offset); }
        if kind == Keyword::EndFont { return Err(BitmapError::InvalidResource); }
        if kind == Keyword::StartProperties { for _ in 0..header.properties { offset = source.skip_line(offset)?; } }
    }
}

pub fn scan_glyph(source: &mut impl LineSource, mut offset: TextOffset, glyph: &mut GlyphRecord, sink: &mut impl TokenSink) -> Result<TextOffset,BitmapError> {
    let mut rows = 0i32;
    let mut kind = Keyword::StartFont;
    loop {
        let line = source.line(offset)?;
        let next = line.next;
        if let Some(parsed) = parse_line(line.bytes,sink) {
            glyph.apply(parsed.action);
            if let LineAction::Name(name) = parsed.action { sink.clear_name(name.len()); }
            kind = parsed.keyword;
        }
        match kind {
            Keyword::Bitmap => { rows = glyph.bounds[1] as i32; glyph.bitmap = Some(next); offset = next; },
            Keyword::Data => {
                if rows < 0 { return Err(BitmapError::InvalidDimensions); }
                if rows == 0 { return Err(BitmapError::InvalidResource); }
                let row_length = next.0.checked_sub(offset.0).ok_or(BitmapError::InvalidResource)?;
                let bitmap_length = (rows as usize).checked_mul(row_length).ok_or(BitmapError::Truncated)?;
                offset = TextOffset(offset.0.checked_add(bitmap_length).ok_or(BitmapError::Truncated)?);
                rows = 0;
            },
            Keyword::EndChar => return Ok(next), Keyword::EndFont => return Err(BitmapError::MissingGlyph),
            _ => offset = next,
        }
    }
}

pub fn hex_digit(byte: u8) -> u8 { if byte < 64 { byte.wrapping_sub(48) } else { (byte & 15).wrapping_add(9) } }

pub trait BitmapOutput { fn put(&mut self, offset: usize, byte: u8) -> Result<(),BitmapError>; }
impl BitmapOutput for [u8] {
    fn put(&mut self, offset: usize, byte: u8) -> Result<(),BitmapError> { *self.get_mut(offset).ok_or(BitmapError::WorkspaceTooSmall)? = byte; Ok(()) }
}

pub fn decode_bitmap(source: &mut impl LineSource, glyph: &GlyphRecord, output: &mut (impl BitmapOutput + ?Sized), stride: i32, y: i32) -> Result<(),BitmapError> {
    let mut offset = glyph.bitmap.ok_or(BitmapError::MissingBitmap)?;
    if glyph.bounds[1] < 0 { return Ok(()); }
    for row in 0..glyph.bounds[1] as i32 {
        let line = source.line(offset)?;
        for (index,pair) in line.bytes.chunks(2).enumerate() {
            let byte = hex_digit(pair[0]).wrapping_mul(16).wrapping_add(pair.get(1).map_or(0, |byte| hex_digit(*byte)));
            output.put((row.wrapping_add(y)).wrapping_mul(stride).wrapping_add(index as i32) as usize,byte)?;
        }
        offset = line.next;
    }
    if source.line(offset)?.bytes == b"ENDCHAR" { Ok(()) } else { Err(BitmapError::MissingEndChar) }
}

fn map_error(error: BitmapError) -> FontError {
    match error {
        BitmapError::Truncated => FontError::Truncated,
        BitmapError::InvalidResource | BitmapError::IndexCapacity => FontError::InvalidDirectory,
        _ => FontError::InvalidBitmap,
    }
}

fn glyph_metrics(glyph: &GlyphRecord) -> Result<GlyphMetrics,FontError> {
    let width = u32::try_from(glyph.bounds[0]).map_err(|_| FontError::InvalidBitmap)?;
    let height = u32::try_from(glyph.bounds[1]).map_err(|_| FontError::InvalidBitmap)?;
    Ok(GlyphMetrics {
        width: PixelExtent::new(width), height: PixelExtent::new(height),
        advance_x: PixelOffset::new(glyph.advance[0] as i32), advance_y: PixelOffset::new(glyph.advance[1] as i32),
        bearing_x: PixelOffset::new(glyph.bounds[2] as i32),
        bearing_y: PixelOffset::new(-(glyph.bounds[3] as i32) - height as i32),
    })
}

pub fn right_overhang(glyph: &GlyphRecord) -> i32 { (glyph.bounds[2] as i32 + glyph.bounds[0] as i32 - glyph.advance[0] as i32).max(0) }
pub fn secondary_extent(header: &BdfHeader) -> i32 { (header.bounds[0] as i32 + header.bounds[2] as i32 - header.advance[0] as i32).max(0) }

pub struct BdfResource<'a> { bytes: &'a [u8], header: BdfHeader, first_glyph: TextOffset, glyph_count: usize, thai: ThaiProperties }
impl<'a> BdfResource<'a> {
    pub fn parse(bytes: &'a [u8]) -> Result<Self,FontError> {
        if !bytes.starts_with(b"STARTFONT ") { return Err(FontError::InvalidSignature); }
        let mut source = ResourceLines::new(bytes);
        let mut header = BdfHeader::EMPTY;
        let mut sink = |_: Token<'_>| {};
        let first_glyph = scan_header(&mut source,&mut header,&mut sink).map_err(map_error)?;
        if header.version.is_none() { return Err(FontError::UnsupportedFormat); }
        if header.bounds[0] < 0 || header.bounds[1] < 0 { return Err(FontError::InvalidSize); }
        let mut offset = first_glyph;
        let mut count = 0usize;
        loop {
            let mut glyph = GlyphRecord::EMPTY;
            match scan_glyph(&mut source,offset,&mut glyph,&mut sink) {
                Ok(next) => {
                    let metrics = glyph_metrics(&glyph)?;
                    let stride = (metrics.width.get() as usize + 7) / 8;
                    let mut bitmap = glyph.bitmap.ok_or(FontError::InvalidBitmap)?;
                    for _ in 0..metrics.height.get() {
                        let line = source.line(bitmap).map_err(map_error)?;
                        if line.bytes.len().div_ceil(2) > stride { return Err(FontError::InvalidBitmap); }
                        bitmap = line.next;
                    }
                    if source.line(bitmap).map_err(map_error)?.bytes != b"ENDCHAR" { return Err(FontError::InvalidBitmap); }
                    count = count.checked_add(1).ok_or(FontError::InvalidDirectory)?;
                    offset = next;
                },
                Err(BitmapError::MissingGlyph) => break,
                Err(error) => return Err(map_error(error)),
            }
        }
        if i32::try_from(count).ok() != Some(header.characters) { return Err(FontError::InvalidDirectory); }
        let mut thai = ThaiProperties::default();
        properties(&mut source, &mut sink, |property| thai.set(property)).map_err(map_error)?;
        Ok(Self { bytes, header, first_glyph, glyph_count: count, thai })
    }

    /// The THAI_TOP, THAI_ABOVE, THAI_BASE, THAI_BELOW and THAI_SHIFT properties.
    pub const fn thai_properties(&self) -> ThaiProperties { self.thai }
    pub fn right_overhang(&self, id: GlyphId) -> Result<i32, FontError> { self.glyph_record(id).map(|glyph| right_overhang(&glyph)) }

    pub fn size(&self) -> FontSize { FontSize { width: PixelExtent::new(self.header.bounds[0] as u32), height: PixelExtent::new(self.header.bounds[1] as u32) } }
    pub fn baseline(&self) -> PixelOffset { PixelOffset::new(self.header.bounds[1] as i32 + self.header.bounds[3] as i32) }
    pub fn header(&self) -> &BdfHeader { &self.header }

    fn glyph_record(&self, id: GlyphId) -> Result<GlyphRecord,FontError> {
        let code = u16::try_from(id.get()).map_err(|_| FontError::MissingGlyph(id))?;
        let mut source = ResourceLines::new(self.bytes);
        let mut offset = self.first_glyph;
        let mut found = None;
        let mut sink = |_: Token<'_>| {};
        for _ in 0..self.glyph_count {
            let mut glyph = GlyphRecord::EMPTY;
            offset = scan_glyph(&mut source,offset,&mut glyph,&mut sink).map_err(map_error)?;
            if glyph.code.0 == code { found = Some(glyph); }
        }
        found.ok_or(FontError::MissingGlyph(id))
    }

    pub fn metrics(&self, id: GlyphId, _size: FontSize) -> Result<GlyphMetrics,FontError> {
        glyph_metrics(&self.glyph_record(id)?)
    }

    pub fn render<'m>(&'m self, id: GlyphId, config: FontConfig, decoded: &'m mut [u8]) -> Result<GlyphMask<'m>,FontError> {
        if config.coverage != CoverageEncoding::PackedMsb1 || config.writing != WritingMode::Horizontal { return Err(FontError::UnsupportedFormat); }
        let glyph = self.glyph_record(id)?;
        let metrics = glyph_metrics(&glyph)?;
        let stride = (metrics.width.get() as usize + 7) / 8;
        let required = stride.checked_mul(metrics.height.get() as usize).ok_or(FontError::InvalidBitmap)?;
        let provided = decoded.len();
        let output = decoded.get_mut(..required).ok_or(FontError::workspace(required,provided))?;
        output.fill(0);
        decode_bitmap(&mut ResourceLines::new(self.bytes),&glyph,output,stride as i32,0).map_err(map_error)?;
        GlyphMask::from_renderer(output,metrics.width,metrics.height,ByteStride::new(stride),CoverageEncoding::PackedMsb1,metrics)
    }
}

pub trait ByteSource { fn byte(&self, offset: usize) -> Result<u8,BitmapError>; }
impl ByteSource for [u8] { fn byte(&self, offset: usize) -> Result<u8,BitmapError> { self.get(offset).copied().ok_or(BitmapError::Truncated) } }

pub fn icon_glyph_offset(bytes: &(impl ByteSource + ?Sized), code: GlyphCode) -> Result<Option<usize>,BitmapError> {
    let pages = u16::from_le_bytes([bytes.byte(32)?,bytes.byte(33)?]) as usize;
    for index in 0..pages {
        if bytes.byte(34 + index * 2)? == (code.0 >> 8) as u8 {
            let slot = bytes.byte(35 + index * 2)? as usize;
            let word = 34usize.checked_add(pages * 2).and_then(|value| value.checked_add(slot * 1024)).and_then(|value| value.checked_add((code.0 as usize & 255) * 4)).ok_or(BitmapError::InvalidResource)?;
            let offset = u32::from_le_bytes([bytes.byte(word)?,bytes.byte(word + 1)?,bytes.byte(word + 2)?,bytes.byte(word + 3)?]);
            return if offset == 0 { Ok(None) } else { Ok(Some((offset as usize).checked_add(32).ok_or(BitmapError::InvalidResource)?)) };
        }
    }
    Ok(None)
}

pub struct IconResource<'a> { bytes: &'a [u8] }
impl<'a> IconResource<'a> {
    pub fn parse(bytes: &'a [u8]) -> Result<Self,FontError> {
        if bytes.get(..8) != Some(b"DCW ICON") { return Err(FontError::InvalidSignature); }
        let count = bytes.get(32..34).ok_or(FontError::Truncated)?;
        let pages = u16::from_le_bytes([count[0],count[1]]) as usize;
        for index in 0..pages {
            let entry = bytes.get(34 + index * 2..36 + index * 2).ok_or(FontError::Truncated)?;
            let directory = 34usize.checked_add(pages * 2).and_then(|value| value.checked_add(entry[1] as usize * 1024)).ok_or(FontError::InvalidDirectory)?;
            let words = bytes.get(directory..directory + 1024).ok_or(FontError::Truncated)?;
            for word in words.chunks_exact(4) {
                let offset = u32::from_le_bytes([word[0],word[1],word[2],word[3]]);
                if offset == 0 { continue; }
                let offset = (offset as usize).checked_add(32).ok_or(FontError::InvalidDirectory)?;
                let dimensions = bytes.get(offset..offset + 2).ok_or(FontError::Truncated)?;
                let length = (dimensions[0] as usize + 7) / 8 * dimensions[1] as usize;
                bytes.get(offset + 2..offset + 2 + length).ok_or(FontError::Truncated)?;
            }
        }
        Ok(Self { bytes })
    }

    pub fn size(&self) -> FontSize { FontSize { width: PixelExtent::new(self.bytes[8] as u32), height: PixelExtent::new(self.bytes[9] as u32) } }
    pub fn baseline(&self) -> PixelOffset { PixelOffset::new(0) }

    fn glyph(&self, id: GlyphId) -> Result<(usize,GlyphMetrics),FontError> {
        let code = GlyphCode(u16::try_from(id.get()).map_err(|_| FontError::MissingGlyph(id))?);
        let offset = icon_glyph_offset(self.bytes,code).map_err(map_error)?.ok_or(FontError::MissingGlyph(id))?;
        let width = PixelExtent::new(self.bytes[offset] as u32);
        let height = PixelExtent::new(self.bytes[offset + 1] as u32);
        let metrics = GlyphMetrics { width, height, advance_x: PixelOffset::new(width.get() as i32), advance_y: PixelOffset::new(0), bearing_x: PixelOffset::new(0), bearing_y: PixelOffset::new(0) };
        Ok((offset,metrics))
    }

    pub fn metrics(&self, id: GlyphId, _size: FontSize) -> Result<GlyphMetrics,FontError> { self.glyph(id).map(|(_,metrics)| metrics) }

    pub fn right_overhang(&self, id: GlyphId) -> Result<i32, FontError> {
        self.glyph(id).map(|(_, metrics)| (metrics.width.get() as i32 + metrics.bearing_x.get() - metrics.advance_x.get()).max(0))
    }

    pub fn render<'m>(&'m self, id: GlyphId, config: FontConfig, decoded: &'m mut [u8]) -> Result<GlyphMask<'m>,FontError> {
        if config.writing != WritingMode::Horizontal { return Err(FontError::UnsupportedFormat); }
        let (offset,metrics) = self.glyph(id)?;
        let stride = (metrics.width.get() as usize + 7) / 8;
        let length = stride * metrics.height.get() as usize;
        let source = &self.bytes[offset + 2..offset + 2 + length];
        let ink = match config.coverage {
            CoverageEncoding::PackedMsb1 => return GlyphMask::from_renderer(source,metrics.width,metrics.height,ByteStride::new(stride),CoverageEncoding::PackedMsb1,metrics),
            CoverageEncoding::Gray4 => CoverageInk::Gray4, CoverageEncoding::Gray8 => CoverageInk::Gray8,
        };
        let required = (metrics.width.get() as usize).checked_mul(metrics.height.get() as usize).ok_or(FontError::InvalidBitmap)?;
        let provided = decoded.len();
        let output = decoded.get_mut(..required).ok_or(FontError::workspace(required,provided))?;
        expand_packed(source,metrics.width.get() as i32,metrics.height.get() as i32,stride as i32,output,ink).map_err(map_error)?;
        GlyphMask::from_renderer(output,metrics.width,metrics.height,ByteStride::new(metrics.width.get() as usize),config.coverage,metrics)
    }
}

pub trait GlyphIndex {
    fn insert(&mut self, code: GlyphCode, offset: TextOffset) -> bool;
    fn finish(&self) -> Result<(),BitmapError>;
}

pub fn index_glyphs(source: &mut impl LineSource, mut offset: TextOffset, index: &mut impl GlyphIndex, sink: &mut impl TokenSink) -> Result<usize,BitmapError> {
    let mut count = 0usize;
    loop {
        let mut glyph = GlyphRecord::EMPTY;
        let Ok(next) = scan_glyph(source,offset,&mut glyph,sink) else { break; };
        count += 1;
        if !index.insert(glyph.code,offset) { break; }
        offset = next;
    }
    index.finish()?;
    Ok(count)
}

#[derive(Clone, Copy)]
pub enum ThaiField { Top, Above, Base, Below, Shift }
#[derive(Clone, Copy)]
pub struct ThaiProperty { pub field: ThaiField, pub value: i16 }

/// The five THAI_* properties, in the order stock writes them at 0x1190d18.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ThaiProperties { pub top: i16, pub above: i16, pub base: i16, pub below: i16, pub shift: i16 }
impl ThaiProperties {
    fn set(&mut self, property: ThaiProperty) {
        match property.field {
            ThaiField::Top => self.top = property.value,
            ThaiField::Above => self.above = property.value,
            ThaiField::Base => self.base = property.value,
            ThaiField::Below => self.below = property.value,
            ThaiField::Shift => self.shift = property.value,
        }
    }
}

pub fn thai_property(line: &[u8], name: &[u8], value_offset: usize) -> Option<ThaiProperty> {
    let field = match name { b"THAI_TOP" => ThaiField::Top, b"THAI_ABOVE" => ThaiField::Above, b"THAI_BASE" => ThaiField::Base, b"THAI_BELOW" => ThaiField::Below, b"THAI_SHIFT" => ThaiField::Shift, _ => return None };
    Some(ThaiProperty { field, value: decimal(token(line,value_offset).bytes) as i16 })
}

pub fn properties(source: &mut impl LineSource, sink: &mut impl TokenSink, mut output: impl FnMut(ThaiProperty)) -> Result<(),BitmapError> {
    let mut offset = TextOffset(0);
    let mut header = BdfHeader::EMPTY;
    let mut kind = Keyword::StartFont;
    loop {
        let line = source.line(offset)?;
        offset = line.next;
        if let Some(parsed) = parse_line(line.bytes,sink) { header.apply(parsed.action); kind = parsed.keyword; }
        if kind == Keyword::Chars || kind == Keyword::EndProperties { return Ok(()); }
        if kind == Keyword::EndFont { return Err(BitmapError::InvalidResource); }
        if kind == Keyword::StartProperties {
            for _ in 0..header.properties {
                let line = source.line(offset)?;
                offset = line.next;
                let name = token(line.bytes,0);
                sink.token(name);
                if let Some(property) = thai_property(line.bytes,name.bytes,name.next) { output(property); }
            }
        }
    }
}

pub struct LineProgress { pub next: Option<TextOffset>, pub consumed: usize }
pub fn property_lines(source: &mut impl LineSource, sink: &mut impl TokenSink, offset: TextOffset, count: i32, mut output: impl FnMut(ThaiProperty)) -> Result<LineProgress,BitmapError> {
    let mut current = offset;
    let mut next = None;
    for _ in 0..count {
        let line = source.line(current)?;
        current = line.next;
        let name = token(line.bytes,0);
        sink.token(name);
        if let Some(property) = thai_property(line.bytes,name.bytes,name.next) { output(property); }
        next = Some(current);
    }
    Ok(LineProgress { next, consumed: current.0.wrapping_sub(offset.0) })
}

#[derive(Clone, Copy)]
pub enum CoverageInk { Gray4, Gray8 }
impl CoverageInk { fn byte(self) -> u8 { match self { Self::Gray4 => 15, Self::Gray8 => 255 } } }

pub fn packed_bit(source: &(impl ByteSource + ?Sized), stride: i32, row: i32, column: i32) -> Result<bool,BitmapError> {
    Ok(source.byte(row.wrapping_mul(stride).wrapping_add(column >> 3) as usize)? & (128 >> (column & 7)) != 0)
}

pub fn expand_packed(source: &(impl ByteSource + ?Sized), width: i32, height: i32, stride: i32, output: &mut (impl BitmapOutput + ?Sized), ink: CoverageInk) -> Result<(),BitmapError> {
    for row in 0..height {
        for column in 0..width {
            output.put(row.wrapping_mul(width).wrapping_add(column) as usize,if packed_bit(source,stride,row,column)? { ink.byte() } else { 0 })?;
        }
    }
    Ok(())
}

pub trait PixelOutput: BitmapOutput { fn get(&self, offset: usize) -> Result<u8,BitmapError>; }
impl PixelOutput for [u8] { fn get(&self, offset: usize) -> Result<u8,BitmapError> { self.get(offset).copied().ok_or(BitmapError::WorkspaceTooSmall) } }

#[derive(Clone, Copy)]
pub struct BitmapGeometry { pub width: i32, pub height: i32, pub stride: i32 }
#[derive(Clone, Copy)]
pub struct Origin { pub x: i32, pub y: i32 }
#[derive(Clone, Copy)]
pub enum DrawMode { Packed, Coverage(CoverageInk), Suppressed }

pub fn blend_icon(source: &(impl ByteSource + ?Sized), source_width: i32, source_height: i32, target: BitmapGeometry, origin: Origin, mode: DrawMode, output: &mut (impl PixelOutput + ?Sized)) -> Result<(),BitmapError> {
    let source_stride = (source_width + 7) >> 3;
    let rows = source_height.min(target.height.wrapping_sub(origin.y));
    let columns = source_width.min(target.width.wrapping_sub(origin.x));
    match mode {
        DrawMode::Packed => {
            let shift = origin.x & 7;
            let bytes = columns.wrapping_add(7) >> 3;
            for row in 0..rows {
                let destination = row.wrapping_add(origin.y).wrapping_mul(target.stride).wrapping_add(if shift == 0 { origin.x } else { origin.x >> 3 });
                for index in 0..bytes {
                    let byte = source.byte(row.wrapping_mul(source_stride).wrapping_add(index) as usize)?;
                    let first = destination.wrapping_add(index) as usize;
                    output.put(first,output.get(first)? | (byte >> shift))?;
                    if shift != 0 {
                        let second = first.wrapping_add(1);
                        output.put(second,output.get(second)? | byte.wrapping_shl((8 - shift) as u32))?;
                    }
                }
            }
        },
        DrawMode::Coverage(ink) => {
            for row in 0..rows {
                for column in 0..columns {
                    let destination = row.wrapping_add(origin.y).wrapping_mul(target.stride).wrapping_add(origin.x).wrapping_add(column) as usize;
                    output.put(destination,output.get(destination)? | if packed_bit(source,source_stride,row,column)? { ink.byte() } else { 0 })?;
                }
            }
        },
        DrawMode::Suppressed => {},
    }
    Ok(())
}

pub fn blend_bdf(source: &(impl ByteSource + ?Sized), source_stride: i32, glyph: &GlyphRecord, target: BitmapGeometry, origin: Origin, output: &mut (impl PixelOutput + ?Sized)) -> Result<(),BitmapError> {
    let bits_per_row = target.stride.wrapping_mul(8);
    for row in 0..glyph.bounds[1] as i32 {
        let y = origin.y.wrapping_add(row);
        if y >= target.height { break; }
        for column in 0..glyph.bounds[0] as i32 {
            let x = origin.x.wrapping_add(column);
            if x >= bits_per_row { break; }
            let index = y.wrapping_mul(bits_per_row).wrapping_add(x) >> 3;
            let old = output.get(index as usize)?;
            output.put(index as usize,old | if packed_bit(source,source_stride,row,column)? { 1 << ((!x) & 7) } else { 0 })?;
        }
    }
    Ok(())
}

pub struct ClearRectangle { pub origin: Origin, pub width: i32, pub height: i32, pub clear: bool }
pub fn clear_rectangle(rectangle: ClearRectangle, target: BitmapGeometry, output: &mut (impl PixelOutput + ?Sized)) -> Result<(),BitmapError> {
    let bits_per_row = target.stride.wrapping_mul(8);
    let mut row = rectangle.origin.y;
    while row < target.height && row < rectangle.origin.y.wrapping_add(rectangle.height) {
        let mut column = rectangle.origin.x;
        while column < bits_per_row && column < rectangle.origin.x.wrapping_add(rectangle.width) {
            let index = row.wrapping_mul(bits_per_row).wrapping_add(column) / 8;
            let old = output.get(index as usize)?;
            output.put(index as usize,if rectangle.clear { old & !(1 << ((!column) & 7)) } else { old })?;
            column = column.wrapping_add(1);
        }
        row = row.wrapping_add(1);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{BdfResource, ByteStride, CoverageEncoding, FontConfig, FontError, FontSize, GlyphId, GlyphMask, GlyphMetrics, PixelExtent, PixelOffset, WritingMode};
    use super::super::{FontAsset, FontEngine, FontWorkspace};

    const BDF: &[u8] = b"STARTFONT 2.2\r\nFONT synthetic\r\nSIZE 8 75 75\r\nFONTBOUNDINGBOX 5 7 -1 -2\r\nSTARTPROPERTIES 2\r\nTHAI_TOP 3\r\nTHAI_BASE -2\r\nENDPROPERTIES\r\nCHARS 1\r\nSTARTCHAR A\r\nENCODING 65\r\nSWIDTH 500 0\r\nDWIDTH 4 0\r\nBBX 5 3 1 -1\r\nBITMAP\r\nA8\r\n50\r\nF8\r\nENDCHAR\r\nENDFONT\r\n";

    fn config(coverage: CoverageEncoding) -> FontConfig {
        FontConfig { size: FontSize { width: PixelExtent::new(99), height: PixelExtent::new(101) }, coverage, writing: WritingMode::Horizontal }
    }

    fn icon() -> [u8; 0xa00] {
        let mut bytes = [0; 0xa00];
        bytes[..8].copy_from_slice(b"DCW ICON");
        bytes[8..12].copy_from_slice(&[9,11,1,0]);
        bytes[32..36].copy_from_slice(&[1,0,0x12,1]);
        bytes[1268..1272].copy_from_slice(&[0,9,0,0]);
        bytes[2336..2341].copy_from_slice(&[5,3,0xa8,0x50,0xf8]);
        bytes
    }

    #[test]
    fn bdf_constructor_returns_actual_native_metrics_and_pixels() {
        let asset = FontAsset::parse(BDF).unwrap();
        let mut pixels = [0x55; 3];
        let mut font = FontEngine::new(asset,config(CoverageEncoding::PackedMsb1),FontWorkspace::bitmap(&mut pixels)).unwrap();
        let mask = font.glyph_mask(GlyphId::new(65)).unwrap();
        assert_eq!(mask.pixels(),&[0xa8,0x50,0xf8]);
        assert_eq!(mask.stride().get(),1);
        assert_eq!(mask.encoding(),CoverageEncoding::PackedMsb1);
        assert_eq!(mask.metrics(),GlyphMetrics {
            width: PixelExtent::new(5), height: PixelExtent::new(3),
            advance_x: PixelOffset::new(4), advance_y: PixelOffset::new(0),
            bearing_x: PixelOffset::new(1), bearing_y: PixelOffset::new(-2),
        });
        let resource = BdfResource::parse(BDF).unwrap();
        assert_eq!(resource.baseline(),PixelOffset::new(5));
        assert_eq!(resource.size(),FontSize { width: PixelExtent::new(5), height: PixelExtent::new(7) });
    }

    #[test]
    fn bdf_reports_workspace_and_missing_character_errors() {
        let mut pixels = [0; 2];
        let mut font = FontEngine::new(FontAsset::parse(BDF).unwrap(),config(CoverageEncoding::PackedMsb1),FontWorkspace::bitmap(&mut pixels)).unwrap();
        assert!(matches!(font.glyph_mask(GlyphId::new(65)),Err(FontError::InsufficientWorkspace { required, provided }) if required.get() == 3 && provided.get() == 2));
        assert!(matches!(font.glyph_mask(GlyphId::new(66)),Err(FontError::MissingGlyph(id)) if id == GlyphId::new(66)));
    }

    #[test]
    fn bdf_rejects_unsupported_coverage_in_actual_constructor() {
        let mut pixels = [0; 15];
        assert!(matches!(FontEngine::new(FontAsset::parse(BDF).unwrap(),config(CoverageEncoding::Gray8),FontWorkspace::bitmap(&mut pixels)),Err(FontError::UnsupportedFormat)));
    }

    #[test]
    fn dcw_icon_uses_page_slot_and_needs_no_packed_workspace() {
        let bytes = icon();
        let mut pixels = [];
        let mut font = FontEngine::new(FontAsset::parse(&bytes).unwrap(),config(CoverageEncoding::PackedMsb1),FontWorkspace::bitmap(&mut pixels)).unwrap();
        let mask = font.glyph_mask(GlyphId::new(0x1234)).unwrap();
        assert_eq!(mask.pixels(),&[0xa8,0x50,0xf8]);
        assert_eq!(mask.metrics(),GlyphMetrics {
            width: PixelExtent::new(5), height: PixelExtent::new(3),
            advance_x: PixelOffset::new(5), advance_y: PixelOffset::new(0),
            bearing_x: PixelOffset::new(0), bearing_y: PixelOffset::new(0),
        });
        assert!(matches!(font.glyph_mask(GlyphId::new(0x1235)),Err(FontError::MissingGlyph(id)) if id == GlyphId::new(0x1235)));
    }

    #[test]
    fn dcw_icon_expands_real_four_and_eight_bit_coverage() {
        let bytes = icon();
        let mut pixels = [0x77; 15];
        let mut font = FontEngine::new(FontAsset::parse(&bytes).unwrap(),config(CoverageEncoding::Gray4),FontWorkspace::bitmap(&mut pixels)).unwrap();
        let mask = font.glyph_mask(GlyphId::new(0x1234)).unwrap();
        assert_eq!(mask.pixels(),&[15,0,15,0,15,0,15,0,15,0,15,15,15,15,15]);
        assert_eq!(mask.stride().get(),5);
        assert_eq!(mask.encoding(),CoverageEncoding::Gray4);
        let mut pixels = [0x77; 15];
        let mut font = FontEngine::new(FontAsset::parse(&bytes).unwrap(),config(CoverageEncoding::Gray8),FontWorkspace::bitmap(&mut pixels)).unwrap();
        assert_eq!(font.glyph_mask(GlyphId::new(0x1234)).unwrap().pixels(),&[255,0,255,0,255,0,255,0,255,0,255,255,255,255,255]);
    }

    #[test]
    fn bitmap_constructors_reject_truncated_and_unrelated_formats() {
        let bytes = icon();
        assert!(matches!(FontAsset::parse(&bytes[..100]),Err(FontError::Truncated)));
        assert!(matches!(FontAsset::parse(b"GVFF unrelated"),Err(FontError::InvalidSignature)));
        assert!(matches!(FontAsset::parse(b"STARTFONT 2.2\r\n"),Err(FontError::Truncated)));
        assert!(matches!(FontAsset::parse(b"STARTFONT 2.2\r\nFONTBOUNDINGBOX 5 7 -1 -2\r\nCHARS 1\r\nSTARTCHAR A\r\nENCODING 65\r\nDWIDTH 4 0\r\nBBX 5 -3 1 -1\r\nBITMAP\r\nA8\r\nENDCHAR\r\nENDFONT\r\n"),Err(FontError::InvalidBitmap)));
    }

    #[test]
    fn public_gray4_masks_reject_invalid_active_samples_but_allow_stride_padding() {
        let metrics = GlyphMetrics { width: PixelExtent::new(1), height: PixelExtent::new(1), advance_x: PixelOffset::new(1), advance_y: PixelOffset::new(0), bearing_x: PixelOffset::new(0), bearing_y: PixelOffset::new(0) };
        assert!(matches!(GlyphMask::new(&[16],metrics.width,metrics.height,ByteStride::new(1),CoverageEncoding::Gray4,metrics),Err(FontError::InvalidBitmap)));
        let metrics = GlyphMetrics { height: PixelExtent::new(2), ..metrics };
        let mask = GlyphMask::new(&[15,255,0],metrics.width,metrics.height,ByteStride::new(2),CoverageEncoding::Gray4,metrics).unwrap();
        assert_eq!(mask.pixels(),&[15,255,0]);
    }
}
