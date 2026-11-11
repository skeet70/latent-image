//! Guest memory for the record helper exports: the linked image's absolute addresses.

use crate::record_address_space::{ByteCount, GuestAddress, GuestAddressSpace, GuestDomain, MemoryError};

pub struct InstalledMemory;

impl GuestAddressSpace for InstalledMemory {
    fn domain(&self) -> GuestDomain { GuestDomain::A }
    fn read_u8(&mut self, address: GuestAddress) -> Result<u8, MemoryError> { Ok(unsafe { core::ptr::read_volatile(address.get() as *const u8) }) }
    fn read_u16(&mut self, address: GuestAddress) -> Result<u16, MemoryError> { Ok(unsafe { core::ptr::read_volatile(address.get() as *const u16) }) }
    fn read_u32(&mut self, address: GuestAddress) -> Result<u32, MemoryError> { Ok(unsafe { core::ptr::read_volatile(address.get() as *const u32) }) }
    fn write_u8(&mut self, address: GuestAddress, value: u8) -> Result<(), MemoryError> { unsafe { core::ptr::write_volatile(address.get() as *mut u8, value) }; Ok(()) }
    fn write_u16(&mut self, address: GuestAddress, value: u16) -> Result<(), MemoryError> { unsafe { core::ptr::write_volatile(address.get() as *mut u16, value) }; Ok(()) }
    fn write_u32(&mut self, address: GuestAddress, value: u32) -> Result<(), MemoryError> { unsafe { core::ptr::write_volatile(address.get() as *mut u32, value) }; Ok(()) }
    fn copy_bytes(&mut self, destination: GuestAddress, source: GuestAddress, length: ByteCount) -> Result<(), MemoryError> {
        unsafe { core::ptr::copy(source.get() as *const u8, destination.get() as *mut u8, length.0 as usize) };
        Ok(())
    }
}
