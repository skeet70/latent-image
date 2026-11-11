//! Getters over the shared block at 0x0091_904c.
//!
//! Each body is the image-4 instruction sequence at the address in its name.

use crate::record_address_space::{GuestAddress, GuestAddressSpace, MemoryError, RecordContext};
use crate::record_helper_shared_state::SHARED_BLOCK;
use crate::record_profile_accessors::{read_header, HeaderField};

const FIELD_756F94: HeaderField = HeaderField::SignedHalfword(0x1fc);
const FIELD_7571BC: HeaderField = HeaderField::SignedHalfword(0x216);
const FIELD_75720C: HeaderField = HeaderField::Word(0x218);

fn block() -> GuestAddress { GuestAddress::new(SHARED_BLOCK) }

pub fn helper_756f94<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<u32, MemoryError> {
    read_header(context, block(), FIELD_756F94)
}

pub fn helper_7571bc<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<u32, MemoryError> {
    read_header(context, block(), FIELD_7571BC)
}

pub fn helper_75720c<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<u32, MemoryError> {
    read_header(context, block(), FIELD_75720C)
}

#[cfg(test)]
mod tests {
    //! Expected values come from the image-4 bodies at 0x00756f94-0x0075720c.
    use super::*;
    use crate::record_address_space::{ByteCount, GuestDomain, HostAddressSpace, MemoryRegion, RegionDescriptor, RegionDomain, RegionPermission};

    struct Memory { block: [u8; 0x240] }

    fn with_memory<R>(memory: &mut Memory, body: impl FnOnce(&mut RecordContext<HostAddressSpace<'_, '_>, ()>) -> R) -> R {
        let descriptor = RegionDescriptor::new(GuestAddress::new(SHARED_BLOCK), ByteCount(0x240), RegionDomain::Shared, RegionPermission::ReadWrite).unwrap();
        let mut regions = [MemoryRegion::new(descriptor, &mut memory.block).unwrap()];
        let space = HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap();
        body(&mut RecordContext::new(space, ()))
    }

    #[test]
    fn getters_read_and_sign_extend_their_fields() {
        let mut memory = Memory { block: [0; 0x240] };
        memory.block[0x1fc..0x1fe].copy_from_slice(&0xfffeu16.to_le_bytes());
        memory.block[0x216..0x218].copy_from_slice(&0x8001u16.to_le_bytes());
        memory.block[0x218..0x21c].copy_from_slice(&0x1234_5678u32.to_le_bytes());
        with_memory(&mut memory, |context| {
            assert_eq!(helper_756f94(context), Ok(0xffff_fffe));
            assert_eq!(helper_7571bc(context), Ok(0xffff_8001));
            assert_eq!(helper_75720c(context), Ok(0x1234_5678));
        });
    }

    #[test]
    fn getters_return_positive_values_unchanged() {
        let mut memory = Memory { block: [0; 0x240] };
        memory.block[0x1fc..0x1fe].copy_from_slice(&0x0123u16.to_le_bytes());
        memory.block[0x216..0x218].copy_from_slice(&0x4567u16.to_le_bytes());
        with_memory(&mut memory, |context| {
            assert_eq!(helper_756f94(context), Ok(0x123));
            assert_eq!(helper_7571bc(context), Ok(0x4567));
        });
    }
}
