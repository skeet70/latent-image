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
