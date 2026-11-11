//! Source: x70view 55436fc85c0560dd252b0410af6f2427fe198cd3.
use crate::record_address_space::{ByteCount, GuestAddress, GuestAddressSpace, MemoryError};

#[derive(Clone, Copy)]
pub(super) struct Address(pub u32);

impl Address {
    pub(super) const fn guest(self) -> GuestAddress { GuestAddress::new(self.0) }
    pub(super) fn offset(self, bytes: u32) -> Self { Self(self.0.wrapping_add(bytes)) }
    pub(super) fn word(self, memory: &mut impl GuestAddressSpace) -> Result<u32, MemoryError> { memory.read_u32(self.guest()) }
    pub(super) fn half(self, memory: &mut impl GuestAddressSpace) -> Result<u16, MemoryError> { memory.read_u16(self.guest()) }
    pub(super) fn byte(self, memory: &mut impl GuestAddressSpace) -> Result<u8, MemoryError> { memory.read_u8(self.guest()) }
    pub(super) fn set_word(self, memory: &mut impl GuestAddressSpace, value: u32) -> Result<(), MemoryError> { memory.write_u32(self.guest(), value) }
    pub(super) fn set_half(self, memory: &mut impl GuestAddressSpace, value: u16) -> Result<(), MemoryError> { memory.write_u16(self.guest(), value) }
    pub(super) fn clear(self, memory: &mut impl GuestAddressSpace, count: ByteCount, value: u8) -> Result<(), MemoryError> {
        for offset in 0..count.0 { memory.write_u8(self.guest().checked_add(offset)?, value)?; }
        Ok(())
    }
    pub(super) fn copy_from(self, memory: &mut impl GuestAddressSpace, source: Self, count: ByteCount) -> Result<(), MemoryError> {
        memory.copy_bytes(self.guest(), source.guest(), count)
    }
}

pub(super) const SELECTION: Address = Address(0x016a_d740);
pub(super) const JOB: Address = Address(0x016b_3ddc);
pub(super) const SAVED: Address = Address(0x016a_0500);
pub(super) const ACTIVE: Address = Address(0x016a_d744);

