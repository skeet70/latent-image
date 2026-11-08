#![no_std]

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
pub enum SurfaceId { Osd }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PanelPoint { x: u16, y: u16 }
impl PanelPoint {
    pub const fn new(x: u16, y: u16) -> Option<Self> {
        if x < 720 && y < 480 { Some(Self { x, y }) } else { None }
    }
    pub const fn x(self) -> u16 { self.x }
    pub const fn y(self) -> u16 { self.y }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PanelRect { origin: PanelPoint, width: u16, height: u16 }
impl PanelRect {
    pub const fn new(origin: PanelPoint, width: u16, height: u16) -> Option<Self> {
        if width != 0 && height != 0 && width <= 720 - origin.x && height <= 480 - origin.y {
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
