pub mod bitmap;
pub mod outline;
pub mod raster;
pub mod resource;
pub mod scanlines;
pub mod raster_vm;
pub mod shaping;

pub use resource::{AssetOffset, FontAsset, FontAssetKind};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GlyphId(u32);
impl GlyphId {
    pub const fn new(value: u32) -> Self { Self(value) }
    pub const fn get(self) -> u32 { self.0 }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PixelExtent(u32);
impl PixelExtent {
    pub const fn new(value: u32) -> Self { Self(value) }
    pub const fn get(self) -> u32 { self.0 }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PixelOffset(i32);
impl PixelOffset {
    pub const fn new(value: i32) -> Self { Self(value) }
    pub const fn get(self) -> i32 { self.0 }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ByteStride(usize);
impl ByteStride {
    pub const fn new(value: usize) -> Self { Self(value) }
    pub const fn get(self) -> usize { self.0 }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ByteLength(usize);
impl ByteLength {
    pub const fn new(value: usize) -> Self { Self(value) }
    pub const fn get(self) -> usize { self.0 }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Opcode(u8);
impl Opcode {
    pub const fn new(value: u8) -> Self { Self(value) }
    pub const fn get(self) -> u8 { self.0 }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FontUnit(i32);
impl FontUnit {
    pub const fn new(value: i32) -> Self { Self(value) }
    pub const fn get(self) -> i32 { self.0 }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AdvancePerMille(i16);
impl AdvancePerMille {
    pub const fn new(value: i16) -> Self { Self(value) }
    pub const fn get(self) -> i16 { self.0 }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GlyphAdvance { FullEm, PerMille(AdvancePerMille) }
impl GlyphAdvance {
    pub fn pixels(self, extent: PixelExtent) -> PixelOffset {
        let value = match self {
            Self::FullEm => extent.get() as i32,
            Self::PerMille(value) => (extent.get() as i32).wrapping_mul(i32::from(value.get())).wrapping_add(999) / 1000,
        };
        PixelOffset::new(i32::from(value as i16))
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FontPoint { pub x: FontUnit, pub y: FontUnit }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutlineCommand {
    MoveTo(FontPoint), LineTo(FontPoint), QuadraticTo { control: FontPoint, end: FontPoint }, Close,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OutlineMetrics { pub program_origin: FontPoint, pub source_advance: GlyphAdvance }
pub struct GlyphOutline<'work> { pub commands: &'work [OutlineCommand], pub metrics: OutlineMetrics }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FontSize { pub width: PixelExtent, pub height: PixelExtent }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WritingMode { Horizontal, Vertical }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CoverageEncoding { PackedMsb1, Gray4, Gray8 }
impl CoverageEncoding {
    fn row_bytes(self, width: PixelExtent) -> Result<usize, FontError> {
        let width = usize::try_from(width.get()).map_err(|_| FontError::InvalidSize)?;
        match self {
            Self::PackedMsb1 => width.checked_add(7).map(|value| value / 8).ok_or(FontError::InvalidSize),
            Self::Gray4 | Self::Gray8 => Ok(width),
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FontConfig { pub size: FontSize, pub coverage: CoverageEncoding, pub writing: WritingMode }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GlyphMetrics {
    pub width: PixelExtent, pub height: PixelExtent,
    pub advance_x: PixelOffset, pub advance_y: PixelOffset,
    pub bearing_x: PixelOffset, pub bearing_y: PixelOffset,
}
pub struct GlyphMask<'work> {
    pixels: &'work [u8], width: PixelExtent, height: PixelExtent,
    stride: ByteStride, encoding: CoverageEncoding, metrics: GlyphMetrics,
}
impl<'work> GlyphMask<'work> {
    pub fn new(pixels: &'work [u8], width: PixelExtent, height: PixelExtent, stride: ByteStride, encoding: CoverageEncoding, metrics: GlyphMetrics) -> Result<Self, FontError> {
        let mask = Self::from_renderer(pixels, width, height, stride, encoding, metrics)?;
        if encoding == CoverageEncoding::Gray4 && !mask.pixels.is_empty() {
            for row in mask.pixels.chunks(stride.get()) {
                if row.iter().take(width.get() as usize).any(|sample| *sample > 15) { return Err(FontError::InvalidBitmap); }
            }
        }
        Ok(mask)
    }
    pub const fn pixels(&self) -> &'work [u8] { self.pixels }
    pub const fn width(&self) -> PixelExtent { self.width }
    pub const fn height(&self) -> PixelExtent { self.height }
    pub const fn stride(&self) -> ByteStride { self.stride }
    pub const fn encoding(&self) -> CoverageEncoding { self.encoding }
    pub const fn metrics(&self) -> GlyphMetrics { self.metrics }
    /// Gray4 active samples must be in 0..=15; stride padding is unrestricted.
    pub(crate) fn from_renderer(pixels: &'work [u8], width: PixelExtent, height: PixelExtent, stride: ByteStride, encoding: CoverageEncoding, metrics: GlyphMetrics) -> Result<Self, FontError> {
        if width != metrics.width || height != metrics.height { return Err(FontError::InvalidBitmap); }
        let rows = usize::try_from(height.get()).map_err(|_| FontError::InvalidSize)?;
        let row_bytes = encoding.row_bytes(width)?;
        let needed = if rows == 0 || row_bytes == 0 { 0 } else {
            if stride.get() < row_bytes { return Err(FontError::InvalidBitmap); }
            (rows - 1).checked_mul(stride.get()).and_then(|prefix| prefix.checked_add(row_bytes)).ok_or(FontError::InvalidSize)?
        };
        if pixels.len() < needed { return Err(FontError::workspace(needed, pixels.len())); }
        Ok(Self { pixels: &pixels[..needed], width, height, stride, encoding, metrics })
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FontError {
    InvalidSignature, UnsupportedFormat, Truncated, InvalidDirectory, InvalidBitmap, InvalidSize,
    MissingGlyph(GlyphId), InsufficientWorkspace { required: ByteLength, provided: ByteLength },
    InvalidInstruction(Opcode), DivisionByZero { numerator: i32 },
    UninitializedRasterState,
}
impl FontError {
    pub const fn workspace(required: usize, provided: usize) -> Self {
        Self::InsufficientWorkspace { required: ByteLength::new(required), provided: ByteLength::new(provided) }
    }
}

pub enum FontWorkspace<'work> {
    Bitmap { decoded: &'work mut [u8] },
    Fdl {
        raster: &'work mut raster::RasterWorkspace,
        pixels: &'work mut [u8],
    },
}
impl<'work> FontWorkspace<'work> {
    pub fn bitmap(decoded: &'work mut [u8]) -> Self { Self::Bitmap { decoded } }
    pub fn fdl(raster: &'work mut raster::RasterWorkspace, pixels: &'work mut [u8]) -> Self {
        Self::Fdl { raster, pixels }
    }
}
pub struct FontEngine<'asset, 'work> {
    asset: FontAsset<'asset>, config: FontConfig, workspace: FontWorkspace<'work>,
}
impl<'asset, 'work> FontEngine<'asset, 'work> {
    pub fn new(asset: FontAsset<'asset>, config: FontConfig, workspace: FontWorkspace<'work>) -> Result<Self, FontError> {
        let compatible = matches!((&asset.backend, &workspace),
            (resource::FontBackend::Fdl(_), FontWorkspace::Fdl { .. })
            | (resource::FontBackend::Bdf(_), FontWorkspace::Bitmap { .. })
            | (resource::FontBackend::Icon(_), FontWorkspace::Bitmap { .. }));
        if !compatible { return Err(FontError::UnsupportedFormat); }
        if matches!(&asset.backend, resource::FontBackend::Fdl(_)) && (config.size.width.get() > 400 || config.size.height.get() > 400) { return Err(FontError::InvalidSize); }
        if matches!(&asset.backend, resource::FontBackend::Bdf(_)) && config.coverage != CoverageEncoding::PackedMsb1 { return Err(FontError::UnsupportedFormat); }
        if !matches!(&asset.backend, resource::FontBackend::Fdl(_)) && config.writing != WritingMode::Horizontal { return Err(FontError::UnsupportedFormat); }
        Ok(Self { asset, config, workspace })
    }
    pub const fn config(&self) -> FontConfig { self.config }
    pub fn glyph_mask(&mut self, glyph: GlyphId) -> Result<GlyphMask<'_>, FontError> {
        match (&self.asset.backend, &mut self.workspace) {
            (resource::FontBackend::Bdf(font), FontWorkspace::Bitmap { decoded }) => font.render(glyph, self.config, decoded),
            (resource::FontBackend::Icon(font), FontWorkspace::Bitmap { decoded }) => font.render(glyph, self.config, decoded),
            (resource::FontBackend::Fdl(font), FontWorkspace::Fdl { raster, pixels }) => raster::render_glyph(font, glyph, self.config, raster, pixels),
            _ => Err(FontError::UnsupportedFormat),
        }
    }
}
