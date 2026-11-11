use super::{FontId, PaletteIndex, PanelPoint, PanelRect, SurfaceId};
use super::commands::{Alpha, CatalogTextId, CustomTextId, ImageId, Colour, DrawCommand, DrawList, Rgba, ShapePaint, TextPenOp, TextRef};
use super::font_core::{ByteStride, CoverageEncoding, FontAsset, FontConfig, FontEngine, FontError, FontWorkspace, GlyphId, GlyphMask, GlyphMetrics, PixelExtent, PixelOffset};
use super::font_core::shaping::{detect_mode, detect_script_run, shape_text, CharacterCode, CharacterIndex, DirectionHint, LayoutExtent16, LayoutOffset, ScriptTables, ShapingMetrics, TextDirection, ThaiMetrics, MAX_TEXT_UNITS};
const SCRIPT_TABLES: ScriptTables<'static> = ScriptTables::firmware();
use super::shape_assets::{Gvff, NaturalPlacement, Paint, PaintIndex, PaintMode, Ramp, RampStop, Recolour, ShapeAsset, ShapeContent, ShapeError};
use super::vector_raster::{self, ClipBounds, CurveAccuracy, DashBudget, SampleGrid, StrokeMetrics, StrokePrimitive, StrokeStyle, VectorError, Vertex};

#[cfg(test)]
const BYTES: usize = super::PANEL_BYTES;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AssetProvenance<'a> {
    Original { author: &'a str },
    OpenLicensed { license: &'a str, attribution: &'a str },
    PrivateExtract { source: &'a str },
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RasterError {
    InvalidSurfaceBuffer,
    InvalidAssetPack,
    SurfaceMismatch,
    InvalidProvenance,
    DuplicateAsset,
    UnsortedAssets,
    MissingPalette(PaletteIndex),
    MissingImage(ImageId),
    MissingCatalogText(CatalogTextId),
    MissingCustomText(CustomTextId),
    MissingGlyph { font: FontId, glyph: CharacterCode },
    UnsupportedCharacter(char),
    TextPositionOverflow,
    TextPenUnset,
    TextPenOutOfBounds,
    InvalidGlyphMask,
    InvalidPaint,
    MissingFont(FontId),
    Font { font: FontId, error: FontError },
    MissingShape(ImageId),
    MissingVectorWorkspace,
    DuplicateFont(FontId),
    UnsortedFonts,
    DuplicateShape(ImageId),
    UnsortedShapes,
    AmbiguousImageOwnership(ImageId),
    Shape(ShapeError),
    Vector(VectorError),
}

pub struct FontBinding<'asset, 'work> { id: FontId, engine: FontEngine<'asset, 'work> }
impl<'asset, 'work> FontBinding<'asset, 'work> {
    pub fn new(id: FontId, bytes: &'asset [u8], config: FontConfig, workspace: FontWorkspace<'work>) -> Result<Self, RasterError> {
        let asset = FontAsset::parse(bytes).map_err(|error| RasterError::Font { font: id, error })?;
        let engine = FontEngine::new(asset, config, workspace).map_err(|error| RasterError::Font { font: id, error })?;
        Ok(Self { id, engine })
    }
    pub const fn id(&self) -> FontId { self.id }
}
pub enum FontRegistry<'asset, 'work> { Coverage8, Engines(&'work mut [FontBinding<'asset, 'work>]) }
#[derive(Clone, Copy)]
pub struct ShapeBinding<'asset> { asset: ShapeAsset<'asset> }
impl<'asset> ShapeBinding<'asset> {
    pub const fn new(asset: ShapeAsset<'asset>) -> Self { Self { asset } }
    pub const fn id(&self) -> ImageId { self.asset.entry.image }
}
pub struct VectorWorkspace<'work> {
    pub vertices: &'work mut [Vertex],
    pub strokes: &'work mut [StrokePrimitive],
    pub dash_budget: DashBudget,
    pub accuracy: CurveAccuracy,
    pub samples: SampleGrid,
}
pub struct RasterResources<'pack, 'asset, 'work> {
    pack: &'pack AssetPack<'asset>,
    fonts: FontRegistry<'asset, 'work>,
    shapes: &'pack [ShapeBinding<'asset>],
    vector: Option<VectorWorkspace<'work>>,
}
impl<'pack, 'asset, 'work> RasterResources<'pack, 'asset, 'work> {
    pub fn coverage8(pack: &'pack AssetPack<'asset>) -> Self {
        Self { pack, fonts: FontRegistry::Coverage8, shapes: &[], vector: None }
    }
    pub fn new(pack: &'pack AssetPack<'asset>, fonts: FontRegistry<'asset, 'work>, shapes: &'pack [ShapeBinding<'asset>], vector: Option<VectorWorkspace<'work>>) -> Result<Self, RasterError> {
        if let FontRegistry::Engines(bindings) = &fonts {
            for pair in bindings.windows(2) {
                if pair[0].id == pair[1].id { return Err(RasterError::DuplicateFont(pair[1].id)); }
                if pair[0].id.0 > pair[1].id.0 { return Err(RasterError::UnsortedFonts); }
            }
        }
        let mut image_position = 8;
        let mut images = core::iter::from_fn(|| {
            while image_position < pack.bytes.len() {
                let record = read_record(pack.bytes, &mut image_position).ok()?;
                if record.kind == AssetKind::Image { return Some(record.id); }
                if record.kind > AssetKind::Image { image_position = pack.bytes.len(); break; }
            }
            None
        }).peekable();
        let mut previous = None;
        for shape in shapes {
            let id = shape.id();
            if previous == Some(id) { return Err(RasterError::DuplicateShape(id)); }
            if previous.is_some_and(|previous: ImageId| previous.value() > id.value()) { return Err(RasterError::UnsortedShapes); }
            while images.peek().is_some_and(|image| *image < id.value()) { images.next(); }
            if images.peek() == Some(&id.value()) { return Err(RasterError::AmbiguousImageOwnership(id)); }
            previous = Some(id);
        }
        Ok(Self { pack, fonts, shapes, vector })
    }
}

enum FontRenderer<'borrow, 'asset, 'work> {
    Coverage8(&'borrow AssetPack<'asset>),
    Engine(&'borrow mut FontEngine<'asset, 'work>),
}
struct ShapingAdapter<'a, 'borrow, 'asset, 'work> { renderer: &'a mut FontRenderer<'borrow, 'asset, 'work>, font: FontId }
impl ShapingMetrics for ShapingAdapter<'_, '_, '_, '_> {
    fn advance(&mut self, code: CharacterCode) -> Result<LayoutOffset, FontError> {
        let advance = match self.renderer {
            FontRenderer::Coverage8(assets) => {
                let record = assets.find(AssetKind::Glyph, self.font.0, code.0 as u32).ok_or(FontError::MissingGlyph(GlyphId::new(code.0 as u32)))?;
                i32::from(record.advance)
            }
            FontRenderer::Engine(engine) => engine.advance(GlyphId::new(code.0 as u32))?.get(),
        };
        Ok(LayoutOffset(advance))
    }
    fn height(&mut self) -> Result<LayoutOffset, FontError> {
        Ok(match self.renderer {
            FontRenderer::Coverage8(_) => LayoutOffset(0),
            FontRenderer::Engine(engine) => LayoutOffset(engine.config().size.height.get() as i32),
        })
    }
    fn right_overhang(&mut self, code: CharacterCode) -> Result<LayoutOffset, FontError> {
        Ok(LayoutOffset(match self.renderer {
            FontRenderer::Coverage8(_) => 0,
            FontRenderer::Engine(engine) => engine.right_overhang(GlyphId::new(code.0 as u32))?,
        }))
    }
    /// The outline, builtin and icon stroke padding at 0x1190c54, 0x1190c5c and 0x1193fd0 is zero.
    fn stroke_padding_top(&mut self, _padding: &mut LayoutOffset) -> Result<(), FontError> { Ok(()) }
    fn thai_metrics(&mut self, output: &mut ThaiMetrics) -> Result<(), FontError> {
        if let FontRenderer::Engine(engine) = self.renderer {
            let properties = engine.thai_properties();
            *output = ThaiMetrics {
                rise: LayoutExtent16(properties.top as u16),
                reserved_first: LayoutExtent16(properties.above as u16),
                reserved_second: LayoutExtent16(properties.base as u16),
                clip_descent: LayoutExtent16(properties.below as u16),
                left_shift: LayoutExtent16(properties.shift as u16),
            };
        }
        Ok(())
    }
}
impl<'pack, 'asset, 'work> RasterResources<'pack, 'asset, 'work> {
    fn font(&mut self, id: FontId) -> Result<FontRenderer<'_, 'asset, 'work>, RasterError> {
        match &mut self.fonts {
            FontRegistry::Coverage8 => Ok(FontRenderer::Coverage8(self.pack)),
            FontRegistry::Engines(bindings) => {
                let index = bindings.binary_search_by_key(&id.0, |binding| binding.id.0).map_err(|_| RasterError::MissingFont(id))?;
                Ok(FontRenderer::Engine(&mut bindings[index].engine))
            }
        }
    }
}

#[derive(Clone, Copy, Eq, Ord, PartialEq, PartialOrd)]
enum AssetKind { Palette, Image, Glyph, CatalogText, CustomText }
#[derive(Clone, Copy)]
struct Record<'a> { kind: AssetKind, id: u32, key: u32, width: u16, height: u16, stride: u16, advance: i16, data: &'a [u8] }

/// RCP1: magic, little-endian record count, then 24-byte headers and payloads.
/// Headers contain kind:u8, reserved:[u8;3], id:u32, key:u32, width:u16,
/// height:u16, stride:u16, advance:i16, payload_length:u32, all little-endian.
/// Kinds are palette RGBA (1), image RGBA (2), glyph coverage (3), catalog UTF-8 (4), custom UTF-8 (5).
/// Records are strictly ascending by (kind, id, key).
pub struct AssetPack<'a> { bytes: &'a [u8], provenance: AssetProvenance<'a> }
impl<'a> AssetPack<'a> {
    pub fn parse(bytes: &'a [u8], provenance: AssetProvenance<'a>) -> Result<Self, RasterError> {
        let valid = match provenance {
            AssetProvenance::Original { author } => !author.trim().is_empty(),
            AssetProvenance::OpenLicensed { license, attribution } => !license.trim().is_empty() && !attribution.trim().is_empty(),
            AssetProvenance::PrivateExtract { source } => !source.trim().is_empty(),
        };
        if !valid { return Err(RasterError::InvalidProvenance); }
        if bytes.len() < 8 || &bytes[..4] != b"RCP1" { return Err(RasterError::InvalidAssetPack); }
        let count = u32::from_le_bytes(bytes[4..8].try_into().unwrap());
        let pack = Self { bytes, provenance };
        let mut position = 8;
        let mut previous = None;
        for _ in 0..count {
            let record = read_record(bytes, &mut position)?;
            validate_record(record)?;
            let key = (record.kind, record.id, record.key);
            if let Some(previous) = previous {
                if key == previous { return Err(RasterError::DuplicateAsset); }
                if key < previous { return Err(RasterError::UnsortedAssets); }
            }
            previous = Some(key);
        }
        if position != bytes.len() { return Err(RasterError::InvalidAssetPack); }
        Ok(pack)
    }
    pub const fn provenance(&self) -> AssetProvenance<'a> { self.provenance }
    fn find(&self, kind: AssetKind, id: u32, key: u32) -> Option<Record<'a>> {
        let mut position = 8;
        while position < self.bytes.len() {
            let record = read_record(self.bytes, &mut position).ok()?;
            if record.kind == kind && record.id == id && record.key == key { return Some(record); }
            if (record.kind, record.id, record.key) > (kind, id, key) { return None; }
        }
        None
    }
    fn colour(&self, colour: Colour) -> Result<Rgba, RasterError> {
        let record = self.find(AssetKind::Palette, colour.palette.0, 0).ok_or(RasterError::MissingPalette(colour.palette))?;
        Ok(Rgba { red: record.data[0], green: record.data[1], blue: record.data[2], alpha: multiply(record.data[3], colour.alpha.0) })
    }
}
fn read_record<'a>(bytes: &'a [u8], position: &mut usize) -> Result<Record<'a>, RasterError> {
    let end = position.checked_add(24).ok_or(RasterError::InvalidAssetPack)?;
    let h = bytes.get(*position..end).ok_or(RasterError::InvalidAssetPack)?;
    if h[1..4] != [0, 0, 0] { return Err(RasterError::InvalidAssetPack); }
    let kind = match h[0] {
        1 => AssetKind::Palette,
        2 => AssetKind::Image,
        3 => AssetKind::Glyph,
        4 => AssetKind::CatalogText,
        5 => AssetKind::CustomText,
        _ => return Err(RasterError::InvalidAssetPack),
    };
    let word = |offset| u32::from_le_bytes(h[offset..offset + 4].try_into().unwrap());
    let half = |offset| u16::from_le_bytes(h[offset..offset + 2].try_into().unwrap());
    let payload_end = end.checked_add(word(20) as usize).ok_or(RasterError::InvalidAssetPack)?;
    let data = bytes.get(end..payload_end).ok_or(RasterError::InvalidAssetPack)?;
    *position = payload_end;
    Ok(Record { kind, id: word(4), key: word(8), width: half(12), height: half(14), stride: half(16), advance: half(18) as i16, data })
}
fn validate_record(record: Record<'_>) -> Result<(), RasterError> {
    let scalar = record.width == 0 && record.height == 0 && record.stride == 0 && record.advance == 0 && record.key == 0;
    let valid = match record.kind {
        AssetKind::Palette => scalar && record.data.len() == 4,
        AssetKind::Image => ImageId::new(record.id).is_some() && record.width != 0 && record.height != 0
            && record.stride as usize >= record.width as usize * 4
            && (record.stride as usize).checked_mul(record.height as usize) == Some(record.data.len())
            && record.key == 0 && record.advance == 0,
        AssetKind::Glyph => char::from_u32(record.key).is_some() && record.advance >= 0
            && record.stride >= record.width
            && (record.stride as usize).checked_mul(record.height as usize) == Some(record.data.len()),
        AssetKind::CatalogText => CatalogTextId::new(record.id).is_some() && scalar && core::str::from_utf8(record.data).is_ok(),
        AssetKind::CustomText => CustomTextId::new(record.id).is_some() && scalar && core::str::from_utf8(record.data).is_ok(),
    };
    if valid { Ok(()) } else { Err(RasterError::InvalidAssetPack) }
}

#[derive(Clone, Copy)]
struct TextPen { x: i64, y: i64 }
impl From<PanelPoint> for TextPen {
    fn from(point: PanelPoint) -> Self { Self { x: i64::from(point.x()), y: i64::from(point.y()) } }
}
pub struct Surface<'a> { id: SurfaceId, width: usize, height: usize, pixels: &'a mut [u8] }
impl<'a> Surface<'a> {
    pub fn new(id: SurfaceId, pixels: &'a mut [u8]) -> Result<Self, RasterError> {
        if pixels.len() != id.byte_len() { return Err(RasterError::InvalidSurfaceBuffer); }
        let (width, height) = id.dimensions();
        Ok(Self { id, width, height, pixels })
    }
    pub fn pixels(&self) -> &[u8] { self.pixels }
    pub(crate) fn dimensions(&self) -> (usize, usize) { (self.width, self.height) }
    pub fn draw_glyph_mask(&mut self, baseline: PanelPoint, mask: &GlyphMask<'_>, colour: Rgba) -> Result<(), RasterError> {
        self.draw_mask_at(baseline.into(), mask, colour, Bounds { left: 0, top: 0, right: self.width, bottom: self.height })
    }
    fn draw_mask_at(&mut self, baseline: TextPen, mask: &GlyphMask<'_>, colour: Rgba, clip: Bounds) -> Result<(), RasterError> {
        let metrics = mask.metrics();
        let left = baseline.x.checked_add(i64::from(metrics.bearing_x.get())).ok_or(RasterError::TextPositionOverflow)?;
        let top = baseline.y.checked_add(i64::from(metrics.bearing_y.get())).ok_or(RasterError::TextPositionOverflow)?;
        let right = left.checked_add(i64::from(mask.width().get())).ok_or(RasterError::TextPositionOverflow)?;
        let bottom = top.checked_add(i64::from(mask.height().get())).ok_or(RasterError::TextPositionOverflow)?;
        let visible_left = left.max(clip.left as i64).min(clip.right as i64) as usize;
        let visible_top = top.max(clip.top as i64).min(clip.bottom as i64) as usize;
        let visible_right = right.max(clip.left as i64).min(clip.right as i64) as usize;
        let visible_bottom = bottom.max(clip.top as i64).min(clip.bottom as i64) as usize;
        for y in visible_top..visible_bottom {
            let source_y = (y as i64 - top) as usize;
            let row = source_y * mask.stride().get();
            for x in visible_left..visible_right {
                let source_x = (x as i64 - left) as usize;
                let coverage = match mask.encoding() {
                    CoverageEncoding::PackedMsb1 => {
                        let byte = mask.pixels()[row + source_x / 8];
                        if byte & (0x80 >> (source_x % 8)) == 0 { 0 } else { 255 }
                    }
                    CoverageEncoding::Gray4 => mask.pixels()[row + source_x] * 17,
                    CoverageEncoding::Gray8 => mask.pixels()[row + source_x],
                };
                self.blend(x, y, Rgba { alpha: multiply(colour.alpha, coverage), ..colour });
            }
        }
        Ok(())
    }
    pub fn draw_shape(&mut self, origin: PanelPoint, asset: ShapeAsset<'_>, alpha: Alpha, paint: ShapePaint, workspace: &mut VectorWorkspace<'_>) -> Result<(), RasterError> {
        let clip = Bounds { left: 0, top: 0, right: self.width, bottom: self.height };
        self.draw_shape_clipped(origin, asset, alpha, paint, clip, workspace)
    }
    fn draw_shape_clipped(&mut self, origin: PanelPoint, asset: ShapeAsset<'_>, alpha: Alpha, paint: ShapePaint, clip: Bounds, workspace: &mut VectorWorkspace<'_>) -> Result<(), RasterError> {
        match asset.content {
            ShapeContent::Gradient { file, recolour } => {
                let recolour = match paint { ShapePaint::Original => recolour, ShapePaint::Recolour(palette) => Some(Recolour::from_palette(palette)) };
                self.draw_gvff_clipped(origin, file, recolour, alpha, clip, workspace)
            }
            ShapeContent::Styled(paths) => {
                if paint != ShapePaint::Original { return Err(RasterError::InvalidPaint); }
                if clip.left >= clip.right || clip.top >= clip.bottom { return Ok(()); }
                let clip = ClipBounds::new(clip.left as i32, clip.top as i32, clip.right as i32, clip.bottom as i32).unwrap();
                let placement = NaturalPlacement::styled(origin, asset.entry);
                for item in paths.items() {
                    if item.mode == PaintMode::None { continue; }
                    let path = vector_raster::flatten_path_placed(item.path, workspace.vertices, workspace.accuracy, placement).map_err(RasterError::Vector)?;
                    if matches!(item.mode, PaintMode::Fill | PaintMode::StrokeAndFill) {
                        let rgb = item.fill_rgb.map(|value| f64::from(value.value()));
                        let paint = PreparedPaint::Solid(rgba_from_floats([rgb[0], rgb[1], rgb[2], 1.0]));
                        let mut sink = PaintCoverage { surface: self, paint, placement, opacity: alpha, blend: PaintBlend::SourceOver };
                        vector_raster::fill(path, item.fill_rule, clip, workspace.samples, &mut sink);
                    }
                    if matches!(item.mode, PaintMode::Stroke | PaintMode::StrokeAndFill) {
                        let metrics = StrokeMetrics::new(item.stroke_width.value(), item.miter_limit.value()).map_err(RasterError::Vector)?;
                        let style = StrokeStyle { metrics, cap: item.cap, join: item.join, dash: None };
                        let rgb = item.stroke_rgb.map(|value| f64::from(value.value()));
                        let paint = PreparedPaint::Solid(rgba_from_floats([rgb[0], rgb[1], rgb[2], 1.0]));
                        let mut sink = PaintCoverage { surface: self, paint, placement, opacity: alpha, blend: PaintBlend::SourceOver };
                        vector_raster::stroke(path, style, workspace.strokes, workspace.dash_budget, clip, workspace.samples, &mut sink).map_err(RasterError::Vector)?;
                    }
                }
                Ok(())
            }
        }
    }
    pub fn draw_gvff(&mut self, origin: PanelPoint, file: Gvff<'_>, recolour: Option<Recolour>, alpha: Alpha, workspace: &mut VectorWorkspace<'_>) -> Result<(), RasterError> {
        let clip = Bounds { left: 0, top: 0, right: self.width, bottom: self.height };
        self.draw_gvff_clipped(origin, file, recolour, alpha, clip, workspace)
    }
    fn draw_gvff_clipped(&mut self, origin: PanelPoint, file: Gvff<'_>, recolour: Option<Recolour>, alpha: Alpha, clip: Bounds, workspace: &mut VectorWorkspace<'_>) -> Result<(), RasterError> {
        if clip.left >= clip.right || clip.top >= clip.bottom { return Ok(()); }
        let clip = ClipBounds::new(clip.left as i32, clip.top as i32, clip.right as i32, clip.bottom as i32).unwrap();
        let placement = NaturalPlacement::natural(origin, file.bounds());
        for item in file.items() {
            let item = item.map_err(RasterError::Shape)?;
            if item.mode == PaintMode::None { continue; }
            let path = vector_raster::flatten_path_placed(item.path, workspace.vertices, workspace.accuracy, placement).map_err(RasterError::Vector)?;
            let blend = if item.source_over { PaintBlend::SourceOver } else { PaintBlend::Source };
            if let Some(paint) = item.fill {
                let mut sink = PaintCoverage::new(self, file, paint, recolour, placement, alpha, blend)?;
                vector_raster::fill(path, item.fill_rule, clip, workspace.samples, &mut sink);
            }
            if let Some(paint) = item.stroke {
                let metrics = StrokeMetrics::new(item.stroke_width.value(), item.miter_limit.value()).map_err(RasterError::Vector)?;
                let style = StrokeStyle { metrics, cap: item.cap, join: item.join, dash: item.dash };
                let mut sink = PaintCoverage::new(self, file, paint, recolour, placement, alpha, blend)?;
                vector_raster::stroke(path, style, workspace.strokes, workspace.dash_budget, clip, workspace.samples, &mut sink).map_err(RasterError::Vector)?;
            }
        }
        Ok(())
    }
    /// Replaces the complete frame, clearing previous pixels to transparent first.
    pub fn render(&mut self, list: DrawList<'_>, resources: &mut RasterResources<'_, '_, '_>) -> Result<(), RasterError> {
        if self.id != list.surface() { return Err(RasterError::SurfaceMismatch); }
        self.pixels.fill(0);
        let extent = Bounds { left: 0, top: 0, right: self.width, bottom: self.height };
        let mut clip = extent;
        let mut pen = None;
        for command in list.commands() {
            match *command {
                DrawCommand::Clip(rect) => clip = Bounds::rect(rect).intersect(extent),
                DrawCommand::ResetClip => clip = extent,
                DrawCommand::Fill { rect, colour } => {
                    let colour = resources.pack.colour(colour)?;
                    self.fill(Bounds::rect(rect).intersect(clip), colour);
                }
                DrawCommand::Border { rect, stroke, colour } => {
                    let colour = resources.pack.colour(colour)?;
                    let bounds = Bounds::rect(rect);
                    let s = stroke.pixels() as usize;
                    let inner = Bounds { left: bounds.left.saturating_add(s), top: bounds.top.saturating_add(s), right: bounds.right.saturating_sub(s), bottom: bounds.bottom.saturating_sub(s) };
                    let clipped = bounds.intersect(clip);
                    for y in clipped.top..clipped.bottom {
                        for x in clipped.left..clipped.right {
                            if x < inner.left || x >= inner.right || y < inner.top || y >= inner.bottom { self.blend(x, y, colour); }
                        }
                    }
                }
                DrawCommand::Image { origin, image, alpha } => {
                    let image = resources.pack.find(AssetKind::Image, image.value(), 0).ok_or(RasterError::MissingImage(image))?;
                    self.bitmap(origin.x() as usize, origin.y() as usize, image, clip, alpha);
                }
                DrawCommand::Shape { origin, image, alpha, paint } => {
                    let index = resources.shapes.binary_search_by_key(&image.value(), |binding| binding.id().value()).map_err(|_| RasterError::MissingShape(image))?;
                    let asset = resources.shapes[index].asset;
                    let workspace = resources.vector.as_mut().ok_or(RasterError::MissingVectorWorkspace)?;
                    self.draw_shape_clipped(origin, asset, alpha, paint, clip, workspace)?;
                }
                DrawCommand::Glyph { origin, font, glyph, colour } => {
                    let colour = resources.pack.colour(colour)?;
                    let code = CharacterCode(u16::try_from(glyph as u32).map_err(|_| RasterError::UnsupportedCharacter(glyph))?);
                    let mut renderer = resources.font(font)?;
                    let _ = self.render_glyph(origin.into(), font, code, colour, clip, &mut renderer)?;
                }
                DrawCommand::Text { origin, text, font, colour } => {
                    pen = Some(self.text(origin.into(), text, font, colour, clip, resources)?);
                }
                DrawCommand::TextPen(operation) => {
                    pen = Some(match operation {
                        TextPenOp::Set(origin) => {
                            if origin.x() as usize >= self.width || origin.y() as usize >= self.height { return Err(RasterError::TextPositionOverflow); }
                            origin.into()
                        }
                        TextPenOp::SetX(x) => {
                            if x.value() as usize >= self.width { return Err(RasterError::TextPositionOverflow); }
                            let mut current = pen.ok_or(RasterError::TextPenUnset)?;
                            current.x = i64::from(x.value());
                            current
                        }
                        TextPenOp::SetY(y) => {
                            if y.value() as usize >= self.height { return Err(RasterError::TextPositionOverflow); }
                            let mut current = pen.ok_or(RasterError::TextPenUnset)?;
                            current.y = i64::from(y.value());
                            current
                        }
                        TextPenOp::AdvanceX { pixels, max_before } => {
                            let mut current = pen.ok_or(RasterError::TextPenUnset)?;
                            if current.x > i64::from(max_before.value()) { return Err(RasterError::TextPenOutOfBounds); }
                            current.x = current.x.checked_add(i64::from(pixels.value())).ok_or(RasterError::TextPositionOverflow)?;
                            current
                        }
                    });
                }
                DrawCommand::ContinueText { text, font, colour } => {
                    let origin = pen.ok_or(RasterError::TextPenUnset)?;
                    pen = Some(self.text(origin, text, font, colour, clip, resources)?);
                }
            }
        }
        Ok(())
    }
    fn text(&mut self, mut pen: TextPen, text: TextRef<'_>, font: FontId, colour: Colour, clip: Bounds, resources: &mut RasterResources<'_, '_, '_>) -> Result<TextPen, RasterError> {
        let assets = resources.pack;
        let colour = assets.colour(colour)?;
        let text = match text {
            TextRef::Inline(text) => text,
            TextRef::Catalog(id) => {
                let record = assets.find(AssetKind::CatalogText, id.value(), 0).ok_or(RasterError::MissingCatalogText(id))?;
                core::str::from_utf8(record.data).map_err(|_| RasterError::InvalidAssetPack)?
            }
            TextRef::Custom(id) => {
                let record = assets.find(AssetKind::CustomText, id.value(), 0).ok_or(RasterError::MissingCustomText(id))?;
                core::str::from_utf8(record.data).map_err(|_| RasterError::InvalidAssetPack)?
            }
        };
        let mut renderer = resources.font(font)?;
        let mut units = [0u16; MAX_TEXT_UNITS];
        let mut length = 0usize;
        for character in text.chars() {
            if character as u32 > 0xffff { return Err(RasterError::UnsupportedCharacter(character)); }
            let mut encoded = [0u16; 2];
            let encoded = character.encode_utf16(&mut encoded);
            if length + encoded.len() > MAX_TEXT_UNITS {
                return Err(RasterError::Font { font, error: FontError::InvalidSize });
            }
            units[length..length + encoded.len()].copy_from_slice(encoded);
            length += encoded.len();
        }
        let base = detect_mode(&units[..length]).map_or(DirectionHint(0x200e), |mode| DirectionHint(u32::from(mode.direction().0)));
        loop {
            if length == 0 { break; }
            let run = detect_script_run(&units[..length], CharacterIndex(0), base);
            let end = run.range.end.0 as usize;
            if end == 0 { break; }
            let shaped = {
                let mut metrics = ShapingAdapter { renderer: &mut renderer, font };
                shape_text(&units[..end], base, &mut metrics, &SCRIPT_TABLES)
            };
            let shaped = match shaped {
                Ok(shaped) => shaped,
                Err(FontError::MissingGlyph(id)) if matches!(&renderer, FontRenderer::Coverage8(_)) => {
                    return Err(RasterError::MissingGlyph { font, glyph: CharacterCode(id.get() as u16) });
                }
                Err(error) => return Err(RasterError::Font { font, error }),
            };
            let advance = i64::from(shaped.advance().0);
            let right_to_left = base.0 == TextDirection::RIGHT_TO_LEFT.0 as u32;
            let origin_x = if right_to_left { pen.x.checked_sub(advance).ok_or(RasterError::TextPositionOverflow)? } else { pen.x };
            for glyph in shaped.glyphs() {
                let origin = TextPen { x: origin_x + i64::from(glyph.pen.0), y: pen.y + i64::from(glyph.mark.y.0) };
                pen.y = pen.y.checked_add(self.render_glyph(origin, font, glyph.code, colour, clip, &mut renderer)?).ok_or(RasterError::TextPositionOverflow)?;
                if let Some(glyph_clip) = glyph.clip {
                    let bound = |value: i64, limit: usize| value.clamp(0, limit as i64) as usize;
                    let left = origin.x + i64::from(glyph_clip.x.0);
                    let top = origin.y + i64::from(glyph_clip.y.0);
                    let right = left + i64::from(glyph_clip.width.0);
                    let bottom = top + i64::from(glyph_clip.height.0);
                    let rectangle = Bounds { left: bound(left, self.width), top: bound(top, self.height), right: bound(right, self.width), bottom: bound(bottom, self.height) };
                    self.clear(rectangle.intersect(clip));
                }
            }
            pen.x = if right_to_left { origin_x } else { origin_x.checked_add(advance).ok_or(RasterError::TextPositionOverflow)? };
            units.copy_within(end..length, 0);
            length -= end;
        }
        Ok(pen)
    }
    fn render_glyph(&mut self, pen: TextPen, font: FontId, glyph: CharacterCode, colour: Rgba, clip: Bounds, renderer: &mut FontRenderer<'_, '_, '_>) -> Result<i64, RasterError> {
        let mask = match renderer {
            FontRenderer::Coverage8(assets) => {
                let record = assets.find(AssetKind::Glyph, font.0, glyph.0 as u32).ok_or(RasterError::MissingGlyph { font, glyph })?;
                let metrics = GlyphMetrics {
                    width: PixelExtent::new(u32::from(record.width)), height: PixelExtent::new(u32::from(record.height)),
                    advance_x: PixelOffset::new(i32::from(record.advance)), advance_y: PixelOffset::new(0),
                    bearing_x: PixelOffset::new(0), bearing_y: PixelOffset::new(0),
                };
                GlyphMask::new(record.data, metrics.width, metrics.height, ByteStride::new(record.stride as usize), CoverageEncoding::Gray8, metrics).map_err(|_| RasterError::InvalidGlyphMask)?
            }
            FontRenderer::Engine(engine) => engine.glyph_mask(GlyphId::new(glyph.0 as u32)).map_err(|error| RasterError::Font { font, error })?,
        };
        self.draw_mask_at(pen, &mask, colour, clip)?;
        Ok(i64::from(mask.metrics().advance_y.get()))
    }
    fn bitmap(&mut self, x: usize, y: usize, record: Record<'_>, clip: Bounds, alpha: Alpha) {
        let bounds = Bounds { left: x, top: y, right: x.saturating_add(record.width as usize), bottom: y.saturating_add(record.height as usize) }.intersect(clip);
        for py in bounds.top..bounds.bottom {
            for px in bounds.left..bounds.right {
                let row = (py - y) * record.stride as usize;
                let at = row + (px - x) * 4;
                let colour = Rgba { red: record.data[at], green: record.data[at + 1], blue: record.data[at + 2], alpha: multiply(record.data[at + 3], alpha.0) };
                self.blend(px, py, colour);
            }
        }
    }
    fn fill(&mut self, bounds: Bounds, colour: Rgba) {
        for y in bounds.top..bounds.bottom { for x in bounds.left..bounds.right { self.blend(x, y, colour); } }
    }
    fn clear(&mut self, bounds: Bounds) {
        for y in bounds.top..bounds.bottom {
            let row = y * self.width * 4;
            self.pixels[row + bounds.left * 4..row + bounds.right * 4].fill(0);
        }
    }
    pub(crate) fn blend(&mut self, x: usize, y: usize, source: Rgba) {
        if source.alpha == 0 { return; }
        let index = (y * self.width + x) * 4;
        let destination = &mut self.pixels[index..index + 4];
        if source.alpha == 255 { destination.copy_from_slice(&[source.red, source.green, source.blue, 255]); return; }
        let sa = source.alpha as u32;
        let da = destination[3] as u32;
        let inverse = 255 - sa;
        let denominator = sa * 255 + da * inverse;
        for (output, input) in destination[..3].iter_mut().zip([source.red, source.green, source.blue]) {
            let numerator = input as u32 * sa * 255 + *output as u32 * da * inverse;
            *output = ((numerator + denominator / 2) / denominator) as u8;
        }
        destination[3] = ((denominator + 127) / 255) as u8;
    }
    fn source_coverage(&mut self, x: usize, y: usize, source: Rgba, coverage: u8) {
        if coverage == 0 { return; }
        let index = (y * self.width + x) * 4;
        let destination = &mut self.pixels[index..index + 4];
        let source_weight = source.alpha as u32 * coverage as u32;
        let destination_weight = destination[3] as u32 * (255 - coverage as u32);
        let denominator = source_weight + destination_weight;
        if denominator == 0 { destination.fill(0); return; }
        for (output, input) in destination[..3].iter_mut().zip([source.red, source.green, source.blue]) {
            *output = ((input as u32 * source_weight + *output as u32 * destination_weight + denominator / 2) / denominator) as u8;
        }
        destination[3] = ((denominator + 127) / 255) as u8;
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PaintBlend { Source, SourceOver }

enum PreparedPaint<'a> {
    Solid(Rgba),
    Linear { x: f64, y: f64, dx: f64, dy: f64, length_squared: f64, ramp: Ramp<'a> },
    Radial { fx: f64, fy: f64, relative_x: f64, relative_y: f64, radius_squared: f64, ramp: Ramp<'a> },
}
impl<'a> PreparedPaint<'a> {
    fn new(file: Gvff<'a>, index: PaintIndex, recolour: Option<Recolour>) -> Result<Self, RasterError> {
        let paint = file.paints().nth(index.value() as usize).ok_or(RasterError::InvalidPaint)?;
        let ramp_at = |index: super::shape_assets::RampIndex| {
            let ramp = file.ramps().nth(index.value() as usize).ok_or(RasterError::InvalidPaint)?;
            let mut previous = None;
            for stop in ramp.stops() {
                let offset = stop.offset.value();
                if !(0.0..=1.0).contains(&offset) || previous.is_some_and(|previous| offset < previous) { return Err(RasterError::InvalidPaint); }
                previous = Some(offset);
            }
            if previous.is_none() { return Err(RasterError::InvalidPaint); }
            Ok(ramp)
        };
        match paint {
            Paint::Solid { rgba } => {
                let values = core::array::from_fn(|index| rgba.get(index).unwrap());
                let values = if let Some(recolour) = recolour { recolour.map(values) } else { values };
                Ok(Self::Solid(rgba_from_floats(values.map(|value| f64::from(value.value())))))
            }
            Paint::Linear { endpoints, ramp } => {
                let x = f64::from(endpoints.get(0).unwrap().value());
                let y = f64::from(endpoints.get(1).unwrap().value());
                let dx = f64::from(endpoints.get(2).unwrap().value()) - x;
                let dy = f64::from(endpoints.get(3).unwrap().value()) - y;
                Ok(Self::Linear { x, y, dx, dy, length_squared: dx * dx + dy * dy, ramp: ramp_at(ramp)? })
            }
            Paint::Radial { circle, ramp } => {
                let cx = f64::from(circle.get(0).unwrap().value());
                let cy = f64::from(circle.get(1).unwrap().value());
                let mut fx = f64::from(circle.get(2).unwrap().value());
                let mut fy = f64::from(circle.get(3).unwrap().value());
                let radius = f64::from(circle.get(4).unwrap().value());
                if radius <= 0.0 { return Err(RasterError::InvalidPaint); }
                let mut relative_x = fx - cx;
                let mut relative_y = fy - cy;
                let distance_squared = relative_x * relative_x + relative_y * relative_y;
                if distance_squared > radius * radius {
                    let scale = radius / libm::sqrt(distance_squared);
                    relative_x *= scale;
                    relative_y *= scale;
                    fx = cx + relative_x;
                    fy = cy + relative_y;
                }
                Ok(Self::Radial { fx, fy, relative_x, relative_y, radius_squared: radius * radius, ramp: ramp_at(ramp)? })
            }
        }
    }
    fn sample(&self, x: f64, y: f64) -> Rgba {
        match *self {
            Self::Solid(colour) => colour,
            Self::Linear { x: start_x, y: start_y, dx, dy, length_squared, ramp } => {
                let parameter = if length_squared == 0.0 { 1.0 } else { ((x - start_x) * dx + (y - start_y) * dy) / length_squared };
                sample_ramp(ramp, parameter)
            }
            Self::Radial { fx, fy, relative_x, relative_y, radius_squared, ramp } => {
                let vx = x - fx;
                let vy = y - fy;
                let a = vx * vx + vy * vy;
                let b = relative_x * vx + relative_y * vy;
                let c = (relative_x * relative_x + relative_y * relative_y - radius_squared).min(0.0);
                let root = -b + libm::sqrt((b * b - a * c).max(0.0));
                let parameter = if a == 0.0 { 0.0 } else if root <= 0.0 { 1.0 } else { a / root };
                sample_ramp(ramp, parameter)
            }
        }
    }
}

pub struct PaintCoverage<'surface, 'pixels, 'asset> {
    surface: &'surface mut Surface<'pixels>,
    paint: PreparedPaint<'asset>,
    placement: NaturalPlacement,
    opacity: Alpha,
    blend: PaintBlend,
}
impl<'surface, 'pixels, 'asset> PaintCoverage<'surface, 'pixels, 'asset> {
    pub fn new(surface: &'surface mut Surface<'pixels>, file: Gvff<'asset>, paint: PaintIndex, recolour: Option<Recolour>, placement: NaturalPlacement, opacity: Alpha, blend: PaintBlend) -> Result<Self, RasterError> {
        Ok(Self { surface, paint: PreparedPaint::new(file, paint, recolour)?, placement, opacity, blend })
    }
}
impl super::vector_raster::CoverageSink for PaintCoverage<'_, '_, '_> {
    fn cover(&mut self, x: i32, y: i32, coverage: u8) {
        if x < 0 || y < 0 || x as usize >= self.surface.width || y as usize >= self.surface.height || coverage == 0 { return; }
        let mut colour = self.paint.sample(self.placement.unmap_x(f64::from(x) + 0.5), self.placement.unmap_y(f64::from(y) + 0.5));
        colour.alpha = multiply(colour.alpha, self.opacity.0);
        match self.blend {
            PaintBlend::SourceOver => {
                colour.alpha = multiply(colour.alpha, coverage);
                self.surface.blend(x as usize, y as usize, colour);
            }
            PaintBlend::Source => self.surface.source_coverage(x as usize, y as usize, colour, coverage),
        }
    }
}

fn stop_premultiplied(stop: RampStop) -> [f64; 4] {
    let values = stop.rgba.map(|value| f64::from(value.value()).clamp(0.0, 1.0));
    [values[0] * values[3], values[1] * values[3], values[2] * values[3], values[3]]
}
// Paint initialization 0x840554 sets parameter 0x1a07 to one; GVFF leaves it unchanged.
fn sample_ramp(ramp: Ramp<'_>, parameter: f64) -> Rgba {
    let mut stops = ramp.stops();
    let mut left = stops.next().unwrap();
    let parameter = parameter.clamp(0.0, 1.0);
    if parameter < f64::from(left.offset.value()) { return rgba_from_floats(left.rgba.map(|value| f64::from(value.value()))); }
    for right in stops {
        if parameter < f64::from(right.offset.value()) {
            let fraction = (parameter - f64::from(left.offset.value())) / f64::from(right.offset.value() - left.offset.value());
            let left = stop_premultiplied(left);
            let right = stop_premultiplied(right);
            let mut result: [f64; 4] = core::array::from_fn(|index| left[index] + (right[index] - left[index]) * fraction);
            let alpha = result[3];
            if alpha != 0.0 { for channel in &mut result[..3] { *channel /= alpha; } }
            return rgba_from_floats(result);
        }
        left = right;
    }
    rgba_from_floats(left.rgba.map(|value| f64::from(value.value())))
}
fn rgba_from_floats(values: [f64; 4]) -> Rgba {
    let values = values.map(|value| (value.clamp(0.0, 1.0) * 255.0 + 0.5) as u8);
    Rgba { red: values[0], green: values[1], blue: values[2], alpha: values[3] }
}

fn multiply(a: u8, b: u8) -> u8 { ((a as u16 * b as u16 + 127) / 255) as u8 }
#[derive(Clone, Copy)]
struct Bounds { left: usize, top: usize, right: usize, bottom: usize }
impl Bounds {
    fn rect(rect: PanelRect) -> Self {
        Self { left: rect.origin().x() as usize, top: rect.origin().y() as usize, right: rect.origin().x() as usize + rect.width() as usize, bottom: rect.origin().y() as usize + rect.height() as usize }
    }
    fn intersect(self, other: Self) -> Self {
        Self { left: self.left.max(other.left), top: self.top.max(other.top), right: self.right.min(other.right), bottom: self.bottom.min(other.bottom) }
    }
}

#[cfg(test)]
#[path = "raster_tests.rs"]
mod tests;
