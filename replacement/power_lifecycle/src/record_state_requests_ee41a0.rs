//! Latched request bits and their acknowledgement bytes.
//!
//! Each body is the image-5 instruction sequence at the address in its name.

use crate::record_address_space::{GuestAddress, GuestAddressSpace, MemoryError, RecordContext};

const REQUESTS: u32 = 0x0169_9698;
const REQUEST_WORD: u32 = 0x14;
const FLAGS: u32 = 0x0169_8a34;

fn request<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, mask: u32, byte_offset: u32, bit: u32) -> Result<(), MemoryError> {
    let word = context.read_u32(GuestAddress::new(REQUESTS + REQUEST_WORD))?;
    context.write_u32(GuestAddress::new(REQUESTS + REQUEST_WORD), word | mask)?;
    let address = GuestAddress::new(FLAGS + byte_offset);
    let value = context.read_u8(address)?;
    context.write_u8(address, value & !(1 << bit))
}

pub fn helper_ee41a0<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<(), MemoryError> {
    request(context, 0x1, 0x10, 2)
}

pub fn helper_ee422c<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<(), MemoryError> {
    request(context, 0x40, 0x10, 6)
}

pub fn helper_ee4268<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<(), MemoryError> {
    request(context, 0x80, 0x10, 7)
}

pub fn helper_ee433c<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<(), MemoryError> {
    request(context, 0x4, 0x10, 0)
}

pub fn helper_ee4378<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<(), MemoryError> {
    request(context, 0x8, 0x10, 1)
}

pub fn helper_ee43f0<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<(), MemoryError> {
    request(context, 0x10, 0x10, 4)
}

pub fn helper_ee4550<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<(), MemoryError> {
    request(context, 0x20, 0x11, 2)
}

pub fn helper_ee4a70<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<(), MemoryError> {
    request(context, 0x400, 0x10, 3)
}

#[cfg(test)]
mod tests {
    //! Expected values come from the image-5 bodies at 0x00ee41a0-0x00ee4aa4.
    use super::*;
    use crate::record_address_space::{ByteCount, GuestDomain, HostAddressSpace, MemoryRegion, RegionDescriptor, RegionDomain, RegionPermission};

    struct Memory { requests: [u8; 0x18], flags: [u8; 0x12] }

    fn with_memory<R>(memory: &mut Memory, body: impl FnOnce(&mut RecordContext<HostAddressSpace<'_, '_>, ()>) -> R) -> R {
        let descriptor = |base, length| RegionDescriptor::new(GuestAddress::new(base), ByteCount(length), RegionDomain::Shared, RegionPermission::ReadWrite).unwrap();
        let mut regions = [
            MemoryRegion::new(descriptor(REQUESTS, 0x18), &mut memory.requests).unwrap(),
            MemoryRegion::new(descriptor(FLAGS, 0x12), &mut memory.flags).unwrap(),
        ];
        let space = HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap();
        body(&mut RecordContext::new(space, ()))
    }

    #[test]
    fn every_body_sets_its_request_and_clears_its_flag() {
        let mut memory = Memory { requests: [0; 0x18], flags: [0xff; 0x12] };
        with_memory(&mut memory, |context| {
            assert_eq!(helper_ee41a0(context), Ok(()));
            assert_eq!(helper_ee422c(context), Ok(()));
            assert_eq!(helper_ee4268(context), Ok(()));
            assert_eq!(helper_ee433c(context), Ok(()));
            assert_eq!(helper_ee4378(context), Ok(()));
            assert_eq!(helper_ee43f0(context), Ok(()));
            assert_eq!(helper_ee4550(context), Ok(()));
            assert_eq!(helper_ee4a70(context), Ok(()));
        });
        assert_eq!(&memory.requests[0x14..0x18], &(0x1 | 0x40 | 0x80 | 0x4 | 0x8 | 0x10 | 0x20 | 0x400u32).to_le_bytes());
        assert_eq!(memory.flags[0x10], 0xffu8 & !(1u8 << 2) & !(1u8 << 6) & !(1u8 << 7) & !(1u8 << 0) & !(1u8 << 1) & !(1u8 << 4) & !(1u8 << 3));
        assert_eq!(memory.flags[0x11], 0xffu8 & !(1u8 << 2));
        assert_eq!(&memory.flags[..0x10], &[0xff; 0x10]);
    }
}
