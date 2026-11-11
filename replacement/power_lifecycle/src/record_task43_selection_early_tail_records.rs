use crate::record_address_space::{ByteCount, GuestAddress, MemoryError};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SelectionFrame { base: GuestAddress, fp: GuestAddress }

impl SelectionFrame {
    pub const LENGTH: ByteCount = ByteCount(0x600);
    pub fn new(base: GuestAddress) -> Result<Self, MemoryError> { Ok(Self { base, fp: base.checked_add(Self::LENGTH.0)? }) }
    pub fn slot(self, below_fp: u32) -> Result<GuestAddress, MemoryError> {
        let address = self.fp.get().checked_sub(below_fp).ok_or(MemoryError::AddressOverflow)?;
        if address < self.base.get() { return Err(MemoryError::AddressOverflow); }
        Ok(GuestAddress::new(address))
    }
    pub fn geometry_frame(self) -> Result<Self, MemoryError> { Ok(Self { base: self.base, fp: self.slot(0x500)? }) }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SelectionGeometry { pub x: u32, pub y: u32, pub width: u32, pub height: u32 }

pub fn projected_geometry(width: u32, height: u32, horizontal: u32, vertical: u32) -> SelectionGeometry {
    SelectionGeometry {
        x: height.wrapping_mul(116).wrapping_div(1000).wrapping_mul(horizontal.wrapping_sub(3)).wrapping_add(height >> 1),
        y: width.wrapping_mul(116).wrapping_div(1000).wrapping_mul(vertical.wrapping_sub(3)).wrapping_add(width >> 1),
        width,
        height,
    }
}
