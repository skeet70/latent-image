use super::{FontId, PaletteIndex, PanelPoint, PanelRect, SurfaceId};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Rgba { pub red: u8, pub green: u8, pub blue: u8, pub alpha: u8 }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Alpha(pub u8);
impl Alpha { pub const OPAQUE: Self = Self(255); }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Colour { pub palette: PaletteIndex, pub alpha: Alpha }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StrokeWidth(u16);
impl StrokeWidth {
    pub const fn new(pixels: u16) -> Option<Self> { if pixels == 0 { None } else { Some(Self(pixels)) } }
    pub const fn pixels(self) -> u16 { self.0 }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CatalogTextId(u32);
impl CatalogTextId {
    pub const fn new(value: u32) -> Option<Self> { if value <= 0x83d { Some(Self(value)) } else { None } }
    pub const fn value(self) -> u32 { self.0 }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CustomTextId(u32);
impl CustomTextId {
    pub const fn new(value: u32) -> Option<Self> { if value <= 0x00ff_ffff { Some(Self(value)) } else { None } }
    pub const fn value(self) -> u32 { self.0 }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ImageId(u32);
impl ImageId {
    pub const fn new(value: u32) -> Option<Self> { if value <= 0x00ff_ffff { Some(Self(value)) } else { None } }
    pub const fn value(self) -> u32 { self.0 }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TextX(u16);
impl TextX {
    pub const fn new_for(surface: SurfaceId, value: u16) -> Option<Self> {
        if (value as usize) < surface.dimensions().0 { Some(Self(value)) } else { None }
    }
    pub const fn value(self) -> u16 { self.0 }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TextY(u16);
impl TextY {
    pub const fn new_for(surface: SurfaceId, value: u16) -> Option<Self> {
        if (value as usize) < surface.dimensions().1 { Some(Self(value)) } else { None }
    }
    pub const fn value(self) -> u16 { self.0 }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TextAdvance(u16);
impl TextAdvance {
    pub const fn new(value: u16) -> Self { Self(value) }
    pub const fn value(self) -> u16 { self.0 }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TextCoordinate(u16);
impl TextCoordinate {
    pub const fn new(value: u16) -> Self { Self(value) }
    pub const fn value(self) -> u16 { self.0 }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TextPenOp {
    Set(PanelPoint),
    SetX(TextX),
    SetY(TextY),
    AdvanceX { pixels: TextAdvance, max_before: TextCoordinate },
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ShapeSecondarySource { White, Red }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ShapePalette {
    pub primary: Rgba,
    pub secondary: Rgba,
    pub secondary_source: ShapeSecondarySource,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ShapePaint { Original, Recolour(ShapePalette) }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TextRef<'a> { Catalog(CatalogTextId), Custom(CustomTextId), Inline(&'a str) }
/// Destination of a scaled draw in panel pixels, top-left origin; may extend past the surface.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SceneRect { x: i32, y: i32, width: u16, height: u16 }
impl SceneRect {
    pub const fn new(x: i32, y: i32, width: u16, height: u16) -> Option<Self> {
        if width == 0 || height == 0 { None } else { Some(Self { x, y, width, height }) }
    }
    pub const fn x(self) -> i32 { self.x }
    pub const fn y(self) -> i32 { self.y }
    pub const fn width(self) -> u16 { self.width }
    pub const fn height(self) -> u16 { self.height }
}
/// Byte order of one 32-bit pixel in memory.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PixelOrder {
    Rgba,
    /// OpenVG `VG_sRGBA_8888` words (R in bits 31:24) stored little-endian.
    Abgr,
}
/// Straight-alpha 8-bit-per-channel pixels; rows top-down, `stride` bytes apart.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PixelSource<'a> { width: u16, height: u16, stride: usize, order: PixelOrder, bytes: &'a [u8] }
impl<'a> PixelSource<'a> {
    pub fn new(width: u16, height: u16, stride: usize, order: PixelOrder, bytes: &'a [u8]) -> Option<Self> {
        let row = usize::from(width) * 4;
        if width == 0 || height == 0 || stride < row { return None; }
        let needed = stride.checked_mul(usize::from(height) - 1)?.checked_add(row)?;
        if bytes.len() < needed { return None; }
        Some(Self { width, height, stride, order, bytes })
    }
    pub const fn width(self) -> u16 { self.width }
    pub const fn height(self) -> u16 { self.height }
    pub const fn stride(self) -> usize { self.stride }
    pub const fn order(self) -> PixelOrder { self.order }
    pub const fn bytes(self) -> &'a [u8] { self.bytes }
    /// The pixel at `(x, y)` as straight RGBA.
    pub fn rgba(self, x: u16, y: u16) -> Option<Rgba> {
        if x >= self.width || y >= self.height { return None; }
        let start = usize::from(y) * self.stride + usize::from(x) * 4;
        let [a, b, c, d]: [u8; 4] = self.bytes[start..start + 4].try_into().ok()?;
        Some(match self.order {
            PixelOrder::Rgba => Rgba { red: a, green: b, blue: c, alpha: d },
            PixelOrder::Abgr => Rgba { red: d, green: c, blue: b, alpha: a },
        })
    }
}
/// The 96-byte colour block of a recoloured GVFF draw: three (source RGBA, replacement RGBA) pairs of
/// little-endian f32 words, as the shape object stores it at `+0xec` (`I4:6da6a0-6da6ac`).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RecolourBlock(pub [u8; 96]);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DrawCommand<'a> {
    Fill { rect: PanelRect, colour: Colour },
    Border { rect: PanelRect, stroke: StrokeWidth, colour: Colour },
    /// Resets the text pen to origin and advances it by the rendered glyph metrics.
    Text { origin: PanelPoint, text: TextRef<'a>, font: FontId, colour: Colour },
    TextPen(TextPenOp),
    /// Starts at the preceding text pen and advances it by the rendered glyph metrics.
    ContinueText { text: TextRef<'a>, font: FontId, colour: Colour },
    Glyph { origin: PanelPoint, font: FontId, glyph: char, colour: Colour },
    Image { origin: PanelPoint, image: ImageId, alpha: Alpha },
    Shape { origin: PanelPoint, image: ImageId, alpha: Alpha, paint: ShapePaint },
    /// Draws `source` stretched onto `dest`, blended source-over within the current clip.
    ScaledImage { dest: SceneRect, source: PixelSource<'a> },
    /// Draws a GVFF block with its bounds mapped onto `dest`, recoloured when a block is given.
    ScaledShape { dest: SceneRect, gvff: &'a [u8], recolour: Option<RecolourBlock> },
    Clip(PanelRect),
    ResetClip,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DrawList<'a> { surface: SurfaceId, commands: &'a [DrawCommand<'a>] }
impl<'a> DrawList<'a> {
    pub const fn new(surface: SurfaceId, commands: &'a [DrawCommand<'a>]) -> Self { Self { surface, commands } }
    pub const fn surface(self) -> SurfaceId { self.surface }
    pub const fn commands(self) -> &'a [DrawCommand<'a>] { self.commands }
}
