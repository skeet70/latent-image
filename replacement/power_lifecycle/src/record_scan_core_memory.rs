use super::record_address_space::{ByteCount, GuestAddress, GuestAddressSpace, MemoryError};

pub(crate) trait ScanMemoryAddress {
    fn byte(self, memory: &mut impl GuestAddressSpace) -> Result<u8, MemoryError>;
    fn half(self, memory: &mut impl GuestAddressSpace) -> Result<u16, MemoryError>;
    fn word(self, memory: &mut impl GuestAddressSpace) -> Result<u32, MemoryError>;
    fn address(self, memory: &mut impl GuestAddressSpace) -> Result<GuestAddress, MemoryError>;
    fn set_byte(self, value: u8, memory: &mut impl GuestAddressSpace) -> Result<(), MemoryError>;
    fn set_half(self, value: u16, memory: &mut impl GuestAddressSpace) -> Result<(), MemoryError>;
    fn set_word(self, value: u32, memory: &mut impl GuestAddressSpace) -> Result<(), MemoryError>;
    fn copy_from(self, source: GuestAddress, count: ByteCount, memory: &mut impl GuestAddressSpace) -> Result<(), MemoryError>;
    fn fill(self, value: u8, count: ByteCount, memory: &mut impl GuestAddressSpace) -> Result<(), MemoryError>;
}

impl ScanMemoryAddress for GuestAddress {
    fn byte(self, memory: &mut impl GuestAddressSpace) -> Result<u8, MemoryError> { memory.read_u8(self) }
    fn half(self, memory: &mut impl GuestAddressSpace) -> Result<u16, MemoryError> { memory.read_u16(self) }
    fn word(self, memory: &mut impl GuestAddressSpace) -> Result<u32, MemoryError> { memory.read_u32(self) }
    fn address(self, memory: &mut impl GuestAddressSpace) -> Result<GuestAddress, MemoryError> { Ok(GuestAddress::new(memory.read_u32(self)?)) }
    fn set_byte(self, value: u8, memory: &mut impl GuestAddressSpace) -> Result<(), MemoryError> { memory.write_u8(self, value) }
    fn set_half(self, value: u16, memory: &mut impl GuestAddressSpace) -> Result<(), MemoryError> { memory.write_u16(self, value) }
    fn set_word(self, value: u32, memory: &mut impl GuestAddressSpace) -> Result<(), MemoryError> { memory.write_u32(self, value) }
    fn copy_from(self, source: GuestAddress, count: ByteCount, memory: &mut impl GuestAddressSpace) -> Result<(), MemoryError> { memory.copy_bytes(self, source, count) }
    fn fill(self, value: u8, count: ByteCount, memory: &mut impl GuestAddressSpace) -> Result<(), MemoryError> {
        for offset in 0..count.0 { memory.write_u8(self.checked_add(offset)?, value)?; }
        Ok(())
    }
}

pub(crate) const SAMPLE_STATE: GuestAddress = GuestAddress::new(0x016d_6cbc);
pub(crate) const MOTION_STATE: GuestAddress = GuestAddress::new(0x016d_7390);
