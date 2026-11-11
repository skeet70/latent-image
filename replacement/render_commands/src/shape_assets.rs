use super::resources::{BitmapEntry, BitmapKind, DataAddress, ResourceError, ResourceMemory};
use super::PanelPoint;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ShapeError {
    Truncated,
    Overflow,
    Magic,
    NonFinite,
    CoordinateCount,
    InvalidSegment(u8),
    InvalidState,
    PaintIndex,
    RampIndex,
    DashCount,
    NativeBitmap,
    Resource(ResourceError),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Recolour { mappings: [ColourMapping; 3] }
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ColourMapping { pub source: [Scalar; 4], pub replacement: [Scalar; 4] }
impl Recolour {
    pub fn from_palette(palette: super::commands::ShapePalette) -> Self {
        use super::commands::ShapeSecondarySource;
        let primary = rgba_components(palette.primary);
        let secondary = rgba_components(palette.secondary);
        let transparent = [Scalar(0.0); 4];
        Self { mappings: [
            ColourMapping { source: [Scalar(0.0), Scalar(0.0), Scalar(0.0), Scalar(1.0)], replacement: primary },
            ColourMapping {
                source: [Scalar(1.0), Scalar(0.0), Scalar(0.0), Scalar(1.0)],
                replacement: if palette.secondary_source == ShapeSecondarySource::Red { secondary } else { transparent },
            },
            ColourMapping {
                source: [Scalar(1.0); 4],
                replacement: if palette.secondary_source == ShapeSecondarySource::White { secondary } else { transparent },
            },
        ] }
    }
    fn parse(bytes: &[u8]) -> Result<Self, ShapeError> {
        let mut cursor = Cursor::new(bytes);
        let mut mappings = [ColourMapping { source: [Scalar(0.0); 4], replacement: [Scalar(0.0); 4] }; 3];
        for mapping in &mut mappings {
            for component in &mut mapping.source { *component = Scalar::read(cursor.take(4)?)?; }
            for component in &mut mapping.replacement { *component = Scalar::read(cursor.take(4)?)?; }
        }
        Ok(Self { mappings })
    }
    pub fn map(self, colour: [Scalar; 4]) -> [Scalar; 4] {
        self.mappings.iter().find(|mapping| mapping.source == colour).map_or(colour, |mapping| mapping.replacement)
    }
}

fn rgba_components(colour: super::commands::Rgba) -> [Scalar; 4] {
    [colour.red, colour.green, colour.blue, colour.alpha].map(|component| Scalar(f32::from(component) / 255.0))
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ShapeContent<'a> {
    Gradient { file: Gvff<'a>, recolour: Option<Recolour> },
    Styled(StyledPaths<'a>),
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShapeAsset<'a> { pub entry: BitmapEntry, pub content: ShapeContent<'a> }
impl<'a> ShapeAsset<'a> {
    pub fn decode(entry: BitmapEntry, memory: &ResourceMemory<'a>) -> Result<Self, ShapeError> {
        if entry.kind == BitmapKind::NativeRgba { return Err(ShapeError::NativeBitmap); }
        let file = Gvff::parse(memory.region_tail(entry.data).map_err(ShapeError::Resource)?)?;
        Ok(Self { entry, content: ShapeContent::Gradient { file, recolour: None } })
    }
    pub fn from_request(entry: BitmapEntry, request: &[u8], memory: &ResourceMemory<'a>) -> Result<Self, ShapeError> {
        let content = match ShapeRequest::parse(entry, request)? {
            ShapeRequest::Gradient { data, recolour } => {
                let file = Gvff::parse(memory.region_tail(data).map_err(ShapeError::Resource)?)?;
                let recolour = if recolour {
                    Some(Recolour::parse(request.get(20..116).ok_or(ShapeError::Truncated)?)?)
                } else { None };
                ShapeContent::Gradient { file, recolour }
            }
            ShapeRequest::Styled { segments, values, segment_bytes } => {
                ShapeContent::Styled(StyledPaths::parse(
                    memory.bytes(segments, segment_bytes as usize).map_err(ShapeError::Resource)?,
                    memory.region_tail(values).map_err(ShapeError::Resource)?,
                )?)
            }
        };
        Ok(Self { entry, content })
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NaturalPlacement { x_offset: f64, y_offset: f64 }
impl NaturalPlacement {
    pub fn natural(origin: PanelPoint, bounds: [Scalar; 4]) -> Self {
        Self {
            x_offset: f64::from(origin.x()) - f64::from(bounds[0].value()),
            y_offset: f64::from(origin.y()) + f64::from(bounds[1].value()),
        }
    }
    pub fn styled(origin: PanelPoint, entry: BitmapEntry) -> Self {
        Self {
            x_offset: f64::from(origin.x()),
            y_offset: f64::from(origin.y()) + f64::from(entry.height),
        }
    }
    pub fn x_offset(self) -> f64 { self.x_offset }
    pub fn y_offset(self) -> f64 { self.y_offset }
    pub fn y_sign(self) -> f64 { -1.0 }
    pub fn map_x(self, local: f64) -> f64 { self.x_offset + local }
    pub fn map_y(self, local: f64) -> f64 { self.y_offset - local }
    pub fn unmap_x(self, output: f64) -> f64 { output - self.x_offset }
    pub fn unmap_y(self, output: f64) -> f64 { self.y_offset - output }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Scalar(f32);
impl Scalar {
    pub fn value(self) -> f32 { self.0 }
    fn read(bytes: &[u8]) -> Result<Self, ShapeError> {
        let value = f32::from_bits(word(bytes, 0)?);
        if value.is_finite() { Ok(Self(value)) } else { Err(ShapeError::NonFinite) }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FloatWords<'a>(&'a [u8]);
impl<'a> FloatWords<'a> {
    fn new(bytes: &'a [u8]) -> Result<Self, ShapeError> {
        if bytes.len() % 4 != 0 { return Err(ShapeError::CoordinateCount); }
        for value in bytes.chunks_exact(4) { Scalar::read(value)?; }
        Ok(Self(bytes))
    }
    pub fn len(self) -> usize { self.0.len() / 4 }
    pub fn is_empty(self) -> bool { self.0.is_empty() }
    pub fn get(self, index: usize) -> Option<Scalar> {
        let start = index.checked_mul(4)?;
        Scalar::read(self.0.get(start..start.checked_add(4)?)?).ok()
    }
    pub fn iter(self) -> impl ExactSizeIterator<Item = Scalar> + 'a {
        self.0.chunks_exact(4).map(|bytes| Scalar(f32::from_le_bytes(bytes.try_into().unwrap())))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CoordinateMode { Absolute, Relative }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SegmentKind {
    Close, Move, Line, Horizontal, Vertical, Quadratic, Cubic,
    SmoothQuadratic, SmoothCubic, SmallCounterClockwiseArc, SmallClockwiseArc,
    LargeCounterClockwiseArc, LargeClockwiseArc,
}
impl SegmentKind {
    fn decode(byte: u8) -> Result<Self, ShapeError> {
        match byte & !1 {
            0 => Ok(Self::Close), 2 => Ok(Self::Move), 4 => Ok(Self::Line),
            6 => Ok(Self::Horizontal), 8 => Ok(Self::Vertical), 10 => Ok(Self::Quadratic),
            12 => Ok(Self::Cubic), 14 => Ok(Self::SmoothQuadratic), 16 => Ok(Self::SmoothCubic),
            18 => Ok(Self::SmallCounterClockwiseArc), 20 => Ok(Self::SmallClockwiseArc),
            22 => Ok(Self::LargeCounterClockwiseArc), 24 => Ok(Self::LargeClockwiseArc),
            _ => Err(ShapeError::InvalidSegment(byte)),
        }
    }
    fn coordinates(self) -> usize {
        match self {
            Self::Close => 0, Self::Horizontal | Self::Vertical => 1,
            Self::Move | Self::Line | Self::SmoothQuadratic => 2,
            Self::Quadratic | Self::SmoothCubic => 4, Self::Cubic => 6,
            Self::SmallCounterClockwiseArc | Self::SmallClockwiseArc |
            Self::LargeCounterClockwiseArc | Self::LargeClockwiseArc => 5,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Segment<'a> { pub kind: SegmentKind, pub mode: CoordinateMode, pub coordinates: FloatWords<'a> }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Encoding { OpenVg, Styled }
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Path<'a> { segments: &'a [u8], coordinates: FloatWords<'a>, encoding: Encoding }
impl<'a> Path<'a> {
    fn new(segments: &'a [u8], coordinates: &'a [u8], encoding: Encoding) -> Result<Self, ShapeError> {
        let mut count = 0usize;
        for &segment in segments {
            count = count.checked_add(decode_segment(segment, encoding)?.0.coordinates()).ok_or(ShapeError::Overflow)?;
        }
        let coordinates = FloatWords::new(coordinates)?;
        if count != coordinates.len() { return Err(ShapeError::CoordinateCount); }
        Ok(Self { segments, coordinates, encoding })
    }
    pub fn segments(self) -> impl ExactSizeIterator<Item = Segment<'a>> + 'a {
        let mut offset = 0;
        self.segments.iter().map(move |&byte| {
            let (kind, mode) = decode_segment(byte, self.encoding).unwrap();
            let end = offset + kind.coordinates() * 4;
            let coordinates = FloatWords(&self.coordinates.0[offset..end]);
            offset = end;
            Segment { kind, mode, coordinates }
        })
    }
}
fn decode_segment(byte: u8, encoding: Encoding) -> Result<(SegmentKind, CoordinateMode), ShapeError> {
    match encoding {
        Encoding::OpenVg => Ok((SegmentKind::decode(byte)?, if byte & 1 == 0 { CoordinateMode::Absolute } else { CoordinateMode::Relative })),
        Encoding::Styled => Ok((match byte {
            b'M' => SegmentKind::Move, b'L' => SegmentKind::Line,
            b'C' => SegmentKind::Cubic, b'E' => SegmentKind::Close,
            _ => return Err(ShapeError::InvalidSegment(byte)),
        }, CoordinateMode::Absolute)),
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FillRule { EvenOdd, NonZero }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LineCap { Butt, Round, Square }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LineJoin { Miter, Round, Bevel }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PaintMode { None, Stroke, Fill, StrokeAndFill }
impl PaintMode {
    fn parse(raw: u32) -> Result<Self, ShapeError> {
        match raw { 0 => Ok(Self::None), 1 => Ok(Self::Stroke), 2 => Ok(Self::Fill), 3 => Ok(Self::StrokeAndFill), _ => Err(ShapeError::InvalidState) }
    }
    fn stroke(self) -> bool { matches!(self, Self::Stroke | Self::StrokeAndFill) }
    fn fill(self) -> bool { matches!(self, Self::Fill | Self::StrokeAndFill) }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PaintIndex(u32);
impl PaintIndex { pub fn value(self) -> u32 { self.0 } }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RampIndex(u32);
impl RampIndex { pub fn value(self) -> u32 { self.0 } }
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Paint<'a> {
    Solid { rgba: FloatWords<'a> },
    Linear { endpoints: FloatWords<'a>, ramp: RampIndex },
    Radial { circle: FloatWords<'a>, ramp: RampIndex },
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ramp<'a> { stops: FloatWords<'a> }
impl<'a> Ramp<'a> {
    pub fn stops(self) -> impl ExactSizeIterator<Item = RampStop> + 'a {
        self.stops.0.chunks_exact(20).map(|bytes| RampStop {
            offset: Scalar::read(bytes).unwrap(),
            rgba: core::array::from_fn(|index| Scalar::read(&bytes[4 + index * 4..]).unwrap()),
        })
    }
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RampStop { pub offset: Scalar, pub rgba: [Scalar; 4] }
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StrokeDash<'a> { pub phase: Scalar, pub lengths: FloatWords<'a> }
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GradientItem<'a> {
    pub mode: PaintMode,
    pub fill_rule: FillRule,
    pub cap: LineCap,
    pub join: LineJoin,
    pub stroke_width: Scalar,
    pub miter_limit: Scalar,
    pub source_over: bool,
    pub stroke: Option<PaintIndex>,
    pub fill: Option<PaintIndex>,
    pub dash: Option<StrokeDash<'a>>,
    pub path: Path<'a>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Gvff<'a> {
    bounds: [Scalar; 4],
    ramps: &'a [u8],
    paints: &'a [u8],
    paths: &'a [u8],
    dashes: &'a [u8],
    ramp_count: u32,
    item_count: u32,
    consumed: usize,
}
impl<'a> Gvff<'a> {
    pub fn parse(bytes: &'a [u8]) -> Result<Self, ShapeError> {
        let mut cursor = Cursor::new(bytes);
        let header = cursor.take(56)?;
        if word(header, 0)? != 0x46465647 { return Err(ShapeError::Magic); }
        let item_count = word(header, 20)?;
        let paint_count = word(header, 24)?;
        let ramp_count = word(header, 28)?;
        let dash_count = word(header, 32)?;
        let bounds = [Scalar::read(&header[40..])?, Scalar::read(&header[44..])?, Scalar::read(&header[48..])?, Scalar::read(&header[52..])?];
        let ramp_start = cursor.offset;
        for _ in 0..ramp_count {
            let count = word(cursor.take(4)?, 0)?;
            FloatWords::new(cursor.take(product(count, 20)?)?)?;
        }
        let ramps = &bytes[ramp_start..cursor.offset];
        let paints = cursor.take(product(paint_count, 36)?)?;
        for record in paints.chunks_exact(36) { decode_paint(record, ramp_count)?; }
        let path_start = cursor.offset;
        let mut dash_bytes = 0;
        for _ in 0..item_count {
            let record = cursor.take(52)?;
            validate_item(record, paint_count)?;
            let index = word(record, 20)?;
            if index != 0x29a && index < dash_count {
                dash_bytes = dash_bytes.max(product(index.checked_add(1).ok_or(ShapeError::Overflow)?, 32)?);
            }
            read_path(&mut cursor, record)?;
        }
        let paths = &bytes[path_start..cursor.offset];
        let dashes = cursor.take(dash_bytes)?;
        let result = Self { bounds, ramps, paints, paths, dashes, ramp_count, item_count, consumed: cursor.offset };
        for item in result.items() { item?; }
        Ok(result)
    }
    pub fn bounds(self) -> [Scalar; 4] { self.bounds }
    pub fn consumed_bytes(self) -> usize { self.consumed }
    pub fn item_count(self) -> u32 { self.item_count }
    pub fn ramps(self) -> impl Iterator<Item = Ramp<'a>> + 'a {
        let mut cursor = Cursor::new(self.ramps);
        (0..self.ramp_count).map(move |_| {
            let count = word(cursor.take(4).unwrap(), 0).unwrap();
            Ramp { stops: FloatWords(cursor.take(product(count, 20).unwrap()).unwrap()) }
        })
    }
    pub fn paints(self) -> impl ExactSizeIterator<Item = Paint<'a>> + 'a {
        self.paints.chunks_exact(36).map(move |record| decode_paint(record, self.ramp_count).unwrap())
    }
    pub fn items(self) -> impl Iterator<Item = Result<GradientItem<'a>, ShapeError>> + 'a {
        let mut cursor = Cursor::new(self.paths);
        (0..self.item_count).map(move |_| {
            let record = cursor.take(52)?;
            let mode = PaintMode::parse(word(record, 0)?)?;
            let index = word(record, 20)?;
            let dash = if index == 0x29a || index as usize >= self.dashes.len() / 32 { None } else {
                let start = product(index, 32)?;
                let bytes = &self.dashes[start..start + 32];
                let count = word(bytes, 0)?;
                if count > 6 { return Err(ShapeError::DashCount); }
                Some(StrokeDash { phase: Scalar::read(&bytes[4..])?, lengths: FloatWords::new(&bytes[8..8 + product(count, 4)?])? })
            };
            Ok(GradientItem {
                mode, fill_rule: fill_rule(word(record, 24)?)?,
                join: line_join(word(record, 28)?)?, cap: line_cap(word(record, 32)?)?,
                stroke_width: Scalar::read(&record[36..])?, miter_limit: Scalar::read(&record[40..])?,
                source_over: word(record, 8)? != 0,
                stroke: if mode.stroke() { Some(PaintIndex(word(record, 12)?)) } else { None },
                fill: if mode.fill() { Some(PaintIndex(word(record, 16)?)) } else { None },
                dash, path: read_path(&mut cursor, record)?,
            })
        })
    }
}
fn read_path<'a>(cursor: &mut Cursor<'a>, record: &[u8]) -> Result<Path<'a>, ShapeError> {
    let count = word(record, 44)?;
    let segments = cursor.take(count as usize)?;
    // Stock 0x6dac5c..0x6dac64 adds four bytes even when the segment count is aligned.
    let padded = (count as usize & !3).checked_add(4).ok_or(ShapeError::Overflow)?;
    cursor.take(padded - count as usize)?;
    Path::new(segments, cursor.take(word(record, 48)? as usize)?, Encoding::OpenVg)
}
fn decode_paint(record: &[u8], ramp_count: u32) -> Result<Paint<'_>, ShapeError> {
    match word(record, 4)? {
        0x1b01 | 0x1b02 => {
            let ramp = word(record, 28)?;
            if ramp >= ramp_count { return Err(ShapeError::RampIndex); }
            if word(record, 4)? == 0x1b01 {
                Ok(Paint::Linear { endpoints: FloatWords::new(&record[8..24])?, ramp: RampIndex(ramp) })
            } else {
                Ok(Paint::Radial { circle: FloatWords::new(&record[8..28])?, ramp: RampIndex(ramp) })
            }
        }
        _ => Ok(Paint::Solid { rgba: FloatWords::new(&record[8..24])? }),
    }
}
fn validate_item(record: &[u8], paint_count: u32) -> Result<(), ShapeError> {
    let mode = PaintMode::parse(word(record, 0)?)?;
    if (mode.stroke() && word(record, 12)? >= paint_count) || (mode.fill() && word(record, 16)? >= paint_count) { return Err(ShapeError::PaintIndex); }
    Ok(())
}
fn fill_rule(raw: u32) -> Result<FillRule, ShapeError> {
    match raw { 0x1900 => Ok(FillRule::EvenOdd), 0x1901 => Ok(FillRule::NonZero), _ => Err(ShapeError::InvalidState) }
}
fn line_cap(raw: u32) -> Result<LineCap, ShapeError> {
    match raw { 0x1700 => Ok(LineCap::Butt), 0x1701 => Ok(LineCap::Round), 0x1702 => Ok(LineCap::Square), _ => Err(ShapeError::InvalidState) }
}
fn line_join(raw: u32) -> Result<LineJoin, ShapeError> {
    match raw { 0x1800 => Ok(LineJoin::Miter), 0x1801 => Ok(LineJoin::Round), 0x1802 => Ok(LineJoin::Bevel), _ => Err(ShapeError::InvalidState) }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ShapeRequest {
    Gradient { data: DataAddress, recolour: bool },
    Styled { segments: DataAddress, values: DataAddress, segment_bytes: u32 },
}
impl ShapeRequest {
    pub fn parse(entry: BitmapEntry, request: &[u8]) -> Result<Self, ShapeError> {
        if entry.kind == BitmapKind::NativeRgba { return Err(ShapeError::NativeBitmap); }
        let parameter = word(request, 16)?;
        if parameter == 1 || parameter == 2 {
            Ok(Self::Gradient { data: DataAddress(word(request, 8)?), recolour: parameter == 2 })
        } else {
            Ok(Self::Styled { segments: DataAddress(word(request, 8)?), values: DataAddress(word(request, 12)?), segment_bytes: word(request, 0)? })
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StyledItem<'a> {
    pub fill_rule: FillRule, pub mode: PaintMode, pub cap: LineCap, pub join: LineJoin,
    pub stroke_width: Scalar, pub miter_limit: Scalar,
    pub stroke_rgb: [Scalar; 3], pub fill_rgb: [Scalar; 3], pub path: Path<'a>,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StyledPaths<'a> { segments: &'a [u8], values: &'a [u8], item_count: usize, consumed_values: usize }
impl<'a> StyledPaths<'a> {
    pub fn parse(segments: &'a [u8], values: &'a [u8]) -> Result<Self, ShapeError> {
        let mut segment_cursor = Cursor::new(segments);
        let mut value_cursor = Cursor::new(values);
        let mut item_count = 0;
        while segment_cursor.offset < segments.len() {
            read_styled(&mut segment_cursor, &mut value_cursor)?;
            item_count += 1;
        }
        Ok(Self { segments, values: &values[..value_cursor.offset], item_count, consumed_values: value_cursor.offset })
    }
    pub fn consumed_value_bytes(self) -> usize { self.consumed_values }
    pub fn items(self) -> impl ExactSizeIterator<Item = StyledItem<'a>> + 'a {
        let mut segments = Cursor::new(self.segments);
        let mut values = Cursor::new(self.values);
        (0..self.item_count).map(move |_| read_styled(&mut segments, &mut values).unwrap())
    }
}
fn read_styled<'a>(segments: &mut Cursor<'a>, values: &mut Cursor<'a>) -> Result<StyledItem<'a>, ShapeError> {
    let header = segments.take(4)?;
    let style = values.take(36)?;
    let count = Scalar::read(&style[32..])?.value();
    if count < 0.0 || count >= 2147483648.0 { return Err(ShapeError::CoordinateCount); }
    let commands = segments.take(count as usize)?;
    let mut coordinate_count = 0usize;
    for &command in commands {
        coordinate_count = coordinate_count.checked_add(decode_segment(command, Encoding::Styled)?.0.coordinates()).ok_or(ShapeError::Overflow)?;
    }
    let path = Path::new(commands, values.take(coordinate_count.checked_mul(4).ok_or(ShapeError::Overflow)?)?, Encoding::Styled)?;
    Ok(StyledItem {
        fill_rule: if header[0] == b'E' { FillRule::EvenOdd } else { FillRule::NonZero },
        mode: match (matches!(header[0], b'E' | b'F'), header[1] == b'S') {
            (false, false) => PaintMode::None, (false, true) => PaintMode::Stroke,
            (true, false) => PaintMode::Fill, (true, true) => PaintMode::StrokeAndFill,
        },
        cap: match header[2] { b'B' => LineCap::Butt, b'R' => LineCap::Round, b'S' => LineCap::Square, _ => return Err(ShapeError::InvalidState) },
        join: match header[3] { b'M' => LineJoin::Miter, b'R' => LineJoin::Round, b'B' => LineJoin::Bevel, _ => return Err(ShapeError::InvalidState) },
        stroke_width: Scalar::read(style)?, miter_limit: Scalar::read(&style[4..])?,
        stroke_rgb: [Scalar::read(&style[8..])?, Scalar::read(&style[12..])?, Scalar::read(&style[16..])?],
        fill_rgb: [Scalar::read(&style[20..])?, Scalar::read(&style[24..])?, Scalar::read(&style[28..])?], path,
    })
}
fn word(bytes: &[u8], offset: usize) -> Result<u32, ShapeError> {
    let end = offset.checked_add(4).ok_or(ShapeError::Overflow)?;
    Ok(u32::from_le_bytes(bytes.get(offset..end).ok_or(ShapeError::Truncated)?.try_into().unwrap()))
}
fn product(count: u32, size: usize) -> Result<usize, ShapeError> { (count as usize).checked_mul(size).ok_or(ShapeError::Overflow) }
struct Cursor<'a> { bytes: &'a [u8], offset: usize }
impl<'a> Cursor<'a> {
    fn new(bytes: &'a [u8]) -> Self { Self { bytes, offset: 0 } }
    fn take(&mut self, count: usize) -> Result<&'a [u8], ShapeError> {
        let end = self.offset.checked_add(count).ok_or(ShapeError::Overflow)?;
        let result = self.bytes.get(self.offset..end).ok_or(ShapeError::Truncated)?;
        self.offset = end;
        Ok(result)
    }
}

#[cfg(test)]
#[path = "shape_assets_tests.rs"]
mod tests;
