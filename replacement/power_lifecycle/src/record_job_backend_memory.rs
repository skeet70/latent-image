use crate::record_address_space::{ByteCount, GuestAddress, GuestAddressSpace, MemoryError};

pub(super) trait JobAddress: Copy {
    fn at(self, offset: u32) -> Result<GuestAddress, MemoryError>;
    fn word(self, offset: u32, memory: &mut impl GuestAddressSpace) -> Result<u32, MemoryError>;
    fn half(self, offset: u32, memory: &mut impl GuestAddressSpace) -> Result<u16, MemoryError>;
    fn signed(self, offset: u32, memory: &mut impl GuestAddressSpace) -> Result<i32, MemoryError>;
    fn byte(self, offset: u32, memory: &mut impl GuestAddressSpace) -> Result<u8, MemoryError>;
    fn put(self, offset: u32, value: u32, memory: &mut impl GuestAddressSpace) -> Result<(), MemoryError>;
    fn put_half(self, offset: u32, value: i32, memory: &mut impl GuestAddressSpace) -> Result<(), MemoryError>;
    fn put_byte(self, offset: u32, value: u8, memory: &mut impl GuestAddressSpace) -> Result<(), MemoryError>;
    fn clear(self, size: u32, memory: &mut impl GuestAddressSpace) -> Result<(), MemoryError>;
    fn copy(self, source: GuestAddress, size: u32, memory: &mut impl GuestAddressSpace) -> Result<(), MemoryError>;
}
impl JobAddress for GuestAddress {
    fn at(self, offset: u32) -> Result<GuestAddress, MemoryError> { self.checked_add(offset) }
    fn word(self, offset: u32, memory: &mut impl GuestAddressSpace) -> Result<u32, MemoryError> { memory.read_u32(self.at(offset)?) }
    fn half(self, offset: u32, memory: &mut impl GuestAddressSpace) -> Result<u16, MemoryError> { memory.read_u16(self.at(offset)?) }
    fn signed(self, offset: u32, memory: &mut impl GuestAddressSpace) -> Result<i32, MemoryError> { self.half(offset, memory).map(|value| value as i16 as i32) }
    fn byte(self, offset: u32, memory: &mut impl GuestAddressSpace) -> Result<u8, MemoryError> { memory.read_u8(self.at(offset)?) }
    fn put(self, offset: u32, value: u32, memory: &mut impl GuestAddressSpace) -> Result<(), MemoryError> { memory.write_u32(self.at(offset)?, value) }
    fn put_half(self, offset: u32, value: i32, memory: &mut impl GuestAddressSpace) -> Result<(), MemoryError> { memory.write_u16(self.at(offset)?, value as u16) }
    fn put_byte(self, offset: u32, value: u8, memory: &mut impl GuestAddressSpace) -> Result<(), MemoryError> { memory.write_u8(self.at(offset)?, value) }
    fn clear(self, size: u32, memory: &mut impl GuestAddressSpace) -> Result<(), MemoryError> { for offset in 0..size { memory.write_u8(self.at(offset)?, 0)?; } Ok(()) }
    fn copy(self, source: GuestAddress, size: u32, memory: &mut impl GuestAddressSpace) -> Result<(), MemoryError> { memory.copy_bytes(self, source, ByteCount(size)) }
}
