#![no_std]

pub const PANEL_WIDTH: usize = 720;
pub const PANEL_HEIGHT: usize = 480;
pub const PANEL_BYTES: usize = PANEL_WIDTH * PANEL_HEIGHT * 4;

pub mod commands;
#[path = "../../develop/src"]
mod develop_source {
    pub mod font_core;
}
pub use develop_source::font_core;
pub mod raster;
pub mod producer;
pub mod publisher;
pub mod context;
pub mod resources;
pub mod runtime;
pub mod shape_assets;
pub mod vector_raster;
pub mod actor;
pub mod actor_request;
pub mod actor_sequence;
pub use commands::{Alpha, CatalogTextId, Colour, CustomTextId, ImageId, StrokeWidth, TextRef, DrawCommand, DrawList, TextPenOp, TextX, TextY, TextAdvance, TextCoordinate, Rgba, ShapePaint, ShapePalette, ShapeSecondarySource};
pub use raster::{Surface, AssetPack, AssetProvenance, RasterError, RasterResources, FontBinding, FontRegistry, ShapeBinding, VectorWorkspace, PaintBlend, PaintCoverage};

#[cfg(test)]
mod publisher_tests;
#[cfg(test)]
mod context_tests;
#[cfg(test)]
mod runtime_tests;
#[cfg(test)]
mod script_tables_tests;
#[cfg(test)]
mod actor_tests;
#[cfg(test)]
pub(crate) mod test_buffers;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(transparent)]
pub struct NodeId(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(transparent)]
pub struct ResourceId(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(transparent)]
pub struct FontId(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(transparent)]
pub struct PaletteIndex(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(transparent)]
pub struct RowIndex(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(transparent)]
pub struct PageIndex(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SurfaceId { Osd, Stage1, Stage2, Stage3, Stage4, Stage5 }
impl SurfaceId {
    pub const fn dimensions(self) -> (usize, usize) {
        match self {
            Self::Osd | Self::Stage1 => (720, 480),
            Self::Stage2 => (360, 240),
            Self::Stage3 => (960, 544),
            Self::Stage4 => (1280, 720),
            Self::Stage5 => (640, 480),
        }
    }
    pub const fn byte_len(self) -> usize {
        let (width, height) = self.dimensions();
        width * height * 4
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PanelPoint { x: u16, y: u16 }
impl PanelPoint {
    pub const fn new(x: u16, y: u16) -> Option<Self> { Self::new_for(SurfaceId::Osd, x, y) }
    pub const fn new_for(surface: SurfaceId, x: u16, y: u16) -> Option<Self> {
        let (width, height) = surface.dimensions();
        if (x as usize) < width && (y as usize) < height { Some(Self { x, y }) } else { None }
    }
    pub const fn x(self) -> u16 { self.x }
    pub const fn y(self) -> u16 { self.y }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PanelRect { origin: PanelPoint, width: u16, height: u16 }
impl PanelRect {
    pub const fn new(origin: PanelPoint, width: u16, height: u16) -> Option<Self> {
        Self::new_for(SurfaceId::Osd, origin, width, height)
    }
    pub const fn new_for(surface: SurfaceId, origin: PanelPoint, width: u16, height: u16) -> Option<Self> {
        let (surface_width, surface_height) = surface.dimensions();
        if width != 0 && height != 0 && (origin.x as usize) < surface_width && (origin.y as usize) < surface_height
            && width as usize <= surface_width - origin.x as usize && height as usize <= surface_height - origin.y as usize {
            Some(Self { origin, width, height })
        } else { None }
    }
    pub const fn origin(self) -> PanelPoint { self.origin }
    pub const fn width(self) -> u16 { self.width }
    pub const fn height(self) -> u16 { self.height }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(transparent)]
pub struct DrawReport(pub [u8; 0x44]);
impl DrawReport {
    pub const fn zeroed() -> Self { Self([0; 0x44]) }
    pub fn put_word(&mut self, offset: usize, value: u32) {
        let bytes = value.to_le_bytes();
        self.0[offset] = bytes[0];
        self.0[offset + 1] = bytes[1];
        self.0[offset + 2] = bytes[2];
        self.0[offset + 3] = bytes[3];
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SideState { pub kind: u32, pub alternate_font: bool }
